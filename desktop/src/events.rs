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
/// (HTTP 401 — bad/revoked API key). The chat UI routes these to a
/// dedicated "update your key" banner instead of the raw error line.
pub const QUERY_ERROR_KIND_AUTH: &str = "auth";
/// `query:failed` payload `error_kind` for HTTP 402 — provider quota or
/// credit exhausted. Routed to the "quota exhausted" banner with
/// update-key / view-usage recovery actions (S1-1, review P-N1).
pub const QUERY_ERROR_KIND_QUOTA: &str = "quota";
/// `query:failed` payload `error_kind` for HTTP 429 — rate limited.
/// Routed to the "rate limited" banner with a wait hint + Retry.
pub const QUERY_ERROR_KIND_RATE_LIMIT: &str = "rate_limit";
/// `query:failed` payload `error_kind` for HTTP 403 — the key is valid but
/// lacks permission for the model/resource. Routed to the "access denied"
/// banner pointing at Settings.
pub const QUERY_ERROR_KIND_AUTHZ: &str = "authz";
/// `query:failed` payload `error_kind` for every other failure (network,
/// timeout, 5xx, provider outage, engine bug, …). Rendered as today.
pub const QUERY_ERROR_KIND_OTHER: &str = "other";

// === R5-2 — failover / key-rotation notices on the wire ===
//
// The engine's retry observer (R3-1 / R4-3) renders its notices into
// `QueryEvent::Progress` messages ("falling back to X@Y (reason)" /
// "rotating API key (i/N) for P (reason)" — wording pinned by
// `agent_loop::format_retry_notice_message` tests). The desktop send loop
// used to drop Progress events entirely, so a successful failover looked
// like a silent stall. This desktop-local event (same pattern as
// `SESSION_AUTO_UNARCHIVED_EVENT`: no engine wire change) carries the
// recognized notices to the chat UI, which renders them as subtle
// system-style lines — NOT error banners; the request continued.

/// Desktop event name for in-stream retry notices (`query:notice`).
pub const QUERY_NOTICE_EVENT: &str = "query:notice";

/// Notice kind: the engine failed over to a fallback model/provider
/// (`R3-1`) and the request continued on the new target.
pub const QUERY_NOTICE_KIND_FAILOVER: &str = "failover";
/// Notice kind: the engine rotated to the provider's next API key
/// (`R4-3`) and the request continued.
pub const QUERY_NOTICE_KIND_KEY_ROTATION: &str = "key_rotation";

/// Classify an engine `QueryEvent::Progress` message into a desktop notice
/// kind, or `None` when the message is not a retry notice (plain progress
/// stays dropped, as before). Matches the engine's rendered line prefixes —
/// the exact wordings are pinned engine-side
/// (`agent_loop::format_retry_notice_message`), so prefix matching on the
/// lowercased text is stable.
pub(crate) fn classify_retry_notice(message: &str) -> Option<&'static str> {
    let lower = message.trim_start().to_lowercase();
    if lower.starts_with("falling back to ") {
        Some(QUERY_NOTICE_KIND_FAILOVER)
    } else if lower.starts_with("rotating api key (") {
        Some(QUERY_NOTICE_KIND_KEY_ROTATION)
    } else {
        None
    }
}

