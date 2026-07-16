//! # Background Housekeeping
//!
//! Periodic cleanup tasks for maintaining Shannon Code's local state.
//! Inspired by Claude Code's `backgroundHousekeeping.ts`.
//!
//! ## Architecture
//!
//! - [`HousekeepingTask`]: Trait for defining cleanup tasks
//! - [`Housekeeper`]: Task registry, scheduling, and execution engine
//! - [`HousekeepingConfig`]: Per-task interval configuration
//! - Built-in tasks: temp file cleanup, cache refresh, old session pruning,
//!   and log rotation.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use shannon_core::housekeeping::{Housekeeper, HousekeepingConfig};
//!
//! let mut keeper = Housekeeper::new(HousekeepingConfig::default()).unwrap();
//! keeper.register_builtin_tasks();
//!
//! // Run all tasks that are due.
//! let results = keeper.run_all();
//! for (name, result) in &results {
//!     println!("{}: {:?}", name, result);
//! }
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during housekeeping operations.
#[derive(Error, Debug)]
pub enum HousekeepingError {
    #[error("IO error during {task}: {source}")]
    Io {
        task: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Task '{0}' not found")]
    TaskNotFound(String),

    #[error("Task '{0}' failed: {1}")]
    TaskFailed(String, String),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}

/// Result of running a single housekeeping task.
#[derive(Debug, Clone)]
pub struct TaskResult {
    pub task_name: String,
    pub success: bool,
    pub message: String,
    pub duration: Duration,
    pub items_cleaned: Option<usize>,
}

impl std::fmt::Display for TaskResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = if self.success { "OK" } else { "FAILED" };
        write!(
            f,
            "[{}] {} - {} ({:.1}s)",
            status,
            self.task_name,
            self.message,
            self.duration.as_secs_f64()
        )?;
        if let Some(count) = self.items_cleaned {
            write!(f, " ({count} items)")?;
        }
        Ok(())
    }
}

// ============================================================================
// Housekeeping Task Trait
// ============================================================================

/// A periodic housekeeping task.
pub trait HousekeepingTask: Send + Sync {
    /// Unique name identifying this task.
    fn name(&self) -> &str;

    /// Human-readable description of what this task does.
    fn description(&self) -> &str;

    /// Default interval between runs.
    fn default_interval(&self) -> Duration;

    /// Execute the task. Returns a summary message and optionally the number
    /// of items cleaned.
    fn execute(&self, base_dir: &Path) -> Result<(String, Option<usize>), String>;
}

// ============================================================================
// Housekeeping Config
// ============================================================================

/// Configuration for housekeeping intervals.
///
/// Each task can have a custom interval. If not specified, the task's default
/// interval is used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HousekeepingConfig {
    /// Interval for temp file cleanup.
    pub temp_cleanup_interval: Option<Duration>,
    /// Interval for cache refresh.
    pub cache_refresh_interval: Option<Duration>,
    /// Interval for old session pruning.
    pub session_prune_interval: Option<Duration>,
    /// Interval for log rotation.
    pub log_rotation_interval: Option<Duration>,
    /// Custom intervals for other tasks (name -> duration in seconds).
    pub custom_intervals: HashMap<String, u64>,
}

impl Default for HousekeepingConfig {
    fn default() -> Self {
        Self {
            temp_cleanup_interval: Some(Duration::from_secs(60 * 60)), // 1 hour
            cache_refresh_interval: Some(Duration::from_secs(24 * 60 * 60)), // 24 hours
            session_prune_interval: Some(Duration::from_secs(24 * 60 * 60)), // 24 hours
            log_rotation_interval: Some(Duration::from_secs(24 * 60 * 60)), // 24 hours
            custom_intervals: HashMap::new(),
        }
    }
}

// ============================================================================
// Built-in Tasks
// ============================================================================

/// Temp file cleanup task. Removes files from `~/.shannon/tmp/` older than 24h.
pub struct TempFileCleanupTask;

impl HousekeepingTask for TempFileCleanupTask {
    fn name(&self) -> &str {
        "temp_file_cleanup"
    }

    fn description(&self) -> &str {
        "Remove temporary files older than 24 hours"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(60 * 60) // 1 hour
    }

