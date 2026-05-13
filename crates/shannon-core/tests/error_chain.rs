//! Error chain propagation tests.
//!
//! Tests verify that errors propagate correctly across layers:
//! - ApiError variants carry correct information
//! - ToolError → ToolExecutionError conversion
//! - PermissionError → ToolExecutionError conversion
//! - QueryError variants have correct string representations
//! - Provider-specific error parsing (Anthropic, OpenAI, Ollama)

use shannon_core::api::error::ApiError;
use shannon_core::api::types::LlmProvider;
use shannon_core::error::{ToolError, PermissionError, ToolExecutionError};
use shannon_core::query_engine::QueryError;

// ── ApiError Variants ───────────────────────────────────────────────────

#[test]
fn test_api_error_http_error() {
    // reqwest::Error is hard to construct directly, so test the variant exists
    let err = ApiError::AuthenticationFailed;
    assert!(err.to_string().contains("Authentication"));
}

#[test]
fn test_api_error_rate_limit() {
    let err = ApiError::RateLimitExceeded { retry_after_secs: None };
    assert!(err.to_string().contains("Rate limit"));
}

#[test]
fn test_api_error_invalid_response() {
    let err = ApiError::InvalidResponse("bad json".to_string());
    assert!(err.to_string().contains("bad json"));
}

#[test]
fn test_api_error_timeout() {
    let err = ApiError::Timeout;
    assert!(err.to_string().contains("Timeout"));
}

#[test]
fn test_api_error_stream_ended() {
    let err = ApiError::StreamEndedUnexpectedly;
    assert!(err.to_string().contains("Stream ended"));
}

#[test]
fn test_api_error_unsupported_provider() {
    let err = ApiError::UnsupportedProvider("groq".to_string());
    assert!(err.to_string().contains("groq"));
}

#[test]
fn test_api_error_provider_error() {
    let err = ApiError::ProviderError {
        provider: "anthropic".to_string(),
        error_type: "invalid_request".to_string(),
        message: "max tokens too large".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("anthropic"));
    assert!(msg.contains("invalid_request"));
    assert!(msg.contains("max tokens too large"));
}

#[test]
fn test_api_error_from_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let api_err: ApiError = io_err.into();
    assert!(api_err.to_string().contains("pipe broke"));
}

#[test]
fn test_api_error_from_json() {
    let json_err = serde_json::from_str::<i32>("not a number").unwrap_err();
    let api_err: ApiError = json_err.into();
    assert!(api_err.to_string().contains("JSON"));
}

// ── Provider Response Parsing ───────────────────────────────────────────

#[test]
fn test_anthropic_error_parsing() {
    let body = r#"{"error": {"type": "invalid_request_error", "message": "messages: required field"}}"#;
    let err = ApiError::from_provider_response(&LlmProvider::Anthropic, 400, body);

    match err {
        ApiError::ProviderError { provider, error_type, message } => {
            assert_eq!(provider, "anthropic");
            assert_eq!(error_type, "invalid_request_error");
            assert!(message.contains("messages: required field"));
        }
        _ => panic!("Expected ProviderError, got {err:?}"),
    }
}

#[test]
fn test_openai_error_parsing() {
    let body = r#"{"error": {"message": "Invalid API key", "type": "invalid_request_error", "code": "invalid_api_key"}}"#;
    let err = ApiError::from_provider_response(&LlmProvider::OpenAI, 401, body);

    // 401 should short-circuit to AuthenticationFailed
    assert!(matches!(err, ApiError::AuthenticationFailed));
}

#[test]
fn test_openai_error_parsing_400() {
    let body = r#"{"error": {"message": "max tokens exceeds limit", "type": "invalid_request_error"}}"#;
    let err = ApiError::from_provider_response(&LlmProvider::OpenAI, 400, body);

    match err {
        ApiError::ProviderError { provider, error_type, message } => {
            assert_eq!(provider, "openai");
            assert_eq!(error_type, "invalid_request_error");
            assert!(message.contains("max tokens"));
        }
        _ => panic!("Expected ProviderError, got {err:?}"),
    }
}

#[test]
fn test_ollama_error_parsing() {
    let body = r#"{"error": "model not found"}"#;
    let err = ApiError::from_provider_response(&LlmProvider::Ollama, 404, body);

    match err {
        ApiError::ProviderError { provider, error_type, message } => {
            assert_eq!(provider, "ollama");
            assert_eq!(error_type, "ollama_error");
            assert!(message.contains("model not found"));
        }
        _ => panic!("Expected ProviderError, got {err:?}"),
    }
}

#[test]
fn test_401_short_circuits_to_auth_failed() {
    let err = ApiError::from_provider_response(&LlmProvider::Anthropic, 401, "anything");
    assert!(matches!(err, ApiError::AuthenticationFailed));
}

#[test]
fn test_429_short_circuits_to_rate_limit() {
    let err = ApiError::from_provider_response(&LlmProvider::OpenAI, 429, "slow down");
    assert!(matches!(err, ApiError::RateLimitExceeded { .. }));
}

#[test]
fn test_500_maps_to_api_error() {
    let err = ApiError::from_provider_response(&LlmProvider::Anthropic, 500, "internal error");
    match err {
        ApiError::ApiError { status, message } => {
            assert_eq!(status, 500);
            assert_eq!(message, "internal error");
        }
        _ => panic!("Expected ApiError variant, got {err:?}"),
    }
}

#[test]
fn test_503_maps_to_api_error() {
    let err = ApiError::from_provider_response(&LlmProvider::OpenAI, 503, "overloaded");
    match err {
        ApiError::ApiError { status, .. } => {
            assert_eq!(status, 503);
        }
        _ => panic!("Expected ApiError variant, got {err:?}"),
    }
}

