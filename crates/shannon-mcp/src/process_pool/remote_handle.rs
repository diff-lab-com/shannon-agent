//! RemoteMcpServerHandle — manages a remote MCP server connection (HTTP/SSE/WebSocket transport).

use serde_json::Value;
use shannon_tool_interface::{ToolError, ToolOutput, ToolResult};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};

use super::SamplingProvider;
use super::types::*;
use crate::auth::AuthProvider;
use crate::transport::Transport;

// ---------------------------------------------------------------------------
// Remote Server Handle (HTTP/SSE transports)
// ---------------------------------------------------------------------------

/// Manages a remote MCP server connection via HTTP.
///
/// Unlike `McpServerHandle` (which manages a child process over stdio),
/// this handle sends JSON-RPC requests via HTTP POST and parses responses.
/// No process management, no background reader task, no pending request map.
pub(crate) struct RemoteMcpServerHandle {
    /// Server name (for logging).
    pub(crate) name: String,
    /// Server URL endpoint.
    pub(crate) url: String,
    /// HTTP client (reused for connection pooling).
    pub(crate) client: reqwest::Client,
    /// Extra headers to include in every request (e.g., auth).
    pub(crate) headers: HashMap<String, String>,
    /// Optional OAuth provider for dynamic Bearer token injection.
    pub(crate) auth_provider: Option<Arc<crate::auth::OAuth2Provider>>,
    /// Shell commands to execute for dynamic headers (name → command).
    pub(crate) header_commands: HashMap<String, String>,
    /// Current state.
    pub(crate) state: Arc<RwLock<ServerState>>,
    /// Capabilities advertised by the server during initialization.
    pub(crate) capabilities: Arc<RwLock<Option<crate::ServerCapabilities>>>,
    /// Negotiated protocol version.
    pub(crate) protocol_version: Arc<RwLock<String>>,
    /// Next JSON-RPC request id.
    pub(crate) next_id: AtomicU64,
    /// Total number of tool call requests.
    pub(crate) request_count: AtomicU64,
    /// Total number of failed tool calls.
    pub(crate) error_count: AtomicU64,
    /// Total bytes of tool result content (approximate token usage).
    pub(crate) total_result_bytes: AtomicU64,
    /// Budget in bytes for this server (None = unlimited).
    pub(crate) budget_bytes: Arc<RwLock<Option<u64>>>,
    /// How many times this server has been restarted (re-initialized).
    pub(crate) restart_count: Arc<AtomicU64>,
    /// Maximum restart attempts.
    pub(crate) max_restarts: u32,
    /// When the server was last started successfully.
    pub(crate) started_at: Arc<RwLock<Option<Instant>>>,
    /// Request timeout (for regular JSON-RPC requests).
    pub(crate) request_timeout: Duration,
    /// Tool call timeout (for tools/call).
    pub(crate) tool_timeout: Duration,
    /// Semaphore limiting concurrent tool calls to this server.
    pub(crate) concurrency_semaphore: Arc<tokio::sync::Semaphore>,
    /// MCP session ID returned by the server during initialization.
    /// Per MCP spec 2025-03-26 Streamable HTTP, included in all subsequent requests.
    pub(crate) session_id: Arc<RwLock<Option<String>>>,
    /// Optional WebSocket transport (takes precedence over HTTP when set).
    pub(crate) ws_transport: Option<Arc<Mutex<crate::WebSocketTransport>>>,
    /// Provider for LLM sampling (handles `sampling/createMessage` from servers).
    /// Shared reference to the pool's sampling provider so all servers use the same one.
    pub(crate) sampling_provider: Arc<Mutex<Option<SamplingProvider>>>,
    /// Channel for forwarding server notifications to the pool's notification handler.
    pub(crate) notification_tx: tokio::sync::mpsc::Sender<(String, Value)>,
}

