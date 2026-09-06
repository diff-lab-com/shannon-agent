//! Loopback engine API server (P0.1) + routine trigger endpoint (P0-3).
//!
//! The desktop shell embeds the Shannon engine in-process; until now it did
//! not expose any HTTP/WS surface, so the supervised `shannon-gateway` had
//! nowhere to connect — its `engine.wsUrl` pointed at nothing. This module
//! spawns `shannon_core::api_server::ShannonApiServer` on the loopback
//! interface so the gateway (and, later, the mobile bridge through the
//! gateway) can reach the in-process engine at
//! `ws://127.0.0.1:{LOOPBACK_PORT}/api/ws`.
//!
//! P0-3 adds `POST /api/routines/:id/trigger` to the same listener (merged
//! into the router via `ShannonApiServer::with_extra_routes`). It fires a
//! scheduled task through the shared unattended execution path
//! ([`crate::inbox_commands::spawn_routine_run`]) and records the run in the
//! SQLite inbox (`source=trigger`).
//!
//! ## Auth contract (HMAC, shared with the notifier webhook signing)
//!
//! - No `[notifications.webhook] secret` configured → **403** (disabled by
//!   default — safe default, the endpoint is opt-in via the same secret the
//!   webhook notifier uses).
//! - With a secret: the request must carry
//!   `X-Shannon-Signature: sha256=<hex>` computed over the **raw** request
//!   body (see `shannon_core::webhook::verify_signature`). Missing or wrong
//!   signature → **401**.
//! - Success → **202** `{"runId": "..."}`; the run completes asynchronously.
//!
//! The bind is **always** loopback. Remote/mobile access is carried by
//! `shannon-relay` (E2E), never by widening this bind — see
//! `claudedocs/mobile-host-architecture.md` and P0.2 (CORS/auth hardening)
//! before any change here.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::Deserialize;
use shannon_core::api_server::ShannonApiServer;
use shannon_core::inbox_store::SOURCE_TRIGGER;
use shannon_core::scheduled_task_store::ScheduledTaskStore;
use shannon_core::tools::ToolRegistry;
use shannon_engine::api::types::LlmClientConfig;
use shannon_tools::register_default_tools_with_providers;
use tokio::sync::RwLock;

use crate::commands::AppState;
use crate::commands_usage::UsageStore;
use crate::config::DesktopConfig;
use crate::inbox_commands::RoutineRunDeps;

/// Loopback bind address — always `127.0.0.1`, never widened.
pub const LOOPBACK_HOST: &str = "127.0.0.1";

/// Loopback bind port. Mirrors the gateway config's default `engine.wsUrl`
/// (`ws://127.0.0.1:33420/api/ws`).
pub const LOOPBACK_PORT: u16 = 33420;

/// HMAC signature header for the trigger endpoint — same header the notifier
/// sends on outgoing webhooks (`sha256=<hex>` over the raw body).
pub const TRIGGER_SIGNATURE_HEADER: &str = "X-Shannon-Signature";

/// State for the `POST /api/routines/:id/trigger` route. Holds Arc clones of
/// the AppState slices the runner needs plus the resolved webhook secret.
/// Generic over the Tauri runtime so tests can drive it with `mock_app`;
/// production uses the default (`Wry`).
pub(crate) struct TriggerState<R: tauri::Runtime = tauri::Wry> {
    pub(crate) app: tauri::AppHandle<R>,
    pub(crate) inbox: Arc<shannon_core::inbox_store::InboxStore>,
    pub(crate) task_store: Arc<ScheduledTaskStore>,
    pub(crate) runs_store: Arc<shannon_core::scheduled_runs::ScheduledRunsStore>,
    pub(crate) usage_store: Arc<UsageStore>,
    pub(crate) client_config: Arc<RwLock<LlmClientConfig>>,
    pub(crate) desktop_config: Arc<RwLock<DesktopConfig>>,
    pub(crate) tools: Arc<ToolRegistry>,
    /// Shared memory store handle (P2-4b) for the runner's engine.
    pub(crate) memory_store: crate::commands_memory::SharedMemoryStore,
    /// `[notifications.webhook] secret` resolved once at spawn time.
    /// `None` disables the endpoint (403) — safe default.
    pub(crate) secret: Option<String>,
}

// Manual impl: `derive(Clone)` would add an unnecessary `R: Clone` bound
// (`AppHandle<R>` is `Clone` for every `R: Runtime` — it is Arc-backed).
impl<R: tauri::Runtime> Clone for TriggerState<R> {
    fn clone(&self) -> Self {
        Self {
            app: self.app.clone(),
            inbox: self.inbox.clone(),
            task_store: self.task_store.clone(),
            runs_store: self.runs_store.clone(),
            usage_store: self.usage_store.clone(),
            client_config: self.client_config.clone(),
            desktop_config: self.desktop_config.clone(),
            tools: self.tools.clone(),
            memory_store: self.memory_store.clone(),
            secret: self.secret.clone(),
        }
    }
}

