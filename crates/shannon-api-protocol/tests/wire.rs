//! Round-trip, default, tag, and handshake tests for the wire protocol.
//!
//! These tests are deliberately low-level: every byte on the wire matters, so
//! the assertions are against raw `serde_json::Value` (or exact JSON strings)
//! rather than just "did it parse". The shape contract lives here; if a
//! future field rename changes the wire, these tests must change too.

use serde_json::json;
use shannon_api_protocol::{
    ApprovalDecision, ApprovalRespondRequest, HealthResponse, ModelInfo, ModelsResponse,
    PROTOCOL_VERSION, QueryRequest, QueryResponse, SseEventName, ToolEntry, ToolsListResponse,
    UsageInfo, WsClientMessage, WsServerMessage,
};
use uuid::Uuid;

// ── Protocol version ────────────────────────────────────────────────────

#[test]
fn protocol_version_is_stable_string() {
    // Bumping requires a deliberate change in two places: this constant and
    // the handshake. If PROTOCOL_VERSION becomes a non-string the entire
    // contract (gen-ts + gateway) breaks, so guard it here.
    assert!(!PROTOCOL_VERSION.is_empty());
    assert!(PROTOCOL_VERSION.contains('.'));
}

// ── QueryRequest ────────────────────────────────────────────────────────

