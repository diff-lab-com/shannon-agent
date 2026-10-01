//! P2-1 / P2-6 — usage governance + pre-task cost estimation.
//!
//! - `get_usage_governance` powers the sidebar % bar and the /usage budget
//!   card: month-to-date spend across every source the Usage page counts
//!   (the chat ledger plus scheduled-routine spend folded in from the
//!   runs store), the user-set monthly budget, and the 80% / 100%
//!   threshold state. Reaching a threshold fires a desktop notification
//!   through the shared notifier pipeline on `AppState` —
//!   exactly once per threshold per calendar month (persisted markers,
//!   re-armed on month rollover). The /usage banner itself is derived from
//!   the live percent, so it stays up while the state persists.
//! - `estimate_task_cost` aggregates a routine's historical run costs from
//!   the scheduled-runs store into a read-only min/max/avg range for the
//!   creation-confirm surfaces (ScheduleForm, OPC quick task). A `None`
//!   task id aggregates across all routines — the "similar tasks" baseline
//!   a brand-new routine is estimated against.
//!
//! Deliberately reads-only: the budget itself lives in
//! `DesktopConfig::monthly_budget_usd` (written via `configure`), the
//! spend numbers come from the same stores `get_usage_stats` reads, and
//! the executor's own per-routine `budget_usd` gate (journey P1 G2b) is
//! untouched — this module only *visualizes* budgets, it never enforces.

use std::path::PathBuf;

use chrono::{DateTime, Datelike, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use shannon_core::notifier::{Notification, NotificationLevel, Notifier};
use shannon_core::scheduled_runs::{ScheduledRun, ScheduledRunsStore};

use crate::commands::AppState;
use crate::commands_usage::UsageRecord;

/// Share of the monthly budget at which the first advisory fires (the hard
/// notice is at 100%). Same 80/100 split the session budget uses
/// (`cost_commands::BUDGET_WARNING_FRACTION`).
pub(crate) const MONTHLY_WARN_FRACTION: f64 = 0.8;

/// Most recent cost-tracked runs folded into one [`TaskCostEstimate`].
const ESTIMATE_RUN_LIMIT: usize = 20;

// ── Threshold markers (persisted, once-per-month) ─────────────────────────

/// Which monthly thresholds have already fired. Keyed to the calendar month
/// (`"%Y-%m"`): a rollover re-arms both thresholds. One file
/// (`~/.shannon/desktop/usage-governance.json`), rewritten only when a
/// threshold actually fires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThresholdMarkers {
    pub month: String,
    pub warned_80: bool,
    pub hit_100: bool,
}

/// Load/save the marker file. Mirrors the `UsageStore` pattern: fixed
/// default path, injectable `with_path` for tests.
pub struct ThresholdMarkerStore {
    path: PathBuf,
}

impl ThresholdMarkerStore {
    /// Default location: `~/.shannon/desktop/usage-governance.json`.
    pub fn new() -> Self {
        Self {
            path: default_marker_path(),
        }
    }

    /// Custom path (for testing).
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Stored markers, `None` when the file is missing or unreadable (a
    /// corrupt marker file degrades to "nothing fired yet" — the worst case
    /// is one repeat notification).
    pub fn load(&self) -> Option<ThresholdMarkers> {
        let content = std::fs::read_to_string(&self.path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Persist markers. Creates parent dirs; the caller treats a failure as
    /// log-only (the % bar still works, the alert may re-fire next check).
    pub fn save(&self, markers: &ThresholdMarkers) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create marker dir: {e}"))?;
        }
        let content =
            serde_json::to_string_pretty(markers).map_err(|e| format!("serialize markers: {e}"))?;
        std::fs::write(&self.path, content).map_err(|e| format!("write markers: {e}"))
    }
}

impl Default for ThresholdMarkerStore {
    fn default() -> Self {
        Self::new()
    }
}

fn default_marker_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".shannon")
        .join("desktop")
        .join("usage-governance.json")
}

/// Outcome of folding current month spend into the threshold state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ThresholdDecision {
    /// Fire the 80% desktop notification this check.
    pub fire_80: bool,
    /// Fire the 100% desktop notification this check.
    pub fire_100: bool,
    /// Markers to persist (the input markers with the fired flags latched).
    pub next: ThresholdMarkers,
}

