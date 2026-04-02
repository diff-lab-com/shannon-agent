//! Todo list management tools
//!
//! Provides implementations for:
//! - TodoWrite: Create and manage session task checklists
//!
//! Enables hierarchical task organization with persistent memory.

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Todo item status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// Todo item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// Unique ID
    pub id: String,

    /// Todo content/description
    pub content: String,

    /// Current status
    pub status: TodoStatus,
}

/// Input for writing todos
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TodoWriteInput {
    /// The updated todo list
    pub todos: Vec<TodoItem>,
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
type TodoStore = Arc<RwLock<HashMap<String, Vec<TodoItem>>>>;

/// Todo write tool
pub struct TodoWriteTool {
    description: String,
    store: TodoStore,
    session_id: String,
}

impl TodoWriteTool {
    pub fn new() -> Self {
        Self {
            description: "Create and manage session task checklists for tracking work progress".to_string(),
            store: Arc::new(RwLock::new(HashMap::new())),
            session_id: Uuid::new_v4().to_string(),
        }
    }

    /// Write todos to store
    async fn write_todos(&self, input: TodoWriteInput) -> Result<TodoWriteOutput, ToolError> {
        let key = &self.session_id;

        // Get old todos
        let old_todos = {
            let store = self.store.read().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {}", e))
            })?;
            store.get(key).cloned().unwrap_or_default()
        };

        // Check if all todos are completed
        let all_done = input.todos.iter().all(|t| t.status == TodoStatus::Completed);

        // If all done, clear the list; otherwise, store new todos
        let new_todos = if all_done {
            // Clear completed todos
            {
                let mut store = self.store.write().map_err(|e| {
                    ToolError::ExecutionFailed(format!("Failed to acquire store lock: {}", e))
                })?;
                store.insert(key.clone(), Vec::new());
            }
            Vec::new()
        } else {
            // Store new todos
            {
                let mut store = self.store.write().map_err(|e| {
                    ToolError::ExecutionFailed(format!("Failed to acquire store lock: {}", e))
                })?;
                store.insert(key.clone(), input.todos.clone());
            }
            input.todos.clone()
        };

        // Check if verification nudge is needed
        // (3+ items completed, none marked as verification)
        let verification_nudge_needed = if all_done && old_todos.len() >= 3 {
            let has_verification = old_todos.iter().any(|t| {
                t.content.to_lowercase().contains("verif")
            });
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
            .map_err(|e| ToolError::InvalidInput(format!("Invalid todo write input: {}", e)))?;
        let output = self.write_todos(write_input).await?;

        let todo_count = output.new_todos.len();
        let content = if todo_count == 0 {
            "All todos completed, list cleared".to_string()
        } else {
            let pending = output.new_todos.iter()
                .filter(|t| t.status == TodoStatus::Pending)
                .count();
            format!("Updated todo list: {} items ({} pending)", todo_count, pending)
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
            "required": ["todos"]
        })
    }
}
