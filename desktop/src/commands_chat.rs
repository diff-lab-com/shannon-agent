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
            ModelInfo {
                id: m.id.to_string(),
                name: m.display_name.to_string(),
                provider: provider_str.to_string(),
                context_window: m.context_window,
                price_in: pricing.as_ref().map(|p| p.input_price_per_mtok),
                price_out: pricing.as_ref().map(|p| p.output_price_per_mtok),
                tier: None,
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

/// Body of [`cancel_query`], split out so the routing behavior is testable
/// without a Wry app handle.
async fn cancel_session_query(state: &AppState, session_id: Option<&str>) -> Result<(), String> {
    let (_, session) = state.registry.resolve_explicit_or_active(session_id)?;

    // Take the cancellation token and cancel it
    let token_opt = {
        let mut token_guard = session.cancellation_token.lock().await;
        token_guard.take()
    };

    if let Some(token) = token_opt {
        token.cancel();
    }

    // Clear querying flag
    {
        let mut querying = session.querying.lock().await;
        *querying = false;
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
/// set, the global `client_config` otherwise.
///
/// Resolution failure (the overridden provider was deleted from the engine
/// store since the override was written) degrades to the global config with
/// a warning — a stale override must never block a send.
pub(crate) async fn resolve_client_config_for_session(
    state: &AppState,
    session: &crate::session_registry::SessionState,
) -> shannon_engine::api::LlmClientConfig {
    let base = state.client_config.read().await.clone();
    let Some(ov) = session.model_override_snapshot() else {
        return base;
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

/// Pure body of [`resolve_client_config_for_session`]: apply a
/// `(provider_slug, model_id)` override on top of the global client config,
/// resolving provider identity, base URL and credential from the engine
/// store's `"default"` profile roster (the same source
/// `AppState::build_client_config` uses for the global target — swapping
/// only provider/model on the global config would keep the OLD provider's
/// base_url and API key, which fails for any cross-provider override).
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
        .profiles
        .get("default")?
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
        let kind_ok = config.profiles.get("default").is_some_and(|pf| {
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

    *session
        .model_override
        .lock()
        .map_err(|_| "session model override lock poisoned".to_string())? =
        Some(crate::session_registry::SessionModelOverride {
            provider: provider_clean,
            model: model_id,
        });
    Ok(())
}

/// Clear the session-level model override — the session goes back to
/// inheriting the global default (including future default changes).
/// Idempotent: clearing a session without an override is a no-op.
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

        /// Without an override the resolution is the identity — the global
        /// config passes through untouched (new chats inherit the default).
        #[tokio::test]
        async fn resolve_client_config_without_override_is_identity() {
            let state = AppState::new();
            let key = state.registry.create();
            let session = state.registry.get(key).unwrap();
            assert!(session.model_override_snapshot().is_none());

            let resolved = super::super::resolve_client_config_for_session(&state, &session).await;
            let base = state.client_config.read().await.clone();
            assert_eq!(resolved.model, base.model);
            assert_eq!(resolved.provider, base.provider);
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

        super::cancel_session_query(&state, None)
            .await
            .expect("legacy cancel succeeds");

        assert!(token.is_cancelled(), "active session's query is cancelled");
        assert!(
            !*state
                .registry
                .get(SessionKey(active.session_id))
                .unwrap()
                .querying
                .try_lock()
                .unwrap(),
            "querying flag is cleared"
        );
    }
}
