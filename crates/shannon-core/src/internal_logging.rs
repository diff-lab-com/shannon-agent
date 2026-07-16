//! # Internal Logging
//!
//! Structured internal logging with Kubernetes namespace detection.
//! Provides in-memory log entries for debugging and diagnostics.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

/// Log level for internal log entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InternalLogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for InternalLogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InternalLogLevel::Debug => write!(f, "DEBUG"),
            InternalLogLevel::Info => write!(f, "INFO"),
            InternalLogLevel::Warn => write!(f, "WARN"),
            InternalLogLevel::Error => write!(f, "ERROR"),
        }
    }
}

impl From<&str> for InternalLogLevel {
    fn from(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "DEBUG" => InternalLogLevel::Debug,
            "INFO" => InternalLogLevel::Info,
            "WARN" => InternalLogLevel::Warn,
            "ERROR" => InternalLogLevel::Error,
            _ => InternalLogLevel::Info,
        }
    }
}

/// A single structured log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalLogEntry {
    /// ISO 8601 timestamp of the log entry.
    pub timestamp: String,
    /// Log level.
    pub level: String,
    /// Component that generated the log (e.g., "query_engine", "api").
    pub component: String,
    /// Human-readable log message.
    pub message: String,
    /// Additional structured metadata.
    pub metadata: HashMap<String, Value>,
}

impl InternalLogEntry {
    /// Create a new log entry.
    pub fn new(level: InternalLogLevel, component: &str, message: &str) -> Self {
        Self {
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: level.to_string(),
            component: component.to_string(),
            message: message.to_string(),
            metadata: HashMap::new(),
        }
    }

    /// Create a new log entry with metadata.
    pub fn with_metadata(
        level: InternalLogLevel,
        component: &str,
        message: &str,
        metadata: HashMap<String, Value>,
    ) -> Self {
        Self {
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: level.to_string(),
            component: component.to_string(),
            message: message.to_string(),
            metadata,
        }
    }
}

/// Structured internal logger with in-memory storage and K8s namespace detection.
///
/// Stores log entries in memory and can detect whether the application
/// is running inside a Kubernetes environment.
pub struct InternalLogger {
    /// In-memory log entries storage.
    entries: Mutex<Vec<InternalLogEntry>>,
    /// Detected Kubernetes namespace, if running in K8s.
    k8s_namespace: Mutex<Option<String>>,
}

impl InternalLogger {
    /// Create a new internal logger.
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            k8s_namespace: Mutex::new(None),
        }
    }

    /// Log a message at the given level.
    pub fn log(&self, level: InternalLogLevel, component: &str, message: &str) {
        let entry = InternalLogEntry::new(level, component, message);
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(entry);
        }
    }

    /// Log a message with additional metadata.
    pub fn log_with_metadata(
        &self,
        level: InternalLogLevel,
        component: &str,
        message: &str,
        metadata: HashMap<String, Value>,
    ) {
        let entry = InternalLogEntry::with_metadata(level, component, message, metadata);
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(entry);
        }
    }

    /// Get all stored log entries.
    pub fn get_entries(&self) -> Vec<InternalLogEntry> {
        self.entries
            .lock()
            .map(|entries| entries.clone())
            .unwrap_or_default()
    }

    /// Clear all stored log entries.
    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }

    /// Detect the Kubernetes namespace the application is running in.
    ///
    /// Checks the standard Kubernetes environment variables and service account
    /// files to determine if the application is running inside a pod.
    /// Returns `Some(namespace)` if in K8s, `None` otherwise.
    pub fn detect_k8s_namespace(&self) -> Option<String> {
        // Check cached value first
        if let Ok(namespace) = self.k8s_namespace.lock() {
            if namespace.is_some() {
                return namespace.clone();
            }
        }

        // Check KUBERNETES_SERVICE_HOST (set in all K8s pods)
        if std::env::var("KUBERNETES_SERVICE_HOST").is_ok() {
            // Try to read namespace from service account
            let namespace = Self::read_k8s_namespace_from_file();
            if let Some(ref ns) = namespace {
                if let Ok(mut cached) = self.k8s_namespace.lock() {
                    *cached = Some(ns.clone());
                }
            }
            return namespace;
        }

        None
    }

    /// Read the K8s namespace from the service account namespace file.
    fn read_k8s_namespace_from_file() -> Option<String> {
        let namespace_path = "/var/run/secrets/kubernetes.io/serviceaccount/namespace";
        std::fs::read_to_string(namespace_path)
            .ok()
            .map(|s| s.trim().to_string())
    }

    /// Get the number of stored log entries.
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    /// Check if there are no log entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get log entries filtered by level.
    pub fn get_entries_by_level(&self, level: InternalLogLevel) -> Vec<InternalLogEntry> {
        self.get_entries()
            .into_iter()
            .filter(|e| e.level == level.to_string())
            .collect()
    }

    /// Get log entries filtered by component.
    pub fn get_entries_by_component(&self, component: &str) -> Vec<InternalLogEntry> {
        self.get_entries()
            .into_iter()
            .filter(|e| e.component == component)
            .collect()
    }
}

