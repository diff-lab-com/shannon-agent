//! §4.8 guard nodes and hook adapters mounted on the session bus.
//!
//! The tool pre-execute sequence is expressed as two typed Waterfall chains
//! ([`EventBus::guard_pipeline`](crate::bus::EventBus::guard_pipeline)):
//!
//! 1. **Permission gate** (`PIPELINE_PERMISSION`) — the shared
//!    [`PermissionManager`] verdict is the *first* node of the chain; every
//!    verdict is published to the bus as a durable `permission/decision`
//!    row (route (b) agreed in §4.9).
//! 2. **PreToolUse hooks** (`PIPELINE_HOOKS_PRE_TOOL_USE`) — configured
//!    hooks observe/mutate the tool input (the "`next()` chain") exactly
//!    like the previous inline `run_hooks` block, and each executed hook
//!    lands in the log as a durable `hook/fired` audit row.
//!
//! Fire-and-forget hook types (UserPromptSubmit, PostToolUse, …) stop being
//! direct method calls: their triggers ride the bus as routing-only
//! `custom` events under [`NS_HOOK_TRIGGER`](crate::bus::NS_HOOK_TRIGGER)
//! and a [`HookManagerAdapter`] subscription executes them through the
//! unchanged public [`HookManager::run_hooks`] API.

use std::sync::{Arc, RwLock};
use std::time::Instant;

use crate::bus::{EventBus, Flow, hook_fired_event, hook_trigger_event, permission_decision_event};
use serde_json::Value;
use shannon_engine::hooks::{HookDecision, HookEvent, HookEventType, HookManager, HookResult};
use shannon_engine::permissions::{PermissionManager, PermissionPrompt};
use shannon_types::session_event::{HookFiredPayload, SessionEventBody};

/// Pipeline key: stage 1 — permission gate (chain-head node).
pub const PIPELINE_PERMISSION: &str = "tool/guard/permission";
/// Pipeline key: stage 2 — PreToolUse hooks rewriting/vetoing input.
pub const PIPELINE_HOOKS_PRE_TOOL_USE: &str = "tool/guard/pre-tool-use-hooks";

// ============================================================================
// Typed Waterfall context
// ============================================================================

/// Context threaded through the tool pre-execute waterfall stages.
#[derive(Debug)]
pub struct ToolGuardContext {
    /// Tool name about to run.
    pub tool_name: String,
    /// Model-emitted input (possibly rewritten by PreToolUse hooks).
    pub input: Value,
    /// Verdict produced by the permission gate.
    pub verdict: PermissionVerdict,
    /// Deny reason from a PreToolUse hook, if any (`Hook denied: {reason}`).
    pub hook_deny: Option<String>,
}

impl ToolGuardContext {
    /// Context for one pending tool call.
    pub fn new(tool_name: impl Into<String>, input: Value) -> Self {
        Self {
            tool_name: tool_name.into(),
            input,
            verdict: PermissionVerdict::Pending,
            hook_deny: None,
        }
    }
}

/// What the permission gate decided for this call.
#[derive(Debug)]
pub enum PermissionVerdict {
    /// Not yet evaluated.
    Pending,
    /// Approved without user interaction.
    Allowed,
    /// Needs a user decision (the interactive flow lives in the query loop,
    /// unchanged); `elapsed_ms` measures classification time.
    Prompt {
        /// The rendered prompt awaiting the user's choice.
        prompt: Box<PermissionPrompt>,
        /// Classification latency, published with the decision.
        elapsed_ms: u128,
    },
    /// Refused before reaching the interactive layer.
    Denied {
        /// Message identical to the pre-bus tool-result text.
        reason: String,
        /// Classification latency, published with the decision.
        elapsed_ms: u128,
    },
}

// ============================================================================
// Node 1: the permission gate (chain-head node of the guard chain)
// ============================================================================

/// Guard-chain node wrapping `PermissionManager::classify_and_check`.
///
/// A faithful re-wrap: same manager, same synchronous call, same
/// deny / prompt / allow triad. What changed with the bus is that every
/// outcome *also* publishes a durable decision row carrying `{tool,
/// decision, mode}` plus latency folded into `reason`.
pub struct PermissionGateNode {
    permissions: Arc<RwLock<PermissionManager>>,
    session_id: uuid::Uuid,
    bus: EventBus,
}

