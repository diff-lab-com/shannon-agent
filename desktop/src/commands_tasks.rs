//! Task board commands — list and update tasks stored under `.claude/tasks/`.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).

use serde::{Deserialize, Serialize};

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
}

/// List tasks from .claude/tasks/ directory (team task system).
///
/// Recurses into team subdirectories: `.claude/tasks/{team}/{id}.json`. Also
/// accepts top-level `.json` files for backward compatibility. Parses
/// `blockedBy`, `blocks`, `dueDate`, `activeForm`, `owner`, and `priority`
/// from the JSON shape used by the Claude Code / Shannon task format.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn list_tasks() -> Result<Vec<TaskInfo>, String> {
    let tasks_dir = std::path::Path::new(".claude/tasks");
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
#[tauri::command]
pub async fn update_task(payload: UpdateTaskPayload) -> Result<TaskInfo, String> {
    let tasks_dir = std::path::Path::new(".claude/tasks");
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
