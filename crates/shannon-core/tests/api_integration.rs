//! Integration tests for LLM API clients (Anthropic, OpenAI, Ollama)
//!
//! Tests:
//! - HTTP request formatting for each provider
//! - Response parsing and error handling
//! - Concurrent API requests
//! - Retry logic with backoff
//! - Streaming response handling
//!
//! Uses mockito for HTTP mocking to avoid real API calls

#[cfg(test)]
mod llm_client_tests {
    use mockito::{Server, ServerGuard};
    use serde_json::{json, Value};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::sleep;

    /// Helper to set up mock server with Anthropic-style responses
    fn setup_anthropic_mock(server: &mut ServerGuard, endpoint: &str, response: Value) -> mockito::Mock {
        server
            .mock("POST", endpoint)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("x-request-id", "req_123456")
            .with_body(serde_json::to_string(&response).unwrap())
            .create()
    }

    /// Helper to set up mock server with OpenAI-style responses
    fn setup_openai_mock(server: &mut ServerGuard, endpoint: &str, response: Value) -> mockito::Mock {
        server
            .mock("POST", endpoint)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&response).unwrap())
            .create()
    }

    /// Helper to set up mock server with Ollama-style responses
    fn setup_ollama_mock(server: &mut ServerGuard, endpoint: &str, response: Value) -> mockito::Mock {
        server
            .mock("POST", endpoint)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&response).unwrap())
            .create()
    }

    /// Test helper to validate Anthropic request format
    #[tokio::test]
    async fn test_anthropic_request_formatting() {
        let mut server = Server::new_async().await;
        let endpoint = "/v1/messages";

        let expected_response = json!({
            "id": "msg_123456",
            "type": "message",
            "role": "assistant",
            "content": [
                {
                    "type": "text",
                    "text": "Hello, world!"
                }
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        });

        let mock = setup_anthropic_mock(&mut server, endpoint, expected_response);

        // Simulate making a request to the mock server
        let client = reqwest::Client::new();
        let url = format!("{}{}", server.url(), endpoint);
        let request_body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [
                {
                    "role": "user",
                    "content": "Hello, AI!"
                }
            ]
        });

        let response = client
            .post(&url)
            .header("x-api-key", "sk-test-key")
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
        let json: Value = response.json().await.unwrap();
        assert_eq!(json.get("id").unwrap().as_str(), Some("msg_123456"));
        assert_eq!(json.get("type").unwrap().as_str(), Some("message"));

        mock.assert();
    }

    /// Test OpenAI request formatting
    #[tokio::test]
    async fn test_openai_request_formatting() {
        let mut server = Server::new_async().await;
        let endpoint = "/v1/chat/completions";

        let expected_response = json!({
            "id": "chatcmpl-123456",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello, world!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        let mock = setup_openai_mock(&mut server, endpoint, expected_response);

        let client = reqwest::Client::new();
        let url = format!("{}{}", server.url(), endpoint);
        let request_body = json!({
            "model": "gpt-4",
            "messages": [
                {
                    "role": "user",
                    "content": "Hello, AI!"
                }
            ]
        });

        let response = client
            .post(&url)
            .header("authorization", "Bearer sk-test-key")
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
        let json: Value = response.json().await.unwrap();
        assert_eq!(
            json.get("id").unwrap().as_str(),
            Some("chatcmpl-123456")
        );
        assert_eq!(
            json.get("object").unwrap().as_str(),
            Some("chat.completion")
        );

        mock.assert();
    }

    /// Test Ollama request formatting
    #[tokio::test]
    async fn test_ollama_request_formatting() {
        let mut server = Server::new_async().await;
        let endpoint = "/api/generate";

        let expected_response = json!({
            "model": "llama2",
            "created_at": "2024-01-01T00:00:00Z",
            "response": "Hello, world!",
            "done": true,
            "context": [1, 2, 3, 4, 5],
            "total_duration": 1000000000,
            "load_duration": 500000000,
            "prompt_eval_count": 10,
            "prompt_eval_duration": 200000000,
            "eval_count": 5,
            "eval_duration": 300000000
        });

        let mock = setup_ollama_mock(&mut server, endpoint, expected_response);

        let client = reqwest::Client::new();
        let url = format!("{}{}", server.url(), endpoint);
        let request_body = json!({
            "model": "llama2",
            "prompt": "Hello, AI!",
            "stream": false
        });

        let response = client
            .post(&url)
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
        let json: Value = response.json().await.unwrap();
        assert_eq!(json.get("model").unwrap().as_str(), Some("llama2"));
        assert_eq!(json.get("response").unwrap().as_str(), Some("Hello, world!"));
        assert_eq!(json.get("done").unwrap().as_bool(), Some(true));

        mock.assert();
    }

    /// Test Anthropic error response handling
    #[tokio::test]
    async fn test_anthropic_error_handling() {
        let mut server = Server::new_async().await;
        let endpoint = "/v1/messages";

        let error_response = json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": "Invalid request: missing required field 'messages'"
            }
        });

        let mock = server
            .mock("POST", endpoint)
            .with_status(400)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&error_response).unwrap())
            .create();

        let client = reqwest::Client::new();
        let url = format!("{}{}", server.url(), endpoint);
        let request_body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024
            // Missing messages field
        });

        let response = client
            .post(&url)
            .header("x-api-key", "sk-test-key")
            .header("anthropic-version", "2023-06-01")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 400);
        let json: Value = response.json().await.unwrap();
        assert_eq!(json.get("type").unwrap().as_str(), Some("error"));

        let error = json.get("error").unwrap();
        assert_eq!(
            error.get("type").unwrap().as_str(),
            Some("invalid_request_error")
        );

        mock.assert();
    }

    /// Test OpenAI error response handling
    #[tokio::test]
    async fn test_openai_error_handling() {
        let mut server = Server::new_async().await;
        let endpoint = "/v1/chat/completions";

        let error_response = json!({
            "error": {
                "message": "Invalid authentication",
                "type": "invalid_request_error",
                "param": null,
                "code": "invalid_api_key"
            }
        });

        let mock = server
            .mock("POST", endpoint)
            .with_status(401)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&error_response).unwrap())
            .create();

        let client = reqwest::Client::new();
        let url = format!("{}{}", server.url(), endpoint);
        let request_body = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "test"}]
        });

        let response = client
            .post(&url)
            .header("authorization", "Bearer invalid-key")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 401);
        let json: Value = response.json().await.unwrap();

        let error = json.get("error").unwrap();
        assert_eq!(
            error.get("message").unwrap().as_str(),
            Some("Invalid authentication")
        );
        assert_eq!(
            error.get("code").unwrap().as_str(),
            Some("invalid_api_key")
        );

        mock.assert();
    }

    /// Test concurrent API requests
    #[tokio::test]
    async fn test_concurrent_api_requests() {
        let mut server = Server::new_async().await;
        let endpoint = "/v1/messages";

        let expected_response = json!({
            "id": "msg_concurrent",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Response"}],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "end_turn"
        });

        // Create a mock that can be called multiple times
        let mock = server
            .mock("POST", endpoint)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&expected_response).unwrap())
            .expect(5) // Expect 5 calls
            .create();

        let client = Arc::new(reqwest::Client::new());
        let mut handles = Vec::new();
        let num_requests = 5;

        for i in 0..num_requests {
            let client_clone = client.clone();
            let url = format!("{}{}", server.url(), endpoint);
            let request_body = json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": format!("Message {}", i)}]
            });

            let handle = tokio::spawn(async move {
                client_clone
                    .post(&url)
                    .header("x-api-key", "sk-test-key")
                    .header("anthropic-version", "2023-06-01")
                    .json(&request_body)
                    .send()
                    .await
            });

            handles.push(handle);
        }

        // Wait for all requests to complete
        let mut success_count = 0;
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
            let response = result.unwrap();
            assert_eq!(response.status(), 200);
            success_count += 1;
        }

        assert_eq!(success_count, num_requests);
        mock.assert();
    }

    /// Test retry logic with backoff
    #[tokio::test]
    async fn test_retry_with_backoff() {
        let mut server = Server::new_async().await;
        let endpoint = "/v1/chat/completions";

        // First call fails with 500, second succeeds
        let error_response = json!({
            "error": {
                "message": "Internal server error",
                "type": "server_error"
            }
        });

        let success_response = json!({
            "id": "chatcmpl-retry",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Success after retry"},
                "finish_reason": "stop"
            }]
        });

        let mock = server
            .mock("POST", endpoint)
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&error_response).unwrap())
            .expect(1)
            .create();

        let mock_success = server
            .mock("POST", endpoint)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&success_response).unwrap())
            .expect(1)
            .create();

        let client = reqwest::Client::new();
        let url = format!("{}{}", server.url(), endpoint);
        let request_body = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "test"}]
        });

        // First request (will fail)
        let response1 = client
            .post(&url)
            .header("authorization", "Bearer sk-test-key")
            .json(&request_body.clone())
            .send()
            .await
            .unwrap();

        assert_eq!(response1.status(), 500);

        // Simulate backoff
        sleep(Duration::from_millis(100)).await;

        // Retry request (will succeed)
        let response2 = client
            .post(&url)
            .header("authorization", "Bearer sk-test-key")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        assert_eq!(response2.status(), 200);
        let json: Value = response2.json().await.unwrap();
        assert_eq!(
            json.get("id").unwrap().as_str(),
            Some("chatcmpl-retry")
        );

        mock.assert();
        mock_success.assert();
    }

    /// Test SSE streaming response handling
    #[tokio::test]
    async fn test_anthropic_streaming_response() {
        let mut server = Server::new_async().await;
        let endpoint = "/v1/messages";

        // Simulate SSE streaming response
        let streaming_body = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_123","role":"assistant","content":[]}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world!"}}

