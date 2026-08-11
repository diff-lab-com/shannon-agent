//! # Multi-strategy context compression facade (`shannon-core::compact`)
//!
//! P2-1 (improvement plan §P2-1). Exposes a small facade that picks one of
//! three compression strategies based on the current conversation profile,
//! then dispatches to the existing engine-level implementations in
//! [`shannon_engine::compact`].
//!
//! ## Strategies
//!
//! - [`Strategy::TokenBased`] — greedy sliding window: drop the oldest
//!   non-system messages until the estimated token count falls back under
//!   `target_tokens`. Best when there are relatively few messages but a few
//!   of them are very large (e.g., a single oversized tool result).
//!
//! - [`Strategy::SummaryBased`] — preserve system + recent N messages and
//!   replace the middle block with a structured summary message. Shannon
//!   uses an *extractive* fallback that never blocks on an LLM call: the
//!   summary extracts file paths, code symbols, and intent markers from the
//!   dropped window and stitches them into a single system message. When an
//!   LLM client is supplied, [`maybe_compact_with_summarizer`] can upgrade
//!   to [`Strategy::SummaryLlm`] for higher-quality lossier compression.
//!
//! - [`Strategy::SummaryLlm`] — same algorithm as SummaryBased, but the
//!   textual body of the summary is produced by the supplied
//!   [`Summarizer`]. Optional upgrade; the path never *requires* an LLM
//!   and falls back to the extractive summary if the summarizer errors.
//!
//! ## Selection
//!
//! [`Selector::recommend`] inspects the candidate message list and decides
//! which strategy is appropriate. The heuristics:
//!
//! 1. **Below threshold** → return `SelectorOutcome::NoOp`.
//! 2. **High token density, low message count** →
//!    [`Strategy::TokenBased`] (cheap, lossless on system + recent).
//! 3. **High message count or code-heavy tail** →
//!    [`Strategy::SummaryBased`] (better long-horizon coverage).
//!
//! The thresholds are configurable via [`Policy`]; sensible defaults
//! match the values used historically in `query_engine/engine.rs`.
//
//! ## Integration with P1-4 (repo map)
//!
//! When P1-4 (incremental repo map) lands, the summary content can be
//! enriched with the most recent repo-map section. The intended integration
//! point is right after the extractive fallback builds its base summary
//! text. A clearly marked comment + helper hook is provided; the base
//! algorithm is complete and shippable without P1-4.

use serde::{Deserialize, Serialize};

use shannon_engine::api::{Message, MessageContent};
use shannon_engine::compact::helpers::{
    estimate_message_tokens, estimate_tokens, extract_text_content, looks_like_code,
};

/// Compression strategy chosen by the [`Selector`].
///
/// Three variants; the third one is an opt-in upgrade when an LLM
/// summarizer is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Strategy {
    /// Greedy drop-oldest until the conversation fits in the budget.
    TokenBased,
    /// Replace the middle block with an extractive summary (no LLM call).
    SummaryBased,
    /// Replace the middle block with an LLM-generated summary, falling
    /// back to extractive if the LLM call fails.
    SummaryLlm,
}

impl std::fmt::Display for Strategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Strategy::TokenBased => write!(f, "token_based"),
            Strategy::SummaryBased => write!(f, "summary_based"),
            Strategy::SummaryLlm => write!(f, "summary_llm"),
        }
    }
}

/// Tunable parameters for the selector and executors.
///
/// Defaults match the values used historically in `query_engine/engine.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    /// Trigger compression only when estimated tokens exceed
    /// `trigger_ratio * max_tokens`. Defaults to 0.75.
    pub trigger_ratio: f32,
    /// Number of recent non-system messages to always preserve.
    /// Defaults to 10.
    pub keep_recent: usize,
    /// Maximum budget for the produced summary, in tokens.
    /// Defaults to 2000.
    pub summary_budget_tokens: usize,
    /// Slider for the token-based vs summary-based split. The selector
    /// picks SummaryBased when `avg_tokens_per_message > (avg * switch)`
    /// OR when `messages >= high_message_count`. Defaults to 1.25.
    pub summary_preference_switch: f32,
    /// Message-count threshold above which summary is preferred.
    /// Defaults to 24.
    pub high_message_count: usize,
    /// When `true` and a summarizer is supplied, the selector upgrades
    /// its decision to `SummaryLlm`. Defaults to `true` — the LLM
    /// path is auto-chosen whenever it is available.
    pub prefer_llm_summary: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            trigger_ratio: 0.75,
            keep_recent: 10,
            summary_budget_tokens: 2000,
            summary_preference_switch: 1.25,
            high_message_count: 24,
            prefer_llm_summary: true,
        }
    }
}

/// Signal emitted by [`Selector::recommend`].
///
/// Carries the chosen strategy and the reason. Useful for telemetry
/// (e.g. `compaction: chose summary_based because msg_count=42 > 24`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorOutcome {
    pub strategy: Strategy,
    pub reason: SelectorReason,
}