impl PermissionGateNode {
    /// Node over the session's shared permission state.
    pub fn new(
        permissions: Arc<RwLock<PermissionManager>>,
        session_id: uuid::Uuid,
        bus: EventBus,
    ) -> Self {
        Self {
            permissions,
            session_id,
            bus,
        }
    }

    /// Evaluate the pending call and fill `ctx.verdict`. Always continues
    /// the chain: downstream stages inspect the verdict themselves, which
    /// keeps denial/prompt handling byte-identical to the pre-bus loop.
    pub async fn evaluate(&self, ctx: &mut ToolGuardContext) -> Flow {
        let started = Instant::now();
        let result = {
            let guard = shannon_types::recover_lock(self.permissions.read());
            guard.classify_and_check(self.session_id, &ctx.tool_name, &ctx.input)
        };
        let elapsed_ms = started.elapsed().as_millis();
        let mode = {
            let guard = shannon_types::recover_lock(self.permissions.read());
            guard.approval_mode().short_label().to_string()
        };
        let tool = ctx.tool_name.clone();
        match result {
            Err(err) => {
                // Same user-facing message the loop produced before the bus.
                ctx.verdict = PermissionVerdict::Denied {
                    reason: format!("Tool denied by classifier: {tool}"),
                    elapsed_ms,
                };
                emit_decision(
                    &self.bus,
                    &tool,
                    "deny",
                    Some(&err.to_string()),
                    &mode,
                    elapsed_ms,
                );
            }
            Ok(None) => {
                ctx.verdict = PermissionVerdict::Allowed;
                emit_decision(&self.bus, &tool, "allow", None, &mode, elapsed_ms);
            }
            Ok(Some(prompt)) => {
                if prompt.risk_level == shannon_engine::permissions::RiskLevel::Critical {
                    ctx.verdict = PermissionVerdict::Denied {
                        reason: format!("Tool denied: {}", prompt.description),
                        elapsed_ms,
                    };
                    emit_decision(
                        &self.bus,
                        &tool,
                        "deny",
                        Some(&format!("critical risk: {}", prompt.description)),
                        &mode,
                        elapsed_ms,
                    );
                } else {
                    let risk_reason = prompt.risk_reason.clone();
                    ctx.verdict = PermissionVerdict::Prompt {
                        prompt: Box::new(prompt),
                        elapsed_ms,
                    };
                    emit_decision(
                        &self.bus,
                        &tool,
                        "ask",
                        Some(&risk_reason),
                        &mode,
                        elapsed_ms,
                    );
                }
            }
        }
        Flow::Continue
    }
}

/// Publish one durable `permission/decision` row.
///
/// The frozen vocabulary payload has no dedicated latency field, so the
/// measurement rides inside `reason` ("… (12ms)") instead of inventing a
/// second schema. Also the sink for plugin-gate decisions via the installed
/// [`crate::bus`] broadcaster — both sources share this exact row shape.
pub fn emit_decision(
    bus: &EventBus,
    tool_name: &str,
    decision: &str,
    reason: Option<&str>,
    mode: &str,
    elapsed_ms: u128,
) {
    bus.dispatch(
        permission_decision_event(shannon_types::session_event::PermissionDecisionPayload {
            tool_name: Some(tool_name.to_string()),
            request: None,
            decision: decision.to_string(),
            reason: reason.map(|r| format!("{r} ({elapsed_ms}ms)")),
            mode: Some(mode.to_string()),
        })
        .into(),
        crate::bus::DispatchMode::Emit,
    );
}

// ============================================================================
// Node 2: PreToolUse hooks (input-rewriting waterfall stage)
// ============================================================================

/// Guard-chain node running configured PreToolUse hooks via the unchanged
/// [`HookManager`] API and mirroring executed hooks into the log as
/// `hook/fired` audit rows.
///
/// With no hooks configured (the default everywhere in CI) this node writes
/// nothing and changes nothing — byte-parity with the pre-bus loop.
pub struct PreToolUseHookNode {
    hooks: Arc<tokio::sync::RwLock<HookManager>>,
    bus: EventBus,
}

