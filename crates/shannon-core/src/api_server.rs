//! HTTP/WebSocket API server for Shannon Code.
//!
//! Exposes REST, SSE, and WebSocket APIs so external tools and remote TUI
//! instances can interact with Shannon over the network.

use crate::VERSION;
use crate::query_engine::{
    PermissionRequest, QueryContext, QueryEngine, QueryEvent, QueryMetadata,
};
use crate::tools::ToolRegistry;
use axum::Json;
use axum::extract::State;
use axum::extract::ws::{Message as WsMsg, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use shannon_engine::api::{LlmClient, LlmClientConfig, Message};
use shannon_engine::permissions::{PermissionChoice, PermissionManager};
use shannon_engine::state::StateManager;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

// ── Request / Response types ───────────────────────────────────────────

/// JSON body for `POST /api/query`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    /// The user prompt to send to the LLM.
    pub prompt: String,
    /// Optional model override (e.g. `"claude-sonnet-4"`, `"gpt-4o"`).
    #[serde(default)]
    pub model: Option<String>,
    /// Optional client-supplied session identity (a UUID string). When omitted
    /// or unparseable the server mints a fresh UUID. Lets a caller attribute
    /// successive requests to the same conversation session; cross-request
    /// history persistence is wired up in P0-e (the contract lands here).
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Aggregated JSON response returned by `POST /api/query`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    /// The full text content produced by the LLM.
    pub text: String,
    /// The model that was used.
    pub model: String,
    /// Token usage breakdown.
    pub usage: Option<UsageInfo>,
    /// Any error that occurred (non-fatal accumulation).
    #[serde(default)]
    pub errors: Vec<String>,
    /// The session id attributed to this query — echoes the client-supplied
    /// `session_id` when one was provided, otherwise the freshly-minted UUID
    /// the server used. Lets callers record which session a stateless request
    /// was attributed to (`#[serde(default)]` keeps old payloads parseable).
    #[serde(default)]
    pub session_id: Uuid,
}

/// Token usage information included in the query response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

/// JSON response for `GET /api/health`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// JSON response for `GET /api/models`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsResponse {
    pub models: Vec<ModelInfo>,
}

/// Information about a single available model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
}

/// JSON response for `POST /api/tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsListResponse {
    pub tools: Vec<ToolEntry>,
}

/// Summary of a single registered tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEntry {
    pub name: String,
    pub description: String,
}

/// Generic error returned by all API endpoints.
#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

// ── Approval wire types (P0-b) ─────────────────────────────────────────

/// Wire representation of a human's approval decision for `POST
/// /api/approval/respond`. Decoupled from the engine's `PermissionChoice` so
/// the HTTP contract stays stable when the engine enum grows new variants.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ApprovalDecision {
    #[serde(rename = "allow_once")]
    AllowOnce,
    #[serde(rename = "always_allow")]
    AlwaysAllow,
    #[serde(rename = "deny")]
    Deny,
}

impl ApprovalDecision {
    fn to_choice(self) -> PermissionChoice {
        match self {
            ApprovalDecision::AllowOnce => PermissionChoice::AllowOnce,
            ApprovalDecision::AlwaysAllow => PermissionChoice::AlwaysAllow,
            ApprovalDecision::Deny => PermissionChoice::Deny,
        }
    }
}

/// JSON body for `POST /api/approval/respond`.
#[derive(Debug, Deserialize)]
pub struct ApprovalRespondRequest {
    pub request_id: String,
    pub choice: ApprovalDecision,
}

// ── Shared application state ───────────────────────────────────────────

/// Shared state accessible to all route handlers.
#[derive(Clone)]
pub struct AppState {
    /// LLM client configuration used for creating per-request clients.
    pub client_config: LlmClientConfig,
    /// Tool registry shared read-only for listing available tools.
    pub tools: Arc<ToolRegistry>,
    /// Active WebSocket sessions keyed by session ID.
    pub ws_sessions: Arc<RwLock<HashMap<String, Arc<Mutex<WsSession>>>>>,
    /// Pending tool-approval requests awaiting a `POST /api/approval/respond`
    /// decision, keyed by permission prompt id. The engine's approval channel
    /// (the `process_query` 2nd arg) forwards each `PermissionRequest` here via
    /// the WS handler; a per-request resolver task awaits the client's choice
    /// (300s timeout → `Deny`) and forwards it back to the engine.
    pub approval_registry: Arc<Mutex<HashMap<String, oneshot::Sender<PermissionChoice>>>>,
}

/// A single WebSocket session holding conversation history.
pub struct WsSession {
    /// Conversation messages accumulated across turns.
    pub messages: Vec<Message>,
    /// The model override for this session.
    pub model: Option<String>,
}

// ── WebSocket protocol messages ─────────────────────────────────────────

/// Incoming message from a WebSocket client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsClientMessage {
    /// Send a query to the LLM.
    #[serde(rename = "query")]
    Query {
        prompt: String,
        model: Option<String>,
        /// Optional session id override (UUID string). When omitted the
        /// connection's own session id is used. Lets a caller multiplex
        /// several conversations over a single socket.
        #[serde(default)]
        session_id: Option<String>,
    },
    /// Clear conversation history for this session.
    #[serde(rename = "clear")]
    Clear,
    /// Request current session info.
    #[serde(rename = "info")]
    Info,
    /// Cancel the current in-progress query.
    #[serde(rename = "cancel")]
    Cancel,
}

/// Outgoing message sent to a WebSocket client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsServerMessage {
    /// A text chunk from the LLM response.
    #[serde(rename = "text")]
    Text { content: String },
    /// Tool use event.
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        input: serde_json::Value,
    },
    /// Tool result event.
    #[serde(rename = "tool_result")]
    ToolResult { name: String, output: String },
    /// Token usage update.
    #[serde(rename = "usage")]
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
    },
    /// Query completed.
    #[serde(rename = "completed")]
    Completed { model: String },
    /// Query failed.
    #[serde(rename = "failed")]
    Failed { error: String },
    /// Query was cancelled by the client via `WsClientMessage::Cancel`. Emitted
    /// after the in-progress query's event stream has been dropped (which aborts
    /// the engine's producer task).
    #[serde(rename = "cancelled")]
    Cancelled,
    /// Engine requests human approval for a tool call. The client responds via
    /// `POST /api/approval/respond` with the matching `request_id`.
    #[serde(rename = "approval_request")]
    ApprovalRequest {
        request_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        description: String,
        is_destructive: bool,
        diff_preview: Option<String>,
    },
    /// Session info response.
    #[serde(rename = "session_info")]
    SessionInfo {
        message_count: usize,
        model: Option<String>,
    },
    /// Error in protocol.
    #[serde(rename = "error")]
    Error { message: String },
}

// ── ShannonApiServer ───────────────────────────────────────────────────

/// Builder-style server that wires up routes and starts listening.
pub struct ShannonApiServer {
    client_config: LlmClientConfig,
    tools: Arc<ToolRegistry>,
    host: String,
    port: u16,
    /// Optional bearer token. When set, every non-`/api/health` request must
    /// carry `Authorization: Bearer <token>`. `None` disables auth (loopback).
    auth_token: Option<String>,
    /// Allowed CORS origins. Empty = no `Access-Control-Allow-Origin` header
    /// (browser cross-origin denied; non-browser clients like the gateway are
    /// unaffected by CORS).
    allowed_origins: Vec<String>,
    /// Explicit opt-in to bind on a non-loopback interface. Defaults to
    /// `false`; when `true`, `serve()` additionally requires `auth_token`.
    allow_nonloopback: bool,
}

impl ShannonApiServer {
    /// Create a new server using the given LLM client configuration for every
    /// incoming query.
    pub fn new(client_config: LlmClientConfig) -> Self {
        Self {
            client_config,
            tools: Arc::new(ToolRegistry::new()),
            host: "127.0.0.1".to_string(),
            port: 8080,
            auth_token: None,
            allowed_origins: Vec::new(),
            allow_nonloopback: false,
        }
    }

