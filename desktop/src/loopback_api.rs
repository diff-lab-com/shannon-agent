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
//! (`crate::inbox_commands::spawn_routine_run`) and records the run in the
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

/// Resolve the execution-world providers for the loopback engine's registry
/// from the persisted desktop config — the same P1-3 assembly seam as
/// `AppState::new` (interactive sends) and the goal runner (unattended runs).
/// `Ok(None)` = register `base` unchanged; `Err` = invalid mode (the caller
/// must degrade loudly to `base`, never silently pretend to restrict).
/// Named so the construction point's sandbox behaviour is directly
/// assertable (see the tests below).
fn loopback_sandbox_providers(
    desktop_config: &DesktopConfig,
    base: &shannon_tools::ToolProviders,
) -> Result<Option<shannon_tools::ToolProviders>, String> {
    crate::sandbox_assembly::effective_sandbox_providers(
        desktop_config
            .sandbox
            .as_ref()
            .and_then(|s| s.mode.as_deref()),
        desktop_config.working_dir.as_deref(),
        base,
    )
}

/// Build the loopback engine API server from an LLM client config plus a
/// freshly-registered default tool set. Pure construction — does not bind.
///
/// The tool set honours the persisted `sandbox.mode` config: IM-channel
/// (T9) and mobile-dispatch (T14) turns execute through this registry via
/// the gateway, so the execution-mode switcher must hold on this path too —
/// not only on the interactive (`AppState::new`) and goal-runner seams.
///
/// When `state` carries an injected agent-teams context (B2, see
/// `crate::agent_teams::enable`), the loopback `AgentTool` is re-pointed at
/// the chat session's context handle and `team_task_*` tools are registered
/// — so an IM/mobile turn that calls `agent_spawn` / `team_task_*` lands on
/// the same coordinator the interactive chat uses.
///
/// The handle (not a snapshot of its value) is shared: the `AgentTool`
/// consults it on every call, so enabling agent teams AFTER the loopback
/// server has started takes effect on the next loopback turn, same as it
/// does for chat. Lifecycle events flow too — the `subagent:start|stop`
/// observer lives on the shared registry, so sub-agents spawned through a
/// loopback turn surface in the desktop UI with no extra wiring here.
pub fn build_server(
    client_config: LlmClientConfig,
    desktop_config: &DesktopConfig,
    state: &crate::commands::AppState,
) -> ShannonApiServer {
    let tools = build_loopback_tools(desktop_config, state);
    ShannonApiServer::new(client_config)
        .with_tools(tools)
        .host(LOOPBACK_HOST)
        .port(LOOPBACK_PORT)
}

