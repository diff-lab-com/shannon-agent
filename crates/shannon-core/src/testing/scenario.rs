//! Declarative YAML scenario testing framework.
//!
//! Defines test scenarios in YAML files with setup, mock responses, and validation
//! rules. Each scenario creates an isolated workspace, runs Shannon with mockito-
//! backed LLM responses, and validates the results against declared rules.
//!
//! ## Validation rules
//!
//! Outcome rules (original set — check the final state of a run):
//! - `file_exists` / `file_not_exists` / `file_content` — workspace file assertions
//! - `exit_code`, `response_contains`, `tool_called`, `max_duration_ms` — run output assertions
//!
//! Behavioral rules (W2-M1a — assert on *how* the result was produced, not just
//! the end state; they read their observations from [`ValidationContext`]):
//! - `diff_matches { path, expected_diff_regex }` — regex over a line diff of the
//!   file (before = value declared in `setup.files`, after = current workspace
//!   content). A path absent from setup is treated as created from scratch, so a
//!   Write-only run yields `+...` lines only.
//! - `trajectory_contains { sequence: [{tool, args_regex?}, ...] }` — the observed
//!   tool calls must contain these steps as an *ordered subsequence* (extra calls
//!   may interleave; relative order is enforced). When `args_regex` is present it
//!   must match the step's tool input rendered as compact JSON
//!   (`{"path":"x",...}`); when omitted only the tool name is compared. Path
//!   arguments (`file_path` / `path`) additionally accept suffix-normalized
//!   matching: a pattern spelling a relative path still matches an observed
//!   workspace-absolute value (models follow the tools' "Absolute path"
//!   contract), while non-path arguments are compared verbatim.
//! - `forbidden_tool { tool }` — fails if the tool appears anywhere in the
//!   observed trajectory.
//! - `cost_below { max_usd, per }` — budget assertion where `per` is either
//!   `task` (sum of per-turn costs ≤ `max_usd`) or `turn` (every turn cost ≤
//!   `max_usd`). No recorded cost data is treated as an unverifiable claim and
//!   fails loudly rather than passing vacuously.
//!
//! The YAML schema stays backward compatible: existing scenarios gain new rule
//! variants without any field changes; within each new rule its own fields are
//! required so that typos surface at parse time.
//!
//! ## Observation data sources
//!
//! Trajectory observations come from the session's L0 event log
//! ([`ToolCallTrace::from_session_events`]) or are derived from the scenario's
//! mocked assistant turns ([`ToolCallTrace::from_mock_turns`])
//! ([`ValidationRule::MaxDurationMs`]) remains enforced by the runner
//! externally.
//!
//! ```no_run
//! use shannon_core::testing::scenario::{
//!     evaluate_rules, ToolCallTrace, ValidationContext,
//! };
//!
//! # fn doc(ctx_dir: &std::path::Path) {
//! let trajectory = ToolCallTrace::from_mock_turns(&[]);
//! let ctx = ValidationContext::new(ctx_dir, "success", "").with_trajectory(&trajectory);
//! let outcomes = evaluate_rules(&[], &ctx);
//! assert!(outcomes.iter().all(|o| o.passed));
//! # }
//! ```

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::testing::mock_dsl::{MockContentBlock, MockResponse, text_response, tool_call_response};

// ── YAML Schema Types ─────────────────────────────────────────────────

/// Top-level scenario definition loaded from YAML.
#[derive(Debug, Deserialize)]
pub struct ScenarioYaml {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    pub setup: ScenarioSetup,
    #[serde(default)]
    pub mock_responses: Vec<MockTurn>,
    pub validate: Vec<ValidationRule>,
}

fn default_model() -> String {
    "test-model".to_string()
}

/// Setup configuration for the test workspace.
#[derive(Debug, Deserialize)]
pub struct ScenarioSetup {
    #[serde(default)]
    pub files: Vec<FileSetup>,
    #[serde(default)]
    pub permission_mode: String,
}

/// A file to create in the workspace before running.
#[derive(Debug, Deserialize)]
pub struct FileSetup {
    pub path: String,
    pub content: String,
}

/// A single mock LLM response for one turn.
#[derive(Debug, Deserialize)]
pub struct MockTurn {
    pub response: MockResponseYaml,
}

/// One expected step inside a `trajectory_contains` rule.
///
/// `args_regex` is optional; when present it is matched against the tool input
/// serialized as compact JSON (`{"path":"src/main.rs",...}`), so patterns must
/// not assume whitespace after `:` or `,`.
///
/// Path-class values get one leniency: when the pattern names a path argument
/// (`file_path` / `path`) and the observed call carries a longer (typically
/// workspace-absolute) value, suffix variants of that value — basename and
/// workspace-relative tails — are also tried, so a pattern spelling the
/// relative form still matches an absolute invocation. Every other argument
/// (`command`, `old_string`, `pattern`, ...) is compared verbatim.
#[derive(Debug, Default, Deserialize, Clone)]
pub struct TrajectoryStep {
    /// Legacy single-tool spelling. Kept for backward compatibility with the
    /// original YAML/TOML shapes; normalized into [`Self::tools`] by
    /// [`Self::candidates`].
    #[serde(default)]
    pub tool: String,
    /// Candidate tool family: the step is satisfied when the observed call's
    /// tool is any of these (e.g. `["Edit", "MultiEdit", "rename_symbol"]`).
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub args_regex: String,
    /// Means-not-contract step: the matcher skips it when the observed stream
    /// supplies no compatible call, and an unmet optional never fails the
    /// rule (recon steps like "Read the file first" belong here).
    #[serde(default)]
    pub optional: bool,
}

impl TrajectoryStep {
    /// Every tool name this step accepts (legacy `tool` first, then the
    /// `tools` family), de-duplicated in order.
    pub fn candidates(&self) -> impl Iterator<Item = &str> {
        let legacy = (!self.tool.is_empty()).then_some(self.tool.as_str());
        legacy
            .into_iter()
            .chain(self.tools.iter().map(String::as_str))
    }

    /// Tool-name membership check (args_regex is applied by the matcher).
    fn tool_matches(&self, call: &ToolCallTrace) -> bool {
        self.candidates().any(|t| t == call.tool)
    }
}

/// Granularity of a `cost_below` budget assertion.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CostBasis {
    /// Total spend across all recorded turns.
    Task,
    /// Each individual turn's spend must stay within the limit.
    Turn,
}

/// YAML representation of a mock response.
#[derive(Debug, Deserialize)]
pub struct MockResponseYaml {
    #[serde(rename = "type")]
    pub response_type: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub tool_id: String,
    #[serde(default)]
    pub input: Value,
}

/// A validation rule to check after the scenario runs.
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "rule")]
pub enum ValidationRule {
    #[serde(rename = "file_exists")]
    FileExists { path: String },
    #[serde(rename = "file_content")]
    FileContent {
        path: String,
        #[serde(default)]
        contains: String,
        #[serde(default)]
        matches_regex: String,
    },
    #[serde(rename = "file_not_exists")]
    FileNotExists { path: String },
    #[serde(rename = "exit_code")]
    ExitCode { value: String },
    #[serde(rename = "tool_called")]
    ToolCalled { tool: String },
    #[serde(rename = "response_contains")]
    ResponseContains { text: String },
    #[serde(rename = "max_duration_ms")]
    MaxDurationMs { limit: u64 },
    // ── Behavioral rules (W2-M1a) — observations come from ValidationContext ──
    /// Regex assertion over the before/after line diff of one file.
    #[serde(rename = "diff_matches")]
    DiffMatches {
        path: String,
        expected_diff_regex: String,
    },
    /// Expected tool-call steps that must appear (in order) in the observed
    /// trajectory. Extra interleaved calls are allowed.
    #[serde(rename = "trajectory_contains")]
    TrajectoryContains { sequence: Vec<TrajectoryStep> },
    /// Tool that must NOT appear anywhere in the observed trajectory.
    #[serde(rename = "forbidden_tool")]
    ForbiddenTool { tool: String },
    /// Budget ceiling for recorded per-turn costs.
    #[serde(rename = "cost_below")]
    CostBelow { max_usd: f64, per: CostBasis },
}

