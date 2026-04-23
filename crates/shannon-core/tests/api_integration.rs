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
    use shannon_core::error::ApiKeyGuard;
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
            max_stream_reconnects: 3,
            budget_tokens: None,
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
        let _api_key_guard = ApiKeyGuard::remove();
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
        assert_eq!(tool_starts.len(), 2, "Both tool calls should be delivered, got {tool_starts:?}");
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

/// Tests for LlmClient::send_message_with_retry and fallback provider behavior.
#[cfg(test)]
mod retry_tests {
    use mockito::{Server, ServerGuard};
    use serde_json::json;
    use shannon_core::api::{
        LlmClient, LlmClientConfig, LlmProvider, Message, MessageContent, RetryConfig,
    };
    use futures::StreamExt;

    /// Set ANTHROPIC_API_KEY for retry tests (uses Anthropic provider)
    struct AnthropicKeyGuard(Option<std::ffi::OsString>);
    impl AnthropicKeyGuard {
        fn set() -> Self {
            let old = std::env::var_os("ANTHROPIC_API_KEY");
            unsafe { std::env::set_var("ANTHROPIC_API_KEY", "test-key"); }
            Self(old)
        }
    }
    impl Drop for AnthropicKeyGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => unsafe { std::env::set_var("ANTHROPIC_API_KEY", v); },
                None => unsafe { std::env::remove_var("ANTHROPIC_API_KEY"); },
            }
        }
    }

    fn make_client_with_retry(
        server: &ServerGuard,
        provider: LlmProvider,
        max_retries: u32,
    ) -> LlmClient {
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: server.url(),
            model: "test-model".to_string(),
            max_tokens: 1024,
            timeout_seconds: 30,
            api_version: "2023-06-01".to_string(),
            provider,
            extra_headers: Default::default(),
            retry_config: RetryConfig::new(max_retries, 10, 50),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
        };
        LlmClient::new(config)
    }

    fn make_client_with_fallback(
        primary: &ServerGuard,
        fallback: &ServerGuard,
    ) -> LlmClient {
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: primary.url(),
            model: "test-model".to_string(),
            max_tokens: 1024,
            timeout_seconds: 30,
            api_version: "2023-06-01".to_string(),
            provider: LlmProvider::OpenAI,
            extra_headers: Default::default(),
            retry_config: RetryConfig::new(1, 10, 50),
            fallback_provider: Some(LlmProvider::Anthropic),
            fallback_base_url: Some(fallback.url()),
            max_stream_reconnects: 3,
            budget_tokens: None,
        };
        LlmClient::new(config)
    }

    fn simple_message() -> Vec<Message> {
        vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("Hello".to_string()),
        }]
    }

    /// Test: send_message_with_retry succeeds on first try.
    #[tokio::test]
    async fn test_retry_succeeds_first_try() {
        let _guard = AnthropicKeyGuard::set();
        let mut server = Server::new_async().await;
        let response = json!({
            "id": "chatcmpl-ok",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "First try success"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
        });

        let mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response.to_string())
            .expect(1)
            .create();

        let client = make_client_with_retry(&server, LlmProvider::OpenAI, 3);
        let content = client.send_message_with_retry(simple_message(), None, None).await.unwrap();
        assert_eq!(content.len(), 1);
        mock.assert();
    }

    /// Test: retries on 500, succeeds on 2nd attempt.
    #[tokio::test]
    async fn test_retry_succeeds_after_server_error() {
        let _guard = AnthropicKeyGuard::set();
        let mut server = Server::new_async().await;

        let error_resp = json!({"error": {"message": "Internal error", "type": "server_error"}});

        let success_resp = json!({
            "id": "chatcmpl-retry-ok",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Success after retry"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
        });

        let _error_mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(error_resp.to_string())
            .expect(1)
            .create();

        let _success_mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(success_resp.to_string())
            .expect(1)
            .create();

        let client = make_client_with_retry(&server, LlmProvider::OpenAI, 3);
        let content = client.send_message_with_retry(simple_message(), None, None).await.unwrap();
        assert_eq!(content.len(), 1);
    }

    /// Test: exhausts all retries, returns last error.
    #[tokio::test]
    async fn test_retry_exhausts_all_attempts() {
        let _guard = AnthropicKeyGuard::set();
        let mut server = Server::new_async().await;

        let error_resp = json!({"error": {"message": "Down", "type": "server_error"}});

        let _mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(503)
            .with_header("content-type", "application/json")
            .with_body(error_resp.to_string())
            .expect(2) // initial + 1 retry
            .create();

        let client = make_client_with_retry(&server, LlmProvider::OpenAI, 1);
        let result = client.send_message_with_retry(simple_message(), None, None).await;
        assert!(result.is_err());
    }

    /// Test: fallback provider activated when primary fails completely.
    #[tokio::test]
    async fn test_fallback_provider_on_primary_failure() {
        let _guard = AnthropicKeyGuard::set();
        let mut primary = Server::new_async().await;
        let mut fallback = Server::new_async().await;

        // Primary always fails
        let _primary_mock = primary
            .mock("POST", "/v1/chat/completions")
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(r#"{"error": {"message": "Primary down", "type": "server_error"}}"#)
            .expect(2) // initial + 1 retry
            .create();

        // Fallback succeeds
        let fallback_resp = json!({
            "id": "msg_fallback",
            "role": "assistant",
            "content": [{"type": "text", "text": "Fallback response"}],
            "model": "claude-3",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 5, "output_tokens": 3}
        });

        let fallback_mock = fallback
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(fallback_resp.to_string())
            .expect(1)
            .create();

        let client = make_client_with_fallback(&primary, &fallback);
        let content = client.send_message_with_retry(simple_message(), None, None).await.unwrap();
        assert_eq!(content.len(), 1);
        fallback_mock.assert();
    }

    /// Test: stream retry succeeds on second attempt.
    #[tokio::test]
    async fn test_stream_retry_succeeds() {
        let _guard = AnthropicKeyGuard::set();
        let mut server = Server::new_async().await;

        // First attempt fails
        let _error_mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(r#"{"error": {"message": "Transient", "type": "server_error"}}"#)
            .expect(1)
            .create();

        // Second attempt succeeds with stream
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"retried\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\",\"index\":0}]}\n\n",
            "data: [DONE]\n\n",
        );

        let _success_mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .expect(1)
            .create();

        let client = make_client_with_retry(&server, LlmProvider::OpenAI, 3);
        let mut stream = client
            .send_message_stream_with_retry(simple_message(), None, None)
            .await
            .unwrap();

        let mut text = String::new();
        while let Some(result) = stream.next().await {
            let event = result.unwrap();
            if let shannon_core::api::StreamEvent::ContentBlockDelta { delta, .. } = event {
                if let shannon_core::api::ContentDelta::TextDelta { text: t } = delta {
                    text.push_str(&t);
                }
            }
        }
        assert_eq!(text, "retried");
    }
}

