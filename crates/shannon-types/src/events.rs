//! Wire-format event payloads shared between the Shannon engine and shells.
//!
//! These types live in `shannon-types` so the engine (which has no Tauri
//! dependency) and any shell (Tauri-based, CLI, or future surfaces) can
//! agree on a single serialization contract. Shells re-export them and
//! add Tauri-specific emit helpers; they must not change the field names
//! or shapes without bumping [`EVENT_SCHEMA_VERSION`].
//!
//! ## Schema versioning
//!
//! Every event implicitly carries schema version 1 — the wire format in
//! use since the events module was introduced. When a payload field is
//! renamed, removed, or has its type changed, bump
//! [`EVENT_SCHEMA_VERSION`] and document the migration in
//! `docs/architecture/d4-state-sync-protocol.md`.
//!
//! Old shells that receive a newer event silently ignore unknown fields
//! (serde's default behavior for `Deserialize`). New shells that receive
//! an older event fill missing fields with `serde(default)`. This avoids
//! the need for a startup capability handshake.

use serde::{Deserialize, Serialize};

/// Current event payload schema version. Bump on any breaking change to
/// a payload's field names or types.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Envelope wrapping an event with its schema version.
///
/// Not used by existing events (which are emitted bare for backwards
/// compatibility). New event families that need forward-compatible
/// negotiation should emit `EventEnvelope` instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    /// Schema version of the payload. Compare against
    /// [`EVENT_SCHEMA_VERSION`] on the receiver.
    pub schema_version: u32,
    /// Event name, e.g. `"query:text"`. Mirrors the value passed to
    /// `app.emit(name, payload)` on the Tauri side.
    pub event: String,
    /// The typed payload. Shells that don't know the schema should
    /// deserialize into `serde_json::Value` and ignore.
    pub payload: T,
}

impl<T> EventEnvelope<T> {
    pub fn new(event: impl Into<String>, payload: T) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            event: event.into(),
            payload,
        }
    }
}

/// A streaming text chunk from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryTextPayload {
    pub query_id: String,
    pub content: String,
}

/// A tool call has started.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStartPayload {
    pub query_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
}

/// A tool call has completed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultPayload {
    pub query_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub result: String,
    pub is_error: bool,
}

/// Tool progress update (e.g., bash command output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgressPayload {
    pub query_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub progress: f32,
    pub message: String,
}

/// Extended thinking content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingPayload {
    pub query_id: String,
    pub content: String,
}

/// Background task status and update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTaskUpdate {
    pub task_id: String,
    pub status: String,
    pub prompt: String,
    pub output: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
}

/// Background task info for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTaskInfo {
    pub task_id: String,
    pub prompt: String,
    pub status: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub output: String,
}

/// Token usage and cost update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsagePayload {
    pub query_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

/// Query completed successfully.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryCompletedPayload {
    pub query_id: String,
}

/// Query failed.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_point: Option<usize>,
}

/// Session loaded event with messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLoaded {
    pub messages: Vec<ChatMessage>,
}

/// Chat message (wire format for event serialization).
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
    pub action: String,
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
    pub progress: f32,
    pub status: String,
}

/// Diff file info for review panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffFileInfo {
    pub path: String,
    pub status: String,
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

/// Workflow streaming — a task has moved to a new step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStepPayload {
    pub task_id: String,
    pub run_id: String,
    pub step_index: usize,
    pub step_total: usize,
    pub step_label: String,
    pub status: String,
    pub error: Option<String>,
    pub timestamp_ms: u64,
}

/// Workflow streaming — a task is being auto-retried after a failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRetryPayload {
    pub task_id: String,
    pub run_id: String,
    pub attempt: usize,
    pub max_attempts: usize,
    pub delay_ms: u64,
    pub last_error: String,
    pub timestamp_ms: u64,
}

/// Event name constants. Kept here so the engine and all shells agree
/// on the exact strings emitted on the wire.
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
    pub const TASK_STEP: &str = "task:step";
    pub const TASK_RETRY: &str = "task:retry";
    pub const SKILL_PROPOSAL_AVAILABLE: &str = "skill-proposal-available";
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn event_names_use_colon_separator() {
        assert!(event_names::QUERY_TEXT.contains(':'));
        assert!(event_names::TASK_STEP.contains(':'));
        assert!(event_names::TASK_RETRY.contains(':'));
    }

    #[test]
    fn envelope_round_trip() {
        let payload = QueryTextPayload {
            query_id: "q1".into(),
            content: "hello".into(),
        };
        let env = EventEnvelope::new(event_names::QUERY_TEXT, payload);
        let json = serde_json::to_string(&env).unwrap();
        let back: EventEnvelope<QueryTextPayload> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(back.event, event_names::QUERY_TEXT);
        assert_eq!(back.payload.query_id, "q1");
    }

    #[test]
    fn task_step_payload_round_trip() {
        let p = TaskStepPayload {
            task_id: "t1".into(),
            run_id: "r1".into(),
            step_index: 2,
            step_total: 5,
            step_label: "Running query".into(),
            status: "started".into(),
            error: None,
            timestamp_ms: 1700000000,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: TaskStepPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task_id, "t1");
        assert_eq!(back.step_index, 2);
        assert_eq!(back.status, "started");
        assert!(back.error.is_none());
    }

    #[test]
    fn session_info_skips_optional_fields_when_none() {
        let info = SessionInfo {
            id: "s1".into(),
            title: "T".into(),
            created_at: 1,
            message_count: 0,
            working_dir: None,
            parent_id: None,
            branch_point: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("working_dir"));
        assert!(!json.contains("parent_id"));
        assert!(!json.contains("branch_point"));
    }

    #[test]
    fn event_envelope_validates_against_schema() {
        // Load the generated JSON Schema
        let schema_path = concat!(env!("CARGO_MANIFEST_DIR"), "/schema/events.schema.json");
        let schema_content = fs::read_to_string(schema_path).expect("Failed to read schema file");

        let schemas: serde_json::Value =
            serde_json::from_str(&schema_content).expect("Failed to parse schema JSON");

        // Get the EventEnvelope schema
        let envelope_schema = schemas
            .get("EventEnvelope")
            .expect("EventEnvelope schema not found");

        // Create a sample envelope
        let payload = QueryTextPayload {
            query_id: "test-q123".into(),
            content: "Hello, world!".into(),
        };
        let envelope = EventEnvelope::new("query:text", payload);

        // Serialize to JSON
        let envelope_json = serde_json::to_value(envelope).expect("Failed to serialize envelope");

        // Verify the JSON matches the schema structure
        let obj = envelope_json.as_object().expect("Not an object");
        assert!(obj.contains_key("schema_version"));
        assert!(obj.contains_key("event"));
        assert!(obj.contains_key("payload"));

        // Verify against schema properties
        let schema_props = envelope_schema
            .get("properties")
            .and_then(|p| p.as_object())
            .expect("Schema has no properties");

        assert!(schema_props.contains_key("schema_version"));
        assert!(schema_props.contains_key("event"));
        assert!(schema_props.contains_key("payload"));

        // Verify the schema_version matches current version
        assert_eq!(
            obj.get("schema_version").and_then(|v| v.as_u64()),
            Some(EVENT_SCHEMA_VERSION as u64)
        );
    }
}
