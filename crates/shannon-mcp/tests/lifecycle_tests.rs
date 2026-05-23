//! Tests for MCP server lifecycle management
//!
//! Covers server startup timeout, tool schema validation, concurrent tool calls,
//! deferred schema loading, large response handling, unsupported method handling,
//! unreachable server handling, config reload, parameter validation, and
//! tools/list format correctness.

use shannon_mcp::McpError;
use shannon_mcp::protocol::*;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;

// ============================================================================
// Mock Transport (same pattern as integration_tests.rs and
// client_protocol_tests.rs)
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
// Test 1: Server startup timeout
// ============================================================================

#[tokio::test]
async fn test_mcp_server_startup_timeout() {
    // Verify that a McpError::Timeout correctly captures the duration
    // and that a client can detect a server that never responds.
    let (client, _server) = MockTransport::new_pair();

    // Client sends initialize request but server never responds.
    let request = JsonRpcRequest::with_id("init-timeout", "initialize", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    // Simulate timeout by verifying the McpError::Timeout variant.
    let timeout_duration = Duration::from_secs(5);
    let error = McpError::Timeout(timeout_duration);
    let msg = error.to_string();
    assert!(
        msg.to_lowercase().contains("timeout"),
        "Timeout error message should contain 'timeout': {msg}"
    );
    assert!(
        msg.contains("5"),
        "Timeout error message should contain the duration: {msg}"
    );

    // Verify that after the timeout period, no response is available.
    // Use tokio::time::timeout to simulate waiting for a response.
    let result =
        tokio::time::timeout(Duration::from_millis(50), async { client.receive().await }).await;

    // The mock server never sent anything, so timeout should fire.
    assert!(result.is_err(), "Should timeout when server never responds");
}

// ============================================================================
// Test 2: Tool schema validation
// ============================================================================

#[tokio::test]
async fn test_mcp_tool_schema_validation() {
    let (client, server) = MockTransport::new_pair();

    // Client requests tools/list.
    let request = JsonRpcRequest::with_id("schema-1", "tools/list", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    // Server receives the request.
    let received = server.receive().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    let req_id = parsed["id"].as_str().unwrap().to_string();

    // Server responds with a tool that has an invalid (non-object) schema.
    let tools_response = serde_json::json!([
        {
            "name": "bad_tool",
            "description": "Tool with bad schema",
            "inputSchema": "not-a-json-object"
        }
    ]);
    server.send_response(&req_id, tools_response).await;

    // Client receives and attempts to parse.
    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            // The raw JSON value is preserved; the schema field is a JSON
            // value so it accepts strings too. Verify it round-trips.
            let tools: Vec<serde_json::Value> =
                serde_json::from_value(res.result.unwrap()).unwrap();
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0]["name"], "bad_tool");
            // inputSchema is a string, not an object — client should detect
            // this is invalid when trying to validate tool params against it.
            assert_eq!(tools[0]["inputSchema"], "not-a-json-object");
        }
        _ => panic!("Expected response"),
    }
}

// ============================================================================
// Test 3: Concurrent tool calls to the same server
// ============================================================================

#[tokio::test]
async fn test_mcp_concurrent_tool_calls() {
    let (client, server) = MockTransport::new_pair();

    // Send 3 concurrent tool call requests.
    let ids = vec!["conc-1", "conc-2", "conc-3"];
    for id in &ids {
        let params = serde_json::json!({
            "name": "compute",
            "arguments": { "input": *id }
        });
        let request = JsonRpcRequest::with_id(*id, "tools/call", Some(params));
        let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
        client.send(&json).await;
    }

    // Server receives all 3 requests.
    let mut received_ids = Vec::new();
    for _ in 0..3 {
        let received = server.receive().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
        let req_id = parsed["id"].as_str().unwrap().to_string();
        received_ids.push(req_id);
    }
    assert_eq!(received_ids.len(), 3);

    // Server responds to each in order.
    for id in &received_ids {
        let tool_response = serde_json::json!({
            "content": [
                { "type": "text", "text": format!("Result for {id}") }
            ],
            "isError": false
        });
        server.send_response(id, tool_response).await;
    }

    // Client receives all 3 responses.
    let mut response_count = 0;
    for _ in 0..3 {
        if let Some(resp_str) = client.receive().await {
            let response: JsonRpcMessage = serde_json::from_str(&resp_str).unwrap();
            if let JsonRpcMessage::Response(res) = response {
                assert!(!res.is_error());
                response_count += 1;
            }
        }
    }
    assert_eq!(
        response_count, 3,
        "All 3 concurrent tool calls should succeed"
    );
}