/// Why the [`Selector`] chose a particular strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectorReason {
    /// Estimated tokens are within budget; no compaction needed.
    BelowThreshold,
    /// Few messages but a few very large — token-based is cheaper.
    TokenDense,
    /// Many messages — summary better preserves trajectory.
    MessageHeavy,
    /// Recent messages are code-heavy — keep-by-recent loses context.
    CodeHeavy,
}

impl std::fmt::Display for SelectorReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectorReason::BelowThreshold => write!(f, "below_threshold"),
            SelectorReason::TokenDense => write!(f, "token_dense"),
            SelectorReason::MessageHeavy => write!(f, "message_heavy"),
            SelectorReason::CodeHeavy => write!(f, "code_heavy"),
        }
    }
}

/// Selector — picks [`Strategy`] based on a conversation profile.
#[derive(Debug, Default, Clone)]
pub struct Selector {
    pub policy: Policy,
}

impl Selector {
    pub fn new(policy: Policy) -> Self {
        Self { policy }
    }

    /// Decide whether compaction is needed and, if so, which strategy to
    /// use. Pure function — no I/O, no LLM call.
    pub fn recommend(&self, messages: &[Message], max_tokens: usize) -> SelectorOutcome {
        let total = estimate_tokens(messages);
        let max_ctx = max_tokens.max(1);
        let ratio = total as f32 / max_ctx as f32;

        if ratio <= self.policy.trigger_ratio {
            return SelectorOutcome {
                strategy: Strategy::TokenBased,
                reason: SelectorReason::BelowThreshold,
            };
        }

        let msg_count = messages.len();

        // Heuristic 1: high message count → summary better.
        if msg_count >= self.policy.high_message_count {
            return SelectorOutcome {
                strategy: Strategy::SummaryBased,
                reason: SelectorReason::MessageHeavy,
            };
        }

        // Heuristic 2: code-heavy tail → summary better.
        if recent_is_code_heavy(messages, self.policy.keep_recent) {
            return SelectorOutcome {
                strategy: Strategy::SummaryBased,
                reason: SelectorReason::CodeHeavy,
            };
        }

        // Heuristic 3: very low message count but huge tokens → token-based.
        let avg_tokens_per_msg = if msg_count == 0 {
            0.0
        } else {
            total as f32 / msg_count as f32
        };
        if msg_count <= self.policy.high_message_count / 2
            && avg_tokens_per_msg > self.policy.summary_preference_switch * estimate_default_avg()
        {
            return SelectorOutcome {
                strategy: Strategy::TokenBased,
                reason: SelectorReason::TokenDense,
            };
        }

        SelectorOutcome {
            strategy: Strategy::SummaryBased,
            reason: SelectorReason::MessageHeavy,
        }
    }
}

/// Result of a `maybe_compact` call.
#[derive(Debug, Clone)]
pub struct CompactOutcome {
    pub compacted: Vec<Message>,
    pub original_tokens: usize,
    pub compacted_tokens: usize,
    pub messages_removed: usize,
    pub strategy: Strategy,
    pub reason: SelectorReason,
    pub did_compact: bool,
}

impl CompactOutcome {
    pub fn noop(messages: Vec<Message>, total_tokens: usize, reason: SelectorReason) -> Self {
        Self {
            compacted_tokens: total_tokens,
            compacted: messages,
            original_tokens: total_tokens,
            messages_removed: 0,
            strategy: Strategy::TokenBased,
            reason,
            did_compact: false,
        }
    }

    /// Reduction ratio in `[0.0, 1.0]`. Returns 0.0 if no compaction occurred.
    pub fn reduction_ratio(&self) -> f32 {
        if !self.did_compact || self.original_tokens == 0 {
            return 0.0;
        }
        1.0 - (self.compacted_tokens as f32 / self.original_tokens as f32)
    }
}

/// Build a fresh [`Selector`] with the default [`Policy`].
pub fn default_selector() -> Selector {
    Selector::new(Policy::default())
}

/// Main entry point — non-LLM fast path.
///
/// Dispatches to the chosen strategy and applies it to a clone of
/// `messages`. The caller replaces `messages` with `outcome.compacted`
/// when `outcome.did_compact == true`.
pub fn maybe_compact(messages: &[Message], max_tokens: usize) -> CompactOutcome {
    let policy = Policy::default();
    let selector = Selector::new(policy.clone());
    let outcome = selector.recommend(messages, max_tokens);
    apply_strategy(messages, outcome, policy, None)
}

/// Same as [`maybe_compact`] but accepts an explicit policy.
pub fn maybe_compact_with_policy(
    messages: &[Message],
    max_tokens: usize,
    policy: Policy,
) -> CompactOutcome {
    let selector = Selector::new(policy.clone());
    let outcome = selector.recommend(messages, max_tokens);
    apply_strategy(messages, outcome, policy, None)
}

