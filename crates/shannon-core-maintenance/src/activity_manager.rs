//! # Activity Manager
//!
//! Long-running task activity tracking with progress reporting and system-level
//! activity indication. Inspired by Claude Code's `activityManager.ts`.
//!
//! ## Architecture
//!
//! - [`Activity`]: A tracked task with status and progress
//! - [`ActivityStatus`]: Lifecycle states (Pending, Running, Completed, Failed, Cancelled)
//! - [`ActivityManager`]: Singleton-style tracker for all active activities
//!
//! ## Usage
//!
//! ```rust
//! use shannon_core_maintenance::activity_manager::{ActivityManager, ActivityStatus};
//!
//! let mut mgr = ActivityManager::new();
//! let id = mgr.start_activity("build", "Building project...");
//!
//! mgr.begin_activity(&id).unwrap();
//! mgr.update_progress(&id, 50).unwrap();
//! mgr.complete_activity(&id).unwrap();
//!
//! let active = mgr.get_active();
//! assert!(active.is_empty());
//! ```

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};
use uuid::Uuid;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during activity management.
#[derive(Error, Debug)]
pub enum ActivityError {
    #[error("Activity not found: {0}")]
    NotFound(Uuid),

    #[error("Invalid state transition: {from} -> {to}")]
    InvalidTransition {
        from: ActivityStatus,
        to: ActivityStatus,
    },

    #[error("Activity already exists: {0}")]
    AlreadyExists(Uuid),

    #[error("Progress out of range: {0} (must be 0-100)")]
    InvalidProgress(u8),
}

// ============================================================================
// Data Types
// ============================================================================

/// The lifecycle status of an activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityStatus {
    /// Activity has been created but not started.
    Pending,
    /// Activity is currently running.
    Running,
    /// Activity completed successfully.
    Completed,
    /// Activity failed with an error.
    Failed,
    /// Activity was cancelled by the user.
    Cancelled,
}

impl std::fmt::Display for ActivityStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// A tracked activity with status and progress information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    /// Unique identifier for this activity.
    pub id: Uuid,
    /// Short name for the activity (e.g., "build", "test", "deploy").
    pub name: String,
    /// Current lifecycle status.
    pub status: ActivityStatus,
    /// Progress percentage (0-100).
    pub progress: u8,
    /// When the activity was created.
    pub started_at: DateTime<Utc>,
    /// When the activity reached a terminal state, if applicable.
    pub completed_at: Option<DateTime<Utc>>,
    /// Human-readable description of the activity.
    pub description: String,
    /// Optional error message if the activity failed.
    pub error_message: Option<String>,
    /// Arbitrary metadata.
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Activity {
    /// Create a new activity with the given name and description.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            status: ActivityStatus::Pending,
            progress: 0,
            started_at: Utc::now(),
            completed_at: None,
            description: description.into(),
            error_message: None,
            metadata: HashMap::new(),
        }
    }

    /// Check if the activity is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            ActivityStatus::Completed
                | ActivityStatus::Failed
                | ActivityStatus::Cancelled
        )
    }

    /// Check if the activity is currently active (running).
    pub fn is_active(&self) -> bool {
        self.status == ActivityStatus::Running
    }

    /// Get the elapsed time since the activity started.
    pub fn elapsed(&self) -> Duration {
        let end = self.completed_at.unwrap_or_else(Utc::now);
        end.signed_duration_since(self.started_at)
            .to_std()
            .unwrap_or(Duration::ZERO)
    }
}

// ============================================================================
// Activity Manager
// ============================================================================

/// Manages activity tracking for long-running tasks.
///
/// Supports creating, updating, and querying activities. Optionally sets the
/// system terminal title on supported platforms.
pub struct ActivityManager {
    activities: HashMap<Uuid, Activity>,
    /// Whether to set the terminal title on activity changes.
    set_terminal_title: bool,
}

impl Default for ActivityManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivityManager {
    /// Create a new activity manager.
    pub fn new() -> Self {
        Self {
            activities: HashMap::new(),
            set_terminal_title: false,
        }
    }

    /// Create a new activity manager with terminal title support enabled.
    pub fn with_terminal_title() -> Self {
        Self {
            activities: HashMap::new(),
            set_terminal_title: true,
        }
    }

