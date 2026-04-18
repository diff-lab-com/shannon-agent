//! MCP Process Pool — persistent MCP server process management.
//!
//! Manages long-lived MCP server processes with:
//! - Persistent JSON-RPC connections over stdio
//! - Health monitoring via periodic ping
//! - Automatic restart with exponential backoff on failure
//! - Graceful shutdown (SIGTERM → wait → SIGKILL)
//!
//! # Architecture
//!
//! ```text
//! McpProcessPool
//! ├── HashMap<String, McpServerHandle>
//! │   ├── stdin writer (Arc<Mutex>)
//! │   ├── stdout reader task (JoinHandle)
//! │   ├── pending requests (DashMap<id, oneshot>)
//! │   └── state: Starting / Healthy / Unhealthy / Stopped
//! └── Background health-check task
//! ```

use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use serde::Serialize;
use shannon_tool_interface::{Tool, ToolError, ToolOutput, ToolResult};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{oneshot, Mutex, RwLock};
use tracing::{debug, info, warn};

/// Type alias for the async sampling callback.
///
/// Takes a `CreateMessageRequest` and returns a `CreateMessageResult`.
type SamplingProvider = Arc<
    dyn Fn(
            crate::CreateMessageRequest,
        ) -> Pin<Box<dyn Future<Output = Result<crate::CreateMessageResult, String>> + Send>>
        + Send
        + Sync,
>;

/// Maximum length for MCP tool descriptions (in characters).
///
/// Some servers dump 15-60KB into `tool.description`, wasting ~15K tokens per turn.
/// Claude Code caps at 2,048 chars; we match that.
const MAX_TOOL_DESCRIPTION_CHARS: usize = 2048;

/// Maximum length for MCP tool results (in characters).
///
/// Some MCP tools return 100KB+ responses. Sending all of that to the LLM wastes
/// tokens and degrades response quality. Claude Code truncates at ~25K chars.
const MAX_TOOL_RESULT_CHARS: usize = 25_000;

/// Default timeout for establishing a new MCP server connection (initialize handshake).
const DEFAULT_CONNECTION_TIMEOUT_SECS: u64 = 30;

/// Default timeout for regular JSON-RPC requests (tools/list, ping, etc.).
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 60;

/// Default timeout for tool call requests (tools/call).
///
/// Tool calls can be long-running (e.g. file search, code analysis).
/// Claude Code uses a very generous timeout (~27.8h); we use 10 minutes
/// which covers virtually all realistic tool executions.
const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 600;

/// Truncate a tool result string to [`MAX_TOOL_RESULT_CHARS`].
///
/// For JSON content, preserves opening structure and adds a truncation notice.
/// For plain text, cuts at the last newline boundary within budget when possible.
fn truncate_tool_result(content: &str) -> String {
    if content.len() <= MAX_TOOL_RESULT_CHARS {
        return content.to_string();
    }

    let original_len = content.len();

    // Try to detect JSON and preserve structure
    let trimmed = content.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        // JSON-like content: truncate at char boundary, add notice
        let mut end = MAX_TOOL_RESULT_CHARS;
        while !content.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        let truncated = &content[..end];
        // Try to cut at a reasonable boundary (comma or newline)
        let cut = truncated
            .rfind(|c: char| c == '\n' || c == ',')
            .unwrap_or(end);
        let cut = if content.is_char_boundary(cut) { cut } else { end };
        format!(
            "{}\n\n[...truncated: showed {} of {} chars ({:.0}% omitted)]",
            &content[..cut],
            cut,
            original_len,
            ((original_len - cut) as f64 / original_len as f64) * 100.0,
        )
    } else {
        // Plain text: try to cut at newline boundary
        let mut end = MAX_TOOL_RESULT_CHARS;
        while !content.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        let truncated = &content[..end];
        let cut = truncated
            .rfind('\n')
            .unwrap_or(end);
        let cut = if content.is_char_boundary(cut) { cut } else { end };
        format!(
            "{}\n\n[...truncated: showed {} of {} chars ({:.0}% omitted)]",
            &content[..cut],
            cut,
            original_len,
            ((original_len - cut) as f64 / original_len as f64) * 100.0,
        )
    }
}

