//! # Session Log Tee (§4.2, W1-P0b)
//!
//! The bypass tee that mirrors every engine [`QueryEvent`] into the L0
//! session log as it is broadcast — "model-visible == recorded". The engine
//! routes its event channel through [`TeeHandle`] at a single injection
//! point; the tee maps, redacts, truncates, and appends.
//!
//! - **Never fails the session**: a disabled or degraded tee (env switch,
//!   lock conflict, write failure) records nothing and stays silent. All
//!   write failures degrade inside [`SessionLogWriter::record`].
//! - **Usage folding**: `Usage` events (full token/cost/cache triple) are
//!   folded into the next `turn/end` instead of being logged 1:1.
//! - **Request headers**: [`SessionTee::record_request_header`] receives the
//!   *exact* adapter-serialized request body (captured by `LlmClient` at
//!   serialization time) and appends it verbatim as the `wire_body` of a
//!   `request/header` event, so the log can byte-reconstruct every request.
//!   Headers are exempt from the payload truncation limit.
//! - **Truncation**: single-event payloads over [`MAX_EVENT_BYTES`] keep the
//!   first [`TRUNCATION_HEAD_BYTES`] plus a `[shannon:truncated …]` marker
//!   carrying the original length and sha256 of the full content.
//! - **Redaction at write time** (§4.14): every string passes through one
//!   immutable [`RedactionPolicy`] snapshot — built-in token prefixes,
//!   user-configured prefixes/regexes/values from
//!   `<shannon-home>/redaction.toml`, and env-secret values. Disk is clean;
//!   no post-hoc scrubbing exists or is needed.
//!
//! ## Switch and storage
//!
//! Logging is on by default. Set `SHANNON_SESSION_LOG=off` to disable. Logs
//! live at `<SHANNON_HOME or ~/.shannon>/sessions/<session_id>/events.jsonl`;
//! delete that file (or the whole `<session_id>` directory) to clear a
//! session's log. `SHANNON_HOME=<dir>` relocates the root for tests.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use sha2::{Digest, Sha256};
use shannon_types::session_event::{
    RequestHeaderPayload, SessionEventBody, SessionStartPayload, TokenUsage, ToolManifestEntry,
    TurnEndPayload, TurnStartPayload, UserMessagePayload,
};
use tracing::warn;

use super::redaction::RedactionPolicy;
use super::{SessionLogWriter, query_event_to_session_body, token_usage_from_event};
use crate::QueryEvent;

// Compat surface: the masking entry points moved to `super::redaction`;
// re-exported so `session_log::tee::{redact_string, redact_value,
// redact_with_secrets}` keeps resolving for any caller.
pub use super::redaction::{redact_string, redact_value, redact_with_secrets};

/// Payloads larger than this are truncated (request headers exempt).
pub const MAX_EVENT_BYTES: usize = 256 * 1024;
/// How much of an oversized payload string is kept.
pub const TRUNCATION_HEAD_BYTES: usize = 64 * 1024;

/// Env var that disables the tee entirely (`off`, `0`, or `false`).
pub const SWITCH_ENV: &str = "SHANNON_SESSION_LOG";

// ============================================================================
// Redaction (§4.14) — the policy lives in [`super::redaction`]; the tee
// snapshots one immutable instance per query at open time.
// ============================================================================

// ============================================================================
// Truncation
// ============================================================================

/// sha256 of `data`, hex-encoded.
fn sha256_hex(data: &str) -> String {
    let digest = Sha256::digest(data.as_bytes());
    hex::encode(digest)
}

/// Keep the head of an oversized string plus a marker with the original
/// length and content hash. Cutting on a char boundary keeps the log valid
/// UTF-8 JSON even when the cut lands inside a multi-byte character.
fn truncate_string(s: &str) -> String {
    if s.len() <= TRUNCATION_HEAD_BYTES {
        return s.to_string();
    }
    let mut cut = TRUNCATION_HEAD_BYTES;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let head = &s[..cut];
    format!(
        "{head}\n[shannon:truncated original_bytes={} sha256={}]",
        s.len(),
        sha256_hex(s)
    )
}

/// Serialized size of a body line.
fn body_len(body: &SessionEventBody) -> usize {
    serde_json::to_string(body).map(|s| s.len()).unwrap_or(0)
}

/// Enforce the payload size limit on the string-carrying kinds. Small
/// payloads pass through untouched; oversized ones keep a 64KB head plus the
/// truncation marker. `request/header` is exempt by design (plan §4.2).
fn enforce_size_limit(mut body: SessionEventBody) -> SessionEventBody {
    if body_len(&body) <= MAX_EVENT_BYTES {
        return body;
    }
    match &mut body {
        SessionEventBody::UserMessage(p) => p.content = truncate_string(&p.content),
        SessionEventBody::AssistantChunk(p) => p.delta = truncate_string(&p.delta),
        SessionEventBody::AssistantMessage(p) => p.content = truncate_string(&p.content),
        SessionEventBody::ToolCall(p) => p.arguments = truncate_string(&p.arguments),
        SessionEventBody::ToolResult(p) => p.output = truncate_string(&p.output),
        SessionEventBody::Error(p) => p.message = truncate_string(&p.message),
        // request/header must stay complete; remaining kinds carry no
        // free-form model/tool text worth guarding.
        _ => {}
    }
    body
}

