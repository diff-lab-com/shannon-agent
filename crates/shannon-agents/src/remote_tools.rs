//! Remote team tools for process-based agents.
//!
//! These tools communicate with the coordinator via JSON-RPC over stdin/stdout,
//! enabling process-isolated agents to interact with the shared task board
//! and send messages to teammates.

use async_trait::async_trait;
use serde_json::{Value, json};
use shannon_core::tools::{Tool, ToolError, ToolOutput, ToolResult};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::{Mutex, oneshot};

use crate::protocol::{JsonRpcMessage, frame_message, methods};

/// Channel for communicating with the coordinator from a process agent.
///
/// Wraps stdout for sending JSON-RPC messages and tracks pending
/// request-response pairs so tools can await coordinator replies.
#[derive(Clone)]
pub struct CoordinatorChannel {
    /// Writer to stdout (for sending JSON-RPC to coordinator).
    writer: Arc<Mutex<BufWriter<tokio::io::Stdout>>>,
    /// Pending RPC responses keyed by request ID.
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<JsonRpcMessage>>>>,
    /// Next request ID.
    next_id: Arc<AtomicU64>,
}

fn success_output(content: Value) -> ToolOutput {
    ToolOutput {
        content: content.to_string(),
        is_error: false,
        metadata: HashMap::new(),
    }
}

