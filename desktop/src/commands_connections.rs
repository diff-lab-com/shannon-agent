//! Gateway social-connection commands — OS keyring credential storage +
//! read/write of `~/.shannon/gateway/config.json`.
//!
//! The gateway (`shannon-gateway`) reads each platform adapter's credentials
//! from the OS keyring at `start()` time, keyed `"<service>/<account>"`
//! (macOS `security find-generic-password`, Linux `secret-tool lookup`).
//! These commands let the desktop UI write those same entries via the Rust
//! `keyring` crate — same OS backend, so entries are compatible by
//! construction — and manage which adapters the gateway enables, persisting
//! the gateway's own config schema (camelCase, mirroring
//! `shannon-gateway/src/config/types.ts`) verbatim.
//!
//! Security: credentials are written straight to the OS keyring and never
//! touch the webview, the repo, or `config.json` — only the *keyring key
//! names* an adapter needs are recorded in config (`secrets` map). This is
//! the F14 contract the gateway already relies on.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::commands::AppState;
use crate::config::save_config;
use crate::gateway_supervisor::{GatewayProcessState, GatewaySupervisor, GatewaySupervisorStatus};

/// Default keyring `service` when a secret key has no `/` separator. Matches
/// `createCliKeyringProvider`'s default in the gateway.
const DEFAULT_SERVICE: &str = "shannon-gateway";

/// One gateway adapter entry. Serializes to the gateway's camelCase schema.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct GatewayAdapterConfig {
    pub platform: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<serde_json::Value>,
    /// Map of adapter-local secret name → OS-keyring key
    /// (e.g. `"botToken" → "slack/bot-token"`). The values are keyring *key
    /// names*, never the secrets themselves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secrets: Option<BTreeMap<String, String>>,
}

/// Engine connection block. `ws_url`/`http_base_url` target the Shannon
/// engine's `api_server` (loopback by default).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayEngineConfig {
    pub ws_url: String,
    pub http_base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Full gateway config — the on-disk shape of
/// `~/.shannon/gateway/config.json`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayConfig {
    pub engine: GatewayEngineConfig,
    pub adapters: Vec<GatewayAdapterConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    /// Inbound mobile `shannon/*` server (P1.3). None → mobile disabled in the
    /// gateway; the desktop's default config enables it so phones can pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mobile: Option<GatewayMobileConfig>,
}

/// Gateway mobile-server block. Mirrors `MobileGatewayConfig` in
/// `shannon-gateway/src/config/types.ts`. Paths point the gateway at the same
/// pairing files the desktop's pairing commands read/write (Design D channel).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayMobileConfig {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devices_file: Option<String>,
}

/// Split `"<service>/<account>"` into its parts. With no `/`, the whole key
/// is the account and `service` defaults to [`DEFAULT_SERVICE`] — identical
/// semantics to the gateway's `splitKey`. Pure; no I/O.
fn split_secret_key(key: &str) -> (&str, &str) {
    match key.find('/') {
        Some(idx) => (&key[..idx], &key[idx + 1..]),
        None => (DEFAULT_SERVICE, key),
    }
}

fn gateway_config_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "cannot resolve home directory".to_string())?;
    Ok(home.join(".shannon").join("gateway").join("config.json"))
}

/// A sane loopback default used when no gateway config exists yet. The user
/// can edit the engine URLs from the UI; this just gives the panel a starting
/// point so it never renders empty.
const CANONICAL_ENGINE_WS_URL: &str = "ws://127.0.0.1:33420/api/ws";
const CANONICAL_ENGINE_HTTP_BASE_URL: &str = "http://127.0.0.1:33420";

fn default_gateway_config() -> GatewayConfig {
    GatewayConfig {
        engine: GatewayEngineConfig {
            ws_url: CANONICAL_ENGINE_WS_URL.into(),
            http_base_url: CANONICAL_ENGINE_HTTP_BASE_URL.into(),
            model: None,
        },
        adapters: vec![],
        log_level: Some("info".into()),
        // Enable the inbound mobile shannon/* server so phones can pair (P1.3).
        mobile: Some(crate::commands_mobile_pairing::default_mobile_config()),
    }
}

