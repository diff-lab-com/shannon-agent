//! # Notifier
//!
//! System notification support with pluggable handlers.
//!
//! The [`Notifier`] dispatches notifications to one or more registered
//! [`NotificationHandler`] implementations.  Three built-in handlers are
//! provided:
//!
//! - [`LogNotifier`] -- prints to stderr
//! - [`FileNotifier`] -- appends to a log file
//! - [`CallbackNotifier`] -- invokes a custom closure
//!
//! ## Example
//!
//! ```
//! use shannon_core::notifier::{Notifier, LogNotifier};
//!
//! let mut notifier = Notifier::new();
//! notifier.add_handler(Box::new(LogNotifier::new()));
//! notifier.info("Hello", "World").unwrap();
//! ```

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use thiserror::Error;

// ============================================================================
// Error types
// ============================================================================

/// Errors produced by notifier operations.
#[derive(Error, Debug)]
pub enum NotifierError {
    /// An I/O error occurred (e.g. writing to a log file).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A handler returned an error.
    #[error("handler '{name}' failed: {reason}")]
    HandlerFailed { name: String, reason: String },
}

// ============================================================================
// Core types
// ============================================================================

/// Severity level of a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationLevel {
    /// General informational notice.
    Info,
    /// An operation succeeded.
    Success,
    /// Something warrants attention but is not fatal.
    Warning,
    /// An operation failed.
    #[serde(alias = "critical")]
    Error,
}

impl fmt::Display for NotificationLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Success => write!(f, "SUCCESS"),
            Self::Warning => write!(f, "WARNING"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

impl NotificationLevel {
    /// Numeric severity for `minimum_level` filtering. Higher = more severe.
    pub fn severity(self) -> u8 {
        match self {
            Self::Info | Self::Success => 0,
            Self::Warning => 1,
            Self::Error => 2,
        }
    }

    /// Parse a level from a case-insensitive string (`"info"`, `"warning"`, `"critical"`/`"error"`).
    pub fn parse_lossy(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "info" | "success" => Some(Self::Info),
            "warning" => Some(Self::Warning),
            "error" | "critical" => Some(Self::Error),
            _ => None,
        }
    }
}

use serde::{Deserialize, Serialize};

/// A single notification payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Human-readable title.
    pub title: String,
    /// Detailed body text.
    pub body: String,
    /// Severity level.
    pub level: NotificationLevel,
    /// Unique identifier for this notification.
    pub id: String,
    /// When the notification was created.
    pub timestamp: DateTime<Utc>,
    /// Logical source for cooldown/dedup keying (e.g. `"tool:Edit"`, `"error:ApiTimeout"`).
    /// Notifications with the same `source` are coalesced within their cooldown window.
    /// `None` means never coalesce (each notification is unique).
    #[serde(default)]
    pub source: Option<String>,
    /// Optional action identifier (e.g. `"approve:permission_request_42"`).
    /// Consumed by interactive frontends (desktop app) to render action buttons.
    /// CLI shell-out ignores this field.
    #[serde(default)]
    pub action_id: Option<String>,
}

// ============================================================================
// Cooldown / deduplication
// ============================================================================

/// Per-source-type cooldown windows (milliseconds) for notification deduplication.
///
/// Sources whose key matches the configured category are deduplicated within the
/// window. `0` means no cooldown (always fire).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationCooldownConfig {
    /// Permission-request notifications — always unique, no cooldown.
    #[serde(default = "NotificationCooldownConfig::default_permission_ms")]
    pub permission_ms: u64,
    /// Query/task completion — one-shot, no cooldown.
    #[serde(default = "NotificationCooldownConfig::default_query_complete_ms")]
    pub query_complete_ms: u64,
    /// Per-tool-name dedup window (e.g. `"tool:Edit"`, `"tool:Bash"`).
    #[serde(default = "NotificationCooldownConfig::default_tool_complete_ms")]
    pub tool_complete_ms: u64,
    /// Per-error-type dedup window (e.g. `"error:ApiTimeout"`).
    #[serde(default = "NotificationCooldownConfig::default_error_ms")]
    pub error_ms: u64,
    /// Per-agent-id idle dedup window.
    #[serde(default = "NotificationCooldownConfig::default_agent_idle_ms")]
    pub agent_idle_ms: u64,
}

impl NotificationCooldownConfig {
    fn default_permission_ms() -> u64 {
        0
    }
    fn default_query_complete_ms() -> u64 {
        0
    }
    fn default_tool_complete_ms() -> u64 {
        3000
    }
    fn default_error_ms() -> u64 {
        5000
    }
    fn default_agent_idle_ms() -> u64 {
        10000
    }
}

