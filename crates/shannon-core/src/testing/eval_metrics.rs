//! Per-task metrics extraction and failure classification (§4.7 W2-M2).
//!
//! Upgrades the eval report from pass/fail to three dimensions — cost,
//! trajectory quality, and failure type. Two components live here:
//!
//! 1. **Metrics extractor** — consumes one or more L0 `events.jsonl` logs
//!    ([`extract_from_events_log`]) and folds them into a fully-populated
//!    [`TaskMetrics`] record: token triple, cost, turns, tool-call count,
//!    wall clock, strict tool loops, verbatim invalid retries, and permission
//!    denials. Dry-run rehearsals never spawn the engine, so they use
//!    [`derive_from_stream`]: the same counter logic over the captured
//!    NDJSON stream with reduced fidelity (no L0 kinds exist there), reported
//!    honestly via [`MetricSource::DerivedStream`] with cost/cache left at
//!    their unknown defaults. No synthetic `events.jsonl` is ever written to
//!    stand in for a real log.
//! 2. **Failure classifier** — seven failure classes decided by an external
//!    TOML rule table ([`FailureRules::embedded`], overridable by path): no
//!    model or provider names are hardcoded anywhere; classes are pure
//!    event-shape predicates.
//!
//! ## Metric provenance (field → L0 source)
//!
//! | field                  | source in `events.jsonl`                                |
//! |------------------------|---------------------------------------------------------|
//! | `tokens_in/out`, cache | sum over `turn/end.usage.{input,output,…}`              |
//! | `cost_usd`             | sum over `turn/end.usage.cost_usd` (`null` if unobserved) |
//! | `turns`                | highest envelope `turn` on any event; the runner then   |
//! |                        | reconciles upward with the stream's `done.turns_used`   |
//! |                        | (L0 turns are one per query by design, so the envelope  |
//! |                        | alone reads 1 for any single headless run)              |
//! | `tool_calls`           | count of `tool/call`                                    |
//! | `wall_clock_ms`        | last − first event `ts_ns`                              |
//! | `loops`                | ≥3 consecutive identical `(tool, args-hash)` invocations |
//! | `invalid_calls`        | errored call retried verbatim immediately afterwards    |
//! | `permission_blocks`    | `permission/decision` events with a denying decision     |
//!
//! Usage is read from `turn/end` only: the §4.2 tee folds each step's usage
//! into exactly one closing `turn/end`, so also summing
//! `assistant/message.usage` would double-count.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::session_log::SessionLogReader;
use crate::testing::eval_runner::EvalError;
use shannon_types::session_event::{
    SessionEvent, SessionEventBody, SurfaceReplacePayload, TokenUsage, ToolResultPayload,
    TurnEndPayload, TurnStartPayload,
};

// ── Report types ───────────────────────────────────────────────────────

/// Where a task's metrics came from. Report-visible so derived numbers can
/// never be mistaken for genuine L0 observations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricSource {
    /// Extracted from the real `events.jsonl` written by the engine tee.
    #[default]
    #[serde(rename = "events_jsonl")]
    EventsLog,
    /// Folded from the runner's NDJSON capture because dry-run stubs bypass
    /// the engine entirely; reduced fidelity, clearly labeled.
    #[serde(rename = "derived_stream")]
    DerivedStream,
}

impl MetricSource {
    /// Stable wire spelling used in reports and digests.
    pub fn as_str(self) -> &'static str {
        match self {
            MetricSource::EventsLog => "events_jsonl",
            MetricSource::DerivedStream => "derived_stream",
        }
    }
}

/// One detected loop run: evidence kept beside the numbers for the archived
/// postmortem bundle and the model-ceiling verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopDetail {
    /// Tool name repeated.
    pub tool: String,
    /// Call signature shared by every attempt in the streak.
    pub args_hash: String,
    /// Consecutive identical invocations observed (≥3 by definition).
    pub streak: u32,
}

/// Extractor outputs beyond the numeric cost matrix: classifier inputs that
/// have no place in a spreadsheet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FailureSignals {
    /// Distinct call signatures whose final attempt still errored.
    pub tool_error_unrecovered: u32,
    /// Tool errors raised by Edit/MultiEdit/Write specifically.
    pub edit_tool_errors: u32,
    /// Total errored `tool/result` events observed.
    pub tool_errors: u32,
    /// Categories seen on `error` events (e.g. `rate_limit`, `query-failed`).
    pub error_categories: Vec<String>,
    /// Any `turn/end` closed as interrupted.
    pub turn_interrupted: bool,
    /// Count of `surface/replace` events with reason `compaction`.
    pub compaction_events: u32,
    /// Human-readable loop evidence for archives ("Read ×4 <hash>").
    pub loop_notes: Vec<String>,
}

impl FailureSignals {
    /// Flatten into `name → textual value` pairs for TOML rule evaluation.
    /// Error categories become `error_<category-with-'-'-as-'_'>` booleans so
    /// rules like `signal = "error_rate_limit"` work without listing logic
    /// in Rust.
    pub fn signal_table(&self) -> BTreeMap<String, String> {
        let mut table = BTreeMap::new();
        table.insert(
            "tool_error_unrecovered".to_string(),
            self.tool_error_unrecovered.to_string(),
        );
        table.insert(
            "edit_tool_errors".to_string(),
            self.edit_tool_errors.to_string(),
        );
        table.insert("tool_errors".to_string(), self.tool_errors.to_string());
        table.insert(
            "turn_interrupted".to_string(),
            self.turn_interrupted.to_string(),
        );
        table.insert(
            "compaction_events".to_string(),
            self.compaction_events.to_string(),
        );
        for category in &self.error_categories {
            let key = format!("error_{}", category.replace('-', "_"));
            table.insert(key, "true".to_string());
        }
        table
    }
}

/// Fully-populated per-task metrics. Every field is always serialized (no
/// `skip_serializing_if`) — completeness means field *presence*; only
/// `cost_usd` / `wall_clock_ms` may legitimately hold `null`, where the
/// provider or stream mode genuinely observed nothing. A missing field is a
/// bug and is caught by [`missing_fields`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TaskMetrics {
    /// Sum of `turn/end.usage.input_tokens`.
    pub tokens_in: u64,
    /// Sum of `turn/end.usage.output_tokens`.
    pub tokens_out: u64,
    /// Sum of prompt-cache write tokens.
    pub cache_creation_tokens: u64,
    /// Sum of prompt-cache read tokens.
    pub cache_read_tokens: u64,
    /// USD cost summed from turn-end observations; `null` when the provider
    /// reported none (honest unknown — never fabricated).
    pub cost_usd: Option<f64>,
    /// Agent turns actually taken. L0 source: highest envelope `turn`,
    /// reconciled upward with the stream's `done.turns_used` (the engine
    /// opens one L0 vocabulary turn per query, so the envelope alone reads
    /// 1 regardless of LLM step count).
    pub turns: u32,
    /// Count of `tool/call` events (dry-run: trajectory length).
    pub tool_calls: u32,
    /// First-to-last event span when both endpoints were logged.
    pub wall_clock_ms: Option<u64>,
    /// Number of maximal runs of ≥3 identical consecutive calls.
    pub loops: u32,
    /// Longest identical consecutive-call streak observed.
    pub loop_max_streak: u32,
    /// Errored call followed immediately by the identical call again.
    pub invalid_calls: u32,
    /// Permission decisions that denied an action.
    pub permission_blocks: u32,
    /// Which feeder produced these numbers.
    pub source: MetricSource,
}

