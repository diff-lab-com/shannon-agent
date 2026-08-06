//! CLI plumbing for `shannon list-providers` and `shannon providers add|remove`.
//!
//! Mirrors the desktop's Add Provider / Delete Provider flows: read/write the
//! engine's `~/.shannon/providers.toml` via [`shannon_core::provider_config_store::ProviderConfigStore`].
//!
//! **Decision A1 (no plaintext):** every code path here writes a
//! `CredentialRef::Store { service }` reference. There is intentionally no
//! `--api-key <raw>` flag — if a CLI caller needs to put a secret into the
//! credential store, they use `shannon credentials` (separate command
//! surface). The CLI never accepts or persists an API-key string.
//!
//! **Tier validation (canonical-only):** `--tier` accepts only the canonical
//! `fast` / `standard` / `pro` names. Anthropic aliases (`haiku`/`sonnet`/
//! `opus`) and other provider-native aliases (`flash`/`mini`/`plus`/`ultra`/
//! `max`) are rejected at parse time so the persisted `ProviderTiers` always
//! uses canonical keys (the schema does not have an `auto` key, and the
//! aliases exist only as user-input sugar in `/model`).
//!
//! All public entry points return [`anyhow::Result`] and write only via
//! [`ProviderConfigStore`] — no direct file I/O, no separate types.

use std::collections::HashMap;
use std::io::{self, Write};

use anyhow::{Context, Result, anyhow, bail};
use shannon_core::provider_config_service::ProviderConfigService;
use shannon_core::provider_config_store::ProviderConfigStore;
use shannon_engine::api::LlmProvider;
use shannon_types::provider_config::{CredentialRef, ProviderKind, ProviderProfile, ProviderTiers};

/// Canonical tier names accepted by `--tier`. Aliases are rejected.
/// Doc-only: the runtime validator is [`validate_canonical_tier`].
#[allow(dead_code)]
const CANONICAL_TIERS: &[&str] = &["fast", "standard", "pro"];

/// Default `LlmProvider::default_base_url()` per known `ProviderKind`. Used
/// when `--base-url` is omitted for a kind that has a canonical endpoint.
///
/// `openai-compatible` and `ollama` are intentionally omitted — for those the
/// caller MUST supply `--base-url` (we surface a clear validation error
/// rather than silently defaulting `ollama` to `http://localhost:11434`).
fn default_base_url_for_kind(kind: &ProviderKind) -> Option<&'static str> {
    use ProviderKind::*;
    match kind {
        Anthropic => Some(LlmProvider::Anthropic.default_base_url()),
        OpenAi => Some(LlmProvider::OpenAI.default_base_url()),
        Gemini => Some(LlmProvider::Gemini.default_base_url()),
        Deepseek => Some(LlmProvider::DeepSeek.default_base_url()),
        // OpenAI-compatible + Ollama require an explicit --base-url. The
        // desktop's Add Provider form forces the same choice; we reject
        // silently-defaulting here too (avoids accidentally pointing at
        // localhost:11434 or assuming the Zhipu/Moonshot route).
        OpenAiCompatible | Ollama => None,
        _ => None,
    }
}

/// Parse `--kind` into [`ProviderKind`]. Uses the same kebab-case / canonical
/// mapping as the rest of the CLI (matches `ProviderKind`'s `serde(rename)`
/// schema).
fn parse_kind(kind: &str) -> Result<ProviderKind> {
    match kind {
        "anthropic" => Ok(ProviderKind::Anthropic),
        "openai" => Ok(ProviderKind::OpenAi),
        "openai-compatible" => Ok(ProviderKind::OpenAiCompatible),
        "ollama" => Ok(ProviderKind::Ollama),
        "gemini" => Ok(ProviderKind::Gemini),
        "deepseek" => Ok(ProviderKind::Deepseek),
        other => Err(anyhow!(
            "unknown --kind '{other}'; expected one of: anthropic, openai, openai-compatible, ollama, gemini, deepseek"
        )),
    }
}

/// Format a `ProviderKind` for the human-readable table. Matches the Rust
/// `Debug` form users already see in logs and the desktop Add-Provider modal
/// dropdown (capitalised CamelCase).
fn format_kind(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "Anthropic",
        ProviderKind::OpenAi => "OpenAI",
        ProviderKind::OpenAiCompatible => "OpenAICompat",
        ProviderKind::Ollama => "Ollama",
        ProviderKind::Gemini => "Gemini",
        ProviderKind::Deepseek => "DeepSeek",
        _ => "Other",
    }
}

// ── list-providers ──────────────────────────────────────────────────────

/// JSON-friendly summary of one provider row. Used by both the
/// `list-providers` JSON output and as the shape the table formatter renders.
#[derive(serde::Serialize)]
pub struct ProviderRow {
    pub id: String,
    pub kind: String,
    pub base_url: String,
    pub model_id: String,
    pub tier: String,
    pub extra_headers_count: usize,
    pub has_api_key_ref: bool,
    /// Service name when the credential is `CredentialRef::Store { service }`;
    /// `None` for any other credential backend (env / keyring / ephemeral).
    pub credential_service: Option<String>,
}

