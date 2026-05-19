//! # Query Engine
//!
//! Main orchestrator for streaming query processing with tool orchestration.

mod context_injector;
mod engine;
mod streaming;
mod types;

// Re-export all public types to maintain the same public API as the original flat file.
pub use context_injector::ContextInjector;
pub use engine::QueryEngine;
pub use types::{
    CompressionStrategy, ConversationStats, CostEstimate, CostTracker, PermissionRequest, QueryContext, QueryEngineConfig, QueryError,
    QueryEvent, QueryMetadata, QueryStream,
};

#[cfg(test)]
mod tests {
    use super::*;
    use super::streaming::ConversationState;
    use crate::tools::{Tool, ToolOutput};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use uuid::Uuid;

    struct TestTool {
        name: String,
    }

    #[async_trait]
    impl Tool for TestTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "A test tool"
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                }
            })
        }

        async fn execute(
            &self,
            _input: serde_json::Value,
        ) -> Result<ToolOutput, crate::tools::ToolError> {
            Ok(ToolOutput {
                content: "Test executed".to_string(),
                is_error: false,
                metadata: HashMap::new(),
            })
        }
    }

    #[tokio::test]
    async fn test_query_context_creation() {
        let context = QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: "Hello".to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: Some(4096),
                model: "claude-3-5-sonnet-20241022".to_string(),
                temperature: Some(0.7),
                top_p: None,
            },
        };
        assert_eq!(context.user_message, "Hello");
        assert!(context.metadata.tools_allowed);
    }

    #[test]
    fn test_conversation_stats() {
        let stats = ConversationStats {
            message_count: 5,
            turn_count: 2,
            total_tokens: 1000,
            total_cost: 0.01,
        };
        assert_eq!(stats.message_count, 5);
        assert_eq!(stats.turn_count, 2);
    }

    #[test]
    fn test_query_engine_config_default() {
        let config = QueryEngineConfig::default();
        assert_eq!(config.max_turns, 20);
        assert_eq!(config.timeout_seconds, 300);
        assert!(!config.verbose);
        assert_eq!(config.max_context_tokens, None);
        assert_eq!(config.compression_threshold, 0.8);
        assert_eq!(config.keep_recent_messages, 10);
    }

    #[test]
    fn test_conversation_token_estimation() {
        let mut conv = ConversationState::default();
        conv.messages.push(crate::api::Message {
            role: "user".to_string(),
            content: crate::api::MessageContent::Text("Hello world".to_string()),
        });
        conv.messages.push(crate::api::Message {
            role: "assistant".to_string(),
            content: crate::api::MessageContent::Text("Hi there!".to_string()),
        });

        let tokens = conv.estimate_tokens();
        // "Hello world" (11) + "Hi there!" (10) = 21 chars / 4 ≈ 5 tokens
        assert!((4..=7).contains(&tokens));
    }

    #[test]
    fn test_conversation_compression_needed() {
        let config = QueryEngineConfig {
            max_context_tokens: Some(100),
            compression_threshold: 0.8,
            keep_recent_messages: 2,
            system_prompt: None,
            ..Default::default()
        };

        let mut conv = ConversationState::default();
        // Add small messages - under threshold
        for _ in 0..5 {
            conv.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Text("Hi".to_string()),
            });
        }

        assert!(!conv.needs_compression(&config));

        // Add many messages - over threshold
        for _ in 0..50 {
            conv.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Text(
                    "This is a longer message to increase token count".to_string(),
                ),
            });
        }

        assert!(conv.needs_compression(&config));
    }

    #[test]
    fn test_conversation_compress() {
        let config = QueryEngineConfig {
            keep_recent_messages: 2,
            ..Default::default()
        };

        let mut conv = ConversationState::default();
        for i in 0..8 {
            conv.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Text(format!("Message {i}")),
            });
        }

        let original_count = conv.messages.len();
        conv.compress(&config);

        // Cache-aware: [m0, m1, summary, m6, m7] = 5 messages
        assert_eq!(conv.messages.len(), 5);
        assert!(original_count > conv.messages.len());

        // First two messages are preserved cache prefix
        match &conv.messages[0].content {
            crate::api::MessageContent::Text(text) => {
                assert_eq!(text, "Message 0");
            }
            _ => panic!("First message should be preserved cache prefix"),
        }

        // Summary inserted after cache prefix
        match &conv.messages[2].content {
            crate::api::MessageContent::Text(text) => {
                assert!(text.contains("[Previous conversation summary]"));
                assert!(text.contains("Summary of"));
            }
            _ => panic!("Expected summary message after cache prefix"),
        }

        // Last message is the most recent
        match &conv.messages[4].content {
            crate::api::MessageContent::Text(text) => {
                assert_eq!(text, "Message 7");
            }
            _ => panic!("Last message should be the most recent"),
        }
    }

    // CostTracker tests

    #[test]
    fn test_cost_tracker_calculate_cost_sonnet() {
        let model = "claude-3-5-sonnet-20241022";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 0);
        assert!((cost - 3.0).abs() < 0.001);

        let cost = CostTracker::calculate_cost(model, 0, 1_000_000);
        assert!((cost - 15.0).abs() < 0.001);

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_haiku() {
        let model = "claude-3-5-haiku-20241022";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 0);
        assert!((cost - 0.80).abs() < 0.001);

        let cost = CostTracker::calculate_cost(model, 0, 1_000_000);
        assert!((cost - 4.0).abs() < 0.001);

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 4.80).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_opus() {
        let model = "claude-3-opus-20240229";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 0);
        assert!((cost - 15.0).abs() < 0.001);

        let cost = CostTracker::calculate_cost(model, 0, 1_000_000);
        assert!((cost - 75.0).abs() < 0.001);

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 90.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_sonnet4() {
        let model = "claude-sonnet-4-20250514";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_default() {
        let model = "unknown-model";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_small_tokens() {
        let model = "claude-3-5-sonnet-20241022";

        let cost = CostTracker::calculate_cost(model, 1000, 500);
        let expected = (1000.0 / 1_000_000.0) * 3.0 + (500.0 / 1_000_000.0) * 15.0;
        assert!((cost - expected).abs() < 0.000001);
    }

    #[test]
    fn test_cost_tracker_record_usage() {
        let mut tracker = CostTracker::new("claude-3-5-sonnet-20241022".to_string());

        tracker.record_usage("claude-3-5-sonnet-20241022", 100_000, 50_000);
        assert_eq!(tracker.total_input_tokens, 100_000);
        assert_eq!(tracker.total_output_tokens, 50_000);
        assert!(tracker.total_cost_usd > 0.0);

        tracker.record_usage("claude-3-5-sonnet-20241022", 200_000, 100_000);
        assert_eq!(tracker.total_input_tokens, 300_000);
        assert_eq!(tracker.total_output_tokens, 150_000);
        assert!(tracker.total_cost_usd > 0.001);
    }

    #[test]
    fn test_cost_tracker_summary() {
        let tracker = CostTracker::new("claude-3-5-haiku".to_string());
        let summary = tracker.summary();

        assert!(summary.contains("claude-3-5-haiku"));
        assert!(summary.contains("Input tokens:"));
        assert!(summary.contains("Output tokens:"));
        assert!(summary.contains("Total cost:"));
    }

    #[test]
    fn test_cost_tracker_total_cost() {
        let mut tracker = CostTracker::new("claude-3-opus".to_string());

        assert!((tracker.total_cost() - 0.0).abs() < 0.001);

        tracker.record_usage("claude-3-opus", 1_000_000, 1_000_000);
        // Opus: $15 input + $75 output = $90 total
        assert!((tracker.total_cost() - 90.0).abs() < 0.001);
    }

    // OpenAI model cost tests

    #[test]
    fn test_cost_tracker_calculate_cost_gpt4o() {
        let model = "gpt-4o";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 0);
        assert!((cost - 2.5).abs() < 0.001);

        let cost = CostTracker::calculate_cost(model, 0, 1_000_000);
        assert!((cost - 10.0).abs() < 0.001);

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 12.5).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_gpt4turbo() {
        let model = "gpt-4-turbo";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 40.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_gpt35turbo() {
        let model = "gpt-3.5-turbo";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 2.0).abs() < 0.001);
    }

    // Ollama local model cost tests (free)

    #[test]
    fn test_cost_tracker_calculate_cost_ollama_llama() {
        let model = "llama3";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_ollama_mistral() {
        let model = "mistral:7b";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_ollama_qwen() {
        let model = "qwen:72b";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 0.0).abs() < 0.001);
    }

    // Mixed model cost tracking

    #[test]
    fn test_cost_tracker_mixed_models() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());

        // Claude Sonnet 4: $3 + $15 = $18
        tracker.record_usage("claude-sonnet-4-20250514", 1_000_000, 1_000_000);

        // GPT-4o: $2.5 + $10 = $12.50
        tracker.record_usage("gpt-4o", 1_000_000, 1_000_000);

        // Ollama: $0
        tracker.record_usage("llama3:70b", 1_000_000, 1_000_000);

        // Total: $18 + $12.50 + $0 = $30.50
        assert!((tracker.total_cost() - 30.5).abs() < 0.001);
        assert_eq!(tracker.total_input_tokens, 3_000_000);
        assert_eq!(tracker.total_output_tokens, 3_000_000);
    }

    #[test]
    fn test_cost_tracker_zero_tokens() {
        let model = "claude-sonnet-4";
        let cost = CostTracker::calculate_cost(model, 0, 0);
        assert!((cost - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_summary_includes_model_name() {
        let mut tracker = CostTracker::new("gpt-4o".to_string());
        tracker.record_usage("gpt-4o", 500_000, 250_000);

        let summary = tracker.summary();
        assert!(summary.contains("gpt-4o"));
        assert!(summary.contains("500000"));
        assert!(summary.contains("250000"));
    }

    // QueryError display tests

    #[test]
    fn test_query_error_display_messages() {
        let err = QueryError::ApiError("rate limited".to_string());
        assert!(err.to_string().contains("API error"));
        assert!(err.to_string().contains("rate limited"));

        let err = QueryError::ToolError("bash failed".to_string());
        assert!(err.to_string().contains("Tool execution error"));

        let err = QueryError::PermissionDenied("read blocked".to_string());
        assert!(err.to_string().contains("Permission denied"));

        let err = QueryError::StateError("session lost".to_string());
        assert!(err.to_string().contains("State error"));

        let err = QueryError::InvalidQuery("empty".to_string());
        assert!(err.to_string().contains("Invalid query"));

        let err = QueryError::RateLimitExceeded;
        assert!(err.to_string().contains("Rate limit"));

        let err = QueryError::Timeout;
        assert!(err.to_string().contains("timeout"));

        let err = QueryError::ConfigurationError("bad key".to_string());
        assert!(err.to_string().contains("Configuration error"));
    }

    // QueryEvent variant construction tests

    #[test]
    fn test_query_event_started() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Started { query_id: id };
        match event {
            QueryEvent::Started { query_id } => assert_eq!(query_id, id),
            _ => panic!("Expected Started variant"),
        }
    }

    #[test]
    fn test_query_event_text() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Text {
            query_id: id,
            content: "Hello world".to_string(),
        };
        match event {
            QueryEvent::Text { content, .. } => assert_eq!(content, "Hello world"),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_query_event_tool_use_request() {
        let id = Uuid::new_v4();
        let event = QueryEvent::ToolUseRequest {
            query_id: id,
            tool_use_id: "tool_123".to_string(),
            tool_name: "bash".to_string(),
            tool_input: serde_json::json!({"command": "ls"}),
        };
        match event {
            QueryEvent::ToolUseRequest {
                tool_name,
                tool_input,
                ..
            } => {
                assert_eq!(tool_name, "bash");
                assert_eq!(tool_input["command"], "ls");
            }
            _ => panic!("Expected ToolUseRequest variant"),
        }
    }

    #[test]
    fn test_query_event_tool_use_result() {
        let id = Uuid::new_v4();
        let event = QueryEvent::ToolUseResult {
            query_id: id,
            tool_use_id: "tool_456".to_string(),
            tool_name: "read".to_string(),
            result: "file contents".to_string(),
            is_error: false,
        };
        match event {
            QueryEvent::ToolUseResult {
                result, is_error, ..
            } => {
                assert_eq!(result, "file contents");
                assert!(!is_error);
            }
            _ => panic!("Expected ToolUseResult variant"),
        }
    }

    #[test]
    fn test_query_event_tool_use_result_error() {
        let event = QueryEvent::ToolUseResult {
            query_id: Uuid::new_v4(),
            tool_use_id: "t1".to_string(),
            tool_name: "bash".to_string(),
            result: "permission denied".to_string(),
            is_error: true,
        };
        match event {
            QueryEvent::ToolUseResult { is_error, .. } => assert!(is_error),
            _ => panic!("Expected ToolUseResult variant"),
        }
    }

    #[test]
    fn test_query_event_turn_completed() {
        let id = Uuid::new_v4();
        let event = QueryEvent::TurnCompleted {
            query_id: id,
            turn_number: 3,
            tokens_used: 1500,
        };
        match event {
            QueryEvent::TurnCompleted {
                turn_number,
                tokens_used,
                ..
            } => {
                assert_eq!(turn_number, 3);
                assert_eq!(tokens_used, 1500);
            }
            _ => panic!("Expected TurnCompleted variant"),
        }
    }

    #[test]
    fn test_query_event_completed() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Completed { query_id: id };
        assert!(matches!(event, QueryEvent::Completed { query_id: _ }));
    }

    #[test]
    fn test_query_event_failed() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Failed {
            query_id: id,
            error: "timeout".to_string(),
        };
        match event {
            QueryEvent::Failed { error, .. } => assert_eq!(error, "timeout"),
            _ => panic!("Expected Failed variant"),
        }
    }

    #[test]
    fn test_query_event_progress() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Progress {
            query_id: id,
            message: "Processing...".to_string(),
        };
        match event {
            QueryEvent::Progress { message, .. } => assert_eq!(message, "Processing..."),
            _ => panic!("Expected Progress variant"),
        }
    }

    #[test]
    fn test_query_event_usage() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Usage {
            query_id: id,
            input_tokens: 1000,
            output_tokens: 500,
            cost_usd: 0.015,
            cache_creation_tokens: 200,
            cache_read_tokens: 500,
        };
        match event {
            QueryEvent::Usage {
                input_tokens,
                output_tokens,
                cost_usd,
                cache_creation_tokens,
                cache_read_tokens,
                ..
            } => {
                assert_eq!(input_tokens, 1000);
                assert_eq!(output_tokens, 500);
                assert!((cost_usd - 0.015).abs() < 0.0001);
                assert_eq!(cache_creation_tokens, 200);
                assert_eq!(cache_read_tokens, 500);
            }
            _ => panic!("Expected Usage variant"),
        }
    }

    #[test]
    fn test_usage_cache_tokens_deserialize() {
        // Simulate Anthropic API response with cache tokens
        let json = r#"{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":200,"cache_read_input_tokens":500}"#;
        let usage: crate::api::Usage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_creation_input_tokens, 200);
        assert_eq!(usage.cache_read_input_tokens, 500);
    }

    #[test]
    fn test_usage_cache_tokens_default() {
        // API response without cache tokens should default to 0
        let json = r#"{"input_tokens":100,"output_tokens":50}"#;
        let usage: crate::api::Usage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.cache_creation_input_tokens, 0);
        assert_eq!(usage.cache_read_input_tokens, 0);
    }

    #[test]
    fn test_query_event_cost() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Cost {
            query_id: id,
            total_cost_usd: 1.23,
            input_tokens: 50000,
            output_tokens: 25000,
        };
        match event {
            QueryEvent::Cost {
                total_cost_usd,
                input_tokens,
                output_tokens,
                ..
            } => {
                assert!((total_cost_usd - 1.23).abs() < 0.001);
                assert_eq!(input_tokens, 50000);
                assert_eq!(output_tokens, 25000);
            }
            _ => panic!("Expected Cost variant"),
        }
    }

    #[test]
    fn test_query_event_info() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Info {
            query_id: id,
            message: "compaction: 50000 -> 10000 tokens (80% reduction)".to_string(),
        };
        match event {
            QueryEvent::Info { message, .. } => {
                assert!(message.contains("compaction"));
                assert!(message.contains("80%"));
            }
            _ => panic!("Expected Info variant"),
        }
    }

    // QueryMetadata serialization tests

    #[test]
    fn test_query_metadata_serialization_roundtrip() {
        let metadata = QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: true,
            max_tokens: Some(8192),
            model: "claude-sonnet-4-20250514".to_string(),
            temperature: Some(0.7),
            top_p: Some(0.95),
        };
        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: QueryMetadata = serde_json::from_str(&json).unwrap();
        assert!(deserialized.tools_allowed);
        assert_eq!(deserialized.max_tokens, Some(8192));
        assert_eq!(deserialized.model, "claude-sonnet-4-20250514");
        assert_eq!(deserialized.temperature, Some(0.7));
        assert_eq!(deserialized.top_p, Some(0.95));
    }

    #[test]
    fn test_query_metadata_serialization_none_fields() {
        let metadata = QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: false,
            max_tokens: None,
            model: "gpt-4o".to_string(),
            temperature: None,
            top_p: None,
        };
        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: QueryMetadata = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.tools_allowed);
        assert!(deserialized.max_tokens.is_none());
        assert!(deserialized.temperature.is_none());
        assert!(deserialized.top_p.is_none());
    }

    // ConversationState edge case tests

    #[test]
    fn test_conversation_state_default() {
        let conv = ConversationState::default();
        assert!(conv.messages.is_empty());
        assert_eq!(conv.turn_count, 0);
        assert_eq!(conv.total_tokens, 0);
        assert!((conv.total_cost - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_conversation_estimate_tokens_empty() {
        let conv = ConversationState::default();
        assert_eq!(conv.estimate_tokens(), 0);
    }

    #[test]
    fn test_conversation_estimate_tokens_blocks_content() {
        use crate::api::{ContentBlock, MessageContent};
        let mut conv = ConversationState::default();

        conv.messages.push(crate::api::Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::Text {
                text: "Hello from block".to_string(),
            }]),
        });

        let tokens = conv.estimate_tokens();
        assert!((3..=6).contains(&tokens));
    }

    #[test]
    fn test_conversation_estimate_tokens_tool_use_block() {
        use crate::api::{ContentBlock, MessageContent};
        let mut conv = ConversationState::default();

        conv.messages.push(crate::api::Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "bash".to_string(),
                input: serde_json::json!({"command": "ls -la"}),
            }]),
        });

        let tokens = conv.estimate_tokens();
        assert!(tokens > 0);
    }

    #[test]
    fn test_conversation_compress_empty_does_nothing() {
        let config = QueryEngineConfig::default();
        let mut conv = ConversationState::default();
        conv.compress(&config);
        assert!(conv.messages.is_empty());
    }

    #[test]
    fn test_conversation_compress_few_messages_no_change() {
        let config = QueryEngineConfig {
            keep_recent_messages: 5,
            ..Default::default()
        };
        let mut conv = ConversationState::default();
        for i in 0..4 {
            conv.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Text(format!("Msg {i}")),
            });
        }
        conv.compress(&config);
        assert_eq!(conv.messages.len(), 4);
    }

    #[test]
    fn test_conversation_compress_exactly_threshold() {
        let config = QueryEngineConfig {
            keep_recent_messages: 2,
            ..Default::default()
        };
        let mut conv = ConversationState::default();
        // Need enough messages that split_point > min_preserve (2)
        for i in 0..6 {
            conv.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Text(format!("Message {i}")),
            });
        }
        conv.compress(&config);
        // drain(2..4) removes 2 messages, summary inserted at 2
        // [m0, m1, summary, m4, m5] = 5 messages
        assert_eq!(conv.messages.len(), 5);
        // First 2 preserved for cache prefix
        match &conv.messages[0].content {
            crate::api::MessageContent::Text(text) => {
                assert_eq!(text, "Message 0");
            }
            _ => panic!("Expected text content"),
        }
        // Summary inserted after cache prefix
        match &conv.messages[2].content {
            crate::api::MessageContent::Text(text) => {
                assert!(text.contains("[Previous conversation summary]"));
            }
            _ => panic!("Expected text content"),
        }
        // Recent messages preserved at the tail
        match &conv.messages[4].content {
            crate::api::MessageContent::Text(text) => {
                assert_eq!(text, "Message 5");
            }
            _ => panic!("Expected text content"),
        }
    }

    #[test]
    fn test_conversation_needs_compression_no_limit() {
        let config = QueryEngineConfig {
            max_context_tokens: None,
            ..Default::default()
        };
        let mut conv = ConversationState::default();
        for _ in 0..1000 {
            conv.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Text("A very long message".repeat(100)),
            });
        }
        assert!(!conv.needs_compression(&config));
    }

    #[test]
    fn test_conversation_needs_compression_under_threshold() {
        let config = QueryEngineConfig {
            max_context_tokens: Some(10000),
            compression_threshold: 0.8,
            ..Default::default()
        };
        let mut conv = ConversationState::default();
        conv.messages.push(crate::api::Message {
            role: "user".to_string(),
            content: crate::api::MessageContent::Text("Hi".to_string()),
        });
        assert!(!conv.needs_compression(&config));
    }

    // CostTracker edge cases

    #[test]
    fn test_cost_tracker_new_initializes_zero() {
        let tracker = CostTracker::new("claude-3-5-sonnet".to_string());
        assert_eq!(tracker.total_input_tokens, 0);
        assert_eq!(tracker.total_output_tokens, 0);
        assert!((tracker.total_cost_usd - 0.0).abs() < 0.001);
        assert_eq!(tracker.model_name, "claude-3-5-sonnet");
    }

    #[test]
    fn test_cost_tracker_default_is_sonnet() {
        let tracker = CostTracker::default();
        assert_eq!(tracker.model_name, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_cost_tracker_accumulates_correctly() {
        let mut tracker = CostTracker::new("gpt-4o".to_string());

        tracker.record_usage("gpt-4o", 100_000, 50_000);
        tracker.record_usage("gpt-4o", 200_000, 100_000);
        tracker.record_usage("gpt-4o", 300_000, 150_000);

        assert_eq!(tracker.total_input_tokens, 600_000);
        assert_eq!(tracker.total_output_tokens, 300_000);

        let expected = (600_000.0 / 1_000_000.0) * 2.5 + (300_000.0 / 1_000_000.0) * 10.0;
        assert!((tracker.total_cost() - expected).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost_case_sensitivity() {
        let cost_lower = CostTracker::calculate_cost("claude-3-5-sonnet", 1_000_000, 0);
        let cost_mixed = CostTracker::calculate_cost("Claude-3-5-Sonnet", 1_000_000, 0);
        assert!((cost_lower - 3.0).abs() < 0.001);
        assert!((cost_mixed - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost_model_with_prefix() {
        let cost = CostTracker::calculate_cost("gpt-4o-mini", 1_000_000, 1_000_000);
        assert!((cost - 0.75).abs() < 0.001);
    }

    // QueryEngineConfig edge cases

    #[test]
    fn test_query_engine_config_custom() {
        let config = QueryEngineConfig {
            max_turns: 5,
            max_budget_usd: Some(1.0),
            timeout_seconds: 60,
            verbose: true,
            enable_thinking: false,
            max_context_tokens: Some(50_000),
            compression_threshold: 0.6,
            keep_recent_messages: 5,
            compression_strategy: CompressionStrategy::default(),
            system_prompt: None,
            auto_commit: false,
            effort_level: None,
            focus_area: None,
            fast_model: None,
            plan_model: None,
            max_parallel_tools: 10,
        };
        assert_eq!(config.max_turns, 5);
        assert_eq!(config.max_budget_usd, Some(1.0));
        assert_eq!(config.timeout_seconds, 60);
        assert!(config.verbose);
        assert!(!config.enable_thinking);
        assert_eq!(config.max_context_tokens, Some(50_000));
        assert!((config.compression_threshold - 0.6).abs() < 0.001);
    }

    // ConversationStats tests

    #[test]
    fn test_conversation_stats_debug() {
        let stats = ConversationStats {
            message_count: 10,
            turn_count: 5,
            total_tokens: 5000,
            total_cost: 0.25,
        };
        let debug_str = format!("{stats:?}");
        assert!(debug_str.contains("message_count"));
        assert!(debug_str.contains("turn_count"));
    }

    #[test]
    fn test_conversation_stats_clone() {
        let stats = ConversationStats {
            message_count: 3,
            turn_count: 1,
            total_tokens: 500,
            total_cost: 0.01,
        };
        let cloned = stats.clone();
        assert_eq!(cloned.message_count, stats.message_count);
        assert_eq!(cloned.turn_count, stats.turn_count);
        assert_eq!(cloned.total_tokens, stats.total_tokens);
    }

    // QueryContext tests

    #[test]
    fn test_query_context_debug() {
        let ctx = QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: "test query".to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: false,
                max_tokens: None,
                model: "test-model".to_string(),
                temperature: None,
                top_p: None,
            },
        };
        let debug_str = format!("{ctx:?}");
        assert!(debug_str.contains("test query"));
    }

    // QueryStream type alias test

    #[test]
    fn test_query_stream_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<QueryStream>();
    }

    // ConversationState compress edge cases

    #[test]
    fn test_conversation_compress_minimum_messages() {
        let mut state = ConversationState::default();
        let config = QueryEngineConfig {
            keep_recent_messages: 3,
            ..QueryEngineConfig::default()
        };

        // Need enough messages that split_point > min_preserve (2)
        for i in 0..8 {
            state.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Blocks(vec![crate::api::ContentBlock::Text {
                    text: format!("Message number {i}"),
                }]),
            });
        }

        let before = state.messages.len();
        state.compress(&config);
        assert!(state.messages.len() < before);
    }

    #[test]
    fn test_conversation_compress_preserves_recent_order() {
        let mut state = ConversationState::default();
        let config = QueryEngineConfig {
            keep_recent_messages: 2,
            ..QueryEngineConfig::default()
        };

        for i in 0..4 {
            state.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Blocks(vec![crate::api::ContentBlock::Text {
                    text: format!("Msg {i}"),
                }]),
            });
        }

        state.compress(&config);

        let len = state.messages.len();
        if let crate::api::MessageContent::Blocks(blocks) = &state.messages[len - 2].content {
            if let crate::api::ContentBlock::Text { text: t1 } = &blocks[0] {
                assert!(t1.contains("Msg 2"));
            }
        }
        if let crate::api::MessageContent::Blocks(blocks) = &state.messages[len - 1].content {
            if let crate::api::ContentBlock::Text { text: t2 } = &blocks[0] {
                assert!(t2.contains("Msg 3"));
            }
        }
    }

    #[test]
    fn test_conversation_state_estimate_tokens_with_tool_use() {
        let mut state = ConversationState::default();
        state.messages.push(crate::api::Message {
            role: "assistant".to_string(),
            content: crate::api::MessageContent::Blocks(vec![
                crate::api::ContentBlock::Text {
                    text: "Running bash command".to_string(),
                },
                crate::api::ContentBlock::ToolUse {
                    id: "tu_1".to_string(),
                    name: "bash".to_string(),
                    input: serde_json::json!({"command": "ls -la"}),
                },
            ]),
        });
        let tokens = state.estimate_tokens();
        assert!(tokens > 0);
    }

    // Serialization edge cases

    #[test]
    fn test_query_metadata_minimal_serialization() {
        let metadata = QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: false,
            max_tokens: None,
            model: "test-model".to_string(),
            temperature: None,
            top_p: None,
        };
        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: QueryMetadata = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.tools_allowed);
        assert!(deserialized.max_tokens.is_none());
        assert!(deserialized.temperature.is_none());
        assert!(deserialized.top_p.is_none());
    }

    #[test]
    fn test_conversation_stats_serialization() {
        let stats = ConversationStats {
            message_count: 42,
            turn_count: 10,
            total_tokens: 50000,
            total_cost: 1.234,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: ConversationStats = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.message_count, 42);
        assert_eq!(deserialized.turn_count, 10);
        assert_eq!(deserialized.total_tokens, 50000);
        assert!((deserialized.total_cost - 1.234).abs() < 0.001);
    }

    #[test]
    fn test_conversation_stats_zero_values() {
        let stats = ConversationStats {
            message_count: 0,
            turn_count: 0,
            total_tokens: 0,
            total_cost: 0.0,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: ConversationStats = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.message_count, 0);
        assert_eq!(deserialized.total_cost, 0.0);
    }

    // CostTracker model name matching

    #[test]
    fn test_cost_tracker_model_name_variants() {
        let cost = CostTracker::calculate_cost("claude-3-5-sonnet", 1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.001);

        let cost = CostTracker::calculate_cost("claude-3-5-haiku", 1_000_000, 1_000_000);
        assert!((cost - 4.80).abs() < 0.001);

        let cost = CostTracker::calculate_cost("claude-3-opus", 1_000_000, 1_000_000);
        assert!((cost - 90.0).abs() < 0.001);

        let cost = CostTracker::calculate_cost("unknown-model", 1_000_000, 1_000_000);
        assert!(cost > 0.0);
    }

    #[test]
    fn test_cost_tracker_accumulate_multiple_models() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());

        tracker.record_usage("claude-sonnet-4", 100_000, 50_000);
        tracker.record_usage("gpt-4o", 200_000, 100_000);
        tracker.record_usage("llama3", 50_000, 25_000);

        assert_eq!(tracker.total_input_tokens, 350_000);
        assert_eq!(tracker.total_output_tokens, 175_000);
        assert!(tracker.total_cost() > 0.0);
    }

    #[test]
    fn test_cost_tracker_accumulate_across_recordings() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());

        tracker.record_usage("claude-sonnet-4", 1_000_000, 500_000);
        let cost1 = tracker.total_cost();

        tracker.record_usage("claude-sonnet-4", 1_000_000, 500_000);
        let cost2 = tracker.total_cost();

        assert!((cost2 - 2.0 * cost1).abs() < 0.001);
    }

    #[test]
    fn test_query_engine_config_builder_chained() {
        let config = QueryEngineConfig {
            max_turns: 1,
            max_budget_usd: Some(0.01),
            timeout_seconds: 10,
            verbose: false,
            enable_thinking: false,
            max_context_tokens: Some(1000),
            compression_threshold: 0.9,
            keep_recent_messages: 1,
            compression_strategy: CompressionStrategy::default(),
            system_prompt: None,
            auto_commit: false,
            effort_level: None,
            focus_area: None,
            fast_model: None,
            plan_model: None,
            max_parallel_tools: 10,
        };
        assert_eq!(config.max_turns, 1);
        assert_eq!(config.max_budget_usd, Some(0.01));
        assert_eq!(config.timeout_seconds, 10);
        assert!(!config.verbose);
        assert!(!config.enable_thinking);
        assert_eq!(config.max_context_tokens, Some(1000));
        assert!((config.compression_threshold - 0.9).abs() < 0.001);
        assert_eq!(config.keep_recent_messages, 1);
    }

    #[test]
    fn test_fast_model_routing_simple_query() {
        use crate::model_registry::{ModelRouter, TaskType};

        // Verify ModelRouter returns valid model IDs for all task types
        for task in [TaskType::QuickQuery, TaskType::CodeGeneration, TaskType::ArchitectureDesign, TaskType::ComplexWorkflow] {
            let model = ModelRouter::recommend(task);
            assert!(!model.is_empty(), "ModelRouter should return a model for {task:?}");
        }

        // Verify fast_model config field works
        let config = QueryEngineConfig {
            fast_model: Some("claude-3-5-haiku-20241022".to_string()),
            ..QueryEngineConfig::default()
        };
        assert_eq!(config.fast_model.as_deref(), Some("claude-3-5-haiku-20241022"));
    }
}
