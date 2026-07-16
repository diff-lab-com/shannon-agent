//! Team task management tools for the shared TaskBoard.
//!
//! Provides LLM-callable tools to create, update, list, and get team tasks
//! that are coordinated across agents via the AgentCoordinator's TaskBoard.

use crate::coordinator::AgentCoordinator;
use crate::task::{AgentTask, TaskPriority, TaskStatus};
use async_trait::async_trait;
use serde_json::{Value, json};
use shannon_core::tools::{Tool, ToolError, ToolOutput, ToolResult};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

fn success_output(content: Value) -> ToolOutput {
    ToolOutput {
        content: content.to_string(),
        is_error: false,
        metadata: HashMap::new(),
    }
}

// ── TeamTaskCreateTool ─────────────────────────────────────────────────

/// Tool to create a task on the shared team task board.
pub struct TeamTaskCreateTool {
    coordinator: Arc<AgentCoordinator>,
}

impl TeamTaskCreateTool {
    pub fn new(coordinator: Arc<AgentCoordinator>) -> Self {
        Self { coordinator }
    }
}

#[async_trait]
impl Tool for TeamTaskCreateTool {
    fn name(&self) -> &str {
        "team_task_create"
    }

    fn description(&self) -> &str {
        "Create a new task on the shared team task board. Tasks can be assigned to agents \
         and tracked across the team. Returns the created task ID."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "subject": {
                    "type": "string",
                    "description": "Brief action title in imperative form (e.g. 'Implement auth middleware')"
                },
                "description": {
                    "type": "string",
                    "description": "Detailed description of what needs to be done"
                },
                "priority": {
                    "type": "string",
                    "enum": ["low", "medium", "high", "critical"],
                    "description": "Task priority (default: medium)"
                },
                "owner": {
                    "type": "string",
                    "description": "Agent name to assign this task to (optional)"
                },
                "active_form": {
                    "type": "string",
                    "description": "Present continuous form for progress display (e.g. 'Implementing auth')"
                },
                "required_capabilities": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Capabilities required to perform this task"
                },
                "blocked_by": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs this task depends on"
                }
            },
            "required": ["subject", "description"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let subject = input["subject"].as_str().unwrap_or_default().to_string();
        let description = input["description"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        if subject.is_empty() {
            return Err(ToolError::InvalidInput("subject is required".into()));
        }

        let priority = match input["priority"].as_str().unwrap_or("medium") {
            "low" => TaskPriority::Low,
            "high" => TaskPriority::High,
            "critical" => TaskPriority::Critical,
            _ => TaskPriority::Medium,
        };

        let mut task = AgentTask::new(subject, description, priority);

        if let Some(form) = input["active_form"].as_str() {
            task.active_form = Some(form.to_string());
        }

        if let Some(caps) = input["required_capabilities"].as_array() {
            task.required_capabilities = caps
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }

        if let Some(blocked) = input["blocked_by"].as_array() {
            task.blocked_by = blocked
                .iter()
                .filter_map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                .collect();
        }

        let task_id = task.id;
        let owner = input["owner"].as_str().map(|s| s.to_string());

        self.coordinator
            .task_board()
            .add_task(task)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create task: {e}")))?;

        // If owner specified, assign the task
        if let Some(ref agent_name) = owner {
            if let Err(e) = self
                .coordinator
                .task_board()
                .assign_task(task_id, agent_name.clone())
                .await
            {
                tracing::warn!(task_id = %task_id, agent = %agent_name, "Task created but assignment failed: {e}");
            }
        }

        Ok(success_output(json!({
            "task_id": task_id.to_string(),
            "status": "created",
            "assigned_to": owner,
        })))
    }
}

// ── TeamTaskUpdateTool ─────────────────────────────────────────────────

/// Tool to update task status (claim, complete, fail) on the shared task board.
pub struct TeamTaskUpdateTool {
    coordinator: Arc<AgentCoordinator>,
}

impl TeamTaskUpdateTool {
    pub fn new(coordinator: Arc<AgentCoordinator>) -> Self {
        Self { coordinator }
    }
}

#[async_trait]
impl Tool for TeamTaskUpdateTool {
    fn name(&self) -> &str {
        "team_task_update"
    }