impl Default for CoordinatorChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl CoordinatorChannel {
    /// Create a new coordinator channel using stdout.
    pub fn new() -> Self {
        Self {
            writer: Arc::new(Mutex::new(BufWriter::new(tokio::io::stdout()))),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Allocate a new request ID.
    fn next_id(&self) -> i64 {
        self.next_id.fetch_add(1, Ordering::Relaxed) as i64
    }

    /// Send a JSON-RPC notification (fire-and-forget).
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let msg = JsonRpcMessage::notification(method, params);
        let line = frame_message(&msg).map_err(|e| e.to_string())?;
        let mut writer = self.writer.lock().await;
        writer
            .write_all(line.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        writer.flush().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Send a JSON-RPC request and return a oneshot receiver for the response.
    pub async fn request(
        &self,
        method: &str,
        params: Value,
    ) -> Result<oneshot::Receiver<JsonRpcMessage>, String> {
        let id = self.next_id();
        let msg = JsonRpcMessage::request(method, params, id);
        let line = frame_message(&msg).map_err(|e| e.to_string())?;

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        let mut writer = self.writer.lock().await;
        writer
            .write_all(line.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        writer.flush().await.map_err(|e| e.to_string())?;
        Ok(rx)
    }

    /// Dispatch an incoming response to the waiting tool.
    /// Called from the main stdin reader loop when a response arrives.
    pub async fn dispatch_response(&self, msg: JsonRpcMessage) {
        if let Some(id) = msg.id.as_ref().and_then(|id| match id {
            crate::protocol::JsonRpcId::Number(n) => Some(*n),
            _ => None,
        }) {
            let mut pending = self.pending.lock().await;
            if let Some(sender) = pending.remove(&id) {
                let _ = sender.send(msg);
            }
        }
    }
}

// ── RemoteTeamTaskListTool ─────────────────────────────────────────────

/// Tool to list tasks via JSON-RPC to the coordinator.
pub struct RemoteTeamTaskListTool {
    channel: CoordinatorChannel,
}

impl RemoteTeamTaskListTool {
    pub fn new(channel: CoordinatorChannel) -> Self {
        Self { channel }
    }
}

#[async_trait]
impl Tool for RemoteTeamTaskListTool {
    fn name(&self) -> &str {
        "team_task_list"
    }

    fn description(&self) -> &str {
        "List tasks on the shared team task board. Optionally filter by status. \
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
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let params = json!({
            "status": input.get("status").and_then(|v| v.as_str()).unwrap_or(""),
        });

        let rx = self
            .channel
            .request(methods::LIST_TASKS, params)
            .await
            .map_err(ToolError::ExecutionFailed)?;

        let response = rx.await.map_err(|_| {
            ToolError::ExecutionFailed("Coordinator dropped response channel".into())
        })?;

        // The response result contains the task list
        if let Some(ref error) = response.error {
            return Err(ToolError::ExecutionFailed(format!(
                "Coordinator error: {error:?}"
            )));
        }

        let result = response.result.as_ref().unwrap_or(&json!(null));
        Ok(success_output(result.clone()))
    }
}

// ── RemoteTeamTaskClaimTool ────────────────────────────────────────────

/// Tool to claim a task via JSON-RPC to the coordinator.
pub struct RemoteTeamTaskClaimTool {
    channel: CoordinatorChannel,
    agent_name: String,
}

impl RemoteTeamTaskClaimTool {
    pub fn new(channel: CoordinatorChannel, agent_name: String) -> Self {
        Self {
            channel,
            agent_name,
        }
    }
}

#[async_trait]
impl Tool for RemoteTeamTaskClaimTool {
    fn name(&self) -> &str {
        "team_task_claim"
    }

    fn description(&self) -> &str {
        "Claim the next available task from the shared task board, or a specific task by ID. \
         The task will be assigned to you and marked as in_progress."
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
        let params = json!({
            "agent_name": self.agent_name,
            "task_id": input.get("task_id").and_then(|v| v.as_str()).unwrap_or(""),
        });

        let rx = self
            .channel
            .request(methods::CLAIM_TASK, params)
            .await
            .map_err(ToolError::ExecutionFailed)?;

        let response = rx.await.map_err(|_| {
            ToolError::ExecutionFailed("Coordinator dropped response channel".into())
        })?;

        if let Some(ref error) = response.error {
            return Err(ToolError::ExecutionFailed(format!(
                "Claim failed: {error:?}"
            )));
        }

        let result = response.result.as_ref().unwrap_or(&json!(null));
        Ok(success_output(result.clone()))
    }
}

// ── RemoteTeamNotifyIdleTool ───────────────────────────────────────────

/// Tool to notify the coordinator that the agent is idle (fire-and-forget).
pub struct RemoteTeamNotifyIdleTool {
    channel: CoordinatorChannel,
    agent_name: String,
}

impl RemoteTeamNotifyIdleTool {
    pub fn new(channel: CoordinatorChannel, agent_name: String) -> Self {
        Self {
            channel,
            agent_name,
        }
    }
}

#[async_trait]
impl Tool for RemoteTeamNotifyIdleTool {
    fn name(&self) -> &str {
        "team_notify_idle"
    }

    fn description(&self) -> &str {
        "Notify the team coordinator that you are idle and ready for new work."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object" })
    }

    async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
        self.channel
            .notify(
                methods::AGENT_IDLE,
                json!({
                    "agent_name": self.agent_name,
                    "available_tasks_count": 0,
                }),
            )
            .await
            .map_err(ToolError::ExecutionFailed)?;

        Ok(success_output(json!({
            "agent": self.agent_name,
            "status": "idle",
        })))
    }
}

// ── RemoteSendMessageTool ──────────────────────────────────────────────

/// Tool to send a message to a teammate via JSON-RPC.
pub struct RemoteSendMessageTool {
    channel: CoordinatorChannel,
    agent_name: String,
}

impl RemoteSendMessageTool {
    pub fn new(channel: CoordinatorChannel, agent_name: String) -> Self {
        Self {
            channel,
            agent_name,
        }
    }
}

#[async_trait]
impl Tool for RemoteSendMessageTool {
    fn name(&self) -> &str {
        "team_send_message"
    }

    fn description(&self) -> &str {
        "Send a message to a teammate or broadcast to all teammates. \
         Use 'to' for a specific agent name, '*' for broadcast to all, \
         or omit 'to' for broadcast. Add 'summary' for a 5-10 word preview."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": {
                    "type": "string",
                    "description": "Recipient agent name, '*' for broadcast, or omit for broadcast to all"
                },
                "message": {
                    "type": "string",
                    "description": "Message content"
                },
                "summary": {
                    "type": "string",
                    "description": "Short preview of the message (5-10 words) for UI display"
                },
                "priority": {
                    "type": "string",
                    "enum": ["low", "normal", "high"],
                    "description": "Message priority (default: normal)"
                }
            },
            "required": ["message"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let message = input["message"].as_str().unwrap_or_default().to_string();
        if message.is_empty() {
            return Err(ToolError::InvalidInput("message is required".into()));
        }

        let to = input
            .get("to")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string();
        let summary = input
            .get("summary")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut params = json!({
            "from": self.agent_name,
            "to": to,
            "message": message,
            "priority": input.get("priority").and_then(|v| v.as_str()).unwrap_or("normal"),
        });

        if let Some(ref s) = summary {
            params["summary"] = json!(s);
        }

        self.channel
            .notify(methods::SEND_MESSAGE, params)
            .await
            .map_err(ToolError::ExecutionFailed)?;

        let mut result = json!({
            "status": "sent",
            "from": self.agent_name,
        });
        if let Some(s) = summary {
            result["summary"] = json!(s);
        }

        Ok(success_output(result))
    }
}

// ── RemoteTeamTaskCreateTool ──────────────────────────────────────────

/// Tool to create a new task on the shared task board via JSON-RPC.
pub struct RemoteTeamTaskCreateTool {
    channel: CoordinatorChannel,
}

impl RemoteTeamTaskCreateTool {
    pub fn new(channel: CoordinatorChannel) -> Self {
        Self { channel }
    }
}

#[async_trait]
impl Tool for RemoteTeamTaskCreateTool {
    fn name(&self) -> &str {
        "team_task_create"
    }

    fn description(&self) -> &str {
        "Create a new task on the shared team task board. \
         Returns the created task ID. Use this to break work into sub-tasks."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "subject": {
                    "type": "string",
                    "description": "Short task title (imperative form)"
                },
                "description": {
                    "type": "string",
                    "description": "Detailed task description and requirements"
                },
                "priority": {
                    "type": "string",
                    "enum": ["low", "normal", "high", "critical"],
                    "description": "Task priority (default: normal)"
                },
                "blocked_by": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs this task depends on"
                }
            },
            "required": ["subject"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let subject = input["subject"].as_str().unwrap_or_default().to_string();
        if subject.is_empty() {
            return Err(ToolError::InvalidInput("subject is required".into()));
        }

        let params = json!({
            "subject": subject,
            "description": input.get("description").and_then(|v| v.as_str()).unwrap_or(""),
            "priority": input.get("priority").and_then(|v| v.as_str()).unwrap_or("normal"),
            "blocked_by": input.get("blocked_by").and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
                .unwrap_or_default(),
        });

        let rx = self
            .channel
            .request(methods::CREATE_TASK, params)
            .await
            .map_err(ToolError::ExecutionFailed)?;

        let response = rx.await.map_err(|_| {
            ToolError::ExecutionFailed("Coordinator dropped response channel".into())
        })?;

        if let Some(ref error) = response.error {
            return Err(ToolError::ExecutionFailed(format!(
                "Create task failed: {error:?}"
            )));
        }

        let result = response.result.as_ref().unwrap_or(&json!(null));
        Ok(success_output(result.clone()))
    }
}

// ── RemoteTeamTaskUpdateTool ──────────────────────────────────────────

/// Tool to update a task on the shared task board via JSON-RPC.
pub struct RemoteTeamTaskUpdateTool {
    channel: CoordinatorChannel,
    agent_name: String,
}

impl RemoteTeamTaskUpdateTool {
    pub fn new(channel: CoordinatorChannel, agent_name: String) -> Self {
        Self {
            channel,
            agent_name,
        }
    }
}

#[async_trait]
impl Tool for RemoteTeamTaskUpdateTool {
    fn name(&self) -> &str {
        "team_task_update"
    }

