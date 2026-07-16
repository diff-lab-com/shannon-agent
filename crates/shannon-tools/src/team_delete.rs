//! Team delete tool
//!
//! Cleans up team resources by removing a team from the registry
//! and clearing its associated task list.

use crate::todo::TaskStore;
use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Input for the TeamDelete tool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TeamDeleteInput {
    /// Team name to delete
    pub team_name: String,
}

/// Output from the TeamDelete tool
#[derive(Debug, Serialize)]
pub struct TeamDeleteOutput {
    /// Whether the team was found and deleted
    pub deleted: bool,

    /// Confirmation message
    pub message: String,

    /// Number of tasks cleaned up
    pub tasks_removed: usize,
}

/// A team entry in the registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamEntry {
    /// Team name
    pub team_name: String,

    /// Team description
    pub description: String,

    /// Creation timestamp
    pub created_at: String,
}

/// Shared team registry
pub type TeamRegistry = Arc<RwLock<HashMap<String, TeamEntry>>>;

/// Team delete tool — removes a team and cleans up associated tasks.
pub struct TeamDeleteTool {
    description: String,
    team_registry: TeamRegistry,
    task_store: TaskStore,
}

impl Default for TeamDeleteTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TeamDeleteTool {
    pub fn new() -> Self {
        Self {
            description: "Remove a team and clean up its associated task list".to_string(),
            team_registry: Arc::new(RwLock::new(HashMap::new())),
            task_store: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_stores(team_registry: TeamRegistry, task_store: TaskStore) -> Self {
        Self {
            description: "Remove a team and clean up its associated task list".to_string(),
            team_registry,
            task_store,
        }
    }

    async fn delete_team(&self, input: TeamDeleteInput) -> Result<TeamDeleteOutput, ToolError> {
        // Remove team from registry
        let removed_team = {
            let mut registry = self.team_registry.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire team registry lock: {e}"))
            })?;

            registry.remove(&input.team_name)
        };

        // Clean up tasks associated with this team
        let tasks_removed = {
            let mut store = self.task_store.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire task store lock: {e}"))
            })?;

            let before_count = store.len();
            // Remove all tasks whose metadata contains a matching team_name
            store.retain(|_, task| {
                if let Some(ref meta) = task.metadata {
                    if let Some(team) = meta.get("team") {
                        if let Some(team_str) = team.as_str() {
                            return team_str != input.team_name;
                        }
                    }
                }
                true
            });
            before_count - store.len()
        };

        match removed_team {
            Some(_entry) => Ok(TeamDeleteOutput {
                deleted: true,
                message: format!("Team '{}' deleted successfully", input.team_name),
                tasks_removed,
            }),
            None => Err(ToolError::InvalidInput(format!(
                "Team '{}' not found",
                input.team_name
            ))),
        }
    }
}

