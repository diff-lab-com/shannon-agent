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
use std::time::{Duration, SystemTime};

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
    /// Interval for session-log retention (GC over `sessions/`).
    #[serde(default)]
    pub session_retention_interval: Option<Duration>,
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
            session_retention_interval: Some(Duration::from_secs(24 * 60 * 60)), // 24 hours
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

/// Old session pruning task — the env-gated form of the archived-aware
/// session GC.
///
/// **Opt-in and disabled by default** (adversarial review F10/F13): the
/// desktop config key `session_gc_enabled` defaults to `false` and the
/// core-side mirror (`SHANNON_SESSION_GC_ENABLED`, see
/// `session_gc_enabled`) is unset unless explicitly exported, so a
/// registration alone never deletes anything.
///
/// Since Task 2 (archive MVP, 卡A) the policy this task stands for is
/// "only **archived** sessions past a retention window are prunable" —
/// see [`prune_archived_sessions`]. This env-gated core form has no
/// retention-window source (the window lives in the desktop config key
/// `session_retention_days`, default `None` = never delete), so it
/// delegates with `None` and **deletes nothing**; it exists so the task
/// registry keeps answering honestly ("enabled, but no retention window
/// configured — zero deletions"). The only path that ever deletes is the
/// desktop wiring, which passes the user's configured window.
pub struct OldSessionPruneTask;

impl HousekeepingTask for OldSessionPruneTask {
    fn name(&self) -> &str {
        "old_session_prune"
    }

    fn description(&self) -> &str {
        "Remove archived session directories past the retention window \
         (opt-in via session_gc_enabled; window via desktop config, absent here)"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(24 * 60 * 60) // 24 hours
    }

    fn execute(&self, base_dir: &Path) -> Result<(String, Option<usize>), String> {
        prune_old_sessions(base_dir, session_gc_enabled(), SystemTime::now())
    }
}

/// Session-GC master gate: the core-side mirror of the desktop config key
/// `session_gc_enabled` (serde default `false`). Core cannot read the
/// desktop's `config.json`, so the opt-in crosses the crate boundary as an
/// environment variable, `SHANNON_SESSION_GC_ENABLED=1|true` — the same
/// pattern [`SessionRetentionConfig::from_env`] uses for its policy knobs.
/// Unset or any other value means **disabled**: nothing is ever deleted.
fn session_gc_enabled() -> bool {
    std::env::var("SHANNON_SESSION_GC_ENABLED")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Fail-closed prune decision for one session: prune only when its age is
/// **known** (`Some`) and past the cutoff. `None` — a stat failure means the
/// age is unknown — always keeps the session: a destructive path must never
/// guess (an epoch-0 fallback would make every unreadable session look
/// infinitely old and delete it).
fn prunable_since(mtime: Option<SystemTime>, cutoff: SystemTime) -> bool {
    matches!(mtime, Some(m) if m < cutoff)
}

/// Outcome of one archived-session GC pass ([`prune_archived_sessions`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArchivedPruneReport {
    /// Session ids whose directories were removed (deletion order).
    pub deleted_session_ids: Vec<String>,
    /// Sessions that were archived and old enough but whose age could not
    /// be read (stat failure). Always skipped — a destructive path never
    /// guesses.
    pub skipped_unknown_age: usize,
    /// Sessions skipped because their curation sidecar does not mark them
    /// archived (reported for observability; never deleted by this pass).
    pub not_archived: usize,
}

