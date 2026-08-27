//! # L0 → OTLP Telemetry Bridge (§4.14)
//!
//! Rewrites the former atomic-counter `telemetry.rs` into a bridge that
//! folds the L0 session log into an OpenTelemetry span tree and a set of
//! projection-derived counters, exportable to any OTLP backend (Jaeger,
//! Grafana Tempo, Honeycomb, …).
//!
//! ## The Pi contract (research §遥测契约)
//!
//! - **NOOP by default**: with `SHANNON_TELEMETRY` unset, no exporter, SDK
//!   provider, thread, or task is ever constructed. Export calls cost one
//!   branch; telemetry can neither block nor fail the main loop.
//! - **Never blocks the engine**: when enabled, spans are handed to the
//!   SDK's batch processor (background delivery) and metric increments are
//!   lock-free counter adds. An unreachable endpoint degrades at
//!   construction time to an inert sink — it never panics, propagates, or
//!   stalls the caller.
//!
//! ## Span folding rules ([`build_span_tree`])
//!
//! A pure function from a log slice to a parents-first node list:
//!
//! - one root span `shannon.session` covering `[first ts, last ts]`,
//!   attributed with model/provider/version/cwd from `session/start`;
//! - one `shannon.turn` span per turn window (`first event` →
//!   `turn/end`, falling back to the last observed ts), carrying turn
//!   number, reason, usage, and cost attributes;
//! - one `shannon.tool.{name}` span per `tool/call`→`tool/result` pair
//!   under its owning turn span;
//! - explicit envelope ids win: an anchor event's `span_id` becomes the
//!   node id, and its `parent_span_id` becomes the node's parent whenever
//!   it matches another produced node id. Otherwise structural ids
//!   (`session-{id}`, `turn-{n}`, `tool-{tool_use_id}`) keep every tree
//!   deterministic;
//! - unpaired `tool/call`s close at the containing turn's end instead of
//!   being dropped, so interrupted turns still render;
//! - a bucket holding nothing but the `session/start` preamble folds into
//!   the root span rather than emitting an empty turn row.
//!
//! Metrics are projection counts: [`project_session_analytics`] feeds the
//! OTel counters on every export ("metrics 由投影计数").
//!
//! ## Switch matrix
//!
//! | Config / env                                | Behavior |
//! |---------------------------------------------|----------|
//! | `SHANNON_TELEMETRY` unset / not `1`         | NOOP — no sinks built at all |
//! | `=1` + `trace_export=true`                  | traces → OTLP gRPC `{endpoint}` |
//! | `=1` + `metrics_export=true`                | counters → OTLP gRPC every `export_interval` |
//! | endpoint unreachable                        | batch retries silently; engine never sees errors |
//!
//! ## Jaeger/Grafana acceptance demo
//!
//! A compose stack lives at `scripts/otel-demo/docker-compose.yml`:
//!
//! ```bash
//! docker compose -f scripts/otel-demo/docker-compose.yml up -d  # Jaeger UI :16686
//! # …produce sessions with SHANNON_TELEMETRY=1…
//! docker compose -f scripts/otel-demo/docker-compose.yml down   # teardown
//! ```
//!
//! Open <http://localhost:16686>, select service `shannon-code`: the
//! waterfall shows the full session → turn → tool tree.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, MeterProvider as _};
use opentelemetry::trace::{
    Span, SpanBuilder, TraceContextExt as _, Tracer as _, TracerProvider as _,
};
use opentelemetry_otlp::WithExportConfig;
use serde::{Deserialize, Serialize};
use shannon_types::session_event::{SessionEvent, SessionEventBody, TurnEndPayload};
use thiserror::Error;
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors that can occur during telemetry operations.
#[derive(Error, Debug)]
pub enum TelemetryError {
    /// Telemetry was constructed disabled (`SHANNON_TELEMETRY` off).
    #[error("Telemetry is disabled")]
    Disabled,

    /// Shutdown already happened; recording stays off.
    #[error("Telemetry already shut down")]
    AlreadyShutdown,

    /// Configuration rejected before any sink could be built.
    #[error("Configuration error: {0}")]
    Config(String),
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for Shannon telemetry. Defaults are NOOP; all other fields
/// only matter once `enabled` is true.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Whether telemetry collection is enabled (opt-in).
    pub enabled: bool,
    /// OTLP endpoint URL (default: `http://localhost:4317`, gRPC).
    pub endpoint: String,
    /// Logical service name (default: `shannon-code`).
    pub service_name: String,
    /// Service version (defaults to the crate version).
    pub service_version: String,
    /// Interval for periodic metric exports (when `metrics_export` is on).
    #[serde(default = "default_export_interval")]
    pub export_interval: Duration,
    /// Whether to export traces (the span tree).
    pub trace_export: bool,
    /// Whether to export metrics (projection counters).
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