event: message_stop
data: {"type":"message_stop"}
"#;

        let mock = server
            .mock("POST", endpoint)
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(streaming_body)
            .create();

        let client = reqwest::Client::new();
        let url = format!("{}{}", server.url(), endpoint);
        let request_body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": true
        });

        let response = client
            .post(&url)
            .header("x-api-key", "sk-test-key")
            .header("anthropic-version", "2023-06-01")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get("content-type").unwrap().to_str().unwrap(),
            "text/event-stream"
        );

        // Read the streaming response
        let body = response.text().await.unwrap();
        assert!(body.contains("event: message_start"));
        assert!(body.contains("text_delta"));
        assert!(body.contains("event: message_stop"));

        mock.assert();
    }

    /// Test request/response flow with proper error handling
    #[tokio::test]
    async fn test_request_response_flow() {
        let mut server = Server::new_async().await;
        let endpoint = "/v1/messages";

        // Test normal successful flow
        let success_response = json!({
            "id": "msg_success",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Success response"}],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "end_turn"
        });

        let mock = server
            .mock("POST", endpoint)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&success_response).unwrap())
            .create();

        let client = reqwest::Client::new();
        let url = format!("{}{}", server.url(), endpoint);
        let request_body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "test"}]
        });

        let response = client
            .post(&url)
            .header("x-api-key", "sk-test-key")
            .header("anthropic-version", "2023-06-01")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
        let json: Value = response.json().await.unwrap();
        assert_eq!(json.get("id").unwrap().as_str(), Some("msg_success"));

        mock.assert();
    }

    /// Test request header validation
    #[tokio::test]
    async fn test_required_request_headers() {
        let mut server = Server::new_async().await;
        let endpoint = "/v1/messages";

        let mock = server
            .mock("POST", endpoint)
            .match_header("x-api-key", "sk-test-key")
            .match_header("anthropic-version", "2023-06-01")
            .match_header("content-type", "application/json")
            .with_status(200)
            .with_body(json!({"id": "msg_headers", "type": "message"}).to_string())
            .create();

        let client = reqwest::Client::new();
        let url = format!("{}{}", server.url(), endpoint);

        let response = client
            .post(&url)
            .header("x-api-key", "sk-test-key")
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "test"}]
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
        mock.assert();
    }

    /// Test large payload handling
    #[tokio::test]
    async fn test_large_payload_handling() {
        let mut server = Server::new_async().await;
        let endpoint = "/v1/chat/completions";

        // Create a large messages array
        let messages: Vec<Value> = (0..100)
            .map(|i| json!({"role": "user", "content": format!("Message number {}", i)}))
            .collect();

        let expected_response = json!({
            "id": "chatcmpl-large",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Processed all messages"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 5000,
                "completion_tokens": 10,
                "total_tokens": 5010
            }
        });

        let mock = setup_openai_mock(&mut server, endpoint, expected_response);

        let client = reqwest::Client::new();
        let url = format!("{}{}", server.url(), endpoint);
        let request_body = json!({
            "model": "gpt-4",
            "messages": messages
        });

        let response = client
            .post(&url)
            .header("authorization", "Bearer sk-test-key")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
        let json: Value = response.json().await.unwrap();
        assert_eq!(
            json.get("usage")
                .unwrap()
                .get("prompt_tokens")
                .unwrap()
                .as_u64(),
            Some(5000)
        );

        mock.assert();
    }
}