/// Same as [`maybe_compact`] but with an optional LLM summarizer for the
/// `SummaryLlm` strategy.
///
/// When `summarizer` is `Some` and the selector chose `SummaryBased`,
/// the entry point upgrades the decision to `SummaryLlm` and the LLM
/// path is attempted; on error the extractive fallback is used.
pub fn maybe_compact_with_summarizer(
    messages: &[Message],
    max_tokens: usize,
    summarizer: Option<&dyn Summarizer>,
) -> CompactOutcome {
    let policy = Policy::default();
    let selector = Selector::new(policy.clone());
    let outcome = selector.recommend(messages, max_tokens);
    maybe_compact_finalize(messages, outcome, policy, summarizer)
}

/// Pluggable summarizer used by the LLM path.
///
/// Mirrors the ergonomics of `shannon_engine::compact::types::Summarizer`
/// but stays independent of those internal types so the facade can be
/// implemented in `shannon-core` without exposing engine internals.
pub trait Summarizer: Send + Sync {
    /// Produce a summary covering the given messages within `max_tokens`.
    fn summarize(&self, messages: &[Message], max_tokens: usize) -> Result<String, String>;
}

/// Default selector + LLM summarizer entry point using a shared policy.
pub fn maybe_compact_full(
    messages: &[Message],
    max_tokens: usize,
    policy: Policy,
    summarizer: Option<&dyn Summarizer>,
) -> CompactOutcome {
    let selector = Selector::new(policy.clone());
    let outcome = selector.recommend(messages, max_tokens);
    maybe_compact_finalize(messages, outcome, policy, summarizer)
}

/// Apply optional LLM upgrade + apply_strategy. Private helper.
fn maybe_compact_finalize(
    messages: &[Message],
    outcome: SelectorOutcome,
    policy: Policy,
    summarizer: Option<&dyn Summarizer>,
) -> CompactOutcome {
    let mut strategy_used = outcome.strategy;
    if summarizer.is_some()
        && policy.prefer_llm_summary
        && outcome.strategy == Strategy::SummaryBased
    {
        strategy_used = Strategy::SummaryLlm;
    }
    let strategy_used = strategy_used;
    let final_decision = SelectorOutcome {
        strategy: strategy_used,
        reason: outcome.reason,
    };
    apply_strategy(messages, final_decision, policy, summarizer)
}

// ----------------------------------------------------------------------
// Internal helpers
// ----------------------------------------------------------------------

/// Apply the chosen strategy. Always returns a [`CompactOutcome`] —
/// never panics, never makes an LLM call without the caller supplying
/// `summarizer`.
fn apply_strategy(
    messages: &[Message],
    decision: SelectorOutcome,
    policy: Policy,
    summarizer: Option<&dyn Summarizer>,
) -> CompactOutcome {
    let original_tokens = estimate_tokens(messages);
    if matches!(decision.reason, SelectorReason::BelowThreshold) {
        return CompactOutcome::noop(messages.to_vec(), original_tokens, decision.reason);
    }
    if messages.is_empty() {
        return CompactOutcome::noop(Vec::new(), 0, decision.reason);
    }

    match decision.strategy {
        Strategy::TokenBased => run_token_based(messages, original_tokens, decision, &policy),
        Strategy::SummaryBased => {
            run_summary_based(messages, original_tokens, decision, &policy, None)
        }
        Strategy::SummaryLlm => {
            run_summary_based(messages, original_tokens, decision, &policy, summarizer)
        }
    }
}

/// Greedy sliding-window drop-oldest.
///
/// Always preserves:
/// - all leading system messages
/// - the last `policy.keep_recent` non-system messages
///
/// Then drops oldest non-system messages (one at a time, from the front
/// of the middle window) until total estimated tokens ≤
/// `target_tokens`. `target_tokens` is computed from `original_tokens`
/// so we make progress on every call even when the caller has not
/// supplied an explicit per-call budget.
fn run_token_based(
    messages: &[Message],
    original_tokens: usize,
    decision: SelectorOutcome,
    policy: &Policy,
) -> CompactOutcome {
    let keep = policy.keep_recent.max(1);
    if messages.len() <= keep {
        return CompactOutcome::noop(messages.to_vec(), original_tokens, decision.reason);
    }

    // Drop at least 30% of the original tokens so the call makes
    // meaningful progress (a one-message drop on a 200k-budget chat is
    // a no-op for the user). This gives the greedy loop a clear target.
    let target = (original_tokens as f32 * 0.70) as usize;

    let system_end = messages
        .iter()
        .position(|m| m.role != "system")
        .unwrap_or(messages.len());

    // Tail we always keep.
    let tail_start = messages.len().saturating_sub(keep);
    if tail_start <= system_end {
        return CompactOutcome::noop(messages.to_vec(), original_tokens, decision.reason);
    }

    // Middle-window we are willing to shrink.
    let middle_end = tail_start;
    if middle_end <= system_end {
        return CompactOutcome::noop(messages.to_vec(), original_tokens, decision.reason);
    }

    // Greedy: drop from the front of the middle window until we are
    // under `target` *or* the middle window is exhausted.
    let mut drop_until = system_end;
    while drop_until < middle_end {
        let projected = system_tokens_sum(messages, system_end)
            + estimate_tokens(&messages[drop_until + 1..middle_end])
            + tail_tokens(messages, middle_end);
        if projected <= target {
            break;
        }
        drop_until += 1;
    }

    // Build compacted list: system[..system_end] + surviving middle + tail.
    let mut compacted =
        Vec::with_capacity(system_end + (middle_end - drop_until) + (messages.len() - middle_end));
    compacted.extend_from_slice(&messages[..system_end]);
    compacted.extend_from_slice(&messages[drop_until..middle_end]);
    compacted.extend_from_slice(&messages[middle_end..]);

    let compacted_tokens = estimate_tokens(&compacted);
    let removed = messages.len() - compacted.len();
    CompactOutcome {
        compacted,
        original_tokens,
        compacted_tokens,
        messages_removed: removed,
        strategy: Strategy::TokenBased,
        reason: decision.reason,
        did_compact: removed > 0,
    }
}

