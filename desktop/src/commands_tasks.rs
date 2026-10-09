//! Task board commands — list and update tasks stored under `.claude/tasks/`.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//!
//! R2-P1-3: the tasks root is anchored to `desktop_config.working_dir`
//! (falling back to `~`) — never the desktop process CWD, which is
//! uncontrollable (macOS GUI launches run with CWD=/, where a CWD-relative
//! `.claude/tasks` cannot even be created, so quick-created tasks failed
//! with `boardSyncFailed` forever). Reads (`list_tasks`) and writes
//! (`update_task`) resolve the SAME root so a minted card is visible to
//! the board that displays it.

use crate::commands::AppState;
use crate::config::MissionConfig;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Task info for the task board.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: String,
    pub title: String,
    pub status: String,
    pub assignee: Option<String>,
    pub priority: Option<String>,
    pub description: Option<String>,
    /// IDs of tasks this task depends on (waits on). JSON: `blockedBy`.
    #[serde(default)]
    pub blocked_by: Vec<String>,
    /// IDs of tasks that wait on this task. JSON: `blocks`.
    #[serde(default)]
    pub blocks: Vec<String>,
    /// Optional due date as unix seconds. JSON: `dueDate`.
    #[serde(default)]
    pub due_date: Option<i64>,
    /// Active form label for in-progress status. JSON: `activeForm`.
    #[serde(default)]
    pub active_form: Option<String>,
    /// Execution semantics for this task's downstream chain. JSON: `executionMode`.
    /// `serial` (default) means each task in `blocks` waits for the previous to
    /// finish. `parallel` means all `blocks` run concurrently once this completes.
    #[serde(default)]
    pub execution_mode: Option<String>,
    /// Team / session subdir name the task file lives in. Empty when the task
    /// lives at the top level of `.claude/tasks/`.
    #[serde(default)]
    pub team: Option<String>,
    /// 看板金额: ledger spend of the producing session, joined at list time
    /// from the usage ledger when `team` is a real session uuid the ledger
    /// has records for (possibly `Some(0.0)`). `None` — never an estimate —
    /// for hand-built tasks (`<adhoc>`, top-level) and sessions the ledger
    /// never saw. Additive + skipped when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// Payload for `update_task`. All fields optional except `id`.
/// Writes through to `.claude/tasks/{team}/{id}.json` (creates the file if missing).
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateTaskPayload {
    pub id: String,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub priority: Option<String>,
    pub due_date: Option<i64>,
    /// When set, writes `executionMode` to the task JSON.
    pub execution_mode: Option<String>,
    /// P1-3: when set, writes `subject` — the field `list_tasks` projects to
    /// `TaskInfo.title`. Lets a caller mint a board task through the adhoc
    /// path (`.claude/tasks/<adhoc>/{id}.json`) instead of only editing an
    /// existing one.
    pub title: Option<String>,
}

/// Resolve the base directory the task board is anchored to (R2-P1-3),
/// mirroring the custom-profiles anchor (`automation_commands.rs`):
/// `desktop_config.working_dir` when set (CLI project semantics — the same
/// `.claude/tasks` a session running in that project writes), otherwise `~`
/// (user-global). Returns an explicit error when neither is available —
/// callers must not fall back to the desktop process CWD.
///
/// Public so the `bench_opc` example can compose the same anchored walk
/// without Tauri state.
pub fn anchored_tasks_dir_base(working_dir: Option<&str>) -> Result<std::path::PathBuf, String> {
    match working_dir.map(str::trim).filter(|s| !s.is_empty()) {
        Some(dir) => Ok(std::path::PathBuf::from(dir)),
        None => dirs::home_dir()
            .ok_or_else(|| "could not resolve $HOME — cannot anchor tasks directory".to_string()),
    }
}

/// Read the `working_dir` the task commands anchor to.
pub(crate) async fn configured_working_dir(state: &State<'_, AppState>) -> Option<String> {
    state.desktop_config.read().await.working_dir.clone()
}

