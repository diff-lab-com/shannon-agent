//! # Analytics: Usage Tracking and Analysis
//!
//! Records and aggregates usage events across Shannon sessions for insights
//! into tool usage, session patterns, and error rates.
//!
//! ## Architecture
//!
//! Events are stored in-memory and optionally persisted to disk as JSONL files
//! under `~/.shannon/analytics/{date}.jsonl` (one event per line).
//!
//! - [`AnalyticsEvent`]: Individual usage event
//! - [`AnalyticsEventType`]: Typed event discriminator
//! - [`AnalyticsStore`]: In-memory store with optional disk persistence
//! - [`ToolStats`]: Per-tool aggregated statistics
//! - [`SessionStats`]: Per-session aggregated statistics
//! - [`DailyStats`]: Per-day aggregated statistics
//!
//! ## Usage
//!
//! ```rust
//! use shannon_core::analytics::{AnalyticsStore, AnalyticsEventType};
//!
//! let mut store = AnalyticsStore::new();
//! store.record(AnalyticsEventType::SessionStart, Default::default());
//!
//! let summary = store.summary();
//! println!("Total events: {}", summary.total_events);
//! ```

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors that can occur during analytics operations.
#[derive(Debug)]
pub enum AnalyticsError {
    /// An I/O error occurred during persistence.
    Io(std::io::Error),
    /// A JSON serialization or deserialization error.
    Json(serde_json::Error),
}

impl std::fmt::Display for AnalyticsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
        }
    }
}

impl std::error::Error for AnalyticsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for AnalyticsError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for AnalyticsError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

// ---------------------------------------------------------------------------
// AnalyticsEventType
// ---------------------------------------------------------------------------

/// Discriminator describing the kind of analytics event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnalyticsEventType {
    /// A tool was executed.
    ToolExecution {
        tool_name: String,
        duration_ms: u64,
        success: bool,
    },
    /// The user submitted a prompt.
    PromptSubmitted { token_count: Option<usize> },
    /// A response was received from the model.
    ResponseReceived {
        token_count: Option<usize>,
        duration_ms: u64,
    },
    /// A file was read, written, or modified.
    FileOperation {
        operation: String,
        file_path: String,
    },
    /// A new session was started.
    SessionStart,
    /// A session ended.
    SessionEnd,
    /// An error occurred.
    Error {
        error_type: String,
        tool_name: Option<String>,
    },
    /// A permission was requested from the user.
    PermissionRequest { tool_name: String, approved: bool },
}

// ---------------------------------------------------------------------------
// AnalyticsEvent
// ---------------------------------------------------------------------------

/// A single analytics event with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsEvent {
    /// Unique identifier (UUID).
    pub id: String,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
    /// The kind of event.
    pub event_type: AnalyticsEventType,
    /// Arbitrary key-value properties attached to the event.
    pub properties: HashMap<String, Value>,
    /// The session this event belongs to.
    pub session_id: String,
}

impl AnalyticsEvent {
    /// Create a new analytics event.
    pub fn new(event_type: AnalyticsEventType, session_id: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            event_type,
            properties: HashMap::new(),
            session_id,
        }
    }

    /// Builder-style setter for a property.
    pub fn with_property(mut self, key: impl Into<String>, value: Value) -> Self {
        self.properties.insert(key.into(), value);
        self
    }

    /// Builder-style setter for multiple properties.
    pub fn with_properties(mut self, props: HashMap<String, Value>) -> Self {
        self.properties.extend(props);
        self
    }

    /// Returns the date of this event (UTC).
    pub fn date(&self) -> NaiveDate {
        self.timestamp.date_naive()
    }

    /// Returns `true` if this event represents an error.
    pub fn is_error(&self) -> bool {
        matches!(self.event_type, AnalyticsEventType::Error { .. })
    }

    /// Returns `true` if this event represents a tool execution.
    pub fn is_tool_execution(&self) -> bool {
        matches!(self.event_type, AnalyticsEventType::ToolExecution { .. })
    }
}

// ---------------------------------------------------------------------------
// ToolStats
// ---------------------------------------------------------------------------

/// Aggregated statistics for a single tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolStats {
    /// Name of the tool.
    pub tool_name: String,
    /// Total number of invocations.
    pub total_calls: usize,
    /// Number of successful invocations.
    pub successful_calls: usize,
    /// Number of failed invocations.
    pub failed_calls: usize,
    /// Average duration in milliseconds.
    pub avg_duration_ms: f64,
    /// Cumulative duration in milliseconds.
    pub total_duration_ms: u64,
}

