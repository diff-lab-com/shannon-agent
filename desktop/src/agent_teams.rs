//! B2 — desktop bridge for real sub-agent execution ("agent teams").
//!
//! The engine's `agent_spawn` tool is a placeholder unless an
//! `AgentToolContext` (alias of `shannon_agents::TeamContext`) is injected
//! into the handle that `register_default_tools_with_providers` returned at
//! startup. This module owns that lifecycle:
//!
//! - [`enable`] builds the context (bypassing the `SHANNON_AGENT_TEAMS`
//!   env gate — the desktop Settings toggle is the gate, and a GUI process
//!   must not mutate its own environment after threads have spawned),
//!   wraps it with the shared LLM executor, registers a lifecycle observer
//!   that forwards `Spawned`/`Completed` transitions to the frontend as
//!   `subagent:start` / `subagent:stop`, and injects the context into the
//!   AppState handle the tool consults on every call.
//! - [`disable`] revokes the context — in-flight sub-agent runs finish
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

/// Map a registry lifecycle transition to its wire payload.
pub(crate) fn lifecycle_payload(event: &SubAgentLifecycle) -> SubAgentEventPayload {
    match event {
        SubAgentLifecycle::Spawned {
            agent_id,
            agent_name,
            team,
        } => SubAgentEventPayload {
            agent_id: agent_id.clone(),
            agent_name: agent_name.clone(),
            team: team.clone(),
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
            ok: Some(*ok),
            result_summary: Some(result_summary.clone()),
        },
    }
}

/// True when the persisted Settings toggle turns agent teams on.
pub(crate) async fn config_enabled(state: &crate::commands::AppState) -> bool {
    state.desktop_config.read().await.agent_teams_enabled
}

/// Build the context and inject it into the tool handle shared with
/// `AgentTool`. Idempotent: enabling twice keeps the first context (its
/// registry already carries this session's spawned agents).
#[cfg(feature = "tauri")]
pub(crate) async fn enable(
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
            let payload = lifecycle_payload(&event);
            if let Err(e) = tauri::Emitter::emit(&handle, name, payload) {
                tracing::warn!("subagent event emit failed: {e}");
            }
        }));

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
        let payload = lifecycle_payload(&event);
        assert_eq!(payload.agent_id, "agent_abc");
        assert_eq!(payload.team.as_deref(), Some("q3-roadmap"));
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
        let payload = lifecycle_payload(&event);
        assert_eq!(payload.ok, Some(false));
        assert_eq!(payload.result_summary.as_deref(), Some("rate limited"));
        assert_eq!(payload.team, None);
    }

    #[test]
    fn payload_serializes_camel_case_and_skips_stop_only_fields() {
        let start = lifecycle_payload(&SubAgentLifecycle::Spawned {
            agent_id: "a".into(),
            agent_name: "n".into(),
            team: None,
        });
        let json = serde_json::to_value(&start).unwrap();
        assert!(json.get("agentId").is_some());
        assert!(json.get("agentName").is_some());
        assert!(json.get("ok").is_none());
        assert!(json.get("resultSummary").is_none());

        let stop = lifecycle_payload(&SubAgentLifecycle::Completed {
            agent_id: "a".into(),
            agent_name: "n".into(),
            ok: true,
            result_summary: "done".into(),
        });
        let json = serde_json::to_value(&stop).unwrap();
        assert_eq!(json["ok"], serde_json::json!(true));
        assert_eq!(json["resultSummary"], serde_json::json!("done"));
    }
}
