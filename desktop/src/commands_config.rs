//! Configuration commands — configure, switch_provider, get_config.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::commands::AppState;
use crate::config::{self, DesktopConfig, ProviderConnection, ProvidersFile};
use crate::events;
use crate::events::event_names;
use shannon_core::provider_config_service::ProviderConfigService;
use shannon_types::provider_config::ProviderTiers;

/// Resolve a desktop kind slug to the engine's canonical default base
/// URL for that kind. Returns `None` for kinds that have no canonical
/// URL (today: `openai-compatible`, which is the catch-all the user
/// must supply a custom URL for, and any unknown slug). The caller
/// falls back to an empty string when this is `None`, which the
/// engine's `resolve_provider` will reject — a user error, not a
/// default.
fn default_base_url_for_kind(kind: &str) -> Option<&'static str> {
    if kind == "openai-compatible" {
        return None;
    }
    let provider = llm_provider_for_active_mirror(kind)?;
    Some(provider.default_base_url())
}

/// Build a v2 `ProviderProfile` from a `ProviderConnection` using the
/// engine's canonical default base URL for the connection's kind. The
/// user-supplied `base_url` always wins; the default is the fallback
/// for connections that don't set one (e.g. a brand-new Anthropic
/// connection — engine should still be able to talk to
/// `https://api.anthropic.com`).
fn connection_to_profile(
    conn: &ProviderConnection,
) -> shannon_types::provider_config::ProviderProfile {
    let default = default_base_url_for_kind(&conn.provider_kind).unwrap_or("");
    conn.to_provider_profile(default)
}

/// Land a managed connection in the engine's
/// `~/.shannon/providers.toml` via
/// `ProviderConfigService::upsert`. The desktop's `providers.json`
/// cache is the read-side fan-out for the UI; the engine store is the
/// source of truth for the runtime path.
///
/// `upsert(make_active = true)` pins
/// `active_target.{provider_id, model_id}` on the `"default"` model
/// profile as a side effect, so this single helper covers both the
/// `save_provider` and `set_active_provider` desktop flows — there is
/// no separate "activate" call against the engine store.
///
/// All writes route through [`ProviderConfigService`] (ADR-0008 P2-5
/// Decision 3 single write path). Lock ordering: the process-internal
/// `AppState::provider_store` mutex is acquired first, then the
/// cross-process `flock` via [`ProviderConfigService::lock`] (the
/// contract documented on that method — reverse order deadlocks). The
/// store is moved out of the guard for the R-M-W round-trip and
/// restored on completion so concurrent desktop commands can't clobber
/// each other or a concurrent `shannon providers add` CLI invocation.
async fn land_profile_in_engine_store(
    state: &tauri::State<'_, AppState>,
    conn: &ProviderConnection,
    model_id: &str,
) -> Result<(), String> {
    let profile = connection_to_profile(conn);
    let mut guard = state.provider_store.lock().await;
    let store = std::mem::take(&mut *guard);
    let mut svc = ProviderConfigService::from_store(store);
    {
        let mut locked = svc
            .lock()
            .map_err(|e| format!("could not lock providers.toml: {e}"))?;
        locked
            .upsert(profile, model_id, true)
            .map_err(|e| format!("could not persist to providers.toml: {e}"))?;
    }
    *guard = svc.into_inner();
    Ok(())
}

/// Look up the active provider's id and kind in the engine
/// `ProviderConfigStore`. Returns `None` when no active target is set
/// (an empty `provider_id` in `active_target`). Used by the
/// `configure('model' | 'api_key' | 'base_url' | 'provider')` arms to
/// route write paths through the store without touching the legacy
/// `DesktopConfig.{provider,api_key,base_url,model}` mirror fields
/// (P1.2-A — the A1 fix).
fn active_provider_id_and_kind(
    store: &shannon_core::provider_config_store::ProviderConfigStore,
) -> Option<(String, String)> {
    let cfg = store.config();
    let pf = cfg.profiles.get("default")?;
    let id = pf.active_target.provider_id.clone();
    if id.is_empty() {
        return None;
    }
    let profile = pf.providers.iter().find(|p| p.id == id)?;
    Some((id, provider_kind_slug(&profile.kind).to_string()))
}

/// Find the first managed provider whose `kind` matches `kind_str`.
/// Returns `(provider_id, model_id)` of the matching slot; the model
/// id is the current `active_target.model_id` (preserved across the
/// switch so a `/model X` followed by `configure('provider', Y)`
/// keeps the chosen model in the new provider's default slot).
fn find_provider_by_kind(
    store: &shannon_core::provider_config_store::ProviderConfigStore,
    kind_str: &str,
) -> Option<(String, String)> {
    let cfg = store.config();
    let pf = cfg.profiles.get("default")?;
    let profile = pf
        .providers
        .iter()
        .find(|p| provider_kind_slug(&p.kind) == kind_str)?;
    Some((profile.id.clone(), pf.active_target.model_id.clone()))
}

/// Map a [`shannon_types::provider_config::ProviderKind`] to the
/// kebab-case slug the desktop UI speaks (matches
/// `LlmProvider::default_base_url` + the `llm_provider_for_active_mirror`
/// mirror in the reverse direction). The wire format is also kebab-case
/// per `#[serde(rename_all = "kebab-case")]` on the enum, so this stays
/// the one canonical mapping the desktop needs.
fn provider_kind_slug(k: &shannon_types::provider_config::ProviderKind) -> &'static str {
    use shannon_types::provider_config::ProviderKind as K;
    match k {
        K::Anthropic => "anthropic",
        K::OpenAi => "openai",
        K::OpenAiCompatible => "openai-compatible",
        K::Ollama => "ollama",
        K::Gemini => "gemini",
        K::Deepseek => "deepseek",
        // Defensive default for `#[non_exhaustive]`; the engine
        // adds new kinds before the desktop picks them up, so an
        // unrecognised kind lands as "unsupported" rather than a panic.
        _ => "unsupported",
    }
}

/// Rebuild `state.client_config` from the engine
/// `ProviderConfigStore`, layering the desktop `ShannonConfig`
/// overrides (temperature, max_tokens) on top. Called by every
/// `configure` arm that changes the store so the live query path
/// (`send_message` / `cancel_query`) sees the new target without
/// waiting for a restart.
///
/// `build_client_config` is `pub(crate)` and pure on the store +
/// overrides, so this just locks, computes, and writes
/// `state.client_config` in place. Drops both locks before
/// returning.
async fn rebuild_client_config_from_store(
    state: &tauri::State<'_, AppState>,
) -> Result<(), String> {
    let overrides = {
        let dc = state.desktop_config.read().await;
        shannon_core::unified_config::ShannonConfig {
            max_tokens: dc.max_tokens.map(|v| v as usize),
            temperature: dc.temperature,
            ..Default::default()
        }
    };
    let new_cc = {
        let store = state.provider_store.lock().await;
        AppState::build_client_config(&store, &overrides).unwrap_or_default()
    };
    let mut cc = state.client_config.write().await;
    *cc = new_cc;
    Ok(())
}

/// Remove a managed connection from the engine's `providers.toml`
/// via `ProviderConfigService::disconnect_by_slug`. If the removed
/// slot was the active target, the engine clears
/// `active_target.{provider_id, model_id}` to empty — the resolver
/// falls back to synthesis on the next request. Idempotent: removing
/// an unknown id writes nothing.
///
/// All writes route through [`ProviderConfigService`] (ADR-0008 P2-5
/// Decision 3 single write path). Same mutex-then-flock ordering as
/// [`land_profile_in_engine_store`].
async fn remove_profile_from_engine_store(
    state: &tauri::State<'_, AppState>,
    profile_id: &str,
) -> Result<(), String> {
    let mut guard = state.provider_store.lock().await;
    let store = std::mem::take(&mut *guard);
    let mut svc = ProviderConfigService::from_store(store);
    {
        let mut locked = svc
            .lock()
            .map_err(|e| format!("could not lock providers.toml: {e}"))?;
        locked
            .disconnect_by_slug(profile_id)
            .map_err(|e| format!("could not persist to providers.toml: {e}"))?;
    }
    *guard = svc.into_inner();
    Ok(())
}

/// Configuration update payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigUpdate {
    pub key: String,
    pub value: String,
}

/// Provider switch request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSwitchRequest {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: String,
}

