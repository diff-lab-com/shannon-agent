//! Message protection and priority classification for compaction decisions.

use std::collections::HashSet;

use crate::api::{Message, MessageContent};

use super::compact_messages::{CompactMessagesResult, CompactionStrategy, compact_messages};
use super::helpers::{
    estimate_message_tokens, estimate_tokens, extract_text_content, looks_like_code,
};

// ============================================================================
// Message Protection
// ============================================================================

/// Tracks message indices that should be protected from compaction.
///
/// Users can mark specific messages as "important" to prevent them from
/// being summarized or dropped during context compression.
#[derive(Debug, Clone, Default)]
pub struct MessageProtector {
    /// Indices of messages that must never be compacted.
    protected: HashSet<usize>,
}

impl MessageProtector {
    /// Create a new empty protector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark a message index as protected.
    pub fn protect(&mut self, index: usize) {
        self.protected.insert(index);
    }

    /// Remove protection from a message index.
    pub fn unprotect(&mut self, index: usize) {
        self.protected.remove(&index);
    }

    /// Check if a message index is protected.
    pub fn is_protected(&self, index: usize) -> bool {
        self.protected.contains(&index)
    }

    /// Return the number of protected messages.
    pub fn protected_count(&self) -> usize {
        self.protected.len()
    }

    /// Return all protected indices.
    pub fn protected_indices(&self) -> &HashSet<usize> {
        &self.protected
    }

    /// Clear all protections.
    pub fn clear(&mut self) {
        self.protected.clear();
    }
}

// ============================================================================
// Priority Classification
// ============================================================================

/// Classify a message's priority for compaction decisions.
///
/// Priority is based on role and content:
/// - **Critical**: system messages, user instructions (never compact)
/// - **High**: assistant responses with code, tool use/results (compact last)
/// - **Normal**: regular user/assistant messages
/// - **Low**: verbose tool output, very long messages (compact first)
pub fn classify_message_priority(msg: &Message) -> crate::context_budget::MessagePriority {
    use crate::context_budget::MessagePriority;

    match msg.role.as_str() {
        "system" => MessagePriority::Critical,
        "user" => {
            let text = extract_text_content(msg);
            // Short user instructions are critical
            if text.len() < 200 {
                return MessagePriority::Critical;
            }
            MessagePriority::Normal
        }
        "assistant" => {
            if looks_like_code(msg) {
                MessagePriority::High
            } else {
                MessagePriority::Normal
            }
        }
        _ => MessagePriority::Low,
    }
}