impl Default for NotificationCooldownConfig {
    fn default() -> Self {
        Self {
            permission_ms: Self::default_permission_ms(),
            query_complete_ms: Self::default_query_complete_ms(),
            tool_complete_ms: Self::default_tool_complete_ms(),
            error_ms: Self::default_error_ms(),
            agent_idle_ms: Self::default_agent_idle_ms(),
        }
    }
}

/// Top-level notification configuration parsed from `[notifications]` in `.shannon.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationsConfig {
    /// Master switch. When `false`, all notification emission is suppressed.
    #[serde(default)]
    pub enabled: bool,
    /// Whether to play a sound on fire (platform-dependent). Default `true` when enabled.
    #[serde(default = "NotificationsConfig::default_sound")]
    pub sound: bool,
    /// Notifications below this severity are filtered out.
    #[serde(default = "NotificationsConfig::default_minimum_level")]
    pub minimum_level: NotificationLevel,
    /// Per-source-type cooldown windows.
    #[serde(default)]
    pub cooldown: NotificationCooldownConfig,
}

impl NotificationsConfig {
    fn default_sound() -> bool {
        true
    }
    fn default_minimum_level() -> NotificationLevel {
        NotificationLevel::Info
    }

    /// Interactive REPL defaults: enabled, sound on, all sources at info level.
    pub fn interactive_default() -> Self {
        Self {
            enabled: true,
            sound: true,
            minimum_level: NotificationLevel::Info,
            cooldown: NotificationCooldownConfig::default(),
        }
    }

    /// Headless / CI defaults: disabled unless explicitly opted in.
    pub fn headless_default() -> Self {
        Self {
            enabled: false,
            sound: false,
            minimum_level: NotificationLevel::Info,
            cooldown: NotificationCooldownConfig::default(),
        }
    }
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        // TOML deserialization default: opt-in (matches headless default).
        Self::headless_default()
    }
}

/// Tracks last-fired timestamps per source key for cooldown/dedup.
///
/// Thread-safe via `DashMap`. Keys are typically `Notification::source`
/// (e.g. `"tool:Edit"`, `"error:ApiTimeout"`) or fall back to `Notification::id`
/// when source is `None` (in which case dedup never applies — each fire is unique).
pub struct Cooldown {
    last_fired: DashMap<String, Instant>,
}

impl Default for Cooldown {
    fn default() -> Self {
        Self::new()
    }
}

impl Cooldown {
    /// Create an empty cooldown tracker.
    pub fn new() -> Self {
        Self {
            last_fired: DashMap::new(),
        }
    }

    /// Returns `true` if the key may fire now (no recent fire within `window_ms`),
    /// and records the current time. Returns `false` if suppressed by cooldown.
    ///
    /// `window_ms == 0` always returns `true` (no cooldown) but still records the
    /// timestamp so subsequent calls with a non-zero window see the latest fire.
    pub fn check_and_record(&self, key: &str, window_ms: u64) -> bool {
        let now = Instant::now();
        if let Some(entry) = self.last_fired.get(key) {
            let elapsed = now.duration_since(*entry.value());
            if window_ms > 0 && elapsed < Duration::from_millis(window_ms) {
                return false;
            }
        }
        self.last_fired.insert(key.to_string(), now);
        true
    }

    /// Returns the number of tracked source keys.
    pub fn tracked_count(&self) -> usize {
        self.last_fired.len()
    }

    /// Clear all tracked timestamps.
    pub fn clear(&self) {
        self.last_fired.clear();
    }
}

// ============================================================================
// Handler trait
// ============================================================================

/// Trait for notification delivery backends.
pub trait NotificationHandler: Send + Sync {
    /// Deliver a notification.
    fn send(&self, notification: &Notification) -> Result<(), NotifierError>;
    /// Human-readable name for this handler (used in error messages).
    fn name(&self) -> &str;
}

// ============================================================================
// Built-in handlers
// ============================================================================

/// Prints notifications to stderr.
pub struct LogNotifier {
    name: String,
}

impl LogNotifier {
    /// Create a new `LogNotifier` with the default name `"log"`.
    pub fn new() -> Self {
        Self {
            name: "log".to_string(),
        }
    }

