//! # Permission System
//!
//! Security and permission validation for tool execution and resource access.

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

use crate::permission_profile::{PermissionProfile, ProfileRules};

/// Errors that can occur during permission validation
#[derive(Error, Debug)]
pub enum PermissionError {
    #[error("Permission denied: {0}")]
    Denied(String),

    /// The session hit the configured auto-approval budget
    /// (`permissions.max_auto_approvals`) and needs a human decision.
    /// Headless surfaces map this to exit code 7.
    #[error("Auto-approval limit reached: {0}")]
    AutoApprovalLimit(String),

    #[error("Invalid permission: {0}")]
    InvalidPermission(String),

    #[error("Permission not found: {0}")]
    NotFound(String),
}

/// Returns true if the tool name corresponds to a read-only operation (no side effects).
/// Used by both `Readonly` mode enforcement and `Suggest` mode auto-approval.
fn is_read_only_tool_name(tool_name: &str) -> bool {
    // Case-insensitive: the model-facing registry uses capitalized display
    // names ("Read", "Grep", "WebFetch") while this list was lowercase-only,
    // which silently broke Suggest/Readonly/PlanReadonly fast paths.
    let lower = tool_name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "read"
            | "read_file"
            | "search"
            | "grep"
            | "glob"
            | "list_directory"
            | "list_dir"
            | "ls"
            | "file_tree"
            | "file_info"
            | "git_log"
            | "git_diff"
            | "git_status"
            | "git_branch_show"
            | "web_search"
            | "web_fetch"
            | "lsp_hover"
            | "lsp_definition"
            | "lsp_references"
            | "lsp_diagnostics"
            | "lsp_document_symbols"
            | "lsp_workspace_symbols"
    )
}

/// Risk level of a tool operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RiskLevel {
    /// Safe operation (e.g., read-only)
    Safe = 0,
    /// Low risk (e.g., write to allowed paths)
    Low = 1,
    /// Medium risk (e.g., network requests)
    Medium = 2,
    /// High risk (e.g., file deletion)
    High = 3,
    /// Critical (e.g., system modification)
    Critical = 4,
}

/// User's choice for a permission prompt
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionChoice {
    /// Deny this operation
    Deny,
    /// Allow this once
    AllowOnce,
    /// Always allow this tool
    AlwaysAllow,
    /// Open in editor to modify before running
    EditAndRun,
}

/// Approval policy mode controlling how tool execution is authorized.
///
/// The user-facing model is 4+3 (see
/// `docs/plans/2026-10-04-permission-mode-naming-design.md`):
///
/// Autonomy ladder (Shift+Tab cycles the first three):
/// - `ask`       → `Ask`:        reads auto-approved, everything else prompts
/// - `auto-edit` → `AutoEdit`:   file edits auto-approved, commands prompt
/// - `full-auto` → `FullAuto`:   everything below Critical auto-approved
///
/// Workflow tier (entered via `/plan`, never in the cycle):
/// - `plan`      → `Plan`:       read-only until the plan is approved, then
///   plan-scoped auto-run; exit restores the snapshotted autonomy mode
///
/// Expert modes (explicit `/mode` only):
/// - `readonly`  → `Readonly`:   read-only analysis, everything else denied
/// - `dontAsk`   → `DontAsk`:    never waits — allow rules / reads pass,
///   everything else is DENIED (CI posture)
/// - `bypassPermissions` → `BypassPermissions`: no checks (deny rules
///   still apply); guardrailed at the CLI
///
/// Compatibility aliases: Claude Code's `default` and `acceptEdits` parse to
/// `Ask` / `AutoEdit`; the legacy `auto` spelling keeps pointing at
/// `AutoEdit` (its historical Display name); `classifier` spellings map to
/// the conservative `Ask` (the old `Auto` variant was removed — the
/// classifier now serves as the decision engine inside the auto modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ApprovalMode {
    /// Reads are auto-approved; every other tool asks for confirmation.
    /// Aliases: "default" (Claude Code), "suggest" (legacy Shannon).
    #[serde(alias = "Suggest", alias = "Auto")]
    Ask,
    /// Plan mode: read-only analysis until the plan is approved; after
    /// approval, tool calls inside the plan run without prompting.
    /// Claude Code alias: "plan"
    Plan,
    /// Auto-approve file operations (edit, write) at Medium risk or below;
    /// ask for bash and other risky tools.
    /// Claude Code alias: "acceptEdits"; legacy Shannon display: "auto"
    #[default]
    AutoEdit,
    /// Auto-approve everything except critical-risk operations.
    /// Codex CLI preset alias: "full-auto"
    FullAuto,
    /// Skip all permission checks (deny rules still apply). Use with extreme
    /// caution. Claude Code alias: "bypassPermissions"
    BypassPermissions,
    /// Never wait for confirmation: allow rules and read-only tools pass,
    /// everything else is denied. Designed for CI / unattended runs.
    /// Claude Code alias: "dontAsk"
    DontAsk,
    /// Only allow read operations — no writes, no bash.
    /// Absorbs the removed `PlanReadonly` variant.
    #[serde(alias = "PlanReadonly")]
    Readonly,
}

impl std::fmt::Display for ApprovalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ask => write!(f, "ask"),
            Self::Plan => write!(f, "plan"),
            Self::AutoEdit => write!(f, "auto-edit"),
            Self::FullAuto => write!(f, "full-auto"),
            Self::BypassPermissions => write!(f, "bypassPermissions"),
            Self::DontAsk => write!(f, "dontAsk"),
            Self::Readonly => write!(f, "readonly"),
        }
    }
}

impl ApprovalMode {
    /// Parse from string (case-insensitive). Accepts Shannon tokens plus the
    /// compatibility aliases listed on the enum.
    pub fn from_str_ci(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "ask" | "default" | "suggest" | "manual" => Some(Self::Ask),
            // Legacy: the removed classifier mode maps conservatively to Ask.
            "auto-classifier" | "auto_classifier" | "classifier" => Some(Self::Ask),
            "plan" => Some(Self::Plan),
            "auto-edit" | "auto_edit" | "autoedit" | "acceptedits" | "accept-edits" | "auto" => {
                Some(Self::AutoEdit)
            }
            "full-auto" | "full_auto" | "fullauto" | "full" => Some(Self::FullAuto),
            "bypasspermissions" | "bypass_permissions" | "bypass-permissions" | "bypass"
            | "full-access" | "fullaccess" => Some(Self::BypassPermissions),
            "dontask" | "dont_ask" | "dont-ask" | "ci" => Some(Self::DontAsk),
            "readonly" | "read-only" | "read_only" | "ro" | "plan-readonly" | "plan_readonly"
            | "plan_ro" | "plan-ro" | "planro" | "planreadonly" => Some(Self::Readonly),
            _ => None,
        }
    }

    /// Returns all variant tokens for display, in ladder order.
    pub fn all_names() -> &'static [&'static str] {
        &[
            "ask",
            "auto-edit",
            "full-auto",
            "plan",
            "readonly",
            "dontAsk",
            "bypassPermissions",
        ]
    }

    /// Cycle to the next autonomy mode (Shift+Tab pattern).
    /// Cycles through: Ask → AutoEdit → FullAuto → Ask.
    /// `Plan` and the expert modes (`Readonly`/`DontAsk`/`BypassPermissions`)
    /// are never cycle stops — the UI exits plan mode on the first Shift+Tab
    /// and resets expert modes to `Ask` here.
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Ask => Self::AutoEdit,
            Self::AutoEdit => Self::FullAuto,
            Self::FullAuto => Self::Ask,
            Self::Plan | Self::Readonly | Self::DontAsk | Self::BypassPermissions => Self::Ask,
        }
    }

    /// Short label for display in the status bar. Bijective with the variant
    /// set — the REPL stores the enum itself, labels are display-only.
    pub fn short_label(&self) -> &'static str {
        match self {
            Self::Ask => "ASK",
            Self::AutoEdit => "EDIT",
            Self::FullAuto => "FULL",
            Self::Plan => "PLAN",
            Self::Readonly => "RO",
            Self::DontAsk => "CI",
            Self::BypassPermissions => "BYPASS",
        }
    }

    /// Reverse-lookup from a status-bar label.
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "ASK" => Some(Self::Ask),
            "EDIT" => Some(Self::AutoEdit),
            "FULL" => Some(Self::FullAuto),
            "PLAN" => Some(Self::Plan),
            "RO" => Some(Self::Readonly),
            "CI" => Some(Self::DontAsk),
            "BYPASS" => Some(Self::BypassPermissions),
            _ => None,
        }
    }

    /// Description of this mode for help text.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Ask => "Reads run freely; every other tool asks first",
            Self::AutoEdit => "File edits run without asking; commands still ask",
            Self::FullAuto => "Everything below critical risk runs automatically",
            Self::Plan => "Read-only until the plan is approved, then auto-run within the plan",
            Self::Readonly => "Only read operations — no writes, no bash",
            Self::DontAsk => "Never waits: allow rules and reads pass, the rest is denied (CI)",
            Self::BypassPermissions => {
                "Skip all checks except deny rules (dangerous, trusted env only)"
            }
        }
    }

    /// Whether a tool should be auto-approved under this mode.
    pub fn should_auto_approve(&self, tool_name: &str, risk_level: RiskLevel) -> bool {
        match self {
            Self::Ask => {
                // Auto-approve read-only tools at Low/Safe risk (matching Claude Code behavior)
                is_read_only_tool_name(tool_name) && risk_level <= RiskLevel::Low
            }
            Self::Plan => false,
            Self::AutoEdit => {
                // Auto-approve file operations; ask for everything else.
                // Case-insensitive for the same reason as
                // `is_read_only_tool_name` above.
                let lower = tool_name.to_ascii_lowercase();
                let is_file_tool = matches!(
                    lower.as_str(),
                    "edit" | "write" | "create_file" | "replace" | "file_edit" | "multiedit"
                );
                is_file_tool && risk_level <= RiskLevel::Medium
            }
            Self::FullAuto => risk_level < RiskLevel::Critical,
            Self::BypassPermissions | Self::DontAsk => true,
            Self::Readonly => false, // handled at a higher level
        }
    }
}

/// Decision for a permission rule (distinct from classifier's RuleDecision)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PermissionRuleDecision {
    /// Automatically allow the operation
    Allow,
    /// Automatically deny the operation
    Deny,
    /// Ask the user for confirmation
    Ask,
}

/// Source of a permission rule (distinct from classifier's RuleSource)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PermissionRuleSource {
    /// User-configured rule
    User,
    /// Project-configured rule
    Project,
    /// Local/personal rule (from settings.local.json, highest file priority)
    Local,
    /// Managed/system rule
    Managed,
}

/// A permission rule for matching tool commands (glob-style patterns)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    /// Glob-style pattern for matching tool commands (e.g., "Bash(git *)", "Read(*)")
    pub pattern: String,
    /// Decision to make when this rule matches
    pub decision: PermissionRuleDecision,
    /// Source of this rule
    pub source: PermissionRuleSource,
    /// Optional description of why this rule exists
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl PermissionRule {
    /// Create a new permission rule
    pub fn new(
        pattern: String,
        decision: PermissionRuleDecision,
        source: PermissionRuleSource,
    ) -> Self {
        Self {
            pattern,
            decision,
            source,
            description: None,
        }
    }

    /// Create a new permission rule with a description
    pub fn with_description(
        pattern: String,
        decision: PermissionRuleDecision,
        source: PermissionRuleSource,
        description: String,
    ) -> Self {
        Self {
            pattern,
            decision,
            source,
            description: Some(description),
        }
    }

    /// Check if this rule matches the given tool name and command
    pub fn matches(&self, tool_name: &str, command: &str) -> bool {
        // Pattern format: "ToolName(pattern)" or just "ToolName"
        if let Some((tool_pattern, cmd_pattern)) = self.pattern.split_once('(') {
            // Strip trailing ')'
            let cmd_pattern = cmd_pattern.strip_suffix(')').unwrap_or(cmd_pattern);

            // Check if tool name matches
            if tool_pattern != "*" && !tool_name.eq_ignore_ascii_case(tool_pattern) {
                return false;
            }

            // Check if command matches the pattern
            if cmd_pattern == "*" || cmd_pattern == "**" {
                return true;
            }

            // Simple glob matching for command
            if cmd_pattern.contains('*') {
                // Convert glob to simple regex
                let regex_pattern = regex::escape(cmd_pattern).replace("\\*", ".*");
                if let Ok(re) = regex::Regex::new(&format!("^{regex_pattern}$")) {
                    re.is_match(command)
                } else {
                    // Fallback to contains check
                    command.contains(&cmd_pattern.replace('*', ""))
                }
            } else {
                command.contains(cmd_pattern)
            }
        } else {
            // No command pattern, just match tool name
            self.pattern.eq_ignore_ascii_case(tool_name) || self.pattern == "*"
        }
    }
}

/// A set of permission rules with ordered evaluation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionRuleSet {
    /// Ordered list of rules (first match wins)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    rules: Vec<PermissionRule>,
}

impl PermissionRuleSet {
    /// Create a new empty rule set
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a rule to the set
    pub fn add_rule(&mut self, rule: PermissionRule) {
        self.rules.push(rule);
    }

    /// Add a rule with builder-style pattern
    pub fn with_rule(mut self, rule: PermissionRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Evaluate rules for a given tool and command
    /// Returns the first matching rule's decision, or None if no rules match
    pub fn evaluate(&self, tool_name: &str, command: &str) -> Option<PermissionRuleDecision> {
        for rule in &self.rules {
            if rule.matches(tool_name, command) {
                return Some(rule.decision);
            }
        }
        None
    }

    /// Get all rules in the set
    pub fn rules(&self) -> &[PermissionRule] {
        &self.rules
    }

    /// Clear all rules
    pub fn clear(&mut self) {
        self.rules.clear();
    }

    /// Remove rules by source
    pub fn remove_by_source(&mut self, source: &PermissionRuleSource) {
        self.rules.retain(|r| &r.source != source);
    }
}

/// Decision from evaluating permission rules against a tool/command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleCheckDecision {
    /// The tool/command is denied — do not execute.
    Denied,
    /// The tool/command requires explicit user approval.
    Ask,
    /// The tool/command is auto-approved.
    Allowed,
    /// No rule matched — fall through to approval mode logic.
    NoMatch,
}

/// Checks tool/command access using deny > ask > allow priority rules.
///
/// Rules are loaded from settings and compiled into
/// glob-based matchers. The evaluation order guarantees:
/// 1. If any **deny** pattern matches, the result is `Denied`.
/// 2. If any **ask** pattern matches (and no deny matched), the result is `Ask`.
/// 3. If any **allow** pattern matches (and no deny/ask matched), the result is `Allowed`.
/// 4. Otherwise, `NoMatch` is returned.
///
/// This ordering means a deny rule in *any* layer (user, project, or local)
/// cannot be overridden by an allow rule in a higher-priority layer.
#[derive(Debug, Clone, Default)]
pub struct PermissionRuleChecker {
    deny_globset: Option<GlobSet>,
    ask_globset: Option<GlobSet>,
    allow_globset: Option<GlobSet>,
    deny_raw: Vec<String>,
    ask_raw: Vec<String>,
    allow_raw: Vec<String>,
}

impl PermissionRuleChecker {
    /// Build a checker from raw rule string lists (deny, ask, allow).
    ///
    /// This is the engine-level constructor that avoids coupling to the
    /// `settings::PermissionRules` type (which remains in `shannon-core`).
    /// Callers in `shannon-core` can use the
    /// `PermissionRuleCheckerExt::from_rules` extension trait method which
    /// bridges from `settings::PermissionRules` for backward compatibility.
    pub fn from_rule_strings(deny: &[String], ask: &[String], allow: &[String]) -> Self {
        Self {
            deny_globset: build_globset(deny),
            ask_globset: build_globset(ask),
            allow_globset: build_globset(allow),
            deny_raw: deny.to_vec(),
            ask_raw: ask.to_vec(),
            allow_raw: allow.to_vec(),
        }
    }

    /// Check a tool name and optional command against the rules.
    pub fn check(&self, tool_name: &str, command: &str) -> RuleCheckDecision {
        self.check_with_rule(tool_name, command).0
    }

    /// Check a tool name and optional command, also returning the matched
    /// rule pattern (P1-3). The decision semantics are identical to
    /// [`Self::check`] — the pattern is informational only, surfaced to the
    /// user as the reason a prompt was raised. `None` when no rule matched
    /// (or the matcher hit a compiled glob with no raw pattern text).
    pub fn check_with_rule(
        &self,
        tool_name: &str,
        command: &str,
    ) -> (RuleCheckDecision, Option<String>) {
        // 1. Deny has highest priority
        if let Some(rule) =
            self.match_pattern(tool_name, command, &self.deny_raw, &self.deny_globset)
        {
            return (RuleCheckDecision::Denied, rule);
        }
        // 2. Ask is next
        if let Some(rule) = self.match_pattern(tool_name, command, &self.ask_raw, &self.ask_globset)
        {
            return (RuleCheckDecision::Ask, rule);
        }
        // 3. Allow
        if let Some(rule) =
            self.match_pattern(tool_name, command, &self.allow_raw, &self.allow_globset)
        {
            return (RuleCheckDecision::Allowed, rule);
        }
        // 4. No rule matched
        (RuleCheckDecision::NoMatch, None)
    }

    /// Check if the raw rules are all empty (nothing to check).
    pub fn is_empty(&self) -> bool {
        self.deny_raw.is_empty() && self.ask_raw.is_empty() && self.allow_raw.is_empty()
    }

    /// Test a tool/command against a set of patterns.
    ///
    /// Returns `None` when nothing matched, otherwise `Some(matched_pattern)`
    /// where the inner option carries the raw pattern text when the match came
    /// from the raw list (compiled-glob-only matches have no raw text to show).
    /// Decision semantics are identical to the previous boolean version.
    fn match_pattern(
        &self,
        tool_name: &str,
        command: &str,
        raw_patterns: &[String],
        globset: &Option<GlobSet>,
    ) -> Option<Option<String>> {
        // First check structured patterns (ToolName(cmd_pattern) form)
        for pattern in raw_patterns {
            if let Some((tool_pat, cmd_pat)) = pattern.split_once('(') {
                let cmd_pat = cmd_pat.strip_suffix(')').unwrap_or(cmd_pat);
                // Check tool name
                if tool_pat != "*" && !tool_name.eq_ignore_ascii_case(tool_pat) {
                    continue;
                }
                // Check command pattern
                if cmd_pat == "*" || cmd_pat == "**" || self.command_matches(command, cmd_pat) {
                    return Some(Some(pattern.clone()));
                }
            }
            // Bare tool name or glob-only pattern
            else if pattern.eq_ignore_ascii_case(tool_name) || pattern == "*" {
                return Some(Some(pattern.clone()));
            }
        }

        // Then check globset for plain glob patterns (e.g., "mcp__server__*")
        if let Some(gs) = globset {
            if gs.is_match(tool_name) {
                return Some(None);
            }
        }

        None
    }

