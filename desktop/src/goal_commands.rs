//! P0-2 — Desktop goal runner: Tauri commands + unattended turn loop.
//!
//! Productizes the TUI `/goal` loop for the desktop: `start_goal_run`
//! injects a [`shannon_core::query_engine::GoalSpec`] into the session's
//! engine, registers the `goal_get` / `goal_update` tools against the live
//! runner state, and drives `engine.process_query` turn after turn in the
//! background — emitting the same `query:*` streaming events as
//! `send_message`, so the chat page shows the run live. After every turn
//! the pure decision in [`shannon_core::goal_loop`] (shared verbatim with
//! the TUI) decides: continue, or finish.
//!
//! Architecture mirrors the P0-3 [`crate::inbox_commands::spawn_routine_run`]
//! executor, including its final-state discipline:
//!
//! - a single `finalize` choke point writes the terminal status (sidecar +
//!   DTO + `goal:updated` emit); every exit path (decision terminal, stop,
//!   engine failure, panic) flows through it, so a run can never be left
//!   non-terminal;
//! - the engine phase runs under a panic guard (nested `tokio::spawn` +
//!   `JoinHandle`); a panic maps to a paused run with the panic text as
//!   `lastError`;
//! - completed / blocked / paused outcomes write a `source="goal"` item to
//!   the shared SQLite `InboxStore`; `stopped` deliberately does not.
//!
//! Persistence stays inside the existing session sidecar
//! (`SessionSidecar.goal`, [`shannon_core::session_log::StoredGoal`]) — no
//! new storage. The in-memory registry owns the richer run fields; after an
//! app restart `list_goal_runs` reconciles: any sidecar goal still
//! `active` (no live runner) is surfaced as an `interrupted` run the user
//! can resume.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use shannon_core::goal_loop::GoalMarker;
use shannon_core::inbox_store::{InboxItemNew, SOURCE_GOAL};
use shannon_core::query_engine::{GoalSpec, QueryContext, QueryEngine, QueryEvent, QueryMetadata};
use shannon_core::session_log::{SessionStore, StoredGoal};
use shannon_engine::api::client::LlmClient;
use shannon_engine::permissions::{ApprovalMode, PermissionManager, PermissionRuleChecker};
use shannon_engine::state::StateManager;
use shannon_tools::register_default_tools_with_providers;
use tauri::Emitter;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::commands::AppState;
use crate::config::DesktopConfig;
use crate::events::event_names;

// Guard-rail parity note: on `resume_goal_run` the continuation budget is
// re-armed (iterations, strike counters and spend reset) — mirrors the
// TUI's `/goal resume`.

// ── DTOs (frozen frontend contract — camelCase, do not reshape) ──────────

/// Run lifecycle surfaced to the Tasks page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalRunStatus {
    Running,
    Paused,
    Completed,
    Blocked,
    Stopped,
    Interrupted,
}

impl GoalRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            GoalRunStatus::Running => "running",
            GoalRunStatus::Paused => "paused",
            GoalRunStatus::Completed => "completed",
            GoalRunStatus::Blocked => "blocked",
            GoalRunStatus::Stopped => "stopped",
            GoalRunStatus::Interrupted => "interrupted",
        }
    }

    /// A run that still owns its session (manual sends are rejected).
    fn is_active(self) -> bool {
        matches!(self, GoalRunStatus::Running | GoalRunStatus::Paused)
    }
}

/// One goal run as rendered by the Tasks-page run card.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalRunDto {
    pub session_id: String,
    pub title: String,
    pub objective: String,
    /// `running | paused | completed | blocked | stopped | interrupted`
    pub status: String,
    pub iterations: u32,
    pub max_turns: Option<u32>,
    pub spent_usd: f64,
    pub budget_usd: Option<f64>,
    pub stall_strikes: u32,
    pub last_error: Option<String>,
    pub started_at_ms: i64,
    pub updated_at_ms: i64,
}

/// `start_goal_run` response — `{ sessionId }`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalRunStarted {
    pub session_id: String,
}

// ── Runner state ─────────────────────────────────────────────────────────

/// Mutable run state shared between the Tauri commands and the spawned
/// turn loop.
#[derive(Debug)]
pub(crate) struct GoalRunState {
    pub session_id: Uuid,
    pub title: String,
    pub objective: String,
    pub status: GoalRunStatus,
    pub iterations: u32,
    pub max_turns: Option<u32>,
    pub spent_usd: f64,
    pub budget_usd: Option<f64>,
    pub stall_strikes: u32,
    pub consecutive_no_tool_turns: u32,
    pub last_error: Option<String>,
    pub started_at_ms: i64,
    pub updated_at_ms: i64,
}

impl GoalRunState {
    fn dto(&self) -> GoalRunDto {
        GoalRunDto {
            session_id: self.session_id.to_string(),
            title: self.title.clone(),
            objective: self.objective.clone(),
            status: self.status.as_str().to_string(),
            iterations: self.iterations,
            max_turns: self.max_turns,
            spent_usd: self.spent_usd,
            budget_usd: self.budget_usd,
            stall_strikes: self.stall_strikes,
            last_error: self.last_error.clone(),
            started_at_ms: self.started_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }

    /// TUI-parity resume: `/goal resume` re-arms the budget — iterations
    /// and both guard counters reset, spend recomputed from zero.
    fn reset_for_resume(&mut self) {
        self.status = GoalRunStatus::Running;
        self.iterations = 0;
        self.consecutive_no_tool_turns = 0;
        self.stall_strikes = 0;
        self.spent_usd = 0.0;
        self.updated_at_ms = now_ms();
    }
}

/// Handle owned by the registry; the turn loop and the control commands
/// coordinate through it.
pub(crate) struct GoalRunHandle {
    pub(crate) state: tokio::sync::Mutex<GoalRunState>,
    /// Stop: cancels the in-flight turn and terminates the loop.
    pub(crate) cancel: CancellationToken,
    /// Resume signal for a parked (paused) loop.
    pub(crate) resume_notify: tokio::sync::Notify,
}

impl GoalRunHandle {
    async fn dto(&self) -> GoalRunDto {
        self.state.lock().await.dto()
    }

    async fn status(&self) -> GoalRunStatus {
        self.state.lock().await.status
    }

    /// `pause_goal_run`: takes effect at the next turn boundary (mirrors
    /// the TUI, where a paused goal simply stops re-queuing continuations;
    /// the in-flight turn completes so its cost/counters stay honest).
    pub(crate) async fn pause(&self) -> Result<(), String> {
        let mut s = self.state.lock().await;
        if s.status != GoalRunStatus::Running {
            return Err(format!(
                "goal run is {} (only running runs can pause)",
                s.status.as_str()
            ));
        }
        s.status = GoalRunStatus::Paused;
        s.updated_at_ms = now_ms();
        Ok(())
    }

    /// `resume_goal_run`: wakes a parked loop and re-arms the budget.
    pub(crate) async fn resume(&self) -> Result<(), String> {
        {
            let mut s = self.state.lock().await;
            if s.status != GoalRunStatus::Paused {
                return Err(format!(
                    "goal run is {} (only paused runs can resume)",
                    s.status.as_str()
                ));
            }
            s.reset_for_resume();
        }
        self.resume_notify.notify_waiters();
        Ok(())
    }

