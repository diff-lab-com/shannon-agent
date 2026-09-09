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

/// ---- Phase 2 wiring points (blueprint §9.6) -------------------------------
///
/// The engine drives these at the outgoing-send boundary and the tool
/// execution boundary. Both are no-ops until a plugin is installed via
/// [`set_context_transform`].
use shannon_engine::api::types::{ContentBlock, Message, MessageContent, ToolResultContent};
use shannon_plugin_api::{IngestBlock, IngestSource, RestoreAction};

fn ingest_source_for_role(role: &str) -> IngestSource {
    match role {
        "user" => IngestSource::UserMessage,
        _ => IngestSource::Other,
    }
}

fn transform_text(
    t: &dyn shannon_plugin_api::ContextTransform,
    source: IngestSource,
    text: &mut String,
) {
    let mut block = IngestBlock {
        source,
        text: std::mem::take(text),
    };
    let _ = t.transform_ingest(&mut block);
    *text = block.text;
}

fn transform_content_blocks(
    t: &dyn shannon_plugin_api::ContextTransform,
    blocks: &mut [ContentBlock],
) {
    for b in blocks.iter_mut() {
        match b {
            ContentBlock::Text { text } => {
                transform_text(t, IngestSource::Other, text);
            }
            ContentBlock::ToolResult { content, .. } => match content {
                Some(ToolResultContent::Single(s)) => {
                    transform_text(t, IngestSource::Other, s);
                }
                Some(ToolResultContent::Multiple(inner)) => {
                    transform_content_blocks(t, inner);
                }
                None => {}
            },
            // `ToolUse` echoes the model's own output (already surrogate-form
            // where applicable); deterministic idempotence (I3) makes touching
            // it unnecessary.
            _ => {}
        }
    }
}

/// Wiring point 1 (send boundary): run every outgoing message through the
/// installed transform. Deterministic (I1) + idempotent (I3) transforms keep
/// the request prefix byte-stable across turns, so provider prompt caching
/// is unaffected. Returns the input vector unchanged when no transform is
/// installed.
pub fn transform_outgoing_messages(messages: Vec<Message>) -> Vec<Message> {
    let Some(t) = context_transform() else {
        return messages;
    };
    messages
        .into_iter()
        .map(|mut m| {
            match &mut m.content {
                MessageContent::Text(text) => {
                    transform_text(t.as_ref(), ingest_source_for_role(&m.role), text);
                }
                MessageContent::Blocks(blocks) => {
                    transform_content_blocks(t.as_ref(), blocks);
                }
            }
            m
        })
        .collect()
}