/// Update a single desktop config key. The frontend uses this for every
/// settings panel mutation — model, api_key, theme, toggles, etc. Persists
/// the new config to `~/.shannon/desktop/config.json` and emits
/// `CONFIG_UPDATED` so other windows / the tray can react.
#[tauri::command]
pub async fn configure(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    update: ConfigUpdate,
) -> Result<(), String> {
    match update.key.as_str() {
        "model" => {
            // P1.2-B: the engine `ProviderConfigStore` is the source of
            // truth for the active model. Configure writes only touch
            // the store; the live `state.client_config` is rebuilt from
            // it by `rebuild_client_config_from_store` so any reader
            // (`send_message`, `get_status`, etc.) sees the new model
            // on the next read.
            let new_model_id = update.value.clone();
            let kind_str = {
                let mut guard = state.provider_store.lock().await;
                let (_, kind_str) = active_provider_id_and_kind(&guard).ok_or_else(|| {
                    "configure('model'): no active provider — add one in Settings → Models first"
                        .to_string()
                })?;
                let provider = llm_provider_for_active_mirror(&kind_str).ok_or_else(|| {
                    format!("configure('model'): unsupported active kind `{kind_str}`")
                })?;
                // Route through ProviderConfigService (ADR-0008 P2-5
                // Decision 3 single write path). This also closes a
                // latent gap: the old bare `store.save()` held the
                // in-process mutex but no cross-process flock, so a
                // concurrent CLI write could tear the file. `svc.lock()`
                // acquires the flock; mutex-then-flock ordering is the
                // contract documented on `ProviderConfigService::lock`.
                let store = std::mem::take(&mut *guard);
                let mut svc = ProviderConfigService::from_store(store);
                {
                    let mut locked = svc
                        .lock()
                        .map_err(|e| format!("could not lock providers.toml: {e}"))?;
                    locked
                        .set_active(&provider, &new_model_id)
                        .map_err(|e| format!("could not persist providers.toml: {e}"))?;
                }
                *guard = svc.into_inner();
                kind_str
            };
            rebuild_client_config_from_store(&state).await?;
            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "model".into(),
                    value: new_model_id,
                },
            );
            let _ = kind_str;
            Ok(())
        }
        "api_key" => {
            // P1.2-A — the A1 fix: write the API key to
            // `~/.shannon/credentials/<provider_id>.json` via
            // `CredentialManager`, NOT to `desktop_cfg.api_key` /
            // `cfg.api_key` (plaintext in `config.json`). The
            // engine resolver reads the credential file at request
            // time so no in-memory cache update is needed.
            let new_key = update.value.clone();
            let (active_id, label) = {
                let store = state.provider_store.lock().await;
                let pf = store.config().profiles.get("default").ok_or_else(|| {
                    "configure('api_key'): no providers configured — add one first".to_string()
                })?;
                let active_id = pf.active_target.provider_id.clone();
                if active_id.is_empty() {
                    return Err(
                        "configure('api_key'): no active provider — add one in Settings → Models first"
                            .to_string(),
                    );
                }
                let profile = pf
                    .providers
                    .iter()
                    .find(|p| p.id == active_id)
                    .ok_or_else(|| {
                        format!("configure('api_key'): active provider `{active_id}` not in store")
                    })?;
                (active_id, profile.display_name.clone())
            };
            store_provider_key(&active_id, &label, Some(&new_key))?;
            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "api_key".into(),
                    value: "***".into(),
                },
            );
            Ok(())
        }
        "base_url" => {
            // P1.2-A: route through the engine store via
            // `upsert_profile`. Preserves every other field of the
            // active profile (kind, credential, tiers,
            // default_max_tokens, extra_headers, quirks).
            let new_url = validate_base_url(&update.value)?;
            let (active_id, model_id, mut profile) = {
                let store = state.provider_store.lock().await;
                let pf =
                    store.config().profiles.get("default").ok_or_else(|| {
                        "configure('base_url'): no providers configured".to_string()
                    })?;
                let active_id = pf.active_target.provider_id.clone();
                if active_id.is_empty() {
                    return Err(
                        "configure('base_url'): no active provider — add one first".to_string()
                    );
                }
                let model_id = pf.active_target.model_id.clone();
                let profile = pf
                    .providers
                    .iter()
                    .find(|p| p.id == active_id)
                    .ok_or_else(|| {
                        format!("configure('base_url'): active provider `{active_id}` not in store")
                    })?
                    .clone();
                (active_id, model_id, profile)
            };
            profile.base_url = new_url.clone();
            {
                let mut guard = state.provider_store.lock().await;
                let store = std::mem::take(&mut *guard);
                let mut svc = ProviderConfigService::from_store(store);
                {
                    let mut locked = svc
                        .lock()
                        .map_err(|e| format!("could not lock providers.toml: {e}"))?;
                    // make_active = true matches the prior
                    // `upsert_profile` side effect of pinning the
                    // active target; the profile being rewritten is
                    // already the active one anyway.
                    locked
                        .upsert(profile, &model_id, true)
                        .map_err(|e| format!("could not persist providers.toml: {e}"))?;
                }
                *guard = svc.into_inner();
            }
            rebuild_client_config_from_store(&state).await?;
            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "base_url".into(),
                    value: new_url,
                },
            );
            let _ = active_id;
            Ok(())
        }
        "provider" => {
            // P1.2-A: the legacy write-path bug — `configure('provider')`
            // used to update only `state.provider` / `desktop_cfg.provider`
            // and never touched the engine store, so the runtime client
            // kept using the old target until restart. Now routes through
            // `set_active` on the engine store and rebuilds the live
            // client_config.
            let kind_str = update.value.clone();
            let (provider_id, model_id) = {
                let store = state.provider_store.lock().await;
                find_provider_by_kind(&store, &kind_str).ok_or_else(|| {
                    format!(
                        "configure('provider'): no managed provider with kind `{kind_str}` — \
                         add one in Settings → Models first"
                    )
                })?
            };
            let provider = llm_provider_for_active_mirror(&kind_str)
                .ok_or_else(|| format!("configure('provider'): unsupported kind `{kind_str}`"))?;
            {
                let mut guard = state.provider_store.lock().await;
                let store = std::mem::take(&mut *guard);
                let mut svc = ProviderConfigService::from_store(store);
                {
                    let mut locked = svc
                        .lock()
                        .map_err(|e| format!("could not lock providers.toml: {e}"))?;
                    locked
                        .set_active(&provider, &model_id)
                        .map_err(|e| format!("could not persist providers.toml: {e}"))?;
                }
                *guard = svc.into_inner();
            }
            rebuild_client_config_from_store(&state).await?;
            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "provider".into(),
                    value: kind_str,
                },
            );
            let _ = provider_id;
            Ok(())
        }
        "working_dir" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.working_dir = Some(update.value.clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "working_dir".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "theme" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.theme = Some(update.value.clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "theme".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "approval_mode" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.approval_mode = Some(update.value.clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "approval_mode".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "strategic_focus" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.strategic_focus = Some(update.value.clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "strategic_focus".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "performance_strategy" => {
            let strategy = update.value.clone();
            if !matches!(strategy.as_str(), "speed" | "balanced" | "high-quality") {
                return Err(format!("Invalid performance_strategy: {strategy}"));
            }
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.performance_strategy = Some(strategy.clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "performance_strategy".into(),
                    value: strategy,
                },
            );

            Ok(())
        }
        "memory_enabled" | "telemetry" | "encryption" | "debug_console" => {
            let enabled = match update.value.to_ascii_lowercase().as_str() {
                "true" => true,
                "false" => false,
                _ => {
                    return Err(format!(
                        "Invalid boolean for {}: {}",
                        update.key, update.value
                    ));
                }
            };
            let mut desktop_cfg = state.desktop_config.write().await;
            match update.key.as_str() {
                "memory_enabled" => desktop_cfg.memory_enabled = Some(enabled),
                "telemetry" => desktop_cfg.telemetry_enabled = Some(enabled),
                "encryption" => desktop_cfg.encryption_enabled = Some(enabled),
                "debug_console" => desktop_cfg.debug_console = Some(enabled),
                other => {
                    return Err(format!("Unrecognized boolean key: {other}"));
                }
            }

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: update.key.clone(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "temperature" => {
            let parsed: f32 = update
                .value
                .parse()
                .map_err(|e| format!("Invalid temperature: {e}"))?;
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.temperature = Some(parsed);

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "temperature".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "max_tokens" => {
            let parsed: u32 = update
                .value
                .parse()
                .map_err(|e| format!("Invalid max_tokens: {e}"))?;
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.max_tokens = Some(parsed);

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "max_tokens".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "plan" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.plan = Some(update.value.clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "plan".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "clear_cache" => {
            // P0-4: clear the active session's message buffer instead of
            // the (removed) `state.messages` field.
            let session = state.registry.get_or_create_active();
            let mut messages = session.messages.lock().await;
            messages.clear();
            Ok(())
        }
        "factory_reset" => {
            let default_cfg = DesktopConfig::default();
            let mut desktop_cfg = state.desktop_config.write().await;
            *desktop_cfg = default_cfg.clone();
            drop(desktop_cfg);
            config::save_config(&default_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "factory_reset".into(),
                    value: "true".into(),
                },
            );

            Ok(())
        }
        "enabled_providers" => {
            // Wire shape: JSON array of slugs as a string. Three
            // accepted payloads (ADR-0005 P4.9):
            //   - "[]"             → Some(vec![]) (user toggled all off)
            //   - '["a","b"]'      → Some(vec!["a","b"])
            //   - "null" / ""      → None (reset to engine env-var behaviour)
            let parsed: Option<Vec<String>> = match update.value.trim() {
                "" | "null" => None,
                raw => Some(
                    serde_json::from_str::<Vec<String>>(raw)
                        .map_err(|e| format!("invalid enabled_providers `{raw}`: {e}"))?,
                ),
            };

            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.enabled_providers = parsed.clone();

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "enabled_providers".into(),
                    value: serde_json::to_string(&parsed).unwrap_or_else(|_| "null".into()),
                },
            );

            Ok(())
        }
        "cancel_subscription" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.plan = None;
            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            Ok(())
        }
        _ => Err(format!("Unknown config key: {}", update.key)),
    }
}

/// Switch to a different LLM provider.
///
/// P1.2-B (ADR-0005): with the singular `DesktopConfig.provider` /
/// `api_key` / `base_url` / `model` fields removed, this command is a
/// thin shim that simply rebuilds the live client config from the
/// engine `ProviderConfigStore` (which has already been updated by the
/// caller via [`save_provider`] / [`set_active_provider`]) and emits
/// `CONFIG_UPDATED` so the tray refreshes its label.
///
/// Pre-P1.2 callers wrote `state.model` / `state.provider` mutexes and
/// mirrored the new fields into `DesktopConfig`. Those targets are gone,
/// so the function now does almost nothing on its own — it exists
/// primarily so the frontend `switchProvider` invoke keeps its existing
/// wire contract.
#[tauri::command]
pub async fn switch_provider(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    request: ProviderSwitchRequest,
) -> Result<(), String> {
    let _ = request;

    let desktop_cfg = state.desktop_config.read().await.clone();
    let shannon_overrides = shannon_core::unified_config::ShannonConfig {
        max_tokens: desktop_cfg.max_tokens.map(|v| v as usize),
        temperature: desktop_cfg.temperature,
        ..Default::default()
    };
    let new_client_config = {
        let store_guard = state.provider_store.lock().await;
        AppState::build_client_config(&store_guard, &shannon_overrides).unwrap_or_default()
    };

    let new_provider_label = new_client_config.provider.to_string();
    {
        let mut c = state.client_config.write().await;
        *c = new_client_config;
    }

    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "provider".into(),
            value: new_provider_label,
        },
    );

    Ok(())
}