    /// Validate a state transition.
    fn validate_transition(from: ActivityStatus, to: ActivityStatus) -> Result<(), ActivityError> {
        match (from, to) {
            // Allowed transitions
            (ActivityStatus::Pending, ActivityStatus::Running) => Ok(()),
            (ActivityStatus::Pending, ActivityStatus::Cancelled) => Ok(()),
            (ActivityStatus::Running, ActivityStatus::Completed) => Ok(()),
            (ActivityStatus::Running, ActivityStatus::Failed) => Ok(()),
            (ActivityStatus::Running, ActivityStatus::Cancelled) => Ok(()),
            // Same state is a no-op (allowed)
            (a, b) if a == b => Ok(()),
            // Everything else is invalid
            _ => Err(ActivityError::InvalidTransition { from, to }),
        }
    }

    /// Update the terminal title based on active activities.
    fn update_terminal_title(&self) {
        if !self.set_terminal_title {
            return;
        }

        let active: Vec<&Activity> = self
            .activities
            .values()
            .filter(|a| a.is_active())
            .collect();

        if active.is_empty() {
            let _ = std::process::Command::new("printf")
                .args(["\\033]0;Shannon Code\\007"])
                .output();
        } else {
            let title = if active.len() == 1 {
                format!(
                    "Shannon Code - {} ({}/{})",
                    active[0].name, active[0].progress, 100
                )
            } else {
                format!("Shannon Code - {} tasks running", active.len())
            };
            // Use ANSI escape sequence to set terminal title.
            let _ = std::process::Command::new("printf")
                .args([format!("\\033]0;{}\\007", title)])
                .output();
        }
    }

