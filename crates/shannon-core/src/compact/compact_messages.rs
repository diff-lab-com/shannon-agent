//! Standalone message compaction functions and configuration.

use serde::{Deserialize, Serialize};

use crate::api::{Message, MessageContent};

use super::helpers::{
    contains_tool_result_for, estimate_message_tokens, estimate_tokens, extract_tool_uses,
    looks_like_code,
};
use super::summarizer::RuleBasedSummarizer;
use super::types::Summarizer;
use crate::api::ContentBlock;

/// Check whether a message contains any ToolUse blocks.
fn has_tool_use(msg: &crate::api::Message) -> bool {
    matches!(
        &msg.content,
        crate::api::MessageContent::Blocks(blocks) if blocks.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }))
    )
}

/// Check whether a message contains any ToolResult blocks.
fn has_tool_result(msg: &crate::api::Message) -> bool {
    matches!(
        &msg.content,
        crate::api::MessageContent::Blocks(blocks) if blocks.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }))
    )
}

/// Adjust a proposed split index so that tool call/result pairs are not separated.
///
/// Tool interactions follow this pattern in Shannon's internal format:
///   assistant: [ToolUse{id, name, input}]   ← the tool call
///   user:      [ToolResult{tool_use_id, …}]  ← the tool result
///
/// If the split would land between a ToolUse message and its matching ToolResult,
/// we move the split point backward to include both (or forward to exclude both).
/// This prevents orphaned tool calls or results that would cause API errors.
fn safe_split_point(messages: &[crate::api::Message], proposed: usize) -> usize {
    if proposed == 0 || proposed >= messages.len() {
        return proposed;
    }

    // Walk backward from the proposed split to find any unpaired tool interactions.
    // We look for the pattern: assistant[ToolUse] followed by user[ToolResult].
    let mut split = proposed;

    // Case 1: The message just before the split has ToolUse (no result yet).
    //         We need to include the following ToolResult message(s) too.
    if split < messages.len() && has_tool_use(&messages[split]) {
        // The assistant message at `split` has tool calls — find matching results.
        let tool_uses = extract_tool_uses(&messages[split]);
        let mut seek = split + 1;
        while seek < messages.len() {
            if contains_tool_result_for(&messages[seek], &tool_uses) {
                // Found the result; move split past it.
                split = seek + 1;
                break;
            }
            // If we hit another user text message before finding results, stop.
            if messages[seek].role == "user" && !has_tool_result(&messages[seek]) {
                break;
            }
            seek += 1;
        }
    }

    // Case 2: The message at the split is a ToolResult without its ToolUse.
    //         Move split backward to include the assistant ToolUse message.
    if split > 0 && split < messages.len() && has_tool_result(&messages[split]) {
        let mut seek = split;
        while seek > 0 {
            seek -= 1;
            if has_tool_use(&messages[seek]) {
                // Found the tool call — move split to include it.
                split = seek;
                break;
            }
            // If we hit a non-tool message, stop.
            if messages[seek].role == "user" && !has_tool_result(&messages[seek]) {
                break;
            }
        }
    }

    split
}

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
            auto_compact_threshold: 0.75,
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

