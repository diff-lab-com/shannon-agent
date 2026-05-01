//! API error types

use crate::api::types::LlmProvider;
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

    #[error("Provider error ({provider}): [{error_type}] {message}")]
    ProviderError {
        provider: String,
        error_type: String,
        message: String,
    },
}

impl ApiError {
    /// Parse a provider-specific error response body into a structured
    /// [`ApiError::ProviderError`].
    ///
    /// Recognised formats:
    /// - **Anthropic**: `{ "error": { "type": "...", "message": "..." } }`
    /// - **OpenAI**: `{ "error": { "message": "...", "type": "...", "code": "..." } }`
    /// - **Ollama**: `{ "error": "..." }`
    ///
    /// Falls back to using the raw body as the message when the JSON does not
    /// match any known format.
    pub fn from_provider_response(provider: &LlmProvider, status: u16, body: &str) -> Self {
        let provider_name = provider.to_string();

        // Special-case well-known HTTP status codes regardless of body.
        match status {
            401 => return ApiError::AuthenticationFailed,
            429 => return ApiError::RateLimitExceeded,
            // Server errors: use ApiError variant so the retry system can match
            // on the status code. ProviderError is for client errors with
            // structured provider info.
            500 | 502 | 503 | 504 => {
                return ApiError::ApiError {
                    status,
                    message: body.to_string(),
                };
            }
            _ => {}
        }

        // Try to parse provider-specific JSON.
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(body) {
            match provider {
                LlmProvider::Anthropic | LlmProvider::Custom => {
                    // Anthropic: { "error": { "type": "...", "message": "..." } }
                    if let Some(err_obj) = val.get("error").and_then(|e| e.as_object()) {
                        let error_type = err_obj
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let message = err_obj
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or(body)
                            .to_string();
                        return ApiError::ProviderError {
                            provider: provider_name,
                            error_type,
                            message,
                        };
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
                    // OpenAI-compatible: { "error": { "message": "...", "type": "...", "code": "..." } }
                    if let Some(err_obj) = val.get("error").and_then(|e| e.as_object()) {
                        let error_type = err_obj
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let message = err_obj
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or(body)
                            .to_string();
                        return ApiError::ProviderError {
                            provider: provider_name,
                            error_type,
                            message,
                        };
                    }
                }
                LlmProvider::Ollama => {
                    // Ollama: { "error": "..." }
                    if let Some(msg) = val.get("error").and_then(|v| v.as_str()) {
                        let message = if msg.contains("can't find closing")
                            || msg.contains("unexpected end")
                        {
                            format!(
                                "{msg} — the model generated malformed output. \
                                 Try a different model or simplify the prompt."
                            )
                        } else {
                            msg.to_string()
                        };
                        return ApiError::ProviderError {
                            provider: provider_name,
                            error_type: "ollama_error".to_string(),
                            message,
                        };
                    }
                }
                LlmProvider::Gemini => {
                    // Gemini: { "error": { "code": ..., "message": "...", "status": "..." } }
                    if let Some(err_obj) = val.get("error").and_then(|e| e.as_object()) {
                        let error_type = err_obj
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let message = err_obj
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or(body)
                            .to_string();
                        return ApiError::ProviderError {
                            provider: provider_name,
                            error_type,
                            message,
                        };
                    }
                }
                LlmProvider::Bedrock => {
                    // AWS Bedrock: { "message": "..." } or { "message": "...", "type": "..." }
                    if let Some(msg) = val.get("message").and_then(|v| v.as_str()) {
                        let error_type = val
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("bedrock_error")
                            .to_string();
                        return ApiError::ProviderError {
                            provider: provider_name,
                            error_type,
                            message: msg.to_string(),
                        };
                    }
                }
            }
        }

        // Fallback: use the raw body as the message.
        ApiError::ProviderError {
            provider: provider_name,
            error_type: format!("http_{status}"),
            message: body.to_string(),
        }
    }

    /// Check whether this error is caused by the request exceeding the
    /// model's context window (token overflow).
    pub fn is_token_overflow(&self) -> bool {
        match self {
            ApiError::ApiError { status, message } => {
                if *status != 400 {
                    return false;
                }
                let lower = message.to_lowercase();
                lower.contains("context_length")
                    || lower.contains("context length")
                    || lower.contains("max_tokens")
                    || lower.contains("too many tokens")
                    || lower.contains("token limit")
                    || lower.contains("reduce the length")
                    || lower.contains("input is too long")
                    || lower.contains("maximum context")
            }
            ApiError::ProviderError { message, .. } => {
                let lower = message.to_lowercase();
                lower.contains("context_length")
                    || lower.contains("context length")
                    || lower.contains("too many tokens")
                    || lower.contains("token limit")
                    || lower.contains("reduce the length")
                    || lower.contains("input is too long")
                    || lower.contains("maximum context")
            }
            _ => false,
        }
    }

    /// Return a user-facing suggestion for how to resolve this error.
    pub fn user_suggestion(&self) -> Option<String> {
        if self.is_token_overflow() {
            return Some("The conversation is too long. Try /compact to compress context, or start a new session.".to_string());
        }
        match self {
            ApiError::RateLimitExceeded => {
                Some("Rate limited — the request will be retried automatically. If this persists, consider using a different model.".to_string())
            }
            ApiError::AuthenticationFailed => {
                Some("Authentication failed. Check your API key with /config or set SHANNON_API_KEY.".to_string())
            }
            ApiError::Timeout => {
                Some("Request timed out. Try again, use a smaller model, or reduce context with /compact.".to_string())
            }
            ApiError::ApiError { status, .. } if *status >= 500 => {
                Some("Server error — the request will be retried automatically. If this persists, try switching models with /model.".to_string())
            }
            _ => None,
        }
    }
}
