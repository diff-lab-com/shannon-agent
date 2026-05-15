//! HTTP/WebSocket API server for Shannon Code.
//!
//! Exposes REST, SSE, and WebSocket APIs so external tools and remote TUI
//! instances can interact with Shannon over the network.

use crate::api::{LlmClient, LlmClientConfig, Message};
use crate::permissions::PermissionManager;
use crate::query_engine::{QueryContext, QueryEngine, QueryEvent, QueryMetadata};
use crate::state::StateManager;
use crate::tools::ToolRegistry;
use crate::VERSION;
use axum::extract::ws::{Message as WsMsg, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
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
    Query { prompt: String, model: Option<String> },
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
    ToolUse { name: String, input: serde_json::Value },
    /// Tool result event.
    #[serde(rename = "tool_result")]
    ToolResult { name: String, output: String },
    /// Token usage update.
    #[serde(rename = "usage")]
    Usage { input_tokens: u64, output_tokens: u64, cost_usd: f64 },
    /// Query completed.
    #[serde(rename = "completed")]
    Completed { model: String },
    /// Query failed.
    #[serde(rename = "failed")]
    Failed { error: String },
    /// Session info response.
    #[serde(rename = "session_info")]
    SessionInfo { message_count: usize, model: Option<String> },
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

    /// Build the `axum::Router` with all routes and middleware.
    fn build_router(&self) -> axum::Router<()> {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        axum::Router::new()
            .route("/api/health", get(health_handler))
            .route("/api/models", get(models_handler))
            .route("/api/query", post(query_handler))
            .route("/api/query/stream", get(query_stream_handler))
            .route("/api/tools/list", post(tools_list_handler))
            .route("/api/ws", get(ws_handler))
            .layer(cors)
            .with_state(AppState {
                client_config: self.client_config.clone(),
                tools: self.tools.clone(),
                ws_sessions: Arc::new(RwLock::new(HashMap::new())),
            })
    }

    /// Start the server and block until shutdown.
    pub async fn serve(self) -> Result<(), Box<dyn std::error::Error>> {
        let addr = format!("{}:{}", self.host, self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        tracing::info!("Shannon API server listening on {addr}");
        let router = self.build_router();
        axum::serve(listener, router).await?;
        Ok(())
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
    let engine = QueryEngine::with_defaults(client, tools, permissions, state_mgr);

    let context = QueryContext {
        query_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
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
    let engine = QueryEngine::with_defaults(client, tools, permissions, state_mgr);

    let context = QueryContext {
        query_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
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
                };
                let data = serde_json::to_string(&event).unwrap_or_default();
                Some(Ok(Event::default().event(event_type).data(data)))
            }
            Err(e) => {
                let data =
                    serde_json::to_string(&serde_json::json!({ "error": e.to_string() }))
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
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_socket(socket, state))
}

async fn handle_ws_socket(mut socket: WebSocket, state: AppState) {
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

    // Send session greeting
    let greeting = WsServerMessage::SessionInfo {
        message_count: 0,
        model: None,
    };
    if let Ok(json) = serde_json::to_string(&greeting) {
        if let Err(e) = socket.send(WsMsg::Text(json)).await {
            tracing::debug!("WebSocket send failed: {e}");
        }
    }

    loop {
        let msg = match socket.recv().await {
            Some(Ok(WsMsg::Text(text))) => text,
            Some(Ok(WsMsg::Close(_))) | None => break,
            Some(Ok(_)) => continue,
            Some(Err(_)) => break,
        };

        let client_msg: WsClientMessage = match serde_json::from_str(&msg) {
            Ok(m) => m,
            Err(e) => {
                let err = WsServerMessage::Error {
                    message: format!("Invalid message: {e}"),
                };
                if let Ok(json) = serde_json::to_string(&err) {
                    if let Err(e) = socket.send(WsMsg::Text(json)).await {
            tracing::debug!("WebSocket send failed: {e}");
        }
                }
                continue;
            }
        };

        match client_msg {
            WsClientMessage::Query { prompt, model } => {
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

                let context = QueryContext {
                    query_id: uuid::Uuid::new_v4(),
                    session_id: uuid::Uuid::parse_str(&session_id).unwrap_or_default(),
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

                let mut stream = engine.process_query(context, None).await;

                while let Some(result) = stream.next().await {
                    let server_msg = match result {
                        Ok(QueryEvent::Text { content, .. }) => Some(WsServerMessage::Text { content }),
                        Ok(QueryEvent::ToolUseRequest { tool_name, tool_input, .. }) => {
                            Some(WsServerMessage::ToolUse {
                                name: tool_name,
                                input: tool_input,
                            })
                        }
                        Ok(QueryEvent::ToolUseResult { tool_name, result, .. }) => {
                            Some(WsServerMessage::ToolResult { name: tool_name, output: result })
                        }
                        Ok(QueryEvent::Usage { input_tokens, output_tokens, cost_usd, .. }) => {
                            Some(WsServerMessage::Usage { input_tokens, output_tokens, cost_usd })
                        }
                        Ok(QueryEvent::Completed { .. }) => {
                            Some(WsServerMessage::Completed { model: config.model.clone() })
                        }
                        Ok(QueryEvent::Failed { error, .. }) => {
                            Some(WsServerMessage::Failed { error })
                        }
                        Ok(_) => None,
                        Err(e) => Some(WsServerMessage::Failed { error: e.to_string() }),
                    };

                    if let Some(msg) = server_msg {
                        if let Ok(json) = serde_json::to_string(&msg) {
                            if socket.send(WsMsg::Text(json)).await.is_err() {
                                break;
                            }
                        }
                    }
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
                if let Ok(json) = serde_json::to_string(&info) {
                    if let Err(e) = socket.send(WsMsg::Text(json)).await {
            tracing::debug!("WebSocket send failed: {e}");
        }
                }
            }
            WsClientMessage::Info => {
                let s = session.lock().await;
                let info = WsServerMessage::SessionInfo {
                    message_count: s.messages.len(),
                    model: s.model.clone(),
                };
                if let Ok(json) = serde_json::to_string(&info) {
                    if let Err(e) = socket.send(WsMsg::Text(json)).await {
            tracing::debug!("WebSocket send failed: {e}");
        }
                }
            }
            WsClientMessage::Cancel => {
                // Future: wire up cancellation token
                let err = WsServerMessage::Error {
                    message: "Cancel not yet supported".to_string(),
                };
                if let Ok(json) = serde_json::to_string(&err) {
                    if let Err(e) = socket.send(WsMsg::Text(json)).await {
            tracing::debug!("WebSocket send failed: {e}");
        }
                }
            }
        }
    }

    // Cleanup session
    {
        let mut sessions = state.ws_sessions.write().await;
        sessions.remove(&session_id);
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::Router;
    use tower::ServiceExt;

    fn test_app() -> Router<()> {
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: "http://localhost:11434".to_string(),
            model: "test-model".to_string(),
            ..Default::default()
        };
        ShannonApiServer::new(config).build_router()
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = test_app();
        let req = Request::builder()
            .uri("/api/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let health: HealthResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(health.status, "ok");
        assert_eq!(health.version, VERSION);
    }

    #[tokio::test]
    async fn test_models_endpoint() {
        let app = test_app();
        let req = Request::builder()
            .uri("/api/models")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let models: ModelsResponse = serde_json::from_slice(&body).unwrap();
        assert!(!models.models.is_empty());
        assert!(models.models.iter().any(|m| m.id == "claude-sonnet-4"));
        assert!(models.models.iter().any(|m| m.id == "gpt-4o"));
    }

    #[tokio::test]
    async fn test_query_endpoint_empty_prompt() {
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
    async fn test_tools_list_endpoint_empty_registry() {
        let app = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/tools/list")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let tools: ToolsListResponse = serde_json::from_slice(&body).unwrap();
        // Default registry has no tools registered.
        assert!(tools.tools.is_empty());
    }
}
