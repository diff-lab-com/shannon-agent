//! Checkpoint system for undo/revert operations (Claude Code compatible).
//!
//! Creates lightweight git commits before file-modifying tool executions
//! and tracks per-turn file changes. Supports persistent checkpoint storage
//! and four restore modes:
//! - Restore code and conversation
//! - Restore conversation only
//! - Restore code only
//! - Summarize from here (compact messages from that point)

use shannon_types::recover_lock;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

/// Log a non-critical error instead of silently swallowing it.
macro_rules! log_err {
    ($result:expr, $msg:expr) => {
        if let Err(e) = $result {
            tracing::warn!("{}: {e}", $msg);
        }
    };
}

/// Maximum number of checkpoints to retain per session.
const MAX_CHECKPOINTS: usize = 50;

/// Maximum age in days before auto-cleanup removes checkpoint files.
const CHECKPOINT_MAX_AGE_DAYS: i64 = 30;

/// A single checkpoint representing a point-in-time snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

/// A per-turn checkpoint that ties git state to conversation context.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TurnCheckpoint {
    /// Index of the conversation turn (0-based).
    pub turn_index: usize,
    /// Git checkpoint at the start of this turn.
    pub checkpoint: Checkpoint,
    /// Files modified during this turn (relative paths).
    pub files_changed: Vec<String>,
    /// Preview of the user's prompt for this turn (first 80 chars).
    pub prompt_preview: Option<String>,
}

/// Restore mode for rewind operations (Claude Code compatible).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreMode {
    /// Revert both code changes and conversation history.
    CodeAndConversation,
    /// Only rewind conversation history, keep current code.
    ConversationOnly,
    /// Only revert file changes, keep conversation.
    CodeOnly,
}

/// Manages git-based checkpoints with per-turn tracking and persistence.
#[derive(Debug, Clone)]
pub struct CheckpointManager {
    checkpoints: Arc<Mutex<Vec<TurnCheckpoint>>>,
    enabled: bool,
    session_id: String,
    storage_dir: PathBuf,
}

impl CheckpointManager {
    /// Create a new checkpoint manager.
    pub fn new() -> Self {
        let storage_dir = dirs::home_dir()
            .map(|h| h.join(".shannon").join("checkpoints"))
            .unwrap_or_else(|| PathBuf::from(".shannon/checkpoints"));
        Self {
            checkpoints: Arc::new(Mutex::new(Vec::new())),
            enabled: Self::is_git_repo(),
            session_id: String::new(),
            storage_dir,
        }
    }

    /// Create a checkpoint manager for a specific session.
    pub fn for_session(session_id: &str) -> Self {
        let storage_dir = dirs::home_dir()
            .map(|h| h.join(".shannon").join("checkpoints"))
            .unwrap_or_else(|| PathBuf::from(".shannon/checkpoints"));
        let mgr = Self {
            checkpoints: Arc::new(Mutex::new(Vec::new())),
            enabled: Self::is_git_repo(),
            session_id: session_id.to_string(),
            storage_dir,
        };
        // Try to load persisted checkpoints for this session
        log_err!(mgr.load_from_disk(), "failed to load checkpoints from disk");
        mgr
    }