/// Compact messages while respecting protected indices and message priorities.
///
/// This is a priority-aware version of [`compact_messages`] that:
/// 1. Never removes protected messages
/// 2. Compacts low-priority messages first, then normal, then high
/// 3. Never touches critical messages
pub fn compact_messages_with_protection(
    messages: &[Message],
    strategy: &CompactionStrategy,
    max_tokens: usize,
    keep_recent: usize,
    protector: &MessageProtector,
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

    // If no protected messages, fall back to standard compaction
    if protector.protected_count() == 0 {
        return compact_messages(messages, strategy, max_tokens, keep_recent);
    }

    // Identify system messages (always preserved)
    let system_end = messages
        .iter()
        .position(|m| m.role != "system")
        .unwrap_or(messages.len());

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
    let budget = max_tokens.saturating_sub(system_tokens);

    // Classify non-system messages by priority
    let mut low_indices: Vec<usize> = Vec::new();
    let mut normal_indices: Vec<usize> = Vec::new();
    let mut high_indices: Vec<usize> = Vec::new();
    // critical_indices and protected are always kept

    for (i, msg) in non_system.iter().enumerate() {
        let abs_idx = system_end + i;
        if protector.is_protected(abs_idx) {
            continue; // Always keep protected
        }
        let priority = classify_message_priority(msg);
        if priority == crate::context_budget::MessagePriority::Critical {
            continue; // Always keep critical
        }
        match priority {
            crate::context_budget::MessagePriority::Low => low_indices.push(i),
            crate::context_budget::MessagePriority::Normal => normal_indices.push(i),
            crate::context_budget::MessagePriority::High => high_indices.push(i),
            _ => {}
        }
    }

    // Calculate how many tokens we need to shed
    let excess = non_system_tokens.saturating_sub(budget);
    if excess == 0 {
        return CompactMessagesResult {
            messages: messages.to_vec(),
            original_count,
            compacted_count: original_count,
            original_tokens,
            compacted_tokens: original_tokens,
            did_compact: false,
        };
    }

    // Evict in priority order: Low -> Normal -> High
    let mut to_remove: HashSet<usize> = HashSet::new();
    let mut tokens_shed = 0usize;

    for idx_list in [&low_indices, &normal_indices, &high_indices] {
        for &rel_idx in idx_list {
            if tokens_shed >= excess {
                break;
            }
            let abs_idx = system_end + rel_idx;
            if protector.is_protected(abs_idx) {
                continue;
            }
            let msg_tokens = estimate_message_tokens(&messages[abs_idx]);
            to_remove.insert(abs_idx);
            tokens_shed += msg_tokens;
        }
        if tokens_shed >= excess {
            break;
        }
    }

    if to_remove.is_empty() {
        return CompactMessagesResult {
            messages: messages.to_vec(),
            original_count,
            compacted_count: original_count,
            original_tokens,
            compacted_tokens: original_tokens,
            did_compact: false,
        };
    }

    // Build result: keep system + summary + kept messages
    let removed_count = to_remove.len();
    let summary = Message {
        role: "system".to_string(),
        content: MessageContent::Text(format!(
            "[Priority-aware compaction: {removed_count} messages removed (low/normal priority first), \
             {} protected messages preserved]",
            protector.protected_count()
        )),
    };

    let mut result = Vec::with_capacity(system_end + 1 + messages.len() - removed_count);
    result.extend_from_slice(&messages[..system_end]);
    result.push(summary);
    for (i, msg) in messages.iter().enumerate() {
        if i >= system_end && !to_remove.contains(&i) {
            result.push(msg.clone());
        }
    }

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

    // ── MessageProtector ─────────────────────────────────────────────────

    #[test]
    fn test_protector_new_is_empty() {
        let p = MessageProtector::new();
        assert_eq!(p.protected_count(), 0);
        assert!(!p.is_protected(0));
        assert!(p.protected_indices().is_empty());
    }

    #[test]
    fn test_protector_protect_and_check() {
        let mut p = MessageProtector::new();
        p.protect(3);
        p.protect(7);
        assert!(p.is_protected(3));
        assert!(p.is_protected(7));
        assert!(!p.is_protected(0));
        assert!(!p.is_protected(4));
        assert_eq!(p.protected_count(), 2);
    }

    #[test]
    fn test_protector_unprotect() {
        let mut p = MessageProtector::new();
        p.protect(1);
        p.protect(2);
        assert_eq!(p.protected_count(), 2);
        p.unprotect(1);
        assert!(!p.is_protected(1));
        assert!(p.is_protected(2));
        assert_eq!(p.protected_count(), 1);
    }

    #[test]
    fn test_protector_unprotect_nonexistent_is_noop() {
        let mut p = MessageProtector::new();
        p.unprotect(99); // should not panic
        assert_eq!(p.protected_count(), 0);
    }

    #[test]
    fn test_protector_clear() {
        let mut p = MessageProtector::new();
        p.protect(1);
        p.protect(2);
        p.protect(3);
        p.clear();
        assert_eq!(p.protected_count(), 0);
        assert!(!p.is_protected(1));
    }

    #[test]
    fn test_protector_duplicate_protect() {
        let mut p = MessageProtector::new();
        p.protect(5);
        p.protect(5);
        assert_eq!(p.protected_count(), 1);
    }

    #[test]
    fn test_protector_protected_indices() {
        let mut p = MessageProtector::new();
        p.protect(1);
        p.protect(3);
        p.protect(5);
        let indices = p.protected_indices();
        assert!(indices.contains(&1));
        assert!(indices.contains(&3));
        assert!(indices.contains(&5));
        assert!(!indices.contains(&2));
    }

    // ── classify_message_priority ────────────────────────────────────────

    #[test]
    fn test_classify_system_critical() {
        let msg = Message {
            role: "system".into(),
            content: MessageContent::Text("instructions".into()),
        };
        assert_eq!(
            classify_message_priority(&msg),
            crate::context_budget::MessagePriority::Critical
        );
    }

    #[test]
    fn test_classify_short_user_critical() {
        let msg = Message {
            role: "user".into(),
            content: MessageContent::Text("fix the bug".into()),
        };
        assert_eq!(
            classify_message_priority(&msg),
            crate::context_budget::MessagePriority::Critical
        );
    }

    #[test]
    fn test_classify_long_user_normal() {
        let long_text = "x".repeat(300);
        let msg = Message {
            role: "user".into(),
            content: MessageContent::Text(long_text),
        };
        assert_eq!(
            classify_message_priority(&msg),
            crate::context_budget::MessagePriority::Normal
        );
    }

    #[test]
    fn test_classify_assistant_with_code_high() {
        let msg = Message {
            role: "assistant".into(),
            content: MessageContent::Text("```rust\nfn main() {}\n```".into()),
        };
        // looks_like_code should detect the code block
        assert_eq!(
            classify_message_priority(&msg),
            crate::context_budget::MessagePriority::High
        );
    }

    #[test]
    fn test_classify_assistant_without_code_normal() {
        let msg = Message {
            role: "assistant".into(),
            content: MessageContent::Text("Sure, I can help with that.".into()),
        };
        assert_eq!(
            classify_message_priority(&msg),
            crate::context_budget::MessagePriority::Normal
        );
    }

    #[test]
    fn test_classify_tool_result_low() {
        let msg = Message {
            role: "tool".into(),
            content: MessageContent::Text("output".into()),
        };
        assert_eq!(
            classify_message_priority(&msg),
            crate::context_budget::MessagePriority::Low
        );
    }

    #[test]
    fn test_classify_unknown_role_low() {
        let msg = Message {
            role: "other".into(),
            content: MessageContent::Text("data".into()),
        };
        assert_eq!(
            classify_message_priority(&msg),
            crate::context_budget::MessagePriority::Low
        );
    }

    // ── compact_messages_with_protection ─────────────────────────────────

    #[test]
    fn test_compact_empty_messages() {
        let protector = MessageProtector::new();
        let result = compact_messages_with_protection(
            &[],
            &CompactionStrategy::Summarize,
            1000,
            2,
            &protector,
        );
        assert!(!result.did_compact);
        assert_eq!(result.original_count, 0);
    }

    #[test]
    fn test_compact_under_budget_no_change() {
        let msgs = vec![Message {
            role: "user".into(),
            content: MessageContent::Text("hello".into()),
        }];
        let protector = MessageProtector::new();
        let result = compact_messages_with_protection(
            &msgs,
            &CompactionStrategy::Summarize,
            10000,
            2,
            &protector,
        );
        assert!(!result.did_compact);
        assert_eq!(result.compacted_count, 1);
    }

    #[test]
    fn test_protected_messages_preserved() {
        // Build a large conversation that exceeds budget
        let mut msgs = Vec::new();
        for i in 0..20 {
            msgs.push(Message {
                role: if i % 2 == 0 { "user" } else { "tool" }.into(),
                content: MessageContent::Text("x".repeat(500)),
            });
        }
        let mut protector = MessageProtector::new();
        protector.protect(5); // protect one specific message
        let result = compact_messages_with_protection(
            &msgs,
            &CompactionStrategy::Summarize,
            2000,
            2,
            &protector,
        );
        // Protected message should still be in result
        let protected_content = extract_text_content(&msgs[5]);
        let result_texts: Vec<String> = result.messages.iter().map(extract_text_content).collect();
        assert!(result_texts.iter().any(|t| t == &protected_content));
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MessageProtector>();
    }
}
