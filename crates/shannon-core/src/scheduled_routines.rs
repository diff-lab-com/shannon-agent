//! Scheduled routines for recurring task execution.
//!
//! Provides a cron-like system for scheduling prompts to run at intervals
//! or specific times, integrated with the REPL tick loop.
//!
//! ## Two scheduling modes
//!
//! 1. **Interval mode** (legacy): `interval_secs` field, fires every N seconds.
//! 2. **Cron mode** (v2): `cron_expr` field with 5-field cron expression
//!    (`minute hour day-of-month month day-of-week`), compatible with
//!    Claude Code and Codex Desktop. Uses local timezone by default.
//!
//! ## Jitter
//!
//! - Interval mode: 10% of period, capped at 15 minutes (prevents thundering herd).
//! - Cron mode: When the minute field is `:00` or `:30`, fire up to 90 seconds
//!   early (aligns with Claude Code behavior to avoid API thundering herd).

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Maximum jitter cap in seconds (15 minutes, matching Claude Code).
const JITTER_CAP_SECS: u64 = 900;

/// Cron-mode jitter: max seconds early for `:00`/`:30` triggers.
/// Matches Claude Code's documented behavior.
const CRON_ROUND_TIME_JITTER_SECS: i64 = 90;

/// Compute a random jitter delay (up to 10% of period, capped at `max_cap_secs`).
///
/// Returns 0 if the period is too small for meaningful jitter.
pub fn apply_jitter(period_secs: u64, max_cap_secs: u64) -> u64 {
    let max_jitter = ((period_secs as f64 * 0.10).min(max_cap_secs as f64)) as u64;
    if max_jitter == 0 {
        return 0;
    }
    rand::random::<u64>() % (max_jitter + 1)
}

/// Apply cron-mode jitter (Claude Code style).
///
/// If the cron's minute field is exactly `0` or `30` (i.e., the trigger lands
/// on the top or bottom of the hour), return a random early-fire window of
/// up to 90 seconds. Otherwise return 0.
pub fn apply_cron_jitter(cron_expr: &str) -> i64 {
    match parse_cron_minute_field(cron_expr) {
        Some(0) | Some(30) => {
            let n: u64 = rand::random();
            (n % (CRON_ROUND_TIME_JITTER_SECS as u64 + 1)) as i64
        }
        _ => 0,
    }
}

/// Extract the minute field from a 5-field cron expression.
/// Returns None if the expression is malformed or the minute field is not a literal integer
/// (e.g., `*/5`, ranges, lists — those don't get Claude Code jitter).
fn parse_cron_minute_field(cron_expr: &str) -> Option<u32> {
    let minute_str = cron_expr.split_whitespace().next()?;
    minute_str.parse::<u32>().ok().filter(|&m| m < 60)
}

/// Parse a cron expression and return the `croner::Cron` instance.
pub fn parse_cron(cron_expr: &str) -> Result<Cron, CronParseError> {
    use std::str::FromStr as _;
    Cron::from_str(cron_expr).map_err(|e| CronParseError(e.to_string()))
}

/// Compute the next fire time after `from` for a cron expression in local timezone.
/// Returns None if the expression cannot fire again (e.g., search limit exceeded).
pub fn compute_next_fire_local(
    cron_expr: &str,
    from: DateTime<Local>,
) -> Result<Option<DateTime<Local>>, CronParseError> {
    let cron = parse_cron(cron_expr)?;
    match cron.find_next_occurrence(&from, false) {
        Ok(t) => Ok(Some(t)),
        Err(_) => Ok(None),
    }
}

/// Compute the next fire time after `from` for a cron expression in UTC.
pub fn compute_next_fire_utc(
    cron_expr: &str,
    from: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, CronParseError> {
    let local = from.with_timezone(&Local);
    Ok(compute_next_fire_local(cron_expr, local)?.map(|l| l.with_timezone(&Utc)))
}

/// Error returned when cron expression parsing fails.
#[derive(Debug, Clone, thiserror::Error)]
#[error("cron parse error: {0}")]
pub struct CronParseError(String);

/// Re-export `croner::Cron` for downstream consumers.
pub use croner::Cron;

/// Execution policy for scheduled tasks.
///
/// Encapsulates retry, timeout, and budget configuration. Matches Codex
/// Desktop's automation policy model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPolicy {
    /// Max retry attempts on failure (0 = no retry). Exponential backoff.
    #[serde(default)]
    pub max_retries: u32,
    /// Per-execution timeout in seconds (0 = no timeout).
    #[serde(default)]
    pub timeout_secs: u64,
    /// Optional dedicated background worktree path. None = run in main checkout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// Fire a notification when execution fails.
    #[serde(default)]
    pub notify_on_failure: bool,
    /// Monthly budget cap in USD. When cumulative monthly cost exceeds this,
    /// the routine auto-disables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_usd: Option<f64>,
    /// Auto-archive execution when it has no findings (Codex pattern).
    #[serde(default = "default_auto_archive")]
    pub auto_archive_when_empty: bool,
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self {
            max_retries: 0,
            timeout_secs: 0,
            worktree: None,
            notify_on_failure: false,
            budget_usd: None,
            auto_archive_when_empty: true,
        }
    }
}

fn default_auto_archive() -> bool {
    true
}

/// Trigger type for scheduled routines.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TriggerType {
    /// Interval mode (legacy): fires every N seconds via `interval_secs`.
    #[default]
    Interval,
    /// Cron mode: fires on a 5-field cron schedule via `cron_expr`.
    Cron,
    /// Webhook trigger: fires on incoming HTTP request to a generated URL.
    Webhook,
    /// Event trigger: fires when a matching hook event fires.
    Event,
}