    /// Set the session ID (for persistence).
    pub fn set_session_id(&mut self, session_id: &str) {
        self.session_id = session_id.to_string();
        log_err!(
            self.load_from_disk(),
            "failed to load checkpoints from disk"
        );
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
    pub fn create_checkpoint(
        &self,
        tool_name: &str,
        description: &str,
    ) -> Result<Checkpoint, String> {
        if !self.enabled {
            return Err("Not in a git repository — checkpoints unavailable".to_string());
        }

        let has_changes = {
            let output = Command::new("git")
                .args(["status", "--porcelain"])
                .output()
                .map_err(|e| format!("Failed to check git status: {e}"))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            !stdout.trim().is_empty()
        };

        if !has_changes {
            let hash = Self::get_head_hash()?;
            let cp = Checkpoint {
                short_hash: hash[..7.min(hash.len())].to_string(),
                hash: hash.clone(),
                description: format!("pre-{tool_name}: {description} (no changes)"),
                timestamp: chrono::Utc::now().timestamp(),
            };
            return Ok(cp);
        }

        let stage_output = Command::new("git")
            .args(["add", "-A"])
            .output()
            .map_err(|e| format!("Failed to stage changes: {e}"))?;
        if !stage_output.status.success() {
            return Err("Failed to stage changes for checkpoint".to_string());
        }

        let commit_msg = format!("shannon: checkpoint before {tool_name}\n\n{description}");
        let commit_output = Command::new("git")
            .args(["commit", "-m", &commit_msg, "--no-gpg-sign"])
            .output()
            .map_err(|e| format!("Failed to create checkpoint commit: {e}"))?;

        if !commit_output.status.success() {
            let stderr = String::from_utf8_lossy(&commit_output.stderr);
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

        Ok(cp)
    }

    /// Record a per-turn checkpoint with file change tracking.
    pub fn record_turn(
        &self,
        turn_index: usize,
        checkpoint: Checkpoint,
        files_changed: Vec<String>,
        prompt_preview: Option<String>,
    ) {
        let tc = TurnCheckpoint {
            turn_index,
            checkpoint,
            files_changed,
            prompt_preview,
        };

        {
            let mut checkpoints = recover_lock(self.checkpoints.lock());
            checkpoints.push(tc);

            // Trim old checkpoints
            if checkpoints.len() > MAX_CHECKPOINTS {
                let drain_count = checkpoints.len() - MAX_CHECKPOINTS;
                checkpoints.drain(..drain_count);
            }
        }

        log_err!(self.save_to_disk(), "failed to save checkpoints after push");
    }
    pub fn list_checkpoints(&self) -> Vec<TurnCheckpoint> {
        recover_lock(self.checkpoints.lock()).clone()
    }

    /// List legacy checkpoints (git-only, without turn info).
    pub fn list_legacy_checkpoints(&self) -> Vec<Checkpoint> {
        recover_lock(self.checkpoints.lock())
            .iter()
            .map(|tc| tc.checkpoint.clone())
            .collect()
    }

    /// Revert to a specific turn checkpoint by index.
    pub fn revert_to(&self, index: usize, mode: RestoreMode) -> Result<TurnCheckpoint, String> {
        if !self.enabled && mode != RestoreMode::ConversationOnly {
            return Err("Not in a git repository".to_string());
        }

        let tc = {
            let checkpoints = recover_lock(self.checkpoints.lock());
            if index >= checkpoints.len() {
                return Err(format!(
                    "Invalid checkpoint index {index}. Available: 0..{}",
                    checkpoints.len().saturating_sub(1)
                ));
            }
            checkpoints[index].clone()
        };

        // Revert code if needed
        if mode == RestoreMode::CodeAndConversation || mode == RestoreMode::CodeOnly {
            let output = Command::new("git")
                .args(["reset", "--hard", &tc.checkpoint.hash])
                .output()
                .map_err(|e| format!("Failed to reset: {e}"))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Revert failed: {stderr}"));
            }
        }

        // Remove checkpoints after the reverted one
        recover_lock(self.checkpoints.lock()).truncate(index + 1);
        log_err!(
            self.save_to_disk(),
            "failed to save checkpoints after revert"
        );

        Ok(tc)
    }

    /// Revert the most recent checkpoint (convenience method).
    pub fn undo_last(&self) -> Result<Checkpoint, String> {
        let count = recover_lock(self.checkpoints.lock()).len();
        if count == 0 {
            return Err("No checkpoints to undo".to_string());
        }
        let tc = self.revert_to(count - 1, RestoreMode::CodeAndConversation)?;
        Ok(tc.checkpoint)
    }

    /// Pop (discard) the most recent checkpoint without reverting.
    pub fn discard_last(&self) -> Option<TurnCheckpoint> {
        let popped = recover_lock(self.checkpoints.lock()).pop();
        log_err!(
            self.save_to_disk(),
            "failed to save checkpoints after discard"
        );
        popped
    }

    /// Clear all checkpoints.
    pub fn clear(&self) {
        recover_lock(self.checkpoints.lock()).clear();
        log_err!(
            self.save_to_disk(),
            "failed to save checkpoints after clear"
        );
    }

    /// Number of stored checkpoints.
    pub fn len(&self) -> usize {
        recover_lock(self.checkpoints.lock()).len()
    }

    /// Whether there are any checkpoints.
    pub fn is_empty(&self) -> bool {
        recover_lock(self.checkpoints.lock()).is_empty()
    }

    // ---- Persistence ----

    /// Get the file path for this session's checkpoints.
    fn session_checkpoint_path(&self) -> Option<PathBuf> {
        if self.session_id.is_empty() {
            return None;
        }
        Some(self.storage_dir.join(format!("{}.json", self.session_id)))
    }

    /// Save checkpoints to disk.
    pub fn save_to_disk(&self) -> Result<(), String> {
        let path = match self.session_checkpoint_path() {
            Some(p) => p,
            None => return Ok(()),
        };

        if let Some(parent) = path.parent() {
            log_err!(
                fs::create_dir_all(parent),
                "failed to create checkpoint directory"
            );
        }

        let checkpoints = recover_lock(self.checkpoints.lock());
        let json = serde_json::to_string_pretty(&*checkpoints)
            .map_err(|e| format!("Failed to serialize checkpoints: {e}"))?;

        fs::write(&path, json).map_err(|e| format!("Failed to write checkpoints: {e}"))?;

        Ok(())
    }

    /// Load checkpoints from disk.
    pub fn load_from_disk(&self) -> Result<(), String> {
        let path = match self.session_checkpoint_path() {
            Some(p) => p,
            None => return Ok(()),
        };

        if !path.exists() {
            return Ok(());
        }

        let data =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read checkpoints: {e}"))?;

        let loaded: Vec<TurnCheckpoint> =
            serde_json::from_str(&data).map_err(|e| format!("Failed to parse checkpoints: {e}"))?;

        let mut checkpoints = recover_lock(self.checkpoints.lock());
        *checkpoints = loaded;

        Ok(())
    }

    /// Clean up checkpoint files older than CHECKPOINT_MAX_AGE_DAYS.
    pub fn cleanup_old_checkpoints() -> Result<usize, String> {
        let storage_dir = dirs::home_dir()
            .map(|h| h.join(".shannon").join("checkpoints"))
            .unwrap_or_else(|| PathBuf::from(".shannon/checkpoints"));

        if !storage_dir.exists() {
            return Ok(0);
        }

        let cutoff = chrono::Utc::now() - chrono::Duration::days(CHECKPOINT_MAX_AGE_DAYS);
        let cutoff_ts = cutoff.timestamp();
        let mut removed = 0;

        let entries = fs::read_dir(&storage_dir)
            .map_err(|e| format!("Failed to read checkpoint dir: {e}"))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            // Check file modification time as proxy for age
            if let Ok(metadata) = fs::metadata(&path) {
                if let Ok(modified) = metadata.modified() {
                    let mod_time: i64 = modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    if mod_time < cutoff_ts {
                        log_err!(
                            fs::remove_file(&path),
                            "failed to remove old checkpoint file"
                        );
                        removed += 1;
                    }
                }
            }
        }

        Ok(removed)
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
        let result = mgr.revert_to(0, RestoreMode::CodeAndConversation);
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

    #[test]
    fn test_turn_checkpoint_serialization() {
        let tc = TurnCheckpoint {
            turn_index: 0,
            checkpoint: Checkpoint {
                hash: "abc123def456".to_string(),
                short_hash: "abc123d".to_string(),
                description: "test checkpoint".to_string(),
                timestamp: 1234567890,
            },
            files_changed: vec!["src/main.rs".to_string(), "lib.rs".to_string()],
            prompt_preview: Some("fix the bug".to_string()),
        };

        let json = serde_json::to_string(&tc).unwrap();
        let deserialized: TurnCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.turn_index, 0);
        assert_eq!(deserialized.files_changed.len(), 2);
        assert_eq!(deserialized.prompt_preview, Some("fix the bug".to_string()));
    }

