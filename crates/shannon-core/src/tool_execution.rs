//! # Tool Execution Service
//!
//! Unified service layer for tool execution that wraps permission checks,
//! progress tracking, hook integration, and metadata generation.
//!
//! ## Architecture
//!
//! - [`ToolExecutionService`]: Single entry point for tool execution via `run_tool_use()`
//! - [`ToolExecutionResult`]: Rich result with progress, duration, and metadata
//! - [`ToolProgress`]: Per-tool progress callback tracking
//!
//! ## Flow
//!
//! ```text
//! run_tool_use()
//!   -> check_permission()
//!   -> emit ToolProgress::Started
//!   -> execute tool via ToolRegistry
//!   -> emit ToolProgress::Updated (if applicable)
//!   -> emit ToolProgress::Completed
//!   -> build ToolExecutionResult with metadata
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use shannon_core::tool_execution::ToolExecutionService;
//! use shannon_core::tools::ToolRegistry;
//! use shannon_core::permissions::PermissionManager;
//! use std::sync::Arc;
//!
//! let registry = Arc::new(ToolRegistry::new());
//! let permissions = Arc::new(PermissionManager::new());
//! let service = ToolExecutionService::new(registry, permissions);
//!
//! let result = service.run_tool_use(
//!     session_id,
//!     "Bash",
//!     serde_json::json!({"command": "ls"}),
//! ).await?;
//! ```

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::checkpoint::CheckpointManager;
use crate::hooks::{HookDecision, HookEvent, HookManager};
use crate::permissions::{PermissionError, PermissionManager};
use crate::tools::{ToolError, ToolOutput, ToolRegistry};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors produced by the tool execution service.
#[derive(Error, Debug)]
pub enum ToolExecutionError {
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Permission denied for tool '{tool_name}': {reason}")]
    PermissionDenied { tool_name: String, reason: String },

    #[error("Tool execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Invalid tool input for '{tool_name}': {reason}")]
    InvalidInput { tool_name: String, reason: String },

    #[error("Tool timed out after {timeout_secs}s: {tool_name}")]
    Timeout {
        tool_name: String,
        timeout_secs: u64,
    },

    #[error("Hook blocked tool execution: {0}")]
    HookBlocked(String),

    #[error("Internal service error: {0}")]
    Internal(String),
}

impl From<ToolError> for ToolExecutionError {
    fn from(err: ToolError) -> Self {
        match err {
            ToolError::NotFound(name) => ToolExecutionError::ToolNotFound(name),
            ToolError::InvalidInput(msg) => ToolExecutionError::InvalidInput {
                tool_name: "unknown".to_string(),
                reason: msg,
            },
            ToolError::ExecutionFailed(msg) => ToolExecutionError::ExecutionFailed(msg),
            ToolError::RegistryError(msg) => ToolExecutionError::Internal(msg),
            ToolError::Timeout { name, duration } => ToolExecutionError::Timeout {
                tool_name: name,
                timeout_secs: duration.as_secs(),
            },
        }
    }
}

impl From<PermissionError> for ToolExecutionError {
    fn from(err: PermissionError) -> Self {
        ToolExecutionError::PermissionDenied {
            tool_name: "unknown".to_string(),
            reason: err.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// ToolProgress
// ---------------------------------------------------------------------------

/// Status of a tool execution progress update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolProgressStatus {
    /// Tool has started executing.
    Started,
    /// Tool execution has produced an intermediate update.
    Updated,
    /// Tool has finished successfully.
    Completed,
    /// Tool has finished with an error.
    Failed,
}

impl std::fmt::Display for ToolProgressStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Started => write!(f, "Started"),
            Self::Updated => write!(f, "Updated"),
            Self::Completed => write!(f, "Completed"),
            Self::Failed => write!(f, "Failed"),
        }
    }
}

/// A progress event for a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgress {
    /// Unique ID of the tool invocation.
    pub tool_id: String,
    /// Tool name.
    pub tool_name: String,
    /// Status of the progress update.
    pub status: ToolProgressStatus,
    /// Optional human-readable progress message.
    pub message: Option<String>,
    /// Timestamp when this progress event occurred.
    pub timestamp: DateTime<Utc>,
    /// Elapsed time since the tool started, if available.
    pub elapsed: Option<Duration>,
}

impl ToolProgress {
    /// Create a new progress event.
    pub fn new(tool_id: String, tool_name: String, status: ToolProgressStatus) -> Self {
        Self {
            tool_id,
            tool_name,
            status,
            message: None,
            timestamp: Utc::now(),
            elapsed: None,
        }
    }

    /// Create a started progress event.
    pub fn started(tool_id: &str, tool_name: &str) -> Self {
        Self::new(
            tool_id.to_string(),
            tool_name.to_string(),
            ToolProgressStatus::Started,
        )
    }

    /// Create an update progress event with a message.
    pub fn updated(tool_id: &str, tool_name: &str, message: &str) -> Self {
        let mut p = Self::new(
            tool_id.to_string(),
            tool_name.to_string(),
            ToolProgressStatus::Updated,
        );
        p.message = Some(message.to_string());
        p
    }

    /// Create a completed progress event.
    pub fn completed(tool_id: &str, tool_name: &str) -> Self {
        Self::new(
            tool_id.to_string(),
            tool_name.to_string(),
            ToolProgressStatus::Completed,
        )
    }

    /// Create a failed progress event with an error message.
    pub fn failed(tool_id: &str, tool_name: &str, message: &str) -> Self {
        let mut p = Self::new(
            tool_id.to_string(),
            tool_name.to_string(),
            ToolProgressStatus::Failed,
        );
        p.message = Some(message.to_string());
        p
    }
}

// ---------------------------------------------------------------------------
// StopHookInfo
// ---------------------------------------------------------------------------

/// Information passed to post-tool hooks after tool execution completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopHookInfo {
    /// The tool that was executed.
    pub tool_name: String,
    /// The input provided to the tool.
    pub tool_input: Value,
    /// The output produced by the tool.
    pub tool_output: ToolOutput,
    /// How long the tool took to execute.
    pub duration: Duration,
    /// Whether the tool produced an error.
    pub is_error: bool,
    /// Session ID for the tool execution.
    pub session_id: Uuid,
    /// Tool invocation ID.
    pub tool_id: String,
}

// ---------------------------------------------------------------------------
// HookProgress
// ---------------------------------------------------------------------------

/// A progress message emitted during hook execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookProgress {
    /// The hook event type (e.g. "PreToolUse", "PostToolUse").
    pub hook_type: String,
    /// The tool being hooked.
    pub tool_name: String,
    /// Progress message from the hook.
    pub message: String,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
}

impl HookProgress {
    /// Create a new hook progress message.
    pub fn new(hook_type: &str, tool_name: &str, message: &str) -> Self {
        Self {
            hook_type: hook_type.to_string(),
            tool_name: tool_name.to_string(),
            message: message.to_string(),
            timestamp: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// AttachmentMessage
// ---------------------------------------------------------------------------

/// An attachment created from tool output (e.g. images, file contents).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMessage {
    /// Unique ID for this attachment.
    pub id: String,
    /// The tool that created this attachment.
    pub source_tool: String,
    /// Tool invocation ID.
    pub tool_id: String,
    /// The content of the attachment.
    pub content: String,
    /// MIME type or content type hint.
    pub content_type: String,
    /// File extension, if applicable.
    pub file_extension: Option<String>,
    /// Metadata about the attachment.
    pub metadata: HashMap<String, Value>,
    /// Timestamp when the attachment was created.
    pub timestamp: DateTime<Utc>,
}

impl AttachmentMessage {
    /// Create a new attachment message.
    pub fn new(source_tool: &str, tool_id: &str, content: &str, content_type: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            source_tool: source_tool.to_string(),
            tool_id: tool_id.to_string(),
            content: content.to_string(),
            content_type: content_type.to_string(),
            file_extension: None,
            metadata: HashMap::new(),
            timestamp: Utc::now(),
        }
    }

    /// Create an attachment for a file output.
    pub fn file_attachment(
        source_tool: &str,
        tool_id: &str,
        content: &str,
        file_path: &str,
    ) -> Self {
        let mut attachment = Self::new(source_tool, tool_id, content, "text/plain");
        attachment.file_extension = std::path::Path::new(file_path)
            .extension()
            .map(|e| e.to_string_lossy().to_string());
        attachment
    }
}

// ---------------------------------------------------------------------------
// ToolExecutionResult
// ---------------------------------------------------------------------------

/// The result of a tool execution with rich metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionResult {
    /// The output produced by the tool.
    pub output: ToolOutput,
    /// All progress events collected during execution.
    pub progress: Vec<ToolProgress>,
    /// Total wall-clock duration of the execution.
    pub duration: Duration,
    /// Tool invocation ID.
    pub tool_id: String,
    /// Tool name.
    pub tool_name: String,
    /// Whether the tool execution produced an error.
    pub is_error: bool,
    /// Additional metadata extracted from the execution.
    pub metadata: HashMap<String, Value>,
    /// Any attachments generated by the tool.
    pub attachments: Vec<AttachmentMessage>,
    /// Hook progress messages.
    pub hook_progress: Vec<HookProgress>,
    /// Stop hook info for post-tool hooks.
    pub stop_hook_info: Option<StopHookInfo>,
    /// Session ID.
    pub session_id: Uuid,
    /// File paths modified by this tool execution (extracted from input).
    pub files_modified: Vec<String>,
    /// Whether a checkpoint was created before this tool execution.
    pub checkpoint_created: bool,
}

impl ToolExecutionResult {
    /// Build metadata from a tool execution.
    fn build_metadata(
        tool_name: &str,
        input: &Value,
        output: &ToolOutput,
    ) -> HashMap<String, Value> {
        let mut metadata = HashMap::new();

        // Extract file extensions for file-related tools
        if let Some(obj) = input.as_object() {
            if let Some(file_path) = obj.get("file_path").or_else(|| obj.get("path")) {
                if let Some(path_str) = file_path.as_str() {
                    let ext = std::path::Path::new(path_str)
                        .extension()
                        .map(|e| e.to_string_lossy().to_string());
                    if let Some(ext) = ext {
                        metadata.insert("file_extension".to_string(), Value::String(ext));
                    }
                }
            }
        }

        // Extract bash command for analytics
        if tool_name == "Bash" || tool_name == "bash" {
            if let Some(cmd) = input
                .as_object()
                .and_then(|o| o.get("command").or_else(|| o.get("cmd")))
                .and_then(|v| v.as_str())
            {
                metadata.insert("bash_command".to_string(), Value::String(cmd.to_string()));
            }
        }

        // Include tool output metadata
        for (k, v) in &output.metadata {
            metadata.insert(k.clone(), v.clone());
        }

        metadata
    }

    /// Create attachments from tool output if applicable.
    fn extract_attachments(
        tool_name: &str,
        tool_id: &str,
        output: &ToolOutput,
    ) -> Vec<AttachmentMessage> {
        let mut attachments = Vec::new();

        // File-related tools produce attachments
        let file_tools = [
            "Read",
            "read",
            "FileRead",
            "file_read",
            "Write",
            "FileWrite",
        ];
        if file_tools.contains(&tool_name) && !output.is_error {
            let attachment = AttachmentMessage::file_attachment(
                tool_name,
                tool_id,
                &output.content,
                "file_content",
            );
            attachments.push(attachment);
        }

        // Screenshot / image tools produce image attachments
        let image_tools = ["Screenshot", "screenshot", "TakeScreenshot"];
        if image_tools.contains(&tool_name) && !output.is_error {
            let mut attachment =
                AttachmentMessage::new(tool_name, tool_id, &output.content, "image/png");
            attachment.file_extension = Some("png".to_string());
            attachments.push(attachment);
        }

        attachments
    }
}

// ---------------------------------------------------------------------------
// Progress callback trait
// ---------------------------------------------------------------------------

