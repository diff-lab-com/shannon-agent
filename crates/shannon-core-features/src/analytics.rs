//! Analytics and usage tracking

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

/// Analytics event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsEvent {
    pub id: Uuid,
    pub event_type: String,
    pub properties: HashMap<String, serde_json::Value>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub user_id: Option<String>,
}

/// Analytics store
pub struct AnalyticsStore {
    events: Vec<AnalyticsEvent>,
    storage_path: Option<PathBuf>,
}

impl AnalyticsStore {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            storage_path: None,
        }
    }

    pub fn with_storage_path(mut self, path: PathBuf) -> Self {
        self.storage_path = Some(path);
        self
    }

    /// Track an event
    pub fn track(&mut self, event_type: String, properties: HashMap<String, serde_json::Value>) -> Uuid {
        let event = AnalyticsEvent {
            id: Uuid::new_v4(),
            event_type,
            properties,
            timestamp: chrono::Utc::now(),
            user_id: None,
        };

        let id = event.id;
        self.events.push(event);
        id
    }

    /// Get events by type
    pub fn get_events_by_type(&self, event_type: &str) -> Vec<&AnalyticsEvent> {
        self.events.iter()
            .filter(|e| e.event_type == event_type)
            .collect()
    }

    /// Get recent events
    pub fn recent_events(&self, limit: usize) -> Vec<&AnalyticsEvent> {
        self.events.iter()
            .rev()
            .take(limit)
            .collect()
    }
}

impl Default for AnalyticsStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Event tracker
pub struct EventTracker {
    store: AnalyticsStore,
}

impl EventTracker {
    pub fn new(store: AnalyticsStore) -> Self {
        Self { store }
    }

    /// Track a user action
    pub fn track_action(&mut self, action: &str, details: HashMap<String, serde_json::Value>) -> Uuid {
        self.store.track(format!("action:{}", action), details)
    }

    /// Track an error
    pub fn track_error(&mut self, error: &str, details: HashMap<String, serde_json::Value>) -> Uuid {
        self.store.track("error".to_string(), {
            let mut props = details;
            props.insert("error".to_string(), serde_json::json!(error));
            props
        })
    }
}