/// Archived-aware session GC over one sessions **container**
/// (`<container>/<uuid>/` directories — the Task 2 / 卡A policy).
///
/// Policy: a session directory is removed only when **all** of these hold
/// —
/// 1. GC itself is enabled (the caller's gate; the desktop wiring requires
///    the desktop config `session_gc_enabled == true` and lets the
///    `SHANNON_SESSION_GC_ENABLED` env var only *force-disable* — an env
///    var can never switch deletion on);
/// 2. the session's curation sidecar (`<id>/curation.json`) marks it
///    `archived` (a missing/unparsable sidecar reads as not archived, so
///    nothing disappears before an explicit user action);
/// 3. a retention window was configured (`retention_days = Some`) —
///    `None` means *never delete*, so the default configuration performs
///    zero deletions even with GC enabled;
/// 4. the session's age is **known** (its `events.jsonl` mtime reads) and
///    older than the window — fail-closed via [`prunable_since`]: a stat
///    failure skips the session, it is never guessed old.
///
/// Only UUID-named directories are considered (foreign siblings are never
/// GC targets) and removals are whole-directory `remove_dir_all`s. `now`
/// is injected for tests.
pub fn prune_archived_sessions(
    sessions_dir: &Path,
    retention_days: Option<u32>,
    now: SystemTime,
) -> Result<ArchivedPruneReport, String> {
    let mut report = ArchivedPruneReport::default();
    // No window configured → the default "never auto-delete" posture:
    // nothing is prunable, however old or however archived.
    let Some(days) = retention_days else {
        return Ok(report);
    };
    if !sessions_dir.exists() {
        return Ok(report);
    }
    // `days * 86400` cannot overflow u64 for any u32, but `checked_sub`
    // still can (a window longer than the time since the epoch is not a
    // representable cutoff). Fail closed: an unrepresentable window deletes
    // nothing (falling back to `now` or the epoch would make everything or
    // nothing look past-retention by accident).
    let Some(cutoff) = now.checked_sub(Duration::from_secs(u64::from(days) * 24 * 60 * 60)) else {
        return Ok(report);
    };

    let store = crate::session_log::SessionStore::new(sessions_dir);
    for entry in crate::session_log::scan_session_summaries(sessions_dir) {
        let Ok(id) = uuid::Uuid::parse_str(&entry.session_id) else {
            continue; // foreign directories sharing the container
        };
        if !store.curation(&id).archived {
            report.not_archived += 1;
            continue;
        }
        let mtime = entry.events_path.metadata().and_then(|m| m.modified()).ok();
        if !prunable_since(mtime, cutoff) {
            if mtime.is_none() {
                report.skipped_unknown_age += 1;
                warn!(
                    session_id = %entry.session_id,
                    events = %entry.events_path.display(),
                    "archived session GC: events.jsonl mtime unknown; skipping session (fail closed)"
                );
            }
            continue;
        }
        let Some(dir) = entry.events_path.parent() else {
            continue;
        };
        match std::fs::remove_dir_all(dir) {
            Ok(()) => {
                info!(
                    session_id = %entry.session_id,
                    dir = %dir.display(),
                    retention_days = days,
                    "archived session GC: removed archived session directory"
                );
                report.deleted_session_ids.push(entry.session_id);
            }
            Err(e) => {
                warn!(
                    session_id = %entry.session_id,
                    dir = %dir.display(),
                    error = %e,
                    "archived session GC: failed to remove session directory"
                );
            }
        }
    }
    Ok(report)
}

/// Env-gated form of the archived-session GC ([`OldSessionPruneTask`]).
///
/// `enabled` mirrors the desktop `session_gc_enabled` config key (default
/// `false`): when false, nothing is touched. The core crate has no access
/// to the desktop's `session_retention_days` config, so this form runs the
/// policy with **no retention window** — zero deletions by construction.
/// The deleting path is [`prune_archived_sessions`], driven by the desktop
/// wiring with the user's configured window.
pub fn prune_old_sessions(
    base_dir: &Path,
    enabled: bool,
    now: SystemTime,
) -> Result<(String, Option<usize>), String> {
    if !enabled {
        return Ok((
            "Session GC disabled (session_gc_enabled=false); nothing pruned".to_string(),
            Some(0),
        ));
    }
    let sessions_dir = base_dir.join("sessions");
    let report = prune_archived_sessions(&sessions_dir, None, now)?;
    Ok((
        format!(
            "Session GC enabled but no retention window configured (core form has no \
             session_retention_days source); nothing pruned. Not archived: {}; \
             unknown age: {}",
            report.not_archived, report.skipped_unknown_age
        ),
        Some(0),
    ))
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
// Session Log Retention (GC over the `sessions/` container)
// ============================================================================

/// One GiB, the unit of `SHANNON_SESSION_RETENTION_MAX_GB`.
const GIB: u64 = 1024 * 1024 * 1024;

/// Retention policy for stored session logs
/// ([`SessionLogRetentionTask`]).
///
/// Defaults: 30 days, 5 GiB — overridable via the
/// `SHANNON_SESSION_RETENTION_DAYS` / `SHANNON_SESSION_RETENTION_MAX_GB`
/// environment variables.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRetentionConfig {
    /// Sessions whose last activity is newer than this are never deleted,
    /// no matter how far over budget the container is (the conservative
    /// half of the rule).
    pub retention_days: u64,
    /// Total byte budget for the sessions container. Over budget, the
    /// oldest past-retention sessions are deleted until the projected
    /// total fits again.
    pub max_bytes: u64,
}

impl Default for SessionRetentionConfig {
    fn default() -> Self {
        Self {
            retention_days: 30,
            max_bytes: 5 * GIB,
        }
    }
}