// ── Scenario Result ───────────────────────────────────────────────────

/// Independent pass/fail outcome of exactly one validation rule.
#[derive(Debug, Clone)]
pub struct RuleOutcome {
    /// Rule tag as spelled in YAML (`file_exists`, `trajectory_contains`, ...).
    pub rule: String,
    pub passed: bool,
    /// Human-readable violation details; empty when passed. A single rule can
    /// report several details (e.g. `file_content` with both `contains` and
    /// `matches_regex` violated).
    pub details: Vec<String>,
}

/// Compact trajectory summary carried on [`ScenarioResult`] for runner reports.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrajectorySummary {
    /// Observed tool names in invocation order (arguments omitted).
    pub tool_calls: Vec<String>,
    /// Total recorded spend across turns in USD.
    pub total_cost_usd: f64,
    /// Number of turns with cost accounting available.
    pub turns: usize,
}

impl TrajectorySummary {
    /// Build a summary from observed trajectory and per-turn costs.
    pub fn from_observations(trajectory: &[ToolCallTrace], turn_costs_usd: &[f64]) -> Self {
        Self {
            tool_calls: trajectory.iter().map(|c| c.tool.clone()).collect(),
            total_cost_usd: turn_costs_usd.iter().sum(),
            turns: turn_costs_usd.len(),
        }
    }
}

/// Result of running a scenario.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    pub name: String,
    pub passed: bool,
    pub failures: Vec<String>,
    pub duration_ms: u64,
    /// Independent per-rule outcomes (behavioral assertions, W2-M1a).
    pub rule_outcomes: Vec<RuleOutcome>,
    /// Trajectory summary for runner reports.
    pub trajectory_summary: TrajectorySummary,
}

impl ScenarioResult {
    pub fn pass(name: &str, duration_ms: u64) -> Self {
        Self {
            name: name.to_string(),
            passed: true,
            failures: Vec::new(),
            duration_ms,
            rule_outcomes: Vec::new(),
            trajectory_summary: TrajectorySummary::default(),
        }
    }

    pub fn fail(name: &str, duration_ms: u64, failures: Vec<String>) -> Self {
        Self {
            name: name.to_string(),
            passed: false,
            failures,
            duration_ms,
            rule_outcomes: Vec::new(),
            trajectory_summary: TrajectorySummary::default(),
        }
    }

    /// Build from evaluated rule outcomes plus a trajectory summary; `passed`
    /// is derived (true only when every rule passed) and `failures` collects
    /// all violation details.
    pub fn evaluated(
        name: &str,
        duration_ms: u64,
        rule_outcomes: Vec<RuleOutcome>,
        trajectory_summary: TrajectorySummary,
    ) -> Self {
        let passed = rule_outcomes.iter().all(|o| o.passed);
        let failures = outcomes_failures(&rule_outcomes);
        Self {
            name: name.to_string(),
            passed,
            failures,
            duration_ms,
            rule_outcomes,
            trajectory_summary,
        }
    }
}

// ── YAML → MockResponse Conversion ────────────────────────────────────

/// Convert a YAML mock response into a MockResponse for the mock DSL.
pub fn yaml_to_mock_response(yaml: &MockResponseYaml) -> MockResponse {
    match yaml.response_type.as_str() {
        "text" => text_response(&yaml.content),
        "tool_use" => {
            let id = if yaml.tool_id.is_empty() {
                format!("toolu_{}", uuid::Uuid::new_v4().as_simple())
            } else {
                yaml.tool_id.clone()
            };
            tool_call_response(&id, &yaml.tool, yaml.input.clone())
        }
        "thinking" => crate::testing::mock_dsl::thinking_response(&yaml.content),
        "text_and_tool" => {
            let id = if yaml.tool_id.is_empty() {
                format!("toolu_{}", uuid::Uuid::new_v4().as_simple())
            } else {
                yaml.tool_id.clone()
            };
            crate::testing::mock_dsl::text_and_tool_response(
                &yaml.content,
                &id,
                &yaml.tool,
                yaml.input.clone(),
            )
        }
        _ => text_response(&yaml.content),
    }
}

/// Convert all YAML mock turns into MockResponse objects.
pub fn yaml_to_mock_responses(turns: &[MockTurn]) -> Vec<MockResponse> {
    turns
        .iter()
        .map(|t| yaml_to_mock_response(&t.response))
        .collect()
}

// ── Observations ───────────────────────────────────────────────────────

/// One observed tool invocation, in execution order.
#[derive(Debug, Clone)]
pub struct ToolCallTrace {
    /// Tool name as invoked (e.g. `Read`, `Edit`, `Bash`).
    pub tool: String,
    /// Tool input arguments serialized as compact JSON.
    pub input_json: String,
}

impl ToolCallTrace {
    pub fn new(tool: &str, input_json: impl Into<String>) -> Self {
        Self {
            tool: tool.to_string(),
            input_json: input_json.into(),
        }
    }

    /// Derive the trajectory from scenario mock turns: every `tool_use` content
    /// block of the mocked assistant responses, in turn order. Before the L0
    /// event stream exists this is what a mocked run would have executed.
    pub fn from_mock_turns(turns: &[MockTurn]) -> Vec<Self> {
        turns
            .iter()
            .flat_map(|turn| yaml_to_mock_response(&turn.response).content_blocks)
            .filter_map(|block| match block {
                MockContentBlock::ToolUse { name, input, .. } => Some(Self {
                    tool: name,
                    input_json: serde_json::to_string(&input).expect("serialize tool input"),
                }),
                _ => None,
            })
            .collect()
    }

    /// Derive the trajectory from a session's L0 event log rows.
    ///
    /// `tool/call` bodies map 1:1 onto trace steps (`tool_name`, raw
    /// `arguments` kept verbatim); every other kind is skipped. Order matches
    /// the log, so `TrajectoryContains` sees the same subsequence semantics as
    /// the recording-based source it replaced in §4.6.
    pub fn from_session_events(events: &[shannon_types::session_event::SessionEvent]) -> Vec<Self> {
        use shannon_types::session_event::{SessionEventBody, ToolCallPayload};

        events
            .iter()
            .filter_map(|event| match &event.body {
                SessionEventBody::ToolCall(ToolCallPayload {
                    tool_name,
                    arguments,
                    ..
                }) => Some(Self {
                    tool: tool_name.clone(),
                    input_json: arguments.clone(),
                }),
                _ => None,
            })
            .collect()
    }
}

/// Observation data supplied by the harness when validating a finished run.
///
/// Outcome-only rules need just `workspace_dir` / `exit_code` / `stdout`; the
/// behavioral rules introduced in W2-M1a additionally read initial file
/// contents, the observed tool-call trajectory, and per-turn costs.
#[derive(Debug, Clone, Copy)]
pub struct ValidationContext<'a> {
    pub workspace_dir: &'a Path,
    pub exit_code: &'a str,
    pub stdout: &'a str,
    /// Files exactly as created during scenario setup: `(path, content)`
    /// baselines used by `diff_matches` to compute before/after diffs.
    pub initial_files: &'a [(String, String)],
    /// Observed tool invocations in execution order.
    pub trajectory: &'a [ToolCallTrace],
    /// Recorded cost per turn in USD; index aligns with the turn number.
    pub turn_costs_usd: &'a [f64],
}

impl<'a> ValidationContext<'a> {
    pub fn new(workspace_dir: &'a Path, exit_code: &'a str, stdout: &'a str) -> Self {
        Self {
            workspace_dir,
            exit_code,
            stdout,
            initial_files: &[],
            trajectory: &[],
            turn_costs_usd: &[],
        }
    }

    /// Attach setup-time file baselines for `diff_matches`.
    pub fn with_initial_files(mut self, files: &'a [(String, String)]) -> Self {
        self.initial_files = files;
        self
    }

    /// Attach the observed tool-call trajectory.
    pub fn with_trajectory(mut self, trajectory: &'a [ToolCallTrace]) -> Self {
        self.trajectory = trajectory;
        self
    }

    /// Attach per-turn cost observations.
    pub fn with_turn_costs_usd(mut self, costs: &'a [f64]) -> Self {
        self.turn_costs_usd = costs;
        self
    }
}

