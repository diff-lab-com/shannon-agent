//! Multi-turn conversation tests: history accumulation, context compaction,
//! and message interleaving.

use shannon_core::api::{ContentBlock, Message, MessageContent, ToolResultContent};
use shannon_core::query_engine::{CompressionStrategy, QueryEngineConfig};

/// Re-implementation of TestConversation for testing since the module is private.
/// Mirrors `query_engine::streaming::TestConversation` exactly.
#[derive(Debug, Clone, Default)]
struct TestConversation {
    messages: Vec<Message>,
    turn_count: usize,
    total_tokens: u64,
    total_cost: f64,
}

impl TestConversation {
    fn estimate_tokens(&self) -> usize {
        let mut total_chars = 0;
        for msg in &self.messages {
            total_chars += match &msg.content {
                MessageContent::Text(text) => text.len(),
                MessageContent::Blocks(blocks) => {
                    let mut n = 0;
                    for b in blocks {
                        match b {
                            ContentBlock::Text { text } => n += text.len(),
                            ContentBlock::ToolUse { name, input, .. } => {
                                n += name.len() + serde_json::to_string(input).map_or(0, |s| s.len());
                            }
                            ContentBlock::ToolResult { content: Some(c), .. } => match c {
                                ToolResultContent::Single(s) => n += s.len(),
                                ToolResultContent::Multiple(items) => {
                                    n += items.iter().map(|b| match b {
                                        ContentBlock::Text { text } => text.len(),
                                        _ => 0,
                                    }).sum::<usize>();
                                }
                            },
                            _ => {}
                        }
                    }
                    n
                }
            };
        }
        total_chars / 4
    }

    fn needs_compression(&self, config: &QueryEngineConfig) -> bool {
        if let Some(max_tokens) = config.max_context_tokens {
            let threshold = (max_tokens as f32 * config.compression_threshold) as usize;
            self.estimate_tokens() > threshold
        } else {
            false
        }
    }

    fn compress(&mut self, config: &QueryEngineConfig) {
        if self.messages.len() <= config.keep_recent_messages + 1 {
            return;
        }
        let split_point = self.messages.len().saturating_sub(config.keep_recent_messages);
        match config.compression_strategy {
            CompressionStrategy::SummarizeOld => {
                let old: Vec<Message> = self.messages.drain(..split_point).collect();
                let summary = summarize_messages(&old);
                let summary_msg = Message {
                    role: "system".to_string(),
                    content: MessageContent::Text(format!("[Previous conversation summary]\n\n{summary}")),
                };
                self.messages.insert(0, summary_msg);
            }
            CompressionStrategy::TruncateOldest => {
                self.messages.drain(..split_point);
            }
        }
    }
}

fn summarize_messages(messages: &[Message]) -> String {
    let mut parts = Vec::new();
    for msg in messages {
        match &msg.content {
            MessageContent::Text(text) => {
                let role = if msg.role == "user" { "User" } else { "Assistant" };
                let preview = if text.len() > 100 { format!("{}...", &text[..100]) } else { text.clone() };
                parts.push(format!("{role}: {preview}"));
            }
            MessageContent::Blocks(blocks) => {
                for b in blocks {
                    if let ContentBlock::ToolUse { name, .. } = b {
                        parts.push(format!("Tool used: {name}"));
                    }
                }
            }
        }
    }
    format!("Summary of {} messages:\n{}", messages.len(), parts.join("\n"))
}

// ── Helpers ──────────────────────────────────────────────────────────

fn text_msg(role: &str, text: &str) -> Message {
    Message {
        role: role.to_string(),
        content: MessageContent::Text(text.to_string()),
    }
}

fn assistant_with_tools() -> Message {
    Message {
        role: "assistant".to_string(),
        content: MessageContent::Blocks(vec![
            ContentBlock::Text {
                text: "Let me read that file.".to_string(),
            },
            ContentBlock::ToolUse {
                id: "tool_1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "/tmp/test.rs"}),
            },
        ]),
    }
}

