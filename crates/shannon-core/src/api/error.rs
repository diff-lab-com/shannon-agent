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

/// Check if a message string matches known Ollama malformed-output patterns.
///
/// Shared between `ApiError::is_ollama_malformed_output()` and `RetryPolicy::is_retryable()`
/// so that pattern lists stay in sync.
pub fn is_ollama_malformed_message(message: &str) -> bool {
    let normalized = message.replace('\u{2019}', "'");
    let lower = normalized.to_ascii_lowercase();
    lower.contains("can't find closing")
        || lower.contains("can't closing")
        || lower.contains("closing '}'")
        || lower.contains("unexpected end")
        || lower.contains("malformed")
        || lower.contains("json: cannot unmarshal")
        || lower.contains("invalid json")
        || lower.contains("parse error")
        || lower.contains("unexpected token")
        || lower.contains("looks like object")
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
                | LlmProvider::Moonshot
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
                        let message = msg.to_string();
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

    /// Check if this is a recoverable Ollama malformed-output error.
    ///
    /// Ollama can return these errors either as streaming chunks (handled in
    /// `normalize_ollama_event`) or as HTTP error responses (handled in the
    /// engine).  The check is Unicode-aware: Ollama sometimes uses U+2019
    /// (RIGHT SINGLE QUOTATION MARK) instead of ASCII `'` in error messages.
    pub fn is_ollama_malformed_output(&self) -> bool {
        let message = match self {
            // In-stream chunk errors (ProviderError from normalize_ollama_event)
            ApiError::ProviderError { provider, message, .. } => {
                if provider != "ollama" {
                    return false;
                }
                message
            }
            // HTTP 500 errors (from_provider_response maps server errors to ApiError)
            ApiError::ApiError { status, message } => {
                if *status != 500 {
                    return false;
                }
                message
            }
            _ => return false,
        };
        is_ollama_malformed_message(message)
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
    /// when a model generates malformed output. With status 500, this maps to
    /// ApiError (server error), not ProviderError.
    #[test]
    fn test_ollama_malformed_output_error() {
        let body = r#"{"error":"Value looks like object, but can't find closing '}' symbol"}"#;
        let err = ApiError::from_provider_response(&LlmProvider::Ollama, 500, body);
        match err {
            ApiError::ApiError { status, .. } => {
                assert_eq!(status, 500);
            }
            other => panic!("Expected ApiError for status 500, got {other:?}"),
        }
    }

    /// Ollama malformed output with status 400 should be ProviderError with
    /// the raw Ollama message (no appended suggestion — that's user_suggestion()'s job).
    #[test]
    fn test_ollama_malformed_output_no_duplicate_suggestion() {
        let body = r#"{"error":"Value looks like object, but can't find closing '}' symbol"}"#;
        let err = ApiError::from_provider_response(&LlmProvider::Ollama, 400, body);
        match err {
            ApiError::ProviderError { message, .. } => {
                // The message should be the raw Ollama error only — no appended suggestion
                assert!(message.contains("can't find closing"), "Should contain original error: {message}");
                assert!(!message.contains("Try switching models"), "Should NOT contain suggestion text: {message}");
                assert!(!message.contains("simplify your prompt"), "Should NOT contain suggestion text: {message}");
            }
            other => panic!("Expected ProviderError for status 400, got {other:?}"),
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

    /// Regression: HTTP 500 from Ollama maps to ApiError::ApiError, but
    /// `is_ollama_malformed_output()` must still detect it so the engine can
    /// retry without tools.
    #[test]
    fn test_ollama_malformed_output_http_500_detected() {
        let err = ApiError::ApiError {
            status: 500,
            message: r#"{"error":"Value looks like object, but can't find closing '}' symbol"}"#.to_string(),
        };
        assert!(err.is_ollama_malformed_output(), "HTTP 500 malformed output must be detected for retry");
    }

    #[test]
    fn test_ollama_malformed_output_http_500_not_other_status() {
        let err = ApiError::ApiError {
            status: 400,
            message: r#"{"error":"can't find closing"}"#.to_string(),
        };
        assert!(!err.is_ollama_malformed_output(), "Status 400 should not match");
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

    // ── Regression: error message formatting ────────────────────────────

    /// Verifies that combining error display + user_suggestion produces no
    /// double periods (e.g. "prompt..") or duplicated content.
    /// This mirrors the format used in engine.rs:
    ///   format!("{e}.{suggestion}")
    #[test]
    fn test_error_plus_suggestion_no_double_period() {
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "Value looks like object, but can't find closing '}' symbol".to_string(),
        };
        let suggestion = err.user_suggestion()
            .map(|s| format!(" {s}"))
            .unwrap_or_default();
        let combined = format!("{err}.{suggestion}");

        // No double periods
        assert!(!combined.contains(".."), "Should not contain double periods: {combined}");
        // No triple periods either (ellipsis is fine)
        // The error and suggestion should both be present
        assert!(combined.contains("can't find closing"), "Should contain error: {combined}");
        assert!(combined.contains("/model"), "Should contain suggestion: {combined}");
    }

    /// All user_suggestion() returns must end with a period — the engine.rs
    /// format string does NOT add one, so the suggestion must be self-contained.
    #[test]
    fn test_all_user_suggestions_end_with_period() {
        let cases: Vec<ApiError> = vec![
            ApiError::RateLimitExceeded { retry_after_secs: None },
            ApiError::AuthenticationFailed,
            ApiError::Timeout,
            ApiError::ApiError { status: 500, message: "server error".to_string() },
            ApiError::ProviderError {
                provider: "ollama".to_string(),
                error_type: "ollama_error".to_string(),
                message: "can't find closing '}' symbol".to_string(),
            },
        ];
        for err in cases {
            if let Some(s) = err.user_suggestion() {
                assert!(s.ends_with('.'), "user_suggestion for {err:?} must end with period: \"{s}\"");
            }
        }
    }

    /// Error message + suggestion must not duplicate content between the two.
    #[test]
    fn test_error_suggestion_no_content_duplication() {
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "Value looks like object, but can't find closing '}' symbol".to_string(),
        };
        let display = format!("{err}");
        let suggestion = err.user_suggestion();

        // The display message should NOT contain the suggestion text
        if let Some(ref _s) = suggestion {
            // Check key phrases from the suggestion don't appear in the error display
            assert!(
                !display.contains("Try switching models"),
                "Error display should not duplicate suggestion: {display}"
            );
            assert!(
                !display.contains("simplify your prompt"),
                "Error display should not duplicate suggestion: {display}"
            );
        }

        // The suggestion itself should be present and meaningful
        assert!(suggestion.is_some());
        let s = suggestion.unwrap();
        assert!(s.len() > 20, "Suggestion should be meaningful, got: {s}");
    }
}
