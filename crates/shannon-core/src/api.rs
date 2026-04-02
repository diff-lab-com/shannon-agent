//! # Claude API Client
//!
//! Async Claude API client with streaming support for the Claude Messages API.
//!
//! This module implements a production-ready Claude API client with:
//! - SSE (Server-Sent Events) streaming support
//! - Message API with tool use
//! - Comprehensive error handling
//! - Request/response models matching Claude's API specification

use futures::{Stream, StreamExt};
use futures::task::{Context, Poll};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;
use std::time::Duration;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

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

    #[error("Invalid request body: {0}")]
    InvalidRequestBody(String),

    #[error("Stream ended unexpectedly")]
    StreamEndedUnexpectedly,

    #[error("Tool use error: {0}")]
    ToolUseError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Configuration for the Claude API client
#[derive(Debug, Clone)]
pub struct ClaudeClientConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub timeout_seconds: u64,
    pub version: String,
}

impl Default for ClaudeClientConfig {
    fn default() -> Self {
        Self {
            api_key: std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            base_url: "https://api.anthropic.com".to_string(),
            model: "claude-3-5-sonnet-20241022".to_string(),
            max_tokens: 4096,
            timeout_seconds: 120,
            version: "2023-06-01".to_string(),
        }
    }
}

// ============================================================================
// Message Types
// ============================================================================

/// Message content for Claude API requests
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Simple text content
    Text(String),
    /// Structured content blocks
    Blocks(Vec<ContentBlock>),
}

/// Content block in messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "image")]
    Image {
        source: ImageSource,
    },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: Option<ToolResultContent>,
        is_error: Option<bool>,
    },
}

/// Image source for image blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub media_type: String,
    pub data: String,
}

/// Tool result content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Single(String),
    Multiple(Vec<ContentBlock>),
}

/// Message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: MessageContent,
}

// ============================================================================
// Request/Response Types
// ============================================================================

/// Streaming request parameters
#[derive(Debug, Clone, Serialize)]
pub struct MessageRequest {
    pub model: String,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
}

/// Tool definition for function calling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// Usage information from API response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// API response (non-streaming)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageResponse {
    pub id: String,
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub usage: Usage,
}

// ============================================================================
// Stream Event Types
// ============================================================================

/// Streaming event from Claude API
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: MessageResponse },

    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: usize,
        content_block: ContentBlock,
    },

    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        index: usize,
        delta: ContentDelta,
    },

    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize },

    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDeltaDelta,
        usage: Usage,
    },

    #[serde(rename = "message_stop")]
    MessageStop,

    #[serde(rename = "ping")]
    Ping,
}

/// Delta for content block streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },

    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
}

/// Message delta event data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDeltaDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

// ============================================================================
// Stream Implementation
// ============================================================================

/// Stream of API events
pub type MessageStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, ApiError>> + Send>>;

/// SSE stream implementation for Claude API
pub struct SseStream {
    response: reqwest::Response,
    buffer: String,
    done: bool,
}

impl SseStream {
    fn new(response: reqwest::Response) -> Self {
        Self {
            response,
            buffer: String::new(),
            done: false,
        }
    }

    fn parse_sse_line(&mut self, line: &str) -> Option<Result<StreamEvent, ApiError>> {
        let line = line.trim();
        
        // Skip empty lines and comments
        if line.is_empty() || line.starts_with(':') {
            return None;
        }

        // Parse SSE format: "data: {...}"
        if let Some(json_str) = line.strip_prefix("data: ") {
            if json_str == "[DONE]" {
                self.done = true;
                return Some(Ok(StreamEvent::MessageStop));
            }

            match serde_json::from_str::<StreamEvent>(json_str) {
                Ok(event) => Some(Ok(event)),
                Err(e) => Some(Err(ApiError::InvalidResponse(format!(
                    "Failed to parse SSE event: {}", e
                )))),
            }
        } else {
            Some(Err(ApiError::InvalidResponse(format!(
                "Invalid SSE line: {}", line
            ))))
        }
    }
}

