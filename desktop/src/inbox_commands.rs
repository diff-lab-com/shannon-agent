//! Tauri IPC commands for the SQLite inbox (P0-3 backend).
//!
//! Storage lives in `~/.shannon/inbox.db` via
//! [`shannon_core::inbox_store::InboxStore`]. This module owns:
//!
//! - the five frontend-facing commands (`list_inbox_items`,
//!   `update_inbox_item_status`, `get_inbox_stats`, `rerun_inbox_item`,
//!   `continue_inbox_item_session`) — **command names and payload shapes are
//!   an interface contract with the desktop UI task and must stay verbatim**;
//! - `spawn_routine_run`, the shared async executor used by both
//!   `rerun_inbox_item` and the loopback `POST /api/routines/:id/trigger`
//!   endpoint. It mirrors the unattended execution path of
//!   `commands::start_background_task` (fresh `QueryEngine`, configured
//!   approval mode, usage ledger writes) and, on completion, writes:
//!     1. the run finish into the legacy JSONL history (same run id — the
//!        mirror is retained for read-fallback and export; T7 made the
//!        SQLite `routine_runs` table the authoritative read source),
//!     2. the inbox item (`source=routine|scheduled_task|trigger`), summary
//!        truncated to ≤500 chars,
//!     3. the `inbox_item_id` back-link on the `routine_runs` row,
//!     4. an `inbox-updated` event so the UI refreshes without polling.
//! - the T7 history read path (`read_run_history`, used by
//!   `list_task_executions`): SQLite first, JSONL fallback + warning, plus
//!   the idempotent startup backfill (`spawn_run_history_backfill`) that
//!   imports JSONL-only runs into `routine_runs`.
//!
//! The legacy triage commands in `scheduled_commands.rs` are untouched (the
//! UI migration to this store happens in the frontend task).

use chrono::{Datelike as _, TimeZone as _};
use shannon_core::inbox_store::{
    InboxItem, InboxItemNew, InboxStats, InboxStatus, InboxStore, InboxStoreError, RunRecord,
};
use shannon_core::query_engine::{QueryContext, QueryEngine, QueryEvent, QueryMetadata};
use shannon_core::scheduled_retry::{RetryDecision, RetryPolicy};
use shannon_core::scheduled_routines::ScheduledRoutine;
use shannon_core::scheduled_runs::{RunStatus, ScheduledRun, ScheduledRunsStore};
use shannon_engine::api::client::LlmClient;
use shannon_engine::permissions::{PermissionManager, PermissionRuleChecker};
use shannon_engine::state::StateManager;
use std::time::Duration;
use tauri::Emitter;
use tokio::sync::RwLock;

use crate::commands::AppState;
use crate::config::DesktopConfig;
use crate::events::event_names;
use crate::scheduled_commands::TaskExecution;

/// Max length of the generated inbox item summary (brief: ≤500 chars).
const SUMMARY_MAX_CHARS: usize = 500;

/// Scan cap for the one-shot history backfill. The JSONL store's
/// `prune_old` currently has **no production caller**, so the store is not
/// pruned in practice — this cap is purely defensive: it keeps a runaway
/// (oversized) runs directory from wedging startup, at the cost of leaving
/// runs beyond the newest 10 000 unbackfilled until the store shrinks.
const BACKFILL_SCAN_LIMIT: usize = 10_000;

/// Sources that can be rerun through the scheduled-task execution path.
const RERUNNABLE_SOURCES: [&str; 3] = [
    shannon_core::inbox_store::SOURCE_ROUTINE,
    shannon_core::inbox_store::SOURCE_SCHEDULED_TASK,
    shannon_core::inbox_store::SOURCE_TRIGGER,
];

// ── Tauri commands (frontend contract — do not rename/reshape) ──────────

/// List inbox items, newest first.
#[tauri::command]
pub async fn list_inbox_items(
    state: tauri::State<'_, AppState>,
    status: Option<String>,
    source: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<InboxItem>, String> {
    let status = status
        .map(|s| InboxStatus::parse(&s))
        .transpose()
        .map_err(|e| e.to_string())?;
    state
        .inbox_store()
        .list(status, source.as_deref(), limit.unwrap_or(100))
        .map_err(|e| e.to_string())
}

/// Transition an inbox item to `pending` / `read` / `archived`.
///
/// Emits `inbox-updated` so badges refresh without polling.
#[tauri::command]
pub async fn update_inbox_item_status(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: i64,
    status: String,
) -> Result<(), String> {
    let status = InboxStatus::parse(&status).map_err(|e| e.to_string())?;
    state
        .inbox_store()
        .update_status(id, status)
        .map_err(|e| e.to_string())?;
    let _ = app_handle.emit(event_names::INBOX_UPDATED, id);
    Ok(())
}

/// Badge counts: `{ pending, today }`.
#[tauri::command]
pub async fn get_inbox_stats(state: tauri::State<'_, AppState>) -> Result<InboxStats, String> {
    state.inbox_store().stats().map_err(|e| e.to_string())
}

/// Re-run the routine / scheduled task behind an inbox item through the
/// existing unattended execution path. Returns the new run id.
#[tauri::command]
pub async fn rerun_inbox_item(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: i64,
) -> Result<String, String> {
    let inbox = state.inbox_store();
    let item = inbox
        .get_item(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("inbox item not found: {id}"))?;

    if !RERUNNABLE_SOURCES.contains(&item.source.as_str()) {
        return Err(format!(
            "inbox item source '{}' cannot be rerun",
            item.source
        ));
    }
    let task_id = item
        .source_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("inbox item {id} has no source task id"))?;

    let routine = state
        .scheduled_task_store()
        .load(task_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("task not found: {task_id}"))?;

    let deps = RoutineRunDeps::from_state(&state);
    spawn_routine_run(&deps, app_handle, routine, &item.source, None).await
}

/// Return the session id linked to an inbox item so the frontend can resume
/// the conversation with the existing session-loading commands.
#[tauri::command]
pub async fn continue_inbox_item_session(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<String, String> {
    let item = state
        .inbox_store()
        .get_item(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("inbox item not found: {id}"))?;
    item.session_id
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("inbox item {id} has no linked session"))
}

// ── T7: authoritative run-history read path ──────────────────────────────
//
// Since T7 the SQLite `routine_runs` table (same inbox.db, owned by
// `InboxStore`) is the authoritative read source for the History view;
// the legacy JSONL store (`~/.shannon/scheduled-runs/`) is demoted to
// (a) a best-effort write mirror — retained for one release so a downgrade
// or fallback keeps working, and (b) the read fallback below, so the UI
// always has data even when the inbox store cannot be opened or queried.

/// Milliseconds → whole seconds for the legacy second-resolution timestamps.
fn ms_to_secs(ms: Option<i64>) -> Option<i64> {
    ms.map(|v| v.div_euclid(1000))
}

/// Project a `routine_runs` row onto the legacy `TaskExecution` contract.
///
/// Mapping rules (JSONL field ↔ `routine_runs` column) — the run ids are
/// the same string by construction (`spawn_routine_run` mints the JSONL
/// mirror with the SQLite run id), so `get_execution_detail` keeps
/// resolving projected rows:
/// - `run_id` ↔ `id`
/// - `started_at`/`finished_at` ↔ `started_at_ms`/`finished_at_ms` (ms → s)
/// - `status` ↔ `status` (both stores share the lowercase vocabulary:
///   running/succeeded/failed plus the scheduler-mirrored queued/cancelled)
/// - `error_message` ↔ `error`
/// - `task_name` ↔ `task_name`, falling back to the task id (JSONL always
///   carried a name; the column is nullable)
/// - `cost_usd`/`token_usage` ↔ the run's cost columns verbatim (R2-W2-2:
///   the finalize path populates them; runs from before cost tracking read
///   back `None` and the UI keeps hiding their cost cells — no estimates).
pub(crate) fn run_record_to_execution(run: &RunRecord) -> TaskExecution {
    TaskExecution {
        run_id: run.id.clone(),
        task_id: run.task_id.clone(),
        task_name: run
            .task_name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| run.task_id.clone()),
        started_at: ms_to_secs(run.started_at_ms).unwrap_or(0),
        finished_at: ms_to_secs(run.finished_at_ms),
        status: run.status.clone(),
        error_message: run.error.clone(),
        cost_usd: run.cost_usd,
        token_usage: run.token_usage,
    }
}

/// Map a legacy JSONL run onto the `routine_runs` schema (backfill
/// direction). Status uses the same `Debug`-lowercase rendering as the
/// JSONL → `TaskExecution` projection, so the two projections of one run
/// agree field for field (asserted by the sampling test below).
pub(crate) fn scheduled_run_to_record(run: &ScheduledRun) -> RunRecord {
    let started_ms = run.started_at.timestamp_millis();
    let finished_ms = run.finished_at.map(|t| t.timestamp_millis());
    RunRecord {
        id: run.run_id.clone(),
        task_id: run.task_id.clone(),
        task_name: Some(run.task_name.clone()),
        status: format!("{:?}", run.status).to_lowercase(),
        error: run.error_message.clone(),
        started_at_ms: Some(started_ms),
        finished_at_ms: finished_ms,
        duration_ms: finished_ms.map(|f| (f - started_ms).max(0)),
        inbox_item_id: None,
        cost_usd: run.cost_usd,
        token_usage: run.token_usage,
    }
}

/// Read the run history backing `list_task_executions`: the inbox store
/// (`routine_runs`) first, falling back to the legacy JSONL store with a
/// warning when the inbox read fails — the UI must always get data.
///
/// `read_inbox` is injected so tests can drive both branches (a healthy
/// `InboxStore` cannot fail on demand).
pub(crate) fn read_run_history<F>(
    read_inbox: F,
    jsonl: &ScheduledRunsStore,
    task_id: Option<&str>,
    limit: usize,
) -> Result<Vec<TaskExecution>, String>
where
    F: FnOnce() -> Result<Vec<RunRecord>, InboxStoreError>,
{
    match read_inbox() {
        Ok(rows) => Ok(rows.iter().map(run_record_to_execution).collect()),
        Err(e) => {
            tracing::warn!(
                error = %e,
                task_id = ?task_id,
                "history: inbox store read failed — falling back to the legacy JSONL runs store"
            );
            let runs = match task_id {
                Some(id) => jsonl.list_by_task(id, limit),
                None => jsonl.list_recent(limit),
            }
            .map_err(|e| e.to_string())?;
            Ok(runs
                .iter()
                .map(crate::scheduled_commands::run_to_execution)
                .collect())
        }
    }
}

/// Read one run's detail row backing `get_execution_detail`, fully
/// symmetric to [`read_run_history`]: the inbox store (`routine_runs`)
/// first — projected with the same [`run_record_to_execution`] mapping the
/// list path uses — falling back to the legacy JSONL `find_by_id`
/// (behaviour fully preserved) when the run is not in `routine_runs` yet
/// (e.g. a JSONL-only `running` placeholder the startup backfill
/// deliberately skips) or when the inbox read fails (warn only, never
/// surfaces). Without the fallback a JSONL mirror write that failed while
/// its SQLite write succeeded would list the run in History but 404 here.
///
/// `read_inbox` is injected so tests can drive all three branches (a
/// healthy `InboxStore` can neither miss nor fail on demand).
pub(crate) fn read_run_detail<F>(
    read_inbox: F,
    jsonl: &ScheduledRunsStore,
    run_id: &str,
) -> Result<TaskExecution, String>
where
    F: FnOnce() -> Result<Option<RunRecord>, InboxStoreError>,
{
    let from_jsonl = || -> Result<TaskExecution, String> {
        let run = jsonl
            .find_by_id(run_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("run not found: {run_id}"))?;
        Ok(crate::scheduled_commands::run_to_execution(&run))
    };
    match read_inbox() {
        Ok(Some(record)) => Ok(run_record_to_execution(&record)),
        // Not in `routine_runs` (pre-backfill legacy run / JSONL-only
        // placeholder) — a normal miss, so no warning: the JSONL store is
        // the read fallback, same as the T7 list path.
        Ok(None) => from_jsonl(),
        Err(e) => {
            tracing::warn!(
                error = %e,
                run_id = %run_id,
                "execution detail: inbox store read failed — falling back to the legacy JSONL runs store"
            );
            from_jsonl()
        }
    }
}

/// One-shot best-effort backfill: import every JSONL run that has no
/// `routine_runs` row yet, so the authoritative read path keeps showing
/// history that predates the switch. Idempotent — [`InboxStore::import_run`]
/// keys on the run id, so re-running (every startup) never duplicates.
///
/// JSONL rows still in `running` are **skipped**: they are ghosts (a
/// drained placeholder whose process died between drain and retire —
/// since P0-4 `trigger_task_now` spawns real runs and writes no
/// placeholder anymore). A run that truly started already owns
/// its SQLite row (`record_run_start` at spawn time), so nothing genuine is
/// lost — and importing ghosts would both show fake `running` history and
/// trip the scheduler's 24h in-flight guard for the task.
///
/// Backfilled rows land in `routine_runs` only; `inbox_items` is not
/// touched, so unread inbox counts stay clean (the "archived, 不打扰"
/// intent of the T7 ruling) while the full history stays queryable.
/// Returns `(imported, already_present)`.
pub(crate) fn backfill_runs_from_jsonl(
    inbox: &InboxStore,
    jsonl: &ScheduledRunsStore,
) -> Result<(usize, usize), String> {
    let runs = jsonl
        .list_recent(BACKFILL_SCAN_LIMIT)
        .map_err(|e| e.to_string())?;
    let mut imported = 0usize;
    let mut present = 0usize;
    for run in &runs {
        if run.status == RunStatus::Running {
            continue;
        }
        match inbox.import_run(&scheduled_run_to_record(run)) {
            Ok(true) => imported += 1,
            Ok(false) => present += 1,
            Err(e) => {
                // Per-row failure is tolerated: the JSONL fallback keeps the
                // run visible and the next startup retries the import.
                tracing::warn!(
                    run_id = %run.run_id,
                    error = %e,
                    "history backfill: failed to import run; keeping JSONL copy"
                );
            }
        }
    }
    Ok((imported, present))
}

/// Startup hook: run the history backfill off the UI path. Best-effort —
/// failures warn only and never block app start.
pub fn spawn_run_history_backfill(state: &AppState) {
    let inbox = state.inbox_store();
    let jsonl = state.scheduled_runs_store.clone();
    tauri::async_runtime::spawn(async move {
        match backfill_runs_from_jsonl(&inbox, &jsonl) {
            Ok((imported, present)) if imported > 0 => tracing::info!(
                imported,
                present,
                "history backfill: legacy JSONL runs imported into the inbox store"
            ),
            Ok((_, _)) => {}
            Err(e) => tracing::warn!(
                error = %e,
                "history backfill: legacy JSONL scan failed; JSONL stays available via the read fallback"
            ),
        }
    });
}

// ── Shared execution path ────────────────────────────────────────────────

/// P2-5: resolve the off-peak model override for a routine run.
///
/// The `offpeak.model_override` config applies iff ALL of:
/// - the routine has an `execution_window` in its policy,
/// - an override is configured and non-empty (empty = disabled), and
/// - the run starts inside the window.
///
/// Pure in `now` so tests inject the clock; `spawn_routine_run` calls it
/// with the real clock at spawn time. Reruns and loopback triggers get the
/// same treatment because they funnel through the same executor.
pub(crate) fn resolve_offpeak_model(
    routine: &ScheduledRoutine,
    configured_override: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
    let configured = configured_override
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let window = routine.policy.as_ref()?.execution_window.as_ref()?;
    if window.contains_utc(now) {
        Some(configured.to_string())
    } else {
        None
    }
}

// ── P1-2: ExecutionPolicy wiring (timeout / retries / budget / worktree) ──

/// Scan cap for the budget aggregation. A routine firing every minute for a
/// year produces ~5×10⁵ runs; the budget check only needs a broad "how much
/// has this routine already spent" figure, so the newest 500 runs are plenty
/// and the SQL stays cheap.
const BUDGET_RUN_SCAN: u32 = 500;

/// The executor-side slice of a routine's `ExecutionPolicy`
/// (`shannon_core::scheduled_routines`) — the fields `spawn_routine_run`
/// actually enforces now that the wiring exists:
///
/// - `timeout_secs > 0` → each attempt is aborted after that long and the
///   run finalizes as failed with the timeout as the recorded reason
///   (`0` = unlimited, the legacy behavior);
/// - `max_retries` → failed attempts are retried with the
///   `scheduled_retry` exponential-backoff + jitter policy. Per core's
///   convention the number counts **total attempts** (original run
///   included): `max_retries: 3` runs the prompt up to three times.
pub(crate) struct RunExecutionPolicy {
    timeout: Option<Duration>,
    retry: RetryPolicy,
}

impl RunExecutionPolicy {
    /// Derive from the routine's stored policy; a missing policy keeps the
    /// fully legacy behavior (no timeout, no retries).
    pub(crate) fn of(routine: &ScheduledRoutine) -> Self {
        let policy = routine.policy.as_ref();
        Self {
            timeout: match policy.map(|p| p.timeout_secs).unwrap_or(0) {
                0 => None,
                secs => Some(Duration::from_secs(secs)),
            },
            retry: RetryPolicy::from_max_retries(policy.map(|p| p.max_retries).unwrap_or(0)),
        }
    }
}

/// Error prefix carried by a budget-aborted attempt's outcome so the outer
/// run task (and `scheduled_retry::is_retryable_error`) recognizes the stop
/// as terminal (R2-W2-3).
pub(crate) const BUDGET_ABORT_MARKER: &str = "budget_exceeded";

/// Poll cadence of the mid-run budget guard (R2-W2-3). Billing resolution of
/// the abort — Usage events land in the tracker as the engine emits them, so
/// the stop trails the cap by at most one poll plus one stream chunk.
const BUDGET_ABORT_POLL: Duration = Duration::from_secs(2);

// ── W3-2: consecutive-failure auto pause + failure notifications ──────────

