use super::super::Repl;
use crate::{Result, widgets::ChatRole};
use rust_i18n::t;
use shannon_core::model_registry;
use shannon_core::provider_resolver::resolve_model_ref;
use shannon_engine::api::LlmProvider;
use shannon_types::model_ref::ModelRef;
use shannon_types::provider_config::{ProviderTiers, TierName};
use shannon_types::recover_lock;

/// Per-provider timeout for the concurrent `/health` live probe. Short on
/// purpose: the verdict is only a hint and a slow provider shouldn't make the
/// dashboard feel frozen. (ADR-0008 P3-3 — named instead of an inline magic
/// number so the probe and the connect-refresh stay independently tunable.)
const HEALTH_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Background models.dev refresh spawned by `/connect` so the model picker that
/// opens right after shows freshly discovered models. Distinct from
/// [`model_registry::dynamic::DEFAULT_FETCH_TIMEOUT`] (15s), which backs the
/// explicit `/model refresh` command: the connect flow should be snappy since
/// the static catalog is the authoritative fallback, while an explicit refresh
/// is the user opting into "wait as long as it takes". (ADR-0008 P3-3.)
const CONNECT_REFRESH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

// ── Submodules (ADR-0008 P2-8 — split oversized config.rs by command domain) ─
mod connect;
mod provider;

pub(crate) use connect::{handle_connect, handle_disconnect};
pub(crate) use provider::handle_provider;

/// Resolve a `/model` argument into `(model_id, optional provider)` (ADR-0005
/// Phase 3).
///
/// Accepts both spellings:
/// - **Qualified** `provider/model` (e.g. `anthropic/claude-sonnet-4-…`,
///   `ollama/llama3`): the provider comes from the ref, and the model is
///   alias-expanded *within* that provider.
/// - **Bare** (legacy): an alias (`sonnet`) or literal id; the provider is set
///   only when the model is known to the catalog.
pub(crate) fn resolve_model_arg(args: &str) -> (String, Option<LlmProvider>) {
    match ModelRef::parse(args.trim()) {
        Some(mref) => {
            let r = resolve_model_ref(&mref);
            (r.model_id, Some(r.provider))
        }
        None => {
            let info = model_registry::model_info_for_alias(args.trim());
            let model_id = info
                .map(|m| m.id.to_string())
                .unwrap_or_else(|| args.trim().to_string());
            (model_id, info.map(|m| m.provider.clone()))
        }
    }
}

/// Refresh the chat widget's first-screen StatusCard from `repl.state`.
///
/// Centralizes provider/model/tier derivation so every switch path and the
/// REPL init/resume paths render identical values. The tier label is the
/// **authoritative** `tier_label_for_id` (catalog capabilities) — never the
/// status-bar substring heuristic — so the card and the status pill agree
/// (ADR-0008 Decision 1).
pub(crate) fn sync_active_to_chat(repl: &mut Repl) {
    let provider = repl
        .state
        .selected_provider
        .as_ref()
        .map(shannon_core::provider_resolver::llm_provider_id);
    let model = repl.state.model.clone();
    let tier = repl.state.model.as_deref().map(|m| {
        shannon_core::model_registry::tier_label_for_id(m)
            .as_str()
            .to_string()
    });
    repl.chat.set_active(provider, model, tier);
}

/// Infer a canonical tier for a model id, returning `None` for catalog
/// unknowns so we never persist an inferred tier for a model we cannot
/// classify (ADR-0008 Decision 2 helper).
fn infer_tier_for_model(model_id: &str) -> Option<TierName> {
    use shannon_core::model_registry::{TierLabel, tier_label_for_id};
    match tier_label_for_id(model_id) {
        TierLabel::Fast => Some(TierName::Fast),
        TierLabel::Standard => Some(TierName::Standard),
        TierLabel::Pro => Some(TierName::Pro),
        TierLabel::Unknown => None,
    }
}

/// The single mutation path for switching the active provider/model/tier
/// (ADR-0008 Decision 2).
///
/// Every `/model`, `/provider`, `/connect`, and `/model --tier` switch goes
/// through here so the four call sites cannot drift. In one place it:
/// 1. Updates `repl.state` (model, selected_provider, context_window).
/// 2. Syncs the query engine (`set_model_for_provider` + `pre_resolve_context`).
/// 3. Persists runtime preferences (and, when `persist_tier`, the tier pin to
///    `providers.toml`, rolling back state on failure).
/// 4. Refreshes the chat widget via [`sync_active_to_chat`] so the
///    first-screen StatusCard reflects the switch immediately.
///
/// `model_id = None` means "switch provider, keep the current model" — used by
/// `/provider` when a provider has no built-in catalog.
///
/// Returns the resolved context window (`None` when genuinely unknown) so
/// callers that print a "context: …" label can format it honestly.
pub(crate) fn apply_model_selection(
    repl: &mut Repl,
    provider: LlmProvider,
    model_id: Option<String>,
    tier: Option<TierName>,
    persist_tier: bool,
) -> Result<Option<usize>> {
    let prev_model = repl.state.model.clone();
    let prev_provider = repl.state.selected_provider.clone();

    if let Some(ref m) = model_id {
        repl.state.model = Some(m.clone());
    }
    repl.state.selected_provider = Some(provider.clone());

    // Sync the engine + resolve the real context window. `catch_unwind`
    // consolidates the four former panic-swallow sites into one, and now
    // logs at ERROR instead of failing silently (ADR-0008 / plan P2-6).
    let effective_model = repl.state.model.clone().unwrap_or_default();
    let ctx_opt = if let Some(ref mut engine) = repl.query_engine {
        engine.set_model_for_provider(effective_model.clone(), provider.clone());
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            repl.runtime.block_on(engine.pre_resolve_context());
        }));
        if panicked.is_err() {
            tracing::error!(
                model = %effective_model,
                provider = ?provider,
                "pre_resolve_context panicked while switching model (recovered)"
            );
        }
        engine.resolved_context_window_opt()
    } else {
        shannon_core::model_registry::context_window_for_opt(&effective_model)
    };
    repl.state.context_window =
        ctx_opt.unwrap_or(shannon_core::model_registry::FALLBACK_CONTEXT_WINDOW);

    crate::repl::preferences::save_preferences(&crate::repl::preferences::Preferences {
        model: repl.state.model.clone(),
        provider: repl.state.selected_provider.clone(),
        theme: Some(repl.state.theme.name.to_string()),
    });

    // Optional tier pin → providers.toml. Roll back state on failure so a bad
    // write never leaves the REPL pointing at an unpinned selection.
    if persist_tier {
        if let Some(resolved_tier) = tier.or_else(|| infer_tier_for_model(&effective_model)) {
            if let Err(e) =
                persist_model_to_providers_toml(&provider, &effective_model, resolved_tier)
            {
                repl.state.model = prev_model;
                repl.state.selected_provider = prev_provider;
                sync_active_to_chat(repl);
                return Err(e);
            }
        }
    }

    sync_active_to_chat(repl);
    Ok(ctx_opt)
}

/// True when the first whitespace-delimited token of `args` is exactly `flag`
/// or a `--flag=value` spelling of it.
///
/// Replaces the old `args.starts_with("--tier")` / `starts_with("--max-tokens")`
/// dispatch, which mis-routed any token sharing the prefix (e.g. `--tierfoo`)
/// into the wrong handler. The two flags share no prefix, so order no longer
/// matters either (ADR-0008 P2-4).
fn first_flag_is(args: &str, flag: &str) -> bool {
    let Some(first) = args.split_whitespace().next() else {
        return false;
    };
    first == flag || first.starts_with(&format!("{flag}="))
}

