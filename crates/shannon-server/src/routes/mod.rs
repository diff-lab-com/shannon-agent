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
    /// Optional multimodal attachments delivered alongside `content`.
    /// Images are carried to the LLM as base64 content blocks (Anthropic /
    /// OpenAI vision); anything else is rejected with a 400.
    #[serde(default)]
    pub attachments: Option<Vec<MessageAttachment>>,
}

/// A single base64-encoded attachment on a message.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MessageAttachment {
    /// File name (informational; shown to the model in the message text).
    pub name: Option<String>,
    /// MIME type. Supported: image/png, image/jpeg, image/gif, image/webp.
    pub media_type: String,
    /// Base64-encoded file bytes.
    pub data: String,
}

/// Stable error body returned with non-2xx responses on
/// `/v1/sessions/:id/messages`. Mirrors Anthropic/OpenAI conventions:
/// a `code` for programmatic handling plus a human-readable `message`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApiError {
    /// Stable error class — see the `code` field for known values.
    pub code: &'static str,
    /// Human-readable detail, safe to surface to the user.
    pub message: String,
}

impl ApiError {
    pub const EMPTY_MESSAGE_CODE: &str = "empty_message";
    pub const EMPTY_MESSAGE_TEXT: &str = "content is empty and no attachments provided";
    pub const SESSION_NOT_FOUND_CODE: &str = "session_not_found";
    pub const SESSION_NOT_FOUND_TEXT: &str = "session not found";

    pub fn empty_message() -> Self {
        Self {
            code: Self::EMPTY_MESSAGE_CODE,
            message: Self::EMPTY_MESSAGE_TEXT.into(),
        }
    }

    pub fn session_not_found() -> Self {
        Self {
            code: Self::SESSION_NOT_FOUND_CODE,
            message: Self::SESSION_NOT_FOUND_TEXT.into(),
        }
    }

    fn attachment(message: String) -> Self {
        Self {
            code: "attachment_invalid",
            message,
        }
    }
}