    fn description(&self) -> &str {
        "Update a task on the shared team task board. \
         Can change status, description, or add dependencies."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Task UUID to update"
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "failed", "blocked"],
                    "description": "New task status"
                },
                "description": {
                    "type": "string",
                    "description": "Updated task description"
                },
                "add_blocks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs that this task blocks"
                },
                "add_blocked_by": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs that block this task"
                }
            },
            "required": ["task_id"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let task_id = input["task_id"].as_str().unwrap_or_default().to_string();
        if task_id.is_empty() {
            return Err(ToolError::InvalidInput("task_id is required".into()));
        }

        let params = json!({
            "agent_name": self.agent_name,
            "task_id": task_id,
            "status": input.get("status").and_then(|v| v.as_str()).unwrap_or(""),
            "description": input.get("description").and_then(|v| v.as_str()).unwrap_or(""),
            "add_blocks": input.get("add_blocks").and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
                .unwrap_or_default(),
            "add_blocked_by": input.get("add_blocked_by").and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
                .unwrap_or_default(),
        });

        let rx = self
            .channel
            .request(methods::UPDATE_TASK, params)
            .await
            .map_err(ToolError::ExecutionFailed)?;

        let response = rx.await.map_err(|_| {
            ToolError::ExecutionFailed("Coordinator dropped response channel".into())
        })?;

        if let Some(ref error) = response.error {
            return Err(ToolError::ExecutionFailed(format!(
                "Update task failed: {error:?}"
            )));
        }

        let result = response.result.as_ref().unwrap_or(&json!(null));
        Ok(success_output(result.clone()))
    }
}

