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
//! - [`LlmClient`]: Async LLM API client with multi-provider and streaming support
//! - [`SettingsManager`]: Configuration management for user and project settings
//! - [`AutoUpdater`]: Automatic update checking via GitHub Releases
//! - [`PluginManager`]: Plugin discovery, loading, and lifecycle management
//! - [`MemoryStore`]: Persistent memory storage with search and cleanup
//! - [`AutoDreamService`]: Automatic memory extraction from conversations
//! - [`DiagnosticTracker`]: Error tracking, pattern analysis, and diagnostic event management
//! - [`VoiceModeService`]: Voice input/output management and keyword spotting
//! - [`MagicDocsService`]: Automatic documentation generation from source paths
//! - [`SessionHistoryManager`]: Session history listing, searching, archiving, and resumption
//! - [`TranscriptStore`]: Persistent conversation transcript storage and search
//! - [`ActivityManager`]: Long-running task activity tracking with progress
//! - [`Housekeeper`]: Periodic background cleanup tasks

pub mod query_engine;
pub mod tools;
pub mod permissions;
pub mod state;
pub mod api;
pub mod project_memory;
pub mod settings;
pub mod hooks;
pub mod plugins;
pub mod updater;
pub mod suggestions;
pub mod memory;
pub mod extract_memories;
pub mod diagnostics;
pub mod analytics;
pub mod notifier;
pub mod tips;
pub mod rate_limit;
pub mod away_summary;
pub mod tool_use_summary;
pub mod token_estimation;
pub mod prevent_sleep;
pub mod policy_limits;
pub mod rate_limit_messages;
pub mod ai_limits;
pub mod vcr;
pub mod internal_logging;
pub mod git_operation_tracking;
pub mod voice_mode;
pub mod magic_docs;
pub mod oauth;
pub mod settings_sync;
pub mod remote_settings;
pub mod mcp_advanced;
pub mod api_services;
pub mod bridge_service;
pub mod session_history;
pub mod compact;
pub mod streaming_tool_executor;
pub mod tool_execution;
pub mod tool_hooks;
pub mod doctor;
pub mod permission_classifier;
pub mod team_memory_sync;
pub mod auto_dream_consolidation;
pub mod mcp_server_approval;
pub mod session_transcript;
pub mod activity_manager;
pub mod housekeeping;
pub mod credential_manager;
pub mod billing;
pub mod enhanced_suggestions;
pub mod ui_adapter;

