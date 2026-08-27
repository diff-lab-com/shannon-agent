//! # Session Log (L0 unified event log)
//!
//! The append-only JSONL log of typed [`SessionEvent`]s — the single
//! authoritative record of a session. Built on the vocabulary in
//! `shannon_types::session_event` (plan §4.1, W1-P0a):
//!
//! - [`SessionLogWriter`]: the *only* writer. Exclusive append handle
//!   (`flock`), tail recovery on open, seq = events written so far, and
//!   crash-tolerant degradation (count + warn, never fail the session).
//! - [`SessionLogReader`]: streaming line-by-line reader. Unknown event
//!   kinds are rejected by default; skipping requires an explicit opt-in.
//! - [`query_event_to_session_body`]: pure mapping from engine
//!   [`QueryEvent`]s to log bodies, prepared for the §4.2 tee injection.
//!
//! Storage layout: `~/.shannon/sessions/<session_id>/events.jsonl` — one
//! directory per session, leaving room for projections and metadata.

pub mod l0_subscriber;
pub mod projections;
pub mod reader;
pub mod session_store;
pub mod tee;
pub mod writer;

pub use l0_subscriber::L0TeeSubscriber;
pub use projections::{
    ConversationProjection, SearchHit, SessionAnalytics, SessionScanEntry, ToolAggregate,
    cutoff_seq_for_message_index, project_analytics_jsonl, project_conversation,
    project_permission_decisions, project_session_analytics, scan_session_summaries, search_events,
};
pub use reader::{SessionEventIter, SessionLogReader};
pub use session_store::{
    SessionSidecar, SessionStore, SessionStoreError, StoredSession, StoredSessionInfo,
    StoredSessionMeta, default_store,
};
pub use tee::{SessionTee, TeeHandle};
pub use writer::{FlushPolicy, SessionLogWriter};

use std::path::{Path, PathBuf};

use shannon_types::session_event::{
    AssistantChunkPayload, ErrorPayload, SessionEventBody, TokenUsage, ToolCallPayload,
    ToolResultPayload, TurnEndPayload, TurnStartPayload,
};

use crate::QueryEvent;

// ============================================================================
// Errors
// ============================================================================

/// Errors that can occur while reading or opening session logs.
///
/// Note: [`SessionLogWriter::record`] is deliberately infallible — write
/// failures degrade to a failure counter plus a warning so a log problem can
/// never take down a session. Only opening (locking) fails hard.
#[derive(thiserror::Error, Debug)]
pub enum SessionLogError {
    /// Underlying I/O failure.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// An event failed to serialize.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// A line referenced an event kind outside the vocabulary. Rejected by
    /// default (required semantics); readers may explicitly opt in to skip.
    #[error("unknown event kind `{kind}` at line {line}")]
    UnknownEvent {
        /// 1-based line number in the JSONL file.
        line: usize,
        /// The unrecognized kind string.
        kind: String,
    },

    /// A line was not valid JSON or did not match its declared kind's payload.
    #[error("malformed event at line {line}: {message}")]
    MalformedEvent {
        /// 1-based line number in the JSONL file.
        line: usize,
        /// What went wrong.
        message: String,
    },

    /// Another writer holds the exclusive lock on this log.
    #[error("session log is already locked by another writer: {path}")]
    AlreadyLocked {
        /// The contested events file.
        path: PathBuf,
        /// The underlying lock error.
        #[source]
        source: std::io::Error,
    },

    /// The log file does not exist.
    #[error("session log not found: {0}")]
    NotFound(PathBuf),

    /// The session storage directory could not be resolved.
    #[error("session storage root not initialized")]
    NotInitialized,
}

// ============================================================================
// Storage paths
// ============================================================================

/// Resolve the events file for `session_id` under a Shannon home directory:
/// `<base>/sessions/<session_id>/events.jsonl`.
pub fn session_events_path(base_dir: &Path, session_id: &str) -> PathBuf {
    base_dir
        .join("sessions")
        .join(session_id)
        .join("events.jsonl")
}

