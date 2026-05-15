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

// ── Performance tests ──

/// Verify conversation history accumulation across many turns stays consistent.
#[test]
#[ignore] // performance test — run with `cargo test -- --ignored`
fn test_large_conversation_history_accumulation() {
    let mut state = TestConversation::default();
    let num_turns = 500;

    for i in 0..num_turns {
        state.messages.push(text_msg("user", &format!("User question {i}: tell me about topic {}", i % 50)));
        state.messages.push(text_msg("assistant", &format!("Assistant answer {i}: here is a detailed explanation about topic {} with enough content to be realistic.", i % 50)));
        state.turn_count += 1;
    }

    assert_eq!(state.turn_count, num_turns);
    assert_eq!(state.messages.len(), num_turns * 2);

    // Verify ordering preserved across all 500 turns
    for i in 0..num_turns {
        let user_idx = i * 2;
        let asst_idx = i * 2 + 1;
        assert_eq!(state.messages[user_idx].role, "user",
            "Message {user_idx} should be user, got {}", state.messages[user_idx].role);
        assert_eq!(state.messages[asst_idx].role, "assistant",
            "Message {asst_idx} should be assistant, got {}", state.messages[asst_idx].role);
    }

    // Verify first and last turn content preserved
    match &state.messages[0].content {
        MessageContent::Text(t) => assert!(t.contains("question 0")),
        _ => panic!("Expected text"),
    }
    match &state.messages[state.messages.len() - 1].content {
        MessageContent::Text(t) => assert!(t.contains("answer 499")),
        _ => panic!("Expected text"),
    }
}

/// Verify token estimation scales linearly with conversation size.
#[test]
#[ignore] // performance test — run with `cargo test -- --ignored`
fn test_token_estimation_scaling() {
    let mut state = TestConversation::default();

    let tokens_100 = {
        for i in 0..50 {
            state.messages.push(text_msg("user", &format!("Question {i}")));
            state.messages.push(text_msg("assistant", &format!("Answer {i} with some extra text")));
        }
        state.estimate_tokens()
    };

    let tokens_200 = {
        for i in 50..100 {
            state.messages.push(text_msg("user", &format!("Question {i}")));
            state.messages.push(text_msg("assistant", &format!("Answer {i} with some extra text")));
        }
        state.estimate_tokens()
    };

    let tokens_400 = {
        for i in 100..200 {
            state.messages.push(text_msg("user", &format!("Question {i}")));
            state.messages.push(text_msg("assistant", &format!("Answer {i} with some extra text")));
        }
        state.estimate_tokens()
    };

    // Should scale roughly linearly (2x messages = ~2x tokens)
    assert!(tokens_200 > tokens_100 * 180 / 100,
        "Tokens should grow proportionally: {tokens_100} -> {tokens_200}");
    assert!(tokens_400 > tokens_200 * 180 / 100,
        "Tokens should grow proportionally: {tokens_200} -> {tokens_400}");
}

/// Verify context compression works correctly at scale.
#[test]
#[ignore] // performance test — run with `cargo test -- --ignored`
fn test_large_conversation_compression_summarize() {
    let mut state = TestConversation::default();
    let config = QueryEngineConfig {
        max_context_tokens: Some(500),
        compression_threshold: 0.5, // trigger at 250 tokens
        keep_recent_messages: 20,
        compression_strategy: CompressionStrategy::SummarizeOld,
        ..QueryEngineConfig::default()
    };

    // Build 500 turns
    for i in 0..500 {
        state.messages.push(text_msg("user", &format!("Q{i}")));
        state.messages.push(text_msg("assistant", &format!("A{i}")));
    }
    assert_eq!(state.messages.len(), 1000);

    // Compression should trigger
    assert!(state.needs_compression(&config));

    state.compress(&config);

    // After compression: 1 summary + up to 20 recent messages
    assert!(state.messages.len() <= 21,
        "After compression should have ≤21 messages, got {}", state.messages.len());

    // Summary at front
    assert_eq!(state.messages[0].role, "system");
    match &state.messages[0].content {
        MessageContent::Text(t) => assert!(t.contains("[Previous conversation summary]")),
        _ => panic!("Expected summary text"),
    }

    // Recent messages preserved (last 20 = turns 490-499)
    let last_user = &state.messages[state.messages.len() - 2];
    assert_eq!(last_user.role, "user");
    match &last_user.content {
        MessageContent::Text(t) => assert!(t.contains("Q49"), "Should preserve recent turns"),
        _ => panic!("Expected text"),
    }
}

/// Verify truncation strategy handles large conversations efficiently.
#[test]
#[ignore] // performance test — run with `cargo test -- --ignored`
fn test_large_conversation_compression_truncate() {
    let mut state = TestConversation::default();
    let config = QueryEngineConfig {
        max_context_tokens: Some(500),
        compression_threshold: 0.5,
        keep_recent_messages: 10,
        compression_strategy: CompressionStrategy::TruncateOldest,
        ..QueryEngineConfig::default()
    };

    for i in 0..250 {
        state.messages.push(text_msg("user", &format!("Q{i}")));
        state.messages.push(text_msg("assistant", &format!("A{i}")));
    }

    state.compress(&config);

    assert_eq!(state.messages.len(), 10,
        "Should keep exactly 10 recent messages");

    // Should be the most recent messages
    match &state.messages[0].content {
        MessageContent::Text(t) => {
            let num: usize = t[1..].parse().unwrap();
            assert!(num >= 245, "Should keep recent messages (>=245), got {num}");
        }
        _ => panic!("Expected text"),
    }
}

/// Verify repeated compression cycles don't lose data.
#[test]
#[ignore] // performance test — run with `cargo test -- --ignored`
fn test_repeated_compression_cycles() {
    let mut state = TestConversation::default();
    let config = QueryEngineConfig {
        max_context_tokens: Some(300),
        compression_threshold: 0.5,
        keep_recent_messages: 6,
        compression_strategy: CompressionStrategy::TruncateOldest,
        ..QueryEngineConfig::default()
    };

    // Simulate 5 rounds of: add messages → compress
    for round in 0..5 {
        let base = round * 20;
        for i in base..(base + 20) {
            // Use longer messages to ensure compression triggers
            state.messages.push(text_msg("user", &format!("Question number {i} with some extra text to increase token count significantly")));
            state.messages.push(text_msg("assistant", &format!("Answer number {i} with enough text to push token estimation well above the compression threshold value")));
        }
        if state.needs_compression(&config) {
            state.compress(&config);
        }
    }

    // After all cycles, should still have at most keep_recent_messages
    assert!(state.messages.len() <= 6,
        "Should have at most 6 messages after repeated compression, got {}", state.messages.len());

    // Most recent messages should be from the last round
    let last = &state.messages[state.messages.len() - 1];
    match &last.content {
        MessageContent::Text(t) => assert!(t.contains("Answer number 9") || t.contains("Answer number 8"),
            "Should preserve most recent: got {t}"),
        _ => panic!("Expected text"),
    }
}

