//! API retry logic with exponential backoff and provider-specific error handling.

use crate::api::error::ApiError;
use std::time::Duration;
use tokio::time::sleep;

/// Configuration for API retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_retries: u32,
    /// Initial backoff duration in milliseconds.
    pub initial_backoff_ms: u64,
    /// Maximum backoff duration in milliseconds.
    pub max_backoff_ms: u64,
    /// HTTP status codes that are retryable.
    pub retryable_status_codes: Vec<u16>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff_ms: 1000,
            max_backoff_ms: 30_000,
            retryable_status_codes: vec![429, 500, 502, 503, 504],
        }
    }
}

impl RetryConfig {
    /// Create a new retry config with custom values.
    pub fn new(max_retries: u32, initial_backoff_ms: u64, max_backoff_ms: u64) -> Self {
        Self {
            max_retries,
            initial_backoff_ms,
            max_backoff_ms,
            retryable_status_codes: vec![429, 500, 502, 503, 504],
        }
    }

    /// Check if an error is retryable.
    pub fn is_retryable(&self, error: &ApiError) -> bool {
        match error {
            ApiError::RateLimitExceeded { .. } => true,
            ApiError::ApiError { status, message } => {
                // Ollama malformed output (HTTP 500): retrying with the same tools
                // is futile. Let the engine retry WITHOUT tools instead.
                if *status == 500 && super::error::is_ollama_malformed_message(message) {
                    return false;
                }
                self.retryable_status_codes.contains(status)
            }
            ApiError::HttpError(e) => e.is_timeout() || e.is_connect(),
            ApiError::Timeout => true,
            // Auth errors, invalid responses, and provider errors are not retryable
            ApiError::AuthenticationFailed
            | ApiError::InvalidResponse(_)
            | ApiError::InvalidRequestBody(_)
            | ApiError::UnsupportedProvider(_)
            | ApiError::StreamEndedUnexpectedly
            | ApiError::ToolUseError(_)
            | ApiError::Io(_)
            | ApiError::JsonError(_)
            | ApiError::ProviderError { .. } => false,
        }
    }

    /// Calculate the backoff duration for a given attempt number.
    pub fn backoff_duration(&self, attempt: u32) -> Duration {
        let exponent = 2u64.saturating_pow(attempt);
        let backoff_ms = self.initial_backoff_ms.saturating_mul(exponent);
        let capped = backoff_ms.min(self.max_backoff_ms);
        // Add jitter: random 0-25% of the backoff
        let jitter = (capped as f64 * 0.25 * rand_jitter_factor()) as u64;
        Duration::from_millis(capped + jitter)
    }
}

/// Simple deterministic jitter factor (0.0 to 1.0).
/// Uses attempt-based variation instead of actual random for determinism.
fn rand_jitter_factor() -> f64 {
    // Use a simple hash-like approach for deterministic jitter
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    now as f64 / u32::MAX as f64
}