// ============================================================================
// Request-header assembly (EpochHeader)
// ============================================================================

/// Sampling/transport fields the adapters may place at the top level of the
/// wire body (or, for Ollama, under `options`).
const ADAPTER_DEFAULT_KEYS: &[&str] = &[
    "max_tokens",
    "temperature",
    "top_p",
    "top_k",
    "stream",
    "stop_sequences",
    "budget_tokens",
    "thinking",
    "reasoning_effort",
];

/// Extract the adapter-level sampling defaults actually sent.
fn adapter_defaults_from_wire(wire: &Value) -> Value {
    let mut out = serde_json::Map::new();
    let mut take_from = |source: &Value| {
        for key in ADAPTER_DEFAULT_KEYS {
            if let Some(v) = source.get(*key) {
                if !v.is_null() {
                    out.insert((*key).to_string(), v.clone());
                }
            }
        }
    };
    take_from(wire);
    if let Some(options) = wire.get("options") {
        take_from(options);
    }
    Value::Object(out)
}

/// Extract the rendered system prompt from the wire body. Handles the plain
/// string form and the Anthropic block form; OpenAI-style system *messages*
/// stay in `wire_body` only.
fn system_from_wire(wire: &Value) -> Option<String> {
    match wire.get("system")? {
        Value::String(s) => Some(s.clone()),
        Value::Array(blocks) => {
            let texts: Vec<String> = blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .map(String::from)
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n\n"))
            }
        }
        _ => None,
    }
}

/// sha256 of a tool's JSON schema, prefixed for readability.
fn schema_hash(schema: &Value) -> String {
    let encoded = serde_json::to_string(schema).unwrap_or_else(|_| schema.to_string());
    format!("sha256:{}", sha256_hex(&encoded))
}

/// Build the tool manifest (name + schema hash) from the wire body's tool
/// list. Covers the Anthropic (`{name, input_schema}`) and OpenAI
/// (`{function: {name, parameters}}`) shapes; anything else hashes the whole
/// entry minus `cache_control`.
fn tools_from_wire(wire: &Value) -> Vec<ToolManifestEntry> {
    let Some(Value::Array(entries)) = wire.get("tools") else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let name = entry
                .get("name")
                .or_else(|| entry.get("function").and_then(|f| f.get("name")))
                .and_then(Value::as_str)?;
            let schema = entry
                .get("input_schema")
                .or_else(|| entry.get("parameters"))
                .or_else(|| entry.get("function").and_then(|f| f.get("parameters")))
                .cloned()
                .unwrap_or_else(|| {
                    let mut without_cache = entry.clone();
                    if let Some(map) = without_cache.as_object_mut() {
                        map.remove("cache_control");
                    }
                    without_cache
                });
            Some(ToolManifestEntry {
                name: name.to_string(),
                schema_hash: schema_hash(&schema),
            })
        })
        .collect()
}

// ============================================================================
// SessionTee
// ============================================================================

/// The per-query L0 recorder. Owns the single writer for the duration of one
/// `process_query` call; all methods are infallible.
pub struct SessionTee {
    writer: Option<SessionLogWriter>,
    /// Immutable masking rules captured at open time (`redaction.toml` +
    /// env snapshot). Write-time redaction sees one consistent policy even
    /// if the file changes mid-turn.
    policy: Arc<RedactionPolicy>,
    /// Usage triple accumulated from `Usage` events this turn, folded into
    /// `turn/end` (plan §4.2: tokens / cost / cache triple).
    turn_usage: Option<TokenUsage>,
    /// Bare token count from the last `TurnCompleted` — the usage fallback
    /// when no `Usage` event was seen (text-only turns emit none).
    bare_tokens: Option<u64>,
    /// True between `turn/start` and the closing `turn/end`. Guarantees the
    /// start/end pairing: every opened turn is closed exactly once — on
    /// `Completed`, on `Failed`, or (cancellation) on drop as interrupted.
    turn_open: bool,
    /// Whether this tee opened a fresh log (first header says "initial").
    fresh_log: bool,
    headers_written: u32,
}

impl SessionTee {
    /// Open the tee for `session_id` under the default Shannon home
    /// (`SHANNON_HOME` honored). Honors the [`SWITCH_ENV`] off switch.
    pub fn open(session_id: &str, model: &str, provider: Option<&str>) -> Self {
        Self::open_opt(session_id, model, provider, None)
    }

    /// Open the tee rooted at an explicit base directory (tests, benches).
    pub fn open_in_dir(
        base_dir: &Path,
        session_id: &str,
        model: &str,
        provider: Option<&str>,
    ) -> Self {
        Self::open_opt(session_id, model, provider, Some(base_dir))
    }

