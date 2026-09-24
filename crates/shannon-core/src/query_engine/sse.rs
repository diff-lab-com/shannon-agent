//! SSE wire mapping for [`QueryEvent`] — the single
//! `QueryEvent → (event name, JSON payload)` converter shared by every SSE
//! producer: `shannon-core`'s `api_server` and the headless `shannon-server`.
//!
//! Review §P2-6: before this module the two servers disagreed on SSE event
//! names (`shannon-server` bucketed everything into `text` / `completed` /
//! `error` / `event`; `api_server` emitted the full per-variant names). The
//! canonical contract is now [`shannon_api_protocol::SseEventName`]; the
//! mapping here is exhaustive on purpose, so adding a [`QueryEvent`] variant
//! breaks compilation until the wire contract is extended deliberately.
//! Serialization-failure semantics (§P3-4) also live here so both servers
//! inherit them.

use shannon_api_protocol::SseEventName;
use uuid::Uuid;

use super::types::QueryEvent;

/// Map a [`QueryEvent`] to its canonical SSE event name.
///
/// Exhaustive match by design (§P2-6): a new `QueryEvent` variant fails to
/// compile here until its SSE name is chosen, keeping the protocol crate's
/// [`SseEventName`] contract from drifting from the event stream.
pub fn sse_event_name(event: &QueryEvent) -> SseEventName {
    match event {
        QueryEvent::Started { .. } => SseEventName::Started,
        QueryEvent::Text { .. } => SseEventName::Text,
        QueryEvent::ToolUseRequest { .. } => SseEventName::ToolUseRequest,
        QueryEvent::ToolUseResult { .. } => SseEventName::ToolUseResult,
        QueryEvent::TurnCompleted { .. } => SseEventName::TurnCompleted,
        QueryEvent::Completed { .. } => SseEventName::Completed,
        QueryEvent::Failed { .. } => SseEventName::Failed,
        QueryEvent::Warning { .. } => SseEventName::Warning,
        QueryEvent::Progress { .. } => SseEventName::Progress,
        QueryEvent::ToolProgress { .. } => SseEventName::ToolProgress,
        QueryEvent::Thinking { .. } => SseEventName::Thinking,
        QueryEvent::Usage { .. } => SseEventName::Usage,
        QueryEvent::Cost { .. } => SseEventName::Cost,
        QueryEvent::Info { .. } => SseEventName::Info,
        QueryEvent::ConversationUpdate { .. } => SseEventName::ConversationUpdate,
        QueryEvent::RateLimit { .. } => SseEventName::RateLimit,
    }
}

/// Map a [`QueryEvent`] to its SSE `(event name, JSON payload)` pair.
///
/// §P3-4: a serialization failure (e.g. a NaN cost value, which
/// `serde_json` refuses to emit) must not degrade into an empty payload —
/// the client would receive a semantically empty event of the *expected*
/// type. Instead the event is logged and an explicit `error` event with a
/// description goes out on the wire.
pub fn sse_parts_from_query_event(event: &QueryEvent) -> (&'static str, String) {
    sse_parts(sse_event_name(event).as_str(), event)
}

/// Serialize `value` into an SSE `(event name, JSON payload)` pair.
///
/// §P3-4: on serialization failure the payload must not be empty — the
/// client would receive a semantically empty event of the *expected* type.
/// Instead the failure is logged and an explicit `error` event with a
/// description goes out on the wire.
pub fn sse_parts<T: serde::Serialize>(
    event_type: &'static str,
    value: &T,
) -> (&'static str, String) {
    match serde_json::to_string(value) {
        Ok(data) => (event_type, data),
        Err(e) => {
            tracing::error!(
                error = %e,
                event_type,
                "SSE event serialization failed; emitting explicit error event"
            );
            let data = serde_json::json!({
                "error": format!("event serialization failed: {e}"),
                "event_type": event_type,
            })
            .to_string();
            (SseEventName::Error.as_str(), data)
        }
    }
}

