//! # API Services Layer
//!
//! Provides a high-level abstraction for managing API interactions including
//! request tracking, usage statistics, and rate limit handling.
//!
//! ## Architecture
//!
//! - [`ApiManager`]: Central manager for API interactions and request dispatch
//! - [`UsageTracker`]: Tracks cumulative usage statistics across requests
//! - [`ApiRequest`]: Represents an outgoing API request
//! - [`ApiResponse`]: Represents an incoming API response
//! - [`UsageStats`]: Aggregated usage statistics (tokens, cost, requests)
//! - [`RateLimitInfo`]: Rate limit metadata from API responses
//!
//! ## Example
//!
//! ```no_run
//! use shannon_core_api::{ApiManager, ApiRequest, UsageTracker};
//!
//! let mut manager = ApiManager::new();
//! let request = ApiRequest::get("https://api.example.com/data");
//! // manager.execute(request).await?;
//! ```

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during API service operations.
#[derive(Error, Debug)]
pub enum ApiServiceError {
    #[error("Request failed: {0}")]
    RequestFailed(String),

    #[error("Rate limit exceeded: retry after {retry_after_ms}ms")]
    RateLimitExceeded { retry_after_ms: u64 },

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Response parsing error: {0}")]
    ParseError(String),

    #[error("No records found for model: {0}")]
    ModelNotFound(String),
}

// ============================================================================
// ApiRequest
// ============================================================================

/// An outgoing API request with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiRequest {
    /// HTTP method (GET, POST, PUT, DELETE, etc.)
    pub method: String,

    /// Target endpoint URL
    pub endpoint: String,

    /// Request headers
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Request body (JSON)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,

    /// When the request was created
    pub timestamp: DateTime<Utc>,

    /// Model identifier for usage tracking (e.g. "claude-opus-4-6")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Unique request identifier for correlation
    pub id: String,
}

impl ApiRequest {
    /// Create a new GET request.
    pub fn get(endpoint: &str) -> Self {
        Self::new("GET", endpoint)
    }

    /// Create a new POST request with a body.
    pub fn post(endpoint: &str, body: serde_json::Value) -> Self {
        let mut req = Self::new("POST", endpoint);
        req.body = Some(body);
        req
    }

    /// Create a new PUT request with a body.
    pub fn put(endpoint: &str, body: serde_json::Value) -> Self {
        let mut req = Self::new("PUT", endpoint);
        req.body = Some(body);
        req
    }

    /// Create a new DELETE request.
    pub fn delete(endpoint: &str) -> Self {
        Self::new("DELETE", endpoint)
    }

    /// Create a request with a custom method.
    fn new(method: &str, endpoint: &str) -> Self {
        Self {
            method: method.to_string(),
            endpoint: endpoint.to_string(),
            headers: HashMap::new(),
            body: None,
            timestamp: Utc::now(),
            model: None,
            id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Add a header to the request.
    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.to_string(), value.to_string());
        self
    }

    /// Set the model for this request (used for usage tracking).
    pub fn with_model(mut self, model: &str) -> Self {
        self.model = Some(model.to_string());
        self
    }

    /// Validate the request has required fields.
    pub fn validate(&self) -> Result<(), ApiServiceError> {
        if self.method.is_empty() {
            return Err(ApiServiceError::InvalidRequest(
                "HTTP method must not be empty".to_string(),
            ));
        }
        if self.endpoint.is_empty() {
            return Err(ApiServiceError::InvalidRequest(
                "Endpoint must not be empty".to_string(),
            ));
        }
        Ok(())
    }
}

// ============================================================================
// ApiResponse
// ============================================================================

/// An incoming API response with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse {
    /// HTTP status code
    pub status: u16,

    /// Response body (JSON)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,

    /// Response headers
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Duration of the request in milliseconds
    pub duration_ms: u64,

    /// When the response was received
    pub timestamp: DateTime<Utc>,

    /// Rate limit information if present in response headers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitInfo>,

    /// Correlation ID matching the request
    pub request_id: String,

    /// Whether the response indicates success (2xx status)
    pub success: bool,
}