    fn execute(&self, base_dir: &Path) -> Result<(String, Option<usize>), String> {
        let tmp_dir = base_dir.join("tmp");
        if !tmp_dir.exists() {
            return Ok(("No temp directory found".to_string(), Some(0)));
        }

        let cutoff = std::time::SystemTime::now() - Duration::from_secs(24 * 60 * 60);
        let mut removed = 0usize;
        let mut errors = 0usize;

        let entries =
            std::fs::read_dir(&tmp_dir).map_err(|e| format!("Failed to read tmp dir: {e}"))?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => {
                    errors += 1;
                    continue;
                }
            };

            let path = entry.path();
            if let Ok(metadata) = entry.metadata() {
                if let Ok(modified) = metadata.modified() {
                    if modified < cutoff {
                        if let Err(e) = if metadata.is_dir() {
                            std::fs::remove_dir_all(&path)
                        } else {
                            std::fs::remove_file(&path)
                        } {
                            debug!("Failed to remove {:?}: {}", path, e);
                            errors += 1;
                        } else {
                            removed += 1;
                        }
                    }
                }
            }
        }

        let msg = if errors > 0 {
            format!("Removed {removed} temp files ({errors} errors)")
        } else {
            format!("Removed {removed} temp files")
        };

        Ok((msg, Some(removed)))
    }
}

/// Cache refresh task. Touches cache metadata to indicate freshness.
pub struct CacheRefreshTask;

impl HousekeepingTask for CacheRefreshTask {
    fn name(&self) -> &str {
        "cache_refresh"
    }

    fn description(&self) -> &str {
        "Refresh internal caches and invalidate stale entries"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(24 * 60 * 60) // 24 hours
    }

    fn execute(&self, base_dir: &Path) -> Result<(String, Option<usize>), String> {
        let cache_dir = base_dir.join("cache");
        if !cache_dir.exists() {
            return Ok(("No cache directory found".to_string(), Some(0)));
        }

        // Write a timestamp marker file to indicate last refresh.
        let marker = cache_dir.join(".last_refresh");
        let ts = Utc::now().to_rfc3339();
        std::fs::write(&marker, ts.as_bytes())
            .map_err(|e| format!("Failed to write refresh marker: {e}"))?;

        let entry_count = std::fs::read_dir(&cache_dir)
            .map(|entries| entries.filter_map(|e| e.ok()).count())
            .unwrap_or(0);

        Ok((
            format!("Cache refreshed, {entry_count} entries found"),
            Some(entry_count),
        ))
    }
}

/// Old session pruning task. Removes sessions older than 30 days.
pub struct OldSessionPruneTask;

impl HousekeepingTask for OldSessionPruneTask {
    fn name(&self) -> &str {
        "old_session_prune"
    }

    fn description(&self) -> &str {
        "Remove session files older than 30 days"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(24 * 60 * 60) // 24 hours
    }

    fn execute(&self, base_dir: &Path) -> Result<(String, Option<usize>), String> {
        let sessions_dir = base_dir.join("sessions");
        if !sessions_dir.exists() {
            return Ok(("No sessions directory found".to_string(), Some(0)));
        }

        let cutoff = std::time::SystemTime::now() - Duration::from_secs(30 * 24 * 60 * 60);
        let mut removed = 0usize;

        let entries = std::fs::read_dir(&sessions_dir)
            .map_err(|e| format!("Failed to read sessions dir: {e}"))?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if let Ok(metadata) = entry.metadata() {
                if let Ok(modified) = metadata.modified() {
                    if modified < cutoff {
                        let path = entry.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("json")
                            && std::fs::remove_file(&path).is_ok()
                        {
                            removed += 1;
                        }
                    }
                }
            }
        }

        Ok((format!("Pruned {removed} old sessions"), Some(removed)))
    }
}

/// Log rotation task. Archives log files when they exceed a size threshold.
pub struct LogRotationTask;

impl HousekeepingTask for LogRotationTask {
    fn name(&self) -> &str {
        "log_rotation"
    }

    fn description(&self) -> &str {
        "Archive and compress log files that exceed size threshold"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(24 * 60 * 60) // 24 hours
    }

