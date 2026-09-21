//! Todo list management tools
//!
//! Provides implementations for:
//! - TodoWrite: Create and manage session task checklists
//! - TaskCreate: Create new tasks
//! - TaskList: List all tasks
//! - TaskUpdate: Update existing tasks
//! - TaskGet: Get details of a specific task
//!
//! Enables hierarchical task organization with persistent memory.

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Todo item status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// Enhanced Todo item with task management capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// Unique ID (UUID)
    pub task_id: String,

    /// Todo content/description
    pub content: String,

    /// Current status
    pub status: TodoStatus,

    /// Subject/title of the task
    #[serde(default)]
    pub subject: String,

    /// Detailed description
    #[serde(default)]
    pub description: String,

    /// Active form for display (e.g., "Implementing feature")
    pub active_form: Option<String>,

    /// Optional metadata
    pub metadata: Option<serde_json::Value>,

    /// ISO timestamp when task was created
    #[serde(default)]
    pub created_at: String,

    /// Tasks that block this task (dependency tracking)
    #[serde(default)]
    pub blocked_by: Vec<String>,

    /// Which surface manages this item: [`ORIGIN_TODO`] (TodoWrite plan
    /// list), [`ORIGIN_TASK`] (TaskCreate backlog), or `""` (legacy items
    /// written before the field existed — treated as todo-managed).
    /// TodoWrite's replace-semantics rewrites only the todo partition, so
    /// backlog items survive plan rewrites (P1-3).
    #[serde(default)]
    pub origin: String,
}

impl TodoItem {
    /// Create a new TodoItem with a UUID and timestamp
    pub fn new(content: String) -> Self {
        Self {
            task_id: Uuid::new_v4().to_string(),
            subject: content.clone(),
            description: content.clone(),
            content,
            status: TodoStatus::Pending,
            active_form: None,
            metadata: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            blocked_by: Vec::new(),
            origin: ORIGIN_TODO.to_string(),
        }
    }

    /// Create a new TodoItem with full details
    pub fn with_details(
        subject: String,
        description: String,
        active_form: Option<String>,
        metadata: Option<serde_json::Value>,
        blocked_by: Vec<String>,
    ) -> Self {
        Self {
            task_id: Uuid::new_v4().to_string(),
            subject,
            content: description.clone(),
            description,
            status: TodoStatus::Pending,
            active_form,
            metadata,
            created_at: chrono::Utc::now().to_rfc3339(),
            blocked_by,
            // Neutral by default: the creating surface stamps its own
            // partition (TaskCreate → "task"; TodoWrite normalizes to
            // "todo" on write).
            origin: String::new(),
        }
    }
}

/// Input for writing todos
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TodoWriteInput {
    /// The updated todo list
    pub todos: Vec<TodoItem>,
}

/// Input for creating a task
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskCreateInput {
    /// Task subject/title
    pub subject: String,

    /// Detailed description
    pub description: String,

    /// Optional active form for display
    pub active_form: Option<String>,

    /// Optional metadata
    pub metadata: Option<serde_json::Value>,
}

/// Output from task creation
#[derive(Debug, Serialize)]
pub struct TaskCreateOutput {
    /// Created task ID
    pub task_id: String,

    /// Confirmation message
    pub message: String,
}

/// Input for updating a task
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskUpdateInput {
    /// Task ID to update
    pub task_id: String,

    /// Optional new status
    pub status: Option<String>,

    /// Optional new subject
    pub subject: Option<String>,

    /// Optional new description
    pub description: Option<String>,
}

/// Output from task update
#[derive(Debug, Serialize)]
pub struct TaskUpdateOutput {
    /// Updated task
    pub task: TodoItem,

    /// Confirmation message
    pub message: String,
}

/// Input for getting a task
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskGetInput {
    /// Task ID to fetch
    pub task_id: String,
}

/// Output from task get
#[derive(Debug, Serialize)]
pub struct TaskGetOutput {
    /// Task details if found
    pub task: Option<TodoItem>,

    /// Whether task was found
    pub found: bool,
}

/// Input for listing tasks
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskListInput {
    /// Optional status filter (pending/in_progress/completed)
    pub status_filter: Option<String>,
}

/// Output from task list
#[derive(Debug, Serialize)]
pub struct TaskListOutput {
    /// List of tasks
    pub tasks: Vec<TodoItem>,

    /// Total count
    pub count: usize,
}

/// Output from writing todos
#[derive(Debug, Serialize)]
pub struct TodoWriteOutput {
    /// The todo list before the update
    pub old_todos: Vec<TodoItem>,

    /// The todo list after the update
    pub new_todos: Vec<TodoItem>,

    /// Whether verification is recommended
    pub verification_nudge_needed: Option<bool>,
}

/// Todo store (shared state)
/// Shared task store (shared across all tools).
///
/// C+D Phase 1: was joined with TodoWriteTool's session-scoped
/// `TodoStore = HashMap<session_id, Vec<TodoItem>>`; that alias and the
/// `session_id` field on TodoWriteTool were removed in favour of this
/// flat-by-task_id store. Both TodoWrite and Task* now share the same
/// map — items inserted via either surface are visible to both.
/// P1-3: each item carries an `origin` partition so TodoWrite's
/// replace-semantics never deletes Task-backlog items (and vice versa).
pub type TaskStore = Arc<RwLock<HashMap<String, TodoItem>>>;

