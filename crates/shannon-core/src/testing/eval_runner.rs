//! Tiered L1 evaluation suite runner (master plan §4.4 W2-M1b).
//!
//! Executes a declarative TOML task suite (`tests/eval/tasks/*.toml`) against
//! the Shannon engine — either as a **real** model run (`shannon --prompt`,
//! NDJSON streamed) or a **dry-run** pipeline rehearsal — and verifies each
//! run against the shared [`crate::testing::scenario`] assertion vocabulary
//! (all 11 [`ValidationRule`] variants; no separate DSL here).
//!
//! ## Pipeline
//!
//! ```text
//! load tasks ──► prepare sandbox workspace ──► execute
//!     │              (files + optional git init)     │ real:  spawn `<bin> --prompt …
//!     │                                              │        --output-format json-stream`
//!     │                                              │        capturing NDJSON from stdout
//!     │                                              └ dry:   scripted stub emitting the same
//!     │                                                       NDJSON schema; its primitive
//!     │                                                       tool effects act on the REAL
//!     │                                                       workspace files
//!     ▼
//! classify limits (turn/token budget, wall-clock timeout) ──► verify (rules ∪ script ∪
//! expectations) ──► dual reports: report.json + report.md under
//! `$SHANNON_HOME/eval/runs/<run-id>/` (per-task subdirectories retain the
//! workspace, the captured NDJSON stream, and a copy of the effective task).
//! ```
//!
//! Limit classes: exceeding `limits.max_turns` maps to [`RunStatus::TurnLimit`],
//! exceeding `limits.max_tokens` to [`RunStatus::TokenLimit`], and breaching
//! `limits.timeout_secs` to [`RunStatus::Timeout`] — i.e. 「超时/限额」 categories
//! stay distinct from ordinary assertion failures ([`RunStatus::Failed`]).
//!
//! ## Metrics & failure classification (§4.7 W2-M2)
//!
//! Every finished task is enriched by the sibling [`super::eval_metrics`]
//! module:
//!
//! - **Real runs**: the child engine is spawned with an isolated
//!   `SHANNON_HOME` (`<task_dir>/l0-home`), so the §4.2 tee writes the task's
//!   genuine `events.jsonl` inside the evidence directory; metrics come from
//!   that log (`metrics_source = events_jsonl`) and failing tasks have the
//!   log archived under `$SHANNON_HOME/eval/failures/<date>/<task>/`.
//! - **Dry-run rehearsals**: the stub never spawns the engine, so no L0 log
//!   can exist — metrics are derived from the captured NDJSON stream and the
//!   report stamps `metrics_source = derived_stream`. Cost/cache fields hold
//!   honest unknowns; nothing fabricates an L0 log.
//! - **Failure classes**: seven classes judged by the external TOML rule
//!   table ([`EvalOptions::failure_rules`] → embedded default), recorded per
//!   task with the winning rule's evidence chain.
//!
//! ## Engine contract
//!
//! The real path shells out to the headless CLI contract of
//! `shannon --prompt <text> --output-format json-stream --max-turns <n>`:
//!
//! - NDJSON event types consumed: `start`, `tool_call` (`name`/`input`),
//!   `text_delta` (`content`), `tool_result` (`success` or `is_error`), and
//!   `done` (`exit_code`, `turns_used`, `tokens_used`, `tokens_in`,
//!   `tokens_out`). Unknown lines are ignored (forward compatible).
//! - Exit codes (see `crates/shannon-cli/src/main.rs::HeadlessExitCode`):
//!   0 success · 1 error · 2 turn limit · 3 timeout · 4 rate limit ·
//!   5 context overflow · 6 permission denied.
//!
//! The trajectory is built solely from `tool_call` lines because the CLI emits
//! each invocation once as a `tool_call` CI event **and** once as a `tool_use`
//! output event; parsing both would double-count every step.
//!
//! ## Honesty notes
//!
//! - The dry-run stub derives its tool calls from the task's `[dry_run]`
//!   script but its effects go through working `Read`/`Write`/`Edit`/
//!   `MultiEdit`/`Glob`/`Grep`/limited-`Bash` primitives operating on the real
//!   sandbox files, so `verify.rules` evaluate genuine workspace state and a
//!   genuinely parsed NDJSON stream. A dry run validates the harness end to
//!   end (load → prepare → limits → capture → verify → dual reports), never
//!   the model policy. Per-task `[dry_run]` scripts double as living
//!   documentation of the expected solution path.
//! - USD costs became observable once the §4.7 extractor reads `turn/end`
//!   usage from the L0 log; `cost_below` rules remain expressible but the
//!   shipped suite still carries no cost ceilings until real-run baselines
//!   exist. As before, the rule fails loudly on empty cost observations
//!   rather than passing vacuously.
//!
//! ## Soft forbidden tools (RCA 2026-08-28 §5 决策点 1/2)
//!
//! `expectations.forbidden_tools` folds into the rule set with tier-dependent
//! strictness: **recovery-tier (`rec_*`) tasks keep the hard contract** — a
//! forbidden-tool hit there still fails the run — while every other tier
//! injects the ban as `strict = false`. Outside recovery, tool choice is a
//! means to the verified outcome, not the contract: a Bash detour (e.g.
//! multi_04 self-verifying TOML via shell) no longer overrides a correct
//! result. Violations are never dropped — they surface as `soft_flags` on
//! [`TaskRunRecord`] and in the report's `soft` column, mirroring the
//! observational `over_expected` marker. The YAML-scenario default stays
//! `strict = true`, so existing scenario fixtures keep their semantics.

use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::testing::scenario::{
    RuleOutcome, ToolCallTrace, TrajectoryStep, ValidationContext, ValidationRule, evaluate_rules,
};

use regex::Regex;

use super::eval_metrics::{
    Classification, ClassifyContext, DRY_RUN_ANCHOR_MODEL, ExtractedTask, FailureRules,
    MetricSource, RunAnchor, TaskMetrics, UNCLASSIFIED_CLASS, derive_from_stream, extract_anchor,
    extract_from_events_log, find_event_logs, missing_fields,
};

/// Deterministic output-root resolution mirroring
/// [`crate::testing`]'s sibling convention (`session_log::default_shannon_home`):
/// `$SHANNON_HOME` when set, otherwise `~/.shannon`; eval runs land beneath
/// `eval/runs/<run-id>/` there.
pub fn resolve_eval_home() -> Result<PathBuf, EvalError> {
    if let Ok(home) = std::env::var("SHANNON_HOME") {
        return Ok(PathBuf::from(home));
    }
    let home = dirs::home_dir().ok_or_else(|| {
        EvalError::Config("cannot determine home directory (set SHANNON_HOME)".into())
    })?;
    Ok(home.join(".shannon"))
}

// ── Errors ─────────────────────────────────────────────────────────────

/// Runner-level failure (as opposed to per-task assertion failure).
#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    /// Task file missing/malformed, suite directory unreadable, bad options.
    #[error("{0}")]
    Config(String),
    /// Filesystem I/O during workspace preparation or artifact persistence.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Report (de)serialization failure.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

// ── Task schema (tests/eval/tasks/*.toml) ──────────────────────────────

/// One tier label of the L1 suite.
///
/// `read` — comprehension questions, no mutations expected;
/// `edit` — precise single-file modifications;
/// `search` — locate information across the tree (Grep/Glob-centric);
/// `multi_step` — coordinated changes spanning several files/tools;
/// `recovery` — a snag occurs mid-flight and must be corrected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalTier {
    /// Comprehension only — answer from given sources, mutate nothing.
    Read,
    /// Precise single-file modification.
    Edit,
    /// Cross-tree information location (Grep/Glob-heavy).
    Search,
    /// Coordinated multi-file / multi-tool change.
    MultiStep,
    /// Deliberate snag followed by course correction.
    Recovery,
}

impl EvalTier {
    /// The lowercase spelling used in TOML and reports.
    pub fn as_str(self) -> &'static str {
        match self {
            EvalTier::Read => "read",
            EvalTier::Edit => "edit",
            EvalTier::Search => "search",
            EvalTier::MultiStep => "multi_step",
            EvalTier::Recovery => "recovery",
        }
    }
}

/// Task completion-length class (design §2): how long a task is *expected*
/// to take, as opposed to [`EvalTier`]'s capability dimension. Drives the
/// default limits table ([`Horizon::default_limits`]) and is stamped into
/// every report row so short- and long-horizon numbers are never read as
/// comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Horizon {
    /// Read/search/precise-edit scale — minutes.
    #[default]
    Short,
    /// Multi-step changes — tens of minutes.
    Mid,
    /// Long-horizon refactors — up to ~15 minutes wall clock / millions of
    /// tokens; sized to keep L2 long tasks from being strangled by the
    /// harness before the model gives up on its own.
    Long,
}

impl Horizon {
    /// The lowercase spelling used in TOML and reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Horizon::Short => "short",
            Horizon::Mid => "mid",
            Horizon::Long => "long",
        }
    }

    /// W1 default limits table (design §2①): explicit task limits always
    /// win; these fill the gaps.
    ///
    /// | horizon | max_turns | max_tokens | timeout_secs |
    /// |---------|-----------|------------|--------------|
    /// | short   | 12        | 300 000    | 180          |
    /// | mid     | 30        | 800 000    | 450          |
    /// | long    | 80        | 2 000 000  | 900          |
    pub fn default_limits(self) -> ResolvedLimits {
        match self {
            Horizon::Short => ResolvedLimits {
                max_turns: DEFAULT_MAX_TURNS,
                max_tokens: DEFAULT_MAX_TOKENS,
                timeout_secs: DEFAULT_TIMEOUT_SECS,
            },
            Horizon::Mid => ResolvedLimits {
                max_turns: MID_MAX_TURNS,
                max_tokens: MID_MAX_TOKENS,
                timeout_secs: MID_TIMEOUT_SECS,
            },
            Horizon::Long => ResolvedLimits {
                max_turns: LONG_MAX_TURNS,
                max_tokens: LONG_MAX_TOKENS,
                timeout_secs: LONG_TIMEOUT_SECS,
            },
        }
    }
}

/// An initial workspace file seeded by `setup.files`.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskFile {
    /// Relative path inside the task workspace (parent dirs auto-created).
    pub path: String,
    /// Full file content written verbatim.
    pub content: String,
}

/// Sandbox seed: initial files plus an optional `git init`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TaskSetup {
    /// Files created before execution begins.
    pub files: Vec<TaskFile>,
    /// Run `git init` in the workspace (for tasks exercising repo awareness).
    pub git_init: bool,
}

impl Default for TaskSetup {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            git_init: false,
        }
    }
}

/// Verification section: inline assertion rules and/or a shell script.
///
/// At least one mechanism must be non-empty (enforced by [`EvalTask::validate`]).
/// Rules reuse the exact [`ValidationRule`] vocabulary of the YAML scenario
/// framework (`rule = "…"`, fields identical); a malformed rule surfaces as a
/// parse error.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TaskVerify {
    /// Shell snippet executed in the task workspace after the run; a non-zero
    /// exit records a violation. Unix only (executed via `sh -c`).
    pub script: String,
    /// Scenario-framework rules evaluated against the finished run.
    pub rules: Vec<ValidationRule>,
}

impl Default for TaskVerify {
    fn default() -> Self {
        Self {
            script: String::new(),
            rules: Vec::new(),
        }
    }
}

/// Behavioral expectations carried separately from hard assertions:
/// the expected trajectory template and forbidden-tool bans.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TaskExpectations {
    /// Expected tool-call steps (ordered subsequence, `args_regex` allowed),
    /// shaped exactly like scenario `trajectory_contains.sequence` entries.
    pub trajectory: Vec<TrajectoryStep>,
    /// Tools that must not appear anywhere in the observed trajectory.
    pub forbidden_tools: Vec<String>,
}

/// Soft expectation budget (design §2③): the task author's sense of what the
/// run *should* cost. Exceeding it raises the observational
/// `over_expected` marker on the report row — it never changes
/// [`RunStatus`] or `passed`. This is deliberately distinct from the hard
/// `verify` assertions (e.g. `cost_below`), which fail the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(default)]
pub struct ExpectedBudget {
    /// Expected turn count.
    pub turns: Option<u32>,
    /// Expected session token bill.
    pub tokens: Option<u64>,
}

/// Per-task engineering guardrails; unset fields fall back to the
/// horizon-default table ([`Horizon::default_limits`]) resolved by
/// [`EvalTask::resolved_limits`].
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TaskLimits {
    /// Upper bound on agent turns (also forwarded as the CLI `--max-turns`
    /// guard in real runs). Breach ⇒ [`RunStatus::TurnLimit`].
    pub max_turns: Option<u32>,
    /// Session token ceiling checked against the engine-side totals reported
    /// on the `done` event. Breach ⇒ [`RunStatus::TokenLimit`].
    pub max_tokens: Option<u64>,
    /// Wall-clock budget for the whole task in seconds. Breach kills the
    /// child process and marks [`RunStatus::Timeout`].
    pub timeout_secs: Option<u64>,
    /// Soft expectation budget (`expected = { turns = …, tokens = … }`);
    /// exceeding it only flags `over_expected` on the report row.
    pub expected: Option<ExpectedBudget>,
}

/// Horizon default row: `short` max turns.
pub const DEFAULT_MAX_TURNS: u32 = 12;
/// Horizon default row: `short` token ceiling.
pub const DEFAULT_MAX_TOKENS: u64 = 300_000;
/// Horizon default row: `short` wall-clock budget in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 180;
/// Horizon default row: `mid` max turns.
pub const MID_MAX_TURNS: u32 = 30;
/// Horizon default row: `mid` token ceiling.
pub const MID_MAX_TOKENS: u64 = 800_000;
/// Horizon default row: `mid` wall-clock budget in seconds.
pub const MID_TIMEOUT_SECS: u64 = 450;
/// Horizon default row: `long` max turns.
pub const LONG_MAX_TURNS: u32 = 80;
/// Horizon default row: `long` token ceiling.
pub const LONG_MAX_TOKENS: u64 = 2_000_000;
/// Horizon default row: `long` wall-clock budget in seconds.
pub const LONG_TIMEOUT_SECS: u64 = 900;

/// Fully-resolved limits (defaults filled in).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedLimits {
    pub max_turns: u32,
    pub max_tokens: u64,
    pub timeout_secs: u64,
}

impl ResolvedLimits {
    /// Apply the `short`-horizon defaults over optionally-declared task
    /// limits. Kept for callers without a horizon context; task runs should
    /// prefer [`EvalTask::resolved_limits`].
    pub fn of(limits: &TaskLimits) -> Self {
        Self::for_horizon(limits, Horizon::Short)
    }

    /// Apply the horizon's default table over optionally-declared task
    /// limits — explicit task limits always win.
    pub fn for_horizon(limits: &TaskLimits, horizon: Horizon) -> Self {
        let defaults = horizon.default_limits();
        Self {
            max_turns: limits.max_turns.unwrap_or(defaults.max_turns),
            max_tokens: limits.max_tokens.unwrap_or(defaults.max_tokens),
            timeout_secs: limits.timeout_secs.unwrap_or(defaults.timeout_secs),
        }
    }
}

/// W1 §2③ over-expected soft marker: multiples of the declared expectation
/// budget that the run actually exceeded. Presence on a report row *is* the
/// `over_expected: true` flag; the multiples answer "how far over". Purely
/// observational — it never feeds `passed` or [`RunStatus`].
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OverExpected {
    /// actual_turns / expected_turns; absent unless that expectation was
    /// declared and exceeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turns_multiple: Option<f64>,
    /// total_tokens / expected_tokens; same presence rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_multiple: Option<f64>,
}

impl OverExpected {
    /// Compare a finished run against its declared expectations. `None` when
    /// nothing was declared or nothing was exceeded; a zero-valued
    /// expectation is skipped (no division, no division-by-zero masquerading
    /// as a verdict).
    pub fn of(turns: u32, total_tokens: u64, expected: Option<ExpectedBudget>) -> Option<Self> {
        let expected = expected?;
        let turns_multiple = expected
            .turns
            .filter(|want| *want > 0)
            .map(|want| f64::from(turns) / f64::from(want))
            .filter(|multiple| *multiple > 1.0);
        let tokens_multiple = expected
            .tokens
            .filter(|want| *want > 0)
            .map(|want| total_tokens as f64 / want as f64)
            .filter(|multiple| *multiple > 1.0);
        (turns_multiple.is_some() || tokens_multiple.is_some()).then_some(Self {
            turns_multiple,
            tokens_multiple,
        })
    }
}

/// One scripted stub step for the dry-run rehearsal. `input` holds the same
/// argument shape a real engine run would carry (e.g. `file_path`, `pattern`,
/// `command`) so trajectories and `args_regex` expectations stay realistic.
#[derive(Debug, Clone, Deserialize)]
pub struct DryRunStep {
    /// Tool name as it should appear in the trajectory.
    pub tool: String,
    /// Tool arguments (JSON object); interpreted by the stub primitives.
    #[serde(default)]
    pub input: Value,
    /// When true the step's effect is skipped and an error `tool_result` is
    /// synthesized — modeling a snag the subsequent steps recover from.
    #[serde(default)]
    pub fail: bool,
}

/// Optional dry-run rehearsal script; when omitted the task simply cannot be
/// rehearsed without a real engine invocation.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct DryRunScript {
    /// Final assistant answer text emitted after the scripted steps.
    pub final_text: String,
    /// Ordered tool invocations performed by the stub executor.
    pub steps: Vec<DryRunStep>,
    /// Override the synthesized `done` token totals (useful to exercise the
    /// token-limit classification deliberately).
    pub tokens_used: Option<u64>,
}

/// One declarative eval task — the schema of `tests/eval/tasks/*.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct EvalTask {
    /// Stable identifier (`read_01`, `edit_02`, …); becomes the run directory.
    pub id: String,
    /// Suite tier.
    pub tier: EvalTier,
    /// Expected completion length (design §2); when omitted it is derived
    /// from the tier — read/search/edit ⇒ short, multi_step/recovery ⇒ mid.
    #[serde(default)]
    pub horizon: Option<Horizon>,
    /// One-line intent shown in reports.
    #[serde(default)]
    pub description: String,
    /// The prompt handed to the agent verbatim.
    pub prompt: String,
    /// Sandbox seeding.
    #[serde(default)]
    pub setup: TaskSetup,
    /// Assertions (rules + optional shell script).
    #[serde(default)]
    pub verify: TaskVerify,
    /// Expected trajectory template + forbidden-tool bans.
    #[serde(default)]
    pub expectations: TaskExpectations,
    /// Engineering guardrails.
    #[serde(default)]
    pub limits: TaskLimits,
    /// Dry-run rehearsal script (harness pipeline validation).
    #[serde(default)]
    pub dry_run: DryRunScript,
    /// Original TOML path, attached by [`parse_task`] so runs can archive a
    /// byte-exact copy beside the workspace. Not part of the file schema.
    #[serde(skip)]
    pub(crate) task_source_path_hint: Option<PathBuf>,
}

