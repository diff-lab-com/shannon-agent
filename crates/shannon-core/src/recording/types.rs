//! Recording types for session capture and replay.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Metadata for a recorded session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecordingMeta {
    pub session_id: String,
    pub model: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub total_turns: usize,
    pub total_tokens: u64,
    pub total_tool_calls: usize,
}

/// A single entry in a session recording (one JSONL line).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RecordingEntry {
    /// Session started.
    SessionStart {
        session_id: String,
        model: String,
        timestamp: String,
    },
    /// User sent a message.
    UserMessage {
        content: String,
        turn: usize,
    },
    /// LLM API request.
    LlmRequest {
        turn: usize,
        request_hash: String,
        body: Value,
    },
    /// LLM API response.
    LlmResponse {
        turn: usize,
        body: Value,
    },
    /// Query engine event (text, tool use, progress, etc.).
    QueryEvent {
        event: crate::QueryEvent,
    },
    /// Tool call with input, output, and timing.
    ToolCall {
        tool: String,
        input: Value,
        result: String,
        is_error: bool,
        duration_ms: u64,
    },
    /// Session ended.
    SessionEnd {
        session_id: String,
        total_turns: usize,
        total_tokens: u64,
    },
}
