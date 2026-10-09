//! `shannon-api-protocol` — the single source of truth for Shannon's
//! `api_server` wire contract (REST, SSE, WebSocket).
//!
//! Why a dedicated crate? Phase A of the consolidation runbook turns this
//! contract into the canonical schema that **every** consumer
//! (gateway, desktop, code, private mobile / service clients) reads from.
//! Keeping the types here means a single change site: move a field, the
//! runtime, the codegen binary (`gen-ts`), the gateway's generated
//! `types.gen.ts`, and the doc all update together — no drift.
//!
//! ## Design rules
//!
//! - **Pure serde / uuid only.** No axum, no tower, no engine internals —
//!   the protocol crate is a leaf. Engine/server code depends on this crate,
//!   not the other way around.
//! - **Field names match the wire 1:1.** Everything is `snake_case` because
//!   Rust's serde default rename is `snake_case`; we never want a transform
//!   layer between the wire and the type.
//! - **`#[serde(tag = "type")]` on every WebSocket enum.** The discriminated
//!   union is what makes `{ "type": "text", ... }` round-trippable; `gen-ts`
//!   reuses the same shape on the TypeScript side.
//! - **`#[serde(default)]` on every optional field.** Old payloads must keep
//!   parsing when new optional fields are added — see the `session_id`
//!   round-trip test for the contract.
//! - **`ApprovalDecision` lives here; the engine's `PermissionChoice`
//!   conversion stays in `shannon-core`.** Decoupling means the HTTP contract
//!   stays stable when the engine enum grows new variants.
//!
//! ## Protocol version
//!
//! [`PROTOCOL_VERSION`] is the wire-level version of this crate. The first
//! `WsServerMessage::SessionInfo` frame on every connection carries
//! `protocol_version` (added in a backward-compatible way — existing
//! parsers ignore unknown fields). When you add a breaking change, bump it
//! here and the runtime, and downstream clients can refuse connections
//! whose version they do not understand.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable, monotonically increasing wire-protocol version. Bumped whenever a
/// change to the published types alters the on-the-wire bytes in a
/// non-backward-compatible way. Read it from
/// `WsServerMessage::SessionInfo::protocol_version`.
pub const PROTOCOL_VERSION: &str = "0.8.0";
// R2-W2 additive batch (session enumeration for the phone's session surface +
// rich approval payloads): `sessions.list` / `session.history` client frames,
// their `sessions.snapshot` / `session.transcript` responses, and the optional
// `ts` / `agent` / `risk` fields on `ApprovalRequest`. Every addition is a new
// variant (old servers never emit it) or an `Option` with `#[serde(default)]`
// (old payloads keep parsing) — backward compatible per the policy above, so
// the version deliberately stays at 0.8.0.
//
// Tool-result cards batch (cross-repo spec §R, 2026-10): the same policy —
// optional `tool_use_id` / `ts` on `ToolUse`, optional `tool_use_id` /
// `is_error` / `meta` / `ts` on `ToolResult` (fields the WS projection used to
// drop, now forwarded), and the optional `artifacts` fold on
// `TranscriptMessage` (L0-derived, empty-absent). No key changed meaning; the
// version stays at 0.8.0.
//
// B6.0-4 approval-TTL batch (2026-10-09 ruling): optional `approval_ttl_ms` on
// the `query` client frame — a per-request override of the approval resolver's
// wait window. Absent/zero keeps the legacy 300s behavior byte-for-byte; the
// server clamps the override at 24h. Additive `Option`, so the version stays
// at 0.8.0.

// ── HTTP request / response types ───────────────────────────────────────

/// JSON body for `POST /api/query`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct QueryRequest {
    /// The user prompt to send to the LLM.
    pub prompt: String,
    /// Optional model override (e.g. `"claude-sonnet-4"`, `"gpt-4o"`).
    #[serde(default)]
    pub model: Option<String>,
    /// Optional client-supplied session identity (a UUID string). When omitted
    /// or unparseable the server mints a fresh UUID. Lets a caller attribute
    /// successive requests to the same conversation session; cross-request
    /// history persistence is wired up in P0-e (the contract lands here).
    #[serde(default)]
    pub session_id: Option<String>,
    /// Optional multimodal attachments delivered alongside `prompt`.
    #[serde(default)]
    pub attachments: Option<Vec<MessageAttachment>>,
    /// K4/P2-3: optional per-query approval mode (one of the seven tokens).
    /// Absent = the server-side default (settings `defaultMode` / profile /
    /// engine default). Refused with 400 on unknown tokens; bypass is
    /// subject to the same server-side guardrails as the CLI.
    #[serde(default)]
    pub approval_mode: Option<String>,
}

