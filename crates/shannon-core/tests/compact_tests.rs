//! Integration tests for the context compression module (`shannon_core::compact`).
//!
//! Covers: CompactConfig, CompactError, CompactStrategy, CompactResult,
//! MessageGroup, CompactPrompt, CompactEngine, RuleBasedSummarizer.

use shannon_core::api::{ContentBlock, Message, MessageContent, ToolResultContent};
use shannon_core::compact::*;
use std::time::Duration;

// ============================================================================
// Helpers
// ============================================================================

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
            input: serde_json::json!({ "command": input }),
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

/// Build a message whose estimated token count exceeds the default
/// `micro_compact_threshold` of 4096 tokens (~16384 chars at 4 chars/token).
fn large_user_msg() -> Message {
    let long_text = "A".repeat(20_000);
    user_msg(&long_text)
}

/// A custom [`Summarizer`] that returns a fixed string, useful for testing
/// engine behaviour independently of the rule-based summarizer logic.
#[derive(Debug, Clone, Default)]
struct StubSummarizer {
    summary: String,
    micro_summary: String,
}

impl StubSummarizer {
    fn new(summary: &str, micro_summary: &str) -> Self {
        Self {
            summary: summary.to_string(),
            micro_summary: micro_summary.to_string(),
        }
    }
}

impl Summarizer for StubSummarizer {
    fn summarize(&self, _messages: &[Message], _max_tokens: usize) -> Result<String, CompactError> {
        Ok(self.summary.clone())
    }

    fn micro_summarize(
        &self,
        _message: &Message,
        _max_tokens: usize,
    ) -> Result<String, CompactError> {
        Ok(self.micro_summary.clone())
    }
}

// ============================================================================
// 1. CompactConfig
// ============================================================================

mod compact_config_tests {
    use super::*;

    #[test]
    fn default_values_match_documentation() {
        let cfg = CompactConfig::default();
        assert_eq!(cfg.max_output_tokens, 2000);
        assert_eq!(cfg.keep_recent_count, 10);
        assert!((cfg.trigger_threshold - 0.92).abs() < f32::EPSILON);
        assert!((cfg.warning_threshold - 0.85).abs() < f32::EPSILON);
        assert!((cfg.critical_threshold - 0.97).abs() < f32::EPSILON);
        assert_eq!(cfg.reserved_summary_tokens, 20000);
        assert!(cfg.enable_micro_compact);
        assert_eq!(cfg.micro_compact_threshold, 4096);
        assert!(cfg.enable_session_memory_compact);
        assert_eq!(cfg.max_context_tokens, 200_000);
    }

    #[test]
    fn with_max_context_overrides_only_that_field() {
        let cfg = CompactConfig::with_max_context(50_000);
        assert_eq!(cfg.max_context_tokens, 50_000);
        // All other fields should remain at defaults.
        assert_eq!(cfg.max_output_tokens, 2000);
        assert_eq!(cfg.keep_recent_count, 10);
        assert!((cfg.trigger_threshold - 0.92).abs() < f32::EPSILON);
    }

