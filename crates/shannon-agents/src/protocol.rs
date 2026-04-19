//! JSON-RPC protocol types for coordinator-agent IPC communication.
//!
//! Agents running as separate processes communicate with the coordinator
//! via JSON-RPC 2.0 over stdin/stdout. This module defines the shared
//! protocol types used by both sides.

use serde::{Deserialize, Serialize};

// ── JSON-RPC 2.0 envelope ─────────────────────────────────────────

/// A JSON-RPC 2.0 request or notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcMessage {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<JsonRpcId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC ID — can be a number or a string.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcId {
    Number(i64),
    String(String),
}

/// JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// Standard JSON-RPC error codes
impl JsonRpcError {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;

    pub fn not_found(method: &str) -> Self {
        Self {
            code: Self::METHOD_NOT_FOUND,
            message: format!("Method not found: {method}"),
            data: None,
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: Self::INTERNAL_ERROR,
            message: msg.into(),
            data: None,
        }
    }
}

impl JsonRpcMessage {
    /// Create a request (has ID, expects response).
    pub fn request(method: impl Into<String>, params: serde_json::Value, id: i64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: Some(method.into()),
            params: Some(params),
            id: Some(JsonRpcId::Number(id)),
            result: None,
            error: None,
        }
    }

    /// Create a notification (no ID, no response expected).
    pub fn notification(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: Some(method.into()),
            params: Some(params),
            id: None,
            result: None,
            error: None,
        }
    }

    /// Create a success response.
    pub fn response(id: JsonRpcId, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: None,
            params: None,
            id: Some(id),
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response.
    pub fn error_response(id: JsonRpcId, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: None,
            params: None,
            id: Some(id),
            result: None,
            error: Some(error),
        }
    }

    /// Check if this is a notification (no ID).
    pub fn is_notification(&self) -> bool {
        self.id.is_none() && self.method.is_some()
    }

    /// Check if this is a request (has ID, has method).
    pub fn is_request(&self) -> bool {
        self.id.is_some() && self.method.is_some()
    }

    /// Check if this is a response (no method, has result or error).
    pub fn is_response(&self) -> bool {
        self.method.is_none()
    }

    /// Get the method name, if present.
    pub fn method(&self) -> Option<&str> {
        self.method.as_deref()
    }
}

// ── Coordinator → Agent methods ───────────────────────────────────

/// Parameters for the `execute_task` RPC (coordinator → agent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteTaskParams {
    pub task_id: String,
    pub subject: String,
    pub description: String,
    #[serde(default)]
    pub priority: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
}

/// Parameters for the `shutdown` notification (coordinator → agent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShutdownParams {
    pub reason: String,
}

// ── Agent → Coordinator methods ───────────────────────────────────

/// Parameters for the `agent_ready` notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentReadyParams {
    pub agent_name: String,
    pub capabilities: Vec<String>,
}

/// Parameters for the `task_progress` notification (streaming output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgressParams {
    pub task_id: String,
    pub chunk: String,
}

/// Parameters for the `task_complete` notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCompleteParams {
    pub task_id: String,
    pub success: bool,
    pub output: String,
}

/// Parameters for the `agent_idle` notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdleParams {
    pub agent_name: String,
    pub available_tasks_count: usize,
}

/// Parameters for the `claim_task` request (agent asks to claim a task).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimTaskParams {
    pub agent_name: String,
    pub team_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

/// Result of a successful `claim_task` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimTaskResult {
    /// The claimed task, if any was available.
    pub task: Option<ExecuteTaskParams>,
}

/// Parameters for the `send_message` request (agent → agent via coordinator).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageParams {
    pub from: String,
    pub to: String,
    pub content: String,
    pub team_name: String,
}

/// Parameters for the `list_tasks` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTasksParams {
    pub team_name: String,
    pub agent_name: String,
}

/// Result of a `list_tasks` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTasksResult {
    pub tasks: Vec<TaskSummary>,
}

/// Summary of a task sent over IPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub subject: String,
    pub status: String,
    pub owner: Option<String>,
}

// ── Well-known method names ───────────────────────────────────────

pub mod methods {
    // Coordinator → Agent
    pub const EXECUTE_TASK: &str = "execute_task";
    pub const SHUTDOWN: &str = "shutdown";
    #[allow(dead_code)]
    pub const PING: &str = "ping";

    // Agent → Coordinator
    pub const AGENT_READY: &str = "agent_ready";
    pub const TASK_PROGRESS: &str = "task_progress";
    pub const TASK_COMPLETE: &str = "task_complete";
    pub const AGENT_IDLE: &str = "agent_idle";
    pub const CLAIM_TASK: &str = "claim_task";
    pub const SEND_MESSAGE: &str = "send_message";
    pub const LIST_TASKS: &str = "list_tasks";
    pub const CREATE_TASK: &str = "create_task";
    pub const UPDATE_TASK: &str = "update_task";
    pub const GET_TASK: &str = "get_task";
    pub const TEAM_MANIFEST: &str = "team_manifest";
    pub const DISBAND_TEAM: &str = "disband_team";
    pub const ADD_AGENT: &str = "add_agent";
}

// ── Helper: line-delimited JSON transport framing ─────────────────

/// Frame a JSON-RPC message as a single line for stdin/stdout transport.
pub fn frame_message(msg: &JsonRpcMessage) -> Result<String, serde_json::Error> {
    let mut line = serde_json::to_string(msg)?;
    line.push('\n');
    Ok(line)
}

/// Parse a line-delimited JSON-RPC message.
pub fn parse_message(line: &str) -> Result<JsonRpcMessage, serde_json::Error> {
    serde_json::from_str(line.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let msg = JsonRpcMessage::request(
            methods::EXECUTE_TASK,
            serde_json::to_value(ExecuteTaskParams {
                task_id: "abc-123".to_string(),
                subject: "Fix bug".to_string(),
                description: "Fix the login bug".to_string(),
                priority: "High".to_string(),
                active_form: Some("Fixing login bug".to_string()),
            }).unwrap(),
            1,
        );
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"execute_task\""));
        assert!(json.contains("\"abc-123\""));

        let parsed: JsonRpcMessage = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_request());
        assert_eq!(parsed.method(), Some("execute_task"));
    }

    #[test]
    fn test_notification_serialization() {
        let msg = JsonRpcMessage::notification(
            methods::AGENT_READY,
            serde_json::to_value(AgentReadyParams {
                agent_name: "worker-1".to_string(),
                capabilities: vec!["rust".to_string()],
            }).unwrap(),
        );
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"agent_ready\""));
        assert!(!json.contains("\"id\""));

        let parsed: JsonRpcMessage = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_notification());
    }

    #[test]
    fn test_response_serialization() {
        let msg = JsonRpcMessage::response(
            JsonRpcId::Number(1),
            serde_json::to_value(ClaimTaskResult { task: None }).unwrap(),
        );
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"result\""));

        let parsed: JsonRpcMessage = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_response());
    }

    #[test]
    fn test_error_response() {
        let msg = JsonRpcMessage::error_response(
            JsonRpcId::Number(42),
            JsonRpcError::not_found("bogus"),
        );
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"error\""));
        assert!(json.contains("-32601"));
    }

    #[test]
    fn test_frame_roundtrip() {
        let msg = JsonRpcMessage::notification(
            methods::TASK_PROGRESS,
            serde_json::json!({"task_id": "t1", "chunk": "halfway"}),
        );
        let framed = frame_message(&msg).unwrap();
        assert!(framed.ends_with('\n'));

        let parsed = parse_message(&framed).unwrap();
        assert_eq!(parsed.method(), Some("task_progress"));
    }
}