pub(crate) fn handle_model(repl: &mut Repl, args: &str) -> Result<()> {
    // /model --tier <name> [provider] [--save]
    if first_flag_is(args, "--tier") {
        return handle_model_tier(repl, args);
    }

    // /model --max-tokens <N> [--save] — override the per-request max-tokens
    // ceiling for the active provider, optionally persisting to providers.toml.
    if first_flag_is(args, "--max-tokens") {
        return handle_model_max_tokens(repl, args);
    }

    // /model refresh — pull models.dev and rebuild the dynamic overlay (Phase D).
    if args.trim() == "refresh" {
        return handle_model_refresh(repl);
    }

    if args.is_empty() {
        let picker = crate::widgets::select::ModelPickerWidget::new(repl.state.model.as_deref());
        repl.state.model_picker = Some(picker);
    } else {
        let (resolved_id, resolved_provider) = resolve_model_arg(args);

        // Effective provider: the one implied by the arg, else the current
        // one. A bare alias/id without a resolvable provider keeps the active
        // provider; if none is active, the user must `/connect` first.
        let provider = match resolved_provider.or_else(|| repl.state.selected_provider.clone()) {
            Some(p) => p,
            None => {
                repl.chat.add_message(
                    ChatRole::System,
                    t!("commands.provider.no_selection").to_string(),
                );
                return Ok(());
            }
        };

        let ctx_opt =
            apply_model_selection(repl, provider, Some(resolved_id.clone()), None, false)?;
        let ctx_label = format_context_label(ctx_opt);
        // `ctx_opt = None` means the id is unknown to both the static catalog
        // and the models.dev overlay — surface a warning so typos don't fail
        // silently at the next query (plan P1-7). The model is still set as-is
        // as an escape hatch for ids the catalog hasn't indexed yet.
        let prefix = if ctx_opt.is_none() {
            format!(
                "⚠ '{resolved_id}' is not in the catalog; using as-is. Run /model refresh to \
                 pull the latest models, or /model <provider>/<id> for a qualified id.\n\n"
            )
        } else {
            String::new()
        };
        let msg = format!(
            "{prefix}{} (context: {ctx_label})",
            t!("commands.model.set", name = &resolved_id)
        );
        repl.chat.add_message(ChatRole::System, msg);
    }
    Ok(())
}

/// `/model refresh` — fetch models.dev and rebuild the dynamic overlay, and
/// refresh the LiteLLM community pricing table, **in the background**.
///
/// The two network fetches run on the tokio runtime via `runtime.spawn`, so
/// the REPL never blocks on them. This replaces the old `block_on` of two
/// sequential fetches that froze the UI for up to 2× timeout (plan P1-3), and
/// mirrors the `/connect` background-refresh pattern (ADR-0007).
///
/// On any error (offline, timeout, non-200, malformed payload) the built-in
/// catalog is left untouched — never a crash. Outcome reporting (success
/// count / error) back into chat is deferred to a channel follow-up; for now
/// the picker re-reads the overlay on next open, so a successful refresh is
/// visible the moment the user re-opens `/model`.
fn handle_model_refresh(repl: &mut Repl) -> Result<()> {
    let timeout = model_registry::dynamic::DEFAULT_FETCH_TIMEOUT;
    repl.chat.add_message(
        ChatRole::System,
        "Refreshing models.dev catalog + LiteLLM pricing in the background…".to_string(),
    );

    // Non-blocking: discard the JoinHandle (clippy::let_underscore_future).
    // Errors are swallowed by design — a failure leaves the cached overlay in
    // place and the user can simply retry.
    std::mem::drop(repl.runtime.spawn(async move {
        let _ = model_registry::dynamic::refresh_overlay_async(timeout).await;
        let _ = shannon_core::query_engine::litellm::refresh_async(timeout).await;
    }));
    Ok(())
}

/// Format a resolved context window for user-facing labels. `None` (unknown)
/// renders as the localized "unknown" string instead of fabricating a number
/// (Phase E).
fn format_context_label(ctx: Option<usize>) -> String {
    match ctx {
        Some(n) if n >= 1_000_000 => format!("{}M", n / 1_000_000),
        Some(n) if n >= 1_000 => format!("{}K", n / 1_000),
        Some(n) => n.to_string(),
        None => t!("commands.model.context_unknown").to_string(),
    }
}

/// Parse a provider name string (with aliases) into an [`LlmProvider`].
fn parse_provider_name(name: &str) -> Result<LlmProvider> {
    // Thin delegate over the single alias table (ADR-0008 Decision 1 / P2-3):
    // `llm_provider_from_slug` is the union of every provider-name match that
    // used to live here, in `provider_str_to_llm`, and in `llm_provider_from_id`,
    // so adding a provider alias is now a one-place edit. Case-insensitive and
    // whitespace-tolerant.
    shannon_core::provider_resolver::llm_provider_from_slug(name).ok_or_else(|| {
        let msg = format!("Unknown provider: {name}. Use /provider to list available providers.");
        msg.into()
    })
}

/// Unified provider-connection vocabulary so `/connect` and `/provider` never
/// disagree on wording (ADR-0008 P1-2).
///
/// Replaces the two independent label sets that previously existed:
/// - `/connect` dashboard: `no auth` / `✓ connected` / `key stored` / `no key`
/// - `/provider`: `key OK` / `no key` / `no auth`
///
/// (The welcome status card uses a coarser binary `●`/`○` presence model keyed
/// off `connected_slugs()` — unified separately in P2-2 — so it is intentionally
/// not driven by this enum.)
///
/// The `key OK` vs `key stored` mismatch is the user-visible bug this fixes:
/// both now render as the same enum variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderConnectionStatus {
    /// Provider needs no key (e.g. Ollama).
    NoAuth,
    /// A provider config is persisted AND a credential is stored → works on next launch.
    Connected,
    /// A credential exists but `/connect` hasn't persisted a provider config yet.
    KeyStored,
    /// Auth required, nothing stored.
    NoKey,
}

impl ProviderConnectionStatus {
    /// Single decision point mapping the three underlying booleans to a status.
    /// Pure — unit-tested below.
    pub(crate) fn classify(requires_auth: bool, connected: bool, has_key: bool) -> Self {
        if !requires_auth {
            Self::NoAuth
        } else if connected && has_key {
            Self::Connected
        } else if has_key {
            Self::KeyStored
        } else {
            Self::NoKey
        }
    }
}

impl std::fmt::Display for ProviderConnectionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAuth => write!(f, "no auth"),
            Self::Connected => write!(f, "✓ connected"),
            Self::KeyStored => write!(f, "key stored"),
            Self::NoKey => write!(f, "no key"),
        }
    }
}

/// Classify a provider's connection status for display. Thin delegate over
/// [`ProviderConnectionStatus::classify`] kept for call-site readability.
fn connect_status(requires_auth: bool, connected: bool, has_key: bool) -> ProviderConnectionStatus {
    ProviderConnectionStatus::classify(requires_auth, connected, has_key)
}

/// Slugs of providers that have a persisted provider config in
/// `~/.shannon/providers.toml` (i.e. `/connect` was run for them).
///
/// Thin delegate over [`shannon_core::provider_config_store::connected_slugs`]
/// so the welcome status card and the `/connect` dashboard share one
/// implementation (ADR-0008 Decision 3).
fn connected_provider_slugs() -> std::collections::HashSet<String> {
    shannon_core::provider_config_store::connected_slugs()
}