/// Store a credential in the OS keyring under `"<service>/<account>"`.
#[tauri::command]
pub async fn gateway_set_secret(key: String, value: String) -> Result<(), String> {
    let (service, account) = split_secret_key(&key);
    let entry = keyring::Entry::new(service, account).map_err(err)?;
    entry.set_password(&value).map_err(err)
}

/// Retrieve a credential, or `None` if no entry exists. Never errors on a
/// missing entry (so the UI can probe presence without try/catch noise).
#[tauri::command]
pub async fn gateway_get_secret(key: String) -> Result<Option<String>, String> {
    let (service, account) = split_secret_key(&key);
    let entry = keyring::Entry::new(service, account).map_err(err)?;
    match entry.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(err(e)),
    }
}

/// Whether a keyring entry exists for `key`. Cheaper than fetching the value
/// and the only thing the UI needs to show a "configured" badge.
#[tauri::command]
pub async fn gateway_has_secret(key: String) -> Result<bool, String> {
    let (service, account) = split_secret_key(&key);
    let entry = keyring::Entry::new(service, account).map_err(err)?;
    match entry.get_password() {
        Ok(_) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(e) => Err(err(e)),
    }
}

/// Delete a keyring entry. Idempotent — a missing entry is success.
#[tauri::command]
pub async fn gateway_delete_secret(key: String) -> Result<(), String> {
    let (service, account) = split_secret_key(&key);
    let entry = keyring::Entry::new(service, account).map_err(err)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(err(e)),
    }
}

/// Read the gateway config. Returns a loopback default if no file exists yet
/// (first-run); errors only on a present-but-unparseable file.
#[tauri::command]
pub async fn gateway_read_config() -> Result<GatewayConfig, String> {
    let path = gateway_config_path()?;
    if !path.exists() {
        return Ok(default_gateway_config());
    }
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("gateway config: cannot read {path:?}: {e}"))?;
    let cfg: GatewayConfig = serde_json::from_str(&raw)
        .map_err(|e| format!("gateway config: invalid JSON in {path:?}: {e}"))?;
    Ok(cfg)
}

