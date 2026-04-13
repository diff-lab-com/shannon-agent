//! # Diagnostic Tracking System
//!
//! Error tracking, pattern analysis, and diagnostic event management.
//!
//! ## Overview
//!
//! - [`DiagnosticTracker`]: Central tracker for recording and analyzing diagnostic events
//! - [`DiagnosticEvent`]: Individual diagnostic event with rich context
//! - [`ErrorPattern`]: Detected error pattern from recurring issues
//! - [`DiagnosticSummary`]: Aggregated view of diagnostic state
//!
//! ## Usage
//!
//! ```rust
//! use shannon_core::diagnostics::{DiagnosticTracker, DiagnosticLevel, DiagnosticCategory};
//!
//! let mut tracker = DiagnosticTracker::new();
//! tracker.record_error("connection failed", DiagnosticCategory::Network, Default::default());
//!
//! let summary = tracker.get_summary();
//! println!("Total events: {}", summary.total_events);
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// DiagnosticLevel
// ---------------------------------------------------------------------------

/// Severity level for diagnostic events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum DiagnosticLevel {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

impl DiagnosticLevel {
    /// Returns a human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
            Self::Critical => "CRITICAL",
        }
    }
}

impl std::fmt::Display for DiagnosticLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ---------------------------------------------------------------------------
// DiagnosticCategory
// ---------------------------------------------------------------------------

/// Broad category of a diagnostic event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiagnosticCategory {
    Compilation,
    Runtime,
    Tool,
    Permission,
    Network,
    FileSystem,
}

impl DiagnosticCategory {
    /// Returns a human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Compilation => "COMPILATION",
            Self::Runtime => "RUNTIME",
            Self::Tool => "TOOL",
            Self::Permission => "PERMISSION",
            Self::Network => "NETWORK",
            Self::FileSystem => "FILE_SYSTEM",
        }
    }
}

impl std::fmt::Display for DiagnosticCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ---------------------------------------------------------------------------
// DiagnosticEvent
// ---------------------------------------------------------------------------

/// A single diagnostic event with rich context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticEvent {
    /// Unique identifier (UUID).
    pub id: String,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
    /// Severity level.
    pub level: DiagnosticLevel,
    /// Category of the event.
    pub category: DiagnosticCategory,
    /// Human-readable message.
    pub message: String,
    /// Arbitrary key-value context attached to the event.
    pub context: HashMap<String, Value>,
    /// Optional stack trace.
    pub stack_trace: Option<String>,
    /// Source file path, if applicable.
    pub file_path: Option<String>,
    /// Source line number, if applicable.
    pub line_number: Option<u32>,
}

impl DiagnosticEvent {
    /// Create a new diagnostic event with the given fields.
    pub fn new(
        level: DiagnosticLevel,
        category: DiagnosticCategory,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            level,
            category,
            message: message.into(),
            context: HashMap::new(),
            stack_trace: None,
            file_path: None,
            line_number: None,
        }
    }

    /// Builder-style setter for context.
    pub fn with_context(mut self, key: impl Into<String>, value: Value) -> Self {
        self.context.insert(key.into(), value);
        self
    }

    /// Builder-style setter for stack trace.
    pub fn with_stack_trace(mut self, trace: impl Into<String>) -> Self {
        self.stack_trace = Some(trace.into());
        self
    }

    /// Builder-style setter for source location.
    pub fn with_location(mut self, file: impl Into<String>, line: u32) -> Self {
        self.file_path = Some(file.into());
        self.line_number = Some(line);
        self
    }
}

// ---------------------------------------------------------------------------
// ErrorPattern
// ---------------------------------------------------------------------------

/// A detected pattern among recurring diagnostic events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPattern {
    /// Normalized (abstracted) representation of the error message.
    pub pattern: String,
    /// How many times this pattern has been observed.
    pub count: usize,
    /// First occurrence.
    pub first_seen: DateTime<Utc>,
    /// Most recent occurrence.
    pub last_seen: DateTime<Utc>,
    /// Category that the pattern belongs to.
    pub category: DiagnosticCategory,
    /// A suggested fix, if one could be inferred.
    pub suggested_fix: Option<String>,
}

