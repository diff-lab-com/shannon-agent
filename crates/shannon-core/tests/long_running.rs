//! Long-running scenario stress tests for Shannon Code.
//!
//! These tests simulate extended sessions (50-100+ turns) to verify:
//! - Context compaction keeps request sizes bounded
//! - Memory stability over many turns
//! - Error recovery chains work correctly
//! - Tool call history is maintained consistently
//!
//! All tests are marked `#[ignore]` and only run via `test-release.sh`
//! or explicitly with `cargo test -- --ignored`.

use serde_json::json;
use shannon_core::api::{ContentBlock, Message, MessageContent, ToolResultContent};
use shannon_core::compact::helpers::estimate_tokens;
use shannon_core::query_engine::{CompressionStrategy, QueryEngineConfig};
use shannon_core::testing::mock_dsl::*;
use shannon_core::testing::snapshot::*;

// ── Message helpers ──────────────────────────────────────────────────

fn user_msg(text: &str) -> Message {
    Message {
        role: "user".to_string(),
        content: MessageContent::Text(text.to_string()),
    }
}

fn assistant_text_msg(text: &str) -> Message {
    Message {
        role: "assistant".to_string(),
        content: MessageContent::Text(text.to_string()),
    }
}

fn tool_use_msg(id: usize, name: &str, input: serde_json::Value) -> Message {
    Message {
        role: "assistant".to_string(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
            id: format!("toolu_{id}"),
            name: name.to_string(),
            input,
        }]),
    }
}

fn tool_result_msg(tool_use_id: usize, result: &str, is_error: bool) -> Message {
    Message {
        role: "user".to_string(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: format!("toolu_{tool_use_id}"),
            content: Some(ToolResultContent::Single(result.to_string())),
            is_error: Some(is_error),
        }]),
    }
}

// ── Conversation simulator ──────────────────────────────────────────

/// Simulates a multi-turn conversation with compaction, tracking
/// approximate context sizes at each turn.
struct ConversationSimulator {
    messages: Vec<Message>,
    config: QueryEngineConfig,
    turn: usize,
}

impl ConversationSimulator {
    fn new(config: QueryEngineConfig) -> Self {
        Self {
            messages: Vec::new(),
            config,
            turn: 0,
        }
    }

    fn add_user(&mut self, text: &str) {
        self.messages.push(user_msg(text));
    }

    fn add_assistant(&mut self, text: &str) {
        self.messages.push(assistant_text_msg(text));
        self.turn += 1;
    }

    fn add_tool_cycle(
        &mut self,
        tool_name: &str,
        tool_input: serde_json::Value,
        result: &str,
        is_error: bool,
    ) {
        let id = self.turn + 1;
        self.messages.push(tool_use_msg(id, tool_name, tool_input));
        self.messages.push(tool_result_msg(id, result, is_error));
        self.turn += 1;
    }

    fn add_assistant_after_tool(&mut self, text: &str) {
        self.messages.push(assistant_text_msg(text));
    }

    fn estimate_tokens(&self) -> usize {
        estimate_tokens(&self.messages)
    }

    /// Apply compaction using the engine's strategy, returning the new token count.
    fn compact_if_needed(&mut self) -> bool {
        if self.messages.len() <= self.config.keep_recent_messages + 1 {
            return false;
        }
        let split_point = self
            .messages
            .len()
            .saturating_sub(self.config.keep_recent_messages);
        match self.config.compression_strategy {
            CompressionStrategy::TruncateOldest => {
                self.messages.drain(..split_point);
            }
            CompressionStrategy::SummarizeOld => {
                let old: Vec<Message> = self.messages.drain(..split_point).collect();
                let summary = summarize_messages(&old);
                let summary_msg = Message {
                    role: "system".to_string(),
                    content: MessageContent::Text(format!(
                        "[Previous conversation summary]\n\n{summary}"
                    )),
                };
                self.messages.insert(0, summary_msg);
            }
        }
        true
    }

    fn needs_compression(&self) -> bool {
        if let Some(max_tokens) = self.config.max_context_tokens {
            let threshold = (max_tokens as f32 * self.config.compression_threshold) as usize;
            self.estimate_tokens() > threshold
        } else {
            false
        }
    }
}

