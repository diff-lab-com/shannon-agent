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

use futures_util::{Stream, StreamExt};
use futures_util::task::{Context, Poll};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
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

    #[error("Unsupported provider: {0}")]
    UnsupportedProvider(String),
}

// ============================================================================
// Provider Types
// ============================================================================

/// Supported LLM providers
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LlmProvider {
    /// Anthropic API (api.anthropic.com)
    Anthropic,
    /// OpenAI API (api.openai.com)
    OpenAI,
    /// Ollama local inference (localhost:11434)
    Ollama,
    /// Custom endpoint with user-defined base URL
    Custom,
}

impl LlmProvider {
    /// Detect provider from a base URL.
    ///
    /// Ollama detection requires port 11434 or the string "ollama" in the URL,
    /// to avoid misidentifying arbitrary localhost services.
    pub fn from_base_url(base_url: &str) -> Self {
        let url = base_url.to_lowercase();
        if url.contains("api.anthropic.com") {
            LlmProvider::Anthropic
        } else if url.contains("api.openai.com") {
            LlmProvider::OpenAI
        } else if url.contains("ollama")
            || url.contains(":11434")
            || (url.contains("localhost") && url.contains("11434"))
        {
            LlmProvider::Ollama
        } else {
            LlmProvider::Custom
        }
    }

    /// Get the API endpoint path for this provider
    pub fn endpoint(&self) -> &'static str {
        match self {
            LlmProvider::Anthropic => "/v1/messages",
            LlmProvider::OpenAI => "/v1/chat/completions",
            LlmProvider::Ollama => "/api/chat",
            LlmProvider::Custom => "/v1/messages",
        }
    }

    /// Whether this provider requires authentication
    pub fn requires_auth(&self) -> bool {
        !matches!(self, LlmProvider::Ollama)
    }
}

impl std::fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmProvider::Anthropic => write!(f, "anthropic"),
            LlmProvider::OpenAI => write!(f, "openai"),
            LlmProvider::Ollama => write!(f, "ollama"),
            LlmProvider::Custom => write!(f, "custom"),
        }
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the LLM API client
#[derive(Debug, Clone)]
pub struct LlmClientConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub timeout_seconds: u64,
    pub api_version: String,
    pub provider: LlmProvider,
    pub extra_headers: HashMap<String, String>,
}

impl Default for LlmClientConfig {
    fn default() -> Self {
        let api_key = std::env::var("SHANNON_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .unwrap_or_default();

        let base_url = std::env::var("SHANNON_BASE_URL")
            .or_else(|_| std::env::var("ANTHROPIC_BASE_URL"))
            .or_else(|_| std::env::var("OPENAI_BASE_URL"))
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());

        let model = std::env::var("SHANNON_MODEL")
            .or_else(|_| std::env::var("ANTHROPIC_MODEL"))
            .or_else(|_| std::env::var("OPENAI_MODEL"))
            .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());

        let provider = LlmProvider::from_base_url(&base_url);

        let api_version = match provider {
            LlmProvider::Anthropic => std::env::var("ANTHROPIC_API_VERSION")
                .unwrap_or_else(|_| "2023-06-01".to_string()),
            _ => String::new(),
        };

        Self {
            api_key,
            base_url,
            model,
            max_tokens: 4096,
            timeout_seconds: 120,
            api_version,
            provider,
            extra_headers: HashMap::new(),
        }
    }
}

impl LlmClientConfig {
    /// Create a permissive config for Ollama (no auth needed)
    pub fn ollama_default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "http://localhost:11434".to_string(),
            model: "llama3".to_string(),
            max_tokens: 4096,
            timeout_seconds: 300,
            api_version: String::new(),
            provider: LlmProvider::Ollama,
            extra_headers: HashMap::new(),
        }
    }

    /// Create config for OpenAI
    pub fn openai_default() -> Self {
        let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com".to_string());
        let model = std::env::var("OPENAI_MODEL")
            .unwrap_or_else(|_| "gpt-4o".to_string());
        Self {
            api_key,
            base_url,
            model,
            max_tokens: 4096,
            timeout_seconds: 120,
            api_version: String::new(),
            provider: LlmProvider::OpenAI,
            extra_headers: HashMap::new(),
        }
    }
}

/// Backward-compatible alias
pub type ClaudeClientConfig = LlmClientConfig;

// ============================================================================
// Message Types
// ============================================================================

/// Message content for API requests
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

/// Streaming event from API
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

/// SSE stream implementation
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

/// LLM API client with multi-provider and streaming support
pub struct LlmClient {
    config: LlmClientConfig,
    client: Client,
}

impl LlmClient {
    /// Create a new LLM API client
    pub fn new(config: LlmClientConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .expect("Failed to create HTTP client");

        Self { config, client }
    }

