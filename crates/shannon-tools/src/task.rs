//! Task management tools
//!
//! Provides implementations for:
//! - TaskCreate: Create new tasks
//! - TaskUpdate: Update task status
//! - TaskGet: Fetch task details
//! - TaskList: List all tasks

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Task operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation")]
pub enum TaskOperation {
    Create(TaskCreateInput),
    Update(TaskUpdateInput),
    Get(TaskGetInput),
    List(TaskListInput),
}

/// Task status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

/// Task data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique task ID
    pub id: String,

    /// Task subject/title
    pub subject: String,

    /// Detailed description
    pub description: String,

    /// Current status
    pub status: TaskStatus,

    /// Optional agent owner
    pub owner: Option<String>,

    /// Tasks this task blocks
    pub blocks: Vec<String>,

    /// Tasks blocking this task
    pub blocked_by: Vec<String>,

    /// Optional metadata
    pub metadata: Option<HashMap<String, serde_json::Value>>,

    /// Active form for display
    pub active_form: Option<String>,
}

/// Input for creating a task
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskCreateInput {
    /// Task subject/title
    pub subject: String,

    /// Detailed description
    pub description: String,

    /// Optional active form
    pub active_form: Option<String>,

    /// Optional metadata
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Output from task creation
#[derive(Debug, Serialize)]
pub struct TaskCreateOutput {
    /// Created task
    pub task: Task,

    /// Success message
    pub message: String,
}

/// Input for updating a task
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskUpdateInput {
    /// Task ID to update
    pub task_id: String,

    /// Optional new status
    pub status: Option<TaskStatus>,

    /// Optional new owner
    pub owner: Option<String>,

    /// Optional tasks this blocks
    pub add_blocks: Option<Vec<String>>,

    /// Optional tasks blocking this
    pub add_blocked_by: Option<Vec<String>>,

    /// Optional new subject
    pub subject: Option<String>,

    /// Optional new description
    pub description: Option<String>,
}

/// Output from task update
#[derive(Debug, Serialize)]
pub struct TaskUpdateOutput {
    /// Updated task
    pub task: Task,

    /// Success message
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
    /// Task details
    pub task: Option<Task>,

    /// Whether task was found
    pub found: bool,
}

/// Input for listing tasks
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskListInput {
    /// Optional status filter
    pub status_filter: Option<TaskStatus>,

    /// Optional owner filter
    pub owner_filter: Option<String>,
}

/// Output from task list
#[derive(Debug, Serialize)]
pub struct TaskListOutput {
    /// List of tasks
    pub tasks: Vec<Task>,

    /// Total count
    pub count: usize,

    /// Status breakdown
    pub status_counts: HashMap<String, usize>,
}

/// In-memory task store (shared state)
type TaskStore = Arc<RwLock<HashMap<String, Task>>>;

/// Task tool implementation
pub struct TaskTool {
    description: String,
    store: TaskStore,
    next_id: Arc<RwLock<usize>>,
}

impl Default for TaskTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskTool {
    pub fn new() -> Self {
        Self {
            description: "Create and manage tasks for tracking work progress".to_string(),
            store: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(RwLock::new(1)),
        }
    }

    async fn create_task(&self, input: TaskCreateInput) -> Result<TaskCreateOutput, ToolError> {
        let mut id_guard = self.next_id.write().await;
        let task_id = format!("{}", *id_guard);
        *id_guard += 1;
        drop(id_guard);

        let task = Task {
            id: task_id.clone(),
            subject: input.subject,
            description: input.description,
            status: TaskStatus::Pending,
            owner: None,
            blocks: Vec::new(),
            blocked_by: Vec::new(),
            metadata: input.metadata,
            active_form: input.active_form,
        };

        {
            let mut store = self.store.write().await;
            store.insert(task_id.clone(), task.clone());
        }

        Ok(TaskCreateOutput {
            task,
            message: format!("Created task {task_id}"),
        })
    }

    async fn update_task(&self, input: TaskUpdateInput) -> Result<TaskUpdateOutput, ToolError> {
        let mut store = self.store.write().await;

        let task = store
            .get_mut(&input.task_id)
            .ok_or_else(|| ToolError::InvalidInput(format!("Task {} not found", input.task_id)))?;

        // Update fields if provided
        if let Some(status) = input.status {
            task.status = status;
        }

        if let Some(owner) = input.owner {
            task.owner = Some(owner);
        }

        if let Some(add_blocks) = input.add_blocks {
            task.blocks.extend(add_blocks);
        }

        if let Some(add_blocked_by) = input.add_blocked_by {
            task.blocked_by.extend(add_blocked_by);
        }

        if let Some(subject) = input.subject {
            task.subject = subject;
        }

        if let Some(description) = input.description {
            task.description = description;
        }

        let updated_task = task.clone();

        Ok(TaskUpdateOutput {
            task: updated_task,
            message: format!("Updated task {}", input.task_id),
        })
    }

    async fn get_task(&self, input: TaskGetInput) -> Result<TaskGetOutput, ToolError> {
        let store = self.store.read().await;
        let task = store.get(&input.task_id).cloned();

        Ok(TaskGetOutput {
            found: task.is_some(),
            task,
        })
    }

    async fn list_tasks(&self, input: TaskListInput) -> Result<TaskListOutput, ToolError> {
        let store = self.store.read().await;

        let mut tasks: Vec<Task> = store
            .values()
            .filter(|task| {
                if let Some(ref status_filter) = input.status_filter {
                    if &task.status != status_filter {
                        return false;
                    }
                }

                if let Some(ref owner_filter) = input.owner_filter {
                    if task.owner.as_ref() != Some(owner_filter) {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect();

        // Sort by ID
        tasks.sort_by(|a, b| a.id.cmp(&b.id));

        // Calculate status breakdown
        let mut status_counts: HashMap<String, usize> = HashMap::new();
        for task in store.values() {
            let status_str = format!("{:?}", task.status);
            *status_counts.entry(status_str).or_insert(0) += 1;
        }

        Ok(TaskListOutput {
            count: tasks.len(),
            tasks,
            status_counts,
        })
    }
}

#[async_trait]
impl Tool for TaskTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        // Parse operation type from input
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing operation field".to_string()))?;

        match operation {
            "Create" => {
                let create_input: TaskCreateInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid create input: {e}")))?;
                let output = self.create_task(create_input).await?;
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
            "Update" => {
                let update_input: TaskUpdateInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid update input: {e}")))?;
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
            "Get" => {
                let get_input: TaskGetInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid get input: {e}")))?;
                let task_id = get_input.task_id.clone();
                let output = self.get_task(get_input).await?;
                Ok(ToolOutput {
                    content: if output.found {
                        format!("Task found: {task_id}")
                    } else {
                        format!("Task not found: {task_id}")
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
            "List" => {
                let list_input: TaskListInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid list input: {e}")))?;
                let output = self.list_tasks(list_input).await?;
                Ok(ToolOutput {
                    content: format!("Found {} tasks", output.count),
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("count".to_string(), json!(output.count));
                        map.insert("tasks".to_string(), json!(output.tasks));
                        map
                    },
                })
            }
            _ => Err(ToolError::InvalidInput(format!(
                "Unknown operation: {operation}"
            ))),
        }
    }

    fn name(&self) -> &str {
        "Task"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "description": "Operation type",
                    "enum": ["Create", "Update", "Get", "List"]
                },
                "task_id": {
                    "type": "string",
                    "description": "Task ID"
                },
                "subject": {
                    "type": "string",
                    "description": "Task subject"
                },
                "description": {
                    "type": "string",
                    "description": "Task description"
                },
                "status": {
                    "type": "string",
                    "description": "Task status",
                    "enum": ["pending", "in_progress", "completed"]
                },
                "filter": {
                    "type": "string",
                    "description": "Filter tasks by status"
                }
            },
            "required": ["operation"]
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn create_input(subject: &str, desc: &str) -> serde_json::Value {
        json!({
            "operation": "Create",
            "subject": subject,
            "description": desc
        })
    }

    // ── TaskStatus serialization ────────────────────────────────────────

    #[test]
    fn test_task_status_serialization() {
        assert_eq!(
            serde_json::to_string(&TaskStatus::Pending).unwrap(),
            "\"pending\""
        );
        // rename_all = "lowercase" converts InProgress -> inprogress
        assert_eq!(
            serde_json::to_string(&TaskStatus::InProgress).unwrap(),
            "\"inprogress\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Completed).unwrap(),
            "\"completed\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Deleted).unwrap(),
            "\"deleted\""
        );
    }

    #[test]
    fn test_task_status_deserialization() {
        let status: TaskStatus = serde_json::from_str("\"pending\"").unwrap();
        assert_eq!(status, TaskStatus::Pending);
        let status: TaskStatus = serde_json::from_str("\"inprogress\"").unwrap();
        assert_eq!(status, TaskStatus::InProgress);
        let status: TaskStatus = serde_json::from_str("\"completed\"").unwrap();
        assert_eq!(status, TaskStatus::Completed);
        let status: TaskStatus = serde_json::from_str("\"deleted\"").unwrap();
        assert_eq!(status, TaskStatus::Deleted);
    }

    // ── Task metadata ───────────────────────────────────────────────────

    #[test]
    fn test_task_serialization_roundtrip() {
        let task = Task {
            id: "1".into(),
            subject: "Test task".into(),
            description: "A test".into(),
            status: TaskStatus::InProgress,
            owner: Some("agent-1".into()),
            blocks: vec!["2".into()],
            blocked_by: vec!["3".into()],
            metadata: Some(HashMap::from([("key".into(), json!("value"))])),
            active_form: Some("Testing".into()),
        };
        let json = serde_json::to_string(&task).unwrap();
        let parsed: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, task.id);
        assert_eq!(parsed.subject, task.subject);
        assert_eq!(parsed.status, task.status);
        assert_eq!(parsed.owner, task.owner);
        assert_eq!(parsed.blocks, task.blocks);
        assert_eq!(parsed.blocked_by, task.blocked_by);
    }

    // ── CRUD operations ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_create_task() {
        let tool = TaskTool::new();
        let output = tool
            .execute(create_input("Fix bug", "Fix the auth bug"))
            .await
            .unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("Created task"));
        assert!(output.metadata.contains_key("task"));
    }

    #[tokio::test]
    async fn test_create_multiple_tasks_increments_id() {
        let tool = TaskTool::new();
        let out1 = tool.execute(create_input("Task 1", "First")).await.unwrap();
        let out2 = tool
            .execute(create_input("Task 2", "Second"))
            .await
            .unwrap();
        let task1 = out1.metadata.get("task").unwrap().as_object().unwrap();
        let task2 = out2.metadata.get("task").unwrap().as_object().unwrap();
        let id1 = task1.get("id").unwrap().as_str().unwrap();
        let id2 = task2.get("id").unwrap().as_str().unwrap();
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn test_create_task_with_metadata() {
        let tool = TaskTool::new();
        let input = json!({
            "operation": "Create",
            "subject": "Tagged task",
            "description": "Has metadata",
            "metadata": {"priority": "high", "tags": ["urgent"]}
        });
        let output = tool.execute(input).await.unwrap();
        assert!(!output.is_error);
        let task = output.metadata.get("task").unwrap();
        let meta = task.get("metadata").unwrap().as_object().unwrap();
        assert_eq!(meta.get("priority").unwrap(), "high");
    }

    #[tokio::test]
    async fn test_get_task_found() {
        let tool = TaskTool::new();
        let created = tool.execute(create_input("Find me", "desc")).await.unwrap();
        let task_id = created
            .metadata
            .get("task")
            .unwrap()
            .get("id")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();

        let output = tool
            .execute(json!({
                "operation": "Get",
                "task_id": task_id
            }))
            .await
            .unwrap();
        assert!(output.metadata.get("found").unwrap().as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_get_task_not_found() {
        let tool = TaskTool::new();
        let output = tool
            .execute(json!({
                "operation": "Get",
                "task_id": "999"
            }))
            .await
            .unwrap();
        assert!(!output.metadata.get("found").unwrap().as_bool().unwrap());
        assert!(output.is_error);
    }

    #[tokio::test]
    async fn test_update_task_status() {
        let tool = TaskTool::new();
        let created = tool
            .execute(create_input("Update me", "desc"))
            .await
            .unwrap();
        let task_id = created
            .metadata
            .get("task")
            .unwrap()
            .get("id")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();

        let output = tool
            .execute(json!({
                "operation": "Update",
                "task_id": task_id,
                "status": "inprogress"
            }))
            .await
            .unwrap();
        assert!(!output.is_error);
        let task = output.metadata.get("task").unwrap();
        assert_eq!(task.get("status").unwrap(), "inprogress");
    }

    #[tokio::test]
    async fn test_update_task_owner() {
        let tool = TaskTool::new();
        let created = tool.execute(create_input("Own me", "desc")).await.unwrap();
        let task_id = created
            .metadata
            .get("task")
            .unwrap()
            .get("id")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();

        let output = tool
            .execute(json!({
                "operation": "Update",
                "task_id": task_id,
                "owner": "agent-1"
            }))
            .await
            .unwrap();
        let task = output.metadata.get("task").unwrap();
        assert_eq!(task.get("owner").unwrap(), "agent-1");
    }

    #[tokio::test]
    async fn test_update_task_adds_blocks() {
        let tool = TaskTool::new();
        let created = tool
            .execute(create_input("Block test", "desc"))
            .await
            .unwrap();
        let task_id = created
            .metadata
            .get("task")
            .unwrap()
            .get("id")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();

        let output = tool
            .execute(json!({
                "operation": "Update",
                "task_id": task_id,
                "add_blocks": ["2", "3"]
            }))
            .await
            .unwrap();
        let task = output.metadata.get("task").unwrap();
        let blocks = task.get("blocks").unwrap().as_array().unwrap();
        assert_eq!(blocks.len(), 2);
    }

    #[tokio::test]
    async fn test_update_task_not_found() {
        let tool = TaskTool::new();
        let result = tool
            .execute(json!({
                "operation": "Update",
                "task_id": "999",
                "status": "completed"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_tasks_empty() {
        let tool = TaskTool::new();
        let output = tool.execute(json!({"operation": "List"})).await.unwrap();
        assert!(!output.is_error);
        assert_eq!(output.metadata.get("count").unwrap(), 0);
    }

    #[tokio::test]
    async fn test_list_tasks_with_filter() {
        let tool = TaskTool::new();
        // Create two tasks
        tool.execute(create_input("Task A", "desc")).await.unwrap();
        let created = tool.execute(create_input("Task B", "desc")).await.unwrap();
        let task_id = created
            .metadata
            .get("task")
            .unwrap()
            .get("id")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        // Complete one
        tool.execute(json!({
            "operation": "Update",
            "task_id": task_id,
            "status": "completed"
        }))
        .await
        .unwrap();

        // List only pending
        let output = tool
            .execute(json!({
                "operation": "List",
                "status_filter": "pending"
            }))
            .await
            .unwrap();
        let count = output.metadata.get("count").unwrap().as_u64().unwrap();
        assert_eq!(count, 1);

        // List only completed
        let output = tool
            .execute(json!({
                "operation": "List",
                "status_filter": "completed"
            }))
            .await
            .unwrap();
        let count = output.metadata.get("count").unwrap().as_u64().unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_list_status_counts() {
        let tool = TaskTool::new();
        tool.execute(create_input("T1", "d")).await.unwrap();
        tool.execute(create_input("T2", "d")).await.unwrap();
        let output = tool.execute(json!({"operation": "List"})).await.unwrap();
        // status_counts is computed internally; verify tasks are returned
        let tasks = output.metadata.get("tasks").unwrap().as_array().unwrap();
        assert_eq!(tasks.len(), 2);
        // Both should be pending by default
        for task in tasks {
            assert_eq!(task.get("status").unwrap(), "pending");
        }
    }

    // ── Error cases ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_missing_operation_field() {
        let tool = TaskTool::new();
        let result = tool.execute(json!({"subject": "test"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unknown_operation() {
        let tool = TaskTool::new();
        let result = tool.execute(json!({"operation": "Delete"})).await;
        assert!(result.is_err());
    }

    // ── Tool trait ──────────────────────────────────────────────────────

    #[test]
    fn test_tool_name() {
        let tool = TaskTool::new();
        assert_eq!(tool.name(), "Task");
    }

    #[test]
    fn test_tool_description() {
        let tool = TaskTool::new();
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_tool_input_schema() {
        let tool = TaskTool::new();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["operation"].is_object());
    }
}