/// Origin partition managed by `TodoWrite` (the model-facing plan list).
pub const ORIGIN_TODO: &str = "todo";
/// Origin partition managed by `TaskCreate` (the backlog surface).
pub const ORIGIN_TASK: &str = "task";

/// R1-3: one process-wide task store. TaskCreate/TaskUpdate/TaskGet/TaskList
/// all default to this global, so an item created via one tool is visible to
/// the others without explicit wiring. Persistence is layered on top: every
/// mutation re-serializes the store to disk and the next process loads it.
fn global_task_store() -> TaskStore {
    use std::sync::OnceLock;
    static STORE: OnceLock<TaskStore> = OnceLock::new();
    STORE
        .get_or_init(|| {
            let store: TaskStore = Arc::new(RwLock::new(HashMap::new()));
            if let Some(path) = todo_persist_path() {
                if let Ok(bytes) = std::fs::read(&path) {
                    if let Ok(items) = serde_json::from_slice::<Vec<TodoItem>>(&bytes) {
                        if let Ok(mut guard) = store.write() {
                            for item in items {
                                guard.insert(item.task_id.clone(), item);
                            }
                        }
                    }
                }
            }
            store
        })
        .clone()
}

/// Resolve the per-process task-persistence path
/// (`$SHANNON_HOME/todos/<fnv1a(cwd)>.json`; env override
/// `SHANNON_TODO_PERSIST=0` disables).
fn todo_persist_path() -> Option<std::path::PathBuf> {
    if std::env::var("SHANNON_TODO_PERSIST")
        .ok()
        .map(|s| s == "0")
        .unwrap_or(false)
    {
        return None;
    }
    let cwd = std::env::current_dir().ok()?;
    let hash = fnv1a_64(cwd.to_string_lossy().as_bytes());
    let home = if let Ok(custom) = std::env::var("SHANNON_HOME") {
        std::path::PathBuf::from(custom)
    } else {
        dirs::home_dir()?.join(".shannon")
    };
    Some(home.join("todos").join(format!("{hash:016x}.json")))
}

/// R1-3: render the current todo/task checklist as a markdown block for the
/// engine's reinjection path (memory/CLAUDE.md analog for tasks). Returns
/// `None` when there's nothing to show.
pub fn todo_reinjection_block() -> Option<String> {
    let store = global_task_store();
    todo_reinjection_block_for_store(&store)
}

/// Store-parameterized variant of [`todo_reinjection_block`] (tests,
/// embedders with their own store).
pub(crate) fn todo_reinjection_block_for_store(store: &TaskStore) -> Option<String> {
    let mut items: Vec<TodoItem> = Vec::new();
    if let Ok(guard) = store.read() {
        items.extend(guard.values().cloned());
    }
    if items.is_empty() {
        return None;
    }
    let mut by_status = items
        .into_iter()
        .map(|t| {
            let mark = match t.status {
                TodoStatus::Pending => " ",
                TodoStatus::InProgress => "~",
                TodoStatus::Completed => "x",
            };
            let label = if t.subject.is_empty() {
                t.content.clone()
            } else {
                t.subject.clone()
            };
            (mark, label)
        })
        .collect::<Vec<_>>();
    by_status.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut out = String::from("## Current Task List\n\n");
    for (mark, label) in by_status {
        out.push_str(&format!("- [{mark}] {label}\n"));
    }
    out.push_str("\n_Keep this list current via TodoWrite / TaskUpdate._\n");
    Some(out)
}