/// Top-level JSON structure for `--json`.
#[derive(serde::Serialize)]
struct ListProvidersJson {
    active: Option<ActiveTargetJson>,
    providers: Vec<ProviderRow>,
}

#[derive(serde::Serialize)]
struct ActiveTargetJson {
    provider_id: String,
    model_id: String,
}

/// Build a human-readable model id per provider. Looks at the per-tier
/// override that matches the provider's "default" tier (standard), falling
/// back to the first tier set, then to `active_target.model_id`.
///
/// The list table shows ONE model per provider — the "primary" model that a
/// user would expect when they ask "what does this row fire?". For most
/// providers this is just `active_target.model_id`; for openai-compatible
/// entries where `model_id` was left blank at upsert time we pick the
/// standard-tier override.
fn primary_model_id_for(profile: &ProviderProfile, active_model: &str) -> String {
    if !active_model.is_empty() {
        return active_model.to_string();
    }
    profile
        .tiers
        .standard
        .clone()
        .or_else(|| profile.tiers.fast.clone())
        .or_else(|| profile.tiers.pro.clone())
        .unwrap_or_default()
}

/// Build a `(value, has_api_key_ref, credential_service)` triple from a
/// `CredentialRef`. The CLI never serialises the raw secret — when the ref
/// is `Store { service }` we expose only the service name.
fn describe_credential(cred: &CredentialRef) -> (bool, Option<String>) {
    match cred {
        CredentialRef::Store { service } => (true, Some(service.clone())),
        _ => (false, None),
    }
}

/// Collect the rows for the configured `default` model profile, in insertion
/// order. Returns an empty Vec when the profile has no providers.
fn collect_rows(store: &ProviderConfigStore) -> (Vec<ProviderRow>, Option<ActiveTargetJson>) {
    let config = store.config();
    let default = match config.profiles.get("default") {
        Some(p) => p,
        None => return (Vec::new(), None),
    };
    let active = default.active_target.clone();
    let active_json = if active.provider_id.is_empty() && active.model_id.is_empty() {
        None
    } else {
        Some(ActiveTargetJson {
            provider_id: active.provider_id.clone(),
            model_id: active.model_id.clone(),
        })
    };

    let active_model_id = active.model_id.clone();
    let rows: Vec<ProviderRow> = default
        .providers
        .iter()
        .map(|p| {
            let model_id = if p.id == active.provider_id {
                primary_model_id_for(p, &active_model_id)
            } else {
                primary_model_id_for(p, "")
            };
            let (has_api_key_ref, credential_service) = describe_credential(&p.credential);
            let tier = p
                .tiers
                .standard
                .clone()
                .or_else(|| p.tiers.fast.clone())
                .or_else(|| p.tiers.pro.clone())
                .unwrap_or_default();
            ProviderRow {
                id: p.id.clone(),
                kind: format_kind(&p.kind).to_string(),
                base_url: p.base_url.clone(),
                model_id,
                tier,
                extra_headers_count: p.extra_headers.len(),
                has_api_key_ref,
                credential_service,
            }
        })
        .collect();

    (rows, active_json)
}

/// Render the fixed-width table. Columns are sized to the longest cell in
/// each column (header counts).
fn render_table<W: Write>(w: &mut W, rows: &[ProviderRow], active_id: Option<&str>) -> Result<()> {
    let headers = ["ACTIVE", "ID", "KIND", "BASE URL", "MODEL"];
    let mut widths = [
        headers[0].len(),
        headers[1].len(),
        headers[2].len(),
        headers[3].len(),
        headers[4].len(),
    ];

    let mut lines: Vec<[String; 5]> = Vec::with_capacity(rows.len());
    for r in rows {
        let active = match active_id {
            Some(a) if a == r.id => "*".to_string(),
            _ => String::new(),
        };
        let cells = [
            active,
            r.id.clone(),
            r.kind.clone(),
            r.base_url.clone(),
            r.model_id.clone(),
        ];
        for (i, c) in cells.iter().enumerate() {
            if c.len() > widths[i] {
                widths[i] = c.len();
            }
        }
        lines.push(cells);
    }

    // Header row
    writeln!(
        w,
        "{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}  {:<w4$}",
        headers[0],
        headers[1],
        headers[2],
        headers[3],
        headers[4],
        w0 = widths[0],
        w1 = widths[1],
        w2 = widths[2],
        w3 = widths[3],
        w4 = widths[4],
    )?;

    // Body rows
    for cells in &lines {
        writeln!(
            w,
            "{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}  {:<w4$}",
            cells[0],
            cells[1],
            cells[2],
            cells[3],
            cells[4],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
            w3 = widths[3],
            w4 = widths[4],
        )?;
    }
    Ok(())
}

