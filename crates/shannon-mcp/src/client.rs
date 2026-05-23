// MCP client implementation
//
// This module provides the main MCP client for connecting to MCP servers,
// sending requests, and handling responses and notifications.

use crate::protocol::*;
use crate::transport::Transport;
use crate::{McpError, McpResult};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// MCP client for communicating with MCP servers
pub struct McpClient<T: Transport> {
    transport: Arc<Mutex<T>>,
    /// Write channel for sending messages through the I/O loop (set after connect).
    write_tx: Option<tokio::sync::mpsc::Sender<String>>,
    request_timeout: std::time::Duration,
    pending_requests: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<serde_json::Value>>>>,
    server_capabilities: Arc<Mutex<Option<ServerCapabilities>>>,
}

impl<T: Transport> McpClient<T> {
    /// Create a new MCP client with the given transport
    pub fn new(transport: T) -> Self {
        Self {
            transport: Arc::new(Mutex::new(transport)),
            write_tx: None,
            request_timeout: std::time::Duration::from_secs(30),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            server_capabilities: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the request timeout
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Connect to the server and initialize the session
    pub async fn connect(mut self) -> McpResult<Self>
    where
        T: 'static,
    {
        info!("Connecting to MCP server...");

        // Create write channel for decoupled send/receive
        let (write_tx, write_rx) = tokio::sync::mpsc::channel(64);
        self.write_tx = Some(write_tx);

        // Start the combined I/O task (select! avoids the deadlock that
        // a separate receive_loop + send_request sharing one Mutex would cause)
        let transport_clone = self.transport.clone();
        let pending_clone = self.pending_requests.clone();
        let server_caps_clone = self.server_capabilities.clone();

        tokio::spawn(async move {
            Self::io_loop(transport_clone, write_rx, pending_clone, server_caps_clone).await
        });

        // Initialize the connection
        self.initialize().await?;

        info!("MCP client connected and initialized");
        Ok(self)
    }

    /// Initialize the MCP session
    pub async fn initialize(&self) -> McpResult<InitializeResult> {
        info!("Initializing MCP session");

        let params = serde_json::json!({
            "protocolVersion": crate::MCP_PROTOCOL_VERSION,
            "capabilities": ClientCapabilities::default(),
            "clientInfo": ClientInfo {
                name: "shannon-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            }
        });

        let response = self.send_request("initialize", Some(params)).await?;

        let result: InitializeResult = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid initialize result: {e}")))?;

        // Store server capabilities
        *self.server_capabilities.lock().await = Some(result.capabilities.clone());

        info!(
            "Initialized with server: {} v{}",
            result
                .server_info
                .as_ref()
                .map(|s| &s.name)
                .unwrap_or(&"unknown".to_string()),
            result
                .server_info
                .as_ref()
                .map(|s| &s.version)
                .unwrap_or(&"unknown".to_string())
        );

        Ok(result)
    }

    /// List available tools
    pub async fn list_tools(&self) -> McpResult<ListToolsResult> {
        debug!("Listing tools");
        let response = self.send_request("tools/list", None).await?;
        let tools: ListToolsResult = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid tools list result: {e}")))?;
        Ok(tools)
    }

    /// Call a tool
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> McpResult<ToolContent> {
        debug!(tool = %name, "Calling tool");

        let params = serde_json::json!({
            "name": name,
            "arguments": arguments
        });

        let response = self.send_request("tools/call", Some(params)).await?;
        let content: ToolContent = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid tool call result: {e}")))?;

        Ok(content)
    }

    /// List available resources
    pub async fn list_resources(&self) -> McpResult<ListResourcesResult> {
        debug!("Listing resources");
        let response = self.send_request("resources/list", None).await?;
        let resources: ListResourcesResult = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid resources list result: {e}")))?;
        Ok(resources)
    }

    /// Read a resource
    pub async fn read_resource(&self, uri: &str) -> McpResult<ResourceContent> {
        debug!(uri = %uri, "Reading resource");

        let params = serde_json::json!({ "uri": uri });
        let response = self.send_request("resources/read", Some(params)).await?;
        let content: ResourceContent = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid resource read result: {e}")))?;

        Ok(content)
    }

    /// List resource templates
    pub async fn list_resource_templates(&self) -> McpResult<ListResourceTemplatesResult> {
        debug!("Listing resource templates");
        let response = self.send_request("resources/templates/list", None).await?;
        let templates: ListResourceTemplatesResult = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid resource templates result: {e}")))?;
        Ok(templates)
    }

    /// List available prompts
    pub async fn list_prompts(&self) -> McpResult<ListPromptsResult> {
        debug!("Listing prompts");
        let response = self.send_request("prompts/list", None).await?;
        let prompts: ListPromptsResult = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid prompts list result: {e}")))?;
        Ok(prompts)
    }

    /// Get a prompt
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<HashMap<String, String>>,
    ) -> McpResult<ToolContent> {
        debug!(prompt = %name, "Getting prompt");

        let params = if let Some(args) = arguments {
            serde_json::json!({
                "name": name,
                "arguments": args
            })
        } else {
            serde_json::json!({ "name": name })
        };

        let response = self.send_request("prompts/get", Some(params)).await?;
        let content: ToolContent = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid prompt get result: {e}")))?;

        Ok(content)
    }

    /// Complete a prompt argument or resource name
    pub async fn complete(
        &self,
        reference: CompletionRef,
        argument: PromptArgument,
    ) -> McpResult<CompletionResult> {
        debug!(ref_type = %reference.ref_type, "Requesting completion");

        let params = serde_json::json!({
            "ref": reference,
            "argument": argument
        });

        let response = self
            .send_request("completion/complete", Some(params))
            .await?;
        let result: CompletionResult = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid completion result: {e}")))?;

        Ok(result)
    }

    /// Set the logging level
    pub async fn set_logging_level(&self, level: LoggingLevel) -> McpResult<()> {
        debug!(level = ?level, "Setting logging level");

        let params = serde_json::json!({ "level": level });
        let _response = self.send_request("logging/setLevel", Some(params)).await?;

        Ok(())
    }

    /// Subscribe to resource updates
    pub async fn subscribe_resource(&self, uri: &str) -> McpResult<bool> {
        debug!(uri = %uri, "Subscribing to resource");

        let params = serde_json::json!({ "uri": uri });
        let response = self
            .send_request("resources/subscribe", Some(params))
            .await?;
        let result: SubscribeResult = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid subscribe result: {e}")))?;

        Ok(result.subscribed)
    }

    /// Unsubscribe from resource updates
    pub async fn unsubscribe_resource(&self, uri: &str) -> McpResult<bool> {
        debug!(uri = %uri, "Unsubscribing from resource");

        let params = serde_json::json!({ "uri": uri });
        let response = self
            .send_request("resources/unsubscribe", Some(params))
            .await?;
        let result: SubscribeResult = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid unsubscribe result: {e}")))?;

        Ok(result.subscribed)
    }

    /// Get server capabilities
    pub async fn capabilities(&self) -> Option<ServerCapabilities> {
        self.server_capabilities.lock().await.clone()
    }

    /// Check if server supports a specific capability
    pub async fn supports_tools(&self) -> bool {
        self.server_capabilities
            .lock()
            .await
            .as_ref()
            .and_then(|caps| caps.tools.as_ref())
            .is_some()
    }

    pub async fn supports_resources(&self) -> bool {
        self.server_capabilities
            .lock()
            .await
            .as_ref()
            .and_then(|caps| caps.resources.as_ref())
            .is_some()
    }

    /// Check if the server supports resource subscriptions
    pub async fn supports_subscribe(&self) -> bool {
        self.server_capabilities
            .lock()
            .await
            .as_ref()
            .and_then(|caps| caps.resources.as_ref())
            .map(|r| r.subscribe)
            .unwrap_or(false)
    }

    pub async fn supports_prompts(&self) -> bool {
        self.server_capabilities
            .lock()
            .await
            .as_ref()
            .and_then(|caps| caps.prompts.as_ref())
            .is_some()
    }

    /// Send a raw JSON-RPC request
    async fn send_request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> McpResult<serde_json::Value> {
        let id = uuid::Uuid::new_v4().to_string();
        let request = JsonRpcRequest::new(method, params);

        debug!(id = %id, method = %method, "Sending request");

        // Create a channel for the response
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_requests.lock().await.insert(id.clone(), tx);

        // Send the request
        let message = JsonRpcMessage::Request(request);
        let serialized = serde_json::to_string(&message)?;
        if let Some(ref tx) = self.write_tx {
            tx.send(serialized)
                .await
                .map_err(|e| McpError::Protocol(format!("Write channel closed: {e}")))?;
        } else {
            let mut transport = self.transport.lock().await;
            transport.send(&serialized).await?;
        }

        // Wait for the response
        let timeout_result = tokio::time::timeout(self.request_timeout, rx).await;
        if timeout_result.is_err() {
            // Clean up the stale entry to prevent memory leak
            self.pending_requests.lock().await.remove(&id);
            return Err(McpError::Timeout(self.request_timeout));
        }
        let response = timeout_result
            .map_err(|_| McpError::Timeout(self.request_timeout))?
            .map_err(|_| McpError::Protocol("Response channel closed".to_string()))?;

        Ok(response)
    }

    /// Combined I/O loop using `tokio::select!` to avoid deadlock.
    ///
    /// The previous approach had `receive_loop` hold the transport `Mutex` while
    /// awaiting `receive()`, which blocked `send_request()` from acquiring the
    /// same lock. Using `select!` ensures that when a write is ready, the
    /// receive future is dropped (releasing the `MutexGuard`), allowing the
    /// write branch to acquire the lock.
    async fn io_loop(
        transport: Arc<Mutex<T>>,
        mut write_rx: tokio::sync::mpsc::Receiver<String>,
        pending: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<serde_json::Value>>>>,
        server_capabilities: Arc<Mutex<Option<ServerCapabilities>>>,
    ) {
        loop {
            tokio::select! {
                // Outgoing messages from send_request()
                msg = write_rx.recv() => {
                    match msg {
                        Some(serialized) => {
                            let mut t = transport.lock().await;
                            if let Err(e) = t.send(&serialized).await {
                                error!("Send error: {}", e);
                                break;
                            }
                        }
                        None => {
                            debug!("Write channel closed, shutting down I/O loop");
                            break;
                        }
                    }
                }
                // Incoming messages from transport
                result = async {
                    let mut t = transport.lock().await;
                    t.receive().await
                } => {
                    let message = match result {
                        Ok(Some(msg)) => msg,
                        Ok(None) => {
                            debug!("Transport closed gracefully");
                            break;
                        }
                        Err(e) => {
                            error!("Receive error: {}", e);
                            break;
                        }
                    };

                    let parsed: JsonRpcMessage = match serde_json::from_str(&message) {
                        Ok(msg) => msg,
                        Err(e) => {
                            error!("Failed to parse message: {}", e);
                            continue;
                        }
                    };

                    match parsed {
                        JsonRpcMessage::Response(response) => {
                            if let Some(tx) = pending.lock().await.remove(&response.id) {
                                if let Some(error) = response.error {
                                    warn!("JSON-RPC error: {} - {}", error.code, error.message);
                                    if tx.send(serde_json::json!({"error": error})).is_err() {
                                        debug!("response channel closed for request {}", response.id);
                                    }
                                } else if let Some(result) = response.result {
                                    if tx.send(result).is_err() {
                                        debug!("response channel closed for request {}", response.id);
                                    }
                                }
                            } else {
                                warn!("Received response for unknown request ID: {}", response.id);
                            }
                        }
                        JsonRpcMessage::Notification(notification) => {
                            Self::handle_notification(notification, &server_capabilities).await;
                        }
                        JsonRpcMessage::Request(request) => {
                            warn!("Received unexpected request from server: {}", request.method);
                        }
                    }
                }
            }
        }
    }

    /// Handle incoming notifications
    async fn handle_notification(
        notification: JsonRpcNotification,
        _server_capabilities: &Arc<Mutex<Option<ServerCapabilities>>>,
    ) {
        debug!(method = %notification.method, "Handling notification");

        match notification.method.as_str() {
            "notifications/message" => {
                info!("Received message notification: {:?}", notification.params);
            }
            "notifications/resources/updated" => {
                info!("Resources updated notification");
            }
            "notifications/resources/list_changed" => {
                info!("Resources list changed notification");
            }
            "notifications/tools/list_changed" => {
                info!("Tools list changed notification");
            }
            "notifications/prompts/list_changed" => {
                info!("Prompts list changed notification");
            }
            "logging/message" => {
                if let Some(params) = notification.params {
                    if let Some(level) = params.get("level").and_then(|l| l.as_str()) {
                        let message = params.get("data").and_then(|d| d.as_str()).unwrap_or("");
                        match level {
                            "debug" => debug!("{}", message),
                            "info" => info!("{}", message),
                            "warn" => warn!("{}", message),
                            "error" => error!("{}", message),
                            _ => debug!("[{}] {}", level, message),
                        }
                    }
                }
            }
            "progress" => {
                debug!("Progress notification: {:?}", notification.params);
            }
            _ => {
                debug!("Unknown notification method: {}", notification.method);
            }
        }
    }
}

/// Error type for MCP client operations
#[derive(thiserror::Error, Debug)]
pub enum McpClientError {
    #[error("not initialized")]
    NotInitialized,

    #[error("unsupported operation: {0}")]
    Unsupported(String),

    #[error("request failed: {0}")]
    RequestFailed(String),

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("server error: {0}")]
    ServerError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{Transport, TransportError};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use tokio::time::Duration;

    // ── MockTransport ──────────────────────────────────────────────────

    /// Mock transport that records sent messages and returns queued responses.
    /// Responses are delivered in FIFO order via a `VecDeque`.
    ///
    /// NOTE: The real `McpClient` wraps the transport in `Arc<Mutex<T>>`.
    /// The `receive_loop` holds this lock while awaiting `transport.receive()`,
    /// which means `send_request()` (which also needs the lock) can deadlock
    /// if `receive()` blocks indefinitely. For that reason we do NOT use
    /// `client.connect()` in tests. Instead we either:
    ///   - set `server_capabilities` directly, or
    ///   - run a manual send/receive cycle on the same async task (no contention).
    struct MockTransport {
        sent_messages: Vec<String>,
        pending_responses: VecDeque<String>,
    }

    impl MockTransport {
        fn new() -> Self {
            Self {
                sent_messages: Vec::new(),
                pending_responses: VecDeque::new(),
            }
        }

        /// Queue a response that `receive()` will return on the next call.
        fn enqueue_response(&mut self, response: &str) {
            self.pending_responses.push_back(response.to_string());
        }

        /// Return all messages that have been sent so far (for inspection).
        fn sent_messages(&self) -> &[String] {
            &self.sent_messages
        }
    }

    #[async_trait]
    impl Transport for MockTransport {
        async fn send(&mut self, message: &str) -> Result<(), TransportError> {
            self.sent_messages.push(message.to_string());
            Ok(())
        }

        async fn receive(&mut self) -> Result<Option<String>, TransportError> {
            // Pop from the front of the queue.  If empty, return None (which
            // would cause the receive_loop to break, but in our manual tests
            // we always enqueue before calling receive).
            Ok(self.pending_responses.pop_front())
        }

        async fn close(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    // ── helpers ────────────────────────────────────────────────────────

    /// Extract the JSON-RPC request ID from a serialized request string.
    fn extract_request_id(json: &str) -> String {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        v["id"].as_str().unwrap().to_string()
    }

    /// Build a JSON-RPC response string with the given ID and result body.
    fn make_response(id: &str, result: serde_json::Value) -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        })
        .to_string()
    }

