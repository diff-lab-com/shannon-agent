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
//! - [`SettingsManager`]: Configuration management for user and project settings
//! - [`AutoUpdater`]: Automatic update checking via GitHub Releases
//! - [`PluginManager`]: Plugin discovery, loading, and lifecycle management
//! - [`MemoryStore`]: Persistent memory storage with search and cleanup
//! - [`AutoDreamService`]: Automatic memory extraction from conversations
//! - [`DiagnosticTracker`]: Error tracking, pattern analysis, and diagnostic event management

pub mod query_engine;
pub mod tools;
pub mod permissions;
pub mod state;
pub mod api;
pub mod claude_md;
pub mod settings;
pub mod hooks;
pub mod plugins;
pub mod updater;
pub mod suggestions;
pub mod memory;
pub mod diagnostics;
pub mod notifier;
pub mod tips;

// Re-export key types for convenience
pub use query_engine::{QueryEngine, QueryContext, QueryEvent};
pub use tools::{Tool, ToolInfo, ToolRegistry, ToolOutput, ToolResult};
pub use permissions::{PermissionManager, Permission, PermissionLevel};
pub use state::{
    StateManager, SessionState, SessionData, SessionInfo, SessionPersistMetadata,
};
pub use api::{
    ClaudeClient, ClaudeClientConfig, MessageStream,
    ContentBlock, ContentDelta, ImageSource, Message, MessageContent,
    MessageRequest, MessageResponse, StreamEvent, ToolDefinition, Usage,
    ApiError,
};
pub use settings::{Settings, SettingsManager, SettingsError};
pub use hooks::{HookManager, HookEvent, HookResult, HookDecision, HookEventType, HookError};
pub use plugins::{PluginManager, PluginManifest, PluginState, PluginError, Plugin, PluginStateFile};
pub use updater::{AutoUpdater, UpdateStatus, UpdaterConfig, ReleaseInfo, UpdateError};
pub use memory::{MemoryStore, MemoryEntry, MemoryCategory, AutoDreamService, MemoryError};
pub use suggestions::{
    Suggestion, SuggestionCategory, SuggestionContext, SuggestionEngine, SuggestionRule,
};
pub use diagnostics::{
    DiagnosticTracker, DiagnosticEvent, DiagnosticLevel, DiagnosticCategory,
    ErrorPattern, DiagnosticSummary,
};
pub use notifier::{
    Notification, NotificationLevel, Notifier, NotificationHandler,
    LogNotifier, FileNotifier, CallbackNotifier, NotifierError,
};
pub use tips::{
    Tip, TipCategory, TipCondition, TipManager, TipContext, TipError,
};

/// Core error types for Shannon
pub mod error {
    pub use crate::api::ApiError;
    pub use crate::tools::ToolError;
    pub use crate::permissions::PermissionError;
    pub use crate::state::StateError;
    pub use crate::settings::SettingsError;
    pub use crate::hooks::HookError;
    pub use crate::plugins::PluginError;
    pub use crate::updater::UpdateError;
    pub use crate::memory::MemoryError;
    pub use crate::notifier::NotifierError;
    pub use crate::tips::TipError;
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
