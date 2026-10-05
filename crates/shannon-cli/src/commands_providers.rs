//! CLI plumbing for `shannon list-providers` and
//! `shannon providers add|remove|model-meta`.
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
//! All public entry points return [`anyhow::Result`] and write through
//! [`ProviderConfigService`] — the single semantic write path shared with the
//! REPL's `/connect` (ADR-0008 P2-5). No direct file I/O, no separate types.

use std::collections::HashMap;
use std::io::{self, Write};

use anyhow::{Context, Result, anyhow, bail};
use shannon_core::credential_manager::CredentialManager;
use shannon_core::provider_config_service::ProviderConfigService;
use shannon_core::provider_config_store::ProviderConfigStore;
use shannon_engine::api::LlmProvider;
use shannon_types::provider_config::{
    CredentialRef, ModelCapability, ModelSpec, ProviderKind, ProviderModelConfig, ProviderProfile,
    ProviderTiers,
};

/// Canonical tier names accepted by `--tier`. Aliases are rejected.
/// Doc-only: the runtime validator is [`validate_canonical_tier`].
#[allow(dead_code)] // KEEP: doc-only reference for accepted tier names; runtime validation lives in `validate_canonical_tier`.
const CANONICAL_TIERS: &[&str] = &["fast", "standard", "pro"];

/// Warn on stderr when `~/.shannon/providers.toml` exists but cannot be
/// parsed as a provider config (one unknown field in a hand-edited block is
/// enough — the schema is `deny_unknown_fields`). Without this, every read
/// surface silently shows "nothing connected" and the user has no hint why
/// (the `tracing` warn in `load` sits below the CLI's default `error` log
/// level). Writes to the file are refused by the store's data-integrity
/// guard with the full parse error; this warning is the read-side hint.
pub fn warn_if_providers_toml_unparseable() {
    if let Some(err) = shannon_core::provider_config_store::parse_error(None) {
        eprintln!(
            "warning: ~/.shannon/providers.toml exists but is not a valid provider config; \
             it is being ignored until fixed (writes to it are refused):\n  {err}"
        );
    }
}

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

