//! HTTP API server for Shannon Code.
//!
//! Exposes a REST and SSE API so external tools can interact with Shannon
//! over HTTP.

use crate::api::{LlmClient, LlmClientConfig};
use crate::permissions::PermissionManager;
use crate::query_engine::{QueryContext, QueryEngine, QueryEvent, QueryMetadata};
use crate::state::StateManager;
use crate::tools::ToolRegistry;
use crate::VERSION;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
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
            .layer(cors)
            .with_state(AppState {
                client_config: self.client_config.clone(),
                tools: self.tools.clone(),
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