    /// Open under an explicit base directory and policy — the deterministic
    /// seam for hosts/tests that must not depend on `~/.shannon`.
    pub fn open_in_dir_with_policy(
        base_dir: &Path,
        session_id: &str,
        model: &str,
        provider: Option<&str>,
        policy: Arc<RedactionPolicy>,
    ) -> Self {
        Self::open_with_writer(session_id, model, provider, Some(base_dir), false, policy)
    }

    /// Open the tee writing to `<container>/<session_id>/events.jsonl` where
    /// `container` is the sessions directory itself (§4.6 L0 layout). The
    /// engine uses this so log location always matches the `StateManager`
    /// that reads sessions back.
    pub fn open_in_container(
        container: &Path,
        session_id: &str,
        model: &str,
        provider: Option<&str>,
    ) -> Self {
        Self::open_in_container_with_policy(container, session_id, model, provider, None)
    }

    /// Open under an explicit container and policy. `None` resolves the
    /// default [`RedactionPolicy`] (`redaction.toml` + env snapshot).
    pub fn open_in_container_with_policy(
        container: &Path,
        session_id: &str,
        model: &str,
        provider: Option<&str>,
        policy: Option<Arc<RedactionPolicy>>,
    ) -> Self {
        Self::open_with_writer(
            session_id,
            model,
            provider,
            Some(container),
            true,
            policy.unwrap_or_else(RedactionPolicy::resolve),
        )
    }

    fn open_opt(
        session_id: &str,
        model: &str,
        provider: Option<&str>,
        base_dir: Option<&Path>,
    ) -> Self {
        Self::open_with_writer(
            session_id,
            model,
            provider,
            base_dir,
            false,
            RedactionPolicy::resolve(),
        )
    }

    fn open_with_writer(
        session_id: &str,
        model: &str,
        provider: Option<&str>,
        base_dir: Option<&Path>,
        container_layout: bool,
        policy: Arc<RedactionPolicy>,
    ) -> Self {
        if disabled_by_env() {
            return Self::disabled();
        }
        let opened = match base_dir {
            Some(base) if container_layout => SessionLogWriter::open_layout(base, session_id),
            Some(base) => SessionLogWriter::open_in_dir(base, session_id),
            None => SessionLogWriter::open(session_id),
        };
        match opened {
            Ok(mut writer) => {
                let fresh = writer.next_seq() == 0;
                if fresh {
                    // First event of the log: model / provider / cwd / version.
                    writer.record(SessionEventBody::SessionStart(SessionStartPayload {
                        model: model.to_string(),
                        provider: provider.map(String::from),
                        cwd: std::env::current_dir()
                            .ok()
                            .map(|p| p.display().to_string()),
                        app_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                    }));
                }
                Self {
                    writer: Some(writer),
                    policy,
                    turn_usage: None,
                    bare_tokens: None,
                    turn_open: false,
                    fresh_log: fresh,
                    headers_written: 0,
                }
            }
            // Lock conflict or I/O failure: degrade, never fail the session.
            Err(e) => {
                warn!(session_id, error = %e, "session log tee disabled for this query");
                Self::disabled()
            }
        }
    }

    fn disabled() -> Self {
        Self {
            writer: None,
            policy: Arc::new(RedactionPolicy::default()),
            turn_usage: None,
            bare_tokens: None,
            turn_open: false,
            fresh_log: false,
            headers_written: 0,
        }
    }

    /// True when events are actually being recorded.
    pub fn is_enabled(&self) -> bool {
        self.writer.is_some()
    }

    /// Record the user message that started the turn.
    pub fn record_user_message(&mut self, content: &str) {
        self.record_body(SessionEventBody::UserMessage(UserMessagePayload {
            source: UserMessagePayload::SOURCE_USER.into(),
            content: self.policy.redact_str(content),
        }));
    }

    /// Record `turn/start` for a query. The engine never broadcasts a
    /// `Started` QueryEvent, so the tee owns this boundary.
    pub fn record_turn_start(&mut self, query_id: Option<String>) {
        self.turn_open = true;
        // §4.15 online signals: re-arm the per-turn takeover latch.
        crate::signals::observe_turn_start();
        self.record_body(SessionEventBody::TurnStart(TurnStartPayload { query_id }));
    }

    /// Record one engine event: fold usage, map, redact, truncate, append.
    ///
    /// Turn boundaries are coalesced: the engine emits `TurnCompleted` /
    /// `Usage` only in the tool-call path (one pair per LLM step), while a
    /// vocabulary turn is the whole user-visible round. So per-step data is
    /// accumulated (`Usage` summed, `TurnCompleted` kept as the bare-token
    /// fallback) and exactly one `turn/end` closes the turn — on
    /// `Completed`, on `Failed`, or on drop as interrupted.
    pub fn record_query_event(&mut self, event: &QueryEvent) {
        match event {
            QueryEvent::Usage { .. } => {
                let usage = token_usage_from_event(event).expect("Usage maps to usage");
                self.add_turn_usage(usage);
                return;
            }
            QueryEvent::TurnCompleted { tokens_used, .. } => {
                self.bare_tokens = Some(*tokens_used);
                return;
            }
            QueryEvent::Completed { .. } => {
                self.close_turn(TurnEndPayload::REASON_COMPLETED, None);
                return;
            }
            QueryEvent::Failed { error, .. } => {
                // The failure itself is an error event (per the §4.1
                // mapping), then it closes the turn with the reason set.
                if let Some(body) = query_event_to_session_body(event) {
                    self.record_body(body);
                }
                self.close_turn(TurnEndPayload::REASON_FAILED, Some(error.clone()));
                return;
            }
            _ => {}
        }
        if let Some(body) = query_event_to_session_body(event) {
            self.record_body(body);
        }
    }