/// Normalize multi-element error content into a single coherent error message.
///
/// MCP servers can return errors with multiple content blocks of different types
/// (text, images, embedded resources). This function extracts all blocks into a
/// single string, summarizing non-text content.
fn normalize_error_content(content_array: &[serde_json::Value]) -> String {
    content_array
        .iter()
        .map(|block| {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => block
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
                Some("image") => {
                    let mime = block.get("mimeType")
                        .and_then(|m| m.as_str())
                        .unwrap_or("image/unknown");
                    format!("[{mime} image]")
                }
                Some("resource") => {
                    let uri = block.get("resource")
                        .and_then(|r| r.get("uri"))
                        .and_then(|u| u.as_str())
                        .unwrap_or("unknown");
                    let text = block.get("resource")
                        .and_then(|r| r.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    if text.is_empty() {
                        format!("[resource: {uri}]")
                    } else {
                        format!("[resource: {uri}]\n{text}")
                    }
                }
                other => {
                    let text = block.get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    if text.is_empty() {
                        format!("[{} block]", other.unwrap_or("unknown"))
                    } else {
                        text.to_string()
                    }
                }
            }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

/// Lifecycle state of an MCP server process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ServerState {
    /// Process is being started and initialized.
    Starting,
    /// Process is healthy and accepting requests.
    Healthy,
    /// Process failed health check or crashed. Contains error message.
    Unhealthy(String),
    /// Process has been shut down.
    Stopped,
}

/// Runtime status of an MCP server process, including health metrics.
#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    /// Server name.
    pub name: String,
    /// Current lifecycle state.
    pub state: ServerState,
    /// Time since the server was last started (None if not running).
    pub uptime: Option<Duration>,
    /// Total number of tool call requests sent.
    pub request_count: u64,
    /// Total number of failed requests.
    pub error_count: u64,
    /// Number of restart attempts since initial start.
    pub restart_count: u64,
    /// Time since last successful health check (None if never checked).
    pub last_health_check: Option<Duration>,
}

// ---------------------------------------------------------------------------
// Pending request tracking
// ---------------------------------------------------------------------------

/// A pending JSON-RPC request waiting for a response.
struct PendingRequest {
    /// Oneshot channel to deliver the response.
    tx: oneshot::Sender<Value>,
    /// When this request was created (for timeout tracking).
    created_at: Instant,
    /// Optional progress token sent in `_meta.progressToken`.
    progress_token: Option<Value>,
    /// Optional callback invoked on `notifications/progress` for this request.
    on_progress: Option<Arc<dyn Fn(f64, Option<f64>) + Send + Sync>>,
}

// ---------------------------------------------------------------------------
// Server handle
// ---------------------------------------------------------------------------

/// Manages a single persistent MCP server process.
struct McpServerHandle {
    /// Server name (for logging).
    name: String,
    /// Command to spawn the process.
    command: String,
    /// Command arguments.
    args: Vec<String>,
    /// Environment variables for the process.
    env: HashMap<String, String>,
    /// stdin writer — locked during writes to serialize requests.
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    /// Next JSON-RPC request id.
    next_id: AtomicU64,
    /// Pending requests keyed by JSON-RPC id.
    pending: Arc<DashMap<u64, PendingRequest>>,
    /// Current state.
    state: Arc<RwLock<ServerState>>,
    /// Child process handle (for kill on drop).
    child: Arc<Mutex<Option<Child>>>,
    /// Background stdout reader task.
    reader_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// How many times this server has been restarted.
    restart_count: Arc<AtomicU64>,
    /// Maximum restart attempts before giving up.
    max_restarts: u32,
    /// Health check interval.
    health_interval: Duration,
    /// Request timeout (for regular JSON-RPC requests like tools/list, ping).
    request_timeout: Duration,
    /// Connection timeout (for initialize handshake during startup).
    connection_timeout: Duration,
    /// Tool call timeout (for tools/call which can be long-running).
    tool_timeout: Duration,
    /// When the server was last started successfully.
    started_at: Arc<RwLock<Option<Instant>>>,
    /// Total number of tool call requests.
    request_count: AtomicU64,
    /// Total number of failed tool calls.
    error_count: AtomicU64,
    /// When the last successful health check occurred.
    last_health_check: Arc<RwLock<Option<Instant>>>,
    /// Channel to forward server notifications to the pool.
    notification_tx: tokio::sync::mpsc::UnboundedSender<(String, Value)>,
    /// Provider for filesystem roots (used to respond to `roots/list` from servers).
    roots_provider: Arc<Mutex<Option<Arc<dyn Fn() -> Vec<crate::Root> + Send + Sync>>>>,
    /// Provider for LLM sampling (used to respond to `sampling/createMessage` from servers).
    sampling_provider: Arc<Mutex<Option<SamplingProvider>>>,
}

impl McpServerHandle {
    /// Start the MCP server process and perform initialization handshake.
    async fn start(&self) -> Result<(), String> {
        let mut parts: Vec<String> = self
            .command
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
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

        let stdin = child.stdin.take().ok_or_else(|| {
            format!("MCP server '{}' stdin not available", self.name)
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            format!("MCP server '{}' stdout not available", self.name)
        })?;

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
        let handle = tokio::spawn(async move {
            Self::read_responses(reader, &pending, &server_name, &notification_tx, stdin_clone, roots_provider, sampling_provider).await;
        });
        *self.reader_task.lock().await = Some(handle);

        // Send initialize request (uses connection timeout, not regular request timeout)
        *self.state.write().await = ServerState::Starting;
        let init_response = self
            .send_request_with_timeout("initialize", serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "roots": { "listChanged": true },
                    "sampling": {}
                },
                "clientInfo": {"name": "shannon-code", "version": "0.1.0"}
            }), self.connection_timeout)
            .await?;

        debug!(
            server = %self.name,
            response = %init_response,
            "MCP server initialized"
        );

        // Send initialized notification (no id, no response expected)
        self.send_notification("notifications/initialized", serde_json::json!({}))
            .await?;

        *self.state.write().await = ServerState::Healthy;
        *self.started_at.write().await = Some(Instant::now());
        info!(server = %self.name, "MCP server is healthy");
        Ok(())
    }

    /// Background task: read JSON-RPC responses from stdout and route to pending requests.
    async fn read_responses(
        reader: BufReader<ChildStdout>,
        pending: &DashMap<u64, PendingRequest>,
        server_name: &str,
        notification_tx: &tokio::sync::mpsc::UnboundedSender<(String, Value)>,
        stdin: Arc<Mutex<Option<ChildStdin>>>,
        roots_provider: Arc<Mutex<Option<Arc<dyn Fn() -> Vec<crate::Root> + Send + Sync>>>>,
        sampling_provider: Arc<Mutex<Option<SamplingProvider>>>,
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
                        let _ = writer.write_all(msg.as_bytes()).await;
                        let _ = writer.flush().await;
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
                    let params = value.get("params").cloned().unwrap_or(serde_json::json!({}));
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
                    let _ = writer.write_all(msg.as_bytes()).await;
                    let _ = writer.flush().await;
                }
            }
            // Extract the id to route responses to pending requests.
            else if let Some(id) = value.get("id").and_then(|v| v.as_u64()) {
                if let Some((_, pending_req)) = pending.remove(&id) {
                    let _ = pending_req.tx.send(value);
                }
            }
            // Progress notifications are routed to the matching pending request.
            else if value.get("method").and_then(|m| m.as_str())
                == Some("notifications/progress")
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
            // Other notifications (no id) are forwarded to the pool.
            else if value.get("method").is_some() {
                debug!(
                    server = %server_name,
                    method = %value["method"],
                    "Received notification from MCP server"
                );
                let _ = notification_tx.send((server_name.to_string(), value));
            }
        }
        debug!(server = %server_name, "Stdout reader ended");
    }

    /// Send a JSON-RPC request and wait for the response.
    async fn send_request(&self, method: &str, params: Value) -> Result<Value, String> {
        self.send_request_with_progress(method, params, None, None, self.request_timeout).await
    }

    /// Send a JSON-RPC request with a specific timeout.
    async fn send_request_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        self.send_request_with_progress(method, params, None, None, timeout).await
    }

    /// Send a JSON-RPC request with optional progress reporting.
    ///
    /// When `progress_token` is `Some`, the token is included in `_meta.progressToken`
    /// of the request params so the server can send `notifications/progress`.
    /// The `on_progress` callback is invoked for each progress notification received.
    async fn send_request_with_progress(
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
                created_at: Instant::now(),
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
            stdin
                .write_all(request_str.as_bytes())
                .await
                .map_err(|e| {
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
                    self.name, self.request_timeout
                ))
            }
        }
    }

    /// Send a JSON-RPC notification (no id, no response expected).
    async fn send_notification(&self, method: &str, params: Value) -> Result<(), String> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        let mut stdin_guard = self.stdin.lock().await;
        let stdin = stdin_guard.as_mut().ok_or_else(|| {
            format!("MCP server '{}' stdin not available", self.name)
        })?;

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
        stdin.flush().await.map_err(|e| {
            format!("MCP server '{}' flush failed: {}", self.name, e)
        })?;

        Ok(())
    }

    /// Call a tool on this server via `tools/call`.
    async fn call_tool(&self, tool_name: &str, arguments: Value) -> ToolResult<ToolOutput> {
        self.call_tool_with_progress(tool_name, arguments, None).await
    }

    /// Call a tool with optional progress reporting.
    ///
    /// When `on_progress` is `Some`, a unique progress token is generated and
    /// sent in `_meta.progressToken`. The server may then emit
    /// `notifications/progress` which triggers the callback.
    async fn call_tool_with_progress(
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
                        let content = truncate_tool_result(&normalized);
                        return Ok(ToolOutput::error(content));
                    }
                } else {
                    // For success, extract only text blocks (current behavior)
                    let texts: Vec<String> = content_array
                        .iter()
                        .filter_map(|block| {
                            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                block.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                            } else {
                                None
                            }
                        })
                        .collect();

                    if !texts.is_empty() {
                        let content = truncate_tool_result(&texts.join("\n"));
                        return Ok(ToolOutput::success(content));
                    }
                }
            }
            // Fallback: return the result as JSON string (also truncated)
            return Ok(ToolOutput::success(truncate_tool_result(&result.to_string())));
        }

        Ok(ToolOutput::success(truncate_tool_result(&response.to_string())))
    }

    /// Send a ping to check server health.
    async fn ping(&self) -> Result<(), String> {
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
    async fn get_status(&self) -> ServerStatus {
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
        }
    }

    /// Gracefully shut down the server process.
    async fn shutdown(&self) {
        info!(server = %self.name, "Shutting down MCP server");
        *self.state.write().await = ServerState::Stopped;

        // Close stdin to signal the process
        {
            let mut stdin_guard = self.stdin.lock().await;
            *stdin_guard = None;
        }

        // Give the process a moment to exit gracefully
        let child_arc = self.child.clone();
        let name = self.name.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let mut child_guard = child_arc.lock().await;
            if let Some(ref mut child) = *child_guard {
                let _ = child.kill().await;
                warn!(server = %name, "Force-killed MCP server process");
            }
        });

        // Cancel reader task
        {
            let mut reader_guard = self.reader_task.lock().await;
            if let Some(handle) = reader_guard.take() {
                handle.abort();
            }
        }

        // Clear pending requests
        self.pending.clear();

        // Remove child
        *self.child.lock().await = None;
    }

    /// Get the current state.
    async fn get_state(&self) -> ServerState {
        self.state.read().await.clone()
    }
}

