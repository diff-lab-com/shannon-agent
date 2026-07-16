//! Monthly budget enforcement for scheduled routines.
//!
//! Each [`crate::scheduled_routines::ScheduledRoutine`] may declare an
//! `ExecutionPolicy.budget_usd` cap. When the routine's cumulative spend
//! in the current calendar month (UTC) exceeds that cap, the routine is
//! auto-disabled to prevent runaway cost — matching Codex's automation
//! pattern.
//!
//! ## Cost Source
//!
//! Per-run costs come from [`crate::scheduled_runs::ScheduledRun::cost_usd`].
//! The sum is computed across the calendar-month UTC window from the
//! JSONL run history via [`crate::scheduled_runs::ScheduledRunsStore::list_by_time_range`].
//!
//! ## Enforcement Point
//!
//! [`BudgetEnforcer::check`] returns a [`BudgetVerdict`] for the upcoming
//! fire. Callers should call this **before** invoking the routine so a
//! disabled routine is short-circuited without spending tokens. After
//! each successful fire, callers call [`BudgetEnforcer::record_spend`]
//! to keep the in-memory totals fresh between history reads.
//!
//! Persistence: the enforcer keeps a per-task month-to-date total in
//! memory and refreshes from the runs store on construction. For
//! multi-process scenarios, the runs store is the source of truth —
//! re-instantiate the enforcer (or call [`BudgetEnforcer::refresh`])
//! after a restart to pick up writes from sibling processes.

use chrono::{DateTime, Datelike, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::scheduled_runs::{RunStatus, ScheduledRunsStore};

/// Per-task month-to-date spend, plus the task's monthly cap.
///
/// Cheap to construct, mutate, and serialize for telemetry. One instance
/// per routine; the [`BudgetEnforcer`] holds a `HashMap<task_id, BudgetState>`
/// keyed by routine ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BudgetState {
    /// The routine's monthly cap in USD (`None` = no cap).
    pub cap_usd: Option<f64>,
    /// Cumulative spend in the current month (UTC).
    pub spent_usd: f64,
    /// First day of the current billing month at 00:00 UTC.
    pub month_start: DateTime<Utc>,
    /// Number of times the routine has been disabled by the enforcer.
    pub disable_count: u32,
}

impl BudgetState {
    fn new(cap_usd: Option<f64>, month_start: DateTime<Utc>) -> Self {
        Self {
            cap_usd,
            spent_usd: 0.0,
            month_start,
            disable_count: 0,
        }
    }

    /// Roll the window over to the current UTC month, zeroing `spent_usd`.
    pub fn roll_over(&mut self, new_month_start: DateTime<Utc>) {
        self.spent_usd = 0.0;
        self.month_start = new_month_start;
    }
}

/// Verdict returned by [`BudgetEnforcer::check`].
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetVerdict {
    /// Routine is allowed to fire.
    Allow {
        /// Remaining budget in USD (`None` if no cap is set).
        remaining_usd: Option<f64>,
    },
    /// Routine has exceeded its monthly cap and must be disabled.
    Deny {
        /// Cumulative spend this month.
        spent_usd: f64,
        /// Configured cap.
        cap_usd: f64,
        /// Local reason (operator-facing).
        reason: String,
    },
    /// No budget cap configured for this routine.
    NoBudget,
}

/// In-memory enforcer that tracks month-to-date spend per task and
/// returns allow/deny verdicts before each fire.
///
/// The enforcer reads from a [`ScheduledRunsStore`] on construction
/// and [`refresh`](Self::refresh). Between refreshes, callers should
/// call [`record_spend`](Self::record_spend) to keep totals accurate.
pub struct BudgetEnforcer {
    store: ScheduledRunsStore,
    states: HashMap<String, BudgetState>,
}

impl BudgetEnforcer {
    /// Build a new enforcer, seeding per-task totals from the runs store.
    pub fn new(store: ScheduledRunsStore) -> Self {
        let mut enforcer = Self {
            store,
            states: HashMap::new(),
        };
        if let Err(e) = enforcer.refresh() {
            // Refresh failures shouldn't panic the constructor; downstream
            // callers will see empty states and treat them as zero spend.
            tracing::warn!("budget enforcer: initial refresh failed: {}", e);
        }
        enforcer
    }

    /// Construct an enforcer with a custom runs-store base path.
    pub fn with_base(base: PathBuf) -> Self {
        Self::new(ScheduledRunsStore::with_base(base))
    }

