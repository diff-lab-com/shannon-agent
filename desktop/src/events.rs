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
    DiffHunk, EVENT_SCHEMA_VERSION, EventEnvelope, HunkAction, PermissionRequest,
    QueryCancelledPayload, QueryCompletedPayload, QueryTextPayload, SessionInfo, SessionLoaded,
    TaskRetryPayload, TaskStepPayload, ThinkingPayload, ToolProgressPayload, ToolResultPayload,
    ToolStartPayload, UpdateAvailablePayload, UpdateProgressPayload, UsagePayload,
    VoiceModelDownloadProgressPayload,
};

/// `query:failed` payload `error_kind` for provider-authentication failures
/// (HTTP 401/403 — bad/revoked API key). The chat UI routes these to a
/// dedicated "update your key" banner instead of the raw error line.
pub const QUERY_ERROR_KIND_AUTH: &str = "auth";
/// `query:failed` payload `error_kind` for every other failure (network,
/// rate limit, provider outage, engine bug, …). Rendered as today.
pub const QUERY_ERROR_KIND_OTHER: &str = "other";

/// Classify a query-failure error string into the machine-readable
/// `error_kind` carried on the desktop `query:failed` payload.
///
/// The engine's stream pipeline hands the desktop only the error's
/// **Display string** (the exact string that lands in
/// `QueryEvent::Failed`), so the classification has to match text — same
/// trade-off the CLI's `classify_headless_failure` already makes. The
/// primary signal is the engine `ApiError::AuthenticationFailed` Display
/// text ("Authentication failed", `shannon-engine/src/api/error.rs`),
/// which is what an HTTP 401/403 from any provider collapses to; the
/// remaining phrases cover error strings that surface from other layers
/// (provider JSON bodies, gateway relays) without re-introducing a
/// typed-error dependency the stream doesn't carry.
pub(crate) fn classify_query_error_kind(error: &str) -> &'static str {
    let lower = error.to_lowercase();
    if lower.contains("authentication failed")
        || lower.contains("unauthorized")
        || lower.contains("invalid api key")
        || lower.contains("invalid x-api-key")
        || lower.contains("api key rejected")
        || lower.contains("check your api key")
    {
        QUERY_ERROR_KIND_AUTH
    } else {
        QUERY_ERROR_KIND_OTHER
    }
}

/// Desktop `query:failed` wire payload — the frozen engine
/// `QueryFailedPayload` shape (`shannon-types`, read-only for the desktop)
/// plus a desktop-only `error_kind` field, so the frontend can route
/// authentication failures without string matching in JS. Field names and
/// the first three fields mirror the engine type exactly; `error_kind` is
/// additive, so older frontends ignore it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DesktopQueryFailedPayload {
    pub query_id: String,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub error_kind: &'static str,
}

/// Build the desktop `query:failed` payload — classifies `error` once, at
/// the emit site, so every `QUERY_FAILED` emitter carries the same kind.
pub(crate) fn query_failed_payload(
    query_id: &str,
    error: &str,
    session_id: Option<String>,
) -> DesktopQueryFailedPayload {
    DesktopQueryFailedPayload {
        query_id: query_id.to_string(),
        error: error.to_string(),
        session_id,
        error_kind: classify_query_error_kind(error),
    }
}

/// Workflow streaming — helper to emit a `task:step` event from
/// anywhere with an `AppHandle`. Silently no-ops on emit error so a
/// frontend disconnect never breaks task execution.
#[cfg(feature = "tauri")]
#[allow(clippy::too_many_arguments)] // event helper: payload fields are inherent
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
        assert_eq!(
            event_names::SKILL_PROPOSAL_AVAILABLE,
            "skill-proposal-available"
        );
        // P3-6 — Task 3's frontend listens on this exact string.
        assert_eq!(event_names::TERMINAL_EXIT, "terminal:exit");
    }

    #[test]
    fn query_text_payload_round_trip() {
        let p = QueryTextPayload {
            query_id: "abc".into(),
            content: "hello".into(),
            session_id: Some("s1".into()),
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
            session_id: None,
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
            session_id: Some("s1".into()),
            reason: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(back["tool"], "bash");
        assert_eq!(back["risk"], "medium");
        assert_eq!(back["request_id"], "req-123");
        // P1-3: reason unset → stays off the wire and deserializes as None.
        assert!(back.get("reason").is_none());
    }

    // === query:failed error_kind classification (2026-09-29 provider review
    // §2-3: chat auth failures must route to the "update key" banner) ===

    #[test]
    fn classify_query_error_kind_matches_engine_auth_failure_text() {
        // The engine `ApiError::AuthenticationFailed` Display text — what an
        // HTTP 401/403 from any provider collapses to.
        assert_eq!(
            classify_query_error_kind("Authentication failed"),
            QUERY_ERROR_KIND_AUTH
        );
        // ... and its `user_suggestion` wording (some paths embed it).
        assert_eq!(
            classify_query_error_kind(
                "Authentication failed. Check your API key with /config or set SHANNON_API_KEY."
            ),
            QUERY_ERROR_KIND_AUTH
        );
        // Other layers' unauthorized phrasings.
        assert_eq!(
            classify_query_error_kind("invalid api key provided"),
            QUERY_ERROR_KIND_AUTH
        );
        assert_eq!(
            classify_query_error_kind("401 Unauthorized from upstream"),
            QUERY_ERROR_KIND_AUTH
        );
    }

    #[test]
    fn classify_query_error_kind_leaves_other_failures_unclassified() {
        assert_eq!(classify_query_error_kind("boom"), QUERY_ERROR_KIND_OTHER);
        assert_eq!(
            classify_query_error_kind("Rate limit exceeded"),
            QUERY_ERROR_KIND_OTHER
        );
        assert_eq!(
            classify_query_error_kind("error sending request"),
            QUERY_ERROR_KIND_OTHER
        );
        assert_eq!(classify_query_error_kind(""), QUERY_ERROR_KIND_OTHER);
    }

    #[test]
    fn query_failed_payload_carries_kind_and_mirrors_engine_fields() {
        let p = query_failed_payload("q-1", "Authentication failed", Some("s1".into()));
        assert_eq!(p.query_id, "q-1");
        assert_eq!(p.error, "Authentication failed");
        assert_eq!(p.session_id.as_deref(), Some("s1"));
        assert_eq!(p.error_kind, QUERY_ERROR_KIND_AUTH);
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"error_kind\":\"auth\""), "{json}");
        // The three engine-mirrored field names are unchanged.
        assert!(json.contains("\"query_id\":\"q-1\""), "{json}");
        assert!(json.contains("\"session_id\":\"s1\""), "{json}");
    }

    #[test]
    fn query_failed_payload_without_session_omits_field() {
        // Matches the engine payload's skip_serializing_if contract.
        let p = query_failed_payload("q-2", "boom", None);
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("session_id"), "{json}");
        assert!(json.contains("\"error_kind\":\"other\""), "{json}");
    }
}
