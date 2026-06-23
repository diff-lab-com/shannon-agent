//! Configuration commands — configure, switch_provider, get_config.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::commands::AppState;
use crate::config::{self, DesktopConfig};
use crate::events;
use crate::events::event_names;

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
            let mut model = state.model.lock().await;
            *model = update.value.clone();
            let mut cfg = state.client_config.write().await;
            cfg.model = update.value;

            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.model = Some((*model).clone());
            drop(desktop_cfg);

            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "model".into(),
                    value: (*model).clone(),
                },
            );

            Ok(())
        }
        "api_key" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.api_key = Some(update.value.clone());

            let mut cfg = state.client_config.write().await;
            cfg.api_key = update.value.clone();

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

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
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.base_url = Some(update.value.clone());

            let mut cfg = state.client_config.write().await;
            cfg.base_url = update.value.clone();

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "base_url".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "provider" => {
            let mut provider = state.provider.lock().await;
            *provider = update.value.clone();

            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.provider = Some((*provider).clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "provider".into(),
                    value: update.value,
                },
            );

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
            let mut messages = state.messages.lock().await;
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
#[tauri::command]
pub async fn switch_provider(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    request: ProviderSwitchRequest,
) -> Result<(), String> {
    let existing = state.desktop_config.read().await;
    let new_config = DesktopConfig {
        provider: Some(request.provider.clone()),
        api_key: request.api_key.clone().or_else(|| existing.api_key.clone()),
        base_url: request
            .base_url
            .clone()
            .or_else(|| existing.base_url.clone()),
        model: Some(request.model.clone()),
        working_dir: existing.working_dir.clone(),
        theme: existing.theme.clone(),
        mcp_servers: existing.mcp_servers.clone(),
        approval_mode: existing.approval_mode.clone(),
        strategic_focus: existing.strategic_focus.clone(),
        performance_strategy: existing.performance_strategy.clone(),
        memory_enabled: existing.memory_enabled,
        telemetry_enabled: existing.telemetry_enabled,
        encryption_enabled: existing.encryption_enabled,
        debug_console: existing.debug_console,
        temperature: existing.temperature,
        max_tokens: existing.max_tokens,
        plan: existing.plan.clone(),
        skill_loop_enabled: existing.skill_loop_enabled,
        skill_loop_min_duration_secs: existing.skill_loop_min_duration_secs,
        skill_loop_min_tool_calls: existing.skill_loop_min_tool_calls,
    };
    drop(existing);

    let client_config = AppState::build_client_config(&new_config);

    {
        let mut c = state.client_config.write().await;
        *c = client_config;
    }
    {
        let mut m = state.model.lock().await;
        *m = request.model.clone();
    }
    {
        let mut p = state.provider.lock().await;
        *p = request.provider;
    }
    {
        let mut dc = state.desktop_config.write().await;
        *dc = new_config.clone();
    }

    config::save_config(&new_config)?;

    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "provider".into(),
            value: new_config.provider.clone().unwrap_or_default(),
        },
    );

    Ok(())
}

/// Get the current desktop config (for settings panel).
#[tauri::command]
pub async fn get_config(state: tauri::State<'_, AppState>) -> Result<DesktopConfig, String> {
    let cfg = state.desktop_config.read().await;
    let mut display = cfg.clone();
    if display.api_key.is_some() {
        display.api_key = Some("***".into());
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

/// Ping a provider's "list models" endpoint to verify the API key works.
///
/// Each provider has a cheap GET endpoint that requires auth — we use it as
/// a liveness check. 200 → Success, 401/403 → InvalidKey, 429 → RateLimited,
/// 5xx → ProviderError, network failure → NetworkUnreachable, everything
/// else → Unknown.
#[tauri::command]
pub async fn test_provider_connection(
    provider: String,
    api_key: String,
) -> Result<TestConnectionResult, String> {
    let (url, auth_header) = match provider.as_str() {
        "anthropic" => (
            "https://api.anthropic.com/v1/models?limit=1".to_string(),
            format!("x-api-key: {api_key}"),
        ),
        "openai" => (
            "https://api.openai.com/v1/models".to_string(),
            format!("Authorization: Bearer {api_key}"),
        ),
        "deepseek" => (
            "https://api.deepseek.com/models".to_string(),
            format!("Authorization: Bearer {api_key}"),
        ),
        "ollama" => {
            // Ollama doesn't need auth; just ping the tags endpoint.
            let host = std::env::var("OLLAMA_HOST")
                .unwrap_or_else(|_| "http://localhost:11434".to_string());
            return ping_provider(&format!("{}/api/tags", host.trim_end_matches('/')), None)
                .await
                .map_err(|e| e.to_string());
        }
        other => return Err(format!("unknown provider: {other}")),
    };
    ping_provider(&url, Some(&auth_header))
        .await
        .map_err(|e| e.to_string())
}

async fn ping_provider(
    url: &str,
    auth_header: Option<&str>,
) -> Result<TestConnectionResult, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let mut req = client.get(url);
    if let Some(auth) = auth_header {
        let (name, value) = auth
            .split_once(": ")
            .ok_or_else(|| "malformed auth header".to_string())?;
        req = req.header(name, value);
    }
    if auth_header.is_some() && auth_header.unwrap().starts_with("x-api-key:") {
        req = req.header("anthropic-version", "2023-06-01");
    }
    let resp = req.send().await.map_err(|e| {
        if e.is_connect() || e.is_timeout() {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::NetworkUnreachable,
                e.to_string(),
            )) as Box<dyn std::error::Error + Send + Sync>
        } else {
            Box::new(e) as Box<dyn std::error::Error + Send + Sync>
        }
    })?;
    let status = resp.status().as_u16();
    Ok(match status {
        200..=299 => TestConnectionResult::Success,
        401 | 403 => TestConnectionResult::InvalidKey,
        429 => TestConnectionResult::RateLimited,
        500..=599 => TestConnectionResult::ProviderError { status },
        _ => TestConnectionResult::Unknown {
            message: format!("HTTP {status}"),
        },
    })
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
}