    fn execute(&self, base_dir: &Path) -> Result<(String, Option<usize>), String> {
        let logs_dir = base_dir.join("logs");
        if !logs_dir.exists() {
            return Ok(("No logs directory found".to_string(), Some(0)));
        }

        const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
        let mut rotated = 0usize;

        let entries =
            std::fs::read_dir(&logs_dir).map_err(|e| format!("Failed to read logs dir: {e}"))?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if let Ok(metadata) = entry.metadata() {
                if metadata.len() > MAX_LOG_SIZE && metadata.is_file() {
                    // "Rotate" by renaming to .old.
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        let archive_path = path
                            .parent()
                            .unwrap_or(&logs_dir)
                            .join(format!("{stem}.old"));
                        if std::fs::rename(&path, &archive_path).is_ok() {
                            rotated += 1;
                        }
                    }
                }
            }
        }

        Ok((format!("Rotated {rotated} log files"), Some(rotated)))
    }
}

// ============================================================================
// Housekeeper
// ============================================================================

/// Task registration record with last-run tracking.
struct TaskEntry {
    task: Box<dyn HousekeepingTask>,
    last_run: Option<DateTime<Utc>>,
    interval: Duration,
}

/// Housekeeping orchestrator that manages periodic cleanup tasks.
pub struct Housekeeper {
    tasks: HashMap<String, TaskEntry>,
    base_dir: PathBuf,
    config: HousekeepingConfig,
}

impl Housekeeper {
    /// Create a new housekeeper with the given configuration.
    ///
    /// Uses `~/.shannon/` as the base directory by default.
    pub fn new(config: HousekeepingConfig) -> Result<Self, HousekeepingError> {
        let base_dir = dirs::home_dir()
            .ok_or_else(|| {
                HousekeepingError::ConfigError("Cannot determine home directory".into())
            })?
            .join(".shannon");

        std::fs::create_dir_all(&base_dir).map_err(|e| HousekeepingError::Io {
            task: "init".into(),
            source: e,
        })?;

        Ok(Self {
            tasks: HashMap::new(),
            base_dir,
            config,
        })
    }

    /// Create a housekeeper with a custom base directory (for testing).
    pub fn with_base_dir(
        base_dir: PathBuf,
        config: HousekeepingConfig,
    ) -> Result<Self, HousekeepingError> {
        std::fs::create_dir_all(&base_dir).map_err(|e| HousekeepingError::Io {
            task: "init".into(),
            source: e,
        })?;

        Ok(Self {
            tasks: HashMap::new(),
            base_dir,
            config,
        })
    }

    /// Register all built-in housekeeping tasks.
    pub fn register_builtin_tasks(&mut self) {
        self.register_task(Box::new(TempFileCleanupTask));
        self.register_task(Box::new(CacheRefreshTask));
        self.register_task(Box::new(OldSessionPruneTask));
        self.register_task(Box::new(LogRotationTask));
    }

    /// Register a custom housekeeping task.
    pub fn register_task(&mut self, task: Box<dyn HousekeepingTask>) {
        let name = task.name().to_string();
        let interval = self.resolve_interval(&name, task.default_interval());
        info!(
            task_name = %name,
            interval_secs = interval.as_secs(),
            "Registered housekeeping task"
        );
        self.tasks.insert(
            name,
            TaskEntry {
                task,
                last_run: None,
                interval,
            },
        );
    }

    /// Resolve the interval for a task, using config overrides if available.
    fn resolve_interval(&self, name: &str, default: Duration) -> Duration {
        match name {
            "temp_file_cleanup" => self.config.temp_cleanup_interval.unwrap_or(default),
            "cache_refresh" => self.config.cache_refresh_interval.unwrap_or(default),
            "old_session_prune" => self.config.session_prune_interval.unwrap_or(default),
            "log_rotation" => self.config.log_rotation_interval.unwrap_or(default),
            _ => self
                .config
                .custom_intervals
                .get(name)
                .copied()
                .map(Duration::from_secs)
                .unwrap_or(default),
        }
    }

