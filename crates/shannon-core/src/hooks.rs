//! # Hooks System
//!
//! A hook system that allows executing shell commands at various lifecycle
//! points, similar to Claude Code's hook mechanism.
//!
//! ## Hook Events
//!
//! Hooks can be triggered on these events:
//! - [`HookEvent::PreToolUse`]: Before a tool is executed
//! - [`HookEvent::PostToolUse`]: After a tool completes
//! - [`HookEvent::SessionStart`]: When a session begins
//! - [`HookEvent::SessionEnd`]: When a session ends
//! - [`HookEvent::Notification`]: When a notification is emitted
//! - [`HookEvent::UserPromptSubmit`]: When the user submits a prompt
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

/// Definition of a single hook command within a hook group
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookDef {
    /// Shell command to execute
    pub command: String,
    /// Timeout in seconds (default: 30)
    #[serde(default = "default_hook_timeout")]
    pub timeout: u64,
    /// Whether to wait for the result before continuing (default: true)
    #[serde(default = "default_hook_blocking")]
    pub blocking: bool,
}

fn default_hook_timeout() -> u64 {
    30
}

fn default_hook_blocking() -> bool {
    true
}

impl HookDef {
    /// Create a new hook definition
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            timeout: default_hook_timeout(),
            blocking: default_hook_blocking(),
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

    /// Get the timeout as a Duration
    pub fn timeout_duration(&self) -> Duration {
        Duration::from_secs(self.timeout)
    }
}

/// A group of hooks with a matcher pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfig {
    /// Matcher pattern: regex or "*" for wildcard (match all)
    pub matcher: String,
    /// List of hook definitions to execute
    pub hooks: Vec<HookDef>,
}

impl HookConfig {
    /// Create a new hook config
    pub fn new(matcher: impl Into<String>) -> Self {
        Self {
            matcher: matcher.into(),
            hooks: Vec::new(),
        }
    }

    /// Add a hook definition
    pub fn with_hook(mut self, hook: HookDef) -> Self {
        self.hooks.push(hook);
        self
    }

    /// Check if this config matches the given subject string.
    ///
    /// Matching supports:
    /// - `"*"` wildcard: matches everything
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

        // Try as anchored regex
        if let Ok(re) = regex::Regex::new(&format!("^{}$", self.matcher)) {
            re.is_match(subject)
        } else {
            // If not valid regex, fall back to substring match
            subject.contains(&self.matcher)
        }
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
                let result = if hook_def.blocking {
                    self.execute_hook(&hook_def.command, hook_def.timeout_duration(), &event_json).await?
                } else {
                    // For non-blocking hooks, spawn and detach
                    self.spawn_hook(&hook_def.command, &event_json)?;
                    continue;
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

            let decision = HookResult::parse_decision(&stdout);

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
            hooks: vec![HookDef {
                command: "echo test".to_string(),
                timeout: 10,
                blocking: false,
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