fn tool_result_msg() -> Message {
    Message {
        role: "user".to_string(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "tool_1".to_string(),
            content: Some(ToolResultContent::Single("fn main() {}".to_string())),
            is_error: None,
        }]),
    }
}

fn low_threshold_config() -> QueryEngineConfig {
    QueryEngineConfig {
        max_context_tokens: Some(200), // very low for testing
        compression_threshold: 0.5,    // trigger at 100 tokens
        keep_recent_messages: 4,
        compression_strategy: CompressionStrategy::SummarizeOld,
        ..QueryEngineConfig::default()
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[test]
fn test_conversation_history_accumulation() {
    let mut state = TestConversation::default();

    // Turn 1
    state.messages.push(text_msg("user", "Hello"));
    state.messages.push(text_msg("assistant", "Hi there!"));
    state.turn_count += 1;

    // Turn 2
    state.messages.push(text_msg("user", "Explain Rust ownership"));
    state.messages.push(text_msg("assistant", "Rust ownership is..."));
    state.turn_count += 1;

    // Turn 3
    state.messages.push(text_msg("user", "Give an example"));
    state.messages.push(text_msg("assistant", "Here's an example:"));
    state.turn_count += 1;

    assert_eq!(state.turn_count, 3);
    assert_eq!(state.messages.len(), 6);

    // Verify ordering: user, assistant alternating
    assert_eq!(state.messages[0].role, "user");
    assert_eq!(state.messages[1].role, "assistant");
    assert_eq!(state.messages[2].role, "user");
    assert_eq!(state.messages[3].role, "assistant");
    assert_eq!(state.messages[4].role, "user");
    assert_eq!(state.messages[5].role, "assistant");

    // Verify content preserved
    assert!(matches!(&state.messages[0].content, MessageContent::Text(t) if t == "Hello"));
    assert!(matches!(&state.messages[3].content, MessageContent::Text(t) if t == "Rust ownership is..."));
}

#[test]
fn test_tool_result_interleaving() {
    let mut state = TestConversation::default();

    // User asks question
    state.messages.push(text_msg("user", "Read the file"));
    // Assistant responds with text + tool_use
    state.messages.push(assistant_with_tools());
    // Tool result comes back as user message (standard pattern)
    state.messages.push(tool_result_msg());
    // Assistant follows up
    state.messages.push(text_msg("assistant", "The file contains a main function."));

    assert_eq!(state.messages.len(), 4);

    // Verify tool_use block exists
    if let MessageContent::Blocks(blocks) = &state.messages[1].content {
        assert!(blocks.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. })));
    } else {
        panic!("Expected Blocks content for assistant message");
    }

    // Verify tool_result block exists
    if let MessageContent::Blocks(blocks) = &state.messages[2].content {
        assert!(blocks.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. })));
    } else {
        panic!("Expected Blocks content for tool result");
    }
}

#[test]
fn test_estimate_tokens() {
    let mut state = TestConversation::default();

    // Empty state
    assert_eq!(state.estimate_tokens(), 0);

    // Add a message (~25 chars = ~6 tokens)
    state.messages.push(text_msg("user", "Hello, this is a test message!"));
    let tokens = state.estimate_tokens();
    assert!(tokens > 0, "Should estimate some tokens");
    assert!(tokens < 100, "Should be a small number for short text");

    // Add more messages
    state.messages.push(text_msg("assistant", "I understand. Let me help you with that."));
    let tokens_after = state.estimate_tokens();
    assert!(tokens_after > tokens, "Tokens should increase with more messages");
}

#[test]
fn test_needs_compression_below_threshold() {
    let mut state = TestConversation::default();
    let config = low_threshold_config(); // threshold at 100 tokens

    // Small conversation shouldn't need compression
    state.messages.push(text_msg("user", "Hi"));
    state.messages.push(text_msg("assistant", "Hello!"));

    assert!(!state.needs_compression(&config));
}