/// Aggregated JSON response returned by `POST /api/query`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct QueryResponse {
    /// The full text content produced by the LLM.
    pub text: String,
    /// The model that was used.
    pub model: String,
    /// Token usage breakdown.
    #[serde(default)]
    pub usage: Option<UsageInfo>,
    /// Any error that occurred (non-fatal accumulation).
    #[serde(default)]
    pub errors: Vec<String>,
    /// The session id attributed to this query — echoes the client-supplied
    /// `session_id` when one was provided, otherwise the freshly-minted UUID
    /// the server used. Lets callers record which session a stateless request
    /// was attributed to (`#[serde(default)]` keeps old payloads parseable).
    #[serde(default)]
    pub session_id: Uuid,
}

/// Token usage information included in the query response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct UsageInfo {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

/// JSON response for `GET /api/health`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// JSON response for `GET /api/models`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct ModelsResponse {
    pub models: Vec<ModelInfo>,
}

/// Information about a single available model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
    /// Human-readable display name, when the source catalog carries one
    /// (WP-15 T5: the gateway's model picker shows labels). Optional and
    /// omitted when unset, so existing clients keep parsing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// JSON response for `POST /api/tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct ToolsListResponse {
    pub tools: Vec<ToolEntry>,
}

/// Summary of a single registered tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct ToolEntry {
    pub name: String,
    pub description: String,
}

// ── Approval wire types (P0-b) ──────────────────────────────────────────

/// Wire representation of a human's approval decision for `POST
/// /api/approval/respond`. Decoupled from the engine's `PermissionChoice` so
/// the HTTP contract stays stable when the engine enum grows new variants.
/// N3 note: `Copy` was dropped when `AlwaysAllowKind` started carrying the
/// `kind` payload — every consumer already worked by value or reference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub enum ApprovalDecision {
    #[serde(rename = "allow_once")]
    AllowOnce,
    #[serde(rename = "always_allow")]
    AlwaysAllow,
    /// P3-3: in-session always-allow (never persisted). The mobile client's
    /// "allow for this session" scope maps here.
    #[serde(rename = "always_allow_session")]
    AlwaysAllowSession,
    /// N3: per-kind trust grant (the mobile "always allow this category"
    /// scope). `kind` is the approval's tool name on the wire — the grant is
    /// persisted to the engine's per-kind trust store (`~/.shannon/trust/
    /// kinds.toml`) and auto-approves subsequent requests whose tool name
    /// matches EXACTLY. Never a global always-allow: deny/ask rules and the
    /// global deny gate keep binding. Wire form:
    /// `{"always_allow_kind": {"kind": "Bash"}}`.
    #[serde(rename = "always_allow_kind")]
    AlwaysAllowKind { kind: String },
    #[serde(rename = "deny")]
    Deny,
}

/// JSON body for `POST /api/approval/respond`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct ApprovalRespondRequest {
    pub request_id: String,
    pub choice: ApprovalDecision,
}

/// P3-3: response for `GET /api/approval/mode` — the approval token
/// currently in effect for the session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct ApprovalModeState {
    /// One of the seven approval tokens (`ask`, `plan`, `auto-edit`,
    /// `full-auto`, `readonly`, `dontAsk`, `bypassPermissions`).
    pub mode: String,
}

/// P3-3: body for `POST /api/approval/mode`. The mobile gateway may only
/// TIGHTEN — the route rejects everything except `readonly`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct ApprovalModeRequest {
    /// Target session. Unknown ids are rejected (the gateway only ever
    /// tightens a session it is attached to).
    pub session_id: String,
    /// Must be `readonly` (the mobile one-tap tighten).
    pub mode: String,
}

// ── N3 per-kind trust wire types ────────────────────────────────────────

/// N3: response for `GET /api/trust/kinds` — the active per-kind trust
/// grants, sorted by kind. Empty `kinds` when nothing is trusted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct TrustKindsResponse {
    pub kinds: Vec<TrustedKindEntry>,
}