impl ApiResponse {
    /// Create a new API response.
    pub fn new(status: u16, request_id: &str, duration_ms: u64) -> Self {
        let success = (200..300).contains(&status);
        Self {
            status,
            body: None,
            headers: HashMap::new(),
            duration_ms,
            timestamp: Utc::now(),
            rate_limit: None,
            request_id: request_id.to_string(),
            success,
        }
    }

    /// Create a successful response.
    pub fn ok(request_id: &str, duration_ms: u64, body: serde_json::Value) -> Self {
        let mut resp = Self::new(200, request_id, duration_ms);
        resp.body = Some(body);
        resp
    }

    /// Create an error response.
    pub fn error(status: u16, request_id: &str, duration_ms: u64, message: &str) -> Self {
        let mut resp = Self::new(status, request_id, duration_ms);
        resp.body = Some(serde_json::json!({ "error": message }));
        resp
    }

    /// Create a rate-limited response.
    pub fn rate_limited(
        request_id: &str,
        duration_ms: u64,
        retry_after_ms: u64,
        remaining: u32,
        limit: u32,
        reset_at: DateTime<Utc>,
    ) -> Self {
        let mut resp = Self::new(429, request_id, duration_ms);
        resp.body = Some(serde_json::json!({
            "error": "rate_limit_exceeded",
            "retry_after_ms": retry_after_ms,
        }));
        resp.rate_limit = Some(RateLimitInfo {
            remaining,
            limit,
            reset_at,
            retry_after: Some(Duration::milliseconds(retry_after_ms as i64)),
        });
        resp
    }

    /// Set rate limit info on this response.
    pub fn with_rate_limit(mut self, info: RateLimitInfo) -> Self {
        self.rate_limit = Some(info);
        self
    }
}

// ============================================================================
// RateLimitInfo
// ============================================================================

/// Rate limit metadata extracted from API response headers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RateLimitInfo {
    /// Number of requests remaining in the current window
    pub remaining: u32,

    /// Maximum number of requests allowed in the window
    pub limit: u32,

    /// When the rate limit window resets
    pub reset_at: DateTime<Utc>,

    /// How long to wait before retrying (if rate limited)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<Duration>,
}

impl RateLimitInfo {
    /// Create a new rate limit info.
    pub fn new(remaining: u32, limit: u32, reset_at: DateTime<Utc>) -> Self {
        Self {
            remaining,
            limit,
            reset_at,
            retry_after: None,
        }
    }

    /// Check if the rate limit is exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.remaining == 0
    }

    /// Get the retry_after as milliseconds, if set.
    pub fn retry_after_ms(&self) -> Option<i64> {
        self.retry_after.map(|d| d.num_milliseconds())
    }

    /// Calculate the percentage of quota used.
    pub fn usage_percent(&self) -> f64 {
        if self.limit == 0 {
            return 0.0;
        }
        let used = self.limit.saturating_sub(self.remaining);
        (used as f64 / self.limit as f64) * 100.0
    }
}

// ============================================================================
// UsageStats
// ============================================================================

/// Per-model usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ModelUsage {
    /// Number of requests
    pub requests: u64,

    /// Total input tokens
    pub input_tokens: u64,

    /// Total output tokens
    pub output_tokens: u64,

    /// Estimated cost in USD
    pub cost: f64,
}

impl ModelUsage {
    /// Create a new empty model usage record.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a single request's usage.
    pub fn record(&mut self, input_tokens: u64, output_tokens: u64, cost: f64) {
        self.requests += 1;
        self.input_tokens += input_tokens;
        self.output_tokens += output_tokens;
        self.cost += cost;
    }

    /// Total tokens (input + output).
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// Aggregated usage statistics across all models.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageStats {
    /// Total number of requests across all models
    pub total_requests: u64,

    /// Total tokens across all models
    pub total_tokens: u64,

