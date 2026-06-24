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

    #[error("Invalid permission: {0}")]
    InvalidPermission(String),

    #[error("Permission not found: {0}")]
    NotFound(String),
}

/// Returns true if the tool name corresponds to a read-only operation (no side effects).
/// Used by both `Readonly` mode enforcement and `Suggest` mode auto-approval.
fn is_read_only_tool_name(tool_name: &str) -> bool {
    matches!(
        tool_name,
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
/// Compatible with Claude Code's permission modes:
/// - `default`  → [`Suggest`]:            ask for each new tool use
/// - `plan`     → [`Plan`]:               plan first, ask before execution
/// - `auto`     → [`AutoEdit`]:           auto-accept file ops, ask for bash
/// - `bypassPermissions` → [`BypassPermissions`]: skip all checks
/// - `dontAsk`  → [`DontAsk`]:            accept everything without prompting
///
/// Shannon extensions:
/// - `full-auto` → [`FullAuto`]:  auto-approve everything except critical
/// - `readonly`  → [`Readonly`]:  only allow read operations
/// - `auto`      → [`Auto`]:      background safety classifier auto-approves low-risk operations
/// - `plan-ro`   → [`PlanReadonly`]: read-only analysis mode, no tool execution allowed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ApprovalMode {
    /// Ask for confirmation on every tool execution.
    /// Claude Code alias: "default"
    Suggest,
    /// Plan mode: AI proposes a plan first, asks for approval before executing.
    /// Once the plan is approved, individual tool calls are auto-approved.
    /// Claude Code alias: "plan"
    Plan,
    /// Auto-approve file operations (edit, write); ask for bash and other risky tools.
    /// Claude Code alias: "auto"
    #[default]
    AutoEdit,
    /// Auto-approve everything except critical-risk operations.
    FullAuto,
    /// Skip all permission checks entirely. Use with extreme caution.
    /// Claude Code alias: "bypassPermissions"
    BypassPermissions,
    /// Accept everything without prompting, including critical operations.
    /// Claude Code alias: "dontAsk"
    DontAsk,
    /// Only allow read operations — no writes, no bash.
    Readonly,
    /// Background safety classifier auto-approves low-risk operations, asks for high-risk.
    /// Safe/Low risk: auto-approve, Medium+ risk: prompt, Critical: deny.
    Auto,
    /// Read-only analysis mode - no tool execution allowed, only read operations.
    /// Denies all tool execution except Read/Grep/Glob/List operations.
    PlanReadonly,
}

impl std::fmt::Display for ApprovalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Suggest => write!(f, "default"),
            Self::Plan => write!(f, "plan"),
            Self::AutoEdit => write!(f, "auto"),
            Self::FullAuto => write!(f, "full-auto"),
            Self::BypassPermissions => write!(f, "bypassPermissions"),
            Self::DontAsk => write!(f, "dontAsk"),
            Self::Readonly => write!(f, "readonly"),
            Self::Auto => write!(f, "auto-classifier"),
            Self::PlanReadonly => write!(f, "plan-readonly"),
        }
    }
}

