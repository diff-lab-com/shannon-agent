//! # L0 Projections (§4.6 W1-P1)
//!
//! Everything consumers used to read from side files is **derived** here from
//! the authoritative [`SessionEvent`] log:
//!
//! - [`project_conversation`]: rebuild the engine message history, usage
//!   totals, and turn structure (the restore path — `events.jsonl` is the
//!   only source of session state).
//! - [`project_analytics_jsonl`]: the analytics aggregate view (per-session
//!   JSONL) with the eight legacy dimensions preserved (tool execution,
//!   prompt submitted, response received, file operation, session start/end,
//!   error, permission request).
//! - [`search_events`]: transcript-style full-text search over the log.
//! - [`scan_session_summaries`] / [`SessionScanEntry`]: the directory scan
//!   behind session listing (L0 layout: `<container>/<uuid>/events.jsonl`).
//!
//! Projections are pure functions of an event slice: no I/O except where a
//! function's name says so, and never a write to L0.

use std::path::{Path, PathBuf};

use chrono::TimeZone;
use shannon_types::session_event::{
    PermissionDecisionPayload, SessionEvent, SessionEventBody, TokenUsage, ToolResultPayload,
    TurnEndPayload,
};

/// Conversation rebuild output ([`project_conversation`]).
#[derive(Debug, Clone)]
pub struct ConversationProjection {
    /// Rebuilt message history. Mirrors what the live engine accumulates:
    /// `user/message` → user text messages; assistant `assistant/chunk`
    /// deltas plus `tool/call`s → one assistant blocks message per step;
    /// each `tool/result` → its own user message carrying one
    /// `tool_result` block.
    pub messages: Vec<shannon_engine::api::Message>,
    /// For every projected message, the inclusive `[first_seq, last_seq]`
    /// event range it was derived from. Used by branch cut-off (`/branch`)
    /// and by trace tooling to attribute a message to log rows.
    pub message_origin_seqs: Vec<(u64, u64)>,
    /// Number of started turns (`turn/start` events).
    pub turn_count: usize,
    /// Summed token usage across all `turn/end` (and finalized-step)
    /// payloads.
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
    /// Summed USD cost when every contributing event carried one.
    pub total_cost_usd: Option<f64>,
    /// Number of `tool/call` events.
    pub tool_call_count: usize,
    /// Number of `tool/result` events flagged as errors.
    pub tool_error_count: usize,
}

impl Default for ConversationProjection {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            message_origin_seqs: Vec::new(),
            turn_count: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cache_creation_tokens: 0,
            total_cache_read_tokens: 0,
            total_cost_usd: None,
            tool_call_count: 0,
            tool_error_count: 0,
        }
    }
}

impl ConversationProjection {
    fn add_usage(&mut self, usage: &shannon_types::session_event::TokenUsage) {
        self.total_input_tokens += usage.input_tokens;
        self.total_output_tokens += usage.output_tokens;
        self.total_cache_creation_tokens += usage.cache_creation_tokens;
        self.total_cache_read_tokens += usage.cache_read_tokens;
        self.total_cost_usd = match (self.total_cost_usd, usage.cost_usd) {
            (Some(a), Some(b)) => Some(a + b),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        };
    }
}

// ============================================================================
// Conversation projection (restore path)
// ============================================================================

/// One assistant step being accumulated from streamed chunks + tool calls.
struct AssistantStep {
    first_seq: u64,
    last_seq: u64,
    text: String,
    tool_uses: Vec<shannon_engine::api::ContentBlock>,
}

impl AssistantStep {
    fn new(first_seq: u64) -> Self {
        Self {
            first_seq,
            last_seq: first_seq,
            text: String::new(),
            tool_uses: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty() && self.tool_uses.is_empty()
    }
}

/// State machine that folds L0 bodies into engine-shaped messages.
///
/// Push order deliberately mirrors the live query loop: the finalized
/// assistant `(text?)+tool_use blocks` message lands before the tool result
/// that answers it, and each tool result becomes exactly one user message
/// holding one block (the engine drains pending results one message each).
struct ConversationFolder {
    out: ConversationProjection,
    step: Option<AssistantStep>,
}

impl ConversationFolder {
    fn new() -> Self {
        Self {
            out: ConversationProjection::default(),
            step: None,
        }
    }

    /// Close the open assistant step, if any material accumulated.
    fn flush_step(&mut self) {
        let Some(step) = self.step.take() else {
            return;
        };
        if step.is_empty() {
            return;
        }
        let mut blocks = Vec::with_capacity(1 + step.tool_uses.len());
        if !step.text.is_empty() {
            blocks.push(shannon_engine::api::ContentBlock::Text {
                text: step.text.clone(),
            });
        }
        blocks.extend(step.tool_uses.iter().cloned());
        self.out.messages.push(shannon_engine::api::Message {
            role: "assistant".to_string(),
            content: shannon_engine::api::MessageContent::Blocks(blocks),
        });
        self.out
            .message_origin_seqs
            .push((step.first_seq, step.last_seq));
    }

    fn push_user_text(&mut self, seq: u64, content: &str) {
        self.out.messages.push(shannon_engine::api::Message {
            role: "user".to_string(),
            content: shannon_engine::api::MessageContent::Text(content.to_string()),
        });
        self.out.message_origin_seqs.push((seq, seq));
    }

