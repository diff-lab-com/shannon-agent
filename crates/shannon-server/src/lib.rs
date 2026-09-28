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
    full_router(client_config, token, Some(auth::HostGuardConfig::default()))
}

/// Production router builder shared by [`router`] and the `run` entry
/// points: resolves the config-file secrets, the effective serve token and
/// the shared inbox, then delegates to [`router_full`]. `host_guard` is
/// `Some` for loopback binds (F15) and `None` for non-loopback opt-in binds,
/// where a token is mandatory and the client-facing hostname is unknowable.
fn full_router(
    client_config: LlmClientConfig,
    token: Option<String>,
    host_guard: Option<auth::HostGuardConfig>,
) -> Router {
    let secret = read_webhook_secret();
    let github_secret = github_secret_from_config();
    let routines = github::load_routines();
    let inbox = std::sync::Arc::new(github::open_inbox());
    router_full(
        client_config,
        effective_serve_token(token),
        secret,
        github_secret,
        routines,
        inbox,
        host_guard,
    )
}

/// Effective bearer token: an explicit token wins, else `SHANNON_SERVE_TOKEN`.
///
/// Review F47: this single value must feed BOTH the bearer middleware and
/// `AppState::auth_token` (the trigger endpoint's `bearer_ok` decision).
/// Previously the middleware applied the env fallback while the state did
/// not, so a server started with only `SHANNON_SERVE_TOKEN` enforced bearer
/// auth yet refused an authenticated client on `POST /routines/:id/trigger`.
///
/// Kept a free function (not inlined into [`router_full`]) so the env read
/// stays out of the test seam — tests inject the token explicitly.
fn effective_serve_token(explicit: Option<String>) -> Option<String> {
    explicit.or_else(|| std::env::var("SHANNON_SERVE_TOKEN").ok())
}

/// Host guard to install for a bind host: enforcing on loopback (the
/// DNS-rebinding surface), absent for non-loopback binds (token-gated).
/// The literal bound host joins the allowlist so e.g. `127.0.0.2` keeps
/// working (F15).
fn guard_for_bind(host: &str) -> Option<auth::HostGuardConfig> {
    if is_loopback_host(host) {
        Some(auth::HostGuardConfig::default().with_extra_hosts(vec![host.to_string()]))
    } else {
        None
    }
}

/// [`router`] with all state injected (test seam for the GitHub hook).
///
/// `host_guard`: `Some` installs the F15 Host-header guard (use
/// [`auth::HostGuardConfig::default`] for the production loopback policy),
/// `None` skips it. The `token` is taken as-is — unlike the production
/// builder it does NOT consult `SHANNON_SERVE_TOKEN`, but it does feed both
/// the middleware and `AppState::auth_token` (F47 contract).
#[doc(hidden)]
pub fn router_full(
    client_config: LlmClientConfig,
    token: Option<String>,
    webhook_secret: Option<String>,
    github_secret: Option<String>,
    routines: Vec<shannon_core::scheduled_routines::ScheduledRoutine>,
    inbox: std::sync::Arc<shannon_core::inbox_store::InboxStore>,
    host_guard: Option<auth::HostGuardConfig>,
) -> Router {
    let state = AppState {
        client_config,
        sessions: sessions::SessionRegistry::default(),
        webhook_secret,
        // F47: the same effective token the middleware below enforces.
        auth_token: token.clone(),
        github_secret,
        github_deliveries: github::delivery_cache(),
        routines: std::sync::Arc::new(routines),
        inbox,
    };
    let mut router = Router::new()
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
            // Review F15: this stays — it is the attachment contract, and
            // the Host guard above is what closes the unauthenticated
            // drive-by surface.
            axum::extract::DefaultBodyLimit::max(128 * 1024 * 1024),
        );
    router = router.layer(middleware::from_fn_with_state(
        auth::AuthConfig::new(token),
        auth::bearer_middleware,
    ));
    // F15: outermost layer so a rebinding Host is rejected before any auth
    // or body processing happens.
    if let Some(guard) = host_guard {
        router = router.layer(middleware::from_fn_with_state(
            guard,
            auth::host_guard_middleware,
        ));
    }
    router.with_state(state)
}

