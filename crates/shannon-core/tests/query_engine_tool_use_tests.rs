//! Integration tests for the query engine tool-use pipeline.
//!
//! Tests the full cycle: user message → streaming → tool call →
//! tool result → continue streaming → completed.
//!
//! Uses mockito for HTTP mocking to avoid real API calls.

#[cfg(test)]
mod tool_use_tests {
    use async_trait::async_trait;
    use futures::StreamExt;
    use mockito::{Server, ServerGuard};
    use serde_json::{Value, json};
    use shannon_core::api::{LlmClientConfig, LlmProvider};
    use shannon_core::permissions::PermissionManager;
    use shannon_core::query_engine::{
        QueryContext, QueryEngine, QueryEngineConfig, QueryEvent, QueryMetadata,
    };
    use shannon_core::state::StateManager;
    use shannon_core::tools::{Tool, ToolOutput, ToolRegistry, ToolResult};
    use std::collections::HashMap;
    use uuid::Uuid;

    /// Set ANTHROPIC_API_KEY for tests (some code paths check env var).
    struct KeyGuard(Option<std::ffi::OsString>);
    impl KeyGuard {
        fn set() -> Self {
            let old = std::env::var_os("ANTHROPIC_API_KEY");
            unsafe {
                std::env::set_var("ANTHROPIC_API_KEY", "test-key");
            }
            Self(old)
        }
    }
    impl Drop for KeyGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => unsafe { std::env::set_var("ANTHROPIC_API_KEY", v) },
                None => unsafe { std::env::remove_var("ANTHROPIC_API_KEY") },
            }
        }
    }

    // ── Helpers ──

    /// A tool that records calls and returns a pre-configured response.
    struct RecordableTool {
        name: String,
        responses: std::sync::Mutex<Vec<ToolOutput>>,
        call_count: std::sync::atomic::AtomicUsize,
    }

    impl RecordableTool {
        fn new(name: &str, response: ToolOutput) -> Self {
            Self {
                name: name.to_string(),
                responses: std::sync::Mutex::new(vec![response]),
                call_count: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        #[allow(dead_code)]
        fn with_responses(name: &str, responses: Vec<ToolOutput>) -> Self {
            Self {
                name: name.to_string(),
                responses: std::sync::Mutex::new(responses),
                call_count: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        #[allow(dead_code)]
        fn call_count(&self) -> usize {
            self.call_count.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Tool for RecordableTool {
        async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
            self.call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Ok(ToolOutput::success("default response".to_string()))
            } else {
                Ok(responses[0].clone())
            }
        }
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            "recordable test tool"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }
    }

    /// A tool that always returns an error.
    struct FailingTool {
        name: String,
    }

    #[async_trait]
    impl Tool for FailingTool {
        async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
            Ok(ToolOutput::error("tool execution failed".to_string()))
        }
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            "failing test tool"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }
    }

    fn create_engine(mock_url: &str, registry: ToolRegistry) -> QueryEngine {
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
            max_stream_reconnects: 0,
            budget_tokens: None,
            reasoning_effort: None,
        };
        let client = shannon_core::api::LlmClient::new(config);
        QueryEngine::new(
            client,
            registry,
            PermissionManager::new(),
            StateManager::new(),
            QueryEngineConfig::default(),
        )
    }

    fn make_context(msg: &str) -> QueryContext {
        QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: msg.to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: Some(4096),
                model: "claude-sonnet-4-20250514".to_string(),
                temperature: None,
                top_p: None,
            },
        }
    }

    /// SSE response: text + tool_use (stop_reason: tool_use).
    fn sse_tool_use_response(
        text: &str,
        tool_id: &str,
        tool_name: &str,
        tool_input: &str,
    ) -> String {
        format!(
            "data: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_tool\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{{\"input_tokens\":20,\"output_tokens\":0}}}}}}\n\n\
             data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n\
             data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{text}\"}}}}\n\n\
             data: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n\
             data: {{\"type\":\"content_block_start\",\"index\":1,\"content_block\":{{\"type\":\"tool_use\",\"id\":\"{tool_id}\",\"name\":\"{tool_name}\",\"input\":{{}}}}}}\n\n\
             data: {{\"type\":\"content_block_delta\",\"index\":1,\"delta\":{{\"type\":\"input_json_delta\",\"partial_json\":\"{tool_input}\"}}}}\n\n\
             data: {{\"type\":\"content_block_stop\",\"index\":1}}\n\n\
             data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"tool_use\"}},\"usage\":{{\"input_tokens\":20,\"output_tokens\":15}}}}\n\n\
             data: {{\"type\":\"message_stop\"}}\n\n",
            text = text,
            tool_id = tool_id,
            tool_name = tool_name,
            tool_input = tool_input,
        )
    }

    /// SSE response: text-only (stop_reason: end_turn).
    fn sse_text_response(text: &str) -> String {
        format!(
            "data: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_final\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{{\"input_tokens\":30,\"output_tokens\":0}}}}}}\n\n\
             data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n\
             data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{text}\"}}}}\n\n\
             data: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n\
             data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\"}},\"usage\":{{\"input_tokens\":30,\"output_tokens\":10}}}}\n\n\
             data: {{\"type\":\"message_stop\"}}\n\n",
            text = text,
        )
    }

    /// SSE response: text intro + two tool_use blocks (stop_reason: tool_use).
    fn sse_multi_tool_response(
        intro_text: &str,
        tool1_id: &str,
        tool1_name: &str,
        tool1_input: &str,
        tool2_id: &str,
        tool2_name: &str,
        tool2_input: &str,
    ) -> String {
        format!(
            "data: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_multi\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{{\"input_tokens\":25,\"output_tokens\":0}}}}}}\n\n\
             data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n\
             data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{intro}\"}}}}\n\n\
             data: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n\
             data: {{\"type\":\"content_block_start\",\"index\":1,\"content_block\":{{\"type\":\"tool_use\",\"id\":\"{t1id}\",\"name\":\"{t1n}\",\"input\":{{}}}}}}\n\n\
             data: {{\"type\":\"content_block_delta\",\"index\":1,\"delta\":{{\"type\":\"input_json_delta\",\"partial_json\":\"{t1i}\"}}}}\n\n\
             data: {{\"type\":\"content_block_stop\",\"index\":1}}\n\n\
             data: {{\"type\":\"content_block_start\",\"index\":2,\"content_block\":{{\"type\":\"tool_use\",\"id\":\"{t2id}\",\"name\":\"{t2n}\",\"input\":{{}}}}}}\n\n\
             data: {{\"type\":\"content_block_delta\",\"index\":2,\"delta\":{{\"type\":\"input_json_delta\",\"partial_json\":\"{t2i}\"}}}}\n\n\
             data: {{\"type\":\"content_block_stop\",\"index\":2}}\n\n\
             data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"tool_use\"}},\"usage\":{{\"input_tokens\":25,\"output_tokens\":20}}}}\n\n\
             data: {{\"type\":\"message_stop\"}}\n\n",
            intro = intro_text,
            t1id = tool1_id,
            t1n = tool1_name,
            t1i = tool1_input,
            t2id = tool2_id,
            t2n = tool2_name,
            t2i = tool2_input,
        )
    }

    fn setup_mock(server: &mut ServerGuard, body: &str) -> mockito::Mock {
        server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create()
    }

    async fn collect_events(engine: &QueryEngine, ctx: QueryContext) -> Vec<QueryEvent> {
        let stream = engine.process_query(ctx, None).await;
        let mut events = Vec::new();
        let mut s = Box::pin(stream);
        while let Some(Ok(event)) = s.next().await {
            events.push(event);
        }
        events
    }

    // ── Tests ──

    #[tokio::test]
    async fn test_tool_use_then_text_response() {
        let _guard = KeyGuard::set();
        // Full pipeline: text + tool_use → tool execution → final text answer
        let mut server = Server::new_async().await;
        let mock_url = server.url();

        let registry = ToolRegistry::new();
        let bash_tool = RecordableTool::new(
            "bash",
            ToolOutput::success("total 0\ndrwxr-xr-x 2 user user 64 Jan 1 00:00 .".to_string()),
        );
        registry.register(Box::new(bash_tool)).unwrap();

        let engine = create_engine(&mock_url, registry);

        // First response: text + tool_use
        let _m1 = setup_mock(
            &mut server,
            &sse_tool_use_response(
                "Let me check that.",
                "toolu_bash_1",
                "bash",
                r#"{\"command\":\"ls -la\"}"#,
            ),
        );
        // Second response: text answer using tool result
        let _m2 = setup_mock(&mut server, &sse_text_response("The directory is empty."));

        let ctx = make_context("List files in current directory");
        let events = collect_events(&engine, ctx).await;

        // Verify full pipeline
        let has_tool_request = events.iter().any(|e| {
            matches!(
                e, QueryEvent::ToolUseRequest { tool_name, .. } if tool_name == "bash"
            )
        });
        let has_tool_result = events.iter().any(|e| matches!(
            e, QueryEvent::ToolUseResult { tool_name, is_error, .. } if tool_name == "bash" && !is_error
        ));
        let has_completed = events
            .iter()
            .any(|e| matches!(e, QueryEvent::Completed { .. }));

        let final_text: String = events
            .iter()
            .filter_map(|e| match e {
                QueryEvent::Text { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();

        assert!(has_tool_request, "Pipeline should request bash tool");
        assert!(has_tool_result, "Pipeline should produce bash tool result");
        assert!(has_completed, "Pipeline should complete");
        assert!(
            final_text.contains("The directory is empty."),
            "Final text should contain tool-derived response. Got: {final_text}"
        );

        // Verify ConversationUpdate preserves the full flow
        let updates: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                QueryEvent::ConversationUpdate { messages, .. } => Some(messages.clone()),
                _ => None,
            })
            .collect();
        assert!(!updates.is_empty(), "Should have ConversationUpdate events");
    }

    #[tokio::test]
    async fn test_multiple_tool_uses_in_single_turn() {
        let _guard = KeyGuard::set();
        // SSE returns 2 tool_use blocks → both executed → final text
        let mut server = Server::new_async().await;
        let mock_url = server.url();

        let registry = ToolRegistry::new();
        let read_tool = RecordableTool::new(
            "read_file",
            ToolOutput::success("file contents here".to_string()),
        );
        let search_tool =
            RecordableTool::new("search", ToolOutput::success("found 3 matches".to_string()));
        registry.register(Box::new(read_tool)).unwrap();
        registry.register(Box::new(search_tool)).unwrap();

        let engine = create_engine(&mock_url, registry);

        // First response: text intro + two tool_use blocks
        let _m1 = setup_mock(
            &mut server,
            &sse_multi_tool_response(
                "Checking now.",
                "toolu_1",
                "read_file",
                r#"{\"path\":\"/tmp/test.txt\"}"#,
                "toolu_2",
                "search",
                r#"{\"pattern\":\"TODO\"}"#,
            ),
        );
        // Second response: combined answer
        let _m2 = setup_mock(&mut server, &sse_text_response("Found TODO in 3 places."));

        let ctx = make_context("Check the file for TODOs");
        let events = collect_events(&engine, ctx).await;

        let tool_requests: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                QueryEvent::ToolUseRequest { tool_name, .. } => Some(tool_name.clone()),
                _ => None,
            })
            .collect();

        assert!(
            tool_requests.contains(&"read_file".to_string()),
            "Should request read_file"
        );
        assert!(
            tool_requests.contains(&"search".to_string()),
            "Should request search"
        );

        let final_text: String = events
            .iter()
            .filter_map(|e| match e {
                QueryEvent::Text { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            final_text.contains("Found TODO"),
            "Final answer should combine tool results. Got: {final_text}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, QueryEvent::Completed { .. }))
        );
    }

    #[tokio::test]
    async fn test_tool_use_error_handling() {
        let _guard = KeyGuard::set();
        // Tool execution fails → error result → engine still completes
        let mut server = Server::new_async().await;
        let mock_url = server.url();

        let registry = ToolRegistry::new();
        let fail_tool = FailingTool {
            name: "bash".to_string(),
        };
        registry.register(Box::new(fail_tool)).unwrap();

        let engine = create_engine(&mock_url, registry);

        // First response: tool_use
        let _m1 = setup_mock(
            &mut server,
            &sse_tool_use_response(
                "Let me try.",
                "toolu_1",
                "bash",
                r#"{\"command\":\"rm -rf /\"}"#,
            ),
        );
        // Second response: acknowledge error
        let _m2 = setup_mock(
            &mut server,
            &sse_text_response("The command was not allowed."),
        );

        let ctx = make_context("Delete everything");
        let events = collect_events(&engine, ctx).await;

        let has_error_result = events.iter().any(|e| matches!(
            e, QueryEvent::ToolUseResult { tool_name, is_error, .. } if tool_name == "bash" && *is_error
        ));
        assert!(has_error_result, "Should have error tool result");

        // Engine must still complete (not hang or crash)
        assert!(
            events
                .iter()
                .any(|e| matches!(e, QueryEvent::Completed { .. })),
            "Engine must complete even after tool error"
        );

        // ConversationUpdate must still be emitted
        assert!(
            events
                .iter()
                .any(|e| matches!(e, QueryEvent::ConversationUpdate { .. })),
            "ConversationUpdate must be emitted even after tool error"
        );
    }

    #[tokio::test]
    async fn test_tool_use_event_ordering() {
        let _guard = KeyGuard::set();
        // Verify event ordering: Text → ToolUseRequest → ToolUseResult → Text → Completed
        let mut server = Server::new_async().await;
        let mock_url = server.url();

        let registry = ToolRegistry::new();
        registry
            .register(Box::new(RecordableTool::new(
                "bash",
                ToolOutput::success("ok".to_string()),
            )))
            .unwrap();

        let engine = create_engine(&mock_url, registry);

        let _m1 = setup_mock(
            &mut server,
            &sse_tool_use_response("Working", "toolu_1", "bash", r#"{\"command\":\"echo hi\"}"#),
        );
        let _m2 = setup_mock(&mut server, &sse_text_response("Done."));

        let ctx = make_context("test");
        let events = collect_events(&engine, ctx).await;

        let text_before_tool = events
            .iter()
            .position(|e| matches!(e, QueryEvent::Text { .. }));
        let tool_req_idx = events
            .iter()
            .position(|e| matches!(e, QueryEvent::ToolUseRequest { .. }));
        let tool_res_idx = events
            .iter()
            .position(|e| matches!(e, QueryEvent::ToolUseResult { .. }));
        let completed_idx = events
            .iter()
            .position(|e| matches!(e, QueryEvent::Completed { .. }));

        assert!(text_before_tool.is_some(), "Text event must exist");
        assert!(tool_req_idx.is_some(), "ToolUseRequest must exist");
        assert!(tool_res_idx.is_some(), "ToolUseResult must exist");
        assert!(completed_idx.is_some(), "Completed must exist");

        assert!(
            text_before_tool < tool_req_idx,
            "Text must precede ToolUseRequest"
        );
        assert!(
            tool_req_idx < tool_res_idx,
            "ToolUseRequest must precede ToolUseResult"
        );
        assert!(
            tool_res_idx < completed_idx,
            "ToolUseResult must precede Completed"
        );
    }

    #[tokio::test]
    async fn test_tool_use_preserves_conversation_for_next_turn() {
        let _guard = KeyGuard::set();
        // After a tool-use turn, the conversation should be restorable
        // and usable for a subsequent text-only turn.
        let mut server = Server::new_async().await;
        let mock_url = server.url();

        let registry = ToolRegistry::new();
        registry
            .register(Box::new(RecordableTool::new(
                "bash",
                ToolOutput::success("hello world".to_string()),
            )))
            .unwrap();

        let mut engine = create_engine(&mock_url, registry);

        // Turn 1: tool use
        let _m1 = setup_mock(
            &mut server,
            &sse_tool_use_response(
                "Running",
                "toolu_1",
                "bash",
                r#"{\"command\":\"echo hello\"}"#,
            ),
        );
        let _m2 = setup_mock(&mut server, &sse_text_response("Output: hello world"));
        let ctx1 = make_context("echo hello");
        let events1 = collect_events(&engine, ctx1).await;

        // Restore messages
        let update1 = events1
            .iter()
            .find_map(|e| match e {
                QueryEvent::ConversationUpdate { messages, .. } => Some(messages.clone()),
                _ => None,
            })
            .expect("Turn 1 must emit ConversationUpdate");
        engine.restore_messages(update1);

        // Turn 2: text-only follow-up
        let _m3 = setup_mock(
            &mut server,
            &sse_text_response("Previous output was hello world."),
        );
        let ctx2 = make_context("What was the output?");
        let events2 = collect_events(&engine, ctx2).await;

        let update2 = events2
            .iter()
            .find_map(|e| match e {
                QueryEvent::ConversationUpdate { messages, .. } => Some(messages.clone()),
                _ => None,
            })
            .expect("Turn 2 must emit ConversationUpdate");

        // After 2 turns, conversation should have accumulated correctly
        assert!(
            update2.len() >= 4,
            "After 2 turns: at least 4 messages, got {}",
            update2.len()
        );

        // The tool-use turn content should be preserved
        let all_text: String = update2
            .iter()
            .map(|m| match &m.content {
                shannon_core::api::MessageContent::Text(t) => t.clone(),
                shannon_core::api::MessageContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| match b {
                        shannon_core::api::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect(),
            })
            .collect();
        assert!(
            all_text.contains("hello world"),
            "Tool result from turn 1 must survive into turn 2"
        );
    }
}