    /// Fold one durable event body into the conversation state.
    fn fold(&mut self, event: &SessionEvent) {
        match &event.body {
            SessionEventBody::UserMessage(p) => {
                // A real prompt always starts fresh conversation content;
                // anything half-accumulated belongs to the previous span.
                self.flush_step();
                self.push_user_text(event.seq, &p.content);
            }
            SessionEventBody::AssistantChunk(p) => {
                // Thinking deltas are transient stream detail; the live
                // conversation keeps only text + tool_use blocks.
                if p.thinking {
                    return;
                }
                let step = self
                    .step
                    .get_or_insert_with(|| AssistantStep::new(event.seq));
                step.last_seq = event.seq;
                step.text.push_str(&p.delta);
            }
            SessionEventBody::AssistantMessage(p) => {
                // Authoritative finalize (interrupt coalescing): replaces any
                // partially streamed text for this step.
                let first_seq = self.step.as_ref().map_or(event.seq, |s| s.first_seq);
                let mut step = AssistantStep::new(first_seq);
                step.last_seq = event.seq;
                step.text = p.content.clone();
                self.step = Some(step);
            }
            SessionEventBody::ToolCall(p) => {
                let input: serde_json::Value =
                    serde_json::from_str(&p.arguments).unwrap_or(serde_json::Value::Null);
                let step = self
                    .step
                    .get_or_insert_with(|| AssistantStep::new(event.seq));
                step.last_seq = event.seq;
                step.tool_uses
                    .push(shannon_engine::api::ContentBlock::ToolUse {
                        id: p.tool_use_id.clone(),
                        name: p.tool_name.clone(),
                        input,
                    });
                self.out.tool_call_count += 1;
            }
            SessionEventBody::ToolResult(p) => {
                // The result finalizes the assistant step that asked for it —
                // matches the live loop's push order.
                self.flush_step();
                self.push_tool_result(event.seq, p);
            }
            SessionEventBody::TurnStart(_) => {
                self.out.turn_count += 1;
            }
            SessionEventBody::TurnEnd(TurnEndPayload { usage, .. }) => {
                self.flush_step();
                if let Some(usage) = usage {
                    self.out.add_usage(usage);
                }
            }
            _ => {}
        }
    }

    fn push_tool_result(&mut self, seq: u64, p: &ToolResultPayload) {
        if p.is_error {
            self.out.tool_error_count += 1;
        }
        self.out.messages.push(shannon_engine::api::Message {
            role: "user".to_string(),
            content: shannon_engine::api::MessageContent::Blocks(vec![
                shannon_engine::api::ContentBlock::ToolResult {
                    tool_use_id: p.tool_use_id.clone(),
                    content: Some(shannon_engine::api::ToolResultContent::Single(
                        p.output.clone(),
                    )),
                    is_error: Some(p.is_error),
                },
            ]),
        });
        self.out.message_origin_seqs.push((seq, seq));
    }

    fn finish(mut self) -> ConversationProjection {
        self.flush_step();
        self.out
    }
}

/// Rebuild the conversation history from L0 events.
///
/// Pure function over the event slice — see [`ConversationProjection`] for
/// the produced shape. This is the single source behind
/// `StateManager::load_session`; no snapshot file is consulted.
pub fn project_conversation(events: &[SessionEvent]) -> ConversationProjection {
    let mut folder = ConversationFolder::new();
    for event in events {
        folder.fold(event);
    }
    folder.finish()
}

// ============================================================================
// ============================================================================
// Turn timeline projection (§4.14 desktop Turn Timeline)
// ============================================================================

/// One tool execution inside a timeline turn (waterfall row).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TimelineToolEntry {
    /// Provider tool-use id this row answers.
    pub tool_use_id: String,
    /// Tool name.
    pub tool_name: String,
    /// `tool/call` timestamp, ns since epoch.
    pub start_ts_ns: u64,
    /// `tool/result` timestamp (falls back to the call ts when unpaired).
    pub end_ts_ns: u64,
    /// Measured duration when the result carried one, else the call→result
    /// delta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Result flagged as error (also true for unpaired/interrupted calls).
    #[serde(default)]
    pub is_error: bool,
}

/// One user-visible turn: window, terminal reason, usage triple, tools.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TimelineTurn {
    /// Turn number from the event envelope.
    pub turn: u64,
    /// First event ts of the turn window.
    pub start_ts_ns: u64,
    /// `turn/end` ts (or last observed event of the turn).
    pub end_ts_ns: u64,
    /// Terminal reason from `turn/end`, when the turn closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Error that ended the turn, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Usage summed over the turn (from the `turn/end` payload).
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Tool executions in call order.
    pub tools: Vec<TimelineToolEntry>,
}

/// One cumulative sample on the token/cost curve (taken at each `turn/end`).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TimelineCumulativePoint {
    /// Sample time (`turn/end` ts), ns since epoch.
    pub ts_ns: u64,
    /// Total output tokens across all closed turns so far.
    pub output_tokens_total: u64,
    /// Total USD cost so far; stays None until any value exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_total_usd: Option<f64>,
}

/// The whole Turn Timeline for one session: turns × tool rows × token/cost
/// accumulation. Consumed by the desktop `trace_timeline` command — derived
/// here so no consumer re-implements the fold over L0.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct TurnTimeline {
    /// Owning session id.
    pub session_id: String,
    /// Model from `session/start`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// First event ts (root of every waterfall axis).
    #[serde(default)]
    pub started_ts_ns: u64,
    /// Last event ts.
    #[serde(default)]
    pub ended_ts_ns: u64,
    /// Per-turn detail rows, in first-appearance order.
    pub turns: Vec<TimelineTurn>,
    /// Running totals sampled at every `turn/end`.
    pub cumulative: Vec<TimelineCumulativePoint>,
}

/// Builder state per envelope-turn number.
#[derive(Default)]
struct TimelineTurnAcc {
    start_ts_ns: u64,
    end_ts_ns: u64,
    reason: Option<String>,
    error: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
    cost_usd: Option<f64>,
    /// Tool rows in call order.
    tools: Vec<TimelineToolAcc>,
}

