//! Host-side seam for the secret-guard plugin (blueprint artifact **(c)**
//! wiring: `docs/research/llm-secret-redaction-research-2026-09.md` §9).
//!
//! Phase 0 scope: a read-only audit over the serialized outbound request,
//! invoked from the existing request-capture point in the query engine.
//! With no plugin installed this is a no-op — zero behavior change, zero
//! cache impact (the audit never mutates the wire body, so byte-identity
//! with the request on the wire is preserved and prompt caching is safe).
//!
//! Later phases (ingest-time substitution, execution-face restore) use the
//! same seam: install an implementation of
//! [`shannon_plugin_api::ContextTransform`] via [`set_context_transform`].
//! The engine loop, not this module, will drive wiring points 1–3; Phase 0
//! only exercises `audit_wire`.
//!
//! Policy: a plugin is supplied by the host composition (today: tests and
//! embedders; later: the `secret-guard-plugin` crate behind a feature
//! flag/config). The engine itself never constructs a concrete plugin.

use std::sync::{Arc, OnceLock, RwLock};

use shannon_plugin_api::ContextTransform;

static GLOBAL_TRANSFORM: OnceLock<RwLock<Option<Arc<dyn ContextTransform>>>> = OnceLock::new();

fn slot() -> &'static RwLock<Option<Arc<dyn ContextTransform>>> {
    GLOBAL_TRANSFORM.get_or_init(|| RwLock::new(None))
}

/// Install (or remove, with `None`) the process-wide content transform.
pub fn set_context_transform(t: Option<Arc<dyn ContextTransform>>) {
    if let Ok(mut guard) = slot().write() {
        *guard = t;
    }
}

/// The currently installed transform, if any.
pub fn context_transform() -> Option<Arc<dyn ContextTransform>> {
    slot().read().ok().and_then(|g| g.clone())
}

/// Read-only audit of the serialized request body through the installed
/// transform. Returns the number of findings; each finding is logged with
/// its rule id only — never the secret value (the contract's
/// `AuditFinding` carries no secret material by construction).
pub fn audit_wire_and_log(wire: &serde_json::Value) -> usize {
    let Some(transform) = context_transform() else {
        return 0;
    };
    let findings = transform.audit_wire(wire);
    let count = findings.len();
    for f in findings {
        // Deliberately narrow log: rule id only. The wire body itself is
        // already teed into the L0 session log under the redaction policy.
        tracing::warn!(
            target: "shannon::secret_guard",
            rule = %f.rule_id,
            "potential secret detected in outbound LLM request (value redacted)"
        );
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use shannon_plugin_api::{AuditFinding, IngestBlock, RestoreAction, TransformAction};

    struct FakeDetector;

    impl ContextTransform for FakeDetector {
        fn transform_ingest(&self, _block: &mut IngestBlock) -> TransformAction {
            TransformAction::Passthrough
        }
        fn restore_tool_args(&self, _tool: &str, _args: &mut serde_json::Value) -> RestoreAction {
            RestoreAction::Unchanged
        }
        fn restore_display(&self, _text: &mut String) -> RestoreAction {
            RestoreAction::Unchanged
        }
        fn audit_wire(&self, _wire: &serde_json::Value) -> Vec<AuditFinding> {
            vec![AuditFinding {
                rule_id: "aws-access-token".to_string(),
            }]
        }
    }

    #[test]
    fn audit_noops_without_plugin_then_reports_findings() {
        // Default state: no plugin installed → zero findings, no logs.
        assert_eq!(audit_wire_and_log(&json!({ "messages": [] })), 0);

        set_context_transform(Some(Arc::new(FakeDetector)));
        let n = audit_wire_and_log(&json!({ "messages": [{ "role": "user" }] }));
        // Restore default state first so other tests observe the no-op seam.
        set_context_transform(None);
        assert_eq!(n, 1);
        assert!(context_transform().is_none());
    }
}
