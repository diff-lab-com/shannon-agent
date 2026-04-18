//! # Permission System
//!
//! Security and permission validation for tool execution and resource access.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

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
        "read" | "read_file" | "search" | "grep" | "glob"
            | "list_directory" | "list_dir" | "ls" | "file_tree" | "file_info"
            | "git_log" | "git_diff" | "git_status" | "git_branch_show"
            | "web_search" | "web_fetch"
            | "lsp_hover" | "lsp_definition" | "lsp_references" | "lsp_diagnostics"
            | "lsp_document_symbols" | "lsp_workspace_symbols"
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
            "bypasspermissions" | "bypass_permissions" | "bypass-permissions" => Some(Self::BypassPermissions),
            "dontask" | "dont_ask" | "dont-ask" => Some(Self::DontAsk),
            "readonly" | "read-only" | "read_only" => Some(Self::Readonly),
            _ => None,
        }
    }

    /// Returns all variant names for display (Claude Code compatible).
    pub fn all_names() -> &'static [&'static str] {
        &["default", "plan", "auto", "full-auto", "bypassPermissions", "dontAsk", "readonly"]
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
    pub fn new(
        tool_name: String,
        default_risk_level: RiskLevel,
        description: String,
    ) -> Self {
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
            input_str.contains(pattern) || {
                if pattern.contains('*') {
                    // Simple wildcard matching
                    let regex_pattern = pattern.replace('*', ".*");
                    input_str.matches(&regex_pattern).count() > 0
                } else {
                    false
                }
            }
        })
    }

    /// Check if the given input requires confirmation
    pub fn requires_confirmation(&self, input_str: &str) -> bool {
        self.confirmation_patterns.iter().any(|pattern| {
            input_str.contains(pattern) || {
                if pattern.contains('*') {
                    let regex_pattern = pattern.replace('*', ".*");
                    input_str.matches(&regex_pattern).count() > 0
                } else {
                    false
                }
            }
        })
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
#[derive(Debug, Clone)]
pub struct PermissionMemory {
    /// Always-allowed tools
    always_allowed: HashSet<String>,
    /// Always-denied tools
    always_denied: HashSet<String>,
    /// Session-specific choices
    session_choices: HashMap<uuid::Uuid, HashMap<String, PermissionChoice>>,
}

impl PermissionMemory {
    /// Create a new permission memory
    pub fn new() -> Self {
        Self {
            always_allowed: HashSet::new(),
            always_denied: HashSet::new(),
            session_choices: HashMap::new(),
        }
    }

    /// Check if a tool is always allowed for this session
    pub fn is_always_allowed(&self, session_id: uuid::Uuid, tool_name: &str) -> bool {
        self.always_allowed.contains(tool_name)
            || self.session_choices
                .get(&session_id)
                .and_then(|choices| choices.get(tool_name))
                .map(|choice| choice == &PermissionChoice::AlwaysAllow)
                .unwrap_or(false)
    }

    /// Check if a tool is always denied
    pub fn is_always_denied(&self, tool_name: &str) -> bool {
        self.always_denied.contains(tool_name)
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
}

impl Default for PermissionMemory {
    fn default() -> Self {
        Self::new()
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

    /// Current approval policy mode
    approval_mode: ApprovalMode,

    /// Sessions with an approved plan (for Plan mode auto-approval).
    plan_approved_sessions: HashSet<uuid::Uuid>,

    /// Tools flagged as destructive via MCP `annotations.destructiveHint`.
    /// These always require user confirmation, even in auto-approve modes.
    destructive_tools: HashSet<String>,
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
            approval_mode: ApprovalMode::default(),
            plan_approved_sessions: HashSet::new(),
            destructive_tools: HashSet::new(),
        };

        // Register default tool policies for common tools
        manager.register_default_policies();

        manager
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
        self.tool_policies.insert("FileEdit".to_string(), edit_policy);

        // FileWrite tool - medium risk
        let write_policy = ToolPermissionPolicy::new(
            "FileWrite".to_string(),
            RiskLevel::Medium,
            "Write to files".to_string(),
        )
        .add_deny_pattern("/etc/")
        .add_deny_pattern("/usr/bin/")
        .add_deny_pattern("/boot/");
        self.tool_policies.insert("FileWrite".to_string(), write_policy);

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
        self.tool_policies.insert("WebFetch".to_string(), web_policy);
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
                if perm.resource == required.resource && perm.action == required.action
                    && perm.grants(required.level) {
                        return Ok(());
                    }
            }
        }

        // Fall back to default permissions
        for perm in &self.default_permissions {
            if perm.resource == required.resource && perm.action == required.action
                && perm.grants(required.level) {
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
    pub fn get_session_permissions(
        &self,
        session_id: uuid::Uuid,
    ) -> HashSet<Permission> {
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
        self.approval_mode = mode;
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
            });
        }

        // Get tool policy
        let policy = self.tool_policies.get(tool_name);

        // Determine risk level and description
        let (risk_level, description) = if let Some(policy) = policy {
            let input_str = serde_json::to_string(tool_input).unwrap_or_default();
            (
                policy.risk_level_for(&input_str),
                format!("{}: {}", policy.description, Self::format_input_summary(tool_input)),
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
            PermissionChoice::Deny => {
                Err(PermissionError::Denied(format!(
                    "User denied: {}",
                    prompt.description
                )))
            }
            PermissionChoice::AllowOnce | PermissionChoice::AlwaysAllow => {
                // Remember the choice
                self.memory.remember_choice(session_id, prompt.tool_name.clone(), choice);
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
                format!("{}...", &s[..47])
            } else {
                s
            }
        }
    }

    /// Helper to truncate a JSON value for display
    fn truncate_value(value: &serde_json::Value) -> String {
        let s = serde_json::to_string(value).unwrap_or_else(|_| "?".to_string());
        if s.len() > 30 {
            format!("{}...", &s[..27])
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
        // --- Approval mode overrides ---
        match self.approval_mode {
            // BypassPermissions / DontAsk: skip all permission checks
            ApprovalMode::BypassPermissions | ApprovalMode::DontAsk => {
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
                return self.create_permission_prompt_with_risk(
                    tool_name,
                    tool_input,
                    session_id,
                    risk,
                );
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
            let desc = format!("{}: {}", p.description, Self::format_input_summary(tool_input));
            if is_destructive { format!("[DESTRUCTIVE] {desc}") } else { desc }
        } else {
            let desc = format!("Execute tool '{}': {}", tool_name, Self::format_input_summary(tool_input));
            if is_destructive { format!("[DESTRUCTIVE] {desc}") } else { desc }
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
        }))
    }
}