/// `(tool_use_id, tool_name, call_ts, result?)` — the raw pairing buffer
/// before `TimelineTurnAcc::finish` folds it into render rows.
type TimelineToolAcc = (String, String, u64, Option<(u64, bool, Option<u64>)>);

impl TimelineTurnAcc {
    fn add_usage(&mut self, usage: &TokenUsage) {
        self.input_tokens += usage.input_tokens;
        self.output_tokens += usage.output_tokens;
        self.cache_creation_tokens += usage.cache_creation_tokens;
        self.cache_read_tokens += usage.cache_read_tokens;
        self.cost_usd = match (self.cost_usd, usage.cost_usd) {
            (Some(a), Some(b)) => Some(a + b),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        };
    }

    fn finish(self, turn: u64) -> TimelineTurn {
        TimelineTurn {
            turn,
            start_ts_ns: self.start_ts_ns,
            end_ts_ns: self.end_ts_ns.max(self.start_ts_ns),
            reason: self.reason,
            error: self.error,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_creation_tokens: self.cache_creation_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cost_usd: self.cost_usd,
            tools: self
                .tools
                .into_iter()
                .map(|(id, name, call_ts, done)| {
                    // An unpaired call closes at its own ts and renders as an
                    // error instead of being dropped (interrupted turns).
                    let (end_ts, is_error, duration_ms) = done.unwrap_or((call_ts, true, None));
                    TimelineToolEntry {
                        tool_use_id: id,
                        tool_name: name,
                        start_ts_ns: call_ts.min(end_ts),
                        end_ts_ns: end_ts.max(call_ts),
                        duration_ms,
                        is_error,
                    }
                })
                .collect(),
        }
    }
}

/// Slot-or-create the accumulator for envelope-turn `n`, ordered by first
/// appearance (the log's chronology).
fn timeline_slot(accs: &mut Vec<(u64, TimelineTurnAcc)>, n: u64, ts: u64) -> &mut TimelineTurnAcc {
    if let Some(pos) = accs.iter().position(|(key, _)| *key == n) {
        &mut accs[pos].1
    } else {
        accs.push((
            n,
            TimelineTurnAcc {
                start_ts_ns: ts,
                ..Default::default()
            },
        ));
        let (_, last) = accs.last_mut().expect("just pushed");
        last
    }
}

/// Derive the Turn Timeline projection from one session's events.
///
/// Pure function like [`project_conversation`] — carries render data only;
/// message bodies never leave the log through this view.
pub fn project_turn_timeline(events: &[SessionEvent]) -> TurnTimeline {
    let mut out = TurnTimeline {
        session_id: events
            .first()
            .map(|e| e.session_id.clone())
            .unwrap_or_default(),
        ..Default::default()
    };
    let Some(first_ev) = events.first() else {
        return out;
    };
    let last_ts = events
        .iter()
        .map(|e| e.ts_ns)
        .max()
        .unwrap_or(first_ev.ts_ns);
    out.started_ts_ns = first_ev.ts_ns;
    out.ended_ts_ns = last_ts;

    let mut accs: Vec<(u64, TimelineTurnAcc)> = Vec::new();
    let mut curve_output_total = 0u64;
    let mut curve_cost_total = None::<f64>;

    for event in events {
        match &event.body {
            SessionEventBody::SessionStart(p) => {
                out.model.get_or_insert_with(|| p.model.clone());
            }
            SessionEventBody::ToolCall(p) => {
                let acc = timeline_slot(&mut accs, event.turn, event.ts_ns);
                acc.tools.push((
                    p.tool_use_id.clone(),
                    p.tool_name.clone(),
                    event.ts_ns,
                    None,
                ));
            }
            SessionEventBody::ToolResult(p) => {
                let acc = timeline_slot(&mut accs, event.turn, event.ts_ns);
                let open = acc
                    .tools
                    .iter_mut()
                    .rev()
                    .find(|(id, _, _, done)| id == &p.tool_use_id && done.is_none());
                match open {
                    Some((_, _, call_ts, done)) => {
                        let measured = p
                            .duration_ms
                            .unwrap_or_else(|| event.ts_ns.saturating_sub(*call_ts) / 1_000_000);
                        *done = Some((event.ts_ns, p.is_error, Some(measured)));
                    }
                    // A result whose call was never scanned (truncated log
                    // head, deduped export, …) still renders as one row.
                    None => acc.tools.push((
                        p.tool_use_id.clone(),
                        p.tool_name.clone(),
                        event.ts_ns,
                        Some((event.ts_ns, p.is_error, p.duration_ms)),
                    )),
                }
            }
            SessionEventBody::TurnEnd(TurnEndPayload {
                reason,
                usage,
                error,
            }) => {
                let acc = timeline_slot(&mut accs, event.turn, event.ts_ns);
                acc.end_ts_ns = event.ts_ns.max(acc.start_ts_ns);
                acc.reason.get_or_insert_with(|| reason.clone());
                if acc.error.is_none() {
                    acc.error.clone_from(error);
                }
                if let Some(u) = usage {
                    acc.add_usage(u);
                    curve_output_total += u.output_tokens;
                    curve_cost_total = match (curve_cost_total, u.cost_usd) {
                        (Some(a), Some(b)) => Some(a + b),
                        (Some(a), None) | (None, Some(a)) => Some(a),
                        (None, None) => None,
                    };
                }
                out.cumulative.push(TimelineCumulativePoint {
                    ts_ns: event.ts_ns,
                    output_tokens_total: curve_output_total,
                    cost_total_usd: curve_cost_total,
                });
            }
            // Every other durable content kind anchors/keeps its turn row so
            // windows reflect reality even when `turn/start` predates the
            // scanned slice (branch tails, exported heads).
            _ => {
                timeline_slot(&mut accs, event.turn, event.ts_ns);
            }
        }
    }

    out.turns = accs.into_iter().map(|(n, acc)| acc.finish(n)).collect();
    out
}