impl SessionRetentionConfig {
    /// Resolve the policy from the environment, falling back to
    /// [`SessionRetentionConfig::default`] per unparsable/absent variable.
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(raw) = std::env::var("SHANNON_SESSION_RETENTION_DAYS") {
            match raw.trim().parse::<u64>() {
                Ok(days) => config.retention_days = days,
                Err(_) => warn!(
                    value = %raw,
                    "SHANNON_SESSION_RETENTION_DAYS is not a number; using default \
                     ({} days)",
                    config.retention_days
                ),
            }
        }
        if let Ok(raw) = std::env::var("SHANNON_SESSION_RETENTION_MAX_GB") {
            match raw.trim().parse::<f64>() {
                Ok(gb) if gb.is_finite() && gb >= 0.0 => {
                    config.max_bytes = (gb * GIB as f64) as u64;
                }
                _ => warn!(
                    value = %raw,
                    "SHANNON_SESSION_RETENTION_MAX_GB is not a number; using default \
                     ({} GiB)",
                    config.max_bytes / GIB
                ),
            }
        }
        config
    }
}

/// Per-session disk-usage snapshot feeding the retention planner.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionUsage {
    /// Session id (the directory name under the container).
    pub session_id: String,
    /// The session directory itself (`<container>/<id>/`).
    pub dir: PathBuf,
    /// Total bytes of the session's files.
    pub size_bytes: u64,
    /// Last activity: the later of the directory and `events.jsonl` mtimes.
    pub last_modified: SystemTime,
}

/// Size of one session directory. Prefers the E-9 index sidecar's `log_len`
/// when it still validates against `events.jsonl` (an authoritative total
/// without re-reading); otherwise stats each file directly. Sidecars
/// (`meta.json`, `index.json`) are tiny but counted.
fn session_dir_usage(entry: &crate::session_log::SessionScanEntry) -> u64 {
    let index_path = crate::session_log::session_index::index_path_for(&entry.events_path);
    let indexed_len =
        crate::session_log::SessionIndex::load_if_valid(&entry.events_path, &index_path)
            .map(|i| i.log_len);
    let Some(dir) = entry.events_path.parent() else {
        return 0;
    };
    let mut total = 0u64;
    for file in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(meta) = file.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let path = file.path();
        if path == entry.events_path {
            total += indexed_len.unwrap_or(meta.len());
        } else {
            total += meta.len();
        }
    }
    total
}

/// Last activity of one scanned session, fail-closed: `None` when neither
/// the `events.jsonl` mtime nor the directory mtime can be read. Callers
/// must treat `None` as "age unknown — never a deletion candidate" (an
/// epoch-0 fallback here would make every unreadable session look
/// infinitely old and delete it).
fn session_last_modified(events_path: &Path) -> Option<SystemTime> {
    let events_mtime = events_path.metadata().and_then(|m| m.modified()).ok();
    let dir_mtime = events_path
        .parent()
        .and_then(|d| d.metadata().ok())
        .and_then(|m| m.modified().ok());
    match (events_mtime, dir_mtime) {
        (Some(e), Some(d)) => Some(e.max(d)),
        (Some(e), None) => Some(e),
        (None, Some(d)) => Some(d),
        (None, None) => None,
    }
}

/// Scan a sessions container for per-session usage, oldest activity first.
///
/// Only UUID-named directories with an `events.jsonl` are considered —
/// housekeeping GC never touches foreign siblings. Sessions whose age
/// cannot be determined (both mtimes unreadable) are **omitted** rather
/// than reported with a guessed timestamp: the retention planner only sees
/// entries of this scan, so an omitted session can never be planned for
/// deletion (fail closed — see [`session_last_modified`]).
pub fn scan_session_usage(sessions_dir: &Path) -> Vec<SessionUsage> {
    let mut usage: Vec<SessionUsage> = crate::session_log::scan_session_summaries(sessions_dir)
        .into_iter()
        .filter(|entry| uuid::Uuid::parse_str(&entry.session_id).is_ok())
        .filter_map(|entry| {
            let last_modified = session_last_modified(&entry.events_path)?;
            Some(SessionUsage {
                session_id: entry.session_id.clone(),
                dir: entry
                    .events_path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .to_path_buf(),
                size_bytes: session_dir_usage(&entry),
                last_modified,
            })
        })
        .collect();
    usage.sort_by_key(|u| u.last_modified); // oldest first
    usage
}

/// Pure retention planner: which sessions to delete so the container fits
/// its budget.
///
/// Deliberately conservative — a session is deleted only when it is **both**
/// past `retention_days` since its last activity **and** the container
/// exceeds `max_bytes` (retention-days alone never deletes anything).
/// Candidates go oldest-activity-first until the projected total fits the
/// budget or the candidates run out. Returns the deletions in that order.
pub fn plan_session_retention(
    usage: &[SessionUsage],
    config: &SessionRetentionConfig,
    now: SystemTime,
) -> Vec<SessionUsage> {
    let total: u64 = usage.iter().map(|u| u.size_bytes).sum();
    if total <= config.max_bytes {
        return Vec::new();
    }
    let day_secs = config
        .retention_days
        .checked_mul(24 * 60 * 60)
        .unwrap_or(u64::MAX);
    // Fail closed on cutoff arithmetic: `checked_sub` only fails when the
    // window is longer than the time since the epoch (a nonsensical
    // config), and the old epoch-0 fallback there made EVERY session look
    // past-retention. An uncomputable cutoff deletes nothing.
    let Some(cutoff) = now.checked_sub(Duration::from_secs(day_secs)) else {
        return Vec::new();
    };
    // Oldest activity first, regardless of the caller's input order.
    let mut candidates: Vec<&SessionUsage> =
        usage.iter().filter(|u| u.last_modified < cutoff).collect();
    candidates.sort_by_key(|u| u.last_modified);
    let mut projected = total;
    let mut deletions = Vec::new();
    for session in candidates {
        if projected <= config.max_bytes {
            break;
        }
        projected = projected.saturating_sub(session.size_bytes);
        deletions.push(session.clone());
    }
    deletions
}