/// N3: one active per-kind trust grant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct TrustedKindEntry {
    /// The trusted kind — the engine tool name, matched exactly on later
    /// approval requests (no globs, no prefix semantics).
    pub kind: String,
    /// When the grant was made, epoch milliseconds.
    pub granted_at: u64,
}

/// N3: body for `POST /api/trust/revoke`. Revocation is idempotent and takes
/// effect immediately — every subsequent permission check re-reads the live
/// store, so the next matching request prompts again.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct TrustRevokeRequest {
    /// The kind to revoke. Unknown kinds report `revoked: false` (idempotent).
    pub kind: String,
}

/// N3: response for `POST /api/trust/revoke`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct TrustRevokeResponse {
    /// Echo of the revoked kind.
    pub kind: String,
    /// True when a grant was actually removed; false when nothing was trusted
    /// under this kind (idempotent replay).
    pub revoked: bool,
}

// ── SSE event-name contract ─────────────────────────────────────────────

/// Canonical SSE `event:` names for every streaming endpoint that carries
/// `QueryEvent`-shaped traffic (`POST /api/query/stream`, the deprecated
/// `GET /api/query/stream`, and the headless server's
/// `POST /v1/sessions/:id/messages`).
///
/// This enum is the wire contract (review §P2-6): every SSE producer must
/// emit exactly these names. The exhaustive `QueryEvent → SseEventName`
/// mapping lives in `shannon-core::query_engine::sse` — next to the event
/// enum itself, because `QueryEvent` references engine types and this crate
/// must stay a leaf — so adding a variant there breaks compilation until the
/// mapping (and therefore this contract) is extended deliberately. The
/// `gen-ts` binary publishes the names to gateway clients as the
/// `SseEventName` string-literal union.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename_all = "snake_case")]
pub enum SseEventName {
    /// Query started processing.
    Started,
    /// Text chunk from the model.
    Text,
    /// Tool use requested by the model.
    ToolUseRequest,
    /// Tool execution completed.
    ToolUseResult,
    /// Turn completed (multi-turn query).
    TurnCompleted,
    /// Query completed successfully.
    Completed,
    /// Query failed with an error.
    Failed,
    /// Non-fatal warning; the query continues.
    Warning,
    /// Progress update.
    Progress,
    /// Tool execution progress update.
    ToolProgress,
    /// Thinking content from extended thinking mode.
    Thinking,
    /// Token usage statistics.
    Usage,
    /// Cost summary.
    Cost,
    /// Informational event (compaction metrics, context pressure, …).
    Info,
    /// Updated conversation state.
    ConversationUpdate,
    /// Rate-limit info from provider response headers.
    RateLimit,
    /// Transport-level error channel — not a `QueryEvent` payload. Used
    /// when the query stream itself errors (`{"error": …}`) and, per §P3-4,
    /// when an event's serialization fails (the payload then carries
    /// `{"error": …, "event_type": …}` naming the event that was lost).
    Error,
}

impl SseEventName {
    /// The exact string emitted in the SSE `event:` field.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Text => "text",
            Self::ToolUseRequest => "tool_use_request",
            Self::ToolUseResult => "tool_use_result",
            Self::TurnCompleted => "turn_completed",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Warning => "warning",
            Self::Progress => "progress",
            Self::ToolProgress => "tool_progress",
            Self::Thinking => "thinking",
            Self::Usage => "usage",
            Self::Cost => "cost",
            Self::Info => "info",
            Self::ConversationUpdate => "conversation_update",
            Self::RateLimit => "rate_limit",
            Self::Error => "error",
        }
    }
}

// ── WebSocket protocol messages ─────────────────────────────────────────

/// A base64-encoded media attachment delivered alongside a message
/// (REST `/v1/sessions/:id/messages`, `POST /api/query`, and the
/// `WsClientMessage::Query` frame). B4: the gateway maps IM media onto
/// this shape; the server validates MIME/size identically on every path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct MessageAttachment {
    /// File name (informational; shown to the model in the message text).
    #[serde(default)]
    pub name: Option<String>,
    /// MIME type. Supported: image/png, image/jpeg, image/gif, image/webp.
    pub media_type: String,
    /// Base64-encoded file bytes.
    pub data: String,
}

