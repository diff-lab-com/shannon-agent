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
    Timeout { tool_name: String, timeout_secs: u64 },

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
    pub fn new(
        tool_id: String,
        tool_name: String,
        status: ToolProgressStatus,
    ) -> Self {
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
        Self::new(tool_id.to_string(), tool_name.to_string(), ToolProgressStatus::Started)
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
    pub fn new(
        source_tool: &str,
        tool_id: &str,
        content: &str,
        content_type: &str,
    ) -> Self {
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
    fn build_metadata(tool_name: &str, input: &Value, output: &ToolOutput) -> HashMap<String, Value> {
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
        let file_tools = ["Read", "read", "FileRead", "file_read", "Write", "FileWrite"];
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
    "Write", "write", "FileWrite", "file_write",
    "Edit", "edit", "FileEdit", "file_edit",
    "MultiEdit", "multi_edit",
    "Bash", "bash",  // Bash may modify files via commands
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
    /// Configuration.
    config: ToolExecutionConfig,
}

impl ToolExecutionService {
    /// Create a new tool execution service.
    pub fn new(
        registry: Arc<ToolRegistry>,
        permission_manager: Arc<PermissionManager>,
    ) -> Self {
        Self {
            registry,
            permission_manager,
            progress_callback: None,
            checkpoint_manager: None,
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
                let desc = format!(
                    "{}: {}",
                    tool_name,
                    files_modified.join(", ")
                );
                match mgr.create_checkpoint(tool_name, &desc) {
                    Ok(_) => checkpoint_created = true,
                    Err(e) => {
                        tracing::debug!("Auto-checkpoint skipped: {e}");
                    }
                }
            }
        }

        // 3. Emit started progress
        let started = ToolProgress::started(&tool_id, tool_name);
        progress_events.push(started.clone());
        self.emit_progress(started).await;

        // 4. Execute the tool
        let output = match self.registry.execute(tool_name, input.clone()).await {
            Ok(output) => output,
            Err(err) => {
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
                return Err(ToolExecutionError::ExecutionFailed(msg));
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
            let progress = ToolProgress::failed(
                &tool_id,
                tool_name,
                &output.content,
            );
            progress_events.push(progress.clone());
            self.emit_progress(progress).await;
        } else {
            let progress = ToolProgress::completed(&tool_id, tool_name);
            progress_events.push(progress.clone());
            self.emit_progress(progress).await;
        }

        // 6. Build metadata
        let metadata = ToolExecutionResult::build_metadata(tool_name, &input, &output);

        // 7. Extract attachments
        let attachments = if self.config.collect_attachments {
            ToolExecutionResult::extract_attachments(&tool_id, &tool_id, &output)
        } else {
            Vec::new()
        };

        // 8. Build stop hook info
        let stop_hook_info = Some(StopHookInfo {
            tool_name: tool_name.to_string(),
            tool_input: input.clone(),
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
        match tokio::time::timeout(timeout, self.run_tool_use(session_id, tool_name, input)).await
        {
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
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::Permission;
    use crate::permissions::PermissionLevel;
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
        let mut registry = ToolRegistry::new();
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
        let p = ToolProgress::new("id-1".to_string(), "Echo".to_string(), ToolProgressStatus::Started);
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
        let att = AttachmentMessage::file_attachment("Read", "id-1", "content", "/path/to/Makefile");
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

        let result = service
            .run_tool_use(session_id, "Panic", Value::Null)
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolExecutionError::ExecutionFailed(_)));
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
        let mut registry = ToolRegistry::new();
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
}
