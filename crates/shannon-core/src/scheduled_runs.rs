//! Execution history for scheduled tasks.
//!
//! Append-only JSONL storage partitioned by year/month for efficient rolling.
//! Each run is a single JSON line; updates append a new line with the same
//! `run_id` and a higher `revision` (last-write-wins on read).
//!
//! ## Layout
//! ```text
//! ~/.shannon/scheduled-runs/
//! ├── 2026/
//! │   ├── 06.jsonl   # June 2026 runs
//! │   └── 05.jsonl   # May 2026 runs
//! └── 2025/
//!     └── 12.jsonl
//! ```
//!
//! ## Rolling
//! Lines older than `rolling_days` (default 90) are pruned on [`ScheduledRunsStore::prune_old`].
//! Pruning is in-place: lines still within the window are rewritten.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};

/// Errors returned by the runs store.
#[derive(Debug, thiserror::Error)]
pub enum RunsStoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Status of a scheduled run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    /// Run is currently executing.
    Running,
    /// Run completed successfully.
    Succeeded,
    /// Run failed (see `error_message`).
    Failed,
    /// Run was cancelled by the user.
    Cancelled,
    /// Run was archived (Codex `auto_archive_when_empty` pattern — no findings).
    Archived,
}

/// A single execution record for a scheduled task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledRun {
    /// Unique run ID (8-char prefix of UUID v4).
    pub run_id: String,
    /// ID of the task that ran.
    pub task_id: String,
    /// Name of the task that ran (denormalized for historical readability).
    pub task_name: String,
    /// When the run started.
    pub started_at: DateTime<Utc>,
    /// When the run finished (None if still running).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    /// Run status.
    pub status: RunStatus,
    /// Error message if status is `Failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// Cost in USD (None if not tracked).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Total token usage (None if not tracked).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<u64>,
    /// Revision number for last-write-wins semantics (incremented on updates).
    #[serde(default)]
    pub revision: u32,
}

impl ScheduledRun {
    /// Create a new run record with status = Running and a fresh UUID.
    pub fn start(task_id: &str, task_name: &str) -> Self {
        Self {
            run_id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
            task_id: task_id.to_string(),
            task_name: task_name.to_string(),
            started_at: Utc::now(),
            finished_at: None,
            status: RunStatus::Running,
            error_message: None,
            cost_usd: None,
            token_usage: None,
            revision: 0,
        }
    }

    /// Mark this run as finished with the given status.
    pub fn finish(&mut self, status: RunStatus, error: Option<String>) {
        self.finished_at = Some(Utc::now());
        self.status = status;
        self.error_message = error;
        self.revision += 1;
    }
}

/// Append-only JSONL execution history store.
#[derive(Debug, Clone)]
pub struct ScheduledRunsStore {
    base_dir: PathBuf,
    /// Rolling window in days. Lines older than this are pruned.
    pub rolling_days: u32,
    /// Archive threshold in bytes. Files larger than this are candidates for
    /// external archival (not yet implemented — placeholder for Sprint 2).
    pub archive_threshold_bytes: u64,
}

impl ScheduledRunsStore {
    /// Create a store at the default location (`~/.shannon/scheduled-runs/`).
    pub fn new() -> Self {
        Self {
            base_dir: default_base_dir(),
            rolling_days: 90,
            archive_threshold_bytes: 10 * 1024 * 1024,
        }
    }

