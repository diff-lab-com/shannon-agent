//! Tauri IPC commands for Scheduled Tasks, Triage, and Triggered Routines.
//!
//! Backs the Sprint 2 desktop UI: Tasks board, Triage sidebar, History view,
//! and Triggered Routines panel. Storage lives in `~/.shannon/`:
//!
//! - `scheduled-tasks/<slug>-<id>/` — per-task `SKILL.md` + `task.json`
//! - `scheduled-runs/YYYY/MM.jsonl` — append-only execution history
//! - `triage.jsonl` — needs-review items (failed runs, budget alerts, etc.)
//! - `routine-overrides.json` — per-name enabled/disabled overrides for
//!   triggered routines (the source TOML is read-only here)
//!
//! Field names mirror [`shannon_core::scheduled_routines::ScheduledRoutine`]
//! exactly (no rename to "ScheduledTask") so the frontend can pass structs
//! through verbatim.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use shannon_core::scheduled_routines::{
    ExecutionPolicy, ScheduledRoutine, TriggerType, compute_next_fire_utc,
};
use shannon_core::scheduled_runs::{ScheduledRun, ScheduledRunsStore};
use shannon_core::scheduled_task_store::ScheduledTaskStore;
use shannon_core::triggered_routines::TriggeredRoutineRegistry;
use tokio::sync::RwLock;

use crate::commands::AppState;

// ─── DTOs ───────────────────────────────────────────────────────────────────

/// Payload for `create_scheduled_task`.
///
/// Field names match [`ScheduledRoutine`] exactly. Either `interval_secs`
/// (interval mode) or `cron_expr` (cron mode) must be supplied; the
/// `trigger_type` field selects which is consulted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskPayload {
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub trigger_type: Option<String>,
    #[serde(default)]
    pub interval_secs: Option<u64>,
    #[serde(default)]
    pub cron_expr: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub max_fires: Option<u32>,
    #[serde(default)]
    pub policy: Option<ExecutionPolicy>,
    /// Initial dependency list. Defaults to empty when omitted.
    #[serde(default)]
    pub depends_on: Option<Vec<String>>,
}

/// Payload for `update_scheduled_task`. All fields optional except `id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTaskPayload {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub trigger_type: Option<String>,
    #[serde(default)]
    pub interval_secs: Option<u64>,
    #[serde(default)]
    pub cron_expr: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub max_fires: Option<u32>,
    #[serde(default)]
    pub policy: Option<ExecutionPolicy>,
    /// Replaces the dependency list wholesale when supplied. The UI sends
    /// the full new list (add or remove); an empty vec clears all deps.
    #[serde(default)]
    pub depends_on: Option<Vec<String>>,
}

/// Result of `preview_cron`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronPreview {
    pub expression: String,
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Next N fire times as Unix timestamps (seconds), in chronological order.
    pub next_fires: Vec<i64>,
}

/// A single triage item needing user attention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriageItem {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Kind: `"failed_run"`, `"budget_exceeded"`, `"needs_review"`, etc.
    pub kind: String,
    pub message: String,
    pub created_at: i64,
    #[serde(default)]
    pub revision: u32,
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub archived: bool,
}

/// Filters for `list_triage_items`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriageFilter {
    #[serde(default)]
    pub unread_only: Option<bool>,
    #[serde(default)]
    pub unarchived_only: Option<bool>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Aggregate triage counts for the sidebar badge.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriageStats {
    pub total: usize,
    pub unread: usize,
    pub archived: usize,
    pub by_kind: HashMap<String, usize>,
}

/// Lightweight execution record for the history list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecution {
    pub run_id: String,
    pub task_id: String,
    pub task_name: String,
    pub started_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<u64>,
}

/// Full execution detail view (history list item + task metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionDetail {
    #[serde(flatten)]
    pub execution: TaskExecution,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron_expr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_fire_at: Option<i64>,
}

/// Triggered routine row for the routines panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggeredRoutineDto {
    pub name: String,
    pub trigger: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    pub command: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Response from `trigger_task_now`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerResponse {
    pub run_id: String,
    pub task_id: String,
    pub task_name: String,
}

// ─── AppState storage helpers ───────────────────────────────────────────────

/// Append-only JSONL triage store. Mirrors the runs-store pattern: updates
/// append a new line with `revision += 1`; reads resolve to the latest
/// revision per `id`.
#[derive(Debug, Clone)]
pub struct TriageStore {
    path: PathBuf,
}

impl TriageStore {
    /// Default location: `~/.shannon/triage.jsonl`.
    pub fn new() -> Self {
        Self {
            path: default_triage_path(),
        }
    }

    /// Custom path (for testing).
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Path accessor.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Append a new triage item.
    pub fn add(&self, kind: &str, message: &str) -> Result<TriageItem, String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create triage dir: {e}"))?;
        }
        let item = TriageItem {
            id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
            task_id: None,
            task_name: None,
            run_id: None,
            kind: kind.to_string(),
            message: message.to_string(),
            created_at: Utc::now().timestamp(),
            revision: 0,
            read: false,
            archived: false,
        };
        self.append(&item)?;
        Ok(item)
    }

    /// Append a fully-constructed item (used by tests and future auto-triage).
    pub fn append(&self, item: &TriageItem) -> Result<(), String> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("open triage file: {e}"))?;
        let line = serde_json::to_string(item).map_err(|e| format!("encode triage: {e}"))?;
        writeln!(file, "{line}").map_err(|e| format!("write triage: {e}"))?;
        Ok(())
    }

    /// Atomically mutate an item by ID using `update_fn`, then append the
    /// updated record with bumped revision.
    fn mutate<F>(&self, id: &str, update_fn: F) -> Result<TriageItem, String>
    where
        F: FnOnce(&mut TriageItem),
    {
        let mut item = self
            .find_by_id(id)?
            .ok_or_else(|| format!("triage item not found: {id}"))?;
        update_fn(&mut item);
        item.revision = item.revision.saturating_add(1);
        self.append(&item)?;
        Ok(item)
    }

    /// Mark an item as read.
    pub fn mark_read(&self, id: &str) -> Result<TriageItem, String> {
        self.mutate(id, |i| i.read = true)
    }

    /// Archive an item.
    pub fn archive(&self, id: &str) -> Result<TriageItem, String> {
        self.mutate(id, |i| {
            i.archived = true;
            i.read = true;
        })
    }

    /// Find a single item by ID (returns the latest revision).
    pub fn find_by_id(&self, id: &str) -> Result<Option<TriageItem>, String> {
        Ok(self.latest_by_id()?.remove(id))
    }

    /// List items matching the filter, newest first.
    pub fn list(&self, filter: &TriageFilter) -> Result<Vec<TriageItem>, String> {
        let mut items: Vec<_> = self
            .latest_by_id()?
            .into_values()
            .filter(|i| {
                if filter.unread_only.unwrap_or(false) && i.read {
                    return false;
                }
                if filter.unarchived_only.unwrap_or(false) && i.archived {
                    return false;
                }
                if let Some(ref kind) = filter.kind {
                    if &i.kind != kind {
                        return false;
                    }
                }
                true
            })
            .collect();
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        if let Some(limit) = filter.limit {
            items.truncate(limit);
        }
        Ok(items)
    }

    /// Compute aggregate stats.
    pub fn stats(&self) -> Result<TriageStats, String> {
        let items: Vec<_> = self.latest_by_id()?.into_values().collect();
        let total = items.len();
        let mut unread = 0usize;
        let mut archived = 0usize;
        let mut by_kind: HashMap<String, usize> = HashMap::new();
        for item in &items {
            if !item.read {
                unread += 1;
            }
            if item.archived {
                archived += 1;
            }
            *by_kind.entry(item.kind.clone()).or_insert(0) += 1;
        }
        Ok(TriageStats {
            total,
            unread,
            archived,
            by_kind,
        })
    }

    /// Read all entries, keeping only the latest revision per ID.
    fn latest_by_id(&self) -> Result<HashMap<String, TriageItem>, String> {
        let mut map: HashMap<String, TriageItem> = HashMap::new();
        if !self.path.exists() {
            return Ok(map);
        }
        let file = match OpenOptions::new().read(true).open(&self.path) {
            Ok(f) => f,
            Err(_) => return Ok(map),
        };
        for line in BufReader::new(file).lines() {
            let line = line.map_err(|e| format!("read triage line: {e}"))?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(item) = serde_json::from_str::<TriageItem>(&line) {
                let entry = map.entry(item.id.clone()).or_insert_with(|| item.clone());
                if item.revision >= entry.revision {
                    *entry = item;
                }
            }
        }
        Ok(map)
    }
}