/// List tasks from .claude/tasks/ directory (team task system).
///
/// Recurses into team subdirectories: `.claude/tasks/{team}/{id}.json`. Also
/// accepts top-level `.json` files for backward compatibility. Parses
/// `blockedBy`, `blocks`, `dueDate`, `activeForm`, `owner`, and `priority`
/// from the JSON shape used by the Claude Code / Shannon task format.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn list_tasks(state: State<'_, AppState>) -> Result<Vec<TaskInfo>, String> {
    let tasks_dir = anchored_tasks_dir_base(configured_working_dir(&state).await.as_deref())?;
    let mut tasks = list_tasks_in(&tasks_dir.join(".claude").join("tasks"))?;
    // 看板金额: agent-session tasks are named by their session uuid, so the
    // cost joins for free from the usage ledger — a session with ledger
    // rows answers its exact spend (Some, possibly 0.0); anything else
    // (hand-built `<adhoc>`/top-level tasks, sessions the ledger never saw)
    // stays None. No estimates.
    let usage_store = state.usage_store.clone();
    attach_session_costs(&mut tasks, |session_id| {
        usage_store.session_ledger_cost(session_id)
    });
    Ok(tasks)
}

/// Join each session-owned task with its ledger spend. `spend_for` answers
/// `None` for sessions without ledger association; the join also requires
/// the task's `team` to parse as a session uuid, so hand-built tasks
/// (`<adhoc>`, top-level) never receive a cost.
pub(crate) fn attach_session_costs(
    tasks: &mut [TaskInfo],
    spend_for: impl Fn(&str) -> Option<f64>,
) {
    for task in tasks {
        let Some(team) = task.team.as_deref() else {
            continue;
        };
        if uuid::Uuid::parse_str(team).is_err() {
            continue;
        }
        if let Some(cost) = spend_for(team) {
            task.cost_usd = Some(cost);
        }
    }
}

// ---------------------------------------------------------------------------
// 使命进度 (缓期批 2): mission progress projection
// ---------------------------------------------------------------------------

/// One linked task's row in the mission progress projection. Absent data
/// stays absent (honesty contract): an id the board no longer knows reports
/// `found = false` with every optional field `None` — its state is never
/// invented, and an unassociated cost is `None`, never an estimated zero.
#[derive(Debug, Clone, Serialize)]
pub struct MissionTaskProgress {
    /// The linked task-board id (as stored in `MissionConfig::task_ids`).
    pub task_id: String,
    /// Whether [`list_tasks_in`] still knows a task with this id.
    pub found: bool,
    /// Board title (`subject`), `None` when the task is missing.
    pub title: Option<String>,
    /// Board status, `None` when the task is missing.
    pub status: Option<String>,
    /// Ledger spend via the 批 1 `cost_usd` join — only for tasks whose
    /// `team` is a session uuid the usage ledger has records for (possibly
    /// `Some(0.0)`). `None` = no honest association.
    pub cost_usd: Option<f64>,
}

/// Read model behind the `mission_progress` command (缓期批 2): the pinned
/// mission plus where each linked task stands and how much of the budget
/// the linked tasks have spent.
#[derive(Debug, Clone, Serialize)]
pub struct MissionProgress {
    pub name: String,
    /// Budget from the mission config, verbatim (`None` = no budget).
    pub budget_usd: Option<f64>,
    /// Deadline from the mission config, verbatim epoch-ms (`None` = none).
    pub deadline_ts: Option<i64>,
    /// Sum of the linked tasks' ledger costs. `None` when **no** linked
    /// task has a ledger association at all — deliberately not `Some(0.0)`,
    /// which would claim measured spend where none was ever recorded. When
    /// at least one linked task carries a cost (even `Some(0.0)`), the sum
    /// covers every costed task and uncosted ones contribute nothing.
    pub budget_used_usd: Option<f64>,
    /// One row per distinct linked task id, in `task_ids` order.
    pub tasks: Vec<MissionTaskProgress>,
}

/// Pure core of [`mission_progress`]: project a mission over the joined
/// task list (`list_tasks_in` output + [`attach_session_costs`] already
/// applied, so `cost_usd` carries the 批 1 ledger join semantics).
///
/// Duplicate ids across team directories resolve deterministically to the
/// first match in the id-sorted board order, and a linked id is projected
/// at most once (first occurrence in `task_ids`), so a duplicated link can
/// never double-count toward `budget_used_usd`.
pub(crate) fn build_mission_progress(
    mission: &MissionConfig,
    tasks: &[TaskInfo],
) -> MissionProgress {
    let mut rows: Vec<MissionTaskProgress> = Vec::new();
    let mut any_cost = false;
    let mut budget_used = 0.0;
    for task_id in &mission.task_ids {
        if rows.iter().any(|r| r.task_id == *task_id) {
            continue;
        }
        let found = tasks.iter().find(|t| t.id == *task_id);
        let cost = found.and_then(|t| t.cost_usd);
        if let Some(cost) = cost {
            any_cost = true;
            budget_used += cost;
        }
        rows.push(MissionTaskProgress {
            task_id: task_id.clone(),
            found: found.is_some(),
            title: found.map(|t| t.title.clone()),
            status: found.map(|t| t.status.clone()),
            cost_usd: cost,
        });
    }
    MissionProgress {
        name: mission.name.clone(),
        budget_usd: mission.budget_usd,
        deadline_ts: mission.deadline_ts,
        budget_used_usd: any_cost.then_some(budget_used),
        tasks: rows,
    }
}