/// Build the loopback tool registry: sandboxed default tools + the B2-4
/// team-state wiring. Split from [`build_server`] so tests can inspect the
/// registry (`ShannonApiServer` keeps its tools private).
fn build_loopback_tools(
    desktop_config: &DesktopConfig,
    state: &crate::commands::AppState,
) -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    let assembly = shannon_remote::assembly::assemble_dynamic();
    let sandboxed_providers = match loopback_sandbox_providers(desktop_config, &assembly.providers)
    {
        Ok(providers) => providers,
        Err(e) => {
            tracing::error!(
                "loopback engine API server: sandbox disabled, continuing unrestricted: {e}"
            );
            None
        }
    };
    if let Err(e) = register_default_tools_with_providers(
        &mut tools,
        sandboxed_providers.as_ref().unwrap_or(&assembly.providers),
    ) {
        tracing::warn!("loopback engine API server: default tool registration failed: {e}");
    }
    // B2-4 — share the chat session's team state with the loopback
    // registry: swap the fresh `AgentTool`'s empty context handle for the
    // AppState handle, then register `team_task_*` bound to the same
    // coordinator when teams are on. Swapping the handle (vs. snapshotting
    // the coordinator at build time, the PR #93 shape this replaces) means
    // a later `agent_teams::enable` is picked up by the NEXT loopback turn's
    // `agent_spawn` — the tool consults the handle on every call.
    // Known asymmetry: `team_task_*` registration itself is build-time (the
    // coordinator value must exist to bind the tools), so toggling teams on
    // mid-session makes loopback `agent_spawn` live immediately but its
    // `team_task_*` tools only after an app restart. Documented in the
    // surfaces audit.
    let agent_ctx = state.agent_tool_context.clone();
    if !shannon_tools::swap_agent_tool_context(&mut tools, agent_ctx.clone()) {
        tracing::warn!(
            "loopback engine API server: Agent tool swap failed — agent_spawn stays placeholder"
        );
    }
    if let Err(e) = shannon_tools::register_team_tools_when_enabled(&mut tools, &agent_ctx) {
        tracing::warn!("loopback engine API server: team_task tool registration failed: {e}");
    }
    tools
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
    let desktop_config = state.desktop_config.read().await.clone();
    let secret =
        crate::commands_notifications::load_desktop_webhook_config().and_then(|c| c.secret);
    let trigger_enabled = secret.is_some();
    let trigger = trigger_router(TriggerState::from_state(state, app, secret));
    let server = build_server(client_config, &desktop_config, state).with_extra_routes(trigger);
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

    /// Desktop config carrying only the sandbox-relevant fields (P1-3).
    fn sandbox_cfg(mode: Option<&str>) -> DesktopConfig {
        DesktopConfig {
            working_dir: Some("/tmp".to_string()),
            sandbox: mode.map(|m| crate::config::SandboxConfig {
                mode: Some(m.to_string()),
            }),
            ..DesktopConfig::default()
        }
    }

    /// The loopback construction point must apply the persisted
    /// `sandbox.mode` exactly like the interactive (`AppState::new`) and
    /// goal-runner seams: IM-channel (T9) and mobile-dispatch (T14) turns
    /// execute through this registry via the gateway. Assertion style
    /// mirrors `sandbox_assembly.rs` (decorated set over the dynamic world).
    #[test]
    fn loopback_build_applies_persisted_sandbox_mode() {
        let base = shannon_remote::assembly::assemble_dynamic().providers;

        // Unset / `off` → the plain dynamic assembly, no decoration.
        assert!(
            loopback_sandbox_providers(&DesktopConfig::default(), &base)
                .unwrap()
                .is_none()
        );
        assert!(
            loopback_sandbox_providers(&sandbox_cfg(Some("off")), &base)
                .unwrap()
                .is_none()
        );

        // `local` → a decorated provider set: fresh fs/process wrappers over
        // the dynamic world (not the base Arcs), world-sandbox handle
        // preserved.
        let decorated = loopback_sandbox_providers(&sandbox_cfg(Some("local")), &base)
            .unwrap()
            .expect("local mode must yield providers");
        assert!(
            !std::sync::Arc::ptr_eq(&decorated.fs, &base.fs),
            "fs tools must be policy-wrapped"
        );
        assert!(
            !std::sync::Arc::ptr_eq(&decorated.process, &base.process),
            "process tools must be policy-wrapped"
        );
        assert!(decorated.world_sandbox.is_some(), "world_sandbox preserved");

        // Unknown mode → loud error (build_server degrades to base + logs;
        // never a silent fake sandbox).
        let err = loopback_sandbox_providers(&sandbox_cfg(Some("banana")), &base)
            .err()
            .expect("unknown mode must be an error");
        assert!(err.contains("unknown sandbox.mode"), "{err}");
    }

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
        // Build a throwaway `AppState` so we can pass the `&state` arg that
        // `build_server` now takes (B2 follow-up — picks up the chat's
        // coordinator for `team_task_*` registration when agent teams is
        // enabled). The test only exercises the health endpoint, so a
        // default-constructed state is sufficient.
        let state = crate::commands::AppState::new();
        let server = build_server(
            LlmClientConfig::default(),
            &DesktopConfig::default(),
            &state,
        )
        .port(port);
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

    /// B2-4 — the loopback registry shares the chat session's team state:
    /// the `Agent` tool is re-pointed at `state.agent_tool_context` (handle,
    /// not a snapshot), and `team_task_*` tools are gated on the handle being
    /// populated at build time.
    ///
    /// The later-injection property is asserted end-to-end: a registry built
    /// while teams were OFF executes `SendMessage` as the placeholder (Ok)
    /// before the toggle, and as the coordinator-backed path (Err — agent
    /// not found) after the context is injected into the shared handle. The
    /// accepted asymmetry — `team_task_*` registration stays build-time — is
    /// documented in the surfaces audit.
    #[test]
    fn loopback_build_wires_chat_team_state_and_later_injection_is_live() {
        let state = crate::commands::AppState::new();

        // Teams off (default): swap succeeded (Agent present), no team_task.
        let tools = build_loopback_tools(&DesktopConfig::default(), &state);
        assert!(
            tools.get("Agent").is_some(),
            "Agent tool must be registered"
        );
        assert!(
            tools.get("team_task_create").is_none(),
            "empty handle must not register team_task tools"
        );

        // Placeholder path: SendMessage to a nonexistent agent is Ok.
        let placeholder = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tools.execute(
                "Agent",
                serde_json::json!({
                    "operation": "SendMessage",
                    "agent_id": "nobody",
                    "message": "ping"
                }),
            ))
            .expect("placeholder SendMessage must not error");
        assert!(!placeholder.is_error);
        assert!(
            placeholder.content.contains("delivered"),
            "{}",
            placeholder.content
        );

        // Simulate the user toggling teams ON after the server started:
        // inject a context into the SAME handle the loopback registry shares.
        let ctx = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(shannon_agents::TeamContext::new_unchecked(
                shannon_engine::api::LlmClientConfig::default(),
            ))
            .expect("TeamContext::new_unchecked");
        *state.agent_tool_context.lock().expect("handle poisoned") = Some(ctx);

        // Same registry, same call: now the coordinator-backed path runs and
        // rejects the unknown agent — later injection is live.
        let live = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tools.execute(
                "Agent",
                serde_json::json!({
                    "operation": "SendMessage",
                    "agent_id": "nobody",
                    "message": "ping"
                }),
            ));
        let err = live.expect_err("post-injection SendMessage must hit the coordinator path");
        assert!(
            err.to_string().contains("Failed to send message"),
            "unexpected error: {err}"
        );

        // A registry built AFTER the toggle registers the team_task trio.
        let tools_after = build_loopback_tools(&DesktopConfig::default(), &state);
        assert!(tools_after.get("team_task_create").is_some());
        assert!(tools_after.get("team_task_update").is_some());
        assert!(tools_after.get("team_task_list").is_some());
    }
}
