//! Typed event payloads for Tauri frontend events.
//!
//! These map directly from QueryEngine's QueryEvent variants to
//! JSON payloads emitted via `app_handle.emit()`.

use serde::{Deserialize, Serialize};

/// A streaming text chunk from the LLM.
#[derive(Debug, Clone, Serialize)]
pub struct QueryTextPayload {
    pub query_id: String,
    pub content: String,
}

/// A tool call has started.
#[derive(Debug, Clone, Serialize)]
pub struct ToolStartPayload {
    pub query_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
}

/// A tool call has completed.
#[derive(Debug, Clone, Serialize)]
pub struct ToolResultPayload {
    pub query_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub result: String,
    pub is_error: bool,
}

/// Tool progress update (e.g., bash command output).
#[derive(Debug, Clone, Serialize)]
pub struct ToolProgressPayload {
    pub query_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub progress: f32,
    pub message: String,
}

/// Extended thinking content.
#[derive(Debug, Clone, Serialize)]
pub struct ThinkingPayload {
    pub query_id: String,
    pub content: String,
}

/// Background task status and update.
#[derive(Debug, Clone, Serialize)]
pub struct BackgroundTaskUpdate {
    pub task_id: String,
    pub status: String, // "running", "completed", "failed"
    pub prompt: String,
    pub output: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
}

/// Background task info for listing.
#[derive(Debug, Clone, Serialize)]
pub struct BackgroundTaskInfo {
    pub task_id: String,
    pub prompt: String,
    pub status: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub output: String,
}

/// Token usage and cost update.
#[derive(Debug, Clone, Serialize)]
pub struct UsagePayload {
    pub query_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

/// Query completed successfully.
#[derive(Debug, Clone, Serialize)]
pub struct QueryCompletedPayload {
    pub query_id: String,
}

/// Query failed.
#[derive(Debug, Clone, Serialize)]
pub struct QueryFailedPayload {
    pub query_id: String,
    pub error: String,
}

/// Permission request for tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub tool: String,
    pub input: serde_json::Value,
    pub risk: String,
    pub request_id: String,
}

/// Session information for session list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub message_count: usize,
}

/// Session loaded event with messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLoaded {
    pub messages: Vec<ChatMessage>,
}

/// Chat message (replicated from commands.rs for event serialization).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

/// Query cancelled event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryCancelledPayload {
    pub query_id: String,
}

/// Config updated event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigUpdatedPayload {
    pub key: String,
    pub value: String,
}

/// Hunk action for applying diffs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HunkAction {
    pub line_start: u32,
    pub line_end: u32,
    pub action: String, // "accept" or "reject"
}

/// Update available event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAvailablePayload {
    pub version: String,
    pub date: Option<String>,
    pub body: Option<String>,
}

/// Update download progress event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProgressPayload {
    pub progress: f32, // 0.0 to 1.0
    pub status: String,
}

/// Diff file info for review panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffFileInfo {
    pub path: String,
    pub status: String, // "modified", "added", "deleted"
    pub hunks: Vec<DiffHunk>,
}

/// Diff hunk containing changed lines.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub content: String,
}

/// Tauri event names used in emit/listen.
pub mod event_names {
    pub const QUERY_TEXT: &str = "query:text";
    pub const QUERY_TOOL_START: &str = "query:tool-start";
    pub const QUERY_TOOL_RESULT: &str = "query:tool-result";
    pub const QUERY_TOOL_PROGRESS: &str = "query:tool-progress";
    pub const QUERY_THINKING: &str = "query:thinking";
    pub const QUERY_USAGE: &str = "query:usage";
    pub const QUERY_COMPLETED: &str = "query:completed";
    pub const QUERY_FAILED: &str = "query:failed";
    pub const QUERY_CANCELLED: &str = "query:cancelled";
    pub const PERMISSION_REQUEST: &str = "permission-request";
    pub const SESSIONS_UPDATED: &str = "sessions-updated";
    pub const SESSION_LOADED: &str = "session-loaded";
    pub const CONFIG_UPDATED: &str = "config-updated";
    pub const DIFF_REVIEW_AVAILABLE: &str = "diff-review-available";
    pub const BACKGROUND_TASK_UPDATE: &str = "background-task-update";
    pub const BACKGROUND_TASKS_UPDATED: &str = "background-tasks-updated";
    pub const UPDATE_AVAILABLE: &str = "update-available";
    pub const UPDATE_PROGRESS: &str = "update-progress";
    pub const UPDATE_COMPLETED: &str = "update-completed";
    pub const CHECK_UPDATES: &str = "check-updates";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_text_payload_serialization() {
        let p = QueryTextPayload {
            query_id: "abc".into(),
            content: "hello".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("abc"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn test_tool_start_payload_serialization() {
        let p = ToolStartPayload {
            query_id: "q1".into(),
            tool_use_id: "t1".into(),
            tool_name: "bash".into(),
            tool_input: serde_json::json!({"command": "ls"}),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("bash"));
        assert!(json.contains("ls"));
    }

    #[test]
    fn test_event_names_are_valid() {
        // Ensure event names are non-empty and follow the namespaced format
        assert!(!event_names::QUERY_TEXT.is_empty());
        assert!(event_names::QUERY_TEXT.contains(':'));
        assert!(event_names::QUERY_TOOL_START.contains(':'));
        assert!(event_names::QUERY_COMPLETED.contains(':'));
        assert!(event_names::QUERY_FAILED.contains(':'));
    }

    #[test]
    fn test_permission_request_serialization() {
        let req = PermissionRequest {
            tool: "bash".into(),
            input: serde_json::json!({"command": "ls"}),
            risk: "medium".into(),
            request_id: "req-123".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: PermissionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tool, "bash");
        assert_eq!(deserialized.risk, "medium");
        assert_eq!(deserialized.request_id, "req-123");
    }

    #[test]
    fn test_session_info_serialization() {
        let info = SessionInfo {
            id: "sess-1".into(),
            title: "My Chat".into(),
            created_at: 1700000000,
            message_count: 5,
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: SessionInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "sess-1");
        assert_eq!(deserialized.title, "My Chat");
        assert_eq!(deserialized.message_count, 5);
    }

    #[test]
    fn test_session_loaded_serialization() {
        let loaded = SessionLoaded {
            messages: vec![
                ChatMessage {
                    role: "user".into(),
                    content: "hello".into(),
                    timestamp: 100,
                },
                ChatMessage {
                    role: "assistant".into(),
                    content: "hi".into(),
                    timestamp: 101,
                },
            ],
        };
        let json = serde_json::to_string(&loaded).unwrap();
        let deserialized: SessionLoaded = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.messages.len(), 2);
        assert_eq!(deserialized.messages[0].role, "user");
    }

    #[test]
    fn test_chat_message_serialization() {
        let msg = ChatMessage {
            role: "user".into(),
            content: "test".into(),
            timestamp: 1700000000,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, "user");
        assert_eq!(deserialized.content, "test");
    }
}