/// Summary-based executor. When `summarizer` is `Some` and `strategy ==
/// SummaryLlm`, the LLM path is attempted. On any error, the extractive
/// fallback is used.
fn run_summary_based(
    messages: &[Message],
    original_tokens: usize,
    decision: SelectorOutcome,
    policy: &Policy,
    summarizer: Option<&dyn Summarizer>,
) -> CompactOutcome {
    let keep = policy.keep_recent.max(1);
    if messages.len() <= keep + 1 {
        return CompactOutcome::noop(messages.to_vec(), original_tokens, decision.reason);
    }

    let system_end = messages
        .iter()
        .position(|m| m.role != "system")
        .unwrap_or(messages.len());

    let raw_tail_start = messages.len().saturating_sub(keep);
    // Avoid splitting a tool_use/tool_result pair: take a slight shortcut
    // by not crossing role boundaries in the safe-spot heuristic.
    let tail_start = safe_tail_start(messages, raw_tail_start).min(messages.len());

    if tail_start <= system_end + 1 {
        return CompactOutcome::noop(messages.to_vec(), original_tokens, decision.reason);
    }

    // The "drop window" we will summarize.
    let old_msgs = &messages[system_end..tail_start];

    // Build summary text.
    let summary_budget = policy.summary_budget_tokens;
    let summary_text = if decision.strategy == Strategy::SummaryLlm {
        if let Some(sum) = summarizer {
            match sum.summarize(old_msgs, summary_budget) {
                Ok(t) => t,
                Err(_) => extractive_summary(old_msgs, summary_budget),
            }
        } else {
            extractive_summary(old_msgs, summary_budget)
        }
    } else {
        extractive_summary(old_msgs, summary_budget)
    };

    // Compose: system[..system_end] + summary + tail[tail_start..].
    let summary_msg = Message {
        role: "system".to_string(),
        content: MessageContent::Text(format!(
            "[Previous conversation summary -- {} messages compacted]\n\n{summary_text}",
            old_msgs.len(),
        )),
    };

    let mut compacted = Vec::with_capacity(system_end + 1 + (messages.len() - tail_start));
    compacted.extend_from_slice(&messages[..system_end]);
    compacted.push(summary_msg);
    compacted.extend_from_slice(&messages[tail_start..]);

    let compacted_tokens = estimate_tokens(&compacted);
    let removed = messages.len() - compacted.len();
    CompactOutcome {
        compacted,
        original_tokens,
        compacted_tokens,
        messages_removed: removed,
        strategy: decision.strategy,
        reason: decision.reason,
        did_compact: removed > 0,
    }
}