    /// Record one bus-delivered input (§4.8): either a durable vocabulary
    /// body or a fold directive — byte-identical to what
    /// [`Self::record_query_event`] writes for the equivalent engine event.
    ///
    /// Routing-only events (hook triggers in reserved namespaces) are skipped:
    /// they are distribution topics, not durable rows; their audit trail lands
    /// as explicit `hook/fired` bodies instead.
    pub fn record_bus_input(&mut self, input: &crate::bus::BusInput) {
        match input {
            crate::bus::BusInput::Event(event) => {
                if event.is_routing_only() {
                    return;
                }
                self.record_body(event.body.clone());
            }
            crate::bus::BusInput::Coalesce(coalesce) => match coalesce {
                crate::bus::CoalesceInput::StepUsage(usage) => self.add_turn_usage(usage.clone()),
                crate::bus::CoalesceInput::BareTokens(tokens) => self.bare_tokens = Some(*tokens),
                crate::bus::CoalesceInput::TurnBoundary { reason, error } => {
                    self.close_turn(reason, error.clone());
                }
            },
        }
    }

    /// Sum one step's usage into the turn accumulator.
    fn add_turn_usage(&mut self, usage: TokenUsage) {
        self.turn_usage = Some(match self.turn_usage.take() {
            Some(existing) => TokenUsage {
                input_tokens: existing.input_tokens + usage.input_tokens,
                output_tokens: existing.output_tokens + usage.output_tokens,
                cache_creation_tokens: existing.cache_creation_tokens + usage.cache_creation_tokens,
                cache_read_tokens: existing.cache_read_tokens + usage.cache_read_tokens,
                cost_usd: match (existing.cost_usd, usage.cost_usd) {
                    (Some(a), Some(b)) => Some(a + b),
                    (Some(a), None) | (None, Some(a)) => Some(a),
                    (None, None) => None,
                },
            },
            None => usage,
        });
    }

    /// Close the open turn with `reason`, emitting exactly one `turn/end`
    /// carrying the accumulated usage (or the bare-token fallback).
    fn close_turn(&mut self, reason: &str, error: Option<String>) {
        if !self.turn_open {
            return;
        }
        self.turn_open = false;
        // §4.15 online signals: every terminal close (completed/failed/
        // interrupted/…) funnels through here, so this is the single
        // counting point for turn denominators and interruption numerators.
        crate::signals::observe_turn_end(reason);
        let usage = self.turn_usage.take().or_else(|| {
            self.bare_tokens.map(|tokens| TokenUsage {
                input_tokens: 0,
                output_tokens: tokens,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
                cost_usd: None,
            })
        });
        self.record_body(SessionEventBody::TurnEnd(TurnEndPayload {
            reason: reason.into(),
            usage,
            error,
        }));
    }

    /// Record a `request/header` built from the adapter's own serialized
    /// request body. Headers bypass the size limit; only redaction applies.
    pub fn record_request_header(
        &mut self,
        wire_body: &Value,
        model: &str,
        provider: Option<&str>,
        config_snapshot: Value,
    ) {
        let reason = if self.fresh_log && self.headers_written == 0 {
            "initial"
        } else {
            "turn"
        };
        self.headers_written += 1;
        let payload = RequestHeaderPayload {
            model: model.to_string(),
            provider: provider.map(String::from),
            adapter_defaults: adapter_defaults_from_wire(wire_body),
            system: system_from_wire(wire_body).map(|s| self.policy.redact_str(&s)),
            tools: tools_from_wire(wire_body),
            config_snapshot: self.policy.redact_value(&config_snapshot),
            reason: Some(reason.into()),
            // The exact wire product — never truncated, so the log alone can
            // byte-reconstruct this request.
            wire_body: Some(self.policy.redact_value(wire_body)),
        };
        self.record_body(SessionEventBody::RequestHeader(payload));
    }

    /// Redact, enforce the size limit, and append. Infallible by design.
    fn record_body(&mut self, body: SessionEventBody) {
        // Redact first (immutable borrow), then hand off to the writer.
        let body = enforce_size_limit(self.redact_body(body));
        let Some(writer) = self.writer.as_mut() else {
            return;
        };
        writer.record(body);
    }