/// Consecutive scheduled-fire failures after which a routine pauses itself
/// (W3-2, R2-P2-C). A constant by design — no new configuration surface.
pub(crate) const AUTO_PAUSE_FAILURE_THRESHOLD: u32 = 3;

/// Machine-readable run-record reason prefix for the pausing failure, in the
/// [`BUDGET_ABORT_MARKER`] style (prefix survives summary truncation).
pub(crate) const AUTO_PAUSE_MARKER: &str = "auto_paused_after_3_failures";

/// Scan cap when re-deriving the failure streak from SQLite at finalize
/// time. Only the newest few terminal runs matter; the cap bounds the query
/// for pathological histories.
const AUTO_PAUSE_RUN_SCAN: u32 = 25;

/// Await one engine attempt, converting a panic, a policy timeout, **or a
/// mid-run budget trip** into a failed [`RunOutcome`] instead of leaving the
/// run `running` forever.
///
/// On timeout or budget trip the spawned engine task is aborted (dropping a
/// `JoinHandle` alone only detaches — the query stream would keep running
/// and billing), the task is reaped, and the reason becomes the recorded
/// failure. The budget arm shares the timeout's abort channel: a `watch`
/// signal trips the same abort-the-`JoinHandle` path.
async fn run_with_timeout<F>(
    engine_future: F,
    timeout: Option<Duration>,
    budget_abort: Option<tokio::sync::watch::Receiver<bool>>,
) -> RunOutcome
where
    F: std::future::Future<Output = RunOutcome> + Send + 'static,
{
    let mut handle = tokio::spawn(engine_future);
    match race_attempt(&mut handle, timeout, budget_abort).await {
        AttemptRace::Finished(Ok(outcome)) => outcome,
        AttemptRace::Finished(Err(join_error)) => RunOutcome::panicked(join_error.to_string()),
        AttemptRace::BudgetTripped => {
            handle.abort();
            let _ = handle.await;
            tracing::warn!(
                marker = BUDGET_ABORT_MARKER,
                "routine run: budget cap reached mid-execution — attempt aborted"
            );
            RunOutcome {
                failed: true,
                error: Some(format!(
                    "{BUDGET_ABORT_MARKER}: monthly budget cap reached mid-run"
                )),
                output: String::new(),
                session_id: None,
                cost_usd: None,
                token_usage: None,
            }
        }
    }
}

/// Verdict of one attempt's race between completion and the budget stop.
enum AttemptRace {
    Finished(Result<RunOutcome, tokio::task::JoinError>),
    BudgetTripped,
}

/// Race the attempt `JoinHandle` against the timeout and the budget stop
/// signal. Borrows the handle only for the race — the caller keeps ownership
/// so it can abort and reap the task after the verdict.
async fn race_attempt(
    handle: &mut tokio::task::JoinHandle<RunOutcome>,
    timeout: Option<Duration>,
    mut budget_abort: Option<tokio::sync::watch::Receiver<bool>>,
) -> AttemptRace {
    // Resolves only when the shared budget flag trips. With `None` (no
    // budget) — or a sender gone without tripping (the run task finished and
    // dropped the watcher) — the arm stays pending forever and never wins.
    let budget_fired = async {
        match budget_abort.as_mut() {
            Some(rx) => loop {
                if *rx.borrow_and_update() {
                    break;
                }
                if rx.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            },
            None => std::future::pending::<()>().await,
        }
    };
    let attempt = async {
        match timeout {
            None => handle.await,
            Some(limit) => match tokio::time::timeout(limit, &mut *handle).await {
                Ok(res) => res,
                Err(_elapsed) => {
                    handle.abort();
                    let _ = handle.await;
                    tracing::warn!(
                        timeout_secs = limit.as_secs(),
                        "routine run: execution timed out — attempt aborted"
                    );
                    Ok(RunOutcome {
                        failed: true,
                        error: Some(format!("routine run timed out after {}s", limit.as_secs())),
                        output: String::new(),
                        session_id: None,
                        cost_usd: None,
                        token_usage: None,
                    })
                }
            },
        }
    };
    // `biased` with the attempt arm first: when the engine completes at the
    // same tick the watcher trips, the finished outcome wins — a completed
    // run is not retroactively aborted (its spend lands on the record either
    // way, and the finalize path still annotates the trip).
    tokio::select! {
        biased;
        res = attempt => AttemptRace::Finished(res),
        _ = budget_fired => AttemptRace::BudgetTripped,
    }
}

/// Execute the engine phase under the routine's timeout/retry policy
/// (P1-2). Each attempt gets a fresh engine future (streams are
/// single-shot) and its own timeout; a failed attempt is retried with the
/// `scheduled_retry` backoff while the retry budget allows and the error
/// looks transient. The final outcome carries the give-up context so the
/// run record shows how many attempts were made and why they stopped.
///
/// W2-3: when `budget_abort` is `Some`, a trip aborts the attempt (see
/// [`run_with_timeout`]) and ends the whole run — a spent-up cap is not
/// transient, so the retry loop returns the budget outcome untouched
/// instead of scheduling another paid attempt.
pub(crate) async fn execute_with_policy<F, Fut>(
    mut engine: F,
    policy: &RunExecutionPolicy,
    budget_abort: Option<tokio::sync::watch::Receiver<bool>>,
) -> RunOutcome
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = RunOutcome> + Send + 'static,
{
    let mut attempt: u32 = 1;
    loop {
        let outcome = run_with_timeout(engine(), policy.timeout, budget_abort.clone()).await;
        if !outcome.failed {
            return outcome;
        }
        if budget_abort.as_ref().is_some_and(|rx| *rx.borrow()) {
            return outcome;
        }
        let error = outcome.error.clone().unwrap_or_default();
        let verdict = shannon_core::scheduled_retry::decide_retry(&policy.retry, attempt, &error);
        match verdict.decision {
            RetryDecision::Retry {
                next_attempt,
                run_at: _,
            } => {
                tracing::warn!(
                    attempt,
                    next_attempt,
                    delay_ms = verdict.delay.as_millis() as u64,
                    error = %error,
                    "routine run: attempt failed — retrying with backoff"
                );
                tokio::time::sleep(verdict.delay).await;
                attempt = next_attempt;
            }
            RetryDecision::GiveUp { reason } => {
                let mut final_outcome = outcome;
                final_outcome.error = Some(format!(
                    "{error} (gave up after {attempt} attempt(s): {reason:?})"
                ));
                return final_outcome;
            }
        }
    }
}

/// Cumulative usage-ledger spend attributed to `task_id`'s runs within the
/// current calendar month (UTC) — the routine dimension of the budget gate.
///
/// Attribution path: run row → back-linked inbox item → session id → usage
/// ledger lines carrying that session id. Runs whose usage predates the
/// session attribution fix are invisible here (their ledger lines carry no
/// session id); the budget gate under-counts rather than over-counts.
pub(crate) fn routine_month_spend(
    usage: &crate::commands_usage::UsageStore,
    inbox: &InboxStore,
    task_id: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> f64 {
    let runs = match inbox.list_runs_by_task(task_id, BUDGET_RUN_SCAN) {
        Ok(runs) => runs,
        Err(e) => {
            tracing::warn!(
                task_id = task_id,
                error = %e,
                "routine budget: run history unreadable — spend reported as 0"
            );
            return 0.0;
        }
    };
    let mut sessions: std::collections::HashSet<String> = std::collections::HashSet::new();
    for run in runs {
        let Some(item_id) = run.inbox_item_id else {
            continue;
        };
        if let Ok(Some(item)) = inbox.get_item(item_id) {
            if let Some(session_id) = item.session_id.filter(|s| !s.trim().is_empty()) {
                sessions.insert(session_id);
            }
        }
    }
    if sessions.is_empty() {
        return 0.0;
    }
    let month_start = now
        .date_naive()
        .with_day(1)
        .unwrap_or_else(|| now.date_naive())
        .and_hms_opt(0, 0, 0)
        .unwrap_or_else(|| now.naive_utc());
    let month_start_ms = chrono::Utc
        .from_utc_datetime(&month_start)
        .timestamp_millis();
    usage
        .load()
        .iter()
        .filter(|record| record.timestamp_ms as i64 >= month_start_ms)
        .filter(|record| {
            record
                .session_id
                .as_deref()
                .is_some_and(|s| sessions.contains(s))
        })
        .map(|record| record.cost_usd)
        .sum()
}

/// One poll of the mid-run budget guard (W2-3): `true` once the routine's
/// month aggregate plus the in-flight run's last-known spend has reached the
/// configured cap. Same aggregation basis as [`budget_skip_reason`] (month,
/// session-attributed) plus the in-flight increment that aggregation cannot
/// see yet — the run's inbox item (and with it the session back-link the
/// aggregation walks) only exists after finalize.
pub(crate) fn budget_live_exceeded(
    usage: &crate::commands_usage::UsageStore,
    inbox: &InboxStore,
    task_id: &str,
    cap: f64,
    in_flight_usd: Option<f64>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    let spent = routine_month_spend(usage, inbox, task_id, now) + in_flight_usd.unwrap_or(0.0);
    spent >= cap
}

/// The P1-2 budget gate: `Some(reason)` when the routine has a monthly
/// budget configured and the attributed spend has reached it — the caller
/// skips execution and records the reason on the run. A missing policy, a
/// zero cap (treated as "no budget"), or spend below the cap yields `None`.
pub(crate) fn budget_skip_reason(
    deps: &RoutineRunDeps,
    routine: &ScheduledRoutine,
) -> Option<String> {
    let cap = routine
        .policy
        .as_ref()?
        .budget_usd
        .filter(|cap| *cap > 0.0)?;
    let spent = routine_month_spend(
        &deps.usage_store,
        &deps.inbox,
        &routine.id,
        chrono::Utc::now(),
    );
    if spent >= cap {
        Some(format!(
            "skipped: monthly budget ${spent:.2} of ${cap:.2} exceeded"
        ))
    } else {
        None
    }
}

/// Resolve the effective per-run working directory (P1-2 `worktree` wiring).
///
/// With the policy flag unset this is the routine's `working_dir` sidecar
/// (P-E1 behavior, unchanged). With `policy.worktree` set, the run is pinned
/// to the task's scheduled-worktree directory under `base_dir` (the same
/// `.shannon/scheduled-worktrees/<slug>-<id>/` layout the Workspaces tab's
/// management surface lists and prunes):
///
/// 1. existing directory → used as-is (created earlier via the Workspaces
///    tab or a previous run);
/// 2. missing but the routine has a project dir → the worktree is forked
///    from that repo's `HEAD` (auto-create);
/// 3. missing and nothing to fork from (or git fails) → warning logged and
///    the run degrades to the plain sidecar dir (no isolation).
///
/// The engine's per-run working directory is used — the process cwd is
/// never touched (background threads must not move global state).
pub(crate) fn resolve_run_working_dir(
    routine: &ScheduledRoutine,
    sidecar_dir: Option<&str>,
    base_dir: &std::path::Path,
) -> Option<String> {
    let sidecar = sidecar_dir
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_string);
    let worktree_requested = routine
        .policy
        .as_ref()
        .and_then(|p| p.worktree.as_deref())
        .map(|w| !w.trim().is_empty())
        .unwrap_or(false);
    if !worktree_requested {
        return sidecar;
    }
    let dir_name =
        shannon_core::scheduled_worktree::ScheduledWorktree::dir_name(&routine.id, &routine.name);
    let worktree_dir = base_dir.join(&dir_name);
    if worktree_dir.is_dir() {
        return Some(worktree_dir.to_string_lossy().into_owned());
    }
    if let Some(repo) = sidecar.as_deref() {
        let repo = crate::commands_projects::normalize_path(repo);
        match shannon_core::scheduled_worktree::create_named(
            std::path::Path::new(repo),
            base_dir,
            &dir_name,
            &shannon_core::scheduled_worktree::ScheduledWorktree::branch_name(
                &routine.id,
                &routine.name,
            ),
            "HEAD",
        ) {
            Ok(path) => return Some(path.to_string_lossy().into_owned()),
            Err(e) => tracing::warn!(
                task_id = %routine.id,
                error = %e,
                "routine run: worktree creation failed — running without isolation"
            ),
        }
    } else {
        tracing::warn!(
            task_id = %routine.id,
            "routine run: worktree policy set but the routine has no project dir to fork from — running without isolation"
        );
    }
    sidecar
}

/// The legacy JSONL mirror seam. Object-safe on purpose: tests inject a
/// failing implementation to prove a mirror outage can never wedge the
/// SQLite `routine_runs` row (review fix round 1, Important 1).
pub(crate) trait RunMirror: Send + Sync {
    /// Append the initial `Running` record.
    fn record_start(&self, run: &ScheduledRun) -> Result<(), String>;
    /// Append the finish revision for `run_id`, carrying the run's cost/token
    /// totals (R2-W2-2 — `None`s keep the legacy "not tracked" shape).
    fn record_finish(
        &self,
        run_id: &str,
        status: RunStatus,
        error: Option<String>,
        spend: RunSpend,
    ) -> Result<(), String>;
}

impl RunMirror for shannon_core::scheduled_runs::ScheduledRunsStore {
    fn record_start(&self, run: &ScheduledRun) -> Result<(), String> {
        self.record(run).map(|_| ()).map_err(|e| e.to_string())
    }

    fn record_finish(
        &self,
        run_id: &str,
        status: RunStatus,
        error: Option<String>,
        spend: RunSpend,
    ) -> Result<(), String> {
        self.update(run_id, |r| {
            // The mirror carries the same spend the authoritative SQLite row
            // gets; the Usage page keeps reading the ledger, not the mirror.
            r.cost_usd = spend.cost_usd;
            r.token_usage = spend.token_usage;
            r.finish(status, error);
        })
        .map_err(|e| e.to_string())
    }
}

/// The routine-finish webhook seam (office Wave 2 B6'). When a routine has
/// `notify_webhook` set, [`finalize_run`] asks this port whether the user has
/// a webhook sink configured and, if so, hands it the run-finish
/// notification (task name + status + output summary). Object-safe so tests
/// can record deliveries instead of posting to the network.
pub(crate) trait RoutineWebhookPort: Send + Sync {
    /// Whether a `[notifications.webhook]` sink is configured.
    fn is_configured(&self) -> bool;
    /// Fire-and-forget delivery of the run-finish notification.
    fn deliver(&self, notification: &shannon_core::notifier::Notification);
}

/// Production port. Resolves `[notifications.webhook]` through the same
/// read path as the webhook settings commands
/// ([`crate::commands_notifications::load_desktop_webhook_config`]) and
/// delivers via core's [`shannon_core::notifier::WebhookHandler`] — the
/// existing template/signing/fire-and-forget machinery, nothing new on the
/// wire.
pub(crate) struct DesktopWebhookPort;

impl RoutineWebhookPort for DesktopWebhookPort {
    fn is_configured(&self) -> bool {
        crate::commands_notifications::load_desktop_webhook_config().is_some()
    }

    fn deliver(&self, notification: &shannon_core::notifier::Notification) {
        use shannon_core::notifier::NotificationHandler as _;
        let Some(cfg) = crate::commands_notifications::load_desktop_webhook_config() else {
            return;
        };
        let handler = match shannon_core::notifier::WebhookHandler::new(cfg) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(error = %e, "routine webhook: handler construction failed");
                return;
            }
        };
        // WebhookHandler::send is fire-and-forget by contract (it spawns its
        // own delivery task); errors here are client-construction level only.
        if let Err(e) = handler.send(notification) {
            tracing::warn!(error = %e, "routine webhook: delivery failed");
        }
    }
}

/// The mid-run budget-abort notification seam (W2-3): fire-and-forget
/// delivery of the desktop notification. Object-safe so tests can record
/// dispatches instead of wiring the platform notifier.
pub(crate) trait RunNotifyPort: Send + Sync {
    fn notify(&self, notification: &shannon_core::notifier::Notification);
}

/// Production port: the shared `Notifier` attached to `AppState` at startup
/// — the exact pipeline query notifications use (desktop popup handler +
/// webhook fan-out, master switch / DND prefs applied by the handler).
pub(crate) struct DesktopRunNotifier(pub(crate) std::sync::Arc<shannon_core::notifier::Notifier>);

impl RunNotifyPort for DesktopRunNotifier {
    fn notify(&self, notification: &shannon_core::notifier::Notification) {
        if let Err(e) = self.0.notify(notification) {
            tracing::warn!(error = %e, "routine budget: abort notification dispatch failed");
        }
    }
}

/// Desktop notification for a mid-run budget stop — the user-facing half of
/// the dual channel; the run record's `budget_exceeded` error is the other
/// (History + inbox show it without any extra wiring).
fn notify_budget_abort(port: &dyn RunNotifyPort, task_name: &str, cap: f64, spent: f64) {
    let notification = shannon_core::notifier::Notification {
        title: format!("Shannon — {task_name}: budget reached"),
        body: format!("Monthly budget ${cap:.2} reached — the run was aborted at ${spent:.2}."),
        level: shannon_core::notifier::NotificationLevel::Error,
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        source: Some("routine_budget_abort".to_string()),
        action_id: None,
    };
    port.notify(&notification);
    tracing::info!(
        task_name,
        cap,
        spent,
        "routine budget: abort notification dispatched"
    );
}

