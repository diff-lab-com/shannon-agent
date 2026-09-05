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
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
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
    /// Lossless resume signal. `watch::changed()` compares against the
    /// last-seen generation rather than "a send that happens while someone
    /// is awaiting", so a resume that fires *before* the parked loop
    /// registers its wakeup still wakes it — the exact window where
    /// `Notify::notify_waiters` loses the notification (fix round 1,
    /// Important 2). The receiver must be marked (`borrow_and_update`)
    /// BEFORE the first paused check; `wait_while_paused` owns that order.
    pub(crate) resume_tx: tokio::sync::watch::Sender<u64>,
    resume_rx: tokio::sync::watch::Receiver<u64>,
}

impl GoalRunHandle {
    /// Constructor pairing the watch channel (generation 0).
    pub(crate) fn new(state: GoalRunState) -> Self {
        let (resume_tx, resume_rx) = tokio::sync::watch::channel(0u64);
        Self {
            state: tokio::sync::Mutex::new(state),
            cancel: CancellationToken::new(),
            resume_tx,
            resume_rx,
        }
    }

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
        // Bump the generation last: a waiter that already read `Paused`
        // will either see the new status on re-check or wake on the
        // changed generation — never both miss.
        self.resume_tx.send_modify(|generation| *generation += 1);
        Ok(())
    }

    /// `stop_goal_run`: cancels the current turn; the loop finalizes the
    /// run as `stopped` (no inbox item).
    pub(crate) fn stop(&self) {
        self.cancel.cancel();
    }

    /// Park until the run leaves `Paused` (or is stopped). Returns `true`
    /// if at least one paused state was observed — callers use this to
    /// re-arm the continuation prompt after a resume.
    ///
    /// Lossless by construction: the watch receiver is marked *before* the
    /// first status read, so a resume landing anywhere between "read
    /// Paused" and "await changed()" either already flipped the status
    /// (loop re-check exits) or bumps the generation past the seen one
    /// (`changed()` resolves immediately).
    pub(crate) async fn wait_while_paused(&self) -> bool {
        let mut resume_rx = self.resume_rx.clone();
        resume_rx.borrow_and_update();
        let mut parked = false;
        while self.state.lock().await.status == GoalRunStatus::Paused {
            parked = true;
            tokio::select! {
                _ = self.cancel.cancelled() => return parked,
                changed = resume_rx.changed() => {
                    if changed.is_err() {
                        // Sender dropped: impossible while the handle lives;
                        // treat as a wake to avoid hanging on a re-check.
                        return parked;
                    }
                    drop(resume_rx.borrow_and_update());
                }
            }
        }
        parked
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
    let handle = Arc::new(GoalRunHandle::new(run_state));
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
    let handle = Arc::new(GoalRunHandle::new(run_state));
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

/// What one turn produced. The engine runner folds the `goal_get` /
/// `goal_update` tool verdicts (read from the run's shared core after the
/// stream ends) into `tool_completed` / `tool_blocked`; the loop scans
/// `assistant_text` for the end-of-reply marker and consults
/// `tool_*` first — explicit structured statements beat the marker scan.
pub(crate) struct TurnObservation {
    /// Full assistant text of the turn (concatenated text events).
    assistant_text: String,
    had_tool_calls: bool,
    cost_usd: f64,
    failure: Option<String>,
    cancelled: bool,
    /// `goal_update` reported `complete` mid-turn.
    tool_completed: bool,
    /// `goal_update` reported `blocked` mid-turn, with its reason.
    tool_blocked: Option<String>,
}

/// Terminal outcomes the loop hands to [`finalize_goal_run`].
#[derive(Debug)]
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

/// Per-turn request. `iterations_done` is the completed-continuation count
/// *before* this turn — the `goal_get` snapshot shows exactly what the TUI
/// would show at the same point.
pub(crate) struct GoalTurnRequest {
    pub(crate) user_message: String,
    pub(crate) iterations_done: u32,
}

/// Single-turn execution, abstracted so the decision loop is testable
/// without an LLM: production wires [`EngineGoalTurnRunner`]; tests inject
/// a stub returning scripted [`TurnObservation`]s.
pub(crate) trait GoalTurnRunner: Send {
    /// Execute one turn and report what it produced.
    fn run_turn(
        &mut self,
        turn: GoalTurnRequest,
    ) -> Pin<Box<dyn Future<Output = TurnObservation> + Send + '_>>;

    /// Called once when the loop reaches a terminal state. The engine
    /// runner clears the injected `GoalSpec` here (`set_goal(None)`).
    fn finish(&mut self) {}
}

/// Production turn runner: owns the run's engine (built once —
/// `set_goal` once, restored from the L0 log) and streams every turn's
/// `QueryEvent`s to the Tauri wire so the chat page renders the run live.
struct EngineGoalTurnRunner<R: tauri::Runtime> {
    engine: QueryEngine,
    app: tauri::AppHandle<R>,
    handle: Arc<GoalRunHandle>,
    deps: GoalRunDeps,
    session_id: Uuid,
    model: String,
    model_for_usage: String,
    provider: String,
    /// goal_get/goal_update bridge, shared with the registered tools.
    shared: Arc<Mutex<GoalSharedCore>>,
}

impl<R: tauri::Runtime> EngineGoalTurnRunner<R> {
    /// Build the engine + per-run tool registry. Mirrors
    /// `spawn_routine_run`: unattended → configured approval mode honoured,
    /// persisted rules applied, FullAuto default.
    async fn new(
        deps: &GoalRunDeps,
        app: tauri::AppHandle<R>,
        handle: &Arc<GoalRunHandle>,
    ) -> Result<Self, String> {
        let client_config = deps.client_config.read().await.clone();
        let approval_mode_str = deps.desktop_config.read().await.approval_mode.clone();
        let model = client_config.model.clone();
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

        let (session_id, objective, max_turns, budget_usd) = {
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

        // Per-run tool registry: default tools + goal_get/goal_update bound
        // to this run's shared core (never the global registry — goal tools
        // must not leak into other sessions).
        let mut tools = shannon_core::tools::ToolRegistry::new();
        let assembly = shannon_remote::assembly::assemble_dynamic();
        // P1-3: honour the persisted `sandbox.mode` on the unattended path
        // too — same assembly-time seam as AppState::new.
        let desktop_cfg = deps.desktop_config.read().await;
        let sandboxed_providers = match crate::sandbox_assembly::effective_sandbox_providers(
            desktop_cfg.sandbox.as_ref().and_then(|s| s.mode.as_deref()),
            desktop_cfg.working_dir.as_deref(),
            &assembly.providers,
        ) {
            Ok(providers) => providers,
            Err(e) => {
                tracing::error!("goal run: sandbox disabled, continuing unrestricted: {e}");
                None
            }
        };
        register_default_tools_with_providers(
            &mut tools,
            sandboxed_providers.as_ref().unwrap_or(&assembly.providers),
        )
        .map_err(|e| format!("goal tool registry init failed: {e}"))?;
        shannon_tools::goal::register_goal_tools(
            &mut tools,
            Arc::new(RunnerGoalAccess {
                core: shared.clone(),
            }),
        )
        .map_err(|e| format!("registering goal tools failed: {e}"))?;

        let mut engine = QueryEngine::with_defaults_arc(
            LlmClient::new(client_config),
            Arc::new(tools),
            permissions,
            StateManager::new(),
        );
        engine.set_session_id(session_id);
        match engine.restore_session(session_id) {
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(session = %session_id, error = %e, "goal: history restore failed")
            }
        }
        // Goal injection: non-cached system block on every turn (brief:
        // engine.set_goal(GoalSpec{objective, paused:false})).
        engine.set_goal(Some(GoalSpec {
            objective,
            paused: false,
        }));

        Ok(Self {
            engine,
            app,
            handle: handle.clone(),
            deps: deps.clone(),
            session_id,
            model_for_usage: model.clone(),
            model,
            provider,
            shared,
        })
    }

    /// Drive one engine turn to completion, streaming events to the wire.
    async fn stream_turn(&mut self, turn: GoalTurnRequest) -> TurnObservation {
        // Sync the tools' view + engine goal for this turn (objective edits
        // via `update_goal_objective` take effect here — next turn).
        {
            let objective_now = self.handle.state.lock().await.objective.clone();
            let mut core = self.shared.lock().expect("goal shared core poisoned");
            core.objective = objective_now.clone();
            core.iterations = turn.iterations_done;
            core.tool_completed = false;
            core.tool_blocked = None;
            self.engine.set_goal(Some(GoalSpec {
                objective: objective_now,
                paused: false,
            }));
        }
        let user_message = turn.user_message;
        let query_id = Uuid::new_v4();
        let qid = query_id.to_string();
        let context = QueryContext {
            query_id,
            session_id: self.session_id,
            user_message,
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: None,
                model: self.model.clone(),
                temperature: None,
                top_p: None,
            },
        };

        let mut observation = TurnObservation {
            assistant_text: String::new(),
            had_tool_calls: false,
            cost_usd: 0.0,
            failure: None,
            cancelled: false,
            tool_completed: false,
            tool_blocked: None,
        };
        let mut completed = false;

        let stream = self.engine.process_query(context, None).await;
        use futures::StreamExt;
        let mut pin_stream = std::pin::pin!(stream);
        while let Some(event_result) = pin_stream.next().await {
            if self.handle.cancel.is_cancelled() {
                observation.cancelled = true;
                let _ = self.app.emit(
                    event_names::QUERY_CANCELLED,
                    crate::events::QueryCancelledPayload {
                        query_id: qid.clone(),
                        session_id: Some(self.session_id.to_string()),
                    },
                );
                break;
            }
            match event_result {
                Ok(event) => match event {
                    QueryEvent::Text { content, .. } => {
                        observation.assistant_text.push_str(&content);
                        let _ = self.app.emit(
                            event_names::QUERY_TEXT,
                            crate::events::QueryTextPayload {
                                query_id: qid.clone(),
                                content,
                                session_id: Some(self.session_id.to_string()),
                            },
                        );
                    }
                    QueryEvent::ToolUseRequest {
                        tool_use_id,
                        tool_name,
                        tool_input,
                        ..
                    } => {
                        observation.had_tool_calls = true;
                        let _ = self.app.emit(
                            event_names::QUERY_TOOL_START,
                            crate::events::ToolStartPayload {
                                query_id: qid.clone(),
                                tool_use_id,
                                tool_name,
                                tool_input,
                                session_id: Some(self.session_id.to_string()),
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
                        let _ = self.app.emit(
                            event_names::QUERY_TOOL_RESULT,
                            crate::events::ToolResultPayload {
                                query_id: qid.clone(),
                                tool_use_id,
                                tool_name,
                                result,
                                is_error,
                                session_id: Some(self.session_id.to_string()),
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
                        let _ = self.app.emit(
                            event_names::QUERY_TOOL_PROGRESS,
                            crate::events::ToolProgressPayload {
                                query_id: qid.clone(),
                                tool_use_id,
                                tool_name,
                                progress,
                                message,
                                session_id: Some(self.session_id.to_string()),
                            },
                        );
                    }
                    QueryEvent::Thinking { content, .. } => {
                        let _ = self.app.emit(
                            event_names::QUERY_THINKING,
                            crate::events::ThinkingPayload {
                                query_id: qid.clone(),
                                content,
                                session_id: Some(self.session_id.to_string()),
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
                        observation.cost_usd += event_cost;
                        // Best-effort ledger write, mirroring send_message.
                        let _ = self
                            .deps
                            .usage_store
                            .append(&crate::commands_usage::record_event(
                                &self.model_for_usage,
                                &self.provider,
                                crate::commands_usage::UsageTotals {
                                    input_tokens,
                                    output_tokens,
                                    cache_creation_tokens,
                                    cache_read_tokens,
                                    cost_usd: event_cost,
                                },
                                Some(&self.session_id.to_string()),
                            ));
                        let _ = self.app.emit(
                            event_names::QUERY_USAGE,
                            crate::events::UsagePayload {
                                query_id: qid.clone(),
                                input_tokens,
                                output_tokens,
                                cost_usd: event_cost,
                                session_id: Some(self.session_id.to_string()),
                            },
                        );
                    }
                    QueryEvent::Completed { .. } => {
                        completed = true;
                        break;
                    }
                    QueryEvent::Failed { error, .. } => {
                        let _ = self.app.emit(
                            event_names::QUERY_FAILED,
                            crate::events::QueryFailedPayload {
                                query_id: qid.clone(),
                                error: error.clone(),
                                session_id: Some(self.session_id.to_string()),
                            },
                        );
                        observation.failure = Some(error);
                        break;
                    }
                    _ => {}
                },
                Err(e) => {
                    let err = e.to_string();
                    let _ = self.app.emit(
                        event_names::QUERY_FAILED,
                        crate::events::QueryFailedPayload {
                            query_id: qid.clone(),
                            error: err.clone(),
                            session_id: Some(self.session_id.to_string()),
                        },
                    );
                    observation.failure = Some(err);
                    break;
                }
            }
        }
        if completed && !observation.cancelled {
            let _ = self.app.emit(
                event_names::QUERY_COMPLETED,
                crate::events::QueryCompletedPayload {
                    query_id: qid.clone(),
                    session_id: Some(self.session_id.to_string()),
                },
            );
        }

        // Fold the tools' mid-turn verdicts into the observation (and reset
        // them so the next turn starts clean).
        {
            let mut core = self.shared.lock().expect("goal shared core poisoned");
            observation.tool_completed = core.tool_completed;
            observation.tool_blocked = core.tool_blocked.take();
            core.tool_completed = false;
        }
        observation
    }
}

impl<R: tauri::Runtime> GoalTurnRunner for EngineGoalTurnRunner<R> {
    fn run_turn(
        &mut self,
        turn: GoalTurnRequest,
    ) -> Pin<Box<dyn Future<Output = TurnObservation> + Send + '_>> {
        Box::pin(self.stream_turn(turn))
    }

    fn finish(&mut self) {
        // Terminal: drop the injected goal (brief: engine.set_goal(None)).
        self.engine.set_goal(None);
    }
}

/// Drive the goal run to a terminal state. Production wiring: build the
/// engine runner, wrap the loop in the panic guard, finalize. Final-state
/// discipline: every exit — decision terminal, stop, engine failure, panic
/// — flows through [`finalize_goal_run`]; the run can never be left
/// `running`. Generic over the Tauri runtime so tests can drive it with
/// `mock_app`.
async fn run_goal_loop<R: tauri::Runtime>(
    deps: GoalRunDeps,
    app: tauri::AppHandle<R>,
    handle: Arc<GoalRunHandle>,
) {
    let deps_for_runner = deps.clone();
    let app_for_runner = app.clone();
    let handle_for_runner = handle.clone();
    let engine_future = async move {
        let mut runner = match EngineGoalTurnRunner::new(
            &deps_for_runner,
            app_for_runner.clone(),
            &handle_for_runner,
        )
        .await
        {
            Ok(runner) => runner,
            Err(e) => return GoalTerminal::Paused { reason: Some(e) },
        };
        let terminal = run_turn_loop(
            &deps_for_runner,
            &app_for_runner,
            &handle_for_runner,
            &mut runner,
        )
        .await;
        // Terminal: drop the injected goal (brief: engine.set_goal(None)).
        runner.finish();
        terminal
    };
    let terminal = match tokio::spawn(engine_future).await {
        Ok(terminal) => terminal,
        Err(join_error) => GoalTerminal::Paused {
            reason: Some(format!("goal task panicked: {join_error}")),
        },
    };
    finalize_goal_run(&deps, &app, &handle, terminal).await;
}

/// Push the current DTO snapshot to the Tasks page (turn progress, terminal
/// state, pause/resume — the run cards never poll).
async fn emit_goal_update<R: tauri::Runtime>(app: &tauri::AppHandle<R>, handle: &GoalRunHandle) {
    let dto = handle.dto().await;
    let _ = app.emit(event_names::GOAL_UPDATED, dto);
}

/// The decision loop, generic over the turn executor. Counters live here
/// and mirror `check_goal_continuation` exactly (see the decision arms);
/// marker scanning, tool-verdict precedence and prompt injection are all
/// covered by stub-driven tests.
async fn run_turn_loop<R: tauri::Runtime, T: GoalTurnRunner + ?Sized>(
    deps: &GoalRunDeps,
    app: &tauri::AppHandle<R>,
    handle: &Arc<GoalRunHandle>,
    turns: &mut T,
) -> GoalTerminal {
    // Track the runner's own copy of the counters for the decision input.
    let (max_turns, budget_usd) = {
        let s = handle.state.lock().await;
        (s.max_turns, s.budget_usd)
    };
    let mut iterations: u32 = 0;
    let mut spent_usd: f64 = 0.0;
    let mut consecutive_no_tool_turns: u32 = 0;
    let mut stall_strikes: u32 = 0;
    // Whether at least one turn completed — distinguishes "parked before
    // turn 1" (resume still injects the objective) from a mid-run resume
    // (TUI re-arm: the next prompt is a fresh continuation prompt).
    let mut ran_any_turn = false;
    // The next user message: the objective for turn 1, the exact TUI
    // continuation prompt for subsequent turns.
    let mut pending_prompt = Some(handle.state.lock().await.objective.clone());

    loop {
        // Park while paused (pause takes effect at turn boundaries); stop
        // wins over everything.
        let parked = handle.wait_while_paused().await;
        if handle.cancel.is_cancelled() {
            return GoalTerminal::Stopped;
        }
        if handle.status().await != GoalRunStatus::Running {
            continue; // something else changed — re-check at the top
        }
        if parked {
            // Resume re-armed the budget (TUI parity): adopt the reset
            // counters. After any completed turn the next prompt is a fresh
            // continuation prompt; a resume before turn 1 keeps the
            // objective pending.
            let s = handle.state.lock().await;
            iterations = s.iterations;
            spent_usd = s.spent_usd;
            consecutive_no_tool_turns = s.consecutive_no_tool_turns;
            stall_strikes = s.stall_strikes;
            if ran_any_turn {
                pending_prompt = Some(shannon_core::goal_loop::continuation_prompt(
                    iterations + 1,
                    s.max_turns.unwrap_or(0),
                    &s.objective,
                ));
            } else {
                pending_prompt.get_or_insert_with(|| s.objective.clone());
            }
        }

        let Some(user_message) = pending_prompt.take() else {
            // Unreachable: pending_prompt is always set before the loop and
            // re-armed on every Continue/resume. Treat as a safety stop.
            return GoalTerminal::Paused {
                reason: Some("internal: continuation queue ran dry".into()),
            };
        };

        let observation = turns
            .run_turn(GoalTurnRequest {
                user_message,
                iterations_done: iterations,
            })
            .await;
        ran_any_turn = true;

        // Spend accumulates every turn; the iteration/guard counters do NOT
        // advance here — the TUI passes pre-turn counters into the decision
        // and the decision arms apply the advancement exactly once (see
        // check_goal_continuation). Cancels/failures leave counters
        // untouched, mirroring the TUI where an errored/cancelled query
        // simply stops the loop.
        spent_usd += observation.cost_usd;
        {
            let mut s = handle.state.lock().await;
            s.spent_usd = spent_usd;
            s.updated_at_ms = now_ms();
            if let Some(err) = &observation.failure {
                s.last_error = Some(err.clone());
            }
        }
        persist_turn_progress(deps, handle).await;
        // Fix round 1, Important 1: the run cards subscribe to
        // `goal:updated`; emit once per completed turn so iterations/spend
        // move live instead of only at control actions and finalize.
        emit_goal_update(app, handle).await;

        // Cancelled mid-turn → stopped (no inbox, no decision).
        if observation.cancelled {
            return GoalTerminal::Stopped;
        }

        // Engine failure → recoverable pause with the error preserved.
        if let Some(err) = observation.failure {
            return GoalTerminal::Paused { reason: Some(err) };
        }

        // Structured tool verdicts (goal_update) beat the marker scan —
        // they are explicit model statements made mid-turn.
        if observation.tool_completed {
            return GoalTerminal::Completed;
        }
        if let Some(reason) = observation.tool_blocked {
            return GoalTerminal::Blocked(reason);
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
                return GoalTerminal::Completed;
            }
            Some(GoalMarker::Blocked(reason)) => {
                return GoalTerminal::Blocked(reason);
            }
            None => {}
        }

        // P0-4: session-budget guard — first-limit-wins with the goal's own
        // budget. The goal cap is enforced inside the decision below; the
        // session cap (sidecar `budget_usd`) is checked here against the
        // usage ledger's cumulative session spend (goal turns write the same
        // ledger as manual sends). Completion/block verdicts above win: the
        // turn that actually finished (or hard-blocked) the goal is not
        // retroactively aborted, but the run never continues past the cap.
        {
            let session_id = handle.state.lock().await.session_id;
            let cap = deps.session_store().sidecar(&session_id).budget_usd;
            if let Some(cap) = cap {
                let spent = deps.usage_store.spent_for_session(&session_id.to_string());
                if crate::cost_commands::budget_verdict(spent, cap)
                    == crate::cost_commands::BudgetVerdict::Exceeded
                {
                    crate::cost_commands::emit_budget_status(
                        app,
                        false,
                        &session_id.to_string(),
                        spent,
                        cap,
                    );
                    return GoalTerminal::Paused {
                        reason: Some(format!("session budget reached: ${spent:.4} of ${cap:.4}")),
                    };
                }
            }
        }

        // Pure decision (shared with the TUI). The input carries the
        // counters as of *before* this turn's advancement — the core
        // advances them internally for its verdicts, and the arms below
        // persist the advanced values exactly like the TUI does.
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
                // Advance the guard counters exactly like the TUI's
                // check_goal_continuation Continue arm.
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
                    s.consecutive_no_tool_turns = consecutive_no_tool_turns;
                    s.stall_strikes = stall_strikes;
                }
                persist_turn_progress(deps, handle).await;
                emit_goal_update(app, handle).await;
                pending_prompt = Some(prompt);
            }
            shannon_core::goal_loop::GoalContinuation::MaxReached => {
                // TUI parity: the MaxReached arm pauses without persisting
                // the decision's internal +1.
                return GoalTerminal::Paused { reason: None };
            }
            shannon_core::goal_loop::GoalContinuation::BudgetLimited(reason) => {
                return GoalTerminal::Paused {
                    reason: Some(reason),
                };
            }
            shannon_core::goal_loop::GoalContinuation::PausedNoProgress(reason) => {
                // TUI parity: the no-progress pause persists the advanced
                // counters and the decision's +1 so the user can see the
                // strike counts that tripped it.
                iterations += 1;
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
                    s.consecutive_no_tool_turns = consecutive_no_tool_turns;
                    s.stall_strikes = stall_strikes;
                }
                persist_turn_progress(deps, handle).await;
                return GoalTerminal::Paused {
                    reason: Some(reason),
                };
            }
            // decide_goal_continuation never returns Inactive (see core docs).
            shannon_core::goal_loop::GoalContinuation::Inactive
            | shannon_core::goal_loop::GoalContinuation::Completed
            | shannon_core::goal_loop::GoalContinuation::Blocked(_) => {
                return GoalTerminal::Paused {
                    reason: Some("internal: unexpected decision verdict".into()),
                };
            }
        }
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
        Arc::new(GoalRunHandle::new(GoalRunState {
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
        }))
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

    // ── turn loop (fix round 1, Important 3): stub-driven coverage ─────

    /// Scripted single-turn executor: pops one observation per call and
    /// records the injected user messages for prompt assertions.
    struct StubTurnRunner {
        script: Mutex<Vec<TurnObservation>>,
        messages: Mutex<Vec<String>>,
    }

    impl StubTurnRunner {
        fn new(script: Vec<TurnObservation>) -> Self {
            Self {
                script: Mutex::new(script),
                messages: Mutex::new(Vec::new()),
            }
        }

        fn messages(&self) -> Vec<String> {
            self.messages.lock().unwrap().clone()
        }
    }

    impl GoalTurnRunner for StubTurnRunner {
        fn run_turn(
            &mut self,
            turn: GoalTurnRequest,
        ) -> Pin<Box<dyn Future<Output = TurnObservation> + Send + '_>> {
            Box::pin(async move {
                self.messages.lock().unwrap().push(turn.user_message);
                // Vec::remove panics on an exhausted script — loud failure
                // beats a silent hang if the loop runs past the script.
                self.script.lock().unwrap().remove(0)
            })
        }
    }

    fn obs(text: &str, had_tools: bool) -> TurnObservation {
        TurnObservation {
            assistant_text: text.into(),
            had_tool_calls: had_tools,
            cost_usd: 0.0,
            failure: None,
            cancelled: false,
            tool_completed: false,
            tool_blocked: None,
        }
    }

    /// Fresh running handle (counters at zero) for loop tests.
    fn fresh_handle(session: Uuid) -> Arc<GoalRunHandle> {
        Arc::new(GoalRunHandle::new(GoalRunState {
            session_id: session,
            title: "Ship the thing".into(),
            objective: "make CI green".into(),
            status: GoalRunStatus::Running,
            iterations: 0,
            max_turns: None,
            spent_usd: 0.0,
            budget_usd: None,
            stall_strikes: 0,
            consecutive_no_tool_turns: 0,
            last_error: None,
            started_at_ms: 1_000,
            updated_at_ms: 2_000,
        }))
    }

    /// ① Counter bookkeeping matches check_goal_continuation field by
    /// field (the scenario fixed in 2c5e72b9): a tool turn resets strikes,
    /// two consecutive no-tool turns trip anti-spin on the SECOND one, and
    /// the PausedNoProgress arm persists the advanced counters with +1.
    #[tokio::test]
    async fn turn_loop_counters_match_tui_bookkeeping() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let handle = fresh_handle(Uuid::new_v4());
        let mut stub = StubTurnRunner::new(vec![
            obs("Partial progress.", true), // Continue {1}, strikes reset
            obs("Still working.", false),   // Continue {2}, strikes 0→1
            obs("Just thinking.", false),   // anti-spin: cons 1→2 ⇒ PausedNoProgress
        ]);

        let terminal = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_turn_loop(&deps, &app, &handle, &mut stub),
        )
        .await
        .expect("loop must not hang");

        // The reason rides the terminal verdict; finalize persists it into
        // `last_error` (covered by finalize_paused_writes_inbox_with_reason).
        match &terminal {
            GoalTerminal::Paused { reason: Some(r) } => {
                assert!(r.contains("Two consecutive"), "{r}")
            }
            other => panic!("two no-tool turns must pause: {other:?}"),
        }
        let s = handle.state.lock().await;
        assert_eq!(
            s.iterations, 3,
            "decision +1 per Continue/PausedNoProgress arm"
        );
        assert_eq!(s.consecutive_no_tool_turns, 2);
        assert_eq!(
            s.stall_strikes, 2,
            "one no-tool turn before the trip (0→1→2)"
        );
        // run_turn_loop returns the verdict without finalizing — flipping
        // the stored status / last_error is finalize_goal_run's job.
        assert_eq!(s.status, GoalRunStatus::Running);
        // Prompt injection ③: turn 1 carries the objective verbatim, later
        // turns carry the exact TUI continuation contract.
        let msgs = stub.messages();
        assert_eq!(msgs[0], "make CI green");
        assert!(msgs[1].contains("[Goal iteration 1/∞]"), "{}", msgs[1]);
        assert!(msgs[1].contains("make CI green"));
        assert!(msgs[1].contains("GOAL_COMPLETE") && msgs[1].contains("GOAL_BLOCKED"));
        assert!(msgs[2].contains("[Goal iteration 2/∞]"), "{}", msgs[2]);
    }

    /// ② Verdict precedence: goal_update's structured `complete` beats a
    /// GOAL_BLOCKED marker on the same reply; a plain blocked marker (no
    /// tool verdict) maps to Blocked(reason); mid-text markers never count.
    #[tokio::test]
    async fn turn_loop_tool_verdict_beats_marker_scan() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let handle = fresh_handle(Uuid::new_v4());
        let mut tool_done = obs("Cannot access cluster.\nGOAL_BLOCKED: no kubeconfig", true);
        tool_done.tool_completed = true;
        let mut stub = StubTurnRunner::new(vec![tool_done]);

        let terminal = run_turn_loop(&deps, &app, &handle, &mut stub).await;
        assert!(matches!(terminal, GoalTerminal::Completed), "{terminal:?}");
    }

    #[tokio::test]
    async fn turn_loop_blocked_marker_carries_reason() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let handle = fresh_handle(Uuid::new_v4());
        let mut stub = StubTurnRunner::new(vec![obs(
            "Cannot proceed.\nGOAL_BLOCKED: need prod credentials",
            true,
        )]);

        let terminal = run_turn_loop(&deps, &app, &handle, &mut stub).await;
        match terminal {
            GoalTerminal::Blocked(reason) => assert_eq!(reason, "need prod credentials"),
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn turn_loop_mid_text_marker_does_not_stop_the_run() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let handle = fresh_handle(Uuid::new_v4());
        let mut stub = StubTurnRunner::new(vec![
            obs("GOAL_COMPLETE is near — one more step.", true),
            obs("Done.\nGOAL_COMPLETE", true),
        ]);

        let terminal = run_turn_loop(&deps, &app, &handle, &mut stub).await;
        assert!(matches!(terminal, GoalTerminal::Completed), "{terminal:?}");
        assert_eq!(stub.messages().len(), 2, "first turn must continue");
    }

    /// ④ Park → resume → continue: a paused loop wakes on resume (TUI
    /// re-arm: counters reset, fresh continuation prompt) and runs to the
    /// completion marker. Timeout guards against a lost wakeup.
    #[tokio::test]
    async fn turn_loop_park_resume_then_continue() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let handle = fresh_handle(Uuid::new_v4());
        handle.pause().await.unwrap();

        let handle_for_task = handle.clone();
        let deps_for_task = deps.clone();
        let app_for_task = app.clone();
        let task = tokio::spawn(async move {
            let mut stub = StubTurnRunner::new(vec![
                obs("Working…", true),
                obs("All green.\nGOAL_COMPLETE", true),
            ]);
            run_turn_loop(&deps_for_task, &app_for_task, &handle_for_task, &mut stub).await
        });

        // Give the loop a beat to park, then resume and let it finish.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        handle.resume().await.unwrap();

        let terminal = tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .expect("parked loop must wake on resume")
            .unwrap();
        assert!(matches!(terminal, GoalTerminal::Completed), "{terminal:?}");
        let s = handle.state.lock().await;
        // TUI counting: the marker-completing turn is not a continuation,
        // so exactly one Continue (after the objective turn) is counted.
        assert_eq!(s.iterations, 1, "resume re-armed: counted from 0");
        assert_eq!(
            s.status,
            GoalRunStatus::Running,
            "run_turn_loop does not finalize"
        );
    }

    /// ⑤ Fix round 1, Important 1: every completed turn emits
    /// `goal:updated` — one emit after the per-turn spend sync plus one on
    /// the Continue branch (terminal turns emit via finalize, absent here).
    #[tokio::test]
    async fn turn_loop_emits_goal_updated_every_turn() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tauri::Listener;

        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let handle = fresh_handle(Uuid::new_v4());

        let emissions = Arc::new(AtomicUsize::new(0));
        let counter = emissions.clone();
        app.listen_any(event_names::GOAL_UPDATED, move |_event| {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        let mut stub = StubTurnRunner::new(vec![
            obs("Working…", true),             // Continue → post-turn emit + Continue emit
            obs("Done.\nGOAL_COMPLETE", true), // terminal → post-turn emit only
        ]);

        let terminal = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_turn_loop(&deps, &app, &handle, &mut stub),
        )
        .await
        .expect("loop must not hang");
        assert!(matches!(terminal, GoalTerminal::Completed), "{terminal:?}");
        assert_eq!(
            emissions.load(Ordering::SeqCst),
            3,
            "2 emits on the continuing turn + 1 on the terminal turn"
        );
    }

    /// ⑤b P0-4: the session budget co-exists with the goal's own budget —
    /// first-limit-wins. The session cap lives in the sidecar
    /// (`budget_usd`); the ledger already holds spend at the cap, so after
    /// the first turn the loop pauses with a session-budget reason and
    /// emits `budget:exceeded` instead of queueing a continuation.
    #[tokio::test]
    async fn turn_loop_pauses_when_session_budget_exceeded() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tauri::Listener;

        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let session = Uuid::new_v4();
        let handle = fresh_handle(session);

        // Session cap in the sidecar + cumulative session spend at the cap.
        deps.session_store()
            .save_sidecar_replace(
                &session,
                &shannon_core::session_log::SessionSidecar {
                    budget_usd: Some(1.0),
                    ..Default::default()
                },
            )
            .unwrap();
        deps.usage_store
            .append(&crate::commands_usage::record_event(
                "test-model",
                "anthropic",
                crate::commands_usage::UsageTotals {
                    input_tokens: 1,
                    output_tokens: 1,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    cost_usd: 1.0,
                },
                Some(&session.to_string()),
            ))
            .unwrap();

        let exceeded = Arc::new(AtomicUsize::new(0));
        let counter = exceeded.clone();
        app.listen_any(event_names::BUDGET_EXCEEDED, move |_event| {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        let mut stub = StubTurnRunner::new(vec![
            obs("Turn one done.", true),
            obs("Turn two must never run.", true),
        ]);

        let terminal = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_turn_loop(&deps, &app, &handle, &mut stub),
        )
        .await
        .expect("loop must not hang");
        match terminal {
            GoalTerminal::Paused { reason } => {
                let reason = reason.expect("session-budget pause carries a reason");
                assert!(
                    reason.contains("session budget"),
                    "reason must name the session budget: {reason}"
                );
            }
            other => panic!("expected session-budget Paused, got {other:?}"),
        }
        assert_eq!(
            exceeded.load(Ordering::SeqCst),
            1,
            "budget:exceeded fires once"
        );
        assert_eq!(
            stub.messages().len(),
            1,
            "no continuation turn may start past the session cap"
        );
    }

    /// ⑤c P0-4: under the session cap the guard is a no-op — the run
    /// completes normally even with a sidecar cap present.
    #[tokio::test]
    async fn turn_loop_ignores_session_budget_below_the_cap() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let session = Uuid::new_v4();
        let handle = fresh_handle(session);

        deps.session_store()
            .save_sidecar_replace(
                &session,
                &shannon_core::session_log::SessionSidecar {
                    budget_usd: Some(10.0),
                    ..Default::default()
                },
            )
            .unwrap();

        let mut stub = StubTurnRunner::new(vec![obs("Done.\nGOAL_COMPLETE", true)]);
        let terminal = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_turn_loop(&deps, &app, &handle, &mut stub),
        )
        .await
        .expect("loop must not hang");
        assert!(matches!(terminal, GoalTerminal::Completed), "{terminal:?}");
    }

    /// ⑥ Fix round 1, Important 2 regression: a resume that lands *before*
    /// the loop registers its wakeup must not be lost. The loop starts
    /// parked; resume is issued from another task while the loop is in its
    /// pre-park window; the watch generation (marked before the status
    /// check) guarantees the wake. Timeout guards the old Notify race.
    #[tokio::test]
    async fn resume_before_park_registration_still_wakes_the_loop() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let deps = temp_deps(tmp.path());
        let handle = fresh_handle(Uuid::new_v4());
        handle.pause().await.unwrap();

        let handle_for_task = handle.clone();
        let deps_for_task = deps.clone();
        let app_for_task = app.clone();
        let task = tokio::spawn(async move {
            let mut stub = StubTurnRunner::new(vec![obs("Recovered.\nGOAL_COMPLETE", true)]);
            run_turn_loop(&deps_for_task, &app_for_task, &handle_for_task, &mut stub).await
        });

        // Resume immediately — races with the loop's park entry. Under the
        // previous Notify::notify_waiters scheme this could be lost.
        handle.resume().await.unwrap();

        let terminal = tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .expect("resume must wake the loop even when it races the park")
            .unwrap();
        assert!(matches!(terminal, GoalTerminal::Completed), "{terminal:?}");
    }

    /// `wait_while_paused` is lossless by construction: mark-before-check.
    /// A resume that already fired (status Running, generation bumped)
    /// exits immediately without parking; a resume while parked wakes.
    #[tokio::test]
    async fn wait_while_paused_wakes_on_resume_and_reports_parked() {
        let handle = fresh_handle(Uuid::new_v4());

        // Not paused: returns immediately, parked == false.
        let parked = handle.wait_while_paused().await;
        assert!(!parked);

        handle.pause().await.unwrap();
        let waiter = tokio::spawn({
            let handle = handle.clone();
            async move { handle.wait_while_paused().await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        handle.resume().await.unwrap();
        let parked = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("waiter must wake on resume")
            .unwrap();
        assert!(parked, "observing a paused state must report parked=true");
    }
}