/// Incoming message from a WebSocket client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum WsClientMessage {
    /// Send a query to the LLM.
    #[serde(rename = "query")]
    Query {
        prompt: String,
        model: Option<String>,
        /// Optional session id override (UUID string). When omitted the
        /// connection's own session id is used. Lets a caller multiplex
        /// several conversations over a single socket.
        #[serde(default)]
        session_id: Option<String>,
        /// Optional multimodal attachments (B4) — validated and attached to
        /// the query exactly like the REST paths.
        #[serde(default)]
        attachments: Option<Vec<MessageAttachment>>,
        /// B6.0-4 TTL (2026-10-09 ruling): optional per-request approval
        /// window override, in milliseconds. When absent or zero the server
        /// keeps the legacy resolver behavior (300s → Deny); a supplied value
        /// is honored up to a 24h ceiling (86_400_000 ms, server-clamped).
        /// `skip_serializing_if` keeps the emitted frame byte-identical to
        /// the pre-TTL shape whenever the override is not requested. Old
        /// servers ignore the unknown key; a server that predates the field
        /// simply keeps timing out at 300s.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approval_ttl_ms: Option<u64>,
    },
    /// Clear conversation history for this session.
    #[serde(rename = "clear")]
    Clear,
    /// Request current session info.
    #[serde(rename = "info")]
    Info,
    /// Cancel the current in-progress query.
    #[serde(rename = "cancel")]
    Cancel,
    /// K4/P2-3: request an approval-mode change for this connection's
    /// session. Answered with [`WsServerMessage::ApprovalMode`]. The server
    /// is the authority: a mode the server refuses (unknown token, or
    /// bypass/dontAsk blocked by policy) answers `ok: false` with the reason.
    #[serde(rename = "approval.mode")]
    SetApprovalMode {
        /// One of the seven approval tokens: `ask`, `plan`, `auto-edit`,
        /// `full-auto`, `readonly`, `dontAsk`, `bypassPermissions`.
        mode: String,
    },
    /// Enumerate the persisted sessions the engine can serve (R2-W2: the
    /// phone's session picker). Answered with
    /// [`WsServerMessage::SessionsSnapshot`].
    #[serde(rename = "sessions.list")]
    SessionsList,
    /// Request one page of a session's stored transcript (R2-W2: the phone's
    /// history backfill). Answered with
    /// [`WsServerMessage::SessionTranscript`]; a session the engine has no
    /// log for answers an EMPTY transcript rather than an error — callers
    /// treat that as "the server has no content for this session yet".
    #[serde(rename = "session.history")]
    SessionHistory {
        /// The session to read. An id the engine has no log for yields an
        /// empty transcript (no error), so a malformed or foreign id is
        /// handled with the same quiet path.
        session_id: String,
        /// ISO-8601 UTC cursor: the page carries the messages strictly OLDER
        /// than this timestamp. Absent = the latest page.
        #[serde(default)]
        before: Option<String>,
        /// Page size. Absent = 50; values below 1 clamp to 1; values above
        /// 500 clamp to 500.
        #[serde(default)]
        limit: Option<u32>,
    },
}