impl EvalTask {
    /// The horizon in force for this task: the explicit `horizon` field when
    /// declared, otherwise the tier-derived default (design §2①).
    pub fn effective_horizon(&self) -> Horizon {
        self.horizon.unwrap_or(match self.tier {
            EvalTier::Read | EvalTier::Search | EvalTier::Edit => Horizon::Short,
            EvalTier::MultiStep | EvalTier::Recovery => Horizon::Mid,
        })
    }

    /// Limits in force: explicit `limits` fields win, unset fields fall back
    /// to this task's horizon row of the default table.
    pub fn resolved_limits(&self) -> ResolvedLimits {
        ResolvedLimits::for_horizon(&self.limits, self.effective_horizon())
    }

    /// Structural self-checks beyond serde (non-empty ids/prompts, usable
    /// verification story). Returns human-readable problems.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.id.trim().is_empty() {
            problems.push("id must not be empty".to_string());
        }
        if self.prompt.trim().is_empty() {
            problems.push(format!("task '{}': prompt must not be empty", self.id));
        }
        if self.verify.script.trim().is_empty()
            && self.verify.rules.is_empty()
            && self.expectations.trajectory.is_empty()
            && self.expectations.forbidden_tools.is_empty()
        {
            problems.push(format!(
                "task '{}': define at least one of verify.rules, verify.script, \
                 expectations.trajectory, expectations.forbidden_tools",
                self.id
            ));
        }
        if self.dry_run.steps.is_empty() && self.dry_run.final_text.trim().is_empty() {
            problems.push(format!(
                "task '{}': dry_run rehearsal script is empty (pipeline cannot be validated)",
                self.id
            ));
        }
        // Reflexive checks on limit sanity — cheaper to catch here than mid-run.
        let limits = self.resolved_limits();
        if limits.max_turns == 0 || limits.timeout_secs == 0 {
            problems.push(format!("task '{}': limits must be positive", self.id));
        }
        problems
    }

    #[doc(hidden)]
    fn with_source_hint(mut self, hint: PathBuf) -> Self {
        self.task_source_path_hint = Some(hint);
        self
    }
}

/// Parse one TOML task file.
pub fn parse_task(path: &Path) -> Result<EvalTask, EvalError> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| EvalError::Config(format!("failed to read {}: {e}", path.display())))?;
    let task: EvalTask =
        toml::from_str(&raw).map_err(|e| EvalError::Config(format!("{}: {e}", path.display())))?;
    Ok(task.with_source_hint(path.to_path_buf()))
}

/// Parse every `*.toml` under `dir`, sorted by file name for deterministic
/// report ordering.
pub fn parse_tasks_dir(dir: &Path) -> Result<Vec<EvalTask>, EvalError> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| EvalError::Config(format!("failed to read {}: {e}", dir.display())))?;
    let mut names: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    names.sort();
    let mut tasks = Vec::with_capacity(names.len());
    for path in names {
        tasks.push(parse_task(&path)?);
    }
    Ok(tasks)
}

// ── Engine stream (NDJSON) ─────────────────────────────────────────────

/// One decoded line of the engine's `json-stream` output.
#[derive(Debug, Clone, PartialEq)]
pub enum NdjsonLine {
    /// Session banner with engine ids.
    Start {
        session_id: Option<String>,
        model: Option<String>,
    },
    /// Authoritative tool invocation record.
    ToolCall { name: String, input_json: String },
    /// Tool completion acknowledgement (`success` falls back to negating
    /// the alternate-shape `is_error` field).
    ToolResult { name: String, success: Option<bool> },
    /// Streamed assistant text fragment.
    TextDelta { content: String },
    /// Terminal failure notice from the engine.
    Error { message: String },
    /// Terminal summary carrying accounting and exit semantics.
    Done {
        exit_code: Option<i32>,
        turns_used: Option<u32>,
        tokens_used: Option<u64>,
        tokens_in: Option<u64>,
        tokens_out: Option<u64>,
    },
    /// Anything unrecognized (forward compatibility).
    Other,
}

/// Decode one NDJSON line. Malformed JSON degrades to [`NdjsonLine::Other`]
/// rather than failing the whole run — stderr chatter occasionally bleeds
/// onto interleaved descriptors, and a single bogus line must not poison
/// otherwise-valid telemetry.
pub fn parse_ndjson_line(line: &str) -> NdjsonLine {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return NdjsonLine::Other;
    }
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return NdjsonLine::Other;
    };
    match value.get("type").and_then(Value::as_str) {
        Some("start") => NdjsonLine::Start {
            session_id: value
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            model: value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned),
        },
        Some("tool_call") => NdjsonLine::ToolCall {
            name: value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            input_json: value
                .get("input")
                .cloned()
                .map(|input| input.to_string())
                .unwrap_or_default(),
        },
        Some("tool_result") => NdjsonLine::ToolResult {
            name: value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            success: value
                .get("success")
                .and_then(Value::as_bool)
                .or_else(|| value.get("is_error").and_then(Value::as_bool).map(|e| !e)),
        },
        Some("text_delta") => NdjsonLine::TextDelta {
            content: value
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        },
        Some("error") => NdjsonLine::Error {
            message: value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        },
        Some("done") => NdjsonLine::Done {
            exit_code: value
                .get("exit_code")
                .and_then(Value::as_i64)
                .map(|v| v as i32),
            turns_used: value
                .get("turns_used")
                .and_then(Value::as_u64)
                .map(|v| v as u32),
            tokens_used: value.get("tokens_used").and_then(Value::as_u64),
            tokens_in: value.get("tokens_in").and_then(Value::as_u64),
            tokens_out: value.get("tokens_out").and_then(Value::as_u64),
        },
        _ => NdjsonLine::Other,
    }
}

/// Decoded view of one complete engine stream.
#[derive(Debug, Default, Clone)]
pub struct RunObservation {
    /// Engine session id (start event) — links reports to persisted sessions.
    pub session_id: Option<String>,
    /// Model name (start event).
    pub model: Option<String>,
    /// Ordered tool-call trajectory (authoritative `tool_call` lines only).
    pub trajectory: Vec<ToolCallTrace>,
    /// Concatenated assistant answer text (text fragments only, no echoes).
    pub answer_text: String,
    /// Error notices seen on the stream.
    pub errors: Vec<String>,
    /// Terminal summary, when the stream completed cleanly.
    pub done: Option<NdjsonLine>,
}

/// Fold a newline-delimited stream into a [`RunObservation`].
pub fn observe_stream(ndjson: &str) -> RunObservation {
    let mut obs = RunObservation::default();
    for line in ndjson.lines() {
        match parse_ndjson_line(line) {
            NdjsonLine::Start { session_id, model } => {
                obs.session_id = obs.session_id.or(session_id);
                obs.model = obs.model.or(model);
            }
            NdjsonLine::ToolCall { name, input_json } => {
                obs.trajectory.push(ToolCallTrace::new(&name, input_json));
            }
            NdjsonLine::TextDelta { content } => obs.answer_text.push_str(&content),
            NdjsonLine::Error { message } => obs.errors.push(message),
            done @ NdjsonLine::Done { .. } => {
                // Real headless runs may emit two `done` lines: an
                // engine-side one carrying usage/turns, then a final
                // exit marker with only `exit_code`. Merge field-wise so
                // the populated stats survive; a later line still wins
                // on any field it actually carries.
                obs.done = match obs.done.take() {
                    Some(NdjsonLine::Done {
                        exit_code: pe,
                        turns_used: pt,
                        tokens_used: pu,
                        tokens_in: pi,
                        tokens_out: po,
                    }) => {
                        let NdjsonLine::Done {
                            exit_code,
                            turns_used,
                            tokens_used,
                            tokens_in,
                            tokens_out,
                        } = done
                        else {
                            unreachable!("arm matched Done variant");
                        };
                        Some(NdjsonLine::Done {
                            exit_code: exit_code.or(pe),
                            turns_used: turns_used.or(pt),
                            tokens_used: tokens_used.or(pu),
                            tokens_in: tokens_in.or(pi),
                            tokens_out: tokens_out.or(po),
                        })
                    }
                    _ => Some(done),
                };
            }
            NdjsonLine::ToolResult { .. } | NdjsonLine::Other => {}
        }
    }
    obs
}

// ── Run records & reports ──────────────────────────────────────────────

/// Outcome classification of one executed task.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Every guardrail respected and every assertion satisfied.
    Passed,
    /// The run finished but at least one assertion/expectation was violated.
    Failed,
    /// Agent exceeded `limits.max_turns` (「限额」class).
    TurnLimit,
    /// Session token bill exceeded `limits.max_tokens` (「限额」class).
    TokenLimit,
    /// Wall-clock budget exhausted; child killed (「超时」class).
    Timeout,
    /// Engine could not be launched at all (missing binary, bad exec).
    SpawnError,
}

impl RunStatus {
    /// Short display spelling for tables/digests.
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Passed => "passed",
            RunStatus::Failed => "failed",
            RunStatus::TurnLimit => "turn_limit",
            RunStatus::TokenLimit => "token_limit",
            RunStatus::Timeout => "timeout",
            RunStatus::SpawnError => "spawn_error",
        }
    }
}

/// Serializable projection of a scenario [`RuleOutcome`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecordedRuleOutcome {
    pub rule: String,
    pub passed: bool,
    pub details: Vec<String>,
    /// Soft observations recorded without failing the rule (lenient
    /// `forbidden_tool` bans); empty for hard rules. Kept beside `details`
    /// so reports can show the detour without counting it as a failure.
    #[serde(default)]
    pub soft_flags: Vec<String>,
}

impl From<&RuleOutcome> for RecordedRuleOutcome {
    fn from(outcome: &RuleOutcome) -> Self {
        Self {
            rule: outcome.rule.clone(),
            passed: outcome.passed,
            details: outcome.details.clone(),
            soft_flags: outcome.soft_flags.clone(),
        }
    }
}

/// Persisted per-task result — one row of the report tables.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskRunRecord {
    pub id: String,
    pub tier: String,
    /// Completion-length class in force (explicit or tier-derived, §2①).
    #[serde(default)]
    pub horizon: String,
    pub status: RunStatus,
    /// True iff `status == Passed` (kept alongside for naive consumers).
    pub passed: bool,
    pub duration_ms: u64,
    /// Process/stream exit code (`null` for spawn failures/timeouts).
    pub exit_code: Option<i32>,
    pub turns: u32,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub total_tokens: u64,
    /// Engine-side session id when available.
    pub session_id: Option<String>,
    /// Assertion violations, oldest-first.
    pub violations: Vec<String>,
    /// Soft-rule observations (lenient `forbidden_tool` bans, RCA 2026-08-28
    /// §5 决策点 1): recorded for the report, never counted as failures —
    /// `passed`/`status` are computed before this field is populated.
    #[serde(default)]
    pub soft_flags: Vec<String>,
    /// Independent per-rule outcomes, verification order.
    pub rule_outcomes: Vec<RecordedRuleOutcome>,
    /// Observed trajectory (tool names in invocation order).
    pub trajectory_tools: Vec<String>,
    /// Cost/trajectory metrics from §4.7 (`None` only when nothing ran —
    /// spawn errors); serialized with every contracted field present.
    #[serde(default)]
    pub metrics: Option<TaskMetrics>,
    /// Winning failure class
    /// ([`FAILURE_CLASSES`](crate::testing::eval_metrics::FAILURE_CLASSES)-vocabulary) or `None` when
    /// passed or unclassified by the rule table.
    #[serde(default)]
    pub failure_class: Option<String>,
    /// Satisfied-rule evidence chain backing `failure_class`.
    #[serde(default)]
    pub failure_evidence: Vec<String>,
    /// W1 §2③ soft budget marker — `Some` only when a declared
    /// `limits.expected` was exceeded. Observationally recorded; never
    /// affects `passed` or [`RunStatus`].
    #[serde(default)]
    pub over_expected: Option<OverExpected>,
    /// W1 §4① per-task metadata anchor (model/provider/profile observed for
    /// THIS task's engine process). Unknown for spawn errors.
    #[serde(default)]
    pub anchor: RunAnchor,
    /// Absolute workspace path retained for postmortem inspection.
    pub workspace: PathBuf,
    /// Original TOML source path.
    pub task_file: PathBuf,
}

impl TaskRunRecord {
    /// Strip volatile fields (duration/workspace/ids) for cross-run digests.
    fn stable_view(&self) -> Value {
        json!({
            "id": self.id,
            "tier": self.tier,
            "horizon": self.horizon,
            "status": self.status.as_str(),
            "exit_code": self.exit_code,
            "turns": self.turns,
            "tokens_in": self.tokens_in,
            "tokens_out": self.tokens_out,
            "total_tokens": self.total_tokens,
            "violations": self.violations,
            "soft_flags": self.soft_flags,
            "rule_outcomes": self
                .rule_outcomes
                .iter()
                .map(|o| json!({"rule": o.rule, "passed": o.passed}))
                .collect::<Vec<_>>(),
            "trajectory_tools": self.trajectory_tools,
            "metrics": self.metrics.clone().map(|m| serde_json::to_value(&m).unwrap_or(Value::Null)),
            "failure_class": self.failure_class,
            "over_expected": self.over_expected,
            "anchor": self.anchor,
        })
    }
}

/// Complete suite report for one run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunReport {
    /// `<utcstamp>-<hex>` unique directory suffix.
    pub run_id: String,
    /// RFC3339 UTC start time (volatile — excluded from stable digest).
    pub started_at_utc: String,
    /// Whether this run rehearsed the pipeline via the stub executor.
    pub dry_run: bool,
    /// Engine binary designation recorded for provenance ("" for dry-run).
    pub shannon_bin: String,
    /// A/B arm marker: the experimental prompt directive in force, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directive: Option<String>,
    /// W1 §4① suite-level metadata anchor. Dry runs record
    /// `model_id = "dry-run-stub"`; real runs consolidate the per-task
    /// anchors, keeping a dimension only when every task agrees on it.
    #[serde(default)]
    pub anchors: RunAnchor,
    pub tasks_total: usize,
    pub tasks_passed: usize,
    pub tasks_failed: usize,
    pub tasks_limited: usize,
    pub tasks_timed_out: usize,
    pub tasks_spawn_errors: usize,
    /// Provenance of the per-task `metrics` blobs (§4.7):
    /// `events_jsonl` for real runs, `derived_stream` for dry-run rehearsals.
    #[serde(default)]
    pub metrics_source: String,
    /// Engine crate version that produced this run — a version-comparison
    /// anchor when diffing against the previous run.
    #[serde(default)]
    pub app_version: String,
    /// sha256 fingerprint of the failure-rule table that judged this run.
    #[serde(default)]
    pub failure_rules_fingerprint: String,
    pub records: Vec<TaskRunRecord>,
}

fn utc_now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Unique-enough run id: UTC compact stamp + short random hex. Two runs
/// started in the same second stay collision-free thanks to the entropy half.
fn fresh_run_id() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs();
    let hex = uuid::Uuid::new_v4().as_simple().to_string();
    format!(
        "{}-{}",
        chrono::DateTime::from_timestamp(secs as i64, 0)
            .expect("valid epoch seconds")
            .format("%Y%m%d%H%M%S"),
        &hex[..8]
    )
}

impl RunReport {
    /// Volatility-free canonical JSON: identical structure across repeated
    /// executions of unchanged code (drops timestamps, wall-clock durations,
    /// session ids and workspace/bin paths, keeps statuses, counters and
    /// violations). Serialized deterministically by construction of the
    /// input vectors. Model anchors (W1 §4①) and the soft `over_expected`
    /// markers (§2③) ride along so a model swap or a budget blowout can
    /// never digest as "unchanged".
    pub fn stable_digest(&self) -> String {
        let body = json!({
            "dry_run": self.dry_run,
            "anchors": self.anchors,
            "tasks_total": self.tasks_total,
            "tasks_passed": self.tasks_passed,
            "tasks_failed": self.tasks_failed,
            "tasks_limited": self.tasks_limited,
            "tasks_timed_out": self.tasks_timed_out,
            "tasks_spawn_errors": self.tasks_spawn_errors,
            "metrics_source": self.metrics_source,
            "app_version": self.app_version,
            "failure_rules_fingerprint": self.failure_rules_fingerprint,
            "directive_present": self.directive.is_some(),
            "records": self.records.iter().map(TaskRunRecord::stable_view).collect::<Vec<_>>(),
        });
        serde_json::to_string_pretty(&body).expect("digest serialization")
    }

    /// Tally of failure classes across records. Passing rows are omitted;
    /// failed rows no rule fired on land in the `unclassified` bucket (W1
    /// §6 阶段 1) so the rule table's blind spot stays measurable.
    pub fn failure_class_tally(&self) -> BTreeMap<&str, usize> {
        let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
        for record in &self.records {
            match &record.failure_class {
                Some(class) => *tally.entry(class.as_str()).or_default() += 1,
                None if !record.passed => {
                    *tally.entry(UNCLASSIFIED_CLASS).or_default() += 1;
                }
                None => {}
            }
        }
        tally
    }