// ── RemoteTeamTaskGetTool ─────────────────────────────────────────────

/// Tool to get full task details by ID via JSON-RPC.
pub struct RemoteTeamTaskGetTool {
    channel: CoordinatorChannel,
}

impl RemoteTeamTaskGetTool {
    pub fn new(channel: CoordinatorChannel) -> Self {
        Self { channel }
    }
}

#[async_trait]
impl Tool for RemoteTeamTaskGetTool {
    fn name(&self) -> &str {
        "team_task_get"
    }

    fn description(&self) -> &str {
        "Get full details for a specific task by ID. \
         Returns the task subject, description, status, owner, dependencies, and metadata."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Task UUID to retrieve"
                }
            },
            "required": ["task_id"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let task_id = input["task_id"].as_str().unwrap_or_default().to_string();
        if task_id.is_empty() {
            return Err(ToolError::InvalidInput("task_id is required".into()));
        }

        let params = json!({
            "task_id": task_id,
        });

        let rx = self
            .channel
            .request(methods::GET_TASK, params)
            .await
            .map_err(ToolError::ExecutionFailed)?;

        let response = rx.await.map_err(|_| {
            ToolError::ExecutionFailed("Coordinator dropped response channel".into())
        })?;

        if let Some(ref error) = response.error {
            return Err(ToolError::ExecutionFailed(format!(
                "Get task failed: {error:?}"
            )));
        }

        let result = response.result.as_ref().unwrap_or(&json!(null));
        Ok(success_output(result.clone()))
    }
}

// ── RemoteTeamManifestTool ────────────────────────────────────────────

/// Tool to discover team members and capabilities via JSON-RPC.
pub struct RemoteTeamManifestTool {
    channel: CoordinatorChannel,
}

impl RemoteTeamManifestTool {
    pub fn new(channel: CoordinatorChannel) -> Self {
        Self { channel }
    }
}

#[async_trait]
impl Tool for RemoteTeamManifestTool {
    fn name(&self) -> &str {
        "team_manifest"
    }

    fn description(&self) -> &str {
        "Get the team manifest showing all teammates and their capabilities. \
         Use this to discover who else is on the team and what they can do."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object" })
    }

    async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
        let rx = self
            .channel
            .request(methods::TEAM_MANIFEST, json!({}))
            .await
            .map_err(ToolError::ExecutionFailed)?;

        let response = rx.await.map_err(|_| {
            ToolError::ExecutionFailed("Coordinator dropped response channel".into())
        })?;

        if let Some(ref error) = response.error {
            return Err(ToolError::ExecutionFailed(format!(
                "Manifest request failed: {error:?}"
            )));
        }

        let result = response.result.as_ref().unwrap_or(&json!(null));
        Ok(success_output(result.clone()))
    }
}

/// Tool to disband a team (team lead only).
pub struct RemoteDisbandTeamTool {
    channel: CoordinatorChannel,
}

impl RemoteDisbandTeamTool {
    pub fn new(channel: CoordinatorChannel) -> Self {
        Self { channel }
    }
}

#[async_trait]
impl Tool for RemoteDisbandTeamTool {
    fn name(&self) -> &str {
        "disband_team"
    }

    fn description(&self) -> &str {
        "Disband the specified team, shutting down all agents. \
         Only team leads can perform this action."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "team_name": { "type": "string", "description": "Name of the team to disband" }
            },
            "required": ["team_name"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let team_name = input["team_name"].as_str().unwrap_or_default().to_string();
        let rx = self
            .channel
            .request(methods::DISBAND_TEAM, json!({ "team_name": team_name }))
            .await
            .map_err(ToolError::ExecutionFailed)?;

        let response = rx.await.map_err(|_| {
            ToolError::ExecutionFailed("Coordinator dropped response channel".into())
        })?;

        if let Some(ref error) = response.error {
            return Err(ToolError::ExecutionFailed(format!(
                "Disband request failed: {error:?}"
            )));
        }

        let result = response.result.as_ref().unwrap_or(&json!(null));
        Ok(success_output(result.clone()))
    }
}

/// Tool to add an agent to a team (team lead only).
pub struct RemoteAddAgentTool {
    channel: CoordinatorChannel,
}

