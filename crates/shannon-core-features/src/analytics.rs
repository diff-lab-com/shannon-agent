//! # Analytics
//!
//! Event tracking, aggregation, and persistence for Shannon Code usage analytics.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use thiserror::Error;
use uuid::Uuid;

// ============================================================================
// Error
// ============================================================================

/// Errors that can occur during analytics operations.
#[derive(Debug, Error)]
pub enum AnalyticsError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Storage path not set")]
    NoStoragePath,
}

// ============================================================================
// Event
// ============================================================================

/// A single analytics event with properties and timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsEvent {
    /// Unique event identifier.
    pub id: Uuid,

    /// Event type (e.g., "tool_call", "command", "error").
    pub event_type: String,

    /// Event properties as key-value pairs.
    pub properties: HashMap<String, serde_json::Value>,

    /// When the event occurred.
    pub timestamp: DateTime<Utc>,

    /// Optional user or session identifier.
    pub session_id: Option<String>,
}

impl AnalyticsEvent {
    /// Create a new analytics event.
    pub fn new(
        event_type: String,
        properties: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            event_type,
            properties,
            timestamp: Utc::now(),
            session_id: None,
        }
    }

    /// Set the session ID for this event.
    pub fn with_session(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Add a property to this event.
    pub fn with_property(mut self, key: String, value: serde_json::Value) -> Self {
        self.properties.insert(key, value);
        self
    }
}

// ============================================================================
// Store
// ============================================================================

/// Persistent storage for analytics events.
///
/// Events are stored in JSONL format (one JSON object per line) for
/// efficient append-only writes and streaming reads.
pub struct AnalyticsStore {
    storage_path: PathBuf,
    in_memory_events: Vec<AnalyticsEvent>,
}

impl AnalyticsStore {
    /// Create a new analytics store with the given storage path.
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            storage_path,
            in_memory_events: Vec::new(),
        }
    }

    /// Track an event, storing it in memory.
    pub fn track(&mut self, event: AnalyticsEvent) {
        self.in_memory_events.push(event);
    }

    /// Persist all in-memory events to disk as JSONL.
    pub fn persist(&mut self) -> Result<(), AnalyticsError> {
        if self.in_memory_events.is_empty() {
            return Ok(());
        }

        // Ensure directory exists
        if let Some(parent) = self.storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.storage_path)?;

        for event in &self.in_memory_events {
            let json = serde_json::to_string(event)?;
            writeln!(file, "{}", json)?;
        }

        self.in_memory_events.clear();
        Ok(())
    }

    /// Load all events from disk.
    pub fn load(&mut self) -> Result<Vec<AnalyticsEvent>, AnalyticsError> {
        if !self.storage_path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.storage_path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if let Ok(event) = serde_json::from_str::<AnalyticsEvent>(&line) {
                events.push(event);
            }
        }

        Ok(events)
    }

    /// Clear the in-memory event buffer without persisting.
    pub fn clear_buffer(&mut self) {
        self.in_memory_events.clear();
    }

    /// Return the number of events currently in the memory buffer.
    pub fn buffered_count(&self) -> usize {
        self.in_memory_events.len()
    }
}

// ============================================================================
// Aggregation
// ============================================================================

/// Aggregated statistics for a specific dimension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedStats {
    /// The dimension key (e.g., tool name, date).
    pub key: String,

    /// Total count of events for this key.
    pub count: usize,

    /// First event timestamp.
    pub first_seen: DateTime<Utc>,

    /// Last event timestamp.
    pub last_seen: DateTime<Utc>,

    /// Sum of numeric properties (if applicable).
    pub sums: HashMap<String, f64>,
}

/// Aggregator for computing statistics across events.
pub struct AnalyticsAggregator;

impl AnalyticsAggregator {
    /// Aggregate events by tool name.
    pub fn by_tool(events: &[AnalyticsEvent]) -> Vec<AggregatedStats> {
        let mut by_tool: HashMap<String, Vec<&AnalyticsEvent>> = HashMap::new();

        for event in events {
            if let Some(tool) = event.properties.get("tool") {
                if let Some(tool_name) = tool.as_str() {
                    by_tool
                        .entry(tool_name.to_string())
                        .or_default()
                        .push(event);
                }
            }
        }

        Self::compute_stats(by_tool)
    }

