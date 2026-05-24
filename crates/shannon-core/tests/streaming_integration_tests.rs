//! Integration tests for SSE streaming parser with real HTTP responses.
//!
//! Tests SseStream against actual mockito HTTP servers to verify
//! chunk boundary handling, event parsing, and multi-format support.

#![allow(clippy::collapsible_match)]

#[cfg(test)]
mod streaming_tests {
    use futures::StreamExt;
    use mockito::{Server, ServerGuard};
    use shannon_core::api::LlmProvider;
    use shannon_core::api::streaming::{LastEventId, SseStream};

    // ── Helpers ──

    fn mock_sse_stream(server: &mut ServerGuard, body: &str) -> mockito::Mock {
        server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create()
    }

    /// Full Anthropic SSE stream: message_start → text → message_delta → stop.
    fn anthropic_full_stream(text: &str) -> String {
        format!(
            "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_test\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{{\"input_tokens\":10,\"output_tokens\":0}}}}}}\n\n\
             event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n\
             event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{text}\"}}}}\n\n\
             event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n\
             event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\"}},\"usage\":{{\"input_tokens\":10,\"output_tokens\":5}}}}\n\n\
             event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n",
        )
    }

    /// Anthropic SSE stream with thinking blocks.
    fn anthropic_thinking_stream(thinking: &str, text: &str) -> String {
        format!(
            "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_think\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{{\"input_tokens\":10,\"output_tokens\":0}}}}}}\n\n\
             event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"thinking\",\"thinking\":\"\"}}}}\n\n\
             event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"thinking_delta\",\"thinking\":\"{thinking}\"}}}}\n\n\
             event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n\
             event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":1,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n\
             event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":1,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{text}\"}}}}\n\n\
             event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":1}}\n\n\
             event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\"}},\"usage\":{{\"input_tokens\":10,\"output_tokens\":15}}}}\n\n\
             event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n",
        )
    }

    /// Anthropic SSE stream with text + tool_use.
    fn anthropic_tool_use_stream(
        text: &str,
        tool_id: &str,
        tool_name: &str,
        tool_input: &str,
    ) -> String {
        format!(
            "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_tool\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{{\"input_tokens\":20,\"output_tokens\":0}}}}}}\n\n\
             event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n\
             event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{text}\"}}}}\n\n\
             event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n\
             event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":1,\"content_block\":{{\"type\":\"tool_use\",\"id\":\"{tool_id}\",\"name\":\"{tool_name}\",\"input\":{{}}}}}}\n\n\
             event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":1,\"delta\":{{\"type\":\"input_json_delta\",\"partial_json\":\"{tool_input}\"}}}}\n\n\
             event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":1}}\n\n\
             event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"tool_use\"}},\"usage\":{{\"input_tokens\":20,\"output_tokens\":15}}}}\n\n\
             event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n",
        )
    }

    /// OpenAI-format SSE stream.
    #[allow(dead_code)] // KEEP: test helper
    fn openai_text_stream(text: &str) -> String {
        format!(
            "data: {{\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1234567890,\"model\":\"gpt-4\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{text}\"}},\"finish_reason\":null}}]}}\n\n\
             data: {{\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1234567890,\"model\":\"gpt-4\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n\
             data: [DONE]\n\n",
        )
    }

    async fn collect_stream_events(
        server_url: &str,
        _body: &str,
        provider: LlmProvider,
    ) -> Vec<shannon_core::api::StreamEvent> {
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{server_url}/v1/messages"))
            .header("content-type", "application/json")
            .body(r#"{"model":"test","messages":[]}"#)
            .send()
            .await
            .unwrap();

        let last_event_id = LastEventId::default();
        let mut stream = SseStream::new(response, provider, last_event_id);
        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => events.push(event),
                Err(_) => break,
            }
        }
        events
    }

    // ── Tests ──

    #[tokio::test]
    async fn test_anthropic_streaming_full_cycle() {
        let mut server = Server::new_async().await;
        let mock_url = server.url();
        let _m = mock_sse_stream(&mut server, &anthropic_full_stream("Hello world!"));

        let events = collect_stream_events(&mock_url, "", LlmProvider::Anthropic).await;

        // Verify event sequence
        use shannon_core::api::StreamEvent;
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::MessageStart { .. })),
            "Should have MessageStart"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::ContentBlockStart { .. })),
            "Should have ContentBlockStart"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::ContentBlockDelta { .. })),
            "Should have ContentBlockDelta"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::ContentBlockStop { .. })),
            "Should have ContentBlockStop"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::MessageDelta { .. })),
            "Should have MessageDelta"
        );
        assert!(
            events.iter().any(|e| matches!(e, StreamEvent::MessageStop)),
            "Should have MessageStop"
        );

        // Verify text content
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                    shannon_core::api::ContentDelta::TextDelta { text } => Some(text.as_str()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(text, "Hello world!");
    }

    #[tokio::test]
    async fn test_anthropic_thinking_blocks() {
        let mut server = Server::new_async().await;
        let mock_url = server.url();
        let _m = mock_sse_stream(
            &mut server,
            &anthropic_thinking_stream("Let me analyze this...", "The answer is 42."),
        );

        let events = collect_stream_events(&mock_url, "", LlmProvider::Anthropic).await;

        use shannon_core::api::StreamEvent;
        // Should have thinking delta events
        let thinking: String = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                    shannon_core::api::ContentDelta::ThinkingDelta { thinking } => {
                        Some(thinking.as_str())
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(thinking, "Let me analyze this...");

        // And text events separately
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                    shannon_core::api::ContentDelta::TextDelta { text } => Some(text.as_str()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(text, "The answer is 42.");
    }

    #[tokio::test]
    async fn test_anthropic_tool_use_blocks() {
        let mut server = Server::new_async().await;
        let mock_url = server.url();
        let _m = mock_sse_stream(
            &mut server,
            &anthropic_tool_use_stream(
                "Let me check.",
                "toolu_1",
                "bash",
                r#"{\"command\":\"ls\"}"#,
            ),
        );

        let events = collect_stream_events(&mock_url, "", LlmProvider::Anthropic).await;

        use shannon_core::api::{ContentBlock, StreamEvent};

        // Should have tool_use content block
        let has_tool_use = events.iter().any(|e| {
            matches!(
                e,
                StreamEvent::ContentBlockStart {
                    content_block: ContentBlock::ToolUse { .. },
                    ..
                }
            )
        });
        assert!(has_tool_use, "Should have tool_use content block");

        // Should have input_json_delta
        let has_input_delta = events.iter().any(|e| {
            matches!(
                e,
                StreamEvent::ContentBlockDelta {
                    delta: shannon_core::api::ContentDelta::InputJsonDelta { .. },
                    ..
                }
            )
        });
        assert!(has_input_delta, "Should have input_json_delta");

        // And text should also be present
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                    shannon_core::api::ContentDelta::TextDelta { text } => Some(text.as_str()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(text, "Let me check.");
    }

    #[tokio::test]
    async fn test_data_only_format_without_event_prefix() {
        // Verify parser handles "data:" lines without "event:" prefix.
        let body = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_test\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"test\",\"stop_reason\":null,\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"data-only\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":5,\"output_tokens\":3}}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );

        let mut server = Server::new_async().await;
        let mock_url = server.url();
        let _m = mock_sse_stream(&mut server, body);

        let events = collect_stream_events(&mock_url, "", LlmProvider::Anthropic).await;

        let text: String = events
            .iter()
            .filter_map(|e| match e {
                shannon_core::api::StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                    shannon_core::api::ContentDelta::TextDelta { text } => Some(text.as_str()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(text, "data-only", "Should parse data:-only SSE format");
    }

    #[tokio::test]
    async fn test_partial_stream_preserves_content() {
        // Stream ends mid-way (no message_stop). Verify partial content is kept.
        let body = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_partial\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"test\",\"stop_reason\":null,\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial content here\"}}\n\n",
        );
        // No content_block_stop, no message_stop — stream just ends.

        let mut server = Server::new_async().await;
        let mock_url = server.url();
        let _m = mock_sse_stream(&mut server, body);

        let events = collect_stream_events(&mock_url, "", LlmProvider::Anthropic).await;

        // Should still have the partial text
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                shannon_core::api::StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                    shannon_core::api::ContentDelta::TextDelta { text } => Some(text.as_str()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(
            text, "partial content here",
            "Partial content should be preserved"
        );
    }

    #[tokio::test]
    async fn test_multiple_text_deltas_assembled() {
        // Multiple text deltas should be emitted individually (not merged).
        let body = "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_multi\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"test\",\"stop_reason\":null,\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello \"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"beautiful \"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"world!\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":5,\"output_tokens\":10}}\n\n\
             data: {\"type\":\"message_stop\"}\n\n".to_string();

        let mut server = Server::new_async().await;
        let mock_url = server.url();
        let _m = mock_sse_stream(&mut server, &body);

        let events = collect_stream_events(&mock_url, "", LlmProvider::Anthropic).await;

        // Count individual text deltas
        let text_deltas: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                shannon_core::api::StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                    shannon_core::api::ContentDelta::TextDelta { text } => Some(text.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect();

        assert_eq!(text_deltas.len(), 3, "Should have 3 separate text deltas");
        assert_eq!(text_deltas.join(""), "Hello beautiful world!");
    }
}
