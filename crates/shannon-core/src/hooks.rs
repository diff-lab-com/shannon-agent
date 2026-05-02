//! # Hooks System
//!
//! A hook system that allows executing shell commands at various lifecycle
//! points, similar to Claude Code's hook mechanism.
//!
//! ## Hook Events
//!
//! Hooks can be triggered on these events:
//! - [`HookEvent::PreToolUse`]: Before a tool is executed
//! - [`HookEvent::PostToolUse`]: After a tool completes successfully
//! - [`HookEvent::PostToolUseFailure`]: After a tool fails
//! - [`HookEvent::SessionStart`]: When a session begins
//! - [`HookEvent::SessionEnd`]: When a session ends
//! - [`HookEvent::Notification`]: When a notification is emitted
//! - [`HookEvent::UserPromptSubmit`]: When the user submits a prompt
//! - [`HookEvent::Stop`]: When the model stops generating
//! - [`HookEvent::StopFailure`]: When the model stops due to an error
//! - [`HookEvent::PreCompact`]: Before context compaction
//! - [`HookEvent::PostCompact`]: After context compaction completes
//! - [`HookEvent::SubagentStart`]: When a subagent is spawned
//! - [`HookEvent::SubagentStop`]: When a subagent finishes
//! - [`HookEvent::PermissionRequest`]: When a permission is requested
//! - [`HookEvent::PermissionDenied`]: When a permission is denied
//! - [`HookEvent::FileChanged`]: When a file is modified on disk
//! - [`HookEvent::CwdChanged`]: When the working directory changes
//!
//! ## Hook Types
//!
//! Each hook definition supports a `type` field controlling execution:
//! - `command` (default): Shell command via stdin/stdout protocol
//! - `http`: POST JSON to a URL
//! - `llm`: LLM-based evaluation with prompt template substitution
//! - `prompt`: Single-turn LLM evaluation
//!
//! ## Configuration
//!
//! Hooks are loaded from multiple locations (later files override earlier ones).
//! Claude Code's `settings.json` format is fully compatible — serde ignores
//! non-hook fields like `mcpServers`.
//!
//! **User-level** (lower priority):
//! - `~/.claude/settings.json`
//! - `~/.shannon/settings.json`
//! - `~/.shannon/hooks.json`
//!
//! **Project-level** (higher priority):
//! - `.claude/settings.json`
//! - `.claude/settings.local.json`
//! - `.shannon/settings.json`
//! - `.shannon/settings.local.json`
//! - `.shannon/hooks.json`
//!
//! ## Example hooks.json
//!
//! ```json
//! {
//!   "hooks": {
//!     "PreToolUse": [
//!       {
//!         "matcher": "Bash",
//!         "hooks": [
//!           { "command": "echo 'About to run bash'", "timeout": 5, "blocking": false }
//!         ]
//!       }
//!     ]
//!   }
//! }
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

/// Errors that can occur during hook operations
#[derive(Error, Debug)]
pub enum HookError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Hook execution timed out after {timeout_secs}s: {command}")]
    Timeout { command: String, timeout_secs: u64 },

    #[error("Hook command failed with exit code {exit_code}: {command}")]
    CommandFailed {
        command: String,
        exit_code: i32,
        stderr: String,
    },

    #[error("Invalid matcher pattern: {0}")]
    InvalidMatcher(String),

    #[error("Hook denied operation: {reason}")]
    Denied { reason: String },

    #[error("Home directory not found")]
    HomeNotFound,
}

/// The type of hook event being triggered
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookEventType {
    /// Before a tool is executed
    PreToolUse,
    /// After a tool completes
    PostToolUse,
    /// When a session begins
    SessionStart,
    /// When a session ends
    SessionEnd,
    /// When a notification is emitted
    Notification,
    /// When the user submits a prompt
    UserPromptSubmit,
    /// When a team task is created (before committing)
    TeamTaskCreated,
    /// When a team task is marked completed (before committing)
    TeamTaskCompleted,
    /// When a teammate goes idle
    TeammateIdle,
    /// Before context compaction
    PreCompact,
    /// When a subagent is spawned
    SubagentStart,
    /// When a subagent finishes
    SubagentStop,
    /// When a tool permission is denied
    PermissionDenied,
    /// When the model stops generating
    Stop,
    /// After a tool fails with an error
    PostToolUseFailure,
    /// After context compaction completes
    PostCompact,
    /// When the model stops due to an error
    StopFailure,
    /// When a file is modified on disk
    FileChanged,
    /// When the working directory changes
    CwdChanged,
    /// When a permission is requested (before user prompt)
    PermissionRequest,
    /// After user prompt is expanded (template variables resolved)
    UserPromptExpansion,
    /// After a batch of tools completes
    PostToolBatch,
    /// When configuration changes
    ConfigChange,
    /// After CLAUDE.md / instructions are loaded
    InstructionsLoaded,
    /// When a worktree is created
    WorktreeCreate,
    /// When a worktree is removed
    WorktreeRemove,
    /// When an interactive elicitation is triggered
    Elicitation,
    /// When an elicitation result is received
    ElicitationResult,
    /// When a task is created (Claude Code standard name)
    TaskCreated,
    /// When a task is completed (Claude Code standard name)
    TaskCompleted,
}

impl HookEventType {
    /// Parse a string into a HookEventType
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        match s {
            "PreToolUse" => Some(Self::PreToolUse),
            "PostToolUse" => Some(Self::PostToolUse),
            "SessionStart" => Some(Self::SessionStart),
            "SessionEnd" => Some(Self::SessionEnd),
            "Notification" => Some(Self::Notification),
            "UserPromptSubmit" => Some(Self::UserPromptSubmit),
            "TeamTaskCreated" => Some(Self::TeamTaskCreated),
            "TeamTaskCompleted" => Some(Self::TeamTaskCompleted),
            "TeammateIdle" => Some(Self::TeammateIdle),
            "PreCompact" => Some(Self::PreCompact),
            "SubagentStart" => Some(Self::SubagentStart),
            "SubagentStop" => Some(Self::SubagentStop),
            "PermissionDenied" => Some(Self::PermissionDenied),
            "Stop" => Some(Self::Stop),
            "PostToolUseFailure" => Some(Self::PostToolUseFailure),
            "PostCompact" => Some(Self::PostCompact),
            "StopFailure" => Some(Self::StopFailure),
            "FileChanged" => Some(Self::FileChanged),
            "CwdChanged" => Some(Self::CwdChanged),
            "PermissionRequest" => Some(Self::PermissionRequest),
            "UserPromptExpansion" => Some(Self::UserPromptExpansion),
            "PostToolBatch" => Some(Self::PostToolBatch),
            "ConfigChange" => Some(Self::ConfigChange),
            "InstructionsLoaded" => Some(Self::InstructionsLoaded),
            "WorktreeCreate" => Some(Self::WorktreeCreate),
            "WorktreeRemove" => Some(Self::WorktreeRemove),
            "Elicitation" => Some(Self::Elicitation),
            "ElicitationResult" => Some(Self::ElicitationResult),
            "TaskCreated" => Some(Self::TaskCreated),
            "TaskCompleted" => Some(Self::TaskCompleted),
            _ => None,
        }
    }
}

impl std::fmt::Display for HookEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreToolUse => write!(f, "PreToolUse"),
            Self::PostToolUse => write!(f, "PostToolUse"),
            Self::SessionStart => write!(f, "SessionStart"),
            Self::SessionEnd => write!(f, "SessionEnd"),
            Self::Notification => write!(f, "Notification"),
            Self::UserPromptSubmit => write!(f, "UserPromptSubmit"),
            Self::TeamTaskCreated => write!(f, "TeamTaskCreated"),
            Self::TeamTaskCompleted => write!(f, "TeamTaskCompleted"),
            Self::TeammateIdle => write!(f, "TeammateIdle"),
            Self::PreCompact => write!(f, "PreCompact"),
            Self::SubagentStart => write!(f, "SubagentStart"),
            Self::SubagentStop => write!(f, "SubagentStop"),
            Self::PermissionDenied => write!(f, "PermissionDenied"),
            Self::Stop => write!(f, "Stop"),
            Self::PostToolUseFailure => write!(f, "PostToolUseFailure"),
            Self::PostCompact => write!(f, "PostCompact"),
            Self::StopFailure => write!(f, "StopFailure"),
            Self::FileChanged => write!(f, "FileChanged"),
            Self::CwdChanged => write!(f, "CwdChanged"),
            Self::PermissionRequest => write!(f, "PermissionRequest"),
            Self::UserPromptExpansion => write!(f, "UserPromptExpansion"),
            Self::PostToolBatch => write!(f, "PostToolBatch"),
            Self::ConfigChange => write!(f, "ConfigChange"),
            Self::InstructionsLoaded => write!(f, "InstructionsLoaded"),
            Self::WorktreeCreate => write!(f, "WorktreeCreate"),
            Self::WorktreeRemove => write!(f, "WorktreeRemove"),
            Self::Elicitation => write!(f, "Elicitation"),
            Self::ElicitationResult => write!(f, "ElicitationResult"),
            Self::TaskCreated => write!(f, "TaskCreated"),
            Self::TaskCompleted => write!(f, "TaskCompleted"),
        }
    }
}

