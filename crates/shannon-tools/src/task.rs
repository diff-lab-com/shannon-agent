//! Task management tools
//!
//! Provides implementations for:
//! - TaskCreate: Create new tasks
//! - TaskUpdate: Update task status
//! - TaskGet: Fetch task details
//! - TaskList: List all tasks

use crate::{Tool, ToolError, ToolResult, ToolOutput};
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