    /// Provide a pre-populated [`ToolRegistry`] so that the `/api/tools/list`
    /// endpoint returns the real tool set.
    pub fn with_tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = Arc::new(tools);
        self
    }

    /// Override the bind host.
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Override the bind port.
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Require `Authorization: Bearer <token>` on every non-`/api/health`
    /// request. Required for any non-loopback bind (enforced by `serve()`).
    pub fn auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    /// Allow the listed origins to make cross-origin requests (sets
    /// `Access-Control-Allow-Origin`). Empty by default = deny-by-default;
    /// non-browser clients (the gateway) are unaffected by CORS.
    pub fn allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.allowed_origins = origins;
        self
    }

    /// Explicitly opt in to binding on a non-loopback interface. When set,
    /// `serve()` also requires [`Self::auth_token`].
    pub fn allow_nonloopback(mut self, allow: bool) -> Self {
        self.allow_nonloopback = allow;
        self
    }

    /// Build the `axum::Router` with all routes and middleware.
    fn build_router(&self) -> axum::Router<()> {
        let cors = build_cors_layer(&self.allowed_origins);

        axum::Router::new()
            .route("/api/health", get(health_handler))
            .route("/api/models", get(models_handler))
            .route("/api/query", post(query_handler))
            .route("/api/query/stream", get(query_stream_handler))
            .route("/api/tools/list", post(tools_list_handler))
            .route("/api/ws", get(ws_handler))
            .route("/api/approval/respond", post(approval_respond_handler))
            .layer(axum::middleware::from_fn_with_state(
                self.auth_token.clone(),
                auth_middleware,
            ))
            .layer(cors)
            .with_state(AppState {
                client_config: self.client_config.clone(),
                tools: self.tools.clone(),
                ws_sessions: Arc::new(RwLock::new(HashMap::new())),
                approval_registry: Arc::new(Mutex::new(HashMap::new())),
            })
    }

    /// Start the server and block until shutdown.
    pub async fn serve(self) -> Result<(), Box<dyn std::error::Error>> {
        self.validate_bind()?;
        let addr = format!("{}:{}", self.host, self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        tracing::info!("Shannon API server listening on {addr}");
        let router = self.build_router();
        axum::serve(listener, router).await?;
        Ok(())
    }

    /// Validate the bind configuration before opening the listener.
    ///
    /// Refuses any non-loopback host unless [`Self::allow_nonloopback`] is set,
    /// and additionally requires [`Self::auth_token`] for non-loopback binds.
    /// Loopback binds are always permitted — existing loopback deployments
    /// (desktop, local TUI) need no opt-in and no auth.
    fn validate_bind(&self) -> Result<(), Box<dyn std::error::Error>> {
        if is_loopback_host(&self.host) {
            return Ok(());
        }
        if !self.allow_nonloopback {
            return Err(format!(
                "refusing to bind non-loopback host '{}': set .allow_nonloopback(true) to opt in",
                self.host
            )
            .into());
        }
        if self.auth_token.is_none() {
            return Err(format!(
                "non-loopback host '{}' requires auth: set .auth_token(...) before binding",
                self.host
            )
            .into());
        }
        Ok(())
    }
}

/// Return true for loopback bind hosts (`127.0.0.0/8`, `::1`, `localhost`).
fn is_loopback_host(host: &str) -> bool {
    host == "localhost" || host == "::1" || host.starts_with("127.")
}

/// Constant-time string comparison to avoid timing oracles on token checks.
fn ct_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Build a CORS layer that is deny-by-default: with no origins configured it
/// emits no `Access-Control-Allow-Origin` header (browser cross-origin denied);
/// with a non-empty list it echoes a matched origin. Methods and headers stay
/// permissive — only origin is gated, since they don't widen read access.
fn build_cors_layer(origins: &[String]) -> CorsLayer {
    let mut cors = CorsLayer::new().allow_methods(Any).allow_headers(Any);
    if !origins.is_empty() {
        let parsed: Vec<HeaderValue> = origins
            .iter()
            .filter_map(|o| o.parse::<HeaderValue>().ok())
            .collect();
        cors = cors.allow_origin(parsed);
    }
    cors
}

/// Axum middleware enforcing the optional bearer-token auth policy.
///
/// `/api/health` is always public (liveness probes must work unauthenticated).
/// When the configured token (the middleware state) is `None`, all routes pass
/// through (loopback). Otherwise every request must carry
/// `Authorization: Bearer <token>` with a constant-time-equal match; a missing
/// or mismatched header yields `401`. The token is passed as an independent
/// `from_fn_with_state` state so it does not pollute the shared [`AppState`].
async fn auth_middleware(
    State(expected): State<Option<String>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, StatusCode> {
    if req.uri().path() == "/api/health" {
        return Ok(next.run(req).await);
    }
    let expected = match &expected {
        Some(t) => t,
        None => return Ok(next.run(req).await),
    };
    let provided = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.trim());
    match provided {
        Some(t) if ct_eq(t, expected) => Ok(next.run(req).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

// ── Route handlers ─────────────────────────────────────────────────────

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: VERSION.to_string(),
    })
}

async fn models_handler(State(state): State<AppState>) -> Json<ModelsResponse> {
    let provider_str = state.client_config.provider.to_string();
    let model = state.client_config.model.clone();

    // Return a small set of well-known models alongside the configured one.
    let mut models = vec![
        ModelInfo {
            id: "claude-sonnet-4".to_string(),
            provider: "anthropic".to_string(),
        },
        ModelInfo {
            id: "gpt-4o".to_string(),
            provider: "openai".to_string(),
        },
        ModelInfo {
            id: "llama3".to_string(),
            provider: "ollama".to_string(),
        },
    ];

    // Add the currently-configured model if it is not already in the list.
    if !models.iter().any(|m| m.id == model) {
        models.push(ModelInfo {
            id: model,
            provider: provider_str,
        });
    }

    Json(ModelsResponse { models })
}

/// Resolve the effective session id for an incoming query.
///
/// Prefer a client-supplied hint when it parses as a UUID; otherwise fall
/// back to `fallback` (a freshly-minted UUID for the HTTP endpoints, or the
/// connection's own session id for WebSocket). A malformed hint is silently
/// ignored rather than rejected — the session id is attribution metadata,
/// not an auth credential, so a bad hint must never block a query.
fn resolve_session_id(hint: Option<&str>, fallback: Uuid) -> Uuid {
    hint.and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or(fallback)
}

/// Attribute `engine` to the caller's session and restore any prior history.
///
/// The engine auto-saves the conversation to disk after each query under
/// `engine.session_id`. Aligning that id with the caller's `session_id`
/// means the save lands under a key the caller can reference on a later
/// stateless request. `restore_session` loads any prior history for that id
/// (and sets the id itself on hit); on first contact — no prior file — we
/// set the id directly so the auto-save still keys correctly instead of
/// writing under the constructor's random UUID. A disk read error is logged
/// and downgraded to a fresh start: history is a convenience, not a
/// correctness gate, so an unreadable file must never block a query.
fn attach_session(engine: &mut QueryEngine, session_id: Uuid) {
    let loaded = match engine.restore_session(session_id) {
        Ok(found) => found,
        Err(e) => {
            tracing::warn!(
                session = %session_id,
                "session history load failed: {e}; starting fresh"
            );
            false
        }
    };
    if !loaded {
        engine.session_id = session_id;
    }
}

async fn query_handler(
    State(state): State<AppState>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, ApiError> {
    if req.prompt.trim().is_empty() {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "prompt must not be empty".to_string(),
        });
    }

    let mut config = state.client_config.clone();
    if let Some(ref model) = req.model {
        config.model = model.clone();
    }

    let client = if config.provider.requires_auth() {
        LlmClient::new(config.clone())
    } else {
        LlmClient::new_unauthenticated(config.clone())
    };

    // Create a fresh engine per request (stateless).
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new();
    let state_mgr = StateManager::new();
    let mut engine = QueryEngine::with_defaults(client, tools, permissions, state_mgr);

    let session_id = resolve_session_id(req.session_id.as_deref(), Uuid::new_v4());
    attach_session(&mut engine, session_id);

    let context = QueryContext {
        query_id: Uuid::new_v4(),
        session_id,
        user_message: req.prompt,
        metadata: QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: true,
            max_tokens: None,
            model: config.model.clone(),
            temperature: None,
            top_p: None,
        },
    };

    let mut stream = engine.process_query(context, None).await;

    let mut text = String::new();
    let mut usage: Option<UsageInfo> = None;
    let mut errors: Vec<String> = Vec::new();

    while let Some(event_result) = stream.next().await {
        match event_result {
            Ok(QueryEvent::Text { content, .. }) => {
                text.push_str(&content);
            }
            Ok(QueryEvent::Usage {
                input_tokens,
                output_tokens,
                cost_usd,
                ..
            }) => {
                usage = Some(UsageInfo {
                    input_tokens,
                    output_tokens,
                    cost_usd,
                });
            }
            Ok(QueryEvent::Failed { error, .. }) => {
                errors.push(error);
            }
            Ok(_) => {}
            Err(e) => {
                errors.push(e.to_string());
            }
        }
    }

    Ok(Json(QueryResponse {
        text,
        model: config.model,
        usage,
        errors,
        session_id,
    }))
}