    /// `stop_goal_run`: cancels the current turn; the loop finalizes the
    /// run as `stopped` (no inbox item).
    pub(crate) fn stop(&self) {
        self.cancel.cancel();
    }
}

/// Per-session runner registry. The mutex is `std` on purpose: guards are
/// never held across `.await` and the Tauri command + `send_message` paths
/// only need short synchronous lookups.
#[derive(Default)]
pub(crate) struct GoalRunRegistry {
    runs: Mutex<HashMap<Uuid, Arc<GoalRunHandle>>>,
}

impl GoalRunRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a runner for `session_id`. Fails while another *active*
    /// (running/paused) runner exists for the same session — one goal loop
    /// per session (brief: 互斥). Terminal handles don't block a restart.
    fn start(&self, session_id: Uuid, handle: Arc<GoalRunHandle>) -> Result<(), String> {
        let mut runs = self.runs.lock().expect("goal registry poisoned");
        if let Some(existing) = runs.get(&session_id) {
            if existing.status_is_active_sync() {
                return Err("a goal run is already active on this session".into());
            }
        }
        runs.insert(session_id, handle);
        Ok(())
    }

    pub(crate) fn get(&self, session_id: &Uuid) -> Option<Arc<GoalRunHandle>> {
        self.runs
            .lock()
            .expect("goal registry poisoned")
            .get(session_id)
            .cloned()
    }

    pub(crate) fn list(&self) -> Vec<Arc<GoalRunHandle>> {
        self.runs
            .lock()
            .expect("goal registry poisoned")
            .values()
            .cloned()
            .collect()
    }

    /// True while an active (running/paused) runner owns the session — the
    /// `send_message` guard and the frontend composer gate both consult
    /// this (directly or via the goal DTO).
    pub(crate) fn blocks_session(&self, session_id: &Uuid) -> bool {
        self.get(session_id)
            .map(|h| h.status_is_active_sync())
            .unwrap_or(false)
    }
}

impl GoalRunHandle {
    /// Non-async status check for sync contexts (`GoalRunRegistry::blocks_session`).
    pub(crate) fn status_is_active_sync(&self) -> bool {
        self.status_sync().is_active()
    }