/// Resolve the events file when `dir` **is** the sessions container:
/// `<dir>/<session_id>/events.jsonl`.
///
/// This is the layout listed and scanned since §4.6 ([`projections`] works
/// over containers; writers persist through [`SessionLogWriter::open_layout`]).
pub fn session_log_container_path(dir: &Path, session_id: &str) -> PathBuf {
    dir.join(session_id).join("events.jsonl")
}

/// Resolve the sidecar metadata file for one session in a container:
/// `<dir>/<session_id>/meta.json` (user-curation fields only; every other
/// value is projected from the event log).
pub fn session_meta_container_path(dir: &Path, session_id: &str) -> PathBuf {
    dir.join(session_id).join("meta.json")
}

/// Effective log container for an active engine: `SHANNON_HOME` relocates
/// the whole Shannon root (legacy override, still beats everything); without
/// it the sessions container owned by the caller's `StateManager` wins —
/// which honors `SHANNON_SESSIONS_DIR` redirections wired into that manager.
pub fn effective_log_container(state_sessions_dir: &Path) -> PathBuf {
    match std::env::var("SHANNON_HOME") {
        Ok(home) if !home.trim().is_empty() => PathBuf::from(home).join("sessions"),
        _ => state_sessions_dir.to_path_buf(),
    }
}

/// Resolve the default Shannon home directory: `$SHANNON_HOME` when set,
/// otherwise `~/.shannon` (mirrors `shannon-agents` persistence).
pub fn default_shannon_home() -> Result<PathBuf, SessionLogError> {
    if let Ok(home_var) = std::env::var("SHANNON_HOME") {
        return Ok(PathBuf::from(home_var));
    }
    let home = dirs::home_dir().ok_or(SessionLogError::NotInitialized)?;
    Ok(home.join(".shannon"))
}

// ============================================================================
// QueryEvent -> SessionEvent mapping (pure; tee prep for §4.2)
// ============================================================================

/// Map one engine [`QueryEvent`] to a session-event body, for the §4.2 tee.
///
/// Pure function: no I/O, no state. Returns `None` for `QueryEvent` variants
/// that have no honest v1 vocabulary event:
///
/// - `Usage` / `Cost` — the tee folds usage into `turn/end` instead (see
///   [`token_usage_from_event`]); token data is also carried by
///   `assistant/message` payloads.
/// - `Completed` — the turn boundary is already emitted via `TurnStart` (from
///   `Started`) and `TurnEnd` (from `TurnCompleted` / `Failed`); the session
///   continues, and vocabulary v1 has no session/end event.
/// - `Progress` / `ToolProgress` / `Info` — transient UI progress, not part
///   of the durable record.
/// - `ConversationUpdate` — the tee derives `assistant/message` from it when
///   finalizing a step (needs coalescing state, not a 1:1 mapping).
///
/// The match is exhaustive on purpose: a future `QueryEvent` variant must
/// make a conscious mapping decision here.
pub fn query_event_to_session_body(event: &QueryEvent) -> Option<SessionEventBody> {
    let body = match event {
        QueryEvent::Started { .. } => {
            SessionEventBody::TurnStart(TurnStartPayload { query_id: None })
        }
        QueryEvent::Text { content, .. } => {
            SessionEventBody::AssistantChunk(AssistantChunkPayload {
                delta: content.clone(),
                thinking: false,
            })
        }
        QueryEvent::Thinking { content, .. } => {
            SessionEventBody::AssistantChunk(AssistantChunkPayload {
                delta: content.clone(),
                thinking: true,
            })
        }
        QueryEvent::ToolUseRequest {
            tool_use_id,
            tool_name,
            tool_input,
            ..
        } => SessionEventBody::ToolCall(ToolCallPayload {
            tool_use_id: tool_use_id.clone(),
            tool_name: tool_name.clone(),
            // Serialized back to a string: the engine already parsed the
            // model's arguments, so this is the rawest form available here.
            arguments: tool_input.to_string(),
        }),
        QueryEvent::ToolUseResult {
            tool_use_id,
            tool_name,
            result,
            is_error,
            meta,
            ..
        } => SessionEventBody::ToolResult(ToolResultPayload {
            tool_use_id: tool_use_id.clone(),
            tool_name: tool_name.clone(),
            output: result.clone(),
            is_error: *is_error,
            duration_ms: None,
            // Boxed in the event; payload keeps the vocabulary's plain Value.
            meta: (**meta).clone(),
        }),
        QueryEvent::TurnCompleted { tokens_used, .. } => {
            SessionEventBody::TurnEnd(TurnEndPayload {
                reason: TurnEndPayload::REASON_COMPLETED.into(),
                usage: Some(TokenUsage {
                    input_tokens: 0,
                    output_tokens: *tokens_used,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    cost_usd: None,
                }),
                error: None,
            })
        }
        QueryEvent::Failed { error, .. } => SessionEventBody::Error(ErrorPayload {
            category: "query-failed".into(),
            message: error.clone(),
            detail: None,
        }),
        QueryEvent::Warning { message, .. } => SessionEventBody::Error(ErrorPayload {
            category: "warning".into(),
            message: message.clone(),
            detail: None,
        }),
        QueryEvent::RateLimit {
            requests_used,
            requests_limit,
            ..
        } => SessionEventBody::Error(ErrorPayload {
            category: "rate_limit".into(),
            message: format!("rate limit: {requests_used}/{requests_limit} requests used"),
            detail: None,
        }),
        // No honest 1:1 vocabulary event — see the doc comment above.
        QueryEvent::Completed { .. }
        | QueryEvent::Progress { .. }
        | QueryEvent::ToolProgress { .. }
        | QueryEvent::Usage { .. }
        | QueryEvent::Cost { .. }
        | QueryEvent::Info { .. }
        | QueryEvent::ConversationUpdate { .. } => return None,
    };
    Some(body)
}

