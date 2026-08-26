//! # Session Event Vocabulary (L0 unified event log)
//!
//! The typed vocabulary for Shannon's append-only session event log — the
//! single authoritative record ("L0") from which every other view (LLM message
//! history, projections, telemetry, replay) is derived. Modeled on the
//! DeepSeek-harness `SessionEventMap` design:
//!
//! - A session is an append-only log of typed [`SessionEvent`]s. Message
//!   history is *derived* from the log, never stored separately.
//! - `seq` is assigned by the single writer and equals the number of events
//!   already written, so seqs are strictly continuous from `0`.
//! - Every event carries a `kind` string. Unknown kinds are **required** by
//!   default: a reader must reject them ([`UnknownEventKindError`]) rather
//!   than silently dropping data. Only readers that explicitly opt in may
//!   skip unknown kinds (see `shannon_core::session_log::SessionLogReader`).
//!
//! This module is pure types: zero engine dependencies, no I/O, no clocks.
//! The reader/writer live in `shannon_core::session_log`.
//!
//! Wire format — one JSON object per JSONL line, payload flattened next to
//! the `kind` tag:
//!
//! ```json
//! {"seq":0,"ts_ns":1756200000000000000,"session_id":"s1","turn":1,
//!  "kind":"tool/call","tool_use_id":"toolu_1","tool_name":"Bash",
//!  "arguments":"{\"command\":\"ls\"}"}
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

// ============================================================================
// Kind vocabulary
// ============================================================================

/// Error returned when a `kind` string is not part of the vocabulary.
///
/// This is the "required by default" invariant: an unknown event kind must
/// fail the reader instead of being silently dropped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown session event kind: {0}")]
pub struct UnknownEventKindError(pub String);

/// The discriminator of a session event. 18 kinds in vocabulary v1.
///
/// The serde representation is the wire string (e.g. `"tool/call"`), matching
/// the `kind` tag of [`SessionEventBody`] exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionEventKind {
    /// Session opened: model / provider / cwd / version snapshot.
    #[serde(rename = "session/start")]
    SessionStart,
    /// Seed/resume boundary: this session continues a parent session.
    #[serde(rename = "session/end-seed")]
    SessionEndSeed,
    /// A user message entered the session.
    #[serde(rename = "user/message")]
    UserMessage,
    /// A streaming delta from the assistant (token-level replay fidelity).
    #[serde(rename = "assistant/chunk")]
    AssistantChunk,
    /// A finalized assistant message, carrying usage and `interrupted`.
    #[serde(rename = "assistant/message")]
    AssistantMessage,
    /// A tool invocation request; `arguments` stays the raw model-emitted
    /// JSON string (unparsed).
    #[serde(rename = "tool/call")]
    ToolCall,
    /// A tool execution result, with error identity, duration, and
    /// tool-private metadata.
    #[serde(rename = "tool/result")]
    ToolResult,
    /// Per-request snapshot (EpochHeader): config, adapter defaults, rendered
    /// system prompt, and tool manifest with schema hashes.
    #[serde(rename = "request/header")]
    RequestHeader,
    /// The assembled request context (input messages) for a request.
    #[serde(rename = "request/context")]
    RequestContext,
    /// A permission decision made for a tool action.
    #[serde(rename = "permission/decision")]
    PermissionDecision,
    /// A hook that fired.
    #[serde(rename = "hook/fired")]
    HookFired,
    /// A turn (user-visible round) started.
    #[serde(rename = "turn/start")]
    TurnStart,
    /// A turn ended, with reason and usage.
    #[serde(rename = "turn/end")]
    TurnEnd,
    /// Full todo-list snapshot (last-write-wins).
    #[serde(rename = "todo/write")]
    TodoWrite,
    /// Events appended to the human-visible surface.
    #[serde(rename = "surface/append")]
    SurfaceAppend,
    /// A surface range replaced (e.g. compaction masking with a summary node).
    #[serde(rename = "surface/replace")]
    SurfaceReplace,
    /// An error (query failure, rate limit, log corruption warning, …).
    #[serde(rename = "error")]
    Error,
    /// Namespaced extension payload for future/custom producers.
    #[serde(rename = "custom")]
    Custom,
}