/// Largest decoded attachment (10 MB), mirroring the desktop app limit.
const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
/// Maximum attachments per message (Anthropic accepts up to 100; this keeps
/// a single request's multimodal payload bounded).
const MAX_ATTACHMENTS: usize = 8;
/// MIME types the multimodal adapters can serialize.
const SUPPORTED_MEDIA_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Validate attachments and convert them to provider-agnostic content
/// blocks. Returns a user-facing error message on the first violation.
fn attachments_to_blocks(
    attachments: &[MessageAttachment],
) -> Result<Vec<shannon_engine::api::ContentBlock>, String> {
    use base64::Engine;

    if attachments.len() > MAX_ATTACHMENTS {
        return Err(format!(
            "too many attachments: {} (max {MAX_ATTACHMENTS})",
            attachments.len()
        ));
    }

    let mut blocks = Vec::with_capacity(attachments.len());
    for (i, att) in attachments.iter().enumerate() {
        let label = att
            .name
            .clone()
            .unwrap_or_else(|| format!("attachment-{i}"));
        if !SUPPORTED_MEDIA_TYPES.contains(&att.media_type.as_str()) {
            return Err(format!(
                "attachment \"{label}\": unsupported media_type \"{}\" (supported: {})",
                att.media_type,
                SUPPORTED_MEDIA_TYPES.join(", ")
            ));
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(att.data.as_bytes())
            .map_err(|_| format!("attachment \"{label}\": data is not valid base64"))?;
        if decoded.len() > MAX_ATTACHMENT_BYTES {
            return Err(format!(
                "attachment \"{label}\": {} bytes exceeds the {MAX_ATTACHMENT_BYTES} byte limit",
                decoded.len()
            ));
        }
        blocks.push(shannon_engine::api::ContentBlock::Image {
            source: shannon_engine::api::ImageSource::base64(
                att.media_type.clone(),
                att.data.clone(),
            ),
        });
    }
    Ok(blocks)
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
    (StatusCode, Json<ApiError>),
> {
    if request.content.trim().is_empty() && request.attachments.as_ref().is_none_or(Vec::is_empty) {
        return Err((StatusCode::BAD_REQUEST, Json(ApiError::empty_message())));
    }
    let attachments = match request.attachments.as_deref() {
        None => Vec::new(),
        Some(atts) => match attachments_to_blocks(atts) {
            Ok(blocks) => blocks,
            Err(message) => {
                tracing::warn!("attachment validation failed: {message}");
                return Err((StatusCode::BAD_REQUEST, Json(ApiError::attachment(message))));
            }
        },
    };
    let session = state
        .sessions
        .get(id)
        .await
        .ok_or((StatusCode::NOT_FOUND, Json(ApiError::session_not_found())))?;
    let engine = session.engine;
    let context = shannon_core::query_engine::QueryContext {
        query_id: Uuid::new_v4(),
        session_id: id,
        user_message: request.content,
        attachments,
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn png_b64(len: usize) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(vec![0x89u8; len])
    }

    #[test]
    fn test_valid_image_attachments_convert_to_blocks() {
        let atts = vec![
            MessageAttachment {
                name: Some("shot.png".into()),
                media_type: "image/png".into(),
                data: png_b64(16),
            },
            MessageAttachment {
                name: None,
                media_type: "image/jpeg".into(),
                data: png_b64(16),
            },
        ];
        let blocks = attachments_to_blocks(&atts).unwrap();
        assert_eq!(blocks.len(), 2);
        assert!(matches!(
            blocks[0],
            shannon_engine::api::ContentBlock::Image { .. }
        ));
    }

    #[test]
    fn test_unsupported_media_type_rejected() {
        let atts = vec![MessageAttachment {
            name: Some("doc.pdf".into()),
            media_type: "application/pdf".into(),
            data: png_b64(16),
        }];
        let err = attachments_to_blocks(&atts).unwrap_err();
        assert!(err.contains("unsupported media_type"), "got: {err}");
    }

    #[test]
    fn test_invalid_base64_rejected() {
        let atts = vec![MessageAttachment {
            name: None,
            media_type: "image/png".into(),
            data: "not!base64!".into(),
        }];
        let err = attachments_to_blocks(&atts).unwrap_err();
        assert!(err.contains("not valid base64"), "got: {err}");
    }

    #[test]
    fn test_oversized_attachment_rejected() {
        let atts = vec![MessageAttachment {
            name: Some("big.png".into()),
            media_type: "image/png".into(),
            data: png_b64(MAX_ATTACHMENT_BYTES + 1),
        }];
        let err = attachments_to_blocks(&atts).unwrap_err();
        assert!(err.contains("exceeds"), "got: {err}");
    }

    #[test]
    fn test_too_many_attachments_rejected() {
        let atts: Vec<MessageAttachment> = (0..=MAX_ATTACHMENTS)
            .map(|_| MessageAttachment {
                name: None,
                media_type: "image/png".into(),
                data: png_b64(4),
            })
            .collect();
        let err = attachments_to_blocks(&atts).unwrap_err();
        assert!(err.contains("too many attachments"), "got: {err}");
    }

    #[test]
    fn test_svg_not_accepted_via_rest() {
        // Vision providers accept png/jpeg/gif/webp only; the desktop app
        // filters SVG before this point, and the REST API rejects it.
        let atts = vec![MessageAttachment {
            name: Some("logo.svg".into()),
            media_type: "image/svg+xml".into(),
            data: png_b64(8),
        }];
        assert!(attachments_to_blocks(&atts).is_err());
    }

    // ── ApiError body — structured JSON for non-2xx responses ──
    // Locks down the contract added in the T2 follow-up: clients can
    // surface `message` directly; `code` is stable for programmatic
    // handling.

    #[test]
    fn api_error_serialization_includes_code_and_message() {
        let err = ApiError::empty_message();
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"code\":\"empty_message\""));
        assert!(json.contains("\"message\":"));
    }

    #[test]
    fn api_error_attachment_carries_validation_message() {
        let err = ApiError::attachment(String::from("attachment \"x\": unsupported media_type"));
        assert_eq!(err.code, "attachment_invalid");
        assert!(err.message.contains("unsupported"));
    }

    #[test]
    fn api_error_session_not_found_is_stable() {
        let err = ApiError::session_not_found();
        assert_eq!(err.code, ApiError::SESSION_NOT_FOUND_CODE);
    }
}