/// Desktop `query:notice` wire payload — the engine's raw notice line plus
/// the machine-readable kind the UI needs to pick icon + localized label.
/// `message` is the verbatim engine text (replayable from the L0 log).
#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryNoticePayload {
    pub query_id: String,
    /// One of [`QUERY_NOTICE_KIND_FAILOVER`] / [`QUERY_NOTICE_KIND_KEY_ROTATION`].
    pub kind: &'static str,
    /// The verbatim engine notice line, e.g.
    /// `falling back to glm-5.3-flash@zhipu (rate limited)`.
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Resolve the machine-readable `error_kind` carried on the desktop
/// `query:failed` payload (S1-1, review 2026-10-05 §3 P-N1).
///
/// The structured kind is **preferred**: the engine classifies at the emit
/// site where the typed error is in scope (`ApiError::error_kind()` —
/// typed status → kind, no string sniffing) and ships it on
/// `QueryEvent::Failed`. This function consumes it verbatim; the desktop
/// never re-derives a classification the engine already made.
///
/// The `error` text matching below is a **transitional fallback** only,
/// for emit paths that don't carry the field yet (stream `Err` payloads,
/// panic messages, engine events from before the field). Once every
/// engine failure path sets `error_kind`, the fallback — and this
/// function's text-sniffing branch — should be deleted.
///
/// An unknown structured kind (a future engine adding a value this shell
/// doesn't know) lands in `other`: the engine has spoken, so the text
/// fallback must NOT re-classify its decision; `other` keeps the plain
/// error banner, which is safe for any unseen kind.
pub(crate) fn classify_query_error_kind(engine_kind: Option<&str>, error: &str) -> &'static str {
    if let Some(kind) = engine_kind {
        return match kind {
            QUERY_ERROR_KIND_AUTH => QUERY_ERROR_KIND_AUTH,
            QUERY_ERROR_KIND_QUOTA => QUERY_ERROR_KIND_QUOTA,
            QUERY_ERROR_KIND_RATE_LIMIT => QUERY_ERROR_KIND_RATE_LIMIT,
            QUERY_ERROR_KIND_AUTHZ => QUERY_ERROR_KIND_AUTHZ,
            _ => QUERY_ERROR_KIND_OTHER,
        };
    }
    // === transitional text fallback (see doc comment) ===
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
/// failure classes without string matching in JS. Field names and the
/// first three fields mirror the engine type exactly; `error_kind` is
/// additive, so older frontends ignore it. The kind is the engine's
/// structured classification when the event carries one, else the
/// transitional text fallback (see `classify_query_error_kind`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct DesktopQueryFailedPayload {
    pub query_id: String,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub error_kind: &'static str,
}

// === Settings R3 T8 — desktop ask_user question round-trip ===
//
// `ask-user-request` / `ask-user-resolved` payloads. Desktop-only wire
// types (same pattern as `QueryNoticePayload`): the event *names* live in
// `shannon_types::events::event_names` so the shells agree on the strings,
// but the engine never emits these — only `DesktopQuestionHandler` does.

/// One selectable option of an `ask-user-request`. Mirrors
/// `shannon_tools::ask_user::QuestionOption` exactly (label + description).
#[derive(Debug, Clone, serde::Serialize)]
pub struct AskUserOptionPayload {
    pub label: String,
    pub description: String,
}

/// `ask-user-request` wire payload — one question from the engine's
/// `ask_user_question` tool, flattened from
/// `shannon_tools::ask_user::Question` plus the correlation id the answer
/// travels back through (`respond_ask_user(request_id, answers)`). The
/// handler asks one question at a time, so one event carries one question.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AskUserRequest {
    /// Correlation id — echoed by `respond_ask_user` and by the
    /// `ask-user-resolved` timeout event.
    pub request_id: String,
    /// The question text displayed to the user.
    pub question: String,
    /// Short label / chip shown alongside the question (may be empty).
    pub header: String,
    /// Selectable options; empty means free-form text only.
    pub options: Vec<AskUserOptionPayload>,
    /// Whether several options may be selected at once.
    pub multi_select: bool,
    /// Present only when 提问自动继续 is on: how long until the backend
    /// auto-answers (`timeout_ms`/1000 → the card's mm:ss countdown). Pure
    /// display — the backend drives the actual timeout; `None` = wait
    /// forever.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// `ask-user-resolved` wire payload — a pending question settled without an
/// answer (the auto-continue timeout fired). `timed_out` is always `true`
/// today; the field keeps the wire shape stable if the answered path ever
/// needs to broadcast resolution to every window too.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AskUserResolved {
    pub request_id: String,
    pub timed_out: bool,
}

