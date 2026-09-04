mod auth;
pub mod routes;
pub mod sessions;
pub mod sse;
use axum::{
    Router, middleware,
    routing::{get, post},
};
use shannon_engine::api::LlmClientConfig;
use utoipa::OpenApi;

#[derive(Clone)]
pub struct AppState {
    pub client_config: LlmClientConfig,
    pub sessions: sessions::SessionRegistry,
    /// HMAC secret for `POST /routines/:id/trigger` (P0-3), read once at
    /// router construction from `[notifications.webhook] secret` in
    /// `~/.shannon/config.toml`. `None` disables the endpoint (403).
    pub webhook_secret: Option<String>,
    /// Bearer token, mirroring the auth middleware config. When set, a valid
    /// `Authorization: Bearer` request may use the trigger endpoint without
    /// an HMAC signature (the middleware has already enforced the token).
    pub auth_token: Option<String>,
}
#[derive(OpenApi)]
#[openapi(
    paths(
        routes::create_session,
        routes::get_session,
        routes::post_message,
        routes::trigger_routine
    ),
    components(schemas(
        routes::CreateSessionRequest,
        routes::CreateSessionResponse,
        routes::MessageRequest,
        routes::TriggerRoutineRequest,
        routes::TriggerAccepted,
        routes::TriggerError,
        sessions::SessionSummary
    ))
)]
pub struct ApiDoc;

pub fn router(client_config: LlmClientConfig, token: Option<String>) -> Router {
    let secret = read_webhook_secret();
    router_with_secret(client_config, token, secret)
}

/// [`router`] with the trigger-endpoint HMAC secret injected (test seam).
#[doc(hidden)]
pub fn router_with_secret(
    client_config: LlmClientConfig,
    token: Option<String>,
    webhook_secret: Option<String>,
) -> Router {
    let state = AppState {
        client_config,
        sessions: sessions::SessionRegistry::default(),
        webhook_secret,
        auth_token: token.clone(),
    };
    Router::new()
        .route("/v1/sessions", post(routes::create_session))
        .route("/v1/sessions/:id", get(routes::get_session))
        .route("/v1/sessions/:id/messages", post(routes::post_message))
        .route("/routines/:id/trigger", post(routes::trigger_routine))
        .route(
            "/openapi.json",
            get(|| async { axum::Json(ApiDoc::openapi()) }),
        )
        .layer(middleware::from_fn_with_state(
            auth::AuthConfig::new(token.or_else(|| std::env::var("SHANNON_SERVE_TOKEN").ok())),
            auth::bearer_middleware,
        ))
        .with_state(state)
}

/// Resolve the trigger-endpoint HMAC secret the same way the desktop does:
/// `[notifications.webhook] secret` from the global Shannon config
/// (`~/.shannon/config.toml`). Missing section/field → `None`, which disables
/// the endpoint (safe default, 403).
fn read_webhook_secret() -> Option<String> {
    shannon_core::unified_config::ConfigBuilder::new()
        .load_global_toml()
        .build()
        .notifications
        .and_then(|n| n.webhook)
        .and_then(|w| w.secret)
}

pub async fn run(
    host: &str,
    port: u16,
    client_config: LlmClientConfig,
    token: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    axum::serve(listener, router(client_config, token)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_config() -> LlmClientConfig {
        shannon_core::LlmClientConfig {
            provider: shannon_engine::api::types::LlmProvider::Ollama,
            model: "test-model".into(),
            base_url: "http://127.0.0.1:1".into(),
            ..Default::default()
        }
    }

    async fn trigger(
        app: Router,
        id: &str,
        body: &str,
        signature: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut req = Request::builder()
            .method("POST")
            .uri(format!("/routines/{id}/trigger"))
            .header("content-type", "application/json");
        if let Some(sig) = signature {
            req = req.header("X-Shannon-Signature", sig);
        }
        let res = app
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 64 * 1024)
            .await
            .unwrap();
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, json)
    }

    #[tokio::test]
    async fn trigger_without_secret_is_403() {
        let app = router_with_secret(test_config(), None, None);
        let (status, body) = trigger(app, "r1", "{}", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("secret")
        );
    }

    #[tokio::test]
    async fn trigger_with_secret_requires_valid_signature() {
        // Missing header → 401.
        let app = router_with_secret(test_config(), None, Some("s3cret".into()));
        let (status, body) = trigger(app, "r1", "{}", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

        // Wrong signature → 401.
        let app = router_with_secret(test_config(), None, Some("s3cret".into()));
        let bad = shannon_core::webhook::sign_signature("other-secret", b"{}");
        let (status, body) = trigger(app, "r1", "{}", Some(&bad)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

        // Valid signature → past auth, into the serve-mode execution
        // limitation (501; see routes::trigger_routine docs).
        let app = router_with_secret(test_config(), None, Some("s3cret".into()));
        let sig = shannon_core::webhook::sign_signature("s3cret", b"{}");
        let (status, body) = trigger(app, "r1", "{}", Some(&sig)).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{body}");
        assert!(
            body["hint"]
                .as_str()
                .unwrap_or_default()
                .contains("desktop")
        );
    }

    #[tokio::test]
    async fn trigger_bearer_token_replaces_hmac() {
        // Token configured: the bearer middleware already validated the
        // request, so no HMAC signature is needed — goes straight to 501.
        let app = router_with_secret(test_config(), Some("tok".into()), Some("s3cret".into()));
        let req = Request::builder()
            .method("POST")
            .uri("/routines/r1/trigger")
            .header("content-type", "application/json")
            .header("authorization", "Bearer tok")
            .body(Body::from("{}"))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn trigger_route_is_documented_in_openapi() {
        let app = router_with_secret(test_config(), None, None);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let openapi: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            openapi["paths"]["/routines/{id}/trigger"].is_object(),
            "trigger path missing from OpenAPI"
        );
        let desc =
            openapi["paths"]["/routines/{id}/trigger"]["post"]["responses"]["501"]["description"]
                .as_str()
                .unwrap_or_default();
        assert!(desc.contains("desktop"), "501 must document the limitation");
    }
}
