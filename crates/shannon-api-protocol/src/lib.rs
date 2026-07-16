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
pub const PROTOCOL_VERSION: &str = "0.6.0";

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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub enum ApprovalDecision {
    #[serde(rename = "allow_once")]
    AllowOnce,
    #[serde(rename = "always_allow")]
    AlwaysAllow,
    #[serde(rename = "deny")]
    Deny,
}

/// JSON body for `POST /api/approval/respond`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct ApprovalRespondRequest {
    pub request_id: String,
    pub choice: ApprovalDecision,
}

// ── WebSocket protocol messages ─────────────────────────────────────────

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
    /// Tool use event.
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        input: serde_json::Value,
    },
    /// Tool result event.
    #[serde(rename = "tool_result")]
    ToolResult { name: String, output: String },
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
    /// `POST /api/approval/respond` with the matching `request_id`.
    #[serde(rename = "approval_request")]
    ApprovalRequest {
        request_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        description: String,
        is_destructive: bool,
        diff_preview: Option<String>,
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