impl SessionEventKind {
    /// Every kind in vocabulary v1. Drives exhaustive roundtrip tests.
    pub const ALL: [SessionEventKind; 18] = [
        SessionEventKind::SessionStart,
        SessionEventKind::SessionEndSeed,
        SessionEventKind::UserMessage,
        SessionEventKind::AssistantChunk,
        SessionEventKind::AssistantMessage,
        SessionEventKind::ToolCall,
        SessionEventKind::ToolResult,
        SessionEventKind::RequestHeader,
        SessionEventKind::RequestContext,
        SessionEventKind::PermissionDecision,
        SessionEventKind::HookFired,
        SessionEventKind::TurnStart,
        SessionEventKind::TurnEnd,
        SessionEventKind::TodoWrite,
        SessionEventKind::SurfaceAppend,
        SessionEventKind::SurfaceReplace,
        SessionEventKind::Error,
        SessionEventKind::Custom,
    ];

    /// The wire string for this kind.
    pub const fn as_str(self) -> &'static str {
        match self {
            SessionEventKind::SessionStart => "session/start",
            SessionEventKind::SessionEndSeed => "session/end-seed",
            SessionEventKind::UserMessage => "user/message",
            SessionEventKind::AssistantChunk => "assistant/chunk",
            SessionEventKind::AssistantMessage => "assistant/message",
            SessionEventKind::ToolCall => "tool/call",
            SessionEventKind::ToolResult => "tool/result",
            SessionEventKind::RequestHeader => "request/header",
            SessionEventKind::RequestContext => "request/context",
            SessionEventKind::PermissionDecision => "permission/decision",
            SessionEventKind::HookFired => "hook/fired",
            SessionEventKind::TurnStart => "turn/start",
            SessionEventKind::TurnEnd => "turn/end",
            SessionEventKind::TodoWrite => "todo/write",
            SessionEventKind::SurfaceAppend => "surface/append",
            SessionEventKind::SurfaceReplace => "surface/replace",
            SessionEventKind::Error => "error",
            SessionEventKind::Custom => "custom",
        }
    }
}

impl fmt::Display for SessionEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SessionEventKind {
    type Err = UnknownEventKindError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        for kind in Self::ALL {
            if kind.as_str() == s {
                return Ok(kind);
            }
        }
        Err(UnknownEventKindError(s.to_string()))
    }
}

// ============================================================================
// Shared payload types
// ============================================================================

/// Token usage for one assistant step / turn (cache triple included).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Prompt tokens consumed.
    pub input_tokens: u64,
    /// Completion tokens generated.
    pub output_tokens: u64,
    /// Tokens written to the provider prompt cache.
    #[serde(default)]
    pub cache_creation_tokens: u64,
    /// Tokens read from the provider prompt cache.
    #[serde(default)]
    pub cache_read_tokens: u64,
    /// Cost of the step in USD, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// One entry of the tool manifest embedded in a [`RequestHeaderPayload`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolManifestEntry {
    /// Tool name as presented to the model.
    pub name: String,
    /// Hash (e.g. sha256 hex) of the tool's JSON schema.
    pub schema_hash: String,
}

/// One entry of a todo-list snapshot (full snapshot, last-write-wins).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TodoSnapshotEntry {
    /// Todo content text.
    pub content: String,
    /// Status string (e.g. "pending", "in_progress", "completed").
    pub status: String,
}

// ============================================================================
// Payloads (one per kind)
// ============================================================================

/// Payload for [`SessionEventKind::SessionStart`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionStartPayload {
    /// The model id active at session start.
    pub model: String,
    /// The provider id, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Working directory of the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Shannon version that wrote the log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
}

/// Payload for [`SessionEventKind::SessionEndSeed`]: the seed/resume boundary
/// of a session forked or continued from a parent session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEndSeedPayload {
    /// Why the seed boundary exists (e.g. "seed", "fork", "resume").
    pub reason: String,
    /// The parent session this one derives from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
}

/// Payload for [`SessionEventKind::UserMessage`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserMessagePayload {
    /// Where the message came from; see the `SOURCE_*` constants.
    pub source: String,
    /// Message content.
    pub content: String,
}

impl UserMessagePayload {
    /// A human-typed prompt.
    pub const SOURCE_USER: &'static str = "user";
    /// Content injected by an agent on the user's behalf.
    pub const SOURCE_AGENT_INJECT: &'static str = "agent.inject";
    /// Content injected to resume a goal.
    pub const SOURCE_GOAL_RESUME: &'static str = "goal.resume";
}

