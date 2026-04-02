//! Shannon Tools - Claude Code tool implementations
//!
//! This crate provides implementations of Claude Code tools including:
//! - File operations (Read, Write, Edit, Glob)
//! - System operations (Bash commands, Sleep)
//! - Web operations (WebFetch, WebSearch)
//! - Agent operations (Agent spawning, messaging)
//! - Task operations (Todo management, task lists)
//! - Notebook operations (NotebookEdit for Jupyter notebooks)
//! - Worktree operations (EnterWorktree, ExitWorktree for git worktrees)
//! - MCP operations (ReadMcpResource, ListMcpResources for MCP servers)
//! - Skill operations (Skill for user-callable skills)
//! - Cron operations (CronCreate, CronDelete, CronList for scheduling)
//! - Messaging operations (SendMessage for team communication)

pub mod file;
pub mod system;
pub mod web;
pub mod agent;
pub mod task;
pub mod notebook;
pub mod worktree;
pub mod mcp;
pub mod messaging;
pub mod todo;
pub mod skill;
pub mod cron;

// Re-exports for convenience
pub use file::{FileTool, FileOperation};
pub use system::{SystemTool, ShellCommand, SleepTool, BashTool, PowerShellTool};
pub use web::{WebFetchTool, WebSearchTool, WebOperation};
pub use agent::{AgentTool, AgentOperation};
pub use task::{TaskTool, TaskOperation};
pub use notebook::{NotebookEditTool, NotebookEditInput, NotebookEditOutput};
pub use worktree::{WorktreeTool, EnterWorktreeInput, EnterWorktreeOutput, ExitWorktreeInput, ExitWorktreeOutput};
pub use mcp::{McpResourceTool, ReadMcpResourceInput, ReadMcpResourceOutput, ListMcpResourcesInput, ListMcpResourcesOutput};
pub use messaging::{SendMessageTool, SendMessageInput, SendMessageOutput};
pub use todo::{TodoWriteTool, TodoWriteInput, TodoWriteOutput};
pub use skill::{SkillTool, SkillInvokeInput, SkillInvokeOutput};
pub use cron::{CronTool, CronCreateInput, CronCreateOutput, CronDeleteInput, CronDeleteOutput, CronListInput, CronListOutput};

/// Tool execution result
pub type ToolResult<T> = Result<T, ToolError>;

/// Common error type for all tools
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("File operation failed: {0}")]
    FileError(String),

    #[error("System command failed: {0}")]
    SystemError(String),

    #[error("Web request failed: {0}")]
    WebError(String),

    #[error("Agent operation failed: {0}")]
    AgentError(String),

    #[error("Task operation failed: {0}")]
    TaskError(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// Base trait for all tools
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Execute the tool operation
    async fn execute(&self, input: serde_json::Value) -> ToolResult<serde_json::Value>;

    /// Get the tool name
    fn name(&self) -> &str;

    /// Get the tool description
    fn description(&self) -> &str;

    /// Validate input parameters
    fn validate_input(&self, input: &serde_json::Value) -> Result<(), ToolError> {
        // Default implementation - override for custom validation
        Ok(())
    }
}