pub(crate) fn handle_init(repl: &mut Repl) -> Result<()> {
    let mut init_info = String::new();
    let cwd = &repl.state.working_directory;

    let is_git = std::path::Path::new(cwd).join(".git").exists();
    if is_git {
        init_info.push_str("Git repository: detected\n");
    } else {
        init_info.push_str("Git repository: not found\n");
    }

    let agents_md_path = std::path::Path::new(cwd).join("AGENTS.md");
    if agents_md_path.exists() {
        init_info.push_str("AGENTS.md: already exists\n");
    } else {
        let default_content = "# Project Instructions\n\nThis file contains project-specific instructions for Shannon.\n\n## Coding Standards\n\n- Follow existing code patterns\n- Write clear, descriptive commit messages\n- Keep functions focused and concise\n\n## Project Structure\n\n- Describe your project structure here\n";
        match std::fs::write(&agents_md_path, default_content) {
            Ok(_) => init_info.push_str("AGENTS.md: created with default template\n"),
            Err(e) => init_info.push_str(&format!("AGENTS.md: failed to create ({e})\n")),
        }
    }

    init_info.push_str(&format!("Working directory: {cwd}\n"));
    repl.chat.add_message(
        ChatRole::System,
        t!("repl.project_initialized", info = init_info).to_string(),
    );
    Ok(())
}

pub(crate) fn handle_config(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_commands::config_utils;
    use shannon_tools::config::ConfigManager;

    let mut manager = ConfigManager::new();
    if let Err(e) = manager.load() {
        repl.chat.add_message(
            ChatRole::System,
            t!("commands.config.warning_load", error = e).to_string(),
        );
    }

    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let action_str = parts.first().copied().unwrap_or("");
    let action = config_utils::parse_config_action(action_str);

    let output = match action {
        config_utils::ConfigAction::List => {
            let prefix = if action_str.is_empty() {
                None
            } else {
                parts.get(1).copied()
            };
            let keys = manager.list(prefix);
            if keys.is_empty() {
                config_utils::format_config_list()
            } else {
                let mut out = config_utils::format_config_list();
                out.push_str(&format!(
                    "\nConfig file: {}\n",
                    manager.config_path().display()
                ));
                for key in &keys {
                    let val = manager.get(key).unwrap_or(serde_json::Value::Null);
                    out.push_str(&format!("  {key} = {val}\n"));
                }
                out
            }
        }
        config_utils::ConfigAction::Get => {
            let key = parts.get(1).copied().unwrap_or("");
            if key.is_empty() {
                "Usage: /config get <key>".to_string()
            } else {
                match manager.get(key) {
                    Some(_val) => config_utils::format_config_get(key),
                    None => format!("Config key not found: {key}"),
                }
            }
        }
        config_utils::ConfigAction::Set => {
            let key = parts.get(1).copied().unwrap_or("");
            let value_str = parts.get(2).copied().unwrap_or("");
            if key.is_empty() || value_str.is_empty() {
                "Usage: /config set <key> <value>".to_string()
            } else {
                let value: serde_json::Value = if value_str == "true" {
                    serde_json::json!(true)
                } else if value_str == "false" {
                    serde_json::json!(false)
                } else if let Ok(n) = value_str.parse::<i64>() {
                    serde_json::json!(n)
                } else if let Ok(n) = value_str.parse::<f64>() {
                    serde_json::json!(n)
                } else {
                    serde_json::json!(value_str)
                };
                manager.set(key.to_string(), value.clone());
                let mut msg = match manager.save() {
                    Ok(_) => config_utils::format_config_set(key, &value.to_string()),
                    Err(e) => format!("Error: saving config: {e}"),
                };
                // ADR-0005 Phase 4: mirror engine-read flat keys into
                // ~/.shannon/config.toml (the file the engine loads on next
                // launch), so `/config set model X` actually takes effect.
                // Secrets are refused by the allowlist — they belong in
                // /credentials, never in a config file (decision A1).
                if shannon_core::config_persist::is_writable_key(key) {
                    match shannon_core::config_persist::set_global_config_key(None, key, value_str)
                    {
                        Ok(path) => msg.push_str(&format!("\n  engine config: {}", path.display())),
                        Err(e) => msg.push_str(&format!("\n  warning: config.toml: {e}")),
                    }
                }
                msg
            }
        }
        config_utils::ConfigAction::Reset => {
            let key = parts.get(1).copied().unwrap_or("");
            if key.is_empty() {
                "Usage: /config reset <key>".to_string()
            } else {
                let existed = manager.reset(key);
                let mut msg = if existed {
                    let _val = manager.get(key).unwrap_or(serde_json::Value::Null);
                    match manager.save() {
                        Ok(_) => config_utils::format_config_reset(key),
                        Err(e) => format!("Error: saving config: {e}"),
                    }
                } else {
                    config_utils::format_config_reset(key)
                };
                // ADR-0005 Phase 4: also drop the key from the engine-read
                // config.toml. Reset only deletes, so it is safe for any key.
                match shannon_core::config_persist::reset_global_config_key(None, key) {
                    Ok(true) => msg.push_str("\n  removed from config.toml."),
                    Ok(false) => {}
                    Err(e) => msg.push_str(&format!("\n  warning: config.toml: {e}")),
                }
                msg
            }
        }
        config_utils::ConfigAction::Help => config_utils::format_config_list(),
    };

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

pub(crate) fn handle_mode(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_engine::permissions::ApprovalMode;

    let trimmed = args.trim();

    if trimmed.is_empty() {
        // Show current mode and available options
        let current = {
            let query_engine = match repl.query_engine.as_ref() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(
                        ChatRole::System,
                        "Error: Query engine not available.".to_string(),
                    );
                    return Ok(());
                }
            };
            let permissions = recover_lock(query_engine.permissions().read());
            permissions.approval_mode()
        };
        let mut msg = format!("Current approval mode: {current}\n\nAvailable modes:\n");
        for name in ApprovalMode::all_names() {
            let mode = ApprovalMode::from_str_ci(name)
                .expect("from_str_ci should return valid mode for all_names()");
            let marker = if mode == current { " *" } else { "" };
            msg.push_str(&format!("  {name}{marker} — {}\n", mode.description()));
        }
        {
            repl.chat.add_message(ChatRole::System, msg);
        }
        return Ok(());
    }

    match ApprovalMode::from_str_ci(trimmed) {
        Some(mode) => {
            let query_engine = match repl.query_engine.as_ref() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(
                        ChatRole::System,
                        "Error: Query engine not available.".to_string(),
                    );
                    return Ok(());
                }
            };
            recover_lock(query_engine.permissions().write()).set_approval_mode(mode);
            repl.state.approval_mode_label = mode.short_label().to_string();
            {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Approval mode set to: {mode}\n{}", mode.description()),
                );
            }
            Ok(())
        }
        None => {
            let valid = ApprovalMode::all_names().join(", ");
            {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Unknown mode: '{trimmed}'. Valid modes: {valid}"),
                );
            }
            Ok(())
        }
    }
}