/// Payload for [`SessionEventKind::AssistantChunk`]: one streaming delta,
/// preserved verbatim for token-level replay fidelity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantChunkPayload {
    /// The delta text.
    pub delta: String,
    /// True when the delta belongs to an extended-thinking stream.
    #[serde(default)]
    pub thinking: bool,
}

/// Payload for [`SessionEventKind::AssistantMessage`]: a finalized assistant
/// message. When a step is interrupted, the event finalizes the prefix with
/// `interrupted: true`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessagePayload {
    /// Full message content.
    pub content: String,
    /// Usage of the step that produced this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    /// True when the step was interrupted before completing.
    #[serde(default)]
    pub interrupted: bool,
}

/// Payload for [`SessionEventKind::ToolCall`]. `arguments` is kept as the
/// raw, unparsed JSON string exactly as the model emitted it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallPayload {
    /// Provider-issued tool-use id, paired with the eventual result.
    pub tool_use_id: String,
    /// Tool name.
    pub tool_name: String,
    /// Raw model-emitted arguments JSON (unparsed string).
    pub arguments: String,
}

/// Payload for [`SessionEventKind::ToolResult`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultPayload {
    /// The tool-use id this result answers.
    pub tool_use_id: String,
    /// Tool name.
    pub tool_name: String,
    /// Tool output (rendered string form).
    pub output: String,
    /// Whether the tool reported an error.
    #[serde(default)]
    pub is_error: bool,
    /// Wall-clock execution duration, when measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Tool-private metadata (arbitrary JSON).
    #[serde(default)]
    pub meta: serde_json::Value,
}

/// Payload for [`SessionEventKind::RequestHeader`] — the "EpochHeader": the
/// per-request envelope making every LLM request a pure function of the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestHeaderPayload {
    /// Model id for the request.
    pub model: String,
    /// Provider id, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Adapter-level sampling defaults actually sent (temperature, max_tokens,
    /// top_p, …) as applied by the provider adapter.
    #[serde(default)]
    pub adapter_defaults: serde_json::Value,
    /// The rendered system prompt for the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// The assembled tool manifest with per-tool schema hashes.
    #[serde(default)]
    pub tools: Vec<ToolManifestEntry>,
    /// Snapshot of the engine config that produced the request.
    #[serde(default)]
    pub config_snapshot: serde_json::Value,
    /// Why this header was written (e.g. "initial", "change") — a full
    /// snapshot is recorded on every request-shape change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Payload for [`SessionEventKind::RequestContext`]: the input message list
/// assembled for a request (as generic JSON — engine message types are not
/// part of the type layer).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestContextPayload {
    /// The messages sent with the request.
    #[serde(default)]
    pub input_messages: Vec<serde_json::Value>,
    /// Token estimate for the context, when computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_estimate: Option<u64>,
}

/// Payload for [`SessionEventKind::PermissionDecision`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionDecisionPayload {
    /// Tool involved in the decision, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Human-readable description of the guarded operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    /// Decision string (e.g. "allow", "deny", "ask").
    pub decision: String,
    /// Why the decision was made (rule id, user choice, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Permission mode active at decision time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// Payload for [`SessionEventKind::HookFired`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookFiredPayload {
    /// Hook event type that fired.
    pub event: String,
    /// Hook (name/label) that handled it.
    pub hook: String,
    /// Outcome (e.g. "ok", "error", "blocked").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// Hook execution duration, when measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Payload for [`SessionEventKind::TurnStart`]. The turn number itself lives
/// in the envelope (`turn`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnStartPayload {
    /// Engine query id, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_id: Option<String>,
}

/// Payload for [`SessionEventKind::TurnEnd`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnEndPayload {
    /// Why the turn ended; see the `REASON_*` constants.
    pub reason: String,
    /// Usage accumulated over the turn, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    /// Error that ended the turn, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TurnEndPayload {
    /// The turn ran to completion.
    pub const REASON_COMPLETED: &'static str = "completed";
    /// The turn failed.
    pub const REASON_FAILED: &'static str = "failed";
    /// The user interrupted the turn.
    pub const REASON_INTERRUPTED: &'static str = "interrupted";
    /// The turn cap (`max_turns`) was reached.
    pub const REASON_MAX_TURNS: &'static str = "max-turns";
    /// The budget limit stopped the turn.
    pub const REASON_BUDGET_EXCEEDED: &'static str = "budget-exceeded";
    /// A timeout stopped the turn.
    pub const REASON_TIMEOUT: &'static str = "timeout";
}

