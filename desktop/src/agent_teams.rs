//! B2 — desktop bridge for real sub-agent execution ("agent teams").
//!
//! The engine's `agent_spawn` tool is a placeholder unless an
//! `AgentToolContext` (alias of `shannon_agents::TeamContext`) is injected
//! into the handle that `register_default_tools_with_providers` returned at
//! startup. This module owns that lifecycle:
//!
//! - `enable` builds the context (bypassing the `SHANNON_AGENT_TEAMS`
//!   env gate — the desktop Settings toggle is the gate, and a GUI process
//!   must not mutate its own environment after threads have spawned),
//!   wraps it with the shared LLM executor, registers a lifecycle observer
//!   that forwards `Spawned`/`Completed` transitions to the frontend as
//!   `subagent:start` / `subagent:stop`, and injects the context into the
//!   AppState handle the tool consults on every call.
//! - `disable` revokes the context — in-flight sub-agent runs finish
//!   (the engine cloned what it needed), new `agent_spawn` calls fall back
//!   to the placeholder output.
//!
//! Real execution runs in-process via a child QueryEngine (see
//! `AgentTool::execute_subagent` in shannon-tools) — no extra binary is
//! spawned, but every sub-agent run is a real LLM conversation that
//! consumes API quota. This is why the toggle defaults to off.

use std::sync::Arc;

use serde::Serialize;
use shannon_agents::{SubAgentLifecycle, TeamContext, shared_executor};
use shannon_engine::api::LlmClient;

/// Wire event fired when the registry accepts a new sub-agent.
pub const SUBAGENT_START: &str = "subagent:start";
/// Wire event fired when a sub-agent run finishes (ok or failed).
pub const SUBAGENT_STOP: &str = "subagent:stop";

/// Payload for `subagent:start` / `subagent:stop`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentEventPayload {
    pub agent_id: String,
    pub agent_name: String,
    /// Team the sub-agent was spawned into (engine default: `_global`).
    pub team: Option<String>,
    /// W10 audit §6-D — the Shannon session the spawning run belongs to, so
    /// a session window can drop banners of OTHER sessions' sub-agents
    /// (the same per-window filter every `session_id`-stamped event gets).
    /// Attribution is `sole_active_run_session`: the unambiguous single live
    /// run; zero or several concurrent runs → `None`, which is omitted from
    /// the wire and makes the frontend filter degrade to the pre-fix
    /// every-window banner instead of dropping the event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Only set on `subagent:stop`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    /// Only set on `subagent:stop` — run result, or the failure reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
}

/// Map a registry lifecycle transition to its wire event name.
pub(crate) fn lifecycle_event_name(event: &SubAgentLifecycle) -> &'static str {
    match event {
        SubAgentLifecycle::Spawned { .. } => SUBAGENT_START,
        SubAgentLifecycle::Completed { .. } => SUBAGENT_STOP,
    }
}

/// Map a registry lifecycle transition to its wire payload. `session_id` is
/// the caller's session attribution (see [`SubAgentEventPayload::session_id`])
/// — the lifecycle event itself carries no session identity.
pub(crate) fn lifecycle_payload(
    event: &SubAgentLifecycle,
    session_id: Option<String>,
) -> SubAgentEventPayload {
    match event {
        SubAgentLifecycle::Spawned {
            agent_id,
            agent_name,
            team,
        } => SubAgentEventPayload {
            agent_id: agent_id.clone(),
            agent_name: agent_name.clone(),
            team: team.clone(),
            session_id,
            ok: None,
            result_summary: None,
        },
        SubAgentLifecycle::Completed {
            agent_id,
            agent_name,
            ok,
            result_summary,
        } => SubAgentEventPayload {
            agent_id: agent_id.clone(),
            agent_name: agent_name.clone(),
            team: None,
            session_id,
            ok: Some(*ok),
            result_summary: Some(result_summary.clone()),
        },
    }
}

/// W10 audit §6-D — best-effort session attribution for the emit site: the
/// SOLE live run's session (the same scoping contract as the ask_user
/// card), or `None` when zero or several runs are live. Fail-open: an
/// unattributable payload keeps the pre-§6-D every-window banner.
#[cfg(feature = "tauri")]
fn attributed_session<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<String> {
    use tauri::Manager;
    let state = app.try_state::<crate::commands::AppState>()?;
    state.sole_active_run_session()
}

/// True when the persisted Settings toggle turns agent teams on.
/// `pub` because the bin crate's `main.rs` startup calls it.
pub async fn config_enabled(state: &crate::commands::AppState) -> bool {
    state.desktop_config.read().await.agent_teams_enabled
}

