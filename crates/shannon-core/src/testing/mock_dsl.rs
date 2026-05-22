//! Mock Response DSL for constructing provider-specific SSE responses in tests.
//!
//! Provides a unified builder pattern for creating mock LLM responses that work
//! across Anthropic, OpenAI, and Ollama providers, replacing the ad-hoc builder
//! functions previously scattered across test files.

#[cfg(test)]
use mockito::{Mock, ServerGuard};
use serde_json::{json, Value};

/// A provider-agnostic mock response that can be rendered to any provider's SSE format.
#[derive(Debug, Clone)]
pub struct MockResponse {
    pub content_blocks: Vec<MockContentBlock>,
    pub usage: MockUsage,
    pub stop_reason: String,
}

/// A single content block in a mock response.
#[derive(Debug, Clone)]
pub enum MockContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    Thinking { text: String },
}

/// Token usage for a mock response.
#[derive(Debug, Clone)]
pub struct MockUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_creation_input_tokens: u32,
    pub cache_read_input_tokens: u32,
}

impl Default for MockUsage {
    fn default() -> Self {
        Self {
            input_tokens: 10,
            output_tokens: 8,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        }
    }
}

impl Default for MockResponse {
    fn default() -> Self {
        Self {
            content_blocks: vec![],
            usage: MockUsage::default(),
            stop_reason: "end_turn".to_string(),
        }
    }
}

// ── Builder functions ──────────────────────────────────────────────────

/// Create a simple text response.
pub fn text_response(text: &str) -> MockResponse {
    MockResponse {
        content_blocks: vec![MockContentBlock::Text { text: text.to_string() }],
        ..Default::default()
    }
}

/// Create a response with a tool call.
pub fn tool_call_response(id: &str, name: &str, input: Value) -> MockResponse {
    MockResponse {
        content_blocks: vec![MockContentBlock::ToolUse {
            id: id.to_string(),
            name: name.to_string(),
            input,
        }],
        stop_reason: "tool_use".to_string(),
        ..Default::default()
    }
}

/// Create a response with thinking content.
pub fn thinking_response(text: &str) -> MockResponse {
    MockResponse {
        content_blocks: vec![MockContentBlock::Thinking { text: text.to_string() }],
        ..Default::default()
    }
}

/// Create a multi-block response (e.g., thinking + text, text + tool call).
pub fn multi_block_response(blocks: Vec<MockContentBlock>) -> MockResponse {
    let has_tool_use = blocks.iter().any(|b| matches!(b, MockContentBlock::ToolUse { .. }));
    MockResponse {
        content_blocks: blocks,
        stop_reason: if has_tool_use { "tool_use".to_string() } else { "end_turn".to_string() },
        ..Default::default()
    }
}

/// Create a thinking + text response (common pattern).
pub fn thinking_and_text_response(thinking: &str, text: &str) -> MockResponse {
    multi_block_response(vec![
        MockContentBlock::Thinking { text: thinking.to_string() },
        MockContentBlock::Text { text: text.to_string() },
    ])
}

/// Create a text + tool call response.
pub fn text_and_tool_response(text: &str, tool_id: &str, tool_name: &str, tool_input: Value) -> MockResponse {
    multi_block_response(vec![
        MockContentBlock::Text { text: text.to_string() },
        MockContentBlock::ToolUse {
            id: tool_id.to_string(),
            name: tool_name.to_string(),
            input: tool_input,
        },
    ])
}

/// Create a response simulating a cache hit (high cache_read, zero cache_creation).
pub fn cached_response(text: &str, cached_tokens: u32) -> MockResponse {
    MockResponse {
        content_blocks: vec![MockContentBlock::Text { text: text.to_string() }],
        usage: MockUsage {
            input_tokens: 100,
            output_tokens: 20,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: cached_tokens,
        },
        stop_reason: "end_turn".to_string(),
    }
}

/// Create a response simulating a cache miss (high cache_creation, zero cache_read).
pub fn cache_miss_response(text: &str, creation_tokens: u32) -> MockResponse {
    MockResponse {
        content_blocks: vec![MockContentBlock::Text { text: text.to_string() }],
        usage: MockUsage {
            input_tokens: 100,
            output_tokens: 20,
            cache_creation_input_tokens: creation_tokens,
            cache_read_input_tokens: 0,
        },
        stop_reason: "end_turn".to_string(),
    }
}

