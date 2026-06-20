//! Notification commands — native OS notifications, webhook config, inbound listener.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//!
//! `fire_query_notification`, `NotificationKind`, and
//! `load_desktop_webhook_config` are `pub(crate)` because `send_message` (still
//! in `commands.rs`) and `AppState::new` call them across the module boundary.

use crate::commands::AppState;
use serde::{Deserialize, Serialize};

// === Payloads / DTOs =====================================================

/// Payload for `send_notification`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NotificationPayload {
    pub title: String,
    pub body: String,
    /// Reserved for future use. Current tauri-plugin-notification v2 API
    /// surface on the desktop's pinned shannon-core rev does not expose
    /// per-level icon mapping; level is currently informational only.
    #[serde(default)]
    pub level: Option<String>,
}

/// Serializable webhook config mirror — keeps the TS boundary clean and
/// avoids leaking `WebhookTemplate`'s serde-tagged enum directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfigDto {
    pub url: String,
    pub template: String,
    pub secret: Option<String>,
    pub timeout_ms: u64,
    pub include_body: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlackInboundDto {
    pub bot_token: String,
    pub trigger_word: String,
    pub allowed_channels: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelegramInboundDto {
    pub bot_token: String,
    pub trigger_word: String,
    pub allowed_chats: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InboundConfigDto {
    pub slack: Option<SlackInboundDto>,
    pub telegram: Option<TelegramInboundDto>,
}

// === Commands ============================================================

/// Fire a native OS notification via `tauri-plugin-notification`.
///
/// The desktop's pinned `shannon-core` rev (`a19a15d` = v0.5.2) exposes the
/// P1 notifications orchestrator. The frontend "Test notification" button
/// invokes this command directly; query-event firing sites
/// (`fire_query_notification` below) go through the shared `Notifier` on
/// `AppState` so they benefit from cooldown + level filtering.
#[tauri::command]
pub async fn send_notification(
    app: tauri::AppHandle,
    payload: NotificationPayload,
) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;

    app.notification()
        .builder()
        .title(payload.title)
        .body(payload.body)
        .show()
        .map_err(|e| format!("notification show failed: {e}"))
}

#[tauri::command]
pub async fn get_webhook_config() -> Result<Option<WebhookConfigDto>, String> {
    let cfg = load_desktop_webhook_config();
    Ok(cfg.map(|c| WebhookConfigDto {
        url: c.url,
        template: template_to_str(&c.template),
        secret: c.secret,
        timeout_ms: c.timeout_ms,
        include_body: c.include_body,
    }))
}

#[tauri::command]
pub async fn save_webhook_config(dto: WebhookConfigDto) -> Result<(), String> {
    save_webhook_config_to_disk(&dto)
}

#[tauri::command]
pub async fn clear_webhook_config() -> Result<(), String> {
    clear_webhook_config_on_disk()
}

#[tauri::command]
pub async fn get_inbound_config() -> Result<InboundConfigDto, String> {
    let path = resolve_webhook_config_path()?;
    if !path.exists() {
        return Ok(InboundConfigDto::default());
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let Ok(root) = toml::from_str::<toml::Value>(&existing) else {
        return Ok(InboundConfigDto::default());
    };
    let Some(notif) = root.get("notifications").and_then(|v| v.as_table()) else {
        return Ok(InboundConfigDto::default());
    };
    let Some(inbound) = notif.get("inbound").and_then(|v| v.as_table()) else {
        return Ok(InboundConfigDto::default());
    };
    let slack = inbound
        .get("slack")
        .and_then(|v| v.as_table())
        .map(|t| SlackInboundDto {
            bot_token: t
                .get("bot_token")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            trigger_word: t
                .get("trigger_word")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            allowed_channels: t
                .get("allowed_channels")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        });
    let telegram = inbound
        .get("telegram")
        .and_then(|v| v.as_table())
        .map(|t| TelegramInboundDto {
            bot_token: t
                .get("bot_token")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            trigger_word: t
                .get("trigger_word")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            allowed_chats: t
                .get("allowed_chats")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        });
    Ok(InboundConfigDto { slack, telegram })
}

#[tauri::command]
pub async fn save_inbound_config(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    dto: InboundConfigDto,
) -> Result<(), String> {
    let path = resolve_webhook_config_path()?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut root: toml::Value =
        toml::from_str(&existing).unwrap_or(toml::Value::Table(toml::value::Table::new()));
    let table = root
        .as_table_mut()
        .ok_or_else(|| "config root is not a table".to_string())?;
    let notifications = table
        .entry("notifications")
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let notif_table = notifications
        .as_table_mut()
        .ok_or_else(|| "notifications is not a table".to_string())?;
    let mut inbound = toml::value::Table::new();
    if let Some(s) = &dto.slack {
        let mut t = toml::value::Table::new();
        t.insert("bot_token".into(), toml::Value::String(s.bot_token.clone()));
        t.insert(
            "trigger_word".into(),
            toml::Value::String(s.trigger_word.clone()),
        );
        t.insert(
            "allowed_channels".into(),
            toml::Value::Array(
                s.allowed_channels
                    .iter()
                    .map(|c| toml::Value::String(c.clone()))
                    .collect(),
            ),
        );
        inbound.insert("slack".into(), toml::Value::Table(t));
    }
    if let Some(tg) = &dto.telegram {
        let mut t = toml::value::Table::new();
        t.insert(
            "bot_token".into(),
            toml::Value::String(tg.bot_token.clone()),
        );
        t.insert(
            "trigger_word".into(),
            toml::Value::String(tg.trigger_word.clone()),
        );
        t.insert(
            "allowed_chats".into(),
            toml::Value::Array(
                tg.allowed_chats
                    .iter()
                    .map(|c| toml::Value::String(c.clone()))
                    .collect(),
            ),
        );
        inbound.insert("telegram".into(), toml::Value::Table(t));
    }
    notif_table.insert("inbound".into(), toml::Value::Table(inbound));
    let serialized = toml::to_string_pretty(&root).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, serialized).map_err(|e| format!("write {}: {e}", path.display()))?;
    tracing::info!(path = %path.display(), "inbound config saved");
    restart_inbound_listener(&state, &app_handle, &dto).await;
    Ok(())
}

#[tauri::command]
pub async fn clear_inbound_config(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let path = resolve_webhook_config_path()?;
    if !path.exists() {
        let mut listener = state.inbound_listener.lock().await;
        if let Some(h) = listener.as_mut() {
            h.stop().await;
        }
        *listener = None;
        return Ok(());
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let Ok(mut root) = toml::from_str::<toml::Value>(&existing) else {
        return Ok(());
    };
    if let Some(table) = root.as_table_mut() {
        if let Some(notif) = table
            .get_mut("notifications")
            .and_then(|v| v.as_table_mut())
        {
            notif.remove("inbound");
        }
    }
    let serialized = toml::to_string_pretty(&root).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, serialized).map_err(|e| format!("write {}: {e}", path.display()))?;
    tracing::info!(path = %path.display(), "inbound config cleared");
    let mut listener = state.inbound_listener.lock().await;
    if let Some(h) = listener.as_mut() {
        h.stop().await;
    }
    *listener = None;
    Ok(())
}

#[tauri::command]
pub async fn get_inbound_listener_status(
    state: tauri::State<'_, AppState>,
) -> Result<crate::inbound::InboundListenerStatus, String> {
    let listener = state.inbound_listener.lock().await;
    Ok(listener.as_ref().map(|h| h.status()).unwrap_or_default())
}

#[tauri::command]
pub async fn stop_inbound_listener(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut listener = state.inbound_listener.lock().await;
    if let Some(h) = listener.as_mut() {
        h.stop().await;
    }
    *listener = None;
    Ok(())
}

// === Helpers ==============================================================

/// Query-event notification kinds used by `fire_query_notification`.
pub(crate) enum NotificationKind {
    Completed,
    Failed(String),
}

/// Fire a cooldown-aware notification for a query lifecycle event.
///
/// `Completed` uses `source = "query_complete"` with a 0ms window (always
/// fires — each query completion is worth surfacing). `Failed` uses
/// `source = "query_error"` with a 5000ms window to coalesce cascading
/// errors (e.g. repeated API timeouts within a retry storm).
///
/// Returns whether the notification was actually dispatched (`Ok(false)`
/// means suppressed by cooldown). Production callers discard the result.
pub(crate) fn fire_query_notification(
    notifier: &shannon_core::notifier::Notifier,
    kind: NotificationKind,
) -> Result<bool, shannon_core::notifier::NotifierError> {
    use chrono::Utc;
    use shannon_core::notifier::{Notification, NotificationLevel};

    let (title, body, level, source, window_ms) = match kind {
        NotificationKind::Completed => (
            "Shannon".to_string(),
            "Query complete".to_string(),
            NotificationLevel::Info,
            "query_complete".to_string(),
            0_u64,
        ),
        NotificationKind::Failed(err) => {
            let body = if err.chars().count() > 200 {
                let truncated: String = err.chars().take(197).collect();
                format!("{truncated}...")
            } else {
                err
            };
            (
                "Shannon — query failed".to_string(),
                body,
                NotificationLevel::Error,
                "query_error".to_string(),
                5_000_u64,
            )
        }
    };

    let notification = Notification {
        title,
        body,
        level,
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        source: Some(source),
        action_id: None,
    };

    notifier.notify_dedup(&notification, window_ms)
}

/// Best-effort load of `[notifications.webhook]` from `~/.shannon/config.toml`
/// and `.shannon.toml` (project-local). Returns `None` on any error — never
/// panics the app on config issues.
pub(crate) fn load_desktop_webhook_config() -> Option<shannon_core::notifier::WebhookConfig> {
    let cfg = shannon_core::unified_config::ConfigBuilder::new()
        .load_global_toml()
        .load_local_toml()
        .build();
    cfg.notifications.and_then(|n| n.webhook)
}

fn template_to_str(t: &shannon_core::notifier::WebhookTemplate) -> String {
    use shannon_core::notifier::WebhookTemplate::*;
    match t {
        Slack => "slack".into(),
        Discord => "discord".into(),
        Feishu => "feishu".into(),
        Wechat => "wechat".into(),
        Teams => "teams".into(),
        Telegram => "telegram".into(),
        DingTalk => "dingtalk".into(),
        Raw => "raw".into(),
        Custom(s) => format!("custom:{s}"),
    }
}

fn template_from_str(s: &str) -> shannon_core::notifier::WebhookTemplate {
    use shannon_core::notifier::WebhookTemplate::*;
    match s {
        "slack" => Slack,
        "discord" => Discord,
        "feishu" => Feishu,
        "wechat" => Wechat,
        "teams" => Teams,
        "telegram" => Telegram,
        "dingtalk" => DingTalk,
        "raw" => Raw,
        other => {
            if let Some(custom) = other.strip_prefix("custom:") {
                Custom(custom.to_string())
            } else {
                Raw
            }
        }
    }
}

/// Resolve the config file path used for the webhook section. Prefers the
/// project-local `.shannon.toml` if it exists; otherwise uses the global
/// `~/.shannon/config.toml` (creating the parent directory if missing).
fn resolve_webhook_config_path() -> Result<std::path::PathBuf, String> {
    let local = std::path::Path::new(".shannon.toml");
    if local.exists() {
        return Ok(local.to_path_buf());
    }
    let home = dirs::home_dir().ok_or_else(|| "could not resolve $HOME".to_string())?;
    let dir = home.join(".shannon");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    Ok(dir.join("config.toml"))
}

/// Read-modify-write the `[notifications.webhook]` table on disk. Preserves
/// all other top-level tables and keys. Comments and formatting are lost
/// (TOML round-trip via `toml::Value`), which is acceptable for a settings
/// UI write path.
fn save_webhook_config_to_disk(dto: &WebhookConfigDto) -> Result<(), String> {
    let path = resolve_webhook_config_path()?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut root: toml::Value =
        toml::from_str(&existing).unwrap_or(toml::Value::Table(toml::value::Table::new()));

    let table = root.as_table_mut().ok_or_else(|| {
        "config root is not a table — refusing to overwrite user config".to_string()
    })?;

    let template_val = match template_from_str(&dto.template) {
        shannon_core::notifier::WebhookTemplate::Custom(s) => toml::Value::Table({
            let mut t = toml::value::Table::new();
            t.insert("custom".into(), toml::Value::String(s));
            t
        }),
        t => toml::Value::String(template_to_str(&t)),
    };

    let mut wh = toml::value::Table::new();
    wh.insert("url".into(), toml::Value::String(dto.url.clone()));
    if let Some(s) = &dto.secret {
        wh.insert("secret".into(), toml::Value::String(s.clone()));
    }
    wh.insert("template".into(), template_val);
    wh.insert(
        "timeout_ms".into(),
        toml::Value::Integer(dto.timeout_ms as i64),
    );
    wh.insert(
        "include_body".into(),
        toml::Value::Boolean(dto.include_body),
    );

    let notifications = table
        .entry("notifications")
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let notif_table = notifications
        .as_table_mut()
        .ok_or_else(|| "notifications is not a table".to_string())?;
    notif_table.insert("webhook".into(), toml::Value::Table(wh));

    let serialized = toml::to_string_pretty(&root).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, serialized).map_err(|e| format!("write {}: {e}", path.display()))?;
    tracing::info!(path = %path.display(), "webhook config saved");
    Ok(())
}

