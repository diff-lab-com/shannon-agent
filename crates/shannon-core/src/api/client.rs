//! LLM API client with multi-provider and streaming support.

use reqwest::Client;
use std::time::Duration;

use super::error::ApiError;
use super::retry::retry_request;
use super::streaming::MessageStream;
use super::types::*;

/// LLM API client with multi-provider and streaming support
#[derive(Clone)]
pub struct LlmClient {
    config: LlmClientConfig,
    client: Client,
}

impl LlmClient {
    /// Build a reqwest client with the given timeout (seconds).
    ///
    /// Falls back to a default client if TLS initialization fails,
    /// logging the error instead of panicking.
    fn build_client(timeout_secs: u64) -> Client {
        Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("Failed to build HTTP client with timeout ({timeout_secs}s): {e}; falling back to default");
                Client::new()
            })
    }

    /// Create a new LLM API client
    pub fn new(config: LlmClientConfig) -> Self {
        let client = Self::build_client(config.timeout_seconds);

        Self { config, client }
    }

    /// Create a new LLM API client, returning an error if client construction fails.
    pub fn try_new(config: LlmClientConfig) -> Result<Self, ApiError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .map_err(|e| ApiError::InvalidResponse(format!("Failed to create HTTP client: {e}")))?;
        Ok(Self { config, client })
    }

    /// Create client from environment variables.
    ///
    /// Checks `SHANNON_API_KEY` -> `ANTHROPIC_API_KEY` -> `OPENAI_API_KEY`
    /// and auto-detects provider from base URL. Falls back to Ollama if no
    /// API keys are found.
    pub fn from_env() -> Self {
        let config = LlmClientConfig::default();

        // Validate configuration (will catch missing API keys for auth-required providers)
        if let Err(e) = config.validate() {
            tracing::warn!("LLM config issue: {}", e);
        }

        if config.provider.requires_auth() {
            Self::new(config)
        } else {
            Self::new_unauthenticated(config)
        }
    }

    /// Create a client that requires no authentication (e.g., Ollama)
    pub fn new_unauthenticated(config: LlmClientConfig) -> Self {
        let client = Self::build_client(config.timeout_seconds);

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
            LlmProvider::OpenAI
            | LlmProvider::Azure
            | LlmProvider::Mistral
            | LlmProvider::DeepSeek
            | LlmProvider::Groq
            | LlmProvider::Together
            | LlmProvider::OpenRouter
            | LlmProvider::Cohere
            | LlmProvider::Fireworks
            | LlmProvider::Perplexity
            | LlmProvider::Xai
            | LlmProvider::Ai21
            | LlmProvider::SiliconFlow
            | LlmProvider::Zhipu
            | LlmProvider::Cloudflare
            | LlmProvider::Replicate => {
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
            LlmProvider::Gemini => {
                // Gemini uses API key as query parameter, handled in endpoint URL construction.
                // However, also set as header for some endpoint styles.
                headers.push(("x-goog-api-key".to_string(), self.config.api_key.clone()));
            }
            LlmProvider::Bedrock => {
                // Bedrock uses AWS SigV4 auth; for now use Bearer token via extra_headers
                // or the API key as a session token. Full SigV4 signing would require
                // an AWS SDK dependency — this supports API-key-based access patterns.
                for (k, v) in &self.config.extra_headers {
                    headers.push((k.clone(), v.clone()));
                }
                if !self.config.api_key.is_empty() && self.config.extra_headers.is_empty() {
                    headers.push(("Authorization".to_string(), format!("Bearer {}", self.config.api_key)));
                }
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
        let max_reconnects = self.config.max_stream_reconnects;

        // Clone upfront for potential reconnection use
        let messages_clone = messages.clone();
        let tools_clone = tools.clone();
        let system_clone = system.clone();

        let request_body = MessageRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            system,
            system_blocks: None,
            messages,
            tools,
            stream: Some(true),
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            budget_tokens: self.config.budget_tokens,
            thinking_budget: None,
            reasoning_effort: self.config.reasoning_effort,
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
                Some(reqwest::StatusCode::TOO_MANY_REQUESTS) => ApiError::RateLimitExceeded { retry_after_secs: None },
                Some(status) => ApiError::ApiError {
                    status: status.as_u16(),
                    message: format!("HTTP error: {e}"),
                },
                None => ApiError::HttpError(e),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            if status == 429 {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok());
                let _ = response.text().await;
                return Err(ApiError::RateLimitExceeded { retry_after_secs: retry_after });
            }
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiError::from_provider_response(
                &self.config.provider,
                status,
                &error_text,
            ));
        }

        if max_reconnects > 0 {
            Ok(super::streaming::sse_stream_from_response_resumable(
                response,
                self.config.provider.clone(),
                Self::new(self.config.clone()),
                messages_clone,
                tools_clone,
                system_clone,
                max_reconnects,
            ))
        } else {
            Ok(super::streaming::sse_stream_from_response(response, self.config.provider.clone()))
        }
    }

    /// Send a message with streaming response using structured system blocks.
    ///
    /// When available, this enables prompt caching by sending the system prompt
    /// as an array of typed content blocks with cache breakpoints.
    pub async fn send_message_stream_structured(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        system_blocks: Vec<super::types::SystemContentBlock>,
    ) -> Result<MessageStream, ApiError> {
        let request_body = MessageRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            system: None,
            system_blocks: Some(system_blocks),
            messages,
            tools,
            stream: Some(true),
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            budget_tokens: self.config.budget_tokens,
            thinking_budget: None,
            reasoning_effort: self.config.reasoning_effort,
        };

        let url = self.endpoint_url();
        let headers = self.auth_headers();

        let mut request = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .header("anthropic-version", "2023-06-01");

        for (k, v) in &headers {
            request = request.header(k.as_str(), v.as_str());
        }

        let body = super::adapter::serialize_request(&request_body, &self.config.provider);
        request = request.json(&body);

        let response = request
            .send()
            .await
            .map_err(|e| match e.status() {
                Some(reqwest::StatusCode::UNAUTHORIZED) => ApiError::AuthenticationFailed,
                Some(reqwest::StatusCode::TOO_MANY_REQUESTS) => ApiError::RateLimitExceeded { retry_after_secs: None },
                Some(status) => ApiError::ApiError {
                    status: status.as_u16(),
                    message: format!("HTTP error: {e}"),
                },
                None => ApiError::HttpError(e),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            if status == 429 {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok());
                let _ = response.text().await;
                return Err(ApiError::RateLimitExceeded { retry_after_secs: retry_after });
            }
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiError::from_provider_response(
                &self.config.provider,
                status,
                &error_text,
            ));
        }

        Ok(super::streaming::sse_stream_from_response(response, self.config.provider.clone()))
    }

    /// Send a streaming message with optional resumption via `Last-Event-ID`.
    ///
    /// If `last_event_id` is `Some`, the `Last-Event-ID` header is added to
    /// the request so that providers that support SSE resumption can replay
    /// events after the given ID.
    pub async fn send_message_stream_resumable(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        system: Option<String>,
        last_event_id: Option<String>,
    ) -> Result<MessageStream, ApiError> {
        let request_body = MessageRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            system,
            system_blocks: None,
            messages,
            tools,
            stream: Some(true),
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            budget_tokens: self.config.budget_tokens,
            thinking_budget: None,
            reasoning_effort: None,
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

        if let Some(ref eid) = last_event_id {
            request = request.header("Last-Event-ID", eid.as_str());
        }

        let response = request
            .send()
            .await
            .map_err(|e| match e.status() {
                Some(reqwest::StatusCode::UNAUTHORIZED) => ApiError::AuthenticationFailed,
                Some(reqwest::StatusCode::TOO_MANY_REQUESTS) => ApiError::RateLimitExceeded { retry_after_secs: None },
                Some(status) => ApiError::ApiError {
                    status: status.as_u16(),
                    message: format!("HTTP error: {e}"),
                },
                None => ApiError::HttpError(e),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            if status == 429 {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok());
                let _ = response.text().await;
                return Err(ApiError::RateLimitExceeded { retry_after_secs: retry_after });
            }
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiError::from_provider_response(
                &self.config.provider,
                status,
                &error_text,
            ));
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
            system_blocks: None,
            messages,
            tools,
            stream: Some(false),
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            budget_tokens: self.config.budget_tokens,
            thinking_budget: None,
            reasoning_effort: None,
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
                Some(reqwest::StatusCode::TOO_MANY_REQUESTS) => ApiError::RateLimitExceeded { retry_after_secs: None },
                Some(status) => ApiError::ApiError {
                    status: status.as_u16(),
                    message: format!("HTTP error: {e}"),
                },
                None => ApiError::HttpError(e),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            if status == 429 {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok());
                let _ = response.text().await;
                return Err(ApiError::RateLimitExceeded { retry_after_secs: retry_after });
            }
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiError::from_provider_response(
                &self.config.provider,
                status,
                &error_text,
            ));
        }

        // Read the raw text first so we can apply provider-specific normalization
        let body = response.text().await.map_err(|e| {
            ApiError::InvalidResponse(format!("Failed to read response body: {e}"))
        })?;

        let api_response = super::adapter::normalize_response(&body, &self.config.provider)?;

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

    /// Update the model AND switch provider (including base_url).
    ///
    /// This is the correct method to call when the user selects a model from
    /// a different provider (e.g. picking an Ollama model while the client was
    /// configured for Anthropic).
    pub fn set_model_for_provider(&mut self, model: String, provider: LlmProvider) {
        let base_url = provider.default_base_url().to_string();
        self.config.model = model;
        self.config.provider = provider;
        self.config.base_url = base_url;
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

    /// Send a message with retry logic and optional provider fallback.
    pub async fn send_message_with_retry(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        system: Option<String>,
    ) -> Result<Vec<ContentBlock>, ApiError> {
        let retry_config = &self.config.retry_config;
        let result = retry_request(retry_config, || {
            self.send_message(messages.clone(), tools.clone(), system.clone())
        })
        .await;

        match result {
            Ok(blocks) => Ok(blocks),
            Err(primary_err) => {
                // Try fallback provider if configured
                if let (Some(fallback_provider), Some(fallback_base_url)) =
                    (&self.config.fallback_provider, &self.config.fallback_base_url)
                {
                    tracing::warn!(
                        "Primary provider {} failed: {}. Falling back to {} at {}",
                        self.config.provider,
                        primary_err,
                        fallback_provider,
                        fallback_base_url,
                    );
                    let mut fallback_config = self.config.clone();
                    fallback_config.provider = fallback_provider.clone();
                    fallback_config.base_url = fallback_base_url.clone();
                    // Inherit retry config
                    let fallback_retry = fallback_config.retry_config.clone();
                    let fallback_client = Self::new(fallback_config);
                    retry_request(&fallback_retry, || {
                        fallback_client
                            .send_message(messages.clone(), tools.clone(), system.clone())
                    })
                    .await
                } else {
                    Err(primary_err)
                }
            }
        }
    }

    /// Send a streaming message with retry logic and optional provider fallback.
    pub async fn send_message_stream_with_retry(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        system: Option<String>,
    ) -> Result<MessageStream, ApiError> {
        let retry_config = &self.config.retry_config;
        let result = retry_request(retry_config, || {
            self.send_message_stream(messages.clone(), tools.clone(), system.clone())
        })
        .await;

        match result {
            Ok(stream) => Ok(stream),
            Err(primary_err) => {
                if let (Some(fallback_provider), Some(fallback_base_url)) =
                    (&self.config.fallback_provider, &self.config.fallback_base_url)
                {
                    tracing::warn!(
                        "Primary provider {} stream failed: {}. Falling back to {} at {}",
                        self.config.provider,
                        primary_err,
                        fallback_provider,
                        fallback_base_url,
                    );
                    let mut fallback_config = self.config.clone();
                    fallback_config.provider = fallback_provider.clone();
                    fallback_config.base_url = fallback_base_url.clone();
                    let fallback_retry = fallback_config.retry_config.clone();
                    let fallback_client = Self::new(fallback_config);
                    retry_request(&fallback_retry, || {
                        fallback_client
                            .send_message_stream(messages.clone(), tools.clone(), system.clone())
                    })
                    .await
                } else {
                    Err(primary_err)
                }
            }
        }
    }

    /// Send a structured streaming message with retry logic and optional provider fallback.
    pub async fn send_message_stream_structured_with_retry(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        system_blocks: Vec<super::types::SystemContentBlock>,
    ) -> Result<MessageStream, ApiError> {
        let retry_config = &self.config.retry_config;
        let result = retry_request(retry_config, || {
            self.send_message_stream_structured(messages.clone(), tools.clone(), system_blocks.clone())
        })
        .await;

        match result {
            Ok(stream) => Ok(stream),
            Err(primary_err) => {
                if let (Some(fallback_provider), Some(fallback_base_url)) =
                    (&self.config.fallback_provider, &self.config.fallback_base_url)
                {
                    tracing::warn!(
                        "Primary provider {} structured stream failed: {}. Falling back to {} at {}",
                        self.config.provider,
                        primary_err,
                        fallback_provider,
                        fallback_base_url,
                    );
                    let mut fallback_config = self.config.clone();
                    fallback_config.provider = fallback_provider.clone();
                    fallback_config.base_url = fallback_base_url.clone();
                    let fallback_retry = fallback_config.retry_config.clone();
                    let fallback_client = Self::new(fallback_config);
                    retry_request(&fallback_retry, || {
                        fallback_client
                            .send_message_stream_structured(messages.clone(), tools.clone(), system_blocks.clone())
                    })
                    .await
                } else {
                    Err(primary_err)
                }
            }
        }
    }
}

/// Backward-compatible alias
pub type ClaudeClient = LlmClient;
