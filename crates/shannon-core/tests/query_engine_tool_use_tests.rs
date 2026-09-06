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
    use shannon_core::query_engine::{
        QueryContext, QueryEngine, QueryEngineConfig, QueryEvent, QueryMetadata,
    };
    use shannon_core::tools::{Tool, ToolOutput, ToolRegistry, ToolResult};
    use shannon_engine::api::{LlmClientConfig, LlmProvider};
    use shannon_engine::permissions::PermissionManager;
    use shannon_engine::state::StateManager;
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

        #[allow(dead_code)] // KEEP: test helper
        fn with_responses(name: &str, responses: Vec<ToolOutput>) -> Self {
            Self {
                name: name.to_string(),
                responses: std::sync::Mutex::new(responses),
                call_count: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        #[allow(dead_code)] // KEEP: test helper
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
            retry_config: shannon_engine::api::RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 0,
            budget_tokens: None,
            reasoning_effort: None,
        };
        let client = shannon_engine::api::LlmClient::new(config);
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
             data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{intro_text}\"}}}}\n\n\
             data: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n\
             data: {{\"type\":\"content_block_start\",\"index\":1,\"content_block\":{{\"type\":\"tool_use\",\"id\":\"{tool1_id}\",\"name\":\"{tool1_name}\",\"input\":{{}}}}}}\n\n\
             data: {{\"type\":\"content_block_delta\",\"index\":1,\"delta\":{{\"type\":\"input_json_delta\",\"partial_json\":\"{tool1_input}\"}}}}\n\n\
             data: {{\"type\":\"content_block_stop\",\"index\":1}}\n\n\
             data: {{\"type\":\"content_block_start\",\"index\":2,\"content_block\":{{\"type\":\"tool_use\",\"id\":\"{tool2_id}\",\"name\":\"{tool2_name}\",\"input\":{{}}}}}}\n\n\
             data: {{\"type\":\"content_block_delta\",\"index\":2,\"delta\":{{\"type\":\"input_json_delta\",\"partial_json\":\"{tool2_input}\"}}}}\n\n\
             data: {{\"type\":\"content_block_stop\",\"index\":2}}\n\n\
             data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"tool_use\"}},\"usage\":{{\"input_tokens\":25,\"output_tokens\":20}}}}\n\n\
             data: {{\"type\":\"message_stop\"}}\n\n",
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
                shannon_engine::api::MessageContent::Text(t) => t.clone(),
                shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| match b {
                        shannon_engine::api::ContentBlock::Text { text } => Some(text.as_str()),
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

// ── OpenAI-compat trailing usage frame regression (dogfood 2026-08-23) ──

mod openai_trailing_usage_tests {
    //! MiniMax/DeepSeek-style SSE: the real usage arrives in a separate
    //! `choices: []` frame AFTER the `finish_reason` chunk. On tool turns
    //! the engine breaks out of the stream to execute tools, so that frame
    //! must be drained before abandoning the stream — otherwise the whole
    //! request is metered as zero tokens (dogfood 2026-08-23 l1 lost 62 of
    //! 63 requests this way).

    use async_trait::async_trait;
    use futures::StreamExt;
    use mockito::Server;
    use serde_json::{Value, json};
    use shannon_core::query_engine::{
        QueryContext, QueryEngine, QueryEngineConfig, QueryEvent, QueryMetadata,
    };
    use shannon_core::tools::{Tool, ToolOutput, ToolRegistry, ToolResult};
    use shannon_engine::api::{LlmClientConfig, LlmProvider};
    use shannon_engine::permissions::PermissionManager;
    use shannon_engine::state::StateManager;
    use std::collections::HashMap;
    use uuid::Uuid;

    struct EchoTool;
    #[async_trait]
    impl Tool for EchoTool {
        async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
            Ok(ToolOutput::success("echo-ok".to_string()))
        }
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echo test tool"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }
    }

    /// Tool-call turn: tool_calls chunk, then a `finish_reason: "tool_calls"`
    /// chunk WITHOUT usage, then the trailing usage-only frame (empty
    /// `choices`), then [DONE]. This is the recorded MiniMax M-series shape.
    fn openai_sse_tool_then_usage(prompt: u64, completion: u64) -> String {
        let chunks = [
            r#"{"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"echo","arguments":"{}"}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
        ];
        let usage = format!(
            r#"{{"choices":[],"usage":{{"prompt_tokens":{},"completion_tokens":{},"total_tokens":{},"prompt_tokens_details":{{"cached_tokens":0}}}}}}"#,
            prompt,
            completion,
            prompt + completion
        );
        let mut body = String::new();
        for c in chunks.iter().chain(std::iter::once(&usage.as_str())) {
            body.push_str("data: ");
            body.push_str(c);
            body.push_str("\n\n");
        }
        body.push_str("data: [DONE]\n\n");
        body
    }

    /// Final text turn: content delta, `finish_reason: "stop"` chunk without
    /// usage, then the trailing usage-only frame, then [DONE].
    fn openai_sse_text_then_usage(prompt: u64, completion: u64) -> String {
        let chunks = [
            r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"all done"},"finish_reason":null}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        ];
        let usage = format!(
            r#"{{"choices":[],"usage":{{"prompt_tokens":{},"completion_tokens":{},"total_tokens":{},"prompt_tokens_details":{{"cached_tokens":0}}}}}}"#,
            prompt,
            completion,
            prompt + completion
        );
        let mut body = String::new();
        for c in chunks.iter().chain(std::iter::once(&usage.as_str())) {
            body.push_str("data: ");
            body.push_str(c);
            body.push_str("\n\n");
        }
        body.push_str("data: [DONE]\n\n");
        body
    }

    async fn setup_openai_engine(bodies: Vec<String>) -> (QueryEngine, mockito::ServerGuard) {
        let mut server = Server::new_async().await;
        for body in bodies {
            server
                .mock("POST", "/v1/chat/completions")
                .with_status(200)
                .with_header("content-type", "text/event-stream")
                .with_body(body)
                .expect(1)
                .create();
        }
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: server.url(),
            model: "test-model".to_string(),
            max_tokens: 4096,
            timeout_seconds: 10,
            api_version: "2023-06-01".to_string(),
            provider: LlmProvider::OpenAI,
            extra_headers: HashMap::new(),
            retry_config: shannon_engine::api::RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 0,
            budget_tokens: None,
            reasoning_effort: None,
        };
        let registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        let engine = QueryEngine::new(
            shannon_engine::api::LlmClient::new(config),
            registry,
            PermissionManager::new(),
            StateManager::new(),
            QueryEngineConfig::default(),
        );
        (engine, server)
    }

    async fn run_query(engine: &QueryEngine) -> Vec<QueryEvent> {
        let ctx = QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: "use the echo tool".to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: Some(4096),
                model: "test-model".to_string(),
                temperature: None,
                top_p: None,
            },
        };
        let stream = engine.process_query(ctx, None).await;
        let mut events = Vec::new();
        let mut s = Box::pin(stream);
        while let Some(Ok(event)) = s.next().await {
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn test_openai_trailing_usage_frame_counted_on_tool_turn() {
        let (engine, _server) = setup_openai_engine(vec![
            openai_sse_tool_then_usage(100, 10),
            openai_sse_text_then_usage(200, 20),
        ])
        .await;
        let events = run_query(&engine).await;

        // Tool turn must count the trailing frame, not zero.
        let turn_tokens: Vec<u64> = events
            .iter()
            .filter_map(|e| match e {
                QueryEvent::TurnCompleted { tokens_used, .. } => Some(*tokens_used),
                _ => None,
            })
            .collect();
        assert_eq!(
            turn_tokens.first(),
            Some(&110),
            "tool turn tokens must come from the trailing usage frame"
        );

        // Cost must accumulate BOTH requests: 100+200 in, 10+20 out.
        let cost = events.iter().rev().find_map(|e| match e {
            QueryEvent::Cost {
                input_tokens,
                output_tokens,
                ..
            } => Some((*input_tokens, *output_tokens)),
            _ => None,
        });
        assert_eq!(
            cost,
            Some((300, 30)),
            "Cost totals must include the tool request's trailing usage frame"
        );
    }
}