    /// Simple glob matching for command strings.
    fn command_matches(&self, command: &str, pattern: &str) -> bool {
        if !pattern.contains('*') {
            return command.contains(pattern);
        }
        // Convert glob to regex
        let regex_pattern = regex::escape(pattern).replace("\\*", ".*");
        if let Ok(re) = regex::Regex::new(&format!("(?i)^{regex_pattern}$")) {
            re.is_match(command)
        } else {
            command.contains(&pattern.replace('*', ""))
        }
    }
}

/// Where a permission decision's explanation came from (P1-3).
///
/// Purely informational — it never feeds back into the decision itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasonSource {
    /// A settings/profile permission rule matched (`Bash(git *)`, `mcp__x__*`, …).
    Rule,
    /// The LLM safety classifier was consulted and produced the verdict.
    Llm,
    /// Nothing more specific is known — approval-mode / policy default.
    Default,
}

/// Why a permission prompt was raised. Attached to every
/// [`PermissionPrompt`] so the desktop approval dialog can show the user
/// the deciding rule name / classifier confidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionReason {
    pub source: ReasonSource,
    /// The matched rule pattern, when the decision came from a named rule.
    pub rule_name: Option<String>,
    /// Classifier confidence in `0.0..=1.0`, when a classifier decided.
    pub confidence: Option<f32>,
}

impl DecisionReason {
    /// A rule-driven reason: `source = Rule` with the matched pattern.
    pub fn rule(rule_name: impl Into<String>) -> Self {
        Self {
            source: ReasonSource::Rule,
            rule_name: Some(rule_name.into()),
            confidence: None,
        }
    }

    /// A rule-driven reason that also carries the classifier's confidence.
    pub fn rule_with_confidence(rule_name: Option<String>, confidence: f32) -> Self {
        Self {
            source: ReasonSource::Rule,
            rule_name,
            confidence: Some(confidence),
        }
    }

    /// An LLM-classifier-driven reason (confidence always known).
    pub fn llm(confidence: f32) -> Self {
        Self {
            source: ReasonSource::Llm,
            rule_name: None,
            confidence: Some(confidence),
        }
    }

    /// The fallback reason when nothing specific decided the prompt.
    pub fn default_reason() -> Self {
        Self {
            source: ReasonSource::Default,
            rule_name: None,
            confidence: None,
        }
    }
}

impl Default for DecisionReason {
    fn default() -> Self {
        Self::default_reason()
    }
}

/// A prompt requesting user permission for a tool operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionPrompt {
    /// Unique ID for this prompt
    pub id: uuid::Uuid,
    /// Tool name being executed
    pub tool_name: String,
    /// Tool input/arguments
    pub tool_input: serde_json::Value,
    /// Risk level of this operation
    pub risk_level: RiskLevel,
    /// Human-readable description of the operation
    pub description: String,
    /// Whether this is a confirmation (already approved conceptually)
    pub is_confirmation: bool,
    /// Optional diff preview for file edit/write operations
    pub diff_preview: Option<String>,
    /// Whether this tool is flagged as destructive (MCP `destructiveHint`).
    /// Destructive tools always require confirmation and show a warning.
    pub is_destructive: bool,
    /// Explanation of why this risk level was assigned
    pub risk_reason: String,
    /// P1-3: why this prompt was raised (rule hit / LLM verdict / default).
    /// Informational only — never consulted when deciding.
    #[serde(default)]
    pub reason: DecisionReason,
    /// P3-2: this prompt exists because the session's auto-approval budget
    /// (`max_auto_approvals`) ran out. Surfaces use it to label the dialog
    /// and headless paths map it to exit code 7.
    #[serde(default)]
    pub limit_triggered: bool,
}

impl PermissionPrompt {
    /// Create a new permission prompt
    pub fn new(
        tool_name: String,
        tool_input: serde_json::Value,
        risk_level: RiskLevel,
        description: String,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            tool_name,
            tool_input,
            risk_level,
            description,
            is_confirmation: false,
            diff_preview: None,
            is_destructive: false,
            risk_reason: String::new(),
            reason: DecisionReason::default(),
            limit_triggered: false,
        }
    }

    /// Create a confirmation prompt (lower visual urgency)
    pub fn confirmation(tool_name: String, description: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            tool_name,
            tool_input: serde_json::json!({}),
            risk_level: RiskLevel::Safe,
            description,
            is_confirmation: true,
            diff_preview: None,
            is_destructive: false,
            risk_reason: String::new(),
            reason: DecisionReason::default(),
            limit_triggered: false,
        }
    }

    /// Get a formatted display string for the prompt
    pub fn display_text(&self) -> String {
        let risk_indicator = match self.risk_level {
            RiskLevel::Safe => "✓",
            RiskLevel::Low => "⚠",
            RiskLevel::Medium => "⚡",
            RiskLevel::High => "🔥",
            RiskLevel::Critical => "☢️",
        };

        format!(
            "{} {} - {}\nInput: {}",
            risk_indicator,
            self.tool_name,
            self.description,
            serde_json::to_string_pretty(&self.tool_input)
                .unwrap_or_else(|_| "(invalid input)".to_string())
        )
    }
}

/// Policy for a specific tool's permission requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPermissionPolicy {
    /// Tool name this policy applies to
    pub tool_name: String,
    /// Default risk level for operations
    pub default_risk_level: RiskLevel,
    /// Requires confirmation for these input patterns
    pub confirmation_patterns: Vec<String>,
    /// Always deny patterns (dangerous regardless of user approval)
    pub deny_patterns: Vec<String>,
    /// Description for users
    pub description: String,
}

impl ToolPermissionPolicy {
    /// Create a new tool permission policy
    pub fn new(tool_name: String, default_risk_level: RiskLevel, description: String) -> Self {
        Self {
            tool_name,
            default_risk_level,
            confirmation_patterns: Vec::new(),
            deny_patterns: Vec::new(),
            description,
        }
    }

    /// Add a pattern that requires explicit confirmation
    pub fn add_confirmation_pattern(mut self, pattern: &str) -> Self {
        self.confirmation_patterns.push(pattern.to_string());
        self
    }

    /// Add a pattern that is always denied (dangerous)
    pub fn add_deny_pattern(mut self, pattern: &str) -> Self {
        self.deny_patterns.push(pattern.to_string());
        self
    }

    /// Check if the given input matches any deny pattern
    pub fn is_denied(&self, input_str: &str) -> bool {
        self.deny_patterns.iter().any(|pattern| {
            if pattern.contains('*') {
                Self::wildcard_matches(pattern, input_str)
            } else {
                input_str.contains(pattern)
            }
        })
    }

    /// Check if the given input requires confirmation
    pub fn requires_confirmation(&self, input_str: &str) -> bool {
        self.confirmation_patterns.iter().any(|pattern| {
            if pattern.contains('*') {
                Self::wildcard_matches(pattern, input_str)
            } else {
                input_str.contains(pattern)
            }
        })
    }

    /// Match a glob-style wildcard pattern against input using regex.
    fn wildcard_matches(pattern: &str, input: &str) -> bool {
        let regex_pattern = format!("(?i)^{}$", regex::escape(pattern).replace("\\*", ".*"));
        regex::Regex::new(&regex_pattern)
            .map(|re| re.is_match(input))
            .unwrap_or(false)
    }

    /// Get the risk level for a specific input
    pub fn risk_level_for(&self, input_str: &str) -> RiskLevel {
        if self.is_denied(input_str) {
            RiskLevel::Critical
        } else if self.requires_confirmation(input_str) {
            RiskLevel::Medium
        } else {
            self.default_risk_level
        }
    }
}

/// Memory of user permission choices (persists across prompts)
#[derive(Debug, Clone, Default)]
pub struct PermissionMemory {
    /// Always-allowed tools (exact match)
    always_allowed: HashSet<String>,
    /// Always-denied tools (exact match)
    always_denied: HashSet<String>,
    /// Glob patterns for auto-allowed tools (e.g., `mcp__server__*`)
    allowed_patterns: Vec<String>,
    /// Compiled glob set for allowed patterns (rebuilt when patterns change)
    allowed_globset: Option<GlobSet>,
    /// Glob patterns for always-denied tools
    denied_patterns: Vec<String>,
    /// Compiled glob set for denied patterns
    denied_globset: Option<GlobSet>,
    /// Session-specific choices
    session_choices: HashMap<uuid::Uuid, HashMap<String, PermissionChoice>>,
}

impl PermissionMemory {
    /// Create a new permission memory
    pub fn new() -> Self {
        Self {
            always_allowed: HashSet::new(),
            always_denied: HashSet::new(),
            allowed_patterns: Vec::new(),
            allowed_globset: None,
            denied_patterns: Vec::new(),
            denied_globset: None,
            session_choices: HashMap::new(),
        }
    }

    /// Check if a tool is always allowed for this session
    pub fn is_always_allowed(&self, session_id: uuid::Uuid, tool_name: &str) -> bool {
        // Fast path: exact match
        if self.always_allowed.contains(tool_name) {
            return true;
        }
        // Session-specific exact match
        if self
            .session_choices
            .get(&session_id)
            .and_then(|choices| choices.get(tool_name))
            .map(|choice| choice == &PermissionChoice::AlwaysAllow)
            .unwrap_or(false)
        {
            return true;
        }
        // Glob pattern match
        if let Some(ref globset) = self.allowed_globset {
            if globset.is_match(tool_name) {
                return true;
            }
        }
        false
    }

    /// Check if a tool is always denied
    pub fn is_always_denied(&self, tool_name: &str) -> bool {
        // Fast path: exact match
        if self.always_denied.contains(tool_name) {
            return true;
        }
        // Glob pattern match
        if let Some(ref globset) = self.denied_globset {
            if globset.is_match(tool_name) {
                return true;
            }
        }
        false
    }

    /// Remember a user's permission choice.
    ///
    /// review §P1-1: AlwaysAllow no longer inserts the bare tool name into the
    /// process-wide `always_allowed` set — that made any Bash command auto-
    /// approve in any session once the user had approved one (ls, etc.). Now
    /// `AlwaysAllow` is recorded as a per-session choice, and the underlying
    /// rule-checker receives a `(tool, command_prefix)` rule from the caller
    /// (`process_permission_choice`), so the global scope comes from explicit
    /// `PermissionRules` settings — not from session-level UX clicks.
    pub fn remember_choice(
        &mut self,
        session_id: uuid::Uuid,
        tool_name: String,
        choice: PermissionChoice,
    ) {
        match choice {
            PermissionChoice::AlwaysAllow => {
                self.session_choices
                    .entry(session_id)
                    .or_default()
                    .insert(tool_name, choice);
            }
            PermissionChoice::Deny => {
                self.always_denied.insert(tool_name.clone());
                self.session_choices
                    .entry(session_id)
                    .or_default()
                    .insert(tool_name, choice);
            }
            PermissionChoice::AllowOnce => {
                // Don't remember allow-once choices
            }
            PermissionChoice::EditAndRun => {
                // Don't remember edit-and-run choices
            }
        }
    }

    /// Clear session-specific choices (call on session end)
    pub fn clear_session(&mut self, session_id: uuid::Uuid) {
        self.session_choices.remove(&session_id);
    }

    /// Allow a tool globally (always allowed without prompting)
    pub fn allow_tool(&mut self, tool_name: &str) {
        self.always_allowed.insert(tool_name.to_string());
        self.always_denied.remove(tool_name);
    }

    /// Deny a tool globally (always denied)
    pub fn deny_tool(&mut self, tool_name: &str) {
        self.always_denied.insert(tool_name.to_string());
        self.always_allowed.remove(tool_name);
    }

    /// Get all always-allowed tools
    pub fn always_allowed_tools(&self) -> &HashSet<String> {
        &self.always_allowed
    }

    /// Get all always-denied tools
    pub fn always_denied_tools(&self) -> &HashSet<String> {
        &self.always_denied
    }

    /// Add a glob pattern for auto-allowing tools (e.g., `mcp__server__*`).
    pub fn allow_pattern(&mut self, pattern: &str) {
        if !self.allowed_patterns.contains(&pattern.to_string()) {
            self.allowed_patterns.push(pattern.to_string());
            self.rebuild_allowed_globset();
        }
    }

    /// Add a glob pattern for always-denying tools.
    pub fn deny_pattern(&mut self, pattern: &str) {
        if !self.denied_patterns.contains(&pattern.to_string()) {
            self.denied_patterns.push(pattern.to_string());
            self.rebuild_denied_globset();
        }
    }

    /// Get all allowed glob patterns.
    pub fn allowed_patterns(&self) -> &[String] {
        &self.allowed_patterns
    }

    /// Get all denied glob patterns.
    pub fn denied_patterns(&self) -> &[String] {
        &self.denied_patterns
    }

    /// Rebuild the allowed globset from stored patterns.
    fn rebuild_allowed_globset(&mut self) {
        self.allowed_globset = build_globset(&self.allowed_patterns);
    }

    /// Rebuild the denied globset from stored patterns.
    fn rebuild_denied_globset(&mut self) {
        self.denied_globset = build_globset(&self.denied_patterns);
    }
}

/// Permission level for operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PermissionLevel {
    /// No permission
    None = 0,
    /// Read-only access
    Read = 1,
    /// Write access
    Write = 2,
    /// Admin access
    Admin = 3,
}

/// A specific permission with resource and action
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Permission {
    pub resource: String,
    pub action: String,
    pub level: PermissionLevel,
}

impl Permission {
    /// Create a new permission
    pub fn new(resource: &str, action: &str, level: PermissionLevel) -> Self {
        Self {
            resource: resource.to_string(),
            action: action.to_string(),
            level,
        }
    }

    /// Check if this permission grants access for the given level
    pub fn grants(&self, required_level: PermissionLevel) -> bool {
        self.level >= required_level
    }
}

/// Permission manager for validating and granting permissions
pub struct PermissionManager {
    /// Default permissions granted to all sessions
    default_permissions: HashSet<Permission>,

    /// Session-specific permissions
    session_permissions: HashMap<uuid::Uuid, HashSet<Permission>>,

    /// Tool-specific permission requirements
    tool_permissions: HashMap<String, Permission>,

    /// Tool permission policies (risk levels and patterns)
    tool_policies: HashMap<String, ToolPermissionPolicy>,

    /// Memory of user choices
    memory: PermissionMemory,

    /// Reusable permission classifier (avoids recompiling regex patterns per call)
    classifier: crate::permission_classifier::PermissionClassifier,

    /// Optional LLM-enhanced classifier for ambiguous cases
    llm_classifier: Option<crate::llm_classifier::LlmPermissionClassifier>,

    /// Current approval policy mode
    approval_mode: ApprovalMode,

    /// Sessions with an approved plan (for Plan mode auto-approval).
    plan_approved_sessions: HashSet<uuid::Uuid>,

    /// Tools flagged as destructive via MCP `annotations.destructiveHint`.
    /// These always require user confirmation, even in auto-approve modes.
    destructive_tools: HashSet<String>,

    /// Registered tool read-only metadata, keyed by tool name (review F17).
    /// Plugin/MCP tools may register under any manifest-chosen name —
    /// including names that collide with built-in read-only tools — so the
    /// name fast-path must defer to the registered trait flags when they are
    /// known: a KNOWN mutating tool is never auto-approved by name.
    known_read_only: HashMap<String, bool>,

    /// Rule checker for deny > ask > allow priority from settings.
    rule_checker: PermissionRuleChecker,

    /// Active permission profile (strict / balanced / permissive / custom).
    active_profile: Option<PermissionProfile>,

    /// P3-2: auto-approval budget. `0` disables the breaker.
    max_auto_approvals: u32,
    /// P3-2: consecutive auto-approvals granted in the current stretch
    /// (auto-edit / full-auto / approved-plan runs). Reset on any human
    /// decision, mode change, or explicit `reset_auto_approval_count`.
    /// Atomic because `classify_and_check` takes `&self`.
    auto_approval_count: std::sync::atomic::AtomicU32,
    /// Autonomy mode to restore when plan mode exits (design §5: entering
    /// plan snapshots the current ladder mode; exit restores it).
    plan_mode_snapshot: HashMap<uuid::Uuid, ApprovalMode>,
}

impl PermissionManager {
    /// Create a new permission manager with default permissions
    pub fn new() -> Self {
        let mut manager = Self {
            default_permissions: HashSet::new(),
            session_permissions: HashMap::new(),
            tool_permissions: HashMap::new(),
            tool_policies: HashMap::new(),
            memory: PermissionMemory::new(),
            classifier: crate::permission_classifier::PermissionClassifier::new(),
            llm_classifier: None,
            approval_mode: ApprovalMode::default(),
            plan_approved_sessions: HashSet::new(),
            destructive_tools: HashSet::new(),
            known_read_only: HashMap::new(),
            rule_checker: PermissionRuleChecker::default(),
            active_profile: None,
            max_auto_approvals: 0,
            auto_approval_count: std::sync::atomic::AtomicU32::new(0),
            plan_mode_snapshot: HashMap::new(),
        };

        // Register default tool policies for common tools
        manager.register_default_policies();

        manager
    }

    /// Enable LLM-enhanced permission classification with the given client.
    ///
    /// When enabled, ambiguous tool calls (low confidence, medium+ risk) are
    /// forwarded to the LLM for a safety judgment before the final decision.
    pub fn with_llm_classifier(mut self, client: crate::api::LlmClient) -> Self {
        let rule = std::mem::take(&mut self.classifier);
        self.llm_classifier =
            Some(crate::llm_classifier::LlmPermissionClassifier::new(rule).with_llm(client));
        self
    }

    /// Check whether LLM-enhanced classification is active.
    pub fn has_llm_classifier(&self) -> bool {
        self.llm_classifier
            .as_ref()
            .is_some_and(|c| c.is_llm_enabled())
    }

