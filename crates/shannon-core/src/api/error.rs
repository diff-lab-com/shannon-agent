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
                LlmProvider::OpenAI => {
                    // OpenAI: { "error": { "message": "...", "type": "...", "code": "..." } }
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
                        return ApiError::ProviderError {
                            provider: provider_name,
                            error_type: "ollama_error".to_string(),
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
}