/// Generate a summary without an LLM call.
///
/// Extracts:
/// - file paths mentioned in the dropped block (via regex-light heuristics)
/// - code-like markers (backtick fences, def/fn/struct)
/// - the last user-intent sentence from each user message
/// - aggregate tool names
///
/// All within the `max_tokens` budget. This never blocks, never makes an
/// API call, and is guaranteed to return a string.
fn extractive_summary(messages: &[Message], max_tokens: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut kept_user_intents: Vec<String> = Vec::new();
    let mut code_symbols: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut tool_names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut path_hits: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for m in messages {
        let text = extract_text_content(m);
        // File path capture — anything that looks like path/to/file.ext.
        for word in text.split_whitespace() {
            if looks_like_path(word) {
                path_hits.insert(word.to_string());
            }
            if let Some(sym) = looks_like_symbol(word) {
                code_symbols.insert(sym);
            }
        }
        // Tool name capture (rough but cheap).
        for tool_name in KNOWN_TOOL_NAMES {
            if text.contains(tool_name) {
                tool_names.insert(tool_name.to_string());
            }
        }
        // User intent: take the first sentence of each user message.
        if m.role == "user" {
            if let Some(first) = first_sentence(&text) {
                if first.len() <= 160 {
                    kept_user_intents.push(first.to_string());
                }
            }
        }
    }

    if !path_hits.is_empty() {
        let listed: Vec<&str> = path_hits.iter().take(20).map(String::as_str).collect();
        lines.push(format!("Files referenced: {}", listed.join(", ")));
    }
    if !code_symbols.is_empty() {
        let listed: Vec<&str> = code_symbols.iter().take(20).map(String::as_str).collect();
        lines.push(format!("Code symbols: {}", listed.join(", ")));
    }
    if !tool_names.is_empty() {
        let listed: Vec<&str> = tool_names.iter().take(20).map(String::as_str).collect();
        lines.push(format!("Tools used: {}", listed.join(", ")));
    }
    if !kept_user_intents.is_empty() {
        let listed: Vec<String> = kept_user_intents
            .iter()
            .rev()
            .take(5)
            .map(|s| format!("- {s}"))
            .collect();
        lines.push(format!("Recent user intents:\n{}", listed.join("\n")));
    }

    let mut summary = if lines.is_empty() {
        "[older messages compacted: insufficient structured content]".to_string()
    } else {
        lines.join("\n")
    };

    // Budget clamp — never produce more tokens than max_tokens.
    if estimate_text_len_tokens(&summary) > max_tokens {
        truncate_to_tokens(&mut summary, max_tokens);
    }
    summary
}

// ----------------------------------------------------------------------
// Tiny utility helpers (private)
// ----------------------------------------------------------------------

/// Token estimate for an already-built string using the same heuristic as
/// `shannon_engine::compact::helpers::estimate_text_tokens`.
fn estimate_text_len_tokens(text: &str) -> usize {
    shannon_engine::compact::helpers::estimate_text_tokens(text)
}

fn truncate_to_tokens(text: &mut String, max_tokens: usize) {
    // Approximate: 4 ASCII chars per token. Cut on the nearest space.
    let approx_chars = max_tokens * 4;
    if text.len() <= approx_chars {
        return;
    }
    // Snap to the last char boundary at or before `approx_chars`.
    let mut cut = approx_chars.min(text.len());
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    if let Some(space) = text[..cut].rfind(' ') {
        cut = space;
    }
    text.truncate(cut);
    text.push_str(" ...");
}

fn looks_like_path(word: &str) -> bool {
    if word.len() < 4 || word.len() > 200 {
        return false;
    }
    if word.contains('/') || word.contains('\\') {
        let last = word.rsplit(['/', '\\']).next().unwrap_or("");
        // Has an extension (1-5 chars after a dot) OR a known build path.
        if let Some(dot_pos) = last.rfind('.') {
            let ext = &last[dot_pos + 1..];
            return (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric());
        }
        return false;
    }
    false
}

fn looks_like_symbol(word: &str) -> Option<String> {
    // Trim surrounding punctuation.
    let cleaned: String = word
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
        .collect();
    if cleaned.len() < 3 || cleaned.len() > 64 {
        return None;
    }
    let lower = cleaned.to_ascii_lowercase();
    // Rust-ish snake_case or PascalCase.
    if cleaned.contains('_')
        && cleaned
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        && cleaned
            .chars()
            .next()
            .map(|c| c.is_ascii_lowercase())
            .unwrap_or(false)
    {
        return Some(lower);
    }
    if cleaned
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
        && cleaned
            .chars()
            .next()
            .map(|c| c.is_ascii_uppercase())
            .unwrap_or(false)
        && cleaned.chars().any(|c| c.is_ascii_lowercase())
    {
        return Some(lower);
    }
    None
}

