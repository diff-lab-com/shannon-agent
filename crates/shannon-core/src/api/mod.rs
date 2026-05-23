//! # LLM API Client
//!
//! Async LLM API client with streaming support for multiple providers.
//!
//! This module implements a production-ready API client with:
//! - Multi-provider support (Anthropic, OpenAI, Ollama, Custom)
//! - SSE (Server-Sent Events) streaming support
//! - Message API with tool use
//! - Comprehensive error handling
//! - Request/response models compatible with common LLM APIs

pub mod adapter;
pub mod client;
pub mod error;
pub mod retry;
pub mod streaming;
pub mod types;

// Re-export all public types so that `crate::api::X` paths continue to work.
pub use error::ApiError;

pub use types::{
    ClaudeClientConfig, ContentBlock, ContentDelta, ImageSource, LlmClientConfig, LlmProvider,
    Message, MessageContent, MessageDeltaDelta, MessageRequest, MessageResponse, StreamEvent,
    SystemContentBlock, ToolDefinition, ToolResultContent, Usage,
};

pub use retry::RetryConfig;
pub use streaming::MessageStream;

pub use client::{ClaudeClient, LlmClient};

// Tests from the original flat module
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // --- Provider Detection Tests ---

    #[test]
    fn test_provider_detection_anthropic() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.anthropic.com"),
            LlmProvider::Anthropic
        );
    }

    #[test]
    fn test_provider_detection_openai() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.openai.com"),
            LlmProvider::OpenAI
        );
    }

    #[test]
    fn test_provider_detection_ollama_localhost() {
        assert_eq!(
            LlmProvider::from_base_url("http://localhost:11434"),
            LlmProvider::Ollama
        );
    }

    #[test]
    fn test_provider_detection_ollama_127() {
        assert_eq!(
            LlmProvider::from_base_url("http://127.0.0.1:11434"),
            LlmProvider::Ollama
        );
    }

    #[test]
    fn test_provider_detection_ollama_by_name() {
        assert_eq!(
            LlmProvider::from_base_url("http://my-server:8080/ollama"),
            LlmProvider::Ollama
        );
    }

    #[test]
    fn test_provider_detection_ollama_by_port() {
        assert_eq!(
            LlmProvider::from_base_url("http://192.168.1.100:11434"),
            LlmProvider::Ollama
        );
    }

    #[test]
    fn test_provider_detection_localhost_non_ollama_is_custom() {
        // A localhost service on a non-11434 port without "ollama" should be Custom
        assert_eq!(
            LlmProvider::from_base_url("http://localhost:8080"),
            LlmProvider::Custom
        );
        assert_eq!(
            LlmProvider::from_base_url("http://127.0.0.1:5000"),
            LlmProvider::Custom
        );
    }

    #[test]
    fn test_provider_detection_custom() {
        assert_eq!(
            LlmProvider::from_base_url("https://my-llm.example.com"),
            LlmProvider::Custom
        );
    }

    // --- Provider Endpoint Tests ---

    #[test]
    fn test_endpoint_anthropic() {
        assert_eq!(LlmProvider::Anthropic.endpoint(), "/v1/messages");
    }

    #[test]
    fn test_endpoint_openai() {
        assert_eq!(LlmProvider::OpenAI.endpoint(), "/v1/chat/completions");
    }

    #[test]
    fn test_endpoint_ollama() {
        assert_eq!(LlmProvider::Ollama.endpoint(), "/api/chat");
    }

    #[test]
    fn test_endpoint_custom() {
        assert_eq!(LlmProvider::Custom.endpoint(), "/v1/messages");
    }

    #[test]
    fn test_provider_requires_auth() {
        assert!(LlmProvider::Anthropic.requires_auth());
        assert!(LlmProvider::OpenAI.requires_auth());
        assert!(LlmProvider::Custom.requires_auth());
        assert!(!LlmProvider::Ollama.requires_auth());
    }

    #[test]
    fn test_provider_display() {
        assert_eq!(LlmProvider::Anthropic.to_string(), "anthropic");
        assert_eq!(LlmProvider::OpenAI.to_string(), "openai");
        assert_eq!(LlmProvider::Ollama.to_string(), "ollama");
        assert_eq!(LlmProvider::Custom.to_string(), "custom");
    }

    // --- Config Tests ---

    #[test]
    fn test_config_default() {
        let config = LlmClientConfig::default();
        assert_eq!(config.max_tokens, 4096);
        // Timeout depends on provider: 120 for cloud, 300 for Ollama fallback
        assert!(config.timeout_seconds == 120 || config.timeout_seconds == 300);
        assert!(config.extra_headers.is_empty());
    }

    #[test]
    fn test_config_ollama_default() {
        let config = LlmClientConfig::ollama_default();
        assert_eq!(config.provider, LlmProvider::Ollama);
        assert_eq!(config.base_url, "http://localhost:11434");
        assert!(config.api_key.is_empty());
        assert!(config.api_version.is_empty());
    }

    #[test]
    fn test_config_openai_default() {
        let config = LlmClientConfig::openai_default();
        assert_eq!(config.provider, LlmProvider::OpenAI);
        assert_eq!(config.base_url, "https://api.openai.com");
        assert!(config.api_key.is_empty()); // no key set in test env
    }

    #[test]
    fn test_client_creation() {
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            provider: LlmProvider::Anthropic,
            ..Default::default()
        };
        let client = LlmClient::new(config);
        assert_eq!(client.api_key(), "test-key");
        assert_eq!(client.provider(), &LlmProvider::Anthropic);
    }

    #[test]
    fn test_client_unauthenticated() {
        let config = LlmClientConfig::ollama_default();
        let client = LlmClient::new_unauthenticated(config);
        assert_eq!(client.provider(), &LlmProvider::Ollama);
    }

    #[test]
    fn test_client_set_base_url_auto_detects() {
        let mut client = LlmClient::new(LlmClientConfig::default());
        client.set_base_url("https://api.openai.com".to_string());
        assert_eq!(client.provider(), &LlmProvider::OpenAI);
        assert_eq!(client.base_url(), "https://api.openai.com");
    }

    #[test]
    fn test_client_add_header() {
        let mut client = LlmClient::new(LlmClientConfig {
            provider: LlmProvider::Custom,
            ..Default::default()
        });
        client.add_header("X-Custom".to_string(), "value".to_string());
        assert_eq!(
            client.config().extra_headers.get("X-Custom"),
            Some(&"value".to_string())
        );
    }

    // --- Auth Headers Tests ---

    #[test]
    fn test_auth_headers_anthropic() {
        let client = LlmClient::new(LlmClientConfig {
            api_key: "sk-ant-test".to_string(),
            api_version: "2023-06-01".to_string(),
            provider: LlmProvider::Anthropic,
            ..Default::default()
        });
        let headers = client.auth_headers();
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "x-api-key" && v == "sk-ant-test")
        );
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "anthropic-version" && v == "2023-06-01")
        );
    }

    #[test]
    fn test_auth_headers_openai() {
        let client = LlmClient::new(LlmClientConfig {
            api_key: "sk-oai-test".to_string(),
            provider: LlmProvider::OpenAI,
            ..Default::default()
        });
        let headers = client.auth_headers();
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "Bearer sk-oai-test")
        );
    }

    #[test]
    fn test_auth_headers_ollama() {
        let client = LlmClient::new(LlmClientConfig::ollama_default());
        let headers = client.auth_headers();
        assert!(headers.is_empty());
    }

    #[test]
    fn test_auth_headers_custom() {
        let mut extra = HashMap::new();
        extra.insert("X-Auth".to_string(), "token123".to_string());
        let client = LlmClient::new(LlmClientConfig {
            provider: LlmProvider::Custom,
            extra_headers: extra,
            ..Default::default()
        });
        let headers = client.auth_headers();
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "X-Auth" && v == "token123")
        );
    }

    // --- Endpoint URL Tests ---

    #[test]
    fn test_endpoint_url_anthropic() {
        let client = LlmClient::new(LlmClientConfig {
            base_url: "https://api.anthropic.com".to_string(),
            provider: LlmProvider::Anthropic,
            ..Default::default()
        });
        assert_eq!(
            client.endpoint_url(),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn test_endpoint_url_openai() {
        let client = LlmClient::new(LlmClientConfig {
            base_url: "https://api.openai.com".to_string(),
            provider: LlmProvider::OpenAI,
            ..Default::default()
        });
        assert_eq!(
            client.endpoint_url(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_endpoint_url_ollama() {
        let client = LlmClient::new(LlmClientConfig::ollama_default());
        assert_eq!(client.endpoint_url(), "http://localhost:11434/api/chat");
    }

    // --- Message Serialization Tests ---

    #[test]
    fn test_message_content_serialization() {
        let content = MessageContent::Text("Hello".to_string());
        let json = serde_json::to_string(&content).unwrap();
        assert_eq!(json, "\"Hello\"");
    }

    #[test]
    fn test_content_block_text_serialization() {
        let block = ContentBlock::Text {
            text: "Hello".to_string(),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#"type":"text""#));
        assert!(json.contains("Hello"));
    }

    #[test]
    fn test_tool_use_block_serialization() {
        let block = ContentBlock::ToolUse {
            id: "tool_123".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "echo hello"}),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#"type":"tool_use""#));
        assert!(json.contains("bash"));
    }

    #[test]
    fn test_stream_event_serialization() {
        let event = StreamEvent::MessageStop;
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("message_stop"));
    }

    #[test]
    fn test_message_request_serialization() {
        let request = MessageRequest {
            model: "test-model".to_string(),
            max_tokens: 4096,
            system: None,
            system_blocks: None,
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Text("Hello".to_string()),
            }],
            tools: None,
            stream: Some(true),
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            budget_tokens: None,
            thinking_budget: None,
            reasoning_effort: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("stream"));
        assert!(json.contains("true"));
    }

    #[test]
    fn test_tool_definition_serialization() {
        let tool = ToolDefinition {
            name: "bash".to_string(),
            description: "Run bash commands".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"}
                }
            }),
            cache_control: None,
            strict: Some(true),
        };
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("bash"));
        assert!(json.contains("strict"));
    }

    // --- Backward Compatibility Tests ---

    #[test]
    fn test_backward_compat_claude_client_config() {
        let config: ClaudeClientConfig = ClaudeClientConfig {
            api_key: "test".to_string(),
            ..Default::default()
        };
        assert_eq!(config.api_key, "test");
    }

    #[test]
    fn test_backward_compat_claude_client() {
        let client: ClaudeClient = ClaudeClient::new(LlmClientConfig {
            api_key: "test".to_string(),
            ..Default::default()
        });
        assert_eq!(client.api_key(), "test");
    }

    // --- Provider Serialization ---

    #[test]
    fn test_provider_serde_roundtrip() {
        let provider = LlmProvider::Anthropic;
        let json = serde_json::to_string(&provider).unwrap();
        let parsed: LlmProvider = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, LlmProvider::Anthropic);
    }

    #[test]
    fn test_all_providers_serde() {
        for provider in &[
            LlmProvider::Anthropic,
            LlmProvider::OpenAI,
            LlmProvider::Ollama,
            LlmProvider::Custom,
        ] {
            let json = serde_json::to_string(provider).unwrap();
            let parsed: LlmProvider = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, provider);
        }
    }
}
