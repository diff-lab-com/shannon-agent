//! # Rate Limiting
//!
//! Token-bucket rate limiting and exponential backoff strategies for API request management.
//!
//! ## Architecture
//!
//! - [`RateLimiter`]: Multi-key rate limiter using token bucket algorithm
//! - [`TokenBucket`]: Per-key bucket tracking available tokens and refill timing
//! - [`ExponentialBackoff`]: Configurable retry backoff with optional jitter

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Result of a rate limit check.
#[derive(Debug, Clone, PartialEq)]
pub enum RateLimitResult {
    /// The request is allowed.
    Allowed {
        /// Number of requests remaining in the current window.
        remaining: usize,
        /// When the rate limit window fully resets.
        reset_at: Instant,
    },
    /// The request is rejected due to rate limiting.
    Rejected {
        /// How long the caller should wait before retrying.
        retry_after: Duration,
        /// The maximum number of requests allowed in the window.
        limit: usize,
    },
}

/// Configuration for the rate limiter.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum number of requests allowed in the window.
    pub max_requests: usize,
    /// Duration of the sliding window in seconds.
    pub window_seconds: usize,
    /// Maximum burst size (initial token capacity).
    pub burst_size: usize,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 60,
            window_seconds: 60,
            burst_size: 10,
        }
    }
}

impl RateLimitConfig {
    /// Create a new rate limit configuration with the given parameters.
    pub fn new(max_requests: usize, window_seconds: usize, burst_size: usize) -> Self {
        Self {
            max_requests,
            window_seconds,
            burst_size,
        }
    }
}

/// A token bucket for tracking request rates on a single key.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    /// Current number of available tokens.
    pub tokens: f64,
    /// Maximum number of tokens the bucket can hold.
    pub max_tokens: f64,
    /// Timestamp of the last token refill.
    pub last_refill: Instant,
    /// Rate of token refill (tokens per second).
    pub refill_rate: f64,
}

impl TokenBucket {
    /// Create a new token bucket.
    pub fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            last_refill: Instant::now(),
            refill_rate,
        }
    }

    /// Refill tokens based on elapsed time since the last refill.
    pub fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        let tokens_to_add = elapsed * self.refill_rate;
        self.tokens = (self.tokens + tokens_to_add).min(self.max_tokens);
        self.last_refill = now;
    }

    /// Try to consume a token. Returns true if a token was available.
    pub fn try_consume(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Get the estimated time until one token is available.
    pub fn retry_after(&self) -> Duration {
        if self.tokens >= 1.0 {
            Duration::ZERO
        } else {
            let deficit = 1.0 - self.tokens;
            let seconds = deficit / self.refill_rate;
            Duration::from_secs_f64(seconds)
        }
    }
}

/// Multi-key rate limiter using the token bucket algorithm.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    /// Per-key token buckets.
    buckets: HashMap<String, TokenBucket>,
    /// Global rate limit configuration.
    config: RateLimitConfig,
}

impl RateLimiter {
    /// Create a new rate limiter with the default configuration.
    pub fn new() -> Self {
        Self::with_config(RateLimitConfig::default())
    }

    /// Create a new rate limiter with a custom configuration.
    pub fn with_config(config: RateLimitConfig) -> Self {
        Self {
            buckets: HashMap::new(),
            config,
        }
    }

    /// Get or create a token bucket for the given key.
    fn get_or_create_bucket(&mut self, key: &str) -> &mut TokenBucket {
        let refill_rate = self.config.max_requests as f64 / self.config.window_seconds as f64;
        let max_tokens = self.config.burst_size as f64;
        self.buckets
            .entry(key.to_string())
            .or_insert_with(|| TokenBucket::new(max_tokens, refill_rate))
    }

    /// Check if a request is allowed for the given key.
    ///
    /// This does NOT consume a token. Use `record` after a successful request,
    /// or use `check` + `record` in sequence for allow-then-deduct semantics.
    pub fn check(&mut self, key: &str) -> RateLimitResult {
        let bucket = self.get_or_create_bucket(key);
        bucket.refill();

        if bucket.tokens >= 1.0 {
            let remaining = bucket.tokens as usize;
            let reset_at = bucket.last_refill + Duration::from_secs(self.config.window_seconds as u64);
            RateLimitResult::Allowed { remaining, reset_at }
        } else {
            let retry_after = bucket.retry_after();
            RateLimitResult::Rejected {
                retry_after,
                limit: self.config.max_requests,
            }
        }
    }

