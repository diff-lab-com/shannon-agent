//! `/model` command handlers — split from `config.rs` (ADR-0008 P2-8).
//!
//! `/model` switches the active model id (bare alias, qualified
//! `provider/model`, or tier via `--tier`), refreshes the dynamic catalog
//! (`refresh`), or overrides the per-request max-tokens ceiling
//! (`--max-tokens`). The cross-group helpers (`apply_model_selection`,
//! `first_flag_is`, `parse_provider_name`, `resolve_model_arg`) live in the
//! parent [`super`] module; `persist_model_to_providers_toml` (the
//! `(provider, tier) → id` write-back used by /provider and /connect too) also
//! stays there.

use super::{apply_model_selection, first_flag_is, parse_provider_name, resolve_model_arg};
use crate::repl::Repl;
use crate::{Result, widgets::ChatRole};
use rust_i18n::t;
use shannon_core::model_registry;
use shannon_engine::api::LlmProvider;
use shannon_types::provider_config::{ProviderTiers, TierName};

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
    use shannon_core::provider_config_service::ProviderConfigService;

    // Route through the single write path (ADR-0008 P2-5) shared with
    // `/connect` and the CLI's `providers add`.
    let mut svc = ProviderConfigService::load();
    svc.set_max_tokens(provider, next)
        .map_err(|e| format!("failed to persist default_max_tokens to providers.toml: {e}"))?;
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