/// Collect the rows for the **active** model profile
/// (`config.active_profile`, R3-2 — `"default"` when unset), in insertion
/// order. Returns an empty Vec when the profile has no providers.
fn collect_rows(store: &ProviderConfigStore) -> (Vec<ProviderRow>, Option<ActiveTargetJson>) {
    let config = store.config();
    let default = match config.active_model_profile() {
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
        models: Vec::new(),
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

/// Validate and apply the remove. Returns whether the removed slot was the
/// active target. **Does not persist** — that's the caller's job.
///
/// Test-only seam for asserting store-level `remove_profile` behavior without
/// disk I/O. The production path [`run_providers_remove`] routes through
/// [`ProviderConfigService::disconnect_by_slug`] instead (ADR-0008 P2-5).
#[cfg(test)]
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

/// Removes the provider slot by routing through the single semantic write
/// path — [`ProviderConfigService::disconnect_by_slug`] (ADR-0008 P2-5).
///
/// `<ID>` is the raw stored id (`ProviderProfile.id`), which may be a custom
/// slug like `glm` that does not round-trip through `LlmProvider` →
/// [`ProviderConfigService::disconnect`] (that method canonicalizes `glm` →
/// `zhipu` and would miss). `disconnect_by_slug` matches the raw string
/// directly.
///
/// Prints the outcome: which file was written, and — when the removed slot
/// was the active target — the next still-connected provider the engine will
/// fall back to (or a synthesis-mode warning when none remain).
pub fn run_providers_remove(
    store: &mut ProviderConfigStore,
    args: &RemoveProviderArgs,
) -> Result<()> {
    let id = args.id.trim();
    if id.is_empty() {
        bail!("provider id cannot be empty");
    }

    // Hand the already-loaded store to the service so the write goes through
    // the one semantic path. `mem::take` lets us move the store into the
    // service and recover it afterward without a clone — same pattern as
    // `run_providers_add`.
    let mut svc = ProviderConfigService::from_store(std::mem::take(store));
    let outcome = svc
        .disconnect_by_slug(id)
        .with_context(|| "failed to persist providers.toml")?;
    *store = svc.into_inner();

    if !outcome.was_connected {
        // Idempotent: unknown id is a no-op (historical behavior), but surface
        // it so the user knows the remove did not match anything.
        println!("Provider {id} was not found in providers.toml (no change)");
        return Ok(());
    }

    if let Some(path) = &outcome.saved_path {
        println!("Removed provider {id}");
        println!("  Persisted to: {}", path.display());
    }
    if outcome.was_active {
        match &outcome.next_active {
            Some(next) => println!("  Active target switched to: {next}"),
            None => eprintln!(
                "warning: removed provider was the active target; \
                 no other connected provider remains, so the engine will \
                 fall back to synthesis on the next request. \
                 Run `shannon list-providers` to confirm."
            ),
        }
    }
    Ok(())
}

// ── providers model-meta (R2-4) ─────────────────────────────────────────

/// Parameters captured from `shannon providers model-meta …` clap args.
/// Mirrors [`ModelSpec`] semantics; the pure builder lives in
/// [`build_model_meta_spec`] so validation is unit-testable without disk
/// I/O.
#[derive(Debug, Clone)]
pub struct ModelMetaArgs {
    pub provider: String,
    pub model: String,
    pub display_name: Option<String>,
    pub context: Option<u32>,
    pub max_output: Option<u32>,
    pub price_in: Option<f64>,
    pub price_out: Option<f64>,
    pub cap: Vec<String>,
    pub remove: bool,
}

/// Parse a `--cap` value into a [`ModelCapability`]. Unknown names are
/// rejected with the accepted list (the schema rejects them too, but the
/// CLI surfaces a friendlier error before any write).
pub fn parse_capability(s: &str) -> Result<ModelCapability> {
    match s.trim().to_ascii_lowercase().as_str() {
        "reasoning" => Ok(ModelCapability::Reasoning),
        "coding" => Ok(ModelCapability::Coding),
        "speed" => Ok(ModelCapability::Speed),
        "cheap" => Ok(ModelCapability::Cheap),
        "vision" => Ok(ModelCapability::Vision),
        "tool_use" | "tools" | "tool-use" => Ok(ModelCapability::ToolUse),
        other => Err(anyhow!(
            "unknown --cap '{other}'; expected one of: reasoning, coding, speed, cheap, vision, tool_use"
        )),
    }
}

/// Build (or merge into) the [`ModelSpec`] for a `providers model-meta`
/// invocation. When `existing` is `Some`, its values are the starting
/// point and the supplied flags overwrite only the fields they set — so
/// `providers model-meta glm glm-4.6 --context 200000` updates just the
/// context window. Pure: no store access, no disk I/O.
pub fn build_model_meta_spec(
    existing: Option<&ModelSpec>,
    args: &ModelMetaArgs,
) -> Result<ModelSpec> {
    let provider = args.provider.trim();
    let model = args.model.trim();
    if provider.is_empty() {
        bail!("provider id cannot be empty");
    }
    if model.is_empty() {
        bail!("model id cannot be empty");
    }

    let mut spec = match existing {
        Some(e) => {
            let mut s = e.clone();
            // The positional MODEL id always wins (renames via upsert).
            s.id = model.to_string();
            s
        }
        None => ModelSpec {
            id: model.to_string(),
            display_name: None,
            context_window: None,
            max_output: None,
            cost_per_m_input: None,
            cost_per_m_output: None,
            capabilities: Vec::new(),
        },
    };

    if args.display_name.is_some() {
        spec.display_name = args.display_name.clone();
    }
    if args.context.is_some() {
        spec.context_window = args.context;
    }
    if args.max_output.is_some() {
        spec.max_output = args.max_output;
    }
    if args.price_in.is_some() {
        spec.cost_per_m_input = args.price_in;
    }
    if args.price_out.is_some() {
        spec.cost_per_m_output = args.price_out;
    }
    if !args.cap.is_empty() {
        spec.capabilities = args
            .cap
            .iter()
            .map(|c| parse_capability(c))
            .collect::<Result<Vec<_>>>()?;
    }

    // Semantic validation (positive limits, finite non-negative prices) —
    // the same rules `providers.toml` load enforces, applied before any
    // write so the user gets a clean error instead of a refused file.
    spec.validate().map_err(|e| anyhow!("{e}"))?;
    Ok(spec)
}

/// Run `shannon providers model-meta <PROVIDER> <MODEL> [flags]`.
///
/// Routes through [`ProviderConfigService`] (`set_model_meta` /
/// `remove_model_meta`) — the single semantic write path for
/// `providers.toml`, shared with the REPL and the desktop. The provider id
/// is the raw stored slug (an openai-compatible slot may be `glm`, which
/// must not be canonicalized to `zhipu`).
pub fn run_providers_model_meta(
    store: &mut ProviderConfigStore,
    args: &ModelMetaArgs,
) -> Result<()> {
    if args.remove {
        // Idempotence with a visible outcome: when the provider slot exists
        // but carries no such declaration, say so and skip the write (the
        // store-level remove is a no-op in that case anyway).
        let entry_exists = store
            .config()
            .profiles
            .get("default")
            .and_then(|mp| mp.providers.iter().find(|p| p.id == args.provider.trim()))
            .map(|p| p.models.iter().any(|m| m.id == args.model.trim()))
            .unwrap_or(false);
        if !entry_exists {
            println!(
                "No model declaration for {model} on provider {provider} (no change)",
                model = args.model.trim(),
                provider = args.provider.trim(),
            );
            return Ok(());
        }
        let mut svc = ProviderConfigService::from_store(std::mem::take(store));
        let result = svc.remove_model_meta(args.provider.trim(), args.model.trim());
        *store = svc.into_inner();
        result.with_context(|| "failed to persist providers.toml")?;
        println!(
            "Removed model metadata for {model} on provider {provider}",
            model = args.model.trim(),
            provider = args.provider.trim(),
        );
        Ok(())
    } else {
        // Merge with the current entry (if any) so omitted flags keep the
        // previously-declared values.
        let existing = store
            .config()
            .profiles
            .get("default")
            .and_then(|mp| mp.providers.iter().find(|p| p.id == args.provider.trim()))
            .and_then(|p| p.models.iter().find(|m| m.id == args.model.trim()))
            .cloned();
        let spec = build_model_meta_spec(existing.as_ref(), args)?;
        let summary = summarize_spec(&spec);

        let mut svc = ProviderConfigService::from_store(std::mem::take(store));
        let result = svc.set_model_meta(args.provider.trim(), spec);
        *store = svc.into_inner();
        let saved_path = result.with_context(|| "failed to persist providers.toml")?;
        println!(
            "Declared model metadata for {model} on provider {provider}: {summary}",
            model = args.model.trim(),
            provider = args.provider.trim(),
        );
        println!("  Persisted to: {}", saved_path.display());
        Ok(())
    }
}

/// One-line human summary of a [`ModelSpec`] for command output.
fn summarize_spec(spec: &ModelSpec) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(ctx) = spec.context_window {
        parts.push(format!("context={ctx}"));
    }
    if let Some(out) = spec.max_output {
        parts.push(format!("max_output={out}"));
    }
    if let (Some(i), Some(o)) = (spec.cost_per_m_input, spec.cost_per_m_output) {
        parts.push(format!("pricing=${i}/${o} per Mtok"));
    } else if spec.cost_per_m_input.is_some() || spec.cost_per_m_output.is_some() {
        parts.push("pricing=(incomplete — needs both prices)".to_string());
    }
    if !spec.capabilities.is_empty() {
        let caps: Vec<&str> = spec
            .capabilities
            .iter()
            .map(|c| match c {
                ModelCapability::Reasoning => "reasoning",
                ModelCapability::Coding => "coding",
                ModelCapability::Speed => "speed",
                ModelCapability::Cheap => "cheap",
                ModelCapability::Vision => "vision",
                ModelCapability::ToolUse => "tool_use",
                _ => "other",
            })
            .collect();
        parts.push(format!("caps={}", caps.join(",")));
    }
    if parts.is_empty() {
        parts.push("(metadata only)".to_string());
    }
    parts.join(" ")
}

// ── providers keys (R4-3: multi-key rotation) ───────────────────────────

/// The `shannon providers keys <PROVIDER> …` action, resolved from the clap
/// subcommand by `main.rs`. Kept as a plain enum so the implementation is
/// testable without constructing clap types.
#[derive(Debug, Clone)]
pub enum ProvidersKeysAction {
    List,
    Add { key_ref: String },
    Remove { index: usize },
    Activate { index: usize },
}

/// Parse an `add` credential REFERENCE. Decision A1 (no plaintext on the
/// CLI): only references are accepted — `env:VAR_NAME` reads the variable at
/// invocation time, `store:SERVICE` copies the value of another stored
/// credential. A raw key is deliberately rejected; it belongs in
/// `/connect`/TUI, which write the credential store directly.
fn parse_key_ref(raw: &str) -> Result<(&str, &str)> {
    let trimmed = raw.trim();
    if let Some(var) = trimmed.strip_prefix("env:") {
        if var.trim().is_empty() {
            bail!("env: reference needs a variable name (e.g. env:ANTHROPIC_API_KEY_2)");
        }
        return Ok(("env", var.trim()));
    }
    if let Some(service) = trimmed.strip_prefix("store:") {
        if service.trim().is_empty() {
            bail!("store: reference needs a service name (e.g. store:anthropic-work)");
        }
        return Ok(("store", service.trim()));
    }
    bail!(
        "key reference must be `env:VAR_NAME` or `store:SERVICE` (got '{trimmed}'); \
         a raw key is never accepted on the CLI — enter it via /connect instead"
    );
}

/// Resolve a parsed key reference to its current value. `store:` references
/// are read from `dir` — the SAME credential store the manager mutates — so
/// the resolution honors hermetic (test) directories, not just `$HOME`.
fn resolve_key_ref(kind: &str, name: &str, dir: &std::path::Path) -> Result<String> {
    match kind {
        "env" => {
            let value = std::env::var(name)
                .map_err(|_| anyhow!("environment variable '{name}' is not set"))?;
            if value.trim().is_empty() {
                bail!("environment variable '{name}' is empty");
            }
            Ok(value)
        }
        "store" => {
            let value = shannon_core::credential_manager::read_credential_value(dir, name)
                .ok_or_else(|| anyhow!("no credential stored for service '{name}'"))?;
            if value.trim().is_empty() {
                bail!("stored credential '{name}' has an empty value");
            }
            Ok(value)
        }
        _ => unreachable!("parse_key_ref only yields env|store"),
    }
}

/// Mask a key for display: never the full value. Long keys keep a
/// recognizable head/tail; short ones collapse entirely.
fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "…".to_string();
    }
    let head: String = chars.iter().take(6).collect();
    let tail: String = chars.iter().rev().take(4).rev().collect();
    format!("{head}…{tail}")
}

/// Render the `providers keys list` table (fixed-width, same style as
/// `list-providers`). Rotation order: row 0 is the ACTIVE key.
fn render_keys_table(keys: &[String]) -> String {
    let headers = ["INDEX", "ACTIVE", "KEY"];
    let mut widths = [headers[0].len(), headers[1].len(), headers[2].len()];
    let mut lines: Vec<[String; 3]> = Vec::with_capacity(keys.len());
    for (i, key) in keys.iter().enumerate() {
        let cells = [
            i.to_string(),
            if i == 0 {
                "*".to_string()
            } else {
                String::new()
            },
            mask_key(key),
        ];
        for (c, w) in cells.iter().zip(widths.iter_mut()) {
            if c.chars().count() > *w {
                *w = c.chars().count();
            }
        }
        lines.push(cells);
    }
    let mut out = String::new();
    out.push_str(&format!(
        "{:<w0$}  {:<w1$}  {:<w2$}\n",
        headers[0],
        headers[1],
        headers[2],
        w0 = widths[0],
        w1 = widths[1],
        w2 = widths[2],
    ));
    for cells in &lines {
        out.push_str(&format!(
            "{:<w0$}  {:<w1$}  {:<w2$}\n",
            cells[0],
            cells[1],
            cells[2],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
        ));
    }
    out
}