    #[test]
    fn validate_accepts_valid_config() {
        assert!(CompactConfig::default().validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_max_output_tokens() {
        let cfg = CompactConfig {
            max_output_tokens: 0,
            ..Default::default()
        };
        let err = cfg.validate().unwrap_err();
        assert!(
            matches!(err, CompactError::InvalidConfig(ref msg) if msg.contains("max_output_tokens"))
        );
    }

    #[test]
    fn validate_rejects_zero_keep_recent_count() {
        let cfg = CompactConfig {
            keep_recent_count: 0,
            ..Default::default()
        };
        let err = cfg.validate().unwrap_err();
        assert!(
            matches!(err, CompactError::InvalidConfig(ref msg) if msg.contains("keep_recent_count"))
        );
    }

    #[test]
    fn validate_rejects_zero_trigger_threshold() {
        let cfg = CompactConfig {
            trigger_threshold: 0.0,
            ..Default::default()
        };
        assert!(matches!(cfg.validate(), Err(CompactError::InvalidConfig(_))));
    }

    #[test]
    fn validate_rejects_trigger_threshold_above_one() {
        let cfg = CompactConfig {
            trigger_threshold: 1.5,
            ..Default::default()
        };
        assert!(matches!(cfg.validate(), Err(CompactError::InvalidConfig(_))));
    }

    #[test]
    fn validate_accepts_trigger_threshold_of_exactly_one() {
        let cfg = CompactConfig {
            trigger_threshold: 1.0,
            critical_threshold: 1.0,
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_max_context_tokens() {
        let cfg = CompactConfig {
            max_context_tokens: 0,
            ..Default::default()
        };
        let err = cfg.validate().unwrap_err();
        assert!(
            matches!(err, CompactError::InvalidConfig(ref msg) if msg.contains("max_context_tokens"))
        );
    }

    #[test]
    fn serialization_roundtrip_json() {
        let cfg = CompactConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let restored: CompactConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.max_output_tokens, cfg.max_output_tokens);
        assert_eq!(restored.keep_recent_count, cfg.keep_recent_count);
        assert!((restored.trigger_threshold - cfg.trigger_threshold).abs() < f32::EPSILON);
        assert_eq!(restored.enable_micro_compact, cfg.enable_micro_compact);
        assert_eq!(restored.micro_compact_threshold, cfg.micro_compact_threshold);
        assert_eq!(
            restored.enable_session_memory_compact,
            cfg.enable_session_memory_compact
        );
        assert_eq!(restored.max_context_tokens, cfg.max_context_tokens);
    }

    #[test]
    fn serialization_roundtrip_custom_values() {
        let cfg = CompactConfig {
            max_output_tokens: 500,
            keep_recent_count: 3,
            trigger_threshold: 0.6,
            warning_threshold: 0.85,
            critical_threshold: 0.97,
            reserved_summary_tokens: 20_000,
            enable_micro_compact: false,
            micro_compact_threshold: 2048,
            enable_session_memory_compact: false,
            max_context_tokens: 128_000,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let restored: CompactConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.max_output_tokens, 500);
        assert_eq!(restored.keep_recent_count, 3);
        assert!((restored.trigger_threshold - 0.6).abs() < f32::EPSILON);
        assert!(!restored.enable_micro_compact);
        assert_eq!(restored.micro_compact_threshold, 2048);
        assert!(!restored.enable_session_memory_compact);
        assert_eq!(restored.max_context_tokens, 128_000);
    }
}

// ============================================================================
// 2. CompactError Display Traits
// ============================================================================

mod compact_error_tests {
    use super::*;

    #[test]
    fn no_messages_to_compact_display() {
        let err = CompactError::NoMessagesToCompact;
        let msg = format!("{err}");
        assert!(msg.contains("No messages to compact"));
    }

    #[test]
    fn summarization_failed_display() {
        let err = CompactError::SummarizationFailed("LLM timeout".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("Summarization failed"));
        assert!(msg.contains("LLM timeout"));
    }

    #[test]
    fn invalid_config_display() {
        let err = CompactError::InvalidConfig("max_output_tokens must be > 0".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("Invalid configuration"));
        assert!(msg.contains("max_output_tokens"));
    }

    #[test]
    fn token_estimation_error_display() {
        let err = CompactError::TokenEstimationError("overflow".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("Token estimation error"));
    }

    #[test]
    fn already_in_progress_display() {
        let err = CompactError::AlreadyInProgress;
        let msg = format!("{err}");
        assert!(msg.contains("already in progress"));
    }

    #[test]
    fn timeout_display() {
        let err = CompactError::Timeout;
        let msg = format!("{err}");
        assert!(msg.contains("duration exceeded"));
    }

    #[test]
    fn error_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CompactError>();
    }
}

// ============================================================================
// 3. CompactStrategy Display Trait
// ============================================================================

mod compact_strategy_tests {
    use super::*;

    #[test]
    fn all_variants_have_display() {
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

    #[test]
    fn strategy_equality() {
        assert_eq!(CompactStrategy::SummarizeOld, CompactStrategy::SummarizeOld);
        assert_ne!(CompactStrategy::SummarizeOld, CompactStrategy::TruncateOld);
    }

    #[test]
    fn strategy_serialization_roundtrip() {
        let strategies = vec![
            CompactStrategy::TruncateOld,
            CompactStrategy::SummarizeOld,
            CompactStrategy::MicroCompress,
            CompactStrategy::GroupCompress,
            CompactStrategy::SessionMemoryCompress,
        ];
        for s in &strategies {
            let json = serde_json::to_string(s).unwrap();
            let restored: CompactStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(*s, restored);
        }
    }
}

// ============================================================================
// 4. CompactResult
// ============================================================================

mod compact_result_tests {
    use super::*;

    #[test]
    fn no_change_constructor() {
        let result = CompactResult::no_change(CompactStrategy::TruncateOld, 5000);
        assert_eq!(result.original_tokens, 5000);
        assert_eq!(result.compacted_tokens, 5000);
        assert!((result.reduction_ratio - 0.0).abs() < f32::EPSILON);
        assert_eq!(result.messages_removed, 0);
        assert_eq!(result.messages_compacted, 0);
        assert_eq!(result.duration, Duration::ZERO);
        assert_eq!(result.strategy, CompactStrategy::TruncateOld);
    }

    #[test]
    fn no_change_with_different_strategies() {
        for strategy in [
            CompactStrategy::TruncateOld,
            CompactStrategy::SummarizeOld,
            CompactStrategy::MicroCompress,
            CompactStrategy::GroupCompress,
            CompactStrategy::SessionMemoryCompress,
        ] {
            let result = CompactResult::no_change(strategy.clone(), 100);
            assert_eq!(result.strategy, strategy);
        }
    }

    #[test]
    fn display_format() {
        let result = CompactResult {
            original_tokens: 10_000,
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
    fn display_shows_zero_duration() {
        let result = CompactResult::no_change(CompactStrategy::MicroCompress, 100);
        let display = format!("{result}");
        assert!(display.contains("0.00s"));
    }

    #[test]
    fn serialization_roundtrip() {
        let result = CompactResult {
            original_tokens: 8000,
            compacted_tokens: 2000,
            reduction_ratio: 0.75,
            messages_removed: 10,
            messages_compacted: 10,
            duration: Duration::from_millis(42),
            strategy: CompactStrategy::GroupCompress,
        };
        let json = serde_json::to_string(&result).unwrap();
        let restored: CompactResult = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.original_tokens, 8000);
        assert_eq!(restored.compacted_tokens, 2000);
        assert!((restored.reduction_ratio - 0.75).abs() < f32::EPSILON);
        assert_eq!(restored.messages_removed, 10);
        assert_eq!(restored.messages_compacted, 10);
        assert_eq!(restored.strategy, CompactStrategy::GroupCompress);
    }
}

// ============================================================================
// 5. MessageGroup
// ============================================================================

mod message_group_tests {
    use super::*;

    #[test]
    fn user_turn_total_tokens() {
        let group = MessageGroup::UserTurn {
            messages: vec![
                GroupedMessage {
                    message: user_msg("Hello"),
                    original_index: 0,
                    estimated_tokens: 10,
                },
                GroupedMessage {
                    message: user_msg("World"),
                    original_index: 1,
                    estimated_tokens: 20,
                },
            ],
        };
        assert_eq!(group.total_tokens(), 30);
    }

    #[test]
    fn assistant_turn_total_tokens() {
        let group = MessageGroup::AssistantTurn {
            messages: vec![GroupedMessage {
                message: assistant_msg("Hi"),
                original_index: 0,
                estimated_tokens: 5,
            }],
        };
        assert_eq!(group.total_tokens(), 5);
    }

    #[test]
    fn tool_use_turn_total_tokens() {
        let group = MessageGroup::ToolUseTurn {
            tool_name: "bash".to_string(),
            tool_use_id: "t1".to_string(),
            messages: vec![
                GroupedMessage {
                    message: tool_use_msg("t1", "bash", "ls"),
                    original_index: 0,
                    estimated_tokens: 15,
                },
                GroupedMessage {
                    message: tool_result_msg("t1", "file.txt", false),
                    original_index: 1,
                    estimated_tokens: 25,
                },
            ],
        };
        assert_eq!(group.total_tokens(), 40);
    }

    #[test]
    fn system_message_total_tokens() {
        let group = MessageGroup::SystemMessage {
            messages: vec![GroupedMessage {
                message: system_msg("Preamble"),
                original_index: 0,
                estimated_tokens: 100,
            }],
        };
        assert_eq!(group.total_tokens(), 100);
    }

    #[test]
    fn total_tokens_empty_group() {
        let group = MessageGroup::UserTurn {
            messages: vec![],
        };
        assert_eq!(group.total_tokens(), 0);
    }

    #[test]
    fn messages_slice_returns_all_variants() {
        let gm = GroupedMessage {
            message: user_msg("x"),
            original_index: 0,
            estimated_tokens: 1,
        };

        let user = MessageGroup::UserTurn {
            messages: vec![gm.clone()],
        };
        assert_eq!(user.messages().len(), 1);

        let asst = MessageGroup::AssistantTurn {
            messages: vec![gm.clone()],
        };
        assert_eq!(asst.messages().len(), 1);

        let tool = MessageGroup::ToolUseTurn {
            tool_name: "t".to_string(),
            tool_use_id: "id".to_string(),
            messages: vec![gm.clone()],
        };
        assert_eq!(tool.messages().len(), 1);

        let sys = MessageGroup::SystemMessage {
            messages: vec![gm],
        };
        assert_eq!(sys.messages().len(), 1);
    }

    #[test]
    fn label_user_turn() {
        let group = MessageGroup::UserTurn {
            messages: vec![
                GroupedMessage {
                    message: user_msg("a"),
                    original_index: 0,
                    estimated_tokens: 1,
                },
                GroupedMessage {
                    message: user_msg("b"),
                    original_index: 1,
                    estimated_tokens: 1,
                },
            ],
        };
        let label = group.label();
        assert!(label.contains("UserTurn"));
        assert!(label.contains("2 messages"));
    }

    #[test]
    fn label_assistant_turn() {
        let group = MessageGroup::AssistantTurn {
            messages: vec![GroupedMessage {
                message: assistant_msg("hi"),
                original_index: 0,
                estimated_tokens: 1,
            }],
        };
        assert!(group.label().contains("AssistantTurn"));
        assert!(group.label().contains("1 messages"));
    }

    #[test]
    fn label_tool_use_turn() {
        let group = MessageGroup::ToolUseTurn {
            tool_name: "read_file".to_string(),
            tool_use_id: "tu_1".to_string(),
            messages: vec![GroupedMessage {
                message: tool_use_msg("tu_1", "read_file", "main.rs"),
                original_index: 0,
                estimated_tokens: 5,
            }],
        };
        let label = group.label();
        assert!(label.contains("ToolUse[read_file]"));
        assert!(label.contains("1 messages"));
    }

    #[test]
    fn label_system_message() {
        let group = MessageGroup::SystemMessage {
            messages: vec![GroupedMessage {
                message: system_msg("init"),
                original_index: 0,
                estimated_tokens: 1,
            }],
        };
        assert!(group.label().contains("SystemMessage"));
    }
}

// ============================================================================
// 6. CompactPrompt
// ============================================================================

mod compact_prompt_tests {
    use super::*;

    #[test]
    fn system_prompt_mentions_token_limit() {
        let prompt = CompactPrompt::system_prompt(1500);
        assert!(prompt.contains("1500"));
        assert!(prompt.contains("tokens"));
    }

    #[test]
    fn system_prompt_describes_task() {
        let prompt = CompactPrompt::system_prompt(2000);
        assert!(prompt.contains("conversation compression"));
        assert!(prompt.contains("summary"));
    }

    #[test]
    fn system_prompt_preserves_key_requirements() {
        let prompt = CompactPrompt::system_prompt(1000);
        assert!(prompt.contains("goals"));
        assert!(prompt.contains("decisions"));
        assert!(prompt.contains("File paths"));
        assert!(prompt.contains("errors"));
    }

    #[test]
    fn conversation_to_summarize_formats_roles() {
        let msgs = vec![
            user_msg("What is Rust?"),
            assistant_msg("A systems programming language."),
        ];
        let text = CompactPrompt::conversation_to_summarize(&msgs);
        assert!(text.contains("[user]"));
        assert!(text.contains("What is Rust?"));
        assert!(text.contains("[assistant]"));
        assert!(text.contains("systems programming language"));
    }

    #[test]
    fn conversation_to_summarize_empty_returns_empty() {
        let text = CompactPrompt::conversation_to_summarize(&[]);
        assert!(text.is_empty());
    }

    #[test]
    fn conversation_to_summarize_truncates_long_messages() {
        let long = "X".repeat(600);
        let msgs = vec![user_msg(&long)];
        let text = CompactPrompt::conversation_to_summarize(&msgs);
        // Should be truncated to ~500 chars + "..."
        assert!(text.len() < long.len());
        assert!(text.contains("..."));
    }

    #[test]
    fn micro_compact_prompt_contains_role_and_limit() {
        let msg = user_msg("Some content here");
        let prompt = CompactPrompt::micro_compact_prompt(&msg, 500);
        assert!(prompt.contains("user"));
        assert!(prompt.contains("500"));
        assert!(prompt.contains("Summarize"));
    }

    #[test]
    fn micro_compact_prompt_truncates_very_long_content() {
        let content = "Z".repeat(3000);
        let msg = assistant_msg(&content);
        let prompt = CompactPrompt::micro_compact_prompt(&msg, 100);
        assert!(prompt.len() < content.len() + 200);
    }
}

// ============================================================================
// 7. CompactEngine
// ============================================================================

mod compact_engine_tests {
    use super::*;

    // -- Creation --

    #[test]
    fn with_defaults_succeeds() {
        let engine = CompactEngine::with_defaults();
        assert!(engine.is_ok());
    }

    #[test]
    fn new_with_custom_summarizer() {
        let engine = CompactEngine::new(
            CompactConfig::default(),
            Box::new(StubSummarizer::new("stub summary", "stub micro")),
        );
        assert!(engine.is_ok());
    }

    #[test]
    fn new_rejects_invalid_config() {
        let cfg = CompactConfig {
            max_output_tokens: 0,
            ..Default::default()
        };
        let result = CompactEngine::new(cfg, Box::new(RuleBasedSummarizer::new()));
        assert!(matches!(result, Err(CompactError::InvalidConfig(_))));
    }

    // -- Config access --

    #[test]
    fn config_returns_reference() {
        let engine = CompactEngine::with_defaults().unwrap();
        let cfg = engine.config();
        assert_eq!(cfg.max_context_tokens, 200_000);
        assert_eq!(cfg.keep_recent_count, 10);
    }

    #[test]
    fn set_config_accepts_valid() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let new_cfg = CompactConfig {
            max_output_tokens: 3000,
            ..Default::default()
        };
        assert!(engine.set_config(new_cfg).is_ok());
        assert_eq!(engine.config().max_output_tokens, 3000);
    }

    #[test]
    fn set_config_rejects_invalid_and_preserves_old() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        // Set a known good config first.
        let good = CompactConfig {
            max_output_tokens: 3000,
            ..Default::default()
        };
        engine.set_config(good).unwrap();

        // Try to set an invalid config.
        let bad = CompactConfig {
            max_output_tokens: 0,
            ..Default::default()
        };
        assert!(engine.set_config(bad).is_err());
        // Original config should still be the good one.
        assert_eq!(engine.config().max_output_tokens, 3000);
    }

    // -- analyze_context --

    #[test]
    fn analyze_context_empty_messages() {
        let engine = CompactEngine::with_defaults().unwrap();
        let analysis = engine.analyze_context(&[]);
        assert_eq!(analysis.estimated_tokens, 0);
        assert!(!analysis.should_compact);
        assert_eq!(analysis.compactable_message_count, 0);
        assert_eq!(analysis.micro_compact_candidates, 0);
        assert!((analysis.context_usage_ratio - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn analyze_context_small_conversation_below_threshold() {
        let engine = CompactEngine::with_defaults().unwrap();
        let msgs = vec![
            user_msg("Hello"),
            assistant_msg("Hi"),
        ];
        let analysis = engine.analyze_context(&msgs);
        assert!(!analysis.should_compact);
        assert!(analysis.estimated_tokens > 0);
        assert!(analysis.context_usage_ratio < 0.1);
    }

    #[test]
    fn analyze_context_above_threshold_triggers_compact() {
        let engine = CompactEngine::new(
            CompactConfig {
                max_context_tokens: 100,
                trigger_threshold: 0.5,
                warning_threshold: 0.4,
                critical_threshold: 0.6,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let msgs: Vec<Message> = (0..50)
            .map(|i| user_msg(&format!("Message {i} with padding to add tokens")))
            .collect();

        let analysis = engine.analyze_context(&msgs);
        assert!(analysis.should_compact);
        assert!(analysis.context_usage_ratio > 0.5);
    }

    #[test]
    fn analyze_context_counts_micro_compact_candidates() {
        let engine = CompactEngine::with_defaults().unwrap();
        let msgs = vec![
            user_msg("short"),
            large_user_msg(), // >4096 tokens
            assistant_msg("short"),
        ];
        let analysis = engine.analyze_context(&msgs);
        assert_eq!(analysis.micro_compact_candidates, 1);
    }

    #[test]
    fn analyze_context_micro_compact_disabled() {
        let engine = CompactEngine::new(
            CompactConfig {
                enable_micro_compact: false,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();
        let msgs = vec![large_user_msg()];
        let analysis = engine.analyze_context(&msgs);
        assert_eq!(analysis.micro_compact_candidates, 0);
    }

    // -- auto_compact_check --

    #[test]
    fn auto_compact_check_returns_false_when_below_threshold() {
        let engine = CompactEngine::with_defaults().unwrap();
        let msgs = vec![user_msg("Hello"), assistant_msg("Hi")];
        assert!(!engine.auto_compact_check(&msgs));
    }

    #[test]
    fn auto_compact_check_returns_true_when_above_threshold() {
        let engine = CompactEngine::new(
            CompactConfig {
                max_context_tokens: 50,
                trigger_threshold: 0.5,
                warning_threshold: 0.4,
                critical_threshold: 0.6,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();
        let msgs: Vec<Message> = (0..30)
            .map(|_| user_msg("Enough text to consume tokens in the context window"))
            .collect();
        assert!(engine.auto_compact_check(&msgs));
    }

    // -- compact (full) --

    #[test]
    fn compact_empty_returns_error() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut msgs: Vec<Message> = vec![];
        let result = engine.compact(&mut msgs);
        assert!(matches!(result, Err(CompactError::NoMessagesToCompact)));
    }

    #[test]
    fn compact_too_few_messages_returns_no_change() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut msgs = vec![user_msg("Hi"), assistant_msg("Hello")];
        let result = engine.compact(&mut msgs).unwrap();
        assert_eq!(result.messages_removed, 0);
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn compact_reduces_message_count() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut msgs: Vec<Message> = (0..20)
            .flat_map(|i| {
                vec![
                    user_msg(&format!("User {i}")),
                    assistant_msg(&format!("Asst {i}")),
                ]
            })
            .collect();

        let original_len = msgs.len();
        let result = engine.compact(&mut msgs).unwrap();
        assert!(result.messages_removed > 0);
        assert!(msgs.len() < original_len);
        // Summary message should be first.
        assert_eq!(msgs[0].role, "system");
    }

    #[test]
    fn compact_preserves_recent_messages() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut msgs: Vec<Message> = (0..10)
            .flat_map(|i| {
                vec![
                    user_msg(&format!("U{i}")),
                    assistant_msg(&format!("A{i}")),
                ]
            })
            .collect();

        engine.compact(&mut msgs).unwrap();
        // 1 summary + 4 kept recent = 5
        assert_eq!(msgs.len(), 5);
        // Last two messages should be the most recent user/assistant pair.
        assert_eq!(msgs[msgs.len() - 2].role, "user");
        assert_eq!(msgs[msgs.len() - 1].role, "assistant");
    }

    #[test]
    fn compact_result_has_valid_metrics() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut msgs: Vec<Message> = (0..20)
            .flat_map(|i| {
                vec![
                    user_msg(&format!("Message {i}")),
                    assistant_msg(&format!("Response {i}")),
                ]
            })
            .collect();

        let result = engine.compact(&mut msgs).unwrap();
        assert!(result.original_tokens > 0);
        assert!(result.duration >= Duration::ZERO);
        assert!(result.reduction_ratio >= 0.0);
        assert!(result.messages_compacted > 0);
        assert_eq!(result.strategy, CompactStrategy::SummarizeOld);
    }

    #[test]
    fn compact_clears_in_progress_flag_on_success() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut msgs: Vec<Message> = (0..15)
            .flat_map(|i| {
                vec![
                    user_msg(&format!("U{i}")),
                    assistant_msg(&format!("A{i}")),
                ]
            })
            .collect();

        engine.compact(&mut msgs).unwrap();
        // The internal `compacting` flag should be cleared after completion.
        // Verify by calling compact again -- it should not return AlreadyInProgress.
        let result = engine.compact(&mut msgs);
        assert!(!matches!(result, Err(CompactError::AlreadyInProgress)));
    }

    #[test]
    fn compact_uses_custom_summarizer() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 2,
                ..Default::default()
            },
            Box::new(StubSummarizer::new("STUB_SUMMARY_CONTENT", "micro")),
        )
        .unwrap();

        let mut msgs: Vec<Message> = (0..10)
            .flat_map(|i| {
                vec![
                    user_msg(&format!("U{i}")),
                    assistant_msg(&format!("A{i}")),
                ]
            })
            .collect();

        engine.compact(&mut msgs).unwrap();
        // The summary message should contain our stub text.
        match &msgs[0].content {
            MessageContent::Text(text) => {
                assert!(text.contains("STUB_SUMMARY_CONTENT"));
            }
            _ => panic!("Expected Text content for summary message"),
        }
    }

    // -- micro_compact --

    #[test]
    fn micro_compact_compresses_large_messages() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut msgs = vec![
            user_msg("Normal"),
            large_user_msg(),
            assistant_msg("Normal response"),
        ];

        let result = engine.micro_compact(&mut msgs).unwrap();
        assert_eq!(result.messages_compacted, 1);
        assert_eq!(result.strategy, CompactStrategy::MicroCompress);
        assert_eq!(result.messages_removed, 0);
    }

    #[test]
    fn micro_compact_leaves_small_messages_untouched() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut msgs = vec![
            user_msg("Short"),
            assistant_msg("Also short"),
        ];
        let result = engine.micro_compact(&mut msgs).unwrap();
        assert_eq!(result.messages_compacted, 0);
        // Content should be unchanged.
        match &msgs[0].content {
            MessageContent::Text(t) => assert_eq!(t, "Short"),
            _ => panic!("Expected Text"),
        }
    }

    #[test]
    fn micro_compact_disabled_returns_no_change() {
        let engine = CompactEngine::new(
            CompactConfig {
                enable_micro_compact: false,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();
        let mut msgs = vec![large_user_msg()];
        let result = engine.micro_compact(&mut msgs).unwrap();
        assert_eq!(result.messages_compacted, 0);
        assert_eq!(result.strategy, CompactStrategy::MicroCompress);
    }

    #[test]
    fn micro_compact_with_empty_messages() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut msgs: Vec<Message> = vec![];
        let result = engine.micro_compact(&mut msgs).unwrap();
        assert_eq!(result.messages_compacted, 0);
        assert_eq!(result.original_tokens, 0);
    }

    // -- group_messages --

    #[test]
    fn group_messages_empty() {
        let engine = CompactEngine::with_defaults().unwrap();
        assert!(engine.group_messages(&[]).is_empty());
    }

    #[test]
    fn group_messages_user_assistant_user() {
        let engine = CompactEngine::with_defaults().unwrap();
        let msgs = vec![
            user_msg("Hello"),
            assistant_msg("Hi"),
            user_msg("How are you?"),
        ];
        let groups = engine.group_messages(&msgs);
        assert_eq!(groups.len(), 3);
        assert!(matches!(&groups[0], MessageGroup::UserTurn { .. }));
        assert!(matches!(&groups[1], MessageGroup::AssistantTurn { .. }));
        assert!(matches!(&groups[2], MessageGroup::UserTurn { .. }));
    }

    #[test]
    fn group_messages_consecutive_same_role() {
        let engine = CompactEngine::with_defaults().unwrap();
        let msgs = vec![
            system_msg("Preamble 1"),
            system_msg("Preamble 2"),
            user_msg("Question"),
        ];
        let groups = engine.group_messages(&msgs);
        assert_eq!(groups.len(), 2);
        // Two system messages should be in one group.
        assert!(matches!(
            &groups[0],
            MessageGroup::SystemMessage { messages } if messages.len() == 2
        ));
    }

    #[test]
    fn group_messages_tool_use_pairs_with_result() {
        let engine = CompactEngine::with_defaults().unwrap();
        let msgs = vec![
            user_msg("Run ls"),
            tool_use_msg("t1", "bash", "ls"),
            tool_result_msg("t1", "file1.txt", false),
            assistant_msg("Here are the files."),
        ];
        let groups = engine.group_messages(&msgs);
        // Expect: UserTurn, ToolUseTurn, AssistantTurn
        assert_eq!(groups.len(), 3);
        assert!(matches!(&groups[1], MessageGroup::ToolUseTurn { tool_name, .. } if tool_name == "bash"));
    }

    // -- post_compact_cleanup --

    #[test]
    fn post_compact_cleanup_removes_duplicate_summaries() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut msgs = vec![
            system_msg("[Previous conversation summary]\nContent A"),
            system_msg("[Previous conversation summary]\nContent A"),
            user_msg("Next question"),
        ];
        let removed = engine.post_compact_cleanup(&mut msgs);
        assert_eq!(removed, 1);
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn post_compact_cleanup_keeps_different_summaries() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut msgs = vec![
            system_msg("[Previous conversation summary]\nPart A"),
            system_msg("[Previous conversation summary]\nPart B"),
            user_msg("Question"),
        ];
        let removed = engine.post_compact_cleanup(&mut msgs);
        // Different summaries are not duplicates.
        assert_eq!(removed, 0);
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn post_compact_cleanup_collapses_excessive_system_messages() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut msgs = vec![
            system_msg("S1"),
            system_msg("S2"),
            system_msg("S3"),
            system_msg("S4"),
            system_msg("S5"),
            user_msg("Question"),
        ];
        let removed = engine.post_compact_cleanup(&mut msgs);
        // More than 3 consecutive system messages should be collapsed.
        assert!(removed > 0);
        assert!(msgs.len() < 6);
    }

    #[test]
    fn post_compact_cleanup_noop_for_normal_messages() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut msgs = vec![
            user_msg("Hello"),
            assistant_msg("Hi"),
            user_msg("Question"),
        ];
        let removed = engine.post_compact_cleanup(&mut msgs);
        assert_eq!(removed, 0);
        assert_eq!(msgs.len(), 3);
    }