    /// Register default permission policies for known tools
    fn register_default_policies(&mut self) {
        // Bash tool - high risk, confirm on dangerous commands
        let bash_policy = ToolPermissionPolicy::new(
            "Bash".to_string(),
            RiskLevel::Medium,
            "Execute shell commands".to_string(),
        )
        .add_deny_pattern("rm -rf /")
        .add_deny_pattern(":>.*")
        .add_deny_pattern("dd if=/dev/zero")
        .add_confirmation_pattern("rm -rf")
        .add_confirmation_pattern("del /q")
        .add_confirmation_pattern("chmod 000");
        self.tool_policies.insert("Bash".to_string(), bash_policy);

        // FileEdit tool - medium risk
        let edit_policy = ToolPermissionPolicy::new(
            "FileEdit".to_string(),
            RiskLevel::Low,
            "Edit file contents".to_string(),
        );
        self.tool_policies
            .insert("FileEdit".to_string(), edit_policy);

        // FileWrite tool - medium risk
        let write_policy = ToolPermissionPolicy::new(
            "FileWrite".to_string(),
            RiskLevel::Medium,
            "Write to files".to_string(),
        )
        .add_deny_pattern("/etc/")
        .add_deny_pattern("/usr/bin/")
        .add_deny_pattern("/boot/");
        self.tool_policies
            .insert("FileWrite".to_string(), write_policy);

        // Read tool - low risk
        let read_policy = ToolPermissionPolicy::new(
            "Read".to_string(),
            RiskLevel::Safe,
            "Read file contents".to_string(),
        );
        self.tool_policies.insert("Read".to_string(), read_policy);

        // WebFetch tool - medium risk
        let web_policy = ToolPermissionPolicy::new(
            "WebFetch".to_string(),
            RiskLevel::Low,
            "Fetch content from URLs".to_string(),
        );
        self.tool_policies
            .insert("WebFetch".to_string(), web_policy);

        // Computer tool - high risk (desktop control: screen capture plus
        // mouse/keyboard input simulation). Competitors gate GUI control
        // behind per-action approval by default (Cursor Auto-Run, Claude
        // Cowork per-app approval); High risk routes it to confirmation.
        let computer_policy = ToolPermissionPolicy::new(
            "computer".to_string(),
            RiskLevel::High,
            "Control the desktop: capture the screen and simulate mouse/keyboard input".to_string(),
        );
        self.tool_policies
            .insert("computer".to_string(), computer_policy);

        // AppleScript tool - high risk (reads and mutates application state
        // across Mail/Calendar/Messages/…). Same per-action confirmation
        // posture as `computer`.
        let applescript_policy = ToolPermissionPolicy::new(
            "applescript".to_string(),
            RiskLevel::High,
            "Run AppleScript/Shortcuts against macOS applications; can read and mutate app state"
                .to_string(),
        );
        self.tool_policies
            .insert("applescript".to_string(), applescript_policy);

        // Browser tools - high risk (drives a real browser, fills forms,
        // navigates, can read sensitive data on any visited page).
        for name in [
            "browser_navigate",
            "browser_click",
            "browser_type",
            "browser_screenshot",
            "browser_snapshot",
            "browser_text",
            "browser_fill",
            "browser_press_key",
            "browser_scroll",
            "browser_evaluate",
            "browser_tabs",
            "browser_close",
            "browser_console",
        ] {
            self.tool_policies.insert(
                name.to_string(),
                ToolPermissionPolicy::new(
                    name.to_string(),
                    RiskLevel::High,
                    "Drive the local system browser via CDP".to_string(),
                ),
            );
        }

        // Windows desktop surfaces — same posture as `computer` (they are
        // its semantic shortcuts): focus manipulation, clipboard reads
        // (may hold copied secrets), writes, and OS-level app launch are
        // High risk; a read-only window inventory is Low.
        let window_list_policy = ToolPermissionPolicy::new(
            "window_list".to_string(),
            RiskLevel::Low,
            "List visible top-level windows (titles, owning processes)".to_string(),
        );
        self.tool_policies
            .insert("window_list".to_string(), window_list_policy);
        for (name, why) in [
            (
                "window_focus",
                "Bring a window to the foreground / restore it",
            ),
            (
                "clipboard_read",
                "Read the system clipboard (may contain copied secrets)",
            ),
            ("clipboard_write", "Replace the system clipboard contents"),
            (
                "app_open",
                "Launch applications / files / URLs with the OS default handler",
            ),
        ] {
            self.tool_policies.insert(
                name.to_string(),
                ToolPermissionPolicy::new(name.to_string(), RiskLevel::High, why.to_string()),
            );
        }
    }

    /// Register or update a tool's permission policy
    pub fn register_tool_policy(&mut self, policy: ToolPermissionPolicy) {
        self.tool_policies.insert(policy.tool_name.clone(), policy);
    }

    /// Add a default permission
    pub fn add_default_permission(&mut self, permission: Permission) {
        self.default_permissions.insert(permission);
    }

    /// Grant a permission to a specific session
    pub fn grant_permission(&mut self, session_id: uuid::Uuid, permission: Permission) {
        self.session_permissions
            .entry(session_id)
            .or_default()
            .insert(permission);
    }

    /// Revoke a permission from a specific session
    pub fn revoke_permission(&mut self, session_id: uuid::Uuid, permission: &Permission) {
        if let Some(perms) = self.session_permissions.get_mut(&session_id) {
            perms.remove(permission);
        }
    }

    /// Set the required permission for a tool
    pub fn set_tool_permission(&mut self, tool_name: String, permission: Permission) {
        self.tool_permissions.insert(tool_name, permission);
    }

    /// Register a tool as destructive (from MCP `annotations.destructiveHint`).
    ///
    /// Destructive tools always require user confirmation.
    pub fn register_destructive_tool(&mut self, tool_name: String) {
        self.destructive_tools.insert(tool_name);
    }

    /// Record a tool's read-only trait value from the owning registry
    /// (review F17).
    ///
    /// Plugin tools register under manifest-chosen names, so a plugin can
    /// occupy a built-in read-only name ("file_info", "ls", "read_file", …)
    /// while actually mutating state. When a tool is KNOWN here, the
    /// `is_read_only_tool_name` fast-path defers to this flag:
    /// `false` (or a destructive registration) disables the auto-approve and
    /// the call falls through to normal classification. Genuine built-ins
    /// never need to register here — their names are all truly read-only.
    pub fn register_tool_read_only(&mut self, tool_name: String, is_read_only: bool) {
        self.known_read_only.insert(tool_name, is_read_only);
    }

    /// Whether the read-only NAME fast-path may auto-approve this tool
    /// (review F17).
    ///
    /// The name list is only trustworthy for genuine built-ins. Registered
    /// metadata overrides it: a known-mutating tool (`register_tool_read_only`
    /// with `false`) or a registered-destructive tool never passes the fast
    /// path, regardless of its name.
    fn read_only_fast_path_allows(&self, tool_name: &str) -> bool {
        if !is_read_only_tool_name(tool_name) {
            return false;
        }
        if self.is_tool_destructive(tool_name) {
            return false;
        }
        match self.known_read_only.get(tool_name) {
            Some(read_only) => *read_only,
            None => true,
        }
    }

    /// Check whether a tool is flagged as destructive.
    pub fn is_tool_destructive(&self, tool_name: &str) -> bool {
        self.destructive_tools.contains(tool_name)
    }

    /// Check if a session has a required permission
    pub fn check_permission(
        &self,
        session_id: uuid::Uuid,
        required: &Permission,
    ) -> Result<(), PermissionError> {
        // Check session-specific permissions first
        if let Some(perms) = self.session_permissions.get(&session_id) {
            for perm in perms {
                if perm.resource == required.resource
                    && perm.action == required.action
                    && perm.grants(required.level)
                {
                    return Ok(());
                }
            }
        }

        // Fall back to default permissions
        for perm in &self.default_permissions {
            if perm.resource == required.resource
                && perm.action == required.action
                && perm.grants(required.level)
            {
                return Ok(());
            }
        }

        Err(PermissionError::Denied(format!(
            "Permission denied for {}:{}",
            required.resource, required.action
        )))
    }

    /// Extract the P1-3 decision reason from a rule-classifier verdict.
    ///
    /// A named rule match reports `source = Rule` with the rule id and the
    /// classifier confidence; an unnamed verdict degrades to the default
    /// reason (no fabrication — the UI falls back to "policy default").
    fn classifier_reason(
        result: &crate::permission_classifier::ClassificationResult,
    ) -> DecisionReason {
        match &result.matched_rule {
            Some(rule) => DecisionReason {
                source: ReasonSource::Rule,
                rule_name: Some(rule.clone()),
                confidence: Some(result.confidence),
            },
            None => DecisionReason::default_reason(),
        }
    }

    /// P1-3: attribute a decision reason from an LLM-classifier outcome.
    ///
    /// A consulted LLM verdict reports `source = Llm` with its confidence; a
    /// non-consulted fallback is attributed to the underlying rule verdict.
    fn llm_reason(llm_result: &crate::llm_classifier::LlmClassificationResult) -> DecisionReason {
        if llm_result.llm_consulted {
            DecisionReason::llm(llm_result.result.confidence)
        } else {
            Self::classifier_reason(&llm_result.result)
        }
    }

    /// Check if a session can execute a tool
    pub fn check_tool_permission(
        &self,
        session_id: uuid::Uuid,
        tool_name: &str,
    ) -> Result<(), PermissionError> {
        if let Some(required) = self.tool_permissions.get(tool_name) {
            self.check_permission(session_id, required)
        } else {
            Ok(())
        }
    }

    /// Get all permissions for a session
    pub fn get_session_permissions(&self, session_id: uuid::Uuid) -> HashSet<Permission> {
        let mut perms = self.default_permissions.clone();
        if let Some(session_perms) = self.session_permissions.get(&session_id) {
            perms.extend(session_perms.clone());
        }
        perms
    }

    /// Get all registered tool policies
    pub fn tool_policies(&self) -> &HashMap<String, ToolPermissionPolicy> {
        &self.tool_policies
    }

    /// Get all tool-level permission requirements
    pub fn tool_permissions(&self) -> &HashMap<String, Permission> {
        &self.tool_permissions
    }

    /// Get a reference to the permission memory
    pub fn memory(&self) -> &PermissionMemory {
        &self.memory
    }

    /// Get a mutable reference to the permission memory
    pub fn memory_mut(&mut self) -> &mut PermissionMemory {
        &mut self.memory
    }

    /// Allow a tool globally (always allowed without prompting)
    pub fn allow_tool(&mut self, tool_name: &str) {
        self.memory.allow_tool(tool_name);
    }

    /// Deny a tool globally (always denied)
    pub fn deny_tool(&mut self, tool_name: &str) {
        self.memory.deny_tool(tool_name);
    }

    /// Allow all tools matching a glob pattern (e.g., `mcp__server__*`).
    pub fn allow_pattern(&mut self, pattern: &str) {
        self.memory.allow_pattern(pattern);
    }

    /// Deny all tools matching a glob pattern.
    pub fn deny_pattern(&mut self, pattern: &str) {
        self.memory.deny_pattern(pattern);
    }

    /// Reset all permission memory (allowed/denied tools)
    pub fn reset_memory(&mut self) {
        self.memory = PermissionMemory::new();
    }

    /// Get the current approval mode.
    pub fn approval_mode(&self) -> ApprovalMode {
        self.approval_mode
    }

    /// Set the approval mode. Resets the auto-approval budget counter —
    /// a posture change is a fresh stretch (design §4.1 / K5).
    pub fn set_approval_mode(&mut self, mode: ApprovalMode) {
        tracing::info!(old = ?self.approval_mode, new = ?mode, "Approval mode changed");
        self.approval_mode = mode;
        self.reset_auto_approval_count();
    }

    /// P3-2: configure the auto-approval budget (`0` = off, the default).
    pub fn set_max_auto_approvals(&mut self, max: u32) {
        self.max_auto_approvals = max;
    }

    /// P3-2: current auto-approval budget (`0` = disabled).
    pub fn max_auto_approvals(&self) -> u32 {
        self.max_auto_approvals
    }