/// Execute an async operation with retry logic.
///
/// The closure `f` is called up to `max_retries + 1` times.
/// On retryable errors, waits with exponential backoff before retrying.
pub async fn retry_request<F, Fut, T>(config: &RetryConfig, mut f: F) -> Result<T, ApiError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, ApiError>>,
{
    let mut last_error: Option<ApiError> = None;

    for attempt in 0..=config.max_retries {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if attempt >= config.max_retries || !config.is_retryable(&e) {
                    return Err(e);
                }
                let backoff = config.backoff_duration(attempt);
                // Honor server-suggested Retry-After for 429 responses
                let wait = match &e {
                    ApiError::RateLimitExceeded {
                        retry_after_secs: Some(secs),
                    } => {
                        let ra = Duration::from_secs(*secs);
                        if ra > backoff { ra } else { backoff }
                    }
                    _ => backoff,
                };
                tracing::warn!(
                    "API request failed (attempt {}/{}): {}. Retrying in {:?}",
                    attempt + 1,
                    config.max_retries + 1,
                    e,
                    wait
                );
                sleep(wait).await;
                last_error = Some(e);
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| ApiError::InvalidResponse("All retries exhausted".to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_backoff_ms, 1000);
        assert_eq!(config.max_backoff_ms, 30_000);
        assert!(config.retryable_status_codes.contains(&429));
        assert!(config.retryable_status_codes.contains(&500));
        assert!(config.retryable_status_codes.contains(&503));
    }

    #[test]
    fn test_retry_config_custom() {
        let config = RetryConfig::new(5, 500, 60_000);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.initial_backoff_ms, 500);
        assert_eq!(config.max_backoff_ms, 60_000);
    }

    #[test]
    fn test_is_retryable_rate_limit() {
        let config = RetryConfig::default();
        assert!(config.is_retryable(&ApiError::RateLimitExceeded {
            retry_after_secs: None
        }));
    }

    #[test]
    fn test_is_retryable_server_error() {
        let config = RetryConfig::default();
        assert!(config.is_retryable(&ApiError::ApiError {
            status: 500,
            message: "Internal Server Error".to_string(),
        }));
        assert!(config.is_retryable(&ApiError::ApiError {
            status: 503,
            message: "Service Unavailable".to_string(),
        }));
    }

    #[test]
    fn test_is_not_retryable_auth_error() {
        let config = RetryConfig::default();
        assert!(!config.is_retryable(&ApiError::AuthenticationFailed));
    }

    #[test]
    fn test_is_not_retryable_client_error() {
        let config = RetryConfig::default();
        assert!(!config.is_retryable(&ApiError::ApiError {
            status: 400,
            message: "Bad Request".to_string(),
        }));
    }

    #[test]
    fn test_is_not_retryable_invalid_response() {
        let config = RetryConfig::default();
        assert!(!config.is_retryable(&ApiError::InvalidResponse("bad".to_string())));
    }

    #[test]
    fn test_backoff_duration_increases() {
        let config = RetryConfig::new(5, 1000, 30_000);
        let d0 = config.backoff_duration(0);
        let d1 = config.backoff_duration(1);
        let d2 = config.backoff_duration(2);

        // Backoff should increase (ignoring jitter)
        assert!(d1 >= d0);
        assert!(d2 >= d1);
    }

    #[test]
    fn test_backoff_duration_capped() {
        let config = RetryConfig::new(10, 1000, 5000);
        let d = config.backoff_duration(100);
        // Should be capped at max_backoff_ms + jitter
        assert!(d.as_millis() <= 6500); // 5000 + 25% jitter
    }

    #[tokio::test]
    async fn test_retry_succeeds_first_try() {
        let config = RetryConfig::default();
        let result: Result<i32, ApiError> = retry_request(&config, || async { Ok(42) }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_succeeds_after_retries() {
        let config = RetryConfig::new(3, 10, 100);
        let mut attempts = 0;
        let result: Result<i32, ApiError> = retry_request(&config, || {
            attempts += 1;
            async move {
                if attempts < 3 {
                    Err(ApiError::RateLimitExceeded {
                        retry_after_secs: None,
                    })
                } else {
                    Ok(99)
                }
            }
        })
        .await;
        assert_eq!(result.unwrap(), 99);
    }

    #[tokio::test]
    async fn test_retry_fails_after_max_retries() {
        let config = RetryConfig::new(2, 10, 100);
        let result: Result<i32, ApiError> = retry_request(&config, || async {
            Err(ApiError::RateLimitExceeded {
                retry_after_secs: None,
            })
        })
        .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ApiError::RateLimitExceeded { .. }
        ));
    }

    #[tokio::test]
    async fn test_retry_no_retry_on_auth_error() {
        let config = RetryConfig::new(3, 10, 100);
        let mut attempts = 0;
        let result: Result<i32, ApiError> = retry_request(&config, || {
            attempts += 1;
            async move { Err(ApiError::AuthenticationFailed) }
        })
        .await;
        assert!(result.is_err());
        // Should only have tried once (no retries for auth errors)
        assert_eq!(attempts, 1);
    }

    // ── Ollama malformed output retryability tests ─────────────────────

    #[test]
    fn test_not_retryable_ollama_malformed_output_500() {
        // HTTP 500 with malformed output message → NOT retryable at HTTP level.
        // The engine will retry WITHOUT tools at a higher level.
        let config = RetryConfig::default();
        let err = ApiError::ApiError {
            status: 500,
            message: "can't find closing '}' symbol".to_string(),
        };
        assert!(
            !config.is_retryable(&err),
            "Ollama malformed 500 should NOT be retryable at HTTP level"
        );
    }

    #[test]
    fn test_not_retryable_ollama_malformed_output_500_unexpected_end() {
        let config = RetryConfig::default();
        let err = ApiError::ApiError {
            status: 500,
            message: "unexpected end of input".to_string(),
        };
        assert!(
            !config.is_retryable(&err),
            "Ollama 'unexpected end' 500 should NOT be retryable"
        );
    }

    #[test]
    fn test_not_retryable_ollama_malformed_output_500_keyword() {
        let config = RetryConfig::default();
        let err = ApiError::ApiError {
            status: 500,
            message: "the model generated malformed output".to_string(),
        };
        assert!(
            !config.is_retryable(&err),
            "Ollama 'malformed output' 500 should NOT be retryable"
        );
    }

    #[test]
    fn test_retryable_generic_500() {
        // Generic 500 (not malformed output) IS retryable
        let config = RetryConfig::default();
        let err = ApiError::ApiError {
            status: 500,
            message: "Internal Server Error".to_string(),
        };
        assert!(config.is_retryable(&err), "Generic 500 should be retryable");
    }

    #[test]
    fn test_not_retryable_provider_error_malformed() {
        // ProviderError is never retryable (engine handles retry-without-tools)
        let config = RetryConfig::default();
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "can't find closing '}' symbol".to_string(),
        };
        assert!(
            !config.is_retryable(&err),
            "ProviderError should NOT be retryable"
        );
    }

    #[test]
    fn test_not_retryable_ollama_model_not_found() {
        let config = RetryConfig::default();
        let err = ApiError::ProviderError {
            provider: "ollama".to_string(),
            error_type: "ollama_error".to_string(),
            message: "model 'foo' not found".to_string(),
        };
        assert!(
            !config.is_retryable(&err),
            "Ollama 'model not found' should NOT be retryable"
        );
    }

    #[test]
    fn test_not_retryable_openai_invalid_request() {
        let config = RetryConfig::default();
        let err = ApiError::ProviderError {
            provider: "openai".to_string(),
            error_type: "invalid_request_error".to_string(),
            message: "max_tokens is required".to_string(),
        };
        assert!(
            !config.is_retryable(&err),
            "OpenAI invalid request should NOT be retryable"
        );
    }

    #[tokio::test]
    async fn test_no_retry_for_ollama_malformed_500() {
        // Malformed output 500 should NOT be retried — engine handles it
        let config = RetryConfig::new(3, 10, 100);
        let mut attempts = 0;
        let result: Result<i32, ApiError> = retry_request(&config, || {
            attempts += 1;
            async move {
                Err(ApiError::ApiError {
                    status: 500,
                    message: "can't find closing '}' symbol".to_string(),
                })
            }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(attempts, 1, "Malformed output 500 should NOT be retried");
    }

    #[tokio::test]
    async fn test_no_retry_for_non_transient_provider_error() {
        let config = RetryConfig::new(3, 10, 100);
        let mut attempts = 0;
        let result: Result<i32, ApiError> = retry_request(&config, || {
            attempts += 1;
            async move {
                Err(ApiError::ProviderError {
                    provider: "openai".to_string(),
                    error_type: "invalid_request_error".to_string(),
                    message: "invalid api key".to_string(),
                })
            }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(
            attempts, 1,
            "Non-transient ProviderError should NOT be retried"
        );
    }
}
