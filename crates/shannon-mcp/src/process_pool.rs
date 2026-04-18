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
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{oneshot, Mutex, RwLock};
use tracing::{debug, info, warn};
use crate::auth::{AuthProvider, OAuth2Provider, discover_oauth_endpoints};
use crate::config::{McpAuthConfig, HeaderSource};

/// Type alias for the async sampling callback.
///
/// Takes a `CreateMessageRequest` and returns a `CreateMessageResult`.
pub(crate) type SamplingProvider = Arc<
    dyn Fn(
            crate::CreateMessageRequest,
        ) -> Pin<Box<dyn Future<Output = Result<crate::CreateMessageResult, String>> + Send>>
        + Send
        + Sync,
>;

/// Provider for elicitation requests (server → client user prompts).
///
/// Takes an [`ElicitationRequest`] and returns an [`ElicitationResult`].
/// Typically wired to a TUI prompt or auto-declined in non-interactive mode.
pub(crate) type ElicitationProvider = Arc<
    dyn Fn(
            crate::ElicitationRequest,
        ) -> Pin<Box<dyn Future<Output = Result<crate::ElicitationResult, String>> + Send>>
        + Send
        + Sync,
>;

// ---------------------------------------------------------------------------
// Tool permission helpers
// ---------------------------------------------------------------------------

/// Simple glob pattern matching supporting `*` (any chars) and `?` (single char).
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_match_impl(&p, &t, 0, 0)
}

fn glob_match_impl(p: &[char], t: &[char], pi: usize, ti: usize) -> bool {
    if pi == p.len() {
        return ti == t.len();
    }
    if p[pi] == '*' {
        // '*' matches zero or more characters
        return glob_match_impl(p, t, pi + 1, ti) // match zero chars
            || (ti < t.len() && glob_match_impl(p, t, pi, ti + 1)); // consume one char
    }
    if ti < t.len() && (p[pi] == '?' || p[pi] == t[ti]) {
        return glob_match_impl(p, t, pi + 1, ti + 1);
    }
    false
}

/// Check whether a tool name is permitted by the given allow/deny patterns.
///
/// Pattern syntax:
/// - `mcp__fetch__*`  — allow all tools from the `fetch` server
/// - `!mcp__internal__*` — deny all tools from the `internal` server
/// - Empty patterns list → everything is allowed (default)
fn is_tool_allowed_by_patterns(tool_name: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return true; // No restrictions configured → allow all
    }

    let mut has_allow_patterns = false;
    let mut denied = false;
    let mut explicitly_allowed = false;

    for pattern in patterns {
        if let Some(deny_pattern) = pattern.strip_prefix('!') {
            // Deny rule — check first
            if glob_match(deny_pattern, tool_name) {
                denied = true;
            }
        } else {
            has_allow_patterns = true;
            if glob_match(pattern, tool_name) {
                explicitly_allowed = true;
            }
        }
    }

    if denied {
        return false;
    }

    if has_allow_patterns {
        return explicitly_allowed;
    }

    // Only deny patterns were specified and this tool wasn't denied → allow
    true
}

// ---------------------------------------------------------------------------
// Tool result chunking store
// ---------------------------------------------------------------------------

/// Stores oversized tool results so they can be retrieved in chunks later.
///
/// When a tool result is compressed or truncated, the full content is stored
/// here with a unique chunk ID. The LLM can then request the full result
/// or the next chunk if needed.
struct ToolResultStore {
    /// Full results keyed by chunk ID.
    results: DashMap<String, StoredResult>,
    /// Maximum age for stored results (auto-evicted).
    max_age: Duration,
}

/// A stored tool result with metadata.
struct StoredResult {
    /// The full content.
    full_content: String,
    /// Tool name that produced this result.
    tool_name: String,
    /// When this result was stored.
    stored_at: Instant,
}

impl ToolResultStore {
    fn new() -> Self {
        Self {
            results: DashMap::new(),
            max_age: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Store a result and return its chunk ID.
    fn store(&self, tool_name: &str, full_content: String) -> String {
        let id = format!("chunk_{}", uuid::Uuid::new_v4().as_simple());
        self.results.insert(
            id.clone(),
            StoredResult {
                full_content,
                tool_name: tool_name.to_string(),
                stored_at: Instant::now(),
            },
        );
        id
    }

    /// Get the full content for a chunk ID.
    fn get_full(&self, chunk_id: &str) -> Option<(String, String)> {
        self.results.get(chunk_id).map(|r| {
            (r.tool_name.clone(), r.full_content.clone())
        })
    }

    /// Get a specific chunk (offset, length) of a stored result.
    fn get_chunk(&self, chunk_id: &str, offset: usize, max_chars: usize) -> Option<ChunkResult> {
        self.results.get(chunk_id).map(|r| {
            let content = &r.full_content;
            let total_len = content.len();
            if offset >= total_len {
                return ChunkResult {
                    content: String::new(),
                    offset,
                    total_len,
                    has_more: false,
                    tool_name: r.tool_name.clone(),
                };
            }
            // Find safe char boundary
            let mut end = (offset + max_chars).min(total_len);
            while !content.is_char_boundary(end) && end > offset {
                end -= 1;
            }
            let has_more = end < total_len;
            ChunkResult {
                content: content[offset..end].to_string(),
                offset: end,
                total_len,
                has_more,
                tool_name: r.tool_name.clone(),
            }
        })
    }

    /// Evict expired results.
    fn evict_expired(&self) {
        self.results.retain(|_, v| v.stored_at.elapsed() < self.max_age);
    }
}

/// Result of retrieving a chunk from the store.
pub struct ChunkResult {
    /// Content of this chunk.
    pub content: String,
    /// Byte offset for the next chunk request.
    pub offset: usize,
    /// Total byte length of the full stored result.
    pub total_len: usize,
    /// Whether more content remains after this chunk.
    pub has_more: bool,
    /// Tool name that produced this result.
    pub tool_name: String,
}

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

/// Compress a tool result string to fit within [`MAX_TOOL_RESULT_CHARS`].
///
/// Uses format-aware strategies instead of simple truncation:
/// - **JSON arrays**: show first N items + item count summary
/// - **JSON objects**: show all keys with truncated values
/// - **Stack traces / line-based text**: show first/last lines + line count
/// - **Long text**: paragraph-aware truncation
///
/// For content that is already within budget, returns it unchanged.
fn truncate_tool_result(content: &str) -> String {
    if content.len() <= MAX_TOOL_RESULT_CHARS {
        return content.to_string();
    }

    let original_len = content.len();
    let trimmed = content.trim();

    // Strategy 1: Try JSON-aware compression for JSON content.
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let compressed = compress_json(&value, MAX_TOOL_RESULT_CHARS);
            let pct = ((original_len - compressed.len()) as f64 / original_len as f64) * 100.0;
            return format!(
                "{}\n\n[compressed: showed ~{} of ~{} chars ({:.0}% omitted)]",
                compressed,
                compressed.len(),
                original_len,
                pct,
            );
        }
    }

    // Strategy 2: Line-based compression for structured text (stack traces, logs).
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() > 20 {
        let head_budget = MAX_TOOL_RESULT_CHARS / 2;
        let tail_budget = MAX_TOOL_RESULT_CHARS / 2;

        let mut head_lines = Vec::new();
        let mut head_len = 0;
        for line in &lines {
            if head_len + line.len() + 1 > head_budget {
                break;
            }
            head_lines.push(*line);
            head_len += line.len() + 1;
        }

        let mut tail_lines = Vec::new();
        let mut tail_len = 0;
        for line in lines.iter().rev() {
            if tail_len + line.len() + 1 > tail_budget {
                break;
            }
            tail_lines.push(*line);
            tail_len += line.len() + 1;
        }
        tail_lines.reverse();

        let omitted_lines = lines.len() - head_lines.len() - tail_lines.len();
        let head_text = head_lines.join("\n");
        let tail_text = tail_lines.join("\n");
        let pct = ((original_len - head_text.len() - tail_text.len()) as f64 / original_len as f64) * 100.0;

        return format!(
            "{}\n\n... [{} lines omitted] ...\n\n{}\n\n[compressed: showed ~{} of ~{} chars ({:.0}% omitted)]",
            head_text,
            omitted_lines,
            tail_text,
            head_text.len() + tail_text.len(),
            original_len,
            pct,
        );
    }