/// Wiring point 2 (tool execution boundary): the model echoes surrogates
/// into tool arguments; restore real values here, on the execution face
/// only. Never persist the restored input back into conversation history.
/// `Err(reason)` means the plugin failed under `FailMode::Closed` — the
/// caller must refuse execution with a tool-level error.
pub fn restore_tool_args_for_execution(
    tool: &str,
    input: &mut serde_json::Value,
) -> Result<(), String> {
    let Some(t) = context_transform() else {
        return Ok(());
    };
    match t.restore_tool_args(tool, input) {
        RestoreAction::Unchanged => Ok(()),
        RestoreAction::Restored(stats) => {
            for token in &stats.unresolved {
                tracing::warn!(
                    target: "shannon::secret_guard",
                    tool,
                    token = %token,
                    "unresolved placeholder in tool arguments (possible F3/F4); passing through to execution"
                );
            }
            Ok(())
        }
        RestoreAction::Failed { reason } => Err(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use shannon_plugin_api::{AuditFinding, IngestBlock, RestoreAction, TransformAction};
    use std::sync::Mutex;

    /// Tests that touch the process-global transform hold this lock.
    static GLOBAL_LOCK: Mutex<()> = Mutex::new(());

    const SECRET: &str = "AKIAIOSFODNN7EXAMPLE";
    const TOKEN: &str = "SG1:FAKEFAKEFAKEFAKE";

    /// Walks JSON strings replacing TOKEN→SECRET on restore.
    struct RoundTrip;

    fn walk(v: &mut serde_json::Value, from: &str, to: &str) -> bool {
        match v {
            serde_json::Value::String(s) => {
                if s.contains(from) {
                    *s = s.replace(from, to);
                    true
                } else {
                    false
                }
            }
            serde_json::Value::Array(a) => a.iter_mut().any(|x| walk(x, from, to)),
            serde_json::Value::Object(m) => m.values_mut().any(|x| walk(x, from, to)),
            _ => false,
        }
    }

    impl shannon_plugin_api::ContextTransform for RoundTrip {
        fn transform_ingest(&self, block: &mut IngestBlock) -> TransformAction {
            if block.text.contains(SECRET) {
                block.text = block.text.replace(SECRET, TOKEN);
                TransformAction::Modified
            } else {
                TransformAction::Passthrough
            }
        }
        fn restore_tool_args(&self, _tool: &str, args: &mut serde_json::Value) -> RestoreAction {
            if walk(args, TOKEN, SECRET) {
                RestoreAction::Restored(shannon_plugin_api::RestoreStats {
                    replaced: 1,
                    fuzzy: 0,
                    unresolved: Vec::new(),
                })
            } else {
                RestoreAction::Unchanged
            }
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

    struct Failing;

    impl shannon_plugin_api::ContextTransform for Failing {
        fn transform_ingest(&self, _block: &mut IngestBlock) -> TransformAction {
            TransformAction::Passthrough
        }
        fn restore_tool_args(&self, _tool: &str, _args: &mut serde_json::Value) -> RestoreAction {
            RestoreAction::Failed {
                reason: "locked".to_string(),
            }
        }
        fn restore_display(&self, _text: &mut String) -> RestoreAction {
            RestoreAction::Unchanged
        }
        fn audit_wire(&self, _wire: &serde_json::Value) -> Vec<AuditFinding> {
            Vec::new()
        }
    }

    #[test]
    fn outgoing_messages_noop_without_plugin() {
        let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_context_transform(None);
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text(format!("key={SECRET}")),
        }];
        let out = transform_outgoing_messages(messages);
        match &out[0].content {
            MessageContent::Text(t) => assert_eq!(t, &format!("key={SECRET}")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn outgoing_messages_transform_text_and_tool_results() {
        let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_context_transform(Some(std::sync::Arc::new(RoundTrip)));
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::Text(format!("key={SECRET}")),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".to_string(),
                    content: Some(ToolResultContent::Single(format!("value {SECRET}"))),
                    is_error: None,
                }]),
            },
        ];
        let out = transform_outgoing_messages(messages);
        set_context_transform(None);
        match &out[0].content {
            MessageContent::Text(t) => assert!(!t.contains(SECRET), "raw secret on wire: {t}"),
            other => panic!("unexpected {other:?}"),
        }
        match &out[1].content {
            MessageContent::Blocks(blocks) => match &blocks[0] {
                ContentBlock::ToolResult { content, .. } => match content {
                    Some(ToolResultContent::Single(s)) => {
                        assert!(!s.contains(SECRET), "tool result leaked: {s}");
                    }
                    other => panic!("unexpected {other:?}"),
                },
                other => panic!("unexpected {other:?}"),
            },
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn restore_at_execution_boundary_roundtrips_tool_args() {
        let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_context_transform(Some(std::sync::Arc::new(RoundTrip)));
        let mut args = json!({ "file_path": "/app/.env", "content": format!("id={TOKEN}") });
        let res = restore_tool_args_for_execution("Write", &mut args);
        set_context_transform(None);
        assert!(res.is_ok());
        assert_eq!(args["content"], format!("id={SECRET}"));
    }

    #[test]
    fn restore_failure_maps_to_err_for_fail_closed() {
        let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_context_transform(Some(std::sync::Arc::new(Failing)));
        let mut args = json!({ "x": 1 });
        let res = restore_tool_args_for_execution("Bash", &mut args);
        set_context_transform(None);
        assert_eq!(res.expect_err("fail-closed maps to Err"), "locked");
    }

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
        let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