/// Pure threshold state machine: each threshold fires at most once per
/// calendar month, re-arms on month rollover, and a jump straight past the
/// cap fires the 100% notice only (the 80% flag latches alongside — the
/// same arm-order contract `BudgetTurnGuard` implements for the
/// per-session budget).
pub(crate) fn evaluate_thresholds(
    stored: Option<ThresholdMarkers>,
    month: &str,
    spent_usd: f64,
    budget_usd: f64,
) -> ThresholdDecision {
    let mut markers = match stored {
        Some(m) if m.month == month => m,
        _ => ThresholdMarkers {
            month: month.to_string(),
            warned_80: false,
            hit_100: false,
        },
    };

    let at_warn = spent_usd >= budget_usd * MONTHLY_WARN_FRACTION;
    let at_cap = spent_usd >= budget_usd;

    let fire_100 = at_cap && !markers.hit_100;
    if fire_100 {
        markers.hit_100 = true;
        markers.warned_80 = true;
    }
    let fire_80 = !fire_100 && at_warn && !markers.warned_80;
    if fire_80 {
        markers.warned_80 = true;
    }

    ThresholdDecision {
        fire_80,
        fire_100,
        next: markers,
    }
}

// ── Window math (local calendar, like the Usage page's day buckets) ───────

/// Calendar-month key of a local timestamp: `"%Y-%m"`.
pub(crate) fn month_key(now: DateTime<Local>) -> String {
    now.format("%Y-%m").to_string()
}

/// Epoch-ms of local midnight on the 1st of the month `now` falls in.
/// Out-of-range inputs degrade to 0 (everything counts toward the month —
/// the same fail-open choice `day_label` makes).
pub(crate) fn month_start_ms(now: DateTime<Local>) -> u64 {
    Local
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .map(|dt| dt.timestamp_millis().max(0) as u64)
        .unwrap_or(0)
}

/// Month-to-date and trailing-7-day cost over pre-folded records. Both
/// windows follow the local clock — the same convention the Usage page's
/// per-day buckets use (`commands_usage::day_label`).
pub(crate) fn summarize_windows(
    records: &[UsageRecord],
    now_ms: u64,
    month_start: u64,
) -> (f64, f64) {
    let week_start = now_ms.saturating_sub(7 * 86_400_000);
    let mut month_cost = 0.0f64;
    let mut week_cost = 0.0f64;
    for r in records {
        if r.timestamp_ms >= month_start {
            month_cost += r.cost_usd;
        }
        if r.timestamp_ms >= week_start {
            week_cost += r.cost_usd;
        }
    }
    (month_cost, week_cost)
}

/// The chat ledger verbatim — the windows this module sums over (R2-W2-2).
/// The former scheduled-runs fold would now double-count routine spend:
/// every routine run writes session-attributed ledger lines, and since the
/// run records carry the same cost the fold's lump sums would count it twice.
fn collect_governance_records(usage: &crate::commands_usage::UsageStore) -> Vec<UsageRecord> {
    usage.load()
}

// ── get_usage_governance ──────────────────────────────────────────────────

/// One sidebar/budget-card snapshot. camelCase on the wire, mirroring the
/// P0-4 DTOs.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageGovernanceDto {
    /// Calendar month the snapshot is keyed to (`"%Y-%m"`).
    pub month: String,
    /// Month-to-date spend across all sources.
    pub month_cost_usd: f64,
    /// Trailing 7-day spend (the no-budget fallback the sidebar shows).
    pub last7d_cost_usd: f64,
    /// User-set monthly budget (`DesktopConfig::monthly_budget_usd`).
    pub budget_usd: Option<f64>,
    /// `month_cost / budget * 100`, unclamped (a burst past the cap shows
    /// the real number). `None` without a budget.
    pub percent: Option<f64>,
    /// Marker state — the 80% desktop notification already fired this month.
    pub warned_80: bool,
    /// Marker state — the 100% desktop notification already fired this month.
    pub hit_100: bool,
    /// Live banner level for /usage: `Some("100")` at/over the cap,
    /// `Some("80")` at/over the warn line, `None` below both or without a
    /// budget. Derived from the current percent, not the once-per-month
    /// markers, so the banner stays up while the state persists.
    pub threshold_reached: Option<&'static str>,
}