/// The credential-service name a provider's profile points at, or an error
/// explaining that multi-key management needs a store credential.
fn credential_service_for(store: &ProviderConfigStore, provider_id: &str) -> Result<String> {
    let config = store.config();
    let profile = config
        .active_model_profile()
        .ok_or_else(|| anyhow!("no active profile in providers.toml"))?;
    let provider = profile
        .providers
        .iter()
        .find(|p| p.id == provider_id.trim())
        .ok_or_else(|| {
            anyhow!(
                "provider '{provider}' is not configured; see `shannon list-providers`",
                provider = provider_id.trim()
            )
        })?;
    match &provider.credential {
        CredentialRef::Store { service } => Ok(service.clone()),
        other => bail!(
            "provider '{provider}' uses a {other:?} credential; multi-key management \
             requires a credential-store entry (re-add with `shannon providers add \
             --api-key-ref <service>`)",
            provider = provider_id.trim()
        ),
    }
}

/// Run `shannon providers keys <PROVIDER> list|add|remove|activate`.
///
/// Reads the provider profile from `store` (read-only — the rotation list
/// lives in the credential store, not in providers.toml) and mutates the
/// service's credential entry through [`CredentialManager`], the same 0600
/// store `/connect` writes. Rotation order is the stored list order with the
/// ACTIVE key first; the engine consumes it on the next request.
pub fn run_providers_keys(
    store: &ProviderConfigStore,
    credentials: &mut CredentialManager,
    provider_id: &str,
    action: &ProvidersKeysAction,
) -> Result<()> {
    let service = credential_service_for(store, provider_id)?;
    // Sync the in-memory manager with disk so persist() rewrites exactly
    // what is on disk plus our mutation.
    credentials.load()?;

    match action {
        ProvidersKeysAction::List => match credentials.keys(&service) {
            Ok(keys) => {
                print!("{}", render_keys_table(&keys));
                if keys.len() > 1 {
                    println!(
                        "\nrotation: active key first; 401/persistent-429 walks down this list \
                         before any provider failover"
                    );
                }
            }
            Err(shannon_core::credential_manager::CredentialError::NotFound(_)) => {
                bail!(
                    "no credential stored for service '{service}'; connect the provider first \
                     (/connect or `shannon providers add`)"
                );
            }
            Err(e) => return Err(e.into()),
        },
        ProvidersKeysAction::Add { key_ref } => {
            let (kind, name) = parse_key_ref(key_ref)?;
            let value = resolve_key_ref(kind, name, credentials.dir())?;
            let count = credentials
                .add_key(&service, &value)
                .map_err(|e| map_key_error(e, &service))?;
            println!(
                "Added key {masked} to {provider} (store:{service}); {count} keys in rotation",
                masked = mask_key(&value),
                provider = provider_id.trim(),
                service = service,
            );
        }
        ProvidersKeysAction::Remove { index } => {
            let removed_active = *index == 0;
            credentials
                .remove_key(&service, *index)
                .map_err(|e| map_key_error(e, &service))?;
            let remaining = credentials.keys(&service).unwrap_or_default().len();
            println!(
                "Removed key at index {index} from {provider} (store:{service}); {remaining} remaining",
                provider = provider_id.trim(),
                service = service,
            );
            if removed_active && remaining > 0 {
                println!("  The next stored key is now active.");
            }
        }
        ProvidersKeysAction::Activate { index } => {
            credentials
                .activate_key(&service, *index)
                .map_err(|e| map_key_error(e, &service))?;
            let active = credentials
                .keys(&service)
                .ok()
                .and_then(|k| k.first().cloned())
                .unwrap_or_default();
            println!(
                "Activated key {masked} for {provider} (store:{service}); picked up on the next request",
                masked = mask_key(&active),
                provider = provider_id.trim(),
                service = service,
            );
        }
    }
    Ok(())
}

/// Humanize credential-store errors for the keys surface.
fn map_key_error(
    e: shannon_core::credential_manager::CredentialError,
    service: &str,
) -> anyhow::Error {
    use shannon_core::credential_manager::CredentialError;
    match e {
        CredentialError::NotFound(_) => anyhow!(
            "no credential stored for service '{service}'; connect the provider first \
             (/connect or `shannon providers add`)"
        ),
        other => anyhow!("{other}"),
    }
}

// ── providers export / import (R4-2: portable provider snapshots) ────────
//
// `shannon providers export` writes a portable snapshot of
// `~/.shannon/providers.toml`; `shannon providers import <FILE>` overlays it
// onto the live file. TOML in = TOML out: the snapshot is TOML because
// `providers.toml` is TOML, so the export reads like the file it reproduces.
//
// Portability story (decision A1): the snapshot carries credential
// REFERENCES — env var names / credential-store service names — never
// plaintext values. The references must resolve on the importing machine;
// import prints a checklist of which ones need attention.

/// Format marker embedded in every export and verified on import. Bump on a
/// breaking envelope change so old files get an actionable error instead of a
/// silent misparse.
pub const EXPORT_SCHEMA: &str = "shannon-providers-export/v1";

/// The literal string `--redact` writes in place of every credential
/// reference.
pub const REDACTED_MARKER: &str = "<redacted>";

/// The portable snapshot envelope. `schema`/`redacted` let the importer
/// reject foreign files and refuse redacted snapshots *before* attempting to
/// parse the payload, so each failure mode gets its own actionable error.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderExportFile {
    /// Always [`EXPORT_SCHEMA`].
    pub schema: String,
    /// True when [`REDACTED_MARKER`] replaced the credential references.
    pub redacted: bool,
    /// The snapshot payload: the full `ProviderModelConfig` (profiles,
    /// active profile pointer, active targets, tiers, per-model metadata,
    /// fallback models, auxiliary roles, gateway routing, credential refs).
    pub config: ProviderModelConfig,
}

/// Parameters for `shannon providers export`.
#[derive(Debug, Clone, Default)]
pub struct ExportArgs {
    /// Write the snapshot to this file instead of stdout.
    pub out: Option<std::path::PathBuf>,
    /// Mask credential references to [`REDACTED_MARKER`].
    pub redact: bool,
}

/// Parameters for `shannon providers import`.
#[derive(Debug, Clone, Default)]
pub struct ImportArgs {
    /// Path to the snapshot file (as written by `providers export`).
    pub file: String,
    /// Replace providers whose ids already exist instead of refusing.
    pub force: bool,
    /// Make this profile the active one after the merge.
    pub set_active: Option<String>,
}

/// Header comment block prepended to every export. Comments are legal TOML
/// and ignored by the parser, so the file parses as the envelope directly.
fn export_header(redacted: bool) -> String {
    let mut s = String::new();
    s.push_str(
        "# Shannon provider setup export — a portable snapshot of ~/.shannon/providers.toml.\n",
    );
    s.push_str("# Import on another machine with: shannon providers import <this-file>\n");
    s.push_str("# Credentials travel as REFERENCES only (env var names / credential-store\n");
    s.push_str("# service names) — never as secret values. References must resolve on the\n");
    s.push_str("# importing machine; `shannon providers import` prints a checklist of what\n");
    s.push_str("# needs attention.\n");
    if redacted {
        s.push_str(&format!(
            "# --redact: credential references were masked to \"{REDACTED_MARKER}\" — this file\n\
             # is for review/sharing, NOT for import (the references are gone).\n"
        ));
    }
    s
}

/// Build the export envelope from the store snapshot. Errors when there is
/// nothing to export — a snapshot of an empty config would import as
/// "nothing" and only confuse.
fn build_export_document(store: &ProviderConfigStore) -> Result<ProviderExportFile> {
    let config = store.config().clone();
    let total: usize = config.profiles.values().map(|mp| mp.providers.len()).sum();
    if total == 0 {
        bail!(
            "no providers configured in providers.toml; nothing to export \
             (connect one with /connect or `shannon providers add`)"
        );
    }
    Ok(ProviderExportFile {
        schema: EXPORT_SCHEMA.to_string(),
        redacted: false,
        config,
    })
}

/// Replace every `credential` table under `config.profiles.*.providers[*]`
/// with the [`REDACTED_MARKER`] string. Returns how many references were
/// masked. Targeted walk (not a generic recursion) so exactly the
/// credential-ref details are masked and nothing else in the snapshot moves.
fn redact_export_value(value: &mut toml::Value) -> usize {
    let Some(providers) = value
        .get_mut("config")
        .and_then(|c| c.get_mut("profiles"))
        .and_then(|p| p.as_table_mut())
        .map(|profiles| {
            profiles
                .iter_mut()
                .filter_map(|(_name, mp)| mp.get_mut("providers"))
                .filter_map(|v| v.as_array_mut())
                .collect::<Vec<_>>()
        })
    else {
        return 0;
    };
    let mut masked = 0;
    for arr in providers {
        for provider in arr.iter_mut() {
            if let Some(table) = provider.as_table_mut()
                && table.contains_key("credential")
            {
                table.insert(
                    "credential".to_string(),
                    toml::Value::String(REDACTED_MARKER.to_string()),
                );
                masked += 1;
            }
        }
    }
    masked
}

