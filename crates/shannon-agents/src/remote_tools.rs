//! Remote team tools for process-based agents.
//!
//! These tools communicate with the coordinator via JSON-RPC over stdin/stdout,
//! enabling process-isolated agents to interact with the shared task board
//! and send messages to teammates.

use async_trait::async_trait;
use serde_json::{json, Value};
use shannon_core::tools::{Tool, ToolError, ToolOutput, ToolResult};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::{Mutex, oneshot};

use crate::protocol::{frame_message, JsonRpcMessage, methods};

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
        writer.write_all(line.as_bytes()).await.map_err(|e| e.to_string())?;
        writer.flush().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Send a JSON-RPC request and return a oneshot receiver for the response.
    pub async fn request(&self, method: &str, params: Value) -> Result<oneshot::Receiver<JsonRpcMessage>, String> {
        let id = self.next_id();
        let msg = JsonRpcMessage::request(method, params, id);
        let line = frame_message(&msg).map_err(|e| e.to_string())?;

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        let mut writer = self.writer.lock().await;
        writer.write_all(line.as_bytes()).await.map_err(|e| e.to_string())?;
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

        let rx = self.channel.request(methods::LIST_TASKS, params)
            .await.map_err(|e| ToolError::ExecutionFailed(e))?;

        let response = rx.await
            .map_err(|_| ToolError::ExecutionFailed("Coordinator dropped response channel".into()))?;

        // The response result contains the task list
        if let Some(ref error) = response.error {
            return Err(ToolError::ExecutionFailed(format!("Coordinator error: {error:?}")));
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
        Self { channel, agent_name }
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

        let rx = self.channel.request(methods::CLAIM_TASK, params)
            .await.map_err(|e| ToolError::ExecutionFailed(e))?;

        let response = rx.await
            .map_err(|_| ToolError::ExecutionFailed("Coordinator dropped response channel".into()))?;

        if let Some(ref error) = response.error {
            return Err(ToolError::ExecutionFailed(format!("Claim failed: {error:?}")));
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
        Self { channel, agent_name }
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
        self.channel.notify(methods::AGENT_IDLE, json!({
            "agent_name": self.agent_name,
            "available_tasks_count": 0,
        })).await.map_err(|e| ToolError::ExecutionFailed(e))?;

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
        Self { channel, agent_name }
    }
}

#[async_trait]
impl Tool for RemoteSendMessageTool {
    fn name(&self) -> &str {
        "team_send_message"
    }

    fn description(&self) -> &str {
        "Send a message to a teammate or broadcast to all teammates. \
         Use 'to' for a specific agent or omit for broadcast."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": {
                    "type": "string",
                    "description": "Recipient agent name (omit for broadcast)"
                },
                "message": {
                    "type": "string",
                    "description": "Message content"
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

        let params = json!({
            "from": self.agent_name,
            "to": input.get("to").and_then(|v| v.as_str()).unwrap_or("*"),
            "message": message,
            "priority": input.get("priority").and_then(|v| v.as_str()).unwrap_or("normal"),
        });

        self.channel.notify(methods::SEND_MESSAGE, params)
            .await.map_err(|e| ToolError::ExecutionFailed(e))?;

        Ok(success_output(json!({
            "status": "sent",
            "from": self.agent_name,
        })))
    }
}