        let service_name =
            std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "shannon-code".to_string());

        Self {
            enabled,
            endpoint,
            service_name,
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            export_interval: default_export_interval(),
            trace_export: true,
            metrics_export: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Span tree projection (pure function — unit-testable without a backend)
// ---------------------------------------------------------------------------

/// Attribute value kinds the tree projection emits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum SpanAttribute {
    /// Text attribute.
    Str(String),
    /// Integer attribute.
    Int(i64),
    /// Float attribute.
    Float(f64),
    /// Boolean attribute.
    Bool(bool),
}

impl From<&str> for SpanAttribute {
    fn from(v: &str) -> Self {
        SpanAttribute::Str(v.to_string())
    }
}
impl From<String> for SpanAttribute {
    fn from(v: String) -> Self {
        SpanAttribute::Str(v)
    }
}
impl From<i64> for SpanAttribute {
    fn from(v: i64) -> Self {
        SpanAttribute::Int(v)
    }
}

/// One node of the folded L0 span tree; parents precede children.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetrySpanNode {
    /// Stable span id (explicit envelope id when present, else synthetic).
    pub span_id: String,
    /// Parent id; `None` only for the session root.
    pub parent_span_id: Option<String>,
    /// Semantic name (`shannon.session` / `shannon.turn` /
    /// `shannon.tool.{name}`).
    pub name: String,
    /// Start timestamp, ns since epoch.
    pub start_ts_ns: u64,
    /// End timestamp, ns since epoch.
    pub end_ts_ns: u64,
    /// Ordered attributes.
    pub attributes: Vec<(String, SpanAttribute)>,
}

/// Root span name.
pub const SPAN_SESSION: &str = "shannon.session";
/// Turn span name.
pub const SPAN_TURN: &str = "shannon.turn";