/// Build the context and inject it into the tool handle shared with
/// `AgentTool`. Idempotent: enabling twice keeps the first context (its
/// registry already carries this session's spawned agents).
#[cfg(feature = "tauri")]
pub async fn enable(
    state: &crate::commands::AppState,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    {
        let existing = state
            .agent_tool_context
            .lock()
            .expect("agent tool context lock poisoned");
        if existing.is_some() {
            return Ok(());
        }
    }

    let client_config = state.client_config.read().await.clone();
    let ctx = TeamContext::new_unchecked(client_config)
        .await
        .map_err(|e| format!("Agent teams init failed: {e}"))?;

    // P1 — inherit the lead's approval policy so sub-agents don't silently
    // escalate to `FullAuto` when the parent is in `Suggest` / `Plan` /
    // `Readonly`. The previous shape set `permission_mode = "default"`
    // unconditionally, which the in-process sub-agent path ignored — but
    // pinning the field here keeps it visible to future consumers (TUI
    // observers, debug dumps) and is the contract the engine reads from.
    let parent_permission_mode = state
        .desktop_config
        .read()
        .await
        .approval_mode
        .clone()
        .unwrap_or_else(|| "default".to_string());
    // Validate the same way the chat lead does — unknown values silently
    // land on `Suggest`, which is a sane default. We do NOT abort enable()
    // on an unrecognised value because the user can fix it from Settings
    // and the worst case is a stricter mode.
    let _ = crate::commands::parse_approval_mode(&parent_permission_mode);
    let ctx = ctx.with_permission_mode(parent_permission_mode);

    // Mirror the TUI injection: a shared executor lets teammates make real
    // LLM calls through the coordinator.
    let ctx = {
        let llm_client = LlmClient::new(ctx.client_config.clone());
        ctx.with_executor(shared_executor(llm_client))
    };

    let handle = app_handle.clone();
    ctx.registry
        .register_observer(Arc::new(move |event: SubAgentLifecycle| {
            let name = lifecycle_event_name(&event);
            // W10 audit §6-D: stamp the spawning run's session so session
            // windows can filter foreign banners (None = unattributable →
            // the frontend degrades to every-window).
            let payload = lifecycle_payload(&event, attributed_session(&handle));
            if let Err(e) = tauri::Emitter::emit(&handle, name, payload) {
                tracing::warn!("subagent event emit failed: {e}");
            }
        }));

    // Wire team_task_* tools (team_task_create/update/list) into the
    // shared `AppState::tools` registry so the lead LLM can manage the
    // shared `TaskBoard`. The registry uses interior mutability, so we
    // pass `&Arc<ToolRegistry>` directly. Failures are non-fatal — the
    // user gets agent_spawn without task-board tools, which is still
    // useful; logging makes the gap visible.
    let coordinator = ctx.coordinator.clone();
    let tools_arc = state.tools.clone();
    if let Err(e) = shannon_tools::register_team_tools_arc(&tools_arc, coordinator) {
        tracing::warn!("team_task tools registration failed (agent_spawn still works): {e}");
    } else {
        tracing::info!("team_task_create/update/list tools registered");
    }

    *state
        .agent_tool_context
        .lock()
        .expect("agent tool context lock poisoned") = Some(ctx);
    tracing::info!("agent teams enabled — agent_spawn now executes real sub-agents");
    Ok(())
}