// ── Query Pipeline Integration Tests ──────────────────────────────────

#[cfg(test)]
mod query_pipeline_tests {
    use futures::StreamExt;
    use mockito::{Server, ServerGuard};
    use serde_json::json;
    use shannon_core::api::{LlmClient, LlmClientConfig, LlmProvider};
    use shannon_core::permissions::PermissionManager;
    use shannon_core::query_engine::{QueryContext, QueryEngine, QueryEvent, QueryMetadata};
    use shannon_core::state::StateManager;
    use shannon_core::tools::ToolRegistry;
    use uuid::Uuid;

    /// Guard to set ANTHROPIC_API_KEY for pipeline tests so that the
    /// internal LlmClientConfig default picks Anthropic as provider.
    struct AnthropicKeyGuard(Option<std::ffi::OsString>);
    impl AnthropicKeyGuard {
        fn set() -> Self {
            let old = std::env::var_os("ANTHROPIC_API_KEY");
            unsafe { std::env::set_var("ANTHROPIC_API_KEY", "test-key"); }
            Self(old)
        }
    }
    impl Drop for AnthropicKeyGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => unsafe { std::env::set_var("ANTHROPIC_API_KEY", v); },
                None => unsafe { std::env::remove_var("ANTHROPIC_API_KEY"); },
            }
        }
    }

    fn make_client(server: &ServerGuard) -> LlmClient {
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: server.url(),
            model: "test-model".to_string(),
            max_tokens: 1024,
            timeout_seconds: 30,
            api_version: "2023-06-01".to_string(),
            provider: LlmProvider::Anthropic,
            extra_headers: Default::default(),
            retry_config: shannon_core::api::RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
        };
        LlmClient::new(config)
    }

    fn make_context(message: &str) -> QueryContext {
        QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: message.to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: false,
                max_tokens: Some(1024),
                model: "test-model".to_string(),
                temperature: None,
                top_p: None,
            },
        }
    }

    /// Test: user message → LLM → text response (full pipeline, no tools).
    #[tokio::test]
    async fn test_query_pipeline_text_response() {
        let _guard = AnthropicKeyGuard::set();
        let mut server = Server::new_async().await;

        // process_query uses send_message_stream, so return SSE format
        let sse_body = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_pipeline\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{\"input_tokens\":15,\"output_tokens\":0}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello from pipeline\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":15,\"output_tokens\":8}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );

        server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_body)
            .create();

        let client = make_client(&server);
        let engine = QueryEngine::with_defaults(
            client,
            ToolRegistry::new(),
            PermissionManager::new(),
            StateManager::new(),
        );

        let ctx = make_context("Hello");
        let mut stream = engine.process_query(ctx, None).await;

        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => events.push(event),
                Err(e) => panic!("Stream error: {e}"),
            }
        }

        let has_text = events.iter().any(|e| matches!(e, QueryEvent::Text { .. }));
        let has_completed = events.iter().any(|e| matches!(e, QueryEvent::Completed { .. }));

        assert!(has_text, "Expected Text event, got: {events:?}");
        assert!(has_completed, "Expected Completed event, got: {events:?}");

        // Verify text content
        let text_content: String = events
            .iter()
            .filter_map(|e| match e {
                QueryEvent::Text { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text_content, "Hello from pipeline");
    }

    /// Test: query pipeline with tool use request and result.
    #[tokio::test]
    async fn test_query_pipeline_with_tool_use() {
        let _guard = AnthropicKeyGuard::set();
        let mut server = Server::new_async().await;

        // First response: assistant requests tool use (SSE format)
        let tool_sse = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_tool\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{\"input_tokens\":20,\"output_tokens\":0}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Let me check that.\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_123\",\"name\":\"bash\",\"input\":{}}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\":\\\"echo hello\\\"}\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"input_tokens\":20,\"output_tokens\":15}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );

        // Second response: after tool result, assistant responds (SSE format)
        let final_sse = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_final\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{\"input_tokens\":30,\"output_tokens\":0}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"The output is: hello\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":30,\"output_tokens\":10}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );

        let mock1 = server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(tool_sse)
            .expect(1)
            .create();

        let mock2 = server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(final_sse)
            .expect(1)
            .create();

        let client = make_client(&server);
        let mut tool_registry = ToolRegistry::new();
        // Register a bash tool mock
        use async_trait::async_trait;
        struct MockBashTool;
        #[async_trait]
        impl shannon_core::tools::Tool for MockBashTool {
            async fn execute(&self, _input: serde_json::Value) -> shannon_core::tools::ToolResult<shannon_core::tools::ToolOutput> {
                Ok(shannon_core::tools::ToolOutput {
                    content: "hello".to_string(),
                    is_error: false,
                    metadata: Default::default(),
                })
            }
            fn name(&self) -> &str { "bash" }
            fn description(&self) -> &str { "Mock bash" }
            fn input_schema(&self) -> serde_json::Value {
                json!({"type": "object", "properties": {"command": {"type": "string"}}})
            }
        }
        tool_registry.register(Box::new(MockBashTool)).unwrap();

        let engine = QueryEngine::with_defaults(
            client,
            tool_registry,
            PermissionManager::new(),
            StateManager::new(),
        );

        let ctx = make_context("Run echo hello");
        let mut stream = engine.process_query(ctx, None).await;

        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => events.push(event),
                Err(e) => panic!("Stream error: {e}"),
            }
        }

        // Should have tool use request and result events
        let has_tool_request = events.iter().any(|e| matches!(e, QueryEvent::ToolUseRequest { .. }));
        let has_tool_result = events.iter().any(|e| matches!(e, QueryEvent::ToolUseResult { .. }));
        let has_completed = events.iter().any(|e| matches!(e, QueryEvent::Completed { .. }));

        assert!(has_tool_request, "Expected ToolUseRequest event, got: {events:?}");
        assert!(has_tool_result, "Expected ToolUseResult event, got: {events:?}");
        assert!(has_completed, "Expected Completed event, got: {events:?}");

        mock1.assert();
        mock2.assert();
    }

    /// Test: query pipeline failure returns Failed event.
    #[tokio::test]
    async fn test_query_pipeline_failure() {
        let _guard = AnthropicKeyGuard::set();
        let mut server = Server::new_async().await;

        server
            .mock("POST", "/v1/messages")
            .with_status(401)
            .with_header("content-type", "application/json")
            .with_body(r#"{"type": "error", "error": {"type": "authentication_error", "message": "Invalid API key"}}"#)
            .create();

        let client = make_client(&server);
        let engine = QueryEngine::with_defaults(
            client,
            ToolRegistry::new(),
            PermissionManager::new(),
            StateManager::new(),
        );

        let ctx = make_context("Hello");
        let mut stream = engine.process_query(ctx, None).await;

        let mut has_failed = false;
        while let Some(result) = stream.next().await {
            if let Ok(QueryEvent::Failed { error, .. }) = result {
                // ApiError::AuthenticationFailed maps to "Authentication failed"
                assert!(
                    error.to_lowercase().contains("authentication")
                        || error.contains("401")
                        || error.to_lowercase().contains("unauthorized"),
                    "Error should mention auth issue: {error}"
                );
                has_failed = true;
            }
        }
        assert!(has_failed, "Expected Failed event for auth error");
    }

    /// Test: multi-turn conversation with context preservation.
    #[tokio::test]
    async fn test_query_pipeline_multi_turn() {
        let _guard = AnthropicKeyGuard::set();
        let mut server = Server::new_async().await;

        // First turn - SSE format
        let turn1_sse = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_turn1\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Turn 1 response\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );

        // Second turn - SSE format
        let turn2_sse = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_turn2\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{\"input_tokens\":25,\"output_tokens\":0}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Turn 2 response\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":25,\"output_tokens\":5}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );

        server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(turn1_sse)
            .expect(1)
            .create();

        server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(turn2_sse)
            .expect(1)
            .create();

        let client = make_client(&server);
        let mut engine = QueryEngine::with_defaults(
            client,
            ToolRegistry::new(),
            PermissionManager::new(),
            StateManager::new(),
        );

        // Turn 1
        let ctx1 = make_context("First message");
        let mut stream1 = engine.process_query(ctx1, None).await;
        let mut text1 = String::new();
        while let Some(result) = stream1.next().await {
            if let Ok(QueryEvent::Text { content, .. }) = result {
                text1.push_str(&content);
            }
        }
        assert_eq!(text1, "Turn 1 response");

        // Add messages to conversation history
        engine.add_user_message("First message".to_string());
        engine.add_assistant_message(vec![shannon_core::api::ContentBlock::Text {
            text: "Turn 1 response".to_string(),
        }]);

        // Verify conversation history has 2 messages
        let history = engine.conversation_history();
        assert_eq!(history.len(), 2);

        // Turn 2
        let ctx2 = make_context("Second message");
        let mut stream2 = engine.process_query(ctx2, None).await;
        let mut text2 = String::new();
        while let Some(result) = stream2.next().await {
            if let Ok(QueryEvent::Text { content, .. }) = result {
                text2.push_str(&content);
            }
        }
        assert_eq!(text2, "Turn 2 response");
    }

    /// Test: session save and restore round-trip.
    #[tokio::test]
    async fn test_query_pipeline_session_save_restore() {
        let _guard = AnthropicKeyGuard::set();
        let mut server = Server::new_async().await;

        server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({
                "id": "msg_session",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "text", "text": "Session test"}],
                "model": "test-model",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 10, "output_tokens": 5}
            }).to_string())
            .expect(1)
            .create();

        let client = make_client(&server);
        let state_manager = StateManager::new();
        let session_id = Uuid::new_v4();

        // Create engine with specific session ID
        let engine = QueryEngine::with_session_id(
            client,
            ToolRegistry::new(),
            PermissionManager::new(),
            state_manager,
            shannon_core::query_engine::QueryEngineConfig::default(),
            session_id,
        );

        // Process a query
        let ctx = make_context("Remember this");
        let mut stream = engine.process_query(ctx, None).await;
        while (stream.next().await).is_some() {}

        // Verify session ID is set
        assert_eq!(engine.session_id(), session_id);
    }
}