impl ApprovalMode {
    /// Parse from string (case-insensitive). Accepts both Shannon and Claude Code names.
    pub fn from_str_ci(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "suggest" | "ask" | "default" => Some(Self::Suggest),
            "plan" => Some(Self::Plan),
            "auto-edit" | "auto_edit" | "auto" | "acceptedits" => Some(Self::AutoEdit),
            "full-auto" | "full_auto" | "fullauto" => Some(Self::FullAuto),
            "bypasspermissions" | "bypass_permissions" | "bypass-permissions" => {
                Some(Self::BypassPermissions)
            }
            "dontask" | "dont_ask" | "dont-ask" => Some(Self::DontAsk),
            "readonly" | "read-only" | "read_only" => Some(Self::Readonly),
            "auto-classifier" | "auto_classifier" | "classifier" => Some(Self::Auto),
            "plan-readonly" | "plan_readonly" | "plan_ro" => Some(Self::PlanReadonly),
            _ => None,
        }
    }

    /// Returns all variant names for display (Claude Code compatible).
    pub fn all_names() -> &'static [&'static str] {
        &[
            "default",
            "plan",
            "auto",
            "full-auto",
            "bypassPermissions",
            "dontAsk",
            "readonly",
            "auto-classifier",
            "plan-readonly",
        ]
    }

    /// Cycle to the next commonly-used mode (Shift+Tab pattern).
    /// Cycles through: Suggest → AutoEdit → Plan → FullAuto → Suggest
    /// BypassPermissions/DontAsk/Readonly/Auto/PlanReadonly are set explicitly via /mode.
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Suggest => Self::AutoEdit,
            Self::AutoEdit => Self::Plan,
            Self::Plan => Self::FullAuto,
            Self::FullAuto => Self::Suggest,
            // All other modes cycle back to Suggest (start of cycle)
            Self::Auto
            | Self::Readonly
            | Self::PlanReadonly
            | Self::BypassPermissions
            | Self::DontAsk => Self::Suggest,
        }
    }

    /// Short label for display in the status bar (max ~10 chars).
    pub fn short_label(&self) -> &'static str {
        match self {
            Self::Suggest => "ASK",
            Self::AutoEdit => "EDIT",
            Self::Plan => "PLAN",
            Self::FullAuto => "AUTO",
            Self::BypassPermissions => "FULL",
            Self::DontAsk => "FULL",
            Self::Readonly => "ASK",
            Self::Auto => "AUTO",
            Self::PlanReadonly => "PLAN",
        }
    }

    /// Reverse-lookup from short_label string to the primary ApprovalMode variant.
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "ASK" => Some(Self::Suggest),
            "EDIT" => Some(Self::AutoEdit),
            "PLAN" => Some(Self::Plan),
            "AUTO" => Some(Self::FullAuto),
            "FULL" => Some(Self::BypassPermissions),
            _ => None,
        }
    }

    /// Description of this mode for help text.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Suggest => "Auto-approve read-only tools; ask for write/bash operations",
            Self::Plan => "Plan first, then auto-approve within the approved plan",
            Self::AutoEdit => "Auto-approve file edits; ask for bash/other risky tools",
            Self::FullAuto => "Auto-approve everything except critical operations",
            Self::BypassPermissions => "Skip all permission checks (dangerous, trusted env only)",
            Self::DontAsk => "Accept everything without prompting",
            Self::Readonly => "Only allow read operations — no writes, no bash",
            Self::Auto => "Auto-approve Safe/Low risk; prompt Medium+; deny Critical",
            Self::PlanReadonly => {
                "Read-only analysis: deny all tool execution except Read/Grep/Glob/List"
            }
        }
    }

    /// Whether a tool should be auto-approved under this mode.
    pub fn should_auto_approve(&self, tool_name: &str, risk_level: RiskLevel) -> bool {
        match self {
            Self::Suggest => {
                // Auto-approve read-only tools at Low/Safe risk (matching Claude Code behavior)
                is_read_only_tool_name(tool_name) && risk_level <= RiskLevel::Low
            }
            Self::Plan => false,
            Self::AutoEdit => {
                // Auto-approve file operations; ask for everything else
                let is_file_tool = matches!(
                    tool_name,
                    "edit" | "write" | "create_file" | "replace" | "file_edit"
                );
                is_file_tool && risk_level <= RiskLevel::Medium
            }
            Self::FullAuto => risk_level < RiskLevel::Critical,
            Self::BypassPermissions | Self::DontAsk => true,
            Self::Readonly => false, // handled at a higher level
            Self::Auto => {
                // Auto-approve Safe and Low risk; prompt for Medium and High; deny Critical
                risk_level <= RiskLevel::Low
            }
            Self::PlanReadonly => false, // handled at a higher level (deny all except read operations)
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
    /// [`PermissionRuleCheckerExt::from_rules`] extension trait method which
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
        // 1. Deny has highest priority
        if self.matches_pattern(tool_name, command, &self.deny_raw, &self.deny_globset) {
            return RuleCheckDecision::Denied;
        }
        // 2. Ask is next
        if self.matches_pattern(tool_name, command, &self.ask_raw, &self.ask_globset) {
            return RuleCheckDecision::Ask;
        }
        // 3. Allow
        if self.matches_pattern(tool_name, command, &self.allow_raw, &self.allow_globset) {
            return RuleCheckDecision::Allowed;
        }
        // 4. No rule matched
        RuleCheckDecision::NoMatch
    }

    /// Check if the raw rules are all empty (nothing to check).
    pub fn is_empty(&self) -> bool {
        self.deny_raw.is_empty() && self.ask_raw.is_empty() && self.allow_raw.is_empty()
    }

    /// Test a tool/command against a set of patterns.
    fn matches_pattern(
        &self,
        tool_name: &str,
        command: &str,
        raw_patterns: &[String],
        globset: &Option<GlobSet>,
    ) -> bool {
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
                    return true;
                }
            }
            // Bare tool name or glob-only pattern
            else if pattern.eq_ignore_ascii_case(tool_name) || pattern == "*" {
                return true;
            }
        }

        // Then check globset for plain glob patterns (e.g., "mcp__server__*")
        if let Some(gs) = globset {
            if gs.is_match(tool_name) {
                return true;
            }
        }

        false
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

    /// Remember a user's permission choice
    pub fn remember_choice(
        &mut self,
        session_id: uuid::Uuid,
        tool_name: String,
        choice: PermissionChoice,
    ) {
        match choice {
            PermissionChoice::AlwaysAllow => {
                self.always_allowed.insert(tool_name.clone());
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

    /// Rule checker for deny > ask > allow priority from settings.
    rule_checker: PermissionRuleChecker,

    /// Active permission profile (strict / balanced / permissive / custom).
    active_profile: Option<PermissionProfile>,
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
            rule_checker: PermissionRuleChecker::default(),
            active_profile: None,
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

    /// Set the approval mode.
    pub fn set_approval_mode(&mut self, mode: ApprovalMode) {
        tracing::info!(old = ?self.approval_mode, new = ?mode, "Approval mode changed");
        self.approval_mode = mode;
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
            ApprovalMode::Suggest
        } else {
            // Fallback: treat as Suggest
            ApprovalMode::Suggest
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
            ApprovalMode::Suggest
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
        })
    }

    /// Process user's permission choice
    pub fn process_permission_choice(
        &mut self,
        session_id: uuid::Uuid,
        prompt: &PermissionPrompt,
        choice: PermissionChoice,
    ) -> Result<(), PermissionError> {
        match choice {
            PermissionChoice::Deny => Err(PermissionError::Denied(format!(
                "User denied: {}",
                prompt.description
            ))),
            PermissionChoice::AllowOnce | PermissionChoice::AlwaysAllow => {
                // Remember the choice
                self.memory
                    .remember_choice(session_id, prompt.tool_name.clone(), choice);
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
        // --- Permission rule checker (deny > ask > allow from settings) ---
        if !self.rule_checker.is_empty() {
            let command = tool_input
                .get("command")
                .or_else(|| tool_input.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match self.rule_checker.check(tool_name, command) {
                RuleCheckDecision::Denied => {
                    return Err(PermissionError::Denied(format!(
                        "Denied by permission rule: {tool_name}"
                    )));
                }
                RuleCheckDecision::Ask => {
                    return self.create_permission_prompt_with_risk(
                        tool_name,
                        tool_input,
                        session_id,
                        RiskLevel::Medium,
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
        match self.approval_mode {
            // BypassPermissions / DontAsk: skip all permission checks
            ApprovalMode::BypassPermissions | ApprovalMode::DontAsk => {
                tracing::debug!(mode = ?self.approval_mode, tool = %tool_name, "Permission check bypassed");
                return Ok(None);
            }
            ApprovalMode::Readonly => {
                // Only allow read-only tools
                if is_read_only_tool_name(tool_name) {
                    return Ok(None);
                }
                return Err(PermissionError::Denied(format!(
                    "Readonly mode: {tool_name} is not a read operation"
                )));
            }
            ApprovalMode::PlanReadonly => {
                // Only allow read-only tools (Read, Grep, Glob, List operations)
                // Deny all tool execution - this is analysis-only mode
                if is_read_only_tool_name(tool_name) {
                    return Ok(None);
                }
                return Err(PermissionError::Denied(format!(
                    "PlanReadonly mode: tool execution not allowed (read-only analysis mode): {tool_name}"
                )));
            }
            ApprovalMode::Auto => {
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

                // Auto mode: auto-approve Safe/Low, prompt Medium+, deny Critical
                if risk <= RiskLevel::Low {
                    return Ok(None);
                } else if risk >= RiskLevel::Critical {
                    return Err(PermissionError::Denied(format!(
                        "Auto mode: critical-risk operations denied: {tool_name} (risk: {risk:?})"
                    )));
                }

                // Medium or High risk: prompt user
                return self
                    .create_permission_prompt_with_risk(tool_name, tool_input, session_id, risk);
            }
            ApprovalMode::Plan => {
                // If plan is approved for this session, auto-approve all tools
                if self.plan_approved_sessions.contains(&session_id) {
                    return Ok(None);
                }
                // Otherwise behave like Suggest — prompt unless always-allowed
                if self.memory.is_always_allowed(session_id, tool_name) {
                    return Ok(None);
                }
                // Destructive tools always require confirmation
                if self.is_tool_destructive(tool_name) {
                    return Ok(self.create_permission_prompt(tool_name, tool_input, session_id));
                }
                // Fall through to classifier for risk level and prompt creation
            }
            ApprovalMode::Suggest => {
                // Always-allowed in memory → auto-approve
                if self.memory.is_always_allowed(session_id, tool_name) {
                    return Ok(None);
                }
                // Auto-approve read-only tools (matching Claude Code behavior:
                // Read, Glob, Grep, etc. don't need confirmation)
                if is_read_only_tool_name(tool_name) {
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

                // If the mode says auto-approve, do it
                if self.approval_mode.should_auto_approve(tool_name, risk) {
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
                return self
                    .create_permission_prompt_with_risk(tool_name, tool_input, session_id, risk);
            }
        }

        // --- Default classifier logic (Suggest / Plan mode path) ---
        let result = self.classifier.classify(tool_name, tool_input);

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
                )
            }
            crate::permission_classifier::RuleDecision::Ask => {
                // Always prompt the user
                self.create_permission_prompt_with_risk(
                    tool_name,
                    tool_input,
                    session_id,
                    convert_classifier_risk(result.risk_level),
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

        // Only the Auto mode benefits from LLM classification; other modes
        // have deterministic rules that don't need LLM judgment.
        if !matches!(
            self.approval_mode,
            ApprovalMode::Auto | ApprovalMode::AutoEdit | ApprovalMode::FullAuto
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
            match self.rule_checker.check(tool_name, command) {
                RuleCheckDecision::Denied => {
                    return Err(PermissionError::Denied(format!(
                        "Denied by permission rule: {tool_name}"
                    )));
                }
                RuleCheckDecision::Ask => {
                    return self.create_permission_prompt_with_risk(
                        tool_name,
                        tool_input,
                        session_id,
                        RiskLevel::Medium,
                    );
                }
                RuleCheckDecision::Allowed => return Ok(None),
                RuleCheckDecision::NoMatch => {}
            }
        }

        // Bypass modes
        if matches!(
            self.approval_mode,
            ApprovalMode::BypassPermissions | ApprovalMode::DontAsk
        ) {
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
                    self.create_permission_prompt_with_risk(tool_name, tool_input, session_id, risk)
                }
            }
            crate::permission_classifier::RuleDecision::Ask => {
                self.create_permission_prompt_with_risk(tool_name, tool_input, session_id, risk)
            }
        }
    }

    /// Create a permission prompt with an explicit risk level from classifier.
    fn create_permission_prompt_with_risk(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        session_id: uuid::Uuid,
        risk_level: RiskLevel,
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
        }))
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
        // always_allowed is global, not session-scoped, so still true
        assert!(mem.is_always_allowed(sid, "Bash"));
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
        assert_eq!(ApprovalMode::Suggest.to_string(), "default");
        assert_eq!(ApprovalMode::Plan.to_string(), "plan");
        assert_eq!(ApprovalMode::AutoEdit.to_string(), "auto");
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
        // Claude Code names
        assert_eq!(
            ApprovalMode::from_str_ci("default"),
            Some(ApprovalMode::Suggest)
        );
        assert_eq!(ApprovalMode::from_str_ci("plan"), Some(ApprovalMode::Plan));
        assert_eq!(
            ApprovalMode::from_str_ci("auto"),
            Some(ApprovalMode::AutoEdit)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("bypassPermissions"),
            Some(ApprovalMode::BypassPermissions)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("dontAsk"),
            Some(ApprovalMode::DontAsk)
        );
        // Shannon aliases
        assert_eq!(
            ApprovalMode::from_str_ci("suggest"),
            Some(ApprovalMode::Suggest)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("ask"),
            Some(ApprovalMode::Suggest)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("auto-edit"),
            Some(ApprovalMode::AutoEdit)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("full-auto"),
            Some(ApprovalMode::FullAuto)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("readonly"),
            Some(ApprovalMode::Readonly)
        );
        // Case insensitive
        assert_eq!(
            ApprovalMode::from_str_ci("DEFAULT"),
            Some(ApprovalMode::Suggest)
        );
        assert_eq!(ApprovalMode::from_str_ci("PLAN"), Some(ApprovalMode::Plan));
        assert_eq!(
            ApprovalMode::from_str_ci("AUTO"),
            Some(ApprovalMode::AutoEdit)
        );
        // Invalid
        assert_eq!(ApprovalMode::from_str_ci("invalid"), None);
    }

    #[test]
    fn test_approval_mode_all_names() {
        let names = ApprovalMode::all_names();
        assert!(names.contains(&"default"));
        assert!(names.contains(&"plan"));
        assert!(names.contains(&"auto"));
        assert!(names.contains(&"bypassPermissions"));
        assert!(names.contains(&"dontAsk"));
        assert!(names.contains(&"readonly"));
    }

    #[test]
    fn test_approval_mode_auto_approve_suggest() {
        let mode = ApprovalMode::Suggest;
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
    fn test_dont_ask_accepts_everything() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::DontAsk);
        let sid = Uuid::new_v4();

        let result = mgr.classify_and_check(
            sid,
            "Bash",
            &serde_json::json!({"command": "anything dangerous"}),
        );
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

    // ── New permission modes tests ─────────────────────────────────────

    #[test]
    fn test_auto_mode_auto_approves_low_risk() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Auto);
        let sid = Uuid::new_v4();

        // Safe/Low risk operations should be auto-approved
        // Read tool has Safe risk in policy
        let result = mgr.classify_and_check(sid, "Read", &serde_json::json!({"path": "/tmp/test"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // auto-approved
    }

    #[test]
    fn test_auto_mode_prompts_medium_risk() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Auto);
        let sid = Uuid::new_v4();

        // Medium risk operations should prompt
        // FileWrite has Medium risk in policy
        let result =
            mgr.classify_and_check(sid, "FileWrite", &serde_json::json!({"path": "/tmp/test"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_some()); // prompt needed
    }

    #[test]
    fn test_auto_mode_denies_critical_risk() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::Auto);
        let sid = Uuid::new_v4();

        // Critical risk operations should be denied
        let result =
            mgr.classify_and_check(sid, "Bash", &serde_json::json!({"command": "rm -rf /"}));
        assert!(result.is_err()); // denied
    }

    #[test]
    fn test_plan_readonly_mode_allows_read_tools() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::PlanReadonly);
        let sid = Uuid::new_v4();

        // Read tools should be allowed
        let result = mgr.classify_and_check(sid, "read", &serde_json::json!({"path": "/tmp/test"}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // auto-allowed
    }

    #[test]
    fn test_plan_readonly_mode_denies_write_tools() {
        let mut mgr = PermissionManager::new();
        mgr.set_approval_mode(ApprovalMode::PlanReadonly);
        let sid = Uuid::new_v4();

        // Write tools should be denied
        let result = mgr.classify_and_check(sid, "bash", &serde_json::json!({"command": "ls"}));
        assert!(result.is_err()); // denied
    }

    #[test]
    fn test_approval_mode_display_new_modes() {
        assert_eq!(ApprovalMode::Auto.to_string(), "auto-classifier");
        assert_eq!(ApprovalMode::PlanReadonly.to_string(), "plan-readonly");
    }

    #[test]
    fn test_approval_mode_from_str_new_modes() {
        assert_eq!(
            ApprovalMode::from_str_ci("auto-classifier"),
            Some(ApprovalMode::Auto)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("auto_classifier"),
            Some(ApprovalMode::Auto)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("classifier"),
            Some(ApprovalMode::Auto)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("plan-readonly"),
            Some(ApprovalMode::PlanReadonly)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("plan_readonly"),
            Some(ApprovalMode::PlanReadonly)
        );
        assert_eq!(
            ApprovalMode::from_str_ci("plan_ro"),
            Some(ApprovalMode::PlanReadonly)
        );
    }

    #[test]
    fn test_approval_mode_cycle_includes_new_modes() {
        // cycle_next() cycles 4 modes: Suggest → AutoEdit → Plan → FullAuto → Suggest
        let modes = [
            ApprovalMode::Suggest,
            ApprovalMode::AutoEdit,
            ApprovalMode::Plan,
            ApprovalMode::FullAuto,
        ];

        let mut current = ApprovalMode::Suggest;
        for expected in &modes[1..] {
            current = current.cycle_next();
            assert_eq!(current, *expected);
        }

        // After FullAuto, should cycle back to Suggest
        current = current.cycle_next();
        assert_eq!(current, ApprovalMode::Suggest);
    }

    #[test]
    fn test_approval_mode_short_label_new_modes() {
        assert_eq!(ApprovalMode::Auto.short_label(), "AUTO");
        assert_eq!(ApprovalMode::PlanReadonly.short_label(), "PLAN");
    }

    #[test]
    fn test_approval_mode_description_new_modes() {
        let auto_desc = ApprovalMode::Auto.description();
        assert!(auto_desc.contains("Auto-approve Safe/Low"));

        let plan_ro_desc = ApprovalMode::PlanReadonly.description();
        assert!(
            plan_ro_desc.contains("Read-only analysis")
                || plan_ro_desc.contains("read-only analysis")
        );
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
        mgr.set_approval_mode(ApprovalMode::Suggest);
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
}
