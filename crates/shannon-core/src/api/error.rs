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
    RateLimitExceeded { retry_after_secs: Option<u64> },

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

    #[error("Provider error ({provider}): {error_type} — {message}")]
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
            429 => return ApiError::RateLimitExceeded { retry_after_secs: None },
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
            ApiError::RateLimitExceeded { .. } => {
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
            ApiError::ProviderError { message, .. } if message.contains("can't find closing") || message.contains("malformed output") => {
                Some("The model generated invalid output. Try switching models with /model, or simplify your prompt.".to_string())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Error display format tests ──────────────────────────────────────

    /// Regression: ProviderError display must NOT wrap error_type in brackets
    /// because `[ollama_error]` renders as separate visual chunks in the TUI
    /// when the terminal wraps lines.
    #[test]
    fn test_provider_error_display_no_brackets() {
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "Value looks like object, but can't find closing '}' symbol".to_string(),
        };
        let display = format!("{err}");
        assert!(
            !display.contains("[ollama_error]"),
            "ProviderError display should not wrap error_type in brackets: {display}"
        );
        assert!(
            display.contains("Provider error (ollama)"),
            "Should contain provider name: {display}"
        );
        assert!(
            display.contains("ollama_error"),
            "Should contain error_type: {display}"
        );
        assert!(
            display.contains("can't find closing"),
            "Should contain message: {display}"
        );
    }

    #[test]
    fn test_provider_error_display_openai() {
        let err = ApiError::ProviderError {
            provider: "openai".to_string(),
            error_type: "invalid_request_error".to_string(),
            message: "max_tokens is required".to_string(),
        };
        let display = format!("{err}");
        assert!(display.contains("Provider error (openai)"), "{display}");
        assert!(display.contains("invalid_request_error"), "{display}");
        assert!(!display.contains("[invalid_request_error]"), "No brackets: {display}");
    }

    // ── Ollama error parsing tests ──────────────────────────────────────

    /// Regression: Ollama returns `{"error": "can't find closing '}' symbol..."}`
    /// when a model generates malformed output. Must be parsed correctly.
    #[test]
    fn test_ollama_malformed_output_error() {
        let body = r#"{"error":"Value looks like object, but can't find closing '}' symbol"}"#;
        let err = ApiError::from_provider_response(&LlmProvider::Ollama, 500, body);
        match err {
            ApiError::ProviderError { provider, error_type, message } => {
                assert_eq!(provider, "ollama");
                assert_eq!(error_type, "ollama_error");
                assert!(message.contains("can't find closing"), "Should contain original error: {message}");
                assert!(message.contains("malformed output"), "Should add helpful context: {message}");
            }
            ApiError::ApiError { status, .. } => {
                // 500 gets mapped to ApiError, not ProviderError
                assert_eq!(status, 500);
            }
            other => panic!("Expected ProviderError or ApiError, got {other:?}"),
        }
    }

    /// Ollama malformed output with status 400 (not 500) should be ProviderError.
    #[test]
    fn test_ollama_malformed_output_status_400() {
        let body = r#"{"error":"json: cannot unmarshal"}"#;
        let err = ApiError::from_provider_response(&LlmProvider::Ollama, 400, body);
        match err {
            ApiError::ProviderError { provider, error_type, message } => {
                assert_eq!(provider, "ollama");
                assert_eq!(error_type, "ollama_error");
                assert!(message.contains("json: cannot unmarshal"), "{message}");
            }
            other => panic!("Expected ProviderError, got {other:?}"),
        }
    }

    #[test]
    fn test_ollama_generic_error() {
        let body = r#"{"error":"model not found"}"#;
        let err = ApiError::from_provider_response(&LlmProvider::Ollama, 404, body);
        match err {
            ApiError::ProviderError { message, .. } => {
                assert_eq!(message, "model not found");
            }
            other => panic!("Expected ProviderError, got {other:?}"),
        }
    }

    // ── Token overflow detection ────────────────────────────────────────

    #[test]
    fn test_is_token_overflow_provider_error() {
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "context length exceeded".to_string(),
        };
        assert!(err.is_token_overflow());
    }

    #[test]
    fn test_is_not_token_overflow_malformed() {
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "can't find closing '}' symbol".to_string(),
        };
        assert!(!err.is_token_overflow(), "Malformed output is NOT a token overflow");
    }

    #[test]
    fn test_user_suggestion_malformed_output() {
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "Value looks like object, but can't find closing '}' symbol".to_string(),
        };
        let suggestion = err.user_suggestion();
        assert!(suggestion.is_some(), "Should have a suggestion for malformed output");
        let s = suggestion.unwrap();
        assert!(s.contains("/model"), "Should suggest /model: {s}");
    }

    #[test]
    fn test_user_suggestion_generic_provider_error() {
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "model not found".to_string(),
        };
        assert!(err.user_suggestion().is_none(), "Generic ProviderError should have no suggestion");
    }
}
