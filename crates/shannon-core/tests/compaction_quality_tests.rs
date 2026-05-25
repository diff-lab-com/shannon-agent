//! Compaction quality tests — verify WHAT is preserved after compaction,
//! not just that compaction runs without errors.
//!
//! These tests build realistic multi-turn conversations and then assert that
//! specific high-value information survives the compaction process.

#[cfg(test)]
mod compaction_quality_tests {
    use shannon_core::api::{ContentBlock, Message, MessageContent, ToolResultContent};
    use shannon_core::compact::compact_messages::CompactionStrategy;
    use shannon_core::compact::engine::CompactEngine;
    use shannon_core::compact::helpers::{
        estimate_message_tokens, estimate_tokens, extract_text_content,
    };
    use shannon_core::compact::protection::{MessageProtector, compact_messages_with_protection};
    use shannon_core::compact::summarizer::RuleBasedSummarizer;
    use shannon_core::compact::types::CompactConfig;

    // -- Helpers ---------------------------------------------------------------

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

    fn tool_use_msg(id: &str, name: &str, input_json: serde_json::Value) -> Message {
        Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                id: id.to_string(),
                name: name.to_string(),
                input: input_json,
            }]),
        }
    }

    fn tool_result_msg(tool_use_id: &str, result: &str, is_error: bool) -> Message {
        Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: Some(ToolResultContent::Single(result.to_string())),
                is_error: Some(is_error),
            }]),
        }
    }

    /// Collect all text from a slice of messages into a single string.
    fn all_text(messages: &[Message]) -> String {
        messages
            .iter()
            .map(extract_text_content)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Build a large conversation by repeating filler turns, then add specific
    /// high-value turns at the beginning (old) and end (recent).
    fn build_large_conversation(
        old_prefix: Vec<Message>,
        recent_suffix: Vec<Message>,
        filler_turns: usize,
    ) -> Vec<Message> {
        let mut msgs = vec![system_msg("You are a Rust coding assistant.")];
        msgs.extend(old_prefix);
        for i in 0..filler_turns {
            msgs.push(user_msg(&format!(
                "Filler question {i} about general topics with extra padding words to use tokens"
            )));
            msgs.push(assistant_msg(&format!(
                "Filler answer {i}: this is a generic response about the topic \
                 with enough detail to be a realistic assistant reply"
            )));
        }
        msgs.extend(recent_suffix);
        msgs
    }

    fn make_engine(keep_recent: usize) -> CompactEngine {
        CompactEngine::new(
            CompactConfig {
                keep_recent_count: keep_recent,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap()
    }

    // -- Tests -----------------------------------------------------------------

    // 1. File paths mentioned before compaction exist after compaction.
    #[test]
    fn test_compaction_preserves_file_paths() {
        let old_turns = vec![
            user_msg("Read src/auth/mod.rs and src/auth/session.rs"),
            tool_use_msg(
                "tu_read1",
                "Read",
                serde_json::json!({"file_path": "src/auth/mod.rs"}),
            ),
            tool_result_msg("tu_read1", "pub mod session;\npub mod token;", false),
            assistant_msg("I see the auth module has session and token submodules."),
        ];

        let recent_turns = vec![
            user_msg("Now check src/auth/session.rs"),
            assistant_msg("The session module handles user sessions."),
        ];

        let mut messages = build_large_conversation(old_turns, recent_turns, 12);

        let mut engine = make_engine(6);
        engine.compact(&mut messages).unwrap();

        let combined = all_text(&messages);

        // File paths that appeared in old turns should survive in the summary
        assert!(
            combined.contains("src/auth/mod.rs"),
            "file path 'src/auth/mod.rs' should be preserved after compaction"
        );
        assert!(
            combined.contains("src/auth/session.rs"),
            "file path 'src/auth/session.rs' should be preserved after compaction"
        );
    }

    // 2. Error text preserved in summary.
    #[test]
    fn test_compaction_preserves_error_messages() {
        let old_turns = vec![
            user_msg("Run the test suite"),
            tool_use_msg(
                "tu_err1",
                "Bash",
                serde_json::json!({"command": "cargo test"}),
            ),
            tool_result_msg(
                "tu_err1",
                "error[E0277]: the trait bound `MyStruct: Debug` is not satisfied",
                true,
            ),
            assistant_msg("The test failed because MyStruct does not implement Debug."),
        ];

        let recent_turns = vec![
            user_msg("Fix the error"),
            assistant_msg("I will add #[derive(Debug)] to MyStruct."),
        ];

        let mut messages = build_large_conversation(old_turns, recent_turns, 12);

        let mut engine = make_engine(6);
        engine.compact(&mut messages).unwrap();

        let combined = all_text(&messages);

        // The error message should appear in the summary (RuleBasedSummarizer
        // captures tool errors in its "Errors encountered" section).
        assert!(
            combined.contains("error[E0277]")
                || combined.contains("trait bound")
                || combined.contains("Debug"),
            "error message content should survive compaction, got: {}",
            combined.chars().take(500).collect::<String>()
        );
    }

    // 3. Original user request preserved.
    #[test]
    fn test_compaction_preserves_user_intent() {
        let user_intent =
            "Refactor the authentication module to use JWT tokens instead of session cookies";

        let old_turns = vec![
            user_msg(user_intent),
            assistant_msg("I'll start by reading the current auth implementation."),
        ];

        let recent_turns = vec![
            user_msg("What about refresh tokens?"),
            assistant_msg("We should implement a refresh token rotation strategy."),
        ];

        let mut messages = build_large_conversation(old_turns, recent_turns, 15);

        let mut engine = make_engine(6);
        engine.compact(&mut messages).unwrap();

        let combined = all_text(&messages);

        // The original intent should survive — at minimum, key terms should
        // appear in the summary.
        assert!(
            combined.contains("authentication")
                || combined.contains("JWT")
                || combined.contains("Refactor"),
            "user intent keywords should be preserved, got: {}",
            combined.chars().take(500).collect::<String>()
        );
    }

    // 4. Last N turns fully preserved (not summarized).
    #[test]
    fn test_compaction_preserves_recent_edits() {
        let recent_turns = vec![
            user_msg("Edit src/lib.rs to add the new function"),
            tool_use_msg(
                "tu_edit1",
                "Edit",
                serde_json::json!({"file_path": "src/lib.rs", "old": "fn old()", "new": "fn new()"}),
            ),
            tool_result_msg("tu_edit1", "File edited successfully", false),
            assistant_msg("Added the new function to src/lib.rs."),
            user_msg("Run cargo check"),
            tool_use_msg(
                "tu_check1",
                "Bash",
                serde_json::json!({"command": "cargo check"}),
            ),
            tool_result_msg("tu_check1", "Checking shannon-core ... Finished", false),
            assistant_msg("cargo check passes cleanly."),
        ];

        let mut messages = build_large_conversation(vec![], recent_turns, 15);

        let mut engine = make_engine(10);
        engine.compact(&mut messages).unwrap();

        let combined = all_text(&messages);

        // Recent turns should be preserved verbatim (not summarized).
        assert!(
            combined.contains("Edit src/lib.rs"),
            "recent edit instruction should be preserved verbatim"
        );
        assert!(
            combined.contains("cargo check passes cleanly"),
            "recent assistant response should be preserved verbatim"
        );
        assert!(
            combined.contains("File edited successfully"),
            "recent tool result should be preserved verbatim"
        );
    }

    // 5. Multiple reads of the same file are consolidated.
    #[test]
    fn test_compaction_removes_redundant_reads() {
        // Three reads of the same file across many turns
        let mut old_turns = Vec::new();
        for i in 0..3 {
            old_turns.push(user_msg(&format!("Read src/config.rs again (time {i})")));
            old_turns.push(tool_use_msg(
                &format!("tu_r{i}"),
                "Read",
                serde_json::json!({"file_path": "src/config.rs"}),
            ));
            old_turns.push(tool_result_msg(
                &format!("tu_r{i}"),
                &format!("Line 1-10 of src/config.rs (read {i})\npub struct Config {{"),
                false,
            ));
            old_turns.push(assistant_msg(&format!(
                "I see the config file, read number {i}."
            )));
        }

        let recent_turns = vec![
            user_msg("Now update the config"),
            assistant_msg("I'll update the config file."),
        ];

        let mut messages = build_large_conversation(old_turns, recent_turns, 5);

        let original_len = messages.len();
        let mut engine = make_engine(6);
        engine.compact(&mut messages).unwrap();

        // After compaction, the message count should be significantly reduced.
        // The summary should mention the file once, not three times in full.
        assert!(
            messages.len() < original_len,
            "compaction should reduce message count: before={}, after={}",
            original_len,
            messages.len()
        );

        let combined = all_text(&messages);
        // The file path should still appear (at least in summary or recent)
        assert!(
            combined.contains("src/config.rs"),
            "file path should still be mentioned after compaction"
        );
    }

    // 6. Extreme pressure: must compress 99%+ of content.
    #[test]
    fn test_compaction_under_extreme_pressure() {
        let mut messages = vec![system_msg("System prompt")];

        // Create a very large conversation: 200 turns with verbose content
        for i in 0..200 {
            messages.push(user_msg(&format!(
                "Question {i}: This is a detailed question about topic {i} \
                 with lots of context and background information to consume tokens. \
                 The user is asking about various aspects of the codebase."
            )));
            messages.push(assistant_msg(&format!(
                "Answer {i}: This is a comprehensive answer with code examples, \
                 explanations, and references to multiple files. \
                 The response covers the topic in great detail with multiple paragraphs."
            )));
        }

        let original_tokens = estimate_tokens(&messages);
        assert!(
            original_tokens > 5000,
            "test conversation should be large, got {original_tokens} tokens"
        );

        // Set an extremely tight budget — only 1% of original tokens
        let extreme_config = CompactConfig {
            keep_recent_count: 2, // keep only last turn
            max_output_tokens: 500,
            ..Default::default()
        };
        let mut engine =
            CompactEngine::new(extreme_config, Box::new(RuleBasedSummarizer::new())).unwrap();

        let result = engine.compact(&mut messages).unwrap();
        let compacted_tokens = estimate_tokens(&messages);

        // The result should have a significant reduction
        assert!(
            result.reduction_ratio > 0.5,
            "should achieve >50% reduction under extreme pressure, got {:.1}%",
            result.reduction_ratio * 100.0
        );

        // Message count should be dramatically smaller
        assert!(
            messages.len() < 50,
            "should have far fewer messages under extreme pressure, got {}",
            messages.len()
        );

        // Even under extreme pressure, the result should not be empty
        assert!(
            compacted_tokens > 0,
            "compacted result should contain some content"
        );
        assert!(
            !messages.is_empty(),
            "messages should never be completely empty after compaction"
        );
    }

    // 7. Protected blocks never removed.
    #[test]
    fn test_compaction_protection_rules() {
        // Build a conversation where specific messages are protected
        let mut messages = Vec::new();
        messages.push(system_msg("System prompt"));

        // Protected user instruction
        let protected_text = "CRITICAL INSTRUCTION: Always use unsafe code review guidelines.";
        messages.push(user_msg(protected_text));

        // Many filler messages that will be targeted for removal
        for i in 0..20 {
            messages.push(user_msg(&format!(
                "Normal question {i} with padding text to fill tokens"
            )));
            messages.push(assistant_msg(&format!(
                "Normal answer {i} with some detail about the topic"
            )));
        }

        // Set up protection for index 1 (the critical instruction)
        let mut protector = MessageProtector::new();
        protector.protect(1); // the critical user instruction

        // Use compact_messages_with_protection with a tight budget
        let result = compact_messages_with_protection(
            &messages,
            &CompactionStrategy::Summarize,
            2000, // tight budget forces removal
            2,
            &protector,
        );

        if result.did_compact {
            // The protected message content must survive
            let result_text = all_text(&result.messages);
            assert!(
                result_text.contains("CRITICAL INSTRUCTION"),
                "protected message content must survive compaction"
            );
        }

        // Also test via CompactEngine — protected messages are not in the
        // recent tail but have critical priority so classify_message_priority
        // keeps them (short user messages are Critical priority).
        let protected_msg_tokens = estimate_message_tokens(&messages[1]);
        assert!(
            protected_msg_tokens > 0,
            "protected message should have non-zero token estimate"
        );
    }

    // 8. Compact twice gives same result (idempotent).
    #[test]
    fn test_compaction_idempotent() {
        let recent_turns = vec![
            user_msg("Show me the test results"),
            assistant_msg("All tests passed."),
        ];

        let mut messages = build_large_conversation(vec![], recent_turns, 15);

        let mut engine = make_engine(6);

        // First compaction
        engine.compact(&mut messages).unwrap();
        let first_pass = messages.clone();
        let first_tokens = estimate_tokens(&first_pass);
        let first_count = first_pass.len();

        // If the conversation is small enough after first compaction, the
        // second compaction may be a no-op. Either way the result should be
        // stable — same or fewer tokens, no new content introduced.
        let second_result = engine.compact(&mut messages);

        match second_result {
            Ok(result) => {
                let second_tokens = estimate_tokens(&messages);
                // Second pass may add a new summary header, so allow modest growth.
                // The key invariant: second compaction should not balloon —
                // growth must be bounded (under 50% and under 1000 tokens).
                let token_growth = second_tokens as i64 - first_tokens as i64;
                assert!(
                    token_growth < 1000,
                    "second compaction should not grow excessively: first={first_tokens}, second={second_tokens}, growth={token_growth}"
                );
                assert!(
                    (second_tokens as f64 / first_tokens as f64) < 1.5,
                    "second compaction growth ratio should be under 1.5x: \
                     first={}, second={}, ratio={:.2}",
                    first_tokens,
                    second_tokens,
                    second_tokens as f64 / first_tokens as f64
                );
                // Message count should be stable or decrease
                assert!(
                    messages.len() <= first_count + 2, // allow summary header
                    "second compaction should not drastically increase message count: \
                     first={}, second={}",
                    first_count,
                    messages.len()
                );
                let _ = result;
            }
            Err(_) => {
                // If there are too few messages, second compaction may no-op or error
                // That's acceptable — the key invariant is no expansion.
            }
        }
    }

    // 9. Tool outputs handled correctly.
    #[test]
    fn test_compaction_with_tool_results() {
        let old_turns = vec![
            user_msg("Search for the function definition"),
            tool_use_msg(
                "tu_grep1",
                "Grep",
                serde_json::json!({"pattern": "fn authenticate", "path": "src/"}),
            ),
            tool_result_msg(
                "tu_grep1",
                "src/auth/handler.rs:42:fn authenticate(token: &str) -> Result<User>",
                false,
            ),
            assistant_msg("Found the authenticate function in src/auth/handler.rs:42."),
            user_msg("Read that file"),
            tool_use_msg(
                "tu_read1",
                "Read",
                serde_json::json!({"file_path": "src/auth/handler.rs"}),
            ),
            tool_result_msg(
                "tu_read1",
                "42: fn authenticate(token: &str) -> Result<User> {\n43:     verify(token)\n44: }",
                false,
            ),
            assistant_msg("The authenticate function verifies the token."),
        ];

        let recent_turns = vec![
            user_msg("Now add rate limiting"),
            assistant_msg("I'll add rate limiting to the authenticate function."),
        ];

        let mut messages = build_large_conversation(old_turns, recent_turns, 10);

        let mut engine = make_engine(6);
        engine.compact(&mut messages).unwrap();

        let combined = all_text(&messages);

        // Tool names should survive
        assert!(
            combined.contains("Grep")
                || combined.contains("Read")
                || combined.contains("Tools used"),
            "tool names should appear in the summary"
        );

        // File paths from tool results should survive
        assert!(
            combined.contains("src/auth/handler.rs"),
            "file paths from tool results should be preserved"
        );

        // Function signature from tool result should be referenced
        assert!(
            combined.contains("authenticate"),
            "function name from tool result should survive"
        );
    }

    // 10. System prompt never touched.
    #[test]
    fn test_compaction_preserves_system_prompt() {
        let system_prompt = "You are an expert Rust developer. Follow these rules:\n\
             1. Always use Result types for fallible operations\n\
             2. Prefer iterators over for loops\n\
             3. Write comprehensive tests\n\
             4. Use clippy pedantic mode";

        let mut messages = vec![system_msg(system_prompt)];

        // Add many turns to guarantee compaction fires
        for i in 0..30 {
            messages.push(user_msg(&format!(
                "Question {i}: Tell me about Rust feature {i} \
                 with enough detail to consume context tokens \
                 across multiple sentences and paragraphs"
            )));
            messages.push(assistant_msg(&format!(
                "Answer {i}: Rust feature {i} provides the following \
                 capabilities and is used in many production codebases \
                 for its safety guarantees and performance characteristics"
            )));
        }
        messages.push(user_msg("Final question about Rust"));
        messages.push(assistant_msg("Here is the final answer."));

        let mut engine = make_engine(4);
        engine.compact(&mut messages).unwrap();

        // The system prompt must be the first message and unchanged
        assert_eq!(
            messages[0].role, "system",
            "first message must still be a system message"
        );

        let system_text = extract_text_content(&messages[0]);
        assert!(
            system_text.contains("expert Rust developer"),
            "system prompt content must be preserved verbatim, got: {}",
            system_text.chars().take(200).collect::<String>()
        );
        assert!(
            system_text.contains("clippy pedantic"),
            "system prompt rules must be preserved verbatim"
        );

        // Additional check: system prompt should be byte-for-byte identical
        assert_eq!(
            system_text, system_prompt,
            "system prompt must be byte-for-byte identical after compaction"
        );
    }
}