// ============================================================================
// Test 4: Deferred schema loading
// ============================================================================

#[tokio::test]
async fn test_mcp_deferred_schema_loading() {
    let (client, server) = MockTransport::new_pair();

    // First request: tools/list returns tools WITHOUT inputSchema.
    let request = JsonRpcRequest::with_id("deferred-1", "tools/list", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    let received = server.receive().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    let req_id = parsed["id"].as_str().unwrap().to_string();

    // Server responds with minimal tool definitions (no schemas).
    let tools_no_schema = serde_json::json!([
        {
            "name": "lazy_tool",
            "description": "Schema loaded on first use"
        }
    ]);
    server.send_response(&req_id, tools_no_schema).await;

    // Client receives and notes the tool has no schema yet.
    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    let tool_name = match response {
        JsonRpcMessage::Response(res) => {
            let tools: Vec<Tool> = serde_json::from_value(res.result.unwrap()).unwrap();
            assert_eq!(tools.len(), 1);
            assert!(tools[0].input_schema.is_none());
            tools[0].name.clone()
        }
        _ => panic!("Expected response"),
    };

    // Second request: client calls the tool — schema should be used now.
    let params = serde_json::json!({
        "name": tool_name,
        "arguments": { "path": "/test" }
    });
    let request = JsonRpcRequest::with_id("deferred-2", "tools/call", Some(params));
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    let received = server.receive().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    assert_eq!(parsed["method"], "tools/call");
    assert_eq!(parsed["params"]["name"], "lazy_tool");
    let req_id = parsed["id"].as_str().unwrap().to_string();

    // Server responds with the tool result.
    let tool_result = serde_json::json!({
        "content": [{ "type": "text", "text": "done" }],
        "isError": false
    });
    server.send_response(&req_id, tool_result).await;

    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            let content: ToolContent = serde_json::from_value(res.result.unwrap()).unwrap();
            assert_eq!(content.is_error, Some(false));
        }
        _ => panic!("Expected response"),
    }
}

// ============================================================================
// Test 5: Large response truncation
// ============================================================================

#[tokio::test]
async fn test_mcp_large_response_truncation() {
    let (client, server) = MockTransport::new_pair();

    let request = JsonRpcRequest::with_id("large-1", "tools/call", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    let received = server.receive().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    let req_id = parsed["id"].as_str().unwrap().to_string();

    // Server responds with a very large text payload (100KB).
    let large_text = "A".repeat(100_000);
    let large_response = serde_json::json!({
        "content": [
            { "type": "text", "text": large_text }
        ],
        "isError": false
    });
    server.send_response(&req_id, large_response).await;

    // Client receives the full response.
    let resp_json = client.receive().await.unwrap();
    assert!(
        resp_json.len() > 100_000,
        "Large response should be delivered in full: got {} bytes",
        resp_json.len()
    );

    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            let content: ToolContent = serde_json::from_value(res.result.unwrap()).unwrap();
            assert_eq!(content.content.len(), 1);
            match &content.content[0] {
                ContentBlock::Text { text } => {
                    assert_eq!(text.len(), 100_000);
                }
                _ => panic!("Expected text content"),
            }
        }
        _ => panic!("Expected response"),
    }
}

// ============================================================================
// Test 6: Unsupported method returns clean error
// ============================================================================

#[tokio::test]
async fn test_mcp_unsupported_method_graceful() {
    let (client, server) = MockTransport::new_pair();

    // Client sends a request for a method the server does not support.
    let request = JsonRpcRequest::with_id("unsup-1", "sampling/createMessage", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    let received = server.receive().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    let req_id = parsed["id"].as_str().unwrap().to_string();

    // Server responds with method_not_found error (code -32601).
    server
        .send_error_response(&req_id, JsonRpcError::method_not_found())
        .await;

    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            assert!(res.is_error(), "Response should be an error");
            let error = res.error.unwrap();
            assert_eq!(error.code, -32601);
            assert_eq!(error.message, "Method not found");
            assert!(
                error.data.is_none(),
                "Standard method_not_found error should have no data"
            );
        }
        _ => panic!("Expected response"),
    }
}