    fn description(&self) -> &str {
        "Update a team task's status. Use 'in_progress' to claim/start a task, \
         'completed' to mark it done, 'failed' to mark it failed with a reason, \
         or 'cancelled' to cancel it."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The UUID of the task to update"
                },
                "status": {
                    "type": "string",
                    "enum": ["in_progress", "completed", "failed", "cancelled"],
                    "description": "New status for the task"
                },
                "reason": {
                    "type": "string",
                    "description": "Reason for failure (required when status is 'failed')"
                }
            },
            "required": ["task_id", "status"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let task_id_str = input["task_id"].as_str().unwrap_or_default();
        let task_id = Uuid::parse_str(task_id_str)
            .map_err(|_| ToolError::InvalidInput("Invalid task_id UUID".into()))?;

        let status_str = input["status"].as_str().unwrap_or_default();
        let board = self.coordinator.task_board();

        match status_str {
            "in_progress" => {
                board
                    .update_task_status(task_id, TaskStatus::InProgress)
                    .await
                    .map_err(|e| {
                        ToolError::ExecutionFailed(format!("Failed to update task: {e}"))
                    })?;
            }
            "completed" => {
                board.complete_task(task_id).await.map_err(|e| {
                    ToolError::ExecutionFailed(format!("Failed to complete task: {e}"))
                })?;
            }
            "failed" => {
                let reason = input["reason"]
                    .as_str()
                    .unwrap_or("No reason provided")
                    .to_string();
                board
                    .fail_task(task_id, reason)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(format!("Failed to fail task: {e}")))?;
            }
            "cancelled" => {
                board
                    .update_task_status(task_id, TaskStatus::Cancelled)
                    .await
                    .map_err(|e| {
                        ToolError::ExecutionFailed(format!("Failed to cancel task: {e}"))
                    })?;
            }
            _ => {
                return Err(ToolError::InvalidInput(format!(
                    "Invalid status '{status_str}'. Use: in_progress, completed, failed, or cancelled"
                )));
            }
        }

        Ok(success_output(json!({
            "task_id": task_id.to_string(),
            "status": status_str,
        })))
    }
}

// ── TeamTaskListTool ────────────────────────────────────────────────────

/// Tool to list tasks on the shared team task board.
pub struct TeamTaskListTool {
    coordinator: Arc<AgentCoordinator>,
}

impl TeamTaskListTool {
    pub fn new(coordinator: Arc<AgentCoordinator>) -> Self {
        Self { coordinator }
    }
}

#[async_trait]
impl Tool for TeamTaskListTool {
    fn name(&self) -> &str {
        "team_task_list"
    }

    fn description(&self) -> &str {
        "List tasks on the shared team task board. Optionally filter by status or agent owner. \
         Returns a summary with task details."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "failed", "blocked", "cancelled"],
                    "description": "Filter tasks by status (optional, returns all if omitted)"
                },
                "agent": {
                    "type": "string",
                    "description": "Filter tasks assigned to a specific agent (optional)"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let board = self.coordinator.task_board();

        let tasks = if let Some(agent) = input["agent"].as_str() {
            board.get_agent_tasks(agent).await
        } else if let Some(status_str) = input["status"].as_str() {
            let status = match status_str {
                "pending" => TaskStatus::Pending,
                "in_progress" => TaskStatus::InProgress,
                "completed" => TaskStatus::Completed,
                "failed" => TaskStatus::Failed(String::new()),
                "blocked" => TaskStatus::Blocked,
                "cancelled" => TaskStatus::Cancelled,
                _ => {
                    return Err(ToolError::InvalidInput(format!(
                        "Invalid status: {status_str}"
                    )));
                }
            };
            board.list_tasks_by_status(status).await
        } else {
            board.list_all_tasks().await
        };

        let summary = board.summary().await;

        let task_list: Vec<Value> = tasks
            .iter()
            .map(|t| {
                let status_str = match &t.status {
                    TaskStatus::Pending => "pending".to_string(),
                    TaskStatus::InProgress => "in_progress".to_string(),
                    TaskStatus::Completed => "completed".to_string(),
                    TaskStatus::Failed(r) => format!("failed: {r}"),
                    TaskStatus::Blocked => "blocked".to_string(),
                    TaskStatus::Cancelled => "cancelled".to_string(),
                };
                json!({
                    "id": t.id.to_string(),
                    "subject": t.subject,
                    "status": status_str,
                    "owner": t.owner,
                    "priority": format!("{:?}", t.priority).to_lowercase(),
                    "blocked_by": t.blocked_by.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
                })
            })
            .collect();

        Ok(success_output(json!({
            "summary": {
                "total": summary.total_tasks,
                "pending": summary.pending_tasks,
                "in_progress": summary.in_progress_tasks,
                "completed": summary.completed_tasks,
                "failed": summary.failed_tasks,
            },
            "tasks": task_list,
        })))
    }
}

