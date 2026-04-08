//! Activity management

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Duration;

/// Activity event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub id: uuid::Uuid,
    pub event_type: ActivityType,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub metadata: serde_json::Value,
}

/// Activity type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityType {
    UserInput,
    ToolExecution,
    ApiCall,
    SystemEvent,
    Idle,
}

/// Activity summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivitySummary {
    pub total_events: usize,
    pub active_duration: Duration,
    pub last_activity: Option<chrono::DateTime<chrono::Utc>>,
    pub idle_duration: Duration,
}

/// Activity manager
pub struct ActivityManager {
    events: VecDeque<ActivityEvent>,
    max_events: usize,
    idle_threshold: Duration,
    last_activity: Option<chrono::DateTime<chrono::Utc>>,
}

impl ActivityManager {
    pub fn new(max_events: usize, idle_threshold: Duration) -> Self {
        Self {
            events: VecDeque::with_capacity(max_events),
            max_events,
            idle_threshold,
            last_activity: None,
        }
    }

    /// Record an activity event
    pub fn record_activity(&mut self, event_type: ActivityType, metadata: serde_json::Value) {
        let now = chrono::Utc::now();
        let event = ActivityEvent {
            id: uuid::Uuid::new_v4(),
            event_type,
            timestamp: now,
            metadata,
        };

        self.events.push_back(event);
        self.last_activity = Some(now);

        // Trim old events if needed
        while self.events.len() > self.max_events {
            self.events.pop_front();
        }
    }

    /// Get activity summary
    pub fn get_summary(&self) -> ActivitySummary {
        let now = chrono::Utc::now();
        let idle_duration = self.last_activity
            .map(|la| {
                let diff = now - la;
                diff.to_std().unwrap_or(Duration::ZERO)
            })
            .unwrap_or(Duration::ZERO);

        let (active_duration, _first_event) = if let (Some(first), Some(last)) = (
            self.events.front(),
            self.events.back(),
        ) {
            let diff = last.timestamp - first.timestamp;
            let duration = diff.to_std().unwrap_or(Duration::ZERO);
            (duration, Some(first.timestamp))
        } else {
            (Duration::ZERO, None)
        };

        ActivitySummary {
            total_events: self.events.len(),
            active_duration,
            last_activity: self.last_activity,
            idle_duration,
        }
    }

    /// Check if currently idle
    pub fn is_idle(&self) -> bool {
        let now = chrono::Utc::now();
        self.last_activity
            .map(|la| {
                let diff = now - la;
                diff.to_std().unwrap_or(Duration::ZERO) > self.idle_threshold
            })
            .unwrap_or(false)
    }

    /// Get time since last activity
    pub fn time_since_last_activity(&self) -> Option<Duration> {
        let now = chrono::Utc::now();
        self.last_activity.map(|la| {
            let diff = now - la;
            diff.to_std().unwrap_or(Duration::ZERO)
        })
    }

    /// Clear old events
    pub fn clear_old_events(&mut self, older_than: Duration) {
        let now = chrono::Utc::now();
        while let Some(event) = self.events.front() {
            let diff = now - event.timestamp;
            if diff.to_std().unwrap_or(Duration::ZERO) > older_than {
                self.events.pop_front();
            } else {
                break;
            }
        }
    }
}