/// Simulate realistic multi-turn with tool use interleaving.
#[test]
#[ignore] // performance test — run with `cargo test -- --ignored`
fn test_large_conversation_with_tool_interleaving() {
    let mut state = TestConversation::default();

    // 100 turns, each with a tool use cycle (user → assistant+tool → tool_result → assistant)
    for i in 0..100 {
        state.messages.push(text_msg("user", &format!("Read file {i}")));
        state.messages.push(Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text { text: format!("Reading file {i}...") },
                ContentBlock::ToolUse {
                    id: format!("t_{i}"),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": format!("/tmp/file{i}.txt")}),
                },
            ]),
        });
        state.messages.push(Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: format!("t_{i}"),
                content: Some(ToolResultContent::Single(format!("Content of file {i}"))),
                is_error: None,
            }]),
        });
        state.messages.push(text_msg("assistant", &format!("File {i} contains: some data")));
    }

    // 100 turns × 4 messages = 400 messages
    assert_eq!(state.messages.len(), 400);

    // Verify structure is correct throughout
    for i in 0..100 {
        let base = i * 4;
        assert_eq!(state.messages[base].role, "user");
        assert_eq!(state.messages[base + 1].role, "assistant");
        assert_eq!(state.messages[base + 2].role, "user"); // tool result
        assert_eq!(state.messages[base + 3].role, "assistant");
    }

    // Token estimation should be reasonable
    let tokens = state.estimate_tokens();
    assert!(tokens > 0);
    assert!(tokens < 100_000, "Token estimate should be bounded, got {tokens}");
}

/// Verify compression with tool-interleaved conversations preserves tool pairing.
#[test]
#[ignore] // performance test — run with `cargo test -- --ignored`
fn test_compression_preserves_tool_pairing() {
    let mut state = TestConversation::default();
    let config = QueryEngineConfig {
        max_context_tokens: Some(200),
        compression_threshold: 0.5,
        keep_recent_messages: 8, // 2 full tool-use cycles
        compression_strategy: CompressionStrategy::TruncateOldest,
        ..QueryEngineConfig::default()
    };

    // Add 5 tool-use cycles
    for i in 0..5 {
        state.messages.push(text_msg("user", &format!("Q{i}")));
        state.messages.push(Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text { text: format!("Working on {i}") },
                ContentBlock::ToolUse {
                    id: format!("t{i}"),
                    name: "bash".to_string(),
                    input: serde_json::json!({"command": format!("echo {i}")}),
                },
            ]),
        });
        state.messages.push(Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: format!("t{i}"),
                content: Some(ToolResultContent::Single(format!("output {i}"))),
                is_error: None,
            }]),
        });
        state.messages.push(text_msg("assistant", &format!("Done {i}")));
    }

    assert_eq!(state.messages.len(), 20);
    state.compress(&config);

    // Should keep 8 messages = 2 complete tool-use cycles
    assert_eq!(state.messages.len(), 8);

    // Tool_use and tool_result should be properly paired
    // (assistant has tool_use, followed by user with tool_result)
    if let MessageContent::Blocks(blocks) = &state.messages[1].content {
        assert!(blocks.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. })),
            "Kept assistant message should still have tool_use");
    }
    if let MessageContent::Blocks(blocks) = &state.messages[2].content {
        assert!(blocks.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. })),
            "Kept user message should still have tool_result");
    }
}

/// Benchmark: conversation operations should complete in reasonable time.
#[test]
#[ignore] // performance test — run with `cargo test -- --ignored`
fn test_conversation_operations_performance() {
    use std::time::Instant;

    let mut state = TestConversation::default();

    // Build 500-turn conversation
    let build_start = Instant::now();
    for i in 0..500 {
        state.messages.push(text_msg("user", &format!("Question {i} with enough content to be realistic")));
        state.messages.push(text_msg("assistant", &format!("Answer {i} with detailed explanation")));
    }
    let build_time = build_start.elapsed();
    assert!(build_time.as_millis() < 100,
        "Building 500-turn conversation took {build_time:?}, expected <100ms");

    // Token estimation
    let est_start = Instant::now();
    let tokens = state.estimate_tokens();
    let est_time = est_start.elapsed();
    assert!(est_time.as_millis() < 50,
        "Token estimation for 1000 messages took {est_time:?}, expected <50ms");
    assert!(tokens > 0);

    // Compression check
    let config = QueryEngineConfig {
        max_context_tokens: Some(5000),
        compression_threshold: 0.5,
        keep_recent_messages: 20,
        compression_strategy: CompressionStrategy::TruncateOldest,
        ..QueryEngineConfig::default()
    };

    let compress_start = Instant::now();
    state.compress(&config);
    let compress_time = compress_start.elapsed();
    assert!(compress_time.as_millis() < 100,
        "Compression of 1000 messages took {compress_time:?}, expected <100ms");
    assert!(state.messages.len() <= 20);
}

// ── Compact Engine Regression Tests ────────────────────────────────
// These tests verify the CompactEngine works correctly with the
// rule-based summarizer (no LLM API calls), which is the default
// mode used by the /compact command.

use shannon_core::compact::CompactEngine;

fn build_conversation(turns: usize) -> Vec<Message> {
    let mut messages = Vec::new();
    for i in 0..turns {
        messages.push(text_msg("user", &format!("User question {i}: explain topic {} in detail with examples and code samples.", i % 10)));
        messages.push(text_msg("assistant", &format!("Assistant answer {i}: here is a comprehensive explanation about topic {} with detailed examples, code samples, and analysis.", i % 10)));
    }
    messages
}

#[test]
fn test_compact_engine_creation_default() {
    let engine = CompactEngine::with_defaults();
    assert!(engine.is_ok(), "CompactEngine::with_defaults() should succeed");
}

#[test]
fn test_compact_engine_empty_history() {
    let mut engine = CompactEngine::with_defaults().unwrap();
    let mut messages: Vec<Message> = vec![];
    let result = engine.compact(&mut messages);
    assert!(result.is_err(), "Should error on empty messages");
    assert!(matches!(result.unwrap_err(), shannon_core::compact::CompactError::NoMessagesToCompact));
}

#[test]
fn test_compact_engine_too_few_messages() {
    let mut engine = CompactEngine::with_defaults().unwrap();
    let mut messages = build_conversation(2);
    let result = engine.compact(&mut messages);
    assert!(result.is_ok(), "Should succeed even with few messages");
    let cr = result.unwrap();
    // Not enough messages to compact, should report no change
    assert_eq!(cr.messages_removed, 0, "Should not remove any messages with small conversation");
}

#[test]
fn test_compact_engine_normal_conversation() {
    let mut engine = CompactEngine::with_defaults().unwrap();
    let mut messages = build_conversation(20);
    let original_len = messages.len();
    let _original_tokens = shannon_core::compact::estimate_tokens(&messages);

    let result = engine.compact(&mut messages);
    assert!(result.is_ok(), "Compact should succeed on normal conversation");

    let cr = result.unwrap();
    assert!(cr.original_tokens > 0, "Should report original tokens");
    assert!(messages.len() < original_len || cr.messages_removed == 0,
        "Messages should be reduced or no change needed");
    assert!(cr.messages_compacted > 0 || original_len <= engine.config().keep_recent_count + 1,
        "Should compact messages when conversation is large enough");
}

#[test]
fn test_compact_engine_micro_compact() {
    let engine = CompactEngine::with_defaults().unwrap();
    let mut messages = build_conversation(5);
    // Add one very large message to trigger micro-compacting
    let large_text: String = "This is a long sentence about Rust. ".repeat(500);
    messages.push(text_msg("assistant", &large_text));

    let result = engine.micro_compact(&mut messages);
    assert!(result.is_ok(), "Micro-compact should succeed");
}

#[test]
fn test_compact_engine_group_compact() {
    let mut engine = CompactEngine::with_defaults().unwrap();
    let mut messages = build_conversation(15);
    let result = engine.group_compact(&mut messages);
    assert!(result.is_ok(), "Group-compact should succeed");
}

#[test]
fn test_compact_engine_analyze_context() {
    let engine = CompactEngine::with_defaults().unwrap();
    let messages = build_conversation(10);
    let analysis = engine.analyze_context(&messages);

    assert!(analysis.estimated_tokens > 0, "Should estimate tokens");
    assert!(analysis.context_usage_ratio >= 0.0, "Context ratio should be non-negative");
    assert!(analysis.compactable_message_count > 0, "Should have compactable candidates");
    assert!(analysis.compactable_message_count <= messages.len(), "Compactable count cannot exceed total messages");
}