// ---------------------------------------------------------------------------
// SessionStats
// ---------------------------------------------------------------------------

/// Aggregated statistics for a single session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionStats {
    /// Session identifier.
    pub session_id: String,
    /// When the session started.
    pub started_at: DateTime<Utc>,
    /// When the session ended, if it has ended.
    pub ended_at: Option<DateTime<Utc>>,
    /// Number of tool calls in the session.
    pub tool_calls: usize,
    /// Number of errors in the session.
    pub errors: usize,
    /// Total session duration in milliseconds, if the session has ended.
    pub duration_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// DailyStats
// ---------------------------------------------------------------------------

/// Aggregated statistics for a single day.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DailyStats {
    /// The date (UTC).
    pub date: NaiveDate,
    /// Number of sessions on this day.
    pub sessions: usize,
    /// Number of tool calls on this day.
    pub tool_calls: usize,
    /// Number of errors on this day.
    pub errors: usize,
    /// Cumulative duration in milliseconds on this day.
    pub total_duration_ms: u64,
}

// ---------------------------------------------------------------------------
// AnalyticsSummary
// ---------------------------------------------------------------------------

/// High-level summary of all recorded analytics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalyticsSummary {
    /// Total number of recorded events.
    pub total_events: usize,
    /// Total number of sessions.
    pub total_sessions: usize,
    /// Total number of tool calls.
    pub total_tool_calls: usize,
    /// Total number of errors.
    pub total_errors: usize,
    /// Total number of permission requests.
    pub total_permission_requests: usize,
    /// Total number of prompts submitted.
    pub total_prompts: usize,
    /// Total number of responses received.
    pub total_responses: usize,
    /// Total number of file operations.
    pub total_file_operations: usize,
    /// Cumulative duration of all tool calls in milliseconds.
    pub total_tool_duration_ms: u64,
}

// ---------------------------------------------------------------------------
// AnalyticsStore
// ---------------------------------------------------------------------------

/// In-memory analytics store with optional JSONL disk persistence.
///
/// Events are stored chronologically and can be queried by session, tool, or date.
pub struct AnalyticsStore {
    /// All recorded events.
    events: Vec<AnalyticsEvent>,
    /// The current session identifier.
    session_id: String,
    /// Optional directory for JSONL persistence.
    storage_path: Option<PathBuf>,
}