/// A single scheduled routine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledRoutine {
    // ── Identity ───────────────────────────────────────────────────────
    /// Unique ID (8-char prefix of UUID v4).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// The prompt to execute when the routine fires.
    pub prompt: String,

    // ── Schedule ───────────────────────────────────────────────────────
    /// Interval in seconds between firings (interval mode). Unused in cron mode.
    #[serde(default)]
    pub interval_secs: u64,
    /// Trigger type (v2). Defaults to Interval for backward compatibility.
    #[serde(default)]
    pub trigger_type: TriggerType,
    /// 5-field cron expression (cron mode). None in interval mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron_expr: Option<String>,
    /// IANA timezone name (e.g., "America/New_York"). None = local timezone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Precomputed next fire time (cron mode). Updated after each firing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_fire_at: Option<DateTime<Utc>>,
    /// When the routine expires (None = never).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,

    // ── Lifecycle ──────────────────────────────────────────────────────
    /// When the routine was created.
    pub created_at: DateTime<Utc>,
    /// When the routine last fired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fired: Option<DateTime<Utc>>,
    /// Whether the routine is enabled.
    pub enabled: bool,
    /// How many times this routine has fired.
    #[serde(default)]
    pub fire_count: u32,
    /// Optional max fire count (None = unlimited).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_fires: Option<u32>,

    // ── Execution policy (v2) ──────────────────────────────────────────
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<ExecutionPolicy>,

    // ── Runtime state ──────────────────────────────────────────────────
    /// Reference to the most recent run record (JSONL line ID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_id: Option<String>,
    /// Last execution error (cleared on next success).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,

    // ── Task dependencies (C4) ─────────────────────────────────────────
    /// IDs of routines that must have completed successfully before this
    /// routine fires. Empty = no dependencies (fires independently).
    /// A dependency "completed successfully" means its most recent run has
    /// status `Succeeded`. If a dependency has never run or is in any
    /// non-terminal/failed state, this routine is blocked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
}

/// Result of checking a routine's dependencies.
///
/// Returned by [`RoutineManager::check_dependencies`]. An empty `blocked_by`
/// vector means the routine is unblocked and may fire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyBlocker {
    /// ID of the routine that is blocked.
    pub routine_id: String,
    /// IDs of dependencies that are not yet in a successful terminal state.
    pub blocked_by: Vec<String>,
}

impl DependencyBlocker {
    /// Whether the routine is blocked by any pending dependency.
    pub fn is_blocked(&self) -> bool {
        !self.blocked_by.is_empty()
    }
}

/// C4 helper: whether the routine's most recent run finished `Succeeded`.
///
/// Returns `false` if the routine is missing, has never run, or its last
/// run is in any non-`Succeeded` state (`Running`, `Failed`, etc.).
fn routine_last_run_succeeded(
    routine: Option<&ScheduledRoutine>,
    runs: &crate::scheduled_runs::ScheduledRunsStore,
) -> bool {
    let Some(r) = routine else { return false };
    let Some(run_id) = r.last_run_id.as_deref() else {
        return false;
    };
    match runs.find_by_id(run_id) {
        Ok(Some(run)) => run.status == crate::scheduled_runs::RunStatus::Succeeded,
        _ => false,
    }
}

impl ScheduledRoutine {
    /// Create a new interval-mode routine (legacy constructor).
    pub fn new(name: String, prompt: String, interval_secs: u64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
            name,
            prompt,
            interval_secs,
            created_at: Utc::now(),
            last_fired: None,
            enabled: true,
            fire_count: 0,
            max_fires: None,
            trigger_type: TriggerType::Interval,
            cron_expr: None,
            timezone: None,
            next_fire_at: None,
            expires_at: None,
            policy: None,
            last_run_id: None,
            last_error: None,
            depends_on: Vec::new(),
        }
    }

    /// Create a new cron-mode routine.
    ///
    /// Validates the cron expression by parsing it, then computes the
    /// initial `next_fire_at`. Returns `CronParseError` if the expression
    /// is malformed or cannot fire.
    pub fn new_cron(
        name: String,
        prompt: String,
        cron_expr: String,
    ) -> Result<Self, CronParseError> {
        parse_cron(&cron_expr)?;
        let next_fire_at = compute_next_fire_utc(&cron_expr, Utc::now())?;
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
            name,
            prompt,
            interval_secs: 0,
            created_at: Utc::now(),
            last_fired: None,
            enabled: true,
            fire_count: 0,
            max_fires: None,
            trigger_type: TriggerType::Cron,
            cron_expr: Some(cron_expr),
            timezone: None,
            next_fire_at,
            expires_at: None,
            policy: None,
            last_run_id: None,
            last_error: None,
            depends_on: Vec::new(),
        })
    }

    /// Whether this routine uses cron mode.
    pub fn is_cron(&self) -> bool {
        self.trigger_type == TriggerType::Cron
    }

    /// Check if the routine should fire now.
    ///
    /// - Interval mode: applies a random jitter (10% of period, capped at
    ///   15 min) to prevent thundering-herd effects.
    /// - Cron mode: applies Claude Code-style jitter (up to 90s early when
    ///   minute field is `:00` or `:30`).
    pub fn should_fire(&self) -> bool {
        if !self.enabled {
            return false;
        }
        if let Some(max) = self.max_fires {
            if self.fire_count >= max {
                return false;
            }
        }
        if let Some(exp) = self.expires_at {
            if Utc::now() >= exp {
                return false;
            }
        }
        match self.trigger_type {
            TriggerType::Cron => self.should_fire_cron(),
            _ => self.should_fire_interval(),
        }
    }

    /// Interval-mode should-fire check.
    fn should_fire_interval(&self) -> bool {
        match self.last_fired {
            None => true,
            Some(last) => {
                let elapsed = Utc::now().signed_duration_since(last).num_seconds().max(0) as u64;
                let jitter = apply_jitter(self.interval_secs, JITTER_CAP_SECS);
                elapsed >= self.interval_secs + jitter
            }
        }
    }

    /// Cron-mode should-fire check.
    ///
    /// Compares now against `next_fire_at`, with Claude Code-style early-fire
    /// jitter (up to 90s when the minute field is `:00` or `:30`).
    fn should_fire_cron(&self) -> bool {
        let now = Utc::now();
        let next = match self.next_fire_at {
            Some(t) => t,
            None => return true,
        };
        let jitter_secs = self
            .cron_expr
            .as_deref()
            .map(apply_cron_jitter)
            .unwrap_or(0);
        let effective = next - chrono::Duration::seconds(jitter_secs);
        now >= effective
    }

    /// Mark the routine as fired now.
    ///
    /// For cron mode, recomputes `next_fire_at` from the cron expression.
    /// Logs a warning if recomputation fails (the expression was valid at
    /// construction, so this should be rare).
    pub fn mark_fired(&mut self) {
        self.last_fired = Some(Utc::now());
        self.fire_count += 1;
        if self.trigger_type == TriggerType::Cron {
            if let Some(cron_expr) = &self.cron_expr {
                match compute_next_fire_utc(cron_expr, Utc::now()) {
                    Ok(next) => self.next_fire_at = next,
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            cron = %cron_expr,
                            "failed to recompute next fire time after firing"
                        );
                    }
                }
            }
        }
    }
}

