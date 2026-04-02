//! # Shannon Core
//!
//! Core engine for Shannon Code - query processing, tool orchestration, and state management.
//!
//! ## Architecture
//!
//! - [`QueryEngine`]: Main orchestrator for streaming query processing
//! - [`ToolRegistry`]: Dynamic tool registration and execution
//! - [`PermissionManager`]: Security and permission validation
//! - [`StateManager`]: Persistent state and session management
//! - [`ClaudeClient`]: Async Claude API client with streaming support

pub mod query_engine;
pub mod tools;
pub mod permissions;
pub mod state;
pub mod api;

// Re-export key types for convenience
pub use query_engine::{QueryEngine, QueryContext, QueryEvent};
pub use tools::{Tool, ToolRegistry, ToolOutput, ToolResult};
pub use permissions::{PermissionManager, Permission, PermissionLevel};
pub use state::{StateManager, SessionState};
pub use api::{
    ClaudeClient, ClaudeClientConfig, MessageStream,
    ContentBlock, ContentDelta, ImageSource, Message, MessageContent,
    MessageRequest, MessageResponse, StreamEvent, ToolDefinition, Usage,
    ApiError,
};

/// Core error types for Shannon
pub mod error {
    pub use crate::api::ApiError;
    pub use crate::tools::ToolError;
    pub use crate::permissions::PermissionError;
    pub use crate::state::StateError;
}

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Common Result type for Shannon operations
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