/// [`router`] with only the legacy trigger-endpoint secret injected — the
/// pre-P2-7 test seam, kept for the existing trigger tests. The GitHub hook
/// is disabled (no secret) and backed by an in-memory inbox and no routines.
/// Installs the production loopback Host guard (F15).
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
        Some(auth::HostGuardConfig::default()),
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
    run_with_allow_nonloopback(host, port, client_config, token, false).await
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
    // F15: enforce the Host guard on loopback binds (the drive-by surface);
    // non-loopback binds already require the token and get no guard, since
    // clients may address the host by any of its DNS names.
    axum::serve(
        listener,
        full_router(client_config, token, guard_for_bind(host)),
    )
    .await?;
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

    // -------------------------------------------------------------------
    // review F15: Host-header guard (DNS-rebinding)
    // -------------------------------------------------------------------

    /// GET an unknown session and report the status. Any status other than
    /// 403 proves the request got past the Host guard (404 = handler ran).
    async fn get_session_status(app: Router, host: Option<&str>) -> StatusCode {
        let mut req = Request::builder().uri("/v1/sessions/00000000-0000-0000-0000-000000000000");
        if let Some(h) = host {
            req = req.header("host", h);
        }
        let res = app
            .oneshot(req.body(Body::empty()).expect("build request"))
            .await
            .expect("oneshot");
        res.status()
    }

    #[tokio::test]
    async fn host_guard_rejects_rebound_hostnames() {
        let app = router_with_secret(test_config(), None, None);
        for host in [
            "evil.com",
            "evil.com:8080",
            "127.0.0.1.evil.com",
            "localhost.evil.com",
            "metadata.google.internal",
        ] {
            assert_eq!(
                get_session_status(app.clone(), Some(host)).await,
                StatusCode::FORBIDDEN,
                "Host '{host}' must be rejected by the rebinding guard"
            );
        }
    }

    #[tokio::test]
    async fn host_guard_allows_loopback_host_forms() {
        let app = router_with_secret(test_config(), None, None);
        for host in [
            "127.0.0.1",
            "127.0.0.1:3000",
            "localhost",
            "LOCALHOST:8080",
            "::1",
            "[::1]",
            "[::1]:9000",
        ] {
            assert_eq!(
                get_session_status(app.clone(), Some(host)).await,
                StatusCode::NOT_FOUND,
                "Host '{host}' must pass the guard (404 = unknown session, not 403)"
            );
        }
        // No Host header at all (HTTP/1.0 tooling, in-process probes) passes:
        // no browser ever omits it.
        assert_eq!(
            get_session_status(app.clone(), None).await,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn host_guard_allows_the_literal_bound_host() {
        // A loopback bind that is not one of the well-known names (e.g.
        // 127.0.0.2) must stay reachable through its literal form via
        // extra_hosts — this is what `guard_for_bind` wires up.
        let inbox = std::sync::Arc::new(
            shannon_core::inbox_store::InboxStore::open_in_memory()
                .expect("in-memory inbox always opens"),
        );
        let app = router_full(
            test_config(),
            None,
            None,
            None,
            Vec::new(),
            inbox,
            Some(auth::HostGuardConfig::default().with_extra_hosts(vec!["127.0.0.2".to_string()])),
        );
        assert_eq!(
            get_session_status(app.clone(), Some("127.0.0.2:9999")).await,
            StatusCode::NOT_FOUND,
            "the literal bound host must be allowed"
        );
        assert_eq!(
            get_session_status(app.clone(), Some("evil.com")).await,
            StatusCode::FORBIDDEN,
            "everything else stays rejected"
        );
    }

    // -------------------------------------------------------------------
    // review F47: one effective token for middleware AND trigger endpoint
    // -------------------------------------------------------------------

    #[test]
    fn effective_serve_token_env_fallback() {
        // No explicit token → the env var is honored…
        // SAFETY: this is the only place in this crate's tests that touches
        // the process env; router_full and every other test path read the
        // token exclusively from their arguments, so no sibling test can
        // observe this mutation even when `cargo test` shares the process.
        unsafe { std::env::set_var("SHANNON_SERVE_TOKEN", "env-only-token") };
        assert_eq!(
            effective_serve_token(None).as_deref(),
            Some("env-only-token")
        );
        // …and an explicit token wins over it.
        assert_eq!(
            effective_serve_token(Some("explicit".into())).as_deref(),
            Some("explicit")
        );
        unsafe { std::env::remove_var("SHANNON_SERVE_TOKEN") };
        assert_eq!(effective_serve_token(None), None);
    }

    #[tokio::test]
    async fn router_full_token_feeds_both_middleware_and_trigger() {
        // The F47 contract: with a token configured (explicit or — in
        // production — via effective_serve_token), the middleware rejects
        // requests without the right bearer (401), and the trigger endpoint
        // treats a valid bearer as pre-authenticated (501 serve limitation,
        // not 403).
        let app = router_with_secret(test_config(), Some("tok".into()), None);

        for auth in [None, Some("Bearer wrong")] {
            let mut req = Request::builder()
                .method("POST")
                .uri("/routines/r1/trigger")
                .header("content-type", "application/json");
            if let Some(a) = auth {
                req = req.header("authorization", a);
            }
            let res = app
                .clone()
                .oneshot(req.body(Body::from("{}")).unwrap())
                .await
                .unwrap();
            assert_eq!(
                res.status(),
                StatusCode::UNAUTHORIZED,
                "missing/wrong bearer must be rejected by the middleware"
            );
        }

        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/routines/r1/trigger")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer tok")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::NOT_IMPLEMENTED,
            "valid bearer replaces HMAC at the trigger endpoint"
        );
    }
}