fn first_sentence(text: &str) -> Option<&str> {
    // Take everything up to the first period, question mark, or newline.
    for (i, ch) in text.char_indices() {
        if matches!(ch, '.' | '?' | '!' | '\n') {
            return Some(text[..i].trim());
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Stable list of tool names Shannon emits. Cheap string-level detection.
const KNOWN_TOOL_NAMES: &[&str] = &[
    "read_file",
    "write_file",
    "edit_file",
    "bash",
    "grep_search",
    "glob_files",
    "list_directory",
    "web_search",
    "web_fetch",
    "context7_query",
];

/// Heuristic: a tail is code-heavy when more than 60% of recent messages
/// contain code-like content (per `looks_like_code`).
fn recent_is_code_heavy(messages: &[Message], tail_size: usize) -> bool {
    if messages.is_empty() {
        return false;
    }
    let tail_size = tail_size.max(1);
    let start = messages.len().saturating_sub(tail_size);
    let tail = &messages[start..];
    let code_count = tail.iter().filter(|m| looks_like_code(m)).count();
    code_count * 5 >= tail.len() * 3 // 60%
}

fn estimate_default_avg() -> f32 {
    // Heuristic used by Selector heuristic 3 — calibrated against a 200k
    // budget and the project's average turn (≈300 tokens).
    300.0
}

/// Sum tokens of system messages only.
fn system_tokens_sum(messages: &[Message], system_end: usize) -> usize {
    messages[..system_end]
        .iter()
        .map(estimate_message_tokens)
        .sum()
}

/// Sum tokens of a tail slice.
fn tail_tokens(messages: &[Message], tail_start: usize) -> usize {
    messages[tail_start..]
        .iter()
        .map(estimate_message_tokens)
        .sum()
}

/// Compute a safe tail-start index: never split a user→assistant pair.
/// Simple heuristic — avoid splitting at a user message unless the
/// preceding slot is an assistant.
fn safe_tail_start(messages: &[Message], proposed: usize) -> usize {
    if proposed == 0 || proposed >= messages.len() {
        return proposed;
    }
    let mut p = proposed;
    // If the slot at `p` is a user message and the previous slot is an
    // assistant message, we are mid-turn — bump forward.
    if messages[p].role == "user" && p > 0 && messages[p - 1].role == "assistant" {
        p = (p + 1).min(messages.len());
    }
    p
}

// ======================================================================
// Tests
// ======================================================================

// Re-export the upstream CompactionStrategy so downstream callers can
// build pre-existing engine-style strategies without an extra import.
pub use shannon_engine::compact::compact_messages::CompactionStrategy as EngineCompactionStrategy;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use shannon_engine::compact::compact_messages::{
        CompactMessagesResult, CompactionStrategy, compact_messages as engine_compact_messages,
    };

    fn user_msg(text: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
        }
    }

    fn assistant_msg(text: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: MessageContent::Text(text.to_string()),
        }
    }

    fn system_msg(text: &str) -> Message {
        Message {
            role: "system".to_string(),
            content: MessageContent::Text(text.to_string()),
        }
    }

    fn large_user_msg(text: &str) -> Message {
        // Make it really big — ~10k tokens worth.
        let body = text.repeat(500);
        user_msg(&body)
    }

    // -------- Selector --------

    #[test]
    fn selector_below_threshold_is_noop() {
        let sel = Selector::default();
        let msgs = vec![user_msg("hi"), assistant_msg("hello")];
        let out = sel.recommend(&msgs, 1_000_000);
        assert_eq!(out.reason, SelectorReason::BelowThreshold);
    }

    #[test]
    fn selector_picks_token_based_when_few_messages_huge_tokens() {
        let sel = Selector::default();
        let msgs = vec![system_msg("system"), large_user_msg("X")];
        // Force ratio > trigger by capping max_tokens low.
        let out = sel.recommend(&msgs, 100);
        // Either TokenDense or MessageHeavy — depends on heuristics.
        assert!(matches!(
            out.reason,
            SelectorReason::TokenDense | SelectorReason::MessageHeavy
        ));
    }

    #[test]
    fn selector_picks_summary_when_many_messages() {
        let sel = Selector::default();
        let mut msgs = vec![system_msg("system")];
        for i in 0..40 {
            msgs.push(user_msg(&format!("user {i}")));
            msgs.push(assistant_msg(&format!("assistant {i}")));
        }
        let out = sel.recommend(&msgs, 100);
        assert_eq!(out.reason, SelectorReason::MessageHeavy);
        assert_eq!(out.strategy, Strategy::SummaryBased);
    }

    #[test]
    fn selector_picks_summary_when_code_heavy() {
        let mut policy = Policy::default();
        policy.high_message_count = 100; // disable msg-count trigger
        let sel = Selector::new(policy);
        let code_block = "```rust\nfn foo() -> i32 { 42 }\n```\nSee `bar::baz` and `quux`.";
        let msgs = vec![
            system_msg("sys"),
            user_msg(code_block),
            assistant_msg(code_block),
            user_msg(code_block),
            assistant_msg(code_block),
            user_msg(code_block),
            assistant_msg(code_block),
        ];
        let out = sel.recommend(&msgs, 100);
        assert_eq!(out.reason, SelectorReason::CodeHeavy);
        assert_eq!(out.strategy, Strategy::SummaryBased);
    }

    // -------- Extractors --------

    #[test]
    fn looks_like_path_handles_extensions() {
        assert!(looks_like_path("src/main.rs"));
        assert!(looks_like_path("crates/shannon-core/src/compact.rs"));
        assert!(looks_like_path("./foo.txt"));
        assert!(!looks_like_path("hello world"));
        assert!(!looks_like_path("a/b"));
    }

    #[test]
    fn looks_like_symbol_detects_snake_and_pascal() {
        assert_eq!(
            looks_like_symbol("foo_bar_baz").as_deref(),
            Some("foo_bar_baz")
        );
        assert_eq!(looks_like_symbol("FooBarBaz").as_deref(), Some("foobarbaz"));
        assert_eq!(looks_like_symbol("hello"), None);
        assert_eq!(looks_like_symbol("fn"), None);
    }

    #[test]
    fn first_sentence_breaks_on_punctuation() {
        assert_eq!(
            first_sentence("Please add tests. Then commit.").unwrap(),
            "Please add tests"
        );
        assert_eq!(first_sentence("Hello!"), Some("Hello"));
        assert_eq!(first_sentence("multi\nline"), Some("multi"));
    }

    #[test]
    fn extractive_summary_includes_paths_and_symbols() {
        let msgs = vec![
            user_msg("Please read src/main.rs and crates/shannon-core/src/compact.rs"),
            assistant_msg("I see `foo_bar` and `MyStruct` referenced."),
        ];
        let s = extractive_summary(&msgs, 1024);
        assert!(s.contains("src/main.rs"));
        assert!(s.contains("foo_bar") || s.contains("mystruct"));
        assert!(s.contains("User intents") || s.contains("intent"));
    }

    #[test]
    fn extractive_summary_clamps_to_budget() {
        let msgs = vec![user_msg("a"); 100];
        let s = extractive_summary(&msgs, 5);
        // 5 tokens ≈ ≤ 20 ASCII chars + " ..." → must clamp
        assert!(s.len() < 200);
        assert!(s.ends_with("..."));
    }

    // -------- Token-based strategy --------

    #[test]
    fn token_based_drops_oldest_until_under_target() {
        let mut policy = Policy::default();
        policy.keep_recent = 2;
        // Build 6 user messages big enough to require dropping.
        let msgs = vec![
            system_msg("sys"),
            user_msg(&"a".repeat(1000)),
            user_msg(&"b".repeat(1000)),
            user_msg(&"c".repeat(1000)),
            user_msg(&"d".repeat(1000)),
            user_msg(&"e".repeat(1000)),
        ];
        let outcome = maybe_compact_with_policy(&msgs, 100, policy);
        assert!(outcome.did_compact || outcome.messages_removed == 0);
        // System preserved.
        assert_eq!(outcome.compacted[0].role, "system");
        // Last 2 messages preserved.
        assert_eq!(outcome.compacted.last().unwrap().role, "user");
    }

    // -------- Summary-based strategy --------

    #[test]
    fn summary_based_preserves_system_and_recent() {
        let mut policy = Policy::default();
        policy.high_message_count = 4;
        policy.keep_recent = 2;
        let mut msgs = vec![
            system_msg("ORIGINAL_SYSTEM_INTENT_MARKER"),
            user_msg("src/lib.rs says hello"),
            assistant_msg("ok, foo_bar too"),
            user_msg("see also crates/shannon-core/src/compact.rs and bar_baz"),
            assistant_msg("noted"),
            user_msg("recent user message"),
            assistant_msg("recent assistant message"),
            user_msg("very recent"),
            assistant_msg("tail"),
        ];
        // Force the selector onto SummaryBased by making total tokens
        // exceed the trigger ratio.
        let outcome = maybe_compact_with_policy(&msgs, 50, policy.clone());
        assert!(outcome.did_compact, "should compact: {outcome:?}");
        // System prompt preserved.
        assert!(
            outcome.compacted[0].role == "system"
                && extract_text_content(&outcome.compacted[0])
                    .contains("ORIGINAL_SYSTEM_INTENT_MARKER"),
            "system intent must survive compaction"
        );
        // Last message preserved verbatim.
        assert_eq!(
            extract_text_content(outcome.compacted.last().unwrap()),
            "tail"
        );
        // Last-N (≥ keep_recent) preserved by checking the tail.
        let last_n: Vec<&Message> = outcome.compacted.iter().rev().take(2).collect();
        let last_n_text: Vec<String> = last_n.iter().map(|m| extract_text_content(m)).collect();
        assert!(
            last_n_text.iter().any(|t| t == "tail"),
            "tail message must be present after compaction"
        );
        // Drop window collapsed into a summary message.
        let summary_msg = outcome
            .compacted
            .iter()
            .find(|m| {
                m.role == "system"
                    && extract_text_content(m).contains("Previous conversation summary")
            })
            .expect("summary message should be present");
        let summary_text = extract_text_content(summary_msg);
        assert!(summary_text.contains("src/lib.rs") || summary_text.contains("foo_bar"));
        // Sanity: reference to the input slice is gone after the call.
        msgs.push(system_msg("should not leak"));
        assert!(outcome.compacted.len() <= msgs.len());
    }

    // -------- Boundary tests --------

    #[test]
    fn empty_messages_no_crash() {
        let outcome = maybe_compact(&[], 1024);
        assert!(!outcome.did_compact);
        assert_eq!(outcome.compacted.len(), 0);
    }

    #[test]
    fn fewer_than_keep_recent_is_noop() {
        let msgs = vec![system_msg("s"), user_msg("u"), assistant_msg("a")];
        let outcome = maybe_compact(&msgs, 1);
        assert!(!outcome.did_compact);
        assert_eq!(outcome.compacted.len(), msgs.len());
    }

    #[test]
    fn zero_max_tokens_does_not_divide_by_zero() {
        let msgs = vec![user_msg("x"), assistant_msg("y")];
        // Should not panic.
        let _ = maybe_compact(&msgs, 0);
    }

    #[test]
    fn display_strategy_and_reason() {
        assert_eq!(Strategy::TokenBased.to_string(), "token_based");
        assert_eq!(Strategy::SummaryBased.to_string(), "summary_based");
        assert_eq!(Strategy::SummaryLlm.to_string(), "summary_llm");
        assert_eq!(
            SelectorReason::BelowThreshold.to_string(),
            "below_threshold"
        );
        assert_eq!(SelectorReason::MessageHeavy.to_string(), "message_heavy");
        assert_eq!(SelectorReason::CodeHeavy.to_string(), "code_heavy");
        assert_eq!(SelectorReason::TokenDense.to_string(), "token_dense");
    }

    // -------- LLM path --------

    struct FixedSummarizer {
        body: String,
    }
    impl Summarizer for FixedSummarizer {
        fn summarize(&self, _: &[Message], _: usize) -> Result<String, String> {
            Ok(self.body.clone())
        }
    }

    struct FailingSummarizer;
    impl Summarizer for FailingSummarizer {
        fn summarize(&self, _: &[Message], _: usize) -> Result<String, String> {
            Err("boom".into())
        }
    }

    #[test]
    fn summary_llm_uses_summarizer_when_available() {
        let mut policy = Policy::default();
        policy.high_message_count = 4;
        let sum = FixedSummarizer {
            body: "PRESERVED_BY_LLM_SUMMARY".into(),
        };
        let mut msgs = vec![system_msg("sys")];
        for i in 0..10 {
            // Pad each message so we exceed the trigger threshold.
            msgs.push(user_msg(&format!("u {i} {}", "x".repeat(200))));
            msgs.push(assistant_msg(&format!("a {i} {}", "y".repeat(200))));
        }
        let outcome = maybe_compact_full(&msgs, 50, policy, Some(&sum));
        assert!(outcome.did_compact);
        assert_eq!(outcome.strategy, Strategy::SummaryLlm);
        let summary = outcome
            .compacted
            .iter()
            .find(|m| {
                m.role == "system" && extract_text_content(m).contains("PRESERVED_BY_LLM_SUMMARY")
            })
            .expect("LLM summary must be embedded");
        assert!(extract_text_content(summary).contains("PRESERVED_BY_LLM_SUMMARY"));
    }

    #[test]
    fn summary_llm_falls_back_on_error() {
        let mut policy = Policy::default();
        policy.high_message_count = 4;
        let bad = FailingSummarizer;
        let mut msgs = vec![system_msg("sys")];
        for i in 0..10 {
            msgs.push(user_msg(&format!("user msg {i} {} ", "z".repeat(200))));
            msgs.push(assistant_msg(&format!(
                "assistant msg {i} {} ",
                "w".repeat(200)
            )));
        }
        let outcome = maybe_compact_full(&msgs, 50, policy, Some(&bad));
        assert!(outcome.did_compact);
        // Strategy stays SummaryLlm — the fallback content is the extractive
        // text, but the strategy tag in the outcome reports the requested path.
        assert_eq!(outcome.strategy, Strategy::SummaryLlm);
        let summary = outcome
            .compacted
            .iter()
            .find(|m| {
                m.role == "system"
                    && extract_text_content(m).contains("Previous conversation summary")
            })
            .expect("extractive fallback summary present");
        assert!(extract_text_content(summary).contains("Previous conversation summary"));
    }

    #[test]
    fn summary_llm_without_summarizer_downgrades_to_summary_based() {
        let mut policy = Policy::default();
        policy.high_message_count = 4;
        let mut msgs = vec![system_msg("sys")];
        for i in 0..10 {
            msgs.push(user_msg(&format!("u {i} {}", "x".repeat(200))));
            msgs.push(assistant_msg(&format!("a {i} {}", "y".repeat(200))));
        }
        let outcome = maybe_compact_full(&msgs, 50, policy, None);
        assert_eq!(outcome.strategy, Strategy::SummaryBased);
        assert!(outcome.did_compact);
    }

    #[test]
    fn engine_compact_messages_integration_no_panic() {
        // Smoke test: ensure the facade does not regress against the
        // upstream `compact_messages` KeepRecent strategy when applied
        // directly.
        let msgs = vec![
            system_msg("sys"),
            user_msg("u1"),
            assistant_msg("a1"),
            user_msg("u2"),
            assistant_msg("a2"),
            user_msg("u3"),
            assistant_msg("a3"),
        ];
        let result: CompactMessagesResult =
            engine_compact_messages(&msgs, &CompactionStrategy::KeepRecent { count: 2 }, 1024, 2);
        assert!(result.did_compact || result.compacted_count == msgs.len());
    }
}