/// Fold a slice of L0 events into the session → turn → tool span tree.
///
/// Pure function: no I/O, no global state, deterministic ids. Events must
/// be in seq order (they always are on disk). An empty slice yields an
/// empty vec; otherwise there is always exactly one root.
pub fn build_span_tree(events: &[SessionEvent]) -> Vec<TelemetrySpanNode> {
    let Some(first) = events.first() else {
        return Vec::new();
    };
    let last_ts = events.iter().map(|e| e.ts_ns).max().unwrap_or(first.ts_ns);
    let session_id = &first.session_id;

    // --- session/start payload + root identity ------------------------------
    let start_payload = events.iter().find_map(|e| match &e.body {
        SessionEventBody::SessionStart(p) => Some(p),
        _ => None,
    });
    let root_id = events
        .iter()
        .find(|e| matches!(e.body, SessionEventBody::SessionStart(_)))
        .and_then(|e| e.span_id.clone())
        .unwrap_or_else(|| format!("session-{session_id}"));
    let mut root_attrs: Vec<(String, SpanAttribute)> =
        vec![("shannon.session_id".into(), session_id.clone().into())];
    if let Some(p) = start_payload {
        root_attrs.push(("shannon.model".into(), p.model.clone().into()));
        for (key, value) in [
            ("shannon.provider", &p.provider),
            ("shannon.version", &p.app_version),
            ("shannon.cwd", &p.cwd),
        ] {
            if let Some(v) = value {
                root_attrs.push((key.into(), v.clone().into()));
            }
        }
    }

    // --- group events by turn number, preserving first-seen order ----------
    let mut turn_order: Vec<u64> = Vec::new();
    let mut per_turn: HashMap<u64, Vec<&SessionEvent>> = HashMap::new();
    for event in events {
        per_turn
            .entry(event.turn)
            .or_insert_with(|| {
                turn_order.push(event.turn);
                Vec::new()
            })
            .push(event);
    }

    // Buckets holding nothing but the session preamble (e.g. the
    // `session/start` row itself) fold into the root span instead of
    // producing an empty `shannon.turn` row.
    let mut out: Vec<TelemetrySpanNode> = vec![TelemetrySpanNode {
        parent_span_id: None,
        span_id: root_id.clone(),
        name: SPAN_SESSION.to_string(),
        start_ts_ns: first.ts_ns.min(last_ts),
        end_ts_ns: last_ts.max(first.ts_ns),
        attributes: root_attrs,
    }];

    let mut known_ids: Vec<String> = vec![root_id.clone()];

    for &n in &turn_order {
        if per_turn[&n]
            .iter()
            .all(|e| matches!(e.body, SessionEventBody::SessionStart(_)))
        {
            continue;
        }
        let evs = &per_turn[&n];
        let turn_start = evs.first().expect("bucket non-empty by construction").ts_ns;

        // The turn boundary anchors explicit-id lookups (start preferred).
        let anchor = evs
            .iter()
            .find(|e| matches!(e.body, SessionEventBody::TurnStart(_)))
            .or_else(|| evs.first());

        let turn_end_ev = evs
            .iter()
            .rev()
            .find(|e| matches!(e.body, SessionEventBody::TurnEnd(_)));
        let turn_end = turn_end_ev.map_or(last_ts.max(turn_start), |e| e.ts_ns);

        let turn_id = anchor
            .and_then(|e| e.span_id.clone())
            .unwrap_or_else(|| format!("turn-{n}"));

        // Structural parent is the root unless the anchor's explicit
        // parent_span_id names another node we actually produced.
        let parent = anchor
            .and_then(|e| e.parent_span_id.as_ref())
            .filter(|pid| known_ids.contains(pid))
            .cloned()
            .unwrap_or_else(|| root_id.clone());

        let mut attrs: Vec<(String, SpanAttribute)> =
            vec![("shannon.turn".into(), SpanAttribute::Int(n as i64))];
        if let Some(SessionEventBody::TurnEnd(TurnEndPayload { reason, usage, .. })) =
            turn_end_ev.map(|e| &e.body)
        {
            attrs.push(("shannon.turn_reason".into(), reason.clone().into()));
            if let Some(u) = usage {
                attrs.push((
                    "shannon.input_tokens".into(),
                    SpanAttribute::Int(u.input_tokens as i64),
                ));
                attrs.push((
                    "shannon.output_tokens".into(),
                    SpanAttribute::Int(u.output_tokens as i64),
                ));
                if let Some(cost) = u.cost_usd {
                    attrs.push(("shannon.cost_usd".into(), SpanAttribute::Float(cost)));
                }
            }
        }

        out.push(TelemetrySpanNode {
            parent_span_id: Some(parent),
            span_id: turn_id.clone(),
            name: SPAN_TURN.to_string(),
            start_ts_ns: turn_start.min(turn_end),
            end_ts_ns: turn_end.max(turn_start),
            attributes: attrs,
        });
        known_ids.push(turn_id.clone());

        // Tools under this turn: pair calls with results by tool_use_id.
        let mut open_calls: Vec<(&SessionEvent, Option<&SessionEvent>)> = Vec::new();
        for ev in evs {
            match &ev.body {
                SessionEventBody::ToolCall(_) => open_calls.push((ev, None)),
                SessionEventBody::ToolResult(res) => {
                    let slot = open_calls
                        .iter_mut()
                        .rev()
                        .find(|(call, done)| {
                            done.is_none()
                                && matches!(
                                    &call.body,
                                    SessionEventBody::ToolCall(c) if c.tool_use_id == res.tool_use_id
                                )
                        })
                        .map(|(_, done)| done);
                    if let Some(done) = slot {
                        *done = Some(ev);
                    }
                }
                _ => {}
            }
        }

        for (call_ev, result_ev) in &open_calls {
            let SessionEventBody::ToolCall(call) = &call_ev.body else {
                continue;
            };
            let result_end = result_ev.map_or(turn_end, |r| r.ts_ns);
            let mut tattrs: Vec<(String, SpanAttribute)> = vec![
                ("shannon.tool_name".into(), call.tool_name.clone().into()),
                (
                    "shannon.tool_use_id".into(),
                    call.tool_use_id.clone().into(),
                ),
            ];
            if let Some(r) = result_ev {
                if let SessionEventBody::ToolResult(p) = &r.body {
                    tattrs.push(("shannon.tool_error".into(), SpanAttribute::Bool(p.is_error)));
                    let measured = p
                        .duration_ms
                        .unwrap_or_else(|| r.ts_ns.saturating_sub(call_ev.ts_ns) / 1_000_000);
                    tattrs.push((
                        "shannon.duration_ms".into(),
                        SpanAttribute::Int(measured as i64),
                    ));
                }
            } else {
                // Unpaired call (interrupted): mark explicitly, don't drop.
                tattrs.push(("shannon.tool_error".into(), SpanAttribute::Bool(true)));
            }

            let tool_parent = call_ev
                .parent_span_id
                .as_ref()
                .filter(|pid| known_ids.contains(pid))
                .cloned()
                .unwrap_or_else(|| turn_id.clone());
            let tool_id = call_ev
                .span_id
                .clone()
                .unwrap_or_else(|| format!("tool-{}", call.tool_use_id));

            out.push(TelemetrySpanNode {
                parent_span_id: Some(tool_parent),
                span_id: tool_id,
                name: format!("{SPAN_TOOL_PREFIX_NAME}{}", call.tool_name),
                start_ts_ns: call_ev.ts_ns.min(result_end),
                end_ts_ns: result_end.max(call_ev.ts_ns),
                attributes: tattrs,
            });
        }

        // Tool nodes also register their ids for later explicit links.
        for (call_ev, _) in &open_calls {
            if let Some(id) = call_ev.span_id.as_ref() {
                known_ids.push(id.clone());
            }
        }
    }

    out
}

/// Prefix for tool spans; full name is `{SPAN_TOOL_PREFIX}{tool_name}`.
const SPAN_TOOL_PREFIX_NAME: &str = "shannon.tool.";

fn ns_to_system_time(ns: u64) -> SystemTime {
    UNIX_EPOCH
        .checked_add(Duration::from_nanos(ns))
        .unwrap_or(UNIX_EPOCH)
}