fn clear_webhook_config_on_disk() -> Result<(), String> {
    let path = resolve_webhook_config_path()?;
    if !path.exists() {
        return Ok(());
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let Ok(mut root) = toml::from_str::<toml::Value>(&existing) else {
        return Ok(());
    };
    if let Some(table) = root.as_table_mut() {
        if let Some(notif) = table
            .get_mut("notifications")
            .and_then(|v| v.as_table_mut())
        {
            notif.remove("webhook");
        }
    }
    let serialized = toml::to_string_pretty(&root).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, serialized).map_err(|e| format!("write {}: {e}", path.display()))?;
    tracing::info!(path = %path.display(), "webhook config cleared");
    Ok(())
}

/// (Re)spawn inbound workers to match `dto`. Stops any existing workers
/// first so callers can mutate config and observe the listener reflect it.
async fn restart_inbound_listener(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    dto: &InboundConfigDto,
) {
    let mut listener = state.inbound_listener.lock().await;
    if let Some(h) = listener.as_mut() {
        h.stop().await;
    }
    let slack = dto.slack.as_ref().map(|s| crate::inbound::SlackConfig {
        bot_token: s.bot_token.clone(),
        trigger_word: s.trigger_word.clone(),
        allowed_channels: s.allowed_channels.clone(),
    });
    let telegram = dto
        .telegram
        .as_ref()
        .map(|t| crate::inbound::TelegramConfig {
            bot_token: t.bot_token.clone(),
            trigger_word: t.trigger_word.clone(),
            allowed_chats: t.allowed_chats.clone(),
        });
    *listener = Some(crate::inbound::InboundListener::start(
        app_handle.clone(),
        slack,
        telegram,
    ));
}

