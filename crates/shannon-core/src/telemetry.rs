//! # OpenTelemetry Integration for Shannon Code
//!
//! Provides lightweight telemetry via atomic counters hooked into the
//! `tracing` ecosystem. No external OTLP dependency is required — metrics
//! are collected in-process and exposed through [`TelemetryMetrics`].
//!
//! A custom [`TelemetryLayer`] implements `tracing_subscriber::Layer` to
//! capture events whose target starts with `"shannon::"`.
//!
//! ## Activation
//!
//! Telemetry is **opt-in**. Set `SHANNON_TELEMETRY=1` or construct a
//! [`TelemetryConfig`] with `enabled: true`.
//!
//! Standard OTLP environment variables are also honoured:
//! - `OTEL_EXPORTER_OTLP_ENDPOINT`
//! - `OTEL_SERVICE_NAME`
//!
//! ## Usage
//!
//! ```rust
//! use shannon_core::telemetry::{TelemetryManager, TelemetryConfig};
//!
//! let mgr = TelemetryManager::new(TelemetryConfig::default());
//! mgr.record_query_start("q-1", "claude-sonnet");
//! mgr.record_query_end("q-1", std::time::Duration::from_millis(120), 512);
//!
//! let metrics = mgr.metrics();
//! assert_eq!(metrics.api_calls, 1);
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};
use tracing_subscriber::Layer;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors that can occur during telemetry operations.
#[derive(Error, Debug)]
pub enum TelemetryError {
    #[error("Telemetry is disabled")]
    Disabled,

    #[error("Telemetry already shut down")]
    AlreadyShutdown,

    #[error("Configuration error: {0}")]
    Config(String),
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for Shannon telemetry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Whether telemetry collection is enabled (opt-in).
    pub enabled: bool,
    /// OTLP endpoint URL (default: `http://localhost:4317`).
    pub endpoint: String,
    /// Logical service name (default: `shannon-code`).
    pub service_name: String,
    /// Service version (defaults to the crate version).
    pub service_version: String,
    /// How often metrics would be exported if an exporter were attached.
    #[serde(default = "default_export_interval")]
    pub export_interval: Duration,
    /// Whether to export traces.
    pub trace_export: bool,
    /// Whether to export metrics.
    pub metrics_export: bool,
}