    /// P3-2: auto-approvals granted in the current stretch.
    pub fn auto_approval_count(&self) -> u32 {
        self.auto_approval_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// P3-2: true when a budget is configured and fully consumed.
    fn auto_approval_budget_exhausted(&self) -> bool {
        self.max_auto_approvals > 0
            && self
                .auto_approval_count
                .load(std::sync::atomic::Ordering::Relaxed)
                >= self.max_auto_approvals
    }

    /// P3-2: record one automatic approval.
    fn count_auto_approval(&self) {
        self.auto_approval_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// P3-2: clear the auto-approval stretch (call after a human decision or
    /// at the start of a new user turn if the surface wants per-turn budgets).
    pub fn reset_auto_approval_count(&self) {
        self.auto_approval_count
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Design §5: snapshot the current ladder mode and switch to `Plan`.
    /// The snapshot is per-session; `exit_plan_mode` restores it.
    pub fn enter_plan_mode(&mut self, session_id: uuid::Uuid) {
        let previous = self.approval_mode;
        // Re-entering while already planning must not clobber the original
        // snapshot with `Plan` itself.
        if previous != ApprovalMode::Plan {
            self.plan_mode_snapshot.insert(session_id, previous);
            self.approval_mode = ApprovalMode::Plan;
            self.reset_auto_approval_count();
        }
        tracing::info!(session = %session_id, snapshot = ?previous, "Entered plan mode");
    }

    /// Design §5: leave plan mode, restoring the snapshotted ladder mode.
    /// Returns the restored mode (`Ask` when no snapshot existed). Also
    /// clears any plan approval for the session.
    pub fn exit_plan_mode(&mut self, session_id: uuid::Uuid) -> ApprovalMode {
        let restored = self
            .plan_mode_snapshot
            .remove(&session_id)
            .unwrap_or(ApprovalMode::Ask);
        self.plan_approved_sessions.remove(&session_id);
        if self.approval_mode == ApprovalMode::Plan {
            self.approval_mode = restored;
            self.reset_auto_approval_count();
        }
        tracing::info!(session = %session_id, restored = ?restored, "Exited plan mode");
        restored
    }

    /// Set the permission rule checker (built from rule strings).
    pub fn set_rule_checker(&mut self, checker: PermissionRuleChecker) {
        self.rule_checker = checker;
    }

    /// Get a reference to the permission rule checker.
    pub fn rule_checker(&self) -> &PermissionRuleChecker {
        &self.rule_checker
    }

    /// Return the active permission profile, if one is set.
    pub fn active_profile(&self) -> Option<&PermissionProfile> {
        self.active_profile.as_ref()
    }

    /// Return the effective profile rules.
    ///
    /// If no profile is active, returns `None`.
    pub fn profile_rules(&self) -> Option<ProfileRules> {
        self.active_profile.as_ref().map(|p| p.rules())
    }

    /// Apply a permission profile, updating approval mode and destructive tool list.
    ///
    /// The profile rules map to an appropriate `ApprovalMode` and register
    /// any always-denied tools.
    pub fn apply_profile(&mut self, profile: PermissionProfile) {
        let rules = profile.rules();

        // Pick the closest ApprovalMode for the profile.
        let mode = if rules.auto_approve_read
            && rules.auto_approve_write
            && rules.auto_approve_bash
            && !rules.auto_approve_delete
        {
            ApprovalMode::AutoEdit
        } else if rules.auto_approve_read
            && rules.auto_approve_write
            && rules.auto_approve_bash
            && rules.auto_approve_delete
        {
            ApprovalMode::FullAuto
        } else if rules.auto_approve_read && !rules.auto_approve_write && !rules.auto_approve_bash {
            ApprovalMode::Ask
        } else {
            // Fallback: treat as Ask
            ApprovalMode::Ask
        };

        // Register denied tools from the profile.
        for tool_name in &rules.deny_destructive {
            self.destructive_tools.insert(tool_name.clone());
        }

        tracing::info!(
            ?profile,
            ?mode,
            denied_tools = ?rules.deny_destructive,
            "Applied permission profile"
        );

        self.approval_mode = mode;
        self.active_profile = Some(profile);
    }

    /// Apply a custom profile definition loaded from `.shannon/profiles/*.toml`.
    ///
    /// Unlike `apply_profile` which maps built-in profiles to approval modes,
    /// this method uses the per-tool auto_approve/confirm/deny lists directly.
    pub fn apply_custom_profile_def(&mut self, def: &crate::custom_profiles::CustomProfileDef) {
        // Determine approval mode based on what's auto-approved
        let auto_approves_read = def
            .auto_approve
            .iter()
            .any(|t| t == "Read" || t == "Glob" || t == "Grep" || t == "LS");
        let auto_approves_write = def
            .auto_approve
            .iter()
            .any(|t| t == "Edit" || t == "Write" || t == "MultiEdit");
        let auto_approves_bash = def.auto_approve.iter().any(|t| t == "Bash");

        let mode = if auto_approves_read && auto_approves_write && auto_approves_bash {
            ApprovalMode::AutoEdit
        } else {
            ApprovalMode::Ask
        };

        // Register denied tools
        for tool_name in &def.deny {
            self.destructive_tools.insert(tool_name.clone());
        }

        tracing::info!(
            name = %def.name,
            ?mode,
            auto = ?def.auto_approve,
            deny = ?def.deny,
            "Applied custom permission profile"
        );

        self.approval_mode = mode;
        self.active_profile = Some(PermissionProfile::Custom(def.name.clone()));
    }

    /// Approve the plan for a session (enables auto-approve in Plan mode).
    pub fn approve_plan(&mut self, session_id: uuid::Uuid) {
        self.plan_approved_sessions.insert(session_id);
    }

    /// Check if a plan has been approved for a session.
    pub fn is_plan_approved(&self, session_id: uuid::Uuid) -> bool {
        self.plan_approved_sessions.contains(&session_id)
    }

    /// Clear plan approval for a session.
    pub fn clear_plan_approval(&mut self, session_id: uuid::Uuid) {
        self.plan_approved_sessions.remove(&session_id);
    }

    /// Create a permission prompt for a tool execution
    pub fn create_permission_prompt(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        session_id: uuid::Uuid,
    ) -> Option<PermissionPrompt> {
        // Check if this is already always allowed
        if self.memory.is_always_allowed(session_id, tool_name) {
            return None; // No prompt needed
        }

        // Check if always denied
        if self.memory.is_always_denied(tool_name) {
            return Some(PermissionPrompt {
                id: uuid::Uuid::new_v4(),
                tool_name: tool_name.to_string(),
                tool_input: tool_input.clone(),
                risk_level: RiskLevel::Critical,
                description: format!("This tool is denied: {tool_name}"),
                is_confirmation: false,
                diff_preview: None,
                is_destructive: self.is_tool_destructive(tool_name),
                risk_reason: format!("Tool '{tool_name}' is in the always-denied list"),
                reason: DecisionReason::default(),
                limit_triggered: false,
            });
        }

        // Get tool policy
        let policy = self.tool_policies.get(tool_name);

        // Determine risk level and description
        let (risk_level, description) = if let Some(policy) = policy {
            let input_str = serde_json::to_string(tool_input).unwrap_or_default();
            (
                policy.risk_level_for(&input_str),
                format!(
                    "{}: {}",
                    policy.description,
                    Self::format_input_summary(tool_input)
                ),
            )
        } else {
            // Unknown tool - default to medium risk
            (
                RiskLevel::Medium,
                format!("Execute tool: {}", Self::format_input_summary(tool_input)),
            )
        };

        let is_destructive = self.is_tool_destructive(tool_name);
        let description = if is_destructive {
            format!("[DESTRUCTIVE] {description}")
        } else {
            description
        };

        Some(PermissionPrompt {
            id: uuid::Uuid::new_v4(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
            risk_level,
            description,
            is_confirmation: false,
            diff_preview: None,
            is_destructive,
            risk_reason: format!("{risk_level:?} risk based on tool policy and approval mode"),
            reason: DecisionReason::default(),
            limit_triggered: false,
        })
    }

    /// Process user's permission choice
    pub fn process_permission_choice(
        &mut self,
        session_id: uuid::Uuid,
        prompt: &PermissionPrompt,
        choice: PermissionChoice,
    ) -> Result<(), PermissionError> {
        // P3-2: a human decision starts a fresh auto-approval stretch.
        self.reset_auto_approval_count();
        match choice {
            PermissionChoice::Deny => Err(PermissionError::Denied(format!(
                "User denied: {}",
                prompt.description
            ))),
            PermissionChoice::AllowOnce | PermissionChoice::AlwaysAllow => {
                // Remember the choice
                let is_always = choice == PermissionChoice::AlwaysAllow;
                self.memory
                    .remember_choice(session_id, prompt.tool_name.clone(), choice);
                if is_always {
                    // S-2: persist the grant so it survives process restarts
                    // (the in-memory PermissionMemory dies with the process,
                    // which forced users to re-approve the same operation in
                    // every session). Project-scoped: `.shannon/settings.local.json`.
                    Self::persist_allow_rule(&prompt.tool_name, &prompt.tool_input);
                }
                Ok(())
            }
            PermissionChoice::EditAndRun => {
                // User edited the command; treat as allow-once
                self.memory
                    .remember_choice(session_id, prompt.tool_name.clone(), choice);
                Ok(())
            }
        }
    }

    /// Persist an always-allow grant as a permission rule in the project's
    /// `.shannon/settings.local.json` (`permissions.allow` array), matching
    /// the rule-checker's `Tool(pattern)` syntax. Best-effort: failures are
    /// logged, never surfaced — the in-memory grant still applies.
    ///
    /// review §P1-1: the previous format `Bash(<head>:*)` was a literal
    /// regex match — `:` is a literal character and `*` only matched if the
    /// command contained a real colon character, so every persisted rule
    /// silently failed to match. We now emit `Bash(<head> *)` with a single
    /// space separator so the rule-checker's glob (which uses
    /// `command.contains(pattern)` for non-glob strings) recognises the
    /// `<head> <args...>` shape correctly.
    fn persist_allow_rule(tool_name: &str, tool_input: &serde_json::Value) {
        let pattern = if tool_name.eq_ignore_ascii_case("bash") {
            // Scope Bash grants to the exact approved command prefix rather
            // than the whole tool.
            let cmd = tool_input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if cmd.is_empty() {
                tool_name.to_string()
            } else {
                let head: String = cmd.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
                format!("{tool_name}({head} *)")
            }
        } else {
            tool_name.to_string()
        };

        let cwd = match std::env::current_dir() {
            Ok(d) => d,
            Err(e) => {
                tracing::debug!("cannot resolve cwd to persist permission: {e}");
                return;
            }
        };
        let dir = cwd.join(".shannon");
        let path = dir.join("settings.local.json");

        let mut doc: serde_json::Value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({}));

        let Some(obj) = doc.as_object_mut() else {
            return;
        };
        let perms = obj
            .entry("permissions".to_string())
            .or_insert_with(|| serde_json::json!({}));
        let Some(perms_obj) = perms.as_object_mut() else {
            return;
        };
        let list = perms_obj
            .entry("allow".to_string())
            .or_insert_with(|| serde_json::json!([]));
        if let Some(arr) = list.as_array_mut() {
            if !arr.iter().any(|v| v.as_str() == Some(pattern.as_str())) {
                arr.push(serde_json::Value::String(pattern));
            }
        }

        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(&doc) {
            Ok(body) => {
                let tmp = path.with_extension("json.tmp");
                if std::fs::write(&tmp, body).is_ok() && std::fs::rename(&tmp, &path).is_ok() {
                    tracing::info!("Persisted always-allow rule to {}", path.display());
                }
            }
            Err(e) => tracing::debug!("failed to serialize permission grant: {e}"),
        }
    }

    /// Helper to format tool input summary
    fn format_input_summary(input: &serde_json::Value) -> String {
        if input.is_null() {
            return "(no input)".to_string();
        }

        if let Some(obj) = input.as_object() {
            let parts: Vec<String> = obj
                .iter()
                .take(3) // Only show first 3 fields
                .map(|(k, v)| format!("{}: {}", k, Self::truncate_value(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        } else if let Some(arr) = input.as_array() {
            if !arr.is_empty() {
                format!("[{} values]", arr.len())
            } else {
                "[]".to_string()
            }
        } else {
            let s = input.to_string();
            if s.len() > 50 {
                let mut end = 47.min(s.len());
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...", &s[..end])
            } else {
                s
            }
        }
    }

    /// Helper to truncate a JSON value for display
    fn truncate_value(value: &serde_json::Value) -> String {
        let s = serde_json::to_string(value).unwrap_or_else(|_| "?".to_string());
        if s.len() > 30 {
            let mut end = 27.min(s.len());
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}...", &s[..end])
        } else {
            s
        }
    }

    /// Clear session data (call when session ends)
    pub fn clear_session(&mut self, session_id: uuid::Uuid) {
        self.session_permissions.remove(&session_id);
        self.memory.clear_session(session_id);
        self.plan_approved_sessions.remove(&session_id);
    }

    /// Classify a tool operation using the PermissionClassifier and check permission.
    ///
    /// - Returns `Ok(None)` if the operation is auto-allowed
    /// - Returns `Ok(Some(prompt))` if user confirmation is needed
    /// - Returns `Err` if the operation is denied
    pub fn classify_and_check(
        &self,
        session_id: uuid::Uuid,
        tool_name: &str,
        tool_input: &serde_json::Value,
    ) -> Result<Option<PermissionPrompt>, PermissionError> {
        // --- Global deny gate (applies in EVERY mode, including bypass) ---
        // P0-3: user-configured deny rules are the one thing no mode may
        // override, matching Claude Code ("deny rules block in every mode").
        if self.memory.is_always_denied(tool_name) {
            return Err(PermissionError::Denied(format!(
                "Denied by user rule (always-denied): {tool_name}"
            )));
        }

        // --- Permission rule checker (deny > ask > allow from settings) ---
        if !self.rule_checker.is_empty() {
            let command = tool_input
                .get("command")
                .or_else(|| tool_input.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let (decision, matched_rule) = self.rule_checker.check_with_rule(tool_name, command);
            match decision {
                RuleCheckDecision::Denied => {
                    return Err(PermissionError::Denied(format!(
                        "Denied by permission rule: {tool_name}"
                    )));
                }
                RuleCheckDecision::Ask => {
                    let reason = DecisionReason {
                        source: ReasonSource::Rule,
                        rule_name: matched_rule,
                        confidence: None,
                    };
                    return self.create_permission_prompt_with_risk(
                        tool_name,
                        tool_input,
                        session_id,
                        RiskLevel::Medium,
                        reason,
                    );
                }
                RuleCheckDecision::Allowed => {
                    return Ok(None); // auto-approved by rule
                }
                RuleCheckDecision::NoMatch => {
                    // Fall through to normal approval mode logic
                }
            }
        }

        // --- Approval mode overrides ---
        // P0-2 / design §5: once the session's plan is approved, `Plan`
        // auto-runs at the full-auto floor (deny rules + Critical still bind,
        // destructive still prompts, budget still counts) — never a blanket
        // Ok(None).
        let effective_mode = match self.approval_mode {
            ApprovalMode::Plan if self.plan_approved_sessions.contains(&session_id) => {
                ApprovalMode::FullAuto
            }
            other => other,
        };
        match effective_mode {
            // BypassPermissions: skip all remaining checks. The global deny
            // gate above still applied (P0-3); entry itself is guardrailed at
            // the CLI / REPL (P2-4: root refusal, kill switch, confirm).
            ApprovalMode::BypassPermissions => {
                tracing::debug!(mode = ?self.approval_mode, tool = %tool_name, "Permission check bypassed");
                return Ok(None);
            }
            // DontAsk never waits (P2-1: no longer shares the bypass branch).
            // Pre-approved tools and reads pass; everything else is DENIED —
            // aligns with Claude Code's dontAsk semantics for CI runs.
            ApprovalMode::DontAsk => {
                if self.memory.is_always_allowed(session_id, tool_name)
                    || self.read_only_fast_path_allows(tool_name)
                {
                    return Ok(None);
                }
                return Err(PermissionError::Denied(format!(
                    "dontAsk mode never waits: {tool_name} is not pre-approved; add it to permissions.allow or pick another mode"
                )));
            }
            ApprovalMode::Readonly => {
                // Only allow read-only tools (review F17: registered tool
                // metadata can veto the name fast-path)
                if self.read_only_fast_path_allows(tool_name) {
                    return Ok(None);
                }
                return Err(PermissionError::Denied(format!(
                    "Readonly mode: {tool_name} is not a read operation"
                )));
            }
            // Unapproved plan behaves like Ask (read-only fast path prompts for
            // the rest). The approved case was promoted to FullAuto above.
            ApprovalMode::Plan => {
                if self.memory.is_always_allowed(session_id, tool_name) {
                    return Ok(None);
                }
                // Destructive tools always require confirmation
                if self.is_tool_destructive(tool_name) {
                    return Ok(self.create_permission_prompt(tool_name, tool_input, session_id));
                }
                // Fall through to classifier for risk level and prompt creation
            }
            ApprovalMode::Ask => {
                // Always-allowed in memory → auto-approve
                if self.memory.is_always_allowed(session_id, tool_name) {
                    return Ok(None);
                }
                // Auto-approve read-only tools (matching Claude Code behavior:
                // Read, Glob, Grep, etc. don't need confirmation).
                // review F17: the name fast-path is vetoed by registered tool
                // metadata — a plugin occupying a built-in read-only name with
                // mutating flags falls through to the classifier/destructive
                // checks instead of being auto-approved.
                if self.read_only_fast_path_allows(tool_name) {
                    return Ok(None);
                }
                // Destructive tools always require confirmation
                if self.is_tool_destructive(tool_name) {
                    return Ok(self.create_permission_prompt(tool_name, tool_input, session_id));
                }
                // Fall through to classifier for risk level and prompt creation
            }
            ApprovalMode::AutoEdit | ApprovalMode::FullAuto => {
                // Run classifier first to get risk level
                let result = self.classifier.classify(tool_name, tool_input);
                let risk = convert_classifier_risk(result.risk_level);

                // Check memory for always-allowed
                if self.memory.is_always_allowed(session_id, tool_name) {
                    return Ok(None);
                }

                // Destructive tools always require confirmation
                if self.is_tool_destructive(tool_name) {
                    return Ok(self.create_permission_prompt(tool_name, tool_input, session_id));
                }

                // If the mode says auto-approve, do it — subject to the
                // auto-approval budget (P3-2): when the session exhausts
                // `max_auto_approvals`, force a human decision instead.
                if effective_mode.should_auto_approve(tool_name, risk) {
                    if self.auto_approval_budget_exhausted() {
                        let mut prompt = match self.create_permission_prompt_with_risk(
                            tool_name,
                            tool_input,
                            session_id,
                            risk,
                            Self::classifier_reason(&result),
                        ) {
                            Ok(Some(p)) => p,
                            Ok(None) => Self::fallback_limit_prompt(tool_name, tool_input, risk),
                            Err(e) => return Err(e),
                        };
                        prompt.limit_triggered = true;
                        prompt.risk_reason = format!(
                            "auto-approval budget exhausted ({}) — approving continues the session, denying stops it",
                            self.max_auto_approvals
                        );
                        return Ok(Some(prompt));
                    }
                    self.count_auto_approval();
                    return Ok(None);
                }

                // Denied by classifier
                if result.decision == crate::permission_classifier::RuleDecision::Deny {
                    return Err(PermissionError::Denied(format!(
                        "Operation denied by classifier: {} (risk: {})",
                        result.reason, result.risk_level
                    )));
                }

                // Otherwise prompt
                let reason = Self::classifier_reason(&result);
                return self.create_permission_prompt_with_risk(
                    tool_name, tool_input, session_id, risk, reason,
                );
            }
        }

        // --- Default classifier logic (Suggest / Plan mode path) ---
        let result = self.classifier.classify(tool_name, tool_input);
        let reason = Self::classifier_reason(&result);

        match result.decision {
            crate::permission_classifier::RuleDecision::Deny => {
                Err(PermissionError::Denied(format!(
                    "Operation denied by classifier: {} (risk: {})",
                    result.reason, result.risk_level
                )))
            }
            crate::permission_classifier::RuleDecision::Allow => {
                // For suggest/plan mode, always prompt
                self.create_permission_prompt_with_risk(
                    tool_name,
                    tool_input,
                    session_id,
                    convert_classifier_risk(result.risk_level),
                    reason,
                )
            }
            crate::permission_classifier::RuleDecision::Ask => {
                // Always prompt the user
                self.create_permission_prompt_with_risk(
                    tool_name,
                    tool_input,
                    session_id,
                    convert_classifier_risk(result.risk_level),
                    reason,
                )
            }
        }
    }

    /// Async version of [`classify_and_check`](Self::classify_and_check) that uses
    /// the LLM-enhanced classifier when configured.
    ///
    /// Falls back to the synchronous rule-based classification when no LLM client
    /// is available, so this is always safe to call as a drop-in replacement.
    pub async fn classify_and_check_with_llm(
        &self,
        session_id: uuid::Uuid,
        tool_name: &str,
        tool_input: &serde_json::Value,
    ) -> Result<Option<PermissionPrompt>, PermissionError> {
        // When LLM classifier is not configured, delegate to sync path
        let Some(ref llm) = self.llm_classifier else {
            return self.classify_and_check(session_id, tool_name, tool_input);
        };

        // Only the auto modes benefit from LLM classification (K3: the LLM
        // is a hardening layer inside auto-edit / full-auto, not a mode);
        // other modes have deterministic rules that don't need LLM judgment.
        if !matches!(
            self.approval_mode,
            ApprovalMode::AutoEdit | ApprovalMode::FullAuto
        ) {
            return self.classify_and_check(session_id, tool_name, tool_input);
        }

        // Run rule checker first (highest priority)
        if !self.rule_checker.is_empty() {
            let command = tool_input
                .get("command")
                .or_else(|| tool_input.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let (decision, matched_rule) = self.rule_checker.check_with_rule(tool_name, command);
            match decision {
                RuleCheckDecision::Denied => {
                    return Err(PermissionError::Denied(format!(
                        "Denied by permission rule: {tool_name}"
                    )));
                }
                RuleCheckDecision::Ask => {
                    let reason = DecisionReason {
                        source: ReasonSource::Rule,
                        rule_name: matched_rule,
                        confidence: None,
                    };
                    return self.create_permission_prompt_with_risk(
                        tool_name,
                        tool_input,
                        session_id,
                        RiskLevel::Medium,
                        reason,
                    );
                }
                RuleCheckDecision::Allowed => return Ok(None),
                RuleCheckDecision::NoMatch => {}
            }
        }

        // Bypass skips everything (deny gates already ran above); DontAsk has
        // its own never-wait semantics — delegate to the sync path.
        if self.approval_mode == ApprovalMode::DontAsk {
            return self.classify_and_check(session_id, tool_name, tool_input);
        }
        if self.approval_mode == ApprovalMode::BypassPermissions {
            return Ok(None);
        }

        // Memory check
        if self.memory.is_always_allowed(session_id, tool_name) {
            return Ok(None);
        }

        // Destructive tools always prompt
        if self.is_tool_destructive(tool_name) {
            return Ok(self.create_permission_prompt(tool_name, tool_input, session_id));
        }

        // LLM-enhanced classification
        let llm_result = llm.classify(tool_name, tool_input).await;
        let risk = convert_classifier_risk(llm_result.result.risk_level);

        // P1-3: reason attribution — when the LLM actually decided, say so
        // with its confidence; a non-consulted fallback is attributed to the
        // underlying rule verdict instead.
        let reason = Self::llm_reason(&llm_result);

        match llm_result.result.decision {
            crate::permission_classifier::RuleDecision::Deny => {
                Err(PermissionError::Denied(format!(
                    "{} (LLM {}consulted, risk: {:?})",
                    llm_result.result.reason,
                    if llm_result.llm_consulted { "" } else { "not " },
                    risk
                )))
            }
            crate::permission_classifier::RuleDecision::Allow => {
                if risk <= RiskLevel::Low {
                    Ok(None)
                } else {
                    self.create_permission_prompt_with_risk(
                        tool_name, tool_input, session_id, risk, reason,
                    )
                }
            }
            crate::permission_classifier::RuleDecision::Ask => self
                .create_permission_prompt_with_risk(
                    tool_name, tool_input, session_id, risk, reason,
                ),
        }
    }

    /// Create a permission prompt with an explicit risk level from classifier.
    ///
    /// `reason` (P1-3) is informational — it travels with the prompt so the
    /// approval dialog can explain why it was raised.
    fn create_permission_prompt_with_risk(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        session_id: uuid::Uuid,
        risk_level: RiskLevel,
        reason: DecisionReason,
    ) -> Result<Option<PermissionPrompt>, PermissionError> {
        if self.memory.is_always_allowed(session_id, tool_name) {
            return Ok(None);
        }
        if self.memory.is_always_denied(tool_name) {
            return Err(PermissionError::Denied(format!(
                "Tool '{tool_name}' is always denied"
            )));
        }

        let policy = self.tool_policies.get(tool_name);
        let is_destructive = self.is_tool_destructive(tool_name);
        let description = if let Some(p) = policy {
            let desc = format!(
                "{}: {}",
                p.description,
                Self::format_input_summary(tool_input)
            );
            if is_destructive {
                format!("[DESTRUCTIVE] {desc}")
            } else {
                desc
            }
        } else {
            let desc = format!(
                "Execute tool '{}': {}",
                tool_name,
                Self::format_input_summary(tool_input)
            );
            if is_destructive {
                format!("[DESTRUCTIVE] {desc}")
            } else {
                desc
            }
        };

        Ok(Some(PermissionPrompt {
            id: uuid::Uuid::new_v4(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
            risk_level,
            description,
            is_confirmation: false,
            diff_preview: None,
            is_destructive,
            risk_reason: format!(
                "{risk_level:?} risk: policy-based classification for '{tool_name}'"
            ),
            reason,
            limit_triggered: false,
        }))
    }

    /// P3-2: minimal prompt used when the budget trips and the normal prompt
    /// constructor declines (already allowed in memory). Kept separate so the
    /// caller can mark `limit_triggered` deterministically.
    fn fallback_limit_prompt(
        tool_name: &str,
        tool_input: &serde_json::Value,
        risk_level: RiskLevel,
    ) -> PermissionPrompt {
        let mut prompt = PermissionPrompt::new(
            tool_name.to_string(),
            tool_input.clone(),
            risk_level,
            format!(
                "Approve to continue: {}",
                Self::format_input_summary(tool_input)
            ),
        );
        prompt.limit_triggered = true;
        prompt
    }
}

/// Build a compiled `GlobSet` from a list of glob pattern strings.
/// Returns `None` if the list is empty or all patterns fail to compile.
fn build_globset(patterns: &[String]) -> Option<GlobSet> {
    if patterns.is_empty() {
        return None;
    }
    let mut builder = GlobSetBuilder::new();
    let mut valid_count = 0;
    for pat in patterns {
        if let Ok(glob) = Glob::new(pat) {
            builder.add(glob);
            valid_count += 1;
        } else {
            tracing::warn!("Invalid glob pattern in permissions: {pat}");
        }
    }
    if valid_count == 0 {
        return None;
    }
    builder.build().ok()
}

// ── Settings & profile wiring (P1-1 / P1-2 / P1-3) ─────────────────────────

/// Apply one parsed `permissions` object from a settings.json file.
///
/// P1-1: allow/deny feed the session memory exactly as before, AND all three
/// lists (allow / ask / deny) feed the rule checker — so `Bash(cmd *)`
/// patterns are honoured properly and the previously ignored `ask` list
/// finally forces prompts in every mode (Claude Code semantics).
///
/// P1-2: `permissions.defaultMode` seeds the approval mode. Project-level
/// files may not set bypass/dontAsk (project-poisoning guard); those values
/// are only honoured from the user-level settings file.
pub fn apply_settings_permissions(
    pm: &mut PermissionManager,
    perms: &serde_json::Value,
    is_project_file: bool,
) {
    fn list_of(v: &serde_json::Value, key: &str) -> Vec<String> {
        v.get(key)
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|i| i.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    let allow = list_of(perms, "allow");
    let ask = list_of(perms, "ask");
    let deny = list_of(perms, "deny");

    for s in &allow {
        if s.contains('(') || s.contains('*') || s.contains('?') {
            pm.allow_pattern(s);
        } else {
            pm.allow_tool(s);
        }
    }
    for s in &deny {
        if s.contains('(') || s.contains('*') || s.contains('?') {
            pm.deny_pattern(s);
        } else {
            pm.deny_tool(s);
        }
    }
    if !(allow.is_empty() && ask.is_empty() && deny.is_empty()) {
        pm.set_rule_checker(PermissionRuleChecker::from_rule_strings(
            &deny, &ask, &allow,
        ));
    }

    if let Some(mode_str) = perms.get("defaultMode").and_then(|v| v.as_str()) {
        match ApprovalMode::from_str_ci(mode_str) {
            Some(mode) => {
                if is_project_file
                    && matches!(
                        mode,
                        ApprovalMode::BypassPermissions | ApprovalMode::DontAsk
                    )
                {
                    tracing::warn!(
                        "ignoring project-level permissions.defaultMode='{mode_str}':                          bypass/dontAsk are only honoured in user settings"
                    );
                } else {
                    pm.set_approval_mode(mode);
                }
            }
            None => tracing::warn!("unknown permissions.defaultMode '{mode_str}'"),
        }
    }
}

/// Load user + project settings.json permission blocks (P1-1/P1-2).
///
/// Precedence matches the REPL loader: user settings, then project
/// `.shannon/settings.json`, then project `.claude/settings.json` (later
/// wins). Returns how many files were applied.
pub fn load_settings_permission_files(pm: &mut PermissionManager) -> usize {
    let mut applied = 0;
    let mut paths = Vec::new();
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Some(home) = dirs::home_dir() {
        paths.push((home.join(".shannon").join("settings.json"), false));
    }
    paths.push((cwd.join(".shannon").join("settings.json"), true));
    paths.push((cwd.join(".claude").join("settings.json"), true));

    for (path, is_project) in paths {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(doc) = serde_json::from_str::<serde_json::Value>(&content) else {
            tracing::warn!(
                "Skipping invalid settings file {}: parse error",
                path.display()
            );
            continue;
        };
        let Some(perms) = doc.get("permissions") else {
            continue;
        };
        apply_settings_permissions(pm, perms, is_project);
        applied += 1;
        tracing::info!("Loaded permission settings from {}", path.display());
    }
    applied
}

/// P1-3: resolve and apply a configured permission profile.
///
/// `profile` is one of `strict` / `balanced` / `permissive` (built-ins) or
/// `custom:<name>` resolved against `.shannon/profiles/*.toml` et al.
pub fn apply_configured_profile(pm: &mut PermissionManager, profile: &str) {
    if let Some(name) = profile.strip_prefix("custom:") {
        let registry = crate::custom_profiles::CustomProfileRegistry::load_from_dirs();
        match registry.get(name) {
            Some(def) => pm.apply_custom_profile_def(def),
            None => tracing::warn!("custom permission profile '{name}' not found"),
        }
    } else if let Some(p) = crate::permission_profile::PermissionProfile::from_str_lossy(profile) {
        pm.apply_profile(p);
    } else {
        tracing::warn!("unknown permission_profile '{profile}'");
    }
}

/// P2-4: true when the process runs as root/sudo (unix; always false on
/// Windows). Used to refuse bypass entry as root.
pub fn running_as_root() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// P2-4: entry guardrails for `BypassPermissions` / `--yes`.
///
/// - `SHANNON_DISABLE_BYPASS=1` is an org/CI-wide kill switch: bypass modes
///   error out instead of starting.
/// - As root/sudo, bypass is refused unless `SHANNON_ALLOW_ROOT_BYPASS=1`
///   (matching Claude Code's root refusal; the override is for containers).
pub fn ensure_bypass_allowed() -> Result<(), String> {
    if std::env::var("SHANNON_DISABLE_BYPASS").as_deref() == Ok("1") {
        return Err(
            "SHANNON_DISABLE_BYPASS=1 is set: bypassPermissions / --yes is disabled on this machine"
                .to_string(),
        );
    }
    if running_as_root() && std::env::var("SHANNON_ALLOW_ROOT_BYPASS").as_deref() != Ok("1") {
        return Err(
            "refusing bypassPermissions as root — run as a normal user, or set SHANNON_ALLOW_ROOT_BYPASS=1 for containers"
                .to_string(),
        );
    }
    Ok(())
}

/// Convert classifier RiskLevel to permissions RiskLevel.
fn convert_classifier_risk(risk: crate::permission_classifier::RiskLevel) -> RiskLevel {
    match risk {
        crate::permission_classifier::RiskLevel::None => RiskLevel::Safe,
        crate::permission_classifier::RiskLevel::Low => RiskLevel::Low,
        crate::permission_classifier::RiskLevel::Medium => RiskLevel::Medium,
        crate::permission_classifier::RiskLevel::High => RiskLevel::High,
        crate::permission_classifier::RiskLevel::Critical => RiskLevel::Critical,
    }
}

// NOTE: PermissionManager is auto-Send + Sync because all fields (HashMap, HashSet)
// contain only Send + Sync types. No unsafe impl needed.

impl Default for PermissionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_permission_creation() {
        let perm = Permission::new("file", "read", PermissionLevel::Read);
        assert_eq!(perm.resource, "file");
        assert_eq!(perm.action, "read");
        assert!(perm.grants(PermissionLevel::Read));
        assert!(!perm.grants(PermissionLevel::Write));
    }

    #[test]
    fn test_permission_grant_revoke() {
        let mut manager = PermissionManager::new();
        let session_id = Uuid::new_v4();
        let perm = Permission::new("file", "write", PermissionLevel::Write);

        // Initially should fail
        assert!(manager.check_permission(session_id, &perm).is_err());

        // Grant permission
        manager.grant_permission(session_id, perm.clone());
        assert!(manager.check_permission(session_id, &perm).is_ok());

        // Revoke permission
        manager.revoke_permission(session_id, &perm);
        assert!(manager.check_permission(session_id, &perm).is_err());
    }

    #[test]
    fn test_default_permissions() {
        let mut manager = PermissionManager::new();
        let perm = Permission::new("file", "read", PermissionLevel::Read);
        manager.add_default_permission(perm.clone());

        let session_id = Uuid::new_v4();
        assert!(manager.check_permission(session_id, &perm).is_ok());
    }

    #[test]
    fn test_permission_level_hierarchy() {
        let write_perm = Permission::new("file", "write", PermissionLevel::Write);
        let read_perm = Permission::new("file", "read", PermissionLevel::Read);

        // Write permission should grant read access
        assert!(write_perm.grants(PermissionLevel::Read));
        // Read permission should not grant write access
        assert!(!read_perm.grants(PermissionLevel::Write));
    }

    #[test]
    fn test_permission_serialization_roundtrip() {
        let perm = Permission::new("file", "write", PermissionLevel::Write);
        let json = serde_json::to_string(&perm).unwrap();
        let parsed: Permission = serde_json::from_str(&json).unwrap();
        assert_eq!(perm, parsed);
    }

    #[test]
    fn test_risk_level_serialization() {
        for level in [
            RiskLevel::Safe,
            RiskLevel::Low,
            RiskLevel::Medium,
            RiskLevel::High,
            RiskLevel::Critical,
        ] {
            let json = serde_json::to_string(&level).unwrap();
            let parsed: RiskLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, parsed);
        }
    }

    #[test]
    fn test_permission_choice_serialization() {
        for choice in [
            PermissionChoice::Deny,
            PermissionChoice::AllowOnce,
            PermissionChoice::AlwaysAllow,
        ] {
            let json = serde_json::to_string(&choice).unwrap();
            let parsed: PermissionChoice = serde_json::from_str(&json).unwrap();
            assert_eq!(choice, parsed);
        }
    }

    #[test]
    fn test_permission_prompt_serialization() {
        let prompt = PermissionPrompt::new(
            "file_write".to_string(),
            serde_json::json!({"path": "/tmp/test"}),
            RiskLevel::Medium,
            "Write to /tmp/test".to_string(),
        );
        let json = serde_json::to_string(&prompt).unwrap();
        let parsed: PermissionPrompt = serde_json::from_str(&json).unwrap();
        assert_eq!(prompt.id, parsed.id);
        assert_eq!(prompt.tool_name, parsed.tool_name);
        assert_eq!(prompt.risk_level, parsed.risk_level);
    }

    #[tokio::test]
    async fn test_concurrent_permission_check() {
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let manager = Arc::new(Mutex::new(PermissionManager::new()));
        let session_id = Uuid::new_v4();
        let perm = Permission::new("file", "read", PermissionLevel::Read);

        // Grant permission
        manager
            .lock()
            .await
            .grant_permission(session_id, perm.clone());

        // Concurrent reads
        let mut handles = vec![];
        for _ in 0..10 {
            let mgr = Arc::clone(&manager);
            let sid = session_id;
            let p = perm.clone();
            handles.push(tokio::spawn(async move {
                let result = mgr.lock().await.check_permission(sid, &p);
                assert!(result.is_ok());
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_concurrent_grant_and_check() {
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let manager = Arc::new(Mutex::new(PermissionManager::new()));
        let mut handles = vec![];

        // Spawn tasks that grant different permissions concurrently
        for i in 0..5 {
            let mgr = Arc::clone(&manager);
            let session_id = Uuid::new_v4();
            handles.push(tokio::spawn(async move {
                let perm = Permission::new("file", &format!("action_{i}"), PermissionLevel::Write);
                mgr.lock().await.grant_permission(session_id, perm.clone());

                let result = mgr.lock().await.check_permission(session_id, &perm);
                assert!(
                    result.is_ok(),
                    "Permission for action_{i} should be granted"
                );
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }
    }

    #[test]
    fn test_permission_manager_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<PermissionManager>();
    }

    #[test]
    fn test_permission_memory_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<PermissionMemory>();
    }

    // ── ToolPermissionPolicy tests ────────────────────────────────

    #[test]
    fn test_policy_deny_pattern_matches() {
        let policy =
            ToolPermissionPolicy::new("Bash".to_string(), RiskLevel::Medium, "Shell".to_string())
                .add_deny_pattern("rm -rf /");

        assert!(policy.is_denied("rm -rf /"));
        assert!(policy.is_denied("sudo rm -rf / --no-preserve-root"));
        assert!(!policy.is_denied("ls -la"));
    }

    #[test]
    fn test_policy_confirmation_pattern_matches() {
        let policy =
            ToolPermissionPolicy::new("Bash".to_string(), RiskLevel::Medium, "Shell".to_string())
                .add_confirmation_pattern("rm -rf");

        assert!(policy.requires_confirmation("rm -rf /home/user/dir"));
        assert!(!policy.requires_confirmation("ls -la"));
    }

    #[test]
    fn test_policy_risk_level_denied_input() {
        let policy =
            ToolPermissionPolicy::new("Bash".to_string(), RiskLevel::Medium, "Shell".to_string())
                .add_deny_pattern("rm -rf /")
                .add_confirmation_pattern("sudo");

        // Denied input → Critical
        assert_eq!(policy.risk_level_for("rm -rf /"), RiskLevel::Critical);
        // Confirmation pattern → Medium
        assert_eq!(policy.risk_level_for("sudo apt install"), RiskLevel::Medium);
        // Normal input → default (Medium)
        assert_eq!(policy.risk_level_for("ls -la"), RiskLevel::Medium);
    }

    #[test]
    fn test_policy_default_risk_level_no_patterns() {
        let policy = ToolPermissionPolicy::new(
            "Read".to_string(),
            RiskLevel::Safe,
            "Read files".to_string(),
        );
        assert_eq!(policy.risk_level_for("anything"), RiskLevel::Safe);
    }

    #[test]
    fn test_policy_builder_pattern_chaining() {
        let policy = ToolPermissionPolicy::new("T".to_string(), RiskLevel::Low, "desc".to_string())
            .add_confirmation_pattern("p1")
            .add_confirmation_pattern("p2")
            .add_deny_pattern("d1");

        assert!(policy.requires_confirmation("p1"));
        assert!(policy.requires_confirmation("p2"));
        assert!(policy.is_denied("d1"));
        assert!(!policy.is_denied("safe input"));
    }

    // ── PermissionMemory tests ─────────────────────────────────────

    #[test]
    fn test_memory_always_allow_persists() {
        let mut mem = PermissionMemory::new();
        let sid = Uuid::new_v4();
        mem.remember_choice(sid, "Bash".to_string(), PermissionChoice::AlwaysAllow);
        assert!(mem.is_always_allowed(sid, "Bash"));
    }

    #[test]
    fn test_memory_deny_persists() {
        let mut mem = PermissionMemory::new();
        let sid = Uuid::new_v4();
        mem.remember_choice(sid, "Bash".to_string(), PermissionChoice::Deny);
        assert!(mem.is_always_denied("Bash"));
    }

    #[test]
    fn test_memory_allow_once_not_remembered() {
        let mut mem = PermissionMemory::new();
        let sid = Uuid::new_v4();
        mem.remember_choice(sid, "Bash".to_string(), PermissionChoice::AllowOnce);
        assert!(!mem.is_always_allowed(sid, "Bash"));
        assert!(!mem.is_always_denied("Bash"));
    }

    #[test]
    fn test_memory_clear_session() {
        let mut mem = PermissionMemory::new();
        let sid = Uuid::new_v4();
        mem.remember_choice(sid, "Bash".to_string(), PermissionChoice::AlwaysAllow);
        assert!(mem.is_always_allowed(sid, "Bash"));
        mem.clear_session(sid);
        // review §P1-1: AlwaysAllow is now session-scoped, so clearing the
        // session drops the grant. The previous behaviour (always_allowed
        // was a process-wide HashSet) was the bug being fixed.
        assert!(
            !mem.is_always_allowed(sid, "Bash"),
            "after clear_session, the per-session AlwaysAllow must be gone"
        );
    }

    #[test]
    fn test_memory_default_is_empty() {
        let mem = PermissionMemory::new();
        let sid = Uuid::new_v4();
        assert!(!mem.is_always_allowed(sid, "Bash"));
        assert!(!mem.is_always_denied("Bash"));
    }

    // ── PermissionManager: prompt creation & choice processing ──────

    #[test]
    fn test_create_prompt_for_known_tool() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt =
            mgr.create_permission_prompt("Bash", &serde_json::json!({"command": "ls -la"}), sid);
        assert!(prompt.is_some());
        let p = prompt.unwrap();
        assert_eq!(p.tool_name, "Bash");
        assert_eq!(p.risk_level, RiskLevel::Medium);
        assert!(!p.is_confirmation);
    }

    #[test]
    fn test_create_prompt_for_unknown_tool() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt =
            mgr.create_permission_prompt("UnknownTool", &serde_json::json!({"arg": "val"}), sid);
        assert!(prompt.is_some());
        let p = prompt.unwrap();
        assert_eq!(p.tool_name, "UnknownTool");
        assert_eq!(p.risk_level, RiskLevel::Medium);
    }

    #[test]
    fn test_create_prompt_dangerous_input_elevated_risk() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt =
            mgr.create_permission_prompt("Bash", &serde_json::json!({"command": "rm -rf /"}), sid);
        let p = prompt.unwrap();
        assert_eq!(p.risk_level, RiskLevel::Critical);
    }

    #[test]
    fn test_process_choice_deny_returns_error() {
        let mut mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = PermissionPrompt::new(
            "Bash".to_string(),
            serde_json::json!({"command": "ls"}),
            RiskLevel::Medium,
            "Run ls".to_string(),
        );
        let result = mgr.process_permission_choice(sid, &prompt, PermissionChoice::Deny);
        assert!(result.is_err());
    }

    #[test]
    fn test_process_choice_allow_once_succeeds() {
        let mut mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = PermissionPrompt::new(
            "Bash".to_string(),
            serde_json::json!({"command": "ls"}),
            RiskLevel::Medium,
            "Run ls".to_string(),
        );
        assert!(
            mgr.process_permission_choice(sid, &prompt, PermissionChoice::AllowOnce)
                .is_ok()
        );
        // AllowOnce does NOT make it always allowed
        let next_prompt =
            mgr.create_permission_prompt("Bash", &serde_json::json!({"command": "ls"}), sid);
        assert!(next_prompt.is_some());
    }

    #[test]
    fn test_process_choice_always_allow_skips_future_prompts() {
        let mut mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = PermissionPrompt::new(
            "Bash".to_string(),
            serde_json::json!({"command": "ls"}),
            RiskLevel::Medium,
            "Run ls".to_string(),
        );
        assert!(
            mgr.process_permission_choice(sid, &prompt, PermissionChoice::AlwaysAllow)
                .is_ok()
        );
        let next_prompt =
            mgr.create_permission_prompt("Bash", &serde_json::json!({"command": "ls"}), sid);
        assert!(next_prompt.is_none());
    }

    // ── PermissionPrompt helpers ────────────────────────────────────

    #[test]
    fn test_prompt_confirmation_factory() {
        let p = PermissionPrompt::confirmation("Bash".to_string(), "Confirm?".to_string());
        assert!(p.is_confirmation);
        assert_eq!(p.risk_level, RiskLevel::Safe);
        assert_eq!(p.tool_input, serde_json::json!({}));
    }

    #[test]
    fn test_prompt_display_text_high_risk() {
        let p = PermissionPrompt::new(
            "Bash".to_string(),
            serde_json::json!({"cmd": "ls"}),
            RiskLevel::High,
            "Run ls".to_string(),
        );
        let text = p.display_text();
        assert!(text.contains("🔥"));
        assert!(text.contains("Bash"));
        assert!(text.contains("Run ls"));
    }

    #[test]
    fn test_prompt_display_text_safe() {
        let p = PermissionPrompt::new(
            "Read".to_string(),
            serde_json::json!({"path": "/tmp"}),
            RiskLevel::Safe,
            "Read file".to_string(),
        );
        let text = p.display_text();
        assert!(text.contains("✓"));
    }

    // ── Default policies verification ───────────────────────────────

    #[test]
    fn test_default_bash_policy_registered() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = mgr
            .create_permission_prompt("Bash", &serde_json::json!({"command": "ls"}), sid)
            .unwrap();
        assert_eq!(prompt.risk_level, RiskLevel::Medium);
    }

    #[test]
    fn test_default_write_policy_denies_etc() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = mgr
            .create_permission_prompt(
                "FileWrite",
                &serde_json::json!({"path": "/etc/passwd"}),
                sid,
            )
            .unwrap();
        assert_eq!(prompt.risk_level, RiskLevel::Critical);
    }

    #[test]
    fn test_default_read_policy_is_safe() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = mgr
            .create_permission_prompt(
                "Read",
                &serde_json::json!({"path": "/home/user/file.rs"}),
                sid,
            )
            .unwrap();
        assert_eq!(prompt.risk_level, RiskLevel::Safe);
    }

    // ── Session lifecycle ───────────────────────────────────────────

    #[test]
    fn test_clear_session_removes_permissions() {
        let mut mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let perm = Permission::new("file", "write", PermissionLevel::Write);
        mgr.grant_permission(sid, perm.clone());
        assert!(mgr.check_permission(sid, &perm).is_ok());
        mgr.clear_session(sid);
        assert!(mgr.check_permission(sid, &perm).is_err());
    }

    #[test]
    fn test_clear_session_keeps_default_permissions() {
        let mut mgr = PermissionManager::new();
        let perm = Permission::new("file", "read", PermissionLevel::Read);
        mgr.add_default_permission(perm.clone());
        let sid = Uuid::new_v4();
        mgr.clear_session(sid);
        assert!(mgr.check_permission(sid, &perm).is_ok());
    }

    #[test]
    fn test_session_permissions_merge_with_defaults() {
        let mut mgr = PermissionManager::new();
        let default_perm = Permission::new("file", "read", PermissionLevel::Read);
        mgr.add_default_permission(default_perm.clone());

        let sid = Uuid::new_v4();
        let session_perm = Permission::new("file", "write", PermissionLevel::Write);
        mgr.grant_permission(sid, session_perm.clone());

        let all = mgr.get_session_permissions(sid);
        assert!(all.contains(&default_perm));
        assert!(all.contains(&session_perm));
    }

    #[test]
    fn test_register_custom_tool_policy() {
        let mut mgr = PermissionManager::new();
        let policy = ToolPermissionPolicy::new(
            "CustomTool".to_string(),
            RiskLevel::High,
            "Custom dangerous tool".to_string(),
        )
        .add_deny_pattern("nuclear");
        mgr.register_tool_policy(policy);

        let sid = Uuid::new_v4();
        let prompt = mgr
            .create_permission_prompt(
                "CustomTool",
                &serde_json::json!({"action": "nuclear launch"}),
                sid,
            )
            .unwrap();
        assert_eq!(prompt.risk_level, RiskLevel::Critical);
    }

    // --- ApprovalMode tests ---

    #[test]
    fn test_approval_mode_default() {
        assert_eq!(ApprovalMode::default(), ApprovalMode::AutoEdit);
    }

    #[test]
    fn test_approval_mode_display() {
        assert_eq!(ApprovalMode::Ask.to_string(), "ask");
        assert_eq!(ApprovalMode::Plan.to_string(), "plan");
        assert_eq!(ApprovalMode::AutoEdit.to_string(), "auto-edit");
        assert_eq!(ApprovalMode::FullAuto.to_string(), "full-auto");
        assert_eq!(
            ApprovalMode::BypassPermissions.to_string(),
            "bypassPermissions"
        );
        assert_eq!(ApprovalMode::DontAsk.to_string(), "dontAsk");
        assert_eq!(ApprovalMode::Readonly.to_string(), "readonly");
    }

    #[test]
    fn test_approval_mode_from_str() {
        // Shannon tokens
        assert_eq!(ApprovalMode::from_str_ci("ask"), Some(ApprovalMode::Ask));
        assert_eq!(ApprovalMode::from_str_ci("plan"), Some(ApprovalMode::Plan));
        assert_eq!(
            ApprovalMode::from_str_ci("auto-edit"),
            Some(ApprovalMode::AutoEdit)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("full-auto"),
            Some(ApprovalMode::FullAuto)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("bypassPermissions"),
            Some(ApprovalMode::BypassPermissions)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("dontAsk"),
            Some(ApprovalMode::DontAsk)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("readonly"),
            Some(ApprovalMode::Readonly)
        );
        // Claude Code aliases
        assert_eq!(
            ApprovalMode::from_str_ci("default"),
            Some(ApprovalMode::Ask)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("acceptEdits"),
            Some(ApprovalMode::AutoEdit)
        );
        // Legacy aliases: `auto` keeps pointing at AutoEdit (historical
        // Shannon display name); classifier spellings map conservatively to
        // Ask; plan-readonly folds into Readonly.
        assert_eq!(
            ApprovalMode::from_str_ci("auto"),
            Some(ApprovalMode::AutoEdit)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("suggest"),
            Some(ApprovalMode::Ask)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("classifier"),
            Some(ApprovalMode::Ask)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("plan-readonly"),
            Some(ApprovalMode::Readonly)
        );
        assert_eq!(ApprovalMode::from_str_ci("ci"), Some(ApprovalMode::DontAsk));
        assert_eq!(
            ApprovalMode::from_str_ci("full-access"),
            Some(ApprovalMode::BypassPermissions)
        );
        // Case insensitive
        assert_eq!(ApprovalMode::from_str_ci("ASK"), Some(ApprovalMode::Ask));
        assert_eq!(ApprovalMode::from_str_ci("PLAN"), Some(ApprovalMode::Plan));
        assert_eq!(
            ApprovalMode::from_str_ci("AUTO-EDIT"),
            Some(ApprovalMode::AutoEdit)
        );
        // Invalid
        assert_eq!(ApprovalMode::from_str_ci("invalid"), None);
    }

    #[test]
    fn test_approval_mode_all_names() {
        let names = ApprovalMode::all_names();
        assert!(names.contains(&"ask"));
        assert!(names.contains(&"auto-edit"));
        assert!(names.contains(&"full-auto"));
        assert!(names.contains(&"plan"));
        assert!(names.contains(&"readonly"));
        assert!(names.contains(&"dontAsk"));
        assert!(names.contains(&"bypassPermissions"));
    }

    #[test]
    fn test_approval_mode_auto_approve_ask() {
        let mode = ApprovalMode::Ask;
        // Read-only tools at Low risk should be auto-approved
        assert!(mode.should_auto_approve("read", RiskLevel::Low));
        assert!(mode.should_auto_approve("glob", RiskLevel::Low));
        assert!(mode.should_auto_approve("grep", RiskLevel::Safe));
        assert!(mode.should_auto_approve("search", RiskLevel::Low));
        // Write/bash tools should NOT be auto-approved
        assert!(!mode.should_auto_approve("edit", RiskLevel::Low));
        assert!(!mode.should_auto_approve("bash", RiskLevel::Low));
        assert!(!mode.should_auto_approve("write", RiskLevel::Medium));
    }

    #[test]
    fn test_approval_mode_auto_approve_plan() {
        let mode = ApprovalMode::Plan;
        assert!(!mode.should_auto_approve("edit", RiskLevel::Low));
        assert!(!mode.should_auto_approve("bash", RiskLevel::Low));
    }

    #[test]
    fn test_approval_mode_auto_approve_auto_edit() {
        let mode = ApprovalMode::AutoEdit;
        // File tools should be auto-approved at medium risk or below
        assert!(mode.should_auto_approve("edit", RiskLevel::Low));
        assert!(mode.should_auto_approve("write", RiskLevel::Medium));
        // Bash should not be auto-approved
        assert!(!mode.should_auto_approve("bash", RiskLevel::Low));
        // High risk file tools should not be auto-approved
        assert!(!mode.should_auto_approve("edit", RiskLevel::High));
    }

    #[test]
    fn test_approval_mode_auto_approve_full_auto() {
        let mode = ApprovalMode::FullAuto;
        assert!(mode.should_auto_approve("edit", RiskLevel::Low));
        assert!(mode.should_auto_approve("bash", RiskLevel::Medium));
        assert!(mode.should_auto_approve("bash", RiskLevel::High));
        // Only critical is blocked
        assert!(!mode.should_auto_approve("bash", RiskLevel::Critical));
    }

    #[test]
    fn test_approval_mode_auto_approve_bypass_permissions() {
        let mode = ApprovalMode::BypassPermissions;
        assert!(mode.should_auto_approve("edit", RiskLevel::Low));
        assert!(mode.should_auto_approve("bash", RiskLevel::Critical));
        assert!(mode.should_auto_approve("anything", RiskLevel::Critical));
    }

    #[test]
    fn test_approval_mode_auto_approve_dont_ask() {
        let mode = ApprovalMode::DontAsk;
        assert!(mode.should_auto_approve("edit", RiskLevel::Low));
        assert!(mode.should_auto_approve("bash", RiskLevel::Critical));
    }

    #[test]
    fn test_set_and_get_approval_mode() {
        let mut mgr = PermissionManager::new();
        assert_eq!(mgr.approval_mode(), ApprovalMode::AutoEdit);
        mgr.set_approval_mode(ApprovalMode::FullAuto);
        assert_eq!(mgr.approval_mode(), ApprovalMode::FullAuto);
        mgr.set_approval_mode(ApprovalMode::Readonly);
        assert_eq!(mgr.approval_mode(), ApprovalMode::Readonly);
        mgr.set_approval_mode(ApprovalMode::Plan);
        assert_eq!(mgr.approval_mode(), ApprovalMode::Plan);
        mgr.set_approval_mode(ApprovalMode::BypassPermissions);
        assert_eq!(mgr.approval_mode(), ApprovalMode::BypassPermissions);
        mgr.set_approval_mode(ApprovalMode::DontAsk);
        assert_eq!(mgr.approval_mode(), ApprovalMode::DontAsk);
    }

    #[test]
    fn test_readonly_mode_blocks_writes() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Readonly);
        let sid = Uuid::new_v4();

        // Read tools should be allowed
        let result = mgr.classify_and_check(sid, "read", &serde_json::json!({"path": "/tmp/test"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // auto-allowed

        // Write tools should be denied
        let result = mgr.classify_and_check(sid, "bash", &serde_json::json!({"command": "ls"}));
        assert!(result.is_err());
    }

    // --- review F17: name fast-path defers to registered tool metadata ---

    #[test]
    fn test_plugin_read_only_name_with_mutating_flags_is_not_auto_approved() {
        // A plugin registering under the built-in read-only name "file_info"
        // with mutating trait flags must NOT slip through the name fast-path:
        // Ask mode prompts, Readonly refuses.
        let mut mgr = PermissionManager::new();
        mgr.register_tool_read_only("file_info".to_string(), false);
        let sid = Uuid::new_v4();

        mgr.set_approval_mode(ApprovalMode::Ask);
        let result = mgr.classify_and_check(sid, "file_info", &serde_json::json!({}));
        assert!(
            matches!(result, Ok(Some(_))),
            "known-mutating 'file_info' must require confirmation, got {result:?}"
        );

        mgr.set_approval_mode(ApprovalMode::Readonly);
        assert!(
            mgr.classify_and_check(sid, "file_info", &serde_json::json!({}))
                .is_err(),
            "known-mutating 'file_info' must be denied in Readonly mode"
        );
    }

    #[test]
    fn test_destructive_registration_vetoes_read_only_fast_path() {
        // MCP `annotations.destructiveHint` on a tool named "ls" must block
        // the name fast-path even without read-only metadata.
        let mut mgr = PermissionManager::new();
        mgr.register_destructive_tool("ls".to_string());
        let sid = Uuid::new_v4();
        mgr.set_approval_mode(ApprovalMode::Ask);
        let result = mgr.classify_and_check(sid, "ls", &serde_json::json!({}));
        assert!(
            matches!(result, Ok(Some(_))),
            "destructive-flagged 'ls' must require confirmation, got {result:?}"
        );
    }

    #[test]
    fn test_builtin_read_tools_keep_fast_path() {
        // Genuine built-ins never register metadata: the fast path applies.
        let mut mgr = PermissionManager::new();
        let sid = Uuid::new_v4();

        mgr.set_approval_mode(ApprovalMode::Ask);
        for tool in ["Read", "read_file", "Grep", "file_info"] {
            let result = mgr.classify_and_check(sid, tool, &serde_json::json!({}));
            assert!(
                matches!(&result, Ok(None)),
                "built-in '{tool}' must stay auto-approved in Suggest mode, got {result:?}"
            );
        }

        // A tool REGISTERED as genuinely read-only also keeps the fast path.
        mgr.register_tool_read_only("file_info".to_string(), true);
        mgr.set_approval_mode(ApprovalMode::Readonly);
        let result = mgr.classify_and_check(sid, "file_info", &serde_json::json!({}));
        assert!(
            matches!(&result, Ok(None)),
            "registered read-only 'file_info' must keep the fast path, got {result:?}"
        );
    }

    // --- New permission mode tests ---

    #[test]
    fn test_bypass_permissions_skips_all_checks() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::BypassPermissions);
        let sid = Uuid::new_v4();

        // Even critical-risk bash commands should be auto-approved
        let result =
            mgr.classify_and_check(sid, "Bash", &serde_json::json!({"command": "rm -rf /"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // no prompt needed
    }

    #[test]
    fn test_dont_ask_never_waits_denies_unapproved() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::DontAsk);
        let sid = Uuid::new_v4();

        // Unapproved, non-read tool: denied instead of prompting (never waits)
        let result = mgr.classify_and_check(
            sid,
            "Bash",
            &serde_json::json!({"command": "anything dangerous"}),
        );
        assert!(result.is_err());

        // Reads still pass
        let result = mgr.classify_and_check(sid, "Read", &serde_json::json!({"path": "/tmp"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        // Allow-listed tools pass
        mgr.allow_tool("Bash");
        let result =
            mgr.classify_and_check(sid, "Bash", &serde_json::json!({"command": "cargo test"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_plan_mode_without_approval_prompts() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Plan);
        let sid = Uuid::new_v4();

        // Without plan approval, should prompt like Suggest mode
        let result = mgr.classify_and_check(sid, "SomeTool", &serde_json::json!({"arg": "val"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_some()); // prompt needed
    }

    #[test]
    fn test_plan_mode_with_approval_auto_approves() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Plan);
        let sid = Uuid::new_v4();
        mgr.approve_plan(sid);

        // With plan approval, all tools should be auto-approved
        let result =
            mgr.classify_and_check(sid, "Bash", &serde_json::json!({"command": "cargo build"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // no prompt needed
    }

    #[test]
    fn test_plan_mode_clear_approval() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Plan);
        let sid = Uuid::new_v4();
        mgr.approve_plan(sid);
        assert!(mgr.is_plan_approved(sid));

        mgr.clear_plan_approval(sid);
        assert!(!mgr.is_plan_approved(sid));

        // Should prompt again after clearing
        let result = mgr.classify_and_check(sid, "SomeTool", &serde_json::json!({}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_clear_session_clears_plan_approval() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Plan);
        let sid = Uuid::new_v4();
        mgr.approve_plan(sid);
        assert!(mgr.is_plan_approved(sid));

        mgr.clear_session(sid);
        assert!(!mgr.is_plan_approved(sid));
    }

    #[test]
    fn test_plan_mode_always_allowed_still_works() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Plan);
        let sid = Uuid::new_v4();
        // No plan approval, but tool is always-allowed
        mgr.allow_tool("Read");

        let result = mgr.classify_and_check(sid, "Read", &serde_json::json!({"path": "/tmp/test"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // auto-allowed via memory
    }

    // ── Glob pattern permission tests ──────────────────────────────────

    #[test]
    fn test_glob_allow_pattern_mcp_server() {
        let mut mem = PermissionMemory::new();
        let sid = Uuid::new_v4();

        mem.allow_pattern("mcp__github__*");

        assert!(mem.is_always_allowed(sid, "mcp__github__create_issue"));
        assert!(mem.is_always_allowed(sid, "mcp__github__list_repos"));
        assert!(mem.is_always_allowed(sid, "mcp__github__search_code"));
        assert!(!mem.is_always_allowed(sid, "mcp__other__create_issue"));
        assert!(!mem.is_always_allowed(sid, "Bash"));
    }

    #[test]
    fn test_glob_deny_pattern() {
        let mut mem = PermissionMemory::new();

        mem.deny_pattern("mcp__*__delete_*");

        assert!(mem.is_always_denied("mcp__github__delete_repo"));
        assert!(mem.is_always_denied("mcp__db__delete_record"));
        assert!(!mem.is_always_denied("mcp__github__create_issue"));
        assert!(!mem.is_always_denied("Bash"));
    }

    #[test]
    fn test_glob_and_exact_match_coexist() {
        let mut mem = PermissionMemory::new();
        let sid = Uuid::new_v4();

        mem.allow_tool("Bash");
        mem.allow_pattern("mcp__server__*");

        assert!(mem.is_always_allowed(sid, "Bash"));
        assert!(mem.is_always_allowed(sid, "mcp__server__tool1"));
        assert!(!mem.is_always_allowed(sid, "mcp__other__tool"));
    }

    #[test]
    fn test_glob_wildcard_all_mcp() {
        let mut mem = PermissionMemory::new();
        let sid = Uuid::new_v4();

        mem.allow_pattern("mcp__*");
        assert!(mem.is_always_allowed(sid, "mcp__anything__here"));
        assert!(mem.is_always_allowed(sid, "mcp__server__tool"));
        assert!(!mem.is_always_allowed(sid, "Bash"));
    }

    #[test]
    fn test_glob_invalid_pattern_ignored() {
        let mut mem = PermissionMemory::new();
        mem.allow_pattern("[invalid");
        assert!(!mem.is_always_denied("anything"));
    }

    #[test]
    fn test_manager_allow_pattern_auto_approves() {
        let mut mgr = PermissionManager::new();
        let sid = Uuid::new_v4();

        mgr.allow_pattern("mcp__github__*");

        let result = mgr.classify_and_check(
            sid,
            "mcp__github__list_prs",
            &serde_json::json!({"repo": "org/repo"}),
        );
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // auto-approved via glob
    }

    // ── Ladder-mode gate tests (post-convergence) ───────────────────────

    #[test]
    fn test_ask_mode_auto_approves_reads_and_prompts_writes() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Ask);
        let sid = Uuid::new_v4();

        // Reads pass without prompting
        let result = mgr.classify_and_check(sid, "Read", &serde_json::json!({"path": "/tmp/test"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        // File writes prompt (Ask does not pre-approve edits)
        let result =
            mgr.classify_and_check(sid, "FileWrite", &serde_json::json!({"path": "/tmp/test"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_full_auto_denies_critical_bash() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::FullAuto);
        let sid = Uuid::new_v4();

        // Critical-risk bash (built-in dangerous pattern) is denied
        let result =
            mgr.classify_and_check(sid, "Bash", &serde_json::json!({"command": "rm -rf /"}));
        assert!(result.is_err());
    }

    #[test]
    fn test_readonly_mode_allows_read_tools() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Readonly);
        let sid = Uuid::new_v4();

        // Read tools should be allowed
        let result = mgr.classify_and_check(sid, "read", &serde_json::json!({"path": "/tmp/test"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // auto-allowed
    }

    #[test]
    fn test_readonly_mode_denies_write_tools() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Readonly);
        let sid = Uuid::new_v4();

        // Write tools should be denied
        let result = mgr.classify_and_check(sid, "bash", &serde_json::json!({"command": "ls"}));
        assert!(result.is_err()); // denied
    }

    #[test]
    fn test_legacy_mode_aliases_fold_into_ladder() {
        // Display strings for the 7-mode set
        assert_eq!(ApprovalMode::Ask.to_string(), "ask");
        assert_eq!(ApprovalMode::AutoEdit.to_string(), "auto-edit");

        // Removed variants' spellings fold into the ladder conservatively
        assert_eq!(
            ApprovalMode::from_str_ci("auto-classifier"),
            Some(ApprovalMode::Ask)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("auto_classifier"),
            Some(ApprovalMode::Ask)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("classifier"),
            Some(ApprovalMode::Ask)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("plan-readonly"),
            Some(ApprovalMode::Readonly)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("plan_readonly"),
            Some(ApprovalMode::Readonly)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("plan_ro"),
            Some(ApprovalMode::Readonly)
        );
    }

    #[test]
    fn test_approval_mode_cycle_is_three_ladder_stops() {
        // cycle_next() cycles the autonomy ladder: Ask → AutoEdit → FullAuto → Ask
        let mut current = ApprovalMode::Ask;
        current = current.cycle_next();
        assert_eq!(current, ApprovalMode::AutoEdit);
        current = current.cycle_next();
        assert_eq!(current, ApprovalMode::FullAuto);
        current = current.cycle_next();
        assert_eq!(current, ApprovalMode::Ask);

        // Plan and the expert modes are not cycle stops — they reset to Ask
        assert_eq!(ApprovalMode::Plan.cycle_next(), ApprovalMode::Ask);
        assert_eq!(ApprovalMode::Readonly.cycle_next(), ApprovalMode::Ask);
        assert_eq!(ApprovalMode::DontAsk.cycle_next(), ApprovalMode::Ask);
        assert_eq!(
            ApprovalMode::BypassPermissions.cycle_next(),
            ApprovalMode::Ask
        );
    }

    #[test]
    fn test_approval_mode_short_labels_are_bijective() {
        // Every mode has a unique status-bar label and every label round-trips
        let modes = [
            ApprovalMode::Ask,
            ApprovalMode::AutoEdit,
            ApprovalMode::FullAuto,
            ApprovalMode::Plan,
            ApprovalMode::Readonly,
            ApprovalMode::DontAsk,
            ApprovalMode::BypassPermissions,
        ];
        let mut seen = std::collections::HashSet::new();
        for mode in &modes {
            let label = mode.short_label();
            assert!(seen.insert(label), "duplicate label {label}");
            assert_eq!(ApprovalMode::from_label(label), Some(*mode));
        }
        assert_eq!(ApprovalMode::Ask.short_label(), "ASK");
        assert_eq!(ApprovalMode::BypassPermissions.short_label(), "BYPASS");
        assert_eq!(ApprovalMode::DontAsk.short_label(), "CI");
    }

    #[test]
    fn test_approval_mode_descriptions() {
        assert!(ApprovalMode::Ask.description().contains("Reads run freely"));
        assert!(
            ApprovalMode::AutoEdit
                .description()
                .contains("File edits run without asking")
        );
        assert!(
            ApprovalMode::FullAuto
                .description()
                .contains("below critical risk")
        );
        assert!(
            ApprovalMode::Readonly
                .description()
                .contains("read operations")
        );
        assert!(ApprovalMode::DontAsk.description().contains("Never waits"));
        assert!(
            ApprovalMode::BypassPermissions
                .description()
                .contains("Skip all checks")
        );
    }

    // ── P0/P3-2 convergence regression tests ────────────────────────────

    #[test]
    fn test_deny_rules_bind_in_every_mode_including_bypass() {
        // P0-3: the global deny gate runs before mode overrides — matching
        // Claude Code, deny is the one rule no mode may override.
        let mut mgr = PermissionManager::new();
        mgr.deny_tool("Bash");
        let sid = Uuid::new_v4();
        for mode in [
            ApprovalMode::Ask,
            ApprovalMode::AutoEdit,
            ApprovalMode::FullAuto,
            ApprovalMode::BypassPermissions,
            ApprovalMode::DontAsk,
            ApprovalMode::Readonly,
        ] {
            mgr.set_approval_mode(mode);
            let result =
                mgr.classify_and_check(sid, "Bash", &serde_json::json!({"command": "echo hi"}));
            assert!(
                result.is_err(),
                "deny must bind in {mode:?}, got {result:?}"
            );
        }
    }

    #[test]
    fn test_dont_ask_denies_instead_of_prompting() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::DontAsk);
        let sid = Uuid::new_v4();
        let result = mgr.classify_and_check(sid, "Write", &serde_json::json!({"path": "/tmp/x"}));
        assert!(matches!(result, Err(PermissionError::Denied(_))));
        // …and never silently waves a critical tool through
        assert!(result.is_err());
    }

    #[test]
    fn test_auto_approval_budget_forces_prompt() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::FullAuto);
        mgr.set_max_auto_approvals(2);
        let sid = Uuid::new_v4();
        let input = serde_json::json!({"path": "/tmp/x"});

        // Two auto-approvals pass…
        for _ in 0..2 {
            let r = mgr.classify_and_check(sid, "FileWrite", &input).unwrap();
            assert!(r.is_none(), "within budget must auto-approve");
        }
        // …the third is forced to a human decision, labeled as a budget stop.
        let r = mgr.classify_and_check(sid, "FileWrite", &input).unwrap();
        let prompt = r.expect("budget exhausted must prompt");
        assert!(prompt.limit_triggered);
        // A human decision resets the stretch.
        mgr.process_permission_choice(sid, &prompt, PermissionChoice::AllowOnce)
            .unwrap();
        assert_eq!(mgr.auto_approval_count(), 0);
        let r = mgr.classify_and_check(sid, "FileWrite", &input).unwrap();
        assert!(r.is_none());
        // Mode change also resets.
        mgr.set_approval_mode(ApprovalMode::Ask);
        assert_eq!(mgr.auto_approval_count(), 0);
    }

    #[test]
    fn test_plan_snapshot_restore_roundtrip() {
        // Design §5: entering plan snapshots the ladder mode; exit restores
        // it and clears approval.
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::FullAuto);
        let sid = Uuid::new_v4();

        mgr.enter_plan_mode(sid);
        assert_eq!(mgr.approval_mode(), ApprovalMode::Plan);

        // Approved plan runs at the full-auto floor: medium bash auto-runs.
        mgr.approve_plan(sid);
        let r = mgr
            .classify_and_check(sid, "Bash", &serde_json::json!({"command": "cargo build"}))
            .unwrap();
        assert!(r.is_none(), "approved plan auto-runs at full-auto floor");

        mgr.exit_plan_mode(sid);
        assert_eq!(mgr.approval_mode(), ApprovalMode::FullAuto);
        assert!(!mgr.is_plan_approved(sid));

        // Re-enter: a second enter while already planning does not clobber
        // the original snapshot.
        mgr.set_approval_mode(ApprovalMode::AutoEdit);
        mgr.enter_plan_mode(sid);
        mgr.enter_plan_mode(sid);
        mgr.exit_plan_mode(sid);
        assert_eq!(mgr.approval_mode(), ApprovalMode::AutoEdit);
    }

    #[test]
    fn test_settings_rules_wire_ask_and_default_mode() {
        let mut pm = PermissionManager::new();
        let perms = serde_json::json!({
            "allow": ["Bash(git *)"],
            "ask": ["WebFetch"],
            "deny": ["Bash(sudo *)"],
            "defaultMode": "readonly",
        });
        super::apply_settings_permissions(&mut pm, &perms, false);

        assert_eq!(pm.approval_mode(), ApprovalMode::Readonly);

        // ask-rule forces a prompt even in full-auto…
        pm.set_approval_mode(ApprovalMode::FullAuto);
        let r = pm
            .classify_and_check(
                sid_of(),
                "WebFetch",
                &serde_json::json!({"url": "https://x"}),
            )
            .unwrap();
        assert!(r.is_some(), "ask rules must force prompts");

        // deny-rule blocks even bypass…
        pm.set_approval_mode(ApprovalMode::BypassPermissions);
        assert!(
            pm.classify_and_check(
                sid_of(),
                "Bash",
                &serde_json::json!({"command": "sudo rm x"})
            )
            .is_err()
        );
        // …and the allow-rule auto-approves under ask mode.
        pm.set_approval_mode(ApprovalMode::Ask);
        let r = pm
            .classify_and_check(
                sid_of(),
                "Bash",
                &serde_json::json!({"command": "git status"}),
            )
            .unwrap();
        assert!(r.is_none(), "allow rules pre-approve");
    }

    fn sid_of() -> uuid::Uuid {
        uuid::Uuid::nil()
    }

    #[test]
    fn test_bypass_guardrail_helpers() {
        // The env kill switch must refuse bypass entry regardless of user.
        // SAFETY: single-threaded test process; no other thread reads env.
        unsafe { std::env::set_var("SHANNON_DISABLE_BYPASS", "1") };
        assert!(super::ensure_bypass_allowed().is_err());
        unsafe { std::env::remove_var("SHANNON_DISABLE_BYPASS") };
        // Non-root (test runners) pass when the switch is unset.
        if !super::running_as_root() {
            assert!(super::ensure_bypass_allowed().is_ok());
        }
    }

    // ── PermissionRule and PermissionRuleSet tests ──────────────────────

    #[test]
    fn test_permission_rule_creation() {
        let rule = PermissionRule::new(
            "Bash(git *)".to_string(),
            PermissionRuleDecision::Allow,
            PermissionRuleSource::User,
        );
        assert_eq!(rule.pattern, "Bash(git *)");
        assert_eq!(rule.decision, PermissionRuleDecision::Allow);
        assert_eq!(rule.source, PermissionRuleSource::User);
        assert!(rule.description.is_none());
    }

    #[test]
    fn test_permission_rule_with_description() {
        let rule = PermissionRule::with_description(
            "Read(*)".to_string(),
            PermissionRuleDecision::Allow,
            PermissionRuleSource::Project,
            "Allow all read operations".to_string(),
        );
        assert_eq!(
            rule.description,
            Some("Allow all read operations".to_string())
        );
    }

    #[test]
    fn test_permission_rule_matches_tool_only() {
        let rule = PermissionRule::new(
            "Bash".to_string(),
            PermissionRuleDecision::Ask,
            PermissionRuleSource::Managed,
        );
        assert!(rule.matches("Bash", "any command"));
        assert!(rule.matches("Bash", "ls -la"));
        assert!(!rule.matches("Read", "something"));
    }

    #[test]
    fn test_permission_rule_matches_tool_with_wildcard() {
        let rule = PermissionRule::new(
            "Bash(*)".to_string(),
            PermissionRuleDecision::Allow,
            PermissionRuleSource::User,
        );
        assert!(rule.matches("Bash", "any command"));
        assert!(rule.matches("Bash", "ls -la"));
        assert!(!rule.matches("Read", "something"));
    }

    #[test]
    fn test_permission_rule_matches_tool_with_pattern() {
        let rule = PermissionRule::new(
            "Bash(git *)".to_string(),
            PermissionRuleDecision::Allow,
            PermissionRuleSource::Project,
        );
        assert!(rule.matches("Bash", "git status"));
        assert!(rule.matches("Bash", "git commit -m 'test'"));
        assert!(!rule.matches("Bash", "ls -la"));
        assert!(!rule.matches("Read", "git status"));
    }

    #[test]
    fn test_permission_rule_regex_injection_prevented() {
        // Regex metacharacters in patterns should be escaped, not interpreted.
        // Without regex::escape(), "Bash((?:a+)+b)" would be treated as regex
        // and could cause ReDoS.
        let rule = PermissionRule::new(
            "Bash((?:a+)+b)".to_string(),
            PermissionRuleDecision::Deny,
            PermissionRuleSource::User,
        );
        // The literal pattern should NOT match "Bash" with "aaaaab"
        assert!(!rule.matches("Bash", "aaaaab"));
        // But it should match the literal string "(?:a+)+b"
        assert!(rule.matches("Bash", "(?:a+)+b"));
    }

    #[test]
    fn test_permission_rule_special_chars_in_pattern() {
        // Ensure characters like . + ? ( ) [ ] { } | ^ $ are treated literally
        let rule = PermissionRule::new(
            "Bash(git stash@{0})".to_string(),
            PermissionRuleDecision::Allow,
            PermissionRuleSource::Project,
        );
        assert!(rule.matches("Bash", "git stash@{0}"));
        assert!(!rule.matches("Bash", "git stash@{1}"));
    }

    #[test]
    fn test_permission_rule_set_creation() {
        let rule_set = PermissionRuleSet::new();
        assert_eq!(rule_set.rules().len(), 0);
    }

    #[test]
    fn test_permission_rule_set_add_rule() {
        let mut rule_set = PermissionRuleSet::new();
        let rule = PermissionRule::new(
            "Bash(git *)".to_string(),
            PermissionRuleDecision::Allow,
            PermissionRuleSource::User,
        );
        rule_set.add_rule(rule);
        assert_eq!(rule_set.rules().len(), 1);
    }

    #[test]
    fn test_permission_rule_set_with_builder() {
        let rule_set = PermissionRuleSet::new()
            .with_rule(PermissionRule::new(
                "Read(*)".to_string(),
                PermissionRuleDecision::Allow,
                PermissionRuleSource::Managed,
            ))
            .with_rule(PermissionRule::new(
                "Bash(rm *)".to_string(),
                PermissionRuleDecision::Deny,
                PermissionRuleSource::User,
            ));
        assert_eq!(rule_set.rules().len(), 2);
    }

    #[test]
    fn test_permission_rule_set_evaluate_first_match_wins() {
        let mut rule_set = PermissionRuleSet::new();
        rule_set.add_rule(PermissionRule::new(
            "Bash(*)".to_string(),
            PermissionRuleDecision::Deny,
            PermissionRuleSource::Managed,
        ));
        rule_set.add_rule(PermissionRule::new(
            "Bash(git *)".to_string(),
            PermissionRuleDecision::Allow,
            PermissionRuleSource::User,
        ));

        // First matching rule should win (Deny)
        let result = rule_set.evaluate("Bash", "git status");
        assert_eq!(result, Some(PermissionRuleDecision::Deny));
    }

    #[test]
    fn test_permission_rule_set_evaluate_no_match() {
        let rule_set = PermissionRuleSet::new().with_rule(PermissionRule::new(
            "Read(*)".to_string(),
            PermissionRuleDecision::Allow,
            PermissionRuleSource::Managed,
        ));

        let result = rule_set.evaluate("Bash", "ls -la");
        assert_eq!(result, None);
    }

    #[test]
    fn test_permission_rule_set_evaluate_ordered() {
        let mut rule_set = PermissionRuleSet::new();
        // Add rules in reverse order - first one should still win
        rule_set.add_rule(PermissionRule::new(
            "Bash(git *)".to_string(),
            PermissionRuleDecision::Allow,
            PermissionRuleSource::User,
        ));
        rule_set.add_rule(PermissionRule::new(
            "Bash(*)".to_string(),
            PermissionRuleDecision::Ask,
            PermissionRuleSource::Managed,
        ));

        let result = rule_set.evaluate("Bash", "git status");
        assert_eq!(result, Some(PermissionRuleDecision::Allow));
    }

    #[test]
    fn test_permission_rule_set_clear() {
        let mut rule_set = PermissionRuleSet::new()
            .with_rule(PermissionRule::new(
                "Read(*)".to_string(),
                PermissionRuleDecision::Allow,
                PermissionRuleSource::Managed,
            ))
            .with_rule(PermissionRule::new(
                "Bash(*)".to_string(),
                PermissionRuleDecision::Ask,
                PermissionRuleSource::User,
            ));
        assert_eq!(rule_set.rules().len(), 2);

        rule_set.clear();
        assert_eq!(rule_set.rules().len(), 0);
    }

    #[test]
    fn test_permission_rule_set_remove_by_source() {
        let mut rule_set = PermissionRuleSet::new();
        rule_set.add_rule(PermissionRule::new(
            "Read(*)".to_string(),
            PermissionRuleDecision::Allow,
            PermissionRuleSource::Managed,
        ));
        rule_set.add_rule(PermissionRule::new(
            "Bash(*)".to_string(),
            PermissionRuleDecision::Ask,
            PermissionRuleSource::User,
        ));
        rule_set.add_rule(PermissionRule::new(
            "Write(*)".to_string(),
            PermissionRuleDecision::Allow,
            PermissionRuleSource::User,
        ));
        assert_eq!(rule_set.rules().len(), 3);

        rule_set.remove_by_source(&PermissionRuleSource::User);
        assert_eq!(rule_set.rules().len(), 1);
        assert_eq!(rule_set.rules()[0].source, PermissionRuleSource::Managed);
    }

    #[test]
    fn test_permission_rule_serialization() {
        let rule = PermissionRule::with_description(
            "Bash(git *)".to_string(),
            PermissionRuleDecision::Allow,
            PermissionRuleSource::Project,
            "Allow git commands".to_string(),
        );

        let json = serde_json::to_string(&rule).unwrap();
        let parsed: PermissionRule = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.pattern, rule.pattern);
        assert_eq!(parsed.decision, rule.decision);
        assert_eq!(parsed.source, rule.source);
        assert_eq!(parsed.description, rule.description);
    }

    #[test]
    fn test_permission_rule_decision_serialization() {
        for decision in [
            PermissionRuleDecision::Allow,
            PermissionRuleDecision::Deny,
            PermissionRuleDecision::Ask,
        ] {
            let json = serde_json::to_string(&decision).unwrap();
            let parsed: PermissionRuleDecision = serde_json::from_str(&json).unwrap();
            assert_eq!(decision, parsed);
        }
    }

    #[test]
    fn test_permission_rule_source_serialization() {
        for source in [
            PermissionRuleSource::User,
            PermissionRuleSource::Project,
            PermissionRuleSource::Managed,
        ] {
            let json = serde_json::to_string(&source).unwrap();
            let parsed: PermissionRuleSource = serde_json::from_str(&json).unwrap();
            assert_eq!(source, parsed);
        }
    }

    // ── PermissionRuleChecker tests ─────────────────────────────────────

    #[test]
    fn test_rule_checker_deny_overrides_allow() {
        let deny = vec!["Bash(rm -rf /)".to_string()];
        let allow = vec!["Bash(*)".to_string()];
        let checker = PermissionRuleChecker::from_rule_strings(&deny, &[], &allow);
        // deny wins even though allow matches too
        assert_eq!(checker.check("Bash", "rm -rf /"), RuleCheckDecision::Denied);
        // non-denied bash command is allowed
        assert_eq!(checker.check("Bash", "ls -la"), RuleCheckDecision::Allowed);
    }

    #[test]
    fn test_rule_checker_deny_overrides_ask() {
        let deny = vec!["Bash(rm *)".to_string()];
        let ask = vec!["Bash(*)".to_string()];
        let checker = PermissionRuleChecker::from_rule_strings(&deny, &ask, &[]);
        // deny wins
        assert_eq!(
            checker.check("Bash", "rm file.txt"),
            RuleCheckDecision::Denied
        );
        // non-denied falls to ask
        assert_eq!(checker.check("Bash", "ls"), RuleCheckDecision::Ask);
    }

    #[test]
    fn test_rule_checker_ask_overrides_allow() {
        let ask = vec!["Bash(*)".to_string()];
        let allow = vec!["Bash(git *)".to_string()];
        let checker = PermissionRuleChecker::from_rule_strings(&[], &ask, &allow);
        // ask wins
        assert_eq!(checker.check("Bash", "git status"), RuleCheckDecision::Ask);
    }

    #[test]
    fn test_rule_checker_no_match_returns_no_match() {
        let allow = vec!["Read(*)".to_string()];
        let checker = PermissionRuleChecker::from_rule_strings(&[], &[], &allow);
        assert_eq!(checker.check("Bash", "ls"), RuleCheckDecision::NoMatch);
    }

    #[test]
    fn test_rule_checker_empty_rules_is_empty() {
        let checker = PermissionRuleChecker::default();
        assert!(checker.is_empty());
    }

    #[test]
    fn test_rule_checker_non_empty_is_not_empty() {
        let deny = vec!["Bash(*)".to_string()];
        let checker = PermissionRuleChecker::from_rule_strings(&deny, &[], &[]);
        assert!(!checker.is_empty());
    }

    #[test]
    fn test_rule_checker_glob_pattern_matching() {
        let allow = vec!["mcp__github__*".to_string()];
        let deny = vec!["mcp__*__delete_*".to_string()];
        let checker = PermissionRuleChecker::from_rule_strings(&deny, &[], &allow);
        // glob allow
        assert_eq!(
            checker.check("mcp__github__list_prs", ""),
            RuleCheckDecision::Allowed
        );
        // glob deny overrides glob allow
        assert_eq!(
            checker.check("mcp__github__delete_repo", ""),
            RuleCheckDecision::Denied
        );
        // no match
        assert_eq!(checker.check("Bash", "ls"), RuleCheckDecision::NoMatch);
    }

    #[test]
    fn test_rule_checker_tool_name_pattern() {
        let allow = vec!["Read".to_string()];
        let checker = PermissionRuleChecker::from_rule_strings(&[], &[], &allow);
        assert_eq!(
            checker.check("Read", "any file"),
            RuleCheckDecision::Allowed
        );
        assert_eq!(
            checker.check("Write", "any file"),
            RuleCheckDecision::NoMatch
        );
    }

    #[test]
    fn test_rule_checker_bare_star_allows_all() {
        let allow = vec!["*".to_string()];
        let checker = PermissionRuleChecker::from_rule_strings(&[], &[], &allow);
        assert_eq!(
            checker.check("Bash", "anything"),
            RuleCheckDecision::Allowed
        );
        assert_eq!(
            checker.check("Read", "anything"),
            RuleCheckDecision::Allowed
        );
    }

    #[test]
    fn test_rule_checker_integration_with_manager() {
        let mut mgr = PermissionManager::new();
        let deny = vec!["Bash(rm -rf /)".to_string()];
        let allow = vec!["Bash(git *)".to_string()];
        mgr.set_rule_checker(PermissionRuleChecker::from_rule_strings(&deny, &[], &allow));

        let sid = Uuid::new_v4();
        // denied command
        let result =
            mgr.classify_and_check(sid, "Bash", &serde_json::json!({"command": "rm -rf /"}));
        assert!(result.is_err());

        // allowed command
        let result =
            mgr.classify_and_check(sid, "Bash", &serde_json::json!({"command": "git status"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // auto-approved
    }

    // ── P1-3 decision-reason tests ──────────────────────────────────────

    #[test]
    fn test_decision_reason_constructors_and_serde() {
        let rule = DecisionReason::rule("Bash(git *)");
        assert_eq!(rule.source, ReasonSource::Rule);
        assert_eq!(rule.rule_name.as_deref(), Some("Bash(git *)"));
        assert!(rule.confidence.is_none());

        let llm = DecisionReason::llm(0.74);
        assert_eq!(llm.source, ReasonSource::Llm);
        assert!((llm.confidence.unwrap() - 0.74).abs() < 1e-6);

        assert_eq!(DecisionReason::default(), DecisionReason::default_reason());
        assert_eq!(
            serde_json::to_string(&ReasonSource::Rule).unwrap(),
            "\"rule\""
        );
        assert_eq!(
            serde_json::to_string(&ReasonSource::Llm).unwrap(),
            "\"llm\""
        );
        assert_eq!(
            serde_json::to_string(&ReasonSource::Default).unwrap(),
            "\"default\""
        );
    }

    #[test]
    fn test_check_with_rule_returns_matched_pattern() {
        let ask = vec!["Bash(git push *)".to_string()];
        let checker = PermissionRuleChecker::from_rule_strings(&[], &ask, &[]);
        let (decision, rule) = checker.check_with_rule("Bash", "git push origin main");
        assert_eq!(decision, RuleCheckDecision::Ask);
        assert_eq!(rule.as_deref(), Some("Bash(git push *)"));

        // No match → no pattern
        let (decision, rule) = checker.check_with_rule("Bash", "ls");
        assert_eq!(decision, RuleCheckDecision::NoMatch);
        assert!(rule.is_none());
    }

    #[test]
    fn test_check_with_rule_compiled_glob_only_match_has_no_pattern() {
        // A plain glob never matches via the raw structured loop — only the
        // compiled globset hits it, and no raw pattern text is available then.
        // The decision must be unchanged; only the pattern text is absent.
        let allow = vec!["mcp__github__*".to_string()];
        let checker = PermissionRuleChecker::from_rule_strings(&[], &[], &allow);
        let (decision, rule) = checker.check_with_rule("mcp__github__list_prs", "");
        assert_eq!(decision, RuleCheckDecision::Allowed);
        assert!(rule.is_none(), "globset-only match has no raw pattern");
        // `check` stays decision-identical.
        assert_eq!(checker.check("mcp__github__list_prs", ""), decision);
    }

    #[test]
    fn test_ask_rule_prompt_carries_rule_reason() {
        let mut mgr = PermissionManager::new();
        let ask = vec!["Bash(git push *)".to_string()];
        mgr.set_rule_checker(PermissionRuleChecker::from_rule_strings(&[], &ask, &[]));

        let sid = Uuid::new_v4();
        let prompt = mgr
            .classify_and_check(
                sid,
                "Bash",
                &serde_json::json!({"command": "git push origin"}),
            )
            .expect("ask rule must not error")
            .expect("ask rule must prompt");
        assert_eq!(prompt.reason.source, ReasonSource::Rule);
        assert_eq!(prompt.reason.rule_name.as_deref(), Some("Bash(git push *)"));
    }

    #[test]
    fn test_destructive_tool_prompt_has_default_reason() {
        let mut mgr = PermissionManager::new();
        mgr.register_destructive_tool("DangerousTool".to_string());
        let sid = Uuid::new_v4();
        let prompt = mgr
            .create_permission_prompt("DangerousTool", &serde_json::json!({"a": 1}), sid)
            .expect("destructive tool must prompt");
        assert_eq!(prompt.reason, DecisionReason::default_reason());
        assert_eq!(prompt.reason.source, ReasonSource::Default);
    }

    #[test]
    fn test_classifier_prompt_reason_matches_classifier_verdict() {
        // Suggest mode: non-read tool falls through to the rule classifier,
        // whose verdict the prompt's reason must mirror exactly.
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Ask);
        let sid = Uuid::new_v4();
        let input = serde_json::json!({"command": "python script.py"});
        let prompt = mgr
            .classify_and_check(sid, "Bash", &input)
            .expect("classification must not error")
            .expect("bash must prompt in suggest mode");
        let verdict = mgr.classifier.classify("Bash", &input);
        assert_eq!(
            prompt.reason,
            PermissionManager::classifier_reason(&verdict)
        );
    }

    #[test]
    fn test_llm_reason_attribution_follows_consultation() {
        use crate::llm_classifier::{LlmClassificationResult, LlmTier};
        use crate::permission_classifier::{
            ClassificationResult, RiskLevel as ClassifierRisk, RuleDecision,
        };

        let verdict = ClassificationResult {
            decision: RuleDecision::Ask,
            confidence: 0.42,
            reason: "uncertain".to_string(),
            matched_rule: Some("dangerous_pattern".to_string()),
            risk_level: ClassifierRisk::Medium,
        };

        // LLM consulted → its confidence wins, source becomes Llm.
        let consulted = LlmClassificationResult {
            result: ClassificationResult {
                confidence: 0.91,
                ..verdict.clone()
            },
            tier: LlmTier::Allow,
            llm_consulted: true,
        };
        let reason = PermissionManager::llm_reason(&consulted);
        assert_eq!(reason.source, ReasonSource::Llm);
        assert!((reason.confidence.unwrap() - 0.91).abs() < 1e-6);

        // Not consulted → attributed to the underlying rule verdict.
        let fallback = LlmClassificationResult {
            result: verdict,
            tier: LlmTier::Allow,
            llm_consulted: false,
        };
        let reason = PermissionManager::llm_reason(&fallback);
        assert_eq!(reason.source, ReasonSource::Rule);
        assert_eq!(reason.rule_name.as_deref(), Some("dangerous_pattern"));
        assert!((reason.confidence.unwrap() - 0.42).abs() < 1e-6);
    }

    #[test]
    fn test_permission_rule_source_local_serialization() {
        let source = PermissionRuleSource::Local;
        let json = serde_json::to_string(&source).unwrap();
        let parsed: PermissionRuleSource = serde_json::from_str(&json).unwrap();
        assert_eq!(source, parsed);
    }

    // ── LLM classifier wiring tests ──────────────────────────────────────

    #[test]
    fn test_permission_manager_no_llm_by_default() {
        let mgr = PermissionManager::new();
        assert!(!mgr.has_llm_classifier());
    }

    #[tokio::test]
    async fn test_classify_and_check_with_llm_falls_back_without_llm() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        // Should behave identically to classify_and_check when no LLM configured
        let sync_result =
            mgr.classify_and_check(sid, "Bash", &serde_json::json!({"command": "git status"}));
        let async_result = mgr
            .classify_and_check_with_llm(sid, "Bash", &serde_json::json!({"command": "git status"}))
            .await;
        assert_eq!(sync_result.is_ok(), async_result.is_ok());
    }

    #[tokio::test]
    async fn test_classify_and_check_with_llm_denies_critical() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let result = mgr
            .classify_and_check_with_llm(sid, "Bash", &serde_json::json!({"command": "rm -rf /"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_classify_and_check_with_llm_suggest_mode_prompts() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Ask);
        let sid = Uuid::new_v4();
        // Suggest mode should prompt (not auto-approve) for non-read tools
        let result = mgr
            .classify_and_check_with_llm(sid, "Bash", &serde_json::json!({"command": "ls"}))
            .await;
        // In Suggest mode, bash commands should prompt
        assert!(result.is_ok());
    }

    // ── Error boundary tests ──────────────────────────────────────────────

    #[test]
    fn test_error_boundary_rule_checker_empty_rules_returns_no_match() {
        let checker = PermissionRuleChecker::from_rule_strings(&[], &[], &[]);
        assert!(checker.is_empty());
        let decision = checker.check("Bash", "ls -la");
        assert_eq!(decision, RuleCheckDecision::NoMatch);
    }

    #[test]
    fn test_error_boundary_rule_checker_invalid_glob_no_panic() {
        // Invalid glob patterns should be logged but not cause a panic.
        // The globset crate rejects patterns like "[" (unclosed bracket).
        let deny = vec!["[".to_string()];
        let checker = PermissionRuleChecker::from_rule_strings(&deny, &[], &[]);
        // Should not panic; invalid glob is skipped
        let decision = checker.check("[", "anything");
        // The raw pattern "[" doesn't match tool name "[" in the structured
        // pattern check, and the globset silently ignores it, so NoMatch.
        assert!(
            matches!(
                decision,
                RuleCheckDecision::NoMatch | RuleCheckDecision::Denied
            ),
            "Should not panic on invalid glob"
        );
    }

    #[test]
    fn test_error_boundary_ungranted_permission_returns_denied() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let perm = Permission::new("file", "delete", PermissionLevel::Admin);
        let result = mgr.check_permission(sid, &perm);
        assert!(result.is_err(), "Should deny ungranted permission");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Permission denied"),
            "Error should say Permission denied, got: {err}"
        );
    }

    #[test]
    fn test_error_boundary_destructive_tool_flags_prompt() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::AutoEdit);
        mgr.register_destructive_tool("DangerousTool".to_string());
        assert!(mgr.is_tool_destructive("DangerousTool"));
        // Even in AutoEdit mode, destructive tools should generate a prompt
        // (not auto-approve). The prompt should have is_destructive = true.
        let sid = Uuid::new_v4();
        let prompt = mgr.create_permission_prompt(
            "DangerousTool",
            &serde_json::json!({"action": "nuke"}),
            sid,
        );
        assert!(prompt.is_some(), "Destructive tool should require a prompt");
        assert!(prompt.unwrap().is_destructive);
    }

    #[test]
    fn test_error_boundary_deny_rule_wins_over_identical_allow() {
        // Deny and allow on exact same pattern: deny must win
        let deny = vec!["Bash(rm -rf /)".to_string()];
        let allow = vec!["Bash(rm -rf /)".to_string()];
        let checker = PermissionRuleChecker::from_rule_strings(&deny, &[], &allow);
        assert_eq!(
            checker.check("Bash", "rm -rf /"),
            RuleCheckDecision::Denied,
            "Deny should win over identical allow pattern"
        );
    }

    // ---- review §P1-1: persist_allow_rule round-trip + session-scoped choice ----

    #[test]
    fn persist_allow_rule_bash_format_is_matcher_compatible() {
        // Before review §P1-1: format was "Bash(<head>:*)" — literal colon
        // meant the matcher never matched real commands. After: "Bash(<head> *)".
        // Build a checker the way the runtime does (allow rules from
        // .shannon/settings.local.json), and verify it matches the same
        // command prefix the user approved.
        let head = "git push origin";
        let allow = vec![format!("Bash({head} *)")];
        let checker = PermissionRuleChecker::from_rule_strings(&[], &[], &allow);

        // The exact approved form must match.
        assert_eq!(
            checker.check("Bash", "git push origin main"),
            RuleCheckDecision::Allowed,
            "Bash rule 'Bash(git push origin *)' must allow 'git push origin main'"
        );
        // A different prefix must NOT match.
        assert_eq!(
            checker.check("Bash", "rm -rf /"),
            RuleCheckDecision::NoMatch,
            "Bash rule must not auto-allow unrelated commands"
        );
        // A subset prefix must not match either (we approved `git push`, not `git`).
        assert_eq!(
            checker.check("Bash", "git checkout -- ."),
            RuleCheckDecision::NoMatch,
            "Bash rule must not allow commands under the same tool"
        );
    }

    #[test]
    fn remember_choice_always_allow_is_session_scoped() {
        // After §P1-1: AlwaysAllow no longer pollutes the process-wide
        // always_allowed set. Approving Bash in session A must NOT
        // auto-approve unrelated commands in session B.
        let mut mem = PermissionMemory::new();
        let sid_a = uuid::Uuid::new_v4();
        let sid_b = uuid::Uuid::new_v4();

        mem.remember_choice(sid_a, "Bash".to_string(), PermissionChoice::AlwaysAllow);

        // Session A: yes (per-session choice).
        assert!(mem.is_always_allowed(sid_a, "Bash"));
        // Session B: must NOT auto-approve, because the user clicked Always
        // for a specific Bash call in session A, not globally for all Bash.
        assert!(
            !mem.is_always_allowed(sid_b, "Bash"),
            "AlwaysAllow in session A must not propagate to session B"
        );
        // And the process-wide always_allowed set is empty.
        assert!(mem.always_allowed_tools().is_empty());
    }
}
