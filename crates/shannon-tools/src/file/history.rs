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
use thiserror::Error;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during file history operations.
#[derive(Error, Debug)]
pub enum FileHistoryError {
    #[error("Snapshot not found: {file_path} / {snapshot_id}")]
    SnapshotNotFound { file_path: String, snapshot_id: String },

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
}

impl FileSnapshot {
    /// Create a new snapshot.
    pub fn new(
        file_path: PathBuf,
        content: String,
        operation: FileOperation,
    ) -> Self {
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
        }
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
    /// In-memory cache of file histories.
    cache: HashMap<PathBuf, FileHistory>,
    /// Whether the cache has been loaded from disk.
    cache_loaded: bool,
}

impl FileHistoryManager {
    /// Create a new FileHistoryManager with the given configuration.
    pub fn new(config: FileHistoryConfig) -> Self {
        let manager = Self {
            history_dir: config.history_dir,
            max_history_per_file: config.max_history_per_file,
            max_total_history_mb: config.max_total_history_mb,
            cache: HashMap::new(),
            cache_loaded: false,
        };

        // Ensure the history directory exists
        let _ = std::fs::create_dir_all(&manager.history_dir);

        manager
    }

    /// Create a new manager with a temporary directory (for testing).
    pub fn new_temp() -> Result<Self, FileHistoryError> {
        let temp_path = std::env::temp_dir().join(format!("shannon_history_test_{}", Uuid::new_v4()));
        let config = FileHistoryConfig {
            history_dir: temp_path,
            max_history_per_file: 10,
            max_total_history_mb: 10,
        };
        Ok(Self::new(config))
    }

    /// Ensure the history directory exists.
    fn ensure_dir(&self) -> Result<(), FileHistoryError> {
        std::fs::create_dir_all(&self.history_dir)?;
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
            let content = std::fs::read_to_string(&index_path)?;
            let index: HashMap<String, Vec<String>> = serde_json::from_str(&content)?;

            for (file_path_str, snapshot_ids) in index {
                let file_path = PathBuf::from(&file_path_str);
                let mut snapshots = Vec::new();

                for id in &snapshot_ids {
                    let snapshot_path = self.file_dir(&file_path).join(format!("{id}.json"));
                    if snapshot_path.exists() {
                        if let Ok(content) = std::fs::read_to_string(&snapshot_path) {
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
        std::fs::write(&index_path, content)?;

        Ok(())
    }

    /// Save a single snapshot to disk.
    fn save_snapshot(&self, snapshot: &FileSnapshot) -> Result<(), FileHistoryError> {
        let dir = self.file_dir(&snapshot.file_path);
        std::fs::create_dir_all(&dir)?;

        let snapshot_path = dir.join(format!("{}.json", snapshot.id));
        let content = serde_json::to_string_pretty(snapshot)?;
        std::fs::write(&snapshot_path, content)?;

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
    pub fn rollback(
        &mut self,
        file_path: &Path,
        id: &str,
    ) -> Result<String, FileHistoryError> {
        self.ensure_cache_loaded()?;

        let snapshot = self.get_snapshot(file_path, id)?;

        // Record the rollback as an edit operation
        let _ = self.record_snapshot(file_path, &snapshot.content, FileOperation::Edit);

        Ok(snapshot.content.clone())
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
            let excess = history.snapshots.len().saturating_sub(history.max_snapshots);
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
            let snapshot_path =
                self.file_dir(file_path).join(format!("{snapshot_id}.json"));
            if let Err(e) = std::fs::remove_file(&snapshot_path) {
                tracing::debug!("Failed to remove old snapshot: {e}");
            }
        }

        if removed > 0 {
            self.save_index()?;
        }

        Ok(removed)
    }

    /// Check total storage usage against the quota.
    fn check_storage_quota(&self) -> Result<(), FileHistoryError> {
        let total_bytes = dir_size(&self.history_dir).unwrap_or(0);
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

    let additions = hunks.iter().filter(|h| h.new_count > 0).map(|h| h.new_count).sum();
    let deletions = hunks.iter().filter(|h| h.old_count > 0).map(|h| h.old_count).sum();

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
fn dir_size(path: &Path) -> Result<u64, std::io::Error> {
    if !path.is_dir() {
        return Ok(std::fs::metadata(path)?.len());
    }

    let mut total = 0u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            total += dir_size(&entry.path())?;
        } else {
            total += metadata.len();
        }
    }
    Ok(total)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
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
        let s1 = FileSnapshot::new(PathBuf::from("/tmp/test.rs"), "v1".to_string(), FileOperation::Edit);
        let s2 = FileSnapshot::new(PathBuf::from("/tmp/test.rs"), "v2".to_string(), FileOperation::Edit);

        assert!(history.add_snapshot(s1).is_some());
        assert_eq!(history.len(), 1);
        assert!(history.latest().is_some());

        assert!(history.add_snapshot(s2).is_some());
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_history_deduplication() {
        let mut history = FileHistory::new(PathBuf::from("/tmp/test.rs"), 5);
        let s1 = FileSnapshot::new(PathBuf::from("/tmp/test.rs"), "same".to_string(), FileOperation::Edit);
        let s2 = FileSnapshot::new(PathBuf::from("/tmp/test.rs"), "same".to_string(), FileOperation::Edit);

        assert!(history.add_snapshot(s1).is_some());
        assert!(history.add_snapshot(s2).is_none()); // deduplicated
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn test_history_max_snapshots_eviction() {
        let mut history = FileHistory::new(PathBuf::from("/tmp/test.rs"), 3);

        for i in 0..5 {
            let content = format!("version_{i}");
            let snapshot = FileSnapshot::new(PathBuf::from("/tmp/test.rs"), content, FileOperation::Edit);
            history.add_snapshot(snapshot);
        }

        assert_eq!(history.len(), 3);
        // Oldest snapshots should have been evicted
        assert_eq!(history.latest().unwrap().content, "version_4");
    }

    #[test]
    fn test_history_get_by_id() {
        let mut history = FileHistory::new(PathBuf::from("/tmp/test.rs"), 5);
        let s1 = FileSnapshot::with_id("id-1", PathBuf::from("/tmp/test.rs"), "v1".to_string(), FileOperation::Edit);
        let s2 = FileSnapshot::with_id("id-2", PathBuf::from("/tmp/test.rs"), "v2".to_string(), FileOperation::Edit);

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
}
