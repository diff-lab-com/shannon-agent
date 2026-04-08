//! Rate limiting

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::Instant;

/// Rate limit configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub max_requests: u32,
    pub window_duration: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 100,
            window_duration: Duration::from_secs(60),
        }
    }
}

/// Rate limit result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitResult {
    Allowed,
    Denied { retry_after: Duration },
}

/// Rate limiter using token bucket algorithm
pub struct RateLimiter {
    limits: HashMap<String, RateLimitState>,
}

#[derive(Debug, Clone)]
struct RateLimitState {
    tokens: f64,
    last_update: Instant,
    max_requests: u32,
    window_duration: Duration,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            limits: HashMap::new(),
        }
    }

    /// Configure a rate limit for a key
    pub fn configure(&mut self, key: String, config: RateLimitConfig) {
        self.limits.insert(key, RateLimitState {
            tokens: config.max_requests as f64,
            last_update: Instant::now(),
            max_requests: config.max_requests,
            window_duration: config.window_duration,
        });
    }

    /// Check if a request is allowed
    pub fn check(&mut self, key: &str) -> RateLimitResult {
        let state = self.limits.entry(key.to_string()).or_insert_with(|| {
            RateLimitState {
                tokens: 100.0,
                last_update: Instant::now(),
                max_requests: 100,
                window_duration: Duration::from_secs(60),
            }
        });

        // Refill tokens based on time elapsed
        let now = Instant::now();
        let elapsed = now.duration_since(state.last_update);
        let refill = (elapsed.as_secs_f64() / state.window_duration.as_secs_f64()) * state.max_requests as f64;
        state.tokens = (state.tokens + refill).min(state.max_requests as f64);
        state.last_update = now;

        if state.tokens >= 1.0 {
            state.tokens -= 1.0;
            RateLimitResult::Allowed
        } else {
            let retry_after = Duration::from_secs_f64(
                (1.0 - state.tokens) / (state.max_requests as f64 / state.window_duration.as_secs_f64())
            );
            RateLimitResult::Denied { retry_after }
        }
    }

    /// Get current token count for a key
    pub fn get_tokens(&self, key: &str) -> Option<f64> {
        self.limits.get(key).map(|s| s.tokens)
    }

    /// Reset rate limit for a key
    pub fn reset(&mut self, key: &str) {
        if let Some(state) = self.limits.get_mut(key) {
            state.tokens = state.max_requests as f64;
            state.last_update = Instant::now();
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}