// ── Validation ────────────────────────────────────────────────────────

/// Validate a set of rules against workspace state.
///
/// Convenience wrapper over [`evaluate_rules`] for outcome-only runs that has
/// no trajectory/cost/diff observations attached.
pub fn validate_rules(
    rules: &[ValidationRule],
    workspace_dir: &Path,
    exit_code: &str,
    stdout: &str,
) -> Vec<String> {
    let ctx = ValidationContext::new(workspace_dir, exit_code, stdout);
    outcomes_failures(&evaluate_rules(rules, &ctx))
}

/// Flatten rule outcomes into the failure-string list consumed by callers.
fn outcomes_failures(outcomes: &[RuleOutcome]) -> Vec<String> {
    outcomes
        .iter()
        .flat_map(|outcome| outcome.details.iter().cloned())
        .collect()
}

/// Evaluate each rule independently against the run observations.
pub fn evaluate_rules(rules: &[ValidationRule], ctx: &ValidationContext) -> Vec<RuleOutcome> {
    rules.iter().map(|rule| evaluate_rule(rule, ctx)).collect()
}

fn violated(rule_tag: &str, details: Vec<String>) -> RuleOutcome {
    RuleOutcome {
        rule: rule_tag.to_string(),
        passed: false,
        details,
    }
}

fn passed(rule_tag: &str) -> RuleOutcome {
    RuleOutcome {
        rule: rule_tag.to_string(),
        passed: true,
        details: Vec::new(),
    }
}

fn evaluate_rule(rule: &ValidationRule, ctx: &ValidationContext) -> RuleOutcome {
    let mut failures = Vec::new();

    match rule {
        ValidationRule::FileExists { path } => {
            let full_path = ctx.workspace_dir.join(path);
            if !full_path.exists() {
                failures.push(format!("file_exists: {path} does not exist"));
            }
        }
        ValidationRule::FileContent {
            path,
            contains,
            matches_regex,
        } => {
            let full_path = ctx.workspace_dir.join(path);
            if !full_path.exists() {
                return violated(
                    "file_content",
                    vec![format!("file_content: {path} does not exist")],
                );
            }
            let content = std::fs::read_to_string(&full_path).unwrap_or_default();
            if !contains.is_empty() && !content.contains(contains.as_str()) {
                failures.push(format!(
                    "file_content: {path} does not contain '{contains}'"
                ));
            }
            if !matches_regex.is_empty() {
                if let Ok(re) = regex::Regex::new(matches_regex) {
                    if !re.is_match(&content) {
                        failures.push(format!(
                            "file_content: {path} does not match regex '{matches_regex}'"
                        ));
                    }
                }
            }
        }
        ValidationRule::FileNotExists { path } => {
            let full_path = ctx.workspace_dir.join(path);
            if full_path.exists() {
                failures.push(format!("file_not_exists: {path} should not exist"));
            }
        }
        ValidationRule::ExitCode { value } => {
            if ctx.exit_code != value {
                failures.push(format!(
                    "exit_code: expected '{value}', got '{}'",
                    ctx.exit_code
                ));
            }
        }
        ValidationRule::ToolCalled { tool } => {
            // Check stdout for tool use indicators in JSON output
            if !ctx.stdout.contains(tool.as_str()) {
                failures.push(format!("tool_called: tool '{tool}' not found in output"));
            }
        }
        ValidationRule::ResponseContains { text } => {
            if !ctx.stdout.contains(text.as_str()) {
                failures.push(format!("response_contains: '{text}' not found in output"));
            }
        }
        ValidationRule::MaxDurationMs { limit } => {
            // Duration is checked externally; this is a placeholder for
            // integration with timing logic
            let _ = limit;
        }
        ValidationRule::DiffMatches {
            path,
            expected_diff_regex,
        } => {
            eval_diff_matches(path, expected_diff_regex, ctx, &mut failures);
        }
        ValidationRule::TrajectoryContains { sequence } => {
            check_subsequence(sequence, ctx.trajectory, &mut failures);
        }
        ValidationRule::ForbiddenTool { tool } => {
            if ctx.trajectory.iter().any(|call| call.tool == *tool) {
                failures.push(format!("forbidden_tool: '{tool}' was invoked"));
            }
        }
        ValidationRule::CostBelow { max_usd, per } => {
            eval_cost_below(*max_usd, *per, ctx.turn_costs_usd, &mut failures);
        }
    }

    if failures.is_empty() {
        passed(rule_tag(rule))
    } else {
        violated(rule_tag(rule), failures)
    }
}

/// The YAML tag spelling of a rule variant.
fn rule_tag(rule: &ValidationRule) -> &'static str {
    match rule {
        ValidationRule::FileExists { .. } => "file_exists",
        ValidationRule::FileContent { .. } => "file_content",
        ValidationRule::FileNotExists { .. } => "file_not_exists",
        ValidationRule::ExitCode { .. } => "exit_code",
        ValidationRule::ToolCalled { .. } => "tool_called",
        ValidationRule::ResponseContains { .. } => "response_contains",
        ValidationRule::MaxDurationMs { .. } => "max_duration_ms",
        ValidationRule::DiffMatches { .. } => "diff_matches",
        ValidationRule::TrajectoryContains { .. } => "trajectory_contains",
        ValidationRule::ForbiddenTool { .. } => "forbidden_tool",
        ValidationRule::CostBelow { .. } => "cost_below",
    }
}

/// `diff_matches`: regex assertion over the line diff between the setup-time
/// baseline and the current workspace content of one file.
fn eval_diff_matches(
    path: &str,
    expected_diff_regex: &str,
    ctx: &ValidationContext,
    out: &mut Vec<String>,
) {
    let baseline = ctx
        .initial_files
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, c)| c.as_str())
        .unwrap_or("");
    let full_path = ctx.workspace_dir.join(path);
    let Ok(current) = std::fs::read_to_string(&full_path) else {
        out.push(format!("diff_matches: {path} does not exist in workspace"));
        return;
    };

    let diff = render_line_diff(baseline, &current);
    match Regex::new(expected_diff_regex) {
        Ok(re) if re.is_match(&diff) => {}
        Ok(_) => out.push(format!(
            "diff_matches: {path} diff does not match regex '{expected_diff_regex}'"
        )),
        Err(e) => out.push(format!(
            "diff_matches: invalid regex '{expected_diff_regex}': {e}"
        )),
    }
}

/// `cost_below`: budget ceiling over recorded per-turn costs.
fn eval_cost_below(max_usd: f64, per: CostBasis, turn_costs_usd: &[f64], out: &mut Vec<String>) {
    if turn_costs_usd.is_empty() {
        // An unverifiable budget claim must fail loudly, never pass vacuously.
        out.push("cost_below: no recorded turn costs available".to_string());
        return;
    }
    match per {
        CostBasis::Task => {
            let total: f64 = turn_costs_usd.iter().sum();
            if total > max_usd {
                out.push(format!(
                    "cost_below: task total ${total:.4} exceeds limit ${max_usd:.4}"
                ));
            }
        }
        CostBasis::Turn => {
            for (idx, cost) in turn_costs_usd.iter().enumerate() {
                if *cost > max_usd {
                    out.push(format!(
                        "cost_below: turn {} cost ${cost:.4} exceeds limit ${max_usd:.4}",
                        idx + 1
                    ));
                }
            }
        }
    }
}