/// Outgoing message sent to a WebSocket client.
///
/// The first `SessionInfo` frame emitted on every connection also carries
/// `protocol_version`, so a client can refuse to interoperate when the
/// server is on an unexpected protocol version. The field is
/// `#[serde(default)]` so legacy clients that only read `message_count` /
/// `model` keep parsing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum WsServerMessage {
    /// A text chunk from the LLM response.
    #[serde(rename = "text")]
    Text { content: String },
    /// Reasoning-channel content (WP-15 P0-2): the model's chain-of-thought,
    /// either from a native thinking stream or re-split from inline
    /// `<think>` blocks by the engine. Additive in 0.7.0 — clients that
    /// don't know the variant never receive it from older servers, and a
    /// newer server may simply not emit it. Clients may render it as a
    /// collapsible thinking section or ignore it.
    #[serde(rename = "thinking")]
    Thinking { content: String },
    /// Tool use event.
    ///
    /// Additive fields (tool-result cards, cross-repo spec §R): `tool_use_id`
    /// lets clients pair use→result frames into one card; `ts` is the
    /// forward-time stamp (epoch ms, the §L1 approval `ts` precedent). Both
    /// stay absent on legacy servers, and clients ignore unknown keys.
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        input: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_use_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ts: Option<u64>,
    },
    /// Tool result event.
    ///
    /// Additive fields (§R): `tool_use_id`/`is_error` recover engine state the
    /// WS projection used to drop, `meta` passes the §4.12 tool-private
    /// metadata through uninterpreted, `ts` is the forward-time stamp. There
    /// is deliberately NO `duration_ms` here — the live `QueryEvent` does not
    /// carry one and the hosts do not synthesize timings; wall-clock duration
    /// is served only by the L0-derived `TranscriptMessage` artifacts.
    #[serde(rename = "tool_result")]
    ToolResult {
        name: String,
        output: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_use_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        meta: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ts: Option<u64>,
    },
    /// Token usage update.
    #[serde(rename = "usage")]
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
    },
    /// Query completed.
    #[serde(rename = "completed")]
    Completed { model: String },
    /// Query failed.
    #[serde(rename = "failed")]
    Failed { error: String },
    /// Query was cancelled by the client via `WsClientMessage::Cancel`. Emitted
    /// after the in-progress query's event stream has been dropped (which aborts
    /// the engine's producer task).
    #[serde(rename = "cancelled")]
    Cancelled,
    /// Engine requests human approval for a tool call. The client responds via
    /// `POST /api/approval/respond` with the matching `request_id`. The R2-W2
    /// enrichment fields (`ts`, `agent`, `risk`) are all optional with
    /// `#[serde(default)]`: clients built against the pre-0.8 shape ignore
    /// them, and an engine that cannot honestly populate one leaves it `None`.
    #[serde(rename = "approval_request")]
    ApprovalRequest {
        request_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        description: String,
        is_destructive: bool,
        diff_preview: Option<String>,
        /// When the request was raised, epoch milliseconds (R2-W2: the phone
        /// renders approval age). `None` from engines that don't stamp it.
        #[serde(default)]
        ts: Option<u64>,
        /// The agent/profile context the request was issued under, when the
        /// engine tracks one (R2-W2). `None` when no active-agent context
        /// exists — callers must not guess.
        #[serde(default)]
        agent: Option<AgentRef>,
        /// Scope/reversibility classification of the operation (R2-W2).
        /// `None` until the engine carries a real scope/reversible verdict —
        /// never synthesized from `is_destructive` or risk levels.
        #[serde(default)]
        risk: Option<RiskInfo>,
    },
    /// Session info response. The greeting emitted on connection carries the
    /// server's [`PROTOCOL_VERSION`] in `protocol_version` so clients can
    /// reject incompatible servers early. Existing clients that ignore
    /// unknown fields continue to parse the legacy `message_count` / `model`
    /// pair without modification.
    #[serde(rename = "session_info")]
    SessionInfo {
        message_count: usize,
        model: Option<String>,
        /// Wire-level protocol version. `#[serde(default)]` keeps older
        /// payloads (no `protocol_version`) parseable.
        #[serde(default)]
        protocol_version: Option<String>,
    },
    /// Error in protocol.
    #[serde(rename = "error")]
    Error { message: String },
    /// K4/P2-3: answer to `WsClientMessage::SetApprovalMode` — echoes the
    /// mode that is NOW in effect (the server's choice, not the request's).
    #[serde(rename = "approval.mode")]
    ApprovalMode {
        /// The effective approval token after the change (unchanged when
        /// `ok` is false).
        mode: String,
        /// False when the server refused the change; `error` carries why.
        ok: bool,
        #[serde(default)]
        error: Option<String>,
    },
    /// Answer to `WsClientMessage::SessionsList` (R2-W2): the persisted
    /// sessions, most recently active first.
    #[serde(rename = "sessions.snapshot")]
    SessionsSnapshot {
        /// One summary per persisted session. Empty when the engine has no
        /// sessions yet — an empty snapshot is an answer, not an error.
        sessions: Vec<SessionSummary>,
    },
    /// Answer to `WsClientMessage::SessionHistory` (R2-W2): one page of the
    /// session's stored transcript, in chronological (ascending `ts`) order.
    #[serde(rename = "session.transcript")]
    SessionTranscript {
        /// Echoes the requested `session_id` — including the unknown-id case,
        /// which answers an empty transcript instead of an error.
        session_id: String,
        /// The page's messages, ascending by `ts`.
        messages: Vec<TranscriptMessage>,
        /// True when still-older messages exist beyond this page (paging
        /// continues by re-requesting with `before` = this page's first `ts`).
        has_more: bool,
    },
}