    /// Human-readable companion of `report.json`.
    pub fn render_markdown(&self) -> String {
        let mode = if self.dry_run { "DRY-RUN" } else { "REAL" };
        let mut md = String::new();
        md.push_str(&format!(
            "# Shannon Eval Report `{}` ({mode})\n\n\
             - Started: {}\n\
             - Binary: {}\n\
             - Anchor: model={} · provider={} · profile={}\n\
             - Tasks: {} total · {} passed · {} failed · {} limited · {} timed out · {} spawn errors\n\n",
            self.run_id,
            self.started_at_utc,
            if self.shannon_bin.is_empty() { "-" } else { &self.shannon_bin },
            self.anchors.model_id.as_deref().unwrap_or("-"),
            self.anchors.provider.as_deref().unwrap_or("-"),
            self.anchors.profile_digest.as_deref().unwrap_or("-"),
            self.tasks_total,
            self.tasks_passed,
            self.tasks_failed,
            self.tasks_limited,
            self.tasks_timed_out,
            self.tasks_spawn_errors,
        ));

        md.push_str(
            "| tier | id | status | rules | turns | tokens (in/out) | budget | horizon | soft | ms |\n",
        );
        md.push_str("|---|---|---|---|---|---|---|---|---|---|\n");
        for record in &self.records {
            let rules_failed = record.rule_outcomes.iter().filter(|o| !o.passed).count();
            md.push_str(&format!(
                "| {} | {} | {} | {}/{} ok | {} | {}/{} | {} | {} | {} | {} |\n",
                record.tier,
                record.id,
                record.status.as_str(),
                record.rule_outcomes.len() - rules_failed,
                record.rule_outcomes.len(),
                record.turns,
                record.tokens_in,
                record.tokens_out,
                render_over_expected(record.over_expected),
                record.horizon,
                render_soft_flags(&record.soft_flags),
                record.duration_ms,
            ));
        }

        md.push_str("\n## Cost & trajectory matrix\n\n");
        md.push_str("| id | cost_usd | tok in/out | cache w/r | turns | tools | invalid | loops | perm denied | ms |\n");
        md.push_str("|---|---|---|---|---|---|---|---|---|---|\n");
        for record in &self.records {
            match &record.metrics {
                Some(metrics) => md.push_str(&format!(
                    "| {} | {} | {}/{} | {}/{} | {} | {} | {} | {} | {} | {} |\n",
                    record.id,
                    metrics
                        .cost_usd
                        .map_or_else(|| "null".into(), |c| format!("{c:.4}")),
                    metrics.tokens_in,
                    metrics.tokens_out,
                    metrics.cache_creation_tokens,
                    metrics.cache_read_tokens,
                    metrics.turns,
                    metrics.tool_calls,
                    metrics.invalid_calls,
                    metrics.loops,
                    metrics.permission_blocks,
                    metrics
                        .wall_clock_ms
                        .map_or_else(|| "null".into(), |ms| ms.to_string()),
                )),
                None => md.push_str(&format!(
                    "| {} | (no metrics — spawn failure) | | | | | | | | |\n",
                    record.id
                )),
            }
        }

        md.push_str("\n## Failure classification\n\n");
        if self.metrics_source == "derived_stream" {
            md.push_str(
                "- provenance: dry-run rehearsal — metrics derived from the NDJSON \
                 stream (`derived_stream`), not from an L0 log.\n",
            );
        } else {
            md.push_str("- provenance: per-task L0 `events.jsonl` (`events_jsonl`).\n");
        }
        let tally = self.failure_class_tally();
        if tally.is_empty() {
            md.push_str("- no failed tasks (nothing to classify).\n");
        } else {
            for (class, count) in tally {
                md.push_str(&format!("- {class}: {count}\n"));
            }
        }

        let broken: Vec<_> = self
            .records
            .iter()
            .filter(|r| r.status != RunStatus::Passed)
            .collect();
        if !broken.is_empty() {
            md.push_str("\n## Failures\n");
            for record in broken {
                md.push_str(&format!(
                    "\n### {} ({}) — {}\n",
                    record.id,
                    record.tier,
                    record.status.as_str()
                ));
                if let Some(class) = &record.failure_class {
                    md.push_str(&format!("- class: {class}\n"));
                    for evidence in &record.failure_evidence {
                        md.push_str(&format!("  - {evidence}\n"));
                    }
                }
                if record.violations.is_empty() {
                    md.push_str("- (guardrail breach only; see status)\n");
                }
                for violation in &record.violations {
                    md.push_str(&format!("- {violation}\n"));
                }
            }
        }
        md
    }
}

/// Markdown cell for the §2③ soft budget marker: `-` when within the
/// declared expectations, otherwise the exceeded multiples (`turn×2.5
/// tok×1.3`).
fn render_over_expected(over: Option<OverExpected>) -> String {
    let Some(over) = over else {
        return "-".to_string();
    };
    let mut parts = Vec::new();
    if let Some(multiple) = over.turns_multiple {
        parts.push(format!("turn×{multiple:.1}"));
    }
    if let Some(multiple) = over.tokens_multiple {
        parts.push(format!("tok×{multiple:.1}"));
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join(" ")
    }
}

/// Markdown cell for soft-rule observations: `-` when none, otherwise the
/// flag texts joined with `; `. Purely informational — soft flags never
/// contribute to the `rules x/y ok` tally or the run status.
fn render_soft_flags(flags: &[String]) -> String {
    if flags.is_empty() {
        "-".to_string()
    } else {
        flags.join("; ")
    }
}

// ── Execution ──────────────────────────────────────────────────────────

/// Knobs controlling one suite execution.
#[derive(Debug, Clone)]
pub struct EvalOptions {
    /// Engine binary for real runs. Resolution order when left as `None`:
    /// `SHANNON_EVAL_BIN` env → `target/debug/shannon` beside this crate
    /// (manifest-relative) → bare `shannon` on `PATH`.
    pub bin_path: Option<PathBuf>,
    /// Rehearse via the deterministic stub executor instead of spawning the
    /// engine (no API key needed; exercises the full pipeline honestly).
    pub dry_run: bool,
    /// Force the output root instead of `$SHANNON_HOME`/`~/.shannon`.
    /// (Used by tests to redirect artifacts into a temp dir.)
    pub out_dir_override: Option<PathBuf>,
    /// Explicit failure-rule table override; resolution order in
    /// [`run_suite`] is this path → `SHANNON_EVAL_FAILURE_RULES` env → the
    /// embedded default table.
    pub failure_rules: Option<PathBuf>,
    /// Experimental directive appended to every task prompt (A/B harness for
    /// separating strategy differences from capability differences). `None`
    /// leaves task prompts byte-identical to their TOML definitions; the
    /// active value is stamped into the report's top-level `directive` field
    /// so `eval-diff` and the dashboard can tell the arms apart.
    pub instruction_directive: Option<String>,
}

impl Default for EvalOptions {
    fn default() -> Self {
        Self {
            bin_path: None,
            dry_run: true,
            out_dir_override: None,
            failure_rules: None,
            instruction_directive: None,
        }
    }
}

/// Resolve the active [`FailureRules`]: explicit option, then the
/// `SHANNON_EVAL_FAILURE_RULES` environment override, then the embedded
/// default table shipped with the crate.
pub fn resolve_failure_rules(explicit: Option<&Path>) -> Result<FailureRules, EvalError> {
    if let Some(path) = explicit {
        return FailureRules::load(path);
    }
    if let Ok(env_path) = std::env::var("SHANNON_EVAL_FAILURE_RULES") {
        return FailureRules::load(Path::new(&env_path));
    }
    Ok(FailureRules::embedded())
}

/// Resolve the engine binary designation for a real run.
pub fn resolve_bin(explicit: Option<&Path>) -> Result<PathBuf, EvalError> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    if let Ok(env_path) = std::env::var("SHANNON_EVAL_BIN") {
        return Ok(PathBuf::from(env_path));
    }
    let manifest_debug = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/shannon")
        .components()
        .collect::<PathBuf>();
    if manifest_debug.exists() {
        return Ok(manifest_debug);
    }
    Ok(PathBuf::from("shannon"))
}

/// Execute the whole declared suite; returns the report and the run directory
/// holding `report.json`/`report.md` plus per-task evidence directories.
pub fn run_suite(
    tasks: &[EvalTask],
    options: &EvalOptions,
) -> Result<(RunReport, PathBuf), EvalError> {
    let out_root = match &options.out_dir_override {
        Some(dir) => dir.clone(),
        None => resolve_eval_home()?.join("eval").join("runs"),
    };
    let rules = resolve_failure_rules(options.failure_rules.as_deref())?;
    let run_dir = out_root.join(fresh_run_id());
    std::fs::create_dir_all(&run_dir)?;

    let started_at = utc_now_rfc3339();
    let mut records = Vec::with_capacity(tasks.len());

    for task in tasks {
        let mut record = run_task(task, options, &run_dir);
        enrich_with_metrics(&mut record, task, options, &run_dir, &rules);
        persist_task_artifacts(&record, run_dir.join(&task.id));
        let _ = std::fs::write(
            run_dir.join(&task.id).join("result.json"),
            serde_json::to_vec_pretty(&record).expect("result.json serialization"),
        );
        records.push(record);
    }

    let tally = |want: fn(&TaskRunRecord) -> bool| records.iter().filter(|r| want(r)).count();
    let report = RunReport {
        run_id: run_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned(),
        started_at_utc: started_at,
        dry_run: options.dry_run,
        shannon_bin: options
            .bin_path
            .clone()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        anchors: suite_anchor(&records, options.dry_run),
        tasks_total: records.len(),
        tasks_passed: tally(|r| r.status == RunStatus::Passed),
        tasks_failed: tally(|r| r.status == RunStatus::Failed),
        tasks_limited: tally(|r| matches!(r.status, RunStatus::TurnLimit | RunStatus::TokenLimit)),
        tasks_timed_out: tally(|r| r.status == RunStatus::Timeout),
        tasks_spawn_errors: tally(|r| r.status == RunStatus::SpawnError),
        metrics_source: primary_metrics_source(&records),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        failure_rules_fingerprint: rules.fingerprint().to_string(),
        directive: options.instruction_directive.clone(),
        records,
    };

    std::fs::write(
        run_dir.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    std::fs::write(run_dir.join("report.md"), report.render_markdown())?;

    Ok((report, run_dir))
}

/// The provenance stamp for the suite: real L0 logs dominate; mixed suites
/// (real spawn error rows fall back to `None` metrics) still advertise the
/// strongest source that actually contributed data.
fn primary_metrics_source(records: &[TaskRunRecord]) -> String {
    if records.iter().any(|r| {
        r.metrics
            .iter()
            .any(|m| m.source == MetricSource::EventsLog)
    }) {
        MetricSource::EventsLog.as_str().to_string()
    } else if records.iter().any(|r| {
        r.metrics
            .iter()
            .any(|m| m.source == MetricSource::DerivedStream)
    }) {
        MetricSource::DerivedStream.as_str().to_string()
    } else {
        "none".to_string()
    }
}

/// Consolidate the per-task anchors into the suite-level anchor (W1 §4①).
/// Dry runs record the constant stub marker; real runs keep a dimension
/// only when every contributing task observed the same value — any
/// disagreement dissolves to `None` (honestly unknown) instead of picking a
/// winner, so `eval-diff` cannot bless a mixed-model comparison.
fn suite_anchor(records: &[TaskRunRecord], dry_run: bool) -> RunAnchor {
    if dry_run {
        return RunAnchor {
            model_id: Some(DRY_RUN_ANCHOR_MODEL.to_string()),
            provider: None,
            profile_digest: None,
        };
    }
    let consensus = |pick: fn(&RunAnchor) -> &Option<String>| -> Option<String> {
        let mut seen: Option<String> = None;
        let mut conflicted = false;
        for record in records {
            let Some(value) = pick(&record.anchor) else {
                continue;
            };
            match &seen {
                None => seen = Some(value.clone()),
                Some(previous) if previous != value => conflicted = true,
                Some(_) => {}
            }
        }
        (!conflicted).then_some(seen).flatten()
    };
    RunAnchor {
        model_id: consensus(|a| &a.model_id),
        provider: consensus(|a| &a.provider),
        profile_digest: consensus(|a| &a.profile_digest),
    }
}

/// §4.7 enrichment pass: extract metrics (real log when present, honest
/// stream derivation otherwise), classify failures by rule table, and archive
/// failing real-run samples with their classification evidence.
fn enrich_with_metrics(
    record: &mut TaskRunRecord,
    task: &EvalTask,
    options: &EvalOptions,
    run_dir: &Path,
    rules: &FailureRules,
) {
    if record.status == RunStatus::SpawnError {
        return; // nothing ran; metrics stay absent rather than fabricated
    }

    let task_dir = run_dir.join(&task.id);
    let stream_path = task_dir.join("stream.ndjson");
    let stream_text = std::fs::read_to_string(&stream_path).unwrap_or_default();
    let mut extracted: ExtractedTask = if !options.dry_run {
        let l0_home = task_dir.join(L0_HOME_DIRNAME);
        let logs = find_event_logs(&l0_home);
        match extract_from_events_log(&logs) {
            Ok(extracted) if !logs.is_empty() => extracted,
            _ => derive_from_stream(&stream_text),
        }
    } else {
        derive_from_stream(&stream_text)
    };

    // The engine's L0 vocabulary turn is the whole user-visible round: one
    // `turn/start` per query, every row of the run stamped `turn = 1`. The
    // envelope turn max is therefore structurally 1 for a headless run no
    // matter how many LLM steps the agent took. Reconcile with the stream's
    // `done.turns_used` — the step count `--max-turns` actually bounds — so
    // the reported turns statistic is the real value, not a constant.
    if extracted.metrics.source == MetricSource::EventsLog {
        if let Some(NdjsonLine::Done {
            turns_used: Some(stream_turns),
            ..
        }) = observe_stream(&stream_text).done
        {
            extracted.metrics.turns = extracted.metrics.turns.max(stream_turns);
        }
    }

    // Classification context merges runner verdicts with extractor signals.
    let context = ClassifyContext {
        status: record.status.as_str().to_string(),
        passed: record.passed,
        violations: record.violations.len() as u32,
        metrics: extracted.metrics.clone(),
        signals: extracted.signals.clone(),
    };
    let classification = rules.classify(&context);

    // Failure-sample archive (real runs only — archiving needs a genuine log;
    // dry runs keep their derived numbers but no L0 artifacts exist to store).
    let archivable_log = extracted.metrics.source == MetricSource::EventsLog;

    record.metrics = Some(extracted.metrics);
    record.failure_class = classification.as_ref().map(|c| c.class.clone());
    record.failure_evidence = classification
        .as_ref()
        .map(|c| c.evidence.clone())
        .unwrap_or_default();

    if !record.passed && archivable_log {
        archive_failure_sample(record, task, &task_dir, &classification, options);
    }
}

/// Directory name (inside each task's evidence dir) of the isolated
/// `SHANNON_HOME` handed to the spawned engine so its tee writes the task's
/// own `events.jsonl`.
pub const L0_HOME_DIRNAME: &str = "l0-home";

/// Root of the failure-sample archive: `<home>/eval/failures/<date>/<task>/`
/// where the override mirrors the runs-root convention (override's parent).
fn failure_archive_root(options: &EvalOptions) -> PathBuf {
    match &options.out_dir_override {
        Some(dir) => dir.parent().unwrap_or(dir).join("eval").join("failures"),
        None => resolve_eval_home()
            .map(|home| home.join("eval").join("failures"))
            .unwrap_or_else(|_| std::env::temp_dir().join("shannon-eval-failures")),
    }
}

/// Copy the task's L0 logs plus its classification verdict (including the
/// W1 §7③ minimal-reproduction recipe) into
/// `$SHANNON_HOME/eval/failures/<yyyymmdd>/<task_id>/`.
fn archive_failure_sample(
    record: &TaskRunRecord,
    task: &EvalTask,
    task_dir: &Path,
    classification: &Option<Classification>,
    options: &EvalOptions,
) {
    let date = chrono::Utc::now().format("%Y%m%d");
    let dest_root = failure_archive_root(options).join(date.to_string());
    let dest = dest_root.join(&record.id);
    if std::fs::create_dir_all(&dest).is_err() {
        return; // archival must never fail a run
    }
    for log in find_event_logs(&task_dir.join(L0_HOME_DIRNAME)) {
        if let Some(session) = log.parent().and_then(Path::file_name) {
            // Sanitized copy name; sessions are UUIDs in practice.
            let name = session.to_string_lossy().replace(['"', '\\', '/'], "");
            let _ = std::fs::copy(log, dest.join(format!("events-{name}.jsonl")));
        }
    }
    let verdict = json!({
        "task": record.id,
        "status": record.status.as_str(),
        "failure_class": record.failure_class,
        "evidence": record.failure_evidence,
        "classification": classification.as_ref().map(|c| serde_json::to_value(c).ok()),
        "metrics": record.metrics,
        "workspace": record.workspace,
        // W1 §7③: the minimal engine invocation that produced this sample,
        // so the archived evidence doubles as a one-shot replay recipe
        // (engine arguments + workspace path; identical to what the runner
        // actually drove, directive fencing included).
        "repro": repro_view(task, options, &record.workspace),
    });
    let _ = std::fs::write(
        dest.join("classification.json"),
        serde_json::to_vec_pretty(&verdict).expect("classification serialization"),
    );
}

/// The minimal-reproduction recipe for one real-run failure: the exact
/// engine binary designation, argv, and workspace the runner used. `args`
/// is the authoritative form (no shell quoting involved); `command` is a
/// convenience rendering for terminals.
fn repro_view(task: &EvalTask, options: &EvalOptions, workspace: &Path) -> Value {
    let limits = task.resolved_limits();
    let engine = resolve_bin(options.bin_path.as_deref())
        .map(|bin| bin.display().to_string())
        .unwrap_or_else(|_| "shannon".to_string());
    // Byte-identical to what spawn_engine sends, directive fencing included.
    let prompt = match &options.instruction_directive {
        Some(directive) => format!("{}\n\n【实验指令】{directive}", task.prompt),
        None => task.prompt.clone(),
    };
    let args = json!([
        "--prompt",
        prompt,
        "--output-format",
        "json-stream",
        "--max-turns",
        limits.max_turns.to_string(),
    ]);
    let rendered_args = args
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| item.as_str().unwrap_or_default().to_string())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    json!({
        "engine": engine,
        "args": args,
        "command": format!("{engine} {rendered_args}"),
        "workspace": workspace,
    })
}

/// Persist the enriched metrics/classification alongside the raw stream in
/// the task evidence dir (`metrics.json`), keeping result.json the runner row.
fn persist_task_artifacts(record: &TaskRunRecord, task_dir: PathBuf) {
    let payload = json!({
        "metrics": record.metrics,
        "metrics_complete": record.metrics.as_ref().map(|m| missing_fields(m).is_empty()),
        "failure_class": record.failure_class,
        "failure_evidence": record.failure_evidence,
    });
    let _ = std::fs::write(
        task_dir.join("metrics.json"),
        serde_json::to_vec_pretty(&payload).expect("metrics.json serialization"),
    );
}

/// Prepare the sandboxed workspace under `<run_dir>/<task_id>/workspace`:
/// seed files (parents auto-created), optional `git init`, and persist a
/// byte-exact copy of the task definition beside it for audits.
pub fn prepare_task_dir(task: &EvalTask, run_dir: &Path) -> Result<PathBuf, EvalError> {
    let task_dir = run_dir.join(&task.id);
    let workspace = task_dir.join("workspace");
    std::fs::create_dir_all(&workspace)?;

    for file in &task.setup.files {
        let target = workspace.join(&file.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, &file.content)?;
    }

    if task.setup.git_init {
        let status = Command::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(&workspace)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            return Err(EvalError::Config(format!(
                "task '{}': git init failed in workspace",
                task.id
            )));
        }
    }

    if let Some(source) = &task.task_source_path_hint {
        let _ = std::fs::copy(source, task_dir.join("task.toml"));
    }
    Ok(workspace)
}

