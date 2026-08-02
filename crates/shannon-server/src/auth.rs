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

pub async fn bearer_middleware(
    axum::extract::State(auth): axum::extract::State<AuthConfig>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(expected) = auth.token.as_deref() else {
        return Ok(next.run(request).await);
    };
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