/// Revoke the context. In-flight runs finish; new `agent_spawn` calls fall
/// back to the placeholder output.
pub(crate) fn disable(state: &crate::commands::AppState) {
    *state
        .agent_tool_context
        .lock()
        .expect("agent tool context lock poisoned") = None;
    tracing::info!("agent teams disabled — agent_spawn is a placeholder again");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawned_maps_to_start_event() {
        let event = SubAgentLifecycle::Spawned {
            agent_id: "agent_abc".into(),
            agent_name: "research-analyst-1234abcd".into(),
            team: Some("q3-roadmap".into()),
        };
        assert_eq!(lifecycle_event_name(&event), SUBAGENT_START);
        let payload = lifecycle_payload(&event, Some("sess-1".into()));
        assert_eq!(payload.agent_id, "agent_abc");
        assert_eq!(payload.team.as_deref(), Some("q3-roadmap"));
        // W10 audit §6-D: the caller's session attribution rides the payload.
        assert_eq!(payload.session_id.as_deref(), Some("sess-1"));
        assert_eq!(payload.ok, None);
        assert_eq!(payload.result_summary, None);
    }

    #[test]
    fn completed_maps_to_stop_event_with_outcome() {
        let event = SubAgentLifecycle::Completed {
            agent_id: "agent_abc".into(),
            agent_name: "research-analyst-1234abcd".into(),
            ok: false,
            result_summary: "rate limited".into(),
        };
        assert_eq!(lifecycle_event_name(&event), SUBAGENT_STOP);
        let payload = lifecycle_payload(&event, Some("sess-1".into()));
        assert_eq!(payload.ok, Some(false));
        assert_eq!(payload.result_summary.as_deref(), Some("rate limited"));
        assert_eq!(payload.team, None);
        assert_eq!(payload.session_id.as_deref(), Some("sess-1"));
    }

    #[test]
    fn payload_serializes_camel_case_and_skips_stop_only_fields() {
        let start = lifecycle_payload(
            &SubAgentLifecycle::Spawned {
                agent_id: "a".into(),
                agent_name: "n".into(),
                team: None,
            },
            Some("sess-1".into()),
        );
        let json = serde_json::to_value(&start).unwrap();
        assert!(json.get("agentId").is_some());
        assert!(json.get("agentName").is_some());
        assert_eq!(json["sessionId"], serde_json::json!("sess-1"));
        assert!(json.get("ok").is_none());
        assert!(json.get("resultSummary").is_none());

        let stop = lifecycle_payload(
            &SubAgentLifecycle::Completed {
                agent_id: "a".into(),
                agent_name: "n".into(),
                ok: true,
                result_summary: "done".into(),
            },
            None,
        );
        let json = serde_json::to_value(&stop).unwrap();
        assert_eq!(json["ok"], serde_json::json!(true));
        assert_eq!(json["resultSummary"], serde_json::json!("done"));
        // Unattributable (0 or ≥2 live runs) → the field is omitted, which
        // is the legacy shape the frontend filter degrades on.
        assert!(json.get("sessionId").is_none());
    }

    // === W10 audit §6-D — session attribution at the emit site ============

    #[cfg(feature = "tauri")]
    mod attribution {
        use super::*;
        use crate::commands::{AppState, SessionMeta};
        use tauri::Manager;

        fn manage_app() -> tauri::App<tauri::test::MockRuntime> {
            let app = tauri::test::mock_app();
            app.manage(AppState::new());
            app
        }

        /// The display-list entry `sole_active_run_session` requires for a
        /// rail-reachable session (C1 contract).
        fn rail_session(state: &AppState, id: &str) {
            state.sessions.try_lock().unwrap().push(SessionMeta {
                id: id.to_string(),
                title: "S".into(),
                created_at: 0,
                message_count: 0,
                working_dir: None,
                parent_id: None,
                branch_point: None,
            });
        }

        #[test]
        fn attributes_the_sole_live_run_session() {
            let app = manage_app();
            let handle = app.handle().clone();
            let state = handle.state::<AppState>();
            let session_id = "aaaa1111-0000-4000-8000-00000000000a";
            rail_session(&state, session_id);
            let _guard = crate::commands::ActiveSessionRunGuard::register(
                &state.active_run_sessions,
                session_id.to_string(),
            );

            assert_eq!(attributed_session(&handle).as_deref(), Some(session_id));
        }

        #[test]
        fn none_when_zero_or_several_runs_are_live() {
            let app = manage_app();
            let handle = app.handle().clone();
            let state = handle.state::<AppState>();
            let a = "aaaa1111-0000-4000-8000-00000000000a";
            let b = "bbbb2222-0000-4000-8000-00000000000b";
            rail_session(&state, a);
            rail_session(&state, b);

            // Zero live runs → unattributable.
            assert_eq!(attributed_session(&handle), None);

            // Two concurrent live runs → ambiguous → fail-open None (the
            // frontend keeps the pre-§6-D every-window banner).
            let _ga = crate::commands::ActiveSessionRunGuard::register(
                &state.active_run_sessions,
                a.to_string(),
            );
            let _gb = crate::commands::ActiveSessionRunGuard::register(
                &state.active_run_sessions,
                b.to_string(),
            );
            assert_eq!(attributed_session(&handle), None);
        }

        #[test]
        fn none_when_the_run_session_is_not_in_the_rail() {
            let app = manage_app();
            let handle = app.handle().clone();
            let state = handle.state::<AppState>();
            // Live run on a session no window rail can show (background
            // batch/goal runs register their hidden sessions too).
            let _guard = crate::commands::ActiveSessionRunGuard::register(
                &state.active_run_sessions,
                "ghost-0000-0000-0000-000000000000".to_string(),
            );
            assert_eq!(attributed_session(&handle), None);
        }
    }
}