/// Extract token usage from a [`QueryEvent`], for the §4.2 tee to fold into
/// `turn/end` (plan: tokens / cost / cache triple).
///
/// `Usage` carries the full triple; `Cost` carries session totals; a bare
/// `TurnCompleted` only knows its output token count. Returns `None` for
/// events that carry no usage information.
pub fn token_usage_from_event(event: &QueryEvent) -> Option<TokenUsage> {
    let usage = match event {
        QueryEvent::Usage {
            input_tokens,
            output_tokens,
            cost_usd,
            cache_creation_tokens,
            cache_read_tokens,
            ..
        } => TokenUsage {
            input_tokens: *input_tokens,
            output_tokens: *output_tokens,
            cache_creation_tokens: *cache_creation_tokens,
            cache_read_tokens: *cache_read_tokens,
            cost_usd: Some(*cost_usd),
        },
        QueryEvent::Cost {
            total_cost_usd,
            input_tokens,
            output_tokens,
            ..
        } => TokenUsage {
            input_tokens: *input_tokens,
            output_tokens: *output_tokens,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            cost_usd: Some(*total_cost_usd),
        },
        QueryEvent::TurnCompleted { tokens_used, .. } => TokenUsage {
            input_tokens: 0,
            output_tokens: *tokens_used,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            cost_usd: None,
        },
        _ => return None,
    };
    Some(usage)
}

// ============================================================================
// §4.8 bridge: QueryEvent → bus inputs (the L0 subscriber's diet)
// ============================================================================

