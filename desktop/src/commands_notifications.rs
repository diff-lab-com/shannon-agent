//! Notification commands — native OS notifications and webhook config.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//!
//! `fire_query_notification`, `NotificationKind`, and
//! `load_desktop_webhook_config` are `pub(crate)` because `send_message` (still
//! in `commands.rs`) and `AppState::new` call them across the module boundary.

use crate::commands::AppState;
use crate::events::{self, event_names};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

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

/// Saved desktop-notification preferences (master enable + DND window).
/// Mirrors the `notifications_*` fields on `DesktopConfig`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPrefsDto {
    pub master_enabled: bool,
    pub dnd_enabled: bool,
    #[serde(default)]
    pub dnd_start: Option<String>,
    #[serde(default)]
    pub dnd_end: Option<String>,
    pub on_completed: bool,
    pub on_failed: bool,
    /// Surface a desktop notification when the user's attention is required
    /// (tool-approval waits, budget alerts). `#[serde(default)]` keeps old
    /// frontend payloads / config files loadable; defaults to enabled.
    #[serde(default = "default_on_needs_attention")]
    pub on_needs_attention: bool,
    /// Play the frontend-composited chime (Web Audio) on task completed /
    /// failed / needs-attention events. Independent of the OS notification
    /// sound (R4). Defaults to disabled.
    #[serde(default)]
    pub sound_enabled: bool,
}

fn default_on_needs_attention() -> bool {
    true
}

/// Read the current desktop-notification preferences.
#[tauri::command]
pub async fn get_notification_prefs() -> Result<NotificationPrefsDto, String> {
    let c = crate::config::load_config();
    Ok(NotificationPrefsDto {
        master_enabled: c.notifications_master_enabled,
        dnd_enabled: c.notifications_dnd_enabled,
        dnd_start: c.notifications_dnd_start,
        dnd_end: c.notifications_dnd_end,
        on_completed: c.notifications_on_completed,
        on_failed: c.notifications_on_failed,
        on_needs_attention: c.notifications_on_needs_attention,
        sound_enabled: c.notifications_sound_enabled,
    })
}

/// Persist desktop-notification preferences. Validates the `"HH:MM"` window
/// bounds when present, then writes through `DesktopConfig` and emits
/// `CONFIG_UPDATED` so any open settings panel refreshes.
#[tauri::command]
pub async fn set_notification_prefs(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    prefs: NotificationPrefsDto,
) -> Result<(), String> {
    if let Some(s) = prefs.dnd_start.as_deref() {
        parse_hhmm(s).ok_or_else(|| format!("invalid dnd_start (expected HH:MM): {s}"))?;
    }
    if let Some(e) = prefs.dnd_end.as_deref() {
        parse_hhmm(e).ok_or_else(|| format!("invalid dnd_end (expected HH:MM): {e}"))?;
    }
    let snapshot = {
        let mut dc = state.desktop_config.write().await;
        dc.notifications_master_enabled = prefs.master_enabled;
        dc.notifications_dnd_enabled = prefs.dnd_enabled;
        dc.notifications_dnd_start = prefs.dnd_start;
        dc.notifications_dnd_end = prefs.dnd_end;
        dc.notifications_on_completed = prefs.on_completed;
        dc.notifications_on_failed = prefs.on_failed;
        dc.notifications_on_needs_attention = prefs.on_needs_attention;
        dc.notifications_sound_enabled = prefs.sound_enabled;
        dc.clone()
    };
    crate::config::save_config(&snapshot)?;
    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "notifications".into(),
            value: "updated".into(),
        },
    );
    Ok(())
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

// === Webhook test send (P1-7) ==============================================

/// Result of `test_webhook` — the delivery verdict the settings "send test"
/// button surfaces, plus the HTTP status when the receiver answered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookTestResult {
    pub success: bool,
    pub status: Option<u16>,
    /// Human-readable summary: `HTTP 200` on delivery, the transport error
    /// line on failure, or why the URL was blocked.
    pub detail: String,
}