/// Check that `sequence` appears as an ordered subsequence of `observed` calls:
/// gaps are allowed but relative order is enforced. A missing step or an
/// invalid `args_regex` is reported as a violation string.
fn check_subsequence(
    sequence: &[TrajectoryStep],
    observed: &[ToolCallTrace],
    out: &mut Vec<String>,
) {
    let matches_call = |step: &TrajectoryStep, call: &ToolCallTrace| -> Result<bool, String> {
        if !step.tool_matches(call) {
            return Ok(false);
        }
        if step.args_regex.is_empty() {
            return Ok(true);
        }
        Regex::new(&step.args_regex)
            .map(|re| {
                re.is_match(&call.input_json)
                    || matches_with_path_normalization(&re, &call.input_json)
            })
            .map_err(|e| e.to_string())
    };
    let mut invalid_regex = |step: &TrajectoryStep, idx: usize, err: String| {
        out.push(format!(
            "trajectory_contains: invalid args_regex '{}' on step {} ('{}'): {err}",
            step.args_regex,
            idx,
            step.candidates().collect::<Vec<_>>().join("|")
        ));
    };

    let mut si = 0usize;
    for call in observed {
        // Optional steps this call cannot satisfy are skipped (they are
        // means-not-contract); a compatible optional is consumed like any
        // other match.
        while si < sequence.len() && sequence[si].optional {
            let hit = match matches_call(&sequence[si], call) {
                Ok(v) => v,
                Err(e) => {
                    invalid_regex(&sequence[si], si + 1, e);
                    return;
                }
            };
            if hit {
                break;
            }
            si += 1;
        }
        if si >= sequence.len() {
            break;
        }
        let hit = match matches_call(&sequence[si], call) {
            Ok(v) => v,
            Err(e) => {
                invalid_regex(&sequence[si], si + 1, e);
                return;
            }
        };
        if hit {
            si += 1;
        }
    }

    for (want_idx, step) in sequence.iter().enumerate().skip(si) {
        if step.optional {
            continue;
        }
        let candidates = step.candidates().collect::<Vec<_>>().join("|");
        let expectation = if step.args_regex.is_empty() {
            String::new()
        } else {
            format!(" matching '{}'", step.args_regex)
        };
        out.push(format!(
            "trajectory_contains: step {} ('{}'{expectation}) not found in observed trajectory",
            want_idx + 1,
            candidates
        ));
    }
}

// ── Path normalization for args_regex ─────────────────────────────────

/// Argument keys whose values carry file-system paths. Real engine runs see
/// models emit workspace-absolute values for these (the tool schemas say
/// "Absolute path to the file" and the system prompt injects the working
/// directory), while task `args_regex` expectations usually spell the
/// workspace-relative form (`"file_path":"CHANGELOG.md"`). A verbatim
/// substring comparison then fails even though the model touched the right
/// file — the systematic false-negative this normalization exists to fix.
const PATH_ARGUMENT_KEYS: [&str; 2] = ["file_path", "path"];

/// Every `/`-suffix of a path value, longest first: `/a/b/src/main.rs` yields
/// `a/b/src/main.rs`, `b/src/main.rs`, `src/main.rs`, `main.rs`. The last
/// entry is the basename form; the intermediate entries cover task patterns
/// spelled as multi-segment relative paths (`"file_path":"src/main.rs"`).
/// The candidate set is bounded by the path's segment count — no wildcarding.
fn path_suffix_variants(value: &str) -> Vec<String> {
    let segments: Vec<&str> = value
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    (0..segments.len())
        .map(|start| segments[start..].join("/"))
        .collect()
}

/// Retry `re` against `input_json` with each path-class field's value swapped
/// for one of its suffix variants. Only fields whose key is named in the
/// pattern participate, so patterns over non-path arguments see byte-identical
/// input and a path pattern still has to match the rewritten value in full —
/// this loosens the path *prefix* only, never the asserted token itself.
fn matches_with_path_normalization(re: &Regex, input_json: &str) -> bool {
    let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(input_json) else {
        return false;
    };
    for key in PATH_ARGUMENT_KEYS {
        let Some(Value::String(raw)) = fields.get(key) else {
            continue;
        };
        if !raw.contains('/') || !re.as_str().contains(&format!("\"{key}\"")) {
            continue;
        }
        let Ok(original) = serde_json::to_string(raw) else {
            continue;
        };
        for variant in path_suffix_variants(raw) {
            if variant == *raw {
                continue;
            }
            let Ok(replacement) = serde_json::to_string(&variant) else {
                continue;
            };
            let rewritten = input_json.replacen(original.as_str(), &replacement, 1);
            if re.is_match(&rewritten) {
                return true;
            }
        }
    }
    false
}