#[test]
fn test_needs_compression_above_threshold() {
    let mut state = TestConversation::default();
    let config = low_threshold_config(); // threshold at 100 tokens (200 * 0.5)

    // Add enough text to exceed threshold
    // ~500 chars / 4 = ~125 tokens > 100 threshold
    for i in 0..20 {
        state.messages.push(text_msg("user", &format!("Message number {i} with some extra content to increase token count significantly")));
        state.messages.push(text_msg("assistant", &format!("Response {i} with enough text to push token estimation well above the compression threshold")));
    }

    assert!(state.needs_compression(&config), "Should need compression with 40 messages");
}

#[test]
fn test_compression_summarize_old() {
    let mut state = TestConversation::default();
    let config = QueryEngineConfig {
        max_context_tokens: Some(200),
        compression_threshold: 0.5,
        keep_recent_messages: 4,
        compression_strategy: CompressionStrategy::SummarizeOld,
        ..QueryEngineConfig::default()
    };

    // Add 10 messages (5 turns)
    for i in 0..5 {
        state.messages.push(text_msg("user", &format!("User question {i}")));
        state.messages.push(text_msg("assistant", &format!("Assistant answer {i}")));
    }

    let original_count = state.messages.len();
    assert_eq!(original_count, 10);

    // Compress — should keep 4 recent, summarize the rest
    state.compress(&config);

    // After compression: 1 summary + 4 recent = 5 messages
    assert!(
        state.messages.len() <= 5,
        "Expected ≤5 messages after SummarizeOld compression, got {}",
        state.messages.len()
    );

    // First message should be the summary
    assert_eq!(state.messages[0].role, "system");
    if let MessageContent::Text(text) = &state.messages[0].content {
        assert!(text.contains("[Previous conversation summary]"), "Expected summary header, got: {text}");
    } else {
        panic!("Expected Text content for summary message");
    }

    // Last 4 messages should be preserved (turns 3-4)
    assert_eq!(state.messages[state.messages.len() - 4].role, "user");
}

#[test]
fn test_compression_truncate_oldest() {
    let mut state = TestConversation::default();
    let config = QueryEngineConfig {
        max_context_tokens: Some(200),
        compression_threshold: 0.5,
        keep_recent_messages: 4,
        compression_strategy: CompressionStrategy::TruncateOldest,
        ..QueryEngineConfig::default()
    };

    // Add 10 messages
    for i in 0..5 {
        state.messages.push(text_msg("user", &format!("User question {i}")));
        state.messages.push(text_msg("assistant", &format!("Assistant answer {i}")));
    }

    state.compress(&config);

    // Should keep exactly 4 recent messages
    assert_eq!(state.messages.len(), 4, "Expected exactly 4 messages after TruncateOldest");

    // Should be the last 4 messages (turns 4)
    if let MessageContent::Text(t) = &state.messages[0].content {
        assert!(t.contains("question 3") || t.contains("question 4"),
            "Expected recent messages, got: {t}");
    }
}

#[test]
fn test_compression_preserves_recent_messages() {
    let mut state = TestConversation::default();
    let config = QueryEngineConfig {
        keep_recent_messages: 6, // keep 3 turns
        compression_strategy: CompressionStrategy::TruncateOldest,
        ..low_threshold_config()
    };

    // Add 20 messages (10 turns)
    for i in 0..10 {
        state.messages.push(text_msg("user", &format!("Q{i}")));
        state.messages.push(text_msg("assistant", &format!("A{i}")));
    }

    state.compress(&config);

    // Last 6 messages (3 turns) should be preserved
    let last_content: Vec<&str> = state.messages.iter().rev().take(2).filter_map(|m| {
        if let MessageContent::Text(t) = &m.content { Some(t.as_str()) } else { None }
    }).collect();

    assert!(last_content.iter().any(|t| t.contains("A9")),
        "Most recent assistant message should be preserved");
}