#[test]
fn query_request_serialization() {
    let req = QueryRequest {
        prompt: "hello world".to_string(),
        model: Some("gpt-4o".to_string()),
        session_id: None,
        attachments: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("hello world"));
    assert!(json.contains("gpt-4o"));

    let deserialized: QueryRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.prompt, "hello world");
    assert_eq!(deserialized.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn query_request_model_defaults_to_none() {
    let req: QueryRequest = serde_json::from_str(r#"{"prompt": "test"}"#).unwrap();
    assert_eq!(req.prompt, "test");
    assert!(req.model.is_none());
}

#[test]
fn query_request_session_id_defaults_to_none() {
    let req: QueryRequest = serde_json::from_str(r#"{"prompt": "hi"}"#).unwrap();
    assert!(req.session_id.is_none());
}

#[test]
fn query_request_deserializes_session_id_field() {
    let id = Uuid::new_v4();
    let json = format!(r#"{{"prompt": "hi", "session_id": "{id}"}}"#);
    let req: QueryRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req.session_id, Some(id.to_string()));
}

// ── QueryResponse ───────────────────────────────────────────────────────

#[test]
fn query_response_round_trips_session_id() {
    let id = Uuid::new_v4();
    let resp = QueryResponse {
        text: "hello".to_string(),
        model: "m".to_string(),
        usage: None,
        errors: Vec::new(),
        session_id: id,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: QueryResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.session_id, id);
}

#[test]
fn query_response_with_usage_and_errors() {
    let resp = QueryResponse {
        text: "response text".to_string(),
        model: "test-model".to_string(),
        usage: Some(UsageInfo {
            input_tokens: 100,
            output_tokens: 50,
            cost_usd: 0.005,
        }),
        errors: vec![],
        session_id: Uuid::new_v4(),
    };
    let parsed: serde_json::Value = serde_json::to_value(&resp).unwrap();
    assert_eq!(parsed["text"], "response text");
    assert_eq!(parsed["model"], "test-model");
    assert_eq!(parsed["usage"]["input_tokens"], 100);
    assert_eq!(parsed["usage"]["output_tokens"], 50);
    assert_eq!(parsed["usage"]["cost_usd"], 0.005);
    assert_eq!(parsed["errors"].as_array().unwrap().len(), 0);
}

#[test]
fn query_response_backward_compatible_when_usage_missing() {
    // Old payload without `usage` or `session_id` must still parse.
    let resp: QueryResponse =
        serde_json::from_str(r#"{"text":"t","model":"m","errors":[]}"#).unwrap();
    assert_eq!(resp.text, "t");
    assert!(resp.usage.is_none());
    assert_eq!(resp.errors.len(), 0);
    // session_id defaults to the nil UUID — it carries the #[serde(default)]
    // marker precisely so legacy payloads remain parseable.
    assert_eq!(resp.session_id, Uuid::nil());
}

// ── UsageInfo ───────────────────────────────────────────────────────────

#[test]
fn usage_info_serialization() {
    let info = UsageInfo {
        input_tokens: 500,
        output_tokens: 200,
        cost_usd: 0.0123,
    };
    let parsed: UsageInfo = serde_json::from_value(serde_json::to_value(&info).unwrap()).unwrap();
    assert_eq!(parsed.input_tokens, 500);
    assert_eq!(parsed.output_tokens, 200);
    assert!((parsed.cost_usd - 0.0123).abs() < f64::EPSILON);
}

// ── HealthResponse ──────────────────────────────────────────────────────

#[test]
fn health_response_serialization() {
    let resp = HealthResponse {
        status: "ok".to_string(),
        version: "1.0.0".to_string(),
    };
    let parsed: HealthResponse =
        serde_json::from_value(serde_json::to_value(&resp).unwrap()).unwrap();
    assert_eq!(parsed.status, "ok");
    assert_eq!(parsed.version, "1.0.0");
}

// ── ModelsResponse / ModelInfo ───────────────────────────────────────────

#[test]
fn models_response_serialization() {
    let resp = ModelsResponse {
        models: vec![
            ModelInfo {
                id: "gpt-4o".to_string(),
                provider: "openai".to_string(),
                name: Some("GPT-4o".to_string()),
            },
            ModelInfo {
                id: "llama3".to_string(),
                provider: "ollama".to_string(),
                name: None,
            },
        ],
    };
    let parsed: ModelsResponse =
        serde_json::from_value(serde_json::to_value(&resp).unwrap()).unwrap();
    assert_eq!(parsed.models.len(), 2);
    assert_eq!(parsed.models[0].id, "gpt-4o");
    assert_eq!(parsed.models[1].provider, "ollama");
}

// ── ToolsListResponse / ToolEntry ────────────────────────────────────────

#[test]
fn tools_list_response_serialization() {
    let resp = ToolsListResponse {
        tools: vec![ToolEntry {
            name: "bash".to_string(),
            description: "Execute shell commands".to_string(),
        }],
    };
    let parsed: ToolsListResponse =
        serde_json::from_value(serde_json::to_value(&resp).unwrap()).unwrap();
    assert_eq!(parsed.tools.len(), 1);
    assert_eq!(parsed.tools[0].name, "bash");
}

// ── ApprovalDecision ────────────────────────────────────────────────────

#[test]
fn approval_decision_serde_round_trip() {
    for (decision, wire) in [
        (ApprovalDecision::AllowOnce, "allow_once"),
        (ApprovalDecision::AlwaysAllow, "always_allow"),
        (ApprovalDecision::Deny, "deny"),
    ] {
        let json = serde_json::to_string(&decision).unwrap();
        assert_eq!(json, format!("\"{wire}\""));
        let back: ApprovalDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(back, decision);
    }
}

#[test]
fn approval_decision_unknown_variant_is_rejected() {
    let res: Result<ApprovalDecision, _> = serde_json::from_str("\"oops\"");
    assert!(res.is_err());
}

// ── ApprovalRespondRequest ──────────────────────────────────────────────

#[test]
fn approval_respond_request_serialization() {
    let body = ApprovalRespondRequest {
        request_id: "abc-123".to_string(),
        choice: ApprovalDecision::AllowOnce,
    };
    let parsed: serde_json::Value = serde_json::to_value(&body).unwrap();
    assert_eq!(parsed["request_id"], "abc-123");
    assert_eq!(parsed["choice"], "allow_once");
}

// ── WsClientMessage ─────────────────────────────────────────────────────

#[test]
fn ws_client_message_query_serialization() {
    let msg = WsClientMessage::Query {
        prompt: "hello".to_string(),
        model: Some("gpt-4o".to_string()),
        session_id: None,
        attachments: None,
    };
    let parsed: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(parsed["type"], "query");
    assert_eq!(parsed["prompt"], "hello");
    assert_eq!(parsed["model"], "gpt-4o");
}

#[test]
fn ws_client_message_query_without_model_emits_null() {
    let msg = WsClientMessage::Query {
        prompt: "test".to_string(),
        model: None,
        session_id: None,
        attachments: None,
    };
    let parsed: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(parsed["type"], "query");
    assert!(parsed["model"].is_null());
}

#[test]
fn ws_client_message_query_round_trips_session_id() {
    let id = Uuid::new_v4();
    let json = format!(r#"{{"type": "query", "prompt": "hi", "session_id": "{id}"}}"#);
    let msg: WsClientMessage = serde_json::from_str(&json).unwrap();
    match msg {
        WsClientMessage::Query { session_id, .. } => {
            assert_eq!(session_id, Some(id.to_string()));
        }
        other => panic!("expected Query, got {other:?}"),
    }
}

#[test]
fn ws_client_message_clear_info_cancel() {
    assert_eq!(
        serde_json::to_value(&WsClientMessage::Clear).unwrap()["type"],
        "clear"
    );
    assert_eq!(
        serde_json::to_value(&WsClientMessage::Info).unwrap()["type"],
        "info"
    );
    assert_eq!(
        serde_json::to_value(&WsClientMessage::Cancel).unwrap()["type"],
        "cancel"
    );
}

#[test]
fn ws_client_message_roundtrip_all_variants() {
    let messages = vec![
        WsClientMessage::Query {
            prompt: "test prompt".to_string(),
            model: Some("llama3".to_string()),
            session_id: None,
            attachments: None,
        },
        WsClientMessage::Clear,
        WsClientMessage::Info,
        WsClientMessage::Cancel,
    ];
    for msg in messages {
        let json = serde_json::to_string(&msg).unwrap();
        let roundtrip: WsClientMessage = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&roundtrip).unwrap();
        assert_eq!(json, json2);
    }
}

#[test]
fn ws_client_message_invalid_type_rejected() {
    let res: Result<WsClientMessage, _> = serde_json::from_str(r#"{"type":"unknown_type"}"#);
    assert!(res.is_err());
}

#[test]
fn ws_client_message_missing_type_rejected() {
    let res: Result<WsClientMessage, _> = serde_json::from_str(r#"{"prompt":"hello"}"#);
    assert!(res.is_err());
}

// ── WsServerMessage ─────────────────────────────────────────────────────

#[test]
fn ws_server_message_text() {
    let msg = WsServerMessage::Text {
        content: "hello world".to_string(),
    };
    let parsed: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(parsed["type"], "text");
    assert_eq!(parsed["content"], "hello world");
}

#[test]
fn ws_server_message_tool_use() {
    let msg = WsServerMessage::ToolUse {
        name: "bash".to_string(),
        input: json!({"command": "ls"}),
    };
    let parsed: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(parsed["type"], "tool_use");
    assert_eq!(parsed["name"], "bash");
    assert_eq!(parsed["input"]["command"], "ls");
}

#[test]
fn ws_server_message_tool_result() {
    let msg = WsServerMessage::ToolResult {
        name: "bash".to_string(),
        output: "file1.txt\nfile2.txt".to_string(),
    };
    let parsed: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(parsed["type"], "tool_result");
    assert_eq!(parsed["name"], "bash");
    assert_eq!(parsed["output"], "file1.txt\nfile2.txt");
}

#[test]
fn ws_server_message_usage() {
    let msg = WsServerMessage::Usage {
        input_tokens: 100,
        output_tokens: 50,
        cost_usd: 0.003,
    };
    let parsed: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(parsed["type"], "usage");
    assert_eq!(parsed["input_tokens"], 100);
    assert_eq!(parsed["output_tokens"], 50);
    assert!((parsed["cost_usd"].as_f64().unwrap() - 0.003).abs() < f64::EPSILON);
}

#[test]
fn ws_server_message_completed() {
    let msg = WsServerMessage::Completed {
        model: "claude-sonnet-4".to_string(),
    };
    let parsed: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(parsed["type"], "completed");
    assert_eq!(parsed["model"], "claude-sonnet-4");
}

#[test]
fn ws_server_message_failed() {
    let msg = WsServerMessage::Failed {
        error: "timeout".to_string(),
    };
    let parsed: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(parsed["type"], "failed");
    assert_eq!(parsed["error"], "timeout");
}

#[test]
fn ws_server_message_cancelled() {
    let parsed: serde_json::Value = serde_json::to_value(&WsServerMessage::Cancelled).unwrap();
    assert_eq!(parsed["type"], "cancelled");
}

#[test]
fn ws_server_message_approval_request() {
    let msg = WsServerMessage::ApprovalRequest {
        request_id: "abc-123".to_string(),
        tool_name: "bash".to_string(),
        tool_input: json!({"command": "ls"}),
        description: "Run a shell command".to_string(),
        is_destructive: true,
        diff_preview: Some("--- old\n+++ new".to_string()),
    };
    let parsed: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(parsed["type"], "approval_request");
    assert_eq!(parsed["request_id"], "abc-123");
    assert_eq!(parsed["tool_name"], "bash");
    assert_eq!(parsed["is_destructive"], true);
    assert_eq!(parsed["diff_preview"], "--- old\n+++ new");
}

#[test]
fn ws_server_message_session_info_with_protocol_version() {
    let parsed: serde_json::Value =
        serde_json::to_value(WsServerMessage::greeting(0, None)).unwrap();
    assert_eq!(parsed["type"], "session_info");
    assert_eq!(parsed["message_count"], 0);
    assert!(parsed["model"].is_null());
    assert_eq!(parsed["protocol_version"], PROTOCOL_VERSION);
}

#[test]
fn ws_server_message_session_info_legacy_payload_still_parses() {
    // Pre-Phase A client payload (no protocol_version) must still deserialize
    // so the handshake extension is backward compatible.
    let legacy = r#"{"type":"session_info","message_count":3,"model":"gpt-4o"}"#;
    let msg: WsServerMessage = serde_json::from_str(legacy).unwrap();
    match msg {
        WsServerMessage::SessionInfo {
            message_count,
            model,
            protocol_version,
        } => {
            assert_eq!(message_count, 3);
            assert_eq!(model.as_deref(), Some("gpt-4o"));
            assert!(protocol_version.is_none());
        }
        other => panic!("expected SessionInfo, got {other:?}"),
    }
}

#[test]
fn ws_server_message_session_info_model_none_emits_null() {
    let msg = WsServerMessage::greeting(5, None);
    let parsed: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(parsed["type"], "session_info");
    assert_eq!(parsed["message_count"], 5);
    assert!(parsed["model"].is_null());
}

#[test]
fn ws_server_message_error() {
    let msg = WsServerMessage::Error {
        message: "something failed".to_string(),
    };
    let parsed: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["message"], "something failed");
}

#[test]
fn ws_server_message_roundtrip_all_variants() {
    let messages = vec![
        WsServerMessage::Text {
            content: "hi".to_string(),
        },
        WsServerMessage::ToolUse {
            name: "read".to_string(),
            input: json!({"path": "/tmp"}),
        },
        WsServerMessage::ToolResult {
            name: "read".to_string(),
            output: "contents".to_string(),
        },
        WsServerMessage::Usage {
            input_tokens: 10,
            output_tokens: 5,
            cost_usd: 0.001,
        },
        WsServerMessage::Completed {
            model: "test".to_string(),
        },
        WsServerMessage::Failed {
            error: "err".to_string(),
        },
        WsServerMessage::greeting(3, Some("m".to_string())),
        WsServerMessage::Error {
            message: "bad".to_string(),
        },
        WsServerMessage::Cancelled,
        WsServerMessage::ApprovalRequest {
            request_id: "r".to_string(),
            tool_name: "bash".to_string(),
            tool_input: json!({}),
            description: "d".to_string(),
            is_destructive: false,
            diff_preview: None,
        },
    ];
    for msg in messages {
        let json = serde_json::to_string(&msg).unwrap();
        let roundtrip: WsServerMessage = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&roundtrip).unwrap();
        assert_eq!(json, json2);
    }
}

#[test]
fn ws_server_message_invalid_type_rejected() {
    let res: Result<WsServerMessage, _> = serde_json::from_str(r#"{"type":"not_a_real_type"}"#);
    assert!(res.is_err());
}

// ── SseEventName (SSE event-name contract) ──────────────────────────────

/// The full expected set of SSE `event:` names, pinned here so an accidental
/// rename (or a new variant with a malformed name) cannot ship silently.
/// Keep in lockstep with `SseEventName`; the `query_engine::sse` mapping is
/// compile-checked against this enum, so a variant addition fails the build
/// until both sides agree.
const EXPECTED_SSE_EVENT_NAMES: [&str; 17] = [
    "started",
    "text",
    "tool_use_request",
    "tool_use_result",
    "turn_completed",
    "completed",
    "failed",
    "warning",
    "progress",
    "tool_progress",
    "thinking",
    "usage",
    "cost",
    "info",
    "conversation_update",
    "rate_limit",
    "error",
];

#[test]
fn sse_event_names_are_non_empty_lowercase_snake_case() {
    let all = [
        SseEventName::Started,
        SseEventName::Text,
        SseEventName::ToolUseRequest,
        SseEventName::ToolUseResult,
        SseEventName::TurnCompleted,
        SseEventName::Completed,
        SseEventName::Failed,
        SseEventName::Warning,
        SseEventName::Progress,
        SseEventName::ToolProgress,
        SseEventName::Thinking,
        SseEventName::Usage,
        SseEventName::Cost,
        SseEventName::Info,
        SseEventName::ConversationUpdate,
        SseEventName::RateLimit,
        SseEventName::Error,
    ];
    assert_eq!(
        all.len(),
        EXPECTED_SSE_EVENT_NAMES.len(),
        "SseEventName grew a variant: update EXPECTED_SSE_EVENT_NAMES and the \
         query_engine::sse mapping together"
    );
    for name in all {
        let wire = name.as_str();
        assert!(!wire.is_empty(), "{name:?} must have a non-empty name");
        assert!(
            wire.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{name:?} name {wire:?} must be lowercase ASCII"
        );
        assert!(
            !wire.starts_with('_') && !wire.ends_with('_') && !wire.contains("__"),
            "{name:?} name {wire:?} must be clean snake_case"
        );
    }
}

#[test]
fn sse_event_name_set_is_pinned_to_the_contract() {
    // Every declared name is produced, and nothing outside the pinned set is.
    let mut produced: Vec<&str> = [
        SseEventName::Started,
        SseEventName::Text,
        SseEventName::ToolUseRequest,
        SseEventName::ToolUseResult,
        SseEventName::TurnCompleted,
        SseEventName::Completed,
        SseEventName::Failed,
        SseEventName::Warning,
        SseEventName::Progress,
        SseEventName::ToolProgress,
        SseEventName::Thinking,
        SseEventName::Usage,
        SseEventName::Cost,
        SseEventName::Info,
        SseEventName::ConversationUpdate,
        SseEventName::RateLimit,
        SseEventName::Error,
    ]
    .iter()
    .map(|n| n.as_str())
    .collect();
    produced.sort_unstable();

    let mut expected = EXPECTED_SSE_EVENT_NAMES.to_vec();
    expected.sort_unstable();

    assert_eq!(produced, expected, "SSE event-name contract drifted");
}

#[test]
fn sse_event_name_serde_round_trip() {
    for (name, wire) in [
        (SseEventName::ToolUseRequest, "tool_use_request"),
        (SseEventName::ConversationUpdate, "conversation_update"),
        (SseEventName::RateLimit, "rate_limit"),
        (SseEventName::Error, "error"),
    ] {
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, format!("\"{wire}\""));
        let back: SseEventName = serde_json::from_str(&json).unwrap();
        assert_eq!(back, name);
        assert_eq!(back.as_str(), wire);
    }
}

#[test]
fn sse_event_name_unknown_wire_value_rejected() {
    let res: Result<SseEventName, _> = serde_json::from_str("\"event\"");
    assert!(res.is_err(), "the legacy bucket name must not parse back");
}

// ── Greeting helper ─────────────────────────────────────────────────────

#[test]
fn greeting_helper_includes_protocol_version() {
    let msg = WsServerMessage::greeting(7, Some("gpt-4o".to_string()));
    let WsServerMessage::SessionInfo {
        message_count,
        model,
        protocol_version,
    } = msg
    else {
        panic!("greeting() must produce a SessionInfo frame");
    };
    assert_eq!(message_count, 7);
    assert_eq!(model.as_deref(), Some("gpt-4o"));
    assert_eq!(protocol_version.as_deref(), Some(PROTOCOL_VERSION));
}
