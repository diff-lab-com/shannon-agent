//! # Claude API Client
//!
//! Async Claude API client with streaming support.

use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use thiserror::Error;

/// Errors that can occur during API communication
#[derive(Error, Debug)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("API error: {status} - {message}")]
    ApiError { status: u16, message: String },

    #[error("Timeout")]
    Timeout,
}

/// Configuration for the Claude API client
#[derive(Debug, Clone)]
pub struct ClaudeClientConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub timeout_seconds: u64,
}

impl Default for ClaudeClientConfig {
    fn default() -> Self {
        Self {
            api_key: std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            base_url: "https://api.anthropic.com/v1/messages".to_string(),
            model: "claude-3-5-sonnet-20241022".to_string(),
            max_tokens: 4096,
            timeout_seconds: 120,
        }
    }
}

/// Message content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessageContent {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: Option<bool>,
    },
}

/// Message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Vec<MessageContent>,
}

/// Streaming event from Claude API
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: serde_json::Value },

    #[serde(rename = "content_block_start")]
    ContentBlockStart { index: usize, content_block: serde_json::Value },

    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: usize, delta: serde_json::Value },

    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize },

    #[serde(rename = "message_delta")]
    MessageDelta { delta: serde_json::Value, usage: serde_json::Value },

    #[serde(rename = "message_stop")]
    MessageStop,
}

/// Stream of API events
pub type MessageStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, ApiError>> + Send>>;

/// Claude API client with streaming support
pub struct ClaudeClient {
    config: ClaudeClientConfig,
    client: Client,
}

impl ClaudeClient {
    /// Create a new Claude API client
    pub fn new(config: ClaudeClientConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .build()
            .expect("Failed to create HTTP client");

        Self { config, client }
    }

    /// Create client from environment variables
    pub fn from_env() -> Result<Self, ApiError> {
        let api_key =
            std::env::var("ANTHROPIC_API_KEY").map_err(|_| ApiError::AuthenticationFailed)?;

        if api_key.is_empty() {
            return Err(ApiError::AuthenticationFailed);
        }

        Ok(Self::new(ClaudeClientConfig {
            api_key,
            ..Default::default()
        }))
    }

    /// Send a message with streaming response
    pub async fn send_message_stream(
        &self,
        _messages: Vec<Message>,
        _tools: Option<serde_json::Value>,
    ) -> Result<MessageStream, ApiError> {
        // Stub: Return empty stream for now
        Ok(Box::pin(futures::stream::empty()))
    }

    /// Send a message and wait for complete response
    pub async fn send_message(
        &self,
        _messages: Vec<Message>,
        _tools: Option<serde_json::Value>,
    ) -> Result<Vec<MessageContent>, ApiError> {
        // Stub: Return empty response for now
        Ok(vec![])
    }

    /// Get the configured model name
    pub fn model(&self) -> &str {
        &self.config.model
    }

    /// Get the configured API key
    pub fn api_key(&self) -> &str {
        &self.config.api_key
    }

    /// Update the model
    pub fn set_model(&mut self, model: String) {
        self.config.model = model;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_default() {
        let config = ClaudeClientConfig::default();
        assert_eq!(config.model, "claude-3-5-sonnet-20241022");
        assert_eq!(config.max_tokens, 4096);
    }

    #[test]
    fn test_client_creation() {
        let config = ClaudeClientConfig {
            api_key: "test-key".to_string(),
            ..Default::default()
        };
        let client = ClaudeClient::new(config);
        assert_eq!(client.api_key(), "test-key");
    }

    #[test]
    fn test_client_from_env_missing_key() {
        // Ensure no API key is set
        unsafe {
            std::env::remove_var("ANTHROPIC_API_KEY");
        }
        let result = ClaudeClient::from_env();
        assert!(matches!(result, Err(ApiError::AuthenticationFailed)));
    }

    #[test]
    fn test_message_content_serialization() {
        let content = MessageContent::Text {
            text: "Hello".to_string(),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("text"));
    }

    #[test]
    fn test_stream_event_serialization() {
        let event = StreamEvent::MessageStop;
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("message_stop"));
    }
}