    /// Create client from environment variables.
    ///
    /// Checks `SHANNON_API_KEY` → `ANTHROPIC_API_KEY` → `OPENAI_API_KEY`
    /// and auto-detects provider from base URL.
    pub fn from_env() -> Result<Self, ApiError> {
        let api_key = std::env::var("SHANNON_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .map_err(|_| ApiError::AuthenticationFailed)?;

        if api_key.is_empty() {
            return Err(ApiError::AuthenticationFailed);
        }

        let config = LlmClientConfig::default();
        Ok(Self::new(LlmClientConfig {
            api_key,
            ..config
        }))
    }

    /// Create a client that requires no authentication (e.g., Ollama)
    pub fn new_unauthenticated(config: LlmClientConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .expect("Failed to create HTTP client");

        Self { config, client }
    }

    /// Build authentication headers for the configured provider
    fn auth_headers(&self) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        match self.config.provider {
            LlmProvider::Anthropic => {
                headers.push(("x-api-key".to_string(), self.config.api_key.clone()));
                if !self.config.api_version.is_empty() {
                    headers.push(("anthropic-version".to_string(), self.config.api_version.clone()));
                }
            }
            LlmProvider::OpenAI => {
                headers.push(("Authorization".to_string(), format!("Bearer {}", self.config.api_key)));
            }
            LlmProvider::Custom => {
                // Use extra_headers for custom provider auth
                for (k, v) in &self.config.extra_headers {
                    headers.push((k.clone(), v.clone()));
                }
            }
            LlmProvider::Ollama => {
                // No auth needed
            }
        }
        headers
    }

    /// Get the full endpoint URL for the configured provider
    fn endpoint_url(&self) -> String {
        format!("{}{}", self.config.base_url, self.config.provider.endpoint())
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

        let url = self.endpoint_url();
        let headers = self.auth_headers();

        let mut request = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&request_body);

        for (key, value) in headers {
            request = request.header(&key, &value);
        }

        let response = request
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

        let reader = response;
        let bytes_stream = reader.bytes_stream();

        let event_stream = bytes_stream.then(|bytes_result| async move {
            let bytes = bytes_result.map_err(|e| ApiError::HttpError(e))?;
            let text = String::from_utf8_lossy(&bytes);

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

        let url = self.endpoint_url();
        let headers = self.auth_headers();

        let mut request = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&request_body);

        for (key, value) in headers {
            request = request.header(&key, &value);
        }

        let response = request
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

    /// Get the configured provider
    pub fn provider(&self) -> &LlmProvider {
        &self.config.provider
    }

    /// Update the model
    pub fn set_model(&mut self, model: String) {
        self.config.model = model;
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.config.base_url
    }

    /// Set a custom base URL (auto-detects provider)
    pub fn set_base_url(&mut self, base_url: String) {
        self.config.provider = LlmProvider::from_base_url(&base_url);
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

    /// Add a custom header (for Custom provider)
    pub fn add_header(&mut self, key: String, value: String) {
        self.config.extra_headers.insert(key, value);
    }

    /// Get a reference to the full config
    pub fn config(&self) -> &LlmClientConfig {
        &self.config
    }
}

/// Backward-compatible alias
pub type ClaudeClient = LlmClient;

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(config.timeout_seconds, 120);
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
        assert_eq!(client.config().extra_headers.get("X-Custom"), Some(&"value".to_string()));
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
        assert!(headers.iter().any(|(k, v)| k == "x-api-key" && v == "sk-ant-test"));
        assert!(headers.iter().any(|(k, v)| k == "anthropic-version" && v == "2023-06-01"));
    }

    #[test]
    fn test_auth_headers_openai() {
        let client = LlmClient::new(LlmClientConfig {
            api_key: "sk-oai-test".to_string(),
            provider: LlmProvider::OpenAI,
            ..Default::default()
        });
        let headers = client.auth_headers();
        assert!(headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer sk-oai-test"));
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
        assert!(headers.iter().any(|(k, v)| k == "X-Auth" && v == "token123"));
    }

    // --- Endpoint URL Tests ---

    #[test]
    fn test_endpoint_url_anthropic() {
        let client = LlmClient::new(LlmClientConfig {
            base_url: "https://api.anthropic.com".to_string(),
            provider: LlmProvider::Anthropic,
            ..Default::default()
        });
        assert_eq!(client.endpoint_url(), "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn test_endpoint_url_openai() {
        let client = LlmClient::new(LlmClientConfig {
            base_url: "https://api.openai.com".to_string(),
            provider: LlmProvider::OpenAI,
            ..Default::default()
        });
        assert_eq!(client.endpoint_url(), "https://api.openai.com/v1/chat/completions");
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
        for provider in &[LlmProvider::Anthropic, LlmProvider::OpenAI, LlmProvider::Ollama, LlmProvider::Custom] {
            let json = serde_json::to_string(provider).unwrap();
            let parsed: LlmProvider = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, provider);
        }
    }
}