/// Serialized-field names contracted to appear on every report row.
pub const METRIC_FIELDS: [&str; 13] = [
    "tokens_in",
    "tokens_out",
    "cache_creation_tokens",
    "cache_read_tokens",
    "cost_usd",
    "turns",
    "tool_calls",
    "wall_clock_ms",
    "loops",
    "loop_max_streak",
    "invalid_calls",
    "permission_blocks",
    "source",
];

/// Validation standard ①: list metric fields absent from the serialized form;
/// an empty result proves zero-missing completeness.
pub fn missing_fields(metrics: &TaskMetrics) -> Vec<&'static str> {
    let Ok(value) = serde_json::to_value(metrics) else {
        return METRIC_FIELDS.to_vec();
    };
    METRIC_FIELDS
        .iter()
        .copied()
        .filter(|field| value.get(field).is_none())
        .collect()
}

// ── Metadata anchor (W1 §4①) ───────────────────────────────────────────

/// The tally-only bucket for failed rows no rule fired on (design §6 阶段 1).
/// Deliberately NOT a [`FAILURE_CLASSES`] member: the rule table cannot claim
/// it, so reports record the residue instead of pretending a class exists.
pub const UNCLASSIFIED_CLASS: &str = "unclassified";

/// Model id stamped into anchors for dry-run rehearsals (§4①): the stub never
/// talks to a provider, so the anchor is a constant marker, not a model name.
pub const DRY_RUN_ANCHOR_MODEL: &str = "dry-run-stub";

/// Metadata anchor: which model/provider/profile a run executed under.
/// `None` fields mean "honestly unknown" (e.g. logs written before the
/// request-header tee existed) and are never guessed from elsewhere.
///
/// This is the W1 diff-protocol input: `eval-diff` compares anchors before
/// any stability/regression verdict and refuses to issue one when they
/// disagree (ATTRIBUTE-SPLIT), so a model swap can never masquerade as a
/// code-change regression.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunAnchor {
    /// Model id as the provider saw it (`request/header` `wire_body.model`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// Provider id when the engine recorded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// sha256 (16 hex) over the request header's `config_snapshot` — the
    /// closest honest witness of the profile/config in force at run time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_digest: Option<String>,
}

impl RunAnchor {
    /// True when every dimension is unknown.
    pub fn is_unknown(&self) -> bool {
        self.model_id.is_none() && self.provider.is_none() && self.profile_digest.is_none()
    }
}

/// Extract a run anchor from one task's L0 events. The first
/// `request/header` wins — its `wire_body.model` is the wire product the
/// provider actually received, the most honest model observation — falling
/// back to the `session/start` banner for logs written without a header.
pub fn extract_anchor(events: &[SessionEvent]) -> RunAnchor {
    let mut anchor = RunAnchor::default();
    for event in events {
        match &event.body {
            SessionEventBody::SessionStart(start) => {
                if anchor.model_id.is_none() {
                    anchor.model_id = Some(start.model.clone());
                }
                if anchor.provider.is_none() {
                    anchor.provider = start.provider.clone();
                }
            }
            SessionEventBody::RequestHeader(header) => {
                anchor.model_id = Some(
                    header
                        .wire_body
                        .as_ref()
                        .and_then(|body| body.get("model"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| header.model.clone()),
                );
                anchor.provider = header.provider.clone();
                anchor.profile_digest = snapshot_fingerprint(&header.config_snapshot);
                // The opening header decides; later reason="change" re-headers
                // describe mid-run switches, not what the run started under.
                break;
            }
            _ => {}
        }
    }
    anchor
}

/// sha256 (16 hex) over a header config snapshot; null/empty snapshots stay
/// unknown rather than hashing into a constant that would masquerade as
/// provenance.
fn snapshot_fingerprint(snapshot: &serde_json::Value) -> Option<String> {
    match snapshot {
        serde_json::Value::Null => None,
        value => {
            let rendered = value.to_string();
            (rendered != "{}").then(|| {
                let digest = Sha256::digest(rendered.as_bytes());
                hex::encode(&digest[..8])
            })
        }
    }
}

// ── Signature hashing ──────────────────────────────────────────────────

/// Canonical argument encoding: parsed JSON re-serialized with recursively
/// sorted object keys so whitespace/key order cannot split one call shape
/// into two signatures. Unparseable arguments fall back to the trimmed raw
/// string (tee truncation markers included — still deterministic).
pub fn canonical_arguments(arguments: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(value) => canonicalize_value(&value),
        Err(_) => arguments.trim().to_string(),
    }
}

fn canonicalize_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<&str, &serde_json::Value> =
                map.iter().map(|(k, v)| (k.as_str(), v)).collect();
            let rebuilt: BTreeMap<&str, serde_json::Value> = sorted
                .into_iter()
                .map(|(k, v)| {
                    let canonical =
                        serde_json::from_str::<serde_json::Value>(&canonicalize_value(v))
                            .unwrap_or_else(|_| v.clone());
                    (k, canonical)
                })
                .collect();
            serde_json::to_string(&rebuilt).unwrap_or_else(|_| "{}".to_string())
        }
        other => other.to_string(),
    }
}

/// Stable identity of one invocation attempt: tool name plus 16 hex chars of
/// SHA-256 over the canonicalized arguments.
pub fn call_signature(tool: &str, arguments: &str) -> String {
    let digest = Sha256::digest(canonical_arguments(arguments).as_bytes());
    format!("{tool}:{}", hex::encode(&digest[..8]))
}

// ── Shared counting core ───────────────────────────────────────────────

/// One invocation attempt paired with its outcome (`None` = no result seen).
struct Attempt {
    signature: String,
    tool: String,
    errored: Option<bool>,
}

/// Outcome of folding an ordered attempt list: loop runs, longest streak,
/// human loop notes, verbatim-retry count, and last-attempt outcome per
/// signature (the unrecovered-error signal).
struct FoldOutcome {
    loops: u32,
    max_streak: u32,
    details: Vec<LoopDetail>,
    invalid_calls: u32,
    last_outcome: BTreeMap<String, bool>,
}