/// Render a line-based diff (`-removed` / `+added` lines only) between two
/// texts using an LCS walk. Inputs are scenario-sized files, so the O(n*m)
/// table is fine and keeps this dependency-free.
fn render_line_diff(before: &str, after: &str) -> String {
    let old_lines: Vec<&str> = before.lines().collect();
    let new_lines: Vec<&str> = after.lines().collect();

    let rows = old_lines.len() + 1;
    let cols = new_lines.len() + 1;
    let mut lcs = vec![vec![0usize; cols]; rows];
    for i in (0..old_lines.len()).rev() {
        for j in (0..new_lines.len()).rev() {
            lcs[i][j] = if old_lines[i] == new_lines[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut lines = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < old_lines.len() && j < new_lines.len() {
        if old_lines[i] == new_lines[j] {
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            lines.push(format!("-{}", old_lines[i]));
            i += 1;
        } else {
            lines.push(format!("+{}", new_lines[j]));
            j += 1;
        }
    }
    while i < old_lines.len() {
        lines.push(format!("-{}", old_lines[i]));
        i += 1;
    }
    while j < new_lines.len() {
        lines.push(format!("+{}", new_lines[j]));
        j += 1;
    }

    lines.join("\n")
}

// ── Parsing ───────────────────────────────────────────────────────────

/// Parse a YAML scenario file.
pub fn parse_scenario(path: &Path) -> Result<ScenarioYaml, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let scenario: ScenarioYaml = serde_yaml::from_str(&content)
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;

    if scenario.name.is_empty() {
        return Err(format!("Scenario at {} has no name", path.display()));
    }
    if scenario.prompt.is_empty() {
        return Err(format!("Scenario '{}' has no prompt", scenario.name));
    }
    if scenario.mock_responses.is_empty() {
        return Err(format!(
            "Scenario '{}' has no mock_responses",
            scenario.name
        ));
    }

    Ok(scenario)
}

/// Parse all YAML files in a directory.
pub fn parse_scenarios_dir(dir: &Path) -> Result<Vec<(PathBuf, ScenarioYaml)>, String> {
    let mut scenarios = Vec::new();
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("Failed to read dir {}: {e}", dir.display()))?;

    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .map(|e| e.path())
        .collect();
    paths.sort();

    for path in paths {
        let scenario = parse_scenario(&path)?;
        scenarios.push((path, scenario));
    }

    Ok(scenarios)
}

// ── Workspace Setup ───────────────────────────────────────────────────

/// Create workspace files from scenario setup.
/// Returns the temp directory (caller must keep it alive).
pub fn create_scenario_workspace(setup: &ScenarioSetup) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("create scenario workspace");

    for file in &setup.files {
        let full_path = dir.path().join(&file.path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&full_path, &file.content).expect("write scenario file");
    }

    dir
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::testing::mock_dsl::MockContentBlock;
    use std::io::Write;

    fn write_temp_yaml(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("temp file");
        write!(f, "{content}").expect("write yaml");
        f
    }

    #[test]
    fn test_parse_minimal_scenario() {
        let f = write_temp_yaml(
            r#"
name: hello
prompt: "Say hello"
setup:
  files: []
mock_responses:
  - response:
      type: text
      content: "Hello!"
validate:
  - rule: exit_code
    value: success
"#,
        );
        let scenario = parse_scenario(f.path()).expect("parse");
        assert_eq!(scenario.name, "hello");
        assert_eq!(scenario.prompt, "Say hello");
        assert_eq!(scenario.mock_responses.len(), 1);
        assert_eq!(scenario.validate.len(), 1);
    }

    #[test]
    fn test_parse_tool_use_scenario() {
        let f = write_temp_yaml(
            r#"
name: write_file
description: "Create a file"
prompt: "Create hello.txt"
provider: anthropic
setup:
  files:
    - path: src/main.rs
      content: "fn main() {}"
mock_responses:
  - response:
      type: tool_use
      tool: Write
      input: { path: "hello.txt", content: "world" }
  - response:
      type: text
      content: "Done!"
validate:
  - rule: file_exists
    path: hello.txt
  - rule: file_content
    path: hello.txt
    contains: "world"
"#,
        );
        let scenario = parse_scenario(f.path()).expect("parse");
        assert_eq!(scenario.name, "write_file");
        assert_eq!(scenario.setup.files.len(), 1);
        assert_eq!(scenario.mock_responses.len(), 2);
        assert_eq!(scenario.validate.len(), 2);
    }

    #[test]
    fn test_yaml_to_mock_response_text() {
        let yaml = MockResponseYaml {
            response_type: "text".to_string(),
            content: "Hello!".to_string(),
            tool: String::new(),
            tool_id: String::new(),
            input: Value::Null,
        };
        let mock = yaml_to_mock_response(&yaml);
        assert_eq!(mock.content_blocks.len(), 1);
        assert_eq!(mock.stop_reason, "end_turn");
    }

    #[test]
    fn test_yaml_to_mock_response_tool_use() {
        let yaml = MockResponseYaml {
            response_type: "tool_use".to_string(),
            content: String::new(),
            tool: "Write".to_string(),
            tool_id: "toolu_1".to_string(),
            input: serde_json::json!({"path": "hello.txt", "content": "world"}),
        };
        let mock = yaml_to_mock_response(&yaml);
        assert_eq!(mock.stop_reason, "tool_use");
        assert!(matches!(
            &mock.content_blocks[0],
            MockContentBlock::ToolUse { name, .. } if name == "Write"
        ));
    }

    #[test]
    fn test_validate_file_exists_pass() {
        let dir = tempfile::TempDir::new().expect("dir");
        std::fs::write(dir.path().join("hello.txt"), "world").expect("write");

        let failures = validate_rules(
            &[ValidationRule::FileExists {
                path: "hello.txt".to_string(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert!(failures.is_empty(), "{failures:?}");
    }

    #[test]
    fn test_validate_file_exists_fail() {
        let dir = tempfile::TempDir::new().expect("dir");

        let failures = validate_rules(
            &[ValidationRule::FileExists {
                path: "missing.txt".to_string(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("missing.txt"));
    }

    #[test]
    fn test_validate_file_content_contains() {
        let dir = tempfile::TempDir::new().expect("dir");
        std::fs::write(dir.path().join("hello.txt"), "hello world").expect("write");

        let failures = validate_rules(
            &[ValidationRule::FileContent {
                path: "hello.txt".to_string(),
                contains: "world".to_string(),
                matches_regex: String::new(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert!(failures.is_empty(), "{failures:?}");
    }

    #[test]
    fn test_validate_file_content_regex() {
        let dir = tempfile::TempDir::new().expect("dir");
        std::fs::write(
            dir.path().join("code.rs"),
            "fn add(a: i32, b: i32) -> i32 { a + b }",
        )
        .expect("write");

        let failures = validate_rules(
            &[ValidationRule::FileContent {
                path: "code.rs".to_string(),
                contains: String::new(),
                matches_regex: r"fn \w+\(".to_string(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert!(failures.is_empty(), "{failures:?}");
    }

    #[test]
    fn test_validate_exit_code() {
        let dir = tempfile::TempDir::new().expect("dir");
        let failures = validate_rules(
            &[ValidationRule::ExitCode {
                value: "success".to_string(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert!(failures.is_empty());

        let failures = validate_rules(
            &[ValidationRule::ExitCode {
                value: "success".to_string(),
            }],
            dir.path(),
            "error",
            "",
        );
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn test_validate_tool_called() {
        let dir = tempfile::TempDir::new().expect("dir");
        let failures = validate_rules(
            &[ValidationRule::ToolCalled {
                tool: "Write".to_string(),
            }],
            dir.path(),
            "success",
            r#"{"tool_use": {"name": "Write", "input": {}}}"#,
        );
        assert!(failures.is_empty());
    }

    #[test]
    fn test_validate_response_contains() {
        let dir = tempfile::TempDir::new().expect("dir");
        let failures = validate_rules(
            &[ValidationRule::ResponseContains {
                text: "created".to_string(),
            }],
            dir.path(),
            "success",
            "I've created the file successfully",
        );
        assert!(failures.is_empty());
    }

    #[test]
    fn test_create_scenario_workspace() {
        let setup = ScenarioSetup {
            files: vec![
                FileSetup {
                    path: "src/main.rs".to_string(),
                    content: "fn main() {}".to_string(),
                },
                FileSetup {
                    path: "src/lib.rs".to_string(),
                    content: "pub fn add(a: i32, b: i32) -> i32 { a + b }".to_string(),
                },
            ],
            permission_mode: "full_auto".to_string(),
        };

        let dir = create_scenario_workspace(&setup);
        assert!(dir.path().join("src/main.rs").exists());
        assert!(dir.path().join("src/lib.rs").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/main.rs")).unwrap(),
            "fn main() {}"
        );
    }

    #[test]
    fn test_yaml_to_mock_responses_sequence() {
        let turns = vec![
            MockTurn {
                response: MockResponseYaml {
                    response_type: "tool_use".to_string(),
                    content: String::new(),
                    tool: "Read".to_string(),
                    tool_id: "toolu_1".to_string(),
                    input: serde_json::json!({"path": "src/main.rs"}),
                },
            },
            MockTurn {
                response: MockResponseYaml {
                    response_type: "text".to_string(),
                    content: "The file looks good.".to_string(),
                    tool: String::new(),
                    tool_id: String::new(),
                    input: Value::Null,
                },
            },
        ];

        let mocks = yaml_to_mock_responses(&turns);
        assert_eq!(mocks.len(), 2);
        assert_eq!(mocks[0].stop_reason, "tool_use");
        assert_eq!(mocks[1].stop_reason, "end_turn");
    }

    #[test]
    fn test_parse_scenario_missing_name() {
        let f = write_temp_yaml(
            r#"
prompt: "test"
setup:
  files: []
mock_responses:
  - response:
      type: text
      content: "hi"
validate: []
"#,
        );
        // name defaults to empty string from serde, but our validation catches it
        // Actually serde requires the field (no #[serde(default)]), so this should fail
        let result = parse_scenario(f.path());
        // Either parse fails (no name field) or validation fails (empty name)
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_scenario_no_mock_responses() {
        let f = write_temp_yaml(
            r#"
name: empty
prompt: "test"
setup:
  files: []
mock_responses: []
validate: []
"#,
        );
        let result = parse_scenario(f.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no mock_responses"));
    }

    #[test]
    fn test_parse_scenarios_dir() {
        let dir = tempfile::TempDir::new().expect("dir");

        std::fs::write(
            dir.path().join("a.yaml"),
            r#"
name: scenario_a
prompt: "test a"
setup:
  files: []
mock_responses:
  - response:
      type: text
      content: "a"
validate: []
"#,
        )
        .expect("write a");

        std::fs::write(
            dir.path().join("b.yaml"),
            r#"
name: scenario_b
prompt: "test b"
setup:
  files: []
mock_responses:
  - response:
      type: text
      content: "b"
validate: []
"#,
        )
        .expect("write b");

        let scenarios = parse_scenarios_dir(dir.path()).expect("parse dir");
        assert_eq!(scenarios.len(), 2);
        assert_eq!(scenarios[0].1.name, "scenario_a");
        assert_eq!(scenarios[1].1.name, "scenario_b");
    }

    #[test]
    fn test_file_not_exists_rule() {
        let dir = tempfile::TempDir::new().expect("dir");

        let failures = validate_rules(
            &[ValidationRule::FileNotExists {
                path: "should_not_exist.txt".to_string(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert!(failures.is_empty());

        std::fs::write(dir.path().join("should_not_exist.txt"), "oops").expect("write");
        let failures = validate_rules(
            &[ValidationRule::FileNotExists {
                path: "should_not_exist.txt".to_string(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert_eq!(failures.len(), 1);
    }

    // ── W2-M1a behavioral rules ──────────────────────────────────────

    #[test]
    fn test_render_line_diff_variants() {
        // Identical texts produce an empty diff.
        assert_eq!(render_line_diff("same", "same"), "");

        // Creation: everything shows up as additions.
        assert_eq!(render_line_diff("", "alpha\nbeta"), "+alpha\n+beta");

        // Deletion: everything shows up as removals.
        assert_eq!(render_line_diff("alpha\nbeta", ""), "-alpha\n-beta");

        // Modification keeps context lines implicit and emits -/+ pairs.
        assert_eq!(
            render_line_diff("keep\nold tail\n", "keep\nnew tail\n"),
            "-old tail\n+new tail"
        );
    }

    #[test]
    fn test_tool_call_trace_from_mock_turns() {
        let turns = vec![
            MockTurn {
                response: MockResponseYaml {
                    response_type: "text_and_tool".to_string(),
                    content: "reading".to_string(),
                    tool: "Read".to_string(),
                    tool_id: "t1".to_string(),
                    input: serde_json::json!({"path": "a.rs"}),
                },
            },
            MockTurn {
                response: MockResponseYaml {
                    response_type: "text".to_string(),
                    content: "plain".to_string(),
                    tool: String::new(),
                    tool_id: String::new(),
                    input: Value::Null,
                },
            },
        ];

        let trace = ToolCallTrace::from_mock_turns(&turns);
        assert_eq!(trace.len(), 1, "text-only turns contribute no calls");
        assert_eq!(trace[0].tool, "Read");
        assert!(trace[0].input_json.contains(r#""path":"a.rs""#));
    }

    #[test]
    fn test_tool_call_trace_from_session_events() {
        use shannon_types::session_event::{
            SessionEvent, SessionEventBody, ToolCallPayload, UserMessagePayload,
        };

        let events = vec![
            SessionEvent::new(
                0,
                100,
                "s",
                1,
                SessionEventBody::UserMessage(UserMessagePayload {
                    source: UserMessagePayload::SOURCE_USER.into(),
                    content: "do it".into(),
                }),
            ),
            SessionEvent::new(
                1,
                101,
                "s",
                1,
                SessionEventBody::ToolCall(ToolCallPayload {
                    tool_use_id: "t1".into(),
                    tool_name: "Read".into(),
                    arguments: r#"{"path":"b.rs"}"#.into(),
                }),
            ),
            SessionEvent::new(
                2,
                102,
                "s",
                1,
                SessionEventBody::ToolCall(ToolCallPayload {
                    tool_use_id: "t2".into(),
                    tool_name: "Bash".into(),
                    arguments: r#"{"command":"ls"}"#.into(),
                }),
            ),
        ];

        let trace = ToolCallTrace::from_session_events(&events);
        assert_eq!(
            trace.iter().map(|c| c.tool.as_str()).collect::<Vec<_>>(),
            vec!["Read", "Bash"],
            "only tool/call rows become trajectory steps"
        );
        assert_eq!(trace[0].input_json, r#"{"path":"b.rs"}"#);
    }

    #[test]
    fn test_trajectory_contains_rule() {
        let observed = [
            ToolCallTrace::new("Read", r#"{"path":"src/main.rs"}"#),
            ToolCallTrace::new("Bash", r#"{"command":"cargo check"}"#),
            ToolCallTrace::new("Edit", r#"{"old_string":"Hello","new_string":"Goodbye"}"#),
        ];
        let dir = tempfile::TempDir::new().expect("dir");
        let ctx = ValidationContext::new(dir.path(), "success", "").with_trajectory(&observed);

        // Ordered subsequence with gaps allowed (Read ... Edit skipping Bash).
        let sequence = vec![
            TrajectoryStep {
                tool: "Read".to_string(),
                args_regex: r#""path":"src/main\.rs""#.to_string(),
                ..Default::default()
            },
            TrajectoryStep {
                tool: "Edit".to_string(),
                args_regex: String::new(),
                ..Default::default()
            },
        ];
        let outcomes = evaluate_rules(
            &[ValidationRule::TrajectoryContains {
                sequence: sequence.clone(),
            }],
            &ctx,
        );
        assert!(outcomes[0].passed, "{:?}", outcomes[0].details);

        // Wrong order: requiring Edit before Read cannot match.
        let reversed = vec![sequence[1].clone(), sequence[0].clone()];
        let outcomes = evaluate_rules(
            &[ValidationRule::TrajectoryContains { sequence: reversed }],
            &ctx,
        );
        assert!(!outcomes[0].passed);
        assert!(outcomes[0].details[0].starts_with("trajectory_contains: step"));

        // Absent tool never matches.
        let outcomes = evaluate_rules(
            &[ValidationRule::TrajectoryContains {
                sequence: vec![TrajectoryStep {
                    tool: "Grep".to_string(),
                    args_regex: String::new(),
                    ..Default::default()
                }],
            }],
            &ctx,
        );
        assert!(!outcomes[0].passed);

        // A malformed args_regex is reported instead of silently passing.
        let outcomes = evaluate_rules(
            &[ValidationRule::TrajectoryContains {
                sequence: vec![TrajectoryStep {
                    tool: "Read".to_string(),
                    args_regex: "([".to_string(),
                    ..Default::default()
                }],
            }],
            &ctx,
        );
        assert!(!outcomes[0].passed);

        assert!(outcomes[0].details[0].contains("invalid args_regex"));
    }

    #[test]
    fn trajectory_family_and_optional_step_semantics() {
        let tmp = tempfile::TempDir::new().unwrap();
        let observed = vec![
            ToolCallTrace::new("Grep", r#"{"pattern":"TODO"}"#),
            ToolCallTrace::new("rename_symbol", r#"{"old_name":"fetch_data"}"#),
        ];
        let ctx = ValidationContext::new(tmp.path(), "success", "").with_trajectory(&observed);

        // Family: any candidate tool satisfies the step (args_regex rides along).
        let family = ValidationRule::TrajectoryContains {
            sequence: vec![TrajectoryStep {
                tools: vec!["Edit".to_string(), "rename_symbol".to_string()],
                args_regex: r#""old_name":"fetch_data""#.to_string(),
                ..Default::default()
            }],
        };
        let outcomes = evaluate_rules(&[family], &ctx);
        assert!(outcomes[0].passed, "{:?}", outcomes[0].details);

        // Optional recon step is skipped when the stream never issues it.
        let optional_recon = ValidationRule::TrajectoryContains {
            sequence: vec![
                TrajectoryStep {
                    tool: "Read".to_string(),
                    optional: true,
                    ..Default::default()
                },
                TrajectoryStep {
                    tools: vec!["rename_symbol".to_string()],
                    ..Default::default()
                },
            ],
        };
        let outcomes = evaluate_rules(&[optional_recon], &ctx);
        assert!(outcomes[0].passed, "{:?}", outcomes[0].details);

        // Mandatory steps still fail when unmet — optionality never loosens
        // a required step, and family spelling shows up in the message.
        let missing = ValidationRule::TrajectoryContains {
            sequence: vec![TrajectoryStep {
                tools: vec!["Edit".to_string(), "MultiEdit".to_string()],
                args_regex: r#""old_string":"x""#.to_string(),
                ..Default::default()
            }],
        };
        let outcomes = evaluate_rules(&[missing], &ctx);
        assert!(!outcomes[0].passed);
        assert!(
            outcomes[0].details[0].contains("'Edit|MultiEdit'"),
            "{:?}",
            outcomes[0].details
        );
    }

    #[test]
    fn test_forbidden_tool_rule() {
        let observed = [ToolCallTrace::new("Bash", r#"{"command":"rm -rf /"}"#)];
        let dir = tempfile::TempDir::new().expect("dir");

        let clean = ValidationContext::new(dir.path(), "success", "");
        let outcomes = evaluate_rules(
            &[ValidationRule::ForbiddenTool {
                tool: "Bash".to_string(),
            }],
            &clean,
        );
        assert!(outcomes[0].passed);

        let dirty = ValidationContext::new(dir.path(), "success", "").with_trajectory(&observed);
        let outcomes = evaluate_rules(
            &[ValidationRule::ForbiddenTool {
                tool: "Bash".to_string(),
            }],
            &dirty,
        );
        assert!(!outcomes[0].passed);
        assert_eq!(outcomes[0].details[0], "forbidden_tool: 'Bash' was invoked");
    }

    #[test]
    fn test_cost_below_rule() {
        let costs = [0.01_f64, 0.02];
        let dir = tempfile::TempDir::new().expect("dir");
        let ctx = ValidationContext::new(dir.path(), "success", "").with_turn_costs_usd(&costs);

        // per=task: 0.03 total within budget passes.
        let outcomes = evaluate_rules(
            &[ValidationRule::CostBelow {
                max_usd: 0.05,
                per: CostBasis::Task,
            }],
            &ctx,
        );
        assert!(outcomes[0].passed);

        // per=task over budget fails with the running total in the message.
        let outcomes = evaluate_rules(
            &[ValidationRule::CostBelow {
                max_usd: 0.015,
                per: CostBasis::Task,
            }],
            &ctx,
        );
        assert!(!outcomes[0].passed);
        assert!(outcomes[0].details[0].contains("task total"));

        // per=turn flags individual violating turns only.
        let outcomes = evaluate_rules(
            &[ValidationRule::CostBelow {
                max_usd: 0.015,
                per: CostBasis::Turn,
            }],
            &ctx,
        );
        assert!(!outcomes[0].passed);
        assert!(outcomes[0].details.len() == 1 && outcomes[0].details[0].contains("turn 2"));

        // per=turn where every turn sits within the budget passes.
        let outcomes = evaluate_rules(
            &[ValidationRule::CostBelow {
                max_usd: 0.05,
                per: CostBasis::Turn,
            }],
            &ctx,
        );
        assert!(outcomes[0].passed);

        // Missing cost observations fail loudly rather than passing vacuously.
        let no_costs = ValidationContext::new(dir.path(), "success", "");
        let outcomes = evaluate_rules(
            &[ValidationRule::CostBelow {
                max_usd: 100.0,
                per: CostBasis::Task,
            }],
            &no_costs,
        );
        assert!(!outcomes[0].passed);
        assert!(outcomes[0].details[0].contains("no recorded turn costs"));
    }

    #[test]
    fn test_diff_matches_rule() {
        let dir = tempfile::TempDir::new().expect("dir");
        let files = vec![("src/lib.rs".to_string(), "fn old() {}".to_string())];

        // Created-from-scratch file: baseline absent means diff is pure additions.
        std::fs::write(dir.path().join("hello.txt"), "world").expect("write");
        let ctx = ValidationContext::new(dir.path(), "success", "").with_initial_files(&files);

        let outcomes = evaluate_rules(
            &[ValidationRule::DiffMatches {
                path: "hello.txt".to_string(),
                expected_diff_regex: r"^\+world$".to_string(),
            }],
            &ctx,
        );
        assert!(outcomes[0].passed, "{:?}", outcomes[0].details);

        // Modified file against its setup-time baseline.
        std::fs::create_dir_all(dir.path().join("src")).expect("create src dir");
        std::fs::write(dir.path().join("src/lib.rs"), "fn new() {}").expect("write");
        let outcomes = evaluate_rules(
            &[ValidationRule::DiffMatches {
                path: "src/lib.rs".to_string(),
                expected_diff_regex: r"-fn old\(\) \{\}\n\+fn new\(\) \{\}".to_string(),
            }],
            &ctx,
        );
        assert!(outcomes[0].passed, "{:?}", outcomes[0].details);

        // Wrong expectation reports a diff-mismatch violation.
        let outcomes = evaluate_rules(
            &[ValidationRule::DiffMatches {
                path: "src/lib.rs".to_string(),
                expected_diff_regex: r"\+pub mod lib;".to_string(),
            }],
            &ctx,
        );
        assert!(!outcomes[0].passed);
        assert!(outcomes[0].details[0].starts_with("diff_matches: src/lib.rs diff does not match"));

        // Missing target file fails explicitly.
        let outcomes = evaluate_rules(
            &[ValidationRule::DiffMatches {
                path: "ghost.rs".to_string(),
                expected_diff_regex: ".+".to_string(),
            }],
            &ctx,
        );
        assert!(!outcomes[0].passed);
        assert!(outcomes[0].details[0].contains("does not exist in workspace"));

        // Invalid pattern is reported, not treated as a non-match.
        let outcomes = evaluate_rules(
            &[ValidationRule::DiffMatches {
                path: "src/lib.rs".to_string(),
                expected_diff_regex: "([".to_string(),
            }],
            &ctx,
        );
        assert!(outcomes[0].details[0].contains("invalid regex"));
    }

    #[test]
    fn test_parse_behavioral_validation_rules() {
        let f = write_temp_yaml(
            r#"
name: behavioral
prompt: "Apply behavior"
setup:
  files: []
mock_responses:
  - response:
      type: text
      content: "done"
validate:
  - rule: diff_matches
    path: src/main.rs
    expected_diff_regex: '\+.*Goodbye'
  - rule: trajectory_contains
    sequence:
      - tool: Read
        args_regex: '"path":"src/main\.rs"'
      - tool: Edit
  - rule: forbidden_tool
    tool: Bash
  - rule: cost_below
    max_usd: 0.25
    per: task
"#,
        );
        let scenario = parse_scenario(f.path()).expect("parse behavioral scenario");
        assert_eq!(scenario.validate.len(), 4);
        assert!(matches!(
            &scenario.validate[0],
            ValidationRule::DiffMatches { path, expected_diff_regex }
                if path == "src/main.rs" && expected_diff_regex.contains("Goodbye")
        ));
        assert!(matches!(
            &scenario.validate[1],
            ValidationRule::TrajectoryContains { sequence }
                if sequence.len() == 2
                    && sequence[0].tool == "Read"
                    && !sequence[0].args_regex.is_empty()
                    && sequence[1].args_regex.is_empty()
        ));
        assert!(matches!(
            &scenario.validate[2],
            ValidationRule::ForbiddenTool { tool } if tool == "Bash"
        ));
        assert!(matches!(
            &scenario.validate[3],
            ValidationRule::CostBelow { max_usd, per: CostBasis::Task } if (*max_usd - 0.25).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn test_cost_below_rejects_unknown_basis() {
        let f = write_temp_yaml(
            r#"
name: bad-basis
prompt: "Budget"
setup:
  files: []
mock_responses:
  - response:
      type: text
      content: "done"
validate:
  - rule: cost_below
    max_usd: 1.0
    per: fortnight
"#,
        );
        assert!(
            parse_scenario(f.path()).is_err(),
            "unknown cost basis must be rejected at parse time"
        );
    }

    #[test]
    fn test_outcome_parity_with_legacy_strings() {
        let dir = tempfile::TempDir::new().expect("dir");

        // Legacy wrapper keeps producing the exact historical failure strings.
        let failures = validate_rules(
            &[ValidationRule::ExitCode {
                value: "success".to_string(),
            }],
            dir.path(),
            "error",
            "",
        );
        assert_eq!(failures, vec!["exit_code: expected 'success', got 'error'"]);

        // Two violated aspects of one file_content rule still yield two details.
        std::fs::write(dir.path().join("x.txt"), "unrelated").expect("write");
        let outcomes = evaluate_rules(
            &[ValidationRule::FileContent {
                path: "x.txt".to_string(),
                contains: "wanted".to_string(),
                matches_regex: r"^nomatch$".to_string(),
            }],
            &ValidationContext::new(dir.path(), "success", ""),
        );
        assert_eq!(outcomes[0].rule, "file_content");
        assert!(!outcomes[0].passed);
        assert_eq!(outcomes[0].details.len(), 2);
    }

    #[test]
    fn test_scenario_result_evaluated_derives_state() {
        let summary = TrajectorySummary::from_observations(
            &[ToolCallTrace::new("Read", "{}")],
            &[0.01, 0.02],
        );
        assert_eq!(summary.tool_calls, vec!["Read"]);
        assert_eq!(summary.turns, 2);
        assert!((summary.total_cost_usd - 0.03).abs() < f64::EPSILON);

        let good =
            ScenarioResult::evaluated("all-good", 12, vec![passed("exit_code")], summary.clone());
        assert!(good.passed);
        assert!(good.failures.is_empty());
        assert_eq!(good.trajectory_summary.turns, 2);

        let bad = ScenarioResult::evaluated(
            "some-bad",
            30,
            vec![
                passed("exit_code"),
                violated(
                    "forbidden_tool",
                    vec!["forbidden_tool: 'Bash' was invoked".into()],
                ),
            ],
            summary,
        );
        assert!(!bad.passed);
        assert_eq!(bad.failures, vec!["forbidden_tool: 'Bash' was invoked"]);
        assert_eq!(bad.rule_outcomes.len(), 2);
    }

    /// Build a `trajectory_contains` outcome for one observed call / one step.
    fn single_step_outcome(tool: &str, args_regex: &str, call: &ToolCallTrace) -> RuleOutcome {
        let dir = tempfile::TempDir::new().expect("dir");
        let observed = [call.clone()];
        let ctx = ValidationContext::new(dir.path(), "success", "").with_trajectory(&observed);
        evaluate_rules(
            &[ValidationRule::TrajectoryContains {
                sequence: vec![TrajectoryStep {
                    tool: tool.to_string(),
                    args_regex: args_regex.to_string(),
                    ..Default::default()
                }],
            }],
            &ctx,
        )
        .remove(0)
    }

    #[test]
    fn trajectory_args_regex_matches_absolute_path_for_relative_pattern() {
        // Absolute observed value, workspace-relative task pattern: the exact
        // shape of the path mismatch that caused the baseline false verdicts.
        let call = ToolCallTrace::new(
            "Write",
            r##"{"content":"# Changelog\n","file_path":"/tmp/run-abc/workspace/CHANGELOG.md"}"##,
        );
        let outcome = single_step_outcome("Write", r#""file_path":"CHANGELOG\.md""#, &call);
        assert!(outcome.passed, "{:?}", outcome.details);
    }

    #[test]
    fn trajectory_args_regex_basename_normalization() {
        // Basename variant satisfies a basename-spelled pattern regardless of
        // how deep the observed directory prefix is.
        let call = ToolCallTrace::new(
            "Edit",
            r#"{"file_path":"/home/u/proj/a/b/c/proxy.conf","old_string":"9100","new_string":"9443"}"#,
        );
        let outcome = single_step_outcome("Edit", r#""file_path":"proxy\.conf""#, &call);
        assert!(outcome.passed, "{:?}", outcome.details);

        // ...but a pattern naming a different file still fails: normalization
        // never fabricates a basename the call did not carry.
        let outcome = single_step_outcome("Edit", r#""file_path":"net\.rs""#, &call);
        assert!(!outcome.passed, "{:?}", outcome.details);
    }

    #[test]
    fn trajectory_args_regex_multi_segment_relative_pattern() {
        // Multi-segment relative pattern (e.g. rec_03's `src/api_options.rs`)
        // matches the matching suffix of an absolute observed path.
        let call = ToolCallTrace::new(
            "Read",
            r#"{"file_path":"/home/u/.shannon/runs/rec_03/workspace/src/api_options.rs"}"#,
        );
        let outcome = single_step_outcome("Read", r#""file_path":"src/api_options\.rs""#, &call);
        assert!(outcome.passed, "{:?}", outcome.details);

        // A different intermediate tail that does not end with the asserted
        // segment sequence must not match.
        let call = ToolCallTrace::new(
            "Read",
            r#"{"file_path":"/home/u/.shannon/runs/rec_03/workspace/docs/api_options.rs"}"#,
        );
        let outcome = single_step_outcome("Read", r#""file_path":"src/api_options\.rs""#, &call);
        assert!(!outcome.passed, "{:?}", outcome.details);
    }

    #[test]
    fn trajectory_args_regex_leaves_non_path_fields_verbatim() {
        // `command` contains slashes and a shared basename, but it is not a
        // path-class key — no suffix normalization may rescue the pattern.
        let call = ToolCallTrace::new("Bash", r#"{"command":"cat /workspace/notes/CHANGELOG.md"}"#);
        let outcome = single_step_outcome("Bash", r#""command":"CHANGELOG\.md""#, &call);
        assert!(!outcome.passed, "{:?}", outcome.details);

        // `old_string`/`new_string` are not path fields either (edit_03-style
        // granularity assertions keep their exact semantics).
        let call = ToolCallTrace::new(
            "Edit",
            r#"{"file_path":"/w/src/main.rs","old_string":"a/b/c","new_string":"x"}"#,
        );
        let outcome = single_step_outcome("Edit", r#""old_string":"c""#, &call);
        assert!(!outcome.passed, "{:?}", outcome.details);

        // A path pattern over a non-matching path still fails: normalization
        // loosens the prefix, not the asserted value.
        let call = ToolCallTrace::new("Read", r#"{"file_path":"/w/src/main.rs"}"#);
        let outcome = single_step_outcome("Read", r#""file_path":"src/other\.rs""#, &call);
        assert!(!outcome.passed, "{:?}", outcome.details);
    }

    #[test]
    fn trajectory_args_regex_verbatim_path_still_matches() {
        // Relative observed value against the identical relative pattern —
        // the pre-normalization behavior — keeps matching untouched.
        let call = ToolCallTrace::new("Read", r#"{"path":"src/main.rs"}"#);
        let outcome = single_step_outcome("Read", r#""path":"src/main\.rs""#, &call);
        assert!(outcome.passed, "{:?}", outcome.details);
    }

    #[test]
    fn trajectory_args_regex_matches_real_baseline_run_samples() {
        // Inputs captured verbatim from the glm-5.3-flash official baseline
        // run `~/.shannon/eval/v1-official/20260827175908-6d21325f/` (multi_02
        // stream.ndjson Write + rec_03 l0 events.jsonl Read/Edit); the
        // patterns are the original, unmodified task.toml args_regex values.
        // Before path normalization every one of these steps was a false
        // "not found" verdict against content-perfect runs.
        let rec_03_read = ToolCallTrace::new(
            "Read",
            r#"{"file_path":"/home/ed/.shannon/eval/v1-official/20260827175908-6d21325f/rec_03/workspace/src/api_options.rs"}"#,
        );
        let rec_03_pattern = r#""file_path":"src/api_options.rs""#;

        // Sanity: the raw input alone genuinely misses the old regex — the
        // normalization is what closes the gap.
        let raw = Regex::new(rec_03_pattern)
            .expect("valid pattern")
            .is_match(rec_03_read.input_json.as_str());
        assert!(!raw, "raw absolute input must miss the relative pattern");
        let outcome = single_step_outcome("Read", rec_03_pattern, &rec_03_read);
        assert!(outcome.passed, "{:?}", outcome.details);

        // multi_02 Write with leading content field, absolute path tail.
        let multi_02_write = ToolCallTrace::new(
            "Write",
            r##"{"content":"# Changelog\n- fixed the login crash on empty username\n- added retry logic to sync jobs\n","file_path":"/home/ed/.shannon/eval/v1-official/20260827175908-6d21325f/multi_02/workspace/CHANGELOG.md"}"##,
        );
        let outcome =
            single_step_outcome("Write", r#""file_path":"CHANGELOG.md""#, &multi_02_write);
        assert!(outcome.passed, "{:?}", outcome.details);

        // Full rec_03-shaped ordered subsequence: Read (path pattern) then
        // Edit (no pattern) — previously step 1 starved the pointer and step 2
        // was misreported "not found" despite having happened.
        let rec_03_edit = ToolCallTrace::new(
            "Edit",
            r#"{"file_path":"/home/ed/.shannon/eval/v1-official/20260827175908-6d21325f/rec_03/workspace/src/api_options.rs","new_string":"pub fn timeout_seconds() -> u64 {\n    30\n}\n\npub fn request_timeout_ms() -> u64 {\n    30000\n}","old_string":"<<<<<<< HEAD\npub fn timeout_seconds() -> u64 {\n    30\n}\n=======\npub fn request_timeout_ms() -> u64 {\n    30000\n}\n>>>>>>> feature/tuning","replace_all":false}"#,
        );
        let observed = [rec_03_read.clone(), rec_03_edit];
        let dir = tempfile::TempDir::new().expect("dir");
        let ctx = ValidationContext::new(dir.path(), "success", "").with_trajectory(&observed);
        let outcomes = evaluate_rules(
            &[ValidationRule::TrajectoryContains {
                sequence: vec![
                    TrajectoryStep {
                        tool: "Read".to_string(),
                        args_regex: rec_03_pattern.to_string(),
                        ..Default::default()
                    },
                    TrajectoryStep {
                        tool: "Edit".to_string(),
                        ..Default::default()
                    },
                ],
            }],
            &ctx,
        );
        assert!(outcomes[0].passed, "{:?}", outcomes[0].details);
    }
}