// Trace / transcript-style accessors on L0
// ============================================================================

/// One full-text hit from [`search_events`].
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// Seq of the matching event row.
    pub seq: u64,
    /// Kind string of the matching row (e.g. `"user/message"`).
    pub kind: &'static str,
    /// Turn number of the matching row.
    pub turn: u64,
    /// The matched row's textual payload (content/delta/output/message).
    pub text: String,
}

fn event_searchable_text(body: &SessionEventBody) -> Option<String> {
    match body {
        SessionEventBody::UserMessage(p) => Some(p.content.clone()),
        SessionEventBody::AssistantChunk(p) => Some(p.delta.clone()),
        SessionEventBody::AssistantMessage(p) => Some(p.content.clone()),
        SessionEventBody::ToolCall(p) => Some(p.arguments.clone()),
        SessionEventBody::ToolResult(p) => Some(p.output.clone()),
        SessionEventBody::Error(p) => Some(p.message.clone()),
        _ => None,
    }
}

/// Case-insensitive substring search across the textual kinds of the log —
/// the replacement for the deleted transcript store's search surface.
pub fn search_events(events: &[SessionEvent], pattern: &str) -> Vec<SearchHit> {
    let needle = pattern.to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for event in events {
        if let Some(text) = event_searchable_text(&event.body) {
            if text.to_lowercase().contains(&needle) {
                hits.push(SearchHit {
                    seq: event.seq,
                    kind: event.kind().as_str(),
                    turn: event.turn,
                    text,
                });
            }
        }
    }
    hits
}

/// Collect the permission decisions recorded in a session, in order.
pub fn project_permission_decisions(events: &[SessionEvent]) -> Vec<PermissionDecisionPayload> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            SessionEventBody::PermissionDecision(p) => Some(p.clone()),
            _ => None,
        })
        .collect()
}

/// Resolve the events up to a message boundary for branching.
///
/// Returns the max last-seq covered by the first `message_index` projected
/// messages (every parent event with `seq <= cutoff` seeds the branch), or
/// `None` when there is nothing to copy for index 0.
pub fn cutoff_seq_for_message_index(proj: &ConversationProjection, message_index: usize) -> u64 {
    proj.message_origin_seqs
        .iter()
        .take(message_index)
        .map(|(_, end)| *end)
        .max()
        .unwrap_or(0)
}

// ============================================================================
// Analytics projection (derived aggregate view)
// ============================================================================

/// Per-tool execution aggregates (dimension of the legacy analytics view).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToolAggregate {
    /// Tool invocations seen.
    pub calls: u64,
    /// Invocations whose `tool/result` was not an error.
    pub successes: u64,
    /// Invocations whose `tool/result` was an error.
    pub failures: u64,
    /// Sum of measured durations.
    pub total_duration_ms: u64,
}

/// Aggregate analytics view derived from one session's events. Preserves the
/// eight legacy dimensions: tool executions, prompts submitted, responses
/// received, file operations, session start/end markers, errors, and
/// permission requests.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionAnalytics {
    /// Owning session id.
    pub session_id: String,
    /// The model from `session/start`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// ISO date (UTC) of the session's first event.
    pub date: String,
    /// True once a `session/start` row was seen.
    pub session_started: bool,
    /// Closed turns (a `turn/end` carrying a terminal reason).
    pub turns_completed: u64,
    /// `user/message` count (prompts submitted dimension).
    pub prompts_submitted: u64,
    /// User-visible prompt characters (stands in for legacy prompt tokens,
    /// which the telemetry side never actually counted).
    pub prompt_chars: u64,
    /// Finalized assistant messages (responses received dimension).
    pub responses_received: u64,
    /// Response output tokens summed from `turn/end` usage.
    pub response_output_tokens: u64,
    /// File-operation counts keyed by operation kind
    /// (read/write/edit/search/delete).
    pub file_operations: std::collections::BTreeMap<String, u64>,
    /// Per-tool execution stats.
    pub tools: std::collections::BTreeMap<String, ToolAggregate>,
    /// Errors grouped by category.
    pub errors: std::collections::BTreeMap<String, u64>,
    /// Permission request totals.
    pub permission_requests_total: u64,
    /// Permission requests that were allowed.
    pub permission_requests_approved: u64,
}

impl SessionAnalytics {
    /// Record one tool call for `tool_name`.
    fn record_tool_call(&mut self, tool_name: &str) {
        self.tools.entry(tool_name.to_string()).or_default().calls += 1;
    }

    /// Record one tool result, pairing duration into the same aggregate.
    fn record_tool_result(&mut self, p: &ToolResultPayload) {
        let agg = self.tools.entry(p.tool_name.clone()).or_default();
        if p.is_error {
            agg.failures += 1;
        } else {
            agg.successes += 1;
        }
        agg.total_duration_ms += p.duration_ms.unwrap_or(0);
        if let Some(op) = file_operation_kind(&p.tool_name, &p.output, p.is_error) {
            *self.file_operations.entry(op.to_string()).or_insert(0) += 1;
        }
    }
}

/// Map file-manipulating tools to the legacy FileOperation dimension.
fn file_operation_kind(tool_name: &str, output: &str, is_error: bool) -> Option<&'static str> {
    if is_error {
        return None;
    }
    let lower = output.to_lowercase();
    let denied = lower.contains("denied") || lower.contains("permission");
    match tool_name {
        "Read" | "ReadFile" => (!denied).then_some("read"),
        "Write" | "WriteFile" => (!denied).then_some("write"),
        "Edit" | "MultiEdit" => (!denied).then_some("edit"),
        "NotebookEdit" => (!denied).then_some("edit"),
        "Glob" => (!denied).then_some("search"),
        "Grep" => (!denied).then_some("search"),
        _ => None,
    }
}

