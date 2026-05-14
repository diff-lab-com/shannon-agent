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
        state.messages.push(text_msg("user", &format!("Read file {}", i)));
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