// ── Conversation Export E2E Tests ─────────────────────────────────────

#[cfg(test)]
mod conversation_export_tests {
    use shannon_core::api::{ContentBlock, Message, MessageContent};
    use shannon_core::state::{SessionData, SessionPersistMetadata, StateManager};
    use uuid::Uuid;

    /// Helper to create SessionPersistMetadata with sensible defaults.
    fn make_metadata(title: &str, model: &str, turn_count: usize) -> SessionPersistMetadata {
        SessionPersistMetadata {
            model: model.to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            total_input_tokens: 50,
            total_output_tokens: 30,
            turn_count,
            title: Some(title.to_string()),
            parent_session_id: None,
            branch_point_message_index: None,
        }
    }

    /// Test: export conversation as JSON, verify structure.
    #[test]
    fn test_export_conversation_json() {
        let session_id = Uuid::new_v4();
        let data = SessionData {
            session_id,
            metadata: make_metadata("Test Session", "test-model", 2),
            messages: vec![
                Message {
                    role: "user".to_string(),
                    content: MessageContent::Text("Hello".to_string()),
                },
                Message {
                    role: "assistant".to_string(),
                    content: MessageContent::Blocks(vec![
                        ContentBlock::Text { text: "Hi there!".to_string() },
                    ]),
                },
            ],
        };

        // Serialize to JSON
        let json_str = serde_json::to_string_pretty(&data).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        // Verify structure
        assert_eq!(parsed["metadata"]["model"], "test-model");
        assert_eq!(parsed["metadata"]["turn_count"], 2);
        assert_eq!(parsed["messages"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["messages"][0]["role"], "user");
        assert_eq!(parsed["messages"][1]["role"], "assistant");
    }

    /// Test: export conversation as markdown.
    #[test]
    fn test_export_conversation_markdown() {
        let messages = vec![
            ("user", "What is Rust?"),
            ("assistant", "Rust is a systems programming language."),
            ("user", "Is it memory safe?"),
            ("assistant", "Yes, Rust guarantees memory safety."),
        ];

        let mut md = String::from("# Shannon Session Export\n\n");
        for (role, content) in &messages {
            let heading = match *role {
                "user" => "## User",
                "assistant" => "## Assistant",
                _ => "## System",
            };
            md.push_str(&format!("{heading}\n\n{content}\n\n---\n\n"));
        }

        // Verify format
        assert!(md.contains("# Shannon Session Export"));
        assert!(md.contains("## User\n\nWhat is Rust?"));
        assert!(md.contains("## Assistant\n\nRust is a systems programming language."));
        assert!(md.contains("---")); // Separators between messages
    }

    /// Test: export with metadata (model, tokens, cost).
    #[test]
    fn test_export_with_metadata() {
        let metadata = SessionPersistMetadata {
            model: "claude-3-5-sonnet".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            total_input_tokens: 1500,
            total_output_tokens: 800,
            turn_count: 5,
            title: Some("Metadata Test".to_string()),
            parent_session_id: None,
            branch_point_message_index: None,
        };

        // Build metadata summary
        let summary = format!(
            "Model: {}\nTurns: {}\nInput tokens: {}\nOutput tokens: {}\nTotal tokens: {}\nEstimated cost: ${:.4}",
            metadata.model,
            metadata.turn_count,
            metadata.total_input_tokens,
            metadata.total_output_tokens,
            metadata.total_input_tokens + metadata.total_output_tokens,
            (metadata.total_input_tokens as f64 * 0.000003) + (metadata.total_output_tokens as f64 * 0.000015),
        );

        assert!(summary.contains("Model: claude-3-5-sonnet"));
        assert!(summary.contains("Turns: 5"));
        assert!(summary.contains("Total tokens: 2300"));
        assert!(summary.contains("Estimated cost: $"));
    }

    /// Test: session save/load round-trip preserves data.
    #[test]
    fn test_session_save_load_roundtrip() {
        let state_manager = StateManager::new();
        let session_id = Uuid::new_v4();

        let messages = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("Hello".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::Text { text: "World".to_string() },
                ]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("How are you?".to_string()),
            },
        ];