/// SSE streaming endpoint. The caller supplies `prompt` and optional `model`
/// as query parameters, e.g. `GET /api/query/stream?prompt=hello&model=llama3`.
async fn query_stream_handler(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let prompt = params.get("prompt").cloned().unwrap_or_default();
    if prompt.trim().is_empty() {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "prompt query parameter must not be empty".to_string(),
        });
    }

    let mut config = state.client_config.clone();
    if let Some(model) = params.get("model") {
        config.model = model.to_string();
    }

    let client = if config.provider.requires_auth() {
        LlmClient::new(config.clone())
    } else {
        LlmClient::new_unauthenticated(config.clone())
    };

    // Create a fresh engine per request (stateless).
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new();
    let state_mgr = StateManager::new();
    let mut engine = QueryEngine::with_defaults(client, tools, permissions, state_mgr);

    let session_id =
        resolve_session_id(params.get("session_id").map(String::as_str), Uuid::new_v4());
    attach_session(&mut engine, session_id);

    let context = QueryContext {
        query_id: Uuid::new_v4(),
        session_id,
        user_message: prompt,
        metadata: QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: true,
            max_tokens: None,
            model: config.model.clone(),
            temperature: None,
            top_p: None,
        },
    };

    let query_stream = engine.process_query(context, None).await;

    // Convert QueryEvent stream into SSE events.
    let sse_stream = query_stream.filter_map(|result| async move {
        match result {
            Ok(event) => {
                let event_type = match &event {
                    QueryEvent::Text { .. } => "text",
                    QueryEvent::ToolUseRequest { .. } => "tool_use_request",
                    QueryEvent::ToolUseResult { .. } => "tool_use_result",
                    QueryEvent::Usage { .. } => "usage",
                    QueryEvent::Completed { .. } => "completed",
                    QueryEvent::Failed { .. } => "failed",
                    QueryEvent::Progress { .. } => "progress",
                    QueryEvent::Cost { .. } => "cost",
                    QueryEvent::TurnCompleted { .. } => "turn_completed",
                    QueryEvent::Started { .. } => "started",
                    QueryEvent::ToolProgress { .. } => "tool_progress",
                    QueryEvent::Thinking { .. } => "thinking",
                    QueryEvent::Info { .. } => "info",
                    QueryEvent::RateLimit { .. } => "rate_limit",
                    QueryEvent::ConversationUpdate { .. } => "conversation_update",
                    QueryEvent::Warning { .. } => "warning",
                };
                let data = serde_json::to_string(&event).unwrap_or_default();
                Some(Ok(Event::default().event(event_type).data(data)))
            }
            Err(e) => {
                let data = serde_json::to_string(&serde_json::json!({ "error": e.to_string() }))
                    .unwrap_or_default();
                Some(Ok(Event::default().event("error").data(data)))
            }
        }
    });

    Ok(Sse::new(sse_stream).keep_alive(KeepAlive::default()))
}

async fn tools_list_handler(State(state): State<AppState>) -> Json<ToolsListResponse> {
    let tools = state
        .tools
        .list_tools_info()
        .into_iter()
        .map(|info| ToolEntry {
            name: info.name,
            description: info.description,
        })
        .collect();

    Json(ToolsListResponse { tools })
}

// ── WebSocket handler ────────────────────────────────────────────────────

/// HTTP upgrade handler for WebSocket connections.
///
/// Clients connect via `ws://host:port/api/ws` and send/receive JSON messages
/// using the [`WsClientMessage`] / [`WsServerMessage`] protocol.
/// `POST /api/approval/respond` — respond to an approval request previously
/// emitted as `WsServerMessage::ApprovalRequest`. Resolves the pending
/// resolver oneshot; if the request is unknown (already resolved, or the 300s
/// resolver timeout already elapsed and cleaned it up), returns 404.
async fn approval_respond_handler(
    State(state): State<AppState>,
    Json(body): Json<ApprovalRespondRequest>,
) -> impl IntoResponse {
    let mut registry = state.approval_registry.lock().await;
    match registry.remove(&body.request_id) {
        Some(tx) => {
            let _ = tx.send(body.choice.to_choice());
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "resolved"})),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!("approval request not found: {}", body.request_id)
            })),
        ),
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_socket(socket, state))
}

