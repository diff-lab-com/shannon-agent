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

mod adapter;
mod discovery;
mod handle;
mod remote_handle;
mod types;

#[cfg(test)]
mod tests;

use dashmap::DashMap;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::auth::OAuth2Provider;
use crate::config::{HeaderSource, McpAuthConfig};
use crate::resource_subscription::ResourceSubscriptionManager;
use crate::webhook::{EventPublisher, McpEvent, McpEventType};
use handle::McpServerHandle;
use remote_handle::RemoteMcpServerHandle;
use types::*;

// Re-exports from submodules to maintain public API.
pub use adapter::PooledMcpToolAdapter;
pub use discovery::{
    PooledDiscoveryResult, UserPromptCallback, discover_pooled_remote_tools, discover_pooled_tools,
    make_elicitation_provider, make_sampling_provider,
};
pub use types::{ChunkResult, ServerState, ServerStatus};

/// Type alias for the async sampling callback.
///
/// Takes a `CreateMessageRequest` and returns a `CreateMessageResult`.
pub(crate) type SamplingProvider = Arc<
    dyn Fn(
            crate::CreateMessageRequest,
        )
            -> Pin<Box<dyn Future<Output = Result<crate::CreateMessageResult, String>> + Send>>
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
        )
            -> Pin<Box<dyn Future<Output = Result<crate::ElicitationResult, String>> + Send>>
        + Send
        + Sync,
>;

// ---------------------------------------------------------------------------
// Process Pool
// ---------------------------------------------------------------------------

/// Manages a pool of persistent MCP server processes.
///
/// The pool keeps server processes alive across multiple tool calls,
/// handles health monitoring, automatic restarts, and graceful shutdown.
pub struct McpProcessPool {
    /// Server handles keyed by server name (stdio transport).
    pub(crate) handles: DashMap<String, Arc<McpServerHandle>>,
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
    notification_rx: Arc<Mutex<tokio::sync::mpsc::Receiver<(String, Value)>>>,
    /// Notification sender — cloned into each server handle.
    notification_tx: tokio::sync::mpsc::Sender<(String, Value)>,
    /// Callback invoked when a server reports `notifications/tools/list_changed`.
    ///
    /// Receives `(server_name, new_tool_adapters)` so the caller can hot-swap
    /// the tools in its registry without restarting the server.
    on_tools_changed:
        Arc<Mutex<Option<Arc<dyn Fn(&str, Vec<PooledMcpToolAdapter>) + Send + Sync>>>>,
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
    pub(crate) progress_callback:
        Arc<Mutex<Option<Arc<dyn Fn(&str, f64, Option<f64>) + Send + Sync>>>>,
    /// Glob patterns for tool allowlisting (from `allowedTools` config).
    /// Empty = all tools allowed. `!` prefix = deny.
    allowed_patterns: Arc<RwLock<Vec<String>>>,
    /// Maximum concurrent tool calls per server (default: 8).
    max_concurrent_per_server: u32,
    /// Maximum tool result size in characters (default: 1_000_000 = ~1MB).
    pub(crate) max_output_chars: usize,
    /// Store for oversized tool results, enabling chunked retrieval.
    result_store: Arc<ToolResultStore>,
    /// When true, MCP tools register with minimal schemas and full schemas are
    /// fetched on-demand via ToolSearch. Reduces context usage by ~85%.
    defer_tool_schemas: Arc<AtomicBool>,
    /// Full input schemas keyed by tool name (e.g. "mcp__fetch__fetch").
    /// Populated during discovery when `defer_tool_schemas` is enabled.
    deferred_schemas: DashMap<String, Value>,
    /// Tool descriptions keyed by tool name, for fuzzy search support.
    deferred_descriptions: DashMap<String, String>,
    /// Background config watcher task handle.
    config_watcher_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// Callback invoked after a config hot-reload completes.
    /// Receives a list of human-readable change descriptions.
    on_config_reloaded: Arc<Mutex<Option<Arc<dyn Fn(&[String]) + Send + Sync>>>>,
    /// Optional event publisher for webhook notifications.
    event_publisher: Option<Arc<EventPublisher>>,
    /// Resource subscription manager tracking active subscriptions.
    subscriptions: Arc<ResourceSubscriptionManager>,
}