/// Create a response with mixed cache activity (both creation and read).
pub fn mixed_cache_response(text: &str, creation_tokens: u32, read_tokens: u32) -> MockResponse {
    MockResponse {
        content_blocks: vec![MockContentBlock::Text { text: text.to_string() }],
        usage: MockUsage {
            input_tokens: 100,
            output_tokens: 20,
            cache_creation_input_tokens: creation_tokens,
            cache_read_input_tokens: read_tokens,
        },
        stop_reason: "end_turn".to_string(),
    }
}

// ── Provider-specific SSE formatters ───────────────────────────────────

/// Render a MockResponse as Anthropic SSE event stream.
pub fn anthropic_sse(response: &MockResponse) -> String {
    let mut body = String::new();

    // message_start
    body.push_str(&format!(
        "data: {}\n\n",
        json!({
            "type": "message_start",
            "message": {
                "id": "msg_test",
                "role": "assistant",
                "content": [],
                "model": "test-model",
                "stop_reason": null,
                "usage": {
                    "input_tokens": response.usage.input_tokens,
                    "output_tokens": 0,
                    "cache_creation_input_tokens": response.usage.cache_creation_input_tokens,
                    "cache_read_input_tokens": response.usage.cache_read_input_tokens
                }
            }
        })
    ));

    for (i, block) in response.content_blocks.iter().enumerate() {
        match block {
            MockContentBlock::Text { text } => {
                let escaped = escape_sse(text);
                body.push_str(&format!(
                    "data: {}\n\n",
                    json!({"type": "content_block_start", "index": i, "content_block": {"type": "text", "text": ""}})
                ));
                body.push_str(&format!(
                    "data: {}\n\n",
                    json!({"type": "content_block_delta", "index": i, "delta": {"type": "text_delta", "text": escaped}})
                ));
                body.push_str(&format!(
                    "data: {}\n\n",
                    json!({"type": "content_block_stop", "index": i})
                ));
            }
            MockContentBlock::ToolUse { id, name, input } => {
                body.push_str(&format!(
                    "data: {}\n\n",
                    json!({"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}}})
                ));
                body.push_str(&format!(
                    "data: {}\n\n",
                    json!({"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": input.to_string()}})
                ));
                body.push_str(&format!(
                    "data: {}\n\n",
                    json!({"type": "content_block_stop", "index": i})
                ));
            }
            MockContentBlock::Thinking { text } => {
                let escaped = escape_sse(text);
                body.push_str(&format!(
                    "data: {}\n\n",
                    json!({"type": "content_block_start", "index": i, "content_block": {"type": "thinking", "thinking": ""}})
                ));
                body.push_str(&format!(
                    "data: {}\n\n",
                    json!({"type": "content_block_delta", "index": i, "delta": {"type": "thinking_delta", "thinking": escaped}})
                ));
                body.push_str(&format!(
                    "data: {}\n\n",
                    json!({"type": "content_block_stop", "index": i})
                ));
            }
        }
    }

    // message_delta + message_stop
    body.push_str(&format!(
        "data: {}\n\n",
        json!({"type": "message_delta", "delta": {"stop_reason": response.stop_reason}, "usage": {"input_tokens": response.usage.input_tokens, "output_tokens": response.usage.output_tokens}})
    ));
    body.push_str(&format!(
        "data: {}\n\n",
        json!({"type": "message_stop"})
    ));

    body
}

/// Render a MockResponse as OpenAI SSE event stream.
pub fn openai_sse(response: &MockResponse) -> String {
    let mut body = String::new();

    for block in &response.content_blocks {
        match block {
            MockContentBlock::Text { text } => {
                let escaped = escape_sse(text);
                body.push_str(&format!(
                    "data: {}\n\n",
                    json!({"id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1, "model": "test",
                        "choices": [{"index": 0, "delta": {"role": "assistant", "content": escaped}, "finish_reason": null}]
                    })
                ));
            }
            MockContentBlock::ToolUse { id, name, input } => {
                body.push_str(&format!(
                    "data: {}\n\n",
                    json!({"id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1, "model": "test",
                        "choices": [{"index": 0, "delta": {"role": "assistant", "tool_calls": [{"index": 0, "id": id, "type": "function", "function": {"name": name, "arguments": input.to_string()}}]}, "finish_reason": null}]
                    })
                ));
            }
            MockContentBlock::Thinking { text } => {
                // OpenAI doesn't have native thinking; include as reasoning_content
                let escaped = escape_sse(text);
                body.push_str(&format!(
                    "data: {}\n\n",
                    json!({"id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1, "model": "test",
                        "choices": [{"index": 0, "delta": {"role": "assistant", "reasoning_content": escaped}, "finish_reason": null}]
                    })
                ));
            }
        }
    }

    // Final chunk with finish_reason
    let finish_reason = if response.stop_reason == "tool_use" { "tool_calls" } else { "stop" };
    body.push_str(&format!(
        "data: {}\n\n",
        json!({"id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1, "model": "test",
            "choices": [{"index": 0, "delta": {}, "finish_reason": finish_reason}]
        })
    ));
    body.push_str("data: [DONE]\n\n");

    body
}

/// Render a MockResponse as Ollama NDJSON stream.
pub fn ollama_sse(response: &MockResponse) -> String {
    let mut body = String::new();

    for block in &response.content_blocks {
        match block {
            MockContentBlock::Text { text } => {
                let escaped = escape_sse(text);
                body.push_str(&format!(
                    "{{\"message\":{{\"role\":\"assistant\",\"content\":\"{escaped}\"}},\"model\":\"test\",\"done\":false}}\n"
                ));
            }
            MockContentBlock::ToolUse { name, input, .. } => {
                body.push_str(&format!(
                    "{{\"message\":{{\"role\":\"assistant\",\"content\":\"\",\"tool_calls\":[{{\"function\":{{\"name\":\"{name}\",\"arguments\":{}}}}}]}},\"model\":\"test\",\"done\":false}}\n",
                    input
                ));
            }
            MockContentBlock::Thinking { text } => {
                let escaped = escape_sse(text);
                body.push_str(&format!(
                    "{{\"message\":{{\"role\":\"assistant\",\"content\":\"\",\"thinking\":\"{escaped}\"}},\"model\":\"test\",\"done\":false}}\n"
                ));
            }
        }
    }

    // Final done message
    body.push_str(
        "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"model\":\"test\",\"done\":true}\n"
    );

    body
}

/// Render a non-streaming Ollama JSON response.
pub fn ollama_non_streaming(response: &MockResponse) -> String {
    let content: String = response.content_blocks.iter().map(|b| {
        match b {
            MockContentBlock::Text { text } => text.clone(),
            _ => String::new(),
        }
    }).collect();

    let escaped = escape_sse(&content);
    format!(
        "{{\"message\":{{\"role\":\"assistant\",\"content\":\"{escaped}\"}},\"model\":\"test\",\"done\":true}}"
    )
}

// ── Mock server mounting helpers ───────────────────────────────────────

/// Determine the API endpoint for a given provider.
pub fn provider_endpoint(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "/v1/messages",
        "openai" | "deepseek" => "/v1/chat/completions",
        "groq" => "/openai/v1/chat/completions",
        "ollama" => "/api/chat",
        "mistral" => "/v1/chat/completions",
        _ => "/v1/chat/completions",
    }
}

