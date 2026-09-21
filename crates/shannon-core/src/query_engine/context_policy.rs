//! A PR-2: context-pressure / compaction-trigger ladder extracted from
//! `engine.rs`.
//!
//! Owns the threshold ladder (micro-prune at 70% → full-compaction at
//! compression_threshold → pair-aware truncate fallback after repeated
//! failures) so the agent loop reads as a pipeline rather than a
//! 100-line inline block.
//!
//! The actual compaction work (LLM summarizer, pair-aware split,
//! selector) stays in `shannon-engine::compact` and `shannon-core::compact`;
//! this module answers the single question: given the current
//! conversation state and config, what should happen next? The answer
//! drives the engine's branching.

use shannon_engine::api::{Message, MessageContent};
use shannon_engine::compact::CompactEngine;
use shannon_engine::compact::helpers::{estimate_text_tokens, estimate_tokens};

use super::env_config::MICRO_PRUNE_THRESHOLD;

/// The recommended action for the engine's main loop. Variants
/// intentionally order from least expensive to most disruptive so a
/// caller can do a `match` that mirrors the cost ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextAction {
    /// No action: estimated usage below the warning threshold.
    Continue,
    /// Micro-prune stale tool results in the older-turn window
    /// (200-char previews). Cheap, in-place, no message removal so
    /// tool_use/tool_result pairing is preserved.
    MicroPrune,
    /// Full compaction (selector + LLM summarizer / token-based greedy).
    Compact,
    /// Truncate old messages with pair-aware split after compaction
    /// has repeatedly failed (circuit breaker).
    TruncateFallback,
}

/// Decide the action from the conversation's current usage ratio and
/// the failure counter. The returned `Action` does NOT mutate
/// `messages` — the caller is responsible for executing it and
/// re-measuring usage.
///
/// `before_tokens` is the pre-action token estimate (used by the
/// "compaction must reduce tokens" regression below); pass the
/// engine's own estimate.
pub fn evaluate(
    usage_ratio: f32,
    compact_failure_count: u32,
    max_compaction_failures: u32,
    full_compaction_threshold: f32,
) -> ContextAction {
    if compact_failure_count >= max_compaction_failures {
        return ContextAction::TruncateFallback;
    }
    if usage_ratio > full_compaction_threshold {
        return ContextAction::Compact;
    }
    if usage_ratio > MICRO_PRUNE_THRESHOLD {
        return ContextAction::MicroPrune;
    }
    ContextAction::Continue
}

/// Re-estimate the conversation's token usage after a change. Helpers
/// kept here (rather than re-imported from `streaming`) so the
/// caller never has to walk the engine crate just to recompute the
/// percentage.
pub fn reestimate(messages: &[Message], system_prompt: Option<&str>) -> usize {
    let msgs = estimate_tokens(messages);
    msgs + system_prompt.map(estimate_text_tokens).unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The full ladder maps onto the engine's existing wiring:
    /// - below micro threshold -> Continue
    /// - above micro threshold, below full compaction -> MicroPrune
    /// - above full threshold -> Compact
    /// - above full threshold after too many failures -> TruncateFallback
    #[test]
    fn ladder_matches_thresholds() {
        let full = 0.8_f32;
        assert_eq!(
            evaluate(0.50, 0, 2, full),
            ContextAction::Continue,
            "below micro threshold -> Continue"
        );
        assert_eq!(
            evaluate(0.75, 0, 2, full),
            ContextAction::MicroPrune,
            "above 0.7 below 0.8 -> MicroPrune"
        );
        assert_eq!(
            evaluate(0.85, 0, 2, full),
            ContextAction::Compact,
            "above 0.8 -> Compact"
        );
        assert_eq!(
            evaluate(0.50, 2, 2, full),
            ContextAction::TruncateFallback,
            "failure count saturates -> TruncateFallback even below threshold"
        );
        assert_eq!(
            evaluate(0.95, 3, 2, full),
            ContextAction::TruncateFallback,
            "failure count saturates -> TruncateFallback"
        );
    }

    /// A PR-2 regression (left over from the first audit): after a
    /// compaction action executes, the re-estimated token count must
    /// be less than or equal to the pre-action count. This catches
    /// "compaction silently dropped" regressions like the P0-1
    /// livelock the Wave-0 audit fixed — the live-loop test in
    /// engine.rs is the integration check; this is the unit contract.
    #[test]
    fn micro_prune_estimate_must_not_grow() {
        // Synthesize a conversation where the older half has bloated
        // tool results (good candidate for pruning) and the recent
        // tail is small.
        use shannon_engine::api::{ContentBlock, ToolResultContent};
        let big_text = "x".repeat(2_000);
        let mut messages: Vec<Message> = Vec::new();
        // Four old "tool result" rows stuffed with filler (multi-KB each).
        for i in 0..4 {
            messages.push(Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: format!("tool_{i}"),
                    content: Some(ToolResultContent::Single(big_text.clone())),
                    is_error: Some(false),
                }]),
            });
        }
        // Recent tail: small assistant text.
        messages.push(Message {
            role: "assistant".to_string(),
            content: MessageContent::Text("summary".to_string()),
        });
        let before = reestimate(&messages, None);
        // Apply prune to the older window (keep 1 = the assistant tail).
        let keep = 1.min(messages.len());
        let head = messages.len() - keep;
        CompactEngine::prune_stale_tool_results(&mut messages[..head]);
        let after = reestimate(&messages, None);
        assert!(
            after <= before,
            "prune must not grow the estimate (before={before}, after={after})"
        );
        // In our fixture the older halves are gigantic and the prune
        // caps each at ~200 chars; assert a meaningful drop.
        assert!(
            after < before / 2,
            "prune should substantially shrink the old-window tool results (before={before}, after={after})"
        );
    }
}