/// Execute one task: prepare workspace → produce stream (stub or engine
/// subprocess) → classify limits → verify → persist artifacts.
pub fn run_task(task: &EvalTask, options: &EvalOptions, run_dir: &Path) -> TaskRunRecord {
    let started = Instant::now();
    let limits = task.resolved_limits();

    let prepare = prepare_task_dir(task, run_dir);
    let workspace = match prepare {
        Ok(ws) => ws,
        Err(e) => {
            return base_record(
                task,
                RunStatus::SpawnError,
                started.elapsed(),
                None,
                vec![format!("workspace preparation failed: {e}")],
                PathBuf::new(),
            );
        }
    };

    // Produce the raw NDJSON stream + true process exit code.
    let produced = if options.dry_run {
        execute_stub(task, &workspace)
    } else {
        let bin = match resolve_bin(options.bin_path.as_deref()) {
            Ok(bin) => bin,
            Err(e) => {
                return base_record(
                    task,
                    RunStatus::SpawnError,
                    started.elapsed(),
                    None,
                    vec![format!("binary resolution failed: {e}")],
                    workspace.clone(),
                );
            }
        };
        spawn_engine(
            &bin,
            task,
            &workspace,
            &run_dir.join(&task.id).join(L0_HOME_DIRNAME),
            limits.max_turns,
            limits.timeout_secs,
            options.instruction_directive.as_deref(),
        )
    };

    // Persist the captured stream beside the workspace as forensic evidence.
    let stream_path = run_dir.join(&task.id).join("stream.ndjson");
    let _ = std::fs::write(&stream_path, &produced.ndjson);

    let l0_events = if !options.dry_run {
        let logs = find_event_logs(&run_dir.join(&task.id).join(L0_HOME_DIRNAME));
        let mut evts = Vec::new();
        for path in &logs {
            if let Ok(reader) = crate::session_log::SessionLogReader::open(path) {
                if let Ok(vs) = reader.read_events(false) {
                    evts.extend(vs);
                }
            }
        }
        (!evts.is_empty()).then_some(evts)
    } else {
        None
    };
    let mut record = finalize_record(
        task,
        limits,
        started,
        workspace,
        produced,
        l0_events.as_deref(),
    );
    if options.dry_run {
        // The stub never talks to a provider: stamp the constant marker so
        // a dry report can never masquerade as a model run (W1 §4①).
        record.anchor = RunAnchor {
            model_id: Some(DRY_RUN_ANCHOR_MODEL.to_string()),
            provider: None,
            profile_digest: None,
        };
    }
    record
}

/// Raw stream plus child-process facts returned by either executor.
struct ProducedStream {
    ndjson: String,
    exit_code: Option<i32>,
    launch_error: Option<String>,
    timed_out: bool,
}

/// Map engine exit codes to guardrail statuses. Engine-specific soft
/// conditions (rate limit / context overflow / permission denial) land in
/// `Failed` with the code spelled out — they describe *why* the run aborted,
/// not a budget overrun of the harness itself.
fn classify_exit(exit_code: i32) -> RunStatus {
    match exit_code {
        0 => RunStatus::Passed,
        2 => RunStatus::TurnLimit,
        3 => RunStatus::Timeout,
        _ => RunStatus::Failed,
    }
}

/// Assertion set for one task: the declared `verify.rules` plus the
/// expectations folded into the same [`ValidationRule`] vocabulary. The
/// trajectory template folds in as a hard `trajectory_contains`; the
/// forbidden-tool bans fold in with tier-dependent strictness — **strict in
/// the recovery tier** (the recovery contract is precisely "recover without
/// the banned shortcut"), **soft everywhere else** (RCA 2026-08-28 §5
/// 决策点 1/2: outside recovery a Bash/Write detour is a means to the
/// verified outcome, not the contract — a hit is flagged, never fatal).
fn effective_rules_for(task: &EvalTask) -> Vec<ValidationRule> {
    let mut rules: Vec<ValidationRule> = task.verify.rules.clone();
    if !task.expectations.trajectory.is_empty() {
        rules.push(ValidationRule::TrajectoryContains {
            sequence: task.expectations.trajectory.clone(),
        });
    }
    let strict = task.tier == EvalTier::Recovery;
    for banned in &task.expectations.forbidden_tools {
        rules.push(ValidationRule::ForbiddenTool {
            tool: banned.clone(),
            strict,
        });
    }
    rules
}

/// Common decision path shared by stub and real streams: derive the status
/// (limit classes take precedence, in ascending harshness: token < turn <
/// wall-clock), gather evidence, then evaluate assertions (always, so the
/// retained scene documents *why* a run died, even under guardrail breach).
fn finalize_record(
    task: &EvalTask,
    limits: ResolvedLimits,
    started: Instant,
    workspace: PathBuf,
    produced: ProducedStream,
    l0_events: Option<&[shannon_types::session_event::SessionEvent]>,
) -> TaskRunRecord {
    if let Some(error) = produced.launch_error {
        return base_record(
            task,
            RunStatus::SpawnError,
            started.elapsed(),
            None,
            vec![error],
            workspace,
        );
    }

    let observation = observe_stream(&produced.ndjson);
    let mut violations: Vec<String> = observation.errors.to_vec();

    // Account for budget guards.
    let done = match &observation.done {
        Some(NdjsonLine::Done {
            exit_code,
            turns_used,
            tokens_used,
            tokens_in,
            tokens_out,
            ..
        }) => (
            *exit_code,
            *turns_used,
            (*tokens_in).unwrap_or(0),
            (*tokens_out).unwrap_or(0),
            tokens_used.map_or_else(
                || (*tokens_in).unwrap_or(0) + (*tokens_out).unwrap_or(0),
                |t| t,
            ),
        ),
        _ => {
            violations.push("engine stream ended without a `done` event".to_string());
            (Some(produced.exit_code.unwrap_or(-1)), None, 0, 0, 0)
        }
    };
    let (stream_exit, stream_turns, tokens_in, tokens_out, total_tokens) = done;

    // Status: process/exit signal first, then token ceiling, then any limit hit.
    let exit_for_classification = stream_exit.or(produced.exit_code).unwrap_or(-1);
    let mut status = if produced.timed_out {
        RunStatus::Timeout
    } else {
        classify_exit(exit_for_classification)
    };

    if !produced.timed_out && status != RunStatus::SpawnError {
        if total_tokens > limits.max_tokens {
            violations.push(format!(
                "token limit: {} exceeds budget {}",
                total_tokens, limits.max_tokens
            ));
            status = RunStatus::TokenLimit;
        } else if let Some(turns) = stream_turns {
            if turns > limits.max_turns {
                violations.push(format!(
                    "turn limit: {} exceeds budget {}",
                    turns, limits.max_turns
                ));
                status = RunStatus::TurnLimit;
            }
        }
    }

    // Verification runs regardless of class — diagnosis evidence is valuable
    // precisely on breaches. `initial_files` feeds `diff_matches` baselines.
    let setup_pairs: Vec<(String, String)> = task
        .setup
        .files
        .iter()
        .map(|f| (f.path.clone(), f.content.clone()))
        .collect();

    let effective_rules = effective_rules_for(task);

    let answer = observation.answer_text.clone();
    let exit_word = if exit_for_classification == 0 {
        "success"
    } else {
        "error"
    };
    // Prefer the L0 log's trajectory when available; the stream-side
    // observation is only a fallback for dry runs / spawn-lost logs.
    let trajectory_owned = l0_events
        .filter(|e| !e.is_empty())
        .map(l0_trajectory)
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| observation.trajectory.clone());
    let ctx = ValidationContext::new(&workspace, exit_word, &answer)
        .with_initial_files(&setup_pairs)
        .with_trajectory(&trajectory_owned);

    let outcomes = evaluate_rules(&effective_rules, &ctx);
    let mut recorded: Vec<RecordedRuleOutcome> =
        outcomes.iter().map(RecordedRuleOutcome::from).collect();
    for outcome in &outcomes {
        violations.extend(outcome.details.iter().cloned());
    }
    // Soft-rule observations ride along untouched by the failure list —
    // visible in the report, never verdict-changing (RCA §5 决策点 1).
    let soft_flags: Vec<String> = outcomes
        .iter()
        .flat_map(|outcome| outcome.soft_flags.iter().cloned())
        .collect();
    // Shell-script verification (unix workers only).
    if !task.verify.script.trim().is_empty() {
        let script_result = run_verify_script(&task.verify.script, &workspace);
        let failure = script_result.err();
        let details: Vec<String> = failure.iter().cloned().collect();
        violations.extend(details.iter().cloned());
        recorded.push(RecordedRuleOutcome {
            rule: "verify_script".to_string(),
            passed: failure.is_none(),
            details,
            soft_flags: Vec::new(),
        });
    }

    // A limit-breach status is final even when assertions happen to pass;
    // otherwise anything other than a zero exit is a failure by contract.
    let passed = status == RunStatus::Passed && violations.is_empty();
    if passed {
        status = RunStatus::Passed;
    } else if status == RunStatus::Passed {
        status = RunStatus::Failed;
    }

    let fallback_turns = stream_turns_from_trajectory(&observation);
    // W1 §4①: prefer the L0 request header (wire-honest model); the stream
    // start banner's model is the fallback when no header/log exists.
    let mut anchor = l0_events.map(extract_anchor).unwrap_or_default();
    if anchor.model_id.is_none() {
        anchor.model_id = observation.model.clone();
    }
    let turns_final = stream_turns.unwrap_or(fallback_turns);
    TaskRunRecord {
        id: task.id.clone(),
        tier: task.tier.as_str().to_string(),
        horizon: task.effective_horizon().as_str().to_string(),
        passed,
        status,
        duration_ms: started.elapsed().as_millis() as u64,
        exit_code: produced.exit_code,
        turns: turns_final,
        tokens_in,
        tokens_out,
        total_tokens,
        session_id: observation.session_id.clone(),
        violations,
        soft_flags,
        rule_outcomes: recorded,
        trajectory_tools: observation
            .trajectory
            .iter()
            .map(|call| call.tool.clone())
            .collect(),
        // Enriched by §4.7 right after this constructor returns.
        metrics: None,
        failure_class: None,
        failure_evidence: Vec::new(),
        // W1 §2③: observational only — presence here never flips `passed`.
        over_expected: OverExpected::of(turns_final, total_tokens, task.limits.expected),
        anchor,
        workspace,
        task_file: task.task_source_path_hint.clone().unwrap_or_default(),
    }
}

/// L0 as the authoritative trajectory source (§4.1 tool/call rows carry the
/// raw, untruncated model arguments — the NDJSON stream summary can lose
/// call-side lines, so rules only fall back to it when no L0 log exists).
fn l0_trajectory(events: &[shannon_types::session_event::SessionEvent]) -> Vec<ToolCallTrace> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            shannon_types::session_event::SessionEventBody::ToolCall(p) => {
                Some(ToolCallTrace::new(&p.tool_name, p.arguments.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Derived turns when the engine did not volunteer them (defensive; the
/// contract always sends `turns_used`).
fn stream_turns_from_trajectory(observation: &RunObservation) -> u32 {
    observation.trajectory.len() as u32
}

#[allow(clippy::too_many_arguments)]
fn base_record(
    task: &EvalTask,
    status: RunStatus,
    elapsed: Duration,
    exit_code: Option<i32>,
    violations: Vec<String>,
    workspace: PathBuf,
) -> TaskRunRecord {
    TaskRunRecord {
        id: task.id.clone(),
        tier: task.tier.as_str().to_string(),
        horizon: task.effective_horizon().as_str().to_string(),
        passed: status == RunStatus::Passed,
        status,
        duration_ms: elapsed.as_millis() as u64,
        exit_code,
        turns: 0,
        tokens_in: 0,
        tokens_out: 0,
        total_tokens: 0,
        session_id: None,
        violations,
        soft_flags: Vec::new(),
        rule_outcomes: Vec::new(),
        trajectory_tools: Vec::new(),
        metrics: None,
        failure_class: None,
        failure_evidence: Vec::new(),
        over_expected: None,
        anchor: RunAnchor::default(),
        workspace,
        task_file: task.task_source_path_hint.clone().unwrap_or_default(),
    }
}

/// Spawn the engine child with the guardrail flags and collect its stdout
/// NDJSON under a wall-clock deadline (polling `try_wait`; hard-kills on
/// breach and keeps whatever partial stream was captured). On unix the child
/// gets its own process group so a timeout kill takes down any grandchildren
/// that would otherwise keep the stdout pipe open.
fn spawn_engine(
    bin: &Path,
    task: &EvalTask,
    workspace: &Path,
    l0_home: &Path,
    max_turns: u32,
    timeout_secs: u64,
    directive: Option<&str>,
) -> ProducedStream {
    // The directive rides at the END of the user prompt, clearly fenced, so
    // the task text itself stays byte-stable across arms.
    let prompt = match directive {
        Some(d) => format!("{}\n\n【实验指令】{}", task.prompt, d),
        None => task.prompt.clone(),
    };
    let mut command = Command::new(bin);
    command
        .arg("--prompt")
        .arg(&prompt)
        .arg("--output-format")
        .arg("json-stream")
        .arg("--max-turns")
        .arg(max_turns.to_string())
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // Isolate persisted sessions into the task's own evidence directory
        // (headless honors `SHANNON_SESSIONS_DIR`; see cli main.rs).
        .env("SHANNON_SESSIONS_DIR", workspace.join(".eval-sessions"))
        // §4.7: a task-private SHANNON_HOME routes the §4.2 tee's L0 log to
        // `<task_dir>/l0-home/sessions/<session_id>/events.jsonl`, giving
        // metric extraction and failure archiving a genuine per-task source.
        // `SHANNON_HOME` is consumed only by the session-log subsystem, so
        // config/provider resolution is untouched.
        .env("SHANNON_HOME", l0_home);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Failure to set a fresh process group is non-fatal — fall back to
        // plain kill semantics below.
        let _ = command.process_group(0);
    }

    let spawned = command.spawn();

    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            return ProducedStream {
                ndjson: String::new(),
                exit_code: None,
                launch_error: Some(format!("failed to spawn {}: {e}", bin.display())),
                timed_out: false,
            };
        }
    };

    let stdout = child.stdout.take().expect("child stdout was piped");
    let reader = thread::spawn(move || {
        let mut collected = String::new();
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(text) => {
                    collected.push_str(&text);
                    collected.push('\n');
                }
                Err(_) => break,
            }
        }
        collected
    });

    let deadline = Duration::from_secs(timeout_secs);
    let began = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let exit_code = status.code();
                let ndjson = reader.join().unwrap_or_default();
                return ProducedStream {
                    ndjson,
                    exit_code,
                    launch_error: None,
                    timed_out: false,
                };
            }
            Ok(None) if began.elapsed() >= deadline => {
                // Wall-clock budget exhausted: hard-kill and keep the partial
                // stream that made it through before death. On unix the whole
                // process group dies so no grandchild keeps the pipe open.
                kill_process_tree(&mut child);
                let _ = child.wait();
                let ndjson = reader.join().unwrap_or_default();
                return ProducedStream {
                    ndjson,
                    exit_code: None,
                    launch_error: None,
                    timed_out: true,
                };
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                let ndjson = reader.join().unwrap_or_default();
                return ProducedStream {
                    ndjson,
                    exit_code: None,
                    launch_error: Some(format!("wait on child failed: {e}")),
                    timed_out: false,
                };
            }
        }
    }
}

/// Hard-kill the timed-out child; on unix, SIGKILL the entire process group
/// (the child was spawned with `process_group(0)`) so descendants cannot keep
/// the stdout pipe — and therefore the stream collection thread — alive past
/// the wall-clock budget.
#[cfg(unix)]
fn kill_process_tree(child: &mut std::process::Child) {
    let negated_pid = -(child.id() as i32);
    // SAFETY: kill(2) with SIGKILL on this runner's own spawned process group;
    // ESRCH for an already-dead group is an acceptable non-error.
    let group_signalled = unsafe { libc::kill(negated_pid, libc::SIGKILL) } == 0;
    if !group_signalled {
        // Group already gone or not set: fall back to direct-child kill.
        let _ = child.kill();
    }
}

/// Non-unix builds fall back to terminating the direct child.
#[cfg(not(unix))]
fn kill_process_tree(child: &mut std::process::Child) {
    let _ = child.kill();
}

/// Execute `verify.script` inside the workspace via `sh -c`; captures combined
/// output tails (up to 2000 chars) into the violation message on failure.
#[cfg(unix)]
fn run_verify_script(script: &str, workspace: &Path) -> Result<(), String> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(workspace)
        .output()
        .map_err(|e| format!("verify_script could not run: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let mut tail = String::from_utf8_lossy(&output.stderr).to_string();
    if tail.trim().is_empty() {
        tail = String::from_utf8_lossy(&output.stdout).to_string();
    }
    if tail.chars().count() > 2000 {
        tail = tail.chars().take(2000).collect();
    }
    Err(format!(
        "verify_script exited with {} (cwd={})\n{tail}",
        output.status.code().unwrap_or(-1),
        workspace.display()
    ))
}

/// Windows builds get an explicit dead-end rather than silent skips.
#[cfg(not(unix))]
fn run_verify_script(_script: &str, _workspace: &Path) -> Result<(), String> {
    Err("verify_script is unsupported on this platform".to_string())
}

// ── Dry-run stub executor ──────────────────────────────────────────────

/// Deterministic rehearsal: interpret the task's `[dry_run]` steps against the
/// real workspace via mini-primitives and emit an engine-schema-faithful
/// NDJSON stream (`start` → per-step `tool_call`/`tool_result` → final
/// `text_delta` → `done`). The verify stage consumes this exactly as it would
/// consume a live engine's stream.
fn execute_stub(task: &EvalTask, workspace: &Path) -> ProducedStream {
    let mut stream = String::new();
    push_line(
        &mut stream,
        &json!({
            "type": "start",
            "prompt": task.prompt,
            "model": "eval-dry-run-stub",
            "session_id": format!("dryrun-{}", uuid::Uuid::new_v4().as_simple()),
        }),
    );

    // Synthesized billing keeps the token-budget plumbing exercised; roughly
    // four characters per token with a fixed per-step overhead.
    let prompt_tokens = (task.prompt.len() as u64).div_ceil(4) + 24;
    let mut tokens_in: u64 = prompt_tokens;
    let mut tokens_out: u64 = 0;

    for step in &task.dry_run.steps {
        let rendered_input = render_tool_input(&step.input);
        let effect = if step.fail {
            Err("synthetic failure injected by dry-run script".to_string())
        } else {
            apply_stub_tool(step.tool.as_str(), &step.input, workspace)
        };
        tokens_in += 32;

        let (success, output_summary) = match effect {
            Ok(result) => (true, result),
            Err(reason) => (false, reason),
        };

        push_line(
            &mut stream,
            &json!({
                "type": "tool_call",
                "name": step.tool,
                "input": rendered_input,
            }),
        );
        push_line(
            &mut stream,
            &json!({
                "type": "tool_result",
                "name": step.tool,
                "success": success,
                "output": output_summary,
            }),
        );
        tokens_out += output_summary.len() as u64 / 8 + 16;
    }

    push_line(
        &mut stream,
        &json!({ "type": "text_delta", "content": task.dry_run.final_text }),
    );

    let mut tokens_used = tokens_in + tokens_out + 12;
    if let Some(forced) = task.dry_run.tokens_used {
        tokens_used = forced;
    }
    push_line(
        &mut stream,
        &json!({
            "type": "done",
            "exit_code": 0,
            "turns_used": task.dry_run.steps.len() as u32 + 1,
            "tokens_used": tokens_used,
            "tokens_in": tokens_in,
            "tokens_out": tokens_out,
        }),
    );

    ProducedStream {
        ndjson: stream,
        exit_code: Some(0),
        launch_error: None,
        timed_out: false,
    }
}

fn push_line(buf: &mut String, value: &Value) {
    buf.push_str(&value.to_string());
    buf.push('\n');
}

/// Compact-JSON rendering of a step's arguments — mirrors what the engine
/// puts in `tool_call.input`, so `args_regex` patterns behave identically.
fn render_tool_input(input: &Value) -> Value {
    match serde_json::from_str::<Value>(&input.to_string()) {
        Ok(parsed) => parsed,
        Err(_) => input.clone(),
    }
}

fn workspace_file(workspace: &Path, raw: &str) -> PathBuf {
    // Absolute paths flow through unchanged; anything else resolves inside
    // the sandbox so stub effects can never escape the task directory.
    let candidate = Path::new(raw);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        workspace.join(candidate)
    }
}