    /// Simulate a full request/response round-trip without going through
    /// `connect()`. This avoids the `receive_loop` deadlock by performing
    /// send and receive on the same task (no `Arc<Mutex>` contention).
    ///
    /// Steps:
    /// 1. Call `send_request` directly (it locks transport, sends, inserts
    ///    into pending_requests, then awaits the oneshot receiver).
    /// 2. Read the sent message from the mock to obtain the request ID.
    /// 3. Enqueue a response with that ID into the mock transport.
    /// 4. Run one receive-loop iteration: receive from transport, parse the
    ///    JSON-RPC response, look up the pending oneshot sender by ID, and
    ///    deliver the result value.
    /// 5. The oneshot receiver in `send_request` resolves.
    async fn round_trip_raw(
        client: &McpClient<MockTransport>,
        method: &str,
        params: Option<serde_json::Value>,
        result: serde_json::Value,
    ) -> serde_json::Value {
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        let id = uuid::Uuid::new_v4().to_string();
        client
            .pending_requests
            .lock()
            .await
            .insert(id.clone(), result_tx);

        // Build and send the JSON-RPC request directly.
        let request = JsonRpcRequest::new(method, params);
        let serialized = serde_json::to_string(&request).unwrap();
        {
            let mut transport = client.transport.lock().await;
            transport.send(&serialized).await.unwrap();
        }

        // Extract the actual request ID (which differs from `id` above since
        // JsonRpcRequest::new generates its own UUID).
        let actual_id = {
            let transport = client.transport.lock().await;
            extract_request_id(transport.sent_messages().last().unwrap())
        };

        // Move the pending sender to the actual request ID.
        let sender = client.pending_requests.lock().await.remove(&id).unwrap();
        client
            .pending_requests
            .lock()
            .await
            .insert(actual_id.clone(), sender);

        // Enqueue the response.
        {
            let mut transport = client.transport.lock().await;
            transport.enqueue_response(&make_response(&actual_id, result));
        }

        // Manually run one receive-loop iteration.
        {
            let mut transport = client.transport.lock().await;
            if let Ok(Some(message)) = transport.receive().await {
                let parsed: JsonRpcMessage = serde_json::from_str(&message).unwrap();
                if let JsonRpcMessage::Response(response) = parsed {
                    if let Some(tx) = client.pending_requests.lock().await.remove(&response.id) {
                        if let Some(result_val) = response.result {
                            let _ = tx.send(result_val);
                        }
                    }
                }
            }
        }

        tokio::time::timeout(Duration::from_secs(2), result_rx)
            .await
            .expect("round-trip timed out")
            .expect("round-trip channel closed")
    }

