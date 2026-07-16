//! # Shannon Core
//!
//! Clippy lint suppressions: rustfmt expands `if let A && B` into nested blocks
//! that trigger collapsible_if/match. derivable_impls, manual_is_multiple_of,
//! and manual_checked_div are style preferences from newer clippy versions.
#![allow(
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::derivable_impls
)]
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
//! - [`MemoryStore`]: Persistent memory storage with search and cleanup
//! - [`AutoDreamService`]: Automatic memory extraction from conversations
//! - [`DiagnosticTracker`]: Error tracking, pattern analysis, and diagnostic event management
//! - [`VoiceModeService`]: Voice input/output management and keyword spotting
//! - [`MagicDocsService`]: Automatic documentation generation from source paths
//! - [`SessionHistoryManager`]: Session history listing, searching, archiving, and resumption
//! - [`TranscriptStore`]: Persistent conversation transcript storage and search
//! - [`ActivityManager`]: Long-running task activity tracking with progress
//! - [`Housekeeper`]: Periodic background cleanup tasks

// Warn on .unwrap() in production code so new instances are caught by CI.
// Existing instances will show as warnings — fix incrementally.
#![warn(clippy::unwrap_used)]

// Initialize i18n translations from workspace locales directory
rust_i18n::i18n!("../../locales", fallback = "en");

pub mod ai_limits;
pub mod analytics;
pub mod api_services;
pub mod away_summary;
pub mod bridge_service;
pub mod checkpoint;
pub mod config_migration;
pub mod diagnostics;
pub mod extract_memories;
pub mod git_operation_tracking;
pub mod internal_logging;
pub mod magic_docs;
pub mod mcp_advanced;
pub mod mcp_tool_adapter;
pub mod memory;
pub mod model_registry;
pub mod notifier;
pub mod oauth;
pub mod output_format;
pub mod policy_limits;
pub mod prevent_sleep;
pub mod progressive_loader;
pub mod project_instructions;
pub mod project_memory;
pub mod provider_resolver;
pub mod query_engine;
pub mod rate_limit;
pub mod rate_limit_messages;
pub mod remote_settings;
pub mod session_history;
pub mod settings;
pub mod settings_sync;
pub mod smart_context;
pub mod suggestions;
pub mod tips;
pub mod token_estimation;
pub mod tool_cache;
pub mod tool_execution;
pub mod tool_orchestration;
pub mod tool_use_summary;
pub mod tools;
pub mod unified_config;
pub mod updater;
pub mod vcr;
pub mod voice_mode;

pub mod activity_manager;
pub mod api_server;
pub mod auto_dream_consolidation;
pub mod billing;
pub mod credential_manager;
pub mod doctor;
pub mod enhanced_suggestions;
pub mod feature_flags;
pub mod housekeeping;
pub mod lsp;
pub mod mcp_server_approval;
pub mod plugin;
pub mod preference_memory;
pub mod recording;
pub mod sandbox;
pub mod scheduled_budget;
pub mod scheduled_retry;
pub mod scheduled_routines;
pub mod scheduled_runs;
pub mod scheduled_task_store;
pub mod scheduled_worktree;
pub mod session_persist;
pub mod session_recovery;
pub mod session_transcript;
pub mod skill_loop;
pub mod team_memory_sync;
pub mod telemetry;
pub mod ui_adapter;
pub mod webhook;

pub mod testing;

pub mod i18n;
pub mod triggered_routines;

