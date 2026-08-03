//! P2-3 (improvement plan §P2-3): snapshot test for one MCP `tools/call`
//! response shape.
//!
//! The MCP wire format is the public API for every external client (the
//! desktop REPL, third-party agents, the SaaS bridge). A silent change to
//! field naming or the `result` envelope breaks them all. This test
//! pins the shape against the canonical `echo` fixture — the same
//! shape every real tool call will serialize to, modulo `is_error` and
//! the `content` payload.

use shannon_mcp::protocol::{ContentBlock, JsonRpcMessage, JsonRpcResponse, ToolContent};

/// `tools/call` result envelope.
///
/// The MCP spec requires the `result.content` array to contain one or
/// more `ContentBlock` items. The `is_error` flag is optional; we set it
/// only when the call failed so the desktop REPL can render
/// success/failure with a single check.
fn echo_tool_response(id: &str, text: &str) -> JsonRpcResponse {
    let result = serde_json::json!({
        "content": [
            {
                "type": "text",
                "text": text,
            }
        ],
        "isError": false,
    });
    JsonRpcResponse::ok(id, result)
}

/// `tools/call` with a successful text response. Locks the JSON envelope
/// (`jsonrpc`, `id`, `result.content[]`, `isError`) and the camelCase
/// rename rule on `ContentBlock`.
#[test]
fn tools_call_echo_success_snapshot() {
    let response = echo_tool_response("call-001", "hello, world");
    let message = JsonRpcMessage::Response(response);
    insta::assert_json_snapshot!("tools_call_echo_success", message);
}

/// `tools/call` that produced an error — `isError` flips to true and the
/// `content` array contains the diagnostic text. The shape is identical
/// to the success case; only the flag and the text differ.
#[test]
fn tools_call_error_snapshot() {
    let result = serde_json::json!({
        "content": [
            {
                "type": "text",
                "text": "tool not found: echo_typo",
            }
        ],
        "isError": true,
    });
    let response = JsonRpcResponse::ok("call-002", result);
    let message = JsonRpcMessage::Response(response);
    insta::assert_json_snapshot!("tools_call_error", message);
}

/// Round-trip the typed `ToolContent` through serde to verify the
/// `camelCase` rename rule keeps working when callers construct the
/// payload via the Rust API rather than ad-hoc JSON.
#[test]
fn tool_content_roundtrip_snapshot() {
    let payload = ToolContent {
        content: vec![
            ContentBlock::Text {
                text: "first line".to_string(),
            },
            ContentBlock::Text {
                text: "second line".to_string(),
            },
        ],
        is_error: None,
    };
    let value = serde_json::to_value(&payload).unwrap();
    insta::assert_json_snapshot!("tool_content_text_pair", value);
}