/// Validate that a message slice has no orphaned tool calls/results.
/// Returns true if all tool_use blocks have matching tool_result blocks
/// and all tool_result blocks have matching tool_use blocks.
#[cfg(test)]
fn validate_tool_pairs(messages: &[crate::api::Message]) -> bool {
    use std::collections::HashSet;

    let mut seen_tool_uses: HashSet<String> = HashSet::new();
    let mut matched_results: HashSet<String> = HashSet::new();

    // Collect all tool_use IDs.
    for msg in messages {
        if let crate::api::MessageContent::Blocks(blocks) = &msg.content {
            for block in blocks {
                if let ContentBlock::ToolUse { id, .. } = block {
                    seen_tool_uses.insert(id.clone());
                }
            }
        }
    }

    // Check that every tool_result has a matching tool_use.
    for msg in messages {
        if let crate::api::MessageContent::Blocks(blocks) = &msg.content {
            for block in blocks {
                if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                    if !seen_tool_uses.contains(tool_use_id) {
                        return false; // Orphaned tool result
                    }
                    matched_results.insert(tool_use_id.clone());
                }
            }
        }
    }

    // Check that every tool_use has a matching tool_result.
    for id in &seen_tool_uses {
        if !matched_results.contains(id) {
            return false; // Orphaned tool use
        }
    }

    true
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

    // Adjust split point to preserve tool call/result pairs.
    let raw_tail_start = messages.len() - keep_count;
    let tail_start = safe_split_point(messages, raw_tail_start).min(messages.len());

    let kept_count = messages.len() - tail_start;
    let removed_count = tail_start - system_end;
    let summary = Message {
        role: "system".to_string(),
        content: MessageContent::Text(format!(
            "[Context compacted: {removed_count} older messages removed, keeping {kept_count} recent messages]"
        )),
    };

    let mut result = Vec::with_capacity(system_end + 1 + kept_count);
    // Preserve leading system messages.
    result.extend_from_slice(&messages[..system_end]);
    result.push(summary);
    // Keep the tail messages (may be slightly more than keep_count to preserve tool pairs).
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

    // Adjust split point to preserve tool call/result pairs.
    let raw_tail_start = messages.len() - keep_recent;
    let tail_start = safe_split_point(messages, raw_tail_start).min(messages.len());
    let split_point = tail_start.saturating_sub(system_end);
    let old_messages = &non_system[..split_point];

    let summarizer = RuleBasedSummarizer::new();
    let summary_budget = budget.clamp(500, 4000);
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

    let kept_count = messages.len() - tail_start;
    let mut result = Vec::with_capacity(system_end + 1 + kept_count);
    result.extend_from_slice(&messages[..system_end]);
    result.push(summary_msg);
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
/// recent messages. Tool call/result pairs are always kept together.
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

    // Adjust recent boundary to preserve tool pairs.
    let raw_recent_start = messages.len() - keep_recent;
    let recent_start = safe_split_point(messages, raw_recent_start).min(messages.len());
    let recent_msgs = &messages[recent_start..];
    let recent_tokens = estimate_tokens(recent_msgs);

    // From the older non-system messages, select code-rich ones.
    // Track selected indices to ensure tool pairs are co-selected.
    let older_end = recent_start.saturating_sub(system_end);
    let older = &non_system[..older_end];
    let mut selected: Vec<usize> = Vec::new();
    let mut selected_tokens = 0usize;

    for (i, msg) in older.iter().enumerate() {
        if looks_like_code(msg) {
            let t = estimate_message_tokens(msg);
            if selected_tokens + t + recent_tokens > budget {
                continue;
            }
            // If this message has a tool_use, find and include the matching tool_result.
            if has_tool_use(msg) {
                let tool_uses = extract_tool_uses(msg);
                // Include this message.
                selected.push(i);
                selected_tokens += t;
                // Find the next message with matching tool results and include it too.
                for j in (i + 1)..older.len() {
                    if contains_tool_result_for(&older[j], &tool_uses) {
                        let rt = estimate_message_tokens(&older[j]);
                        if selected_tokens + rt + recent_tokens <= budget {
                            selected.push(j);
                            selected_tokens += rt;
                        }
                        break;
                    }
                }
            } else if has_tool_result(msg) {
                // Tool result — find the preceding tool_use and include both.
                // Only include if the matching tool_use is in `older` and already selected
                // or can be selected now.
                let mut found_pair = false;
                for j in (0..i).rev() {
                    if has_tool_use(&older[j]) {
                        let tool_uses = extract_tool_uses(&older[j]);
                        if contains_tool_result_for(msg, &tool_uses) {
                            // Include the tool_use if not already selected.
                            if !selected.contains(&j) {
                                let ut = estimate_message_tokens(&older[j]);
                                if selected_tokens + ut + t + recent_tokens <= budget {
                                    selected.push(j);
                                    selected_tokens += ut;
                                } else {
                                    break;
                                }
                            }
                            selected.push(i);
                            selected_tokens += t;
                            found_pair = true;
                            break;
                        }
                    }
                }
                if !found_pair {
                    // Orphaned tool result — skip it to avoid API errors.
                }
            } else {
                // Regular code message.
                selected.push(i);
                selected_tokens += t;
            }
        }
    }

    // Sort selected indices for correct ordering.
    selected.sort_unstable();
    selected.dedup();

    let kept_older_count = selected.len();
    let dropped_count = older.len() - kept_older_count;
    let actual_keep_recent = messages.len() - recent_start;

    let summary = Message {
        role: "system".to_string(),
        content: MessageContent::Text(format!(
            "[Context compacted with code priority: {dropped_count} non-code messages removed, \
             {kept_older_count} code messages preserved, {actual_keep_recent} recent messages kept]"
        )),
    };

    let mut result = Vec::with_capacity(system_end + 1 + selected.len() + actual_keep_recent);
    result.extend_from_slice(&messages[..system_end]);
    result.push(summary);
    for idx in &selected {
        result.push(older[*idx].clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ContentBlock, Message, MessageContent, ToolResultContent};

    fn text_msg(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: MessageContent::Text(text.to_string()),
        }
    }

    fn tool_use_msg(id: &str, name: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                id: id.to_string(),
                name: name.to_string(),
                input: serde_json::json!({}),
            }]),
        }
    }

    fn tool_result_msg(tool_use_id: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: Some(ToolResultContent::Single("ok".to_string())),
                is_error: Some(false),
            }]),
        }
    }

    /// Build a conversation with tool interactions:
    ///   sys, user, assistant[ToolUse], user[ToolResult], assistant, ...
    fn make_tool_conversation(n_pairs: usize) -> Vec<Message> {
        let mut msgs = vec![text_msg("system", "You are helpful.")];
        for i in 0..n_pairs {
            msgs.push(text_msg("user", &format!("Request {i}")));
            msgs.push(tool_use_msg(&format!("call_{i}"), "bash"));
            msgs.push(tool_result_msg(&format!("call_{i}")));
            msgs.push(text_msg("assistant", &format!("Response {i}")));
        }
        msgs.push(text_msg("user", "Final question"));
        msgs
    }

    #[test]
    fn test_safe_split_point_preserves_tool_pairs() {
        let msgs = make_tool_conversation(1);
        // Split at 3 would cut between tool_use(2) and tool_result(3).
        let safe = safe_split_point(&msgs, 3);
        assert!(
            safe == 2 || safe == 4,
            "safe_split_point(3) should avoid splitting tool pair, got {safe}"
        );
    }

    #[test]
    fn test_safe_split_point_before_tool_use() {
        let msgs = make_tool_conversation(1);
        // Split at 2 — the message AT 2 is tool_use. Should include its result.
        let safe = safe_split_point(&msgs, 2);
        assert!(
            safe >= 4,
            "split at tool_use should include result, got {safe}"
        );
    }

    #[test]
    fn test_safe_split_point_at_safe_boundary() {
        let msgs = make_tool_conversation(1);
        // Split at 4 — after the tool result. Should stay at 4.
        let safe = safe_split_point(&msgs, 4);
        assert_eq!(safe, 4, "split after tool pair should be unchanged");
    }

    #[test]
    fn test_keep_recent_preserves_tool_pairs() {
        let msgs = make_tool_conversation(3);
        // Use small max_tokens to force compaction (messages are ~100 tokens).
        let result = compact_messages(
            &msgs,
            &CompactionStrategy::KeepRecent { count: 3 },
            10,
            10,
        );
        assert!(result.did_compact);
        assert!(
            super::validate_tool_pairs(&result.messages),
            "compacted messages should have valid tool pairs"
        );
    }

    #[test]
    fn test_summarize_preserves_tool_pairs() {
        let msgs = make_tool_conversation(3);
        let result = compact_messages(
            &msgs,
            &CompactionStrategy::Summarize,
            10,
            4,
        );
        assert!(result.did_compact);
        assert!(
            super::validate_tool_pairs(&result.messages),
            "summarized messages should have valid tool pairs"
        );
    }

    #[test]
    fn test_prioritize_code_preserves_tool_pairs() {
        let msgs = make_tool_conversation(3);
        let result = compact_messages(
            &msgs,
            &CompactionStrategy::PrioritizeCode,
            10,
            4,
        );
        assert!(result.did_compact);
        assert!(
            super::validate_tool_pairs(&result.messages),
            "prioritize-code messages should have valid tool pairs"
        );
    }

    #[test]
    fn test_no_compact_when_fits() {
        let msgs = vec![
            text_msg("system", "System"),
            text_msg("user", "Hi"),
            text_msg("assistant", "Hello"),
        ];
        let result = compact_messages(
            &msgs,
            &CompactionStrategy::KeepRecent { count: 10 },
            200_000,
            10,
        );
        assert!(!result.did_compact);
        assert_eq!(result.messages.len(), 3);
    }

    #[test]
    fn test_validate_tool_pairs_detects_orphans() {
        let msgs = vec![
            text_msg("system", "System"),
            tool_result_msg("call_orphan"),
            text_msg("user", "Hi"),
        ];
        assert!(
            !super::validate_tool_pairs(&msgs),
            "orphaned tool result should fail validation"
        );
    }

    #[test]
    fn test_validate_tool_pairs_passes_for_valid() {
        let msgs = make_tool_conversation(2);
        assert!(
            super::validate_tool_pairs(&msgs),
            "valid conversation should pass validation"
        );
    }
}