// Re-export key types for convenience
pub use query_engine::{QueryEngine, QueryContext, QueryEvent};
pub use tools::{Tool, ToolInfo, ToolRegistry, ToolOutput, ToolResult};
pub use permissions::{PermissionManager, Permission, PermissionLevel};
pub use state::{
    StateManager, SessionState, SessionData, SessionInfo, SessionPersistMetadata,
};
pub use api::{
    LlmClient, LlmClientConfig, LlmProvider, MessageStream,
    ContentBlock, ContentDelta, ImageSource, Message, MessageContent,
    MessageRequest, MessageResponse, StreamEvent, ToolDefinition, Usage,
    ApiError,
    // Backward-compatible aliases
    ClaudeClient, ClaudeClientConfig,
};
pub use settings::{Settings, SettingsManager, SettingsError};
pub use hooks::{HookManager, HookEvent, HookResult, HookDecision, HookEventType, HookError};
pub use plugins::{PluginManager, PluginManifest, PluginState, PluginError, Plugin, PluginStateFile};
pub use updater::{AutoUpdater, UpdateStatus, UpdaterConfig, ReleaseInfo, UpdateError};
pub use memory::{
    MemoryStore, MemoryEntry, MemoryCategory, AutoDreamService, MemoryError,
    MemoryType, SessionMemoryConfig, MemoryConsolidator, ConsolidationResult,
};
pub use extract_memories::{
    MemoryExtractor, ExtractionConfig, ExtractionResult, ExtractionCategory,
    ExtractionError, MessageSummary, ExtractedMemory,
};
pub use suggestions::{
    Suggestion, SuggestionCategory, SuggestionContext, SuggestionEngine, SuggestionRule,
};
pub use diagnostics::{
    DiagnosticTracker, DiagnosticEvent, DiagnosticLevel, DiagnosticCategory,
    ErrorPattern, DiagnosticSummary,
};
pub use analytics::{
    AnalyticsStore, AnalyticsEvent, AnalyticsEventType, AnalyticsError, AnalyticsSummary,
    ToolStats, SessionStats, DailyStats,
};
pub use notifier::{
    Notification, NotificationLevel, Notifier, NotificationHandler,
    LogNotifier, FileNotifier, CallbackNotifier, NotifierError,
};
pub use tips::{
    Tip, TipCategory, TipCondition, TipManager, TipContext, TipError,
};
pub use rate_limit::{
    RateLimiter, RateLimitConfig, RateLimitResult, TokenBucket, ExponentialBackoff,
};
pub use policy_limits::{PolicyLimits, PolicyLimitsManager, PolicyCheckResult, PolicyError};
pub use rate_limit_messages::RateLimitMessageBuilder;
pub use ai_limits::{AiLimitType, AiUsageRecord, AiLimitsTracker, LimitStatus};
pub use vcr::{Vcr, VcrConfig, VcrRecording, VcrError};
pub use internal_logging::{InternalLogEntry, InternalLogLevel, InternalLogger};
pub use git_operation_tracking::{GitOperation, GitOperationTracker};
pub use voice_mode::{
    VoiceModeService, VoiceConfig, VoiceCommand, VoiceCommandResult, VoiceStatus,
    VoiceSession, TranscriptionResult, KeywordSpotter, VoiceError,
};
pub use magic_docs::{
    MagicDocsService, DocSection, DocGenerationRequest, DocOutput, DocOutputFormat,
    DocLevel, DocMetadata, MagicDocsError,
};
pub use oauth::{OAuthService, OAuthClient, OAuthToken, OAuthError, TokenEncryption};
pub use settings_sync::{
    SettingsSyncService, SyncRecord, SyncStatus, DeviceRegistry, DeviceInfo, SyncError,
};
pub use remote_settings::{
    RemoteSettingsProvider, RemoteManagedSettings, SettingOverride, SettingSource, RemoteSettingsError,
};
pub use mcp_advanced::{
    McpChannelManager, McpServerRegistry, ElicitationHandler,
    McpServerConfig, McpChannel, ElicitationRequest,
    TransportType, ChannelStatus, ChannelCapabilities,
    ElicitationStatus, McpAdvancedError,
};
pub use api_services::{
    ApiManager, UsageTracker, ApiRequest, ApiResponse,
    UsageStats, ModelUsage, RateLimitInfo, ApiServiceError,
};
pub use bridge_service::{
    BridgeService, BridgeSession, BridgeConfig, BridgeStatus,
    SessionMessage, MessageDirection, BridgeError,
};
pub use session_history::{
    SessionHistoryManager, SessionHistoryEntry, SessionFilter, ResumeInfo,
    SessionMetadata, SessionSortField, SortOrder, SessionHistoryError,
};
pub use streaming_tool_executor::{StreamingToolExecutor, TrackedTool, ToolStatus};
pub use tool_execution::{ToolExecutionService, ToolExecutionResult, ToolProgress, ToolProgressStatus};
pub use compact::{CompactEngine, CompactConfig, CompactResult, CompactStrategy, MessageGroup, CompactError};
pub use tool_hooks::{
    ToolHookChain, ToolHook, ToolHookResult, ToolHookDecision, ToolHookContext,
    ToolHookError, PermissionToolHook, LoggingToolHook, StopOnDenyHook,
};
pub use permission_classifier::{
    PermissionClassifier, PermissionClassifierError, PermissionRule, PermissionRuleParser,
    ClassificationResult, ClassificationResultBuilder, DangerousPattern,
    RuleDecision, RuleSource, RiskLevel,
};
pub use team_memory_sync::{
    TeamMemorySync, TeamMemoryConfig, TeamMemorySyncError, SyncResult,
    SecretScanner, SecretRule, SecretMatch, TeamMemoryGuard,
};
pub use auto_dream_consolidation::{
    ConsolidationLock, ConsolidationGuard, ConsolidationPrompt, ConsolidationConfig,
    EnhancedConsolidationResult, ConsolidationError, should_consolidate,
};
pub use mcp_server_approval::{
    McpApprovalManager, McpApprovalPolicy, McpServerApprovalRequest,
    McpTransportType, ApprovalDecision, RiskAssessment, McpApprovalError,
};
pub use session_transcript::{
    TranscriptStore, TranscriptEntry, TranscriptRole, TranscriptQuery,
    TranscriptError, ToolCallRecord,
    SessionTranscriptStats, GlobalTranscriptStats,
};
pub use activity_manager::{
    ActivityManager, Activity, ActivityStatus, ActivityError,
};
pub use housekeeping::{
    Housekeeper, HousekeepingTask, HousekeepingConfig, HousekeepingError,
    TaskResult, TempFileCleanupTask, CacheRefreshTask,
    OldSessionPruneTask, LogRotationTask,
};
pub use credential_manager::{
    CredentialManager, Credential, CredentialError, CredentialSummary,
    CredentialFileDescriptor, CredentialFileFormat, PortableCredential,
    PortableCredentialBundle, ImportResult,
};
pub use billing::{
    BillingManager, BillingPeriod, UsageRecord, BillingConfig,
    BillingError, ModelUsageSummary, BudgetAlert, BudgetAlertType, DailyUsage,
};