    // Strategy 3: Paragraph-aware truncation for prose text.
    let mut end = MAX_TOOL_RESULT_CHARS;
    while !content.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    // Try to cut at a paragraph boundary (double newline).
    let truncated = &content[..end];
    let cut = truncated
        .rfind("\n\n")
        .map(|pos| pos)
        .or_else(|| truncated.rfind('\n'))
        .unwrap_or(end);
    let cut = if content.is_char_boundary(cut) { cut } else { end };
    let pct = ((original_len - cut) as f64 / original_len as f64) * 100.0;
    format!(
        "{}\n\n[compressed: showed ~{} of ~{} chars ({:.0}% omitted)]",
        &content[..cut],
        cut,
        original_len,
        pct,
    )
}

/// Format-aware JSON compression.
///
/// - Arrays: show first N items + summary of remaining count.
/// - Objects: show all keys with truncated values.
/// - Primitives: pass through.
fn compress_json(value: &serde_json::Value, budget: usize) -> String {
    match value {
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return "[]".to_string();
            }
            // Determine how many items fit in budget.
            let mut result = String::from("[\n");
            let mut shown = 0;
            for item in items {
                let item_str = format!("  {},\n", serde_json::to_string(item).unwrap_or_default());
                if result.len() + item_str.len() + 50 > budget {
                    break;
                }
                result.push_str(&item_str);
                shown += 1;
            }
            let remaining = items.len() - shown;
            if remaining > 0 {
                result.push_str(&format!(
                    "  // ... {} more items\n",
                    remaining
                ));
            }
            result.push(']');
            result
        }
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                return "{}".to_string();
            }
            let mut result = String::from("{\n");
            let value_budget = 200; // max chars per value
            for (key, val) in map {
                let val_str = serde_json::to_string(val).unwrap_or_default();
                let display_val = if val_str.len() > value_budget {
                    let mut v_end = value_budget;
                    while !val_str.is_char_boundary(v_end) && v_end > 0 {
                        v_end -= 1;
                    }
                    format!("{}...\"", &val_str[..v_end])
                } else {
                    val_str
                };
                let line = format!("  \"{}\": {},\n", key, display_val);
                if result.len() + line.len() + 10 > budget {
                    result.push_str(&format!("  // ... {} more keys\n", map.len() - result.lines().count() + 1));
                    break;
                }
                result.push_str(&line);
            }
            result.push('}');
            result
        }
        _ => {
            let s = serde_json::to_string(value).unwrap_or_default();
            if s.len() <= budget {
                s
            } else {
                let mut end = budget;
                while !s.is_char_boundary(end) && end > 0 {
                    end -= 1;
                }
                format!("{}...", &s[..end])
            }
        }
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
    /// Provider for elicitation (used to respond to `elicitation/create` from servers).
    elicitation_provider: Arc<Mutex<Option<ElicitationProvider>>>,
    /// Capabilities advertised by the server during initialization.
    capabilities: Arc<RwLock<Option<crate::ServerCapabilities>>>,
    /// Negotiated protocol version.
    protocol_version: Arc<RwLock<String>>,
    /// Semaphore limiting concurrent tool calls to this server.
    concurrency_semaphore: Arc<tokio::sync::Semaphore>,
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
            Self::read_responses(reader, &pending, &server_name, &notification_tx, stdin_clone, roots_provider, sampling_provider, elicitation_provider).await;
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
                    if lower.contains("error") || lower.contains("fatal") || lower.contains("panic") {
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

        // Parse capabilities from init response.
        if let Some(result) = init_response.get("result") {
            if let Ok(caps) = serde_json::from_value::<crate::ServerCapabilities>(
                result.get("capabilities").cloned().unwrap_or(serde_json::json!({})),
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
    async fn read_responses(
        reader: BufReader<ChildStdout>,
        pending: &DashMap<u64, PendingRequest>,
        server_name: &str,
        notification_tx: &tokio::sync::mpsc::UnboundedSender<(String, Value)>,
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
            // Handle elicitation/create server→client request.
            else if value.get("method").and_then(|m| m.as_str()) == Some("elicitation/create")
                && value.get("id").is_some()
            {
                let req_id = value.get("id").cloned();
                let provider = elicitation_provider.lock().await;
                let response_value = if let Some(ref handler) = *provider {
                    let params = value.get("params").cloned().unwrap_or(serde_json::json!({}));
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
        let child_arc = self.child.clone();
        let name = self.name.clone();
        tokio::spawn(async move {
            let mut child_guard = child_arc.lock().await;
            // Take the child out of the option — we now own it.
            if let Some(mut child) = child_guard.take() {
                // Try waiting for graceful exit first
                match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
                    Ok(Ok(_status)) => {
                        debug!(server = %name, "MCP server exited gracefully");
                    }
                    _ => {
                        // Force kill and reap to prevent zombie
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        warn!(server = %name, "Force-killed MCP server process (zombie reaped)");
                    }
                }
            }
        });
    }

    /// Get the current state.
    async fn get_state(&self) -> ServerState {
        self.state.read().await.clone()
    }
}

// ---------------------------------------------------------------------------
// Remote Server Handle (HTTP/SSE transports)
// ---------------------------------------------------------------------------

/// Manages a remote MCP server connection via HTTP.
///
/// Unlike `McpServerHandle` (which manages a child process over stdio),
/// this handle sends JSON-RPC requests via HTTP POST and parses responses.
/// No process management, no background reader task, no pending request map.
struct RemoteMcpServerHandle {
    /// Server name (for logging).
    name: String,
    /// Server URL endpoint.
    url: String,
    /// HTTP client (reused for connection pooling).
    client: reqwest::Client,
    /// Extra headers to include in every request (e.g., auth).
    headers: HashMap<String, String>,
    /// Optional OAuth provider for dynamic Bearer token injection.
    auth_provider: Option<Arc<OAuth2Provider>>,
    /// Shell commands to execute for dynamic headers (name → command).
    header_commands: HashMap<String, String>,
    /// Current state.
    state: Arc<RwLock<ServerState>>,
    /// Capabilities advertised by the server during initialization.
    capabilities: Arc<RwLock<Option<crate::ServerCapabilities>>>,
    /// Negotiated protocol version.
    protocol_version: Arc<RwLock<String>>,
    /// Next JSON-RPC request id.
    next_id: AtomicU64,
    /// Total number of tool call requests.
    request_count: AtomicU64,
    /// Total number of failed tool calls.
    error_count: AtomicU64,
    /// How many times this server has been restarted (re-initialized).
    restart_count: Arc<AtomicU64>,
    /// Maximum restart attempts.
    max_restarts: u32,
    /// When the server was last started successfully.
    started_at: Arc<RwLock<Option<Instant>>>,
    /// Request timeout (for regular JSON-RPC requests).
    request_timeout: Duration,
    /// Tool call timeout (for tools/call).
    tool_timeout: Duration,
    /// Semaphore limiting concurrent tool calls to this server.
    concurrency_semaphore: Arc<tokio::sync::Semaphore>,
}

impl RemoteMcpServerHandle {
    /// Initialize the remote server: send `initialize` + `notifications/initialized`.
    async fn start(&self) -> Result<(), String> {
        *self.state.write().await = ServerState::Starting;

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
                result.get("capabilities").cloned().unwrap_or(serde_json::json!({})),
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

    /// Send a JSON-RPC request via HTTP POST and wait for the response.
    async fn send_request_with_timeout(
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

            // Auto-discovery: try RFC 9728/8414 OAuth metadata for helpful guidance.
            let discovery_hint = match discover_oauth_endpoints(&self.url).await {
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

        self.parse_jsonrpc_response(response).await
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
        // Resolve dynamic headers from shell commands.
        for (name, command) in &self.header_commands {
            let output = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(command.as_str())
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

        tokio::time::timeout(
            timeout,
            builder
                .header("Content-Type", "application/json")
                .json(&request)
                .send(),
        )
        .await
        .map_err(|_| format!("Remote MCP server '{}' request timed out after {:?}", self.name, timeout))?
        .map_err(|e| format!("Remote MCP server '{}' HTTP request failed: {}", self.name, e))
    }

    /// Parse a successful HTTP response as JSON-RPC.
    async fn parse_jsonrpc_response(
        &self,
        response: reqwest::Response,
    ) -> Result<Value, String> {

        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("Remote MCP server '{}' response parse error: {}", self.name, e))?;

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

    /// Send a JSON-RPC notification (no id, no response expected).
    async fn send_notification(&self, method: &str, params: Value) -> Result<(), String> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

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

    /// Call a tool on this remote server via `tools/call`.
    async fn call_tool(&self, tool_name: &str, arguments: Value) -> ToolResult<ToolOutput> {
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
                        let content = truncate_tool_result(&normalized);
                        return Ok(ToolOutput::error(content));
                    }
                } else {
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
            return Ok(ToolOutput::success(truncate_tool_result(&result.to_string())));
        }

        Ok(ToolOutput::success(truncate_tool_result(&response.to_string())))
    }

    /// Get the current state.
    async fn get_state(&self) -> ServerState {
        self.state.read().await.clone()
    }

    /// Get detailed status including metrics.
    async fn get_status(&self) -> ServerStatus {
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
        }
    }

    /// Reset state for restart.
    async fn reset(&self) {
        *self.state.write().await = ServerState::Stopped;
        *self.started_at.write().await = None;
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
    /// Server handles keyed by server name (stdio transport).
    handles: DashMap<String, Arc<McpServerHandle>>,
    /// Remote server handles keyed by server name (HTTP/SSE transport).
    remote_handles: DashMap<String, Arc<RemoteMcpServerHandle>>,
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
    /// Provider for elicitation. Called when a server sends `elicitation/create`.
    elicitation_provider: Arc<Mutex<Option<ElicitationProvider>>>,
    /// TTL-based cache for read-only tool results.
    tool_cache: Arc<RwLock<HashMap<String, (String, Instant)>>>,
    /// TTL for cached tool results (default: 60 seconds).
    cache_ttl: Duration,
    /// Callback invoked when an MCP server sends `notifications/progress`.
    /// Receives `(tool_name, progress, total)`.
    progress_callback: Arc<Mutex<Option<Arc<dyn Fn(&str, f64, Option<f64>) + Send + Sync>>>>,
    /// Glob patterns for tool allowlisting (from `allowedTools` config).
    /// Empty = all tools allowed. `!` prefix = deny.
    allowed_patterns: Arc<RwLock<Vec<String>>>,
    /// Maximum concurrent tool calls per server (default: 8).
    max_concurrent_per_server: u32,
    /// Maximum tool result size in characters (default: 1_000_000 = ~1MB).
    max_output_chars: usize,
    /// Store for oversized tool results, enabling chunked retrieval.
    result_store: Arc<ToolResultStore>,
    /// When true, MCP tools register with minimal schemas and full schemas are
    /// fetched on-demand via ToolSearch. Reduces context usage by ~85%.
    defer_tool_schemas: Arc<AtomicBool>,
    /// Full input schemas keyed by tool name (e.g. "mcp__fetch__fetch").
    /// Populated during discovery when `defer_tool_schemas` is enabled.
    deferred_schemas: DashMap<String, Value>,
}

impl McpProcessPool {
    /// Create a new process pool with default settings.
    pub fn new() -> Self {
        let (notification_tx, notification_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            handles: DashMap::new(),
            remote_handles: DashMap::new(),
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
            elicitation_provider: Arc::new(Mutex::new(None)),
            tool_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl: Duration::from_secs(60),
            progress_callback: Arc::new(Mutex::new(None)),
            allowed_patterns: Arc::new(RwLock::new(Vec::new())),
            max_concurrent_per_server: 8,
            max_output_chars: 1_000_000,
            result_store: Arc::new(ToolResultStore::new()),
            defer_tool_schemas: Arc::new(AtomicBool::new(false)),
            deferred_schemas: DashMap::new(),
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

    /// Set glob patterns for tool allowlisting.
    ///
    /// Pattern syntax:
    /// - `"mcp__fetch__*"` — allow all tools from the `fetch` server
    /// - `"!mcp__internal__*"` — deny all tools from the `internal` server
    /// - Empty list = all tools allowed (default)
    pub async fn set_allowed_patterns(&self, patterns: Vec<String>) {
        *self.allowed_patterns.write().await = patterns;
    }

    /// Check whether a specific tool name is permitted by the allowlist patterns.
    pub async fn is_tool_allowed(&self, tool_name: &str) -> bool {
        let patterns = self.allowed_patterns.read().await;
        is_tool_allowed_by_patterns(tool_name, &patterns)
    }

    /// Enable or disable deferred tool schema loading.
    ///
    /// When enabled, MCP tools register with minimal schemas (`{"type":"object"}`)
    /// and the real schemas are stored in `deferred_schemas` for on-demand retrieval
    /// via the `McpToolSearchTool`. This reduces context usage by ~85%.
    pub fn set_defer_tool_schemas(&self, enabled: bool) {
        self.defer_tool_schemas.store(enabled, Ordering::SeqCst);
    }

    /// Check whether deferred tool schema loading is enabled.
    pub fn is_defer_tool_schemas(&self) -> bool {
        self.defer_tool_schemas.load(Ordering::SeqCst)
    }

    /// Store the real input schema for a tool (used during discovery when deferred).
    pub fn store_deferred_schema(&self, tool_name: &str, schema: Value) {
        self.deferred_schemas.insert(tool_name.to_string(), schema);
    }

    /// Retrieve the real input schema for a tool.
    ///
    /// Returns `None` if the tool has no stored schema (not an MCP tool, or
    /// deferred mode is off).
    pub fn get_deferred_schema(&self, tool_name: &str) -> Option<Value> {
        self.deferred_schemas.get(tool_name).map(|v| v.value().clone())
    }

    /// List all tool names that have deferred schemas stored.
    pub fn deferred_schema_tool_names(&self) -> Vec<String> {
        self.deferred_schemas.iter().map(|e| e.key().clone()).collect()
    }

    /// Retrieve the full stored content for a truncated tool result.
    ///
    /// Returns `None` if the chunk ID doesn't exist or has expired.
    pub async fn get_stored_result(&self, chunk_id: &str) -> Option<(String, String)> {
        self.result_store.get_full(chunk_id)
    }

    /// Retrieve a chunk of a stored tool result for incremental reading.
    ///
    /// Returns `None` if the chunk ID doesn't exist or has expired.
    pub async fn get_result_chunk(
        &self,
        chunk_id: &str,
        offset: usize,
        max_chars: usize,
    ) -> Option<ChunkResult> {
        self.result_store.get_chunk(chunk_id, offset, max_chars)
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
            elicitation_provider: self.elicitation_provider.clone(),
            capabilities: Arc::new(RwLock::new(None)),
            protocol_version: Arc::new(RwLock::new(String::new())),
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(
                self.max_concurrent_per_server as usize,
            )),
        });

        handle.start().await?;
        self.handles.insert(name.to_string(), handle);
        Ok(())
    }

    /// Start a remote MCP server (HTTP/SSE transport) and add it to the pool.
    ///
    /// Connects to the remote URL, sends `initialize` handshake, and stores
    /// the handle for tool calls. If `auth` is provided, resolves it:
    /// - API key: merged into static headers immediately
    /// - OAuth: stored as a dynamic provider that injects Bearer tokens
    ///
    /// Headers support `HeaderSource::Static` (used directly) and
    /// `HeaderSource::Command` (executed at request time).
    pub async fn start_remote_server(
        &self,
        name: &str,
        url: &str,
        header_sources: HashMap<String, HeaderSource>,
        auth: Option<McpAuthConfig>,
    ) -> Result<(), String> {
        // Split headers into static and dynamic (command-based).
        let mut resolved_headers = HashMap::new();
        let mut header_commands = HashMap::new();
        for (key, source) in header_sources {
            match source {
                HeaderSource::Static(value) => {
                    resolved_headers.insert(key, value);
                }
                HeaderSource::Command { command } => {
                    header_commands.insert(key, command);
                }
            }
        }

        let mut auth_provider: Option<Arc<OAuth2Provider>> = None;

        match auth {
            Some(McpAuthConfig::ApiKey { key, header, prefix }) => {
                let header_name = header.as_deref().unwrap_or("X-API-Key");
                let value = match prefix {
                    Some(p) => format!("{p} {key}"),
                    None => key,
                };
                resolved_headers.insert(header_name.to_string(), value);
                info!(server = %name, "Configured API key auth for remote MCP server");
            }
            Some(McpAuthConfig::OAuth {
                client_id,
                client_secret,
                auth_url,
                token_url,
                redirect_url,
                scopes,
            }) => {
                let provider = OAuth2Provider::new(
                    client_id,
                    auth_url,
                    token_url,
                    redirect_url,
                )
                .with_scopes(scopes);
                let provider = match client_secret {
                    Some(secret) => provider.with_client_secret(secret),
                    None => provider,
                };
                auth_provider = Some(Arc::new(provider));
                info!(server = %name, "Configured OAuth auth for remote MCP server");
            }
            None => {}
        }

        let handle = Arc::new(RemoteMcpServerHandle {
            name: name.to_string(),
            url: url.to_string(),
            client: reqwest::Client::new(),
            headers: resolved_headers,
            auth_provider,
            header_commands,
            state: Arc::new(RwLock::new(ServerState::Starting)),
            capabilities: Arc::new(RwLock::new(None)),
            protocol_version: Arc::new(RwLock::new(String::new())),
            next_id: AtomicU64::new(1),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            restart_count: Arc::new(AtomicU64::new(0)),
            max_restarts: self.max_restarts,
            started_at: Arc::new(RwLock::new(None)),
            request_timeout: self.request_timeout,
            tool_timeout: self.tool_timeout,
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(
                self.max_concurrent_per_server as usize,
            )),
        });

        handle.start().await?;
        self.remote_handles.insert(name.to_string(), handle);
        Ok(())
    }

    /// Call a tool on a server in the pool.
    ///
    /// Checks both stdio and remote handles. For stdio servers, attempts
    /// restart if unhealthy. For remote servers, re-initializes on failure.
    /// Uses the pool's global `max_output_chars` limit.
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> ToolResult<ToolOutput> {
        self.call_tool_with_limit(server_name, tool_name, arguments, self.max_output_chars).await
    }

    /// Call a tool with an explicit output limit (per-tool override).
    ///
    /// Same as `call_tool` but uses the provided `max_chars` instead of
    /// the pool's global default. Use this when a tool specifies
    /// `_meta.maxResultSizeChars` in its definition.
    pub async fn call_tool_with_limit(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
        max_chars: usize,
    ) -> ToolResult<ToolOutput> {

        // Check remote handles first (simpler, no process management).
        if let Some(remote) = self.remote_handles.get(server_name) {
            let remote = remote.clone();
            let _permit = remote.concurrency_semaphore.acquire().await.map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "MCP server '{server_name}' concurrency semaphore closed: {e}"
                ))
            })?;

            let state = remote.get_state().await;
            if matches!(state, ServerState::Healthy) {
                return match remote.call_tool(tool_name, arguments).await {
                    Ok(output) => Ok(self.enforce_output_limit(output, max_chars, &format!("mcp__{server_name}__{tool_name}"))),
                    Err(e) => {
                        *remote.state.write().await =
                            ServerState::Unhealthy(e.to_string());
                        let restarts = remote.restart_count.fetch_add(1, Ordering::Relaxed) as u32;
                        if restarts < remote.max_restarts {
                            remote.reset().await;
                            if let Err(reinit_err) = remote.start().await {
                                warn!(server = %server_name, error = %reinit_err, "Remote server re-init failed");
                            }
                        }
                        Err(e)
                    }
                };
            }
            // Try re-initializing.
            remote.reset().await;
            remote.start().await.map_err(|e| {
                ToolError::ExecutionFailed(format!("Remote MCP server '{server_name}' restart failed: {e}"))
            })?;
            return remote.call_tool(tool_name, arguments).await
                .map(|o| self.enforce_output_limit(o, max_chars, &format!("mcp__{server_name}__{tool_name}")));
        }

        // Stdio handle.
        let handle = self
            .handles
            .get(server_name)
            .ok_or_else(|| {
                ToolError::NotFound(format!("MCP server '{server_name}' not in pool"))
            })?
            .clone();

        let _permit = handle.concurrency_semaphore.acquire().await.map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "MCP server '{server_name}' concurrency semaphore closed: {e}"
            ))
        })?;

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
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        match handle.call_tool(tool_name, arguments).await {
            Ok(output) => Ok(self.enforce_output_limit(output, max_chars, &format!("mcp__{server_name}__{tool_name}"))),
            Err(e) => {
                *handle.state.write().await =
                    ServerState::Unhealthy(e.to_string());
                Err(e)
            }
        }
    }

    /// Enforce maximum output size by truncating oversized results.
    /// Enforce maximum output size by truncating oversized results.
    /// Stores the full content in the result store for later retrieval.
    fn enforce_output_limit(&self, mut output: ToolOutput, max_chars: usize, tool_name: &str) -> ToolOutput {
        if output.content.len() > max_chars {
            let full_content = output.content.clone();
            let truncated_len = full_content.len();

            // Store the full content for chunked retrieval (in-memory).
            let chunk_id = self.result_store.store(tool_name, full_content.clone());

            // Persist to disk so the LLM can use the Read tool to access the full result.
            let disk_path = self.persist_result_to_disk(&chunk_id, &full_content, tool_name);

            // Truncate at char boundary
            let mut end = max_chars;
            while !output.content.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            output.content.truncate(end);

            if let Some(ref path) = disk_path {
                output.content.push_str(&format!(
                    "\n\n[...truncated: showed ~{:.0}K of ~{:.0}K chars | full result saved to: {} | chunk_id={chunk_id}]",
                    max_chars as f64 / 1024.0,
                    truncated_len as f64 / 1024.0,
                    path.display(),
                ));
            } else {
                output.content.push_str(&format!(
                    "\n\n[...truncated: showed ~{:.0}K of ~{:.0}K chars | chunk_id={chunk_id}]",
                    max_chars as f64 / 1024.0,
                    truncated_len as f64 / 1024.0,
                ));
            }
        }
        output
    }

    /// Persist a large tool result to `.shannon/mcp_results/{chunk_id}.json`.
    ///
    /// Returns the file path on success, or `None` if the write fails.
    fn persist_result_to_disk(
        &self,
        chunk_id: &str,
        content: &str,
        tool_name: &str,
    ) -> Option<std::path::PathBuf> {
        let dir = std::path::Path::new(".shannon/mcp_results");
        if let Err(e) = std::fs::create_dir_all(dir) {
            warn!(error = %e, "Failed to create .shannon/mcp_results directory");
            return None;
        }

        let path = dir.join(format!("{chunk_id}.json"));
        let data = serde_json::json!({
            "chunk_id": chunk_id,
            "tool_name": tool_name,
            "content": content,
            "stored_at": chrono::Utc::now().to_rfc3339(),
        });

        match std::fs::write(&path, data.to_string()) {
            Ok(()) => {
                debug!(path = %path.display(), "Persisted large MCP result to disk");
                Some(path)
            }
            Err(e) => {
                warn!(error = %e, path = %path.display(), "Failed to persist MCP result to disk");
                None
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
        self.call_tool_with_progress_and_limit(
            server_name, tool_name, arguments, on_progress, self.max_output_chars,
        ).await
    }

    /// Call a tool with progress reporting and an explicit output limit.
    pub async fn call_tool_with_progress_and_limit(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
        on_progress: Arc<dyn Fn(f64, Option<f64>) + Send + Sync>,
        max_chars: usize,
    ) -> ToolResult<ToolOutput> {
        let handle = self
            .handles
            .get(server_name)
            .ok_or_else(|| {
                ToolError::NotFound(format!("MCP server '{server_name}' not in pool"))
            })?
            .clone();

        let _permit = handle.concurrency_semaphore.acquire().await.map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "MCP server '{server_name}' concurrency semaphore closed: {e}"
            ))
        })?;

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
            Ok(output) => Ok(self.enforce_output_limit(output, max_chars, &format!("mcp__{server_name}__{tool_name}"))),
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
        // Check remote handles first.
        if let Some(remote) = self.remote_handles.get(server_name) {
            let remote = remote.clone();
            match remote
                .send_request_with_timeout("ping", serde_json::json!({}), remote.request_timeout)
                .await
            {
                Ok(_) => {
                    *remote.state.write().await = ServerState::Healthy;
                    Ok(())
                }
                Err(e) => {
                    *remote.state.write().await = ServerState::Unhealthy(e.clone());
                    Err(e)
                }
            }
        } else {
            let handle = self
                .handles
                .get(server_name)
                .ok_or_else(|| format!("MCP server '{server_name}' not in pool"))?;
            handle.ping().await
        }
    }

    /// Send a JSON-RPC request to a server (works for both stdio and remote).
    pub(crate) async fn send_server_request(
        &self,
        server_name: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        if let Some(remote) = self.remote_handles.get(server_name) {
            return remote
                .send_request_with_timeout(method, params, remote.request_timeout)
                .await;
        }
        let handle = self
            .handles
            .get(server_name)
            .ok_or_else(|| format!("MCP server '{server_name}' not in pool"))?;
        handle.send_request(method, params).await
    }

    /// List prompts from a specific server via `prompts/list`.
    pub async fn list_prompts(
        &self,
        server_name: &str,
    ) -> Result<Vec<crate::Prompt>, String> {
        if !self.has_prompts(server_name).await {
            return Err(format!("Server '{server_name}' does not support prompts"));
        }
        let response = self.send_server_request(server_name, "prompts/list", serde_json::json!({})).await?;
        let prompts_value = response
            .get("result")
            .and_then(|r| r.get("prompts"))
            .cloned()
            .unwrap_or(serde_json::json!([]));
        serde_json::from_value(prompts_value)
            .map_err(|e| format!("Failed to parse prompts list: {e}"))
    }

    /// List prompts from all connected servers (stdio + remote).
    pub async fn list_all_prompts(&self) -> Vec<(String, Vec<crate::Prompt>)> {
        let mut result = Vec::new();

        // Stdio handles.
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

        // Remote handles.
        for entry in self.remote_handles.iter() {
            let name = entry.key().clone();
            let remote = entry.value();
            if let Ok(response) = remote
                .send_request_with_timeout("prompts/list", serde_json::json!({}), remote.request_timeout)
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
        if !self.has_prompts(server_name).await {
            return Err(format!("Server '{server_name}' does not support prompts"));
        }
        let params = serde_json::json!({
            "name": prompt_name,
            "arguments": arguments.unwrap_or_default(),
        });
        let response = self.send_server_request(server_name, "prompts/get", params).await?;
        response.get("result").cloned().ok_or_else(|| {
            format!("MCP server '{server_name}' returned no result for prompt '{prompt_name}'")
        })
    }

    /// Get the state of a specific server.
    pub async fn server_state(&self, server_name: &str) -> Option<ServerState> {
        if let Some(remote) = self.remote_handles.get(server_name) {
            return Some(remote.get_state().await);
        }
        let handle = self.handles.get(server_name)?;
        Some(handle.get_state().await)
    }

    /// Get detailed status of a specific server, including metrics.
    pub async fn server_status(&self, server_name: &str) -> Option<ServerStatus> {
        if let Some(remote) = self.remote_handles.get(server_name) {
            return Some(remote.get_status().await);
        }
        let handle = self.handles.get(server_name)?;
        Some(handle.get_status().await)
    }

    /// List all server names and their states (stdio + remote).
    pub async fn list_servers(&self) -> Vec<(String, ServerState)> {
        let mut result = Vec::new();
        for entry in self.handles.iter() {
            let state = entry.value().get_state().await;
            result.push((entry.key().clone(), state));
        }
        for entry in self.remote_handles.iter() {
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

    /// Set the elicitation provider for handling `elicitation/create` requests.
    ///
    /// The provider receives an [`ElicitationRequest`] and returns an
    /// [`ElicitationResult`]. If no provider is set, servers receive a
    /// "method not found" error when they attempt elicitation.
    pub async fn set_elicitation_provider(&self, provider: ElicitationProvider) {
        *self.elicitation_provider.lock().await = Some(provider);
    }

    /// Set the TTL for tool result caching (default: 60 seconds).
    pub fn set_cache_ttl(&mut self, ttl: Duration) {
        self.cache_ttl = ttl;
    }

    /// Set a callback invoked when MCP servers send `notifications/progress`.
    ///
    /// The callback receives `(tool_name, progress, total)` where `total` may
    /// be `None` if the server did not include it.
    pub async fn set_progress_callback(
        &self,
        callback: Arc<dyn Fn(&str, f64, Option<f64>) + Send + Sync>,
    ) {
        *self.progress_callback.lock().await = Some(callback);
    }

    /// Request completions from a server that supports the completions capability.
    ///
    /// Sends `completion/complete` to the server and returns the result.
    pub async fn complete(
        &self,
        server_name: &str,
        ref_type: &str,
        ref_uri: Option<&str>,
        ref_name: Option<&str>,
        argument_name: &str,
        argument_value: &str,
    ) -> Result<crate::CompletionResult, String> {
        let params = serde_json::json!({
            "ref": {
                "type": ref_type,
                "uri": ref_uri,
                "name": ref_name,
            },
            "argument": {
                "name": argument_name,
                "value": argument_value,
            }
        });

        let response = self
            .send_server_request(server_name, "completion/complete", params)
            .await?;

        serde_json::from_value::<crate::CompletionResult>(
            response.get("result").cloned().unwrap_or(serde_json::json!({})),
        )
        .map_err(|e| format!("Failed to parse completion result: {e}"))
    }

    /// Clear all cached tool results.
    pub async fn clear_cache(&self) {
        self.tool_cache.write().await.clear();
    }

    /// Clear cached results for a specific server.
    pub async fn clear_server_cache(&self, server_name: &str) {
        let prefix = format!("{server_name}:");
        let mut cache = self.tool_cache.write().await;
        cache.retain(|k, _| !k.starts_with(&prefix));
    }

    /// Look up a cached tool result. Returns `None` if not cached or expired.
    async fn get_cached(&self, key: &str) -> Option<String> {
        let cache = self.tool_cache.read().await;
        if let Some((value, timestamp)) = cache.get(key) {
            if timestamp.elapsed() < self.cache_ttl {
                return Some(value.clone());
            }
        }
        None
    }

    /// Store a tool result in the cache.
    async fn put_cached(&self, key: &str, value: String) {
        let mut cache = self.tool_cache.write().await;
        cache.insert(key.to_string(), (value, Instant::now()));
    }

    /// Start a background task that listens for server notifications
    /// and dispatches them to the appropriate callback.
    ///
    /// Currently handles:
    /// - `notifications/tools/list_changed` → calls `on_tools_changed` callback
    pub fn start_notification_handler(&self) {
        let rx = self.notification_rx.clone();
        let on_tools_changed = self.on_tools_changed.clone();
        let tool_cache = self.tool_cache.clone();

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
                                // Invalidate cached results for this server.
                                {
                                    let prefix = format!("{server_name}:");
                                    let mut cache = tool_cache.write().await;
                                    cache.retain(|k, _| !k.starts_with(&prefix));
                                }
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

    /// Gracefully shut down all servers (stdio + remote).
    pub async fn shutdown_all(&self) {
        info!("Shutting down all MCP servers");
        for entry in self.handles.iter() {
            entry.value().shutdown().await;
        }
        self.handles.clear();
        self.remote_handles.clear();
    }

    /// Stop a specific server by name.
    pub async fn stop_server(&self, name: &str) -> Result<(), String> {
        if let Some((_, handle)) = self.handles.remove(name) {
            handle.shutdown().await;
            info!(server = %name, "Stopped MCP server");
            return Ok(());
        }
        if self.remote_handles.remove(name).is_some() {
            info!(server = %name, "Removed remote MCP server");
            return Ok(());
        }
        Err(format!("MCP server '{name}' not found"))
    }

    /// Reload server configuration — diff against current state and apply changes.
    ///
    /// - New servers in `config` are started
    /// - Servers no longer in `config` are stopped
    /// - Changed servers (different command/args/url) are restarted
    /// - `allowed_tools` patterns are updated
    pub async fn reload_from_config(
        &self,
        config: &crate::config::McpConfig,
    ) -> Result<Vec<String>, String> {
        let mut changes: Vec<String> = Vec::new();
        let config_servers: std::collections::HashSet<&str> =
            config.mcp_servers.keys().map(|s| s.as_str()).collect();

        // Collect current server names
        let current_stdio: std::collections::HashSet<String> =
            self.handles.iter().map(|e| e.key().clone()).collect();
        let current_remote: std::collections::HashSet<String> =
            self.remote_handles.iter().map(|e| e.key().clone()).collect();

        // Stop servers no longer in config
        for name in current_stdio.iter().chain(current_remote.iter()) {
            if !config_servers.contains(name.as_str()) {
                self.stop_server(name).await?;
                changes.push(format!("Removed server '{name}'"));
            }
        }

        // Start new servers and restart changed ones
        for (name, server_config) in &config.mcp_servers {
            let is_current = current_stdio.contains(name) || current_remote.contains(name);

            match server_config {
                crate::config::McpServerConfig::Stdio { command, args, env } => {
                    if !is_current {
                        self.start_server(name, command, args, env).await?;
                        changes.push(format!("Started stdio server '{name}'"));
                    }
                    // Note: detecting config changes for existing servers would require
                    // storing the original config — a future enhancement.
                }
                crate::config::McpServerConfig::Sse { url, headers, auth }
                | crate::config::McpServerConfig::Http { url, headers, auth } => {
                    if !is_current {
                        self.start_remote_server(name, url, headers.clone(), auth.clone()).await?;
                        changes.push(format!("Started remote server '{name}'"));
                    }
                }
                crate::config::McpServerConfig::WebSocket { .. } => {
                    if !is_current {
                        changes.push(format!(
                            "Skipped WebSocket server '{name}' (not yet supported)"
                        ));
                    }
                }
            }
        }

        // Update allowed tools patterns
        if !config.allowed_tools.is_empty() {
            self.set_allowed_patterns(config.allowed_tools.clone()).await;
            changes.push(format!(
                "Updated allowed tools: {} pattern(s)",
                config.allowed_tools.len()
            ));
        }

        info!(changes = changes.len(), "Config reload completed");
        Ok(changes)
    }

    /// Get the number of servers in the pool (stdio + remote).
    pub fn server_count(&self) -> usize {
        self.handles.len() + self.remote_handles.len()
    }

    /// Check whether a server supports the `tools` capability.
    pub async fn has_tools(&self, server_name: &str) -> bool {
        self.get_capabilities(server_name)
            .await
            .map_or(false, |c| c.tools.is_some())
    }

    /// Check whether a server supports the `resources` capability.
    pub async fn has_resources(&self, server_name: &str) -> bool {
        self.get_capabilities(server_name)
            .await
            .map_or(false, |c| c.resources.is_some())
    }

    /// Check whether a server supports the `prompts` capability.
    pub async fn has_prompts(&self, server_name: &str) -> bool {
        self.get_capabilities(server_name)
            .await
            .map_or(false, |c| c.prompts.is_some())
    }

    /// Get the negotiated protocol version for a server.
    pub async fn server_protocol_version(&self, server_name: &str) -> Option<String> {
        if let Some(handle) = self.handles.get(server_name) {
            let v = handle.protocol_version.read().await;
            if v.is_empty() { None } else { Some(v.clone()) }
        } else if let Some(handle) = self.remote_handles.get(server_name) {
            let v = handle.protocol_version.read().await;
            if v.is_empty() { None } else { Some(v.clone()) }
        } else {
            None
        }
    }

    /// Retrieve the stored capabilities for a server (stdio or remote).
    async fn get_capabilities(&self, server_name: &str) -> Option<crate::ServerCapabilities> {
        if let Some(handle) = self.handles.get(server_name) {
            handle.capabilities.read().await.clone()
        } else if let Some(handle) = self.remote_handles.get(server_name) {
            handle.capabilities.read().await.clone()
        } else {
            None
        }
    }
}

impl Drop for McpProcessPool {
    fn drop(&mut self) {
        // Best-effort cleanup: kill child processes to prevent zombies.
        // Since Drop can't be async, we synchronously iterate and kill.
        for entry in self.handles.iter() {
            let handle = entry.value();
            if let Ok(mut child_guard) = handle.child.try_lock() {
                if let Some(ref mut child) = *child_guard {
                    // start_kill() is sync and sends SIGKILL immediately.
                    let _ = child.start_kill();
                }
                *child_guard = None;
            }
        }
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
    /// Per-tool output limit in chars (from `_meta.maxResultSizeChars`).
    /// Overrides the pool's global `max_output_chars` when set.
    max_output_chars: Option<usize>,
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
        Self::with_output_limit(
            pool,
            server_name,
            remote_tool_name,
            description,
            input_schema,
            annotations,
            None,
        )
    }

    /// Create a pooled tool adapter with an explicit per-tool output limit.
    ///
    /// `max_output_chars` overrides the pool's global limit for this specific tool.
    /// Parse from `_meta.maxResultSizeChars` in the MCP tool definition.
    pub fn with_output_limit(
        pool: Arc<McpProcessPool>,
        server_name: String,
        remote_tool_name: String,
        description: String,
        input_schema: Value,
        annotations: Option<crate::ToolAnnotations>,
        max_output_chars: Option<usize>,
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
            max_output_chars,
        }
    }

    /// Internal helper: calls the tool via the pool, using progress reporting
    /// when a progress callback is registered on the pool.
    async fn call_tool_inner(&self, input: Value) -> ToolResult<ToolOutput> {
        // Use per-tool limit if set, otherwise use pool's global default.
        let max_chars = self.max_output_chars.unwrap_or(self.pool.max_output_chars);

        let progress_cb = self.pool.progress_callback.lock().await;
        if let Some(ref cb) = *progress_cb {
            let tool_name = self.tool_name.clone();
            let cb = cb.clone();
            drop(progress_cb);

            let on_progress = Arc::new(move |progress: f64, total: Option<f64>| {
                cb(&tool_name, progress, total);
            });

            self.pool
                .call_tool_with_progress_and_limit(
                    &self.server_name,
                    &self.remote_tool_name,
                    input,
                    on_progress,
                    max_chars,
                )
                .await
        } else {
            drop(progress_cb);
            self.pool
                .call_tool_with_limit(&self.server_name, &self.remote_tool_name, input, max_chars)
                .await
        }
    }

    /// Produce a deterministic, sorted JSON string for cache key stability.
    fn sorted_args(input: &Value) -> String {
        match input {
            Value::Object(map) => {
                let mut pairs: Vec<(String, String)> = map
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect();
                pairs.sort_by(|a, b| a.0.cmp(&b.0));
                pairs.into_iter()
                    .map(|(k, v)| format!("{k}:{v}"))
                    .collect::<Vec<_>>()
                    .join(",")
            }
            other => other.to_string(),
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
        // When deferred mode is enabled, return a minimal stub to save context.
        // The real schema is available via pool.get_deferred_schema() / McpToolSearchTool.
        if self.pool.is_defer_tool_schemas() {
            serde_json::json!({
                "type": "object",
                "description": format!(
                    "Use the mcp__tool_search tool with tool_name=\"{}\" to get the full parameter schema before calling this tool.",
                    self.tool_name
                )
            })
        } else {
            self.input_schema.clone()
        }
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        // Check tool permission against allowlist patterns.
        if !self.pool.is_tool_allowed(&self.tool_name).await {
            return Err(ToolError::ExecutionFailed(format!(
                "Tool '{}' is not in the allowed tools list",
                self.tool_name
            )));
        }

        // Check cache for read-only tools.
        if self.is_read_only() {
            let cache_key = format!(
                "{}:{}:{}",
                self.server_name,
                self.remote_tool_name,
                Self::sorted_args(&input)
            );
            if let Some(cached) = self.pool.get_cached(&cache_key).await {
                debug!(
                    server = %self.server_name,
                    tool = %self.remote_tool_name,
                    "Returning cached tool result"
                );
                return Ok(ToolOutput::success(cached));
            }

            let result = self.call_tool_inner(input.clone()).await?;

            // Store in cache on success.
            if !result.is_error {
                self.pool.put_cached(&cache_key, result.content.clone()).await;
            }

            return Ok(result);
        }

        self.call_tool_inner(input).await
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

// ---------------------------------------------------------------------------
// Sampling provider bridge
// ---------------------------------------------------------------------------

/// Create a sampling provider that delegates to an [`shannon_core::api::client::LlmClient`].
///
/// This wires MCP `sampling/createMessage` requests through to Shannon's LLM
/// backend. The provider:
/// - Converts `SamplingMessageRole` → LLM message roles (`"user"` / `"assistant"`)
/// - Converts `SamplingContent` → LLM content types
/// - Passes `system_prompt` through as the system message
/// - Logs each request for observability
///
/// Returns a `SamplingProvider` suitable for [`McpProcessPool::set_sampling_provider`].
pub fn make_sampling_provider(
    client: std::sync::Arc<shannon_core::api::client::LlmClient>,
) -> SamplingProvider {
    use shannon_core::api::types::{ContentBlock, Message, MessageContent};
    use crate::{CreateMessageRequest, CreateMessageResult, SamplingContent, SamplingMessageRole};

    Arc::new(move |req: CreateMessageRequest| {
        let client = client.clone();
        Box::pin(async move {
            tracing::info!(
                messages = req.messages.len(),
                model_hint = ?req.model_preferences.as_ref().and_then(|p| p.hints.as_ref().and_then(|h| h.first().and_then(|h| h.name.as_deref()))),
                "MCP sampling request"
            );

            // Convert sampling messages → LLM messages.
            let messages: Vec<Message> = req.messages.into_iter().map(|msg| {
                let role = match msg.role {
                    SamplingMessageRole::User => "user".to_string(),
                    SamplingMessageRole::Assistant => "assistant".to_string(),
                };
                let content = match msg.content {
                    SamplingContent::Text { text } => MessageContent::Text(text),
                    SamplingContent::Image { data, mime_type } => {
                        MessageContent::Blocks(vec![ContentBlock::Image {
                            source: shannon_core::api::types::ImageSource::base64(mime_type, data),
                        }])
                    }
                };
                Message { role, content }
            }).collect();

            let response = client
                .send_message(messages, None, req.system_prompt)
                .await
                .map_err(|e| format!("Sampling LLM call failed: {e}"))?;

            // Extract text from response content blocks.
            let mut text = String::new();
            for block in &response {
                if let ContentBlock::Text { text: t } = block {
                    text.push_str(t);
                }
            }

            Ok(CreateMessageResult {
                role: SamplingMessageRole::Assistant,
                model: "shannon-code".to_string(),
                content: SamplingContent::Text { text },
                stop_reason: Some(crate::StopReason::EndTurn),
            })
        })
    })
}

/// User prompt callback type for elicitation.
///
/// Receives the server's message and optional JSON Schema,
/// returns `(ElicitationAction, Option<Value>)` where the value
/// is the user's structured input on accept.
pub type UserPromptCallback = Arc<
    dyn Fn(String, Option<serde_json::Value>) -> Pin<Box<dyn Future<Output = (crate::ElicitationAction, Option<serde_json::Value>)> + Send>>
        + Send
        + Sync,
>;

/// Create an elicitation provider that delegates to a user prompt callback.
///
/// When an MCP server sends `elicitation/create`, the callback is invoked
/// with the server's message and optional schema. The callback should
/// present the prompt to the user (e.g., via the TUI) and return the result.
///
/// If no callback is provided, all elicitation requests are auto-declined.
pub fn make_elicitation_provider(
    prompt_callback: Option<UserPromptCallback>,
) -> ElicitationProvider {
    use crate::{ElicitationRequest, ElicitationResult, ElicitationAction};

    Arc::new(move |req: ElicitationRequest| {
        let callback = prompt_callback.clone();
        Box::pin(async move {
            tracing::info!(
                message = %req.message,
                has_schema = req.requested_schema.is_some(),
                "MCP elicitation request"
            );

            match callback {
                Some(cb) => {
                    let (action, content) = cb(req.message, req.requested_schema).await;
                    Ok(ElicitationResult { action, content })
                }
                None => {
                    tracing::warn!("Elicitation request auto-declined (no callback configured)");
                    Ok(ElicitationResult {
                        action: ElicitationAction::Decline,
                        content: None,
                    })
                }
            }
        })
    })
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

    // Check capabilities before attempting tools/list.
    if !pool.has_tools(server_name).await {
        debug!(
            server = %server_name,
            "Server does not advertise tools capability; skipping tools/list"
        );
        return Ok(PooledDiscoveryResult {
            server_name: server_name.to_string(),
            tools: Vec::new(),
        });
    }

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

            // Parse per-tool output limit from _meta.maxResultSizeChars.
            let max_output_chars: Option<usize> = tool_value
                .get("_meta")
                .and_then(|m| m.get("maxResultSizeChars"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);

            // Store the real schema for deferred retrieval if enabled.
            let adapter = PooledMcpToolAdapter::with_output_limit(
                pool.clone(),
                server_name.to_string(),
                name.clone(),
                description,
                input_schema.clone(),
                annotations,
                max_output_chars,
            );

            // When deferred mode is on, store the real schema and let the adapter
            // return a minimal stub via input_schema().
            if pool.is_defer_tool_schemas() {
                pool.store_deferred_schema(&adapter.tool_name, input_schema);
            }

            tools.push(adapter);
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
    fn test_deferred_tool_schema_off_by_default() {
        let pool = McpProcessPool::new();
        assert!(!pool.is_defer_tool_schemas());
    }

    #[test]
    fn test_deferred_tool_schema_returns_minimal_when_enabled() {
        let pool = Arc::new(McpProcessPool::new());
        pool.set_defer_tool_schemas(true);

        let full_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "The URL to fetch"},
                "method": {"type": "string", "enum": ["GET", "POST"]}
            },
            "required": ["url"]
        });

        let adapter = PooledMcpToolAdapter::new(
            pool.clone(),
            "fetch".to_string(),
            "fetch".to_string(),
            "Fetch a URL".to_string(),
            full_schema.clone(),
            None,
        );

        // Store the deferred schema.
        pool.store_deferred_schema(adapter.name(), full_schema.clone());

        // input_schema() should return minimal stub.
        let schema = adapter.input_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema.get("properties").is_none());

        // Verify the real schema is retrievable.
        let real = pool.get_deferred_schema(adapter.name()).unwrap();
        assert_eq!(real["properties"]["url"]["type"], "string");
    }

    #[test]
    fn test_deferred_tool_schema_returns_full_when_disabled() {
        let pool = Arc::new(McpProcessPool::new());
        // Deferred mode is OFF by default.

        let full_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            }
        });

        let adapter = PooledMcpToolAdapter::new(
            pool,
            "search".to_string(),
            "search".to_string(),
            "Search".to_string(),
            full_schema.clone(),
            None,
        );

        // input_schema() should return the full schema.
        let schema = adapter.input_schema();
        assert_eq!(schema["properties"]["query"]["type"], "string");
    }

    #[test]
    fn test_deferred_schema_store_and_retrieve() {
        let pool = McpProcessPool::new();
        pool.store_deferred_schema("mcp__test__tool", serde_json::json!({"type": "object"}));
        assert!(pool.get_deferred_schema("mcp__test__tool").is_some());
        assert!(pool.get_deferred_schema("mcp__nonexistent").is_none());
        assert!(pool.deferred_schema_tool_names().contains(&"mcp__test__tool".to_string()));
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
        assert!(result.contains("[compressed:") || result.contains("[...truncated:"));
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
        assert!(result.contains("[compressed:") || result.contains("[...truncated:"));
    }

    #[test]
    fn test_tool_result_truncation_preserves_unicode() {
        // String with multi-byte chars
        let content = "日本語テスト\n".repeat(10_000);
        let result = truncate_tool_result(&content);
        // Should not panic on char boundary
        assert!(result.contains("[compressed:") || result.contains("[...truncated:"));
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

    // -- Tool permission tests --------------------------------------------

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("mcp__fetch__fetch", "mcp__fetch__fetch"));
        assert!(!glob_match("mcp__fetch__fetch", "mcp__fetch__search"));
    }

    #[test]
    fn test_glob_match_star() {
        assert!(glob_match("mcp__fetch__*", "mcp__fetch__fetch"));
        assert!(glob_match("mcp__fetch__*", "mcp__fetch__search"));
        assert!(glob_match("mcp__*", "mcp__fetch__fetch"));
        assert!(!glob_match("mcp__fetch__*", "mcp__other__tool"));
    }

    #[test]
    fn test_glob_match_question() {
        assert!(glob_match("mcp__x__?", "mcp__x__a"));
        assert!(!glob_match("mcp__x__?", "mcp__x__ab"));
    }

    #[test]
    fn test_is_tool_allowed_empty_patterns() {
        assert!(is_tool_allowed_by_patterns("mcp__anything__tool", &[]));
    }

    #[test]
    fn test_is_tool_allowed_allow_pattern() {
        let patterns = vec!["mcp__fetch__*".to_string()];
        assert!(is_tool_allowed_by_patterns("mcp__fetch__fetch", &patterns));
        assert!(!is_tool_allowed_by_patterns("mcp__other__tool", &patterns));
    }

    #[test]
    fn test_is_tool_allowed_deny_pattern() {
        let patterns = vec!["!mcp__internal__*".to_string()];
        assert!(!is_tool_allowed_by_patterns("mcp__internal__secret", &patterns));
        assert!(is_tool_allowed_by_patterns("mcp__fetch__fetch", &patterns));
    }

    #[test]
    fn test_is_tool_allowed_mixed_patterns() {
        let patterns = vec![
            "mcp__fetch__*".to_string(),
            "mcp__memory__*".to_string(),
            "!mcp__internal__*".to_string(),
        ];
        assert!(is_tool_allowed_by_patterns("mcp__fetch__fetch", &patterns));
        assert!(is_tool_allowed_by_patterns("mcp__memory__create", &patterns));
        assert!(!is_tool_allowed_by_patterns("mcp__internal__secret", &patterns));
        assert!(!is_tool_allowed_by_patterns("mcp__other__tool", &patterns));
    }

    #[test]
    fn test_is_tool_allowed_deny_overrides_allow() {
        let patterns = vec![
            "mcp__*".to_string(),
            "!mcp__internal__*".to_string(),
        ];
        assert!(is_tool_allowed_by_patterns("mcp__fetch__fetch", &patterns));
        assert!(!is_tool_allowed_by_patterns("mcp__internal__tool", &patterns));
    }

    #[tokio::test]
    async fn test_pool_allowed_patterns() {
        let pool = McpProcessPool::new();
        assert!(pool.is_tool_allowed("mcp__anything__tool").await);

        pool.set_allowed_patterns(vec!["mcp__fetch__*".to_string()]).await;
        assert!(pool.is_tool_allowed("mcp__fetch__fetch").await);
        assert!(!pool.is_tool_allowed("mcp__other__tool").await);
    }

    #[test]
    fn test_enforce_output_limit_under() {
        let pool = McpProcessPool::new();
        let output = ToolOutput::success("hello world".to_string());
        let limited = pool.enforce_output_limit(output, 1000, "mcp__test__tool");
        assert_eq!(limited.content, "hello world");
    }

    #[test]
    fn test_enforce_output_limit_over() {
        let pool = McpProcessPool::new();
        let long_content = "a".repeat(2000);
        let output = ToolOutput::success(long_content);
        let limited = pool.enforce_output_limit(output, 1000, "mcp__test__tool");
        assert!(limited.content.len() < 1200); // 1000 + truncation notice + chunk_id
        assert!(limited.content.contains("[...truncated:"));
    }

    #[test]
    fn test_enforce_output_limit_preserves_unicode() {
        let pool = McpProcessPool::new();
        // Unicode chars at boundary
        let content = "日本語".repeat(500); // Each char is 3 bytes
        let output = ToolOutput::success(content);
        let limited = pool.enforce_output_limit(output, 100, "mcp__test__tool");
        // Should not panic on char boundary
        assert!(limited.content.contains("[...truncated:") || limited.content.len() <= 200);
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
    async fn test_tool_result_store_roundtrip() {
        let pool = McpProcessPool::new();
        // Trigger enforce_output_limit with content that exceeds the limit.
        let long_content = "x".repeat(2000);
        let output = ToolOutput::success(long_content.clone());
        let limited = pool.enforce_output_limit(output, 100, "mcp__srv__tool");

        // Should contain a chunk_id in the truncation notice.
        assert!(limited.content.contains("[...truncated:"));
        let chunk_id = limited.content
            .split("chunk_id=")
            .nth(1)
            .and_then(|s| s.split(']').next())
            .expect("should have chunk_id")
            .to_string();

        // Retrieve full content.
        let (tool_name, full) = pool.get_stored_result(&chunk_id)
            .await
            .expect("should find stored result");
        assert_eq!(tool_name, "mcp__srv__tool");
        assert_eq!(full, long_content);
    }

    #[tokio::test]
    async fn test_tool_result_store_chunking() {
        let pool = McpProcessPool::new();
        let long_content = "abcdefghij".repeat(100); // 1000 chars
        let output = ToolOutput::success(long_content.clone());
        let limited = pool.enforce_output_limit(output, 50, "mcp__srv__tool");

        let chunk_id = limited.content
            .split("chunk_id=")
            .nth(1)
            .and_then(|s| s.split(']').next())
            .expect("should have chunk_id")
            .to_string();

        // Get first chunk.
        let chunk = pool.get_result_chunk(&chunk_id, 0, 100)
            .await
            .expect("should get chunk");
        assert_eq!(chunk.content.len(), 100);
        assert!(chunk.has_more);
        assert_eq!(chunk.total_len, 1000);

        // Get second chunk.
        let chunk2 = pool.get_result_chunk(&chunk_id, chunk.offset, 100)
            .await
            .expect("should get chunk 2");
        assert!(chunk2.has_more);

        // Get beyond end.
        let chunk_end = pool.get_result_chunk(&chunk_id, 2000, 100)
            .await
            .expect("should handle past-end");
        assert!(!chunk_end.has_more);
        assert!(chunk_end.content.is_empty());
    }

    #[tokio::test]
    async fn test_tool_result_store_missing() {
        let pool = McpProcessPool::new();
        assert!(pool.get_stored_result("nonexistent").await.is_none());
        assert!(pool.get_result_chunk("nonexistent", 0, 100).await.is_none());
    }

    #[test]
    fn test_disk_persistence_saves_file() {
        let pool = McpProcessPool::new();
        let content = "x".repeat(5000);
        let output = ToolOutput::success(content.clone());
        let limited = pool.enforce_output_limit(output, 100, "mcp__test__disk");

        // Should reference a file path.
        assert!(limited.content.contains("full result saved to:"));
        assert!(limited.content.contains(".shannon/mcp_results/"));

        // Extract chunk_id and verify the file exists.
        let chunk_id = limited.content
            .split("chunk_id=")
            .nth(1)
            .and_then(|s| s.split(']').next())
            .expect("should have chunk_id")
            .to_string();

        let path = std::path::Path::new(".shannon/mcp_results").join(format!("{chunk_id}.json"));
        assert!(path.exists(), "disk file should exist");

        // Verify file content.
        let file_data: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(file_data["tool_name"], "mcp__test__disk");
        assert_eq!(file_data["content"], content);
        assert!(file_data["stored_at"].is_string());

        // Clean up.
        let _ = std::fs::remove_file(&path);
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