/// 使命进度 (缓期批 2): progress projection for the configured mission.
///
/// Returns `Ok(None)` when no mission is configured — the UI hides the
/// card rather than showing an empty shell (absent stays absent). The task
/// board is read from the same anchored `.claude/tasks` root
/// [`list_tasks`] uses, and costs go through the same 批 1 ledger join, so
/// this command can never disagree with the board it mirrors.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn mission_progress(
    state: State<'_, AppState>,
) -> Result<Option<MissionProgress>, String> {
    let Some(mission) = state.desktop_config.read().await.mission.clone() else {
        return Ok(None);
    };
    let tasks_dir = anchored_tasks_dir_base(configured_working_dir(&state).await.as_deref())?;
    let mut tasks = list_tasks_in(&tasks_dir.join(".claude").join("tasks"))?;
    let usage_store = state.usage_store.clone();
    attach_session_costs(&mut tasks, |session_id| {
        usage_store.session_ledger_cost(session_id)
    });
    Ok(Some(build_mission_progress(&mission, &tasks)))
}

/// List tasks under an explicit tasks root. Path-parameterised core of
/// [`list_tasks`] so tests and the `bench_opc` example can exercise the
/// walk without Tauri state or a dependency on the desktop process CWD.
pub fn list_tasks_in(tasks_dir: &std::path::Path) -> Result<Vec<TaskInfo>, String> {
    if !tasks_dir.is_dir() {
        return Ok(Vec::new());
    }

    let canonical_root = tasks_dir
        .canonicalize()
        .map_err(|e| format!("Invalid tasks dir: {e}"))?;

    let mut tasks = Vec::new();
    collect_tasks_recursive(&canonical_root, &canonical_root, &mut tasks)?;
    tasks.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(tasks)
}

/// Recursively walk `dir`, parse any `*.json` file as a TaskInfo-like record,
/// and append to `out`. Skips symlinks pointing outside `root`. The team
/// (session subdir name) is derived from the parent directory of each file
/// relative to `root` and assigned to the parsed TaskInfo.
fn collect_tasks_recursive(
    dir: &std::path::Path,
    root: &std::path::Path,
    out: &mut Vec<TaskInfo>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Cannot read tasks dir {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let canonical = match path.canonicalize() {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !canonical.starts_with(root) {
            continue;
        }
        if canonical.is_dir() {
            // Recurse into team/session subdirectory.
            collect_tasks_recursive(&canonical, root, out)?;
            continue;
        }
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            let content = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let task: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // Derive team name from parent dir relative to root.
            // e.g. `.claude/tasks/<session-uuid>/3.json` → team = "<session-uuid>".
            // Top-level files (`.claude/tasks/3.json`) → team = None.
            let team = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .filter(|_name| {
                    // Drop when the parent IS the root.
                    path.parent()
                        .and_then(|p| p.canonicalize().ok())
                        .map(|canon_parent| canon_parent != *root)
                        .unwrap_or(true)
                })
                .map(String::from);
            if let Some(parsed) = parse_task_value(&task, team) {
                out.push(parsed);
            }
        }
    }
    Ok(())
}