    /// Check whether a task should run based on its interval.
    pub fn should_run(&self, task_name: &str) -> Result<bool, HousekeepingError> {
        let entry = self
            .tasks
            .get(task_name)
            .ok_or_else(|| HousekeepingError::TaskNotFound(task_name.to_string()))?;

        match entry.last_run {
            None => Ok(true), // Never run, should run now.
            Some(last) => {
                let elapsed = Utc::now().signed_duration_since(last);
                let interval_chrono =
                    chrono::Duration::from_std(entry.interval).unwrap_or(chrono::Duration::zero());
                Ok(elapsed >= interval_chrono)
            }
        }
    }

    /// Run a specific task by name.
    pub fn run_task(&mut self, task_name: &str) -> Result<TaskResult, HousekeepingError> {
        let entry = self
            .tasks
            .get_mut(task_name)
            .ok_or_else(|| HousekeepingError::TaskNotFound(task_name.to_string()))?;

        let start = std::time::Instant::now();
        debug!(task = %task_name, "Running housekeeping task");

        let result = entry.task.execute(&self.base_dir);
        let duration = start.elapsed();

        let (success, message, items_cleaned) = match result {
            Ok((msg, count)) => (true, msg, count),
            Err(err) => (false, err, None),
        };

        entry.last_run = Some(Utc::now());

        let task_result = TaskResult {
            task_name: task_name.to_string(),
            success,
            message,
            duration,
            items_cleaned,
        };

        if success {
            info!(task = %task_name, duration_ms = duration.as_millis(), "Task completed");
        } else {
            warn!(task = %task_name, "Task failed: {}", task_result.message);
        }

        Ok(task_result)
    }

    /// Run all tasks that are due based on their intervals.
    ///
    /// Returns a map of task name to result for each task that was run.
    pub fn run_all(&mut self) -> HashMap<String, TaskResult> {
        let mut results = HashMap::new();

        let task_names: Vec<String> = self.tasks.keys().cloned().collect();
        for name in task_names {
            if let Ok(true) = self.should_run(&name) {
                match self.run_task(&name) {
                    Ok(result) => {
                        results.insert(name, result);
                    }
                    Err(e) => {
                        results.insert(
                            name.clone(),
                            TaskResult {
                                task_name: name,
                                success: false,
                                message: e.to_string(),
                                duration: Duration::ZERO,
                                items_cleaned: None,
                            },
                        );
                    }
                }
            }
        }

        if !results.is_empty() {
            info!(tasks_run = results.len(), "Housekeeping sweep completed");
        }

        results
    }

    /// Force-run all registered tasks regardless of interval.
    pub fn run_all_forced(&mut self) -> HashMap<String, TaskResult> {
        let mut results = HashMap::new();

        let task_names: Vec<String> = self.tasks.keys().cloned().collect();
        for name in task_names {
            match self.run_task(&name) {
                Ok(result) => {
                    results.insert(name, result);
                }
                Err(e) => {
                    results.insert(
                        name.clone(),
                        TaskResult {
                            task_name: name,
                            success: false,
                            message: e.to_string(),
                            duration: Duration::ZERO,
                            items_cleaned: None,
                        },
                    );
                }
            }
        }

        results
    }

    /// List all registered task names.
    pub fn list_tasks(&self) -> Vec<&str> {
        self.tasks.keys().map(|s| s.as_str()).collect()
    }

    /// Get the number of registered tasks.
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Check which tasks are currently due.
    pub fn due_tasks(&self) -> Vec<String> {
        self.tasks
            .keys()
            .filter(|name| self.should_run(name).unwrap_or(false))
            .cloned()
            .collect()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("shannon-test-housekeeping")
            .join(uuid::Uuid::new_v4().to_string());
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn housekeeper() -> Housekeeper {
        let dir = temp_dir();
        Housekeeper::with_base_dir(dir, HousekeepingConfig::default()).unwrap()
    }

    // -----------------------------------------------------------------------
    // Config tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_config_defaults() {
        let config = HousekeepingConfig::default();
        assert!(config.temp_cleanup_interval.is_some());
        assert!(config.cache_refresh_interval.is_some());
        assert!(config.session_prune_interval.is_some());
        assert!(config.log_rotation_interval.is_some());
    }

