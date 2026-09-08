//! # File History Tracking
//!
//! Tracks file modification history with snapshot-based versioning, diffs,
//! and rollback support. Inspired by Claude Code's fileHistory system.
//!
//! ## Architecture
//!
//! - [`FileHistoryManager`]: Central manager for recording and retrieving file snapshots
//! - [`FileSnapshot`]: A point-in-time capture of file content
//! - [`FileHistory`]: Complete history for a single file
//! - [`FileDiff`]: Diff between two snapshots with line-level granularity
//!
//! ## Example
//!
//! ```no_run
//! use shannon_tools::file::history::{FileHistoryManager, FileHistoryConfig, FileOperation};
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let config = FileHistoryConfig::default();
//! let mut manager = FileHistoryManager::new(config);
//!
//! // Record a snapshot
//! let snapshot = manager.record_snapshot(
//!     Path::new("src/main.rs"),
//!     "fn main() { println!(\"Hello\"); }",
//!     FileOperation::Edit,
//! )?;
//!
//! // Get history for the file
//! let history = manager.get_history(Path::new("src/main.rs"))?;
//! println!("Snapshots: {}", history.snapshots.len());
//! # Ok(())
//! # }
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during file history operations.
#[derive(Error, Debug)]
pub enum FileHistoryError {
    #[error("Snapshot not found: {file_path} / {snapshot_id}")]
    SnapshotNotFound {
        file_path: String,
        snapshot_id: String,
    },

    #[error("No history for file: {0}")]
    NoHistory(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Diff error: {0}")]
    Diff(String),

    #[error("Rollback error: {0}")]
    Rollback(String),

    #[error("Storage quota exceeded: {used_mb:.1} MB used of {max_mb} MB limit")]
    StorageQuota { used_mb: f64, max_mb: usize },

    #[error("Invalid file path: {0}")]
    InvalidPath(String),
}

// ---------------------------------------------------------------------------
// FileOperation
// ---------------------------------------------------------------------------

/// The type of file operation that created a snapshot.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FileOperation {
    /// File was created.
    Create,
    /// File was edited/modified.
    Edit,
    /// File was deleted.
    Delete,
    /// File was read (no modification).
    Read,
}

impl FileOperation {
    /// Returns a short label for the operation.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Create => "CREATE",
            Self::Edit => "EDIT",
            Self::Delete => "DELETE",
            Self::Read => "READ",
        }
    }
}

impl std::fmt::Display for FileOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ---------------------------------------------------------------------------
// FileSnapshot
// ---------------------------------------------------------------------------

/// A point-in-time capture of file content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    /// Unique identifier for this snapshot.
    pub id: String,
    /// Absolute path to the file.
    pub file_path: PathBuf,
    /// When the snapshot was taken.
    pub timestamp: DateTime<Utc>,
    /// The full content of the file at this point in time.
    pub content: String,
    /// The type of operation that triggered this snapshot.
    pub operation: FileOperation,
    /// Number of lines in the content.
    pub line_count: usize,
    /// SHA-256 hash of the content for deduplication.
    pub hash: String,
    /// Conversation turn this snapshot was captured at (W6-2 B.2 turn-based rewind).
    /// `None` for ordinary per-edit snapshots not tied to a turn boundary.
    #[serde(default)]
    pub turn_index: Option<usize>,
}

impl FileSnapshot {
    /// Create a new snapshot.
    pub fn new(file_path: PathBuf, content: String, operation: FileOperation) -> Self {
        let line_count = content.lines().count();
        let hash = compute_content_hash(&content);
        Self {
            id: Uuid::new_v4().to_string(),
            file_path,
            timestamp: Utc::now(),
            content,
            operation,
            line_count,
            hash,
            turn_index: None,
        }
    }

    /// Create a snapshot with a specific ID (for testing / import).
    pub fn with_id(
        id: impl Into<String>,
        file_path: PathBuf,
        content: String,
        operation: FileOperation,
    ) -> Self {
        let line_count = content.lines().count();
        let hash = compute_content_hash(&content);
        Self {
            id: id.into(),
            file_path,
            timestamp: Utc::now(),
            content,
            operation,
            line_count,
            hash,
            turn_index: None,
        }
    }

    /// Returns true if the content matches another snapshot.
    pub fn content_matches(&self, other: &FileSnapshot) -> bool {
        self.hash == other.hash
    }
}

// ---------------------------------------------------------------------------
// FileHistory
// ---------------------------------------------------------------------------

/// Complete history of snapshots for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHistory {
    /// Path to the file this history tracks.
    pub file_path: PathBuf,
    /// Ordered list of snapshots (oldest first).
    pub snapshots: Vec<FileSnapshot>,
    /// Maximum number of snapshots to retain.
    pub max_snapshots: usize,
}

impl FileHistory {
    /// Create a new empty history for a file.
    pub fn new(file_path: PathBuf, max_snapshots: usize) -> Self {
        Self {
            file_path,
            snapshots: Vec::new(),
            max_snapshots,
        }
    }

    /// Add a snapshot, enforcing the max_snapshots limit.
    /// Returns the snapshot that was added (or None if it was a duplicate).
    pub fn add_snapshot(&mut self, snapshot: FileSnapshot) -> Option<FileSnapshot> {
        // Deduplicate: skip if the latest snapshot has the same hash
        if let Some(last) = self.snapshots.last() {
            if last.content_matches(&snapshot) {
                return None;
            }
        }

        self.snapshots.push(snapshot);

        // Evict oldest snapshots if we exceed the limit
        while self.snapshots.len() > self.max_snapshots {
            self.snapshots.remove(0);
        }

        self.snapshots.last().cloned()
    }

    /// Get the most recent snapshot.
    pub fn latest(&self) -> Option<&FileSnapshot> {
        self.snapshots.last()
    }

    /// Get a snapshot by ID.
    pub fn get_by_id(&self, id: &str) -> Option<&FileSnapshot> {
        self.snapshots.iter().find(|s| s.id == id)
    }

    /// Get the number of snapshots.
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    /// Returns true if there are no snapshots.
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }
}

// ---------------------------------------------------------------------------
// DiffHunk
// ---------------------------------------------------------------------------

/// A contiguous block of changes in a diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    /// Starting line in the "before" content.
    pub old_start: usize,
    /// Number of lines in the "before" content.
    pub old_count: usize,
    /// Starting line in the "after" content.
    pub new_start: usize,
    /// Number of lines in the "after" content.
    pub new_count: usize,
    /// The content of the hunk (prefixed with +/- for changes).
    pub content: String,
}

// ---------------------------------------------------------------------------
// FileDiff
// ---------------------------------------------------------------------------

/// A diff between two file snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    /// The "before" snapshot (None for new files).
    pub snapshot_before: Option<FileSnapshot>,
    /// The "after" snapshot.
    pub snapshot_after: FileSnapshot,
    /// Number of lines added.
    pub additions: usize,
    /// Number of lines deleted.
    pub deletions: usize,
    /// The individual hunks of the diff.
    pub hunks: Vec<DiffHunk>,
}

impl FileDiff {
    /// Returns the net change in line count.
    pub fn net_change(&self) -> isize {
        self.additions as isize - self.deletions as isize
    }

    /// Returns a unified diff string.
    pub fn to_unified(&self) -> String {
        let mut output = String::new();

        let old_path = self
            .snapshot_before
            .as_ref()
            .map(|s| s.file_path.display().to_string())
            .unwrap_or_else(|| "/dev/null".to_string());
        let new_path = self.snapshot_after.file_path.display().to_string();

        output.push_str(&format!("--- {old_path}\n"));
        output.push_str(&format!("+++ {new_path}\n"));

        for hunk in &self.hunks {
            output.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
            ));
            output.push_str(&hunk.content);
            if !hunk.content.ends_with('\n') {
                output.push('\n');
            }
        }

        output
    }
}