// Re-export key types for convenience
pub use ai_limits::{AiLimitType, AiLimitsTracker, AiUsageRecord, LimitStatus};
pub use analytics::{
    AnalyticsError, AnalyticsEvent, AnalyticsEventType, AnalyticsStore, AnalyticsSummary,
    DailyStats, SessionStats, ToolStats,
};
pub use api_services::{
    ApiManager, ApiRequest, ApiResponse, ApiServiceError, ModelUsage, RateLimitInfo, UsageStats,
    UsageTracker,
};
pub use bridge_service::{
    BridgeConfig, BridgeError, BridgeService, BridgeSession, BridgeStatus, MessageDirection,
    SessionMessage,
};
pub use checkpoint::{
    Checkpoint, CheckpointManager, FileChangePreview, RestoreMode, RevertPreview, TurnCheckpoint,
};
pub use config_migration::{SecretBinding, default_secrets_path, persist_secrets};
pub use diagnostics::{
    DiagnosticCategory, DiagnosticEvent, DiagnosticLevel, DiagnosticSummary, DiagnosticTracker,
    ErrorPattern,
};
pub use extract_memories::{
    ExtractedMemory, ExtractionCategory, ExtractionConfig, ExtractionError, ExtractionResult,
    MemoryExtractor, MessageSummary,
};
pub use git_operation_tracking::{GitOperation, GitOperationTracker};
pub use internal_logging::{InternalLogEntry, InternalLogLevel, InternalLogger};
pub use magic_docs::{
    DocGenerationRequest, DocLevel, DocMetadata, DocOutput, DocOutputFormat, DocSection,
    MagicDocsError, MagicDocsService,
};
pub use mcp_advanced::{
    ChannelCapabilities, ChannelStatus, ElicitationHandler, ElicitationRequest, ElicitationStatus,
    McpAdvancedError, McpChannel, McpChannelManager, McpServerConfig, McpServerRegistry,
    TransportType,
};
pub use mcp_tool_adapter::{
    DEFERRED_SCHEMA_THRESHOLD, DeferredSchemaSearchTool, DeferredSchemaStore, McpToolAdapter,
    PromptInfo, discover_tools, discover_tools_http, prepare_deferred_schemas,
};
pub use memory::{
    AutoDreamService, ConsolidationResult, MemoryCategory, MemoryConsolidator, MemoryEntry,
    MemoryError, MemoryStore, MemoryType, SessionMemoryConfig,
};
pub use notifier::{
    CallbackNotifier, FileNotifier, LogNotifier, Notification, NotificationHandler,
    NotificationLevel, Notifier, NotifierError,
};
pub use oauth::{OAuthClient, OAuthError, OAuthService, OAuthToken, TokenEncryption};
pub use output_format::{OutputEvent, StructuredOutputConfig, StructuredOutputError};
pub use policy_limits::{PolicyCheckResult, PolicyError, PolicyLimits, PolicyLimitsManager};
pub use query_engine::{
    QueryContext, QueryEngine, QueryEvent, browser_control_prompt, teammate_instructions,
};
pub use rate_limit::{
    ExponentialBackoff, RateLimitConfig, RateLimitResult, RateLimiter, TokenBucket,
};
pub use rate_limit_messages::RateLimitMessageBuilder;
pub use remote_settings::{
    RemoteManagedSettings, RemoteSettingsError, RemoteSettingsProvider, SettingOverride,
    SettingSource,
};
pub use session_history::{
    ResumeInfo, SessionFilter, SessionHistoryEntry, SessionHistoryError, SessionHistoryManager,
    SessionMetadata, SessionSortField, SortOrder,
};
pub use session_recovery::{
    RecoveryMetadata, SessionLogEntry, SessionRecovery, SessionRecoveryError,
};
pub use settings::{Settings, SettingsError, SettingsManager};
pub use settings_sync::{
    DeviceInfo, DeviceRegistry, SettingsSyncService, SyncError, SyncRecord, SyncStatus,
};
pub use shannon_engine::api::{
    ApiError,
    // Backward-compatible aliases
    ClaudeClient,
    ClaudeClientConfig,
    ContentBlock,
    ContentDelta,
    ImageSource,
    LlmClient,
    LlmClientConfig,
    LlmProvider,
    Message,
    MessageContent,
    MessageRequest,
    MessageResponse,
    MessageStream,
    RetryConfig,
    RetryPolicy,
    StreamEvent,
    ToolDefinition,
    Usage,
};
pub use shannon_engine::compact::{
    CompactConfig, CompactEngine, CompactError, CompactResult, CompactStrategy, MessageGroup,
    RuleBasedSummarizer, Summarizer,
};
pub use shannon_engine::context_pressure::{
    ContextPressureMonitor, PressureLevel, PressureMetrics, PressureRecommendation,
};
pub use shannon_engine::hooks::{
    HookDecision, HookError, HookEvent, HookEventType, HookManager, HookResult,
};
pub use shannon_engine::permissions::{
    ApprovalMode, Permission, PermissionLevel, PermissionManager,
};
pub use shannon_engine::state::{
    SessionData, SessionInfo, SessionPersistMetadata, SessionState, StateManager,
};
pub use shannon_engine::streaming_tool_executor::{StreamingToolExecutor, ToolStatus, TrackedTool};
pub use suggestions::{
    Suggestion, SuggestionCategory, SuggestionContext, SuggestionEngine, SuggestionRule,
};
pub use tips::{Tip, TipCategory, TipCondition, TipContext, TipError, TipManager};
pub use tool_cache::{ToolCacheConfig, ToolResultCache};
pub use tool_execution::{
    ToolExecutionResult, ToolExecutionService, ToolProgress, ToolProgressStatus,
};
pub use tools::{Tool, ToolInfo, ToolOutput, ToolRegistry, ToolResult};
pub use unified_config::{ConfigBuilder, ShannonConfig};
pub use updater::{AutoUpdater, ReleaseInfo, UpdateError, UpdateStatus, UpdaterConfig};
pub use vcr::{Vcr, VcrConfig, VcrError, VcrRecording};
pub use voice_mode::{
    KeywordSpotter, TranscriptionResult, VoiceCommand, VoiceCommandResult, VoiceConfig, VoiceError,
    VoiceModeService, VoiceSession, VoiceStatus,
};

