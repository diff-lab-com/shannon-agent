use super::super::Repl;
use crate::Result;
use shannon_core::model_registry;
use shannon_core::provider_resolver::resolve_model_ref;
use shannon_engine::api::LlmProvider;
use shannon_types::model_ref::ModelRef;
use shannon_types::provider_config::TierName;

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
mod appearance;
mod config_kv;
mod connect;
mod model;
mod provider;

pub(crate) use appearance::{
    handle_accessibility, handle_color, handle_lang, handle_statusline, handle_terminal_setup,
    handle_theme,
};
pub(crate) use config_kv::{
    handle_config, handle_context, handle_init, handle_local_models, handle_mode,
};
pub(crate) use connect::{handle_connect, handle_disconnect};
pub(crate) use model::handle_model;
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
    use super::appearance::parse_color_string;
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