// ---------------------------------------------------------------------------
// FileHistoryConfig
// ---------------------------------------------------------------------------

/// Configuration for the FileHistoryManager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHistoryConfig {
    /// Directory where history data is stored.
    pub history_dir: PathBuf,
    /// Maximum number of snapshots per file.
    pub max_history_per_file: usize,
    /// Maximum total history storage in MB.
    pub max_total_history_mb: usize,
    /// Time-to-live for snapshots, in whole seconds. Snapshots older than
    /// this become cleanup-eligible. `None` disables time-based expiry (only
    /// the per-file count limit and storage quota apply). Default: 7 days
    /// (604_800). Stored as seconds because `std::time::Duration` does not
    /// implement `Serialize`/`Deserialize`.
    pub ttl: Option<u64>,
}

impl Default for FileHistoryConfig {
    fn default() -> Self {
        let history_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".shannon")
            .join("file_history");

        Self {
            history_dir,
            max_history_per_file: 50,
            max_total_history_mb: 100,
            // 7 days in seconds (604_800). Snapshots older than this expire.
            ttl: Some(7 * 24 * 60 * 60),
        }
    }
}

impl FileHistoryConfig {
    /// Build config from `SHANNON_FILE_HISTORY*` env overrides (W6-2 A.4),
    /// falling back to [`FileHistoryConfig::default`]. Returns `None` only
    /// when history is explicitly disabled.
    ///
    /// Recognized env vars (consistent with the documented `SHANNON_*` config
    /// layer; see `web.rs` for precedent):
    /// - `SHANNON_FILE_HISTORY` — `0`/`false`/`off`/`no` disables history
    ///   entirely (tools register without a manager = pre-W6-2 behavior).
    ///   Unset or any other value keeps it enabled (default-on).
    /// - `SHANNON_FILE_HISTORY_DIR` — overrides `history_dir`.
    /// - `SHANNON_FILE_HISTORY_TTL` — overrides `ttl`, in whole seconds.
    ///   `0` disables time-based expiry (`ttl = None`); count + quota still apply.
    ///
    /// Unset or unparseable values keep the default.
    pub fn from_env() -> Option<Self> {
        Self::from_env_vars(
            std::env::var("SHANNON_FILE_HISTORY").ok(),
            std::env::var("SHANNON_FILE_HISTORY_DIR").ok(),
            std::env::var("SHANNON_FILE_HISTORY_TTL").ok(),
        )
    }

    /// Pure core of [`from_env`](Self::from_env): accepts the raw env values so
    /// it can be unit-tested without mutating process-global environment state.
    pub(crate) fn from_env_vars(
        enabled: Option<String>,
        dir: Option<String>,
        ttl: Option<String>,
    ) -> Option<Self> {
        if matches!(enabled.as_deref(), Some("0" | "false" | "off" | "no")) {
            return None;
        }
        let mut cfg = Self::default();
        if let Some(dir) = dir {
            let dir = dir.trim();
            if !dir.is_empty() {
                cfg.history_dir = PathBuf::from(dir);
            }
        }
        if let Some(ttl) = ttl {
            if let Ok(secs) = ttl.trim().parse::<u64>() {
                // 0 → no time-based expiry (None); positive N → Some(N) seconds.
                cfg.ttl = (secs > 0).then_some(secs);
            }
        }
        Some(cfg)
    }
}

// ---------------------------------------------------------------------------
// FileHistoryManager
// ---------------------------------------------------------------------------

/// Manages file history with snapshot recording, diff computation, and rollback.
///
/// Snapshots are stored as JSON files in a directory structure:
///
/// ```text
/// history_dir/
///   _index.json           -- global index of tracked files
///   <file_hash>/
///     <snapshot_id>.json  -- individual snapshots
/// ```
pub struct FileHistoryManager {
    history_dir: PathBuf,
    max_history_per_file: usize,
    max_total_history_mb: usize,
    /// Time-to-live; snapshots older than this are cleanup-eligible.
    /// `None` disables time-based expiry.
    ttl: Option<Duration>,
    /// In-memory cache of file histories.
    cache: HashMap<PathBuf, FileHistory>,
    /// Whether the cache has been loaded from disk.
    cache_loaded: bool,
    /// Filesystem world backing snapshot persistence (§4.11).
    fs: std::sync::Arc<dyn shannon_tool_interface::FileSystemProvider>,
}

/// Action that [`FileHistoryManager::rewind_file_to_turn`] prescribes for a file
/// when rewinding code to a given turn (W6-2 B.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewindAction {
    /// Restore the file to this content.
    Restore(String),
    /// The file's earliest turn-tagged snapshot is newer than the target turn, so it
    /// did not exist at the target — delete it.
    Delete,
    /// No turn-tagged history for this file — leave it untouched.
    NoChange,
}

impl FileHistoryManager {
    /// Create a new FileHistoryManager with the given configuration.
    pub fn new(config: FileHistoryConfig) -> Self {
        let manager = Self {
            history_dir: config.history_dir,
            max_history_per_file: config.max_history_per_file,
            max_total_history_mb: config.max_total_history_mb,
            ttl: config.ttl.map(Duration::from_secs),
            cache: HashMap::new(),
            cache_loaded: false,
            fs: crate::defaults::fs(),
        };

        // Ensure the history directory exists
        let _ = manager.fs.create_dir_all_blocking(&manager.history_dir);

        manager
    }

    /// Inject a filesystem world override for snapshot storage (§4.11).
    pub fn with_fs(
        mut self,
        fs: std::sync::Arc<dyn shannon_tool_interface::FileSystemProvider>,
    ) -> Self {
        self.fs = fs;
        self
    }

    /// Create a new manager with a temporary directory (for testing).
    pub fn new_temp() -> Result<Self, FileHistoryError> {
        let temp_path =
            std::env::temp_dir().join(format!("shannon_history_test_{}", Uuid::new_v4()));
        let config = FileHistoryConfig {
            history_dir: temp_path,
            max_history_per_file: 10,
            max_total_history_mb: 10,
            ttl: Some(7 * 24 * 60 * 60),
        };
        Ok(Self::new(config))
    }

    /// Ensure the history directory exists.
    fn ensure_dir(&self) -> Result<(), FileHistoryError> {
        self.fs.create_dir_all_blocking(&self.history_dir)?;
        Ok(())
    }

    /// Get the storage subdirectory for a file.
    fn file_dir(&self, file_path: &Path) -> PathBuf {
        let hash = compute_content_hash(&file_path.to_string_lossy());
        self.history_dir.join(hash)
    }

    /// Compute the SHA-256 hash of content.
    pub fn compute_content_hash(content: &str) -> String {
        compute_content_hash(content)
    }

