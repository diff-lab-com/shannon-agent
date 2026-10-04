//! Chat-related Tauri commands (extracted from `commands.rs`).
//!
//! First step of the commands.rs decomposition (R2-A3 / P1.1). The chat
//! domain is the smallest cohesive cluster that touches AppState directly
//! without dragging in session/config/mcp plumbing — good template for the
//! later, larger extractions.
//!
//! Functions stay registered under their original `commands::*` path via
//! `pub use crate::commands_chat::*;` in `commands.rs`, so the invoke_handler
//! list in `main.rs` does not change.

use crate::commands::{AppState, ChatMessage, ModelInfo, StatusResponse, ToolInfo};

/// Get all conversation messages.
///
/// P0-4: reads from the active session in `state.registry` instead of the
/// (removed) `state.messages` field.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn get_conversation(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ChatMessage>, String> {
    let session = state.registry.get_or_create_active();
    let messages = session.messages.lock().await;
    Ok(messages.clone())
}

/// A-6 fix (R4 group 3): the id of the ACTIVE session — the one
/// [`get_conversation`] answers for. The main window's cold start loaded the
/// conversation but had no way to learn WHICH session it came from (the
/// `Vec<ChatMessage>` return carries no identity), leaving the UI's
/// `currentSessionId` null until the first manual switch. Read-only by
/// design: `SessionRegistry::active_key` never materializes a session, so
/// calling this after `get_conversation` (which does the materializing)
/// reports exactly the session the rendered messages belong to.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn get_active_session_id(
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, String> {
    Ok(state.registry.active_key().map(|key| key.0.to_string()))
}

/// List available models for the current provider.
///
/// Routed through `shannon_core::model_registry::merged_models_for_provider`
/// so the desktop shell finally shares one source of truth with the CLI
/// (ADR-0005 Phase 2 / task 4) — the previous hard-coded `match` returned a
/// stale, three-model snapshot that diverged from `MODEL_CATALOG` and the
/// dynamic models.dev overlay. Unknown context windows render as `0` (the
/// UI surfaces "unknown" rather than fabricating a value, P0-2 honest-cost).
///
/// Honors the desktop's `enabled_providers` allowlist
/// (`shannon_core::model_registry::effective_provider_allowlist`):
/// - `None` (no desktop override) → fall back to env-var allowlist
/// - `Some(slice)` → only return models whose provider slug is in the
///   slice. The engine env vars are ignored when the desktop has an
///   explicit override (P4.9 precedence).
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn list_models(state: tauri::State<'_, AppState>) -> Result<Vec<ModelInfo>, String> {
    use shannon_core::model_registry::effective_provider_allowlist;

    let provider_str = state.client_config.read().await.provider.to_string();
    let allowlist = {
        let dc = state.desktop_config.read().await;
        dc.enabled_providers.clone()
    };
    list_models_for(
        &provider_str,
        effective_provider_allowlist(allowlist.as_deref()),
    )
}

/// Pure helper backing [`list_models`] and (in tests) `get_provider_allowlist`.
/// Given the active provider slug and the resolved allowlist, build the
/// wire [`ModelInfo`] vec.
///
/// `allowlist = Some(vec![])` means "user toggled every provider off" —
/// we return an empty list. `allowlist = Some(non_empty)` filters by
/// slug case-insensitively. `allowlist = None` means no restriction
/// (engine env-var allowlist already applied by
/// [`effective_provider_allowlist`], so this case never reaches a
/// restrictive filter).
fn list_models_for(
    provider_str: &str,
    allowlist: Option<Vec<String>>,
) -> Result<Vec<ModelInfo>, String> {
    use shannon_core::model_registry::merged_models_for_provider;
    use shannon_core::query_engine::pricing_for_model_opt;

    let provider = llm_provider_from_slug(provider_str)
        .ok_or_else(|| format!("unknown provider slug `{provider_str}`; cannot list models"))?;

    // Allowlist short-circuit: `Some(vec![])` ⇒ user toggled every
    // provider off in the desktop UI. Return empty so the picker shows
    // the "no models" state rather than a stale default.
    if let Some(slice) = allowlist.as_deref() {
        if slice.is_empty() {
            return Ok(Vec::new());
        }
        let active_slug = provider.to_string().to_lowercase();
        let allowed = slice.iter().any(|s| s.eq_ignore_ascii_case(&active_slug));
        if !allowed {
            return Ok(Vec::new());
        }
    }

    let models = merged_models_for_provider(provider);

    Ok(models
        .into_iter()
        .map(|m| {
            let pricing = pricing_for_model_opt(m.id);
            // R3-3: surface the catalog's tier classification on the wire so
            // the plan/act tier controls (header switcher, Settings →
            // Models) can show WHICH model each tier resolves to with the
            // same data the picker lists. `Unknown` stays `None` — the UI
            // renders no tier badge rather than guessing (honest metadata).
            let tier_label = shannon_core::model_registry::tier_label_for_id(m.id);
            ModelInfo {
                id: m.id.to_string(),
                name: m.display_name.to_string(),
                provider: provider_str.to_string(),
                context_window: m.context_window,
                price_in: pricing.as_ref().map(|p| p.input_price_per_mtok),
                price_out: pricing.as_ref().map(|p| p.output_price_per_mtok),
                tier: (tier_label != shannon_core::model_registry::TierLabel::Unknown)
                    .then(|| tier_label.as_str().to_string()),
                dynamic: None,
                // R2-3: the merged catalog carries real capability data for
                // both static entries (curated table) and dynamic overlay
                // entries (derived from models.dev input modalities), so
                // the vision bit is always known once a model exists.
                vision: Some(
                    m.capabilities
                        .has(shannon_core::model_registry::ModelCapabilities::vision()),
                ),
            }
        })
        .collect())
}

/// Return the currently-effective provider allowlist for the desktop UI
/// (ADR-0005 P4.9). Reads the desktop's persisted `enabled_providers`
/// override and merges with the engine's `SHANNON_*_PROVIDERS` env vars
/// via [`shannon_core::model_registry::effective_provider_allowlist`].
///
/// Return shape:
/// - `Some(vec)` when an explicit or env-var allowlist is in effect.
///   `Some(vec![])` ⇒ user toggled every provider off.
/// - `None` ⇒ no restriction (full catalog visible).
///
/// The UI uses this to render the Settings → Provider visibility
/// checkboxes in their current state (a "Reset to default" button sends
/// `null` to clear the desktop override).
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn get_provider_allowlist(
    state: tauri::State<'_, AppState>,
) -> Result<Option<Vec<String>>, String> {
    use shannon_core::model_registry::effective_provider_allowlist;
    let dc = state.desktop_config.read().await;
    Ok(effective_provider_allowlist(
        dc.enabled_providers.as_deref(),
    ))
}

/// Map a desktop provider slug (e.g. `"anthropic"`, `"openai-compatible"`) to
/// the engine's `LlmProvider` so we can walk `model_registry`. The
/// `openai-compatible` catch-all (GLM / Zhipu / Moonshot / …) maps to
/// `LlmProvider::OpenAI` for catalog-walking purposes — the actual request
/// still goes through the user's custom `base_url`. Unknown slugs return
/// `None` and the caller surfaces a friendly error.
fn llm_provider_from_slug(s: &str) -> Option<shannon_engine::api::LlmProvider> {
    use shannon_engine::api::LlmProvider;
    match s {
        "anthropic" => Some(LlmProvider::Anthropic),
        "openai" => Some(LlmProvider::OpenAI),
        "ollama" => Some(LlmProvider::Ollama),
        "gemini" => Some(LlmProvider::Gemini),
        "deepseek" => Some(LlmProvider::DeepSeek),
        // openai-compatible: collapse to OpenAI for catalog walking — the
        // real provider is whatever the user's `base_url` points at.
        "openai-compatible" => Some(LlmProvider::OpenAI),
        _ => None,
    }
}

