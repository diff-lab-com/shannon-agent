//! Git-based checkpoint system for undo/revert operations.
//!
//! Creates lightweight git commits before file-modifying tool executions,
//! allowing users to revert to a known-good state via `/undo`.

use std::process::Command;
use std::sync::{Arc, Mutex};

/// Maximum number of checkpoints to retain.
const MAX_CHECKPOINTS: usize = 50;

/// A single checkpoint representing a point-in-time snapshot.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    /// Git commit hash.
    pub hash: String,
    /// Short hash (first 7 chars).
    pub short_hash: String,
    /// Description of what triggered this checkpoint.
    pub description: String,
    /// Timestamp (seconds since epoch).
    pub timestamp: i64,
}

/// Manages git-based checkpoints for undo operations.
#[derive(Debug, Clone)]
pub struct CheckpointManager {
    checkpoints: Arc<Mutex<Vec<Checkpoint>>>,
    enabled: bool,
}

impl CheckpointManager {
    /// Create a new checkpoint manager.
    pub fn new() -> Self {
        Self {
            checkpoints: Arc::new(Mutex::new(Vec::new())),
            enabled: Self::is_git_repo(),
        }
    }

    /// Check if the current directory is inside a git repo.
    fn is_git_repo() -> bool {
        Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Whether checkpointing is available (requires a git repo).
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Create a checkpoint before a tool execution.
    ///
    /// Returns `Ok(Checkpoint)` if a checkpoint was created, or `Err` with
    /// a reason if not (e.g., no changes to save, not a git repo).
    pub fn create_checkpoint(&self, tool_name: &str, description: &str) -> Result<Checkpoint, String> {
        if !self.enabled {
            return Err("Not in a git repository — checkpoints unavailable".to_string());
        }

        // Check if there are any changes to checkpoint
        let has_changes = {
            let output = Command::new("git")
                .args(["status", "--porcelain"])
                .output()
                .map_err(|e| format!("Failed to check git status: {e}"))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            !stdout.trim().is_empty()
        };

        if !has_changes {
            // Nothing to checkpoint — record the current HEAD as a reference point
            let hash = Self::get_head_hash()?;
            let cp = Checkpoint {
                short_hash: hash[..7.min(hash.len())].to_string(),
                hash: hash.clone(),
                description: format!("pre-{tool_name}: {description} (no changes)"),
                timestamp: chrono::Utc::now().timestamp(),
            };
            self.checkpoints.lock().unwrap().push(cp.clone());
            return Ok(cp);
        }

        // Stage all changes
        let stage_output = Command::new("git")
            .args(["add", "-A"])
            .output()
            .map_err(|e| format!("Failed to stage changes: {e}"))?;
        if !stage_output.status.success() {
            return Err("Failed to stage changes for checkpoint".to_string());
        }

        // Create the checkpoint commit
        let commit_msg = format!("shannon: checkpoint before {tool_name}\n\n{description}");
        let commit_output = Command::new("git")
            .args(["commit", "-m", &commit_msg, "--no-gpg-sign"])
            .output()
            .map_err(|e| format!("Failed to create checkpoint commit: {e}"))?;

        if !commit_output.status.success() {
            let stderr = String::from_utf8_lossy(&commit_output.stderr);
            // "nothing to commit" is OK — means the changes were already committed
            if !stderr.contains("nothing to commit") {
                return Err(format!("Checkpoint commit failed: {stderr}"));
            }
        }

        let hash = Self::get_head_hash()?;
        let cp = Checkpoint {
            short_hash: hash[..7.min(hash.len())].to_string(),
            hash: hash.clone(),
            description: format!("pre-{tool_name}: {description}"),
            timestamp: chrono::Utc::now().timestamp(),
        };

        let mut checkpoints = self.checkpoints.lock().unwrap();
        checkpoints.push(cp.clone());

        // Trim old checkpoints
        if checkpoints.len() > MAX_CHECKPOINTS {
            let drain_count = checkpoints.len() - MAX_CHECKPOINTS;
            checkpoints.drain(..drain_count);
        }

        Ok(cp)
    }

    /// List all stored checkpoints (most recent last).
    pub fn list_checkpoints(&self) -> Vec<Checkpoint> {
        self.checkpoints.lock().unwrap().clone()
    }

    /// Revert to a specific checkpoint by index (0 = oldest).
    ///
    /// Returns the checkpoint that was reverted to.
    pub fn revert_to(&self, index: usize) -> Result<Checkpoint, String> {
        if !self.enabled {
            return Err("Not in a git repository".to_string());
        }

        let cp = {
            let checkpoints = self.checkpoints.lock().unwrap();
            if index >= checkpoints.len() {
                return Err(format!(
                    "Invalid checkpoint index {index}. Available: 0..{}",
                    checkpoints.len().saturating_sub(1)
                ));
            }
            checkpoints[index].clone()
        };

        // Reset to the checkpoint commit
        let output = Command::new("git")
            .args(["reset", "--hard", &cp.hash])
            .output()
            .map_err(|e| format!("Failed to reset: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Revert failed: {stderr}"));
        }

        // Remove all checkpoints after the reverted one
        self.checkpoints.lock().unwrap().truncate(index + 1);

        Ok(cp)
    }