mod openai_truncation_continuation_tests {
    //! Regression (dogfood 2026-08-23 l1-bulk-migrate, outcome_fail): the
    //! model spent the entire 4096-token output budget on `<think>`
    //! reasoning; the stream closed with `finish_reason: "length"`, zero
    //! visible answer and no tool calls. The engine treated that truncated
    //! no-op message as a final turn — the headless run exited 0 without
    //! writing any of the expected artifacts. A truncation stop with no
    //! tool calls must re-prompt the model to continue instead of
    //! completing the query.

    use futures::StreamExt;
    use mockito::{Mock, Server};
    use shannon_core::query_engine::{
        QueryContext, QueryEngine, QueryEngineConfig, QueryEvent, QueryMetadata,
    };
    use shannon_core::tools::ToolRegistry;
    use shannon_engine::api::{LlmClientConfig, LlmProvider, MessageContent};
    use shannon_engine::permissions::PermissionManager;
    use shannon_engine::state::StateManager;
    use std::collections::HashMap;
    use uuid::Uuid;

    /// Truncated turn: `<think>` reasoning in content, then a
    /// `finish_reason: "length"` chunk WITHOUT usage, then the trailing
    /// usage-only frame (empty `choices`), then [DONE]. This is the
    /// recorded MiniMax-M3 wire shape: 4095 reasoning tokens, no visible
    /// answer, no tool calls.
    fn openai_sse_think_truncated_then_usage(prompt: u64, completion: u64) -> String {
        let chunks = [
            r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"<think>Now I have a complete picture. Let me plan the restructure: move customer.rs into src/people/, pricing.rs into src/sales/, update the module declarations"},"finish_reason":null}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"length"}]}"#,
        ];
        let usage = format!(
            r#"{{"choices":[],"usage":{{"prompt_tokens":{},"completion_tokens":{},"total_tokens":{},"prompt_tokens_details":{{"cached_tokens":0}}}}}}"#,
            prompt,
            completion,
            prompt + completion
        );
        let mut body = String::new();
        for c in chunks.iter().chain(std::iter::once(&usage.as_str())) {
            body.push_str("data: ");
            body.push_str(c);
            body.push_str("\n\n");
        }
        body.push_str("data: [DONE]\n\n");
        body
    }

    /// Completed text turn after the continuation re-prompt.
    fn openai_sse_text_then_usage(prompt: u64, completion: u64) -> String {
        let chunks = [
            r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"all done"},"finish_reason":null}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        ];
        let usage = format!(
            r#"{{"choices":[],"usage":{{"prompt_tokens":{},"completion_tokens":{},"total_tokens":{},"prompt_tokens_details":{{"cached_tokens":0}}}}}}"#,
            prompt,
            completion,
            prompt + completion
        );
        let mut body = String::new();
        for c in chunks.iter().chain(std::iter::once(&usage.as_str())) {
            body.push_str("data: ");
            body.push_str(c);
            body.push_str("\n\n");
        }
        body.push_str("data: [DONE]\n\n");
        body
    }

    async fn setup_openai_engine(
        bodies: Vec<String>,
    ) -> (QueryEngine, Vec<Mock>, mockito::ServerGuard) {
        let mut server = Server::new_async().await;
        let mut mocks = Vec::new();
        for body in bodies {
            mocks.push(
                server
                    .mock("POST", "/v1/chat/completions")
                    .with_status(200)
                    .with_header("content-type", "text/event-stream")
                    .with_body(body)
                    .expect(1)
                    .create(),
            );
        }
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: server.url(),
            model: "test-model".to_string(),
            max_tokens: 4096,
            timeout_seconds: 10,
            api_version: "2023-06-01".to_string(),
            provider: LlmProvider::OpenAI,
            extra_headers: HashMap::new(),
            retry_config: shannon_engine::api::RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 0,
            budget_tokens: None,
            reasoning_effort: None,
        };
        let engine = QueryEngine::new(
            shannon_engine::api::LlmClient::new(config),
            ToolRegistry::new(),
            PermissionManager::new(),
            StateManager::new(),
            QueryEngineConfig::default(),
        );
        (engine, mocks, server)
    }

    async fn run_query(engine: &QueryEngine) -> Vec<QueryEvent> {
        let ctx = QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: "restructure the crate".to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: Some(4096),
                model: "test-model".to_string(),
                temperature: None,
                top_p: None,
            },
        };
        let stream = engine.process_query(ctx, None).await;
        let mut events = Vec::new();
        let mut s = Box::pin(stream);
        while let Some(Ok(event)) = s.next().await {
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn truncated_think_only_uses_truncation_not_nudge() {
        // Precedence (A1 vs truncation continuation): a `length`-cut
        // response whose `<think>` never closes looks like a think-only
        // response, but the truncation machinery owns it. The think-only
        // nudge must stay silent so the truncation bound remains a bound.
        let (engine, mocks, _server) = setup_openai_engine(vec![
            openai_sse_think_truncated_then_usage(300, 4096),
            openai_sse_text_then_usage(400, 120),
        ])
        .await;
        let events = run_query(&engine).await;

        for m in &mocks {
            m.assert();
        }
        assert!(
            events.iter().any(|e| matches!(
                e,
                QueryEvent::Warning { message, .. } if message.contains("output token limit")
            )),
            "the truncation continuation must own the re-prompt"
        );
        assert!(
            !events.iter().any(|e| matches!(
                e,
                QueryEvent::Warning { message, .. } if message.contains("re-prompting")
            )),
            "the think-only nudge must not fire on an output-limit truncation"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, QueryEvent::Completed { .. }))
        );
    }

    #[tokio::test]
    async fn length_truncation_without_tool_calls_continues_instead_of_completing() {
        let (engine, mocks, _server) = setup_openai_engine(vec![
            openai_sse_think_truncated_then_usage(300, 4096),
            openai_sse_text_then_usage(400, 120),
        ])
        .await;
        let events = run_query(&engine).await;

        // The truncated response must NOT have ended the query: a second
        // request (the continuation re-prompt) went out and completed it.
        for m in &mocks {
            m.assert();
        }
        assert!(
            events
                .iter()
                .any(|e| matches!(e, QueryEvent::Completed { .. })),
            "query must complete via the continuation, not the truncation"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, QueryEvent::Failed { .. })),
            "continuation is not a failure"
        );

        // The truncation is surfaced to the user, not silent.
        assert!(
            events.iter().any(|e| matches!(
                e,
                QueryEvent::Warning { message, .. } if message.contains("output token limit")
            )),
            "expected a truncation warning event"
        );

        // Metering covers BOTH requests (trailing usage frames drained).
        let cost = events.iter().rev().find_map(|e| match e {
            QueryEvent::Cost {
                input_tokens,
                output_tokens,
                ..
            } => Some((*input_tokens, *output_tokens)),
            _ => None,
        });
        assert_eq!(cost, Some((700, 4216)));

        // Conversation keeps the truncated reasoning for context, the
        // continuation re-prompt, and the final answer.
        let history: Vec<_> = events
            .iter()
            .rev()
            .find_map(|e| match e {
                QueryEvent::ConversationUpdate { messages, .. } => Some(messages.clone()),
                _ => None,
            })
            .expect("at least one ConversationUpdate event");
        assert!(
            history
                .iter()
                .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("<think>"))),
            "truncated reasoning must stay in context"
        );
        assert!(
            history
                .iter()
                .any(|m| m.role == "user" && matches!(&m.content, MessageContent::Text(t) if t.contains("cut off by the output token limit"))),
            "continuation re-prompt must be in the conversation"
        );
        assert!(
            history
                .iter()
                .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("all done"))),
            "final answer must be in the conversation"
        );
    }

    #[tokio::test]
    async fn perpetual_truncation_is_bounded_and_still_completes() {
        // A model that overruns its output budget on EVERY response must
        // not loop forever: after MAX_TRUNCATION_CONTINUATIONS re-prompts
        // the query ends as a normal (truncated) completion.
        let bodies: Vec<String> = (0..6)
            .map(|i| openai_sse_think_truncated_then_usage(100 * (i + 1), 4096))
            .collect();
        let (engine, mocks, _server) = setup_openai_engine(bodies).await;
        let events = run_query(&engine).await;

        for m in &mocks {
            m.assert();
        }
        assert!(
            events
                .iter()
                .any(|e| matches!(e, QueryEvent::Completed { .. })),
            "query must terminate with Completed once continuations are exhausted"
        );
    }
}