/// Determine the content type for a given provider.
pub fn provider_content_type(provider: &str) -> &'static str {
    match provider {
        "ollama" => "application/x-ndjson",
        _ => "text/event-stream",
    }
}

/// Render a MockResponse for the given provider.
pub fn render_for_provider(provider: &str, response: &MockResponse) -> String {
    match provider {
        "anthropic" => anthropic_sse(response),
        "ollama" => ollama_sse(response),
        _ => openai_sse(response),
    }
}

/// Mount a single mock response on the server.
#[cfg(test)]
pub fn mount_sse_once(server: &mut ServerGuard, provider: &str, response: &MockResponse) -> Mock {
    let endpoint = provider_endpoint(provider);
    let content_type = provider_content_type(provider);
    let body = render_for_provider(provider, response);

    let mut mock = server
        .mock("POST", endpoint)
        .with_status(200)
        .with_header("content-type", content_type)
        .with_body(&body)
        .expect(1);

    if provider == "anthropic" {
        mock = mock.with_header("anthropic-version", "2023-06-01");
    }

    mock.create()
}

/// Mount a sequence of mock responses for multi-turn testing.
#[cfg(test)]
pub fn mount_sse_sequence(server: &mut ServerGuard, provider: &str, responses: Vec<MockResponse>) -> Vec<Mock> {
    responses.into_iter().map(|response| {
        mount_sse_once(server, provider, &response)
    }).collect()
}

