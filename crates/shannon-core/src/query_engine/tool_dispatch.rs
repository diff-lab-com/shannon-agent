//! A PR-3: tool-dispatch primitives extracted from `engine.rs`.
//!
//! The full tool dispatch loop (permission waterfall -> PreToolUse hook
//! -> parallel/serial partitioning -> result capture) is still inlined
//! in `engine.rs::process_query`; that work is the A PR-3 next step
//! (migration tracked separately). This module ships the **primitives**
//! the loop relies on so the loop body shrinks while the migration is
//! in flight:
//!
//! - `ToolUseRequest`: the typed wire-shape for `QueryEvent::ToolUseRequest`
//! - `StrandedInputGuard`: per-turn input tracking for the P3-8 partial-
//!   stream salvage rules
//! - `ToolDispatchPlan`: the per-turn snapshot needed to execute tool calls
//!   after the LLM stream lands (and after the type-safety checks)
//!
//! Together these let the next A PR-3 sub-step collapse ~600 lines of
//! inline tool-loop into a single `dispatch_plan().execute()` call
//! while preserving byte-for-byte event semantics (the existing
//! mocked-SSE integration tests in `engine.rs` will catch regressions).

use shannon_engine::api::Message;
use serde_json::Value;

/// A single normalized tool call the LLM emitted this turn. The full
/// `ContentBlock::ToolUse { id, name, input }` is richer (carries
/// provenance flags); this struct captures only what the dispatch loop
/// needs (the call lookup + result pairing).
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

impl ToolCall {
    pub fn new(id: impl Into<String>, name: impl Into<String>, input: Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            input,
        }
    }
}

/// Tracks tool-use IDs the producer has emitted a `ToolUseRequest` for in
/// the current turn. Used by the partial-stream salvage rules (P3-8):
/// when the stream dies mid-turn, any input messages the model emitted
/// that the producer never built a `ToolUseRequest` for are treated as
/// stray → either flushed as synthetic error tool_results (so the next
/// turn sees paired results) or dropped depending on stream position.
///
/// The full implementation is still inlined in `engine.rs`; this struct
/// is the minimal public surface so a future PR can move the salvage
/// path into this module without changing the producer contract.
#[derive(Default)]
pub struct StrandedInputGuard {
    emitted_ids: std::collections::HashSet<String>,
}

impl StrandedInputGuard {
    pub fn new() -> Self {
        Self {
            emitted_ids: std::collections::HashSet::new(),
        }
    }

    /// Mark a tool-use id as emitted (e.g. via `ToolUseRequest`).
    pub fn note_emitted(&mut self, id: &str) {
        self.emitted_ids.insert(id.to_string());
    }

    /// Returns the set of tool-use IDs the producer never built a
    /// request for (i.e. was stranded when the stream ended). The
    /// caller pairs these with synthetic error tool_results so the
    /// next turn does not 400 on orphaned tool_use.
    pub fn stranded(&self, candidates: &[String]) -> Vec<String> {
        candidates
            .iter()
            .filter(|id| !self.emitted_ids.contains(*id))
            .cloned()
            .collect()
    }
}

/// A snapshot of the tool calls the LLM just emitted, after the engine
/// has normalized / deduped / paired them. The dispatch loop takes a
/// `&ToolDispatchPlan` and runs each call through the permission gate
/// + the parallel/serial partitioner.
#[derive(Debug, Clone)]
pub struct ToolDispatchPlan {
    pub calls: Vec<ToolCall>,
    pub prior_messages: Vec<Message>,
}

impl ToolDispatchPlan {
    pub fn new(calls: Vec<ToolCall>, prior_messages: Vec<Message>) -> Self {
        Self {
            calls,
            prior_messages,
        }
    }

    pub fn len(&self) -> usize {
        self.calls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

    /// Yield parallel-safe vs serial-only batches. Read-only tools can
    /// run concurrently; everything else stays serial. Delegates to the
    /// registry's `partition_tool_calls` adapter; `max_parallel` controls
    /// the concurrency cap (matches the engine's `max_parallel_tools`
    /// config).
    pub fn partition(
        &self,
        registry: &crate::tools::ToolRegistry,
        max_parallel: usize,
    ) -> Vec<crate::tools::ToolBatch> {
        let triplets = self
            .calls
            .iter()
            .map(|c| (c.id.clone(), c.name.clone(), c.input.clone()))
            .collect();
        registry.partition_tool_calls(triplets, max_parallel)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn stranded_guard_filters_emitted_ids() {
        let mut g = StrandedInputGuard::new();
        g.note_emitted("a");
        g.note_emitted("b");
        let stranded = g.stranded(&["a".into(), "b".into(), "c".into(), "d".into()]);
        assert_eq!(stranded, vec!["c".to_string(), "d".to_string()]);
    }

    #[test]
    fn dispatch_plan_len_and_empty() {
        let empty = ToolDispatchPlan::new(vec![], vec![]);
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        let two = ToolDispatchPlan::new(
            vec![
                ToolCall::new("a", "Read", serde_json::json!({})),
                ToolCall::new("b", "Bash", serde_json::json!({})),
            ],
            vec![],
        );
        assert!(!two.is_empty());
        assert_eq!(two.len(), 2);
    }
}