/// Get current application status.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn get_status(state: tauri::State<'_, AppState>) -> Result<StatusResponse, String> {
    let cc = state.client_config.read().await;
    let model = cc.model.clone();
    let provider = cc.provider.to_string();
    drop(cc);
    let session = state.registry.get_or_create_active();
    let querying = session.querying.lock().await;
    let messages = session.messages.lock().await;
    let working_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());

    Ok(StatusResponse {
        model,
        provider,
        querying: *querying,
        message_count: messages.len(),
        working_dir,
    })
}

/// Cancel the in-flight query.
///
/// P1-1 (multi-window routing fix): `session_id` — when provided, cancels
/// **that** session's in-flight query without touching the shared
/// active-session pointer (an unknown id is a hard error). Without it the
/// legacy active-session fallback applies.
#[tauri::command]
pub async fn cancel_query(
    state: tauri::State<'_, AppState>,
    _app_handle: tauri::AppHandle,
    session_id: Option<String>,
) -> Result<(), String> {
    cancel_session_query(&state, session_id.as_deref()).await
}

/// B1-4 (P1-3): whether THIS session currently has a live query — the
/// single-session read the frontend's stop watchdog reconciles against
/// when the `query:cancelled` terminal event never arrives. Read-only: a
/// session the registry does not know reports idle (`false`) and is NOT
/// materialized (`get_or_create` would resurrect a deleted session's
/// entry just by asking about it). A malformed id is a hard error, same
/// vocabulary as [`SessionRegistry::resolve_explicit_or_active`].
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn get_session_querying(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<bool, String> {
    let uuid = uuid::Uuid::parse_str(session_id.trim())
        .map_err(|e| format!("invalid sessionId: {e}"))?;
    Ok(state.registry.is_querying(uuid).await)
}

/// Body of [`cancel_query`], split out so the routing behavior is testable
/// without a Wry app handle.
///
/// A-17 fix (R4 group 6): cancel takes + fires the token but deliberately
/// does NOT reset `session.querying` here. The streaming loop observes the
/// cancellation immediately (A-18 fix, R4 group 7: the token races the
/// stream via `tokio::select!` in commands.rs), so the loop now unwinds
/// promptly — but the latch is still reset exclusively by the query loop's
/// exit path (`send_message`'s spawned task — it runs on EVERY exit: ok,
/// engine error, cancel, and the caught panic), so a resend in the
/// window is rejected with "A query is already in progress" until the old
/// loop has actually unwound.
async fn cancel_session_query(state: &AppState, session_id: Option<&str>) -> Result<(), String> {
    let (_, session) = state.registry.resolve_explicit_or_active(session_id)?;

    // Take the cancellation token and cancel it. The querying latch stays
    // held (see the A-17 note above) — the loop exit resets it.
    let token_opt = {
        let mut token_guard = session.cancellation_token.lock().await;
        token_guard.take()
    };

    if let Some(token) = token_opt {
        token.cancel();
    } else if *session.querying.lock().await {
        // B1-4 (P1-3): the stop landed in `send_message`'s latch→token
        // window (the latch is up but the token is not stored yet), where
        // this used to be a silent success — no token fired, no
        // `query:cancelled` would ever be emitted, and the frontend's stop
        // button waited forever. Record the intent instead;
        // `send_message` consumes it right after storing the token and
        // cancels the fresh run immediately. With the latch DOWN there is
        // nothing running and nothing to wait for — the historical no-op
        // stays (and must not set the flag, or the next legitimate send
        // would start pre-cancelled).
        session.set_cancel_pending();
    }

    Ok(())
}

/// List available tools.
#[tauri::command]
pub async fn list_tools(state: tauri::State<'_, AppState>) -> Result<Vec<ToolInfo>, String> {
    let tools = state.tools.list_tools_info();
    Ok(tools
        .into_iter()
        .map(|t| ToolInfo {
            name: t.name,
            description: t.description,
            enabled: true,
        })
        .collect())
}

// ===== R2-1 — session-level model override (composer chip) =====
//
// The composer model chip used to funnel straight into the global
// `configure('model')` / `configure('provider')` arms, so switching models
// in one chat silently re-targeted EVERY chat. R2-1 splits the two intents:
//
//   - picking a model in the chip writes a **session override**
//     (`set_session_model`) that only affects subsequent queries of that
//     session;
//   - the chip's "Set as default" action performs today's global configure
//     (existing semantics, the frontend keeps calling `configure`).
//
// Precedence: session override > global default (`AppState::client_config`,
// itself built from the engine store's `active_target`). A session without
// an override — including every new chat — inherits the global default.
// The override lives on `SessionState` (in-memory, app lifetime; see
// `SessionModelOverride` for the restart story) and the chip renders a
// "session" suffix while one is active so the state is never silent.

/// Resolve the effective client config for a query on `session`:
/// the session override re-resolved against the engine store when one is
/// set, the global `client_config` (possibly upgraded to the R3-3 phase
/// tier model) otherwise.
///
/// Resolution failure (the overridden provider was deleted from the engine
/// store since the override was written) degrades to the global config with
/// a warning — a stale override must never block a send.
///
/// R5-5 — precedence table across the three run classes (pinned by tests
/// here and in `session_override_tests`):
///
/// ```text
/// run class            config source                              override/tier applied?
/// ───────────────────  ─────────────────────────────────────────  ──────────────────────
/// interactive session  resolve_client_config_for_session (HERE)   session override (R2-1)
///                                                                 > phase tier (R3-3)
///                                                                 > global default
/// unattended           `state.client_config` read DIRECTLY by     NONE — global default only;
/// (goal / batch /      the run constructors                       overrides and phase tiers
/// routine / dream /    (GoalRunDeps, BatchRunDeps,                are intentionally invisible:
/// skill loop)          RoutineRunDeps, commands_skill_loop,       an unattended run must track
///                      commands_dream)                            the user's global target, not
///                                                                 whatever chat happened to be
///                                                                 focused when it fired
/// desktop global       AppState::client_config (the store's       —
///                      active_target, rebuilt by `configure`)
/// ```
///
/// The unattended pin holds because (a) `set_session_model` never writes
/// `state.client_config` (session state only — see the write-through below),
/// (b) phase tiers are consulted exclusively inside this function (and its
/// `apply_phase_tier_to_base` helper), and (c) the unattended constructors
/// clone the `client_config` Arc without resolving through here. The
/// `unattended_paths_pin_global_config` test in this file pins (a)+(b)+(c).
pub(crate) async fn resolve_client_config_for_session(
    state: &AppState,
    session: &crate::session_registry::SessionState,
) -> shannon_engine::api::LlmClientConfig {
    let base = state.client_config.read().await.clone();
    let Some(ov) = session.model_override_snapshot() else {
        // R3-3: no explicit session override — the phase tier (plan/act)
        // preference applies, if one is configured and resolves.
        return apply_phase_tier_to_base(state, base).await;
    };
    let store_config = {
        let store = state.provider_store.lock().await;
        store.config().clone()
    };
    match apply_session_override(&base, &store_config, &ov.provider, &ov.model) {
        Some(cc) => cc,
        None => {
            tracing::warn!(
                provider = %ov.provider,
                model = %ov.model,
                "session model override no longer resolvable — falling back to global default"
            );
            base
        }
    }
}

/// R3-3: apply the plan/act phase-tier preference to the global base config.
///
/// The tier (if any) is chosen from the desktop config's `plan_tier` /
/// `act_tier` keys by the current approval mode (`plan` ⇒ plan phase), then
/// resolved to a concrete model via the engine tier resolution. Any failure
/// to resolve (tier unset/invalid, provider absent from the store, no
/// catalog match) degrades silently to the unmodified global default — a
/// tier preference must never block a send.
async fn apply_phase_tier_to_base(
    state: &AppState,
    base: shannon_engine::api::LlmClientConfig,
) -> shannon_engine::api::LlmClientConfig {
    let (plan_tier, act_tier, approval_mode) = {
        let dc = state.desktop_config.read().await;
        (
            dc.plan_tier.clone(),
            dc.act_tier.clone(),
            dc.approval_mode.clone(),
        )
    };
    let Some(tier) = crate::phase_tier::effective_phase_tier(
        approval_mode.as_deref(),
        plan_tier.as_deref(),
        act_tier.as_deref(),
    ) else {
        return base;
    };
    let store_config = {
        let store = state.provider_store.lock().await;
        store.config().clone()
    };
    match apply_tier_override(&base, &store_config, tier) {
        Some(cc) => cc,
        None => {
            tracing::debug!(
                tier = tier.as_str(),
                provider = %base.provider,
                "phase tier did not resolve for the active provider — using global default model"
            );
            base
        }
    }
}

/// Pure body of [`apply_phase_tier_to_base`]: swap the base config's model
/// for `tier`'s resolved concrete model.
///
/// The tier resolves **for the provider already in use** (`base.provider`,
/// the engine store's active target) so phase switching stays inside the
/// configured provider — matching the tier vocabulary (`fast`/`standard`/
/// `pro`) the Add Provider modal exposes per connection. The profile's
/// persisted `tiers` table (providers.toml v2, the same source the TUI's
/// `/model --tier --save` writes) feeds the resolution first; catalog
/// inference with the cost tie-break comes second. Returns `None` when the
/// active target or its provider slot is missing, or the tier doesn't
/// resolve — the caller keeps the base config unchanged.
pub(crate) fn apply_tier_override(
    base: &shannon_engine::api::LlmClientConfig,
    config: &shannon_types::provider_config::ProviderModelConfig,
    tier: crate::phase_tier::PhaseTier,
) -> Option<shannon_engine::api::LlmClientConfig> {
    let profile = config.active_model_profile()?;
    let active_id = profile.active_target.provider_id.trim();
    if active_id.is_empty() {
        return None;
    }
    let entry = profile.providers.iter().find(|p| p.id == active_id)?;
    let model = crate::phase_tier::resolve_tier_model(tier, base.provider.clone(), &entry.tiers)?;
    let mut out = base.clone();
    out.model = model;
    Some(out)
}

/// Pure body of [`resolve_client_config_for_session`]: apply a
/// `(provider_slug, model_id)` override on top of the global client config,
/// resolving provider identity, base URL and credential from the engine
/// store's **active** profile roster (`active_profile_key()` — `"default"`
/// when unset; R3-2 renamed the old hardcoded `"default"` lookup so a
/// session override keeps resolving against the same roster the global
/// target was built from) — swapping only provider/model on the global
/// config would keep the OLD provider's base_url and API key, which fails
/// for any cross-provider override.
///
/// Returns `None` when no managed provider matches `provider_slug` — the
/// caller falls back to the global config. Behavioural overrides
/// (`max_tokens`, `timeout`, reasoning effort, retry config …) come from
/// `base` unchanged.
pub(crate) fn apply_session_override(
    base: &shannon_engine::api::LlmClientConfig,
    config: &shannon_types::provider_config::ProviderModelConfig,
    provider_slug: &str,
    model_id: &str,
) -> Option<shannon_engine::api::LlmClientConfig> {
    use shannon_core::provider_resolver::{llm_provider_from_slug, resolve_credential};

    let slug = provider_slug.trim().to_lowercase();
    let profile = config
        .active_model_profile()?
        .providers
        .iter()
        // Match on both slug vocabularies the codebase speaks: the
        // kebab-case provider-kind slug the UI sends (and
        // `configure('provider')` stores against) and the profile id
        // (a canonical `LlmProvider` name, e.g. "zhipu"), so overrides
        // written from either vocabulary resolve.
        .find(|p| {
            crate::commands_config::provider_kind_slug(&p.kind) == slug
                || llm_provider_from_slug(&p.id).is_some_and(|lp| lp.to_string() == slug)
        })?;

    let provider = llm_provider_from_slug(&slug).unwrap_or_else(|| {
        shannon_core::provider_resolver::resolve_provider(&profile.kind, &profile.base_url)
    });
    let mut out = base.clone();
    out.provider = provider;
    out.base_url = profile.base_url.clone();
    out.api_key = resolve_credential(&profile.credential);
    out.model = model_id.to_string();
    out.extra_headers = profile.extra_headers.clone();
    // R2-1 × R3-1 contract: an explicitly pinned session target is the user
    // telling the engine exactly where to send traffic — failover must not
    // second-guess it (documented precedence: session override > phase tier
    // > global default, and pinned targets don't fail over).
    out.retry_config.suppress_failover = true;
    Some(out)
}

/// Pin a session-level model override: subsequent queries of THIS session
/// use `provider` + `model`; other sessions and new chats keep the global
/// default.
///
/// The model value runs through the same `normalize_model_id` repair as
/// `configure('model')` (legacy clients may still send a display name), and
/// `provider` must name a managed provider in the engine store — an
/// override that could never resolve is rejected at write time instead of
/// silently falling back at query time.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn set_session_model(
    state: tauri::State<'_, AppState>,
    session_id: Option<String>,
    provider: String,
    model: String,
) -> Result<(), String> {
    let (_, session) = state
        .registry
        .resolve_explicit_or_active(session_id.as_deref())?;

    let provider_clean = provider.trim().to_lowercase();
    if provider_clean.is_empty() {
        return Err("set_session_model: provider must not be empty".into());
    }

    // Validate against the engine store AND normalize the model id inside
    // the same critical section, so the stored override is always
    // resolvable (`apply_session_override` can find the provider).
    let engine_provider = {
        let store = state.provider_store.lock().await;
        let config = store.config();
        // Same roster `apply_session_override` resolves against: the ACTIVE
        // model profile (R3-2), `"default"` when the pointer is unset.
        let kind_ok = config.active_model_profile().is_some_and(|pf| {
            pf.providers
                .iter()
                .any(|p| crate::commands_config::provider_kind_slug(&p.kind) == provider_clean)
        });
        if !kind_ok {
            return Err(format!(
                "set_session_model: no managed provider with kind `{provider_clean}` — \
                 add one in Settings → Models first"
            ));
        }
        crate::commands_config::llm_provider_for_active_mirror(&provider_clean).ok_or_else(
            || format!("set_session_model: unsupported provider kind `{provider_clean}`"),
        )?
    };
    let model_id = crate::commands_config::normalize_model_id(engine_provider, &model);

    let ov = crate::session_registry::SessionModelOverride {
        provider: provider_clean,
        model: model_id,
    };
    *session
        .model_override
        .lock()
        .map_err(|_| "session model override lock poisoned".to_string())? = Some(ov.clone());
    // R5-1: write through to the durable sidecar so the override survives a
    // restart. Best-effort — the in-memory override is already live, and a
    // failed save must not make the UI revert a working switch; the next
    // successful write-through persists it.
    if let Err(e) = persist_session_override(&state, session.session_id, Some(ov)) {
        tracing::warn!(
            session = %session.session_id,
            error = %e,
            "session model override could not be persisted — survives in memory only"
        );
    }
    Ok(())
}