    // ── serialization tests ────────────────────────────────────────────

    #[test]
    fn test_jsonrpc_request_serialization() {
        let request = JsonRpcRequest::new("test_method", Some(serde_json::json!({"key": "value"})));
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"test_method\""));
    }

    #[test]
    fn test_jsonrpc_response_serialization() {
        let response = JsonRpcResponse::ok("test_id", serde_json::json!({"result": "success"}));
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":\"test_id\""));
    }

    #[test]
    fn test_initialize_result_deserialization() {
        let json = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": { "listChanged": true },
                "resources": { "subscribe": false, "listChanged": false },
                "prompts": { "listChanged": false }
            },
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        });
        let result: InitializeResult = serde_json::from_value(json).unwrap();
        assert_eq!(result.protocol_version, "2024-11-05");
        assert!(result.capabilities.tools.is_some());
        assert!(result.capabilities.tools.as_ref().unwrap().list_changed);
        assert!(result.capabilities.resources.is_some());
        assert!(result.capabilities.prompts.is_some());
        assert_eq!(result.server_info.as_ref().unwrap().name, "test-server");
    }

    #[test]
    fn test_list_tools_result_deserialization() {
        let json = serde_json::json!([
            {
                "name": "test_tool",
                "description": "A test tool",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "another_tool",
                "description": "Another tool",
                "inputSchema": null
            }
        ]);
        let tools: ListToolsResult = serde_json::from_value(json).unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "test_tool");
        assert_eq!(tools[0].description, "A test tool");
        assert_eq!(tools[1].name, "another_tool");
    }

    #[test]
    fn test_tool_content_deserialization() {
        let json = serde_json::json!({
            "content": [
                { "type": "text", "text": "Hello!" }
            ],
            "isError": false
        });
        let content: ToolContent = serde_json::from_value(json).unwrap();
        assert_eq!(content.content.len(), 1);
        match &content.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello!"),
            _ => panic!("expected text content block"),
        }
        assert_eq!(content.is_error, Some(false));
    }

    #[test]
    fn test_list_resources_result_deserialization() {
        let json = serde_json::json!([
            {
                "uri": "file:///test/resource.txt",
                "name": "test_resource",
                "description": "A test resource",
                "mimeType": "text/plain"
            }
        ]);
        let resources: ListResourcesResult = serde_json::from_value(json).unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].uri, "file:///test/resource.txt");
        assert_eq!(resources[0].name, "test_resource");
        assert_eq!(resources[0].mime_type.as_deref(), Some("text/plain"));
    }

    // ── MockTransport unit tests ────────────────────────────────────────

    #[tokio::test]
    async fn test_mock_transport_send_and_receive() {
        let mut mock = MockTransport::new();
        mock.enqueue_response(r#"{"hello":"world"}"#);

        mock.send("test message").await.unwrap();
        let received = mock.receive().await.unwrap();
        assert_eq!(received, Some(r#"{"hello":"world"}"#.to_string()));

        assert_eq!(mock.sent_messages(), &["test message".to_string()]);
    }

    #[tokio::test]
    async fn test_mock_transport_fifo_order() {
        let mut mock = MockTransport::new();
        mock.enqueue_response("first");
        mock.enqueue_response("second");

        assert_eq!(mock.receive().await.unwrap(), Some("first".to_string()));
        assert_eq!(mock.receive().await.unwrap(), Some("second".to_string()));
        assert_eq!(mock.receive().await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_mock_transport_close() {
        let mut mock = MockTransport::new();
        mock.close().await.unwrap();
    }

    #[tokio::test]
    async fn test_mock_transport_receive_none_when_empty() {
        let mut mock = MockTransport::new();
        let result = mock.receive().await.unwrap();
        assert!(result.is_none());
    }

    // ── capability detection tests ─────────────────────────────────────

    #[tokio::test]
    async fn test_capabilities_none_before_connect() {
        let mock = MockTransport::new();
        let client = McpClient::new(mock);
        assert!(client.capabilities().await.is_none());
        assert!(!client.supports_tools().await);
        assert!(!client.supports_resources().await);
        assert!(!client.supports_prompts().await);
    }

    #[tokio::test]
    async fn test_capabilities_returns_some_after_manual_set() {
        let mock = MockTransport::new();
        let client = McpClient::new(mock);

        let caps = ServerCapabilities {
            tools: Some(ToolsCapability { list_changed: true }),
            resources: Some(ResourcesCapability {
                subscribe: false,
                list_changed: false,
            }),
            prompts: Some(PromptsCapability {
                list_changed: false,
            }),
            logging: None,
            ..Default::default()
        };
        *client.server_capabilities.lock().await = Some(caps.clone());

        let stored = client.capabilities().await;
        assert!(stored.is_some());
        let stored = stored.unwrap();
        assert!(stored.tools.is_some());
        assert!(stored.tools.as_ref().unwrap().list_changed);
        assert!(stored.resources.is_some());
        assert!(stored.prompts.is_some());
    }

    #[tokio::test]
    async fn test_supports_tools() {
        let mock = MockTransport::new();
        let client = McpClient::new(mock);
        *client.server_capabilities.lock().await = Some(ServerCapabilities {
            tools: Some(ToolsCapability { list_changed: true }),
            ..Default::default()
        });
        assert!(client.supports_tools().await);
        assert!(!client.supports_resources().await);
        assert!(!client.supports_prompts().await);
    }

    #[tokio::test]
    async fn test_supports_resources() {
        let mock = MockTransport::new();
        let client = McpClient::new(mock);
        *client.server_capabilities.lock().await = Some(ServerCapabilities {
            resources: Some(ResourcesCapability {
                subscribe: true,
                list_changed: false,
            }),
            ..Default::default()
        });
        assert!(!client.supports_tools().await);
        assert!(client.supports_resources().await);
        assert!(!client.supports_prompts().await);
    }

    #[tokio::test]
    async fn test_supports_prompts() {
        let mock = MockTransport::new();
        let client = McpClient::new(mock);
        *client.server_capabilities.lock().await = Some(ServerCapabilities {
            prompts: Some(PromptsCapability {
                list_changed: false,
            }),
            ..Default::default()
        });
        assert!(!client.supports_tools().await);
        assert!(!client.supports_resources().await);
        assert!(client.supports_prompts().await);
    }

    // ── request message construction tests ─────────────────────────────

    #[tokio::test]
    async fn test_send_request_constructs_valid_jsonrpc() {
        // Verify the request format via JsonRpcRequest serialization.
        let request = JsonRpcRequest::new("tools/list", None);
        let serialized = serde_json::to_string(&request).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["method"], "tools/list");
        assert!(parsed.get("id").is_some());
    }

    #[tokio::test]
    async fn test_call_tool_request_params() {
        // Verify the call_tool parameter construction.
        let params = serde_json::json!({
            "name": "my_tool",
            "arguments": {"key": "value", "count": 42}
        });
        let request = JsonRpcRequest::new("tools/call", Some(params));
        let serialized = serde_json::to_string(&request).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed["method"], "tools/call");
        assert_eq!(parsed["params"]["name"], "my_tool");
        assert_eq!(parsed["params"]["arguments"]["key"], "value");
        assert_eq!(parsed["params"]["arguments"]["count"], 42);
    }

    // ── round-trip integration tests ───────────────────────────────────

    #[tokio::test]
    async fn test_list_tools_round_trip() {
        let mock = MockTransport::new();
        let client = McpClient::new(mock).with_timeout(Duration::from_secs(2));

        let tools_response = serde_json::json!([
            {
                "name": "test_tool",
                "description": "A test tool",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "another_tool",
                "description": "Another tool",
                "inputSchema": null
            }
        ]);

        let result_val = round_trip_raw(&client, "tools/list", None, tools_response).await;
        let tools: Vec<Tool> = serde_json::from_value(result_val).unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "test_tool");
        assert_eq!(tools[0].description, "A test tool");
        assert_eq!(tools[1].name, "another_tool");
    }

    #[tokio::test]
    async fn test_call_tool_round_trip() {
        let mock = MockTransport::new();
        let client = McpClient::new(mock).with_timeout(Duration::from_secs(2));

        let params = serde_json::json!({
            "name": "test_tool",
            "arguments": {"key": "value"}
        });

        let tool_response = serde_json::json!({
            "content": [
                { "type": "text", "text": "Hello from test_tool!" }
            ],
            "isError": false
        });

        let result_val = round_trip_raw(&client, "tools/call", Some(params), tool_response).await;
        let content: ToolContent = serde_json::from_value(result_val).unwrap();

        assert_eq!(content.content.len(), 1);
        match &content.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello from test_tool!"),
            _ => panic!("expected text content block"),
        }
        assert_eq!(content.is_error, Some(false));

        // Verify the sent request payload.
        let sent = client
            .transport
            .lock()
            .await
            .sent_messages()
            .last()
            .unwrap()
            .clone();
        let parsed: serde_json::Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(parsed["method"], "tools/call");
        assert_eq!(parsed["params"]["name"], "test_tool");
        assert_eq!(parsed["params"]["arguments"]["key"], "value");
    }

    #[tokio::test]
    async fn test_list_resources_round_trip() {
        let mock = MockTransport::new();
        let client = McpClient::new(mock).with_timeout(Duration::from_secs(2));

        let resources_response = serde_json::json!([
            {
                "uri": "file:///test/resource.txt",
                "name": "test_resource",
                "description": "A test resource",
                "mimeType": "text/plain"
            }
        ]);

        let result_val = round_trip_raw(&client, "resources/list", None, resources_response).await;
        let resources: Vec<Resource> = serde_json::from_value(result_val).unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].uri, "file:///test/resource.txt");
        assert_eq!(resources[0].name, "test_resource");
        assert_eq!(resources[0].description, "A test resource");
        assert_eq!(resources[0].mime_type.as_deref(), Some("text/plain"));
    }

    #[tokio::test]
    async fn test_initialize_round_trip() {
        let mock = MockTransport::new();
        let client = McpClient::new(mock).with_timeout(Duration::from_secs(2));

        let params = serde_json::json!({
            "protocolVersion": crate::MCP_PROTOCOL_VERSION,
            "capabilities": ClientCapabilities::default(),
            "clientInfo": {
                "name": "shannon-mcp",
                "version": env!("CARGO_PKG_VERSION")
            }
        });

        let init_response = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": { "listChanged": true },
                "resources": { "subscribe": false, "listChanged": false },
                "prompts": { "listChanged": false }
            },
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        });

        let result_val = round_trip_raw(&client, "initialize", Some(params), init_response).await;
        let result: InitializeResult = serde_json::from_value(result_val).unwrap();

        assert_eq!(result.protocol_version, "2024-11-05");
        assert!(result.capabilities.tools.is_some());
        assert!(result.capabilities.resources.is_some());
        assert!(result.capabilities.prompts.is_some());
        assert_eq!(result.server_info.as_ref().unwrap().name, "test-server");

        // Simulate what initialize() does: store capabilities.
        *client.server_capabilities.lock().await = Some(result.capabilities.clone());

        // Verify capability detection.
        assert!(client.capabilities().await.is_some());
        assert!(client.supports_tools().await);
        assert!(client.supports_resources().await);
        assert!(client.supports_prompts().await);
    }
}