/// Payload for [`SessionEventKind::TodoWrite`]: a full todo-list snapshot;
/// projections apply last-write-wins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TodoWritePayload {
    /// The complete todo list at write time.
    pub todos: Vec<TodoSnapshotEntry>,
}

/// Payload for [`SessionEventKind::SurfaceAppend`]: a contiguous event range
/// became visible on the human transcript surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceAppendPayload {
    /// First seq of the appended range (inclusive).
    pub start_seq: u64,
    /// Last seq of the appended range (inclusive).
    pub end_seq: u64,
}

/// Payload for [`SessionEventKind::SurfaceReplace`]: a surface range is
/// masked and replaced in place (e.g. a compaction summary node), with the
/// masked source events still referenced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceReplacePayload {
    /// First seq of the replaced range (inclusive).
    pub start_seq: u64,
    /// Last seq of the replaced range (inclusive).
    pub end_seq: u64,
    /// Seqs of the events masked by this replacement.
    pub source_event_seqs: Vec<u64>,
    /// Why the replacement happened (e.g. "compaction").
    pub reason: String,
}

/// Payload for [`SessionEventKind::Error`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorPayload {
    /// Error category (e.g. "query-failed", "rate_limit", "log-corruption").
    pub category: String,
    /// Human-readable error message.
    pub message: String,
    /// Structured details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

impl ErrorPayload {
    /// Category used by the log itself when recovering a truncated tail.
    pub const CATEGORY_LOG_CORRUPTION: &'static str = "log-corruption";
}

/// Payload for [`SessionEventKind::Custom`]: a namespaced extension payload.
/// Unknown producers must use a reverse-DNS-style namespace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomPayload {
    /// Namespace of the producer (e.g. "com.example.plugin").
    pub namespace: String,
    /// Producer-defined payload.
    #[serde(default)]
    pub data: serde_json::Value,
}

// ============================================================================
// Event body (kind + payload) and envelope
// ============================================================================

/// The kind plus its typed payload. Serialized as the `kind` string tag with
/// the payload struct flattened alongside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SessionEventBody {
    /// See [`SessionStartPayload`].
    #[serde(rename = "session/start")]
    SessionStart(SessionStartPayload),
    /// See [`SessionEndSeedPayload`].
    #[serde(rename = "session/end-seed")]
    SessionEndSeed(SessionEndSeedPayload),
    /// See [`UserMessagePayload`].
    #[serde(rename = "user/message")]
    UserMessage(UserMessagePayload),
    /// See [`AssistantChunkPayload`].
    #[serde(rename = "assistant/chunk")]
    AssistantChunk(AssistantChunkPayload),
    /// See [`AssistantMessagePayload`].
    #[serde(rename = "assistant/message")]
    AssistantMessage(AssistantMessagePayload),
    /// See [`ToolCallPayload`].
    #[serde(rename = "tool/call")]
    ToolCall(ToolCallPayload),
    /// See [`ToolResultPayload`].
    #[serde(rename = "tool/result")]
    ToolResult(ToolResultPayload),
    /// See [`RequestHeaderPayload`].
    #[serde(rename = "request/header")]
    RequestHeader(RequestHeaderPayload),
    /// See [`RequestContextPayload`].
    #[serde(rename = "request/context")]
    RequestContext(RequestContextPayload),
    /// See [`PermissionDecisionPayload`].
    #[serde(rename = "permission/decision")]
    PermissionDecision(PermissionDecisionPayload),
    /// See [`HookFiredPayload`].
    #[serde(rename = "hook/fired")]
    HookFired(HookFiredPayload),
    /// See [`TurnStartPayload`].
    #[serde(rename = "turn/start")]
    TurnStart(TurnStartPayload),
    /// See [`TurnEndPayload`].
    #[serde(rename = "turn/end")]
    TurnEnd(TurnEndPayload),
    /// See [`TodoWritePayload`].
    #[serde(rename = "todo/write")]
    TodoWrite(TodoWritePayload),
    /// See [`SurfaceAppendPayload`].
    #[serde(rename = "surface/append")]
    SurfaceAppend(SurfaceAppendPayload),
    /// See [`SurfaceReplacePayload`].
    #[serde(rename = "surface/replace")]
    SurfaceReplace(SurfaceReplacePayload),
    /// See [`ErrorPayload`].
    #[serde(rename = "error")]
    Error(ErrorPayload),
    /// See [`CustomPayload`].
    #[serde(rename = "custom")]
    Custom(CustomPayload),
}

