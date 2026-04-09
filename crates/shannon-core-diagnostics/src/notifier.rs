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
//! use shannon_core_diagnostics::notifier::{Notifier, LogNotifier};
//!
//! let mut notifier = Notifier::new();
//! notifier.add_handler(Box::new(LogNotifier::new()));
//! notifier.info("Hello", "World").unwrap();
//! ```

use chrono::{DateTime, Utc};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
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
    HandlerFailed {
        name: String,
        reason: String,
    },
}

// ============================================================================
// Core types
// ============================================================================

/// Severity level of a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationLevel {
    /// General informational notice.
    Info,
    /// An operation succeeded.
    Success,
    /// Something warrants attention but is not fatal.
    Warning,
    /// An operation failed.
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
}

impl Notifier {
    /// Create an empty `Notifier` with no handlers.
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
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
        let mut errors: Vec<String> = Vec::new();

        for handler in &self.handlers {
            if let Err(e) = handler.send(notification) {
                errors.push(format!("{}", e));
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
            title: format!("title-{}", id),
            body: format!("body-{}", id),
            level: NotificationLevel::Info,
            id: id.into(),
            timestamp: Utc::now(),
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
        };
        cb.send(&notification).unwrap();

        let titles = received.lock().unwrap();
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0], "cb-test");
    }

    #[test]
    fn test_callback_notifier_propagates_error() {
        let cb = CallbackNotifier::new(|_n: &Notification| -> Result<(), NotifierError> {
            Err(NotifierError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "boom",
            )))
        });

        let notification = Notification {
            title: "err".into(),
            body: "".into(),
            level: NotificationLevel::Error,
            id: "e1".into(),
            timestamp: Utc::now(),
        };
        let result = cb.send(&notification);
        assert!(result.is_err());
    }

    #[test]
    fn test_callback_notifier_custom_name() {
        let cb = CallbackNotifier::with_name(
            |_n: &Notification| Ok(()),
            "custom-cb",
        );
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
        n.add_handler(Box::new(CallbackNotifier::new(|_n| -> Result<(), NotifierError> {
            Err(NotifierError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "fail",
            )))
        })));
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
        };
        let json = serde_json::to_string(&n).unwrap();
        let back: Notification = serde_json::from_str(&json).unwrap();
        assert_eq!(back.title, n.title);
        assert_eq!(back.level, n.level);
        assert_eq!(back.id, n.id);
    }
}