/// Fold ordered attempts into loop/retry metrics.
///
/// Definitions (plan-strict):
/// - **loop**: ≥3 *consecutive* attempts sharing one signature; any differing
///   intervening call breaks the run. Each maximal qualifying run counts once.
/// - **invalid_call**: an attempt whose immediately preceding attempt had the
///   same signature and errored — the 「错误后原样重试」 pattern. Retry chains
///   count each extra verbatim repeat; changing arguments between retries is
///   healthy recovery, not an invalid call.
fn fold_attempts(attempts: &[Attempt]) -> FoldOutcome {
    let mut loops = 0u32;
    let mut max_streak = 0u32;
    let mut details = Vec::new();
    let mut invalid_calls = 0u32;

    let mut index = 0usize;
    while index < attempts.len() {
        let signature = &attempts[index].signature;
        let mut run_end = index + 1;
        while run_end < attempts.len() && attempts[run_end].signature == *signature {
            run_end += 1;
        }
        let streak = (run_end - index) as u32;
        if streak > max_streak {
            max_streak = streak;
        }
        if streak >= 3 {
            loops += 1;
            details.push(LoopDetail {
                tool: attempts[index].tool.clone(),
                args_hash: signature.clone(),
                streak,
            });
        }
        for window in (index + 1)..run_end {
            let previous_errored = attempts[window - 1].errored == Some(true);
            if previous_errored {
                // Every attempt inside an identical-run after an error counts
                // as a verbatim retry of it.
                invalid_calls += 1;
            }
        }
        index = run_end;
    }

    let mut last_outcome: BTreeMap<String, bool> = BTreeMap::new();
    for attempt in attempts {
        if let Some(errored) = attempt.errored {
            last_outcome.insert(attempt.signature.clone(), errored);
        }
    }
    FoldOutcome {
        loops,
        max_streak,
        details,
        invalid_calls,
        last_outcome,
    }
}

// ── Extraction results ─────────────────────────────────────────────────

/// Metrics plus the classifier-facing signals extracted together.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtractedTask {
    /// Cost/trajectory metrics for the report row.
    pub metrics: TaskMetrics,
    /// Structural signals feeding the failure rules.
    pub signals: FailureSignals,
}

/// Locate `events.jsonl` files written by one real-mode child inside its
/// isolated `SHANNON_HOME` (sorted for determinism).
pub fn find_event_logs(l0_home: &Path) -> Vec<PathBuf> {
    let sessions = l0_home.join("sessions");
    let Ok(entries) = std::fs::read_dir(&sessions) else {
        return Vec::new();
    };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("events.jsonl"))
        .filter(|path| path.is_file())
        .collect();
    logs.sort();
    logs
}

/// The real extractor: fold one or more session logs (earliest first) into a
/// single task view. Multiple files occur when a child wrote several sessions
/// (engine restarts); they are concatenated in the given order.
pub fn extract_from_events_log(paths: &[PathBuf]) -> Result<ExtractedTask, EvalError> {
    let mut extractor = EventAccumulator::new(MetricSource::EventsLog);
    for path in paths {
        let reader = SessionLogReader::open(path)
            .map_err(|e| EvalError::Config(format!("cannot open {}: {e}", path.display())))?;
        let events = reader
            .read_events(false)
            .map_err(|e| EvalError::Config(format!("cannot read {}: {e}", path.display())))?;
        extractor.absorb_l0(&events);
    }
    Ok(extractor.finish())
}

/// Feed accumulator over typed L0 bodies.
struct EventAccumulator {
    output: ExtractedTask,
    attempts: Vec<Attempt>,
    /// `tool_use_id -> attempt index`, joining results to their calls.
    open_calls: BTreeMap<String, usize>,
    first_ts_ns: Option<u128>,
    last_ts_ns: Option<u128>,
}

impl EventAccumulator {
    fn new(source: MetricSource) -> Self {
        Self {
            output: ExtractedTask {
                metrics: TaskMetrics {
                    source,
                    ..TaskMetrics::default()
                },
                signals: FailureSignals::default(),
            },
            attempts: Vec::new(),
            open_calls: BTreeMap::new(),
            first_ts_ns: None,
            last_ts_ns: None,
        }
    }

    fn absorb_l0(&mut self, events: &[SessionEvent]) {
        for event in events {
            self.first_ts_ns.get_or_insert(event.ts_ns as u128);
            self.last_ts_ns = Some(event.ts_ns as u128);
            self.output.metrics.turns = self
                .output
                .metrics
                .turns
                .max(event.turn.min(u32::MAX as u64) as u32);

            match &event.body {
                SessionEventBody::TurnStart(TurnStartPayload { .. }) => {}
                SessionEventBody::TurnEnd(end) => self.absorb_turn_end(end),
                SessionEventBody::ToolCall(call) => {
                    self.output.metrics.tool_calls += 1;
                    let index = self.attempts.len();
                    self.open_calls.insert(call.tool_use_id.clone(), index);
                    self.attempts.push(Attempt {
                        signature: call_signature(&call.tool_name, &call.arguments),
                        tool: call.tool_name.clone(),
                        errored: None,
                    });
                }
                SessionEventBody::ToolResult(result) => self.absorb_result(result),
                SessionEventBody::PermissionDecision(decision) => {
                    if is_deny(&decision.decision) {
                        self.output.metrics.permission_blocks += 1;
                    }
                }
                SessionEventBody::SurfaceReplace(SurfaceReplacePayload { reason, .. }) => {
                    if reason == "compaction" {
                        self.output.signals.compaction_events += 1;
                    }
                }
                SessionEventBody::Error(error) => {
                    if !self
                        .output
                        .signals
                        .error_categories
                        .contains(&error.category)
                    {
                        self.output
                            .signals
                            .error_categories
                            .push(error.category.clone());
                    }
                }
                _ => {}
            }
        }
    }

    fn absorb_turn_end(&mut self, end: &TurnEndPayload) {
        if let Some(usage) = &end.usage {
            accumulate_usage(&mut self.output.metrics, usage);
        }
        if end.reason == TurnEndPayload::REASON_INTERRUPTED {
            self.output.signals.turn_interrupted = true;
        }
    }

    fn absorb_result(&mut self, result: &ToolResultPayload) {
        let Some(&index) = self.open_calls.get(&result.tool_use_id) else {
            return; // defensive: result without tracked call
        };
        self.open_calls.remove(&result.tool_use_id);
        let attempt = &mut self.attempts[index];
        if result.is_error {
            attempt.errored = Some(true);
            self.output.signals.tool_errors += 1;
            if matches!(attempt.tool.as_str(), "Edit" | "MultiEdit" | "Write") {
                self.output.signals.edit_tool_errors += 1;
            }
        } else {
            attempt.errored = Some(false);
        }
    }

    fn finish(mut self) -> ExtractedTask {
        let folded = fold_attempts(&self.attempts);
        self.output.metrics.loops = folded.loops;
        self.output.metrics.loop_max_streak = folded.max_streak;
        self.output.metrics.invalid_calls = folded.invalid_calls;
        self.output.signals.tool_error_unrecovered = folded
            .last_outcome
            .values()
            .filter(|failed| **failed)
            .count() as u32;
        self.output.signals.loop_notes = folded
            .details
            .iter()
            .map(|detail| format!("{} ×{} ({})", detail.tool, detail.streak, detail.args_hash))
            .collect();
        self.output.metrics.wall_clock_ms = match (self.first_ts_ns, self.last_ts_ns) {
            (Some(start), Some(end)) => Some(((end - start) / 1_000_000) as u64),
            _ => None,
        };
        self.output
    }
}