/// Snapshot for the sidebar % bar and the /usage budget card. Side effect:
/// when a budget is set and a threshold is newly reached, fires the
/// once-per-month desktop notification through the shared notifier (with
/// the user's notification prefs — master switch / DND — applied by the
/// handler already attached to it) and persists the markers.
///
/// Read-only otherwise; safe to poll. The frontend polls this from the
/// sidebar, which is what keeps the threshold check running without any
/// wiring into `send_message` or the routine executor.
#[tauri::command]
pub async fn get_usage_governance(
    state: tauri::State<'_, AppState>,
) -> Result<UsageGovernanceDto, String> {
    let budget = state
        .desktop_config
        .read()
        .await
        .monthly_budget_usd
        .filter(|b| b.is_finite() && *b > 0.0);

    let now = Local::now();
    let now_ms = now.timestamp_millis().max(0) as u64;
    let records = collect_governance_records(&state.usage_store);
    let (month_cost, week_cost) = summarize_windows(&records, now_ms, month_start_ms(now));
    let month = month_key(now);

    let Some(budget) = budget else {
        return Ok(UsageGovernanceDto {
            month,
            month_cost_usd: month_cost,
            last7d_cost_usd: week_cost,
            budget_usd: None,
            percent: None,
            warned_80: false,
            hit_100: false,
            threshold_reached: None,
        });
    };

    let store = ThresholdMarkerStore::new();
    let decision = evaluate_thresholds(store.load(), &month, month_cost, budget);
    if decision.fire_80 {
        fire_budget_threshold_notification(&state.notifier, 80, month_cost, budget);
    }
    if decision.fire_100 {
        fire_budget_threshold_notification(&state.notifier, 100, month_cost, budget);
    }
    if decision.fire_80 || decision.fire_100 {
        // Best-effort: a failed write costs one repeat notification on the
        // next poll, never a broken request.
        if let Err(e) = store.save(&decision.next) {
            tracing::warn!(error = %e, "failed to persist usage threshold markers");
        }
    }

    let percent = Some(month_cost / budget * 100.0);
    let threshold_reached = match percent {
        Some(p) if p >= 100.0 => Some("100"),
        Some(p) if p >= 100.0 * MONTHLY_WARN_FRACTION => Some("80"),
        _ => None,
    };

    Ok(UsageGovernanceDto {
        month,
        month_cost_usd: month_cost,
        last7d_cost_usd: week_cost,
        budget_usd: Some(budget),
        percent,
        warned_80: decision.next.warned_80,
        hit_100: decision.next.hit_100,
        threshold_reached,
    })
}

/// Desktop notification for a newly reached monthly threshold. Pushed
/// through the shared notifier so the master-switch/DND prefs and the
/// webhook fan-out apply exactly as they do for query notifications.
/// English copy matches the other backend-fired notifications (the desktop
/// has no backend-side locale).
fn fire_budget_threshold_notification(
    notifier: &Notifier,
    threshold_pct: u32,
    spent_usd: f64,
    budget_usd: f64,
) {
    let body = if threshold_pct >= 100 {
        format!("Monthly budget reached: ${spent_usd:.2} of ${budget_usd:.2}.")
    } else {
        format!("Monthly spend at {threshold_pct}% of budget: ${spent_usd:.2} of ${budget_usd:.2}.")
    };
    let notification = Notification {
        title: "Shannon — monthly budget".to_string(),
        body,
        level: NotificationLevel::Warning,
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        source: Some(format!("usage_budget_{threshold_pct}")),
        action_id: None,
    };
    match notifier.notify(&notification) {
        Ok(()) => tracing::info!(threshold_pct, "monthly budget notification dispatched"),
        Err(e) => tracing::warn!(error = %e, "monthly budget notification dispatch failed"),
    }
}

// ── estimate_task_cost (P2-6) ─────────────────────────────────────────────

/// Read-only cost range for one routine (or the all-routines baseline when
/// the caller has no id yet — a routine being created has no history of its
/// own). `last_usd` is the most recent cost-tracked run; `runs_counted` is
/// capped at `ESTIMATE_RUN_LIMIT`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskCostEstimate {
    /// `false` = no cost-tracked history → the UI shows "first run, no
    /// estimate yet".
    pub has_history: bool,
    pub runs_counted: u32,
    pub min_usd: Option<f64>,
    pub max_usd: Option<f64>,
    pub avg_usd: Option<f64>,
    pub last_usd: Option<f64>,
}