/// SSRF guard for `test_webhook`, mirroring the settings UI's
/// `validateWebhookUrl` (desktop/ui/src/lib/packageValidation.ts): http(s)
/// only, never a loopback / private / link-local / `.local` host. The save
/// path already enforces this in the webview; the command re-applies the
/// same rules so a hand-edited config can't turn the test button into an
/// internal-network POST. `Some(reason)` = blocked.
fn webhook_url_blocked(raw: &str) -> Option<String> {
    let url = match url::Url::parse(raw.trim()) {
        Ok(u) => u,
        Err(_) => return Some("not a valid URL".to_string()),
    };
    if url.scheme() != "http" && url.scheme() != "https" {
        return Some(format!(
            "scheme `{}` is not allowed (http/https only)",
            url.scheme()
        ));
    }
    let host = url
        .host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']');
    let h = host.to_ascii_lowercase();
    let private = h == "localhost"
        || h.ends_with(".local")
        || h.starts_with("127.")
        || h.starts_with("10.")
        || h.starts_with("192.168.")
        || h.starts_with("169.254.")
        || (h.starts_with("172.")
            && h.split('.')
                .nth(1)
                .and_then(|o| o.parse::<u8>().ok())
                .is_some_and(|o| (16..=31).contains(&o)))
        || h.starts_with("0.")
        || h == "::1"
        || h.starts_with("fc00:")
        || h.starts_with("fe80:")
        || h.starts_with("fd");
    if private {
        return Some(format!(
            "host `{host}` resolves to a private/loopback range"
        ));
    }
    None
}

/// Send a one-shot test payload to the currently configured webhook URL.
///
/// Reuses the production pipeline end-to-end: the saved
/// `[notifications.webhook]` config (preset template, secret, timeout),
/// `WebhookHandler::render_body` for the payload, and the same HMAC
/// signature header — but a single synchronous POST (`deliver_once`, no
/// retries) so the UI can show an immediate verdict. `title`/`body` come
/// from the caller so the test message renders in the user's locale.
#[tauri::command]
pub async fn test_webhook(title: String, body: String) -> Result<WebhookTestResult, String> {
    let config = load_desktop_webhook_config()
        .ok_or_else(|| "no webhook configured — save a webhook URL first".to_string())?;
    // SSRF guard (same rules the save-path webview enforces) — applied before
    // any await so a hand-edited config can't turn the test button into an
    // internal-network POST.
    if let Some(reason) = webhook_url_blocked(&config.url) {
        return Ok(WebhookTestResult {
            success: false,
            status: None,
            detail: reason,
        });
    }
    Ok(deliver_test_webhook(&config, title, body).await)
}