/// Convert classifier RiskLevel to permissions RiskLevel.
fn convert_classifier_risk(
    risk: crate::permission_classifier::RiskLevel,
) -> RiskLevel {
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
        for level in [RiskLevel::Safe, RiskLevel::Low, RiskLevel::Medium, RiskLevel::High, RiskLevel::Critical] {
            let json = serde_json::to_string(&level).unwrap();
            let parsed: RiskLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, parsed);
        }
    }

    #[test]
    fn test_permission_choice_serialization() {
        for choice in [PermissionChoice::Deny, PermissionChoice::AllowOnce, PermissionChoice::AlwaysAllow] {
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
        manager.lock().await.grant_permission(session_id, perm.clone());

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
                assert!(result.is_ok(), "Permission for action_{i} should be granted");
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
        let policy = ToolPermissionPolicy::new(
            "Bash".to_string(),
            RiskLevel::Medium,
            "Shell".to_string(),
        )
        .add_deny_pattern("rm -rf /");

        assert!(policy.is_denied("rm -rf /"));
        assert!(policy.is_denied("sudo rm -rf / --no-preserve-root"));
        assert!(!policy.is_denied("ls -la"));
    }

    #[test]
    fn test_policy_confirmation_pattern_matches() {
        let policy = ToolPermissionPolicy::new(
            "Bash".to_string(),
            RiskLevel::Medium,
            "Shell".to_string(),
        )
        .add_confirmation_pattern("rm -rf");

        assert!(policy.requires_confirmation("rm -rf /home/user/dir"));
        assert!(!policy.requires_confirmation("ls -la"));
    }

    #[test]
    fn test_policy_risk_level_denied_input() {
        let policy = ToolPermissionPolicy::new(
            "Bash".to_string(),
            RiskLevel::Medium,
            "Shell".to_string(),
        )
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
        let prompt = mgr.create_permission_prompt(
            "Bash",
            &serde_json::json!({"command": "ls -la"}),
            sid,
        );
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
        let prompt = mgr.create_permission_prompt(
            "UnknownTool",
            &serde_json::json!({"arg": "val"}),
            sid,
        );
        assert!(prompt.is_some());
        let p = prompt.unwrap();
        assert_eq!(p.tool_name, "UnknownTool");
        assert_eq!(p.risk_level, RiskLevel::Medium);
    }

    #[test]
    fn test_create_prompt_dangerous_input_elevated_risk() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = mgr.create_permission_prompt(
            "Bash",
            &serde_json::json!({"command": "rm -rf /"}),
            sid,
        );
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
        assert!(mgr.process_permission_choice(sid, &prompt, PermissionChoice::AllowOnce).is_ok());
        // AllowOnce does NOT make it always allowed
        let next_prompt = mgr.create_permission_prompt(
            "Bash",
            &serde_json::json!({"command": "ls"}),
            sid,
        );
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
        assert!(mgr.process_permission_choice(sid, &prompt, PermissionChoice::AlwaysAllow).is_ok());
        let next_prompt = mgr.create_permission_prompt(
            "Bash",
            &serde_json::json!({"command": "ls"}),
            sid,
        );
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
        let prompt = mgr.create_permission_prompt(
            "Bash",
            &serde_json::json!({"command": "ls"}),
            sid,
        ).unwrap();
        assert_eq!(prompt.risk_level, RiskLevel::Medium);
    }

    #[test]
    fn test_default_write_policy_denies_etc() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = mgr.create_permission_prompt(
            "FileWrite",
            &serde_json::json!({"path": "/etc/passwd"}),
            sid,
        ).unwrap();
        assert_eq!(prompt.risk_level, RiskLevel::Critical);
    }

    #[test]
    fn test_default_read_policy_is_safe() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = mgr.create_permission_prompt(
            "Read",
            &serde_json::json!({"path": "/home/user/file.rs"}),
            sid,
        ).unwrap();
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
        let prompt = mgr.create_permission_prompt(
            "CustomTool",
            &serde_json::json!({"action": "nuclear launch"}),
            sid,
        ).unwrap();
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
        assert_eq!(ApprovalMode::BypassPermissions.to_string(), "bypassPermissions");
        assert_eq!(ApprovalMode::DontAsk.to_string(), "dontAsk");
        assert_eq!(ApprovalMode::Readonly.to_string(), "readonly");
    }

    #[test]
    fn test_approval_mode_from_str() {
        // Claude Code names
        assert_eq!(ApprovalMode::from_str_ci("default"), Some(ApprovalMode::Suggest));
        assert_eq!(ApprovalMode::from_str_ci("plan"), Some(ApprovalMode::Plan));
        assert_eq!(ApprovalMode::from_str_ci("auto"), Some(ApprovalMode::AutoEdit));
        assert_eq!(ApprovalMode::from_str_ci("bypassPermissions"), Some(ApprovalMode::BypassPermissions));
        assert_eq!(ApprovalMode::from_str_ci("dontAsk"), Some(ApprovalMode::DontAsk));
        // Shannon aliases
        assert_eq!(ApprovalMode::from_str_ci("suggest"), Some(ApprovalMode::Suggest));
        assert_eq!(ApprovalMode::from_str_ci("ask"), Some(ApprovalMode::Suggest));
        assert_eq!(ApprovalMode::from_str_ci("auto-edit"), Some(ApprovalMode::AutoEdit));
        assert_eq!(ApprovalMode::from_str_ci("full-auto"), Some(ApprovalMode::FullAuto));
        assert_eq!(ApprovalMode::from_str_ci("readonly"), Some(ApprovalMode::Readonly));
        // Case insensitive
        assert_eq!(ApprovalMode::from_str_ci("DEFAULT"), Some(ApprovalMode::Suggest));
        assert_eq!(ApprovalMode::from_str_ci("PLAN"), Some(ApprovalMode::Plan));
        assert_eq!(ApprovalMode::from_str_ci("AUTO"), Some(ApprovalMode::AutoEdit));
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
        let result = mgr.classify_and_check(
            sid,
            "Bash",
            &serde_json::json!({"command": "rm -rf /"}),
        );
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
        let result = mgr.classify_and_check(
            sid,
            "SomeTool",
            &serde_json::json!({"arg": "val"}),
        );
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
        let result = mgr.classify_and_check(
            sid,
            "Bash",
            &serde_json::json!({"command": "cargo build"}),
        );
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
        let result = mgr.classify_and_check(
            sid,
            "SomeTool",
            &serde_json::json!({}),
        );
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

        let result = mgr.classify_and_check(
            sid,
            "Read",
            &serde_json::json!({"path": "/tmp/test"}),
        );
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // auto-allowed via memory
    }
}