/// Human-readable byte size for task messages ("8.00 MiB", "1.50 GiB").
fn human_bytes(bytes: u64) -> String {
    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024 * 1024) as f64)
    } else if bytes >= 1024 {
        format!("{:.2} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Delete whole session directories. Returns `(removed, bytes_freed)` —
/// every successful deletion is logged individually (id + bytes) and the
/// totals are logged by the caller.
fn delete_session_dirs(deletions: &[SessionUsage]) -> (usize, u64) {
    let mut removed = 0usize;
    let mut freed = 0u64;
    for deletion in deletions {
        match std::fs::remove_dir_all(&deletion.dir) {
            Ok(()) => {
                removed += 1;
                freed += deletion.size_bytes;
                info!(
                    session_id = %deletion.session_id,
                    bytes = deletion.size_bytes,
                    "session log retention: deleted session directory"
                );
            }
            Err(e) => {
                warn!(
                    session_id = %deletion.session_id,
                    dir = %deletion.dir.display(),
                    error = %e,
                    "session log retention: failed to delete session directory"
                );
            }
        }
    }
    (removed, freed)
}

/// Session log GC: enforces the session retention policy over the sessions
/// container (`<base>/sessions`).
///
/// Policy comes from `SHANNON_SESSION_RETENTION_DAYS` (default 30) and
/// `SHANNON_SESSION_RETENTION_MAX_GB` (default 5), unless overridden with
/// [`SessionLogRetentionTask::with_config`] (tests, embedders).
///
/// Housekeeping runs offline — no caller of this module holds an
/// active-session handle to exempt, so planning exempts nothing. The
/// retention-days gate is what keeps in-use data safe: a session younger
/// than the window is never a deletion candidate, and over-budget deletion
/// stops at recent sessions.
pub struct SessionLogRetentionTask {
    override_config: Option<SessionRetentionConfig>,
}

impl SessionLogRetentionTask {
    /// Environment-driven configuration.
    pub fn new() -> Self {
        Self {
            override_config: None,
        }
    }

    /// Pin the policy explicitly (tests, embedders).
    pub fn with_config(config: SessionRetentionConfig) -> Self {
        Self {
            override_config: Some(config),
        }
    }

    fn effective_config(&self) -> SessionRetentionConfig {
        self.override_config
            .clone()
            .unwrap_or_else(SessionRetentionConfig::from_env)
    }
}

impl Default for SessionLogRetentionTask {
    fn default() -> Self {
        Self::new()
    }
}

impl HousekeepingTask for SessionLogRetentionTask {
    fn name(&self) -> &str {
        "session_log_retention"
    }

    fn description(&self) -> &str {
        "Delete whole session directories older than the retention window, \
         oldest first, only while the sessions container exceeds its size budget"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(24 * 60 * 60) // 24 hours
    }

    fn execute(&self, base_dir: &Path) -> Result<(String, Option<usize>), String> {
        let config = self.effective_config();
        let sessions_dir = base_dir.join("sessions");
        if !sessions_dir.exists() {
            return Ok(("No sessions directory found".to_string(), Some(0)));
        }

        let usage = scan_session_usage(&sessions_dir);
        let total: u64 = usage.iter().map(|u| u.size_bytes).sum();
        let deletions = plan_session_retention(&usage, &config, std::time::SystemTime::now());

        if deletions.is_empty() {
            let msg = format!(
                "Session logs within policy: {} session(s), {} / {} budget",
                usage.len(),
                human_bytes(total),
                human_bytes(config.max_bytes),
            );
            return Ok((msg, Some(0)));
        }

        let (removed, freed) = delete_session_dirs(&deletions);
        info!(
            deleted_sessions = removed,
            bytes_freed = freed,
            retention_days = config.retention_days,
            budget_bytes = config.max_bytes,
            container_bytes_before = total,
            "session log retention: pruned old session logs"
        );
        let msg = format!(
            "Deleted {removed} old session(s), freed {} (container was {} / {} budget)",
            human_bytes(freed),
            human_bytes(total),
            human_bytes(config.max_bytes),
        );
        Ok((msg, Some(removed)))
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
        self.register_task(Box::new(SessionLogRetentionTask::new()));
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
            "session_log_retention" => self.config.session_retention_interval.unwrap_or(default),
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

    // -----------------------------------------------------------------------
    // Session GC (prune_old_sessions over the real per-session layout)
    // -----------------------------------------------------------------------

    /// The task handle itself is opt-in-disabled (`SHANNON_SESSION_GC_ENABLED`
    /// unset in tests): a 60-day-old real session directory survives the
    /// daily run untouched. Supersedes the retired flat `*.json` fixtures.
    #[test]
    fn old_session_prune_task_is_disabled_by_default_over_real_layout() {
        let task = OldSessionPruneTask;
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        fs::create_dir_all(&container).unwrap();
        let old = uuid::Uuid::new_v4();
        seed_real_session(&container, &old, 1024, 60 * 24 * 3600);

        let (msg, count) = task.execute(tmp.path()).unwrap();
        assert_eq!(count, Some(0), "{msg}");
        assert!(msg.contains("disabled"), "{msg}");
        assert!(
            container.join(old.to_string()).exists(),
            "GC off → the real session directory is never touched"
        );
    }

    #[test]
    fn prune_old_sessions_disabled_touches_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        fs::create_dir_all(&container).unwrap();
        let old = uuid::Uuid::new_v4();
        seed_real_session(&container, &old, 1024, 60 * 24 * 3600);

        let (msg, count) = prune_old_sessions(tmp.path(), false, SystemTime::now()).unwrap();
        assert_eq!(count, Some(0), "{msg}");
        assert!(msg.contains("disabled"), "{msg}");
        assert!(
            container.join(old.to_string()).exists(),
            "GC off → nothing is deleted, however old"
        );
    }

    #[test]
    fn prune_old_sessions_enabled_without_window_deletes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        fs::create_dir_all(&container).unwrap();
        let old = uuid::Uuid::new_v4();
        seed_real_session(&container, &old, 1024, 60 * 24 * 3600);

        // The env-gated core form has no retention-window source (the
        // window lives in the desktop config), so even enabled it must
        // never delete — the desktop wiring is the only deleting path.
        let (msg, count) = prune_old_sessions(tmp.path(), true, SystemTime::now()).unwrap();
        assert_eq!(count, Some(0), "{msg}");
        assert!(msg.contains("no retention window"), "{msg}");
        assert!(
            container.join(old.to_string()).exists(),
            "no window configured → the real session directory is never touched"
        );
    }

    /// The destructive path must fail closed: `None` (stat failure) means
    /// "keep" — an epoch fallback would make every unreadable session look
    /// infinitely old and delete it.
    #[test]
    fn prunable_since_fails_closed_on_unknown_age() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000_000);
        let cutoff = now - Duration::from_secs(30 * 24 * 60 * 60);
        // Unknown age → never prunable.
        assert!(!prunable_since(None, cutoff));
        // Known ages decide normally.
        let ancient = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        let recent = now - Duration::from_secs(60);
        assert!(prunable_since(Some(ancient), cutoff));
        assert!(!prunable_since(Some(recent), cutoff));
    }

    /// Mark an existing real session directory archived through the real
    /// curation sidecar (`<id>/curation.json`) — the same file the desktop
    /// archive command writes.
    fn archive_session_dir(container: &Path, id: &uuid::Uuid) {
        let curation = crate::session_log::session_curation_path(container, &id.to_string());
        std::fs::write(&curation, r#"{"archived":true}"#).unwrap();
    }

    /// Task 2 (卡A) policy boundaries: only **archived AND past-retention**
    /// sessions are pruned; unarchived and recent sessions survive, foreign
    /// directories are never touched, and the report names what it did.
    #[test]
    fn prune_archived_sessions_removes_only_archived_past_retention() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        fs::create_dir_all(&container).unwrap();

        let old_archived = uuid::Uuid::new_v4();
        let old_active = uuid::Uuid::new_v4();
        let recent_archived = uuid::Uuid::new_v4();
        seed_real_session(&container, &old_archived, 512, 40 * 24 * 3600);
        seed_real_session(&container, &old_active, 512, 40 * 24 * 3600);
        seed_real_session(&container, &recent_archived, 512, 0);
        archive_session_dir(&container, &old_archived);
        archive_session_dir(&container, &recent_archived);
        // A foreign (non-UUID) sibling must survive even though it is old.
        let foreign = container.join("not-a-uuid");
        fs::create_dir_all(&foreign).unwrap();
        fs::write(foreign.join("events.jsonl"), "x").unwrap();

        let report = prune_archived_sessions(&container, Some(30), SystemTime::now()).unwrap();
        assert_eq!(
            report.deleted_session_ids,
            vec![old_archived.to_string()],
            "exactly the archived-and-old session is deleted"
        );
        assert_eq!(report.not_archived, 1, "the unarchived old session");
        assert_eq!(report.skipped_unknown_age, 0);
        assert!(!container.join(old_archived.to_string()).exists());
        assert!(
            container.join(old_active.to_string()).exists(),
            "an unarchived session is never auto-deleted, however old"
        );
        assert!(
            container.join(recent_archived.to_string()).exists(),
            "an archived session inside the retention window is kept"
        );
        assert!(foreign.exists(), "foreign directories are never GC targets");
    }

    /// `retention_days = None` (the config default) means **never delete**:
    /// even archived sessions past any plausible window survive while GC
    /// itself is enabled.
    #[test]
    fn prune_archived_sessions_retention_none_deletes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        fs::create_dir_all(&container).unwrap();
        let ancient = uuid::Uuid::new_v4();
        seed_real_session(&container, &ancient, 512, 400 * 24 * 3600);
        archive_session_dir(&container, &ancient);

        let report = prune_archived_sessions(&container, None, SystemTime::now()).unwrap();
        assert!(report.deleted_session_ids.is_empty());
        assert!(
            container.join(ancient.to_string()).exists(),
            "retention None → zero deletions even with GC enabled"
        );
    }

    /// A retention window so large its cutoff is unrepresentable deletes
    /// nothing (fail closed) instead of accidentally making every session
    /// look past-retention.
    #[test]
    fn prune_archived_sessions_unrepresentable_window_deletes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        fs::create_dir_all(&container).unwrap();
        let ancient = uuid::Uuid::new_v4();
        seed_real_session(&container, &ancient, 512, 1000 * 24 * 3600);
        archive_session_dir(&container, &ancient);

        let report =
            prune_archived_sessions(&container, Some(u32::MAX), SystemTime::now()).unwrap();
        assert!(report.deleted_session_ids.is_empty());
        assert!(container.join(ancient.to_string()).exists());
    }

    #[test]
    fn prune_archived_sessions_noop_when_container_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let report =
            prune_archived_sessions(&tmp.path().join("sessions"), Some(30), SystemTime::now())
                .unwrap();
        assert!(report.deleted_session_ids.is_empty());
    }

    /// Unreadable sessions are never deleted: a session directory whose
    /// contents cannot be stat'd never yields a known age, and both the
    /// scan (omits it, see [`session_last_modified`]) and the archived-GC
    /// decision ([`prunable_since`] on `None`) fail closed. Root ignores
    /// directory permission bits, so the chmod simulation is skipped when
    /// the stat still succeeds.
    #[test]
    fn unreadable_session_dirs_are_never_deleted_by_archived_gc() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        let id = uuid::Uuid::new_v4();
        seed_real_session(&container, &id, 512, 400 * 24 * 3600);
        archive_session_dir(&container, &id);

        let dir = container.join(id.to_string());
        use std::os::unix::fs::PermissionsExt;
        let orig = fs::metadata(&dir).unwrap().permissions();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::metadata(dir.join("events.jsonl")).is_ok() {
            // Root (or an ACL override): the simulation is impossible here.
            fs::set_permissions(&dir, orig).unwrap();
            return;
        }
        let report = prune_archived_sessions(&container, Some(30), SystemTime::now()).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(report.deleted_session_ids.is_empty(), "{report:?}");
        assert!(
            dir.exists(),
            "a session with no readable age is never deleted"
        );
    }

    /// `scan_session_usage` omits sessions whose age cannot be read, so the
    /// budget planner can never even see them as candidates (fail closed).
    #[test]
    fn session_last_modified_is_none_for_unreadable_paths() {
        // Neither the file nor its parent dir exists → both stat attempts
        // fail → age unknown.
        let missing_root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        assert!(session_last_modified(&missing_root.join("events.jsonl")).is_none());
        // A readable file still resolves (the common case).
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("events.jsonl"), b"x").unwrap();
        assert!(session_last_modified(&tmp.path().join("events.jsonl")).is_some());
    }

    /// A retention window whose cutoff arithmetic overflows (larger than
    /// the time since the epoch) deletes nothing instead of falling back to
    /// an epoch-0 cutoff that would make everything eligible.
    #[test]
    fn plan_session_retention_overflow_cutoff_deletes_nothing() {
        let usage = vec![fixture_usage(
            "ancient",
            4 * GIB,
            1, // ~1970-01-01: older than any sane window
        )];
        let config = SessionRetentionConfig {
            retention_days: u64::MAX,
            max_bytes: GIB,
        };
        let plan = plan_session_retention(&usage, &config, plan_now());
        assert!(
            plan.is_empty(),
            "an unrepresentable cutoff must fail closed"
        );
    }

    #[test]
    fn prune_old_sessions_noop_when_no_sessions_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // Enabled without a container: still zero deletions, honest message.
        let (msg, count) = prune_old_sessions(tmp.path(), true, SystemTime::now()).unwrap();
        assert_eq!(count, Some(0));
        assert!(msg.contains("no retention window"), "{msg}");
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
        assert_eq!(keeper.task_count(), 5);
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
        assert_eq!(results.len(), 5);
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
        assert_eq!(results1.len(), 5);

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
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn test_due_tasks() {
        let mut keeper = housekeeper();
        keeper.register_builtin_tasks();

        let due = keeper.due_tasks();
        assert_eq!(due.len(), 5);

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
        assert!(tasks.contains(&"session_log_retention"));
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

    // -----------------------------------------------------------------------
    // Session log retention (session log GC)
    // -----------------------------------------------------------------------

    /// One MiB.
    const MIB: u64 = 1024 * 1024;
    /// Fixed "now" for planner tests: some instant after every fixture time.
    const PLAN_NOW_SECS: u64 = 10_000_000;

    fn fixture_usage(id: &str, size_bytes: u64, last_modified_secs: u64) -> SessionUsage {
        SessionUsage {
            session_id: id.to_string(),
            dir: PathBuf::from(format!("/nonexistent/{id}")),
            size_bytes: size_bytes,
            last_modified: SystemTime::UNIX_EPOCH + Duration::from_secs(last_modified_secs),
        }
    }

    fn plan_config(days: u64, max_gib: u64) -> SessionRetentionConfig {
        SessionRetentionConfig {
            retention_days: days,
            max_bytes: max_gib * GIB,
        }
    }

    fn plan_now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(PLAN_NOW_SECS)
    }

    fn days_before_now(days: u64) -> u64 {
        PLAN_NOW_SECS.saturating_sub(days * 24 * 60 * 60)
    }

    #[test]
    fn plan_deletes_oldest_past_retention_first_until_under_budget() {
        // Container: 3 GiB old (40d) + 2 GiB mid (35d) + 1 GiB recent (1d);
        // budget 4 GiB → over by 2 GiB. Only the two old sessions are
        // eligible; deleting the oldest (3 GiB) alone restores the budget,
        // so the mid one must survive.
        let usage = vec![
            fixture_usage("recent", 1 * GIB, days_before_now(1)),
            fixture_usage("mid", 2 * GIB, days_before_now(35)),
            fixture_usage("oldest", 3 * GIB, days_before_now(40)),
        ];
        let plan = plan_session_retention(&usage, &plan_config(30, 4), plan_now());
        let ids: Vec<&str> = plan.iter().map(|u| u.session_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["oldest"],
            "single oldest-first deletion that fits"
        );
    }

    #[test]
    fn plan_deletes_multiple_oldest_first_when_needed() {
        let usage = vec![
            fixture_usage("recent", GIB, days_before_now(1)),
            fixture_usage("old-b", 2 * GIB, days_before_now(31)),
            fixture_usage("old-a", 3 * GIB, days_before_now(45)),
        ];
        // Budget 2 GiB: 6 GiB total → even after old-a (3 GiB) + old-b
        // (2 GiB) the projected 1 GiB fits; both old sessions go, oldest first.
        let plan = plan_session_retention(&usage, &plan_config(30, 2), plan_now());
        let ids: Vec<&str> = plan.iter().map(|u| u.session_id.as_str()).collect();
        assert_eq!(ids, vec!["old-a", "old-b"]);
    }

    #[test]
    fn plan_under_budget_deletes_nothing_even_if_ancient() {
        let usage = vec![
            fixture_usage("ancient", GIB, days_before_now(400)),
            fixture_usage("old", 2 * GIB, days_before_now(60)),
        ];
        // 3 GiB total ≤ 4 GiB budget: retention-days alone deletes nothing.
        let plan = plan_session_retention(&usage, &plan_config(30, 4), plan_now());
        assert!(plan.is_empty(), "size-budget-driven: nothing over budget");
    }

    #[test]
    fn plan_keeps_recent_sessions_even_when_over_budget() {
        let usage = vec![
            fixture_usage("yesterday", 3 * GIB, days_before_now(1)),
            fixture_usage("week-ago", 3 * GIB, days_before_now(7)),
            fixture_usage("edge", 1 * GIB, days_before_now(29)),
        ];
        // 7 GiB over a 4 GiB budget, but every session is inside the 30-day
        // window — the conservative rule keeps all of them.
        let plan = plan_session_retention(&usage, &plan_config(30, 4), plan_now());
        assert!(plan.is_empty(), "recent sessions are never candidates");
    }

    #[test]
    fn plan_zero_retention_days_makes_everything_eligible() {
        let usage = vec![
            fixture_usage("newest", GIB, PLAN_NOW_SECS - 60),
            fixture_usage("older", 4 * GIB, days_before_now(2)),
        ];
        let plan = plan_session_retention(&usage, &plan_config(0, 1), plan_now());
        let ids: Vec<&str> = plan.iter().map(|u| u.session_id.as_str()).collect();
        assert_eq!(ids, vec!["older"], "newest survives; budget restored");
    }

    /// Build a real session directory under `container` with an
    /// `events.jsonl` of `size_bytes` and a last-modified stamp `age_secs`
    /// in the past (relative to the real clock).
    fn seed_real_session(
        container: &Path,
        id: &uuid::Uuid,
        size_bytes: u64,
        age_secs: u64,
    ) -> PathBuf {
        let dir = container.join(id.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("events.jsonl");
        std::fs::write(&log, vec![b'x'; size_bytes as usize]).unwrap();
        // Backdate both the file and the directory (File::set_modified works
        // through a read-only handle; the write happened just above).
        let old = std::time::SystemTime::now() - Duration::from_secs(age_secs);
        std::fs::File::open(&log)
            .unwrap()
            .set_modified(old)
            .unwrap();
        std::fs::File::open(&dir)
            .unwrap()
            .set_modified(old)
            .unwrap();
        dir
    }

    #[test]
    fn scan_session_usage_sizes_and_orders_oldest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        std::fs::create_dir_all(&container).unwrap();
        let small_id = uuid::Uuid::new_v4();
        let big_id = uuid::Uuid::new_v4();
        seed_real_session(&container, &small_id, 2 * MIB, 40 * 24 * 3600);
        std::thread::sleep(std::time::Duration::from_millis(20));
        seed_real_session(&container, &big_id, 5 * MIB, 5);

        let usage = scan_session_usage(&container);
        assert_eq!(usage.len(), 2);
        assert_eq!(usage[0].session_id, small_id.to_string(), "oldest first");
        assert_eq!(usage[0].size_bytes, 2 * MIB);
        assert_eq!(usage[1].size_bytes, 5 * MIB);
        assert_eq!(usage[1].dir, container.join(big_id.to_string()));
    }

    #[test]
    fn retention_task_deletes_old_oversized_sessions_and_reports_freed_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        std::fs::create_dir_all(&container).unwrap();
        let old_id = uuid::Uuid::new_v4();
        let recent_id = uuid::Uuid::new_v4();
        seed_real_session(&container, &old_id, 8 * MIB, 60 * 24 * 3600);
        seed_real_session(&container, &recent_id, 8 * MIB, 0);

        // Budget 12 MiB, retention 30 days: total 16 MiB is over budget and
        // only the old session is past retention → exactly it is deleted.
        let task = SessionLogRetentionTask::with_config(SessionRetentionConfig {
            retention_days: 30,
            max_bytes: 12 * MIB,
        });
        let (msg, removed) = task.execute(tmp.path()).unwrap();
        assert_eq!(removed, Some(1), "message: {msg}");
        assert!(!container.join(old_id.to_string()).exists());
        assert!(
            container.join(recent_id.to_string()).exists(),
            "recent kept"
        );
        assert!(
            msg.contains("1 old session") && msg.contains("8.00 MiB"),
            "{msg}"
        );

        // A second run is now within budget: nothing further is deleted.
        let (_, removed_again) = task.execute(tmp.path()).unwrap();
        assert_eq!(removed_again, Some(0));
    }

    #[test]
    fn retention_task_keeps_recent_sessions_when_container_is_oversized() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        std::fs::create_dir_all(&container).unwrap();
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        seed_real_session(&container, &a, 6 * MIB, 2 * 24 * 3600);
        seed_real_session(&container, &b, 6 * MIB, 1 * 24 * 3600);

        // Budget 4 MiB, retention 30 days: over budget but everything is
        // recent — the conservative rule deletes nothing.
        let task = SessionLogRetentionTask::with_config(SessionRetentionConfig {
            retention_days: 30,
            max_bytes: 4 * MIB,
        });
        let (msg, removed) = task.execute(tmp.path()).unwrap();
        assert_eq!(removed, Some(0), "{msg}");
        assert!(container.join(a.to_string()).exists());
        assert!(container.join(b.to_string()).exists());
        assert!(msg.contains("within policy"), "{msg}");
    }

    #[test]
    fn retention_task_noop_when_no_sessions_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let task = SessionLogRetentionTask::with_config(SessionRetentionConfig::default());
        let (msg, removed) = task.execute(tmp.path()).unwrap();
        assert_eq!(removed, Some(0));
        assert!(msg.contains("No sessions directory"));
    }

    #[test]
    fn retention_config_defaults_match_spec() {
        let config = SessionRetentionConfig::default();
        assert_eq!(config.retention_days, 30);
        assert_eq!(config.max_bytes, 5 * GIB);
    }
}