    // -- group_compact --

    #[test]
    fn group_compact_empty_returns_error() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut msgs: Vec<Message> = vec![];
        let result = engine.group_compact(&mut msgs);
        assert!(matches!(result, Err(CompactError::NoMessagesToCompact)));
    }

    #[test]
    fn group_compact_too_few_returns_no_change() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut msgs = vec![user_msg("Hi"), assistant_msg("Hello")];
        let result = engine.group_compact(&mut msgs).unwrap();
        assert_eq!(result.messages_removed, 0);
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn group_compact_reduces_messages() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut msgs: Vec<Message> = (0..20)
            .flat_map(|i| {
                vec![
                    user_msg(&format!("User {i}")),
                    assistant_msg(&format!("Asst {i}")),
                ]
            })
            .collect();

        let original_len = msgs.len();
        let result = engine.group_compact(&mut msgs).unwrap();
        assert!(result.messages_removed > 0);
        assert!(msgs.len() < original_len);
        assert_eq!(result.strategy, CompactStrategy::GroupCompress);
        // First message should be the group-compacted summary.
        assert_eq!(msgs[0].role, "system");
        match &msgs[0].content {
            MessageContent::Text(t) => assert!(t.contains("Group-compacted")),
            _ => panic!("Expected Text content"),
        }
    }

    // -- session_memory_compact --

    #[test]
    fn session_memory_compact_empty_returns_error() {
        let engine = CompactEngine::with_defaults().unwrap();
        let result = engine.compact_session_memory(&[]);
        assert!(matches!(result, Err(CompactError::NoMessagesToCompact)));
    }

    #[test]
    fn session_memory_compact_disabled_returns_no_change() {
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
        assert_eq!(result.strategy, CompactStrategy::SessionMemoryCompress);
    }

    #[test]
    fn session_memory_compact_produces_summary() {
        let engine = CompactEngine::with_defaults().unwrap();
        let long = "X".repeat(500);
        let entries: Vec<Message> = (0..5)
            .map(|i| system_msg(&format!("Memory {i}: {long}")))
            .collect();

        let result = engine.compact_session_memory(&entries).unwrap();
        assert!(result.messages_removed > 0);
        assert!(result.original_tokens > result.compacted_tokens);
        assert_eq!(result.strategy, CompactStrategy::SessionMemoryCompress);
    }
}

