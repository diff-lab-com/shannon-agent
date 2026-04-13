//! Task stop tool
//!
//! Stops/cancels a running task by marking it as cancelled in the task store.

use crate::todo::{TaskStore, TodoStatus};
use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

/// Input for the TaskStop tool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskStopInput {
    /// Task ID to stop/cancel
    pub task_id: String,
}

/// Output from the TaskStop tool
#[derive(Debug, Serialize)]
pub struct TaskStopOutput {
    /// Whether the task was found and stopped
    pub stopped: bool,

    /// Confirmation message
    pub message: String,
}

/// Task stop tool — cancels a running task.
pub struct TaskStopTool {
    description: String,
    task_store: TaskStore,
}

impl Default for TaskStopTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskStopTool {
    pub fn new() -> Self {
        Self {
            description: "Stop or cancel a running task by its ID".to_string(),
            task_store: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    pub fn with_store(task_store: TaskStore) -> Self {
        Self {
            description: "Stop or cancel a running task by its ID".to_string(),
            task_store,
        }
    }

    async fn stop_task(&self, input: TaskStopInput) -> Result<TaskStopOutput, ToolError> {
        let mut store = self.task_store.write().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire task store lock: {e}"))
        })?;

        let task = store
            .get_mut(&input.task_id)
            .ok_or_else(|| ToolError::InvalidInput(format!("Task {} not found", input.task_id)))?;

        let was_running = task.status == TodoStatus::InProgress;
        task.status = TodoStatus::Completed; // Mark as completed (stopped)
        task.description = format!(
            "{} [CANCELLED]",
            task.description
        );
        task.content = task.description.clone();

        let message = if was_running {
            format!("Task {} stopped successfully", input.task_id)
        } else {
            format!(
                "Task {} was not running (status: {:?}), marked as stopped",
                input.task_id, task.status
            )
        };

        Ok(TaskStopOutput {
            stopped: true,
            message,
        })
    }
}

#[async_trait]
impl Tool for TaskStopTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let stop_input: TaskStopInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid task stop input: {e}")))?;
        let task_id = stop_input.task_id.clone();
        let output = self.stop_task(stop_input).await?;

        Ok(ToolOutput {
            content: output.message,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("stopped".to_string(), json!(output.stopped));
                map.insert("task_id".to_string(), json!(task_id));
                map
            },
        })
    }

    fn name(&self) -> &str {
        "TaskStop"
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
                    "description": "Task ID to stop/cancel"
                }
            },
            "required": ["task_id"]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::todo::TodoItem;
    use uuid::Uuid;

    fn make_task(status: TodoStatus) -> TodoItem {
        TodoItem {
            task_id: Uuid::new_v4().to_string(),
            subject: "Test task".to_string(),
            description: "Original description".to_string(),
            content: "Original description".to_string(),
            status,
            active_form: None,
            metadata: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            blocked_by: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_task_stop_running() {
        let store: TaskStore = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
        let tool = TaskStopTool::with_store(store.clone());

        let task = make_task(TodoStatus::InProgress);
        let task_id = task.task_id.clone();
        store.write().unwrap().insert(task_id.clone(), task);

        let result = tool
            .stop_task(TaskStopInput { task_id: task_id.clone() })
            .await
            .unwrap();

        assert!(result.stopped);
        assert!(result.message.contains("stopped successfully"));

        // Verify the task was actually modified
        let store = store.read().unwrap();
        let task = store.get(&task_id).unwrap();
        assert_eq!(task.status, TodoStatus::Completed);
        assert!(task.description.contains("[CANCELLED]"));
    }

    #[tokio::test]
    async fn test_task_stop_not_found() {
        let store: TaskStore = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
        let tool = TaskStopTool::with_store(store);

        let result = tool
            .stop_task(TaskStopInput {
                task_id: "non-existent".to_string(),
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_task_stop_already_completed() {
        let store: TaskStore = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
        let tool = TaskStopTool::with_store(store.clone());

        let task = make_task(TodoStatus::Completed);
        let task_id = task.task_id.clone();
        store.write().unwrap().insert(task_id.clone(), task);

        let result = tool
            .stop_task(TaskStopInput { task_id: task_id.clone() })
            .await
            .unwrap();

        assert!(result.stopped);
        assert!(result.message.contains("not running"));
    }

    #[tokio::test]
    async fn test_task_stop_pending() {
        let store: TaskStore = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
        let tool = TaskStopTool::with_store(store.clone());

        let task = make_task(TodoStatus::Pending);
        let task_id = task.task_id.clone();
        store.write().unwrap().insert(task_id.clone(), task);

        let result = tool
            .stop_task(TaskStopInput { task_id: task_id.clone() })
            .await
            .unwrap();

        assert!(result.stopped);

        // Verify the task was modified
        let store = store.read().unwrap();
        let task = store.get(&task_id).unwrap();
        assert_eq!(task.status, TodoStatus::Completed);
        assert!(task.description.contains("[CANCELLED]"));
    }

    #[tokio::test]
    async fn test_task_stop_preserves_subject() {
        let store: TaskStore = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
        let tool = TaskStopTool::with_store(store.clone());

        let mut task = make_task(TodoStatus::InProgress);
        task.subject = "Important task".to_string();
        task.description = "Do important work".to_string();
        task.content = "Do important work".to_string();
        let task_id = task.task_id.clone();
        store.write().unwrap().insert(task_id.clone(), task);

        tool.stop_task(TaskStopInput { task_id: task_id.clone() })
            .await
            .unwrap();

        let store = store.read().unwrap();
        let task = store.get(&task_id).unwrap();
        // Subject should be preserved
        assert_eq!(task.subject, "Important task");
        // Description should be updated with [CANCELLED]
        assert!(task.description.contains("[CANCELLED]"));
    }

    #[tokio::test]
    async fn test_task_stop_tool_name_and_schema() {
        let tool = TaskStopTool::new();
        assert_eq!(tool.name(), "TaskStop");
        assert!(tool.description().contains("cancel") || tool.description().contains("stop"));

        let schema = tool.input_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("task_id"));

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("task_id")));
    }

    #[tokio::test]
    async fn test_task_stop_invalid_json() {
        let store: TaskStore = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
        let tool = TaskStopTool::with_store(store);

        let result = tool.execute(json!({"task_id": 123})).await;
        assert!(result.is_err());
    }
}