/// Auto-start the listener from app setup if inbound config already exists.
/// Called from `main.rs::setup()` after `AppState` is constructed.
pub async fn bootstrap_inbound_listener(state: &AppState, app_handle: &tauri::AppHandle) {
    let dto = match get_inbound_config().await {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "inbound bootstrap: could not read config");
            return;
        }
    };
    if dto.slack.is_none() && dto.telegram.is_none() {
        return;
    }
    restart_inbound_listener(state, app_handle, &dto).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use shannon_core::notifier::{Cooldown, LogNotifier, Notifier};

    #[test]
    fn test_fire_query_notification_completed_always_fires() {
        let mut notifier = Notifier::new().with_cooldown(Cooldown::new());
        notifier.add_handler(Box::new(LogNotifier::new()));
        let first = fire_query_notification(&notifier, NotificationKind::Completed).unwrap();
        assert!(first);
        let second = fire_query_notification(&notifier, NotificationKind::Completed).unwrap();
        assert!(second, "completed has 0ms window — no cooldown");
    }

    #[test]
    fn test_fire_query_notification_failed_coalesces() {
        let mut notifier = Notifier::new().with_cooldown(Cooldown::new());
        notifier.add_handler(Box::new(LogNotifier::new()));
        let first =
            fire_query_notification(&notifier, NotificationKind::Failed("api timeout".into()))
                .unwrap();
        assert!(first);
        let second =
            fire_query_notification(&notifier, NotificationKind::Failed("api timeout 2".into()))
                .unwrap();
        assert!(
            !second,
            "second failure within 5s window should be coalesced"
        );
    }

    #[test]
    fn test_notification_payload_deserializes_with_optional_level() {
        let json = serde_json::json!({
            "title": "Hello",
            "body": "World",
        });
        let p: NotificationPayload = serde_json::from_value(json).expect("parse");
        assert_eq!(p.title, "Hello");
        assert_eq!(p.body, "World");
        assert!(p.level.is_none());
    }

    #[test]
    fn test_notification_payload_deserializes_with_level() {
        let json = serde_json::json!({
            "title": "Boom",
            "body": "broken",
            "level": "error",
        });
        let p: NotificationPayload = serde_json::from_value(json).expect("parse");
        assert_eq!(p.level.as_deref(), Some("error"));
    }

    // ── template_to_str / template_from_str roundtrip ─────────────────

    #[test]
    fn webhook_template_str_roundtrip_preserves_known_variants() {
        use shannon_core::notifier::WebhookTemplate::*;
        for t in [
            Slack, Discord, Feishu, Wechat, Teams, Telegram, DingTalk, Raw,
        ] {
            let s = template_to_str(&t);
            let back = template_from_str(&s);
            assert_eq!(template_to_str(&back), s, "roundtrip not stable for {s}");
        }
    }

    #[test]
    fn webhook_template_custom_roundtrip() {
        use shannon_core::notifier::WebhookTemplate;
        let original = WebhookTemplate::Custom("X".repeat(120));
        let s = template_to_str(&original);
        assert!(s.starts_with("custom:"));
        let back = template_from_str(&s);
        assert_eq!(template_to_str(&back), s);
    }

    #[test]
    fn webhook_template_from_str_unknown_falls_back_to_raw() {
        use shannon_core::notifier::WebhookTemplate;
        assert_eq!(template_from_str("unknown"), WebhookTemplate::Raw);
        assert_eq!(template_from_str(""), WebhookTemplate::Raw);
    }
}