    /// Create a store at a custom base directory (useful for testing).
    pub fn with_base(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            rolling_days: 90,
            archive_threshold_bytes: 10 * 1024 * 1024,
        }
    }

    /// Return the base directory path.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Append a run record. Returns the run_id.
    pub fn record(&self, run: &ScheduledRun) -> Result<String, RunsStoreError> {
        let path = self.path_for(run.started_at);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        let line = serde_json::to_string(run)?;
        writeln!(file, "{line}")?;
        Ok(run.run_id.clone())
    }

    /// Convenience: record a new `Running` run for a task. Returns the run_id.
    pub fn start_run(&self, task_id: &str, task_name: &str) -> Result<String, RunsStoreError> {
        let run = ScheduledRun::start(task_id, task_name);
        self.record(&run)
    }

    /// Update an existing run (appends a new line with the same `run_id`).
    pub fn update(
        &self,
        run_id: &str,
        update_fn: impl FnOnce(&mut ScheduledRun),
    ) -> Result<(), RunsStoreError> {
        let existing = self.find_by_id(run_id)?.ok_or_else(|| {
            RunsStoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("run not found: {run_id}"),
            ))
        })?;
        let mut updated = existing;
        update_fn(&mut updated);
        self.record(&updated)?;
        Ok(())
    }

    /// Find a single run by ID (returns the latest revision).
    pub fn find_by_id(&self, run_id: &str) -> Result<Option<ScheduledRun>, RunsStoreError> {
        for run in self.iter_all()? {
            if run.run_id == run_id {
                return Ok(Some(run));
            }
        }
        Ok(None)
    }

    /// List runs for a specific task, most recent first.
    pub fn list_by_task(
        &self,
        task_id: &str,
        limit: usize,
    ) -> Result<Vec<ScheduledRun>, RunsStoreError> {
        let mut runs: Vec<_> = self
            .iter_all()?
            .into_iter()
            .filter(|r| r.task_id == task_id)
            .collect();
        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        runs.truncate(limit);
        Ok(runs)
    }

    /// List runs in a time range (start inclusive, end exclusive), oldest first.
    pub fn list_by_time_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<ScheduledRun>, RunsStoreError> {
        let mut runs: Vec<_> = self
            .iter_all()?
            .into_iter()
            .filter(|r| r.started_at >= start && r.started_at < end)
            .collect();
        runs.sort_by(|a, b| a.started_at.cmp(&b.started_at));
        Ok(runs)
    }

    /// List the most recent runs across all tasks, newest first.
    pub fn list_recent(&self, limit: usize) -> Result<Vec<ScheduledRun>, RunsStoreError> {
        let mut runs = self.iter_all()?;
        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        runs.truncate(limit);
        Ok(runs)
    }

    /// Prune entries older than `rolling_days`. Returns count pruned.
    ///
    /// Pruning is in-place: the file is rewritten with only entries inside
    /// the window. The latest revision per `run_id` within the window is
    /// preserved.
    pub fn prune_old(&self) -> Result<usize, RunsStoreError> {
        let threshold = Utc::now() - chrono::Duration::days(self.rolling_days as i64);
        if !self.base_dir.exists() {
            return Ok(0);
        }
        let mut pruned = 0usize;
        for year_entry in fs::read_dir(&self.base_dir)? {
            let year_entry = year_entry?;
            if !year_entry.file_type()?.is_dir() {
                continue;
            }
            for month_entry in fs::read_dir(year_entry.path())? {
                let month_entry = month_entry?;
                let path = month_entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                pruned += self.prune_file(&path, threshold)?;
            }
        }
        Ok(pruned)
    }

    /// Prune a single file in-place, keeping only entries at or after `threshold`.
    fn prune_file(&self, path: &Path, threshold: DateTime<Utc>) -> Result<usize, RunsStoreError> {
        let file = match OpenOptions::new().read(true).open(path) {
            Ok(f) => f,
            Err(_) => return Ok(0),
        };
        let reader = BufReader::new(file);
        let mut latest_by_id: HashMap<String, ScheduledRun> = HashMap::new();
        let mut pruned = 0usize;
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<ScheduledRun>(&line) {
                Ok(run) => {
                    if run.started_at < threshold {
                        pruned += 1;
                    } else {
                        let entry = latest_by_id
                            .entry(run.run_id.clone())
                            .or_insert(run.clone());
                        if run.revision > entry.revision {
                            *entry = run;
                        }
                    }
                }
                Err(_) => continue,
            }
        }
        if pruned == 0 {
            return Ok(0);
        }
        let kept: Vec<ScheduledRun> = latest_by_id.into_values().collect();
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(path)?;
        for run in &kept {
            let line = serde_json::to_string(run)?;
            writeln!(file, "{line}")?;
        }
        Ok(pruned)
    }

    /// Iterate all runs across all months, returning the latest revision per run_id.
    fn iter_all(&self) -> Result<Vec<ScheduledRun>, RunsStoreError> {
        let mut by_id: HashMap<String, ScheduledRun> = HashMap::new();
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }
        for year_entry in fs::read_dir(&self.base_dir)? {
            let year_entry = year_entry?;
            if !year_entry.file_type()?.is_dir() {
                continue;
            }
            for month_entry in fs::read_dir(year_entry.path())? {
                let month_entry = month_entry?;
                let path = month_entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                self.collect_latest(&path, &mut by_id)?;
            }
        }
        Ok(by_id.into_values().collect())
    }

    /// Read a file and track the latest revision per run_id.
    fn collect_latest(
        &self,
        path: &Path,
        latest: &mut HashMap<String, ScheduledRun>,
    ) -> Result<(), RunsStoreError> {
        let file = match OpenOptions::new().read(true).open(path) {
            Ok(f) => f,
            Err(_) => return Ok(()),
        };
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(run) = serde_json::from_str::<ScheduledRun>(&line) {
                let entry = latest
                    .entry(run.run_id.clone())
                    .or_insert_with(|| run.clone());
                if run.revision > entry.revision {
                    *entry = run;
                }
            }
        }
        Ok(())
    }

    /// Compute the JSONL path for a given timestamp.
    fn path_for(&self, ts: DateTime<Utc>) -> PathBuf {
        self.base_dir
            .join(format!("{:04}", ts.year()))
            .join(format!("{:02}.jsonl", ts.month()))
    }
}