/// Map one broadcast [`QueryEvent`] to the bus inputs that reproduce exactly
/// what [`SessionTee::record_query_event`] used to write when the tee was a
/// direct bypass of `EventTx` (§4.2). This is the migration seam of plan
/// §4.8: the L0 writer is now a built-in bus subscriber, and this function is
/// its single mapping source.
///
/// Semantics mirrored verbatim from the pre-bus record path:
///
/// - [`QueryEvent::Usage`] / [`QueryEvent::TurnCompleted`] become fold
///   directives ([`crate::bus::CoalesceInput`]), not standalone rows.
/// - [`QueryEvent::Completed`] closes the open turn; [`QueryEvent::Failed`]
///   produces **two** inputs — the mapped `error` row and then the turn
///   boundary, in that order (dispatch them as one batch).
/// - `Progress` / `ToolProgress` / `Info` / `Cost` / `ConversationUpdate`
///   map to nothing (transient or folded elsewhere), same as before.
pub fn query_event_to_bus_inputs(event: &QueryEvent) -> Vec<crate::bus::BusInput> {
    use crate::bus::{BusEvent, BusInput, CoalesceInput};

    match event {
        // Fold-only inputs first: they never produce standalone rows.
        QueryEvent::Usage { .. } => {
            let usage = token_usage_from_event(event).expect("Usage maps to usage");
            vec![BusInput::Coalesce(CoalesceInput::StepUsage(usage))]
        }
        QueryEvent::TurnCompleted { tokens_used, .. } => {
            vec![BusInput::Coalesce(CoalesceInput::BareTokens(*tokens_used))]
        }
        QueryEvent::Completed { .. } => vec![BusInput::Coalesce(CoalesceInput::TurnBoundary {
            reason: TurnEndPayload::REASON_COMPLETED.into(),
            error: None,
        })],
        QueryEvent::Failed { error, .. } => {
            let mut inputs = Vec::with_capacity(2);
            if let Some(body) = query_event_to_session_body(event) {
                inputs.push(BusInput::Event(
                    BusEvent::new(body).with_origin("engine-stream"),
                ));
            }
            inputs.push(BusInput::Coalesce(CoalesceInput::TurnBoundary {
                reason: TurnEndPayload::REASON_FAILED.into(),
                error: Some(error.clone()),
            }));
            inputs
        }
        // Everything else: publish whatever the pure mapping yields.
        _ => match query_event_to_session_body(event) {
            Some(body) => vec![BusInput::Event(
                BusEvent::new(body).with_origin("engine-stream"),
            )],
            None => Vec::new(),
        },
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn query_id() -> Uuid {
        Uuid::new_v4()
    }

    #[test]
    fn test_session_events_path_layout() {
        let path = session_events_path(Path::new("/base"), "abc-123");
        assert_eq!(path, PathBuf::from("/base/sessions/abc-123/events.jsonl"));
    }

    #[test]
    fn test_mapping_started_to_turn_start() {
        let body = query_event_to_session_body(&QueryEvent::Started {
            query_id: query_id(),
        })
        .unwrap();
        assert!(matches!(
            body,
            SessionEventBody::TurnStart(TurnStartPayload { query_id: None })
        ));
    }

    #[test]
    fn test_mapping_text_and_thinking_to_chunks() {
        let text = query_event_to_session_body(&QueryEvent::Text {
            query_id: query_id(),
            content: "hi".into(),
        })
        .unwrap();
        match text {
            SessionEventBody::AssistantChunk(chunk) => {
                assert_eq!(chunk.delta, "hi");
                assert!(!chunk.thinking);
            }
            other => panic!("wrong body: {other:?}"),
        }

        let thinking = query_event_to_session_body(&QueryEvent::Thinking {
            query_id: query_id(),
            content: "hmm".into(),
        })
        .unwrap();
        match thinking {
            SessionEventBody::AssistantChunk(chunk) => {
                assert_eq!(chunk.delta, "hmm");
                assert!(chunk.thinking);
            }
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_mapping_tool_call_keeps_raw_arguments_string() {
        let body = query_event_to_session_body(&QueryEvent::ToolUseRequest {
            query_id: query_id(),
            tool_use_id: "toolu_9".into(),
            tool_name: "Read".into(),
            tool_input: serde_json::json!({"file_path": "/tmp/a.rs"}),
        })
        .unwrap();
        match body {
            SessionEventBody::ToolCall(call) => {
                assert_eq!(call.tool_use_id, "toolu_9");
                assert_eq!(call.tool_name, "Read");
                assert_eq!(call.arguments, r#"{"file_path":"/tmp/a.rs"}"#);
            }
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_mapping_tool_result() {
        let body = query_event_to_session_body(&QueryEvent::ToolUseResult {
            query_id: query_id(),
            tool_use_id: "toolu_9".into(),
            tool_name: "Read".into(),
            result: "contents".into(),
            is_error: true,
            meta: Box::new(serde_json::json!({"classification": "sandbox_denied"})),
        })
        .unwrap();
        match body {
            SessionEventBody::ToolResult(result) => {
                assert!(result.is_error);
                assert_eq!(result.output, "contents");
                assert_eq!(result.duration_ms, None);
                // §4.12: event meta is mirrored into the payload verbatim.
                assert_eq!(
                    result.meta["classification"],
                    serde_json::Value::String("sandbox_denied".into())
                );
            }
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_mapping_turn_completed_carries_usage() {
        let body = query_event_to_session_body(&QueryEvent::TurnCompleted {
            query_id: query_id(),
            turn_number: 1,
            tokens_used: 128,
        })
        .unwrap();
        match body {
            SessionEventBody::TurnEnd(end) => {
                assert_eq!(end.reason, TurnEndPayload::REASON_COMPLETED);
                let usage = end.usage.unwrap();
                assert_eq!(usage.output_tokens, 128);
                assert_eq!(usage.input_tokens, 0);
            }
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_mapping_failures_and_rate_limit_to_error() {
        let failed = query_event_to_session_body(&QueryEvent::Failed {
            query_id: query_id(),
            error: "boom".into(),
        })
        .unwrap();
        match failed {
            SessionEventBody::Error(e) => {
                assert_eq!(e.category, "query-failed");
                assert_eq!(e.message, "boom");
            }
            other => panic!("wrong body: {other:?}"),
        }

        let rate = query_event_to_session_body(&QueryEvent::RateLimit {
            query_id: query_id(),
            requests_used: 40,
            requests_limit: 50,
        })
        .unwrap();
        match rate {
            SessionEventBody::Error(e) => assert_eq!(e.category, "rate_limit"),
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_mapping_unmapped_variants_return_none() {
        assert!(
            query_event_to_session_body(&QueryEvent::Completed {
                query_id: query_id()
            })
            .is_none()
        );
        assert!(
            query_event_to_session_body(&QueryEvent::Progress {
                query_id: query_id(),
                message: "working".into(),
            })
            .is_none()
        );
        assert!(
            query_event_to_session_body(&QueryEvent::ToolProgress {
                query_id: query_id(),
                tool_use_id: "t".into(),
                tool_name: "Bash".into(),
                progress: 0.5,
                message: "running".into(),
            })
            .is_none()
        );
        assert!(
            query_event_to_session_body(&QueryEvent::Cost {
                query_id: query_id(),
                total_cost_usd: 0.1,
                input_tokens: 1,
                output_tokens: 2,
            })
            .is_none()
        );
        assert!(
            query_event_to_session_body(&QueryEvent::Info {
                query_id: query_id(),
                message: "info".into(),
            })
            .is_none()
        );
        assert!(
            query_event_to_session_body(&QueryEvent::ConversationUpdate {
                query_id: query_id(),
                messages: Vec::new(),
            })
            .is_none()
        );
    }

    #[test]
    fn test_token_usage_from_usage_event() {
        let usage = token_usage_from_event(&QueryEvent::Usage {
            query_id: query_id(),
            input_tokens: 100,
            output_tokens: 200,
            cost_usd: 0.5,
            cache_creation_tokens: 10,
            cache_read_tokens: 20,
        })
        .unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 200);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cost_usd, Some(0.5));
    }

    #[test]
    fn test_token_usage_from_cost_event() {
        let usage = token_usage_from_event(&QueryEvent::Cost {
            query_id: query_id(),
            total_cost_usd: 1.5,
            input_tokens: 300,
            output_tokens: 400,
        })
        .unwrap();
        assert_eq!(usage.cost_usd, Some(1.5));
        assert_eq!(usage.input_tokens, 300);
    }

    #[test]
    fn test_token_usage_none_for_non_usage_events() {
        assert!(
            token_usage_from_event(&QueryEvent::Started {
                query_id: query_id()
            })
            .is_none()
        );
        assert!(
            token_usage_from_event(&QueryEvent::Text {
                query_id: query_id(),
                content: "x".into(),
            })
            .is_none()
        );
    }
}