/// Persist the current store contents to disk (atomic tmp+rename; log+swallow
/// on error so a transient disk issue never fails a tool call).
fn persist_todo_store(store: &TaskStore) {
    let Some(path) = todo_persist_path() else {
        return;
    };
    let items = match store.read() {
        Ok(guard) => guard.values().cloned().collect::<Vec<_>>(),
        Err(_) => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = match serde_json::to_vec_pretty(&items) {
        Ok(b) => b,
        Err(_) => return,
    };
    if std::fs::write(&tmp, &bytes).is_err() {
        return;
    }
    let _ = std::fs::rename(&tmp, &path);
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Todo write tool
pub struct TodoWriteTool {
    description: String,
    /// C+D Phase 1: now backed by the global TaskStore (HashMap<String,
    /// TodoItem>) — the same store TaskCreate/TaskUpdate/TaskList use.
    /// Was a session-scoped HashMap<String, Vec<TodoItem>> with a
    /// `session_id` field as key; both removed.
    store: TaskStore,
}

/// Task create tool
pub struct TaskCreateTool {
    description: String,
    task_store: TaskStore,
    /// C+D Phase 2: when true, the tool's schema is excluded from the
    /// tools/definitions sent to the model. The tool is still callable
    /// directly (host-side code / `mcp__tool_search`-style discovery)
    /// and the description still says what it does for human readers.
    hidden_from_llm: bool,
}

/// Task list tool
pub struct TaskListTool {
    description: String,
    task_store: TaskStore,
    hidden_from_llm: bool,
}

/// Task update tool
pub struct TaskUpdateTool {
    description: String,
    task_store: TaskStore,
    hidden_from_llm: bool,
}

/// Task get tool
pub struct TaskGetTool {
    description: String,
    task_store: TaskStore,
    hidden_from_llm: bool,
}

impl Default for TodoWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoWriteTool {
    pub fn new() -> Self {
        Self::with_store(global_task_store())
    }

    /// Inject a store (tests / embedders). Production path uses the
    /// process-global store via [`TodoWriteTool::new`].
    pub fn with_store(task_store: TaskStore) -> Self {
        Self {
            description: "Create and manage a structured task checklist for the current session.\n\
\n\
Use it for multi-step work — 3 or more steps, or any task complex enough\n\
that progress could be lost track of: write the plan as items up front,\n\
keep exactly one item in_progress while working, and mark items completed\n\
as soon as they finish. Each call REPLACES the whole list, so always send\n\
the full updated set; when every item is completed the list clears. Skip \
it for single trivial actions that need no tracking."
                .to_string(),
            store: task_store,
        }
    }

    /// Write todos to the shared task store (C+D Phase 1: was a separate
    /// session-scoped HashMap; now backed by the global TaskStore so
    /// TodoWrite and Task* tools see the same items).
    ///
    /// Merge semantics (preserves the model-visible contract: "each call
    /// REPLACES the whole list"):
    ///   1. Input `todos` are upserted by `task_id`.
    ///   2. Same-partition items whose `task_id` is NOT in the input are
    ///      removed — the model dropped them from the plan (this is the
    ///      "REPLACES the whole list" contract; dropped completed items
    ///      are removed too, so the list never accumulates stale `[x]`
    ///      rows across plan rewrites).
    ///   3. Items from the OTHER partition (`origin = "task"`, created
    ///      via TaskCreate) are never touched — TodoWrite manages only
    ///      the todo partition (P1-3 review fix for cross-surface
    ///      deletion).
    ///   4. "All done" rule: when every input item is Completed, the
    ///      todo partition is cleared entirely — "when every item is
    ///      completed the list clears" (restored pre-Phase-1 contract).
    ///
    /// Known limitation (documented, accepted for a single-user terminal
    /// agent): two concurrent sessions in the SAME process share the
    /// todo partition and their writes replace each other.
    async fn write_todos(&self, input: TodoWriteInput) -> Result<TodoWriteOutput, ToolError> {
        // Normalize the origin: everything arriving through TodoWrite
        // belongs to the todo partition (legacy/empty origin included).
        let mut todos = input.todos.clone();
        for t in &mut todos {
            if t.origin.is_empty() {
                t.origin = ORIGIN_TODO.to_string();
            }
        }
        let all_done = todos.iter().all(|t| t.status == TodoStatus::Completed);
        let input_ids: std::collections::HashSet<String> =
            todos.iter().map(|t| t.task_id.clone()).collect();

        // Snapshot the managed partition before the merge (for the
        // `old_todos` output + verification nudge).
        let old_todos: Vec<TodoItem> = {
            let store = self.store.read().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {e}"))
            })?;
            store
                .values()
                .filter(|t| t.origin != ORIGIN_TASK)
                .cloned()
                .collect::<Vec<_>>()
        };

        // Apply the merge. Persistence happens AFTER the write guard is
        // dropped — persist_todo_store takes its own read lock, and
        // std::sync::RwLock is not reentrant (a write→read recursion
        // deadlocks; found by the P1-3 reproduction tests).
        if all_done {
            let mut store = self.store.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {e}"))
            })?;
            let to_remove: Vec<String> = store
                .iter()
                .filter(|(_, v)| v.origin != ORIGIN_TASK)
                .map(|(k, _)| k.clone())
                .collect();
            for k in to_remove {
                store.remove(&k);
            }
        } else {
            let mut store = self.store.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {e}"))
            })?;
            for item in &todos {
                store.insert(item.task_id.clone(), item.clone());
            }
            let to_remove: Vec<String> = store
                .iter()
                .filter(|(k, v)| v.origin != ORIGIN_TASK && !input_ids.contains(*k))
                .map(|(k, _)| k.clone())
                .collect();
            for k in to_remove {
                store.remove(&k);
            }
        }
        persist_todo_store(&self.store);

        let new_todos = input.todos.clone();

        // Verification nudge: 3+ items were completed by this write and
        // none mention verification.
        let verification_nudge_needed = if all_done && old_todos.len() >= 3 {
            let has_verification = old_todos
                .iter()
                .any(|t| t.content.to_lowercase().contains("verif"));
            !has_verification
        } else {
            false
        };

        Ok(TodoWriteOutput {
            old_todos,
            new_todos,
            verification_nudge_needed: Some(verification_nudge_needed),
        })
    }
}