impl PreToolUseHookNode {
    /// Node over the session's hook manager + audit sink.
    pub fn new(hooks: Arc<tokio::sync::RwLock<HookManager>>, bus: EventBus) -> Self {
        Self { hooks, bus }
    }

    /// Run the hooks and record their verdicts into `ctx`. Continues the
    /// chain regardless: the deny travels in `ctx.hook_deny`, preserving
    /// the exact pre-bus message flow in the query loop.
    pub async fn evaluate(&self, ctx: &mut ToolGuardContext) -> Flow {
        let event = HookEvent::PreToolUse {
            tool_name: ctx.tool_name.clone(),
            input: ctx.input.clone(),
        };
        let started = Instant::now();
        let outcome = self.hooks.read().await.run_hooks(&event).await;
        let duration_ms = duration_since(started);
        match outcome {
            Ok(results) => {
                for result in &results {
                    self.audit_row(HookEventType::PreToolUse.to_string(), result, duration_ms);
                }
                match HookManager::resolve_results(&results) {
                    HookDecision::Deny { reason } => {
                        ctx.hook_deny = Some(reason);
                    }
                    HookDecision::Modify {
                        modified_input: Some(new_input),
                        ..
                    } => {
                        tracing::debug!(
                            "PreToolUse hook modified input for tool '{}'",
                            ctx.tool_name
                        );
                        ctx.input = new_input.clone();
                    }
                    _ => {}
                }
            }
            Err(e) => {
                // Same behavior as pre-bus: log and treat as Allow.
                tracing::warn!("PreToolUse hook error: {e}");
                self.bus.dispatch(
                    hook_fired_event(HookFiredPayload {
                        event: HookEventType::PreToolUse.to_string(),
                        hook: "<manager-error>".into(),
                        outcome: Some("error".into()),
                        duration_ms: Some(duration_ms),
                    })
                    .into(),
                    crate::bus::DispatchMode::Emit,
                );
            }
        }
        Flow::Continue
    }

    fn audit_row(&self, event_name: String, result: &HookResult, duration_ms: u64) {
        let outcome = outcome_label(result);
        self.bus.dispatch(
            hook_fired_event(HookFiredPayload {
                event: event_name,
                hook: result.command.chars().take(120).collect(),
                outcome: Some(outcome.into()),
                duration_ms: Some(duration_ms),
            })
            .into(),
            crate::bus::DispatchMode::Emit,
        );
    }
}

fn outcome_label(result: &HookResult) -> &'static str {
    match &result.decision {
        HookDecision::Deny { .. } => "denied",
        HookDecision::Modify { .. } => "modified",
        HookDecision::Allow => {
            if result.exit_code == 0 {
                "ok"
            } else {
                "error"
            }
        }
    }
}