fn default_export_interval() -> Duration {
    Duration::from_secs(30)
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

impl TelemetryConfig {
    /// Build a config, falling back to environment variables where available.
    ///
    /// | Variable | Field |
    /// |----------|-------|
    /// | `SHANNON_TELEMETRY` | `enabled` (`1` = true) |
    /// | `OTEL_EXPORTER_OTLP_ENDPOINT` | `endpoint` |
    /// | `OTEL_SERVICE_NAME` | `service_name` |
    pub fn from_env() -> Self {
        let enabled = std::env::var("SHANNON_TELEMETRY")
            .map(|v| v == "1")
            .unwrap_or(false);

        let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:4317".to_string());

        let service_name = std::env::var("OTEL_SERVICE_NAME")
            .unwrap_or_else(|_| "shannon-code".to_string());

        Self {
            enabled,
            endpoint,
            service_name,
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            export_interval: Duration::from_secs(30),
            trace_export: true,
            metrics_export: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Metrics snapshot
// ---------------------------------------------------------------------------

/// Point-in-time snapshot of collected telemetry counters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryMetrics {
    pub spans_created: u64,
    pub events_emitted: u64,
    pub errors_reported: u64,
    pub tool_calls: u64,
    pub api_calls: u64,
    pub tokens_used: u64,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// Manages OpenTelemetry-compatible telemetry for Shannon Code.
///
/// Uses atomic counters for lock-free metric collection. A
/// [`TelemetryLayer`] can be attached to a `tracing` subscriber to
/// automatically capture `shannon::`-targeted events.
pub struct TelemetryManager {
    config: TelemetryConfig,
    spans_created: AtomicU64,
    events_emitted: AtomicU64,
    errors_reported: AtomicU64,
    tool_calls: AtomicU64,
    api_calls: AtomicU64,
    tokens_used: AtomicU64,
    shutdown: AtomicU64,
}

impl TelemetryManager {
    /// Create a new telemetry manager from the given config.
    pub fn new(config: TelemetryConfig) -> Self {
        let mgr = Self {
            config,
            spans_created: AtomicU64::new(0),
            events_emitted: AtomicU64::new(0),
            errors_reported: AtomicU64::new(0),
            tool_calls: AtomicU64::new(0),
            api_calls: AtomicU64::new(0),
            tokens_used: AtomicU64::new(0),
            shutdown: AtomicU64::new(0),
        };

        if mgr.config.enabled {
            info!(
                service = %mgr.config.service_name,
                version = %mgr.config.service_version,
                endpoint = %mgr.config.endpoint,
                "Shannon telemetry enabled"
            );
        }

        mgr
    }

    /// Reference to the active configuration.
    pub fn config(&self) -> &TelemetryConfig {
        &self.config
    }

    /// Whether telemetry collection is active.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled && self.shutdown.load(Ordering::Relaxed) == 0
    }

    // -----------------------------------------------------------------------
    // Event recording
    // -----------------------------------------------------------------------

    /// Record the start of a query / conversation turn.
    pub fn record_query_start(&self, query_id: &str, model: &str) {
        if !self.is_enabled() {
            return;
        }
        self.api_calls.fetch_add(1, Ordering::Relaxed);
        self.events_emitted.fetch_add(1, Ordering::Relaxed);
        debug!(
            query_id = %query_id,
            model = %model,
            "shannon::query_start"
        );
    }

    /// Record the end of a query / conversation turn.
    pub fn record_query_end(&self, query_id: &str, duration: Duration, tokens: u64) {
        if !self.is_enabled() {
            return;
        }
        self.tokens_used.fetch_add(tokens, Ordering::Relaxed);
        self.events_emitted.fetch_add(1, Ordering::Relaxed);
        debug!(
            query_id = %query_id,
            duration_ms = duration.as_millis() as u64,
            tokens = tokens,
            "shannon::query_end"
        );
    }

    /// Record a tool invocation.
    pub fn record_tool_call(&self, tool: &str, success: bool, duration: Duration) {
        if !self.is_enabled() {
            return;
        }
        self.tool_calls.fetch_add(1, Ordering::Relaxed);
        self.events_emitted.fetch_add(1, Ordering::Relaxed);
        debug!(
            tool = %tool,
            success = success,
            duration_ms = duration.as_millis() as u64,
            "shannon::tool_call"
        );
    }

    /// Record an API call to an upstream LLM provider.
    pub fn record_api_call(
        &self,
        provider: &str,
        model: &str,
        tokens: u64,
        latency: Duration,
    ) {
        if !self.is_enabled() {
            return;
        }
        self.api_calls.fetch_add(1, Ordering::Relaxed);
        self.tokens_used.fetch_add(tokens, Ordering::Relaxed);
        self.events_emitted.fetch_add(1, Ordering::Relaxed);
        debug!(
            provider = %provider,
            model = %model,
            tokens = tokens,
            latency_ms = latency.as_millis() as u64,
            "shannon::api_call"
        );
    }

    /// Record an error event.
    pub fn record_error(&self, component: &str, error_type: &str) {
        if !self.is_enabled() {
            return;
        }
        self.errors_reported.fetch_add(1, Ordering::Relaxed);
        self.events_emitted.fetch_add(1, Ordering::Relaxed);
        warn!(
            component = %component,
            error_type = %error_type,
            "shannon::error"
        );
    }

    // -----------------------------------------------------------------------
    // Metrics retrieval
    // -----------------------------------------------------------------------

    /// Return a point-in-time snapshot of all metric counters.
    pub fn metrics(&self) -> TelemetryMetrics {
        TelemetryMetrics {
            spans_created: self.spans_created.load(Ordering::Relaxed),
            events_emitted: self.events_emitted.load(Ordering::Relaxed),
            errors_reported: self.errors_reported.load(Ordering::Relaxed),
            tool_calls: self.tool_calls.load(Ordering::Relaxed),
            api_calls: self.api_calls.load(Ordering::Relaxed),
            tokens_used: self.tokens_used.load(Ordering::Relaxed),
        }
    }

    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// Gracefully shut down the telemetry manager.
    ///
    /// After shutdown all recording methods become no-ops.
    pub fn shutdown(&self) -> Result<(), TelemetryError> {
        if self
            .shutdown
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(TelemetryError::AlreadyShutdown);
        }

        if self.config.enabled {
            let m = self.metrics();
            info!(
                spans = m.spans_created,
                events = m.events_emitted,
                errors = m.errors_reported,
                tools = m.tool_calls,
                api_calls = m.api_calls,
                tokens = m.tokens_used,
                "Shannon telemetry shutdown"
            );
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Internal helpers for TelemetryLayer
    // -----------------------------------------------------------------------

    /// Increment the spans-created counter (used by [`TelemetryLayer`]).
    fn inc_spans(&self) {
        if self.is_enabled() {
            self.spans_created.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Increment the events-emitted counter (used by [`TelemetryLayer`]).
    fn inc_events(&self) {
        if self.is_enabled() {
            self.events_emitted.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// ---------------------------------------------------------------------------
// TelemetryLayer  (tracing integration)
// ---------------------------------------------------------------------------

/// A `tracing_subscriber::Layer` that captures events and spans whose
/// target starts with `"shannon::"` and feeds them into a
/// [`TelemetryManager`].
///
/// ```rust
/// use std::sync::Arc;
/// use tracing_subscriber::prelude::*;
/// use shannon_core::telemetry::{TelemetryConfig, TelemetryManager, TelemetryLayer};
///
/// let mgr = Arc::new(TelemetryManager::new(TelemetryConfig::default()));
/// let layer = TelemetryLayer::new(Arc::clone(&mgr));
///
/// tracing_subscriber::registry().with(layer).init();
/// ```
pub struct TelemetryLayer {
    manager: Arc<TelemetryManager>,
}

impl TelemetryLayer {
    /// Create a new layer backed by the given manager.
    pub fn new(manager: Arc<TelemetryManager>) -> Self {
        Self { manager }
    }
}

impl<S> Layer<S> for TelemetryLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let target = event.metadata().target();
        if target.starts_with("shannon::") {
            self.manager.inc_events();
        }
    }

    fn on_new_span(
        &self,
        _attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        // We count all new spans; finer filtering can be added later.
        self.manager.inc_spans();
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    // -- Config tests -------------------------------------------------------

    #[test]
    fn test_default_config_is_disabled() {
        // Default config may or may not pick up env vars, so we test the
        // explicit constructor with a clean env baseline.
        let cfg = TelemetryConfig {
            enabled: false,
            endpoint: "http://localhost:4317".to_string(),
            service_name: "shannon-code".to_string(),
            service_version: "0.0.0".to_string(),
            export_interval: Duration::from_secs(30),
            trace_export: true,
            metrics_export: true,
        };
        assert!(!cfg.enabled);
        assert_eq!(cfg.endpoint, "http://localhost:4317");
        assert_eq!(cfg.service_name, "shannon-code");
    }

    #[test]
    fn test_from_env_defaults() {
        // Clear any env vars that could interfere (unsafe in edition 2024).
        unsafe {
            std::env::remove_var("SHANNON_TELEMETRY");
            std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
            std::env::remove_var("OTEL_SERVICE_NAME");
        }

        let cfg = TelemetryConfig::from_env();
        assert!(!cfg.enabled);
        assert_eq!(cfg.endpoint, "http://localhost:4317");
        assert_eq!(cfg.service_name, "shannon-code");
        assert!(!cfg.service_version.is_empty());
    }

    #[test]
    fn test_manager_default_config_is_noop() {
        let mgr = TelemetryManager::new(TelemetryConfig {
            enabled: false,
            endpoint: "http://localhost:4317".to_string(),
            service_name: "shannon-code".to_string(),
            service_version: "0.0.0".to_string(),
            export_interval: Duration::from_secs(30),
            trace_export: true,
            metrics_export: true,
        });

        // All recording methods should be no-ops when disabled.
        mgr.record_query_start("q1", "test-model");
        mgr.record_query_end("q1", Duration::from_millis(100), 256);
        mgr.record_tool_call("bash", true, Duration::from_millis(50));
        mgr.record_api_call("anthropic", "claude-sonnet", 128, Duration::from_millis(200));
        mgr.record_error("api", "timeout");

        let m = mgr.metrics();
        assert_eq!(m.spans_created, 0);
        assert_eq!(m.events_emitted, 0);
        assert_eq!(m.errors_reported, 0);
        assert_eq!(m.tool_calls, 0);
        assert_eq!(m.api_calls, 0);
        assert_eq!(m.tokens_used, 0);
    }

    // -- Event recording tests ----------------------------------------------

    fn enabled_manager() -> TelemetryManager {
        TelemetryManager::new(TelemetryConfig {
            enabled: true,
            endpoint: "http://localhost:4317".to_string(),
            service_name: "shannon-code".to_string(),
            service_version: "0.0.0".to_string(),
            export_interval: Duration::from_secs(30),
            trace_export: true,
            metrics_export: true,
        })
    }

    #[test]
    fn test_record_query_start_increments_api_calls() {
        let mgr = enabled_manager();
        mgr.record_query_start("q1", "claude-sonnet");
        let m = mgr.metrics();
        assert_eq!(m.api_calls, 1);
        assert_eq!(m.events_emitted, 1);
    }

    #[test]
    fn test_record_query_end_increments_tokens() {
        let mgr = enabled_manager();
        mgr.record_query_end("q1", Duration::from_millis(120), 512);
        let m = mgr.metrics();
        assert_eq!(m.tokens_used, 512);
        assert_eq!(m.events_emitted, 1);
    }

    #[test]
    fn test_record_tool_call_increments_tool_counter() {
        let mgr = enabled_manager();
        mgr.record_tool_call("bash", true, Duration::from_millis(50));
        mgr.record_tool_call("grep", false, Duration::from_millis(10));
        let m = mgr.metrics();
        assert_eq!(m.tool_calls, 2);
        assert_eq!(m.events_emitted, 2);
    }

    #[test]
    fn test_record_api_call() {
        let mgr = enabled_manager();
        mgr.record_api_call("anthropic", "claude-sonnet", 256, Duration::from_millis(200));
        let m = mgr.metrics();
        assert_eq!(m.api_calls, 1);
        assert_eq!(m.tokens_used, 256);
        assert_eq!(m.events_emitted, 1);
    }

    #[test]
    fn test_record_error() {
        let mgr = enabled_manager();
        mgr.record_error("api", "timeout");
        mgr.record_error("tool", "permission_denied");
        let m = mgr.metrics();
        assert_eq!(m.errors_reported, 2);
        assert_eq!(m.events_emitted, 2);
    }

    #[test]
    fn test_metrics_snapshot_is_consistent() {
        let mgr = enabled_manager();

        mgr.record_query_start("q1", "model-a");
        mgr.record_query_end("q1", Duration::from_millis(100), 100);
        mgr.record_tool_call("bash", true, Duration::from_millis(10));
        mgr.record_api_call("anthropic", "model-a", 50, Duration::from_millis(50));
        mgr.record_error("api", "rate_limit");

        let m = mgr.metrics();
        // query_start -> 1 api_call + 1 event
        // query_end   -> 100 tokens + 1 event
        // tool_call   -> 1 tool_call + 1 event
        // api_call    -> 1 api_call + 50 tokens + 1 event
        // error       -> 1 error + 1 event
        assert_eq!(m.api_calls, 2);       // query_start + api_call
        assert_eq!(m.tokens_used, 150);   // 100 + 50
        assert_eq!(m.tool_calls, 1);
        assert_eq!(m.errors_reported, 1);
        assert_eq!(m.events_emitted, 5);
    }

    // -- Shutdown tests -----------------------------------------------------

    #[test]
    fn test_shutdown_is_clean() {
        let mgr = enabled_manager();
        assert!(mgr.shutdown().is_ok());

        // Second shutdown should fail.
        match mgr.shutdown() {
            Err(TelemetryError::AlreadyShutdown) => {}
            other => panic!("expected AlreadyShutdown, got {:?}", other),
        }
    }

    #[test]
    fn test_shutdown_stops_recording() {
        let mgr = enabled_manager();
        mgr.record_query_start("q1", "model-a");
        assert_eq!(mgr.metrics().api_calls, 1);

        mgr.shutdown().unwrap();

        // Recording after shutdown should be a no-op.
        mgr.record_query_start("q2", "model-b");
        assert_eq!(mgr.metrics().api_calls, 1);
    }

    // -- TelemetryLayer tests ------------------------------------------------

    #[test]
    fn test_telemetry_layer_captures_shannon_events() {
        use tracing_subscriber::prelude::*;

        let mgr = Arc::new(enabled_manager());
        let layer = TelemetryLayer::new(Arc::clone(&mgr));

        // Use a dispatcher-based approach: set the subscriber, emit events,
        // then restore.  We use `with_default` to scope the subscriber.
        let subscriber = tracing_subscriber::registry().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            // This event targets "shannon::test" — should be captured.
            tracing::info!(target: "shannon::test", "test event");
            // This event targets "other::crate" — should be ignored.
            tracing::info!(target: "other::crate", "other event");
        });

        let m = mgr.metrics();
        assert_eq!(m.events_emitted, 1); // only shannon:: event counted
    }

    #[test]
    fn test_telemetry_layer_captures_spans() {
        use tracing_subscriber::prelude::*;

        let mgr = Arc::new(enabled_manager());
        let layer = TelemetryLayer::new(Arc::clone(&mgr));

        let subscriber = tracing_subscriber::registry().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            // Spans are counted regardless of target.
            let _span = tracing::span!(target: "shannon::test", tracing::Level::INFO, "test_span").entered();
        });

        let m = mgr.metrics();
        assert_eq!(m.spans_created, 1);
    }

    // -- Concurrency test ---------------------------------------------------

    #[test]
    fn test_concurrent_increments() {
        let mgr = Arc::new(enabled_manager());
        let mut handles = vec![];

        for _ in 0..4 {
            let mgr = Arc::clone(&mgr);
            handles.push(thread::spawn(move || {
                for _ in 0..250 {
                    mgr.record_query_start("q", "model");
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(mgr.metrics().api_calls, 1000);
    }
}