    /// Re-read run history for every tracked task and rebuild the
    /// month-to-date totals.
    pub fn refresh(&mut self) -> Result<(), crate::scheduled_runs::RunsStoreError> {
        self.states.clear();
        // We don't know which tasks exist until we look; for each known task
        // we compute a window and sum successful + failed + archived runs
        // (we don't double-count `Running` because they have no cost yet).
        let window_start = first_of_this_month_utc();
        let window_end = window_start + ChronoDuration::days(31);

        // List runs in the window grouped by task.
        let runs = self
            .store
            .list_by_time_range(window_start, window_end)
            .unwrap_or_default();
        let mut by_task: HashMap<String, f64> = HashMap::new();
        for r in runs {
            if matches!(r.status, RunStatus::Running) {
                continue;
            }
            if let Some(c) = r.cost_usd {
                *by_task.entry(r.task_id).or_insert(0.0) += c;
            }
        }

        for (task_id, spent) in by_task {
            self.states.insert(
                task_id,
                BudgetState {
                    cap_usd: None, // filled in by `register_cap` if known
                    spent_usd: spent,
                    month_start: window_start,
                    disable_count: 0,
                },
            );
        }
        Ok(())
    }

    /// Register a routine's cap. Idempotent — overwrites any prior cap.
    pub fn register_cap(&mut self, task_id: &str, cap_usd: Option<f64>) {
        let now = first_of_this_month_utc();
        let entry = self
            .states
            .entry(task_id.to_string())
            .or_insert_with(|| BudgetState::new(cap_usd, now));
        entry.cap_usd = cap_usd;
        if entry.month_start.month() != now.month() || entry.month_start.year() != now.year() {
            entry.roll_over(now);
        }
    }

    /// Record spend for a single run. Called after a routine fires.
    pub fn record_spend(&mut self, task_id: &str, cost_usd: f64) {
        self.roll_if_new_month(task_id);
        let entry = self
            .states
            .entry(task_id.to_string())
            .or_insert_with(|| BudgetState::new(None, first_of_this_month_utc()));
        entry.spent_usd += cost_usd;
    }

    /// Check whether `task_id` may fire right now.
    pub fn check(&mut self, task_id: &str) -> BudgetVerdict {
        self.roll_if_new_month(task_id);
        let Some(cap) = self
            .states
            .get(task_id)
            .and_then(|s| s.cap_usd)
        else {
            return BudgetVerdict::NoBudget;
        };
        let spent = self
            .states
            .get(task_id)
            .map(|s| s.spent_usd)
            .unwrap_or(0.0);
        if spent >= cap {
            BudgetVerdict::Deny {
                spent_usd: spent,
                cap_usd: cap,
                reason: format!(
                    "monthly budget exhausted: spent ${:.4} >= cap ${:.4}",
                    spent, cap
                ),
            }
        } else {
            BudgetVerdict::Allow {
                remaining_usd: Some((cap - spent).max(0.0)),
            }
        }
    }

    /// Mark a routine as disabled-by-budget. Bumps the disable counter
    /// for observability.
    pub fn mark_disabled(&mut self, task_id: &str) {
        let entry = self
            .states
            .entry(task_id.to_string())
            .or_insert_with(|| BudgetState::new(None, first_of_this_month_utc()));
        entry.disable_count = entry.disable_count.saturating_add(1);
    }

    /// Snapshot the in-memory state. Used by tests and telemetry.
    pub fn snapshot(&self, task_id: &str) -> Option<&BudgetState> {
        self.states.get(task_id)
    }

    /// Iterate every tracked task's state (for dashboards / telemetry).
    pub fn iter(&self) -> impl Iterator<Item = (&String, &BudgetState)> {
        self.states.iter()
    }

    fn roll_if_new_month(&mut self, task_id: &str) {
        let now = first_of_this_month_utc();
        if let Some(state) = self.states.get_mut(task_id) {
            if state.month_start.month() != now.month() || state.month_start.year() != now.year()
            {
                state.roll_over(now);
            }
        }
    }
}