/// Render the export file: header comments + pretty TOML. With `redact`,
/// credential references are masked first and the envelope's `redacted` flag
/// is set so an importer refuses the file with the redaction explanation
/// instead of a confusing credential-parse error.
fn render_export_toml(doc: &ProviderExportFile, redact: bool) -> Result<String> {
    // Round-trip through a mutable `toml::Value` so --redact can rewrite the
    // credential tables in place.
    let serialized = toml::to_string_pretty(doc).context("export payload is not serializable")?;
    let mut value: toml::Value =
        toml::from_str(&serialized).context("export payload is not valid TOML")?;
    if redact {
        let masked = redact_export_value(&mut value);
        debug_assert_eq!(
            masked,
            doc.config
                .profiles
                .values()
                .map(|mp| mp.providers.len())
                .sum::<usize>(),
            "redaction must mask every credential reference"
        );
        if let Some(envelope) = value.as_table_mut() {
            envelope.insert("redacted".to_string(), toml::Value::Boolean(true));
        }
    }
    let body =
        toml::to_string_pretty(&value).context("redacted export payload is not serializable")?;
    let mut out = export_header(redact);
    out.push_str(&body);
    Ok(out)
}

/// Parse + validate an export file body. Every failure names the fix: missing
/// schema marker (foreign file), unknown schema (wrong version), redacted
/// snapshot (refs gone), payload parse error, wrong payload version, invalid
/// per-model metadata (the same semantic check `providers.toml` load applies).
fn parse_export_document(text: &str) -> Result<ProviderExportFile> {
    let value: toml::Value = toml::from_str(text)
        .context("file is not valid TOML; expected a `shannon providers export` snapshot")?;
    match value.get("schema").and_then(|v| v.as_str()) {
        None => bail!(
            "missing the `schema` marker — this does not look like a file written by \
             `shannon providers export`; re-create it with `shannon providers export --out <file>`"
        ),
        Some(s) if s == EXPORT_SCHEMA => {}
        Some(other) => bail!(
            "unsupported export schema '{other}' (this binary understands '{EXPORT_SCHEMA}'); \
             re-export from a Shannon version that wrote a compatible file"
        ),
    }
    if value
        .get("redacted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        bail!(
            "this snapshot was exported with --redact: credential references were masked to \
             \"{REDACTED_MARKER}\", so the providers in it cannot authenticate. Re-export \
             without --redact (the plain export still carries references only, never secret values)"
        );
    }
    let doc: ProviderExportFile = value.try_into().map_err(|e| {
        anyhow!(
            "the export payload failed to parse as a provider config: {e}; \
             re-create the file with `shannon providers export`"
        )
    })?;
    if doc.config.version != ProviderModelConfig::VERSION {
        bail!(
            "export payload has schema version {} but this binary understands version {}; \
             re-export from a compatible Shannon version",
            doc.config.version,
            ProviderModelConfig::VERSION
        );
    }
    doc.config
        .validate_models()
        .map_err(|e| anyhow!("the export payload carries invalid per-model metadata: {e}"))?;
    Ok(doc)
}

/// Run `shannon providers export [--out <FILE>] [--redact]`.
///
/// Default output is stdout (redirect-friendly: `shannon providers export >
/// providers-snapshot.toml`); the human summary always goes to stderr so a
/// redirected stdout carries nothing but the TOML. Files are written 0600 —
/// the snapshot reveals endpoint URLs and credential *names*, which is less
/// than providers.toml itself exposes, but there is no reason to publish it.
pub fn run_providers_export(store: &ProviderConfigStore, args: &ExportArgs) -> Result<()> {
    let doc = build_export_document(store)?;
    let text = render_export_toml(&doc, args.redact)?;
    let profile_count = doc.config.profiles.len();
    let provider_count: usize = doc
        .config
        .profiles
        .values()
        .map(|mp| mp.providers.len())
        .sum();

    match &args.out {
        Some(path) => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("cannot create directory {}", parent.display()))?;
            }
            std::fs::write(path, &text)
                .with_context(|| format!("cannot write {}", path.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            }
            eprintln!(
                "Exported {provider_count} provider(s) across {profile_count} profile(s) to {}",
                path.display()
            );
            eprintln!(
                "  schema: {EXPORT_SCHEMA} (TOML; credential references only — no secret values)"
            );
            if args.redact {
                eprintln!(
                    "  --redact: credential references masked to \"{REDACTED_MARKER}\"; \
                     this file cannot be re-imported — re-connect providers on the target machine"
                );
            }
        }
        None => {
            print!("{text}");
            eprintln!(
                "exported {provider_count} provider(s) across {profile_count} profile(s); \
                 schema {EXPORT_SCHEMA} (summary on stderr — stdout is the snapshot)"
            );
        }
    }
    Ok(())
}

/// How one credential reference resolves against the importing machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CredRefStatus {
    /// The reference points at something this machine can read today.
    Resolved,
    /// Known backend, but the referenced value is absent — the provider will
    /// not authenticate until the user supplies it.
    Unresolved,
    /// Cannot be verified programmatically — needs a human check.
    Verify,
}

/// Resolve one credential reference against `cred_dir` (the credential store
/// directory; `None` uses the default `~/.shannon/credentials`). Never reads
/// or prints the secret value — only whether one exists.
fn check_credential_ref(
    cred: &CredentialRef,
    cred_dir: Option<&std::path::Path>,
) -> (CredRefStatus, String) {
    match cred {
        CredentialRef::Store { service } => {
            let found = match cred_dir {
                Some(dir) => {
                    shannon_core::credential_manager::read_credential_value(dir, service).is_some()
                }
                None => shannon_core::credential_manager::read_credential_value_default(service)
                    .is_some(),
            };
            if found {
                (
                    CredRefStatus::Resolved,
                    format!("store:{service} — credential found"),
                )
            } else {
                (
                    CredRefStatus::Unresolved,
                    format!(
                        "store:{service} — no stored credential on this machine; connect the \
                         provider (/connect) or reuse another stored key \
                         (`shannon providers keys <provider> add store:<other-service>`)"
                    ),
                )
            }
        }
        CredentialRef::Env { var } => match std::env::var(var) {
            Ok(v) if !v.trim().is_empty() => (CredRefStatus::Resolved, format!("env:{var} — set")),
            _ => (
                CredRefStatus::Unresolved,
                format!("env:{var} — not set in this environment; export it before use"),
            ),
        },
        CredentialRef::Keyring { service, account } => (
            CredRefStatus::Verify,
            format!(
                "keyring:{service}/{account} — verify the keyring entry exists on this machine"
            ),
        ),
        CredentialRef::InlineLegacy { .. } => (
            CredRefStatus::Unresolved,
            "legacy masked credential — re-enter the key via /connect".to_string(),
        ),
        CredentialRef::Ephemeral => (
            CredRefStatus::Verify,
            "ephemeral — the key must be injected per session (never persisted)".to_string(),
        ),
    }
}

/// Build the post-import checklist lines: one per imported provider slot,
/// flagged `[ok]` / `[missing]` / `[check]`. Import never touches the
/// credential store — this list is the "which refs need attention" contract.
fn build_import_checklist(
    config: &ProviderModelConfig,
    cred_dir: Option<&std::path::Path>,
) -> Vec<String> {
    let mut lines = Vec::new();
    for name in config.profile_names() {
        let Some(mp) = config.profiles.get(&name) else {
            continue;
        };
        for provider in &mp.providers {
            let (status, desc) = check_credential_ref(&provider.credential, cred_dir);
            let tag = match status {
                CredRefStatus::Resolved => "ok",
                CredRefStatus::Unresolved => "missing",
                CredRefStatus::Verify => "check",
            };
            lines.push(format!("[{tag}] {name}/{} — {desc}", provider.id));
        }
    }
    lines
}