    fn status_sync(&self) -> GoalRunStatus {
        // The tokio mutex can't be locked synchronously; use try_lock with a
        // blocking fallback. Commands never hold it across awaits while also
        // calling this, so try_lock succeeds in practice; on contention
        // conservatively report active (fail closed on the send guard).
        match self.state.try_lock() {
            Ok(s) => s.status,
            Err(_) => GoalRunStatus::Running,
        }
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// ── State slices the loop needs (mirrors RoutineRunDeps) ─────────────────

#[derive(Clone)]
pub(crate) struct GoalRunDeps {
    pub(crate) inbox: Arc<shannon_core::inbox_store::InboxStore>,
    pub(crate) usage_store: Arc<crate::commands_usage::UsageStore>,
    pub(crate) client_config: Arc<RwLock<shannon_engine::api::types::LlmClientConfig>>,
    pub(crate) desktop_config: Arc<RwLock<DesktopConfig>>,
    /// Session container (`~/.shannon/sessions`) for sidecar persistence.
    pub(crate) sessions_dir: PathBuf,
}

impl GoalRunDeps {
    pub(crate) fn from_state(state: &AppState) -> Self {
        Self {
            inbox: state.inbox_store(),
            usage_store: state.usage_store.clone(),
            client_config: state.client_config.clone(),
            desktop_config: state.desktop_config.clone(),
            sessions_dir: state.state_manager.sessions_dir().to_path_buf(),
        }
    }

    fn session_store(&self) -> SessionStore {
        SessionStore::new(self.sessions_dir.clone())
    }
}

// ── Tauri commands (frozen contract) ─────────────────────────────────────

/// Start an unattended goal run on `session_id` (created when omitted).
#[tauri::command]
pub async fn start_goal_run(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    session_id: Option<String>,
    title: String,
    objective: String,
    max_turns: Option<u32>,
    budget_usd: Option<f64>,
) -> Result<GoalRunStarted, String> {
    let objective = objective.trim().to_string();
    if objective.is_empty() {
        return Err("goal objective must not be empty".into());
    }
    let title = {
        let t = title.trim();
        if t.is_empty() {
            objective.chars().take(50).collect::<String>()
        } else {
            t.to_string()
        }
    };

    // Resolve / create the session.
    let (session_uuid, created_session) = match session_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(raw) => (
            Uuid::parse_str(raw).map_err(|e| format!("invalid sessionId: {e}"))?,
            false,
        ),
        None => (
            create_goal_session(&state, &app_handle, &title).await?,
            true,
        ),
    };

    let deps = GoalRunDeps::from_state(&state);
    let started = now_ms();
    let run_state = GoalRunState {
        session_id: session_uuid,
        title: title.clone(),
        objective: objective.clone(),
        status: GoalRunStatus::Running,
        iterations: 0,
        max_turns,
        spent_usd: 0.0,
        budget_usd,
        stall_strikes: 0,
        consecutive_no_tool_turns: 0,
        last_error: None,
        started_at_ms: started,
        updated_at_ms: started,
    };
    let handle = Arc::new(GoalRunHandle {
        state: tokio::sync::Mutex::new(run_state),
        cancel: CancellationToken::new(),
        resume_notify: tokio::sync::Notify::new(),
    });
    state
        .goal_runs
        .start(session_uuid, handle.clone())
        .map_err(|e| format!("{e} (stop or remove the existing run first)"))?;

    // Persist the goal anchor immediately: a crash mid-run leaves an
    // `active` sidecar goal that the next launch reconciles to `interrupted`.
    persist_sidecar_goal(
        &deps,
        session_uuid,
        Some(StoredGoal {
            objective: objective.clone(),
            status: "active".into(),
            iterations: 0,
            max_iterations: max_turns.unwrap_or(0) as usize,
        }),
        created_session.then_some(title.as_str()),
    );

    let dto = handle.dto().await;
    let _ = app_handle.emit(event_names::GOAL_UPDATED, dto);

    let task_deps = deps.clone();
    tokio::spawn(run_goal_loop(task_deps, app_handle, handle));

    Ok(GoalRunStarted {
        session_id: session_uuid.to_string(),
    })
}

/// All runs: live registry first, then interrupted reconciliation from the
/// session sidecars.
#[tauri::command]
pub async fn list_goal_runs(state: tauri::State<'_, AppState>) -> Result<Vec<GoalRunDto>, String> {
    let deps = GoalRunDeps::from_state(&state);
    let mut runs = Vec::new();
    for handle in state.goal_runs.list() {
        runs.push(handle.dto().await);
    }
    for card in reconcile_interrupted_runs(&deps, &state.goal_runs) {
        runs.push(card);
    }
    runs.sort_by(|a, b| b.started_at_ms.cmp(&a.started_at_ms));
    Ok(runs)
}

/// One run by session id (live or interrupted).
#[tauri::command]
pub async fn get_goal_run(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<Option<GoalRunDto>, String> {
    let uuid = Uuid::parse_str(session_id.trim()).map_err(|e| format!("invalid sessionId: {e}"))?;
    if let Some(handle) = state.goal_runs.get(&uuid) {
        return Ok(Some(handle.dto().await));
    }
    let deps = GoalRunDeps::from_state(&state);
    Ok(reconcile_interrupted_runs(&deps, &state.goal_runs)
        .into_iter()
        .find(|c| c.session_id == uuid.to_string()))
}

/// Terminal `stopped`: cancels the current turn; no inbox item is written.
#[tauri::command]
pub async fn stop_goal_run(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    session_id: String,
) -> Result<(), String> {
    let uuid = Uuid::parse_str(session_id.trim()).map_err(|e| format!("invalid sessionId: {e}"))?;
    if let Some(handle) = state.goal_runs.get(&uuid) {
        handle.stop();
        return Ok(());
    }
    // Interrupted card (no live runner): stopping = clearing the active
    // sidecar anchor so it stops resurfacing as interrupted.
    let deps = GoalRunDeps::from_state(&state);
    let store = deps.session_store();
    let mut sidecar = store.sidecar(&uuid);
    if sidecar.goal.as_ref().map(|g| g.status.as_str()) == Some("active") {
        let title = sidecar.title.clone();
        let objective = sidecar
            .goal
            .as_ref()
            .map(|g| g.objective.clone())
            .unwrap_or_default();
        if let Some(goal) = sidecar.goal.as_mut() {
            goal.status = "paused".into();
        }
        store
            .save_sidecar_replace(&uuid, &sidecar)
            .map_err(|e| e.to_string())?;
        let mut dto = interrupted_dto(uuid, title.as_deref(), &objective, 0, 0, now_ms());
        dto.status = GoalRunStatus::Stopped.as_str().into();
        let _ = app_handle.emit(event_names::GOAL_UPDATED, dto);
    }
    Ok(())
}

/// Pause at the next turn boundary.
#[tauri::command]
pub async fn pause_goal_run(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    session_id: String,
) -> Result<(), String> {
    let uuid = Uuid::parse_str(session_id.trim()).map_err(|e| format!("invalid sessionId: {e}"))?;
    let handle = state
        .goal_runs
        .get(&uuid)
        .ok_or_else(|| "no live goal run for this session".to_string())?;
    handle.pause().await?;
    // Keep the sidecar anchor honest while parked.
    {
        let s = handle.state.lock().await;
        persist_sidecar_goal(
            &GoalRunDeps::from_state(&state),
            uuid,
            Some(StoredGoal {
                objective: s.objective.clone(),
                status: "paused".into(),
                iterations: usize::try_from(s.iterations).unwrap_or(usize::MAX),
                max_iterations: s.max_turns.unwrap_or(0) as usize,
            }),
            None,
        );
    }
    let dto = handle.dto().await;
    let _ = app_handle.emit(event_names::GOAL_UPDATED, dto);
    Ok(())
}

/// Resume a paused run: re-arms the budget (TUI `/goal resume` semantics).
#[tauri::command]
pub async fn resume_goal_run(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    session_id: String,
) -> Result<(), String> {
    let uuid = Uuid::parse_str(session_id.trim()).map_err(|e| format!("invalid sessionId: {e}"))?;
    let deps = GoalRunDeps::from_state(&state);
    if let Some(handle) = state.goal_runs.get(&uuid) {
        handle.resume().await?;
        // Sidecar anchor back to active while the loop runs again.
        let stored = {
            let s = handle.state.lock().await;
            StoredGoal {
                objective: s.objective.clone(),
                status: "active".into(),
                iterations: usize::try_from(s.iterations).unwrap_or(usize::MAX),
                max_iterations: s.max_turns.unwrap_or(0) as usize,
            }
        };
        persist_sidecar_goal(&deps, uuid, Some(stored), None);
        let dto = handle.dto().await;
        let _ = app_handle.emit(event_names::GOAL_UPDATED, dto);
        return Ok(());
    }
    // Interrupted reconciliation: resurrect the sidecar goal as a fresh run.
    let store = deps.session_store();
    let sidecar = store.sidecar(&uuid);
    let Some(goal) = sidecar.goal else {
        return Err("no goal found for this session".into());
    };
    if goal.status != "active" {
        return Err(format!(
            "session goal is '{}' (only active/interrupted runs can resume)",
            goal.status
        ));
    }
    let objective = goal.objective.clone();
    let max_turns = (goal.max_iterations > 0).then_some(goal.max_iterations as u32);
    drop(store);

    let started = now_ms();
    let run_state = GoalRunState {
        session_id: uuid,
        title: sidecar
            .title
            .clone()
            .unwrap_or_else(|| objective.chars().take(50).collect()),
        objective,
        status: GoalRunStatus::Running,
        iterations: 0,
        max_turns,
        spent_usd: 0.0,
        budget_usd: None,
        stall_strikes: 0,
        consecutive_no_tool_turns: 0,
        last_error: None,
        started_at_ms: started,
        updated_at_ms: started,
    };
    let handle = Arc::new(GoalRunHandle {
        state: tokio::sync::Mutex::new(run_state),
        cancel: CancellationToken::new(),
        resume_notify: tokio::sync::Notify::new(),
    });
    state
        .goal_runs
        .start(uuid, handle.clone())
        .map_err(|e| e.to_string())?;
    persist_sidecar_goal(
        &deps,
        uuid,
        Some(StoredGoal {
            objective: handle.state.lock().await.objective.clone(),
            status: "active".into(),
            iterations: 0,
            max_iterations: max_turns.unwrap_or(0) as usize,
        }),
        None,
    );
    let dto = handle.dto().await;
    let _ = app_handle.emit(event_names::GOAL_UPDATED, dto);
    let task_deps = deps.clone();
    tokio::spawn(run_goal_loop(task_deps, app_handle, handle));
    Ok(())
}

/// Rewrite the objective; a running loop picks it up on the next turn.
#[tauri::command]
pub async fn update_goal_objective(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    session_id: String,
    objective: String,
) -> Result<(), String> {
    let objective = objective.trim().to_string();
    if objective.is_empty() {
        return Err("goal objective must not be empty".into());
    }
    let uuid = Uuid::parse_str(session_id.trim()).map_err(|e| format!("invalid sessionId: {e}"))?;
    let deps = GoalRunDeps::from_state(&state);
    if let Some(handle) = state.goal_runs.get(&uuid) {
        {
            let mut s = handle.state.lock().await;
            if !s.status.is_active() {
                return Err(format!(
                    "goal run is {} (objective is final once terminal)",
                    s.status.as_str()
                ));
            }
            s.objective = objective.clone();
            s.updated_at_ms = now_ms();
        }
        let stored = {
            let s = handle.state.lock().await;
            StoredGoal {
                objective: s.objective.clone(),
                status: "active".into(),
                iterations: usize::try_from(s.iterations).unwrap_or(usize::MAX),
                max_iterations: s.max_turns.unwrap_or(0) as usize,
            }
        };
        persist_sidecar_goal(&deps, uuid, Some(stored), None);
        let dto = handle.dto().await;
        let _ = app_handle.emit(event_names::GOAL_UPDATED, dto);
        return Ok(());
    }
    // Interrupted card: rewrite the sidecar objective in place.
    let store = deps.session_store();
    let mut sidecar = store.sidecar(&uuid);
    match sidecar.goal.as_mut() {
        Some(goal) if goal.status == "active" => {
            goal.objective = objective;
            persist_full_sidecar(&store, uuid, sidecar);
            Ok(())
        }
        Some(goal) => Err(format!("session goal is '{}' (not editable)", goal.status)),
        None => Err("no goal found for this session".into()),
    }
}

// ── Session creation (new-session branch of start_goal_run) ─────────────

/// Create a fresh L0 session for a goal run, mirroring `new_session` but
/// without stealing the user's active session.
async fn create_goal_session(
    state: &tauri::State<'_, AppState>,
    app_handle: &tauri::AppHandle,
    title: &str,
) -> Result<Uuid, String> {
    let id = Uuid::new_v4();
    let id_str = id.to_string();
    let model = state.client_config.read().await.model.clone();
    shannon_core::session_log::SessionTee::open_in_container(
        state.l0_store().container(),
        &id_str,
        &model,
        None,
    )
    .close();

    let now = crate::commands::chrono_timestamp();
    state
        .sessions
        .lock()
        .await
        .push(crate::commands::SessionMeta {
            id: id_str.clone(),
            title: title.to_string(),
            created_at: now,
            message_count: 0,
            working_dir: None,
            parent_id: None,
            branch_point: None,
        });
    state.registry.insert(id);

    let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());
    Ok(id)
}

// ── Sidecar persistence ──────────────────────────────────────────────────

/// Read-modify-write the sidecar's goal row (mirrors the TUI's
/// `save_goal_sidecar`: load full sidecar, replace `goal`, save unmerged so
/// an explicit write is authoritative). `title` additionally backfills the
/// sidecar title (used when the runner created the session).
fn persist_sidecar_goal(
    deps: &GoalRunDeps,
    session_id: Uuid,
    goal: Option<StoredGoal>,
    title: Option<&str>,
) {
    let store = deps.session_store();
    let mut sidecar = store.sidecar(&session_id);
    if let Some(t) = title {
        sidecar.title = Some(t.to_string());
    }
    sidecar.goal = goal;
    if let Err(e) = store.save_sidecar_replace(&session_id, &sidecar) {
        tracing::warn!(session = %session_id, error = %e, "goal: sidecar save failed");
    }
}

/// Variant used when the caller already holds a loaded sidecar.
fn persist_full_sidecar(
    store: &SessionStore,
    session_id: Uuid,
    sidecar: shannon_core::session_log::SessionSidecar,
) {
    if let Err(e) = store.save_sidecar_replace(&session_id, &sidecar) {
        tracing::warn!(session = %session_id, error = %e, "goal: sidecar save failed");
    }
}

// ── Restart reconciliation ───────────────────────────────────────────────

/// Scan the session container for sidecar goals still marked `active` with
/// no live runner: the app restarted (or crashed) under them, so they are
/// surfaced as `interrupted` runs the user can resume or stop.
fn reconcile_interrupted_runs(deps: &GoalRunDeps, registry: &GoalRunRegistry) -> Vec<GoalRunDto> {
    let store = deps.session_store();
    let entries = match std::fs::read_dir(&deps.sessions_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut cards = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(session_id) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| Uuid::parse_str(n).ok())
        else {
            continue;
        };
        if registry.get(&session_id).is_some() {
            continue; // live runner already covers this session
        }
        let sidecar = store.sidecar(&session_id);
        let Some(goal) = sidecar.goal else { continue };
        if goal.status != "active" {
            continue;
        }
        let mtime_ms = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or_else(now_ms);
        cards.push(interrupted_dto(
            session_id,
            sidecar.title.as_deref(),
            &goal.objective,
            goal.iterations,
            goal.max_iterations,
            mtime_ms,
        ));
    }
    cards
}