// ---------------------------------------------------------------------------
// DiagnosticSummary
// ---------------------------------------------------------------------------

/// Aggregated diagnostic state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticSummary {
    /// Total number of recorded events.
    pub total_events: usize,
    /// Breakdown by severity level.
    pub by_level: HashMap<DiagnosticLevel, usize>,
    /// Breakdown by category.
    pub by_category: HashMap<DiagnosticCategory, usize>,
    /// Detected error patterns (recurring issues).
    pub error_patterns: Vec<ErrorPattern>,
    /// Most recent events (last 10).
    pub most_recent: Vec<DiagnosticEvent>,
}

// ---------------------------------------------------------------------------
// DiagnosticTracker
// ---------------------------------------------------------------------------

const DEFAULT_MAX_EVENTS: usize = 1000;

/// Central tracker for recording and analyzing diagnostic events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticTracker {
    events: Vec<DiagnosticEvent>,
    max_events: usize,
    storage_path: Option<PathBuf>,
}

impl Default for DiagnosticTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosticTracker {
    /// Create a new tracker with default capacity (1000 events).
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            max_events: DEFAULT_MAX_EVENTS,
            storage_path: None,
        }
    }

    /// Create a tracker with a custom max event capacity.
    pub fn with_capacity(max_events: usize) -> Self {
        Self {
            events: Vec::with_capacity(max_events),
            max_events,
            storage_path: None,
        }
    }

    /// Set the file path used by [`save`](Self::save) and [`load`](Self::load).
    pub fn with_storage_path(mut self, path: PathBuf) -> Self {
        self.storage_path = Some(path);
        self
    }

    // ---- Recording --------------------------------------------------------

    /// Record a diagnostic event.
    ///
    /// When the buffer is full the oldest event is evicted (FIFO).
    pub fn record(&mut self, event: DiagnosticEvent) {
        if self.events.len() >= self.max_events {
            self.events.remove(0);
        }
        self.events.push(event);
    }

    /// Convenience method: record an error-level event.
    pub fn record_error(
        &mut self,
        message: impl Into<String>,
        category: DiagnosticCategory,
        context: HashMap<String, Value>,
    ) {
        let mut event = DiagnosticEvent::new(DiagnosticLevel::Error, category, message);
        event.context = context;
        self.record(event);
    }

    /// Convenience method: record a warning-level event.
    pub fn record_warning(
        &mut self,
        message: impl Into<String>,
        category: DiagnosticCategory,
        context: HashMap<String, Value>,
    ) {
        let mut event = DiagnosticEvent::new(DiagnosticLevel::Warning, category, message);
        event.context = context;
        self.record(event);
    }

    /// Convenience method: record an info-level event.
    pub fn record_info(
        &mut self,
        message: impl Into<String>,
        category: DiagnosticCategory,
        context: HashMap<String, Value>,
    ) {
        let mut event = DiagnosticEvent::new(DiagnosticLevel::Info, category, message);
        event.context = context;
        self.record(event);
    }

    // ---- Queries ----------------------------------------------------------

    /// Return the most recent *count* events (newest first).
    pub fn get_recent(&self, count: usize) -> Vec<DiagnosticEvent> {
        self.events
            .iter()
            .rev()
            .take(count)
            .cloned()
            .collect()
    }

    /// Return all events matching a given severity level.
    pub fn get_by_level(&self, level: DiagnosticLevel) -> Vec<DiagnosticEvent> {
        self.events
            .iter()
            .filter(|e| e.level == level)
            .cloned()
            .collect()
    }

    /// Return all events matching a given category.
    pub fn get_by_category(&self, category: DiagnosticCategory) -> Vec<DiagnosticEvent> {
        self.events
            .iter()
            .filter(|e| e.category == category)
            .cloned()
            .collect()
    }

    /// Analyze recurring errors and return detected patterns.
    pub fn get_error_patterns(&self) -> Vec<ErrorPattern> {
        let mut pattern_map: HashMap<(String, DiagnosticCategory), ErrorPatternAccum> =
            HashMap::new();

        for event in &self.events {
            // Only analyze error and critical events for patterns.
            if event.level != DiagnosticLevel::Error && event.level != DiagnosticLevel::Critical {
                continue;
            }

            let normalized = normalize_message(&event.message);
            let key = (normalized, event.category);

            let accum = pattern_map.entry(key).or_insert_with(|| ErrorPatternAccum {
                first_seen: event.timestamp,
                last_seen: event.timestamp,
                count: 0,
            });

            accum.count += 1;
            if event.timestamp < accum.first_seen {
                accum.first_seen = event.timestamp;
            }
            if event.timestamp > accum.last_seen {
                accum.last_seen = event.timestamp;
            }
        }

        pattern_map
            .into_iter()
            .map(|((pattern, category), accum)| {
                let suggested_fix = suggest_fix(&pattern);
                ErrorPattern {
                    pattern,
                    count: accum.count,
                    first_seen: accum.first_seen,
                    last_seen: accum.last_seen,
                    category,
                    suggested_fix,
                }
            })
            .collect()
    }

    /// Produce an aggregated summary of all recorded events.
    pub fn get_summary(&self) -> DiagnosticSummary {
        let mut by_level: HashMap<DiagnosticLevel, usize> = HashMap::new();
        let mut by_category: HashMap<DiagnosticCategory, usize> = HashMap::new();

        for event in &self.events {
            *by_level.entry(event.level).or_insert(0) += 1;
            *by_category.entry(event.category).or_insert(0) += 1;
        }

        let most_recent = self.get_recent(10);
        let error_patterns = self.get_error_patterns();

        DiagnosticSummary {
            total_events: self.events.len(),
            by_level,
            by_category,
            error_patterns,
            most_recent,
        }
    }

    /// Remove all recorded events.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Return the number of currently stored events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Return true if no events are stored.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Return the configured max capacity.
    pub fn max_events(&self) -> usize {
        self.max_events
    }

    // ---- Persistence ------------------------------------------------------

    /// Persist the event log to the configured storage path as JSON.
    ///
    /// Returns an error if no storage path has been set.
    pub fn save(&self) -> std::io::Result<()> {
        let path = self
            .storage_path
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no storage path set"))?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)
    }

    /// Load events from the configured storage path (replaces current state).
    ///
    /// Returns an error if no storage path has been set or the file cannot be read/parsed.
    pub fn load(&mut self) -> std::io::Result<()> {
        let path = self
            .storage_path
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no storage path set"))?;
        let data = std::fs::read_to_string(path)?;
        let loaded: DiagnosticTracker = serde_json::from_str(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.events = loaded.events;
        self.max_events = loaded.max_events;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

struct ErrorPatternAccum {
    first_seen: DateTime<Utc>,
    last_seen: DateTime<Utc>,
    count: usize,
}

/// Normalize an error message so that similar errors map to the same pattern.
///
/// Strips:
/// - Hex addresses (e.g. `0x7f3a...`)
/// - File paths that look like Unix/Windows paths
/// - Line numbers after `:line` or `:col:line`
/// - UUIDs
/// - Numeric IDs that look variable
fn normalize_message(message: &str) -> String {
    let mut result = message.to_string();

    // Strip hex addresses like 0x7f3a4b2c (run early to protect hex digits)
    let hex_re = Regex::new(r"0x[0-9a-fA-F]{4,}").expect("hex regex should be valid");
    result = hex_re.replace_all(&result, "HEXADDR").to_string();

    // Strip UUIDs
    let uuid_re = Regex::new(
        r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
    )
    .expect("uuid regex should be valid");
    result = uuid_re.replace_all(&result, "<UUID>").to_string();

    // Strip absolute Unix paths: /home/user/... or /usr/lib/...
    let unix_path_re = Regex::new(r"(?:/[\w._-]+){2,}").expect("unix path regex should be valid");
    result = unix_path_re.replace_all(&result, "<PATH>").to_string();

    // Strip Windows paths: C:\Users\...
    let win_path_re = Regex::new(r"[A-Za-z]:\\(?:[\w._-]+\\?)+").expect("windows path regex should be valid");
    result = win_path_re.replace_all(&result, "<PATH>").to_string();

    // Strip line:col references (e.g. `:42` or `:42:10`)
    let line_col_re = Regex::new(r":\d{1,5}(:\d{1,5})?").expect("line:col regex should be valid");
    result = line_col_re.replace_all(&result, ":<NUM>").to_string();

    // Restore hex address markers (after line-col so digits aren't consumed)
    result = result.replace("HEXADDR", "0xHEX");

    // Normalize backtick-quoted identifiers (e.g. `module_name` -> `<IDENT>`)
    let ident_re = Regex::new(r"`[^`]+`").expect("identifier regex should be valid");
    result = ident_re.replace_all(&result, "`<IDENT>`").to_string();

    // Normalize double-quoted strings to group similar errors
    let str_re = Regex::new(r#""[^"]+""#).expect("string regex should be valid");
    result = str_re.replace_all(&result, "\"<STR>\"").to_string();

    result
}

/// Provide a simple suggested fix based on the normalized error pattern.
fn suggest_fix(pattern: &str) -> Option<String> {
    let lower = pattern.to_lowercase();

    let fixes: &[(&str, &str)] = &[
        ("cannot find module", "Check that the module exists and is listed in your imports or Cargo.toml."),
        ("file not found", "Verify the file path and ensure the file exists."),
        ("permission denied", "Check file/directory permissions or run with appropriate privileges."),
        ("connection refused", "Ensure the target service is running and reachable."),
        ("connection timed out", "Check network connectivity and firewall settings."),
        ("network unreachable", "Verify network configuration and DNS settings."),
        ("disk full", "Free up disk space or increase storage quota."),
        ("out of memory", "Reduce memory usage or increase available RAM/swap."),
        ("stack overflow", "Check for infinite recursion in your code."),
        ("type mismatch", "Verify that types match expected signatures."),
        ("cannot borrow.*mutably", "Review ownership rules; consider using RefCell or restructuring borrows."),
        ("timeout", "Increase timeout duration or check if the operation is blocking."),
        ("authentication failed", "Verify credentials and API key configuration."),
        ("rate limit", "Reduce request frequency or implement exponential backoff."),
        ("not found", "Check that the requested resource exists and the identifier is correct."),
        ("already exists", "Use a different name or remove the existing resource first."),
        ("invalid argument", "Review the function arguments and expected types."),
        ("eof", "The stream ended unexpectedly; check for premature closures or truncated data."),
        ("utf-8 error", "Ensure input data is valid UTF-8 encoding."),
        ("serde", "Check that the data structure matches the expected serialization format."),
    ];

    for (keyword, fix) in fixes {
        if lower.contains(keyword) {
            return Some((*fix).to_string());
        }
    }

    None
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ---- DiagnosticEvent tests --------------------------------------------

    #[test]
    fn test_diagnostic_event_new() {
        let event = DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Runtime, "test error");
        assert!(!event.id.is_empty());
        assert_eq!(event.level, DiagnosticLevel::Error);
        assert_eq!(event.category, DiagnosticCategory::Runtime);
        assert_eq!(event.message, "test error");
        assert!(event.context.is_empty());
        assert!(event.stack_trace.is_none());
        assert!(event.file_path.is_none());
        assert!(event.line_number.is_none());
    }

    #[test]
    fn test_diagnostic_event_builder() {
        let event = DiagnosticEvent::new(DiagnosticLevel::Warning, DiagnosticCategory::Tool, "oops")
            .with_context("key", Value::String("val".into()))
            .with_stack_trace("at main.rs:10")
            .with_location("src/main.rs", 42);

        assert_eq!(event.context.get("key").unwrap(), &Value::String("val".into()));
        assert_eq!(event.stack_trace.as_deref(), Some("at main.rs:10"));
        assert_eq!(event.file_path.as_deref(), Some("src/main.rs"));
        assert_eq!(event.line_number, Some(42));
    }

    #[test]
    fn test_diagnostic_event_has_unique_id() {
        let a = DiagnosticEvent::new(DiagnosticLevel::Info, DiagnosticCategory::Network, "a");
        let b = DiagnosticEvent::new(DiagnosticLevel::Info, DiagnosticCategory::Network, "b");
        assert_ne!(a.id, b.id);
    }

    // ---- DiagnosticLevel tests --------------------------------------------

    #[test]
    fn test_diagnostic_level_ordering() {
        assert!(DiagnosticLevel::Critical > DiagnosticLevel::Error);
        assert!(DiagnosticLevel::Error > DiagnosticLevel::Warning);
        assert!(DiagnosticLevel::Warning > DiagnosticLevel::Info);
        assert!(DiagnosticLevel::Info > DiagnosticLevel::Debug);
    }

    #[test]
    fn test_diagnostic_level_display() {
        assert_eq!(DiagnosticLevel::Debug.to_string(), "DEBUG");
        assert_eq!(DiagnosticLevel::Critical.to_string(), "CRITICAL");
    }

    // ---- DiagnosticCategory tests -----------------------------------------

    #[test]
    fn test_diagnostic_category_display() {
        assert_eq!(DiagnosticCategory::FileSystem.to_string(), "FILE_SYSTEM");
        assert_eq!(DiagnosticCategory::Tool.to_string(), "TOOL");
    }

    // ---- DiagnosticTracker basics -----------------------------------------

    #[test]
    fn test_tracker_new() {
        let tracker = DiagnosticTracker::new();
        assert!(tracker.is_empty());
        assert_eq!(tracker.max_events(), 1000);
    }

    #[test]
    fn test_tracker_with_capacity() {
        let tracker = DiagnosticTracker::with_capacity(50);
        assert_eq!(tracker.max_events(), 50);
    }

    #[test]
    fn test_tracker_record_and_len() {
        let mut tracker = DiagnosticTracker::new();
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Info, DiagnosticCategory::Runtime, "first"));
        assert_eq!(tracker.len(), 1);
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Tool, "second"));
        assert_eq!(tracker.len(), 2);
    }

    #[test]
    fn test_tracker_record_eviction() {
        let mut tracker = DiagnosticTracker::with_capacity(3);
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Info, DiagnosticCategory::Runtime, "a"));
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Info, DiagnosticCategory::Runtime, "b"));
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Info, DiagnosticCategory::Runtime, "c"));
        assert_eq!(tracker.len(), 3);

        // Adding a 4th event evicts "a".
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Info, DiagnosticCategory::Runtime, "d"));
        assert_eq!(tracker.len(), 3);
        assert_eq!(tracker.get_recent(3)[0].message, "d");
        assert_eq!(tracker.get_recent(3)[2].message, "b");
    }

    #[test]
    fn test_tracker_clear() {
        let mut tracker = DiagnosticTracker::new();
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Runtime, "x"));
        tracker.clear();
        assert!(tracker.is_empty());
    }

    // ---- Convenience recording --------------------------------------------

    #[test]
    fn test_record_error() {
        let mut tracker = DiagnosticTracker::new();
        let mut ctx = HashMap::new();
        ctx.insert("code".to_string(), Value::Number(500.into()));
        tracker.record_error("server error", DiagnosticCategory::Network, ctx);

        let events = tracker.get_by_level(DiagnosticLevel::Error);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "server error");
        assert_eq!(events[0].category, DiagnosticCategory::Network);
    }

    #[test]
    fn test_record_warning() {
        let mut tracker = DiagnosticTracker::new();
        tracker.record_warning("low disk", DiagnosticCategory::FileSystem, HashMap::new());
        assert_eq!(tracker.get_by_level(DiagnosticLevel::Warning).len(), 1);
    }

    #[test]
    fn test_record_info() {
        let mut tracker = DiagnosticTracker::new();
        tracker.record_info("started", DiagnosticCategory::Runtime, HashMap::new());
        assert_eq!(tracker.get_by_level(DiagnosticLevel::Info).len(), 1);
    }

    // ---- Queries ----------------------------------------------------------

    #[test]
    fn test_get_recent() {
        let mut tracker = DiagnosticTracker::new();
        for i in 0..5 {
            tracker.record(DiagnosticEvent::new(DiagnosticLevel::Info, DiagnosticCategory::Runtime, format!("msg_{i}")));
        }
        let recent = tracker.get_recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].message, "msg_4");
        assert_eq!(recent[1].message, "msg_3");
        assert_eq!(recent[2].message, "msg_2");
    }

    #[test]
    fn test_get_recent_larger_than_buffer() {
        let mut tracker = DiagnosticTracker::new();
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Info, DiagnosticCategory::Runtime, "only"));
        let recent = tracker.get_recent(100);
        assert_eq!(recent.len(), 1);
    }

    #[test]
    fn test_get_by_level() {
        let mut tracker = DiagnosticTracker::new();
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Runtime, "e1"));
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Info, DiagnosticCategory::Runtime, "i1"));
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Tool, "e2"));

        let errors = tracker.get_by_level(DiagnosticLevel::Error);
        assert_eq!(errors.len(), 2);
        let infos = tracker.get_by_level(DiagnosticLevel::Info);
        assert_eq!(infos.len(), 1);
    }

    #[test]
    fn test_get_by_category() {
        let mut tracker = DiagnosticTracker::new();
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Network, "n1"));
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Runtime, "r1"));
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Warning, DiagnosticCategory::Network, "n2"));

        let net = tracker.get_by_category(DiagnosticCategory::Network);
        assert_eq!(net.len(), 2);
    }

    // ---- Error patterns ---------------------------------------------------

    #[test]
    fn test_error_patterns_basic() {
        let mut tracker = DiagnosticTracker::new();
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Runtime, "cannot find module `foo`"));
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Runtime, "cannot find module `bar`"));
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Info, DiagnosticCategory::Runtime, "started")); // info ignored

        let patterns = tracker.get_error_patterns();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].count, 2);
        assert_eq!(patterns[0].category, DiagnosticCategory::Runtime);
        assert!(patterns[0].suggested_fix.is_some());
    }

    #[test]
    fn test_error_patterns_different_categories() {
        let mut tracker = DiagnosticTracker::new();
        // Same normalized pattern, same category -> should group
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Network, "connection refused for /home/alice/config"));
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Network, "connection refused for /home/bob/config"));
        // Same text but different category -> separate pattern
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::FileSystem, "connection refused"));

        let patterns = tracker.get_error_patterns();
        assert_eq!(patterns.len(), 2);
    }

    #[test]
    fn test_error_patterns_critical_also_counted() {
        let mut tracker = DiagnosticTracker::new();
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Critical, DiagnosticCategory::Runtime, "disk full on /dev/sda1"));
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Critical, DiagnosticCategory::Runtime, "disk full on /dev/sda2"));

        let patterns = tracker.get_error_patterns();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].count, 2);
    }

    // ---- Summary ----------------------------------------------------------

    #[test]
    fn test_summary_basic() {
        let mut tracker = DiagnosticTracker::new();
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Network, "err"));
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Info, DiagnosticCategory::Runtime, "info"));
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Warning, DiagnosticCategory::Tool, "warn"));

        let summary = tracker.get_summary();
        assert_eq!(summary.total_events, 3);
        assert_eq!(*summary.by_level.get(&DiagnosticLevel::Error).unwrap(), 1);
        assert_eq!(*summary.by_level.get(&DiagnosticLevel::Info).unwrap(), 1);
        assert_eq!(*summary.by_level.get(&DiagnosticLevel::Warning).unwrap(), 1);
        assert_eq!(*summary.by_category.get(&DiagnosticCategory::Network).unwrap(), 1);
    }

    #[test]
    fn test_summary_empty() {
        let tracker = DiagnosticTracker::new();
        let summary = tracker.get_summary();
        assert_eq!(summary.total_events, 0);
        assert!(summary.by_level.is_empty());
        assert!(summary.error_patterns.is_empty());
    }

    // ---- Persistence ------------------------------------------------------

    #[test]
    fn test_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diagnostics.json");

        let mut tracker = DiagnosticTracker::new().with_storage_path(path.clone());
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Runtime, "persist me"));
        tracker.save().unwrap();

        let mut loaded = DiagnosticTracker::new().with_storage_path(path.clone());
        loaded.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.events[0].message, "persist me");
    }

    #[test]
    fn test_save_no_storage_path() {
        let tracker = DiagnosticTracker::new();
        let result = tracker.save();
        assert!(result.is_err());
    }

    #[test]
    fn test_load_no_storage_path() {
        let mut tracker = DiagnosticTracker::new();
        let result = tracker.load();
        assert!(result.is_err());
    }

    #[test]
    fn test_load_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let mut tracker = DiagnosticTracker::new().with_storage_path(path);
        let result = tracker.load();
        assert!(result.is_err());
    }

    // ---- Normalize message ------------------------------------------------

    #[test]
    fn test_normalize_hex_addresses() {
        assert_eq!(
            normalize_message("segfault at 0x7f3a4b2c"),
            "segfault at 0xHEX"
        );
    }

    #[test]
    fn test_normalize_uuids() {
        assert_eq!(
            normalize_message("user 550e8400-e29b-41d4-a716-446655440000 not found"),
            "user <UUID> not found"
        );
    }

    #[test]
    fn test_normalize_unix_paths() {
        assert_eq!(
            normalize_message("error in /home/user/project/src/main.rs"),
            "error in <PATH>"
        );
    }

    #[test]
    fn test_normalize_line_numbers() {
        assert_eq!(
            normalize_message("error at main.rs:42:10"),
            "error at main.rs:<NUM>"
        );
    }

    #[test]
    fn test_normalize_combined() {
        let msg = "panic at /home/alice/project/src/lib.rs:127:0x55aa1234";
        let norm = normalize_message(msg);
        assert!(norm.contains("<PATH>"));
        assert!(norm.contains("0xHEX"));
        assert!(norm.contains("<NUM>"));
    }

    // ---- Suggest fix ------------------------------------------------------

    #[test]
    fn test_suggest_fix_cannot_find_module() {
        assert_eq!(
            suggest_fix("cannot find module `foo`"),
            Some("Check that the module exists and is listed in your imports or Cargo.toml.".to_string())
        );
    }

    #[test]
    fn test_suggest_fix_connection_refused() {
        assert_eq!(
            suggest_fix("connection refused to 192.168.1.1:8080"),
            Some("Ensure the target service is running and reachable.".to_string())
        );
    }

    #[test]
    fn test_suggest_fix_permission_denied() {
        assert_eq!(
            suggest_fix("Permission denied while opening file"),
            Some("Check file/directory permissions or run with appropriate privileges.".to_string())
        );
    }

    #[test]
    fn test_suggest_fix_unknown() {
        assert_eq!(suggest_fix("something completely unexpected"), None);
    }

    #[test]
    fn test_suggest_fix_rate_limit() {
        assert_eq!(
            suggest_fix("API rate limit exceeded"),
            Some("Reduce request frequency or implement exponential backoff.".to_string())
        );
    }

    // ---- Serialization ----------------------------------------------------

    #[test]
    fn test_diagnostic_event_serialization_roundtrip() {
        let mut ctx = HashMap::new();
        ctx.insert("key".to_string(), Value::Bool(true));
        let event = DiagnosticEvent::new(DiagnosticLevel::Critical, DiagnosticCategory::Permission, "denied")
            .with_context("key", Value::Bool(true))
            .with_location("/etc/config.toml", 10);

        let json = serde_json::to_string(&event).unwrap();
        let restored: DiagnosticEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, event.id);
        assert_eq!(restored.level, event.level);
        assert_eq!(restored.category, event.category);
        assert_eq!(restored.message, event.message);
        assert_eq!(restored.file_path, event.file_path);
        assert_eq!(restored.line_number, event.line_number);
    }

    #[test]
    fn test_tracker_serialization_roundtrip() {
        let mut tracker = DiagnosticTracker::with_capacity(5);
        tracker.record(DiagnosticEvent::new(DiagnosticLevel::Error, DiagnosticCategory::Runtime, "bug"));
        let json = serde_json::to_string(&tracker).unwrap();
        let restored: DiagnosticTracker = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored.max_events(), 5);
    }
}
