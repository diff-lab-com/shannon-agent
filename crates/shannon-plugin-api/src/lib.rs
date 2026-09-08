//! # shannon-plugin-api
//!
//! Content-transform middleware contract between the Shannon engine and
//! plugins (blueprint artifact **(c)**, research doc
//! `docs/research/llm-secret-redaction-research-2026-09.md` §9).
//!
//! The first consumer is the secret-guard plugin (artifact **b**, repo
//! `secret-guard-plugin`, wrapping artifact **a**, repo `secret-guard`).
//!
//! ## Contract invariants (any implementation MUST uphold them)
//!
//! - **I1 byte-stable determinism** — the same input content must always
//!   transform to the same bytes. Provider prompt caching keys on the
//!   byte-exact prefix of a request; a non-deterministic transform destroys
//!   the cache for the whole conversation suffix.
//! - **I2 one-way flow** — `transform_ingest` output is what enters
//!   conversation history. Values returned by `restore_tool_args` /
//!   `restore_display` live on the execution/display face only and MUST
//!   never be written back into history.
//! - **I3 idempotence** — transforming already-transformed content is a
//!   no-op (otherwise each turn rewrites the prefix and invalidates cache).
//! - **I4 explicit failure semantics** — a plugin declares [`FailMode`];
//!   under `Closed` an internal failure blocks the content with a reason,
//!   under `Open` it passes through. Silence is not a failure mode.
//!
//! ## Wire audit (Phase 0)
//!
//! [`ContextTransform::audit_wire`] is a read-only pass over the serialized
//! request body. It can never mutate or block; findings carry **no secret
//! material** ([`AuditFinding`]) so they are safe to log. This is the
//! lowest-risk payload for proving the contract end to end.

#![forbid(unsafe_code)]

use std::sync::Arc;

/// Where inbound content came from. Lets a plugin apply per-source policy
/// (e.g. redact tool output, only audit user messages).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestSource {
    /// Text typed by the user this turn.
    UserMessage,
    /// Output of a tool call (file reads, shell output, MCP results).
    ToolResult { tool: String },
    /// Context the engine injected (project instructions, repo map, files).
    InjectedContext { path: Option<String> },
    /// Compaction/summarization payload.
    Compaction,
    /// Anything else.
    Other,
}

/// A content block about to enter conversation history.
#[derive(Debug, Clone)]
pub struct IngestBlock {
    /// Provenance of the content.
    pub source: IngestSource,
    /// Text content (mutated in place by the plugin).
    pub text: String,
}

/// A detection on the outbound wire. Carries **no secret material** by
/// construction — findings are safe to log and aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditFinding {
    /// Detector rule identifier (e.g. `aws-access-token`).
    pub rule_id: String,
}

/// Result of [`ContextTransform::transform_ingest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransformAction {
    /// Content unchanged.
    Passthrough,
    /// `block.text` was rewritten in place.
    Modified,
    /// Content must not proceed (fail-closed policy or plugin decision).
    Blocked { reason: String },
}

/// Aggregate outcome of a restore pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreStats {
    /// Occurrences restored by exact match.
    pub replaced: usize,
    /// Occurrences recovered via fuzzy repair (model mutated the token).
    pub fuzzy: usize,
    /// Placeholder-looking tokens with no registry hit — the host must
    /// surface these to the user rather than silently ship a broken
    /// artifact (blueprint failure modes F3/F4).
    pub unresolved: Vec<String>,
}

/// Result of [`ContextTransform::restore_tool_args`] /
/// [`ContextTransform::restore_display`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreAction {
    /// Nothing placeholder-shaped was present.
    Unchanged,
    /// Restoration happened (see [`RestoreStats::unresolved`] for gaps).
    Restored(RestoreStats),
    /// The plugin failed internally; `FailMode` decides how the host reacts.
    Failed { reason: String },
}

/// What a plugin does when it cannot fulfill a request.
///
/// The default posture in the ecosystem is fail-open (GitGuardian's AI hook
/// passes requests it cannot scan); fail-closed is for managed/enterprise
/// deployments (blueprint §3.3, §9.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailMode {
    /// On internal failure, let content through and record the failure.
    #[default]
    Open,
    /// On internal failure, block the content with a reason.
    Closed,
}

/// Content-transform middleware. Implemented by plugins (e.g. the
/// secret-guard plugin); invoked by the engine at the blueprint's three
/// wiring points plus the read-only wire audit.
pub trait ContextTransform: Send + Sync {
    /// Wiring point 1 — content entering conversation history (user
    /// messages, tool results, injected context, compaction input).
    /// Implementations must uphold I1/I2/I3.
    fn transform_ingest(&self, block: &mut IngestBlock) -> TransformAction;

    /// Wiring point 2 — tool arguments before local execution. This is where
    /// placeholders the model echoed become real values on the execution
    /// face (Write/Edit contents, shell commands). Never persisted to
    /// history.
    fn restore_tool_args(&self, tool: &str, args: &mut serde_json::Value) -> RestoreAction;

    /// Wiring point 3 — text about to be displayed to the user (or pushed to
    /// IM channels). Same one-way rule as wiring point 2.
    fn restore_display(&self, text: &mut String) -> RestoreAction;

    /// Read-only audit over the serialized request body (Phase 0 payload).
    /// Must not mutate `wire`; must not include secret values in findings.
    fn audit_wire(&self, wire: &serde_json::Value) -> Vec<AuditFinding>;
}

/// The engine's built-in no-op implementation (also the default state of
/// the host seam — zero behavior change unless a plugin is installed).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopTransform;

impl ContextTransform for NoopTransform {
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
        Vec::new()
    }
}

/// A shared, type-erased transform handle.
pub fn shared(t: impl ContextTransform + 'static) -> Arc<dyn ContextTransform> {
    Arc::new(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn noop_is_passthrough_everywhere() {
        let t = NoopTransform;
        let mut block = IngestBlock {
            source: IngestSource::UserMessage,
            text: "AKIAIOSFODNN7EXAMPLE".to_string(),
        };
        assert_eq!(t.transform_ingest(&mut block), TransformAction::Passthrough);
        assert_eq!(block.text, "AKIAIOSFODNN7EXAMPLE");

        let mut args = json!({ "file_path": "/tmp/x", "content": "SG1:AAAAAAAAAAAAAAAA" });
        assert_eq!(
            t.restore_tool_args("Write", &mut args),
            RestoreAction::Unchanged
        );

        let mut text = String::from("SG1:AAAAAAAAAAAAAAAA");
        assert_eq!(t.restore_display(&mut text), RestoreAction::Unchanged);

        let wire = json!({ "model": "claude-sonnet-4-5", "messages": [] });
        assert!(t.audit_wire(&wire).is_empty());
    }

    #[test]
    fn contract_types_are_deduplicatable_and_shareable() {
        let handles: Vec<Arc<dyn ContextTransform>> =
            vec![shared(NoopTransform), shared(NoopTransform)];
        let mut block = IngestBlock {
            source: IngestSource::ToolResult {
                tool: "Read".to_string(),
            },
            text: "hello".to_string(),
        };
        for h in &handles {
            assert_eq!(h.transform_ingest(&mut block), TransformAction::Passthrough);
        }
        assert_eq!(block.text, "hello");
    }

    #[test]
    fn fail_mode_default_is_open() {
        assert_eq!(FailMode::default(), FailMode::Open);
    }
}