/// Run `shannon list-providers`.
///
/// Reads the persisted store, prints either a fixed-width table (default) or
/// `--json` output, then returns without writing. Never fails on missing
/// config — an empty store produces an empty table / `{"active": null,
/// "providers": []}`.
pub fn run_list_providers(store: &ProviderConfigStore, json: bool) -> Result<()> {
    let (rows, active) = collect_rows(store);
    if json {
        let out = ListProvidersJson {
            active,
            providers: rows,
        };
        let s = serde_json::to_string_pretty(&out)?;
        println!("{s}");
    } else {
        let active_id = active.as_ref().map(|a| a.provider_id.as_str());
        let stdout = io::stdout();
        let mut out = stdout.lock();
        render_table(&mut out, &rows, active_id)?;
        if rows.is_empty() {
            // Surface a hint so a user with no providers knows what to do.
            eprintln!("No providers configured. Run `shannon providers add --help` to add one.");
        }
    }
    Ok(())
}

// ── providers add ───────────────────────────────────────────────────────

/// Parameters captured from `shannon providers add …` clap args. Built and
/// validated before any state mutation, so the parse layer can return clean
/// error messages without rolling back a partial write.
#[derive(Debug, Clone)]
pub struct AddProviderArgs {
    pub id: String,
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub model: String,
    pub api_key_ref: Option<String>,
    pub tier: Option<String>,
    pub extra_header: Vec<String>,
    pub set_active: bool,
}

/// Parse a `--extra-header KEY=VAL` token. Returns `(key, value)`. Rejects
/// empty keys (a header name is required by HTTP semantics) and empty
/// values (the caller's intent is ambiguous; force them to be explicit).
fn parse_extra_header(pair: &str) -> Result<(String, String)> {
    let (key, val) = pair
        .split_once('=')
        .ok_or_else(|| anyhow!("--extra-header must be in KEY=VALUE form (got '{pair}')"))?;
    let key = key.trim().to_string();
    let val = val.trim().to_string();
    if key.is_empty() {
        bail!("--extra-header key cannot be empty");
    }
    if val.is_empty() {
        bail!("--extra-header value cannot be empty (key was '{key}')");
    }
    Ok((key, val))
}

/// Validate `tier` against the canonical names. We deliberately do NOT use
/// `TierName::from_user_input` here: that function happily converts aliases
/// to canonical tier keys, but the user asked for canonical-only persistence
/// (the schema has no alias keys). Aliases produce a different error from
/// "unknown" so the user gets actionable feedback.
fn validate_canonical_tier(tier: &str) -> Result<&'static str> {
    let lower = tier.to_ascii_lowercase();
    match lower.as_str() {
        "fast" => Ok("fast"),
        "standard" => Ok("standard"),
        "pro" => Ok("pro"),
        // Common aliases get a tailored message — they're valid for `--tier`
        // in `/model` but NOT in the persisted schema.
        "haiku" | "flash" | "mini" | "nano" => {
            bail!("--tier '{tier}' is an alias for 'fast'; use 'fast' instead")
        }
        "sonnet" | "plus" | "medium" | "turbo" => {
            bail!("--tier '{tier}' is an alias for 'standard'; use 'standard' instead")
        }
        "opus" | "ultra" | "max" | "large" => {
            bail!("--tier '{tier}' is an alias for 'pro'; use 'pro' instead")
        }
        "auto" => bail!("--tier 'auto' is resolver-only; use 'fast', 'standard', or 'pro'"),
        other => bail!("unknown --tier '{other}'; expected one of: fast, standard, pro"),
    }
}

/// Resolve `--base-url`: if supplied use it verbatim; otherwise look up the
/// canonical default for the kind. Errors when the kind has no canonical
/// default (openai-compatible / ollama always require an explicit URL).
fn resolve_base_url(kind: &ProviderKind, supplied: Option<String>) -> Result<String> {
    if let Some(b) = supplied {
        if b.trim().is_empty() {
            bail!("--base-url cannot be empty");
        }
        return Ok(b.trim().to_string());
    }
    default_base_url_for_kind(kind)
        .map(|s| s.to_string())
        .ok_or_else(|| {
            anyhow!(
                "--base-url is required for --kind {kind}; supply the endpoint URL (e.g. https://api.example.com/v1)",
                kind = kind_user_input_name(kind),
            )
        })
}

/// User-facing kebab-case form of a `ProviderKind` (matches the input the
/// user passed to `--kind` and the values used in tests / error messages).
fn kind_user_input_name(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenAi => "openai",
        ProviderKind::OpenAiCompatible => "openai-compatible",
        ProviderKind::Ollama => "ollama",
        ProviderKind::Gemini => "gemini",
        ProviderKind::Deepseek => "deepseek",
        _ => "unknown",
    }
}