impl McpProcessPool {
    /// Create a new process pool with default settings.
    pub fn new() -> Self {
        let (notification_tx, notification_rx) = tokio::sync::mpsc::channel(1024);
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
            deferred_descriptions: DashMap::new(),
            config_watcher_task: Arc::new(Mutex::new(None)),
            on_config_reloaded: Arc::new(Mutex::new(None)),
            event_publisher: None,
            subscriptions: Arc::new(ResourceSubscriptionManager::new()),
        }
    }

    /// Set the event publisher for webhook notifications.
    ///
    /// When set, the pool fires MCP events (server connected/disconnected,
    /// tool call start/complete, notifications) to registered webhooks.
    pub fn with_event_publisher(mut self, publisher: EventPublisher) -> Self {
        self.event_publisher = Some(Arc::new(publisher));
        self
    }

    /// Get a reference to the event publisher, if configured.
    pub fn event_publisher(&self) -> Option<&Arc<EventPublisher>> {
        self.event_publisher.as_ref()
    }

    /// Fire an event to the event publisher (if configured).
    async fn fire_event(&self, event: McpEvent) {
        if let Some(ref publisher) = self.event_publisher {
            publisher.publish(event).await;
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
        self.deferred_schemas
            .get(tool_name)
            .map(|v| v.value().clone())
    }

    /// List all tool names that have deferred schemas stored.
    pub fn deferred_schema_tool_names(&self) -> Vec<String> {
        self.deferred_schemas
            .iter()
            .map(|e| e.key().clone())
            .collect()
    }

    /// Store a tool description for fuzzy search.
    pub fn store_deferred_description(&self, tool_name: &str, description: String) {
        self.deferred_descriptions
            .insert(tool_name.to_string(), description);
    }

    /// Get a tool's description for search display.
    pub fn get_deferred_description(&self, tool_name: &str) -> Option<String> {
        self.deferred_descriptions
            .get(tool_name)
            .map(|v| v.value().clone())
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
            total_result_bytes: AtomicU64::new(0),
            budget_bytes: Arc::new(RwLock::new(None)),
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
        self.fire_event(McpEvent::new(
            McpEventType::ServerConnected,
            name.to_string(),
            None,
            serde_json::json!({"transport": "stdio"}),
        ))
        .await;
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
            Some(McpAuthConfig::ApiKey {
                key,
                header,
                prefix,
            }) => {
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
                let provider = OAuth2Provider::new(client_id, auth_url, token_url, redirect_url)
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
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|e| {
                    tracing::error!("Failed to create HTTP client: {e}");
                    reqwest::Client::new()
                }),
            headers: resolved_headers,
            auth_provider,
            header_commands,
            state: Arc::new(RwLock::new(ServerState::Starting)),
            capabilities: Arc::new(RwLock::new(None)),
            protocol_version: Arc::new(RwLock::new(String::new())),
            next_id: AtomicU64::new(1),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            total_result_bytes: AtomicU64::new(0),
            budget_bytes: Arc::new(RwLock::new(None)),
            restart_count: Arc::new(AtomicU64::new(0)),
            max_restarts: self.max_restarts,
            started_at: Arc::new(RwLock::new(None)),
            request_timeout: self.request_timeout,
            tool_timeout: self.tool_timeout,
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(
                self.max_concurrent_per_server as usize,
            )),
            session_id: Arc::new(RwLock::new(None)),
            ws_transport: None,
            sampling_provider: self.sampling_provider.clone(),
            notification_tx: self.notification_tx.clone(),
        });

        handle.start().await?;
        self.remote_handles.insert(name.to_string(), handle);
        self.fire_event(McpEvent::new(
            McpEventType::ServerConnected,
            name.to_string(),
            None,
            serde_json::json!({"transport": "http"}),
        ))
        .await;
        Ok(())
    }

    /// Start a WebSocket-based MCP server and add it to the pool.
    ///
    /// Creates a [`WebSocketTransport`], connects to the endpoint, then runs
    /// the standard MCP initialization handshake. The resulting handle is
    /// stored in `remote_handles` with `ws_transport` set, so all subsequent
    /// requests go over the WebSocket instead of HTTP.
    pub async fn start_websocket_server(
        &self,
        name: &str,
        url: &str,
        auth: Option<McpAuthConfig>,
    ) -> Result<(), String> {
        // Connect WebSocket transport.
        let mut ws = crate::WebSocketTransport::new(url);
        ws.connect()
            .await
            .map_err(|e| format!("WebSocket connect failed for '{name}': {e}"))?;

        // Resolve auth into static headers (WebSocket doesn't use HTTP headers
        // natively, but we store them for any future subprotocol use).
        let mut resolved_headers = HashMap::new();
        let auth_provider: Option<Arc<OAuth2Provider>> = match auth {
            Some(McpAuthConfig::ApiKey {
                key,
                header,
                prefix,
            }) => {
                let header_name = header.as_deref().unwrap_or("X-API-Key");
                let value = match prefix {
                    Some(p) => format!("{p} {key}"),
                    None => key,
                };
                resolved_headers.insert(header_name.to_string(), value);
                info!(server = %name, "Configured API key auth for WebSocket MCP server");
                None
            }
            Some(McpAuthConfig::OAuth {
                client_id,
                client_secret,
                auth_url,
                token_url,
                redirect_url,
                scopes,
            }) => {
                let provider = OAuth2Provider::new(client_id, auth_url, token_url, redirect_url)
                    .with_scopes(scopes);
                let provider = match client_secret {
                    Some(secret) => provider.with_client_secret(secret),
                    None => provider,
                };
                info!(server = %name, "Configured OAuth auth for WebSocket MCP server");
                Some(Arc::new(provider))
            }
            None => None,
        };

        let handle = Arc::new(RemoteMcpServerHandle {
            name: name.to_string(),
            url: url.to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|e| {
                    tracing::error!("Failed to create HTTP client: {e}");
                    reqwest::Client::new()
                }),
            headers: resolved_headers,
            auth_provider,
            header_commands: HashMap::new(),
            state: Arc::new(RwLock::new(ServerState::Starting)),
            capabilities: Arc::new(RwLock::new(None)),
            protocol_version: Arc::new(RwLock::new(String::new())),
            next_id: AtomicU64::new(1),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            total_result_bytes: AtomicU64::new(0),
            budget_bytes: Arc::new(RwLock::new(None)),
            restart_count: Arc::new(AtomicU64::new(0)),
            max_restarts: self.max_restarts,
            started_at: Arc::new(RwLock::new(None)),
            request_timeout: self.request_timeout,
            tool_timeout: self.tool_timeout,
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(
                self.max_concurrent_per_server as usize,
            )),
            session_id: Arc::new(RwLock::new(None)),
            ws_transport: Some(Arc::new(Mutex::new(ws))),
            sampling_provider: self.sampling_provider.clone(),
            notification_tx: self.notification_tx.clone(),
        });

        handle.start().await?;
        self.remote_handles.insert(name.to_string(), handle);
        self.fire_event(McpEvent::new(
            McpEventType::ServerConnected,
            name.to_string(),
            None,
            serde_json::json!({"transport": "websocket"}),
        ))
        .await;
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
    ) -> shannon_tool_interface::ToolResult<shannon_tool_interface::ToolOutput> {
        let result = self
            .call_tool_with_limit(server_name, tool_name, arguments, self.max_output_chars)
            .await;

        // Fire ToolCallCompleted event.
        let (status, payload) = match &result {
            Ok(output) => (
                "success",
                serde_json::json!({
                    "is_error": output.is_error,
                    "content_length": output.content.len(),
                }),
            ),
            Err(e) => (
                "error",
                serde_json::json!({"error": e.to_string()}),
            ),
        };
        self.fire_event(McpEvent::new(
            McpEventType::ToolCallCompleted,
            server_name.to_string(),
            Some(tool_name.to_string()),
            serde_json::json!({"status": status, "result": payload}),
        ))
        .await;

        result
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
    ) -> shannon_tool_interface::ToolResult<shannon_tool_interface::ToolOutput> {
        use shannon_tool_interface::ToolError;

        // Budget check — reject early if server has exceeded its byte budget.
        if self.is_over_budget(server_name).await {
            return Err(ToolError::ExecutionFailed(format!(
                "MCP server '{server_name}' has exceeded its result byte budget"
            )));
        }

        // Fire ToolCallStarted event.
        self.fire_event(McpEvent::new(
            McpEventType::ToolCallStarted,
            server_name.to_string(),
            Some(tool_name.to_string()),
            serde_json::json!({"arguments_preview": arguments.to_string().chars().take(200).collect::<String>()}),
        ))
        .await;

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
                let args_clone = arguments.clone();
                return match remote.call_tool(tool_name, arguments).await {
                    Ok(output) => {
                        let byte_count = output.content.len() as u64;
                        let result = Ok(self.enforce_output_limit(
                            output,
                            max_chars,
                            &format!("mcp__{server_name}__{tool_name}"),
                        ));
                        self.track_result_bytes_for(server_name, byte_count);
                        result
                    }
                    Err(e) => {
                        *remote.state.write().await = ServerState::Unhealthy(e.to_string());
                        let restarts = remote.restart_count.fetch_add(1, Ordering::Relaxed) as u32;
                        if restarts < remote.max_restarts {
                            remote.reset().await;
                            if let Err(reinit_err) = remote.start().await {
                                warn!(server = %server_name, error = %reinit_err, "Remote server re-init failed");
                                return Err(e);
                            }
                            // Auto-retry once after reconnection
                            warn!(server = %server_name, tool = %tool_name, "Retrying tool call after reconnect");
                            match remote.call_tool(tool_name, args_clone).await {
                                Ok(output) => {
                                    *remote.state.write().await = ServerState::Healthy;
                                    let byte_count = output.content.len() as u64;
                                    let result = Ok(self.enforce_output_limit(
                                        output,
                                        max_chars,
                                        &format!("mcp__{server_name}__{tool_name}"),
                                    ));
                                    self.track_result_bytes_for(server_name, byte_count);
                                    result
                                }
                                Err(retry_err) => {
                                    *remote.state.write().await =
                                        ServerState::Unhealthy(retry_err.to_string());
                                    Err(retry_err)
                                }
                            }
                        } else {
                            Err(e)
                        }
                    }
                };
            }
            // Try re-initializing.
            remote.reset().await;
            remote.start().await.map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "Remote MCP server '{server_name}' restart failed: {e}"
                ))
            })?;
            let output = remote.call_tool(tool_name, arguments).await.map(|o| {
                self.enforce_output_limit(o, max_chars, &format!("mcp__{server_name}__{tool_name}"))
            })?;
            self.track_result_bytes_for(server_name, output.content.len() as u64);
            return Ok(output);
        }

        // Stdio handle.
        let handle = self
            .handles
            .get(server_name)
            .ok_or_else(|| ToolError::NotFound(format!("MCP server '{server_name}' not in pool")))?
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
                let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
                loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    match handle.get_state().await {
                        ServerState::Healthy => break,
                        ServerState::Stopped | ServerState::Unhealthy(_) => {
                            break;
                        }
                        ServerState::Starting if tokio::time::Instant::now() > deadline => {
                            return Err(ToolError::ExecutionFailed(format!(
                                "MCP server '{server_name}' timed out waiting to start"
                            )));
                        }
                        _ => continue,
                    }
                }
            }
        }

        let args_clone = arguments.clone();
        match handle.call_tool(tool_name, arguments).await {
            Ok(output) => {
                let byte_count = output.content.len() as u64;
                let result = Ok(self.enforce_output_limit(
                    output,
                    max_chars,
                    &format!("mcp__{server_name}__{tool_name}"),
                ));
                self.track_result_bytes_for(server_name, byte_count);
                result
            }
            Err(e) => {
                *handle.state.write().await = ServerState::Unhealthy(e.to_string());
                // Auto-retry once: restart the server and try again
                if let Ok(()) = self.restart_server(&handle).await {
                    warn!(server = %server_name, tool = %tool_name, "Retrying tool call after server restart");
                    match handle.call_tool(tool_name, args_clone).await {
                        Ok(output) => {
                            let byte_count = output.content.len() as u64;
                            let result = Ok(self.enforce_output_limit(
                                output,
                                max_chars,
                                &format!("mcp__{server_name}__{tool_name}"),
                            ));
                            self.track_result_bytes_for(server_name, byte_count);
                            result
                        }
                        Err(retry_err) => {
                            *handle.state.write().await =
                                ServerState::Unhealthy(retry_err.to_string());
                            Err(retry_err)
                        }
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Enforce maximum output size by truncating oversized results.
    /// Stores the full content in the result store for later retrieval.
    fn enforce_output_limit(
        &self,
        mut output: shannon_tool_interface::ToolOutput,
        max_chars: usize,
        tool_name: &str,
    ) -> shannon_tool_interface::ToolOutput {
        if output.content.len() > max_chars {
            let full_content = output.content.clone();
            let original_len = full_content.len();

            // Store the full content for chunked retrieval (in-memory).
            let chunk_id = self.result_store.store(tool_name, full_content.clone());

            // Persist to disk so the LLM can use the Read tool to access the full result.
            let disk_path = self.persist_result_to_disk(&chunk_id, &full_content, tool_name);

            // Use format-aware compression instead of simple truncation.
            let mut compressed = truncate_tool_result(&output.content, max_chars);
            // If compression didn't fit in budget, truncate_tool_result already handled it.
            // Replace the trailing [compressed: ...] marker with our own that includes
            // persistence info.
            if let Some(pos) = compressed.rfind("\n\n[compressed:") {
                let before = &compressed[..pos];
                if let Some(ref path) = disk_path {
                    compressed = format!(
                        "{}\n\n[compressed: showed ~{:.0}K of ~{:.0}K chars | full result saved to: {} | chunk_id={chunk_id}]",
                        before,
                        before.len() as f64 / 1024.0,
                        original_len as f64 / 1024.0,
                        path.display(),
                    );
                } else {
                    compressed = format!(
                        "{}\n\n[compressed: showed ~{:.0}K of ~{:.0}K chars | chunk_id={chunk_id}]",
                        before,
                        before.len() as f64 / 1024.0,
                        original_len as f64 / 1024.0,
                    );
                }
            }
            output.content = compressed;
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
    ) -> shannon_tool_interface::ToolResult<shannon_tool_interface::ToolOutput> {
        self.call_tool_with_progress_and_limit(
            server_name,
            tool_name,
            arguments,
            on_progress,
            self.max_output_chars,
        )
        .await
    }

    /// Call a tool with progress reporting and an explicit output limit.
    pub async fn call_tool_with_progress_and_limit(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
        on_progress: Arc<dyn Fn(f64, Option<f64>) + Send + Sync>,
        max_chars: usize,
    ) -> shannon_tool_interface::ToolResult<shannon_tool_interface::ToolOutput> {
        use shannon_tool_interface::ToolError;

        // Budget check.
        if self.is_over_budget(server_name).await {
            return Err(ToolError::ExecutionFailed(format!(
                "MCP server '{server_name}' has exceeded its result byte budget"
            )));
        }

        let handle = self
            .handles
            .get(server_name)
            .ok_or_else(|| ToolError::NotFound(format!("MCP server '{server_name}' not in pool")))?
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
                let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
                loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    match handle.get_state().await {
                        ServerState::Healthy => break,
                        ServerState::Stopped | ServerState::Unhealthy(_) => {
                            break;
                        }
                        ServerState::Starting if tokio::time::Instant::now() > deadline => {
                            return Err(ToolError::ExecutionFailed(format!(
                                "MCP server '{server_name}' timed out waiting to start"
                            )));
                        }
                        _ => continue,
                    }
                }
            }
        }

        match handle
            .call_tool_with_progress(tool_name, arguments, Some(on_progress))
            .await
        {
            Ok(output) => {
                let byte_count = output.content.len() as u64;
                let result = Ok(self.enforce_output_limit(
                    output,
                    max_chars,
                    &format!("mcp__{server_name}__{tool_name}"),
                ));
                self.track_result_bytes_for(server_name, byte_count);
                result
            }
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

    /// Send multiple JSON-RPC requests as a batch to a remote server.
    ///
    /// Uses a single HTTP POST with a JSON array of requests. Returns results
    /// matched to each request. Only supported for remote (HTTP) servers.
    pub async fn send_batch_server_request(
        &self,
        server_name: &str,
        requests: Vec<(&str, Value)>,
    ) -> Result<Vec<(u64, Result<Value, String>)>, String> {
        if let Some(remote) = self.remote_handles.get(server_name) {
            return remote
                .send_batch_request(requests, remote.request_timeout)
                .await;
        }
        // Stdio servers don't support batch — fall back to sequential.
        let mut results = Vec::with_capacity(requests.len());
        let handle = self
            .handles
            .get(server_name)
            .ok_or_else(|| format!("MCP server '{server_name}' not in pool"))?;
        for (method, params) in requests {
            let result = handle.send_request(method, params).await;
            match result {
                Ok(value) => {
                    let id = value.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                    results.push((id, Ok(value)));
                }
                Err(e) => results.push((0, Err(e))),
            }
        }
        Ok(results)
    }

    /// Call multiple tools on the same server as a batch.
    ///
    /// For remote (HTTP) servers, this uses a single HTTP POST with a JSON-RPC
    /// batch array. For stdio servers, it falls back to sequential calls.
    /// Returns results in the same order as the input.
    pub async fn call_tools_batch(
        &self,
        server_name: &str,
        tool_calls: Vec<(&str, Value)>,
    ) -> Vec<shannon_tool_interface::ToolResult<shannon_tool_interface::ToolOutput>> {
        use shannon_tool_interface::{ToolError, ToolOutput};

        if tool_calls.is_empty() {
            return Vec::new();
        }

        let count = tool_calls.len();
        let requests: Vec<(&str, Value)> = tool_calls
            .into_iter()
            .map(|(tool_name, args)| {
                (
                    "tools/call",
                    serde_json::json!({ "name": tool_name, "arguments": args }),
                )
            })
            .collect();

        match self.send_batch_server_request(server_name, requests).await {
            Ok(results) => results
                .into_iter()
                .map(|(_, result)| {
                    let value = result.map_err(ToolError::ExecutionFailed)?;
                    // Parse content the same way as RemoteMcpServerHandle::call_tool.
                    if let Some(result) = value.get("result") {
                        if let Some(content_array) =
                            result.get("content").and_then(|c| c.as_array())
                        {
                            let is_error = result
                                .get("isError")
                                .and_then(|e| e.as_bool())
                                .unwrap_or(false);
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
                            let content =
                                truncate_tool_result(&texts.join("\n"), MAX_TOOL_RESULT_CHARS);
                            if is_error {
                                return Ok(ToolOutput::error(content));
                            }
                            return Ok(ToolOutput::success(content));
                        }
                    }
                    Ok(ToolOutput::success(String::new()))
                })
                .collect(),
            Err(e) => (0..count)
                .map(|_| Err(ToolError::ExecutionFailed(e.clone())))
                .collect(),
        }
    }

    /// List prompts from a specific server via `prompts/list`.
    pub async fn list_prompts(&self, server_name: &str) -> Result<Vec<crate::Prompt>, String> {
        if !self.has_prompts(server_name).await {
            return Err(format!("Server '{server_name}' does not support prompts"));
        }
        let response = self
            .send_server_request(server_name, "prompts/list", serde_json::json!({}))
            .await?;
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
                .send_request_with_timeout(
                    "prompts/list",
                    serde_json::json!({}),
                    remote.request_timeout,
                )
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
        let response = self
            .send_server_request(server_name, "prompts/get", params)
            .await?;
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

    /// Set a callback invoked after a config hot-reload completes.
    ///
    /// The callback receives a list of human-readable change descriptions
    /// (e.g., "Started stdio server 'fetch'", "Removed server 'old'").
    pub async fn set_on_config_reloaded(&self, callback: Arc<dyn Fn(&[String]) + Send + Sync>) {
        *self.on_config_reloaded.lock().await = Some(callback);
    }

    /// Start a background task that watches MCP config files for changes
    /// and triggers hot-reload when modifications are detected.
    ///
    /// Polls every `interval` duration. Compares file modification times
    /// against the previously observed state to detect changes efficiently
    /// without adding a filesystem watcher dependency.
    pub fn start_config_watcher(self: &Arc<Self>, project_dir: PathBuf, interval: Duration) {
        let pool = self.clone();
        let search_paths = crate::config::config_search_paths(&project_dir);

        let task = tokio::spawn(async move {
            // Track last-seen modification times for each config path.
            let mut mtimes: HashMap<PathBuf, std::time::SystemTime> = HashMap::new();

            // Initialize with current state (skip first reload).
            for path in &search_paths {
                if let Ok(meta) = std::fs::metadata(path) {
                    if let Ok(modified) = meta.modified() {
                        mtimes.insert(path.clone(), modified);
                    }
                }
            }

            loop {
                tokio::time::sleep(interval).await;

                let mut changed = false;
                for path in &search_paths {
                    match std::fs::metadata(path) {
                        Ok(meta) => {
                            if let Ok(modified) = meta.modified() {
                                let prev = mtimes.get(path).copied();
                                if prev != Some(modified) {
                                    changed = true;
                                    mtimes.insert(path.clone(), modified);
                                }
                            }
                        }
                        Err(_) => {
                            // File removed — note it if we had a previous entry.
                            if mtimes.remove(path).is_some() {
                                changed = true;
                            }
                        }
                    }
                }

                if !changed {
                    continue;
                }

                info!("MCP config file change detected, reloading");

                match crate::config::discover_config(&project_dir) {
                    Ok(config) => match pool.reload_from_config(&config).await {
                        Ok(changes) => {
                            if !changes.is_empty() {
                                for change in &changes {
                                    info!(change = %change, "Config reload change");
                                }
                                let guard = pool.on_config_reloaded.lock().await;
                                if let Some(cb) = guard.as_ref() {
                                    cb(&changes);
                                }
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "Config reload failed");
                        }
                    },
                    Err(e) => {
                        warn!(error = %e, "Failed to discover MCP config for reload");
                    }
                }
            }
        });

        // Store the task handle — stop_config_watcher can cancel it.
        // We abuse the notification here by putting it in a separate lock.
        // Use a tokio::spawn to avoid holding the lock across .await
        let task_handle = self.config_watcher_task.clone();
        tokio::spawn(async move {
            let mut guard = task_handle.lock().await;
            if let Some(prev) = guard.take() {
                prev.abort();
            }
            *guard = Some(task);
        });
    }

    /// Stop the background config watcher task.
    pub async fn stop_config_watcher(&self) {
        let mut guard = self.config_watcher_task.lock().await;
        if let Some(task) = guard.take() {
            task.abort();
            info!("Config watcher stopped");
        }
    }

    /// Set a callback that is invoked when a server reports
    /// `notifications/tools/list_changed`.
    ///
    /// The callback receives `(server_name, new_tool_adapters)` — the caller
    /// should replace all existing tools from that server with the new adapters.
    pub async fn set_on_tools_changed(
        &self,
        callback: Arc<dyn Fn(&str, Vec<PooledMcpToolAdapter>) + Send + Sync>,
    ) {
        *self.on_tools_changed.lock().await = Some(callback);
    }

    /// Re-fetch the tool list from a connected server.
    ///
    /// Sends `tools/list` to the server, parses the response into new
    /// [`PooledMcpToolAdapter`] instances, updates deferred schemas if enabled,
    /// and returns the adapters. Returns an empty `Vec` if the server does not
    /// support tools or is not connected.
    pub async fn refresh_tools_for_server(
        self: &Arc<Self>,
        server_name: &str,
    ) -> Vec<PooledMcpToolAdapter> {
        if !self.has_tools(server_name).await {
            return Vec::new();
        }

        let response = match self
            .send_server_request(server_name, "tools/list", serde_json::json!({}))
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(
                    server = %server_name,
                    error = %e,
                    "Failed to refresh tools (tools/list)"
                );
                return Vec::new();
            }
        };

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

                let annotations: Option<crate::ToolAnnotations> = tool_value
                    .get("annotations")
                    .and_then(|a| serde_json::from_value(a.clone()).ok());

                let max_output_chars: Option<usize> = tool_value
                    .get("_meta")
                    .and_then(|m| m.get("maxResultSizeChars"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);

                let tool_timeout_secs: Option<u64> = tool_value
                    .get("_meta")
                    .and_then(|m| m.get("timeoutSeconds"))
                    .and_then(|v| v.as_u64());

                let desc_for_store = description.clone();
                let adapter = PooledMcpToolAdapter::with_output_limit(
                    self.clone(),
                    server_name.to_string(),
                    name.clone(),
                    description,
                    input_schema.clone(),
                    annotations,
                    max_output_chars,
                    tool_timeout_secs,
                );

                if self.is_defer_tool_schemas() {
                    self.store_deferred_schema(&adapter.tool_name, input_schema);
                    self.store_deferred_description(&adapter.tool_name, desc_for_store);
                }

                tools.push(adapter);
            }
        }

        info!(
            server = %server_name,
            tools = tools.len(),
            "Refreshed tool list from server"
        );

        tools
    }

    /// Set the roots provider callback.
    ///
    /// When a server sends a `roots/list` request, this callback is invoked
    /// to obtain the filesystem roots to return. If not set, an empty list
    /// is returned.
    pub async fn set_roots_provider(
        &self,
        provider: Arc<dyn Fn() -> Vec<crate::Root> + Send + Sync>,
    ) {
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
            response
                .get("result")
                .cloned()
                .unwrap_or(serde_json::json!({})),
        )
        .map_err(|e| format!("Failed to parse completion result: {e}"))
    }

    /// Send a `notifications/cancelled` to a server to cancel an in-progress request.
    ///
    /// Per MCP spec, this is a *notification* (no id, no response expected).
    /// The server should abort the request identified by `request_id` and
    /// may optionally clean up resources.
    pub async fn cancel_request(
        &self,
        server_name: &str,
        request_id: u64,
        reason: Option<&str>,
    ) -> Result<(), String> {
        let params = serde_json::json!({
            "requestId": request_id,
            "reason": reason,
        });

        // For remote servers, send a notification (no id → no response expected).
        if let Some(remote) = self.remote_handles.get(server_name) {
            remote
                .send_notification("notifications/cancelled", params)
                .await?;
            return Ok(());
        }

        // For stdio servers, write the notification to stdin.
        let handle = self
            .handles
            .get(server_name)
            .ok_or_else(|| format!("MCP server '{server_name}' not in pool"))?;

        handle
            .send_notification("notifications/cancelled", params)
            .await
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

    /// Set a byte budget for a specific server. Once the cumulative result bytes
    /// exceed this limit, further `call_tool` calls return an error.
    pub async fn set_server_budget(&self, server_name: &str, budget_bytes: u64) {
        if let Some(remote) = self.remote_handles.get(server_name) {
            *remote.budget_bytes.write().await = Some(budget_bytes);
        } else if let Some(handle) = self.handles.get(server_name) {
            *handle.budget_bytes.write().await = Some(budget_bytes);
        }
    }

    /// Get the total result bytes consumed by a server.
    pub async fn server_total_result_bytes(&self, server_name: &str) -> Option<u64> {
        if let Some(remote) = self.remote_handles.get(server_name) {
            Some(remote.total_result_bytes.load(Ordering::Relaxed))
        } else {
            self.handles
                .get(server_name)
                .map(|h| h.total_result_bytes.load(Ordering::Relaxed))
        }
    }

    /// Check whether a server has exceeded its byte budget.
    ///
    /// Returns `true` if the server has a budget set and the cumulative
    /// result bytes meet or exceed it. Servers without a budget always
    /// return `false`.
    pub async fn is_over_budget(&self, server_name: &str) -> bool {
        if let Some(remote) = self.remote_handles.get(server_name) {
            let budget = *remote.budget_bytes.read().await;
            return budget
                .map(|b| remote.total_result_bytes.load(Ordering::Relaxed) >= b)
                .unwrap_or(false);
        }
        if let Some(handle) = self.handles.get(server_name) {
            let budget = *handle.budget_bytes.read().await;
            return budget
                .map(|b| handle.total_result_bytes.load(Ordering::Relaxed) >= b)
                .unwrap_or(false);
        }
        false
    }

    /// Track result bytes for a server after a successful tool call.
    fn track_result_bytes_for(&self, server_name: &str, byte_count: u64) {
        if let Some(remote) = self.remote_handles.get(server_name) {
            remote
                .total_result_bytes
                .fetch_add(byte_count, Ordering::Relaxed);
        } else if let Some(handle) = self.handles.get(server_name) {
            handle
                .total_result_bytes
                .fetch_add(byte_count, Ordering::Relaxed);
        }
    }

    /// Look up a cached tool result. Returns `None` if not cached or expired.
    pub(crate) async fn get_cached(&self, key: &str) -> Option<String> {
        let cache = self.tool_cache.read().await;
        if let Some((value, timestamp)) = cache.get(key) {
            if timestamp.elapsed() < self.cache_ttl {
                return Some(value.clone());
            }
        }
        None
    }

    /// Store a tool result in the cache.
    ///
    /// Evicts expired entries when the cache exceeds 256 entries to prevent
    /// unbounded memory growth.
    pub(crate) async fn put_cached(&self, key: &str, value: String) {
        let mut cache = self.tool_cache.write().await;
        cache.insert(key.to_string(), (value, Instant::now()));
        if cache.len() > 256 {
            cache.retain(|_, (_, ts)| ts.elapsed() < self.cache_ttl);
        }
    }

    /// Start a background task that listens for server notifications
    /// and dispatches them to the appropriate callback.
    ///
    /// Currently handles:
    /// - `notifications/tools/list_changed` → re-fetches tools, calls `on_tools_changed`
    /// - `notifications/resources/list_changed` → invalidates resource cache
    /// - `notifications/prompts/list_changed` → logs for awareness
    pub fn start_notification_handler(self: &Arc<Self>) {
        let rx = self.notification_rx.clone();
        let on_tools_changed = self.on_tools_changed.clone();
        let tool_cache = self.tool_cache.clone();
        let event_publisher = self.event_publisher.clone();
        let subscriptions = self.subscriptions.clone();
        let pool = self.clone();

        tokio::spawn(async move {
            loop {
                let mut rx_guard = rx.lock().await;
                match rx_guard.recv().await {
                    Some((server_name, notification)) => {
                        drop(rx_guard);

                        // Fire notification event to webhooks.
                        if let Some(ref publisher) = event_publisher {
                            let event = McpEvent::new(
                                McpEventType::NotificationReceived,
                                server_name.clone(),
                                None,
                                notification.clone(),
                            );
                            publisher.publish(event).await;
                        }

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

                                // Re-fetch tools from the server.
                                let new_tools = pool.refresh_tools_for_server(&server_name).await;

                                let guard = on_tools_changed.lock().await;
                                if let Some(ref cb) = *guard {
                                    cb(&server_name, new_tools);
                                }
                            }
                            "notifications/resources/list_changed" => {
                                info!(
                                    server = %server_name,
                                    "Received resources/list_changed notification"
                                );
                            }
                            "notifications/resources/updated" => {
                                debug!(
                                    server = %server_name,
                                    "Received resources/updated notification"
                                );
                                subscriptions.handle_notification(&server_name, &notification);
                            }
                            "notifications/prompts/list_changed" => {
                                info!(
                                    server = %server_name,
                                    "Received prompts/list_changed notification"
                                );
                            }
                            "notifications/message" => {
                                // Forward MCP server log messages to our logging system.
                                let level = notification
                                    .get("params")
                                    .and_then(|p| p.get("level"))
                                    .and_then(|l| l.as_str())
                                    .unwrap_or("info");
                                let data = notification
                                    .get("params")
                                    .and_then(|p| p.get("data"))
                                    .and_then(|d| d.as_str())
                                    .unwrap_or("");
                                let logger = notification
                                    .get("params")
                                    .and_then(|p| p.get("logger"))
                                    .and_then(|l| l.as_str())
                                    .unwrap_or("");
                                let target = if logger.is_empty() {
                                    format!("mcp:{server_name}")
                                } else {
                                    format!("mcp:{server_name}:{logger}")
                                };
                                match level {
                                    "error" | "critical" | "alert" | "emergency" => {
                                        error!(target = %target, "{data}")
                                    }
                                    "warning" => {
                                        warn!(target = %target, "{data}")
                                    }
                                    "debug" => {
                                        debug!(target = %target, "{data}")
                                    }
                                    _ => {
                                        info!(target = %target, "{data}")
                                    }
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

    /// Get a reference to the resource subscription manager.
    pub fn subscriptions(&self) -> &ResourceSubscriptionManager {
        &self.subscriptions
    }

    /// Subscribe to updates for a resource URI on a specific server.
    ///
    /// Sends `resources/subscribe` to the MCP server and records the
    /// subscription locally. Returns an error if the server is not connected
    /// or does not support resource subscriptions.
    pub async fn subscribe_resource(
        &self,
        server_name: &str,
        resource_uri: &str,
    ) -> Result<(), String> {
        // Check that the server supports subscriptions.
        let caps = self.get_capabilities(server_name).await;
        let supports = caps
            .as_ref()
            .and_then(|c| c.resources.as_ref())
            .is_some_and(|r| r.subscribe);

        if !supports {
            return Err(format!(
                "Server '{server_name}' does not support resource subscriptions"
            ));
        }

        // Send the subscribe request.
        let params = serde_json::json!({ "uri": resource_uri });
        self.send_server_request(server_name, "resources/subscribe", params)
            .await?;

        // Record the subscription locally.
        self.subscriptions.subscribe(server_name, resource_uri);
        info!(
            server = %server_name,
            uri = %resource_uri,
            "Subscribed to resource"
        );
        Ok(())
    }

    /// Unsubscribe from updates for a resource URI.
    ///
    /// Sends `resources/unsubscribe` to the MCP server and removes the
    /// subscription from local tracking.
    pub async fn unsubscribe_resource(&self, resource_uri: &str) -> Result<(), String> {
        // Look up which server owns this subscription.
        let server_name = self
            .subscriptions
            .get_subscription(resource_uri)
            .map(|info| info.server_name.clone())
            .ok_or_else(|| format!("No active subscription for resource '{resource_uri}'"))?;

        // Send the unsubscribe request.
        let params = serde_json::json!({ "uri": resource_uri });
        self.send_server_request(&server_name, "resources/unsubscribe", params)
            .await?;

        // Remove the subscription locally.
        self.subscriptions.unsubscribe(resource_uri);
        info!(
            server = %server_name,
            uri = %resource_uri,
            "Unsubscribed from resource"
        );
        Ok(())
    }

    /// Gracefully shut down all servers (stdio + remote).
    ///
    /// For each stdio server this closes stdin, waits up to 2 s for the child
    /// process to exit, then force-kills it. Remote handles are simply dropped
    /// (no OS process to reap). Background tasks (health checker, config
    /// watcher) are also cancelled.
    pub async fn shutdown_all(&self) {
        info!("Shutting down all MCP servers");

        // Cancel background health-check task.
        {
            let mut guard = self.health_task.lock().await;
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }

        // Cancel background config-watcher task.
        {
            let mut guard = self.config_watcher_task.lock().await;
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }

        for entry in self.handles.iter() {
            entry.value().shutdown().await;
        }
        self.handles.clear();
        self.remote_handles.clear();

        // Clear all resource subscriptions.
        for sub in self.subscriptions.list_subscriptions() {
            self.subscriptions.unsubscribe(&sub.resource_uri);
        }
    }

    /// Stop a specific server by name.
    pub async fn stop_server(&self, name: &str) -> Result<(), String> {
        // Remove subscriptions for this server regardless of transport type.
        self.subscriptions.unsubscribe_all_for_server(name);

        if let Some((_, handle)) = self.handles.remove(name) {
            handle.shutdown().await;
            info!(server = %name, "Stopped MCP server");
            self.fire_event(McpEvent::new(
                McpEventType::ServerDisconnected,
                name.to_string(),
                None,
                serde_json::json!({"reason": "stopped"}),
            ))
            .await;
            return Ok(());
        }
        if self.remote_handles.remove(name).is_some() {
            info!(server = %name, "Removed remote MCP server");
            self.fire_event(McpEvent::new(
                McpEventType::ServerDisconnected,
                name.to_string(),
                None,
                serde_json::json!({"reason": "stopped"}),
            ))
            .await;
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
        let current_remote: std::collections::HashSet<String> = self
            .remote_handles
            .iter()
            .map(|e| e.key().clone())
            .collect();

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
                        self.start_remote_server(name, url, headers.clone(), auth.clone())
                            .await?;
                        changes.push(format!("Started remote server '{name}'"));
                    }
                }
                crate::config::McpServerConfig::WebSocket { url, auth } => {
                    if !is_current {
                        self.start_websocket_server(name, url, auth.clone()).await?;
                        changes.push(format!("Started WebSocket server '{name}'"));
                    }
                }
            }
        }

        // Update allowed tools patterns
        if !config.allowed_tools.is_empty() {
            self.set_allowed_patterns(config.allowed_tools.clone())
                .await;
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
            .is_some_and(|c| c.tools.is_some())
    }

    /// Check whether a server supports the `resources` capability.
    pub async fn has_resources(&self, server_name: &str) -> bool {
        self.get_capabilities(server_name)
            .await
            .is_some_and(|c| c.resources.is_some())
    }

    /// Check whether a server supports the `prompts` capability.
    pub async fn has_prompts(&self, server_name: &str) -> bool {
        self.get_capabilities(server_name)
            .await
            .is_some_and(|c| c.prompts.is_some())
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
        // Best-effort graceful cleanup: try to shut down child processes
        // cleanly before falling back to SIGKILL.
        //
        // Since Drop can't be async we spawn a detached thread that creates
        // its own small tokio runtime and runs the async shutdown with a hard
        // upper-bound timeout (5 s).  If we're somehow outside tokio we fall
        // back to immediate synchronous SIGKILL.

        if self.handles.is_empty() {
            return;
        }

        let handles: Vec<Arc<McpServerHandle>> =
            self.handles.iter().map(|e| e.value().clone()).collect();
        self.handles.clear();
        self.remote_handles.clear();

        // Clone the health_task handle so we can abort it from the cleanup thread.
        let health_task = self.health_task.clone();

        if tokio::runtime::Handle::try_current().is_ok() {
            // We're inside a tokio runtime — spawn a detached thread with its
            // own runtime to avoid the "cannot block_on inside async" panic.
            let _ = std::thread::Builder::new()
                .name("mcp-pool-shutdown".into())
                .spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_time()
                        .build();
                    let Ok(rt) = rt else { return };
                    rt.block_on(async {
                        // Abort the background health check task first.
                        if let Some(task) = health_task.lock().await.take() {
                            task.abort();
                        }

                        let _ = tokio::time::timeout(Duration::from_secs(5), async {
                            for h in &handles {
                                h.shutdown().await;
                            }
                        })
                        .await;
                    });
                });
        } else {
            // Outside tokio — fall back to synchronous SIGKILL.
            for h in &handles {
                if let Ok(mut child_guard) = h.child.try_lock() {
                    if let Some(ref mut child) = *child_guard {
                        let _ = child.start_kill();
                    }
                    *child_guard = None;
                }
            }
        }
    }
}

impl Default for McpProcessPool {
    fn default() -> Self {
        Self::new()
    }
}