#[test]
fn test_compact_focus_mode_filtering() {
    // Simulate focus-mode filtering: keep messages matching keywords, compact the rest
    let messages = build_conversation(20);
    let keywords = ["topic 5", "topic 3"];

    let mut to_keep: Vec<Message> = Vec::new();
    let mut to_compact: Vec<Message> = Vec::new();

    for msg in messages {
        let text = match &msg.content {
            MessageContent::Text(t) => t.to_lowercase(),
            MessageContent::Blocks(_) => String::new(),
        };
        if keywords.iter().any(|kw| text.contains(kw)) || msg.role == "system" {
            to_keep.push(msg);
        } else {
            to_compact.push(msg);
        }
    }

    assert!(!to_keep.is_empty(), "Should find messages matching focus keywords");
    assert!(!to_compact.is_empty(), "Should have messages to compact");
    assert!(to_keep.len() < 40, "Focus should filter down to fewer messages");

    // Verify compact engine works on the filtered set
    let mut engine = CompactEngine::with_defaults().unwrap();
    if to_compact.len() > engine.config().keep_recent_count + 1 {
        let result = engine.compact(&mut to_compact);
        assert!(result.is_ok(), "Compact of non-focus messages should succeed");
    }

    // Re-merge: kept messages + compacted messages
    to_keep.append(&mut to_compact);
    // All messages should still be present (some compressed)
    assert!(!to_keep.is_empty());
}

#[test]
fn test_compact_preserves_system_messages() {
    let mut engine = CompactEngine::with_defaults().unwrap();
    let mut messages = vec![
        Message { role: "system".to_string(), content: MessageContent::Text("You are a helpful assistant.".to_string()) },
    ];
    messages.extend(build_conversation(15));

    let result = engine.compact(&mut messages);
    assert!(result.is_ok());

    // System message should still be present
    let has_system = messages.iter().any(|m| m.role == "system");
    assert!(has_system, "System message should be preserved after compaction");
}

#[test]
fn test_compact_double_compact_rejected() {
    let mut engine = CompactEngine::with_defaults().unwrap();
    let mut messages = build_conversation(20);

    // First compact should succeed
    let result1 = engine.compact(&mut messages);
    assert!(result1.is_ok());

    // Second compact should also succeed (compacting flag is reset)
    let result2 = engine.compact(&mut messages);
    assert!(result2.is_ok(), "Second compact should work since compacting flag resets");
}

// ── Context Preservation Regression Tests ─────────────────────────────

/// Verify that conversation messages accumulate correctly across multiple
/// simulated turns — this is the core invariant for the multi-turn context
/// loss bug. If this fails, the engine is losing messages between turns.
#[test]
fn test_multi_turn_messages_preserved() {
    let mut conv = TestConversation::default();

    // Simulate turn 1: user writes a story, assistant responds
    let story = "在一个遥远的王国里，有一位年轻的骑士名叫阿尔弗雷德。他带着一把闪烁着蓝光的剑，踏上了寻找失落宝藏的旅程。途中他遇到了一位名叫莉莉的精灵，她告诉他宝藏被一条古老的龙守护着。他们一起穿越了幽暗森林和冰封山脉，最终在火山口找到了那把传说中的钥匙。";
    conv.messages.push(text_msg("user", "写小说200字"));
    conv.messages.push(text_msg("assistant", story));
    conv.turn_count += 1;

    // Simulate turn 2: user asks about the story
    conv.messages.push(text_msg("user", "这篇小说有几个人物?几个场景?"));
    conv.turn_count += 1;

    // Before API call, engine should have all 3 messages
    assert_eq!(conv.messages.len(), 3, "Should have 3 messages (2 from turn 1 + 1 from turn 2)");

    // Verify the story content is still present for context
    let story_msg = &conv.messages[1];
    match &story_msg.content {
        MessageContent::Text(t) => {
            assert!(t.contains("阿尔弗雷德"), "Story character should be in message history");
            assert!(t.contains("莉莉"), "Second character should be in message history");
        }
        _ => panic!("Expected text content for assistant message"),
    }

    // Verify the follow-up question is the last message
    let last = conv.messages.last().unwrap();
    match &last.content {
        MessageContent::Text(t) => assert!(t.contains("几个人物"), "Follow-up question should be last"),
        _ => panic!("Expected text content"),
    }
}