fn interrupted_dto(
    session_id: Uuid,
    title: Option<&str>,
    objective: &str,
    iterations: usize,
    max_iterations: usize,
    at_ms: i64,
) -> GoalRunDto {
    GoalRunDto {
        session_id: session_id.to_string(),
        title: title
            .map(str::to_string)
            .unwrap_or_else(|| objective.chars().take(50).collect()),
        objective: objective.to_string(),
        status: GoalRunStatus::Interrupted.as_str().to_string(),
        iterations: iterations as u32,
        max_turns: (max_iterations > 0).then_some(max_iterations as u32),
        // Spend and budget are runner-memory state; they are not part of
        // StoredGoal, so an interrupted card honestly reports zero/unknown.
        spent_usd: 0.0,
        budget_usd: None,
        stall_strikes: 0,
        last_error: None,
        started_at_ms: at_ms,
        updated_at_ms: at_ms,
    }
}

// ── goal_get / goal_update bridge ────────────────────────────────────────

/// State the goal tools read/write mid-turn, shared with the turn loop via
/// a `std::sync::Mutex` (tool calls are sync, inside the agent loop).
#[derive(Default)]
struct GoalSharedCore {
    objective: String,
    status: String, // "active" | "paused" | "complete"
    iterations: u32,
    max_iterations: u32,
    max_budget_usd: Option<f64>,
    /// Set by `goal_update` tool calls; consumed by the loop post-turn.
    tool_completed: bool,
    tool_blocked: Option<String>,
}

struct RunnerGoalAccess {
    core: Arc<Mutex<GoalSharedCore>>,
}

impl shannon_tools::goal::GoalStateAccess for RunnerGoalAccess {
    fn snapshot(&self) -> Option<shannon_tools::goal::GoalSnapshot> {
        let core = self.core.lock().expect("goal shared core poisoned");
        Some(shannon_tools::goal::GoalSnapshot {
            objective: core.objective.clone(),
            status: core.status.clone(),
            iterations: core.iterations as usize,
            max_iterations: core.max_iterations as usize,
            max_budget_usd: core.max_budget_usd,
        })
    }

    fn apply_update(&self, outcome: shannon_tools::goal::GoalUpdateOutcome) -> Option<()> {
        let mut core = self.core.lock().expect("goal shared core poisoned");
        match outcome {
            shannon_tools::goal::GoalUpdateOutcome::Completed => {
                core.status = "complete".into();
                core.tool_completed = true;
            }
            shannon_tools::goal::GoalUpdateOutcome::Paused(reason) => {
                core.status = "paused".into();
                core.tool_blocked = Some(reason);
            }
            shannon_tools::goal::GoalUpdateOutcome::Rejected(_) => {}
        }
        Some(())
    }
}

// ── The turn loop ────────────────────────────────────────────────────────

/// What one engine turn produced.
struct TurnObservation {
    /// Full assistant text of the turn (concatenated text events).
    assistant_text: String,
    had_tool_calls: bool,
    cost_usd: f64,
    failure: Option<String>,
    cancelled: bool,
}

/// Terminal outcomes the loop hands to [`finalize_goal_run`].
pub(crate) enum GoalTerminal {
    Completed,
    Blocked(String),
    /// Recoverable pause: max reached, budget capped, anti-spin/stall, or
    /// an engine failure (`reason`/`last_error` carry the details).
    Paused {
        reason: Option<String>,
    },
    Stopped,
}

/// Drive the goal run to a terminal state. Owns one engine for the whole
/// run (`set_goal` once, `set_goal(None)` at the end) and streams every
/// turn's `QueryEvent`s to the Tauri wire so the chat page renders the run.
///
/// Final-state discipline: every exit — decision terminal, stop, engine
/// failure, panic — flows through [`finalize_goal_run`]; the run can never
/// be left `running`. Generic over the Tauri runtime so tests can drive it
/// with `mock_app`.
async fn run_goal_loop<R: tauri::Runtime>(
    deps: GoalRunDeps,
    app: tauri::AppHandle<R>,
    handle: Arc<GoalRunHandle>,
) {
    let engine_future = drive_goal_turns(deps.clone(), app.clone(), handle.clone());
    let terminal = match tokio::spawn(engine_future).await {
        Ok(terminal) => terminal,
        Err(join_error) => GoalTerminal::Paused {
            reason: Some(format!("goal task panicked: {join_error}")),
        },
    };
    finalize_goal_run(&deps, &app, &handle, terminal).await;
}