/// The state slices `spawn_routine_run` needs, Arc-cloned so the spawned
/// task owns its inputs. Constructed from [`AppState`] (Tauri commands) or
/// from the loopback trigger endpoint's state.
pub(crate) struct RoutineRunDeps {
    pub(crate) inbox: std::sync::Arc<shannon_core::inbox_store::InboxStore>,
    /// Legacy JSONL history mirror (best-effort — see `spawn_routine_run`).
    pub(crate) runs_store: std::sync::Arc<dyn RunMirror>,
    /// Routine-finish webhook routing (B6'). Production default is
    /// [`DesktopWebhookPort`]; tests inject recording mocks.
    pub(crate) webhook: std::sync::Arc<dyn RoutineWebhookPort>,
    /// Budget-abort desktop notification routing (W2-3). Production default
    /// is [`DesktopRunNotifier`]; tests inject recording mocks.
    pub(crate) notify: std::sync::Arc<dyn RunNotifyPort>,
    pub(crate) usage_store: std::sync::Arc<crate::commands_usage::UsageStore>,
    pub(crate) client_config: std::sync::Arc<RwLock<shannon_engine::api::types::LlmClientConfig>>,
    pub(crate) desktop_config: std::sync::Arc<RwLock<DesktopConfig>>,
    pub(crate) tools: std::sync::Arc<shannon_core::tools::ToolRegistry>,
    /// Shared memory store handle (P2-4b) — passed into the spawned runner so
    /// its engine attaches the same store the interactive path uses.
    pub(crate) memory_store: crate::commands_memory::SharedMemoryStore,
    /// Scheduled-task store (P-E1): read for the routine's `working_dir`
    /// sidecar at spawn time.
    pub(crate) scheduled_tasks:
        std::sync::Arc<shannon_core::scheduled_task_store::ScheduledTaskStore>,
    /// Base sessions directory for the run's engine (P-E1). Resolved through
    /// `effective_log_container` so the working-dir stamp lands in exactly
    /// the container the engine's L0 tee will open.
    pub(crate) sessions_dir: std::path::PathBuf,
}

impl RoutineRunDeps {
    pub(crate) fn from_state(state: &AppState) -> Self {
        Self {
            inbox: state.inbox_store(),
            runs_store: state.scheduled_runs_store.clone(),
            webhook: std::sync::Arc::new(DesktopWebhookPort),
            notify: std::sync::Arc::new(DesktopRunNotifier(state.notifier.clone())),
            usage_store: state.usage_store.clone(),
            client_config: state.client_config.clone(),
            desktop_config: state.desktop_config.clone(),
            tools: state.tools.clone(),
            memory_store: state.memory_store.clone(),
            scheduled_tasks: state.scheduled_task_store.clone(),
            sessions_dir: shannon_core::session_log::effective_log_container(
                state.state_manager.sessions_dir(),
            ),
        }
    }
}

/// Everything [`finalize_run`] needs to close out a run.
pub(crate) struct RunFinishContext {
    pub(crate) run_id: String,
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) source: String,
    pub(crate) note: Option<String>,
    pub(crate) started_ms: i64,
    /// The routine's `notify_webhook` flag (B6'). When true, the finish
    /// path routes a status notification through [`RoutineWebhookPort`].
    pub(crate) notify_webhook: bool,
    /// The routine's `policy.notify_on_failure` flag (W3-2). When true a
    /// failed run sends the failure notification (and the auto-pause alert,
    /// dual-channel); when false the failure stays silent — run record +
    /// triage card only. A routine without a stored policy defaults to
    /// true, matching the create-form default.
    pub(crate) notify_on_failure: bool,
}

/// A run's observed spend, as persisted on the run records (SQLite
/// `routine_runs` + the legacy JSONL mirror). `None` = "nothing observed" —
/// the UI hides the cell; estimates are never fabricated (R2-P0-3 ruling).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct RunSpend {
    pub(crate) cost_usd: Option<f64>,
    pub(crate) token_usage: Option<u64>,
}

impl RunSpend {
    /// Last-known-wins merge used at finalize time: a non-`None` value from
    /// the cross-attempt tracker beats the engine outcome's own single-
    /// attempt sum; a `None` on either side keeps the other's value.
    pub(crate) fn merge(self, tracker: RunSpend) -> RunSpend {
        RunSpend {
            cost_usd: tracker.cost_usd.or(self.cost_usd),
            token_usage: tracker.token_usage.or(self.token_usage),
        }
    }
}

/// In-flight spend tracker for one routine run (W2-3). The engine future
/// feeds every Usage event into it; the budget watcher polls the totals and
/// the finalize path records them. Interior-mutable and `Send + Sync`, so
/// the attempt task and the watcher share one `Arc`.
///
/// # 花费口径 (accounting rule)
///
/// The tracker holds the **last-known cumulative totals of the Usage events
/// the engine actually emitted** — the honest known spend at abort time.
/// Known partial spend is recorded verbatim; when no Usage event arrived
/// yet, the totals stay `None` (never an estimate). Known gaps, by design:
///
/// - events are observed per stream chunk, so the recorded abort spend can
///   trail the provider's true spend by one unflushed chunk — the last
///   known value is kept rather than guessing;
/// - each retry attempt mints a fresh engine session; only the live
///   attempt's session id is exposed, so the finalized run record links the
///   last attempt. Earlier attempts' ledger lines keep their own session
///   ids but are not back-linked from the run — the W1 ledger backlog
///   "retried runs under-count in the per-routine month aggregation"; the
///   month spend in the budget guard carries the same under-count.
#[derive(Debug, Default)]
pub(crate) struct RunSpendTracker {
    totals: std::sync::Mutex<RunSpend>,
    session: std::sync::Mutex<Option<uuid::Uuid>>,
}

impl RunSpendTracker {
    /// Fold one Usage event into the cumulative totals.
    pub(crate) fn add_usage(&self, cost_usd: f64, tokens: u64) {
        let mut totals = self.totals.lock().expect("run spend tracker poisoned");
        totals.cost_usd = Some(totals.cost_usd.unwrap_or(0.0) + cost_usd);
        totals.token_usage = Some(totals.token_usage.unwrap_or(0) + tokens);
    }

    /// Pin the session the live attempt opened (spend attribution).
    pub(crate) fn set_session(&self, session_id: uuid::Uuid) {
        *self.session.lock().expect("run spend tracker poisoned") = Some(session_id);
    }

    /// The last-known cumulative totals (`None` before the first event).
    pub(crate) fn snapshot(&self) -> RunSpend {
        *self.totals.lock().expect("run spend tracker poisoned")
    }

    /// The live attempt's session id, for linking the aborted run's spend.
    pub(crate) fn session_id(&self) -> Option<String> {
        self.session
            .lock()
            .expect("run spend tracker poisoned")
            .map(|s| s.to_string())
    }
}

/// What the engine phase of a run produced. `session_id` is `None` when the
/// engine phase never got far enough to open a session (e.g. panic).
///
/// R2-W2-2: `cost_usd`/`token_usage` carry the run's Usage-event totals —
/// `None` when nothing was consumed/observed (never an estimate).
pub(crate) struct RunOutcome {
    pub(crate) failed: bool,
    pub(crate) error: Option<String>,
    pub(crate) output: String,
    pub(crate) session_id: Option<String>,
    pub(crate) cost_usd: Option<f64>,
    pub(crate) token_usage: Option<u64>,
}

impl RunOutcome {
    /// Outcome for a task whose future panicked before returning.
    fn panicked(join_error: String) -> Self {
        Self {
            failed: true,
            error: Some(format!("routine task panicked: {join_error}")),
            output: String::new(),
            session_id: None,
            cost_usd: None,
            token_usage: None,
        }
    }
}

/// Kick off an unattended routine execution and return its run id.
///
/// Mirrors `commands::start_background_task`: fresh engine, configured
/// approval mode (unattended → persisted deny/allow rules honoured, prompts
/// auto-allowed), usage written to the ledger. On completion the run is
/// finished in the SQLite `routine_runs` table (authoritative, decision D6)
/// and mirrored into the legacy JSONL history, and an inbox item is appended.
/// Generic over the Tauri runtime so tests can drive it with `mock_app`.
///
/// ## Failure ordering guarantees
///
/// - The JSONL mirror is **best-effort**: if it cannot be written (start or
///   finish), we log a warning and continue. SQLite is the system of record,
///   so a mirror outage must neither abort the run nor leave the SQLite row
///   stuck in `running`.
/// - The engine phase runs under [`execute_with_policy`] (P1-2): each
///   attempt is guarded against panics and honours the routine's
///   `timeout_secs`; failed attempts retry with `scheduled_retry` backoff
///   while the `max_retries` budget allows. Every terminal path reaches
///   [`finalize_run`], which marks the run succeeded/failed — a panic,
///   timeout, or exhausted retry budget can never leave the SQLite row
///   stuck in `running`.
pub(crate) async fn spawn_routine_run<R: tauri::Runtime>(
    deps: &RoutineRunDeps,
    app: tauri::AppHandle<R>,
    routine: ScheduledRoutine,
    inbox_source: &str,
    note: Option<String>,
) -> Result<String, String> {
    let run_id = deps
        .inbox
        .record_run_start(&routine.id, &routine.name)
        .map_err(|e| e.to_string())?;

    // Best-effort mirror of the run start into the legacy JSONL history
    // (same run id) so the existing History view keeps working until the UI
    // task switches it to the SQLite store. A failure here must NOT abort —
    // otherwise the SQLite row we just created would never get a finish.
    let mut jsonl_run = ScheduledRun::start(&routine.id, &routine.name);
    jsonl_run.run_id = run_id.clone();
    if let Err(e) = deps.runs_store.record_start(&jsonl_run) {
        tracing::warn!(
            run_id = %run_id,
            error = %e,
            "inbox: legacy JSONL run mirror unavailable; continuing with SQLite only"
        );
    }

    let finish_deps = RoutineRunDeps {
        inbox: deps.inbox.clone(),
        runs_store: deps.runs_store.clone(),
        webhook: deps.webhook.clone(),
        notify: deps.notify.clone(),
        usage_store: deps.usage_store.clone(),
        client_config: deps.client_config.clone(),
        desktop_config: deps.desktop_config.clone(),
        tools: deps.tools.clone(),
        memory_store: deps.memory_store.clone(),
        scheduled_tasks: deps.scheduled_tasks.clone(),
        sessions_dir: deps.sessions_dir.clone(),
    };
    let ctx = RunFinishContext {
        run_id: run_id.clone(),
        task_id: routine.id.clone(),
        task_name: routine.name.clone(),
        source: inbox_source.to_string(),
        note,
        started_ms: chrono::Utc::now().timestamp_millis(),
        notify_webhook: routine.notify_webhook,
        // W3-2: thread the policy flag so finalize can honour it. No stored
        // policy = the product default (true), same as the create form.
        notify_on_failure: routine
            .policy
            .as_ref()
            .map(|p| p.notify_on_failure)
            .unwrap_or(true),
    };

    // P1-2 budget gate: a routine past its configured monthly budget is
    // skipped entirely — the run finalizes as failed carrying the reason, so
    // History and the inbox show why nothing executed (instead of silently
    // not firing or burning more money).
    if let Some(reason) = budget_skip_reason(deps, &routine) {
        tracing::info!(
            run_id = %run_id,
            task_id = %routine.id,
            reason = %reason,
            "routine run skipped: monthly budget exhausted"
        );
        finalize_run(
            &finish_deps,
            &app,
            ctx,
            RunOutcome {
                failed: true,
                error: Some(reason),
                output: String::new(),
                session_id: None,
                cost_usd: None,
                token_usage: None,
            },
            RunSpend::default(),
        );
        return Ok(run_id);
    }

    let client_config = deps.client_config.read().await.clone();
    let approval_mode_str = deps.desktop_config.read().await.approval_mode.clone();
    // P2-5: in-window routine executions downgrade to the configured
    // `offpeak.model_override` model (empty config = disabled). Everything
    // else about the run — approval mode, tools, usage ledger — is unchanged;
    // only QueryMetadata.model and the usage attribution swap.
    let offpeak_override = {
        let desktop_cfg = deps.desktop_config.read().await;
        resolve_offpeak_model(
            &routine,
            desktop_cfg.offpeak.effective_model_override(),
            chrono::Utc::now(),
        )
    };
    if let Some(ref m) = offpeak_override {
        tracing::info!(
            run_id = %run_id,
            model = %m,
            "off-peak window active: executing routine with model override"
        );
    }
    let model = offpeak_override.unwrap_or_else(|| client_config.model.clone());
    let model_for_usage = model.clone();
    let provider = client_config.provider.to_string();
    let prompt = routine.prompt.clone();
    let usage_store = deps.usage_store.clone();
    let tools = deps.tools.clone();
    let memory_store = deps.memory_store.clone();

    // W2-3 mid-run budget guard. `policy_budget` is `None` when no budget is
    // configured — the whole guard (tracker polling, abort signal, abort
    // rewrite) compiles down to the legacy behavior. The tracker accumulates
    // across retry attempts so the run record's spend covers every attempt;
    // the `watch` channel is the same abort channel the attempt timeout uses.
    let policy_budget = routine
        .policy
        .as_ref()
        .and_then(|p| p.budget_usd)
        .filter(|cap| cap.is_finite() && *cap > 0.0);
    let spend_tracker = std::sync::Arc::new(RunSpendTracker::default());
    let (budget_tx, budget_rx) = tokio::sync::watch::channel(false);

    // P-E1: the routine's project directory, when it has one. Best-effort —
    // a vanished task dir or unreadable sidecar degrades to "no project".
    let sidecar_working_dir = deps
        .scheduled_tasks
        .working_dir_of(&routine.id)
        .unwrap_or_else(|e| {
            tracing::debug!(
                task_id = %routine.id,
                error = %e,
                "routine run: working_dir sidecar unreadable"
            );
            None
        })
        .filter(|d| !d.trim().is_empty());
    // P1-2 `worktree` wiring: when the policy flag is set the run is pinned
    // to the task's scheduled-worktree directory (created on demand from the
    // routine's project), falling back to the plain sidecar dir with a
    // warning when no worktree can be provided.
    let worktree_base = shannon_core::scheduled_worktree::default_base_dir();
    let routine_working_dir =
        resolve_run_working_dir(&routine, sidecar_working_dir.as_deref(), &worktree_base);

    // Owned copy for the engine future (the closure must be 'static; `deps`
    // is only borrowed here).
    let run_sessions_dir = deps.sessions_dir.clone();
    // W2-3: the engine-future factory gets its own handle on the shared
    // tracker; the run task keeps the original for the watcher and finalize.
    let engine_spend_tracker = spend_tracker.clone();

    // P1-2: the engine phase is a *factory* — query streams are single-shot,
    // so the retry policy needs a fresh engine per attempt. The closure
    // clones its (cheap Arc/String) inputs per call and stays `FnMut`.
    let make_engine_future = move || {
        let client_config = client_config.clone();
        let approval_mode_str = approval_mode_str.clone();
        let run_sessions_dir = run_sessions_dir.clone();
        let memory_store = memory_store.clone();
        let routine_working_dir = routine_working_dir.clone();
        let model = model.clone();
        let provider = provider.clone();
        let prompt = prompt.clone();
        let usage_store = usage_store.clone();
        let tools = tools.clone();
        let model_for_usage = model_for_usage.clone();
        let spend_tracker = engine_spend_tracker.clone();
        async move {
            let client = LlmClient::new(client_config);

            // Same policy as background tasks: run unattended under the
            // configured approval mode plus persisted rules. review §P1-2:
            // the previous default of FullAuto silently bypassed the user's
            // global approval mode for every unattended path. SECURITY.md
            // promises that unattended paths honour the user's mode; FullAuto
            // must require an explicit opt-in via the routine's approval_mode
            // field, and we default to Suggest otherwise.
            let mut permissions = PermissionManager::new();
            let mode = crate::commands::unattended_approval_mode(approval_mode_str.as_deref());
            permissions.set_approval_mode(mode);
            let mut settings = shannon_core::settings::SettingsManager::new();
            if settings.load_from_files().is_ok() {
                let rules = &settings.settings_mut().permissions;
                permissions.set_rule_checker(PermissionRuleChecker::from_rule_strings(
                    &rules.deny,
                    &rules.ask,
                    &rules.allow,
                ));
            }

            // P-E1: pin the run's engine to the SAME sessions container the L0
            // tee will resolve (`effective_log_container`), so the working-dir
            // stamp below lands in the log the engine actually opens. Falls back
            // to the default manager if the custom dir cannot be created.
            let state_manager =
                shannon_engine::state::StateManager::with_sessions_dir(run_sessions_dir.clone())
                    .unwrap_or_else(|e| {
                        tracing::warn!(
                            error = %e,
                            "routine run: custom sessions dir unusable, using default"
                        );
                        StateManager::new()
                    });
            let engine = crate::commands_memory::attach_shared_memory(
                QueryEngine::with_defaults_arc(client, tools, permissions, state_manager),
                &memory_store,
            );
            // P-E1: the routine's project drives the engine's host-dependent
            // reads (memory injection/extraction project key) — an existing
            // per-run working-directory parameter, so it is threaded here
            // instead of ever touching the process cwd (global state).
            let engine = match &routine_working_dir {
                Some(dir) => {
                    engine.with_working_directory(crate::commands_projects::normalize_path(dir))
                }
                None => engine,
            };

            let session_id = uuid::Uuid::new_v4();
            // W2-3: pin the live attempt's session on the shared tracker so a
            // budget abort can link the in-flight spend back to this run.
            spend_tracker.set_session(session_id);
            // P-E1: stamp the session's durable metadata before the engine's
            // tee opens the (fresh) log — session/start with the routine's
            // working dir as `cwd`, the field the session store projects to
            // `project_path` / `SessionMeta.working_dir`. `None` keeps today's
            // behavior byte-for-byte (the tee writes its own row).
            if let Some(dir) = &routine_working_dir {
                stamp_session_working_dir(
                    &shannon_core::session_log::effective_log_container(&run_sessions_dir),
                    session_id,
                    &model,
                    &provider,
                    dir,
                );
            }
            let context = QueryContext {
                query_id: uuid::Uuid::new_v4(),
                session_id,
                user_message: prompt.clone(),
                metadata: QueryMetadata {
                    timestamp: chrono::Utc::now(),
                    tools_allowed: true,
                    max_tokens: None,
                    model,
                    temperature: None,
                    top_p: None,
                },
                attachments: Vec::new(),
            };

            let mut final_output = String::new();
            let mut failure: Option<String> = None;
            // R2-W2-2: this attempt's Usage-event totals — persisted on the
            // run record so the OPC/History cost columns carry real data.
            // (What is NOT covered: spend from earlier, retried attempts —
            // each attempt sums only itself; see the W2-3 budget-abort
            // accounting note for the known undercount.)
            let mut attempt_cost_usd = 0.0f64;
            let mut attempt_tokens = 0u64;

            let stream = engine.process_query(context, None).await;
            use futures::StreamExt;
            let mut pin_stream = std::pin::pin!(stream);
            while let Some(event_result) = pin_stream.next().await {
                match event_result {
                    Ok(event) => match event {
                        QueryEvent::Text { content, .. } => final_output.push_str(&content),
                        QueryEvent::Usage {
                            input_tokens,
                            output_tokens,
                            cost_usd,
                            cache_creation_tokens,
                            cache_read_tokens,
                            ..
                        } => {
                            attempt_cost_usd += cost_usd;
                            attempt_tokens += input_tokens
                                + output_tokens
                                + cache_creation_tokens
                                + cache_read_tokens;
                            // W2-3: feed the cross-attempt tracker (budget
                            // guard + finalize spend source of truth).
                            spend_tracker.add_usage(
                                cost_usd,
                                input_tokens
                                    + output_tokens
                                    + cache_creation_tokens
                                    + cache_read_tokens,
                            );
                            // Best-effort ledger write, mirroring background tasks.
                            // P1-2: the write is attributed to this run's session so
                            // the routine budget gate can aggregate per-routine spend
                            // (pre-attribution runs wrote `None` and were invisible
                            // to the aggregation).
                            let _ = usage_store.append(&crate::commands_usage::record_event(
                                &model_for_usage,
                                &provider,
                                crate::commands_usage::UsageTotals {
                                    input_tokens,
                                    output_tokens,
                                    cache_creation_tokens,
                                    cache_read_tokens,
                                    cost_usd,
                                },
                                Some(session_id.to_string()).as_deref(),
                            ));
                        }
                        QueryEvent::Completed { .. } => break,
                        QueryEvent::Failed { error, .. } => {
                            failure = Some(error);
                            break;
                        }
                        _ => {}
                    },
                    Err(e) => {
                        failure = Some(e.to_string());
                        break;
                    }
                }
            }

            RunOutcome {
                failed: failure.is_some(),
                error: failure,
                output: final_output,
                session_id: Some(session_id.to_string()),
                cost_usd: Some(attempt_cost_usd),
                token_usage: Some(attempt_tokens),
            }
        }
    };

    // P1-2: the whole engine phase (panic guard included) now runs under the
    // routine's timeout/retry policy. A retried run keeps its single SQLite
    // `routine_runs` row — the final state plus the give-up annotation (how
    // many attempts, why they stopped) is what History shows.
    let run_policy = RunExecutionPolicy::of(&routine);
    // Copied out of `ctx` for the watcher/abort rewrite — `ctx` itself moves
    // into `finalize_run` at the end of the task.
    let ctx_task_id = ctx.task_id.clone();
    let ctx_task_name = ctx.task_name.clone();
    tokio::spawn(async move {
        // W2-3: poll month aggregate + this run's last-known in-flight spend
        // against the cap while the engine runs; a trip sends the abort
        // signal the attempt race is parked on.
        let budget_watcher = policy_budget.map(|cap| {
            let usage = finish_deps.usage_store.clone();
            let inbox = finish_deps.inbox.clone();
            let task_id = ctx_task_id.clone();
            let tracker = spend_tracker.clone();
            let tx = budget_tx.clone();
            tokio::spawn(async move {
                loop {
                    let in_flight = tracker.snapshot().cost_usd;
                    if budget_live_exceeded(
                        &usage,
                        &inbox,
                        &task_id,
                        cap,
                        in_flight,
                        chrono::Utc::now(),
                    ) {
                        tracing::warn!(
                            task_id = %task_id,
                            cap_usd = cap,
                            in_flight_usd = ?in_flight,
                            "routine budget: cap reached mid-run — tripping abort"
                        );
                        let _ = tx.send(true);
                        return;
                    }
                    tokio::time::sleep(BUDGET_ABORT_POLL).await;
                }
            })
        });

        let mut outcome =
            execute_with_policy(make_engine_future, &run_policy, Some(budget_rx.clone())).await;
        if let Some(watcher) = budget_watcher {
            watcher.abort();
        }

        // R2-W2-2/W2-3 花费口径: prefer the cross-attempt tracker (covers
        // retried attempts the engine outcome cannot see); fall back to the
        // outcome's own single-attempt sum. `None` stays `None` — no
        // estimates on the run record.
        let spend = RunSpend {
            cost_usd: outcome.cost_usd,
            token_usage: outcome.token_usage,
        }
        .merge(spend_tracker.snapshot());

        // Budget stop: terminal failure with the machine-readable reason, the
        // in-flight session linked (so the aborted spend stays attributable to
        // this routine in later month aggregations), and the desktop
        // notification — the run-record error is the second channel.
        let tripped = *budget_rx.borrow();
        if tripped {
            if let Some(cap) = policy_budget {
                let spent = spend.cost_usd.unwrap_or(0.0);
                outcome.failed = true;
                outcome.error = Some(format!(
                    "{BUDGET_ABORT_MARKER}: monthly budget ${cap:.2} reached — run aborted at ${spent:.2}"
                ));
                outcome.session_id = spend_tracker.session_id();
                notify_budget_abort(finish_deps.notify.as_ref(), &ctx_task_name, cap, spent);
            }
        }

        finalize_run(&finish_deps, &app, ctx, outcome, spend);
    });

    Ok(run_id)
}