#[async_trait]
impl Tool for TodoWriteTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let write_input: TodoWriteInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid todo write input: {e}")))?;
        let output = self.write_todos(write_input).await?;

        let todo_count = output.new_todos.len();
        let content = if todo_count == 0 {
            "All todos completed, list cleared".to_string()
        } else {
            let pending = output
                .new_todos
                .iter()
                .filter(|t| t.status == TodoStatus::Pending)
                .count();
            format!("Updated todo list: {todo_count} items ({pending} pending)")
        };

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("old_todos".to_string(), json!(output.old_todos));
                map.insert("new_todos".to_string(), json!(output.new_todos));
                if let Some(nudge) = output.verification_nudge_needed {
                    map.insert("verification_nudge_needed".to_string(), json!(nudge));
                }
                map
            },
        })
    }

    fn name(&self) -> &str {
        "TodoWrite"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "List of todo items",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string", "description": "Todo ID"},
                            "content": {"type": "string", "description": "Todo description"},
                            "status": {"type": "string", "description": "Todo status", "enum": ["pending", "in_progress", "completed"]}
                        },
                        "required": ["id", "content", "status"]
                    }
                }
            },
            "required": ["todos"],
            "additionalProperties": false
        })
    }
}

impl Default for TaskCreateTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskCreateTool {
    pub fn new() -> Self {
        Self {
            description: "Create a new task with subject, description, and optional metadata"
                .to_string(),
            task_store: global_task_store(),
            hidden_from_llm: false,
        }
    }

    pub fn with_store(task_store: TaskStore) -> Self {
        Self {
            description: "Create a new task with subject, description, and optional metadata"
                .to_string(),
            task_store,
            hidden_from_llm: false,
        }
    }

    /// C+D Phase 2: hide from the LLM-facing tool schema (still callable
    /// by host code / `mcp__tool_search`-style discovery).
    pub fn hidden(mut self) -> Self {
        self.hidden_from_llm = true;
        self
    }

    async fn create_task(&self, input: TaskCreateInput) -> Result<TaskCreateOutput, ToolError> {
        let mut task = TodoItem::with_details(
            input.subject,
            input.description,
            input.active_form,
            input.metadata,
            Vec::new(), // blocked_by starts empty
        );
        task.origin = ORIGIN_TASK.to_string();

        let task_id = task.task_id.clone();

        {
            let mut store = self.task_store.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire task store lock: {e}"))
            })?;
            store.insert(task_id.clone(), task);
        }
        // Persist OUTSIDE the write guard — persist_todo_store takes its
        // own read lock and std::sync::RwLock is not reentrant (P1-3
        // reproduction run deadlocked here).
        persist_todo_store(&self.task_store);

        Ok(TaskCreateOutput {
            task_id,
            message: "Task created successfully".to_string(),
        })
    }
}

#[async_trait]
impl Tool for TaskCreateTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let create_input: TaskCreateInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid task create input: {e}")))?;
        let output = self.create_task(create_input).await?;

        Ok(ToolOutput {
            content: output.message,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("task_id".to_string(), json!(output.task_id));
                map
            },
        })
    }

    fn name(&self) -> &str {
        "TaskCreate"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "subject": {
                    "type": "string",
                    "description": "Task subject/title"
                },
                "description": {
                    "type": "string",
                    "description": "Detailed description of the task"
                },
                "active_form": {
                    "type": "string",
                    "description": "Active form for display (e.g., 'Implementing feature')"
                },
                "metadata": {
                    "type": "object",
                    "description": "Optional metadata as JSON object"
                }
            },
            "required": ["subject", "description"]
        })
    }

    fn hidden_from_llm(&self) -> bool {
        self.hidden_from_llm
    }
}

impl Default for TaskListTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskListTool {
    pub fn new() -> Self {
        Self {
            description: "List all tasks with optional status filter".to_string(),
            task_store: Arc::new(RwLock::new(HashMap::new())),
            hidden_from_llm: false,
        }
    }

    pub fn with_store(task_store: TaskStore) -> Self {
        Self {
            description: "List all tasks with optional status filter".to_string(),
            task_store,
            hidden_from_llm: false,
        }
    }

    /// C+D Phase 2: hide from the LLM-facing tool schema (still callable
    /// by host code / `mcp__tool_search`-style discovery).
    pub fn hidden(mut self) -> Self {
        self.hidden_from_llm = true;
        self
    }

    async fn list_tasks(&self, input: TaskListInput) -> Result<TaskListOutput, ToolError> {
        let store = self.task_store.read().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire task store lock: {e}"))
        })?;

        let mut tasks: Vec<TodoItem> = store
            .values()
            .filter(|task| {
                if let Some(ref status_filter) = input.status_filter {
                    let task_status = match task.status {
                        TodoStatus::Pending => "pending",
                        TodoStatus::InProgress => "in_progress",
                        TodoStatus::Completed => "completed",
                    };
                    return task_status == status_filter;
                }
                true
            })
            .cloned()
            .collect();

        // Sort by creation time
        tasks.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        Ok(TaskListOutput {
            count: tasks.len(),
            tasks,
        })
    }
}

#[async_trait]
impl Tool for TaskListTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let list_input: TaskListInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid task list input: {e}")))?;
        let output = self.list_tasks(list_input).await?;

        Ok(ToolOutput {
            content: format!("Found {} task(s)", output.count),
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("count".to_string(), json!(output.count));
                map.insert("tasks".to_string(), json!(output.tasks));
                map
            },
        })
    }

    fn name(&self) -> &str {
        "TaskList"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "status_filter": {
                    "type": "string",
                    "description": "Filter tasks by status",
                    "enum": ["pending", "in_progress", "completed"]
                }
            }
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    fn hidden_from_llm(&self) -> bool {
        self.hidden_from_llm
    }
}

