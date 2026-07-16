//! Loopback engine API server (P0.1).
//!
//! The desktop shell embeds the Shannon engine in-process; until now it did
//! not expose any HTTP/WS surface, so the supervised `shannon-gateway` had
//! nowhere to connect — its `engine.wsUrl` pointed at nothing. This module
//! spawns `shannon_core::api_server::ShannonApiServer` on the loopback
//! interface so the gateway (and, later, the mobile bridge through the
//! gateway) can reach the in-process engine at
//! `ws://127.0.0.1:{LOOPBACK_PORT}/api/ws`.
//!
//! The bind is **always** loopback. Remote/mobile access is carried by
//! `shannon-relay` (E2E), never by widening this bind — see
//! `claudedocs/mobile-host-architecture.md` and P0.2 (CORS/auth hardening)
//! before any change here.

use shannon_core::api_server::ShannonApiServer;
use shannon_core::tools::ToolRegistry;
use shannon_engine::api::types::LlmClientConfig;
use shannon_tools::register_default_tools;

use crate::commands::AppState;

/// Loopback bind address — always `127.0.0.1`, never widened.
pub const LOOPBACK_HOST: &str = "127.0.0.1";

/// Loopback bind port. Mirrors the gateway config's default `engine.wsUrl`
/// (`ws://127.0.0.1:33420/api/ws`).
pub const LOOPBACK_PORT: u16 = 33420;

/// Build the loopback engine API server from an LLM client config plus a
/// freshly-registered default tool set. Pure construction — does not bind.
pub fn build_server(client_config: LlmClientConfig) -> ShannonApiServer {
    let mut tools = ToolRegistry::new();
    if let Err(e) = register_default_tools(&mut tools) {
        tracing::warn!("loopback engine API server: default tool registration failed: {e}");
    }
    ShannonApiServer::new(client_config)
        .with_tools(tools)
        .host(LOOPBACK_HOST)
        .port(LOOPBACK_PORT)
}

/// Spawn the loopback engine API server on a detached background task.
///
/// Reads the app's current LLM client config, builds the server bound to
/// `127.0.0.1:{LOOPBACK_PORT}`, and runs `serve()` on a spawned task that
/// lives for the rest of the process. Bind/runtime failures are logged, not
/// fatal — the desktop UI uses the engine in-process directly and is
/// unaffected by the loopback server's health.
///
/// Must be awaited from a tokio runtime context (reads the async RwLock).
pub async fn spawn(state: &AppState) {
    let client_config = state.client_config.read().await.clone();
    let server = build_server(client_config);
    tracing::info!("Spawning loopback engine API server on {LOOPBACK_HOST}:{LOOPBACK_PORT}");
    tokio::spawn(async move {
        if let Err(e) = server.serve().await {
            tracing::error!("Loopback engine API server exited: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The loopback server, built via the same path as production, must
    /// actually listen and answer `/api/health` on the loopback interface.
    /// Uses a throwaway port (not `LOOPBACK_PORT`) so the test is hermetic
    /// and parallel-safe.
    #[tokio::test]
    async fn loopback_server_answers_health() {
        // Reserve a free port, then release it for the server to bind.
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind probe");
        let port = probe.local_addr().expect("probe addr").port();
        drop(probe);

        // Same construction as `build_server`, overridden to the free port.
        let server = build_server(LlmClientConfig::default()).port(port);
        tokio::spawn(async move {
            let _ = server.serve().await;
        });

        // Give the listener a moment to bind.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let resp = reqwest::get(format!("http://127.0.0.1:{port}/api/health"))
            .await
            .expect("GET /api/health");
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = resp.json().await.expect("health json");
        assert_eq!(body["status"], "ok");
    }
}
