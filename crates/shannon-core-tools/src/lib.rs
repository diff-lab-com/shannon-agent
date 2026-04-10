//! # Shannon Core Tools
//!
//! Tool management, execution, and permission system for the Shannon platform.
//!
//! This crate provides:
//! - Tool registration and execution via [`tools::ToolRegistry`]
//! - Permission-based tool access control
//! - Tool execution with progress tracking via [`tool_execution::ToolExecutionService`]
//! - Streaming tool executor for concurrent operations
//! - Tool use summary generation
//! - Permission pattern classification
//!
//! ## Example
//!
//! ```rust,ignore
//! use shannon_core_tools::{tools::ToolRegistry, tool_execution::ToolExecutionService};
//! use shannon_core_base::permissions::PermissionManager;
//! use std::sync::Arc;
//!
//! let registry = Arc::new(ToolRegistry::new());
//! let permissions = Arc::new(PermissionManager::new());
//! let service = ToolExecutionService::new(registry, permissions);
//! ```

pub mod permission_classifier;
pub mod streaming_tool_executor;
pub mod tool_execution;
pub mod tool_hooks;
pub mod tool_use_summary;
pub mod tools;

// Re-exports for convenience
pub use permission_classifier::{
    PermissionClassifier, PermissionRule, RuleDecision, RuleSource,
};
pub use streaming_tool_executor::{
    ExecutorError, StreamingToolExecutor, ToolStatus, TrackedTool,
};
pub use tool_execution::{
    ToolExecutionError, ToolExecutionResult, ToolExecutionService, ToolProgress,
    ToolProgressStatus,
};
pub use tool_hooks::{
    PostLoggingToolHook, PermissionToolHook, StopOnDenyHook, ToolHook,
    ToolHookChain, ToolHookContext, ToolHookDecision, ToolHookError,
    ToolHookResult,
};
pub use tool_use_summary::{
    AiSummaryConfig, EnhancedToolUseSummary, SummarySource, ToolUseInfo,
    ToolUseSummary,
};
pub use tools::ToolRegistry;