impl Default for InternalLogger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_logger_new() {
        let logger = InternalLogger::new();
        assert!(logger.is_empty());
    }

    #[test]
    fn test_logger_default() {
        let logger = InternalLogger::default();
        assert_eq!(logger.len(), 0);
    }

    #[test]
    fn test_log_single_entry() {
        let logger = InternalLogger::new();
        logger.log(InternalLogLevel::Info, "test_component", "test message");
        assert_eq!(logger.len(), 1);

        let entries = logger.get_entries();
        assert_eq!(entries[0].level, "INFO");
        assert_eq!(entries[0].component, "test_component");
        assert_eq!(entries[0].message, "test message");
        assert!(entries[0].metadata.is_empty());
    }

    #[test]
    fn test_log_multiple_entries() {
        let logger = InternalLogger::new();
        logger.log(InternalLogLevel::Info, "comp1", "message 1");
        logger.log(InternalLogLevel::Warn, "comp2", "message 2");
        logger.log(InternalLogLevel::Error, "comp1", "message 3");
        assert_eq!(logger.len(), 3);
    }

    #[test]
    fn test_log_with_metadata() {
        let logger = InternalLogger::new();
        let mut metadata = HashMap::new();
        metadata.insert(
            "request_id".to_string(),
            Value::String("abc-123".to_string()),
        );
        metadata.insert("duration_ms".to_string(), Value::Number(42.into()));

        logger.log_with_metadata(
            InternalLogLevel::Debug,
            "api",
            "Request completed",
            metadata,
        );

        let entries = logger.get_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].metadata.get("request_id").unwrap(), "abc-123");
        assert_eq!(entries[0].metadata.get("duration_ms").unwrap(), 42);
    }

    #[test]
    fn test_clear() {
        let logger = InternalLogger::new();
        logger.log(InternalLogLevel::Info, "comp", "msg1");
        logger.log(InternalLogLevel::Info, "comp", "msg2");
        assert_eq!(logger.len(), 2);

        logger.clear();
        assert!(logger.is_empty());
    }

    #[test]
    fn test_get_entries_by_level() {
        let logger = InternalLogger::new();
        logger.log(InternalLogLevel::Info, "comp", "info msg");
        logger.log(InternalLogLevel::Error, "comp", "error msg");
        logger.log(InternalLogLevel::Info, "comp", "info msg 2");

        let info_entries = logger.get_entries_by_level(InternalLogLevel::Info);
        assert_eq!(info_entries.len(), 2);

        let error_entries = logger.get_entries_by_level(InternalLogLevel::Error);
        assert_eq!(error_entries.len(), 1);
    }

    #[test]
    fn test_get_entries_by_component() {
        let logger = InternalLogger::new();
        logger.log(InternalLogLevel::Info, "engine", "msg1");
        logger.log(InternalLogLevel::Info, "api", "msg2");
        logger.log(InternalLogLevel::Info, "engine", "msg3");

        let engine_entries = logger.get_entries_by_component("engine");
        assert_eq!(engine_entries.len(), 2);

        let api_entries = logger.get_entries_by_component("api");
        assert_eq!(api_entries.len(), 1);
    }

    #[test]
    fn test_log_level_display() {
        assert_eq!(format!("{}", InternalLogLevel::Debug), "DEBUG");
        assert_eq!(format!("{}", InternalLogLevel::Info), "INFO");
        assert_eq!(format!("{}", InternalLogLevel::Warn), "WARN");
        assert_eq!(format!("{}", InternalLogLevel::Error), "ERROR");
    }

    #[test]
    fn test_log_level_from_str() {
        assert_eq!(InternalLogLevel::from("debug"), InternalLogLevel::Debug);
        assert_eq!(InternalLogLevel::from("INFO"), InternalLogLevel::Info);
        assert_eq!(InternalLogLevel::from("Warn"), InternalLogLevel::Warn);
        assert_eq!(InternalLogLevel::from("ERROR"), InternalLogLevel::Error);
        assert_eq!(InternalLogLevel::from("unknown"), InternalLogLevel::Info);
    }

    #[test]
    fn test_log_entry_timestamp_format() {
        let entry = InternalLogEntry::new(InternalLogLevel::Info, "comp", "msg");
        // Should be parseable as ISO 8601
        let parsed = chrono::DateTime::parse_from_rfc3339(&entry.timestamp);
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_log_entry_serialization() {
        let mut metadata = HashMap::new();
        metadata.insert("key".to_string(), Value::String("value".to_string()));
        let entry = InternalLogEntry::with_metadata(
            InternalLogLevel::Error,
            "engine",
            "something failed",
            metadata,
        );

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("ERROR"));
        assert!(json.contains("engine"));
        assert!(json.contains("something failed"));
        assert!(json.contains("key"));
    }

    #[test]
    fn test_detect_k8s_namespace_outside_k8s() {
        let logger = InternalLogger::new();
        // Outside K8s, should return None (unless KUBERNETES_SERVICE_HOST is set)
        let result = logger.detect_k8s_namespace();
        // This test assumes we're not running inside K8s
        // If the env var happens to be set, skip the assertion
        if std::env::var("KUBERNETES_SERVICE_HOST").is_err() {
            assert!(result.is_none());
        }
    }

    #[test]
    fn test_k8s_namespace_caching() {
        let logger = InternalLogger::new();
        // First call
        let result1 = logger.detect_k8s_namespace();
        // Second call should return cached value
        let result2 = logger.detect_k8s_namespace();
        assert_eq!(result1, result2);
    }
}
