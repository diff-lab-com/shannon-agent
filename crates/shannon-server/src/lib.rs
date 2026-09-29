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
    /// Shared shutdown flag (T4): flipped to `true` when the serve loop
    /// begins draining (SIGINT/SIGTERM). SSE response bodies watch it so
    /// in-flight streams end with a terminal `error` frame (see
    /// [`sse::with_shutdown_and_cap`]) instead of being cut mid-frame or
    /// pinging forever. Routers built outside the serve loop get a channel
    /// whose sender is dropped immediately — the flag can then never fire,
    /// which the SSE guard treats as "no shutdown will happen".
    pub shutdown: tokio::sync::watch::Receiver<bool>,
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
    full_router(
        client_config,
        token,
        Some(auth::HostGuardConfig::default()),
        detached_shutdown_receiver(),
    )
}

/// Production router builder shared by [`router`] and the `run` entry
/// points: resolves the config-file secrets, the effective serve token and
/// the shared inbox, then delegates to [`router_full`]. `host_guard` is
/// `Some` for loopback binds (F15) and `None` for non-loopback opt-in binds,
/// where a token is mandatory and the client-facing hostname is unknowable.
/// `shutdown` is the drain flag the serve loop flips on SIGINT/SIGTERM (T4);
/// routers built outside a serve loop pass [`detached_shutdown_receiver`].
fn full_router(
    client_config: LlmClientConfig,
    token: Option<String>,
    host_guard: Option<auth::HostGuardConfig>,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> Router {
    let secret = read_webhook_secret();
    let github_secret = github_secret_from_config();
    let routines = github::load_routines();
    let inbox = std::sync::Arc::new(github::open_inbox());
    router_full_with_shutdown(
        client_config,
        effective_serve_token(token),
        secret,
        github_secret,
        routines,
        inbox,
        host_guard,
        shutdown,
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
/// working (F15), as do the `SHANNON_SERVE_EXTRA_HOSTS` entries (T15c).
fn guard_for_bind(host: &str) -> Option<auth::HostGuardConfig> {
    if is_loopback_host(host) {
        let mut extra_hosts = effective_extra_hosts();
        extra_hosts.push(host.to_string());
        Some(auth::HostGuardConfig::default().with_extra_hosts(extra_hosts))
    } else {
        None
    }
}

/// Extra Host allowlist entries from `SHANNON_SERVE_EXTRA_HOSTS` (T15c,
/// comma-separated), read once at router/run construction — same pattern as
/// [`effective_serve_token`]. The loopback Host guard's allowlist becomes
/// `loopback names + literal bound host + these entries`, for clients that
/// must reach a loopback-bound server through a name that resolves to
/// loopback (hosts-file alias, SSH tunnel hostname, reverse proxy). Empty
/// or absent → no extra entries (the default policy is unchanged). Entries
/// are trimmed; empty items are skipped.
fn effective_extra_hosts() -> Vec<String> {
    std::env::var("SHANNON_SERVE_EXTRA_HOSTS")
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// A shutdown watch that can never fire (T4): the sender is dropped before
/// the receiver is returned, so `changed()` reports closed forever. Used by
/// every router built outside the serve loop (`router`, the test seams);
/// the SSE shutdown guard treats a closed channel as "no shutdown".
fn detached_shutdown_receiver() -> tokio::sync::watch::Receiver<bool> {
    let (tx, rx) = tokio::sync::watch::channel(false);
    drop(tx);
    rx
}

/// [`router`] with all state injected (test seam for the GitHub hook).
///
/// `host_guard`: `Some` installs the F15 Host-header guard (use
/// [`auth::HostGuardConfig::default`] for the production loopback policy),
/// `None` skips it. The `token` is taken as-is — unlike the production
/// builder it does NOT consult `SHANNON_SERVE_TOKEN`, but it does feed both
/// the middleware and `AppState::auth_token` (F47 contract). The shutdown
/// flag is a [`detached_shutdown_receiver`] — no serve loop will ever fire
/// it; use [`router_full_with_shutdown`] to inject a real one.
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
    router_full_with_shutdown(
        client_config,
        token,
        webhook_secret,
        github_secret,
        routines,
        inbox,
        host_guard,
        detached_shutdown_receiver(),
    )
}

/// [`router_full`] with the serve loop's drain flag injected (T4): SSE
/// response bodies hold clones of this receiver and end with a terminal
/// `error` frame once it flips `true`.
// KEEP: the argument list mirrors `router_full` (its public test seam) plus
// exactly one shutdown receiver; bundling them into a struct would churn
// every call site for one added field.
#[allow(clippy::too_many_arguments)]
fn router_full_with_shutdown(
    client_config: LlmClientConfig,
    token: Option<String>,
    webhook_secret: Option<String>,
    github_secret: Option<String>,
    routines: Vec<shannon_core::scheduled_routines::ScheduledRoutine>,
    inbox: std::sync::Arc<shannon_core::inbox_store::InboxStore>,
    host_guard: Option<auth::HostGuardConfig>,
    shutdown: tokio::sync::watch::Receiver<bool>,
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
        shutdown,
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
///
/// Shuts down gracefully (T4): on SIGINT (Ctrl-C) or SIGTERM the listener
/// stops accepting, every in-flight SSE stream receives a terminal `error`
/// frame ([`sse::with_shutdown_and_cap`]) and completes, and once all
/// connections drain — or after `SHUTDOWN_DRAIN_WINDOW`, whichever comes
/// first — the future resolves. A wedged query can therefore no longer hold
/// the process open past the drain window.
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
    // T4: the drain flag is created before the router so SSE response bodies
    // can observe it.
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let router = full_router(client_config, token, guard_for_bind(host), shutdown_rx);
    serve_loop(
        listener,
        router,
        shutdown_signal(),
        shutdown_tx,
        SHUTDOWN_DRAIN_WINDOW,
    )
    .await
}

/// How long in-flight requests (SSE streams in particular) may keep
/// draining after a shutdown signal before the serve loop gives up on them
/// and forces exit (T4). Bounded so a wedged query cannot hold the process
/// open indefinitely.
const SHUTDOWN_DRAIN_WINDOW: std::time::Duration = std::time::Duration::from_secs(30);

/// Resolve when the process is asked to terminate: Ctrl-C (`SIGINT`) or
/// `SIGTERM` (unix; on other platforms only Ctrl-C is watched).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C signal handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("shutdown signal: Ctrl-C"),
        _ = terminate => tracing::info!("shutdown signal: SIGTERM"),
    }
}

/// The serve loop shared by [`run_with_allow_nonloopback`] and the shutdown
/// tests (T4).
///
/// `shutdown` is the production signal source ([`shutdown_signal`]) or a
/// test trigger; when it resolves, the loop (a) stops accepting new
/// connections via `with_graceful_shutdown` and (b) flips the shared drain
/// flag so in-flight SSE bodies emit their terminal frame and finish. If
/// connections are still open after `drain` has elapsed from that moment,
/// the loop forces exit and returns `Ok(())` (a signal-driven exit is not
/// an error; the OS reaps whatever is left when the process ends).
async fn serve_loop(
    listener: tokio::net::TcpListener,
    router: Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    drain: std::time::Duration,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // One trigger feeds two consumers (graceful stop + bounded drain), and a
    // plain future can only be awaited once — forward it through a watch.
    // Both receivers subscribe before the sender moves into the forwarder.
    let graceful_rx = shutdown_tx.subscribe();
    let forced_rx = shutdown_tx.subscribe();
    let forwarder = tokio::spawn(async move {
        shutdown.await;
        tracing::info!(?drain, "shutting down: draining in-flight connections");
        let _ = shutdown_tx.send(true);
    });
    // Resolves as soon as the flag flips: `with_graceful_shutdown` then waits
    // for the (now-terminating) in-flight connections.
    let graceful = shutdown_fired(graceful_rx);
    // Resolves `drain` after the flag flips: the forced-exit backstop.
    let forced = async move {
        shutdown_fired(forced_rx).await;
        tokio::time::sleep(drain).await;
    };
    let outcome = tokio::select! {
        result = axum::serve(listener, router).with_graceful_shutdown(graceful) => Some(result),
        _ = forced => None,
    };
    forwarder.abort();
    match outcome {
        Some(Ok(())) => {
            tracing::info!("server stopped cleanly");
            Ok(())
        }
        Some(Err(e)) => Err(e.into()),
        None => {
            tracing::warn!(
                ?drain,
                "drain window elapsed with connections still open; forcing shutdown"
            );
            Ok(())
        }
    }
}

/// Resolves once the drain flag is `true`; a closed channel (every sender
/// gone) is treated as "this flag will never fire" and pends forever.
/// (`changed` + short-lived `borrow`, because `Receiver::wait_for` holds a
/// !Send guard across awaits and would poison the serve future's Send-ness.)
async fn shutdown_fired(mut rx: tokio::sync::watch::Receiver<bool>) {
    loop {
        if rx.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
        if *rx.borrow() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_config() -> LlmClientConfig {
        shannon_core::LlmClientConfig {
            thinking_type: None,
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

    // -------------------------------------------------------------------
    // T15c: SHANNON_SERVE_EXTRA_HOSTS → HostGuardConfig wiring
    // -------------------------------------------------------------------

    /// SAFETY: nextest runs each test in its own process, so this env
    /// mutation cannot be observed by sibling tests.
    #[test]
    fn effective_extra_hosts_parses_comma_list_and_defaults_empty() {
        unsafe {
            std::env::set_var(
                "SHANNON_SERVE_EXTRA_HOSTS",
                " tunnel.example.com , myhost.local,, ",
            )
        };
        assert_eq!(
            effective_extra_hosts(),
            vec!["tunnel.example.com".to_string(), "myhost.local".to_string()],
            "entries are trimmed and empty items skipped"
        );
        unsafe { std::env::set_var("SHANNON_SERVE_EXTRA_HOSTS", "") };
        assert!(
            effective_extra_hosts().is_empty(),
            "empty value = no entries"
        );
        unsafe { std::env::remove_var("SHANNON_SERVE_EXTRA_HOSTS") };
        assert!(
            effective_extra_hosts().is_empty(),
            "absent var = no entries (current behavior)"
        );
    }

    /// SAFETY: nextest runs each test in its own process, so this env
    /// mutation cannot be observed by sibling tests.
    #[test]
    fn guard_for_bind_appends_env_extra_hosts_after_the_bound_host() {
        unsafe { std::env::set_var("SHANNON_SERVE_EXTRA_HOSTS", "myhost.local") };
        let guard = guard_for_bind("127.0.0.1").expect("loopback bind installs a guard");
        assert_eq!(
            guard.extra_hosts,
            vec!["myhost.local".to_string(), "127.0.0.1".to_string()],
            "env entries come first, the literal bound host still joins"
        );
        unsafe { std::env::remove_var("SHANNON_SERVE_EXTRA_HOSTS") };
        let guard = guard_for_bind("127.0.0.1").expect("loopback bind installs a guard");
        assert_eq!(
            guard.extra_hosts,
            vec!["127.0.0.1".to_string()],
            "absent env = the pre-T15c allowlist"
        );
        assert!(
            guard_for_bind("0.0.0.0").is_none(),
            "non-loopback stays guardless"
        );
    }

    #[tokio::test]
    async fn env_extra_hosts_pass_the_guard_others_stay_403() {
        unsafe { std::env::set_var("SHANNON_SERVE_EXTRA_HOSTS", "myhost.local") };
        let guard = guard_for_bind("127.0.0.1").expect("loopback bind installs a guard");
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
            Some(guard),
        );
        for host in ["myhost.local", "myhost.local:3000", "127.0.0.1"] {
            assert_eq!(
                get_session_status(app.clone(), Some(host)).await,
                StatusCode::NOT_FOUND,
                "Host '{host}' must pass the guard (404 = handler ran)"
            );
        }
        for host in ["evil.com", "other.local"] {
            assert_eq!(
                get_session_status(app.clone(), Some(host)).await,
                StatusCode::FORBIDDEN,
                "Host '{host}' must still be rejected"
            );
        }
        unsafe { std::env::remove_var("SHANNON_SERVE_EXTRA_HOSTS") };
    }

    // -------------------------------------------------------------------
    // T4: graceful shutdown (bounded drain on SIGINT/SIGTERM)
    // -------------------------------------------------------------------

    #[tokio::test]
    async fn serve_loop_resolves_when_shutdown_trigger_fires() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind ephemeral port");
        let app = router_with_secret(test_config(), None, None);
        // Simulated Ctrl-C: the trigger resolves 50ms in, like a signal
        // arriving after the server has been running.
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let _ = tx.send(());
        });
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let serve = serve_loop(
            listener,
            app,
            async {
                let _ = rx.await;
            },
            shutdown_tx,
            // Drain window larger than the test timeout: resolution must come
            // from the graceful path, not the forced backstop.
            std::time::Duration::from_secs(30),
        );
        let started = std::time::Instant::now();
        let result = tokio::time::timeout(std::time::Duration::from_secs(10), serve)
            .await
            .expect("serve must resolve once the shutdown trigger fires");
        assert!(
            result.is_ok(),
            "graceful shutdown must not error: {result:?}"
        );
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(40),
            "serve must wait for the trigger, not exit early"
        );
    }

    #[tokio::test]
    async fn serve_loop_resolves_when_shutdown_trigger_is_dropped() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind ephemeral port");
        let app = router_with_secret(test_config(), None, None);
        // Dropping the trigger's sender also resolves the shutdown future
        // (rx.await returns Err) — shutdown must still proceed.
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        drop(tx);
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let serve = serve_loop(
            listener,
            app,
            async {
                let _ = rx.await;
            },
            shutdown_tx,
            std::time::Duration::from_secs(30),
        );
        let result = tokio::time::timeout(std::time::Duration::from_secs(10), serve)
            .await
            .expect("serve must resolve when the trigger is dropped");
        assert!(
            result.is_ok(),
            "graceful shutdown must not error: {result:?}"
        );
    }
}