/// The engine phase, split out so `run_goal_loop` can wrap it in the panic
/// guard. Returns the terminal verdict (or keeps running until stop).
async fn drive_goal_turns<R: tauri::Runtime>(
    deps: GoalRunDeps,
    app: tauri::AppHandle<R>,
    handle: Arc<GoalRunHandle>,
) -> GoalTerminal {
    // Engine + config (mirrors spawn_routine_run: unattended → configured
    // approval mode honoured, persisted rules applied, FullAuto default).
    let client_config = deps.client_config.read().await.clone();
    let approval_mode_str = deps.desktop_config.read().await.approval_mode.clone();
    let model = client_config.model.clone();
    let model_for_usage = model.clone();
    let provider = client_config.provider.to_string();

    let mut permissions = PermissionManager::new();
    let mode = approval_mode_str
        .as_deref()
        .and_then(|s| match s {
            "full_auto" => Some(ApprovalMode::FullAuto),
            "auto_edit" => Some(ApprovalMode::AutoEdit),
            "auto" => Some(ApprovalMode::Auto),
            "plan" => Some(ApprovalMode::Plan),
            _ => None,
        })
        .unwrap_or(ApprovalMode::FullAuto);
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

    // Per-run tool registry: default tools + goal_get/goal_update bound to
    // this run's shared core (never the global registry — goal tools must
    // not leak into other sessions).
    let (session_uuid, objective, max_turns, budget_usd) = {
        let s = handle.state.lock().await;
        (s.session_id, s.objective.clone(), s.max_turns, s.budget_usd)
    };
    let shared = Arc::new(Mutex::new(GoalSharedCore {
        objective: objective.clone(),
        status: "active".into(),
        iterations: 0,
        max_iterations: max_turns.unwrap_or(0),
        max_budget_usd: budget_usd,
        tool_completed: false,
        tool_blocked: None,
    }));
    let mut tools = shannon_core::tools::ToolRegistry::new();
    let assembly = shannon_remote::assembly::assemble_dynamic();
    if let Err(e) = register_default_tools_with_providers(&mut tools, &assembly.providers) {
        return GoalTerminal::Paused {
            reason: Some(format!("goal tool registry init failed: {e}")),
        };
    }
    if let Err(e) = shannon_tools::goal::register_goal_tools(
        &mut tools,
        Arc::new(RunnerGoalAccess {
            core: shared.clone(),
        }),
    ) {
        return GoalTerminal::Paused {
            reason: Some(format!("registering goal tools failed: {e}")),
        };
    }

    let mut engine = QueryEngine::with_defaults_arc(
        LlmClient::new(client_config),
        Arc::new(tools),
        permissions,
        StateManager::new(),
    );
    engine.set_session_id(session_uuid);
    match engine.restore_session(session_uuid) {
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(session = %session_uuid, error = %e, "goal: history restore failed")
        }
    }
    // Goal injection: non-cached system block on every turn (brief:
    // engine.set_goal(GoalSpec{objective, paused:false})).
    engine.set_goal(Some(GoalSpec {
        objective: objective.clone(),
        paused: false,
    }));

    // Track the runner's own copy of the counters for the decision input.
    let mut iterations: u32 = 0;
    let mut spent_usd: f64 = 0.0;
    let mut consecutive_no_tool_turns: u32 = 0;
    let mut stall_strikes: u32 = 0;
    // The next user message: the objective for turn 1, the exact TUI
    // continuation prompt for subsequent turns.
    let mut pending_prompt = Some(objective.clone());

    loop {
        // Stop wins over everything.
        if handle.cancel.is_cancelled() {
            engine.set_goal(None);
            return GoalTerminal::Stopped;
        }
        // Park while paused (pause takes effect at turn boundaries). The
        // status re-check inside the loop closes the wakeup race where
        // `resume` fires between the outer check and the select.
        let mut parked = false;
        while handle.status().await == GoalRunStatus::Paused {
            parked = true;
            tokio::select! {
                _ = handle.cancel.cancelled() => {
                    engine.set_goal(None);
                    return GoalTerminal::Stopped;
                }
                _ = handle.resume_notify.notified() => {}
            }
        }
        if handle.status().await != GoalRunStatus::Running {
            continue; // cancelled/something changed — re-check at the top
        }
        if parked {
            // Resume re-armed the budget (TUI parity): recompute the next
            // prompt from the reset counters.
            let s = handle.state.lock().await;
            iterations = s.iterations;
            spent_usd = s.spent_usd;
            consecutive_no_tool_turns = s.consecutive_no_tool_turns;
            stall_strikes = s.stall_strikes;
            pending_prompt = Some(shannon_core::goal_loop::continuation_prompt(
                iterations + 1,
                s.max_turns.unwrap_or(0),
                &s.objective,
            ));
            shared.lock().expect("poisoned").objective = s.objective.clone();
        }

        let Some(user_message) = pending_prompt.take() else {
            // Unreachable: pending_prompt is always set before the loop and
            // re-armed on every Continue/resume. Treat as a safety stop.
            engine.set_goal(None);
            return GoalTerminal::Paused {
                reason: Some("internal: continuation queue ran dry".into()),
            };
        };

        // Sync the tools' view + engine goal for this turn (objective edits
        // via `update_goal_objective` take effect here — next turn).
        {
            let objective_now = handle.state.lock().await.objective.clone();
            let mut core = shared.lock().expect("goal shared core poisoned");
            core.objective = objective_now.clone();
            core.iterations = iterations;
            core.tool_completed = false;
            core.tool_blocked = None;
            engine.set_goal(Some(GoalSpec {
                objective: objective_now,
                paused: false,
            }));
        }

        let query_id = Uuid::new_v4();
        let observation = run_single_turn(
            &mut engine,
            &app,
            &handle,
            query_id,
            session_uuid,
            user_message,
            model.clone(),
            &deps,
            &model_for_usage,
            &provider,
        )
        .await;

        // Update the loop counters from the observation.
        iterations += 1;
        spent_usd += observation.cost_usd;
        if observation.had_tool_calls {
            consecutive_no_tool_turns = 0;
            stall_strikes = stall_strikes.saturating_sub(1);
        } else {
            consecutive_no_tool_turns += 1;
            stall_strikes += 1;
        }
        {
            let mut s = handle.state.lock().await;
            s.iterations = iterations;
            s.spent_usd = spent_usd;
            s.consecutive_no_tool_turns = consecutive_no_tool_turns;
            s.stall_strikes = stall_strikes;
            s.updated_at_ms = now_ms();
            if let Some(err) = &observation.failure {
                s.last_error = Some(err.clone());
            }
        }
        persist_turn_progress(&deps, &handle).await;

        // Cancelled mid-turn → stopped (no inbox, no decision).
        if observation.cancelled {
            engine.set_goal(None);
            return GoalTerminal::Stopped;
        }

        // Engine failure → recoverable pause with the error preserved.
        if let Some(err) = observation.failure {
            engine.set_goal(None);
            return GoalTerminal::Paused { reason: Some(err) };
        }

        // Structured tool verdicts (goal_update) beat the marker scan —
        // they are explicit model statements made mid-turn.
        {
            let core = shared.lock().expect("goal shared core poisoned");
            if core.tool_completed {
                engine.set_goal(None);
                return GoalTerminal::Completed;
            }
            if let Some(reason) = core.tool_blocked.clone() {
                engine.set_goal(None);
                return GoalTerminal::Blocked(reason);
            }
        }

        // Marker on the final non-empty line (TUI contract).
        let marker = observation
            .assistant_text
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .and_then(shannon_core::goal_loop::detect_goal_marker);
        match marker {
            Some(GoalMarker::Complete) => {
                engine.set_goal(None);
                return GoalTerminal::Completed;
            }
            Some(GoalMarker::Blocked(reason)) => {
                engine.set_goal(None);
                return GoalTerminal::Blocked(reason);
            }
            None => {}
        }

        // Pure decision (shared with the TUI).
        let input = shannon_core::goal_loop::GoalDecisionInput {
            objective: handle.state.lock().await.objective.clone(),
            iterations,
            max_iterations: max_turns.unwrap_or(0),
            consecutive_no_tool_turns,
            stall_strikes,
            max_budget_usd: budget_usd,
            spent_usd,
            had_tool_calls: observation.had_tool_calls,
        };
        match shannon_core::goal_loop::decide_goal_continuation(&input, None) {
            shannon_core::goal_loop::GoalContinuation::Continue {
                iterations: next,
                prompt,
            } => {
                iterations = next;
                // Re-derive the guard counters exactly like the TUI's
                // check_goal_continuation Continue arm.
                if observation.had_tool_calls {
                    consecutive_no_tool_turns = 0;
                    stall_strikes = stall_strikes.saturating_sub(1);
                } else {
                    consecutive_no_tool_turns += 1;
                    stall_strikes += 1;
                }
                pending_prompt = Some(prompt);
            }
            shannon_core::goal_loop::GoalContinuation::MaxReached => {
                engine.set_goal(None);
                return GoalTerminal::Paused { reason: None };
            }
            shannon_core::goal_loop::GoalContinuation::BudgetLimited(reason) => {
                engine.set_goal(None);
                return GoalTerminal::Paused {
                    reason: Some(reason),
                };
            }
            shannon_core::goal_loop::GoalContinuation::PausedNoProgress(reason) => {
                engine.set_goal(None);
                return GoalTerminal::Paused {
                    reason: Some(reason),
                };
            }
            // decide_goal_continuation never returns Inactive (see core docs).
            shannon_core::goal_loop::GoalContinuation::Inactive
            | shannon_core::goal_loop::GoalContinuation::Completed
            | shannon_core::goal_loop::GoalContinuation::Blocked(_) => {
                engine.set_goal(None);
                return GoalTerminal::Paused {
                    reason: Some("internal: unexpected decision verdict".into()),
                };
            }
        }
    }
}

