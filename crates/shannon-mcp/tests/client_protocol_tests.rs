//! Tests for MCP client protocol handling
//!
//! Covers request/response ID correlation, tool listing/invocation,
//! resource operations, server capability negotiation, timeout handling,
//! and error handling for malformed responses.

use shannon_mcp::McpError;
use shannon_mcp::protocol::*;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;

// ============================================================================
// Mock Transport (reused from integration_tests.rs pattern)
// ============================================================================

#[derive(Clone)]
struct MockTransport {
    sender: Arc<Mutex<tokio::sync::mpsc::UnboundedSender<String>>>,
    receiver: Arc<Mutex<tokio::sync::mpsc::UnboundedReceiver<String>>>,
}

impl MockTransport {
    fn new_pair() -> (Self, Self) {
        let (tx1, rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, rx2) = tokio::sync::mpsc::unbounded_channel();

        let transport1 = Self {
            sender: Arc::new(Mutex::new(tx1)),
            receiver: Arc::new(Mutex::new(rx2)),
        };
        let transport2 = Self {
            sender: Arc::new(Mutex::new(tx2)),
            receiver: Arc::new(Mutex::new(rx1)),
        };

        (transport1, transport2)
    }

    async fn send(&self, message: &str) {
        let sender = self.sender.lock().await;
        let _ = sender.send(message.to_string());
    }

    async fn receive(&self) -> Option<String> {
        let mut receiver = self.receiver.lock().await;
        receiver.recv().await
    }

    async fn send_response(&self, id: &str, result: serde_json::Value) {
        let response = JsonRpcResponse::ok(id, result);
        let message = JsonRpcMessage::Response(response);
        let json = serde_json::to_string(&message).unwrap();
        self.send(&json).await;
    }

    async fn send_error_response(&self, id: &str, error: JsonRpcError) {
        let response = JsonRpcResponse::error(id, error);
        let message = JsonRpcMessage::Response(response);
        let json = serde_json::to_string(&message).unwrap();
        self.send(&json).await;
    }
}

// ============================================================================
// Request/Response ID Correlation Tests
// ============================================================================

#[tokio::test]
async fn test_request_response_id_correlation() {
    let (client, server) = MockTransport::new_pair();

    // Client sends request with specific ID.
    let request = JsonRpcRequest::with_id("correlate-42", "tools/list", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    // Server receives and extracts the ID.
    let received = server.receive().await.unwrap();
    let parsed: JsonRpcMessage = serde_json::from_str(&received).unwrap();
    let req_id = match parsed {
        JsonRpcMessage::Request(req) => req.id,
        _ => panic!("Expected request"),
    };
    assert_eq!(req_id, "correlate-42");

    // Server responds with matching ID.
    server.send_response(&req_id, serde_json::json!([])).await;

    // Client receives the correlated response.
    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            assert_eq!(res.id, "correlate-42");
        }
        _ => panic!("Expected response"),
    }
}

#[tokio::test]
async fn test_multiple_requests_maintain_id_order() {
    let (client, server) = MockTransport::new_pair();

    let ids: Vec<&str> = vec!["req-a", "req-b", "req-c"];
    for id in &ids {
        let request = JsonRpcRequest::with_id(*id, "method", None);
        let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
        client.send(&json).await;
    }

    // Server receives all and responds in reverse order.
    let mut received_ids = Vec::new();
    for _ in 0..3 {
        let received = server.receive().await.unwrap();
        let parsed: JsonRpcMessage = serde_json::from_str(&received).unwrap();
        if let JsonRpcMessage::Request(req) = parsed {
            received_ids.push(req.id);
        }
    }
    assert_eq!(received_ids, vec!["req-a", "req-b", "req-c"]);

    // Respond in reverse.
    for id in received_ids.iter().rev() {
        server.send_response(id, serde_json::json!("ok")).await;
    }

    let mut response_ids = Vec::new();
    for _ in 0..3 {
        let resp_json = client.receive().await.unwrap();
        let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
        if let JsonRpcMessage::Response(res) = response {
            response_ids.push(res.id);
        }
    }
    assert_eq!(response_ids, vec!["req-c", "req-b", "req-a"]);
}

// ============================================================================
// Tool Listing and Invocation Tests
// ============================================================================