/// Record the run session's `session/start` with the routine's working
/// directory (P-E1) — the durable session metadata of an unattended run.
///
/// The engine's L0 tee writes `session/start` itself only on a fresh log,
/// and it always records the *process* cwd; pre-recording the row here pins
/// the session's project path to the routine's directory instead. That is
/// the field the session store projects to `project_path` /
/// `SessionMeta.working_dir` (project adoption, triage join, restart
/// hydration) — the same pre-creation pattern as the goal runner's
/// `create_goal_session`. The process cwd is never touched here: background
/// threads must not move process-global state; host-dependent engine reads
/// are pinned separately via `QueryEngine::with_working_directory`.
///
/// Best-effort: a failed or skipped stamp is logged and the run proceeds —
/// the tee then writes its own `session/start` exactly as before. A log
/// that already has events is never touched (no double `session/start`).
fn stamp_session_working_dir(
    container: &std::path::Path,
    session_id: uuid::Uuid,
    model: &str,
    provider: &str,
    working_dir: &str,
) {
    let mut writer = match shannon_core::session_log::SessionLogWriter::open_layout(
        container,
        &session_id.to_string(),
    ) {
        Ok(writer) => writer,
        Err(e) => {
            tracing::warn!(
                session = %session_id,
                error = %e,
                "routine run: session log unreachable, working_dir not stamped"
            );
            return;
        }
    };
    if writer.next_seq() > 0 {
        tracing::debug!(
            session = %session_id,
            "routine run: session log already started, working_dir stamp skipped"
        );
        return;
    }
    writer.record(
        shannon_types::session_event::SessionEventBody::SessionStart(
            shannon_types::session_event::SessionStartPayload {
                model: model.to_string(),
                provider: Some(provider.to_string()),
                cwd: Some(crate::commands_projects::normalize_path(working_dir).to_string()),
                app_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                os: Some(std::env::consts::OS.to_string()),
                arch: Some(std::env::consts::ARCH.to_string()),
                browser_cdp: Some(
                    std::env::var("SHANNON_BROWSER_CDP")
                        .map(|v| !v.trim().is_empty())
                        .unwrap_or(false),
                ),
            },
        ),
    );
    if let Err(e) = writer.close() {
        tracing::warn!(
            session = %session_id,
            error = %e,
            "routine run: working_dir stamp write failed"
        );
    }
}

/// Close out a run: legacy JSONL finish (best-effort), inbox item, SQLite
/// `routine_runs` finish (with `inbox_item_id` back-link and the run's
/// cost/token totals, R2-W2-2), the W3-2 failure aftermath (failure
/// notification, consecutive-failure auto pause), and the refresh event.
/// Every path through here terminates the SQLite run — this is the single
/// choke point that prevents `running` rows from wedging.
fn finalize_run<R: tauri::Runtime>(
    deps: &RoutineRunDeps,
    app: &tauri::AppHandle<R>,
    ctx: RunFinishContext,
    outcome: RunOutcome,
    spend: RunSpend,
) {
    let finished_ms = chrono::Utc::now().timestamp_millis();
    let duration_secs = (finished_ms - ctx.started_ms).max(0) / 1000;
    let status = if outcome.failed {
        "failed"
    } else {
        "succeeded"
    };

    // W3-2: decide the auto pause BEFORE any record is written, so the
    // machine-readable pause reason can be folded into the persisted error.
    // Only scheduler fires can complete the streak (manual triggers never
    // pause a routine and never count toward it); the streak itself is
    // re-derived from SQLite per finalize — stateless, so concurrent
    // finalizes cannot double-count.
    let auto_pause = outcome.failed
        && ctx.source == shannon_core::inbox_store::SOURCE_ROUTINE
        && scheduled_failure_streak(deps, &ctx.task_id, AUTO_PAUSE_FAILURE_THRESHOLD - 1)
            == AUTO_PAUSE_FAILURE_THRESHOLD - 1;

    let run_error = outcome
        .error
        .as_deref()
        .map(|e| truncate_chars(e, SUMMARY_MAX_CHARS));
    // W3-2: the pausing failure carries the reason prefix (budget-abort
    // style) plus the self-heal path, in the run record AND the inbox card.
    let run_error = if auto_pause {
        let base = run_error.unwrap_or_default();
        Some(truncate_chars(
            &format!(
                "{AUTO_PAUSE_MARKER}: paused automatically after {AUTO_PAUSE_FAILURE_THRESHOLD} \
                 consecutive scheduled failures — re-enable it from the task card · {base}"
            ),
            SUMMARY_MAX_CHARS,
        ))
    } else {
        run_error
    };

    // 1. Legacy JSONL history (best-effort — never blocks the inbox).
    let jsonl_status = if outcome.failed {
        RunStatus::Failed
    } else {
        RunStatus::Succeeded
    };
    if let Err(e) =
        deps.runs_store
            .record_finish(&ctx.run_id, jsonl_status, run_error.clone(), spend)
    {
        tracing::warn!(
            run_id = %ctx.run_id,
            error = %e,
            "inbox: legacy JSONL run mirror finish failed; SQLite run record is authoritative"
        );
    }

    // 2. Completion webhook routing (B6'). Fired before the inbox item so a
    // "sink not configured" skip can be annotated in the run record itself.
    // The delivery is fire-and-forget; only a *skipped* delivery (flag on,
    // no sink configured) comes back as a note — delivery failures stay in
    // the logs, mirroring WebhookHandler's own contract.
    let webhook_note = deliver_routine_webhook(deps, &ctx, &outcome);

    // 3. Inbox item + 4. SQLite run back-link. The summary doubles as the
    // durable run record, so the skipped-webhook note lands in it.
    // W3-2: capture what the aftermath needs before the item consumes the
    // context's owned fields.
    let task_name = ctx.task_name.clone();
    let task_id = ctx.task_id.clone();
    let notify_on_failure = ctx.notify_on_failure;
    let mut summary = build_summary(
        ctx.note.as_deref(),
        duration_secs,
        &outcome.output,
        run_error.is_some(),
    );
    if let Some(note) = &webhook_note {
        summary = truncate_chars(&format!("{summary} · {note}"), SUMMARY_MAX_CHARS);
    }
    // W3-2: the triage card doubles as the recovery surface — say why the
    // routine stopped and how to bring it back.
    if auto_pause {
        summary = truncate_chars(
            &format!(
                "{summary} · auto-paused after {AUTO_PAUSE_FAILURE_THRESHOLD} consecutive \
                 failures — re-enable it from the task card"
            ),
            SUMMARY_MAX_CHARS,
        );
    }
    let item = deps
        .inbox
        .append_item(InboxItemNew {
            source: ctx.source,
            source_id: Some(ctx.task_id),
            session_id: outcome.session_id,
            title: ctx.task_name,
            summary,
            error: run_error.clone(),
        })
        .map_err(|e| e.to_string());
    let item_id = match item {
        Ok(saved) => Some(saved.id),
        Err(e) => {
            tracing::warn!(run_id = %ctx.run_id, error = %e, "inbox: failed to append run item");
            None
        }
    };
    if let Err(e) = deps.inbox.record_run_finish(
        &ctx.run_id,
        status,
        run_error.as_deref(),
        item_id,
        spend.cost_usd,
        spend.token_usage,
    ) {
        tracing::warn!(run_id = %ctx.run_id, error = %e, "inbox: failed to finish run record");
    }

    // 5. W3-2 failure aftermath: flip enabled at the threshold (the bool
    // reports the actual transition — notifications fire once), then the
    // failure notification, then the dual-channel auto-pause alert. All of
    // it is gated by the routine's `policy.notify_on_failure`; false keeps
    // the failure silent (run record + triage card only).
    let paused_now = if auto_pause {
        pause_routine_after_consecutive_failures(deps, &task_id)
    } else {
        false
    };
    if notify_on_failure && outcome.failed {
        notify_run_failed(deps.notify.as_ref(), &task_name, run_error.as_deref());
        if paused_now {
            notify_auto_paused(deps.notify.as_ref(), deps.webhook.as_ref(), &task_name);
        }
    }

    // 6. Refresh signal.
    let _ = app.emit(event_names::INBOX_UPDATED, ctx.run_id);
}

/// Length of the scheduled-fire failure streak among the terminal runs
/// already recorded for `task_id` (newest first), stopping at the first
/// success (any success clears the streak). Only scheduler-originated runs
/// count — a run's linked inbox item decides (`source == routine`), so
/// manual triggers and trigger-item reruns never advance the streak.
/// Non-terminal rows (`running`, `queued`/`cancelled` tombstones) are
/// invisible to it.
///
/// Stateless on purpose: the streak is re-derived from the authoritative
/// SQLite `routine_runs` on every finalize instead of being accumulated in
/// memory, so there is no counter that concurrent finalizes could
/// double-increment — and no counter to lose across restarts.
fn scheduled_failure_streak(deps: &RoutineRunDeps, task_id: &str, threshold: u32) -> u32 {
    if threshold == 0 {
        return 0;
    }
    let Ok(runs) = deps.inbox.list_runs_by_task(task_id, AUTO_PAUSE_RUN_SCAN) else {
        return 0;
    };
    let mut streak = 0u32;
    for run in runs {
        match run.status.as_str() {
            "succeeded" => return 0,
            "failed" => {}
            _ => continue,
        }
        let scheduled = run
            .inbox_item_id
            .and_then(|id| deps.inbox.get_item(id).ok().flatten())
            .is_some_and(|item| item.source == shannon_core::inbox_store::SOURCE_ROUTINE);
        if !scheduled {
            continue;
        }
        streak += 1;
        if streak >= threshold {
            break;
        }
    }
    streak
}

/// Disable a routine whose scheduled failures reached the threshold (W3-2).
/// Fresh load + `enabled` check keeps the write idempotent: only the call
/// that actually flips the flag returns true, so re-entrant or racing
/// finalizes cannot duplicate the pause or its notification. The scheduler
/// tick naturally respects `enabled == false` (`should_fire_at`).
fn pause_routine_after_consecutive_failures(deps: &RoutineRunDeps, task_id: &str) -> bool {
    let routine = match deps.scheduled_tasks.load(task_id) {
        Ok(Some(r)) => r,
        Ok(None) => {
            tracing::warn!(
                task_id = %task_id,
                "auto pause: routine not found in the task store"
            );
            return false;
        }
        Err(e) => {
            tracing::warn!(
                task_id = %task_id,
                error = %e,
                "auto pause: routine unreadable — staying enabled this round"
            );
            return false;
        }
    };
    if !routine.enabled {
        // Already paused (manual toggle, budget stop, or a racing finalize).
        return false;
    }
    let mut paused = routine;
    paused.enabled = false;
    if let Err(e) = deps.scheduled_tasks.save(&paused) {
        tracing::warn!(
            task_id = %task_id,
            error = %e,
            "auto pause: persisting enabled=false failed — routine stays enabled"
        );
        return false;
    }
    tracing::info!(
        task_id = %task_id,
        threshold = AUTO_PAUSE_FAILURE_THRESHOLD,
        marker = AUTO_PAUSE_MARKER,
        "routine auto-paused after consecutive scheduled failures"
    );
    true
}

/// Desktop notification for one failed run (W3-2 — this is the long-promised
/// consumption of `policy.notify_on_failure`, previously UI-only).
fn notify_run_failed(port: &dyn RunNotifyPort, task_name: &str, error: Option<&str>) {
    let body = error
        .map(|e| truncate_chars(e, 200))
        .unwrap_or_else(|| "The run failed — details are in Triage.".to_string());
    let notification = shannon_core::notifier::Notification {
        title: format!("Shannon — {task_name}: run failed"),
        body,
        level: shannon_core::notifier::NotificationLevel::Error,
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        source: Some("routine_run_failed".to_string()),
        action_id: None,
    };
    port.notify(&notification);
    tracing::debug!(task_name, "routine failure notification dispatched");
}

/// Dual-channel auto-pause alert (W3-2): the desktop notification plus the
/// configured webhook sink. The copy always names the self-heal path —
/// re-enable the routine from its task card.
fn notify_auto_paused(
    notify: &dyn RunNotifyPort,
    webhook: &dyn RoutineWebhookPort,
    task_name: &str,
) {
    let title = format!("Shannon — {task_name}: auto-paused");
    let body = format!(
        "Paused automatically after {AUTO_PAUSE_FAILURE_THRESHOLD} consecutive scheduled \
         failures. To resume, re-enable it from its task card."
    );
    let build = |source: &str| shannon_core::notifier::Notification {
        title: title.clone(),
        body: body.clone(),
        level: shannon_core::notifier::NotificationLevel::Error,
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        source: Some(source.to_string()),
        action_id: None,
    };
    notify.notify(&build("routine_auto_pause"));
    webhook.deliver(&build("routine_auto_pause_webhook"));
    tracing::info!(
        task_name,
        "routine auto-pause notification dispatched (desktop + webhook)"
    );
}

