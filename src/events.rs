//! Typed event payloads for Tauri frontend events.
//!
//! All payload structs and the `event_names` constants are defined in
//! `shannon_types::events` so the engine and shell share a single wire
//! contract. This module re-exports them for convenience inside the
//! desktop shell and adds Tauri-specific emit helpers that depend on
//! `tauri::AppHandle`.
//!
//! See `docs/architecture/d4-state-sync-protocol.md` for the schema
//! versioning contract.
//!
//! **`events::ChatMessage` vs `commands::ChatMessage`** — these are
//! distinct types with different roles, not duplicates:
//! - `events::ChatMessage` (re-exported here from `shannon_types::events`)
//!   is the wire format emitted to the frontend: 3 fields
//!   (`role`, `content`, `timestamp`).
//! - `commands::ChatMessage` (defined in `src/commands.rs`) is the
//!   app-internal representation that additionally carries
//!   `file_attachments`. Conversion happens at the IPC boundary inside
//!   `commands_sessions::load_session` and `switch_session`.

#[cfg(feature = "tauri")]
use tauri::Emitter;

pub use shannon_types::events::event_names;
pub use shannon_types::events::{
    BackgroundTaskInfo, BackgroundTaskUpdate, ChatMessage, ConfigUpdatedPayload, DiffFileInfo,
    DiffHunk, EventEnvelope, HunkAction, PermissionRequest, QueryCancelledPayload,
    QueryCompletedPayload, QueryFailedPayload, QueryTextPayload, SessionInfo, SessionLoaded,
    TaskRetryPayload, TaskStepPayload, ThinkingPayload, ToolProgressPayload, ToolResultPayload,
    ToolStartPayload, UpdateAvailablePayload, UpdateProgressPayload, UsagePayload,
    EVENT_SCHEMA_VERSION,
};

/// Workflow streaming — helper to emit a `task:step` event from
/// anywhere with an `AppHandle`. Silently no-ops on emit error so a
/// frontend disconnect never breaks task execution.
#[cfg(feature = "tauri")]
pub fn emit_task_step<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    task_id: &str,
    run_id: &str,
    step_index: usize,
    step_total: usize,
    step_label: &str,
    status: &str,
    error: Option<&str>,
) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let payload = TaskStepPayload {
        task_id: task_id.into(),
        run_id: run_id.into(),
        step_index,
        step_total,
        step_label: step_label.into(),
        status: status.into(),
        error: error.map(|s| s.into()),
        timestamp_ms,
    };
    let _ = app.emit(event_names::TASK_STEP, payload);
}

/// Workflow streaming — helper to emit a `task:retry` event.
#[cfg(feature = "tauri")]
pub fn emit_task_retry<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    task_id: &str,
    run_id: &str,
    attempt: usize,
    max_attempts: usize,
    delay_ms: u64,
    last_error: &str,
) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let payload = TaskRetryPayload {
        task_id: task_id.into(),
        run_id: run_id.into(),
        attempt,
        max_attempts,
        delay_ms,
        last_error: last_error.into(),
        timestamp_ms,
    };
    let _ = app.emit(event_names::TASK_RETRY, payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn re_exports_match_canonical_schema_version() {
        assert_eq!(EVENT_SCHEMA_VERSION, 1);
    }

    #[test]
    fn event_names_re_exported() {
        assert_eq!(event_names::QUERY_TEXT, "query:text");
        assert_eq!(event_names::TASK_STEP, "task:step");
        assert_eq!(event_names::TASK_RETRY, "task:retry");
        assert_eq!(event_names::SKILL_PROPOSAL_AVAILABLE, "skill-proposal-available");
    }

    #[test]
    fn query_text_payload_round_trip() {
        let p = QueryTextPayload {
            query_id: "abc".into(),
            content: "hello".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("abc"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn tool_start_payload_carries_input_value() {
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
    }

    #[test]
    fn permission_request_round_trip() {
        let req = PermissionRequest {
            tool: "bash".into(),
            input: serde_json::json!({"command": "ls"}),
            risk: "medium".into(),
            request_id: "req-123".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: PermissionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tool, "bash");
        assert_eq!(back.risk, "medium");
        assert_eq!(back.request_id, "req-123");
    }
}