/// R5-1 — durable write-through for the session model override sidecar.
/// `override_value = None` clears the entry. Re-prunes the whole map
/// against the L0 session log before saving (the documented prune policy:
/// load-time prune is memory-only, writes reconcile), so deleted sessions'
/// stale entries eventually reach disk too.
fn persist_session_override(
    state: &AppState,
    session_id: uuid::Uuid,
    override_value: Option<crate::session_registry::SessionModelOverride>,
) -> Result<(), String> {
    let sessions_dir = state.state_manager.sessions_dir().to_path_buf();
    let mut store = state
        .session_overrides
        .lock()
        .map_err(|_| "session override sidecar lock poisoned".to_string())?;
    store.record(session_id, override_value, |id| {
        shannon_core::session_log::session_log_container_path(&sessions_dir, id).exists()
    })
}

/// Clear the session-level model override — the session goes back to
/// inheriting the global default (including future default changes).
/// Idempotent: clearing a session without an override is a no-op.
///
/// R5-1: the cleared state is written through to the sidecar so a restart
/// doesn't resurrect the override.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn clear_session_model(
    state: tauri::State<'_, AppState>,
    session_id: Option<String>,
) -> Result<(), String> {
    let (_, session) = state
        .registry
        .resolve_explicit_or_active(session_id.as_deref())?;
    *session
        .model_override
        .lock()
        .map_err(|_| "session model override lock poisoned".to_string())? = None;
    // R5-1 write-through (best-effort, same contract as the set path).
    if let Err(e) = persist_session_override(&state, session.session_id, None) {
        tracing::warn!(
            session = %session.session_id,
            error = %e,
            "session model override clear could not be persisted — in-memory only"
        );
    }
    Ok(())
}