fn accumulate_usage(metrics: &mut TaskMetrics, usage: &TokenUsage) {
    metrics.tokens_in += usage.input_tokens;
    metrics.tokens_out += usage.output_tokens;
    metrics.cache_creation_tokens += usage.cache_creation_tokens;
    metrics.cache_read_tokens += usage.cache_read_tokens;
    metrics.cost_usd = match (metrics.cost_usd, usage.cost_usd) {
        (Some(a), Some(b)) => Some(a + b),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    };
}

/// True for any decision spelling that denies an action.
fn is_deny(decision: &str) -> bool {
    let normalized = decision.trim().to_ascii_lowercase();
    normalized == "deny" || normalized == "denied" || normalized.starts_with("deny")
}

// ── NDJSON derivation (dry-run honesty path) ───────────────────────────

/// Derive metrics from a captured engine-shape NDJSON stream. Used only where
/// no L0 log can exist (dry-run stubs never spawn the engine): cost and cache
/// stay at their unknown defaults, signals stay minimal-but-real where the
/// stream actually shows errors, and `source` stamps
/// [`MetricSource::DerivedStream`] so consumers cannot misread the origin.
pub fn derive_from_stream(ndjson: &str) -> ExtractedTask {
    use super::eval_runner::parse_ndjson_line;

    let mut output = ExtractedTask {
        metrics: TaskMetrics {
            source: MetricSource::DerivedStream,
            ..TaskMetrics::default()
        },
        signals: FailureSignals::default(),
    };
    let mut attempts: Vec<Attempt> = Vec::new();

    for line in ndjson.lines() {
        match parse_ndjson_line(line) {
            super::eval_runner::NdjsonLine::ToolCall { name, input_json } => {
                output.metrics.tool_calls += 1;
                attempts.push(Attempt {
                    signature: call_signature(&name, &input_json),
                    tool: name,
                    errored: None,
                });
            }
            super::eval_runner::NdjsonLine::ToolResult { success, name: _ } => {
                if success == Some(false) {
                    if let Some(last) = attempts
                        .iter_mut()
                        .rev()
                        .find(|attempt| attempt.errored.is_none())
                    {
                        last.errored = Some(true);
                        output.signals.tool_errors += 1;
                        if matches!(last.tool.as_str(), "Edit" | "MultiEdit" | "Write") {
                            output.signals.edit_tool_errors += 1;
                        }
                    }
                }
            }
            super::eval_runner::NdjsonLine::Done {
                turns_used,
                tokens_in,
                tokens_out,
                ..
            } => {
                output.metrics.turns = turns_used.unwrap_or(0);
                output.metrics.tokens_in = tokens_in.unwrap_or(0);
                output.metrics.tokens_out = tokens_out.unwrap_or(0);
            }
            _ => {}
        }
    }

    let folded = fold_attempts(&attempts);
    output.metrics.loops = folded.loops;
    output.metrics.loop_max_streak = folded.max_streak;
    output.metrics.invalid_calls = folded.invalid_calls;
    output.signals.tool_error_unrecovered = folded
        .last_outcome
        .values()
        .filter(|failed| **failed)
        .count() as u32;
    output.signals.loop_notes = folded
        .details
        .iter()
        .map(|detail| format!("{} ×{} ({})", detail.tool, detail.streak, detail.args_hash))
        .collect();
    output
}

// ── Failure classification (external TOML rule table) ──────────────────

/// The seven planned failure classes (§4.7).
pub const FAILURE_CLASSES: [&str; 7] = [
    "instruction_misunderstanding",
    "tool_failure_unrecovered",
    "context_loss",
    "permission_misreject",
    "timeout_or_limit",
    "edit_conflict",
    "model_ceiling",
];

/// The runner-side facts a rule may reference (plus every
/// [`FailureSignals::signal_table`] entry).
#[derive(Debug, Clone, Default)]
pub struct ClassifyContext {
    /// Final runner verdict string (`RunStatus::as_str`).
    pub status: String,
    /// Whether assertions all passed.
    pub passed: bool,
    /// Assertion/script violation count.
    pub violations: u32,
    /// The task's metrics (cost matrix values are also addressable signals).
    pub metrics: TaskMetrics,
    /// Structural signals extracted from the log/stream.
    pub signals: FailureSignals,
}

impl ClassifyContext {
    /// Everything visible to rules, as text.
    fn signal_table(&self) -> BTreeMap<String, String> {
        let mut table = self.signals.signal_table();
        let pairs: [(&str, String); 15] = [
            ("status", self.status.clone()),
            ("passed", self.passed.to_string()),
            ("violations", self.violations.to_string()),
            (
                "permission_blocks",
                self.metrics.permission_blocks.to_string(),
            ),
            ("loops", self.metrics.loops.to_string()),
            ("loop_max_streak", self.metrics.loop_max_streak.to_string()),
            ("invalid_calls", self.metrics.invalid_calls.to_string()),
            ("tool_calls", self.metrics.tool_calls.to_string()),
            ("turns", self.metrics.turns.to_string()),
            ("tokens_in", self.metrics.tokens_in.to_string()),
            ("tokens_out", self.metrics.tokens_out.to_string()),
            (
                "cache_creation_tokens",
                self.metrics.cache_creation_tokens.to_string(),
            ),
            (
                "cache_read_tokens",
                self.metrics.cache_read_tokens.to_string(),
            ),
            ("cost_observed", self.metrics.cost_usd.is_some().to_string()),
            (
                "duration_observed",
                self.metrics.wall_clock_ms.is_some().to_string(),
            ),
        ];
        for (key, value) in pairs {
            table.insert(key.to_string(), value);
        }
        table
    }
}

/// A satisfied-rule rendering with its evidence chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Classification {
    /// One of [`FAILURE_CLASSES`].
    pub class: String,
    /// Human description copied from the winning rule.
    pub rule_description: String,
    /// One entry per satisfied condition: `signal op value (actual=…)`.
    pub evidence: Vec<String>,
}

// ── TOML surface ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RulesFile {
    schema_version: u32,
    #[serde(default)]
    rule: Vec<RuleDefToml>,
}

