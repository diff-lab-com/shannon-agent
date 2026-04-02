//! Shannon Tools - Claude Code tool implementations
//!
//! This crate provides implementations of Claude Code tools including:
//! - File operations (Read, Write, Edit, Glob)
//! - System operations (Bash commands)
//! - Web operations (WebFetch, WebSearch)
//! - Agent operations (Agent spawning, messaging)
//! - Task operations (Todo management, task lists)

pub mod file;
pub mod system;
pub mod web;
pub mod agent;
pub mod task;

// Re-exports for convenience
pub use file::{FileTool, FileOperation};
pub use system::{SystemTool, ShellCommand};
pub use web::{WebFetchTool, WebSearchTool, WebOperation};
pub use agent::{AgentTool, AgentOperation};
pub use task::{TaskTool, TaskOperation};

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