async fn handle_ws_socket(socket: WebSocket, state: AppState) {
    let session_id = uuid::Uuid::new_v4().to_string();
    let session = Arc::new(Mutex::new(WsSession {
        messages: Vec::new(),
        model: None,
    }));

    // Register session
    {
        let mut sessions = state.ws_sessions.write().await;
        sessions.insert(session_id.clone(), session.clone());
    }

    // Split the socket into independent send/receive halves so client messages
    // (notably `Cancel`) can be received *concurrently* with the query event
    // stream being drained and forwarded on the send half. The previous
    // single-socket loop called `socket.recv()` inside the query arm, so a
    // `Cancel` sent during a live query was never observed (the inner loop
    // blocked the outer receive).
    let (mut sender, mut receiver) = socket.split();

    // Send session greeting
    let _ = send_msg(
        &mut sender,
        WsServerMessage::SessionInfo {
            message_count: 0,
            model: None,
        },
    )
    .await;

    'outer: loop {
        let client_msg = match receiver.next().await {
            Some(Ok(WsMsg::Text(text))) => match serde_json::from_str::<WsClientMessage>(&text) {
                Ok(m) => m,
                Err(e) => {
                    let _ = send_msg(
                        &mut sender,
                        WsServerMessage::Error {
                            message: format!("Invalid message: {e}"),
                        },
                    )
                    .await;
                    continue;
                }
            },
            Some(Ok(WsMsg::Close(_))) | None => break,
            Some(Ok(_)) => continue,
            Some(Err(_)) => break,
        };

        match client_msg {
            WsClientMessage::Query {
                prompt,
                model,
                session_id: query_session_hint,
            } => {
                let mut config = state.client_config.clone();
                if let Some(ref m) = model {
                    config.model = m.clone();
                }
                // Update session model
                {
                    let mut s = session.lock().await;
                    if model.is_some() {
                        s.model = model.clone();
                    }
                }

                let client = if config.provider.requires_auth() {
                    LlmClient::new(config.clone())
                } else {
                    LlmClient::new_unauthenticated(config.clone())
                };

                let tools = ToolRegistry::new();
                let permissions = PermissionManager::new();
                let state_mgr = StateManager::new();
                let mut engine = QueryEngine::with_defaults(client, tools, permissions, state_mgr);

                // Restore conversation history
                {
                    let s = session.lock().await;
                    engine.restore_messages(s.messages.clone());
                }

                let effective_session_id = resolve_session_id(
                    query_session_hint.as_deref(),
                    uuid::Uuid::parse_str(&session_id).unwrap_or_default(),
                );

                let context = QueryContext {
                    query_id: uuid::Uuid::new_v4(),
                    session_id: effective_session_id,
                    user_message: prompt,
                    metadata: QueryMetadata {
                        timestamp: chrono::Utc::now(),
                        tools_allowed: true,
                        max_tokens: None,
                        model: config.model.clone(),
                        temperature: None,
                        top_p: None,
                    },
                };

                // Wire the engine's approval channel so tool calls requiring
                // human approval emit `ApprovalRequest` to the client. The
                // client responds via `POST /api/approval/respond`; a resolver
                // task (300s timeout → Deny) forwards the choice back to the
                // engine. See `claudedocs/social-connection-architecture.md` P0-b.
                let (perm_tx, mut perm_rx) = mpsc::unbounded_channel::<PermissionRequest>();
                // `process_query` returns a stream whose drop aborts the engine's
                // producer task — so dropping `stream` (on cancel, or when the
                // socket closes mid-query) actually interrupts the LLM/tool loop
                // instead of leaving it running (P0-c).
                let mut stream = engine.process_query(context, Some(perm_tx)).await;

                let mut cancelled = false;
                loop {
                    tokio::select! {
                        // QueryEvent stream — drain and forward to the client.
                        result_opt = stream.next() => {
                            let Some(result) = result_opt else { break };
                            let server_msg = match result {
                                Ok(QueryEvent::Text { content, .. }) => {
                                    Some(WsServerMessage::Text { content })
                                }
                                Ok(QueryEvent::ToolUseRequest {
                                    tool_name,
                                    tool_input,
                                    ..
                                }) => Some(WsServerMessage::ToolUse {
                                    name: tool_name,
                                    input: tool_input,
                                }),
                                Ok(QueryEvent::ToolUseResult {
                                    tool_name, result, ..
                                }) => Some(WsServerMessage::ToolResult {
                                    name: tool_name,
                                    output: result,
                                }),
                                Ok(QueryEvent::Usage {
                                    input_tokens,
                                    output_tokens,
                                    cost_usd,
                                    ..
                                }) => Some(WsServerMessage::Usage {
                                    input_tokens,
                                    output_tokens,
                                    cost_usd,
                                }),
                                Ok(QueryEvent::Completed { .. }) => Some(WsServerMessage::Completed {
                                    model: config.model.clone(),
                                }),
                                Ok(QueryEvent::Failed { error, .. }) => {
                                    Some(WsServerMessage::Failed { error })
                                }
                                Ok(_) => None,
                                Err(e) => Some(WsServerMessage::Failed {
                                    error: e.to_string(),
                                }),
                            };
                            if let Some(msg) = server_msg {
                                if !send_msg(&mut sender, msg).await {
                                    break 'outer;
                                }
                            }
                        }
                        // Engine requests human approval for a tool call.
                        req_opt = perm_rx.recv() => {
                            let Some(req) = req_opt else { continue };
                            let prompt = req.prompt;
                            let request_id = prompt.id.to_string();
                            let engine_response_tx = req.response_tx;
                            let (decision_tx, decision_rx) =
                                oneshot::channel::<PermissionChoice>();
                            state.approval_registry
                                .lock()
                                .await
                                .insert(request_id.clone(), decision_tx);
                            let areq = WsServerMessage::ApprovalRequest {
                                request_id: request_id.clone(),
                                tool_name: prompt.tool_name.clone(),
                                tool_input: prompt.tool_input.clone(),
                                description: prompt.description.clone(),
                                is_destructive: prompt.is_destructive,
                                diff_preview: prompt.diff_preview.clone(),
                            };
                            if !send_msg(&mut sender, areq).await {
                                break 'outer;
                            }
                            // Resolver: await the HTTP decision (300s), forward
                            // the choice to the engine, then clean up the entry.
                            let registry = state.approval_registry.clone();
                            let rid = request_id.clone();
                            tokio::spawn(async move {
                                let choice = match tokio::time::timeout(
                                    Duration::from_secs(300),
                                    decision_rx,
                                ).await {
                                    Ok(Ok(c)) => c,
                                    // Timeout, or the WS handler dropped the
                                    // resolver on socket close → deny.
                                    _ => PermissionChoice::Deny,
                                };
                                let _ = engine_response_tx.send(choice);
                                registry.lock().await.remove(&rid);
                            });
                        }
                        // Concurrent client messages while a query is live.
                        // `Cancel` interrupts the query (breaking the loop drops
                        // `stream`, aborting the producer). Any other message is
                        // rejected while busy; socket close tears down the
                        // whole session.
                        client = receiver.next() => match client {
                            Some(Ok(WsMsg::Text(t))) => {
                                match serde_json::from_str::<WsClientMessage>(&t) {
                                    Ok(WsClientMessage::Cancel) => {
                                        cancelled = true;
                                        break;
                                    }
                                    Ok(_) => {
                                        let _ = send_msg(
                                            &mut sender,
                                            WsServerMessage::Error {
                                                message: "query in progress; send {\"type\":\"cancel\"} to interrupt".to_string(),
                                            },
                                        )
                                        .await;
                                    }
                                    Err(_) => { /* ignore malformed mid-query */ }
                                }
                            }
                            Some(Ok(WsMsg::Close(_))) | None | Some(Err(_)) => break 'outer,
                            Some(Ok(_)) => { /* ping/binary — ignore */ }
                        }
                    }
                }

                // `stream` drops here → engine aborts the producer task.
                if cancelled {
                    let _ = send_msg(&mut sender, WsServerMessage::Cancelled).await;
                }

                // Persist conversation
                {
                    let mut s = session.lock().await;
                    s.messages = engine.conversation_messages().to_vec();
                }
            }
            WsClientMessage::Clear => {
                let mut s = session.lock().await;
                s.messages.clear();
                let info = WsServerMessage::SessionInfo {
                    message_count: 0,
                    model: s.model.clone(),
                };
                let _ = send_msg(&mut sender, info).await;
            }
            WsClientMessage::Info => {
                let s = session.lock().await;
                let info = WsServerMessage::SessionInfo {
                    message_count: s.messages.len(),
                    model: s.model.clone(),
                };
                let _ = send_msg(&mut sender, info).await;
            }
            WsClientMessage::Cancel => {
                // No query in progress — nothing to cancel.
                let _ = send_msg(
                    &mut sender,
                    WsServerMessage::Error {
                        message: "no active query to cancel".to_string(),
                    },
                )
                .await;
            }
        }
    }

    // Cleanup session
    {
        let mut sessions = state.ws_sessions.write().await;
        sessions.remove(&session_id);
    }
}

