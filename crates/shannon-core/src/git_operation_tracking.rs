//! # Git Operation Tracking
//!
//! Audit logging for git operations performed during a session.
//! Records operation type, arguments, timestamps, and success/failure status.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// A recorded git operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitOperation {
    /// Type of git operation (e.g., "commit", "push", "checkout").
    pub operation_type: String,
    /// Arguments passed to the git command.
    pub args: Vec<String>,
    /// ISO 8601 timestamp of when the operation occurred.
    pub timestamp: String,
    /// Working directory where the operation was executed, if known.
    pub working_dir: Option<String>,
    /// Whether the operation succeeded.
    pub success: bool,
}

impl GitOperation {
    /// Create a new git operation record.
    pub fn new(operation_type: &str, args: Vec<String>, success: bool) -> Self {
        Self {
            operation_type: operation_type.to_string(),
            args,
            timestamp: chrono::Utc::now().to_rfc3339(),
            working_dir: None,
            success,
        }
    }

    /// Create a new git operation record with a working directory.
    pub fn with_working_dir(
        operation_type: &str,
        args: Vec<String>,
        working_dir: &str,
        success: bool,
    ) -> Self {
        Self {
            operation_type: operation_type.to_string(),
            args,
            timestamp: chrono::Utc::now().to_rfc3339(),
            working_dir: Some(working_dir.to_string()),
            success,
        }
    }
}

/// Tracker for git operations during a session.
///
/// Records operations in memory for audit and review purposes.
pub struct GitOperationTracker {
    /// In-memory operation history.
    history: Mutex<Vec<GitOperation>>,
}

impl GitOperationTracker {
    /// Create a new git operation tracker.
    pub fn new() -> Self {
        Self {
            history: Mutex::new(Vec::new()),
        }
    }

    /// Record a git operation.
    pub fn record(&self, op: GitOperation) {
        if let Ok(mut history) = self.history.lock() {
            history.push(op);
        }
    }

    /// Get the full operation history.
    pub fn get_history(&self) -> Vec<GitOperation> {
        self.history.lock().map(|h| h.clone()).unwrap_or_default()
    }

    /// Get the most recent operations, limited to `count`.
    ///
    /// Returns the last `count` operations in chronological order.
    pub fn get_recent(&self, count: usize) -> Vec<GitOperation> {
        self.history
            .lock()
            .map(|h| {
                let len = h.len();
                let start = len.saturating_sub(count);
                h[start..].to_vec()
            })
            .unwrap_or_default()
    }

    /// Clear all recorded operations.
    pub fn clear(&self) {
        if let Ok(mut history) = self.history.lock() {
            history.clear();
        }
    }

    /// Get the total number of recorded operations.
    pub fn len(&self) -> usize {
        self.history.lock().map(|h| h.len()).unwrap_or(0)
    }

    /// Check if no operations have been recorded.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for GitOperationTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracker_new() {
        let tracker = GitOperationTracker::new();
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_tracker_default() {
        let tracker = GitOperationTracker::default();
        assert_eq!(tracker.len(), 0);
    }

    #[test]
    fn test_record_single() {
        let tracker = GitOperationTracker::new();
        let op = GitOperation::new("commit", vec!["-m".to_string(), "init".to_string()], true);
        tracker.record(op);

        assert_eq!(tracker.len(), 1);
        let history = tracker.get_history();
        assert_eq!(history[0].operation_type, "commit");
        assert_eq!(history[0].args, vec!["-m", "init"]);
        assert!(history[0].success);
    }

    #[test]
    fn test_record_multiple() {
        let tracker = GitOperationTracker::new();

        tracker.record(GitOperation::new("init", vec![], true));
        tracker.record(GitOperation::new("add", vec![".".to_string()], true));
        tracker.record(GitOperation::new(
            "commit",
            vec!["-m".to_string(), "first".to_string()],
            true,
        ));

        assert_eq!(tracker.len(), 3);
    }

    #[test]
    fn test_record_with_working_dir() {
        let tracker = GitOperationTracker::new();
        let op = GitOperation::with_working_dir(
            "checkout",
            vec!["-b".to_string(), "feature".to_string()],
            "/home/user/project",
            true,
        );
        tracker.record(op);

        let history = tracker.get_history();
        assert_eq!(
            history[0].working_dir,
            Some("/home/user/project".to_string())
        );
    }

    #[test]
    fn test_record_failure() {
        let tracker = GitOperationTracker::new();
        let op = GitOperation::new(
            "push",
            vec!["origin".to_string(), "main".to_string()],
            false,
        );
        tracker.record(op);

        let history = tracker.get_history();
        assert!(!history[0].success);
    }

    #[test]
    fn test_get_history() {
        let tracker = GitOperationTracker::new();
        tracker.record(GitOperation::new("init", vec![], true));
        tracker.record(GitOperation::new("add", vec![".".to_string()], true));

        let history = tracker.get_history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].operation_type, "init");
        assert_eq!(history[1].operation_type, "add");
    }

    #[test]
    fn test_get_recent_less_than_total() {
        let tracker = GitOperationTracker::new();
        for i in 0..5 {
            tracker.record(GitOperation::new(
                "commit",
                vec![format!("-m msg{}", i)],
                true,
            ));
        }

        let recent = tracker.get_recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].args[0], "-m msg2");
        assert_eq!(recent[2].args[0], "-m msg4");
    }

    #[test]
    fn test_get_recent_more_than_total() {
        let tracker = GitOperationTracker::new();
        tracker.record(GitOperation::new("init", vec![], true));
        tracker.record(GitOperation::new("add", vec![], true));

        let recent = tracker.get_recent(10);
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn test_get_recent_empty() {
        let tracker = GitOperationTracker::new();
        let recent = tracker.get_recent(5);
        assert!(recent.is_empty());
    }

    #[test]
    fn test_get_recent_zero() {
        let tracker = GitOperationTracker::new();
        tracker.record(GitOperation::new("init", vec![], true));

        let recent = tracker.get_recent(0);
        assert!(recent.is_empty());
    }

    #[test]
    fn test_clear() {
        let tracker = GitOperationTracker::new();
        tracker.record(GitOperation::new("commit", vec![], true));
        tracker.record(GitOperation::new("push", vec![], true));
        assert_eq!(tracker.len(), 2);

        tracker.clear();
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_timestamp_format() {
        let op = GitOperation::new("commit", vec![], true);
        let parsed = chrono::DateTime::parse_from_rfc3339(&op.timestamp);
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_serialization() {
        let op = GitOperation::with_working_dir(
            "checkout",
            vec!["-b".to_string(), "feat".to_string()],
            "/project",
            true,
        );
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("checkout"));
        assert!(json.contains("feat"));
        assert!(json.contains("/project"));
    }
}
