//! Tests for query engine recovery after errors.
//!
//! Regression tests for the bug where the query engine was dropped on error paths,
//! causing "Query engine not available. Please restart the session."
//!
//! The engine takes `&self` in `process_query`, so it is never consumed by the
//! query itself. These tests verify the engine remains usable after various
//! failure scenarios.

#[cfg(test)]
mod engine_recovery_tests {
    use mockito::{Server, ServerGuard};
    use serde_json::{json, Value};
    use shannon_core::api::LlmClientConfig;
    use shannon_core::api::LlmProvider;
    use shannon_core::permissions::PermissionManager;
    use shannon_core::query_engine::{QueryEngine, QueryEngineConfig, QueryContext, QueryMetadata};
    use shannon_core::state::StateManager;
    use shannon_core::tools::ToolRegistry;
    use std::collections::HashMap;
    use uuid::Uuid;

    /// Create a QueryEngine pointing at a mock server.
    fn create_engine(mock_url: &str) -> QueryEngine {
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: mock_url.to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            timeout_seconds: 10,
            api_version: "2023-06-01".to_string(),
            provider: LlmProvider::Anthropic,
            extra_headers: HashMap::new(),
            retry_config: shannon_core::api::RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 0, // No retries in tests
            budget_tokens: None,
            reasoning_effort: None,
        };
        let client = shannon_core::api::LlmClient::new(config);
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new();
        let state = StateManager::new();

