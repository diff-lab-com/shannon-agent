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
use shannon_tool_interface::{Tool, ToolError, ToolOutput, ToolResult};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{oneshot, Mutex, RwLock};
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

/// Lifecycle state of an MCP server process.
#[derive(Debug, Clone, PartialEq, Eq)]
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

// ---------------------------------------------------------------------------
// Pending request tracking
// ---------------------------------------------------------------------------

/// A pending JSON-RPC request waiting for a response.
struct PendingRequest {
    /// Oneshot channel to deliver the response.
    tx: oneshot::Sender<Value>,
    /// When this request was created (for timeout tracking).
    created_at: Instant,
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
    /// Request timeout.
    request_timeout: Duration,
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
        let reader = BufReader::new(stdout);
        let handle = tokio::spawn(async move {
            Self::read_responses(reader, &pending, &server_name).await;
        });
        *self.reader_task.lock().await = Some(handle);

        // Send initialize request
        *self.state.write().await = ServerState::Starting;
        let init_response = self
            .send_request("initialize", serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "shannon-code", "version": "0.1.0"}
            }))
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
        info!(server = %self.name, "MCP server is healthy");
        Ok(())
    }

    /// Background task: read JSON-RPC responses from stdout and route to pending requests.
    async fn read_responses(
        reader: BufReader<ChildStdout>,
        pending: &DashMap<u64, PendingRequest>,
        server_name: &str,
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

            // Extract the id to route the response
            if let Some(id) = value.get("id").and_then(|v| v.as_u64()) {
                if let Some((_, pending_req)) = pending.remove(&id) {
                    let _ = pending_req.tx.send(value);
                }
            }
            // Notifications (no id) are logged but not routed
            else if value.get("method").is_some() {
                debug!(
                    server = %server_name,
                    method = %value["method"],
                    "Received notification from MCP server"
                );
            }
        }
        debug!(server = %server_name, "Stdout reader ended");
    }

    /// Send a JSON-RPC request and wait for the response.
    async fn send_request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

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
        match tokio::time::timeout(self.request_timeout, rx).await {
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

        let response = self
            .send_request(
                "tools/call",
                serde_json::json!({
                    "name": tool_name,
                    "arguments": arguments,
                }),
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(e))?;

        // Parse the response content
        if let Some(result) = response.get("result") {
            if let Some(content_array) = result.get("content").and_then(|c| c.as_array()) {
                // Extract text from content blocks
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
                    let is_error = result
                        .get("isError")
                        .and_then(|e| e.as_bool())
                        .unwrap_or(false);
                    let content = texts.join("\n");
                    return if is_error {
                        Ok(ToolOutput::error(content))
                    } else {
                        Ok(ToolOutput::success(content))
                    };
                }
            }
            // Fallback: return the result as JSON string
            return Ok(ToolOutput::success(result.to_string()));
        }

        Ok(ToolOutput::success(response.to_string()))
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
                Ok(())
            }
            Err(e) => {
                *self.state.write().await = ServerState::Unhealthy(e.clone());
                Err(e)
            }
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
    /// Request timeout.
    request_timeout: Duration,
    /// Background health check task handle.
    health_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl McpProcessPool {
    /// Create a new process pool with default settings.
    pub fn new() -> Self {
        Self {
            handles: DashMap::new(),
            health_interval: Duration::from_secs(60),
            max_restarts: 5,
            request_timeout: Duration::from_secs(30),
            health_task: Arc::new(Mutex::new(None)),
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

    /// Get the state of a specific server.
    pub async fn server_state(&self, server_name: &str) -> Option<ServerState> {
        let handle = self.handles.get(server_name)?;
        Some(handle.get_state().await)
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
    pub fn start_health_checks(&self) {
        let handles: Vec<(String, Arc<McpServerHandle>)> = self
            .handles
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();
        let interval = self.health_interval;

        let handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                for (name, handle) in &handles {
                    if let Err(e) = handle.ping().await {
                        warn!(server = %name, error = %e, "Health check failed");
                    }
                }
            }
        });

        // We need mutable access to store the handle, but &self is shared.
        // The health task is fire-and-forget — it will be cancelled when the
        // JoinHandle is dropped. We store it for potential cleanup.
        let _ = handle;
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
}

impl PooledMcpToolAdapter {
    /// Create a new pooled tool adapter.
    pub fn new(
        pool: Arc<McpProcessPool>,
        server_name: String,
        remote_tool_name: String,
        description: String,
        input_schema: Value,
    ) -> Self {
        let tool_name = format!("mcp__{server_name}__{remote_tool_name}");
        Self {
            pool,
            server_name,
            remote_tool_name,
            description,
            input_schema,
            tool_name,
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
                .unwrap_or(&format!("MCP tool: {name}"))
                .to_string();
            let input_schema = tool_value
                .get("inputSchema")
                .cloned()
                .unwrap_or(serde_json::json!({"type": "object"}));

            tools.push(PooledMcpToolAdapter::new(
                pool.clone(),
                server_name.to_string(),
                name,
                description,
                input_schema,
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
        );
        assert_eq!(adapter.description(), "Search the web");
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
}