    /// Aggregate events by session ID.
    pub fn by_session(events: &[AnalyticsEvent]) -> Vec<AggregatedStats> {
        let mut by_session: HashMap<String, Vec<&AnalyticsEvent>> = HashMap::new();

        for event in events {
            if let Some(ref session_id) = event.session_id {
                by_session
                    .entry(session_id.clone())
                    .or_default()
                    .push(event);
            }
        }

        Self::compute_stats(by_session)
    }

    /// Aggregate events by day (YYYY-MM-DD).
    pub fn by_day(events: &[AnalyticsEvent]) -> Vec<AggregatedStats> {
        let mut by_day: HashMap<String, Vec<&AnalyticsEvent>> = HashMap::new();

        for event in events {
            let day = event.timestamp.format("%Y-%m-%d").to_string();
            by_day.entry(day).or_default().push(event);
        }

        Self::compute_stats(by_day)
    }

    /// Compute aggregated stats from grouped events.
    fn compute_stats(groups: HashMap<String, Vec<&AnalyticsEvent>>) -> Vec<AggregatedStats> {
        groups
            .into_iter()
            .map(|(key, events)| {
                let first_seen = events
                    .iter()
                    .map(|e| e.timestamp)
                    .min()
                    .unwrap_or_else(Utc::now);

                let last_seen = events
                    .iter()
                    .map(|e| e.timestamp)
                    .max()
                    .unwrap_or_else(Utc::now);

                AggregatedStats {
                    key,
                    count: events.len(),
                    first_seen,
                    last_seen,
                    sums: HashMap::new(),
                }
            })
            .collect()
    }
}

// ============================================================================
// Tracker
// ============================================================================

/// High-level event tracker combining store and aggregation.
pub struct EventTracker {
    store: AnalyticsStore,
    session_id: Option<String>,
}

