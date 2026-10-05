//! API error types

use crate::api::types::LlmProvider;
use thiserror::Error;

/// Canonical `error_kind` values carried on structured failure payloads
/// (S1-1, review 2026-10-05 §3 P-N1). The engine classifies failures HERE —
/// where the typed error and its HTTP status are in scope — and shells
/// consume the kind verbatim; nobody re-derives it from message text.
///
/// Classification maps **typed status only** (`ApiError::error_kind`);
/// message sniffing is forbidden by contract. Values are stable wire
/// strings: desktop UI routes each kind to a dedicated recovery banner.
pub mod error_kind {
    /// HTTP 401 — bad/revoked API key.
    pub const AUTH: &str = "auth";
    /// HTTP 402 — provider quota/credit exhausted.
    pub const QUOTA: &str = "quota";
    /// HTTP 429 — rate limited.
    pub const RATE_LIMIT: &str = "rate_limit";
    /// HTTP 403 — key valid but lacks permission for the model/resource.
    pub const AUTHZ: &str = "authz";
    /// Everything else (network, timeout, 5xx, engine bugs, …).
    pub const OTHER: &str = "other";
}

/// Map an HTTP status to its canonical [`error_kind`]. Kept status-based on
/// purpose: 401→auth, 402→quota, 403→authz, 429→rate_limit, rest→other
/// (5xx/network/timeout intentionally land in `other` — they are transient,
/// not user-fixable configurations).
fn error_kind_for_status(status: u16) -> &'static str {
    match status {
        401 => error_kind::AUTH,
        402 => error_kind::QUOTA,
        403 => error_kind::AUTHZ,
        429 => error_kind::RATE_LIMIT,
        _ => error_kind::OTHER,
    }
}

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

    /// Provider error with structured provider info. `status` (S1-1, P-N1)
    /// preserves the HTTP status that `from_provider_response` consumed —
    /// without it the status was dropped the moment a provider answered
    /// 402/403 with a structured JSON body (e.g. DeepSeek's "Insufficient
    /// Balance"), which is exactly the type information the desktop banner
    /// routing needs. `None` for errors parsed outside an HTTP exchange
    /// (in-stream Ollama error fields, adapter probes).
    #[error("Provider error ({provider}): {error_type} — {message}")]
    ProviderError {
        provider: String,
        error_type: String,
        message: String,
        /// HTTP status the provider answered with, when this error came
        /// from an HTTP exchange (`from_provider_response` fills it);
        /// `None` for errors parsed outside an HTTP exchange.
        status: Option<u16>,
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
            429 => {
                return ApiError::RateLimitExceeded {
                    retry_after_secs: None,
                };
            }
            // Server errors: use ApiError variant so the retry system can match
            // on the status code. ProviderError is for client errors with
            // structured provider info. 529 is Anthropic's `overloaded_error`
            // and is transient — it must land in the retryable class too.
            500 | 502 | 503 | 504 | 529 => {
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
                LlmProvider::Anthropic | LlmProvider::Custom | LlmProvider::ZhipuCoding => {
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
                            status: Some(status),
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
                | LlmProvider::ZhipuInternational
                | LlmProvider::ZhipuCodingPlan
                | LlmProvider::Moonshot
                | LlmProvider::Minimax
                | LlmProvider::DashScope
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
                            status: Some(status),
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
                            status: Some(status),
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
                            status: Some(status),
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
                            status: Some(status),
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
            status: Some(status),
        }
    }

    /// S1-1 (review 2026-10-05 §3 P-N1): classify this error into the
    /// canonical [`error_kind`] wire value, **from typed data only** —
    /// variant + status. No message sniffing: the engine is the single
    /// classification source, and shells consume the kind verbatim instead
    /// of re-parsing `Display` text (the fragile chain this replaces).
    ///
    /// 401→auth, 402→quota, 403→authz, 429→rate_limit; timeouts, network
    /// failures, 5xx and everything else→other (transient classes the user
    /// cannot fix by reconfiguring, so they keep the plain error banner).
    pub fn error_kind(&self) -> &'static str {
        match self {
            ApiError::AuthenticationFailed => error_kind::AUTH,
            ApiError::RateLimitExceeded { .. } => error_kind::RATE_LIMIT,
            ApiError::ApiError { status, .. } => error_kind_for_status(*status),
            // `status` is filled by `from_provider_response` for every
            // HTTP-derived provider error — including 402/403 bodies that
            // parse into provider-specific shapes (quota/authz must survive
            // that parse). `None` = never saw an HTTP status → other.
            ApiError::ProviderError { status, .. } => status
                .map(error_kind_for_status)
                .unwrap_or(error_kind::OTHER),
            _ => error_kind::OTHER,
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
            ApiError::ProviderError {
                provider, message, ..
            } => {
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

    /// Check whether this error belongs to the timeout / transient-transport
    /// class that the headless runner classifies as `Timeout`
    /// (`shannon-cli` `classify_headless_failure`: "timed out" / "timeout" /
    /// "error sending request").
    ///
    /// A8 (turn-level stream-death continuation, DeepSWE smoke-2/4 RCA —
    /// docs/research/pier-adapter-notes-2026-09.md §五): the engine's turn
    /// loop uses this to decide whether a dead LLM call may be retried in
    /// place instead of failing the whole run. The words are matched against
    /// this error's **Display string** — the exact string that ends up in
    /// `QueryEvent::Failed` and that the CLI classifier sees — so the two
    /// classifications cannot drift.
    pub fn is_timeout_class(&self) -> bool {
        let lower = self.to_string().to_lowercase();
        lower.contains("timed out")
            || lower.contains("timeout")
            || lower.contains("error sending request")
    }

    /// Check whether this error reports an abnormally terminated response
    /// stream (A8b): the byte stream ended before the provider's terminal
    /// frame (smoke-5: mid-work truncated generation committed as a complete
    /// answer — docs/research/pier-adapter-notes-2026-09.md §五 evidence
    /// trail, `~/.shannon/eval/deepswe-smoke/jobs/deepswe-smoke-5/`).
    ///
    /// Strictly a **type-level** match on [`ApiError::StreamEndedUnexpectedly`]
    /// — deliberately NOT string matching. A stream that ends this way is
    /// never a legitimate completion (clean completions always carry a
    /// terminal frame), so only the variant itself qualifies; any other
    /// error whose Display happens to contain similar words stays out.
    pub fn is_stream_interrupted(&self) -> bool {
        matches!(self, ApiError::StreamEndedUnexpectedly)
    }

    /// Provider-aware guidance for authentication failures (review
    /// 2026-09-29 P0-3): `/config` cannot set keys — `/connect` is the
    /// surface that stores them — and each provider has its own canonical
    /// env var ([`LlmProvider::canonical_api_key_env`]).
    ///
    /// Callers that know the active provider should prefer this over the
    /// provider-agnostic text produced by [`Self::user_suggestion`] for the
    /// bare [`ApiError::AuthenticationFailed`] variant (which, as a unit
    /// variant, carries no provider).
    pub fn auth_failure_suggestion(provider: &LlmProvider) -> String {
        let slug = provider.to_string();
        match provider.canonical_api_key_env() {
            Some(env) => format!(
                "Authentication failed for {slug}. Update the key with /connect {slug} <new-key>, or set {env}."
            ),
            None => format!(
                "Authentication failed for {slug}. Update the key with /connect {slug} <new-key>."
            ),
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
                // The unit variant carries no provider, so the hint stays
                // generic; use [`Self::auth_failure_suggestion`] where the
                // active provider is known. `/config` cannot set keys —
                // `/connect` is the surface that stores them.
                Some("Authentication failed. Update the key with /connect <provider> <new-key>, or set the provider's API key environment variable.".to_string())
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
#[allow(clippy::unwrap_used)]
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
            status: None,
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
            status: None,
        };
        let display = format!("{err}");
        assert!(display.contains("Provider error (openai)"), "{display}");
        assert!(display.contains("invalid_request_error"), "{display}");
        assert!(
            !display.contains("[invalid_request_error]"),
            "No brackets: {display}"
        );
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
                assert!(
                    message.contains("can't find closing"),
                    "Should contain original error: {message}"
                );
                assert!(
                    !message.contains("Try switching models"),
                    "Should NOT contain suggestion text: {message}"
                );
                assert!(
                    !message.contains("simplify your prompt"),
                    "Should NOT contain suggestion text: {message}"
                );
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
            ApiError::ProviderError {
                provider,
                error_type,
                message,
                ..
            } => {
                assert_eq!(provider, "ollama");
                assert_eq!(error_type, "ollama_error");
                assert!(message.contains("json: cannot unmarshal"), "{message}");
            }
            other => panic!("Expected ProviderError, got {other:?}"),
        }
    }

    /// Anthropic's HTTP 529 (overloaded_error) must land in the retryable
    /// `ApiError` class — same as 500/502/503/504 — not the non-retryable
    /// `ProviderError` class, so the retry system sees the status code.
    #[test]
    fn test_529_overloaded_maps_to_retryable_api_error() {
        let body = r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        let err = ApiError::from_provider_response(&LlmProvider::Anthropic, 529, body);
        match &err {
            ApiError::ApiError { status, .. } => assert_eq!(*status, 529),
            other => panic!("Expected ApiError for status 529, got {other:?}"),
        }
        // The classification must be consistent with the retry policy.
        assert!(
            crate::api::retry::RetryConfig::default().is_retryable(&err),
            "529 must classify as retryable"
        );
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
            status: None,
        };
        assert!(err.is_token_overflow());
    }

    #[test]
    fn test_is_not_token_overflow_malformed() {
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "can't find closing '}' symbol".to_string(),
            status: None,
        };
        assert!(
            !err.is_token_overflow(),
            "Malformed output is NOT a token overflow"
        );
    }

    /// Regression: HTTP 500 from Ollama maps to ApiError::ApiError, but
    /// `is_ollama_malformed_output()` must still detect it so the engine can
    /// retry without tools.
    #[test]
    fn test_ollama_malformed_output_http_500_detected() {
        let err = ApiError::ApiError {
            status: 500,
            message: r#"{"error":"Value looks like object, but can't find closing '}' symbol"}"#
                .to_string(),
        };
        assert!(
            err.is_ollama_malformed_output(),
            "HTTP 500 malformed output must be detected for retry"
        );
    }

    #[test]
    fn test_ollama_malformed_output_http_500_not_other_status() {
        let err = ApiError::ApiError {
            status: 400,
            message: r#"{"error":"can't find closing"}"#.to_string(),
        };
        assert!(
            !err.is_ollama_malformed_output(),
            "Status 400 should not match"
        );
    }

    #[test]
    fn test_user_suggestion_malformed_output() {
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "Value looks like object, but can't find closing '}' symbol".to_string(),
            status: None,
        };
        let suggestion = err.user_suggestion();
        assert!(
            suggestion.is_some(),
            "Should have a suggestion for malformed output"
        );
        let s = suggestion.unwrap();
        assert!(s.contains("/model"), "Should suggest /model: {s}");
    }

    #[test]
    fn test_user_suggestion_generic_provider_error() {
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "model not found".to_string(),
            status: None,
        };
        assert!(
            err.user_suggestion().is_none(),
            "Generic ProviderError should have no suggestion"
        );
    }

    // ── Auth-failure suggestion routing (review 2026-09-29 P0-3) ────────

    /// Regression: the 401 suggestion used to point at `/config`, which
    /// cannot set keys. It must route to `/connect` — the surface that
    /// stores credentials.
    #[test]
    fn test_auth_suggestion_routes_to_connect_not_config() {
        let suggestion = ApiError::AuthenticationFailed
            .user_suggestion()
            .expect("AuthenticationFailed must have a suggestion");
        assert!(
            suggestion.contains("/connect"),
            "must point at /connect: {suggestion}"
        );
        assert!(
            !suggestion.contains("/config"),
            "must not point at /config: {suggestion}"
        );
    }

    /// The provider-aware helper names the provider slug and its canonical
    /// env var.
    #[test]
    fn test_auth_failure_suggestion_names_provider_and_env() {
        let s = ApiError::auth_failure_suggestion(&LlmProvider::Anthropic);
        assert!(
            s.starts_with("Authentication failed for anthropic."),
            "must name the provider: {s}"
        );
        assert!(
            s.contains("/connect anthropic <new-key>"),
            "must give the exact /connect invocation: {s}"
        );
        assert!(
            s.contains("ANTHROPIC_API_KEY"),
            "must name the canonical env var: {s}"
        );
        assert!(s.ends_with('.'), "must end with a period: {s}");

        let s = ApiError::auth_failure_suggestion(&LlmProvider::Zhipu);
        assert!(s.contains("/connect zhipu"), "{s}");
        assert!(s.contains("ZHIPU_API_KEY"), "{s}");
    }

    /// Providers without a canonical env var still get the /connect route,
    /// just without the "set {ENV_VAR}" tail.
    #[test]
    fn test_auth_failure_suggestion_without_canonical_env() {
        for provider in [LlmProvider::Custom, LlmProvider::Ollama] {
            let s = ApiError::auth_failure_suggestion(&provider);
            assert!(
                s.contains(&format!("/connect {provider} <new-key>")),
                "must still route to /connect: {s}"
            );
            assert!(
                !s.contains("or set"),
                "no env var to name, so no 'or set' tail: {s}"
            );
            assert!(s.ends_with('.'), "{s}");
        }
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
            status: None,
        };
        let suggestion = err
            .user_suggestion()
            .map(|s| format!(" {s}"))
            .unwrap_or_default();
        let combined = format!("{err}.{suggestion}");

        // No double periods
        assert!(
            !combined.contains(".."),
            "Should not contain double periods: {combined}"
        );
        // No triple periods either (ellipsis is fine)
        // The error and suggestion should both be present
        assert!(
            combined.contains("can't find closing"),
            "Should contain error: {combined}"
        );
        assert!(
            combined.contains("/model"),
            "Should contain suggestion: {combined}"
        );
    }

    /// All user_suggestion() returns must end with a period — the engine.rs
    /// format string does NOT add one, so the suggestion must be self-contained.
    #[test]
    fn test_all_user_suggestions_end_with_period() {
        let cases: Vec<ApiError> = vec![
            ApiError::RateLimitExceeded {
                retry_after_secs: None,
            },
            ApiError::AuthenticationFailed,
            ApiError::Timeout,
            ApiError::ApiError {
                status: 500,
                message: "server error".to_string(),
            },
            ApiError::ProviderError {
                provider: "ollama".to_string(),
                error_type: "ollama_error".to_string(),
                message: "can't find closing '}' symbol".to_string(),
                status: None,
            },
        ];
        for err in cases {
            if let Some(s) = err.user_suggestion() {
                assert!(
                    s.ends_with('.'),
                    "user_suggestion for {err:?} must end with period: \"{s}\""
                );
            }
        }
    }

    // ── S1-1 / P-N1: structured error_kind classification ───────────────

    /// The canonical mapping: typed variant/status → wire kind. This is the
    /// single classification source the desktop consumes; there must be no
    /// text matching anywhere in this path.
    #[test]
    fn test_error_kind_maps_typed_statuses() {
        // Status-carrying variant: 401→auth, 402→quota, 403→authz,
        // 429→rate_limit, everything else (incl. 5xx)→other.
        let kind_of = |status: u16| {
            ApiError::ApiError {
                status,
                message: "x".to_string(),
            }
            .error_kind()
        };
        assert_eq!(kind_of(401), error_kind::AUTH);
        assert_eq!(kind_of(402), error_kind::QUOTA);
        assert_eq!(kind_of(403), error_kind::AUTHZ);
        assert_eq!(kind_of(429), error_kind::RATE_LIMIT);
        assert_eq!(kind_of(500), error_kind::OTHER);
        assert_eq!(kind_of(502), error_kind::OTHER);
        assert_eq!(kind_of(529), error_kind::OTHER);
        assert_eq!(kind_of(400), error_kind::OTHER);
        assert_eq!(kind_of(404), error_kind::OTHER);

        // Dedicated variants.
        assert_eq!(
            ApiError::AuthenticationFailed.error_kind(),
            error_kind::AUTH
        );
        assert_eq!(
            ApiError::RateLimitExceeded {
                retry_after_secs: Some(30)
            }
            .error_kind(),
            error_kind::RATE_LIMIT
        );

        // Transient / non-provider classes stay `other` — timeouts, network
        // send failures (proxied here by I/O), parse errors.
        assert_eq!(ApiError::Timeout.error_kind(), error_kind::OTHER);
        assert_eq!(
            ApiError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "net")).error_kind(),
            error_kind::OTHER
        );
        assert_eq!(
            ApiError::InvalidResponse("error sending request".to_string()).error_kind(),
            error_kind::OTHER
        );
    }

    /// The P-N1 root cause, pinned: a provider answering 402/403 with a
    /// structured JSON body used to collapse into `ProviderError` and DROP
    /// the status — the exact type information the quota/authz banners need
    /// (e.g. DeepSeek's 402 `{"error":{"message":"Insufficient Balance"}}`).
    /// `from_provider_response` must preserve the status so `error_kind()`
    /// still sees 402/403.
    #[test]
    fn test_error_kind_survives_provider_body_parse_for_402_and_403() {
        // DeepSeek-style OpenAI-compatible 402 body.
        let err = ApiError::from_provider_response(
            &LlmProvider::DeepSeek,
            402,
            r#"{"error":{"message":"Insufficient Balance","type":"insufficient_balance"}}"#,
        );
        assert_eq!(err.error_kind(), error_kind::QUOTA);
        match &err {
            ApiError::ProviderError {
                status, message, ..
            } => {
                assert_eq!(*status, Some(402));
                assert!(message.contains("Insufficient Balance"));
            }
            other => panic!("Expected ProviderError, got {other:?}"),
        }

        // Anthropic-style 403 body (permission-flavoured).
        let err = ApiError::from_provider_response(
            &LlmProvider::Anthropic,
            403,
            r#"{"type":"error","error":{"type":"permission_error","message":"your key cannot access this model"}}"#,
        );
        assert_eq!(err.error_kind(), error_kind::AUTHZ);

        // Fallback (non-JSON body) keeps the status too.
        let err = ApiError::from_provider_response(&LlmProvider::Custom, 402, "quota gone");
        assert_eq!(err.error_kind(), error_kind::QUOTA);

        // Non-special statuses stay `other` even with a parsed body.
        let err = ApiError::from_provider_response(
            &LlmProvider::OpenAI,
            404,
            r#"{"error":{"message":"model not found","type":"invalid_request_error"}}"#,
        );
        assert_eq!(err.error_kind(), error_kind::OTHER);
    }

    /// Provider errors parsed OUTSIDE an HTTP exchange (in-stream Ollama
    /// error fields, adapter probes) carry no status → `other`.
    #[test]
    fn test_error_kind_provider_error_without_status_is_other() {
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "model 'foo' not found".to_string(),
            status: None,
        };
        assert_eq!(err.error_kind(), error_kind::OTHER);
    }

    // ── A8: timeout-class detection ─────────────────────────────────────

    /// The three matching words must stay identical to the headless
    /// classifier (`shannon-cli` `classify_headless_failure`, which maps the
    /// `QueryEvent::Failed` string to `HeadlessExitCode::Timeout`): matching
    /// happens on the error's Display string so both sides see the same text
    /// and cannot drift.
    #[test]
    fn test_is_timeout_class_matches_headless_classification_words() {
        // Word 1+2: "timeout" — the dedicated variant and provider strings.
        assert!(ApiError::Timeout.is_timeout_class());
        let provider_timeout = ApiError::ProviderError {
            provider: "zhipu-coding".to_string(),
            error_type: "timeout_error".to_string(),
            message: "upstream request timeout".to_string(),
            status: None,
        };
        assert!(provider_timeout.is_timeout_class());
        // "timed out" — reqwest's TimedOut source surfaces as this wording.
        assert!(
            ApiError::InvalidResponse("operation timed out while streaming".to_string())
                .is_timeout_class()
        );
        // "error sending request" — reqwest Kind::Request (P1-3 class).
        assert!(
            ApiError::InvalidResponse(
                "client error (SendRequest): error sending request for url (http://x)".to_string()
            )
            .is_timeout_class()
        );
    }

    /// Non-timeout classes must not be continuable: A8 must never swallow
    /// rate limits, auth failures, or malformed output.
    #[test]
    fn test_is_timeout_class_rejects_other_classes() {
        assert!(
            !ApiError::RateLimitExceeded {
                retry_after_secs: None
            }
            .is_timeout_class()
        );
        assert!(!ApiError::AuthenticationFailed.is_timeout_class());
        assert!(
            !ApiError::ProviderError {
                provider: "ollama".to_string(),
                error_type: "ollama_error".to_string(),
                message: "Value looks like object, but can't find closing '}' symbol".to_string(),
                status: None,
            }
            .is_timeout_class()
        );
        assert!(!ApiError::InvalidResponse("model not found".to_string()).is_timeout_class());
    }

    /// A8b: stream-interruption detection is TYPE-level — only the
    /// dedicated variant qualifies, regardless of what any Display string
    /// says. This is what makes "normal text-only completion" impossible
    /// to mis-continue: a clean completion never produces this variant.
    #[test]
    fn test_is_stream_interrupted_type_level_only() {
        assert!(ApiError::StreamEndedUnexpectedly.is_stream_interrupted());
        // Same words in a string variant must NOT match (no string sniffing).
        assert!(
            !ApiError::InvalidResponse("Stream ended unexpectedly".to_string())
                .is_stream_interrupted()
        );
        // Timeout is its own class, not a stream interruption.
        assert!(ApiError::Timeout.is_timeout_class());
        assert!(!ApiError::Timeout.is_stream_interrupted());
        assert!(!ApiError::AuthenticationFailed.is_stream_interrupted());
        assert!(
            !ApiError::ProviderError {
                provider: "zhipu-coding".to_string(),
                error_type: "timeout_error".to_string(),
                message: "upstream request timeout".to_string(),
                status: None,
            }
            .is_stream_interrupted()
        );
    }

    /// Error message + suggestion must not duplicate content between the two.
    #[test]
    fn test_error_suggestion_no_content_duplication() {
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "Value looks like object, but can't find closing '}' symbol".to_string(),
            status: None,
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