    #[test]
    fn test_restore_modes() {
        // Just verify the enum variants exist and are distinct
        assert_ne!(
            RestoreMode::CodeAndConversation,
            RestoreMode::ConversationOnly
        );
        assert_ne!(RestoreMode::CodeOnly, RestoreMode::ConversationOnly);
    }

    #[test]
    fn test_default_trait() {
        let mgr = CheckpointManager::default();
        assert!(mgr.is_empty());
    }

    #[test]
    fn test_for_session_constructor() {
        let mgr = CheckpointManager::for_session("test-session-123");
        assert!(mgr.is_enabled());
        assert!(mgr.is_empty());
    }

    #[test]
    fn test_set_session_id() {
        let mut mgr = CheckpointManager::new();
        mgr.set_session_id("abc-def");
        // After setting session ID, save_to_disk will use it
        mgr.clear(); // Should not panic
    }

    #[test]
    fn test_record_turn_stores_checkpoint() {
        let mgr = CheckpointManager::new();
        let cp = Checkpoint {
            hash: "deadbeef1234567890".to_string(),
            short_hash: "deadbee".to_string(),
            description: "before edit".to_string(),
            timestamp: 1700000000,
        };
        mgr.record_turn(
            0,
            cp,
            vec!["src/main.rs".to_string()],
            Some("fix bug".to_string()),
        );
        assert_eq!(mgr.len(), 1);

        let list = mgr.list_checkpoints();
        assert_eq!(list[0].turn_index, 0);
        assert_eq!(list[0].files_changed, vec!["src/main.rs"]);
        assert_eq!(list[0].prompt_preview, Some("fix bug".to_string()));
    }