    /// Create a `LogNotifier` with a custom name.
    pub fn with_name(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Default for LogNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationHandler for LogNotifier {
    fn send(&self, notification: &Notification) -> Result<(), NotifierError> {
        eprintln!(
            "[{}] [{}] {} - {}",
            notification.timestamp.format("%Y-%m-%dT%H:%M:%SZ"),
            notification.level,
            notification.title,
            notification.body,
        );
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Appends notifications to a file on disk.
pub struct FileNotifier {
    path: PathBuf,
    name: String,
}

impl FileNotifier {
    /// Create a new `FileNotifier` that writes to `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let name = format!("file:{}", path.display());
        Self { path, name }
    }

    /// Create a `FileNotifier` with a custom handler name.
    pub fn with_name(path: impl Into<PathBuf>, name: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            path,
            name: name.into(),
        }
    }
}

impl NotificationHandler for FileNotifier {
    fn send(&self, notification: &Notification) -> Result<(), NotifierError> {
        let line = format!(
            "[{}] [{}] {} - {}\n",
            notification.timestamp.format("%Y-%m-%dT%H:%M:%SZ"),
            notification.level,
            notification.title,
            notification.body,
        );

        // Create parent directories if they don't exist.
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?
            .write_all(line.as_bytes())?;

        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Invokes a custom closure for each notification.
pub struct CallbackNotifier<F>
where
    F: Fn(&Notification) -> Result<(), NotifierError> + Send + Sync,
{
    callback: F,
    name: String,
}

impl<F> CallbackNotifier<F>
where
    F: Fn(&Notification) -> Result<(), NotifierError> + Send + Sync,
{
    /// Create a new `CallbackNotifier` wrapping `callback`.
    pub fn new(callback: F) -> Self {
        Self {
            callback,
            name: "callback".to_string(),
        }
    }

    /// Create a `CallbackNotifier` with a custom handler name.
    pub fn with_name(callback: F, name: impl Into<String>) -> Self {
        Self {
            callback,
            name: name.into(),
        }
    }
}

impl<F> NotificationHandler for CallbackNotifier<F>
where
    F: Fn(&Notification) -> Result<(), NotifierError> + Send + Sync,
{
    fn send(&self, notification: &Notification) -> Result<(), NotifierError> {
        (self.callback)(notification)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

// ============================================================================
// Notifier (dispatcher)
// ============================================================================

/// Dispatches notifications to registered handlers.
pub struct Notifier {
    handlers: Vec<Box<dyn NotificationHandler + Send + Sync>>,
    cooldown: Option<Cooldown>,
    minimum_level: Option<NotificationLevel>,
}

impl Notifier {
    /// Create an empty `Notifier` with no handlers.
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            cooldown: None,
            minimum_level: None,
        }
    }

    /// Attach a cooldown tracker for `notify_dedup`.
    pub fn with_cooldown(mut self, cooldown: Cooldown) -> Self {
        self.cooldown = Some(cooldown);
        self
    }

    /// Set a minimum severity level. Notifications below this level are silently dropped.
    pub fn with_minimum_level(mut self, level: NotificationLevel) -> Self {
        self.minimum_level = Some(level);
        self
    }

    /// Register a handler. Handlers are invoked in registration order.
    pub fn add_handler(&mut self, handler: Box<dyn NotificationHandler + Send + Sync>) {
        self.handlers.push(handler);
    }

    /// Remove all handlers whose [`NotificationHandler::name`] equals `name`.
    ///
    /// Returns the number of handlers removed.
    pub fn remove_handler(&mut self, name: &str) -> usize {
        let before = self.handlers.len();
        self.handlers.retain(|h| h.name() != name);
        before - self.handlers.len()
    }

    /// Send a pre-built notification to all handlers.
    ///
    /// Errors from individual handlers are collected and returned as a single
    /// concatenated message.  Even if one handler fails the remaining handlers
    /// are still invoked.
    pub fn notify(&self, notification: &Notification) -> Result<(), NotifierError> {
        if let Some(min) = self.minimum_level {
            if notification.level.severity() < min.severity() {
                return Ok(());
            }
        }

        let mut errors: Vec<String> = Vec::new();

        for handler in &self.handlers {
            if let Err(e) = handler.send(notification) {
                errors.push(format!("{e}"));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(NotifierError::HandlerFailed {
                name: "multiple".to_string(),
                reason: errors.join("; "),
            })
        }
    }

    /// Send a notification with per-source cooldown/dedup.
    ///
    /// Returns `Ok(true)` if dispatched, `Ok(false)` if suppressed by cooldown.
    /// Uses `notification.source` as the dedup key (falls back to `notification.id`,
    /// which is always unique — so `None` sources bypass dedup).
    ///
    /// If no `Cooldown` is attached, this is equivalent to [`Self::notify`] and
    /// always returns `Ok(true)`.
    pub fn notify_dedup(
        &self,
        notification: &Notification,
        window_ms: u64,
    ) -> Result<bool, NotifierError> {
        if let Some(cd) = &self.cooldown {
            let key = notification.source.as_deref().unwrap_or(&notification.id);
            if !cd.check_and_record(key, window_ms) {
                return Ok(false);
            }
        }
        self.notify(notification)?;
        Ok(true)
    }

    // -- Convenience helpers -------------------------------------------------

    fn build_notification(
        &self,
        title: &str,
        body: &str,
        level: NotificationLevel,
    ) -> Notification {
        Notification {
            title: title.to_string(),
            body: body.to_string(),
            level,
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            source: None,
            action_id: None,
        }
    }

    /// Send an informational notification.
    pub fn info(&self, title: &str, body: &str) -> Result<(), NotifierError> {
        self.notify(&self.build_notification(title, body, NotificationLevel::Info))
    }

    /// Send a success notification.
    pub fn success(&self, title: &str, body: &str) -> Result<(), NotifierError> {
        self.notify(&self.build_notification(title, body, NotificationLevel::Success))
    }

    /// Send a warning notification.
    pub fn warning(&self, title: &str, body: &str) -> Result<(), NotifierError> {
        self.notify(&self.build_notification(title, body, NotificationLevel::Warning))
    }

    /// Send an error notification.
    pub fn error(&self, title: &str, body: &str) -> Result<(), NotifierError> {
        self.notify(&self.build_notification(title, body, NotificationLevel::Error))
    }

    /// Return the number of registered handlers.
    pub fn handler_count(&self) -> usize {
        self.handlers.len()
    }
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// DesktopNotifier — OS-level desktop notifications
// ============================================================================

/// Sends OS-level desktop notifications using platform-native tools.
///
/// - Linux: `notify-send` (libnotify)
/// - macOS: `osascript` (AppleScript)
/// - Windows: PowerShell toast notifications
pub struct DesktopNotifier {
    name: String,
}

impl DesktopNotifier {
    /// Create a new `DesktopNotifier`.
    pub fn new() -> Self {
        Self {
            name: "desktop".to_string(),
        }
    }

    /// Check if desktop notifications are likely available on this platform.
    pub fn is_available() -> bool {
        cfg!(target_os = "linux") || cfg!(target_os = "macos") || cfg!(target_os = "windows")
    }

    fn send_linux(&self, notification: &Notification) -> Result<(), NotifierError> {
        let icon = match notification.level {
            NotificationLevel::Info => "dialog-information",
            NotificationLevel::Success => "dialog-information",
            NotificationLevel::Warning => "dialog-warning",
            NotificationLevel::Error => "dialog-error",
        };
        let urgency = match notification.level {
            NotificationLevel::Info | NotificationLevel::Success => "normal",
            NotificationLevel::Warning => "normal",
            NotificationLevel::Error => "critical",
        };
        std::process::Command::new("notify-send")
            .args([
                "-i",
                icon,
                "-u",
                urgency,
                "-t",
                "5000",
                &notification.title,
                &notification.body,
            ])
            .output()
            .map_err(|e| NotifierError::HandlerFailed {
                name: self.name.clone(),
                reason: format!("notify-send failed: {e}"),
            })?;
        Ok(())
    }

    #[allow(dead_code)] // KEEP: cross-platform stub
    fn send_macos(&self, notification: &Notification) -> Result<(), NotifierError> {
        // Escape double quotes in body for AppleScript
        let escaped_body = notification.body.replace('"', "\\\"");
        let escaped_title = notification.title.replace('"', "\\\"");
        let script =
            format!("display notification \"{escaped_body}\" with title \"{escaped_title}\"");
        std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|e| NotifierError::HandlerFailed {
                name: self.name.clone(),
                reason: format!("osascript failed: {e}"),
            })?;
        Ok(())
    }

    #[allow(dead_code)] // KEEP: cross-platform stub
    fn send_windows(&self, notification: &Notification) -> Result<(), NotifierError> {
        // PowerShell toast notification
        let escaped_body = notification.body.replace('\'', "''");
        let escaped_title = notification.title.replace('\'', "''");
        let ps_script = format!(
            "[System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms'); \
             $n = New-Object System.Windows.Forms.NotifyIcon; \
             $n.Icon = [System.Drawing.SystemIcons]::Information; \
             $n.Visible = $true; \
             $n.ShowBalloonTip(5000, '{escaped_title}', '{escaped_body}', 'Info')"
        );
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps_script])
            .output()
            .map_err(|e| NotifierError::HandlerFailed {
                name: self.name.clone(),
                reason: format!("powershell notification failed: {e}"),
            })?;
        Ok(())
    }
}

impl Default for DesktopNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationHandler for DesktopNotifier {
    fn send(&self, notification: &Notification) -> Result<(), NotifierError> {
        #[cfg(target_os = "linux")]
        {
            self.send_linux(notification)
        }
        #[cfg(target_os = "macos")]
        {
            self.send_macos(notification)
        }
        #[cfg(target_os = "windows")]
        {
            self.send_windows(notification)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            let _ = notification;
            Err(NotifierError::HandlerFailed {
                name: self.name.clone(),
                reason: "Desktop notifications not supported on this platform".to_string(),
            })
        }
    }

    fn name(&self) -> &str {
        &self.name
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // -- NotificationLevel display -------------------------------------------

    #[test]
    fn test_level_display() {
        assert_eq!(NotificationLevel::Info.to_string(), "INFO");
        assert_eq!(NotificationLevel::Success.to_string(), "SUCCESS");
        assert_eq!(NotificationLevel::Warning.to_string(), "WARNING");
        assert_eq!(NotificationLevel::Error.to_string(), "ERROR");
    }

    // -- LogNotifier ---------------------------------------------------------

    #[test]
    fn test_log_notifier_name() {
        let n = LogNotifier::new();
        assert_eq!(n.name(), "log");
    }

    #[test]
    fn test_log_notifier_custom_name() {
        let n = LogNotifier::with_name("my-log");
        assert_eq!(n.name(), "my-log");
    }

    #[test]
    fn test_log_notifier_send() {
        let n = LogNotifier::new();
        let notification = Notification {
            title: "test".into(),
            body: "body".into(),
            level: NotificationLevel::Info,
            id: "1".into(),
            timestamp: Utc::now(),
            source: None,
            action_id: None,
        };
        assert!(n.send(&notification).is_ok());
    }

    #[test]
    fn test_log_notifier_default() {
        let n = LogNotifier::default();
        assert_eq!(n.name(), "log");
    }

    // -- FileNotifier --------------------------------------------------------

    #[test]
    fn test_file_notifier_writes_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notifications.log");
        let notifier = FileNotifier::new(&path);

        let notification = Notification {
            title: "disk test".into(),
            body: "written".into(),
            level: NotificationLevel::Success,
            id: "f1".into(),
            timestamp: Utc::now(),
            source: None,
            action_id: None,
        };
        notifier.send(&notification).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("disk test"));
        assert!(contents.contains("written"));
        assert!(contents.contains("SUCCESS"));
    }

    #[test]
    fn test_file_notifier_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("append.log");
        let notifier = FileNotifier::new(&path);

        let make_notification = |id: &str| Notification {
            title: format!("title-{id}"),
            body: format!("body-{id}"),
            level: NotificationLevel::Info,
            id: id.into(),
            timestamp: Utc::now(),
            source: None,
            action_id: None,
        };

        notifier.send(&make_notification("a")).unwrap();
        notifier.send(&make_notification("b")).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("title-a"));
        assert!(contents.contains("title-b"));
    }

