//! Mock MCP server (P2-3 §MCP mocks).
//!
//! Extends the per-test [`MockTransport`] in `integration_tests.rs` /
//! `lifecycle_tests.rs` with a higher-level server stub that actually
//! responds to `initialize`, `tools/list`, and `tools/call`. Picked the
//! echo tool because every test that exercises the wire format already
//! uses it — covering one tool here unblocks every downstream test that
//! wants to spin up a fake server without spawning a child process.
//!
//! The mock is deliberately tiny: it does not parse `params`, does not
//! stream, and does not negotiate capabilities. It is a fixture, not a
//! server. The pinned shape is asserted by `tools_call_snapshot.rs`.

use std::sync::Arc;

use shannon_mcp::protocol::{
    ContentBlock, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, Tool, ToolContent,
};
use tokio::sync::Mutex;

// ----------------------------------------------------------------------------
// Inlined MockTransport — keeps this integration test self-contained.
// Each file under `tests/` is a separate crate, so we cannot share the
// transport from `integration_tests.rs` without a `mod common;` shim.
// ----------------------------------------------------------------------------

#[derive(Clone)]
pub struct MockTransport {
    sender: Arc<Mutex<tokio::sync::mpsc::UnboundedSender<String>>>,
    receiver: Arc<Mutex<tokio::sync::mpsc::UnboundedReceiver<String>>>,
}

impl MockTransport {
    fn new_pair() -> (Self, Self) {
        let (tx1, rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, rx2) = tokio::sync::mpsc::unbounded_channel();
        let a = Self {
            sender: Arc::new(Mutex::new(tx1)),
            receiver: Arc::new(Mutex::new(rx2)),
        };
        let b = Self {
            sender: Arc::new(Mutex::new(tx2)),
            receiver: Arc::new(Mutex::new(rx1)),
        };
        (a, b)
    }

    async fn send(&self, message: &str) {
        let sender = self.sender.lock().await;
        let _ = sender.send(message.to_string());
    }

    async fn receive(&self) -> Option<String> {
        let mut receiver = self.receiver.lock().await;
        receiver.recv().await
    }
}

// ----------------------------------------------------------------------------
// MockMcpServer — the new piece.
// ----------------------------------------------------------------------------

#[derive(Clone)]
pub struct MockMcpServer {
    transport: MockTransport,
    tools: Arc<Mutex<Vec<Tool>>>,
}

impl MockMcpServer {
    /// Build a server stub with a single `echo` tool registered.
    pub fn new(transport: MockTransport) -> Self {
        Self {
            transport,
            tools: Arc::new(Mutex::new(vec![echo_tool()])),
        }
    }

    /// Add an extra tool before the server starts serving requests.
    pub async fn register_tool(&self, tool: Tool) {
        self.tools.lock().await.push(tool);
    }

    /// Drain one inbound request and emit the matching response.
    ///
    /// Returns the parsed `JsonRpcRequest` so tests can assert against
    /// the request shape as well as the response. Blocks until a request
    /// arrives or the channel closes.
    pub async fn serve_one(&self) -> Option<JsonRpcRequest> {
        let raw = self.transport.receive().await?;
        let parsed: JsonRpcMessage = serde_json::from_str(&raw).ok()?;
        match parsed {
            JsonRpcMessage::Request(req) => {
                let response = self.handle(&req).await;
                let json = serde_json::to_string(&JsonRpcMessage::Response(response)).unwrap();
                self.transport.send(&json).await;
                Some(req)
            }
            _ => None,
        }
    }

    async fn handle(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        match req.method.as_str() {
            "initialize" => JsonRpcResponse::ok(
                &req.id,
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "mock-mcp", "version": "0.0.0"},
                }),
            ),
            "tools/list" => {
                let tools = self.tools.lock().await.clone();
                JsonRpcResponse::ok(
                    &req.id,
                    serde_json::json!({"tools": serde_json::to_value(&tools).unwrap()}),
                )
            }
            "tools/call" => echo_call_response(&req.id, &req.params),
            other => JsonRpcResponse::ok(
                &req.id,
                serde_json::json!({"error": format!("unsupported method: {other}")}),
            ),
        }
    }
}

/// Standard echo tool definition. Matches the fixture used by
/// `tools_call_snapshot.rs`.
fn echo_tool() -> Tool {
    Tool {
        name: "echo".to_string(),
        description: "Returns the input text unchanged.".to_string(),
        input_schema: Some(serde_json::json!({
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
        })),
        annotations: None,
    }
}

/// Match the `tools/call` envelope shape that
/// `tools_call_snapshot::tools_call_echo_success_snapshot` pins. Tests
/// can compare the response produced by `serve_one` against the
/// snapshot file.
fn echo_call_response(id: &str, params: &Option<serde_json::Value>) -> JsonRpcResponse {
    let text = params
        .as_ref()
        .and_then(|p| p.get("arguments"))
        .and_then(|a| a.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let payload = ToolContent {
        content: vec![ContentBlock::Text { text }],
        is_error: None,
    };
    JsonRpcResponse::ok(id, serde_json::to_value(&payload).unwrap())
}

/// End-to-end smoke test: spin up a mock server, send `initialize` +
/// `tools/list` + `tools/call`, assert the responses parse as
/// `JsonRpcResponse` and contain the expected fields.
#[tokio::test]
async fn mock_mcp_server_smoke() {
    let (client_transport, server_transport) = MockTransport::new_pair();
    let server = MockMcpServer::new(server_transport);

    // initialize
    let init = JsonRpcRequest::with_id("init-1", "initialize", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(init)).unwrap();
    client_transport.send(&json).await;
    let req = server.serve_one().await.expect("init request");
    assert_eq!(req.method, "initialize");

    // tools/list
    let list = JsonRpcRequest::with_id("list-1", "tools/list", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(list)).unwrap();
    client_transport.send(&json).await;
    let req = server.serve_one().await.expect("list request");
    assert_eq!(req.method, "tools/list");

    // tools/call
    let call = JsonRpcRequest::with_id(
        "call-1",
        "tools/call",
        Some(serde_json::json!({
            "name": "echo",
            "arguments": {"text": "ping"},
        })),
    );
    let json = serde_json::to_string(&JsonRpcMessage::Request(call)).unwrap();
    client_transport.send(&json).await;
    let req = server.serve_one().await.expect("call request");
    assert_eq!(req.method, "tools/call");
}
