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
    use serde_json::{Value, json};
    use shannon_core::query_engine::{QueryContext, QueryEngine, QueryEngineConfig, QueryMetadata};
    use shannon_core::tools::ToolRegistry;
    use shannon_engine::api::LlmClientConfig;
    use shannon_engine::api::LlmProvider;
    use shannon_engine::permissions::PermissionManager;
    use shannon_engine::state::StateManager;
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
            retry_config: shannon_engine::api::RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 0, // No retries in tests
            budget_tokens: None,
            reasoning_effort: None,
        };
        let client = shannon_engine::api::LlmClient::new(config);
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new();
        let state = StateManager::new();

        QueryEngine::new(
            client,
            tools,
            permissions,
            state,
            QueryEngineConfig::default(),
        )
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
    fn setup_error_mock(
        server: &mut ServerGuard,
        status: usize,
        error_json: Value,
    ) -> mockito::Mock {
        server
            .mock("POST", "/v1/messages")
            .with_status(status)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&error_json).unwrap())
            .create()
    }

    /// Set up mock server to return a successful Anthropic streaming response.
    fn setup_success_stream_mock(server: &mut ServerGuard) -> mockito::Mock {
        let body = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_test\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-4-20250514\",\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n\
             event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
             event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello!\"}}\n\n\
             event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n\n\
             event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

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
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
            text: "First response".to_string(),
        }]);

        let session_id = engine.session_id();
        let history_len = engine.conversation_history().len();

        // Simulate an API error
        setup_error_mock(
            &mut server,
            500,
            json!({
                "type": "error",
                "error": {
                    "type": "api_error",
                    "message": "Internal server error"
                }
            }),
        );

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
        let has_error = result.iter().any(|e| e.is_err())
            || result
                .iter()
                .any(|e| matches!(e, Ok(shannon_core::query_engine::QueryEvent::Failed { .. })));
        assert!(has_error, "Expected an error from failed API call");

        // KEY ASSERTION: Engine should still be alive and functional
        assert_eq!(
            engine.session_id(),
            session_id,
            "Session ID should be preserved"
        );
        assert_eq!(
            engine.conversation_history().len(),
            history_len,
            "Conversation history should be preserved after error"
        );

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

        setup_error_mock(
            &mut server,
            401,
            json!({
                "type": "error",
                "error": {
                    "type": "authentication_error",
                    "message": "invalid x-api-key"
                }
            }),
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = make_query_context();
        let _ = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // Engine must survive auth errors — user needs it to retry with a valid key
        assert_eq!(engine.session_id(), session_id);
        assert_eq!(
            engine.conversation_history().len(),
            1,
            "History preserved after auth error"
        );
    }

    #[test]
    fn test_engine_survives_rate_limit_error() {
        let mut server = Server::new();
        let mock_url = server.url();

        let engine = create_engine(&mock_url);
        let session_id = engine.session_id();

        setup_error_mock(
            &mut server,
            429,
            json!({
                "type": "error",
                "error": {
                    "type": "rate_limit_error",
                    "message": "rate limited"
                }
            }),
        );

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
        setup_error_mock(
            &mut server,
            500,
            json!({
                "type": "error",
                "error": {"type": "api_error", "message": "internal error"}
            }),
        );

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
        let has_content = result
            .iter()
            .any(|e| matches!(e, Ok(shannon_core::query_engine::QueryEvent::Text { .. })));
        assert!(
            has_content,
            "Second query should succeed after error recovery"
        );

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
            setup_error_mock(
                &mut server,
                500,
                json!({
                    "type": "error",
                    "error": {"type": "api_error", "message": format!("error #{i}")}
                }),
            );

            let ctx = make_query_context();
            let _ = rt.block_on(async {
                use futures::StreamExt;
                let stream = engine.process_query(ctx, None).await;
                Box::pin(stream).collect::<Vec<_>>().await
            });

            // Engine must survive every iteration
            assert_eq!(
                engine.session_id(),
                session_id,
                "Session lost after error #{i}"
            );
        }

        // Final state: still the same engine
        assert_eq!(engine.session_id(), session_id);
        engine.add_user_message("Still alive".to_string());
        assert_eq!(
            engine.conversation_history().len(),
            1,
            "One explicit message should be in history"
        );
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
        setup_error_mock(
            &mut server,
            500,
            json!({
                "type": "error",
                "error": {"type": "api_error", "message": "connection refused"}
            }),
        );

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
        assert!(
            !session_id.is_nil(),
            "Engine should have a valid session ID after error"
        );

        // Can still add messages and access history
        engine.add_user_message("retry after model switch".to_string());
        assert_eq!(engine.conversation_history().len(), 1);
    }

    // ── Conversation history regression tests ──
    //
    // These test the fix for the bug where the AI didn't remember previous
    // turn content because:
    // 1. The query engine's `self.conversation` was never updated from the
    //    local `conversation` clone used during streaming.
    // 2. The UI code added display-formatted text (with tool output,
    //    progress, etc.) instead of clean AI response text.

    /// Set up a mock that returns a specific text response via SSE streaming.
    fn setup_stream_mock_with_text(server: &mut ServerGuard, response_text: &str) -> mockito::Mock {
        let escaped = response_text
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        let body = format!(
            "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_test\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-4-20250514\",\"stop_reason\":null,\"usage\":{{\"input_tokens\":10,\"output_tokens\":0}}}}}}\n\n\
             event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n\
             event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{escaped}\"}}}}\n\n\
             event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n\
             event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\"}},\"usage\":{{\"input_tokens\":10,\"output_tokens\":5}}}}\n\n\
             event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
        );

        server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create()
    }

    #[test]
    fn test_conversation_update_event_sent_on_success() {
        // After a successful query, a ConversationUpdate event must be sent
        // before the Completed event. This carries the updated conversation
        // messages so the UI can persist the proper history.
        let mut server = Server::new();
        let engine = create_engine(&server.url());

        setup_success_stream_mock(&mut server);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = make_query_context();
        let events = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // Find the ConversationUpdate and Completed events
        let has_conversation_update = events.iter().any(|e| {
            matches!(
                e,
                Ok(shannon_core::query_engine::QueryEvent::ConversationUpdate { .. })
            )
        });
        assert!(
            has_conversation_update,
            "ConversationUpdate event must be sent"
        );

        // ConversationUpdate must come before Completed
        let cu_idx = events
            .iter()
            .position(|e| {
                matches!(
                    e,
                    Ok(shannon_core::query_engine::QueryEvent::ConversationUpdate { .. })
                )
            })
            .expect("ConversationUpdate event index");
        let completed_idx = events
            .iter()
            .position(|e| {
                matches!(
                    e,
                    Ok(shannon_core::query_engine::QueryEvent::Completed { .. })
                )
            })
            .expect("Completed event index");
        assert!(
            cu_idx < completed_idx,
            "ConversationUpdate must come before Completed"
        );
    }

    #[test]
    fn test_conversation_update_contains_clean_messages() {
        // The ConversationUpdate messages must contain the clean user message
        // and assistant response text — NOT display-formatted text with tool
        // outputs, progress markers, etc.
        let mut server = Server::new();
        let engine = create_engine(&server.url());

        let story_text = "Alice and Bob went to the park. They met Charlie there.";
        setup_stream_mock_with_text(&mut server, story_text);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = QueryContext {
            user_message: "Write a short story".to_string(),
            ..make_query_context()
        };
        let events = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // Extract ConversationUpdate messages
        let messages = events
            .iter()
            .find_map(|e| {
                if let Ok(shannon_core::query_engine::QueryEvent::ConversationUpdate {
                    messages,
                    ..
                }) = e
                {
                    Some(messages.clone())
                } else {
                    None
                }
            })
            .expect("ConversationUpdate event must be present");

        // Should have exactly 2 messages: user + assistant
        assert_eq!(messages.len(), 2, "Expected user + assistant messages");

        // First message should be the user's clean input
        assert_eq!(messages[0].role, "user");
        match &messages[0].content {
            shannon_engine::api::MessageContent::Text(t) => {
                assert_eq!(
                    t, "Write a short story",
                    "User message should be the clean input, not display-formatted"
                );
            }
            _ => panic!("Expected Text content for user message"),
        }

        // Second message should be the assistant's clean response
        assert_eq!(messages[1].role, "assistant");
        match &messages[1].content {
            shannon_engine::api::MessageContent::Text(t) => {
                assert_eq!(
                    t, story_text,
                    "Assistant message should be clean AI text, not display-formatted"
                );
            }
            _ => panic!("Expected Text content for assistant message"),
        }

        // The assistant message must NOT contain display artifacts
        let assistant_text = match &messages[1].content {
            shannon_engine::api::MessageContent::Text(t) => t.clone(),
            shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    shannon_engine::api::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(""),
        };
        assert!(
            !assistant_text.contains("[Turn"),
            "Should not contain turn markers"
        );
        assert!(
            !assistant_text.contains("Using:"),
            "Should not contain tool display"
        );
    }

    #[test]
    fn test_conversation_update_enables_multi_turn_memory() {
        // Simulate the full bug scenario:
        // Turn 1: User asks AI to write a story → AI writes about Alice and Bob
        // Turn 2: User asks "how many characters?" → conversation history must
        // include the story from turn 1 so the AI can answer correctly.
        //
        // This test verifies that the API request for turn 2 includes the
        // conversation context from turn 1.
        let mut server = Server::new();
        let mut engine = create_engine(&server.url());

        // ── Turn 1 ──
        let story = "Alice met Bob at the library. They discussed quantum physics.";
        setup_stream_mock_with_text(&mut server, story);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx1 = QueryContext {
            user_message: "Write a story about two people".to_string(),
            ..make_query_context()
        };
        let events1 = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx1, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // Extract and restore conversation messages (simulates what UI does)
        let turn1_messages = events1
            .iter()
            .find_map(|e| {
                if let Ok(shannon_core::query_engine::QueryEvent::ConversationUpdate {
                    messages,
                    ..
                }) = e
                {
                    Some(messages.clone())
                } else {
                    None
                }
            })
            .expect("Turn 1: ConversationUpdate must be present");

        // Restore the proper conversation history
        engine.restore_messages(turn1_messages);
        assert_eq!(
            engine.conversation_history().len(),
            2,
            "After turn 1, engine should have user + assistant messages"
        );

        // ── Turn 2 ──
        // Set up a mock that captures the request body to verify conversation
        // history is included in the API call.
        let turn2_response = "The story has 2 characters: Alice and Bob.";
        setup_stream_mock_with_text(&mut server, turn2_response);

        let ctx2 = QueryContext {
            user_message: "How many characters are in the story?".to_string(),
            ..make_query_context()
        };
        let events2 = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx2, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // Turn 2 should also send ConversationUpdate
        let has_cu = events2.iter().any(|e| {
            matches!(
                e,
                Ok(shannon_core::query_engine::QueryEvent::ConversationUpdate { .. })
            )
        });
        assert!(has_cu, "Turn 2: ConversationUpdate event must also be sent");

        // Turn 2 conversation should have 4 messages: user1, assistant1, user2, assistant2
        let turn2_messages = events2
            .iter()
            .find_map(|e| {
                if let Ok(shannon_core::query_engine::QueryEvent::ConversationUpdate {
                    messages,
                    ..
                }) = e
                {
                    Some(messages.clone())
                } else {
                    None
                }
            })
            .expect("Turn 2: ConversationUpdate must be present");

        assert_eq!(
            turn2_messages.len(),
            4,
            "Turn 2 should have 4 messages (2 turns × user+assistant), got {}",
            turn2_messages.len()
        );

        // Verify the conversation ordering
        assert_eq!(turn2_messages[0].role, "user");
        assert_eq!(turn2_messages[1].role, "assistant");
        assert_eq!(turn2_messages[2].role, "user");
        assert_eq!(turn2_messages[3].role, "assistant");

        // CRITICAL: Verify turn 1 content is preserved in the history
        let user1_text = match &turn2_messages[0].content {
            shannon_engine::api::MessageContent::Text(t) => t.clone(),
            _ => panic!("Expected text"),
        };
        assert!(
            user1_text.contains("Write a story"),
            "Turn 1 user message must be preserved: got '{user1_text}'"
        );

        let assistant1_text = match &turn2_messages[1].content {
            shannon_engine::api::MessageContent::Text(t) => t.clone(),
            _ => panic!("Expected text"),
        };
        assert!(
            assistant1_text.contains("Alice") && assistant1_text.contains("Bob"),
            "Turn 1 assistant response must contain the story content: got '{assistant1_text}'"
        );

        let user2_text = match &turn2_messages[2].content {
            shannon_engine::api::MessageContent::Text(t) => t.clone(),
            _ => panic!("Expected text"),
        };
        assert!(
            user2_text.contains("How many characters"),
            "Turn 2 user message must be preserved: got '{user2_text}'"
        );
    }

    #[test]
    fn test_restore_messages_produces_correct_api_context() {
        // Verify that after restore_messages, the engine's conversation_history
        // returns exactly the messages that were restored (no duplicates, no loss).
        let server = Server::new();
        let mut engine = create_engine(&server.url());

        // Simulate conversation from ConversationUpdate
        let messages = vec![
            shannon_engine::api::Message {
                role: "user".to_string(),
                content: shannon_engine::api::MessageContent::Text("Write a poem".to_string()),
            },
            shannon_engine::api::Message {
                role: "assistant".to_string(),
                content: shannon_engine::api::MessageContent::Text("Roses are red...".to_string()),
            },
            shannon_engine::api::Message {
                role: "user".to_string(),
                content: shannon_engine::api::MessageContent::Text("Make it longer".to_string()),
            },
            shannon_engine::api::Message {
                role: "assistant".to_string(),
                content: shannon_engine::api::MessageContent::Text(
                    "Roses are red, violets are blue...".to_string(),
                ),
            },
        ];

        engine.restore_messages(messages);

        let history = engine.conversation_history();
        assert_eq!(history.len(), 4);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[1].role, "assistant");
        assert_eq!(history[2].role, "user");
        assert_eq!(history[3].role, "assistant");

        // Verify content preserved exactly
        match &history[1].content {
            shannon_engine::api::MessageContent::Text(t) => {
                assert_eq!(t, "Roses are red...");
            }
            _ => panic!("Expected text content"),
        }
    }

    // ── Ollama malformed output recovery tests ──
    //
    // Regression tests for the bug where Ollama "can't find closing '}' symbol"
    // errors were displayed as AI response text instead of a clean error.

    /// Create a QueryEngine configured for Ollama pointing at a mock server.
    fn create_ollama_engine(mock_url: &str) -> QueryEngine {
        let config = LlmClientConfig {
            api_key: String::new(),
            base_url: mock_url.to_string(),
            model: "tiny-model".to_string(),
            max_tokens: 4096,
            timeout_seconds: 10,
            api_version: String::new(),
            provider: LlmProvider::Ollama,
            extra_headers: HashMap::new(),
            retry_config: shannon_engine::api::RetryConfig::new(0, 0, 0),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 0,
            budget_tokens: None,
            reasoning_effort: None,
        };
        let client = shannon_engine::api::LlmClient::new(config);
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new();
        let state = StateManager::new();

        QueryEngine::new(
            client,
            tools,
            permissions,
            state,
            QueryEngineConfig::default(),
        )
    }

    /// Ollama streaming returns HTTP 500 with malformed output error.
    /// This triggers the engine's error path which checks is_ollama_malformed_output().
    fn setup_ollama_stream_error_mock(server: &mut ServerGuard) -> mockito::Mock {
        let body = json!({"error":"Value looks like object, but can't find closing '}' symbol"});

        server
            .mock("POST", "/api/chat")
            .match_body(mockito::Matcher::Regex(r#""stream":true"#.to_string()))
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .expect(1)
            .create()
    }

    /// Ollama non-streaming response with malformed output error.
    /// The adapter catches this and returns warning as ContentBlock::Text.
    fn setup_ollama_non_stream_error_mock(server: &mut ServerGuard) -> mockito::Mock {
        let body = json!({
            "model": "tiny-model",
            "message": {"role": "assistant", "content": ""},
            "error": "Value looks like object, but can't find closing '}' symbol",
            "done": true,
            "prompt_eval_count": 5,
            "eval_count": 0
        });

        server
            .mock("POST", "/api/chat")
            .match_body(mockito::Matcher::Regex(r#""stream":false"#.to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .expect(1)
            .create()
    }

    /// Ollama non-streaming success response.
    fn setup_ollama_non_stream_success_mock(server: &mut ServerGuard, text: &str) -> mockito::Mock {
        let body = json!({
            "model": "tiny-model",
            "message": {"role": "assistant", "content": text},
            "done": true,
            "prompt_eval_count": 5,
            "eval_count": 10
        });

        server
            .mock("POST", "/api/chat")
            .match_body(mockito::Matcher::Regex(r#""stream":false"#.to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .expect(1)
            .create()
    }

    /// Ollama streaming success response with text content.
    fn setup_ollama_stream_success_mock(server: &mut ServerGuard, text: &str) -> mockito::Mock {
        let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
        let body = format!(
            r#"{{"model":"tiny-model","created_at":"2024-01-01T00:00:00Z","message":{{"role":"assistant","content":"{escaped}"}},"done":false}}
{{"model":"tiny-model","created_at":"2024-01-01T00:00:00Z","done":true,"prompt_eval_count":5,"eval_count":10}}"#
        );

        server
            .mock("POST", "/api/chat")
            .match_body(mockito::Matcher::Regex(r#""stream":true"#.to_string()))
            .with_status(200)
            .with_header("content-type", "application/x-ndjson")
            .with_body(body)
            .expect(1)
            .create()
    }

    #[test]
    fn test_ollama_malformed_output_all_retries_fail() {
        // When ALL retries fail (stream → non-stream → minimal), the engine
        // must emit QueryEvent::Failed with a clear message — NOT the raw
        // Ollama error as AI response text.
        let mut server = Server::new();
        let mut engine = create_ollama_engine(&server.url());
        let session_id = engine.session_id();

        // Mock: streaming returns malformed output error
        setup_ollama_stream_error_mock(&mut server);
        // Mock: non-streaming retry also returns error (adapter returns warning as content)
        // Need 2 matches: full-history retry + minimal retry
        setup_ollama_non_stream_error_mock(&mut server);
        setup_ollama_non_stream_error_mock(&mut server);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = make_query_context();
        let events = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // Must emit a Failed event, not Text with warning
        let has_failed = events.iter().any(|e| {
            matches!(e, Ok(shannon_core::query_engine::QueryEvent::Failed { error, .. })
                if error.contains("cannot produce valid output"))
        });
        assert!(
            has_failed,
            "Expected QueryEvent::Failed with model incompatibility message"
        );

        // Must NOT emit the raw Ollama error as AI text
        let has_warning_text = events.iter().any(|e| {
            matches!(e, Ok(shannon_core::query_engine::QueryEvent::Text { content, .. })
                if content.contains("⚠️ Ollama model output error"))
        });
        assert!(
            !has_warning_text,
            "Raw Ollama error should NOT be emitted as AI response text"
        );

        // Engine must survive for the next query
        assert_eq!(engine.session_id(), session_id);
        engine.add_user_message("retry after ollama error".to_string());
        assert_eq!(engine.conversation_history().len(), 1);
    }

    #[test]
    fn test_ollama_malformed_output_retry_succeeds_on_minimal() {
        // When streaming fails but minimal retry (last user message only) succeeds,
        // the engine should return the successful content.
        let mut server = Server::new();
        let engine = create_ollama_engine(&server.url());

        // Mock: streaming returns malformed output error
        setup_ollama_stream_error_mock(&mut server);
        // Mock: full-history non-streaming retry returns error
        setup_ollama_non_stream_error_mock(&mut server);
        // Mock: minimal retry SUCCEEDS
        setup_ollama_non_stream_success_mock(&mut server, "Hello from tiny model!");

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = make_query_context();
        let events = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // Should get the successful content
        let has_content = events.iter().any(|e| {
            matches!(e, Ok(shannon_core::query_engine::QueryEvent::Text { content, .. })
                if content.contains("Hello from tiny model!"))
        });
        assert!(
            has_content,
            "Expected content from successful minimal retry"
        );

        // Should complete successfully (not fail)
        let has_failed = events
            .iter()
            .any(|e| matches!(e, Ok(shannon_core::query_engine::QueryEvent::Failed { .. })));
        assert!(!has_failed, "Should not fail when minimal retry succeeds");
    }

    #[test]
    fn test_ollama_malformed_output_retry_succeeds_without_tools() {
        // When streaming fails but non-streaming retry (without tools) succeeds,
        // the engine should return that content without attempting minimal retry.
        let mut server = Server::new();
        let engine = create_ollama_engine(&server.url());

        // Mock: streaming returns malformed output error
        setup_ollama_stream_error_mock(&mut server);
        // Mock: non-streaming retry SUCCEEDS (no warning in content)
        setup_ollama_non_stream_success_mock(&mut server, "Success without tools!");

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = make_query_context();
        let events = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        let has_content = events.iter().any(|e| {
            matches!(e, Ok(shannon_core::query_engine::QueryEvent::Text { content, .. })
                if content.contains("Success without tools!"))
        });
        assert!(
            has_content,
            "Expected content from successful retry without tools"
        );

        let has_failed = events
            .iter()
            .any(|e| matches!(e, Ok(shannon_core::query_engine::QueryEvent::Failed { .. })));
        assert!(!has_failed);
    }

    #[test]
    fn test_ollama_engine_reusable_after_malformed_output() {
        // After an Ollama malformed output error, the engine must be reusable
        // for subsequent queries that succeed.
        let mut server = Server::new();
        let engine = create_ollama_engine(&server.url());
        let session_id = engine.session_id();

        // Query 1: all retries fail
        setup_ollama_stream_error_mock(&mut server);
        setup_ollama_non_stream_error_mock(&mut server);
        setup_ollama_non_stream_error_mock(&mut server);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx1 = make_query_context();
        let _ = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx1, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        // Query 2: succeeds (simulating user switched to a working model)
        setup_ollama_stream_success_mock(&mut server, "Now working!");

        let ctx2 = make_query_context();
        let events2 = rt.block_on(async {
            use futures::StreamExt;
            let stream = engine.process_query(ctx2, None).await;
            Box::pin(stream).collect::<Vec<_>>().await
        });

        let has_content = events2.iter().any(|e| {
            matches!(e, Ok(shannon_core::query_engine::QueryEvent::Text { content, .. })
                if content.contains("Now working!"))
        });
        assert!(
            has_content,
            "Second query should succeed after Ollama error recovery"
        );
        assert_eq!(engine.session_id(), session_id, "Session preserved");
    }
}
