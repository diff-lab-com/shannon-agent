//! Core command type definitions
//!
//! Based on Claude Code's command system architecture.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Result type for command execution
pub type CommandResult<T> = Result<T, CommandError>;

/// Error type for command execution
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("Command not found: {0}")]
    NotFound(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),

    #[error("Tool not allowed: {0}")]
    ToolNotAllowed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Where the command was loaded from
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandSource {
    /// Built-in command (part of the core application)
    Builtin,
    /// Loaded from MCP server
    Mcp,
    /// Loaded from plugin
    Plugin,
    /// Loaded from skills directory
    Skills,
    /// Loaded from bundled skills
    Bundled,
    /// Deprecated commands directory
    CommandsDeprecated,
    /// Managed/remote command
    Managed,
}

/// Declares which auth/provider environments a command is available in
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandAvailability {
    /// claude.ai OAuth subscriber
    ClaudeAI,
    /// Console API key user (direct api.anthropic.com)
    Console,
    /// Available in all environments
    All,
}

/// Base properties shared by all command types
#[derive(Debug, Clone)]
pub struct CommandBase {
    /// Command name (e.g., "commit", "review-pr")
    pub name: String,

    /// Alternative names for the command
    pub aliases: Vec<String>,

    /// Human-readable description
    pub description: String,

    /// User-specified description (vs auto-generated)
    pub has_user_specified_description: bool,

    /// Command availability (auth/provider requirements)
    pub availability: Vec<CommandAvailability>,

    /// Where the command was loaded from
    pub source: CommandSource,

    /// Whether the command is currently enabled
    pub is_enabled: bool,

    /// Whether to hide from typeahead/help
    pub is_hidden: bool,

    /// Hint text for command arguments
    pub argument_hint: Option<String>,

    /// Detailed usage scenarios
    pub when_to_use: Option<String>,

    /// Command version
    pub version: Option<String>,

    /// Whether models can invoke this command
    pub disable_model_invocation: bool,

    /// Whether users can invoke by typing /command-name
    pub user_invocable: bool,

    /// Is this a workflow-backed command
    pub is_workflow: bool,

    /// Execute immediately without waiting for stop point
    pub immediate: bool,

    /// Args should be redacted from history
    pub is_sensitive: bool,

    /// User-facing name (may differ from internal name)
    pub user_facing_name: Option<String>,
}

impl CommandBase {
    /// Get the user-visible name
    pub fn get_display_name(&self) -> &str {
        self.user_facing_name.as_ref().unwrap_or(&self.name)
    }

    /// Check if command is available in current environment
    pub fn is_available(&self, availability: CommandAvailability) -> bool {
        self.availability.contains(&availability)
            || self.availability.contains(&CommandAvailability::All)
    }
}

/// Prompt command - generates prompts for AI processing
#[derive(Debug, Clone)]
pub struct PromptCommand {
    pub base: CommandBase,

    /// Progress message to show during execution
    pub progress_message: String,

    /// Estimated content length for token estimation
    pub content_length: usize,

    /// Argument names
    pub arg_names: Vec<String>,

    /// Tools allowed during execution
    pub allowed_tools: Vec<String>,

    /// Specific model to use
    pub model: Option<String>,

    /// Hooks to register when invoked
    pub hooks: HashMap<String, serde_json::Value>,

    /// Execution context: inline (default) or fork (sub-agent)
    pub context: ExecutionContext,

    /// Agent type when forked
    pub agent: Option<String>,

    /// Glob patterns for file paths this applies to
    pub paths: Vec<String>,

    /// Prompt template for AI processing (supports {args} placeholder)
    pub prompt_template: Option<String>,
}

/// Execution context for commands
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionContext {
    /// Expand into current conversation
    Inline,
    /// Run as sub-agent with separate context
    Fork,
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self::Inline
    }
}

/// Local command - executed without AI
#[derive(Debug, Clone)]
pub struct LocalCommand {
    pub base: CommandBase,

    /// Supports non-interactive mode
    pub supports_non_interactive: bool,
}