impl<R: tauri::Runtime> TriggerState<R> {
    /// Capture the runner-relevant slices of [`AppState`].
    pub(crate) fn from_state(
        state: &AppState,
        app: tauri::AppHandle<R>,
        secret: Option<String>,
    ) -> Self {
        Self {
            app,
            inbox: state.inbox_store(),
            task_store: state.scheduled_task_store.clone(),
            runs_store: state.scheduled_runs_store.clone(),
            usage_store: state.usage_store.clone(),
            client_config: state.client_config.clone(),
            desktop_config: state.desktop_config.clone(),
            tools: state.tools.clone(),
            memory_store: state.memory_store.clone(),
            secret,
        }
    }

    fn run_deps(&self) -> RoutineRunDeps {
        RoutineRunDeps {
            inbox: self.inbox.clone(),
            runs_store: self.runs_store.clone(),
            usage_store: self.usage_store.clone(),
            client_config: self.client_config.clone(),
            desktop_config: self.desktop_config.clone(),
            tools: self.tools.clone(),
            memory_store: self.memory_store.clone(),
        }
    }
}

/// Request body: `{}` or `{"note": "..."}`. Empty bodies are accepted.
#[derive(Debug, Default, Deserialize)]
struct TriggerBody {
    #[serde(default)]
    note: Option<String>,
}

/// `POST /api/routines/:id/trigger` — HMAC-authenticated routine fire.
pub(crate) async fn trigger_routine<R: tauri::Runtime>(
    State(ts): State<TriggerState<R>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    // 1. Auth — disabled without a secret (safe default).
    let Some(secret) = ts.secret.as_deref() else {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "routine trigger disabled: no [notifications.webhook] secret configured"
            })),
        ));
    };

    // 2. Auth — HMAC over the raw body. A missing header fails verification
    //    (401), same contract as the webhook receiver.
    let provided = headers
        .get(TRIGGER_SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !shannon_core::webhook::verify_signature(secret, &body, provided) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": format!("missing or invalid {TRIGGER_SIGNATURE_HEADER} header")
            })),
        ));
    }

    // 3. Body — `{}` / `{"note": "..."}` / empty.
    let payload: TriggerBody = if body.is_empty() {
        TriggerBody::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid JSON body: {e}") })),
            )
        })?
    };

    // 4. Resolve the routine (scheduled task store, by id or name).
    let routine = ts
        .task_store
        .load(&id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("task store error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("routine not found: {id}") })),
            )
        })?;

    // 5. Fire through the shared unattended execution path. The run record
    //    (`running`) is written synchronously; completion + the inbox item
    //    (source=`trigger`) land asynchronously.
    let run_id = crate::inbox_commands::spawn_routine_run(
        &ts.run_deps(),
        ts.app.clone(),
        routine,
        SOURCE_TRIGGER,
        payload.note,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
    })?;

    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "runId": run_id })),
    )
        .into_response())
}

/// Build the state-applied trigger router merged into the loopback engine
/// server. Exposed for tests: the route handler carries its own state and is
/// exercised directly against a real HTTP listener.
pub(crate) fn trigger_router<R: tauri::Runtime>(ts: TriggerState<R>) -> axum::Router {
    axum::Router::new()
        .route("/api/routines/:id/trigger", post(trigger_routine::<R>))
        .with_state(ts)
}