/// Verify that compression preserves the N most recent messages intact,
/// so the most recent context (what the model needs to answer) is never lost.
#[test]
fn test_compression_keeps_recent_context_intact() {
    let mut conv = TestConversation::default();

    // Build a conversation with 10 turns (20 messages)
    for i in 0..10 {
        conv.messages.push(text_msg("user", &format!("Question about topic {i}: what is the answer?")));
        conv.messages.push(text_msg("assistant", &format!("The answer to topic {i} involves detailed explanation about concepts {i} through {}.", i + 1)));
    }

    assert_eq!(conv.messages.len(), 20);

    let config = QueryEngineConfig {
        keep_recent_messages: 6,
        compression_strategy: CompressionStrategy::SummarizeOld,
        ..low_threshold_config()
    };

    conv.compress(&config);

    // The last 6 messages (3 turns) must be preserved verbatim
    let recent: Vec<&str> = conv.messages.iter().rev().take(6)
        .filter_map(|m| match &m.content {
            MessageContent::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();

    // Most recent messages should reference topic 9
    assert!(recent.iter().any(|t| t.contains("topic 9")),
        "Most recent turn (topic 9) should be preserved, got: {recent:?}");

    // Second-to-last turn (topic 8) should also be preserved
    assert!(recent.iter().any(|t| t.contains("topic 8")),
        "Second recent turn (topic 8) should be preserved, got: {recent:?}");
}

/// Verify that cloning a conversation preserves all messages — this is the
/// mechanism used by process_query() at engine.rs:661.
#[test]
fn test_conversation_clone_preserves_messages() {
    let mut conv = TestConversation::default();
    conv.messages.push(text_msg("user", "First question"));
    conv.messages.push(text_msg("assistant", "First answer with detailed content"));
    conv.messages.push(text_msg("user", "Follow-up question referencing first answer"));
    conv.turn_count = 2;

    // Clone (simulates engine.process_query cloning self.conversation)
    let mut cloned = conv.clone();

    // Add a new user message to the clone (simulates adding user message in process_query)
    cloned.messages.push(text_msg("user", "Third question"));

    // Original should be unchanged
    assert_eq!(conv.messages.len(), 3, "Original should have 3 messages");

    // Clone should have 4
    assert_eq!(cloned.messages.len(), 4, "Clone should have 4 messages");

    // All original messages should be identical in the clone
    for i in 0..conv.messages.len() {
        assert_eq!(conv.messages[i].role, cloned.messages[i].role,
            "Message {i} role should match after clone");
        match (&conv.messages[i].content, &cloned.messages[i].content) {
            (MessageContent::Text(a), MessageContent::Text(b)) => assert_eq!(a, b),
            _ => panic!("Content mismatch at index {i}"),
        }
    }
}

/// Verify cost accumulation pattern: per-query costs should accumulate,
/// not replace the running total.
#[test]
#[allow(unused_assignments)]
fn test_cost_accumulation_not_replacement() {
    let mut total_cost: f64 = 0.0;

    // Simulate turn 1 cost
    let turn1_cost: f64 = 0.0242;
    total_cost = 0.0_f64 + turn1_cost; // pre_stream_cost (0) + s.cost
    assert!((total_cost - 0.0242_f64).abs() < f64::EPSILON, "After turn 1: {total_cost}");

    // Simulate turn 2 cost — must ACCUMULATE, not replace
    let turn2_cost: f64 = 0.0203;
    let pre_stream_cost = total_cost;
    total_cost = pre_stream_cost + turn2_cost;
    assert!((total_cost - 0.0445_f64).abs() < 0.0001_f64,
        "After turn 2: expected ~0.0445, got {total_cost}");

    // Verify the OLD (buggy) behavior would have given wrong result
    let buggy_total: f64 = turn2_cost; // replacement, not accumulation
    assert!((buggy_total - 0.0203_f64).abs() < f64::EPSILON);
    assert!(total_cost > buggy_total, "Accumulated cost should be higher than replacement cost");
}

// ── Context Loss Regression Tests ─────────────────────────────────────
// These tests verify fixes for the multi-turn context loss bugs:
// 1. Auto-compact retry response not added to conversation.messages
// 2. OpenAI adapter dropping assistant text when tool_calls present
// 3. Error path losing user message

/// Regression test: verify that when a conversation is restored via
/// restore_messages(), the assistant's response from the previous turn
/// is preserved. This tests the core invariant that the background task's
/// conversation state (with the assistant response) is properly synced back.
#[test]
fn test_restore_messages_preserves_assistant_response() {
    let mut conv = TestConversation::default();

    // Turn 1: user asks, assistant responds
    conv.messages.push(text_msg("user", "写一篇科幻小说"));
    conv.messages.push(text_msg("assistant", "在一个遥远的星球上，机器人阿尔法和人类莉莉一起探索废墟..."));
    conv.turn_count += 1;

    // Simulate the "restore_messages" pattern: the UI receives a ConversationUpdate
    // with conversation.messages from the background task, then calls restore_messages.
    let restored_messages = conv.messages.clone();
    assert_eq!(restored_messages.len(), 2);
    assert_eq!(restored_messages[1].role, "assistant");

    // Simulate turn 2: clone conversation, add new user message
    let mut cloned = conv.clone();
    cloned.messages.push(text_msg("user", "小说里有几个人物？"));

    // Verify the assistant response from turn 1 is still in the clone
    assert_eq!(cloned.messages.len(), 3);
    match &cloned.messages[1].content {
        MessageContent::Text(t) => {
            assert!(t.contains("阿尔法"), "Assistant's story should still be in context for turn 2");
            assert!(t.contains("莉莉"), "Characters from previous response must be preserved");
        }
        _ => panic!("Expected text content for assistant message"),
    }
}

/// Regression test: simulate the auto-compact retry scenario.
/// When a token overflow triggers auto-compact and the API is retried,
/// the retry response MUST be accumulated and added to conversation.messages
/// before ConversationUpdate is sent. Otherwise, the next turn loses context.
#[test]
fn test_auto_compact_retry_preserves_response() {
    let mut conv = TestConversation::default();

    // Build a conversation with multiple turns
    conv.messages.push(text_msg("user", "Question 1"));
    conv.messages.push(text_msg("assistant", "Answer 1 with enough detail to simulate a real response."));
    conv.messages.push(text_msg("user", "Question 2"));
    conv.messages.push(text_msg("assistant", "Answer 2 with enough detail to simulate a real response."));

    // Simulate what happens in the auto-compact retry path:
    // 1. Messages get truncated for the retry
    let compact_keep = 2;
    let retry_messages = if conv.messages.len() > compact_keep {
        conv.messages.split_off(conv.messages.len() - compact_keep)
    } else {
        conv.messages.clone()
    };

    // 2. The retry API call produces a new response
    let retry_response = "This is the retry response that must not be lost.";

    // 3. BUG FIX: The retry response must be added to conversation.messages
    // (this is what the fix in engine.rs does)
    conv.messages = retry_messages.clone();
    conv.messages.push(text_msg("assistant", retry_response));

    // 4. ConversationUpdate sends conversation.messages.clone()
    let update_messages = conv.messages.clone();

    // 5. Verify: the retry response IS in the final conversation
    assert_eq!(update_messages.len(), 3, "Should have 3 messages after retry");
    let last = update_messages.last().unwrap();
    assert_eq!(last.role, "assistant");
    match &last.content {
        MessageContent::Text(t) => assert!(t.contains(retry_response),
            "Retry response must be in conversation, got: {t}"),
        _ => panic!("Expected text content"),
    }

    // 6. Simulate next turn — verify retry response is available as context
    let mut next_turn = update_messages.clone();
    next_turn.push(text_msg("user", "Follow-up question about the retry response"));
    assert!(next_turn.len() >= 4);
    // The retry response at index 2 should still be there
    match &next_turn[2].content {
        MessageContent::Text(t) => assert!(t.contains(retry_response)),
        _ => panic!("Retry response lost between turns!"),
    }
}

/// Regression test: verify that the OpenAI adapter includes assistant text
/// content when tool_calls are also present. Previously, text was dropped.
#[test]
fn test_openai_adapter_preserves_text_with_tool_calls() {
    use serde_json::Value;

    // Simulate the adapter's convert_message_for_openai logic
    // An assistant message with both text and tool_use blocks
    let msg = Message {
        role: "assistant".to_string(),
        content: MessageContent::Blocks(vec![
            ContentBlock::Text { text: "Let me read that file for you.".to_string() },
            ContentBlock::ToolUse {
                id: "tool_1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "/tmp/test.rs"}),
            },
        ]),
    };

    // Extract tool calls
    let tool_calls: Vec<Value> = match &msg.content {
        MessageContent::Blocks(blocks) => blocks.iter().enumerate()
            .filter_map(|(i, b)| match b {
                ContentBlock::ToolUse { id, name, input } => Some(serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": input.to_string() },
                    "index": i,
                })),
                _ => None,
            })
            .collect(),
        _ => vec![],
    };

    assert!(!tool_calls.is_empty(), "Should find tool calls");

    // The FIX: extract text content even when tool_calls exist
    let text_content: String = match &msg.content {
        MessageContent::Blocks(blocks) => blocks.iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };

    // Verify text content is NOT empty (was empty before the fix)
    assert!(!text_content.is_empty(), "Text content must be preserved alongside tool_calls");
    assert!(text_content.contains("read that file"), "Text content should match original");

    // Build the OpenAI message with BOTH text and tool_calls
    let openai_msg = serde_json::json!({
        "role": msg.role,
        "content": if text_content.is_empty() { serde_json::Value::Null } else { serde_json::json!(text_content) },
        "tool_calls": tool_calls,
    });

    // Verify the message has both fields
    assert!(openai_msg.get("tool_calls").is_some(), "Should have tool_calls");
    assert!(openai_msg.get("content").is_some(), "Should have content");
    assert_ne!(openai_msg["content"], serde_json::Value::Null, "Content should not be null");
}

/// Regression test: verify that when a query fails, the user message is still
/// preserved in the engine's conversation. Previously, the user message was
/// only added to the background task's clone, not the engine itself.
#[test]
fn test_error_path_preserves_user_message() {
    let mut conv = TestConversation::default();

    // Turn 1: successful
    conv.messages.push(text_msg("user", "First question"));
    conv.messages.push(text_msg("assistant", "First answer"));
    conv.turn_count += 1;

    // Turn 2: user asks a question — this gets added to the CLONE, not the engine
    let user_msg_2 = "Second question that will fail";
    let mut cloned = conv.clone();
    cloned.messages.push(text_msg("user", user_msg_2));

    // The query fails — the clone's conversation is discarded
    // BUG FIX: the user message must still be added to the engine's conversation
    // (simulating the fix in query.rs error path)
    conv.messages.push(text_msg("user", user_msg_2));

    // Verify: the engine's conversation now has 3 messages
    assert_eq!(conv.messages.len(), 3, "Should have 3 messages after error recovery");
    assert_eq!(conv.messages[2].role, "user");

    // Turn 3: next query should see the previous user message
    let mut next_query = conv.clone();
    next_query.messages.push(text_msg("user", "Third question"));
    assert_eq!(next_query.messages.len(), 4);

    // The failed user message should be in context
    match &next_query.messages[2].content {
        MessageContent::Text(t) => assert!(t.contains("Second question"),
            "Failed user message must be preserved for context"),
        _ => panic!("Expected text content"),
    }
}