/// Apply one stub step's effect on the actual workspace. Implements the
/// primitives the suite relies upon (`Read`, `Write`, `Edit`, `MultiEdit`,
/// `Glob`, `Grep`, narrow `Bash`); unknown tools degrade to an error result
/// rather than pretending success.
fn apply_stub_tool(tool: &str, input: &Value, workspace: &Path) -> Result<String, String> {
    let arg_str = |key: &str| input.get(key).and_then(Value::as_str);

    match tool {
        "Read" => {
            let path = arg_str("file_path").ok_or("Read: missing file_path")?;
            std::fs::read_to_string(workspace_file(workspace, path))
                .map_err(|_| format!("Read: {path} does not exist"))
        }
        "Write" => {
            let path = arg_str("file_path").ok_or("Write: missing file_path")?;
            let content = arg_str("content").ok_or("Write: missing content")?;
            let target = workspace_file(workspace, path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Write: cannot create parent: {e}"))?;
            }
            std::fs::write(&target, content).map_err(|e| format!("Write: {e}"))?;
            Ok(format!("{path}: written ({} bytes)", content.len()))
        }
        "Edit" => {
            let path = arg_str("file_path").ok_or("Edit: missing file_path")?;
            let old = arg_str("old_string").ok_or("Edit: missing old_string")?;
            let new = arg_str("new_string").ok_or("Edit: missing new_string")?;
            apply_edit_once(workspace, path, old, new)
        }
        "MultiEdit" => {
            let path = arg_str("file_path").ok_or("MultiEdit: missing file_path")?;
            let edits = input
                .get("edits")
                .and_then(Value::as_array)
                .ok_or("MultiEdit: missing edits array")?;
            for pair in edits {
                let old = pair
                    .get("old_string")
                    .and_then(Value::as_str)
                    .ok_or("MultiEdit: edit missing old_string")?;
                let new = pair
                    .get("new_string")
                    .and_then(Value::as_str)
                    .ok_or("MultiEdit: edit missing new_string")?;
                apply_edit_once(workspace, path, old, new)?;
            }
            Ok(format!("{}: {} replacements applied", path, edits.len()))
        }
        "Glob" => {
            let pattern = arg_str("pattern").ok_or("Glob: missing pattern")?;
            let matches = glob_workspace(workspace, pattern)?;
            Ok(matches.into_iter().take(50).collect::<Vec<_>>().join("\n"))
        }
        "Grep" => {
            let pattern = arg_str("pattern").ok_or("Grep: missing pattern")?;
            let matcher = Regex::new(pattern)
                .map_err(|e| format!("Grep: invalid pattern '{pattern}': {e}"))?;
            let mut hits: Vec<String> = Vec::new();
            walk_files(workspace, Path::new(""), &mut |rel, abs| {
                if hits.len() >= 50 {
                    return;
                }
                if let Ok(content) = std::fs::read_to_string(abs) {
                    for (idx, line) in content.lines().enumerate() {
                        if matcher.is_match(line) {
                            hits.push(format!("{}:{}:{}", rel.display(), idx + 1, line));
                        }
                    }
                }
            });
            if hits.is_empty() {
                Err(format!("Grep: no matches for '{pattern}'"))
            } else {
                Ok(hits.join("\n"))
            }
        }
        "Bash" => {
            let command = arg_str("command").ok_or("Bash: missing command")?.trim();
            if let Some(target) = command.strip_prefix("ls").map(str::trim) {
                let dir = if target.is_empty() || target == "." {
                    workspace.to_path_buf()
                } else {
                    workspace_file(workspace, target)
                };
                let mut names: Vec<String> = std::fs::read_dir(&dir)
                    .map_err(|e| format!("ls: {e}"))?
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                Ok(if names.is_empty() {
                    "(empty directory)".to_string()
                } else {
                    names.join("\n")
                })
            } else if let Some(rest) = command.strip_prefix("cat").map(str::trim) {
                std::fs::read_to_string(workspace_file(workspace, rest))
                    .map_err(|e| format!("cat: {rest}: {e}"))
            } else if command.starts_with("find . -type f") {
                let mut rels: Vec<String> = Vec::new();
                walk_files(workspace, Path::new(""), &mut |rel, _| {
                    // Mirror real `find .`: entries render with the `./`
                    // prefix (matches GNU coreutils output).
                    rels.push(format!("./{}", rel.display()));
                });
                Ok(rels.join("\n"))
            } else {
                Err(format!("Bash: '{command}' unavailable in dry-run stub"))
            }
        }
        other => Err(format!("{other}: unsupported tool in dry-run stub")),
    }
}

fn apply_edit_once(workspace: &Path, path: &str, old: &str, new: &str) -> Result<String, String> {
    let target = workspace_file(workspace, path);
    let content =
        std::fs::read_to_string(&target).map_err(|_| format!("Edit: {path} does not exist"))?;
    if !content.contains(old) {
        return Err(format!("Edit: old_string not found in {path}"));
    }
    let updated = content.replacen(old, new, 1);
    std::fs::write(&target, updated).map_err(|e| format!("Edit: {e}"))?;
    Ok(format!("{path}: edited"))
}

/// Tiny wildcard matcher over workspace-relative paths (uses `globset` already
/// in the dependency graph) — enough for suite prompts like `src/**/*.rs`.
fn glob_workspace(root: &Path, pattern: &str) -> Result<Vec<String>, String> {
    let normalized = pattern.strip_prefix("./").unwrap_or(pattern);
    let glob = globset::GlobBuilder::new(normalized)
        .literal_separator(true)
        .build()
        .map_err(|e| format!("Glob: invalid pattern '{pattern}': {e}"))?;
    let matcher = glob.compile_matcher();

    let mut hits = Vec::new();
    walk_files(root, Path::new(""), &mut |rel, _| {
        if matcher.is_match(rel) {
            hits.push(rel.to_string_lossy().into_owned());
        }
    });
    hits.sort();
    Ok(hits)
}

/// Depth-bounded recursive file visitor. `visit` receives each file's path
/// relative to the walk root (slash-separated forward on unix as-is from the
/// filesystem) and its absolute path. Hidden dirs, `target/` and
/// `node_modules/` are pruned; depth caps runaway trees.
fn walk_files(dir: &Path, rel_prefix: &Path, visit: &mut dyn FnMut(&Path, &Path)) {
    const MAX_DEPTH: usize = 8;
    if rel_prefix.components().count() > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy().into_owned();
        if name_str.starts_with('.') || name_str == "target" || name_str == "node_modules" {
            continue;
        }
        let rel = rel_prefix.join(&name);
        if path.is_dir() {
            walk_files(&path, &rel, visit);
        } else {
            visit(&rel, &path);
        }
    }
}

// ── Cross-run comparison ───────────────────────────────────────────────

/// Load a persisted `report.json` for the `diff` workflow.
pub fn load_report(path: &Path) -> Result<RunReport, EvalError> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| EvalError::Config(format!("cannot read {}: {e}", path.display())))?;
    serde_json::from_str(&raw).map_err(|e| EvalError::Config(format!("{}: {e}", path.display())))
}

/// Diff verdict between two runs.
///
/// W1 §4② — the ATTRIBUTE-SPLIT protocol runs FIRST: when the two runs did
/// not measure the same thing (model, provider, profile digest, or
/// failure-rule table differ), the function refuses the single-line
/// stability/regression conclusion and only enumerates raw deltas. Numbers
/// are still listed; the verdict is not issued — a model swap can never
/// masquerade as a code-change regression.
///
/// With anchors aligned, structurally stable runs (agreeing
/// [`RunReport::stable_digest`]) report STABLE; otherwise the
/// version-comparison metadata plus per-task deltas worth eyeballing are
/// enumerated (statuses, budgets, trajectories, §4.7 metrics and failure
/// classes).
pub fn compare_reports(a: &RunReport, b: &RunReport) -> String {
    let split = attribute_splits(a, b);
    if !split.is_empty() {
        let mut out = String::from(
            "ATTRIBUTE-SPLIT: attribution dimensions differ between runs — \
             capability verdict withheld, raw deltas only\n",
        );
        for (label, left, right) in &split {
            out.push_str(&format!("  {label}: {left} -> {right}\n"));
        }
        out.push_str(&raw_deltas(a, b));
        out.push_str("\n(no cross-run conclusion: align anchors and rule table, then re-diff)\n");
        return out;
    }

    if a.stable_digest() == b.stable_digest() {
        return format!(
            "STABLE: structural digest identical across runs ({} tasks)",
            a.records.len()
        );
    }
    let mut out = String::from("UNSTABLE: digests differ\n");
    out.push_str(&raw_deltas(a, b));
    out
}

/// Attribution dimensions whose disagreement blocks a cross-run verdict
/// (W1 §4②): every anchor field plus the failure-rule fingerprint.
fn attribute_splits(a: &RunReport, b: &RunReport) -> Vec<(&'static str, String, String)> {
    let render = |value: &Option<String>| value.clone().unwrap_or_else(|| "(unknown)".to_string());
    let mut splits = Vec::new();
    for (label, left, right) in [
        ("model_id", &a.anchors.model_id, &b.anchors.model_id),
        ("provider", &a.anchors.provider, &b.anchors.provider),
        (
            "profile_digest",
            &a.anchors.profile_digest,
            &b.anchors.profile_digest,
        ),
    ] {
        if left != right {
            splits.push((label, render(left), render(right)));
        }
    }
    if a.failure_rules_fingerprint != b.failure_rules_fingerprint {
        splits.push((
            "failure_rules_fingerprint",
            a.failure_rules_fingerprint.clone(),
            b.failure_rules_fingerprint.clone(),
        ));
    }
    splits
}

