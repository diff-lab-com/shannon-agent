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

use shannon_core::inbox_store::{
    InboxItem, InboxItemNew, InboxStats, InboxStatus, InboxStore, InboxStoreError, RunRecord,
};
use shannon_core::query_engine::{QueryContext, QueryEngine, QueryEvent, QueryMetadata};
use shannon_core::scheduled_routines::ScheduledRoutine;
use shannon_core::scheduled_runs::{RunStatus, ScheduledRun, ScheduledRunsStore};
use shannon_engine::api::client::LlmClient;
use shannon_engine::permissions::{PermissionManager, PermissionRuleChecker};
use shannon_engine::state::StateManager;
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
/// - `cost_usd`/`token_usage` ↔ `None` — never tracked in `routine_runs`;
///   the JSONL fields existed but no write path ever populated them either.
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
        cost_usd: None,
        token_usage: None,
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
/// `trigger_task_now` placeholder or a drained placeholder whose process
/// died between drain and retire). A run that truly started already owns
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

/// The legacy JSONL mirror seam. Object-safe on purpose: tests inject a
/// failing implementation to prove a mirror outage can never wedge the
/// SQLite `routine_runs` row (review fix round 1, Important 1).
pub(crate) trait RunMirror: Send + Sync {
    /// Append the initial `Running` record.
    fn record_start(&self, run: &ScheduledRun) -> Result<(), String>;
    /// Append the finish revision for `run_id`.
    fn record_finish(
        &self,
        run_id: &str,
        status: RunStatus,
        error: Option<String>,
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
    ) -> Result<(), String> {
        self.update(run_id, |r| r.finish(status, error))
            .map_err(|e| e.to_string())
    }
}

/// The state slices `spawn_routine_run` needs, Arc-cloned so the spawned
/// task owns its inputs. Constructed from [`AppState`] (Tauri commands) or
/// from the loopback trigger endpoint's state.
pub(crate) struct RoutineRunDeps {
    pub(crate) inbox: std::sync::Arc<shannon_core::inbox_store::InboxStore>,
    /// Legacy JSONL history mirror (best-effort — see `spawn_routine_run`).
    pub(crate) runs_store: std::sync::Arc<dyn RunMirror>,
    pub(crate) usage_store: std::sync::Arc<crate::commands_usage::UsageStore>,
    pub(crate) client_config: std::sync::Arc<RwLock<shannon_engine::api::types::LlmClientConfig>>,
    pub(crate) desktop_config: std::sync::Arc<RwLock<DesktopConfig>>,
    pub(crate) tools: std::sync::Arc<shannon_core::tools::ToolRegistry>,
    /// Shared memory store handle (P2-4b) — passed into the spawned runner so
    /// its engine attaches the same store the interactive path uses.
    pub(crate) memory_store: crate::commands_memory::SharedMemoryStore,
}