    #[test]
    fn test_file_notifier_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("dir").join("log.txt");
        let notifier = FileNotifier::new(&path);

        let notification = Notification {
            title: "deep".into(),
            body: "dirs".into(),
            level: NotificationLevel::Info,
            id: "d1".into(),
            timestamp: Utc::now(),
            source: None,
            action_id: None,
        };
        notifier.send(&notification).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_file_notifier_name() {
        let n = FileNotifier::new("/tmp/test.log");
        assert!(n.name().starts_with("file:"));
    }

    // -- CallbackNotifier ----------------------------------------------------

    #[test]
    fn test_callback_notifier_invoked() {
        let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let rec = received.clone();

        let cb = CallbackNotifier::new(move |n: &Notification| {
            rec.lock().unwrap().push(n.title.clone());
            Ok(())
        });

        let notification = Notification {
            title: "cb-test".into(),
            body: "".into(),
            level: NotificationLevel::Info,
            id: "c1".into(),
            timestamp: Utc::now(),
            source: None,
            action_id: None,
        };
        cb.send(&notification).unwrap();

        let titles = received.lock().unwrap();
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0], "cb-test");
    }

    #[test]
    fn test_callback_notifier_propagates_error() {
        let cb = CallbackNotifier::new(|_n: &Notification| -> Result<(), NotifierError> {
            Err(NotifierError::Io(std::io::Error::other("boom")))
        });

        let notification = Notification {
            title: "err".into(),
            body: "".into(),
            level: NotificationLevel::Error,
            id: "e1".into(),
            timestamp: Utc::now(),
            source: None,
            action_id: None,
        };
        let result = cb.send(&notification);
        assert!(result.is_err());
    }

    #[test]
    fn test_callback_notifier_custom_name() {
        let cb = CallbackNotifier::with_name(|_n: &Notification| Ok(()), "custom-cb");
        assert_eq!(cb.name(), "custom-cb");
    }

    // -- Notifier (dispatcher) -----------------------------------------------

    #[test]
    fn test_notifier_new_empty() {
        let n = Notifier::new();
        assert_eq!(n.handler_count(), 0);
    }

    #[test]
    fn test_notifier_add_and_count() {
        let mut n = Notifier::new();
        n.add_handler(Box::new(LogNotifier::new()));
        assert_eq!(n.handler_count(), 1);
        n.add_handler(Box::new(LogNotifier::with_name("second")));
        assert_eq!(n.handler_count(), 2);
    }

    #[test]
    fn test_notifier_remove_handler() {
        let mut n = Notifier::new();
        n.add_handler(Box::new(LogNotifier::new()));
        n.add_handler(Box::new(LogNotifier::with_name("second")));
        assert_eq!(n.handler_count(), 2);

        let removed = n.remove_handler("log");
        assert_eq!(removed, 1);
        assert_eq!(n.handler_count(), 1);
    }

    #[test]
    fn test_notifier_remove_nonexistent() {
        let mut n = Notifier::new();
        let removed = n.remove_handler("nope");
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_notifier_info_dispatches() {
        let received: Arc<Mutex<Vec<NotificationLevel>>> = Arc::new(Mutex::new(Vec::new()));
        let rec = received.clone();

        let mut n = Notifier::new();
        n.add_handler(Box::new(CallbackNotifier::new(move |notif| {
            rec.lock().unwrap().push(notif.level);
            Ok(())
        })));

        n.info("t", "b").unwrap();
        let levels = received.lock().unwrap();
        assert_eq!(levels.len(), 1);
        assert_eq!(levels[0], NotificationLevel::Info);
    }

    #[test]
    fn test_notifier_success_dispatches() {
        let received: Arc<Mutex<Vec<NotificationLevel>>> = Arc::new(Mutex::new(Vec::new()));
        let rec = received.clone();

        let mut n = Notifier::new();
        n.add_handler(Box::new(CallbackNotifier::new(move |notif| {
            rec.lock().unwrap().push(notif.level);
            Ok(())
        })));

        n.success("t", "b").unwrap();
        assert_eq!(received.lock().unwrap()[0], NotificationLevel::Success);
    }

    #[test]
    fn test_notifier_warning_dispatches() {
        let received: Arc<Mutex<Vec<NotificationLevel>>> = Arc::new(Mutex::new(Vec::new()));
        let rec = received.clone();

        let mut n = Notifier::new();
        n.add_handler(Box::new(CallbackNotifier::new(move |notif| {
            rec.lock().unwrap().push(notif.level);
            Ok(())
        })));

        n.warning("t", "b").unwrap();
        assert_eq!(received.lock().unwrap()[0], NotificationLevel::Warning);
    }

    #[test]
    fn test_notifier_error_dispatches() {
        let received: Arc<Mutex<Vec<NotificationLevel>>> = Arc::new(Mutex::new(Vec::new()));
        let rec = received.clone();

        let mut n = Notifier::new();
        n.add_handler(Box::new(CallbackNotifier::new(move |notif| {
            rec.lock().unwrap().push(notif.level);
            Ok(())
        })));

        n.error("t", "b").unwrap();
        assert_eq!(received.lock().unwrap()[0], NotificationLevel::Error);
    }

    #[test]
    fn test_notifier_notification_has_unique_id() {
        let ids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let ids_clone = ids.clone();

        let mut n = Notifier::new();
        n.add_handler(Box::new(CallbackNotifier::new(move |notif| {
            ids_clone.lock().unwrap().push(notif.id.clone());
            Ok(())
        })));

        n.info("a", "").unwrap();
        n.info("b", "").unwrap();

        let ids = ids.lock().unwrap();
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn test_notifier_continues_on_handler_error() {
        let counter: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let c = counter.clone();

        let mut n = Notifier::new();
        // First handler always fails.
        n.add_handler(Box::new(CallbackNotifier::new(
            |_n| -> Result<(), NotifierError> {
                Err(NotifierError::Io(std::io::Error::other("fail")))
            },
        )));
        // Second handler should still run.
        n.add_handler(Box::new(CallbackNotifier::new(move |_n| {
            *c.lock().unwrap() += 1;
            Ok(())
        })));

        let result = n.info("t", "b");
        assert!(result.is_err()); // because handler 1 failed
        assert_eq!(*counter.lock().unwrap(), 1); // handler 2 still ran
    }

    #[test]
    fn test_notifier_dispatches_to_multiple_handlers() {
        let count: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let c1 = count.clone();
        let c2 = count.clone();

        let mut n = Notifier::new();
        n.add_handler(Box::new(CallbackNotifier::new(move |_n| {
            *c1.lock().unwrap() += 1;
            Ok(())
        })));
        n.add_handler(Box::new(CallbackNotifier::new(move |_n| {
            *c2.lock().unwrap() += 1;
            Ok(())
        })));

        n.info("t", "b").unwrap();
        assert_eq!(*count.lock().unwrap(), 2);
    }

    #[test]
    fn test_notifier_default() {
        let n = Notifier::default();
        assert_eq!(n.handler_count(), 0);
    }

    #[test]
    fn test_notification_serialization_roundtrip() {
        let n = Notification {
            title: "ser".into(),
            body: "round".into(),
            level: NotificationLevel::Warning,
            id: "s1".into(),
            timestamp: Utc::now(),
            source: None,
            action_id: None,
        };
        let json = serde_json::to_string(&n).unwrap();
        let back: Notification = serde_json::from_str(&json).unwrap();
        assert_eq!(back.title, n.title);
        assert_eq!(back.level, n.level);
        assert_eq!(back.id, n.id);
    }

    // -- Cooldown ------------------------------------------------------------

    #[test]
    fn test_cooldown_first_call_fires() {
        let cd = Cooldown::new();
        assert!(cd.check_and_record("tool:Edit", 1000));
        assert_eq!(cd.tracked_count(), 1);
    }

    #[test]
    fn test_cooldown_second_call_within_window_suppressed() {
        let cd = Cooldown::new();
        assert!(cd.check_and_record("tool:Edit", 60_000));
        assert!(!cd.check_and_record("tool:Edit", 60_000));
    }

    #[test]
    fn test_cooldown_zero_window_always_fires() {
        let cd = Cooldown::new();
        assert!(cd.check_and_record("anything", 0));
        assert!(cd.check_and_record("anything", 0));
        assert!(cd.check_and_record("anything", 0));
    }

    #[test]
    fn test_cooldown_independent_keys() {
        let cd = Cooldown::new();
        assert!(cd.check_and_record("tool:Edit", 60_000));
        assert!(cd.check_and_record("tool:Bash", 60_000));
        assert!(!cd.check_and_record("tool:Edit", 60_000));
        assert!(!cd.check_and_record("tool:Bash", 60_000));
    }

    #[test]
    fn test_cooldown_clear_resets() {
        let cd = Cooldown::new();
        cd.check_and_record("k", 60_000);
        assert_eq!(cd.tracked_count(), 1);
        cd.clear();
        assert_eq!(cd.tracked_count(), 0);
        assert!(cd.check_and_record("k", 60_000));
    }

    // -- Notifier::notify_dedup ---------------------------------------------

    fn capture_notifier() -> (Notifier, Arc<Mutex<Vec<String>>>) {
        let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let rec = received.clone();
        let mut n = Notifier::new().with_cooldown(Cooldown::new());
        n.add_handler(Box::new(CallbackNotifier::new(move |notif| {
            rec.lock().unwrap().push(notif.title.clone());
            Ok(())
        })));
        (n, received)
    }

    fn make_notification(title: &str, source: Option<&str>) -> Notification {
        Notification {
            title: title.into(),
            body: "".into(),
            level: NotificationLevel::Info,
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            source: source.map(str::to_string),
            action_id: None,
        }
    }

    #[test]
    fn test_notify_dedup_first_fires() {
        let (n, received) = capture_notifier();
        let notif = make_notification("first", Some("tool:Edit"));
        assert!(n.notify_dedup(&notif, 60_000).unwrap());
        assert_eq!(received.lock().unwrap().len(), 1);
    }

    #[test]
    fn test_notify_dedup_second_within_window_suppressed() {
        let (n, received) = capture_notifier();
        let notif = make_notification("a", Some("tool:Edit"));
        assert!(n.notify_dedup(&notif, 60_000).unwrap());
        let notif2 = make_notification("b", Some("tool:Edit"));
        assert!(!n.notify_dedup(&notif2, 60_000).unwrap());
        assert_eq!(received.lock().unwrap().len(), 1);
    }

    #[test]
    fn test_notify_dedup_none_source_always_fires() {
        let (n, received) = capture_notifier();
        let n1 = make_notification("a", None);
        let n2 = make_notification("b", None);
        assert!(n.notify_dedup(&n1, 60_000).unwrap());
        assert!(n.notify_dedup(&n2, 60_000).unwrap());
        assert_eq!(received.lock().unwrap().len(), 2);
    }

    #[test]
    fn test_notify_dedup_no_cooldown_attached_always_fires() {
        let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let rec = received.clone();
        let mut n = Notifier::new();
        n.add_handler(Box::new(CallbackNotifier::new(move |notif| {
            rec.lock().unwrap().push(notif.title.clone());
            Ok(())
        })));

        let notif = make_notification("a", Some("tool:Edit"));
        assert!(n.notify_dedup(&notif, 60_000).unwrap());
        assert!(n.notify_dedup(&notif, 60_000).unwrap());
        assert_eq!(received.lock().unwrap().len(), 2);
    }

    // -- Minimum level filtering --------------------------------------------

    #[test]
    fn test_minimum_level_filters_lower_severity() {
        let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let rec = received.clone();
        let mut n = Notifier::new().with_minimum_level(NotificationLevel::Warning);
        n.add_handler(Box::new(CallbackNotifier::new(move |notif| {
            rec.lock().unwrap().push(notif.title.clone());
            Ok(())
        })));

        n.info("info", "").unwrap();
        n.success("success", "").unwrap();
        n.warning("warning", "").unwrap();
        n.error("error", "").unwrap();

        let titles = received.lock().unwrap();
        assert!(!titles.contains(&"info".to_string()));
        assert!(!titles.contains(&"success".to_string()));
        assert!(titles.contains(&"warning".to_string()));
        assert!(titles.contains(&"error".to_string()));
    }

    // -- NotificationLevel helpers ------------------------------------------

    #[test]
    fn test_level_severity_ordering() {
        assert!(NotificationLevel::Info.severity() < NotificationLevel::Warning.severity());
        assert!(NotificationLevel::Warning.severity() < NotificationLevel::Error.severity());
        assert_eq!(
            NotificationLevel::Info.severity(),
            NotificationLevel::Success.severity()
        );
    }

    #[test]
    fn test_level_parse_lossy() {
        assert_eq!(
            NotificationLevel::parse_lossy("info"),
            Some(NotificationLevel::Info)
        );
        assert_eq!(
            NotificationLevel::parse_lossy("INFO"),
            Some(NotificationLevel::Info)
        );
        assert_eq!(
            NotificationLevel::parse_lossy("warning"),
            Some(NotificationLevel::Warning)
        );
        assert_eq!(
            NotificationLevel::parse_lossy("critical"),
            Some(NotificationLevel::Error)
        );
        assert_eq!(
            NotificationLevel::parse_lossy("error"),
            Some(NotificationLevel::Error)
        );
        assert_eq!(NotificationLevel::parse_lossy("bogus"), None);
    }

    // -- Config parsing ------------------------------------------------------

    #[test]
    fn test_notifications_config_default_disabled() {
        let cfg = NotificationsConfig::default();
        assert!(!cfg.enabled);
        assert!(!cfg.sound);
        assert_eq!(cfg.minimum_level, NotificationLevel::Info);
    }

    #[test]
    fn test_notifications_config_interactive_default_enabled() {
        let cfg = NotificationsConfig::interactive_default();
        assert!(cfg.enabled);
        assert!(cfg.sound);
    }

    #[test]
    fn test_notifications_config_toml_parse_full() {
        let toml = r#"
enabled = true
sound = false
minimum_level = "warning"

[cooldown]
permission_ms = 0
query_complete_ms = 0
tool_complete_ms = 2500
error_ms = 4000
agent_idle_ms = 8000
"#;
        let cfg: NotificationsConfig = toml::from_str(toml).unwrap();
        assert!(cfg.enabled);
        assert!(!cfg.sound);
        assert_eq!(cfg.minimum_level, NotificationLevel::Warning);
        assert_eq!(cfg.cooldown.tool_complete_ms, 2500);
        assert_eq!(cfg.cooldown.error_ms, 4000);
        assert_eq!(cfg.cooldown.agent_idle_ms, 8000);
    }

    #[test]
    fn test_notifications_config_toml_uses_defaults_for_missing_fields() {
        let toml = r#"
enabled = true
"#;
        let cfg: NotificationsConfig = toml::from_str(toml).unwrap();
        assert!(cfg.enabled);
        assert!(cfg.sound); // default
        assert_eq!(cfg.cooldown.tool_complete_ms, 3000); // default
        assert_eq!(cfg.cooldown.error_ms, 5000); // default
    }

    #[test]
    fn test_notification_with_source_serializes() {
        let n = Notification {
            title: "t".into(),
            body: "b".into(),
            level: NotificationLevel::Error,
            id: "id1".into(),
            timestamp: Utc::now(),
            source: Some("tool:Edit".into()),
            action_id: Some("approve:perm_42".into()),
        };
        let json = serde_json::to_string(&n).unwrap();
        assert!(json.contains("\"source\":\"tool:Edit\""));
        assert!(json.contains("\"action_id\":\"approve:perm_42\""));
    }
}