// ---------------------------------------------------------------------------
// Process Pool
// ---------------------------------------------------------------------------

/// Manages a pool of persistent MCP server processes.
///
/// The pool keeps server processes alive across multiple tool calls,
/// handles health monitoring, automatic restarts, and graceful shutdown.
pub struct McpProcessPool {
    /// Server handles keyed by server name.
    handles: DashMap<String, Arc<McpServerHandle>>,
    /// Health check interval.
    health_interval: Duration,
    /// Maximum restart attempts.
    max_restarts: u32,
    /// Request timeout (for regular JSON-RPC requests).
    request_timeout: Duration,
    /// Connection timeout (for initialize handshake).
    connection_timeout: Duration,
    /// Tool call timeout (for tools/call).
    tool_timeout: Duration,
    /// Background health check task handle.
    health_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// Notification receiver — servers forward JSON-RPC notifications here.
    notification_rx: Arc<Mutex<tokio::sync::mpsc::UnboundedReceiver<(String, Value)>>>,
    /// Notification sender — cloned into each server handle.
    notification_tx: tokio::sync::mpsc::UnboundedSender<(String, Value)>,
    /// Callback invoked when a server reports `notifications/tools/list_changed`.
    on_tools_changed: Arc<Mutex<Option<Arc<dyn Fn(&str) + Send + Sync>>>>,
    /// Provider for filesystem roots. Called when a server sends `roots/list`.
    roots_provider: Arc<Mutex<Option<Arc<dyn Fn() -> Vec<crate::Root> + Send + Sync>>>>,
    /// Provider for LLM sampling. Called when a server sends `sampling/createMessage`.
    sampling_provider: Arc<Mutex<Option<SamplingProvider>>>,
}