#[tokio::test]
async fn test_tool_listing_response() {
    let (client, server) = MockTransport::new_pair();

    let request = JsonRpcRequest::with_id("tl-1", "tools/list", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    let received = server.receive().await.unwrap();
    let parsed: JsonRpcMessage = serde_json::from_str(&received).unwrap();
    let req_id = match parsed {
        JsonRpcMessage::Request(req) => req.id,
        _ => panic!("Expected request"),
    };

    let tools_response = serde_json::json!([
        {
            "name": "read_file",
            "description": "Read a file from disk",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "search",
            "description": "Search files",
            "inputSchema": { "type": "object" }
        }
    ]);

    server.send_response(&req_id, tools_response).await;

    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            assert_eq!(res.id, "tl-1");
            let tools: Vec<Tool> = serde_json::from_value(res.result.unwrap()).unwrap();
            assert_eq!(tools.len(), 2);
            assert_eq!(tools[0].name, "read_file");
            assert_eq!(tools[1].name, "search");
        }
        _ => panic!("Expected response"),
    }
}

#[tokio::test]
async fn test_tool_invocation_with_arguments() {
    let (client, server) = MockTransport::new_pair();

    let params = serde_json::json!({
        "name": "read_file",
        "arguments": { "path": "/tmp/test.txt" }
    });
    let request = JsonRpcRequest::with_id("tc-1", "tools/call", Some(params));
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    // Verify the request payload on the server side.
    let received = server.receive().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    assert_eq!(parsed["method"], "tools/call");
    assert_eq!(parsed["params"]["name"], "read_file");
    assert_eq!(parsed["params"]["arguments"]["path"], "/tmp/test.txt");

    let req_id = parsed["id"].as_str().unwrap().to_string();

    // Respond with tool content.
    let tool_response = serde_json::json!({
        "content": [
            { "type": "text", "text": "Hello, world!" }
        ],
        "isError": false
    });
    server.send_response(&req_id, tool_response).await;

    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            let content: ToolContent = serde_json::from_value(res.result.unwrap()).unwrap();
            assert_eq!(content.content.len(), 1);
            assert_eq!(content.is_error, Some(false));
            match &content.content[0] {
                ContentBlock::Text { text } => assert_eq!(text, "Hello, world!"),
                _ => panic!("Expected text content"),
            }
        }
        _ => panic!("Expected response"),
    }
}

#[tokio::test]
async fn test_tool_invocation_error_response() {
    let (client, server) = MockTransport::new_pair();

    let request = JsonRpcRequest::with_id("tc-err", "tools/call", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    let received = server.receive().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    let req_id = parsed["id"].as_str().unwrap().to_string();

    let error_response = serde_json::json!({
        "content": [
            { "type": "text", "text": "Tool not found: nonexistent" }
        ],
        "isError": true
    });
    server.send_response(&req_id, error_response).await;

    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            let content: ToolContent = serde_json::from_value(res.result.unwrap()).unwrap();
            assert_eq!(content.is_error, Some(true));
        }
        _ => panic!("Expected response"),
    }
}

// ============================================================================
// Resource Listing Tests
// ============================================================================

#[tokio::test]
async fn test_resource_listing_response() {
    let (client, server) = MockTransport::new_pair();

    let request = JsonRpcRequest::with_id("rl-1", "resources/list", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    let received = server.receive().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    let req_id = parsed["id"].as_str().unwrap().to_string();

    let resources_response = serde_json::json!([
        {
            "uri": "file:///project/README.md",
            "name": "README",
            "description": "Project readme file",
            "mimeType": "text/markdown"
        },
        {
            "uri": "db:///users",
            "name": "Users",
            "description": "",
            "mimeType": "application/json"
        }
    ]);

    server.send_response(&req_id, resources_response).await;

    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            let resources: Vec<Resource> = serde_json::from_value(res.result.unwrap()).unwrap();
            assert_eq!(resources.len(), 2);
            assert_eq!(resources[0].uri, "file:///project/README.md");
            assert_eq!(resources[0].name, "README");
            assert_eq!(resources[0].mime_type.as_deref(), Some("text/markdown"));
            assert_eq!(resources[1].description, "");
        }
        _ => panic!("Expected response"),
    }
}

#[tokio::test]
async fn test_resource_read_response() {
    let (client, server) = MockTransport::new_pair();

    let params = serde_json::json!({ "uri": "file:///data.csv" });
    let request = JsonRpcRequest::with_id("rr-1", "resources/read", Some(params));
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    let received = server.receive().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    assert_eq!(parsed["params"]["uri"], "file:///data.csv");
    let req_id = parsed["id"].as_str().unwrap().to_string();

    let read_response = serde_json::json!({
        "uri": "file:///data.csv",
        "mimeType": "text/csv",
        "contents": [
            { "type": "text", "text": "name,age\nAlice,30\nBob,25" }
        ]
    });
    server.send_response(&req_id, read_response).await;

    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            let content: ResourceContent = serde_json::from_value(res.result.unwrap()).unwrap();
            assert_eq!(content.uri, "file:///data.csv");
            assert_eq!(content.mime_type.as_deref(), Some("text/csv"));
            assert_eq!(content.contents.len(), 1);
        }
        _ => panic!("Expected response"),
    }
}