/// Build the loopback engine API server from an LLM client config plus a
/// freshly-registered default tool set. Pure construction — does not bind.
pub fn build_server(client_config: LlmClientConfig) -> ShannonApiServer {
    let mut tools = ToolRegistry::new();
    let assembly = shannon_remote::assembly::assemble_dynamic();
    if let Err(e) = register_default_tools_with_providers(&mut tools, &assembly.providers) {
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
/// `127.0.0.1:{LOOPBACK_PORT}` (with the P0-3 trigger endpoint merged in),
/// and runs `serve()` on a spawned task that lives for the rest of the
/// process. Bind/runtime failures are logged, not fatal — the desktop UI uses
/// the engine in-process directly and is unaffected by the loopback server's
/// health.
///
/// Must be awaited from a tokio runtime context (reads the async RwLock).
pub async fn spawn(state: &AppState, app: tauri::AppHandle) {
    let client_config = state.client_config.read().await.clone();
    let secret =
        crate::commands_notifications::load_desktop_webhook_config().and_then(|c| c.secret);
    let trigger_enabled = secret.is_some();
    let trigger = trigger_router(TriggerState::from_state(state, app, secret));
    let server = build_server(client_config).with_extra_routes(trigger);
    tracing::info!(
        "Spawning loopback engine API server on {LOOPBACK_HOST}:{LOOPBACK_PORT} \
         (POST /api/routines/:id/trigger {})",
        if trigger_enabled {
            "enabled"
        } else {
            "disabled (no webhook secret)"
        }
    );
    tokio::spawn(async move {
        if let Err(e) = server.serve().await {
            tracing::error!("Loopback engine API server exited: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use shannon_core::inbox_store::InboxStore;
    use shannon_core::scheduled_routines::ScheduledRoutine;
    use shannon_core::scheduled_runs::ScheduledRunsStore;
    use tower::ServiceExt;

    fn test_state<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        tmp: &std::path::Path,
        secret: Option<String>,
    ) -> TriggerState<R> {
        let tasks = ScheduledTaskStore::with_base(tmp.join("tasks"));
        let routine = ScheduledRoutine::new(
            "nightly-scan".to_string(),
            "run the nightly scan".to_string(),
            3600,
        );
        tasks.save(&routine).expect("save routine");

        TriggerState {
            app,
            inbox: Arc::new(
                InboxStore::open_with_legacy(&tmp.join("inbox.db"), None)
                    .expect("open temp inbox db"),
            ),
            task_store: Arc::new(tasks),
            runs_store: Arc::new(ScheduledRunsStore::with_base(tmp.join("runs"))),
            usage_store: Arc::new(UsageStore::with_path(tmp.join("usage.jsonl"))),
            client_config: Arc::new(RwLock::new(LlmClientConfig::default())),
            desktop_config: Arc::new(RwLock::new(DesktopConfig::default())),
            tools: Arc::new(ToolRegistry::new()),
            memory_store: crate::commands_memory::open_shared_store_at(tmp.join("memories")),
            secret,
        }
    }

    async fn post_trigger(
        router: axum::Router,
        id: &str,
        body: &str,
        signature: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut req = axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/routines/{id}/trigger"))
            .header("content-type", "application/json");
        if let Some(sig) = signature {
            req = req.header(TRIGGER_SIGNATURE_HEADER, sig);
        }
        let res = router
            .oneshot(req.body(axum::body::Body::from(body.to_string())).unwrap())
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

    fn test_app() -> tauri::AppHandle<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        app.handle().clone()
    }

    #[tokio::test]
    async fn trigger_requires_secret_returns_403() {
        let app = test_app();
        let tmp = tempfile::tempdir().unwrap();
        let ts = test_state(app, tmp.path(), None);
        let router = trigger_router(ts);
        let (status, body) = post_trigger(router, "whatever", "{}", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("secret"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn trigger_with_secret_rejects_missing_and_bad_signatures() {
        let app = test_app();
        let tmp = tempfile::tempdir().unwrap();
        let secret = "s3cret".to_string();
        let ts = test_state(app, tmp.path(), Some(secret.clone()));
        let router = trigger_router(ts);

        // Missing header → 401.
        let (status, body) = post_trigger(router.clone(), "nightly-scan", "{}", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

        // Tampered signature → 401.
        let bad = shannon_core::webhook::sign_signature("wrong", b"{}");
        let (status, body) = post_trigger(router, "nightly-scan", "{}", Some(&bad)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
        let _ = secret; // used only for clarity above
    }

    #[tokio::test]
    async fn trigger_with_valid_signature_starts_run_202() {
        let app = test_app();
        let tmp = tempfile::tempdir().unwrap();
        let secret = "s3cret".to_string();
        let ts = test_state(app, tmp.path(), Some(secret.clone()));
        // test_state backs the inbox with the tempdir, so the assertions
        // below read the same database the handler writes to.
        let inbox = ts.inbox.clone();
        let router = trigger_router(ts);

        let body = r#"{"note":"fired from CI"}"#;
        let sig = shannon_core::webhook::sign_signature(&secret, body.as_bytes());
        let (status, json) = post_trigger(router, "nightly-scan", body, Some(&sig)).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{json}");
        let run_id = json["runId"].as_str().expect("runId in response");
        assert!(!run_id.is_empty());

        // The run row was written synchronously as `running` before the 202.
        let runs = inbox.list_runs(10).unwrap();
        assert_eq!(runs.len(), 1, "one run recorded");
        assert_eq!(runs[0].id, run_id);
        assert_eq!(runs[0].status, "running");
        assert_eq!(runs[0].task_name.as_deref(), Some("nightly-scan"));
    }

    #[tokio::test]
    async fn trigger_unknown_routine_is_404() {
        let app = test_app();
        let tmp = tempfile::tempdir().unwrap();
        let secret = "s3cret".to_string();
        let ts = test_state(app, tmp.path(), Some(secret.clone()));
        let router = trigger_router(ts);
        let body = "{}";
        let sig = shannon_core::webhook::sign_signature(&secret, body.as_bytes());
        let (status, body_json) = post_trigger(router, "no-such-routine", body, Some(&sig)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body_json}");
    }

    #[tokio::test]
    async fn trigger_invalid_json_body_is_400() {
        let app = test_app();
        let tmp = tempfile::tempdir().unwrap();
        let secret = "s3cret".to_string();
        let ts = test_state(app, tmp.path(), Some(secret.clone()));
        let router = trigger_router(ts);
        let body = "{not json";
        let sig = shannon_core::webhook::sign_signature(&secret, body.as_bytes());
        let (status, _) = post_trigger(router, "nightly-scan", body, Some(&sig)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// The loopback server, built via the same path as production, must
    /// actually listen and answer `/api/health` on the loopback interface,
    /// and must serve the merged-in trigger route.
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