/// Read the session's model override, `None` when the session inherits the
/// global default. The composer chip polls this on session switch / after
/// mutations to keep its "· session" indicator honest.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn get_session_model(
    state: tauri::State<'_, AppState>,
    session_id: Option<String>,
) -> Result<Option<crate::session_registry::SessionModelOverride>, String> {
    let (_, session) = state
        .registry
        .resolve_explicit_or_active(session_id.as_deref())?;
    Ok(session.model_override_snapshot())
}

/// P2-5 — session-level "temporary chat" toggle: `disabled = true` builds
/// this session's subsequent queries WITHOUT the memory layer (no injection
/// of past memories into the prompt, no auto-extraction of new ones). Other
/// sessions are untouched. Takes effect on the next send (the engine is
/// rebuilt per turn).
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn set_session_memory_bypass(
    state: tauri::State<'_, AppState>,
    session_id: Option<String>,
    disabled: bool,
) -> Result<(), String> {
    let (_, session) = state
        .registry
        .resolve_explicit_or_active(session_id.as_deref())?;
    session.set_memory_disabled(disabled);
    // Durable write-through (best-effort, same contract as the model
    // override paths): a failed save must not flip the toggle back in the
    // UI; the next write-through retries the disk.
    if let Err(e) = persist_memory_bypass(&state, session.session_id, disabled) {
        tracing::warn!(
            session = %session.session_id,
            error = %e,
            "session memory bypass flag could not be persisted — in-memory only"
        );
    }
    Ok(())
}

/// Read the session's "temporary chat" flag (`false` = memory in use). The
/// composer toggle polls this on session switch so the control never shows a
/// stale state after a focus change.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn get_session_memory_bypass(
    state: tauri::State<'_, AppState>,
    session_id: Option<String>,
) -> Result<bool, String> {
    let (_, session) = state
        .registry
        .resolve_explicit_or_active(session_id.as_deref())?;
    Ok(session.memory_disabled_snapshot())
}