impl Default for TriageStore {
    fn default() -> Self {
        Self::new()
    }
}

fn default_triage_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".shannon")
        .join("triage.jsonl")
}

/// Per-name enabled/disabled overrides for triggered routines. Stored as a
/// flat JSON map at `~/.shannon/routine-overrides.json`. The source TOML is
/// authoritative for everything else.
#[derive(Debug, Clone, Default)]
pub struct RoutineOverrideStore {
    path: PathBuf,
}

impl RoutineOverrideStore {
    /// Default location: `~/.shannon/routine-overrides.json`.
    pub fn new() -> Self {
        Self {
            path: default_overrides_path(),
        }
    }

    /// Custom path (for testing).
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Load the override map from disk (empty if missing or malformed).
    pub fn load(&self) -> HashMap<String, bool> {
        if !self.path.exists() {
            return HashMap::new();
        }
        let content = match fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(_) => return HashMap::new(),
        };
        serde_json::from_str(&content).unwrap_or_default()
    }

    /// Set the override for a single routine name and persist.
    pub fn set(&self, name: &str, enabled: bool) -> Result<(), String> {
        let mut map = self.load();
        map.insert(name.to_string(), enabled);
        self.write(&map)
    }

    fn write(&self, map: &HashMap<String, bool>) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create overrides dir: {e}"))?;
        }
        let json =
            serde_json::to_string_pretty(map).map_err(|e| format!("encode overrides: {e}"))?;
        fs::write(&self.path, json).map_err(|e| format!("write overrides: {e}"))?;
        Ok(())
    }
}

fn default_overrides_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".shannon")
        .join("routine-overrides.json")
}

/// Extend [`AppState`] with scheduled-task-related handles.
impl AppState {
    /// Borrow the shared scheduled-task store.
    pub fn scheduled_task_store(&self) -> &ScheduledTaskStore {
        &self.scheduled_task_store
    }

    /// Borrow the shared runs store.
    pub fn scheduled_runs_store(&self) -> &ScheduledRunsStore {
        &self.scheduled_runs_store
    }

    /// Borrow the shared triage store.
    pub fn triage_store(&self) -> &TriageStore {
        &self.triage_store
    }

    /// Borrow the shared routine-override store.
    pub fn routine_overrides(&self) -> &RoutineOverrideStore {
        &self.routine_overrides
    }

    /// Borrow the triggered-routine registry (reloaded on demand).
    pub fn triggered_registry(&self) -> &RwLock<TriggeredRoutineRegistry> {
        &self.triggered_registry
    }
}

// ─── helpers ────────────────────────────────────────────────────────────────

/// Parse a trigger-type string ("interval" | "cron" | "webhook" | "event").
fn parse_trigger_type(s: &str) -> Option<TriggerType> {
    match s.to_lowercase().as_str() {
        "interval" => Some(TriggerType::Interval),
        "cron" => Some(TriggerType::Cron),
        "webhook" => Some(TriggerType::Webhook),
        "event" => Some(TriggerType::Event),
        _ => None,
    }
}

/// Convert a Unix timestamp to a UTC DateTime.
fn ts_to_dt(ts: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(ts, 0).unwrap_or_else(Utc::now)
}

/// Convert a [`ScheduledRun`] to a frontend-friendly [`TaskExecution`].
fn run_to_execution(run: &ScheduledRun) -> TaskExecution {
    TaskExecution {
        run_id: run.run_id.clone(),
        task_id: run.task_id.clone(),
        task_name: run.task_name.clone(),
        started_at: run.started_at.timestamp(),
        finished_at: run.finished_at.map(|t| t.timestamp()),
        status: format!("{:?}", run.status).to_lowercase(),
        error_message: run.error_message.clone(),
        cost_usd: run.cost_usd,
        token_usage: run.token_usage,
    }
}

// ─── 15 Tauri commands ──────────────────────────────────────────────────────