// ---------------------------------------------------------------------------
// Metrics snapshot
// ---------------------------------------------------------------------------

/// Point-in-time stats of this manager's export activity (introspection +
/// tests). The semantic counters themselves go straight into OTel.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TelemetryStats {
    /// Sessions handed to the bridge.
    pub exports: u64,
    /// Spans queued into the trace sink.
    pub spans_emitted: u64,
    /// Sinks that failed construction and degraded to inert.
    pub degraded_sinks: u64,
}

// ---------------------------------------------------------------------------
// Sinks
// ---------------------------------------------------------------------------

/// Trace side: provider + tracer over one batch exporter.
struct TraceSink {
    provider: opentelemetry_sdk::trace::SdkTracerProvider,
    tracer: opentelemetry_sdk::trace::Tracer,
}

/// Metric counters pushed once per exported session.
struct MetricCounters {
    turns_completed: Counter<u64>,
    prompts_submitted: Counter<u64>,
    tool_calls: Counter<u64>,
    tool_failures: Counter<u64>,
    output_tokens: Counter<u64>,
    errors: Counter<u64>,
}

/// Metric side: provider + pre-built projection counters.
struct MetricSink {
    provider: opentelemetry_sdk::metrics::SdkMeterProvider,
    counters: MetricCounters,
}

/// Everything the bridge can talk to once enabled. Any member is optional:
/// a failed construction degrades that signal to inert instead of failing
/// the whole bridge.
struct Exporters {
    traces: Option<TraceSink>,
    metrics: Option<MetricSink>,
}

impl Drop for Exporters {
    fn drop(&mut self) {
        // Providers own background workers; shutting them down here keeps
        // drop-order sane when a manager is discarded without shutdown().
        if let Some(t) = self.traces.take() {
            let _ = t.provider.shutdown();
        }
        if let Some(m) = self.metrics.take() {
            let _ = m.provider.shutdown();
        }
    }
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// Manages OpenTelemetry-compatible telemetry for Shannon Code: folds L0
/// event slices into a span tree (traces) and pushes analytics-projection
/// counters (metrics). NOOP unless [`TelemetryConfig::enabled`].
pub struct TelemetryManager {
    config: TelemetryConfig,
    exporters: Option<Exporters>,
    shutdown: AtomicU64,
    stats_exports: AtomicU64,
    stats_spans: AtomicU64,
    stats_degraded: AtomicU64,
}

impl TelemetryManager {
    /// Create a manager honoring `config`. With `enabled: false` (the
    /// default path) this builds **nothing** — see the module docs for the
    /// switch matrix. Enabled sinks that fail construction (bad endpoint,
    /// runtime unavailable) degrade to inert rather than erroring out.
    pub fn new(config: TelemetryConfig) -> Self {
        let exporters = if config.enabled {
            match Self::build_otlp_sinks(&config) {
                Ok(sinks) => {
                    debug!(
                        endpoint = %config.endpoint,
                        traces = config.trace_export,
                        metrics = config.metrics_export,
                        "Shannon OTLP sinks constructed"
                    );
                    Some(sinks)
                }
                Err(e) => {
                    warn!(error = %e, "telemetry sink construction failed; degrading to NOOP");
                    let degraded =
                        u64::from(config.trace_export) + u64::from(config.metrics_export);
                    return Self::finish(config, None, 0, 0, degraded);
                }
            }
        } else {
            None
        };

        Self::finish(config, exporters, 0, 0, 0)
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        config: TelemetryConfig,
        exporters: Option<Exporters>,
        exports: u64,
        spans: u64,
        degraded: u64,
    ) -> Self {
        Self {
            exporters,
            shutdown: AtomicU64::new(0),
            stats_exports: AtomicU64::new(exports),
            stats_spans: AtomicU64::new(spans),
            stats_degraded: AtomicU64::new(degraded),
            config,
        }
    }

    /// Test/embedding seam: like [`Self::new`] but the trace sink runs over
    /// the caller-supplied exporter through a **simple** processor (flush
    /// synchronous at span end), making export assertions deterministic.
    /// Metrics are never constructed here.
    pub fn with_trace_exporter(
        mut config: TelemetryConfig,
        exporter: impl opentelemetry_sdk::trace::SpanExporter + 'static,
    ) -> Self {
        config.enabled = true;
        let resource = Self::resource_for(&config);
        let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .with_resource(resource)
            .with_simple_exporter(exporter)
            .build();
        let tracer = provider.tracer(config.service_name.clone());
        Self::finish(
            config,
            Some(Exporters {
                traces: Some(TraceSink { provider, tracer }),
                metrics: None,
            }),
            0,
            0,
            0,
        )
    }

    fn resource_for(config: &TelemetryConfig) -> opentelemetry_sdk::Resource {
        opentelemetry_sdk::Resource::builder_empty()
            .with_service_name(config.service_name.clone())
            .with_attribute(KeyValue::new(
                "service.version",
                config.service_version.clone(),
            ))
            .build()
    }