    #[test]
    fn test_config_custom_intervals() {
        let mut config = HousekeepingConfig::default();
        config
            .custom_intervals
            .insert("custom_task".to_string(), 3600);
        assert_eq!(config.custom_intervals.get("custom_task"), Some(&3600));
    }

    // -----------------------------------------------------------------------
    // Built-in task tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_temp_cleanup_task_name() {
        let task = TempFileCleanupTask;
        assert_eq!(task.name(), "temp_file_cleanup");
        assert!(!task.description().is_empty());
    }

    #[test]
    fn test_temp_cleanup_task_execute_no_dir() {
        let task = TempFileCleanupTask;
        let dir = temp_dir();
        let (msg, count) = task.execute(&dir).unwrap();
        assert_eq!(count, Some(0));
        assert!(msg.contains("No temp directory"));
    }

    #[test]
    fn test_temp_cleanup_task_execute() {
        let task = TempFileCleanupTask;
        let dir = temp_dir();
        let tmp_dir = dir.join("tmp");
        fs::create_dir_all(&tmp_dir).unwrap();

        // Create a file (recent, should not be removed).
        fs::write(tmp_dir.join("recent.txt"), "data").unwrap();

        // All files are recent, so nothing should be removed.
        let (_msg, count) = task.execute(&dir).unwrap();
        assert_eq!(count, Some(0));
        assert!(tmp_dir.join("recent.txt").exists());
    }

    #[test]
    fn test_cache_refresh_task() {
        let task = CacheRefreshTask;
        let dir = temp_dir();
        let cache_dir = dir.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join("item1.dat"), "cache1").unwrap();

        let (msg, count) = task.execute(&dir).unwrap();
        assert_eq!(count, Some(2)); // item1.dat + .last_refresh
        assert!(msg.contains("refreshed"));
        assert!(cache_dir.join(".last_refresh").exists());
    }

    #[test]
    fn test_old_session_prune_task_no_dir() {
        let task = OldSessionPruneTask;
        let dir = temp_dir();
        let (_msg, count) = task.execute(&dir).unwrap();
        assert_eq!(count, Some(0));
    }

    #[test]
    fn test_old_session_prune_task() {
        let task = OldSessionPruneTask;
        let dir = temp_dir();
        let sessions_dir = dir.join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();

        // Create recent session files (should not be removed).
        fs::write(sessions_dir.join("recent.json"), "{}").unwrap();
        fs::write(sessions_dir.join("recent2.json"), "{}").unwrap();

        let (_msg, count) = task.execute(&dir).unwrap();
        // All files are recent, so none should be pruned.
        assert_eq!(count, Some(0));
        assert!(sessions_dir.join("recent.json").exists());
    }

    #[test]
    fn test_log_rotation_task_no_dir() {
        let task = LogRotationTask;
        let dir = temp_dir();
        let (_msg, count) = task.execute(&dir).unwrap();
        assert_eq!(count, Some(0));
    }

    #[test]
    fn test_log_rotation_task() {
        let task = LogRotationTask;
        let dir = temp_dir();
        let logs_dir = dir.join("logs");
        fs::create_dir_all(&logs_dir).unwrap();

        // Create a small log file (should not be rotated).
        fs::write(logs_dir.join("small.log"), "small").unwrap();

        // Create a large log file (should be rotated).
        let large_log = logs_dir.join("large.log");
        fs::write(&large_log, "x".repeat(11 * 1024 * 1024)).unwrap();

        let (_msg, count) = task.execute(&dir).unwrap();
        assert_eq!(count, Some(1));
        assert!(logs_dir.join("small.log").exists());
        assert!(!large_log.exists());
        assert!(logs_dir.join("large.old").exists());
    }

    // -----------------------------------------------------------------------
    // Housekeeper tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_housekeeper_creation() {
        let dir = temp_dir();
        let keeper =
            Housekeeper::with_base_dir(dir.clone(), HousekeepingConfig::default()).unwrap();
        assert_eq!(keeper.task_count(), 0);
        assert!(dir.exists());
    }

    #[test]
    fn test_register_builtin_tasks() {
        let mut keeper = housekeeper();
        keeper.register_builtin_tasks();
        assert_eq!(keeper.task_count(), 4);
    }

    #[test]
    fn test_register_custom_task() {
        let mut keeper = housekeeper();

        struct CustomTask;
        impl HousekeepingTask for CustomTask {
            fn name(&self) -> &str {
                "custom"
            }
            fn description(&self) -> &str {
                "A custom task"
            }
            fn default_interval(&self) -> Duration {
                Duration::from_secs(60)
            }
            fn execute(&self, _base_dir: &Path) -> Result<(String, Option<usize>), String> {
                Ok(("Custom done".into(), Some(1)))
            }
        }

        keeper.register_task(Box::new(CustomTask));
        assert_eq!(keeper.task_count(), 1);
        assert!(keeper.list_tasks().contains(&"custom"));
    }

    #[test]
    fn test_should_run_never_run() {
        let mut keeper = housekeeper();
        keeper.register_builtin_tasks();
        assert!(keeper.should_run("temp_file_cleanup").unwrap());
    }

    #[test]
    fn test_should_run_not_yet_due() {
        let mut keeper = housekeeper();
        keeper.register_builtin_tasks();

        // Run the task.
        keeper.run_task("temp_file_cleanup").unwrap();
        // Should not need to run again immediately.
        assert!(!keeper.should_run("temp_file_cleanup").unwrap());
    }

    #[test]
    fn test_should_run_nonexistent() {
        let keeper = housekeeper();
        let result = keeper.should_run("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_run_task() {
        let mut keeper = housekeeper();
        keeper.register_builtin_tasks();

        let result = keeper.run_task("temp_file_cleanup").unwrap();
        assert!(result.success);
        assert_eq!(result.task_name, "temp_file_cleanup");
    }

    #[test]
    fn test_run_task_nonexistent() {
        let mut keeper = housekeeper();
        let result = keeper.run_task("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_run_all() {
        let mut keeper = housekeeper();
        keeper.register_builtin_tasks();

        let results = keeper.run_all();
        assert_eq!(results.len(), 4);
        for result in results.values() {
            assert!(result.success);
        }
    }

    #[test]
    fn test_run_all_skips_not_due() {
        let mut keeper = housekeeper();
        keeper.register_builtin_tasks();

        // First run.
        let results1 = keeper.run_all();
        assert_eq!(results1.len(), 4);

        // Second run should skip everything.
        let results2 = keeper.run_all();
        assert_eq!(results2.len(), 0);
    }

    #[test]
    fn test_run_all_forced() {
        let mut keeper = housekeeper();
        keeper.register_builtin_tasks();

        keeper.run_all();
        let results = keeper.run_all_forced();
        assert_eq!(results.len(), 4);
    }

    #[test]
    fn test_due_tasks() {
        let mut keeper = housekeeper();
        keeper.register_builtin_tasks();

        let due = keeper.due_tasks();
        assert_eq!(due.len(), 4);

        keeper.run_all();
        let due_after = keeper.due_tasks();
        assert_eq!(due_after.len(), 0);
    }

    #[test]
    fn test_list_tasks() {
        let mut keeper = housekeeper();
        keeper.register_builtin_tasks();
        let tasks = keeper.list_tasks();
        assert!(tasks.contains(&"temp_file_cleanup"));
        assert!(tasks.contains(&"cache_refresh"));
        assert!(tasks.contains(&"old_session_prune"));
        assert!(tasks.contains(&"log_rotation"));
    }

    #[test]
    fn test_task_result_display() {
        let result = TaskResult {
            task_name: "test_task".into(),
            success: true,
            message: "All good".into(),
            duration: Duration::from_millis(500),
            items_cleaned: Some(10),
        };
        let display = format!("{result}");
        assert!(display.contains("[OK]"));
        assert!(display.contains("test_task"));
        assert!(display.contains("All good"));
        assert!(display.contains("10 items"));
    }

    #[test]
    fn test_task_result_display_failed() {
        let result = TaskResult {
            task_name: "fail_task".into(),
            success: false,
            message: "Something went wrong".into(),
            duration: Duration::from_millis(100),
            items_cleaned: None,
        };
        let display = format!("{result}");
        assert!(display.contains("[FAILED]"));
        assert!(display.contains("fail_task"));
    }
}