#[test]
fn test_compression_not_enough_messages() {
    let mut state = TestConversation::default();
    let config = QueryEngineConfig {
        keep_recent_messages: 10,
        compression_strategy: CompressionStrategy::SummarizeOld,
        ..low_threshold_config()
    };

    // Only 4 messages — fewer than keep_recent_messages + 1 = 11
    state.messages.push(text_msg("user", "Hi"));
    state.messages.push(text_msg("assistant", "Hello"));
    state.messages.push(text_msg("user", "How are you?"));
    state.messages.push(text_msg("assistant", "I'm fine!"));

    let count_before = state.messages.len();
    state.compress(&config);
    assert_eq!(state.messages.len(), count_before, "Should not compress when too few messages");
}

#[test]
fn test_multiple_tool_uses_in_history() {
    let mut state = TestConversation::default();

    // User asks
    state.messages.push(text_msg("user", "Check files and read config"));

    // Assistant calls two tools
    state.messages.push(Message {
        role: "assistant".to_string(),
        content: MessageContent::Blocks(vec![
            ContentBlock::Text { text: "Checking now.".to_string() },
            ContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "bash".to_string(),
                input: serde_json::json!({"command": "ls"}),
            },
            ContentBlock::ToolUse {
                id: "t2".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "config.toml"}),
            },
        ]),
    });

    // Tool results
    state.messages.push(Message {
        role: "user".to_string(),
        content: MessageContent::Blocks(vec![
            ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: Some(ToolResultContent::Single("file1.rs\nfile2.rs".to_string())),
                is_error: None,
            },
            ContentBlock::ToolResult {
                tool_use_id: "t2".to_string(),
                content: Some(ToolResultContent::Single("debug = true".to_string())),
                is_error: None,
            },
        ]),
    });

    state.messages.push(text_msg("assistant", "Found 2 files and config is in debug mode."));

    assert_eq!(state.messages.len(), 4);

    // Verify tool_use pairing — assistant has 2 tool_use blocks
    if let MessageContent::Blocks(blocks) = &state.messages[1].content {
        let tool_uses: Vec<_> = blocks.iter().filter(|b| matches!(b, ContentBlock::ToolUse { .. })).collect();
        assert_eq!(tool_uses.len(), 2, "Should have 2 tool_use blocks");
    }

    // Verify tool_result pairing — user has 2 tool_result blocks
    if let MessageContent::Blocks(blocks) = &state.messages[2].content {
        let results: Vec<_> = blocks.iter().filter(|b| matches!(b, ContentBlock::ToolResult { .. })).collect();
        assert_eq!(results.len(), 2, "Should have 2 tool_result blocks");
    }
}

#[test]
fn test_token_estimation_with_blocks() {
    let mut text_only = TestConversation::default();
    text_only.messages.push(text_msg("user", "Hello world test message"));

    let mut with_blocks = TestConversation::default();
    with_blocks.messages.push(Message {
        role: "user".to_string(),
        content: MessageContent::Blocks(vec![
            ContentBlock::Text { text: "Hello world test message".to_string() },
        ]),
    });

    // Both should estimate similar token counts
    let text_tokens = text_only.estimate_tokens();
    let block_tokens = with_blocks.estimate_tokens();
    assert_eq!(text_tokens, block_tokens, "Same text should have same token estimate regardless of container");
}

#[test]
fn test_empty_conversation() {
    let state = TestConversation::default();
    assert_eq!(state.messages.len(), 0);
    assert_eq!(state.turn_count, 0);
    assert_eq!(state.total_tokens, 0);
    assert_eq!(state.total_cost, 0.0);
    assert_eq!(state.estimate_tokens(), 0);

    let config = QueryEngineConfig::default();
    assert!(!state.needs_compression(&config));
}