    /// Load the cache from disk if not already loaded.
    fn ensure_cache_loaded(&mut self) -> Result<(), FileHistoryError> {
        if self.cache_loaded {
            return Ok(());
        }

        self.ensure_dir()?;

        let index_path = self.history_dir.join("_index.json");
        if index_path.exists() {
            let content = self.fs.read_text_blocking(&index_path)?;
            let index: HashMap<String, Vec<String>> = serde_json::from_str(&content)?;

            for (file_path_str, snapshot_ids) in index {
                let file_path = PathBuf::from(&file_path_str);
                let mut snapshots = Vec::new();

                for id in &snapshot_ids {
                    let snapshot_path = self.file_dir(&file_path).join(format!("{id}.json"));
                    if snapshot_path.exists() {
                        if let Ok(content) = self.fs.read_text_blocking(&snapshot_path) {
                            if let Ok(snapshot) = serde_json::from_str::<FileSnapshot>(&content) {
                                snapshots.push(snapshot);
                            }
                        }
                    }
                }

                let mut history = FileHistory::new(file_path.clone(), self.max_history_per_file);
                history.snapshots = snapshots;
                self.cache.insert(file_path, history);
            }
        }

        self.cache_loaded = true;
        Ok(())
    }

    /// Save the index to disk.
    fn save_index(&self) -> Result<(), FileHistoryError> {
        self.ensure_dir()?;

        let mut index: HashMap<String, Vec<String>> = HashMap::new();
        for (file_path, history) in &self.cache {
            let ids: Vec<String> = history.snapshots.iter().map(|s| s.id.clone()).collect();
            index.insert(file_path.to_string_lossy().to_string(), ids);
        }

        let index_path = self.history_dir.join("_index.json");
        let content = serde_json::to_string_pretty(&index)?;
        self.fs
            .write_bytes_blocking(&index_path, content.as_bytes())?;

        Ok(())
    }

    /// Save a single snapshot to disk.
    fn save_snapshot(&self, snapshot: &FileSnapshot) -> Result<(), FileHistoryError> {
        let dir = self.file_dir(&snapshot.file_path);
        self.fs.create_dir_all_blocking(&dir)?;

        let snapshot_path = dir.join(format!("{}.json", snapshot.id));
        let content = serde_json::to_string_pretty(snapshot)?;
        self.fs
            .write_bytes_blocking(&snapshot_path, content.as_bytes())?;

        Ok(())
    }

    // ---- Public API -------------------------------------------------------

    /// Record a snapshot of file content.
    ///
    /// Returns the snapshot if it was recorded (None if deduplicated).
    pub fn record_snapshot(
        &mut self,
        file_path: &Path,
        content: &str,
        operation: FileOperation,
    ) -> Result<FileSnapshot, FileHistoryError> {
        self.ensure_cache_loaded()?;

        let file_path = file_path.to_path_buf();

        // Check storage quota
        self.check_storage_quota()?;

        let snapshot = FileSnapshot::new(file_path.clone(), content.to_string(), operation);

        let history = self
            .cache
            .entry(file_path.clone())
            .or_insert_with(|| FileHistory::new(file_path.clone(), self.max_history_per_file));

        if let Some(recorded) = history.add_snapshot(snapshot.clone()) {
            self.save_snapshot(&recorded)?;
            self.save_index()?;
            Ok(recorded)
        } else {
            Err(FileHistoryError::Diff(
                "Snapshot deduplicated: content matches the latest snapshot".to_string(),
            ))
        }
    }

    /// Get the complete history for a file.
    pub fn get_history(&mut self, file_path: &Path) -> Result<FileHistory, FileHistoryError> {
        self.ensure_cache_loaded()?;

        self.cache
            .get(file_path)
            .cloned()
            .ok_or_else(|| FileHistoryError::NoHistory(file_path.to_string_lossy().to_string()))
    }

    /// Get a specific snapshot by ID.
    pub fn get_snapshot(
        &mut self,
        file_path: &Path,
        id: &str,
    ) -> Result<FileSnapshot, FileHistoryError> {
        self.ensure_cache_loaded()?;

        let history = self
            .cache
            .get(file_path)
            .ok_or_else(|| FileHistoryError::NoHistory(file_path.to_string_lossy().to_string()))?;

        history
            .get_by_id(id)
            .cloned()
            .ok_or_else(|| FileHistoryError::SnapshotNotFound {
                file_path: file_path.to_string_lossy().to_string(),
                snapshot_id: id.to_string(),
            })
    }

    /// Compute a diff between two snapshots.
    pub fn diff(
        &mut self,
        file_path: &Path,
        id_before: &str,
        id_after: &str,
    ) -> Result<FileDiff, FileHistoryError> {
        self.ensure_cache_loaded()?;

        let snapshot_after = self.get_snapshot(file_path, id_after)?;

        let snapshot_before = if id_before.is_empty() {
            None
        } else {
            Some(self.get_snapshot(file_path, id_before)?)
        };

        compute_diff(snapshot_before, snapshot_after)
    }

    /// Roll back a file to a specific snapshot.
    ///
    /// Returns the content at that snapshot point.
    pub fn rollback(&mut self, file_path: &Path, id: &str) -> Result<String, FileHistoryError> {
        self.ensure_cache_loaded()?;

        let snapshot = self.get_snapshot(file_path, id)?;

        // Record the rollback as an edit operation
        let _ = self.record_snapshot(file_path, &snapshot.content, FileOperation::Edit);

        Ok(snapshot.content.clone())
    }

    /// Roll back a file and persist the restored content **through the
    /// manager's filesystem world**. REPL-side `/rewind` must use this (with
    /// the provider-wired manager from `ToolRegistrationResult`) or a remote
    /// session would restore the file onto the wrong machine.
    pub fn restore(&mut self, file_path: &Path, id: &str) -> Result<String, FileHistoryError> {
        let content = self.rollback(file_path, id)?;
        self.fs
            .write_bytes_blocking(file_path, content.as_bytes())
            .map_err(FileHistoryError::Io)?;
        Ok(content)
    }

    /// Record a turn-boundary snapshot capturing the file's content at the end of a
    /// conversation turn (W6-2 B.2). Tagged with `turn_index` so [`rewind_file_to_turn`]
    /// can locate it. Content-deduplicated like ordinary snapshots.
    ///
    /// [`rewind_file_to_turn`]: FileHistoryManager::rewind_file_to_turn
    pub fn record_turn_snapshot(
        &mut self,
        file_path: &Path,
        content: &str,
        turn_index: usize,
    ) -> Result<FileSnapshot, FileHistoryError> {
        self.ensure_cache_loaded()?;
        self.check_storage_quota()?;

        let mut snapshot = FileSnapshot::new(
            file_path.to_path_buf(),
            content.to_string(),
            FileOperation::Edit,
        );
        snapshot.turn_index = Some(turn_index);

        let history = self
            .cache
            .entry(file_path.to_path_buf())
            .or_insert_with(|| {
                FileHistory::new(file_path.to_path_buf(), self.max_history_per_file)
            });

        if let Some(recorded) = history.add_snapshot(snapshot.clone()) {
            self.save_snapshot(&recorded)?;
            self.save_index()?;
            Ok(recorded)
        } else {
            Err(FileHistoryError::Diff(
                "Snapshot deduplicated: content matches the latest snapshot".to_string(),
            ))
        }
    }