// ── TeamTaskClaimTool ──────────────────────────────────────────────────

/// Tool for an agent to self-claim the next available task.
///
/// Implements the Claude Code model: idle agents proactively claim the
/// lowest-ID unblocked, pending, unowned task from the shared task board.
pub struct TeamTaskClaimTool {
    coordinator: Arc<AgentCoordinator>,
    /// Default team name for claiming tasks
    team_name: String,
    /// The agent name that will claim tasks
    agent_name: String,
}

impl TeamTaskClaimTool {
    pub fn new(coordinator: Arc<AgentCoordinator>, team_name: String, agent_name: String) -> Self {
        Self {
            coordinator,
            team_name,
            agent_name,
        }
    }
}

#[async_trait]
impl Tool for TeamTaskClaimTool {
    fn name(&self) -> &str {
        "team_task_claim"
    }

    fn description(&self) -> &str {
        "Claim the next available task from the shared task board. \
         Automatically selects the lowest-ID unblocked, pending, unowned task \
         and assigns it to you. Returns the claimed task details, or indicates \
         no tasks are available."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Specific task UUID to claim (optional — if omitted, claims the next available task)"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let result = if let Some(task_id_str) = input["task_id"].as_str() {
            // Claim a specific task by ID
            let task_id = Uuid::parse_str(task_id_str)
                .map_err(|_| ToolError::InvalidInput("Invalid task_id UUID".into()))?;
            self.coordinator
                .self_claim_task(&self.team_name, &self.agent_name, task_id)
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to claim task: {e}")))?
        } else {
            // Claim the next available task
            self.coordinator
                .claim_next_task(&self.team_name, &self.agent_name)
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to claim next task: {e}")))?
                .ok_or_else(|| ToolError::ExecutionFailed("No tasks available to claim".into()))?
        };

        let status_str = match &result.status {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed(_) => "failed",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Cancelled => "cancelled",
        };

        Ok(success_output(json!({
            "task_id": result.id.to_string(),
            "subject": result.subject,
            "status": status_str,
            "claimed_by": self.agent_name,
        })))
    }
}

// ── TeamNotifyIdleTool ─────────────────────────────────────────────────

/// Tool for an agent to notify the coordinator that it is idle.
///
/// Returns the list of claimable tasks so the agent can immediately
/// pick up the next one.
pub struct TeamNotifyIdleTool {
    coordinator: Arc<AgentCoordinator>,
    /// Default team name
    team_name: String,
    /// The agent name reporting idle
    agent_name: String,
}

impl TeamNotifyIdleTool {
    pub fn new(coordinator: Arc<AgentCoordinator>, team_name: String, agent_name: String) -> Self {
        Self {
            coordinator,
            team_name,
            agent_name,
        }
    }
}

#[async_trait]
impl Tool for TeamNotifyIdleTool {
    fn name(&self) -> &str {
        "team_notify_idle"
    }

    fn description(&self) -> &str {
        "Notify the team coordinator that you are idle and ready for new work. \
         Returns a list of available tasks you can claim."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object" })
    }

    async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
        let available = self
            .coordinator
            .notify_idle(&self.team_name, &self.agent_name)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to notify idle: {e}")))?;

        let task_list: Vec<Value> = available
            .iter()
            .map(|t| {
                json!({
                    "id": t.id.to_string(),
                    "subject": t.subject,
                    "priority": format!("{:?}", t.priority).to_lowercase(),
                })
            })
            .collect();

        Ok(success_output(json!({
            "agent": self.agent_name,
            "status": "idle",
            "available_tasks": task_list.len(),
            "tasks": task_list,
        })))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn success_output_helper() {
        let output = success_output(json!({"status": "ok"}));
        assert!(!output.is_error);
        assert!(output.content.contains("ok"));
        assert!(output.metadata.is_empty());
    }
}