/// Get the current desktop config (for settings panel).
///
/// P1.2-B (ADR-0005): the top-level `api_key` masking branch is gone —
/// `DesktopConfig` no longer carries the singular `api_key` field (the
/// engine `ProviderConfigStore` + `CredentialManager` own it now). The
/// `stt.api_key` masking stays since the STT sub-config still persists
/// its own key on disk for now.
#[tauri::command]
pub async fn get_config(state: tauri::State<'_, AppState>) -> Result<DesktopConfig, String> {
    let cfg = state.desktop_config.read().await;
    let mut display = cfg.clone();
    if let Some(stt) = display.stt.as_mut() {
        if stt.api_key.is_some() {
            stt.api_key = Some("***".into());
        }
    }
    Ok(display)
}

/// Result of scanning the process environment for a pre-configured provider.
///
/// The Welcome wizard uses this on mount to pre-select a provider + skip the
/// API key entry step when the user already has `ANTHROPIC_API_KEY` etc. set
/// in their shell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedProvider {
    pub provider: String,
    pub has_api_key: bool,
}

/// Scan env vars for a known provider API key. First match wins — the order
/// mirrors the Welcome wizard's recommended-provider ranking.
///
/// Returns `None` if no provider env var is set. Ollama is handled separately
/// (no API key; detected via `OLLAMA_HOST` or default `localhost:11434`).
#[tauri::command]
pub fn detect_provider_from_env() -> Option<DetectedProvider> {
    let candidates: &[(&str, &str)] = &[
        ("ANTHROPIC_API_KEY", "anthropic"),
        ("OPENAI_API_KEY", "openai"),
        ("DEEPSEEK_API_KEY", "deepseek"),
    ];
    for (env_var, provider) in candidates {
        if let Ok(val) = std::env::var(env_var) {
            if !val.trim().is_empty() {
                return Some(DetectedProvider {
                    provider: (*provider).into(),
                    has_api_key: true,
                });
            }
        }
    }
    if std::env::var("OLLAMA_HOST").is_ok() {
        return Some(DetectedProvider {
            provider: "ollama".into(),
            has_api_key: false,
        });
    }
    None
}

/// Categorized connection test result for the Welcome "Test connection" button.
///
/// The frontend maps each variant to a specific toast message so the user
/// knows whether their key is invalid, the network is down, or the provider
/// is having an outage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TestConnectionResult {
    Success,
    InvalidKey,
    RateLimited,
    ProviderError { status: u16 },
    NetworkUnreachable,
    Unknown { message: String },
}

/// Validate + normalize a user-supplied provider base_url.
///
/// Defense-in-depth for the custom-endpoint flow: rejects non-HTTP(S) schemes
/// (e.g. `file:`, `data:`), URLs with embedded credentials, missing hosts, and
/// unparseable input; drops any fragment. Returns the normalized base with no
/// trailing slash.
///
/// It deliberately does **not** block private/loopback hosts: pointing the app
/// at `http://localhost:11434` (Ollama) or a self-hosted model on a private
/// network is a first-class, intended use case. The URL is supplied by the
/// local user themselves (the Add Provider modal) — there is no
/// untrusted/remote input vector reaching this path — so the SSRF scenario of
/// an attacker steering server-side fetches does not apply here.
pub(crate) fn validate_base_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let parsed = url::Url::parse(raw).map_err(|e| format!("invalid base_url `{raw}`: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "base_url must use http or https, got `{}`",
            parsed.scheme()
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("base_url must not contain embedded credentials".into());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "base_url must have a host".to_string())?;
    if host.is_empty() {
        return Err("base_url must have a host".into());
    }
    let mut cleaned = parsed;
    cleaned.set_fragment(None);
    let mut out = cleaned.to_string();
    while out.ends_with('/') {
        out.pop();
    }
    Ok(out)
}

/// Trim an optional base_url from frontend input, returning `None` for
/// empty/blank values and validating non-empty ones.
fn resolve_base_url(raw: &Option<String>) -> Result<Option<String>, String> {
    match raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(b) => Ok(Some(validate_base_url(b)?)),
        None => Ok(None),
    }
}

/// Ping a provider's "list models" endpoint to verify the API key works.
///
/// Thin Tauri-command wrapper over `shannon_engine::api::probe::probe_provider_endpoint`
/// (ADR-0005 task 5 — the engine is the single implementation; this module
/// adds desktop's stricter `validate_base_url` and the typed
/// `TestConnectionResult` mapping so the frontend keeps its existing
/// response shape). 200 → Success, 401/403 → InvalidKey, 429 → RateLimited,
/// 5xx → ProviderError, network/timeout failure → NetworkUnreachable,
/// anything else → Unknown.
#[tauri::command]
pub async fn test_provider_connection(
    provider: String,
    api_key: String,
    base_url: Option<String>,
) -> Result<TestConnectionResult, String> {
    use shannon_engine::api::ApiError;
    use shannon_engine::api::probe::probe_provider_endpoint;

    // Desktop-side strict validation: no embedded credentials, requires a
    // host. The engine does its own defence-in-depth scheme check.
    if let Some(raw) = base_url.as_deref().filter(|s| !s.is_empty()) {
        validate_base_url(raw)?;
    }

    match probe_provider_endpoint(&provider, &api_key, base_url.as_deref()).await {
        Ok(()) => Ok(TestConnectionResult::Success),
        Err(ApiError::AuthenticationFailed) => Ok(TestConnectionResult::InvalidKey),
        Err(ApiError::RateLimitExceeded { .. }) => Ok(TestConnectionResult::RateLimited),
        Err(ApiError::ApiError { status, .. }) if (500..=599).contains(&status) => {
            Ok(TestConnectionResult::ProviderError { status })
        }
        Err(ApiError::Timeout) => Ok(TestConnectionResult::NetworkUnreachable),
        Err(ApiError::HttpError(e)) if e.is_connect() || e.is_timeout() => {
            Ok(TestConnectionResult::NetworkUnreachable)
        }
        Err(other) => Ok(TestConnectionResult::Unknown {
            message: other.to_string(),
        }),
    }
}

/// One row in the response from [`test_all_providers`]. Carries enough
/// identifying info that the Settings → Models "Test all providers" UI
/// can render a per-row status without re-fetching the provider list.
///
/// `latency_ms` is `None` when the probe did not run (auth-missing
/// `NotConfigured`, or a kind the engine cannot probe generically). The
/// error variant uses the same [`TestConnectionResult`] shape the
/// single-provider test returns so the UI can render identical rows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ProviderTestRow {
    pub id: String,
    pub label: String,
    pub provider_kind: String,
    pub result: TestConnectionResult,
    pub latency_ms: Option<u32>,
}