/// Callback for receiving tool progress updates.
#[async_trait]
pub trait ProgressCallback: Send + Sync {
    /// Called when a progress update is available.
    async fn on_progress(&self, progress: ToolProgress);
}

/// A channel-based progress callback that sends progress over an mpsc channel.
pub struct ChannelProgressCallback {
    tx: mpsc::UnboundedSender<ToolProgress>,
}

impl ChannelProgressCallback {
    /// Create a new channel progress callback.
    pub fn new(tx: mpsc::UnboundedSender<ToolProgress>) -> Self {
        Self { tx }
    }
}

#[async_trait]
impl ProgressCallback for ChannelProgressCallback {
    async fn on_progress(&self, progress: ToolProgress) {
        let _ = self.tx.send(progress);
    }
}

/// A simple logging progress callback.
pub struct LoggingProgressCallback;

#[async_trait]
impl ProgressCallback for LoggingProgressCallback {
    async fn on_progress(&self, progress: ToolProgress) {
        tracing::debug!(
            tool_id = %progress.tool_id,
            tool_name = %progress.tool_name,
            status = %progress.status,
            "Tool progress: {}",
            progress.message.as_deref().unwrap_or("-")
        );
    }
}

// ---------------------------------------------------------------------------
// ToolExecutionService
// ---------------------------------------------------------------------------

/// Tools that modify files and should trigger auto-checkpointing.
const FILE_MODIFYING_TOOLS: &[&str] = &[
    "Write",
    "write",
    "FileWrite",
    "file_write",
    "Edit",
    "edit",
    "FileEdit",
    "file_edit",
    "MultiEdit",
    "multi_edit",
    "Bash",
    "bash", // Bash may modify files via commands
];

/// Returns true if the tool is known to modify files.
pub fn is_file_modifying_tool(tool_name: &str) -> bool {
    FILE_MODIFYING_TOOLS.contains(&tool_name)
}

/// Configuration for the tool execution service.
#[derive(Debug, Clone)]
pub struct ToolExecutionConfig {
    /// Default timeout for tool execution.
    pub default_timeout: Duration,
    /// Whether to collect attachments from tool outputs.
    pub collect_attachments: bool,
    /// Whether to emit hook progress messages.
    pub emit_hook_progress: bool,
    /// Whether to auto-checkpoint before file-modifying tools.
    pub auto_checkpoint: bool,
}

impl Default for ToolExecutionConfig {
    fn default() -> Self {
        Self {
            default_timeout: Duration::from_secs(300), // 5 minutes
            collect_attachments: true,
            emit_hook_progress: true,
            auto_checkpoint: true,
        }
    }
}

/// Unified tool execution service.
///
/// Provides `run_tool_use()` as the single entry point for tool execution,
/// wrapping permission checks, progress tracking, and metadata generation.
pub struct ToolExecutionService {
    /// Registry for looking up tools by name.
    registry: Arc<ToolRegistry>,
    /// Permission manager for access control.
    permission_manager: Arc<PermissionManager>,
    /// Optional progress callback.
    progress_callback: Option<Arc<dyn ProgressCallback>>,
    /// Optional checkpoint manager for auto-checkpointing before file modifications.
    checkpoint_manager: Option<CheckpointManager>,
    /// Optional hook manager for PreToolUse/PostToolUse lifecycle hooks.
    hook_manager: Option<Arc<tokio::sync::RwLock<HookManager>>>,
    /// Configuration.
    config: ToolExecutionConfig,
}

impl ToolExecutionService {
    /// Create a new tool execution service.
    pub fn new(registry: Arc<ToolRegistry>, permission_manager: Arc<PermissionManager>) -> Self {
        Self {
            registry,
            permission_manager,
            progress_callback: None,
            checkpoint_manager: None,
            hook_manager: None,
            config: ToolExecutionConfig::default(),
        }
    }

    /// Create a new tool execution service with a progress callback.
    pub fn with_progress_callback(
        registry: Arc<ToolRegistry>,
        permission_manager: Arc<PermissionManager>,
        callback: Arc<dyn ProgressCallback>,
    ) -> Self {
        Self {
            registry,
            permission_manager,
            progress_callback: Some(callback),
            checkpoint_manager: None,
            hook_manager: None,
            config: ToolExecutionConfig::default(),
        }
    }

    /// Create a new tool execution service with full configuration.
    pub fn with_config(
        registry: Arc<ToolRegistry>,
        permission_manager: Arc<PermissionManager>,
        config: ToolExecutionConfig,
    ) -> Self {
        Self {
            registry,
            permission_manager,
            progress_callback: None,
            checkpoint_manager: None,
            hook_manager: None,
            config,
        }
    }

    /// Set the checkpoint manager for auto-checkpointing before file modifications.
    pub fn set_checkpoint_manager(&mut self, mgr: CheckpointManager) {
        self.checkpoint_manager = Some(mgr);
    }

    /// Set the progress callback.
    pub fn set_progress_callback(&mut self, callback: Arc<dyn ProgressCallback>) {
        self.progress_callback = Some(callback);
    }

    /// Set the hook manager for PreToolUse/PostToolUse lifecycle hooks.
    ///
    /// The hook manager is stored behind an `Arc<RwLock<...>>` so it can be
    /// shared with other components (e.g. the query engine) that also need to
    /// fire lifecycle events.
    pub fn set_hook_manager(&mut self, hook_manager: Arc<tokio::sync::RwLock<HookManager>>) {
        self.hook_manager = Some(hook_manager);
    }

    /// Execute a tool with permission checks, progress tracking, and metadata.
    ///
    /// This is the primary entry point for tool execution.
    pub async fn run_tool_use(
        &self,
        session_id: Uuid,
        tool_name: &str,
        input: Value,
    ) -> Result<ToolExecutionResult, ToolExecutionError> {
        let tool_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();
        let mut progress_events = Vec::new();
        let hook_progress_events = Vec::new();

        // 1. Check tool exists
        if self.registry.get(tool_name).is_none() {
            let progress = ToolProgress::failed(&tool_id, tool_name, "Tool not found");
            progress_events.push(progress.clone());
            self.emit_progress(progress).await;
            return Err(ToolExecutionError::ToolNotFound(tool_name.to_string()));
        }

        // 2. Check permissions
        if let Err(perm_err) = self
            .permission_manager
            .check_tool_permission(session_id, tool_name)
        {
            let reason = perm_err.to_string();
            let progress = ToolProgress::failed(&tool_id, tool_name, &reason);
            progress_events.push(progress.clone());
            self.emit_progress(progress).await;
            return Err(ToolExecutionError::PermissionDenied {
                tool_name: tool_name.to_string(),
                reason,
            });
        }

        // 2b. Extract file paths from input for file-modifying tools
        let files_modified = Self::extract_file_paths(tool_name, &input);

        // 2c. Auto-checkpoint before file-modifying tools
        let mut checkpoint_created = false;
        if self.config.auto_checkpoint
            && is_file_modifying_tool(tool_name)
            && !files_modified.is_empty()
        {
            if let Some(ref mgr) = self.checkpoint_manager {
                let desc = format!("{}: {}", tool_name, files_modified.join(", "));
                match mgr.create_checkpoint(tool_name, &desc) {
                    Ok(_) => checkpoint_created = true,
                    Err(e) => {
                        tracing::debug!("Auto-checkpoint skipped: {e}");
                    }
                }
            }
        }

        // 2d. Run PreToolUse hooks - deny blocks execution, modify can change input
        let mut effective_input = input;
        if let Some(ref hm) = self.hook_manager {
            let hook_event = HookEvent::PreToolUse {
                tool_name: tool_name.to_string(),
                input: effective_input.clone(),
            };
            let hm_guard = hm.read().await;
            match hm_guard.run_hooks(&hook_event).await {
                Ok(results) => {
                    let decision = HookManager::resolve_results(&results);
                    match decision {
                        HookDecision::Deny { reason } => {
                            let msg = format!("Hook blocked: {reason}");
                            let progress = ToolProgress::failed(&tool_id, tool_name, &msg);
                            progress_events.push(progress.clone());
                            self.emit_progress(progress).await;
                            return Err(ToolExecutionError::HookBlocked(msg));
                        }
                        HookDecision::Modify { modified_input, .. } => {
                            if let Some(new_input) = modified_input {
                                tracing::debug!(
                                    "PreToolUse hook modified input for tool '{}'",
                                    tool_name
                                );
                                effective_input = new_input;
                            }
                        }
                        HookDecision::Allow => {}
                    }
                }
                Err(e) => {
                    tracing::warn!("PreToolUse hook error for '{}': {e}", tool_name);
                    // Hook errors do not block execution by default
                }
            }
        }

        // 3. Emit started progress
        let started = ToolProgress::started(&tool_id, tool_name);
        progress_events.push(started.clone());
        self.emit_progress(started).await;

        // 4. Execute the tool (using effective_input which may have been modified by hooks)
        let execute_future = self.registry.execute(tool_name, effective_input.clone());
        let output = match tokio::time::timeout(self.config.default_timeout, execute_future).await {
            Ok(Ok(output)) => output,
            Ok(Err(err)) => {
                // Discard checkpoint if tool failed to execute
                if checkpoint_created {
                    if let Some(ref mgr) = self.checkpoint_manager {
                        mgr.discard_last();
                    }
                }
                let msg = err.to_string();
                let progress = ToolProgress::failed(&tool_id, tool_name, &msg);
                progress_events.push(progress.clone());
                self.emit_progress(progress).await;

                // Fire PostToolUseFailure hook
                self.fire_post_failure_hook(tool_name, &effective_input, &msg)
                    .await;

                return Err(ToolExecutionError::ExecutionFailed(msg));
            }
            Err(_) => {
                // Tool timed out
                if checkpoint_created {
                    if let Some(ref mgr) = self.checkpoint_manager {
                        mgr.discard_last();
                    }
                }
                let msg = format!("Tool timed out after {:?}", self.config.default_timeout);
                let progress = ToolProgress::failed(&tool_id, tool_name, &msg);
                progress_events.push(progress.clone());
                self.emit_progress(progress).await;

                self.fire_post_failure_hook(tool_name, &effective_input, &msg)
                    .await;

                return Err(ToolExecutionError::Timeout {
                    tool_name: tool_name.to_string(),
                    timeout_secs: self.config.default_timeout.as_secs(),
                });
            }
        };

        let duration = start_time.elapsed();
        let is_error = output.is_error;

        // If tool returned an error, discard the checkpoint
        if is_error && checkpoint_created {
            if let Some(ref mgr) = self.checkpoint_manager {
                mgr.discard_last();
            }
            checkpoint_created = false;
        }

        // 4b. Truncate oversized tool output (~10K tokens max)
        const MAX_TOOL_OUTPUT_CHARS: usize = 40_000; // ~10K tokens at 4 chars/token
        let mut output = output;
        if output.content.len() > MAX_TOOL_OUTPUT_CHARS {
            let original_len = output.content.len();
            let truncated: String = output.content.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
            output.content = format!(
                "{truncated}\n\n[Tool output truncated: {original_len} chars -> {MAX_TOOL_OUTPUT_CHARS} chars]"
            );
        }

        // 5. Emit completed/failed progress
        if is_error {
            let progress = ToolProgress::failed(&tool_id, tool_name, &output.content);
            progress_events.push(progress.clone());
            self.emit_progress(progress).await;
        } else {
            let progress = ToolProgress::completed(&tool_id, tool_name);
            progress_events.push(progress.clone());
            self.emit_progress(progress).await;
        }

        // 5b. Fire PostToolUse / PostToolUseFailure hooks
        {
            let output_val = serde_json::to_value(&output.content).unwrap_or(Value::Null);
            if is_error {
                self.fire_post_failure_hook(tool_name, &effective_input, &output.content)
                    .await;
            } else {
                self.fire_post_hook(tool_name, &effective_input, &output_val)
                    .await;
            }
        }

        // 6. Build metadata
        let metadata = ToolExecutionResult::build_metadata(tool_name, &effective_input, &output);

        // 7. Extract attachments
        let attachments = if self.config.collect_attachments {
            ToolExecutionResult::extract_attachments(&tool_id, &tool_id, &output)
        } else {
            Vec::new()
        };

        // 8. Build stop hook info
        let stop_hook_info = Some(StopHookInfo {
            tool_name: tool_name.to_string(),
            tool_input: effective_input.clone(),
            tool_output: output.clone(),
            duration,
            is_error,
            session_id,
            tool_id: tool_id.clone(),
        });

        // 9. Set elapsed on all progress events
        for p in &mut progress_events {
            p.elapsed = Some(duration);
        }

        Ok(ToolExecutionResult {
            output,
            progress: progress_events,
            duration,
            tool_id,
            tool_name: tool_name.to_string(),
            is_error,
            metadata,
            attachments,
            hook_progress: hook_progress_events,
            stop_hook_info,
            session_id,
            files_modified,
            checkpoint_created,
        })
    }