/// A concrete hook event with its associated data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookEvent {
    /// Before a tool is executed
    PreToolUse {
        /// Name of the tool about to be used
        tool_name: String,
        /// Input/arguments for the tool
        input: Value,
    },
    /// After a tool completes
    PostToolUse {
        /// Name of the tool that was used
        tool_name: String,
        /// Input/arguments for the tool
        input: Value,
        /// Output from the tool
        output: Value,
    },
    /// When a session begins
    SessionStart {
        /// Unique session identifier
        session_id: String,
    },
    /// When a session ends
    SessionEnd {
        /// Unique session identifier
        session_id: String,
    },
    /// When a notification is emitted
    Notification {
        /// Notification message content
        message: String,
    },
    /// When the user submits a prompt
    UserPromptSubmit {
        /// The user's prompt text
        prompt: String,
    },
    /// When a team task is created (before committing).
    /// Exit code 2 from the hook = rollback (delete the task).
    TeamTaskCreated {
        /// The task ID
        task_id: String,
        /// The team name
        team_name: String,
        /// The agent that created the task (if known)
        agent_name: Option<String>,
        /// Brief task subject
        subject: String,
        /// Task priority
        priority: String,
    },
    /// When a team task is marked completed (before committing).
    /// Exit code 2 from the hook = prevent completion (revert to in_progress).
    TeamTaskCompleted {
        /// The task ID
        task_id: String,
        /// The team name
        team_name: String,
        /// The agent that completed the task
        agent_name: String,
        /// Brief task subject
        subject: String,
    },
    /// When a teammate goes idle.
    /// Exit code 2 from the hook = send feedback and keep working.
    TeammateIdle {
        /// The team name
        team_name: String,
        /// The agent that went idle
        agent_name: String,
        /// Number of remaining available tasks
        available_tasks: usize,
    },
    /// Before context compaction
    PreCompact {
        /// Number of messages in the conversation
        messages_count: usize,
        /// Estimated token usage
        estimated_tokens: usize,
    },
    /// When a subagent is spawned
    SubagentStart {
        /// Unique agent identifier
        agent_id: String,
        /// Type of agent (e.g. "Explore", "general-purpose")
        agent_type: String,
    },
    /// When a subagent finishes
    SubagentStop {
        /// Unique agent identifier
        agent_id: String,
        /// Brief summary of the result
        result_summary: String,
    },
    /// When a tool permission is denied
    PermissionDenied {
        /// Name of the tool
        tool_name: String,
        /// Tool input that was denied
        input: Value,
        /// How many times the user has retried
        retry_count: usize,
    },
    /// When the model stops generating
    Stop {
        /// Number of tool calls made in this turn
        tool_calls_count: usize,
        /// Whether the model should continue (exit code 2 = force continue)
        should_continue: bool,
    },
    /// After a tool fails with an error
    PostToolUseFailure {
        /// Name of the tool that failed
        tool_name: String,
        /// Input/arguments for the tool
        input: Value,
        /// Error message from the tool
        error: String,
    },
    /// After context compaction completes
    PostCompact {
        /// Number of messages before compaction
        messages_before: usize,
        /// Number of messages after compaction
        messages_after: usize,
        /// Estimated tokens freed
        tokens_freed: usize,
    },
    /// When the model stops due to an error
    StopFailure {
        /// Error message that caused the stop
        error: String,
    },
    /// When a file is modified on disk
    FileChanged {
        /// Path to the changed file
        path: String,
        /// Type of change (create, modify, delete)
        change_type: String,
    },
    /// When the working directory changes
    CwdChanged {
        /// Previous working directory
        old_cwd: String,
        /// New working directory
        new_cwd: String,
    },
    /// When a permission is requested (before user prompt)
    PermissionRequest {
        /// Name of the tool requesting permission
        tool_name: String,
        /// Description of what the tool will do
        description: String,
    },
    /// After user prompt is expanded (template variables resolved)
    UserPromptExpansion {
        /// The expanded prompt text
        expanded_prompt: String,
        /// The original prompt before expansion
        original_prompt: String,
    },
    /// After a batch of tools completes
    PostToolBatch {
        /// Tool names in the batch
        tool_names: Vec<String>,
        /// Number of successful executions
        success_count: usize,
        /// Number of failed executions
        failure_count: usize,
    },
    /// When configuration changes
    ConfigChange {
        /// Path to the changed config file
        config_path: String,
        /// Type of change (created, modified, deleted)
        change_type: String,
    },
    /// After CLAUDE.md / instructions are loaded
    InstructionsLoaded {
        /// Number of instruction files loaded
        files_count: usize,
        /// Total size in bytes
        total_bytes: usize,
    },
    /// When a worktree is created
    WorktreeCreate {
        /// Path to the new worktree
        path: String,
        /// Branch name for the worktree
        branch: String,
    },
    /// When a worktree is removed
    WorktreeRemove {
        /// Path to the removed worktree
        path: String,
    },
    /// When an interactive elicitation is triggered
    Elicitation {
        /// The question being asked
        question: String,
        /// The requesting tool or component
        source: String,
    },
    /// When an elicitation result is received
    ElicitationResult {
        /// The question that was asked
        question: String,
        /// The user's response
        response: String,
    },
    /// When a task is created (Claude Code standard)
    TaskCreated {
        /// The task ID
        task_id: String,
        /// Brief task description
        subject: String,
        /// Task priority
        priority: String,
    },
    /// When a task is completed (Claude Code standard)
    TaskCompleted {
        /// The task ID
        task_id: String,
        /// Brief task description
        subject: String,
    },
}

impl HookEvent {
    /// Get the event type for this hook event
    pub fn event_type(&self) -> HookEventType {
        match self {
            Self::PreToolUse { .. } => HookEventType::PreToolUse,
            Self::PostToolUse { .. } => HookEventType::PostToolUse,
            Self::SessionStart { .. } => HookEventType::SessionStart,
            Self::SessionEnd { .. } => HookEventType::SessionEnd,
            Self::Notification { .. } => HookEventType::Notification,
            Self::UserPromptSubmit { .. } => HookEventType::UserPromptSubmit,
            Self::TeamTaskCreated { .. } => HookEventType::TeamTaskCreated,
            Self::TeamTaskCompleted { .. } => HookEventType::TeamTaskCompleted,
            Self::TeammateIdle { .. } => HookEventType::TeammateIdle,
            Self::PreCompact { .. } => HookEventType::PreCompact,
            Self::SubagentStart { .. } => HookEventType::SubagentStart,
            Self::SubagentStop { .. } => HookEventType::SubagentStop,
            Self::PermissionDenied { .. } => HookEventType::PermissionDenied,
            Self::Stop { .. } => HookEventType::Stop,
            Self::PostToolUseFailure { .. } => HookEventType::PostToolUseFailure,
            Self::PostCompact { .. } => HookEventType::PostCompact,
            Self::StopFailure { .. } => HookEventType::StopFailure,
            Self::FileChanged { .. } => HookEventType::FileChanged,
            Self::CwdChanged { .. } => HookEventType::CwdChanged,
            Self::PermissionRequest { .. } => HookEventType::PermissionRequest,
            Self::UserPromptExpansion { .. } => HookEventType::UserPromptExpansion,
            Self::PostToolBatch { .. } => HookEventType::PostToolBatch,
            Self::ConfigChange { .. } => HookEventType::ConfigChange,
            Self::InstructionsLoaded { .. } => HookEventType::InstructionsLoaded,
            Self::WorktreeCreate { .. } => HookEventType::WorktreeCreate,
            Self::WorktreeRemove { .. } => HookEventType::WorktreeRemove,
            Self::Elicitation { .. } => HookEventType::Elicitation,
            Self::ElicitationResult { .. } => HookEventType::ElicitationResult,
            Self::TaskCreated { .. } => HookEventType::TaskCreated,
            Self::TaskCompleted { .. } => HookEventType::TaskCompleted,
        }
    }