/// One persisted session in a [`WsServerMessage::SessionsSnapshot`] (R2-W2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct SessionSummary {
    /// Session id (a UUID string).
    pub session_id: String,
    /// Curated title, when the session carries one.
    #[serde(default)]
    pub title: Option<String>,
    /// First user-message preview.
    #[serde(default)]
    pub preview: Option<String>,
    /// Session start, RFC3339 UTC.
    pub created_at: String,
    /// Last activity, RFC3339 UTC.
    pub updated_at: String,
    /// Started turns.
    pub turn_count: u64,
    /// Cumulative input tokens.
    pub total_input_tokens: u64,
    /// Cumulative output tokens.
    pub total_output_tokens: u64,
}

/// One chat-visible message in a [`WsServerMessage::SessionTranscript`] (R2-W2).
///
/// Only messages with real text content are projected — tool-call bookkeeping
/// (tool_use-only assistant steps, tool_result user messages) stays out of
/// `content`, so `role` is exactly `"user"` (a prompt) or `"assistant"` (a
/// reply). A turn's tool products ride alongside as `artifacts` (below): a
/// turn with tool results but no final assistant text projects as an empty
/// `content` entry carrying them, so offline replays keep the cards the live
/// stream showed (§O4: artifacts are content, not transients).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct TranscriptMessage {
    /// `"user"` or `"assistant"`.
    pub role: String,
    /// The message's text content.
    pub content: String,
    /// Message timestamp, RFC3339 UTC — also the pagination cursor (clients
    /// echo the page's first `ts` back as `before`).
    pub ts: String,
    /// Tool products folded in from the enclosing turn (§R). Empty (and
    /// omitted on the wire) whenever the turn ran no tools — pre-§R peers
    /// never see the key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<TranscriptArtifact>,
}

/// One folded tool product in a [`TranscriptMessage`] (§R, camelCase — the
/// §J projection face convention). v1 projects `tool_result` entries only
/// (a completed turn's durable outputs); `title`/`summary` are reserved for
/// a future engine-side derivation and are never sent in v1 — clients fall
/// back to `tool`. Field-level degrade: every key except `kind`/`tool` may
/// be absent, and an unusable key is treated as absent, never an error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptArtifact {
    /// `"tool_use" | "tool_result"` (v1 emits `"tool_result"`).
    pub kind: String,
    /// Engine tool name (an open set — render the raw string, never map).
    pub tool: String,
    /// Reserved: human-readable one-liner. Not sent in v1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Reserved: ≤2-line digest. Not sent in v1 (clients slice `body`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Full tool output, within the engine's existing retention.
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub is_error: bool,
    /// Wall-clock execution duration from the L0 payload (absent when the
    /// engine did not measure one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// The artifact's own moment, RFC3339 UTC (≠ the host message's `ts`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    /// §4.12 tool-private metadata, passthrough — clients must not interpret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

/// The agent/profile context an approval request was issued under (R2-W2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct AgentRef {
    /// Stable agent id, when the engine tracks one.
    #[serde(default)]
    pub id: Option<String>,
    /// Human-readable agent/profile name, when known.
    #[serde(default)]
    pub name: Option<String>,
}

/// Scope/reversibility classification of an approved operation (R2-W2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct RiskInfo {
    /// How far the operation reaches.
    pub scope: RiskScope,
    /// Whether the effect can be undone.
    pub reversible: bool,
}

/// How far an operation reaches (`RiskInfo::scope`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(rename_all = "lowercase")]
pub enum RiskScope {
    /// Confined to files/state inside the session's sandbox.
    Local,
    /// Reaches the working repository (checkout, branch, git state).
    Repo,
    /// Reaches beyond the repo — machine or network-wide effect.
    System,
}

impl WsServerMessage {
    /// Build the canonical greeting (the first frame sent on every WS
    /// connection). The `protocol_version` field is always populated so a
    /// new client can detect an old server by absence (it will be `None`
    /// from a pre-Phase A build).
    pub fn greeting(message_count: usize, model: Option<String>) -> Self {
        Self::SessionInfo {
            message_count,
            model,
            protocol_version: Some(PROTOCOL_VERSION.to_string()),
        }
    }
}
