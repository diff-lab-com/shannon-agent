mod auth;
pub mod github;
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
    /// HMAC secret for `POST /hooks/github` (P2-7), read once at router
    /// construction from `[hooks.github] secret` in `~/.shannon/config.toml`.
    /// `None` disables the endpoint (503).
    pub github_secret: Option<String>,
    /// `X-GitHub-Delivery` replay cache for the GitHub hook endpoint
    /// (in-memory, bounded; not persisted).
    pub github_deliveries: std::sync::Arc<std::sync::Mutex<github::DeliveryCache>>,
    /// Routine snapshot served to the GitHub hook, loaded from the shared
    /// scheduled-task store (`~/.shannon/scheduled-tasks/`) at router
    /// construction. Read-only: the serve process never mutates routines.
    pub routines: std::sync::Arc<Vec<shannon_core::scheduled_routines::ScheduledRoutine>>,
    /// Shared inbox store (`~/.shannon/inbox.db`): serve-side routine
    /// execution writes run records + results here so the desktop sees them.
    pub inbox: std::sync::Arc<shannon_core::inbox_store::InboxStore>,
}
#[derive(OpenApi)]
#[openapi(
    paths(
        routes::create_session,
        routes::get_session,
        routes::post_message,
        routes::trigger_routine,
        github::github_hook
    ),
    components(schemas(
        routes::CreateSessionRequest,
        routes::CreateSessionResponse,
        routes::MessageRequest,
        routes::TriggerRoutineRequest,
        routes::TriggerAccepted,
        routes::TriggerError,
        github::GitHubHookAccepted,
        github::GitHubHookError,
        sessions::SessionSummary
    ))
)]
pub struct ApiDoc;

pub fn router(client_config: LlmClientConfig, token: Option<String>) -> Router {
    let secret = read_webhook_secret();
    let github_secret = github_secret_from_config();
    let routines = github::load_routines();
    let inbox = std::sync::Arc::new(github::open_inbox());
    router_full(client_config, token, secret, github_secret, routines, inbox)
}

/// [`router`] with all state injected (test seam for the GitHub hook).
#[doc(hidden)]
pub fn router_full(
    client_config: LlmClientConfig,
    token: Option<String>,
    webhook_secret: Option<String>,
    github_secret: Option<String>,
    routines: Vec<shannon_core::scheduled_routines::ScheduledRoutine>,
    inbox: std::sync::Arc<shannon_core::inbox_store::InboxStore>,
) -> Router {
    let state = AppState {
        client_config,
        sessions: sessions::SessionRegistry::default(),
        webhook_secret,
        auth_token: token.clone(),
        github_secret,
        github_deliveries: github::delivery_cache(),
        routines: std::sync::Arc::new(routines),
        inbox,
    };
    Router::new()
        .route("/v1/sessions", post(routes::create_session))
        .route("/v1/sessions/:id", get(routes::get_session))
        .route("/v1/sessions/:id/messages", post(routes::post_message))
        .route("/routines/:id/trigger", post(routes::trigger_routine))
        .route(github::GITHUB_HOOK_PATH, post(github::github_hook))
        .route(
            "/openapi.json",
            get(|| async { axum::Json(ApiDoc::openapi()) }),
        )
        .layer(
            // axum 0.7 caps request bodies at 2 MiB by default, which 413'd
            // any real multimodal message before attachment validation could
            // run. The shared rule (`shannon_core::attachments`) allows 8
            // attachments × 10 MiB decoded; base64 inflates that 4/3 to
            // ~107 MiB, and 128 MiB leaves JSON-encoding headroom on top.
            axum::extract::DefaultBodyLimit::max(128 * 1024 * 1024),
        )
        .layer(middleware::from_fn_with_state(
            auth::AuthConfig::new(token.or_else(|| std::env::var("SHANNON_SERVE_TOKEN").ok())),
            auth::bearer_middleware,
        ))
        .with_state(state)
}

/// [`router`] with only the legacy trigger-endpoint secret injected — the
/// pre-P2-7 test seam, kept for the existing trigger tests. The GitHub hook
/// is disabled (no secret) and backed by an in-memory inbox and no routines.
#[doc(hidden)]
pub fn router_with_secret(
    client_config: LlmClientConfig,
    token: Option<String>,
    webhook_secret: Option<String>,
) -> Router {
    let inbox = std::sync::Arc::new(
        shannon_core::inbox_store::InboxStore::open_in_memory()
            .expect("in-memory inbox always opens"),
    );
    router_full(
        client_config,
        token,
        webhook_secret,
        None,
        Vec::new(),
        inbox,
    )
}