/// Convert a raw JSON value (from disk) into a `TaskInfo`. Returns `None`
/// when the value lacks an `id` field. Field names follow the Shannon task
/// schema: `id`, `subject`, `status`, `owner`, `description`, `priority`,
/// `dueDate`, `activeForm`, `blocks`, `blockedBy`, `executionMode`.
fn parse_task_value(task: &serde_json::Value, team: Option<String>) -> Option<TaskInfo> {
    let id = task.get("id").and_then(|v| v.as_str())?.to_string();
    let title = task
        .get("subject")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled")
        .to_string();
    let status = task
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("pending")
        .to_string();
    let owner = task
        .get("owner")
        .and_then(|v| v.as_str())
        .map(String::from)
        .filter(|o| !o.is_empty());
    let assignee = task
        .get("assignee")
        .and_then(|v| v.as_str())
        .map(String::from)
        .filter(|o| !o.is_empty())
        .or(owner);
    let priority = task
        .get("priority")
        .and_then(|v| v.as_str())
        .map(String::from)
        .filter(|o| !o.is_empty());
    let description = task
        .get("description")
        .and_then(|v| v.as_str())
        .map(String::from);
    let active_form = task
        .get("activeForm")
        .and_then(|v| v.as_str())
        .map(String::from);
    let due_date = task
        .get("dueDate")
        .and_then(|v| v.as_i64())
        .or_else(|| task.get("due_date").and_then(|v| v.as_i64()));
    let execution_mode = task
        .get("executionMode")
        .and_then(|v| v.as_str())
        .or_else(|| task.get("execution_mode").and_then(|v| v.as_str()))
        .map(String::from)
        .filter(|o| o == "parallel" || o == "serial");
    let blocked_by = collect_string_array(task, "blockedBy")
        .into_iter()
        .chain(collect_string_array(task, "blocked_by"))
        .collect();
    let blocks = collect_string_array(task, "blocks");
    Some(TaskInfo {
        id,
        title,
        status,
        assignee,
        priority,
        description,
        blocked_by,
        blocks,
        due_date,
        active_form,
        execution_mode,
        team,
        // Never persisted on disk — joined at list time from the ledger.
        cost_usd: None,
    })
}

/// Read a JSON object field as a `Vec<String>`. Accepts arrays of strings
/// or arrays of objects with an `id` field.
fn collect_string_array(obj: &serde_json::Value, key: &str) -> Vec<String> {
    let arr = match obj.get(key).and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter()
        .filter_map(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.get("id").and_then(|i| i.as_str()).map(String::from))
        })
        .collect()
}

/// Update a task's mutable fields (status, assignee, priority, due_date) and
/// persist back to `.claude/tasks/{team}/{id}.json`. Searches all team
/// subdirectories for the matching id; if not found, creates a new file at
/// `.claude/tasks/<adhoc>/{id}.json`. Returns the updated TaskInfo.
///
/// R2-P1-3: resolves against the same anchored root [`list_tasks`] reads
/// (`working_dir/.claude/tasks`, or `~/.claude/tasks` when no working_dir is
/// set). When the root cannot even be anchored (no working_dir AND no `$HOME`)
/// the command fails with an explicit error — it never falls back to the
/// process CWD, which macOS GUI launches pin to `/`.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn update_task(
    state: State<'_, AppState>,
    payload: UpdateTaskPayload,
) -> Result<TaskInfo, String> {
    let tasks_dir = anchored_tasks_dir_base(configured_working_dir(&state).await.as_deref())?;
    update_task_in(&tasks_dir.join(".claude").join("tasks"), payload)
}