fn summarize_messages(messages: &[Message]) -> String {
    let mut parts = Vec::new();
    for msg in messages {
        match &msg.content {
            MessageContent::Text(text) => {
                let role = if msg.role == "user" {
                    "User"
                } else {
                    "Assistant"
                };
                let preview = if text.len() > 100 {
                    format!("{}...", &text[..100])
                } else {
                    text.clone()
                };
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
    format!(
        "Summary of {} messages:\n{}",
        messages.len(),
        parts.join("\n")
    )
}

/// Build mock responses for N turns of refactoring (alternating text + tool calls).
fn build_refactoring_responses(n: usize) -> Vec<MockResponse> {
    let mut responses = Vec::with_capacity(n);
    for i in 0..n {
        if i % 3 == 0 {
            // Every 3rd turn: tool call to read a file
            responses.push(tool_call_response(
                &format!("toolu_{i}"),
                "Read",
                json!({"path": format!("src/module_{}.rs", i / 3)}),
            ));
        } else if i % 3 == 1 {
            // Next turn: tool call to edit
            responses.push(tool_call_response(
                &format!("toolu_{i}"),
                "Edit",
                json!({"path": format!("src/module_{}.rs", i / 3), "old": "old_code()", "new": "new_code()"}),
            ));
        } else {
            // Text summary
            responses.push(text_response(&format!(
                "I've updated module_{} with the refactored code. The changes improve {}.",
                i / 3,
                match i % 5 {
                    0 => "readability",
                    1 => "performance",
                    2 => "error handling",
                    3 => "type safety",
                    _ => "test coverage",
                }
            )));
        }
    }
    responses
}

/// Build mock responses for N turns of debugging (Read → Bash error → Read → Edit → Bash success).
fn build_debugging_responses(n: usize) -> Vec<MockResponse> {
    let mut responses = Vec::with_capacity(n);
    for i in 0..n {
        match i % 5 {
            0 => responses.push(tool_call_response(
                &format!("toolu_{i}"), "Read",
                json!({"path": format!("src/bug_{}.rs", i / 5)}),
            )),
            1 => responses.push(tool_call_response(
                &format!("toolu_{i}"), "Bash",
                json!({"command": "cargo test"}),
            )),
            2 => responses.push(text_response(&format!(
                "Test failed with error: assertion failed at src/bug_{}.rs:{}",
                i / 5, (i % 20) * 5 + 10,
            ))),
            3 => responses.push(tool_call_response(
                &format!("toolu_{i}"), "Edit",
                json!({"path": format!("src/bug_{}.rs", i / 5), "old": "assert!(false)", "new": "assert!(true)"}),
            )),
            _ => responses.push(text_response(&format!(
                "Fixed the bug in bug_{}. Tests now pass.",
                i / 5,
            ))),
        }
    }
    responses
}

// ── Tests ────────────────────────────────────────────────────────────

#[test]
#[ignore] // Run via test-release.sh only
fn test_100_turn_refactoring_session() {
    let config = QueryEngineConfig {
        max_context_tokens: Some(50_000),
        compression_threshold: 0.6,
        keep_recent_messages: 20,
        compression_strategy: CompressionStrategy::SummarizeOld,
        ..Default::default()
    };
    let mut sim = ConversationSimulator::new(config);

    let responses = build_refactoring_responses(100);
    assert_eq!(responses.len(), 100, "Should generate 100 mock responses");

    // Snapshot sizes at key checkpoints
    let mut checkpoint_tokens: Vec<(usize, usize)> = Vec::new();

    for (i, response) in responses.iter().enumerate() {
        // Add user message
        sim.add_user(&format!("Turn {i}: refactor module {i}"));

        // Add assistant response (text or tool)
        match &response.content_blocks.first() {
            Some(MockContentBlock::ToolUse { name, input, .. }) => {
                let result = format!("File content for turn {i}");
                sim.add_tool_cycle(name, input.clone(), &result, false);
                sim.add_assistant_after_tool(&format!("Completed tool call on turn {i}"));
            }
            Some(MockContentBlock::Text { text }) => {
                sim.add_assistant(text);
            }
            _ => {
                sim.add_assistant(&format!("Response {i}"));
            }
        }

        // Apply compaction if needed
        if sim.needs_compression() {
            sim.compact_if_needed();
        }

        // Record checkpoints
        if i == 49 || i == 99 {
            checkpoint_tokens.push((i + 1, sim.estimate_tokens()));
        }
    }

    assert_eq!(sim.turn, 100, "Should have completed 100 turns");
    assert_eq!(checkpoint_tokens.len(), 2, "Should have 2 checkpoints");

    let tokens_at_50 = checkpoint_tokens[0].1;
    let tokens_at_100 = checkpoint_tokens[1].1;

    // Key assertion: compaction should keep growth sub-linear.
    // At 100 turns, token count should be less than 2x the count at 50 turns.
    assert!(
        tokens_at_100 < tokens_at_50 * 2,
        "Context grew too large: turn 50 = {tokens_at_50} tokens, turn 100 = {tokens_at_100} tokens (ratio > 2x)",
    );
}

#[test]
#[ignore] // Long-running stress test (50+ turns) — run via `scripts/test-release.sh` or `cargo test -- --ignored`
fn test_50_turn_debugging_session() {
    let config = QueryEngineConfig {
        max_context_tokens: Some(40_000),
        compression_threshold: 0.7,
        keep_recent_messages: 15,
        compression_strategy: CompressionStrategy::SummarizeOld,
        ..Default::default()
    };
    let mut sim = ConversationSimulator::new(config);

    let responses = build_debugging_responses(50);
    let mut tool_chain: Vec<(String, serde_json::Value, String, bool)> = Vec::new();
    let mut error_count = 0;
    let mut success_count = 0;

    for (i, response) in responses.iter().enumerate() {
        sim.add_user(&format!("Debug issue #{i}"));

        match &response.content_blocks.first() {
            Some(MockContentBlock::ToolUse { name, input, .. }) => {
                let is_error = (i % 5) == 1; // Bash calls in position 1 produce errors
                let result = if is_error {
                    error_count += 1;
                    format!("error[E0425]: cannot find value in scope at turn {i}")
                } else {
                    success_count += 1;
                    format!("ok: completed at turn {i}")
                };
                sim.add_tool_cycle(name, input.clone(), &result, is_error);
                sim.add_assistant_after_tool(&format!("Debug step {i} completed"));
                tool_chain.push((name.clone(), input.clone(), result, is_error));
            }
            Some(MockContentBlock::Text { text }) => {
                sim.add_assistant(text);
            }
            _ => {
                sim.add_assistant(&format!("Response {i}"));
            }
        }

        if sim.needs_compression() {
            sim.compact_if_needed();
        }
    }

    assert_eq!(sim.turn, 50, "Should complete 50 debugging turns");

    // Verify error recovery chain: should have both errors and successes
    assert!(error_count > 0, "Should have encountered errors");
    assert!(success_count > 0, "Should have had successful operations");

    // Verify tool chain was recorded correctly
    assert!(!tool_chain.is_empty(), "Tool chain should not be empty");
    let chain_snapshot = snapshot_tool_chain(&tool_chain);
    assert!(
        chain_snapshot.contains("[ERROR]"),
        "Snapshot should show errors"
    );
    assert!(
        chain_snapshot.contains("[OK]"),
        "Snapshot should show successes"
    );

    // Verify the last few operations are successful (recovery happened)
    let last_5: Vec<_> = tool_chain.iter().rev().take(5).collect();
    let has_success_in_last_5 = last_5.iter().any(|(_, _, _, is_error)| !is_error);
    assert!(
        has_success_in_last_5,
        "Should have recovered from errors in recent turns"
    );
}

#[test]
#[ignore] // Long-running stress test (80 turns) — run via `scripts/test-release.sh` or `cargo test -- --ignored`
fn test_80_turn_feature_development() {
    let config = QueryEngineConfig {
        max_context_tokens: Some(60_000),
        compression_threshold: 0.65,
        keep_recent_messages: 25,
        compression_strategy: CompressionStrategy::SummarizeOld,
        ..Default::default()
    };
    let mut sim = ConversationSimulator::new(config);

    let mut write_count = 0;
    let mut edit_count = 0;
    let mut test_count = 0;
    let mut fix_count = 0;

    for i in 0..80 {
        sim.add_user(&format!(
            "Feature step {}: {}",
            i,
            match i % 4 {
                0 => "Create new module",
                1 => "Add functionality",
                2 => "Write tests",
                _ => "Fix issues",
            }
        ));

        match i % 4 {
            0 => {
                write_count += 1;
                sim.add_tool_cycle("Write",
                    json!({"path": format!("src/feature_{}.rs", i / 4), "content": format!("pub fn feature_{}() {{}}", i / 4)}),
                    "File written successfully", false);
                sim.add_assistant_after_tool(&format!("Created feature module {}", i / 4));
            }
            1 => {
                edit_count += 1;
                sim.add_tool_cycle("Edit",
                    json!({"path": format!("src/feature_{}.rs", i / 4), "old": "{}", "new": "pub fn new_fn() { /* impl */ }"}),
                    "File edited successfully", false);
                sim.add_assistant_after_tool(&format!("Added functionality to module {}", i / 4));
            }
            2 => {
                test_count += 1;
                let test_passed = i % 8 != 2; // Some tests fail
                let result_msg = if test_passed {
                    format!("Tests pass for feature {}", i / 4)
                } else {
                    format!("Tests failed for feature {}, fixing...", i / 4)
                };
                sim.add_tool_cycle(
                    "Bash",
                    json!({"command": format!("cargo test feature_{}", i / 4)}),
                    if test_passed {
                        "All tests passed"
                    } else {
                        "1 test failed"
                    },
                    !test_passed,
                );
                sim.add_assistant_after_tool(&result_msg);
            }
            _ => {
                fix_count += 1;
                sim.add_tool_cycle("Edit",
                    json!({"path": format!("src/feature_{}.rs", i / 4), "old": "/* impl */", "new": "// fixed"}),
                    "Fix applied", false);
                sim.add_assistant_after_tool(&format!("Fixed issues in feature {}", i / 4));
            }
        }

        if sim.needs_compression() {
            sim.compact_if_needed();
        }
    }

    assert_eq!(sim.turn, 80, "Should complete 80 feature development turns");
    assert_eq!(write_count, 20, "Should have 20 Write operations");
    assert_eq!(edit_count, 20, "Should have 20 Edit operations");
    assert_eq!(test_count, 20, "Should have 20 Bash/test operations");
    assert_eq!(fix_count, 20, "Should have 20 fix operations");

    // Verify context is bounded after compaction
    let final_tokens = sim.estimate_tokens();
    let raw_estimate: usize = (0..80)
        .map(|i| {
            let msg_len = 50 + i.to_string().len(); // user msg
            let tool_len = 100; // tool call + result
            let resp_len = 60; // assistant response
            msg_len + tool_len + resp_len
        })
        .sum();
    // Compacted context should be significantly smaller than un-compacted
    assert!(
        final_tokens < raw_estimate / 3,
        "Compacted context ({final_tokens} tokens) should be < 1/3 of raw estimate ({raw_estimate} tokens)",
    );
}

#[test]
#[ignore] // Long-running stress test (50 turns, memory stability) — run via `scripts/test-release.sh` or `cargo test -- --ignored`
fn test_memory_stability_50_turns() {
    // Test that snapshot sizes (as a proxy for memory usage) don't grow linearly
    // over 50 turns with compaction enabled.
    let config = QueryEngineConfig {
        max_context_tokens: Some(30_000),
        compression_threshold: 0.5,
        keep_recent_messages: 10,
        compression_strategy: CompressionStrategy::TruncateOldest,
        ..Default::default()
    };
    let mut sim = ConversationSimulator::new(config);

    let mut snapshot_sizes: Vec<(usize, usize)> = Vec::new();

    for i in 0..50 {
        // Generate variable-sized content to stress memory
        let large_content = "x".repeat(200 + (i % 5) * 100);
        sim.add_user(&format!(
            "Turn {}: {}",
            i,
            &large_content[..50.min(large_content.len())]
        ));
        sim.add_assistant(&large_content);

        if sim.needs_compression() {
            sim.compact_if_needed();
        }

        // Record snapshot size every 10 turns
        if (i + 1) % 10 == 0 {
            let size = sim.estimate_tokens();
            snapshot_sizes.push((i + 1, size));
        }
    }

    assert_eq!(snapshot_sizes.len(), 5, "Should have 5 checkpoints");

    // Verify growth is sub-linear: each 10-turn segment should not double the size
    for window in snapshot_sizes.windows(2) {
        let (turn_a, size_a) = window[0];
        let (turn_b, size_b) = window[1];
        let ratio = size_b as f64 / size_a as f64;
        assert!(
            ratio < 2.0,
            "Context size doubled between turn {turn_a} ({size_a}) and turn {turn_b} ({size_b}): ratio = {ratio:.2}",
        );
    }

    // Overall: size at turn 50 should be less than 5x size at turn 10
    let size_at_10 = snapshot_sizes[0].1;
    let size_at_50 = snapshot_sizes[4].1;
    let overall_ratio = size_at_50 as f64 / size_at_10 as f64;
    assert!(
        overall_ratio < 5.0,
        "Context grew too much: turn 10 = {size_at_10}, turn 50 = {size_at_50}, ratio = {overall_ratio:.2}",
    );
}

#[test]
#[ignore] // Long-running stress test (30 turns, compaction quality) — run via `scripts/test-release.sh` or `cargo test -- --ignored`
fn test_compaction_quality_long_session() {
    // Build a 30-turn session with specific file names and tool results,
    // trigger compaction, then verify key information is preserved.

    let config = QueryEngineConfig {
        max_context_tokens: Some(10_000),
        compression_threshold: 0.5,
        keep_recent_messages: 8,
        compression_strategy: CompressionStrategy::SummarizeOld,
        ..Default::default()
    };
    let mut sim = ConversationSimulator::new(config);

    // Important file names that should survive compaction
    let critical_files = [
        "src/lib.rs",
        "src/main.rs",
        "Cargo.toml",
        "tests/integration.rs",
    ];

    for i in 0..30 {
        let file = critical_files[i % critical_files.len()];
        sim.add_user(&format!("Work on {file}"));
        sim.add_tool_cycle(
            "Read",
            json!({"path": file}),
            &format!("// Contents of {file} at revision {i}"),
            false,
        );
        sim.add_assistant_after_tool(&format!("Reviewed {file}"));
    }

    let pre_compact_tokens = sim.estimate_tokens();

    // Force compaction
    let did_compact = sim.compact_if_needed();

    assert!(did_compact, "Compaction should have been triggered");

    let post_compact_tokens = sim.estimate_tokens();

    // Compacted context should be less than 50% of original
    assert!(
        post_compact_tokens < pre_compact_tokens / 2,
        "Compacted context ({post_compact_tokens}) should be < 50% of original ({pre_compact_tokens})",
    );

    // Verify recent messages are preserved (last 8 turns = 24 messages)
    // Recent messages should contain the last critical file references
    let recent_msgs: Vec<_> = sim.messages.iter().rev().take(24).collect();
    let mut recent_parts: Vec<String> = Vec::new();
    for m in &recent_msgs {
        match &m.content {
            MessageContent::Text(t) => recent_parts.push(t.clone()),
            MessageContent::Blocks(blocks) => {
                for b in blocks {
                    match b {
                        ContentBlock::ToolUse { name, input, .. } => {
                            recent_parts.push(format!("{name}({input})"));
                        }
                        ContentBlock::ToolResult {
                            content: Some(ToolResultContent::Single(s)),
                            ..
                        } => {
                            recent_parts.push(s.clone());
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    let recent_text = recent_parts.join(" ");

    // At least one critical file should be in the recent context
    let found_any = critical_files.iter().any(|f| recent_text.contains(f));
    assert!(
        found_any,
        "At least one critical file name should be preserved in recent context"
    );

    // Verify subsequent turns still work after compaction
    sim.add_user("Continue working on the project");
    sim.add_assistant("Sure, I'll continue. Let me check the current state.");
    sim.add_tool_cycle(
        "Read",
        json!({"path": "src/lib.rs"}),
        "// Updated lib.rs contents",
        false,
    );
    sim.add_assistant_after_tool("The codebase looks good.");

    assert_eq!(sim.turn, 31, "Should continue to 31 turns after compaction");

    // Take a snapshot of the final state for regression tracking
    let final_snapshot = render_request_snapshot(
        &json!({
            "messages": sim.messages.iter().take(5).map(|m| json!({
                "role": m.role,
                "content": match &m.content {
                    MessageContent::Text(t) => json!(t),
                    MessageContent::Blocks(blocks) => json!(blocks.iter().map(|b| match b {
                        ContentBlock::Text { text } => json!({"type": "text", "text": text}),
                        ContentBlock::ToolUse { name, .. } => json!({"type": "tool_use", "name": name}),
                        ContentBlock::ToolResult { tool_use_id, .. } => json!({"type": "tool_result", "id": tool_use_id}),
                        _ => json!({"type": "unknown"}),
                    }).collect::<Vec<_>>()),
                }
            })).collect::<Vec<_>>()
        }),
        RenderMode::KindOnly,
    );

    assert!(
        !final_snapshot.is_empty(),
        "Final snapshot should not be empty"
    );
    assert!(
        final_snapshot.contains("system/text"),
        "Summary message should be present"
    );
}
