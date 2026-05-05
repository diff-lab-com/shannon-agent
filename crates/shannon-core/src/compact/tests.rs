//! Tests for the context compression module.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ContentBlock, Message, MessageContent, ToolResultContent};
    use std::time::Duration;

    use super::super::compact_messages::{
        compact_messages, CompactionConfig, CompactionStrategy,
    };
    use super::super::engine::CompactEngine;
    use super::super::helpers::{
        estimate_message_tokens, estimate_tokens, extract_text_content, looks_like_code,
        truncate_text,
    };
    use super::super::protection::{classify_message_priority, compact_messages_with_protection, MessageProtector};
    use super::super::summarizer::RuleBasedSummarizer;
    use super::super::types::{
        CompactConfig, CompactError, CompactPrompt, CompactResult, CompactStrategy,
        GroupedMessage, MessageGroup, Summarizer,
    };

    // -- Helper functions for test data --

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

    fn tool_use_msg(id: &str, name: &str, input: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                id: id.to_string(),
                name: name.to_string(),
                input: serde_json::json!({"command": input}),
            }]),
        }
    }

    fn tool_result_msg(tool_use_id: &str, result: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: Some(ToolResultContent::Single(result.to_string())),
                is_error: Some(false),
            }]),
        }
    }

    fn large_user_msg() -> Message {
        // Create a message that exceeds the default micro_compact_threshold of 4096 tokens
        // ~4 chars per token, so we need ~16384 characters
        let long_text = "A".repeat(20000);
        user_msg(&long_text)
    }

    // -- CompactConfig tests --

    #[test]
    fn test_compact_config_default() {
        let config = CompactConfig::default();
        assert_eq!(config.max_output_tokens, 2000);
        assert_eq!(config.keep_recent_count, 10);
        assert!((config.trigger_threshold - 0.8).abs() < 0.001);
        assert!(config.enable_micro_compact);
        assert_eq!(config.micro_compact_threshold, 4096);
        assert!(config.enable_session_memory_compact);
        assert_eq!(config.max_context_tokens, 200_000);
    }

    #[test]
    fn test_compact_config_with_max_context() {
        let config = CompactConfig::with_max_context(100_000);
        assert_eq!(config.max_context_tokens, 100_000);
        assert_eq!(config.keep_recent_count, 10); // other defaults preserved
    }

    #[test]
    fn test_compact_config_validate_ok() {
        let config = CompactConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_compact_config_validate_zero_output_tokens() {
        let config = CompactConfig {
            max_output_tokens: 0,
            ..Default::default()
        };
        assert!(matches!(
            config.validate(),
            Err(CompactError::InvalidConfig(_))
        ));
    }

    #[test]
    fn test_compact_config_validate_zero_keep_count() {
        let config = CompactConfig {
            keep_recent_count: 0,
            ..Default::default()
        };
        assert!(matches!(
            config.validate(),
            Err(CompactError::InvalidConfig(_))
        ));
    }

    #[test]
    fn test_compact_config_validate_bad_threshold() {
        let config = CompactConfig {
            trigger_threshold: 0.0,
            ..Default::default()
        };
        assert!(matches!(
            config.validate(),
            Err(CompactError::InvalidConfig(_))
        ));

        let config = CompactConfig {
            trigger_threshold: 1.5,
            ..Default::default()
        };
        assert!(matches!(
            config.validate(),
            Err(CompactError::InvalidConfig(_))
        ));
    }

    // -- CompactEngine creation --

    #[test]
    fn test_engine_with_defaults() {
        let engine = CompactEngine::with_defaults();
        assert!(engine.is_ok());
        let engine = engine.unwrap();
        assert_eq!(engine.config().keep_recent_count, 10);
    }

    #[test]
    fn test_engine_invalid_config_rejected() {
        let config = CompactConfig {
            max_output_tokens: 0,
            ..Default::default()
        };
        let result = CompactEngine::new(config, Box::new(RuleBasedSummarizer::new()));
        assert!(result.is_err());
    }

    // -- Context analysis --

    #[test]
    fn test_analyze_context_empty() {
        let engine = CompactEngine::with_defaults().unwrap();
        let analysis = engine.analyze_context(&[]);
        assert_eq!(analysis.estimated_tokens, 0);
        assert!(!analysis.should_compact);
        assert_eq!(analysis.compactable_message_count, 0);
        assert_eq!(analysis.micro_compact_candidates, 0);
    }

    #[test]
    fn test_analyze_context_below_threshold() {
        let engine = CompactEngine::with_defaults().unwrap();
        let messages = vec![user_msg("Hello"), assistant_msg("Hi there")];
        let analysis = engine.analyze_context(&messages);
        assert!(!analysis.should_compact);
        assert_eq!(analysis.compactable_message_count, 0);
    }

    #[test]
    fn test_analyze_context_above_threshold() {
        let engine = CompactEngine::new(
            CompactConfig {
                max_context_tokens: 100,
                trigger_threshold: 0.8,
                keep_recent_count: 2,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        // Create messages that exceed the threshold
        let mut messages = Vec::new();
        for i in 0..50 {
            messages.push(user_msg(&format!(
                "This is message number {i} with enough text to accumulate tokens"
            )));
        }

        let analysis = engine.analyze_context(&messages);
        assert!(analysis.should_compact);
        assert!(analysis.compactable_message_count > 0);
    }

    #[test]
    fn test_analyze_context_micro_compact_candidates() {
        let engine = CompactEngine::with_defaults().unwrap();
        let messages = vec![
            user_msg("Small message"),
            large_user_msg(),
            assistant_msg("Normal response"),
        ];
        let analysis = engine.analyze_context(&messages);
        assert_eq!(analysis.micro_compact_candidates, 1);
    }

    // -- Auto-compact check --

    #[test]
    fn test_auto_compact_check_false() {
        let engine = CompactEngine::with_defaults().unwrap();
        let messages = vec![user_msg("Hello"), assistant_msg("Hi")];
        assert!(!engine.auto_compact_check(&messages));
    }

    #[test]
    fn test_auto_compact_check_true() {
        let engine = CompactEngine::new(
            CompactConfig {
                max_context_tokens: 50,
                trigger_threshold: 0.5,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = Vec::new();
        for _ in 0..30 {
            messages.push(user_msg("Long enough message to add tokens"));
        }
        assert!(engine.auto_compact_check(&messages));
    }

    // -- Full compaction --

    #[test]
    fn test_compact_empty_messages() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages: Vec<Message> = Vec::new();
        let result = engine.compact(&mut messages);
        assert!(matches!(result, Err(CompactError::NoMessagesToCompact)));
    }

    #[test]
    fn test_compact_too_few_messages() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![user_msg("Hello"), assistant_msg("Hi")];
        let result = engine.compact(&mut messages);
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.messages_removed, 0);
        assert_eq!(result.strategy, CompactStrategy::SummarizeOld);
        // Messages should be unchanged
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_compact_reduces_messages() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = Vec::new();
        for i in 0..20 {
            messages.push(user_msg(&format!("User message {i}")));
            messages.push(assistant_msg(&format!("Assistant response {i}")));
        }

        let original_count = messages.len();
        let result = engine.compact(&mut messages).unwrap();

        assert!(result.messages_removed > 0);
        assert!(messages.len() < original_count);
        // Should have: 1 summary + 10 recent = 11
        assert_eq!(messages.len(), 11);
        // First message should be the summary
        assert_eq!(messages[0].role, "system");
    }

    #[test]
    fn test_compact_preserves_recent_messages() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = Vec::new();
        for i in 0..10 {
            messages.push(user_msg(&format!("User msg {i}")));
            messages.push(assistant_msg(&format!("Asst msg {i}")));
        }

        engine.compact(&mut messages).unwrap();

        // Should have 1 summary + 4 recent = 5
        assert_eq!(messages.len(), 5);

        // Last 4 should be the original recent messages
        let last_user = &messages[messages.len() - 2];
        let last_asst = &messages[messages.len() - 1];
        assert_eq!(last_user.role, "user");
        assert!(matches!(&last_user.content, MessageContent::Text(t) if t.contains("User msg 9")));
        assert_eq!(last_asst.role, "assistant");
    }

    #[test]
    fn test_compact_result_metrics() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = Vec::new();
        for i in 0..20 {
            messages.push(user_msg(&format!("Message {i}")));
            messages.push(assistant_msg(&format!("Response {i}")));
        }

        let result = engine.compact(&mut messages).unwrap();
        assert!(result.original_tokens > 0);
        assert!(result.duration >= Duration::ZERO);
        assert!(result.reduction_ratio >= 0.0);
        assert!(result.messages_compacted > 0);
    }

    // -- Micro compaction --

    #[test]
    fn test_micro_compact_large_messages() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![
            user_msg("Normal message"),
            large_user_msg(),
            assistant_msg("Normal response"),
        ];

        let result = engine.micro_compact(&mut messages).unwrap();
        assert_eq!(result.messages_compacted, 1);
        assert_eq!(result.strategy, CompactStrategy::MicroCompress);
        // The large message should now be compressed
        match &messages[1].content {
            MessageContent::Text(text) => {
                assert!(text.contains("Compressed"));
                assert!(text.len() < 20000);
            }
            _ => panic!("Expected text content after micro-compact"),
        }
    }

    #[test]
    fn test_micro_compact_disabled() {
        let engine = CompactEngine::new(
            CompactConfig {
                enable_micro_compact: false,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![user_msg("Hello"), large_user_msg()];
        let result = engine.micro_compact(&mut messages).unwrap();
        assert_eq!(result.messages_compacted, 0);
        // Messages unchanged
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_micro_compact_empty() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut messages: Vec<Message> = Vec::new();
        let result = engine.micro_compact(&mut messages).unwrap();
        assert_eq!(result.messages_compacted, 0);
    }

    // -- Session memory compaction --

    #[test]
    fn test_session_memory_compact() {
        let engine = CompactEngine::with_defaults().unwrap();
        // Use longer memory entries so the summarizer can actually compress them
        let long_memory = "X".repeat(500);
        let memory_entries = vec![
            system_msg(&format!("Memory 1: {long_memory}")),
            system_msg(&format!("Memory 2: {long_memory}")),
            system_msg(&format!("Memory 3: {long_memory}")),
            system_msg(&format!("Memory 4: {long_memory}")),
            system_msg(&format!("Memory 5: {long_memory}")),
        ];

        let result = engine.compact_session_memory(&memory_entries).unwrap();
        assert!(result.messages_removed > 0);
        assert!(result.strategy == CompactStrategy::SessionMemoryCompress);
        // Rule-based summarizer truncates to ~150 chars per message, so with 5 long
        // entries the summary should be smaller than the originals
        assert!(result.reduction_ratio >= 0.0);
        assert!(result.original_tokens > result.compacted_tokens);
    }

    #[test]
    fn test_session_memory_compact_disabled() {
        let engine = CompactEngine::new(
            CompactConfig {
                enable_session_memory_compact: false,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let entries = vec![system_msg("Some memory")];
        let result = engine.compact_session_memory(&entries).unwrap();
        assert_eq!(result.messages_removed, 0);
    }

    #[test]
    fn test_session_memory_compact_empty() {
        let engine = CompactEngine::with_defaults().unwrap();
        let result = engine.compact_session_memory(&[]);
        assert!(matches!(result, Err(CompactError::NoMessagesToCompact)));
    }

    // -- Message grouping --

    #[test]
    fn test_group_messages_basic() {
        let engine = CompactEngine::with_defaults().unwrap();
        let messages = vec![
            user_msg("Hello"),
            assistant_msg("Hi there"),
            user_msg("How are you?"),
        ];

        let groups = engine.group_messages(&messages);
        assert_eq!(groups.len(), 3);
        assert!(matches!(&groups[0], MessageGroup::UserTurn { .. }));
        assert!(matches!(&groups[1], MessageGroup::AssistantTurn { .. }));
        assert!(matches!(&groups[2], MessageGroup::UserTurn { .. }));
    }

    #[test]
    fn test_group_messages_tool_use() {
        let engine = CompactEngine::with_defaults().unwrap();
        let messages = vec![
            user_msg("Run ls"),
            tool_use_msg("tool_1", "bash", "ls"),
            tool_result_msg("tool_1", "file1.txt\nfile2.txt"),
            assistant_msg("Here are your files"),
        ];

        let groups = engine.group_messages(&messages);
        // Should have: UserTurn, ToolUseTurn, AssistantTurn
        assert_eq!(groups.len(), 3);
        assert!(matches!(&groups[0], MessageGroup::UserTurn { .. }));
        assert!(matches!(
            &groups[1],
            MessageGroup::ToolUseTurn {
                tool_name,
                messages,
                ..
            } if tool_name == "bash" && messages.len() == 2
        ));
    }

    #[test]
    fn test_group_messages_consecutive_same_role() {
        let engine = CompactEngine::with_defaults().unwrap();
        let messages = vec![
            system_msg("System instruction 1"),
            system_msg("System instruction 2"),
            user_msg("Hello"),
        ];

        let groups = engine.group_messages(&messages);
        // Two system messages should be grouped together
        assert_eq!(groups.len(), 2);
        assert!(matches!(
            &groups[0],
            MessageGroup::SystemMessage { messages } if messages.len() == 2
        ));
    }

    #[test]
    fn test_group_messages_empty() {
        let engine = CompactEngine::with_defaults().unwrap();
        let groups = engine.group_messages(&[]);
        assert!(groups.is_empty());
    }

    // -- Post-compact cleanup --

    #[test]
    fn test_post_compact_cleanup_removes_duplicate_summaries() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![
            system_msg("[Previous conversation summary]\nContent A"),
            system_msg("[Previous conversation summary]\nContent A"),
            user_msg("Next question"),
        ];

        let removed = engine.post_compact_cleanup(&mut messages);
        assert_eq!(removed, 1);
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_post_compact_cleanup_collapses_consecutive_system() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![
            system_msg("Summary 1"),
            system_msg("Summary 2"),
            system_msg("Summary 3"),
            system_msg("Summary 4"),
            system_msg("Summary 5"),
            user_msg("Question"),
        ];

        let removed = engine.post_compact_cleanup(&mut messages);
        assert!(removed > 0);
        // After cleanup, we should have fewer than 6 messages
        assert!(messages.len() < 6);
    }

    #[test]
    fn test_post_compact_cleanup_noop() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![
            user_msg("Hello"),
            assistant_msg("Hi"),
            user_msg("Question"),
        ];

        let removed = engine.post_compact_cleanup(&mut messages);
        assert_eq!(removed, 0);
        assert_eq!(messages.len(), 3);
    }

    // -- CompactResult display --

    #[test]
    fn test_compact_result_display() {
        let result = CompactResult {
            original_tokens: 10000,
            compacted_tokens: 3000,
            reduction_ratio: 0.7,
            messages_removed: 15,
            messages_compacted: 15,
            duration: Duration::from_millis(150),
            strategy: CompactStrategy::SummarizeOld,
        };

        let display = format!("{result}");
        assert!(display.contains("10000"));
        assert!(display.contains("3000"));
        assert!(display.contains("70.0%"));
        assert!(display.contains("15"));
        assert!(display.contains("summarize_old"));
    }

    #[test]
    fn test_compact_result_no_change() {
        let result = CompactResult::no_change(CompactStrategy::TruncateOld, 5000);
        assert_eq!(result.original_tokens, 5000);
        assert_eq!(result.compacted_tokens, 5000);
        assert_eq!(result.reduction_ratio, 0.0);
        assert_eq!(result.messages_removed, 0);
    }

    // -- Strategy display --

    #[test]
    fn test_strategy_display() {
        assert_eq!(format!("{}", CompactStrategy::TruncateOld), "truncate_old");
        assert_eq!(format!("{}", CompactStrategy::SummarizeOld), "summarize_old");
        assert_eq!(format!("{}", CompactStrategy::MicroCompress), "micro_compress");
        assert_eq!(format!("{}", CompactStrategy::GroupCompress), "group_compress");
        assert_eq!(format!("{}", CompactStrategy::SessionMemoryCompress), "session_memory_compress");
    }

    // -- MessageGroup --

    #[test]
    fn test_message_group_total_tokens() {
        let group = MessageGroup::UserTurn {
            messages: vec![
                GroupedMessage {
                    message: user_msg("Hello world"),
                    original_index: 0,
                    estimated_tokens: 3,
                },
                GroupedMessage {
                    message: user_msg("Second message"),
                    original_index: 1,
                    estimated_tokens: 4,
                },
            ],
        };
        assert_eq!(group.total_tokens(), 7);
    }

    #[test]
    fn test_message_group_label() {
        let group = MessageGroup::ToolUseTurn {
            tool_name: "bash".to_string(),
            tool_use_id: "tool_1".to_string(),
            messages: vec![GroupedMessage {
                message: tool_use_msg("tool_1", "bash", "ls"),
                original_index: 0,
                estimated_tokens: 5,
            }],
        };
        let label = group.label();
        assert!(label.contains("bash"));
        assert!(label.contains("1 messages"));
    }

    // -- CompactPrompt --

    #[test]
    fn test_compact_prompt_system_prompt() {
        let prompt = CompactPrompt::system_prompt(1000);
        assert!(prompt.contains("1000"));
        assert!(prompt.contains("summary"));
    }

    #[test]
    fn test_compact_prompt_conversation_to_summarize() {
        let messages = vec![user_msg("Hello"), assistant_msg("Hi there")];
        let prompt = CompactPrompt::conversation_to_summarize(&messages);
        assert!(prompt.contains("[user]: Hello"));
        assert!(prompt.contains("[assistant]: Hi there"));
    }

    #[test]
    fn test_compact_prompt_micro_compact() {
        let msg = user_msg("Some very long content here");
        let prompt = CompactPrompt::micro_compact_prompt(&msg, 500);
        assert!(prompt.contains("500"));
        assert!(prompt.contains("user"));
    }

    // -- Token estimation helpers --

    #[test]
    fn test_estimate_message_tokens_text() {
        let msg = user_msg("Hello world"); // 11 chars
        let tokens = estimate_message_tokens(&msg);
        // 11 / 4 = 2 (rounded down), but max(1)
        assert!((1..=5).contains(&tokens));
    }

    #[test]
    fn test_estimate_message_tokens_blocks() {
        let msg = tool_use_msg("t1", "bash", "echo hello");
        let tokens = estimate_message_tokens(&msg);
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_tokens_slice() {
        let messages = vec![user_msg("A"), assistant_msg("B")];
        let tokens = estimate_tokens(&messages);
        assert!(tokens > 0);
    }

    #[test]
    fn test_truncate_text_short() {
        assert_eq!(truncate_text("Hello", 100), "Hello");
    }

    #[test]
    fn test_truncate_text_long() {
        let result = truncate_text(&"A".repeat(200), 100);
        assert!(result.len() < 200);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_extract_text_content_text_variant() {
        let msg = user_msg("Hello world");
        assert_eq!(extract_text_content(&msg), "Hello world");
    }

    #[test]
    fn test_extract_text_content_blocks_variant() {
        let msg = tool_use_msg("t1", "bash", "ls -la");
        let content = extract_text_content(&msg);
        assert!(content.contains("bash"));
        assert!(content.contains("[Tool:"));
    }

    // -- RuleBasedSummarizer --

    #[test]
    fn test_rule_based_summarizer_empty() {
        let summarizer = RuleBasedSummarizer::new();
        let result = summarizer.summarize(&[], 1000);
        assert!(matches!(result, Err(CompactError::NoMessagesToCompact)));
    }

    #[test]
    fn test_rule_based_summarizer_basic() {
        let summarizer = RuleBasedSummarizer::new();
        let messages = vec![user_msg("Hello"), assistant_msg("Hi there!")];
        let result = summarizer.summarize(&messages, 1000);
        assert!(result.is_ok());
        let summary = result.unwrap();
        assert!(summary.contains("User"));
        assert!(summary.contains("Hello"));
        assert!(summary.contains("Assistant"));
    }

    #[test]
    fn test_rule_based_summarizer_tool_uses() {
        let summarizer = RuleBasedSummarizer::new();
        let messages = vec![
            user_msg("Run ls"),
            tool_use_msg("t1", "bash", "ls"),
        ];
        let result = summarizer.summarize(&messages, 1000).unwrap();
        assert!(result.contains("bash"));
        assert!(result.contains("Tools used"));
    }

    #[test]
    fn test_rule_based_summarizer_file_paths() {
        let summarizer = RuleBasedSummarizer::new();
        let messages = vec![user_msg("Look at src/main.rs and Cargo.toml")];
        let result = summarizer.summarize(&messages, 1000).unwrap();
        assert!(result.contains("src/main.rs"));
        assert!(result.contains("Cargo.toml"));
        assert!(result.contains("Files referenced"));
    }

    #[test]
    fn test_rule_based_summarizer_errors() {
        let summarizer = RuleBasedSummarizer::new();
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: Some(ToolResultContent::Single("Command not found".to_string())),
                is_error: Some(true),
            }]),
        }];
        let result = summarizer.summarize(&messages, 1000).unwrap();
        assert!(result.contains("Errors encountered"));
    }

    #[test]
    fn test_rule_based_micro_summarize() {
        let summarizer = RuleBasedSummarizer::new();
        let msg = user_msg("Long content that should be compressed");
        let result = summarizer.micro_summarize(&msg, 100);
        assert!(result.is_ok());
        let summary = result.unwrap();
        assert!(summary.contains("Compressed"));
        assert!(summary.contains("user"));
    }

    // -- Group compact --

    #[test]
    fn test_group_compact() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = Vec::new();
        for i in 0..20 {
            messages.push(user_msg(&format!("User message {i}")));
            messages.push(assistant_msg(&format!("Response {i}")));
        }

        let original_count = messages.len();
        let result = engine.group_compact(&mut messages).unwrap();
        assert!(result.messages_removed > 0);
        assert!(messages.len() < original_count);
        assert_eq!(messages[0].role, "system");
        assert!(matches!(&messages[0].content, MessageContent::Text(t) if t.contains("Group-compacted")));
    }

    #[test]
    fn test_group_compact_too_few_messages() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![user_msg("Hello"), assistant_msg("Hi")];
        let result = engine.group_compact(&mut messages).unwrap();
        assert_eq!(result.messages_removed, 0);
        assert_eq!(messages.len(), 2); // unchanged
    }

    // -- Integration: full workflow --

    #[test]
    fn test_full_workflow_compact_then_cleanup() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                max_context_tokens: 200,
                trigger_threshold: 0.5,
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = Vec::new();
        for i in 0..25 {
            // Use longer messages to exceed the small max_context_tokens threshold
            messages.push(user_msg(&format!(
                "User message {i} with extra padding text to ensure we exceed token budget"
            )));
            messages.push(assistant_msg(&format!(
                "Response {i} with extra padding text to ensure we exceed token budget significantly"
            )));
        }

        // Auto-compact check
        assert!(engine.auto_compact_check(&messages));

        // Full compact
        let result = engine.compact(&mut messages).unwrap();
        assert!(result.messages_removed > 0);

        // Post-cleanup should not remove anything if there's only one summary
        let cleanup_removed = engine.post_compact_cleanup(&mut messages);
        assert_eq!(cleanup_removed, 0);
    }

    #[test]
    fn test_full_workflow_analyze_then_micro_compact() {
        let engine = CompactEngine::with_defaults().unwrap();

        let mut messages = vec![
            user_msg("Normal"),
            large_user_msg(),
            assistant_msg("Normal response"),
        ];

        let analysis = engine.analyze_context(&messages);
        assert_eq!(analysis.micro_compact_candidates, 1);

        let result = engine.micro_compact(&mut messages).unwrap();
        assert_eq!(result.messages_compacted, 1);
    }

    // -- Edge case: system prompt preserved during compression --

    #[test]
    fn test_compact_preserves_system_prompt_at_front() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![
            system_msg("You are a helpful coding assistant."),
        ];
        for i in 0..15 {
            messages.push(user_msg(&format!("User query {i}")));
            messages.push(assistant_msg(&format!("Response {i}")));
        }

        engine.compact(&mut messages).unwrap();

        // The original system prompt should still be present somewhere
        let has_system_prompt = messages.iter().any(|m| {
            matches!(&m.content, MessageContent::Text(t) if t.contains("helpful coding assistant"))
        });
        assert!(has_system_prompt, "System prompt should be preserved after compaction");
    }

    // -- Edge case: concurrent compact guard --

    #[test]
    fn test_compact_already_in_progress() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        // Manually set the compacting flag to simulate concurrent access
        engine.compacting = true;
        let mut messages = vec![user_msg("test")];
        let result = engine.compact(&mut messages);
        assert!(matches!(result, Err(CompactError::AlreadyInProgress)));
    }

    // -- Edge case: token estimation within reasonable bounds --

    #[test]
    fn test_token_estimation_reasonable_bounds() {
        // 100 chars = ~25 tokens (at 4 chars/token)
        let msg = user_msg(&"A".repeat(100));
        let tokens = estimate_message_tokens(&msg);
        assert!((20..=30).contains(&tokens), "100 chars should be ~25 tokens, got {tokens}");

        // 1000 chars = ~250 tokens
        let msg = user_msg(&"B".repeat(1000));
        let tokens = estimate_message_tokens(&msg);
        assert!((240..=260).contains(&tokens), "1000 chars should be ~250 tokens, got {tokens}");

        // Single char = 1 token (min(1))
        let msg = user_msg("X");
        let tokens = estimate_message_tokens(&msg);
        assert_eq!(tokens, 1);
    }

    // -- Edge case: group_compact with mixed tool_use and text messages --

    #[test]
    fn test_group_compact_mixed_tool_and_text() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![
            user_msg("Look at my project"),
            assistant_msg("Let me check."),
            tool_use_msg("t1", "bash", "find . -name '*.rs'"),
            tool_result_msg("t1", "src/main.rs\nsrc/lib.rs"),
            assistant_msg("I found your Rust files."),
            user_msg("How many lines?"),
            tool_use_msg("t2", "bash", "wc -l src/main.rs"),
            tool_result_msg("t2", "42 src/main.rs"),
            assistant_msg("42 lines total."),
            user_msg("Add a new function"),
            assistant_msg("Done! I've added the function."),
            user_msg("Now run the tests"),
            assistant_msg("All tests pass."),
            user_msg("Great, commit it"),
            assistant_msg("Committed."),
        ];

        let original_count = messages.len();
        let result = engine.group_compact(&mut messages).unwrap();
        assert!(result.messages_removed > 0);
        assert!(messages.len() < original_count);
        // Recent messages should be preserved
        let last_msg = messages.last().unwrap();
        assert_eq!(last_msg.role, "assistant");
    }

    // -- Edge case: set_config validates new config --

    #[test]
    fn test_set_config_validates() {
        let mut engine = CompactEngine::with_defaults().unwrap();

        let valid = CompactConfig {
            max_output_tokens: 3000,
            ..Default::default()
        };
        assert!(engine.set_config(valid).is_ok());
        assert_eq!(engine.config().max_output_tokens, 3000);

        let invalid = CompactConfig {
            max_output_tokens: 0,
            ..Default::default()
        };
        assert!(engine.set_config(invalid).is_err());
        // Original config should be preserved after failed update
        assert_eq!(engine.config().max_output_tokens, 3000);
    }

    // -- CompactionConfig tests --

    #[test]
    fn test_compaction_config_default() {
        let config = CompactionConfig::default();
        assert!((config.auto_compact_threshold - 0.85).abs() < 0.001);
        assert!(config.enabled);
        assert!(matches!(config.strategy, CompactionStrategy::Summarize));
    }

    #[test]
    fn test_compaction_config_disabled() {
        let config = CompactionConfig::disabled();
        assert!(!config.enabled);
    }

    #[test]
    fn test_compaction_strategy_serialization() {
        let strategy = CompactionStrategy::KeepRecent { count: 5 };
        let json = serde_json::to_string(&strategy).unwrap();
        assert!(json.contains("KeepRecent"));
        let deserialized: CompactionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(strategy, deserialized);
    }

    // -- compact_messages tests --

    #[test]
    fn test_compact_messages_empty() {
        let result = compact_messages(&[], &CompactionStrategy::Summarize, 1000, 10);
        assert!(!result.did_compact);
        assert_eq!(result.original_count, 0);
        assert_eq!(result.compacted_count, 0);
    }

    #[test]
    fn test_compact_messages_under_budget() {
        let messages = vec![user_msg("Hello"), assistant_msg("Hi there")];
        let result = compact_messages(&messages, &CompactionStrategy::Summarize, 100_000, 10);
        assert!(!result.did_compact);
        assert_eq!(result.messages.len(), 2);
    }

    #[test]
    fn test_compact_messages_summarize_strategy() {
        let mut messages = vec![system_msg("System prompt")];
        for i in 0..20 {
            messages.push(user_msg(&format!("User message {i}")));
            messages.push(assistant_msg(&format!("Response {i}")));
        }
        let original_count = messages.len();
        let result = compact_messages(
            &messages,
            &CompactionStrategy::Summarize,
            100, // very small budget to force compaction
            4,
        );
        assert!(result.did_compact);
        assert!(result.compacted_count < original_count);
        // Should have: 1 system + 1 summary + 4 recent = 6
        assert_eq!(result.compacted_count, 6);
        // Original system prompt preserved
        assert_eq!(result.messages[0].role, "system");
        let sys_text = match &result.messages[0].content {
            MessageContent::Text(t) => t.clone(),
            _ => String::new(),
        };
        assert!(sys_text.contains("System prompt"));
    }

    #[test]
    fn test_compact_messages_keep_recent_strategy() {
        let mut messages = vec![system_msg("System prompt")];
        for i in 0..15 {
            messages.push(user_msg(&format!(
                "This is a longer user message number {i} with extra padding text to increase token count"
            )));
        }
        let result = compact_messages(
            &messages,
            &CompactionStrategy::KeepRecent { count: 3 },
            50,
            3,
        );
        assert!(result.did_compact);
        // 1 system + 1 summary + 3 recent = 5
        assert_eq!(result.compacted_count, 5);
        // Last message should be the most recent
        let last = result.messages.last().unwrap();
        match &last.content {
            MessageContent::Text(t) => assert!(t.contains("message number 14")),
            _ => panic!("Expected text content"),
        }
    }

    #[test]
    fn test_compact_messages_prioritize_code_strategy() {
        let mut messages = vec![system_msg("System")];
        for i in 0..5 {
            messages.push(user_msg(&format!(
                "This is a regular conversation message number {i} with enough text to matter for compaction"
            )));
        }
        // Add code-heavy messages
        messages.push(user_msg("Look at src/main.rs:\n```rust\nfn main() {}\n```"));
        messages.push(assistant_msg("I see you have ```python\nprint('hello')\n``` in lib.py"));
        for i in 0..5 {
            messages.push(user_msg(&format!(
                "Follow up message number {i} with additional padding for token budget"
            )));
        }
        let result = compact_messages(
            &messages,
            &CompactionStrategy::PrioritizeCode,
            200,
            3,
        );
        assert!(result.did_compact);
        // Code messages should be preserved
        let has_code = result.messages.iter().any(|m| {
            let text = extract_text_content(m);
            text.contains("src/main.rs") || text.contains("lib.py")
        });
        assert!(has_code, "Code messages should be preserved");
    }

    #[test]
    fn test_compact_messages_preserves_system_prompt() {
        let mut messages = vec![
            system_msg("You are a helpful assistant."),
            system_msg("Additional system context."),
        ];
        for i in 0..15 {
            messages.push(user_msg(&format!(
                "This is a conversation message number {i} with enough words to consume tokens"
            )));
        }
        let result = compact_messages(
            &messages,
            &CompactionStrategy::Summarize,
            50,
            4,
        );
        assert!(result.did_compact);
        // First two messages should still be the system messages
        assert_eq!(result.messages[0].role, "system");
        assert_eq!(result.messages[1].role, "system");
        match &result.messages[0].content {
            MessageContent::Text(t) => assert!(t.contains("helpful assistant")),
            _ => panic!("Expected text content"),
        }
    }

    #[test]
    fn test_compact_messages_only_system_no_compact() {
        let messages = vec![
            system_msg("System A"),
            system_msg("System B"),
        ];
        let result = compact_messages(
            &messages,
            &CompactionStrategy::Summarize,
            100,
            10,
        );
        assert!(!result.did_compact);
        assert_eq!(result.messages.len(), 2);
    }

    #[test]
    fn test_looks_like_code() {
        let code_msg = user_msg("```rust\nfn main() {}\n```");
        assert!(looks_like_code(&code_msg));

        let file_msg = user_msg("Look at src/main.rs");
        assert!(looks_like_code(&file_msg));

        let plain_msg = user_msg("Hello, how are you?");
        assert!(!looks_like_code(&plain_msg));

        let tool_msg = tool_use_msg("t1", "bash", "ls -la");
        assert!(looks_like_code(&tool_msg));
    }

    // -- MessageProtector tests --

    #[test]
    fn test_message_protector_empty() {
        let p = MessageProtector::new();
        assert_eq!(p.protected_count(), 0);
        assert!(!p.is_protected(0));
    }

    #[test]
    fn test_message_protector_protect_unprotect() {
        let mut p = MessageProtector::new();
        p.protect(5);
        p.protect(10);
        assert_eq!(p.protected_count(), 2);
        assert!(p.is_protected(5));
        assert!(p.is_protected(10));
        assert!(!p.is_protected(3));

        p.unprotect(5);
        assert!(!p.is_protected(5));
        assert_eq!(p.protected_count(), 1);
    }

    #[test]
    fn test_message_protector_clear() {
        let mut p = MessageProtector::new();
        p.protect(1);
        p.protect(2);
        p.clear();
        assert_eq!(p.protected_count(), 0);
    }

    // -- classify_message_priority tests --

    #[test]
    fn test_priority_system_message() {
        let msg = system_msg("Important instructions");
        assert_eq!(
            classify_message_priority(&msg),
            crate::context_budget::MessagePriority::Critical
        );
    }

    #[test]
    fn test_priority_short_user_message() {
        let msg = user_msg("Fix the bug");
        assert_eq!(
            classify_message_priority(&msg),
            crate::context_budget::MessagePriority::Critical
        );
    }

    #[test]
    fn test_priority_long_user_message() {
        let long_text = "x".repeat(300);
        let msg = user_msg(&long_text);
        assert_eq!(
            classify_message_priority(&msg),
            crate::context_budget::MessagePriority::Normal
        );
    }

    #[test]
    fn test_priority_code_assistant_message() {
        let msg = assistant_msg("Here's the fix:\n```rust\nfn main() {}\n```");
        assert_eq!(
            classify_message_priority(&msg),
            crate::context_budget::MessagePriority::High
        );
    }

    #[test]
    fn test_priority_plain_assistant_message() {
        let msg = assistant_msg("Sure, I can help with that.");
        assert_eq!(
            classify_message_priority(&msg),
            crate::context_budget::MessagePriority::Normal
        );
    }

    // -- compact_messages_with_protection tests --

    #[test]
    fn test_compact_with_protection_preserves_protected() {
        let mut messages = vec![system_msg("System")];
        for i in 0..20 {
            // Long enough to be Normal priority (>200 chars), not Critical
            messages.push(user_msg(&format!("This is message number {i} and it contains enough text to exceed the two hundred character threshold for normal priority classification, ensuring it will be considered for compaction by the priority-aware compaction algorithm")));
        }
        let original_len = messages.len();

        // Protect message at index 5
        let mut protector = MessageProtector::new();
        protector.protect(5);

        let result = compact_messages_with_protection(
            &messages,
            &CompactionStrategy::Summarize,
            100,
            4,
            &protector,
        );

        assert!(result.did_compact);
        // The protected message should appear in the result
        let protected_text = match &messages[5].content {
            MessageContent::Text(t) => t.clone(),
            _ => String::new(),
        };
        let found = result.messages.iter().any(|m| {
            match &m.content {
                MessageContent::Text(t) => *t == protected_text,
                _ => false,
            }
        });
        assert!(found, "Protected message should be preserved in result");
        assert!(result.compacted_count < original_len);
    }

    #[test]
    fn test_compact_with_protection_no_protection_falls_back() {
        let messages = vec![user_msg("Hello"), assistant_msg("Hi")];
        let protector = MessageProtector::new();
        let result = compact_messages_with_protection(
            &messages,
            &CompactionStrategy::Summarize,
            100_000,
            10,
            &protector,
        );
        assert!(!result.did_compact);
        assert_eq!(result.messages.len(), 2);
    }
}