    /// Reference to the active configuration.
    pub fn config(&self) -> &TelemetryConfig {
        &self.config
    }

    /// Whether telemetry collection was requested (opt-in flag).
    pub fn is_enabled(&self) -> bool {
        self.config.enabled && self.shutdown.load(Ordering::SeqCst) == 0
    }

    /// True when at least one live sink exists.
    pub fn has_live_sink(&self) -> bool {
        self.exporters.is_some()
    }

    /// Point-in-time export statistics.
    pub fn stats(&self) -> TelemetryStats {
        TelemetryStats {
            exports: self.stats_exports.load(Ordering::Relaxed),
            spans_emitted: self.stats_spans.load(Ordering::Relaxed),
            degraded_sinks: self.stats_degraded.load(Ordering::Relaxed),
        }
    }

    // -----------------------------------------------------------------------
    // The L0 → OTLP bridge entry point
    // -----------------------------------------------------------------------

    /// Fold one finished L0 log slice through the bridge: emits the span
    /// tree into the trace sink (background delivery) and increments the
    /// projection counters in the metric sink.
    ///
    /// Never blocks on network I/O; never fails. Returns the number of
    /// spans queued (0 when disabled/degraded/shut down).
    pub fn export_l0(&self, events: &[SessionEvent]) -> usize {
        if !self.is_enabled() || !self.has_live_sink() {
            return 0;
        }
        let mut emitted = 0usize;

        if let Some(sink) = self.exporters.as_ref().and_then(|e| e.traces.as_ref()) {
            emitted = emit_span_tree(sink, events);
            self.stats_spans
                .fetch_add(emitted as u64, Ordering::Relaxed);
        }

        if let Some(sink) = self.exporters.as_ref().and_then(|e| e.metrics.as_ref()) {
            record_projection_metrics(sink, events);
        }

        self.stats_exports.fetch_add(1, Ordering::Relaxed);
        emitted
    }

    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// Gracefully flush and shut down every sink. After shutdown all export
    /// methods become no-ops. Idempotent-by-error like the old API: a
    /// second call reports [`TelemetryError::AlreadyShutdown`].
    pub fn shutdown(&self) -> Result<(), TelemetryError> {
        if self
            .shutdown
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(TelemetryError::AlreadyShutdown);
        }
        if let Some(exporters) = &self.exporters {
            if let Some(trace) = &exporters.traces {
                let _ = trace.provider.force_flush();
            }
            if let Some(metric) = &exporters.metrics {
                let _ = metric.provider.force_flush();
            }
            if self.config.enabled {
                let s = self.stats();
                info!(
                    exports = s.exports,
                    spans = s.spans_emitted,
                    "Shannon telemetry shutdown"
                );
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Sink construction (the only place real endpoints appear)
    // -----------------------------------------------------------------------

    fn build_otlp_sinks(config: &TelemetryConfig) -> Result<Exporters, String> {
        let resource = Self::resource_for(config);

        let traces = if config.trace_export {
            let exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(&config.endpoint)
                .build()
                .map_err(|e| format!("span exporter build failed: {e}"))?;
            let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_resource(resource.clone())
                .with_batch_exporter(exporter)
                .build();
            let tracer = provider.tracer(config.service_name.clone());
            Some(TraceSink { provider, tracer })
        } else {
            None
        };

        let metrics = if config.metrics_export {
            let exporter = opentelemetry_otlp::MetricExporter::builder()
                .with_tonic()
                .with_endpoint(&config.endpoint)
                .build()
                .map_err(|e| format!("metric exporter build failed: {e}"))?;
            let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(exporter)
                .with_interval(config.export_interval)
                .build();
            let meter_provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
                .with_resource(resource)
                .with_reader(reader)
                .build();
            let meter = meter_provider.meter("shannon-code");
            Some(MetricSink {
                counters: build_counters(&meter),
                provider: meter_provider,
            })
        } else {
            None
        };

        Ok(Exporters { traces, metrics })
    }
}

fn build_counters(meter: &opentelemetry::metrics::Meter) -> MetricCounters {
    MetricCounters {
        turns_completed: meter
            .u64_counter("shannon.turns.completed")
            .with_description("Completed turns")
            .build(),
        prompts_submitted: meter
            .u64_counter("shannon.prompts.submitted")
            .with_description("User prompts submitted")
            .build(),
        tool_calls: meter
            .u64_counter("shannon.tool.calls")
            .with_description("Tool invocations")
            .build(),
        tool_failures: meter
            .u64_counter("shannon.tool.failures")
            .with_description("Tool invocations that errored")
            .build(),
        output_tokens: meter
            .u64_counter("shannon.tokens.output")
            .with_description("Output tokens generated")
            .build(),
        errors: meter
            .u64_counter("shannon.errors")
            .with_description("Errors by category")
            .build(),
    }
}

/// Emit `events`' span tree through `sink`; returns the queued span count.
fn emit_span_tree(sink: &TraceSink, events: &[SessionEvent]) -> usize {
    let nodes = build_span_tree(events);
    let mut contexts: HashMap<String, opentelemetry::Context> = HashMap::with_capacity(nodes.len());
    let mut emitted = 0usize;

    for node in &nodes {
        // Parents-first ordering lets us resolve every declared parent.
        let parent_cx = match node
            .parent_span_id
            .as_ref()
            .and_then(|pid| contexts.get(pid))
        {
            Some(cx) => cx.clone(),
            None => opentelemetry::Context::new(),
        };
        // Owned Cow name: span names are dynamic (`shannon.tool.{name}`).
        let builder = SpanBuilder {
            name: node.name.clone().into(),
            start_time: Some(ns_to_system_time(node.start_ts_ns)),
            attributes: Some(
                node.attributes
                    .iter()
                    .map(|(k, v)| {
                        KeyValue::new(
                            k.clone(),
                            match v {
                                SpanAttribute::Str(s) => {
                                    opentelemetry::Value::String(s.clone().into())
                                }
                                SpanAttribute::Int(i) => opentelemetry::Value::I64(*i),
                                SpanAttribute::Float(f) => opentelemetry::Value::F64(*f),
                                SpanAttribute::Bool(b) => opentelemetry::Value::Bool(*b),
                            },
                        )
                    })
                    .collect(),
            ),
            ..Default::default()
        };
        let mut span = sink.tracer.build_with_context(builder, &parent_cx);
        span.end_with_timestamp(ns_to_system_time(node.end_ts_ns));
        contexts.insert(node.span_id.clone(), parent_cx.with_span(span));
        emitted += 1;
    }
    emitted
}

/// Push the analytics projection's totals into the metric counters.
fn record_projection_metrics(sink: &MetricSink, events: &[SessionEvent]) {
    let view = crate::session_log::project_session_analytics(events);
    let c = &sink.counters;
    c.turns_completed.add(view.turns_completed, &[]);
    c.prompts_submitted.add(view.prompts_submitted, &[]);
    c.output_tokens.add(view.response_output_tokens, &[]);
    let mut total_calls = 0u64;
    let mut total_failures = 0u64;
    for agg in view.tools.values() {
        total_calls += agg.calls;
        total_failures += agg.failures;
    }
    c.tool_calls.add(total_calls, &[]);
    c.tool_failures.add(total_failures, &[]);
    let total_errors: u64 = view.errors.values().sum();
    c.errors.add(total_errors, &[]);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_types::session_event::{
        AssistantChunkPayload, TokenUsage, ToolCallPayload, ToolResultPayload, TurnStartPayload,
        UserMessagePayload,
    };

    const SID: &str = "sess-tel";
    const NS: u64 = 1_756_200_000_000_000_000;

    fn ev(seq: u64, turn: u64, body: SessionEventBody) -> SessionEvent {
        SessionEvent::new(seq, NS + seq * 10_000_000, SID, turn, body)
    }

    fn user(seq: u64, turn: u64) -> SessionEvent {
        ev(
            seq,
            turn,
            SessionEventBody::UserMessage(UserMessagePayload {
                source: UserMessagePayload::SOURCE_USER.into(),
                content: "hi".into(),
            }),
        )
    }

    fn chunk(seq: u64, turn: u64) -> SessionEvent {
        ev(
            seq,
            turn,
            SessionEventBody::AssistantChunk(AssistantChunkPayload {
                delta: "yo".into(),
                thinking: false,
            }),
        )
    }

    fn call(seq: u64, turn: u64, id: &str, tool: &str) -> SessionEvent {
        ev(
            seq,
            turn,
            SessionEventBody::ToolCall(ToolCallPayload {
                tool_use_id: id.into(),
                tool_name: tool.into(),
                arguments: "{}".into(),
            }),
        )
    }

    fn result(seq: u64, turn: u64, id: &str, tool: &str, is_error: bool) -> SessionEvent {
        ev(
            seq,
            turn,
            SessionEventBody::ToolResult(ToolResultPayload {
                tool_use_id: id.into(),
                tool_name: tool.into(),
                output: "ok".into(),
                is_error,
                duration_ms: Some(42),
                meta: serde_json::json!({}),
            }),
        )
    }

    fn turn_start(seq: u64, turn: u64) -> SessionEvent {
        ev(
            seq,
            turn,
            SessionEventBody::TurnStart(TurnStartPayload { query_id: None }),
        )
    }

    fn turn_end(seq: u64, turn: u64, out_tokens: u64) -> SessionEvent {
        ev(
            seq,
            turn,
            SessionEventBody::TurnEnd(TurnEndPayload {
                reason: TurnEndPayload::REASON_COMPLETED.into(),
                usage: Some(TokenUsage {
                    input_tokens: 10,
                    output_tokens: out_tokens,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    cost_usd: Some(0.25),
                }),
                error: None,
            }),
        )
    }

    fn two_turn_fixture() -> Vec<SessionEvent> {
        vec![
            ev(
                0,
                0,
                SessionEventBody::SessionStart(shannon_types::session_event::SessionStartPayload {
                    model: "claude-sonnet-4".into(),
                    provider: Some("anthropic".into()),
                    cwd: None,
                    app_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                }),
            ),
            turn_start(1, 1),
            user(2, 1),
            chunk(3, 1),
            call(4, 1, "t1", "Bash"),
            result(5, 1, "t1", "Bash", false),
            turn_end(6, 1, 100),
            turn_start(7, 2),
            user(8, 2),
            call(9, 2, "t2", "Read"),
            result(10, 2, "t2", "Read", true),
            call(11, 2, "t3", "Grep"),
            // t3 never returns a result (interrupted) on purpose.
            turn_end(12, 2, 55),
        ]
    }

    // -- pure tree shape -----------------------------------------------------

    #[test]
    fn test_empty_slice_yields_no_spans() {
        assert!(build_span_tree(&[]).is_empty());
    }

    #[test]
    fn test_span_tree_shape_root_turns_tools_parent_chain() {
        let tree = build_span_tree(&two_turn_fixture());
        // root + 2 turns + 3 tools = 6 nodes, parents strictly before children.
        assert_eq!(tree.len(), 6);

        let root = &tree[0];
        assert_eq!(root.name, SPAN_SESSION);
        assert_eq!(root.parent_span_id, None);
        assert_eq!(root.span_id, "session-sess-tel");
        assert!(
            root.attributes
                .contains(&("shannon.model".to_string(), "claude-sonnet-4".into()))
        );
        // Root window covers the whole log window.
        assert_eq!(
            root.end_ts_ns,
            tree.iter().map(|n| n.end_ts_ns).max().unwrap()
        );

        let positions: HashMap<String, usize> = tree
            .iter()
            .enumerate()
            .map(|(i, n)| (n.span_id.clone(), i))
            .collect();
        let idx = |id: &str| *positions.get(id).expect("id present");

        // Turn rows follow the root; tool children follow their turn.
        assert!(idx("turn-1") > idx("session-sess-tel"));
        assert!(idx("tool-t1") > idx("turn-1"));
        assert!(idx("turn-2") > idx("tool-t1"), "second turn after first");
        assert!(idx("tool-t3") > idx("turn-2"));

        // Every non-root node resolves its parent to an earlier node.
        for (i, node) in tree.iter().enumerate().skip(1) {
            let pid = node.parent_span_id.as_ref().expect("non-root parent");
            let pi = idx(pid);
            assert!(pi < i, "parent {} of {} must precede it", pid, node.span_id);
        }

        // Attributes: tool error flags, durations, usage/cost on turns.
        let t1 = &tree[idx("tool-t1")];
        assert_eq!(t1.name, "shannon.tool.Bash");
        assert!(
            t1.attributes
                .contains(&("shannon.tool_error".to_string(), SpanAttribute::Bool(false)))
        );
        assert!(
            t1.attributes
                .contains(&("shannon.duration_ms".to_string(), SpanAttribute::Int(42)))
        );

        let t2 = &tree[idx("tool-t2")];
        assert!(
            t2.attributes
                .contains(&("shannon.tool_error".to_string(), SpanAttribute::Bool(true)))
        );

        let interrupted = &tree[idx("tool-t3")];
        assert!(
            interrupted
                .attributes
                .contains(&("shannon.tool_error".to_string(), SpanAttribute::Bool(true)))
        );

        let turn1 = &tree[idx("turn-1")];
        assert!(
            turn1
                .attributes
                .contains(&("shannon.output_tokens".to_string(), SpanAttribute::Int(100)))
        );
        assert!(
            turn1
                .attributes
                .contains(&("shannon.cost_usd".to_string(), SpanAttribute::Float(0.25)))
        );
        assert!(
            turn1
                .attributes
                .contains(&("shannon.turn_reason".to_string(), "completed".into()))
        );

        // Turn parents default to the root.
        assert_eq!(
            tree[idx("turn-1")].parent_span_id.as_deref(),
            Some("session-sess-tel")
        );
        assert_eq!(
            tree[idx("turn-2")].parent_span_id.as_deref(),
            Some("session-sess-tel")
        );
    }

    #[test]
    fn test_explicit_envelope_ids_override_structural_ids() {
        let mut events = two_turn_fixture();
        // Give turn 1 an explicit span id + parent pointing at the root.
        events[1].span_id = Some("custom-turn-a".into());
        events[1].parent_span_id = Some("session-sess-tel".into());
        // Tool t1 declares its own span id too.
        events[4].span_id = Some("custom-tool-bash".into());
        // Dangling parent reference falls back to the structural owner.
        events[9].span_id = Some("custom-tool-read".into());
        events[9].parent_span_id = Some("nope-missing".into());

        let tree = build_span_tree(&events);
        let get = |id: &str| tree.iter().find(|n| n.span_id == id).expect("node");

        assert_eq!(get("custom-turn-a").name, SPAN_TURN);
        assert_eq!(
            get("custom-turn-a").parent_span_id.as_deref(),
            Some("session-sess-tel")
        );
        // The tool child hangs off the custom turn id (its owner).
        assert_eq!(
            get("custom-tool-bash").parent_span_id.as_deref(),
            Some("custom-turn-a")
        );
        // Dangling explicit parent => structural fallback to owning turn.
        assert_eq!(
            get("custom-tool-read").parent_span_id.as_deref(),
            Some("turn-2")
        );
        // And no duplicated synthetic `turn-1` row remains.
        assert!(tree.iter().all(|n| n.span_id != "turn-1"));
    }

    // -- in-memory OTLP assertion (the automated "Jaeger" check) ---------------

    #[test]
    fn test_in_memory_receiver_sees_full_session_turn_tool_span_tree() {
        use opentelemetry_sdk::trace::InMemorySpanExporterBuilder;

        let exporter = InMemorySpanExporterBuilder::new().build();
        let mgr = TelemetryManager::with_trace_exporter(
            TelemetryConfig {
                enabled: true,
                trace_export: true,
                metrics_export: false,
                ..TelemetryConfig::default()
            },
            exporter.clone(),
        );
        assert!(mgr.is_enabled() && mgr.has_live_sink());

        let queued = mgr.export_l0(&two_turn_fixture());
        assert_eq!(queued, 6, "root + 2 turns + 3 tools");

        let finished = exporter
            .get_finished_spans()
            .expect("in-memory receiver drain");
        assert_eq!(finished.len(), 6, "every span reached the receiver");

        // Name-level shape: exactly one session root, two turn rows.
        let names: Vec<&str> = finished.iter().map(|s| s.name.as_ref()).collect();
        assert_eq!(names.iter().filter(|n| **n == SPAN_SESSION).count(), 1);
        assert_eq!(names.iter().filter(|n| **n == SPAN_TURN).count(), 2);
        for tool in [
            "shannon.tool.Bash",
            "shannon.tool.Read",
            "shannon.tool.Grep",
        ] {
            assert_eq!(names.iter().filter(|n| **n == tool).count(), 1, "{tool}");
        }

        // Parent-chain correctness on the wire: id(span) ↔ parent ids form
        // the same tree the projection promised (session root → turn → tool).
        // Note: the SDK meter name is `&'static str`; the tracer-side
        // instrumentation scope carries the configurable service name, so a
        // fixed meter name is acceptable (counters are namespaced anyway).
        let by_name = |name: &str| {
            finished
                .iter()
                .find(|s| s.name.as_ref() == name)
                .expect("span present")
        };
        let turn2 = by_name(SPAN_TURN);
        let all_turns: Vec<_> = finished
            .iter()
            .filter(|s| s.name.as_ref() == SPAN_TURN)
            .collect();
        for turn in &all_turns {
            let parent = finished
                .iter()
                .find(|s| s.span_context.span_id() == turn.parent_span_id)
                .expect("turn parent present");
            assert_eq!(parent.name.as_ref(), SPAN_SESSION);
            assert_eq!(
                parent.parent_span_id,
                opentelemetry::trace::SpanId::INVALID,
                "root is top"
            );
        }
        let _ = turn2;

        let grep = by_name("shannon.tool.Grep");
        let owner = finished
            .iter()
            .find(|s| s.span_context.span_id() == grep.parent_span_id)
            .expect("tool parent present");
        assert_eq!(owner.name.as_ref(), SPAN_TURN);

        // Stats bookkeeping agrees with what the receiver observed.
        let stats = mgr.stats();
        assert_eq!(stats.exports, 1);
        assert_eq!(stats.spans_emitted, 6);

        // Second export appends; shutdown then makes the bridge inert.
        assert_eq!(mgr.export_l0(&two_turn_fixture()), 6);
        assert_eq!(
            exporter.get_finished_spans().unwrap().len(),
            12,
            "simple processor flushes synchronously"
        );
        mgr.shutdown().expect("first shutdown ok");
        assert!(
            mgr.shutdown().is_err(),
            "second shutdown reports AlreadyShutdown"
        );
        assert_eq!(mgr.export_l0(&two_turn_fixture()), 0, "post-shutdown noop");
    }

    // -- NOOP-default proof ---------------------------------------------------

    #[test]
    fn test_disabled_manager_is_pure_noop_and_builds_nothing() {
        let mgr = TelemetryManager::new(TelemetryConfig {
            enabled: false,
            ..TelemetryConfig::default()
        });
        assert!(!mgr.is_enabled());
        assert!(!mgr.has_live_sink(), "disabled => zero sinks constructed");
        assert_eq!(mgr.export_l0(&two_turn_fixture()), 0);
        let s = mgr.stats();
        assert_eq!((s.exports, s.spans_emitted, s.degraded_sinks), (0, 0, 0));
        assert!(mgr.shutdown().is_ok());
    }
}