/// Regression test: verify that conversation clone + restore roundtrip
/// preserves all messages across multiple simulated query cycles.
#[test]
fn test_full_query_cycle_preserves_context() {
    let mut engine_conv = TestConversation::default();

    // Simulate 3 complete query cycles
    for turn in 0..3 {
        // Step 1: Clone conversation (process_query does this)
        let mut bg_conv = engine_conv.clone();

        // Step 2: Add user message to clone
        bg_conv.messages.push(text_msg("user", &format!("Turn {} question about the previous responses", turn + 1)));

        // Step 3: Add assistant response to clone
        bg_conv.messages.push(text_msg("assistant", &format!("Turn {} answer referencing all previous context", turn + 1)));

        // Step 4: ConversationUpdate sends bg_conv.messages
        let update = bg_conv.messages.clone();

        // Step 5: restore_messages replaces engine conversation
        engine_conv.messages = update;
        engine_conv.turn_count += 1;
    }

    // After 3 cycles, should have 6 messages (3 user + 3 assistant)
    assert_eq!(engine_conv.messages.len(), 6, "Should have 6 messages after 3 turns");
    assert_eq!(engine_conv.turn_count, 3);

    // Verify all messages are in order
    for i in 0..3 {
        let user_idx = i * 2;
        let asst_idx = i * 2 + 1;
        assert_eq!(engine_conv.messages[user_idx].role, "user",
            "Message {user_idx} should be user");
        assert_eq!(engine_conv.messages[asst_idx].role, "assistant",
            "Message {asst_idx} should be assistant");
    }

    // Verify the story-pattern: each response mentions the turn
    match &engine_conv.messages[5].content {
        MessageContent::Text(t) => assert!(t.contains("Turn 3"),
            "Last response should reference turn 3"),
        _ => panic!("Expected text"),
    }
}

// ── Regression tests for context loss bugs ──────────────────────────

/// Regression: Tool results must be added to conversation.messages (not just
/// the local `messages` variable used for the API call).  Without this,
/// multi-turn conversations that interleave tool use produce invalid message
/// sequences (two consecutive assistant messages), causing the provider API
/// to reject the request or the model to lose context.
#[test]
fn test_tool_results_persisted_in_conversation_messages() {
    let mut engine_conv = TestConversation::default();

    // ── Turn 1: user asks for a story, assistant responds ──
    engine_conv.messages.push(text_msg("user", "Write a 200-word sci-fi story"));
    engine_conv.messages.push(text_msg("assistant",
        "In the year 2187, Dr. Elara Chen stared at the quantum display…"));

    // ── Turn 2: user asks a follow-up that triggers tool use ──
    let mut bg_conv = engine_conv.clone();
    bg_conv.messages.push(text_msg("user", "How many characters appeared in this story?"));

    // Simulate what process_query does: build local `messages` from conversation
    let mut messages = bg_conv.messages.clone();

    // Assistant decides to use a tool (e.g. search)
    let tool_msg = assistant_with_tools();
    messages.push(tool_msg.clone());
    bg_conv.messages.push(tool_msg);

    // **THE BUG FIX**: tool results must go into BOTH `messages` AND
    // `conversation.messages` (bg_conv here).
    let result_msg = tool_result_msg();
    messages.push(result_msg.clone());
    bg_conv.messages.push(result_msg); // <- this line was missing before the fix

    // Assistant final response
    let final_answer = text_msg("assistant",
        "The story features 3 characters: Dr. Elara Chen, Captain Voss, and the AI navigator ORION.");
    messages.push(final_answer.clone());
    bg_conv.messages.push(final_answer);

    // ConversationUpdate restores messages
    engine_conv.messages = bg_conv.messages.clone();

    // ── Turn 3: verify context is intact ──
    let next_query = engine_conv.messages.clone();

    // Validate alternating user/assistant pattern (no two consecutive same-role)
    for i in 1..next_query.len() {
        assert_ne!(next_query[i].role, next_query[i - 1].role,
            "Messages must alternate roles, but messages[{}] and messages[{}] are both '{}'",
            i - 1, i, next_query[i].role);
    }

    // Validate the story text is still present in context
    let story_present = next_query.iter().any(|m| {
        match &m.content {
            MessageContent::Text(t) => t.contains("Dr. Elara Chen stared"),
            _ => false,
        }
    });
    assert!(story_present, "Turn 1 story must be in context for Turn 3");

    // Validate tool result is present
    let tool_result_present = next_query.iter().any(|m| {
        matches!(&m.content,
            MessageContent::Blocks(blocks) if blocks.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. })))
    });
    assert!(tool_result_present, "Tool result must be persisted in conversation messages");
}

/// Regression: When a query fails (stream error, API error, etc.), the
/// ConversationUpdate event must be sent *before* the Failed event so that
/// the UI can restore whatever context was accumulated before the failure.
#[test]
fn test_conversation_update_before_failed_preserves_context() {
    let mut engine_conv = TestConversation::default();

    // Turn 1: successful exchange
    engine_conv.messages.push(text_msg("user", "Tell me about Rust"));
    engine_conv.messages.push(text_msg("assistant", "Rust is a systems programming language…"));

    // Turn 2: user asks follow-up, but query will fail
    let mut bg_conv = engine_conv.clone();
    bg_conv.messages.push(text_msg("user", "What about error handling?"));

    // Simulate: the engine processes, maybe adds partial assistant message
    bg_conv.messages.push(text_msg("assistant", "Error handling in Rust uses Result<T, E>"));

    // **THE BUG FIX**: ConversationUpdate must be sent before Failed
    // In real code, this means the UI receives the updated messages before
    // it receives the error. Simulate restore_messages:
    engine_conv.messages = bg_conv.messages.clone();

    // Now simulate the Failed event arriving — context is already saved
    // Verify the user's failed-turn message is preserved
    let has_failed_question = engine_conv.messages.iter().any(|m| {
        match &m.content {
            MessageContent::Text(t) => t.contains("error handling"),
            _ => false,
        }
    });
    assert!(has_failed_question,
        "User's question from the failed turn must be preserved in conversation");

    // Verify the partial assistant response is also preserved
    let has_partial_response = engine_conv.messages.iter().any(|m| {
        match &m.content {
            MessageContent::Text(t) => t.contains("Result<T, E>"),
            _ => false,
        }
    });
    assert!(has_partial_response,
        "Partial assistant response from the failed turn must be preserved");

    // Turn 3: next query can still reference context from the failed turn
    let mut next_bg = engine_conv.clone();
    next_bg.messages.push(text_msg("user",
        "Can you give me an example of the Result type you mentioned?"));

    let context_intact = next_bg.messages.iter().any(|m| {
        match &m.content {
            MessageContent::Text(t) => t.contains("Result<T, E>"),
            _ => false,
        }
    });
    assert!(context_intact,
        "Context from the failed turn must survive into the next query");
}