#[async_trait]
impl Tool for TeamDeleteTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let delete_input: TeamDeleteInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid team delete input: {e}")))?;
        let output = self.delete_team(delete_input).await?;

        Ok(ToolOutput {
            content: output.message.clone(),
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("deleted".to_string(), json!(output.deleted));
                map.insert("tasks_removed".to_string(), json!(output.tasks_removed));
                map
            },
        })
    }

    fn name(&self) -> &str {
        "TeamDelete"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "team_name": {
                    "type": "string",
                    "description": "Name of the team to delete"
                }
            },
            "required": ["team_name"]
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::todo::{TodoItem, TodoStatus};
    use serde_json::json;
    use uuid::Uuid;

    fn make_team(name: &str, description: &str) -> TeamEntry {
        TeamEntry {
            team_name: name.to_string(),
            description: description.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn make_task_with_team(team_name: &str) -> TodoItem {
        TodoItem {
            task_id: Uuid::new_v4().to_string(),
            subject: "Team task".to_string(),
            description: "A task belonging to a team".to_string(),
            content: "A task belonging to a team".to_string(),
            status: TodoStatus::Pending,
            active_form: None,
            metadata: Some(json!({"team": team_name})),
            created_at: chrono::Utc::now().to_rfc3339(),
            blocked_by: Vec::new(),
        }
    }

    fn make_task_without_team() -> TodoItem {
        TodoItem {
            task_id: Uuid::new_v4().to_string(),
            subject: "Orphan task".to_string(),
            description: "A task without a team".to_string(),
            content: "A task without a team".to_string(),
            status: TodoStatus::Pending,
            active_form: None,
            metadata: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            blocked_by: Vec::new(),
        }
    }

    fn setup() -> (TeamRegistry, TaskStore, TeamDeleteTool) {
        let team_registry = Arc::new(RwLock::new(HashMap::new()));
        let task_store: TaskStore = Arc::new(RwLock::new(HashMap::new()));
        let tool = TeamDeleteTool::with_stores(team_registry.clone(), task_store.clone());
        (team_registry, task_store, tool)
    }

    #[tokio::test]
    async fn test_team_delete_success() {
        let (team_registry, _task_store, tool) = setup();

        team_registry
            .write()
            .unwrap()
            .insert("my-team".to_string(), make_team("my-team", "My team"));

        let result = tool
            .delete_team(TeamDeleteInput {
                team_name: "my-team".to_string(),
            })
            .await
            .unwrap();

        assert!(result.deleted);
        assert!(result.message.contains("my-team"));
        assert!(result.message.contains("deleted"));

        // Verify team is gone from registry
        assert!(!team_registry.read().unwrap().contains_key("my-team"));
    }

    #[tokio::test]
    async fn test_team_delete_not_found() {
        let (_, _, tool) = setup();

        let result = tool
            .delete_team(TeamDeleteInput {
                team_name: "ghost-team".to_string(),
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_team_delete_cleans_up_tasks() {
        let (team_registry, task_store, tool) = setup();

        team_registry
            .write()
            .unwrap()
            .insert("backend".to_string(), make_team("backend", "Backend team"));

        // Add tasks for the backend team
        task_store
            .write()
            .unwrap()
            .insert("t1".to_string(), make_task_with_team("backend"));
        task_store
            .write()
            .unwrap()
            .insert("t2".to_string(), make_task_with_team("backend"));

        // Add a task for a different team (should not be removed)
        task_store
            .write()
            .unwrap()
            .insert("t3".to_string(), make_task_with_team("frontend"));

        // Add a task with no team (should not be removed)
        task_store
            .write()
            .unwrap()
            .insert("t4".to_string(), make_task_without_team());

        assert_eq!(task_store.read().unwrap().len(), 4);

        let result = tool
            .delete_team(TeamDeleteInput {
                team_name: "backend".to_string(),
            })
            .await
            .unwrap();

        assert!(result.deleted);
        assert_eq!(result.tasks_removed, 2);

        // Only frontend and orphan tasks should remain
        assert_eq!(task_store.read().unwrap().len(), 2);
        assert!(task_store.read().unwrap().contains_key("t3"));
        assert!(task_store.read().unwrap().contains_key("t4"));
        assert!(!task_store.read().unwrap().contains_key("t1"));
        assert!(!task_store.read().unwrap().contains_key("t2"));
    }

    #[tokio::test]
    async fn test_team_delete_empty_team() {
        let (team_registry, task_store, tool) = setup();

        team_registry.write().unwrap().insert(
            "empty-team".to_string(),
            make_team("empty-team", "No tasks"),
        );

        let result = tool
            .delete_team(TeamDeleteInput {
                team_name: "empty-team".to_string(),
            })
            .await
            .unwrap();

        assert!(result.deleted);
        assert_eq!(result.tasks_removed, 0);
        assert_eq!(task_store.read().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_team_delete_tool_name_and_schema() {
        let tool = TeamDeleteTool::new();
        assert_eq!(tool.name(), "TeamDelete");
        assert!(tool.description().contains("team"));

        let schema = tool.input_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("team_name"));

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("team_name")));
    }

    #[tokio::test]
    async fn test_team_delete_invalid_json() {
        let (_, _, tool) = setup();

        let result = tool.execute(json!({"team_name": 123})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_team_delete_invalid_empty_name() {
        let (_, _, tool) = setup();

        let result = tool
            .delete_team(TeamDeleteInput {
                team_name: "".to_string(),
            })
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_team_entry_serde() {
        let entry = make_team("test-team", "Test description");
        let json_str = serde_json::to_string(&entry).unwrap();
        let deserialized: TeamEntry = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.team_name, "test-team");
        assert_eq!(deserialized.description, "Test description");
    }
}