impl RemoteAddAgentTool {
    pub fn new(channel: CoordinatorChannel) -> Self {
        Self { channel }
    }
}

#[async_trait]
impl Tool for RemoteAddAgentTool {
    fn name(&self) -> &str {
        "add_agent"
    }

    fn description(&self) -> &str {
        "Add a new agent to the specified team. Only team leads can perform this action."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "team_name": { "type": "string", "description": "Name of the team" },
                "agent_name": { "type": "string", "description": "Name for the new agent" },
                "agent_type": { "type": "string", "description": "Type/role of the agent (default: general-purpose)" }
            },
            "required": ["team_name", "agent_name"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let rx = self
            .channel
            .request(methods::ADD_AGENT, input)
            .await
            .map_err(ToolError::ExecutionFailed)?;

        let response = rx.await.map_err(|_| {
            ToolError::ExecutionFailed("Coordinator dropped response channel".into())
        })?;

        if let Some(ref error) = response.error {
            return Err(ToolError::ExecutionFailed(format!(
                "Add agent request failed: {error:?}"
            )));
        }

        let result = response.result.as_ref().unwrap_or(&json!(null));
        Ok(success_output(result.clone()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn coordinator_channel_default() {
        let channel = CoordinatorChannel::default();
        let _ = channel;
    }

    #[test]
    fn coordinator_channel_new() {
        let channel = CoordinatorChannel::new();
        let _ = channel;
    }

    #[test]
    fn success_output_helper() {
        let output = success_output(json!({"status": "ok"}));
        assert!(!output.is_error);
        assert!(output.content.contains("ok"));
    }

    #[test]
    fn remote_team_task_list_tool_schema() {
        let channel = CoordinatorChannel::new();
        let tool = RemoteTeamTaskListTool::new(channel);
        assert_eq!(tool.name(), "team_task_list");
        assert!(!tool.description().is_empty());
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
    }

    #[test]
    fn remote_team_task_claim_tool_schema() {
        let channel = CoordinatorChannel::new();
        let tool = RemoteTeamTaskClaimTool::new(channel, "worker-1".into());
        assert_eq!(tool.name(), "team_task_claim");
        let schema = tool.input_schema();
        assert!(schema["properties"]["task_id"].is_object());
    }

    #[test]
    fn remote_team_notify_idle_tool_schema() {
        let channel = CoordinatorChannel::new();
        let tool = RemoteTeamNotifyIdleTool::new(channel, "worker-1".into());
        assert_eq!(tool.name(), "team_notify_idle");
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
    }

    #[test]
    fn remote_send_message_tool_schema() {
        let channel = CoordinatorChannel::new();
        let tool = RemoteSendMessageTool::new(channel, "worker-1".into());
        assert_eq!(tool.name(), "team_send_message");
        let schema = tool.input_schema();
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("message"))
        );
    }

    #[test]
    fn remote_team_task_create_tool_schema() {
        let channel = CoordinatorChannel::new();
        let tool = RemoteTeamTaskCreateTool::new(channel);
        assert_eq!(tool.name(), "team_task_create");
        let schema = tool.input_schema();
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("subject"))
        );
    }

    #[test]
    fn remote_team_task_update_tool_schema() {
        let channel = CoordinatorChannel::new();
        let tool = RemoteTeamTaskUpdateTool::new(channel, "worker-1".into());
        assert_eq!(tool.name(), "team_task_update");
        let schema = tool.input_schema();
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("task_id"))
        );
    }

    #[test]
    fn remote_team_task_get_tool_schema() {
        let channel = CoordinatorChannel::new();
        let tool = RemoteTeamTaskGetTool::new(channel);
        assert_eq!(tool.name(), "team_task_get");
    }

    #[test]
    fn remote_team_manifest_tool_schema() {
        let channel = CoordinatorChannel::new();
        let tool = RemoteTeamManifestTool::new(channel);
        assert_eq!(tool.name(), "team_manifest");
    }

    #[test]
    fn remote_disband_team_tool_schema() {
        let channel = CoordinatorChannel::new();
        let tool = RemoteDisbandTeamTool::new(channel);
        assert_eq!(tool.name(), "disband_team");
    }

    #[test]
    fn remote_add_agent_tool_schema() {
        let channel = CoordinatorChannel::new();
        let tool = RemoteAddAgentTool::new(channel);
        assert_eq!(tool.name(), "add_agent");
    }

    #[test]
    fn send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CoordinatorChannel>();
    }
}