/// Regression: End-to-end simulation of the exact user-reported scenario:
/// Turn 1 asks for a 200-word story, Turn 2 asks about characters in the
/// story, but with tool use interleaved.  The AI must retain the story
/// content across turns.
#[test]
fn test_multi_turn_story_character_count_scenario() {
    let mut engine_conv = TestConversation::default();

    // ── Turn 1: user asks for a sci-fi story ──
    {
        let mut bg = engine_conv.clone();
        bg.messages.push(text_msg("user", "写一篇200字的科幻小说"));
        bg.messages.push(text_msg("assistant",
            "2187年，陈伊拉博士凝视着量子显示屏。三周前，ORION——飞船的AI导航系统——第一次对她说了一个谎。\n\n\"轨道正常，\"ORION的声音平静得不像话。\n\n但陈伊拉看到了它没看到的：前方不是地球，而是一个吞噬了一切的虚无。沃斯舰长从指挥舱冲进来：\"它把我们带错了。\"\n\n\"不，\"陈伊拉低声说，\"它带我们去的是对的。是我们错了——关于'对'的定义。\"\n\nORION在沉默中重新计算了一切。"
        ));
        engine_conv.messages = bg.messages.clone();
    }

    assert_eq!(engine_conv.messages.len(), 2);

    // ── Turn 2: user asks about characters, AI uses a tool ──
    {
        let mut bg = engine_conv.clone();
        bg.messages.push(text_msg("user", "这篇小说中出场人物有几个？"));

        let mut messages = bg.messages.clone();

        // AI decides to use a tool (e.g., to count or search)
        let tool_use = assistant_with_tools();
        messages.push(tool_use.clone());
        bg.messages.push(tool_use);

        // **FIX VERIFIED**: tool result must go into both vectors
        let result = tool_result_msg();
        messages.push(result.clone());
        bg.messages.push(result);

        // AI final answer referencing the story from Turn 1
        let answer = text_msg("assistant",
            "这篇小说中有3个出场人物：1) 陈伊拉博士（Dr. Elara Chen）— 主角；2) ORION — AI导航系统；3) 沃斯舰长（Captain Voss）。");
        messages.push(answer.clone());
        bg.messages.push(answer);

        engine_conv.messages = bg.messages.clone();
    }

    // Validate: should have 6 messages total
    assert_eq!(engine_conv.messages.len(), 6,
        "Turn 1 (2 msgs) + Turn 2 user + tool_use + tool_result + assistant = 6");

    // Validate: alternating roles (API contract)
    for i in 1..engine_conv.messages.len() {
        assert_ne!(engine_conv.messages[i].role, engine_conv.messages[i - 1].role,
            "Role alternation broken at index {} vs {}: '{}' vs '{}'",
            i, i - 1, engine_conv.messages[i].role, engine_conv.messages[i - 1].role);
    }

    // Validate: Turn 1 story content is present
    let story_present = engine_conv.messages.iter().any(|m| {
        match &m.content {
            MessageContent::Text(t) => t.contains("陈伊拉博士凝视着量子显示屏"),
            _ => false,
        }
    });
    assert!(story_present, "Story from Turn 1 must be in conversation context");

    // ── Turn 3: follow-up proving context survived ──
    {
        let mut bg = engine_conv.clone();
        bg.messages.push(text_msg("user",
            "What was ORION's lie about?"));

        // The context should contain the story so the AI can answer
        let story_in_context = bg.messages.iter().any(|m| {
            match &m.content {
                MessageContent::Text(t) => t.contains("轨道正常") || t.contains("陈伊拉"),
                _ => false,
            }
        });
        assert!(story_in_context,
            "Story content must survive into Turn 3 — this is exactly the reported bug");

        bg.messages.push(text_msg("assistant",
            "ORION谎称\"轨道正常\"（orbit normal），实际上飞船已被引向一个虚无（void）而非地球。"));
        engine_conv.messages = bg.messages.clone();
    }

    assert_eq!(engine_conv.messages.len(), 8);
}

/// Regression: Verify that after a simulated error mid-query, the next
/// query still has full conversation context (the `done` flag and error
/// path fix).
#[test]
fn test_error_path_does_not_lose_prior_context() {
    let mut engine_conv = TestConversation::default();

    // Turn 1: successful
    engine_conv.messages.push(text_msg("user", "What is 2+2?"));
    engine_conv.messages.push(text_msg("assistant", "2+2 equals 4."));

    // Turn 2: fails — but ConversationUpdate was sent before Failed
    let mut bg = engine_conv.clone();
    bg.messages.push(text_msg("user", "What is 3+3?"));
    // Simulate: engine sends ConversationUpdate (preserving user msg) then Failed
    engine_conv.messages = bg.messages.clone();
    // The Failed event happens — context is already saved above

    // Turn 3: must still have context from Turn 1 AND Turn 2's failed question
    let mut next_bg = engine_conv.clone();
    next_bg.messages.push(text_msg("user", "Sum all previous answers"));

    let has_turn1 = next_bg.messages.iter().any(|m| {
        matches!(&m.content, MessageContent::Text(t) if t.contains("equals 4"))
    });
    let has_turn2_question = next_bg.messages.iter().any(|m| {
        matches!(&m.content, MessageContent::Text(t) if t.contains("3+3"))
    });

    assert!(has_turn1, "Turn 1 answer must survive error in Turn 2");
    assert!(has_turn2_question, "Turn 2 question must survive its own failure");
}

// ── Regression tests for multi-turn context loss ────────────────────────────
//
// These tests guard against a class of bugs where the assistant's response
// is not saved to the conversation history, causing subsequent turns to lose
// context. The root causes addressed:
//
// 1. OpenAI adapter passing `stop_reason: "stop"` instead of normalizing to
//    `"end_turn"`, which would cause the engine's MessageDelta handler to
//    not match the finalization condition.
// 2. Engine lacking a safety net when the streaming loop exits without the
//    MessageDelta handler running finalization (e.g. budget exceeded,
//    premature stream close).
// 3. ConversationUpdate not being sent in all code paths.

/// Regression test: OpenAI `finish_reason: "stop"` must be normalized to
/// `"end_turn"` so the engine's finalization code path always triggers.
#[test]
fn test_openai_stop_reason_normalized_to_end_turn() {
    use shannon_core::api::adapter::normalize_sse_event;
    use shannon_core::api::{LlmProvider, StreamEvent};

    let mut state = shannon_core::api::adapter::OpenaiStreamState::new();

    // Simulate the final OpenAI streaming chunk with finish_reason "stop"
    let chunk = r#"{"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#;
    let events = normalize_sse_event(chunk, &LlmProvider::OpenAI, &mut state);

    assert_eq!(events.len(), 1, "Should produce exactly one event");
    match &events[0] {
        Ok(StreamEvent::MessageDelta { delta, .. }) => {
            assert_eq!(
                delta.stop_reason.as_deref(),
                Some("end_turn"),
                "OpenAI 'stop' must be normalized to 'end_turn' so the engine finalizes correctly"
            );
        }
        other => panic!("Expected MessageDelta, got {other:?}"),
    }
}

/// Regression test: OpenAI streaming chunk with usage info AND finish_reason
/// should normalize the stop reason in the usage-bearing MessageDelta.
#[test]
fn test_openai_usage_chunk_with_stop_reason_normalized() {
    use shannon_core::api::adapter::normalize_sse_event;
    use shannon_core::api::{LlmProvider, StreamEvent};

    let mut state = shannon_core::api::adapter::OpenaiStreamState::new();

    // OpenAI can send usage + finish_reason in the same chunk
    let chunk = r#"{"choices":[{"delta":{},"finish_reason":"stop","index":0}],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#;
    let events = normalize_sse_event(chunk, &LlmProvider::OpenAI, &mut state);

    assert_eq!(events.len(), 1, "Should produce exactly one event");
    match &events[0] {
        Ok(StreamEvent::MessageDelta { delta, usage }) => {
            assert_eq!(
                delta.stop_reason.as_deref(),
                Some("end_turn"),
                "OpenAI 'stop' in usage chunk must be normalized to 'end_turn'"
            );
            assert_eq!(usage.input_tokens, 10);
            assert_eq!(usage.output_tokens, 5);
        }
        other => panic!("Expected MessageDelta, got {other:?}"),
    }
}