    #[test]
    fn test_record_turn_truncates_at_max() {
        let mgr = CheckpointManager::new();
        for i in 0..MAX_CHECKPOINTS + 5 {
            let cp = Checkpoint {
                hash: format!("hash{i:020}"),
                short_hash: format!("hash{i:07}"),
                description: format!("turn {i}"),
                timestamp: 1700000000 + i as i64,
            };
            mgr.record_turn(i, cp, vec![], None);
        }
        assert_eq!(mgr.len(), MAX_CHECKPOINTS);
    }

    #[test]
    fn test_discard_last_with_data() {
        let mgr = CheckpointManager::new();
        let cp = Checkpoint {
            hash: "aaa111bbb222".to_string(),
            short_hash: "aaa111b".to_string(),
            description: "test".to_string(),
            timestamp: 1700000000,
        };
        mgr.record_turn(0, cp, vec!["a.rs".to_string()], None);
        assert_eq!(mgr.len(), 1);

        let discarded = mgr.discard_last().unwrap();
        assert_eq!(discarded.turn_index, 0);
        assert!(mgr.is_empty());
    }

    #[test]
    fn test_list_legacy_checkpoints() {
        let mgr = CheckpointManager::new();
        let cp = Checkpoint {
            hash: "cccccc1234567890".to_string(),
            short_hash: "cccccc1".to_string(),
            description: "legacy".to_string(),
            timestamp: 1700000000,
        };
        mgr.record_turn(0, cp, vec![], None);
        let legacy = mgr.list_legacy_checkpoints();
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].hash, "cccccc1234567890");
    }

    #[test]
    fn test_checkpoint_serialization_roundtrip() {
        let cp = Checkpoint {
            hash: "a1b2c3d4e5f6".to_string(),
            short_hash: "a1b2c3d".to_string(),
            description: "before write".to_string(),
            timestamp: 1700000000,
        };
        let json = serde_json::to_string(&cp).unwrap();
        let back: Checkpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(back.hash, cp.hash);
        assert_eq!(back.short_hash, cp.short_hash);
        assert_eq!(back.description, cp.description);
        assert_eq!(back.timestamp, cp.timestamp);
    }

    #[test]
    fn test_multiple_turns_ordering() {
        let mgr = CheckpointManager::new();
        for i in 0..3 {
            let cp = Checkpoint {
                hash: format!("h{i:016}"),
                short_hash: format!("h{i:07}"),
                description: format!("turn {i}"),
                timestamp: 1700000000 + i as i64,
            };
            mgr.record_turn(
                i,
                cp,
                vec![format!("file{i}.rs")],
                Some(format!("prompt {i}")),
            );
        }
        assert_eq!(mgr.len(), 3);
        let list = mgr.list_checkpoints();
        assert_eq!(list[0].turn_index, 0);
        assert_eq!(list[2].turn_index, 2);
    }

    #[test]
    fn test_revert_truncates_checkpoints() {
        let mgr = CheckpointManager::new();
        for i in 0..3 {
            let cp = Checkpoint {
                hash: format!("h{i:016}"),
                short_hash: format!("h{i:07}"),
                description: format!("turn {i}"),
                timestamp: 1700000000 + i as i64,
            };
            mgr.record_turn(i, cp, vec![], None);
        }
        assert_eq!(mgr.len(), 3);
        // revert_to with ConversationOnly doesn't need git
        let result = mgr.revert_to(0, RestoreMode::ConversationOnly);
        assert!(result.is_ok());
        assert_eq!(mgr.len(), 1); // Only index 0 remains
    }
}