// ── Zhipu/GLM coding-plan tool-use broadcast regression (2026-08-27) ──

mod zhipu_tool_use_broadcast_tests {
    //! Zhipu's OpenAI-compatible endpoint (`/api/coding/paas/v4`, glm-4.7 /
    //! glm-5.3-flash) splits `delta.tool_calls` across frames and repeats the
    //! SAME call id on EVERY frame while streaming argument fragments, then
    //! packs `finish_reason: "tool_calls"` AND the real usage into ONE
    //! terminal chunk. That terminal chunk used to take the usage early-return
    //! in `normalize_openai_event`, skipping the finish_reason branch that
    //! synthesizes `ContentBlockStop` — so the engine executed the tools via
    //! its silent post-MessageDelta flush and `QueryEvent::ToolUseRequest`
    //! was never broadcast (dogfood 2026-08-27: glm-5.3-flash 20/20 tasks,
    //! L0 tool/call rows = 0, NDJSON tool_call lines = 0, while the wire
    //! layer showed the model emitting dozens of tool calls per task).
    //!
    //! MiniMax never hit this because its real usage arrives in a separate
    //! `choices: []` frame AFTER the finish chunk, so its tool turns always
    //! reached the finish_reason branch. These tests pin the zhipu shape.

    use async_trait::async_trait;
    use futures::StreamExt;
    use mockito::Server;
    use serde_json::{Value, json};
    use shannon_core::query_engine::{
        QueryContext, QueryEngine, QueryEngineConfig, QueryEvent, QueryMetadata,
    };
    use shannon_core::tools::{Tool, ToolOutput, ToolRegistry, ToolResult};
    use shannon_engine::api::{LlmClientConfig, LlmProvider};
    use shannon_engine::permissions::PermissionManager;
    use shannon_engine::state::StateManager;
    use std::collections::HashMap;
    use uuid::Uuid;

