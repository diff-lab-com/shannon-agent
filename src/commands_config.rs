//! Configuration commands — configure, switch_provider, get_config.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::commands::AppState;
use crate::config::{self, DesktopConfig, ProviderConnection, ProvidersFile};
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
        skill_detection_enabled: existing.skill_detection_enabled,
        notifications_master_enabled: existing.notifications_master_enabled,
        notifications_dnd_enabled: existing.notifications_dnd_enabled,
        notifications_dnd_start: existing.notifications_dnd_start.clone(),
        notifications_dnd_end: existing.notifications_dnd_end.clone(),
        notifications_on_completed: existing.notifications_on_completed,
        notifications_on_failed: existing.notifications_on_failed,
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
fn validate_base_url(raw: &str) -> Result<String, String> {
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

/// Resolve the list-models probe URL + `Name: value` auth header for a provider
/// kind and an optional base_url override. Pure (no network) so it is
/// unit-testable. Ollama is handled by the caller because it uses no auth and
/// an env-derived default host.
///
/// The optional `base_url` lets a user point a built-in kind at a proxy or
/// self-host, and is **required** for `openai-compatible` (GLM/Zhipu,
/// Moonshot/Kimi, MiniMax, Together, Groq, …), which closes the gap where
/// those providers previously fell through to "unknown provider".
fn provider_probe_url(
    provider: &str,
    api_key: &str,
    base_url: Option<&str>,
) -> Result<(String, Option<String>), String> {
    let validated = match base_url {
        Some(raw) => Some(validate_base_url(raw)?),
        None => None,
    };
    let trimmed = validated.as_deref();
    Ok(match provider {
        "anthropic" => {
            let base = trimmed.unwrap_or("https://api.anthropic.com");
            (
                format!("{base}/v1/models?limit=1"),
                Some(format!("x-api-key: {api_key}")),
            )
        }
        "openai" => {
            let base = trimmed.unwrap_or("https://api.openai.com");
            (
                format!("{base}/v1/models"),
                Some(format!("Authorization: Bearer {api_key}")),
            )
        }
        "deepseek" => {
            let base = trimmed.unwrap_or("https://api.deepseek.com");
            (
                format!("{base}/models"),
                Some(format!("Authorization: Bearer {api_key}")),
            )
        }
        "openai-compatible" => {
            let base = trimmed
                .ok_or_else(|| "openai-compatible provider requires a base_url".to_string())?;
            (
                format!("{base}/models"),
                Some(format!("Authorization: Bearer {api_key}")),
            )
        }
        other => return Err(format!("unknown provider: {other}")),
    })
}

/// Ping a provider's "list models" endpoint to verify the API key works.
///
/// Each provider has a cheap GET endpoint that requires auth — we use it as
/// a liveness check. 200 → Success, 401/403 → InvalidKey, 429 → RateLimited,
/// 5xx → ProviderError, network failure → NetworkUnreachable, everything
/// else → Unknown. An optional `base_url` overrides the canonical endpoint
/// (required for `openai-compatible` providers).
#[tauri::command]
pub async fn test_provider_connection(
    provider: String,
    api_key: String,
    base_url: Option<String>,
) -> Result<TestConnectionResult, String> {
    // Ollama needs no auth and uses a bespoke tags endpoint whose default host
    // comes from OLLAMA_HOST (or localhost:11434).
    if provider == "ollama" {
        let host = match base_url.as_deref() {
            Some(raw) => validate_base_url(raw)?,
            None => std::env::var("OLLAMA_HOST")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
        };
        return ping_provider(&format!("{host}/api/tags"), None)
            .await
            .map_err(|e| e.to_string());
    }

    let (url, auth_header) = provider_probe_url(&provider, &api_key, base_url.as_deref())?;
    ping_provider(&url, auth_header.as_deref())
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
}

fn is_known_kind(kind: &str) -> bool {
    matches!(
        kind,
        "anthropic" | "openai" | "deepseek" | "ollama" | "openai-compatible"
    )
}

fn kind_label(kind: &str) -> String {
    match kind {
        "anthropic" => "Anthropic".to_string(),
        "openai" => "OpenAI".to_string(),
        "deepseek" => "DeepSeek".to_string(),
        "ollama" => "Ollama".to_string(),
        "openai-compatible" => "OpenAI-compatible".to_string(),
        other => other.to_string(),
    }
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
fn apply_provider_update(
    conn: &mut ProviderConnection,
    input: &ProviderInput,
    base_url: Option<String>,
) {
    conn.label = input.label.clone();
    conn.provider_kind = input.provider_kind.clone();
    match input.api_key.as_deref() {
        Some(k) if !k.is_empty() && k != "***" => conn.api_key = Some(k.to_string()),
        _ => {}
    }
    conn.base_url = base_url;
    conn.model = input.model.clone().filter(|s| !s.is_empty());
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

/// Mirror a managed provider's fields into the singular `DesktopConfig` that
/// the engine reads. Kept separate so the active-selection logic is testable
/// without a Tauri runtime.
fn mirror_provider_into_config(dc: &mut DesktopConfig, conn: &ProviderConnection) {
    dc.provider = Some(conn.provider_kind.clone());
    dc.api_key = conn.api_key.clone();
    dc.base_url = conn.base_url.clone();
    dc.model = conn.model.clone();
}

/// Build the first-run seeded `ProvidersFile` from the legacy singular config.
/// Returns `None` when no provider was configured to seed from.
fn build_seed_file(cfg: &DesktopConfig) -> Option<ProvidersFile> {
    let kind = cfg
        .provider
        .as_deref()
        .filter(|k| !k.is_empty())?
        .to_string();
    let id = unique_provider_slug(&kind, &[]);
    Some(ProvidersFile {
        active_provider_id: Some(id.clone()),
        providers: vec![ProviderConnection {
            id,
            label: kind_label(&kind),
            provider_kind: kind,
            api_key: cfg.api_key.clone(),
            base_url: cfg.base_url.clone(),
            model: cfg.model.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        }],
    })
}

/// Build a single-entry `ProvidersFile` from the legacy singular config and
/// persist it, so existing users see their current connection on first load.
/// Returns `None` when there is no configured provider to seed from.
async fn seed_from_legacy_config(
    state: &tauri::State<'_, AppState>,
) -> Result<Option<ProvidersFile>, String> {
    let cfg = state.desktop_config.read().await;
    let Some(file) = build_seed_file(&cfg) else {
        return Ok(None);
    };
    config::save_providers(&file)?;
    Ok(Some(file))
}

/// List all managed providers, masking API keys. On first call, lazily migrates
/// the legacy singular config into a single seeded entry so existing users see
/// their current connection rather than an empty list.
#[tauri::command]
pub async fn list_providers(state: tauri::State<'_, AppState>) -> Result<ProvidersFile, String> {
    if !config::providers_path().exists() {
        if let Some(seeded) = seed_from_legacy_config(&state).await? {
            return Ok(mask_providers(seeded));
        }
    }
    Ok(mask_providers(config::load_providers()))
}

/// Insert or update a managed provider. Returns the updated (masked) file.
#[tauri::command]
pub async fn save_provider(
    app_handle: tauri::AppHandle,
    input: ProviderInput,
) -> Result<ProvidersFile, String> {
    if !is_known_kind(&input.provider_kind) {
        return Err(format!("unknown provider kind: {}", input.provider_kind));
    }
    let base_url = resolve_base_url(&input.base_url)?;
    let mut file = config::load_providers();

    if let Some(id) = input.id.as_deref() {
        let conn = file
            .providers
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("provider not found: {id}"))?;
        apply_provider_update(conn, &input, base_url);
    } else {
        let id = unique_provider_slug(&input.label, &file.providers);
        let conn = ProviderConnection {
            id,
            label: input.label.clone(),
            provider_kind: input.provider_kind.clone(),
            api_key: input.api_key.filter(|s| !s.is_empty()),
            base_url,
            model: input.model.filter(|s| !s.is_empty()),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        file.providers.push(conn);
    }

    config::save_providers(&file)?;
    emit_providers_changed(&app_handle, &file);
    Ok(mask_providers(file))
}

/// Delete a managed provider by id. Clears `active_provider_id` if it pointed
/// at the deleted entry. Returns the updated (masked) file.
#[tauri::command]
pub async fn delete_provider(
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<ProvidersFile, String> {
    let file = remove_provider(config::load_providers(), &id)?;
    config::save_providers(&file)?;
    emit_providers_changed(&app_handle, &file);
    Ok(mask_providers(file))
}

/// Activate a managed provider: mirrors its fields into the singular
/// `DesktopConfig` that the engine reads, rebuilds the client config, and
/// persists both stores. Emits `CONFIG_UPDATED` so the tray and any open
/// windows refresh their provider label.
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

    // Mirror into the singular config the engine consumes.
    let desktop_cfg = {
        let mut dc = state.desktop_config.write().await;
        mirror_provider_into_config(&mut dc, &conn);
        dc.clone()
    };

    let client_config = AppState::build_client_config(&desktop_cfg);
    {
        let mut c = state.client_config.write().await;
        *c = client_config;
    }
    {
        let mut m = state.model.lock().await;
        *m = model.clone().unwrap_or_default();
    }
    {
        let mut p = state.provider.lock().await;
        *p = provider_kind.clone();
    }

    config::save_config(&desktop_cfg)?;

    file.active_provider_id = Some(id);
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
    fn probe_url_uses_canonical_endpoints_by_default() {
        let (url, auth) = provider_probe_url("anthropic", "sk-1", None).unwrap();
        assert_eq!(url, "https://api.anthropic.com/v1/models?limit=1");
        assert_eq!(auth.as_deref(), Some("x-api-key: sk-1"));

        let (url, auth) = provider_probe_url("openai", "sk-2", None).unwrap();
        assert_eq!(url, "https://api.openai.com/v1/models");
        assert_eq!(auth.as_deref(), Some("Authorization: Bearer sk-2"));

        let (url, _) = provider_probe_url("deepseek", "sk-3", None).unwrap();
        assert_eq!(url, "https://api.deepseek.com/models");
    }

    #[test]
    fn probe_url_respects_base_url_override() {
        // Anthropic behind a proxy.
        let (url, auth) =
            provider_probe_url("anthropic", "sk-1", Some("https://my-proxy.example.com/")).unwrap();
        assert_eq!(url, "https://my-proxy.example.com/v1/models?limit=1");
        assert_eq!(auth.as_deref(), Some("x-api-key: sk-1"));
    }

    #[test]
    fn probe_url_openai_compatible_requires_base_url() {
        let err = provider_probe_url("openai-compatible", "sk-x", None).unwrap_err();
        assert!(err.contains("base_url"), "unexpected error: {err}");

        let (url, auth) = provider_probe_url(
            "openai-compatible",
            "sk-x",
            Some("https://open.bigmodel.cn/api/paas/v4"),
        )
        .unwrap();
        assert_eq!(url, "https://open.bigmodel.cn/api/paas/v4/models");
        assert_eq!(auth.as_deref(), Some("Authorization: Bearer sk-x"));
    }

    #[test]
    fn probe_url_rejects_unknown_provider() {
        assert!(provider_probe_url("grok", "sk", None).is_err());
    }

    #[test]
    fn probe_url_rejects_unsafe_base_url() {
        // Validation now runs inside provider_probe_url.
        assert!(provider_probe_url("openai", "sk", Some("file:///etc/passwd")).is_err());
        assert!(
            provider_probe_url(
                "openai-compatible",
                "sk",
                Some("https://user:pass@evil.example.com")
            )
            .is_err()
        );
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
                },
                ProviderConnection {
                    id: "b".into(),
                    label: "B".into(),
                    provider_kind: "ollama".into(),
                    api_key: None,
                    base_url: None,
                    model: None,
                    created_at: "2026-06-27T00:00:00Z".into(),
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

    #[test]
    fn kind_label_is_human_readable() {
        assert_eq!(kind_label("anthropic"), "Anthropic");
        assert_eq!(kind_label("openai-compatible"), "OpenAI-compatible");
        assert_eq!(kind_label("custom"), "custom");
    }

    // === Models P2 command-level logic (helpers extracted from the commands) ===

    fn sample_conn(id: &str, kind: &str, key: Option<&str>) -> ProviderConnection {
        ProviderConnection {
            id: id.into(),
            label: id.into(),
            provider_kind: kind.into(),
            api_key: key.map(str::to_string),
            base_url: None,
            model: None,
            created_at: "2026-06-28T00:00:00Z".into(),
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
        }
    }

    #[test]
    fn apply_provider_update_preserves_key_when_masked_or_absent() {
        // start with a stored secret
        let mut conn = sample_conn("anthropic", "anthropic", Some("sk-real"));

        // masked "***" => keep existing
        apply_provider_update(
            &mut conn,
            &provider_input(Some("anthropic"), "Anthropic", "anthropic", Some("***")),
            None,
        );
        assert_eq!(conn.api_key.as_deref(), Some("sk-real"));
        assert_eq!(conn.label, "Anthropic");

        // absent => keep existing
        apply_provider_update(
            &mut conn,
            &provider_input(Some("anthropic"), "Anthropic", "anthropic", None),
            None,
        );
        assert_eq!(conn.api_key.as_deref(), Some("sk-real"));

        // empty string => keep existing
        apply_provider_update(
            &mut conn,
            &provider_input(Some("anthropic"), "Anthropic", "anthropic", Some("")),
            None,
        );
        assert_eq!(conn.api_key.as_deref(), Some("sk-real"));

        // fresh value => replaced
        apply_provider_update(
            &mut conn,
            &provider_input(Some("anthropic"), "Anthropic", "anthropic", Some("sk-new")),
            None,
        );
        assert_eq!(conn.api_key.as_deref(), Some("sk-new"));
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

    #[test]
    fn mirror_provider_into_config_copies_all_singular_fields() {
        let mut dc = DesktopConfig::default();
        let conn = ProviderConnection {
            id: "glm".into(),
            label: "My GLM".into(),
            provider_kind: "openai-compatible".into(),
            api_key: Some("sk-glm".into()),
            base_url: Some("https://open.bigmodel.cn/api/paas/v4".into()),
            model: Some("glm-4.6".into()),
            created_at: "2026-06-28T00:00:00Z".into(),
        };
        mirror_provider_into_config(&mut dc, &conn);
        assert_eq!(dc.provider.as_deref(), Some("openai-compatible"));
        assert_eq!(dc.api_key.as_deref(), Some("sk-glm"));
        assert_eq!(
            dc.base_url.as_deref(),
            Some("https://open.bigmodel.cn/api/paas/v4")
        );
        assert_eq!(dc.model.as_deref(), Some("glm-4.6"));
    }

    #[test]
    fn build_seed_file_returns_none_without_provider() {
        let cfg = DesktopConfig {
            provider: None,
            ..Default::default()
        };
        assert!(build_seed_file(&cfg).is_none());
        let cfg = DesktopConfig {
            provider: Some(String::new()), // empty => treated as unset
            ..Default::default()
        };
        assert!(build_seed_file(&cfg).is_none());
    }

    #[test]
    fn build_seed_file_mirrors_legacy_singular_config() {
        let cfg = DesktopConfig {
            provider: Some("anthropic".into()),
            api_key: Some("sk-legacy".into()),
            model: Some("claude-sonnet-4-6".into()),
            ..Default::default()
        };
        let file = build_seed_file(&cfg).unwrap();
        assert_eq!(file.providers.len(), 1);
        // the active pointer names the sole seeded entry
        assert_eq!(
            file.active_provider_id.as_deref(),
            Some(file.providers[0].id.as_str())
        );
        let conn = &file.providers[0];
        assert_eq!(conn.provider_kind, "anthropic");
        assert_eq!(conn.label, "Anthropic"); // kind_label
        assert_eq!(conn.api_key.as_deref(), Some("sk-legacy"));
        assert_eq!(conn.model.as_deref(), Some("claude-sonnet-4-6"));
    }
}
