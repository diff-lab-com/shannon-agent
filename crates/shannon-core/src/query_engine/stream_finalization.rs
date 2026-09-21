//! A PR-4: stream-finalization primitives extracted from `engine.rs`.
//!
//! The full stream-handling loop (`StreamingPhase` state machine,
//! A13 partial-stream salvage, A1 think-only nudge, N-3 typed error
//! arm) is still inlined in `engine.rs::process_query`; migrating it to
//! dispatch on `LoopDirective` is the follow-up step (tracked
//! separately). Until that lands `finalize_stream` is unused by the
//! loop — hence the module-level `allow(dead_code)`.

#![allow(dead_code)]

/// The active/passive phases of the streaming event loop. Replaces the
/// previous `stream_finalized: bool` flag with explicit states that
/// make transitions self-documenting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingPhase {
    /// Actively receiving content blocks from the SSE stream.
    Receiving,
    /// A terminal event (MessageDelta with tool calls or MessageStop
    /// for text-only responses) has been processed; the conversation
    /// is updated and the outer turn loop should pick up the next
    /// iteration to dispatch tool results or finalize the query.
    Finalized,
}

impl Default for StreamingPhase {
    fn default() -> Self {
        Self::Receiving
    }
}

/// What the turn loop should do once the LLM stream has ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopDirective {
    /// Stay in the outer turn loop — usually because tool calls were
    /// emitted and we need to dispatch them and call the LLM again
    /// with results.
    Continue,
    /// Stay in the turn loop BUT inject a user-side nudge (e.g. A1
    /// think-only nudge or A13 truncation continuation) before the
    /// next iteration.
    ContinueWithNudge { user_text: String },
    /// The query has completed normally — emit `QueryEvent::Completed`
    /// and return from `process_query`.
    Finalize,
    /// The query has failed irrecoverably — emit `QueryEvent::Failed`
    /// with `error` and return.
    Failed { error: String },
}

/// Accumulated state the finalizer inspects to decide what to do next.
/// Passed by reference so the engine can keep mutating these fields
/// during the loop and call `finalize_stream` once per stream-end.
pub struct StreamEnd<'a> {
    pub phase: StreamingPhase,
    pub assistant_text: &'a str,
    pub tool_call_count: usize,
    pub tool_results_pending: usize,
    pub max_turns: u32,
    pub current_turn: u32,
    pub max_think_only_nudges: u32,
    pub think_only_nudges_used: u32,
    pub think_only_min_chars: usize,
    pub stop_reason: Option<&'a str>,
}

/// Classify the stream end + accumulated state into a `LoopDirective`.
///
/// Pure: no side effects, no I/O. The engine still does the work of
/// emitting the right events and re-entering the loop, but the decision
/// tree is now isolated, testable, and one read away.
///
/// Turn-limit semantics (P3 review fix): the engine's hard stop is
/// `turn >= config.max_turns` checked at the TOP of `'agent_loop`
/// (engine.rs ~1927), and the A10 wrap-up nudge fires one turn EARLIER
/// (entering the final turn). This classifier mirrors the hard stop
/// exactly; the A10 nudge is emitted by the loop before it gets here,
/// so this function only sees the boundary itself.
pub fn finalize_stream(end: &StreamEnd<'_>) -> LoopDirective {
    // Hard stop on the turn limit — same boundary as the engine's
    // top-of-loop `turn >= config.max_turns` check.
    if end.current_turn >= end.max_turns {
        return LoopDirective::Finalize;
    }

    // Already finalised -> the outer loop continues to dispatch tool
    // results (or, when tool_call_count == 0, to actually end the
    // query). Returning Continue here is the safest default; the
    // engine still chooses to emit `Completed` once tool_results_pending
    // drains AND no further tool calls were generated.
    if end.phase == StreamingPhase::Finalized {
        if end.tool_call_count == 0 && end.tool_results_pending == 0 {
            // Text-only response with no tool calls -> complete.
            return LoopDirective::Finalize;
        }
        return LoopDirective::Continue;
    }

    // Phase is still `Receiving` -> the stream ended abnormally
    // (timeout / typed error / etc.). The engine has already retried
    // via A8 if applicable; if we are here without a phase advance the
    // query must fail to avoid the livelock the Wave-0 audit fixed.
    LoopDirective::Failed {
        error: format!(
            "stream ended without a terminal frame (text_len={}, tool_calls={})",
            end.assistant_text.len(),
            end.tool_call_count
        ),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn end_with(phase: StreamingPhase, tool_calls: usize, pending: usize, turn: u32) -> StreamEnd<'static> {
        // Safety: we pass borrowed &'static strs via a leak here ONLY
        // for tests; tests are short-lived, so the leak is bounded.
        let text: &'static str = "ok";
        StreamEnd {
            phase,
            assistant_text: text,
            tool_call_count: tool_calls,
            tool_results_pending: pending,
            max_turns: 20,
            current_turn: turn,
            max_think_only_nudges: 2,
            think_only_nudges_used: 0,
            think_only_min_chars: 0,
            stop_reason: None,
        }
    }

    #[test]
    fn finalized_text_only_completes() {
        let d = finalize_stream(&end_with(StreamingPhase::Finalized, 0, 0, 0));
        assert_eq!(d, LoopDirective::Finalize);
    }

    #[test]
    fn finalized_with_tool_calls_continues_for_dispatch() {
        let d = finalize_stream(&end_with(StreamingPhase::Finalized, 2, 0, 0));
        assert_eq!(d, LoopDirective::Continue);
    }

    #[test]
    fn receiving_without_tool_calls_still_needs_a_terminal_frame() {
        // Engine emits `Completed` only when the phase advanced; if
        // not, the query must fail rather than loop forever.
        let d = finalize_stream(&end_with(StreamingPhase::Receiving, 0, 0, 0));
        assert!(matches!(d, LoopDirective::Failed { .. }));
    }

    #[test]
    fn turn_limit_finalizes() {
        // Boundary mirrors the engine's top-of-loop check:
        // turn >= max_turns (20) finalizes; turn 19 is still the final
        // working turn (the A10 wrap-up nudge was already injected).
        let d = finalize_stream(&end_with(StreamingPhase::Finalized, 0, 0, 20));
        assert_eq!(d, LoopDirective::Finalize);
        let still_working = finalize_stream(&end_with(StreamingPhase::Finalized, 2, 0, 19));
        assert_eq!(still_working, LoopDirective::Continue);
    }
}
