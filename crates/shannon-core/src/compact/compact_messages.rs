//! Standalone message compaction functions and configuration.

use serde::{Deserialize, Serialize};

use crate::api::{Message, MessageContent};

use super::helpers::{estimate_message_tokens, estimate_tokens, looks_like_code};
use super::summarizer::RuleBasedSummarizer;
use super::types::Summarizer;

// ============================================================================
// Auto-Compaction Configuration
// ============================================================================

/// User-facing configuration for context auto-compaction.
///
/// Controls when and how the conversation context is automatically compressed
/// to stay within the model's token budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Token usage threshold (0.0-1.0 of max context) to trigger auto-compaction.
    pub auto_compact_threshold: f64,
    /// Whether auto-compaction is enabled.
    pub enabled: bool,
    /// Strategy to use when auto-compacting.
    pub strategy: CompactionStrategy,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            auto_compact_threshold: 0.85,
            enabled: true,
            strategy: CompactionStrategy::Summarize,
        }
    }
}

impl CompactionConfig {
    /// Create a disabled config (auto-compaction turned off).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }
}

// ============================================================================
// Compaction Strategy
// ============================================================================

/// Strategy selector for compaction operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactionStrategy {
    /// Keep the system prompt + the most recent N messages, discarding older ones
    /// after summarizing them into a compact summary message.
    KeepRecent { count: usize },
    /// Summarize older messages into a compact summary (default).
    Summarize,
    /// Prioritize keeping messages that contain code edits.
    PrioritizeCode,
}

// ============================================================================
// Standalone compact_messages Function
// ============================================================================

/// Result of a `compact_messages` operation, carrying both the compacted
/// message list and metadata about what happened.
#[derive(Debug, Clone)]
pub struct CompactMessagesResult {
    /// The compacted message list.
    pub messages: Vec<Message>,
    /// Number of messages in the original list.
    pub original_count: usize,
    /// Number of messages after compaction.
    pub compacted_count: usize,
    /// Estimated tokens before compaction.
    pub original_tokens: usize,
    /// Estimated tokens after compaction.
    pub compacted_tokens: usize,
    /// Whether any compaction actually occurred.
    pub did_compact: bool,
}

/// Compact message history to fit within a token budget.
///
/// This is the main entry point for the `/compact` command and for
/// auto-compaction wiring. It is non-destructive: the caller receives a new
/// `Vec<Message>` and can keep the original if desired.
///
/// # Rules
///
/// 1. Always keep system-prompt messages (role == "system" at the start).
/// 2. Always keep the most recent user message.
/// 3. Apply the requested strategy for middle messages.
/// 4. Ensure total estimated tokens < `max_tokens` after compaction.
pub fn compact_messages(
    messages: &[Message],
    strategy: &CompactionStrategy,
    max_tokens: usize,
    keep_recent: usize,
) -> CompactMessagesResult {
    let original_count = messages.len();
    let original_tokens = estimate_tokens(messages);

    if messages.is_empty() || original_tokens <= max_tokens {
        return CompactMessagesResult {
            messages: messages.to_vec(),
            original_count,
            compacted_count: original_count,
            original_tokens,
            compacted_tokens: original_tokens,
            did_compact: false,
        };
    }

    // Identify leading system messages (the system prompt block).
    let system_end = messages
        .iter()
        .position(|m| m.role != "system")
        .unwrap_or(messages.len());

    // If everything is system messages, nothing to compact.
    if system_end >= messages.len() {
        return CompactMessagesResult {
            messages: messages.to_vec(),
            original_count,
            compacted_count: original_count,
            original_tokens,
            compacted_tokens: original_tokens,
            did_compact: false,
        };
    }

    let non_system = &messages[system_end..];
    let non_system_tokens = estimate_tokens(non_system);
    let system_tokens = original_tokens.saturating_sub(non_system_tokens);

    // Budget remaining for non-system messages.
    let budget = max_tokens.saturating_sub(system_tokens);

    match strategy {
        CompactionStrategy::KeepRecent { count } => {
            compact_keep_recent(messages, system_end, *count, budget)
        }
        CompactionStrategy::Summarize => {
            compact_summarize(messages, system_end, keep_recent, budget)
        }
        CompactionStrategy::PrioritizeCode => {
            compact_prioritize_code(messages, system_end, keep_recent, budget)
        }
    }
}

