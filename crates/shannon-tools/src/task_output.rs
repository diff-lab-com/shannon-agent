//! Task output tool
//!
//! Gets the output/result of a task execution from the task store.

use crate::todo::{TaskStore, TodoItem, TodoStatus};
use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

/// Input for the TaskOutput tool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskOutputInput {
    /// Task ID to get output from
    pub task_id: String,

    /// Timeout in milliseconds for blocking wait (default 30000)
    pub timeout: Option<u64>,

    /// Whether to block until output is available (default true)
    pub block: Option<bool>,
}

/// Output from the TaskOutput tool
#[derive(Debug, Serialize)]
pub struct TaskOutputOutput {
    /// Whether the task was found
    pub found: bool,

    /// The task details if found
    pub task: Option<TodoItem>,

    /// The task's output/content if available
    pub output: Option<String>,

    /// Message describing the result
    pub message: String,
}

/// Task output tool — retrieves the output/result of a task.
pub struct TaskOutputTool {
    description: String,
    task_store: TaskStore,
}

impl Default for TaskOutputTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskOutputTool {
    pub fn new() -> Self {
        Self {
            description: "Get the output/result of a task execution by its ID".to_string(),
            task_store: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    pub fn with_store(task_store: TaskStore) -> Self {
        Self {
            description: "Get the output/result of a task execution by its ID".to_string(),
            task_store,
        }
    }

    /// Build output from a found task, deduplicating the extraction logic.
    fn build_task_output(&self, task: &TodoItem, task_id: &str) -> TaskOutputOutput {
        let output = if task.status == TodoStatus::Completed || !task.content.is_empty() {
            Some(task.content.clone())
        } else {
            None
        };

        TaskOutputOutput {
            found: true,
            task: Some(task.clone()),
            output: output.clone(),
            message: if output.is_some() {
                "Task output retrieved".to_string()
            } else {
                format!(
                    "Task {} found (status: {:?}) but has no output yet",
                    task_id, task.status
                )
            },
        }
    }

    async fn get_output(&self, input: TaskOutputInput) -> Result<TaskOutputOutput, ToolError> {
        let timeout_ms = input.timeout.unwrap_or(30000);
        let block = input.block.unwrap_or(true);

        if block {
            // Blocking wait with timeout
            let deadline =
                tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);

            loop {
                {
                    let store = self.task_store.read().map_err(|e| {
                        ToolError::ExecutionFailed(format!(
                            "Failed to acquire task store lock: {e}"
                        ))
                    })?;

                    if let Some(task) = store.get(&input.task_id) {
                        return Ok(self.build_task_output(task, &input.task_id));
                    }
                }

                // Check timeout
                if tokio::time::Instant::now() >= deadline {
                    return Err(ToolError::ExecutionFailed(format!(
                        "Timeout waiting for task {} output after {}ms",
                        input.task_id, timeout_ms
                    )));
                }

                // Wait a bit before retrying
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        } else {
            // Non-blocking: just read once
            let store = self.task_store.read().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire task store lock: {e}"))
            })?;

            if let Some(task) = store.get(&input.task_id) {
                Ok(self.build_task_output(task, &input.task_id))
            } else {
                Ok(TaskOutputOutput {
                    found: false,
                    task: None,
                    output: None,
                    message: format!("Task {} not found", input.task_id),
                })
            }
        }
    }
}

