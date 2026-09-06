use crate::{AppState, sse};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::sse::{KeepAlive, Sse},
    response::{IntoResponse, Response},
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
    let engine = build_engine(config);
    let summary = state.sessions.create(engine).await;
    Json(CreateSessionResponse {
        id: summary.id,
        created_at: summary.created_at,
        message_count: summary.message_count,
    })
}

/// Build a fresh `QueryEngine` from an LLM client config — the exact engine
/// construction `POST /v1/sessions` uses (bare `ToolRegistry`, default
/// `PermissionManager`). Shared with the GitHub hook's serve-side routine
/// execution (P2-7) so both paths stay identical.
pub(crate) fn build_engine(
    config: shannon_engine::api::LlmClientConfig,
) -> shannon_core::query_engine::QueryEngine {
    let client = if config.provider.requires_auth() {
        shannon_engine::api::LlmClient::new(config)
    } else {
        shannon_engine::api::LlmClient::new_unauthenticated(config)
    };
    shannon_core::query_engine::QueryEngine::with_defaults(
        client,
        shannon_core::tools::ToolRegistry::new(),
        shannon_engine::permissions::PermissionManager::new(),
        shannon_engine::state::StateManager::new(),
    )
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

// ── Routine trigger (P0-3) ──────────────────────────────────────────────

/// HMAC signature header — same wire format the notifier sends on outgoing
/// webhooks and the desktop loopback trigger endpoint expects.
pub(crate) const TRIGGER_SIGNATURE_HEADER: &str = "X-Shannon-Signature";

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TriggerRoutineRequest {
    /// Optional operator note (recorded with the run on the desktop side).
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TriggerAccepted {
    pub run_id: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TriggerError {
    pub error: String,
    /// Present on 501: where the trigger actually works today.
    pub hint: Option<String>,
}

/// Trigger a scheduled routine by id over HTTP.
///
/// Auth (same semantics as the desktop loopback endpoint):
/// - no `[notifications.webhook] secret` configured and no bearer token →
///   **403** (endpoint disabled — safe default);
/// - with a secret: `X-Shannon-Signature: sha256=<hex>` over the raw body is
///   required (missing/invalid → **401**); a valid bearer token (when the
///   server was started with a token) replaces the HMAC check, since the
///   bearer middleware has already authenticated the request.
///
/// **Serve-mode limitation:** the headless server has no scheduler or
/// unattended routine-execution pipeline (the desktop runs those in-process),
/// so authenticated requests get **501** with a pointer to the desktop. The
/// auth contract is fully implemented so clients can be built today.
#[utoipa::path(
    post,
    path = "/routines/{id}/trigger",
    params(("id" = String, Path, description = "Routine id (or name)")),
    request_body = TriggerRoutineRequest,
    responses(
        (status = 202, description = "Routine triggered", body = TriggerAccepted),
        (status = 401, description = "Missing or invalid HMAC signature"),
        (status = 403, description = "Trigger disabled: no webhook secret configured"),
        (status = 501, description = "Routine execution requires desktop or a future scheduler integration", body = TriggerError)
    )
)]
pub async fn trigger_routine(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, (StatusCode, Json<TriggerError>)> {
    let err = |status: StatusCode, error: String, hint: Option<String>| {
        (status, Json(TriggerError { error, hint }))
    };

    // A configured bearer token already authenticated this request (the
    // middleware enforces it) — it replaces the HMAC requirement.
    let bearer_ok = state.auth_token.is_some();

    let Some(secret) = state.webhook_secret.as_deref() else {
        if bearer_ok {
            return Ok(not_implemented(&id));
        }
        return Err(err(
            StatusCode::FORBIDDEN,
            "routine trigger disabled: no [notifications.webhook] secret configured".to_string(),
            None,
        ));
    };

    if !bearer_ok {
        let provided = headers
            .get(TRIGGER_SIGNATURE_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !shannon_core::webhook::verify_signature(secret, &body, provided) {
            return Err(err(
                StatusCode::UNAUTHORIZED,
                format!("missing or invalid {TRIGGER_SIGNATURE_HEADER} header"),
                None,
            ));
        }
    }

    // Note: `id` and the body's optional `note` are accepted per contract;
    // execution is the only part missing in serve mode.
    Ok(not_implemented(&id))
}

fn not_implemented(id: &str) -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(TriggerError {
            error: format!("routine '{id}' cannot be executed by the headless serve process"),
            hint: Some("requires desktop or a future scheduler integration".to_string()),
        }),
    )
        .into_response()
}