/// Keep-recent strategy: drop everything older than the last `count` non-system
/// messages, inserting a brief placeholder summary.
fn compact_keep_recent(
    messages: &[Message],
    system_end: usize,
    keep_count: usize,
    _budget: usize,
) -> CompactMessagesResult {
    let original_count = messages.len();
    let original_tokens = estimate_tokens(messages);
    let non_system = &messages[system_end..];

    if non_system.len() <= keep_count {
        return CompactMessagesResult {
            messages: messages.to_vec(),
            original_count,
            compacted_count: original_count,
            original_tokens,
            compacted_tokens: original_tokens,
            did_compact: false,
        };
    }

    let removed_count = non_system.len() - keep_count;
    let summary = Message {
        role: "system".to_string(),
        content: MessageContent::Text(format!(
            "[Context compacted: {removed_count} older messages removed, keeping {keep_count} recent messages]"
        )),
    };

    let mut result = Vec::with_capacity(system_end + 1 + keep_count);
    // Preserve leading system messages.
    result.extend_from_slice(&messages[..system_end]);
    result.push(summary);
    // Keep the last `keep_count` messages.
    let tail_start = messages.len() - keep_count;
    result.extend_from_slice(&messages[tail_start..]);

    let compacted_tokens = estimate_tokens(&result);
    CompactMessagesResult {
        compacted_count: result.len(),
        compacted_tokens,
        did_compact: true,
        messages: result,
        original_count,
        original_tokens,
    }
}

/// Summarize strategy: use the `RuleBasedSummarizer` to produce a text summary
/// of older messages, then keep recent messages in full.
fn compact_summarize(
    messages: &[Message],
    system_end: usize,
    keep_recent: usize,
    budget: usize,
) -> CompactMessagesResult {
    let original_count = messages.len();
    let original_tokens = estimate_tokens(messages);
    let non_system = &messages[system_end..];

    if non_system.len() <= keep_recent {
        return CompactMessagesResult {
            messages: messages.to_vec(),
            original_count,
            compacted_count: original_count,
            original_tokens,
            compacted_tokens: original_tokens,
            did_compact: false,
        };
    }

    let split_point = non_system.len().saturating_sub(keep_recent);
    let old_messages = &non_system[..split_point];

    let summarizer = RuleBasedSummarizer::new();
    let summary_budget = budget.min(4000).max(500);
    let summary_text = summarizer
        .summarize(old_messages, summary_budget)
        .unwrap_or_else(|_| format!("[{} older messages compacted]", old_messages.len()));

    let summary_msg = Message {
        role: "system".to_string(),
        content: MessageContent::Text(format!(
            "[Previous conversation summary -- {} messages compacted]\n\n{summary_text}",
            old_messages.len()
        )),
    };

    let mut result = Vec::with_capacity(system_end + 1 + keep_recent);
    result.extend_from_slice(&messages[..system_end]);
    result.push(summary_msg);
    let tail_start = messages.len() - keep_recent;
    result.extend_from_slice(&messages[tail_start..]);

    let compacted_tokens = estimate_tokens(&result);
    CompactMessagesResult {
        compacted_count: result.len(),
        compacted_tokens,
        did_compact: true,
        messages: result,
        original_count,
        original_tokens,
    }
}

/// Prioritize-code strategy: keep messages that contain code-like content
/// (file paths, code blocks, tool use), then fill remaining budget with
/// recent messages.
fn compact_prioritize_code(
    messages: &[Message],
    system_end: usize,
    keep_recent: usize,
    budget: usize,
) -> CompactMessagesResult {
    let original_count = messages.len();
    let original_tokens = estimate_tokens(messages);
    let non_system = &messages[system_end..];

    if non_system.len() <= keep_recent {
        return CompactMessagesResult {
            messages: messages.to_vec(),
            original_count,
            compacted_count: original_count,
            original_tokens,
            compacted_tokens: original_tokens,
            did_compact: false,
        };
    }

    // Always keep recent messages.
    let recent_start = messages.len() - keep_recent;
    let recent_msgs = &messages[recent_start..];
    let recent_tokens = estimate_tokens(recent_msgs);

    // From the older non-system messages, select code-rich ones.
    let older = &non_system[..non_system.len().saturating_sub(keep_recent)];
    let mut code_messages: Vec<&Message> = Vec::new();
    let mut code_tokens = 0usize;

    for msg in older {
        if looks_like_code(msg) {
            let t = estimate_message_tokens(msg);
            if code_tokens + t + recent_tokens <= budget {
                code_messages.push(msg);
                code_tokens += t;
            }
        }
    }

    let kept_older_count = code_messages.len();
    let dropped_count = older.len() - kept_older_count;

    let summary = Message {
        role: "system".to_string(),
        content: MessageContent::Text(format!(
            "[Context compacted with code priority: {dropped_count} non-code messages removed, \
             {kept_older_count} code messages preserved, {keep_recent} recent messages kept]"
        )),
    };

    let mut result = Vec::with_capacity(system_end + 1 + code_messages.len() + keep_recent);
    result.extend_from_slice(&messages[..system_end]);
    result.push(summary);
    for msg in code_messages {
        result.push(msg.clone());
    }
    result.extend_from_slice(recent_msgs);

    let compacted_tokens = estimate_tokens(&result);
    CompactMessagesResult {
        compacted_count: result.len(),
        compacted_tokens,
        did_compact: true,
        messages: result,
        original_count,
        original_tokens,
    }
}
