//! The L0 writer as a built-in bus subscriber (§4.8).
//!
//! Plan §4.8 moves persistence from a direct bypass to "log-as-subscribe":
//! the session log's [`TeeHandle`] is mounted on the per-session
//! [`EventBus`](crate::bus::EventBus) as the first built-in subscription, so
//! in-process distribution and the durable record share one dispatch path and
//! one schema. Everything else (redaction, truncation, usage folding, flush
//! policy) stays inside `SessionTee` — this adapter is deliberately a pure
//! forwarder.

use crate::bus::{BusInput, BusSubscriber};

use super::TeeHandle;

/// Subscriber that appends every durable bus input to the session log.
///
/// Skipped inputs (routing-only hook triggers) are filtered inside
/// `SessionTee::record_bus_input`, keeping this type one line of glue.
#[derive(Clone)]
pub struct L0TeeSubscriber {
    tee: TeeHandle,
}

impl L0TeeSubscriber {
    /// Mount `tee` as an L0 subscriber.
    pub fn new(tee: TeeHandle) -> Self {
        Self { tee }
    }

    /// The underlying handle (used by the query loop for lifecycle parity:
    /// user message / turn start / request headers keep their dedicated
    /// entry points, whose wire semantics must not drift).
    pub fn tee(&self) -> &TeeHandle {
        &self.tee
    }
}

impl BusSubscriber for L0TeeSubscriber {
    fn on_input(&self, input: &BusInput) {
        self.tee.record_bus_input(input);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::bus::{BusEvent, DispatchMode, EventBus, TopicFilter};
    use crate::session_log::session_events_path;
    use shannon_types::session_event::{
        CustomPayload, ErrorPayload, HookFiredPayload, PermissionDecisionPayload, SessionEventBody,
    };
    use std::path::Path;
    use std::sync::Arc;

    #[test]
    fn subscriber_appends_durable_bodies_and_skips_routing_only() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let tee = TeeHandle::open_in_dir(dir.path(), "sess-bus", "m", Some("anthropic"));
        let bus = EventBus::new();
        let guard = bus.subscribe(TopicFilter::all(), Arc::new(L0TeeSubscriber::new(tee)));

        // A durable row lands in the log.
        bus.dispatch(
            BusEvent::new(SessionEventBody::Error(ErrorPayload {
                category: "t".into(),
                message: "durable".into(),
                detail: None,
            }))
            .into(),
            DispatchMode::Emit,
        );
        // A routing-only hook trigger does not persist as `custom`.
        bus.dispatch(
            crate::bus::hook_trigger_event("PreToolUse", serde_json::json!({"tool": "x"})).into(),
            DispatchMode::Emit,
        );
        // A folded usage directive neither errors nor persists a row.
        bus.dispatch(
            crate::bus::BusInput::Coalesce(crate::bus::CoalesceInput::BareTokens(42)),
            DispatchMode::Emit,
        );
        drop(guard);

        let path = session_events_path(Path::new(dir.path()), "sess-bus");
        let text = std::fs::read_to_string(path).expect("events.jsonl readable after close");
        assert_eq!(text.matches("\"kind\":\"error\"").count(), 1);
        assert!(
            !text.contains("shannon.hook.trigger"),
            "routing topics not persisted"
        );
        assert!(!text.contains("PreToolUse"));
    }

    #[test]
    fn forwarder_preserves_payloads_without_reinterpretation() {
        // The adapter forwards bodies verbatim; assert kind-tag fidelity for
        // the producer kinds added by §4.8 itself (permission + hooks audit).
        let dir = tempfile::TempDir::new().expect("tempdir");
        let tee = TeeHandle::open_in_dir(dir.path(), "sess-fwd", "m", Some("anthropic"));
        let bus = EventBus::new();
        let guard = bus.subscribe(
            TopicFilter::kinds([
                shannon_types::session_event::SessionEventKind::PermissionDecision,
                shannon_types::session_event::SessionEventKind::HookFired,
            ]),
            Arc::new(L0TeeSubscriber::new(tee)),
        );
        bus.dispatch(
            crate::bus::permission_decision_event(PermissionDecisionPayload {
                tool_name: Some("Bash".into()),
                request: None,
                decision: "deny".into(),
                reason: Some("rule deny-rm".into()),
                mode: Some("auto".into()),
            })
            .into(),
            DispatchMode::Emit,
        );
        bus.dispatch(
            crate::bus::hook_fired_event(HookFiredPayload {
                event: "PostToolUse".into(),
                hook: "/usr/bin/env guard.sh".into(),
                outcome: Some("ok".into()),
                duration_ms: Some(12),
            })
            .into(),
            DispatchMode::Emit,
        );
        // Out-of-filter kinds must NOT be persisted.
        bus.dispatch(
            BusEvent::new(SessionEventBody::Custom(CustomPayload {
                namespace: "example".into(),
                data: serde_json::json!({}),
            }))
            .into(),
            DispatchMode::Emit,
        );
        drop(guard);

        let text = std::fs::read_to_string(session_events_path(Path::new(dir.path()), "sess-fwd"))
            .expect("log readable");
        assert_eq!(text.matches("\"kind\":\"permission/decision\"").count(), 1);
        assert_eq!(text.matches("\"kind\":\"hook/fired\"").count(), 1);
        assert!(
            !text.contains("example"),
            "filter excludes unsubscribed kinds"
        );
    }
}
