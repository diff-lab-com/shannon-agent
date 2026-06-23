use std::env;
use std::fs;
use std::path::Path;

// Type definitions with JsonSchema derives for build.rs
// These must match the types in src/events.rs exactly
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn main() {
    println!("cargo:rerun-if-changed=src/events.rs");

    // Generate schemas for all 23 payload structs + EventEnvelope
    let mut schemas = schemars::Map::new();

    // Query lifecycle events
    schemas.insert(
        "QueryTextPayload".to_string(),
        schemars::schema_for!(QueryTextPayload),
    );
    schemas.insert(
        "ToolStartPayload".to_string(),
        schemars::schema_for!(ToolStartPayload),
    );
    schemas.insert(
        "ToolResultPayload".to_string(),
        schemars::schema_for!(ToolResultPayload),
    );
    schemas.insert(
        "ToolProgressPayload".to_string(),
        schemars::schema_for!(ToolProgressPayload),
    );
    schemas.insert(
        "ThinkingPayload".to_string(),
        schemars::schema_for!(ThinkingPayload),
    );
    schemas.insert(
        "UsagePayload".to_string(),
        schemars::schema_for!(UsagePayload),
    );
    schemas.insert(
        "QueryCompletedPayload".to_string(),
        schemars::schema_for!(QueryCompletedPayload),
    );
    schemas.insert(
        "QueryFailedPayload".to_string(),
        schemars::schema_for!(QueryFailedPayload),
    );
    schemas.insert(
        "QueryCancelledPayload".to_string(),
        schemars::schema_for!(QueryCancelledPayload),
    );

    // Permission and session events
    schemas.insert(
        "PermissionRequest".to_string(),
        schemars::schema_for!(PermissionRequest),
    );
    schemas.insert(
        "SessionInfo".to_string(),
        schemars::schema_for!(SessionInfo),
    );
    schemas.insert(
        "SessionLoaded".to_string(),
        schemars::schema_for!(SessionLoaded),
    );
    schemas.insert(
        "ChatMessage".to_string(),
        schemars::schema_for!(ChatMessage),
    );

    // Background task events
    schemas.insert(
        "BackgroundTaskUpdate".to_string(),
        schemars::schema_for!(BackgroundTaskUpdate),
    );
    schemas.insert(
        "BackgroundTaskInfo".to_string(),
        schemars::schema_for!(BackgroundTaskInfo),
    );

    // Config and update events
    schemas.insert(
        "ConfigUpdatedPayload".to_string(),
        schemars::schema_for!(ConfigUpdatedPayload),
    );
    schemas.insert(
        "UpdateAvailablePayload".to_string(),
        schemars::schema_for!(UpdateAvailablePayload),
    );
    schemas.insert(
        "UpdateProgressPayload".to_string(),
        schemars::schema_for!(UpdateProgressPayload),
    );

    // Diff review events
    schemas.insert("HunkAction".to_string(), schemars::schema_for!(HunkAction));
    schemas.insert(
        "DiffFileInfo".to_string(),
        schemars::schema_for!(DiffFileInfo),
    );
    schemas.insert("DiffHunk".to_string(), schemars::schema_for!(DiffHunk));

    // Workflow streaming events
    schemas.insert(
        "TaskStepPayload".to_string(),
        schemars::schema_for!(TaskStepPayload),
    );
    schemas.insert(
        "TaskRetryPayload".to_string(),
        schemars::schema_for!(TaskRetryPayload),
    );

    // EventEnvelope (generic, using serde_json::Value as concrete type)
    schemas.insert(
        "EventEnvelope".to_string(),
        schemars::schema_for!(EventEnvelope<serde_json::Value>),
    );

    let schema_json = serde_json::to_string_pretty(&schemas).expect("schema serialization failed");

    // Write to OUT_DIR (build-time location)
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_path = Path::new(&out_dir).join("events.schema.json");
    fs::write(&out_path, &schema_json).expect("failed to write schema to OUT_DIR");

    // Also write to source tree (committed, for consumers without build)
    // Use CARGO_MANIFEST_DIR to get the crate directory
    let crate_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let src_path = Path::new(&crate_dir).join("schema/events.schema.json");
    fs::create_dir_all(src_path.parent().unwrap()).expect("failed to create schema directory");
    fs::write(&src_path, &schema_json).expect("failed to write schema to source tree");

    println!(
        "cargo:warning=Generated JSON Schema at: {}",
        src_path.display()
    );
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryTextPayload {
    pub query_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolStartPayload {
    pub query_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolResultPayload {
    pub query_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub result: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolProgressPayload {
    pub query_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub progress: f32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ThinkingPayload {
    pub query_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundTaskUpdate {
    pub task_id: String,
    pub status: String,
    pub prompt: String,
    pub output: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundTaskInfo {
    pub task_id: String,
    pub prompt: String,
    pub status: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UsagePayload {
    pub query_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryCompletedPayload {
    pub query_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryFailedPayload {
    pub query_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PermissionRequest {
    pub tool: String,
    pub input: serde_json::Value,
    pub risk: String,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionLoaded {
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryCancelledPayload {
    pub query_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigUpdatedPayload {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HunkAction {
    pub line_start: u32,
    pub line_end: u32,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateAvailablePayload {
    pub version: String,
    pub date: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateProgressPayload {
    pub progress: f32,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiffFileInfo {
    pub path: String,
    pub status: String,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskRetryPayload {
    pub task_id: String,
    pub run_id: String,
    pub attempt: usize,
    pub max_attempts: usize,
    pub delay_ms: u64,
    pub last_error: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EventEnvelope<T> {
    pub schema_version: u32,
    pub event: String,
    pub payload: T,
}