pub(crate) fn handle_context(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    if trimmed == "reload" {
        // Reload project context into the query engine
        let cwd = std::env::current_dir().unwrap_or_default();
        match shannon_core::project_instructions::load_full_context(&cwd) {
            Some(instructions) => {
                let query_engine = match repl.query_engine.as_mut() {
                    Some(e) => e,
                    None => {
                        repl.chat.add_message(
                            ChatRole::System,
                            "Error: Query engine not available.".to_string(),
                        );
                        return Ok(());
                    }
                };
                query_engine.append_system_prompt(&instructions.content);
                let files = instructions.loaded_files.join(", ");
                {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!("Project context reloaded. Loaded: {files}"),
                    );
                }
            }
            None => {
                repl.chat.add_message(
                        ChatRole::System,
                        "No project context found (no CLAUDE.md/AGENTS.md/GEMINI.md and not in a git repo)".to_string(),
                    );
            }
        }
        return Ok(());
    }

    if trimmed == "usage" {
        let total = repl.state.tokens_used;
        let input = repl.state.input_tokens;
        let output = repl.state.output_tokens;
        let cached = repl.state.cache_read_tokens + repl.state.cache_creation_tokens;
        let other = total.saturating_sub(input + output + cached);

        // Build a colored bar using Unicode block chars
        let bar_w = 40usize;
        let max_ctx = 200_000u64; // default context window
        let pct = if total > 0 {
            (total as f64 / max_ctx as f64).min(1.0)
        } else {
            0.0
        };
        let filled = (pct * bar_w as f64).round() as usize;

        let input_w = if total > 0 {
            (input as f64 / max_ctx as f64 * bar_w as f64).round() as usize
        } else {
            0
        };
        let output_w = if total > 0 {
            (output as f64 / max_ctx as f64 * bar_w as f64).round() as usize
        } else {
            0
        };
        let cached_w = if total > 0 {
            (cached as f64 / max_ctx as f64 * bar_w as f64).round() as usize
        } else {
            0
        };
        let other_w = filled.saturating_sub(input_w + output_w + cached_w);

        let mut bar = String::from("[");
        for _ in 0..input_w {
            bar.push('█');
        }
        for _ in 0..output_w {
            bar.push('▓');
        }
        for _ in 0..cached_w {
            bar.push('░');
        }
        for _ in 0..other_w {
            bar.push('▒');
        }
        for _ in 0..(bar_w.saturating_sub(filled)) {
            bar.push('·');
        }
        bar.push(']');

        let fmt_tok = |t: u64| -> String {
            if t < 1000 {
                format!("{t}")
            } else if t < 1_000_000 {
                format!("{:.1}k", t as f64 / 1000.0)
            } else {
                format!("{:.1}M", t as f64 / 1_000_000.0)
            }
        };

        let mut msg = String::from("Context Window Usage\n\n");
        msg.push_str(&format!("  {} {:.1}%\n\n", bar, pct * 100.0));
        msg.push_str(&format!("  █ Input:    {} tokens\n", fmt_tok(input)));
        msg.push_str(&format!("  ▓ Output:   {} tokens\n", fmt_tok(output)));
        msg.push_str(&format!("  ░ Cached:   {} tokens\n", fmt_tok(cached)));
        if other > 0 {
            msg.push_str(&format!("  ▒ Other:    {} tokens\n", fmt_tok(other)));
        }
        msg.push_str(&format!(
            "  · Free:     {} tokens\n\n",
            fmt_tok(max_ctx.saturating_sub(total))
        ));
        msg.push_str(&format!(
            "  Total used: {} / {} tokens\n",
            fmt_tok(total),
            fmt_tok(max_ctx)
        ));

        if pct > 0.8 {
            msg.push_str("\n  ⚠ Context is over 80% used. Consider /compact to free space.");
        }

        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    // Show current project context info
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut msg = String::from("Project Context:\n\n");

    // Check instruction files
    let instruction_files = ["CLAUDE.md", "AGENTS.md", "GEMINI.md"];
    let mut found_any = false;
    for filename in &instruction_files {
        let path = cwd.join(filename);
        if path.is_file() {
            found_any = true;
            msg.push_str(&format!("  {filename}: found\n"));
        } else {
            msg.push_str(&format!("  {filename}: not found\n"));
        }
    }

    // Check parent directories for instruction files
    let mut current = cwd.parent();
    while let Some(parent) = current {
        for filename in &instruction_files {
            if parent.join(filename).is_file() {
                msg.push_str(&format!("  {filename}: found in {}\n", parent.display()));
                found_any = true;
            }
        }
        current = parent.parent();
    }

    // Git context
    if let Some(git_ctx) = shannon_core::project_instructions::git_context(&cwd) {
        msg.push_str(&format!("\n{git_ctx}"));
        found_any = true;
    } else {
        msg.push_str("\nGit: not a git repository\n");
    }

    if !found_any {
        msg.push_str(
            "\nNo project context available. Create an AGENTS.md file or initialize a git repo.",
        );
    }

    msg.push_str("\nTip: Use /context reload to refresh the project context.");
    {
        repl.chat.add_message(ChatRole::System, msg);
    }
    Ok(())
}

pub(crate) fn handle_local_models(repl: &mut Repl) -> Result<()> {
    let mut output = String::from("Local Model Detection\n\n");

    // Check Ollama
    let ollama_check = std::process::Command::new("curl")
        .args([
            "-s",
            "--connect-timeout",
            "3",
            "http://localhost:11434/api/tags",
        ])
        .output();

    match ollama_check {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            if stdout.is_empty() || !result.status.success() {
                output.push_str("Ollama: not running (localhost:11434 unreachable)\n");
            } else {
                output.push_str("Ollama: running at localhost:11434\n");
                // Parse model list
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    if let Some(models) = json.get("models").and_then(|m| m.as_array()) {
                        if models.is_empty() {
                            output.push_str("  No models installed\n");
                        } else {
                            output.push_str(&format!("  Available models ({}):\n", models.len()));
                            for model in models {
                                let name = model
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("unknown");
                                let size = model
                                    .get("size")
                                    .and_then(|s| s.as_u64())
                                    .map(|b| format!("{:.1} GB", b as f64 / 1e9))
                                    .unwrap_or_default();
                                output.push_str(&format!("    - {name} ({size})\n"));
                            }
                        }
                    }
                } else {
                    output.push_str("  Could not parse model list\n");
                }
            }
        }
        Err(_) => {
            output.push_str("Ollama: not detected (curl not available or host unreachable)\n");
        }
    }

    // Check LM Studio
    let lmstudio_check = std::process::Command::new("curl")
        .args([
            "-s",
            "--connect-timeout",
            "3",
            "http://localhost:1234/v1/models",
        ])
        .output();

    match lmstudio_check {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            if stdout.is_empty() || !result.status.success() {
                output.push_str("\nLM Studio: not running (localhost:1234 unreachable)\n");
            } else {
                output.push_str("\nLM Studio: running at localhost:1234\n");
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    if let Some(models) = json.get("data").and_then(|m| m.as_array()) {
                        if models.is_empty() {
                            output.push_str("  No models loaded\n");
                        } else {
                            output.push_str(&format!("  Loaded models ({}):\n", models.len()));
                            for model in models {
                                let id = model
                                    .get("id")
                                    .and_then(|i| i.as_str())
                                    .unwrap_or("unknown");
                                output.push_str(&format!("    - {id}\n"));
                            }
                        }
                    }
                }
            }
        }
        Err(_) => {
            output.push_str("\nLM Studio: not detected\n");
        }
    }

    // Suggest usage
    output.push_str("\nTo use a local model:\n");
    output.push_str("  /model ollama/llama3\n");
    output.push_str("  /model ollama/mistral\n");
    output.push_str("  /model lmstudio/<model-id>\n");
    output.push_str(&format!(
        "\nCurrent model: {}\n",
        repl.state.model.as_deref().unwrap_or("not set")
    ));

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