impl Stream for SseStream {
    type Item = Result<StreamEvent, ApiError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }

        // Process buffered lines first
        while let Some(pos) = self.buffer.find('\n') {
            let line = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 1..].to_string();
            
            if let Some(event) = self.parse_sse_line(&line) {
                return Poll::Ready(Some(event));
            }
        }

        Poll::Ready(None)
    }
}

// ============================================================================
// Client Implementation
// ============================================================================

/// Claude API client with streaming support
pub struct ClaudeClient {
    config: ClaudeClientConfig,
    client: Client,
}

impl ClaudeClient {
    /// Create a new Claude API client
    pub fn new(config: ClaudeClientConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
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

    /// Send a message with streaming response (SSE)
    pub async fn send_message_stream(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<MessageStream, ApiError> {
        let request_body = MessageRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            system: None,
            messages,
            tools,
            stream: Some(true),
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
        };

        let url = format!("{}/v1/messages", self.config.base_url);
        
        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", &self.config.version)
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| match e.status() {
                Some(reqwest::StatusCode::UNAUTHORIZED) => ApiError::AuthenticationFailed,
                Some(reqwest::StatusCode::TOO_MANY_REQUESTS) => ApiError::RateLimitExceeded,
                Some(status) => ApiError::ApiError {
                    status: status.as_u16(),
                    message: format!("HTTP error: {}", e),
                },
                None => ApiError::HttpError(e),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiError::ApiError {
                status,
                message: error_text,
            });
        }

        // Create reader from response
        let reader = response;
        
        // Convert to bytes stream
        let bytes_stream = reader.bytes_stream();
        
        // Parse SSE events from byte stream  
        let event_stream = bytes_stream.then(|bytes_result| async move {
            let bytes = bytes_result.map_err(|e| ApiError::HttpError(e))?;
            let text = String::from_utf8_lossy(&bytes);
            
            // Parse SSE events from the response
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with(':') {
                    continue;
                }
                
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if json_str == "[DONE]" {
                        return Ok(StreamEvent::MessageStop);
                    }
                    
                    match serde_json::from_str::<StreamEvent>(json_str) {
                        Ok(event) => return Ok(event),
                        Err(e) => return Err(ApiError::InvalidResponse(format!("Parse error: {}", e))),
                    }
                }
            }
            
            // Continue reading for more data
            Err(ApiError::StreamEndedUnexpectedly)
        });
        
        Ok(Box::pin(event_stream))
    }

    /// Send a message and wait for complete response (non-streaming)
    pub async fn send_message(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<Vec<ContentBlock>, ApiError> {
        let request_body = MessageRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            system: None,
            messages,
            tools,
            stream: Some(false),
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
        };

        let url = format!("{}/v1/messages", self.config.base_url);
        
        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", &self.config.version)
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| match e.status() {
                Some(reqwest::StatusCode::UNAUTHORIZED) => ApiError::AuthenticationFailed,
                Some(reqwest::StatusCode::TOO_MANY_REQUESTS) => ApiError::RateLimitExceeded,
                Some(status) => ApiError::ApiError {
                    status: status.as_u16(),
                    message: format!("HTTP error: {}", e),
                },
                None => ApiError::HttpError(e),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiError::ApiError {
                status,
                message: error_text,
            });
        }

        let api_response: MessageResponse = response
            .json()
            .await
            .map_err(|e| ApiError::InvalidResponse(format!("JSON decode error: {}", e)))?;

        Ok(api_response.content)
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

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.config.base_url
    }

    /// Set a custom base URL (for proxies or alternative endpoints)
    pub fn set_base_url(&mut self, base_url: String) {
        self.config.base_url = base_url;
    }

    /// Get max tokens setting
    pub fn max_tokens(&self) -> u32 {
        self.config.max_tokens
    }

    /// Set max tokens for responses
    pub fn set_max_tokens(&mut self, max_tokens: u32) {
        self.config.max_tokens = max_tokens;
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
        assert_eq!(config.version, "2023-06-01");
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
            model: "claude-3-5-sonnet-20241022".to_string(),
            max_tokens: 4096,
            system: None,
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
            strict: Some(true),
        };
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("bash"));
        assert!(json.contains("strict"));
    }
}