impl Default for ScheduledRunsStore {
    fn default() -> Self {
        Self::new()
    }
}

fn default_base_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".shannon")
        .join("scheduled-runs")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_find() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let id = store.start_run("task-1", "Task One").unwrap();

        let found = store.find_by_id(&id).unwrap();
        assert!(found.is_some());
        let run = found.unwrap();
        assert_eq!(run.task_id, "task-1");
        assert_eq!(run.status, RunStatus::Running);
    }

    #[test]
    fn test_update_status() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let id = store.start_run("task-1", "Task One").unwrap();

        store
            .update(&id, |r| r.finish(RunStatus::Succeeded, None))
            .unwrap();

        let run = store.find_by_id(&id).unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Succeeded);
        assert!(run.finished_at.is_some());
    }

    #[test]
    fn test_list_by_task() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        for _ in 0..3 {
            store.start_run("task-1", "A").unwrap();
        }
        store.start_run("task-2", "B").unwrap();

        let list = store.list_by_task("task-1", 10).unwrap();
        assert_eq!(list.len(), 3);
        assert!(list.iter().all(|r| r.task_id == "task-1"));
    }

    #[test]
    fn test_list_by_task_respects_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        for _ in 0..5 {
            store.start_run("task-1", "A").unwrap();
        }

        let list = store.list_by_task("task-1", 2).unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_list_by_time_range() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());

        let now = Utc::now();
        let mut run = ScheduledRun::start("task-1", "A");
        run.started_at = now - chrono::Duration::days(2);
        store.record(&run).unwrap();

        let mut run2 = ScheduledRun::start("task-1", "A");
        run2.started_at = now;
        store.record(&run2).unwrap();

        let start = now - chrono::Duration::days(1);
        let list = store
            .list_by_time_range(start, now + chrono::Duration::seconds(1))
            .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].started_at, now);
    }

    #[test]
    fn test_find_missing_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        assert!(store.find_by_id("nope").unwrap().is_none());
    }

    #[test]
    fn test_prune_old_removes_expired() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());

        let mut old = ScheduledRun::start("task-1", "A");
        old.started_at = Utc::now() - chrono::Duration::days(100);
        store.record(&old).unwrap();

        let fresh_id = store.start_run("task-1", "A").unwrap();

        let pruned = store.prune_old().unwrap();
        assert!(pruned >= 1);

        assert!(store.find_by_id(&old.run_id).unwrap().is_none());
        assert!(store.find_by_id(&fresh_id).unwrap().is_some());
    }

    #[test]
    fn test_prune_preserves_latest_revision() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());

        let id = store.start_run("task-1", "A").unwrap();
        store
            .update(&id, |r| r.finish(RunStatus::Succeeded, None))
            .unwrap();

        let pruned = store.prune_old().unwrap();
        assert_eq!(pruned, 0);

        let run = store.find_by_id(&id).unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Succeeded);
    }

    #[test]
    fn test_path_layout_year_month() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let _id = store.start_run("task-1", "A").unwrap();
        let now = Utc::now();
        let expected = tmp
            .path()
            .join(format!("{:04}", now.year()))
            .join(format!("{:02}.jsonl", now.month()));
        assert!(expected.exists());
    }

    #[test]
    fn test_finish_records_error() {
        let mut run = ScheduledRun::start("t1", "T");
        run.finish(RunStatus::Failed, Some("oops".into()));
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(run.error_message.as_deref(), Some("oops"));
        assert!(run.finished_at.is_some());
    }

    #[test]
    fn test_finish_archived() {
        let mut run = ScheduledRun::start("t1", "T");
        run.finish(RunStatus::Archived, None);
        assert_eq!(run.status, RunStatus::Archived);
    }

    #[test]
    fn test_record_returns_run_id() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let id = store.start_run("task-1", "A").unwrap();
        assert_eq!(id.len(), 8);
    }

    #[test]
    fn test_empty_store_operations() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        assert!(store.list_by_task("any", 10).unwrap().is_empty());
        assert_eq!(store.prune_old().unwrap(), 0);
    }
}