/// /theme — switch color theme or list available themes.
pub(crate) fn handle_theme(repl: &mut Repl, args: &str) -> Result<()> {
    use crate::theme::Theme;

    let args = args.trim();

    if args == "pick" || args == "picker" || args == "preview" {
        let themes = Theme::available();
        let current = &repl.state.theme.name;
        let items: Vec<_> = themes
            .into_iter()
            .map(|name| {
                let label = if name == *current {
                    format!("{name} (current)")
                } else {
                    name.clone()
                };
                crate::widgets::select::SelectItem::new(label, name)
            })
            .collect();

        let picker = crate::widgets::select::FuzzyPickerWidget::new("Theme Picker".to_string())
            .with_items(items);
        repl.state.theme_picker = Some(picker);
        return Ok(());
    }

    if args.is_empty() || args == "list" {
        let current = &repl.state.theme.name;
        let available = Theme::available();
        let mut msg = String::from("Available themes:\n");
        for name in available {
            if name == *current {
                msg.push_str(&format!("  * {name} (current)\n"));
            } else {
                msg.push_str(&format!("    {name}\n"));
            }
        }
        msg.push_str("\nUsage: /theme <name>");
        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    match Theme::named(args) {
        Some(theme) => {
            let name = theme.name.clone();
            repl.renderer.set_theme(&theme);
            repl.state.theme = theme;
            crate::repl::preferences::save_preferences(&crate::repl::preferences::Preferences {
                model: repl.state.model.clone(),
                provider: repl.state.selected_provider.clone(),
                theme: Some(name.to_string()),
            });
            repl.chat
                .add_message(ChatRole::System, format!("Theme switched to '{name}'."));
        }
        None => {
            let available = Theme::available().join(", ");
            repl.chat.add_message(
                ChatRole::System,
                format!("Unknown theme '{args}'. Available: {available}"),
            );
        }
    }

    Ok(())
}

/// /accessibility — toggle or check accessibility mode.
pub(crate) fn handle_accessibility(repl: &mut Repl, args: &str) -> Result<()> {
    let arg = args.trim();
    match arg {
        "on" | "enable" | "true" | "1" => {
            repl.state.accessibility_mode = true;
            crate::a11y::set_enabled(true);
            repl.chat.add_message(
                ChatRole::System,
                "Accessibility mode enabled. Decorative characters replaced with plain text."
                    .to_string(),
            );
        }
        "off" | "disable" | "false" | "0" => {
            repl.state.accessibility_mode = false;
            crate::a11y::set_enabled(false);
            repl.chat
                .add_message(ChatRole::System, "Accessibility mode disabled.".to_string());
        }
        "" | "status" => {
            let state = if repl.state.accessibility_mode {
                "enabled"
            } else {
                "disabled"
            };
            repl.chat.add_message(ChatRole::System,
                format!("Accessibility mode: {state}\n\nUsage: /accessibility on|off\nAlso auto-enabled via NO_GRAPHICS or ACCESSIBILITY env vars."));
        }
        _ => {
            repl.chat.add_message(
                ChatRole::System,
                "Usage: /accessibility on|off|status".to_string(),
            );
        }
    }
    Ok(())
}

pub(crate) fn handle_terminal_setup(repl: &mut Repl) -> Result<()> {
    let mut report = String::from("Terminal Setup Check\n\n");

    // 1. Shell detection
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".to_string());
    let shell_name = std::path::Path::new(&shell)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| shell.clone());
    report.push_str(&format!("Shell: {shell_name} ({shell})\n"));

    // 2. Terminal type
    let term = std::env::var("TERM").unwrap_or_else(|_| "not set".to_string());
    report.push_str(&format!("TERM: {term}\n"));

    // 3. Check if shannon is on PATH
    let shannon_on_path = std::process::Command::new("which")
        .arg("shannon")
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);
    report.push_str(&format!(
        "shannon on PATH: {}\n",
        if shannon_on_path {
            "yes"
        } else {
            "no — add shannon to your PATH"
        }
    ));

    // 4. Check for common terminal tools
    for tool in &["git", "gh", "node"] {
        let found = std::process::Command::new("which")
            .arg(tool)
            .output()
            .ok()
            .map(|o| o.status.success())
            .unwrap_or(false);
        report.push_str(&format!(
            "{tool}: {}\n",
            if found { "found" } else { "not found" }
        ));
    }

    // 5. Check shell integration markers
    // Claude Code uses SHANNON_INTEGRATION_DIR or similar env vars
    let has_integration = std::env::var("SHANNON_SHELL_INTEGRATION")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    report.push_str(&format!(
        "Shell integration: {}\n",
        if has_integration {
            "active"
        } else {
            "not detected — add `eval \"$(shannon init)\"` to your shell profile for inline diagnostics and key bindings"
        }
    ));

    // 6. Check terminal dimensions
    let (w, h) = crossterm::terminal::size().unwrap_or((0, 0));
    report.push_str(&format!("Terminal size: {w}x{h}\n"));
    if w < 80 {
        report.push_str("  ⚠ Terminal width < 80 columns — UI may be cramped\n");
    }

    // 7. Color support
    let colors = std::env::var("COLORTERM").unwrap_or_else(|_| "not set".to_string());
    report.push_str(&format!("COLORTERM: {colors}\n"));

    // 8. Key binding hint
    report.push_str("\nKey bindings:\n");
    report.push_str("  Enter      — submit input\n");
    report.push_str("  Ctrl+C     — cancel current operation\n");
    report.push_str("  Ctrl+D     — exit Shannon\n");
    report.push_str("  Tab        — autocomplete\n");
    report.push_str("  Up/Down    — navigate history\n");
    report.push_str("  Escape     — enter/exit vim normal mode\n");

    report.push_str("\nShell profile setup:\n");
    match shell_name.as_str() {
        "zsh" => report.push_str("  Add to ~/.zshrc:\n    eval \"$(shannon init zsh)\"\n"),
        "bash" => report.push_str("  Add to ~/.bashrc:\n    eval \"$(shannon init bash)\"\n"),
        "fish" => report
            .push_str("  Add to ~/.config/fish/config.fish:\n    shannon init fish | source\n"),
        other => report.push_str(&format!(
            "  Unknown shell '{other}'. Add the appropriate init line to your shell profile.\n"
        )),
    }

    repl.chat.add_message(ChatRole::System, report);
    Ok(())
}

/// Handle /color command — set prompt bar color per session
pub(crate) fn handle_color(repl: &mut Repl, args: &str) -> Result<()> {
    let color = args.trim();
    if color.is_empty() || color == "default" || color == "reset" {
        repl.state.prompt_bar_color = None;
        repl.prompt.set_border_color(None);
        repl.chat.add_message(
            ChatRole::System,
            "Prompt bar color reset to default.".to_string(),
        );
    } else {
        // Validate color by trying to parse it
        let parsed = parse_color_string(color);
        match parsed {
            Some(c) => {
                repl.state.prompt_bar_color = Some(color.to_string());
                repl.prompt.set_border_color(Some(c));
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Prompt bar color set to {color}."),
                );
            }
            None => {
                repl.chat.add_message(ChatRole::System, format!(
                    "Unknown color: \"{color}\". Use a named color (red, green, blue, ...) or hex (#ff0000), or \"default\" to reset."
                ));
            }
        }
    }
    Ok(())
}

/// Parse a color string into a ratatui Color
pub(crate) fn handle_statusline(repl: &mut Repl, args: &str) -> Result<()> {
    let cmd = args.trim();
    if cmd.is_empty() || cmd == "off" || cmd == "reset" || cmd == "default" {
        repl.state.statusline_command = None;
        repl.state.cached_statusline = None;
        repl.chat
            .add_message(ChatRole::System, "Custom statusline disabled.".to_string());
    } else {
        repl.state.statusline_command = Some(cmd.to_string());
        repl.state.cached_statusline = None;
        repl.state.statusline_last_update = None;
        repl.chat
            .add_message(ChatRole::System, format!("Custom statusline set to: {cmd}"));
    }
    Ok(())
}