impl AnalyticsStore {
    /// Create a new analytics store with a fresh session ID.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            session_id: Uuid::new_v4().to_string(),
            storage_path: None,
        }
    }

    /// Create a new analytics store with a specific session ID.
    pub fn with_session_id(session_id: String) -> Self {
        Self {
            events: Vec::new(),
            session_id,
            storage_path: None,
        }
    }

    /// Create a new analytics store with a custom storage path.
    pub fn with_storage_path(path: PathBuf) -> Self {
        Self {
            events: Vec::new(),
            session_id: Uuid::new_v4().to_string(),
            storage_path: Some(path),
        }
    }

    /// Record a new analytics event.
    ///
    /// The event is assigned a UUID, the current timestamp, and the current
    /// session ID.
    pub fn record(&mut self, event_type: AnalyticsEventType, properties: HashMap<String, Value>) {
        let event =
            AnalyticsEvent::new(event_type, self.session_id.clone()).with_properties(properties);
        self.events.push(event);
    }

    /// Record a new analytics event with a custom session ID.
    pub fn record_for_session(
        &mut self,
        event_type: AnalyticsEventType,
        session_id: String,
        properties: HashMap<String, Value>,
    ) {
        let event = AnalyticsEvent::new(event_type, session_id).with_properties(properties);
        self.events.push(event);
    }

    /// Returns a reference to all recorded events.
    pub fn get_events(&self) -> &[AnalyticsEvent] {
        &self.events
    }

    /// Returns events for the current session.
    pub fn get_session_events(&self) -> Vec<&AnalyticsEvent> {
        self.events
            .iter()
            .filter(|e| e.session_id == self.session_id)
            .collect()
    }

    /// Returns events for a specific session ID.
    pub fn get_session_events_by_id(&self, session_id: &str) -> Vec<&AnalyticsEvent> {
        self.events
            .iter()
            .filter(|e| e.session_id == session_id)
            .collect()
    }

    /// Returns aggregated statistics for each tool.
    pub fn get_tool_stats(&self) -> Vec<ToolStats> {
        let mut stats: HashMap<String, (usize, usize, u64)> = HashMap::new();

        for event in &self.events {
            if let AnalyticsEventType::ToolExecution {
                tool_name,
                duration_ms,
                success,
            } = &event.event_type
            {
                let entry = stats.entry(tool_name.clone()).or_default();
                entry.0 += 1; // total_calls
                entry.2 += duration_ms; // total_duration_ms
                if *success {
                    entry.1 += 1; // successful_calls
                }
            }
        }

        let mut result: Vec<ToolStats> = stats
            .into_iter()
            .map(
                |(tool_name, (total_calls, successful_calls, total_duration_ms))| {
                    let failed_calls = total_calls - successful_calls;
                    let avg_duration_ms = if total_calls > 0 {
                        total_duration_ms as f64 / total_calls as f64
                    } else {
                        0.0
                    };
                    ToolStats {
                        tool_name,
                        total_calls,
                        successful_calls,
                        failed_calls,
                        avg_duration_ms,
                        total_duration_ms,
                    }
                },
            )
            .collect();

        result.sort_by(|a, b| b.total_calls.cmp(&a.total_calls));
        result
    }

    /// Returns aggregated statistics for each session.
    pub fn get_session_stats(&self) -> Vec<SessionStats> {
        let mut session_starts: HashMap<String, DateTime<Utc>> = HashMap::new();
        let mut session_ends: HashMap<String, DateTime<Utc>> = HashMap::new();
        let mut session_tool_calls: HashMap<String, usize> = HashMap::new();
        let mut session_errors: HashMap<String, usize> = HashMap::new();

        for event in &self.events {
            let sid = &event.session_id;
            match &event.event_type {
                AnalyticsEventType::SessionStart => {
                    session_starts.entry(sid.clone()).or_insert(event.timestamp);
                }
                AnalyticsEventType::SessionEnd => {
                    session_ends.insert(sid.clone(), event.timestamp);
                }
                AnalyticsEventType::ToolExecution { .. } => {
                    *session_tool_calls.entry(sid.clone()).or_default() += 1;
                }
                AnalyticsEventType::Error { .. } => {
                    *session_errors.entry(sid.clone()).or_default() += 1;
                }
                _ => {}
            }
        }

        // Ensure every session that appears in any event has a start time.
        for event in &self.events {
            session_starts
                .entry(event.session_id.clone())
                .or_insert(event.timestamp);
        }

        let mut result: Vec<SessionStats> = session_starts
            .into_iter()
            .map(|(session_id, started_at)| {
                let ended_at = session_ends.get(&session_id).copied();
                let tool_calls = session_tool_calls.get(&session_id).copied().unwrap_or(0);
                let errors = session_errors.get(&session_id).copied().unwrap_or(0);
                let duration_ms =
                    ended_at.map(|end| (end - started_at).num_milliseconds().unsigned_abs());
                SessionStats {
                    session_id,
                    started_at,
                    ended_at,
                    tool_calls,
                    errors,
                    duration_ms,
                }
            })
            .collect();

        result.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        result
    }

    /// Returns aggregated statistics for each day.
    pub fn get_daily_stats(&self) -> Vec<DailyStats> {
        let mut daily: HashMap<NaiveDate, (HashMap<String, bool>, usize, usize, u64)> =
            HashMap::new();

        for event in &self.events {
            let date = event.date();
            let entry = daily
                .entry(date)
                .or_insert_with(|| (HashMap::new(), 0, 0, 0));
            entry.1 += 1; // total events (for counting tool calls below)
            if let AnalyticsEventType::ToolExecution { duration_ms, .. } = &event.event_type {
                entry.3 += duration_ms;
            }
        }

        // Recompute per-day aggregates properly.
        let mut daily_map: HashMap<NaiveDate, (usize, usize, usize, u64)> = HashMap::new();

        for event in &self.events {
            let date = event.date();
            let entry = daily_map.entry(date).or_default();
            match &event.event_type {
                AnalyticsEventType::SessionStart | AnalyticsEventType::SessionEnd => {}
                AnalyticsEventType::ToolExecution { .. } => {
                    entry.1 += 1; // tool_calls
                }
                AnalyticsEventType::Error { .. } => {
                    entry.2 += 1; // errors
                }
                _ => {}
            }
        }

        // Count sessions per day.
        let mut sessions_per_day: HashMap<NaiveDate, usize> = HashMap::new();
        let session_dates: HashMap<String, NaiveDate> = self
            .events
            .iter()
            .filter(|e| matches!(e.event_type, AnalyticsEventType::SessionStart))
            .map(|e| (e.session_id.clone(), e.date()))
            .collect();

        for event in &self.events {
            let date = event.date();
            if session_dates.contains_key(&event.session_id) {
                *sessions_per_day.entry(date).or_default() += 1;
            }
        }
        // Each session-start maps to exactly one date, so we just count unique session_ids.
        let mut sessions_by_day: HashMap<NaiveDate, usize> = HashMap::new();
        for date in session_dates.values() {
            *sessions_by_day.entry(*date).or_default() += 1;
        }

        // Add tool durations per day.
        let mut durations_per_day: HashMap<NaiveDate, u64> = HashMap::new();
        for event in &self.events {
            if let AnalyticsEventType::ToolExecution { duration_ms, .. } = &event.event_type {
                *durations_per_day.entry(event.date()).or_default() += duration_ms;
            }
        }

        let mut result: Vec<DailyStats> = daily_map
            .into_iter()
            .map(|(date, (_, tool_calls, errors, _))| {
                let sessions = sessions_by_day.get(&date).copied().unwrap_or(0);
                let total_duration_ms = durations_per_day.get(&date).copied().unwrap_or(0);
                DailyStats {
                    date,
                    sessions,
                    tool_calls,
                    errors,
                    total_duration_ms,
                }
            })
            .collect();

        result.sort_by(|a, b| b.date.cmp(&a.date));
        result
    }

    /// Returns a high-level summary of all analytics.
    pub fn summary(&self) -> AnalyticsSummary {
        let total_sessions = self
            .events
            .iter()
            .filter(|e| matches!(e.event_type, AnalyticsEventType::SessionStart))
            .map(|e| e.session_id.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len();

        let mut s = AnalyticsSummary {
            total_events: self.events.len(),
            total_sessions,
            ..Default::default()
        };

        for event in &self.events {
            match &event.event_type {
                AnalyticsEventType::ToolExecution { duration_ms, .. } => {
                    s.total_tool_calls += 1;
                    s.total_tool_duration_ms += duration_ms;
                }
                AnalyticsEventType::Error { .. } => {
                    s.total_errors += 1;
                }
                AnalyticsEventType::PermissionRequest { .. } => {
                    s.total_permission_requests += 1;
                }
                AnalyticsEventType::PromptSubmitted { .. } => {
                    s.total_prompts += 1;
                }
                AnalyticsEventType::ResponseReceived { .. } => {
                    s.total_responses += 1;
                }
                AnalyticsEventType::FileOperation { .. } => {
                    s.total_file_operations += 1;
                }
                AnalyticsEventType::SessionStart | AnalyticsEventType::SessionEnd => {}
            }
        }
        s
    }

    /// Returns the current session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Persist all events to disk as JSONL.
    ///
    /// Events are grouped by date and written to
    /// `{storage_path}/{YYYY-MM-DD}.jsonl`.
    pub fn save(&self) -> Result<(), AnalyticsError> {
        let dir = match &self.storage_path {
            Some(p) => p.clone(),
            None => {
                let home = dirs::home_dir().ok_or_else(|| {
                    AnalyticsError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "home directory not found",
                    ))
                })?;
                home.join(".shannon").join("analytics")
            }
        };

        fs::create_dir_all(&dir)?;

        // Group events by date.
        let mut by_date: HashMap<NaiveDate, Vec<&AnalyticsEvent>> = HashMap::new();
        for event in &self.events {
            by_date.entry(event.date()).or_default().push(event);
        }

        for (date, events) in by_date {
            let file_path = dir.join(format!("{date}.jsonl"));
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file_path)?;

            for event in events {
                let line = serde_json::to_string(event)?;
                writeln!(file, "{line}")?;
            }
        }

        Ok(())
    }

    /// Load events from disk (JSONL files in the storage directory).
    ///
    /// Appends loaded events to the in-memory store. Does not clear existing events.
    pub fn load(&mut self) -> Result<(), AnalyticsError> {
        let dir = match &self.storage_path {
            Some(p) => p.clone(),
            None => {
                let home = dirs::home_dir().ok_or_else(|| {
                    AnalyticsError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "home directory not found",
                    ))
                })?;
                home.join(".shannon").join("analytics")
            }
        };

        if !dir.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                let file = fs::File::open(&path)?;
                let reader = std::io::BufReader::new(file);

                for line in reader.lines() {
                    let line = line?;
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<AnalyticsEvent>(trimmed) {
                        Ok(event) => self.events.push(event),
                        Err(e) => {
                            tracing::warn!(
                                "Skipping malformed analytics line in {}: {e}",
                                path.display()
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Clear all in-memory events.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Returns the number of recorded events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns `true` if no events have been recorded.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl Default for AnalyticsStore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::error::Error;

    fn make_store() -> AnalyticsStore {
        AnalyticsStore::with_session_id("test-session".to_string())
    }

    fn empty_props() -> HashMap<String, Value> {
        HashMap::new()
    }

    // -- Event construction tests --

    #[test]
    fn test_analytics_event_new() {
        let event = AnalyticsEvent::new(AnalyticsEventType::SessionStart, "sess-1".to_string());
        assert!(!event.id.is_empty());
        assert!(event.properties.is_empty());
        assert_eq!(event.session_id, "sess-1");
    }

    #[test]
    fn test_analytics_event_with_property() {
        let event = AnalyticsEvent::new(AnalyticsEventType::SessionStart, "sess-1".to_string())
            .with_property("key", Value::String("val".into()));

        assert_eq!(event.properties.get("key").unwrap(), "val");
    }

    #[test]
    fn test_analytics_event_with_properties() {
        let mut props = HashMap::new();
        props.insert("a".to_string(), Value::Number(1.into()));
        props.insert("b".to_string(), Value::Bool(true));

        let event = AnalyticsEvent::new(AnalyticsEventType::SessionStart, "sess-1".to_string())
            .with_properties(props);

        assert_eq!(event.properties.len(), 2);
    }

    #[test]
    fn test_analytics_event_is_error() {
        let err = AnalyticsEvent::new(
            AnalyticsEventType::Error {
                error_type: "timeout".into(),
                tool_name: None,
            },
            "s1".into(),
        );
        assert!(err.is_error());
        assert!(!err.is_tool_execution());

        let tool = AnalyticsEvent::new(
            AnalyticsEventType::ToolExecution {
                tool_name: "bash".into(),
                duration_ms: 100,
                success: true,
            },
            "s1".into(),
        );
        assert!(!tool.is_error());
        assert!(tool.is_tool_execution());
    }

    #[test]
    fn test_analytics_event_date() {
        let event = AnalyticsEvent::new(AnalyticsEventType::SessionStart, "s1".into());
        assert_eq!(event.date(), Utc::now().date_naive());
    }

    // -- AnalyticsStore basic tests --

    #[test]
    fn test_store_new() {
        let store = AnalyticsStore::new();
        assert!(store.is_empty());
        assert!(store.storage_path.is_none());
        assert!(!store.session_id().is_empty());
    }

    #[test]
    fn test_store_with_session_id() {
        let store = AnalyticsStore::with_session_id("my-session".into());
        assert_eq!(store.session_id(), "my-session");
        assert!(store.is_empty());
    }

    #[test]
    fn test_store_with_storage_path() {
        let path = PathBuf::from("/tmp/test-analytics");
        let store = AnalyticsStore::with_storage_path(path.clone());
        assert_eq!(store.storage_path, Some(path));
    }

    #[test]
    fn test_store_default() {
        let store = AnalyticsStore::default();
        assert!(store.is_empty());
    }

    #[test]
    fn test_store_record_and_len() {
        let mut store = make_store();
        assert_eq!(store.len(), 0);

        store.record(AnalyticsEventType::SessionStart, empty_props());
        assert_eq!(store.len(), 1);

        store.record(
            AnalyticsEventType::ToolExecution {
                tool_name: "bash".into(),
                duration_ms: 50,
                success: true,
            },
            empty_props(),
        );
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_store_record_for_session() {
        let mut store = make_store();
        store.record_for_session(
            AnalyticsEventType::SessionStart,
            "other-session".into(),
            empty_props(),
        );
        assert_eq!(store.len(), 1);

        let events = store.get_session_events();
        assert!(events.is_empty(), "should not appear in current session");
    }

    // -- Event retrieval tests --

    #[test]
    fn test_get_events() {
        let mut store = make_store();
        store.record(AnalyticsEventType::SessionStart, empty_props());
        store.record(AnalyticsEventType::SessionEnd, empty_props());

        let events = store.get_events();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_get_session_events() {
        let mut store = make_store();
        store.record(AnalyticsEventType::SessionStart, empty_props());
        store.record(
            AnalyticsEventType::ToolExecution {
                tool_name: "bash".into(),
                duration_ms: 10,
                success: true,
            },
            empty_props(),
        );
        // Record for another session
        store.record_for_session(
            AnalyticsEventType::SessionStart,
            "other".into(),
            empty_props(),
        );

        let session_events = store.get_session_events();
        assert_eq!(session_events.len(), 2);
    }

    #[test]
    fn test_get_session_events_by_id() {
        let mut store = make_store();
        store.record(AnalyticsEventType::SessionStart, empty_props());
        store.record_for_session(
            AnalyticsEventType::ToolExecution {
                tool_name: "read".into(),
                duration_ms: 20,
                success: false,
            },
            "other-session".into(),
            empty_props(),
        );
        store.record_for_session(
            AnalyticsEventType::Error {
                error_type: "io".into(),
                tool_name: Some("read".into()),
            },
            "other-session".into(),
            empty_props(),
        );

        let other_events = store.get_session_events_by_id("other-session");
        assert_eq!(other_events.len(), 2);
    }

    // -- Tool stats tests --

    #[test]
    fn test_get_tool_stats_empty() {
        let store = make_store();
        assert!(store.get_tool_stats().is_empty());
    }

    #[test]
    fn test_get_tool_stats_single_tool() {
        let mut store = make_store();
        store.record(
            AnalyticsEventType::ToolExecution {
                tool_name: "bash".into(),
                duration_ms: 100,
                success: true,
            },
            empty_props(),
        );
        store.record(
            AnalyticsEventType::ToolExecution {
                tool_name: "bash".into(),
                duration_ms: 200,
                success: false,
            },
            empty_props(),
        );

        let stats = store.get_tool_stats();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].tool_name, "bash");
        assert_eq!(stats[0].total_calls, 2);
        assert_eq!(stats[0].successful_calls, 1);
        assert_eq!(stats[0].failed_calls, 1);
        assert_eq!(stats[0].total_duration_ms, 300);
        assert!((stats[0].avg_duration_ms - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_get_tool_stats_multiple_tools() {
        let mut store = make_store();
        store.record(
            AnalyticsEventType::ToolExecution {
                tool_name: "bash".into(),
                duration_ms: 50,
                success: true,
            },
            empty_props(),
        );
        store.record(
            AnalyticsEventType::ToolExecution {
                tool_name: "read".into(),
                duration_ms: 30,
                success: true,
            },
            empty_props(),
        );
        store.record(
            AnalyticsEventType::ToolExecution {
                tool_name: "bash".into(),
                duration_ms: 50,
                success: true,
            },
            empty_props(),
        );

        let stats = store.get_tool_stats();
        assert_eq!(stats.len(), 2);
        // bash has 2 calls, read has 1 => bash should come first (sorted desc)
        assert_eq!(stats[0].tool_name, "bash");
        assert_eq!(stats[1].tool_name, "read");
    }

    #[test]
    fn test_get_tool_stats_sorted_by_call_count() {
        let mut store = make_store();
        for _ in 0..3 {
            store.record(
                AnalyticsEventType::ToolExecution {
                    tool_name: "bash".into(),
                    duration_ms: 10,
                    success: true,
                },
                empty_props(),
            );
        }
        store.record(
            AnalyticsEventType::ToolExecution {
                tool_name: "read".into(),
                duration_ms: 10,
                success: true,
            },
            empty_props(),
        );

        let stats = store.get_tool_stats();
        assert_eq!(stats[0].total_calls, 3);
        assert_eq!(stats[1].total_calls, 1);
    }

    // -- Session stats tests --

    #[test]
    fn test_get_session_stats_empty() {
        let store = make_store();
        assert!(store.get_session_stats().is_empty());
    }

    #[test]
    fn test_get_session_stats_with_start_end() {
        let mut store = make_store();
        store.record(AnalyticsEventType::SessionStart, empty_props());

        // Simulate an end event 5 seconds later by constructing manually.
        let end_event = AnalyticsEvent::new(AnalyticsEventType::SessionEnd, "test-session".into());
        store.events.push(end_event);

        let stats = store.get_session_stats();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].session_id, "test-session");
        assert!(stats[0].ended_at.is_some());
        assert!(stats[0].duration_ms.is_some());
    }

    #[test]
    fn test_get_session_stats_tool_calls_and_errors() {
        let mut store = make_store();
        store.record(AnalyticsEventType::SessionStart, empty_props());
        store.record(
            AnalyticsEventType::ToolExecution {
                tool_name: "bash".into(),
                duration_ms: 10,
                success: true,
            },
            empty_props(),
        );
        store.record(
            AnalyticsEventType::ToolExecution {
                tool_name: "bash".into(),
                duration_ms: 10,
                success: true,
            },
            empty_props(),
        );
        store.record(
            AnalyticsEventType::Error {
                error_type: "timeout".into(),
                tool_name: None,
            },
            empty_props(),
        );

        let stats = store.get_session_stats();
        assert_eq!(stats[0].tool_calls, 2);
        assert_eq!(stats[0].errors, 1);
    }

    #[test]
    fn test_get_session_stats_multiple_sessions() {
        let mut store = make_store();
        store.record(AnalyticsEventType::SessionStart, empty_props());
        store.record_for_session(
            AnalyticsEventType::SessionStart,
            "session-b".into(),
            empty_props(),
        );
        store.record_for_session(
            AnalyticsEventType::ToolExecution {
                tool_name: "bash".into(),
                duration_ms: 10,
                success: true,
            },
            "session-b".into(),
            empty_props(),
        );

        let stats = store.get_session_stats();
        assert_eq!(stats.len(), 2);

        let session_ids: HashSet<&str> = stats.iter().map(|s| s.session_id.as_str()).collect();
        assert!(session_ids.contains("test-session"));
        assert!(session_ids.contains("session-b"));
    }

    // -- Daily stats tests --

    #[test]
    fn test_get_daily_stats_empty() {
        let store = make_store();
        assert!(store.get_daily_stats().is_empty());
    }

    #[test]
    fn test_get_daily_stats_counts() {
        let mut store = make_store();
        store.record(AnalyticsEventType::SessionStart, empty_props());
        store.record(
            AnalyticsEventType::ToolExecution {
                tool_name: "bash".into(),
                duration_ms: 100,
                success: true,
            },
            empty_props(),
        );
        store.record(
            AnalyticsEventType::ToolExecution {
                tool_name: "read".into(),
                duration_ms: 50,
                success: false,
            },
            empty_props(),
        );
        store.record(
            AnalyticsEventType::Error {
                error_type: "io".into(),
                tool_name: Some("read".into()),
            },
            empty_props(),
        );

        let daily = store.get_daily_stats();
        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0].sessions, 1);
        assert_eq!(daily[0].tool_calls, 2);
        assert_eq!(daily[0].errors, 1);
        assert_eq!(daily[0].total_duration_ms, 150);
    }

    // -- Summary tests --

    #[test]
    fn test_summary_empty() {
        let store = make_store();
        let s = store.summary();
        assert_eq!(s.total_events, 0);
        assert_eq!(s.total_sessions, 0);
    }

    #[test]
    fn test_summary_comprehensive() {
        let mut store = make_store();
        store.record(AnalyticsEventType::SessionStart, empty_props());
        store.record(
            AnalyticsEventType::PromptSubmitted {
                token_count: Some(100),
            },
            empty_props(),
        );
        store.record(
            AnalyticsEventType::ResponseReceived {
                token_count: Some(200),
                duration_ms: 500,
            },
            empty_props(),
        );
        store.record(
            AnalyticsEventType::ToolExecution {
                tool_name: "bash".into(),
                duration_ms: 100,
                success: true,
            },
            empty_props(),
        );
        store.record(
            AnalyticsEventType::FileOperation {
                operation: "read".into(),
                file_path: "/tmp/foo.rs".into(),
            },
            empty_props(),
        );
        store.record(
            AnalyticsEventType::PermissionRequest {
                tool_name: "bash".into(),
                approved: true,
            },
            empty_props(),
        );
        store.record(
            AnalyticsEventType::Error {
                error_type: "timeout".into(),
                tool_name: None,
            },
            empty_props(),
        );

        let s = store.summary();
        assert_eq!(s.total_events, 7);
        assert_eq!(s.total_sessions, 1);
        assert_eq!(s.total_tool_calls, 1);
        assert_eq!(s.total_errors, 1);
        assert_eq!(s.total_permission_requests, 1);
        assert_eq!(s.total_prompts, 1);
        assert_eq!(s.total_responses, 1);
        assert_eq!(s.total_file_operations, 1);
        assert_eq!(s.total_tool_duration_ms, 100);
    }

    // -- Clear tests --

    #[test]
    fn test_clear() {
        let mut store = make_store();
        store.record(AnalyticsEventType::SessionStart, empty_props());
        assert_eq!(store.len(), 1);

        store.clear();
        assert!(store.is_empty());
    }

    // -- Serialization round-trip tests --

    #[test]
    fn test_event_serialization_roundtrip() {
        let event = AnalyticsEvent::new(
            AnalyticsEventType::ToolExecution {
                tool_name: "bash".into(),
                duration_ms: 42,
                success: true,
            },
            "sess-1".into(),
        )
        .with_property("cwd", Value::String("/home".into()));

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: AnalyticsEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(event.id, deserialized.id);
        assert_eq!(event.session_id, deserialized.session_id);
        assert_eq!(event.event_type, deserialized.event_type);
        assert_eq!(event.properties, deserialized.properties);
    }

    #[test]
    fn test_all_event_types_serialization() {
        let types = vec![
            AnalyticsEventType::SessionStart,
            AnalyticsEventType::SessionEnd,
            AnalyticsEventType::PromptSubmitted {
                token_count: Some(50),
            },
            AnalyticsEventType::PromptSubmitted { token_count: None },
            AnalyticsEventType::ResponseReceived {
                token_count: Some(100),
                duration_ms: 200,
            },
            AnalyticsEventType::FileOperation {
                operation: "write".into(),
                file_path: "/tmp/test.rs".into(),
            },
            AnalyticsEventType::Error {
                error_type: "timeout".into(),
                tool_name: Some("bash".into()),
            },
            AnalyticsEventType::Error {
                error_type: "io".into(),
                tool_name: None,
            },
            AnalyticsEventType::PermissionRequest {
                tool_name: "bash".into(),
                approved: true,
            },
        ];

        for event_type in types {
            let event = AnalyticsEvent::new(event_type.clone(), "s1".into());
            let json = serde_json::to_string(&event).unwrap();
            let deserialized: AnalyticsEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event.event_type, deserialized.event_type);
        }
    }

    // -- Persistence tests --

    #[test]
    fn test_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let storage_path = dir.path().to_path_buf();

        // Write events.
        {
            let mut store = AnalyticsStore::with_storage_path(storage_path.clone());
            store.record(AnalyticsEventType::SessionStart, empty_props());
            store.record(
                AnalyticsEventType::ToolExecution {
                    tool_name: "bash".into(),
                    duration_ms: 100,
                    success: true,
                },
                empty_props(),
            );
            store.save().unwrap();
        }

        // Load events into a fresh store.
        {
            let mut store = AnalyticsStore::with_storage_path(storage_path);
            store.load().unwrap();
            assert_eq!(store.len(), 2);
            assert_eq!(store.summary().total_tool_calls, 1);
        }
    }

    #[test]
    fn test_load_nonexistent_directory() {
        let mut store =
            AnalyticsStore::with_storage_path(PathBuf::from("/tmp/nonexistent-analytics-xyz"));
        // Should succeed silently.
        assert!(store.load().is_ok());
        assert!(store.is_empty());
    }

    #[test]
    fn test_load_skips_malformed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("2026-01-01.jsonl");
        let mut file = fs::File::create(&file_path).unwrap();
        writeln!(file, "{{\"valid\": true}}").unwrap();
        writeln!(file, "this is not json").unwrap();
        writeln!(file, "{{\"also bad json").unwrap();

        let mut store = AnalyticsStore::with_storage_path(dir.path().to_path_buf());
        // Should not panic, just skip bad lines.
        assert!(store.load().is_ok());
    }

    #[test]
    fn test_save_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let storage_path = dir.path().join("nested").join("analytics");

        let mut store = AnalyticsStore::with_storage_path(storage_path.clone());
        store.record(AnalyticsEventType::SessionStart, empty_props());
        store.save().unwrap();

        assert!(storage_path.exists());
    }

    // -- AnalyticsError tests --

    #[test]
    fn test_analytics_error_display() {
        let err = AnalyticsError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not found",
        ));
        assert!(err.to_string().contains("not found"));

        let err = AnalyticsError::Json(serde_json::from_str::<Value>("{bad}").unwrap_err());
        assert!(err.to_string().contains("JSON"));
    }

    #[test]
    fn test_analytics_error_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = AnalyticsError::Io(io_err);
        assert!(err.source().is_some());
    }

    // -- Stats type serialization --

    #[test]
    fn test_tool_stats_serialization() {
        let stats = ToolStats {
            tool_name: "bash".into(),
            total_calls: 5,
            successful_calls: 4,
            failed_calls: 1,
            avg_duration_ms: 80.5,
            total_duration_ms: 402,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: ToolStats = serde_json::from_str(&json).unwrap();
        assert_eq!(stats, deserialized);
    }

    #[test]
    fn test_session_stats_serialization() {
        let stats = SessionStats {
            session_id: "s1".into(),
            started_at: Utc::now(),
            ended_at: None,
            tool_calls: 3,
            errors: 1,
            duration_ms: None,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: SessionStats = serde_json::from_str(&json).unwrap();
        assert_eq!(stats.session_id, deserialized.session_id);
        assert_eq!(stats.tool_calls, deserialized.tool_calls);
    }

    #[test]
    fn test_daily_stats_serialization() {
        let stats = DailyStats {
            date: NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
            sessions: 2,
            tool_calls: 10,
            errors: 1,
            total_duration_ms: 5000,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: DailyStats = serde_json::from_str(&json).unwrap();
        assert_eq!(stats, deserialized);
    }
}
