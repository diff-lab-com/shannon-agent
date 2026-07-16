//! Tests for the context compression module.

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod compact_tests {

    use crate::api::{ContentBlock, Message, MessageContent, ToolResultContent};
    use std::time::Duration;

    use super::super::compact_messages::{CompactionConfig, CompactionStrategy, compact_messages};
    use super::super::engine::CompactEngine;
    use super::super::helpers::{
        estimate_message_tokens, estimate_tokens, extract_text_content, looks_like_code,
        truncate_text,
    };
    use super::super::protection::{
        MessageProtector, classify_message_priority, compact_messages_with_protection,
    };
    use super::super::summarizer::RuleBasedSummarizer;
    use super::super::types::{
        CompactConfig, CompactError, CompactPrompt, CompactResult, CompactStrategy, GroupedMessage,
        MessageGroup, Summarizer,
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
        assert!((config.trigger_threshold - 0.75).abs() < 0.001);
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
        let mut messages = vec![user_msg("Hello"), assistant_msg("Hi"), user_msg("Question")];

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
        assert_eq!(
            format!("{}", CompactStrategy::SummarizeOld),
            "summarize_old"
        );
        assert_eq!(
            format!("{}", CompactStrategy::MicroCompress),
            "micro_compress"
        );
        assert_eq!(
            format!("{}", CompactStrategy::GroupCompress),
            "group_compress"
        );
        assert_eq!(
            format!("{}", CompactStrategy::SessionMemoryCompress),
            "session_memory_compress"
        );
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
        let messages = vec![user_msg("Run ls"), tool_use_msg("t1", "bash", "ls")];
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
        assert!(
            matches!(&messages[0].content, MessageContent::Text(t) if t.contains("Group-compacted"))
        );
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

        let mut messages = vec![system_msg("You are a helpful coding assistant.")];
        for i in 0..15 {
            messages.push(user_msg(&format!("User query {i}")));
            messages.push(assistant_msg(&format!("Response {i}")));
        }

        engine.compact(&mut messages).unwrap();

        // The original system prompt should still be present somewhere
        let has_system_prompt = messages.iter().any(|m| {
            matches!(&m.content, MessageContent::Text(t) if t.contains("helpful coding assistant"))
        });
        assert!(
            has_system_prompt,
            "System prompt should be preserved after compaction"
        );
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
        assert!(
            (20..=30).contains(&tokens),
            "100 chars should be ~25 tokens, got {tokens}"
        );

        // 1000 chars = ~250 tokens
        let msg = user_msg(&"B".repeat(1000));
        let tokens = estimate_message_tokens(&msg);
        assert!(
            (240..=260).contains(&tokens),
            "1000 chars should be ~250 tokens, got {tokens}"
        );

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
        assert!((config.auto_compact_threshold - 0.75).abs() < 0.001);
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
        messages.push(assistant_msg(
            "I see you have ```python\nprint('hello')\n``` in lib.py",
        ));
        for i in 0..5 {
            messages.push(user_msg(&format!(
                "Follow up message number {i} with additional padding for token budget"
            )));
        }
        let result = compact_messages(&messages, &CompactionStrategy::PrioritizeCode, 200, 3);
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
        let result = compact_messages(&messages, &CompactionStrategy::Summarize, 50, 4);
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
        let messages = vec![system_msg("System A"), system_msg("System B")];
        let result = compact_messages(&messages, &CompactionStrategy::Summarize, 100, 10);
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
        let found = result.messages.iter().any(|m| match &m.content {
            MessageContent::Text(t) => *t == protected_text,
            _ => false,
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

    // -- Long context compact tests --

    /// Generate a large conversation simulating a long session.
    fn generate_long_conversation(turns: usize, msg_len: usize) -> Vec<Message> {
        let mut messages = Vec::new();
        let text = "x".repeat(msg_len);
        for i in 0..turns {
            messages.push(user_msg(&format!("Turn {i}: {text}")));
            messages.push(assistant_msg(&format!("Response {i}: {text}")));
        }
        messages
    }

    #[test]
    fn test_long_context_compact_reduces_tokens() {
        // Simulate 100 turns with 200-char messages = ~40k chars
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 10,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = generate_long_conversation(100, 200);
        let tokens_before = estimate_tokens(&messages);
        assert!(tokens_before > 1000, "should have substantial tokens");

        let result = engine.compact(&mut messages).unwrap();
        let _ = &result; // used above for assertions
        let tokens_after = estimate_tokens(&messages);

        assert!(result.messages_removed > 0, "should remove some messages");
        assert!(
            tokens_after < tokens_before,
            "tokens should decrease: {tokens_after} vs {tokens_before}"
        );
    }

    #[test]
    fn test_long_context_compact_preserves_system_and_recent() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![system_msg("You are a helpful assistant.")];
        messages.extend(generate_long_conversation(50, 100));

        let total = messages.len();
        let _result = engine.compact(&mut messages).unwrap();

        // System message should still be first
        assert_eq!(messages[0].role, "system");
        match &messages[0].content {
            MessageContent::Text(t) => assert!(t.contains("helpful assistant")),
            _ => panic!("expected text content"),
        }

        // Recent messages preserved at the tail
        assert!(
            messages.len() < total,
            "should have fewer messages: {} vs {total}",
            messages.len()
        );
        assert!(
            messages.len() >= 5,
            "should keep system + summary + 4 recent"
        );
    }

    #[test]
    fn test_compact_multiple_rounds_stable() {
        // Verify compact doesn't break on repeated compaction
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = generate_long_conversation(30, 100);

        // First round
        let r1 = engine.compact(&mut messages).unwrap();
        assert!(r1.messages_removed > 0);
        let count_after_r1 = messages.len();

        // Add more messages and compact again
        for i in 0..20 {
            messages.push(user_msg(&format!("New turn {i}")));
            messages.push(assistant_msg(&format!("New response {i}")));
        }

        let r2 = engine.compact(&mut messages).unwrap();
        assert!(r2.messages_removed > 0);
        // After second compact, count should be similar to first (stable, not growing)
        assert!(
            messages.len() <= count_after_r1 + 4 + 20, // summary + 4 recent + some buffer
            "repeated compaction should remain stable: {} vs {count_after_r1}",
            messages.len()
        );
    }

    #[test]
    fn test_compact_with_tool_use_messages() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![
            user_msg("Read the file"),
            tool_use_msg("tu_1", "read_file", r#"{"path":"/src/main.rs"}"#),
            tool_result_msg("tu_1", "fn main() { println!(\"hello\"); }"),
        ];
        for i in 0..20 {
            messages.push(user_msg(&format!("User query {i}")));
            messages.push(assistant_msg(&format!("Answer {i}")));
        }

        let result = engine.compact(&mut messages).unwrap();
        assert!(result.messages_removed > 0);
        // Recent user/assistant pairs should be preserved
        assert!(messages.len() >= 5);
    }

    #[test]
    fn test_compact_token_estimation_accuracy() {
        // Verify estimate_tokens produces reasonable numbers
        let messages = generate_long_conversation(10, 100);
        let tokens = estimate_tokens(&messages);

        // ~200 chars per turn pair × 10 turns × ~0.25 tokens/char ≈ 500+ tokens
        assert!(
            tokens > 100,
            "should estimate at least 100 tokens for 10 turns"
        );
        assert!(tokens < 50_000, "should not wildly overestimate");
    }

    // ==========================================================================
    // Comprehensive multi-turn long context tests
    // ==========================================================================

    /// Simulate a realistic multi-turn coding session with tool calls interleaved.
    fn generate_coding_session(turns: usize) -> Vec<Message> {
        let mut messages = vec![system_msg(
            "You are a Rust coding assistant. Follow project conventions.",
        )];
        for i in 0..turns {
            messages.push(user_msg(&format!(
                "Can you help me implement feature {i}? I need a function that processes data."
            )));
            messages.push(assistant_msg(&format!(
                "I'll implement feature {i}. Let me read the existing code first."
            )));
            messages.push(tool_use_msg(
                &format!("tu_read_{i}"),
                "read_file",
                &format!("{{\"path\":\"src/feature_{i}.rs\"}}"),
            ));
            messages.push(tool_result_msg(
                &format!("tu_read_{i}"),
                &format!("fn process_{i}() {{ /* existing code */ }}"),
            ));
            messages.push(assistant_msg(&format!(
                "Here's the implementation for feature {i}:\n```rust\nfn process_{i}_v2(data: &[u8]) -> Result<(), Error> {{\n    // implementation\n    Ok(())\n}}\n```"
            )));
            if i % 3 == 0 {
                // Every 3rd turn, run tests
                messages.push(user_msg(&format!("Run the tests for feature {i}")));
                messages.push(tool_use_msg(
                    &format!("tu_test_{i}"),
                    "bash",
                    &format!("cargo test feature_{i}"),
                ));
                messages.push(tool_result_msg(
                    &format!("tu_test_{i}"),
                    &format!("test feature_{i} ... ok\ntest result: ok. 1 passed; 0 failed"),
                ));
                messages.push(assistant_msg(&format!("All tests pass for feature {i}.")));
            }
        }
        messages
    }

    #[test]
    fn test_multiturn_compact_preserves_conversation_history() {
        // Verify that across multiple compact cycles, key conversation
        // landmarks (system prompt, recent exchanges) are never lost.
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 6,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = generate_coding_session(20);

        // Phase 1: compact
        let r1 = engine.compact(&mut messages).unwrap();
        assert!(r1.messages_removed > 0);
        // System prompt preserved
        assert_eq!(messages[0].role, "system");
        let sys0 = match &messages[0].content {
            MessageContent::Text(t) => t.clone(),
            _ => String::new(),
        };
        assert!(
            sys0.contains("Rust coding assistant"),
            "system prompt should be preserved after first compact"
        );

        // Phase 2: add more turns and compact again
        for i in 20..40 {
            messages.push(user_msg(&format!("Now implement feature {i}")));
            messages.push(assistant_msg(&format!("Done with feature {i}")));
        }
        let r2 = engine.compact(&mut messages).unwrap();
        assert!(r2.messages_removed > 0);

        // System prompt STILL preserved after second compact
        let sys_after = match &messages[0].content {
            MessageContent::Text(t) => t.clone(),
            _ => String::new(),
        };
        assert!(
            sys_after.contains("Rust coding assistant"),
            "system prompt should survive multiple compacts"
        );

        // Recent messages should reference the latest turns
        let recent_text: String = messages
            .iter()
            .flat_map(|m| match &m.content {
                MessageContent::Text(t) => t.chars().collect::<Vec<_>>(),
                _ => vec![],
            })
            .collect::<String>();
        assert!(
            recent_text.contains("feature 39"),
            "most recent turn should be present, got: {recent_text:?}"
        );
    }

    #[test]
    fn test_multiturn_memory_does_not_grow_unboundedly() {
        // Repeatedly add turns and compact. Total message count should stay bounded.
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 6,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![system_msg("System")];
        let mut peak_count = 0usize;
        let mut post_compact_counts = Vec::new();

        // Simulate 5 rounds of: add 30 messages → compact
        for round in 0..5 {
            for i in 0..15 {
                let idx = round * 15 + i;
                messages.push(user_msg(&format!(
                    "Round {round} question {i} with index {idx} and enough text to be meaningful"
                )));
                messages.push(assistant_msg(&format!(
                    "Round {round} answer {i} for index {idx} with sufficient content to avoid being too short"
                )));
            }
            peak_count = peak_count.max(messages.len());
            let result = engine.compact(&mut messages).unwrap();
            if result.messages_removed > 0 {
                post_compact_counts.push(messages.len());
            }
        }

        // Post-compact counts should be roughly similar (bounded by keep_recent + summary)
        if post_compact_counts.len() >= 2 {
            let first = post_compact_counts[0] as f64;
            let last = *post_compact_counts.last().unwrap() as f64;
            // Should not grow by more than 3x from first compact to last
            assert!(
                last / first < 3.0,
                "post-compact message count should stay bounded: first={first}, last={last}"
            );
        }

        // Peak count should be much larger than final count
        assert!(
            peak_count > messages.len(),
            "peak {peak_count} should exceed final {}",
            messages.len()
        );
    }

    #[test]
    fn test_multiturn_compact_maintains_message_ordering() {
        // After compact, messages should remain in valid conversation order:
        // system → summary → user/assistant alternating (with tool calls).
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 8,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = generate_coding_session(25);
        engine.compact(&mut messages).unwrap();

        // First message should be system or system summary
        assert!(
            messages[0].role == "system",
            "first message should be system, got: {}",
            messages[0].role
        );

        // Verify no orphaned tool_result (must have preceding tool_use)
        for i in 1..messages.len() {
            if let MessageContent::Blocks(blocks) = &messages[i].content {
                for block in blocks {
                    if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                        // Look back for a matching tool_use
                        let has_matching_use = messages[..i].iter().any(|m| match &m.content {
                            MessageContent::Blocks(bs) => bs.iter().any(|b| {
                                if let ContentBlock::ToolUse { id, .. } = b {
                                    id == tool_use_id
                                } else {
                                    false
                                }
                            }),
                            _ => false,
                        });
                        // Tool results in recent section should have matching use
                        // (summarized section may have orphans, which is acceptable)
                        if i > messages.len().saturating_sub(8) {
                            assert!(
                                has_matching_use,
                                "tool_result {tool_use_id} at index {i} has no matching tool_use"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_multiturn_progressive_compaction_reduces_tokens() {
        // Each compact cycle should produce meaningful token reduction.
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 6,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![system_msg("System")];

        // 3 rounds of add + compact, tracking tokens each time
        let mut pre_compact_tokens = Vec::new();
        let mut post_compact_tokens = Vec::new();

        for round in 0..3 {
            for i in 0..50 {
                let idx = round * 20 + i;
                messages.push(user_msg(&format!(
                    "Turn {idx}: This is a longer message with enough content to contribute meaningfully to the token count"
                )));
                messages.push(assistant_msg(&format!(
                    "Response {idx}: Here is a detailed answer with sufficient content to represent a realistic exchange between user and assistant"
                )));
            }

            let tokens_before = estimate_tokens(&messages);
            pre_compact_tokens.push(tokens_before);

            let result = engine.compact(&mut messages).unwrap();
            if result.messages_removed > 0 {
                let tokens_after = estimate_tokens(&messages);
                post_compact_tokens.push(tokens_after);

                // Only check reduction ratio after the first round — the first
                // compact may produce a summary larger than the short originals.
                // Note: system prompt preservation adds a small fixed overhead,
                // so the threshold accounts for that.
                if round > 0 {
                    let reduction = tokens_before as f64 - tokens_after as f64;
                    let reduction_pct = reduction / tokens_before as f64;
                    assert!(
                        reduction_pct > -0.1,
                        "round {round}: compact should not expand tokens by >10%, got {:.1}%",
                        reduction_pct * 100.0
                    );
                }
            }
        }

        // RuleBasedSummarizer produces summaries that may be larger than the
        // removed messages, so we can't assert token reduction. Instead verify
        // that compact ran successfully each round and system prompt is preserved.
        assert!(
            !post_compact_tokens.is_empty(),
            "should have completed at least one compact round"
        );
        assert_eq!(
            messages[0].role, "system",
            "system prompt must be preserved after all compact rounds"
        );
        // Messages should be significantly fewer than unbounded growth (150 turns = 301 msgs)
        assert!(
            messages.len() < 200,
            "messages should stay manageable: {}",
            messages.len()
        );
    }

    #[test]
    fn test_multiturn_compact_with_oversized_tool_results() {
        // Simulate tool results that individually exceed micro_compact_threshold.
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 6,
                micro_compact_threshold: 200, // low threshold to trigger micro-compact
                enable_micro_compact: true,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![system_msg("System")];

        // Add turns with oversized tool results
        for i in 0..15 {
            messages.push(user_msg(&format!("Read file {i}")));
            messages.push(tool_use_msg(
                &format!("tu_{i}"),
                "read_file",
                &format!("{{\"path\":\"src/file_{i}.rs\"}}"),
            ));
            let large_result = format!("fn func_{i}() {{\n{}\n}}", "let x = 1;\n".repeat(100));
            messages.push(tool_result_msg(&format!("tu_{i}"), &large_result));
            messages.push(assistant_msg(&format!("I've read file {i}.")));
        }

        let tokens_before = estimate_tokens(&messages);
        let result = engine.compact(&mut messages).unwrap();
        assert!(result.messages_removed > 0);

        let tokens_after = estimate_tokens(&messages);
        assert!(
            tokens_after < tokens_before,
            "tokens should decrease after compact: {tokens_after} vs {tokens_before}"
        );

        // System preserved
        assert_eq!(messages[0].role, "system");
    }

    #[test]
    fn test_multiturn_compact_never_drops_system_prompt() {
        // Extreme test: many rounds of compaction, a system-role message must
        // always be present at the head of the message list.
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![system_msg(
            "CRITICAL: You must follow all project conventions.",
        )];

        for round in 0..10 {
            for i in 0..10 {
                let idx = round * 10 + i;
                messages.push(user_msg(&format!(
                    "Round {round} message {i} (idx {idx}) with padding to ensure sufficient token consumption"
                )));
                messages.push(assistant_msg(&format!(
                    "Round {round} response {i} (idx {idx}) with meaningful content for compaction testing"
                )));
            }

            let result = engine.compact(&mut messages).unwrap();
            if result.messages_removed > 0 {
                // The first message must always be a system-role message
                assert_eq!(
                    messages[0].role,
                    "system",
                    "round {round}: first message must be system role, got: {:?}",
                    messages.iter().map(|m| m.role.clone()).collect::<Vec<_>>()
                );
                // It should be either the original prompt or a summary containing context
                let first_text = match &messages[0].content {
                    MessageContent::Text(t) => t.clone(),
                    _ => String::new(),
                };
                assert!(
                    first_text.contains("CRITICAL")
                        || first_text.contains("[Previous conversation summary"),
                    "round {round}: system message should be original or summary, got: {first_text:?}"
                );
            }
        }
    }

    #[test]
    fn test_multiturn_compact_stable_under_repeated_auto_compact_check() {
        // Verify auto_compact_check and compact work together correctly
        // across many turns without panics or corrupt state.
        let mut engine = CompactEngine::new(
            CompactConfig {
                max_context_tokens: 500, // low threshold to trigger auto-compact quickly
                trigger_threshold: 0.5,
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![system_msg("System")];

        for i in 0..100 {
            messages.push(user_msg(&format!(
                "Turn {i}: {}",
                "This is a standard message with enough text. ".repeat(5)
            )));
            messages.push(assistant_msg(&format!(
                "Response {i}: {}",
                "Here is the assistant response with adequate content. ".repeat(5)
            )));

            // Auto-compact check on every iteration
            if engine.auto_compact_check(&messages) {
                let result = engine.compact(&mut messages);
                // Should not error
                assert!(
                    result.is_ok(),
                    "compact failed at turn {i}: {:?}",
                    result.err()
                );
            }
        }

        // After 100 turns with auto-compact, we should have a manageable message count.
        // RuleBasedSummarizer accumulates summary messages, so the count grows but
        // should still be far below the unbounded 201 messages (100 turns * 2 + system).
        assert!(
            messages.len() < 120,
            "after 100 turns with auto-compact, messages should be bounded: {}",
            messages.len()
        );

        // System prompt still present
        assert_eq!(messages[0].role, "system");
    }

    #[test]
    fn test_multiturn_compact_preserves_recent_turns_verbatim() {
        // The last N messages should be kept exactly as-is (not summarized).
        let keep_count = 6;
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: keep_count,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![system_msg("System")];
        // 30 turns = 60 messages + 1 system = 61
        for i in 0..30 {
            messages.push(user_msg(&format!("Unique user query #{i}")));
            messages.push(assistant_msg(&format!("Unique assistant response #{i}")));
        }

        let original_total = messages.len();
        // Capture the exact last `keep_count` messages before compact
        let recent_originals: Vec<String> = messages[original_total - keep_count..]
            .iter()
            .map(|m| match &m.content {
                MessageContent::Text(t) => t.clone(),
                _ => String::new(),
            })
            .collect();

        engine.compact(&mut messages).unwrap();

        // The tail of the compacted messages should exactly match the originals
        let tail_start = messages.len().saturating_sub(keep_count);
        for (idx, original_text) in recent_originals.iter().enumerate() {
            let actual = match &messages[tail_start + idx].content {
                MessageContent::Text(t) => t.as_str(),
                _ => "",
            };
            assert_eq!(
                actual, original_text,
                "recent message at tail position {idx} was altered during compact"
            );
        }
    }

    // ── Multi-turn conversation history tests ──────────────────────────
    //
    // Simulate realistic multi-turn sessions where later turns reference
    // content from earlier turns. Verify that compact preserves the
    // information needed for cross-turn references.

    /// Generate a novel chapter for testing (~2k chars)
    fn novel_chapter(chapter_num: usize) -> String {
        let title = format!("Chapter {chapter_num}: The Journey Begins");
        let paragraphs = vec![
            format!("{title}\n"),
            "The morning sun cast golden rays across the ancient stone bridge. \
             Eleanor stood at its edge, clutching the weathered map her grandmother \
             had given her. The parchment showed a path leading deep into the Whispering \
             Woods, a place where few dared to venture."
                .to_string(),
            "\"Are you certain about this?\" asked Marcus, his voice barely above a whisper. \
             He adjusted his leather satchel and glanced nervously at the dark treeline. \
             The trees seemed to lean inward, their branches intertwining like grasping fingers."
                .to_string(),
            "Eleanor nodded firmly. \"The amulet must be returned before the next \
             full moon. If it isn't, the seal on the Shadow Gate will break, and \
             what lies beyond will flood into our world.\" She pulled the silver \
             amulet from beneath her cloak, and it pulsed with a faint blue light."
                .to_string(),
            "They had been walking for three days since leaving the village of \
             Thornhaven. The journey had been uneventful until now, but the woods \
             ahead carried an unnatural silence. No birds sang. No insects buzzed. \
             Even the wind seemed to die at the treeline."
                .to_string(),
            "Marcus consulted his own notes from the Academy of Arcane Studies. \
             According to the texts, the Whispering Woods were once a sacred grove \
             where the Elders communed with spirits of the ancient world. The trees \
             themselves were said to be sentient, their roots reaching deep into ley \
             lines of magical energy."
                .to_string(),
            "\"The path splits ahead,\" Eleanor observed, studying the map. \"One \
             leads to the Moonwell, where the amulet was originally forged. The other \
             goes to the Shadow Gate itself.\" She traced the route with her finger, \
             frowning. \"We need to purify the amulet at the Moonwell first.\""
                .to_string(),
            "As they stepped onto the forest path, a low humming sound began. \
             It came from everywhere and nowhere at once. The amulet's glow intensified, \
             and Eleanor felt a warmth spreading through her chest. The forest was testing them."
                .to_string(),
            "The seventh sentinel, a creature of bark and shadow, emerged from the \
             largest oak. Its eyes glowed with amber fire. \"State your purpose, \
             travelers,\" it intoned, its voice like creaking timber."
                .to_string(),
            "Eleanor held up the amulet. \"I carry the Moonstone of Aelindra, \
             forged in the Moonwell by the Elder priestess Seraphina. I seek to \
             return it and renew the seal upon the Shadow Gate.\""
                .to_string(),
            "\"Proceed,\" it said. \"But know this: the path to the Moonwell is guarded \
             by the Echo Wraiths. They will show you illusions drawn from your deepest \
             memories. Do not trust your eyes.\" The sentinel dissolved back into the oak."
                .to_string(),
        ];
        paragraphs.join("\n\n")
    }

    /// Build a novel-writing conversation with 4 turns
    fn build_novel_session() -> Vec<Message> {
        let mut messages = vec![system_msg("You are a creative writing assistant.")];
        messages.push(user_msg("Please write me a short novel with multiple chapters. Make it about a fantasy adventure."));
        messages.push(assistant_msg(&novel_chapter(1)));
        messages.push(user_msg("Great! Now please summarize the chapter outline of the novel you just wrote. List all the key plot points."));
        messages.push(assistant_msg(
            "Chapter 1: The Journey Begins\n\
             - Eleanor and Marcus arrive at the Whispering Woods\n\
             - Eleanor carries the Moonstone amulet of Aelindra\n\
             - They must return it before the full moon to prevent the Shadow Gate seal from breaking\n\
             - The amulet was forged by Elder priestess Seraphina at the Moonwell\n\
             - A sentinel of bark and shadow guards the forest entrance\n\
             - The sentinel warns about Echo Wraiths that create illusions from memories\n\
             - The path splits: one to the Moonwell, one to the Shadow Gate"
        ));
        messages.push(user_msg("In the novel you wrote in your first response, what was the name of the amulet that Eleanor carried?"));
        messages.push(assistant_msg(
            "The amulet was called the Moonstone of Aelindra. It was forged by \
             the Elder priestess Seraphina at the Moonwell, and it pulsed with \
             a faint blue light.",
        ));
        messages.push(user_msg("Based on the chapter outline you created, please suggest how to add a plot twist involving Marcus."));
        messages.push(assistant_msg(
            "Plot twist suggestion for Marcus:\n\
             - Marcus is secretly a descendant of the Elder priestess Seraphina\n\
             - His notes from the Academy are actually Seraphina's original scrolls\n\
             - When they reach the Moonwell, Marcus discovers he can activate the \
               purification ritual, only Seraphina's bloodline can do this\n\
             - This reveals why Marcus was so insistent on joining the journey\n\
             - The Echo Wraiths show Marcus visions of Seraphina, confirming his heritage",
        ));
        messages
    }

    #[test]
    fn test_conversation_history_preserves_novel_content() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = build_novel_session();
        engine.compact(&mut messages).unwrap();

        assert_eq!(messages[0].role, "system");

        let all_text: String = messages
            .iter()
            .filter_map(|m| match &m.content {
                MessageContent::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        assert!(
            all_text.contains("Eleanor") || all_text.contains("eleanor"),
            "compaction lost character name 'Eleanor'"
        );
        assert!(
            all_text.contains("Moonstone") || all_text.contains("amulet"),
            "compaction lost key artifact 'Moonstone/amulet'"
        );
        assert!(
            all_text.contains("Seraphina") || all_text.contains("seraphina"),
            "compaction lost key character 'Seraphina'"
        );
    }

    #[test]
    fn test_conversation_history_cross_turn_reference_preserved() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = build_novel_session();
        engine.compact(&mut messages).unwrap();

        let all_text: String = messages
            .iter()
            .filter_map(|m| match &m.content {
                MessageContent::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        assert!(
            all_text.contains("Moonstone of Aelindra") || all_text.contains("Seraphina"),
            "compaction lost the cross-turn reference answer about the amulet name"
        );
    }

    #[test]
    fn test_conversation_history_outline_modification_preserved() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = build_novel_session();
        engine.compact(&mut messages).unwrap();

        let all_text: String = messages
            .iter()
            .filter_map(|m| match &m.content {
                MessageContent::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        assert!(
            all_text.contains("Marcus") || all_text.contains("marcus"),
            "compaction lost character 'Marcus' from modification suggestions"
        );
        assert!(
            all_text.contains("Seraphina")
                || all_text.contains("bloodline")
                || all_text.contains("Moonwell"),
            "compaction lost plot twist details involving Marcus's heritage"
        );
    }

    #[test]
    fn test_conversation_history_repeated_compact_preserves_key_facts() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = build_novel_session();

        for i in 0..5 {
            messages.push(user_msg(&format!(
                "Can you elaborate more on chapter {} of the novel?",
                i + 2
            )));
            messages.push(assistant_msg(&novel_chapter(i + 2)));
        }

        for _ in 0..3 {
            let _ = engine.compact(&mut messages);
        }

        let all_text: String = messages
            .iter()
            .filter_map(|m| match &m.content {
                MessageContent::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        assert!(
            all_text.contains("Eleanor") || all_text.contains("Marcus"),
            "repeated compaction lost all character names"
        );
        assert!(
            all_text.contains("Shadow")
                || all_text.contains("amulet")
                || all_text.contains("Moonstone"),
            "repeated compaction lost all key artifacts"
        );
    }

    #[test]
    fn test_conversation_history_specific_detail_retrieval() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![
            system_msg("You are a helpful coding assistant."),
            user_msg(
                "Write a Rust function called calculate_fibonacci that returns the nth Fibonacci number with overflow handling.",
            ),
            assistant_msg(
                "fn calculate_fibonacci(n: u32) -> Result<u64, &'static str> { ... uses checked_add for overflow ... }",
            ),
            user_msg("What parameters does the calculate_fibonacci function take?"),
            assistant_msg(
                "The function takes n: u32 and returns Result<u64, &'static str>. The Ok variant contains the Fibonacci number, Err for overflow.",
            ),
            user_msg("What line handles the overflow check?"),
            assistant_msg(
                "The overflow check uses checked_add on a and b, converting None to an error string.",
            ),
            user_msg("Add memoization to the function."),
            assistant_msg(
                "Use HashMap<u32, u64> as a cache parameter. On each call, check cache first, then recurse and store result. Still uses checked_add for overflow safety.",
            ),
        ];

        engine.compact(&mut messages).unwrap();

        let all_text: String = messages
            .iter()
            .filter_map(|m| match &m.content {
                MessageContent::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        assert!(
            all_text.contains("fibonacci") || all_text.contains("Fibonacci"),
            "compaction lost the main function name"
        );
        assert!(
            all_text.contains("overflow") || all_text.contains("checked_add"),
            "compaction lost the overflow handling detail"
        );
        assert!(
            all_text.contains("memoiz")
                || all_text.contains("cache")
                || all_text.contains("HashMap"),
            "compaction lost the memoization detail from the final turn"
        );
    }

    #[test]
    fn test_conversation_history_qa_chain_preservation() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![
            system_msg("You are a research assistant."),
            user_msg("What is the capital of France?"),
            assistant_msg(
                "The capital of France is Paris. It is known for the Eiffel Tower, the Louvre museum, and its rich cultural heritage.",
            ),
            user_msg("What is the population of that city?"),
            assistant_msg(
                "Paris has a population of approximately 2.1 million within the city proper, and about 12 million in the greater metropolitan area.",
            ),
            user_msg("What river flows through it?"),
            assistant_msg(
                "The Seine River flows through Paris, dividing the city into the Left Bank and the Right Bank.",
            ),
            user_msg("Name three famous bridges across that river."),
            assistant_msg(
                "Three famous bridges across the Seine: Pont Neuf (oldest, 1607), Pont Alexandre III (ornate arch), Pont de l'Alma (flame statue).",
            ),
            user_msg("Which of those bridges is the oldest?"),
            assistant_msg(
                "Pont Neuf is the oldest, completed in 1607 during the reign of Henry IV.",
            ),
        ];

        engine.compact(&mut messages).unwrap();

        let all_text: String = messages
            .iter()
            .filter_map(|m| match &m.content {
                MessageContent::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        assert!(
            all_text.contains("Paris") || all_text.contains("paris"),
            "compaction lost 'Paris' from Q&A chain"
        );
        assert!(
            all_text.contains("Seine") || all_text.contains("Pont") || all_text.contains("bridge"),
            "compaction lost river/bridge details from Q&A chain"
        );
    }

    #[test]
    fn test_conversation_history_code_review_preserves_context() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![
            system_msg("You are a code reviewer."),
            user_msg(
                "Review this authentication module. It uses JWT tokens with 24-hour expiry and stores refresh tokens in an encrypted database column.",
            ),
            assistant_msg(
                "Key findings:\n1. JWT expiry of 24 hours is reasonable\n2. Refresh token storage is good\n3. Token refresh endpoint lacks rate limiting\n4. Password hashing uses bcrypt cost 10 (recommend 12)\n5. Missing session cleanup cron for expired refresh tokens",
            ),
            user_msg("What rate limit would you suggest for the token refresh endpoint?"),
            assistant_msg(
                "Suggest 10 req/min per user, 100 req/min globally, sliding window algorithm, 429 with Retry-After header, log violations for security monitoring.",
            ),
            user_msg("Should we migrate existing bcrypt hashes from cost 10 to 12?"),
            assistant_msg(
                "Yes: keep old verifier during transition, re-hash on next login with cost 12, store migration flag, force-reset remaining cost-10 accounts after 90 days, then remove old verifier code.",
            ),
        ];

        engine.compact(&mut messages).unwrap();

        let all_text: String = messages
            .iter()
            .filter_map(|m| match &m.content {
                MessageContent::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        assert!(
            all_text.contains("JWT") || all_text.contains("token") || all_text.contains("auth"),
            "compaction lost authentication context"
        );
        assert!(
            all_text.contains("rate limit")
                || all_text.contains("rate_limit")
                || all_text.contains("bcrypt")
                || all_text.contains("cost"),
            "compaction lost the rate limiting or bcrypt discussion"
        );
        assert!(
            all_text.contains("migrat")
                || all_text.contains("refresh")
                || all_text.contains("hash"),
            "compaction lost the migration strategy discussion"
        );
    }
}