impl Default for TaskUpdateTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskUpdateTool {
    pub fn new() -> Self {
        Self {
            description: "Update an existing task's status, subject, or description".to_string(),
            task_store: Arc::new(RwLock::new(HashMap::new())),
            hidden_from_llm: false,
        }
    }

    pub fn with_store(task_store: TaskStore) -> Self {
        Self {
            description: "Update an existing task's status, subject, or description".to_string(),
            task_store,
            hidden_from_llm: false,
        }
    }

    /// C+D Phase 2: hide from the LLM-facing tool schema.
    pub fn hidden(mut self) -> Self {
        self.hidden_from_llm = true;
        self
    }

    async fn update_task(&self, input: TaskUpdateInput) -> Result<TaskUpdateOutput, ToolError> {
        let mut store = self.task_store.write().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire task store lock: {e}"))
        })?;

        let task = store
            .get_mut(&input.task_id)
            .ok_or_else(|| ToolError::InvalidInput(format!("Task {} not found", input.task_id)))?;

        // Update fields if provided
        if let Some(ref status_str) = input.status {
            task.status = match status_str.as_str() {
                "pending" => TodoStatus::Pending,
                "in_progress" => TodoStatus::InProgress,
                "completed" => TodoStatus::Completed,
                _ => {
                    return Err(ToolError::InvalidInput(format!(
                        "Invalid status: {status_str}. Must be pending, in_progress, or completed"
                    )));
                }
            };
        }

        if let Some(ref subject) = input.subject {
            task.subject = subject.clone();
        }

        if let Some(ref description) = input.description {
            task.description = description.clone();
            task.content = description.clone();
        }

        let updated_task = task.clone();
        // Drop the write guard BEFORE persisting — persist_todo_store
        // takes its own read lock (std::sync::RwLock is not reentrant;
        // P1-3 reproduction run deadlocked here).
        drop(store);
        persist_todo_store(&self.task_store);

        Ok(TaskUpdateOutput {
            task: updated_task,
            message: format!("Task {} updated successfully", input.task_id),
        })
    }
}

#[async_trait]
impl Tool for TaskUpdateTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let update_input: TaskUpdateInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid task update input: {e}")))?;
        let output = self.update_task(update_input).await?;

        Ok(ToolOutput {
            content: output.message,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("task".to_string(), json!(output.task));
                map
            },
        })
    }

    fn name(&self) -> &str {
        "TaskUpdate"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Task ID to update"
                },
                "status": {
                    "type": "string",
                    "description": "New status",
                    "enum": ["pending", "in_progress", "completed"]
                },
                "subject": {
                    "type": "string",
                    "description": "New subject/title"
                },
                "description": {
                    "type": "string",
                    "description": "New description"
                }
            },
            "required": ["task_id"]
        })
    }

    fn hidden_from_llm(&self) -> bool {
        self.hidden_from_llm
    }
}

impl Default for TaskGetTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskGetTool {
    pub fn new() -> Self {
        Self {
            description: "Get details of a specific task by ID".to_string(),
            task_store: Arc::new(RwLock::new(HashMap::new())),
            hidden_from_llm: false,
        }
    }

    pub fn with_store(task_store: TaskStore) -> Self {
        Self {
            description: "Get details of a specific task by ID".to_string(),
            task_store,
            hidden_from_llm: false,
        }
    }

    /// C+D Phase 2: hide from the LLM-facing tool schema.
    pub fn hidden(mut self) -> Self {
        self.hidden_from_llm = true;
        self
    }

    async fn get_task(&self, input: TaskGetInput) -> Result<TaskGetOutput, ToolError> {
        let store = self.task_store.read().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire task store lock: {e}"))
        })?;

        let task = store.get(&input.task_id).cloned();

        Ok(TaskGetOutput {
            found: task.is_some(),
            task,
        })
    }
}

