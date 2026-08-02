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
}
#[derive(OpenApi)]
#[openapi(
    paths(routes::create_session, routes::get_session, routes::post_message),
    components(schemas(
        routes::CreateSessionRequest,
        routes::CreateSessionResponse,
        routes::MessageRequest,
        sessions::SessionSummary
    ))
)]
pub struct ApiDoc;

pub fn router(client_config: LlmClientConfig, token: Option<String>) -> Router {
    let state = AppState {
        client_config,
        sessions: sessions::SessionRegistry::default(),
    };
    Router::new()
        .route("/v1/sessions", post(routes::create_session))
        .route("/v1/sessions/:id", get(routes::get_session))
        .route("/v1/sessions/:id/messages", post(routes::post_message))
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
