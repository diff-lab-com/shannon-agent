//! P2-3 (improvement plan §P2-3): snapshot tests for the compact
//! [`Selector`](shannon_core::compact::Selector) output.
//!
//! Locks down the (strategy, reason) tuple the facade returns across the
//! four reasons and the boundary between them. The compact facade is the
//! single integration point used by the engine — any change to its
//! decisions silently shifts which strategy the engine picks, so a
//! snapshot is the cheapest way to catch drift.
//!
//! Each fixture is sized so its named branch is the *first* one the
//! selector matches in the policy chain:
//!   1. `BelowThreshold`  — total/max <= trigger_ratio
//!   2. `MessageHeavy`    — msg_count >= high_message_count (default 24)
//!   3. `CodeHeavy`       — last `keep_recent` messages are >60% code
//!   4. `TokenDense`      — few messages, very large average per message
//!
//! (F2 fix: the previous fixtures all matched branch 1 because total
//! token counts stayed under the 0.75 trigger; the four snapshots were
//! byte-identical and therefore useless. The fixtures below are sized
//! to deterministically hit each named branch.)

use shannon_core::compact::{Policy, Selector, SelectorOutcome, SelectorReason, Strategy};
use shannon_engine::api::{Message, MessageContent};

fn user(text: &str) -> Message {
    Message {
        role: "user".to_string(),
        content: MessageContent::Text(text.to_string()),
    }
}

fn code_assistant(snippet: &str) -> Message {
    // Pad each code block with surrounding commentary so a single
    // message is ~400 ASCII chars (~100 tokens). 10 of these plus a
    // non-trivial head still drive the total well over
    // 0.75 × 4000 = 3000 tokens while keeping `looks_like_code` true.
    let filler = "fn aux_helper() -> i32 { 7 } // ".repeat(40); // 40 * 32 = 1280 chars
    Message {
        role: "assistant".to_string(),
        content: MessageContent::Text(format!(
            "```rust\nfn {snippet}() -> i32 {{ 42 }}\n```\n\
             // supporting code in the same file:\n{filler}\n\
             See `bar_baz` and `MyStruct` for context. {snippet}"
        )),
    }
}

/// Default policy mirrors the one historically used in `query_engine/engine.rs`.
fn selector() -> Selector {
    Selector::new(Policy {
        trigger_ratio: 0.75,
        keep_recent: 10,
        summary_budget_tokens: 2_000,
        summary_preference_switch: 1.25,
        high_message_count: 24,
        prefer_llm_summary: true,
    })
}

fn format(outcome: &SelectorOutcome) -> String {
    format!(
        "strategy: {}\nreason:   {}\n",
        outcome.strategy, outcome.reason
    )
}

/// Below-threshold case: a small conversation must stay on `TokenBased`
/// with `BelowThreshold`. If this drifts, downstream callers that key off
/// `strategy == TokenBased` to skip work will regress silently.
#[test]
fn selector_below_threshold_snapshot() {
    let messages = vec![user("hello"), user("world")];
    let outcome = selector().recommend(&messages, 16_000);
    assert_eq!(outcome.reason, SelectorReason::BelowThreshold);
    insta::assert_snapshot!("compact_selector__below_threshold", format(&outcome));
}

/// Message-heavy case: 30 padded user messages push
/// `msg_count >= high_message_count (24)` AND push total tokens well
/// above `0.75 * 4000 = 3000`. Strategy must be `SummaryBased` with
/// `MessageHeavy`.
#[test]
fn selector_message_heavy_snapshot() {
    // Each message is ~700 ASCII chars ≈ 175 tokens. 30 × 175 ≈ 5250
    // tokens, ratio ≈ 1.0+ against a 4000-token budget — safely above
    // the 0.75 trigger ratio.
    let padding = "padding ".repeat(80); // 640 chars
    let messages: Vec<Message> = (0..30)
        .map(|i| {
            user(&format!(
                "user turn {i:02} with some extra context to push the token count \
                 up past the trigger ratio. {padding} end-of-turn {i}"
            ))
        })
        .collect();
    let outcome = selector().recommend(&messages, 4_000);
    assert_eq!(outcome.reason, SelectorReason::MessageHeavy);
    assert_eq!(outcome.strategy, Strategy::SummaryBased);
    insta::assert_snapshot!("compact_selector__message_heavy", format(&outcome));
}

/// Code-heavy tail case: 13 messages, total well over 3000 tokens, and
/// the last 10 (keep_recent) are all code-block assistant messages. The
/// `looks_like_code` heuristic must classify every tail as code so the
/// 60% threshold is exceeded, and `msg_count < 24` keeps the
/// `MessageHeavy` branch from firing first.
#[test]
fn selector_code_heavy_snapshot() {
    // 3 plain user messages padded to ~700 chars each (≈175 tokens) so
    // the head alone is ~525 tokens, well under the trigger threshold —
    // 3 messages × 175 = 525 tokens. Then 10 code-heavy assistant
    // messages contribute the bulk of the body so the total is well
    // above 0.75 × 4000 = 3000 tokens. Last 10 of 13 are all code →
    // CodeHeavy branch fires before TokenDense (msg_count = 13 > 12).
    let padding = "padding ".repeat(80); // 640 chars
    let mut messages: Vec<Message> = (0..3)
        .map(|i| {
            user(&format!(
                "user setup turn {i} with enough context to push the head over the \
                 trigger ratio and ensure the conversation is not below threshold. {padding} {i}"
            ))
        })
        .collect();
    messages.extend((0..10).map(|i| code_assistant(&format!("impl_{i:02}"))));
    let outcome = selector().recommend(&messages, 4_000);
    assert_eq!(outcome.reason, SelectorReason::CodeHeavy);
    assert_eq!(outcome.strategy, Strategy::SummaryBased);
    insta::assert_snapshot!("compact_selector__code_heavy", format(&outcome));
}

/// Token-dense case: a single very large message whose average token
/// count per message (`avg ≈ 1250`) far exceeds
/// `summary_preference_switch * 300 = 375`, while `msg_count = 2` keeps
/// us under the `high_message_count / 2 = 12` ceiling. The selector
/// must return `Strategy::TokenBased` + `SelectorReason::TokenDense`.
#[test]
fn selector_token_dense_snapshot() {
    // 5000 ASCII chars ≈ 1250 tokens per message. 2 messages × 1250 = 2500
    // tokens against a 3000-token budget → ratio ≈ 0.83 (above 0.75).
    let big = "x".repeat(5_000);
    let messages = vec![user(&big), user(&big)];
    let outcome = selector().recommend(&messages, 3_000);
    assert_eq!(outcome.reason, SelectorReason::TokenDense);
    assert_eq!(outcome.strategy, Strategy::TokenBased);
    insta::assert_snapshot!("compact_selector__token_dense", format(&outcome));
}