/// Run `shannon providers import <FILE> [--force] [--set-active <PROFILE>]`.
///
/// Semantics:
/// - validates the file envelope ([`parse_export_document`]) — foreign or
///   invalid files are refused with actionable errors, redacted snapshots
///   are refused because their references are gone;
/// - default: refuses to overwrite provider ids that already exist in the
///   live file (lists them; `--force` replaces those slots wholesale);
/// - merges additively otherwise: new profiles land verbatim, existing
///   profiles gain the snapshot's new provider slots;
/// - `--set-active <PROFILE>` repoints the active profile; without it the
///   snapshot's pointer is adopted only on a machine with no connected
///   providers (fresh-machine round-trip);
/// - writes `providers.toml` through [`ProviderConfigService`] and NEVER
///   touches the credential store — the post-import checklist reports which
///   credential references resolve and which need attention.
pub fn run_providers_import(
    store: &mut ProviderConfigStore,
    args: &ImportArgs,
    cred_dir: Option<&std::path::Path>,
) -> Result<()> {
    let text = std::fs::read_to_string(&args.file)
        .with_context(|| format!("cannot read '{}'", args.file))?;
    let doc = parse_export_document(&text)
        .with_context(|| format!("'{}' is not a usable Shannon provider export", args.file))?;
    let total: usize = doc
        .config
        .profiles
        .values()
        .map(|mp| mp.providers.len())
        .sum();
    if total == 0 {
        bail!("the snapshot contains no providers; nothing to import");
    }

    // Pre-flight conflict report against the caller's snapshot so a refusal
    // happens before any lock is taken; the service re-checks under the flock
    // (post-reload) so a concurrent writer cannot slip a conflict past us.
    let conflicts = store.import_conflicts(&doc.config);
    if !conflicts.is_empty() && !args.force {
        let list = conflicts
            .iter()
            .map(|(profile, id)| format!("{profile}/{id}"))
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "refusing to import: these provider ids already exist in providers.toml: {list}\n  \
             re-run with --force to replace them, or remove them first \
             (`shannon providers remove <ID>`)"
        );
    }

    // Pre-flight --set-active name check (the service re-validates with
    // provider-count semantics after the merge).
    if let Some(name) = &args.set_active {
        let known =
            doc.config.profiles.contains_key(name) || store.config().profiles.contains_key(name);
        if !known {
            bail!(
                "--set-active '{name}': no profile with that name exists in the snapshot \
                 or in providers.toml"
            );
        }
    }

    // Whether the live file had any connected provider before the import —
    // the snapshot's active-profile pointer is adopted only on a fresh
    // machine, and the summary line reports that provenance.
    let live_was_fresh = !store
        .config()
        .profiles
        .values()
        .any(|mp| !mp.providers.is_empty());

    let mut svc = ProviderConfigService::from_store(std::mem::take(store));
    let outcome = svc
        .import_snapshot(&doc.config, args.force, args.set_active.as_deref())
        .with_context(|| "failed to persist providers.toml")?;
    *store = svc.into_inner();

    // ── summary ──
    println!(
        "Imported provider snapshot from {} ({}):",
        args.file, doc.schema
    );
    for profile in &outcome.summary.profiles_added {
        println!("  profile added: {profile}");
    }
    println!(
        "  providers: {} added, {} replaced",
        outcome.summary.providers_added, outcome.summary.providers_replaced
    );
    let pointer_source = if args.set_active.is_some() {
        "--set-active"
    } else if live_was_fresh {
        "from the snapshot (fresh machine)"
    } else {
        "unchanged"
    };
    println!(
        "  active profile: {} ({pointer_source})",
        outcome.summary.active_profile
    );
    println!("  persisted to: {}", outcome.saved_path.display());

    // ── post-import checklist ──
    let checklist = build_import_checklist(store.config(), cred_dir);
    println!();
    println!(
        "Post-import checklist — credential references were imported, values were not; \
         each must resolve on this machine:"
    );
    for line in &checklist {
        println!("  {line}");
    }
    let attention = checklist
        .iter()
        .filter(|l| l.contains("[missing]") || l.contains("[check]"))
        .count();
    if attention > 0 {
        println!();
        println!(
            "{attention} credential reference(s) need attention before the affected \
             providers will authenticate."
        );
    }
    Ok(())
}

/// Public wrapper around [`parse_kind`] used by the CLI dispatch in
/// `main.rs` (where clap hands us a `String`) and by the unit tests.
pub fn parse_kind_cli(s: &str) -> Result<ProviderKind> {
    parse_kind(s)
}