/// Cap on the run-output summary forwarded in the webhook body. The handler
/// sanitizes and re-truncates per template anyway; this keeps the pre-sanitize
/// string bounded.
const WEBHOOK_BODY_MAX_CHARS: usize = 500;

/// Route a finished run through the configured webhook sink (B6').
///
/// Semantics per the flag + config matrix:
///
/// - flag off → no-op (returns `None`);
/// - flag on, sink configured → deliver `{task name} — {status}` with the
///   output summary (or the error, for failed runs) as the body, fire-and-
///   forget, and return `None`;
/// - flag on, no sink → **silently skip**: no error, no triage item — the
///   caller annotates the run record with the returned note so the History
///   view shows why nothing was delivered.
///
/// Returns the run-record annotation for a skipped delivery, if any.
fn deliver_routine_webhook(
    deps: &RoutineRunDeps,
    ctx: &RunFinishContext,
    outcome: &RunOutcome,
) -> Option<String> {
    if !ctx.notify_webhook {
        return None;
    }
    if !deps.webhook.is_configured() {
        tracing::info!(
            run_id = %ctx.run_id,
            task_id = %ctx.task_id,
            "routine finish: notify_webhook enabled but no webhook configured — skipped"
        );
        return Some("webhook notification skipped (not configured)".to_string());
    }
    let status_word = if outcome.failed {
        "failed"
    } else {
        "succeeded"
    };
    let body = match &outcome.error {
        Some(err) => truncate_chars(err, WEBHOOK_BODY_MAX_CHARS),
        None => truncate_chars(
            first_line_snapshot(&outcome.output, outcome.failed).trim(),
            WEBHOOK_BODY_MAX_CHARS,
        ),
    };
    let notification = shannon_core::notifier::Notification {
        title: format!("{} — {}", ctx.task_name, status_word),
        body,
        level: if outcome.failed {
            shannon_core::notifier::NotificationLevel::Error
        } else {
            shannon_core::notifier::NotificationLevel::Info
        },
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        source: Some("routine_finish".to_string()),
        action_id: None,
    };
    deps.webhook.deliver(&notification);
    tracing::debug!(run_id = %ctx.run_id, "routine finish: webhook notification dispatched");
    None
}

/// Truncate to at most `max` chars without splitting a UTF-8 codepoint, and
/// mark the cut with an ellipsis.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

/// Build the inbox summary: optional trigger note, duration, output snippet —
/// capped at [`SUMMARY_MAX_CHARS`] characters.
fn build_summary(note: Option<&str>, duration_secs: i64, output: &str, failed: bool) -> String {
    let snippet = first_line_snapshot(output, failed);
    let mut parts: Vec<String> = Vec::new();
    if let Some(n) = note.map(str::trim).filter(|n| !n.is_empty()) {
        parts.push(truncate_chars(n, 200).replace('\n', " "));
    }
    parts.push(format!("took {duration_secs}s"));
    if !snippet.is_empty() {
        parts.push(snippet);
    }
    truncate_chars(&parts.join(" · "), SUMMARY_MAX_CHARS)
}