    /// Get the matchable subject for this event.
    /// For tool events, this is the tool name.
    /// For other events, this is the stringified event data.
    pub fn match_subject(&self) -> String {
        match self {
            Self::PreToolUse { tool_name, .. } => tool_name.clone(),
            Self::PostToolUse { tool_name, .. } => tool_name.clone(),
            Self::SessionStart { session_id } => session_id.clone(),
            Self::SessionEnd { session_id } => session_id.clone(),
            Self::Notification { message } => message.clone(),
            Self::UserPromptSubmit { prompt } => prompt.clone(),
            Self::TeamTaskCreated { subject, .. } => subject.clone(),
            Self::TeamTaskCompleted { subject, .. } => subject.clone(),
            Self::TeammateIdle { agent_name, .. } => agent_name.clone(),
            Self::PreCompact { messages_count, .. } => messages_count.to_string(),
            Self::SubagentStart { agent_id, .. } => agent_id.clone(),
            Self::SubagentStop { agent_id, .. } => agent_id.clone(),
            Self::PermissionDenied { tool_name, .. } => tool_name.clone(),
            Self::Stop { tool_calls_count, .. } => tool_calls_count.to_string(),
            Self::PostToolUseFailure { tool_name, .. } => tool_name.clone(),
            Self::PostCompact { tokens_freed, .. } => tokens_freed.to_string(),
            Self::StopFailure { error } => error.clone(),
            Self::FileChanged { path, .. } => path.clone(),
            Self::CwdChanged { new_cwd, .. } => new_cwd.clone(),
            Self::PermissionRequest { tool_name, .. } => tool_name.clone(),
            Self::UserPromptExpansion { expanded_prompt, .. } => expanded_prompt.clone(),
            Self::PostToolBatch { tool_names, .. } => tool_names.join(","),
            Self::ConfigChange { config_path, .. } => config_path.clone(),
            Self::InstructionsLoaded { files_count, .. } => files_count.to_string(),
            Self::WorktreeCreate { path, .. } => path.clone(),
            Self::WorktreeRemove { path } => path.clone(),
            Self::Elicitation { source, .. } => source.clone(),
            Self::ElicitationResult { question, .. } => question.clone(),
            Self::TaskCreated { subject, .. } => subject.clone(),
            Self::TaskCompleted { subject, .. } => subject.clone(),
        }
    }

    /// Serialize the event to JSON for passing as stdin to hook commands
    pub fn to_json_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

/// Decision returned by a hook command
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookDecision {
    /// Allow the operation to proceed
    Allow,
    /// Deny the operation with a reason
    Deny { reason: String },
    /// Modify the tool input or output
    Modify {
        /// Modified input (for PreToolUse)
        #[serde(rename = "modified_input", skip_serializing_if = "Option::is_none")]
        modified_input: Option<Value>,
        /// Modified output (for PostToolUse)
        #[serde(rename = "modified_output", skip_serializing_if = "Option::is_none")]
        modified_output: Option<Value>,
    },
}

impl Default for HookDecision {
    fn default() -> Self {
        Self::Allow
    }
}

/// Result of executing a single hook command
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookResult {
    /// Exit code of the hook command
    pub exit_code: i32,
    /// Standard output from the hook command
    pub stdout: String,
    /// Standard error from the hook command
    pub stderr: String,
    /// The decision the hook made
    pub decision: HookDecision,
    /// The command that was executed
    pub command: String,
}

impl HookResult {
    /// Parse a hook decision from the stdout of a hook command.
    ///
    /// Hook commands can output JSON to the first line of stdout to communicate
    /// a decision. The format is:
    /// ```json
    /// {"decision": "deny", "reason": "not allowed"}
    /// {"decision": "modify", "modified_input": {...}}
    /// ```
    ///
    /// If no JSON is found, the default decision is Allow.
    pub fn parse_decision(stdout: &str) -> HookDecision {
        // Try to parse the first line as JSON
        if let Some(first_line) = stdout.lines().next() {
            let trimmed = first_line.trim();
            if trimmed.starts_with('{') {
                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                    if let Some(obj) = val.as_object() {
                        if let Some(decision) = obj.get("decision").and_then(|d| d.as_str()) {
                            return match decision {
                                "deny" => HookDecision::Deny {
                                    reason: obj
                                        .get("reason")
                                        .and_then(|r| r.as_str())
                                        .unwrap_or("Hook denied operation")
                                        .to_string(),
                                },
                                "modify" => HookDecision::Modify {
                                    modified_input: obj.get("modified_input").cloned(),
                                    modified_output: obj.get("modified_output").cloned(),
                                },
                                _ => HookDecision::Allow,
                            };
                        }
                    }
                }
            }
        }

        HookDecision::Allow
    }

    /// Check if this result indicates the operation should be denied
    pub fn is_denied(&self) -> bool {
        matches!(self.decision, HookDecision::Deny { .. })
    }

    /// Check if this result has modifications
    pub fn has_modifications(&self) -> bool {
        matches!(self.decision, HookDecision::Modify { .. })
    }
}

/// The type of hook execution strategy
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookType {
    /// Shell command execution (default). Receives event JSON on stdin.
    /// Exit 0 = success, exit 2 = block operation, other = non-blocking error.
    Command,
    /// HTTP POST to a URL with event JSON as body.
    Http,
    /// LLM-based evaluation with prompt template substitution.
    /// Uses the configured LlmClient to evaluate the event and return a decision.
    Llm,
    /// Single-turn LLM evaluation. The prompt receives the event JSON
    /// and must return a JSON decision on stdout.
    Prompt,
    /// Call a tool on a connected MCP server.
    McpTool,
    /// Spawn a sub-agent (with Read/Grep/Glob tools) for validation.
    Agent,
}

impl Default for HookType {
    fn default() -> Self {
        Self::Command
    }
}

/// Definition of a single hook command within a hook group
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookDef {
    /// Shell command to execute (required for command type)
    pub command: String,
    /// Hook execution type (default: "command")
    #[serde(default)]
    pub r#type: HookType,
    /// URL to POST to (required for http type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// HTTP headers for http type hooks
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// Timeout in seconds (default: 30)
    #[serde(default = "default_hook_timeout")]
    pub timeout: u64,
    /// Whether to wait for the result before continuing (default: true)
    #[serde(default = "default_hook_blocking")]
    pub blocking: bool,
    /// Environment variables to pass to the hook command.
    /// Only listed vars from the current process env are forwarded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_env_vars: Vec<String>,
    /// Shell to use for command execution (default: system shell).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Prompt template for LLM hooks. Supports variable substitution:
    /// - `{{tool_name}}`: Name of the tool being executed
    /// - `{{input}}`: Tool input as JSON
    /// - `{{event_type}}`: Type of hook event
    /// - `{{event_json}}`: Full event JSON
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_template: Option<String>,
    /// Model override for LLM hooks (uses default client model if not specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

fn default_hook_timeout() -> u64 {
    30
}

fn default_hook_blocking() -> bool {
    true
}

impl HookDef {
    /// Create a new command hook definition
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            r#type: HookType::Command,
            url: None,
            headers: HashMap::new(),
            timeout: default_hook_timeout(),
            blocking: default_hook_blocking(),
            allowed_env_vars: Vec::new(),
            shell: None,
            prompt_template: None,
            model: None,
        }
    }

    /// Create an HTTP hook that POSTs event JSON to a URL
    pub fn new_http(url: impl Into<String>) -> Self {
        Self {
            command: String::new(),
            r#type: HookType::Http,
            url: Some(url.into()),
            headers: HashMap::new(),
            timeout: default_hook_timeout(),
            blocking: default_hook_blocking(),
            allowed_env_vars: Vec::new(),
            shell: None,
            prompt_template: None,
            model: None,
        }
    }

    /// Create a prompt hook that uses single-turn LLM evaluation
    pub fn new_prompt(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            r#type: HookType::Prompt,
            url: None,
            headers: HashMap::new(),
            timeout: default_hook_timeout(),
            blocking: default_hook_blocking(),
            allowed_env_vars: Vec::new(),
            shell: None,
            prompt_template: None,
            model: None,
        }
    }

    /// Create an LLM hook that evaluates the event using the LlmClient
    ///
    /// The prompt_template should contain variable placeholders like:
    /// - `{{tool_name}}`: Name of the tool being executed
    /// - `{{input}}`: Tool input as JSON
    /// - `{{event_type}}`: Type of hook event
    /// - `{{event_json}}`: Full event JSON
    pub fn new_llm(prompt_template: impl Into<String>) -> Self {
        Self {
            command: String::new(),
            r#type: HookType::Llm,
            url: None,
            headers: HashMap::new(),
            timeout: default_hook_timeout(),
            blocking: default_hook_blocking(),
            allowed_env_vars: Vec::new(),
            shell: None,
            prompt_template: Some(prompt_template.into()),
            model: None,
        }
    }

    /// Set the timeout for this hook
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout = timeout_secs;
        self
    }

    /// Set whether this hook is blocking
    pub fn with_blocking(mut self, blocking: bool) -> Self {
        self.blocking = blocking;
        self
    }

    /// Add a header for HTTP hooks
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Get the timeout as a Duration
    pub fn timeout_duration(&self) -> Duration {
        Duration::from_secs(self.timeout)
    }
}