    /// Total estimated cost across all models
    pub total_cost: f64,

    /// Per-model breakdown
    pub by_model: HashMap<String, ModelUsage>,

    /// Time window start
    pub period_start: DateTime<Utc>,

    /// Time window end
    pub period_end: DateTime<Utc>,
}

impl Default for UsageStats {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            total_requests: 0,
            total_tokens: 0,
            total_cost: 0.0,
            by_model: HashMap::new(),
            period_start: now,
            period_end: now,
        }
    }
}

impl UsageStats {
    /// Create usage stats for a specific time period.
    pub fn for_period(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self {
            period_start: start,
            period_end: end,
            ..Default::default()
        }
    }

    /// Record usage for a specific model.
    pub fn record(
        &mut self,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        cost: f64,
    ) {
        let entry = self.by_model.entry(model.to_string()).or_default();
        entry.record(input_tokens, output_tokens, cost);

        self.total_requests += 1;
        self.total_tokens += input_tokens + output_tokens;
        self.total_cost += cost;
        self.period_end = Utc::now();
    }

    /// Get usage stats for a specific model.
    pub fn model_stats(&self, model: &str) -> Option<&ModelUsage> {
        self.by_model.get(model)
    }

    /// Get the most expensive model by total cost.
    pub fn most_expensive_model(&self) -> Option<(String, &ModelUsage)> {
        self.by_model
            .iter()
            .max_by(|a, b| a.1.cost.partial_cmp(&b.1.cost).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(k, v)| (k.clone(), v))
    }

    /// Reset all statistics.
    pub fn reset(&mut self) {
        let now = Utc::now();
        self.total_requests = 0;
        self.total_tokens = 0;
        self.total_cost = 0.0;
        self.by_model.clear();
        self.period_start = now;
        self.period_end = now;
    }
}

// ============================================================================
// UsageTracker
// ============================================================================

/// Tracks API usage statistics over time.
#[derive(Debug, Clone)]
pub struct UsageTracker {
    /// Current session usage stats
    current: UsageStats,

    /// Historical usage snapshots
    history: Vec<UsageStats>,

    /// Maximum history entries to retain
    max_history: usize,
}

impl Default for UsageTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl UsageTracker {
    /// Create a new usage tracker with default settings.
    pub fn new() -> Self {
        Self {
            current: UsageStats::default(),
            history: Vec::new(),
            max_history: 100,
        }
    }

    /// Create a usage tracker with a custom history limit.
    pub fn with_max_history(max_history: usize) -> Self {
        Self {
            current: UsageStats::default(),
            history: Vec::new(),
            max_history,
        }
    }

    /// Record usage from a completed API request.
    pub fn record(
        &mut self,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        cost: f64,
    ) {
        self.current.record(model, input_tokens, output_tokens, cost);
    }

    /// Get current session usage stats.
    pub fn current_stats(&self) -> &UsageStats {
        &self.current
    }

    /// Get the current total cost.
    pub fn total_cost(&self) -> f64 {
        self.current.total_cost
    }

    /// Get the current total token count.
    pub fn total_tokens(&self) -> u64 {
        self.current.total_tokens
    }

    /// Get the current total request count.
    pub fn total_requests(&self) -> u64 {
        self.current.total_requests
    }

    /// Snapshot current stats into history and reset.
    pub fn snapshot_and_reset(&mut self) {
        let snapshot = self.current.clone();
        self.history.push(snapshot);

        // Trim history if needed
        while self.history.len() > self.max_history {
            self.history.remove(0);
        }

        self.current.reset();
    }

    /// Get historical snapshots.
    pub fn history(&self) -> &[UsageStats] {
        &self.history
    }