/// Build the handler, render the test payload through the configured
/// template, and map the single-shot delivery outcome to a
/// [`WebhookTestResult`]. Split from the command (and free of the URL guard)
/// so tests can drive it against a local mock server. Callers must have
/// applied [`webhook_url_blocked`] first.
async fn deliver_test_webhook(
    config: &shannon_core::notifier::WebhookConfig,
    title: String,
    body: String,
) -> WebhookTestResult {
    let handler = match shannon_core::notifier::WebhookHandler::new(config.clone()) {
        Ok(h) => h,
        Err(e) => {
            return WebhookTestResult {
                success: false,
                status: None,
                detail: format!("webhook client build failed: {e}"),
            };
        }
    };
    let notification = shannon_core::notifier::Notification {
        title,
        body,
        level: shannon_core::notifier::NotificationLevel::Info,
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        source: Some("webhook_test".to_string()),
        action_id: None,
        kind: shannon_core::notifier::NotificationKind::Completed,
    };
    let payload = handler.render_body(&notification);
    match handler.deliver_once(payload).await {
        Ok(status) => {
            let success = (200..300).contains(&status);
            WebhookTestResult {
                success,
                status: Some(status),
                detail: format!("HTTP {status}"),
            }
        }
        Err(detail) => WebhookTestResult {
            success: false,
            status: None,
            detail,
        },
    }
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

    let (title, body, level, source, window_ms, core_kind) = match kind {
        NotificationKind::Completed => (
            "Shannon".to_string(),
            "Query complete".to_string(),
            NotificationLevel::Info,
            "query_complete".to_string(),
            0_u64,
            shannon_core::notifier::NotificationKind::Completed,
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
                shannon_core::notifier::NotificationKind::Failed,
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
        kind: core_kind,
    };

    notifier.notify_dedup(&notification, window_ms)
}

/// Fire a query-event notification and surface the outcome instead of silently
/// dropping it. Every call site previously used `let _ = fire_query_notification(…)`,
/// which hid two real conditions: `Ok(false)` — the notifier suppressed the
/// notification (cooldown/dedup) — and `Err` — the OS-notification dispatch
/// itself failed. `context` is a short label (e.g. "query_completed") included
/// in the log line so the source event is identifiable.
pub(crate) fn fire_query_notification_logged(
    notifier: &shannon_core::notifier::Notifier,
    kind: NotificationKind,
    context: &'static str,
) {
    match fire_query_notification(notifier, kind) {
        Ok(true) => tracing::trace!("{} notification dispatched", context),
        Ok(false) => tracing::debug!("{} notification suppressed by cooldown/dedup", context),
        Err(e) => tracing::warn!(error = %e, "{} notification dispatch failed", context),
    }
}

/// Best-effort load of `[notifications.webhook]` from `~/.shannon/config.toml`
/// and `.shannon.toml` (project-local). Returns `None` on any error — never
/// panics the app on config issues.
///
/// Two readers, deliberately: `ConfigBuilder` handles JSON config files in
/// full (serde path), but its TOML fallback parser only understands flat
/// `key = value` lines — a `[notifications.webhook]` table written by
/// [`save_webhook_config_to_disk`] is invisible to it (P1-7: the settings UI
/// round-trip and the startup webhook attach both silently saw `None`). So
/// when the builder comes up empty, re-read the same file the save path
/// wrote (same `.shannon.toml`-else-global preference) with a real TOML
/// parse.
pub(crate) fn load_desktop_webhook_config() -> Option<shannon_core::notifier::WebhookConfig> {
    let cfg = shannon_core::unified_config::ConfigBuilder::new()
        .load_global_toml()
        .load_local_toml()
        .build();
    if let Some(webhook) = cfg.notifications.and_then(|n| n.webhook) {
        return Some(webhook);
    }
    load_webhook_config_from_toml_file()
}

/// Read-only parse of `[notifications.webhook]` from the config file the
/// save path uses (project-local `.shannon.toml` when it exists, else the
/// global `~/.shannon/config.toml`). Never creates anything; `None` on any
/// error or when no webhook table is present.
fn load_webhook_config_from_toml_file() -> Option<shannon_core::notifier::WebhookConfig> {
    let path = webhook_config_read_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let root: toml::Value = toml::from_str(&content).ok()?;
    let table = root.get("notifications")?.get("webhook")?;
    let json = serde_json::to_value(table).ok()?;
    serde_json::from_value(json).ok()
}

/// The config file the webhook section is read from (no side effects — the
/// save path's `resolve_webhook_config_path` additionally creates dirs).
fn webhook_config_read_path() -> Option<std::path::PathBuf> {
    let local = std::path::Path::new(".shannon.toml");
    if local.exists() {
        return Some(local.to_path_buf());
    }
    Some(dirs::home_dir()?.join(".shannon").join("config.toml"))
}

// === Desktop-notification preferences (master enable + DND) ==============

/// Parsed DND preferences used by the OS-notification handler to decide
/// whether to suppress a notification. Loaded fresh from `DesktopConfig` on
/// each call — notifications are infrequent, so the disk read is negligible.
pub(crate) struct NotificationPrefs {
    pub master_enabled: bool,
    pub dnd_enabled: bool,
    dnd_start_min: Option<u32>,
    dnd_end_min: Option<u32>,
    on_completed: bool,
    on_failed: bool,
    on_needs_attention: bool,
}

impl NotificationPrefs {
    /// Read + parse from `DesktopConfig`. Malformed `"HH:MM"` values are
    /// dropped (treated as unset), so a bad edit never panics the handler.
    pub(crate) fn load() -> Self {
        let c = crate::config::load_config();
        NotificationPrefs {
            master_enabled: c.notifications_master_enabled,
            dnd_enabled: c.notifications_dnd_enabled,
            dnd_start_min: c.notifications_dnd_start.as_deref().and_then(parse_hhmm),
            dnd_end_min: c.notifications_dnd_end.as_deref().and_then(parse_hhmm),
            on_completed: c.notifications_on_completed,
            on_failed: c.notifications_on_failed,
            on_needs_attention: c.notifications_on_needs_attention,
        }
    }

    /// Whether the current system-local time falls inside the DND window.
    /// False when DND is off or either bound is unset.
    pub(crate) fn within_dnd_window(&self) -> bool {
        if !self.dnd_enabled {
            return false;
        }
        let (Some(start), Some(end)) = (self.dnd_start_min, self.dnd_end_min) else {
            return false;
        };
        is_within_dnd_window(minutes_since_local_midnight(), start, end)
    }

    /// Whether the event-type toggles permit a notification of the given
    /// semantic kind (three-way since settings-r3 T5): completions honor
    /// `on_completed`, failures honor `on_failed`, and attention requests
    /// (tool approvals, budget alerts) honor `on_needs_attention`.
    pub(crate) fn allows_level(&self, kind: shannon_core::notifier::NotificationKind) -> bool {
        use shannon_core::notifier::NotificationKind as K;
        match kind {
            K::Completed => self.on_completed,
            K::Failed => self.on_failed,
            K::NeedsAttention => self.on_needs_attention,
        }
    }
}

/// Minutes since midnight in system-local time (0..=1439).
fn minutes_since_local_midnight() -> u32 {
    use chrono::Timelike;
    chrono::Local::now().time().num_seconds_from_midnight() / 60
}

/// Parse `"HH:MM"` (24h, lenient — `"9:05"` accepted) into minutes-of-day.
/// `None` when the string is malformed or out of range.
pub(crate) fn parse_hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    let hours: u32 = h.parse().ok()?;
    let minutes: u32 = m.parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(hours * 60 + minutes)
}

