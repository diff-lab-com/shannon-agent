// MCP client implementation
//
// This module provides the main MCP client for connecting to MCP servers,
// sending requests, and handling responses and notifications.

use crate::protocol::*;
use crate::transport::Transport;
use crate::{McpError, McpResult};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// MCP client for communicating with MCP servers
pub struct McpClient<T: Transport> {
    transport: Arc<Mutex<T>>,
    request_timeout: std::time::Duration,
    pending_requests: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<serde_json::Value>>>>,
    server_capabilities: Arc<Mutex<Option<ServerCapabilities>>>,
}

impl<T: Transport> McpClient<T> {
    /// Create a new MCP client with the given transport
    pub fn new(transport: T) -> Self {
        Self {
            transport: Arc::new(Mutex::new(transport)),
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

        // Start the message receiver task
        let transport_clone = self.transport.clone();
        let pending_clone = self.pending_requests.clone();
        let server_caps_clone = self.server_capabilities.clone();

        tokio::spawn(async move {
            Self::receive_loop(transport_clone, pending_clone, server_caps_clone).await
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

        let response = self
            .send_request("initialize", Some(params))
            .await?;

        let result: InitializeResult = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid initialize result: {}", e)))?;

        // Store server capabilities
        *self.server_capabilities.lock().await = Some(result.capabilities.clone());

        info!(
            "Initialized with server: {} v{}",
            result.server_info.as_ref().map(|s| &s.name).unwrap_or(&"unknown".to_string()),
            result.server_info.as_ref().map(|s| &s.version).unwrap_or(&"unknown".to_string())
        );

        Ok(result)
    }

    /// List available tools
    pub async fn list_tools(&self) -> McpResult<ListToolsResult> {
        debug!("Listing tools");
        let response = self.send_request("tools/list", None).await?;
        let tools: ListToolsResult = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid tools list result: {}", e)))?;
        Ok(tools)
    }

    /// Call a tool
    pub async fn call_tool(&self, name: &str, arguments: serde_json::Value) -> McpResult<ToolContent> {
        debug!(tool = %name, "Calling tool");

        let params = serde_json::json!({
            "name": name,
            "arguments": arguments
        });

        let response = self.send_request("tools/call", Some(params)).await?;
        let content: ToolContent = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid tool call result: {}", e)))?;

        Ok(content)
    }

    /// List available resources
    pub async fn list_resources(&self) -> McpResult<ListResourcesResult> {
        debug!("Listing resources");
        let response = self.send_request("resources/list", None).await?;
        let resources: ListResourcesResult = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid resources list result: {}", e)))?;
        Ok(resources)
    }

    /// Read a resource
    pub async fn read_resource(&self, uri: &str) -> McpResult<ResourceContent> {
        debug!(uri = %uri, "Reading resource");

        let params = serde_json::json!({ "uri": uri });
        let response = self.send_request("resources/read", Some(params)).await?;
        let content: ResourceContent = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid resource read result: {}", e)))?;

        Ok(content)
    }

    /// List resource templates
    pub async fn list_resource_templates(&self) -> McpResult<ListResourceTemplatesResult> {
        debug!("Listing resource templates");
        let response = self.send_request("resources/templates/list", None).await?;
        let templates: ListResourceTemplatesResult = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid resource templates result: {}", e)))?;
        Ok(templates)
    }

    /// List available prompts
    pub async fn list_prompts(&self) -> McpResult<ListPromptsResult> {
        debug!("Listing prompts");
        let response = self.send_request("prompts/list", None).await?;
        let prompts: ListPromptsResult = serde_json::from_value(response)
            .map_err(|e| McpError::Protocol(format!("Invalid prompts list result: {}", e)))?;
        Ok(prompts)
    }

    /// Get a prompt
    pub async fn get_prompt(&self, name: &str, arguments: Option<HashMap<String, String>>) -> McpResult<ToolContent> {
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
            .map_err(|e| McpError::Protocol(format!("Invalid prompt get result: {}", e)))?;

        Ok(content)
    }

    /// Send a raw JSON-RPC request
    async fn send_request(&self, method: &str, params: Option<serde_json::Value>) -> McpResult<serde_json::Value> {
        let id = uuid::Uuid::new_v4().to_string();
        let request = JsonRpcRequest::new(method, params);

        debug!(id = %id, method = %method, "Sending request");

        // Create a channel for the response
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_requests.lock().await.insert(id.clone(), tx);

        // Send the request
        let message = JsonRpcMessage::Request(request);
        let serialized = serde_json::to_string(&message)?;
        {
            let mut transport = self.transport.lock().await;
            transport.send(&serialized).await?;
        }

        // Wait for the response
        let response = tokio::time::timeout(self.request_timeout, rx)
            .await
            .map_err(|_| McpError::Timeout(self.request_timeout))?
            .map_err(|_| McpError::Protocol("Response channel closed".to_string()))?;

        Ok(response)
    }

    /// Receive loop for handling incoming messages
    async fn receive_loop(
        transport: Arc<Mutex<T>>,
        pending: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<serde_json::Value>>>>,
        server_capabilities: Arc<Mutex<Option<ServerCapabilities>>>,
    ) {
        loop {
            let message = {
                let mut transport = transport.lock().await;
                match transport.receive().await {
                    Ok(Some(msg)) => msg,
                    Ok(None) => {
                        debug!("Transport closed gracefully");
                        break;
                    }
                    Err(e) => {
                        error!("Receive error: {}", e);
                        break;
                    }
                }
            };

            // Parse the JSON-RPC message
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
                            let _ = tx.send(serde_json::json!({"error": error}));
                        } else if let Some(result) = response.result {
                            let _ = tx.send(result);
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

    /// Handle incoming notifications
    async fn handle_notification(
        notification: JsonRpcNotification,
        server_capabilities: &Arc<Mutex<Option<ServerCapabilities>>>,
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
}