/// Mount a streaming response with multiple chunks separated by delay simulation.
#[cfg(test)]
pub fn mount_sse_streaming(server: &mut ServerGuard, provider: &str, chunks: Vec<&str>, _delay_ms: u64) -> Mock {
    let full_text = chunks.join("");
    let response = text_response(&full_text);
    mount_sse_once(server, provider, &response)
}

/// Mount an error response on the server.
#[cfg(test)]
pub fn mount_error(server: &mut ServerGuard, provider: &str, status: usize, error_body: &str) -> Mock {
    let endpoint = provider_endpoint(provider);
    server
        .mock("POST", endpoint)
        .with_status(status)
        .with_header("content-type", "application/json")
        .with_body(error_body)
        .expect(1)
        .create()
}

/// Mount an Anthropic API error response.
#[cfg(test)]
pub fn mount_anthropic_error(server: &mut ServerGuard, status: usize, error_type: &str, message: &str) -> Mock {
    mount_error(
        server,
        "anthropic",
        status,
        &json!({"type": "error", "error": {"type": error_type, "message": message}}).to_string(),
    )
}

/// Mount an OpenAI API error response.
#[cfg(test)]
pub fn mount_openai_error(server: &mut ServerGuard, status: usize, message: &str) -> Mock {
    mount_error(
        server,
        "openai",
        status,
        &json!({"error": {"message": message, "type": "invalid_request_error"}}).to_string(),
    )
}

// ── Internal helpers ───────────────────────────────────────────────────