/// List all scheduled tasks, sorted by `created_at`.
#[tauri::command]
pub async fn list_scheduled_tasks(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ScheduledRoutine>, String> {
    state
        .scheduled_task_store()
        .list()
        .map_err(|e| e.to_string())
}

/// Create a new scheduled task and persist it.
#[tauri::command]
pub async fn create_scheduled_task(
    state: tauri::State<'_, AppState>,
    payload: CreateTaskPayload,
) -> Result<ScheduledRoutine, String> {
    let trigger_type = payload
        .trigger_type
        .as_deref()
        .and_then(parse_trigger_type)
        .unwrap_or_default();

    let mut routine = match trigger_type {
        TriggerType::Cron => {
            let cron_expr = payload
                .cron_expr
                .clone()
                .ok_or_else(|| "cron_expr is required when trigger_type=cron".to_string())?;
            ScheduledRoutine::new_cron(payload.name.clone(), payload.prompt.clone(), cron_expr)
                .map_err(|e| e.to_string())?
        }
        _ => {
            let interval_secs = payload.interval_secs.unwrap_or(3600);
            ScheduledRoutine::new(payload.name.clone(), payload.prompt.clone(), interval_secs)
        }
    };

    routine.trigger_type = trigger_type;
    if let Some(tz) = payload.timezone.as_ref() {
        routine.timezone = Some(tz.clone());
    }
    if let Some(ts) = payload.expires_at {
        routine.expires_at = Some(ts_to_dt(ts));
    }
    if let Some(max) = payload.max_fires {
        routine.max_fires = Some(max);
    }
    if payload.policy.is_some() {
        routine.policy = payload.policy.clone();
    }
    if let Some(deps) = payload.depends_on.clone() {
        routine.depends_on = deps;
    }

    state
        .scheduled_task_store()
        .save(&routine)
        .map_err(|e| e.to_string())?;
    Ok(routine)
}

/// Update an existing task. Fields not supplied are left unchanged.
#[tauri::command]
pub async fn update_scheduled_task(
    state: tauri::State<'_, AppState>,
    payload: UpdateTaskPayload,
) -> Result<ScheduledRoutine, String> {
    let store = state.scheduled_task_store();
    let mut routine = store
        .load(&payload.id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("task not found: {}", payload.id))?;

    if let Some(name) = payload.name {
        routine.name = name;
    }
    if let Some(prompt) = payload.prompt {
        routine.prompt = prompt;
    }
    if let Some(ref tt) = payload.trigger_type {
        routine.trigger_type =
            parse_trigger_type(tt).ok_or_else(|| format!("bad trigger_type: {tt}"))?;
    }
    if let Some(interval) = payload.interval_secs {
        routine.interval_secs = interval;
    }
    if let Some(ref cron) = payload.cron_expr {
        // Validate + recompute next_fire_at when the expression changes.
        shannon_core::scheduled_routines::parse_cron(cron).map_err(|e| e.to_string())?;
        routine.cron_expr = Some(cron.clone());
        routine.next_fire_at =
            compute_next_fire_utc(cron, Utc::now()).map_err(|e| e.to_string())?;
    }
    if let Some(ref tz) = payload.timezone {
        routine.timezone = if tz.is_empty() {
            None
        } else {
            Some(tz.clone())
        };
    }
    if let Some(enabled) = payload.enabled {
        routine.enabled = enabled;
    }
    if let Some(ts) = payload.expires_at {
        routine.expires_at = Some(ts_to_dt(ts));
    }
    if let Some(max) = payload.max_fires {
        routine.max_fires = Some(max);
    }
    if payload.policy.is_some() {
        routine.policy = payload.policy.clone();
    }
    if let Some(deps) = payload.depends_on.clone() {
        routine.depends_on = deps;
    }

    store.save(&routine).map_err(|e| e.to_string())?;
    Ok(routine)
}

/// Delete a task by ID (also removes its `SKILL.md` / `task.json` directory).
#[tauri::command]
pub async fn delete_scheduled_task(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    state
        .scheduled_task_store()
        .delete(&id)
        .map_err(|e| e.to_string())
}

/// Toggle a task on/off. Returns the new enabled state.
#[tauri::command]
pub async fn toggle_scheduled_task(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    let store = state.scheduled_task_store();
    let mut routine = store
        .load(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("task not found: {id}"))?;
    routine.enabled = !routine.enabled;
    let enabled = routine.enabled;
    store.save(&routine).map_err(|e| e.to_string())?;
    Ok(enabled)
}

/// Fire a task immediately, bypassing the schedule. Returns the new run_id.
///
/// Sprint 2 returns a `Running` run; the actual prompt execution wiring
/// (via `QueryEngine`) lands in Sprint 3.
#[tauri::command]
pub async fn trigger_task_now(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<TriggerResponse, String> {
    let store = state.scheduled_task_store();
    let runs = state.scheduled_runs_store();

    let routine = store
        .load(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("task not found: {id}"))?;

    let run_id = runs
        .start_run(&routine.id, &routine.name)
        .map_err(|e| e.to_string())?;

    Ok(TriggerResponse {
        run_id,
        task_id: routine.id.clone(),
        task_name: routine.name.clone(),
    })
}

/// Preview a cron expression. Returns the next 5 fire times as Unix timestamps.
#[tauri::command]
pub async fn preview_cron(expression: String) -> Result<CronPreview, String> {
    let trimmed = expression.trim();
    if trimmed.is_empty() {
        return Ok(CronPreview {
            expression,
            valid: false,
            error: Some("empty expression".to_string()),
            next_fires: Vec::new(),
        });
    }

    if let Err(e) = shannon_core::scheduled_routines::parse_cron(trimmed) {
        return Ok(CronPreview {
            expression,
            valid: false,
            error: Some(e.to_string()),
            next_fires: Vec::new(),
        });
    }

    let mut next_fires = Vec::with_capacity(5);
    let mut cursor = Utc::now();
    for _ in 0..5 {
        match compute_next_fire_utc(trimmed, cursor) {
            Ok(Some(next)) => {
                next_fires.push(next.timestamp());
                cursor = next;
            }
            _ => break,
        }
    }

    Ok(CronPreview {
        expression,
        valid: true,
        error: None,
        next_fires,
    })
}

/// List triage items matching the filter.
#[tauri::command]
pub async fn list_triage_items(
    state: tauri::State<'_, AppState>,
    filter: Option<TriageFilter>,
) -> Result<Vec<TriageItem>, String> {
    state.triage_store().list(&filter.unwrap_or_default())
}

/// Mark a triage item as read.
#[tauri::command]
pub async fn mark_triage_read(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<TriageItem, String> {
    state.triage_store().mark_read(&id)
}

/// Archive a triage item (also marks it read).
#[tauri::command]
pub async fn archive_triage_item(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<TriageItem, String> {
    state.triage_store().archive(&id)
}

/// Aggregate triage counts for sidebar badges.
#[tauri::command]
pub async fn get_triage_stats(state: tauri::State<'_, AppState>) -> Result<TriageStats, String> {
    state.triage_store().stats()
}

/// List execution records for a task, newest first.
/// When `task_id` is None, returns recent runs across all tasks.
#[tauri::command]
pub async fn list_task_executions(
    state: tauri::State<'_, AppState>,
    task_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<TaskExecution>, String> {
    let cap = limit.unwrap_or(50);
    let runs = match task_id.as_deref() {
        Some(id) if !id.is_empty() => state
            .scheduled_runs_store()
            .list_by_task(id, cap)
            .map_err(|e| e.to_string())?,
        _ => state
            .scheduled_runs_store()
            .list_recent(cap)
            .map_err(|e| e.to_string())?,
    };
    Ok(runs.iter().map(run_to_execution).collect())
}

/// Full execution detail view (lightweight run + task metadata).
#[tauri::command]
pub async fn get_execution_detail(
    state: tauri::State<'_, AppState>,
    run_id: String,
) -> Result<TaskExecutionDetail, String> {
    let run = state
        .scheduled_runs_store()
        .find_by_id(&run_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("run not found: {run_id}"))?;

    let execution = run_to_execution(&run);

    // Best-effort task enrichment — runs may outlive their tasks.
    let (prompt, cron_expr, next_fire_at) = state
        .scheduled_task_store()
        .load(&run.task_id)
        .ok()
        .flatten()
        .map(|r| {
            (
                Some(r.prompt.clone()),
                r.cron_expr.clone(),
                r.next_fire_at.map(|t| t.timestamp()),
            )
        })
        .unwrap_or((None, None, None));

    Ok(TaskExecutionDetail {
        execution,
        prompt,
        cron_expr,
        next_fire_at,
    })
}

/// List triggered routines, applying local overrides for enabled/disabled.
#[tauri::command]
pub async fn list_triggered_routines(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<TriggeredRoutineDto>, String> {
    let registry = state.triggered_registry().read().await;
    let overrides = state.routine_overrides().load();

    let mut rows: Vec<_> = registry
        .all()
        .values()
        .map(|def| TriggeredRoutineDto {
            name: def.name.clone(),
            trigger: def.trigger.clone(),
            matcher: def.matcher.clone(),
            pattern: def.pattern.clone(),
            command: def.command.clone(),
            enabled: overrides.get(&def.name).copied().unwrap_or(def.enabled),
            description: def.description.clone(),
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(rows)
}

/// Toggle a triggered routine on/off via a local override file.
///
/// The source `.shannon/routines.toml` is left untouched — overrides are
/// persisted to `~/.shannon/routine-overrides.json`.
#[tauri::command]
pub async fn toggle_triggered_routine(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<bool, String> {
    let registry = state.triggered_registry().read().await;
    let def = registry
        .all()
        .get(&name)
        .ok_or_else(|| format!("triggered routine not found: {name}"))?
        .clone();
    drop(registry);

    let overrides = state.routine_overrides().load();
    let current = overrides.get(&name).copied().unwrap_or(def.enabled);
    let next = !current;
    state.routine_overrides().set(&name, next)?;
    Ok(next)
}

/// Create a new triggered routine by appending it to `.shannon/routines.toml`.
///
/// Persists the routine to the project-local config file, then hot-reloads the
/// in-memory registry so the new routine is immediately visible to subsequent
/// `list_triggered_routines` calls. Returns the new routine as a DTO.
#[tauri::command]
pub async fn create_triggered_routine(
    state: tauri::State<'_, AppState>,
    name: String,
    trigger: String,
    command: String,
    matcher: Option<String>,
    pattern: Option<String>,
    description: Option<String>,
) -> Result<TriggeredRoutineDto, String> {
    {
        let registry = state.triggered_registry().read().await;
        if registry.all().contains_key(&name) {
            return Err(format!("triggered routine already exists: {name}"));
        }
    }

    let def = shannon_core::triggered_routines::TriggeredRoutineDef {
        name: name.clone(),
        trigger: trigger.clone(),
        matcher: matcher.filter(|s| !s.trim().is_empty()),
        pattern: pattern.filter(|s| !s.trim().is_empty()),
        command: command.clone(),
        enabled: true,
        timeout: 60,
        background: true,
        description: description.filter(|s| !s.trim().is_empty()),
    };

    append_routine_to_project_toml(&def)?;

    // Hot-reload the registry from disk so the new entry is live immediately.
    let reloaded = shannon_core::triggered_routines::TriggeredRoutineRegistry::load_from_dirs();
    *state.triggered_registry().write().await = reloaded;

    Ok(TriggeredRoutineDto {
        name: def.name,
        trigger: def.trigger,
        matcher: def.matcher,
        pattern: def.pattern,
        command: def.command,
        enabled: def.enabled,
        description: def.description,
    })
}

/// Append a `[[routine]]` block to `.shannon/routines.toml`, creating the
/// file (and parent dir) if missing.
fn append_routine_to_project_toml(
    def: &shannon_core::triggered_routines::TriggeredRoutineDef,
) -> Result<(), String> {
    let path = std::path::Path::new(".shannon").join("routines.toml");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create .shannon/: {e}"))?;
    }

    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc = existing.trim_end().to_string();
    if !doc.is_empty() {
        doc.push('\n');
    } else {
        doc.push_str("# Triggered routines — managed by shannon-desktop.\n");
    }
    doc.push_str("\n[[routine]]\n");
    doc.push_str(&format!("name = {}\n", toml_escape_string(&def.name)));
    doc.push_str(&format!("trigger = {}\n", toml_escape_string(&def.trigger)));
    if let Some(m) = &def.matcher {
        doc.push_str(&format!("matcher = {}\n", toml_escape_string(m)));
    }
    if let Some(p) = &def.pattern {
        doc.push_str(&format!("pattern = {}\n", toml_escape_string(p)));
    }
    doc.push_str(&format!("command = {}\n", toml_escape_string(&def.command)));
    doc.push_str(&format!("enabled = {}\n", def.enabled));
    doc.push_str(&format!("timeout = {}\n", def.timeout));
    doc.push_str(&format!("background = {}\n", def.background));
    if let Some(d) = &def.description {
        doc.push_str(&format!("description = {}\n", toml_escape_string(d)));
    }

    std::fs::write(&path, doc).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(())
}

/// Render a TOML basic string literal. Wraps in double quotes, escapes control
/// chars, quotes, and backslashes. Other Unicode passes through as UTF-8 — TOML
/// files are UTF-8 by spec.
fn toml_escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ─── OPC analytics (P4) ──────────────────────────────────────────────────────
//
// Aggregates over the `.claude/tasks/` tree. The 7-day time series uses the
// file mtime of each task JSON as a proxy for activity (the task schema has no
// created/updated timestamps). Completed-day buckets are derived from the
// mtime of completed tasks; created-day buckets from all task files.

/// Daily activity bucket for the OPC analytics panel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpcDayBucket {
    /// YYYY-MM-DD in local time.
    pub date: String,
    pub created: u32,
    pub completed: u32,
}

/// Per-status breakdown entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpcStatusBucket {
    pub status: String,
    pub count: u32,
}

/// Per-assignee workload entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpcAssigneeBucket {
    pub assignee: String,
    pub total: u32,
    pub done: u32,
    pub in_progress: u32,
}

/// Per-priority count.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpcPriorityBucket {
    pub priority: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpcMetrics {
    pub total: u32,
    pub completion_rate: f32,
    pub by_status: Vec<OpcStatusBucket>,
    pub by_priority: Vec<OpcPriorityBucket>,
    pub by_assignee: Vec<OpcAssigneeBucket>,
    pub daily: Vec<OpcDayBucket>,
}

const OPC_WINDOW_DAYS: usize = 7;

#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn get_opc_metrics() -> Result<OpcMetrics, String> {
    let tasks = crate::commands::list_tasks().await?;
    let daily = collect_daily_buckets()?;
    Ok(compute_opc_metrics(&tasks, daily))
}

/// Pure aggregation of per-task counts into the OPC metric buckets.
///
/// Extracted so load benchmarks can exercise it without Tauri state or a
/// filesystem walk for daily buckets — `daily` is passed in.
pub fn compute_opc_metrics(
    tasks: &[crate::commands::TaskInfo],
    daily: Vec<OpcDayBucket>,
) -> OpcMetrics {
    let total = tasks.len() as u32;

    let mut status_counts: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    let mut priority_counts: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    let mut assignee_map: std::collections::HashMap<String, (u32, u32, u32)> =
        std::collections::HashMap::new();
    let mut completed_total = 0u32;

    for t in tasks {
        *status_counts.entry(t.status.clone()).or_insert(0) += 1;
        if let Some(p) = &t.priority {
            *priority_counts.entry(p.clone()).or_insert(0) += 1;
        }
        let is_done = is_completed_status(&t.status);
        let is_inprog = is_in_progress_status(&t.status);
        if is_done {
            completed_total += 1;
        }
        if let Some(a) = &t.assignee {
            let entry = assignee_map.entry(a.clone()).or_insert((0, 0, 0));
            entry.0 += 1;
            if is_done {
                entry.1 += 1;
            }
            if is_inprog {
                entry.2 += 1;
            }
        }
    }

    let completion_rate = if total == 0 {
        0.0
    } else {
        (completed_total as f32) * 100.0 / (total as f32)
    };

    let mut by_status: Vec<OpcStatusBucket> = status_counts
        .into_iter()
        .map(|(status, count)| OpcStatusBucket { status, count })
        .collect();
    by_status.sort_by(|a, b| b.count.cmp(&a.count).then(a.status.cmp(&b.status)));

    let mut by_priority: Vec<OpcPriorityBucket> = priority_counts
        .into_iter()
        .map(|(priority, count)| OpcPriorityBucket { priority, count })
        .collect();
    by_priority.sort_by(|a, b| b.count.cmp(&a.count).then(a.priority.cmp(&b.priority)));

    let mut by_assignee: Vec<OpcAssigneeBucket> = assignee_map
        .into_iter()
        .map(|(assignee, (total, done, in_progress))| OpcAssigneeBucket {
            assignee,
            total,
            done,
            in_progress,
        })
        .collect();
    by_assignee.sort_by(|a, b| b.total.cmp(&a.total).then(a.assignee.cmp(&b.assignee)));

    OpcMetrics {
        total,
        completion_rate,
        by_status,
        by_priority,
        by_assignee,
        daily,
    }
}

fn is_completed_status(s: &str) -> bool {
    matches!(s.to_ascii_lowercase().as_str(), "completed" | "done")
}

fn is_in_progress_status(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "in_progress" | "running" | "pending"
    )
}

fn collect_daily_buckets() -> Result<Vec<OpcDayBucket>, String> {
    let tasks_dir = std::path::Path::new(".claude/tasks");
    if !tasks_dir.is_dir() {
        return Ok(empty_daily_buckets());
    }
    let canonical_root = tasks_dir
        .canonicalize()
        .map_err(|e| format!("Invalid tasks dir: {e}"))?;

    // Start of today (local) and the (OPC_WINDOW_DAYS - 1) days before it.
    let today = today_ymd_local()?;
    let mut buckets: Vec<(chrono::NaiveDate, u32, u32)> = Vec::with_capacity(OPC_WINDOW_DAYS);
    for i in 0..OPC_WINDOW_DAYS {
        let day = today - chrono::Duration::days(i as i64);
        buckets.push((day, 0, 0));
    }
    let earliest = buckets.last().map(|(d, _, _)| *d).unwrap_or(today);

    let visit = |dir: &std::path::Path,
                 buckets: &mut Vec<(chrono::NaiveDate, u32, u32)>|
     -> Result<(), String> {
        let entries =
            std::fs::read_dir(dir).map_err(|e| format!("Cannot read {}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let canonical = match path.canonicalize() {
                Ok(c) => c,
                Err(_) => continue,
            };
            if !canonical.starts_with(&canonical_root) {
                continue;
            }
            if canonical.is_dir() {
                // Recurse inline — closures can't easily recurse, so we iterate a stack.
                continue;
            }
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                let mtime = match std::fs::metadata(&path).and_then(|m| m.modified()) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let date = systemtime_to_local_naive(mtime)?;
                if date < earliest {
                    continue;
                }
                let status = read_task_status(&path).unwrap_or_default();
                let is_done = is_completed_status(&status);
                for (day, created, completed) in buckets.iter_mut() {
                    if *day == date {
                        *created += 1;
                        if is_done {
                            *completed += 1;
                        }
                        break;
                    }
                }
            }
        }
        Ok(())
    };

    // Walk with an explicit stack to avoid recursive closure limits.
    let mut stack: Vec<std::path::PathBuf> = vec![canonical_root.clone()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let canonical = match path.canonicalize() {
                Ok(c) => c,
                Err(_) => continue,
            };
            if !canonical.starts_with(&canonical_root) {
                continue;
            }
            if canonical.is_dir() {
                stack.push(canonical);
                continue;
            }
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                let mtime = match std::fs::metadata(&path).and_then(|m| m.modified()) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let date = match systemtime_to_local_naive(mtime) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                if date < earliest {
                    continue;
                }
                let status = read_task_status(&path).unwrap_or_default();
                let is_done = is_completed_status(&status);
                for (day, created, completed) in buckets.iter_mut() {
                    if *day == date {
                        *created += 1;
                        if is_done {
                            *completed += 1;
                        }
                        break;
                    }
                }
            }
        }
    }
    let _ = visit; // suppress unused closure (kept for future single-dir use)

    let mut out: Vec<OpcDayBucket> = buckets
        .into_iter()
        .map(|(date, created, completed)| OpcDayBucket {
            date: date.format("%Y-%m-%d").to_string(),
            created,
            completed,
        })
        .collect();
    // Reverse so oldest day is first in the array (left-to-right chart order).
    out.reverse();
    Ok(out)
}