// ============================================================================
// Server Capability Negotiation Tests
// ============================================================================

#[tokio::test]
async fn test_initialize_handshake() {
    let (client, server) = MockTransport::new_pair();

    // Client sends initialize request.
    let params = serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": { "name": "test-client", "version": "0.1.0" }
    });
    let request = JsonRpcRequest::with_id("init-1", "initialize", Some(params));
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    let received = server.receive().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    assert_eq!(parsed["method"], "initialize");
    assert_eq!(parsed["params"]["protocolVersion"], "2024-11-05");
    let req_id = parsed["id"].as_str().unwrap().to_string();

    // Server responds with capabilities.
    let init_response = serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": { "listChanged": true },
            "resources": { "subscribe": false, "listChanged": false },
            "prompts": { "listChanged": false }
        },
        "serverInfo": { "name": "test-server", "version": "2.0.0" }
    });
    server.send_response(&req_id, init_response).await;

    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            let result: InitializeResult = serde_json::from_value(res.result.unwrap()).unwrap();
            assert_eq!(result.protocol_version, "2024-11-05");
            assert!(result.capabilities.tools.is_some());
            assert!(result.capabilities.tools.unwrap().list_changed);
            assert!(result.capabilities.resources.is_some());
            assert!(result.capabilities.prompts.is_some());
            assert_eq!(result.server_info.unwrap().name, "test-server");
        }
        _ => panic!("Expected response"),
    }
}

#[tokio::test]
async fn test_capability_only_tools() {
    let json = serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": { "listChanged": false }
        },
        "serverInfo": { "name": "tool-only", "version": "1.0.0" }
    });
    let result: InitializeResult = serde_json::from_value(json).unwrap();
    assert!(result.capabilities.tools.is_some());
    assert!(result.capabilities.resources.is_none());
    assert!(result.capabilities.prompts.is_none());
}

#[tokio::test]
async fn test_capability_with_all_features() {
    let json = serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": { "listChanged": true },
            "resources": { "subscribe": true, "listChanged": true },
            "prompts": { "listChanged": true },
            "logging": { "level": "debug" }
        },
        "serverInfo": { "name": "full-server", "version": "3.0.0" }
    });
    let result: InitializeResult = serde_json::from_value(json).unwrap();
    assert!(result.capabilities.tools.unwrap().list_changed);
    let res = result.capabilities.resources.unwrap();
    assert!(res.subscribe);
    assert!(res.list_changed);
    assert!(result.capabilities.prompts.unwrap().list_changed);
    assert!(result.capabilities.logging.is_some());
}

// ============================================================================
// Timeout Handling Tests
// ============================================================================

#[tokio::test]
async fn test_timeout_error_on_no_response() {
    // Simulate what happens when a response never arrives by verifying
    // the McpError::Timeout variant is correctly constructed.
    let duration = Duration::from_secs(30);
    let error = McpError::Timeout(duration);
    let msg = error.to_string();
    assert!(msg.contains("30"));
    assert!(msg.to_lowercase().contains("timeout"));
}

// ============================================================================
// Error Handling for Malformed Responses
// ============================================================================

#[tokio::test]
async fn test_malformed_json_response() {
    let (client, server) = MockTransport::new_pair();

    let request = JsonRpcRequest::with_id("mal-1", "test", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    // Receive on server side to drain the message.
    let _ = server.receive().await;

    // Server sends back malformed JSON.
    server.send("this is not valid json{{{").await;

    let resp_str = client.receive().await.unwrap();
    let result: Result<JsonRpcMessage, _> = serde_json::from_str(&resp_str);
    assert!(result.is_err(), "Malformed JSON should fail to parse");
}

#[tokio::test]
async fn test_jsonrpc_error_response() {
    let (client, server) = MockTransport::new_pair();

    let request = JsonRpcRequest::with_id("err-1", "unknown_method", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    let _ = server.receive().await;

    server
        .send_error_response("err-1", JsonRpcError::method_not_found())
        .await;

    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            assert!(res.is_error());
            let error = res.error.unwrap();
            assert_eq!(error.code, -32601);
            assert_eq!(error.message, "Method not found");
        }
        _ => panic!("Expected response"),
    }
}

#[tokio::test]
async fn test_response_missing_result_and_error() {
    // A JSON-RPC response with neither result nor error is unusual
    // but should still parse (both fields are optional).
    let json_str = r#"{"jsonrpc":"2.0","id":"empty-1"}"#;
    let parsed: Result<JsonRpcResponse, _> = serde_json::from_str(json_str);
    assert!(parsed.is_ok());
    let response = parsed.unwrap();
    assert_eq!(response.id, "empty-1");
    assert!(response.result.is_none());
    assert!(response.error.is_none());
    assert!(!response.is_error());
}