// ============================================================================
// 8. RuleBasedSummarizer
// ============================================================================

mod rule_based_summarizer_tests {
    use super::*;

    #[test]
    fn summarize_empty_returns_error() {
        let s = RuleBasedSummarizer::new();
        let result = s.summarize(&[], 1000);
        assert!(matches!(result, Err(CompactError::NoMessagesToCompact)));
    }

    #[test]
    fn summarize_produces_summary_with_turn_count() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![
            user_msg("Hello"),
            assistant_msg("Hi there!"),
        ];
        let summary = s.summarize(&msgs, 1000).unwrap();
        assert!(summary.contains("User"));
        assert!(summary.contains("Hello"));
        assert!(summary.contains("Assistant"));
        assert!(summary.contains("2 messages"));
    }

    #[test]
    fn summarize_extracts_tool_names() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![
            user_msg("List files"),
            tool_use_msg("t1", "bash", "ls"),
        ];
        let summary = s.summarize(&msgs, 1000).unwrap();
        assert!(summary.contains("bash"));
        assert!(summary.contains("Tools used"));
    }

    #[test]
    fn summarize_extracts_file_paths() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![user_msg("Check src/main.rs and Cargo.toml")];
        let summary = s.summarize(&msgs, 1000).unwrap();
        assert!(summary.contains("src/main.rs"));
        assert!(summary.contains("Cargo.toml"));
        assert!(summary.contains("Files referenced"));
    }

    #[test]
    fn summarize_reports_errors() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: Some(ToolResultContent::Single("Command not found".to_string())),
                is_error: Some(true),
            }]),
        }];
        let summary = s.summarize(&msgs, 1000).unwrap();
        assert!(summary.contains("Errors encountered"));
        assert!(summary.contains("Command not found"));
    }

    #[test]
    fn summarize_handles_blocks_variant() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text {
                    text: "Here is the result.".to_string(),
                },
                ContentBlock::Image {
                    source: shannon_core::api::ImageSource::base64("image/png", "data"),
                },
            ]),
        }];
        let summary = s.summarize(&msgs, 1000).unwrap();
        assert!(summary.contains("Text:"));
        assert!(summary.contains("Image"));
    }

    #[test]
    fn micro_summarize_returns_compressed_text() {
        let s = RuleBasedSummarizer::new();
        let msg = user_msg("Some long content that should be compressed down");
        let result = s.micro_summarize(&msg, 100).unwrap();
        assert!(result.contains("Compressed"));
        assert!(result.contains("user"));
    }

    #[test]
    fn micro_summarize_truncates_long_content() {
        let s = RuleBasedSummarizer::new();
        let msg = user_msg(&"B".repeat(5000));
        let result = s.micro_summarize(&msg, 100).unwrap();
        // Rule-based micro_summarize truncates to ~500 chars.
        assert!(result.len() < 5000);
    }
}