/// Fan-out probe for every configured provider connection (ADR-0005 P4.12).
///
/// Iterates the engine `ProviderConfigStore` roster, looks up each
/// connection's API key from `~/.shannon/credentials/<id>.json` (A1 — never
/// the plaintext `api_key` column), and probes each endpoint in parallel via
/// the same `probe_provider_endpoint` the single-provider test uses. Returns
/// one [`ProviderTestRow`] per connection, in the same order as the store.
///
/// Two non-network paths surface as `TestConnectionResult::Unknown` without
/// consuming a network round-trip:
/// - The provider kind is not generically probeable (Gemini, Bedrock, etc.).
/// - No API key is resolvable from the credential store (avoids a guaranteed
///   401 and gives the user an actionable "missing key" row).
///
/// **Per-provider timeout:** 6s. Same default the single-provider command
/// uses; small enough that 10 providers finish in < 10s on a healthy
/// network, large enough that a slow link still produces a verdict.
#[tauri::command]
pub async fn test_all_providers(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProviderTestRow>, String> {
    use shannon_engine::api::probe::probe_provider_endpoint;

    let connections: Vec<ProviderConnection> = {
        let store = state.provider_store.lock().await;
        providers_file_from_store(&store).providers
    };

    let timeout = std::time::Duration::from_secs(6);
    let mut tasks = Vec::with_capacity(connections.len());
    for conn in connections {
        let id = conn.id.clone();
        let label = conn.label.clone();
        let provider_kind_str = conn.provider_kind.clone();
        let base_url = conn.base_url.clone();
        let needs_key = match conn.provider_kind.as_str() {
            // Mirror KIND_INFO's `needsKey`: only Ollama runs without a key.
            "ollama" => false,
            _ => true,
        };

        let api_key = if needs_key {
            shannon_core::credential_manager::read_credential_value_default(&id).unwrap_or_default()
        } else {
            String::new()
        };

        if needs_key && api_key.is_empty() {
            tasks.push(tokio::spawn(async move {
                ProviderTestRow {
                    id,
                    label,
                    provider_kind: provider_kind_str,
                    result: TestConnectionResult::Unknown {
                        message: "no API key configured for this provider".to_string(),
                    },
                    latency_ms: None,
                }
            }));
            continue;
        }

        // The engine's `probe_provider_endpoint` understands a kebab-case
        // kind string (mirrors the wire `provider_kind`). Anything outside
        // the supported set is reported as `Unknown` so the UI can render
        // an "unsupported" row instead of letting it masquerade as a
        // connectivity failure.
        let engine_kind = match provider_kind_str.as_str() {
            "anthropic" | "openai" | "openai-compatible" | "ollama" | "deepseek" | "gemini" => {
                provider_kind_str.clone()
            }
            other => {
                let other = other.to_string();
                tasks.push(tokio::spawn(async move {
                    ProviderTestRow {
                        id,
                        label,
                        provider_kind: provider_kind_str,
                        result: TestConnectionResult::Unknown {
                            message: format!("provider kind `{other}` is not supported"),
                        },
                        latency_ms: None,
                    }
                }));
                continue;
            }
        };

        tasks.push(tokio::spawn(async move {
            let start = std::time::Instant::now();
            let result = tokio::time::timeout(
                timeout,
                probe_provider_endpoint(
                    &engine_kind,
                    &api_key,
                    base_url.as_deref().filter(|s| !s.is_empty()),
                ),
            )
            .await;
            let latency_ms = start.elapsed().as_millis() as u32;
            let mapped = match result {
                Ok(Ok(())) => TestConnectionResult::Success,
                Ok(Err(shannon_engine::api::ApiError::AuthenticationFailed)) => {
                    TestConnectionResult::InvalidKey
                }
                Ok(Err(shannon_engine::api::ApiError::RateLimitExceeded { .. })) => {
                    TestConnectionResult::RateLimited
                }
                Ok(Err(shannon_engine::api::ApiError::ApiError { status, .. }))
                    if (500..=599).contains(&status) =>
                {
                    TestConnectionResult::ProviderError { status }
                }
                Ok(Err(shannon_engine::api::ApiError::Timeout)) => {
                    TestConnectionResult::NetworkUnreachable
                }
                Ok(Err(shannon_engine::api::ApiError::HttpError(e)))
                    if e.is_connect() || e.is_timeout() =>
                {
                    TestConnectionResult::NetworkUnreachable
                }
                Ok(Err(other)) => TestConnectionResult::Unknown {
                    message: other.to_string(),
                },
                Err(_) => TestConnectionResult::NetworkUnreachable,
            };
            ProviderTestRow {
                id,
                label,
                provider_kind: provider_kind_str,
                result: mapped,
                latency_ms: Some(latency_ms),
            }
        }));
    }

    let mut rows = Vec::with_capacity(tasks.len());
    for t in tasks {
        match t.await {
            Ok(row) => rows.push(row),
            Err(e) => {
                tracing::warn!("test_all_providers: task join failed: {e}");
            }
        }
    }
    Ok(rows)
}

/// Return the kebab-case string the engine's `probe_provider_endpoint`
/// understands for the given typed `ProviderKind`. Mirrors the wire-side
/// `provider_kind` strings the Add Provider modal already emits.
#[allow(dead_code)] // Test-only after the inline match landed in `test_all_providers`.
fn engine_kind_str(k: &shannon_types::provider_config::ProviderKind) -> String {
    use shannon_types::provider_config::ProviderKind;
    let s: &str = match k {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenAi => "openai",
        ProviderKind::OpenAiCompatible => "openai-compatible",
        ProviderKind::Ollama => "ollama",
        ProviderKind::Deepseek => "deepseek",
        ProviderKind::Gemini => "gemini",
        // `ProviderKind` is `#[non_exhaustive]`; future variants fall back to
        // a literal the engine cannot dispatch on, which surfaces as
        // `Unknown { message: "not supported" }` in the row output rather
        // than silently routing to a wrong probe.
        _ => "unsupported",
    };
    s.to_string()
}

// ===== Managed providers (Models P2) =====
//
// Multiple provider connections are persisted in
// `~/.shannon/desktop/providers.json`. The active connection is mirrored into
// DesktopConfig's singular fields, which is what the engine reads. This keeps
// the engine-facing contract unchanged while letting users manage a roster of
// providers (built-in + custom OpenAI-compatible endpoints like GLM/Kimi).

/// Provider fields supplied by the frontend when adding or editing a managed
/// connection. On edit, `id` identifies the entry; on add it is `None` and the
/// server generates one. An `api_key` of `"***"` or empty means "keep the
/// existing key", so editing the label never blanks the stored secret.
///
/// Phase 2 task 3: the desktop Add Provider modal authors three of the
/// v2 ProviderProfile fields. `extra_headers`, `default_max_tokens`, and
/// `tiers` are mirrored into the connection and passed through to the
/// engine's `ProviderConfigStore` (see `connection_to_profile`). The
/// remaining three v2 fields (`models_url`, `fallback_models`, `quirks`)
/// are read-only on the wire today — the modal doesn't edit them yet —
/// so they stay out of this input shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInput {
    #[serde(default)]
    pub id: Option<String>,
    pub label: String,
    pub provider_kind: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// Per-request HTTP headers. The desktop modal collects key/value
    /// rows; rows with an empty key are dropped client-side so this map
    /// never carries `""` keys. `None` means "don't change" on edit.
    #[serde(default)]
    pub extra_headers: Option<std::collections::HashMap<String, String>>,
    /// Profile-level `max_tokens` default. `None` (or `Some(None)`) means
    /// "no override" — the engine falls back to `cfg.max_tokens` then
    /// 4096. The frontend emits `null` for an empty input.
    #[serde(default)]
    pub default_max_tokens: Option<Option<u32>>,
    /// Per-tier model id overrides. Canonical names only — the modal
    /// doesn't surface the alias vocabulary (`haiku` / `sonnet` / ...)
    /// so the wire shape stays canonical.
    #[serde(default)]
    pub tiers: Option<ProviderTiers>,
}

fn is_known_kind(kind: &str) -> bool {
    matches!(
        kind,
        "anthropic" | "openai" | "deepseek" | "ollama" | "openai-compatible"
    )
}

/// Lowercase alphanumeric slug from an arbitrary label (mirrors the skill
/// candidate slugifier, kept local to avoid a cross-module dependency).
fn slugify_provider(input: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Derive a slug from `label` that does not collide with any existing id.
fn unique_provider_slug(label: &str, existing: &[ProviderConnection]) -> String {
    let base = slugify_provider(label);
    let base = if base.is_empty() {
        "provider".to_string()
    } else {
        base
    };
    let mut candidate = base.clone();
    let mut n = 2;
    while existing.iter().any(|p| p.id == candidate) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

/// Return a copy of `file` with every provider's api_key masked to `"***"`
/// (or left `None`). The UI uses presence to show a "key set" dot without ever
/// receiving the raw secret.
fn mask_providers(mut file: ProvidersFile) -> ProvidersFile {
    for conn in &mut file.providers {
        if conn.api_key.is_some() {
            conn.api_key = Some("***".into());
        }
    }
    file
}

fn emit_providers_changed(app_handle: &tauri::AppHandle, file: &ProvidersFile) {
    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "providers".into(),
            value: file.providers.len().to_string(),
        },
    );
}

/// Apply a provider edit to an existing connection. The API key is preserved
/// unless the caller supplied a fresh (non-empty, non-mask) value, so editing
/// the label/model never blanks the stored secret.
/// Persist a new key into the credential store. Returns Ok when nothing
/// needed to be stored (input key is `None`, empty, or the `"***"` mask).
/// Key write failures surface to the caller; clearing `conn.api_key` only
/// happens after a successful store so a partial write can never leave a
/// provider without its key.
fn store_provider_key(
    conn_id: &str,
    conn_label: &str,
    plaintext: Option<&str>,
) -> Result<(), String> {
    use shannon_core::credential_manager::{Credential, CredentialManager};
    let Some(k) = plaintext.filter(|s| !s.is_empty() && *s != "***") else {
        return Ok(());
    };
    let mut manager =
        CredentialManager::new().map_err(|e| format!("could not open credential store: {e}"))?;
    manager
        .store(Credential::new(conn_label, conn_id, k))
        .map_err(|e| format!("could not write credential `{conn_id}`: {e}"))
}

fn apply_provider_update(
    conn: &mut ProviderConnection,
    input: &ProviderInput,
    base_url: Option<String>,
) {
    conn.label = input.label.clone();
    conn.provider_kind = input.provider_kind.clone();
    // The api_key plaintext path lives in the credential store now — see
    // `save_provider`. Here we only clear when the caller explicitly
    // requested removal (empty string), and never write the key itself.
    if let Some(k) = input.api_key.as_deref() {
        if k.is_empty() {
            conn.api_key = None;
        }
    }
    conn.base_url = base_url;
    conn.model = input.model.clone().filter(|s| !s.is_empty());
    // Phase 2 task 3 — v2 ProviderProfile fields. `None` means "don't
    // touch" so editing the label never blanks an existing setting.
    // `extra_headers` and `default_max_tokens` use `None` for "leave
    // alone" and the inner Option for the value-or-clear signal.
    // `tiers` is plain — replace-on-send matches how the engine store
    // upserts the whole field.
    if let Some(h) = input.extra_headers.as_ref() {
        conn.extra_headers = h.clone();
    }
    if let Some(dmt) = input.default_max_tokens {
        conn.default_max_tokens = dmt;
    }
    if let Some(tiers) = input.tiers.as_ref() {
        conn.tiers = tiers.clone();
    }
}