impl SessionEventBody {
    /// The kind of this body.
    pub fn kind(&self) -> SessionEventKind {
        match self {
            SessionEventBody::SessionStart(_) => SessionEventKind::SessionStart,
            SessionEventBody::SessionEndSeed(_) => SessionEventKind::SessionEndSeed,
            SessionEventBody::UserMessage(_) => SessionEventKind::UserMessage,
            SessionEventBody::AssistantChunk(_) => SessionEventKind::AssistantChunk,
            SessionEventBody::AssistantMessage(_) => SessionEventKind::AssistantMessage,
            SessionEventBody::ToolCall(_) => SessionEventKind::ToolCall,
            SessionEventBody::ToolResult(_) => SessionEventKind::ToolResult,
            SessionEventBody::RequestHeader(_) => SessionEventKind::RequestHeader,
            SessionEventBody::RequestContext(_) => SessionEventKind::RequestContext,
            SessionEventBody::PermissionDecision(_) => SessionEventKind::PermissionDecision,
            SessionEventBody::HookFired(_) => SessionEventKind::HookFired,
            SessionEventBody::TurnStart(_) => SessionEventKind::TurnStart,
            SessionEventBody::TurnEnd(_) => SessionEventKind::TurnEnd,
            SessionEventBody::TodoWrite(_) => SessionEventKind::TodoWrite,
            SessionEventBody::SurfaceAppend(_) => SessionEventKind::SurfaceAppend,
            SessionEventBody::SurfaceReplace(_) => SessionEventKind::SurfaceReplace,
            SessionEventBody::Error(_) => SessionEventKind::Error,
            SessionEventBody::Custom(_) => SessionEventKind::Custom,
        }
    }
}

/// The common envelope of every logged event. `seq` is assigned by the single
/// writer; payload fields live in [`SessionEventBody`], flattened next to the
/// `kind` tag on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEvent {
    /// Monotonic sequence number, assigned by the single writer.
    /// Equals the number of events written before this one.
    pub seq: u64,
    /// Wall-clock timestamp, nanoseconds since the Unix epoch.
    pub ts_ns: u64,
    /// Owning session id.
    pub session_id: String,
    /// Turn number.
    pub turn: u64,
    /// Step within the turn, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<u64>,
    /// Span id, when the event participates in tracing spans.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    /// Parent span id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    /// Kind plus typed payload, flattened onto the envelope on the wire.
    #[serde(flatten)]
    pub body: SessionEventBody,
}

impl SessionEvent {
    /// Build an envelope with the optional fields unset.
    pub fn new(
        seq: u64,
        ts_ns: u64,
        session_id: impl Into<String>,
        turn: u64,
        body: SessionEventBody,
    ) -> Self {
        Self {
            seq,
            ts_ns,
            session_id: session_id.into(),
            turn,
            step: None,
            span_id: None,
            parent_span_id: None,
            body,
        }
    }