        QueryEngine::new(client, tools, permissions, state, QueryEngineConfig::default())
    }

    fn make_query_context() -> QueryContext {
        QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: "test query".to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: Some(100),
                model: "claude-sonnet-4-20250514".to_string(),
                temperature: None,
                top_p: None,
            },
        }
    }

    /// Set up mock server to return an Anthropic-style error response.
    fn setup_error_mock(server: &mut ServerGuard, status: usize, error_json: Value) -> mockito::Mock {
        server
            .mock("POST", "/v1/messages")
            .with_status(status)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&error_json).unwrap())
            .create()
    }

    /// Set up mock server to return a successful Anthropic streaming response.
    fn setup_success_stream_mock(server: &mut ServerGuard) -> mockito::Mock {
        let body = [
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_test\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-4-20250514\",\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello!\"}}",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}",
        ].join("\n\n");

        server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create()
    }

    // ── Engine state preservation tests ──

    #[test]
    fn test_engine_survives_api_error() {
        // Verify that after a failed API call, the engine is still usable:
        // conversation history is intact, new messages can be added, and
        // subsequent queries can be made.
        let mut server = Server::new();
        let mock_url = server.url();

        let mut engine = create_engine(&mock_url);

        // Add initial conversation state
        engine.add_user_message("First message".to_string());
        engine.add_assistant_message(vec![shannon_core::api::ContentBlock::Text {
            text: "First response".to_string(),
        }]);

        let session_id = engine.session_id();
        let history_len = engine.conversation_history().len();

        // Simulate an API error
        setup_error_mock(&mut server, 500, json!({
            "type": "error",
            "error": {
                "type": "api_error",
                "message": "Internal server error"
            }
        }));

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = make_query_context();
        let result = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx, None).await;
            let mut events = Vec::new();
            let mut s = Box::pin(stream);
            while let Some(event) = s.next().await {
                events.push(event);
            }
            events
        });

        // Should have received an error event or stream error
        let has_error = result.iter().any(|e| e.is_err()) ||
            result.iter().any(|e| matches!(e, Ok(shannon_core::query_engine::QueryEvent::Failed { .. })));
        assert!(has_error, "Expected an error from failed API call");

        // KEY ASSERTION: Engine should still be alive and functional
        assert_eq!(engine.session_id(), session_id, "Session ID should be preserved");
        assert_eq!(engine.conversation_history().len(), history_len,
            "Conversation history should be preserved after error");

        // Engine should accept new messages
        engine.add_user_message("After error".to_string());
        assert_eq!(engine.conversation_history().len(), history_len + 1);

        // Model should still be settable
        engine.set_model("claude-haiku-4-5-20251001".to_string());
    }

    #[test]
    fn test_engine_survives_auth_error() {
        // After an authentication error (401/403), the engine must survive
        // so the user can fix their API key and retry.
        let mut server = Server::new();
        let mock_url = server.url();

        let mut engine = create_engine(&mock_url);
        engine.add_user_message("Test".to_string());

        let session_id = engine.session_id();

        setup_error_mock(&mut server, 401, json!({
            "type": "error",
            "error": {
                "type": "authentication_error",
                "message": "invalid x-api-key"
            }
        }));

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = make_query_context();
        let _ = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // Engine must survive auth errors — user needs it to retry with a valid key
        assert_eq!(engine.session_id(), session_id);
        assert_eq!(engine.conversation_history().len(), 1, "History preserved after auth error");
    }

    #[test]
    fn test_engine_survives_rate_limit_error() {
        let mut server = Server::new();
        let mock_url = server.url();

        let engine = create_engine(&mock_url);
        let session_id = engine.session_id();

        setup_error_mock(&mut server, 429, json!({
            "type": "error",
            "error": {
                "type": "rate_limit_error",
                "message": "rate limited"
            }
        }));

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = make_query_context();
        let _ = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // Engine must survive rate limit errors — user will retry later
        assert_eq!(engine.session_id(), session_id);
        assert_eq!(engine.conversation_history().len(), 0);
    }

    #[test]
    fn test_engine_reusable_after_error() {
        // After a failed query, the engine should be able to successfully
        // process a subsequent query (simulating a retry).
        let mut server = Server::new();
        let mock_url = server.url();

        let engine = create_engine(&mock_url);
        let session_id = engine.session_id();

        // First query fails
        setup_error_mock(&mut server, 500, json!({
            "type": "error",
            "error": {"type": "api_error", "message": "internal error"}
        }));

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx1 = make_query_context();
        let _ = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx1, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // Second query succeeds
        setup_success_stream_mock(&mut server);

        let ctx2 = make_query_context();
        let result = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx2, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // Should have received some events (not just errors)
        let has_content = result.iter().any(|e| {
            matches!(e, Ok(shannon_core::query_engine::QueryEvent::Text { .. }))
        });
        assert!(has_content, "Second query should succeed after error recovery");

        // Session preserved across error + success
        assert_eq!(engine.session_id(), session_id);
    }

    #[test]
    fn test_engine_survives_multiple_consecutive_errors() {
        // Engine should survive being used for multiple failed queries in a row.
        // This simulates a flaky network or misconfigured setup.
        let mut server = Server::new();
        let mock_url = server.url();

        let mut engine = create_engine(&mock_url);
        let session_id = engine.session_id();

        let rt = tokio::runtime::Runtime::new().unwrap();

        for i in 0..3 {
            setup_error_mock(&mut server, 500, json!({
                "type": "error",
                "error": {"type": "api_error", "message": format!("error #{i}")}
            }));

            let ctx = make_query_context();
            let _ = rt.block_on(async {
                use futures::StreamExt;
                let stream = engine.process_query(ctx, None).await;
                Box::pin(stream).collect::<Vec<_>>().await
            });

            // Engine must survive every iteration
            assert_eq!(engine.session_id(), session_id,
                "Session lost after error #{i}");
        }

        // Final state: still the same engine
        assert_eq!(engine.session_id(), session_id);
        engine.add_user_message("Still alive".to_string());
        assert_eq!(engine.conversation_history().len(), 1,
            "One explicit message should be in history");
    }

    #[test]
    fn test_engine_survives_malformed_response() {
        // Simulates the Ollama malformed output scenario from the bug report.
        // The engine must not be corrupted by garbage responses.
        let mut server = Server::new();
        let mock_url = server.url();

        let mut engine = create_engine(&mock_url);
        let session_id = engine.session_id();

        // Return malformed JSON that can't be parsed
        server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body("not valid SSE data at all {{{")
            .create();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = make_query_context();
        let _ = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // Engine must survive malformed responses
        assert_eq!(engine.session_id(), session_id);
        engine.add_user_message("After garbage".to_string());
        assert_eq!(engine.conversation_history().len(), 1);
    }

    #[test]
    fn test_engine_model_change_survives_error() {
        // Simulates the exact bug scenario: switch model, get an error,
        // then verify the engine is still available with the new model.
        let mut server = Server::new();
        let mock_url = server.url();

        let mut engine = create_engine(&mock_url);

        // Switch model (as user would do when changing providers)
        engine.set_model("llama3".to_string());
        engine.set_model_for_provider("llama3".to_string(), LlmProvider::Ollama);

        // Now simulate a failure
        setup_error_mock(&mut server, 500, json!({
            "type": "error",
            "error": {"type": "api_error", "message": "connection refused"}
        }));

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = make_query_context();
        let _ = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // KEY ASSERTION: Engine must still be available after model switch + error
        // This is the exact scenario that caused "Query engine not available"
        let session_id = engine.session_id();
        assert!(!session_id.is_nil(), "Engine should have a valid session ID after error");

        // Can still add messages and access history
        engine.add_user_message("retry after model switch".to_string());
        assert_eq!(engine.conversation_history().len(), 1);
    }
}