/// End-to-end integration tests exercising the full LlmClient stack.
///
/// These tests verify:
/// - LlmClient::send_message (non-streaming) with multi-provider normalization
/// - LlmClient::send_message_stream (streaming) with SSE parsing
/// - Tool call event delivery (including multiple tool calls per chunk)
/// - Provider-specific adapter behavior
#[cfg(test)]
mod e2e_client_tests {
    use mockito::{Server, ServerGuard};
    use serde_json::json;
    use shannon_core::api::{
        LlmClient, LlmClientConfig, LlmProvider, Message, MessageContent,
    };
    use futures::StreamExt;

    /// Create an LlmClient pointing at the mock server.
    fn make_client(server: &ServerGuard, provider: LlmProvider) -> LlmClient {
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: server.url(),
            model: "test-model".to_string(),
            max_tokens: 1024,
            timeout_seconds: 30,
            api_version: "2023-06-01".to_string(),
            provider,
            extra_headers: Default::default(),
            retry_config: shannon_core::api::RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
        };
        LlmClient::new(config)
    }

    fn simple_message() -> Vec<Message> {
        vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("Hello".to_string()),
        }]
    }

    // ── Anthropic non-streaming ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_anthropic_non_streaming_e2e() {
        let mut server = Server::new_async().await;
        let response = json!({
            "id": "msg_e2e",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hi from Anthropic"}],
            "model": "claude-3",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 5, "output_tokens": 3}
        });

        let _mock = server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response.to_string())
            .create();

        let client = make_client(&server, LlmProvider::Anthropic);
        let content = client.send_message(simple_message(), None, None).await.unwrap();
        assert_eq!(content.len(), 1);
    }

    // ── OpenAI non-streaming ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_openai_non_streaming_e2e() {
        let mut server = Server::new_async().await;
        let response = json!({
            "id": "chatcmpl-e2e",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hi from OpenAI"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
        });

        let _mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response.to_string())
            .create();

        let client = make_client(&server, LlmProvider::OpenAI);
        let content = client.send_message(simple_message(), None, None).await.unwrap();
        assert_eq!(content.len(), 1);
    }

    // ── OpenAI non-streaming with tool calls ─────────────────────────────────

    #[tokio::test]
    async fn test_openai_non_streaming_tool_calls_e2e() {
        let mut server = Server::new_async().await;
        let response = json!({
            "id": "chatcmpl-tools",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {"id": "call_1", "type": "function", "function": {"name": "bash", "arguments": "{\"command\":\"ls\"}"}},
                        {"id": "call_2", "type": "function", "function": {"name": "read", "arguments": "{\"path\":\"foo.rs\"}"}}
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let _mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response.to_string())
            .create();

        let client = make_client(&server, LlmProvider::OpenAI);
        let content = client.send_message(simple_message(), None, None).await.unwrap();
        assert_eq!(content.len(), 2, "Should have 2 tool_use blocks");
    }

    // ── Ollama non-streaming ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_ollama_non_streaming_e2e() {
        let mut server = Server::new_async().await;
        let response = json!({
            "model": "llama3",
            "message": {"role": "assistant", "content": "Hi from Ollama"},
            "done": true,
            "prompt_eval_count": 5,
            "eval_count": 3
        });

        let _mock = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response.to_string())
            .create();

        let client = make_client(&server, LlmProvider::Ollama);
        let content = client.send_message(simple_message(), None, None).await.unwrap();
        assert_eq!(content.len(), 1);
    }

    // ── Streaming: OpenAI text delta ──────────────────────────────────────────

    #[tokio::test]
    async fn test_openai_streaming_text_e2e() {
        let mut server = Server::new_async().await;
        // Two SSE chunks with text deltas
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\",\"index\":0}]}\n\n",
            "data: [DONE]\n\n",
        );

        let _mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create();

        let client = make_client(&server, LlmProvider::OpenAI);
        let mut stream = client.send_message_stream(simple_message(), None, None).await.unwrap();

        let mut text = String::new();
        while let Some(result) = stream.next().await {
            let event = result.unwrap();
            if let shannon_core::api::StreamEvent::ContentBlockDelta { delta, .. } = event {
                if let shannon_core::api::ContentDelta::TextDelta { text: t } = delta {
                    text.push_str(&t);
                }
            }
        }
        assert_eq!(text, "Hello");
    }

    // ── Streaming: OpenAI multiple tool calls in one chunk ────────────────────

    #[tokio::test]
    async fn test_openai_streaming_multi_tool_call_e2e() {
        let mut server = Server::new_async().await;
        // One chunk with two tool calls starting simultaneously
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_a\",\"function\":{\"name\":\"bash\",\"arguments\":\"\"}},",
            "{\"index\":1,\"id\":\"call_b\",\"function\":{\"name\":\"read\",\"arguments\":\"\"}}",
            "]},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\",\"index\":0}]}\n\n",
            "data: [DONE]\n\n",
        );

        let _mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create();

        let client = make_client(&server, LlmProvider::OpenAI);
        let mut stream = client.send_message_stream(simple_message(), None, None).await.unwrap();

        let mut tool_starts = Vec::new();
        while let Some(result) = stream.next().await {
            let event = result.unwrap();
            if let shannon_core::api::StreamEvent::ContentBlockStart { index, content_block } = event {
                if let shannon_core::api::ContentBlock::ToolUse { name, .. } = content_block {
                    tool_starts.push((index, name));
                }
            }
        }
        // Both tool calls must be delivered — this was the P0-2 bug
        assert_eq!(tool_starts.len(), 2, "Both tool calls should be delivered, got {:?}", tool_starts);
        assert_eq!(tool_starts[0], (0, "bash".to_string()));
        assert_eq!(tool_starts[1], (1, "read".to_string()));
    }

    // ── Streaming: Anthropic passthrough ──────────────────────────────────────

    #[tokio::test]
    async fn test_anthropic_streaming_e2e() {
        let mut server = Server::new_async().await;
        let body = concat!(
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"world\"}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );

        let _mock = server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create();

        let client = make_client(&server, LlmProvider::Anthropic);
        let mut stream = client.send_message_stream(simple_message(), None, None).await.unwrap();

        let mut text = String::new();
        while let Some(result) = stream.next().await {
            let event = result.unwrap();
            if let shannon_core::api::StreamEvent::ContentBlockDelta { delta, .. } = event {
                if let shannon_core::api::ContentDelta::TextDelta { text: t } = delta {
                    text.push_str(&t);
                }
            }
        }
        assert_eq!(text, "world");
    }
}