/// Drive one engine turn to completion, streaming events to the wire.
#[allow(clippy::too_many_arguments)]
async fn run_single_turn<R: tauri::Runtime>(
    engine: &mut QueryEngine,
    app: &tauri::AppHandle<R>,
    handle: &Arc<GoalRunHandle>,
    query_id: Uuid,
    session_id: Uuid,
    user_message: String,
    model: String,
    deps: &GoalRunDeps,
    model_for_usage: &str,
    provider: &str,
) -> TurnObservation {
    let qid = query_id.to_string();
    let context = QueryContext {
        query_id,
        session_id,
        user_message,
        metadata: QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: true,
            max_tokens: None,
            model,
            temperature: None,
            top_p: None,
        },
    };

    let mut assistant_text = String::new();
    let mut had_tool_calls = false;
    let mut cost_usd = 0.0;
    let mut failure: Option<String> = None;
    let mut cancelled = false;
    let mut completed = false;

    let stream = engine.process_query(context, None).await;
    use futures::StreamExt;
    let mut pin_stream = std::pin::pin!(stream);
    while let Some(event_result) = pin_stream.next().await {
        if handle.cancel.is_cancelled() {
            cancelled = true;
            let _ = app.emit(
                event_names::QUERY_CANCELLED,
                crate::events::QueryCancelledPayload {
                    query_id: qid.clone(),
                },
            );
            break;
        }
        match event_result {
            Ok(event) => match event {
                QueryEvent::Text { content, .. } => {
                    assistant_text.push_str(&content);
                    let _ = app.emit(
                        event_names::QUERY_TEXT,
                        crate::events::QueryTextPayload {
                            query_id: qid.clone(),
                            content,
                        },
                    );
                }
                QueryEvent::ToolUseRequest {
                    tool_use_id,
                    tool_name,
                    tool_input,
                    ..
                } => {
                    had_tool_calls = true;
                    let _ = app.emit(
                        event_names::QUERY_TOOL_START,
                        crate::events::ToolStartPayload {
                            query_id: qid.clone(),
                            tool_use_id,
                            tool_name,
                            tool_input,
                        },
                    );
                }
                QueryEvent::ToolUseResult {
                    tool_use_id,
                    tool_name,
                    result,
                    is_error,
                    ..
                } => {
                    let _ = app.emit(
                        event_names::QUERY_TOOL_RESULT,
                        crate::events::ToolResultPayload {
                            query_id: qid.clone(),
                            tool_use_id,
                            tool_name,
                            result,
                            is_error,
                        },
                    );
                }
                QueryEvent::ToolProgress {
                    tool_use_id,
                    tool_name,
                    progress,
                    message,
                    ..
                } => {
                    let _ = app.emit(
                        event_names::QUERY_TOOL_PROGRESS,
                        crate::events::ToolProgressPayload {
                            query_id: qid.clone(),
                            tool_use_id,
                            tool_name,
                            progress,
                            message,
                        },
                    );
                }
                QueryEvent::Thinking { content, .. } => {
                    let _ = app.emit(
                        event_names::QUERY_THINKING,
                        crate::events::ThinkingPayload {
                            query_id: qid.clone(),
                            content,
                        },
                    );
                }
                QueryEvent::Usage {
                    input_tokens,
                    output_tokens,
                    cost_usd: event_cost,
                    cache_creation_tokens,
                    cache_read_tokens,
                    ..
                } => {
                    cost_usd += event_cost;
                    // Best-effort ledger write, mirroring send_message.
                    let _ = deps
                        .usage_store
                        .append(&crate::commands_usage::record_event(
                            model_for_usage,
                            provider,
                            crate::commands_usage::UsageTotals {
                                input_tokens,
                                output_tokens,
                                cache_creation_tokens,
                                cache_read_tokens,
                                cost_usd: event_cost,
                            },
                            Some(&session_id.to_string()),
                        ));
                    let _ = app.emit(
                        event_names::QUERY_USAGE,
                        crate::events::UsagePayload {
                            query_id: qid.clone(),
                            input_tokens,
                            output_tokens,
                            cost_usd: event_cost,
                        },
                    );
                }
                QueryEvent::Completed { .. } => {
                    completed = true;
                    break;
                }
                QueryEvent::Failed { error, .. } => {
                    let _ = app.emit(
                        event_names::QUERY_FAILED,
                        crate::events::QueryFailedPayload {
                            query_id: qid.clone(),
                            error: error.clone(),
                        },
                    );
                    failure = Some(error);
                    break;
                }
                _ => {}
            },
            Err(e) => {
                let err = e.to_string();
                let _ = app.emit(
                    event_names::QUERY_FAILED,
                    crate::events::QueryFailedPayload {
                        query_id: qid.clone(),
                        error: err.clone(),
                    },
                );
                failure = Some(err);
                break;
            }
        }
    }
    if completed && !cancelled {
        let _ = app.emit(
            event_names::QUERY_COMPLETED,
            crate::events::QueryCompletedPayload {
                query_id: qid.clone(),
            },
        );
    }
    TurnObservation {
        assistant_text,
        had_tool_calls,
        cost_usd,
        failure,
        cancelled,
    }
}