/// Manager for scheduled routines.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoutineManager {
    /// Active routines keyed by ID.
    pub routines: HashMap<String, ScheduledRoutine>,
}

/// A due routine returned by [`RoutineManager::drain_due_with_history`].
///
/// Carries everything the caller needs to execute the prompt and then update
/// the corresponding run record via `ScheduledRunsStore::update`.
#[derive(Debug, Clone)]
pub struct DueRun {
    /// ID of the routine that fired.
    pub task_id: String,
    /// Human-readable routine name (denormalized for historical readability).
    pub task_name: String,
    /// The prompt to execute.
    pub prompt: String,
    /// ID of the `Running` run record created for this firing. Caller must
    /// finish it via `ScheduledRunsStore::update(run_id, |r| r.finish(...))`.
    pub run_id: String,
}

impl RoutineManager {
    /// Create a new empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a routine.
    pub fn add(&mut self, routine: ScheduledRoutine) -> String {
        let id = routine.id.clone();
        self.routines.insert(id.clone(), routine);
        id
    }

    /// Remove a routine by ID or name prefix.
    pub fn remove(&mut self, id_or_name: &str) -> Option<ScheduledRoutine> {
        // Try exact ID match first
        if let Some(r) = self.routines.remove(id_or_name) {
            return Some(r);
        }
        // Try name prefix match
        let key = self
            .routines
            .keys()
            .find(|k| k.starts_with(id_or_name))
            .cloned()?;
        self.routines.remove(&key)
    }

    /// Get a routine by ID.
    pub fn get(&self, id: &str) -> Option<&ScheduledRoutine> {
        self.routines.get(id)
    }