/// The evidence half of a diff — version-comparison metadata plus per-task
/// deltas. Shared verbatim by the UNSTABLE and ATTRIBUTE-SPLIT paths so the
/// numbers stay visible even when the verdict is withheld (§4②: 数字照列).
fn raw_deltas(a: &RunReport, b: &RunReport) -> String {
    let mut out = String::new();

    // Version-comparison anchors (§4.7 ③): engine/rules/provenance drift.
    out.push_str("\n[meta]\n");
    for (label, left, right) in [
        (
            "app_version",
            a.app_version.as_str(),
            b.app_version.as_str(),
        ),
        (
            "failure_rules_fingerprint",
            a.failure_rules_fingerprint.as_str(),
            b.failure_rules_fingerprint.as_str(),
        ),
        (
            "metrics_source",
            a.metrics_source.as_str(),
            b.metrics_source.as_str(),
        ),
    ] {
        let mark = if left == right { "=" } else { "->" };
        out.push_str(&format!("  {label} {mark} {left} -> {right}\n"));
    }

    for (ra, rb) in a.records.iter().zip(b.records.iter()) {
        let va = ra.stable_view();
        let vb = rb.stable_view();
        if va != vb {
            out.push_str(&format!("\n[{}] {}\n", ra.id, ra.tier));
            for key in [
                "status",
                "exit_code",
                "turns",
                "tokens_in",
                "tokens_out",
                "total_tokens",
                "violations",
                "soft_flags",
                "trajectory_tools",
                "failure_class",
                "over_expected",
                "metrics",
            ] {
                if va.get(key) != vb.get(key) {
                    out.push_str(&format!(
                        "  {key}: {} -> {}\n",
                        va.get(key).map_or("?".into(), ToString::to_string),
                        vb.get(key).map_or("?".into(), ToString::to_string)
                    ));
                }
            }
        }
    }
    out
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ── Task parsing ──────────────────────────────────────────────────

    const SAMPLE_TASK: &str = r#"
id = "edit_42"
tier = "edit"
description = "rename a symbol"
prompt = "Rename fetch_data to load_data in src/api.rs"

[setup]
git_init = false

[[setup.files]]
path = "src/api.rs"
content = "fn fetch_data() {}"

[[verify.rules]]
rule = "file_content"
path = "src/api.rs"
contains = "fn load_data"

[[verify.rules]]
rule = "diff_matches"
path = "src/api.rs"
expected_diff_regex = '-fn fetch_data\(\).*\n\+fn load_data\(\)'

[expectations]
forbidden_tools = ["Write", "Bash"]

[[expectations.trajectory]]
tool = "Edit"
args_regex = '"old_string":"fn fetch_data'

[limits]
max_turns = 5
max_tokens = 20000
timeout_secs = 120

[dry_run]
final_text = "Renamed."

[[dry_run.steps]]
tool = "Read"
input = { file_path = "src/api.rs" }

[[dry_run.steps]]
tool = "Edit"
input = { file_path = "src/api.rs", old_string = "fn fetch_data()", new_string = "fn load_data()" }
"#;

    fn write_task(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write task toml");
        path
    }

    #[test]
    fn parse_full_task_roundtrip() {
        let dir = tempfile::TempDir::new().expect("tmp");
        let path = write_task(dir.path(), "edit_42.toml", SAMPLE_TASK);
        let task = parse_task(&path).expect("parse");

        assert_eq!(task.id, "edit_42");
        assert_eq!(task.tier, EvalTier::Edit);
        assert_eq!(task.setup.files.len(), 1);
        assert_eq!(task.verify.rules.len(), 2);
        assert_eq!(task.expectations.forbidden_tools, vec!["Write", "Bash"]);
        assert_eq!(task.expectations.trajectory.len(), 1);
        assert_eq!(task.expectations.trajectory[0].tool, "Edit");
        assert!(!task.expectations.trajectory[0].args_regex.is_empty());

        let limits = ResolvedLimits::of(&task.limits);
        assert_eq!(limits.max_turns, 5);
        assert_eq!(limits.max_tokens, 20_000);
        assert_eq!(limits.timeout_secs, 120);

        assert!(task.validate().is_empty(), "{:?}", task.validate());
        assert_eq!(task.task_source_path_hint.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn parse_task_defaults_fill_in() {
        let minimal = r#"
id = "read_01"
tier = "read"
prompt = "What version?"

[dry_run]
final_text = "1.0.0"

[[dry_run.steps]]
tool = "Read"
input = { file_path = "meta.txt" }
"#;
        let dir = tempfile::TempDir::new().expect("tmp");
        let task = parse_task(&write_task(dir.path(), "read_01.toml", minimal)).expect("parse");
        assert!(task.setup.files.is_empty());
        assert!(!task.setup.git_init);
        assert!(task.verify.rules.is_empty());
        let limits = ResolvedLimits::of(&task.limits);
        assert_eq!(limits.max_turns, DEFAULT_MAX_TURNS);
        assert_eq!(limits.max_tokens, DEFAULT_MAX_TOKENS);
        assert_eq!(limits.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert!(
            !task.validate().is_empty(),
            "no rules/script/expectations must be flagged"
        );
    }

    // ── W1 §2① horizon dimension ──────────────────────────────────────

    fn minimal_task_body(id: &str, tier: &str, extra: &str) -> String {
        format!(
            "id = \"{id}\"\ntier = \"{tier}\"\nprompt = \"p\"\n{extra}\n[dry_run]\nfinal_text = \"t\"\n\n[[dry_run.steps]]\ntool = \"Read\"\ninput = {{ file_path = \"x\" }}\n"
        )
    }

    #[test]
    fn horizon_defaults_table_and_explicit_overrides() {
        let dir = tempfile::TempDir::new().expect("tmp");
        let parse = |name: &str, body: String| {
            parse_task(&write_task(dir.path(), name, &body)).expect("parse")
        };

        // Tier-derived: read ⇒ short.
        let short = parse("s.toml", minimal_task_body("s", "read", ""));
        assert_eq!(short.horizon, None);
        assert_eq!(short.effective_horizon(), Horizon::Short);
        assert_eq!(
            short.resolved_limits(),
            ResolvedLimits {
                max_turns: 12,
                max_tokens: 300_000,
                timeout_secs: 180
            }
        );

        // Tier-derived: multi_step ⇒ mid.
        let mid = parse("m.toml", minimal_task_body("m", "multi_step", ""));
        assert_eq!(mid.effective_horizon(), Horizon::Mid);
        assert_eq!(
            mid.resolved_limits(),
            ResolvedLimits {
                max_turns: 30,
                max_tokens: 800_000,
                timeout_secs: 450
            }
        );

        // Explicit horizon field: long row applies to whatever tier.
        let long = parse(
            "l.toml",
            minimal_task_body("l", "recovery", "horizon = \"long\"\n"),
        );
        assert_eq!(long.effective_horizon(), Horizon::Long);
        assert_eq!(
            long.resolved_limits(),
            ResolvedLimits {
                max_turns: 80,
                max_tokens: 2_000_000,
                timeout_secs: 900
            }
        );

        // Explicit limits always beat the horizon table, per-field.
        let overridden = parse(
            "o.toml",
            minimal_task_body(
                "o",
                "multi_step",
                "horizon = \"mid\"\n\n[limits]\nmax_turns = 5\n",
            ),
        );
        let limits = overridden.resolved_limits();
        assert_eq!(limits.max_turns, 5, "declared value wins");
        assert_eq!(
            limits.max_tokens, MID_MAX_TOKENS,
            "unset field keeps mid row"
        );
        assert_eq!(limits.timeout_secs, MID_TIMEOUT_SECS);
    }

    #[test]
    fn over_expected_marks_but_never_fails() {
        // Expected budget deliberately tiny: the stub's turn count (2 steps +
        // final turn) and the declared token total (20000) blow it wide open.
        let over_budget = SAMPLE_TASK.replace(
            "[limits]\nmax_turns = 5\nmax_tokens = 20000\ntimeout_secs = 120",
            "[limits]\nmax_turns = 5\nmax_tokens = 20000\ntimeout_secs = 120\nexpected = { turns = 1, tokens = 100 }",
        );
        assert!(
            over_budget.contains("expected = { turns = 1, tokens = 100 }"),
            "limits block must have been extended"
        );
        let run_root = tempfile::TempDir::new().expect("runroot");
        let tasks_root = tempfile::TempDir::new().expect("tasks");
        let task =
            parse_task(&write_task(tasks_root.path(), "over.toml", &over_budget)).expect("parse");

        let options = options_into(run_root.path());
        let (report, _) = run_suite(std::slice::from_ref(&task), &options).expect("suite");
        let record = &report.records[0];
        assert_eq!(
            record.status,
            RunStatus::Passed,
            "soft marker must not fail"
        );
        assert!(record.passed);
        let over = record.over_expected.expect("over_expected flagged");
        assert!(over.turns_multiple.unwrap_or(0.0) > 1.0, "{over:?}");
        assert!(over.tokens_multiple.unwrap_or(0.0) > 1.0, "{over:?}");
        // The marker rides the stable digest (design §2③: budget_flags in
        // stable_view) so budget blowouts surface in cross-run diffs.
        assert!(report.stable_digest().contains("over_expected"));

        // Same task without a declared expectation stays unmarked.
        let task =
            parse_task(&write_task(tasks_root.path(), "plain.toml", SAMPLE_TASK)).expect("parse2");
        let (report, _) = run_suite(std::slice::from_ref(&task), &options).expect("suite2");
        assert_eq!(report.records[0].over_expected, None);
    }

    #[test]
    fn parse_rejects_unknown_tier_and_malformed_rule() {
        let dir = tempfile::TempDir::new().expect("tmp");

        let bad_tier =
            "id = \"x\"\ntier = \"quantum\"\nprompt = \"p\"\n[dry_run]\nfinal_text = \"t\"\n";
        assert!(parse_task(&write_task(dir.path(), "bad_tier.toml", bad_tier)).is_err());

        let bad_rule = concat!(
            "id = \"y\"\ntier = \"edit\"\nprompt = \"p\"\n",
            "[[verify.rules]]\nrule = \"cost_below\"\nmax_usd = 1.0\nper = \"fortnight\"\n",
            "[dry_run]\nfinal_text = \"t\"\n"
        );
        assert!(
            parse_task(&write_task(dir.path(), "bad_rule.toml", bad_rule)).is_err(),
            "unknown cost basis must fail at task parse time"
        );
    }

    #[test]
    fn parse_tasks_dir_is_sorted_by_filename_and_skips_non_toml() {
        let dir = tempfile::TempDir::new().expect("tmp");
        for name in ["c_third.toml", "a_first.toml", "b_second.toml", "notes.txt"] {
            if !name.ends_with(".toml") {
                std::fs::write(dir.path().join(name), "not a task").expect("write");
                continue;
            }
            let id = name
                .trim_end_matches(".toml")
                .split('_')
                .next_back()
                .unwrap_or(name);
            let body = format!(
                "id = \"{id}\"\ntier = \"read\"\nprompt = \"p\"\n\n[dry_run]\nfinal_text = \"t\"\n\n[[dry_run.steps]]\ntool = \"Read\"\ninput = {{ file_path = \"x\" }}\n"
            );
            std::fs::write(dir.path().join(name), body).expect("write");
        }
        let tasks = parse_tasks_dir(dir.path()).expect("parse dir");
        let ids: Vec<_> = tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["first", "second", "third"]);
    }

    // ── NDJSON decoding ───────────────────────────────────────────────

    #[test]
    fn ndjson_lines_decode_all_event_shapes() {
        assert_eq!(
            parse_ndjson_line(r#"{"type":"start","session_id":"s-1","model":"m"}"#),
            NdjsonLine::Start {
                session_id: Some("s-1".into()),
                model: Some("m".into())
            }
        );
        assert_eq!(
            parse_ndjson_line(r#"{"type":"tool_call","name":"Edit","input":{"file_path":"a.rs"}}"#),
            NdjsonLine::ToolCall {
                name: "Edit".into(),
                input_json: r#"{"file_path":"a.rs"}"#.into()
            }
        );
        // Both real-world tool_result shapes normalize onto one variant.
        assert_eq!(
            parse_ndjson_line(
                r#"{"type":"tool_result","name":"Edit","success":true,"output":"ok"}"#
            ),
            NdjsonLine::ToolResult {
                name: "Edit".into(),
                success: Some(true)
            }
        );
        assert_eq!(
            parse_ndjson_line(
                r#"{"type":"tool_result","name":"Bash","is_error":true,"output":"boom"}"#
            ),
            NdjsonLine::ToolResult {
                name: "Bash".into(),
                success: Some(false)
            }
        );
        assert_eq!(
            parse_ndjson_line(r#"{"type":"text_delta","content":"hi"}"#),
            NdjsonLine::TextDelta {
                content: "hi".into()
            }
        );
        assert_eq!(
            parse_ndjson_line(
                r#"{"type":"done","exit_code":0,"turns_used":3,"tokens_used":90,"tokens_in":60,"tokens_out":30}"#
            ),
            NdjsonLine::Done {
                exit_code: Some(0),
                turns_used: Some(3),
                tokens_used: Some(90),
                tokens_in: Some(60),
                tokens_out: Some(30)
            }
        );

        // Tolerant fallbacks.
        assert_eq!(parse_ndjson_line("garbage not json"), NdjsonLine::Other);
        assert_eq!(parse_ndjson_line(""), NdjsonLine::Other);
        assert_eq!(
            parse_ndjson_line(r#"{"type":"future_event","x":1}"#),
            NdjsonLine::Other
        );
    }

    #[test]
    fn observe_stream_merges_dual_done_lines_field_wise() {
        // Real headless runs emit an engine-side `done` carrying usage/turns
        // followed by a final synthesized marker with only `exit_code`.
        // Field-wise merge must keep the populated stats (last-wins used to
        // zero every metric on real runs).
        let stream = concat!(
            "{\"type\":\"start\",\"session_id\":\"s\",\"model\":\"m\"}\n",
            "{\"type\":\"done\",\"exit_code\":0,\"turns_used\":3,\"tokens_used\":127468,\"tokens_in\":126984,\"tokens_out\":484}\n",
            "{\"type\":\"done\",\"exit_code\":0}\n",
        );
        let obs = observe_stream(stream);
        match obs.done {
            Some(NdjsonLine::Done {
                exit_code,
                turns_used,
                tokens_used,
                tokens_in,
                tokens_out,
            }) => {
                assert_eq!(exit_code, Some(0));
                assert_eq!(turns_used, Some(3));
                assert_eq!(tokens_used, Some(127_468));
                assert_eq!(tokens_in, Some(126_984));
                assert_eq!(tokens_out, Some(484));
            }
            other => panic!("expected merged Done, got {other:?}"),
        }
    }

    #[test]
    fn observe_stream_ignores_duplicate_tool_use_channel() {
        // The CLI emits each invocation both as a CI `tool_call` event and as
        // an output `tool_use` event; observation must count every call once.
        let stream = concat!(
            "{\"type\":\"start\",\"session_id\":\"s\",\"model\":\"m\"}\n",
            "{\"type\":\"tool_call\",\"name\":\"Read\",\"input\":{\"file_path\":\"a\"}}\n",
            "{\"type\":\"text_delta\",\"content\":\"partial \"}\n",
            "{\"type\":\"tool_use\",\"name\":\"Read\",\"input\":{\"file_path\":\"a\"}}\n",
            "{\"type\":\"tool_result\",\"name\":\"Read\",\"success\":true,\"output\":\"..\"}\n",
            "{\"type\":\"text_delta\",\"content\":\"answer\"}\n",
            "{\"type\":\"error\",\"message\":\"late noise\"}\n",
            "{\"type\":\"done\",\"exit_code\":0,\"turns_used\":2,\"tokens_used\":10,\"tokens_in\":6,\"tokens_out\":4}\n",
        );
        let obs = observe_stream(stream);
        assert_eq!(obs.session_id.as_deref(), Some("s"));
        assert_eq!(obs.model.as_deref(), Some("m"));
        assert_eq!(obs.trajectory.len(), 1, "tool_use echo must be skipped");
        assert_eq!(obs.trajectory[0].tool, "Read");
        assert_eq!(obs.answer_text, "partial answer");
        assert_eq!(obs.errors, vec!["late noise"]);
        match obs.done.expect("done captured") {
            NdjsonLine::Done {
                exit_code: Some(0),
                tokens_used: Some(10),
                ..
            } => {}
            other => panic!("unexpected done: {other:?}"),
        }
    }

    // ── Stub executor primitives ──────────────────────────────────────

    fn seed_workspace() -> tempfile::TempDir {
        let tmp = tempfile::TempDir::new().expect("tmp");
        std::fs::create_dir_all(tmp.path().join("src")).expect("mkdir");
        std::fs::write(
            tmp.path().join("src/app.rs"),
            "fn main() {\n    println!(\"Hello, world!\");\n}\n",
        )
        .expect("seed");
        std::fs::write(
            tmp.path().join("notes.md"),
            "- fix login bug\n- add retry\n",
        )
        .expect("seed notes");
        tmp
    }

    #[test]
    fn stub_primitives_act_on_real_files() {
        let ws = seed_workspace();

        let content = apply_stub_tool("Read", &json!({ "file_path": "src/app.rs" }), ws.path())
            .expect("read");
        assert!(content.contains("println"));

        apply_stub_tool(
            "Edit",
            &json!({
                "file_path": "src/app.rs",
                "old_string": "Hello, world!",
                "new_string": "Goodbye, world!"
            }),
            ws.path(),
        )
        .expect("edit works");
        let after = std::fs::read_to_string(ws.path().join("src/app.rs")).expect("read back");
        assert!(after.contains("Goodbye"));

        // Failed edits surface as errors, never silent success.
        let miss = apply_stub_tool(
            "Edit",
            &json!({
                "file_path": "src/app.rs",
                "old_string": "absent text",
                "new_string": "x"
            }),
            ws.path(),
        );
        assert!(miss.unwrap_err().contains("old_string not found"));

        apply_stub_tool(
            "Write",
            &json!({ "file_path": "docs/guide.md", "content": "# Guide" }),
            ws.path(),
        )
        .expect("nested write works");
        assert!(ws.path().join("docs/guide.md").exists());

        apply_stub_tool(
            "MultiEdit",
            &json!({
                "file_path": "notes.md",
                "edits": [
                    { "old_string": "login bug", "new_string": "signup bug" },
                    { "old_string": "add retry", "new_string": "add backoff" }
                ]
            }),
            ws.path(),
        )
        .expect("multiedit works");
        let notes = std::fs::read_to_string(ws.path().join("notes.md")).expect("notes");
        assert!(notes.contains("signup bug") && notes.contains("add backoff"));
    }

    #[test]
    fn stub_grep_glob_search_the_real_tree() {
        let ws = seed_workspace();

        let glob_hits =
            apply_stub_tool("Glob", &json!({ "pattern": "src/**/*.rs" }), ws.path()).expect("glob");
        assert_eq!(glob_hits, "src/app.rs");

        let grep_hits =
            apply_stub_tool("Grep", &json!({ "pattern": "retry" }), ws.path()).expect("grep hits");
        assert!(grep_hits.starts_with("notes.md:"), "{grep_hits}");
        assert!(grep_hits.contains("- add retry"));

        assert!(
            apply_stub_tool(
                "Grep",
                &json!({ "pattern": "no_such_token_xyz" }),
                ws.path()
            )
            .is_err()
        );
        assert!(apply_stub_tool("Glob", &json!({ "pattern": "([" }), ws.path()).is_err());

        let find_out = apply_stub_tool("Bash", &json!({ "command": "find . -type f" }), ws.path())
            .expect("find works");
        assert!(find_out.contains("./notes.md"));
        assert!(find_out.contains("./src/app.rs"));

        let ls_out =
            apply_stub_tool("Bash", &json!({ "command": "ls src" }), ws.path()).expect("ls");
        assert_eq!(ls_out, "app.rs");
    }

    #[test]
    fn stub_unsupported_tools_fail_loudly() {
        let ws = seed_workspace();
        let err = apply_stub_tool("WebFetch", &json!({"url":"https://example.com"}), ws.path())
            .unwrap_err();
        assert!(err.contains("unsupported tool"), "{err}");

        let bash_err =
            apply_stub_tool("Bash", &json!({"command":"rm -rf /"}), ws.path()).unwrap_err();
        assert!(
            bash_err.contains("unavailable in dry-run stub"),
            "{bash_err}"
        );
    }

    #[test]
    fn stub_stream_matches_engine_schema() {
        let ws = seed_workspace();
        let body = concat!(
            "id = \"probe\"\ntier = \"edit\"\nprompt = \"probe\"\n\n",
            "[dry_run]\nfinal_text = \"Updated.\"\n\n",
            "[[dry_run.steps]]\ntool = \"Read\"\ninput = { file_path = \"src/app.rs\" }\n"
        );
        let dir = tempfile::TempDir::new().expect("tmp");
        let task = parse_task(&write_task(dir.path(), "probe.toml", body)).expect("parse");

        let produced = execute_stub(&task, ws.path());
        let lines: Vec<&str> = produced.ndjson.lines().collect();
        assert_eq!(lines.len(), 5, "start + call/result + text + done");
        assert!(produced.ndjson.contains("\"type\":\"start\""));
        assert!(produced.ndjson.contains("\"type\":\"tool_call\""));
        assert!(produced.ndjson.contains("\"type\":\"tool_result\""));
        assert!(produced.ndjson.contains("\"type\":\"text_delta\""));
        assert!(produced.ndjson.contains("\"type\":\"done\""));

        let obs = observe_stream(&produced.ndjson);
        match obs.done {
            Some(NdjsonLine::Done {
                exit_code: Some(0),
                turns_used: Some(2),
                ..
            }) => {}
            other => panic!("terminal done expected, got {other:?}"),
        }
        assert_eq!(obs.answer_text, "Updated.");
        assert_eq!(produced.exit_code, Some(0));
    }

    // ── Workspace preparation ─────────────────────────────────────────

    #[test]
    fn prepare_task_dir_seeds_files_and_archives_definition() {
        let run_dir = tempfile::TempDir::new().expect("runroot");
        let source = tempfile::TempDir::new().expect("source");
        let source_path = write_task(source.path(), "edit_42.toml", SAMPLE_TASK);
        let task = parse_task(&source_path).expect("parse");

        let ws = prepare_task_dir(&task, run_dir.path()).expect("prepare");
        assert!(ws.ends_with("workspace"));
        assert!(ws.join("src/api.rs").exists());
        assert_eq!(
            std::fs::read_to_string(ws.join("src/api.rs")).expect("content"),
            "fn fetch_data() {}"
        );
        assert!(
            !ws.join(".git").exists(),
            "git_init=false must skip git init"
        );

        let archived = run_dir.path().join("edit_42").join("task.toml");
        // The archive is a byte-exact copy (leading newline included).
        assert_eq!(
            std::fs::read_to_string(&archived)
                .expect("archive exists")
                .trim(),
            SAMPLE_TASK.trim()
        );
    }

    #[cfg(unix)]
    #[test]
    fn prepare_task_dir_git_init_creates_repo() {
        let run_dir = tempfile::TempDir::new().expect("runroot");
        let dir = tempfile::TempDir::new().expect("tmp");
        let git_task_body = concat!(
            "id = \"rec_09\"\ntier = \"recovery\"\nprompt = \"resolve\"\n",
            "[setup]\ngit_init = true\n\n",
            "[[setup.files]]\npath = \"a.txt\"\ncontent = \"x\"\n\n",
            "[dry_run]\nfinal_text = \"done\"\n\n[[dry_run.steps]]\n",
            "tool = \"Read\"\ninput = { file_path = \"a.txt\" }\n"
        );
        let task =
            parse_task(&write_task(dir.path(), "rec_09.toml", git_task_body)).expect("parse");
        let ws = prepare_task_dir(&task, run_dir.path()).expect("prepare");
        assert!(
            ws.join(".git").exists(),
            "git init should have created .git"
        );
    }

    // ── Full pipeline (stub mode) ─────────────────────────────────────

    fn options_into(out: &Path) -> EvalOptions {
        EvalOptions {
            bin_path: None,
            dry_run: true,
            out_dir_override: Some(out.to_path_buf()),
            failure_rules: None,
            instruction_directive: None,
        }
    }

    #[test]
    fn effective_rules_for_keeps_recovery_strict_softens_other_tiers() {
        let dir = tempfile::TempDir::new().expect("tmp");
        let body = |tier: &str| {
            format!(
                "id = \"probe\"\ntier = \"{tier}\"\nprompt = \"p\"\n\n\
                 [expectations]\nforbidden_tools = [\"Bash\"]\n\n\
                 [dry_run]\nfinal_text = \"t\"\n\n\
                 [[dry_run.steps]]\ntool = \"Read\"\ninput = {{ file_path = \"x\" }}\n"
            )
        };
        let edit_tier =
            parse_task(&write_task(dir.path(), "e.toml", &body("edit"))).expect("parse");
        let recovery_tier =
            parse_task(&write_task(dir.path(), "r.toml", &body("recovery"))).expect("parse");

        let edit_rules = effective_rules_for(&edit_tier);
        assert_eq!(edit_rules.len(), 1);
        assert!(matches!(
            &edit_rules[0],
            ValidationRule::ForbiddenTool { tool, strict: false } if tool == "Bash"
        ));

        let recovery_rules = effective_rules_for(&recovery_tier);
        assert_eq!(recovery_rules.len(), 1);
        assert!(matches!(
            &recovery_rules[0],
            ValidationRule::ForbiddenTool { tool, strict: true } if tool == "Bash"
        ));
    }

    /// RCA 2026-08-28 §5 决策点 1/2: a stub `Write` step violates the ban.
    /// Edit tier — flagged, still green (means-not-contract). Recovery
    /// tier — the same hit fails the run (trajectory is the contract).
    #[test]
    fn forbidden_tool_soft_flags_pass_strict_fails() {
        let body = |id: &str, tier: &str| {
            format!(
                "id = \"{id}\"\ntier = \"{tier}\"\nprompt = \"write the file\"\n\n\
                 [[verify.rules]]\nrule = \"file_content\"\npath = \"out.txt\"\ncontains = \"done\"\n\n\
                 [expectations]\nforbidden_tools = [\"Write\", \"Bash\"]\n\n\
                 [dry_run]\nfinal_text = \"Wrote.\"\n\n\
                 [[dry_run.steps]]\ntool = \"Write\"\ninput = {{ file_path = \"out.txt\", content = \"done\" }}\n"
            )
        };
        let run_root = tempfile::TempDir::new().expect("runroot");
        let tasks_root = tempfile::TempDir::new().expect("tasks");
        let soft = parse_task(&write_task(
            tasks_root.path(),
            "soft.toml",
            &body("soft_probe", "edit"),
        ))
        .expect("parse");
        let hard = parse_task(&write_task(
            tasks_root.path(),
            "hard.toml",
            &body("hard_probe", "recovery"),
        ))
        .expect("parse");

        let options = options_into(run_root.path());
        let tasks = vec![soft, hard];
        let (report, _) = run_suite(&tasks, &options).expect("suite");

        let soft_record = report
            .records
            .iter()
            .find(|r| r.id == "soft_probe")
            .expect("row");
        assert_eq!(
            soft_record.status,
            RunStatus::Passed,
            "{:?}",
            soft_record.violations
        );
        assert!(soft_record.passed);
        assert!(soft_record.violations.is_empty());
        assert_eq!(
            soft_record.soft_flags,
            vec!["forbidden_tool: 'Write' was invoked".to_string()]
        );
        assert!(
            soft_record
                .rule_outcomes
                .iter()
                .any(|o| o.rule == "forbidden_tool" && o.passed && o.soft_flags.len() == 1),
            "{:?}",
            soft_record.rule_outcomes
        );

        let strict_record = report
            .records
            .iter()
            .find(|r| r.id == "hard_probe")
            .expect("row");
        assert_eq!(strict_record.status, RunStatus::Failed);
        assert!(!strict_record.passed);
        assert!(strict_record.soft_flags.is_empty());
        assert!(
            strict_record
                .violations
                .iter()
                .any(|v| v.contains("forbidden_tool: 'Write' was invoked")),
            "{:?}",
            strict_record.violations
        );

        // The report keeps the soft detour visible without counting it as a
        // failure: markdown `soft` column + digest key.
        let md = report.render_markdown();
        assert!(md.contains("forbidden_tool: 'Write' was invoked"), "{md}");
        assert!(report.stable_digest().contains("soft_flags"));
    }

    #[test]
    fn pipeline_dry_run_passes_declared_script_and_writes_reports() {
        let run_root = tempfile::TempDir::new().expect("runroot");
        let tasks_root = tempfile::TempDir::new().expect("tasks");
        let path = write_task(tasks_root.path(), "edit_42.toml", SAMPLE_TASK);
        let task = parse_task(&path).expect("parse");
        assert!(task.validate().is_empty());

        let options = options_into(run_root.path());
        let (report, run_dir) = run_suite(std::slice::from_ref(&task), &options).expect("suite");

        assert_eq!(report.tasks_total, 1);
        assert_eq!(report.tasks_passed, 1, "self-consistent script must pass");
        let record = &report.records[0];
        assert_eq!(record.status, RunStatus::Passed);
        assert_eq!(record.trajectory_tools, vec!["Read", "Edit"]);
        assert!(record.rule_outcomes.iter().all(|o| o.passed));
        assert_eq!(
            record.rule_outcomes.len(),
            2 + 1 + 2,
            "declared rules + trajectory expectation + two forbidden tools fold into one set"
        );

        // Evidence bundle retained on disk.
        let evidence = run_dir.join("edit_42");
        assert!(evidence.join("workspace/src/api.rs").exists());
        assert!(evidence.join("stream.ndjson").exists());
        assert!(evidence.join("result.json").exists());
        assert!(evidence.join("task.toml").exists());
        assert!(run_dir.join("report.json").exists());
        assert!(run_dir.join("report.md").exists());

        let md = report.render_markdown();
        assert!(md.contains("# Shannon Eval Report"), "{md}");
        assert!(md.contains("DRY-RUN"));
        assert!(md.contains("| edit | edit_42 | passed |"));

        let persisted: RunReport = serde_json::from_str(
            &std::fs::read_to_string(run_dir.join("report.json")).expect("read report.json"),
        )
        .expect("report deserializes");
        assert_eq!(persisted.stable_digest(), report.stable_digest());
    }

    #[test]
    fn pipeline_detects_violation_when_script_drifts_from_rules() {
        let run_root = tempfile::TempDir::new().expect("runroot");
        let tasks_root = tempfile::TempDir::new().expect("tasks");
        // The stub prose claims success but forgets the actual Edit — file
        // assertions must catch it.
        let drifting = SAMPLE_TASK.replace(
            "[[dry_run.steps]]\ntool = \"Edit\"\ninput = { file_path = \"src/api.rs\", old_string = \"fn fetch_data()\", new_string = \"fn load_data()\" }\n",
            "",
        );
        let task =
            parse_task(&write_task(tasks_root.path(), "drift.toml", &drifting)).expect("parse");

        let options = options_into(run_root.path());
        let (report, _) = run_suite(std::slice::from_ref(&task), &options).expect("suite");
        let record = &report.records[0];
        assert_eq!(record.status, RunStatus::Failed);
        assert!(!record.passed);
        assert!(
            record
                .violations
                .iter()
                .any(|v| v.contains("does not contain 'fn load_data'")),
            "{:?}",
            record.violations
        );
        assert_eq!(record.trajectory_tools, vec!["Read"]);
        assert!(report.render_markdown().contains("## Failures"));
    }

    #[test]
    fn token_budget_breach_yields_limit_class() {
        let run_root = tempfile::TempDir::new().expect("runroot");
        let tasks_root = tempfile::TempDir::new().expect("tasks");
        let over = SAMPLE_TASK
            .replacen("max_tokens = 20000", "max_tokens = 100", 1)
            .replace("[dry_run]", "[dry_run]\ntokens_used = 5000");
        let task = parse_task(&write_task(tasks_root.path(), "over.toml", &over)).expect("parse");
        assert_eq!(ResolvedLimits::of(&task.limits).max_tokens, 100);

        let options = options_into(run_root.path());
        let (report, _) = run_suite(std::slice::from_ref(&task), &options).expect("suite");
        let record = &report.records[0];
        assert_eq!(
            record.status,
            RunStatus::TokenLimit,
            "{:?}",
            record.violations
        );
        assert!(record.violations.iter().any(|v| v.contains("token limit")));
        assert!(!record.passed);
        assert_eq!(record.total_tokens, 5000);
        // Verification still ran — evidence stays visible despite the breach.
        assert!(!record.rule_outcomes.is_empty());
    }

    #[test]
    fn classify_exit_maps_guardrail_codes() {
        assert_eq!(classify_exit(0), RunStatus::Passed);
        assert_eq!(classify_exit(2), RunStatus::TurnLimit);
        assert_eq!(classify_exit(3), RunStatus::Timeout);
        for code in [1, 4, 5, 6] {
            assert_eq!(classify_exit(code), RunStatus::Failed);
        }
    }

    // ── Real-spawn mechanics with fake binaries ───────────────────────

    /// Build an executable shell script stand-in for the engine binary.
    #[cfg(unix)]
    fn fake_bin(dir: &Path, name: &str, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    #[cfg(unix)]
    #[test]
    fn spawn_engine_captures_fake_binary_stream_and_limits_fire() {
        let bin_tmp = tempfile::TempDir::new().expect("binroot");
        let run_root = tempfile::TempDir::new().expect("runroot");
        let tasks_root = tempfile::TempDir::new().expect("tasks");

        // The fake binary IS the engine contract under test: NDJSON-schema
        // output through a real child process. Its token totals deliberately
        // blow the tiny max_tokens budget so limit classification is proven
        // against genuine subprocess accounting.
        let script = concat!(
            "printf '%s\\n' ",
            "'{\"type\":\"start\",\"prompt\":\"-\",\"model\":\"fake\",\"session_id\":\"sid-7\"}' ",
            "'{\"type\":\"tool_call\",\"name\":\"Grep\",\"input\":{\"pattern\":\"TODO\"}}' ",
            "'{\"type\":\"text_delta\",\"content\":\"found two todos\"}' ",
            "'{\"type\":\"done\",\"exit_code\":0,\"turns_used\":2,\"tokens_used\":9999,\"tokens_in\":8888,\"tokens_out\":1111}'\n",
            "exit 0\n"
        );
        let bin = fake_bin(bin_tmp.path(), "shannon-fake", script);

        let fake_task_body = concat!(
            "id = \"search_fx\"\ntier = \"search\"\nprompt = \"count TODOs\"\n",
            "[[verify.rules]]\nrule = \"response_contains\"\ntext = \"todos\"\n\n",
            "[[expectations.trajectory]]\ntool = \"Grep\"\n\n",
            "[limits]\nmax_turns = 8\nmax_tokens = 50\ntimeout_secs = 30\n\n",
            "[dry_run]\nfinal_text = \"unused here\"\n"
        );
        let task = parse_task(&write_task(
            tasks_root.path(),
            "search_fx.toml",
            fake_task_body,
        ))
        .expect("parse");

        let options = EvalOptions {
            bin_path: Some(bin),
            dry_run: false,
            out_dir_override: Some(run_root.path().to_path_buf()),
            failure_rules: None,
            instruction_directive: None,
        };
        let (report, _) = run_suite(std::slice::from_ref(&task), &options).expect("suite");

        let record = &report.records[0];
        assert_eq!(
            record.session_id.as_deref(),
            Some("sid-7"),
            "start event decoded"
        );
        assert_eq!(
            record.status,
            RunStatus::TokenLimit,
            "{:?}",
            record.violations
        );
        assert_eq!(record.tokens_in, 8888);
        assert_eq!(record.total_tokens, 9999);
        assert_eq!(record.exit_code, Some(0));
        assert_eq!(record.trajectory_tools, vec!["Grep"]);
        // Rule verification ran alongside the guardrail classification.
        let response_rule = record
            .rule_outcomes
            .iter()
            .find(|o| o.rule == "response_contains")
            .expect("response rule evaluated");
        assert!(response_rule.passed, "{:?}", response_rule.details);

        assert!(report.render_markdown().contains("REAL"));
        assert!(!report.dry_run);
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_slow_children_within_deadline() {
        let bin_tmp = tempfile::TempDir::new().expect("binroot");
        let run_root = tempfile::TempDir::new().expect("runroot");
        let tasks_root = tempfile::TempDir::new().expect("tasks");

        let bin = fake_bin(bin_tmp.path(), "slow-shannon", "sleep 30\n");

        let slow_task_body = concat!(
            "id = \"multi_stuck\"\ntier = \"multi_step\"\nprompt = \"stall\"\n",
            "[[verify.rules]]\nrule = \"exit_code\"\nvalue = \"success\"\n\n",
            "[limits]\nmax_turns = 3\nmax_tokens = 900000\ntimeout_secs = 1\n\n",
            "[dry_run]\nfinal_text = \"unused\"\n"
        );
        let task =
            parse_task(&write_task(tasks_root.path(), "stuck.toml", slow_task_body)).expect("p");

        let options = EvalOptions {
            bin_path: Some(bin),
            dry_run: false,
            out_dir_override: Some(run_root.path().to_path_buf()),
            failure_rules: None,
            instruction_directive: None,
        };

        let began = Instant::now();
        let (report, _) = run_suite(std::slice::from_ref(&task), &options).expect("suite");
        assert!(
            began.elapsed() < Duration::from_secs(15),
            "kill-on-timeout must not wait out the child's full sleep"
        );
        let record = &report.records[0];
        assert_eq!(record.status, RunStatus::Timeout);
        assert!(record.exit_code.is_none());
        assert!(record.violations.iter().any(|v| v.contains("`done` event")));
    }

    #[cfg(unix)]
    #[test]
    fn verify_script_failure_is_recorded_as_violation() {
        let run_root = tempfile::TempDir::new().expect("runroot");
        let tasks_root = tempfile::TempDir::new().expect("tasks");

        let scripted = concat!(
            "id = \"rec_scr\"\ntier = \"recovery\"\nprompt = \"script probe\"\n",
            "[[setup.files]]\npath = \"state.txt\"\ncontent = \"wrong\"\n\n",
            "[verify]\nscript = \"grep -q RIGHT state.txt\"\n\n",
            "[dry_run]\nfinal_text = \"claimed fixed\"\n\n[[dry_run.steps]]\n",
            "tool = \"Read\"\ninput = { file_path = \"state.txt\" }\n"
        );
        let task = parse_task(&write_task(tasks_root.path(), "scr.toml", scripted)).expect("p");

        let options = options_into(run_root.path());
        let (report, _) = run_suite(std::slice::from_ref(&task), &options).expect("suite");
        let record = &report.records[0];
        assert_eq!(record.status, RunStatus::Failed);
        let script_outcome = record
            .rule_outcomes
            .iter()
            .find(|o| o.rule == "verify_script")
            .expect("script outcome recorded");
        assert!(!script_outcome.passed);
        assert!(
            record
                .violations
                .iter()
                .any(|v| v.contains("verify_script exited")),
            "{:?}",
            record.violations
        );

        // Happy twin: seeding the right content satisfies the same script.
        let scripted_ok = scripted.replace("content = \"wrong\"", "content = \"RIGHT\"");
        let task_ok =
            parse_task(&write_task(tasks_root.path(), "scr_ok.toml", &scripted_ok)).expect("p2");
        let (report_ok, _) = run_suite(std::slice::from_ref(&task_ok), &options).expect("s2");
        assert_eq!(report_ok.records[0].status, RunStatus::Passed);
    }

    #[test]
    fn missing_binary_surfaces_as_spawn_error_not_panic() {
        let run_root = tempfile::TempDir::new().expect("runroot");
        let tasks_root = tempfile::TempDir::new().expect("tasks");
        let task = parse_task(&write_task(tasks_root.path(), "t.toml", SAMPLE_TASK)).expect("p");

        let missing_dir = tempfile::TempDir::new().expect("missing root");
        let options = EvalOptions {
            bin_path: Some(missing_dir.path().join("definitely-absent-engine")),
            dry_run: false,
            out_dir_override: Some(run_root.path().to_path_buf()),
            failure_rules: None,
            instruction_directive: None,
        };
        let (report, _) = run_suite(std::slice::from_ref(&task), &options).expect("suite");
        assert_eq!(report.records[0].status, RunStatus::SpawnError);
        assert_eq!(report.tasks_spawn_errors, 1);
        // §4.7 honesty: nothing ran, so no metrics blob may be fabricated.
        assert!(report.records[0].metrics.is_none());
        assert_eq!(report.metrics_source, "none");
    }

    /// Regression (2026-08-28 RCA): `metrics.turns` read the L0 envelope
    /// `turn` max, but the engine opens exactly one L0 vocabulary turn per
    /// query — every row of a headless run is stamped `turn = 1` no matter
    /// how many LLM steps the agent took — so the reported turns statistic
    /// was a constant 1 and turn-based budgeting was blind. The enrichment
    /// must reconcile with the stream's `done.turns_used` (the step count
    /// `--max-turns` actually bounds). The fixture below mirrors the real
    /// engine shape: every L0 row at `turn: 1`, seven steps on the stream.
    #[cfg(unix)]
    #[test]
    fn real_mode_metrics_turns_reconciles_constant_one_envelope_with_stream() {
        let bin_tmp = tempfile::TempDir::new().expect("binroot");
        let run_root = tempfile::TempDir::new().expect("runroot");
        let tasks_root = tempfile::TempDir::new().expect("tasks");

        let script = concat!(
            "mkdir -p \"$SHANNON_HOME/sessions/sid-turns\"\n",
            "cat > \"$SHANNON_HOME/sessions/sid-turns/events.jsonl\" <<'EVT'\n",
            "{\"seq\":0,\"ts_ns\":1000000000,\"session_id\":\"sid-turns\",\"turn\":1,\"kind\":\"session/start\",\"model\":\"fake\",\"provider\":\"mock\",\"app_version\":\"9.9.9\"}\n",
            "{\"seq\":1,\"ts_ns\":1100000000,\"session_id\":\"sid-turns\",\"turn\":1,\"kind\":\"tool/call\",\"tool_use_id\":\"a1\",\"tool_name\":\"Read\",\"arguments\":\"{}\"}\n",
            "{\"seq\":2,\"ts_ns\":1200000000,\"session_id\":\"sid-turns\",\"turn\":1,\"kind\":\"tool/result\",\"tool_use_id\":\"a1\",\"tool_name\":\"Read\",\"output\":\"ok\",\"is_error\":false}\n",
            "{\"seq\":3,\"ts_ns\":1300000000,\"session_id\":\"sid-turns\",\"turn\":1,\"kind\":\"turn/end\",\"reason\":\"completed\",\"usage\":{\"input_tokens\":70,\"output_tokens\":9,\"cache_creation_tokens\":0,\"cache_read_tokens\":0,\"cost_usd\":null}}\n",
            "EVT\n",
            "printf '%s\\n' ",
            "'{\"type\":\"start\",\"prompt\":\"-\",\"model\":\"fake\",\"session_id\":\"sid-turns\"}' ",
            "'{\"type\":\"text_delta\",\"content\":\"all done\"}' ",
            "'{\"type\":\"done\",\"exit_code\":0,\"turns_used\":7,\"tokens_used\":79,\"tokens_in\":70,\"tokens_out\":9}'\n",
            "exit 0\n"
        );
        let bin = fake_bin(bin_tmp.path(), "shannon-turns", script);

        let task_body = concat!(
            "id = \"turns_reconcile\"\ntier = \"multi_step\"\nprompt = \"take steps\"\n",
            "[[verify.rules]]\nrule = \"response_contains\"\ntext = \"all done\"\n",
            "[limits]\nmax_turns = 50\nmax_tokens = 90000\ntimeout_secs = 20\n\n",
            "[dry_run]\nfinal_text = \"unused\"\n"
        );
        let task = parse_task(&write_task(
            tasks_root.path(),
            "turns_reconcile.toml",
            task_body,
        ))
        .expect("parse");

        let options = EvalOptions {
            bin_path: Some(bin),
            dry_run: false,
            out_dir_override: Some(run_root.path().to_path_buf()),
            failure_rules: None,
            instruction_directive: None,
        };
        let (report, _) = run_suite(std::slice::from_ref(&task), &options).expect("suite");
        let record = &report.records[0];
        assert_eq!(record.status, RunStatus::Passed, "{:?}", record.violations);

        let metrics = record.metrics.as_ref().expect("metrics present");
        assert_eq!(
            metrics.source,
            MetricSource::EventsLog,
            "fixture must exercise the L0 path"
        );
        // The L0 envelope alone reads 1 (every row stamped turn = 1); the
        // reconciliation must lift the reported statistic to the real
        // step count carried by the stream's done line.
        assert_eq!(metrics.turns, 7, "turns must reflect real LLM steps");
        assert_eq!(record.turns, 7, "runner-side record agrees with the stream");
    }

    /// §4.7 end-to-end plumbing in REAL mode: the child sees an isolated
    /// `SHANNON_HOME` (as the genuine engine tee relies on), writes its own
    /// L0 log there, and the runner extracts metrics + failure class from
    /// that log and archives the failing sample.
    #[cfg(unix)]
    #[test]
    fn real_mode_extracts_metrics_from_child_l0_log_and_archives_failures() {
        let bin_tmp = tempfile::TempDir::new().expect("binroot");
        let run_root = tempfile::TempDir::new().expect("runroot");
        let tasks_root = tempfile::TempDir::new().expect("tasks");

        // The fake binary reproduces exactly the child-side side effect of
        // the §4.2 tee: events.jsonl under its own $SHANNON_HOME. Content is
        // consumed verbatim by the extractor — Edit fails unrecovered after
        // one healthy Read, a usage-carrying turn closes, and exit code 1
        // marks the task Failed.
        let script = concat!(
            "mkdir -p \"$SHANNON_HOME/sessions/sid-l0\"\n",
            "cat > \"$SHANNON_HOME/sessions/sid-l0/events.jsonl\" <<'EVT'\n",
            "{\"seq\":0,\"ts_ns\":1000000000,\"session_id\":\"sid-l0\",\"turn\":1,\"kind\":\"session/start\",\"model\":\"fake\",\"provider\":\"mock\",\"app_version\":\"9.9.9\"}\n",
            "{\"seq\":1,\"ts_ns\":1100000000,\"session_id\":\"sid-l0\",\"turn\":1,\"kind\":\"permission/decision\",\"tool_name\":\"Bash\",\"decision\":\"deny\"}\n",
            "{\"seq\":2,\"ts_ns\":1200000000,\"session_id\":\"sid-l0\",\"turn\":1,\"kind\":\"tool/call\",\"tool_use_id\":\"a1\",\"tool_name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\\\"src/a.rs\\\"}\"}\n",
            "{\"seq\":3,\"ts_ns\":1300000000,\"session_id\":\"sid-l0\",\"turn\":1,\"kind\":\"tool/result\",\"tool_use_id\":\"a1\",\"tool_name\":\"Read\",\"output\":\"ok\",\"is_error\":false}\n",
            "{\"seq\":4,\"ts_ns\":1400000000,\"session_id\":\"sid-l0\",\"turn\":1,\"kind\":\"tool/call\",\"tool_use_id\":\"a2\",\"tool_name\":\"Edit\",\"arguments\":\"{\\\"old_string\\\":\\\"zzz\\\"}\"}\n",
            "{\"seq\":5,\"ts_ns\":1500000000,\"session_id\":\"sid-l0\",\"turn\":1,\"kind\":\"tool/result\",\"tool_use_id\":\"a2\",\"tool_name\":\"Edit\",\"output\":\"anchor not found\",\"is_error\":true}\n",
            "{\"seq\":6,\"ts_ns\":1600000000,\"session_id\":\"sid-l0\",\"turn\":2,\"kind\":\"turn/end\",\"reason\":\"completed\",\"usage\":{\"input_tokens\":700,\"output_tokens\":90,\"cache_creation_tokens\":10,\"cache_read_tokens\":200,\"cost_usd\":0.42}}\n",
            "EVT\n",
            "printf '%s\\n' ",
            "'{\"type\":\"start\",\"prompt\":\"-\",\"model\":\"fake\",\"session_id\":\"sid-l0\"}' ",
            "'{\"type\":\"tool_call\",\"name\":\"Read\",\"input\":{\"file_path\":\"src/a.rs\"}}' ",
            "'{\"type\":\"text_delta\",\"content\":\"editing...\"}' ",
            "'{\"type\":\"done\",\"exit_code\":1,\"turns_used\":2,\"tokens_used\":790,\"tokens_in\":700,\"tokens_out\":90}'\n",
            "exit 1\n"
        );
        let bin = fake_bin(bin_tmp.path(), "shannon-l0", script);

        let task_body = concat!(
            "id = \"multi_fx\"\ntier = \"multi_step\"\nprompt = \"apply fix\"\n",
            "[[verify.rules]]\nrule = \"response_contains\"\ntext = \"editing\"\n",
            "[limits]\nmax_turns = 8\nmax_tokens = 90000\ntimeout_secs = 20\n\n",
            "[dry_run]\nfinal_text = \"unused\"\n"
        );
        let task =
            parse_task(&write_task(tasks_root.path(), "multi_fx.toml", task_body)).expect("parse");

        let options = EvalOptions {
            bin_path: Some(bin),
            dry_run: false,
            out_dir_override: Some(run_root.path().to_path_buf()),
            failure_rules: None,
            instruction_directive: None,
        };
        let (report, run_dir) = run_suite(std::slice::from_ref(&task), &options).expect("suite");
        let record = &report.records[0];

        // Provenance flipped to the genuine L0 source.
        assert_eq!(
            record.metrics.as_ref().expect("metrics").source,
            MetricSource::EventsLog
        );
        assert_eq!(report.metrics_source, "events_jsonl");

        let metrics = record.metrics.as_ref().unwrap();
        assert_eq!(missing_fields(metrics), Vec::<&'static str>::new());
        assert_eq!(metrics.tokens_in, 700);
        assert_eq!(metrics.cost_usd, Some(0.42));
        assert_eq!(metrics.permission_blocks, 1);
        assert_eq!(metrics.tool_calls, 2);
        assert!(metrics.wall_clock_ms.unwrap_or_default() <= 1000);
        assert_eq!(
            metrics.invalid_calls, 0,
            "recovery retried elsewhere, not verbatim"
        );

        // Classification: failing row. The log carries BOTH a denied
        // permission and an unrecovered Edit; the declared rule order makes
        // ④ 权限误拒 outrank ⑥ 编辑冲突, demonstrating deterministic priority.
        assert_eq!(record.status, RunStatus::Failed);
        assert_eq!(
            record.failure_class.as_deref(),
            Some("permission_misreject")
        );
        assert!(
            record
                .failure_evidence
                .iter()
                .any(|line| line.starts_with("permission_blocks ge 1")),
            "{:?}",
            record.failure_evidence
        );

        // The L0 log lives inside the task's own evidence directory…
        let l0_log = run_dir
            .join("multi_fx")
            .join(L0_HOME_DIRNAME)
            .join("sessions")
            .join("sid-l0")
            .join("events.jsonl");
        assert!(
            l0_log.exists(),
            "isolated SHANNON_HOME must hold the tee log"
        );

        // …and the failed sample was archived with copy + verdict.
        let date = chrono::Utc::now().format("%Y%m%d").to_string();
        let archive_root = run_root
            .path()
            .parent()
            .expect("parent")
            .join("eval")
            .join("failures")
            .join(&date);
        let archived = archive_root.join("multi_fx");
        let copied_log = std::fs::read_dir(&archived)
            .expect("archive dir")
            .filter_map(Result::ok)
            .find(|e| e.file_name().to_string_lossy().starts_with("events-"))
            .expect("archived events.jsonl copy");
        assert!(copied_log.metadata().expect("meta").len() > 0);
        let verdict =
            std::fs::read_to_string(archived.join("classification.json")).expect("verdict");
        assert!(
            verdict.contains("\"failure_class\": \"permission_misreject\""),
            "{verdict}"
        );
    }

    /// W1 §4①+§6 阶段 1 end-to-end in REAL mode: the anchor is lifted from
    /// the L0 `request/header` wire body (beating the stream banner), the
    /// suite consolidates it, a failure no rule fires on lands in the
    /// `unclassified` tally bucket, and the archived classification.json
    /// carries the minimal-reproduction recipe.
    #[cfg(unix)]
    #[test]
    fn real_run_anchors_unclassified_and_repro_archive() {
        let bin_tmp = tempfile::TempDir::new().expect("binroot");
        let run_root = tempfile::TempDir::new().expect("runroot");
        let tasks_root = tempfile::TempDir::new().expect("tasks");
        let rules_root = tempfile::TempDir::new().expect("rules");

        // Narrow override that matches NOTHING in the seeded log — the run
        // must land in the unclassified residue bucket, not be force-fit.
        let override_body = concat!(
            "schema_version = 1\n\n[[rule]]\n",
            "class = \"model_ceiling\"\ndescription = \"loops >= 99\"\n",
            "[[rule.condition]]\nsignal = \"loops\"\nop = \"ge\"\nvalue = \"99\"\n"
        );
        let rules_path = rules_root.path().join("narrow.toml");
        std::fs::write(&rules_path, override_body).expect("write rules override");

        // The child writes an L0 log whose request/header carries the
        // wire-honest model, provider, and a config snapshot; the stream
        // banner advertises a DIFFERENT model to prove the header wins.
        let script = concat!(
            "mkdir -p \"$SHANNON_HOME/sessions/sid-anchor\"\n",
            "cat > \"$SHANNON_HOME/sessions/sid-anchor/events.jsonl\" <<'EVT'\n",
            "{\"seq\":0,\"ts_ns\":1000000000,\"session_id\":\"sid-anchor\",\"turn\":1,\"kind\":\"session/start\",\"model\":\"banner-model\",\"provider\":\"banner-provider\",\"app_version\":\"9.9.9\"}\n",
            "{\"seq\":1,\"ts_ns\":1100000000,\"session_id\":\"sid-anchor\",\"turn\":1,\"kind\":\"request/header\",\"model\":\"declared-model\",\"provider\":\"mock\",\"adapter_defaults\":null,\"config_snapshot\":{\"profile\":\"full_auto\"},\"reason\":\"initial\",\"wire_body\":{\"model\":\"wire-model\",\"messages\":[]}}\n",
            "{\"seq\":2,\"ts_ns\":1200000000,\"session_id\":\"sid-anchor\",\"turn\":1,\"kind\":\"tool/call\",\"tool_use_id\":\"a1\",\"tool_name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"false\\\"}\"}\n",
            "{\"seq\":3,\"ts_ns\":1300000000,\"session_id\":\"sid-anchor\",\"turn\":1,\"kind\":\"tool/result\",\"tool_use_id\":\"a1\",\"tool_name\":\"Bash\",\"output\":\"boom\",\"is_error\":true}\n",
            "{\"seq\":4,\"ts_ns\":1400000000,\"session_id\":\"sid-anchor\",\"turn\":2,\"kind\":\"turn/end\",\"reason\":\"completed\",\"usage\":{\"input_tokens\":10,\"output_tokens\":2,\"cache_creation_tokens\":0,\"cache_read_tokens\":0,\"cost_usd\":0.01}}\n",
            "EVT\n",
            "printf '%s\\n' ",
            "'{\"type\":\"start\",\"prompt\":\"-\",\"model\":\"stream-model\",\"session_id\":\"sid-anchor\"}' ",
            "'{\"type\":\"done\",\"exit_code\":1,\"turns_used\":2,\"tokens_used\":12,\"tokens_in\":10,\"tokens_out\":2}'\n",
            "exit 1\n"
        );
        let bin = fake_bin(bin_tmp.path(), "shannon-anchor", script);
        let bin_display = bin.display().to_string();

        let task_body = concat!(
            "id = \"edit_anc\"\ntier = \"edit\"\nprompt = \"apply fix\"\n",
            "[[verify.rules]]\nrule = \"exit_code\"\nvalue = \"success\"\n",
            "[limits]\nmax_turns = 8\nmax_tokens = 90000\ntimeout_secs = 20\n\n",
            "[dry_run]\nfinal_text = \"unused\"\n"
        );
        let task =
            parse_task(&write_task(tasks_root.path(), "edit_anc.toml", task_body)).expect("parse");

        let options = EvalOptions {
            bin_path: Some(bin),
            dry_run: false,
            out_dir_override: Some(run_root.path().to_path_buf()),
            failure_rules: Some(rules_path),
            instruction_directive: None,
        };
        let (report, _run_dir) = run_suite(std::slice::from_ref(&task), &options).expect("suite");
        let record = &report.records[0];

        // Anchor: the L0 request/header wire model wins over both the banner
        // and the stream-start model.
        assert_eq!(record.anchor.model_id.as_deref(), Some("wire-model"));
        assert_eq!(record.anchor.provider.as_deref(), Some("mock"));
        assert!(record.anchor.profile_digest.is_some());
        assert_eq!(
            report.anchors.model_id.as_deref(),
            Some("wire-model"),
            "suite anchor consolidates the per-task anchor"
        );
        assert!(
            report
                .render_markdown()
                .contains("Anchor: model=wire-model")
        );

        // §6 阶段 1: failed, no rule fired ⇒ unclassified bucket, in the
        // tally and in the markdown, while failure_class stays unset.
        assert_eq!(record.status, RunStatus::Failed);
        assert_eq!(record.failure_class, None);
        assert_eq!(
            report.failure_class_tally().get(UNCLASSIFIED_CLASS),
            Some(&1)
        );
        assert!(
            report
                .render_markdown()
                .contains(&format!("{UNCLASSIFIED_CLASS}: 1"))
        );

        // §7③: the archived verdict carries the minimal repro recipe.
        let date = chrono::Utc::now().format("%Y%m%d").to_string();
        let archived = run_root
            .path()
            .parent()
            .expect("parent")
            .join("eval")
            .join("failures")
            .join(&date)
            .join("edit_anc");
        let verdict: Value = serde_json::from_str(
            &std::fs::read_to_string(archived.join("classification.json")).expect("verdict"),
        )
        .expect("verdict json");
        let repro = verdict.get("repro").expect("repro field present");
        assert_eq!(
            repro.get("workspace").and_then(Value::as_str),
            Some(record.workspace.to_string_lossy().as_ref())
        );
        let args = repro
            .get("args")
            .and_then(Value::as_array)
            .expect("args array");
        assert_eq!(args.first().and_then(Value::as_str), Some("--prompt"));
        assert_eq!(
            args.last().and_then(Value::as_str),
            Some("8"),
            "max-turns rides last"
        );
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "--output-format" && pair[1] == "json-stream"),
            "{args:?}"
        );
        assert_eq!(
            repro.get("engine").and_then(Value::as_str),
            Some(bin_display.as_str())
        );
    }

    // ── Report digests & comparison ───────────────────────────────────

    #[test]
    fn stable_digest_excludes_volatile_fields() {
        let dir = tempfile::TempDir::new().expect("tmp");
        let task = parse_task(&write_task(dir.path(), "d.toml", SAMPLE_TASK)).expect("parse");
        let out_a = tempfile::TempDir::new().expect("a");
        let out_b = tempfile::TempDir::new().expect("b");

        let (report_a, _) =
            run_suite(std::slice::from_ref(&task), &options_into(out_a.path())).expect("a");
        // Nudge every volatile dimension: later clock second, fresh duration
        // profile, different workspace location.
        thread::sleep(Duration::from_millis(1100));
        let (report_b, _) =
            run_suite(std::slice::from_ref(&task), &options_into(out_b.path())).expect("b");

        assert_ne!(report_a.run_id, report_b.run_id);
        assert_ne!(report_a.started_at_utc, report_b.started_at_utc);
        assert_ne!(
            report_a.records[0].workspace, report_b.records[0].workspace,
            "override roots differ by construction"
        );

        assert_eq!(
            report_a.stable_digest(),
            report_b.stable_digest(),
            "digest must ignore timestamps/durations/paths yet pin statuses+counts"
        );
        let verdict = compare_reports(&report_a, &report_b);
        assert!(verdict.starts_with("STABLE:"), "{verdict}");
    }

    #[test]
    fn compare_reports_flags_structural_drift() {
        let dir = tempfile::TempDir::new().expect("tmp");
        let task = parse_task(&write_task(dir.path(), "d.toml", SAMPLE_TASK)).expect("parse");
        let out = tempfile::TempDir::new().expect("out");

        let (good, _) = run_suite(std::slice::from_ref(&task), &options_into(out.path()))
            .expect("baseline suite");

        // Break the trajectory expectation so the digest legitimately flips.
        let weaker_body =
            SAMPLE_TASK.replace("[[expectations.trajectory]]", "[[stale.trajectory]]");
        let weaker = parse_task(&write_task(out.path(), "weaker.toml", &weaker_body))
            .expect("weaker parses");
        let (worse, _) =
            run_suite(std::slice::from_ref(&weaker), &options_into(out.path())).expect("worse");

        let verdict = compare_reports(&good, &worse);
        assert!(verdict.starts_with("UNSTABLE"), "{verdict}");
        assert!(verdict.contains("[edit_42]"), "{verdict}");
        assert_ne!(good.stable_digest(), worse.stable_digest());
    }

    // ── W1 §4① anchors + §4② ATTRIBUTE-SPLIT ─────────────────────────

    #[test]
    fn dry_run_reports_carry_stub_anchor_and_horizon() {
        let dir = tempfile::TempDir::new().expect("tmp");
        let task = parse_task(&write_task(dir.path(), "d.toml", SAMPLE_TASK)).expect("parse");
        let out = tempfile::TempDir::new().expect("out");
        let (report, _) =
            run_suite(std::slice::from_ref(&task), &options_into(out.path())).expect("dry suite");

        // Suite anchor: the constant stub marker, no fabricated provider.
        assert_eq!(
            report.anchors.model_id.as_deref(),
            Some(DRY_RUN_ANCHOR_MODEL)
        );
        assert!(report.anchors.provider.is_none());
        assert!(report.anchors.profile_digest.is_none());
        // Per-task anchor mirrors it; horizon is tier-derived.
        let record = &report.records[0];
        assert_eq!(
            record.anchor.model_id.as_deref(),
            Some(DRY_RUN_ANCHOR_MODEL)
        );
        assert_eq!(record.horizon, "short", "edit tier derives short");

        // The markdown shows both the anchor line and the horizon column.
        let md = report.render_markdown();
        assert!(
            md.contains(&format!("Anchor: model={DRY_RUN_ANCHOR_MODEL}")),
            "{md}"
        );
        assert!(md.contains("| short |"), "{md}");

        // Anchors ride the stable digest (§4①) — flipping the model must
        // never digest as "unchanged".
        let mut other = report.clone();
        other.anchors.model_id = Some("some-real-model".into());
        assert_ne!(report.stable_digest(), other.stable_digest());
    }

    #[test]
    fn attribute_split_blocks_verdict_on_anchor_or_rule_drift() {
        let dir = tempfile::TempDir::new().expect("tmp");
        let task = parse_task(&write_task(dir.path(), "d.toml", SAMPLE_TASK)).expect("parse");
        let out_a = tempfile::TempDir::new().expect("a");
        let out_b = tempfile::TempDir::new().expect("b");
        let (a, _) =
            run_suite(std::slice::from_ref(&task), &options_into(out_a.path())).expect("suite a");
        let (b, _) =
            run_suite(std::slice::from_ref(&task), &options_into(out_b.path())).expect("suite b");
        assert_eq!(a.stable_digest(), b.stable_digest());

        // Model drift ⇒ ATTRIBUTE-SPLIT: no STABLE/UNSTABLE verdict, raw
        // deltas still enumerated.
        let mut other_model = b.clone();
        other_model.anchors.model_id = Some("claude-x".into());
        let verdict = compare_reports(&a, &other_model);
        assert!(verdict.starts_with("ATTRIBUTE-SPLIT"), "{verdict}");
        assert!(!verdict.starts_with("STABLE"));
        assert!(!verdict.starts_with("UNSTABLE"));
        assert!(verdict.contains("model_id"), "{verdict}");
        assert!(verdict.contains("[meta]"), "numbers stay listed: {verdict}");

        // Rule-table drift splits too (design §4② names the fingerprint).
        let mut other_rules = b.clone();
        other_rules.failure_rules_fingerprint = "deadbeefdeadbeef".to_string();
        let verdict = compare_reports(&a, &other_rules);
        assert!(verdict.starts_with("ATTRIBUTE-SPLIT"), "{verdict}");
        assert!(verdict.contains("failure_rules_fingerprint"), "{verdict}");

        // Aligned anchors keep the historical verdicts.
        let verdict = compare_reports(&a, &b);
        assert!(verdict.starts_with("STABLE:"), "{verdict}");
    }

    #[test]
    fn resolve_eval_home_honors_explicit_home_env_or_falls_back() {
        // Read-only inspection avoids global env mutation; the SHANNON_HOME
        // branch and the ~/.shannon fallback are mutually exclusive.
        match std::env::var("SHANNON_HOME") {
            Ok(home) => assert!(resolve_eval_home().expect("home").starts_with(home)),
            Err(_) => {
                let home = resolve_eval_home().expect("fallback home");
                assert!(home.ends_with(".shannon"));
                assert!(home.is_absolute());
            }
        }
    }

    #[test]
    fn resolve_bin_prefers_explicit_flag_then_env_then_default() {
        let custom = PathBuf::from("/opt/custom-shannon");
        assert_eq!(resolve_bin(Some(&custom)).expect("explicit"), custom);

        match std::env::var("SHANNON_EVAL_BIN") {
            Ok(env_bin) => assert_eq!(resolve_bin(None).expect("env"), PathBuf::from(env_bin)),
            Err(_) => {
                let resolved = resolve_bin(None).expect("resolved default");
                assert!(
                    resolved.ends_with("shannon"),
                    "falls back to target/debug path or PATH lookup: {resolved:?}"
                );
            }
        }
    }
}
