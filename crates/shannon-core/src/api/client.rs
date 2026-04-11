//! LLM API client with multi-provider and streaming support.

use reqwest::Client;
use std::time::Duration;

use super::error::ApiError;
use super::streaming::MessageStream;
use super::types::*;

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
    /// Checks `SHANNON_API_KEY` -> `ANTHROPIC_API_KEY` -> `OPENAI_API_KEY`
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
    pub(crate) fn auth_headers(&self) -> Vec<(String, String)> {
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
    pub(crate) fn endpoint_url(&self) -> String {
        format!("{}{}", self.config.base_url, self.config.provider.endpoint())
    }

    /// Send a message with streaming response (SSE)
    pub async fn send_message_stream(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        system: Option<String>,
    ) -> Result<MessageStream, ApiError> {
        let request_body = MessageRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            system,
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
            .json(&super::adapter::serialize_request(&request_body, &self.config.provider));

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

        Ok(super::streaming::sse_stream_from_response(response, self.config.provider.clone()))
    }

    /// Send a message and wait for complete response (non-streaming)
    pub async fn send_message(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        system: Option<String>,
    ) -> Result<Vec<ContentBlock>, ApiError> {
        let request_body = MessageRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            system,
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
            .json(&super::adapter::serialize_request(&request_body, &self.config.provider));

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
