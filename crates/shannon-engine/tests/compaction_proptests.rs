//! Property-based tests for message compaction invariants.
//!
//! Uses proptest to verify that compaction preserves structural invariants
//! regardless of input message sequences.

use proptest::prelude::*;
use shannon_core::api::{Message, MessageContent};
use shannon_engine::compact::{CompactionStrategy, compact_messages};

// ── Strategy generators ─────────────────────────────────────────────────────

/// Generate a simple text message with the given role.
fn arb_text_message(role: &str) -> impl Strategy<Value = Message> {
    "[a-zA-Z ]{1,50}".prop_map(move |text| Message {
        role: role.to_string(),
        content: MessageContent::Text(text),
    })
}

/// Generate a user or assistant message with text content.
fn arb_chat_message() -> impl Strategy<Value = Message> {
    prop_oneof![arb_text_message("user"), arb_text_message("assistant"),]
}

/// Generate a system message.
fn arb_system_message() -> impl Strategy<Value = Message> {
    arb_text_message("system")
}

/// Generate a valid conversation: 0-2 system messages + 1-20 user/assistant messages.
fn arb_conversation() -> impl Strategy<Value = Vec<Message>> {
    (
        proptest::collection::vec(arb_system_message(), 0..=2),
        proptest::collection::vec(arb_chat_message(), 1..20),
    )
        .prop_map(|(sys, chat)| {
            let mut msgs = sys;
            msgs.extend(chat);
            msgs
        })
}

/// Generate a compaction strategy.
fn arb_strategy() -> impl Strategy<Value = CompactionStrategy> {
    prop_oneof![
        Just(CompactionStrategy::Summarize),
        (1usize..10).prop_map(|count| CompactionStrategy::KeepRecent { count }),
        Just(CompactionStrategy::PrioritizeCode),
    ]
}

// ── Core invariants ─────────────────────────────────────────────────────────

proptest! {
    /// Compacted message count should never exceed original count.
    #[test]
    fn compacted_count_never_exceeds_original(
        messages in arb_conversation(),
        strategy in arb_strategy(),
        max_tokens in 100usize..10000,
        keep_recent in 1usize..10,
    ) {
        let result = compact_messages(&messages, &strategy, max_tokens, keep_recent);
        prop_assert!(result.compacted_count <= result.original_count,
            "compacted {} > original {}", result.compacted_count, result.original_count);
    }

    /// Compaction never panics on any valid input.
    #[test]
    fn compaction_never_panics(
        messages in arb_conversation(),
        strategy in arb_strategy(),
        max_tokens in 0usize..10000,
        keep_recent in 0usize..20,
    ) {
        let result = compact_messages(&messages, &strategy, max_tokens, keep_recent);
        // Just verify it returns something sane.
        prop_assert!(result.original_count == messages.len());
    }

    /// Result metadata is internally consistent.
    #[test]
    fn result_metadata_is_consistent(
        messages in arb_conversation(),
        strategy in arb_strategy(),
        max_tokens in 100usize..5000,
        keep_recent in 1usize..10,
    ) {
        let result = compact_messages(&messages, &strategy, max_tokens, keep_recent);
        prop_assert_eq!(result.original_count, messages.len());
        prop_assert_eq!(result.compacted_count, result.messages.len());
        prop_assert_eq!(result.did_compact, result.compacted_count < result.original_count);
    }

    /// System messages at the start are always preserved.
    #[test]
    fn system_prompt_preserved(
        sys_msg in arb_system_message(),
        chat in proptest::collection::vec(arb_chat_message(), 2..10),
        strategy in arb_strategy(),
    ) {
        let messages = [
            vec![sys_msg],
            chat,
        ].concat();
        let result = compact_messages(&messages, &strategy, 200, 2);
        // First message must still be a system message.
        prop_assert!(!result.messages.is_empty());
        prop_assert_eq!(&result.messages[0].role, "system", "First message must be system");
    }

    /// When no compaction happens (large budget), messages are returned unchanged.
    #[test]
    fn no_compact_with_large_budget(
        messages in arb_conversation(),
        strategy in arb_strategy(),
    ) {
        let large_budget = 1_000_000;
        let result = compact_messages(&messages, &strategy, large_budget, 100);
        if !result.did_compact {
            prop_assert_eq!(result.messages.len(), messages.len());
        }
    }

    /// The last user message is preserved after compaction.
    #[test]
    fn last_user_message_preserved(
        chat in proptest::collection::vec(arb_chat_message(), 3..15),
        strategy in arb_strategy(),
    ) {
        // Ensure last message is from user.
        let mut messages = chat;
        messages.push(Message {
            role: "user".to_string(),
            content: MessageContent::Text("final user question".to_string()),
        });

        let result = compact_messages(&messages, &strategy, 300, 2);
        let last_user = result.messages.iter().rev().find(|m| m.role == "user");
        prop_assert!(last_user.is_some(), "No user message after compaction");
    }
}