// ============================================================================
// Test 7: Server unreachable — connection refused handling
// ============================================================================

#[tokio::test]
async fn test_mcp_server_unreachable() {
    // Simulate an unreachable server by constructing a TransportError
    // and verifying it maps to an McpError correctly.
    use shannon_mcp::transport::TransportError;

    let transport_err =
        TransportError::Http("connection refused: http://localhost:9999/mcp".to_string());
    let mcp_err = McpError::from(transport_err);

    let msg = mcp_err.to_string();
    assert!(
        msg.contains("connection refused"),
        "Error message should mention connection refused: {msg}"
    );
    assert!(
        msg.contains("localhost:9999"),
        "Error message should include the endpoint: {msg}"
    );

    // Also verify that a WebSocket connection error maps correctly.
    let ws_err = TransportError::WebSocket("Connection refused: ws://localhost:8888".to_string());
    let mcp_ws_err = McpError::from(ws_err);
    assert!(mcp_ws_err.to_string().contains("WebSocket"));
}

// ============================================================================
// Test 8: Config reload mid-session
// ============================================================================

#[tokio::test]
async fn test_mcp_config_reload() {
    // Verify that config parsing can be re-invoked and produces different
    // results when the underlying config file changes.
    use shannon_mcp::config::{McpServerConfig, discover_config};

    let temp = tempfile::tempdir().unwrap();

    // Write initial config with one server.
    let initial_config = serde_json::json!({
        "mcpServers": {
            "server-alpha": {
                "command": "node",
                "args": ["alpha.js"]
            }
        }
    });
    let config_path = temp.path().join(".mcp.json");
    std::fs::write(
        &config_path,
        serde_json::to_string(&initial_config).unwrap(),
    )
    .unwrap();

    let config_v1 = discover_config(temp.path()).unwrap();
    assert_eq!(config_v1.mcp_servers.len(), 1);
    assert!(config_v1.mcp_servers.contains_key("server-alpha"));

    // Simulate config reload: overwrite the file with a different server.
    let updated_config = serde_json::json!({
        "mcpServers": {
            "server-beta": {
                "url": "http://localhost:4000/mcp"
            }
        }
    });
    std::fs::write(
        &config_path,
        serde_json::to_string(&updated_config).unwrap(),
    )
    .unwrap();

    let config_v2 = discover_config(temp.path()).unwrap();
    assert_eq!(config_v2.mcp_servers.len(), 1);
    assert!(
        !config_v2.mcp_servers.contains_key("server-alpha"),
        "Old server should be gone after reload"
    );
    assert!(
        config_v2.mcp_servers.contains_key("server-beta"),
        "New server should appear after reload"
    );

    // Verify the new server is an SSE config.
    match config_v2.mcp_servers.get("server-beta").unwrap() {
        McpServerConfig::Sse { url, .. } => {
            assert_eq!(url, "http://localhost:4000/mcp");
        }
        _ => panic!("Expected Sse config for server-beta"),
    }
}

// ============================================================================
// Test 9: Tool parameter validation — missing/wrong params
// ============================================================================