fn parse_color_string(s: &str) -> Option<ratatui::style::Color> {
    use ratatui::style::Color;
    let lower = s.to_lowercase();
    match lower.as_str() {
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "blue" => Some(Color::Blue),
        "yellow" => Some(Color::Yellow),
        "magenta" | "purple" | "pink" => Some(Color::Magenta),
        "cyan" | "teal" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "dark_grey" | "darkgrey" => Some(Color::DarkGray),
        "lightred" | "light_red" => Some(Color::LightRed),
        "lightgreen" | "light_green" => Some(Color::LightGreen),
        "lightblue" | "light_blue" => Some(Color::LightBlue),
        "lightyellow" | "light_yellow" => Some(Color::LightYellow),
        "lightmagenta" | "light_magenta" => Some(Color::LightMagenta),
        "lightcyan" | "light_cyan" => Some(Color::LightCyan),
        "black" => Some(Color::Black),
        _ => {
            // Try hex color
            let hex = s.trim_start_matches('#');
            if hex.len() == 6 {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color::Rgb(r, g, b))
            } else {
                None
            }
        }
    }
}

pub(crate) fn handle_lang(repl: &mut Repl, args: &str) -> Result<()> {
    let supported = ["en", "zh", "hi", "es", "fr", "ar", "bn", "pt", "ru", "ja"];
    let input = args.trim();

    if input.is_empty() {
        let current = shannon_core::i18n::current_locale();
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Current language: {current}\n\nUsage: /lang <code>\nSupported: {}",
                supported.join(", ")
            ),
        );
        return Ok(());
    }

    let lang = input.to_lowercase();
    if supported.contains(&lang.as_str()) {
        shannon_core::i18n::set_locale(&lang);
        // Refresh status bar to reflect the new language immediately
        repl.state.status = t!("status.ready").to_string();
        let lang_names = [
            ("en", "English"),
            ("zh", "中文"),
            ("hi", "हिन्दी"),
            ("es", "Español"),
            ("fr", "Français"),
            ("ar", "العربية"),
            ("bn", "বাংলা"),
            ("pt", "Português"),
            ("ru", "Русский"),
            ("ja", "日本語"),
        ];
        let native_name = lang_names
            .iter()
            .find(|(c, _)| *c == lang)
            .map(|(_, n)| *n)
            .unwrap_or(&lang);
        repl.chat.add_message(
            ChatRole::System,
            format!("Language: {native_name} ({lang})"),
        );
    } else {
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Unsupported language: {lang}\nSupported: {}",
                supported.join(", ")
            ),
        );
    }
    Ok(())
}

/// Handle /model --max-tokens <N> [--save] — set or persist a per-provider
/// `default_max_tokens` override. The override is the fallback the engine
/// uses when a request does not specify `max_tokens`
/// ([`shannon_core::unified_config::build_client_from_resolved`] reads it
/// from the active profile).
///
/// Forms accepted:
/// - `/model --max-tokens 8192` — preview the override; do not persist.
/// - `/model --max-tokens 8192 --save` — also persist to providers.toml.
/// - `/model --max-tokens=8192 --save` — equals form, same semantics.
///
/// Special values:
/// - `0` or `clear` — clear the override (revert to the catalog default).
/// - Any other integer is parsed as `u32`; non-numeric input is an error.
///
/// Persistence path: `ProviderConfigStore::set_default_max_tokens` +
/// `save()`. The active provider is taken from `repl.state.selected_provider`;
/// if none is selected the command errors out (same contract as `/model --tier`
/// without an explicit provider).
///
/// Mirrors ADR-0005 P4.13 — closes the parity gap with `/model --tier --save`.
fn handle_model_max_tokens(repl: &mut Repl, args: &str) -> Result<()> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    let first = parts.first().copied().unwrap_or("");
    let raw_value = if first == "--max-tokens" {
        parts.get(1).copied().unwrap_or("")
    } else if let Some(rest) = first.strip_prefix("--max-tokens=") {
        rest
    } else {
        return Err("internal: handle_model_max_tokens called without --max-tokens prefix".into());
    };

    if raw_value.is_empty() {
        return Err(
            "missing value for --max-tokens; usage: /model --max-tokens <N|clear> [--save]".into(),
        );
    }

    // "0" and "clear" both mean "revert to the catalog default" so the user
    // has an obvious escape hatch when an earlier `--save` left an unwanted
    // override behind. Other non-numeric input is rejected as a typo.
    let next = if raw_value == "0" || raw_value.eq_ignore_ascii_case("clear") {
        None
    } else {
        let parsed = raw_value.parse::<u32>().map_err(|e| {
            format!(
                "invalid --max-tokens value '{raw_value}' (expected non-negative integer or 'clear'): {e}"
            )
        })?;
        Some(parsed)
    };

    let save = parts.contains(&"--save");
    let provider = repl.state.selected_provider.clone().ok_or_else(|| {
        "No provider selected; switch to a provider first (`/provider <name>`), \
         then re-run /model --max-tokens"
            .to_string()
    })?;

    if save {
        persist_max_tokens_to_providers_toml(&provider, next)?;
    }

    let action = match next {
        Some(n) => format!("set to {n}"),
        None => "cleared (revert to catalog default)".to_string(),
    };
    let saved_note = if save {
        " and saved to providers.toml"
    } else {
        " (not saved)"
    };
    let msg = format!("default_max_tokens for {provider:?} {action}{saved_note}");
    repl.chat.add_message(ChatRole::System, msg);
    Ok(())
}

/// Persist the resolved `default_max_tokens` override back into
/// `~/.shannon/providers.toml` for the named provider (ADR-0005 P4.13).
///
/// Steps:
/// 1. Load (or default-construct) a [`ProviderConfigStore`] for the v2 file.
/// 2. Write `default_max_tokens` on the active provider slot via
///    `set_default_max_tokens` — `None` clears the override.
/// 3. Atomically persist via `store.save()`.
///
/// Errors do not roll back REPL state: `/model --max-tokens` is a one-shot
/// metadata write, not a model switch, so a transient save failure should be
/// surfaced but must not corrupt the running engine. The caller (`handle_model_max_tokens`)
/// does not depend on `repl.state` to apply the value — the engine reads it
/// on the next request from disk.
fn persist_max_tokens_to_providers_toml(provider: &LlmProvider, next: Option<u32>) -> Result<()> {
    use shannon_core::provider_config_store::ProviderConfigStore;

    let mut store = ProviderConfigStore::load_or_default();
    store
        .set_default_max_tokens(provider, next)
        .save()
        .map_err(|e| {
            format!(
                "failed to persist default_max_tokens to providers.toml: {e} \
                 (provider={}, next={:?})",
                format!("{provider:?}").to_lowercase(),
                next,
            )
        })?;
    Ok(())
}