#[async_trait]
impl Tool for TaskGetTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let get_input: TaskGetInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid task get input: {e}")))?;
        let output = self.get_task(get_input).await?;

        Ok(ToolOutput {
            content: if output.found {
                "Task found".to_string()
            } else {
                "Task not found".to_string()
            },
            is_error: !output.found,
            metadata: {
                let mut map = HashMap::new();
                map.insert("found".to_string(), json!(output.found));
                if let Some(task) = output.task {
                    map.insert("task".to_string(), json!(task));
                }
                map
            },
        })
    }

    fn name(&self) -> &str {
        "TaskGet"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Task ID to fetch"
                }
            },
            "required": ["task_id"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    fn hidden_from_llm(&self) -> bool {
        self.hidden_from_llm
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn create_test_todo_item(content: &str, status: TodoStatus) -> TodoItem {
        TodoItem {
            task_id: Uuid::new_v4().to_string(),
            subject: content.to_string(),
            description: content.to_string(),
            content: content.to_string(),
            status,
            active_form: None,
            metadata: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            blocked_by: Vec::new(),
            origin: ORIGIN_TODO.to_string(),
        }
    }

    #[test]
    fn test_create_test_todo_item_helper() {
        let item = create_test_todo_item("Review code", TodoStatus::Pending);
        assert_eq!(item.subject, "Review code");
        assert_eq!(item.status, TodoStatus::Pending);
        assert!(Uuid::parse_str(&item.task_id).is_ok());

        let in_progress = create_test_todo_item("Fix bug", TodoStatus::InProgress);
        assert_eq!(in_progress.status, TodoStatus::InProgress);
    }

    #[test]
    fn test_todo_item_new() {
        let item = TodoItem::new("Test task".to_string());
        assert_eq!(item.subject, "Test task");
        assert_eq!(item.description, "Test task");
        assert_eq!(item.status, TodoStatus::Pending);
        assert!(Uuid::parse_str(&item.task_id).is_ok());
        assert!(item.active_form.is_none());
        assert!(item.metadata.is_none());
        assert!(item.blocked_by.is_empty());
    }

    #[test]
    fn test_todo_item_with_details() {
        let metadata = json!({"priority": "high", "assignee": "alice"});
        let item = TodoItem::with_details(
            "Feature implementation".to_string(),
            "Implement the new feature".to_string(),
            Some("Implementing feature".to_string()),
            Some(metadata.clone()),
            vec!["task-1".to_string(), "task-2".to_string()],
        );

        assert_eq!(item.subject, "Feature implementation");
        assert_eq!(item.description, "Implement the new feature");
        assert_eq!(item.active_form, Some("Implementing feature".to_string()));
        assert_eq!(item.metadata, Some(metadata));
        assert_eq!(
            item.blocked_by,
            vec!["task-1".to_string(), "task-2".to_string()]
        );
        assert!(Uuid::parse_str(&item.task_id).is_ok());
    }

    // ── P1-3 review fixes: TodoWrite merge semantics ────────────────────

    /// Build a TodoWriteTool against an injected store (isolated from the
    /// process global).
    fn todowrite_on(store: TaskStore) -> TodoWriteTool {
        TodoWriteTool::with_store(store)
    }

    fn mk_item(content: &str, status: TodoStatus) -> TodoItem {
        let mut t = TodoItem::new(content.to_string());
        t.status = status;
        t
    }

    fn store_ids(store: &TaskStore) -> Vec<String> {
        store
            .read()
            .expect("store lock")
            .keys()
            .cloned()
            .collect()
    }

    /// Contract: "Each call REPLACES the whole list." A new plan whose
    /// input drops previously-completed items must remove them — the
    /// list must not accumulate stale `[x]` rows across plan rewrites.
    #[tokio::test]
    async fn todowrite_replaces_dropped_completed_items() {
        let store: TaskStore = Arc::new(RwLock::new(HashMap::new()));
        let tool = todowrite_on(store.clone());

        let a = mk_item("A", TodoStatus::Completed);
        let b = mk_item("B", TodoStatus::Completed);
        tool.write_todos(TodoWriteInput { todos: vec![a.clone(), b.clone()] })
            .await
            .expect("first write");

        // New plan: one fresh pending item; A and B are dropped.
        let c = mk_item("C", TodoStatus::Pending);
        tool.write_todos(TodoWriteInput { todos: vec![c.clone()] })
            .await
            .expect("second write");

        let ids = store_ids(&store);
        assert_eq!(
            ids,
            vec![c.task_id.clone()],
            "new plan must replace the list — stale completed items must not survive"
        );
    }

    /// Contract: "when every item is completed the list clears."
    #[tokio::test]
    async fn todowrite_all_done_clears_the_list() {
        let store: TaskStore = Arc::new(RwLock::new(HashMap::new()));
        let tool = todowrite_on(store.clone());

        let a = mk_item("A", TodoStatus::Pending);
        let b = mk_item("B", TodoStatus::Pending);
        tool.write_todos(TodoWriteInput { todos: vec![a, b] })
            .await
            .expect("seed");

        let a_done = mk_item("A", TodoStatus::Completed);
        let b_done = mk_item("B", TodoStatus::Completed);
        // Re-assert the SAME ids so the write reads as "these are done".
        let mut done = vec![a_done, b_done];
        // (ids are regenerated by mk_item; align them to the seeded ids.)
        let seeded: Vec<String> = {
            let guard = store.read().expect("store lock");
            guard.keys().cloned().collect()
        };
        done[0].task_id = seeded[0].clone();
        done[1].task_id = seeded[1].clone();
        tool.write_todos(TodoWriteInput { todos: done })
            .await
            .expect("complete");

        assert!(
            store_ids(&store).is_empty(),
            "all_done must clear the todo list"
        );
    }

    /// Cross-surface safety: items created via TaskCreate (origin "task")
    /// must survive a TodoWrite plan rewrite — TodoWrite manages only the
    /// todo partition. The reinjection block still shows both.
    #[tokio::test]
    async fn todowrite_does_not_delete_task_origin_items() {
        let store: TaskStore = Arc::new(RwLock::new(HashMap::new()));

        // Seed a task-backlog item (origin "task") via TaskCreateTool.
        let create = TaskCreateTool::with_store(store.clone());
        create
            .execute(json!({
                "subject": "Backlog item",
                "description": "Backlog item"
            }))
            .await
            .expect("task create");
        let task_ids = store_ids(&store);
        assert_eq!(task_ids.len(), 1, "seed failed");

        // TodoWrite with a fresh plan must not touch it.
        let tool = todowrite_on(store.clone());
        let x = mk_item("Plan step", TodoStatus::Pending);
        tool.write_todos(TodoWriteInput { todos: vec![x] })
            .await
            .expect("write");

        let ids = store_ids(&store);
        assert_eq!(ids.len(), 2, "todo + task items must coexist");
        assert!(
            ids.contains(&task_ids[0]),
            "TaskCreate item must survive TodoWrite rewrite"
        );
        // Reinjection still surfaces both surfaces' items.
        let block = todo_reinjection_block_for_store(&store)
            .expect("populated store yields block");
        assert!(block.contains("Plan step") && block.contains("Backlog item"));
    }

    #[tokio::test]
    async fn test_task_create_tool() {
        let tool = TaskCreateTool::new();
        let input = TaskCreateInput {
            subject: "Test task".to_string(),
            description: "Test description".to_string(),
            active_form: Some("Testing".to_string()),
            metadata: Some(json!({"key": "value"})),
        };

        let result = tool.create_task(input).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(Uuid::parse_str(&output.task_id).is_ok());
        assert_eq!(output.message, "Task created successfully");
    }

    #[tokio::test]
    async fn test_task_list_tool() {
        let tool = TaskListTool::new();

        // Create some test tasks
        let task1 = TodoItem::with_details(
            "Task 1".to_string(),
            "Description 1".to_string(),
            None,
            None,
            Vec::new(),
        );
        let mut task2 = TodoItem::with_details(
            "Task 2".to_string(),
            "Description 2".to_string(),
            None,
            None,
            Vec::new(),
        );
        task2.status = TodoStatus::Completed;

        {
            let mut store = tool.task_store.write().unwrap_or_else(|e| e.into_inner());
            store.insert(task1.task_id.clone(), task1);
            store.insert(task2.task_id.clone(), task2);
        }

        // List all tasks
        let result = tool
            .list_tasks(TaskListInput {
                status_filter: None,
            })
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.count, 2);

        // Filter by pending status
        let result = tool
            .list_tasks(TaskListInput {
                status_filter: Some("pending".to_string()),
            })
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.count, 1);
    }

    #[tokio::test]
    async fn test_task_update_tool() {
        let tool = TaskUpdateTool::new();

        // Create a test task
        let task = TodoItem::with_details(
            "Original subject".to_string(),
            "Original description".to_string(),
            None,
            None,
            Vec::new(),
        );
        let task_id = task.task_id.clone();

        {
            let mut store = tool.task_store.write().unwrap_or_else(|e| e.into_inner());
            store.insert(task_id.clone(), task);
        }

        // Update the task
        let result = tool
            .update_task(TaskUpdateInput {
                task_id: task_id.clone(),
                status: Some("in_progress".to_string()),
                subject: Some("Updated subject".to_string()),
                description: Some("Updated description".to_string()),
            })
            .await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.task.status, TodoStatus::InProgress);
        assert_eq!(output.task.subject, "Updated subject");
        assert_eq!(output.task.description, "Updated description");
    }

    #[tokio::test]
    async fn test_task_update_not_found() {
        let tool = TaskUpdateTool::new();

        let result = tool
            .update_task(TaskUpdateInput {
                task_id: "non-existent-id".to_string(),
                status: Some("in_progress".to_string()),
                subject: None,
                description: None,
            })
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn test_task_update_invalid_status() {
        let tool = TaskUpdateTool::new();

        // Create a test task
        let task = TodoItem::with_details(
            "Test".to_string(),
            "Description".to_string(),
            None,
            None,
            Vec::new(),
        );
        let task_id = task.task_id.clone();

        {
            let mut store = tool.task_store.write().unwrap_or_else(|e| e.into_inner());
            store.insert(task_id.clone(), task);
        }

        // Try to update with invalid status
        let result = tool
            .update_task(TaskUpdateInput {
                task_id,
                status: Some("invalid_status".to_string()),
                subject: None,
                description: None,
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_task_get_tool() {
        let tool = TaskGetTool::new();

        // Create a test task
        let task = TodoItem::with_details(
            "Test task".to_string(),
            "Test description".to_string(),
            Some("Testing".to_string()),
            Some(json!({"key": "value"})),
            vec!["blocker-1".to_string()],
        );
        let task_id = task.task_id.clone();

        {
            let mut store = tool.task_store.write().unwrap_or_else(|e| e.into_inner());
            store.insert(task_id.clone(), task);
        }

        // Get the task
        let result = tool
            .get_task(TaskGetInput {
                task_id: task_id.clone(),
            })
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.found);
        assert!(output.task.is_some());
        let retrieved_task = output.task.unwrap();
        assert_eq!(retrieved_task.subject, "Test task");
        assert_eq!(retrieved_task.active_form, Some("Testing".to_string()));
        assert_eq!(retrieved_task.blocked_by, vec!["blocker-1".to_string()]);
    }

    #[tokio::test]
    async fn test_task_get_not_found() {
        let tool = TaskGetTool::new();

        let result = tool
            .get_task(TaskGetInput {
                task_id: "non-existent-id".to_string(),
            })
            .await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.found);
        assert!(output.task.is_none());
    }

    #[test]
    fn test_todo_status_serialization() {
        // Test that TodoStatus serializes correctly
        let pending = TodoStatus::Pending;
        let serialized = serde_json::to_string(&pending).unwrap();
        assert_eq!(serialized, "\"pending\"");

        let in_progress = TodoStatus::InProgress;
        let serialized = serde_json::to_string(&in_progress).unwrap();
        assert_eq!(serialized, "\"in_progress\"");

        let completed = TodoStatus::Completed;
        let serialized = serde_json::to_string(&completed).unwrap();
        assert_eq!(serialized, "\"completed\"");
    }

    #[test]
    fn test_todo_status_deserialization() {
        // Test that TodoStatus deserializes correctly
        let pending: TodoStatus = serde_json::from_str("\"pending\"").unwrap();
        assert_eq!(pending, TodoStatus::Pending);

        let in_progress: TodoStatus = serde_json::from_str("\"in_progress\"").unwrap();
        assert_eq!(in_progress, TodoStatus::InProgress);

        let completed: TodoStatus = serde_json::from_str("\"completed\"").unwrap();
        assert_eq!(completed, TodoStatus::Completed);
    }

    #[tokio::test]
    async fn test_shared_task_store() {
        // Create a shared store
        let store: TaskStore = Arc::new(RwLock::new(HashMap::new()));

        // Create tools with the shared store
        let create_tool = TaskCreateTool::with_store(store.clone());
        let list_tool = TaskListTool::with_store(store.clone());
        let update_tool = TaskUpdateTool::with_store(store.clone());
        let get_tool = TaskGetTool::with_store(store.clone());

        // Create a task
        let create_result = create_tool
            .create_task(TaskCreateInput {
                subject: "Shared task".to_string(),
                description: "Description".to_string(),
                active_form: None,
                metadata: None,
            })
            .await
            .unwrap();
        let task_id = create_result.task_id;

        // List tasks should show 1 task
        let list_result = list_tool
            .list_tasks(TaskListInput {
                status_filter: None,
            })
            .await
            .unwrap();
        assert_eq!(list_result.count, 1);

        // Update the task
        let _update_result = update_tool
            .update_task(TaskUpdateInput {
                task_id: task_id.clone(),
                status: Some("in_progress".to_string()),
                subject: None,
                description: None,
            })
            .await
            .unwrap();

        // Get the task and verify the update
        let get_result = get_tool
            .get_task(TaskGetInput {
                task_id: task_id.clone(),
            })
            .await
            .unwrap();
        assert!(get_result.found);
        assert_eq!(get_result.task.unwrap().status, TodoStatus::InProgress);
    }

    #[test]
    fn test_task_create_input_validation() {
        // Test that required fields are present in input schema
        let tool = TaskCreateTool::new();
        let schema = tool.input_schema();

        let properties = schema.get("properties").unwrap().as_object().unwrap();
        let required = schema.get("required").unwrap().as_array().unwrap();

        assert!(properties.contains_key("subject"));
        assert!(properties.contains_key("description"));
        assert!(properties.contains_key("active_form"));
        assert!(properties.contains_key("metadata"));

        assert!(required.contains(&serde_json::json!("subject")));
        assert!(required.contains(&serde_json::json!("description")));
        assert!(!required.contains(&serde_json::json!("active_form")));
        assert!(!required.contains(&serde_json::json!("metadata")));
    }

    /// R1-3: an item inserted into the task store is visible via the
    /// reinjection helper (so a compacted session re-receives its
    /// checklist) and an empty store yields `None`. Uses an injected
    /// store — the process-global singleton is shared by parallel tests
    /// (P1-3 review fix: the old version raced on the global store and
    /// flaked when TaskCreate tests ran concurrently).
    #[test]
    fn reinjection_block_renders_tasks_and_returns_none_when_empty() {
        let store: TaskStore = Arc::new(RwLock::new(HashMap::new()));
        // Empty store => None.
        assert!(
            todo_reinjection_block_for_store(&store).is_none(),
            "empty store must not produce a reinjection block"
        );
        // Seed two items and re-render.
        {
            let mut guard = store.write().expect("store lock");
            guard.insert(
                "task-1".to_string(),
                TodoItem::with_details(
                    "First task".to_string(),
                    "First task".to_string(),
                    None,
                    None,
                    Vec::new(),
                ),
            );
            guard.insert(
                "task-2".to_string(),
                TodoItem::with_details(
                    "Second task".to_string(),
                    "Second task".to_string(),
                    None,
                    None,
                    Vec::new(),
                ),
            );
        }
        let block = todo_reinjection_block_for_store(&store)
            .expect("populated store yields a block");
        assert!(block.contains("## Current Task List"));
        assert!(block.contains("First task"));
        assert!(block.contains("Second task"));
    }
}