/// Public wrapper around [`validate_canonical_tier`]; exposed for the
/// unit tests (clap-parseable validation flows already call the inner
/// helper via [`run_providers_add`]).
#[allow(dead_code)] // KEEP: thin public wrapper over `validate_canonical_tier`, retained for unit-test coverage of the CLI-facing validation surface.
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
            models: Vec::new(),
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

    // ── providers model-meta ─────────────────────────────────────────

    fn meta_args(model: &str, caps: &[&str]) -> ModelMetaArgs {
        ModelMetaArgs {
            provider: "glm".to_string(),
            model: model.to_string(),
            display_name: None,
            context: None,
            max_output: None,
            price_in: None,
            price_out: None,
            cap: caps.iter().map(|s| s.to_string()).collect(),
            remove: false,
        }
    }

    #[test]
    fn parse_capability_accepts_all_canonical_names_case_insensitively() {
        assert_eq!(parse_capability("vision").unwrap(), ModelCapability::Vision);
        assert_eq!(
            parse_capability("REASONING").unwrap(),
            ModelCapability::Reasoning
        );
        assert_eq!(
            parse_capability(" coding ").unwrap(),
            ModelCapability::Coding
        );
        assert_eq!(parse_capability("Speed").unwrap(), ModelCapability::Speed);
        assert_eq!(parse_capability("cheap").unwrap(), ModelCapability::Cheap);
    }

    #[test]
    fn parse_capability_rejects_unknown_with_accepted_list() {
        let err = parse_capability("visionn").expect_err("must reject");
        let msg = format!("{err}");
        assert!(
            msg.contains("visionn") && msg.contains("reasoning") && msg.contains("vision"),
            "error must name the bad value and the accepted set: {msg}"
        );
    }

    #[test]
    fn build_model_meta_spec_fresh_from_flags() {
        let mut args = meta_args("glm-5.3-flash", &["vision"]);
        args.context = Some(198_000);
        args.max_output = Some(32_768);
        args.price_in = Some(0.5);
        args.price_out = Some(2.0);
        let spec = build_model_meta_spec(None, &args).expect("must build");
        assert_eq!(spec.id, "glm-5.3-flash");
        assert_eq!(spec.context_window, Some(198_000));
        assert_eq!(spec.max_output, Some(32_768));
        assert_eq!(spec.cost_per_m_input, Some(0.5));
        assert_eq!(spec.cost_per_m_output, Some(2.0));
        assert_eq!(spec.capabilities, vec![ModelCapability::Vision]);
    }

    #[test]
    fn build_model_meta_spec_merges_into_existing_entry() {
        let mut existing = build_model_meta_spec(
            None,
            &ModelMetaArgs {
                provider: "glm".into(),
                model: "glm-4.6".into(),
                display_name: Some("GLM".into()),
                context: Some(198_000),
                max_output: Some(8_192),
                price_in: Some(0.6),
                price_out: Some(2.2),
                cap: vec!["vision".into()],
                remove: false,
            },
        )
        .expect("seed spec");

        // Update ONLY the context window: every other value survives.
        let mut args = meta_args("glm-4.6", &[]);
        args.context = Some(200_000);
        let merged = build_model_meta_spec(Some(&existing), &args).expect("merge must succeed");
        assert_eq!(merged.context_window, Some(200_000));
        assert_eq!(merged.display_name.as_deref(), Some("GLM"));
        assert_eq!(merged.max_output, Some(8_192));
        assert_eq!(merged.cost_per_m_input, Some(0.6));
        assert_eq!(merged.cost_per_m_output, Some(2.2));
        assert_eq!(merged.capabilities, vec![ModelCapability::Vision]);
        existing.context_window = Some(200_000);
        assert_eq!(merged, existing);
    }

    #[test]
    fn build_model_meta_spec_rejects_bad_numbers_and_empty_ids() {
        // Zero context.
        let mut args = meta_args("m", &[]);
        args.context = Some(0);
        assert!(build_model_meta_spec(None, &args).is_err());

        // Negative price.
        let mut args = meta_args("m", &[]);
        args.price_in = Some(-0.5);
        assert!(build_model_meta_spec(None, &args).is_err());

        // Empty model id.
        let args = meta_args("", &[]);
        assert!(build_model_meta_spec(None, &args).is_err());

        // Empty provider id.
        let mut args = meta_args("m", &[]);
        args.provider = "  ".to_string();
        assert!(build_model_meta_spec(None, &args).is_err());
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

    // ── providers keys (R4-3) ────────────────────────────────────────────

    /// Hermetic keys fixture: a provider profile (`Store` credential, service
    /// `glm`) in an in-memory store, plus a credential manager rooted at a
    /// temp dir holding the seeded multi-key entry. No HOME swapping — the
    /// manager's dir is injected.
    struct KeysFixture {
        store: ProviderConfigStore,
        dir: tempfile::TempDir,
    }

    impl KeysFixture {
        fn new() -> Self {
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
            let dir = tempfile::tempdir().expect("tempdir");
            let mgr = Self { store, dir };
            // Seed the active key exactly like a /connect write would.
            let mut credentials = mgr.manager();
            credentials
                .store_or_update(shannon_core::credential_manager::Credential::new(
                    "glm",
                    "glm",
                    "sk-glm-primary",
                ))
                .expect("seed credential");
            mgr
        }

        fn manager(&self) -> CredentialManager {
            CredentialManager::with_dir(self.dir.path().to_path_buf()).expect("manager")
        }
    }

    #[test]
    fn providers_keys_parse_ref_accepts_only_references() {
        assert_eq!(parse_key_ref("env:MY_KEY").unwrap(), ("env", "MY_KEY"));
        assert_eq!(parse_key_ref(" store:x ").unwrap(), ("store", "x"));
        assert!(parse_key_ref("env:").is_err(), "empty var rejected");
        assert!(parse_key_ref("store:").is_err(), "empty service rejected");
        let err =
            parse_key_ref("sk-ant-raw-key-material").expect_err("raw keys must be rejected (A1)");
        let msg = format!("{err}");
        assert!(
            msg.contains("env:VAR_NAME") && msg.contains("/connect"),
            "error must explain the accepted forms and the /connect path: {msg}"
        );
    }

    #[test]
    fn providers_keys_mask_never_shows_the_full_key() {
        let masked = mask_key("sk-ant-api03-verylongsecretvalue");
        assert!(masked.starts_with("sk-ant"));
        assert!(masked.ends_with("alue"));
        assert!(!masked.contains("verylongsecret"), "{masked}");
        assert_eq!(mask_key("short"), "…", "short keys collapse entirely");
    }

    #[test]
    fn providers_keys_list_marks_active_first_and_masks() {
        let table = render_keys_table(&[
            "sk-glm-primary-long".to_string(),
            "sk-glm-secondary-long".to_string(),
        ]);
        let rows: Vec<&str> = table.lines().collect();
        assert_eq!(rows.len(), 3, "header + 2 keys: {table}");
        assert!(rows[0].contains("INDEX") && rows[0].contains("ACTIVE"));
        assert!(rows[1].starts_with("0") && rows[1].contains('*'));
        assert!(!rows[2].contains('*'));
        assert!(
            !table.contains("sk-glm-primary-long") && !table.contains("sk-glm-secondary-long"),
            "raw keys must never reach the table: {table}"
        );
    }

    #[test]
    fn providers_keys_requires_a_store_backed_profile() {
        let mut store = ProviderConfigStore::default();
        let mut env_profile = sample_profile(
            "envprov",
            ProviderKind::OpenAiCompatible,
            "https://x.example.com",
            "m",
        );
        env_profile.credential = CredentialRef::Env {
            var: "SOME_VAR".to_string(),
        };
        store.upsert_profile(env_profile, "m");

        let err = credential_service_for(&store, "envprov").expect_err("env-backed must fail");
        assert!(
            format!("{err:#}").contains("credential-store entry"),
            "{err:#}"
        );
        let err = credential_service_for(&store, "ghost").expect_err("unknown id must fail");
        assert!(format!("{err:#}").contains("not configured"), "{err:#}");
    }

    #[test]
    fn providers_keys_add_list_activate_remove_round_trip() {
        let fx = KeysFixture::new();

        // add env:VAR — the A1-sanctioned CLI path for a second key.
        // SAFETY: unique var name owned by this test.
        unsafe { std::env::set_var("SHANNON_KEYS_TEST_SECONDARY", "sk-glm-secondary") };
        let mut credentials = fx.manager();
        run_providers_keys(
            &fx.store,
            &mut credentials,
            "glm",
            &ProvidersKeysAction::Add {
                key_ref: "env:SHANNON_KEYS_TEST_SECONDARY".to_string(),
            },
        )
        .expect("add env ref must succeed");
        // SAFETY: see above.
        unsafe { std::env::remove_var("SHANNON_KEYS_TEST_SECONDARY") };

        // add store:other — copy from another credential file in the store.
        credentials
            .store_or_update(shannon_core::credential_manager::Credential::new(
                "glm-work",
                "glm-work",
                "sk-glm-work",
            ))
            .expect("seed second service");
        run_providers_keys(
            &fx.store,
            &mut credentials,
            "glm",
            &ProvidersKeysAction::Add {
                key_ref: "store:glm-work".to_string(),
            },
        )
        .expect("add store ref must succeed");

        // Rotation order: active first, additions in order.
        assert_eq!(
            credentials.keys("glm").unwrap(),
            vec!["sk-glm-primary", "sk-glm-secondary", "sk-glm-work"]
        );

        // Duplicate add is rejected.
        let mut credentials2 = fx.manager();
        credentials2.load().unwrap();
        let err = run_providers_keys(
            &fx.store,
            &mut credentials2,
            "glm",
            &ProvidersKeysAction::Add {
                key_ref: "store:glm-work".to_string(),
            },
        )
        .expect_err("duplicate must be rejected");
        assert!(format!("{err:#}").contains("already registered"), "{err:#}");

        // activate(2) → work key becomes active (slot 0).
        run_providers_keys(
            &fx.store,
            &mut credentials2,
            "glm",
            &ProvidersKeysAction::Activate { index: 2 },
        )
        .expect("activate must succeed");
        assert_eq!(
            credentials2.keys("glm").unwrap(),
            vec!["sk-glm-work", "sk-glm-primary", "sk-glm-secondary"]
        );

        // remove(0) → removes the active work key, primary is promoted back.
        run_providers_keys(
            &fx.store,
            &mut credentials2,
            "glm",
            &ProvidersKeysAction::Remove { index: 0 },
        )
        .expect("remove must succeed");
        assert_eq!(
            credentials2.keys("glm").unwrap(),
            vec!["sk-glm-primary", "sk-glm-secondary"]
        );

        // Out-of-range remove is a clean error.
        let err = run_providers_keys(
            &fx.store,
            &mut credentials2,
            "glm",
            &ProvidersKeysAction::Remove { index: 9 },
        )
        .expect_err("out of range must fail");
        assert!(format!("{err:#}").contains("out of range"), "{err:#}");

        // State persists for the next process (the engine reads the file).
        let mut reloaded = fx.manager();
        reloaded.load().unwrap();
        assert_eq!(
            reloaded.keys("glm").unwrap(),
            vec!["sk-glm-primary", "sk-glm-secondary"]
        );
    }

    #[test]
    fn providers_keys_on_missing_credential_file_is_a_clean_error() {
        let fx = KeysFixture::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let mut credentials =
            CredentialManager::with_dir(dir.path().to_path_buf()).expect("manager");
        for action in [
            ProvidersKeysAction::List,
            ProvidersKeysAction::Add {
                key_ref: "store:whatever".to_string(),
            },
        ] {
            let err = run_providers_keys(&fx.store, &mut credentials, "glm", &action)
                .expect_err("missing credential must fail cleanly");
            assert!(
                format!("{err:#}").contains("no credential stored"),
                "{err:#}"
            );
        }
    }

    // ── providers export / import (R4-2) ─────────────────────────────────

    /// A provider profile exercising every snapshot-relevant field: custom
    /// credential service name, extra header, tier, fallback models and a
    /// per-model metadata declaration.
    fn rich_profile(id: &str, service: &str, base_url: &str, model: &str) -> ProviderProfile {
        ProviderProfile {
            id: id.to_string(),
            kind: ProviderKind::OpenAiCompatible,
            display_name: format!("{id}-display"),
            base_url: base_url.to_string(),
            models_url: None,
            credential: CredentialRef::Store {
                service: service.to_string(),
            },
            extra_headers: HashMap::from([("X-Team".to_string(), "alpha".to_string())]),
            default_max_tokens: Some(4096),
            fallback_models: vec![format!("{model}-fallback")],
            quirks: Default::default(),
            tiers: ProviderTiers {
                standard: Some(model.to_string()),
                ..Default::default()
            },
            models: vec![ModelSpec {
                id: model.to_string(),
                display_name: Some("GLM".to_string()),
                context_window: Some(198_000),
                max_output: Some(32_768),
                cost_per_m_input: Some(0.5),
                cost_per_m_output: Some(2.0),
                capabilities: vec![ModelCapability::Vision],
            }],
        }
    }

    /// Land `profiles` in a fresh hermetic store at `path` (via the service
    /// — the real write path), each becoming active in order.
    fn seed_store(path: &std::path::Path, profiles: Vec<ProviderProfile>, model: &str) {
        let mut svc = ProviderConfigService::load_at(path);
        for profile in profiles {
            svc.upsert(profile, model, true).expect("seed upsert");
        }
    }

    #[test]
    fn export_render_carries_schema_header_and_full_payload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("providers.toml");
        seed_store(
            &path,
            vec![rich_profile(
                "glm",
                "glm-svc",
                "https://open.bigmodel.cn/v1",
                "glm-4.6",
            )],
            "glm-4.6",
        );
        let store = ProviderConfigStore::load_or_default_at(&path);
        let doc = build_export_document(&store).expect("document builds");
        assert_eq!(doc.schema, EXPORT_SCHEMA);
        assert!(!doc.redacted);

        let text = render_export_toml(&doc, false).expect("renders");
        assert!(
            text.starts_with("# Shannon provider setup export"),
            "{text}"
        );
        assert!(
            text.contains(&format!("schema = \"{EXPORT_SCHEMA}\"")),
            "{text}"
        );
        assert!(text.contains("redacted = false"), "{text}");
        // Payload breadth: profile fields, credential ref, tier, fallback,
        // per-model metadata all travel.
        assert!(
            text.contains("[[config.profiles.default.providers]]"),
            "{text}"
        );
        assert!(text.contains("service = \"glm-svc\""), "{text}");
        assert!(text.contains("backend = \"store\""), "{text}");
        assert!(text.contains("glm-4.6-fallback"), "{text}");
        assert!(text.contains("cost_per_m_input = 0.5"), "{text}");
        assert!(text.contains("X-Team"), "{text}");
        // Active target travels too.
        assert!(text.contains("model_id = \"glm-4.6\""), "{text}");
        // Empty store refuses: nothing to export.
        let empty = ProviderConfigStore::default();
        assert!(build_export_document(&empty).is_err());
    }

    #[test]
    fn export_redaction_masks_every_credential_ref() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("providers.toml");
        let mut env_profile = rich_profile("ci", "ci-svc", "https://ci.example.com", "ci-model");
        env_profile.credential = CredentialRef::Env {
            var: "SHANNON_EXPORT_TEST_VAR".to_string(),
        };
        seed_store(
            &path,
            vec![
                rich_profile("glm", "glm-svc", "https://open.bigmodel.cn/v1", "glm-4.6"),
                env_profile,
            ],
            "glm-4.6",
        );
        let store = ProviderConfigStore::load_or_default_at(&path);
        let doc = build_export_document(&store).expect("document builds");

        let plain = render_export_toml(&doc, false).expect("plain renders");
        // Grep-level no-plaintext guarantee: refs (names) may appear; a
        // secret VALUE cannot (decision A1 — the schema has nowhere to put
        // one). The masked legacy credential of a snapshot is masked by
        // construction; assert the marker contract on the redacted file.
        assert!(
            plain.contains("glm-svc"),
            "plain export keeps ref names: {plain}"
        );

        let redacted = render_export_toml(&doc, true).expect("redacted renders");
        assert!(redacted.contains("redacted = true"), "{redacted}");
        assert!(redacted.contains("<redacted>"), "{redacted}");
        assert!(
            !redacted.contains("glm-svc") && !redacted.contains("SHANNON_EXPORT_TEST_VAR"),
            "credential ref details must be gone: {redacted}"
        );
        assert_eq!(
            redacted.matches("credential = \"<redacted>\"").count(),
            2,
            "exactly one masked ref per provider (the header mention aside)"
        );
        // Redacted files are refused on import (see parse test below).
    }

    #[test]
    fn export_import_round_trip_reproduces_resolution() {
        let dir = tempfile::tempdir().expect("tempdir");
        let original_path = dir.path().join("providers.toml");
        seed_store(
            &original_path,
            vec![
                rich_profile("glm", "glm-svc", "https://open.bigmodel.cn/v1", "glm-4.6"),
                rich_profile("kimi", "kimi-svc", "https://api.moonshot.cn/v1", "k2"),
            ],
            "k2",
        );
        let original = ProviderConfigStore::load_or_default_at(&original_path);
        let doc = build_export_document(&original).expect("document builds");
        let text = render_export_toml(&doc, false).expect("renders");

        // Fresh machine: a brand-new path, no providers.toml at all.
        let fresh_dir = tempfile::tempdir().expect("tempdir");
        let fresh_path = fresh_dir.path().join("providers.toml");
        let mut fresh = ProviderConfigStore::load_or_default_at(&fresh_path);
        let args = ImportArgs {
            file: "snapshot.toml".to_string(),
            force: false,
            set_active: None,
        };
        run_providers_import(&mut fresh, &args, None).expect_err("missing file is an error");
        // (the line above proves the file is read from disk; now write it and
        // import for real)
        std::fs::write(fresh_dir.path().join("snapshot.toml"), &text).expect("write snapshot");
        let args = ImportArgs {
            file: fresh_dir.path().join("snapshot.toml").display().to_string(),
            force: false,
            set_active: None,
        };
        run_providers_import(&mut fresh, &args, None).expect("import succeeds");

        // Same resolution: byte-semantic equality of the whole config
        // (profiles, active pointer, targets, tiers, metadata, refs).
        assert_eq!(
            fresh.config(),
            original.config(),
            "import on a fresh machine must reproduce the snapshot"
        );
        assert_eq!(
            fresh.config().active_profile_key(),
            "default",
            "active pointer round-trips"
        );
    }

    #[test]
    fn import_refuses_existing_ids_without_force_and_replaces_with_force() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source_path = dir.path().join("source.toml");
        seed_store(
            &source_path,
            vec![
                rich_profile("glm", "glm-svc", "https://open.bigmodel.cn/v1", "glm-4.6"),
                rich_profile("kimi", "kimi-svc", "https://api.moonshot.cn/v1", "k2"),
            ],
            "k2",
        );
        let doc = build_export_document(&ProviderConfigStore::load_or_default_at(&source_path))
            .expect("document builds");

        // Target machine already has a DIFFERENT glm.
        let target_dir = tempfile::tempdir().expect("tempdir");
        let target_path = target_dir.path().join("providers.toml");
        seed_store(
            &target_path,
            vec![rich_profile(
                "glm",
                "glm-svc",
                "https://old.example.com",
                "old-model",
            )],
            "old-model",
        );
        let mut target = ProviderConfigStore::load_or_default_at(&target_path);

        // Serialize the snapshot where the command can read it.
        let text = render_export_toml(&doc, false).expect("renders");
        std::fs::write(target_dir.path().join("snapshot.toml"), &text).expect("write");

        // Default: refuse, listing the conflicts, without writing.
        let err = run_providers_import(
            &mut target,
            &ImportArgs {
                file: target_dir
                    .path()
                    .join("snapshot.toml")
                    .display()
                    .to_string(),
                force: false,
                set_active: None,
            },
            None,
        )
        .expect_err("conflict must refuse");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("already exist") && msg.contains("default/glm"),
            "{msg}"
        );
        assert_eq!(
            target.config().profiles["default"].providers[0].base_url,
            "https://old.example.com",
            "refused import must not touch the live file"
        );

        // --force: glm replaced wholesale, kimi added.
        run_providers_import(
            &mut target,
            &ImportArgs {
                file: target_dir
                    .path()
                    .join("snapshot.toml")
                    .display()
                    .to_string(),
                force: true,
                set_active: None,
            },
            None,
        )
        .expect("forced import succeeds");
        let default = &target.config().profiles["default"];
        let glm = default
            .providers
            .iter()
            .find(|p| p.id == "glm")
            .expect("glm still present");
        assert_eq!(glm.base_url, "https://open.bigmodel.cn/v1", "replaced");
        assert_eq!(glm.tiers.standard.as_deref(), Some("glm-4.6"));
        assert!(
            default.providers.iter().any(|p| p.id == "kimi"),
            "kimi added"
        );
    }

    #[test]
    fn parse_export_document_rejects_foreign_and_invalid_files() {
        // Not TOML at all.
        assert!(parse_export_document("hello world").is_err());
        // Valid TOML, but not an export (no schema marker) — e.g. a raw
        // providers.toml copy.
        let raw_providers = "version = 2\n[profiles]\n";
        let err = parse_export_document(raw_providers).expect_err("foreign file refused");
        assert!(format!("{err:#}").contains("schema"), "{err:#}");
        // Wrong schema version marker.
        let err = parse_export_document("schema = \"something-else/v9\"\n")
            .expect_err("unknown schema refused");
        assert!(
            format!("{err:#}").contains(EXPORT_SCHEMA),
            "error names the understood schema: {err:#}"
        );
        // Redacted snapshots are refused with the redaction explanation.
        let redacted = format!(
            "schema = \"{EXPORT_SCHEMA}\"\nredacted = true\n[config]\nversion = 2\n[config.profiles]\n"
        );
        let err = parse_export_document(&redacted).expect_err("redacted refused");
        assert!(
            format!("{err:#}").contains("--redact") && format!("{err:#}").contains(REDACTED_MARKER),
            "{err:#}"
        );
        // Wrong payload version.
        let bad_version = format!(
            "schema = \"{EXPORT_SCHEMA}\"\nredacted = false\n[config]\nversion = 99\n[config.profiles]\n"
        );
        let err = parse_export_document(&bad_version).expect_err("version gate");
        assert!(format!("{err:#}").contains("version 99"), "{err:#}");
        // Semantically invalid payload (duplicate per-model declarations) —
        // the same validation `providers.toml` load applies.
        let dup = format!(
            "schema = \"{EXPORT_SCHEMA}\"\nredacted = false\n\
             [config]\nversion = 2\n\
             [config.profiles.default]\nname = \"default\"\n\
             [config.profiles.default.active_target]\nprovider_id = \"p\"\nmodel_id = \"m\"\nscope = \"global\"\n\
             [[config.profiles.default.providers]]\nid = \"p\"\nkind = \"openai\"\ndisplay_name = \"p\"\nbase_url = \"https://x\"\n\
             [config.profiles.default.providers.credential]\nbackend = \"store\"\nservice = \"p\"\n\
             [[config.profiles.default.providers.models]]\nid = \"m\"\n\
             [[config.profiles.default.providers.models]]\nid = \"m\"\n"
        );
        let err = parse_export_document(&dup).expect_err("invalid metadata refused");
        assert!(
            format!("{err:#}").contains("duplicate model declaration"),
            "{err:#}"
        );
    }

    #[test]
    fn import_set_active_switches_pointer_and_garbage_name_refuses() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source_path = dir.path().join("providers.toml");
        // Seed TWO profiles: default (glm) + work (kimi).
        let mut store = ProviderConfigStore::load_or_default_at(&source_path);
        store.upsert_profile(
            rich_profile("glm", "glm-svc", "https://x.example.com", "m1"),
            "m1",
        );
        store.insert_model_profile("work").expect("work profile");
        store.set_active_profile_key("work");
        store.upsert_profile(
            rich_profile("kimi", "kimi-svc", "https://y.example.com", "m2"),
            "m2",
        );
        store.save().expect("persist fixture");

        let doc = build_export_document(&store).expect("document builds");
        assert!(
            doc.config.profiles.contains_key("work"),
            "fixture has two profiles"
        );

        // Fresh machine: --set-active work makes work active post-import.
        let target_dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            target_dir.path().join("snap.toml"),
            render_export_toml(&doc, false).expect("renders"),
        )
        .expect("write snapshot");
        let mut fresh =
            ProviderConfigStore::load_or_default_at(&target_dir.path().join("providers.toml"));
        run_providers_import(
            &mut fresh,
            &ImportArgs {
                file: target_dir.path().join("snap.toml").display().to_string(),
                force: false,
                set_active: Some("work".to_string()),
            },
            None,
        )
        .expect("import with --set-active succeeds");
        assert_eq!(fresh.config().active_profile_key(), "work");

        // Garbage --set-active name refuses before any write.
        let mut fresh2 =
            ProviderConfigStore::load_or_default_at(&target_dir.path().join("p2.toml"));
        let err = run_providers_import(
            &mut fresh2,
            &ImportArgs {
                file: target_dir.path().join("snap.toml").display().to_string(),
                force: false,
                set_active: Some("ghost".to_string()),
            },
            None,
        )
        .expect_err("unknown --set-active must refuse");
        assert!(
            format!("{err:#}").contains("ghost"),
            "error names the bad profile: {err:#}"
        );
    }

    #[test]
    fn import_checklist_flags_unresolved_credential_refs() {
        // Hermetic credential dir: glm stored, anthropic missing.
        let dir = tempfile::tempdir().expect("tempdir");
        let cred_dir = dir.path().join("credentials");
        let mut manager = CredentialManager::with_dir(cred_dir.clone()).expect("manager");
        manager
            .store_or_update(shannon_core::credential_manager::Credential::new(
                "glm", "glm", "sk-glm",
            ))
            .expect("seed credential");

        // SAFETY: uniquely-named vars owned by this test.
        unsafe { std::env::set_var("SHANNON_IMPORT_CHECK_SET", "x") };
        let config_json = r#"{"version": 2, "profiles": {"default": {
            "name": "default",
            "active_target": {"provider_id": "glm", "model_id": "m", "scope": "global"},
            "providers": [
                {"id": "glm", "kind": "openai-compatible", "display_name": "glm",
                 "base_url": "https://x", "credential": {"backend": "store", "service": "glm"}},
                {"id": "anthropic", "kind": "anthropic", "display_name": "anthropic",
                 "base_url": "https://y", "credential": {"backend": "store", "service": "anthropic"}},
                {"id": "envset", "kind": "openai", "display_name": "envset",
                 "base_url": "https://z", "credential": {"backend": "env", "var": "SHANNON_IMPORT_CHECK_SET"}},
                {"id": "envunset", "kind": "openai", "display_name": "envunset",
                 "base_url": "https://z2", "credential": {"backend": "env", "var": "SHANNON_IMPORT_CHECK_UNSET"}},
                {"id": "kr", "kind": "openai", "display_name": "kr",
                 "base_url": "https://z3", "credential": {"backend": "keyring", "service": "s", "account": "a"}}
            ]}}}"#;
        let config: ProviderModelConfig =
            serde_json::from_str(config_json).expect("fixture parses");
        let lines = build_import_checklist(&config, Some(&cred_dir));
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert!(lines[0].starts_with("[ok] default/glm"), "{lines:?}");
        assert!(
            lines[1].starts_with("[missing] default/anthropic")
                && lines[1].contains("no stored credential"),
            "{lines:?}"
        );
        assert!(lines[2].starts_with("[ok] default/envset"), "{lines:?}");
        assert!(
            lines[3].starts_with("[missing] default/envunset") && lines[3].contains("not set"),
            "{lines:?}"
        );
        assert!(lines[4].starts_with("[check] default/kr"), "{lines:?}");
        // SAFETY: see above.
        unsafe { std::env::remove_var("SHANNON_IMPORT_CHECK_SET") };
    }

    #[test]
    fn import_refuses_empty_snapshot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let text = format!(
            "schema = \"{EXPORT_SCHEMA}\"\nredacted = false\n[config]\nversion = 2\n[config.profiles]\n"
        );
        std::fs::write(dir.path().join("empty.toml"), &text).expect("write");
        let mut store = ProviderConfigStore::load_or_default_at(&dir.path().join("providers.toml"));
        let err = run_providers_import(
            &mut store,
            &ImportArgs {
                file: dir.path().join("empty.toml").display().to_string(),
                force: false,
                set_active: None,
            },
            None,
        )
        .expect_err("empty snapshot refused");
        assert!(format!("{err:#}").contains("no providers"), "{err:#}");
    }
}