fn escape_sse(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_response() {
        let r = text_response("Hello world");
        assert_eq!(r.content_blocks.len(), 1);
        assert_eq!(r.stop_reason, "end_turn");
    }

    #[test]
    fn test_tool_call_response() {
        let r = tool_call_response("toolu_1", "Read", json!({"path": "src/main.rs"}));
        assert_eq!(r.content_blocks.len(), 1);
        assert_eq!(r.stop_reason, "tool_use");
    }

    #[test]
    fn test_thinking_and_text_response() {
        let r = thinking_and_text_response("Let me analyze...", "Here is the answer");
        assert_eq!(r.content_blocks.len(), 2);
        assert_eq!(r.stop_reason, "end_turn");
    }

    #[test]
    fn test_anthropic_sse_renders() {
        let r = text_response("Hello");
        let sse = anthropic_sse(&r);
        assert!(sse.contains("message_start"));
        assert!(sse.contains("content_block_start"));
        assert!(sse.contains("content_block_delta"));
        assert!(sse.contains("message_stop"));
    }

    #[test]
    fn test_openai_sse_renders() {
        let r = text_response("Hello");
        let sse = openai_sse(&r);
        assert!(sse.contains("chat.completion.chunk"));
        assert!(sse.contains("[DONE]"));
    }

    #[test]
    fn test_ollama_sse_renders() {
        let r = text_response("Hello");
        let sse = ollama_sse(&r);
        assert!(sse.contains("\"done\":false"));
        assert!(sse.contains("\"done\":true"));
    }

    #[test]
    fn test_provider_endpoint() {
        assert_eq!(provider_endpoint("anthropic"), "/v1/messages");
        assert_eq!(provider_endpoint("openai"), "/v1/chat/completions");
        assert_eq!(provider_endpoint("ollama"), "/api/chat");
        assert_eq!(provider_endpoint("groq"), "/openai/v1/chat/completions");
    }

    #[test]
    fn test_multi_block_with_tool_use() {
        let r = text_and_tool_response(
            "I'll read the file",
            "toolu_1",
            "Read",
            json!({"path": "src/main.rs"}),
        );
        assert_eq!(r.content_blocks.len(), 2);
        assert_eq!(r.stop_reason, "tool_use");
    }

    #[test]
    fn test_anthropic_sse_with_tool_call() {
        let r = tool_call_response("toolu_1", "Read", json!({"path": "src/lib.rs"}));
        let sse = anthropic_sse(&r);
        assert!(sse.contains("tool_use"));
        assert!(sse.contains("toolu_1"));
    }

    #[test]
    fn test_openai_sse_with_tool_call() {
        let r = tool_call_response("call_1", "Bash", json!({"command": "ls"}));
        let sse = openai_sse(&r);
        assert!(sse.contains("tool_calls"));
        assert!(sse.contains("tool_calls"));
    }

    #[test]
    fn test_render_for_provider() {
        let r = text_response("test");
        assert!(render_for_provider("anthropic", &r).contains("message_start"));
        assert!(render_for_provider("ollama", &r).contains("done"));
        assert!(render_for_provider("openai", &r).contains("chat.completion.chunk"));
    }

    // ── Cache hit rate tests ──────────────────────────────────────────────

    #[test]
    fn test_mock_usage_default_zero_cache() {
        let usage = MockUsage::default();
        assert_eq!(usage.cache_creation_input_tokens, 0);
        assert_eq!(usage.cache_read_input_tokens, 0);
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 8);
    }

    #[test]
    fn test_cached_response_builder() {
        let r = cached_response("Cached!", 5000);
        assert_eq!(r.usage.cache_creation_input_tokens, 0);
        assert_eq!(r.usage.cache_read_input_tokens, 5000);
        assert_eq!(r.stop_reason, "end_turn");
    }

    #[test]
    fn test_cache_miss_response_builder() {
        let r = cache_miss_response("Fresh!", 3000);
        assert_eq!(r.usage.cache_creation_input_tokens, 3000);
        assert_eq!(r.usage.cache_read_input_tokens, 0);
    }

    #[test]
    fn test_mixed_cache_response_builder() {
        let r = mixed_cache_response("Partial", 1000, 4000);
        assert_eq!(r.usage.cache_creation_input_tokens, 1000);
        assert_eq!(r.usage.cache_read_input_tokens, 4000);
    }

    #[test]
    fn test_anthropic_sse_includes_cache_tokens_in_message_start() {
        let r = cached_response("Hello cached", 2000);
        let sse = anthropic_sse(&r);
        assert!(
            sse.contains("\"cache_creation_input_tokens\":0"),
            "SSE should include cache_creation_input_tokens"
        );
        assert!(
            sse.contains("\"cache_read_input_tokens\":2000"),
            "SSE should include cache_read_input_tokens"
        );
    }

    #[test]
    fn test_anthropic_sse_cache_miss_tokens() {
        let r = cache_miss_response("Fresh response", 8000);
        let sse = anthropic_sse(&r);
        assert!(
            sse.contains("\"cache_creation_input_tokens\":8000"),
            "SSE should include cache_creation tokens for miss"
        );
        assert!(
            sse.contains("\"cache_read_input_tokens\":0"),
            "SSE should show zero cache_read for miss"
        );
    }

    #[test]
    fn test_anthropic_sse_mixed_cache_tokens() {
        let r = mixed_cache_response("Partial cache", 1500, 6000);
        let sse = anthropic_sse(&r);
        assert!(sse.contains("\"cache_creation_input_tokens\":1500"));
        assert!(sse.contains("\"cache_read_input_tokens\":6000"));
    }

    #[test]
    fn test_cache_hit_rate_calculation() {
        // Simulate a multi-turn conversation with cache tokens
        let responses = vec![
            cache_miss_response("Turn 1", 10000), // First turn: cache miss
            cached_response("Turn 2", 8000),       // Second turn: cache hit
            cached_response("Turn 3", 9000),       // Third turn: cache hit
            mixed_cache_response("Turn 4", 2000, 7000), // Partial hit
        ];

        let total_cache_read: u32 = responses.iter().map(|r| r.usage.cache_read_input_tokens).sum();
        let total_cache_creation: u32 = responses.iter().map(|r| r.usage.cache_creation_input_tokens).sum();
        let total_input: u32 = responses.iter().map(|r| r.usage.input_tokens).sum();

        // cache_read / (cache_read + cache_creation + input_tokens) = hit rate
        let total_pool = total_cache_read + total_cache_creation + total_input;
        let hit_rate = total_cache_read as f64 / total_pool as f64;

        assert_eq!(total_cache_read, 24000);
        assert_eq!(total_cache_creation, 12000);
        assert!(hit_rate > 0.0, "Hit rate should be positive");
        assert!(hit_rate <= 1.0, "Hit rate should be <= 1.0");
        // 24000 / (24000 + 12000 + 400) ≈ 0.66
        assert!(hit_rate > 0.5, "Hit rate should be > 50% for this scenario, got {hit_rate:.2}");
    }

    #[test]
    fn test_cache_hit_rate_zero_when_no_caching() {
        let responses = vec![
            text_response("No cache here"),
            text_response("No cache either"),
        ];
        let total_cache_read: u32 = responses.iter().map(|r| r.usage.cache_read_input_tokens).sum();
        assert_eq!(total_cache_read, 0, "Default responses should have zero cache read");
    }
}