/// Handle /model --tier <name> [provider] [--save] — resolve a tier to a
/// concrete model id for the chosen (or current) provider, then switch the
/// running engine + REPL state to it. Mirrors the bare-id branch above but
/// goes through `model_registry::resolve_tier` so catalog overrides from
/// `~/.shannon/providers.toml` (Task 17) win over the static catalog.
fn handle_model_tier(repl: &mut Repl, args: &str) -> Result<()> {
    // Accept "--tier X", "--tier X provider", "--tier X provider --save".
    // First token must be either "--tier" or "--tier=<name>".
    let parts: Vec<&str> = args.split_whitespace().collect();
    let first = parts.first().copied().unwrap_or("");
    let (tier_idx, tier_str) = if first == "--tier" {
        let name = parts.get(1).copied().unwrap_or("");
        (0usize, name)
    } else if let Some(rest) = first.strip_prefix("--tier=") {
        (0usize, rest)
    } else {
        // Args did not start with --tier; the caller already filtered these.
        return Err("internal: handle_model_tier called without --tier prefix".into());
    };

    let tier = TierName::from_user_input(tier_str).ok_or_else(|| {
        format!(
            "Unknown tier '{}'. Try one of: {}",
            tier_str,
            TierName::suggestions().join(", ")
        )
    })?;
    let is_auto = matches!(tier, TierName::Auto);

    // Optional provider argument: the next non-`--save` token after `--tier <name>`.
    let explicit_provider_str = parts.get(tier_idx + 2).copied();
    let save = parts.contains(&"--save");
    let provider = match explicit_provider_str {
        Some(p) => parse_provider_name(p)?,
        None => repl.state.selected_provider.clone().ok_or_else(|| {
            "No provider selected; specify one: /model --tier <tier> <provider>".to_string()
        })?,
    };

    let profile_tiers = load_provider_tiers(&provider);
    // `auto` resolves via the lightweight best-default heuristic (standard →
    // pro → fast; ADR-0005 decision ②) — not the full task-type ModelRouter
    // (spec §11 stays unwired). The concrete tier it resolves to is what gets
    // switched and persisted; `auto` itself is never stored.
    let (tier, model_id) = if is_auto {
        let (concrete, id) =
            shannon_core::model_registry::resolve_auto_tier(&provider, &profile_tiers)
                .ok_or_else(|| format!("No model found for tier=auto provider={provider}"))?;
        (concrete, id)
    } else {
        let id = shannon_core::model_registry::resolve_tier(tier_str, &provider, &profile_tiers)
            .ok_or_else(|| {
                format!(
                    "No model found for tier={} provider={provider}",
                    tier.canonical()
                )
            })?;
        (tier, id)
    };

    // Single switch path: state, engine, preferences, and (when --save) the
    // providers.toml tier pin with rollback — all consolidated in
    // apply_model_selection (ADR-0008 Decision 2).
    let ctx_opt = apply_model_selection(
        repl,
        provider.clone(),
        Some(model_id.clone()),
        Some(tier),
        save,
    )?;

    let ctx_label = format_context_label(ctx_opt);
    // For `auto`, surface which concrete tier the heuristic picked.
    let tier_field = if is_auto {
        format!("auto → {}", tier.canonical())
    } else {
        tier.canonical().to_string()
    };
    let msg = format!(
        "{} tier={} (context: {ctx_label})",
        t!("commands.model.set", name = &model_id),
        tier_field
    );
    repl.chat.add_message(ChatRole::System, msg);
    Ok(())
}

/// Load the persisted per-tier model overrides for a provider from
/// `~/.shannon/providers.toml` (ADR-0005 Phase 4 read-back).
///
/// Returns the provider's `ProviderTiers` when a `"default"` profile with that
/// provider slot exists, else an empty `ProviderTiers` so `resolve_tier`
/// falls back to the static catalog. A corrupt/missing file degrades to empty
/// — same graceful contract as [`ProviderConfigStore::load_or_default`].
fn load_provider_tiers(provider: &LlmProvider) -> ProviderTiers {
    use shannon_core::provider_config_store::ProviderConfigStore;

    let id = shannon_core::provider_resolver::llm_provider_id(provider);
    ProviderConfigStore::load_or_default()
        .config()
        .profiles
        .get("default")
        .and_then(|p| p.providers.iter().find(|pr| pr.id == id))
        .map(|pr| pr.tiers.clone())
        .unwrap_or_default()
}