/// Build a complete `ProviderProfile` from the validated args. Returns the
/// profile + the resolved `(id, kind)` so the caller can echo details.
fn build_profile(args: &AddProviderArgs) -> Result<(ProviderProfile, String)> {
    if args.id.trim().is_empty() {
        bail!("provider id cannot be empty");
    }
    let id = args.id.trim().to_string();

    let base_url = resolve_base_url(&args.kind, args.base_url.clone())?;

    // Tier validation: only canonical names accepted. The schema's
    // ProviderTiers has no `alias` field, and persisting an unknown key
    // would silently drop it.
    let tier_canonical = match &args.tier {
        Some(t) => Some(validate_canonical_tier(t)?),
        None => None,
    };

    let mut extra_headers: HashMap<String, String> = HashMap::new();
    for raw in &args.extra_header {
        let (k, v) = parse_extra_header(raw)?;
        if extra_headers.insert(k.clone(), v).is_some() {
            bail!("--extra-header key '{k}' specified more than once");
        }
    }

    // Credential: ALWAYS CredentialRef::Store { service }. We never accept
    // an inline api-key — the engine has no API for storing raw secrets,
    // and the engine contract is "use ~/.shannon/credentials/<svc>.json,
    // not the config file". The --api-key-ref flag is the service name,
    // never the secret value.
    let service = args
        .api_key_ref
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| id.clone());
    let service = service.trim().to_string();
    if service.is_empty() {
        bail!("resolved credential service name is empty");
    }
    let credential = CredentialRef::Store {
        service: service.clone(),
    };

    // Per-tier override: only fill the resolved canonical tier key; the
    // other two tiers stay None so engine fallback behaves as "use
    // active_target model everywhere". Persisting aliases would be a
    // schema violation (`auto` has no key).
    let tiers = match tier_canonical {
        Some("fast") => ProviderTiers {
            fast: Some(args.model.clone()),
            ..Default::default()
        },
        Some("standard") => ProviderTiers {
            standard: Some(args.model.clone()),
            ..Default::default()
        },
        Some("pro") => ProviderTiers {
            pro: Some(args.model.clone()),
            ..Default::default()
        },
        _ => ProviderTiers::default(),
    };

    let profile = ProviderProfile {
        id: id.clone(),
        kind: args.kind.clone(),
        display_name: id.clone(),
        base_url,
        models_url: None,
        credential,
        extra_headers,
        default_max_tokens: None,
        fallback_models: Vec::new(),
        quirks: Default::default(),
        tiers,
    };

    Ok((profile, service))
}

/// Validate args and build the profile + resolved credential service name +
/// resolved model id. Pure (no store mutation, no disk I/O) so the unit tests
/// stay hermetic. Shared by the test seam `apply_provider_add` and the
/// production path [`run_providers_add`] (which persists via
/// [`ProviderConfigService`]).
fn build_and_validate(args: &AddProviderArgs) -> Result<(ProviderProfile, String, String)> {
    let (profile, service) = build_profile(args)?;
    let model_for_active = args.model.trim().to_string();
    if model_for_active.is_empty() {
        bail!("--model cannot be empty");
    }
    Ok((profile, service, model_for_active))
}

/// Validate args and apply the upsert to `store`. Returns the resolved
/// credential service name and the resolved model id used for
/// `active_target`. **Does not persist** — that's the caller's job.
///
/// This is the non-persisting half of `providers add`, kept as a hermetic
/// test seam (the unit tests assert on `store.config()` without touching
/// `~/.shannon/`). The production write path is [`run_providers_add`], which
/// routes the same build through [`ProviderConfigService::upsert`] — the
/// single semantic write path for `providers.toml` (ADR-0008 P2-5).
#[cfg(test)]
pub fn apply_provider_add(
    store: &mut ProviderConfigStore,
    args: &AddProviderArgs,
) -> Result<(String, String)> {
    let (profile, service, model_for_active) = build_and_validate(args)?;
    store.upsert_profile(profile, &model_for_active);
    Ok((service, model_for_active))
}

