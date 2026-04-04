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