    /// Flush and close the log, counting (not propagating) errors. An open
    /// turn closes as interrupted (the caller did not observe a terminal
    /// event).
    pub fn close(mut self) {
        self.close_turn(TurnEndPayload::REASON_INTERRUPTED, None);
        if let Some(writer) = self.writer.take() {
            if let Err(e) = writer.close() {
                warn!(error = %e, "session log close failed");
            }
        }
    }
}

impl Drop for SessionTee {
    fn drop(&mut self) {
        // Cancellation path: the producer was aborted without a terminal
        // event, so the open turn closes as interrupted before the final
        // flush (which also covers tail events mapped to None after the
        // last flush-forcing kind).
        self.close_turn(TurnEndPayload::REASON_INTERRUPTED, None);
        if let Some(writer) = self.writer.take() {
            if let Err(e) = writer.close() {
                warn!(error = %e, "session log close on drop failed");
            }
        }
    }
}

/// Apply the tee's policy to every string field of a body.
impl SessionTee {
    fn redact_body(&self, body: SessionEventBody) -> SessionEventBody {
        match body {
            SessionEventBody::UserMessage(mut p) => {
                p.content = self.policy.redact_str(&p.content);
                SessionEventBody::UserMessage(p)
            }
            SessionEventBody::AssistantChunk(mut p) => {
                p.delta = self.policy.redact_str(&p.delta);
                SessionEventBody::AssistantChunk(p)
            }
            SessionEventBody::AssistantMessage(mut p) => {
                p.content = self.policy.redact_str(&p.content);
                SessionEventBody::AssistantMessage(p)
            }
            SessionEventBody::ToolCall(mut p) => {
                p.arguments = self.policy.redact_str(&p.arguments);
                SessionEventBody::ToolCall(p)
            }
            SessionEventBody::ToolResult(mut p) => {
                p.output = self.policy.redact_str(&p.output);
                p.meta = self.policy.redact_value(&p.meta);
                SessionEventBody::ToolResult(p)
            }
            SessionEventBody::TurnEnd(mut p) => {
                p.error = p.error.as_ref().map(|e| self.policy.redact_str(e));
                SessionEventBody::TurnEnd(p)
            }
            SessionEventBody::RequestHeader(mut p) => {
                p.system = p.system.as_ref().map(|s| self.policy.redact_str(s));
                p.config_snapshot = self.policy.redact_value(&p.config_snapshot);
                if let Some(wire) = p.wire_body.as_ref() {
                    p.wire_body = Some(self.policy.redact_value(wire));
                }
                SessionEventBody::RequestHeader(p)
            }
            SessionEventBody::Error(mut p) => {
                p.message = self.policy.redact_str(&p.message);
                p.detail = p.detail.as_ref().map(|d| self.policy.redact_value(d));
                SessionEventBody::Error(p)
            }
            other => other,
        }
    }
}

/// True when [`SWITCH_ENV`] disables logging (`off` / `0` / `false`).
fn disabled_by_env() -> bool {
    switch_value_disables(std::env::var(SWITCH_ENV).ok().as_deref())
}

/// Pure switch parsing (unit-testable without env mutation).
fn switch_value_disables(value: Option<&str>) -> bool {
    match value {
        Some(v) => {
            let v = v.trim().to_ascii_lowercase();
            v == "off" || v == "0" || v == "false"
        }
        None => false,
    }
}

// ============================================================================
// TeeHandle — shared handle for the engine's single injection point
// ============================================================================

/// Cloneable handle to a [`SessionTee`]. The engine keeps one per query and
/// clones it into the event sender and the request-capture callback. A
/// poisoned lock (a panicking writer) permanently disables this handle
/// rather than failing the session.
#[derive(Clone)]
pub struct TeeHandle {
    tee: Arc<Mutex<SessionTee>>,
}

impl TeeHandle {
    /// Wrap a tee.
    pub fn new(tee: SessionTee) -> Self {
        Self {
            tee: Arc::new(Mutex::new(tee)),
        }
    }

    /// Open a tee under the default Shannon home (see [`SessionTee::open`]).
    pub fn open(session_id: &str, model: &str, provider: Option<&str>) -> Self {
        Self::new(SessionTee::open(session_id, model, provider))
    }

    /// Open a tee rooted at an explicit base directory.
    pub fn open_in_dir(
        base_dir: &Path,
        session_id: &str,
        model: &str,
        provider: Option<&str>,
    ) -> Self {
        Self::new(SessionTee::open_in_dir(
            base_dir, session_id, model, provider,
        ))
    }

    /// Wrap a tee writing to `<container>/<session_id>/events.jsonl` (§4.6
    /// sessions-container layout; see [`SessionTee::open_in_container`]).
    pub fn open_in_container(
        container: &Path,
        session_id: &str,
        model: &str,
        provider: Option<&str>,
    ) -> Self {
        Self::new(SessionTee::open_in_container(
            container, session_id, model, provider,
        ))
    }

    /// A disabled handle (records nothing) — used by non-session callers.
    pub fn disabled() -> Self {
        Self::new(SessionTee::disabled())
    }

    /// True when events are being recorded.
    pub fn is_enabled(&self) -> bool {
        self.tee.lock().map(|t| t.is_enabled()).unwrap_or(false)
    }