    /// Determine how to restore `file_path` to its state at the end of `turn_index`.
    ///
    /// Among the file's **turn-tagged** snapshots, picks the one with the largest
    /// `turn_index <= turn_index` and prescribes a [`RewindAction::Restore`] to its
    /// content. If every turn-tagged snapshot is newer than the target (the file was
    /// first created after that turn), prescribes [`RewindAction::Delete`]. If the
    /// file has no turn-tagged snapshots at all, prescribes [`RewindAction::NoChange`].
    ///
    /// This only *decides* the action; the caller writes to disk so it can confirm
    /// before overwriting uncommitted work.
    pub fn rewind_file_to_turn(
        &mut self,
        file_path: &Path,
        turn_index: usize,
    ) -> Result<RewindAction, FileHistoryError> {
        self.ensure_cache_loaded()?;

        let history = match self.cache.get(file_path) {
            Some(h) => h,
            None => return Ok(RewindAction::NoChange),
        };

        let target = history
            .snapshots
            .iter()
            .filter(|s| s.turn_index.is_some_and(|t| t <= turn_index))
            .max_by_key(|s| s.turn_index);

        match target {
            Some(s) => Ok(RewindAction::Restore(s.content.clone())),
            None => {
                if history.snapshots.iter().any(|s| s.turn_index.is_some()) {
                    Ok(RewindAction::Delete)
                } else {
                    Ok(RewindAction::NoChange)
                }
            }
        }
    }

    /// Determine how to restore `file_path` to its state BEFORE conversation
    /// `turn_index` started — the "rewind to your message N" flavor that
    /// drops turn N and everything after it. Identical to
    /// [`rewind_file_to_turn`](Self::rewind_file_to_turn) except the target
    /// filter is strict (`turn_index < turn`), so a turn-0 rewind prescribes
    /// [`RewindAction::Delete`] for files first created during the session
    /// instead of silently keeping their end-of-turn-0 state.
    ///
    /// Like [`rewind_file_to_turn`](Self::rewind_file_to_turn), this only
    /// decides; the caller writes to disk.
    pub fn rewind_before_turn(
        &mut self,
        file_path: &Path,
        turn_index: usize,
    ) -> Result<RewindAction, FileHistoryError> {
        self.ensure_cache_loaded()?;

        let history = match self.cache.get(file_path) {
            Some(h) => h,
            None => return Ok(RewindAction::NoChange),
        };

        let target = history
            .snapshots
            .iter()
            .filter(|s| s.turn_index.is_some_and(|t| t < turn_index))
            .max_by_key(|s| s.turn_index);

        match target {
            Some(s) => Ok(RewindAction::Restore(s.content.clone())),
            None => {
                if history.snapshots.iter().any(|s| s.turn_index.is_some()) {
                    Ok(RewindAction::Delete)
                } else {
                    Ok(RewindAction::NoChange)
                }
            }
        }
    }

    /// List all tracked files.
    pub fn list_tracked_files(&mut self) -> Result<Vec<PathBuf>, FileHistoryError> {
        self.ensure_cache_loaded()?;

        Ok(self.cache.keys().cloned().collect())
    }

    /// Clean up old snapshots that exceed the per-file limit.
    ///
    /// Returns the number of snapshots removed.
    pub fn cleanup_old_snapshots(&mut self) -> Result<usize, FileHistoryError> {
        self.ensure_cache_loaded()?;

        let mut removed = 0;
        let mut files_to_delete: Vec<(PathBuf, String)> = Vec::new();

        // First pass: determine which snapshots to remove
        for (file_path, history) in &mut self.cache {
            let excess = history
                .snapshots
                .len()
                .saturating_sub(history.max_snapshots);
            if excess > 0 {
                for _ in 0..excess {
                    if let Some(removed_snapshot) = history.snapshots.first() {
                        files_to_delete.push((file_path.clone(), removed_snapshot.id.clone()));
                        history.snapshots.remove(0);
                        removed += 1;
                    }
                }
            }
        }

        // Second pass: delete the snapshot files
        for (file_path, snapshot_id) in &files_to_delete {
            let snapshot_path = self.file_dir(file_path).join(format!("{snapshot_id}.json"));
            if let Err(e) = self.fs.remove_file_blocking(&snapshot_path) {
                tracing::debug!("Failed to remove old snapshot: {e}");
            }
        }

        if removed > 0 {
            self.save_index()?;
        }

        Ok(removed)
    }

    /// Remove all snapshots strictly older than `cutoff`.
    ///
    /// Returns the number of snapshots removed. This is the deterministic,
    /// testable core of time-based expiry; [`FileHistoryManager::cleanup_expired`]
    /// wraps it with the configured TTL. Note this is **not** called
    /// automatically from `record_snapshot` (to keep the write path fast); it
    /// is meant to be invoked periodically — e.g. on session start or when
    /// `/undo` runs.
    pub fn cleanup_expired_before(
        &mut self,
        cutoff: DateTime<Utc>,
    ) -> Result<usize, FileHistoryError> {
        self.ensure_cache_loaded()?;

        let mut removed = 0usize;
        let mut to_delete: Vec<(PathBuf, String)> = Vec::new();

        for (file_path, history) in &mut self.cache {
            let expired: Vec<String> = history
                .snapshots
                .iter()
                .filter(|s| s.timestamp < cutoff)
                .map(|s| s.id.clone())
                .collect();
            for id in &expired {
                to_delete.push((file_path.clone(), id.clone()));
            }
            history.snapshots.retain(|s| s.timestamp >= cutoff);
            removed += expired.len();
        }

        for (file_path, snapshot_id) in &to_delete {
            let snapshot_path = self.file_dir(file_path).join(format!("{snapshot_id}.json"));
            if let Err(e) = self.fs.remove_file_blocking(&snapshot_path) {
                tracing::debug!("Failed to remove expired snapshot: {e}");
            }
        }

        if removed > 0 {
            self.save_index()?;
        }

        Ok(removed)
    }

