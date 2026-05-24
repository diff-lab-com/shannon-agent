//! McpServerHandle — manages a single persistent MCP server process (stdio transport).

use dashmap::DashMap;
use serde_json::Value;
use shannon_tool_interface::{ToolError, ToolOutput, ToolResult};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, RwLock, oneshot};
use tracing::{debug, info, warn};

use super::ElicitationProvider;
use super::SamplingProvider;
use super::types::*;

/// Manages a single persistent MCP server process.
pub(crate) struct McpServerHandle {
    /// Server name (for logging).
    pub(crate) name: String,
    /// Command to spawn the process.
    pub(crate) command: String,
    /// Command arguments.
    pub(crate) args: Vec<String>,
    /// Environment variables for the process.
    pub(crate) env: HashMap<String, String>,
    /// stdin writer — locked during writes to serialize requests.
    pub(crate) stdin: Arc<Mutex<Option<ChildStdin>>>,
    /// Next JSON-RPC request id.
    pub(crate) next_id: AtomicU64,
    /// Pending requests keyed by JSON-RPC id.
    pub(crate) pending: Arc<DashMap<u64, PendingRequest>>,
    /// Current state.
    pub(crate) state: Arc<RwLock<ServerState>>,
    /// Child process handle (for kill on drop).
    pub(crate) child: Arc<Mutex<Option<Child>>>,
    /// Background stdout reader task.
    pub(crate) reader_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// How many times this server has been restarted.
    pub(crate) restart_count: Arc<AtomicU64>,
    /// Maximum restart attempts before giving up.
    pub(crate) max_restarts: u32,
    /// Health check interval.
    #[allow(dead_code)]
    pub(crate) health_interval: Duration,
    /// Request timeout (for regular JSON-RPC requests like tools/list, ping).
    pub(crate) request_timeout: Duration,
    /// Connection timeout (for initialize handshake during startup).
    pub(crate) connection_timeout: Duration,
    /// Tool call timeout (for tools/call which can be long-running).
    pub(crate) tool_timeout: Duration,
    /// When the server was last started successfully.
    pub(crate) started_at: Arc<RwLock<Option<Instant>>>,
    /// Total number of tool call requests.
    pub(crate) request_count: AtomicU64,
    /// Total number of failed tool calls.
    pub(crate) error_count: AtomicU64,
    /// Total bytes of tool result content (approximate token usage).
    pub(crate) total_result_bytes: AtomicU64,
    /// Budget in bytes for this server (None = unlimited).
    pub(crate) budget_bytes: Arc<RwLock<Option<u64>>>,
    /// When the last successful health check occurred.
    pub(crate) last_health_check: Arc<RwLock<Option<Instant>>>,
    /// Channel to forward server notifications to the pool.
    pub(crate) notification_tx: tokio::sync::mpsc::Sender<(String, Value)>,
    /// Provider for filesystem roots (used to respond to `roots/list` from servers).
    pub(crate) roots_provider: Arc<Mutex<Option<Arc<dyn Fn() -> Vec<crate::Root> + Send + Sync>>>>,
    /// Provider for LLM sampling (used to respond to `sampling/createMessage` from servers).
    pub(crate) sampling_provider: Arc<Mutex<Option<SamplingProvider>>>,
    /// Provider for elicitation (used to respond to `elicitation/create` from servers).
    pub(crate) elicitation_provider: Arc<Mutex<Option<ElicitationProvider>>>,
    /// Capabilities advertised by the server during initialization.
    pub(crate) capabilities: Arc<RwLock<Option<crate::ServerCapabilities>>>,
    /// Negotiated protocol version.
    pub(crate) protocol_version: Arc<RwLock<String>>,
    /// Semaphore limiting concurrent tool calls to this server.
    pub(crate) concurrency_semaphore: Arc<tokio::sync::Semaphore>,
}

/// Split a command string into arguments, respecting shell-style quoting.
///
/// Handles double quotes, single quotes, and backslash escaping.
/// Falls back to `split_whitespace` for unquoted segments.
fn shell_split(command: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let chars = command.chars().collect::<Vec<_>>();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
                i += 1;
            }
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                    }
                    current.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err("Unterminated double quote in command".to_string());
                }
                i += 1; // skip closing quote
            }
            '\'' => {
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    current.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err("Unterminated single quote in command".to_string());
                }
                i += 1; // skip closing quote
            }
            '\\' => {
                if i + 1 < chars.len() {
                    i += 1;
                    current.push(chars[i]);
                }
                i += 1;
            }
            _ => {
                current.push(chars[i]);
                i += 1;
            }
        }
    }

    if !current.is_empty() {
        parts.push(current);
    }

    if parts.is_empty() {
        return Err("Empty command after parsing".to_string());
    }

    Ok(parts)
}