    /// Record the user message that started the turn.
    pub fn record_user_message(&self, content: &str) {
        if let Ok(mut tee) = self.tee.lock() {
            tee.record_user_message(content);
        }
    }

    /// Record `turn/start`.
    pub fn record_turn_start(&self, query_id: Option<String>) {
        if let Ok(mut tee) = self.tee.lock() {
            tee.record_turn_start(query_id);
        }
    }

    /// Record one engine event (single injection point for QueryEvents).
    pub fn record_query_event(&self, event: &QueryEvent) {
        if let Ok(mut tee) = self.tee.lock() {
            tee.record_query_event(event);
        }
    }

    /// Record one bus-delivered input (§4.8 L0-subscriber path).
    pub fn record_bus_input(&self, input: &crate::bus::BusInput) {
        if let Ok(mut tee) = self.tee.lock() {
            tee.record_bus_input(input);
        }
    }

    /// Record a request header from the adapter's serialized body.
    pub fn record_request_header(
        &self,
        wire_body: &Value,
        model: &str,
        provider: Option<&str>,
        config_snapshot: Value,
    ) {
        if let Ok(mut tee) = self.tee.lock() {
            tee.record_request_header(wire_body, model, provider, config_snapshot);
        }
    }

    /// Flush and close the underlying tee (idempotent for remaining clones:
    /// the writer only closes once the last reference drops).
    pub fn close(&self) {
        if let Ok(mut tee) = self.tee.lock() {
            if let Some(writer) = tee.writer.as_mut() {
                if let Err(e) = writer.flush() {
                    warn!(error = %e, "session log flush failed on close");
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;
    use shannon_types::session_event::{SessionEventKind, TurnEndPayload};
    use tempfile::TempDir;
    use uuid::Uuid;

    fn open_tee(dir: &TempDir) -> SessionTee {
        SessionTee::open_in_dir(dir.path(), "sess-tee", "claude-sonnet-4", Some("anthropic"))
    }

    fn read_bodies(dir: &TempDir) -> Vec<SessionEventBody> {
        let path = super::super::session_events_path(dir.path(), "sess-tee");
        let reader = super::super::SessionLogReader::open(&path).expect("open reader");
        reader
            .read_events(false)
            .expect("read events")
            .into_iter()
            .map(|e| e.body)
            .collect()
    }

    fn query_id() -> Uuid {
        Uuid::new_v4()
    }

    #[test]
    fn test_open_writes_session_start_on_fresh_log() {
        let dir = TempDir::new().expect("tempdir");
        {
            let tee = open_tee(&dir);
            assert!(tee.is_enabled());
            tee.close();
        }
        let bodies = read_bodies(&dir);
        assert_eq!(bodies.len(), 1);
        match &bodies[0] {
            SessionEventBody::SessionStart(p) => {
                assert_eq!(p.model, "claude-sonnet-4");
                assert_eq!(p.provider.as_deref(), Some("anthropic"));
                assert_eq!(p.app_version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
            }
            other => panic!("wrong body: {other:?}"),
        }
        // Reopening must not duplicate session/start.
        {
            let tee = open_tee(&dir);
            tee.close();
        }
        assert_eq!(read_bodies(&dir).len(), 1, "reopen writes no session/start");
    }

    #[test]
    fn test_query_event_pipeline_maps_and_folds_usage() {
        let dir = TempDir::new().expect("tempdir");
        {
            let mut tee = open_tee(&dir);
            tee.record_user_message("hello");
            tee.record_turn_start(Some("q-1".into()));
            tee.record_query_event(&QueryEvent::Text {
                query_id: query_id(),
                content: "hi there".into(),
            });
            // Two LLM steps, each with a rich usage triple: the triples are
            // summed and folded into the single turn/end — never logged 1:1.
            tee.record_query_event(&QueryEvent::Usage {
                query_id: query_id(),
                input_tokens: 11,
                output_tokens: 22,
                cost_usd: 0.5,
                cache_creation_tokens: 3,
                cache_read_tokens: 4,
            });
            tee.record_query_event(&QueryEvent::TurnCompleted {
                query_id: query_id(),
                turn_number: 1,
                tokens_used: 22,
            });
            tee.record_query_event(&QueryEvent::Usage {
                query_id: query_id(),
                input_tokens: 100,
                output_tokens: 200,
                cost_usd: 1.5,
                cache_creation_tokens: 30,
                cache_read_tokens: 40,
            });
            tee.record_query_event(&QueryEvent::Completed {
                query_id: query_id(),
            });
            tee.close();
        }
        let bodies = read_bodies(&dir);
        let kinds: Vec<SessionEventKind> = bodies.iter().map(|b| b.kind()).collect();
        assert_eq!(
            kinds,
            vec![
                SessionEventKind::SessionStart,
                SessionEventKind::UserMessage,
                SessionEventKind::TurnStart,
                SessionEventKind::AssistantChunk,
                SessionEventKind::TurnEnd,
            ]
        );
        match &bodies[4] {
            SessionEventBody::TurnEnd(TurnEndPayload { reason, usage, .. }) => {
                assert_eq!(reason, TurnEndPayload::REASON_COMPLETED);
                let usage = usage.as_ref().expect("folded usage");
                assert_eq!(usage.input_tokens, 111);
                assert_eq!(usage.output_tokens, 222);
                assert_eq!(usage.cache_creation_tokens, 33);
                assert_eq!(usage.cache_read_tokens, 44);
                assert_eq!(usage.cost_usd, Some(2.0));
            }
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_turn_end_without_usage_keeps_bare_tokens() {
        let dir = TempDir::new().expect("tempdir");
        {
            let mut tee = open_tee(&dir);
            tee.record_turn_start(None);
            // Text-only turns emit no Usage; the bare TurnCompleted token
            // count is the fallback.
            tee.record_query_event(&QueryEvent::TurnCompleted {
                query_id: query_id(),
                turn_number: 1,
                tokens_used: 99,
            });
            tee.record_query_event(&QueryEvent::Completed {
                query_id: query_id(),
            });
            tee.close();
        }
        let bodies = read_bodies(&dir);
        match &bodies[2] {
            SessionEventBody::TurnEnd(p) => {
                assert_eq!(p.reason, TurnEndPayload::REASON_COMPLETED);
                assert_eq!(p.usage.as_ref().expect("usage").output_tokens, 99);
            }
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_failed_turn_closes_with_error() {
        let dir = TempDir::new().expect("tempdir");
        {
            let mut tee = open_tee(&dir);
            tee.record_turn_start(None);
            tee.record_query_event(&QueryEvent::Failed {
                query_id: query_id(),
                error: "boom".into(),
            });
            tee.close();
        }
        let bodies = read_bodies(&dir);
        match &bodies[2] {
            SessionEventBody::Error(p) => assert_eq!(p.category, "query-failed"),
            other => panic!("wrong body: {other:?}"),
        }
        match &bodies[3] {
            SessionEventBody::TurnEnd(p) => {
                assert_eq!(p.reason, TurnEndPayload::REASON_FAILED);
                assert_eq!(p.error.as_deref(), Some("boom"));
            }
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_open_turn_closes_as_interrupted_on_drop() {
        let dir = TempDir::new().expect("tempdir");
        {
            let mut tee = open_tee(&dir);
            tee.record_turn_start(None);
            // No terminal event — drop must close the turn as interrupted.
            drop(tee);
        }
        let bodies = read_bodies(&dir);
        match &bodies[2] {
            SessionEventBody::TurnEnd(p) => {
                assert_eq!(p.reason, TurnEndPayload::REASON_INTERRUPTED)
            }
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_rate_limit_maps_to_error_category() {
        let dir = TempDir::new().expect("tempdir");
        {
            let mut tee = open_tee(&dir);
            tee.record_query_event(&QueryEvent::RateLimit {
                query_id: query_id(),
                requests_used: 40,
                requests_limit: 50,
            });
            tee.close();
        }
        match &read_bodies(&dir)[1] {
            SessionEventBody::Error(p) => assert_eq!(p.category, "rate_limit"),
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_request_header_carries_wire_body_verbatim() {
        let dir = TempDir::new().expect("tempdir");
        let wire = json!({
            "model": "claude-sonnet-4",
            "max_tokens": 128,
            "stream": true,
            "system": [{"type": "text", "text": "You are Shannon."}],
            "tools": [
                {"name": "Bash", "description": "run", "input_schema": {"type": "object"}},
                {"type": "function", "function": {"name": "Read", "parameters": {"type": "object"}}}
            ],
            "messages": [{"role": "user", "content": "hello"}]
        });
        {
            let mut tee = open_tee(&dir);
            tee.record_request_header(
                &wire,
                "claude-sonnet-4",
                Some("anthropic"),
                json!({"max_turns": 20}),
            );
            tee.close();
        }
        match &read_bodies(&dir)[1] {
            SessionEventBody::RequestHeader(p) => {
                assert_eq!(p.reason.as_deref(), Some("initial"));
                assert_eq!(p.system.as_deref(), Some("You are Shannon."));
                assert_eq!(p.adapter_defaults["max_tokens"], json!(128));
                assert_eq!(p.adapter_defaults["stream"], json!(true));
                assert_eq!(p.tools.len(), 2);
                assert_eq!(p.tools[0].name, "Bash");
                assert!(p.tools[0].schema_hash.starts_with("sha256:"));
                assert_eq!(p.tools[1].name, "Read");
                // Byte-faithful wire product, exempt from truncation.
                assert_eq!(p.wire_body.as_ref().expect("wire_body"), &wire);
            }
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_oversized_tool_result_truncated_with_hash_marker() {
        let dir = TempDir::new().expect("tempdir");
        let big = "x".repeat(MAX_EVENT_BYTES + 100);
        {
            let mut tee = open_tee(&dir);
            tee.record_query_event(&QueryEvent::ToolUseResult {
                query_id: query_id(),
                tool_use_id: "t1".into(),
                tool_name: "Bash".into(),
                result: big.clone(),
                is_error: false,
                meta: Box::new(serde_json::Value::Null),
            });
            tee.close();
        }
        match &read_bodies(&dir)[1] {
            SessionEventBody::ToolResult(p) => {
                assert!(
                    p.output.len() < MAX_EVENT_BYTES,
                    "payload must be under the limit"
                );
                assert!(p.output.contains(&format!("original_bytes={}", big.len())));
                let hash_start = p.output.rfind("sha256=").expect("hash marker");
                let hash = &p.output[hash_start + 7..p.output.len() - 1];
                assert_eq!(hash, &sha256_hex(&big));
                // The head is preserved.
                assert!(p.output.starts_with(&"x".repeat(TRUNCATION_HEAD_BYTES)));
            }
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_oversized_request_header_not_truncated() {
        let dir = TempDir::new().expect("tempdir");
        // A header bigger than the payload limit must stay complete.
        let big_system = "y".repeat(MAX_EVENT_BYTES);
        let wire = json!({ "model": "m", "system": big_system.clone() });
        {
            let mut tee = open_tee(&dir);
            tee.record_request_header(&wire, "m", None, json!({}));
            tee.close();
        }
        match &read_bodies(&dir)[1] {
            SessionEventBody::RequestHeader(p) => {
                assert_eq!(p.system.as_deref(), Some(big_system.as_str()));
            }
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_redaction_masks_token_prefixes_and_env_values() {
        // Explicit env-only policy (not the process-wide snapshot): keeps
        // the assertion deterministic regardless of any local
        // `~/.shannon/redaction.toml`.
        let policy = RedactionPolicy::capture_env_only();
        let text = "key sk-ant-abc123456789 and pat ghp_abcdefgh12345678 slack xoxb-12345678-abcdefg plain text";
        let redacted = policy.redact_str(text);
        assert!(!redacted.contains("sk-ant-abc123456789"));
        assert!(!redacted.contains("ghp_abcdefgh12345678"));
        assert!(!redacted.contains("xoxb-12345678-abcdefg"));
        assert!(redacted.contains("plain text"));
        assert_eq!(redacted.matches("[REDACTED]").count(), 3);

        // Exact env-secret values are masked wherever they appear.
        let secret = "super-secret-value-42";
        let out =
            super::redact_with_secrets("curl -H auth:super-secret-value-42", &[secret.to_string()]);
        assert_eq!(out, "curl -H auth:[REDACTED]");

        // Recursive value redaction through the same policy.
        let value = json!({"a": ["sk-ant-abcdefghijk"], "b": 3});
        assert_eq!(
            policy.redact_value(&value),
            json!({"a": ["[REDACTED]"], "b": 3})
        );
    }

    #[test]
    fn test_policy_file_changes_what_a_tee_writes() {
        let dir = TempDir::new().expect("tempdir");
        let config = dir.path().join("redaction.toml");
        std::fs::write(&config, "[values]\nsecrets = [\"plain-internal-marker\"]\n")
            .expect("write toml");
        let policy = Arc::new(RedactionPolicy::load(&config));
        {
            let mut tee =
                SessionTee::open_in_dir_with_policy(dir.path(), "sess-pol", "m", None, policy);
            tee.record_user_message("leak plain-internal-marker here");
            tee.close();
        }
        let path = super::super::session_events_path(dir.path(), "sess-pol");
        let raw = std::fs::read_to_string(path).expect("read events");
        assert!(
            !raw.contains("plain-internal-marker"),
            "value masked on disk"
        );
        assert!(raw.contains("[REDACTED]"));
    }

    #[test]
    fn test_switch_parsing() {
        assert!(!switch_value_disables(None), "default is on");
        assert!(!switch_value_disables(Some("on")));
        assert!(!switch_value_disables(Some("")));
        assert!(switch_value_disables(Some("off")));
        assert!(switch_value_disables(Some("OFF")));
        assert!(switch_value_disables(Some("0")));
        assert!(switch_value_disables(Some(" false ")));
    }

    #[test]
    fn test_tee_handle_disabled_and_poison_safe() {
        let handle = TeeHandle::disabled();
        assert!(!handle.is_enabled());
        // Recording into a disabled handle is a silent no-op.
        handle.record_query_event(&QueryEvent::Text {
            query_id: query_id(),
            content: "x".into(),
        });
        handle.record_user_message("x");
        handle.close();
    }

    #[test]
    fn test_open_failure_degrades_to_disabled() {
        // A base directory that cannot host a file (a file where the
        // sessions dir would be) forces the degraded path, not a panic.
        let dir = TempDir::new().expect("tempdir");
        let block = dir.path().join("sessions");
        std::fs::write(&block, b"not a dir").expect("write blocker");
        let mut tee = SessionTee::open_in_dir(dir.path(), "s", "m", None);
        assert!(!tee.is_enabled());
        tee.record_query_event(&QueryEvent::Text {
            query_id: query_id(),
            content: "x".into(),
        });
        tee.close();
    }
}