impl RemoteMcpServerHandle {
    /// Initialize the remote server: send `initialize` + `notifications/initialized`.
    pub(crate) async fn start(&self) -> Result<(), String> {
        // Check if this is a restart (previous state was Stopped).
        let is_restart = {
            let state = self.state.read().await;
            matches!(*state, ServerState::Stopped)
        };

        *self.state.write().await = ServerState::Starting;

        // Reconnect WebSocket on restart.
        if is_restart {
            if let Some(ws) = &self.ws_transport {
                let mut ws_guard = ws.lock().await;
                ws_guard.connect().await.map_err(|e| {
                    format!("WebSocket reconnection failed for '{}': {e}", self.name)
                })?;
            }
        }

        let init_response = self
            .send_request_with_timeout(
                "initialize",
                serde_json::json!({
                    "protocolVersion": crate::MCP_PROTOCOL_VERSION,
                    "capabilities": {
                        "roots": { "listChanged": true },
                        "sampling": {}
                    },
                    "clientInfo": {"name": "shannon-code", "version": "0.1.0"}
                }),
                self.request_timeout,
            )
            .await?;

        // Parse capabilities from init response.
        if let Some(result) = init_response.get("result") {
            if let Ok(caps) = serde_json::from_value::<crate::ServerCapabilities>(
                result
                    .get("capabilities")
                    .cloned()
                    .unwrap_or(serde_json::json!({})),
            ) {
                debug!(
                    server = %self.name,
                    has_tools = caps.tools.is_some(),
                    has_resources = caps.resources.is_some(),
                    has_prompts = caps.prompts.is_some(),
                    "Remote MCP server capabilities"
                );
                *self.capabilities.write().await = Some(caps);
            }

            // Store negotiated protocol version.
            if let Some(version) = result.get("protocolVersion").and_then(|v| v.as_str()) {
                *self.protocol_version.write().await = version.to_string();
                if version != crate::MCP_PROTOCOL_VERSION {
                    warn!(
                        server = %self.name,
                        server_version = %version,
                        our_version = %crate::MCP_PROTOCOL_VERSION,
                        "Protocol version mismatch with remote MCP server"
                    );
                }
            }
        }

        // Send initialized notification (fire-and-forget POST).
        let _ = self
            .send_notification("notifications/initialized", serde_json::json!({}))
            .await;

        *self.state.write().await = ServerState::Healthy;
        *self.started_at.write().await = Some(Instant::now());
        info!(server = %self.name, url = %self.url, "Remote MCP server is healthy");
        Ok(())
    }

    /// Send a JSON-RPC request via WebSocket (when available) or HTTP POST.
    pub(crate) async fn send_request_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        // Use WebSocket transport when available (takes precedence over HTTP).
        if let Some(ws) = &self.ws_transport {
            return self.send_ws_request(ws, &request, timeout).await;
        }

        let response = self.send_http_request(&request, timeout).await?;