/// Serialize and send a [`WsServerMessage`] on the WebSocket send half.
/// Returns `false` if serialization or the send failed (typically a closed
/// socket), so the caller can tear the session down instead of repeating the
/// serialize/send boilerplate at every call site.
async fn send_msg(
    sink: &mut (impl futures::Sink<WsMsg, Error = axum::Error> + Unpin),
    msg: WsServerMessage,
) -> bool {
    match serde_json::to_string(&msg) {
        Ok(json) => sink.send(WsMsg::Text(json)).await.is_ok(),
        Err(e) => {
            tracing::debug!("WebSocket serialize server message failed: {e}");
            false
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use shannon_engine::api::{LlmProvider, MessageContent};
    use tower::ServiceExt;

    fn test_config() -> LlmClientConfig {
        LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: "http://localhost:11434".to_string(),
            model: "test-model".to_string(),
            provider: LlmProvider::Ollama,
            ..Default::default()
        }
    }

    fn test_app() -> Router<()> {
        ShannonApiServer::new(test_config()).build_router()
    }

    // ── Helper: read response body as bytes ──────────────────────────────

    async fn read_body(body: Body) -> Vec<u8> {
        axum::body::to_bytes(body, usize::MAX)
            .await
            .expect("failed to read body")
            .to_vec()
    }

    fn test_app_with_auth(token: &str) -> Router<()> {
        ShannonApiServer::new(test_config())
            .auth_token(token)
            .build_router()
    }

    fn test_app_with_origins(origins: &[&str]) -> Router<()> {
        ShannonApiServer::new(test_config())
            .allowed_origins(origins.iter().map(|s| s.to_string()).collect())
            .build_router()
    }

    // ══════════════════════════════════════════════════════════════════════
    // Bind policy (P0.2) — non-loopback requires opt-in + auth
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn validate_bind_loopback_ok_without_auth() {
        // Default host is 127.0.0.1 — loopback never needs opt-in or auth.
        let server = ShannonApiServer::new(test_config());
        assert!(server.validate_bind().is_ok());
    }

    #[test]
    fn validate_bind_loopback_variants_all_ok() {
        for host in ["127.0.0.1", "127.1.2.3", "localhost", "::1"] {
            let server = ShannonApiServer::new(test_config()).host(host);
            assert!(
                server.validate_bind().is_ok(),
                "expected '{host}' to be loopback"
            );
        }
    }

    #[test]
    fn validate_bind_nonloopback_refused_without_opt_in() {
        let server = ShannonApiServer::new(test_config()).host("0.0.0.0");
        let err = server.validate_bind().unwrap_err().to_string();
        assert!(
            err.contains("non-loopback") && err.contains("allow_nonloopback"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_bind_nonloopback_refused_without_auth() {
        let server = ShannonApiServer::new(test_config())
            .host("0.0.0.0")
            .allow_nonloopback(true);
        let err = server.validate_bind().unwrap_err().to_string();
        assert!(err.contains("requires auth"), "unexpected error: {err}");
    }

    #[test]
    fn validate_bind_nonloopback_ok_with_opt_in_and_auth() {
        let server = ShannonApiServer::new(test_config())
            .host("0.0.0.0")
            .allow_nonloopback(true)
            .auth_token("secret");
        assert!(server.validate_bind().is_ok());
    }

    // ══════════════════════════════════════════════════════════════════════
    // Auth middleware (P0.2) — bearer-token enforcement
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn auth_disabled_passes_through_without_token() {
        // No auth_token configured → /api/models reachable without a bearer.
        let app = test_app();
        let req = Request::builder()
            .uri("/api/models")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_health_remains_public_when_auth_enabled() {
        let app = test_app_with_auth("secret");
        let req = Request::builder()
            .uri("/api/health")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_rejects_request_without_bearer() {
        let app = test_app_with_auth("secret");
        let req = Request::builder()
            .uri("/api/models")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_rejects_request_with_wrong_bearer() {
        let app = test_app_with_auth("secret");
        let req = Request::builder()
            .uri("/api/models")
            .header("authorization", "Bearer wrong")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_accepts_request_with_correct_bearer() {
        let app = test_app_with_auth("secret");
        let req = Request::builder()
            .uri("/api/models")
            .header("authorization", "Bearer secret")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // ══════════════════════════════════════════════════════════════════════
    // CORS (P0.2) — deny-by-default, explicit allow list
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn cors_no_origins_denies_cross_origin() {
        // Default (empty origins) → no ACAO header even when an Origin is sent.
        let app = test_app();
        let req = Request::builder()
            .uri("/api/health")
            .header("origin", "http://evil.example")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none(),
            "no ACAO header expected when no origins are allowed"
        );
    }

    #[tokio::test]
    async fn cors_allowed_origin_is_echoed() {
        let app = test_app_with_origins(&["http://app.local"]);
        let req = Request::builder()
            .uri("/api/health")
            .header("origin", "http://app.local")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        let acao = response
            .headers()
            .get("access-control-allow-origin")
            .expect("expected ACAO header for allowed origin");
        assert_eq!(acao, "http://app.local");
    }

    // ══════════════════════════════════════════════════════════════════════
    // Health endpoint tests
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_health_endpoint_returns_ok_status() {
        let app = test_app();
        let req = Request::builder()
            .uri("/api/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = read_body(response.into_body()).await;
        let health: HealthResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(health.status, "ok");
        assert_eq!(health.version, VERSION);
    }

    #[tokio::test]
    async fn test_health_endpoint_returns_valid_json() {
        let app = test_app();
        let req = Request::builder()
            .uri("/api/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        let body = read_body(response.into_body()).await;
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed.get("status").is_some());
        assert!(parsed.get("version").is_some());
        assert!(parsed.get("status").unwrap().is_string());
        assert!(parsed.get("version").unwrap().is_string());
    }

    #[tokio::test]
    async fn test_health_endpoint_rejects_post() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    // ══════════════════════════════════════════════════════════════════════
    // Models endpoint tests
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_models_endpoint_returns_builtin_models() {
        let app = test_app();
        let req = Request::builder()
            .uri("/api/models")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = read_body(response.into_body()).await;
        let models: ModelsResponse = serde_json::from_slice(&body).unwrap();
        assert!(!models.models.is_empty());
        assert!(models.models.iter().any(|m| m.id == "claude-sonnet-4"));
        assert!(models.models.iter().any(|m| m.id == "gpt-4o"));
        assert!(models.models.iter().any(|m| m.id == "llama3"));
    }

    #[tokio::test]
    async fn test_models_endpoint_includes_configured_model() {
        let app = test_app();
        let req = Request::builder()
            .uri("/api/models")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        let body = read_body(response.into_body()).await;
        let models: ModelsResponse = serde_json::from_slice(&body).unwrap();
        // "test-model" is the configured model and should be present since
        // it differs from the three built-in ones.
        assert!(
            models
                .models
                .iter()
                .any(|m| m.id == "test-model" && m.provider == "ollama")
        );
    }

    #[tokio::test]
    async fn test_models_endpoint_deduplicates_configured_model() {
        // When the configured model matches a built-in one, it should not
        // appear twice.
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: "http://localhost:11434".to_string(),
            model: "claude-sonnet-4".to_string(),
            provider: LlmProvider::Anthropic,
            ..Default::default()
        };
        let app = ShannonApiServer::new(config).build_router();
        let req = Request::builder()
            .uri("/api/models")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        let body = read_body(response.into_body()).await;
        let models: ModelsResponse = serde_json::from_slice(&body).unwrap();
        let count = models
            .models
            .iter()
            .filter(|m| m.id == "claude-sonnet-4")
            .count();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_models_endpoint_rejects_post() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/models")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn test_models_endpoint_response_structure() {
        let app = test_app();
        let req = Request::builder()
            .uri("/api/models")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        let body = read_body(response.into_body()).await;
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let models_array = parsed.get("models").expect("missing models field");
        assert!(models_array.is_array());
        for model in models_array.as_array().unwrap() {
            assert!(model.get("id").is_some());
            assert!(model.get("provider").is_some());
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // Query endpoint validation tests
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_query_endpoint_empty_prompt_returns_bad_request() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/query")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"prompt": ""}"#))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_query_endpoint_whitespace_prompt_returns_bad_request() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/query")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"prompt": "   "}"#))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_query_endpoint_error_response_format() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/query")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"prompt": ""}"#))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        let body = read_body(response.into_body()).await;
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed.get("error").is_some());
        assert!(parsed.get("error").unwrap().is_string());
        let error_msg = parsed.get("error").unwrap().as_str().unwrap();
        assert!(!error_msg.is_empty());
    }

    #[tokio::test]
    async fn test_query_endpoint_missing_body_returns_4xx() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/query")
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        // axum returns 400 for missing body when JSON is expected
        assert!(response.status().is_client_error());
    }

    #[tokio::test]
    async fn test_query_endpoint_malformed_json_returns_4xx() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/query")
            .header("content-type", "application/json")
            .body(Body::from(r#"not valid json"#))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert!(response.status().is_client_error());
    }

    #[tokio::test]
    async fn test_query_endpoint_missing_prompt_field_returns_4xx() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/query")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"model": "gpt-4o"}"#))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert!(response.status().is_client_error());
    }

    #[tokio::test]
    async fn test_query_endpoint_wrong_content_type_returns_4xx() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/query")
            .header("content-type", "text/plain")
            .body(Body::from(r#"{"prompt": "hello"}"#))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert!(response.status().is_client_error());
    }

    #[tokio::test]
    async fn test_query_endpoint_get_method_rejected() {
        let app = test_app();
        let req = Request::builder()
            .uri("/api/query")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    // ══════════════════════════════════════════════════════════════════════
    // session_id pass-through (P0-d)
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_resolve_session_id_uses_valid_client_hint() {
        let id = Uuid::new_v4();
        let fallback = Uuid::new_v4();
        assert_eq!(resolve_session_id(Some(&id.to_string()), fallback), id);
    }

    #[test]
    fn test_resolve_session_id_falls_back_when_hint_absent() {
        let fallback = Uuid::new_v4();
        assert_eq!(resolve_session_id(None, fallback), fallback);
    }

    #[test]
    fn test_resolve_session_id_falls_back_when_hint_unparseable() {
        let fallback = Uuid::new_v4();
        assert_eq!(resolve_session_id(Some("not-a-uuid"), fallback), fallback);
    }

    #[test]
    fn test_query_request_deserializes_session_id_field() {
        let id = Uuid::new_v4();
        let json = format!(r#"{{"prompt": "hi", "session_id": "{id}"}}"#);
        let req: QueryRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req.session_id, Some(id.to_string()));
    }

    #[test]
    fn test_query_request_session_id_defaults_to_none() {
        let req: QueryRequest = serde_json::from_str(r#"{"prompt": "hi"}"#).unwrap();
        assert!(req.session_id.is_none());
    }

    #[test]
    fn test_query_response_round_trips_session_id() {
        let id = Uuid::new_v4();
        let resp = QueryResponse {
            text: "hello".to_string(),
            model: "m".to_string(),
            usage: None,
            errors: Vec::new(),
            session_id: id,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: QueryResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.session_id, id);
    }

    #[test]
    fn test_ws_query_message_round_trips_session_id() {
        let id = Uuid::new_v4();
        let json = format!(r#"{{"type": "query", "prompt": "hi", "session_id": "{id}"}}"#);
        let msg: WsClientMessage = serde_json::from_str(&json).unwrap();
        match msg {
            WsClientMessage::Query { session_id, .. } => {
                assert_eq!(session_id, Some(id.to_string()));
            }
            other => panic!("expected Query, got {other:?}"),
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // session history persistence (P0-e)
    // ══════════════════════════════════════════════════════════════════════

    fn p0e_temp_sessions_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("shannon-api-server-p0e")
            .join(Uuid::new_v4().to_string());
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_attach_session_loads_prior_history_and_aligns_id() {
        use shannon_engine::api::{Message, MessageContent};
        use shannon_engine::state::SessionPersistMetadata;

        let dir = p0e_temp_sessions_dir();
        let state = StateManager::with_sessions_dir(dir).unwrap();

        // Persist a prior turn under session_id.
        let session_id = Uuid::new_v4();
        let prior = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("hello from the past".to_string()),
        }];
        state
            .save_session(
                &session_id,
                &prior,
                &SessionPersistMetadata {
                    model: "m".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();

        // Fresh engine sharing the same disk-backed state.
        let mut engine = QueryEngine::with_defaults(
            LlmClient::new(test_config()),
            ToolRegistry::new(),
            PermissionManager::new(),
            state,
        );

        // Before: the engine has its own random id and no history.
        assert_ne!(engine.session_id, session_id);
        assert!(engine.conversation.messages.is_empty());

        attach_session(&mut engine, session_id);

        // After: history restored, save key aligned to the caller's session.
        assert_eq!(engine.session_id, session_id);
        assert_eq!(engine.conversation.messages.len(), 1);
    }

    #[test]
    fn test_attach_session_first_contact_aligns_id_without_history() {
        // No prior file → restore_session returns false. The engine's
        // session_id must still be aligned so the post-query auto-save lands
        // under the caller's key (not the constructor's random UUID), which
        // is what makes a later request able to load this conversation.
        let dir = p0e_temp_sessions_dir();
        let state = StateManager::with_sessions_dir(dir).unwrap();

        let mut engine = QueryEngine::with_defaults(
            LlmClient::new(test_config()),
            ToolRegistry::new(),
            PermissionManager::new(),
            state,
        );

        let session_id = Uuid::new_v4();
        assert_ne!(engine.session_id, session_id);

        attach_session(&mut engine, session_id);

        assert_eq!(engine.session_id, session_id);
        assert!(engine.conversation.messages.is_empty());
    }

    // ══════════════════════════════════════════════════════════════════════
    // SSE streaming endpoint tests
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_query_stream_endpoint_empty_prompt_returns_bad_request() {
        let app = test_app();
        let req = Request::builder()
            .uri("/api/query/stream")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_query_stream_endpoint_whitespace_prompt_returns_bad_request() {
        let app = test_app();
        let req = Request::builder()
            .uri("/api/query/stream?prompt=%20%20%20")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_query_stream_endpoint_post_method_rejected() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/query/stream?prompt=hello")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn test_query_stream_endpoint_error_response_format() {
        let app = test_app();
        let req = Request::builder()
            .uri("/api/query/stream")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        let body = read_body(response.into_body()).await;
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed.get("error").is_some());
    }

    // ══════════════════════════════════════════════════════════════════════
    // Tools list endpoint tests
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_tools_list_endpoint_empty_registry() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/tools/list")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = read_body(response.into_body()).await;
        let tools: ToolsListResponse = serde_json::from_slice(&body).unwrap();
        assert!(tools.tools.is_empty());
    }

    #[tokio::test]
    async fn test_tools_list_endpoint_get_method_rejected() {
        let app = test_app();
        let req = Request::builder()
            .uri("/api/tools/list")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn test_tools_list_endpoint_response_structure() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/tools/list")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        let body = read_body(response.into_body()).await;
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed.get("tools").is_some());
        assert!(parsed.get("tools").unwrap().is_array());
    }

    // ══════════════════════════════════════════════════════════════════════
    // WebSocket endpoint tests
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_ws_endpoint_returns_upgrade_required_without_upgrade_headers() {
        let app = test_app();
        // A plain GET without Upgrade headers should return 4xx or similar,
        // since the ws handler requires an upgrade.
        let req = Request::builder()
            .uri("/api/ws")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        // axum's WebSocketUpgrade returns 400 if required headers are missing
        assert!(response.status().is_client_error());
    }

    #[tokio::test]
    async fn test_ws_endpoint_post_method_rejected() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/ws")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    // ══════════════════════════════════════════════════════════════════════
    // Unknown route tests
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_unknown_route_returns_not_found() {
        let app = test_app();
        let req = Request::builder()
            .uri("/api/nonexistent")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_root_route_returns_not_found() {
        let app = test_app();
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // ══════════════════════════════════════════════════════════════════════
    // Request/response type serialization tests
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_query_request_serialization() {
        let req = QueryRequest {
            prompt: "hello world".to_string(),
            model: Some("gpt-4o".to_string()),
            session_id: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("hello world"));
        assert!(json.contains("gpt-4o"));

        let deserialized: QueryRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.prompt, "hello world");
        assert_eq!(deserialized.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn test_query_request_model_defaults_to_none() {
        let json = r#"{"prompt": "test"}"#;
        let req: QueryRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.prompt, "test");
        assert!(req.model.is_none());
    }

    #[test]
    fn test_query_response_serialization() {
        let resp = QueryResponse {
            text: "response text".to_string(),
            model: "test-model".to_string(),
            usage: Some(UsageInfo {
                input_tokens: 100,
                output_tokens: 50,
                cost_usd: 0.005,
            }),
            errors: vec![],
            session_id: Uuid::new_v4(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["text"], "response text");
        assert_eq!(parsed["model"], "test-model");
        assert_eq!(parsed["usage"]["input_tokens"], 100);
        assert_eq!(parsed["usage"]["output_tokens"], 50);
        assert_eq!(parsed["errors"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_query_response_with_errors() {
        let resp = QueryResponse {
            text: String::new(),
            model: "test-model".to_string(),
            usage: None,
            errors: vec!["something went wrong".to_string()],
            session_id: Uuid::new_v4(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["usage"].is_null());
        assert_eq!(parsed["errors"][0], "something went wrong");
    }

    #[test]
    fn test_health_response_serialization() {
        let resp = HealthResponse {
            status: "ok".to_string(),
            version: "1.0.0".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: HealthResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.status, "ok");
        assert_eq!(deserialized.version, "1.0.0");
    }

    #[test]
    fn test_models_response_serialization() {
        let resp = ModelsResponse {
            models: vec![
                ModelInfo {
                    id: "gpt-4o".to_string(),
                    provider: "openai".to_string(),
                },
                ModelInfo {
                    id: "llama3".to_string(),
                    provider: "ollama".to_string(),
                },
            ],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: ModelsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.models.len(), 2);
        assert_eq!(deserialized.models[0].id, "gpt-4o");
        assert_eq!(deserialized.models[1].provider, "ollama");
    }

    #[test]
    fn test_tools_list_response_serialization() {
        let resp = ToolsListResponse {
            tools: vec![ToolEntry {
                name: "bash".to_string(),
                description: "Execute shell commands".to_string(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: ToolsListResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tools.len(), 1);
        assert_eq!(deserialized.tools[0].name, "bash");
    }

    #[test]
    fn test_usage_info_serialization() {
        let info = UsageInfo {
            input_tokens: 500,
            output_tokens: 200,
            cost_usd: 0.0123,
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: UsageInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.input_tokens, 500);
        assert_eq!(parsed.output_tokens, 200);
        assert!((parsed.cost_usd - 0.0123).abs() < f64::EPSILON);
    }

    // ══════════════════════════════════════════════════════════════════════
    // WebSocket message serialization tests
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_ws_client_message_query_serialization() {
        let msg = WsClientMessage::Query {
            prompt: "hello".to_string(),
            model: Some("gpt-4o".to_string()),
            session_id: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "query");
        assert_eq!(parsed["prompt"], "hello");
        assert_eq!(parsed["model"], "gpt-4o");
    }

    #[test]
    fn test_ws_client_message_query_without_model() {
        let msg = WsClientMessage::Query {
            prompt: "test".to_string(),
            model: None,
            session_id: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "query");
        assert!(parsed.get("model").unwrap().is_null());
    }

    #[test]
    fn test_ws_client_message_clear() {
        let msg = WsClientMessage::Clear;
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "clear");
    }

    #[test]
    fn test_ws_client_message_info() {
        let msg = WsClientMessage::Info;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"info""#));
    }

    #[test]
    fn test_ws_client_message_cancel() {
        let msg = WsClientMessage::Cancel;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"cancel""#));
    }

    #[test]
    fn test_ws_client_message_roundtrip() {
        let messages = vec![
            WsClientMessage::Query {
                prompt: "test prompt".to_string(),
                model: Some("llama3".to_string()),
                session_id: None,
            },
            WsClientMessage::Clear,
            WsClientMessage::Info,
            WsClientMessage::Cancel,
        ];
        for msg in messages {
            let json = serde_json::to_string(&msg).unwrap();
            let roundtrip: WsClientMessage = serde_json::from_str(&json).unwrap();
            let json2 = serde_json::to_string(&roundtrip).unwrap();
            assert_eq!(json, json2);
        }
    }

    #[test]
    fn test_ws_client_message_invalid_type_returns_error() {
        let json = r#"{"type": "unknown_type"}"#;
        let result = serde_json::from_str::<WsClientMessage>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_ws_client_message_missing_type_returns_error() {
        let json = r#"{"prompt": "hello"}"#;
        let result = serde_json::from_str::<WsClientMessage>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_ws_server_message_text() {
        let msg = WsServerMessage::Text {
            content: "hello world".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "text");
        assert_eq!(parsed["content"], "hello world");
    }

    #[test]
    fn test_ws_server_message_tool_use() {
        let msg = WsServerMessage::ToolUse {
            name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "tool_use");
        assert_eq!(parsed["name"], "bash");
        assert_eq!(parsed["input"]["command"], "ls");
    }

    #[test]
    fn test_ws_server_message_tool_result() {
        let msg = WsServerMessage::ToolResult {
            name: "bash".to_string(),
            output: "file1.txt\nfile2.txt".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "tool_result");
        assert_eq!(parsed["output"], "file1.txt\nfile2.txt");
    }

    #[test]
    fn test_ws_server_message_usage() {
        let msg = WsServerMessage::Usage {
            input_tokens: 100,
            output_tokens: 50,
            cost_usd: 0.003,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "usage");
        assert_eq!(parsed["input_tokens"], 100);
        assert_eq!(parsed["output_tokens"], 50);
    }

    #[test]
    fn test_ws_server_message_completed() {
        let msg = WsServerMessage::Completed {
            model: "claude-sonnet-4".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "completed");
        assert_eq!(parsed["model"], "claude-sonnet-4");
    }

    #[test]
    fn test_ws_server_message_failed() {
        let msg = WsServerMessage::Failed {
            error: "timeout".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "failed");
        assert_eq!(parsed["error"], "timeout");
    }

    #[test]
    fn test_ws_server_message_session_info() {
        let msg = WsServerMessage::SessionInfo {
            message_count: 5,
            model: Some("gpt-4o".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "session_info");
        assert_eq!(parsed["message_count"], 5);
        assert_eq!(parsed["model"], "gpt-4o");
    }

    #[test]
    fn test_ws_server_message_session_info_no_model() {
        let msg = WsServerMessage::SessionInfo {
            message_count: 0,
            model: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "session_info");
        assert!(parsed.get("model").unwrap().is_null());
    }

    #[test]
    fn test_ws_server_message_error() {
        let msg = WsServerMessage::Error {
            message: "something failed".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["message"], "something failed");
    }

    #[test]
    fn test_ws_server_message_roundtrip_all_variants() {
        let messages = vec![
            WsServerMessage::Text {
                content: "hi".to_string(),
            },
            WsServerMessage::ToolUse {
                name: "read".to_string(),
                input: serde_json::json!({"path": "/tmp"}),
            },
            WsServerMessage::ToolResult {
                name: "read".to_string(),
                output: "contents".to_string(),
            },
            WsServerMessage::Usage {
                input_tokens: 10,
                output_tokens: 5,
                cost_usd: 0.001,
            },
            WsServerMessage::Completed {
                model: "test".to_string(),
            },
            WsServerMessage::Failed {
                error: "err".to_string(),
            },
            WsServerMessage::SessionInfo {
                message_count: 3,
                model: Some("m".to_string()),
            },
            WsServerMessage::Error {
                message: "bad".to_string(),
            },
            WsServerMessage::Cancelled,
        ];
        for msg in messages {
            let json = serde_json::to_string(&msg).unwrap();
            let roundtrip: WsServerMessage = serde_json::from_str(&json).unwrap();
            let json2 = serde_json::to_string(&roundtrip).unwrap();
            assert_eq!(json, json2);
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // ApiError tests
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_api_error_into_response() {
        let error = ApiError {
            status: StatusCode::NOT_FOUND,
            message: "resource not found".to_string(),
        };
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = read_body(Body::new(response.into_body())).await;
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"], "resource not found");
    }

    #[tokio::test]
    async fn test_api_error_internal_server() {
        let error = ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "internal failure".to_string(),
        };
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = read_body(Body::new(response.into_body())).await;
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"], "internal failure");
    }

    #[tokio::test]
    async fn test_api_error_bad_request() {
        let error = ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "prompt must not be empty".to_string(),
        };
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // ══════════════════════════════════════════════════════════════════════
    // ShannonApiServer builder tests
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_server_new_default_values() {
        let server = ShannonApiServer::new(test_config());
        assert_eq!(server.host, "127.0.0.1");
        assert_eq!(server.port, 8080);
    }

    #[test]
    fn test_server_host_builder() {
        let server = ShannonApiServer::new(test_config()).host("0.0.0.0");
        assert_eq!(server.host, "0.0.0.0");
    }

    #[test]
    fn test_server_port_builder() {
        let server = ShannonApiServer::new(test_config()).port(3000);
        assert_eq!(server.port, 3000);
    }

    #[test]
    fn test_server_with_tools_builder() {
        let registry = ToolRegistry::new();
        let server = ShannonApiServer::new(test_config()).with_tools(registry);
        assert_eq!(Arc::strong_count(&server.tools), 1);
    }

    #[test]
    fn test_server_builder_chaining() {
        let server = ShannonApiServer::new(test_config())
            .host("0.0.0.0")
            .port(9090);
        assert_eq!(server.host, "0.0.0.0");
        assert_eq!(server.port, 9090);
    }

    // ══════════════════════════════════════════════════════════════════════
    // AppState tests
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_app_state_is_clone() {
        let state = AppState {
            client_config: test_config(),
            tools: Arc::new(ToolRegistry::new()),
            ws_sessions: Arc::new(RwLock::new(HashMap::new())),
            approval_registry: Arc::new(Mutex::new(HashMap::new())),
        };
        let cloned = state.clone();
        assert!(Arc::ptr_eq(&state.tools, &cloned.tools));
        assert!(Arc::ptr_eq(&state.ws_sessions, &cloned.ws_sessions));
    }

    #[tokio::test]
    async fn test_app_state_ws_sessions_initially_empty() {
        let state = AppState {
            client_config: test_config(),
            tools: Arc::new(ToolRegistry::new()),
            ws_sessions: Arc::new(RwLock::new(HashMap::new())),
            approval_registry: Arc::new(Mutex::new(HashMap::new())),
        };
        let sessions = state.ws_sessions.read().await;
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_app_state_ws_sessions_insert_and_remove() {
        let state = AppState {
            client_config: test_config(),
            tools: Arc::new(ToolRegistry::new()),
            ws_sessions: Arc::new(RwLock::new(HashMap::new())),
            approval_registry: Arc::new(Mutex::new(HashMap::new())),
        };

        let session = Arc::new(Mutex::new(WsSession {
            messages: vec![],
            model: None,
        }));

        // Insert
        {
            let mut sessions = state.ws_sessions.write().await;
            sessions.insert("test-session".to_string(), session.clone());
        }

        // Verify inserted
        {
            let sessions = state.ws_sessions.read().await;
            assert_eq!(sessions.len(), 1);
            assert!(sessions.contains_key("test-session"));
        }

        // Remove
        {
            let mut sessions = state.ws_sessions.write().await;
            sessions.remove("test-session");
        }

        // Verify removed
        {
            let sessions = state.ws_sessions.read().await;
            assert!(sessions.is_empty());
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // Approval roundtrip tests (P0-b)
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_approval_decision_maps_to_permission_choice() {
        assert_eq!(
            ApprovalDecision::AllowOnce.to_choice(),
            PermissionChoice::AllowOnce
        );
        assert_eq!(
            ApprovalDecision::AlwaysAllow.to_choice(),
            PermissionChoice::AlwaysAllow
        );
        assert_eq!(ApprovalDecision::Deny.to_choice(), PermissionChoice::Deny);
    }

    #[test]
    fn test_approval_decision_serde_round_trip() {
        for (decision, wire) in [
            (ApprovalDecision::AllowOnce, "allow_once"),
            (ApprovalDecision::AlwaysAllow, "always_allow"),
            (ApprovalDecision::Deny, "deny"),
        ] {
            let json = serde_json::to_string(&decision).unwrap();
            assert_eq!(json, format!("\"{wire}\""));
            let back: ApprovalDecision = serde_json::from_str(&json).unwrap();
            assert_eq!(back, decision);
        }
    }

    #[test]
    fn test_ws_server_message_approval_request_serialization() {
        let msg = WsServerMessage::ApprovalRequest {
            request_id: "abc-123".to_string(),
            tool_name: "bash".to_string(),
            tool_input: serde_json::json!({"command": "ls"}),
            description: "Run a shell command".to_string(),
            is_destructive: true,
            diff_preview: Some("--- old\n+++ new".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "approval_request");
        assert_eq!(parsed["request_id"], "abc-123");
        assert_eq!(parsed["tool_name"], "bash");
        assert_eq!(parsed["is_destructive"], true);
        assert_eq!(parsed["diff_preview"], "--- old\n+++ new");
    }

    #[tokio::test]
    async fn test_approval_respond_resolves_pending_request() {
        let state = AppState {
            client_config: test_config(),
            tools: Arc::new(ToolRegistry::new()),
            ws_sessions: Arc::new(RwLock::new(HashMap::new())),
            approval_registry: Arc::new(Mutex::new(HashMap::new())),
        };
        let request_id = "test-approval-1".to_string();
        let (tx, rx) = oneshot::channel::<PermissionChoice>();
        state
            .approval_registry
            .lock()
            .await
            .insert(request_id.clone(), tx);

        let body = ApprovalRespondRequest {
            request_id: request_id.clone(),
            choice: ApprovalDecision::AllowOnce,
        };
        let response = approval_respond_handler(State(state.clone()), Json(body))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        // The resolver oneshot must receive the choice.
        let choice = rx.await.expect("resolver oneshot should resolve");
        assert_eq!(choice, PermissionChoice::AllowOnce);

        // Registry entry must be removed after resolution.
        assert!(
            !state
                .approval_registry
                .lock()
                .await
                .contains_key(&request_id)
        );
    }

    #[tokio::test]
    async fn test_approval_respond_unknown_returns_not_found() {
        let state = AppState {
            client_config: test_config(),
            tools: Arc::new(ToolRegistry::new()),
            ws_sessions: Arc::new(RwLock::new(HashMap::new())),
            approval_registry: Arc::new(Mutex::new(HashMap::new())),
        };
        let body = ApprovalRespondRequest {
            request_id: "does-not-exist".to_string(),
            choice: ApprovalDecision::Deny,
        };
        let response = approval_respond_handler(State(state), Json(body))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_approval_respond_endpoint_wired_and_404s_for_unknown() {
        // End-to-end through the router: proves the route is registered and
        // the 404 path works without a pre-seeded registry entry.
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/approval/respond")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"request_id": "missing", "choice": "deny"}).to_string(),
            ))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // ══════════════════════════════════════════════════════════════════════
    // WsSession tests
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_ws_session_default_state() {
        let session = WsSession {
            messages: vec![],
            model: None,
        };
        assert!(session.messages.is_empty());
        assert!(session.model.is_none());
    }

    #[tokio::test]
    async fn test_ws_session_model_update() {
        let session = Arc::new(Mutex::new(WsSession {
            messages: vec![],
            model: None,
        }));

        {
            let mut s = session.lock().await;
            s.model = Some("gpt-4o".to_string());
        }

        let s = session.lock().await;
        assert_eq!(s.model.as_deref(), Some("gpt-4o"));
    }

    #[tokio::test]
    async fn test_ws_session_messages_clear() {
        let session = Arc::new(Mutex::new(WsSession {
            messages: vec![],
            model: None,
        }));

        let test_msg = Message {
            role: "user".to_string(),
            content: MessageContent::Text("hello".to_string()),
        };
        {
            let mut s = session.lock().await;
            s.messages.push(test_msg.clone());
            s.messages.push(test_msg.clone());
        }

        {
            let s = session.lock().await;
            assert_eq!(s.messages.len(), 2);
        }

        {
            let mut s = session.lock().await;
            s.messages.clear();
        }

        {
            let s = session.lock().await;
            assert!(s.messages.is_empty());
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // CORS headers tests
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_cors_headers_present_on_health() {
        // P0.2: CORS is deny-by-default; the header is present only when the
        // request origin matches the configured allow list.
        let app = test_app_with_origins(&["http://example.com"]);
        let req = Request::builder()
            .method("OPTIONS")
            .uri("/api/health")
            .header("origin", "http://example.com")
            .header("access-control-request-method", "GET")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        let cors_header = response
            .headers()
            .get("access-control-allow-origin")
            .expect("CORS header missing for allowed origin");
        assert_eq!(cors_header, "http://example.com");
    }

    #[tokio::test]
    async fn test_cors_headers_present_on_models() {
        // P0.2: an explicit allowed origin gets an ACAO header on a normal GET.
        let app = test_app_with_origins(&["http://app.local"]);
        let req = Request::builder()
            .uri("/api/models")
            .header("origin", "http://app.local")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        let cors_header = response
            .headers()
            .get("access-control-allow-origin")
            .expect("CORS header missing for allowed origin");
        assert_eq!(cors_header, "http://app.local");
    }

    // ══════════════════════════════════════════════════════════════════════
    // Router build / integration tests
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_router_with_custom_tools() {
        let config = test_config();
        let registry = ToolRegistry::new();
        let app = ShannonApiServer::new(config)
            .with_tools(registry)
            .build_router();

        let req = Request::builder()
            .method("POST")
            .uri("/api/tools/list")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = read_body(response.into_body()).await;
        let tools: ToolsListResponse = serde_json::from_slice(&body).unwrap();
        // Custom registry is also empty but the endpoint still works
        assert!(tools.tools.is_empty());
    }

    #[test]
    fn test_build_router_does_not_panic() {
        let config = test_config();
        let server = ShannonApiServer::new(config);
        // Ensure build_router is deterministic and doesn't panic
        let _router = server.build_router();
    }
}