/// Run `shannon providers add …`.
///
/// Validates args, then routes the upsert + persist through
/// [`ProviderConfigService`] — the single write path for `providers.toml`
/// shared with the REPL's `/connect` (ADR-0008 P2-5 step 2). The new provider
/// always becomes the active target (`make_active = true`), matching the
/// pre-refactor behavior where `upsert_profile` pinned `active_target`
/// regardless of `--set-active`; that flag remains a documented no-op so the
/// command-line contract is unchanged. Returns an error before any state
/// mutation if validation fails.
pub fn run_providers_add(store: &mut ProviderConfigStore, args: &AddProviderArgs) -> Result<()> {
    let (profile, service, model_for_active) = build_and_validate(args)?;

    // `--set-active` is a documented no-op: the new provider always becomes
    // active (`make_active = true`), which is the sensible default for
    // `providers add` (a user adding their first provider expects it active).
    // The flag is retained so scripts can express intent; the service can now
    // express `make_active = false`, but wiring the flag would harm the common
    // case, so we deliberately keep the always-active behavior.
    let _ = args.set_active;

    // Hand the already-loaded store to the service so the write goes through
    // the one semantic path. `mem::take` lets us move the store into the
    // service and recover it afterward without a clone.
    let mut svc = ProviderConfigService::from_store(std::mem::take(store));
    let upsert_result = svc.upsert(profile, &model_for_active, true);
    // Always recover the store (the service owns it) whether or not the
    // persist succeeded, so the caller's `&mut` reflects the in-memory state.
    *store = svc.into_inner();
    let saved_path = upsert_result.with_context(|| "failed to persist providers.toml")?;

    println!(
        "Added provider {id} (kind={kind}, model={model})",
        id = args.id.trim(),
        kind = format_kind(&args.kind),
        model = model_for_active,
    );
    println!("  Credential ref: store:{service}");
    println!("  Persisted to: {}", saved_path.display());
    Ok(())
}

// ── providers remove ────────────────────────────────────────────────────

/// Parameters captured from `shannon providers remove <ID>` clap args.
#[derive(Debug, Clone)]
pub struct RemoveProviderArgs {
    pub id: String,
}

/// Run `shannon providers remove <ID>`.
///
/// Validate and apply the remove. Returns whether the removed slot was the
/// active target. **Does not persist** — that's the caller's job.
pub fn apply_provider_remove(
    store: &mut ProviderConfigStore,
    args: &RemoveProviderArgs,
) -> Result<bool> {
    let id = args.id.trim();
    if id.is_empty() {
        bail!("provider id cannot be empty");
    }

    let was_active = store
        .config()
        .profiles
        .get("default")
        .map(|mp| mp.active_target.provider_id == id)
        .unwrap_or(false);

    store.remove_profile(id);
    Ok(was_active)
}

/// Removes the provider slot (engine behaviour: when the active profile is
/// removed, `active_target` is cleared by [`ProviderConfigStore::remove_profile`]).
/// Returns the boolean "was this the active target?" — surfaced as a
/// stderr warning so the user knows to pick a different provider.
pub fn run_providers_remove(
    store: &mut ProviderConfigStore,
    args: &RemoveProviderArgs,
) -> Result<bool> {
    let id = args.id.trim();
    if id.is_empty() {
        bail!("provider id cannot be empty");
    }

    let was_active = apply_provider_remove(store, args)?;

    let saved_path = store
        .save()
        .with_context(|| "failed to persist providers.toml")?;

    println!("Removed provider {id}");
    println!("  Persisted to: {}", saved_path.display());
    Ok(was_active)
}

// ── Pure helpers exposed for tests ──────────────────────────────────────

/// Public wrapper around [`parse_kind`] used by the CLI dispatch in
/// `main.rs` (where clap hands us a `String`) and by the unit tests.
pub fn parse_kind_cli(s: &str) -> Result<ProviderKind> {
    parse_kind(s)
}

/// Public wrapper around [`validate_canonical_tier`]; exposed for the
/// unit tests (clap-parseable validation flows already call the inner
/// helper via [`run_providers_add`]).
#[allow(dead_code)]
pub fn validate_tier_cli(s: &str) -> Result<&'static str> {
    validate_canonical_tier(s)
}

/// Mirror of `render_table`'s algorithm writing to a String. Identical
/// column-widthing logic; used to assert text-output semantics without a
/// TTY/pipe. Marks the active row with `*`.
#[cfg(test)]
fn render_table_for_tests(store: &ProviderConfigStore) -> String {
    let active_id = store.config().profiles.get("default").and_then(|p| {
        if p.active_target.provider_id.is_empty() {
            None
        } else {
            Some(p.active_target.provider_id.clone())
        }
    });

    let profiles = store
        .config()
        .profiles
        .get("default")
        .map(|p| p.providers.clone())
        .unwrap_or_default();

    let headers = ["ACTIVE", "ID", "KIND", "BASE URL", "MODEL"];
    let mut widths = [
        headers[0].len(),
        headers[1].len(),
        headers[2].len(),
        headers[3].len(),
        headers[4].len(),
    ];
    let mut lines: Vec<[String; 5]> = Vec::new();
    for p in &profiles {
        let active_marker = match &active_id {
            Some(a) if a == &p.id => "*".to_string(),
            _ => String::new(),
        };
        let cells = [
            active_marker,
            p.id.clone(),
            format_kind(&p.kind).to_string(),
            p.base_url.clone(),
            primary_model_id_for(p, ""),
        ];
        for (i, c) in cells.iter().enumerate() {
            if c.len() > widths[i] {
                widths[i] = c.len();
            }
        }
        lines.push(cells);
    }
    let mut out = String::new();
    out.push_str(&format!(
        "{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}  {:<w4$}\n",
        headers[0],
        headers[1],
        headers[2],
        headers[3],
        headers[4],
        w0 = widths[0],
        w1 = widths[1],
        w2 = widths[2],
        w3 = widths[3],
        w4 = widths[4],
    ));
    for cells in &lines {
        out.push_str(&format!(
            "{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}  {:<w4$}\n",
            cells[0],
            cells[1],
            cells[2],
            cells[3],
            cells[4],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
            w3 = widths[3],
            w4 = widths[4],
        ));
    }
    out
}

// ── tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sample_profile(
        id: &str,
        kind: ProviderKind,
        base_url: &str,
        model_id: &str,
    ) -> ProviderProfile {
        ProviderProfile {
            id: id.to_string(),
            kind,
            display_name: id.to_string(),
            base_url: base_url.to_string(),
            models_url: None,
            credential: CredentialRef::Store {
                service: id.to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers {
                standard: Some(model_id.to_string()),
                ..Default::default()
            },
        }
    }

    fn build_add_args(
        id: &str,
        kind: ProviderKind,
        base_url: Option<&str>,
        model: &str,
        api_key_ref: Option<&str>,
        tier: Option<&str>,
        extra_header: Vec<&str>,
        set_active: bool,
    ) -> AddProviderArgs {
        AddProviderArgs {
            id: id.to_string(),
            kind,
            base_url: base_url.map(String::from),
            model: model.to_string(),
            api_key_ref: api_key_ref.map(String::from),
            tier: tier.map(String::from),
            extra_header: extra_header.into_iter().map(String::from).collect(),
            set_active,
        }
    }

    // ── list-providers ───────────────────────────────────────────────

    #[test]
    fn list_providers_empty_store_does_not_panic() {
        let store = ProviderConfigStore::default();
        let res = run_list_providers(&store, false);
        assert!(res.is_ok(), "list on empty store must succeed: {res:?}");
    }

    #[test]
    fn list_providers_with_active_marks_star_correctly() {
        let mut store = ProviderConfigStore::default();
        store.upsert_profile(
            sample_profile(
                "anthropic-default",
                ProviderKind::Anthropic,
                "https://api.anthropic.com",
                "claude-sonnet-4-6",
            ),
            "claude-sonnet-4-6",
        );
        store.upsert_profile(
            sample_profile(
                "glm",
                ProviderKind::OpenAiCompatible,
                "https://open.bigmodel.cn/v1",
                "glm-4.6",
            ),
            "glm-4.6",
        );
        // Re-upsert anthropic to make it the active target.
        store.upsert_profile(
            sample_profile(
                "anthropic-default",
                ProviderKind::Anthropic,
                "https://api.anthropic.com",
                "claude-sonnet-4-6",
            ),
            "claude-sonnet-4-6",
        );

        let active_id = store
            .config()
            .profiles
            .get("default")
            .unwrap()
            .active_target
            .provider_id
            .clone();
        assert_eq!(active_id, "anthropic-default");

        let rendered = render_table_for_tests(&store);
        let star_lines: Vec<&str> = rendered.lines().filter(|l| l.starts_with('*')).collect();
        assert_eq!(star_lines.len(), 1, "exactly one star row: {rendered:?}");
        assert!(
            star_lines[0].contains("anthropic-default"),
            "active star must mark the anthropic-default row; got: {:?}",
            star_lines[0]
        );
    }

    // ── providers add ────────────────────────────────────────────────

    #[test]
    fn providers_add_minimal_anthropic_succeeds() {
        let mut store = ProviderConfigStore::default();
        let args = build_add_args(
            "anthropic-test",
            ProviderKind::Anthropic,
            None,
            "claude-sonnet-4-6",
            None,
            None,
            vec![],
            false,
        );

        // Use `load_or_default` + a fresh store to avoid touching real disk.
        apply_provider_add(&mut store, &args).expect("add must succeed");

        let cfg = store.config();
        let default = cfg.profiles.get("default").expect("default profile");
        assert_eq!(default.providers.len(), 1);
        let p = &default.providers[0];
        assert_eq!(p.id, "anthropic-test");
        assert_eq!(p.kind, ProviderKind::Anthropic);
        assert_eq!(p.base_url, "https://api.anthropic.com");
        match &p.credential {
            CredentialRef::Store { service } => assert_eq!(service, "anthropic-test"),
            other => panic!("expected Store credential, got {other:?}"),
        }
        assert_eq!(default.active_target.provider_id, "anthropic-test");
        assert_eq!(default.active_target.model_id, "claude-sonnet-4-6");
    }

    #[test]
    fn providers_add_openai_compatible_requires_base_url() {
        let mut store = ProviderConfigStore::default();
        let args = build_add_args(
            "oai-test",
            ProviderKind::OpenAiCompatible,
            None,
            "gpt-4o",
            None,
            None,
            vec![],
            false,
        );

        let err = apply_provider_add(&mut store, &args).expect_err("must reject");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("--base-url") && msg.contains("openai-compatible"),
            "error must call out missing --base-url for openai-compatible; got: {msg}",
        );

        let providers = store
            .config()
            .profiles
            .get("default")
            .map(|p| p.providers.len())
            .unwrap_or(0);
        assert_eq!(providers, 0, "validation failure must not mutate store");
    }

    #[test]
    fn providers_add_ollama_requires_base_url() {
        let mut store = ProviderConfigStore::default();
        let args = build_add_args(
            "ollama-test",
            ProviderKind::Ollama,
            None,
            "llama3",
            None,
            None,
            vec![],
            false,
        );

        let err =
            apply_provider_add(&mut store, &args).expect_err("must reject without --base-url");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("--base-url") && msg.to_lowercase().contains("ollama"),
            "error must call out missing --base-url for ollama; got: {msg}",
        );
    }

    #[test]
    fn providers_add_rejects_alias_tier() {
        let mut store = ProviderConfigStore::default();
        let args = build_add_args(
            "anthropic-test",
            ProviderKind::Anthropic,
            None,
            "claude-sonnet-4-6",
            None,
            Some("sonnet"), // alias — must be rejected
            vec![],
            false,
        );
        let err = apply_provider_add(&mut store, &args).expect_err("alias tier must fail");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("alias") && msg.contains("standard"),
            "error must point the user at the canonical name; got: {msg}",
        );
        assert_eq!(
            store
                .config()
                .profiles
                .get("default")
                .map(|p| p.providers.len())
                .unwrap_or(0),
            0,
            "rejected add must not mutate store",
        );
    }

    #[test]
    fn providers_add_accepts_canonical_tier_persists_canonical_key() {
        let mut store = ProviderConfigStore::default();
        let args = build_add_args(
            "anthropic-test",
            ProviderKind::Anthropic,
            None,
            "claude-haiku-4-5",
            None,
            Some("fast"),
            vec![],
            false,
        );
        apply_provider_add(&mut store, &args).expect("canonical tier must succeed");
        let p = &store.config().profiles.get("default").unwrap().providers[0];
        assert_eq!(p.tiers.fast.as_deref(), Some("claude-haiku-4-5"));
        assert!(
            p.tiers.standard.is_none(),
            "only canonical tier must be set"
        );
        assert!(p.tiers.pro.is_none());
    }

    #[test]
    fn providers_add_extra_headers_repeatable_each_appends_one_entry() {
        let mut store = ProviderConfigStore::default();
        let args = build_add_args(
            "anthropic-test",
            ProviderKind::Anthropic,
            None,
            "claude-sonnet-4-6",
            None,
            None,
            vec!["X-Foo=bar", "X-Baz=qux"],
            false,
        );
        apply_provider_add(&mut store, &args).expect("two extra headers must succeed");
        let p = &store.config().profiles.get("default").unwrap().providers[0];
        assert_eq!(p.extra_headers.len(), 2);
        assert_eq!(
            p.extra_headers.get("X-Foo").map(String::as_str),
            Some("bar")
        );
        assert_eq!(
            p.extra_headers.get("X-Baz").map(String::as_str),
            Some("qux")
        );
    }

    #[test]
    fn providers_add_rejects_empty_extra_header_key_or_value() {
        // Empty key
        let mut store = ProviderConfigStore::default();
        let args = build_add_args(
            "anthropic-test",
            ProviderKind::Anthropic,
            None,
            "claude-sonnet-4-6",
            None,
            None,
            vec!["=noval"],
            false,
        );
        assert!(apply_provider_add(&mut store, &args).is_err());

        // Empty value
        let mut store = ProviderConfigStore::default();
        let args = build_add_args(
            "anthropic-test",
            ProviderKind::Anthropic,
            None,
            "claude-sonnet-4-6",
            None,
            None,
            vec!["X-Foo="],
            false,
        );
        assert!(apply_provider_add(&mut store, &args).is_err());

        // No '=' at all
        let mut store = ProviderConfigStore::default();
        let args = build_add_args(
            "anthropic-test",
            ProviderKind::Anthropic,
            None,
            "claude-sonnet-4-6",
            None,
            None,
            vec!["no-equals-sign"],
            false,
        );
        assert!(apply_provider_add(&mut store, &args).is_err());
    }

    #[test]
    fn providers_add_api_key_ref_defaults_to_provider_id() {
        let mut store = ProviderConfigStore::default();
        let args = build_add_args(
            "anthropic-test",
            ProviderKind::Anthropic,
            None,
            "claude-sonnet-4-6",
            None,
            None,
            vec![],
            false,
        );
        apply_provider_add(&mut store, &args).expect("must succeed");
        let p = &store.config().profiles.get("default").unwrap().providers[0];
        match &p.credential {
            CredentialRef::Store { service } => assert_eq!(service, "anthropic-test"),
            other => panic!("expected Store credential, got {other:?}"),
        }
    }

    #[test]
    fn providers_add_api_key_ref_override() {
        let mut store = ProviderConfigStore::default();
        let args = build_add_args(
            "anthropic-test",
            ProviderKind::Anthropic,
            None,
            "claude-sonnet-4-6",
            Some("anthropic-prod"),
            None,
            vec![],
            false,
        );
        apply_provider_add(&mut store, &args).expect("must succeed");
        let p = &store.config().profiles.get("default").unwrap().providers[0];
        match &p.credential {
            CredentialRef::Store { service } => assert_eq!(service, "anthropic-prod"),
            other => panic!("expected Store credential, got {other:?}"),
        }
    }

    // ── providers remove ─────────────────────────────────────────────

    #[test]
    fn providers_remove_unknown_id_is_idempotent_returns_false() {
        let mut store = ProviderConfigStore::default();
        let args = RemoveProviderArgs {
            id: "does-not-exist".to_string(),
        };
        let was_active =
            apply_provider_remove(&mut store, &args).expect("remove must be idempotent");
        assert!(!was_active);
    }

    #[test]
    fn providers_remove_clears_active_when_was_active() {
        let mut store = ProviderConfigStore::default();
        store.upsert_profile(
            sample_profile(
                "glm",
                ProviderKind::OpenAiCompatible,
                "https://open.bigmodel.cn/v1",
                "glm-4.6",
            ),
            "glm-4.6",
        );
        let args = RemoveProviderArgs {
            id: "glm".to_string(),
        };
        let was_active = apply_provider_remove(&mut store, &args).expect("remove must succeed");
        assert!(was_active, "must report the slot was the active target");

        let default = store
            .config()
            .profiles
            .get("default")
            .expect("default profile remains");
        assert!(
            default.providers.is_empty(),
            "the only provider slot must be gone",
        );
        assert_eq!(default.active_target.provider_id, "");
        assert_eq!(default.active_target.model_id, "");
    }

    #[test]
    fn providers_remove_does_not_clear_active_when_other_was_active() {
        let mut store = ProviderConfigStore::default();
        store.upsert_profile(
            sample_profile(
                "glm",
                ProviderKind::OpenAiCompatible,
                "https://open.bigmodel.cn/v1",
                "glm-4.6",
            ),
            "glm-4.6",
        );
        store.upsert_profile(
            sample_profile(
                "kimi",
                ProviderKind::OpenAiCompatible,
                "https://api.moonshot.cn/v1",
                "moonshot-v1-8k",
            ),
            "moonshot-v1-8k",
        );
        let args = RemoveProviderArgs {
            id: "glm".to_string(),
        };
        let was_active = apply_provider_remove(&mut store, &args).expect("remove must succeed");
        assert!(!was_active);
        let active = store
            .config()
            .profiles
            .get("default")
            .unwrap()
            .active_target
            .provider_id
            .clone();
        assert_eq!(active, "kimi");
    }

    // ── validate helpers ─────────────────────────────────────────────

    #[test]
    fn parse_kind_accepts_all_canonical_names() {
        let mapping = [
            ("anthropic", ProviderKind::Anthropic),
            ("openai", ProviderKind::OpenAi),
            ("openai-compatible", ProviderKind::OpenAiCompatible),
            ("ollama", ProviderKind::Ollama),
            ("gemini", ProviderKind::Gemini),
            ("deepseek", ProviderKind::Deepseek),
        ];
        for (input, expected) in mapping {
            let got =
                parse_kind_cli(input).unwrap_or_else(|e| panic!("must accept '{input}': {e}"));
            assert_eq!(got, expected, "input '{input}' parsed wrong");
        }
    }

    #[test]
    fn parse_kind_rejects_unknown() {
        assert!(parse_kind_cli("not-a-kind").is_err());
        assert!(parse_kind_cli("").is_err());
    }

    #[test]
    fn validate_tier_accepts_canonical_only() {
        assert_eq!(validate_tier_cli("fast").unwrap(), "fast");
        assert_eq!(validate_tier_cli("standard").unwrap(), "standard");
        assert_eq!(validate_tier_cli("pro").unwrap(), "pro");
        assert_eq!(validate_tier_cli("FAST").unwrap(), "fast");
    }

    #[test]
    fn validate_tier_rejects_all_aliases_with_helpful_message() {
        let aliases = [
            "haiku", "flash", "mini", "nano", "sonnet", "plus", "medium", "turbo", "opus", "ultra",
            "max", "large",
        ];
        for alias in aliases {
            assert!(
                validate_tier_cli(alias).is_err(),
                "alias '{alias}' must be rejected",
            );
        }
        assert!(validate_tier_cli("auto").is_err());
        assert!(validate_tier_cli("unknown").is_err());
    }
}