/// Pure aggregation over run records: keeps the most recent `limit`
/// cost-tracked runs (any status — a failed run's tokens were still spent),
/// order-independent by sorting on `started_at`.
pub(crate) fn estimate_from_runs(runs: &[ScheduledRun], limit: usize) -> TaskCostEstimate {
    let mut tracked: Vec<(DateTime<Utc>, f64)> = runs
        .iter()
        .filter_map(|r| r.cost_usd.map(|c| (r.started_at, c)))
        .collect();
    tracked.sort_by(|a, b| b.0.cmp(&a.0));
    tracked.truncate(limit);

    if tracked.is_empty() {
        return TaskCostEstimate {
            has_history: false,
            runs_counted: 0,
            min_usd: None,
            max_usd: None,
            avg_usd: None,
            last_usd: None,
        };
    }

    let costs: Vec<f64> = tracked.iter().map(|(_, c)| *c).collect();
    let min = costs.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = costs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let sum: f64 = costs.iter().sum();
    TaskCostEstimate {
        has_history: true,
        runs_counted: costs.len() as u32,
        min_usd: Some(min),
        max_usd: Some(max),
        avg_usd: Some(sum / costs.len() as f64),
        last_usd: Some(costs[0]),
    }
}

/// Historical run-cost range for the pre-task estimate surfaces.
/// `taskId = null` aggregates across all routines (the "similar tasks"
/// baseline shown while creating a brand-new one); a non-empty id scopes to
/// that routine's own runs. Read-only, never blocks creation.
#[tauri::command]
pub async fn estimate_task_cost(task_id: Option<String>) -> Result<TaskCostEstimate, String> {
    let store = ScheduledRunsStore::new();
    // iter_all already folds revisions to the latest per run_id, so a
    // generous fetch window costs nothing; the pure core truncates.
    let runs = match task_id.as_deref().map(str::trim) {
        Some(id) if !id.is_empty() => store.list_by_task(id, 200),
        _ => store.list_recent(200),
    };
    let runs = runs.map_err(|e| e.to_string())?;
    Ok(estimate_from_runs(&runs, ESTIMATE_RUN_LIMIT))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::commands_usage::UsageStore;
    use chrono::Duration;

    fn rec(ts_ms: u64, cost: f64) -> UsageRecord {
        UsageRecord {
            timestamp_ms: ts_ms,
            model: "claude-sonnet-4-6".into(),
            provider: "anthropic".into(),
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            cost_usd: cost,
            session_id: None,
        }
    }

    fn run(started_at: DateTime<Utc>, cost: Option<f64>) -> ScheduledRun {
        let mut r = ScheduledRun::start("t1", "Task");
        r.started_at = started_at;
        r.cost_usd = cost;
        r
    }

    // ── evaluate_thresholds ──────────────────────────────────────────────

    fn markers(month: &str, warned: bool, hit: bool) -> Option<ThresholdMarkers> {
        Some(ThresholdMarkers {
            month: month.into(),
            warned_80: warned,
            hit_100: hit,
        })
    }

    #[test]
    fn threshold_fires_once_at_80_then_stays_quiet() {
        // 79% quiet, 80% (boundary inclusive) fires once, later checks quiet.
        let d = evaluate_thresholds(None, "2026-10", 7.9, 10.0);
        assert!(!d.fire_80 && !d.fire_100);

        let d = evaluate_thresholds(None, "2026-10", 8.0, 10.0);
        assert!(d.fire_80 && !d.fire_100);
        assert!(d.next.warned_80 && !d.next.hit_100);

        let d = evaluate_thresholds(markers("2026-10", true, false), "2026-10", 9.5, 10.0);
        assert!(!d.fire_80 && !d.fire_100);
    }

    #[test]
    fn threshold_fires_once_at_100_and_latches() {
        let d = evaluate_thresholds(markers("2026-10", true, false), "2026-10", 10.0, 10.0);
        assert!(d.fire_100 && !d.fire_80, "cap fires the 100% notice only");
        assert!(d.next.hit_100);

        // Still over the cap on later polls: no repeat.
        let d = evaluate_thresholds(Some(d.next), "2026-10", 12.0, 10.0);
        assert!(!d.fire_80 && !d.fire_100);
    }

    #[test]
    fn jump_straight_past_the_cap_fires_100_only() {
        let d = evaluate_thresholds(None, "2026-10", 15.0, 10.0);
        assert!(d.fire_100);
        assert!(!d.fire_80, "no lesser notice before/after the cap notice");
        assert!(d.next.warned_80 && d.next.hit_100, "both latched");
    }

    #[test]
    fn month_rollover_re_arms_both_thresholds() {
        // October latched at 100%; November starts clean and the 80% line
        // fires again (and re-latches in the new month's markers), while
        // the 100% flag stays unset until the cap is actually crossed.
        let stored = markers("2026-10", true, true);
        let d = evaluate_thresholds(stored, "2026-11", 8.5, 10.0);
        assert!(d.fire_80 && !d.fire_100);
        assert_eq!(d.next.month, "2026-11");
        assert!(d.next.warned_80, "the re-armed 80% notice latches again");
        assert!(!d.next.hit_100);

        // And the new month's cap crossing fires fresh.
        let d = evaluate_thresholds(Some(d.next), "2026-11", 10.0, 10.0);
        assert!(d.fire_100 && !d.fire_80);
    }

    #[test]
    fn stale_month_marker_from_a_clock_rollback_does_not_rearm_forward() {
        // Markers dated in the future (clock rollback) still match their
        // month only — a mismatched key re-arms, an exact match latches.
        let d = evaluate_thresholds(markers("2026-12", true, true), "2026-12", 11.0, 10.0);
        assert!(!d.fire_80 && !d.fire_100);
    }

    // ── window math ──────────────────────────────────────────────────────

    #[test]
    fn month_start_ms_is_local_midnight_on_the_first() {
        let now = Local.with_ymd_and_hms(2026, 10, 15, 13, 45, 0).unwrap();
        let expected = Local
            .with_ymd_and_hms(2026, 10, 1, 0, 0, 0)
            .unwrap()
            .timestamp_millis() as u64;
        assert_eq!(month_start_ms(now), expected);
        assert_eq!(month_key(now), "2026-10");
    }

    #[test]
    fn summarize_windows_splits_month_and_week() {
        let now = 1_700_000_000_000u64;
        let day = 86_400_000u64;
        // month_start = now - 10 days → the 12-day-old record is outside.
        let records = vec![
            rec(now, 0.10),
            rec(now - 5 * day, 0.20),
            rec(now - 9 * day, 0.40),
            rec(now - 12 * day, 9.99),
        ];
        let (month, week) = summarize_windows(&records, now, now - 10 * day);
        assert!((month - 0.70).abs() < 1e-9, "{month}");
        assert!((week - 0.30).abs() < 1e-9, "{week}");
    }

    #[test]
    fn governance_records_are_the_ledger_only() {
        // R2-W2-2: routine runs now carry their own cost on the run records
        // AND session-attributed ledger lines — folding the JSONL mirror in
        // would double-count that spend, so governance reads the ledger
        // verbatim even when cost-bearing run records exist beside it.
        let tmp = tempfile::tempdir().unwrap();
        let usage = UsageStore::with_path(tmp.path().join("usage.jsonl"));
        usage.append(&rec(1, 0.10)).unwrap();

        let mut run = ScheduledRun::start("t1", "Digest");
        run.started_at = Utc::now() - Duration::seconds(1);
        run.cost_usd = Some(0.42);
        ScheduledRunsStore::with_base(tmp.path().join("runs"))
            .record(&run)
            .unwrap();

        let now_ms = Utc::now().timestamp_millis().max(0) as u64;
        let records = collect_governance_records(&usage);
        assert_eq!(records.len(), 1, "ledger only — no folded run lump");
        let (month, _) = summarize_windows(&records, now_ms, 0);
        assert!((month - 0.10).abs() < 1e-9, "{month}");
    }

    // ── estimate_from_runs ───────────────────────────────────────────────

    #[test]
    fn estimate_without_history_reports_first_run() {
        let est = estimate_from_runs(&[], ESTIMATE_RUN_LIMIT);
        assert!(!est.has_history);
        assert_eq!(est.runs_counted, 0);
        assert_eq!(est.min_usd, None);
        assert_eq!(est.last_usd, None);

        // Runs that tracked no cost (never reached the accounting point)
        // are not history either.
        let now = Utc::now();
        let est = estimate_from_runs(&[run(now, None), run(now, None)], ESTIMATE_RUN_LIMIT);
        assert!(!est.has_history);
    }

    #[test]
    fn estimate_with_history_gives_min_max_avg_last() {
        let now = Utc::now();
        let runs = vec![
            run(now - Duration::hours(3), Some(0.10)),
            run(now - Duration::hours(2), Some(0.30)),
            run(now - Duration::hours(1), Some(0.20)),
            run(now, None), // untracked run: ignored, but recency anchor
        ];
        let est = estimate_from_runs(&runs, ESTIMATE_RUN_LIMIT);
        assert!(est.has_history);
        assert_eq!(est.runs_counted, 3);
        assert!((est.min_usd.unwrap() - 0.10).abs() < 1e-9);
        assert!((est.max_usd.unwrap() - 0.30).abs() < 1e-9);
        assert!((est.avg_usd.unwrap() - 0.20).abs() < 1e-9);
        // "last" is the most recent *tracked* run (0.20), not the min/max.
        assert!((est.last_usd.unwrap() - 0.20).abs() < 1e-9);
    }

    #[test]
    fn estimate_is_order_independent_and_respects_the_limit() {
        let now = Utc::now();
        // Shuffled input → same result as the sorted variant.
        let a = vec![
            run(now - Duration::hours(1), Some(0.30)),
            run(now, Some(0.10)),
        ];
        let b = vec![
            run(now, Some(0.10)),
            run(now - Duration::hours(1), Some(0.30)),
        ];
        assert_eq!(estimate_from_runs(&a, 10), estimate_from_runs(&b, 10));

        // Only the most recent `limit` cost-tracked runs count.
        let many: Vec<ScheduledRun> = (0..30)
            .map(|i| run(now - Duration::hours(i), Some(i as f64)))
            .collect();
        let est = estimate_from_runs(&many, 5);
        assert_eq!(est.runs_counted, 5);
        assert!((est.min_usd.unwrap() - 0.0).abs() < 1e-9);
        assert!((est.max_usd.unwrap() - 4.0).abs() < 1e-9);
        assert!((est.last_usd.unwrap() - 0.0).abs() < 1e-9);
    }

    // ── marker store round trip ──────────────────────────────────────────

    #[test]
    fn marker_store_round_trips_and_tolerates_missing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ThresholdMarkerStore::with_path(tmp.path().join("nested").join("markers.json"));
        assert!(store.load().is_none(), "missing file → no markers");

        let m = ThresholdMarkers {
            month: "2026-10".into(),
            warned_80: true,
            hit_100: false,
        };
        store.save(&m).unwrap();
        assert_eq!(store.load(), Some(m));

        // A corrupt file degrades to None (one repeat alert worst case).
        let corrupt = ThresholdMarkerStore::with_path(tmp.path().join("corrupt.json"));
        std::fs::write(tmp.path().join("corrupt.json"), "{not json").unwrap();
        assert!(corrupt.load().is_none());
    }

    // ── DTO frozen shape ─────────────────────────────────────────────────

    #[test]
    fn governance_dto_is_frozen_camel_case() {
        let dto = UsageGovernanceDto {
            month: "2026-10".into(),
            month_cost_usd: 1.5,
            last7d_cost_usd: 0.7,
            budget_usd: Some(10.0),
            percent: Some(15.0),
            warned_80: false,
            hit_100: false,
            threshold_reached: None,
        };
        let json = serde_json::to_value(&dto).unwrap();
        for key in [
            "month",
            "monthCostUsd",
            "last7dCostUsd",
            "budgetUsd",
            "percent",
            "warned80",
            "hit100",
            "thresholdReached",
        ] {
            assert!(json.get(key).is_some(), "missing {key} in {json}");
        }
    }

    #[test]
    fn task_cost_estimate_dto_is_frozen_camel_case() {
        let est = TaskCostEstimate {
            has_history: true,
            runs_counted: 3,
            min_usd: Some(0.1),
            max_usd: Some(0.3),
            avg_usd: Some(0.2),
            last_usd: Some(0.2),
        };
        let json = serde_json::to_value(&est).unwrap();
        for key in [
            "hasHistory",
            "runsCounted",
            "minUsd",
            "maxUsd",
            "avgUsd",
            "lastUsd",
        ] {
            assert!(json.get(key).is_some(), "missing {key} in {json}");
        }
    }
}