    struct EchoTool;
    #[async_trait]
    impl Tool for EchoTool {
        async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
            Ok(ToolOutput::success("echo-ok".to_string()))
        }
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echo test tool"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }
    }

    /// Zhipu-shaped tool turn: tool-call frames (same id repeated), then the
    /// terminal chunk carrying `finish_reason: "tool_calls"` + real usage
    /// TOGETHER, then [DONE].
    fn zhipu_sse_tool_turn() -> String {
        let chunks = [
            r#"{"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_z1","type":"function","function":{"name":"echo","arguments":""}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_z1","function":{"arguments":"{\"task\":"}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_z1","function":{"arguments":"\"write tests\"}"}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120}}"#,
        ];
        let mut body = String::new();
        for c in &chunks {
            body.push_str("data: ");
            body.push_str(c);
            body.push_str("\n\n");
        }
        body.push_str("data: [DONE]\n\n");
        body
    }

    /// Zhipu-shaped text turn: content delta, then terminal chunk with
    /// `finish_reason: "stop"` + usage together, then [DONE].
    fn zhipu_sse_text_turn() -> String {
        let chunks = [
            r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"all done"},"finish_reason":null}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":200,"completion_tokens":30,"total_tokens":230}}"#,
        ];
        let mut body = String::new();
        for c in &chunks {
            body.push_str("data: ");
            body.push_str(c);
            body.push_str("\n\n");
        }
        body.push_str("data: [DONE]\n\n");
        body
    }

    async fn setup_zhipu_engine(bodies: Vec<String>) -> (QueryEngine, mockito::ServerGuard) {
        let mut server = Server::new_async().await;
        for body in bodies {
            server
                .mock("POST", "/v1/chat/completions")
                .with_status(200)
                .with_header("content-type", "text/event-stream")
                .with_body(body)
                .expect(1)
                .create();
        }
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: server.url(),
            model: "glm-5.3-flash".to_string(),
            max_tokens: 4096,
            timeout_seconds: 10,
            api_version: "2023-06-01".to_string(),
            provider: LlmProvider::OpenAI,
            extra_headers: HashMap::new(),
            retry_config: shannon_engine::api::RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 0,
            budget_tokens: None,
            reasoning_effort: None,
        };
        let registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        let engine = QueryEngine::new(
            shannon_engine::api::LlmClient::new(config),
            registry,
            PermissionManager::new(),
            StateManager::new(),
            QueryEngineConfig::default(),
        );
        (engine, server)
    }

    async fn run_query(engine: &QueryEngine) -> Vec<QueryEvent> {
        let ctx = QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: "use the echo tool".to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: Some(4096),
                model: "glm-5.3-flash".to_string(),
                temperature: None,
                top_p: None,
            },
        };
        let stream = engine.process_query(ctx, None).await;
        let mut events = Vec::new();
        let mut s = Box::pin(stream);
        while let Some(Ok(event)) = s.next().await {
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn zhipu_tool_turn_broadcasts_tool_use_request_exactly_once_with_full_input() {
        let (engine, _server) =
            setup_zhipu_engine(vec![zhipu_sse_tool_turn(), zhipu_sse_text_turn()]).await;
        let events = run_query(&engine).await;

        // Exactly ONE ToolUseRequest, with the FULLY PARSED input assembled
        // from the split argument fragments.
        let requests: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                QueryEvent::ToolUseRequest {
                    tool_use_id,
                    tool_name,
                    tool_input,
                    ..
                } => Some((tool_use_id.clone(), tool_name.clone(), tool_input.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            requests.len(),
            1,
            "ToolUseRequest must broadcast exactly once, got {requests:?}"
        );
        let (tool_use_id, tool_name, tool_input) = &requests[0];
        assert_eq!(tool_use_id, "call_z1");
        assert_eq!(tool_name, "echo");
        assert_eq!(
            tool_input,
            &json!({"task": "write tests"}),
            "tool_input must be the complete JSON assembled from multi-frame fragments"
        );

        // The tool must have executed exactly once (no double-run), after
        // the request was broadcast.
        let results: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                QueryEvent::ToolUseResult {
                    tool_name,
                    is_error,
                    ..
                } => Some((tool_name.clone(), *is_error)),
                _ => None,
            })
            .collect();
        assert_eq!(
            results.len(),
            1,
            "tool must execute exactly once, got {results:?}"
        );
        assert!(!results[0].1, "tool result must not be an error");

        let req_idx = events
            .iter()
            .position(|e| matches!(e, QueryEvent::ToolUseRequest { .. }))
            .expect("ToolUseRequest present");
        let res_idx = events
            .iter()
            .position(|e| matches!(e, QueryEvent::ToolUseResult { .. }))
            .expect("ToolUseResult present");
        assert!(
            req_idx < res_idx,
            "ToolUseRequest must precede ToolUseResult"
        );

        assert!(
            events
                .iter()
                .any(|e| matches!(e, QueryEvent::Completed { .. })),
            "query must complete after the tool turn"
        );
    }
}