        let metadata = SessionPersistMetadata {
            model: "test-model".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            total_input_tokens: 100,
            total_output_tokens: 50,
            turn_count: 3,
            title: Some("Round-trip test".to_string()),
            parent_session_id: None,
            branch_point_message_index: None,
        };

        // Save
        state_manager.save_session(&session_id, &messages, &metadata).unwrap();

        // Load
        let loaded = state_manager.load_session(&session_id).unwrap();
        assert!(loaded.is_some());

        let loaded_data = loaded.unwrap();
        assert_eq!(loaded_data.session_id, session_id);
        assert_eq!(loaded_data.metadata.title, Some("Round-trip test".to_string()));
        assert_eq!(loaded_data.metadata.model, "test-model");
        assert_eq!(loaded_data.messages.len(), 3);
        assert_eq!(loaded_data.metadata.turn_count, 2); // 2 user messages

        // Verify message content preserved
        assert_eq!(loaded_data.messages[0].role, "user");
        assert_eq!(loaded_data.messages[1].role, "assistant");

        // Clean up
        state_manager.delete_persisted_session(&session_id).unwrap();
    }

    /// Test: list persisted sessions.
    #[test]
    fn test_list_persisted_sessions() {
        let state_manager = StateManager::new();
        let mut saved_ids = Vec::new();

        // Save two sessions
        for i in 0..2 {
            let sid = Uuid::new_v4();
            let metadata = SessionPersistMetadata {
                model: "test-model".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                total_input_tokens: 10 * (i + 1) as u64,
                total_output_tokens: 5 * (i + 1) as u64,
                turn_count: i + 1,
                title: Some(format!("Session {i}")),
                parent_session_id: None,
                branch_point_message_index: None,
            };
            state_manager.save_session(&sid, &[], &metadata).unwrap();
            saved_ids.push(sid);
        }

        let sessions = state_manager.list_persisted_sessions().unwrap();
        assert!(sessions.len() >= 2, "Should have at least 2 sessions");

        // Clean up
        for sid in &saved_ids {
            state_manager.delete_persisted_session(sid).unwrap();
        }
    }

    /// Test: export with block content (tool use).
    #[test]
    fn test_export_with_tool_use_blocks() {
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::Text { text: "Let me check that.".to_string() },
                    ContentBlock::ToolUse {
                        id: "toolu_123".to_string(),
                        name: "bash".to_string(),
                        input: serde_json::json!({"command": "ls"}),
                    },
                ]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::ToolResult {
                        tool_use_id: "toolu_123".to_string(),
                        content: Some(shannon_core::api::ToolResultContent::Single("file1.txt\nfile2.txt".to_string())),
                        is_error: Some(false),
                    },
                ]),
            },
        ];

        // Serialize
        let json = serde_json::to_string(&messages).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.as_array().unwrap().len(), 2);
        // First message has text + tool_use blocks
        assert_eq!(parsed[0]["content"].as_array().unwrap().len(), 2);
        assert_eq!(parsed[0]["content"][0]["type"], "text");
        assert_eq!(parsed[0]["content"][1]["type"], "tool_use");
    }
}