    /// Record a request against the given key, consuming one token.
    ///
    /// Returns the rate limit result after consumption.
    pub fn record(&mut self, key: &str) -> RateLimitResult {
        let bucket = self.get_or_create_bucket(key);

        if bucket.try_consume() {
            let remaining = bucket.tokens as usize;
            let reset_at = bucket.last_refill + Duration::from_secs(self.config.window_seconds as u64);
            RateLimitResult::Allowed { remaining, reset_at }
        } else {
            let retry_after = bucket.retry_after();
            RateLimitResult::Rejected {
                retry_after,
                limit: self.config.max_requests,
            }
        }
    }

    /// Reset the rate limit for the given key, restoring it to full capacity.
    pub fn reset(&mut self, key: &str) {
        let refill_rate = self.config.max_requests as f64 / self.config.window_seconds as f64;
        let max_tokens = self.config.burst_size as f64;
        self.buckets
            .insert(key.to_string(), TokenBucket::new(max_tokens, refill_rate));
    }

    /// Get the remaining request count for the given key.
    ///
    /// Returns the current number of available tokens as an integer.
    pub fn get_remaining(&mut self, key: &str) -> usize {
        let bucket = self.get_or_create_bucket(key);
        bucket.refill();
        bucket.tokens as usize
    }

    /// Get the number of tracked keys.
    #[cfg(test)]
    pub fn key_count(&self) -> usize {
        self.buckets.len()
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Exponential backoff strategy for retry logic.
#[derive(Debug, Clone)]
pub struct ExponentialBackoff {
    /// Base delay between retries.
    pub base_delay: Duration,
    /// Maximum delay cap.
    pub max_delay: Duration,
    /// Maximum number of retries before giving up.
    pub max_retries: usize,
    /// Whether to add random jitter to the delay.
    pub jitter: bool,
}

impl Default for ExponentialBackoff {
    fn default() -> Self {
        Self {
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            max_retries: 5,
            jitter: true,
        }
    }
}

impl ExponentialBackoff {
    /// Create a new exponential backoff strategy.
    pub fn new(base_delay: Duration, max_delay: Duration, max_retries: usize, jitter: bool) -> Self {
        Self {
            base_delay,
            max_delay,
            max_retries,
            jitter,
        }
    }

    /// Calculate the delay for a given attempt number (0-indexed).
    ///
    /// The delay follows the formula: `min(base_delay * 2^attempt, max_delay)`.
    /// If jitter is enabled, the delay is randomized between 50% and 100% of the
    /// calculated value.
    pub fn next_delay(&self, attempt: usize) -> Duration {
        if attempt >= self.max_retries {
            return Duration::ZERO;
        }

        // 2^attempt, but guard against overflow
        let multiplier = if attempt < 63 {
            1u64 << attempt
        } else {
            u64::MAX
        };

        let base_ms = self.base_delay.as_millis() as u128;
        let delay_ms = base_ms.saturating_mul(multiplier as u128);
        let capped_ms = delay_ms.min(self.max_delay.as_millis() as u128);

        if self.jitter {
            // Jitter: random between 50% and 100% of the delay
            let jitter_factor = 0.5 + (pseudo_random(attempt) * 0.5);
            let jittered_ms = (capped_ms as f64 * jitter_factor) as u128;
            Duration::from_millis(jittered_ms as u64)
        } else {
            Duration::from_millis(capped_ms as u64)
        }
    }

    /// Check if a retry should be attempted based on the attempt number.
    ///
    /// Returns true if `attempt < max_retries`.
    pub fn should_retry(&self, attempt: usize) -> bool {
        attempt < self.max_retries
    }
}

/// Simple deterministic pseudo-random number generator for jitter.
/// Uses a basic LCG (Linear Congruential Generator) seeded with the attempt number.
/// This ensures reproducible test behavior while still providing jitter variety.
fn pseudo_random(seed: usize) -> f64 {
    // LCG parameters (Numerical Recipes)
    let mut state = (seed as u64).wrapping_add(1).wrapping_mul(1664525).wrapping_add(1013904223);
    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
    // Normalize to [0, 1)
    (state % 10000) as f64 / 10000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Token Bucket Tests ===

    #[test]
    fn test_token_bucket_initial_state() {
        let bucket = TokenBucket::new(10.0, 1.0);
        assert_eq!(bucket.tokens, 10.0);
        assert_eq!(bucket.max_tokens, 10.0);
        assert_eq!(bucket.refill_rate, 1.0);
    }

    #[test]
    fn test_token_bucket_try_consume() {
        let mut bucket = TokenBucket::new(3.0, 0.0); // zero refill rate for deterministic test
        assert!(bucket.try_consume());
        assert!((bucket.tokens - 2.0).abs() < 1e-6);
        assert!(bucket.try_consume());
        assert!((bucket.tokens - 1.0).abs() < 1e-6);
        assert!(bucket.try_consume());
        assert!(bucket.tokens.abs() < 1e-6);
        assert!(!bucket.try_consume());
    }

    #[test]
    fn test_token_bucket_refill() {
        let mut bucket = TokenBucket::new(5.0, 100.0); // 100 tokens/sec
        // Manually set last_refill in the past so refill adds tokens
        bucket.last_refill = Instant::now() - Duration::from_millis(20);
        bucket.refill(); // No-op on creation, just sets last_refill to now
        // Now set it back to simulate elapsed time
        bucket.last_refill = Instant::now() - Duration::from_millis(20);
        bucket.tokens = 3.0;
        bucket.refill();
        // Should have gained ~2 tokens (100 tokens/sec * 0.02 sec)
        assert!(bucket.tokens > 3.0);
        // Should not exceed max_tokens
        assert!(bucket.tokens <= 5.0);
    }

    #[test]
    fn test_token_bucket_max_tokens_cap() {
        let mut bucket = TokenBucket::new(5.0, 1000.0);
        // Advance time significantly
        bucket.last_refill = Instant::now() - Duration::from_secs(10);
        bucket.refill();
        assert!((bucket.tokens - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_token_bucket_retry_after() {
        let mut bucket = TokenBucket::new(5.0, 10.0);
        // Drain the bucket
        for _ in 0..5 {
            bucket.try_consume();
        }
        let retry = bucket.retry_after();
        // With 10 tokens/sec refill and 0 tokens, should need ~0.1 sec for 1 token
        assert!(retry.as_millis() >= 80 && retry.as_millis() <= 120);
    }

    #[test]
    fn test_token_bucket_retry_after_when_tokens_available() {
        let bucket = TokenBucket::new(5.0, 1.0);
        assert_eq!(bucket.retry_after(), Duration::ZERO);
    }

    // === Rate Limiter Tests ===

    #[test]
    fn test_rate_limiter_new() {
        let limiter = RateLimiter::new();
        assert_eq!(limiter.config.max_requests, 60);
        assert_eq!(limiter.config.window_seconds, 60);
        assert_eq!(limiter.config.burst_size, 10);
    }

    #[test]
    fn test_rate_limiter_custom_config() {
        let config = RateLimitConfig::new(100, 120, 20);
        let limiter = RateLimiter::with_config(config);
        assert_eq!(limiter.config.max_requests, 100);
        assert_eq!(limiter.config.window_seconds, 120);
        assert_eq!(limiter.config.burst_size, 20);
    }

    #[test]
    fn test_rate_limiter_default() {
        let limiter = RateLimiter::default();
        assert_eq!(limiter.config.max_requests, 60);
    }

    #[test]
    fn test_rate_limiter_check_allowed() {
        let mut limiter = RateLimiter::with_config(RateLimitConfig::new(60, 60, 5));
        match limiter.check("key1") {
            RateLimitResult::Allowed { remaining, .. } => {
                assert!(remaining >= 4); // started with 5, check doesn't consume
            }
            RateLimitResult::Rejected { .. } => panic!("Expected Allowed"),
        }
    }

    #[test]
    fn test_rate_limiter_record_allowed() {
        let mut limiter = RateLimiter::with_config(RateLimitConfig::new(60, 60, 3));
        match limiter.record("key1") {
            RateLimitResult::Allowed { remaining, .. } => {
                assert_eq!(remaining, 2); // 3 - 1 = 2
            }
            RateLimitResult::Rejected { .. } => panic!("Expected Allowed"),
        }
    }

    #[test]
    fn test_rate_limiter_record_rejects_when_exhausted() {
        let mut limiter = RateLimiter::with_config(RateLimitConfig::new(60, 60, 2));
        assert!(matches!(limiter.record("key1"), RateLimitResult::Allowed { .. }));
        assert!(matches!(limiter.record("key1"), RateLimitResult::Allowed { .. }));
        match limiter.record("key1") {
            RateLimitResult::Rejected { retry_after, limit } => {
                assert!(retry_after > Duration::ZERO);
                assert_eq!(limit, 60);
            }
            RateLimitResult::Allowed { .. } => panic!("Expected Rejected"),
        }
    }

    #[test]
    fn test_rate_limiter_multiple_keys() {
        let mut limiter = RateLimiter::with_config(RateLimitConfig::new(60, 60, 2));
        // Exhaust key_a
        limiter.record("key_a");
        limiter.record("key_a");
        assert!(matches!(limiter.record("key_a"), RateLimitResult::Rejected { .. }));
        // key_b should still have tokens
        assert!(matches!(limiter.record("key_b"), RateLimitResult::Allowed { .. }));
        assert_eq!(limiter.key_count(), 2);
    }

    #[test]
    fn test_rate_limiter_reset() {
        let mut limiter = RateLimiter::with_config(RateLimitConfig::new(60, 60, 2));
        limiter.record("key1");
        limiter.record("key1");
        assert!(matches!(limiter.record("key1"), RateLimitResult::Rejected { .. }));

        limiter.reset("key1");
        // After reset, should have full capacity again
        match limiter.record("key1") {
            RateLimitResult::Allowed { remaining, .. } => {
                assert_eq!(remaining, 1); // burst 2 - 1 consumed = 1
            }
            RateLimitResult::Rejected { .. } => panic!("Expected Allowed after reset"),
        }
    }

    #[test]
    fn test_rate_limiter_get_remaining() {
        let mut limiter = RateLimiter::with_config(RateLimitConfig::new(60, 60, 5));
        assert_eq!(limiter.get_remaining("key1"), 5);
        limiter.record("key1");
        limiter.record("key1");
        assert_eq!(limiter.get_remaining("key1"), 3);
    }

    #[test]
    fn test_rate_limiter_burst_handling() {
        // burst_size = 3 allows 3 rapid requests, then refill kicks in
        let mut limiter = RateLimiter::with_config(RateLimitConfig::new(10, 10, 3));
        assert!(matches!(limiter.record("burst"), RateLimitResult::Allowed { .. }));
        assert!(matches!(limiter.record("burst"), RateLimitResult::Allowed { .. }));
        assert!(matches!(limiter.record("burst"), RateLimitResult::Allowed { .. }));
        // 4th should be rejected (burst exhausted, refill rate too slow for instant)
        assert!(matches!(limiter.record("burst"), RateLimitResult::Rejected { .. }));
    }

    #[test]
    fn test_rate_limiter_check_does_not_consume() {
        let mut limiter = RateLimiter::with_config(RateLimitConfig::new(60, 60, 2));
        // Multiple checks should all show remaining
        for _ in 0..5 {
            match limiter.check("no_consume") {
                RateLimitResult::Allowed { remaining, .. } => {
                    assert!(remaining >= 1);
                }
                RateLimitResult::Rejected { .. } => panic!("check should not consume tokens"),
            }
        }
    }

    // === Exponential Backoff Tests ===

    #[test]
    fn test_backoff_default() {
        let backoff = ExponentialBackoff::default();
        assert_eq!(backoff.base_delay, Duration::from_secs(1));
        assert_eq!(backoff.max_delay, Duration::from_secs(60));
        assert_eq!(backoff.max_retries, 5);
        assert!(backoff.jitter);
    }

    #[test]
    fn test_backoff_next_delay_no_jitter() {
        let backoff = ExponentialBackoff::new(
            Duration::from_secs(1),
            Duration::from_secs(60),
            5,
            false,
        );

        // 2^0 = 1s
        assert_eq!(backoff.next_delay(0), Duration::from_secs(1));
        // 2^1 = 2s
        assert_eq!(backoff.next_delay(1), Duration::from_secs(2));
        // 2^2 = 4s
        assert_eq!(backoff.next_delay(2), Duration::from_secs(4));
        // 2^3 = 8s
        assert_eq!(backoff.next_delay(3), Duration::from_secs(8));
        // 2^4 = 16s
        assert_eq!(backoff.next_delay(4), Duration::from_secs(16));
    }

    #[test]
    fn test_backoff_next_delay_with_jitter() {
        let backoff = ExponentialBackoff::new(
            Duration::from_secs(1),
            Duration::from_secs(60),
            5,
            true,
        );

        // With jitter, delay should be between 50% and 100% of base
        let delay0 = backoff.next_delay(0);
        assert!(delay0 >= Duration::from_millis(500));
        assert!(delay0 <= Duration::from_secs(1));

        let delay1 = backoff.next_delay(1);
        assert!(delay1 >= Duration::from_secs(1)); // 50% of 2s
        assert!(delay1 <= Duration::from_secs(2)); // 100% of 2s
    }

    #[test]
    fn test_backoff_max_delay_cap() {
        let backoff = ExponentialBackoff::new(
            Duration::from_secs(1),
            Duration::from_secs(5),
            20, // high enough to allow attempt 10
            false,
        );

        // 2^10 = 1024s, but capped at 5s
        assert_eq!(backoff.next_delay(10), Duration::from_secs(5));
    }

    #[test]
    fn test_backoff_should_retry() {
        let backoff = ExponentialBackoff::new(
            Duration::from_secs(1),
            Duration::from_secs(60),
            3,
            false,
        );

        assert!(backoff.should_retry(0));
        assert!(backoff.should_retry(1));
        assert!(backoff.should_retry(2));
        assert!(!backoff.should_retry(3));
        assert!(!backoff.should_retry(100));
    }

    #[test]
    fn test_backoff_next_delay_beyond_max_retries() {
        let backoff = ExponentialBackoff::new(
            Duration::from_secs(1),
            Duration::from_secs(60),
            3,
            false,
        );
        // Beyond max_retries returns ZERO
        assert_eq!(backoff.next_delay(3), Duration::ZERO);
        assert_eq!(backoff.next_delay(100), Duration::ZERO);
    }

    #[test]
    fn test_backoff_deterministic_jitter() {
        // Same attempt should produce same jitter (deterministic pseudo-random)
        let backoff = ExponentialBackoff::new(
            Duration::from_secs(1),
            Duration::from_secs(60),
            5,
            true,
        );

        let delay_a = backoff.next_delay(0);
        let delay_b = backoff.next_delay(0);
        assert_eq!(delay_a, delay_b);

        // Different attempts should (likely) produce different delays
        let delay_c = backoff.next_delay(1);
        assert_ne!(delay_a, delay_c);
    }

    #[test]
    fn test_backoff_millisecond_precision() {
        let backoff = ExponentialBackoff::new(
            Duration::from_millis(100),
            Duration::from_secs(10),
            5,
            false,
        );

        assert_eq!(backoff.next_delay(0), Duration::from_millis(100));
        assert_eq!(backoff.next_delay(1), Duration::from_millis(200));
        assert_eq!(backoff.next_delay(2), Duration::from_millis(400));
    }

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_requests, 60);
        assert_eq!(config.window_seconds, 60);
        assert_eq!(config.burst_size, 10);
    }

    #[test]
    fn test_rate_limit_result_allowed_debug() {
        let result = RateLimitResult::Allowed {
            remaining: 5,
            reset_at: Instant::now(),
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("Allowed"));
        assert!(debug.contains("remaining: 5"));
    }

    #[test]
    fn test_rate_limit_result_rejected_debug() {
        let result = RateLimitResult::Rejected {
            retry_after: Duration::from_secs(2),
            limit: 60,
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("Rejected"));
        assert!(debug.contains("retry_after: 2s"));
        assert!(debug.contains("limit: 60"));
    }

    #[test]
    fn test_rate_limiter_large_burst_zero_refill() {
        // Edge case: very large burst, slow refill
        let mut limiter = RateLimiter::with_config(RateLimitConfig::new(1, 3600, 100));
        // Should allow 100 rapid requests
        for i in 0..100 {
            match limiter.record("big_burst") {
                RateLimitResult::Allowed { remaining, .. } => {
                    assert_eq!(remaining, 99 - i);
                }
                RateLimitResult::Rejected { .. } => {
                    panic!("Should not reject at request {}", i + 1);
                }
            }
        }
        // 101st should be rejected
        assert!(matches!(limiter.record("big_burst"), RateLimitResult::Rejected { .. }));
    }
}