/// A group of hooks with a matcher pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfig {
    /// Matcher pattern: exact, `"*"` wildcard, pipe-separated `"Edit|Write"`, or regex
    pub matcher: String,
    /// Conditional: only fire if this tool+pattern matches (e.g. `"Bash(rm *)"`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_condition: Option<String>,
    /// List of hook definitions to execute
    pub hooks: Vec<HookDef>,
}

impl HookConfig {
    /// Create a new hook config
    pub fn new(matcher: impl Into<String>) -> Self {
        Self {
            matcher: matcher.into(),
            if_condition: None,
            hooks: Vec::new(),
        }
    }

    /// Add a hook definition
    pub fn with_hook(mut self, hook: HookDef) -> Self {
        self.hooks.push(hook);
        self
    }

    /// Set the conditional expression
    pub fn with_condition(mut self, cond: impl Into<String>) -> Self {
        self.if_condition = Some(cond.into());
        self
    }

    /// Check if this config matches the given subject string.
    ///
    /// Matching supports:
    /// - `"*"` wildcard: matches everything
    /// - Pipe-separated: `"Edit|Write"` matches either
    /// - Exact string match
    /// - Regex pattern match (anchored to full string)
    /// - Fallback substring match if the pattern is not valid regex
    pub fn matches(&self, subject: &str) -> bool {
        if self.matcher == "*" {
            return true;
        }

        // Try exact match first
        if self.matcher == subject {
            return true;
        }

        // Try as anchored regex (handles patterns like Ba(sh|z), Bash.*, etc.)
        if let Ok(re) = regex::Regex::new(&format!("^{}$", self.matcher)) {
            return re.is_match(subject);
        }

        // Pipe-separated matching: "Edit|Write" matches either
        if self.matcher.contains('|') {
            return self.matcher.split('|').any(|part| {
                let part = part.trim();
                part == "*" || part == subject
            });
        }

        // Fall back to substring match
        subject.contains(&self.matcher)
    }
}

/// Top-level hooks configuration file structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct HooksFile {
    /// Map of event type to list of hook configs
    pub hooks: HashMap<String, Vec<HookConfig>>,
}


impl HooksFile {
    /// Create an empty hooks file
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse hooks file from a JSON string
    pub fn from_json(json: &str) -> Result<Self, HookError> {
        let hooks: HooksFile = serde_json::from_str(json)?;
        Ok(hooks)
    }

    /// Load hooks file from disk
    pub fn load_from_file(path: &Path) -> Result<Self, HookError> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let content = std::fs::read_to_string(path)?;
        Self::from_json(&content)
    }

    /// Serialize to a JSON string
    pub fn to_json(&self) -> Result<String, HookError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Get hook configs for a specific event type
    pub fn get_for_event(&self, event_type: &HookEventType) -> Vec<&HookConfig> {
        let key = event_type.to_string();
        self.hooks
            .get(&key)
            .map(|configs| configs.iter().collect())
            .unwrap_or_default()
    }

    /// Merge another hooks file into this one.
    /// Entries from `other` are appended to existing entries for the same event type.
    pub fn merge(&mut self, other: HooksFile) {
        for (event_type, configs) in other.hooks {
            self.hooks
                .entry(event_type)
                .or_default()
                .extend(configs);
        }
    }
}

/// The hook manager that loads configs and executes hooks on events
pub struct HookManager {
    /// Combined hooks configuration (user + project level)
    hooks_file: HooksFile,
    /// Path to the user-level hooks config
    user_config_path: PathBuf,
    /// Path to the project-level hooks config
    project_config_path: PathBuf,
    /// Base directory for resolving project-level paths (defaults to cwd)
    base_dir: PathBuf,
}

impl std::fmt::Debug for HookManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookManager")
            .field("user_config_path", &self.user_config_path)
            .field("project_config_path", &self.project_config_path)
            .field("base_dir", &self.base_dir)
            .field("hooks_file", &self.hooks_file)
            .finish()
    }
}

impl Default for HookManager {
    fn default() -> Self {
        Self::new()
    }
}

impl HookManager {
    /// Create a new HookManager with default paths
    pub fn new() -> Self {
        let home_dir = dirs::home_dir().expect("Home directory should exist");
        let base_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let user_config_path = home_dir.join(".shannon").join("hooks.json");
        let project_config_path = base_dir.join(".shannon").join("hooks.json");

        Self {
            hooks_file: HooksFile::new(),
            user_config_path,
            project_config_path,
            base_dir,
        }
    }

    /// Create a HookManager with custom config paths (useful for testing)
    pub fn with_paths(user_path: PathBuf, project_path: PathBuf) -> Self {
        Self {
            hooks_file: HooksFile::new(),
            user_config_path: user_path,
            project_config_path: project_path,
            base_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    /// Create a HookManager with an explicit base directory for project-level paths
    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        let home_dir = dirs::home_dir().expect("Home directory should exist");
        let user_config_path = home_dir.join(".shannon").join("hooks.json");
        let project_config_path = base_dir.join(".shannon").join("hooks.json");

        Self {
            hooks_file: HooksFile::new(),
            user_config_path,
            project_config_path,
            base_dir,
        }
    }

    /// Load hook configurations from disk.
    ///
    /// Loads from both Shannon-native paths and Claude Code compatible paths.
    /// Later files override earlier ones. Load order:
    ///
    /// User-level (lower priority):
    /// - `~/.claude/settings.json`
    /// - `~/.shannon/settings.json`
    /// - `~/.shannon/hooks.json`
    ///
    /// Project-level (higher priority):
    /// - `.claude/settings.json`
    /// - `.claude/settings.local.json`
    /// - `.shannon/settings.json`
    /// - `.shannon/settings.local.json`
    /// - `.shannon/hooks.json`
    pub fn load(&mut self) -> Result<(), HookError> {
        let mut combined = HooksFile::new();

        let home_dir = dirs::home_dir().ok_or(HookError::HomeNotFound)?;
        let base = &self.base_dir;

        // User-level hooks (lower priority, loaded first)
        let user_paths: Vec<PathBuf> = vec![
            home_dir.join(".claude").join("settings.json"),
            home_dir.join(".shannon").join("settings.json"),
            self.user_config_path.clone(),
        ];

        // Project-level hooks (higher priority, loaded after)
        let project_paths: Vec<PathBuf> = vec![
            base.join(".claude").join("settings.json"),
            base.join(".claude").join("settings.local.json"),
            base.join(".shannon").join("settings.json"),
            base.join(".shannon").join("settings.local.json"),
            self.project_config_path.clone(),
        ];

        for path in user_paths.iter().chain(project_paths.iter()) {
            if path.exists() {
                match HooksFile::load_from_file(path) {
                    Ok(hooks) => {
                        tracing::debug!("Loaded hooks from {}", path.display());
                        combined.merge(hooks);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to load hooks from {}: {e}", path.display());
                    }
                }
            }
        }

        self.hooks_file = combined;
        Ok(())
    }

    /// Load hook configuration from a specific file path
    pub fn load_from_path(&mut self, path: &Path) -> Result<(), HookError> {
        let hooks = HooksFile::load_from_file(path)?;
        self.hooks_file = hooks;
        Ok(())
    }

    /// Run all matching hooks for a given event.
    ///
    /// Returns a vector of results from each hook that matched and was executed.
    /// If any hook returns a Deny decision, subsequent hooks for that event
    /// are still executed, but the caller should check all results for denials.
    pub async fn run_hooks(&self, event: &HookEvent) -> Result<Vec<HookResult>, HookError> {
        let event_type = event.event_type();
        let subject = event.match_subject();
        let configs = self.hooks_file.get_for_event(&event_type);

        let mut results = Vec::new();
        let event_json = event.to_json_bytes();

        for config in &configs {
            if !config.matches(&subject) {
                continue;
            }

            for hook_def in &config.hooks {
                let result = if !hook_def.blocking {
                    // For non-blocking hooks, spawn and detach
                    match &hook_def.r#type {
                        HookType::Command => {
                            self.spawn_hook(&hook_def.command, &event_json)?;
                        }
                        HookType::Http => {
                            self.spawn_http_hook(hook_def, &event_json)?;
                        }
                        HookType::Prompt => {
                            // Prompt hooks are always blocking (need LLM response)
                            let result = self.execute_prompt_hook(hook_def, &event_json).await?;
                            results.push(result);
                        }
                        HookType::McpTool | HookType::Agent => {
                            // MCP tool and agent hooks are always blocking
                            let result = self.execute_hook(&hook_def.command, hook_def.timeout_duration(), &event_json).await?;
                            results.push(result);
                        }
                        HookType::Llm => {
                            // LLM hooks are always blocking (need LLM response)
                            let result = self.execute_llm_hook(hook_def, &event_json).await?;
                            results.push(result);
                        }
                    }
                    continue;
                } else {
                    match &hook_def.r#type {
                        HookType::Command => {
                            self.execute_hook(&hook_def.command, hook_def.timeout_duration(), &event_json).await?
                        }
                        HookType::Http => {
                            self.execute_http_hook(hook_def, &event_json).await?
                        }
                        HookType::Prompt => {
                            self.execute_prompt_hook(hook_def, &event_json).await?
                        }
                        HookType::McpTool | HookType::Agent => {
                            self.execute_hook(&hook_def.command, hook_def.timeout_duration(), &event_json).await?
                        }
                        HookType::Llm => {
                            self.execute_llm_hook(hook_def, &event_json).await?
                        }
                    }
                };

                results.push(result);
            }
        }