impl McpProcessPool {
    /// Create a new process pool with default settings.
    pub fn new() -> Self {
        let (notification_tx, notification_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            handles: DashMap::new(),
            health_interval: Duration::from_secs(60),
            max_restarts: 5,
            request_timeout: Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS),
            connection_timeout: Duration::from_secs(DEFAULT_CONNECTION_TIMEOUT_SECS),
            tool_timeout: Duration::from_secs(DEFAULT_TOOL_TIMEOUT_SECS),
            health_task: Arc::new(Mutex::new(None)),
            notification_rx: Arc::new(Mutex::new(notification_rx)),
            notification_tx,
            on_tools_changed: Arc::new(Mutex::new(None)),
            roots_provider: Arc::new(Mutex::new(None)),
            sampling_provider: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the health check interval.
    pub fn set_health_interval(&mut self, interval: Duration) {
        self.health_interval = interval;
    }

    /// Set the maximum restart attempts per server.
    pub fn set_max_restarts(&mut self, max: u32) {
        self.max_restarts = max;
    }

    /// Set the request timeout.
    pub fn set_request_timeout(&mut self, timeout: Duration) {
        self.request_timeout = timeout;
    }

    /// Set the connection timeout (for initialize handshake).
    pub fn set_connection_timeout(&mut self, timeout: Duration) {
        self.connection_timeout = timeout;
    }

    /// Set the tool call timeout (for tools/call).
    pub fn set_tool_timeout(&mut self, timeout: Duration) {
        self.tool_timeout = timeout;
    }

    /// Start an MCP server and add it to the pool.
    ///
    /// Returns an error if the server fails to start.
    pub async fn start_server(
        &self,
        name: &str,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<(), String> {
        let handle = Arc::new(McpServerHandle {
            name: name.to_string(),
            command: command.to_string(),
            args: args.to_vec(),
            env: env.clone(),
            stdin: Arc::new(Mutex::new(None)),
            next_id: AtomicU64::new(1),
            pending: Arc::new(DashMap::new()),
            state: Arc::new(RwLock::new(ServerState::Starting)),
            child: Arc::new(Mutex::new(None)),
            reader_task: Arc::new(Mutex::new(None)),
            restart_count: Arc::new(AtomicU64::new(0)),
            max_restarts: self.max_restarts,
            health_interval: self.health_interval,
            request_timeout: self.request_timeout,
            connection_timeout: self.connection_timeout,
            tool_timeout: self.tool_timeout,
            started_at: Arc::new(RwLock::new(None)),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            last_health_check: Arc::new(RwLock::new(None)),
            notification_tx: self.notification_tx.clone(),
            roots_provider: self.roots_provider.clone(),
            sampling_provider: self.sampling_provider.clone(),
        });

        handle.start().await?;
        self.handles.insert(name.to_string(), handle);
        Ok(())
    }

    /// Call a tool on a server in the pool.
    ///
    /// If the server is unhealthy, attempts to restart it first.
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> ToolResult<ToolOutput> {
        let handle = self
            .handles
            .get(server_name)
            .ok_or_else(|| {
                ToolError::NotFound(format!("MCP server '{server_name}' not in pool"))
            })?
            .clone();

        // Check health and restart if needed
        let state = handle.get_state().await;
        match state {
            ServerState::Healthy => {}
            ServerState::Unhealthy(err) => {
                warn!(
                    server = %server_name,
                    error = %err,
                    "MCP server unhealthy, attempting restart"
                );
                self.restart_server(&handle).await.map_err(|e| {
                    ToolError::ExecutionFailed(format!(
                        "MCP server '{server_name}' restart failed: {e}"
                    ))
                })?;
            }
            ServerState::Stopped => {
                return Err(ToolError::ExecutionFailed(format!(
                    "MCP server '{server_name}' is stopped",
                )));
            }
            ServerState::Starting => {
                // Wait briefly for startup to complete
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        match handle.call_tool(tool_name, arguments).await {
            Ok(output) => Ok(output),
            Err(e) => {
                // Mark as unhealthy on failure
                *handle.state.write().await =
                    ServerState::Unhealthy(e.to_string());
                Err(e)
            }
        }
    }

    /// Call a tool on a server with progress reporting.
    ///
    /// The `on_progress` callback is invoked each time the server sends a
    /// `notifications/progress` message. The callback receives
    /// `(progress, total)` where `total` may be `None` if the server did not
    /// include it.
    pub async fn call_tool_with_progress(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
        on_progress: Arc<dyn Fn(f64, Option<f64>) + Send + Sync>,
    ) -> ToolResult<ToolOutput> {
        let handle = self
            .handles
            .get(server_name)
            .ok_or_else(|| {
                ToolError::NotFound(format!("MCP server '{server_name}' not in pool"))
            })?
            .clone();

        // Check health and restart if needed (same as call_tool)
        let state = handle.get_state().await;
        match state {
            ServerState::Healthy => {}
            ServerState::Unhealthy(err) => {
                warn!(
                    server = %server_name,
                    error = %err,
                    "MCP server unhealthy, attempting restart"
                );
                self.restart_server(&handle).await.map_err(|e| {
                    ToolError::ExecutionFailed(format!(
                        "MCP server '{server_name}' restart failed: {e}"
                    ))
                })?;
            }
            ServerState::Stopped => {
                return Err(ToolError::ExecutionFailed(format!(
                    "MCP server '{server_name}' is stopped",
                )));
            }
            ServerState::Starting => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        match handle
            .call_tool_with_progress(tool_name, arguments, Some(on_progress))
            .await
        {
            Ok(output) => Ok(output),
            Err(e) => {
                *handle.state.write().await = ServerState::Unhealthy(e.to_string());
                Err(e)
            }
        }
    }

    /// Restart an MCP server with exponential backoff.
    async fn restart_server(&self, handle: &McpServerHandle) -> Result<(), String> {
        let restarts = handle.restart_count.fetch_add(1, Ordering::Relaxed) as u32;
        if restarts >= handle.max_restarts {
            *handle.state.write().await = ServerState::Stopped;
            return Err(format!(
                "MCP server '{}' exceeded max restarts ({})",
                handle.name, handle.max_restarts
            ));
        }

        // Shutdown existing process
        handle.shutdown().await;

        // Exponential backoff: 1s, 2s, 4s, 8s, 16s
        let backoff = Duration::from_secs(1 << restarts.min(4));
        info!(
            server = %handle.name,
            attempt = restarts + 1,
            backoff_secs = backoff.as_secs(),
            "Restarting MCP server with backoff"
        );
        tokio::time::sleep(backoff).await;

        handle.start().await
    }

    /// Send a ping to a specific server.
    pub async fn ping(&self, server_name: &str) -> Result<(), String> {
        let handle = self
            .handles
            .get(server_name)
            .ok_or_else(|| format!("MCP server '{server_name}' not in pool"))?;
        handle.ping().await
    }

    /// List prompts from a specific server via `prompts/list`.
    pub async fn list_prompts(
        &self,
        server_name: &str,
    ) -> Result<Vec<crate::Prompt>, String> {
        let handle = self
            .handles
            .get(server_name)
            .ok_or_else(|| format!("MCP server '{server_name}' not in pool"))?;
        let response = handle.send_request("prompts/list", serde_json::json!({})).await?;
        let prompts_value = response
            .get("result")
            .and_then(|r| r.get("prompts"))
            .cloned()
            .unwrap_or(serde_json::json!([]));
        serde_json::from_value(prompts_value)
            .map_err(|e| format!("Failed to parse prompts list: {e}"))
    }

    /// List prompts from all connected servers.
    pub async fn list_all_prompts(&self) -> Vec<(String, Vec<crate::Prompt>)> {
        let mut result = Vec::new();
        for entry in self.handles.iter() {
            let name = entry.key().clone();
            let handle = entry.value();
            if let Ok(response) = handle
                .send_request("prompts/list", serde_json::json!({}))
                .await
            {
                let prompts_value = response
                    .get("result")
                    .and_then(|r| r.get("prompts"))
                    .cloned()
                    .unwrap_or(serde_json::json!([]));
                if let Ok(parsed) = serde_json::from_value(prompts_value) {
                    result.push((name, parsed));
                }
            }
        }
        result
    }

    /// Get a prompt from a specific server via `prompts/get`.
    pub async fn get_prompt(
        &self,
        server_name: &str,
        prompt_name: &str,
        arguments: Option<std::collections::HashMap<String, String>>,
    ) -> Result<Value, String> {
        let handle = self
            .handles
            .get(server_name)
            .ok_or_else(|| format!("MCP server '{server_name}' not in pool"))?;
        let params = serde_json::json!({
            "name": prompt_name,
            "arguments": arguments.unwrap_or_default(),
        });
        let response = handle.send_request("prompts/get", params).await?;
        response.get("result").cloned().ok_or_else(|| {
            format!("MCP server '{server_name}' returned no result for prompt '{prompt_name}'")
        })
    }

    /// Get the state of a specific server.
    pub async fn server_state(&self, server_name: &str) -> Option<ServerState> {
        let handle = self.handles.get(server_name)?;
        Some(handle.get_state().await)
    }

    /// Get detailed status of a specific server, including metrics.
    pub async fn server_status(&self, server_name: &str) -> Option<ServerStatus> {
        let handle = self.handles.get(server_name)?;
        Some(handle.get_status().await)
    }

    /// List all server names and their states.
    pub async fn list_servers(&self) -> Vec<(String, ServerState)> {
        let mut result = Vec::new();
        for entry in self.handles.iter() {
            let state = entry.value().get_state().await;
            result.push((entry.key().clone(), state));
        }
        result
    }

    /// Start background health checks for all servers.
    ///
    /// Periodically pings each server. On failure, marks the server as
    /// unhealthy and attempts an automatic restart with exponential backoff.
    /// The task handle is stored so it can be cancelled via [`stop_health_checks`].
    pub async fn start_health_checks(&self) {
        let handles = self.handles.clone();
        let interval = self.health_interval;
        let max_restarts = self.max_restarts;

        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                for entry in handles.iter() {
                    let name = entry.key().clone();
                    let handle = entry.value();

                    match handle.ping().await {
                        Ok(()) => {
                            debug!(server = %name, "Health check passed");
                        }
                        Err(e) => {
                            warn!(server = %name, error = %e, "Health check failed");

                            // Attempt automatic restart if under the limit.
                            let restarts =
                                handle.restart_count.fetch_add(1, Ordering::Relaxed) as u32;
                            if restarts < max_restarts {
                                handle.shutdown().await;
                                let backoff = Duration::from_secs(1 << restarts.min(4));
                                info!(
                                    server = %name,
                                    attempt = restarts + 1,
                                    backoff_secs = backoff.as_secs(),
                                    "Auto-restarting unhealthy server"
                                );
                                tokio::time::sleep(backoff).await;
                                if let Err(e) = handle.start().await {
                                    warn!(server = %name, error = %e, "Auto-restart failed");
                                }
                            } else {
                                warn!(
                                    server = %name,
                                    max = max_restarts,
                                    "Exceeded max restarts, stopping"
                                );
                                *handle.state.write().await = ServerState::Stopped;
                            }
                        }
                    }
                }
            }
        });

        // Store the task handle so it can be cancelled later.
        let mut guard = self.health_task.lock().await;
        // Abort any previous health task.
        if let Some(prev) = guard.take() {
            prev.abort();
        }
        *guard = Some(task);
    }

    /// Stop the background health check task.
    pub async fn stop_health_checks(&self) {
        let mut guard = self.health_task.lock().await;
        if let Some(task) = guard.take() {
            task.abort();
            info!("Health check task stopped");
        }
    }

    /// Set a callback that is invoked when a server reports
    /// `notifications/tools/list_changed`.
    ///
    /// The callback receives the server name as its argument.
    pub async fn set_on_tools_changed(&self, callback: Arc<dyn Fn(&str) + Send + Sync>) {
        *self.on_tools_changed.lock().await = Some(callback);
    }

    /// Set the roots provider callback.
    ///
    /// When a server sends a `roots/list` request, this callback is invoked
    /// to obtain the filesystem roots to return. If not set, an empty list
    /// is returned.
    pub async fn set_roots_provider(&self, provider: Arc<dyn Fn() -> Vec<crate::Root> + Send + Sync>) {
        *self.roots_provider.lock().await = Some(provider);
    }

    /// Get the current roots from the provider, or an empty list if none set.
    pub async fn get_roots(&self) -> Vec<crate::Root> {
        let guard = self.roots_provider.lock().await;
        match guard.as_ref() {
            Some(provider) => provider(),
            None => Vec::new(),
        }
    }

    /// Set the sampling provider for handling `sampling/createMessage` requests.
    ///
    /// The provider receives a `CreateMessageRequest` and returns a
    /// `CreateMessageResult` (or an error string). This is typically wired to
    /// the application's LLM client. If no provider is set, servers receive a
    /// "method not found" error when they attempt sampling.
    pub async fn set_sampling_provider(&self, provider: SamplingProvider) {
        *self.sampling_provider.lock().await = Some(provider);
    }

    /// Start a background task that listens for server notifications
    /// and dispatches them to the appropriate callback.
    ///
    /// Currently handles:
    /// - `notifications/tools/list_changed` → calls `on_tools_changed` callback
    pub fn start_notification_handler(&self) {
        let rx = self.notification_rx.clone();
        let on_tools_changed = self.on_tools_changed.clone();

        tokio::spawn(async move {
            loop {
                let mut rx_guard = rx.lock().await;
                match rx_guard.recv().await {
                    Some((server_name, notification)) => {
                        drop(rx_guard);

                        let method = notification
                            .get("method")
                            .and_then(|m| m.as_str())
                            .unwrap_or("");

                        match method {
                            "notifications/tools/list_changed" => {
                                info!(
                                    server = %server_name,
                                    "Received tools/list_changed notification"
                                );
                                let guard = on_tools_changed.lock().await;
                                if let Some(ref cb) = *guard {
                                    cb(&server_name);
                                }
                            }
                            other => {
                                debug!(
                                    server = %server_name,
                                    method = %other,
                                    "Unhandled notification"
                                );
                            }
                        }
                    }
                    None => {
                        // Channel closed, exit the loop.
                        break;
                    }
                }
            }
        });
    }

    /// Gracefully shut down all servers.
    pub async fn shutdown_all(&self) {
        info!("Shutting down all MCP servers");
        for entry in self.handles.iter() {
            entry.value().shutdown().await;
        }
        self.handles.clear();
    }

    /// Get the number of servers in the pool.
    pub fn server_count(&self) -> usize {
        self.handles.len()
    }
}

impl Default for McpProcessPool {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Pooled MCP Tool Adapter
// ---------------------------------------------------------------------------

/// A tool adapter that routes calls through the persistent process pool.
///
/// Unlike `McpToolAdapter` (which spawns a fresh process per call),
/// this adapter uses the pool's persistent connections for zero-overhead
/// tool execution after initial startup.
pub struct PooledMcpToolAdapter {
    /// Shared reference to the process pool.
    pool: Arc<McpProcessPool>,
    /// Server name in the pool.
    server_name: String,
    /// Tool name on the MCP server side.
    remote_tool_name: String,
    /// Human-readable description.
    description: String,
    /// JSON Schema for tool input.
    input_schema: Value,
    /// Tool name in the registry (e.g., "mcp__fetch__fetch").
    tool_name: String,
    /// Behavioral hints from the MCP server (readOnly, destructive, etc.).
    annotations: Option<crate::ToolAnnotations>,
}

impl PooledMcpToolAdapter {
    /// Create a new pooled tool adapter.
    pub fn new(
        pool: Arc<McpProcessPool>,
        server_name: String,
        remote_tool_name: String,
        description: String,
        input_schema: Value,
        annotations: Option<crate::ToolAnnotations>,
    ) -> Self {
        let tool_name = format!("mcp__{server_name}__{remote_tool_name}");
        // Truncate oversized descriptions to avoid wasting context tokens.
        let description = if description.len() > MAX_TOOL_DESCRIPTION_CHARS {
            let mut end = MAX_TOOL_DESCRIPTION_CHARS;
            while !description.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            format!("{}…", &description[..end])
        } else {
            description
        };
        Self {
            pool,
            server_name,
            remote_tool_name,
            description,
            input_schema,
            tool_name,
            annotations,
        }
    }
}

impl std::fmt::Debug for PooledMcpToolAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledMcpToolAdapter")
            .field("server_name", &self.server_name)
            .field("tool_name", &self.tool_name)
            .finish()
    }
}

#[async_trait]
impl Tool for PooledMcpToolAdapter {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        self.pool
            .call_tool(&self.server_name, &self.remote_tool_name, input)
            .await
    }