fn empty_daily_buckets() -> Vec<OpcDayBucket> {
    let today = match today_ymd_local() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<OpcDayBucket> = (0..OPC_WINDOW_DAYS)
        .map(|i| {
            let day = today - chrono::Duration::days(i as i64);
            OpcDayBucket {
                date: day.format("%Y-%m-%d").to_string(),
                created: 0,
                completed: 0,
            }
        })
        .collect();
    out.reverse();
    out
}

fn today_ymd_local() -> Result<chrono::NaiveDate, String> {
    let local = chrono::Local::now();
    Ok(local.date_naive())
}

fn systemtime_to_local_naive(t: std::time::SystemTime) -> Result<chrono::NaiveDate, String> {
    let dt_local: chrono::DateTime<chrono::Local> = t.into();
    Ok(dt_local.date_naive())
}

fn read_task_status(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    v.get("status").and_then(|s| s.as_str()).map(String::from)
}

// ─── Worktree management (B9) ───────────────────────────────────────────────
//
// `ExecutionPolicy.worktree` (bool) on a ScheduledRoutine decides whether the
// runner executes the prompt in an isolated git worktree. When true, the
// scheduler calls `create_task_worktree` before launching the agent and
// `remove_task_worktree` after the run completes (or on cleanup). The worktree
// base dir defaults to `.shannon/scheduled-worktrees/` under the project root,
// matching the `/batch` command's isolation layout. Each worktree gets a
// branch `scheduled/<slug>-<id>` so review/merge mirrors the `/batch` flow.