/// Remove a provider by id, clearing the active pointer when it matched.
/// Errors when no provider carried the given id.
fn remove_provider(mut file: ProvidersFile, id: &str) -> Result<ProvidersFile, String> {
    let before = file.providers.len();
    file.providers.retain(|p| p.id != id);
    if file.providers.len() == before {
        return Err(format!("provider not found: {id}"));
    }
    if file.active_provider_id.as_deref() == Some(id) {
        file.active_provider_id = None;
    }
    Ok(file)
}

/// List all managed providers, masking API keys.
///
/// ADR-0005 Phase 2 / task 5: this command is now read-only against the
/// engine's `~/.shannon/providers.toml` via
/// [`shannon_core::provider_config_store::ProviderConfigStore`].
/// `~/.shannon/desktop/providers.json` is no longer consulted on the read
/// path — it remains as a write-through cache maintained by
/// `save_provider` / `delete_provider` for legacy UI surfaces, but the
/// engine store is the single source of truth (ADR-0005 / Phase 2 task
/// 4 + 5 + P1.1).
///
/// The wire shape is unchanged: a [`ProvidersFile`] with
/// `active_provider_id` + `providers: Vec<ProviderConnection>`. Each
/// `ProviderProfile` is mapped to a [`ProviderConnection`] via
/// `config::from_provider_profile`.
///
/// `list_providers` MUST NOT:
/// - Call `land_profile_in_engine_store` or `save` (this is a read-only
///   command).
/// - Read or write `~/.shannon/desktop/providers.json` (the engine
///   store is the only authority; the legacy file is write-through
///   cache, not read source).
/// - Materialize a real api_key. The wire type's `api_key` is
///   `skip_serializing`; `from_provider_profile` leaves it `None`.
///
/// Empty-store policy: an empty engine store returns an empty
/// `ProvidersFile`. We do NOT attempt to re-migrate from the legacy
/// file — Phase 2 task 4 already lifted every entry on AppState
/// startup, so any remaining `providers.json` entries are stale and
/// must not silently resurrect. If a stale legacy file exists at the
/// same time, log a warning — that's an inconsistency the user should
/// hear about.
#[tauri::command]
pub async fn list_providers(state: tauri::State<'_, AppState>) -> Result<ProvidersFile, String> {
    let store = state.provider_store.lock().await;
    let file = providers_file_from_store(&store);
    // Corrupted-state guard: the engine store is empty (after Phase 2
    // task 4's one-shot migration ran) but a legacy `providers.json`
    // still exists on disk. Don't silently re-migrate; surface the
    // inconsistency so a user investigating the empty list knows what
    // to look at.
    if file.providers.is_empty() && config::providers_path().exists() {
        tracing::warn!(
            "engine ProviderConfigStore is empty but legacy providers.json exists; \
             not re-migrating — check Phase 2 task 4 migration logs"
        );
    }
    Ok(file)
}

/// Pure helper that builds the wire-side [`ProvidersFile`] by reading
/// from the engine [`shannon_core::provider_config_store::ProviderConfigStore`].
/// The store's `"default"` model profile is the single provider list
/// `list_providers` reports. `active_target.provider_id` is the
/// `active_provider_id` the UI uses to highlight the current
/// selection.
///
/// Extracted from [`list_providers`] so the read-side mapping is
/// unit-testable without a Tauri runtime.
fn providers_file_from_store(
    store: &shannon_core::provider_config_store::ProviderConfigStore,
) -> ProvidersFile {
    let cfg = store.config();
    // The engine model is `profiles: HashMap<String, ModelProfile>`;
    // only the canonical "default" profile holds user-managed
    // connections on the desktop. Auxiliary / gateway routing profiles
    // are out of scope for this command.
    let default_profile = match cfg.profiles.get("default") {
        Some(p) => p,
        None => {
            return ProvidersFile::default();
        }
    };

    let active_provider_id = if default_profile.active_target.provider_id.is_empty() {
        None
    } else {
        Some(default_profile.active_target.provider_id.clone())
    };

    let providers: Vec<ProviderConnection> = default_profile
        .providers
        .iter()
        .map(|p| config::from_provider_profile(&p.id, p))
        .collect();

    ProvidersFile {
        active_provider_id,
        providers,
    }
}

/// Insert or update a managed provider. Returns the updated (masked) file.
///
/// New keys (non-empty, non-`"***"`) are routed into the credential store
/// (`~/.shannon/credentials/<id>.json`); the on-disk `providers.json` never
/// carries plaintext anymore (A1 — config never carries plaintext secrets).
/// The connection is also landed in the engine's
/// `~/.shannon/providers.toml` via `ProviderConfigStore::upsert_profile`
/// so the runtime path sees the same shape as the REPL's `/connect`
/// (ADR-0005 Phase 2 / task 4).
#[tauri::command]
pub async fn save_provider(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    input: ProviderInput,
) -> Result<ProvidersFile, String> {
    if !is_known_kind(&input.provider_kind) {
        return Err(format!("unknown provider kind: {}", input.provider_kind));
    }
    let base_url = resolve_base_url(&input.base_url)?;
    let mut file = config::load_providers();

    let (target_id, target_label) = if let Some(id) = input.id.as_deref() {
        let conn = file
            .providers
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("provider not found: {id}"))?;
        apply_provider_update(conn, &input, base_url);
        (conn.id.clone(), conn.label.clone())
    } else {
        let id = unique_provider_slug(&input.label, &file.providers);
        let conn = ProviderConnection {
            id: id.clone(),
            label: input.label.clone(),
            provider_kind: input.provider_kind.clone(),
            // The plaintext key lives in the credential store, never in this
            // struct. `store_provider_key` below writes it through the
            // store. We deliberately leave the struct field `None` here.
            api_key: None,
            base_url,
            model: input.model.filter(|s| !s.is_empty()),
            // Phase 2 task 3 — v2 ProviderProfile fields authored by
            // the Add Provider modal. On insert the client sends the
            // explicit value (or `None` for "unset") for all three;
            // defaulting on the wire keeps the Rust path free of
            // `unwrap_or_default()` foot-guns.
            extra_headers: input.extra_headers.clone().unwrap_or_default(),
            default_max_tokens: input.default_max_tokens.unwrap_or(None),
            tiers: input.tiers.clone().unwrap_or_default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            ..Default::default()
        };
        let label = conn.label.clone();
        file.providers.push(conn);
        (id, label)
    };

    // Route any newly-supplied plaintext key into the credential store
    // before persisting `providers.json`. The credential store and the
    // config file are committed separately so a credential-store failure
    // surfaces as a save error rather than a silently half-configured
    // provider.
    store_provider_key(&target_id, &target_label, input.api_key.as_deref())?;

    config::save_providers(&file)?;

    // Mirror the saved connection into the engine's providers.toml so
    // the runtime path reads the same shape as the UI. The model_id
    // we hand the engine is the user-supplied default for this
    // connection (or "default" when the user didn't pick one). For
    // managed openai-compatible connections (glm / kimi / etc.) this
    // uses the desktop's slug ("glm") as the profile id, which
    // `upsert_profile` preserves verbatim — no OpenAI-collapse.
    let updated_conn = file
        .providers
        .iter()
        .find(|p| p.id == target_id)
        .expect("just inserted/updated")
        .clone();
    let model_id = updated_conn
        .model
        .clone()
        .unwrap_or_else(|| "default".to_string());
    land_profile_in_engine_store(&state, &updated_conn, &model_id).await?;

    emit_providers_changed(&app_handle, &file);
    Ok(mask_providers(file))
}

/// Delete a managed provider by id. Clears `active_provider_id` if it pointed
/// at the deleted entry. Returns the updated (masked) file. Also removes
/// the slot from the engine's `providers.toml` so a stale connection
/// cannot survive a desktop restart.
#[tauri::command]
pub async fn delete_provider(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<ProvidersFile, String> {
    let file = remove_provider(config::load_providers(), &id)?;
    config::save_providers(&file)?;
    remove_profile_from_engine_store(&state, &id).await?;
    emit_providers_changed(&app_handle, &file);
    Ok(mask_providers(file))
}

/// Activate a managed provider: lands the connection in the engine's
/// `~/.shannon/providers.toml` via `ProviderConfigStore::upsert_profile`
/// (so the desktop shell and the CLI agree on the active target on the
/// next launch — ADR-0005 Phase 2 acceptance), rebuilds the live
/// `state.client_config` from the engine store, and emits
/// `CONFIG_UPDATED` so the tray and any open windows refresh their
/// provider label.
///
/// P1.2-B (ADR-0005): the legacy mirror step that wrote
/// `DesktopConfig.provider`/`api_key`/`base_url`/`model` is gone —
/// those fields no longer exist. `land_profile_in_engine_store` is the
/// single source-of-truth writer.
#[tauri::command]
pub async fn set_active_provider(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    let mut file = config::load_providers();
    let conn = file
        .providers
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("provider not found: {id}"))?
        .clone();

    let provider_kind = conn.provider_kind.clone();
    let model = conn.model.clone();

    // Land the activation in the engine's `~/.shannon/providers.toml`
    // *before* rebuilding the client config, because the
    // `build_client_config` (P1.1 / ADR-0005) reads from the engine
    // store, not from the legacy mirror. `upsert_profile` (not
    // `set_active(&LlmProvider, ...)`) is used so a managed
    // openai-compatible slot (glm / kimi) keeps its desktop slug as
    // the profile id and does not collapse onto the engine's single
    // "openai" slot.
    let model_id = model.clone().unwrap_or_else(|| "default".to_string());
    land_profile_in_engine_store(&state, &conn, &model_id).await?;

    // The runtime client config reads from the engine store (now
    // updated by `land_profile_in_engine_store` above). Behavioural
    // overrides (`max_tokens`/`temperature`) come from `desktop_cfg`.
    let desktop_cfg = state.desktop_config.read().await.clone();
    let shannon_overrides = shannon_core::unified_config::ShannonConfig {
        max_tokens: desktop_cfg.max_tokens.map(|v| v as usize),
        temperature: desktop_cfg.temperature,
        ..Default::default()
    };
    let client_config = {
        let store_guard = state.provider_store.lock().await;
        AppState::build_client_config(&store_guard, &shannon_overrides).unwrap_or_default()
    };
    {
        let mut c = state.client_config.write().await;
        *c = client_config;
    }

    file.active_provider_id = Some(id.clone());
    config::save_providers(&file)?;

    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "provider".into(),
            value: provider_kind,
        },
    );
    Ok(())
}