impl EventTracker {
    /// Create a new event tracker.
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            store: AnalyticsStore::new(storage_path),
            session_id: None,
        }
    }

    /// Set the session ID for subsequent events.
    pub fn set_session(&mut self, session_id: String) {
        self.session_id = Some(session_id);
    }

    /// Clear the session ID.
    pub fn clear_session(&mut self) {
        self.session_id = None;
    }

    /// Track a tool invocation.
    pub fn track_tool_call(
        &mut self,
        tool_name: &str,
        duration_ms: u64,
    ) -> Result<(), AnalyticsError> {
        let mut properties = HashMap::new();
        properties.insert("tool".to_string(), serde_json::json!(tool_name));
        properties.insert("duration_ms".to_string(), serde_json::json!(duration_ms));

        let event = AnalyticsEvent::new("tool_call".to_string(), properties)
            .with_session("default".to_string());

        self.store.track(event);
        Ok(())
    }

    /// Track a command execution.
    pub fn track_command(&mut self, command: &str, success: bool) -> Result<(), AnalyticsError> {
        let mut properties = HashMap::new();
        properties.insert("command".to_string(), serde_json::json!(command));
        properties.insert("success".to_string(), serde_json::json!(success));

        let event = AnalyticsEvent::new("command".to_string(), properties)
            .with_session("default".to_string());

        self.store.track(event);
        Ok(())
    }

    /// Track an error.
    pub fn track_error(&mut self, error_type: &str, message: &str) -> Result<(), AnalyticsError> {
        let mut properties = HashMap::new();
        properties.insert("error_type".to_string(), serde_json::json!(error_type));
        properties.insert("message".to_string(), serde_json::json!(message));

        let event = AnalyticsEvent::new("error".to_string(), properties)
            .with_session("default".to_string());

        self.store.track(event);
        Ok(())
    }

    /// Persist all tracked events.
    pub fn persist(&mut self) -> Result<(), AnalyticsError> {
        self.store.persist()
    }

    /// Load and aggregate events by tool.
    pub fn aggregate_by_tool(&mut self) -> Result<Vec<AggregatedStats>, AnalyticsError> {
        let events = self.store.load()?;
        Ok(AnalyticsAggregator::by_tool(&events))
    }

    /// Load and aggregate events by day.
    pub fn aggregate_by_day(&mut self) -> Result<Vec<AggregatedStats>, AnalyticsError> {
        let events = self.store.load()?;
        Ok(AnalyticsAggregator::by_day(&events))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_creation() {
        let mut props = HashMap::new();
        props.insert("test".to_string(), serde_json::json!(42));

        let event = AnalyticsEvent::new("test_event".to_string(), props)
            .with_session("session123".to_string());

        assert_eq!(event.event_type, "test_event");
        assert_eq!(event.session_id, Some("session123".to_string()));
        assert_eq!(event.properties.get("test"), Some(&serde_json::json!(42)));
    }

    #[test]
    fn test_event_with_property() {
        let event = AnalyticsEvent::new("base".to_string(), HashMap::new())
            .with_property("key".to_string(), serde_json::json!("value"));

        assert_eq!(event.properties.get("key"), Some(&serde_json::json!("value")));
    }

    #[test]
    fn test_aggregate_by_tool() {
        let mut events = vec![];

        let mut props1 = HashMap::new();
        props1.insert("tool".to_string(), serde_json::json!("grep"));
        events.push(AnalyticsEvent::new("tool_call".to_string(), props1));

        let mut props2 = HashMap::new();
        props2.insert("tool".to_string(), serde_json::json!("grep"));
        events.push(AnalyticsEvent::new("tool_call".to_string(), props2));

        let mut props3 = HashMap::new();
        props3.insert("tool".to_string(), serde_json::json!("edit"));
        events.push(AnalyticsEvent::new("tool_call".to_string(), props3));

        let stats = AnalyticsAggregator::by_tool(&events);

        assert_eq!(stats.len(), 2);
        let grep_stat = stats.iter().find(|s| s.key == "grep").unwrap();
        assert_eq!(grep_stat.count, 2);
        let edit_stat = stats.iter().find(|s| s.key == "edit").unwrap();
        assert_eq!(edit_stat.count, 1);
    }

    #[test]
    fn test_aggregate_by_session() {
        let mut events = vec![];

        let mut props = HashMap::new();
        props.insert("action".to_string(), serde_json::json!("click"));

        let mut event1 = AnalyticsEvent::new("action".to_string(), props.clone());
        event1.session_id = Some("session1".to_string());
        events.push(event1);

        let mut event2 = AnalyticsEvent::new("action".to_string(), props.clone());
        event2.session_id = Some("session2".to_string());
        events.push(event2);

        let stats = AnalyticsAggregator::by_session(&events);
        assert_eq!(stats.len(), 2);
    }

    #[test]
    fn test_aggregate_by_day() {
        let mut events = vec![];

        let mut props = HashMap::new();
        let mut event = AnalyticsEvent::new("test".to_string(), props);
        event.timestamp = Utc::now();
        events.push(event);

        let stats = AnalyticsAggregator::by_day(&events);
        assert_eq!(stats.len(), 1);
        assert!(stats[0].key.starts_with("20")); // Year 2000+
    }

    #[test]
    fn test_tracker_track_tool_call() {
        let temp_dir = std::env::temp_dir();
        let mut tracker = EventTracker::new(temp_dir.join("test-analytics.jsonl"));

        tracker.track_tool_call("grep", 150).unwrap();
        assert_eq!(tracker.store.buffered_count(), 1);

        tracker.persist().unwrap();
        assert_eq!(tracker.store.buffered_count(), 0);
    }

    #[test]
    fn test_tracker_track_command() {
        let temp_dir = std::env::temp_dir();
        let mut tracker = EventTracker::new(temp_dir.join("test-analytics.jsonl"));

        tracker.track_command("/help", true).unwrap();
        tracker.track_command("/unknown", false).unwrap();

        assert_eq!(tracker.store.buffered_count(), 2);
    }

    #[test]
    fn test_tracker_track_error() {
        let temp_dir = std::env::temp_dir();
        let mut tracker = EventTracker::new(temp_dir.join("test-analytics.jsonl"));

        tracker.track_error("IOError", "file not found").unwrap();

        assert_eq!(tracker.store.buffered_count(), 1);
    }

    #[test]
    fn test_event_serialization() {
        let mut props = HashMap::new();
        props.insert("test".to_string(), serde_json::json!(42));

        let event = AnalyticsEvent::new("test_event".to_string(), props);

        let json = serde_json::to_string(&event).unwrap();
        let decoded: AnalyticsEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.event_type, event.event_type);
        assert_eq!(decoded.id, event.id);
    }
}
