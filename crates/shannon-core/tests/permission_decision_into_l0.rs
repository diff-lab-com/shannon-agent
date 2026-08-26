//! §4.8 evidence: both `permission/decision` sources land in L0 with one
//! schema.
//!
//! Route (b) agreed in §4.9 makes plugin-gate refusals ride the bus instead
//! of stopping at a tracing event. This test installs the process-wide sink,
//! fires one decision from *each* source (the plugin gate API and the query
//! layer's guard-node emitter), and requires both to appear as durable
//! `permission/decision` rows in `events.jsonl`.

#![allow(clippy::unwrap_used)]

use shannon_core::bus::{
    DispatchMode, EventBus, PluginDecisionFrame, TopicFilter, install_decision_sink,
    permission_decision_event,
};
use shannon_core::plugin::manifest::PluginPermission;
use shannon_core::plugin::permissions::{PermissionDecision, emit_decision};
use shannon_core::query_engine::guard_nodes;
use shannon_core::session_log::{L0TeeSubscriber, TeeHandle, session_events_path};
use std::path::Path;
use std::sync::Arc;

#[test]
fn permission_decisions_from_both_sources_persist_into_l0() {
    let dir = tempfile::TempDir::new().unwrap();
    let tee = TeeHandle::open_in_dir(dir.path(), "sess-decisions", "m", Some("anthropic"));
    let bus = EventBus::new();
    let _l0_guard = bus.subscribe(
        TopicFilter::all(),
        Arc::new(L0TeeSubscriber::new(tee.clone())),
    );

    // Source 1 — the plugin gate emits through its public API; the §4.8 sink
    // forwards it onto this session's bus as a vocabulary row.
    install_decision_sink({
        let bus = bus.shared();
        Arc::new(move |frame: &PluginDecisionFrame| {
            use shannon_types::session_event::PermissionDecisionPayload;
            bus.dispatch(
                permission_decision_event(PermissionDecisionPayload {
                    tool_name: None,
                    request: Some(format!("plugin '{}'", frame.plugin)),
                    decision: if frame.allowed { "allow" } else { "deny" }.into(),
                    reason: Some(format!(
                        "plugin gate '{}' requires '{}' declared [{}]",
                        frame.point,
                        frame.required,
                        frame.declared.join(", ")
                    )),
                    mode: Some("PLUGIN".into()),
                })
                .into(),
                DispatchMode::Emit,
            );
        })
    });
    emit_decision(
        "unix-probe",
        PluginPermission::Network,
        PermissionDecision::Denied,
        "transport",
        &[PluginPermission::McpTools],
    );

    // Source 2 — the permission gate node's emitter.
    guard_nodes::emit_decision(&bus, "Bash", "allow", Some("rule allow-safe-ls"), "AUTO", 3);

    drop(_l0_guard);
    drop(tee); // flush + close

    let text =
        std::fs::read_to_string(session_events_path(Path::new(dir.path()), "sess-decisions"))
            .unwrap();
    let sample_rows: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("\"kind\":\"permission/decision\""))
        .collect();
    assert_eq!(sample_rows.len(), 2, "both sources persisted");

    let plugin_row = sample_rows.iter().find(|l| l.contains("PLUGIN")).unwrap();
    assert!(plugin_row.contains("\"plugin\":\"unix-probe\"") || plugin_row.contains("unix-probe"));
    assert!(plugin_row.contains("\"decision\":\"deny\""), "{plugin_row}");
    assert!(plugin_row.contains("network"), "{plugin_row}");
    assert!(plugin_row.contains("mcp_tools"), "{plugin_row}");

    let pm_row = sample_rows.iter().find(|l| l.contains("AUTO")).unwrap();
    assert!(pm_row.contains("\"tool_name\":\"Bash\""), "{pm_row}");
    assert!(pm_row.contains("(3ms)"), "{pm_row}");

    // Sample lines for the migration report.
    println!("PLUGIN-SOURCE ROW: {plugin_row}");
    println!("PERM-GATE   ROW: {pm_row}");
}
