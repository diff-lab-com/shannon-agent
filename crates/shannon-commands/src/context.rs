//! Context types for command execution

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Tool use context - provides runtime context for command execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseContext {
    /// Current working directory
    pub cwd: PathBuf,

    /// Environment variables
    pub env: HashMap<String, String>,

    /// Git repository info
    pub git_info: Option<GitInfo>,

    /// Current branch
    pub current_branch: Option<String>,

    /// Default branch (main/master)
    pub default_branch: Option<String>,

    /// App state snapshot
    pub app_state: AppStateSnapshot,
}

/// Git repository information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    /// Is this a git repository
    pub is_repo: bool,

    /// Current branch
    pub branch: Option<String>,

    /// Default branch
    pub default_branch: Option<String>,

    /// Current commit
    pub head: Option<String>,

    /// Remote origin
    pub remote: Option<String>,
}

/// Application state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct AppStateSnapshot {
    /// Tool permission context
    pub tool_permissions: ToolPermissionContext,

    /// Feature flags
    pub feature_flags: HashMap<String, bool>,

    /// User settings
    pub settings: HashMap<String, serde_json::Value>,
}

/// Tool permission context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPermissionContext {
    /// Always allowed rules (tool patterns)
    pub always_allow_rules: HashMap<String, Vec<String>>,

    /// Blocked rules
    pub blocked_rules: Vec<String>,

    /// Approval mode
    pub approval_mode: ApprovalMode,
}

/// Tool approval mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalMode {
    /// Auto-approve all tools
    Auto,

    /// Manual approval for each tool
    Manual,

    /// Smart approval based on context
    Smart,
}

/// Command execution context
#[derive(Debug, Clone)]
pub struct CommandContext {
    /// Working directory
    pub cwd: PathBuf,

    /// Environment
    pub env: HashMap<String, String>,

    /// Tool use context
    pub tool_context: ToolUseContext,

    /// Messages in current conversation
    pub messages: Vec<Message>,

    /// Execution options
    pub options: ExecutionOptions,
}

/// Message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message role
    pub role: MessageRole,

    /// Message content
    pub content: MessageContent,

    /// Whether this is a meta message (hidden from user)
    pub is_meta: bool,
}

/// Message role
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

/// Message content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    /// Text content
    Text(String),

    /// Tool use
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Tool result
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

/// Options for command execution
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct ExecutionOptions {
    /// Model to use
    pub model: Option<String>,

    /// Max tokens
    pub max_tokens: Option<usize>,

    /// Temperature
    pub temperature: Option<f32>,

    /// Stream response
    pub stream: bool,
}


impl Default for ToolPermissionContext {
    fn default() -> Self {
        Self {
            always_allow_rules: HashMap::new(),
            blocked_rules: vec![],
            approval_mode: ApprovalMode::Manual,
        }
    }
}


impl Default for ToolUseContext {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_default(),
            env: HashMap::new(),
            git_info: None,
            current_branch: None,
            default_branch: None,
            app_state: Default::default(),
        }
    }
}

impl Default for CommandContext {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_default(),
            env: HashMap::new(),
            tool_context: Default::default(),
            messages: vec![],
            options: Default::default(),
        }
    }
}
