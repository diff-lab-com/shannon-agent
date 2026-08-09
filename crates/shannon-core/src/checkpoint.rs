//! Per-turn checkpoint tracking for `/rewind` conversation rewind and the
//! history list.
//!
//! NOTE: this module no longer creates git commits or runs `git reset`.
//! Code-level rewind (`/rewind code|both <n>`) is driven by content snapshots
//! in `FileHistoryManager` (shannon-tools), captured per turn in
//! `shannon-ui/src/repl/query.rs`. What remains here is lightweight per-turn
//! metadata (optionally persisted to disk) that powers `/rewind <n>`
//! conversation rewind and the checkpoint history list.

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
///
/// After the git-checkpoint removal, `hash`/`short_hash` carry synthetic
/// values (often empty) — they are retained for the persisted JSON schema and
/// the history-list UI, not for git operations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Checkpoint {
    /// Commit hash (synthetic / empty after the git-checkpoint removal).
    pub hash: String,
    /// Short hash (first 7 chars).
    pub short_hash: String,
    /// Description of what triggered this checkpoint.
    pub description: String,
    /// Timestamp (seconds since epoch).
    pub timestamp: i64,
}

/// A per-turn checkpoint that ties recorded state to conversation context.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TurnCheckpoint {
    /// Index of the conversation turn (0-based).
    pub turn_index: usize,
    /// Checkpoint recorded at the start of this turn.
    pub checkpoint: Checkpoint,
    /// Files modified during this turn (relative paths).
    pub files_changed: Vec<String>,
    /// Preview of the user's prompt for this turn (first 80 chars).
    pub prompt_preview: Option<String>,
}

/// Manages per-turn checkpoint metadata with optional disk persistence.
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

    /// Whether the manager was constructed inside a git repo.
    pub fn is_enabled(&self) -> bool {
        self.enabled
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
}

impl Default for CheckpointManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
    fn test_cleanup_old_checkpoints_no_panic() {
        // Should not panic even if the checkpoint directory does not exist.
        let result = CheckpointManager::cleanup_old_checkpoints();
        assert!(result.is_ok(), "cleanup should not error");
    }
}