    /// The kind of the carried body.
    pub fn kind(&self) -> SessionEventKind {
        self.body.kind()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_body(kind: SessionEventKind) -> SessionEventBody {
        match kind {
            SessionEventKind::SessionStart => SessionEventBody::SessionStart(SessionStartPayload {
                model: "claude-sonnet-4".into(),
                provider: Some("anthropic".into()),
                cwd: Some("/tmp/proj".into()),
                app_version: Some("0.11.0".into()),
            }),
            SessionEventKind::SessionEndSeed => {
                SessionEventBody::SessionEndSeed(SessionEndSeedPayload {
                    reason: "fork".into(),
                    parent_session_id: Some("parent-1".into()),
                })
            }
            SessionEventKind::UserMessage => SessionEventBody::UserMessage(UserMessagePayload {
                source: UserMessagePayload::SOURCE_USER.into(),
                content: "hello world".into(),
            }),
            SessionEventKind::AssistantChunk => {
                SessionEventBody::AssistantChunk(AssistantChunkPayload {
                    delta: "par".into(),
                    thinking: true,
                })
            }
            SessionEventKind::AssistantMessage => {
                SessionEventBody::AssistantMessage(AssistantMessagePayload {
                    content: "final answer".into(),
                    usage: Some(TokenUsage {
                        input_tokens: 10,
                        output_tokens: 20,
                        cache_creation_tokens: 5,
                        cache_read_tokens: 6,
                        cost_usd: Some(0.001),
                    }),
                    interrupted: false,
                })
            }
            SessionEventKind::ToolCall => SessionEventBody::ToolCall(ToolCallPayload {
                tool_use_id: "toolu_1".into(),
                tool_name: "Bash".into(),
                arguments: r#"{"command":"ls"}"#.into(),
            }),
            SessionEventKind::ToolResult => SessionEventBody::ToolResult(ToolResultPayload {
                tool_use_id: "toolu_1".into(),
                tool_name: "Bash".into(),
                output: "file-a\nfile-b".into(),
                is_error: false,
                duration_ms: Some(42),
                meta: json!({"exit_code": 0}),
            }),
            SessionEventKind::RequestHeader => {
                SessionEventBody::RequestHeader(RequestHeaderPayload {
                    model: "claude-sonnet-4".into(),
                    provider: Some("anthropic".into()),
                    adapter_defaults: json!({"temperature": 1.0}),
                    system: Some("You are Shannon.".into()),
                    tools: vec![ToolManifestEntry {
                        name: "Bash".into(),
                        schema_hash: "sha256:abc".into(),
                    }],
                    config_snapshot: json!({"max_turns": 20}),
                    reason: Some("initial".into()),
                })
            }
            SessionEventKind::RequestContext => {
                SessionEventBody::RequestContext(RequestContextPayload {
                    input_messages: vec![json!({"role": "user", "content": "hi"})],
                    token_estimate: Some(3),
                })
            }
            SessionEventKind::PermissionDecision => {
                SessionEventBody::PermissionDecision(PermissionDecisionPayload {
                    tool_name: Some("Bash".into()),
                    request: Some("rm -rf /".into()),
                    decision: "deny".into(),
                    reason: Some("destructive".into()),
                    mode: Some("default".into()),
                })
            }
            SessionEventKind::HookFired => SessionEventBody::HookFired(HookFiredPayload {
                event: "PostToolUse".into(),
                hook: "auto-lint".into(),
                outcome: Some("ok".into()),
                duration_ms: Some(7),
            }),
            SessionEventKind::TurnStart => SessionEventBody::TurnStart(TurnStartPayload {
                query_id: Some("q-1".into()),
            }),
            SessionEventKind::TurnEnd => SessionEventBody::TurnEnd(TurnEndPayload {
                reason: TurnEndPayload::REASON_COMPLETED.into(),
                usage: Some(TokenUsage {
                    input_tokens: 1,
                    output_tokens: 2,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    cost_usd: None,
                }),
                error: None,
            }),
            SessionEventKind::TodoWrite => SessionEventBody::TodoWrite(TodoWritePayload {
                todos: vec![TodoSnapshotEntry {
                    content: "write tests".into(),
                    status: "in_progress".into(),
                }],
            }),
            SessionEventKind::SurfaceAppend => {
                SessionEventBody::SurfaceAppend(SurfaceAppendPayload {
                    start_seq: 3,
                    end_seq: 5,
                })
            }
            SessionEventKind::SurfaceReplace => {
                SessionEventBody::SurfaceReplace(SurfaceReplacePayload {
                    start_seq: 3,
                    end_seq: 40,
                    source_event_seqs: vec![3, 4, 5],
                    reason: "compaction".into(),
                })
            }
            SessionEventKind::Error => SessionEventBody::Error(ErrorPayload {
                category: "rate_limit".into(),
                message: "429 too many requests".into(),
                detail: Some(json!({"retry_after_s": 30})),
            }),
            SessionEventKind::Custom => SessionEventBody::Custom(CustomPayload {
                namespace: "com.example.plugin".into(),
                data: json!({"k": "v"}),
            }),
        }
    }

    fn sample_event(kind: SessionEventKind, with_optionals: bool) -> SessionEvent {
        let mut event = SessionEvent::new(
            7,
            1_756_200_000_000_000_000,
            "session-1",
            2,
            sample_body(kind),
        );
        if with_optionals {
            event.step = Some(1);
            event.span_id = Some("span-1".into());
            event.parent_span_id = Some("span-0".into());
        }
        event
    }

    #[test]
    fn test_vocabulary_has_exactly_18_kinds() {
        assert_eq!(SessionEventKind::ALL.len(), 18);
    }

    #[test]
    fn test_kind_str_roundtrip_all_kinds() {
        for kind in SessionEventKind::ALL {
            let s = kind.as_str();
            assert_eq!(s.parse::<SessionEventKind>().unwrap(), kind, "kind {s}");
            assert_eq!(s, kind.to_string());
        }
    }

    #[test]
    fn test_unknown_kind_string_rejected() {
        assert!("bogus/kind".parse::<SessionEventKind>().is_err());
        assert_eq!(
            "bogus/kind".parse::<SessionEventKind>().unwrap_err(),
            UnknownEventKindError("bogus/kind".to_string())
        );
    }

    /// Verification standard ①: serde roundtrip per kind, with and without
    /// the optional envelope fields.
    #[test]
    fn test_serde_roundtrip_per_kind() {
        for kind in SessionEventKind::ALL {
            for with_optionals in [false, true] {
                let event = sample_event(kind, with_optionals);
                let line = serde_json::to_string(&event).unwrap();
                let back: SessionEvent = serde_json::from_str(&line).unwrap();
                assert_eq!(back, event, "roundtrip failed for {}", kind.as_str());
                assert_eq!(back.kind(), kind);
            }
        }
    }

    #[test]
    fn test_wire_shape_kind_tag_and_flattened_payload() {
        let event = sample_event(SessionEventKind::ToolCall, false);
        let value: serde_json::Value = serde_json::to_value(&event).unwrap();
        // Envelope fields at top level.
        assert_eq!(value["seq"], json!(7));
        assert_eq!(value["session_id"], json!("session-1"));
        assert_eq!(value["turn"], json!(2));
        // `kind` tag and payload fields flattened at top level.
        assert_eq!(value["kind"], json!("tool/call"));
        assert_eq!(value["tool_name"], json!("Bash"));
        assert!(value.get("payload").is_none(), "no nested payload key");
        assert!(value.get("body").is_none(), "no nested body key");
    }

    #[test]
    fn test_optional_envelope_fields_omitted_when_none() {
        let line =
            serde_json::to_string(&sample_event(SessionEventKind::TurnStart, false)).unwrap();
        assert!(!line.contains("\"step\""));
        assert!(!line.contains("\"span_id\""));
        assert!(!line.contains("\"parent_span_id\""));
        let with = serde_json::to_string(&sample_event(SessionEventKind::TurnStart, true)).unwrap();
        assert!(with.contains("\"step\":1"));
        assert!(with.contains("\"span_id\":\"span-1\""));
        assert!(with.contains("\"parent_span_id\":\"span-0\""));
    }

    #[test]
    fn test_unknown_kind_deserialization_rejected_by_default() {
        let line = r#"{"seq":0,"ts_ns":1,"session_id":"s","turn":0,"kind":"plugin/future","x":1}"#;
        let err = serde_json::from_str::<SessionEvent>(line).unwrap_err();
        assert!(
            err.to_string().contains("plugin/future"),
            "error should name the unknown kind: {err}"
        );
    }

    #[test]
    fn test_missing_envelope_field_rejected() {
        let line = r#"{"seq":0,"kind":"turn/start"}"#;
        assert!(serde_json::from_str::<SessionEvent>(line).is_err());
    }

    #[test]
    fn test_defaults_let_minimal_payloads_parse() {
        // Older/simpler writers may omit defaulted payload fields.
        let line = r#"{"seq":0,"ts_ns":1,"session_id":"s","turn":0,"kind":"assistant/chunk","delta":"hi"}"#;
        let event: SessionEvent = serde_json::from_str(line).unwrap();
        match event.body {
            SessionEventBody::AssistantChunk(chunk) => assert!(!chunk.thinking),
            other => panic!("wrong body: {other:?}"),
        }
    }

    #[test]
    fn test_tool_call_arguments_stay_raw_string() {
        let line = serde_json::to_string(&sample_event(SessionEventKind::ToolCall, false)).unwrap();
        // The raw string is embedded as a JSON string value, not nested JSON.
        assert!(line.contains(r#""arguments":"{\"command\":\"ls\"}""#));
    }
}