    fn requires_auth(&self) -> bool {
        false
    }

    fn category(&self) -> &str {
        "mcp"
    }

    fn is_read_only(&self) -> bool {
        self.annotations
            .as_ref()
            .map_or(false, |a| a.read_only_hint)
    }

    fn is_concurrency_safe(&self) -> bool {
        // Idempotent or read-only tools are safe to run concurrently.
        self.annotations
            .as_ref()
            .map_or(false, |a| a.read_only_hint || a.idempotent_hint)
    }

    fn is_destructive(&self) -> bool {
        self.annotations
            .as_ref()
            .map_or(false, |a| a.destructive_hint)
    }
}

// ---------------------------------------------------------------------------
// Discovery via pool
// ---------------------------------------------------------------------------

/// Result of discovering tools via the persistent pool.
pub struct PooledDiscoveryResult {
    /// Server name.
    pub server_name: String,
    /// Tool adapters ready to register.
    pub tools: Vec<PooledMcpToolAdapter>,
}

/// Discover tools from an MCP server using the persistent pool.
///
/// Starts the server in the pool, sends `initialize` + `tools/list`,
/// and returns pooled adapters for each discovered tool.
pub async fn discover_pooled_tools(
    pool: Arc<McpProcessPool>,
    server_name: &str,
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> Result<PooledDiscoveryResult, String> {
    // Start the server in the pool (handles initialize handshake)
    pool.start_server(server_name, command, args, env).await?;

    // Now send tools/list via the pool's persistent connection
    let handle = pool
        .handles
        .get(server_name)
        .ok_or_else(|| format!("Server '{server_name}' not found after start"))?;

    let response = handle
        .send_request("tools/list", serde_json::json!({}))
        .await?;

    let mut tools = Vec::new();

    if let Some(tools_array) = response
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
    {
        for tool_value in tools_array {
            let name = tool_value
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();
            let description = tool_value
                .get("description")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("MCP tool: {name}"));
            let input_schema = tool_value
                .get("inputSchema")
                .cloned()
                .unwrap_or(serde_json::json!({"type": "object"}));

            // Parse tool annotations (behavioral hints) if present.
            let annotations: Option<crate::ToolAnnotations> = tool_value
                .get("annotations")
                .and_then(|a| serde_json::from_value(a.clone()).ok());

            tools.push(PooledMcpToolAdapter::new(
                pool.clone(),
                server_name.to_string(),
                name,
                description,
                input_schema,
                annotations,
            ));
        }
    }

    drop(handle);

    Ok(PooledDiscoveryResult {
        server_name: server_name.to_string(),
        tools,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pooled_tool_adapter_name() {
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "test-server".to_string(),
            "fetch".to_string(),
            "Fetch a URL".to_string(),
            serde_json::json!({"type": "object"}),
            None,
        );
        assert_eq!(adapter.name(), "mcp__test-server__fetch");
    }

    #[test]
    fn test_pooled_tool_adapter_description() {
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "search".to_string(),
            "Search the web".to_string(),
            serde_json::json!({"type": "object"}),
            None,
        );
        assert_eq!(adapter.description(), "Search the web");
    }

    #[test]
    fn test_tool_description_truncation() {
        let long_desc = "x".repeat(5000);
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "tool".to_string(),
            long_desc,
            serde_json::json!({"type": "object"}),
            None,
        );
        let desc = adapter.description();
        assert!(desc.chars().count() <= MAX_TOOL_DESCRIPTION_CHARS + 1, "description should be truncated to ~2048 chars");
        assert!(desc.ends_with('…'));
    }

    #[test]
    fn test_tool_description_short_not_truncated() {
        let short_desc = "A short description".to_string();
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "tool".to_string(),
            short_desc.clone(),
            serde_json::json!({"type": "object"}),
            None,
        );
        assert_eq!(adapter.description(), short_desc);
    }

    #[test]
    fn test_tool_result_not_truncated_under_limit() {
        let content = "x".repeat(100);
        let result = truncate_tool_result(&content);
        assert_eq!(result, content);
    }

    #[test]
    fn test_tool_result_truncated_plain_text() {
        let content = "line\n".repeat(10_000); // 50,000 chars
        let result = truncate_tool_result(&content);
        assert!(result.len() <= MAX_TOOL_RESULT_CHARS + 200); // +200 for notice
        assert!(result.contains("[...truncated:"));
        assert!(result.contains("50"));
        assert!(result.contains("chars"));
        // Should cut at a newline boundary
        assert!(result.lines().count() < 10_000);
    }

    #[test]
    fn test_tool_result_truncated_json() {
        let items: Vec<String> = (0..5000).map(|i| format!(r#"{{"id": {}, "data": "item {}"}}"#, i, i)).collect();
        let content = format!("[{}]", items.join(",\n"));
        let result = truncate_tool_result(&content);
        assert!(result.len() <= MAX_TOOL_RESULT_CHARS + 200);
        assert!(result.contains("[...truncated:"));
    }

    #[test]
    fn test_tool_result_truncation_preserves_unicode() {
        // String with multi-byte chars
        let content = "日本語テスト\n".repeat(10_000);
        let result = truncate_tool_result(&content);
        // Should not panic on char boundary
        assert!(result.contains("[...truncated:"));
    }

    #[test]
    fn test_normalize_error_content_text_only() {
        let blocks = vec![
            serde_json::json!({"type": "text", "text": "Error: file not found"}),
        ];
        assert_eq!(normalize_error_content(&blocks), "Error: file not found");
    }

    #[test]
    fn test_normalize_error_content_multi_text() {
        let blocks = vec![
            serde_json::json!({"type": "text", "text": "Error: connection failed"}),
            serde_json::json!({"type": "text", "text": "Retry after 30 seconds"}),
        ];
        assert_eq!(
            normalize_error_content(&blocks),
            "Error: connection failed\nRetry after 30 seconds"
        );
    }

    #[test]
    fn test_normalize_error_content_with_image() {
        let blocks = vec![
            serde_json::json!({"type": "text", "text": "Screenshot of error:"}),
            serde_json::json!({"type": "image", "mimeType": "image/png", "data": "..."}),
        ];
        let result = normalize_error_content(&blocks);
        assert!(result.contains("Screenshot of error:"));
        assert!(result.contains("[image/png image]"));
    }

    #[test]
    fn test_normalize_error_content_with_resource() {
        let blocks = vec![
            serde_json::json!({"type": "text", "text": "Server returned:"}),
            serde_json::json!({
                "type": "resource",
                "resource": {
                    "uri": "file:///var/log/error.log",
                    "mimeType": "text/plain",
                    "text": "Stack trace here"
                }
            }),
        ];
        let result = normalize_error_content(&blocks);
        assert!(result.contains("[resource: file:///var/log/error.log]"));
        assert!(result.contains("Stack trace here"));
    }

    #[test]
    fn test_normalize_error_content_empty_blocks() {
        let blocks: Vec<serde_json::Value> = vec![];
        assert_eq!(normalize_error_content(&blocks), "");
    }

    #[test]
    fn test_normalize_error_content_unknown_type() {
        let blocks = vec![
            serde_json::json!({"type": "audio", "data": "..."}),
        ];
        let result = normalize_error_content(&blocks);
        assert_eq!(result, "[audio block]");
    }

    #[test]
    fn test_pooled_tool_adapter_category() {
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "tool".to_string(),
            "desc".to_string(),
            serde_json::json!({"type": "object"}),
            None,
        );
        assert_eq!(adapter.category(), "mcp");
    }

    #[test]
    fn test_pooled_tool_adapter_debug() {
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "test".to_string(),
            "tool".to_string(),
            "desc".to_string(),
            serde_json::json!({"type": "object"}),
            None,
        );
        let debug_str = format!("{adapter:?}");
        assert!(debug_str.contains("test"));
        assert!(debug_str.contains("mcp__test"));
    }

    #[test]
    fn test_pool_default() {
        let pool = McpProcessPool::new();
        assert_eq!(pool.server_count(), 0);
    }

    #[tokio::test]
    async fn test_pool_list_servers_empty() {
        let pool = McpProcessPool::new();
        let servers = pool.list_servers().await;
        assert!(servers.is_empty());
    }

    #[tokio::test]
    async fn test_pool_call_tool_not_found() {
        let pool = McpProcessPool::new();
        let result = pool
            .call_tool("nonexistent", "tool", serde_json::json!({}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_pool_ping_not_found() {
        let pool = McpProcessPool::new();
        let result = pool.ping("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_pool_server_state_not_found() {
        let pool = McpProcessPool::new();
        let state = pool.server_state("nonexistent").await;
        assert!(state.is_none());
    }

    #[test]
    fn test_tool_annotations_read_only() {
        let pool = Arc::new(McpProcessPool::new());
        let annotations = crate::ToolAnnotations {
            read_only_hint: true,
            ..Default::default()
        };
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "read_tool".to_string(),
            "Read-only tool".to_string(),
            serde_json::json!({"type": "object"}),
            Some(annotations),
        );
        assert!(adapter.is_read_only());
        assert!(adapter.is_concurrency_safe());
    }

    #[test]
    fn test_tool_annotations_destructive() {
        let pool = Arc::new(McpProcessPool::new());
        let annotations = crate::ToolAnnotations {
            destructive_hint: true,
            ..Default::default()
        };
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "delete_tool".to_string(),
            "Destructive tool".to_string(),
            serde_json::json!({"type": "object"}),
            Some(annotations),
        );
        assert!(!adapter.is_read_only());
        assert!(!adapter.is_concurrency_safe());
    }

    #[test]
    fn test_tool_annotations_idempotent() {
        let pool = Arc::new(McpProcessPool::new());
        let annotations = crate::ToolAnnotations {
            idempotent_hint: true,
            ..Default::default()
        };
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "cache_tool".to_string(),
            "Idempotent tool".to_string(),
            serde_json::json!({"type": "object"}),
            Some(annotations),
        );
        assert!(!adapter.is_read_only());
        assert!(adapter.is_concurrency_safe());
    }

    #[test]
    fn test_tool_annotations_none() {
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "basic_tool".to_string(),
            "Basic tool".to_string(),
            serde_json::json!({"type": "object"}),
            None,
        );
        assert!(!adapter.is_read_only());
        assert!(!adapter.is_concurrency_safe());
    }

    #[tokio::test]
    async fn test_server_status_not_found() {
        let pool = McpProcessPool::new();
        let status = pool.server_status("nonexistent").await;
        assert!(status.is_none());
    }

    #[tokio::test]
    async fn test_pool_stop_health_checks_noop() {
        // Stopping when never started should not panic.
        let pool = McpProcessPool::new();
        pool.stop_health_checks().await;
        assert_eq!(pool.server_count(), 0);
    }

    #[tokio::test]
    async fn test_notification_channel_forwarding() {
        let pool = McpProcessPool::new();

        // Set up a callback that records which server changed.
        // Use std::sync::Mutex since the callback is sync (not async).
        let changed = Arc::new(std::sync::Mutex::new(None::<String>));
        let changed_clone = changed.clone();
        pool.set_on_tools_changed(Arc::new(move |server_name| {
            *changed_clone.lock().unwrap() = Some(server_name.to_string());
        }))
        .await;

        // Start the notification handler.
        pool.start_notification_handler();

        // Simulate a notification from a server by sending through the channel.
        pool.notification_tx
            .send((
                "test-server".to_string(),
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/tools/list_changed"
                }),
            ))
            .unwrap();

        // Give the handler time to process.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let guard = changed.lock().unwrap();
        assert_eq!(guard.as_deref(), Some("test-server"));
    }

    #[test]
    fn test_progress_notification_deserialization() {
        let json = serde_json::json!({
            "progressToken": "pg-42",
            "progress": 50.0,
            "total": 100.0
        });
        let notif: crate::ProgressNotification =
            serde_json::from_value(json).unwrap();
        assert_eq!(notif.progress_token, serde_json::json!("pg-42"));
        assert_eq!(notif.progress, 50.0);
        assert_eq!(notif.total, Some(100.0));
    }

    #[test]
    fn test_progress_notification_without_total() {
        let json = serde_json::json!({
            "progressToken": 7,
            "progress": 3.0
        });
        let notif: crate::ProgressNotification =
            serde_json::from_value(json).unwrap();
        assert_eq!(notif.progress_token, serde_json::json!(7));
        assert_eq!(notif.progress, 3.0);
        assert_eq!(notif.total, None);
    }

    #[tokio::test]
    async fn test_progress_callback_routing() {
        use dashmap::DashMap;

        let pending: Arc<DashMap<u64, PendingRequest>> =
            Arc::new(DashMap::new());
        let (tx, _rx): (tokio::sync::mpsc::UnboundedSender<(String, Value)>, _) =
            tokio::sync::mpsc::unbounded_channel();

        let progress_reports = Arc::new(std::sync::Mutex::new(Vec::<(f64, Option<f64>)>::new()));
        let reports_clone = progress_reports.clone();

        // Insert a pending request with a progress token and callback.
        let (oneshot_tx, _oneshot_rx) = tokio::sync::oneshot::channel();
        pending.insert(
            42,
            PendingRequest {
                tx: oneshot_tx,
                created_at: Instant::now(),
                progress_token: Some(serde_json::json!("pg-test")),
                on_progress: Some(Arc::new(move |progress, total| {
                    reports_clone.lock().unwrap().push((progress, total));
                })),
            },
        );

        // Simulate a progress notification line.
        let line = r#"{"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":"pg-test","progress":25.0,"total":100.0}}"#;
        let value: Value = serde_json::from_str(line).unwrap();

        // Replicate the routing logic from read_responses.
        if value.get("method").and_then(|m| m.as_str()) == Some("notifications/progress") {
            if let Some(token) = value.get("params").and_then(|p| p.get("progressToken")).cloned() {
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

        let reports = progress_reports.lock().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0], (25.0, Some(100.0)));

        // Verify pending request is still there (not removed by progress).
        assert!(pending.contains_key(&42));

        drop(tx);
    }

    #[test]
    fn test_root_serialization() {
        let root = crate::Root {
            uri: "file:///home/user/project".to_string(),
            name: Some("My Project".to_string()),
        };
        let json = serde_json::to_value(&root).unwrap();
        assert_eq!(json["uri"], "file:///home/user/project");
        assert_eq!(json["name"], "My Project");
    }

    #[test]
    fn test_root_without_name() {
        let root = crate::Root {
            uri: "file:///tmp".to_string(),
            name: None,
        };
        let json = serde_json::to_value(&root).unwrap();
        assert_eq!(json["uri"], "file:///tmp");
        assert!(json.get("name").is_none(), "name should be omitted when None");
    }

    #[test]
    fn test_list_roots_result_serialization() {
        let result = crate::ListRootsResult {
            roots: vec![
                crate::Root {
                    uri: "file:///a".to_string(),
                    name: Some("A".to_string()),
                },
                crate::Root {
                    uri: "file:///b".to_string(),
                    name: None,
                },
            ],
        };
        let json = serde_json::to_value(&result).unwrap();
        let roots = json["roots"].as_array().unwrap();
        assert_eq!(roots.len(), 2);
    }

    #[test]
    fn test_roots_capability_serialization() {
        let cap = crate::RootsCapability { list_changed: true };
        let json = serde_json::to_value(&cap).unwrap();
        assert_eq!(json["listChanged"], true);
    }

    #[tokio::test]
    async fn test_roots_provider_default_none() {
        let pool = McpProcessPool::new();
        let roots = pool.get_roots().await;
        assert!(roots.is_empty(), "default roots provider should return empty vec");
    }

    #[tokio::test]
    async fn test_set_and_get_roots() {
        let pool = McpProcessPool::new();
        pool.set_roots_provider(Arc::new(|| {
            vec![
                crate::Root {
                    uri: "file:///workspace".to_string(),
                    name: Some("Workspace".to_string()),
                },
            ]
        }))
        .await;

        let roots = pool.get_roots().await;
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].uri, "file:///workspace");
    }

    // -----------------------------------------------------------------------
    // Sampling tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_message_request_deserialization() {
        let json = serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": { "type": "text", "text": "Hello" }
                }
            ],
            "maxTokens": 100,
            "temperature": 0.7
        });
        let req: crate::CreateMessageRequest =
            serde_json::from_value(json).unwrap();
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.max_tokens, Some(100));
        assert_eq!(req.sampling_params.temperature, Some(0.7));
    }

    #[test]
    fn test_create_message_result_serialization() {
        let result = crate::CreateMessageResult {
            role: crate::SamplingMessageRole::Assistant,
            model: "test-model".to_string(),
            content: crate::SamplingContent::Text {
                text: "Hi there!".to_string(),
            },
            stop_reason: Some(crate::StopReason::EndTurn),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["role"], "assistant");
        assert_eq!(json["model"], "test-model");
        assert_eq!(json["stopReason"], "endTurn");
    }

    #[test]
    fn test_sampling_message_role_serialization() {
        let user = serde_json::to_value(&crate::SamplingMessageRole::User).unwrap();
        assert_eq!(user, "user");
        let assistant = serde_json::to_value(&crate::SamplingMessageRole::Assistant).unwrap();
        assert_eq!(assistant, "assistant");
    }

    #[test]
    fn test_model_preferences_deserialization() {
        let json = serde_json::json!({
            "hints": [{ "name": "claude-3" }],
            "costPriority": 0.5,
            "speedPriority": 0.8,
            "intelligencePriority": 0.9
        });
        let prefs: crate::ModelPreferences = serde_json::from_value(json).unwrap();
        assert_eq!(prefs.hints.as_ref().unwrap().len(), 1);
        assert_eq!(prefs.cost_priority, Some(0.5));
        assert_eq!(prefs.speed_priority, Some(0.8));
        assert_eq!(prefs.intelligence_priority, Some(0.9));
    }

    #[tokio::test]
    async fn test_sampling_provider_default_none() {
        let pool = McpProcessPool::new();
        let guard = pool.sampling_provider.lock().await;
        assert!(guard.is_none(), "default sampling provider should be None");
    }

    #[tokio::test]
    async fn test_set_sampling_provider() {
        let pool = McpProcessPool::new();
        pool.set_sampling_provider(Arc::new(|req| {
            Box::pin(async move {
                Ok(crate::CreateMessageResult {
                    role: crate::SamplingMessageRole::Assistant,
                    model: "mock".to_string(),
                    content: crate::SamplingContent::Text {
                        text: format!("Echo: {} messages", req.messages.len()),
                    },
                    stop_reason: Some(crate::StopReason::EndTurn),
                })
            })
        }))
        .await;

        let guard = pool.sampling_provider.lock().await;
        assert!(guard.is_some());

        // Call the provider to verify it works.
        let provider = guard.as_ref().unwrap();
        let req = crate::CreateMessageRequest {
            messages: vec![crate::SamplingMessage {
                role: crate::SamplingMessageRole::User,
                content: crate::SamplingContent::Text {
                    text: "test".to_string(),
                },
            }],
            model_preferences: None,
            system_prompt: None,
            max_tokens: Some(50),
            sampling_params: crate::SamplingParams::default(),
        };
        let result = provider(req).await.unwrap();
        assert_eq!(result.model, "mock");
    }
}