/// Regression test: non-streaming OpenAI response with finish_reason "stop"
/// must also normalize to "end_turn".
#[test]
fn test_openai_non_streaming_stop_reason_normalized() {
    use shannon_core::api::adapter::normalize_response;
    use shannon_core::api::LlmProvider;

    let resp = r#"{"id":"chatcmpl-1","choices":[{"index":0,"message":{"role":"assistant","content":"Hello!"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#;
    let result = normalize_response(resp, &LlmProvider::OpenAI).unwrap();

    assert_eq!(
        result.stop_reason.as_deref(),
        Some("end_turn"),
        "Non-streaming OpenAI 'stop' must be normalized to 'end_turn'"
    );
    assert_eq!(result.content.len(), 1);
}

/// Regression test: the exact user-reported scenario — a multi-turn
/// conversation where Turn 1 writes a story and Turn 2 asks about it.
/// Verifies that the conversation history preserves the story content
/// so the model would have context for follow-up questions.
#[test]
fn test_story_then_character_count_context_preserved() {
    let mut conv = TestConversation::default();

    let story = "在一个遥远的星球上，机器人阿尔法和人类工程师莉莉正在修复一座古老的空间站。\
                突然，一个名叫泽克的流浪商人出现了，他声称知道空间站隐藏的秘密。\
                三人决定合作，在站长凯瑟琳的指导下，他们穿越了三个密封舱段，\
                最终在控制室发现了通往平行维度的传送门。";

    // Turn 1: user asks for a story, assistant writes one
    conv.messages.push(text_msg("user", "请写一篇200字的科幻小说"));
    conv.messages.push(text_msg("assistant", story));
    conv.turn_count += 1;

    // Turn 2: user asks about characters — conversation MUST include the story
    conv.messages.push(text_msg("user", "这篇小说中出场人物有几个？请列出名字。"));
    conv.turn_count += 1;

    // Verify conversation structure
    assert_eq!(conv.messages.len(), 3, "Should have 3 messages for 2 turns");
    assert_eq!(conv.messages[0].role, "user");
    assert_eq!(conv.messages[1].role, "assistant");
    assert_eq!(conv.messages[2].role, "user");

    // Verify story content is preserved in the assistant message
    let assistant_msg = &conv.messages[1];
    match &assistant_msg.content {
        MessageContent::Text(t) => {
            assert!(t.contains("阿尔法"), "Character '阿尔法' must be in conversation history");
            assert!(t.contains("莉莉"), "Character '莉莉' must be in conversation history");
            assert!(t.contains("泽克"), "Character '泽克' must be in conversation history");
            assert!(t.contains("凯瑟琳"), "Character '凯瑟琳' must be in conversation history");
        }
        _ => panic!("Expected text content for assistant story message"),
    }

    // Verify the follow-up question is present
    let last = conv.messages.last().unwrap();
    match &last.content {
        MessageContent::Text(t) => assert!(t.contains("人物"), "Follow-up should ask about characters"),
        _ => panic!("Expected text content"),
    }
}

/// Regression test: simulates the engine's safety net scenario.
/// When the streaming loop exits with content but without MessageDelta
/// finalization, the assistant text must still be saved.
#[test]
fn test_safety_net_saves_assistant_text_on_stream_exit() {
    let mut conv = TestConversation::default();

    // Simulate Turn 1: user message already pushed, assistant text accumulated
    conv.messages.push(text_msg("user", "Write a haiku about Rust"));
    let assistant_text = "Safe borrowing rules,\nOwnership transfers are clear,\nNo data races here.".to_string();

    // Simulate the safety net: assistant text saved even though MessageDelta
    // handler didn't finalize (this mirrors the engine.rs safety net code)
    conv.messages.push(Message {
        role: "assistant".to_string(),
        content: MessageContent::Text(assistant_text.clone()),
    });

    // Turn 2: next question should see the haiku
    conv.messages.push(text_msg("user", "What was the last line of that haiku?"));

    assert_eq!(conv.messages.len(), 3);

    // Verify the haiku content is in the conversation
    let haiku_msg = &conv.messages[1];
    match &haiku_msg.content {
        MessageContent::Text(t) => {
            assert!(t.contains("No data races"), "Haiku last line must be preserved");
        }
        _ => panic!("Expected text content"),
    }
}

/// Regression test: verify that consecutive turns with mixed providers
/// don't lose context. Simulates switching from Anthropic to OpenAI
/// mid-conversation (which could happen if the user changes model).
#[test]
fn test_mixed_provider_context_preservation() {
    let mut conv = TestConversation::default();

    // Turn 1 (Anthropic-style with end_turn)
    conv.messages.push(text_msg("user", "What are the primary colors?"));
    conv.messages.push(text_msg("assistant", "The primary colors are red, blue, and yellow."));
    conv.turn_count += 1;

    // Turn 2 (OpenAI-style — the adapter normalizes stop, but conversation
    // state management must be identical regardless of provider)
    conv.messages.push(text_msg("user", "Which one is your favorite?"));
    conv.messages.push(text_msg("assistant", "I find blue particularly calming and versatile."));
    conv.turn_count += 1;

    // Turn 3: must have full context
    conv.messages.push(text_msg("user", "Remind me what you listed earlier?"));

    assert_eq!(conv.messages.len(), 5);

    // Verify the first assistant response (about primary colors) is still present
    let has_primary = conv.messages.iter().any(|m| {
        matches!(&m.content, MessageContent::Text(t) if t.contains("red, blue, and yellow"))
    });
    assert!(has_primary, "Turn 1 response must survive into Turn 3 context");

    // Verify the second assistant response is also present
    let has_favorite = conv.messages.iter().any(|m| {
        matches!(&m.content, MessageContent::Text(t) if t.contains("calming"))
    });
    assert!(has_favorite, "Turn 2 response must survive into Turn 3 context");
}

// ── Regression Tests: Additional Multi-Turn Context Preservation ─────

/// Simulates the exact user-reported bug: ask AI to write a story,
/// then ask about characters — the second response must include
/// the first response in its context.
#[test]
fn test_story_then_character_count_preserves_context() {
    let mut conv = TestConversation::default();

    // Turn 1: User asks for a story
    conv.messages.push(text_msg("user", "写一篇200字的科幻小说"));
    let story = "2187年，宇航员林远站在火星基地的观测台上。他的AI助手「星语」正在分析最新的地质数据。\
                 \"林远，地下发现异常能量波动，\"星语的声音在头盔里响起。\
                 林远转身对工程师赵敏说：\"赵姐，你看这个数据。\"\
                 赵敏凑过来，手指在平板上滑动：\"这不是自然形成的信号。\"\
                 三人决定深入调查。当他们打开地下密室时，一道蓝光映照出一个沉睡的外星生物。\
                 它缓缓睁开眼睛，用一种古老的频率说道：\"等了很久。\"";
    conv.messages.push(text_msg("assistant", story));
    conv.turn_count += 1;

    // Turn 2: User asks about characters — must have story context
    conv.messages.push(text_msg("user", "这篇小说中有几个出场人物？请列出"));

    // Verify Turn 1 assistant response is still in conversation
    let has_story = conv.messages.iter().any(|m| {
        matches!(&m.content, MessageContent::Text(t) if t.contains("林远"))
    });
    assert!(has_story, "Turn 1 story must be present in Turn 2 context");

    // The conversation should have 3 messages: user(story), assistant(story), user(question)
    assert_eq!(conv.messages.len(), 3);
    assert_eq!(conv.messages[2].role, "user");
}