/// ns-since-epoch → `YYYY-MM-DD` (UTC) or `"unknown"` when out of range.
fn iso_date(ts_ns: u64) -> String {
    chrono::Utc
        .timestamp_opt(ts_ns as i64 / 1_000_000_000, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Derive the analytics aggregate view for one session.
pub fn project_session_analytics(events: &[SessionEvent]) -> SessionAnalytics {
    let mut view = SessionAnalytics {
        session_id: events
            .first()
            .map(|e| e.session_id.clone())
            .unwrap_or_default(),
        model: None,
        date: events
            .first()
            .map(|e| iso_date(e.ts_ns))
            .unwrap_or_default(),
        ..Default::default()
    };
    for event in events {
        match &event.body {
            SessionEventBody::SessionStart(p) => {
                view.session_started = true;
                view.model.get_or_insert_with(|| p.model.clone());
            }
            SessionEventBody::TurnEnd(TurnEndPayload {
                reason,
                usage,
                error,
            }) => {
                if reason == TurnEndPayload::REASON_COMPLETED {
                    view.turns_completed += 1;
                }
                if error.is_some() {
                    *view.errors.entry(format!("turn/{reason}")).or_insert(0) += 1;
                }
                if let Some(usage) = usage {
                    view.response_output_tokens += usage.output_tokens;
                }
            }
            SessionEventBody::UserMessage(p) => {
                view.prompts_submitted += 1;
                view.prompt_chars += p.content.chars().count() as u64;
            }
            SessionEventBody::AssistantMessage(_) => {
                view.responses_received += 1;
            }
            SessionEventBody::ToolCall(p) => view.record_tool_call(&p.tool_name),
            SessionEventBody::ToolResult(p) => view.record_tool_result(p),
            SessionEventBody::Error(p) => {
                *view.errors.entry(p.category.clone()).or_insert(0) += 1;
            }
            SessionEventBody::PermissionDecision(PermissionDecisionPayload {
                decision, ..
            }) => {
                view.permission_requests_total += 1;
                if decision == "allow" {
                    view.permission_requests_approved += 1;
                }
            }
            _ => {}
        }
    }
    view
}

/// Render one session's analytics view as a JSONL line (no trailing
/// newline) — the serialized product of the L0→analytics projection.
pub fn project_analytics_jsonl(events: &[SessionEvent]) -> String {
    let mut line = serde_json::to_string(&project_session_analytics(events))
        .expect("analytics projection serializes");
    line.push('\n');
    line
}

// ============================================================================
// Directory scan (session listing over L0)
// ============================================================================

/// A scanned session directory entry: enough metadata to list sessions
/// without projecting the whole conversation.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionScanEntry {
    /// Session id (= directory name).
    pub session_id: String,
    /// Path of the scanned `events.jsonl`.
    pub events_path: PathBuf,
    /// Path of the optional sidecar meta file next to it.
    pub meta_path: PathBuf,
}

/// Scan `<sessions_container>/<uuid>/events.jsonl` entries, newest-first by
/// events-file mtime. Non-directories and directories without a log are
/// ignored — `*.toml` name snapshots sharing the container stay untouched.
pub fn scan_session_summaries(sessions_container: &Path) -> Vec<SessionScanEntry> {
    let Ok(entries) = std::fs::read_dir(sessions_container) else {
        return Vec::new();
    };
    let mut found: Vec<(SessionScanEntry, std::time::SystemTime)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let dir_path = e.path();
            let events_path = dir_path.join("events.jsonl");
            if !events_path.is_file() {
                return None;
            }
            let session_id = dir_path.file_name()?.to_string_lossy().to_string();
            let modified = e.metadata().ok().and_then(|m| m.modified().ok());
            Some((
                SessionScanEntry {
                    session_id,
                    events_path: events_path.clone(),
                    meta_path: dir_path.join("meta.json"),
                },
                modified.unwrap_or(std::time::UNIX_EPOCH),
            ))
        })
        .collect();
    found.sort_by(|a, b| b.1.cmp(&a.1));
    found.into_iter().map(|(entry, _)| entry).collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_types::session_event::{
        AssistantChunkPayload, TokenUsage, ToolCallPayload, TurnStartPayload, UserMessagePayload,
    };

    fn ev(seq: u64, ts: u64, body: SessionEventBody) -> SessionEvent {
        SessionEvent::new(seq, ts, "sess-proj", 1, body)
    }

    fn user(seq: u64, content: &str) -> SessionEvent {
        ev(
            seq,
            100 + seq,
            SessionEventBody::UserMessage(UserMessagePayload {
                source: UserMessagePayload::SOURCE_USER.into(),
                content: content.into(),
            }),
        )
    }

    fn chunk(seq: u64, delta: &str) -> SessionEvent {
        ev(
            seq,
            100 + seq,
            SessionEventBody::AssistantChunk(AssistantChunkPayload {
                delta: delta.into(),
                thinking: false,
            }),
        )
    }

    fn call(seq: u64, id: &str, args: &str) -> SessionEvent {
        ev(
            seq,
            100 + seq,
            SessionEventBody::ToolCall(ToolCallPayload {
                tool_use_id: id.into(),
                tool_name: "Bash".into(),
                arguments: args.into(),
            }),
        )
    }

    fn result(seq: u64, id: &str, output: &str, is_error: bool) -> SessionEvent {
        ev(
            seq,
            100 + seq,
            SessionEventBody::ToolResult(ToolResultPayload {
                tool_use_id: id.into(),
                tool_name: "Bash".into(),
                output: output.into(),
                is_error,
                duration_ms: Some(5),
                meta: serde_json::Value::Null,
            }),
        )
    }

    fn turn_start(seq: u64) -> SessionEvent {
        ev(
            seq,
            100 + seq,
            SessionEventBody::TurnStart(TurnStartPayload { query_id: None }),
        )
    }

    fn turn_end(seq: u64, usage: Option<TokenUsage>) -> SessionEvent {
        ev(
            seq,
            100 + seq,
            SessionEventBody::TurnEnd(TurnEndPayload {
                reason: TurnEndPayload::REASON_COMPLETED.into(),
                usage,
                error: None,
            }),
        )
    }

    // -- Turn Timeline projection (§4.14 desktop panel) -----------------------

    fn tev(seq: u64, turn: u64, body: SessionEventBody) -> SessionEvent {
        SessionEvent::new(seq, 1_000 + seq * 10, "sess-proj", turn, body)
    }

    fn tcall(seq: u64, turn: u64, id: &str, tool: &str) -> SessionEvent {
        tev(
            seq,
            turn,
            SessionEventBody::ToolCall(ToolCallPayload {
                tool_use_id: id.into(),
                tool_name: tool.into(),
                arguments: "{}".into(),
            }),
        )
    }

    fn tresult(seq: u64, turn: u64, id: &str, is_error: bool) -> SessionEvent {
        tev(
            seq,
            turn,
            SessionEventBody::ToolResult(ToolResultPayload {
                tool_use_id: id.into(),
                tool_name: "X".into(),
                output: "o".into(),
                is_error,
                duration_ms: Some(33),
                meta: serde_json::Value::Null,
            }),
        )
    }

    fn tcurve_end(seq: u64, turn: u64, out: u64, cost: Option<f64>) -> SessionEvent {
        tev(
            seq,
            turn,
            SessionEventBody::TurnEnd(TurnEndPayload {
                reason: TurnEndPayload::REASON_COMPLETED.into(),
                usage: Some(TokenUsage {
                    input_tokens: 5,
                    output_tokens: out,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    cost_usd: cost,
                }),
                error: None,
            }),
        )
    }

    #[test]
    fn test_project_turn_timeline_rows_tools_and_curve() {
        let events = vec![
            tev(
                0,
                0,
                SessionEventBody::SessionStart(shannon_types::session_event::SessionStartPayload {
                    model: "claude-sonnet-4".into(),
                    provider: Some("anthropic".into()),
                    cwd: None,
                    app_version: None,
                }),
            ),
            user(0, "hi"),
            call(1, "ta", "{}"),
            tresult(2, 1, "ta", false),
            tcurve_end(3, 1, 100, Some(0.10)),
            user(5, "again"),
            tcall(4, 2, "tb", "Read"),
            tresult(5, 2, "tb", true),
            tcall(6, 2, "tc", "Grep"), // never answered
            tcurve_end(7, 2, 40, None),
        ];

        let tl = project_turn_timeline(&events);
        assert_eq!(tl.session_id, "sess-proj");
        assert_eq!(tl.model.as_deref(), Some("claude-sonnet-4"));
        assert_eq!(tl.turns.len(), 2);

        let (t1, t2) = (&tl.turns[0], &tl.turns[1]);
        assert_eq!(t1.tools.len(), 1);
        assert_eq!(t1.tools[0].tool_name, "Bash");
        assert!(!t1.tools[0].is_error);
        assert_eq!(t1.output_tokens, 100);
        assert_eq!(t1.cost_usd, Some(0.10));
        assert_eq!(
            t1.tools[0].duration_ms,
            Some(33),
            "measured duration wins over delta"
        );

        assert_eq!(t2.tools.len(), 2, "interrupted call still renders");
        assert!(t2.tools.iter().any(|t| t.tool_name == "Read" && t.is_error));
        let grep = t2
            .tools
            .iter()
            .find(|t| t.tool_use_id == "tc")
            .expect("grep row kept");
        assert!(grep.is_error, "unpaired call marks error");

        // Curve samples once per closed turn with running totals.
        assert_eq!(tl.cumulative.len(), 2);
        assert_eq!(tl.cumulative[0].output_tokens_total, 100);
        assert_eq!(tl.cumulative[1].output_tokens_total, 140);
        assert_eq!(tl.cumulative[0].cost_total_usd, Some(0.10));
        // A None cost in a later turn must not erase the running total.
        assert_eq!(tl.cumulative[1].cost_total_usd, Some(0.10));
    }

    #[test]
    fn test_project_turn_timeline_empty_and_preamble_only() {
        assert_eq!(
            project_turn_timeline(&[]).turns.len(),
            0,
            "empty log => empty timeline"
        );
        // Only a session/start preamble: no phantom turn rows.
        let start = tev(
            0,
            0,
            SessionEventBody::SessionStart(shannon_types::session_event::SessionStartPayload {
                model: "m".into(),
                provider: None,
                cwd: None,
                app_version: None,
            }),
        );
        let tl = project_turn_timeline(&[start]);
        assert_eq!(tl.turns.len(), 0);
        assert_eq!(tl.model.as_deref(), Some("m"));
        assert!(tl.cumulative.is_empty());
    }

    #[test]
    fn test_plain_text_turn_reconstruction() {
        let events = vec![
            turn_start(0),
            user(1, "hello"),
            chunk(2, "Hi"),
            chunk(3, " there!"),
            turn_end(
                4,
                Some(TokenUsage {
                    input_tokens: 10,
                    output_tokens: 3,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    cost_usd: Some(0.01),
                }),
            ),
        ];
        let proj = project_conversation(&events);
        assert_eq!(proj.messages.len(), 2);
        assert_eq!(proj.turn_count, 1);
        assert_eq!(proj.total_input_tokens, 10);
        assert_eq!(proj.total_output_tokens, 3);
        assert_eq!(proj.total_cost_usd, Some(0.01));

        let rendered: Vec<serde_json::Value> = proj
            .messages
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .collect();
        assert_eq!(rendered[0]["role"], "user");
        assert_eq!(rendered[0]["content"], "hello");
        assert_eq!(rendered[1]["role"], "assistant");
        assert_eq!(rendered[1]["content"][0]["type"], "text");
        assert_eq!(rendered[1]["content"][0]["text"], "Hi there!");
        // Origin spans cover their events.
        assert_eq!(proj.message_origin_seqs[0], (1, 1));
        assert_eq!(proj.message_origin_seqs[1], (2, 3));
    }

    #[test]
    fn test_tool_round_trip_matches_live_push_order() {
        // One turn: prompt → assistant(text+tool_use) → user(tool_result)
        // → assistant(final text). Same ordering the live engine pushes.
        let events = vec![
            turn_start(0),
            user(1, "run ls"),
            chunk(2, "Let me "),
            chunk(3, "check."),
            call(4, "toolu_1", r#"{"command":"ls"}"#),
            result(5, "toolu_1", "file-a\nfile-b", false),
            chunk(6, "Done."),
            turn_end(
                7,
                Some(TokenUsage {
                    input_tokens: 20,
                    output_tokens: 9,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    cost_usd: Some(0.02),
                }),
            ),
        ];
        let proj = project_conversation(&events);
        let rendered: Vec<serde_json::Value> = proj
            .messages
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .collect();

        assert_eq!(proj.messages.len(), 4);
        // Step message carries both the text and the tool_use block.
        assert_eq!(rendered[1]["content"][0]["type"], "text");
        assert_eq!(rendered[1]["content"][0]["text"], "Let me check.");
        assert_eq!(rendered[1]["content"][1]["type"], "tool_use");
        assert_eq!(rendered[1]["content"][1]["id"], "toolu_1");
        assert_eq!(rendered[1]["content"][1]["input"]["command"], "ls");
        // Each result is its own user message with one tool_result block.
        assert_eq!(rendered[2]["role"], "user");
        assert_eq!(rendered[2]["content"][0]["type"], "tool_result");
        assert_eq!(rendered[2]["content"][0]["content"], "file-a\nfile-b");
        assert_eq!(rendered[2]["content"][0]["is_error"], false);
        assert_eq!(rendered[3]["content"][0]["text"], "Done.");
        assert_eq!(proj.tool_call_count, 1);
        assert_eq!(proj.tool_error_count, 0);
    }

    #[test]
    fn test_thinking_chunks_and_broken_args_do_not_break_projection() {
        let thinking = ev(
            2,
            102,
            SessionEventBody::AssistantChunk(AssistantChunkPayload {
                delta: "hmm".into(),
                thinking: true,
            }),
        );
        let bad_call = call(3, "toolu_bad", "{not json");
        let err_result = result(4, "toolu_bad", "boom", true);
        let proj = project_conversation(&[user(0, "x"), thinking, bad_call, err_result]);
        assert_eq!(proj.messages.len(), 3);
        assert_eq!(proj.tool_error_count, 1);
        let value = serde_json::to_value(&proj.messages[1]).unwrap();
        assert_eq!(value["content"][0]["input"], serde_json::Value::Null);
        assert_eq!(value["content"].as_array().unwrap().len(), 1);
        // Thinking chunk contributed nothing.
        assert!(serde_json::to_string(&value).unwrap().find("hmm").is_none());
    }

    #[test]
    fn test_assistant_message_finalize_replaces_partial_stream() {
        let partial_then_final = [
            chunk(1, "par"),
            ev(
                2,
                102,
                SessionEventBody::AssistantMessage(
                    shannon_types::session_event::AssistantMessagePayload {
                        content: "authoritative full text".into(),
                        usage: None,
                        interrupted: true,
                    },
                ),
            ),
        ];
        let proj = project_conversation(&[
            user(0, "q"),
            partial_then_final[0].clone(),
            partial_then_final[1].clone(),
        ]);
        assert_eq!(proj.messages.len(), 2);
        let value = serde_json::to_value(&proj.messages[1]).unwrap();
        assert_eq!(value["content"][0]["text"], "authoritative full text");
    }

    #[test]
    fn test_cutoff_seq_for_branching() {
        let events = vec![
            user(0, "one"),
            chunk(1, "reply-one"),
            user(2, "two"),
            chunk(3, "reply-two-longer"),
        ];
        let proj = project_conversation(&events);
        assert_eq!(cutoff_seq_for_message_index(&proj, 0), 0);
        assert_eq!(cutoff_seq_for_message_index(&proj, 2), 1);
        assert_eq!(cutoff_seq_for_message_index(&proj, 4), 3);
        // Beyond the end clamps at the last covered seq.
        assert_eq!(cutoff_seq_for_message_index(&proj, 99), 3);
    }

    #[test]
    fn test_search_events_finds_and_reports_kind() {
        let events = vec![
            user(0, "Find the needle please"),
            chunk(1, "sewing"),
            result(2, "t", "needle found in haystack", false),
            ev(
                3,
                103,
                SessionEventBody::Error(shannon_types::session_event::ErrorPayload {
                    category: "query-failed".into(),
                    message: "lost needle".into(),
                    detail: None,
                }),
            ),
        ];
        let hits = search_events(&events, "NEEDLE");
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].seq, 0);
        assert_eq!(hits[0].kind, "user/message");
        assert_eq!(hits[1].kind, "tool/result");
        assert_eq!(hits[2].kind, "error");
        assert!(search_events(&events, "").is_empty());
        assert!(search_events(&events, "absent-token").is_empty());
    }

    #[test]
    fn test_search_hit_kind_accessor_stability() {
        let hit = SearchHit {
            seq: 1,
            kind: "tool/call",
            turn: 2,
            text: "args".into(),
        };
        assert_eq!(hit.seq, 1);
        assert_eq!(hit.turn, 2);
    }

    #[test]
    fn test_project_permission_decisions() {
        let allow = ev(
            0,
            100,
            SessionEventBody::PermissionDecision(PermissionDecisionPayload {
                tool_name: Some("Bash".into()),
                request: Some("ls".into()),
                decision: "allow".into(),
                reason: Some("safe".into()),
                mode: Some("auto".into()),
            }),
        );
        let deny = ev(
            1,
            101,
            SessionEventBody::PermissionDecision(PermissionDecisionPayload {
                tool_name: Some("Bash".into()),
                request: Some("rm -rf /".into()),
                decision: "deny".into(),
                reason: Some("destructive".into()),
                mode: Some("default".into()),
            }),
        );
        let decisions = project_permission_decisions(&[allow, deny, user(2, "continue")]);
        assert_eq!(decisions.len(), 2);
        assert_eq!(decisions[1].decision, "deny");
    }

    #[test]
    fn test_analytics_projection_preserves_legacy_dimensions() {
        let perm_allow = ev(
            7,
            107,
            SessionEventBody::PermissionDecision(PermissionDecisionPayload {
                tool_name: Some("Read".into()),
                request: None,
                decision: "allow".into(),
                reason: None,
                mode: Some("auto".into()),
            }),
        );
        let start = ev(
            0,
            1_756_200_000_000_000_000,
            SessionEventBody::SessionStart(shannon_types::session_event::SessionStartPayload {
                model: "m1".into(),
                provider: None,
                cwd: None,
                app_version: None,
            }),
        );
        let read_call = ev(
            2,
            102,
            SessionEventBody::ToolCall(ToolCallPayload {
                tool_use_id: "r1".into(),
                tool_name: "Read".into(),
                arguments: r#"{"path":"/tmp/x.rs"}"#.into(),
            }),
        );
        let read_result = ev(
            4,
            104,
            SessionEventBody::ToolResult(ToolResultPayload {
                tool_use_id: "r1".into(),
                tool_name: "Read".into(),
                output: "/tmp/x.rs contents".into(),
                is_error: false,
                duration_ms: Some(1),
                meta: serde_json::Value::Null,
            }),
        );
        let events = vec![
            start,
            user(1, "do things"),
            read_call,
            call(3, "t2", "{}"),
            read_result,
            result(5, "t2", "permission denied by rule", true),
            chunk(6, "answer"),
            perm_allow,
            turn_end(
                8,
                Some(TokenUsage {
                    input_tokens: 50,
                    output_tokens: 7,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    cost_usd: None,
                }),
            ),
            ev(
                9,
                109,
                SessionEventBody::Error(shannon_types::session_event::ErrorPayload {
                    category: "rate_limit".into(),
                    message: "429".into(),
                    detail: None,
                }),
            ),
        ];
        let view = project_session_analytics(&events);
        assert!(view.session_started);
        assert_eq!(view.model.as_deref(), Some("m1"));
        assert_eq!(view.prompts_submitted, 1);
        assert_eq!(view.responses_received, 0); // no finalized assistant rows
        assert_eq!(view.response_output_tokens, 7);
        let bash = view.tools.get("Bash").expect("bash aggregate");
        assert_eq!((bash.calls, bash.successes, bash.failures), (1, 0, 1));
        let read = view.tools.get("Read").expect("read aggregate");
        assert_eq!((read.calls, read.successes), (1, 1));
        assert_eq!(view.file_operations.get("read"), Some(&1));
        assert!(view.file_operations.get("write").is_none());
        assert_eq!(view.permission_requests_total, 1);
        assert_eq!(view.permission_requests_approved, 1);
        assert_eq!(view.errors.get("rate_limit"), Some(&1));
        let line = project_analytics_jsonl(&events);
        let parsed: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert!(
            parsed["date"].as_str().is_some_and(|d| d.len() == 10),
            "date is an ISO date string"
        );
        assert_eq!(parsed["tools"]["Bash"]["calls"], 1);
        assert_eq!(parsed["tools"]["Read"]["successes"], 1);
    }

    #[test]
    fn test_scan_skips_non_log_entries_and_sorts_newest_first() {
        let dir = tempfile::TempDir::new().unwrap();
        let base = dir.path().join("sessions");
        for name in ["b-second", "a-first", "not-a-session"] {
            std::fs::create_dir_all(base.join(name)).unwrap();
            if name != "not-a-session" {
                std::fs::write(base.join(name).join("events.jsonl"), "").unwrap();
            }
        }
        std::fs::write(base.join("name.toml"), "title = 'x'").unwrap();
        std::fs::write(base.join("stale.json"), "{}").unwrap();

        let older = base.join("a-first");
        let newer = base.join("b-second");
        // Give a-first an older mtime than b-second regardless of FS timing.
        let now = std::time::SystemTime::now();
        let earlier = now - std::time::Duration::from_secs(60);
        set_mtime(&older, earlier);
        set_mtime(&newer, now);

        let scans = scan_session_summaries(&base);
        let ids: Vec<&str> = scans.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, vec!["b-second", "a-first"]);
        assert_eq!(scans[0].meta_path, newer.join("meta.json"));
    }

    fn set_mtime(path: &Path, t: std::time::SystemTime) {
        let f = std::fs::File::open(path).unwrap();
        f.set_modified(t).expect("set mtime");
    }
}