pub use activity_manager::{Activity, ActivityError, ActivityManager, ActivityStatus};
pub use auto_dream_consolidation::{
    ConsolidationConfig, ConsolidationError, ConsolidationGuard, ConsolidationLock,
    ConsolidationPrompt, EnhancedConsolidationResult, should_consolidate,
};
pub use billing::{
    BillingConfig, BillingError, BillingManager, BillingPeriod, BudgetAlert, BudgetAlertType,
    DailyUsage, ModelUsageSummary, UsageRecord,
};
pub use credential_manager::{
    Credential, CredentialError, CredentialFileDescriptor, CredentialFileFormat, CredentialManager,
    CredentialSummary, ImportResult, PortableCredential, PortableCredentialBundle,
};
pub use housekeeping::{
    CacheRefreshTask, Housekeeper, HousekeepingConfig, HousekeepingError, HousekeepingTask,
    LogRotationTask, OldSessionPruneTask, TaskResult, TempFileCleanupTask,
};
pub use lsp::{
    DiscoveredServer, LspClient, LspClientError, LspConfig, LspManager, LspResult, ServerConfig,
    ServerDiscovery, ServerSource,
};
pub use mcp_server_approval::{
    ApprovalDecision, McpApprovalError, McpApprovalManager, McpApprovalPolicy,
    McpServerApprovalRequest, McpTransportType, RiskAssessment,
};
pub use progressive_loader::{ProgressiveLoaderConfig, lines_for_token_budget, truncate_content};
pub use session_transcript::{
    GlobalTranscriptStats, SessionTranscriptStats, ToolCallRecord, TranscriptEntry,
    TranscriptError, TranscriptQuery, TranscriptRole, TranscriptStore,
};
pub use shannon_engine::custom_profiles::{
    CustomProfileDef, CustomProfileError, CustomProfileRegistry,
};
pub use shannon_engine::permission_classifier::{
    ClassificationResult, ClassificationResultBuilder, DangerousPattern, PermissionClassifier,
    PermissionClassifierError, PermissionRule, PermissionRuleParser, RiskLevel, RuleDecision,
    RuleSource,
};
pub use shannon_engine::permission_profile::{PermissionProfile, ProfileRules};
pub use team_memory_sync::{
    SecretMatch, SecretRule, SecretScanner, SyncResult, TeamMemoryConfig, TeamMemoryGuard,
    TeamMemorySync, TeamMemorySyncError,
};
pub use triggered_routines::{
    RoutineExecResult, TriggeredRoutineDef, TriggeredRoutineError, TriggeredRoutineRegistry,
};