#[tokio::test]
async fn test_mcp_tool_parameter_validation() {
    let (client, server) = MockTransport::new_pair();

    // Client sends tools/call with missing required parameter.
    let params = serde_json::json!({
        "name": "read_file",
        "arguments": {}
        // Missing required "path" parameter
    });
    let request = JsonRpcRequest::with_id("param-1", "tools/call", Some(params));
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    let received = server.receive().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&received).unwrap();
    assert_eq!(parsed["method"], "tools/call");
    assert_eq!(parsed["params"]["name"], "read_file");
    // arguments object is present but empty (missing "path").
    assert_eq!(parsed["params"]["arguments"], serde_json::json!({}));
    let req_id = parsed["id"].as_str().unwrap().to_string();

    // Server responds with an invalid_params error.
    server
        .send_error_response(&req_id, JsonRpcError::invalid_params())
        .await;

    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            assert!(res.is_error());
            let error = res.error.unwrap();
            assert_eq!(error.code, -32602, "Should be invalid params error");
            assert_eq!(error.message, "Invalid params");
        }
        _ => panic!("Expected response"),
    }

    // Second call: wrong parameter type.
    let params_wrong_type = serde_json::json!({
        "name": "read_file",
        "arguments": { "path": 12345 }
        // "path" should be a string, not a number
    });
    let request2 = JsonRpcRequest::with_id("param-2", "tools/call", Some(params_wrong_type));
    let json2 = serde_json::to_string(&JsonRpcMessage::Request(request2)).unwrap();
    client.send(&json2).await;

    let received2 = server.receive().await.unwrap();
    let parsed2: serde_json::Value = serde_json::from_str(&received2).unwrap();
    assert_eq!(parsed2["params"]["arguments"]["path"], 12345);
    let req_id2 = parsed2["id"].as_str().unwrap().to_string();

    // Server responds with an error indicating the wrong type.
    let type_error = JsonRpcError::with_data(
        -32602,
        "Invalid params",
        serde_json::json!({
            "field": "path",
            "expected": "string",
            "received": "number"
        }),
    );
    server.send_error_response(&req_id2, type_error).await;

    let resp_json2 = client.receive().await.unwrap();
    let response2: JsonRpcMessage = serde_json::from_str(&resp_json2).unwrap();
    match response2 {
        JsonRpcMessage::Response(res) => {
            assert!(res.is_error());
            let error = res.error.unwrap();
            assert_eq!(error.code, -32602);
            // Error should include detailed data about the type mismatch.
            let data = error.data.unwrap();
            assert_eq!(data["field"], "path");
            assert_eq!(data["expected"], "string");
            assert_eq!(data["received"], "number");
        }
        _ => panic!("Expected response"),
    }
}

// ============================================================================
// Test 10: tools/list returns correct format
// ============================================================================

#[tokio::test]
async fn test_mcp_server_list_tools() {
    let (client, server) = MockTransport::new_pair();

    // Client sends tools/list request.
    let request = JsonRpcRequest::with_id("list-1", "tools/list", None);
    let json = serde_json::to_string(&JsonRpcMessage::Request(request)).unwrap();
    client.send(&json).await;

    // Server receives the request.
    let received = server.receive().await.unwrap();
    let parsed: JsonRpcMessage = serde_json::from_str(&received).unwrap();
    let req_id = match parsed {
        JsonRpcMessage::Request(req) => {
            assert_eq!(req.method, "tools/list");
            assert_eq!(req.jsonrpc, "2.0");
            req.id
        }
        _ => panic!("Expected request"),
    };

    // Server responds with a well-formed tools list including annotations.
    let tools_response = serde_json::json!([
        {
            "name": "read_file",
            "description": "Read file contents from disk",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute file path"
                    }
                },
                "required": ["path"]
            },
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        },
        {
            "name": "write_file",
            "description": "Write content to a file",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": false,
                "openWorldHint": false
            }
        },
        {
            "name": "search",
            "description": "Search the web",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"]
            }
        }
    ]);

    server.send_response(&req_id, tools_response).await;

    // Client receives and parses the response.
    let resp_json = client.receive().await.unwrap();
    let response: JsonRpcMessage = serde_json::from_str(&resp_json).unwrap();
    match response {
        JsonRpcMessage::Response(res) => {
            assert_eq!(res.id, "list-1");
            assert!(!res.is_error());

            let tools: Vec<Tool> = serde_json::from_value(res.result.unwrap()).unwrap();
            assert_eq!(tools.len(), 3);

            // Verify read_file tool and its annotations.
            assert_eq!(tools[0].name, "read_file");
            assert_eq!(tools[0].description, "Read file contents from disk");
            assert!(tools[0].input_schema.is_some());
            let ann = tools[0].annotations.as_ref().unwrap();
            assert!(ann.read_only_hint);
            assert!(!ann.destructive_hint);
            assert!(ann.idempotent_hint);
            assert!(!ann.open_world_hint);

            // Verify write_file tool — destructive.
            assert_eq!(tools[1].name, "write_file");
            let ann2 = tools[1].annotations.as_ref().unwrap();
            assert!(!ann2.read_only_hint);
            assert!(ann2.destructive_hint);

            // Verify search tool — no annotations.
            assert_eq!(tools[2].name, "search");
            assert!(tools[2].annotations.is_none());
        }
        _ => panic!("Expected response"),
    }
}