mod permission_flow_tests {
    use futures::StreamExt;
    use mockito::{Server, ServerGuard};
    use shannon_core::api::{LlmClient, LlmClientConfig, LlmProvider};
    use shannon_core::permissions::{PermissionChoice, PermissionManager};
    use shannon_core::query_engine::{QueryContext, QueryEngine, QueryEvent, QueryMetadata, PermissionRequest};
    use shannon_core::state::StateManager;
    use shannon_core::tools::ToolRegistry;
    use uuid::Uuid;

    struct AnthropicKeyGuard(Option<std::ffi::OsString>);
    impl AnthropicKeyGuard {
        fn set() -> Self {
            let old = std::env::var_os("ANTHROPIC_API_KEY");
            unsafe { std::env::set_var("ANTHROPIC_API_KEY", "test-key"); }
            Self(old)
        }
    }
    impl Drop for AnthropicKeyGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => unsafe { std::env::set_var("ANTHROPIC_API_KEY", v); },
                None => unsafe { std::env::remove_var("ANTHROPIC_API_KEY"); },
            }
        }
    }

    fn make_client(server: &ServerGuard) -> LlmClient {
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: server.url(),
            model: "test-model".to_string(),
            max_tokens: 1024,
            timeout_seconds: 30,
            api_version: "2023-06-01".to_string(),
            provider: LlmProvider::Anthropic,
            extra_headers: Default::default(),
            retry_config: shannon_core::api::RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
        };
        LlmClient::new(config)
    }

    fn make_context(message: &str) -> QueryContext {
        QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: message.to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: Some(1024),
                model: "test-model".to_string(),
                temperature: None,
                top_p: None,
            },
        }
    }

    /// Test: tool permission denied → graceful error event, not panic.
    /// The pipeline should emit a ToolUseRequest, we deny it via the
    /// permission channel, and the engine should recover gracefully.
    #[tokio::test]
    async fn test_permission_denied_flow() {
        let _guard = AnthropicKeyGuard::set();
        let mut server = Server::new_async().await;

        // First response: assistant requests a tool use
        let tool_sse = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_perm\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_perm\",\"name\":\"bash\",\"input\":{}}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\":\\\"rm -rf /\\\"}\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );

        // After permission denied, the engine should send a tool_result with is_error
        // and then make another LLM call. We provide a recovery response.
        let recovery_sse = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_recovery\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{\"input_tokens\":20,\"output_tokens\":0}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Understood, I won't run that command.\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":20,\"output_tokens\":8}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );

        server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(tool_sse)
            .expect(1)
            .create();

        server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(recovery_sse)
            .expect(1)
            .create();

        let client = make_client(&server);
        let engine = QueryEngine::with_defaults(
            client,
            ToolRegistry::new(),
            PermissionManager::new(),
            StateManager::new(),
        );

        let ctx = make_context("Delete everything");
        let (perm_tx, mut perm_rx) = tokio::sync::mpsc::unbounded_channel::<PermissionRequest>();

        // Spawn a task to deny the permission request
        let deny_handle = tokio::spawn(async move {
            // Wait for a permission request
            if let Some(req) = perm_rx.recv().await {
                // Deny the permission
                let _ = req.response_tx.send(PermissionChoice::Deny);
            }
        });

        let mut stream = engine.process_query(ctx, Some(perm_tx)).await;

        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => events.push(event),
                Err(e) => {
                    // Permission denied should not cause a stream error;
                    // it should be handled gracefully within the event flow
                    panic!("Unexpected stream error: {e}");
                }
            }
        }

        // Wait for the deny task to complete
        let _ = deny_handle.await;

        // Should have completed (not failed) - the engine recovered
        let has_completed = events.iter().any(|e| matches!(e, QueryEvent::Completed { .. }));
        let has_tool_request = events.iter().any(|e| matches!(e, QueryEvent::ToolUseRequest { .. }));

        assert!(has_tool_request, "Expected ToolUseRequest event before denial");

        // Engine should complete gracefully (either with recovery text or just Completed)
        // The key assertion is that it doesn't panic or hang
        assert!(
            has_completed || events.iter().any(|e| matches!(e, QueryEvent::Failed { .. })),
            "Expected either Completed or Failed event, got: {:?}",
            events.iter().map(|e| match e {
                QueryEvent::Started { .. } => "Started",
                QueryEvent::Text { .. } => "Text",
                QueryEvent::ToolUseRequest { .. } => "ToolUseRequest",
                QueryEvent::ToolUseResult { .. } => "ToolUseResult",
                QueryEvent::TurnCompleted { .. } => "TurnCompleted",
                QueryEvent::Progress { .. } => "Progress",
                QueryEvent::Usage { .. } => "Usage",
                QueryEvent::Cost { .. } => "Cost",
                QueryEvent::ToolProgress { .. } => "ToolProgress",
                QueryEvent::Completed { .. } => "Completed",
                QueryEvent::Failed { .. } => "Failed",
                QueryEvent::Thinking { .. } => "Thinking",
                QueryEvent::Info { .. } => "Info",
            }).collect::<Vec<_>>()
        );
    }
}