impl McpServerHandle {
    /// Start the MCP server process and perform initialization handshake.
    pub(crate) async fn start(&self) -> Result<(), String> {
        let mut parts: Vec<String> = shell_split(&self.command)?;
        parts.extend(self.args.iter().cloned());

        if parts.is_empty() {
            return Err(format!("MCP server '{}' has empty command", self.name));
        }

        let mut cmd = Command::new(&parts[0]);
        cmd.args(&parts[1..])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().map_err(|e| {
            format!(
                "MCP server '{}' failed to spawn '{}': {}",
                self.name, self.command, e
            )
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("MCP server '{}' stdin not available", self.name))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("MCP server '{}' stdout not available", self.name))?;
        let stderr = child.stderr.take(); // Optional — some servers don't produce stderr

        // Store child and stdin
        *self.child.lock().await = Some(child);
        *self.stdin.lock().await = Some(stdin);

        // Start stdout reader
        let pending = self.pending.clone();
        let server_name = self.name.clone();
        let notification_tx = self.notification_tx.clone();
        let reader = BufReader::new(stdout);
        let stdin_clone = self.stdin.clone();
        let roots_provider = self.roots_provider.clone();
        let sampling_provider = self.sampling_provider.clone();
        let elicitation_provider = self.elicitation_provider.clone();
        let handle = tokio::spawn(async move {
            Self::read_responses(
                reader,
                &pending,
                &server_name,
                &notification_tx,
                stdin_clone,
                roots_provider,
                sampling_provider,
                elicitation_provider,
            )
            .await;
        });
        *self.reader_task.lock().await = Some(handle);

        // Start stderr reader for smart routing of server diagnostics.
        if let Some(stderr_stream) = stderr {
            let stderr_name = self.name.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stderr_stream);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line.trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    let lower = line.to_lowercase();
                    // Route based on content severity
                    if lower.contains("error") || lower.contains("fatal") || lower.contains("panic")
                    {
                        warn!(server = %stderr_name, stderr = %line, "MCP server stderr");
                    } else if lower.contains("warn") {
                        warn!(server = %stderr_name, stderr = %line, "MCP server stderr (warning)");
                    } else {
                        debug!(server = %stderr_name, stderr = %line, "MCP server stderr");
                    }
                }
            });
        }

        // Send initialize request (uses connection timeout, not regular request timeout)
        *self.state.write().await = ServerState::Starting;
        let init_response = self
            .send_request_with_timeout(
                "initialize",
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "roots": { "listChanged": true },
                        "sampling": {}
                    },
                    "clientInfo": {"name": "shannon-code", "version": "0.1.0"}
                }),
                self.connection_timeout,
            )
            .await?;

        debug!(
            server = %self.name,
            response = %init_response,
            "MCP server initialized"
        );

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
                    "MCP server capabilities"
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
                        "Protocol version mismatch with MCP server"
                    );
                }
            }
        }

        // Send initialized notification (no id, no response expected)
        self.send_notification("notifications/initialized", serde_json::json!({}))
            .await?;

        *self.state.write().await = ServerState::Healthy;
        *self.started_at.write().await = Some(Instant::now());
        info!(server = %self.name, "MCP server is healthy");
        Ok(())
    }

    /// Background task: read JSON-RPC responses from stdout and route to pending requests.
    #[allow(clippy::too_many_arguments)]
    async fn read_responses(
        reader: BufReader<ChildStdout>,
        pending: &DashMap<u64, PendingRequest>,
        server_name: &str,
        notification_tx: &tokio::sync::mpsc::Sender<(String, Value)>,
        stdin: Arc<Mutex<Option<ChildStdin>>>,
        roots_provider: Arc<Mutex<Option<Arc<dyn Fn() -> Vec<crate::Root> + Send + Sync>>>>,
        sampling_provider: Arc<Mutex<Option<SamplingProvider>>>,
        elicitation_provider: Arc<Mutex<Option<ElicitationProvider>>>,
    ) {
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }

            let value: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    debug!(
                        server = %server_name,
                        line = %line.chars().take(200).collect::<String>(),
                        error = %e,
                        "Failed to parse JSON-RPC response"
                    );
                    continue;
                }
            };

            // Handle server→client requests (message has both id and method).
            // Must check before regular responses since server requests also carry `id`.
            if value.get("method").and_then(|m| m.as_str()) == Some("roots/list")
                && value.get("id").is_some()
            {
                let req_id = value.get("id").cloned();
                let provider = roots_provider.lock().await;
                let roots = match provider.as_ref() {
                    Some(p) => p(),
                    None => vec![],
                };
                drop(provider);

                let result = serde_json::to_value(crate::ListRootsResult { roots })
                    .unwrap_or_else(|_| serde_json::json!({ "roots": [] }));

                if let Some(req_id) = req_id {
                    let response = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": result,
                    });
                    let mut stdin_guard = stdin.lock().await;
                    if let Some(ref mut writer) = *stdin_guard {
                        let mut msg = serde_json::to_string(&response).unwrap_or_default();
                        msg.push('\n');
                        if let Err(e) = writer.write_all(msg.as_bytes()).await {
                            warn!("Failed to write response to MCP server: {e}");
                            continue;
                        }
                        if let Err(e) = writer.flush().await {
                            warn!("Failed to flush MCP server stdin: {e}");
                        }
                    }
                }
            }
            // Handle sampling/createMessage server→client request.
            else if value.get("method").and_then(|m| m.as_str()) == Some("sampling/createMessage")
                && value.get("id").is_some()
            {
                let req_id = value.get("id").cloned();
                let provider = sampling_provider.lock().await;
                let response_value = if let Some(ref handler) = *provider {
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
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "error": { "code": -32601, "message": "Sampling not supported" },
                    })
                };
                drop(provider);

                let mut stdin_guard = stdin.lock().await;
                if let Some(ref mut writer) = *stdin_guard {
                    let mut msg = serde_json::to_string(&response_value).unwrap_or_default();
                    msg.push('\n');
                    if let Err(e) = writer.write_all(msg.as_bytes()).await {
                        warn!("Failed to write sampling response to MCP server: {e}");
                    }
                    if let Err(e) = writer.flush().await {
                        warn!("Failed to flush MCP server stdin: {e}");
                    }
                }
            }
            // Handle elicitation/create server→client request.
            else if value.get("method").and_then(|m| m.as_str()) == Some("elicitation/create")
                && value.get("id").is_some()
            {
                let req_id = value.get("id").cloned();
                let provider = elicitation_provider.lock().await;
                let response_value = if let Some(ref handler) = *provider {
                    let params = value
                        .get("params")
                        .cloned()
                        .unwrap_or(serde_json::json!({}));
                    match serde_json::from_value::<crate::ElicitationRequest>(params) {
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
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "error": { "code": -32601, "message": "Elicitation not supported" },
                    })
                };
                drop(provider);

                let mut stdin_guard = stdin.lock().await;
                if let Some(ref mut writer) = *stdin_guard {
                    let mut msg = serde_json::to_string(&response_value).unwrap_or_default();
                    msg.push('\n');
                    if let Err(e) = writer.write_all(msg.as_bytes()).await {
                        warn!("Failed to write elicitation response to MCP server: {e}");
                    }
                    if let Err(e) = writer.flush().await {
                        warn!("Failed to flush MCP server stdin: {e}");
                    }
                }
            }
            // Extract the id to route responses to pending requests.
            else if let Some(id) = value.get("id").and_then(|v| v.as_u64()) {
                if let Some((_, pending_req)) = pending.remove(&id) {
                    if let Err(e) = pending_req.tx.send(value) {
                        debug!(server = %server_name, error = %e, "Failed to deliver response to pending request (caller may have timed out)");
                    }
                }
            }
            // Progress notifications are routed to the matching pending request.
            else if value.get("method").and_then(|m| m.as_str()) == Some("notifications/progress")
            {
                if let Some(token) = value
                    .get("params")
                    .and_then(|p| p.get("progressToken"))
                    .cloned()
                {
                    // Find the pending request with this progress token and invoke callback.
                    for entry in pending.iter() {
                        if entry.value().progress_token.as_ref() == Some(&token) {
                            if let Some(ref cb) = entry.value().on_progress {
                                let progress = value
                                    .get("params")
                                    .and_then(|p| p.get("progress"))
                                    .and_then(|v| v.as_f64())
                                    .unwrap_or(0.0);
                                let total = value
                                    .get("params")
                                    .and_then(|p| p.get("total"))
                                    .and_then(|v| v.as_f64());
                                cb(progress, total);
                            }
                            break;
                        }
                    }
                }
            }
            // Handle incoming cancellation notifications from the server.
            // The server cancels a request it previously received.
            else if value.get("method").and_then(|m| m.as_str())
                == Some("notifications/cancelled")
            {
                if let Some(params) = value.get("params") {
                    if let Some(request_id) = params.get("requestId").and_then(|v| v.as_u64()) {
                        let reason = params
                            .get("reason")
                            .and_then(|r| r.as_str())
                            .unwrap_or("cancelled by server");
                        if let Some((_, pending_req)) = pending.remove(&request_id) {
                            let error_response = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": request_id,
                                "error": { "code": -32800, "message": format!("Request cancelled: {reason}") }
                            });
                            if let Err(e) = pending_req.tx.send(error_response) {
                                debug!(server = %server_name, error = %e, request_id, "Failed to deliver cancellation response (caller may have timed out)");
                            }
                            info!(
                                server = %server_name,
                                request_id,
                                reason,
                                "MCP server cancelled request"
                            );
                        }
                    }
                }
            }
            // Server-initiated request with both method and id that wasn't handled above —
            // try to route as a response first to avoid dropping pending request results.
            else if value.get("method").is_some() && value.get("id").is_none() {
                debug!(
                    server = %server_name,
                    method = %value["method"],
                    "Received notification from MCP server"
                );
                if let Err(e) = notification_tx.try_send((server_name.to_string(), value)) {
                    debug!(server = %server_name, error = %e, "Failed to forward notification from MCP server");
                }
            }
        }
        debug!(server = %server_name, "Stdout reader ended");
    }

    /// Send a JSON-RPC request and wait for the response.
    pub(crate) async fn send_request(&self, method: &str, params: Value) -> Result<Value, String> {
        self.send_request_with_progress(method, params, None, None, self.request_timeout)
            .await
    }

    /// Send a JSON-RPC request with a specific timeout.
    pub(crate) async fn send_request_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        self.send_request_with_progress(method, params, None, None, timeout)
            .await
    }

    /// Send a JSON-RPC request with optional progress reporting.
    ///
    /// When `progress_token` is `Some`, the token is included in `_meta.progressToken`
    /// of the request params so the server can send `notifications/progress`.
    /// The `on_progress` callback is invoked for each progress notification received.
    pub(crate) async fn send_request_with_progress(
        &self,
        method: &str,
        params: Value,
        progress_token: Option<Value>,
        on_progress: Option<Arc<dyn Fn(f64, Option<f64>) + Send + Sync>>,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        // Inject _meta.progressToken if provided.
        let params = match progress_token {
            Some(ref token) => {
                let mut p = params;
                if let Some(obj) = p.as_object_mut() {
                    obj.insert(
                        "_meta".to_string(),
                        serde_json::json!({ "progressToken": token }),
                    );
                }
                p
            }
            None => params,
        };

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let (tx, rx) = oneshot::channel();
        self.pending.insert(
            id,
            PendingRequest {
                tx,
                progress_token: progress_token.clone(),
                on_progress,
            },
        );

        // Write request to stdin
        {
            let mut stdin_guard = self.stdin.lock().await;
            let stdin = stdin_guard.as_mut().ok_or_else(|| {
                self.pending.remove(&id);
                format!("MCP server '{}' stdin not available", self.name)
            })?;

            let request_str = serde_json::to_string(&request).unwrap_or_default();
            stdin.write_all(request_str.as_bytes()).await.map_err(|e| {
                self.pending.remove(&id);
                format!("MCP server '{}' stdin write failed: {}", self.name, e)
            })?;
            stdin.write_all(b"\n").await.map_err(|e| {
                self.pending.remove(&id);
                format!("MCP server '{}' newline write failed: {}", self.name, e)
            })?;
            stdin.flush().await.map_err(|e| {
                self.pending.remove(&id);
                format!("MCP server '{}' flush failed: {}", self.name, e)
            })?;
        }

        // Wait for response with timeout
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(response)) => {
                // Check for JSON-RPC error
                if let Some(error) = response.get("error") {
                    let msg = error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown error");
                    Err(format!("MCP server '{}' error: {}", self.name, msg))
                } else {
                    Ok(response)
                }
            }
            Ok(Err(_)) => Err(format!(
                "MCP server '{}' response channel closed",
                self.name
            )),
            Err(_) => {
                self.pending.remove(&id);
                Err(format!(
                    "MCP server '{}' request timed out after {:?}",
                    self.name, timeout
                ))
            }
        }
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

        let mut stdin_guard = self.stdin.lock().await;
        let stdin = stdin_guard
            .as_mut()
            .ok_or_else(|| format!("MCP server '{}' stdin not available", self.name))?;

        let notification_str = serde_json::to_string(&notification).unwrap_or_default();
        stdin
            .write_all(notification_str.as_bytes())
            .await
            .map_err(|e| {
                format!(
                    "MCP server '{}' notification write failed: {}",
                    self.name, e
                )
            })?;
        stdin.write_all(b"\n").await.map_err(|e| {
            format!(
                "MCP server '{}' notification newline failed: {}",
                self.name, e
            )
        })?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("MCP server '{}' flush failed: {}", self.name, e))?;

        Ok(())
    }

    /// Call a tool on this server via `tools/call`.
    pub(crate) async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> ToolResult<ToolOutput> {
        self.call_tool_with_progress(tool_name, arguments, None)
            .await
    }

    /// Call a tool with optional progress reporting.
    ///
    /// When `on_progress` is `Some`, a unique progress token is generated and
    /// sent in `_meta.progressToken`. The server may then emit
    /// `notifications/progress` which triggers the callback.
    pub(crate) async fn call_tool_with_progress(
        &self,
        tool_name: &str,
        arguments: Value,
        on_progress: Option<Arc<dyn Fn(f64, Option<f64>) + Send + Sync>>,
    ) -> ToolResult<ToolOutput> {
        // Check state
        {
            let state = self.state.read().await;
            if !matches!(*state, ServerState::Healthy) {
                return Err(ToolError::ExecutionFailed(format!(
                    "MCP server '{}' is not healthy (state: {:?})",
                    self.name, *state
                )));
            }
        }

        self.request_count.fetch_add(1, Ordering::Relaxed);

        // Generate a progress token if a callback was provided.
        let (progress_token, progress_cb) = match on_progress {
            Some(cb) => {
                let token = serde_json::json!(format!(
                    "pg-{}-{}",
                    self.name,
                    self.next_id.load(Ordering::Relaxed)
                ));
                (Some(token), Some(cb))
            }
            None => (None, None),
        };

        let response = self
            .send_request_with_progress(
                "tools/call",
                serde_json::json!({
                    "name": tool_name,
                    "arguments": arguments,
                }),
                progress_token,
                progress_cb,
                self.tool_timeout,
            )
            .await
            .map_err(|e| {
                self.error_count.fetch_add(1, Ordering::Relaxed);
                ToolError::ExecutionFailed(e)
            })?;

        // Parse the response content
        if let Some(result) = response.get("result") {
            if let Some(content_array) = result.get("content").and_then(|c| c.as_array()) {
                let is_error = result
                    .get("isError")
                    .and_then(|e| e.as_bool())
                    .unwrap_or(false);

                if is_error {
                    // For errors, include ALL content blocks to preserve full error context.
                    let normalized = normalize_error_content(content_array);
                    if !normalized.is_empty() {
                        let content = truncate_tool_result(&normalized, MAX_TOOL_RESULT_CHARS);
                        return Ok(ToolOutput::error(content));
                    }
                } else {
                    // For success, extract only text blocks (current behavior)
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
            // Fallback: return the result as JSON string (also truncated)
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

    /// Send a ping to check server health.
    pub(crate) async fn ping(&self) -> Result<(), String> {
        let state = self.state.read().await;
        if matches!(*state, ServerState::Stopped) {
            return Err(format!("MCP server '{}' is stopped", self.name));
        }
        drop(state);

        match self.send_request("ping", serde_json::json!({})).await {
            Ok(_) => {
                *self.state.write().await = ServerState::Healthy;
                *self.last_health_check.write().await = Some(Instant::now());
                Ok(())
            }
            Err(e) => {
                *self.state.write().await = ServerState::Unhealthy(e.clone());
                Err(e)
            }
        }
    }

    /// Get detailed status including metrics.
    pub(crate) async fn get_status(&self) -> ServerStatus {
        let state = self.state.read().await.clone();
        let started_at = *self.started_at.read().await;
        let last_check = *self.last_health_check.read().await;
        let now = Instant::now();

        ServerStatus {
            name: self.name.clone(),
            uptime: started_at.map(|t| now.duration_since(t)),
            state,
            request_count: self.request_count.load(Ordering::Relaxed),
            error_count: self.error_count.load(Ordering::Relaxed),
            restart_count: self.restart_count.load(Ordering::Relaxed),
            last_health_check: last_check.map(|t| now.duration_since(t)),
            total_result_bytes: self.total_result_bytes.load(Ordering::Relaxed),
            budget_bytes: *self.budget_bytes.read().await,
        }
    }

    /// Gracefully shut down the server process.
    pub(crate) async fn shutdown(&self) {
        info!(server = %self.name, "Shutting down MCP server");
        *self.state.write().await = ServerState::Stopped;

        // Close stdin to signal the process (graceful shutdown signal).
        {
            let mut stdin_guard = self.stdin.lock().await;
            *stdin_guard = None;
        }

        // Cancel reader task first — stops processing incoming messages.
        {
            let mut reader_guard = self.reader_task.lock().await;
            if let Some(handle) = reader_guard.take() {
                handle.abort();
            }
        }

        // Clear pending requests (respond to any waiters with an error).
        self.pending.clear();

        // Graceful shutdown: give process 2s to exit, then SIGKILL + reap.
        // Await directly so callers (shutdown_all, Drop) know the process is reaped.
        {
            let mut child_guard = self.child.lock().await;
            if let Some(mut child) = child_guard.take() {
                match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
                    Ok(Ok(_status)) => {
                        debug!(server = %self.name, "MCP server exited gracefully");
                    }
                    _ => {
                        if let Err(e) = child.kill().await {
                            debug!(server = %self.name, error = %e, "Failed to kill MCP server process");
                        }
                        if let Err(e) = child.wait().await {
                            debug!(server = %self.name, error = %e, "Failed to reap MCP server process");
                        }
                        warn!(server = %self.name, "Force-killed MCP server process (zombie reaped)");
                    }
                }
            }
        }
    }

    /// Get the current state.
    pub(crate) async fn get_state(&self) -> ServerState {
        self.state.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashmap::DashMap;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicU64;
    use std::time::Duration;
    use tokio::sync::{Mutex, RwLock};

    /// Helper: create a minimal handle with all fields for unit testing.
    ///
    /// The handle starts in `Stopped` state with no child process, no stdin,
    /// and no reader task. Call-specific fields (pending, state, counters)
    /// can be manipulated directly in tests.
    fn make_handle(name: &str) -> McpServerHandle {
        let (ntx, _) = tokio::sync::mpsc::channel(1024);
        McpServerHandle {
            name: name.to_string(),
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            stdin: Arc::new(Mutex::new(None)),
            next_id: AtomicU64::new(1),
            pending: Arc::new(DashMap::new()),
            state: Arc::new(RwLock::new(ServerState::Stopped)),
            child: Arc::new(Mutex::new(None)),
            reader_task: Arc::new(Mutex::new(None)),
            restart_count: Arc::new(AtomicU64::new(0)),
            max_restarts: 3,
            health_interval: Duration::from_secs(60),
            request_timeout: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(30),
            tool_timeout: Duration::from_secs(120),
            started_at: Arc::new(RwLock::new(None)),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            total_result_bytes: AtomicU64::new(0),
            budget_bytes: Arc::new(RwLock::new(None)),
            last_health_check: Arc::new(RwLock::new(None)),
            notification_tx: ntx,
            roots_provider: Arc::new(Mutex::new(None)),
            sampling_provider: Arc::new(Mutex::new(None)),
            elicitation_provider: Arc::new(Mutex::new(None)),
            capabilities: Arc::new(RwLock::new(None)),
            protocol_version: Arc::new(RwLock::new(String::new())),
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    // -- shell_split tests -------------------------------------------------

    #[test]
    fn shell_split_simple_command() {
        let parts = shell_split("ls -la /tmp").unwrap();
        assert_eq!(parts, vec!["ls", "-la", "/tmp"]);
    }

    #[test]
    fn shell_split_double_quotes() {
        let parts = shell_split(r#"echo "hello world""#).unwrap();
        assert_eq!(parts, vec!["echo", "hello world"]);
    }

    #[test]
    fn shell_split_single_quotes() {
        let parts = shell_split("echo 'hello world'").unwrap();
        assert_eq!(parts, vec!["echo", "hello world"]);
    }

    #[test]
    fn shell_split_mixed_quotes() {
        let parts = shell_split(r#"cmd "arg one" argtwo 'arg three'"#).unwrap();
        assert_eq!(parts, vec!["cmd", "arg one", "argtwo", "arg three"]);
    }

    #[test]
    fn shell_split_backslash_escape() {
        let parts = shell_split(r"cmd arg\ with\ space").unwrap();
        assert_eq!(parts, vec!["cmd", "arg with space"]);
    }

    #[test]
    fn shell_split_unterminated_double_quote() {
        let result = shell_split(r#"echo "unclosed"#);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unterminated"));
    }

    #[test]
    fn shell_split_unterminated_single_quote() {
        let result = shell_split("echo 'unclosed");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unterminated"));
    }

    #[test]
    fn shell_split_empty_string() {
        let result = shell_split("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty"));
    }

    #[test]
    fn shell_split_only_whitespace() {
        let result = shell_split("   \t  ");
        assert!(result.is_err());
    }

    #[test]
    fn shell_split_single_command() {
        let parts = shell_split("node").unwrap();
        assert_eq!(parts, vec!["node"]);
    }

    #[test]
    fn shell_split_extra_whitespace() {
        let parts = shell_split("  cmd   arg1   arg2  ").unwrap();
        assert_eq!(parts, vec!["cmd", "arg1", "arg2"]);
    }

    #[test]
    fn shell_split_escaped_char_in_double_quote() {
        // Inside double quotes, \n is interpreted: backslash + next char → next char.
        // So \n becomes just 'n'.
        let parts = shell_split(r#"echo "line1\nline2""#).unwrap();
        assert_eq!(parts, vec!["echo", "line1nline2"]);
    }

    #[test]
    fn shell_split_backslash_outside_quotes() {
        // Outside quotes, backslash escapes the next character.
        let parts = shell_split(r"cmd hello\ world").unwrap();
        assert_eq!(parts, vec!["cmd", "hello world"]);
    }

    // -- get_state / get_status tests --------------------------------------

    #[tokio::test]
    async fn get_state_initial_is_stopped() {
        let handle = make_handle("test-srv");
        assert_eq!(handle.get_state().await, ServerState::Stopped);
    }

    #[tokio::test]
    async fn get_state_transition_to_healthy() {
        let handle = make_handle("test-srv");
        *handle.state.write().await = ServerState::Healthy;
        assert_eq!(handle.get_state().await, ServerState::Healthy);
    }

    #[tokio::test]
    async fn get_state_unhealthy_carries_message() {
        let handle = make_handle("test-srv");
        *handle.state.write().await = ServerState::Unhealthy("timeout".to_string());
        assert_eq!(
            handle.get_state().await,
            ServerState::Unhealthy("timeout".to_string())
        );
    }

    #[tokio::test]
    async fn get_status_reflects_state() {
        let handle = make_handle("status-srv");
        let status = handle.get_status().await;
        assert_eq!(status.name, "status-srv");
        assert_eq!(status.state, ServerState::Stopped);
        assert!(status.uptime.is_none());
        assert_eq!(status.request_count, 0);
        assert_eq!(status.error_count, 0);
        assert_eq!(status.restart_count, 0);
    }

    #[tokio::test]
    async fn get_status_includes_metrics() {
        let handle = make_handle("metric-srv");
        handle.request_count.store(42, Ordering::Relaxed);
        handle.error_count.store(3, Ordering::Relaxed);
        handle.total_result_bytes.store(1024, Ordering::Relaxed);
        *handle.budget_bytes.write().await = Some(10_000);
        *handle.state.write().await = ServerState::Healthy;
        *handle.started_at.write().await = Some(Instant::now());

        let status = handle.get_status().await;
        assert_eq!(status.request_count, 42);
        assert_eq!(status.error_count, 3);
        assert_eq!(status.total_result_bytes, 1024);
        assert_eq!(status.budget_bytes, Some(10_000));
        assert!(status.uptime.is_some());
    }

    // -- ping tests --------------------------------------------------------

    #[tokio::test]
    async fn ping_returns_error_when_stopped() {
        let handle = make_handle("stopped-srv");
        let result = handle.ping().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("stopped"));
    }

    #[tokio::test]
    async fn ping_returns_error_when_starting() {
        let handle = make_handle("starting-srv");
        *handle.state.write().await = ServerState::Starting;
        // ping sends a request but there is no stdin, so it will fail
        let result = handle.ping().await;
        assert!(result.is_err());
    }

    // -- send_request error tests ------------------------------------------

    #[tokio::test]
    async fn send_request_fails_when_no_stdin() {
        let handle = make_handle("no-stdin");
        *handle.state.write().await = ServerState::Healthy;
        let result = handle
            .send_request("tools/list", serde_json::json!({}))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("stdin not available"));
    }

    #[tokio::test]
    async fn send_notification_fails_when_no_stdin() {
        let handle = make_handle("no-stdin");
        let result = handle
            .send_notification("notifications/initialized", serde_json::json!({}))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("stdin not available"));
    }

    // -- call_tool state check tests ---------------------------------------

    #[tokio::test]
    async fn call_tool_rejects_when_not_healthy() {
        let handle = make_handle("unhealthy-srv");
        *handle.state.write().await = ServerState::Unhealthy("crashed".to_string());
        let result = handle.call_tool("some_tool", serde_json::json!({})).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            shannon_tool_interface::ToolError::ExecutionFailed(msg) => {
                assert!(msg.contains("not healthy"));
                assert!(msg.contains("crashed"));
            }
            other => panic!("Expected ExecutionFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn call_tool_rejects_when_starting() {
        let handle = make_handle("starting-srv");
        *handle.state.write().await = ServerState::Starting;
        let result = handle.call_tool("some_tool", serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn call_tool_rejects_when_stopped() {
        let handle = make_handle("stopped-srv");
        let result = handle.call_tool("some_tool", serde_json::json!({})).await;
        assert!(result.is_err());
    }

    // -- shutdown tests ----------------------------------------------------

    #[tokio::test]
    async fn shutdown_sets_state_to_stopped() {
        let handle = make_handle("shutdown-srv");
        *handle.state.write().await = ServerState::Healthy;
        handle.shutdown().await;
        assert_eq!(handle.get_state().await, ServerState::Stopped);
    }

    #[tokio::test]
    async fn shutdown_clears_stdin() {
        let handle = make_handle("shutdown-srv");
        // stdin starts as None already
        handle.shutdown().await;
        assert!(handle.stdin.lock().await.is_none());
    }

    #[tokio::test]
    async fn shutdown_clears_pending_requests() {
        let handle = make_handle("shutdown-srv");
        let (tx, rx) = oneshot::channel();
        handle.pending.insert(
            1,
            PendingRequest {
                tx,
                progress_token: None,
                on_progress: None,
            },
        );
        assert_eq!(handle.pending.len(), 1);

        handle.shutdown().await;
        assert!(handle.pending.is_empty());
        // The oneshot receiver should be dropped (sender dropped on clear).
        drop(rx);
    }

    #[tokio::test]
    async fn shutdown_idempotent() {
        let handle = make_handle("multi-shutdown");
        *handle.state.write().await = ServerState::Healthy;
        handle.shutdown().await;
        handle.shutdown().await;
        assert_eq!(handle.get_state().await, ServerState::Stopped);
    }

    // -- start failure tests -----------------------------------------------

    #[tokio::test]
    async fn start_fails_with_nonexistent_command() {
        // Create a handle with a nonexistent command directly.
        // We can't use make_handle() because command is not mutable from outside.
        let (ntx, _) = tokio::sync::mpsc::channel(1024);
        let handle = McpServerHandle {
            name: "bad-cmd".to_string(),
            command: "/nonexistent/binary".to_string(),
            args: vec![],
            env: HashMap::new(),
            stdin: Arc::new(Mutex::new(None)),
            next_id: AtomicU64::new(1),
            pending: Arc::new(DashMap::new()),
            state: Arc::new(RwLock::new(ServerState::Stopped)),
            child: Arc::new(Mutex::new(None)),
            reader_task: Arc::new(Mutex::new(None)),
            restart_count: Arc::new(AtomicU64::new(0)),
            max_restarts: 3,
            health_interval: Duration::from_secs(60),
            request_timeout: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(30),
            tool_timeout: Duration::from_secs(120),
            started_at: Arc::new(RwLock::new(None)),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            total_result_bytes: AtomicU64::new(0),
            budget_bytes: Arc::new(RwLock::new(None)),
            last_health_check: Arc::new(RwLock::new(None)),
            notification_tx: ntx,
            roots_provider: Arc::new(Mutex::new(None)),
            sampling_provider: Arc::new(Mutex::new(None)),
            elicitation_provider: Arc::new(Mutex::new(None)),
            capabilities: Arc::new(RwLock::new(None)),
            protocol_version: Arc::new(RwLock::new(String::new())),
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        };
        let result = handle.start().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("failed to spawn"));
    }

    // -- next_id counter test ----------------------------------------------

    #[test]
    fn next_id_increments_atomically() {
        let handle = make_handle("id-test");
        assert_eq!(handle.next_id.load(Ordering::Relaxed), 1);
        let id1 = handle.next_id.fetch_add(1, Ordering::Relaxed);
        let id2 = handle.next_id.fetch_add(1, Ordering::Relaxed);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(handle.next_id.load(Ordering::Relaxed), 3);
    }

    // -- request_count / error_count tests ---------------------------------

    #[test]
    fn counters_increment_correctly() {
        let handle = make_handle("counter-test");
        assert_eq!(handle.request_count.load(Ordering::Relaxed), 0);
        assert_eq!(handle.error_count.load(Ordering::Relaxed), 0);
        assert_eq!(handle.total_result_bytes.load(Ordering::Relaxed), 0);

        handle.request_count.fetch_add(5, Ordering::Relaxed);
        handle.error_count.fetch_add(2, Ordering::Relaxed);
        handle.total_result_bytes.fetch_add(1024, Ordering::Relaxed);

        assert_eq!(handle.request_count.load(Ordering::Relaxed), 5);
        assert_eq!(handle.error_count.load(Ordering::Relaxed), 2);
        assert_eq!(handle.total_result_bytes.load(Ordering::Relaxed), 1024);
    }
}