pub use enhanced_suggestions::{
    ContextSuggestionEngine, ContextualSuggestion, SuggestionContext as EnhancedSuggestionContext,
    SuggestionError, SuggestionTrigger,
};
pub use ui_adapter::{
    DefaultUiAdapter, DisplayMessage, MessageSeverity, NullUiAdapter, UiAdapter, UiError, UiResult,
    UserChoice,
};
// Backward-compatible re-exports for the claude_md -> project_memory rename
pub use project_memory::{
    MemorySource, MergedMemory, ProjectMemoryConfig as ClaudeMdConfig,
    ProjectMemoryError as ClaudeMdError, ProjectMemoryManager as ClaudeMdManager,
    ProjectMemoryMetadata as ClaudeMdMetadata, ProjectMemorySearchResult as ClaudeMdSearchResult,
    load_memory_index, load_rules,
};
/// Core error types for Shannon
pub mod error {
    pub use crate::activity_manager::ActivityError;
    pub use crate::analytics::AnalyticsError;
    pub use crate::api_services::ApiServiceError;
    pub use crate::auto_dream_consolidation::ConsolidationError;
    pub use crate::billing::BillingError;
    pub use crate::bridge_service::BridgeError;
    pub use crate::credential_manager::CredentialError;
    pub use crate::doctor::DoctorError;
    pub use crate::doctor::{ApiKeyGuard, HomeGuard};
    pub use crate::enhanced_suggestions::SuggestionError;
    pub use crate::extract_memories::ExtractionError;
    pub use crate::housekeeping::HousekeepingError;
    pub use crate::magic_docs::MagicDocsError;
    pub use crate::mcp_advanced::McpAdvancedError;
    pub use crate::mcp_server_approval::McpApprovalError;
    pub use crate::memory::MemoryError;
    pub use crate::notifier::NotifierError;
    pub use crate::oauth::OAuthError;
    pub use crate::policy_limits::PolicyError;
    pub use crate::project_memory::ProjectMemoryError;
    pub use crate::remote_settings::RemoteSettingsError;
    pub use crate::session_history::SessionHistoryError;
    pub use crate::session_recovery::SessionRecoveryError;
    pub use crate::session_transcript::TranscriptError;
    pub use crate::settings::SettingsError;
    pub use crate::settings_sync::SyncError;
    pub use crate::team_memory_sync::TeamMemorySyncError;
    pub use crate::tips::TipError;
    pub use crate::tool_execution::ToolExecutionError;
    pub use crate::tools::ToolError;
    pub use crate::ui_adapter::UiError;
    pub use crate::updater::UpdateError;
    pub use crate::vcr::VcrError;
    pub use crate::voice_mode::VoiceError;
    pub use shannon_engine::api::ApiError;
    pub use shannon_engine::compact::CompactError;
    pub use shannon_engine::hooks::HookError;
    pub use shannon_engine::permission_classifier::PermissionClassifierError;
    pub use shannon_engine::permissions::PermissionError;
    pub use shannon_engine::state::StateError;
    pub use shannon_engine::streaming_tool_executor::ExecutorError;
}

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Common Result type for Shannon operations
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::const_is_empty)]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