/// Build the desktop `query:failed` payload — classifies the failure once,
/// at the emit site, so every `QUERY_FAILED` emitter carries the same
/// kind. `engine_kind` is the structured classification from the engine's
/// `QueryEvent::Failed` (`None` when the failure never had a typed error:
/// stream errors, panics).
pub(crate) fn query_failed_payload(
    query_id: &str,
    error: &str,
    session_id: Option<String>,
    engine_kind: Option<&str>,
) -> DesktopQueryFailedPayload {
    DesktopQueryFailedPayload {
        query_id: query_id.to_string(),
        error: error.to_string(),
        session_id,
        error_kind: classify_query_error_kind(engine_kind, error),
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
        // Settings R3 T8 — AskUserCard listens on these exact strings.
        assert_eq!(event_names::ASK_USER_REQUEST, "ask-user-request");
        assert_eq!(event_names::ASK_USER_RESOLVED, "ask-user-resolved");
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
            risk_reason: None,
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
    // §2-3: chat auth failures must route to the "update key" banner;
    // S1-1/P-N1: structured engine kinds take precedence, the text match
    // below is a transitional fallback) ===

    #[test]
    fn classify_query_error_kind_prefers_structured_engine_kind() {
        // The engine's structured kind wins verbatim — no text re-classification.
        for (kind, expected) in [
            (QUERY_ERROR_KIND_AUTH, QUERY_ERROR_KIND_AUTH),
            (QUERY_ERROR_KIND_QUOTA, QUERY_ERROR_KIND_QUOTA),
            (QUERY_ERROR_KIND_RATE_LIMIT, QUERY_ERROR_KIND_RATE_LIMIT),
            (QUERY_ERROR_KIND_AUTHZ, QUERY_ERROR_KIND_AUTHZ),
            (QUERY_ERROR_KIND_OTHER, QUERY_ERROR_KIND_OTHER),
        ] {
            assert_eq!(
                classify_query_error_kind(Some(kind), "irrelevant text"),
                expected,
                "structured kind '{kind}' must pass through"
            );
        }
        // Even auth-worded text must NOT override a structured decision.
        assert_eq!(
            classify_query_error_kind(
                Some(QUERY_ERROR_KIND_QUOTA),
                "Authentication failed: insufficient balance"
            ),
            QUERY_ERROR_KIND_QUOTA
        );
        // A future engine kind this shell doesn't know lands in `other`
        // (never text-reclassified, never dropped).
        assert_eq!(
            classify_query_error_kind(Some("model_overloaded"), "boom"),
            QUERY_ERROR_KIND_OTHER
        );
    }

    #[test]
    fn classify_query_error_kind_falls_back_to_text_without_engine_kind() {
        // Transitional: emit paths without a typed error (stream Err, panic).
        assert_eq!(
            classify_query_error_kind(None, "Authentication failed"),
            QUERY_ERROR_KIND_AUTH
        );
        assert_eq!(
            classify_query_error_kind(None, "invalid api key provided"),
            QUERY_ERROR_KIND_AUTH
        );
        assert_eq!(
            classify_query_error_kind(None, "401 Unauthorized from upstream"),
            QUERY_ERROR_KIND_AUTH
        );
        // The fallback stays auth/other only — quota/rate-limit/authz wording
        // deliberately does NOT classify here (no string sniffing beyond the
        // legacy auth vocabulary).
        assert_eq!(
            classify_query_error_kind(None, "Insufficient Balance"),
            QUERY_ERROR_KIND_OTHER
        );
        assert_eq!(
            classify_query_error_kind(None, "boom"),
            QUERY_ERROR_KIND_OTHER
        );
        assert_eq!(classify_query_error_kind(None, ""), QUERY_ERROR_KIND_OTHER);
    }

    #[test]
    fn classify_query_error_kind_matches_legacy_auth_fallback_text() {
        // Pre-S1-1 behaviour kept verbatim for the fallback branch.
        assert_eq!(
            classify_query_error_kind(
                None,
                "Authentication failed. Check your API key with /config or set SHANNON_API_KEY."
            ),
            QUERY_ERROR_KIND_AUTH
        );
    }

    #[test]
    fn desktop_error_kind_constants_match_engine_canonical_set() {
        // Single source: the engine's `error_kind` module defines the wire
        // values; the desktop constants must never drift from them.
        use shannon_engine::api::error::error_kind as engine_kinds;
        assert_eq!(QUERY_ERROR_KIND_AUTH, engine_kinds::AUTH);
        assert_eq!(QUERY_ERROR_KIND_QUOTA, engine_kinds::QUOTA);
        assert_eq!(QUERY_ERROR_KIND_RATE_LIMIT, engine_kinds::RATE_LIMIT);
        assert_eq!(QUERY_ERROR_KIND_AUTHZ, engine_kinds::AUTHZ);
        assert_eq!(QUERY_ERROR_KIND_OTHER, engine_kinds::OTHER);
    }

    #[test]
    fn query_failed_payload_carries_kind_and_mirrors_engine_fields() {
        let p = query_failed_payload(
            "q-1",
            "Insufficient Balance",
            Some("s1".into()),
            Some(QUERY_ERROR_KIND_QUOTA),
        );
        assert_eq!(p.query_id, "q-1");
        assert_eq!(p.error, "Insufficient Balance");
        assert_eq!(p.session_id.as_deref(), Some("s1"));
        assert_eq!(p.error_kind, QUERY_ERROR_KIND_QUOTA);
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"error_kind\":\"quota\""), "{json}");
        // The three engine-mirrored field names are unchanged.
        assert!(json.contains("\"query_id\":\"q-1\""), "{json}");
        assert!(json.contains("\"session_id\":\"s1\""), "{json}");
    }

    #[test]
    fn query_failed_payload_without_session_omits_field() {
        // Matches the engine payload's skip_serializing_if contract.
        let p = query_failed_payload("q-2", "boom", None, None);
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("session_id"), "{json}");
        assert!(json.contains("\"error_kind\":\"other\""), "{json}");
    }

    // === Settings R3 T8: ask-user payloads ===

    #[test]
    fn ask_user_request_serializes_flat_question_fields() {
        let p = AskUserRequest {
            request_id: "req-1".into(),
            question: "Deploy now?".into(),
            header: "Confirm".into(),
            options: vec![
                AskUserOptionPayload {
                    label: "Yes".into(),
                    description: "Ship it".into(),
                },
                AskUserOptionPayload {
                    label: "No".into(),
                    description: String::new(),
                },
            ],
            multi_select: false,
            timeout_ms: Some(300_000),
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(back["request_id"], "req-1");
        assert_eq!(back["question"], "Deploy now?");
        assert_eq!(back["header"], "Confirm");
        assert_eq!(back["options"][0]["label"], "Yes");
        assert_eq!(back["multi_select"], false);
        assert_eq!(back["timeout_ms"], 300_000);
    }

    #[test]
    fn ask_user_request_omits_timeout_when_waiting_forever() {
        let p = AskUserRequest {
            request_id: "req-2".into(),
            question: "Name?".into(),
            header: String::new(),
            options: Vec::new(),
            multi_select: false,
            timeout_ms: None,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("timeout_ms"), "{json}");
    }

    #[test]
    fn ask_user_resolved_carries_request_id_and_timeout_flag() {
        let p = AskUserResolved {
            request_id: "req-3".into(),
            timed_out: true,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(back["request_id"], "req-3");
        assert_eq!(back["timed_out"], true);
    }

    // === R5-2: retry-notice classification (failover / key rotation) ===

    #[test]
    fn classify_retry_notice_recognizes_engine_failover_line() {
        // Exact renderings from `agent_loop::format_retry_notice_message`.
        assert_eq!(
            classify_retry_notice("falling back to glm-5.3-flash@zhipu (rate limited)"),
            Some(QUERY_NOTICE_KIND_FAILOVER)
        );
        // Case/padding tolerance for future rewording drift.
        assert_eq!(
            classify_retry_notice("  Falling Back to gpt-5@openai (5xx)"),
            Some(QUERY_NOTICE_KIND_FAILOVER)
        );
    }

    #[test]
    fn classify_retry_notice_recognizes_engine_key_rotation_line() {
        assert_eq!(
            classify_retry_notice("rotating API key (2/3) for openai (429 rate limited)"),
            Some(QUERY_NOTICE_KIND_KEY_ROTATION)
        );
    }

    #[test]
    fn classify_retry_notice_leaves_plain_progress_unclassified() {
        // Plain API retries, engine bookkeeping lines and junk stay dropped
        // (unchanged pre-R5-2 behaviour for non-notice progress).
        assert_eq!(
            classify_retry_notice("API retry 1/5 (next try in 2s): rate limited"),
            None
        );
        assert_eq!(
            classify_retry_notice("Truncated history to 10 messages"),
            None
        );
        assert_eq!(classify_retry_notice(""), None);
        assert_eq!(classify_retry_notice("fallback plan engaged"), None);
    }

    #[test]
    fn query_notice_payload_serializes_kind_and_raw_line() {
        let p = QueryNoticePayload {
            query_id: "q-1".into(),
            kind: QUERY_NOTICE_KIND_FAILOVER,
            message: "falling back to glm-5.3-flash@zhipu (rate limited)".into(),
            session_id: Some("s1".into()),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"kind\":\"failover\""), "{json}");
        assert!(json.contains("falling back to"), "{json}");
        // session_id follows the engine payloads' omit-when-none contract.
        let no_session = QueryNoticePayload {
            query_id: "q-1".into(),
            kind: QUERY_NOTICE_KIND_KEY_ROTATION,
            message: "rotating API key (1/2) for anthropic (401)".into(),
            session_id: None,
        };
        let json = serde_json::to_string(&no_session).unwrap();
        assert!(!json.contains("session_id"), "{json}");
    }
}