/// First instant of the current UTC month.
fn first_of_this_month_utc() -> DateTime<Utc> {
    let now = Utc::now();
    let year = now.year();
    let month = now.month();
    // chrono doesn't expose a "first of month" constructor directly, so we
    // build from `(year, month, 1, 0, 0, 0)`.
    use chrono::NaiveDate;
    let date = NaiveDate::from_ymd_opt(year, month, 1).expect("valid ymd");
    let dt = date
        .and_hms_opt(0, 0, 0)
        .expect("valid hms")
        .and_utc();
    dt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduled_runs::ScheduledRun;
    use chrono::TimeZone as _;

    fn midnight_utc(year: i32, month: u32) -> DateTime<Utc> {
        use chrono::NaiveDate;
        let date = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
        Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
    }

    #[test]
    fn no_cap_allows_fire() {
        let tmp = tempfile::tempdir().unwrap();
        let mut enforcer = BudgetEnforcer::with_base(tmp.path().to_path_buf());
        enforcer.register_cap("t1", None);
        assert_eq!(enforcer.check("t1"), BudgetVerdict::NoBudget);
    }

    #[test]
    fn under_cap_allows_with_remaining() {
        let tmp = tempfile::tempdir().unwrap();
        let mut enforcer = BudgetEnforcer::with_base(tmp.path().to_path_buf());
        enforcer.register_cap("t1", Some(10.0));
        enforcer.record_spend("t1", 3.5);
        match enforcer.check("t1") {
            BudgetVerdict::Allow { remaining_usd } => {
                assert_eq!(remaining_usd, Some(6.5));
            }
            other => panic!("expected Allow, got {:?}", other),
        }
    }

    #[test]
    fn at_or_over_cap_denies() {
        let tmp = tempfile::tempdir().unwrap();
        let mut enforcer = BudgetEnforcer::with_base(tmp.path().to_path_buf());
        enforcer.register_cap("t1", Some(5.0));
        enforcer.record_spend("t1", 5.0);
        match enforcer.check("t1") {
            BudgetVerdict::Deny {
                spent_usd,
                cap_usd,
                ..
            } => {
                assert_eq!(spent_usd, 5.0);
                assert_eq!(cap_usd, 5.0);
            }
            other => panic!("expected Deny, got {:?}", other),
        }
    }

    #[test]
    fn roll_over_resets_spend() {
        let tmp = tempfile::tempdir().unwrap();
        let mut enforcer = BudgetEnforcer::with_base(tmp.path().to_path_buf());
        enforcer.register_cap("t1", Some(2.0));
        // Manually backdate the entry to last month.
        if let Some(state) = enforcer.states.get_mut("t1") {
            state.month_start = midnight_utc(2025, 1);
            state.spent_usd = 1.99;
        }
        // Forcing a check should roll over and reset to 0.
        let verdict = enforcer.check("t1");
        // After rollover spend is 0 < cap 2.0, so Allow.
        assert!(matches!(verdict, BudgetVerdict::Allow { .. }));
        if let Some(state) = enforcer.snapshot("t1") {
            assert_eq!(state.spent_usd, 0.0);
            assert_eq!(state.month_start, midnight_utc(2026, 7));
        } else {
            panic!("state missing after roll-over");
        }
    }

    #[test]
    fn refresh_rebuilds_totals_from_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());

        let mut run = ScheduledRun::start("task-1", "A");
        run.cost_usd = Some(0.42);
        run.finish(RunStatus::Succeeded, None);
        store.record(&run).unwrap();

        let mut run2 = ScheduledRun::start("task-1", "A");
        run2.cost_usd = Some(0.58);
        run2.finish(RunStatus::Failed, None);
        store.record(&run2).unwrap();

        let mut enforcer = BudgetEnforcer::new(store);
        enforcer.register_cap("task-1", Some(5.0));
        assert_eq!(enforcer.snapshot("task-1").unwrap().spent_usd, 1.00);
    }

    #[test]
    fn running_runs_are_excluded_from_total() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledRunsStore::with_base(tmp.path().to_path_buf());
        let mut run = ScheduledRun::start("task-1", "A");
        run.cost_usd = Some(1.23); // status still Running
        store.record(&run).unwrap();

        let mut enforcer = BudgetEnforcer::new(store);
        enforcer.register_cap("task-1", Some(5.0));
        // Running runs are excluded — but our local seed sees 0 spend, not 1.23.
        // We just verify refresh didn't crash and totals are zero.
        assert_eq!(enforcer.snapshot("task-1").map(|s| s.spent_usd), Some(0.0));
    }

    #[test]
    fn disable_count_increments() {
        let tmp = tempfile::tempdir().unwrap();
        let mut enforcer = BudgetEnforcer::with_base(tmp.path().to_path_buf());
        enforcer.register_cap("t1", Some(1.0));
        enforcer.record_spend("t1", 1.0);
        let _ = enforcer.check("t1");
        enforcer.mark_disabled("t1");
        enforcer.mark_disabled("t1");
        assert_eq!(enforcer.snapshot("t1").unwrap().disable_count, 2);
    }
}