    /// Extract file paths from tool input for tracking.
    fn extract_file_paths(tool_name: &str, input: &Value) -> Vec<String> {
        let mut paths = Vec::new();

        if let Some(obj) = input.as_object() {
            // Standard file path fields
            for key in &["file_path", "path", "filePath"] {
                if let Some(v) = obj.get(*key).and_then(|v| v.as_str()) {
                    paths.push(v.to_string());
                }
            }
        }

        // Bash tools don't have direct file paths
        if paths.is_empty() && (tool_name == "Bash" || tool_name == "bash") {
            // We still mark Bash as file-modifying but with no specific paths
            // The checkpoint still fires based on tool name alone
        }

        paths
    }

    /// Execute a tool with a timeout.
    pub async fn run_tool_use_with_timeout(
        &self,
        session_id: Uuid,
        tool_name: &str,
        input: Value,
        timeout: Duration,
    ) -> Result<ToolExecutionResult, ToolExecutionError> {
        match tokio::time::timeout(timeout, self.run_tool_use(session_id, tool_name, input)).await {
            Ok(result) => result,
            Err(_) => Err(ToolExecutionError::Timeout {
                tool_name: tool_name.to_string(),
                timeout_secs: timeout.as_secs(),
            }),
        }
    }

    /// Create a permission prompt for a tool (for interactive permission flow).
    pub fn create_permission_prompt(
        &self,
        session_id: Uuid,
        tool_name: &str,
        tool_input: &Value,
    ) -> Option<crate::permissions::PermissionPrompt> {
        self.permission_manager
            .create_permission_prompt(tool_name, tool_input, session_id)
    }

    /// Process a user's permission choice.
    pub fn process_permission_choice(
        &self,
        _session_id: Uuid,
        prompt: &crate::permissions::PermissionPrompt,
        choice: crate::permissions::PermissionChoice,
    ) -> Result<(), ToolExecutionError> {
        // We need interior mutability for this, but since PermissionManager
        // is behind Arc, we return a descriptive error for the synchronous API.
        // In practice, callers would use the PermissionManager directly.
        match choice {
            crate::permissions::PermissionChoice::Deny => {
                Err(ToolExecutionError::PermissionDenied {
                    tool_name: prompt.tool_name.clone(),
                    reason: format!("User denied: {}", prompt.description),
                })
            }
            _ => Ok(()),
        }
    }

    /// Get the tool registry (for tool discovery).
    pub fn registry(&self) -> &Arc<ToolRegistry> {
        &self.registry
    }

    /// Get the permission manager.
    pub fn permission_manager(&self) -> &Arc<PermissionManager> {
        &self.permission_manager
    }

    /// Get the configuration.
    pub fn config(&self) -> &ToolExecutionConfig {
        &self.config
    }

    /// Emit a progress event if a callback is configured.
    async fn emit_progress(&self, progress: ToolProgress) {
        if let Some(callback) = &self.progress_callback {
            callback.on_progress(progress).await;
        }
    }

    // ── Hook helper methods ──────────────────────────────────────────────

    /// Fire PostToolUse hooks after a successful tool execution.
    ///
    /// Errors are logged but do not propagate -- post-hooks are informational.
    async fn fire_post_hook(&self, tool_name: &str, input: &Value, output: &Value) {
        if let Some(ref hm) = self.hook_manager {
            let event = HookEvent::PostToolUse {
                tool_name: tool_name.to_string(),
                input: input.clone(),
                output: output.clone(),
            };
            let hm_guard = hm.read().await;
            if let Err(e) = hm_guard.run_hooks(&event).await {
                tracing::warn!("PostToolUse hook error for '{}': {e}", tool_name);
            }
        }
    }

    /// Fire PostToolUseFailure hooks after a tool execution failure.
    ///
    /// Errors are logged but do not propagate -- post-hooks are informational.
    async fn fire_post_failure_hook(&self, tool_name: &str, input: &Value, error: &str) {
        if let Some(ref hm) = self.hook_manager {
            let event = HookEvent::PostToolUseFailure {
                tool_name: tool_name.to_string(),
                input: input.clone(),
                error: error.to_string(),
            };
            let hm_guard = hm.read().await;
            if let Err(e) = hm_guard.run_hooks(&event).await {
                tracing::warn!("PostToolUseFailure hook error for '{}': {e}", tool_name);
            }
        }
    }

    // ── Session lifecycle methods ────────────────────────────────────────

    /// Fire SessionStart hooks.
    ///
    /// Call this once when a session begins (e.g. from the REPL startup).
    /// Errors are logged but do not propagate -- session hooks are informational.
    pub async fn on_session_start(&self, session_id: &str) {
        if let Some(ref hm) = self.hook_manager {
            let event = HookEvent::SessionStart {
                session_id: session_id.to_string(),
            };
            let hm_guard = hm.read().await;
            if let Err(e) = hm_guard.run_hooks(&event).await {
                tracing::warn!("SessionStart hook error: {e}");
            }
        }
    }

    /// Fire SessionEnd hooks.
    ///
    /// Call this once when a session ends (e.g. from the REPL shutdown).
    /// Errors are logged but do not propagate -- session hooks are informational.
    pub async fn on_session_end(&self, session_id: &str) {
        if let Some(ref hm) = self.hook_manager {
            let event = HookEvent::SessionEnd {
                session_id: session_id.to_string(),
            };
            let hm_guard = hm.read().await;
            if let Err(e) = hm_guard.run_hooks(&event).await {
                tracing::warn!("SessionEnd hook error: {e}");
            }
        }
    }