    /// Aggregate all history plus current into a single total.
    pub fn aggregate_all(&self) -> UsageStats {
        let mut total = self.current.clone();

        for snapshot in &self.history {
            total.total_requests += snapshot.total_requests;
            total.total_tokens += snapshot.total_tokens;
            total.total_cost += snapshot.total_cost;

            for (model, usage) in &snapshot.by_model {
                let entry = total.by_model.entry(model.clone()).or_default();
                entry.requests += usage.requests;
                entry.input_tokens += usage.input_tokens;
                entry.output_tokens += usage.output_tokens;
                entry.cost += usage.cost;
            }
        }

        total
    }

    /// Reset everything including history.
    pub fn reset_all(&mut self) {
        self.current.reset();
        self.history.clear();
    }
}

// ============================================================================
// ApiManager
// ============================================================================

/// Central manager for API interactions.
#[derive(Debug, Clone)]
pub struct ApiManager {
    /// Usage tracker for monitoring API consumption
    usage_tracker: UsageTracker,

    /// Default headers added to every request
    default_headers: HashMap<String, String>,

    /// Most recent rate limit info per endpoint
    rate_limits: HashMap<String, RateLimitInfo>,

    /// Request history (bounded)
    request_log: Vec<ApiRequest>,
    max_log_size: usize,

    /// Base URL prefix for relative endpoints
    base_url: Option<String>,
}

impl Default for ApiManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiManager {
    /// Create a new API manager with default settings.
    pub fn new() -> Self {
        Self {
            usage_tracker: UsageTracker::new(),
            default_headers: HashMap::new(),
            rate_limits: HashMap::new(),
            request_log: Vec::new(),
            max_log_size: 1000,
            base_url: None,
        }
    }

    /// Create an API manager with a base URL.
    pub fn with_base_url(base_url: &str) -> Self {
        let mut mgr = Self::new();
        mgr.base_url = Some(base_url.to_string());
        mgr
    }

    /// Set a default header for all requests.
    pub fn set_default_header(&mut self, key: &str, value: &str) {
        self.default_headers.insert(key.to_string(), value.to_string());
    }

    /// Remove a default header.
    pub fn remove_default_header(&mut self, key: &str) {
        self.default_headers.remove(key);
    }

    /// Get the usage tracker.
    pub fn usage_tracker(&self) -> &UsageTracker {
        &self.usage_tracker
    }

    /// Get a mutable reference to the usage tracker.
    pub fn usage_tracker_mut(&mut self) -> &mut UsageTracker {
        &mut self.usage_tracker
    }

    /// Log a request and apply default headers.
    pub fn prepare_request(&mut self, mut request: ApiRequest) -> ApiRequest {
        // Apply default headers (don't override explicitly set ones)
        for (key, value) in &self.default_headers {
            request
                .headers
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }

        // Apply base URL if set and the endpoint is relative
        if let Some(ref base) = self.base_url {
            if !request.endpoint.starts_with("http://")
                && !request.endpoint.starts_with("https://")
            {
                request.endpoint = format!("{}{}", base.trim_end_matches('/'), request.endpoint);
            }
        }

        // Log the request
        if self.request_log.len() >= self.max_log_size {
            self.request_log.remove(0);
        }
        self.request_log.push(request.clone());

        request
    }

    /// Process a response and update rate limits and usage.
    pub fn process_response(
        &mut self,
        response: &ApiResponse,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        cost: f64,
    ) {
        // Update rate limits from response
        if let Some(ref rl) = response.rate_limit {
            // Use the endpoint from the request log if available
            if let Some(req) = self
                .request_log
                .iter()
                .find(|r| r.id == response.request_id)
            {
                self.rate_limits
                    .insert(req.endpoint.clone(), rl.clone());
            }
        }

        // Track usage
        self.usage_tracker
            .record(model, input_tokens, output_tokens, cost);
    }

    /// Get rate limit info for an endpoint.
    pub fn get_rate_limit(&self, endpoint: &str) -> Option<&RateLimitInfo> {
        self.rate_limits.get(endpoint)
    }

    /// Check if any endpoint is currently rate limited (remaining == 0).
    pub fn is_rate_limited(&self) -> bool {
        self.rate_limits.values().any(|rl| rl.is_exhausted())
    }