impl RoutineRunDeps {
    pub(crate) fn from_state(state: &AppState) -> Self {
        Self {
            inbox: state.inbox_store(),
            runs_store: state.scheduled_runs_store.clone(),
            usage_store: state.usage_store.clone(),
            client_config: state.client_config.clone(),
            desktop_config: state.desktop_config.clone(),
            tools: state.tools.clone(),
            memory_store: state.memory_store.clone(),
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
}

/// What the engine phase of a run produced. `session_id` is `None` when the
/// engine phase never got far enough to open a session (e.g. panic).
pub(crate) struct RunOutcome {
    pub(crate) failed: bool,
    pub(crate) error: Option<String>,
    pub(crate) output: String,
    pub(crate) session_id: Option<String>,
}

impl RunOutcome {
    /// Outcome for a task whose future panicked before returning.
    fn panicked(join_error: String) -> Self {
        Self {
            failed: true,
            error: Some(format!("routine task panicked: {join_error}")),
            output: String::new(),
            session_id: None,
        }
    }
}

/// Await the engine future, converting a panic in the unattended task into a
/// failed [`RunOutcome`] instead of silently leaving the run `running`
/// forever (review fix round 1, Important 1 — panic half).
async fn run_with_panic_guard<F>(engine_future: F) -> RunOutcome
where
    F: std::future::Future<Output = RunOutcome> + Send + 'static,
{
    match tokio::spawn(engine_future).await {
        Ok(outcome) => outcome,
        Err(join_error) => RunOutcome::panicked(join_error.to_string()),
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
/// - The engine phase runs under [`run_with_panic_guard`]; a panic still
///   reaches [`finalize_run`], which marks the run failed.
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

    let finish_deps = RoutineRunDeps {
        inbox: deps.inbox.clone(),
        runs_store: deps.runs_store.clone(),
        usage_store: deps.usage_store.clone(),
        client_config: deps.client_config.clone(),
        desktop_config: deps.desktop_config.clone(),
        tools: deps.tools.clone(),
        memory_store: deps.memory_store.clone(),
    };
    let ctx = RunFinishContext {
        run_id: run_id.clone(),
        task_id: routine.id.clone(),
        task_name: routine.name.clone(),
        source: inbox_source.to_string(),
        note,
        started_ms: chrono::Utc::now().timestamp_millis(),
    };

    let engine_future = async move {
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

        let engine = crate::commands_memory::attach_shared_memory(
            QueryEngine::with_defaults_arc(client, tools, permissions, StateManager::new()),
            &memory_store,
        );

        let session_id = uuid::Uuid::new_v4();
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
                        // Best-effort ledger write, mirroring background tasks.
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
                            None,
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
        }
    };

    tokio::spawn(async move {
        let outcome = run_with_panic_guard(engine_future).await;
        finalize_run(&finish_deps, &app, ctx, outcome);
    });

    Ok(run_id)
}

/// Close out a run: legacy JSONL finish (best-effort), inbox item, SQLite
/// `routine_runs` finish (with `inbox_item_id` back-link), and the refresh
/// event. Every path through here terminates the SQLite run — this is the
/// single choke point that prevents `running` rows from wedging.
fn finalize_run<R: tauri::Runtime>(
    deps: &RoutineRunDeps,
    app: &tauri::AppHandle<R>,
    ctx: RunFinishContext,
    outcome: RunOutcome,
) {
    let finished_ms = chrono::Utc::now().timestamp_millis();
    let duration_secs = (finished_ms - ctx.started_ms).max(0) / 1000;
    let status = if outcome.failed {
        "failed"
    } else {
        "succeeded"
    };
    let run_error = outcome
        .error
        .as_deref()
        .map(|e| truncate_chars(e, SUMMARY_MAX_CHARS));

    // 1. Legacy JSONL history (best-effort — never blocks the inbox).
    let jsonl_status = if outcome.failed {
        RunStatus::Failed
    } else {
        RunStatus::Succeeded
    };
    if let Err(e) = deps
        .runs_store
        .record_finish(&ctx.run_id, jsonl_status, run_error.clone())
    {
        tracing::warn!(
            run_id = %ctx.run_id,
            error = %e,
            "inbox: legacy JSONL run mirror finish failed; SQLite run record is authoritative"
        );
    }

    // 2. Inbox item + 3. SQLite run back-link.
    let summary = build_summary(
        ctx.note.as_deref(),
        duration_secs,
        &outcome.output,
        run_error.is_some(),
    );
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
    if let Err(e) = deps
        .inbox
        .record_run_finish(&ctx.run_id, status, run_error.as_deref(), item_id)
    {
        tracing::warn!(run_id = %ctx.run_id, error = %e, "inbox: failed to finish run record");
    }

    // 4. Refresh signal.
    let _ = app.emit(event_names::INBOX_UPDATED, ctx.run_id);
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

    use chrono::TimeZone as _;

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
        }
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
            },
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
            },
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

        let outcome = run_with_panic_guard(async { panicking_engine_outcome() }).await;

        std::panic::set_hook(prev_hook);

        assert!(outcome.failed, "panic must map to a failed outcome");
        assert!(
            outcome.error.unwrap().contains("panicked"),
            "error should mention the panic"
        );
        assert!(outcome.session_id.is_none(), "no session was opened");
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
        assert_eq!(exec.cost_usd, None, "never tracked in routine_runs");
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
            .record_run_finish(&run_id, "succeeded", None, None)
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
            .record_run_finish(&run_id, "failed", Some("boom"), None)
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
        // A `running` ghost (trigger_task_now placeholder / crashed drain)
        // must be skipped — see backfill_runs_from_jsonl docs. The JSONL
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
}