/// Validate + persist the gateway config. Writes atomically (temp file +
/// rename) so a crash mid-write can't leave a half-written config. Returns
/// the canonicalized config that was written.
fn write_gateway_config_atomic(config: &GatewayConfig) -> Result<(), String> {
    let path = gateway_config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("gateway config: cannot create {parent:?}: {e}"))?;
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("gateway config: serialize failed: {e}"))?;
    let tmp = NamedTempFile::new_in(path.parent().expect("config path has a parent"))
        .map_err(|e| format!("gateway config: cannot create temp file: {e}"))?;
    fs::write(tmp.path(), &json).map_err(|e| format!("gateway config: write failed: {e}"))?;
    tmp.persist(&path)
        .map_err(|e| format!("gateway config: persist failed: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn gateway_write_config(config: GatewayConfig) -> Result<GatewayConfig, String> {
    if config.engine.ws_url.trim().is_empty() {
        return Err("gateway config: engine.wsUrl must be a non-empty string".into());
    }
    if config.engine.http_base_url.trim().is_empty() {
        return Err("gateway config: engine.httpBaseUrl must be a non-empty string".into());
    }
    if let Some(model) = &config.engine.model {
        if model.trim().is_empty() {
            return Err(
                "gateway config: engine.model must be a non-empty string if present".into(),
            );
        }
    }
    for (i, adapter) in config.adapters.iter().enumerate() {
        if adapter.platform.trim().is_empty() {
            return Err(format!(
                "gateway config: adapters[{i}].platform must be a non-empty string"
            ));
        }
    }

    write_gateway_config_atomic(&config)?;
    Ok(config)
}

fn err(e: keyring::Error) -> String {
    format!("keyring: {e}")
}

// ── E-1: gateway process supervisor (方案 C) ──────────────────────────────
//
// These commands own the supervised gateway process via `AppState`. Lock
// order: always take `desktop_config` and release it BEFORE
// `gateway_supervisor` — never hold both at once.

/// Spawn (or no-op if already running) the local gateway under supervision.
/// Resolves the binary via `gateway.binary_path` → resource dir → `$PATH`;
/// reports `NotInstalled` when nothing resolves (no error thrown).
#[tauri::command]
pub async fn gateway_supervisor_start(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<GatewayProcessState, String> {
    let gw_cfg = state.desktop_config.read().await.gateway.clone();
    let mut guard = state.gateway_supervisor.lock().await;
    if let Some(sup) = guard.as_ref() {
        match sup.status() {
            GatewaySupervisorStatus::Running { .. }
            | GatewaySupervisorStatus::ManagedExternally { .. } => {
                return Ok(GatewayProcessState {
                    managed: gw_cfg.managed,
                    status: sup.status(),
                });
            }
            _ => {}
        }
    }
    let sup = GatewaySupervisor::start(&app, &gw_cfg);
    let status = sup.status();
    *guard = Some(sup);
    Ok(GatewayProcessState {
        managed: gw_cfg.managed,
        status,
    })
}

/// Gracefully stop the supervised gateway. Idempotent.
#[tauri::command]
pub async fn gateway_supervisor_stop(
    state: tauri::State<'_, AppState>,
) -> Result<GatewayProcessState, String> {
    let managed = state.desktop_config.read().await.gateway.managed;
    let mut guard = state.gateway_supervisor.lock().await;
    if let Some(mut sup) = guard.take() {
        sup.stop().await;
    }
    Ok(GatewayProcessState {
        managed,
        status: GatewaySupervisorStatus::Stopped,
    })
}

/// Snapshot of `managed` + the process status (Stopped when no supervisor
/// exists yet).
#[tauri::command]
pub async fn gateway_supervisor_status(
    state: tauri::State<'_, AppState>,
) -> Result<GatewayProcessState, String> {
    let managed = state.desktop_config.read().await.gateway.managed;
    let guard = state.gateway_supervisor.lock().await;
    let status = guard
        .as_ref()
        .map(|s| s.status())
        .unwrap_or(GatewaySupervisorStatus::Stopped);
    Ok(GatewayProcessState { managed, status })
}

/// Persist the 方案 C `managed` flag. Toggling it **off** also stops a running
/// supervised gateway (desktop no longer owns its lifecycle). Toggling it on
/// does **not** auto-start — the user clicks Start, or the next app launch
/// auto-starts via `setup()`.
#[tauri::command]
pub async fn gateway_set_managed(
    state: tauri::State<'_, AppState>,
    managed: bool,
) -> Result<GatewayProcessState, String> {
    {
        let mut cfg = state.desktop_config.write().await;
        cfg.gateway.managed = managed;
    }
    let snapshot = state.desktop_config.read().await.clone();
    save_config(&snapshot).map_err(|e| format!("gateway config: save failed: {e}"))?;
    if !managed {
        let mut guard = state.gateway_supervisor.lock().await;
        if let Some(mut sup) = guard.take() {
            sup.stop().await;
        }
    }
    Ok(GatewayProcessState {
        managed,
        status: GatewaySupervisorStatus::Stopped,
    })
}

/// Auto-start the supervised gateway at app launch when `gateway.managed` is on
/// and no supervisor is already running (E-1 方案 C). Kept as a lib-side helper
/// so `main.rs::setup()` never touches `AppState`'s private fields directly.
pub async fn bootstrap_gateway_supervisor(
    state: &tauri::State<'_, AppState>,
    app: &tauri::AppHandle,
) {
    let mut gw_cfg = state.desktop_config.read().await.gateway.clone();
    if !gw_cfg.managed {
        return;
    }

    let mut gateway_config = match gateway_read_config().await {
        Ok(config) => config,
        Err(error) => {
            tracing::error!("managed gateway config read failed: {error}");
            return;
        }
    };
    gateway_config.engine.ws_url = CANONICAL_ENGINE_WS_URL.into();
    gateway_config.engine.http_base_url = CANONICAL_ENGINE_HTTP_BASE_URL.into();
    if let Err(error) = write_gateway_config_atomic(&gateway_config) {
        tracing::error!("managed gateway config bootstrap failed: {error}");
        return;
    }

    let config_path = match gateway_config_path() {
        Ok(path) => path,
        Err(error) => {
            tracing::error!("managed gateway config path resolution failed: {error}");
            return;
        }
    };
    if !gw_cfg.extra_args.iter().any(|arg| arg == "--config") {
        gw_cfg.extra_args.push("--config".into());
        gw_cfg
            .extra_args
            .push(config_path.to_string_lossy().into_owned());
    }

    let mut guard = state.gateway_supervisor.lock().await;
    let already_running = guard
        .as_ref()
        .map(|supervisor| matches!(supervisor.status(), GatewaySupervisorStatus::Running { .. }))
        .unwrap_or(false);
    if already_running {
        return;
    }

    // Q4-B: if a user-level OS service is already running the gateway,
    // treat it as authoritative — do not spawn a competing child.
    let service_state = crate::gateway_service_probe::query_gateway_service_state().await;
    if service_state == crate::gateway_service_probe::ServiceState::Active {
        tracing::info!(
            "gateway already running as OS service — desktop will not spawn a competing child"
        );
        let supervisor = GatewaySupervisor::managed_externally("shannon-gateway.service");
        *guard = Some(supervisor);
        return;
    }

    let supervisor = GatewaySupervisor::start(app, &gw_cfg);
    let status = supervisor.status();
    *guard = Some(supervisor);
    tracing::info!("gateway supervisor auto-started: {status:?}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_key_with_slash_uses_both_parts() {
        assert_eq!(split_secret_key("slack/bot-token"), ("slack", "bot-token"));
        assert_eq!(split_secret_key("slack/a/b"), ("slack", "a/b"));
    }

    #[test]
    fn split_key_without_slash_defaults_service() {
        assert_eq!(
            split_secret_key("bot-token"),
            (DEFAULT_SERVICE, "bot-token")
        );
    }

    #[test]
    fn config_serializes_to_gateway_camel_case() {
        let cfg = GatewayConfig {
            engine: GatewayEngineConfig {
                ws_url: "ws://h/ws".into(),
                http_base_url: "http://h".into(),
                model: Some("claude-sonnet-4-6".into()),
            },
            adapters: vec![GatewayAdapterConfig {
                platform: "slack".into(),
                enabled: true,
                options: None,
                secrets: Some(BTreeMap::from([(
                    "botToken".into(),
                    "slack/bot-token".into(),
                )])),
            }],
            log_level: Some("info".into()),
            mobile: None,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        // camelCase wire shape — matches shannon-gateway/src/config/types.ts
        assert!(json.contains("\"wsUrl\""));
        assert!(json.contains("\"httpBaseUrl\""));
        assert!(json.contains("\"logLevel\""));
        assert!(json.contains("\"botToken\""));
        // round-trips
        let back: GatewayConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn config_parses_gateway_native_json() {
        // Exactly the shape the gateway itself writes/expects.
        let raw = r#"{
            "engine": { "wsUrl": "ws://127.0.0.1:33420/api/ws", "httpBaseUrl": "http://127.0.0.1:33420" },
            "adapters": [
                { "platform": "telegram", "enabled": true, "secrets": { "botToken": "telegram/bot-token" } }
            ],
            "logLevel": "info"
        }"#;
        let cfg: GatewayConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(cfg.engine.ws_url, "ws://127.0.0.1:33420/api/ws");
        assert_eq!(cfg.adapters.len(), 1);
        assert_eq!(cfg.adapters[0].platform, "telegram");
        assert_eq!(cfg.log_level.as_deref(), Some("info"));
    }

    #[test]
    fn default_config_targets_loopback() {
        let c = default_gateway_config();
        assert!(c.engine.ws_url.starts_with("ws://127.0.0.1"));
        assert!(c.engine.http_base_url.starts_with("http://127.0.0.1"));
        assert!(c.adapters.is_empty());
    }
}