pub use enhanced_suggestions::{
    ContextSuggestionEngine, ContextualSuggestion, SuggestionTrigger,
    SuggestionContext as EnhancedSuggestionContext, SuggestionError,
};
pub use ui_adapter::{
    UiAdapter, UiError, UiResult,
    DefaultUiAdapter, NullUiAdapter,
    DisplayMessage, MessageSeverity, UserChoice,
};
// Backward-compatible re-exports for the claude_md -> project_memory rename
pub use project_memory::{
    ProjectMemoryConfig as ClaudeMdConfig,
    ProjectMemoryMetadata as ClaudeMdMetadata,
    ProjectMemoryManager as ClaudeMdManager,
    ProjectMemorySearchResult as ClaudeMdSearchResult,
    ProjectMemoryError as ClaudeMdError,
    MemorySource,
    MergedMemory,
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
    pub use crate::extract_memories::ExtractionError;
    pub use crate::notifier::NotifierError;
    pub use crate::tips::TipError;
    pub use crate::analytics::AnalyticsError;
    pub use crate::policy_limits::PolicyError;
    pub use crate::vcr::VcrError;
    pub use crate::voice_mode::VoiceError;
    pub use crate::magic_docs::MagicDocsError;
    pub use crate::oauth::OAuthError;
    pub use crate::settings_sync::SyncError;
    pub use crate::remote_settings::RemoteSettingsError;
    pub use crate::mcp_advanced::McpAdvancedError;
    pub use crate::api_services::ApiServiceError;
    pub use crate::bridge_service::BridgeError;
    pub use crate::session_history::SessionHistoryError;
    pub use crate::streaming_tool_executor::ExecutorError;
    pub use crate::tool_execution::ToolExecutionError;
    pub use crate::compact::CompactError;
    pub use crate::doctor::DoctorError;
    pub use crate::tool_hooks::ToolHookError;
    pub use crate::team_memory_sync::TeamMemorySyncError;
    pub use crate::permission_classifier::PermissionClassifierError;
    pub use crate::auto_dream_consolidation::ConsolidationError;
    pub use crate::mcp_server_approval::McpApprovalError;
    pub use crate::session_transcript::TranscriptError;
    pub use crate::activity_manager::ActivityError;
    pub use crate::housekeeping::HousekeepingError;
    pub use crate::enhanced_suggestions::SuggestionError;
    pub use crate::credential_manager::CredentialError;
    pub use crate::billing::BillingError;
    pub use crate::project_memory::ProjectMemoryError;
    pub use crate::ui_adapter::UiError;
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
