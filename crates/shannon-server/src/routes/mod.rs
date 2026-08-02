use crate::{AppState, sse};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::sse::{KeepAlive, Sse},
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateSessionRequest {
    pub model: Option<String>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CreateSessionResponse {
    pub id: Uuid,
    pub created_at: String,
    pub message_count: usize,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MessageRequest {
    pub content: String,
}

#[utoipa::path(post, path = "/v1/sessions", request_body = CreateSessionRequest, responses((status = 200, body = CreateSessionResponse)))]
pub async fn create_session(
    State(state): State<AppState>,
    Json(request): Json<CreateSessionRequest>,
) -> Json<CreateSessionResponse> {
    let mut config = state.client_config.clone();
    if let Some(model) = request.model {
        config.model = model;
    }
    let client = if config.provider.requires_auth() {
        shannon_engine::api::LlmClient::new(config)
    } else {
        shannon_engine::api::LlmClient::new_unauthenticated(config)
    };
    let engine = shannon_core::query_engine::QueryEngine::with_defaults(
        client,
        shannon_core::tools::ToolRegistry::new(),
        shannon_engine::permissions::PermissionManager::new(),
        shannon_engine::state::StateManager::new(),
    );
    let summary = state.sessions.create(engine).await;
    Json(CreateSessionResponse {
        id: summary.id,
        created_at: summary.created_at,
        message_count: summary.message_count,
    })
}

#[utoipa::path(get, path = "/v1/sessions/{id}", params(("id" = Uuid, Path)), responses((status = 200, body = CreateSessionResponse), (status = 404)))]
pub async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CreateSessionResponse>, StatusCode> {
    state
        .sessions
        .get(id)
        .await
        .map(|s| {
            Json(CreateSessionResponse {
                id: s.summary.id,
                created_at: s.summary.created_at,
                message_count: s.summary.message_count,
            })
        })
        .ok_or(StatusCode::NOT_FOUND)
}

#[utoipa::path(post, path = "/v1/sessions/{id}/messages", params(("id" = Uuid, Path)), request_body = MessageRequest, responses((status = 200, content_type = "text/event-stream")))]
pub async fn post_message(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(request): Json<MessageRequest>,
) -> Result<
    Sse<impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>>,
    StatusCode,
> {
    if request.content.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let session = state.sessions.get(id).await.ok_or(StatusCode::NOT_FOUND)?;
    let engine = session.engine;
    let context = shannon_core::query_engine::QueryContext {
        query_id: Uuid::new_v4(),
        session_id: id,
        user_message: request.content,
        metadata: shannon_core::query_engine::QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: true,
            max_tokens: None,
            model: engine.lock().await.client().model().to_string(),
            temperature: None,
            top_p: None,
        },
    };
    let stream = engine
        .lock()
        .await
        .process_query(context, None)
        .await
        .map(|item| {
            Ok(item.map(sse::event).unwrap_or_else(|e| {
                axum::response::sse::Event::default()
                    .event("error")
                    .data(e.to_string())
            }))
        });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
