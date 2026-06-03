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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_permission_context_default() {
        let ctx = ToolPermissionContext::default();
        assert!(ctx.always_allow_rules.is_empty());
        assert!(ctx.blocked_rules.is_empty());
        assert_eq!(ctx.approval_mode, ApprovalMode::Manual);
    }

    #[test]
    fn tool_use_context_default() {
        let ctx = ToolUseContext::default();
        assert!(ctx.cwd.exists() || ctx.cwd.as_os_str().is_empty());
        assert!(ctx.env.is_empty());
        assert!(ctx.git_info.is_none());
        assert!(ctx.current_branch.is_none());
        assert!(ctx.default_branch.is_none());
    }

    #[test]
    fn command_context_default() {
        let ctx = CommandContext::default();
        assert!(ctx.messages.is_empty());
        assert!(ctx.options.model.is_none());
        assert!(!ctx.options.stream);
    }

    #[test]
    fn approval_mode_serde_roundtrip() {
        for mode in [
            ApprovalMode::Auto,
            ApprovalMode::Manual,
            ApprovalMode::Smart,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: ApprovalMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
    }

    #[test]
    fn message_content_text_serde() {
        let msg = Message {
            role: MessageRole::User,
            content: MessageContent::Text("hello".into()),
            is_meta: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(msg.role, back.role);
        assert!(!back.is_meta);
    }

    #[test]
    fn git_info_serde_roundtrip() {
        let info = GitInfo {
            is_repo: true,
            branch: Some("dev".into()),
            default_branch: Some("main".into()),
            head: Some("abc123".into()),
            remote: Some("origin".into()),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: GitInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info.branch, back.branch);
        assert_eq!(info.head, back.head);
    }
}