fn duration_since(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

// ============================================================================
// Fire-and-forget triggers + adapter subscription
// ============================================================================

/// Publish a fire-and-forget hook trigger onto the bus (a routing-only row;
/// see [`crate::bus::NS_HOOK_TRIGGER`]). Skips payload construction entirely
/// when no subscription exists for `custom` events.
pub fn publish_hook_trigger(bus: &EventBus, trigger_type: &str, data: Value) {
    use shannon_types::session_event::SessionEventKind;
    if !bus.has_kind_subscription(SessionEventKind::Custom) {
        return;
    }
    bus.dispatch(
        hook_trigger_event(trigger_type, data).into(),
        crate::bus::DispatchMode::Emit,
    );
}

/// Subscription-side executor: decodes hook triggers back into [`HookEvent`]
/// values and runs them through [`HookManager::run_hooks`] (public API
/// unchanged), then writes one `hook/fired` audit row per executed hook.
#[derive(Clone)]
pub struct HookManagerAdapter {
    hooks: Arc<tokio::sync::RwLock<HookManager>>,
    bus: EventBus,
}

impl HookManagerAdapter {
    /// Adapter over one session's hook manager.
    pub fn new(hooks: Arc<tokio::sync::RwLock<HookManager>>, bus: EventBus) -> Self {
        Self { hooks, bus }
    }

    /// Execute one decoded trigger. No-op (zero log impact) when no hooks
    /// are configured — parity with the direct-call era.
    pub async fn run_decoded(&self, trigger_type: &str, payload: &Value) {
        let Some(event) = decode_trigger(trigger_type, payload) else {
            tracing::debug!(trigger = %trigger_type, "no trigger decoder; skipping");
            return;
        };
        let started = Instant::now();
        let outcome = self.hooks.read().await.run_hooks(&event).await;
        let duration_ms = duration_since(started);
        match outcome {
            Ok(results) => {
                if results.is_empty() {
                    return; // no hooks configured: zero durable impact (parity)
                }
                for result in &results {
                    self.dispatch_audit_row(trigger_type, result, duration_ms);
                }
            }
            Err(e) => {
                tracing::warn!(event = %trigger_type, "hook execution failed: {e}");
            }
        }
    }

    fn dispatch_audit_row(&self, event_name: &str, result: &HookResult, duration_ms: u64) {
        self.bus.dispatch(
            hook_fired_event(HookFiredPayload {
                event: event_name.to_string(),
                hook: result.command.chars().take(120).collect(),
                outcome: Some(outcome_label(result).into()),
                duration_ms: Some(duration_ms),
            })
            .into(),
            crate::bus::DispatchMode::Emit,
        );
    }
}

impl crate::bus::BusSubscriber for HookManagerAdapter {
    fn on_input(&self, input: &BusInput) {
        let BusInput::Event(event) = input else {
            return;
        };
        let SessionEventBody::Custom(payload) = &event.body else {
            return;
        };
        if payload.namespace != NS_HUB_NAMESPACE {
            return;
        }
        let Some(trigger_type) = payload.data.get("type").and_then(Value::as_str) else {
            return;
        };
        let inner = payload.data.get("payload").cloned().unwrap_or(Value::Null);
        if tokio::runtime::Handle::try_current().is_ok() {
            let this = self.clone();
            let trigger_type = trigger_type.to_string();
            tokio::spawn(async move {
                this.run_decoded(&trigger_type, &inner).await;
            });
        } else {
            // Subscriber callbacks are synchronous; without a runtime there
            // is no safe way to drive the async hook manager. Advisory
            // triggers are dropped with a trace rather than blocking.
            tracing::debug!(
                trigger = %trigger_type,
                "hook trigger delivered without runtime context; dropped"
            );
        }
    }
}

use crate::bus::BusInput;

/// Namespace the adapter listens on ([`crate::bus::NS_HOOK_TRIGGER`] minus
/// its internal prefix — routing-only rows live under that prefix).
const NS_HUB_NAMESPACE: &str = crate::bus::NS_HOOK_TRIGGER;

/// Rebuild a typed [`HookEvent`] from a hub payload. Types without engine
/// producers yet decode to `None` until their producer lands.
fn decode_trigger(trigger_type: &str, payload: &Value) -> Option<HookEvent> {
    match trigger_type {
        "UserPromptSubmit" => Some(HookEvent::UserPromptSubmit {
            prompt: payload.get("prompt")?.as_str()?.to_string(),
        }),
        "PostToolUse" => Some(HookEvent::PostToolUse {
            tool_name: payload.get("tool_name")?.as_str()?.to_string(),
            input: payload.get("input").cloned()?,
            output: payload.get("output").cloned()?,
        }),
        "PreToolUse" => Some(HookEvent::PreToolUse {
            tool_name: payload.get("tool_name")?.as_str()?.to_string(),
            input: payload.get("input").cloned()?,
        }),
        _ => None,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::bus::{DispatchMode, TopicFilter};
    use crate::session_log::{L0TeeSubscriber, TeeHandle, session_events_path};

    fn empty_manager() -> Arc<tokio::sync::RwLock<HookManager>> {
        Arc::new(tokio::sync::RwLock::new(HookManager::with_paths(
            std::env::temp_dir().join("hooks-none-user.json"),
            std::env::temp_dir().join("hooks-none-project.json"),
        )))
    }

    #[test]
    fn emit_decision_writes_vocabulary_row_with_latency_in_reason() {
        let dir = tempfile::TempDir::new().unwrap();
        let tee = TeeHandle::open_in_dir(dir.path(), "sess-guard", "m", Some("anthropic"));
        let bus = EventBus::new();
        let keep = bus.subscribe(
            TopicFilter::all(),
            Arc::new(L0TeeSubscriber::new(tee.clone())),
        );
        tee.record_user_message("u");
        emit_decision(&bus, "Bash", "allow", Some("always-allowed"), "AUTO", 12);
        drop(keep);
        drop(tee);

        let text = std::fs::read_to_string(session_events_path(dir.path(), "sess-guard"))
            .expect("log flushed on close");
        assert_eq!(text.matches("\"kind\":\"permission/decision\"").count(), 1);
        assert!(text.contains("\"decision\":\"allow\""));
        assert!(text.contains("\"tool_name\":\"Bash\""));
        assert!(text.contains("\"mode\":\"AUTO\""));
        assert!(
            text.contains("always-allowed (12ms)"),
            "latency folded into reason, got: {text}"
        );
    }

    #[tokio::test]
    async fn pre_tool_use_node_with_no_hooks_is_a_noop_pass_through() {
        let bus = EventBus::new();
        let node = PreToolUseHookNode::new(empty_manager(), bus.shared());
        let mut ctx = ToolGuardContext::new("echo", serde_json::json!({"label": 1}));
        let flow = node.evaluate(&mut ctx).await;
        assert_eq!(flow, Flow::Continue);
        assert!(ctx.hook_deny.is_none(), "no hooks → no deny");
        assert_eq!(ctx.input["label"], 1, "no hooks → untouched input");
        assert!(matches!(ctx.verdict, PermissionVerdict::Pending));
    }

    #[tokio::test]
    async fn adapter_runs_decoded_triggers_without_hooks_configured() {
        let bus = EventBus::new();
        let adapter = HookManagerAdapter::new(empty_manager(), bus.shared());
        adapter
            .run_decoded(
                "PostToolUse",
                &serde_json::json!({"tool_name": "echo", "input": {}, "output": "ok"}),
            )
            .await;
        adapter
            .run_decoded("TotallyUnknownFutureType", &serde_json::json!({}))
            .await;
    }

    #[test]
    fn all_thirty_hook_event_names_route_through_the_custom_kind_subscription() {
        // Capability proof for plan §4.8 item 3: every one of the 30
        // HookEventType wire names is addressable as a subtopic under the
        // single Custom-kind subscription the adapter uses.
        const ALL_30: [&str; 30] = [
            "PreToolUse",
            "PostToolUse",
            "SessionStart",
            "SessionEnd",
            "Notification",
            "UserPromptSubmit",
            "TeamTaskCreated",
            "TeamTaskCompleted",
            "TeammateIdle",
            "PreCompact",
            "SubagentStart",
            "SubagentStop",
            "PermissionDenied",
            "Stop",
            "PostToolUseFailure",
            "PostCompact",
            "StopFailure",
            "FileChanged",
            "CwdChanged",
            "PermissionRequest",
            "UserPromptExpansion",
            "PostToolBatch",
            "ConfigChange",
            "InstructionsLoaded",
            "WorktreeCreate",
            "WorktreeRemove",
            "Elicitation",
            "ElicitationResult",
            "TaskCreated",
            "TaskCompleted",
        ];
        let bus = EventBus::new();
        let received = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let recv = received.clone();
        let _adapter_guard = bus.subscribe_fn(
            TopicFilter::kind(shannon_types::session_event::SessionEventKind::Custom),
            move |input| {
                if let BusInput::Event(event) = input {
                    if let Some(subtopic) = &event.subtopic {
                        recv.lock().unwrap().push(subtopic.to_string());
                    }
                }
            },
        );
        for name in ALL_30 {
            publish_hook_trigger(&bus, name, serde_json::json!({}));
        }
        let mut seen = received.lock().unwrap().clone();
        let mut expected = ALL_30.map(String::from).to_vec();
        seen.sort();
        expected.sort();
        assert_eq!(seen.len(), ALL_30.len(), "every distinct type routed once");
        assert_eq!(seen, expected);
    }
}