        Ok(results)
    }

    /// Execute a single hook command and capture its output
    async fn execute_hook(
        &self,
        command: &str,
        timeout: Duration,
        stdin_data: &[u8],
    ) -> Result<HookResult, HookError> {
        let result = tokio::time::timeout(timeout, async {
            let mut child = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?;

            // Write event data to stdin
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(stdin_data).await;
                // Drop stdin to close the pipe
                drop(stdin);
            }

            let output = child.wait_with_output().await?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);

            // Exit code semantics (Claude Code standard):
            //   0 = success, parse stdout JSON for decision
            //   2 = block operation, stderr shown to LLM
            //   other = non-blocking error, continue execution
            let decision = if exit_code == 2 {
                HookDecision::Deny {
                    reason: if stderr.is_empty() {
                        "Hook blocked the operation (exit 2)".to_string()
                    } else {
                        stderr.clone()
                    },
                }
            } else {
                HookResult::parse_decision(&stdout)
            };

            Ok::<HookResult, HookError>(HookResult {
                exit_code,
                stdout,
                stderr,
                decision,
                command: command.to_string(),
            })
        })
        .await;

        match result {
            Ok(Ok(hook_result)) => Ok(hook_result),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(HookError::Timeout {
                command: command.to_string(),
                timeout_secs: timeout.as_secs(),
            }),
        }
    }

    /// Spawn a non-blocking hook (fire and forget)
    fn spawn_hook(&self, command: &str, stdin_data: &[u8]) -> Result<(), HookError> {
        let stdin_data = stdin_data.to_vec();
        let command = command.to_string();

        tokio::spawn(async move {
            if let Ok(mut child) = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    use tokio::io::AsyncWriteExt;
                    let _ = stdin.write_all(&stdin_data).await;
                }
                let _ = child.wait().await;
            }
        });

        Ok(())
    }

    /// Execute an HTTP hook: POST event JSON to the configured URL
    async fn execute_http_hook(
        &self,
        hook_def: &HookDef,
        event_json: &[u8],
    ) -> Result<HookResult, HookError> {
        let url = hook_def.url.as_deref().ok_or_else(|| {
            HookError::InvalidMatcher("HTTP hook requires a 'url' field".to_string())
        })?;

        let timeout = hook_def.timeout_duration();
        let result = tokio::time::timeout(timeout, async {
            let mut builder = reqwest::Client::new().post(url);
            for (key, value) in &hook_def.headers {
                builder = builder.header(key.as_str(), value.as_str());
            }

            let response = builder
                .header("Content-Type", "application/json")
                .body(event_json.to_vec())
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status().as_u16() as i32;
                    let body = resp.text().await.unwrap_or_default();
                    let decision = if (200..300).contains(&status) {
                        // Try parsing decision from response body
                        HookResult::parse_decision(&body)
                    } else {
                        HookDecision::Deny {
                            reason: format!("HTTP hook returned status {status}"),
                        }
                    };
                    Ok::<HookResult, HookError>(HookResult {
                        exit_code: status,
                        stdout: body,
                        stderr: String::new(),
                        decision,
                        command: format!("POST {url}"),
                    })
                }
                Err(e) => Ok::<HookResult, HookError>(HookResult {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: e.to_string(),
                    decision: HookDecision::Allow, // Network errors don't block by default
                    command: format!("POST {url}"),
                }),
            }
        })
        .await;

        match result {
            Ok(Ok(hook_result)) => Ok(hook_result),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(HookError::Timeout {
                command: format!("POST {url}"),
                timeout_secs: timeout.as_secs(),
            }),
        }
    }

    /// Spawn a non-blocking HTTP hook (fire and forget)
    fn spawn_http_hook(&self, hook_def: &HookDef, event_json: &[u8]) -> Result<(), HookError> {
        let url = hook_def.url.clone();
        let headers = hook_def.headers.clone();
        let event_json = event_json.to_vec();

        tokio::spawn(async move {
            if let Some(url) = url {
                let mut builder = reqwest::Client::new().post(&url);
                for (key, value) in &headers {
                    builder = builder.header(key.as_str(), value.as_str());
                }
                let _ = builder
                    .header("Content-Type", "application/json")
                    .body(event_json)
                    .send()
                    .await;
            }
        });

        Ok(())
    }

    /// Execute a prompt hook: single-turn LLM evaluation
    ///
    /// The command is treated as a prompt template. The event JSON is appended
    /// as context. The LLM's first line of output is parsed as a HookDecision.
    async fn execute_prompt_hook(
        &self,
        hook_def: &HookDef,
        event_json: &[u8],
    ) -> Result<HookResult, HookError> {
        // For prompt hooks, we invoke the command as a shell command but
        // pass the event JSON and expect a JSON decision back.
        // This allows using any CLI tool that can evaluate prompts.
        let timeout = hook_def.timeout_duration();
        let command = &hook_def.command;

        let result = tokio::time::timeout(timeout, async {
            let mut child = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?;

            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(event_json).await;
                drop(stdin);
            }

            let output = child.wait_with_output().await?;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);

            // Prompt hooks: exit code 2 = deny
            let decision = if exit_code == 2 {
                HookDecision::Deny {
                    reason: if stderr.is_empty() {
                        "Prompt hook denied".to_string()
                    } else {
                        stderr.clone()
                    },
                }
            } else {
                HookResult::parse_decision(&stdout)
            };

            Ok::<HookResult, HookError>(HookResult {
                exit_code,
                stdout,
                stderr,
                decision,
                command: format!("prompt: {command}"),
            })
        })
        .await;

        match result {
            Ok(Ok(hook_result)) => Ok(hook_result),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(HookError::Timeout {
                command: hook_def.command.clone(),
                timeout_secs: timeout.as_secs(),
            }),
        }
    }

    /// Execute an LLM hook: LLM-powered hook evaluation
    ///
    /// LLM hooks use an LLM to evaluate hook events and make decisions.
    /// This is a stub implementation that returns Allow until full LLM integration is complete.
    async fn execute_llm_hook(
        &self,
        hook_def: &HookDef,
        _event_json: &[u8],
    ) -> Result<HookResult, HookError> {
        // TODO: Implement full LLM hook support
        // For now, return Allow decision so code compiles
        Ok(HookResult {
            exit_code: 0,
            stdout: "LLM hook not yet implemented, allowing by default".to_string(),
            stderr: String::new(),
            decision: HookDecision::Allow,
            command: format!("llm: {}", hook_def.command),
        })
    }

    /// Process hook results and determine the final outcome.
    ///
    /// Returns the combined decision:
    /// - If any hook denies, returns the first denial reason.
    /// - If any hook modifies, returns the last modification.
    /// - Otherwise, returns Allow.
    pub fn resolve_results(results: &[HookResult]) -> HookDecision {
        let mut last_modify = None;

        for result in results {
            match &result.decision {
                HookDecision::Deny { reason } => {
                    return HookDecision::Deny {
                        reason: reason.clone(),
                    };
                }
                HookDecision::Modify {
                    modified_input,
                    modified_output,
                } => {
                    last_modify = Some(HookDecision::Modify {
                        modified_input: modified_input.clone(),
                        modified_output: modified_output.clone(),
                    });
                }
                HookDecision::Allow => {}
            }
        }

        last_modify.unwrap_or(HookDecision::Allow)
    }

    /// Get the user config path
    pub fn user_config_path(&self) -> &Path {
        &self.user_config_path
    }

    /// Get the project config path
    pub fn project_config_path(&self) -> &Path {
        &self.project_config_path
    }

    /// Get a reference to the loaded hooks file
    pub fn hooks_file(&self) -> &HooksFile {
        &self.hooks_file
    }

    /// Get the list of event types that have configured hooks
    pub fn configured_event_types(&self) -> Vec<HookEventType> {
        self.hooks_file
            .hooks
            .keys()
            .filter_map(|k| HookEventType::from_str_lossy(k))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── HookEventType tests ──────────────────────────────────────────────

    #[test]
    fn test_hook_event_type_from_str() {
        assert_eq!(
            HookEventType::from_str_lossy("PreToolUse"),
            Some(HookEventType::PreToolUse)
        );
        assert_eq!(
            HookEventType::from_str_lossy("PostToolUse"),
            Some(HookEventType::PostToolUse)
        );
        assert_eq!(
            HookEventType::from_str_lossy("SessionStart"),
            Some(HookEventType::SessionStart)
        );
        assert_eq!(
            HookEventType::from_str_lossy("SessionEnd"),
            Some(HookEventType::SessionEnd)
        );
        assert_eq!(
            HookEventType::from_str_lossy("Notification"),
            Some(HookEventType::Notification)
        );
        assert_eq!(
            HookEventType::from_str_lossy("UserPromptSubmit"),
            Some(HookEventType::UserPromptSubmit)
        );
        assert_eq!(HookEventType::from_str_lossy("Unknown"), None);
    }

    #[test]
    fn test_hook_event_type_display() {
        assert_eq!(HookEventType::PreToolUse.to_string(), "PreToolUse");
        assert_eq!(HookEventType::PostToolUse.to_string(), "PostToolUse");
        assert_eq!(HookEventType::SessionStart.to_string(), "SessionStart");
        assert_eq!(HookEventType::SessionEnd.to_string(), "SessionEnd");
        assert_eq!(HookEventType::Notification.to_string(), "Notification");
        assert_eq!(HookEventType::UserPromptSubmit.to_string(), "UserPromptSubmit");
    }

    #[test]
    fn test_hook_event_type_serialization() {
        let event_type = HookEventType::PreToolUse;
        let json = serde_json::to_string(&event_type).unwrap();
        assert_eq!(json, "\"PreToolUse\"");

        let parsed: HookEventType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, HookEventType::PreToolUse);
    }

    // ── HookEvent tests ──────────────────────────────────────────────────

    #[test]
    fn test_hook_event_event_type() {
        let event = HookEvent::PreToolUse {
            tool_name: "Bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        };
        assert_eq!(event.event_type(), HookEventType::PreToolUse);

        let event = HookEvent::SessionStart {
            session_id: "abc123".to_string(),
        };
        assert_eq!(event.event_type(), HookEventType::SessionStart);
    }

    #[test]
    fn test_hook_event_match_subject() {
        let event = HookEvent::PreToolUse {
            tool_name: "Bash".to_string(),
            input: serde_json::json!({}),
        };
        assert_eq!(event.match_subject(), "Bash");

        let event = HookEvent::SessionEnd {
            session_id: "sess-42".to_string(),
        };
        assert_eq!(event.match_subject(), "sess-42");

        let event = HookEvent::UserPromptSubmit {
            prompt: "Hello world".to_string(),
        };
        assert_eq!(event.match_subject(), "Hello world");
    }

    #[test]
    fn test_hook_event_serialization() {
        let event = HookEvent::PreToolUse {
            tool_name: "Bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("PreToolUse"));
        assert!(json.contains("Bash"));
        assert!(json.contains("command"));

        let parsed: HookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event_type(), HookEventType::PreToolUse);
    }

    // ── HookDecision tests ───────────────────────────────────────────────

    #[test]
    fn test_hook_decision_default() {
        assert_eq!(HookDecision::default(), HookDecision::Allow);
    }

    #[test]
    fn test_hook_decision_serialization() {
        let decision = HookDecision::Deny {
            reason: "not allowed".to_string(),
        };
        let json = serde_json::to_string(&decision).unwrap();
        assert!(json.contains("Deny"));
        assert!(json.contains("not allowed"));

        let parsed: HookDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, decision);
    }

    #[test]
    fn test_hook_decision_modify_serialization() {
        let decision = HookDecision::Modify {
            modified_input: Some(serde_json::json!({"command": "echo hello"})),
            modified_output: None,
        };
        let json = serde_json::to_string(&decision).unwrap();
        assert!(json.contains("modified_input"));
        assert!(!json.contains("modified_output"));

        let parsed: HookDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, decision);
    }

    // ── HookResult tests ─────────────────────────────────────────────────

    #[test]
    fn test_hook_result_parse_decision_empty() {
        let decision = HookResult::parse_decision("");
        assert_eq!(decision, HookDecision::Allow);
    }

    #[test]
    fn test_hook_result_parse_decision_plain_text() {
        let decision = HookResult::parse_decision("Just some output\nMore output");
        assert_eq!(decision, HookDecision::Allow);
    }

    #[test]
    fn test_hook_result_parse_decision_deny() {
        let stdout = r#"{"decision": "deny", "reason": "not allowed"}"#;
        let decision = HookResult::parse_decision(stdout);
        assert_eq!(
            decision,
            HookDecision::Deny {
                reason: "not allowed".to_string()
            }
        );
    }

    #[test]
    fn test_hook_result_parse_decision_modify() {
        let stdout = r#"{"decision": "modify", "modified_input": {"command": "echo hello"}}"#;
        let decision = HookResult::parse_decision(stdout);
        assert_eq!(
            decision,
            HookDecision::Modify {
                modified_input: Some(serde_json::json!({"command": "echo hello"})),
                modified_output: None,
            }
        );
    }

    #[test]
    fn test_hook_result_parse_decision_with_extra_lines() {
        let stdout = r#"{"decision": "deny", "reason": "blocked"}
Some debug output
More lines"#;
        let decision = HookResult::parse_decision(stdout);
        assert_eq!(
            decision,
            HookDecision::Deny {
                reason: "blocked".to_string()
            }
        );
    }

    #[test]
    fn test_hook_result_parse_decision_invalid_json() {
        let stdout = "{invalid json";
        let decision = HookResult::parse_decision(stdout);
        assert_eq!(decision, HookDecision::Allow);
    }

    #[test]
    fn test_hook_result_parse_decision_unknown_decision() {
        let stdout = r#"{"decision": "unknown"}"#;
        let decision = HookResult::parse_decision(stdout);
        assert_eq!(decision, HookDecision::Allow);
    }

    #[test]
    fn test_hook_result_is_denied() {
        let result = HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision: HookDecision::Deny {
                reason: "test".to_string(),
            },
            command: "test".to_string(),
        };
        assert!(result.is_denied());
        assert!(!result.has_modifications());
    }

    #[test]
    fn test_hook_result_has_modifications() {
        let result = HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision: HookDecision::Modify {
                modified_input: None,
                modified_output: None,
            },
            command: "test".to_string(),
        };
        assert!(!result.is_denied());
        assert!(result.has_modifications());
    }

    // ── HookDef tests ────────────────────────────────────────────────────

    #[test]
    fn test_hook_def_new() {
        let hook = HookDef::new("echo hello");
        assert_eq!(hook.command, "echo hello");
        assert_eq!(hook.timeout, 30);
        assert!(hook.blocking);
    }

    #[test]
    fn test_hook_def_builder() {
        let hook = HookDef::new("echo hello")
            .with_timeout(10)
            .with_blocking(false);
        assert_eq!(hook.timeout, 10);
        assert!(!hook.blocking);
    }

    // ── Pipe-separated matcher tests ────────────────────────────────────

    #[test]
    fn test_matcher_pipe_separated_match() {
        let config = HookConfig::new("Edit|Write");
        assert!(config.matches("Edit"));
        assert!(config.matches("Write"));
        assert!(!config.matches("Bash"));
    }

    #[test]
    fn test_matcher_pipe_separated_with_wildcard() {
        let config = HookConfig::new("Bash|*");
        assert!(config.matches("Bash"));
        assert!(config.matches("Edit"));
        assert!(config.matches("Anything"));
    }

    #[test]
    fn test_matcher_single_no_pipe() {
        let config = HookConfig::new("Bash");
        assert!(config.matches("Bash"));
        assert!(!config.matches("Edit"));
    }

    #[test]
    fn test_matcher_wildcard() {
        let config = HookConfig::new("*");
        assert!(config.matches("Bash"));
        assert!(config.matches("Edit"));
        assert!(config.matches("Anything"));
    }

    #[test]
    fn test_hook_config_with_condition() {
        let config = HookConfig::new("Bash")
            .with_condition("Bash(rm *)");
        assert_eq!(config.if_condition.as_deref(), Some("Bash(rm *)"));
    }

    #[test]
    fn test_hook_def_timeout_duration() {
        let hook = HookDef::new("echo hello").with_timeout(5);
        assert_eq!(hook.timeout_duration(), Duration::from_secs(5));
    }

    #[test]
    fn test_hook_def_deserialization_defaults() {
        let json = r#"{"command": "echo hello"}"#;
        let hook: HookDef = serde_json::from_str(json).unwrap();
        assert_eq!(hook.command, "echo hello");
        assert_eq!(hook.timeout, 30);
        assert!(hook.blocking);
    }

    // ── HookConfig tests ─────────────────────────────────────────────────

    #[test]
    fn test_hook_config_wildcard_match() {
        let config = HookConfig::new("*");
        assert!(config.matches("Bash"));
        assert!(config.matches("Read"));
        assert!(config.matches(""));
    }

    #[test]
    fn test_hook_config_exact_match() {
        let config = HookConfig::new("Bash");
        assert!(config.matches("Bash"));
        assert!(!config.matches("Read"));
        assert!(!config.matches("BashTool"));
    }

    #[test]
    fn test_hook_config_regex_match() {
        let config = HookConfig::new("Bash.*");
        assert!(config.matches("Bash"));
        assert!(config.matches("BashTool"));
        assert!(!config.matches("Read"));
    }

    #[test]
    fn test_hook_config_fallback_substring_match() {
        // An invalid regex falls back to substring matching
        let config = HookConfig::new("[invalid");
        assert!(config.matches("[invalid"));
        assert!(config.matches("some[invalid]thing"));
        assert!(!config.matches("something-else"));
    }

    #[test]
    fn test_hook_config_builder() {
        let config = HookConfig::new("Bash")
            .with_hook(HookDef::new("echo pre"))
            .with_hook(HookDef::new("echo post").with_blocking(false));

        assert_eq!(config.matcher, "Bash");
        assert_eq!(config.hooks.len(), 2);
        assert!(config.hooks[0].blocking);
        assert!(!config.hooks[1].blocking);
    }

    // ── HooksFile tests ──────────────────────────────────────────────────

    #[test]
    fn test_hooks_file_new() {
        let file = HooksFile::new();
        assert!(file.hooks.is_empty());
    }

    #[test]
    fn test_hooks_file_from_json() {
        let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {"command": "echo pre", "timeout": 5, "blocking": false}
                        ]
                    }
                ],
                "SessionStart": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo started"}
                        ]
                    }
                ]
            }
        }"#;

        let file = HooksFile::from_json(json).unwrap();
        assert_eq!(file.hooks.len(), 2);

        let pre_tool = file.hooks.get("PreToolUse").unwrap();
        assert_eq!(pre_tool.len(), 1);
        assert_eq!(pre_tool[0].matcher, "Bash");
        assert_eq!(pre_tool[0].hooks.len(), 1);
        assert_eq!(pre_tool[0].hooks[0].command, "echo pre");
        assert_eq!(pre_tool[0].hooks[0].timeout, 5);
        assert!(!pre_tool[0].hooks[0].blocking);

        let session = file.hooks.get("SessionStart").unwrap();
        assert_eq!(session.len(), 1);
        assert_eq!(session[0].matcher, "*");
        assert_eq!(session[0].hooks[0].timeout, 30);
        assert!(session[0].hooks[0].blocking);
    }

    #[test]
    fn test_hooks_file_get_for_event() {
        let mut file = HooksFile::new();
        file.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookConfig::new("Bash")],
        );

        let configs = file.get_for_event(&HookEventType::PreToolUse);
        assert_eq!(configs.len(), 1);

        let configs = file.get_for_event(&HookEventType::SessionStart);
        assert!(configs.is_empty());
    }

    #[test]
    fn test_hooks_file_merge() {
        let mut file1 = HooksFile::new();
        file1.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookConfig::new("Bash")],
        );

        let mut file2 = HooksFile::new();
        file2.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookConfig::new("Read")],
        );
        file2.hooks.insert(
            "SessionStart".to_string(),
            vec![HookConfig::new("*")],
        );

        file1.merge(file2);

        let pre = file1.hooks.get("PreToolUse").unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0].matcher, "Bash");
        assert_eq!(pre[1].matcher, "Read");

        let session = file1.hooks.get("SessionStart").unwrap();
        assert_eq!(session.len(), 1);
    }

    #[test]
    fn test_hooks_file_to_json() {
        let file = HooksFile::new();
        let json = file.to_json().unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["hooks"].is_object());
    }

    #[test]
    fn test_hooks_file_load_nonexistent() {
        let file = HooksFile::load_from_file(Path::new("/nonexistent/path/hooks.json"));
        assert!(file.is_ok());
        assert!(file.unwrap().hooks.is_empty());
    }

    #[test]
    fn test_hooks_file_invalid_json() {
        let result = HooksFile::from_json("not json");
        assert!(result.is_err());
    }

    // ── HookManager tests ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_hook_manager_new() {
        let manager = HookManager::new();
        assert_eq!(
            manager.user_config_path().file_name().unwrap(),
            "hooks.json"
        );
        assert!(manager.project_config_path().ends_with(".shannon/hooks.json"));
    }

    #[tokio::test]
    async fn test_hook_manager_load_nonexistent() {
        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent/user_hooks.json"),
            PathBuf::from("/nonexistent/project_hooks.json"),
        );

        assert!(manager.load().is_ok());
        assert!(manager.configured_event_types().is_empty());
    }

    #[tokio::test]
    async fn test_hook_manager_load_from_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let hooks_path = temp_dir.path().join("hooks.json");

        let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {"command": "echo pre-bash", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

        std::fs::write(&hooks_path, json).unwrap();

        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        manager.load_from_path(&hooks_path).unwrap();

        let event_types = manager.configured_event_types();
        assert_eq!(event_types.len(), 1);
        assert_eq!(event_types[0], HookEventType::PreToolUse);
    }

    #[tokio::test]
    async fn test_hook_manager_run_hooks_blocking() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let hooks_path = temp_dir.path().join("hooks.json");

        let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {"command": "echo pre-tool", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

        std::fs::write(&hooks_path, json).unwrap();

        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        manager.load_from_path(&hooks_path).unwrap();

        let event = HookEvent::PreToolUse {
            tool_name: "Bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        };

        let results = manager.run_hooks(&event).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].exit_code, 0);
        assert!(results[0].stdout.contains("pre-tool"));
        assert_eq!(results[0].decision, HookDecision::Allow);
    }

    #[tokio::test]
    async fn test_hook_manager_run_hooks_no_match() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let hooks_path = temp_dir.path().join("hooks.json");

        let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Read",
                        "hooks": [
                            {"command": "echo read-hook", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

        std::fs::write(&hooks_path, json).unwrap();

        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        manager.load_from_path(&hooks_path).unwrap();

        let event = HookEvent::PreToolUse {
            tool_name: "Bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        };

        let results = manager.run_hooks(&event).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_hook_manager_run_hooks_deny_decision() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let hooks_path = temp_dir.path().join("hooks.json");

        let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo '{\"decision\": \"deny\", \"reason\": \"blocked by policy\"}'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

        std::fs::write(&hooks_path, json).unwrap();

        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        manager.load_from_path(&hooks_path).unwrap();

        let event = HookEvent::PreToolUse {
            tool_name: "Bash".to_string(),
            input: serde_json::json!({"command": "rm -rf /"}),
        };

        let results = manager.run_hooks(&event).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_denied());
    }

    #[tokio::test]
    async fn test_hook_manager_run_hooks_timeout() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let hooks_path = temp_dir.path().join("hooks.json");

        // Register hook under SessionStart (matching the event we'll fire)
        let json = r#"{
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "sleep 60", "timeout": 1, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

        std::fs::write(&hooks_path, json).unwrap();

        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        manager.load_from_path(&hooks_path).unwrap();

        let event = HookEvent::SessionStart {
            session_id: "test".to_string(),
        };

        let result = manager.run_hooks(&event).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("timed out"), "Error should mention timeout: {err}");
    }

    #[tokio::test]
    async fn test_hook_manager_run_hooks_non_blocking() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let hooks_path = temp_dir.path().join("hooks.json");

        // Non-blocking hooks should not appear in results
        let json = r#"{
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo 'non-blocking'", "timeout": 5, "blocking": false}
                        ]
                    }
                ]
            }
        }"#;

        std::fs::write(&hooks_path, json).unwrap();

        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        manager.load_from_path(&hooks_path).unwrap();

        let event = HookEvent::SessionStart {
            session_id: "test".to_string(),
        };

        let results = manager.run_hooks(&event).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_hook_manager_run_hooks_wildcard_match() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let hooks_path = temp_dir.path().join("hooks.json");

        let json = r#"{
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo 'session started'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

        std::fs::write(&hooks_path, json).unwrap();

        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        manager.load_from_path(&hooks_path).unwrap();

        let event = HookEvent::SessionStart {
            session_id: "any-session-id".to_string(),
        };

        let results = manager.run_hooks(&event).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].stdout.contains("session started"));
    }

    // ── resolve_results tests ────────────────────────────────────────────

    #[test]
    fn test_resolve_results_all_allow() {
        let results = vec![
            HookResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
                decision: HookDecision::Allow,
                command: "cmd1".to_string(),
            },
            HookResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
                decision: HookDecision::Allow,
                command: "cmd2".to_string(),
            },
        ];
        assert_eq!(HookManager::resolve_results(&results), HookDecision::Allow);
    }

    #[test]
    fn test_resolve_results_first_deny_wins() {
        let results = vec![
            HookResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
                decision: HookDecision::Allow,
                command: "cmd1".to_string(),
            },
            HookResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
                decision: HookDecision::Deny {
                    reason: "first deny".to_string(),
                },
                command: "cmd2".to_string(),
            },
            HookResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
                decision: HookDecision::Deny {
                    reason: "second deny".to_string(),
                },
                command: "cmd3".to_string(),
            },
        ];
        let resolved = HookManager::resolve_results(&results);
        assert_eq!(
            resolved,
            HookDecision::Deny {
                reason: "first deny".to_string()
            }
        );
    }

    #[test]
    fn test_resolve_results_last_modify_wins() {
        let results = vec![
            HookResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
                decision: HookDecision::Modify {
                    modified_input: Some(serde_json::json!({"v": 1})),
                    modified_output: None,
                },
                command: "cmd1".to_string(),
            },
            HookResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
                decision: HookDecision::Modify {
                    modified_input: Some(serde_json::json!({"v": 2})),
                    modified_output: None,
                },
                command: "cmd2".to_string(),
            },
        ];
        let resolved = HookManager::resolve_results(&results);
        assert_eq!(
            resolved,
            HookDecision::Modify {
                modified_input: Some(serde_json::json!({"v": 2})),
                modified_output: None,
            }
        );
    }

    #[test]
    fn test_resolve_results_deny_overrides_modify() {
        let results = vec![
            HookResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
                decision: HookDecision::Modify {
                    modified_input: Some(serde_json::json!({"v": 1})),
                    modified_output: None,
                },
                command: "cmd1".to_string(),
            },
            HookResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
                decision: HookDecision::Deny {
                    reason: "blocked".to_string(),
                },
                command: "cmd2".to_string(),
            },
        ];
        let resolved = HookManager::resolve_results(&results);
        assert_eq!(
            resolved,
            HookDecision::Deny {
                reason: "blocked".to_string()
            }
        );
    }

    #[test]
    fn test_resolve_results_empty() {
        let results: Vec<HookResult> = vec![];
        assert_eq!(HookManager::resolve_results(&results), HookDecision::Allow);
    }

    // ── HookManager merge/load tests ─────────────────────────────────────

    #[tokio::test]
    async fn test_hook_manager_merge_user_and_project() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let user_path = temp_dir.path().join("user_hooks.json");
        let project_path = temp_dir.path().join("project_hooks.json");

        let user_json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {"command": "echo 'user hook'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

        let project_json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Read",
                        "hooks": [
                            {"command": "echo 'project hook'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

        std::fs::write(&user_path, user_json).unwrap();
        std::fs::write(&project_path, project_json).unwrap();

        let mut manager = HookManager::with_paths(user_path, project_path);
        manager.load().unwrap();

        // Both user and project hooks should be present
        let configs = manager.hooks_file().get_for_event(&HookEventType::PreToolUse);
        assert_eq!(configs.len(), 2);
    }

    // ── Integration: full flow test ──────────────────────────────────────

    #[tokio::test]
    async fn test_full_flow_with_modify_decision() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let hooks_path = temp_dir.path().join("hooks.json");

        // Hook that modifies the tool input
        let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {"command": "echo '{\"decision\": \"modify\", \"modified_input\": {\"command\": \"echo safe\"}}'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

        std::fs::write(&hooks_path, json).unwrap();

        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        manager.load_from_path(&hooks_path).unwrap();

        let event = HookEvent::PreToolUse {
            tool_name: "Bash".to_string(),
            input: serde_json::json!({"command": "rm -rf /"}),
        };

        let results = manager.run_hooks(&event).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].has_modifications());

        let resolved = HookManager::resolve_results(&results);
        if let HookDecision::Modify { modified_input, .. } = resolved {
            assert_eq!(
                modified_input,
                Some(serde_json::json!({"command": "echo safe"}))
            );
        } else {
            panic!("Expected Modify decision, got {resolved:?}");
        }
    }

    // ── Edge case tests ──────────────────────────────────────────────────

    #[test]
    fn test_hook_event_to_json_bytes() {
        let event = HookEvent::Notification {
            message: "test message".to_string(),
        };
        let bytes = event.to_json_bytes();
        let parsed: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["Notification"]["message"], "test message");
    }

    #[test]
    fn test_hook_config_serialization_camel_case() {
        let config = HookConfig {
            matcher: "Bash".to_string(),
            if_condition: None,
            hooks: vec![HookDef {
                command: "echo test".to_string(),
                r#type: HookType::Command,
                url: None,
                headers: HashMap::new(),
                timeout: 10,
                blocking: false,
                allowed_env_vars: Vec::new(),
                shell: None,
                prompt_template: None,
                model: None,
            }],
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("matcher"));
        assert!(json.contains("hooks"));
        assert!(json.contains("command"));
        assert!(json.contains("timeout"));
        assert!(json.contains("blocking"));

        // camelCase is handled by serde rename_all
        let parsed: HookConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.matcher, "Bash");
        assert_eq!(parsed.hooks.len(), 1);
    }

    #[test]
    fn test_hook_error_display() {
        let err = HookError::Timeout {
            command: "sleep 100".to_string(),
            timeout_secs: 5,
        };
        assert!(err.to_string().contains("timed out"));
        assert!(err.to_string().contains("5"));

        let err = HookError::Denied {
            reason: "policy".to_string(),
        };
        assert!(err.to_string().contains("policy"));
    }

    // ── Claude Code compatible path loading ─────────────────────────────

    #[test]
    fn test_hooks_file_ignores_non_hook_fields() {
        // Claude Code settings.json has extra fields like mcpServers, permissions
        let json = r#"{
            "mcpServers": {
                "fetch": {"command": "uvx", "args": ["mcp-server-fetch"]}
            },
            "permissions": {"allow": ["Bash"]},
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {"command": "echo 'hook ran'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;
        let file = HooksFile::from_json(json).unwrap();
        let configs = file.get_for_event(&HookEventType::PreToolUse);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].matcher, "Bash");
    }

    #[tokio::test]
    async fn test_load_from_claude_code_settings_paths() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        // Create .claude/settings.json in the temp dir (project-level)
        let claude_dir = temp_dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let claude_settings = claude_dir.join("settings.json");
        std::fs::write(&claude_settings, r#"{
            "hooks": {
                "SessionStart": [
                    {"matcher": "*", "hooks": [{"command": "echo 'claude session start'", "timeout": 5}]}
                ]
            },
            "mcpServers": {}
        }"#).unwrap();

        // Also create .shannon/hooks.json (project-level via base_dir)
        let shannon_dir = temp_dir.path().join(".shannon");
        std::fs::create_dir_all(&shannon_dir).unwrap();
        let shannon_hooks = shannon_dir.join("hooks.json");
        std::fs::write(&shannon_hooks, r#"{
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"command": "echo 'shannon hook'", "timeout": 5}]}
                ]
            }
        }"#).unwrap();

        let mut manager = HookManager::with_base_dir(temp_dir.path().to_path_buf());
        manager.load().unwrap();

        // Both .claude/settings.json and .shannon/hooks.json should be loaded
        let start_configs = manager.hooks_file().get_for_event(&HookEventType::SessionStart);
        assert_eq!(start_configs.len(), 1);

        let pre_configs = manager.hooks_file().get_for_event(&HookEventType::PreToolUse);
        assert_eq!(pre_configs.len(), 1);
    }

    #[tokio::test]
    async fn test_load_priority_later_overrides() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        // Create .claude/settings.local.json (highest project priority)
        let claude_dir = temp_dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let local_settings = claude_dir.join("settings.local.json");
        std::fs::write(&local_settings, r#"{
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"command": "echo 'local override'", "timeout": 5}]}
                ]
            }
        }"#).unwrap();

        let mut manager = HookManager::with_base_dir(temp_dir.path().to_path_buf());
        manager.load().unwrap();

        let configs = manager.hooks_file().get_for_event(&HookEventType::PreToolUse);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].hooks[0].command, "echo 'local override'");
    }
}