/// Is `now_min` inside the `[start, end)` quiet window? Handles overnight
/// wrap (e.g. 22:00 → 07:00). `start == end` means "no window" (returns
/// false) so a freshly-enabled DND with identical bounds doesn't suppress
/// everything.
pub(crate) fn is_within_dnd_window(now_min: u32, start_min: u32, end_min: u32) -> bool {
    if start_min == end_min {
        return false;
    }
    if start_min < end_min {
        now_min >= start_min && now_min < end_min
    } else {
        // overnight wrap
        now_min >= start_min || now_min < end_min
    }
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
    // The `[notifications.webhook]` table carries the plaintext `secret` —
    // owner-only atomic write (R6).
    crate::secret_files::write_atomic_owner_only(&path, serialized.as_bytes())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
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
    // Same file can still hold a webhook `secret` in sibling tables (and the
    // path may be the project-local `.shannon.toml`) — keep the owner-only
    // atomic write (R6).
    crate::secret_files::write_atomic_owner_only(&path, serialized.as_bytes())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    tracing::info!(path = %path.display(), "webhook config cleared");
    Ok(())
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

    // === DND / quiet-hours window parsing + evaluation ===

    #[test]
    fn parse_hhmm_accepts_zero_padded_and_short_forms() {
        assert_eq!(parse_hhmm("00:00"), Some(0));
        assert_eq!(parse_hhmm("23:59"), Some(23 * 60 + 59));
        assert_eq!(parse_hhmm("9:05"), Some(9 * 60 + 5)); // lenient short form
        assert_eq!(parse_hhmm("7:30"), Some(7 * 60 + 30));
    }

    #[test]
    fn parse_hhmm_rejects_out_of_range_and_malformed() {
        assert_eq!(parse_hhmm("24:00"), None); // hour > 23
        assert_eq!(parse_hhmm("12:60"), None); // minute > 59
        assert_eq!(parse_hhmm("noon"), None);
        assert_eq!(parse_hhmm("12"), None);
        assert_eq!(parse_hhmm(""), None);
        assert_eq!(parse_hhmm("ab:cd"), None);
    }

    #[test]
    fn dnd_window_same_day_inclusive_start_exclusive_end() {
        // 09:00–17:00
        let (start, end) = (9 * 60, 17 * 60);
        assert!(is_within_dnd_window(9 * 60, start, end)); // at start
        assert!(is_within_dnd_window(12 * 60, start, end)); // midday
        assert!(!is_within_dnd_window(17 * 60, start, end)); // at end (exclusive)
        assert!(!is_within_dnd_window(8 * 60, start, end)); // before
        assert!(!is_within_dnd_window(23 * 60, start, end)); // after
    }

    #[test]
    fn dnd_window_overnight_wraps_past_midnight() {
        // 22:00–07:00 (wraps midnight)
        let (start, end) = (22 * 60, 7 * 60);
        assert!(is_within_dnd_window(22 * 60, start, end)); // at start
        assert!(is_within_dnd_window(23 * 60, start, end)); // late night
        assert!(is_within_dnd_window(0, start, end)); // midnight
        assert!(is_within_dnd_window(3 * 60, start, end)); // early morning
        assert!(!is_within_dnd_window(7 * 60, start, end)); // at end (exclusive)
        assert!(!is_within_dnd_window(12 * 60, start, end)); // midday
    }

    #[test]
    fn dnd_window_equal_bounds_is_no_window() {
        // start == end means "disabled" — never suppress (so a freshly-enabled
        // DND with identical bounds doesn't suppress everything).
        assert!(!is_within_dnd_window(0, 600, 600));
        assert!(!is_within_dnd_window(600, 600, 600));
        assert!(!is_within_dnd_window(1439, 600, 600));
    }

    #[test]
    fn allows_level_routes_by_kind() {
        use shannon_core::notifier::NotificationKind as K;
        let all = NotificationPrefs {
            master_enabled: true,
            dnd_enabled: false,
            dnd_start_min: None,
            dnd_end_min: None,
            on_completed: true,
            on_failed: true,
            on_needs_attention: true,
        };
        assert!(all.allows_level(K::Completed));
        assert!(all.allows_level(K::Failed));
        assert!(all.allows_level(K::NeedsAttention));

        // Each toggle mutes exactly its own kind.
        let only_errors = NotificationPrefs {
            on_completed: false,
            on_needs_attention: false,
            ..all
        };
        assert!(!only_errors.allows_level(K::Completed));
        assert!(only_errors.allows_level(K::Failed));
        assert!(!only_errors.allows_level(K::NeedsAttention));

        let only_completed = NotificationPrefs {
            on_completed: true,
            on_failed: false,
            on_needs_attention: false,
            ..all
        };
        assert!(only_completed.allows_level(K::Completed));
        assert!(!only_completed.allows_level(K::Failed));
        assert!(!only_completed.allows_level(K::NeedsAttention));

        // Attention-only config: approvals/budget alerts fire while the
        // completed/failed chatter stays muted.
        let attention_only = NotificationPrefs {
            on_completed: false,
            on_failed: false,
            on_needs_attention: true,
            ..all
        };
        assert!(!attention_only.allows_level(K::Completed));
        assert!(!attention_only.allows_level(K::Failed));
        assert!(attention_only.allows_level(K::NeedsAttention));
    }

    #[test]
    fn notification_prefs_dto_roundtrips_new_fields() {
        let dto = NotificationPrefsDto {
            master_enabled: true,
            dnd_enabled: true,
            dnd_start: Some("22:00".into()),
            dnd_end: Some("07:00".into()),
            on_completed: false,
            on_failed: true,
            on_needs_attention: false,
            sound_enabled: true,
        };
        let json = serde_json::to_value(&dto).expect("serialize");
        let back: NotificationPrefsDto = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back.on_needs_attention, false);
        assert_eq!(back.sound_enabled, true);
        assert_eq!(back.dnd_start.as_deref(), Some("22:00"));
    }

    #[test]
    fn notification_prefs_dto_defaults_new_fields_for_old_payloads() {
        // An old frontend (or hand-rolled payload) without the two new keys
        // must still deserialize: needs-attention on, sound off.
        let json = serde_json::json!({
            "master_enabled": true,
            "dnd_enabled": false,
            "on_completed": true,
            "on_failed": true,
        });
        let dto: NotificationPrefsDto = serde_json::from_value(json).expect("parse");
        assert!(dto.on_needs_attention, "needs-attention defaults on");
        assert!(!dto.sound_enabled, "sound defaults off");
    }

    // === test_webhook (P1-7) =================================================

    /// Serialize every `$HOME`-swapping test in this module (same per-module
    /// env lock as policy_limits/model_registry): cargo test runs this
    /// binary's tests on parallel threads, and an unsynchronized HOME swap
    /// races other tests' save/restore windows.
    fn home_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Redirect `$HOME` at `tmp` for the duration of `f` (same save/restore
    /// pattern as the commands_feedback tests) so `load_desktop_webhook_config`
    /// reads a temp `~/.shannon/config.toml` instead of the developer's.
    fn with_temp_home<F: FnOnce(&std::path::Path)>(f: F) {
        let _guard = home_test_lock();
        let tmp = tempfile::tempdir().expect("tempdir");
        let prev = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", tmp.path()) };
        f(tmp.path());
        match prev {
            Some(prev) => unsafe { std::env::set_var("HOME", prev) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }

    fn write_global_webhook_config(home: &std::path::Path, body: &str) {
        let dir = home.join(".shannon");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), body).unwrap();
    }

    #[test]
    fn webhook_url_blocked_rejects_private_and_loopback_hosts() {
        for url in [
            "http://localhost:8080/hook",
            "http://127.0.0.1:9090/hook",
            "http://10.1.2.3/hook",
            "http://192.168.1.10/hook",
            "http://172.16.0.9/hook",
            "http://172.31.255.1/hook",
            "http://169.254.169.254/latest/meta-data",
            "http://0.1.2.3/hook",
            "http://[::1]:33420/hook",
            "http://[fc00::1]/hook",
            "http://[fe80::1]/hook",
            "http://[fd12::1]/hook",
            "https://intranet.corp.local/hook",
        ] {
            assert!(webhook_url_blocked(url).is_some(), "{url} must be blocked");
        }
        // 172.* outside 16–31 is public.
        assert!(webhook_url_blocked("http://172.32.1.9/hook").is_none());
    }

    #[test]
    fn webhook_url_blocked_rejects_non_http_schemes_and_garbage() {
        assert!(webhook_url_blocked("ftp://example.com").is_some());
        assert!(webhook_url_blocked("file:///etc/passwd").is_some());
        assert!(webhook_url_blocked("not a url").is_some());
    }

    #[test]
    fn webhook_url_blocked_allows_public_https() {
        assert!(webhook_url_blocked("https://hooks.slack.com/services/T/B/X").is_none());
        assert!(webhook_url_blocked("https://discord.com/api/webhooks/1/2").is_none());
    }

    #[test]
    fn test_webhook_without_config_errors() {
        with_temp_home(|_home| {
            // No ~/.shannon/config.toml at all. The command fails before its
            // first await, so the HOME swap never spans an yield point.
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let result = test_webhook("t".into(), "b".into()).await;
                assert!(result.is_err(), "no config must Err, got {result:?}");
            });
        });
    }

    fn webhook_config_for(
        url: String,
        template: shannon_core::notifier::WebhookTemplate,
    ) -> shannon_core::notifier::WebhookConfig {
        shannon_core::notifier::WebhookConfig {
            url,
            secret: None,
            template,
            timeout_ms: 5000,
            include_body: true,
        }
    }

    #[tokio::test]
    async fn deliver_test_webhook_renders_config_template_and_reports_success() {
        use mockito::Server;

        let mut server = Server::new_async().await;
        // The rendered Slack preset (include_body = true) carries
        // `*{title}*\n{body}` — match the exact compact JSON the pipeline
        // produces so a template regression can't slip through.
        let mock = server
            .mock("POST", "/hook")
            .with_status(200)
            .match_body(mockito::Matcher::JsonString(
                serde_json::json!({ "text": "*Shannon webhook test*\nhello" }).to_string(),
            ))
            .create_async()
            .await;
        let config = webhook_config_for(
            format!("{}/hook", server.url()),
            shannon_core::notifier::WebhookTemplate::Slack,
        );

        let result =
            deliver_test_webhook(&config, "Shannon webhook test".into(), "hello".into()).await;
        assert!(result.success, "{result:?}");
        assert_eq!(result.status, Some(200));
        assert!(result.detail.contains("200"));
        mock.assert();
    }

    #[tokio::test]
    async fn deliver_test_webhook_reports_non_2xx_as_failure_with_status() {
        use mockito::Server;

        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/hook")
            .with_status(404)
            .create_async()
            .await;
        let config = webhook_config_for(
            format!("{}/hook", server.url()),
            shannon_core::notifier::WebhookTemplate::Raw,
        );

        let result = deliver_test_webhook(&config, "t".into(), "b".into()).await;
        assert!(!result.success);
        assert_eq!(result.status, Some(404));
        assert!(result.detail.contains("404"));
        mock.assert();
    }

    #[tokio::test]
    async fn deliver_test_webhook_maps_transport_failure_to_failed_result() {
        // Port 1 on loopback is a reliable connection-refused.
        let config = webhook_config_for(
            "http://127.0.0.1:1/hook".into(),
            shannon_core::notifier::WebhookTemplate::Raw,
        );
        let result = deliver_test_webhook(&config, "t".into(), "b".into()).await;
        assert!(!result.success);
        assert!(result.status.is_none());
        assert!(result.detail.contains("webhook request failed"));
    }

    #[test]
    fn test_webhook_blocks_private_host_from_config_without_posting() {
        // A hand-edited config targeting loopback must not turn the test
        // button into an internal POST. The guard fires before the command's
        // first await, so the HOME swap never spans a yield point.
        with_temp_home(|home| {
            write_global_webhook_config(
                home,
                "[notifications.webhook]\nurl = \"http://127.0.0.1:9/hook\"\ntemplate = \"raw\"\n",
            );
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let result = test_webhook("t".into(), "b".into())
                    .await
                    .expect("command result");
                assert!(!result.success);
                assert!(result.status.is_none());
                assert!(result.detail.contains("private/loopback"));
            });
        });
    }

    #[test]
    fn load_desktop_webhook_config_reads_the_toml_table_the_save_path_writes() {
        // P1-7 regression: the ConfigBuilder TOML fallback ignores tables, so
        // the section the settings UI saves was invisible to every reader.
        with_temp_home(|home| {
            assert!(
                load_desktop_webhook_config().is_none(),
                "no config file yet"
            );
            write_global_webhook_config(
                home,
                "[notifications.webhook]\nurl = \"https://discord.com/api/webhooks/1/2\"\ntemplate = \"discord\"\nsecret = \"s3cret\"\ntimeout_ms = 9000\ninclude_body = true\n",
            );
            let cfg = load_desktop_webhook_config().expect("webhook table must load");
            assert_eq!(cfg.url, "https://discord.com/api/webhooks/1/2");
            assert_eq!(
                cfg.template,
                shannon_core::notifier::WebhookTemplate::Discord
            );
            assert_eq!(cfg.secret.as_deref(), Some("s3cret"));
            assert_eq!(cfg.timeout_ms, 9000);
            assert!(cfg.include_body);
        });
    }
}