// ============================================================================
// Integration / Workflow Tests
// ============================================================================

mod workflow_tests {
    use super::*;

    #[test]
    fn full_workflow_analyze_compact_cleanup() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                max_context_tokens: 200,
                trigger_threshold: 0.5,
                warning_threshold: 0.4,
                critical_threshold: 0.6,
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut msgs: Vec<Message> = (0..25)
            .flat_map(|i| {
                vec![
                    user_msg(&format!(
                        "User message {i} with extra padding text to exceed token budget"
                    )),
                    assistant_msg(&format!(
                        "Response {i} with extra padding text to exceed token budget significantly"
                    )),
                ]
            })
            .collect();

        // 1. Auto-compact should trigger.
        assert!(engine.auto_compact_check(&msgs));

        // 2. Full compact.
        let result = engine.compact(&mut msgs).unwrap();
        assert!(result.messages_removed > 0);

        // 3. Post-compact cleanup with a single summary should be a no-op.
        let cleaned = engine.post_compact_cleanup(&mut msgs);
        assert_eq!(cleaned, 0);
    }

    #[test]
    fn full_workflow_micro_compact_on_analysis() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut msgs = vec![
            user_msg("Normal"),
            large_user_msg(),
            assistant_msg("Normal response"),
        ];

        let analysis = engine.analyze_context(&msgs);
        assert_eq!(analysis.micro_compact_candidates, 1);

        let result = engine.micro_compact(&mut msgs).unwrap();
        assert_eq!(result.messages_compacted, 1);

        // After micro-compact, the large message should now be compressed.
        match &msgs[1].content {
            MessageContent::Text(text) => {
                assert!(text.contains("Compressed"));
                assert!(text.len() < 20_000);
            }
            _ => panic!("Expected compressed text content"),
        }
    }

    #[test]
    fn group_compact_with_mixed_tool_and_text_messages() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut msgs = vec![
            user_msg("Look at my project"),
            assistant_msg("Let me check."),
            tool_use_msg("t1", "bash", "find . -name '*.rs'"),
            tool_result_msg("t1", "src/main.rs\nsrc/lib.rs", false),
            assistant_msg("I found your Rust files."),
            user_msg("How many lines?"),
            tool_use_msg("t2", "bash", "wc -l src/main.rs"),
            tool_result_msg("t2", "42 src/main.rs", false),
            assistant_msg("42 lines total."),
            user_msg("Add a new function"),
            assistant_msg("Done! I've added the function."),
            user_msg("Now run the tests"),
            assistant_msg("All tests pass."),
            user_msg("Great, commit it"),
            assistant_msg("Committed."),
        ];

        let original_len = msgs.len();
        let result = engine.group_compact(&mut msgs).unwrap();
        assert!(result.messages_removed > 0);
        assert!(msgs.len() < original_len);
        // Last message should be the most recent assistant message.
        let last = msgs.last().unwrap();
        assert_eq!(last.role, "assistant");
    }

    #[test]
    fn compact_preserves_system_prompt_at_front() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut msgs = vec![system_msg("You are a helpful coding assistant.")];
        for i in 0..15 {
            msgs.push(user_msg(&format!("User query {i}")));
            msgs.push(assistant_msg(&format!("Response {i}")));
        }

        engine.compact(&mut msgs).unwrap();

        // The original system prompt should still be present somewhere.
        let has_prompt = msgs.iter().any(|m| {
            matches!(&m.content, MessageContent::Text(t) if t.contains("helpful coding assistant"))
        });
        assert!(has_prompt, "System prompt should be preserved after compaction");
    }

    #[test]
    fn double_compact_works_without_error() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 2,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut msgs: Vec<Message> = (0..15)
            .flat_map(|i| {
                vec![
                    user_msg(&format!("U{i}")),
                    assistant_msg(&format!("A{i}")),
                ]
            })
            .collect();

        let r1 = engine.compact(&mut msgs).unwrap();
        assert!(r1.messages_removed > 0);

        // Second compact on already-compacted messages should either
        // produce no change (too few) or succeed without error.
        let r2 = engine.compact(&mut msgs).unwrap();
        assert!(r2.messages_removed == 0 || r2.messages_removed > 0);
    }
}