/// Local JSX command - rich UI component
#[derive(Debug, Clone)]
pub struct LocalJSXCommand {
    pub base: CommandBase,

    /// Progress message
    pub progress_message: String,
}

/// Unified command type
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
    Prompt(PromptCommand),
    Local(LocalCommand),
    LocalJSX(LocalJSXCommand),
}

impl Command {
    /// Get the command base properties
    pub fn base(&self) -> &CommandBase {
        match self {
            Command::Prompt(cmd) => &cmd.base,
            Command::Local(cmd) => &cmd.base,
            Command::LocalJSX(cmd) => &cmd.base,
        }
    }

    /// Get the command name
    pub fn name(&self) -> &str {
        &self.base().name
    }

    /// Get the display name
    pub fn display_name(&self) -> &str {
        self.base().get_display_name()
    }

    /// Get all aliases
    pub fn aliases(&self) -> &[String] {
        &self.base().aliases
    }

    /// Get description
    pub fn description(&self) -> &str {
        &self.base().description
    }

    /// Check if command is enabled
    pub fn is_enabled(&self) -> bool {
        self.base().is_enabled
    }

    /// Check if command should be hidden
    pub fn is_hidden(&self) -> bool {
        self.base().is_hidden
    }

    /// Get argument hint
    pub fn argument_hint(&self) -> Option<&str> {
        self.base().argument_hint.as_deref()
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/{}", self.name())?;
        if let Some(hint) = self.argument_hint() {
            write!(f, " {hint}")?;
        }
        Ok(())
    }
}

/// Execution result for commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionResult {
    /// Text result
    Text { value: String },

    /// Skip (no output)
    Skip,

    /// Compaction result (for history compaction)
    Compact {
        display_text: Option<String>,
        stats: CompactionStats,
    },

    /// Error
    Error { message: String },
}

/// Statistics for history compaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionStats {
    pub before_tokens: usize,
    pub after_tokens: usize,
    pub messages_removed: usize,
}

/// Trait for command execution
///
/// This trait is reserved for plugin commands that will be loaded dynamically.
/// Built-in commands use `CommandBase` instead, but external plugins will
/// implement `Executable` to integrate with the command dispatch system.
#[allow(dead_code)]
#[async_trait]
pub trait Executable: Send + Sync {
    /// Execute the command with given arguments and context
    async fn execute(
        &self,
        args: &str,
        context: &CommandContext,
    ) -> CommandResult<ExecutionResult>;

    /// Get the prompt for prompt commands
    async fn get_prompt(
        &self,
        args: &str,
        context: &CommandContext,
    ) -> CommandResult<String> {
        let _ = (args, context);
        Err(CommandError::ExecutionError(
            "get_prompt not implemented for this command".to_string(),
        ))
    }
}

/// Command context - provides execution environment
#[derive(Debug, Clone)]
pub struct CommandContext {
    /// Current working directory
    pub cwd: std::path::PathBuf,

    /// Environment variables
    pub env: HashMap<String, String>,

    /// User info
    pub user_info: UserInfo,

    /// Tool permission context
    pub tool_permissions: ToolPermissions,

    /// Session state
    pub session_state: HashMap<String, serde_json::Value>,
}

/// User information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub username: String,
    pub safe_user: String,
    pub user_type: String,
}

/// Tool permissions for command execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPermissions {
    /// Always allowed tool patterns
    pub always_allow: Vec<String>,

    /// Blocked tool patterns
    pub blocked: Vec<String>,
}

impl Default for CommandContext {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_default(),
            env: HashMap::new(),
            user_info: UserInfo {
                username: whoami::username(),
                safe_user: whoami::username(),
                user_type: "user".to_string(),
            },
            tool_permissions: ToolPermissions {
                always_allow: vec![],
                blocked: vec![],
            },
            session_state: HashMap::new(),
        }
    }
}

// External crate for username
mod whoami {
    pub fn username() -> String {
        std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "user".to_string())
    }
}
