//! Turn-recovery ladder — R2-6 extract from `engine.rs`.
//!
//! Encapsulates the A8 / N-3 / A14 retry ladder so the agent loop can read
//! top-down as a pipeline rather than as inline ladder logic. Each entry
//! point mirrors the inline behavior that shipped in the 10k-line engine
//! (tests pin the wire-shape: continuation nudge text, escalation formula,
//! retryable-error substring set).
//!
//! Responsibilities kept here:
//! - `push_turn_continuation_nudge`: A8 continuation prompt (one-shot).
//! - `provider_error_retryable`: N-3 classification of typed provider errors
//!   for in-place turn continuation.
//! - `turn_retries_max`: per-turn budget (env override).
//! - A14 stream-idle watchdog escalation: compute, apply, clear.

use shannon_engine::api::client::LlmClient;
use shannon_engine::api::{Message, MessageContent};

/// Re-prompt appended when a turn's LLM call is retried after a
/// timeout-class stream death. Verbatim from the A8 plan; pinned by test.
pub const TURN_CONTINUATION_NUDGE_PROMPT: &str = "Your previous response stream was \
     interrupted by a network fault. Continue from where you stopped. Keep this \
     response focused and moderately sized.";

/// Default per-turn retry budget for A8 continuation
/// (`SHANNON_TURN_RETRIES`; "0" legitimately disables).
pub const DEFAULT_TURN_RETRIES: u32 = 2;

/// A14: cap for the stream-idle watchdog budget when the engine escalates
/// it across timeout-class turn continuations. The base budget is read
/// from `SHANNON_STREAM_IDLE_SECS` (default 420s); each escalation step
/// scales it by the current retry index but never above this ceiling.
/// Beyond this, an actively-silent stream is genuinely stalled and should
/// not be rescued.
pub const STREAM_IDLE_ESCALATION_CAP_SECS: u64 = 1200;

/// A14: how much each successive timeout-class continuation within the
/// same turn multiplies the stream-idle watchdog budget. Index 1 (first
/// escalation) → ×2 = 840s, index 2 → ×3 = 1260s, then capped.
pub const STREAM_IDLE_ESCALATION_FACTOR_BASE: u64 = 1;

/// Append the A8 continuation nudge (role=user, same injection shape as
/// the A1 think-only nudge) unless it is already the last message. The
/// one-shot guard keeps a repeated stall from stacking copies: the first
/// retry appends, a second consecutive retry reuses the existing nudge so
/// every retry request carries exactly one.
pub fn push_turn_continuation_nudge(messages: &mut Vec<Message>) {
    if let Some(last) = messages.last() {
        if last.role == "user" {
            if let MessageContent::Text(text) = &last.content {
                if text == TURN_CONTINUATION_NUDGE_PROMPT {
                    return;
                }
            }
        }
    }
    messages.push(Message {
        role: "user".to_string(),
        content: MessageContent::Text(TURN_CONTINUATION_NUDGE_PROMPT.to_string()),
    });
}

/// N-3: classify a provider-reported mid-stream error message as worth an
/// in-place continuation retry (same ladder as the A8 timeout path).
/// Conservative on purpose: only transient upstream-capacity classes match;
/// anything else (auth, invalid request, content filter, context overflow)
/// fails the query immediately.
pub fn provider_error_retryable(message: &str) -> bool {
    let m = message.to_lowercase();
    m.contains("overloaded")
        || m.contains("rate limit")
        || m.contains("rate_limit")
        || m.contains("ratelimit")
        || m.contains("capacity")
        || m.contains("timeout")
        || m.contains("temporarily unavailable")
        || m.contains("service unavailable")
        || m.contains("internal server error")
        || m.contains("bad gateway")
}

/// Per-turn retry budget from `SHANNON_TURN_RETRIES` (default 2; "0"
/// disables the A8 ladder).
pub fn turn_retries_max() -> u32 {
    // P3 cleanup: use env_config's reader (single source) instead of a
    // local copy that could drift.
    super::env_config::env_num_override("SHANNON_TURN_RETRIES", DEFAULT_TURN_RETRIES)
}

/// A14: compute the escalated stream-idle watchdog budget for the next
/// continuation attempt. See engine.rs comment history for the formula.
pub fn stream_idle_escalated_budget(
    base_secs: Option<u64>,
    turn_retries_used: u32,
) -> Option<std::time::Duration> {
    let base = base_secs?;
    if turn_retries_used == 0 {
        return None;
    }
    let factor = STREAM_IDLE_ESCALATION_FACTOR_BASE + u64::from(turn_retries_used);
    let raw = base.saturating_mul(factor);
    let capped = raw.min(STREAM_IDLE_ESCALATION_CAP_SECS);
    Some(std::time::Duration::from_secs(capped))
}

/// A14: apply the escalated stream-idle budget to the client before the
/// next continuation attempt. No-op when `base_secs` is unset.
pub fn escalate_stream_idle_override(
    client: &LlmClient,
    base_secs: Option<u64>,
    turn_retries_used: u32,
) {
    let budget = stream_idle_escalated_budget(base_secs, turn_retries_used);
    client.set_stream_idle_override(budget);
}

/// A14: clear any A14 escalation on the client. Called when a stream
/// finalizes normally so the next fresh turn starts again from the base
/// budget.
pub fn clear_stream_idle_override(client: &LlmClient) {
    client.set_stream_idle_override(None);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn retryable_matches_known_transient_classes() {
        for s in [
            "overloaded_error",
            "Anthropic 529 overloaded",
            "rate limit reached",
            "rate_limit_exceeded",
            "upstream request timeout",
            "Service unavailable",
            "Bad gateway",
            "internal server error",
        ] {
            assert!(provider_error_retryable(s), "expected retryable: {s:?}");
        }
    }

    #[test]
    fn retryable_rejects_non_transient() {
        for s in [
            "invalid api key",
            "context length exceeded",
            "content policy violation",
            "model not found",
        ] {
            assert!(
                !provider_error_retryable(s),
                "expected NON-retryable: {s:?}"
            );
        }
    }

    #[test]
    fn nudge_is_idempotent() {
        let mut msgs = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text(TURN_CONTINUATION_NUDGE_PROMPT.to_string()),
        }];
        push_turn_continuation_nudge(&mut msgs);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn escalation_caps_at_1200s() {
        // 420 × (1+4+1) = 2520, capped at 1200
        let budget = stream_idle_escalated_budget(Some(420), 4).unwrap();
        assert_eq!(budget.as_secs(), STREAM_IDLE_ESCALATION_CAP_SECS);
    }

    #[test]
    fn first_attempt_has_no_override() {
        assert!(stream_idle_escalated_budget(Some(420), 0).is_none());
        assert!(stream_idle_escalated_budget(None, 1).is_none());
    }
}