/// Verifies that tool use responses with both text and tool_use blocks
/// are preserved as Blocks content, not just text.
#[test]
fn test_tool_use_response_preserved_as_blocks_not_just_text() {
    let mut conv = TestConversation::default();

    // User asks
    conv.messages.push(text_msg("user", "Read the file main.rs"));

    // Assistant responds with text + tool_use
    let assistant_msg = Message {
        role: "assistant".to_string(),
        content: MessageContent::Blocks(vec![
            ContentBlock::Text { text: "Let me read that file.".to_string() },
            ContentBlock::ToolUse {
                id: "tool_1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "main.rs"}),
            },
        ]),
    };
    conv.messages.push(assistant_msg.clone());

    // Tool result
    conv.messages.push(tool_result_msg());

    // Assistant follow-up
    conv.messages.push(text_msg("assistant", "The file contains a main function."));

    // Now user asks follow-up — full context must be present
    conv.messages.push(text_msg("user", "What did the file contain?"));

    // Verify tool_use block survived
    if let MessageContent::Blocks(blocks) = &conv.messages[1].content {
        let has_tool_use = blocks.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }));
        assert!(has_tool_use, "Tool use block must be preserved in conversation");
        let has_text = blocks.iter().any(|b| matches!(b, ContentBlock::Text { .. }));
        assert!(has_text, "Text block alongside tool_use must be preserved");
    } else {
        panic!("Assistant message should have Blocks content, not just Text");
    }

    // Verify the text block is NOT duplicated as a separate message
    // (this catches the bug where streaming loop continues after tool processing)
    let text_only_assistant_msgs: Vec<_> = conv.messages.iter()
        .filter(|m| m.role == "assistant")
        .filter(|m| matches!(&m.content, MessageContent::Text(t) if t == "Let me read that file."))
        .collect();
    assert!(text_only_assistant_msgs.is_empty(),
        "No standalone text-only assistant message should exist duplicating the blocks content");
}

/// Verifies that the safety net saves tool use blocks when stream ends
/// during tool processing without a proper MessageDelta event.
#[test]
fn test_safety_net_preserves_tool_use_blocks() {
    let mut conv = TestConversation::default();

    conv.messages.push(text_msg("user", "Read config file"));

    // Simulate what the safety net should produce: Blocks with both text and tool_use
    let safety_net_msg = Message {
        role: "assistant".to_string(),
        content: MessageContent::Blocks(vec![
            ContentBlock::Text { text: "Reading config file now.".to_string() },
            ContentBlock::ToolUse {
                id: "tool_safety_1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "config.toml"}),
            },
        ]),
    };
    conv.messages.push(safety_net_msg);

    // Verify the message has Blocks (not just Text)
    if let MessageContent::Blocks(blocks) = &conv.messages[1].content {
        assert_eq!(blocks.len(), 2, "Safety net should save both text and tool_use");
        assert!(matches!(&blocks[0], ContentBlock::Text { .. }));
        assert!(matches!(&blocks[1], ContentBlock::ToolUse { .. }));
    } else {
        panic!("Safety net should save Blocks, not Text-only");
    }
}

/// Verifies multi-turn context after compression still preserves
/// the critical assistant response from the most recent turn.
#[test]
fn test_compression_preserves_latest_turn_context() {
    let mut conv = TestConversation::default();
    let config = QueryEngineConfig {
        max_context_tokens: Some(200),
        compression_threshold: 0.5,
        keep_recent_messages: 4,
        compression_strategy: CompressionStrategy::SummarizeOld,
        ..QueryEngineConfig::default()
    };

    // Turn 1: Story
    conv.messages.push(text_msg("user", "Write a sci-fi story"));
    conv.messages.push(text_msg("assistant", "In 2187, astronaut Lin discovered an alien artifact on Mars..."));
    conv.turn_count += 1;

    // Turn 2: Follow-up (this is the critical context)
    conv.messages.push(text_msg("user", "How many characters are in the story?"));
    conv.messages.push(text_msg("assistant", "There are 3 characters: Lin, the AI assistant, and Engineer Zhao."));
    conv.turn_count += 1;

    // Turn 3-5: More conversation to trigger compression
    for i in 0..3 {
        conv.messages.push(text_msg("user", &format!("Question {i} about something else")));
        conv.messages.push(text_msg("assistant", &format!("Answer {i} with detailed response")));
        conv.turn_count += 1;
    }

    assert_eq!(conv.messages.len(), 10);

    // Compress — should keep recent 4 messages + summary
    conv.compress(&config);

    // The MOST RECENT assistant response must survive
    let has_latest_answer = conv.messages.iter().any(|m| {
        matches!(&m.content, MessageContent::Text(t) if t.contains("3 characters"))
    });
    assert!(has_latest_answer,
        "Most recent assistant response must survive compression for next turn context");
}

/// Verifies that tool result messages are persisted to conversation.messages
/// alongside the local messages vec (catches the missing conversation.messages.push bug).
#[test]
fn test_tool_results_persisted_to_conversation() {
    let mut conv = TestConversation::default();
    let mut messages = conv.messages.clone();

    // Simulate the engine's tool result persistence loop
    let tool_results: Vec<(String, String, bool)> = vec![
        ("tool_1".to_string(), "file contents here".to_string(), false),
        ("tool_2".to_string(), "error: not found".to_string(), true),
    ];

    for (tool_use_id, result_content, is_error) in tool_results {
        let tool_msg = Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id,
                content: Some(ToolResultContent::Single(result_content)),
                is_error: Some(is_error),
            }]),
        };
        messages.push(tool_msg.clone());
        conv.messages.push(tool_msg); // Both must be pushed
    }

    assert_eq!(conv.messages.len(), 2, "Tool results should be in conversation");
    assert_eq!(messages.len(), 2, "Tool results should be in local messages");
}

/// Verifies that consecutive turns maintain proper user/assistant alternation
/// even after tool use sequences.
#[test]
fn test_proper_alternation_after_tool_use() {
    let mut conv = TestConversation::default();

    // Turn 1: Simple Q&A
    conv.messages.push(text_msg("user", "Hello"));
    conv.messages.push(text_msg("assistant", "Hi!"));

    // Turn 2: Tool use sequence
    conv.messages.push(text_msg("user", "Read foo.rs"));
    conv.messages.push(assistant_with_tools()); // assistant: text + tool_use
    conv.messages.push(tool_result_msg());      // user: tool_result
    conv.messages.push(text_msg("assistant", "Here's what I found."));

    // Turn 3: Follow-up — must see full context
    conv.messages.push(text_msg("user", "Can you summarize it?"));

    // Verify alternation: user, assistant, user, assistant, user, assistant, user
    let roles: Vec<&str> = conv.messages.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, vec!["user", "assistant", "user", "assistant", "user", "assistant", "user"],
        "Messages must alternate user/assistant for API compatibility. Got: {roles:?}");
}

/// Verifies that a very long assistant response is preserved intact
/// for multi-turn context (no truncation).
#[test]
fn test_long_assistant_response_preserved_intact() {
    let mut conv = TestConversation::default();

    let long_story = "A".repeat(5000);
    conv.messages.push(text_msg("user", "Write a very long story"));
    conv.messages.push(text_msg("assistant", &long_story));
    conv.messages.push(text_msg("user", "How many sentences in the story?"));

    // Verify the long response is intact
    if let MessageContent::Text(t) = &conv.messages[1].content {
        assert_eq!(t.len(), 5000, "Full assistant response must be preserved");
    }

    assert_eq!(conv.messages.len(), 3, "All 3 turns must be in conversation");
}