    /// Revert the most recent checkpoint (convenience method).
    pub fn undo_last(&self) -> Result<Checkpoint, String> {
        let count = self.checkpoints.lock().unwrap().len();
        if count == 0 {
            return Err("No checkpoints to undo".to_string());
        }
        self.revert_to(count - 1)
    }

    /// Pop (discard) the most recent checkpoint without reverting.
    /// Useful when a tool execution fails and the checkpoint is no longer needed.
    pub fn discard_last(&self) -> Option<Checkpoint> {
        self.checkpoints.lock().unwrap().pop()
    }

    /// Clear all checkpoints.
    pub fn clear(&self) {
        self.checkpoints.lock().unwrap().clear();
    }

    /// Number of stored checkpoints.
    pub fn len(&self) -> usize {
        self.checkpoints.lock().unwrap().len()
    }

    /// Whether there are any checkpoints.
    pub fn is_empty(&self) -> bool {
        self.checkpoints.lock().unwrap().is_empty()
    }

    fn get_head_hash() -> Result<String, String> {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .map_err(|e| format!("Failed to get HEAD: {e}"))?;
        if !output.status.success() {
            return Err("Failed to get current commit hash".to_string());
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl Default for CheckpointManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_manager_new() {
        let mgr = CheckpointManager::new();
        // In a git repo, it should be enabled
        assert!(mgr.is_enabled());
        assert!(mgr.is_empty());
    }

    #[test]
    fn test_checkpoint_manager_list_empty() {
        let mgr = CheckpointManager::new();
        let list = mgr.list_checkpoints();
        assert!(list.is_empty());
    }

    #[test]
    fn test_checkpoint_manager_len() {
        let mgr = CheckpointManager::new();
        assert_eq!(mgr.len(), 0);
        assert!(mgr.is_empty());
    }

    #[test]
    fn test_checkpoint_manager_undo_empty() {
        let mgr = CheckpointManager::new();
        let result = mgr.undo_last();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No checkpoints"));
    }

    #[test]
    fn test_checkpoint_manager_revert_invalid_index() {
        let mgr = CheckpointManager::new();
        let result = mgr.revert_to(0);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid checkpoint index"));
    }

    #[test]
    fn test_checkpoint_manager_clear() {
        let mgr = CheckpointManager::new();
        mgr.clear();
        assert!(mgr.is_empty());
    }

    #[test]
    fn test_checkpoint_manager_discard_last_empty() {
        let mgr = CheckpointManager::new();
        assert!(mgr.discard_last().is_none());
    }
}