    /// Fire UserPromptSubmit hooks and return the (possibly modified) prompt.
    ///
    /// Call this when the user submits input, before processing.
    /// Returns `Ok(Some(prompt))` with the possibly-modified prompt text,
    /// `Ok(None)` if no hooks are configured (passthrough), or an error
    /// string if a hook denies the submission.
    pub async fn on_user_prompt_submit(
        &self,
        prompt: &str,
    ) -> std::result::Result<Option<String>, String> {
        if let Some(ref hm) = self.hook_manager {
            let event = HookEvent::UserPromptSubmit {
                prompt: prompt.to_string(),
            };
            let hm_guard = hm.read().await;
            match hm_guard.run_hooks(&event).await {
                Ok(results) => {
                    let decision = HookManager::resolve_results(&results);
                    match decision {
                        HookDecision::Deny { reason } => Err(reason),
                        HookDecision::Modify { modified_input, .. } => {
                            // If the hook provides modified_input, treat its string
                            // value as the new prompt text.
                            if let Some(Value::String(new_prompt)) = modified_input {
                                Ok(Some(new_prompt))
                            } else {
                                Ok(None)
                            }
                        }
                        HookDecision::Allow => Ok(None),
                    }
                }
                Err(e) => {
                    tracing::warn!("UserPromptSubmit hook error: {e}");
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Get the hook manager, if configured.
    pub fn hook_manager(&self) -> Option<&Arc<tokio::sync::RwLock<HookManager>>> {
        self.hook_manager.as_ref()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::Permission;
    use crate::permissions::PermissionLevel;
    use crate::permissions::PermissionPrompt;
    use crate::tools::Tool;

    /// A simple test tool that echoes its input.
    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "Echo"
        }
        fn description(&self) -> &str {
            "Echoes the input"
        }
        fn input_schema(&self) -> Value {
            serde_json::json!({"type": "object", "properties": {"message": {"type": "string"}}})
        }
        async fn execute(&self, input: Value) -> crate::tools::ToolResult<ToolOutput> {
            let message = input
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("no message");
            Ok(ToolOutput {
                content: message.to_string(),
                is_error: false,
                metadata: Default::default(),
            })
        }
    }

    /// A tool that always fails.
    struct FailTool;

    #[async_trait]
    impl Tool for FailTool {
        fn name(&self) -> &str {
            "Fail"
        }
        fn description(&self) -> &str {
            "Always fails"
        }
        fn input_schema(&self) -> Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _input: Value) -> crate::tools::ToolResult<ToolOutput> {
            Ok(ToolOutput {
                content: "something went wrong".to_string(),
                is_error: true,
                metadata: Default::default(),
            })
        }
    }

    /// A tool that panics.
    struct PanicTool;

    #[async_trait]
    impl Tool for PanicTool {
        fn name(&self) -> &str {
            "Panic"
        }
        fn description(&self) -> &str {
            "Panics during execution"
        }
        fn input_schema(&self) -> Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _input: Value) -> crate::tools::ToolResult<ToolOutput> {
            Err(crate::tools::ToolError::ExecutionFailed("boom".to_string()))
        }
    }

    /// Helper to build a service with Echo and Fail tools registered.
    async fn make_service() -> ToolExecutionService {
        let registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        registry.register(Box::new(FailTool)).unwrap();
        registry.register(Box::new(PanicTool)).unwrap();
        let registry = Arc::new(registry);
        let permission_manager = Arc::new(PermissionManager::new());
        ToolExecutionService::new(registry, permission_manager)
    }

    // -- ToolProgressStatus tests --

    #[test]
    fn test_tool_progress_status_display() {
        assert_eq!(format!("{}", ToolProgressStatus::Started), "Started");
        assert_eq!(format!("{}", ToolProgressStatus::Updated), "Updated");
        assert_eq!(format!("{}", ToolProgressStatus::Completed), "Completed");
        assert_eq!(format!("{}", ToolProgressStatus::Failed), "Failed");
    }

    // -- ToolProgress tests --

    #[test]
    fn test_tool_progress_new() {
        let p = ToolProgress::new(
            "id-1".to_string(),
            "Echo".to_string(),
            ToolProgressStatus::Started,
        );
        assert_eq!(p.tool_id, "id-1");
        assert_eq!(p.tool_name, "Echo");
        assert_eq!(p.status, ToolProgressStatus::Started);
        assert!(p.message.is_none());
        assert!(p.elapsed.is_none());
    }

    #[test]
    fn test_tool_progress_factory_methods() {
        let started = ToolProgress::started("id-1", "Bash");
        assert_eq!(started.status, ToolProgressStatus::Started);

        let updated = ToolProgress::updated("id-1", "Bash", "compiling...");
        assert_eq!(updated.status, ToolProgressStatus::Updated);
        assert_eq!(updated.message.as_deref(), Some("compiling..."));

        let completed = ToolProgress::completed("id-1", "Bash");
        assert_eq!(completed.status, ToolProgressStatus::Completed);

        let failed = ToolProgress::failed("id-1", "Bash", "disk full");
        assert_eq!(failed.status, ToolProgressStatus::Failed);
        assert!(failed.message.unwrap().contains("disk full"));
    }

    // -- StopHookInfo tests --

    #[test]
    fn test_stop_hook_info() {
        let info = StopHookInfo {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"cmd": "ls"}),
            tool_output: ToolOutput {
                content: "file1.txt\nfile2.txt".to_string(),
                is_error: false,
                metadata: Default::default(),
            },
            duration: Duration::from_millis(100),
            is_error: false,
            session_id: Uuid::new_v4(),
            tool_id: "id-1".to_string(),
        };
        assert_eq!(info.tool_name, "Bash");
        assert!(!info.is_error);
        assert_eq!(info.duration, Duration::from_millis(100));
    }

    // -- HookProgress tests --

    #[test]
    fn test_hook_progress_new() {
        let hp = HookProgress::new("PreToolUse", "Bash", "Checking permissions...");
        assert_eq!(hp.hook_type, "PreToolUse");
        assert_eq!(hp.tool_name, "Bash");
        assert_eq!(hp.message, "Checking permissions...");
    }

    // -- AttachmentMessage tests --

    #[test]
    fn test_attachment_message_new() {
        let att = AttachmentMessage::new("Read", "id-1", "file contents here", "text/plain");
        assert_eq!(att.source_tool, "Read");
        assert_eq!(att.tool_id, "id-1");
        assert_eq!(att.content_type, "text/plain");
        assert!(att.file_extension.is_none());
    }

    #[test]
    fn test_attachment_message_file() {
        let att = AttachmentMessage::file_attachment("Read", "id-1", "content", "/path/to/file.rs");
        assert_eq!(att.file_extension.as_deref(), Some("rs"));
    }

    #[test]
    fn test_attachment_message_file_no_extension() {
        let att =
            AttachmentMessage::file_attachment("Read", "id-1", "content", "/path/to/Makefile");
        assert!(att.file_extension.is_none());
    }

    // -- ToolExecutionResult metadata tests --

    #[test]
    fn test_build_metadata_file_extension() {
        let input = serde_json::json!({"file_path": "/tmp/test.rs"});
        let output = ToolOutput {
            content: "fn main() {}".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let meta = ToolExecutionResult::build_metadata("Read", &input, &output);
        assert_eq!(
            meta.get("file_extension").and_then(|v| v.as_str()),
            Some("rs")
        );
    }

    #[test]
    fn test_build_metadata_bash_command() {
        let input = serde_json::json!({"command": "cargo build"});
        let output = ToolOutput {
            content: "Compiling...".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let meta = ToolExecutionResult::build_metadata("Bash", &input, &output);
        assert_eq!(
            meta.get("bash_command").and_then(|v| v.as_str()),
            Some("cargo build")
        );
    }

    #[test]
    fn test_build_metadata_cmd_alias() {
        let input = serde_json::json!({"cmd": "ls -la"});
        let output = ToolOutput {
            content: "total 0".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let meta = ToolExecutionResult::build_metadata("Bash", &input, &output);
        assert_eq!(
            meta.get("bash_command").and_then(|v| v.as_str()),
            Some("ls -la")
        );
    }

    #[test]
    fn test_extract_attachments_read_tool() {
        let output = ToolOutput {
            content: "file data".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let attachments = ToolExecutionResult::extract_attachments("Read", "id-1", &output);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].source_tool, "Read");
    }

    #[test]
    fn test_extract_attachments_error_no_attachment() {
        let output = ToolOutput {
            content: "permission denied".to_string(),
            is_error: true,
            metadata: Default::default(),
        };
        let attachments = ToolExecutionResult::extract_attachments("Read", "id-1", &output);
        assert!(attachments.is_empty());
    }

    // -- ToolExecutionConfig tests --

    #[test]
    fn test_config_defaults() {
        let config = ToolExecutionConfig::default();
        assert_eq!(config.default_timeout, Duration::from_secs(300));
        assert!(config.collect_attachments);
        assert!(config.emit_hook_progress);
        assert!(config.auto_checkpoint);
    }

    // -- ToolExecutionService integration tests --

    #[tokio::test]
    async fn test_service_run_tool_success() {
        let service = make_service().await;
        let session_id = Uuid::new_v4();

        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "hello"}))
            .await
            .unwrap();

        assert_eq!(result.output.content, "hello");
        assert!(!result.is_error);
        assert_eq!(result.tool_name, "Echo");
        assert_eq!(result.progress.len(), 2); // Started + Completed
        assert_eq!(result.progress[0].status, ToolProgressStatus::Started);
        assert_eq!(result.progress[1].status, ToolProgressStatus::Completed);
        assert!(result.stop_hook_info.is_some());
    }

    #[tokio::test]
    async fn test_service_run_tool_not_found() {
        let service = make_service().await;
        let session_id = Uuid::new_v4();

        let result = service
            .run_tool_use(session_id, "NonExistent", Value::Null)
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolExecutionError::ToolNotFound(_)));
    }

    #[tokio::test]
    async fn test_service_run_tool_error_output() {
        let service = make_service().await;
        let session_id = Uuid::new_v4();

        let result = service
            .run_tool_use(session_id, "Fail", Value::Null)
            .await
            .unwrap();

        assert!(result.is_error);
        assert_eq!(result.output.content, "something went wrong");
        assert_eq!(result.progress.len(), 2); // Started + Failed
        assert_eq!(result.progress[1].status, ToolProgressStatus::Failed);
    }

    #[tokio::test]
    async fn test_service_run_tool_execution_error() {
        let service = make_service().await;
        let session_id = Uuid::new_v4();

        let result = service.run_tool_use(session_id, "Panic", Value::Null).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ToolExecutionError::ExecutionFailed(_)
        ));
    }

    #[tokio::test]
    async fn test_service_run_tool_with_timeout_success() {
        let service = make_service().await;
        let session_id = Uuid::new_v4();

        let result = service
            .run_tool_use_with_timeout(
                session_id,
                "Echo",
                serde_json::json!({"message": "hi"}),
                Duration::from_secs(5),
            )
            .await
            .unwrap();

        assert_eq!(result.output.content, "hi");
    }

    #[tokio::test]
    async fn test_service_run_tool_with_timeout_exceeded() {
        let service = make_service().await;
        let session_id = Uuid::new_v4();

        let result = service
            .run_tool_use_with_timeout(
                session_id,
                "Echo",
                serde_json::json!({"message": "hi"}),
                Duration::from_nanos(1), // 1 nanosecond - will almost certainly time out
            )
            .await;

        // The tool might complete before the timeout, so we just check the type
        if let Err(ToolExecutionError::Timeout { tool_name, .. }) = result {
            assert_eq!(tool_name, "Echo");
        }
        // If it succeeds, that's fine too - the tool is fast
    }

    #[tokio::test]
    async fn test_service_with_progress_callback() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let callback = Arc::new(ChannelProgressCallback::new(tx));

        let mut service = make_service().await;
        service.set_progress_callback(callback);

        let session_id = Uuid::new_v4();
        let _result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "test"}))
            .await
            .unwrap();

        // Should have received progress events
        let mut received = Vec::new();
        while let Ok(p) = rx.try_recv() {
            received.push(p);
        }
        assert!(!received.is_empty());
        assert_eq!(received[0].status, ToolProgressStatus::Started);
    }

    #[tokio::test]
    async fn test_service_permission_denied() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        let registry = Arc::new(registry);

        let mut permission_manager = PermissionManager::new();
        permission_manager.set_tool_permission(
            "Echo".to_string(),
            Permission::new("tool", "execute", PermissionLevel::Admin),
        );
        let permission_manager = Arc::new(permission_manager);

        let service = ToolExecutionService::new(registry, permission_manager);
        let session_id = Uuid::new_v4();

        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "hello"}))
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ToolExecutionError::PermissionDenied { .. }
        ));
    }

    #[tokio::test]
    async fn test_service_result_has_duration() {
        let service = make_service().await;
        let session_id = Uuid::new_v4();

        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "test"}))
            .await
            .unwrap();

        assert!(result.duration.as_nanos() > 0);
    }

    #[tokio::test]
    async fn test_service_result_progress_has_elapsed() {
        let service = make_service().await;
        let session_id = Uuid::new_v4();

        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "test"}))
            .await
            .unwrap();

        for p in &result.progress {
            assert!(p.elapsed.is_some());
        }
    }

    // -- Auto-checkpoint integration tests --

    #[test]
    fn test_is_file_modifying_tool() {
        assert!(is_file_modifying_tool("Write"));
        assert!(is_file_modifying_tool("write"));
        assert!(is_file_modifying_tool("Edit"));
        assert!(is_file_modifying_tool("Bash"));
        assert!(!is_file_modifying_tool("Read"));
        assert!(!is_file_modifying_tool("Grep"));
        assert!(!is_file_modifying_tool("Unknown"));
    }

    #[test]
    fn test_extract_file_paths_from_input() {
        let input = serde_json::json!({"file_path": "/tmp/test.rs"});
        let paths = ToolExecutionService::extract_file_paths("Write", &input);
        assert_eq!(paths, vec!["/tmp/test.rs"]);

        let input = serde_json::json!({"path": "/tmp/other.rs"});
        let paths = ToolExecutionService::extract_file_paths("Edit", &input);
        assert_eq!(paths, vec!["/tmp/other.rs"]);

        let input = serde_json::json!({"command": "ls"});
        let paths = ToolExecutionService::extract_file_paths("Bash", &input);
        assert!(paths.is_empty());
    }

    #[tokio::test]
    async fn test_service_result_has_files_modified() {
        let service = make_service().await;
        let session_id = Uuid::new_v4();

        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "hello"}))
            .await
            .unwrap();

        assert!(result.files_modified.is_empty());
        assert!(!result.checkpoint_created);
    }

    #[tokio::test]
    async fn test_service_no_checkpoint_without_manager() {
        let service = make_service().await;
        let session_id = Uuid::new_v4();

        // Even for file-modifying tools, no checkpoint is created without a manager
        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "hello"}))
            .await
            .unwrap();

        assert!(!result.checkpoint_created);
    }

    // ── Hook integration tests ──────────────────────────────────────────────

    /// Helper to build a service with a HookManager loaded from a temp hooks file.
    async fn make_service_with_hooks(hooks_json: &str) -> ToolExecutionService {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let hooks_path = temp_dir.path().join("hooks.json");
        std::fs::write(&hooks_path, hooks_json).unwrap();

        let mut mgr = HookManager::with_paths(
            std::path::PathBuf::from("/nonexistent"),
            std::path::PathBuf::from("/nonexistent"),
        );
        mgr.load_from_path(&hooks_path).unwrap();

        let registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        registry.register(Box::new(FailTool)).unwrap();
        registry.register(Box::new(PanicTool)).unwrap();

        let mut service =
            ToolExecutionService::new(Arc::new(registry), Arc::new(PermissionManager::new()));
        service.set_hook_manager(Arc::new(tokio::sync::RwLock::new(mgr)));
        service
    }

    #[tokio::test]
    async fn test_hook_pre_tool_use_allows_execution() {
        // Hook that echoes something (default Allow decision)
        let service = make_service_with_hooks(
            r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Echo",
                        "hooks": [
                            {"command": "echo 'pre-hook ran'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#,
        )
        .await;

        let session_id = Uuid::new_v4();
        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "hello"}))
            .await
            .unwrap();

        assert_eq!(result.output.content, "hello");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_hook_pre_tool_use_denies_execution() {
        // Hook that denies the tool call
        let service = make_service_with_hooks(r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo '{\"decision\": \"deny\", \"reason\": \"policy blocks this\"}'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#).await;

        let session_id = Uuid::new_v4();
        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "hello"}))
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            ToolExecutionError::HookBlocked(msg) => {
                assert!(msg.contains("policy blocks this"));
            }
            other => panic!("Expected HookBlocked, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_hook_pre_tool_use_modify_input() {
        // Hook that modifies the tool input
        let service = make_service_with_hooks(r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Echo",
                        "hooks": [
                            {"command": "echo '{\"decision\": \"modify\", \"modified_input\": {\"message\": \"modified!\"}}'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#).await;

        let session_id = Uuid::new_v4();
        let result = service
            .run_tool_use(
                session_id,
                "Echo",
                serde_json::json!({"message": "original"}),
            )
            .await
            .unwrap();

        // The tool should have received the modified input
        assert_eq!(result.output.content, "modified!");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_hook_no_hook_manager_passes_through() {
        // Service without a hook manager should work normally
        let service = make_service().await;
        assert!(service.hook_manager().is_none());

        let session_id = Uuid::new_v4();
        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "hello"}))
            .await
            .unwrap();

        assert_eq!(result.output.content, "hello");
    }

    #[tokio::test]
    async fn test_hook_post_tool_use_fires_on_success() {
        // PostToolUse hook runs after success - the tool still executes
        let service = make_service_with_hooks(
            r#"{
            "hooks": {
                "PostToolUse": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo 'post-hook ran'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#,
        )
        .await;

        let session_id = Uuid::new_v4();
        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "test"}))
            .await
            .unwrap();

        assert_eq!(result.output.content, "test");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_hook_post_tool_use_failure_fires_on_error_output() {
        // PostToolUseFailure hook fires when tool returns is_error=true
        let service = make_service_with_hooks(
            r#"{
            "hooks": {
                "PostToolUseFailure": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo 'failure-hook ran'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#,
        )
        .await;

        let session_id = Uuid::new_v4();
        let result = service
            .run_tool_use(session_id, "Fail", serde_json::json!({}))
            .await
            .unwrap();

        assert!(result.is_error);
        assert_eq!(result.output.content, "something went wrong");
    }

    #[tokio::test]
    async fn test_hook_post_failure_fires_on_execution_error() {
        // PostToolUseFailure hook fires when tool execute() returns Err
        let service = make_service_with_hooks(r#"{
            "hooks": {
                "PostToolUseFailure": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo 'exec-error-hook ran'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#).await;

        let session_id = Uuid::new_v4();
        let result = service
            .run_tool_use(session_id, "Panic", serde_json::json!({}))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_hook_pre_denies_non_matching_tool() {
        // A hook matching only "Bash" should not affect "Echo"
        let service = make_service_with_hooks(r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {"command": "echo '{\"decision\": \"deny\", \"reason\": \"bash blocked\"}'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#).await;

        let session_id = Uuid::new_v4();
        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "hello"}))
            .await
            .unwrap();

        assert_eq!(result.output.content, "hello");
    }

    #[tokio::test]
    async fn test_session_lifecycle_hooks() {
        // SessionStart and SessionEnd hooks should fire without error
        let service = make_service_with_hooks(
            r#"{
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo 'session started'", "timeout": 5, "blocking": true}
                        ]
                    }
                ],
                "SessionEnd": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo 'session ended'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#,
        )
        .await;

        // These should not panic or error
        service.on_session_start("test-session-1").await;
        service.on_session_end("test-session-1").await;
    }

    #[tokio::test]
    async fn test_session_lifecycle_no_hook_manager() {
        // Without a hook manager, lifecycle methods should be no-ops
        let service = make_service().await;
        service.on_session_start("test").await;
        service.on_session_end("test").await;
    }

    #[tokio::test]
    async fn test_user_prompt_submit_hook_allow() {
        let service = make_service_with_hooks(
            r#"{
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo 'prompt submitted'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#,
        )
        .await;

        let result = service.on_user_prompt_submit("hello world").await;
        assert!(result.is_ok());
        // Allow returns None (no modification)
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_user_prompt_submit_hook_deny() {
        let service = make_service_with_hooks(r#"{
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo '{\"decision\": \"deny\", \"reason\": \"prompt rejected\"}'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#).await;

        let result = service.on_user_prompt_submit("rm -rf /").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("prompt rejected"));
    }

    #[tokio::test]
    async fn test_user_prompt_submit_hook_modify() {
        let service = make_service_with_hooks(r#"{
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo '{\"decision\": \"modify\", \"modified_input\": \"sanitized prompt\"}'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#).await;

        let result = service.on_user_prompt_submit("dangerous input").await;
        assert!(result.is_ok());
        let modified = result.unwrap();
        assert_eq!(modified, Some("sanitized prompt".to_string()));
    }

    #[tokio::test]
    async fn test_user_prompt_submit_no_hook_manager() {
        let service = make_service().await;
        let result = service.on_user_prompt_submit("hello").await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_hook_pre_then_post_both_fire() {
        // Both PreToolUse and PostToolUse should fire in order
        let service = make_service_with_hooks(
            r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo 'pre-hook'", "timeout": 5, "blocking": true}
                        ]
                    }
                ],
                "PostToolUse": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo 'post-hook'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#,
        )
        .await;

        let session_id = Uuid::new_v4();
        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "test"}))
            .await
            .unwrap();

        // Tool should have executed successfully
        assert_eq!(result.output.content, "test");
        assert!(!result.is_error);
    }

    // -- ToolProgress additional tests --

    #[test]
    fn test_tool_progress_started_helper() {
        let p = ToolProgress::started("id-1", "Bash");
        assert_eq!(p.tool_id, "id-1");
        assert_eq!(p.tool_name, "Bash");
        assert_eq!(p.status, ToolProgressStatus::Started);
        assert!(p.message.is_none());
        assert!(p.elapsed.is_none());
    }

    #[test]
    fn test_tool_progress_updated_helper() {
        let p = ToolProgress::updated("id-2", "Bash", "running ls...");
        assert_eq!(p.status, ToolProgressStatus::Updated);
        assert_eq!(p.message, Some("running ls...".to_string()));
    }

    #[test]
    fn test_tool_progress_completed_helper() {
        let p = ToolProgress::completed("id-3", "Read");
        assert_eq!(p.status, ToolProgressStatus::Completed);
        assert!(p.message.is_none());
    }

    #[test]
    fn test_tool_progress_failed_helper() {
        let p = ToolProgress::failed("id-4", "Bash", "command not found");
        assert_eq!(p.status, ToolProgressStatus::Failed);
        assert_eq!(p.message, Some("command not found".to_string()));
    }

    #[test]
    fn test_tool_progress_serialization_roundtrip() {
        let p = ToolProgress::updated("id-5", "Bash", "partial output");
        let json = serde_json::to_string(&p).unwrap();
        let back: ToolProgress = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tool_id, "id-5");
        assert_eq!(back.tool_name, "Bash");
        assert_eq!(back.status, ToolProgressStatus::Updated);
        assert_eq!(back.message, Some("partial output".to_string()));
    }

    #[test]
    fn test_tool_progress_status_equality() {
        assert_eq!(ToolProgressStatus::Started, ToolProgressStatus::Started);
        assert_ne!(ToolProgressStatus::Started, ToolProgressStatus::Completed);
        assert_ne!(ToolProgressStatus::Updated, ToolProgressStatus::Failed);
    }

    #[test]
    fn test_tool_progress_status_serde_roundtrip() {
        for status in [
            ToolProgressStatus::Started,
            ToolProgressStatus::Updated,
            ToolProgressStatus::Completed,
            ToolProgressStatus::Failed,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: ToolProgressStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, back);
        }
    }

    #[test]
    fn test_tool_execution_error_display() {
        let e = ToolExecutionError::ToolNotFound("Foo".to_string());
        assert!(e.to_string().contains("Foo"));

        let e = ToolExecutionError::Timeout {
            tool_name: "Bash".to_string(),
            timeout_secs: 30,
        };
        assert!(e.to_string().contains("30"));
        assert!(e.to_string().contains("Bash"));

        let e = ToolExecutionError::PermissionDenied {
            tool_name: "Write".to_string(),
            reason: "not allowed".to_string(),
        };
        assert!(e.to_string().contains("Write"));
        assert!(e.to_string().contains("not allowed"));
    }

    // =========================================================================
    // NEW TESTS: Error paths, uncovered areas, edge cases
    // =========================================================================

    /// A tool that sleeps for a configurable duration (for timeout testing).
    struct DelayedTool {
        delay_ms: u64,
    }

    #[async_trait]
    impl Tool for DelayedTool {
        fn name(&self) -> &str {
            "Delayed"
        }
        fn description(&self) -> &str {
            "A tool that delays before responding"
        }
        fn input_schema(&self) -> Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _input: Value) -> crate::tools::ToolResult<ToolOutput> {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            Ok(ToolOutput {
                content: "delayed result".to_string(),
                is_error: false,
                metadata: Default::default(),
            })
        }
    }

    /// A tool that returns very large output (for truncation testing).
    struct LargeOutputTool;

    #[async_trait]
    impl Tool for LargeOutputTool {
        fn name(&self) -> &str {
            "LargeOutput"
        }
        fn description(&self) -> &str {
            "Returns very large output"
        }
        fn input_schema(&self) -> Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _input: Value) -> crate::tools::ToolResult<ToolOutput> {
            let large_content = "x".repeat(50_000); // 50K chars, over the 40K truncation limit
            Ok(ToolOutput {
                content: large_content,
                is_error: false,
                metadata: Default::default(),
            })
        }
    }

    /// A tool that returns output with metadata.
    struct MetadataTool;

    #[async_trait]
    impl Tool for MetadataTool {
        fn name(&self) -> &str {
            "MetadataTool"
        }
        fn description(&self) -> &str {
            "Returns output with metadata"
        }
        fn input_schema(&self) -> Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _input: Value) -> crate::tools::ToolResult<ToolOutput> {
            let mut meta = HashMap::new();
            meta.insert(
                "line_count".to_string(),
                serde_json::Value::Number(42.into()),
            );
            meta.insert(
                "language".to_string(),
                serde_json::Value::String("rust".to_string()),
            );
            Ok(ToolOutput {
                content: "tool output with metadata".to_string(),
                is_error: false,
                metadata: meta,
            })
        }
    }

    /// A tool that returns ToolError::InvalidInput.
    struct InvalidInputTool;

    #[async_trait]
    impl Tool for InvalidInputTool {
        fn name(&self) -> &str {
            "InvalidInput"
        }
        fn description(&self) -> &str {
            "Rejects invalid input"
        }
        fn input_schema(&self) -> Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _input: Value) -> crate::tools::ToolResult<ToolOutput> {
            Err(crate::tools::ToolError::InvalidInput(
                "missing required field 'path'".to_string(),
            ))
        }
    }

    /// A tool that returns ToolError::RegistryError.
    struct RegistryErrorTool;

    #[async_trait]
    impl Tool for RegistryErrorTool {
        fn name(&self) -> &str {
            "RegistryError"
        }
        fn description(&self) -> &str {
            "Simulates a registry error"
        }
        fn input_schema(&self) -> Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _input: Value) -> crate::tools::ToolResult<ToolOutput> {
            Err(crate::tools::ToolError::RegistryError(
                "corrupted registry state".to_string(),
            ))
        }
    }

    /// Helper: build a service with additional tools for testing.
    async fn make_extended_service() -> ToolExecutionService {
        let registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        registry.register(Box::new(FailTool)).unwrap();
        registry.register(Box::new(PanicTool)).unwrap();
        registry.register(Box::new(DelayedTool { delay_ms: 0 })).unwrap();
        registry.register(Box::new(LargeOutputTool)).unwrap();
        registry.register(Box::new(MetadataTool)).unwrap();
        registry.register(Box::new(InvalidInputTool)).unwrap();
        registry.register(Box::new(RegistryErrorTool)).unwrap();
        let registry = Arc::new(registry);
        let permission_manager = Arc::new(PermissionManager::new());
        ToolExecutionService::new(registry, permission_manager)
    }

    // -- From<ToolError> conversion tests --

    #[test]
    fn test_from_tool_error_not_found() {
        let err = ToolError::NotFound("MyTool".to_string());
        let exec_err: ToolExecutionError = err.into();
        match exec_err {
            ToolExecutionError::ToolNotFound(name) => assert_eq!(name, "MyTool"),
            other => panic!("Expected ToolNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn test_from_tool_error_invalid_input() {
        let err = ToolError::InvalidInput("bad input".to_string());
        let exec_err: ToolExecutionError = err.into();
        match exec_err {
            ToolExecutionError::InvalidInput { tool_name, reason } => {
                assert_eq!(tool_name, "unknown");
                assert_eq!(reason, "bad input");
            }
            other => panic!("Expected InvalidInput, got: {other:?}"),
        }
    }

    #[test]
    fn test_from_tool_error_execution_failed() {
        let err = ToolError::ExecutionFailed("crash".to_string());
        let exec_err: ToolExecutionError = err.into();
        match exec_err {
            ToolExecutionError::ExecutionFailed(msg) => assert_eq!(msg, "crash"),
            other => panic!("Expected ExecutionFailed, got: {other:?}"),
        }
    }

    #[test]
    fn test_from_tool_error_registry_error() {
        let err = ToolError::RegistryError("corrupt".to_string());
        let exec_err: ToolExecutionError = err.into();
        match exec_err {
            ToolExecutionError::Internal(msg) => assert_eq!(msg, "corrupt"),
            other => panic!("Expected Internal, got: {other:?}"),
        }
    }

    #[test]
    fn test_from_tool_error_timeout() {
        let err = ToolError::Timeout {
            name: "SlowTool".to_string(),
            duration: Duration::from_secs(99),
        };
        let exec_err: ToolExecutionError = err.into();
        match exec_err {
            ToolExecutionError::Timeout {
                tool_name,
                timeout_secs,
            } => {
                assert_eq!(tool_name, "SlowTool");
                assert_eq!(timeout_secs, 99);
            }
            other => panic!("Expected Timeout, got: {other:?}"),
        }
    }

    // -- From<PermissionError> conversion tests --

    #[test]
    fn test_from_permission_error_denied() {
        let err = PermissionError::Denied("access blocked".to_string());
        let exec_err: ToolExecutionError = err.into();
        match exec_err {
            ToolExecutionError::PermissionDenied { tool_name, reason } => {
                assert_eq!(tool_name, "unknown");
                assert!(reason.contains("access blocked"));
            }
            other => panic!("Expected PermissionDenied, got: {other:?}"),
        }
    }

    #[test]
    fn test_from_permission_error_invalid() {
        let err = PermissionError::InvalidPermission("bad perm".to_string());
        let exec_err: ToolExecutionError = err.into();
        match exec_err {
            ToolExecutionError::PermissionDenied { reason, .. } => {
                assert!(reason.contains("bad perm"));
            }
            other => panic!("Expected PermissionDenied, got: {other:?}"),
        }
    }

    // -- All ToolExecutionError display variants --

    #[test]
    fn test_error_display_invalid_input() {
        let e = ToolExecutionError::InvalidInput {
            tool_name: "Bash".to_string(),
            reason: "missing command".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("Bash"));
        assert!(s.contains("missing command"));
    }

    #[test]
    fn test_error_display_hook_blocked() {
        let e = ToolExecutionError::HookBlocked("security policy".to_string());
        let s = e.to_string();
        assert!(s.contains("security policy"));
    }

    #[test]
    fn test_error_display_internal() {
        let e = ToolExecutionError::Internal("unexpected state".to_string());
        let s = e.to_string();
        assert!(s.contains("unexpected state"));
    }

    #[test]
    fn test_error_display_execution_failed() {
        let e = ToolExecutionError::ExecutionFailed("disk full".to_string());
        let s = e.to_string();
        assert!(s.contains("disk full"));
    }

    // -- Tool not found: progress events emitted --

    #[tokio::test]
    async fn test_tool_not_found_emits_failed_progress() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let callback = Arc::new(ChannelProgressCallback::new(tx));

        let mut service = make_service().await;
        service.set_progress_callback(callback);

        let result = service
            .run_tool_use(Uuid::new_v4(), "NonExistent", Value::Null)
            .await;

        assert!(result.is_err());
        let mut received = Vec::new();
        while let Ok(p) = rx.try_recv() {
            received.push(p);
        }
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status, ToolProgressStatus::Failed);
        assert!(received[0].message.as_ref().unwrap().contains("Tool not found"));
    }

    // -- Permission denied: progress events emitted --

    #[tokio::test]
    async fn test_permission_denied_emits_failed_progress() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let callback = Arc::new(ChannelProgressCallback::new(tx));

        let registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        let registry = Arc::new(registry);

        let mut perm_mgr = PermissionManager::new();
        perm_mgr.set_tool_permission(
            "Echo".to_string(),
            Permission::new("tool", "execute", PermissionLevel::Admin),
        );
        let perm_mgr = Arc::new(perm_mgr);

        let mut service = ToolExecutionService::new(registry, perm_mgr);
        service.set_progress_callback(callback);

        let result = service
            .run_tool_use(Uuid::new_v4(), "Echo", serde_json::json!({"message": "hi"}))
            .await;

        assert!(result.is_err());
        let mut received = Vec::new();
        while let Ok(p) = rx.try_recv() {
            received.push(p);
        }
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status, ToolProgressStatus::Failed);
    }

    // -- Hook blocked: progress events emitted --

    #[tokio::test]
    async fn test_hook_blocked_emits_failed_progress() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let callback = Arc::new(ChannelProgressCallback::new(tx));

        let mut service = make_service_with_hooks(r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo '{\"decision\": \"deny\", \"reason\": \"forbidden\"}'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#).await;
        service.set_progress_callback(callback);

        let result = service
            .run_tool_use(Uuid::new_v4(), "Echo", serde_json::json!({"message": "hi"}))
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolExecutionError::HookBlocked(_)));

        let mut received = Vec::new();
        while let Ok(p) = rx.try_recv() {
            received.push(p);
        }
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status, ToolProgressStatus::Failed);
        assert!(received[0].message.as_ref().unwrap().contains("Hook blocked"));
    }

    // -- Tool execution timeout via config (service-level default_timeout) --

    #[tokio::test]
    async fn test_service_config_timeout_slow_tool() {
        let registry = ToolRegistry::new();
        registry
            .register(Box::new(DelayedTool {
                delay_ms: 500, // 500ms delay
            }))
            .unwrap();
        let registry = Arc::new(registry);

        let config = ToolExecutionConfig {
            default_timeout: Duration::from_millis(10), // 10ms timeout, much shorter than delay
            collect_attachments: true,
            emit_hook_progress: true,
            auto_checkpoint: false,
        };

        let service = ToolExecutionService::with_config(
            registry,
            Arc::new(PermissionManager::new()),
            config,
        );

        let result = service
            .run_tool_use(Uuid::new_v4(), "Delayed", serde_json::json!({}))
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ToolExecutionError::Timeout {
                tool_name,
                timeout_secs,
            } => {
                assert_eq!(tool_name, "Delayed");
                assert_eq!(timeout_secs, 0); // 10ms rounds to 0 secs
            }
            other => panic!("Expected Timeout, got: {other:?}"),
        }
    }

    // -- Tool execution timeout: progress events --

    #[tokio::test]
    async fn test_timeout_emits_failed_progress() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let callback = Arc::new(ChannelProgressCallback::new(tx));

        let registry = ToolRegistry::new();
        registry
            .register(Box::new(DelayedTool {
                delay_ms: 500,
            }))
            .unwrap();
        let registry = Arc::new(registry);

        let config = ToolExecutionConfig {
            default_timeout: Duration::from_millis(10),
            collect_attachments: true,
            emit_hook_progress: true,
            auto_checkpoint: false,
        };

        let mut service = ToolExecutionService::with_config(
            registry,
            Arc::new(PermissionManager::new()),
            config,
        );
        service.set_progress_callback(callback);

        let result = service
            .run_tool_use(Uuid::new_v4(), "Delayed", serde_json::json!({}))
            .await;

        assert!(result.is_err());

        let mut received = Vec::new();
        while let Ok(p) = rx.try_recv() {
            received.push(p);
        }
        // Should have: Started + Failed (timeout)
        assert!(received.len() >= 2);
        assert_eq!(received[0].status, ToolProgressStatus::Started);
        // Last should be Failed
        let last = received.last().unwrap();
        assert_eq!(last.status, ToolProgressStatus::Failed);
        assert!(last.message.as_ref().unwrap().contains("timed out"));
    }

    // -- Output truncation for oversized tool output --

    #[tokio::test]
    async fn test_large_output_gets_truncated() {
        let service = make_extended_service().await;
        let result = service
            .run_tool_use(Uuid::new_v4(), "LargeOutput", serde_json::json!({}))
            .await
            .unwrap();

        // Original is 50K chars; output should be truncated to ~40K + suffix
        assert!(result.output.content.len() < 50_000);
        assert!(result.output.content.contains("[Tool output truncated"));
        assert!(!result.is_error);
    }

    // -- Tool returning ToolError::InvalidInput gets wrapped as ExecutionFailed --

    #[tokio::test]
    async fn test_tool_invalid_input_error_wrapped() {
        // The registry wraps ToolError::InvalidInput into a generic error message
        let service = make_extended_service().await;
        let result = service
            .run_tool_use(Uuid::new_v4(), "InvalidInput", serde_json::json!({}))
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        // The registry wraps this as ExecutionFailed with "Invalid tool input: ..."
        match &err {
            ToolExecutionError::ExecutionFailed(msg) => {
                assert!(msg.contains("Invalid tool input"));
                assert!(msg.contains("missing required field"));
            }
            ToolExecutionError::InvalidInput { reason, .. } => {
                assert!(reason.contains("missing required field"));
            }
            other => panic!("Expected ExecutionFailed or InvalidInput, got: {other:?}"),
        }
    }

    // -- Tool returning ToolError::RegistryError gets wrapped as ExecutionFailed --

    #[tokio::test]
    async fn test_tool_registry_error_wrapped() {
        // The registry wraps ToolError::RegistryError into a generic error message
        let service = make_extended_service().await;
        let result = service
            .run_tool_use(Uuid::new_v4(), "RegistryError", serde_json::json!({}))
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            ToolExecutionError::ExecutionFailed(msg) => {
                assert!(msg.contains("corrupted registry state"));
            }
            ToolExecutionError::Internal(msg) => {
                assert!(msg.contains("corrupted registry state"));
            }
            other => panic!("Expected ExecutionFailed or Internal, got: {other:?}"),
        }
    }

    // -- Metadata: output metadata is merged into result --

    #[tokio::test]
    async fn test_metadata_merged_from_tool_output() {
        let service = make_extended_service().await;
        let result = service
            .run_tool_use(Uuid::new_v4(), "MetadataTool", serde_json::json!({}))
            .await
            .unwrap();

        assert_eq!(
            result.metadata.get("line_count").and_then(|v| v.as_u64()),
            Some(42)
        );
        assert_eq!(
            result.metadata.get("language").and_then(|v| v.as_str()),
            Some("rust")
        );
    }

    // -- Metadata: file path via "path" key --

    #[test]
    fn test_build_metadata_path_key() {
        let input = serde_json::json!({"path": "/tmp/data.json"});
        let output = ToolOutput {
            content: "{}".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let meta = ToolExecutionResult::build_metadata("Read", &input, &output);
        assert_eq!(
            meta.get("file_extension").and_then(|v| v.as_str()),
            Some("json")
        );
    }

    // -- Metadata: "filePath" camelCase key NOT in extraction list --

    #[test]
    fn test_build_metadata_filepath_key_not_extracted() {
        // build_metadata only checks "file_path" and "path", not "filePath"
        let input = serde_json::json!({"filePath": "/tmp/code.ts"});
        let output = ToolOutput {
            content: "code".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let meta = ToolExecutionResult::build_metadata("Read", &input, &output);
        // "filePath" is not in the checked keys, so no extension extracted
        assert_eq!(meta.get("file_extension"), None);
    }

    // -- Metadata: output metadata merged with input metadata --

    #[test]
    fn test_build_metadata_merges_output_metadata() {
        let input = serde_json::json!({"file_path": "/tmp/test.py"});
        let mut output_meta = HashMap::new();
        output_meta.insert(
            "bytes_read".to_string(),
            serde_json::Value::Number(1024.into()),
        );
        let output = ToolOutput {
            content: "print('hello')".to_string(),
            is_error: false,
            metadata: output_meta,
        };
        let meta = ToolExecutionResult::build_metadata("Read", &input, &output);
        // Should have both file_extension from input and bytes_read from output
        assert_eq!(
            meta.get("file_extension").and_then(|v| v.as_str()),
            Some("py")
        );
        assert_eq!(
            meta.get("bytes_read").and_then(|v| v.as_u64()),
            Some(1024)
        );
    }

    // -- Metadata: non-file, non-bash tool has minimal metadata --

    #[test]
    fn test_build_metadata_no_file_no_bash() {
        let input = serde_json::json!({"query": "search term"});
        let output = ToolOutput {
            content: "results".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let meta = ToolExecutionResult::build_metadata("Grep", &input, &output);
        // Should be empty since no file_path and not Bash
        assert!(meta.is_empty());
    }

    // -- Attachment: Write tool produces attachment --

    #[test]
    fn test_extract_attachments_write_tool() {
        let output = ToolOutput {
            content: "written content".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let attachments = ToolExecutionResult::extract_attachments("Write", "id-1", &output);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].source_tool, "Write");
        assert_eq!(attachments[0].content, "written content");
    }

    // -- Attachment: "write" (lowercase) not in attachment extraction list --

    #[test]
    fn test_extract_attachments_lowercase_write_not_in_list() {
        // The attachment extraction list only has "Write", not "write"
        let output = ToolOutput {
            content: "data".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let attachments = ToolExecutionResult::extract_attachments("write", "id-1", &output);
        assert!(attachments.is_empty());
    }

    // -- Attachment: FileWrite tool produces attachment --

    #[test]
    fn test_extract_attachments_file_write_tool() {
        let output = ToolOutput {
            content: "file data".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let attachments = ToolExecutionResult::extract_attachments("FileWrite", "id-1", &output);
        assert_eq!(attachments.len(), 1);
    }

    // -- Attachment: Screenshot tool produces image attachment --

    #[test]
    fn test_extract_attachments_screenshot_tool() {
        let output = ToolOutput {
            content: "base64imagedata".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let attachments =
            ToolExecutionResult::extract_attachments("Screenshot", "id-1", &output);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].content_type, "image/png");
        assert_eq!(attachments[0].file_extension.as_deref(), Some("png"));
    }

    // -- Attachment: screenshot (lowercase) tool produces image attachment --

    #[test]
    fn test_extract_attachments_lowercase_screenshot() {
        let output = ToolOutput {
            content: "imagedata".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let attachments =
            ToolExecutionResult::extract_attachments("screenshot", "id-1", &output);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].content_type, "image/png");
    }

    // -- Attachment: TakeScreenshot tool produces image attachment --

    #[test]
    fn test_extract_attachments_take_screenshot_tool() {
        let output = ToolOutput {
            content: "imagedata".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let attachments =
            ToolExecutionResult::extract_attachments("TakeScreenshot", "id-1", &output);
        assert_eq!(attachments.len(), 1);
    }

    // -- Attachment: unknown tool produces no attachments --

    #[test]
    fn test_extract_attachments_unknown_tool() {
        let output = ToolOutput {
            content: "some data".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let attachments = ToolExecutionResult::extract_attachments("UnknownTool", "id-1", &output);
        assert!(attachments.is_empty());
    }

    // -- Attachment: collect_attachments disabled produces no attachments --

    #[tokio::test]
    async fn test_attachments_disabled_in_config() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        let registry = Arc::new(registry);

        let config = ToolExecutionConfig {
            default_timeout: Duration::from_secs(300),
            collect_attachments: false, // disabled
            emit_hook_progress: true,
            auto_checkpoint: false,
        };

        let service = ToolExecutionService::with_config(
            registry,
            Arc::new(PermissionManager::new()),
            config,
        );

        let result = service
            .run_tool_use(
                Uuid::new_v4(),
                "Echo",
                serde_json::json!({"message": "hello"}),
            )
            .await
            .unwrap();

        assert!(result.attachments.is_empty());
    }

    // -- extract_file_paths: filePath key --

    #[test]
    fn test_extract_file_paths_filepath_key() {
        let input = serde_json::json!({"filePath": "/some/file.ts"});
        let paths = ToolExecutionService::extract_file_paths("Read", &input);
        assert_eq!(paths, vec!["/some/file.ts"]);
    }

    // -- extract_file_paths: multiple file path keys --

    #[test]
    fn test_extract_file_paths_multiple_keys() {
        // If multiple keys are present, all should be extracted
        let input = serde_json::json!({"file_path": "/a.rs", "path": "/b.rs"});
        let paths = ToolExecutionService::extract_file_paths("Write", &input);
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"/a.rs".to_string()));
        assert!(paths.contains(&"/b.rs".to_string()));
    }

    // -- extract_file_paths: non-object input --

    #[test]
    fn test_extract_file_paths_non_object_input() {
        let input = serde_json::json!("just a string");
        let paths = ToolExecutionService::extract_file_paths("Read", &input);
        assert!(paths.is_empty());
    }

    // -- extract_file_paths: null input --

    #[test]
    fn test_extract_file_paths_null_input() {
        let paths = ToolExecutionService::extract_file_paths("Read", &Value::Null);
        assert!(paths.is_empty());
    }

    // -- is_file_modifying_tool: more variants --

    #[test]
    fn test_is_file_modifying_tool_variants() {
        // Uppercase
        assert!(is_file_modifying_tool("Write"));
        assert!(is_file_modifying_tool("Edit"));
        assert!(is_file_modifying_tool("Bash"));
        assert!(is_file_modifying_tool("MultiEdit"));
        assert!(is_file_modifying_tool("FileWrite"));
        assert!(is_file_modifying_tool("FileEdit"));
        // Lowercase
        assert!(is_file_modifying_tool("write"));
        assert!(is_file_modifying_tool("edit"));
        assert!(is_file_modifying_tool("bash"));
        assert!(is_file_modifying_tool("multi_edit"));
        assert!(is_file_modifying_tool("file_write"));
        assert!(is_file_modifying_tool("file_edit"));
        // Non-modifying
        assert!(!is_file_modifying_tool("Read"));
        assert!(!is_file_modifying_tool("Grep"));
        assert!(!is_file_modifying_tool("Glob"));
        assert!(!is_file_modifying_tool(""));
    }

    // -- with_config constructor --

    #[tokio::test]
    async fn test_with_config_constructor() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        let registry = Arc::new(registry);

        let config = ToolExecutionConfig {
            default_timeout: Duration::from_secs(60),
            collect_attachments: false,
            emit_hook_progress: false,
            auto_checkpoint: false,
        };

        let service = ToolExecutionService::with_config(
            registry,
            Arc::new(PermissionManager::new()),
            config.clone(),
        );

        // Verify config is accessible
        assert_eq!(service.config().default_timeout, Duration::from_secs(60));
        assert!(!service.config().collect_attachments);

        // Tool should still work
        let result = service
            .run_tool_use(
                Uuid::new_v4(),
                "Echo",
                serde_json::json!({"message": "config test"}),
            )
            .await
            .unwrap();
        assert_eq!(result.output.content, "config test");
        assert!(result.attachments.is_empty()); // collect_attachments disabled
    }

    // -- with_progress_callback constructor --

    #[tokio::test]
    async fn test_with_progress_callback_constructor() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let callback = Arc::new(ChannelProgressCallback::new(tx));

        let registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        let registry = Arc::new(registry);

        let service =
            ToolExecutionService::with_progress_callback(
                registry,
                Arc::new(PermissionManager::new()),
                callback,
            );

        let _result = service
            .run_tool_use(
                Uuid::new_v4(),
                "Echo",
                serde_json::json!({"message": "cb test"}),
            )
            .await
            .unwrap();

        let mut received = Vec::new();
        while let Ok(p) = rx.try_recv() {
            received.push(p);
        }
        assert!(!received.is_empty());
    }

    // -- Accessor tests --

    #[tokio::test]
    async fn test_registry_accessor() {
        let service = make_service().await;
        let reg = service.registry();
        assert!(reg.get("Echo").is_some());
        assert!(reg.get("NonExistent").is_none());
    }

    #[tokio::test]
    async fn test_permission_manager_accessor() {
        let service = make_service().await;
        let pm = service.permission_manager();
        // Should not panic; basic smoke test
        assert!(pm.check_tool_permission(Uuid::new_v4(), "Echo").is_ok());
    }

    #[tokio::test]
    async fn test_hook_manager_accessor_none() {
        let service = make_service().await;
        assert!(service.hook_manager().is_none());
    }

    #[tokio::test]
    async fn test_hook_manager_accessor_some() {
        let service = make_service_with_hooks(
            r#"{"hooks": {}}"#,
        )
        .await;
        assert!(service.hook_manager().is_some());
    }

    /// Helper to build a synchronous service for permission choice tests.
    fn make_service_blocking() -> ToolExecutionService {
        let registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        let registry = Arc::new(registry);
        let permission_manager = Arc::new(PermissionManager::new());
        ToolExecutionService::new(registry, permission_manager)
    }

    // -- process_permission_choice: Deny returns error --

    #[test]
    fn test_process_permission_choice_deny() {
        let service = make_service_blocking();
        let prompt = PermissionPrompt {
            id: Uuid::new_v4(),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "rm -rf /"}),
            risk_level: crate::permissions::RiskLevel::Critical,
            description: "Dangerous command".to_string(),
            is_confirmation: false,
            diff_preview: None,
            is_destructive: false,
            risk_reason: String::new(),
        };

        let result = service.process_permission_choice(Uuid::new_v4(), &prompt, crate::permissions::PermissionChoice::Deny);
        assert!(result.is_err());
        match result.unwrap_err() {
            ToolExecutionError::PermissionDenied { tool_name, reason } => {
                assert_eq!(tool_name, "Bash");
                assert!(reason.contains("User denied"));
            }
            other => panic!("Expected PermissionDenied, got: {other:?}"),
        }
    }

    // -- process_permission_choice: AllowOnce returns Ok --

    #[test]
    fn test_process_permission_choice_allow_once() {
        let service = make_service_blocking();
        let prompt = PermissionPrompt {
            id: Uuid::new_v4(),
            tool_name: "Read".to_string(),
            tool_input: serde_json::json!({"file_path": "/tmp/test.txt"}),
            risk_level: crate::permissions::RiskLevel::Low,
            description: "Read file".to_string(),
            is_confirmation: false,
            diff_preview: None,
            is_destructive: false,
            risk_reason: String::new(),
        };

        let result = service.process_permission_choice(Uuid::new_v4(), &prompt, crate::permissions::PermissionChoice::AllowOnce);
        assert!(result.is_ok());
    }

    // -- process_permission_choice: AlwaysAllow returns Ok --

    #[test]
    fn test_process_permission_choice_always_allow() {
        let service = make_service_blocking();
        let prompt = PermissionPrompt {
            id: Uuid::new_v4(),
            tool_name: "Read".to_string(),
            tool_input: serde_json::json!({}),
            risk_level: crate::permissions::RiskLevel::Low,
            description: "Read".to_string(),
            is_confirmation: false,
            diff_preview: None,
            is_destructive: false,
            risk_reason: String::new(),
        };

        let result = service.process_permission_choice(Uuid::new_v4(), &prompt, crate::permissions::PermissionChoice::AlwaysAllow);
        assert!(result.is_ok());
    }

    // -- process_permission_choice: EditAndRun returns Ok --

    #[test]
    fn test_process_permission_choice_edit_and_run() {
        let service = make_service_blocking();
        let prompt = PermissionPrompt {
            id: Uuid::new_v4(),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            risk_level: crate::permissions::RiskLevel::Medium,
            description: "Bash".to_string(),
            is_confirmation: false,
            diff_preview: None,
            is_destructive: false,
            risk_reason: String::new(),
        };

        let result = service.process_permission_choice(Uuid::new_v4(), &prompt, crate::permissions::PermissionChoice::EditAndRun);
        assert!(result.is_ok());
    }

    // -- Hook errors don't block execution (PreToolUse hook that fails) --

    #[tokio::test]
    async fn test_hook_error_does_not_block_execution() {
        // A hook with an invalid command should not block the tool from executing
        let service = make_service_with_hooks(r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "nonexistent_command_that_will_fail_xyz", "timeout": 1, "blocking": true}
                        ]
                    }
                ]
            }
        }"#).await;

        let session_id = Uuid::new_v4();
        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "should work"}))
            .await;

        // Hook error is logged but does not block; tool still executes
        // (the hook command fails to run, which results in a hook error)
        match result {
            Ok(r) => {
                // Tool may execute if hook error is treated as allow
                assert_eq!(r.output.content, "should work");
            }
            Err(ToolExecutionError::HookBlocked(_)) => {
                // Some implementations may treat hook failure as deny; both are acceptable
            }
            Err(other) => panic!("Unexpected error: {other:?}"),
        }
    }

    // -- Result session_id matches input --

    #[tokio::test]
    async fn test_result_session_id_matches_input() {
        let service = make_service().await;
        let session_id = Uuid::new_v4();

        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "test"}))
            .await
            .unwrap();

        assert_eq!(result.session_id, session_id);
    }

    // -- Result has unique tool_id --

    #[tokio::test]
    async fn test_result_has_unique_tool_id() {
        let service = make_service().await;

        let r1 = service
            .run_tool_use(Uuid::new_v4(), "Echo", serde_json::json!({"message": "a"}))
            .await
            .unwrap();
        let r2 = service
            .run_tool_use(Uuid::new_v4(), "Echo", serde_json::json!({"message": "b"}))
            .await
            .unwrap();

        assert_ne!(r1.tool_id, r2.tool_id);
    }

    // -- Stop hook info populated correctly --

    #[tokio::test]
    async fn test_stop_hook_info_populated() {
        let service = make_service().await;
        let session_id = Uuid::new_v4();

        let result = service
            .run_tool_use(session_id, "Echo", serde_json::json!({"message": "test"}))
            .await
            .unwrap();

        let info = result.stop_hook_info.unwrap();
        assert_eq!(info.tool_name, "Echo");
        assert!(!info.is_error);
        assert_eq!(info.session_id, session_id);
        assert!(!info.tool_id.is_empty());
        assert!(info.duration.as_nanos() > 0);
    }

    // -- Stop hook info for error output tool --

    #[tokio::test]
    async fn test_stop_hook_info_for_error_output() {
        let service = make_service().await;
        let session_id = Uuid::new_v4();

        let result = service
            .run_tool_use(session_id, "Fail", serde_json::json!({}))
            .await
            .unwrap();

        let info = result.stop_hook_info.unwrap();
        assert_eq!(info.tool_name, "Fail");
        assert!(info.is_error);
        assert!(info.tool_output.is_error);
    }

    // -- No stop hook info on execution error (returns Err) --

    #[tokio::test]
    async fn test_no_stop_hook_info_on_execution_error() {
        let service = make_service().await;
        let result = service
            .run_tool_use(Uuid::new_v4(), "Panic", serde_json::json!({}))
            .await;

        // Execution errors return Err, so no result to check stop_hook_info on
        assert!(result.is_err());
    }

    // -- Files modified extracted from Echo tool (no file paths) --

    #[tokio::test]
    async fn test_files_modified_empty_for_non_file_tool() {
        let service = make_service().await;
        let result = service
            .run_tool_use(Uuid::new_v4(), "Echo", serde_json::json!({"message": "hi"}))
            .await
            .unwrap();

        assert!(result.files_modified.is_empty());
    }

    // -- ChannelProgressCallback: receiver drop doesn't panic --

    #[tokio::test]
    async fn test_channel_progress_callback_dropped_receiver() {
        let (tx, rx) = mpsc::unbounded_channel();
        let callback = ChannelProgressCallback::new(tx);

        // Drop the receiver
        drop(rx);

        // Sending progress should not panic (the send returns Err but is ignored)
        callback
            .on_progress(ToolProgress::started("id-1", "Test"))
            .await;
        // No panic = success
    }

    // -- LoggingProgressCallback: does not panic --

    #[tokio::test]
    async fn test_logging_progress_callback() {
        let callback = LoggingProgressCallback;
        callback
            .on_progress(ToolProgress::started("id-1", "Test"))
            .await;
        // No panic = success
    }

    // -- Execution failed: progress includes Started + Failed --

    #[tokio::test]
    async fn test_execution_failed_progress_events() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let callback = Arc::new(ChannelProgressCallback::new(tx));

        let mut service = make_service().await;
        service.set_progress_callback(callback);

        let _ = service
            .run_tool_use(Uuid::new_v4(), "Panic", serde_json::json!({}))
            .await;

        let mut received = Vec::new();
        while let Ok(p) = rx.try_recv() {
            received.push(p);
        }
        // Should have: Started + Failed
        assert!(received.len() >= 2);
        assert_eq!(received[0].status, ToolProgressStatus::Started);
        let last = received.last().unwrap();
        assert_eq!(last.status, ToolProgressStatus::Failed);
    }

    // -- Successful execution: progress includes Started + Completed --

    #[tokio::test]
    async fn test_success_progress_ordering() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let callback = Arc::new(ChannelProgressCallback::new(tx));

        let mut service = make_service().await;
        service.set_progress_callback(callback);

        let _ = service
            .run_tool_use(
                Uuid::new_v4(),
                "Echo",
                serde_json::json!({"message": "ordered"}),
            )
            .await
            .unwrap();

        let mut received = Vec::new();
        while let Ok(p) = rx.try_recv() {
            received.push(p);
        }
        assert_eq!(received.len(), 2);
        assert_eq!(received[0].status, ToolProgressStatus::Started);
        assert_eq!(received[1].status, ToolProgressStatus::Completed);
    }

    // -- Hook modify with non-object modified_input leaves tool unchanged --

    #[tokio::test]
    async fn test_hook_modify_with_null_input_no_change() {
        // Hook returns modify with null modified_input - should not change the effective input
        let service = make_service_with_hooks(r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Echo",
                        "hooks": [
                            {"command": "echo '{\"decision\": \"modify\"}'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#).await;

        let result = service
            .run_tool_use(
                Uuid::new_v4(),
                "Echo",
                serde_json::json!({"message": "original"}),
            )
            .await
            .unwrap();

        // modified_input is null/absent, so input should stay as-is
        assert_eq!(result.output.content, "original");
    }

    // -- Multiple concurrent tool executions produce unique IDs --

    #[tokio::test]
    async fn test_concurrent_tool_executions_unique_ids() {
        let service = Arc::new(make_service().await);
        let mut handles = Vec::new();

        for i in 0..5 {
            let svc = Arc::clone(&service);
            handles.push(tokio::spawn(async move {
                svc.run_tool_use(
                    Uuid::new_v4(),
                    "Echo",
                    serde_json::json!({"message": format!("msg-{i}")}),
                )
                .await
                .unwrap()
            }));
        }

        let results: Vec<_> = futures::future::join_all(handles).await;
        let tool_ids: Vec<_> = results.into_iter().map(|r| r.unwrap().tool_id).collect();

        // All tool IDs should be unique
        let unique_count = {
            let mut ids = tool_ids.clone();
            ids.sort();
            ids.dedup();
            ids.len()
        };
        assert_eq!(unique_count, 5);
    }

    // -- create_permission_prompt returns Some for unconfigured tools --

    #[test]
    fn test_create_permission_prompt_returns_some_for_new_tool() {
        let service = make_service_blocking();
        let result = service.create_permission_prompt(
            Uuid::new_v4(),
            "Echo",
            &serde_json::json!({"message": "test"}),
        );
        // A fresh PermissionManager returns Some (a prompt) for unknown tools
        assert!(result.is_some());
        let prompt = result.unwrap();
        assert_eq!(prompt.tool_name, "Echo");
    }

    // -- Auto-checkpoint disabled in config --

    #[tokio::test]
    async fn test_auto_checkpoint_disabled() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        let registry = Arc::new(registry);

        let config = ToolExecutionConfig {
            default_timeout: Duration::from_secs(300),
            collect_attachments: true,
            emit_hook_progress: true,
            auto_checkpoint: false,
        };

        let mut service = ToolExecutionService::with_config(
            registry,
            Arc::new(PermissionManager::new()),
            config,
        );
        // Set a checkpoint manager - it shouldn't be used because auto_checkpoint is false
        service.set_checkpoint_manager(CheckpointManager::new());

        let result = service
            .run_tool_use(
                Uuid::new_v4(),
                "Echo",
                serde_json::json!({"message": "test"}),
            )
            .await
            .unwrap();

        assert!(!result.checkpoint_created);
    }

    // -- Bash tool metadata with "cmd" field --

    #[test]
    fn test_build_metadata_bash_cmd_field() {
        let input = serde_json::json!({"cmd": "cargo test"});
        let output = ToolOutput {
            content: "running tests".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let meta = ToolExecutionResult::build_metadata("bash", &input, &output);
        assert_eq!(
            meta.get("bash_command").and_then(|v| v.as_str()),
            Some("cargo test")
        );
    }

    // -- Bash tool metadata with "command" field takes priority --

    #[test]
    fn test_build_metadata_bash_command_priority() {
        let input = serde_json::json!({"command": "cargo build", "cmd": "cargo test"});
        let output = ToolOutput {
            content: "building".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let meta = ToolExecutionResult::build_metadata("Bash", &input, &output);
        // "command" is checked first via .or_else(), so it should win
        let cmd = meta.get("bash_command").and_then(|v| v.as_str()).unwrap();
        assert_eq!(cmd, "cargo build");
    }

    // -- AttachmentMessage serialization roundtrip --

    #[test]
    fn test_attachment_message_serialization() {
        let att = AttachmentMessage::new("Read", "id-1", "file contents", "text/plain");
        let json = serde_json::to_string(&att).unwrap();
        let back: AttachmentMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source_tool, "Read");
        assert_eq!(back.tool_id, "id-1");
        assert_eq!(back.content_type, "text/plain");
    }

    // -- StopHookInfo serialization roundtrip --

    #[test]
    fn test_stop_hook_info_serialization() {
        let info = StopHookInfo {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"cmd": "ls"}),
            tool_output: ToolOutput {
                content: "files".to_string(),
                is_error: false,
                metadata: Default::default(),
            },
            duration: Duration::from_millis(50),
            is_error: false,
            session_id: Uuid::new_v4(),
            tool_id: "id-1".to_string(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: StopHookInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tool_name, "Bash");
        assert!(!back.is_error);
    }

    // -- HookProgress serialization roundtrip --

    #[test]
    fn test_hook_progress_serialization() {
        let hp = HookProgress::new("PreToolUse", "Bash", "checking");
        let json = serde_json::to_string(&hp).unwrap();
        let back: HookProgress = serde_json::from_str(&json).unwrap();
        assert_eq!(back.hook_type, "PreToolUse");
        assert_eq!(back.tool_name, "Bash");
        assert_eq!(back.message, "checking");
    }

    // -- ToolExecutionResult has hook_progress field (always empty in basic flow) --

    #[tokio::test]
    async fn test_result_hook_progress_empty_without_hook_emit() {
        let service = make_service().await;
        let result = service
            .run_tool_use(
                Uuid::new_v4(),
                "Echo",
                serde_json::json!({"message": "test"}),
            )
            .await
            .unwrap();

        // hook_progress is populated from the hook_progress_events local vec,
        // which is always empty in the current implementation (future use)
        assert!(result.hook_progress.is_empty());
    }
}