/// P2-5 — durable write-through for the memory-bypass sidecar (re-prunes the
/// whole map against the L0 session log before saving; the documented policy
/// is identical to the model-override sidecar's).
fn persist_memory_bypass(
    state: &AppState,
    session_id: uuid::Uuid,
    disabled: bool,
) -> Result<(), String> {
    let sessions_dir = state.state_manager.sessions_dir().to_path_buf();
    let mut store = state
        .session_memory_bypass
        .lock()
        .map_err(|_| "session memory bypass sidecar lock poisoned".to_string())?;
    store.record(session_id, disabled, |id| {
        shannon_core::session_log::session_log_container_path(&sessions_dir, id).exists()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Provider allowlist filter (ADR-0005 P4.9) ===

    #[test]
    fn list_models_for_returns_empty_when_allowlist_is_empty() {
        // `Some(vec![])` is the user-set "hide every provider" state.
        // The picker should show the "no models" state rather than
        // falling back to the full catalog.
        let out = list_models_for("anthropic", Some(vec![])).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn list_models_for_filters_when_active_slug_not_in_allowlist() {
        // The desktop's active provider is `openai`, but the
        // allowlist only includes `anthropic`. The picker must
        // surface no models for the (filtered-out) active provider.
        let out = list_models_for("openai", Some(vec!["anthropic".into()])).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn list_models_for_returns_catalog_when_active_slug_in_allowlist() {
        // Allowlist matches the active provider → return the full
        // catalog for that provider.
        let out =
            list_models_for("anthropic", Some(vec!["anthropic".into(), "openai".into()])).unwrap();
        assert!(!out.is_empty(), "anthropic has catalog entries");
        assert!(out.iter().all(|m| m.provider == "anthropic"));
    }

    #[test]
    fn list_models_for_returns_full_catalog_when_allowlist_is_none() {
        // No restriction (engine env-var allowlist already applied
        // upstream, so this case is "no restriction" from this
        // function's view).
        let out = list_models_for("anthropic", None).unwrap();
        assert!(!out.is_empty());
        assert!(out.iter().all(|m| m.provider == "anthropic"));
    }

    #[test]
    fn list_models_for_allowlist_match_is_case_insensitive() {
        // The catalog slugs are lowercase ("anthropic"); a user
        // typing "Anthropic" in the env var must still hit.
        let out = list_models_for("anthropic", Some(vec!["ANTHROPIC".into()])).unwrap();
        assert!(!out.is_empty());
    }

    // === R2-1: session-level model override resolution ===
    //
    // `apply_session_override` is the pure core of the override path —
    // these tests pin precedence (session override wins over the global
    // target it is layered on), full re-resolution (provider identity,
    // base_url, credential — NOT just the model string, or a cross-provider
    // override would keep the old provider's endpoint and key), and the
    // stale-override fallback.
    mod session_override_tests {
        use super::super::apply_session_override;
        use crate::commands::AppState;
        use shannon_core::provider_config_store::ProviderConfigStore;
        use shannon_engine::api::LlmClientConfig;
        use shannon_types::provider_config::{
            ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderKind,
            ProviderModelConfig, ProviderProfile, ProviderTiers, Scope,
        };
        use std::collections::HashMap;

        /// Two managed providers under the `"default"` profile: the active
        /// Anthropic connection plus an OpenAI-compatible GLM connection —
        /// the minimal roster a cross-provider override needs.
        fn fixture_store() -> ProviderConfigStore {
            let anthropic = ProviderProfile {
                id: "anthropic".to_string(),
                kind: ProviderKind::Anthropic,
                display_name: "Anthropic".to_string(),
                base_url: "https://api.anthropic.com".to_string(),
                models_url: None,
                credential: CredentialRef::Env {
                    var: "SOR_TEST_ANTHROPIC_KEY".to_string(),
                },
                extra_headers: HashMap::new(),
                default_max_tokens: None,
                fallback_models: Vec::new(),
                quirks: Default::default(),
                tiers: ProviderTiers::default(),
                models: Vec::new(),
            };
            let mut glm_headers = HashMap::new();
            glm_headers.insert("X-Glm".to_string(), "yes".to_string());
            let glm = ProviderProfile {
                id: "zhipu".to_string(),
                kind: ProviderKind::OpenAiCompatible,
                display_name: "GLM (Zhipu)".to_string(),
                base_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
                models_url: None,
                credential: CredentialRef::Env {
                    var: "SOR_TEST_GLM_KEY".to_string(),
                },
                extra_headers: glm_headers,
                default_max_tokens: None,
                fallback_models: Vec::new(),
                quirks: Default::default(),
                tiers: ProviderTiers::default(),
                models: Vec::new(),
            };
            let mut profiles = HashMap::new();
            profiles.insert(
                "default".to_string(),
                ModelProfile {
                    name: "default".to_string(),
                    active_target: ActiveTarget {
                        provider_id: "anthropic".to_string(),
                        model_id: "claude-sonnet-4-6".to_string(),
                        scope: Scope::Global,
                    },
                    providers: vec![anthropic, glm],
                    auxiliary: HashMap::new(),
                    credential_scope: CredentialScope::Shared,
                },
            );
            ProviderConfigStore::from_config(ProviderModelConfig {
                version: ProviderModelConfig::VERSION,
                active_profile: String::new(),
                profiles,
                gateway: Default::default(),
            })
        }

        /// The global client config the fixture store's active target
        /// would produce (anthropic / claude-sonnet-4-6). `..default()`
        /// keeps the fixture decoupled from new engine fields.
        fn base_config() -> LlmClientConfig {
            LlmClientConfig {
                api_key: "global-anthropic-key".to_string(),
                base_url: "https://api.anthropic.com".to_string(),
                model: "claude-sonnet-4-6".to_string(),
                max_tokens: 8192,
                timeout_seconds: 120,
                provider: shannon_engine::api::LlmProvider::Anthropic,
                ..LlmClientConfig::default()
            }
        }

        // SAFETY: unique variable names touched only by this test module;
        // nextest runs each test in its own process.
        fn set_env(key: &str, val: &str) {
            unsafe { std::env::set_var(key, val) };
        }

        #[test]
        fn cross_provider_override_swaps_provider_model_url_and_credential() {
            set_env("SOR_TEST_GLM_KEY", "glm-secret");
            let store = fixture_store();
            let base = base_config();

            let out =
                apply_session_override(&base, store.config(), "openai-compatible", "glm-5.3-flash")
                    .expect("override resolves");

            assert_eq!(
                out.provider,
                shannon_engine::api::LlmProvider::Zhipu,
                "the override re-uses the engine's own provider resolution \
                 (resolve_provider on kind + base_url: bigmodel.cn → Zhipu wire \
                 format), NOT the desktop catalog-walking OpenAI collapse — same \
                 authority `build_client_from_resolved` has for the active target"
            );
            assert_eq!(out.model, "glm-5.3-flash");
            assert_eq!(
                out.base_url, "https://open.bigmodel.cn/api/paas/v4",
                "the override provider's endpoint must replace the global one"
            );
            assert_eq!(
                out.api_key, "glm-secret",
                "credential re-resolved per profile"
            );
            assert_eq!(
                out.extra_headers.get("X-Glm").map(String::as_str),
                Some("yes"),
                "profile extra_headers apply"
            );
        }

        #[test]
        fn override_keeps_behavioural_fields_from_base() {
            let store = fixture_store();
            let base = base_config();

            let out = apply_session_override(&base, store.config(), "anthropic", "claude-opus-4-7")
                .expect("override resolves");
            assert_eq!(out.model, "claude-opus-4-7");
            assert_eq!(
                out.max_tokens, base.max_tokens,
                "max_tokens is a global behavioural setting"
            );
            assert_eq!(out.timeout_seconds, base.timeout_seconds);
            assert_eq!(out.reasoning_effort, base.reasoning_effort);
            assert_eq!(out.retry_config.max_retries, base.retry_config.max_retries);
        }

        #[test]
        fn same_provider_override_swaps_only_model() {
            let store = fixture_store();
            let base = base_config();
            let out =
                apply_session_override(&base, store.config(), "anthropic", "claude-haiku-4-5")
                    .expect("override resolves");
            assert_eq!(out.provider, base.provider);
            assert_eq!(out.base_url, base.base_url);
            assert_eq!(out.model, "claude-haiku-4-5");
        }

        #[test]
        fn profile_id_vocabulary_matches_too() {
            // The fixture's GLM profile id is "zhipu" (a canonical
            // `LlmProvider` name), its kind slug is "openai-compatible" —
            // both must resolve so overrides written from either
            // vocabulary work.
            let store = fixture_store();
            let base = base_config();
            let out = apply_session_override(&base, store.config(), "zhipu", "glm-5.3-flash")
                .expect("profile id matches");
            assert_eq!(out.model, "glm-5.3-flash");
        }

        #[test]
        fn unknown_provider_slug_returns_none() {
            let store = fixture_store();
            let base = base_config();
            assert!(
                apply_session_override(&base, store.config(), "nonexistent", "m").is_none(),
                "an unresolvable override must fall back to the global config"
            );
        }

        /// The async command body's fallback: a stale override (provider
        /// deleted from the store since it was written) degrades to the
        /// global config instead of failing the send.
        #[tokio::test]
        async fn resolve_client_config_falls_back_when_override_unresolvable() {
            let state = AppState::new();
            let key = state.registry.create();
            let session = state.registry.get(key).unwrap();
            *session.model_override.lock().unwrap() =
                Some(crate::session_registry::SessionModelOverride {
                    provider: "nonexistent".into(),
                    model: "ghost-model".into(),
                });

            let resolved = super::super::resolve_client_config_for_session(&state, &session).await;
            let base = state.client_config.read().await.clone();
            assert_eq!(
                resolved.model, base.model,
                "stale override → global default"
            );
            assert_eq!(resolved.provider, base.provider);
        }

        /// Without an override and without any phase-tier preference the
        /// resolution is the identity — the global config passes through
        /// untouched (new chats inherit the default).
        #[tokio::test]
        async fn resolve_client_config_without_override_is_identity() {
            let state = AppState::new();
            let key = state.registry.create();
            let session = state.registry.get(key).unwrap();
            assert!(session.model_override_snapshot().is_none());
            // R3-3: pin the phase-tier prefs off — `AppState::new()` loads
            // the ambient on-disk config, which is free to carry them.
            {
                let mut cfg = state.desktop_config.write().await;
                cfg.plan_tier = None;
                cfg.act_tier = None;
            }

            let resolved = super::super::resolve_client_config_for_session(&state, &session).await;
            let base = state.client_config.read().await.clone();
            assert_eq!(resolved.model, base.model);
            assert_eq!(resolved.provider, base.provider);
        }

        // === R3-3: plan/act phase-tier preference ===

        use crate::phase_tier::PhaseTier;

        /// The tier→model resolution the assertion expects for the fixture's
        /// active provider (anthropic, default `ProviderTiers`).
        fn expected_tier_model(base: &LlmClientConfig, tier: PhaseTier) -> Option<String> {
            let store = fixture_store();
            let profile = store.config().profiles.get("default").unwrap();
            let entry = profile
                .providers
                .iter()
                .find(|p| p.id == profile.active_target.provider_id)
                .unwrap();
            crate::phase_tier::resolve_tier_model(tier, base.provider.clone(), &entry.tiers)
        }

        /// Act tier applies outside plan mode: the provider stays, the model
        /// becomes the act tier's resolution.
        #[tokio::test]
        async fn resolve_client_config_applies_act_tier_outside_plan_mode() {
            let state = AppState::new();
            *state.client_config.write().await = base_config();
            {
                let mut store = state.provider_store.lock().await;
                *store = fixture_store();
            }
            let key = state.registry.create();
            let session = state.registry.get(key).unwrap();
            {
                let mut cfg = state.desktop_config.write().await;
                cfg.approval_mode = Some("suggest".into());
                cfg.plan_tier = Some("pro".into()); // must be ignored outside plan mode
                cfg.act_tier = Some("fast".into());
            }

            let resolved = super::super::resolve_client_config_for_session(&state, &session).await;
            let base = state.client_config.read().await.clone();
            assert_eq!(resolved.provider, base.provider, "tier stays in-provider");
            assert_eq!(
                Some(resolved.model.as_str()),
                expected_tier_model(&base, PhaseTier::Fast).as_deref(),
                "act tier (fast) resolved through the engine tier logic"
            );
            assert_ne!(resolved.model, base.model, "fast tier ≠ the sonnet default");
        }

        /// Plan mode (approval_mode == "plan") resolves through the PLAN
        /// tier, not the act tier.
        #[tokio::test]
        async fn resolve_client_config_plan_mode_uses_plan_tier() {
            let state = AppState::new();
            *state.client_config.write().await = base_config();
            {
                let mut store = state.provider_store.lock().await;
                *store = fixture_store();
            }
            let key = state.registry.create();
            let session = state.registry.get(key).unwrap();
            {
                let mut cfg = state.desktop_config.write().await;
                cfg.approval_mode = Some("plan".into());
                cfg.plan_tier = Some("pro".into());
                cfg.act_tier = Some("fast".into());
            }

            let resolved = super::super::resolve_client_config_for_session(&state, &session).await;
            let base = state.client_config.read().await.clone();
            assert_eq!(
                Some(resolved.model.as_str()),
                expected_tier_model(&base, PhaseTier::Pro).as_deref(),
                "plan phase resolves through the plan tier"
            );
            assert_ne!(
                Some(resolved.model.as_str()),
                expected_tier_model(&base, PhaseTier::Fast).as_deref(),
                "the act tier must not leak into the plan phase"
            );
        }

        /// Precedence: an explicit R2-1 session override beats the phase
        /// tier, and an unresolvable tier keeps the global default.
        #[tokio::test]
        async fn resolve_client_config_session_override_beats_phase_tier() {
            let state = AppState::new();
            *state.client_config.write().await = base_config();
            {
                let mut store = state.provider_store.lock().await;
                *store = fixture_store();
            }
            let key = state.registry.create();
            let session = state.registry.get(key).unwrap();
            *session.model_override.lock().unwrap() =
                Some(crate::session_registry::SessionModelOverride {
                    provider: "anthropic".into(),
                    model: "claude-opus-4-7".into(),
                });
            {
                let mut cfg = state.desktop_config.write().await;
                cfg.approval_mode = Some("plan".into());
                cfg.plan_tier = Some("fast".into());
                cfg.act_tier = Some("fast".into());
            }

            let resolved = super::super::resolve_client_config_for_session(&state, &session).await;
            assert_eq!(
                resolved.model, "claude-opus-4-7",
                "session override wins over the phase tier"
            );
        }

        #[test]
        fn apply_tier_override_swaps_only_the_model() {
            let store = fixture_store();
            let base = base_config();
            let out = super::super::apply_tier_override(&base, store.config(), PhaseTier::Fast)
                .expect("anthropic resolves a fast tier");
            assert_eq!(out.provider, base.provider);
            assert_eq!(out.base_url, base.base_url, "endpoint untouched");
            assert_eq!(out.api_key, base.api_key, "credential untouched");
            assert_eq!(
                out.max_tokens, base.max_tokens,
                "behavioural fields untouched"
            );
            assert!(
                out.model.contains("haiku"),
                "fast tier → haiku family, got {}",
                out.model
            );
        }

        #[test]
        fn apply_tier_override_persisted_tier_table_wins_over_catalog() {
            // The profile's persisted `tiers` table (providers.toml v2, the
            // `/model --tier --save` write-back target) beats catalog
            // inference — same resolution the TUI performs.
            let store = fixture_store();
            let mut cfg = store.config().clone();
            {
                let profile = cfg.profiles.get_mut("default").unwrap();
                let entry = profile
                    .providers
                    .iter_mut()
                    .find(|p| p.id == profile.active_target.provider_id)
                    .unwrap();
                entry.tiers = ProviderTiers {
                    fast: Some("my-custom-fast".into()),
                    standard: None,
                    pro: None,
                };
            }
            let base = base_config();
            let out = super::super::apply_tier_override(&base, &cfg, PhaseTier::Fast)
                .expect("explicit tier override resolves");
            assert_eq!(out.model, "my-custom-fast");
        }

        #[test]
        fn apply_tier_override_returns_none_without_active_target() {
            // An empty store (no profiles at all) has no active target — the
            // tier contributes nothing and the caller keeps the base model.
            let store = ProviderConfigStore::from_config(ProviderModelConfig {
                version: ProviderModelConfig::VERSION,
                active_profile: String::new(),
                profiles: HashMap::new(),
                gateway: Default::default(),
            });
            let base = base_config();
            assert!(
                super::super::apply_tier_override(&base, store.config(), PhaseTier::Fast).is_none()
            );
        }

        // === R5-1: sidecar persistence (write-through) ===

        /// The full set → restart → clear story at the command layer: the
        /// same `persist_session_override` helper `set_session_model` /
        /// `clear_session_model` call, against stores redirected into a
        /// temp dir (the command bodies themselves need `tauri::State`).
        /// The store-level round-trip/prune/corrupt matrix lives in
        /// `session_override_store::tests`.
        #[tokio::test]
        async fn session_override_sidecar_write_through_survives_restart() {
            let dir = std::env::temp_dir().join(format!(
                "shannon-chat-override-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir");
            let sidecar_path = dir.join("overrides.json");

            // AppState with the sessions dir + sidecar redirected off the
            // real HOME (tests must never write the user's files).
            let mut state = AppState::new();
            state.state_manager = std::sync::Arc::new(
                shannon_engine::state::StateManager::with_sessions_dir(dir.join("sessions"))
                    .expect("temp sessions dir"),
            );
            state.session_overrides = std::sync::Mutex::new(
                crate::session_override_store::SessionOverrideSidecar::load_from(
                    sidecar_path.clone(),
                ),
            );

            let session_id = uuid::Uuid::new_v4();
            // Mirror `new_session`: the L0 log is the session-existence
            // marker the write-through prune checks.
            shannon_core::session_log::SessionTee::open_in_container(
                state.state_manager.sessions_dir(),
                &session_id.to_string(),
                "test-model",
                None,
            )
            .close();
            state.registry.insert(session_id);
            let session = state
                .registry
                .get(crate::session_registry::SessionKey(session_id))
                .expect("registered");

            // set_session_model's write-through (in-memory + disk).
            let ov = crate::session_registry::SessionModelOverride {
                provider: "openai".into(),
                model: "gpt-5".into(),
            };
            *session.model_override.lock().unwrap() = Some(ov.clone());
            super::super::persist_session_override(&state, session_id, Some(ov.clone()))
                .expect("write-through");

            // "Restart": a fresh registry hydrated from disk (the
            // `AppState::new` startup path) must see the override.
            let fresh_registry = crate::session_registry::SessionRegistry::new();
            let reloaded = crate::session_override_store::SessionOverrideSidecar::load_from(
                sidecar_path.clone(),
            );
            assert_eq!(reloaded.len(), 1, "one persisted entry");
            reloaded.apply_to_registry(&fresh_registry);
            assert_eq!(
                fresh_registry
                    .get(crate::session_registry::SessionKey(session_id))
                    .expect("hydrated")
                    .model_override_snapshot(),
                Some(ov),
                "the override survives the restart"
            );

            // clear_session_model's write-through empties the sidecar.
            super::super::persist_session_override(&state, session_id, None)
                .expect("write-through clear");
            assert!(
                crate::session_override_store::SessionOverrideSidecar::load_from(sidecar_path)
                    .is_empty(),
                "the cleared override must not resurrect after a restart"
            );

            std::fs::remove_dir_all(dir).ok();
        }

        // === R5-5: unattended-path precedence pin ===

        /// Goal / batch / routine (unattended) runs use the **global default
        /// only** — no session overrides, no phase tiers. Pins the three
        /// facts the precedence table in `resolve_client_config_for_session`
        /// documents: (a) override + phase prefs never write the global
        /// `client_config`, (b) the unattended run constructors read exactly
        /// that global Arc, and (c) only the interactive resolution path
        /// applies them. If a future unattended path starts routing through
        /// `resolve_client_config_for_session`, the table and this test must
        /// be revisited together.
        #[tokio::test]
        async fn unattended_paths_pin_global_config() {
            let state = AppState::new();
            let global_model_before = state.client_config.read().await.model.clone();
            let global_provider_before = state.client_config.read().await.provider.clone();

            // Every interactive-layer preference, set at once:
            let key = state.registry.create();
            *state
                .registry
                .get(key)
                .unwrap()
                .model_override
                .lock()
                .unwrap() = Some(crate::session_registry::SessionModelOverride {
                provider: "anthropic".into(),
                model: "claude-opus-4-7".into(),
            });
            {
                let mut cfg = state.desktop_config.write().await;
                cfg.approval_mode = Some("plan".into());
                cfg.plan_tier = Some("pro".into());
                cfg.act_tier = Some("fast".into());
            }

            // (a) the global target the unattended paths read is untouched…
            assert_eq!(
                state.client_config.read().await.model,
                global_model_before,
                "session overrides + phase prefs must never rewrite the global default"
            );
            assert_eq!(
                state.client_config.read().await.provider,
                global_provider_before
            );

            // (b) …and the goal runner's config source IS that global Arc
            // (`GoalRunDeps::from_state` clones `state.client_config`, same
            // as BatchRunDeps / RoutineRunDeps) — its LlmClient therefore
            // sees the global default, override and tier invisible.
            let deps = crate::goal_commands::GoalRunDeps::from_state(&state);
            let run_config = deps.client_config.read().await;
            assert_eq!(
                run_config.model, global_model_before,
                "unattended goal runs build their client from the global default only"
            );
            assert_eq!(run_config.provider, global_provider_before);
            drop(run_config);

            // (c) contrast: the INTERACTIVE resolution path does apply the
            // prefs — override > tier — proving the pin is about the path,
            // not missing preferences.
            *state.client_config.write().await = base_config();
            {
                let mut store = state.provider_store.lock().await;
                *store = fixture_store();
            }
            let resolved = super::super::resolve_client_config_for_session(
                &state,
                state.registry.get(key).unwrap().as_ref(),
            )
            .await;
            assert_eq!(
                resolved.model, "claude-opus-4-7",
                "the session override applies on the interactive path"
            );
        }
    }

    // === P1-1 fix: cancel_query explicit per-window routing ===
    //
    // `cancel_session_query` is the command body, split out so the routing
    // behavior is testable with a plain AppState (no Wry handle needed).

    use crate::session_registry::SessionKey;
    use tokio_util::sync::CancellationToken;

    async fn seed_token(state: &AppState, key: SessionKey) -> CancellationToken {
        let token = CancellationToken::new();
        let session = state.registry.get(key).expect("seeded session");
        *session.cancellation_token.lock().await = Some(token.clone());
        token
    }

    /// Explicit sessionId cancels THAT session's query — and never the
    /// active one (the regression: a session window's cancel used to hit
    /// whichever session the shared pointer named).
    #[tokio::test]
    async fn cancel_query_explicit_session_cancels_only_that_session() {
        let state = AppState::new();
        let a = state.registry.create();
        let b = state.registry.create();
        state.registry.set_active(a); // active pointer = A

        let token_a = seed_token(&state, a).await;
        let token_b = seed_token(&state, b).await;

        super::cancel_session_query(&state, Some(&b.0.to_string()))
            .await
            .expect("cancel succeeds");

        assert!(token_b.is_cancelled(), "B's query is cancelled");
        assert!(!token_a.is_cancelled(), "A's query must be untouched");
        assert_eq!(
            state.registry.active_key(),
            Some(a),
            "cancel must not move the active pointer"
        );
        assert!(
            !*state.registry.get(a).unwrap().querying.try_lock().unwrap(),
            "A's querying flag must stay as-is"
        );
    }

    #[tokio::test]
    async fn cancel_query_unknown_session_errors_hard() {
        let state = AppState::new();
        let active = state.registry.get_or_create_active();
        let token = seed_token(&state, SessionKey(active.session_id)).await;

        let err = super::cancel_session_query(&state, Some(&uuid::Uuid::new_v4().to_string()))
            .await
            .expect_err("unknown session must be a hard error");
        assert!(err.contains("unknown session"), "{err}");
        assert!(!token.is_cancelled(), "nothing was cancelled");
    }

    /// No parameter → legacy behavior: cancels the active session's query.
    #[tokio::test]
    async fn cancel_query_none_falls_back_to_active() {
        let state = AppState::new();
        let active = state.registry.get_or_create_active();
        let token = seed_token(&state, SessionKey(active.session_id)).await;
        // A send is in flight (send_message's guard+set latched the session).
        {
            let mut q = active.querying.lock().await;
            *q = true;
        }

        super::cancel_session_query(&state, None)
            .await
            .expect("legacy cancel succeeds");

        assert!(token.is_cancelled(), "active session's query is cancelled");
        assert!(
            *state
                .registry
                .get(SessionKey(active.session_id))
                .unwrap()
                .querying
                .try_lock()
                .unwrap(),
            "querying flag stays held — only the loop exit resets it (A-17)"
        );
    }

    // === A-17 fix (R4 group 6): the latch survives cancel until the loop exit ===
    //
    // The old code cleared `session.querying` inside the cancel command, so a
    // send issued immediately after Stop passed the concurrent-query guard
    // while the old loop was still draining toward its next engine event —
    // two live queries on one session, and the old loop's late events
    // polluted the new turn. These tests replay that sequence at the same
    // level the command bodies run (plain AppState, no Wry handle).

    /// cancel must keep the latch latched: token fired, querying still true.
    /// (Reverts to a failure if the early clear ever comes back.)
    #[tokio::test]
    async fn cancel_keeps_the_querying_latch_latched() {
        let state = AppState::new();
        let key = state.registry.create();
        let token = seed_token(&state, key).await;
        let session = state.registry.get(key).expect("seeded session");
        // send_message's guard+set (commands.rs check-and-set) is what put
        // the latch up before the user pressed Stop.
        {
            let mut q = session.querying.lock().await;
            *q = true;
        }

        super::cancel_session_query(&state, Some(&key.0.to_string()))
            .await
            .expect("cancel succeeds");

        assert!(token.is_cancelled(), "the token must still fire");
        assert!(
            *state
                .registry
                .get(key)
                .unwrap()
                .querying
                .try_lock()
                .unwrap(),
            "the querying latch must STAY HELD across cancel — the old loop is \
             still draining; resetting here reopens the A-17 pollution window"
        );
    }

    /// The full A-17 sequence: send latched → cancel (resend rejected) →
    /// the query loop's exit-path reset (the same two statements as
    /// commands.rs, run on every exit incl. caught panic) → resend accepted.
    #[tokio::test]
    async fn resend_is_rejected_until_the_loop_exit_resets_the_latch() {
        let state = AppState::new();
        let key = state.registry.create();
        let session = state.registry.get(key).unwrap();
        let token = seed_token(&state, key).await;
        {
            let mut q = session.querying.lock().await;
            *q = true;
        }

        super::cancel_session_query(&state, Some(&key.0.to_string()))
            .await
            .expect("cancel succeeds");

        // A resend in the cancel→loop-exit window hits the guard and is
        // rejected (the check half of send_message's check-and-set).
        {
            let q = session.querying.lock().await;
            assert!(token.is_cancelled(), "the cancel still fired the token");
            assert!(*q, "the concurrent-query guard must reject the resend");
        }

        // The loop exit path (commands.rs bottom — ok / error / cancel /
        // panic all funnel here) resets latch + token.
        {
            let mut q = session.querying.lock().await;
            *q = false;
        }
        {
            let mut t = session.cancellation_token.lock().await;
            *t = None;
        }

        // Now the resend's check-and-set succeeds — the session is usable
        // again exactly when the old loop is gone.
        {
            let mut q = session.querying.lock().await;
            assert!(!*q, "the loop exit reopened the latch");
            *q = true;
        }
    }

    // === B1-4 (P1-3): the cancel window (latch up, token not yet stored) ===
    //
    // send_message latches the session and only afterwards stores the
    // cancellation token. A stop landing between the two used to take the
    // None token and silently "succeed" — nothing fired, no
    // `query:cancelled` would ever be emitted, and the frontend's stop
    // button waited forever. These tests replay the sequence at the same
    // level the command bodies run (plain AppState, no Wry handle): the
    // windowed stop records a pending marker; send_message's post-latch
    // block (store token → consume marker → cancel) kills the fresh run
    // immediately; the loop exit clears any spurious marker with the latch.

    /// The full windowed-stop sequence: cancel with the latch up but no
    /// token stored records the intent; storing the token and consuming
    /// the marker (the exact statements of send_message's post-latch
    /// block) leaves the fresh token cancelled, so the spawned loop emits
    /// `query:cancelled` on its first stream step.
    #[tokio::test]
    async fn cancel_in_the_send_window_sets_pending_and_the_stored_token_fires() {
        let state = AppState::new();
        let key = state.registry.create();
        let session = state.registry.get(key).unwrap();

        // send_message's check-and-set has latched the session; the token
        // store has NOT happened yet.
        {
            let mut q = session.querying.lock().await;
            *q = true;
        }

        super::cancel_session_query(&state, Some(&key.0.to_string()))
            .await
            .expect("cancel succeeds");

        // send_message's post-latch block: store the token, then consume
        // the marker and cancel the fresh run with it.
        let token = CancellationToken::new();
        *session.cancellation_token.lock().await = Some(token.clone());
        assert!(
            session.take_cancel_pending(),
            "the windowed stop must be recorded as pending, not silently dropped"
        );
        token.cancel();

        assert!(
            token.is_cancelled(),
            "the fresh run must start pre-cancelled — the windowed stop's \
             `query:cancelled` comes from its own first stream step"
        );
        assert!(
            !session.take_cancel_pending(),
            "the marker is consumed exactly once"
        );
    }

    /// The windowed-stop marker must not poison the NEXT run: a double
    /// stop records a spurious pending (the first stop already fired the
    /// token, the second found None with the latch still up), and the
    /// query loop's exit path clears the flag together with the latch.
    #[tokio::test]
    async fn cancel_window_flag_does_not_outlive_its_querying_epoch() {
        let state = AppState::new();
        let key = state.registry.create();
        let session = state.registry.get(key).unwrap();
        let token = seed_token(&state, key).await;
        {
            let mut q = session.querying.lock().await;
            *q = true;
        }

        // First stop: normal path — token taken and fired.
        super::cancel_session_query(&state, Some(&key.0.to_string()))
            .await
            .expect("first stop fires the token");
        assert!(token.is_cancelled());

        // Second stop in the same epoch: token already taken → spurious
        // pending marker (the latch is still up).
        super::cancel_session_query(&state, Some(&key.0.to_string()))
            .await
            .expect("second stop succeeds");
        assert!(
            session.take_cancel_pending(),
            "the double stop records a spurious marker"
        );

        // The loop exit path (commands.rs bottom) resets latch + token and
        // clears the marker — replay of its exact statements.
        {
            let mut q = session.querying.lock().await;
            *q = false;
        }
        {
            let mut t = session.cancellation_token.lock().await;
            *t = None;
        }
        session.clear_cancel_pending();

        // The NEXT send's consume finds nothing — no inherited stop.
        assert!(
            !session.take_cancel_pending(),
            "the marker must not outlive its querying epoch"
        );
    }

    /// Cancel on an idle session (no latch, no token — e.g. a stop racing
    /// a settle): the historical no-op must NOT record a pending marker,
    /// or the next legitimate send would start pre-cancelled.
    #[tokio::test]
    async fn cancel_of_an_idle_session_records_no_pending() {
        let state = AppState::new();
        let key = state.registry.create();
        let session = state.registry.get(key).unwrap();
        assert!(!*session.querying.lock().await, "fixture: idle session");

        super::cancel_session_query(&state, Some(&key.0.to_string()))
            .await
            .expect("cancel succeeds");

        assert!(
            !session.take_cancel_pending(),
            "an idle cancel must not arm the window marker"
        );
    }
}