    /// Remove snapshots older than the configured TTL.
    ///
    /// Returns the number removed. Returns `Ok(0)` when TTL is `None`.
    pub fn cleanup_expired(&mut self) -> Result<usize, FileHistoryError> {
        self.ensure_cache_loaded()?;
        let Some(ttl) = self.ttl else {
            return Ok(0);
        };
        // chrono::Duration is calendar-based; convert from std::time::Duration.
        // On overflow (impossibly large TTL) fall back to a zero offset, which
        // makes nothing expire — a safe no-op.
        let offset = chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::zero());
        let cutoff = Utc::now()
            .checked_sub_signed(offset)
            .unwrap_or_else(Utc::now);
        self.cleanup_expired_before(cutoff)
    }

    /// Check total storage usage against the quota.
    fn check_storage_quota(&self) -> Result<(), FileHistoryError> {
        let total_bytes = dir_size(self.fs.as_ref(), &self.history_dir).unwrap_or(0);
        let used_mb = total_bytes as f64 / (1024.0 * 1024.0);

        if used_mb > self.max_total_history_mb as f64 {
            return Err(FileHistoryError::StorageQuota {
                used_mb,
                max_mb: self.max_total_history_mb,
            });
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Diff computation
// ---------------------------------------------------------------------------

/// Compute a diff between two snapshots (or a creation diff if before is None).
fn compute_diff(
    snapshot_before: Option<FileSnapshot>,
    snapshot_after: FileSnapshot,
) -> Result<FileDiff, FileHistoryError> {
    let before_lines: Vec<&str> = match &snapshot_before {
        Some(s) => s.content.lines().collect(),
        None => Vec::new(),
    };
    let after_lines: Vec<&str> = snapshot_after.content.lines().collect();

    let hunks = compute_hunks(&before_lines, &after_lines);

    let additions = hunks
        .iter()
        .filter(|h| h.new_count > 0)
        .map(|h| h.new_count)
        .sum();
    let deletions = hunks
        .iter()
        .filter(|h| h.old_count > 0)
        .map(|h| h.old_count)
        .sum();

    Ok(FileDiff {
        snapshot_before,
        snapshot_after,
        additions,
        deletions,
        hunks,
    })
}

/// Compute diff hunks using a simple line-based algorithm.
///
/// This uses a basic longest common subsequence (LCS) approach for small files
/// and falls back to a whole-file diff for larger files.
fn compute_hunks(before: &[&str], after: &[&str]) -> Vec<DiffHunk> {
    if before.is_empty() && after.is_empty() {
        return Vec::new();
    }

    // For small files, use LCS-based diff
    if before.len() + after.len() <= 1000 {
        return lcs_diff(before, after);
    }

    // For large files, use a simple whole-file approach
    simple_diff(before, after)
}

/// LCS-based diff computation for reasonably-sized files.
fn lcs_diff(before: &[&str], after: &[&str]) -> Vec<DiffHunk> {
    let m = before.len();
    let n = after.len();

    // Build LCS table
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if before[i - 1] == after[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    // Backtrack to produce diff
    let mut diff_ops: Vec<DiffOp> = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && before[i - 1] == after[j - 1] {
            diff_ops.push(DiffOp::Context(before[i - 1].to_string()));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            diff_ops.push(DiffOp::Add(after[j - 1].to_string()));
            j -= 1;
        } else if i > 0 {
            diff_ops.push(DiffOp::Remove(before[i - 1].to_string()));
            i -= 1;
        }
    }
    diff_ops.reverse();

    // Group into hunks
    group_into_hunks(diff_ops)
}

/// Simple diff for large files: groups consecutive additions and deletions.
fn simple_diff(before: &[&str], after: &[&str]) -> Vec<DiffHunk> {
    let mut diff_ops: Vec<DiffOp> = Vec::new();

    // Quick check: if they're identical, return empty
    if before == after {
        return Vec::new();
    }

    // Use a hash-based approach for finding common lines
    let before_hashes: Vec<u64> = before.iter().map(|l| hash_line(l)).collect();
    let after_hashes: Vec<u64> = after.iter().map(|l| hash_line(l)).collect();

    // Simple approach: find the longest common prefix and suffix
    let common_prefix = before_hashes
        .iter()
        .zip(after_hashes.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let common_suffix = before_hashes
        .iter()
        .rev()
        .zip(after_hashes.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    // Add context lines for prefix
    for line in before.iter().take(common_prefix) {
        diff_ops.push(DiffOp::Context((*line).to_string()));
    }

    // Add changed lines
    let before_change_end = before.len().saturating_sub(common_suffix);
    for line in before.iter().take(before_change_end).skip(common_prefix) {
        diff_ops.push(DiffOp::Remove((*line).to_string()));
    }
    let after_change_end = after.len().saturating_sub(common_suffix);
    for line in after.iter().take(after_change_end).skip(common_prefix) {
        diff_ops.push(DiffOp::Add((*line).to_string()));
    }

    // Add context lines for suffix
    for line in after.iter().skip(after.len().saturating_sub(common_suffix)) {
        diff_ops.push(DiffOp::Context((*line).to_string()));
    }

    group_into_hunks(diff_ops)
}

/// A single line-level diff operation.
#[derive(Debug, Clone)]
enum DiffOp {
    /// Line is unchanged (context).
    Context(String),
    /// Line was added.
    Add(String),
    /// Line was removed.
    Remove(String),
}

/// Group diff operations into hunks, with up to 3 lines of context between changes.
fn group_into_hunks(ops: Vec<DiffOp>) -> Vec<DiffHunk> {
    let max_context = 3;
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut current_hunk_ops: Vec<DiffOp> = Vec::new();
    let mut context_count = 0;
    let mut old_line = 1;
    let mut new_line = 1;

    let mut in_change = false;
    let mut old_start = 1;
    let mut new_start = 1;
    let mut old_count = 0;
    let mut new_count = 0;

    for op in &ops {
        match op {
            DiffOp::Context(line) => {
                old_line += 1;
                new_line += 1;

                if in_change {
                    context_count += 1;
                }

                current_hunk_ops.push(DiffOp::Context(line.clone()));

                // If we've collected enough context after a change, flush the hunk
                if in_change && context_count >= max_context {
                    let content = format_hunk_ops(&current_hunk_ops);
                    hunks.push(DiffHunk {
                        old_start,
                        old_count,
                        new_start,
                        new_count,
                        content,
                    });
                    current_hunk_ops = Vec::new();
                    in_change = false;
                    context_count = 0;
                }
            }
            DiffOp::Add(line) => {
                new_line += 1;
                if !in_change {
                    old_start = old_line;
                    new_start = new_line;
                    in_change = true;
                    context_count = 0;
                }
                new_count += 1;
                current_hunk_ops.push(DiffOp::Add(line.clone()));
            }
            DiffOp::Remove(line) => {
                old_line += 1;
                if !in_change {
                    old_start = old_line;
                    new_start = new_line;
                    in_change = true;
                    context_count = 0;
                }
                old_count += 1;
                current_hunk_ops.push(DiffOp::Remove(line.clone()));
            }
        }
    }

    // Flush remaining hunk
    if in_change && !current_hunk_ops.is_empty() {
        // Trim trailing context if we're at the end
        let content = format_hunk_ops(&current_hunk_ops);
        hunks.push(DiffHunk {
            old_start,
            old_count,
            new_start,
            new_count,
            content,
        });
    }

    hunks
}

/// Format diff operations into a hunk content string.
fn format_hunk_ops(ops: &[DiffOp]) -> String {
    let mut output = String::new();
    for op in ops {
        match op {
            DiffOp::Context(line) => {
                output.push(' ');
                output.push_str(line);
                output.push('\n');
            }
            DiffOp::Add(line) => {
                output.push('+');
                output.push_str(line);
                output.push('\n');
            }
            DiffOp::Remove(line) => {
                output.push('-');
                output.push_str(line);
                output.push('\n');
            }
        }
    }
    output
}

/// Hash a line for quick comparison.
fn hash_line(line: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    line.hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Compute the SHA-256 hash of content, returning a hex string.
pub fn compute_content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    hex_encode(&result)
}

/// Encode bytes as lowercase hex.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Recursively compute the total size of a directory in bytes.
///
/// Walks through the injected filesystem world rather than filesystem APIs
/// directly (§4.11).
fn dir_size(
    fs: &dyn shannon_tool_interface::FileSystemProvider,
    path: &Path,
) -> Result<u64, std::io::Error> {
    let meta = fs.metadata_blocking(path)?;
    if !meta.is_dir {
        return Ok(meta.len);
    }

    let mut total = 0u64;
    for entry in fs.list_dir_blocking(path)? {
        if entry.is_dir {
            total += dir_size(fs, &entry.path)?;
        } else {
            total += entry.len;
        }
    }
    Ok(total)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ---- FileOperation tests -----------------------------------------------

    #[test]
    fn test_file_operation_label() {
        assert_eq!(FileOperation::Create.label(), "CREATE");
        assert_eq!(FileOperation::Edit.label(), "EDIT");
        assert_eq!(FileOperation::Delete.label(), "DELETE");
        assert_eq!(FileOperation::Read.label(), "READ");
    }

    #[test]
    fn test_file_operation_display() {
        assert_eq!(format!("{}", FileOperation::Edit), "EDIT");
    }

    // ---- FileSnapshot tests ------------------------------------------------

    #[test]
    fn test_snapshot_new() {
        let snapshot = FileSnapshot::new(
            PathBuf::from("/tmp/test.rs"),
            "fn main() {}".to_string(),
            FileOperation::Edit,
        );
        assert!(!snapshot.id.is_empty());
        assert_eq!(snapshot.file_path, PathBuf::from("/tmp/test.rs"));
        assert_eq!(snapshot.operation, FileOperation::Edit);
        assert_eq!(snapshot.line_count, 1);
        assert!(!snapshot.hash.is_empty());
    }

    #[test]
    fn test_snapshot_with_id() {
        let snapshot = FileSnapshot::with_id(
            "custom-id",
            PathBuf::from("/tmp/test.rs"),
            "hello".to_string(),
            FileOperation::Create,
        );
        assert_eq!(snapshot.id, "custom-id");
    }

    #[test]
    fn test_snapshot_content_matches() {
        let s1 = FileSnapshot::new(
            PathBuf::from("/tmp/a.rs"),
            "same content".to_string(),
            FileOperation::Edit,
        );
        let s2 = FileSnapshot::new(
            PathBuf::from("/tmp/b.rs"),
            "same content".to_string(),
            FileOperation::Create,
        );
        assert!(s1.content_matches(&s2));

        let s3 = FileSnapshot::new(
            PathBuf::from("/tmp/c.rs"),
            "different content".to_string(),
            FileOperation::Edit,
        );
        assert!(!s1.content_matches(&s3));
    }

    #[test]
    fn test_snapshot_serialization() {
        let snapshot = FileSnapshot::new(
            PathBuf::from("/tmp/test.rs"),
            "content".to_string(),
            FileOperation::Edit,
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: FileSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, snapshot.id);
        assert_eq!(restored.hash, snapshot.hash);
        assert_eq!(restored.operation, snapshot.operation);
    }

    // ---- FileHistory tests -------------------------------------------------

    #[test]
    fn test_history_new() {
        let history = FileHistory::new(PathBuf::from("/tmp/test.rs"), 5);
        assert!(history.is_empty());
        assert_eq!(history.len(), 0);
        assert!(history.latest().is_none());
    }

    #[test]
    fn test_history_add_snapshot() {
        let mut history = FileHistory::new(PathBuf::from("/tmp/test.rs"), 5);
        let s1 = FileSnapshot::new(
            PathBuf::from("/tmp/test.rs"),
            "v1".to_string(),
            FileOperation::Edit,
        );
        let s2 = FileSnapshot::new(
            PathBuf::from("/tmp/test.rs"),
            "v2".to_string(),
            FileOperation::Edit,
        );

        assert!(history.add_snapshot(s1).is_some());
        assert_eq!(history.len(), 1);
        assert!(history.latest().is_some());

        assert!(history.add_snapshot(s2).is_some());
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_history_deduplication() {
        let mut history = FileHistory::new(PathBuf::from("/tmp/test.rs"), 5);
        let s1 = FileSnapshot::new(
            PathBuf::from("/tmp/test.rs"),
            "same".to_string(),
            FileOperation::Edit,
        );
        let s2 = FileSnapshot::new(
            PathBuf::from("/tmp/test.rs"),
            "same".to_string(),
            FileOperation::Edit,
        );

        assert!(history.add_snapshot(s1).is_some());
        assert!(history.add_snapshot(s2).is_none()); // deduplicated
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn test_history_max_snapshots_eviction() {
        let mut history = FileHistory::new(PathBuf::from("/tmp/test.rs"), 3);

        for i in 0..5 {
            let content = format!("version_{i}");
            let snapshot =
                FileSnapshot::new(PathBuf::from("/tmp/test.rs"), content, FileOperation::Edit);
            history.add_snapshot(snapshot);
        }

        assert_eq!(history.len(), 3);
        // Oldest snapshots should have been evicted
        assert_eq!(history.latest().unwrap().content, "version_4");
    }

    #[test]
    fn test_history_get_by_id() {
        let mut history = FileHistory::new(PathBuf::from("/tmp/test.rs"), 5);
        let s1 = FileSnapshot::with_id(
            "id-1",
            PathBuf::from("/tmp/test.rs"),
            "v1".to_string(),
            FileOperation::Edit,
        );
        let s2 = FileSnapshot::with_id(
            "id-2",
            PathBuf::from("/tmp/test.rs"),
            "v2".to_string(),
            FileOperation::Edit,
        );

        history.add_snapshot(s1);
        history.add_snapshot(s2);

        assert!(history.get_by_id("id-1").is_some());
        assert!(history.get_by_id("id-2").is_some());
        assert!(history.get_by_id("nonexistent").is_none());
    }

    // ---- FileDiff tests ---------------------------------------------------

    #[test]
    fn test_diff_creation() {
        let after = FileSnapshot::new(
            PathBuf::from("/tmp/new.rs"),
            "line1\nline2\nline3".to_string(),
            FileOperation::Create,
        );
        let diff = compute_diff(None, after.clone()).unwrap();

        assert!(diff.snapshot_before.is_none());
        assert_eq!(diff.additions, 3);
        assert_eq!(diff.deletions, 0);
        assert_eq!(diff.net_change(), 3);
    }

    #[test]
    fn test_diff_no_change() {
        let before = FileSnapshot::new(
            PathBuf::from("/tmp/a.rs"),
            "same\ncontent".to_string(),
            FileOperation::Edit,
        );
        let after = FileSnapshot::new(
            PathBuf::from("/tmp/a.rs"),
            "same\ncontent".to_string(),
            FileOperation::Edit,
        );
        let diff = compute_diff(Some(before), after).unwrap();

        assert_eq!(diff.additions, 0);
        assert_eq!(diff.deletions, 0);
        assert!(diff.hunks.is_empty());
    }

    #[test]
    fn test_diff_addition() {
        let before = FileSnapshot::new(
            PathBuf::from("/tmp/a.rs"),
            "line1\nline2".to_string(),
            FileOperation::Edit,
        );
        let after = FileSnapshot::new(
            PathBuf::from("/tmp/a.rs"),
            "line1\ninserted\nline2".to_string(),
            FileOperation::Edit,
        );
        let diff = compute_diff(Some(before), after).unwrap();

        assert_eq!(diff.additions, 1);
        assert!(diff.deletions <= 1);
    }

    #[test]
    fn test_diff_deletion() {
        let before = FileSnapshot::new(
            PathBuf::from("/tmp/a.rs"),
            "line1\nline2\nline3".to_string(),
            FileOperation::Edit,
        );
        let after = FileSnapshot::new(
            PathBuf::from("/tmp/a.rs"),
            "line1\nline3".to_string(),
            FileOperation::Edit,
        );
        let diff = compute_diff(Some(before), after).unwrap();

        assert_eq!(diff.deletions, 1);
    }

    #[test]
    fn test_diff_to_unified() {
        let after = FileSnapshot::new(
            PathBuf::from("/tmp/new.rs"),
            "line1\nline2".to_string(),
            FileOperation::Create,
        );
        let diff = compute_diff(None, after).unwrap();
        let unified = diff.to_unified();

        assert!(unified.contains("--- /dev/null"));
        assert!(unified.contains("+++ /tmp/new.rs"));
        assert!(unified.contains("@@"));
    }

    // ---- Content hash tests -----------------------------------------------

    #[test]
    fn test_compute_content_hash_deterministic() {
        let h1 = compute_content_hash("hello world");
        let h2 = compute_content_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_compute_content_hash_different() {
        let h1 = compute_content_hash("hello");
        let h2 = compute_content_hash("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_compute_content_hash_sha256_length() {
        let hash = compute_content_hash("test");
        assert_eq!(hash.len(), 64); // SHA-256 hex is 64 chars
    }

    // ---- FileHistoryManager tests -----------------------------------------

    #[test]
    fn test_manager_new_temp() {
        let manager = FileHistoryManager::new_temp().unwrap();
        assert!(manager.history_dir.exists());
    }

    #[test]
    fn test_manager_record_and_get_history() {
        let mut manager = FileHistoryManager::new_temp().unwrap();
        let path = Path::new("/tmp/test_manager.rs");

        manager
            .record_snapshot(path, "v1", FileOperation::Create)
            .unwrap();
        manager
            .record_snapshot(path, "v2", FileOperation::Edit)
            .unwrap();

        let history = manager.get_history(path).unwrap();
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_manager_record_deduplication() {
        let mut manager = FileHistoryManager::new_temp().unwrap();
        let path = Path::new("/tmp/test_dedup.rs");

        let result1 = manager.record_snapshot(path, "same", FileOperation::Edit);
        assert!(result1.is_ok());

        let result2 = manager.record_snapshot(path, "same", FileOperation::Edit);
        assert!(result2.is_err());
    }

    #[test]
    fn test_manager_get_snapshot() {
        let mut manager = FileHistoryManager::new_temp().unwrap();
        let path = Path::new("/tmp/test_get.rs");

        let snapshot = manager
            .record_snapshot(path, "content", FileOperation::Create)
            .unwrap();

        let retrieved = manager.get_snapshot(path, &snapshot.id).unwrap();
        assert_eq!(retrieved.id, snapshot.id);
        assert_eq!(retrieved.content, "content");
    }

    #[test]
    fn test_manager_get_snapshot_not_found() {
        let mut manager = FileHistoryManager::new_temp().unwrap();
        let path = Path::new("/tmp/test_notfound.rs");

        let result = manager.get_snapshot(path, "nonexistent-id");
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_diff() {
        let mut manager = FileHistoryManager::new_temp().unwrap();
        let path = Path::new("/tmp/test_diff.rs");

        let s1 = manager
            .record_snapshot(path, "v1", FileOperation::Create)
            .unwrap();
        let s2 = manager
            .record_snapshot(path, "v1\nv2", FileOperation::Edit)
            .unwrap();

        let diff = manager.diff(path, &s1.id, &s2.id).unwrap();
        assert!(diff.additions >= 1);
    }

    #[test]
    fn test_manager_rollback() {
        let mut manager = FileHistoryManager::new_temp().unwrap();
        let path = Path::new("/tmp/test_rollback.rs");

        let s1 = manager
            .record_snapshot(path, "original", FileOperation::Create)
            .unwrap();
        manager
            .record_snapshot(path, "modified", FileOperation::Edit)
            .unwrap();

        let content = manager.rollback(path, &s1.id).unwrap();
        assert_eq!(content, "original");

        // History should now have 3 entries (original, modified, rollback)
        let history = manager.get_history(path).unwrap();
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_record_turn_snapshot_tags_turn_index() {
        let mut manager = FileHistoryManager::new_temp().unwrap();
        let path = Path::new("/tmp/test_turn_tag.rs");

        let recorded = manager.record_turn_snapshot(path, "v1", 3).unwrap();
        assert_eq!(recorded.turn_index, Some(3));

        // The tagged snapshot is retrievable and carries the tag.
        let history = manager.get_history(path).unwrap();
        assert!(history.snapshots.iter().any(|s| s.turn_index == Some(3)));
    }

    #[test]
    fn test_rewind_file_to_turn_restores_earlier_version() {
        let mut manager = FileHistoryManager::new_temp().unwrap();
        let path = Path::new("/tmp/test_turn_rewind.rs");

        // Simulate per-turn captures across turns 1, 2, 3.
        manager.record_turn_snapshot(path, "turn1", 1).unwrap();
        manager.record_turn_snapshot(path, "turn2", 2).unwrap();
        manager.record_turn_snapshot(path, "turn3", 3).unwrap();

        // Rewind to turn 2 → restore turn-2 content.
        assert_eq!(
            manager.rewind_file_to_turn(path, 2).unwrap(),
            RewindAction::Restore("turn2".to_string())
        );
        // Rewind to turn 3 (latest) → restore turn-3 content.
        assert_eq!(
            manager.rewind_file_to_turn(path, 3).unwrap(),
            RewindAction::Restore("turn3".to_string())
        );
    }

    #[test]
    fn test_rewind_file_to_turn_deletes_future_created_file() {
        let mut manager = FileHistoryManager::new_temp().unwrap();
        let path = Path::new("/tmp/test_turn_delete.rs");

        // File first appears at turn 5 (no earlier history).
        manager.record_turn_snapshot(path, "created", 5).unwrap();

        // Rewinding to turn 3 (before it existed) → Delete.
        assert_eq!(
            manager.rewind_file_to_turn(path, 3).unwrap(),
            RewindAction::Delete
        );
    }

    #[test]
    fn test_rewind_before_turn_strict_boundary() {
        let mut manager = FileHistoryManager::new_temp().unwrap();
        let path = Path::new("/tmp/test_before_turn.rs");

        manager.record_turn_snapshot(path, "turn1", 1).unwrap();
        manager.record_turn_snapshot(path, "turn2", 2).unwrap();

        // Rewind-to-message-2 (drop turn 2 and later) → restore end-of-turn-1.
        assert_eq!(
            manager.rewind_before_turn(path, 2).unwrap(),
            RewindAction::Restore("turn1".to_string())
        );
        // Rewind-to-message-1 → the file has no pre-session history → Delete.
        assert_eq!(
            manager.rewind_before_turn(path, 1).unwrap(),
            RewindAction::Delete
        );
        // Untracked file → NoChange.
        assert_eq!(
            manager
                .rewind_before_turn(Path::new("/tmp/never_seen.rs"), 5)
                .unwrap(),
            RewindAction::NoChange
        );
    }

    #[test]
    fn test_rewind_file_to_turn_no_change_for_untracked() {
        let mut manager = FileHistoryManager::new_temp().unwrap();
        let path = Path::new("/tmp/test_turn_nochg.rs");

        // No snapshots at all → NoChange.
        assert_eq!(
            manager.rewind_file_to_turn(path, 2).unwrap(),
            RewindAction::NoChange
        );

        // Only an UNTAGGED (per-edit) snapshot: still NoChange, since turn rewind
        // only considers turn-tagged snapshots.
        manager
            .record_snapshot(path, "edit", FileOperation::Edit)
            .unwrap();
        assert_eq!(
            manager.rewind_file_to_turn(path, 2).unwrap(),
            RewindAction::NoChange
        );
    }

    #[test]
    fn test_manager_list_tracked_files() {
        let mut manager = FileHistoryManager::new_temp().unwrap();

        manager
            .record_snapshot(Path::new("/tmp/a.rs"), "a", FileOperation::Create)
            .unwrap();
        manager
            .record_snapshot(Path::new("/tmp/b.rs"), "b", FileOperation::Create)
            .unwrap();

        let files = manager.list_tracked_files().unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_manager_cleanup_old_snapshots() {
        let mut manager = FileHistoryManager::new_temp().unwrap();
        let path = Path::new("/tmp/test_cleanup.rs");

        // Record snapshots within the current limit (10)
        for i in 0..10 {
            let content = format!("version_{i}");
            let _ = manager.record_snapshot(path, &content, FileOperation::Edit);
        }

        // Verify all 10 are recorded
        let history = manager.get_history(path).unwrap();
        assert_eq!(history.len(), 10);

        // Now reduce the max_snapshots to force cleanup
        for (_, h) in manager.cache.iter_mut() {
            h.max_snapshots = 5;
        }

        let removed = manager.cleanup_old_snapshots().unwrap();
        // Should have removed 5 (from 10 down to 5)
        assert_eq!(removed, 5);

        let history = manager.get_history(path).unwrap();
        assert_eq!(history.len(), 5);
    }

    #[test]
    fn test_manager_persistence() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = FileHistoryConfig {
            history_dir: temp_dir.path().to_path_buf(),
            max_history_per_file: 10,
            max_total_history_mb: 10,
            ttl: Some(7 * 24 * 60 * 60),
        };

        let path = Path::new("/tmp/test_persist.rs");

        // Record a snapshot in the first manager
        {
            let mut manager = FileHistoryManager::new(config.clone());
            manager
                .record_snapshot(path, "persisted content", FileOperation::Create)
                .unwrap();
        }

        // Load it back in a new manager
        {
            let mut manager = FileHistoryManager::new(config);
            let history = manager.get_history(path).unwrap();
            assert_eq!(history.len(), 1);
            assert_eq!(history.snapshots[0].content, "persisted content");
        }
    }

    // ---- FileHistoryConfig tests -------------------------------------------

    #[test]
    fn test_config_default() {
        let config = FileHistoryConfig::default();
        assert_eq!(config.max_history_per_file, 50);
        assert_eq!(config.max_total_history_mb, 100);
        assert_eq!(
            config.ttl,
            Some(7 * 24 * 60 * 60),
            "default TTL should be 7 days in seconds (604_800)"
        );
    }

    // ---- TTL / cleanup_expired tests --------------------------------------

    #[test]
    fn test_cleanup_expired_before_removes_old_snapshots() {
        let mut manager = FileHistoryManager::new_temp().unwrap();
        let path = Path::new("/tmp/test_ttl.rs");

        manager
            .record_snapshot(path, "v1", FileOperation::Create)
            .unwrap();

        // Backdate the recorded snapshot to well before the cutoff.
        let ancient = Utc::now() - chrono::Duration::days(30);
        for history in manager.cache.values_mut() {
            for snap in &mut history.snapshots {
                snap.timestamp = ancient;
            }
        }

        // Cutoff = now; the backdated snapshot is strictly older → removed.
        let removed = manager.cleanup_expired_before(Utc::now()).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(manager.get_history(path).unwrap().len(), 0);

        // Idempotent: nothing left to remove.
        assert_eq!(manager.cleanup_expired_before(Utc::now()).unwrap(), 0);
    }

    #[test]
    fn test_cleanup_expired_ttl_none_is_noop() {
        let config = FileHistoryConfig {
            history_dir: tempfile::tempdir().unwrap().keep(),
            max_history_per_file: 10,
            max_total_history_mb: 10,
            ttl: None,
        };
        let mut manager = FileHistoryManager::new(config);
        let path = Path::new("/tmp/test_ttl_none.rs");
        manager
            .record_snapshot(path, "v1", FileOperation::Create)
            .unwrap();

        // ttl = None → cleanup_expired removes nothing.
        assert_eq!(manager.cleanup_expired().unwrap(), 0);
        assert_eq!(manager.get_history(path).unwrap().len(), 1);
    }

    #[test]
    fn test_cleanup_expired_removes_only_aged_snapshots() {
        // ttl = 1 hour. A backdated (1-day-old) snapshot expires; a freshly
        // recorded one (within the hour) is kept. The 1-hour margin avoids
        // real-time flakiness on loaded machines.
        let config = FileHistoryConfig {
            history_dir: tempfile::tempdir().unwrap().keep(),
            max_history_per_file: 10,
            max_total_history_mb: 10,
            ttl: Some(3600),
        };
        let mut manager = FileHistoryManager::new(config);
        let path = Path::new("/tmp/test_ttl_aged.rs");

        manager
            .record_snapshot(path, "v1", FileOperation::Create)
            .unwrap();
        // Backdate v1 by one day.
        let ancient = Utc::now() - chrono::Duration::days(1);
        for history in manager.cache.values_mut() {
            for snap in &mut history.snapshots {
                snap.timestamp = ancient;
            }
        }
        // Record a fresh v2 (timestamp ≈ now, within the 1h TTL).
        manager
            .record_snapshot(path, "v2", FileOperation::Edit)
            .unwrap();

        let removed = manager.cleanup_expired().unwrap();
        assert_eq!(removed, 1, "only the backdated v1 should be removed");
        let history = manager.get_history(path).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history.snapshots[0].content, "v2");
    }

    // ---- FileHistoryError tests -------------------------------------------

    #[test]
    fn test_error_display() {
        let err = FileHistoryError::NoHistory("/tmp/test.rs".to_string());
        assert!(err.to_string().contains("test.rs"));

        let err = FileHistoryError::SnapshotNotFound {
            file_path: "/tmp/a.rs".to_string(),
            snapshot_id: "abc".to_string(),
        };
        assert!(err.to_string().contains("abc"));
    }

    // ---- from_env / from_env_vars (W6-2 A.4) ----------------------------

    #[test]
    fn from_env_vars_default_enabled() {
        // No disable flag → Some(config with default values).
        let cfg = FileHistoryConfig::from_env_vars(None, None, None).unwrap();
        assert_eq!(cfg.ttl, Some(7 * 24 * 60 * 60));
        assert_eq!(cfg.max_history_per_file, 50);
    }

    #[test]
    fn from_env_vars_disabled_by_flag() {
        for v in ["0", "false", "off", "no"] {
            assert!(
                FileHistoryConfig::from_env_vars(Some(v.to_string()), None, None).is_none(),
                "SHANNON_FILE_HISTORY={v} must disable"
            );
        }
    }

    #[test]
    fn from_env_vars_truthy_or_unknown_keeps_enabled() {
        // "1"/"true"/"on"/"yes" and unknown/empty values do NOT disable.
        for v in ["1", "true", "on", "yes", "enabled", "maybe", ""] {
            assert!(
                FileHistoryConfig::from_env_vars(Some(v.to_string()), None, None).is_some(),
                "SHANNON_FILE_HISTORY={v:?} must not disable"
            );
        }
    }

    #[test]
    fn from_env_vars_dir_override() {
        let cfg =
            FileHistoryConfig::from_env_vars(None, Some("/tmp/shannon_hist_xyz".to_string()), None)
                .unwrap();
        assert_eq!(cfg.history_dir, PathBuf::from("/tmp/shannon_hist_xyz"));
    }

    #[test]
    fn from_env_vars_dir_override_ignores_whitespace_only() {
        let cfg = FileHistoryConfig::from_env_vars(None, Some("   ".to_string()), None).unwrap();
        // Whitespace-only dir keeps the default rather than wiping it.
        assert!(cfg.history_dir.ends_with("file_history"));
    }

    #[test]
    fn from_env_vars_ttl_override() {
        let cfg = FileHistoryConfig::from_env_vars(None, None, Some("3600".to_string())).unwrap();
        assert_eq!(cfg.ttl, Some(3600));
    }

    #[test]
    fn from_env_vars_ttl_zero_means_no_expiry() {
        let cfg = FileHistoryConfig::from_env_vars(None, None, Some("0".to_string())).unwrap();
        assert_eq!(cfg.ttl, None, "TTL=0 disables time-based expiry");
    }

    #[test]
    fn from_env_vars_ttl_unparseable_keeps_default() {
        let cfg =
            FileHistoryConfig::from_env_vars(None, None, Some("not-a-number".to_string())).unwrap();
        assert_eq!(cfg.ttl, Some(7 * 24 * 60 * 60));
    }
}