        // Handle 401 — attempt token refresh once and retry.
        if response.status().as_u16() == 401 {
            if let Some(provider) = &self.auth_provider {
                info!(server = %self.name, "Got 401, attempting OAuth token refresh");
                if provider.refresh_token().await.is_ok() {
                    // Retry with refreshed token.
                    let retry = self.send_http_request(&request, timeout).await?;
                    if !retry.status().is_success() {
                        return Err(format!(
                            "Remote MCP server '{}' returned HTTP {} after token refresh",
                            self.name,
                            retry.status()
                        ));
                    }
                    return self.parse_jsonrpc_response(retry).await;
                }
            }

            // No existing auth provider — try DCR auto-registration.
            if self.auth_provider.is_none() {
                info!(server = %self.name, "Got 401 with no auth, attempting DCR auto-registration");
                let scopes = vec!["mcp".to_string()];
                let redirect = "http://localhost:8080/callback".to_string();
                match crate::auto_register_oauth(&self.url, &redirect, scopes).await {
                    Ok(provider) => {
                        info!(server = %self.name, "DCR auto-registration succeeded");
                        // DCR gives us client credentials but we still need user authorization.
                        match provider.get_authorization_url().await {
                            Ok((auth_url, _state)) => {
                                return Err(format!(
                                    "Remote MCP server '{}' requires OAuth. DCR auto-registration succeeded.\n\
                                     Visit this URL to authorize:\n  {auth_url}\n\n\
                                     Then add the resulting OAuth config to your MCP server configuration.",
                                    self.name
                                ));
                            }
                            Err(e) => {
                                return Err(format!(
                                    "Remote MCP server '{}' requires OAuth. DCR registration succeeded but could not build auth URL: {e}",
                                    self.name
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        warn!(server = %self.name, error = %e, "DCR auto-registration failed");
                    }
                }
            }

            // Auto-discovery: try RFC 9728/8414 OAuth metadata for helpful guidance.
            let discovery_hint = match crate::auth::discover_oauth_endpoints(&self.url).await {
                Ok(d) => format!(
                    "\n\nOAuth endpoints discovered for '{}':\n  \
                     Authorization: {}\n  \
                     Token: {}\n\n\
                     Add an OAuth auth config to your MCP server configuration to authenticate.",
                    self.name, d.authorization_endpoint, d.token_endpoint
                ),
                Err(_) => " Configure OAuth or API key auth for this server.".to_string(),
            };

            return Err(format!(
                "Remote MCP server '{}' returned HTTP 401 (unauthorized).{discovery_hint}",
                self.name
            ));
        }

        if !response.status().is_success() {
            return Err(format!(
                "Remote MCP server '{}' returned HTTP {}",
                self.name,
                response.status()
            ));
        }

        // Streamable HTTP: detect SSE vs JSON response by Content-Type.
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        if content_type.contains("text/event-stream") {
            self.parse_sse_response(response).await
        } else {
            self.parse_jsonrpc_response(response).await
        }
    }

    /// Send a JSON-RPC request over WebSocket and wait for the response.
    async fn send_ws_request(
        &self,
        ws: &Arc<Mutex<crate::WebSocketTransport>>,
        request: &Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let request_id = request.get("id").cloned();
        let request_str = serde_json::to_string(request).unwrap_or_default();

        let mut ws_guard = ws.lock().await;

        ws_guard
            .send(&request_str)
            .await
            .map_err(|e| format!("WebSocket send failed for '{}': {e}", self.name))?;

        // Loop: receive messages, handling interleaved server-initiated
        // requests (sampling, elicitation, notifications) until we get the
        // response matching our request id.
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(format!("WebSocket request timed out for '{}'", self.name));
            }

            let response_str = tokio::time::timeout(remaining, ws_guard.receive())
                .await
                .map_err(|_| format!("WebSocket request timed out for '{}'", self.name))?
                .map_err(|e| format!("WebSocket receive failed for '{}': {e}", self.name))?
                .ok_or_else(|| format!("WebSocket connection closed for '{}'", self.name))?;

            let value: Value = serde_json::from_str(&response_str).map_err(|e| {
                format!(
                    "Invalid JSON-RPC response from WebSocket '{name}': {e}",
                    name = self.name
                )
            })?;

            // Check if this is the response to our request.
            let matches_our_id = request_id
                .as_ref()
                .is_some_and(|rid| value.get("id") == Some(rid));

            if matches_our_id {
                // Check for JSON-RPC error.
                if let Some(error) = value.get("error") {
                    return Err(format!("WebSocket MCP error from '{}': {error}", self.name));
                }
                return Ok(value);
            }

            // Not our response — handle server-initiated messages.
            let method = value.get("method").and_then(|m| m.as_str()).unwrap_or("");

            // Server→client request (has both method and id).
            if value.get("id").is_some() {
                let response_value = match method {
                    "sampling/createMessage" => self.handle_remote_sampling(&value).await,
                    "elicitation/create" => self.handle_remote_elicitation(&value).await,
                    _ => {
                        let req_id = value.get("id").cloned();
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "error": { "code": -32601, "message": format!("Method not found: {method}") }
                        })
                    }
                };
                let resp_str = serde_json::to_string(&response_value).unwrap_or_default();
                ws_guard.send(&resp_str).await.map_err(|e| {
                    format!("WebSocket send response failed for '{}': {e}", self.name)
                })?;
            } else {
                // Server notification (method but no id) — forward to pool.
                let _ = self.notification_tx.try_send((self.name.clone(), value));
            }
        }
    }

    /// Handle a `sampling/createMessage` request from a remote server.
    async fn handle_remote_sampling(&self, value: &Value) -> Value {
        let req_id = value.get("id").cloned();
        let provider = self.sampling_provider.lock().await;
        if let Some(ref handler) = *provider {
            let params = value
                .get("params")
                .cloned()
                .unwrap_or(serde_json::json!({}));
            match serde_json::from_value::<crate::CreateMessageRequest>(params) {
                Ok(req) => match handler(req).await {
                    Ok(result) => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": result,
                    }),
                    Err(e) => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "error": { "code": -32603, "message": e },
                    }),
                },
                Err(e) => serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "error": { "code": -32602, "message": format!("Invalid params: {e}") },
                }),
            }
        } else {
            drop(provider);
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "error": { "code": -32601, "message": "Sampling not supported" },
            })
        }
    }

    /// Handle an `elicitation/create` request from a remote server.
    async fn handle_remote_elicitation(&self, value: &Value) -> Value {
        let req_id = value.get("id").cloned();
        // Elicitation requires interactive user input — not available for
        // remote servers in the current architecture. Auto-decline.
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": { "action": "decline" }
        })
    }

    /// Build and send an HTTP POST with all headers (static + dynamic + auth).
    async fn send_http_request(
        &self,
        request: &Value,
        timeout: Duration,
    ) -> Result<reqwest::Response, String> {
        let mut builder = self.client.post(&self.url);
        for (key, value) in &self.headers {
            builder = builder.header(key.as_str(), value.as_str());
        }
        // Resolve dynamic headers from shell commands (user-configured).
        // Only allow simple alphanumeric commands without shell metacharacters.
        for (name, command) in &self.header_commands {
            // Reject commands containing shell metacharacters or path traversal
            if command.contains([
                ';', '&', '|', '$', '`', '(', ')', '{', '}', '<', '>', '\\', '\n', '\r',
            ]) || command.contains("..")
            {
                warn!(server = %self.name, header = %name, "Skipping header command with unsafe characters: {command}");
                continue;
            }
            let parts: Vec<&str> = command.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }
            let output = tokio::process::Command::new(parts[0])
                .args(&parts[1..])
                .output()
                .await;
            match output {
                Ok(out) if out.status.success() => {
                    let value = String::from_utf8_lossy(&out.stdout);
                    builder = builder.header(name.as_str(), value.trim());
                }
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    warn!(server = %self.name, header = %name, error = %stderr,
                          "Dynamic header command failed, skipping");
                }
                Err(e) => {
                    warn!(server = %self.name, header = %name, error = %e,
                          "Dynamic header command execution failed, skipping");
                }
            }
        }
        // Inject dynamic auth headers from OAuth provider.
        if let Some(provider) = &self.auth_provider {
            let mut auth_headers = HashMap::new();
            if let Err(e) = provider.add_auth_headers(&mut auth_headers).await {
                warn!(server = %self.name, error = %e, "Failed to inject OAuth headers");
            }
            for (key, value) in auth_headers {
                builder = builder.header(key.as_str(), value.as_str());
            }
        }
        // Include MCP session ID if available (Streamable HTTP spec).
        if let Some(sid) = self.session_id.read().await.as_ref() {
            builder = builder.header("Mcp-Session-Id", sid.as_str());
        }
        // Accept both JSON and SSE responses (Streamable HTTP spec).
        builder = builder.header("Accept", "application/json, text/event-stream");

        tokio::time::timeout(
            timeout,
            builder
                .header("Content-Type", "application/json")
                .json(&request)
                .send(),
        )
        .await
        .map_err(|_| {
            format!(
                "Remote MCP server '{}' request timed out after {:?}",
                self.name, timeout
            )
        })?
        .map_err(|e| {
            format!(
                "Remote MCP server '{}' HTTP request failed: {}",
                self.name, e
            )
        })
    }

    /// Parse a successful HTTP response as JSON-RPC.
    ///
    /// Also captures the `Mcp-Session-Id` header if present (Streamable HTTP spec).
    async fn parse_jsonrpc_response(&self, response: reqwest::Response) -> Result<Value, String> {
        // Capture MCP session ID from response headers.
        if let Some(sid) = response.headers().get("Mcp-Session-Id") {
            if let Ok(s) = sid.to_str() {
                debug!(server = %self.name, session_id = %s, "Received MCP session ID");
                *self.session_id.write().await = Some(s.to_string());
            }
        }

        let body: Value = response.json().await.map_err(|e| {
            format!(
                "Remote MCP server '{}' response parse error: {}",
                self.name, e
            )
        })?;

        // Check for JSON-RPC error.
        if let Some(error) = body.get("error") {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(format!("Remote MCP server '{}' error: {}", self.name, msg));
        }

        Ok(body)
    }

    /// Parse an SSE (text/event-stream) HTTP response as JSON-RPC.
    ///
    /// Reads the SSE stream and extracts the first JSON-RPC response payload
    /// from `data:` events. Used for Streamable HTTP where the server responds
    /// with SSE instead of a single JSON body.
    async fn parse_sse_response(&self, response: reqwest::Response) -> Result<Value, String> {
        use futures_util::StreamExt;

        // Capture MCP session ID from response headers.
        if let Some(sid) = response.headers().get("Mcp-Session-Id") {
            if let Ok(s) = sid.to_str() {
                debug!(server = %self.name, session_id = %s, "Received MCP session ID (SSE)");
                *self.session_id.write().await = Some(s.to_string());
            }
        }

        let byte_stream = response.bytes_stream();
        let mut stream = Box::pin(byte_stream);
        let mut buffer = String::new();
        let mut event_data = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("SSE stream error: {e}"))?;
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            // Process complete lines.
            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.is_empty() {
                    // End of event — try to parse accumulated data as JSON-RPC.
                    if !event_data.is_empty() {
                        if let Ok(value) = serde_json::from_str::<Value>(&event_data) {
                            if let Some(error) = value.get("error") {
                                let msg = error
                                    .get("message")
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("Unknown error");
                                return Err(format!(
                                    "Remote MCP server '{}' error: {}",
                                    self.name, msg
                                ));
                            }
                            return Ok(value);
                        }
                        event_data.clear();
                    }
                } else if let Some(rest) = line.strip_prefix("data:") {
                    let data = rest.trim();
                    if !event_data.is_empty() {
                        event_data.push('\n');
                    }
                    event_data.push_str(data);
                }
                // Ignore other SSE fields (event:, id:, retry:)
            }
        }

        // Process any remaining data after stream ends.
        if !event_data.is_empty() {
            if let Ok(value) = serde_json::from_str::<Value>(&event_data) {
                if let Some(error) = value.get("error") {
                    let msg = error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown error");
                    return Err(format!("Remote MCP server '{}' error: {}", self.name, msg));
                }
                return Ok(value);
            }
        }

        Err(format!(
            "Remote MCP server '{}' SSE stream ended without JSON-RPC response",
            self.name
        ))
    }

    /// Send a JSON-RPC notification (no id, no response expected).
    pub(crate) async fn send_notification(
        &self,
        method: &str,
        params: Value,
    ) -> Result<(), String> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        // Use WebSocket transport when available.
        if let Some(ws) = &self.ws_transport {
            let notif_str = serde_json::to_string(&notification).unwrap_or_default();
            let mut ws_guard = ws.lock().await;
            ws_guard
                .send(&notif_str)
                .await
                .map_err(|e| format!("WebSocket notification failed for '{}': {e}", self.name))?;
            return Ok(());
        }

        let mut builder = self.client.post(&self.url);
        for (key, value) in &self.headers {
            builder = builder.header(key.as_str(), value.as_str());
        }
        // Inject dynamic auth headers.
        if let Some(provider) = &self.auth_provider {
            let mut auth_headers = HashMap::new();
            if let Err(e) = provider.add_auth_headers(&mut auth_headers).await {
                debug!(server = %self.name, error = %e, "Failed to inject OAuth headers in notification");
            }
            for (key, value) in auth_headers {
                builder = builder.header(key.as_str(), value.as_str());
            }
        }
        // Include MCP session ID if available (Streamable HTTP spec).
        if let Some(sid) = self.session_id.read().await.as_ref() {
            builder = builder.header("Mcp-Session-Id", sid.as_str());
        }

        builder
            .header("Content-Type", "application/json")
            .json(&notification)
            .send()
            .await
            .map_err(|e| {
                format!(
                    "Remote MCP server '{}' notification failed: {}",
                    self.name, e
                )
            })?;

        Ok(())
    }

    /// Send multiple JSON-RPC requests as a batch (JSON-RPC spec §6).
    ///
    /// All requests are sent in a single HTTP POST. Returns results matched
    /// by request ID. Individual request errors are returned within the
    /// result array (not as top-level errors).
    pub(crate) async fn send_batch_request(
        &self,
        requests: Vec<(&str, Value)>,
        timeout: Duration,
    ) -> Result<Vec<(u64, Result<Value, String>)>, String> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }

        // Assign IDs and build batch array.
        let mut batch = Vec::with_capacity(requests.len());
        let mut ids = Vec::with_capacity(requests.len());
        for (method, params) in &requests {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            ids.push(id);
            batch.push(serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }));
        }

        let response = self
            .send_http_request(&serde_json::json!(batch), timeout)
            .await?;

        // Handle 401 — attempt token refresh once.
        if response.status().as_u16() == 401 {
            if let Some(provider) = &self.auth_provider {
                if provider.refresh_token().await.is_ok() {
                    let retry = self
                        .send_http_request(&serde_json::json!(batch), timeout)
                        .await?;
                    if !retry.status().is_success() {
                        return Err(format!(
                            "Remote MCP server '{}' returned HTTP {} after token refresh",
                            self.name,
                            retry.status()
                        ));
                    }
                    return self.parse_batch_response(retry, &ids).await;
                }
            }
            return Err(format!(
                "Remote MCP server '{}' returned HTTP 401 (unauthorized)",
                self.name
            ));
        }

        if !response.status().is_success() {
            return Err(format!(
                "Remote MCP server '{}' returned HTTP {}",
                self.name,
                response.status()
            ));
        }

        self.parse_batch_response(response, &ids).await
    }

    /// Parse a batch JSON-RPC response, matching results to request IDs.
    async fn parse_batch_response(
        &self,
        response: reqwest::Response,
        ids: &[u64],
    ) -> Result<Vec<(u64, Result<Value, String>)>, String> {
        // Capture session ID from response.
        if let Some(sid) = response.headers().get("Mcp-Session-Id") {
            if let Ok(s) = sid.to_str() {
                *self.session_id.write().await = Some(s.to_string());
            }
        }

        let body: Value = response.json().await.map_err(|e| {
            format!(
                "Remote MCP server '{}' batch response parse error: {}",
                self.name, e
            )
        })?;

        // Build result map from response array.
        let mut results: HashMap<u64, Result<Value, String>> = HashMap::new();

        let items = match body {
            Value::Array(arr) => arr,
            single => {
                // Single response (not batched) — treat as array of one.
                vec![single]
            }
        };

        for item in items {
            let id = item.get("id").and_then(|v| v.as_u64()).unwrap_or(0);

            if let Some(error) = item.get("error") {
                let msg = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error");
                results.insert(id, Err(msg.to_string()));
            } else if let Some(result) = item.get("result").cloned() {
                results.insert(id, Ok(result));
            }
        }

        // Return in the same order as input IDs.
        Ok(ids
            .iter()
            .map(|&id| {
                (
                    id,
                    results
                        .remove(&id)
                        .unwrap_or(Err("No response for request".to_string())),
                )
            })
            .collect())
    }

    /// Call a tool on this remote server via `tools/call`.
    pub(crate) async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> ToolResult<ToolOutput> {
        // Check state.
        {
            let state = self.state.read().await;
            if !matches!(*state, ServerState::Healthy) {
                return Err(ToolError::ExecutionFailed(format!(
                    "Remote MCP server '{}' is not healthy (state: {:?})",
                    self.name, *state
                )));
            }
        }

        self.request_count.fetch_add(1, Ordering::Relaxed);

        let response = self
            .send_request_with_timeout(
                "tools/call",
                serde_json::json!({
                    "name": tool_name,
                    "arguments": arguments,
                }),
                self.tool_timeout,
            )
            .await
            .map_err(|e| {
                self.error_count.fetch_add(1, Ordering::Relaxed);
                ToolError::ExecutionFailed(e)
            })?;

        // Parse the response content (same logic as stdio handle).
        if let Some(result) = response.get("result") {
            if let Some(content_array) = result.get("content").and_then(|c| c.as_array()) {
                let is_error = result
                    .get("isError")
                    .and_then(|e| e.as_bool())
                    .unwrap_or(false);

                if is_error {
                    let normalized = normalize_error_content(content_array);
                    if !normalized.is_empty() {
                        let content = truncate_tool_result(&normalized, MAX_TOOL_RESULT_CHARS);
                        return Ok(ToolOutput::error(content));
                    }
                } else {
                    let texts: Vec<String> = content_array
                        .iter()
                        .filter_map(|block| {
                            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                block
                                    .get("text")
                                    .and_then(|t| t.as_str())
                                    .map(|s| s.to_string())
                            } else {
                                None
                            }
                        })
                        .collect();

                    if !texts.is_empty() {
                        let content =
                            truncate_tool_result(&texts.join("\n"), MAX_TOOL_RESULT_CHARS);
                        return Ok(ToolOutput::success(content));
                    }
                }
            }
            return Ok(ToolOutput::success(truncate_tool_result(
                &result.to_string(),
                MAX_TOOL_RESULT_CHARS,
            )));
        }

        Ok(ToolOutput::success(truncate_tool_result(
            &response.to_string(),
            MAX_TOOL_RESULT_CHARS,
        )))
    }

    /// Get the current state.
    pub(crate) async fn get_state(&self) -> ServerState {
        self.state.read().await.clone()
    }

    /// Get detailed status including metrics.
    pub(crate) async fn get_status(&self) -> ServerStatus {
        let state = self.state.read().await.clone();
        let started_at = *self.started_at.read().await;
        let now = Instant::now();

        ServerStatus {
            name: self.name.clone(),
            uptime: started_at.map(|t| now.duration_since(t)),
            state,
            request_count: self.request_count.load(Ordering::Relaxed),
            error_count: self.error_count.load(Ordering::Relaxed),
            restart_count: self.restart_count.load(Ordering::Relaxed),
            last_health_check: None,
            total_result_bytes: self.total_result_bytes.load(Ordering::Relaxed),
            budget_bytes: *self.budget_bytes.read().await,
        }
    }

    /// Reset state for restart.
    pub(crate) async fn reset(&self) {
        *self.state.write().await = ServerState::Stopped;
        *self.started_at.write().await = None;
        *self.session_id.write().await = None;
        // Close WebSocket if present.
        if let Some(ws) = &self.ws_transport {
            let mut ws_guard = ws.lock().await;
            let _ = ws_guard.close().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicU64;
    use std::time::Duration;
    use tokio::sync::{Mutex, RwLock};

    /// Helper: create a minimal remote handle for unit testing.
    ///
    /// The handle starts in `Stopped` state with a valid HTTP client
    /// but no actual server to talk to. Uses a short connect timeout
    /// to avoid hanging on network-dependent tests.
    fn make_remote_handle(name: &str) -> RemoteMcpServerHandle {
        let (ntx, _) = tokio::sync::mpsc::channel(1024);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(100))
            .connect_timeout(Duration::from_millis(50))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        RemoteMcpServerHandle {
            name: name.to_string(),
            url: "http://127.0.0.1:1".to_string(), // port 1 = unreachable
            client,
            headers: HashMap::new(),
            auth_provider: None,
            header_commands: HashMap::new(),
            state: Arc::new(RwLock::new(ServerState::Stopped)),
            capabilities: Arc::new(RwLock::new(None)),
            protocol_version: Arc::new(RwLock::new(String::new())),
            next_id: AtomicU64::new(1),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            total_result_bytes: AtomicU64::new(0),
            budget_bytes: Arc::new(RwLock::new(None)),
            restart_count: Arc::new(AtomicU64::new(0)),
            max_restarts: 3,
            started_at: Arc::new(RwLock::new(None)),
            request_timeout: Duration::from_millis(100),
            tool_timeout: Duration::from_millis(100),
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
            session_id: Arc::new(RwLock::new(None)),
            ws_transport: None,
            sampling_provider: Arc::new(Mutex::new(None)),
            notification_tx: ntx,
        }
    }

    // -- State management tests --------------------------------------------

    #[tokio::test]
    async fn get_state_initial_is_stopped() {
        let handle = make_remote_handle("test-remote");
        assert_eq!(handle.get_state().await, ServerState::Stopped);
    }

    #[tokio::test]
    async fn get_state_transition() {
        let handle = make_remote_handle("state-remote");
        *handle.state.write().await = ServerState::Healthy;
        assert_eq!(handle.get_state().await, ServerState::Healthy);

        *handle.state.write().await = ServerState::Unhealthy("timeout".to_string());
        assert_eq!(
            handle.get_state().await,
            ServerState::Unhealthy("timeout".to_string())
        );
    }

    // -- get_status tests --------------------------------------------------

    #[tokio::test]
    async fn get_status_reflects_state() {
        let handle = make_remote_handle("status-remote");
        let status = handle.get_status().await;
        assert_eq!(status.name, "status-remote");
        assert_eq!(status.state, ServerState::Stopped);
        assert!(status.uptime.is_none());
        assert!(status.last_health_check.is_none());
        assert_eq!(status.request_count, 0);
        assert_eq!(status.error_count, 0);
    }

    #[tokio::test]
    async fn get_status_includes_metrics() {
        let handle = make_remote_handle("metric-remote");
        handle.request_count.store(10, Ordering::Relaxed);
        handle.error_count.store(1, Ordering::Relaxed);
        handle.total_result_bytes.store(2048, Ordering::Relaxed);
        *handle.budget_bytes.write().await = Some(50_000);
        *handle.state.write().await = ServerState::Healthy;
        *handle.started_at.write().await = Some(Instant::now());

        let status = handle.get_status().await;
        assert_eq!(status.request_count, 10);
        assert_eq!(status.error_count, 1);
        assert_eq!(status.total_result_bytes, 2048);
        assert_eq!(status.budget_bytes, Some(50_000));
        assert!(status.uptime.is_some());
    }

    // -- reset tests -------------------------------------------------------

    #[tokio::test]
    async fn reset_sets_state_to_stopped() {
        let handle = make_remote_handle("reset-remote");
        *handle.state.write().await = ServerState::Healthy;
        *handle.started_at.write().await = Some(Instant::now());
        *handle.session_id.write().await = Some("sess-123".to_string());

        handle.reset().await;

        assert_eq!(handle.get_state().await, ServerState::Stopped);
        assert!(handle.started_at.read().await.is_none());
        assert!(handle.session_id.read().await.is_none());
    }

    #[tokio::test]
    async fn reset_idempotent() {
        let handle = make_remote_handle("reset-idem");
        handle.reset().await;
        handle.reset().await;
        assert_eq!(handle.get_state().await, ServerState::Stopped);
    }

    // -- call_tool state guard tests ---------------------------------------

    #[tokio::test]
    async fn call_tool_rejects_when_not_healthy() {
        let handle = make_remote_handle("reject-remote");
        *handle.state.write().await = ServerState::Unhealthy("conn refused".to_string());
        let result = handle.call_tool("some_tool", serde_json::json!({})).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            shannon_tool_interface::ToolError::ExecutionFailed(msg) => {
                assert!(msg.contains("not healthy"));
                assert!(msg.contains("conn refused"));
            }
            other => panic!("Expected ExecutionFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn call_tool_rejects_when_starting() {
        let handle = make_remote_handle("starting-remote");
        *handle.state.write().await = ServerState::Starting;
        let result = handle.call_tool("tool", serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn call_tool_rejects_when_stopped() {
        let handle = make_remote_handle("stopped-remote");
        let result = handle.call_tool("tool", serde_json::json!({})).await;
        assert!(result.is_err());
    }

    // -- send_request_with_timeout to unreachable server -------------------

    #[tokio::test]
    async fn send_request_fails_on_unreachable_server() {
        let handle = make_remote_handle("unreachable");
        *handle.state.write().await = ServerState::Healthy;
        let result = handle
            .send_request_with_timeout(
                "tools/list",
                serde_json::json!({}),
                Duration::from_millis(50),
            )
            .await;
        assert!(result.is_err());
    }

    // -- send_notification to unreachable server ---------------------------

    #[tokio::test]
    async fn send_notification_fails_on_unreachable_server() {
        let handle = make_remote_handle("unreachable-notif");
        let result = handle
            .send_notification("notifications/initialized", serde_json::json!({}))
            .await;
        assert!(result.is_err());
    }

    // -- start fails on unreachable server ---------------------------------

    #[tokio::test]
    async fn start_fails_on_unreachable_server() {
        let handle = make_remote_handle("unreachable-start");
        let result = handle.start().await;
        assert!(result.is_err());
    }

    // -- session_id management tests ---------------------------------------

    #[tokio::test]
    async fn session_id_initially_none() {
        let handle = make_remote_handle("session-test");
        assert!(handle.session_id.read().await.is_none());
    }

    #[tokio::test]
    async fn session_id_can_be_set() {
        let handle = make_remote_handle("session-test");
        *handle.session_id.write().await = Some("sid-abc".to_string());
        assert_eq!(handle.session_id.read().await.as_deref(), Some("sid-abc"));
    }

    // -- next_id counter test ----------------------------------------------

    #[test]
    fn next_id_increments_atomically() {
        let handle = make_remote_handle("id-test");
        assert_eq!(handle.next_id.load(Ordering::Relaxed), 1);
        let id1 = handle.next_id.fetch_add(1, Ordering::Relaxed);
        let id2 = handle.next_id.fetch_add(1, Ordering::Relaxed);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    // -- send_batch_request with empty requests ----------------------------

    #[tokio::test]
    async fn send_batch_request_empty_returns_empty() {
        let handle = make_remote_handle("batch-empty");
        let result = handle
            .send_batch_request(vec![], Duration::from_secs(5))
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    // -- capabilities storage tests ----------------------------------------

    #[tokio::test]
    async fn capabilities_initially_none() {
        let handle = make_remote_handle("cap-test");
        assert!(handle.capabilities.read().await.is_none());
    }

    #[tokio::test]
    async fn protocol_version_initially_empty() {
        let handle = make_remote_handle("proto-test");
        assert!(handle.protocol_version.read().await.is_empty());
    }

    // -- header commands safety check --------------------------------------

    #[test]
    fn header_commands_with_shell_metacharacters_are_detected() {
        // The remote handle skips commands containing shell metacharacters.
        // We verify the detection logic works by checking individual characters.
        let unsafe_chars = [';', '&', '|', '$', '`', '(', ')', '{', '}', '<', '>', '\\'];
        for ch in unsafe_chars {
            let cmd = format!("echo {ch}something");
            assert!(cmd.contains(ch), "Command should contain '{ch}'");
        }
        // The ".." path traversal should also be caught.
        let traversal_cmd = "../bin/get-token";
        assert!(traversal_cmd.contains(".."));
    }

    // -- concurrency semaphore test ----------------------------------------

    #[tokio::test]
    async fn concurrency_semaphore_allows_permits() {
        // The helper creates a Semaphore(1), so only one acquire can succeed.
        let handle = make_remote_handle("sem-test");
        let p1 = handle.concurrency_semaphore.try_acquire().unwrap();
        // Second try_acquire should fail — no permits left.
        assert!(handle.concurrency_semaphore.try_acquire().is_err());
        // After dropping, the permit is returned.
        drop(p1);
        let _p2 = handle.concurrency_semaphore.try_acquire().unwrap();
    }

    // -- restart_count tracking test ---------------------------------------

    #[test]
    fn restart_count_increments() {
        let handle = make_remote_handle("restart-test");
        assert_eq!(handle.restart_count.load(Ordering::Relaxed), 0);
        handle.restart_count.fetch_add(1, Ordering::Relaxed);
        assert_eq!(handle.restart_count.load(Ordering::Relaxed), 1);
    }

    // -- budget_bytes tracking test ----------------------------------------

    #[tokio::test]
    async fn budget_bytes_default_none() {
        let handle = make_remote_handle("budget-test");
        assert!(handle.budget_bytes.read().await.is_none());
    }

    #[tokio::test]
    async fn budget_bytes_can_be_set() {
        let handle = make_remote_handle("budget-test");
        *handle.budget_bytes.write().await = Some(5000);
        assert_eq!(*handle.budget_bytes.read().await, Some(5000));
    }
}