    /// Get the request log.
    pub fn request_log(&self) -> &[ApiRequest] {
        &self.request_log
    }

    /// Clear the request log.
    pub fn clear_request_log(&mut self) {
        self.request_log.clear();
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ApiRequest tests ----

    #[test]
    fn test_request_get() {
        let req = ApiRequest::get("https://api.example.com/data");
        assert_eq!(req.method, "GET");
        assert_eq!(req.endpoint, "https://api.example.com/data");
        assert!(req.body.is_none());
        assert!(!req.id.is_empty());
    }

    #[test]
    fn test_request_post() {
        let body = serde_json::json!({ "key": "value" });
        let req = ApiRequest::post("https://api.example.com/data", body.clone());
        assert_eq!(req.method, "POST");
        assert_eq!(req.body, Some(body));
    }

    #[test]
    fn test_request_with_headers() {
        let req = ApiRequest::get("https://api.example.com/data")
            .with_header("Authorization", "Bearer token123")
            .with_model("claude-opus-4-6");

        assert_eq!(req.headers.get("Authorization").unwrap(), "Bearer token123");
        assert_eq!(req.model, Some("claude-opus-4-6".to_string()));
    }

    #[test]
    fn test_request_validation() {
        let valid = ApiRequest::get("https://api.example.com");
        assert!(valid.validate().is_ok());

        let invalid_method = ApiRequest::new("", "https://api.example.com");
        assert!(invalid_method.validate().is_err());

        let invalid_endpoint = ApiRequest::new("GET", "");
        assert!(invalid_endpoint.validate().is_err());
    }

    #[test]
    fn test_request_serialization() {
        let req = ApiRequest::get("https://api.example.com/data");
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: ApiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req.id, deserialized.id);
        assert_eq!(req.method, deserialized.method);
        assert_eq!(req.endpoint, deserialized.endpoint);
    }

    // ---- ApiResponse tests ----

    #[test]
    fn test_response_ok() {
        let body = serde_json::json!({ "result": "success" });
        let resp = ApiResponse::ok("req-123", 150, body);

        assert_eq!(resp.status, 200);
        assert!(resp.success);
        assert_eq!(resp.duration_ms, 150);
    }

    #[test]
    fn test_response_error() {
        let resp = ApiResponse::error(500, "req-456", 200, "Internal error");

        assert_eq!(resp.status, 500);
        assert!(!resp.success);
        assert_eq!(
            resp.body,
            Some(serde_json::json!({ "error": "Internal error" }))
        );
    }

    #[test]
    fn test_response_rate_limited() {
        let reset_at = Utc::now() + Duration::seconds(60);
        let resp =
            ApiResponse::rate_limited("req-789", 50, 5000, 0, 100, reset_at);

        assert_eq!(resp.status, 429);
        assert!(!resp.success);
        assert!(resp.rate_limit.is_some());

        let rl = resp.rate_limit.unwrap();
        assert_eq!(rl.remaining, 0);
        assert_eq!(rl.limit, 100);
        assert!(rl.is_exhausted());
        assert_eq!(rl.retry_after_ms(), Some(5000));
    }

    // ---- RateLimitInfo tests ----

    #[test]
    fn test_rate_limit_usage_percent() {
        let reset_at = Utc::now() + Duration::seconds(60);

        let rl = RateLimitInfo::new(75, 100, reset_at);
        assert_eq!(rl.usage_percent(), 25.0);
        assert!(!rl.is_exhausted());

        let rl_exhausted = RateLimitInfo::new(0, 100, reset_at);
        assert_eq!(rl_exhausted.usage_percent(), 100.0);
        assert!(rl_exhausted.is_exhausted());
    }

    // ---- UsageStats tests ----

    #[test]
    fn test_usage_stats_recording() {
        let mut stats = UsageStats::default();

        stats.record("claude-opus-4-6", 100, 200, 0.01);
        stats.record("claude-opus-4-6", 150, 250, 0.015);
        stats.record("claude-sonnet-4-6", 80, 120, 0.005);

        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.total_tokens, 900);
        assert!((stats.total_cost - 0.03).abs() < f64::EPSILON);

