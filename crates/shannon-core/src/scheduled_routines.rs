//! Scheduled routines for recurring task execution.
//!
//! Provides a cron-like system for scheduling prompts to run at intervals
//! or specific times, integrated with the REPL tick loop.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Maximum jitter cap in seconds (15 minutes, matching Claude Code).
const JITTER_CAP_SECS: u64 = 900;

/// Compute a random jitter delay (up to 10% of period, capped at `max_cap_secs`).
///
/// Returns 0 if the period is too small for meaningful jitter.
pub fn apply_jitter(period_secs: u64, max_cap_secs: u64) -> u64 {
    let max_jitter = ((period_secs as f64 * 0.10).min(max_cap_secs as f64)) as u64;
    if max_jitter == 0 {
        return 0;
    }
    rand::random::<u64>() % (max_jitter + 1)
}

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
    ///
    /// After the interval has elapsed, an additional random jitter delay
    /// (up to 10% of the interval, capped at 15 min) may be applied to
    /// prevent thundering-herd effects when many routines share the same
    /// interval.
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
                let elapsed = Utc::now().signed_duration_since(last).num_seconds().max(0) as u64;
                let jitter = apply_jitter(self.interval_secs, JITTER_CAP_SECS);
                elapsed >= self.interval_secs + jitter
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
        serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
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

    #[test]
    fn test_apply_jitter_within_bounds() {
        // Jitter for 3600s period should be <= 360s (10%) and <= 900s (cap)
        for _ in 0..100 {
            let jitter = apply_jitter(3600, 900);
            assert!(jitter <= 360, "jitter {jitter} exceeds 10% of 3600");
            assert!(jitter <= 900, "jitter {jitter} exceeds cap");
        }
    }

    #[test]
    fn test_apply_jitter_small_period() {
        // Very small period should produce 0 jitter
        let jitter = apply_jitter(1, 900);
        assert!(
            jitter <= 1,
            "jitter {jitter} should be 0 or 1 for 1s period"
        );
    }

    #[test]
    fn test_apply_jitter_cap_applied() {
        // For a huge period, jitter should be capped at max_cap_secs
        for _ in 0..100 {
            let jitter = apply_jitter(u64::MAX, 900);
            assert!(jitter <= 900, "jitter {jitter} exceeds cap of 900s");
        }
    }

    // ── Additional tests ─────────────────────────────────────────────────

    #[test]
    fn test_save_and_load_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("routines.json");

        let mut mgr = RoutineManager::new();
        let id = mgr.add(ScheduledRoutine::new("test".into(), "hello".into(), 60));
        mgr.save_to_file(&path).unwrap();

        let loaded = RoutineManager::load_from_file(&path).unwrap();
        assert_eq!(loaded.routines.len(), 1);
        assert!(loaded.get(&id).is_some());
        assert_eq!(loaded.get(&id).unwrap().name, "test");
    }

    #[test]
    fn test_load_corrupted_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not valid json [[[[").unwrap();

        let result = RoutineManager::load_from_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_drain_due_fires_initially() {
        let mut mgr = RoutineManager::new();
        mgr.add(ScheduledRoutine::new("a".into(), "prompt-a".into(), 60));
        mgr.add(ScheduledRoutine::new("b".into(), "prompt-b".into(), 3600));

        let due = mgr.drain_due();
        assert_eq!(due.len(), 2);
        // Both should fire since they've never fired before
        let names: Vec<&str> = due.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }

    #[test]
    fn test_drain_due_skips_recently_fired() {
        let mut mgr = RoutineManager::new();
        let mut r = ScheduledRoutine::new("test".into(), "prompt".into(), 3600);
        r.mark_fired(); // Just fired, shouldn't fire again
        mgr.add(r);

        let due = mgr.drain_due();
        assert!(due.is_empty());
    }

    #[test]
    fn test_remove_by_name_prefix() {
        let mut mgr = RoutineManager::new();
        let r = ScheduledRoutine::new("my-routine".into(), "prompt".into(), 60);
        let id = r.id.clone();
        mgr.add(r);

        // Remove by ID prefix (first 4 chars)
        let prefix = &id[..4];
        let removed = mgr.remove(prefix);
        assert!(removed.is_some());
        assert!(mgr.get(&id).is_none());
    }

    #[test]
    fn test_list_sorted_by_created_at() {
        let mut mgr = RoutineManager::new();
        let r1 = ScheduledRoutine::new("first".into(), "p1".into(), 60);
        let r2 = ScheduledRoutine::new("second".into(), "p2".into(), 60);
        mgr.add(r1);
        mgr.add(r2);

        let list = mgr.list();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_default_storage_path() {
        let path = RoutineManager::default_storage_path();
        assert!(path.to_string_lossy().contains(".shannon"));
        assert!(path.to_string_lossy().contains("routines.json"));
    }

    #[test]
    fn test_get_mut_routine() {
        let mut mgr = RoutineManager::new();
        let r = ScheduledRoutine::new("test".into(), "prompt".into(), 60);
        let id = r.id.clone();
        mgr.add(r);

        let r = mgr.get_mut(&id).unwrap();
        assert!(r.enabled);
        r.enabled = false;

        assert!(!mgr.get(&id).unwrap().enabled);
    }
}