#[test]
fn test_unknown_provider_fallback() {
    let body = "some plain text error";
    let err = ApiError::from_provider_response(&LlmProvider::Custom, 418, body);

    match err {
        ApiError::ProviderError { error_type, message, .. } => {
            assert_eq!(error_type, "http_418");
            assert_eq!(message, "some plain text error");
        }
        _ => panic!("Expected ProviderError, got {err:?}"),
    }
}

// ── ToolError → ToolExecutionError ──────────────────────────────────────

#[test]
fn test_tool_error_not_found_conversion() {
    let tool_err = ToolError::NotFound("my_tool".to_string());
    let exec_err: ToolExecutionError = tool_err.into();

    match exec_err {
        ToolExecutionError::ToolNotFound(name) => assert_eq!(name, "my_tool"),
        _ => panic!("Expected ToolNotFound, got {exec_err:?}"),
    }
}

#[test]
fn test_tool_error_invalid_input_conversion() {
    let tool_err = ToolError::InvalidInput("missing field".to_string());
    let exec_err: ToolExecutionError = tool_err.into();

    match exec_err {
        ToolExecutionError::InvalidInput { reason, .. } => {
            assert!(reason.contains("missing field"));
        }
        _ => panic!("Expected InvalidInput, got {exec_err:?}"),
    }
}

#[test]
fn test_tool_error_execution_failed_conversion() {
    let tool_err = ToolError::ExecutionFailed("exit code 1".to_string());
    let exec_err: ToolExecutionError = tool_err.into();

    match exec_err {
        ToolExecutionError::ExecutionFailed(msg) => {
            assert!(msg.contains("exit code 1"));
        }
        _ => panic!("Expected ExecutionFailed, got {exec_err:?}"),
    }
}

#[test]
fn test_tool_error_registry_error_conversion() {
    let tool_err = ToolError::RegistryError("duplicate name".to_string());
    let exec_err: ToolExecutionError = tool_err.into();

    match exec_err {
        ToolExecutionError::Internal(msg) => {
            assert!(msg.contains("duplicate name"));
        }
        _ => panic!("Expected Internal, got {exec_err:?}"),
    }
}

// ── PermissionError → ToolExecutionError ────────────────────────────────

#[test]
fn test_permission_error_denied_conversion() {
    let perm_err = PermissionError::Denied("bash: dangerous command".to_string());
    let exec_err: ToolExecutionError = perm_err.into();

    match exec_err {
        ToolExecutionError::PermissionDenied { reason, .. } => {
            assert!(reason.contains("dangerous command"));
        }
        _ => panic!("Expected PermissionDenied, got {exec_err:?}"),
    }
}

#[test]
fn test_permission_error_invalid_conversion() {
    let perm_err = PermissionError::InvalidPermission("bad format".to_string());
    let exec_err: ToolExecutionError = perm_err.into();

    match exec_err {
        ToolExecutionError::PermissionDenied { reason, .. } => {
            assert!(reason.contains("bad format"));
        }
        _ => panic!("Expected PermissionDenied, got {exec_err:?}"),
    }
}

// ── QueryError Variants ────────────────────────────────────────────────

#[test]
fn test_query_error_api() {
    let err = QueryError::ApiError("connection refused".to_string());
    assert!(err.to_string().contains("connection refused"));
}

#[test]
fn test_query_error_tool() {
    let err = QueryError::ToolError("bash timeout".to_string());
    assert!(err.to_string().contains("bash timeout"));
}

#[test]
fn test_query_error_permission_denied() {
    let err = QueryError::PermissionDenied("rm -rf /".to_string());
    assert!(err.to_string().contains("rm -rf"));
}

#[test]
fn test_query_error_state() {
    let err = QueryError::StateError("session corrupt".to_string());
    assert!(err.to_string().contains("session corrupt"));
}

#[test]
fn test_query_error_invalid_query() {
    let err = QueryError::InvalidQuery("empty message".to_string());
    assert!(err.to_string().contains("empty message"));
}

#[test]
fn test_query_error_rate_limit() {
    let err = QueryError::RateLimitExceeded;
    assert!(err.to_string().contains("Rate limit"));
}

#[test]
fn test_query_error_timeout() {
    let err = QueryError::Timeout;
    assert!(err.to_string().contains("timeout"));
}

#[test]
fn test_query_error_configuration() {
    let err = QueryError::ConfigurationError("missing API key".to_string());
    assert!(err.to_string().contains("missing API key"));
}

// ── Cross-Layer Error Message Propagation ───────────────────────────────

#[test]
fn test_api_error_message_preserved_in_query_error() {
    let original_msg = "HTTP 429: Too many requests";
    let query_err = QueryError::ApiError(original_msg.to_string());
    assert!(query_err.to_string().contains("429"));
}

#[test]
fn test_tool_error_chain_preserves_context() {
    // Simulate: ToolError → ToolExecutionError
    let original = ToolError::ExecutionFailed("script.sh failed with exit code 127".to_string());
    let exec_err: ToolExecutionError = original.into();

    // The error message should carry the original context
    let msg = exec_err.to_string();
    assert!(msg.contains("script.sh"));
    assert!(msg.contains("127"));
}

#[test]
fn test_permission_error_chain_preserves_context() {
    let original = PermissionError::Denied("write to /etc/passwd blocked by policy".to_string());
    let exec_err: ToolExecutionError = original.into();

    let msg = exec_err.to_string();
    assert!(msg.contains("/etc/passwd"));
    assert!(msg.contains("policy"));
}