/// Condense run output to a short snippet: the first non-empty line (failed
/// runs show the tail, where error summaries usually live).
fn first_line_snapshot(output: &str, failed: bool) -> String {
    let snippet = if failed {
        output
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
    } else {
        output
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
    };
    truncate_chars(snippet, 200).replace('\n', " ")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // (`chrono::TimeZone`/`Datelike` trait scopes flow in through the
    // parent's anonymous imports via `use super::*`.)

    // ── pure helpers ────────────────────────────────────────────────────

    #[test]
    fn truncate_chars_respects_limit_and_utf8() {
        assert_eq!(truncate_chars("hello", 10), "hello");
        assert_eq!(truncate_chars("hello", 5), "hello");
        let cut = truncate_chars("hello world", 5);
        assert!(cut.starts_with("hello"));
        assert!(cut.ends_with('…'));
        // Multi-byte safety.
        let emoji = "🦀".repeat(600);
        let cut = truncate_chars(&emoji, SUMMARY_MAX_CHARS);
        assert_eq!(cut.chars().count(), SUMMARY_MAX_CHARS + 1);
    }

    #[test]
    fn build_summary_includes_note_duration_output() {
        let s = build_summary(Some("from CI"), 12, "First line\nsecond", false);
        assert!(s.contains("from CI"), "{s}");
        assert!(s.contains("took 12s"), "{s}");
        assert!(s.contains("First line"), "{s}");
        assert!(!s.contains("second"), "snippet is one line: {s}");
    }

    #[test]
    fn build_summary_skips_empty_note_and_blank_output() {
        let s = build_summary(Some("   "), 0, "", false);
        assert_eq!(s, "took 0s");
        let s = build_summary(None, 3, "\n \n", false);
        assert_eq!(s, "took 3s");
    }

    #[test]
    fn build_summary_failure_uses_last_line() {
        let s = build_summary(None, 4, "started ok\nfinal error: boom", true);
        assert!(s.contains("boom"), "{s}");
        assert!(!s.contains("started ok"), "{s}");
    }

    #[test]
    fn build_summary_is_capped_at_500_chars() {
        let long = "x".repeat(2_000);
        let s = build_summary(Some(&long), 1, &long, false);
        assert!(s.chars().count() <= SUMMARY_MAX_CHARS + 1);
    }

    #[test]
    fn rerunnable_sources_cover_task_and_trigger() {
        assert!(RERUNNABLE_SOURCES.contains(&shannon_core::inbox_store::SOURCE_ROUTINE));
        assert!(RERUNNABLE_SOURCES.contains(&shannon_core::inbox_store::SOURCE_SCHEDULED_TASK));
        assert!(RERUNNABLE_SOURCES.contains(&shannon_core::inbox_store::SOURCE_TRIGGER));
        assert!(!RERUNNABLE_SOURCES.contains(&shannon_core::inbox_store::SOURCE_GOAL));
    }

    // ── P2-5: off-peak model override resolution ─────────────────────────

    fn windowed_routine(start: u8, end: u8, tz: Option<&str>) -> ScheduledRoutine {
        let mut r = ScheduledRoutine::new("nightly".into(), "p".into(), 3600);
        r.policy = Some(shannon_core::scheduled_routines::ExecutionPolicy {
            execution_window: Some(
                shannon_core::scheduled_routines::ExecutionWindow::new(
                    start,
                    end,
                    tz.map(str::to_string),
                )
                .unwrap(),
            ),
            ..Default::default()
        });
        r
    }

    fn utc_at(h: u32, m: u32) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 1, 15, h, m, 0).unwrap()
    }

    #[test]
    fn offpeak_override_applies_only_inside_window() {
        let routine = windowed_routine(22, 6, Some("UTC"));
        // In window (23:00 UTC) + configured → override.
        assert_eq!(
            resolve_offpeak_model(&routine, Some("glm-4-flash"), utc_at(23, 0)),
            Some("glm-4-flash".into())
        );
        // Boundary: window start hour is inclusive.
        assert_eq!(
            resolve_offpeak_model(&routine, Some("glm-4-flash"), utc_at(22, 0)),
            Some("glm-4-flash".into())
        );
        // End hour inclusive, closes at 07:00.
        assert_eq!(
            resolve_offpeak_model(&routine, Some("glm-4-flash"), utc_at(6, 59)),
            Some("glm-4-flash".into())
        );
        // Outside the window → active model, no override.
        assert_eq!(
            resolve_offpeak_model(&routine, Some("glm-4-flash"), utc_at(12, 0)),
            None
        );
        assert_eq!(
            resolve_offpeak_model(&routine, Some("glm-4-flash"), utc_at(7, 0)),
            None
        );
    }

    #[test]
    fn offpeak_override_requires_configured_non_empty_value() {
        let routine = windowed_routine(22, 6, Some("UTC"));
        assert_eq!(resolve_offpeak_model(&routine, None, utc_at(23, 0)), None);
        assert_eq!(
            resolve_offpeak_model(&routine, Some(""), utc_at(23, 0)),
            None
        );
        assert_eq!(
            resolve_offpeak_model(&routine, Some("   "), utc_at(23, 0)),
            None
        );
        // Whitespace around a value is trimmed, not rejected.
        assert_eq!(
            resolve_offpeak_model(&routine, Some(" glm-4-flash "), utc_at(23, 0)),
            Some("glm-4-flash".into())
        );
    }

    #[test]
    fn offpeak_override_ignored_without_window_and_respects_timezone() {
        // No window → override never applies (immediate execution semantics).
        let plain = ScheduledRoutine::new("plain".into(), "p".into(), 3600);
        assert_eq!(
            resolve_offpeak_model(&plain, Some("glm-4-flash"), utc_at(23, 0)),
            None
        );
        // Window in +08:00: UTC 15:00 is 23:00 wall — inside; UTC 13:00 is
        // 21:00 wall — one hour before the window opens.
        let tz = windowed_routine(22, 6, Some("+08:00"));
        assert_eq!(
            resolve_offpeak_model(&tz, Some("glm-4-flash"), utc_at(15, 0)),
            Some("glm-4-flash".into())
        );
        assert_eq!(
            resolve_offpeak_model(&tz, Some("glm-4-flash"), utc_at(14, 0)),
            Some("glm-4-flash".into()),
            "UTC 14:00 = 22:00+08:00 wall — inclusive window start"
        );
        assert_eq!(
            resolve_offpeak_model(&tz, Some("glm-4-flash"), utc_at(13, 0)),
            None
        );
    }

    // ── store-backed pieces (no Tauri runtime needed) ───────────────────

    fn item_new_min(title: &str) -> InboxItemNew {
        InboxItemNew {
            source: shannon_core::inbox_store::SOURCE_ROUTINE.into(),
            source_id: Some("t".into()),
            session_id: None,
            title: title.into(),
            summary: String::new(),
            error: None,
        }
    }

    #[test]
    fn session_id_contract_is_preserved_through_the_store() {
        // The `continue_inbox_item_session` command filters/preserves
        // session ids exactly like this — the UI relies on receiving the id
        // string, or an Err when there is none.
        let store = shannon_core::inbox_store::InboxStore::open_in_memory().unwrap();
        let with_session = store
            .append_item(InboxItemNew {
                session_id: Some("0195abcd-0000-7000-8000-000000000000".into()),
                ..item_new_min("with session")
            })
            .unwrap();
        let without = store.append_item(item_new_min("no session")).unwrap();

        let got = store.get_item(with_session.id).unwrap().unwrap();
        assert_eq!(
            got.session_id.as_deref(),
            Some("0195abcd-0000-7000-8000-000000000000")
        );
        let missing = store.get_item(without.id).unwrap().unwrap();
        assert!(missing.session_id.is_none());
    }

    // ── review fix round 1 (Important 1): failure-path guarantees ───────

    /// JSONL mirror that always fails — stands in for an unwritable
    /// `~/.shannon/scheduled-runs/` (disk full, permissions, ...).
    struct FailingRunMirror {
        record_start_calls: std::sync::atomic::AtomicUsize,
        record_finish_calls: std::sync::atomic::AtomicUsize,
    }

    impl FailingRunMirror {
        fn new() -> Self {
            Self {
                record_start_calls: std::sync::atomic::AtomicUsize::new(0),
                record_finish_calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    impl RunMirror for FailingRunMirror {
        fn record_start(&self, _run: &ScheduledRun) -> Result<(), String> {
            self.record_start_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err("simulated JSONL mirror outage (start)".into())
        }

        fn record_finish(
            &self,
            _run_id: &str,
            _status: RunStatus,
            _error: Option<String>,
            _spend: RunSpend,
        ) -> Result<(), String> {
            self.record_finish_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err("simulated JSONL mirror outage (finish)".into())
        }
    }

    fn failing_mirror_deps(
        tmp: &std::path::Path,
    ) -> (
        RoutineRunDeps,
        std::sync::Arc<shannon_core::inbox_store::InboxStore>,
    ) {
        let inbox: std::sync::Arc<shannon_core::inbox_store::InboxStore> = std::sync::Arc::new(
            shannon_core::inbox_store::InboxStore::open_with_legacy(&tmp.join("inbox.db"), None)
                .unwrap(),
        );
        let deps = RoutineRunDeps {
            inbox: inbox.clone(),
            runs_store: std::sync::Arc::new(FailingRunMirror::new()),
            webhook: std::sync::Arc::new(RecordingWebhookPort::default()),
            notify: std::sync::Arc::new(RecordingNotifyPort::default()),
            usage_store: std::sync::Arc::new(crate::commands_usage::UsageStore::with_path(
                tmp.join("usage.jsonl"),
            )),
            client_config: std::sync::Arc::new(RwLock::new(
                shannon_engine::api::types::LlmClientConfig::default(),
            )),
            desktop_config: std::sync::Arc::new(RwLock::new(DesktopConfig::default())),
            tools: std::sync::Arc::new(shannon_core::tools::ToolRegistry::new()),
            memory_store: std::sync::Arc::new(std::sync::RwLock::new(
                shannon_core::MemoryStore::new(tmp.join("memories")),
            )),
            scheduled_tasks: std::sync::Arc::new(
                shannon_core::scheduled_task_store::ScheduledTaskStore::with_base(
                    tmp.join("tasks"),
                ),
            ),
            sessions_dir: tmp.join("sessions"),
        };
        (deps, inbox)
    }

    fn finish_ctx(run_id: &str, started_ms: i64) -> RunFinishContext {
        RunFinishContext {
            run_id: run_id.to_string(),
            task_id: "task-1".into(),
            task_name: "Task One".into(),
            source: shannon_core::inbox_store::SOURCE_ROUTINE.into(),
            note: None,
            started_ms,
            notify_webhook: false,
            notify_on_failure: true,
        }
    }

    #[test]
    fn stamp_session_working_dir_writes_start_row_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        let session = uuid::Uuid::new_v4();

        stamp_session_working_dir(&container, session, "m1", "prov", "/work/x/");
        // A second stamp (log already started) must be a no-op.
        stamp_session_working_dir(&container, session, "m2", "prov2", "/work/y");

        let store = shannon_core::session_log::SessionStore::new(container);
        let events = store
            .read_events(&session)
            .unwrap()
            .expect("session log exists");
        let starts: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.body {
                shannon_types::session_event::SessionEventBody::SessionStart(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(starts.len(), 1, "exactly one session/start row");
        assert_eq!(starts[0].cwd.as_deref(), Some("/work/x"), "normalized");
        assert_eq!(starts[0].model, "m1", "first stamp wins");
    }

    #[test]
    fn stamp_session_working_dir_skips_a_log_that_already_started() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        let session = uuid::Uuid::new_v4();

        // Pre-existing log (as when the engine's tee already wrote it).
        let mut writer = shannon_core::session_log::SessionLogWriter::open_layout(
            &container,
            &session.to_string(),
        )
        .unwrap();
        writer.record(
            shannon_types::session_event::SessionEventBody::SessionStart(
                shannon_types::session_event::SessionStartPayload {
                    model: "tee-model".into(),
                    provider: None,
                    cwd: Some("/process/cwd".into()),
                    app_version: None,
                    os: None,
                    arch: None,
                    browser_cdp: None,
                },
            ),
        );
        writer.close().unwrap();

        stamp_session_working_dir(&container, session, "m", "p", "/work/y");

        let store = shannon_core::session_log::SessionStore::new(container);
        let events = store.read_events(&session).unwrap().unwrap();
        let starts: Vec<_> = events
            .iter()
            .filter(|e| {
                matches!(
                    &e.body,
                    shannon_types::session_event::SessionEventBody::SessionStart(_)
                )
            })
            .collect();
        assert_eq!(starts.len(), 1, "no second session/start appended");
    }

    #[test]
    fn sqlite_run_finishes_even_when_jsonl_mirror_fails() {
        // Important 1 (fix round 1): the legacy JSONL mirror being broken
        // must not leave the SQLite `routine_runs` row stuck in `running`.
        let app = tauri::test::mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let (deps, inbox) = failing_mirror_deps(tmp.path());

        let run_id = deps.inbox.record_run_start("task-1", "Task One").unwrap();

        finalize_run(
            &deps,
            app.handle(),
            finish_ctx(&run_id, chrono::Utc::now().timestamp_millis() - 4_000),
            RunOutcome {
                failed: false,
                error: None,
                output: "run finished fine".into(),
                session_id: Some("0195abcd-0000-7000-8000-000000000000".into()),
                cost_usd: None,
                token_usage: None,
            },
            RunSpend::default(),
        );

        let runs = inbox.list_runs(10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_ne!(runs[0].status, "running", "run must not stay running");
        assert_eq!(runs[0].status, "succeeded");
        assert!(runs[0].finished_at_ms.is_some());
        assert!(runs[0].duration_ms.is_some());
        // The inbox item still lands and is back-linked despite the mirror.
        assert!(runs[0].inbox_item_id.is_some());
        let items = inbox.list(None, None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Task One");
    }

    #[test]
    fn sqlite_run_marks_failed_when_engine_failed_and_mirror_fails() {
        let app = tauri::test::mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let (deps, inbox) = failing_mirror_deps(tmp.path());

        let run_id = deps.inbox.record_run_start("task-1", "Task One").unwrap();

        finalize_run(
            &deps,
            app.handle(),
            finish_ctx(&run_id, chrono::Utc::now().timestamp_millis() - 1_000),
            RunOutcome {
                failed: true,
                error: Some("provider unreachable".into()),
                output: String::new(),
                session_id: Some("0195abcd-0000-7000-8000-000000000001".into()),
                cost_usd: None,
                token_usage: None,
            },
            RunSpend::default(),
        );

        let runs = inbox.list_runs(10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "failed", "terminal state must be failed");
        assert!(runs[0].finished_at_ms.is_some());
        let items = inbox.list(None, None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].status, "pending");
        assert_eq!(items[0].error.as_deref(), Some("provider unreachable"));
    }

    #[test]
    fn finalize_persists_cost_and_tokens_to_both_run_stores() {
        // R2-W2-2: the finalize chain writes the run's observed spend to the
        // authoritative SQLite row AND the legacy JSONL mirror, and the
        // History read path (`run_record_to_execution`) carries it back out —
        // the OPC/History cost columns recover without UI changes.
        let app = tauri::test::mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let (deps, _inbox) = webhook_deps(
            tmp.path(),
            std::sync::Arc::new(RecordingWebhookPort::default()),
        );
        let jsonl = jsonl_store(tmp.path());

        let run_id = deps.inbox.record_run_start("task-1", "Task One").unwrap();
        // spawn_routine_run mirrors the start into the JSONL store before the
        // run; reproduce that here (record_finish updates the existing run).
        let mut jsonl_run = ScheduledRun::start("task-1", "Task One");
        jsonl_run.run_id = run_id.clone();
        jsonl.record(&jsonl_run).unwrap();
        let spend = RunSpend {
            cost_usd: Some(0.1234),
            token_usage: Some(42_000),
        };
        finalize_run(
            &deps,
            app.handle(),
            finish_ctx(&run_id, chrono::Utc::now().timestamp_millis() - 1_000),
            RunOutcome {
                failed: false,
                error: None,
                output: "done".into(),
                session_id: Some("0195abcd-0000-7000-8000-000000000003".into()),
                cost_usd: spend.cost_usd,
                token_usage: spend.token_usage,
            },
            spend,
        );

        // SQLite (authoritative).
        let record = &deps.inbox.list_runs(10).unwrap()[0];
        assert_eq!(record.cost_usd, Some(0.1234));
        assert_eq!(record.token_usage, Some(42_000));
        // Read path projects the spend through to the UI contract.
        let exec = run_record_to_execution(record);
        assert_eq!(exec.cost_usd, Some(0.1234));
        assert_eq!(exec.token_usage, Some(42_000));
        // JSONL mirror carries the same totals.
        let mirrored = jsonl.find_by_id(&run_id).unwrap().expect("mirrored run");
        assert_eq!(mirrored.cost_usd, Some(0.1234));
        assert_eq!(mirrored.token_usage, Some(42_000));
    }

    // ── B6': routine-finish webhook routing ─────────────────────────────

    /// Recording double for the [`RoutineWebhookPort`] seam.
    #[derive(Default)]
    struct RecordingWebhookPort {
        configured: std::sync::atomic::AtomicBool,
        deliveries: std::sync::Mutex<Vec<shannon_core::notifier::Notification>>,
    }

    impl RecordingWebhookPort {
        fn with_configured(configured: bool) -> std::sync::Arc<Self> {
            let port = Self::default();
            port.configured
                .store(configured, std::sync::atomic::Ordering::SeqCst);
            std::sync::Arc::new(port)
        }

        fn deliveries(&self) -> Vec<shannon_core::notifier::Notification> {
            self.deliveries.lock().unwrap().clone()
        }
    }

    impl RoutineWebhookPort for RecordingWebhookPort {
        fn is_configured(&self) -> bool {
            self.configured.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn deliver(&self, notification: &shannon_core::notifier::Notification) {
            self.deliveries.lock().unwrap().push(notification.clone());
        }
    }

    /// Records budget-abort notifications instead of touching the platform
    /// notifier (W2-3 test seam for [`RunNotifyPort`]).
    #[derive(Default)]
    struct RecordingNotifyPort {
        dispatches: std::sync::Mutex<Vec<shannon_core::notifier::Notification>>,
    }

    impl RecordingNotifyPort {
        fn dispatches(&self) -> Vec<shannon_core::notifier::Notification> {
            self.dispatches.lock().unwrap().clone()
        }
    }

    impl RunNotifyPort for RecordingNotifyPort {
        fn notify(&self, notification: &shannon_core::notifier::Notification) {
            self.dispatches.lock().unwrap().push(notification.clone());
        }
    }

    /// Deps with a recording webhook port under the caller's control.
    fn webhook_deps(
        tmp: &std::path::Path,
        port: std::sync::Arc<RecordingWebhookPort>,
    ) -> (
        RoutineRunDeps,
        std::sync::Arc<shannon_core::inbox_store::InboxStore>,
    ) {
        let inbox: std::sync::Arc<shannon_core::inbox_store::InboxStore> = std::sync::Arc::new(
            shannon_core::inbox_store::InboxStore::open_with_legacy(&tmp.join("inbox.db"), None)
                .unwrap(),
        );
        let deps = RoutineRunDeps {
            inbox: inbox.clone(),
            runs_store: std::sync::Arc::new(ScheduledRunsStore::with_base(tmp.join("runs"))),
            webhook: port,
            notify: std::sync::Arc::new(RecordingNotifyPort::default()),
            usage_store: std::sync::Arc::new(crate::commands_usage::UsageStore::with_path(
                tmp.join("usage.jsonl"),
            )),
            client_config: std::sync::Arc::new(RwLock::new(
                shannon_engine::api::types::LlmClientConfig::default(),
            )),
            desktop_config: std::sync::Arc::new(RwLock::new(DesktopConfig::default())),
            tools: std::sync::Arc::new(shannon_core::tools::ToolRegistry::new()),
            memory_store: std::sync::Arc::new(std::sync::RwLock::new(
                shannon_core::MemoryStore::new(tmp.join("memories")),
            )),
            scheduled_tasks: std::sync::Arc::new(
                shannon_core::scheduled_task_store::ScheduledTaskStore::with_base(
                    tmp.join("tasks"),
                ),
            ),
            sessions_dir: tmp.join("sessions"),
        };
        (deps, inbox)
    }

    #[test]
    fn notify_webhook_flag_serialization_roundtrip_and_legacy_default() {
        // Roundtrip: a routine with the flag set keeps it across save/load JSON.
        let mut routine = ScheduledRoutine::new("hooked".into(), "p".into(), 60);
        routine.notify_webhook = true;
        let json = serde_json::to_string(&routine).unwrap();
        assert!(json.contains("\"notify_webhook\":true"), "{json}");
        let back: ScheduledRoutine = serde_json::from_str(&json).unwrap();
        assert!(back.notify_webhook);

        // Legacy: JSON written before the field existed loads with false.
        let legacy: ScheduledRoutine =
            serde_json::from_str(r#"{"id":"old","name":"n","prompt":"p","created_at":"2026-01-01T00:00:00Z","enabled":true}"#).unwrap();
        assert!(!legacy.notify_webhook);
    }

    #[test]
    fn webhook_delivers_task_name_status_and_summary_when_configured() {
        let app = tauri::test::mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let port = RecordingWebhookPort::with_configured(true);
        let (deps, inbox) = webhook_deps(tmp.path(), port.clone());

        let run_id = deps.inbox.record_run_start("task-1", "Task One").unwrap();
        let mut ctx = finish_ctx(&run_id, chrono::Utc::now().timestamp_millis() - 1_000);
        ctx.notify_webhook = true;

        finalize_run(
            &deps,
            app.handle(),
            ctx,
            RunOutcome {
                failed: false,
                error: None,
                output: "weekly numbers are in\nsecond line".into(),
                session_id: Some("0195abcd-0000-7000-8000-000000000002".into()),
                cost_usd: None,
                token_usage: None,
            },
            RunSpend::default(),
        );

        let deliveries = port.deliveries();
        assert_eq!(deliveries.len(), 1, "exactly one webhook delivery");
        assert_eq!(deliveries[0].title, "Task One — succeeded");
        assert_eq!(deliveries[0].body, "weekly numbers are in");
        // The run record carries no skip note when delivery happened.
        let items = inbox.list(None, None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert!(
            !items[0].summary.contains("webhook"),
            "no skip annotation expected: {}",
            items[0].summary
        );
    }

    #[test]
    fn webhook_failure_uses_error_as_body_and_error_level() {
        let app = tauri::test::mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let port = RecordingWebhookPort::with_configured(true);
        let (deps, _inbox) = webhook_deps(tmp.path(), port.clone());

        let run_id = deps.inbox.record_run_start("task-1", "Task One").unwrap();
        let mut ctx = finish_ctx(&run_id, chrono::Utc::now().timestamp_millis());
        ctx.notify_webhook = true;

        finalize_run(
            &deps,
            app.handle(),
            ctx,
            RunOutcome {
                failed: true,
                error: Some("provider unreachable".into()),
                output: String::new(),
                session_id: None,
                cost_usd: None,
                token_usage: None,
            },
            RunSpend::default(),
        );

        let deliveries = port.deliveries();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].title, "Task One — failed");
        assert_eq!(deliveries[0].body, "provider unreachable");
        assert!(matches!(
            deliveries[0].level,
            shannon_core::notifier::NotificationLevel::Error
        ));
    }

    #[test]
    fn webhook_skips_silently_with_run_record_note_when_not_configured() {
        let app = tauri::test::mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let port = RecordingWebhookPort::with_configured(false);
        let (deps, inbox) = webhook_deps(tmp.path(), port.clone());

        let run_id = deps.inbox.record_run_start("task-1", "Task One").unwrap();
        let mut ctx = finish_ctx(&run_id, chrono::Utc::now().timestamp_millis());
        ctx.notify_webhook = true;

        finalize_run(
            &deps,
            app.handle(),
            ctx,
            RunOutcome {
                failed: false,
                error: None,
                output: "did the thing".into(),
                session_id: None,
                cost_usd: None,
                token_usage: None,
            },
            RunSpend::default(),
        );

        // Nothing delivered, nothing raised — but the run record notes why.
        assert!(port.deliveries().is_empty(), "no delivery without a sink");
        let items = inbox.list(None, None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert!(
            items[0]
                .summary
                .contains("webhook notification skipped (not configured)"),
            "skip note must annotate the run record: {}",
            items[0].summary
        );
        assert_eq!(items[0].error, None, "the skip is not an error");
    }

    #[test]
    fn webhook_flag_off_never_touches_the_port() {
        let app = tauri::test::mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let port = RecordingWebhookPort::with_configured(true);
        let (deps, inbox) = webhook_deps(tmp.path(), port.clone());

        let run_id = deps.inbox.record_run_start("task-1", "Task One").unwrap();
        // finish_ctx leaves notify_webhook = false.
        finalize_run(
            &deps,
            app.handle(),
            finish_ctx(&run_id, chrono::Utc::now().timestamp_millis()),
            RunOutcome {
                failed: false,
                error: None,
                output: "plain run".into(),
                session_id: None,
                cost_usd: None,
                token_usage: None,
            },
            RunSpend::default(),
        );

        assert!(port.deliveries().is_empty(), "flag off = no delivery");
        let items = inbox.list(None, None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert!(
            !items[0].summary.contains("webhook"),
            "no annotation when the flag is off: {}",
            items[0].summary
        );
    }

    /// A stand-in for the engine future that panics instead of returning.
    fn panicking_engine_outcome() -> RunOutcome {
        panic!("boom");
    }

    #[tokio::test]
    async fn panicked_engine_task_is_recorded_as_failed() {
        // Keep the test output clean: silence the default panic hook while
        // the guarded future intentionally panics.
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        // P1-2: the panic guard lives inside run_with_timeout now (the
        // spawned attempt's JoinError maps to a failed outcome).
        let outcome = run_with_timeout(async { panicking_engine_outcome() }, None, None).await;

        std::panic::set_hook(prev_hook);

        assert!(outcome.failed, "panic must map to a failed outcome");
        assert!(
            outcome.error.unwrap().contains("panicked"),
            "error should mention the panic"
        );
        assert!(outcome.session_id.is_none(), "no session was opened");
    }

    // ── P1-2: ExecutionPolicy wiring (timeout / retries / budget / worktree)

    fn fast_retry_policy(max_attempts: u32) -> RetryPolicy {
        // Deterministic zero-delay retries so the tests exercise the loop,
        // not the wall clock.
        RetryPolicy {
            max_attempts,
            base_delay_secs: 0,
            max_delay_secs: 0,
            jitter_ratio: 0.0,
        }
    }

    fn ok_outcome() -> RunOutcome {
        RunOutcome {
            failed: false,
            error: None,
            output: "done".into(),
            session_id: Some("0195abcd-0000-7000-8000-00000000ffff".into()),
            cost_usd: None,
            token_usage: None,
        }
    }

    fn failed_outcome(error: &str) -> RunOutcome {
        RunOutcome {
            failed: true,
            error: Some(error.into()),
            output: String::new(),
            session_id: None,
            cost_usd: None,
            token_usage: None,
        }
    }

    /// Drive [`execute_with_policy`] with a counting engine factory whose
    /// per-attempt outcomes come from `results` (last one repeats).
    #[allow(clippy::too_many_arguments)]
    async fn drive_policy(
        results: Vec<RunOutcome>,
        retry: RetryPolicy,
        timeout: Option<Duration>,
    ) -> (RunOutcome, usize) {
        drive_policy_with_budget(results, retry, timeout, None).await
    }

    /// [`drive_policy`] variant that also arms the mid-run budget guard.
    async fn drive_policy_with_budget(
        results: Vec<RunOutcome>,
        retry: RetryPolicy,
        timeout: Option<Duration>,
        budget: Option<tokio::sync::watch::Receiver<bool>>,
    ) -> (RunOutcome, usize) {
        use std::sync::atomic::AtomicUsize;
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();
        let results = std::sync::Arc::new(std::sync::Mutex::new(results));
        let policy = RunExecutionPolicy { timeout, retry };
        let outcome = execute_with_policy(
            move || {
                // Counted at factory time — one increment per attempt MINTED,
                // so a budget-aborted attempt that is cancelled before its
                // first poll still counts (the abort means "no second paid
                // attempt", not "the attempt never existed").
                calls_for_closure.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let results = results.clone();
                async move {
                    // Consume the fixture list; an exhausted list repeats
                    // the sentinel failure.
                    let mut r = results.lock().unwrap();
                    if r.is_empty() {
                        failed_outcome("exhausted fixtures")
                    } else {
                        r.remove(0)
                    }
                }
            },
            &policy,
            budget,
        )
        .await;
        let count = calls.load(std::sync::atomic::Ordering::SeqCst);
        (outcome, count)
    }

    #[tokio::test]
    async fn policy_timeout_aborts_a_runaway_attempt() {
        let policy = RunExecutionPolicy {
            timeout: Some(Duration::from_millis(30)),
            retry: RetryPolicy::disabled(),
        };
        let outcome = execute_with_policy(
            || async {
                // Far beyond the 30ms budget — the attempt must be aborted.
                tokio::time::sleep(Duration::from_secs(30)).await;
                ok_outcome()
            },
            &policy,
            None,
        )
        .await;
        assert!(outcome.failed, "timeout must map to a failed outcome");
        let error = outcome.error.expect("timeout reason recorded");
        assert!(error.contains("timed out"), "{error}");
    }

    #[tokio::test]
    async fn policy_without_timeout_is_unlimited() {
        let policy = RunExecutionPolicy {
            timeout: None,
            retry: RetryPolicy::disabled(),
        };
        let outcome = execute_with_policy(|| async { ok_outcome() }, &policy, None).await;
        assert!(!outcome.failed);
    }

    // ── W2-3: mid-run budget guard ──────────────────────────────────────

    #[tokio::test]
    async fn budget_trip_aborts_a_running_attempt() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        // Trip the stop from a side task — the engine future never finishes
        // on its own, so without this the attempt (and the test) would park
        // forever.
        let stop = tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let _ = stop.send(true);
        });
        let outcome = run_with_timeout(
            async {
                std::future::pending::<()>().await;
                ok_outcome()
            },
            None,
            Some(rx),
        )
        .await;
        assert!(outcome.failed, "a budget trip must fail the attempt");
        let error = outcome.error.expect("abort reason recorded");
        assert!(
            error.starts_with(BUDGET_ABORT_MARKER),
            "machine-readable reason expected: {error}"
        );
        assert!(outcome.session_id.is_none(), "abort outcome has no session");
        drop(tx);
    }

    #[tokio::test]
    async fn untripped_budget_channel_never_fires() {
        // Same shape as above but the sender never trips — the engine future
        // completing wins the race (no budget configured behavior is
        // byte-for-byte the legacy one).
        let (tx, rx) = tokio::sync::watch::channel(false);
        let outcome = run_with_timeout(async { ok_outcome() }, None, Some(rx)).await;
        assert!(!outcome.failed);
        drop(tx);
    }

    #[tokio::test]
    async fn budget_trip_ends_the_whole_run_without_retries() {
        // Retry-allowing policy + a channel already tripped: the first
        // attempt's abort must end the run — no second paid attempt.
        let (tx, rx) = tokio::sync::watch::channel(false);
        tx.send(true).expect("trip");
        let (outcome, calls) = drive_policy_with_budget(
            vec![failed_outcome("connection reset")],
            fast_retry_policy(3),
            None,
            Some(rx),
        )
        .await;
        assert!(outcome.failed);
        assert_eq!(calls, 1, "a budget stop is never retried");
        let error = outcome.error.expect("error");
        assert!(error.contains(BUDGET_ABORT_MARKER), "{error}");
    }

    #[test]
    fn run_spend_tracker_accumulates_and_merges_last_known() {
        let tracker = RunSpendTracker::default();
        assert_eq!(tracker.snapshot(), RunSpend::default(), "nothing yet");
        assert!(tracker.session_id().is_none());

        tracker.add_usage(0.01, 100);
        tracker.add_usage(0.02, 23);
        assert_eq!(
            tracker.snapshot(),
            RunSpend {
                cost_usd: Some(0.03),
                token_usage: Some(123),
            }
        );

        let session = uuid::Uuid::new_v4();
        tracker.set_session(session);
        assert_eq!(
            tracker.session_id().as_deref(),
            Some(session.to_string()).as_deref()
        );

        // Last-known-wins merge: tracker totals beat the engine outcome's
        // own sum; `None`s keep the other side.
        let outcome_spend = RunSpend {
            cost_usd: Some(0.01),
            token_usage: None,
        };
        assert_eq!(
            outcome_spend.merge(tracker.snapshot()),
            RunSpend {
                cost_usd: Some(0.03),
                token_usage: Some(123),
            }
        );
        assert_eq!(
            RunSpend::default().merge(RunSpend::default()),
            RunSpend::default()
        );
    }

    #[test]
    fn budget_live_exceeded_uses_month_plus_in_flight() {
        let tmp = tempfile::tempdir().unwrap();
        let (inbox, usage) = budget_fixture(tmp.path());
        // The fixture run's spend: $1.50 attributed to "task-budget" via the
        // back-linked session "s-routine" (budget_fixture itself only wires
        // the run → item → session chain; the ledger line is ours to add).
        let now = chrono::Utc::now();
        let month_start_ms = chrono::Utc
            .from_utc_datetime(
                &now.date_naive()
                    .with_day(1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            )
            .timestamp_millis();
        let _ = usage.append(&usage_record(
            Some("s-routine"),
            1.5,
            month_start_ms + 1_000,
        ));
        // Month alone below the cap → run.
        assert!(!budget_live_exceeded(
            &usage,
            &inbox,
            "task-budget",
            2.0,
            None,
            now
        ));
        // Month + in-flight reaches the cap → trip.
        assert!(budget_live_exceeded(
            &usage,
            &inbox,
            "task-budget",
            2.0,
            Some(0.5),
            now
        ));
        // Exactly at the cap counts as reached (same >= rule as the
        // pre-execution gate).
        assert!(budget_live_exceeded(
            &usage,
            &inbox,
            "task-budget",
            1.5,
            None,
            now
        ));
        // A routine without history never trips on in-flight alone.
        assert!(!budget_live_exceeded(
            &usage,
            &inbox,
            "ghost",
            1.0,
            Some(0.5),
            now
        ));
    }

    #[test]
    fn budget_abort_notification_fires_through_the_port() {
        let port = RecordingNotifyPort::default();
        notify_budget_abort(&port, "Nightly Scan", 5.0, 5.01);
        let dispatches = port.dispatches();
        assert_eq!(dispatches.len(), 1, "exactly one desktop notification");
        let n = &dispatches[0];
        assert!(n.title.contains("Nightly Scan"), "{}", n.title);
        assert!(n.body.contains("$5.00"), "{}", n.body);
        assert!(n.body.contains("$5.01"), "{}", n.body);
        assert!(matches!(
            n.level,
            shannon_core::notifier::NotificationLevel::Error
        ));
        assert_eq!(n.source.as_deref(), Some("routine_budget_abort"));
    }

    #[tokio::test]
    async fn policy_retries_until_success_and_counts_attempts() {
        let (outcome, calls) = drive_policy(
            vec![failed_outcome("connection reset"), ok_outcome()],
            fast_retry_policy(3),
            None,
        )
        .await;
        assert!(!outcome.failed, "second attempt should succeed");
        assert_eq!(calls, 2, "original + one retry");
    }

    #[tokio::test]
    async fn policy_gives_up_after_the_retry_budget_and_annotates_the_error() {
        let (outcome, calls) = drive_policy(vec![], fast_retry_policy(2), None).await;
        // drive_policy's empty fixture list repeats `failed_outcome("exhausted fixtures")`.
        assert!(outcome.failed);
        assert_eq!(
            calls, 2,
            "core convention: max_retries counts total attempts"
        );
        let error = outcome.error.expect("final error");
        assert!(error.contains("gave up after 2 attempt"), "{error}");
        assert!(
            error.contains("AttemptsExhausted"),
            "give-up reason recorded: {error}"
        );
    }

    #[tokio::test]
    async fn policy_zero_retries_never_retries() {
        let (outcome, calls) = drive_policy(
            vec![failed_outcome("connection reset")],
            fast_retry_policy(0),
            None,
        )
        .await;
        assert!(outcome.failed);
        assert_eq!(calls, 1, "no retry budget → single attempt");
        assert!(outcome.error.unwrap().contains("RetriesDisabled"));
    }

    #[tokio::test]
    async fn policy_non_retryable_errors_stop_immediately() {
        let (outcome, calls) = drive_policy(
            vec![failed_outcome("401 unauthorized: invalid api key")],
            fast_retry_policy(3),
            None,
        )
        .await;
        assert!(outcome.failed);
        assert_eq!(calls, 1, "auth errors are hard failures");
        assert!(outcome.error.unwrap().contains("NonRetryableError"));
    }

    #[test]
    fn run_execution_policy_maps_fields() {
        let mut routine = ScheduledRoutine::new("t".into(), "p".into(), 60);
        // No policy → fully legacy behavior.
        let bare = RunExecutionPolicy::of(&routine);
        assert!(bare.timeout.is_none());
        assert!(!bare.retry.allows_retry());

        routine.policy = Some(shannon_core::scheduled_routines::ExecutionPolicy {
            max_retries: 3,
            timeout_secs: 90,
            ..Default::default()
        });
        let wired = RunExecutionPolicy::of(&routine);
        assert_eq!(wired.timeout, Some(Duration::from_secs(90)));
        assert_eq!(wired.retry.max_attempts, 3);

        // timeout_secs = 0 means "no timeout", not "time out instantly".
        routine.policy = Some(shannon_core::scheduled_routines::ExecutionPolicy {
            timeout_secs: 0,
            ..Default::default()
        });
        assert!(RunExecutionPolicy::of(&routine).timeout.is_none());
    }

    /// Inbox + usage fixture for the budget tests: one routine run whose
    /// usage is attributed to session `s-routine` via the back-linked item.
    fn budget_fixture(tmp: &std::path::Path) -> (InboxStore, crate::commands_usage::UsageStore) {
        let inbox = InboxStore::open_in_memory().unwrap();
        let run_id = inbox.record_run_start("task-budget", "Budgeted").unwrap();
        let item = inbox
            .append_item(InboxItemNew {
                source: shannon_core::inbox_store::SOURCE_ROUTINE.into(),
                source_id: Some("task-budget".into()),
                session_id: Some("s-routine".into()),
                title: "Budgeted".into(),
                summary: String::new(),
                error: None,
            })
            .unwrap();
        inbox
            .record_run_finish(&run_id, "succeeded", None, Some(item.id), None, None)
            .unwrap();
        let usage = crate::commands_usage::UsageStore::with_path(tmp.join("usage.jsonl"));
        (inbox, usage)
    }

    fn usage_record(
        session: Option<&str>,
        cost: f64,
        timestamp_ms: i64,
    ) -> crate::commands_usage::UsageRecord {
        crate::commands_usage::UsageRecord {
            timestamp_ms: timestamp_ms as u64,
            model: "m".into(),
            provider: "prov".into(),
            input_tokens: 1,
            output_tokens: 1,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            cost_usd: cost,
            session_id: session.map(str::to_string),
        }
    }

    #[test]
    fn routine_month_spend_sums_only_this_routine_this_month() {
        let tmp = tempfile::tempdir().unwrap();
        let (inbox, usage) = budget_fixture(tmp.path());
        let now = chrono::Utc::now();
        let month_start_ms = chrono::Utc
            .from_utc_datetime(
                &now.date_naive()
                    .with_day(1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            )
            .timestamp_millis();
        let _ = usage.append(&usage_record(
            Some("s-routine"),
            1.5,
            month_start_ms + 1_000,
        ));
        // Other routine's session — ignored.
        let _ = usage.append(&usage_record(Some("s-other"), 9.0, month_start_ms + 2_000));
        // Same session, last month — outside the monthly window.
        let _ = usage.append(&usage_record(
            Some("s-routine"),
            40.0,
            month_start_ms - 5_000,
        ));
        // Unattributed legacy line — invisible to the aggregation.
        let _ = usage.append(&usage_record(None, 100.0, month_start_ms + 3_000));

        let spend = routine_month_spend(&usage, &inbox, "task-budget", now);
        assert!((spend - 1.5).abs() < 1e-9, "{spend}");
    }

    #[test]
    fn routine_month_spend_tolerates_store_failures_and_empty_history() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = InboxStore::open_in_memory().unwrap();
        let usage = crate::commands_usage::UsageStore::with_path(tmp.path().join("usage.jsonl"));
        // No runs for the task → zero spend, no panic.
        assert_eq!(
            routine_month_spend(&usage, &inbox, "ghost", chrono::Utc::now()),
            0.0
        );
    }

    #[test]
    fn budget_skip_reason_triggers_only_at_the_configured_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let (inbox, usage) = budget_fixture(tmp.path());
        let _ = usage.append(&usage_record(
            Some("s-routine"),
            1.5,
            chrono::Utc::now().timestamp_millis(),
        ));
        let deps = RoutineRunDeps {
            inbox: std::sync::Arc::new(inbox),
            runs_store: std::sync::Arc::new(ScheduledRunsStore::with_base(tmp.path().join("runs"))),
            webhook: std::sync::Arc::new(RecordingWebhookPort::default()),
            notify: std::sync::Arc::new(RecordingNotifyPort::default()),
            usage_store: std::sync::Arc::new(usage),
            client_config: std::sync::Arc::new(RwLock::new(
                shannon_engine::api::types::LlmClientConfig::default(),
            )),
            desktop_config: std::sync::Arc::new(RwLock::new(DesktopConfig::default())),
            tools: std::sync::Arc::new(shannon_core::tools::ToolRegistry::new()),
            memory_store: std::sync::Arc::new(std::sync::RwLock::new(
                shannon_core::MemoryStore::new(tmp.path().join("memories")),
            )),
            scheduled_tasks: std::sync::Arc::new(
                shannon_core::scheduled_task_store::ScheduledTaskStore::with_base(
                    tmp.path().join("tasks"),
                ),
            ),
            sessions_dir: tmp.path().join("sessions"),
        };
        let policy = |cap: Option<f64>| -> ScheduledRoutine {
            let mut r = ScheduledRoutine::new("Budgeted".into(), "p".into(), 60);
            // The spend aggregation keys on the routine id.
            r.id = "task-budget".into();
            r.policy = Some(shannon_core::scheduled_routines::ExecutionPolicy {
                budget_usd: cap,
                ..Default::default()
            });
            r
        };

        // Over the cap → skip with the reason carrying both numbers.
        let reason = budget_skip_reason(&deps, &policy(Some(1.0))).expect("over budget");
        assert!(reason.contains("$1.50"), "{reason}");
        assert!(reason.contains("$1.00"), "{reason}");

        // Under the cap → run.
        assert!(budget_skip_reason(&deps, &policy(Some(50.0))).is_none());
        // Zero cap is "no budget configured".
        assert!(budget_skip_reason(&deps, &policy(Some(0.0))).is_none());
        // No budget field at all.
        let mut no_budget = ScheduledRoutine::new("Budgeted".into(), "p".into(), 60);
        no_budget.id = "task-budget".into();
        no_budget.policy = None;
        assert!(budget_skip_reason(&deps, &no_budget).is_none());
    }

    #[test]
    fn resolve_run_working_dir_disabled_keeps_the_sidecar() {
        let routine = ScheduledRoutine::new("abc12345".into(), "p".into(), 60);
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_run_working_dir(&routine, Some("/proj/x"), tmp.path()).as_deref(),
            Some("/proj/x")
        );
        assert_eq!(resolve_run_working_dir(&routine, None, tmp.path()), None);
        // Blank sidecar entries are treated as "no project".
        assert_eq!(
            resolve_run_working_dir(&routine, Some("   "), tmp.path()),
            None
        );
    }

    #[test]
    fn resolve_run_working_dir_uses_an_existing_worktree_dir() {
        let mut routine = ScheduledRoutine::new("Daily Scan".into(), "p".into(), 60);
        routine.id = "abc12345".into();
        routine.policy = Some(shannon_core::scheduled_routines::ExecutionPolicy {
            worktree: Some("on".into()),
            ..Default::default()
        });
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("scheduled-worktrees");
        let existing = base.join(
            shannon_core::scheduled_worktree::ScheduledWorktree::dir_name(
                &routine.id,
                "Daily Scan",
            ),
        );
        std::fs::create_dir_all(&existing).unwrap();

        let resolved =
            resolve_run_working_dir(&routine, Some("/proj/y"), base.as_path()).expect("worktree");
        assert_eq!(std::path::Path::new(&resolved), existing);
    }

    #[test]
    fn resolve_run_working_dir_forks_a_worktree_from_the_project_repo() {
        let mut routine = ScheduledRoutine::new("Scan".into(), "p".into(), 60);
        routine.id = "abc12345".into();
        routine.policy = Some(shannon_core::scheduled_routines::ExecutionPolicy {
            worktree: Some("true".into()),
            ..Default::default()
        });

        // Real throw-away repo, mirroring scheduled_worktree's own tests.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let run = |dir: &std::path::Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .current_dir(dir)
                .args(args)
                .output()
                .expect("git available");
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        run(&repo, &["init"]);
        run(
            &repo,
            &["config", "user.email", "routine-test@shannon.local"],
        );
        run(&repo, &["config", "user.name", "Routine Test"]);
        std::fs::write(repo.join("f.txt"), "v1\n").unwrap();
        run(&repo, &["add", "-A"]);
        run(&repo, &["commit", "-q", "-m", "init"]);

        let base = tmp.path().join("scheduled-worktrees");
        let resolved =
            resolve_run_working_dir(&routine, Some(repo.to_str().unwrap()), base.as_path())
                .expect("auto-created worktree");
        let expected = base.join(
            shannon_core::scheduled_worktree::ScheduledWorktree::dir_name(&routine.id, "Scan"),
        );
        assert_eq!(std::path::Path::new(&resolved), expected);
        assert!(expected.is_dir(), "worktree directory forked from HEAD");
    }

    #[test]
    fn resolve_run_working_dir_falls_back_with_warning_without_a_repo() {
        let mut routine = ScheduledRoutine::new("Scan".into(), "p".into(), 60);
        routine.id = "abc12345".into();
        routine.policy = Some(shannon_core::scheduled_routines::ExecutionPolicy {
            worktree: Some("true".into()),
            ..Default::default()
        });
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("scheduled-worktrees");

        // No project dir at all → degrade to no isolation, no creation.
        assert_eq!(
            resolve_run_working_dir(&routine, None, base.as_path()),
            None
        );
        assert!(!base.exists(), "nothing forked without a repo");

        // A non-repo "project" → git fails → degrade to the sidecar dir.
        let not_a_repo = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_run_working_dir(
                &routine,
                Some(not_a_repo.path().to_str().unwrap()),
                base.as_path()
            )
            .as_deref(),
            Some(not_a_repo.path().to_str().unwrap())
        );
    }

    // ── T7: authoritative history read path ─────────────────────────────

    fn jsonl_store(tmp: &std::path::Path) -> ScheduledRunsStore {
        ScheduledRunsStore::with_base(tmp.join("runs").to_path_buf())
    }

    #[test]
    fn scheduled_run_to_record_maps_all_fields() {
        let mut run = ScheduledRun::start("task-9", "Nightly Scan");
        run.run_id = "abc12345".into();
        let started = run.started_at;
        run.finish(RunStatus::Failed, Some("provider unreachable".into()));

        let record = scheduled_run_to_record(&run);
        assert_eq!(record.id, "abc12345");
        assert_eq!(record.task_id, "task-9");
        assert_eq!(record.task_name.as_deref(), Some("Nightly Scan"));
        assert_eq!(record.status, "failed");
        assert_eq!(record.error.as_deref(), Some("provider unreachable"));
        assert_eq!(record.started_at_ms, Some(started.timestamp_millis()));
        let finished = record.finished_at_ms.expect("finished");
        assert_eq!(
            record.duration_ms,
            Some((finished - started.timestamp_millis()).max(0))
        );
        assert_eq!(record.inbox_item_id, None);
        assert_eq!(record.cost_usd, None, "legacy mirror had no spend");
        assert_eq!(record.token_usage, None);

        // Tombstone statuses render exactly like the JSONL projection.
        let mut queued = ScheduledRun::start("t", "T");
        queued.status = RunStatus::Queued;
        assert_eq!(scheduled_run_to_record(&queued).status, "queued");
        let mut cancelled = ScheduledRun::start("t", "T");
        cancelled.finish(RunStatus::Cancelled, None);
        assert_eq!(scheduled_run_to_record(&cancelled).status, "cancelled");
    }

    #[test]
    fn run_record_to_execution_preserves_run_id_and_converts_ms() {
        let record = RunRecord {
            id: "0195abcd-run".into(),
            task_id: "task-1".into(),
            task_name: Some("Task One".into()),
            status: "succeeded".into(),
            error: None,
            started_at_ms: Some(1_700_000_000_123),
            finished_at_ms: Some(1_700_000_005_000),
            duration_ms: Some(4_877),
            inbox_item_id: Some(7),
            cost_usd: None,
            token_usage: None,
        };
        let exec = run_record_to_execution(&record);
        // The run id is the real SQLite/JSONL id — get_execution_detail
        // keeps resolving projected rows.
        assert_eq!(exec.run_id, "0195abcd-run");
        assert_eq!(exec.task_id, "task-1");
        assert_eq!(exec.started_at, 1_700_000_000, "ms → whole seconds");
        assert_eq!(exec.finished_at, Some(1_700_000_005));
        assert_eq!(exec.status, "succeeded");
        assert!(exec.error_message.is_none());
        assert_eq!(exec.cost_usd, None, "untracked run stays cost-less");
        assert_eq!(exec.token_usage, None);

        // A missing/blank task_name falls back to the task id.
        let anon = RunRecord {
            task_name: None,
            ..record.clone()
        };
        assert_eq!(run_record_to_execution(&anon).task_name, "task-1");
        let blank = RunRecord {
            task_name: Some("   ".into()),
            ..record
        };
        assert_eq!(run_record_to_execution(&blank).task_name, "task-1");
    }

    #[test]
    fn read_run_history_prefers_the_inbox_store() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = InboxStore::open_in_memory().unwrap();
        let jsonl = jsonl_store(tmp.path());

        let run_id = inbox.record_run_start("task-a", "Alpha").unwrap();
        inbox
            .record_run_finish(&run_id, "succeeded", None, None, None, None)
            .unwrap();
        // Decoy: JSONL rows must be ignored while the inbox reads fine.
        jsonl.start_run("task-a", "Alpha").unwrap();

        let limit = 50usize;
        let rows = read_run_history(|| inbox.list_runs(50), &jsonl, None, limit).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].run_id, run_id, "SQLite row projected");
        assert_eq!(rows[0].status, "succeeded");

        // Task-filtered read goes through list_runs_by_task.
        inbox.record_run_start("task-b", "Beta").unwrap();
        let filtered = read_run_history(
            || inbox.list_runs_by_task("task-a", 50),
            &jsonl,
            Some("task-a"),
            limit,
        )
        .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].run_id, run_id);
    }

    #[test]
    fn read_run_history_falls_back_to_jsonl_when_inbox_read_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let jsonl = jsonl_store(tmp.path());
        let a = jsonl.start_run("task-a", "Alpha").unwrap();
        jsonl
            .update(&a, |r| r.finish(RunStatus::Succeeded, None))
            .unwrap();
        let b = jsonl.start_run("task-b", "Beta").unwrap();
        jsonl
            .update(&b, |r| r.finish(RunStatus::Failed, Some("boom".into())))
            .unwrap();

        // Simulated inbox outage: the read closure errors → the JSONL data
        // is returned (with only a warning) instead of failing the command.
        let rows = read_run_history(|| Err(InboxStoreError::Poisoned), &jsonl, None, 50).unwrap();
        assert_eq!(rows.len(), 2, "JSONL fallback serves every run");
        assert!(
            rows.iter().all(|r| r.run_id == a || r.run_id == b),
            "fallback rows come from the JSONL store"
        );
        assert!(rows.iter().any(|r| r.status == "failed"));

        // The fallback honours the task filter too.
        let filtered = read_run_history(
            || Err(InboxStoreError::Poisoned),
            &jsonl,
            Some("task-a"),
            50,
        )
        .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].run_id, a);
        assert_eq!(filtered[0].task_name, "Alpha");
    }

    // ── get_execution_detail read source (卡 2 follow-up) ───────────────

    #[test]
    fn read_run_detail_prefers_the_inbox_store() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = InboxStore::open_in_memory().unwrap();
        let jsonl = jsonl_store(tmp.path());

        let run_id = inbox.record_run_start("task-a", "Alpha").unwrap();
        inbox
            .record_run_finish(&run_id, "failed", Some("boom"), None, None, None)
            .unwrap();
        // Decoy: a same-id JSONL row must be ignored while the inbox row
        // resolves (the SQLite row is authoritative).
        let mut ghost = ScheduledRun::start("task-a", "JSONL Alpha");
        ghost.run_id = run_id.clone();
        jsonl.record(&ghost).unwrap();

        let exec = read_run_detail(
            || {
                Ok(inbox
                    .list_runs(10)
                    .unwrap()
                    .into_iter()
                    .find(|r| r.id == run_id))
            },
            &jsonl,
            &run_id,
        )
        .unwrap();
        // Same projection contract as the T7 list path (run_record_to_execution).
        assert_eq!(exec.run_id, run_id);
        assert_eq!(exec.task_id, "task-a");
        assert_eq!(exec.task_name, "Alpha");
        assert_eq!(exec.status, "failed");
        assert_eq!(exec.error_message.as_deref(), Some("boom"));
        assert_eq!(exec.cost_usd, None);
        assert_eq!(exec.token_usage, None);
    }

    #[test]
    fn read_run_detail_falls_back_to_jsonl_on_inbox_miss() {
        let tmp = tempfile::tempdir().unwrap();
        let jsonl = jsonl_store(tmp.path());

        let a = jsonl.start_run("task-a", "Alpha").unwrap();
        jsonl
            .update(&a, |r| r.finish(RunStatus::Failed, Some("boom".into())))
            .unwrap();

        // The run is not in `routine_runs` (e.g. a JSONL-only placeholder
        // the backfill skips) — the JSONL projection answers instead.
        let exec = read_run_detail(|| Ok(None), &jsonl, &a).unwrap();
        assert_eq!(exec.run_id, a);
        assert_eq!(exec.task_name, "Alpha");
        assert_eq!(exec.status, "failed");
        assert_eq!(exec.error_message.as_deref(), Some("boom"));

        // Neither store has it → the exact legacy "run not found" error.
        let err = read_run_detail(|| Ok(None), &jsonl, "missing").unwrap_err();
        assert_eq!(err, "run not found: missing");
    }

    #[test]
    fn read_run_detail_falls_back_to_jsonl_when_inbox_read_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let jsonl = jsonl_store(tmp.path());
        let a = jsonl.start_run("task-a", "Alpha").unwrap();
        jsonl
            .update(&a, |r| r.finish(RunStatus::Succeeded, None))
            .unwrap();
        // The row even exists in `routine_runs` (backfilled) — but the
        // store read is unavailable.
        let inbox = InboxStore::open_in_memory().unwrap();
        assert!(
            inbox
                .import_run(&scheduled_run_to_record(
                    &jsonl.find_by_id(&a).unwrap().expect("jsonl row")
                ))
                .unwrap()
        );

        // Simulated inbox outage: the read closure errors → the JSONL row
        // is returned (with only a warning) instead of failing the command.
        let exec = read_run_detail(|| Err(InboxStoreError::Poisoned), &jsonl, &a).unwrap();
        assert_eq!(exec.run_id, a);
        assert_eq!(exec.task_name, "Alpha");
        assert_eq!(exec.status, "succeeded");

        // Sanity: without the outage the backfilled inbox row wins.
        let hit = read_run_detail(
            || Ok(inbox.list_runs(10).unwrap().into_iter().next()),
            &jsonl,
            &a,
        )
        .unwrap();
        assert_eq!(hit.run_id, a);
        assert_eq!(hit.started_at, exec.started_at, "same run, same projection");
    }

    /// 抽样一致性：the same batch of runs, once read through the JSONL
    /// store and once backfilled into the inbox store, must project to
    /// identical `TaskExecution`s (count + every field), and the backfill
    /// must be idempotent across repeated passes.
    #[test]
    fn backfill_is_idempotent_and_projections_agree_with_jsonl() {
        let tmp = tempfile::tempdir().unwrap();
        let jsonl = jsonl_store(tmp.path());
        let inbox = InboxStore::open_in_memory().unwrap();

        // Batch: two tasks × terminal states, plus tombstones.
        let mut expected = Vec::new();
        for (task, name) in [("task-1", "Alpha"), ("task-2", "Beta")] {
            let id = jsonl.start_run(task, name).unwrap();
            jsonl
                .update(&id, |r| r.finish(RunStatus::Succeeded, None))
                .unwrap();
            expected.push(id);
            let id = jsonl.start_run(task, name).unwrap();
            jsonl
                .update(&id, |r| r.finish(RunStatus::Failed, Some("boom".into())))
                .unwrap();
            expected.push(id);
        }
        let mut queued = ScheduledRun::start("task-1", "Alpha");
        queued.status = RunStatus::Queued;
        let queued_id = queued.run_id.clone();
        jsonl.record(&queued).unwrap();
        expected.push(queued_id);
        let mut cancelled = ScheduledRun::start("task-2", "Beta");
        cancelled.finish(RunStatus::Cancelled, Some("superseded".into()));
        let cancelled_id = cancelled.run_id.clone();
        jsonl.record(&cancelled).unwrap();
        expected.push(cancelled_id);
        // A `running` ghost (crashed drain / legacy placeholder) must be
        // skipped — see backfill_runs_from_jsonl docs. The JSONL
        // store thus holds `expected.len() + 1` distinct runs.
        jsonl.start_run("task-1", "Alpha").unwrap();

        // First pass imports every terminal/tombstone run; the second pass
        // is a no-op.
        assert_eq!(
            backfill_runs_from_jsonl(&inbox, &jsonl).unwrap(),
            (expected.len(), 0)
        );
        assert_eq!(
            backfill_runs_from_jsonl(&inbox, &jsonl).unwrap(),
            (0, expected.len()),
            "repeat backfill must not duplicate"
        );
        assert_eq!(inbox.list_runs(100).unwrap().len(), expected.len());
        assert!(
            !inbox
                .list_runs(100)
                .unwrap()
                .iter()
                .any(|r| r.status == "running"),
            "running ghosts are not imported"
        );

        // Sampling consistency: per JSONL run, the inbox row projects to
        // the exact same TaskExecution as the JSONL projection (the
        // running ghost is excluded on both sides).
        let jsonl_runs: Vec<_> = jsonl
            .list_recent(100)
            .unwrap()
            .into_iter()
            .filter(|r| r.status != RunStatus::Running)
            .collect();
        assert_eq!(jsonl_runs.len(), expected.len());
        assert_eq!(jsonl.list_recent(100).unwrap().len(), expected.len() + 1);
        let inbox_runs = inbox.list_runs(100).unwrap();
        for run in &jsonl_runs {
            let record = inbox_runs
                .iter()
                .find(|r| r.id == run.run_id)
                .unwrap_or_else(|| panic!("run {} not backfilled", run.run_id));
            assert_eq!(
                &run_record_to_execution(record),
                &crate::scheduled_commands::run_to_execution(run),
                "projection mismatch for run {}",
                run.run_id
            );
        }

        // The task-filtered authoritative read sees the same rows.
        let for_task = inbox.list_runs_by_task("task-1", 100).unwrap();
        assert_eq!(for_task.len(), 3, "2 terminal runs + queued tombstone");
    }

    // ── W3-2: consecutive-failure auto pause + notify_on_failure wiring ──

    /// Deps for the auto-pause tests: the recording webhook port comes back
    /// from [`webhook_deps`] itself (the stored Arc IS the passed one); the
    /// desktop-notification seam is swapped for a fresh recording port the
    /// test keeps a typed handle to.
    fn auto_pause_deps(
        tmp: &std::path::Path,
        webhook: std::sync::Arc<RecordingWebhookPort>,
    ) -> (RoutineRunDeps, std::sync::Arc<RecordingNotifyPort>) {
        let (mut deps, _inbox) = webhook_deps(tmp, webhook);
        let notify = std::sync::Arc::new(RecordingNotifyPort::default());
        deps.notify = notify.clone();
        (deps, notify)
    }

    /// A routine persisted in the deps' task store under `task-1` (enabled),
    /// so the auto-pause path has something to flip.
    fn stored_routine(deps: &RoutineRunDeps) {
        let mut routine = ScheduledRoutine::new("Task One".into(), "p".into(), 3600);
        routine.id = "task-1".into();
        routine.enabled = true;
        deps.scheduled_tasks.save(&routine).unwrap();
    }

    /// One failed scheduled fire through the real `finalize_run`.
    fn finalize_scheduled_failure<R: tauri::Runtime>(
        deps: &RoutineRunDeps,
        app: &tauri::AppHandle<R>,
        n: usize,
        notify_on_failure: bool,
    ) {
        let run_id = deps.inbox.record_run_start("task-1", "Task One").unwrap();
        let mut ctx = finish_ctx(&run_id, chrono::Utc::now().timestamp_millis() - 1_000);
        ctx.source = shannon_core::inbox_store::SOURCE_ROUTINE.into();
        ctx.notify_on_failure = notify_on_failure;
        finalize_run(
            deps,
            app,
            ctx,
            RunOutcome {
                failed: true,
                error: Some(format!("attempt {n} blew up")),
                output: String::new(),
                session_id: None,
                cost_usd: None,
                token_usage: None,
            },
            RunSpend::default(),
        );
    }

    #[test]
    fn three_consecutive_scheduled_failures_auto_pause_and_annotate() {
        let app = tauri::test::mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let webhook = RecordingWebhookPort::with_configured(true);
        let (deps, notify) = auto_pause_deps(tmp.path(), webhook.clone());
        stored_routine(&deps);
        let app_handle = app.handle().clone();

        for n in 1..=3 {
            finalize_scheduled_failure(&deps, &app_handle, n, true);
        }

        // The routine is paused…
        let routine = deps.scheduled_tasks.load("task-1").unwrap().unwrap();
        assert!(!routine.enabled, "3 consecutive failures must disable");
        // …the pausing run record carries the machine-readable reason…
        let runs = deps.inbox.list_runs_by_task("task-1", 10).unwrap();
        assert_eq!(runs.len(), 3);
        assert!(
            runs[0]
                .error
                .as_deref()
                .unwrap()
                .starts_with(AUTO_PAUSE_MARKER),
            "pause reason prefix on the newest (3rd) record: {:?}",
            runs[0].error
        );
        assert!(
            !runs[1]
                .error
                .as_deref()
                .unwrap()
                .contains(AUTO_PAUSE_MARKER)
        );
        // …the inbox card names the self-heal path…
        let items = deps.inbox.list(None, None, 10).unwrap();
        assert!(items[0].summary.contains("re-enable it from the task card"));
        // …and the notifications went out: a failure alert per failed run
        // plus ONE dual-channel pause alert (transition-only).
        let dispatched = notify.dispatches();
        assert_eq!(dispatched.len(), 4, "3 failure alerts + 1 pause alert");
        assert_eq!(
            dispatched
                .iter()
                .filter(|n| n.source.as_deref() == Some("routine_auto_pause"))
                .count(),
            1,
            "exactly one desktop pause alert"
        );
        let deliveries = webhook.deliveries();
        assert_eq!(deliveries.len(), 1, "exactly one webhook pause alert");
        assert_eq!(
            deliveries[0].source.as_deref(),
            Some("routine_auto_pause_webhook")
        );
        assert!(
            deliveries[0]
                .body
                .contains("re-enable it from its task card")
        );

        // A 4th failure after the pause: still one pause alert total
        // (idempotent transition), the failure alert still fires.
        finalize_scheduled_failure(&deps, &app_handle, 4, true);
        let dispatched = notify.dispatches();
        assert_eq!(
            dispatched.len(),
            5,
            "one more failure alert, no second pause"
        );
        let deliveries = webhook.deliveries();
        assert_eq!(deliveries.len(), 1, "no duplicate pause webhook");
        let routine = deps.scheduled_tasks.load("task-1").unwrap().unwrap();
        assert!(!routine.enabled);
    }

    #[test]
    fn one_success_clears_the_consecutive_failure_streak() {
        let app = tauri::test::mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let (deps, _inbox) = webhook_deps(
            tmp.path(),
            std::sync::Arc::new(RecordingWebhookPort::default()),
        );
        stored_routine(&deps);
        let app_handle = app.handle().clone();

        for n in 1..=2 {
            finalize_scheduled_failure(&deps, &app_handle, n, true);
        }
        // A successful scheduled run resets the streak.
        let run_id = deps.inbox.record_run_start("task-1", "Task One").unwrap();
        finalize_run(
            &deps,
            app.handle(),
            finish_ctx(&run_id, chrono::Utc::now().timestamp_millis() - 1_000),
            RunOutcome {
                failed: false,
                error: None,
                output: "all good".into(),
                session_id: None,
                cost_usd: None,
                token_usage: None,
            },
            RunSpend::default(),
        );
        finalize_scheduled_failure(&deps, &app_handle, 3, true);

        let routine = deps.scheduled_tasks.load("task-1").unwrap().unwrap();
        assert!(routine.enabled, "the success in between clears the streak");
        let runs = deps.inbox.list_runs_by_task("task-1", 10).unwrap();
        assert!(
            !runs[0]
                .error
                .as_deref()
                .unwrap()
                .contains(AUTO_PAUSE_MARKER)
        );
    }

    #[test]
    fn manual_trigger_failures_do_not_count_toward_the_streak() {
        let app = tauri::test::mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let (deps, _inbox) = webhook_deps(
            tmp.path(),
            std::sync::Arc::new(RecordingWebhookPort::default()),
        );
        stored_routine(&deps);
        let app_handle = app.handle().clone();

        // Two scheduled failures, then THREE manual-trigger failures — the
        // manual ones must neither pause nor advance the streak.
        for n in 1..=2 {
            finalize_scheduled_failure(&deps, &app_handle, n, true);
        }
        for n in 1..=3 {
            let run_id = deps.inbox.record_run_start("task-1", "Task One").unwrap();
            let mut ctx = finish_ctx(&run_id, chrono::Utc::now().timestamp_millis() - 1_000);
            ctx.source = shannon_core::inbox_store::SOURCE_TRIGGER.into();
            finalize_run(
                &deps,
                app.handle(),
                ctx,
                RunOutcome {
                    failed: true,
                    error: Some(format!("manual {n} blew up")),
                    output: String::new(),
                    session_id: None,
                    cost_usd: None,
                    token_usage: None,
                },
                RunSpend::default(),
            );
        }
        let routine = deps.scheduled_tasks.load("task-1").unwrap().unwrap();
        assert!(routine.enabled, "manual failures must not pause");

        // The next scheduled failure is the THIRD scheduled one → pause.
        finalize_scheduled_failure(&deps, &app_handle, 3, true);
        let routine = deps.scheduled_tasks.load("task-1").unwrap().unwrap();
        assert!(!routine.enabled);
    }

    #[test]
    fn notify_on_failure_false_keeps_failures_silent_but_pauses() {
        let app = tauri::test::mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let webhook = RecordingWebhookPort::with_configured(true);
        let (deps, notify) = auto_pause_deps(tmp.path(), webhook.clone());
        stored_routine(&deps);
        let app_handle = app.handle().clone();

        for n in 1..=3 {
            finalize_scheduled_failure(&deps, &app_handle, n, false);
        }

        // Paused + annotated, but completely silent.
        let routine = deps.scheduled_tasks.load("task-1").unwrap().unwrap();
        assert!(!routine.enabled);
        assert_eq!(notify.dispatches().len(), 0, "no desktop alerts");
        assert_eq!(webhook.deliveries().len(), 0, "no webhook alerts");
        let runs = deps.inbox.list_runs_by_task("task-1", 10).unwrap();
        assert!(
            runs[0]
                .error
                .as_deref()
                .unwrap()
                .starts_with(AUTO_PAUSE_MARKER)
        );
    }
}
