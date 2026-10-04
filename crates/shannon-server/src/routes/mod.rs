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
use shannon_core::api_server::{MessageAttachment, attachments_to_blocks};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateSessionRequest {
    pub model: Option<String>,
    /// K4/P2-3: optional approval mode token (`ask` / `plan` / `auto-edit` /
    /// `full-auto` / `readonly` / `dontAsk` / `bypassPermissions`). Unknown
    /// tokens are rejected with 400; bypass runs the server-side guardrails.
    #[serde(default)]
    pub approval_mode: Option<String>,
}
/// K4/P2-3: error body for session-create failures (unknown mode, refused
/// bypass). Returned with 400.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CreateSessionError {
    pub error: String,
}

impl axum::response::IntoResponse for CreateSessionError {
    fn into_response(self) -> axum::response::Response {
        (axum::http::StatusCode::BAD_REQUEST, axum::Json(self)).into_response()
    }
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
    #[schema(value_type = Vec<MessageAttachmentSchema>)]
    pub attachments: Option<Vec<MessageAttachment>>,
}

/// OpenAPI mirror of `shannon_api_protocol::MessageAttachment` (the
/// protocol crate stays a pure serde leaf, so the request schema derives
/// here and `value_type` points the doc at it). Fields must stay 1:1.
#[derive(Debug, utoipa::ToSchema)]
pub struct MessageAttachmentSchema {
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

#[utoipa::path(post, path = "/v1/sessions", request_body = CreateSessionRequest, responses((status = 200, body = CreateSessionResponse)))]
pub async fn create_session(
    State(state): State<AppState>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<CreateSessionResponse>, CreateSessionError> {
    let mut config = state.client_config.clone();
    if let Some(model) = request.model {
        config.model = model;
    }
    let approval_mode = parse_approval_mode_token(request.approval_mode.as_deref())
        .map_err(|e| CreateSessionError { error: e })?;
    let engine = build_engine(config, approval_mode)
        .map_err(|e| CreateSessionError { error: e })?;
    let summary = state.sessions.create(engine).await;
    Ok(Json(CreateSessionResponse {
        id: summary.id,
        created_at: summary.created_at,
        message_count: summary.message_count,
    }))
}

/// Build a fresh `QueryEngine` from an LLM client config — the exact engine
/// construction `POST /v1/sessions` uses (bare `ToolRegistry`, default
/// `PermissionManager`). Shared with the GitHub hook's serve-side routine
/// execution (P2-7) so both paths stay identical.
pub(crate) fn build_engine(
    config: shannon_engine::api::LlmClientConfig,
    approval_mode: Option<shannon_engine::permissions::ApprovalMode>,
) -> Result<shannon_core::query_engine::QueryEngine, String> {
    let client = if config.provider.requires_auth() {
        shannon_engine::api::LlmClient::new(config)
    } else {
        shannon_engine::api::LlmClient::new_unauthenticated(config)
    };
    // K4/P2-3: served sessions get the same permission bootstrap as the CLI —
    // configured profile, settings rules + defaultMode — and never a bare
    // engine-default manager.
    let mut permissions = shannon_engine::permissions::PermissionManager::new();
    if let Some(profile) =
        shannon_core::unified_config::ShannonConfig::configured_permission_profile()
    {
        shannon_engine::permissions::apply_configured_profile(&mut permissions, &profile);
    }
    shannon_engine::permissions::load_settings_permission_files(&mut permissions);
    if let Some(mode) = approval_mode {
        if mode == shannon_engine::permissions::ApprovalMode::BypassPermissions {
            shannon_engine::permissions::ensure_bypass_allowed()?;
        }
        permissions.set_approval_mode(mode);
    }
    Ok(shannon_core::query_engine::QueryEngine::with_defaults(
        client,
        shannon_core::tools::ToolRegistry::new(),
        permissions,
        shannon_engine::state::StateManager::new(),
    ))
}

/// K4/P2-3: parse a wire approval-mode token with an HTTP-shaped error.
pub(crate) fn parse_approval_mode_token(
    token: Option<&str>,
) -> Result<Option<shannon_engine::permissions::ApprovalMode>, String> {
    match token {
        None => Ok(None),
        Some(t) => shannon_engine::permissions::ApprovalMode::from_str_ci(t)
            .map(Some)
            .ok_or_else(|| {
                format!(
                    "unknown approval_mode '{t}'; valid: {}",
                    shannon_engine::permissions::ApprovalMode::all_names().join(", ")
                )
            }),
    }
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
    // review §P1-6: drop the lock before consuming the stream so the
    // per-event ConversationUpdate handler below can re-acquire it briefly
    // to call restore_messages. Holding the lock across the SSE stream would
    // serialise every concurrent REST message on this session — but more
    // importantly, it would deadlock the per-event restore below which also
    // needs the same lock.
    let query_stream = {
        let guard = engine.lock().await;
        guard.process_query(context, None).await
    };
    let engine_for_events = engine.clone();
    let engine_for_flush = engine_for_events.clone();
    // review F45: when a concurrent request on the same session holds the
    // engine lock, the per-event try_lock below used to fail and the update
    // was silently dropped — the next message then ran without this turn's
    // context. Contended updates are now retained and flushed once the
    // stream ends, with a bounded retry.
    let pending_restore: PendingRestore = Arc::new(std::sync::Mutex::new(None));
    let stasher = Arc::clone(&pending_restore);
    let stream = query_stream
        .map(move |item| {
            // review §P1-6: tap ConversationUpdate so subsequent REST messages
            // on this session see the prior context. We must NOT hold the SSE
            // stream's exclusive access to the engine when re-locking here,
            // hence the explicit clone + per-event try-lock acquisition.
            if let Ok(shannon_core::query_engine::QueryEvent::ConversationUpdate {
                messages, ..
            }) = item.as_ref()
            {
                if let Ok(mut guard) = engine_for_events.try_lock() {
                    guard.restore_messages(messages.clone());
                    if let Ok(mut pending) = stasher.lock() {
                        *pending = None;
                    }
                } else if let Ok(mut pending) = stasher.lock() {
                    *pending = Some(messages.clone());
                }
            }
            Ok(item.map(sse::event).unwrap_or_else(|e| {
                axum::response::sse::Event::default()
                    .event("error")
                    .data(e.to_string())
            }))
        })
        // End-of-stream flush: emits nothing on the wire; it only performs
        // the retained write-back (F45) after the last event.
        .chain(restore_flush_stream(engine_for_flush, pending_restore));
    // T4: the response body cannot outlive the server — it ends with a
    // terminal `error` frame on shutdown (SIGINT/SIGTERM drain) or when the
    // overall stream duration cap expires (a wedged query can no longer
    // hold the connection and its keepalive pings open forever). Scoped to
    // this SSE body stream only; there is deliberately no per-request
    // middleware timeout that could kill a long legitimate turn.
    let guarded =
        sse::with_shutdown_and_cap(stream, state.shutdown.clone(), SSE_STREAM_MAX_DURATION);
    Ok(Sse::new(guarded).keep_alive(KeepAlive::default()))
}

/// Overall cap on one SSE response stream (T4), measured from stream start.
/// Exists so a wedged query cannot hold an SSE connection open forever; at
/// 30 minutes it is orders of magnitude above any legitimate turn, so it is
/// not a per-request timeout. On expiry the stream ends with the terminal
/// `error` event (`sse::with_shutdown_and_cap`).
pub(crate) const SSE_STREAM_MAX_DURATION: std::time::Duration =
    std::time::Duration::from_secs(30 * 60);

// ── ConversationUpdate write-back (review F45) ──────────────────────────

/// Latest un-restored `ConversationUpdate` payload from a live SSE stream.
type PendingRestore = std::sync::Arc<std::sync::Mutex<Option<Vec<shannon_engine::api::Message>>>>;

/// How long / how often the end-of-stream restore retries the engine lock.
/// The lock is held only while a concurrent request builds its query context
/// or performs its own restore, so it frees quickly; five seconds of
/// sustained contention means something is wedged and we give up loudly
/// (with a warn!) instead of silently dropping the turn.
const RESTORE_RETRY_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);
const RESTORE_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// Write `messages` back into the engine's conversation history, retrying
/// the lock for up to [`RESTORE_RETRY_BUDGET`]. Returns `false` when the
/// write-back was abandoned (the caller warns — the next message on this
/// session may run without this turn's context).
async fn restore_with_retry(
    engine: &tokio::sync::Mutex<shannon_core::query_engine::QueryEngine>,
    messages: Vec<shannon_engine::api::Message>,
) -> bool {
    restore_with_retry_budget(
        engine,
        messages,
        RESTORE_RETRY_BUDGET,
        RESTORE_RETRY_INTERVAL,
    )
    .await
}

/// [`restore_with_retry`] with injectable budget/interval (test seam).
async fn restore_with_retry_budget(
    engine: &tokio::sync::Mutex<shannon_core::query_engine::QueryEngine>,
    messages: Vec<shannon_engine::api::Message>,
    budget: std::time::Duration,
    interval: std::time::Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        match engine.try_lock() {
            Ok(mut guard) => {
                guard.restore_messages(messages);
                return true;
            }
            Err(_contended) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(interval).await;
            }
            Err(_contended) => return false,
        }
    }
}