/// Persist the resolved (provider, tier) → model-id mapping back into
/// `~/.shannon/providers.toml` (ADR-0005 Phase 4). Writes both the per-tier
/// override (`set_tier`) **and** the active target (`set_active`) so the
/// choice survives a restart: the engine read-back (`resolve_active_target`)
/// reads `active_target` on launch, while `load_provider_tiers` feeds the tier
/// overrides back to `resolve_tier` for future `/model --tier` switches.
///
/// Steps:
/// 1. Load (or default-construct) a [`ProviderConfigStore`] for the v2 file.
/// 2. Get-or-create the provider profile under the `"default"` model profile.
/// 3. Write the canonical tier field (`fast` / `standard` / `pro`) using
///    `TierName::canonical()` so the persisted key is **always** the
///    canonical name — never the user-facing alias (`haiku`/`sonnet`/`opus`).
/// 4. Set `active_target` to the same provider + model so `resolve_active_target`
///    picks it up on the next launch.
/// 5. Atomically persist via `store.save()`.
///
/// `TierName::Auto` is rejected here as a defense-in-depth: `handle_model_tier`
/// resolves `--tier auto` to a concrete tier (via `resolve_auto_tier`) *before*
/// calling this, so `Auto` never reaches persistence in practice — only
/// canonical `fast`/`standard`/`pro` are ever stored. The guard remains because
/// `set_tier` would otherwise silently ignore `Auto`, and a corrupt tier would
/// be harder to detect than an explicit error.
fn persist_model_to_providers_toml(
    provider: &LlmProvider,
    model_id: &str,
    tier: TierName,
) -> Result<()> {
    use shannon_core::provider_config_store::ProviderConfigStore;

    if matches!(tier, TierName::Auto) {
        return Err("auto must be resolved to a concrete tier before persisting".into());
    }

    let mut store = ProviderConfigStore::load_or_default();
    store
        .set_tier(provider, tier, model_id)
        .set_active(provider, model_id)
        .save()
        .map_err(|e| {
            format!(
                "failed to persist tier override to providers.toml: {e} \
                 (provider={}, tier={}, model_id={})",
                format!("{provider:?}").to_lowercase(),
                tier.canonical(),
                model_id,
            )
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::connect::{parse_connect_args, should_prompt_for_key};
    use super::*;

    #[test]
    fn parse_provider_name_aliases() {
        assert!(matches!(
            parse_provider_name("claude"),
            Ok(LlmProvider::Anthropic)
        ));
        assert!(matches!(
            parse_provider_name("gpt"),
            Ok(LlmProvider::OpenAI)
        ));
        assert!(matches!(
            parse_provider_name("ds"),
            Ok(LlmProvider::DeepSeek)
        ));
        assert!(matches!(
            parse_provider_name("local"),
            Ok(LlmProvider::Ollama)
        ));
        assert!(matches!(parse_provider_name("grok"), Ok(LlmProvider::Xai)));
        assert!(matches!(parse_provider_name("glm"), Ok(LlmProvider::Zhipu)));
    }

    #[test]
    fn parse_provider_name_case_insensitive() {
        assert!(matches!(
            parse_provider_name("ANTHROPIC"),
            Ok(LlmProvider::Anthropic)
        ));
        assert!(matches!(
            parse_provider_name("OpenAI"),
            Ok(LlmProvider::OpenAI)
        ));
    }

    #[test]
    fn parse_provider_name_unknown() {
        assert!(parse_provider_name("unknown_provider").is_err());
    }

    // ── first_flag_is (ADR-0008 P2-4, /model flag dispatch) ──────────────

    #[test]
    fn first_flag_is_matches_exact_and_equals_form() {
        assert!(first_flag_is("--tier standard", "--tier"));
        assert!(first_flag_is("--tier=standard", "--tier"));
        assert!(first_flag_is("--max-tokens 8192", "--max-tokens"));
        assert!(first_flag_is("--max-tokens=8192 --save", "--max-tokens"));
    }

    #[test]
    fn first_flag_is_rejects_shared_prefix() {
        // The bug P2-4 fixes: `starts_with("--tier")` used to route this into
        // the tier handler.
        assert!(!first_flag_is("--tierfoo", "--tier"));
        assert!(!first_flag_is("--tierx standard", "--tier"));
        // `--tier` is not a prefix of `--max-tokens` (the old comment claimed
        // otherwise).
        assert!(!first_flag_is("--max-tokens 8192", "--tier"));
        assert!(!first_flag_is("--tier standard", "--max-tokens"));
    }

    #[test]
    fn first_flag_is_empty_or_non_flag() {
        assert!(!first_flag_is("", "--tier"));
        assert!(!first_flag_is("sonnet", "--tier"));
        assert!(!first_flag_is("refresh", "--tier"));
    }

    // ── parse_connect_args (ADR-0005 Phase 4, /connect) ─────────────────

    #[test]
    fn parse_connect_args_empty_is_none() {
        assert!(parse_connect_args("").is_none());
        assert!(parse_connect_args("   ").is_none());
        assert!(parse_connect_args("\t\n").is_none());
    }

    #[test]
    fn parse_connect_args_provider_only() {
        let p = parse_connect_args("anthropic").expect("some");
        assert_eq!(p.provider_arg, "anthropic");
        assert!(p.key_arg.is_none());
    }

    #[test]
    fn parse_connect_args_provider_and_key() {
        let p = parse_connect_args("anthropic sk-ant-api03-xxx").expect("some");
        assert_eq!(p.provider_arg, "anthropic");
        assert_eq!(p.key_arg, Some("sk-ant-api03-xxx"));
    }

    #[test]
    fn parse_connect_args_trims_surrounding_whitespace() {
        let p = parse_connect_args("   openai   sk-test123   ").expect("some");
        assert_eq!(p.provider_arg, "openai");
        assert_eq!(p.key_arg, Some("sk-test123"));
    }

    #[test]
    fn parse_connect_args_blank_key_is_none() {
        // Trailing whitespace but no key → no key.
        let p = parse_connect_args("ollama   ").expect("some");
        assert_eq!(p.provider_arg, "ollama");
        assert!(p.key_arg.is_none());
    }

    // ── connect_status (ADR-0005 Phase 2, /connect dashboard) ───────────

    #[test]
    fn connect_status_no_auth_provider() {
        // Ollama-style: never needs a key, regardless of connection/key state.
        assert_eq!(connect_status(false, false, false).to_string(), "no auth");
        assert_eq!(connect_status(false, true, true).to_string(), "no auth");
        assert_eq!(
            connect_status(false, false, false),
            ProviderConnectionStatus::NoAuth
        );
    }

    #[test]
    fn connect_status_authed_fully_connected() {
        // Profile persisted + key stored → fully wired.
        assert_eq!(connect_status(true, true, true).to_string(), "✓ connected");
        assert_eq!(
            connect_status(true, true, true),
            ProviderConnectionStatus::Connected
        );
    }

    #[test]
    fn connect_status_key_but_no_profile() {
        // Key exists in the store but /connect hasn't persisted a profile.
        assert_eq!(connect_status(true, false, true).to_string(), "key stored");
        assert_eq!(
            connect_status(true, false, true),
            ProviderConnectionStatus::KeyStored
        );
    }

    #[test]
    fn connect_status_profile_but_no_key() {
        // Stale profile with no key → treat as "no key" (not functional).
        assert_eq!(connect_status(true, true, false).to_string(), "no key");
    }

    #[test]
    fn connect_status_nothing_stored() {
        assert_eq!(connect_status(true, false, false).to_string(), "no key");
    }

    // ── should_prompt_for_key (ADR-0005 Phase 4, /connect wizard) ─────────

    #[test]
    fn should_prompt_for_key_when_auth_no_key_nothing_stored() {
        // The one case the wizard exists for: needs a key, none given, none stored.
        assert!(should_prompt_for_key(true, None, false));
    }

    #[test]
    fn should_prompt_for_key_not_when_inline_key_given() {
        assert!(!should_prompt_for_key(true, Some("sk-x"), false));
    }

    #[test]
    fn should_prompt_for_key_not_when_key_already_stored() {
        // Reconnect flow: key on disk → no need to ask again.
        assert!(!should_prompt_for_key(true, None, true));
    }

    #[test]
    fn should_prompt_for_key_not_for_no_auth_provider() {
        // Ollama-style providers never prompt, regardless of stored state.
        assert!(!should_prompt_for_key(false, None, false));
        assert!(!should_prompt_for_key(false, None, true));
    }

    #[test]
    fn parse_color_string_named() {
        use ratatui::style::Color;
        assert_eq!(parse_color_string("red"), Some(Color::Red));
        assert_eq!(parse_color_string("purple"), Some(Color::Magenta));
        assert_eq!(parse_color_string("grey"), Some(Color::Gray));
        assert_eq!(parse_color_string("light_blue"), Some(Color::LightBlue));
    }

    #[test]
    fn parse_color_string_hex() {
        use ratatui::style::Color;
        assert_eq!(parse_color_string("#ff0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color_string("#00ff00"), Some(Color::Rgb(0, 255, 0)));
    }

    #[test]
    fn parse_color_string_invalid() {
        assert_eq!(parse_color_string("notacolor"), None);
        assert_eq!(parse_color_string("#xyz"), None);
    }

    // ── resolve_model_arg (ADR-0005 Phase 3) ───────────────────────────

    #[test]
    fn resolve_model_arg_qualified_full_id() {
        let (id, provider) = resolve_model_arg("anthropic/claude-sonnet-4-20250514");
        assert_eq!(id, "claude-sonnet-4-20250514");
        assert_eq!(provider, Some(LlmProvider::Anthropic));
    }

    #[test]
    fn resolve_model_arg_qualified_alias_expands_within_provider() {
        let (id, provider) = resolve_model_arg("anthropic/sonnet");
        assert_ne!(id, "sonnet");
        assert!(id.starts_with("claude-"), "got {id}");
        assert_eq!(provider, Some(LlmProvider::Anthropic));
    }

    #[test]
    fn resolve_model_arg_qualified_unknown_model_kept_and_provider_set() {
        let (id, provider) = resolve_model_arg("ollama/llama3");
        assert_eq!(id, "llama3");
        assert_eq!(provider, Some(LlmProvider::Ollama));
    }

    #[test]
    fn resolve_model_arg_bare_alias_resolves() {
        let (id, provider) = resolve_model_arg("sonnet");
        assert!(id.starts_with("claude-sonnet"), "got {id}");
        assert_eq!(provider, Some(LlmProvider::Anthropic));
    }

    #[test]
    fn resolve_model_arg_bare_full_id() {
        let (id, provider) = resolve_model_arg("claude-sonnet-4-20250514");
        assert_eq!(id, "claude-sonnet-4-20250514");
        assert_eq!(provider, Some(LlmProvider::Anthropic));
    }

    #[test]
    fn resolve_model_arg_bare_unknown_no_provider() {
        // A bare id not in the catalog: kept as-is, no provider inferred.
        let (id, provider) = resolve_model_arg("llama3");
        assert_eq!(id, "llama3");
        assert!(provider.is_none());
    }

    #[test]
    fn resolve_model_arg_trims_whitespace() {
        let (id, provider) = resolve_model_arg("  anthropic/claude-sonnet-4-20250514  ");
        assert_eq!(id, "claude-sonnet-4-20250514");
        assert_eq!(provider, Some(LlmProvider::Anthropic));
    }
}
