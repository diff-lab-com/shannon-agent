use axum::{
    extract::Request,
    http::{StatusCode, header},
    middleware::Next,
    response::Response,
};
use subtle::ConstantTimeEq;

#[derive(Clone, Debug)]
pub struct AuthConfig {
    token: Option<String>,
}

impl AuthConfig {
    pub fn new(token: Option<String>) -> Self {
        Self { token }
    }
    #[allow(dead_code)]
    pub fn from_env() -> Self {
        Self::new(
            std::env::var("SHANNON_SERVE_TOKEN")
                .ok()
                .filter(|v| !v.is_empty()),
        )
    }
}

/// Configuration for the router-level Host guard (review F15).
///
/// The allowlist is always the loopback names (`127.0.0.1`, `localhost`,
/// `[::1]`/`::1`) plus [`Self::extra_hosts`]. Callers pass the literal bound
/// host there so binds like `127.0.0.2` (or a host the operator explicitly
/// opted into) keep working.
///
/// `extra_hosts` is also fed by the `SHANNON_SERVE_EXTRA_HOSTS` env var
/// (comma-separated, read once at router/run construction in
/// `crate::guard_for_bind`/`effective_extra_hosts` — T15c): clients that
/// must reach a loopback-bound server through a name resolving to loopback
/// (hosts-file alias, SSH tunnel hostname, reverse proxy) send that name in
/// the `Host` header, and without an allowlist entry the guard would 403
/// them. Empty/absent env leaves the default policy untouched.
#[derive(Clone, Debug, Default)]
pub struct HostGuardConfig {
    /// Additional accepted Host values (bare or `host:port` form).
    pub extra_hosts: Vec<String>,
}

impl HostGuardConfig {
    /// Accept these additional Host values (case-insensitive, bare or
    /// `host:port`).
    pub fn with_extra_hosts(mut self, hosts: Vec<String>) -> Self {
        self.extra_hosts = hosts;
        self
    }
}

/// Router-level Host guard (review F15, DNS-rebinding).
///
/// A server bound to loopback with no auth token (the default `shannon
/// serve`) is reachable by any web page via DNS rebinding: the attacker's
/// domain resolves to `127.0.0.1`, the browser sends `Host: attacker.com:<port>`,
/// and same-origin rules let the page read responses. Browsers always set a
/// `Host` header naming the domain they were told to contact — so rejecting
/// any Host outside the loopback allowlist breaks the rebinding while
/// leaving direct loopback clients untouched. Legitimate non-loopback names
/// can be opted in per deployment via `SHANNON_SERVE_EXTRA_HOSTS` (T15c —
/// see [`HostGuardConfig`]); everything else stays rejected.
///
/// A request **without** a Host header is allowed: HTTP/1.0 tooling and
/// in-process probes send none, and no browser ever does.
pub async fn host_guard_middleware(
    axum::extract::State(cfg): axum::extract::State<HostGuardConfig>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(host) = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
    else {
        return Ok(next.run(request).await);
    };
    // The allowlist predicate is shared with the desktop-embedded server.
    if shannon_core::api_server::host_header_allowed(host, &cfg.extra_hosts) {
        Ok(next.run(request).await)
    } else {
        tracing::warn!(
            host,
            "rejected request: Host header is outside the loopback allowlist (possible DNS rebinding)"
        );
        Err(StatusCode::FORBIDDEN)
    }
}

pub async fn bearer_middleware(
    axum::extract::State(auth): axum::extract::State<AuthConfig>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(expected) = auth.token.as_deref() else {
        return Ok(next.run(request).await);
    };
    // The GitHub webhook endpoint (P2-7) authenticates via its own
    // `X-Hub-Signature-256` HMAC against `[hooks.github] secret`; GitHub
    // cannot send our bearer token, so the bearer check does not apply
    // (a missing secret still yields 503 from the handler itself).
    if request.uri().path() == crate::github::GITHUB_HOOK_PATH {
        return Ok(next.run(request).await);
    }
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match supplied {
        Some(value) if value.as_bytes().ct_eq(expected.as_bytes()).into() => {
            Ok(next.run(request).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