#[derive(Debug, Clone, Deserialize)]
struct RuleDefToml {
    class: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    condition: Vec<ConditionToml>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConditionToml {
    signal: String,
    op: String,
    value: String,
}

/// Parsed, validated rule table ready for evaluation.
#[derive(Debug, Clone)]
pub struct FailureRules {
    rules: Vec<RuleDefToml>,
    /// sha256 fingerprint (first 16 hex chars) of the source TOML — included
    /// in reports so a run states exactly which rule version judged it.
    fingerprint: String,
}

impl FailureRules {
    /// Parse a rule file from disk.
    pub fn load(path: &Path) -> Result<Self, EvalError> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| EvalError::Config(format!("cannot read {}: {e}", path.display())))?;
        Self::from_toml(&raw).map_err(|e| EvalError::Config(format!("{}: {e}", path.display())))
    }

    /// The compiled-in default (`failure_rules.toml` beside this module).
    pub fn embedded() -> Self {
        Self::from_toml(include_str!("failure_rules.toml"))
            .expect("embedded failure_rules.toml must be valid")
    }

    /// sha256 fingerprint (16 hex chars) of the active rule table text.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    fn from_toml(raw: &str) -> Result<Self, String> {
        let file: RulesFile =
            toml::from_str(raw).map_err(|e| format!("rule table failed to parse: {e}"))?;
        if file.schema_version != 1 {
            return Err(format!(
                "unsupported schema_version {}",
                file.schema_version
            ));
        }
        for rule in &file.rule {
            if !FAILURE_CLASSES.contains(&rule.class.as_str()) {
                return Err(format!("unknown failure class '{}'", rule.class));
            }
            for condition in &rule.condition {
                match condition.op.as_str() {
                    "equals" | "ge" | "contains" => {}
                    other => {
                        return Err(format!("unknown op '{other}' in rule for '{}'", rule.class));
                    }
                }
            }
        }
        Ok(Self {
            fingerprint: {
                let digest = Sha256::digest(raw.as_bytes());
                hex::encode(&digest[..8])
            },
            rules: file.rule,
        })
    }

    /// Classify one finished (non-passing) task; passing tasks get no
    /// failure class. Returns `None` when no rule fires (unclassified).
    pub fn classify(&self, context: &ClassifyContext) -> Option<Classification> {
        if context.passed {
            return None;
        }
        let table = context.signal_table();
        let actual_of = |signal: &str| -> Option<String> { table.get(signal).cloned() };

        for rule in &self.rules {
            let mut evidence = Vec::new();
            let mut all_hold = true;
            for condition in &rule.condition {
                let Some(actual) = actual_of(&condition.signal) else {
                    all_hold = false;
                    break;
                };
                let holds = match condition.op.as_str() {
                    "equals" => actual == condition.value,
                    "ge" => actual
                        .parse::<f64>()
                        .ok()
                        .zip(condition.value.parse::<f64>().ok())
                        .is_some_and(|(a, b)| a >= b),
                    "contains" => actual.contains(condition.value.as_str()),
                    _ => false,
                };
                if holds {
                    evidence.push(format!(
                        "{} {} {} (actual={actual})",
                        condition.signal, condition.op, condition.value
                    ));
                } else {
                    all_hold = false;
                    break;
                }
            }
            if all_hold && !evidence.is_empty() {
                return Some(Classification {
                    class: rule.class.clone(),
                    rule_description: rule.description.clone(),
                    evidence,
                });
            }
        }
        None
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::session_log::SessionLogWriter;
    use shannon_types::session_event::{
        ErrorPayload, RequestHeaderPayload, SessionStartPayload, SurfaceReplacePayload,
        TodoSnapshotEntry, TodoWritePayload, ToolCallPayload, UserMessagePayload,
    };
    use tempfile::TempDir;

    use serde_json::Value;

    fn sig(tool: &str, args: &str) -> String {
        call_signature(tool, args)
    }

    fn attempt(tool: &str, args: &str, errored: Option<bool>) -> Attempt {
        Attempt {
            signature: sig(tool, args),
            tool: tool.to_string(),
            errored,
        }
    }

    #[test]
    fn canonical_arguments_normalizes_key_order_and_whitespace() {
        let a = canonical_arguments(r#"{"file_path":"x.rs","old_string":"fn a()"}"#);
        let b = canonical_arguments(r#"{"old_string":"fn a()", "file_path":"x.rs"}"#);
        assert_eq!(a, b, "key order must not change the signature");
        assert_eq!(canonical_arguments("  raw  "), "raw", "trim fallback");
    }

    #[test]
    fn call_signature_separates_tools_and_argument_drift() {
        assert_eq!(
            sig("Read", r#"{"file_path":"a"}"#),
            sig("Read", r#"{"file_path":"a"}"#)
        );
        assert_ne!(
            sig("Read", r#"{"file_path":"a"}"#),
            sig("Grep", r#"{"file_path":"a"}"#)
        );
        assert_ne!(
            sig("Edit", r#"{"old_string":"a"}"#),
            sig("Edit", r#"{"old_string":"b"}"#),
            "changed arguments mean a different call"
        );
    }

    #[test]
    fn fold_requires_three_consecutive_identical_calls() {
        // Two repeats: not a loop.
        let short = vec![
            attempt("Read", r#"{"p":1}"#, Some(false)),
            attempt("Read", r#"{"p":1}"#, Some(false)),
        ];
        let out = fold_attempts(&short);
        assert_eq!(out.loops, 0);
        assert_eq!(out.max_streak, 2);

        // Third consecutive repeat crosses the threshold.
        let long: Vec<_> = (0..3)
            .map(|_| attempt("Grep", r#"{"pattern":"todo"}"#, Some(true)))
            .collect();
        let out = fold_attempts(&long);
        assert_eq!(out.loops, 1);
        assert_eq!(out.max_streak, 3);
        assert_eq!(out.details.len(), 1);
        assert_eq!(out.details[0].tool, "Grep");
    }

    #[test]
    fn fold_breaks_loop_runs_on_differing_call() {
        let attempts = vec![
            attempt("Read", "a", Some(false)),
            attempt("Read", "a", Some(false)),
            attempt("Edit", "x", Some(false)), // interruption resets the run
            attempt("Read", "a", Some(false)),
            attempt("Read", "a", Some(false)),
        ];
        let out = fold_attempts(&attempts);
        assert_eq!(out.loops, 0, "streak never reaches 3");
        assert_eq!(out.max_streak, 2);
    }

    #[test]
    fn fold_counts_verbatim_retries_only_after_immediate_error() {
        // Errored call repeated identically twice: two invalid calls.
        let chain = vec![
            attempt("Edit", r#"{"o":"x"}"#, Some(true)),
            attempt("Edit", r#"{"o":"x"}"#, Some(true)),
            attempt("Edit", r#"{"o":"x"}"#, Some(false)),
        ];
        let out = fold_attempts(&chain);
        assert_eq!(out.invalid_calls, 2);

        // Recovered differently-changed arguments: healthy retry, not invalid,
        // though the abandoned x-anchor signature remains unrecovered on its
        // own terms.
        let healed = vec![
            attempt("Edit", r#"{"o":"x"}"#, Some(true)),
            attempt("Edit", r#"{"o":"y"}"#, Some(false)),
        ];
        let out = fold_attempts(&healed);
        assert_eq!(out.invalid_calls, 0);
        assert_eq!(out.last_outcome.values().filter(|f| **f).count(), 1);

        // Same-signature failure whose FINAL attempt still failed: unrecovered.
        let stuck = vec![
            attempt("Bash", "ls zzz", Some(true)),
            attempt("Bash", "ls zzz", Some(true)),
        ];
        let out = fold_attempts(&stuck);
        assert_eq!(out.invalid_calls, 1);
        assert_eq!(out.last_outcome.values().filter(|f| **f).count(), 1);
    }

    /// Seed a genuine L0 log through the real writer, then extract.
    fn write_log(dir: &Path, session: &str, bodies: Vec<SessionEventBody>) -> PathBuf {
        let mut writer = SessionLogWriter::open_in_dir(dir, session).unwrap();
        for body in bodies {
            writer.record(body);
        }
        let path = writer.path().to_path_buf();
        writer.close().unwrap();
        path
    }

    fn usage(input: u64, output: u64, cache_create: u64, cache_read: u64, cost: f64) -> TokenUsage {
        TokenUsage {
            input_tokens: input,
            output_tokens: output,
            cache_creation_tokens: cache_create,
            cache_read_tokens: cache_read,
            cost_usd: Some(cost),
        }
    }

    fn call(id: &str, tool: &str, args: &str) -> SessionEventBody {
        SessionEventBody::ToolCall(ToolCallPayload {
            tool_use_id: id.into(),
            tool_name: tool.into(),
            arguments: args.into(),
        })
    }

    fn result(id: &str, tool: &str, error: bool) -> SessionEventBody {
        SessionEventBody::ToolResult(ToolResultPayload {
            tool_use_id: id.into(),
            tool_name: tool.into(),
            output: if error { "boom".into() } else { "ok".into() },
            is_error: error,
            duration_ms: Some(3),
            meta: serde_json::Value::Null,
        })
    }

    #[test]
    fn extractor_sums_usage_and_counts_every_metric_kind() {
        let dir = TempDir::new().unwrap();
        let bodies = vec![
            SessionEventBody::SessionStart(shannon_types::session_event::SessionStartPayload {
                model: "test-model".into(),
                provider: Some("mock".into()),
                cwd: None,
                app_version: Some("0.0.0".into()),
            }),
            SessionEventBody::UserMessage(shannon_types::session_event::UserMessagePayload {
                source: "user".into(),
                content: "task".into(),
            }),
            // turn/start events advance the writer's envelope-turn counter
            // exactly as the tee does per user-visible round.
            SessionEventBody::TurnStart(shannon_types::session_event::TurnStartPayload {
                query_id: None,
            }),
            // Turn 1 closes with a usage triple.
            SessionEventBody::TurnEnd(TurnEndPayload {
                reason: TurnEndPayload::REASON_COMPLETED.into(),
                usage: Some(usage(100, 20, 5, 50, 0.25)),
                error: None,
            }),
            SessionEventBody::TurnStart(shannon_types::session_event::TurnStartPayload {
                query_id: None,
            }),
            // Turn 2: two calls — second errors once then retries verbatim and fails.
            SessionEventBody::ToolCall(shannon_types::session_event::ToolCallPayload {
                tool_use_id: "t1".into(),
                tool_name: "Read".into(),
                arguments: r#"{"file_path":"a"}"#.into(),
            }),
            SessionEventBody::ToolResult(shannon_types::session_event::ToolResultPayload {
                tool_use_id: "t1".into(),
                tool_name: "Read".into(),
                output: "text".into(),
                is_error: false,
                duration_ms: None,
                meta: serde_json::Value::Null,
            }),
            call("t2", "Edit", r#"{"old_string":"z"}"#),
            result("t2", "Edit", true),
            call("t3", "Edit", r#"{"old_string":"z"}"#), // verbatim retry (invalid)
            result("t3", "Edit", true),
            // A different-signature recovery keeps the failure contained but healthy.
            call("t4", "Edit", r#"{"old_string":"y"}"#),
            result("t4", "Edit", false),
            // Two permission denials plus one ask that must not count.
            SessionEventBody::PermissionDecision(
                shannon_types::session_event::PermissionDecisionPayload {
                    tool_name: Some("Bash".into()),
                    request: Some("rm".into()),
                    decision: "deny".into(),
                    reason: None,
                    mode: None,
                },
            ),
            SessionEventBody::PermissionDecision(
                shannon_types::session_event::PermissionDecisionPayload {
                    tool_name: Some("Bash".into()),
                    request: Some("git push".into()),
                    decision: "denied".into(),
                    reason: None,
                    mode: None,
                },
            ),
            SessionEventBody::PermissionDecision(
                shannon_types::session_event::PermissionDecisionPayload {
                    tool_name: Some("Write".into()),
                    request: Some("note.txt".into()),
                    decision: "allow".into(),
                    reason: None,
                    mode: None,
                },
            ),
            // Compaction + error categories feed the classifier signals.
            SessionEventBody::SurfaceReplace(SurfaceReplacePayload {
                start_seq: 1,
                end_seq: 4,
                source_event_seqs: vec![1, 2, 3],
                reason: "compaction".into(),
            }),
            SessionEventBody::Error(ErrorPayload {
                category: "rate_limit".into(),
                message: "429".into(),
                detail: None,
            }),
            // Final turn closes (interrupted marker exercises the signal).
            SessionEventBody::TurnEnd(TurnEndPayload {
                reason: TurnEndPayload::REASON_INTERRUPTED.into(),
                usage: Some(usage(30, 10, 0, 80, 0.05)),
                error: None,
            }),
            SessionEventBody::TodoWrite(TodoWritePayload {
                todos: vec![TodoSnapshotEntry {
                    content: "done".into(),
                    status: "completed".into(),
                }],
            }),
        ];
        let path = write_log(dir.path(), "sess-extract", bodies);

        let extracted = extract_from_events_log(&[path]).unwrap();
        let m = &extracted.metrics;

        assert_eq!(missing_fields(m), Vec::<&str>::new(), "every field present");
        assert_eq!(m.source, MetricSource::EventsLog);
        assert_eq!(m.tokens_in, 130);
        assert_eq!(m.tokens_out, 30);
        assert_eq!(m.cache_creation_tokens, 5);
        assert_eq!(m.cache_read_tokens, 130);
        assert_eq!(m.cost_usd, Some(0.30));
        assert_eq!(m.turns, 2);
        assert_eq!(m.tool_calls, 4);
        assert_eq!(
            m.permission_blocks, 2,
            "deny + denied counted, allow skipped"
        );
        assert_eq!(m.invalid_calls, 1);
        assert_eq!(m.loops, 0);
        assert!(m.wall_clock_ms.unwrap_or(u64::MAX) <= 1000, "tiny log");

        let s = &extracted.signals;
        assert_eq!(s.tool_errors, 2);
        assert_eq!(s.edit_tool_errors, 2, "both failing results were Edits");
        assert_eq!(
            s.tool_error_unrecovered, 1,
            "only the z-anchor signature stayed failed"
        );
        assert!(s.turn_interrupted);
        assert_eq!(s.compaction_events, 1);
        assert_eq!(s.error_categories, vec!["rate_limit"]);

        // Signal table exposes the flattenings rules rely on.
        let table = s.signal_table();
        assert_eq!(
            table.get("error_rate_limit").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            table.get("turn_interrupted").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn multi_file_extraction_concatenates_in_given_order() {
        let dir = TempDir::new().unwrap();
        let first = write_log(
            dir.path(),
            "one",
            vec![SessionEventBody::TurnEnd(TurnEndPayload {
                reason: TurnEndPayload::REASON_COMPLETED.into(),
                usage: Some(usage(10, 5, 0, 0, 0.01)),
                error: None,
            })],
        );
        let second = write_log(
            dir.path(),
            "two",
            vec![
                call("a", "Read", r#"{"p":1}"#),
                result("a", "Read", false),
                call("b", "Read", r#"{"p":1}"#),
                result("b", "Read", false),
                call("c", "Read", r#"{"p":1}"#),
                result("c", "Read", false),
                SessionEventBody::TurnEnd(TurnEndPayload {
                    reason: TurnEndPayload::REASON_COMPLETED.into(),
                    usage: Some(usage(4, 2, 1, 3, 0.02)),
                    error: None,
                }),
            ],
        );
        let extracted = extract_from_events_log(&[first, second]).unwrap();
        assert_eq!(extracted.metrics.tokens_in, 14);
        assert_eq!(extracted.metrics.cost_usd, Some(0.03));
        assert_eq!(extracted.metrics.tool_calls, 3);
        assert_eq!(
            extracted.metrics.loops, 1,
            "three consecutive reads across files"
        );
    }

    #[test]
    fn find_event_logs_discovers_isolated_home_layout_sorted() {
        let dir = TempDir::new().unwrap();
        assert!(find_event_logs(dir.path()).is_empty());

        write_log(dir.path(), "bbb", Vec::new());
        write_log(dir.path(), "aaa", Vec::new());
        let found = find_event_logs(dir.path());
        assert_eq!(found.len(), 2);
        assert!(found[0].display().to_string().ends_with("aaa/events.jsonl"));
        assert!(found[1].display().to_string().ends_with("bbb/events.jsonl"));
    }

    #[test]
    fn derived_stream_metrics_match_counters_without_fabricating_cost() {
        let ndjson = concat!(
            "{\"type\":\"start\",\"session_id\":\"s\",\"model\":\"stub\"}\n",
            "{\"type\":\"tool_call\",\"name\":\"Edit\",\"input\":{\"old_string\":\"a\"}}\n",
            "{\"type\":\"tool_result\",\"name\":\"Edit\",\"success\":false,\"output\":\"nope\"}\n",
            "{\"type\":\"tool_call\",\"name\":\"Edit\",\"input\":{\"old_string\":\"a\"}}\n",
            "{\"type\":\"tool_result\",\"name\":\"Edit\",\"success\":true,\"output\":\"ok\"}\n",
            "{\"type\":\"text_delta\",\"content\":\"done\"}\n",
            "{\"type\":\"done\",\"exit_code\":0,\"turns_used\":3,\"tokens_used\":99,\"tokens_in\":60,\"tokens_out\":39}\n",
        );
        let extracted = derive_from_stream(ndjson);
        let m = &extracted.metrics;

        assert_eq!(missing_fields(m), Vec::<&str>::new());
        assert_eq!(m.source, MetricSource::DerivedStream, "must be labeled");
        assert_eq!(m.cost_usd, None, "streams carry no cost observation");
        assert_eq!(m.cache_creation_tokens, 0);
        assert_eq!(m.tokens_in, 60);
        assert_eq!(m.turns, 3);
        assert_eq!(m.tool_calls, 2);
        assert_eq!(m.invalid_calls, 1, "errored verbatim retry counted");
        assert_eq!(m.wall_clock_ms, None);
        assert_eq!(extracted.signals.edit_tool_errors, 1);
    }

    #[test]
    fn embedded_rule_table_parses_and_classifies_all_seven_classes() {
        let rules = FailureRules::embedded();
        assert!(!rules.fingerprint().is_empty());
        assert!(rules.rules.len() >= 7, "at least one rule per class");

        let base_signals = FailureSignals {
            ..FailureSignals::default()
        };
        let context_for = |metrics: TaskMetrics, passed: bool, status: &str| ClassifyContext {
            status: status.to_string(),
            passed,
            violations: 2,
            metrics,
            signals: base_signals.clone(),
        };

        // ⑤ timeout_or_limit — guardrail status family…
        let m = TaskMetrics {
            ..TaskMetrics::default()
        };
        let verdict = rules
            .classify(&context_for(m.clone(), false, "timeout"))
            .expect("timeout classified");
        assert_eq!(verdict.class, "timeout_or_limit");

        // …and the rate-limit error-category route.
        let mut rate_ctx = context_for(TaskMetrics::default(), false, "failed");
        rate_ctx.signals.error_categories = vec!["rate_limit".into()];
        let verdict = rules.classify(&rate_ctx).expect("rate limit classified");
        assert_eq!(verdict.class, "timeout_or_limit");

        // ④ permission misreject.
        let denied = TaskMetrics {
            permission_blocks: 1,
            ..TaskMetrics::default()
        };
        let verdict = rules
            .classify(&context_for(denied, false, "failed"))
            .unwrap();
        assert_eq!(verdict.class, "permission_misreject");

        // ⑥ edit conflict — edit errors dominate with nothing recovered.
        let edits = TaskMetrics::default();
        let mut ctx = context_for(edits, false, "failed");
        ctx.signals.edit_tool_errors = 2;
        ctx.signals.tool_error_unrecovered = 1;
        let verdict = rules.classify(&ctx).unwrap();
        assert_eq!(verdict.class, "edit_conflict");

        // ② plain unrecovered tool failure (non-edit tool).
        let mut ctx = context_for(TaskMetrics::default(), false, "failed");
        ctx.signals.tool_error_unrecovered = 1;
        let verdict = rules.classify(&ctx).unwrap();
        assert_eq!(verdict.class, "tool_failure_unrecovered");

        // ⑦ model ceiling — strict loop threshold reached.
        let looping = TaskMetrics {
            loops: 1,
            loop_max_streak: 3,
            ..TaskMetrics::default()
        };
        let verdict = rules
            .classify(&context_for(looping, false, "failed"))
            .unwrap();
        assert_eq!(verdict.class, "model_ceiling");

        // ③ context loss — interrupted turn or compaction while failing.
        let mut ctx = context_for(TaskMetrics::default(), false, "failed");
        ctx.signals.compaction_events = 1;
        let verdict = rules.classify(&ctx).unwrap();
        assert_eq!(verdict.class, "context_loss");
        let mut ctx = context_for(TaskMetrics::default(), false, "failed");
        ctx.signals.turn_interrupted = true;
        assert_eq!(rules.classify(&ctx).unwrap().class, "context_loss");

        // ① instruction misunderstanding — the clean-trace residual bucket.
        let quiet = TaskMetrics::default();
        let verdict = rules
            .classify(&context_for(quiet, false, "failed"))
            .unwrap();
        assert_eq!(verdict.class, "instruction_misunderstanding");

        // Passed tasks never carry a failure class.
        assert!(
            rules
                .classify(&context_for(TaskMetrics::default(), true, "passed"))
                .is_none()
        );
        // Spawn-error rows fall through cleanly when nothing matches.
        assert!(
            rules
                .classify(&context_for(TaskMetrics::default(), false, "spawn_error"))
                .is_none()
        );
    }

    #[test]
    fn external_rules_override_and_validation_rejects_bad_tables() {
        let dir = TempDir::new().unwrap();

        // Valid override: only one rule — everything else becomes unclassified.
        let override_body = concat!(
            "schema_version = 1\n\n[[rule]]\n",
            "class = \"model_ceiling\"\ndescription = \"only loops matter\"\n",
            "[[rule.condition]]\nsignal = \"loops\"\nop = \"ge\"\nvalue = \"1\"\n"
        );
        let path = dir.path().join("rules.toml");
        std::fs::write(&path, override_body).unwrap();
        let rules = FailureRules::load(&path).unwrap();
        let ctx = ClassifyContext {
            status: "timeout".into(),
            passed: false,
            violations: 1,
            metrics: TaskMetrics::default(),
            signals: FailureSignals::default(),
        };
        assert!(
            rules.classify(&ctx).is_none(),
            "override narrows classification"
        );
        let looping = TaskMetrics {
            loops: 1,
            ..TaskMetrics::default()
        };
        let ctx = ClassifyContext {
            status: "failed".into(),
            passed: false,
            violations: 1,
            metrics: looping,
            signals: FailureSignals::default(),
        };
        assert_eq!(rules.classify(&ctx).unwrap().class, "model_ceiling");

        // Unknown class rejected at load time.
        let bad_class = "schema_version = 1\n\n[[rule]]\nclass = \"vibes\"\n";
        let path = dir.path().join("bad_class.toml");
        std::fs::write(&path, bad_class).unwrap();
        assert!(FailureRules::load(&path).is_err());

        // Unknown op rejected at load time.
        let bad_op = concat!(
            "schema_version = 1\n\n[[rule]]\nclass = \"model_ceiling\"\n",
            "[[rule.condition]]\nsignal = \"loops\"\nop = \"divines\"\nvalue = \"1\"\n"
        );
        let path = dir.path().join("bad_op.toml");
        std::fs::write(&path, bad_op).unwrap();
        assert!(FailureRules::load(&path).is_err());

        // Wrong schema version rejected.
        let bad_schema = "schema_version = 9\n";
        let path = dir.path().join("bad_schema.toml");
        std::fs::write(&path, bad_schema).unwrap();
        assert!(FailureRules::load(&path).is_err());
    }

    #[test]
    fn evidence_chain_names_every_satisfied_condition() {
        let rules = FailureRules::embedded();
        let ctx = ClassifyContext {
            status: "timeout".into(),
            passed: false,
            violations: 0,
            metrics: TaskMetrics {
                wall_clock_ms: Some(305_000),
                ..TaskMetrics::default()
            },
            signals: FailureSignals::default(),
        };
        let verdict = rules.classify(&ctx).expect("classified");
        assert!(
            verdict
                .evidence
                .iter()
                .any(|line| line.starts_with("status equals timeout"))
        );
        assert!(
            verdict
                .evidence
                .iter()
                .any(|line| line.starts_with("passed equals false"))
        );
        assert!(verdict.rule_description.contains("timeout"));
    }

    #[test]
    fn metric_field_contract_lists_exactly_the_struct_fields() {
        assert_eq!(METRIC_FIELDS.len(), 13);
        let serialized = serde_json::to_value(TaskMetrics::default()).unwrap();
        for field in METRIC_FIELDS {
            assert!(
                serialized.get(field).is_some(),
                "{field} must serialize even at defaults"
            );
        }
    }

    // ── W1 §4① metadata anchor ────────────────────────────────────────

    fn header_event(
        model: &str,
        provider: Option<&str>,
        snapshot: Value,
        wire_model: &str,
    ) -> SessionEventBody {
        SessionEventBody::RequestHeader(RequestHeaderPayload {
            model: model.into(),
            provider: provider.map(str::to_owned),
            adapter_defaults: Value::Null,
            system: None,
            tools: Vec::new(),
            config_snapshot: snapshot,
            reason: Some("initial".into()),
            wire_body: Some(serde_json::json!({"model": wire_model, "messages": []})),
        })
    }

    #[test]
    fn extract_anchor_prefers_wire_body_and_digests_config_snapshot() {
        let events = vec![SessionEvent::new(
            0,
            1,
            "s",
            1,
            header_event(
                "declared-model",
                Some("anthropic"),
                serde_json::json!({"profile": "full_auto"}),
                "wire-model",
            ),
        )];

        let anchor = extract_anchor(&events);
        assert_eq!(
            anchor.model_id.as_deref(),
            Some("wire-model"),
            "wire_body.model is what the provider actually received"
        );
        assert_eq!(anchor.provider.as_deref(), Some("anthropic"));
        let digest = anchor
            .profile_digest
            .clone()
            .expect("config snapshot digested");
        assert_eq!(digest.len(), 16, "fingerprint style: 16 hex chars");

        // Deterministic, and sensitive to the snapshot content.
        assert_eq!(anchor, extract_anchor(&events));
        let mut other_snapshot = events;
        let SessionEventBody::RequestHeader(header) = &mut other_snapshot[0].body else {
            unreachable!("header event constructed");
        };
        header.config_snapshot = serde_json::json!({"profile": "read_only"});
        let reanchored = extract_anchor(&other_snapshot);
        assert_ne!(
            anchor.profile_digest, reanchored.profile_digest,
            "a different config in force must digest differently"
        );
    }

    #[test]
    fn extract_anchor_falls_back_to_session_start_banner() {
        let banner = SessionEvent::new(
            0,
            1,
            "s",
            1,
            SessionEventBody::SessionStart(SessionStartPayload {
                model: "banner-model".into(),
                provider: Some("mock".into()),
                cwd: None,
                app_version: None,
            }),
        );

        // Banner only: model+provider come from session/start, no digest.
        let anchor = extract_anchor(std::slice::from_ref(&banner));
        assert_eq!(anchor.model_id.as_deref(), Some("banner-model"));
        assert_eq!(anchor.provider.as_deref(), Some("mock"));
        assert!(anchor.profile_digest.is_none());

        // A header without wire_body: the declared header model wins over the
        // banner, and a null config snapshot stays unknown.
        let header_no_wire_model = SessionEvent::new(
            1,
            2,
            "s",
            1,
            SessionEventBody::RequestHeader(RequestHeaderPayload {
                model: "header-model".into(),
                provider: Some("mock".into()),
                adapter_defaults: Value::Null,
                system: None,
                tools: Vec::new(),
                config_snapshot: Value::Null,
                reason: Some("initial".into()),
                wire_body: Some(serde_json::json!({"messages": []})),
            }),
        );
        let anchor = extract_anchor(&[banner, header_no_wire_model]);
        assert_eq!(anchor.model_id.as_deref(), Some("header-model"));
        assert_eq!(anchor.provider.as_deref(), Some("mock"));
        assert!(
            anchor.profile_digest.is_none(),
            "null snapshot must stay unknown, not hash to a constant"
        );
    }

    #[test]
    fn extract_anchor_without_anchors_stays_honestly_unknown() {
        let anchor = extract_anchor(&[]);
        assert!(anchor.is_unknown());

        let no_metadata = vec![SessionEvent::new(
            0,
            1,
            "s",
            1,
            SessionEventBody::UserMessage(UserMessagePayload {
                source: UserMessagePayload::SOURCE_USER.into(),
                content: "task".into(),
            }),
        )];
        assert!(extract_anchor(&no_metadata).is_unknown());
    }

    #[test]
    fn unclassified_is_a_tally_bucket_not_a_rule_class() {
        assert_eq!(UNCLASSIFIED_CLASS, "unclassified");
        assert!(
            !FAILURE_CLASSES.contains(&UNCLASSIFIED_CLASS),
            "the residue bucket must never be claimable by a rule"
        );
    }
}