    /// Start a new activity. Returns the activity ID.
    ///
    /// The activity starts in `Pending` status. Use `complete_activity`,
    /// `fail_activity`, or `cancel_activity` to move it to a terminal state.
    pub fn start_activity(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Uuid {
        let activity = Activity::new(name, description);
        let id = activity.id;
        info!(
            activity_id = %id,
            name = %activity.name,
            "Started activity"
        );
        self.activities.insert(id, activity);
        self.update_terminal_title();
        id
    }

    /// Transition a pending activity to running.
    pub fn begin_activity(&mut self, id: &Uuid) -> Result<(), ActivityError> {
        let activity = self
            .activities
            .get_mut(id)
            .ok_or(ActivityError::NotFound(*id))?;
        Self::validate_transition(activity.status, ActivityStatus::Running)?;
        activity.status = ActivityStatus::Running;
        activity.started_at = Utc::now();
        debug!(activity_id = %id, "Activity begun");
        self.update_terminal_title();
        Ok(())
    }

    /// Update the progress of a running activity (0-100).
    pub fn update_progress(&mut self, id: &Uuid, progress: u8) -> Result<(), ActivityError> {
        if progress > 100 {
            return Err(ActivityError::InvalidProgress(progress));
        }

        let activity = self
            .activities
            .get_mut(id)
            .ok_or(ActivityError::NotFound(*id))?;

        if activity.status != ActivityStatus::Running {
            return Err(ActivityError::InvalidTransition {
                from: activity.status,
                to: ActivityStatus::Running,
            });
        }

        activity.progress = progress;
        debug!(
            activity_id = %id,
            progress = progress,
            "Updated activity progress"
        );
        self.update_terminal_title();
        Ok(())
    }

    /// Complete an activity successfully.
    pub fn complete_activity(&mut self, id: &Uuid) -> Result<(), ActivityError> {
        let activity = self
            .activities
            .get_mut(id)
            .ok_or(ActivityError::NotFound(*id))?;
        Self::validate_transition(activity.status, ActivityStatus::Completed)?;
        activity.status = ActivityStatus::Completed;
        activity.progress = 100;
        activity.completed_at = Some(Utc::now());
        info!(activity_id = %id, "Activity completed");
        self.update_terminal_title();
        Ok(())
    }

    /// Mark an activity as failed with an error message.
    pub fn fail_activity(
        &mut self,
        id: &Uuid,
        error_message: impl Into<String>,
    ) -> Result<(), ActivityError> {
        let activity = self
            .activities
            .get_mut(id)
            .ok_or(ActivityError::NotFound(*id))?;
        Self::validate_transition(activity.status, ActivityStatus::Failed)?;
        activity.status = ActivityStatus::Failed;
        activity.completed_at = Some(Utc::now());
        activity.error_message = Some(error_message.into());
        warn!(
            activity_id = %id,
            "Activity failed"
        );
        self.update_terminal_title();
        Ok(())
    }

    /// Cancel an activity.
    pub fn cancel_activity(&mut self, id: &Uuid) -> Result<(), ActivityError> {
        let activity = self
            .activities
            .get_mut(id)
            .ok_or(ActivityError::NotFound(*id))?;
        Self::validate_transition(activity.status, ActivityStatus::Cancelled)?;
        activity.status = ActivityStatus::Cancelled;
        activity.completed_at = Some(Utc::now());
        info!(activity_id = %id, "Activity cancelled");
        self.update_terminal_title();
        Ok(())
    }

    /// Get an activity by ID.
    pub fn get(&self, id: &Uuid) -> Option<&Activity> {
        self.activities.get(id)
    }

    /// Get all currently active (running) activities.
    pub fn get_active(&self) -> Vec<&Activity> {
        self.activities.values().filter(|a| a.is_active()).collect()
    }

    /// Get all activities.
    pub fn get_all(&self) -> Vec<&Activity> {
        self.activities.values().collect()
    }

    /// Get all activities matching a given status.
    pub fn get_by_status(&self, status: ActivityStatus) -> Vec<&Activity> {
        self.activities
            .values()
            .filter(|a| a.status == status)
            .collect()
    }

    /// Get all activities matching a given name.
    pub fn get_by_name(&self, name: &str) -> Vec<&Activity> {
        self.activities
            .values()
            .filter(|a| a.name == name)
            .collect()
    }

    /// Remove a terminal activity from the manager.
    ///
    /// Returns the removed activity, or an error if it is not in a terminal state.
    pub fn remove(&mut self, id: &Uuid) -> Result<Activity, ActivityError> {
        let activity = self
            .activities
            .get(id)
            .ok_or(ActivityError::NotFound(*id))?;
        if !activity.is_terminal() {
            return Err(ActivityError::InvalidTransition {
                from: activity.status,
                to: ActivityStatus::Completed, // representative error
            });
        }
        let activity = self.activities.remove(id).unwrap();
        self.update_terminal_title();
        Ok(activity)
    }

    /// Remove all terminal activities.
    ///
    /// Returns the number of activities removed.
    pub fn cleanup(&mut self) -> usize {
        let before = self.activities.len();
        self.activities.retain(|_, a| !a.is_terminal());
        let removed = before - self.activities.len();
        if removed > 0 {
            debug!(removed = removed, "Cleaned up terminal activities");
            self.update_terminal_title();
        }
        removed
    }

    /// Get the total number of tracked activities.
    pub fn count(&self) -> usize {
        self.activities.len()
    }

    /// Get the number of active (running) activities.
    pub fn active_count(&self) -> usize {
        self.activities.values().filter(|a| a.is_active()).count()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_activity_creation() {
        let activity = Activity::new("build", "Building project");
        assert_eq!(activity.name, "build");
        assert_eq!(activity.description, "Building project");
        assert_eq!(activity.status, ActivityStatus::Pending);
        assert_eq!(activity.progress, 0);
        assert!(activity.completed_at.is_none());
        assert!(activity.error_message.is_none());
    }

    #[test]
    fn test_activity_is_terminal() {
        let mut a = Activity::new("test", "test");
        assert!(!a.is_terminal());

        a.status = ActivityStatus::Completed;
        assert!(a.is_terminal());

        a.status = ActivityStatus::Failed;
        assert!(a.is_terminal());

        a.status = ActivityStatus::Cancelled;
        assert!(a.is_terminal());

        a.status = ActivityStatus::Running;
        assert!(!a.is_terminal());
    }

    #[test]
    fn test_activity_is_active() {
        let mut a = Activity::new("test", "test");
        assert!(!a.is_active());

        a.status = ActivityStatus::Running;
        assert!(a.is_active());

        a.status = ActivityStatus::Completed;
        assert!(!a.is_active());
    }

    #[test]
    fn test_activity_elapsed() {
        let a = Activity::new("test", "test");
        let elapsed = a.elapsed();
        assert!(elapsed.as_millis() < 100); // just created, should be < 100ms
    }

    #[test]
    fn test_status_display() {
        assert_eq!(ActivityStatus::Pending.to_string(), "pending");
        assert_eq!(ActivityStatus::Running.to_string(), "running");
        assert_eq!(ActivityStatus::Completed.to_string(), "completed");
        assert_eq!(ActivityStatus::Failed.to_string(), "failed");
        assert_eq!(ActivityStatus::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn test_manager_start_activity() {
        let mut mgr = ActivityManager::new();
        let id = mgr.start_activity("build", "Building...");
        assert_eq!(mgr.count(), 1);
        let activity = mgr.get(&id).unwrap();
        assert_eq!(activity.name, "build");
        assert_eq!(activity.status, ActivityStatus::Pending);
    }

    #[test]
    fn test_manager_begin_activity() {
        let mut mgr = ActivityManager::new();
        let id = mgr.start_activity("build", "Building...");
        mgr.begin_activity(&id).unwrap();
        let activity = mgr.get(&id).unwrap();
        assert_eq!(activity.status, ActivityStatus::Running);
    }

    #[test]
    fn test_manager_begin_nonexistent() {
        let mut mgr = ActivityManager::new();
        let result = mgr.begin_activity(&Uuid::new_v4());
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_update_progress() {
        let mut mgr = ActivityManager::new();
        let id = mgr.start_activity("build", "Building...");
        mgr.begin_activity(&id).unwrap();
        mgr.update_progress(&id, 50).unwrap();

        let activity = mgr.get(&id).unwrap();
        assert_eq!(activity.progress, 50);
    }

    #[test]
    fn test_manager_update_progress_invalid() {
        let mut mgr = ActivityManager::new();
        let id = mgr.start_activity("build", "Building...");
        mgr.begin_activity(&id).unwrap();
        let result = mgr.update_progress(&id, 150);
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_update_progress_not_running() {
        let mut mgr = ActivityManager::new();
        let id = mgr.start_activity("build", "Building...");
        let result = mgr.update_progress(&id, 50);
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_complete_activity() {
        let mut mgr = ActivityManager::new();
        let id = mgr.start_activity("build", "Building...");
        mgr.begin_activity(&id).unwrap();
        mgr.complete_activity(&id).unwrap();

        let activity = mgr.get(&id).unwrap();
        assert_eq!(activity.status, ActivityStatus::Completed);
        assert_eq!(activity.progress, 100);
        assert!(activity.completed_at.is_some());
    }

    #[test]
    fn test_manager_fail_activity() {
        let mut mgr = ActivityManager::new();
        let id = mgr.start_activity("build", "Building...");
        mgr.begin_activity(&id).unwrap();
        mgr.fail_activity(&id, "compilation error").unwrap();

        let activity = mgr.get(&id).unwrap();
        assert_eq!(activity.status, ActivityStatus::Failed);
        assert_eq!(activity.error_message.as_deref(), Some("compilation error"));
        assert!(activity.completed_at.is_some());
    }

    #[test]
    fn test_manager_cancel_activity() {
        let mut mgr = ActivityManager::new();
        let id = mgr.start_activity("build", "Building...");
        mgr.cancel_activity(&id).unwrap();

        let activity = mgr.get(&id).unwrap();
        assert_eq!(activity.status, ActivityStatus::Cancelled);
        assert!(activity.completed_at.is_some());
    }

    #[test]
    fn test_manager_cancel_running_activity() {
        let mut mgr = ActivityManager::new();
        let id = mgr.start_activity("build", "Building...");
        mgr.begin_activity(&id).unwrap();
        mgr.cancel_activity(&id).unwrap();

        let activity = mgr.get(&id).unwrap();
        assert_eq!(activity.status, ActivityStatus::Cancelled);
    }

    #[test]
    fn test_manager_get_active() {
        let mut mgr = ActivityManager::new();
        let id1 = mgr.start_activity("build", "Building...");
        let id2 = mgr.start_activity("test", "Testing...");
        mgr.begin_activity(&id1).unwrap();
        mgr.begin_activity(&id2).unwrap();

        let active = mgr.get_active();
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_manager_get_active_empty() {
        let mgr = ActivityManager::new();
        let active = mgr.get_active();
        assert!(active.is_empty());
    }

    #[test]
    fn test_manager_get_all() {
        let mut mgr = ActivityManager::new();
        mgr.start_activity("build", "Building...");
        mgr.start_activity("test", "Testing...");

        let all = mgr.get_all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_manager_get_by_status() {
        let mut mgr = ActivityManager::new();
        let id1 = mgr.start_activity("build", "Building...");
        mgr.begin_activity(&id1).unwrap();
        mgr.complete_activity(&id1).unwrap();
        let id2 = mgr.start_activity("test", "Testing...");
        mgr.begin_activity(&id2).unwrap();

        let completed = mgr.get_by_status(ActivityStatus::Completed);
        assert_eq!(completed.len(), 1);

        let running = mgr.get_by_status(ActivityStatus::Running);
        assert_eq!(running.len(), 1);
    }

    #[test]
    fn test_manager_get_by_name() {
        let mut mgr = ActivityManager::new();
        mgr.start_activity("build", "Building project");
        mgr.start_activity("build", "Building docs");
        mgr.start_activity("test", "Testing");

        let builds = mgr.get_by_name("build");
        assert_eq!(builds.len(), 2);

        let tests = mgr.get_by_name("test");
        assert_eq!(tests.len(), 1);
    }

    #[test]
    fn test_manager_remove_terminal() {
        let mut mgr = ActivityManager::new();
        let id = mgr.start_activity("build", "Building...");
        mgr.begin_activity(&id).unwrap();
        mgr.complete_activity(&id).unwrap();

        let removed = mgr.remove(&id).unwrap();
        assert_eq!(removed.status, ActivityStatus::Completed);
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn test_manager_remove_non_terminal_fails() {
        let mut mgr = ActivityManager::new();
        let id = mgr.start_activity("build", "Building...");

        let result = mgr.remove(&id);
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_cleanup() {
        let mut mgr = ActivityManager::new();
        let id1 = mgr.start_activity("build", "Building...");
        mgr.begin_activity(&id1).unwrap();
        mgr.complete_activity(&id1).unwrap();
        let id2 = mgr.start_activity("test", "Testing...");
        mgr.begin_activity(&id2).unwrap();

        let removed = mgr.cleanup();
        assert_eq!(removed, 1);
        assert_eq!(mgr.count(), 1);
        assert_eq!(mgr.active_count(), 1);
    }

    #[test]
    fn test_manager_counts() {
        let mut mgr = ActivityManager::new();
        assert_eq!(mgr.count(), 0);
        assert_eq!(mgr.active_count(), 0);

        let id = mgr.start_activity("build", "Building...");
        assert_eq!(mgr.count(), 1);
        assert_eq!(mgr.active_count(), 0);

        mgr.begin_activity(&id).unwrap();
        assert_eq!(mgr.active_count(), 1);

        mgr.complete_activity(&id).unwrap();
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn test_manager_with_terminal_title() {
        let _mgr = ActivityManager::with_terminal_title();
        // Just verify it can be created without panic.
        // Actual terminal title testing requires a TTY.
    }

    #[test]
    fn test_invalid_transition_completed_to_running() {
        let mut mgr = ActivityManager::new();
        let id = mgr.start_activity("build", "Building...");
        mgr.begin_activity(&id).unwrap();
        mgr.complete_activity(&id).unwrap();
        let result = mgr.begin_activity(&id);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_transition_failed_to_running() {
        let mut mgr = ActivityManager::new();
        let id = mgr.start_activity("build", "Building...");
        mgr.begin_activity(&id).unwrap();
        mgr.fail_activity(&id, "error").unwrap();
        let result = mgr.begin_activity(&id);
        assert!(result.is_err());
    }

    #[test]
    fn test_full_lifecycle() {
        let mut mgr = ActivityManager::new();

        // Start -> begin -> progress -> complete
        let id = mgr.start_activity("deploy", "Deploying to production");
        assert_eq!(mgr.get(&id).unwrap().status, ActivityStatus::Pending);

        mgr.begin_activity(&id).unwrap();
        assert_eq!(mgr.get(&id).unwrap().status, ActivityStatus::Running);

        mgr.update_progress(&id, 25).unwrap();
        assert_eq!(mgr.get(&id).unwrap().progress, 25);

        mgr.update_progress(&id, 50).unwrap();
        mgr.update_progress(&id, 75).unwrap();

        mgr.complete_activity(&id).unwrap();
        assert_eq!(mgr.get(&id).unwrap().status, ActivityStatus::Completed);
        assert_eq!(mgr.get(&id).unwrap().progress, 100);
        assert!(mgr.get(&id).unwrap().is_terminal());
        assert!(!mgr.get(&id).unwrap().is_active());
    }

    #[test]
    fn test_activity_serialization() {
        let activity = Activity::new("build", "Building project");
        let json = serde_json::to_string(&activity).unwrap();
        let parsed: Activity = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, activity.id);
        assert_eq!(parsed.name, activity.name);
        assert_eq!(parsed.status, activity.status);
    }
}