/// Best-effort per-turn sidecar refresh (iterations/status) so a crash
/// never loses more than one turn of progress.
async fn persist_turn_progress(deps: &GoalRunDeps, handle: &Arc<GoalRunHandle>) {
    let s = handle.state.lock().await;
    let status = match s.status {
        GoalRunStatus::Running => "active",
        GoalRunStatus::Paused => "paused",
        GoalRunStatus::Completed => "complete",
        _ => "paused",
    };
    persist_sidecar_goal(
        deps,
        s.session_id,
        Some(StoredGoal {
            objective: s.objective.clone(),
            status: status.into(),
            iterations: usize::try_from(s.iterations).unwrap_or(usize::MAX),
            max_iterations: s.max_turns.unwrap_or(0) as usize,
        }),
        None,
    );
}

// ── Finalize (single choke point — mirrors finalize_run) ────────────────

/// Close out a run: terminal status, sidecar row, inbox item (except for
/// `stopped`), and the `goal:updated` emit. Every path reaches this.
async fn finalize_goal_run<R: tauri::Runtime>(
    deps: &GoalRunDeps,
    app: &tauri::AppHandle<R>,
    handle: &Arc<GoalRunHandle>,
    terminal: GoalTerminal,
) {
    let (status, reason) = match terminal {
        GoalTerminal::Completed => (GoalRunStatus::Completed, None),
        GoalTerminal::Blocked(reason) => (GoalRunStatus::Blocked, Some(reason)),
        GoalTerminal::Paused { reason } => (GoalRunStatus::Paused, reason),
        GoalTerminal::Stopped => (GoalRunStatus::Stopped, None),
    };
    let (dto, session_uuid) = {
        let mut s = handle.state.lock().await;
        s.status = status;
        if let Some(r) = &reason {
            s.last_error = Some(r.clone());
        }
        s.updated_at_ms = now_ms();
        (s.dto(), s.session_id)
    };

    // Sidecar: completed → "complete"; every other terminal → "paused"
    // (recoverable, keeps the objective visible in the session, never
    // auto-continues).
    let sidecar_status = match status {
        GoalRunStatus::Completed => "complete",
        _ => "paused",
    };
    persist_sidecar_goal(
        deps,
        session_uuid,
        Some(StoredGoal {
            objective: dto.objective.clone(),
            status: sidecar_status.into(),
            iterations: usize::try_from(dto.iterations).unwrap_or(usize::MAX),
            max_iterations: dto.max_turns.unwrap_or(0) as usize,
        }),
        None,
    );

    // Inbox item for completed / blocked / paused outcomes (brief: stopped
    // writes nothing).
    if matches!(
        status,
        GoalRunStatus::Completed | GoalRunStatus::Blocked | GoalRunStatus::Paused
    ) {
        let summary = match status {
            GoalRunStatus::Completed => format!(
                "Completed after {} turn{} · spent ${:.4}",
                dto.iterations,
                if dto.iterations == 1 { "" } else { "s" },
                dto.spent_usd
            ),
            GoalRunStatus::Blocked => format!(
                "Blocked after {} turns · spent ${:.4}",
                dto.iterations, dto.spent_usd
            ),
            _ => format!(
                "Paused after {} turns · spent ${:.4}",
                dto.iterations, dto.spent_usd
            ),
        };
        let item = deps.inbox.append_item(InboxItemNew {
            source: SOURCE_GOAL.into(),
            source_id: Some(dto.session_id.clone()),
            session_id: Some(dto.session_id.clone()),
            title: dto.title.clone(),
            summary: truncate_chars(&summary, 500),
            error: reason.clone().map(|r| truncate_chars(&r, 500)),
        });
        if let Err(e) = item {
            tracing::warn!(session = %dto.session_id, error = %e, "goal: failed to append inbox item");
        }
        let _ = app.emit(event_names::INBOX_UPDATED, dto.session_id.clone());
    }

    let _ = app.emit(event_names::GOAL_UPDATED, dto);
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn temp_deps(dir: &std::path::Path) -> GoalRunDeps {
        GoalRunDeps {
            inbox: Arc::new(
                shannon_core::inbox_store::InboxStore::open_with_legacy(
                    &dir.join("inbox.db"),
                    None,
                )
                .unwrap(),
            ),
            usage_store: Arc::new(crate::commands_usage::UsageStore::with_path(
                dir.join("usage.jsonl"),
            )),
            client_config: Arc::new(RwLock::new(
                shannon_engine::api::types::LlmClientConfig::default(),
            )),
            desktop_config: Arc::new(RwLock::new(DesktopConfig::default())),
            sessions_dir: dir.join("sessions"),
        }
    }

    fn handle_for(session: Uuid, status: GoalRunStatus) -> Arc<GoalRunHandle> {
        Arc::new(GoalRunHandle {
            state: tokio::sync::Mutex::new(GoalRunState {
                session_id: session,
                title: "Ship the thing".into(),
                objective: "make CI green".into(),
                status,
                iterations: 3,
                max_turns: Some(10),
                spent_usd: 0.75,
                budget_usd: Some(5.0),
                stall_strikes: 1,
                consecutive_no_tool_turns: 0,
                last_error: None,
                started_at_ms: 1_000,
                updated_at_ms: 2_000,
            }),
            cancel: CancellationToken::new(),
            resume_notify: tokio::sync::Notify::new(),
        })
    }

    fn mock_app() -> tauri::AppHandle<tauri::test::MockRuntime> {
        tauri::test::mock_app().handle().clone()
    }

    // ── DTO contract ────────────────────────────────────────────────────

    #[test]
    fn dto_serializes_camel_case_frozen_shape() {
        let dto = handle_for(Uuid::new_v4(), GoalRunStatus::Running)
            .state
            .try_lock()
            .unwrap()
            .dto();
        let json = serde_json::to_value(&dto).unwrap();
        for key in [
            "sessionId",
            "title",
            "objective",
            "status",
            "iterations",
            "maxTurns",
            "spentUsd",
            "budgetUsd",
            "stallStrikes",
            "lastError",
            "startedAtMs",
            "updatedAtMs",
        ] {
            assert!(json.get(key).is_some(), "missing frozen field {key}");
        }
        assert!(json.get("session_id").is_none(), "no snake_case leakage");
    }

    // ── finalize: decision → terminal → sidecar + inbox ─────────────────

    #[tokio::test]
    async fn finalize_completed_writes_sidecar_complete_and_inbox_item() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let session = Uuid::new_v4();
        std::fs::create_dir_all(deps.sessions_dir.join(session.to_string())).unwrap();
        let handle = handle_for(session, GoalRunStatus::Running);

        finalize_goal_run(&deps, &app, &handle, GoalTerminal::Completed).await;

        let dto = handle.dto().await;
        assert_eq!(dto.status, "completed");

        // Sidecar row.
        let sidecar = SessionStore::new(deps.sessions_dir.clone()).sidecar(&session);
        let goal = sidecar.goal.expect("goal persisted");
        assert_eq!(goal.status, "complete");
        assert_eq!(goal.iterations, 3);

        // Inbox item (source=goal, summary carries turns + spend).
        let items = deps.inbox.list(None, Some(SOURCE_GOAL), 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Ship the thing");
        assert!(
            items[0].summary.contains("Completed"),
            "{:?}",
            items[0].summary
        );
        assert!(items[0].summary.contains("3"), "{:?}", items[0].summary);
        assert_eq!(
            items[0].session_id.as_deref(),
            Some(session.to_string()).as_deref()
        );
        assert!(items[0].error.is_none());
    }

    #[tokio::test]
    async fn finalize_blocked_pauses_sidecar_and_inbox_carries_reason() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let session = Uuid::new_v4();
        std::fs::create_dir_all(deps.sessions_dir.join(session.to_string())).unwrap();
        let handle = handle_for(session, GoalRunStatus::Running);

        finalize_goal_run(
            &deps,
            &app,
            &handle,
            GoalTerminal::Blocked("no kubeconfig".into()),
        )
        .await;

        let dto = handle.dto().await;
        assert_eq!(dto.status, "blocked");
        assert_eq!(dto.last_error.as_deref(), Some("no kubeconfig"));

        let sidecar = SessionStore::new(deps.sessions_dir.clone()).sidecar(&session);
        assert_eq!(sidecar.goal.unwrap().status, "paused");

        let items = deps.inbox.list(None, Some(SOURCE_GOAL), 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].error.as_deref(), Some("no kubeconfig"));
    }

    #[tokio::test]
    async fn finalize_paused_writes_inbox_with_reason() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let session = Uuid::new_v4();
        std::fs::create_dir_all(deps.sessions_dir.join(session.to_string())).unwrap();
        let handle = handle_for(session, GoalRunStatus::Running);

        finalize_goal_run(
            &deps,
            &app,
            &handle,
            GoalTerminal::Paused {
                reason: Some("Reached stall-strike budget (3/3)".into()),
            },
        )
        .await;

        assert_eq!(handle.dto().await.status, "paused");
        let items = deps.inbox.list(None, Some(SOURCE_GOAL), 10).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].summary.contains("Paused"));
        assert!(
            items[0]
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("stall-strike")
        );
    }

    #[tokio::test]
    async fn finalize_stopped_writes_no_inbox_item() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let session = Uuid::new_v4();
        std::fs::create_dir_all(deps.sessions_dir.join(session.to_string())).unwrap();
        let handle = handle_for(session, GoalRunStatus::Running);

        finalize_goal_run(&deps, &app, &handle, GoalTerminal::Stopped).await;

        assert_eq!(handle.dto().await.status, "stopped");
        let items = deps.inbox.list(None, Some(SOURCE_GOAL), 10).unwrap();
        assert!(items.is_empty(), "stopped must not write the inbox");
        // Sidecar still parked (recoverable, never auto-continues).
        let sidecar = SessionStore::new(deps.sessions_dir.clone()).sidecar(&session);
        assert_eq!(sidecar.goal.unwrap().status, "paused");
    }

    #[tokio::test]
    async fn panicked_loop_is_finalized_as_paused() {
        // Final-state discipline: a panicking engine phase must still reach
        // finalize with a terminal status.
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let session = Uuid::new_v4();
        std::fs::create_dir_all(deps.sessions_dir.join(session.to_string())).unwrap();
        let handle = handle_for(session, GoalRunStatus::Running);

        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let terminal = match tokio::spawn(async { panic!("boom") }).await {
            Ok(_) => unreachable!(),
            Err(join_error) => GoalTerminal::Paused {
                reason: Some(format!("goal task panicked: {join_error}")),
            },
        };
        std::panic::set_hook(prev_hook);
        finalize_goal_run(&deps, &app, &handle, terminal).await;

        let dto = handle.dto().await;
        assert_eq!(dto.status, "paused");
        assert!(dto.last_error.unwrap_or_default().contains("panicked"));
    }

    // ── control transitions ─────────────────────────────────────────────

    #[tokio::test]
    async fn pause_and_resume_transition_with_rearm() {
        let handle = handle_for(Uuid::new_v4(), GoalRunStatus::Running);
        handle.pause().await.unwrap();
        assert_eq!(handle.status().await, GoalRunStatus::Paused);
        // Pausing twice is rejected.
        assert!(handle.pause().await.is_err());

        handle.resume().await.unwrap();
        let s = handle.state.lock().await;
        assert_eq!(s.status, GoalRunStatus::Running);
        assert_eq!(s.iterations, 0, "resume re-arms the budget (TUI parity)");
        assert_eq!(s.stall_strikes, 0);
        assert_eq!(s.spent_usd, 0.0);
    }

    #[tokio::test]
    async fn resume_rejects_non_paused_states() {
        let handle = handle_for(Uuid::new_v4(), GoalRunStatus::Completed);
        assert!(handle.resume().await.is_err());
        let handle = handle_for(Uuid::new_v4(), GoalRunStatus::Running);
        assert!(handle.resume().await.is_err());
    }

    #[tokio::test]
    async fn stop_cancels_token_and_registry_block_lifts_on_terminal() {
        let session = Uuid::new_v4();
        let registry = GoalRunRegistry::new();
        let handle = handle_for(session, GoalRunStatus::Running);
        registry.start(session, handle.clone()).unwrap();
        assert!(registry.blocks_session(&session));

        // Second active start is rejected (mutual exclusion).
        assert!(
            registry
                .start(session, handle_for(session, GoalRunStatus::Running))
                .is_err()
        );

        handle.stop();
        assert!(handle.cancel.is_cancelled());

        // After the loop finalizes as terminal it no longer blocks, and a
        // new run may start on the same session.
        {
            let mut s = handle.state.lock().await;
            s.status = GoalRunStatus::Stopped;
        }
        assert!(!registry.blocks_session(&session));
        registry
            .start(session, handle_for(session, GoalRunStatus::Running))
            .expect("terminal run must not block a restart");
    }

    // ── restart reconciliation ──────────────────────────────────────────

    fn write_sidecar_goal(deps: &GoalRunDeps, session: Uuid, status: &str, objective: &str) {
        let store = SessionStore::new(deps.sessions_dir.clone());
        let mut sidecar = store.sidecar(&session);
        sidecar.goal = Some(StoredGoal {
            objective: objective.into(),
            status: status.into(),
            iterations: 4,
            max_iterations: 12,
        });
        store.save_sidecar_replace(&session, &sidecar).unwrap();
    }

    #[test]
    fn active_sidecar_goals_reconcile_to_interrupted() {
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let active = Uuid::new_v4();
        let done = Uuid::new_v4();
        std::fs::create_dir_all(deps.sessions_dir.join(active.to_string())).unwrap();
        std::fs::create_dir_all(deps.sessions_dir.join(done.to_string())).unwrap();
        write_sidecar_goal(&deps, active, "active", "revive me");
        write_sidecar_goal(&deps, done, "paused", "tui-owned");

        let registry = GoalRunRegistry::new();
        let cards = reconcile_interrupted_runs(&deps, &registry);
        assert_eq!(
            cards.len(),
            1,
            "only active sidecar goals surface: {cards:?}"
        );
        assert_eq!(cards[0].session_id, active.to_string());
        assert_eq!(cards[0].status, "interrupted");
        assert_eq!(cards[0].objective, "revive me");
        assert_eq!(cards[0].iterations, 4);
        assert_eq!(cards[0].max_turns, Some(12));
    }

    #[test]
    fn live_runners_mask_sidecar_reconciliation() {
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let session = Uuid::new_v4();
        std::fs::create_dir_all(deps.sessions_dir.join(session.to_string())).unwrap();
        write_sidecar_goal(&deps, session, "active", "live");

        let registry = GoalRunRegistry::new();
        registry
            .start(session, handle_for(session, GoalRunStatus::Running))
            .unwrap();
        assert!(reconcile_interrupted_runs(&deps, &registry).is_empty());
    }

    // ── helpers ─────────────────────────────────────────────────────────

    #[test]
    fn truncate_chars_respects_limit_and_utf8() {
        assert_eq!(truncate_chars("hello", 10), "hello");
        let cut = truncate_chars("héllo wörld", 5);
        assert!(cut.starts_with("héllo"));
        assert!(cut.ends_with('…'));
    }
}