/// Update a task under an explicit tasks root. Path-parameterised core of
/// [`update_task`] so tests can exercise mint/update without depending on the
/// desktop process CWD. Creates the root when missing.
fn update_task_in(
    tasks_dir: &std::path::Path,
    payload: UpdateTaskPayload,
) -> Result<TaskInfo, String> {
    let canonical_root = match tasks_dir.canonicalize() {
        Ok(c) => c,
        Err(_) => {
            std::fs::create_dir_all(tasks_dir)
                .map_err(|e| format!("Cannot create tasks dir: {e}"))?;
            tasks_dir
                .canonicalize()
                .map_err(|e| format!("Invalid tasks dir: {e}"))?
        }
    };

    let existing = find_task_file(&canonical_root, &payload.id)?;
    let target_path = match existing {
        Some(p) => p,
        None => {
            let adhoc = canonical_root.join("<adhoc>");
            std::fs::create_dir_all(&adhoc).map_err(|e| format!("Cannot create adhoc dir: {e}"))?;
            adhoc.join(format!("{}.json", payload.id))
        }
    };

    // Read existing JSON (or start from {} if missing) so we preserve fields
    // we don't manage (e.g. activeForm, description) on write-back.
    let mut doc: serde_json::Value = std::fs::read_to_string(&target_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if doc.get("id").is_none() {
        doc["id"] = serde_json::Value::String(payload.id.clone());
    }
    // P1-3: adhoc minting — a brand-new task file must carry a `subject`, or
    // `list_tasks` renders it as "Untitled" on the board.
    if let Some(title) = payload.title {
        doc["subject"] = serde_json::Value::String(title);
    }
    if let Some(status) = payload.status {
        doc["status"] = serde_json::Value::String(status);
    }
    if let Some(assignee) = payload.assignee {
        doc["assignee"] = serde_json::Value::String(assignee);
    }
    if let Some(priority) = payload.priority {
        doc["priority"] = serde_json::Value::String(priority);
    }
    if let Some(due) = payload.due_date {
        doc["dueDate"] = serde_json::Value::Number(serde_json::Number::from(due));
    }
    if let Some(mode) = payload.execution_mode {
        if mode == "parallel" || mode == "serial" {
            doc["executionMode"] = serde_json::Value::String(mode);
        }
    }

    // Atomic write: temp file + rename.
    let serialized =
        serde_json::to_string_pretty(&doc).map_err(|e| format!("Serialize failed: {e}"))?;
    let tmp = target_path.with_extension("json.tmp");
    std::fs::write(&tmp, serialized).map_err(|e| format!("Write failed: {e}"))?;
    std::fs::rename(&tmp, &target_path).map_err(|e| format!("Rename failed: {e}"))?;

    // team is derived from path during list_tasks; not recoverable here
    // since we operate on the doc only. Pass None.
    parse_task_value(&doc, None).ok_or_else(|| "Updated task is missing id".into())
}

/// Find the JSON file for a given task id by walking the tasks root.
/// Returns the canonical path if found.
fn find_task_file(root: &std::path::Path, id: &str) -> Result<Option<std::path::PathBuf>, String> {
    let target_name = format!("{id}.json");
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let canonical = match dir.canonicalize() {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !canonical.starts_with(root) {
            continue;
        }
        let entries = match std::fs::read_dir(&canonical) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path
                .file_name()
                .map(|n| n == target_name.as_str())
                .unwrap_or(false)
            {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn payload(id: &str) -> UpdateTaskPayload {
        UpdateTaskPayload {
            id: id.to_string(),
            status: None,
            assignee: None,
            priority: None,
            due_date: None,
            execution_mode: None,
            title: None,
        }
    }

    // R2-P1-3: the anchor mirrors the profiles precedent — an explicit
    // working_dir wins (trimmed), otherwise $HOME. The desktop process CWD
    // is never consulted, so these assertions hold regardless of where the
    // test binary was launched from.
    #[test]
    fn anchored_base_prefers_working_dir_and_trims() {
        let base = anchored_tasks_dir_base(Some("  /srv/project  ")).expect("resolve");
        assert_eq!(base, std::path::PathBuf::from("/srv/project"));
    }

    #[test]
    fn anchored_base_blank_working_dir_falls_back_to_home() {
        let home = dirs::home_dir().expect("test requires $HOME");
        assert_eq!(anchored_tasks_dir_base(Some("   ")).expect("resolve"), home);
        assert_eq!(anchored_tasks_dir_base(None).expect("resolve"), home);
    }

    /// R2-P1-3 regression: quick-create (adhoc mint) must land under the
    /// given root — never relative to the process CWD.
    #[test]
    fn update_task_in_mints_adhoc_task_under_given_root() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join(".claude/tasks");

        let mut p = payload("abc123");
        p.title = Some("Ship the fix".into());
        p.status = Some("in_progress".into());
        let info = update_task_in(&root, p).expect("mint");

        assert_eq!(info.id, "abc123");
        assert_eq!(info.title, "Ship the fix");
        assert_eq!(info.status, "in_progress");
        let written = root.join("<adhoc>/abc123.json");
        assert!(written.is_file(), "adhoc file at {}", written.display());
        // The minted subject is what list_tasks_in projects back as title.
        let listed = list_tasks_in(&root).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "abc123");
        assert_eq!(listed[0].title, "Ship the fix");
    }

    #[test]
    fn update_task_in_updates_existing_team_task_and_preserves_unknown_fields() {
        let tmp = TempDir::new().unwrap();
        let team_dir = tmp.path().join(".claude/tasks/session-1");
        std::fs::create_dir_all(&team_dir).unwrap();
        std::fs::write(
            team_dir.join("t7.json"),
            r#"{"id":"t7","subject":"Existing","status":"pending","activeForm":"Working"}"#,
        )
        .unwrap();

        let mut p = payload("t7");
        p.status = Some("completed".into());
        let root = tmp.path().join(".claude/tasks");
        let info = update_task_in(&root, p).expect("update");

        assert_eq!(info.status, "completed");
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(team_dir.join("t7.json")).unwrap())
                .unwrap();
        assert_eq!(doc["status"], "completed");
        // Fields we don't manage survive the write-back.
        assert_eq!(doc["activeForm"], "Working");
        // No stray <adhoc> copy was minted for an existing task.
        assert!(!tmp.path().join(".claude/tasks/<adhoc>").exists());
    }

    #[test]
    fn list_tasks_in_returns_empty_when_root_missing() {
        let tmp = TempDir::new().unwrap();
        let listed = list_tasks_in(&tmp.path().join("nope")).expect("list");
        assert!(listed.is_empty());
    }

    // ---- 看板金额: task-board cost join ---------------------------------
    //
    // Agent-session tasks are directory-named by their session uuid, so the
    // cost joins from the usage ledger. A session the ledger has records
    // for answers its exact spend (possibly Some(0.0)); hand-built tasks
    // (`<adhoc>`, top-level) and unassociated sessions stay None — never an
    // estimate.

    fn write_task(root: &std::path::Path, team: Option<&str>, id: &str) {
        let dir = match team {
            Some(team) => root.join(team),
            None => root.to_path_buf(),
        };
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{id}.json")),
            format!(r#"{{"id":"{id}","subject":"T {id}","status":"pending"}}"#),
        )
        .unwrap();
    }

    #[test]
    fn attach_session_costs_joins_ledger_spend_for_session_tasks_only() {
        let session = uuid::Uuid::new_v4();
        let other_session = uuid::Uuid::new_v4();
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join(".claude/tasks");
        write_task(&root, Some(&session.to_string()), "1");
        write_task(&root, Some("<adhoc>"), "2");
        write_task(&root, None, "3");
        write_task(&root, Some(&other_session.to_string()), "4");

        let store = crate::commands_usage::UsageStore::with_path(tmp.path().join("usage.jsonl"));
        store
            .append(&crate::commands_usage::UsageRecord {
                timestamp_ms: 1,
                model: "m".into(),
                provider: "p".into(),
                input_tokens: 10,
                output_tokens: 5,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
                cost_usd: 0.25,
                session_id: Some(session.to_string()),
            })
            .unwrap();

        let mut tasks = list_tasks_in(&root).expect("list");
        attach_session_costs(&mut tasks, |sid| store.session_ledger_cost(sid));
        let by_id = |id: &str| tasks.iter().find(|t| t.id == id).unwrap();

        assert_eq!(
            by_id("1").cost_usd,
            Some(0.25),
            "session task joins its ledger spend"
        );
        assert_eq!(
            by_id("2").cost_usd,
            None,
            "adhoc (hand-built) task gets no cost"
        );
        assert_eq!(
            by_id("3").cost_usd,
            None,
            "top-level hand-built task gets no cost"
        );
        assert_eq!(
            by_id("4").cost_usd,
            None,
            "session the ledger never saw gets no cost (no estimate)"
        );
    }

    #[test]
    fn attach_session_costs_zero_spend_session_reports_some_zero() {
        // A session the ledger HAS records for answers Some(sum) even when
        // the records carry zero cost — distinguishable from no association.
        let session = uuid::Uuid::new_v4();
        let tmp = TempDir::new().unwrap();
        let store = crate::commands_usage::UsageStore::with_path(tmp.path().join("usage.jsonl"));
        store
            .append(&crate::commands_usage::UsageRecord {
                timestamp_ms: 1,
                model: "m".into(),
                provider: "p".into(),
                input_tokens: 10,
                output_tokens: 5,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
                cost_usd: 0.0,
                session_id: Some(session.to_string()),
            })
            .unwrap();

        let mut tasks = vec![TaskInfo {
            id: "1".into(),
            title: "T".into(),
            status: "pending".into(),
            assignee: None,
            priority: None,
            description: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            due_date: None,
            active_form: None,
            execution_mode: None,
            team: Some(session.to_string()),
            cost_usd: None,
        }];
        attach_session_costs(&mut tasks, |sid| store.session_ledger_cost(sid));
        assert_eq!(tasks[0].cost_usd, Some(0.0));
    }

    // === 使命进度 (缓期批 2): mission progress projection ===

    fn task(id: &str, status: &str, team: Option<String>, cost: Option<f64>) -> TaskInfo {
        TaskInfo {
            id: id.to_string(),
            title: format!("Task {id}"),
            status: status.to_string(),
            assignee: None,
            priority: None,
            description: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            due_date: None,
            active_form: None,
            execution_mode: None,
            team,
            cost_usd: cost,
        }
    }

    fn mission(task_ids: &[&str]) -> MissionConfig {
        MissionConfig {
            name: "Test mission".into(),
            budget_usd: Some(10.0),
            deadline_ts: Some(1_800_000_000_000),
            task_ids: task_ids.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn mission_progress_reports_found_missing_and_cost_honestly() {
        let session = uuid::Uuid::new_v4().to_string();
        let tasks = vec![
            task("1", "in_progress", Some(session.clone()), Some(1.25)),
            // "2" is a hand-built task: found on the board, no cost
            // association (never an estimated 0.0).
            task("2", "pending", Some("<adhoc>".into()), None),
            // "9" was deleted from the board: reported missing, not invented.
        ];
        let progress = build_mission_progress(&mission(&["1", "2", "9"]), &tasks);

        assert_eq!(progress.name, "Test mission");
        assert_eq!(progress.budget_usd, Some(10.0));
        assert_eq!(progress.deadline_ts, Some(1_800_000_000_000));
        assert_eq!(progress.tasks.len(), 3);

        assert!(progress.tasks[0].found);
        assert_eq!(progress.tasks[0].status.as_deref(), Some("in_progress"));
        assert_eq!(progress.tasks[0].title.as_deref(), Some("Task 1"));
        assert_eq!(progress.tasks[0].cost_usd, Some(1.25));

        assert!(progress.tasks[1].found);
        assert_eq!(progress.tasks[1].status.as_deref(), Some("pending"));
        assert_eq!(progress.tasks[1].cost_usd, None);

        assert!(!progress.tasks[2].found);
        assert_eq!(progress.tasks[2].status, None);
        assert_eq!(progress.tasks[2].title, None);
        assert_eq!(progress.tasks[2].cost_usd, None);
        assert_eq!(progress.tasks[2].task_id, "9");

        // Only the associated cost counts toward the budget.
        assert_eq!(progress.budget_used_usd, Some(1.25));
    }

    #[test]
    fn mission_progress_no_associated_costs_leaves_budget_used_absent() {
        // Honesty contract: zero associated costs must read as None —
        // Some(0.0) would claim a measured spend that was never recorded.
        let tasks = vec![task("1", "pending", Some("<adhoc>".into()), None)];
        let progress = build_mission_progress(&mission(&["1"]), &tasks);
        assert_eq!(progress.budget_used_usd, None);
    }

    #[test]
    fn mission_progress_sums_all_associated_costs_including_zero() {
        let tasks = vec![
            task("1", "pending", None, Some(0.0)),
            task("2", "done", None, Some(2.5)),
        ];
        let progress = build_mission_progress(&mission(&["1", "2"]), &tasks);
        // Some(0.0) on a linked task IS an association — the sum is Some.
        assert_eq!(progress.budget_used_usd, Some(2.5));
    }

    #[test]
    fn mission_progress_dedupes_linked_ids_and_duplicate_board_ids_are_deterministic() {
        // A duplicated link is projected once (first occurrence) so it can
        // never double-count toward the budget.
        let session = uuid::Uuid::new_v4().to_string();
        let tasks = vec![
            task("1", "a", Some(session.clone()), Some(1.0)),
            task("1", "b", Some(session.clone()), Some(1.0)),
        ];
        let progress = build_mission_progress(&mission(&["1", "1"]), &tasks);
        assert_eq!(progress.tasks.len(), 1);
        // First match in the id-sorted board order wins.
        assert_eq!(progress.tasks[0].status.as_deref(), Some("a"));
        assert_eq!(progress.budget_used_usd, Some(1.0));
    }

    #[test]
    fn mission_progress_empty_links_is_all_absent() {
        let progress = build_mission_progress(&mission(&[]), &[]);
        assert!(progress.tasks.is_empty());
        assert_eq!(progress.budget_used_usd, None);
        assert_eq!(progress.name, "Test mission");
    }
}
