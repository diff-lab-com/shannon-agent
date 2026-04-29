//! Scheduled routines for recurring task execution.
//!
//! Provides a cron-like system for scheduling prompts to run at intervals
//! or specific times, integrated with the REPL tick loop.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single scheduled routine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledRoutine {
    /// Unique ID.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// The prompt to execute when the routine fires.
    pub prompt: String,
    /// Interval in seconds between firings.
    pub interval_secs: u64,
    /// When the routine was created.
    pub created_at: DateTime<Utc>,
    /// When the routine last fired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fired: Option<DateTime<Utc>>,
    /// Whether the routine is enabled.
    pub enabled: bool,
    /// How many times this routine has fired.
    pub fire_count: u32,
    /// Optional max fire count (None = unlimited).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_fires: Option<u32>,
}

impl ScheduledRoutine {
    /// Create a new routine.
    pub fn new(name: String, prompt: String, interval_secs: u64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
            name,
            prompt,
            interval_secs,
            created_at: Utc::now(),
            last_fired: None,
            enabled: true,
            fire_count: 0,
            max_fires: None,
        }
    }

    /// Check if the routine should fire now.
    pub fn should_fire(&self) -> bool {
        if !self.enabled {
            return false;
        }
        if let Some(max) = self.max_fires {
            if self.fire_count >= max {
                return false;
            }
        }
        match self.last_fired {
            None => true,
            Some(last) => {
                let elapsed = Utc::now()
                    .signed_duration_since(last)
                    .num_seconds()
                    .max(0) as u64;
                elapsed >= self.interval_secs
            }
        }
    }

    /// Mark the routine as fired now.
    pub fn mark_fired(&mut self) {
        self.last_fired = Some(Utc::now());
        self.fire_count += 1;
    }
}

/// Manager for scheduled routines.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoutineManager {
    /// Active routines keyed by ID.
    pub routines: HashMap<String, ScheduledRoutine>,
}

impl RoutineManager {
    /// Create a new empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a routine.
    pub fn add(&mut self, routine: ScheduledRoutine) -> String {
        let id = routine.id.clone();
        self.routines.insert(id.clone(), routine);
        id
    }

    /// Remove a routine by ID or name prefix.
    pub fn remove(&mut self, id_or_name: &str) -> Option<ScheduledRoutine> {
        // Try exact ID match first
        if let Some(r) = self.routines.remove(id_or_name) {
            return Some(r);
        }
        // Try name prefix match
        let key = self
            .routines
            .keys()
            .find(|k| k.starts_with(id_or_name))
            .cloned()?;
        self.routines.remove(&key)
    }

    /// Get a routine by ID.
    pub fn get(&self, id: &str) -> Option<&ScheduledRoutine> {
        self.routines.get(id)
    }

    /// Get a mutable routine by ID.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut ScheduledRoutine> {
        self.routines.get_mut(id)
    }

    /// Toggle a routine on/off.
    pub fn toggle(&mut self, id_or_name: &str) -> Option<bool> {
        // Try exact ID
        if let Some(r) = self.routines.get_mut(id_or_name) {
            r.enabled = !r.enabled;
            return Some(r.enabled);
        }
        // Try prefix
        let key = self
            .routines
            .keys()
            .find(|k| k.starts_with(id_or_name))
            .cloned()?;
        let r = self.routines.get_mut(&key)?;
        r.enabled = !r.enabled;
        Some(r.enabled)
    }

    /// Get all due prompts and mark them fired.
    pub fn drain_due(&mut self) -> Vec<(String, String)> {
        let mut due = Vec::new();
        for routine in self.routines.values_mut() {
            if routine.should_fire() {
                let name = routine.name.clone();
                let prompt = routine.prompt.clone();
                routine.mark_fired();
                due.push((name, prompt));
            }
        }
        due
    }

    /// List all routines.
    pub fn list(&self) -> Vec<&ScheduledRoutine> {
        let mut routines: Vec<_> = self.routines.values().collect();
        routines.sort_by_key(|r| r.created_at);
        routines
    }

    /// Save routines to a file.
    pub fn save_to_file(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)
    }

    /// Load routines from a file.
    pub fn load_from_file(path: &std::path::Path) -> Result<Self, std::io::Error> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Get the default storage path.
    pub fn default_storage_path() -> std::path::PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".shannon")
            .join("routines.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routine_should_fire_initially() {
        let r = ScheduledRoutine::new("test".into(), "hello".into(), 60);
        assert!(r.should_fire());
    }

    #[test]
    fn test_routine_not_before_interval() {
        let mut r = ScheduledRoutine::new("test".into(), "hello".into(), 3600);
        r.mark_fired();
        assert!(!r.should_fire());
    }

    #[test]
    fn test_routine_max_fires() {
        let mut r = ScheduledRoutine::new("test".into(), "hello".into(), 0);
        r.max_fires = Some(1);
        r.mark_fired();
        assert!(!r.should_fire());
    }

    #[test]
    fn test_routine_manager_add_remove() {
        let mut mgr = RoutineManager::new();
        let r = ScheduledRoutine::new("test".into(), "hello".into(), 60);
        let id = r.id.clone();
        mgr.add(r);
        assert!(mgr.get(&id).is_some());
        mgr.remove(&id);
        assert!(mgr.get(&id).is_none());
    }

    #[test]
    fn test_routine_manager_toggle() {
        let mut mgr = RoutineManager::new();
        let r = ScheduledRoutine::new("test".into(), "hello".into(), 60);
        let id = r.id.clone();
        mgr.add(r);
        let enabled = mgr.toggle(&id);
        assert_eq!(enabled, Some(false));
    }

    #[test]
    fn test_serialization() {
        let mut mgr = RoutineManager::new();
        mgr.add(ScheduledRoutine::new("test".into(), "hello".into(), 60));
        let json = serde_json::to_string(&mgr).unwrap();
        let back: RoutineManager = serde_json::from_str(&json).unwrap();
        assert_eq!(back.routines.len(), 1);
    }
}