        let opus = stats.model_stats("claude-opus-4-6").unwrap();
        assert_eq!(opus.requests, 2);
        assert_eq!(opus.input_tokens, 250);
        assert_eq!(opus.output_tokens, 450);
    }

    #[test]
    fn test_usage_stats_most_expensive() {
        let mut stats = UsageStats::default();
        stats.record("cheap-model", 100, 100, 0.001);
        stats.record("expensive-model", 100, 100, 0.05);

        let (name, usage) = stats.most_expensive_model().unwrap();
        assert_eq!(name, "expensive-model");
        assert!((usage.cost - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn test_usage_stats_reset() {
        let mut stats = UsageStats::default();
        stats.record("model", 100, 200, 0.01);
        assert_eq!(stats.total_requests, 1);

        stats.reset();
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.total_tokens, 0);
        assert!((stats.total_cost).abs() < f64::EPSILON);
        assert!(stats.by_model.is_empty());
    }

    // ---- UsageTracker tests ----

    #[test]
    fn test_usage_tracker_record_and_snapshot() {
        let mut tracker = UsageTracker::new();

        tracker.record("model-a", 100, 200, 0.01);
        tracker.record("model-a", 50, 100, 0.005);

        assert_eq!(tracker.total_requests(), 2);
        assert_eq!(tracker.total_tokens(), 450);
        assert!((tracker.total_cost() - 0.015).abs() < f64::EPSILON);

        tracker.snapshot_and_reset();

        // Current should be reset
        assert_eq!(tracker.total_requests(), 0);

        // History should have one entry
        assert_eq!(tracker.history().len(), 1);
        assert_eq!(tracker.history()[0].total_requests, 2);
    }

    #[test]
    fn test_usage_tracker_aggregate() {
        let mut tracker = UsageTracker::new();

        tracker.record("model-a", 100, 200, 0.01);
        tracker.snapshot_and_reset();
        tracker.record("model-a", 50, 100, 0.005);

        let aggregate = tracker.aggregate_all();
        assert_eq!(aggregate.total_requests, 2);
        assert_eq!(aggregate.total_tokens, 450);
        assert!((aggregate.total_cost - 0.015).abs() < f64::EPSILON);
    }

    // ---- ApiManager tests ----

    #[test]
    fn test_api_manager_prepare_request() {
        let mut manager = ApiManager::with_base_url("https://api.example.com");
        manager.set_default_header("X-API-Key", "secret");

        let req = ApiRequest::get("/v1/messages");
        let prepared = manager.prepare_request(req);

        assert_eq!(
            prepared.endpoint,
            "https://api.example.com/v1/messages"
        );
        assert_eq!(prepared.headers.get("X-API-Key").unwrap(), "secret");
    }

    #[test]
    fn test_api_manager_process_response() {
        let mut manager = ApiManager::new();

        let req = manager.prepare_request(ApiRequest::get("https://api.example.com/data"));
        let reset_at = Utc::now() + Duration::seconds(60);
        let resp = ApiResponse::rate_limited(&req.id, 100, 5000, 0, 100, reset_at);

        manager.process_response(&resp, "claude-opus-4-6", 100, 200, 0.01);

        assert_eq!(manager.usage_tracker().total_requests(), 1);
        assert!(manager.get_rate_limit("https://api.example.com/data").is_some());
        assert!(manager.is_rate_limited());
    }

    #[test]
    fn test_api_manager_default_headers_not_overridden() {
        let mut manager = ApiManager::new();
        manager.set_default_header("Accept", "application/json");

        let req = ApiRequest::get("https://api.example.com/data")
            .with_header("Accept", "text/plain");

        let prepared = manager.prepare_request(req);
        assert_eq!(prepared.headers.get("Accept").unwrap(), "text/plain");
    }
}