/// End-of-stream flush for the retained restore payload (F45): a zero-item
/// stream that performs the bounded-retry write-back exactly once when the
/// SSE body finishes. If the stream is dropped mid-flight (client
/// disconnect) the flush is skipped — the same failure mode as before this
/// fix, never worse.
fn restore_flush_stream(
    engine: std::sync::Arc<tokio::sync::Mutex<shannon_core::query_engine::QueryEngine>>,
    pending: PendingRestore,
) -> impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>> {
    futures::stream::unfold((engine, pending), |(engine, pending)| async move {
        let messages = pending.lock().ok().and_then(|mut p| p.take());
        if let Some(messages) = messages {
            if !restore_with_retry(&engine, messages).await {
                tracing::warn!(
                    budget = ?RESTORE_RETRY_BUDGET,
                    "abandoning ConversationUpdate restore: engine lock stayed contended; \
                     the next message on this session may miss this turn's context"
                );
            }
        }
        None
    })
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

    // ── F45: ConversationUpdate restore write-back ──────────────────────

    fn test_engine() -> shannon_core::query_engine::QueryEngine {
        let config = shannon_engine::api::LlmClientConfig {
            thinking_type: None,
            provider: shannon_engine::api::types::LlmProvider::Ollama,
            model: "test-model".into(),
            base_url: "http://127.0.0.1:1".into(),
            ..Default::default()
        };
        let client = shannon_engine::api::LlmClient::new_unauthenticated(config);
        shannon_core::query_engine::QueryEngine::with_defaults(
            client,
            shannon_core::tools::ToolRegistry::new(),
            shannon_engine::permissions::PermissionManager::new(),
            shannon_engine::state::StateManager::new(),
        )
    }

    fn turn(role: &str, text: &str) -> shannon_engine::api::Message {
        shannon_engine::api::Message {
            role: role.to_string(),
            content: shannon_engine::api::MessageContent::Text(text.to_string()),
        }
    }

    #[tokio::test]
    async fn restore_writes_back_immediately_when_uncontended() {
        let engine = Arc::new(tokio::sync::Mutex::new(test_engine()));
        assert!(restore_with_retry(&engine, vec![turn("user", "hi")]).await);
        assert_eq!(engine.lock().await.conversation_messages().len(), 1);
    }

    #[tokio::test]
    async fn restore_retries_until_contended_lock_frees() {
        let engine = Arc::new(tokio::sync::Mutex::new(test_engine()));
        // Hold the engine lock the way a concurrent request would (context
        // building / its own restore): the retry must wait it out, not drop
        // the update.
        let guard = engine.lock().await;
        let eng = Arc::clone(&engine);
        let worker = tokio::spawn(async move {
            restore_with_retry_budget(
                &eng,
                vec![turn("user", "hi")],
                std::time::Duration::from_secs(5),
                std::time::Duration::from_millis(20),
            )
            .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(
            !worker.is_finished(),
            "retry must still be waiting while the lock is held"
        );
        drop(guard);
        let restored = tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("restore must complete shortly after the lock frees")
            .expect("worker join");
        assert!(restored);
        assert_eq!(engine.lock().await.conversation_messages().len(), 1);
    }

    #[tokio::test]
    async fn restore_gives_up_and_reports_after_the_budget() {
        let engine = Arc::new(tokio::sync::Mutex::new(test_engine()));
        let guard = engine.lock().await; // never released during the retry
        let restored = restore_with_retry_budget(
            &engine,
            vec![turn("user", "hi")],
            std::time::Duration::from_millis(60),
            std::time::Duration::from_millis(10),
        )
        .await;
        assert!(!restored, "sustained contention must end in a false return");
        // Release before re-locking for the assertion — the guard is still
        // in scope here and tokio::sync::Mutex has no reentrancy.
        drop(guard);
        assert_eq!(
            engine.lock().await.conversation_messages().len(),
            0,
            "nothing was written back"
        );
    }

    #[tokio::test]
    async fn restore_flush_stream_ends_without_emitting_events() {
        // The flush is a zero-item stream: chaining it must not add anything
        // to the SSE wire.
        let engine = Arc::new(tokio::sync::Mutex::new(test_engine()));
        let pending: PendingRestore = Arc::new(std::sync::Mutex::new(Some(vec![
            turn("user", "hi"),
            turn("assistant", "hello"),
        ])));
        let flush = restore_flush_stream(Arc::clone(&engine), pending);
        let items: Vec<_> = flush.collect().await;
        assert!(items.is_empty(), "flush stream must emit nothing");
        assert_eq!(
            engine.lock().await.conversation_messages().len(),
            2,
            "the retained payload was written back"
        );
    }
}