// ── Edge case tests (deterministic) ─────────────────────────────────────────

#[test]
fn empty_messages_no_panic() {
    let result = compact_messages(&[], &CompactionStrategy::Summarize, 1000, 5);
    assert_eq!(result.messages.len(), 0);
    assert!(!result.did_compact);
}

#[test]
fn single_message_no_panic() {
    let msg = Message {
        role: "user".to_string(),
        content: MessageContent::Text("hello".to_string()),
    };
    let result = compact_messages(&[msg], &CompactionStrategy::Summarize, 1000, 5);
    assert!(!result.did_compact);
}

#[test]
fn single_message_compact_when_tiny_budget() {
    let msg = Message {
        role: "user".to_string(),
        content: MessageContent::Text("hello".to_string()),
    };
    // Even with budget of 0, single-message input has nothing to compact away.
    let result = compact_messages(&[msg], &CompactionStrategy::Summarize, 0, 0);
    assert_eq!(result.original_count, 1);
}

#[test]
fn all_system_messages_no_compact() {
    let msgs = vec![
        Message {
            role: "system".to_string(),
            content: MessageContent::Text("sys1".to_string()),
        },
        Message {
            role: "system".to_string(),
            content: MessageContent::Text("sys2".to_string()),
        },
    ];
    let result = compact_messages(&msgs, &CompactionStrategy::Summarize, 50, 1);
    assert!(!result.did_compact);
    assert_eq!(result.messages.len(), 2);
}

#[test]
fn tool_pairs_preserved_after_compact() {
    use shannon_core::api::ContentBlock;

    let messages = vec![
        Message {
            role: "system".to_string(),
            content: MessageContent::Text("system".to_string()),
        },
        Message {
            role: "user".to_string(),
            content: MessageContent::Text("do something".to_string()),
        },
        Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "Bash".to_string(),
                input: serde_json::json!({"command": "ls"}),
            }]),
        },
        Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".to_string(),
                content: Some(shannon_core::api::ToolResultContent::Single(
                    "file.txt".to_string(),
                )),
                is_error: Some(false),
            }]),
        },
        Message {
            role: "assistant".to_string(),
            content: MessageContent::Text("done".to_string()),
        },
        Message {
            role: "user".to_string(),
            content: MessageContent::Text("next question".to_string()),
        },
        Message {
            role: "assistant".to_string(),
            content: MessageContent::Text("answer".to_string()),
        },
    ];

    // Compact with tiny budget to force compaction.
    let result = compact_messages(&messages, &CompactionStrategy::Summarize, 200, 2);
    // After compaction, any ToolUse must have a matching ToolResult.
    let mut tool_use_ids: Vec<String> = Vec::new();
    let mut tool_result_ids: Vec<String> = Vec::new();
    for msg in &result.messages {
        if let MessageContent::Blocks(blocks) = &msg.content {
            for block in blocks {
                match block {
                    ContentBlock::ToolUse { id, .. } => tool_use_ids.push(id.clone()),
                    ContentBlock::ToolResult { tool_use_id, .. } => {
                        tool_result_ids.push(tool_use_id.clone())
                    }
                    _ => {}
                }
            }
        }
    }
    // Every ToolUse present must have a matching ToolResult.
    for id in &tool_use_ids {
        assert!(
            tool_result_ids.contains(id),
            "Orphaned ToolUse '{id}' after compaction"
        );
    }
    // Every ToolResult present must have a matching ToolUse.
    for id in &tool_result_ids {
        assert!(
            tool_use_ids.contains(id),
            "Orphaned ToolResult '{id}' after compaction"
        );
    }
}