/// DTO mirroring [`shannon_core::scheduled_worktree::ScheduledWorktree`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskWorktreeDto {
    pub task_id: String,
    pub task_name: String,
    pub path: String,
    pub branch: String,
}

impl From<shannon_core::scheduled_worktree::ScheduledWorktree> for TaskWorktreeDto {
    fn from(w: shannon_core::scheduled_worktree::ScheduledWorktree) -> Self {
        Self {
            task_id: w.task_id,
            task_name: w.task_name,
            path: w.path.to_string_lossy().into_owned(),
            branch: w.branch,
        }
    }
}

/// Create (or return existing) worktree for a scheduled task.
///
/// Looks up the task by ID to resolve its name, then delegates to
/// [`shannon_core::scheduled_worktree::create_for_task`] under the default
/// base directory. Safe to call repeatedly — returns the existing descriptor
/// if the worktree path already exists.
#[tauri::command]
pub async fn create_task_worktree(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<TaskWorktreeDto, String> {
    let task = state
        .scheduled_task_store()
        .load(&task_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("task not found: {task_id}"))?;
    let base = shannon_core::scheduled_worktree::default_base_dir();
    let wt = shannon_core::scheduled_worktree::create_for_task(&task.id, &task.name, &base)
        .map_err(|e| e.to_string())?;
    Ok(wt.into())
}

/// List all scheduled-task worktrees under the default base directory.
#[tauri::command]
pub async fn list_task_worktrees() -> Result<Vec<TaskWorktreeDto>, String> {
    let base = shannon_core::scheduled_worktree::default_base_dir();
    let list = shannon_core::scheduled_worktree::list(&base).map_err(|e| e.to_string())?;
    Ok(list.into_iter().map(Into::into).collect())
}

/// Remove a worktree by path. Uses `git worktree remove --force`, so any
/// untracked changes inside the worktree are discarded.
#[tauri::command]
pub async fn remove_task_worktree(path: String) -> Result<(), String> {
    shannon_core::scheduled_worktree::remove(std::path::Path::new(&path)).map_err(|e| e.to_string())
}

/// Remove worktrees whose task IDs are no longer in the task store.
///
/// Called by the scheduler on startup and after task deletions. Returns the
/// list of removed paths (for logging/UI feedback).
#[tauri::command]
pub async fn prune_task_worktrees(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let base = shannon_core::scheduled_worktree::default_base_dir();
    let tasks = state
        .scheduled_task_store()
        .list()
        .map_err(|e| e.to_string())?;
    let active: std::collections::HashSet<String> = tasks.iter().map(|t| t.id.clone()).collect();
    let removed = shannon_core::scheduled_worktree::prune_orphans(&base, &active)
        .map_err(|e| e.to_string())?;
    Ok(removed
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── DTO round-trips ──────────────────────────────────────────────────

    #[test]
    fn test_create_task_payload_interval_roundtrip() {
        let payload = CreateTaskPayload {
            name: "Daily Scan".into(),
            prompt: "Run security scan".into(),
            trigger_type: Some("interval".into()),
            interval_secs: Some(3600),
            cron_expr: None,
            timezone: None,
            expires_at: None,
            max_fires: None,
            policy: None,
            depends_on: None,
        };
        let json = serde_json::to_string(&payload).unwrap();
        let back: CreateTaskPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "Daily Scan");
        assert_eq!(back.interval_secs, Some(3600));
    }

    #[test]
    fn test_create_task_payload_cron_roundtrip() {
        let payload = CreateTaskPayload {
            name: "Standup".into(),
            prompt: "Run standup".into(),
            trigger_type: Some("cron".into()),
            interval_secs: None,
            cron_expr: Some("0 9 * * 1-5".into()),
            timezone: Some("America/New_York".into()),
            expires_at: None,
            max_fires: Some(260),
            policy: None,
            depends_on: None,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"cron_expr\""));
        assert!(json.contains("\"max_fires\":260"));
    }

    #[test]
    fn test_update_task_payload_all_optional_except_id() {
        let json = r#"{"id": "abc12345"}"#;
        let payload: UpdateTaskPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.id, "abc12345");
        assert!(payload.name.is_none());
        assert!(payload.enabled.is_none());
        assert!(payload.depends_on.is_none());
    }

    #[test]
    fn test_create_and_update_payloads_roundtrip_depends_on() {
        let create = CreateTaskPayload {
            name: "n".into(),
            prompt: "p".into(),
            trigger_type: None,
            interval_secs: Some(60),
            cron_expr: None,
            timezone: None,
            expires_at: None,
            max_fires: None,
            policy: None,
            depends_on: Some(vec!["dep1".into(), "dep2".into()]),
        };
        let json = serde_json::to_string(&create).unwrap();
        let back: CreateTaskPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.depends_on.as_deref(),
            Some(["dep1".into(), "dep2".into()].as_slice())
        );

        let update = UpdateTaskPayload {
            id: "abc".into(),
            name: None,
            prompt: None,
            trigger_type: None,
            interval_secs: None,
            cron_expr: None,
            timezone: None,
            enabled: None,
            expires_at: None,
            max_fires: None,
            policy: None,
            depends_on: Some(Vec::new()),
        };
        let ujson = serde_json::to_string(&update).unwrap();
        assert!(ujson.contains("\"depends_on\":[]"));
    }

    #[test]
    fn test_execution_policy_uses_budget_usd_not_monthly() {
        // Anti-regression: field must be "budget_usd", not "budget_usd_monthly".
        let policy = ExecutionPolicy {
            max_retries: 1,
            timeout_secs: 30,
            worktree: None,
            notify_on_failure: true,
            budget_usd: Some(10.0),
            auto_archive_when_empty: false,
        };
        let json = serde_json::to_string(&policy).unwrap();
        assert!(json.contains("budget_usd"));
        assert!(!json.contains("monthly"));
    }

    // ── Cron preview ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_preview_cron_valid_expression() {
        let result = preview_cron("*/5 * * * *".into()).await.unwrap();
        assert!(result.valid);
        assert!(result.error.is_none());
        assert_eq!(result.next_fires.len(), 5);
        // Strictly increasing
        for w in result.next_fires.windows(2) {
            assert!(w[0] < w[1], "next_fires not increasing: {w:?}");
        }
    }

    #[tokio::test]
    async fn test_preview_cron_invalid_expression() {
        let result = preview_cron("not a cron".into()).await.unwrap();
        assert!(!result.valid);
        assert!(result.error.is_some());
        assert!(result.next_fires.is_empty());
    }

    #[tokio::test]
    async fn test_preview_cron_empty_expression() {
        let result = preview_cron("   ".into()).await.unwrap();
        assert!(!result.valid);
        assert!(result.next_fires.is_empty());
    }

    // ── Triage store ─────────────────────────────────────────────────────

    #[test]
    fn test_triage_store_add_and_list() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

        let item = store.add("failed_run", "Run xyz failed").unwrap();
        assert_eq!(item.kind, "failed_run");
        assert!(!item.read);
        assert!(!item.archived);

        let list = store.list(&TriageFilter::default()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, item.id);
    }

    #[test]
    fn test_triage_store_mark_read_appends_revision() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

        let item = store.add("needs_review", "review me").unwrap();
        let updated = store.mark_read(&item.id).unwrap();
        assert!(updated.read);
        assert_eq!(updated.revision, 1);

        // Latest revision wins on read.
        let list = store.list(&TriageFilter::default()).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].read);
    }

    #[test]
    fn test_triage_store_archive_implies_read() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

        let item = store.add("failed_run", "fail").unwrap();
        let archived = store.archive(&item.id).unwrap();
        assert!(archived.archived);
        assert!(archived.read);
    }

    #[test]
    fn test_triage_store_filter_unread_only() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

        let a = store.add("failed_run", "a").unwrap();
        let _b = store.add("failed_run", "b").unwrap();
        store.mark_read(&a.id).unwrap();

        let filter = TriageFilter {
            unread_only: Some(true),
            ..Default::default()
        };
        let list = store.list(&filter).unwrap();
        assert_eq!(list.len(), 1);
        assert!(!list[0].read);
    }

    #[test]
    fn test_triage_store_filter_unarchived_only() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

        let a = store.add("failed_run", "a").unwrap();
        let _b = store.add("failed_run", "b").unwrap();
        store.archive(&a.id).unwrap();

        let filter = TriageFilter {
            unarchived_only: Some(true),
            ..Default::default()
        };
        let list = store.list(&filter).unwrap();
        assert_eq!(list.len(), 1);
        assert!(!list[0].archived);
    }

    #[test]
    fn test_triage_store_stats_aggregate() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

        store.add("failed_run", "a").unwrap();
        store.add("failed_run", "b").unwrap();
        let c = store.add("needs_review", "c").unwrap();
        store.archive(&c.id).unwrap();

        let stats = store.stats().unwrap();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.archived, 1);
        assert_eq!(stats.unread, 2); // a + b unread; c archived (also read)
        assert_eq!(*stats.by_kind.get("failed_run").unwrap(), 2);
        assert_eq!(*stats.by_kind.get("needs_review").unwrap(), 1);
    }

    #[test]
    fn test_triage_store_find_missing_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));
        assert!(store.find_by_id("nope").unwrap().is_none());
    }

    #[test]
    fn test_triage_store_limit_truncates() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));
        for i in 0..10 {
            store.add("failed_run", &format!("item {i}")).unwrap();
        }
        let filter = TriageFilter {
            limit: Some(3),
            ..Default::default()
        };
        let list = store.list(&filter).unwrap();
        assert_eq!(list.len(), 3);
    }

    // ── Routine overrides ───────────────────────────────────────────────

    #[test]
    fn test_routine_overrides_set_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let store = RoutineOverrideStore::with_path(tmp.path().join("overrides.json"));

        assert!(store.load().is_empty());

        store.set("lint-after-edit", false).unwrap();
        store.set("test-after-commit", true).unwrap();

        let map = store.load();
        assert_eq!(map.get("lint-after-edit"), Some(&false));
        assert_eq!(map.get("test-after-commit"), Some(&true));
    }

    #[test]
    fn test_routine_overrides_missing_file_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let store = RoutineOverrideStore::with_path(tmp.path().join("missing.json"));
        assert!(store.load().is_empty());
    }

    // ── Send+Sync invariants ────────────────────────────────────────────

    #[test]
    fn test_stores_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TriageStore>();
        assert_send_sync::<RoutineOverrideStore>();
        assert_send_sync::<ScheduledTaskStore>();
        assert_send_sync::<ScheduledRunsStore>();
    }

    #[test]
    fn test_dtos_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TriageItem>();
        assert_send_sync::<TriageStats>();
        assert_send_sync::<TaskExecution>();
        assert_send_sync::<TaskExecutionDetail>();
        assert_send_sync::<TriggeredRoutineDto>();
        assert_send_sync::<CronPreview>();
        assert_send_sync::<CreateTaskPayload>();
        assert_send_sync::<UpdateTaskPayload>();
        assert_send_sync::<TriggerResponse>();
    }

    // ── Pure helpers ────────────────────────────────────────────────────

    #[test]
    fn test_parse_trigger_type_lowercase() {
        assert_eq!(parse_trigger_type("interval"), Some(TriggerType::Interval));
        assert_eq!(parse_trigger_type("CRON"), Some(TriggerType::Cron));
        assert_eq!(parse_trigger_type("Webhook"), Some(TriggerType::Webhook));
        assert_eq!(parse_trigger_type("event"), Some(TriggerType::Event));
        assert_eq!(parse_trigger_type("bogus"), None);
    }

    #[test]
    fn test_ts_to_dt_roundtrip() {
        let now = Utc::now();
        let ts = now.timestamp();
        let back = ts_to_dt(ts);
        assert_eq!(back.timestamp(), ts);
    }

    #[test]
    fn test_run_to_execution_status_lowercase() {
        let run = ScheduledRun::start("t1", "Task One");
        let exec = run_to_execution(&run);
        assert_eq!(exec.status, "running");
        assert_eq!(exec.task_id, "t1");
        assert!(exec.finished_at.is_none());
    }

    #[test]
    fn test_run_to_execution_includes_error_when_failed() {
        use shannon_core::scheduled_runs::RunStatus;
        let mut run = ScheduledRun::start("t1", "Task");
        run.finish(RunStatus::Failed, Some("oops".into()));
        let exec = run_to_execution(&run);
        assert_eq!(exec.status, "failed");
        assert_eq!(exec.error_message.as_deref(), Some("oops"));
        assert!(exec.finished_at.is_some());
    }

    #[test]
    fn test_default_paths_under_shannon_dir() {
        assert!(default_triage_path().to_string_lossy().contains(".shannon"));
        assert!(
            default_overrides_path()
                .to_string_lossy()
                .contains(".shannon")
        );
    }

    #[test]
    fn test_cron_preview_dto_serde_roundtrip() {
        let preview = CronPreview {
            expression: "0 9 * * 1-5".into(),
            valid: true,
            error: None,
            next_fires: vec![1_700_000_000, 1_700_086_400],
        };
        let json = serde_json::to_string(&preview).unwrap();
        let back: CronPreview = serde_json::from_str(&json).unwrap();
        assert!(back.valid);
        assert_eq!(back.next_fires.len(), 2);
    }

    #[test]
    fn test_triage_item_skip_serializing_none() {
        let item = TriageItem {
            id: "abc".into(),
            task_id: None,
            task_name: None,
            run_id: None,
            kind: "failed_run".into(),
            message: "fail".into(),
            created_at: 100,
            revision: 0,
            read: false,
            archived: false,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains("task_id"));
        assert!(!json.contains("run_id"));
        assert!(json.contains("kind"));
    }

    // Smoke check that the chrono re-exports used in command bodies still work.
    #[test]
    fn test_chrono_timestamp_in_modern_era() {
        let now = Utc::now();
        assert!(now.timestamp() > 1_700_000_000, "post-2023 epoch expected");
    }

    // ── P4 helpers ──────────────────────────────────────────────────────

    #[test]
    fn test_is_completed_status_recognizes_done_variants() {
        assert!(is_completed_status("completed"));
        assert!(is_completed_status("Done"));
        assert!(is_completed_status("DONE"));
        assert!(!is_completed_status("pending"));
        assert!(!is_completed_status("in_progress"));
        assert!(!is_completed_status(""));
    }

    #[test]
    fn test_is_in_progress_status_recognizes_active_variants() {
        assert!(is_in_progress_status("in_progress"));
        assert!(is_in_progress_status("running"));
        assert!(is_in_progress_status("pending"));
        assert!(is_in_progress_status("PENDING"));
        assert!(!is_in_progress_status("completed"));
        assert!(!is_in_progress_status("done"));
    }

    #[test]
    fn test_toml_escape_string_plain() {
        assert_eq!(toml_escape_string("hello"), "\"hello\"");
    }

    #[test]
    fn test_toml_escape_string_quotes_and_backslashes() {
        assert_eq!(toml_escape_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    #[test]
    fn test_toml_escape_string_control_chars() {
        assert_eq!(toml_escape_string("a\nb\tc"), "\"a\\nb\\tc\"");
        // Other control chars become \uXXXX.
        let escaped = toml_escape_string("\x01");
        assert_eq!(escaped, "\"\\u0001\"");
    }

    #[test]
    fn test_toml_escape_string_empty() {
        assert_eq!(toml_escape_string(""), "\"\"");
    }

    #[test]
    fn test_read_task_status_extracts_field() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("task.json");
        std::fs::write(&path, r#"{"id":"abc","title":"t","status":"completed"}"#).unwrap();
        assert_eq!(read_task_status(&path).as_deref(), Some("completed"));
    }

    #[test]
    fn test_read_task_status_missing_field_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("task.json");
        std::fs::write(&path, r#"{"id":"abc"}"#).unwrap();
        assert_eq!(read_task_status(&path), None);
    }

    #[test]
    fn test_read_task_status_invalid_json_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("task.json");
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(read_task_status(&path), None);
    }

    // The next two tests change cwd, which would race under parallel test
    // execution. Serialize them with a static Mutex.
    use std::sync::Mutex;
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_collect_daily_buckets_walks_team_dirs() {
        let _guard = CWD_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        // Two task JSONs under .claude/tasks/<team>/ — both today by mtime.
        let team_dir = tmp.path().join(".claude/tasks/Default");
        std::fs::create_dir_all(&team_dir).unwrap();
        std::fs::write(
            team_dir.join("11111111-1111-1111-1111-111111111111.json"),
            r#"{"id":"11111111","status":"completed"}"#,
        )
        .unwrap();
        std::fs::write(
            team_dir.join("22222222-2222-2222-2222-222222222222.json"),
            r#"{"id":"22222222","status":"pending"}"#,
        )
        .unwrap();

        let buckets = collect_daily_buckets().unwrap();
        std::env::set_current_dir(orig).unwrap();

        // 7-day window, oldest-first ordering.
        assert_eq!(buckets.len(), OPC_WINDOW_DAYS);
        // Today is the LAST bucket (oldest-first means today at end).
        let today_bucket = buckets.last().unwrap();
        assert_eq!(today_bucket.created, 2, "both tasks counted as created");
        assert_eq!(today_bucket.completed, 1, "only the completed one");
    }

    #[test]
    fn test_collect_daily_buckets_no_tasks_dir_returns_empty_buckets() {
        let _guard = CWD_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let buckets = collect_daily_buckets().unwrap();
        std::env::set_current_dir(orig).unwrap();

        // No .claude/tasks dir — returns 7 empty buckets.
        assert_eq!(buckets.len(), OPC_WINDOW_DAYS);
        for b in &buckets {
            assert_eq!(b.created, 0);
            assert_eq!(b.completed, 0);
        }
    }

    #[test]
    fn test_append_routine_to_project_toml_creates_file() {
        let _guard = CWD_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let def = shannon_core::triggered_routines::TriggeredRoutineDef {
            name: "lint-after-edit".into(),
            trigger: "PostToolUse".into(),
            matcher: Some("Edit".into()),
            pattern: None,
            command: "just lint".into(),
            enabled: true,
            timeout: 60,
            background: true,
            description: Some("Lint after edits".into()),
        };
        append_routine_to_project_toml(&def).unwrap();

        let written = std::fs::read_to_string(tmp.path().join(".shannon/routines.toml")).unwrap();
        std::env::set_current_dir(orig).unwrap();

        assert!(written.contains("[[routine]]"));
        assert!(written.contains("name = \"lint-after-edit\""));
        assert!(written.contains("trigger = \"PostToolUse\""));
        assert!(written.contains("matcher = \"Edit\""));
        assert!(written.contains("command = \"just lint\""));
        assert!(written.contains("enabled = true"));
    }

    #[test]
    fn test_append_routine_to_project_toml_appends_to_existing() {
        let _guard = CWD_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        // Pre-existing routine in the file.
        std::fs::create_dir_all(tmp.path().join(".shannon")).unwrap();
        std::fs::write(
            tmp.path().join(".shannon/routines.toml"),
            "[[routine]]\nname = \"existing\"\ntrigger = \"PreToolUse\"\ncommand = \"echo hi\"\nenabled = true\ntimeout = 60\nbackground = true\n",
        )
        .unwrap();

        let def = shannon_core::triggered_routines::TriggeredRoutineDef {
            name: "second".into(),
            trigger: "PostToolUse".into(),
            matcher: None,
            pattern: None,
            command: "echo bye".into(),
            enabled: false,
            timeout: 30,
            background: false,
            description: None,
        };
        append_routine_to_project_toml(&def).unwrap();

        let written = std::fs::read_to_string(tmp.path().join(".shannon/routines.toml")).unwrap();
        std::env::set_current_dir(orig).unwrap();

        // Both routines present.
        assert!(written.contains("name = \"existing\""));
        assert!(written.contains("name = \"second\""));
        assert_eq!(
            written.matches("[[routine]]").count(),
            2,
            "two routine blocks"
        );
    }

    // ── parse_trigger_type ────────────────────────────────────────────

    #[test]
    fn parse_trigger_type_maps_known_kinds() {
        assert_eq!(parse_trigger_type("interval"), Some(TriggerType::Interval));
        assert_eq!(parse_trigger_type("cron"), Some(TriggerType::Cron));
        assert_eq!(parse_trigger_type("webhook"), Some(TriggerType::Webhook));
        assert_eq!(parse_trigger_type("event"), Some(TriggerType::Event));
    }

    #[test]
    fn parse_trigger_type_is_case_insensitive() {
        assert_eq!(parse_trigger_type("INTERVAL"), Some(TriggerType::Interval));
        assert_eq!(parse_trigger_type("Cron"), Some(TriggerType::Cron));
    }

    #[test]
    fn parse_trigger_type_rejects_unknown_kinds() {
        assert!(parse_trigger_type("").is_none());
        assert!(parse_trigger_type("daemon").is_none());
        assert!(parse_trigger_type("heartbeat").is_none());
    }

    // ── is_completed_status / is_in_progress_status ───────────────────

    #[test]
    fn is_completed_status_matches_completed_and_done() {
        assert!(is_completed_status("completed"));
        assert!(is_completed_status("done"));
        assert!(is_completed_status("COMPLETED"));
        assert!(is_completed_status("Done"));
        // NOTE: the helper lowercases but does not trim — callers must
        // normalize whitespace before invoking.
        assert!(!is_completed_status("  completed  "));
    }

    #[test]
    fn is_completed_status_rejects_non_terminal_states() {
        assert!(!is_completed_status("running"));
        assert!(!is_completed_status("in_progress"));
        assert!(!is_completed_status("failed"));
        assert!(!is_completed_status("pending"));
        assert!(!is_completed_status(""));
    }

    #[test]
    fn is_in_progress_status_matches_active_states() {
        assert!(is_in_progress_status("in_progress"));
        assert!(is_in_progress_status("running"));
        assert!(is_in_progress_status("pending"));
        assert!(is_in_progress_status("IN_PROGRESS"));
        assert!(is_in_progress_status("Running"));
    }

    #[test]
    fn is_in_progress_status_rejects_terminal_and_unknown() {
        assert!(!is_in_progress_status("completed"));
        assert!(!is_in_progress_status("done"));
        assert!(!is_in_progress_status("failed"));
        assert!(!is_in_progress_status("queued"));
        assert!(!is_in_progress_status(""));
    }
}