#[async_trait]
impl Tool for TaskOutputTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let output_input: TaskOutputInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid task output input: {e}")))?;
        let output = self.get_output(output_input).await?;

        Ok(ToolOutput {
            content: output.message.clone(),
            is_error: !output.found,
            metadata: {
                let mut map = HashMap::new();
                map.insert("found".to_string(), json!(output.found));
                if let Some(ref task) = output.task {
                    map.insert("task".to_string(), json!(task));
                }
                if let Some(ref out) = output.output {
                    map.insert("output".to_string(), json!(out));
                }
                map
            },
        })
    }

    fn name(&self) -> &str {
        "TaskOutput"
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
                    "description": "Task ID to get output from"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds for blocking wait (default 30000)",
                    "minimum": 0,
                    "maximum": 300000
                },
                "block": {
                    "type": "boolean",
                    "description": "Whether to block until output is available (default true)"
                }
            },
            "required": ["task_id"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::todo::TodoItem;
    use uuid::Uuid;

    fn make_task(content: &str, status: TodoStatus) -> TodoItem {
        TodoItem {
            task_id: Uuid::new_v4().to_string(),
            subject: "Test task".to_string(),
            description: content.to_string(),
            content: content.to_string(),
            status,
            active_form: None,
            metadata: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            blocked_by: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_task_output_found_with_content() {
        let store: TaskStore = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
        let tool = TaskOutputTool::with_store(store.clone());

        let task = make_task("Build completed successfully", TodoStatus::Completed);
        let task_id = task.task_id.clone();
        store.write().unwrap().insert(task_id.clone(), task);

        let result = tool
            .get_output(TaskOutputInput {
                task_id,
                timeout: None,
                block: Some(false),
            })
            .await
            .unwrap();

        assert!(result.found);
        assert_eq!(
            result.output,
            Some("Build completed successfully".to_string())
        );
        assert_eq!(result.message, "Task output retrieved");
    }

    #[tokio::test]
    async fn test_task_output_not_found() {
        let store: TaskStore = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
        let tool = TaskOutputTool::with_store(store);

        let result = tool
            .get_output(TaskOutputInput {
                task_id: "non-existent".to_string(),
                timeout: None,
                block: Some(false),
            })
            .await
            .unwrap();

        assert!(!result.found);
        assert!(result.output.is_none());
        assert!(result.task.is_none());
    }

    #[tokio::test]
    async fn test_task_output_pending_no_content() {
        let store: TaskStore = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
        let tool = TaskOutputTool::with_store(store.clone());

        let mut task = make_task("", TodoStatus::Pending);
        task.content = String::new();
        let task_id = task.task_id.clone();
        store.write().unwrap().insert(task_id.clone(), task);

        let result = tool
            .get_output(TaskOutputInput {
                task_id,
                timeout: None,
                block: Some(false),
            })
            .await
            .unwrap();

        assert!(result.found);
        assert!(result.output.is_none());
        assert!(result.message.contains("no output yet"));
    }

    #[tokio::test]
    async fn test_task_output_blocking_timeout() {
        let store: TaskStore = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
        let tool = TaskOutputTool::with_store(store);

        // Block with a very short timeout for a non-existent task
        let result = tool
            .get_output(TaskOutputInput {
                task_id: "ghost-task".to_string(),
                timeout: Some(200), // 200ms
                block: Some(true),
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Timeout"));
        assert!(err.contains("ghost-task"));
    }

    #[tokio::test]
    async fn test_task_output_blocking_immediate_available() {
        let store: TaskStore = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
        let tool = TaskOutputTool::with_store(store.clone());

        let task = make_task("Immediate result", TodoStatus::Completed);
        let task_id = task.task_id.clone();
        store.write().unwrap().insert(task_id.clone(), task);

        // Block=true but task is already available — should return immediately
        let result = tool
            .get_output(TaskOutputInput {
                task_id,
                timeout: Some(5000),
                block: Some(true),
            })
            .await
            .unwrap();

        assert!(result.found);
        assert_eq!(result.output, Some("Immediate result".to_string()));
    }

    #[tokio::test]
    async fn test_task_output_in_progress_with_content() {
        let store: TaskStore = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
        let tool = TaskOutputTool::with_store(store.clone());

        let task = make_task("Partial output so far", TodoStatus::InProgress);
        let task_id = task.task_id.clone();
        store.write().unwrap().insert(task_id.clone(), task);

        let result = tool
            .get_output(TaskOutputInput {
                task_id,
                timeout: None,
                block: Some(false),
            })
            .await
            .unwrap();

        assert!(result.found);
        assert_eq!(result.output, Some("Partial output so far".to_string()));
    }

    #[tokio::test]
    async fn test_task_output_tool_name_and_schema() {
        let tool = TaskOutputTool::new();
        assert_eq!(tool.name(), "TaskOutput");
        assert!(tool.description().contains("output"));

        let schema = tool.input_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("task_id"));
        assert!(props.contains_key("timeout"));
        assert!(props.contains_key("block"));

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("task_id")));
        assert!(!required.contains(&json!("timeout")));
        assert!(!required.contains(&json!("block")));
    }
}