/// Resolve the GitHub hook secret the same way the trigger-endpoint secret is
/// resolved: from the global Shannon config (`~/.shannon/config.toml`), here
/// `[hooks.github] secret`. Missing section/field → `None`, which disables
/// the endpoint (safe default, 503).
fn github_secret_from_config() -> Option<String> {
    shannon_core::unified_config::ConfigBuilder::new()
        .load_global_toml()
        .build()
        .hooks
        .and_then(|h| h.github)
        .and_then(|g| g.secret)
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

/// Validate bind parameters before opening a listener.
///
/// This is the single guard for `shannon serve` / `shannon_server::run` /
/// any future server entry point (refs review §P0-2). Loopback binds are
/// always permitted (desktop, local TUI). Non-loopback binds require
/// explicit `allow_nonloopback = true` AND an auth token; without either,
/// the call refuses — this closes the LAN-RCE hole where
/// `shannon serve --host 0.0.0.0` exposed an unauthenticated agent API.
pub fn validate_serve_bind(
    host: &str,
    allow_nonloopback: bool,
    token: Option<&str>,
) -> Result<(), String> {
    if is_loopback_host(host) {
        return Ok(());
    }
    if !allow_nonloopback {
        return Err(format!(
            "refusing to bind non-loopback host '{host}': pass --allow-nonloopback to opt in"
        ));
    }
    if token.map(str::is_empty).unwrap_or(true) {
        return Err(format!(
            "non-loopback host '{host}' requires --auth-token to be set"
        ));
    }
    Ok(())
}

/// Return true for loopback bind hosts (`127.0.0.0/8`, `::1`, `localhost`).
///
/// Note: `0.0.0.0` is deliberately NOT treated as loopback. It binds all
/// interfaces on Linux/macOS and exposes the service to the LAN; callers
/// must either pass an explicit loopback address (`127.0.0.1`) or set
/// `allow_nonloopback = true` with an auth token (review §P0-2).
pub fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "::1") || host.starts_with("127.")
}

pub async fn run(
    host: &str,
    port: u16,
    client_config: LlmClientConfig,
    token: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Default to NOT allowing non-loopback binds; the CLI passes through its
    // --allow-nonloopback flag explicitly. This means the library entry point
    // is safe-by-default even if a future caller forgets to thread the flag.
    validate_serve_bind(host, false, token.as_deref())?;
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    axum::serve(listener, router(client_config, token)).await?;
    Ok(())
}

/// Run the server with explicit control over the non-loopback opt-in flag.
/// This is the variant the CLI should call after validating user intent.
pub async fn run_with_allow_nonloopback(
    host: &str,
    port: u16,
    client_config: LlmClientConfig,
    token: Option<String>,
    allow_nonloopback: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    validate_serve_bind(host, allow_nonloopback, token.as_deref())?;
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

    #[tokio::test]
    async fn message_route_accepts_bodies_above_axum_default_limit() {
        // axum 0.7's default body limit is 2 MiB, which 413'd every real
        // multimodal message. The raised limit must let a 3 MiB JSON body
        // reach the handler — it then 404s on the unknown session, so any
        // status other than 413 proves the body was consumed.
        let app = router_with_secret(test_config(), None, None);
        let body = format!("{{\"content\":\"{}\"}}", "x".repeat(3 * 1024 * 1024));
        let req = Request::builder()
            .method("POST")
            .uri("/v1/sessions/00000000-0000-0000-0000-000000000000/messages")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .expect("build request");
        let res = app.oneshot(req).await.expect("oneshot");
        let status = res.status();
        assert_ne!(
            status,
            StatusCode::PAYLOAD_TOO_LARGE,
            "body limit still stuck at the axum default"
        );
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND,
            "unexpected status {status}"
        );
    }

    // -------------------------------------------------------------------
    // review §P0-2: serve bind guard
    // -------------------------------------------------------------------

    #[test]
    fn validate_serve_bind_loopback_always_ok() {
        for host in ["127.0.0.1", "127.0.0.42", "localhost", "::1"] {
            assert!(
                validate_serve_bind(host, false, None).is_ok(),
                "loopback host {host} must be allowed without opt-in or token"
            );
            assert!(
                validate_serve_bind(host, true, Some("secret")).is_ok(),
                "loopback host {host} must be allowed even with extra opts"
            );
        }
    }

    #[test]
    fn validate_serve_bind_nonloopback_refused_without_opt_in() {
        for host in ["0.0.0.0", "192.168.1.5", "10.0.0.1", "::", "example.com"] {
            assert!(
                validate_serve_bind(host, false, None).is_err(),
                "non-loopback host {host} without --allow-nonloopback must be refused"
            );
            assert!(
                validate_serve_bind(host, false, Some("secret")).is_err(),
                "non-loopback host {host} without --allow-nonloopback must be refused even with token"
            );
        }
    }

    #[test]
    fn validate_serve_bind_nonloopback_requires_token() {
        for host in ["0.0.0.0", "192.168.1.5"] {
            assert!(
                validate_serve_bind(host, true, None).is_err(),
                "non-loopback host {host} with opt-in but no token must be refused"
            );
            assert!(
                validate_serve_bind(host, true, Some("")).is_err(),
                "empty token must be rejected"
            );
            assert!(
                validate_serve_bind(host, true, Some("secret")).is_ok(),
                "non-loopback host {host} with opt-in AND token must be allowed"
            );
        }
    }
}