/// Resolve a desktop provider slug (e.g. `"openai-compatible"`) to an
/// `LlmProvider` for the engine-side activation mirror. `openai-compatible`
/// collapses to `OpenAI` for catalog walking — the real provider is
/// whatever the user's `base_url` points at (served through the desktop
/// singular config above).
fn llm_provider_for_active_mirror(s: &str) -> Option<shannon_engine::api::LlmProvider> {
    use shannon_engine::api::LlmProvider;
    match s {
        "anthropic" => Some(LlmProvider::Anthropic),
        "openai" => Some(LlmProvider::OpenAI),
        "ollama" => Some(LlmProvider::Ollama),
        "gemini" => Some(LlmProvider::Gemini),
        "deepseek" => Some(LlmProvider::DeepSeek),
        "openai-compatible" => Some(LlmProvider::OpenAI),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_update_round_trips_through_serde() {
        let update = ConfigUpdate {
            key: "model".to_string(),
            value: "claude-opus".to_string(),
        };
        let json = serde_json::to_string(&update).unwrap();
        let back: ConfigUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(back.key, "model");
        assert_eq!(back.value, "claude-opus");
    }

    #[test]
    fn provider_switch_request_round_trips_through_serde() {
        let req = ProviderSwitchRequest {
            provider: "openai".to_string(),
            api_key: Some("sk-test".to_string()),
            base_url: None,
            model: "gpt-4.1".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ProviderSwitchRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.provider, "openai");
        assert_eq!(back.api_key, Some("sk-test".to_string()));
    }

    #[test]
    fn validate_base_url_accepts_http_and_https_and_strips_trailing_slash() {
        assert_eq!(
            validate_base_url("https://api.openai.com").unwrap(),
            "https://api.openai.com"
        );
        assert_eq!(
            validate_base_url("https://open.bigmodel.cn/api/paas/v4/").unwrap(),
            "https://open.bigmodel.cn/api/paas/v4"
        );
        // http + localhost is valid (Ollama / self-hosted models).
        assert_eq!(
            validate_base_url("http://localhost:11434").unwrap(),
            "http://localhost:11434"
        );
        // Fragment is dropped.
        assert_eq!(
            validate_base_url("https://api.openai.com/v1#section").unwrap(),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn validate_base_url_rejects_non_http_schemes() {
        assert!(validate_base_url("file:///etc/passwd").is_err());
        assert!(validate_base_url("data:text/plain,hello").is_err());
        assert!(validate_base_url("gopher://example.com").is_err());
    }

    #[test]
    fn validate_base_url_rejects_embedded_credentials() {
        assert!(validate_base_url("https://user:pass@example.com").is_err());
        assert!(validate_base_url("https://token@example.com").is_err());
    }

    #[test]
    fn validate_base_url_rejects_unparseable_and_schemeless_input() {
        // No scheme → url::Url cannot parse a relative URL.
        assert!(validate_base_url("api.openai.com").is_err());
        assert!(validate_base_url("").is_err());
        assert!(validate_base_url("   ").is_err());
        assert!(validate_base_url("ht!tp://bad").is_err());
    }

    #[test]
    fn resolve_base_url_handles_none_and_blank() {
        assert_eq!(resolve_base_url(&None).unwrap(), None);
        assert_eq!(resolve_base_url(&Some(String::new())).unwrap(), None);
        assert_eq!(resolve_base_url(&Some("   ".to_string())).unwrap(), None);
        assert_eq!(
            resolve_base_url(&Some("https://api.openai.com/".to_string())).unwrap(),
            Some("https://api.openai.com".to_string())
        );
        assert!(resolve_base_url(&Some("file:///x".to_string())).is_err());
    }

    #[test]
    fn slugify_provider_collapses_non_alphanumerics() {
        assert_eq!(slugify_provider("My GLM Key"), "my-glm-key");
        assert_eq!(slugify_provider("UPPER_case!"), "upper-case");
        assert_eq!(slugify_provider("  leading/trailing  "), "leading-trailing");
        assert_eq!(slugify_provider("😎"), "");
    }

    #[test]
    fn unique_provider_slug_appends_suffix_on_collision() {
        let existing = vec![ProviderConnection {
            id: "glm".into(),
            label: "GLM".into(),
            provider_kind: "openai-compatible".into(),
            api_key: None,
            base_url: None,
            model: None,
            created_at: "2026-06-27T00:00:00Z".into(),
            ..Default::default()
        }];
        // "glm" already exists → first collision gets "-2".
        assert_eq!(unique_provider_slug("GLM", &existing), "glm-2");
        // Empty label falls back to the literal "provider".
        assert_eq!(unique_provider_slug("😎", &[]), "provider");
    }

    #[test]
    fn mask_providers_replaces_keys_but_keeps_absence() {
        let file = ProvidersFile {
            active_provider_id: Some("a".into()),
            providers: vec![
                ProviderConnection {
                    id: "a".into(),
                    label: "A".into(),
                    provider_kind: "anthropic".into(),
                    api_key: Some("sk-secret".into()),
                    base_url: None,
                    model: None,
                    created_at: "2026-06-27T00:00:00Z".into(),
                    ..Default::default()
                },
                ProviderConnection {
                    id: "b".into(),
                    label: "B".into(),
                    provider_kind: "ollama".into(),
                    api_key: None,
                    base_url: None,
                    model: None,
                    created_at: "2026-06-27T00:00:00Z".into(),
                    ..Default::default()
                },
            ],
        };
        let masked = mask_providers(file);
        assert_eq!(masked.providers[0].api_key.as_deref(), Some("***"));
        assert!(masked.providers[1].api_key.is_none());
    }

    #[test]
    fn provider_input_deserializes_without_optional_fields() {
        let json = r#"{"label":"GLM","provider_kind":"openai-compatible"}"#;
        let input: ProviderInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.label, "GLM");
        assert!(input.id.is_none());
        assert!(input.api_key.is_none());
        assert!(input.base_url.is_none());
        assert!(input.model.is_none());
    }

    // === Models P2 command-level logic (helpers extracted from the commands) ===

    fn sample_conn(id: &str, kind: &str, _key: Option<&str>) -> ProviderConnection {
        // Note: the `key` parameter is intentionally ignored. ProviderConnection
        // never carries a plaintext `api_key` anymore (ADR-0005 Phase 2 / task 6
        // — keys live in `~/.shannon/credentials/<id>.json`). The parameter
        // is retained so existing call sites continue to compile.
        ProviderConnection {
            id: id.into(),
            label: id.into(),
            provider_kind: kind.into(),
            api_key: None,
            base_url: None,
            model: None,
            created_at: "2026-06-28T00:00:00Z".into(),
            ..Default::default()
        }
    }

    fn provider_input(
        id: Option<&str>,
        label: &str,
        kind: &str,
        key: Option<&str>,
    ) -> ProviderInput {
        ProviderInput {
            id: id.map(str::to_string),
            label: label.into(),
            provider_kind: kind.into(),
            api_key: key.map(str::to_string),
            base_url: None,
            model: None,
            // Phase 2 task 3 — default to `None` so the helper doesn't
            // touch the v2 fields; individual tests pass the field
            // explicitly when they care about it.
            extra_headers: None,
            default_max_tokens: None,
            tiers: None,
        }
    }

    #[test]
    fn apply_provider_update_never_touches_plaintext_key_field() {
        // ADR-0005 Phase 2: `apply_provider_update` no longer mutates
        // `conn.api_key`. Plaintext keys live in the credential store, so the
        // metadata-only update is responsible for everything except the key
        // (which `store_provider_key` writes separately from the caller).
        let mut conn = sample_conn("anthropic", "anthropic", Some("sk-real"));

        for key in [Some("***"), None, Some(""), Some("sk-new")] {
            apply_provider_update(
                &mut conn,
                &provider_input(Some("anthropic"), "Anthropic", "anthropic", key),
                None,
            );
            assert!(
                conn.api_key.is_none(),
                "apply_provider_update must never set api_key (saw key={:?})",
                key
            );
        }
        // The label still updates regardless of key handling.
        assert_eq!(conn.label, "Anthropic");
    }

    #[test]
    fn apply_provider_update_sets_base_url_and_blanks_empty_model() {
        let mut conn = sample_conn("glm", "openai-compatible", Some("k"));
        let input = ProviderInput {
            id: Some("glm".into()),
            label: "My GLM".into(),
            provider_kind: "openai-compatible".into(),
            api_key: Some("***".into()),
            base_url: Some("https://open.bigmodel.cn/api/paas/v4".into()),
            model: Some("".into()), // empty => cleared
            extra_headers: None,
            default_max_tokens: None,
            tiers: None,
        };
        apply_provider_update(
            &mut conn,
            &input,
            Some("https://open.bigmodel.cn/api/paas/v4".into()),
        );
        assert_eq!(
            conn.base_url.as_deref(),
            Some("https://open.bigmodel.cn/api/paas/v4")
        );
        assert!(conn.model.is_none());
    }

    /// Phase 2 task 3 — the desktop Add Provider modal authors
    /// `extra_headers`, `default_max_tokens`, and `tiers`. The wire shape
    /// uses `Option<...>` so the backend can distinguish "leave the
    /// existing value alone" (`None`) from "set to this value"
    /// (`Some(...)`). The tests below pin both signals.
    #[test]
    fn apply_provider_update_applies_v2_profile_fields_when_present() {
        let mut conn = sample_conn("anthropic", "anthropic", Some("k"));
        let mut headers = std::collections::HashMap::new();
        headers.insert("X-Custom".to_string(), "yes".to_string());
        let input = ProviderInput {
            id: Some("anthropic".into()),
            label: "Anthropic".into(),
            provider_kind: "anthropic".into(),
            api_key: Some("***".into()),
            base_url: None,
            model: None,
            extra_headers: Some(headers.clone()),
            default_max_tokens: Some(Some(8192)),
            tiers: Some(ProviderTiers {
                fast: Some("haiku-model".into()),
                standard: Some("sonnet-model".into()),
                pro: Some("opus-model".into()),
            }),
        };
        apply_provider_update(&mut conn, &input, None);
        assert_eq!(conn.extra_headers, headers);
        assert_eq!(conn.default_max_tokens, Some(8192));
        assert_eq!(conn.tiers.fast.as_deref(), Some("haiku-model"));
        assert_eq!(conn.tiers.standard.as_deref(), Some("sonnet-model"));
        assert_eq!(conn.tiers.pro.as_deref(), Some("opus-model"));
    }

    #[test]
    fn apply_provider_update_leaves_v2_profile_fields_untouched_when_none() {
        // Editing the label must not blank out an existing
        // `default_max_tokens` override the user previously set.
        let mut conn = sample_conn("anthropic", "anthropic", Some("k"));
        conn.extra_headers.insert("X-Existing".into(), "yes".into());
        conn.default_max_tokens = Some(4096);
        conn.tiers.standard = Some("prev".into());

        let input = ProviderInput {
            id: Some("anthropic".into()),
            label: "Renamed".into(),
            provider_kind: "anthropic".into(),
            api_key: Some("***".into()),
            base_url: None,
            model: None,
            extra_headers: None,
            default_max_tokens: None,
            tiers: None,
        };
        apply_provider_update(&mut conn, &input, None);
        assert_eq!(conn.label, "Renamed");
        assert_eq!(
            conn.extra_headers.get("X-Existing").map(String::as_str),
            Some("yes"),
        );
        assert_eq!(conn.default_max_tokens, Some(4096));
        assert_eq!(conn.tiers.standard.as_deref(), Some("prev"));
    }

    /// `Some(None)` on `default_max_tokens` is the explicit "clear the
    /// override" signal. The modal never emits this (an empty input
    /// becomes `None` on the wire), but the wire shape supports it so
    /// future editors can clear a previously-set value.
    #[test]
    fn apply_provider_update_explicit_none_on_default_max_tokens_clears_it() {
        let mut conn = sample_conn("anthropic", "anthropic", Some("k"));
        conn.default_max_tokens = Some(4096);

        let input = ProviderInput {
            id: Some("anthropic".into()),
            label: "Anthropic".into(),
            provider_kind: "anthropic".into(),
            api_key: Some("***".into()),
            base_url: None,
            model: None,
            extra_headers: None,
            default_max_tokens: Some(None),
            tiers: None,
        };
        apply_provider_update(&mut conn, &input, None);
        assert!(conn.default_max_tokens.is_none());
    }

    #[test]
    fn remove_provider_clears_active_when_active_is_deleted() {
        let file = ProvidersFile {
            active_provider_id: Some("a".into()),
            providers: vec![
                sample_conn("a", "anthropic", Some("k1")),
                sample_conn("b", "openai", Some("k2")),
            ],
        };
        let out = remove_provider(file, "a").unwrap();
        assert!(out.active_provider_id.is_none());
        assert_eq!(out.providers.len(), 1);
        assert_eq!(out.providers[0].id, "b");
    }

    #[test]
    fn remove_provider_keeps_active_when_other_is_deleted() {
        let file = ProvidersFile {
            active_provider_id: Some("b".into()),
            providers: vec![
                sample_conn("a", "anthropic", Some("k1")),
                sample_conn("b", "openai", Some("k2")),
            ],
        };
        let out = remove_provider(file, "a").unwrap();
        assert_eq!(out.active_provider_id.as_deref(), Some("b"));
        assert_eq!(out.providers.len(), 1);
    }

    #[test]
    fn remove_provider_errors_on_unknown_id() {
        let file = ProvidersFile {
            active_provider_id: Some("a".into()),
            providers: vec![sample_conn("a", "anthropic", Some("k1"))],
        };
        assert!(remove_provider(file, "nope").is_err());
    }

    // === Engine-store write helpers (Phase 2 task 4) ===
    //
    // `land_profile_in_engine_store` and `remove_profile_from_engine_store`
    // are async and require a Tauri State, so they're hard to unit-test
    // in isolation. The pure helpers they build on are tested here —
    // the rest is exercised by the engine-side tests for
    // `ProviderConfigStore::upsert_profile / remove_profile`.

    #[test]
    fn default_base_url_for_kind_returns_canonical_url_per_kind() {
        assert_eq!(
            default_base_url_for_kind("anthropic"),
            Some("https://api.anthropic.com")
        );
        assert_eq!(
            default_base_url_for_kind("openai"),
            Some("https://api.openai.com")
        );
        assert_eq!(
            default_base_url_for_kind("ollama"),
            Some("http://localhost:11434")
        );
        assert_eq!(
            default_base_url_for_kind("deepseek"),
            Some("https://api.deepseek.com")
        );
    }

    #[test]
    fn default_base_url_for_kind_returns_none_for_openai_compatible() {
        // openai-compatible is the user-supplied-URL catch-all — the
        // engine has no canonical default for it.
        assert_eq!(default_base_url_for_kind("openai-compatible"), None);
    }

    #[test]
    fn default_base_url_for_kind_returns_none_for_unknown_kind() {
        // Unknown slugs should not panic — they get the openai-
        // compatible collapse path in `to_provider_profile`.
        assert_eq!(default_base_url_for_kind("anthropicc"), None);
    }

    #[test]
    fn connection_to_profile_passes_user_base_url_through() {
        // When the user supplies a custom base_url (the openai-
        // compatible case), the default-base-url fallback is not used.
        let conn = ProviderConnection {
            id: "kimi".into(),
            label: "Kimi".into(),
            provider_kind: "openai-compatible".into(),
            api_key: None,
            base_url: Some("https://api.moonshot.cn/v1".into()),
            model: Some("moonshot-v1-128k".into()),
            created_at: "2026-07-30T00:00:00Z".into(),
            ..Default::default()
        };
        let profile = connection_to_profile(&conn);
        assert_eq!(profile.base_url, "https://api.moonshot.cn/v1");
        assert_eq!(profile.id, "kimi");
        // OpenAI-compatible collapses to OpenAI in the engine's
        // kind enum; the engine's resolve_provider recovers
        // identity from base_url at resolution time.
        assert_eq!(
            profile.kind,
            shannon_types::provider_config::ProviderKind::OpenAiCompatible
        );
    }

    #[test]
    fn connection_to_profile_falls_back_to_engine_canonical_default() {
        // Anthropic without an explicit base_url → engine default.
        let conn = ProviderConnection {
            id: "anthropic-main".into(),
            label: "Anthropic".into(),
            provider_kind: "anthropic".into(),
            api_key: None,
            base_url: None,
            model: None,
            created_at: "2026-07-30T00:00:00Z".into(),
            ..Default::default()
        };
        let profile = connection_to_profile(&conn);
        assert_eq!(profile.base_url, "https://api.anthropic.com");
    }

    // === list_providers / ProviderConfigStore read path (ADR-0005 Phase 2 task 5) ===
    //
    // `list_providers` is now a pure read against
    // `ProviderConfigStore::config().profiles["default"]`. The tests
    // below pin the wire-shape round-trip: input written through the
    // engine store surfaces as a `ProvidersFile` that the desktop UI
    // already knows how to render. In-memory
    // `ProviderConfigStore::default()` is used so we never touch
    // `~/.shannon/providers.toml`.

    fn upsert_test_profile(
        store: &mut shannon_core::provider_config_store::ProviderConfigStore,
        id: &str,
        kind: shannon_types::provider_config::ProviderKind,
        base_url: &str,
        model_id: &str,
    ) {
        use shannon_types::provider_config::{CredentialRef, ProviderProfile, ProviderTiers};
        let profile = ProviderProfile {
            id: id.to_string(),
            kind,
            display_name: format!("{id} label"),
            base_url: base_url.to_string(),
            models_url: None,
            credential: CredentialRef::Store {
                service: id.to_string(),
            },
            extra_headers: std::collections::HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
        };
        store.upsert_profile(profile, model_id);
    }

    #[test]
    fn list_providers_returns_empty_when_store_has_no_profiles() {
        // No upsert → `"default"` profile slot has zero providers.
        // The wire file must be `{ active: None, providers: [] }`.
        let store = shannon_core::provider_config_store::ProviderConfigStore::default();
        let file = providers_file_from_store(&store);
        assert!(file.active_provider_id.is_none());
        assert!(file.providers.is_empty());
    }

    #[test]
    fn list_providers_maps_engine_profile_to_wire_type() {
        // One profile written via the engine write path surfaces as
        // one `ProviderConnection` with id/label/kind/base_url all
        // preserved.
        let mut store = shannon_core::provider_config_store::ProviderConfigStore::default();
        upsert_test_profile(
            &mut store,
            "anthropic-main",
            shannon_types::provider_config::ProviderKind::Anthropic,
            "https://api.anthropic.com",
            "claude-sonnet-4-20250514",
        );

        let file = providers_file_from_store(&store);
        assert_eq!(file.providers.len(), 1);
        let conn = &file.providers[0];
        assert_eq!(conn.id, "anthropic-main");
        assert_eq!(conn.label, "anthropic-main label");
        assert_eq!(conn.provider_kind, "anthropic");
        assert_eq!(conn.base_url.as_deref(), Some("https://api.anthropic.com"));
        // Active target follows the last upsert.
        assert_eq!(file.active_provider_id.as_deref(), Some("anthropic-main"));
    }

    #[test]
    fn list_providers_reports_active_target() {
        // Two profiles in the store; the active pointer resolves to
        // the most-recent upserted one.
        let mut store = shannon_core::provider_config_store::ProviderConfigStore::default();
        upsert_test_profile(
            &mut store,
            "glm",
            shannon_types::provider_config::ProviderKind::OpenAiCompatible,
            "https://open.bigmodel.cn/api/paas/v4",
            "glm-4.6",
        );
        upsert_test_profile(
            &mut store,
            "kimi",
            shannon_types::provider_config::ProviderKind::OpenAiCompatible,
            "https://api.moonshot.cn/v1",
            "moonshot-v1-128k",
        );
        // Re-point active at `glm` (the third upsert overwrites active).
        upsert_test_profile(
            &mut store,
            "glm",
            shannon_types::provider_config::ProviderKind::OpenAiCompatible,
            "https://open.bigmodel.cn/api/paas/v4",
            "glm-flash",
        );

        let file = providers_file_from_store(&store);
        assert_eq!(file.providers.len(), 2, "two profiles in the store");
        assert_eq!(
            file.active_provider_id.as_deref(),
            Some("glm"),
            "active target follows the most-recent upsert"
        );
    }

    #[test]
    fn list_providers_does_not_serialize_api_key() {
        // A1: the wire type's `api_key` is `skip_serializing`. The
        // pure helper always sets it to None — even if an engine
        // profile could theoretically carry one, we never propagate
        // it. The masked JSON string must contain no "api_key" / no
        // secret-looking value.
        let mut store = shannon_core::provider_config_store::ProviderConfigStore::default();
        upsert_test_profile(
            &mut store,
            "anthropic-main",
            shannon_types::provider_config::ProviderKind::Anthropic,
            "https://api.anthropic.com",
            "claude-sonnet-4-20250514",
        );

        let file = providers_file_from_store(&store);
        assert!(file.providers[0].api_key.is_none());

        let json = serde_json::to_string(&file).expect("wire file serializes");
        assert!(
            !json.contains("api_key"),
            "api_key must not appear in wire JSON (saw {json})"
        );
        assert!(
            !json.contains("sk-"),
            "no secret-like value must appear in wire JSON (saw {json})"
        );
    }

    #[test]
    fn list_providers_with_extra_headers_and_tiers() {
        // v2 fields (`extra_headers`, `default_max_tokens`, `tiers`)
        // round-trip from the engine profile to the wire type.
        use shannon_types::provider_config::{
            CredentialRef, ProviderKind, ProviderProfile, ProviderTiers,
        };
        let mut store = shannon_core::provider_config_store::ProviderConfigStore::default();
        let mut extra_headers = std::collections::HashMap::new();
        extra_headers.insert("X-Foo".to_string(), "bar".to_string());
        extra_headers.insert("X-Region".to_string(), "us-east".to_string());
        let profile = ProviderProfile {
            id: "anthropic-main".into(),
            kind: ProviderKind::Anthropic,
            display_name: "Anthropic".into(),
            base_url: "https://api.anthropic.com".into(),
            models_url: None,
            credential: CredentialRef::Store {
                service: "anthropic-main".into(),
            },
            extra_headers: extra_headers.clone(),
            default_max_tokens: Some(2048),
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers {
                fast: Some("haiku-model".into()),
                standard: Some("sonnet-model".into()),
                pro: Some("opus-model".into()),
            },
        };
        store.upsert_profile(profile, "claude-sonnet-4-20250514");

        let file = providers_file_from_store(&store);
        let conn = &file.providers[0];
        assert_eq!(conn.extra_headers, extra_headers);
        assert_eq!(conn.default_max_tokens, Some(2048));
        assert_eq!(conn.tiers.fast.as_deref(), Some("haiku-model"));
        assert_eq!(conn.tiers.standard.as_deref(), Some("sonnet-model"));
        assert_eq!(conn.tiers.pro.as_deref(), Some("opus-model"));
    }

    // ---- engine_kind_str (ADR-0005 P4.12) ----

    #[test]
    fn engine_kind_str_maps_all_supported_variants() {
        // The kebab-case strings here are what `probe_provider_endpoint`
        // dispatches on; if a new variant is added to ProviderKind, this
        // test catches a missing branch in the mapping before it reaches
        // production.
        use shannon_types::provider_config::ProviderKind;
        assert_eq!(engine_kind_str(&ProviderKind::Anthropic), "anthropic");
        assert_eq!(engine_kind_str(&ProviderKind::OpenAi), "openai");
        assert_eq!(
            engine_kind_str(&ProviderKind::OpenAiCompatible),
            "openai-compatible"
        );
        assert_eq!(engine_kind_str(&ProviderKind::Ollama), "ollama");
        assert_eq!(engine_kind_str(&ProviderKind::Deepseek), "deepseek");
        assert_eq!(engine_kind_str(&ProviderKind::Gemini), "gemini");
    }

    // === P1.2-A: routing helpers for configure() engine-store path ===
    //
    // The new `configure('model' | 'api_key' | 'base_url' | 'provider')`
    // arms consult the engine `ProviderConfigStore` to find the active
    // target before writing. These tests pin the lookup helpers —
    // `active_provider_id_and_kind` and `find_provider_by_kind` — plus
    // `provider_kind_slug`, which is the kebab-case ↔ enum bridge the
    // arms rely on.

    fn store_with_single_anthropic_profile(
        id: &str,
        model_id: &str,
    ) -> shannon_core::provider_config_store::ProviderConfigStore {
        use shannon_types::provider_config::{CredentialRef, ProviderProfile, ProviderTiers};
        let mut store = shannon_core::provider_config_store::ProviderConfigStore::default();
        store.upsert_profile(
            ProviderProfile {
                id: id.to_string(),
                kind: shannon_types::provider_config::ProviderKind::Anthropic,
                display_name: format!("{id} label"),
                base_url: "https://api.anthropic.com".into(),
                models_url: None,
                credential: CredentialRef::Store {
                    service: id.to_string(),
                },
                extra_headers: std::collections::HashMap::new(),
                default_max_tokens: None,
                fallback_models: Vec::new(),
                quirks: Default::default(),
                tiers: ProviderTiers::default(),
            },
            model_id,
        );
        store
    }

    #[test]
    fn active_provider_id_and_kind_returns_some_when_active_is_set() {
        let store = store_with_single_anthropic_profile("anthropic-main", "claude-opus-4-8");
        let (id, kind) = active_provider_id_and_kind(&store).expect("active target is set");
        assert_eq!(id, "anthropic-main");
        assert_eq!(kind, "anthropic");
    }

    #[test]
    fn active_provider_id_and_kind_returns_none_when_no_profiles() {
        // No upsert → `"default"` profile has zero providers and
        // `active_target.provider_id` is empty. The lookup must
        // return `None` so the configure('model'|'api_key'|'base_url')
        // arms emit a clear "no active provider" error instead of
        // crashing on an unwrap.
        let store = shannon_core::provider_config_store::ProviderConfigStore::default();
        assert!(active_provider_id_and_kind(&store).is_none());
    }

    #[test]
    fn find_provider_by_kind_returns_first_matching_slot() {
        let store = store_with_single_anthropic_profile("anthropic-main", "claude-opus-4-8");
        let (id, model_id) =
            find_provider_by_kind(&store, "anthropic").expect("anthropic slot is in the store");
        assert_eq!(id, "anthropic-main");
        assert_eq!(model_id, "claude-opus-4-8");
    }

    #[test]
    fn find_provider_by_kind_returns_none_when_kind_unknown() {
        let store = store_with_single_anthropic_profile("anthropic-main", "claude-opus-4-8");
        assert!(find_provider_by_kind(&store, "openai").is_none());
        assert!(find_provider_by_kind(&store, "").is_none());
    }

    #[test]
    fn provider_kind_slug_covers_all_supported_variants() {
        // Mirrors `engine_kind_str`'s coverage — but on the reverse
        // direction (enum → kebab-case slug). If a new variant is added
        // to ProviderKind, this catches a missing branch in the mapping
        // before `configure('provider')` defaults it to "unsupported".
        use shannon_types::provider_config::ProviderKind as K;
        assert_eq!(provider_kind_slug(&K::Anthropic), "anthropic");
        assert_eq!(provider_kind_slug(&K::OpenAi), "openai");
        assert_eq!(
            provider_kind_slug(&K::OpenAiCompatible),
            "openai-compatible"
        );
        assert_eq!(provider_kind_slug(&K::Ollama), "ollama");
        assert_eq!(provider_kind_slug(&K::Deepseek), "deepseek");
        assert_eq!(provider_kind_slug(&K::Gemini), "gemini");
    }
}