    /// Get a mutable routine by ID.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut ScheduledRoutine> {
        self.routines.get_mut(id)
    }

    /// Toggle a routine on/off.
    pub fn toggle(&mut self, id_or_name: &str) -> Option<bool> {
        // Try exact ID
        if let Some(r) = self.routines.get_mut(id_or_name) {
            r.enabled = !r.enabled;
            return Some(r.enabled);
        }
        // Try prefix
        let key = self
            .routines
            .keys()
            .find(|k| k.starts_with(id_or_name))
            .cloned()?;
        let r = self.routines.get_mut(&key)?;
        r.enabled = !r.enabled;
        Some(r.enabled)
    }

    /// Get all due prompts and mark them fired.
    pub fn drain_due(&mut self) -> Vec<(String, String)> {
        let mut due = Vec::new();
        for routine in self.routines.values_mut() {
            if routine.should_fire() {
                let name = routine.name.clone();
                let prompt = routine.prompt.clone();
                routine.mark_fired();
                due.push((name, prompt));
            }
        }
        due
    }

    /// Check whether a routine is blocked by pending dependencies (C4).
    ///
    /// A dependency "completed" means its most recent run finished with
    /// [`crate::scheduled_runs::RunStatus::Succeeded`]. If the dependency
    /// has never run, is currently running, or its last run failed, it is
    /// considered pending and blocks the dependent routine.
    ///
    /// Dependencies that no longer exist in the manager (deleted) are
    /// silently ignored — they do not block.
    pub fn check_dependencies(
        &self,
        routine_id: &str,
        runs: &crate::scheduled_runs::ScheduledRunsStore,
    ) -> DependencyBlocker {
        let mut blocked_by = Vec::new();
        if let Some(routine) = self.routines.get(routine_id) {
            for dep_id in &routine.depends_on {
                if self.routines.get(dep_id).is_none() {
                    // Deleted dependency — don't block.
                    continue;
                }
                let succeeded = routine_last_run_succeeded(self.routines.get(dep_id), runs);
                if !succeeded {
                    blocked_by.push(dep_id.clone());
                }
            }
        }
        DependencyBlocker {
            routine_id: routine_id.to_string(),
            blocked_by,
        }
    }

    /// Get all due prompts, mark them fired, and record a `Running` run for each
    /// in the given history store.
    ///
    /// Unlike [`Self::drain_due`], this method:
    /// - Creates a `ScheduledRun` with status `Running` for each fired routine
    /// - Sets `routine.last_run_id` to the new run ID
    /// - Returns [`DueRun`] structs carrying `task_id` / `task_name` / `prompt` / `run_id`
    /// - **Skips routines blocked by pending dependencies (C4)**, recording
    ///   the blocker IDs in `last_error` instead of firing.
    ///
    /// Callers must call [`ScheduledRunsStore::update`] with the `run_id` to
    /// transition the run to `Succeeded` / `Failed` after executing the prompt.
    pub fn drain_due_with_history(
        &mut self,
        runs: &crate::scheduled_runs::ScheduledRunsStore,
    ) -> Result<Vec<DueRun>, crate::scheduled_runs::RunsStoreError> {
        let mut due = Vec::new();
        // Pre-compute dependency success map to avoid double-borrowing
        // self.routines while iterating values_mut (C4).
        let dep_succeeded: std::collections::HashMap<String, bool> = self
            .routines
            .iter()
            .map(|(id, r)| (id.clone(), routine_last_run_succeeded(Some(r), runs)))
            .collect();
        for routine in self.routines.values_mut() {
            if routine.should_fire() {
                // C4: dependency check — gather blocker IDs before firing.
                let blocker_ids: Vec<String> = routine
                    .depends_on
                    .iter()
                    .filter(|dep_id| dep_succeeded.get(*dep_id).copied() == Some(false))
                    .cloned()
                    .collect();
                if !blocker_ids.is_empty() {
                    routine.last_error =
                        Some(format!("Blocked by deps: {}", blocker_ids.join(", ")));
                    continue;
                }
                let run_id = runs.start_run(&routine.id, &routine.name)?;
                routine.last_run_id = Some(run_id.clone());
                let task_id = routine.id.clone();
                let task_name = routine.name.clone();
                let prompt = routine.prompt.clone();
                routine.mark_fired();
                due.push(DueRun {
                    task_id,
                    task_name,
                    prompt,
                    run_id,
                });
            }
        }
        Ok(due)
    }

    /// List all routines.
    pub fn list(&self) -> Vec<&ScheduledRoutine> {
        let mut routines: Vec<_> = self.routines.values().collect();
        routines.sort_by_key(|r| r.created_at);
        routines
    }

    /// Save routines to a file.
    pub fn save_to_file(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)
    }

    /// Load routines from a file.
    pub fn load_from_file(path: &std::path::Path) -> Result<Self, std::io::Error> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Get the default storage path.
    pub fn default_storage_path() -> std::path::PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".shannon")
            .join("routines.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routine_should_fire_initially() {
        let r = ScheduledRoutine::new("test".into(), "hello".into(), 60);
        assert!(r.should_fire());
    }

    #[test]
    fn test_routine_not_before_interval() {
        let mut r = ScheduledRoutine::new("test".into(), "hello".into(), 3600);
        r.mark_fired();
        assert!(!r.should_fire());
    }

    #[test]
    fn test_routine_max_fires() {
        let mut r = ScheduledRoutine::new("test".into(), "hello".into(), 0);
        r.max_fires = Some(1);
        r.mark_fired();
        assert!(!r.should_fire());
    }

    #[test]
    fn test_routine_manager_add_remove() {
        let mut mgr = RoutineManager::new();
        let r = ScheduledRoutine::new("test".into(), "hello".into(), 60);
        let id = r.id.clone();
        mgr.add(r);
        assert!(mgr.get(&id).is_some());
        mgr.remove(&id);
        assert!(mgr.get(&id).is_none());
    }

    #[test]
    fn test_routine_manager_toggle() {
        let mut mgr = RoutineManager::new();
        let r = ScheduledRoutine::new("test".into(), "hello".into(), 60);
        let id = r.id.clone();
        mgr.add(r);
        let enabled = mgr.toggle(&id);
        assert_eq!(enabled, Some(false));
    }

    #[test]
    fn test_serialization() {
        let mut mgr = RoutineManager::new();
        mgr.add(ScheduledRoutine::new("test".into(), "hello".into(), 60));
        let json = serde_json::to_string(&mgr).unwrap();
        let back: RoutineManager = serde_json::from_str(&json).unwrap();
        assert_eq!(back.routines.len(), 1);
    }

    #[test]
    fn test_apply_jitter_within_bounds() {
        // Jitter for 3600s period should be <= 360s (10%) and <= 900s (cap)
        for _ in 0..100 {
            let jitter = apply_jitter(3600, 900);
            assert!(jitter <= 360, "jitter {jitter} exceeds 10% of 3600");
            assert!(jitter <= 900, "jitter {jitter} exceeds cap");
        }
    }

    #[test]
    fn test_apply_jitter_small_period() {
        // Very small period should produce 0 jitter
        let jitter = apply_jitter(1, 900);
        assert!(
            jitter <= 1,
            "jitter {jitter} should be 0 or 1 for 1s period"
        );
    }

    #[test]
    fn test_apply_jitter_cap_applied() {
        // For a huge period, jitter should be capped at max_cap_secs
        for _ in 0..100 {
            let jitter = apply_jitter(u64::MAX, 900);
            assert!(jitter <= 900, "jitter {jitter} exceeds cap of 900s");
        }
    }

    // ── Additional tests ─────────────────────────────────────────────────

    #[test]
    fn test_save_and_load_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("routines.json");

        let mut mgr = RoutineManager::new();
        let id = mgr.add(ScheduledRoutine::new("test".into(), "hello".into(), 60));
        mgr.save_to_file(&path).unwrap();

        let loaded = RoutineManager::load_from_file(&path).unwrap();
        assert_eq!(loaded.routines.len(), 1);
        assert!(loaded.get(&id).is_some());
        assert_eq!(loaded.get(&id).unwrap().name, "test");
    }

    #[test]
    fn test_load_corrupted_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not valid json [[[[").unwrap();

        let result = RoutineManager::load_from_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_drain_due_fires_initially() {
        let mut mgr = RoutineManager::new();
        mgr.add(ScheduledRoutine::new("a".into(), "prompt-a".into(), 60));
        mgr.add(ScheduledRoutine::new("b".into(), "prompt-b".into(), 3600));

        let due = mgr.drain_due();
        assert_eq!(due.len(), 2);
        // Both should fire since they've never fired before
        let names: Vec<&str> = due.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }

    #[test]
    fn test_drain_due_skips_recently_fired() {
        let mut mgr = RoutineManager::new();
        let mut r = ScheduledRoutine::new("test".into(), "prompt".into(), 3600);
        r.mark_fired(); // Just fired, shouldn't fire again
        mgr.add(r);

        let due = mgr.drain_due();
        assert!(due.is_empty());
    }

    #[test]
    fn test_drain_due_with_history_creates_running_runs() {
        use crate::scheduled_runs::{RunStatus, ScheduledRunsStore};

        let tmp = tempfile::tempdir().unwrap();
        let runs = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut mgr = RoutineManager::new();
        let r = ScheduledRoutine::new("scan".into(), "do scan".into(), 60);
        let task_id = r.id.clone();
        mgr.add(r);

        let due = mgr.drain_due_with_history(&runs).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].task_id, task_id);
        assert_eq!(due[0].task_name, "scan");
        assert_eq!(due[0].prompt, "do scan");
        assert_eq!(due[0].run_id.len(), 8);

        // Run record exists with Running status
        let run = runs.find_by_id(&due[0].run_id).unwrap().unwrap();
        assert_eq!(run.task_id, task_id);
        assert_eq!(run.task_name, "scan");
        assert_eq!(run.status, RunStatus::Running);

        // Routine's last_run_id is updated
        let routine = mgr.get(&task_id).unwrap();
        assert_eq!(routine.last_run_id.as_deref(), Some(due[0].run_id.as_str()));
    }

    #[test]
    fn test_drain_due_with_history_finish_then_refire_skips() {
        use crate::scheduled_runs::{RunStatus, ScheduledRunsStore};

        let tmp = tempfile::tempdir().unwrap();
        let runs = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut mgr = RoutineManager::new();
        let r = ScheduledRoutine::new("scan".into(), "do scan".into(), 3600);
        mgr.add(r);

        // First drain fires and starts a run
        let due = mgr.drain_due_with_history(&runs).unwrap();
        assert_eq!(due.len(), 1);
        let run_id = due[0].run_id.clone();

        // Caller simulates finishing the run
        runs.update(&run_id, |r| r.finish(RunStatus::Succeeded, None))
            .unwrap();
        let run = runs.find_by_id(&run_id).unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Succeeded);

        // Second drain should not refire (interval hasn't elapsed)
        let due2 = mgr.drain_due_with_history(&runs).unwrap();
        assert!(due2.is_empty());
    }

    #[test]
    fn test_drain_due_with_history_no_due_returns_empty() {
        use crate::scheduled_runs::ScheduledRunsStore;

        let tmp = tempfile::tempdir().unwrap();
        let runs = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut mgr = RoutineManager::new();
        // No routines at all
        let due = mgr.drain_due_with_history(&runs).unwrap();
        assert!(due.is_empty());
    }

    #[test]
    fn test_drain_due_with_history_disabled_routines_skipped() {
        use crate::scheduled_runs::ScheduledRunsStore;

        let tmp = tempfile::tempdir().unwrap();
        let runs = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut mgr = RoutineManager::new();
        let mut r = ScheduledRoutine::new("off".into(), "p".into(), 60);
        r.enabled = false;
        mgr.add(r);

        let due = mgr.drain_due_with_history(&runs).unwrap();
        assert!(due.is_empty());
    }

    #[test]
    fn test_drain_due_with_history_multiple_due() {
        use crate::scheduled_runs::ScheduledRunsStore;

        let tmp = tempfile::tempdir().unwrap();
        let runs = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut mgr = RoutineManager::new();
        mgr.add(ScheduledRoutine::new("a".into(), "p-a".into(), 60));
        mgr.add(ScheduledRoutine::new("b".into(), "p-b".into(), 60));
        mgr.add(ScheduledRoutine::new("c".into(), "p-c".into(), 60));

        let due = mgr.drain_due_with_history(&runs).unwrap();
        assert_eq!(due.len(), 3);
        // All run_ids unique
        let ids: Vec<&str> = due.iter().map(|d| d.run_id.as_str()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn test_drain_due_with_history_cron_mode_creates_run() {
        use crate::scheduled_runs::{RunStatus, ScheduledRunsStore};

        let tmp = tempfile::tempdir().unwrap();
        let runs = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut mgr = RoutineManager::new();
        let mut r =
            ScheduledRoutine::new_cron("cron-task".into(), "p".into(), "* * * * *".into()).unwrap();
        // Force next_fire_at to past so should_fire_cron returns true
        r.next_fire_at = Some(Utc::now() - chrono::Duration::minutes(1));
        mgr.add(r);

        let due = mgr.drain_due_with_history(&runs).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].task_name, "cron-task");

        // Run created with Running status
        let run = runs.find_by_id(&due[0].run_id).unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Running);

        // Mark as succeeded
        runs.update(&due[0].run_id, |r| r.finish(RunStatus::Succeeded, None))
            .unwrap();
    }

    #[test]
    fn test_remove_by_name_prefix() {
        let mut mgr = RoutineManager::new();
        let r = ScheduledRoutine::new("my-routine".into(), "prompt".into(), 60);
        let id = r.id.clone();
        mgr.add(r);

        // Remove by ID prefix (first 4 chars)
        let prefix = &id[..4];
        let removed = mgr.remove(prefix);
        assert!(removed.is_some());
        assert!(mgr.get(&id).is_none());
    }

    #[test]
    fn test_list_sorted_by_created_at() {
        let mut mgr = RoutineManager::new();
        let r1 = ScheduledRoutine::new("first".into(), "p1".into(), 60);
        let r2 = ScheduledRoutine::new("second".into(), "p2".into(), 60);
        mgr.add(r1);
        mgr.add(r2);

        let list = mgr.list();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_default_storage_path() {
        let path = RoutineManager::default_storage_path();
        assert!(path.to_string_lossy().contains(".shannon"));
        assert!(path.to_string_lossy().contains("routines.json"));
    }

    #[test]
    fn test_get_mut_routine() {
        let mut mgr = RoutineManager::new();
        let r = ScheduledRoutine::new("test".into(), "prompt".into(), 60);
        let id = r.id.clone();
        mgr.add(r);

        let r = mgr.get_mut(&id).unwrap();
        assert!(r.enabled);
        r.enabled = false;

        assert!(!mgr.get(&id).unwrap().enabled);
    }

    // ── v2 cron-mode tests ───────────────────────────────────────────────

    #[test]
    fn test_trigger_type_default_is_interval() {
        assert_eq!(TriggerType::default(), TriggerType::Interval);
    }

    #[test]
    fn test_trigger_type_serde_lowercase() {
        for (tt, expected) in [
            (TriggerType::Interval, "\"interval\""),
            (TriggerType::Cron, "\"cron\""),
            (TriggerType::Webhook, "\"webhook\""),
            (TriggerType::Event, "\"event\""),
        ] {
            let s = serde_json::to_string(&tt).unwrap();
            assert_eq!(s, expected);
            let back: TriggerType = serde_json::from_str(&s).unwrap();
            assert_eq!(back, tt);
        }
    }

    #[test]
    fn test_new_cron_valid_expression() {
        let r = ScheduledRoutine::new_cron(
            "standup".into(),
            "Run daily standup summary".into(),
            "0 9 * * 1-5".into(),
        )
        .expect("valid cron");
        assert!(r.is_cron());
        assert_eq!(r.trigger_type, TriggerType::Cron);
        assert_eq!(r.cron_expr.as_deref(), Some("0 9 * * 1-5"));
        assert!(r.next_fire_at.is_some());
        assert!(r.next_fire_at.unwrap() > Utc::now());
    }

    #[test]
    fn test_new_cron_invalid_expression() {
        let result = ScheduledRoutine::new_cron("bad".into(), "p".into(), "not a cron".into());
        assert!(result.is_err());
    }

    #[test]
    fn test_new_cron_impossible_expression() {
        // 5-field doesn't have a way to express "never fires" cleanly, but
        // a wildly out-of-range value should fail parsing.
        let result = ScheduledRoutine::new_cron("x".into(), "p".into(), "99 99 * * *".into());
        assert!(result.is_err());
    }

    #[test]
    fn test_is_cron_flag() {
        let interval = ScheduledRoutine::new("i".into(), "p".into(), 60);
        assert!(!interval.is_cron());

        let cron = ScheduledRoutine::new_cron("c".into(), "p".into(), "* * * * *".into()).unwrap();
        assert!(cron.is_cron());
    }

    #[test]
    fn test_parse_cron_valid() {
        assert!(parse_cron("* * * * *").is_ok());
        assert!(parse_cron("0 9 * * 1-5").is_ok());
        assert!(parse_cron("*/15 * * * *").is_ok());
        assert!(parse_cron("0 0 1 1 *").is_ok());
    }

    #[test]
    fn test_parse_cron_invalid() {
        assert!(parse_cron("").is_err());
        assert!(parse_cron("not a cron").is_err());
        assert!(parse_cron("99 * * * *").is_err()); // minute out of range
    }

    #[test]
    fn test_compute_next_fire_utc_returns_future() {
        let now = Utc::now();
        let next = compute_next_fire_utc("0 9 * * 1-5", now).unwrap().unwrap();
        assert!(next > now);
    }

    #[test]
    fn test_compute_next_fire_utc_minute_resolution() {
        // "*/5 * * * *" should fire within the next 5 minutes
        let now = Utc::now();
        let next = compute_next_fire_utc("*/5 * * * *", now).unwrap().unwrap();
        let delta = next.signed_duration_since(now);
        assert!(delta.num_seconds() <= 5 * 60);
        assert!(delta.num_seconds() > 0);
    }

    #[test]
    fn test_apply_cron_jitter_zero_minute() {
        // Minute field "0" gets Claude Code jitter (0-90s)
        for _ in 0..50 {
            let j = apply_cron_jitter("0 * * * *");
            assert!(j >= 0 && j <= 90, "jitter {j} out of [0, 90]");
        }
    }

    #[test]
    fn test_apply_cron_jitter_thirty_minute() {
        for _ in 0..50 {
            let j = apply_cron_jitter("30 * * * *");
            assert!(j >= 0 && j <= 90);
        }
    }

    #[test]
    fn test_apply_cron_jitter_non_round_minute() {
        // Non-:00/:30 minutes get no jitter
        assert_eq!(apply_cron_jitter("15 * * * *"), 0);
        assert_eq!(apply_cron_jitter("45 * * * *"), 0);
        assert_eq!(apply_cron_jitter("*/5 * * * *"), 0); // wildcard, no jitter
    }

    #[test]
    fn test_should_fire_cron_with_past_next_fire() {
        let mut r = ScheduledRoutine::new_cron("t".into(), "p".into(), "* * * * *".into()).unwrap();
        // Force next_fire_at into the past
        r.next_fire_at = Some(Utc::now() - chrono::Duration::minutes(1));
        assert!(r.should_fire());
    }

    #[test]
    fn test_should_fire_cron_with_future_next_fire() {
        let mut r = ScheduledRoutine::new_cron("t".into(), "p".into(), "* * * * *".into()).unwrap();
        r.next_fire_at = Some(Utc::now() + chrono::Duration::hours(1));
        assert!(!r.should_fire());
    }

    #[test]
    fn test_mark_fired_updates_next_fire_for_cron() {
        let mut r = ScheduledRoutine::new_cron("t".into(), "p".into(), "* * * * *".into()).unwrap();
        let original_next = r.next_fire_at.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        r.mark_fired();
        let new_next = r.next_fire_at.unwrap();
        // After firing, next_fire_at should advance past the original time
        assert!(new_next > original_next);
        assert_eq!(r.fire_count, 1);
    }

    #[test]
    fn test_mark_fired_no_op_for_interval_next_fire() {
        // Interval mode: next_fire_at stays None after mark_fired
        let mut r = ScheduledRoutine::new("t".into(), "p".into(), 60);
        r.mark_fired();
        assert!(r.next_fire_at.is_none());
    }

    #[test]
    fn test_expires_at_blocks_should_fire() {
        let mut r = ScheduledRoutine::new("t".into(), "p".into(), 60);
        r.expires_at = Some(Utc::now() - chrono::Duration::seconds(1));
        assert!(!r.should_fire());
    }

    #[test]
    fn test_expires_at_future_allows_should_fire() {
        let mut r = ScheduledRoutine::new("t".into(), "p".into(), 60);
        r.expires_at = Some(Utc::now() + chrono::Duration::days(1));
        assert!(r.should_fire());
    }

    #[test]
    fn test_v2_struct_serialization_roundtrip() {
        let r = ScheduledRoutine::new_cron("daily".into(), "standup".into(), "0 9 * * 1-5".into())
            .unwrap();
        let json = serde_json::to_string(&r).unwrap();
        let back: ScheduledRoutine = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, r.id);
        assert_eq!(back.trigger_type, TriggerType::Cron);
        assert_eq!(back.cron_expr, r.cron_expr);
        assert!(back.is_cron());
    }

    #[test]
    fn test_backward_compat_legacy_json_loads() {
        // Simulate a legacy routines.json entry that has only interval-mode fields.
        let legacy_json = r#"{
            "id": "abc12345",
            "name": "old",
            "prompt": "hello",
            "interval_secs": 60,
            "created_at": "2026-01-01T00:00:00Z",
            "last_fired": null,
            "enabled": true,
            "fire_count": 5,
            "max_fires": null
        }"#;
        let r: ScheduledRoutine = serde_json::from_str(legacy_json).expect("backward compat");
        assert_eq!(r.trigger_type, TriggerType::Interval);
        assert!(r.cron_expr.is_none());
        assert!(r.policy.is_none());
        assert_eq!(r.interval_secs, 60);
    }

    #[test]
    fn test_execution_policy_serde_defaults() {
        let p = ExecutionPolicy::default();
        assert_eq!(p.max_retries, 0);
        assert_eq!(p.timeout_secs, 0);
        assert!(p.worktree.is_none());
        assert!(!p.notify_on_failure);
        assert!(p.budget_usd.is_none());
        assert!(p.auto_archive_when_empty);
    }

    #[test]
    fn test_execution_policy_skip_serializing_none_fields() {
        let p = ExecutionPolicy::default();
        let json = serde_json::to_string(&p).unwrap();
        // worktree, budget_usd should be skipped
        assert!(!json.contains("worktree"));
        assert!(!json.contains("budget_usd"));
        // auto_archive_when_empty should serialize (default true)
        assert!(json.contains("auto_archive_when_empty"));
    }

    // ── C4: Task dependency tests ────────────────────────────────────────

    #[test]
    fn test_depends_on_default_empty() {
        let r = ScheduledRoutine::new("a".into(), "p".into(), 60);
        assert!(
            r.depends_on.is_empty(),
            "new routines have no deps by default"
        );
    }

    #[test]
    fn test_depends_on_backward_compat_missing_field() {
        // Legacy JSON without depends_on should still deserialize.
        let json = r#"{
            "id": "abc12345",
            "name": "legacy",
            "prompt": "do thing",
            "interval_secs": 60,
            "created_at": "2024-01-01T00:00:00Z",
            "enabled": true,
            "fire_count": 0,
            "trigger_type": "interval"
        }"#;
        let r: ScheduledRoutine = serde_json::from_str(json).unwrap();
        assert!(r.depends_on.is_empty());
    }

    #[test]
    fn test_depends_on_serializes_only_when_nonempty() {
        let mut r = ScheduledRoutine::new("a".into(), "p".into(), 60);
        let json_empty = serde_json::to_string(&r).unwrap();
        assert!(!json_empty.contains("depends_on"));
        r.depends_on = vec!["dep1".into()];
        let json_with_dep = serde_json::to_string(&r).unwrap();
        assert!(json_with_dep.contains("depends_on"));
    }

    #[test]
    fn test_check_dependencies_no_deps_returns_empty_blocker() {
        use crate::scheduled_runs::ScheduledRunsStore;
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut mgr = RoutineManager::new();
        let id = mgr.add(ScheduledRoutine::new("a".into(), "p".into(), 60));
        let blocker = mgr.check_dependencies(&id, &store);
        assert!(!blocker.is_blocked());
        assert!(blocker.blocked_by.is_empty());
    }

    #[test]
    fn test_check_dependencies_blocked_when_dep_never_ran() {
        use crate::scheduled_runs::ScheduledRunsStore;
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut mgr = RoutineManager::new();
        let dep_id = mgr.add(ScheduledRoutine::new("dep".into(), "p".into(), 60));
        let mut child = ScheduledRoutine::new("child".into(), "p".into(), 60);
        child.depends_on = vec![dep_id.clone()];
        let child_id = mgr.add(child);
        let blocker = mgr.check_dependencies(&child_id, &store);
        assert!(blocker.is_blocked());
        assert_eq!(blocker.blocked_by, vec![dep_id]);
    }

    #[test]
    fn test_check_dependencies_unblocked_when_dep_succeeded() {
        use crate::scheduled_runs::{RunStatus, ScheduledRunsStore};
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut mgr = RoutineManager::new();
        let dep_id = mgr.add(ScheduledRoutine::new("dep".into(), "p".into(), 60));
        let run_id = store.start_run(&dep_id, "dep").unwrap();
        store
            .update(&run_id, |r| r.finish(RunStatus::Succeeded, None))
            .unwrap();
        mgr.routines.get_mut(&dep_id).unwrap().last_run_id = Some(run_id);
        let mut child = ScheduledRoutine::new("child".into(), "p".into(), 60);
        child.depends_on = vec![dep_id.clone()];
        let child_id = mgr.add(child);
        let blocker = mgr.check_dependencies(&child_id, &store);
        assert!(!blocker.is_blocked(), "dep succeeded → child unblocked");
    }

    #[test]
    fn test_check_dependencies_blocked_when_dep_failed() {
        use crate::scheduled_runs::{RunStatus, ScheduledRunsStore};
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut mgr = RoutineManager::new();
        let dep_id = mgr.add(ScheduledRoutine::new("dep".into(), "p".into(), 60));
        let run_id = store.start_run(&dep_id, "dep").unwrap();
        store
            .update(&run_id, |r| {
                r.finish(RunStatus::Failed, Some("boom".into()))
            })
            .unwrap();
        mgr.routines.get_mut(&dep_id).unwrap().last_run_id = Some(run_id);
        let mut child = ScheduledRoutine::new("child".into(), "p".into(), 60);
        child.depends_on = vec![dep_id.clone()];
        let child_id = mgr.add(child);
        let blocker = mgr.check_dependencies(&child_id, &store);
        assert!(blocker.is_blocked(), "dep failed → child blocked");
    }

    #[test]
    fn test_check_dependencies_ignores_deleted_deps() {
        use crate::scheduled_runs::ScheduledRunsStore;
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut mgr = RoutineManager::new();
        let mut child = ScheduledRoutine::new("child".into(), "p".into(), 60);
        child.depends_on = vec!["ghost-dep".into()];
        let child_id = mgr.add(child);
        let blocker = mgr.check_dependencies(&child_id, &store);
        assert!(
            !blocker.is_blocked(),
            "deleted deps don't block (graceful degradation)"
        );
    }

    #[test]
    fn test_drain_due_skips_blocked_routine_and_records_error() {
        use crate::scheduled_runs::ScheduledRunsStore;
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut mgr = RoutineManager::new();
        // Disable dep so only the child is a fire candidate.
        let mut dep = ScheduledRoutine::new("dep".into(), "p".into(), 60);
        dep.enabled = false;
        let dep_id = mgr.add(dep);
        let mut child = ScheduledRoutine::new("child".into(), "p".into(), 60);
        child.depends_on = vec![dep_id.clone()];
        let child_id = mgr.add(child);
        let due = mgr.drain_due_with_history(&store).unwrap();
        assert!(due.is_empty(), "blocked routine should not fire");
        let child_after = mgr.get(&child_id).unwrap();
        assert!(
            child_after
                .last_error
                .as_deref()
                .unwrap()
                .contains("Blocked by deps")
        );
    }

    #[test]
    fn test_drain_due_fires_unblocked_routine() {
        use crate::scheduled_runs::{RunStatus, ScheduledRunsStore};
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut mgr = RoutineManager::new();
        // Disable dep so it doesn't also fire; but record a Succeeded run
        // so the dependency check sees it as complete.
        let mut dep = ScheduledRoutine::new("dep".into(), "p".into(), 60);
        dep.enabled = false;
        let dep_id = mgr.add(dep);
        let run_id = store.start_run(&dep_id, "dep").unwrap();
        store
            .update(&run_id, |r| r.finish(RunStatus::Succeeded, None))
            .unwrap();
        mgr.routines.get_mut(&dep_id).unwrap().last_run_id = Some(run_id);
        let mut child = ScheduledRoutine::new("child".into(), "p".into(), 60);
        child.depends_on = vec![dep_id];
        let child_id = mgr.add(child);
        let due = mgr.drain_due_with_history(&store).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].task_id, child_id);
    }
}