/// One representative [`QueryEvent`] per variant — the shared fixture the
/// SSE-contract tests (here and in `shannon-server`) run through both
/// servers' SSE encoders, so a mapping divergence fails a test instead of a
/// client. `#[doc(hidden)]`: test infrastructure, not a public API promise.
#[doc(hidden)]
pub fn representative_events() -> Vec<QueryEvent> {
    let qid = Uuid::new_v4();
    vec![
        QueryEvent::Started { query_id: qid },
        QueryEvent::Text {
            query_id: qid,
            content: "hello".to_string(),
        },
        QueryEvent::ToolUseRequest {
            query_id: qid,
            tool_use_id: "t1".to_string(),
            tool_name: "bash".to_string(),
            tool_input: serde_json::json!({"command": "ls"}),
        },
        QueryEvent::ToolUseResult {
            query_id: qid,
            tool_use_id: "t1".to_string(),
            tool_name: "bash".to_string(),
            result: "ok".to_string(),
            is_error: false,
            meta: Box::new(serde_json::Value::Null),
        },
        QueryEvent::TurnCompleted {
            query_id: qid,
            turn_number: 1,
            tokens_used: 42,
        },
        QueryEvent::Completed { query_id: qid },
        QueryEvent::Failed {
            query_id: qid,
            error: "boom".to_string(),
        },
        QueryEvent::Warning {
            query_id: qid,
            message: "recovered".to_string(),
        },
        QueryEvent::Progress {
            query_id: qid,
            message: "step".to_string(),
        },
        QueryEvent::ToolProgress {
            query_id: qid,
            tool_use_id: "t1".to_string(),
            tool_name: "bash".to_string(),
            progress: 0.5,
            message: "running".to_string(),
        },
        QueryEvent::Thinking {
            query_id: qid,
            content: "hmm".to_string(),
        },
        QueryEvent::Usage {
            query_id: qid,
            input_tokens: 10,
            output_tokens: 5,
            cost_usd: 0.001,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        },
        QueryEvent::Cost {
            query_id: qid,
            total_cost_usd: 0.002,
            input_tokens: 10,
            output_tokens: 5,
        },
        QueryEvent::Info {
            query_id: qid,
            message: "compacted".to_string(),
        },
        QueryEvent::ConversationUpdate {
            query_id: qid,
            messages: Vec::new(),
        },
        QueryEvent::RateLimit {
            query_id: qid,
            requests_used: 3,
            requests_limit: 100,
        },
    ]
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use uuid::Uuid;

    /// The mapping must select the *specific* contract variant per event —
    /// not just any non-error name — so a transposed arm cannot hide here.
    #[test]
    fn every_query_event_maps_to_its_contracted_sse_name() {
        for event in representative_events() {
            let (name, data) = sse_parts_from_query_event(&event);
            let expected = match &event {
                QueryEvent::Started { .. } => SseEventName::Started,
                QueryEvent::Text { .. } => SseEventName::Text,
                QueryEvent::ToolUseRequest { .. } => SseEventName::ToolUseRequest,
                QueryEvent::ToolUseResult { .. } => SseEventName::ToolUseResult,
                QueryEvent::TurnCompleted { .. } => SseEventName::TurnCompleted,
                QueryEvent::Completed { .. } => SseEventName::Completed,
                QueryEvent::Failed { .. } => SseEventName::Failed,
                QueryEvent::Warning { .. } => SseEventName::Warning,
                QueryEvent::Progress { .. } => SseEventName::Progress,
                QueryEvent::ToolProgress { .. } => SseEventName::ToolProgress,
                QueryEvent::Thinking { .. } => SseEventName::Thinking,
                QueryEvent::Usage { .. } => SseEventName::Usage,
                QueryEvent::Cost { .. } => SseEventName::Cost,
                QueryEvent::Info { .. } => SseEventName::Info,
                QueryEvent::ConversationUpdate { .. } => SseEventName::ConversationUpdate,
                QueryEvent::RateLimit { .. } => SseEventName::RateLimit,
            };
            assert_eq!(name, expected.as_str(), "event {event:?}");
            assert!(
                serde_json::from_str::<serde_json::Value>(&data).is_ok(),
                "payload for {name} must be valid JSON"
            );
        }
    }

    // ── §P3-4: serialization failure emits an explicit error event ─────

    #[test]
    fn sse_serializable_event_keeps_its_event_name() {
        let event = QueryEvent::Progress {
            query_id: Uuid::new_v4(),
            message: "step 1".to_string(),
        };
        let (event_type, data) = sse_parts_from_query_event(&event);
        assert_eq!(event_type, "progress");
        let parsed: serde_json::Value = serde_json::from_str(&data).unwrap();
        // QueryEvent serializes externally tagged: {"Progress": {...}}.
        assert_eq!(parsed["Progress"]["message"], "step 1");
    }

    #[test]
    fn sse_unserializable_event_becomes_error_event_not_empty_payload() {
        // A payload whose serialization always fails — standing in for
        // corrupted payloads (e.g. a NaN-bearing value that serde_json
        // rejects in some positions) that previously degraded to an empty
        // wire event carrying the *expected* event name.
        struct AlwaysFails;
        impl serde::Serialize for AlwaysFails {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("unserializable payload"))
            }
        }
        let event = AlwaysFails;
        // Sanity: the raw serialization really does fail.
        assert!(serde_json::to_string(&event).is_err());

        let (event_type, data) = sse_parts("cost", &event);
        assert_eq!(
            event_type, "error",
            "unserializable event must emit an error event"
        );
        assert!(!data.is_empty(), "error event must carry a description");
        let parsed: serde_json::Value = serde_json::from_str(&data).unwrap();
        let error = parsed["error"].as_str().unwrap();
        assert!(
            error.contains("serialization failed"),
            "error event must explain the failure, got: {error}"
        );
        // The failed event type is surfaced so clients know what was lost.
        assert_eq!(parsed["event_type"], "cost");
    }
}
