//! # Tool Hooks
//!
//! A tool hook system that wraps tool execution with before/after hooks.
//! Based on Claude Code's `toolHooks.ts`, this module provides a composable
//! chain of hooks that can intercept, modify, or deny tool calls.
//!
//! ## Hook Types
//!
//! - **PreToolUse**: Executed before a tool runs. Can Allow, Deny, or Ask.
//! - **PostToolUse**: Executed after a tool completes. Can inspect results.
//!
//! ## Built-in Hooks
//!
//! - [`PermissionToolHook`]: Checks permission rules before tool execution.
//! - [`LoggingToolHook`]: Logs tool calls for debugging and auditing.
//! - [`StopOnDenyHook`]: Stops the execution chain on the first Deny result.
//!
//! ## Example
//!
//! ```rust,ignore
//! use shannon_core::tool_hooks::*;
//! use serde_json::json;
//! use std::sync::Arc;
//! use std::sync::atomic::AtomicBool;
//!
//! let mut chain = ToolHookChain::new();
//! chain.add_hook(Box::new(LoggingToolHook::new()));
//! chain.add_hook(Box::new(PermissionToolHook::new()));
//!
//! let ctx = ToolHookContext::pre(
//!     "Bash".to_string(),
//!     "tool-123".to_string(),
//!     json!({"command": "ls"}),
//! );
//!
//! let results = chain.run_pre_hooks(&ctx).unwrap();
//! let decision = ToolHookChain::resolve_permission(&results);
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Errors that can occur during tool hook operations.
#[derive(Error, Debug)]
pub enum ToolHookError {
    /// A hook denied the tool execution.
    #[error("Tool execution denied by hook: {0}")]
    Denied(String),

    /// A hook returned an error during execution.
    #[error("Hook execution error: {0}")]
    ExecutionFailed(String),

    /// The hook type is not recognized.
    #[error("Invalid hook type: {0}")]
    InvalidHookType(String),

    /// Configuration error for a hook.
    #[error("Hook configuration error: {0}")]
    Configuration(String),
}

/// The decision a tool hook makes about a tool execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolHookDecision {
    /// Allow the tool to proceed.
    Allow,
    /// Deny the tool execution.
    Deny,
    /// Ask the user for confirmation.
    Ask,
}

impl Default for ToolHookDecision {
    fn default() -> Self {
        Self::Allow
    }
}

impl std::fmt::Display for ToolHookDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Allow => write!(f, "Allow"),
            Self::Deny => write!(f, "Deny"),
            Self::Ask => write!(f, "Ask"),
        }
    }
}

/// Result returned by a single tool hook execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolHookResult {
    /// The decision made by this hook.
    pub decision: ToolHookDecision,
    /// Optional message explaining the decision.
    pub message: Option<String>,
    /// Optional modified input (for pre-hooks that transform arguments).
    pub updated_input: Option<Value>,
    /// Additional context strings to attach to the tool call.
    pub additional_contexts: Vec<String>,
    /// If true, prevent further hook processing in the chain.
    pub prevent_continuation: bool,
    /// Optional stop reason for aborting the entire agent loop.
    pub stop_reason: Option<String>,
}

impl ToolHookResult {
    /// Create a result that allows the tool to proceed.
    pub fn allow() -> Self {
        Self {
            decision: ToolHookDecision::Allow,
            message: None,
            updated_input: None,
            additional_contexts: Vec::new(),
            prevent_continuation: false,
            stop_reason: None,
        }
    }

    /// Create a result that denies the tool execution.
    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            decision: ToolHookDecision::Deny,
            message: Some(reason.into()),
            updated_input: None,
            additional_contexts: Vec::new(),
            prevent_continuation: true,
            stop_reason: None,
        }
    }

    /// Create a result that asks the user for confirmation.
    pub fn ask(reason: impl Into<String>) -> Self {
        Self {
            decision: ToolHookDecision::Ask,
            message: Some(reason.into()),
            updated_input: None,
            additional_contexts: Vec::new(),
            prevent_continuation: false,
            stop_reason: None,
        }
    }

    /// Create a result that allows with modified input.
    pub fn allow_with_input(updated_input: Value) -> Self {
        Self {
            decision: ToolHookDecision::Allow,
            message: None,
            updated_input: Some(updated_input),
            additional_contexts: Vec::new(),
            prevent_continuation: false,
            stop_reason: None,
        }
    }

    /// Add an additional context string to this result.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.additional_contexts.push(context.into());
        self
    }

    /// Set the stop reason on this result.
    pub fn with_stop_reason(mut self, reason: impl Into<String>) -> Self {
        self.stop_reason = Some(reason.into());
        self.prevent_continuation = true;
        self
    }

    /// Check if this result denies execution.
    pub fn is_denied(&self) -> bool {
        self.decision == ToolHookDecision::Deny
    }

    /// Check if this result requires user confirmation.
    pub fn is_ask(&self) -> bool {
        self.decision == ToolHookDecision::Ask
    }

    /// Check if further hooks in the chain should be skipped.
    pub fn should_stop(&self) -> bool {
        self.prevent_continuation
    }
}

/// Context provided to a tool hook during execution.
#[derive(Debug, Clone)]
pub struct ToolHookContext {
    /// The name of the tool being executed.
    pub tool_name: String,
    /// Unique identifier for this tool use.
    pub tool_use_id: String,
    /// The input/arguments for the tool.
    pub tool_input: Value,
    /// The output from the tool (only available in post-hooks).
    pub tool_output: Option<Value>,
    /// Whether the tool execution resulted in an error.
    pub is_error: bool,
    /// Shared abort signal that hooks can set to request cancellation.
    pub abort_signal: Arc<AtomicBool>,
}

impl ToolHookContext {
    /// Create a context for a pre-tool-use hook.
    pub fn pre(
        tool_name: String,
        tool_use_id: String,
        tool_input: Value,
    ) -> Self {
        Self {
            tool_name,
            tool_use_id,
            tool_input,
            tool_output: None,
            is_error: false,
            abort_signal: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a context for a post-tool-use hook.
    pub fn post(
        tool_name: String,
        tool_use_id: String,
        tool_input: Value,
        tool_output: Value,
        is_error: bool,
    ) -> Self {
        Self {
            tool_name,
            tool_use_id,
            tool_input,
            tool_output: Some(tool_output),
            is_error,
            abort_signal: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a post context from a pre context with the result.
    pub fn to_post(&self, tool_output: Value, is_error: bool) -> Self {
        Self {
            tool_name: self.tool_name.clone(),
            tool_use_id: self.tool_use_id.clone(),
            tool_input: self.tool_input.clone(),
            tool_output: Some(tool_output),
            is_error,
            abort_signal: self.abort_signal.clone(),
        }
    }

    /// Signal that the current operation should be aborted.
    pub fn abort(&self) {
        self.abort_signal.store(true, Ordering::SeqCst);
    }

    /// Check if the abort signal has been set.
    pub fn is_aborted(&self) -> bool {
        self.abort_signal.load(Ordering::SeqCst)
    }
}

/// Trait that all tool hooks must implement.
///
/// A tool hook can inspect, modify, or deny tool executions at pre- and post-use
/// lifecycle points. Hooks are composed into a [`ToolHookChain`] and executed
/// in order.
pub trait ToolHook: Send + Sync {
    /// Returns the hook type: `"PreToolUse"` or `"PostToolUse"`.
    fn hook_type(&self) -> &str;

    /// Returns an optional tool name filter. `None` means this hook applies to
    /// all tools. `Some("Bash")` means it only applies to the Bash tool.
    fn tool_filter(&self) -> Option<&str>;

    /// Execute the hook logic against the given context.
    fn execute(&self, ctx: &ToolHookContext) -> Result<ToolHookResult, ToolHookError>;

    /// Check if this hook applies to the given tool name.
    fn applies_to(&self, tool_name: &str) -> bool {
        match self.tool_filter() {
            Some(filter) => filter == tool_name,
            None => true,
        }
    }
}

/// A chain of tool hooks that are executed in order.
///
/// The chain supports both pre-tool-use and post-tool-use hooks. Hooks are
/// executed in insertion order. If any hook sets `prevent_continuation` on its
/// result, subsequent hooks in the chain are skipped.
pub struct ToolHookChain {
    hooks: Vec<Box<dyn ToolHook>>,
}

impl Default for ToolHookChain {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ToolHookChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolHookChain")
            .field("hook_count", &self.hooks.len())
            .finish()
    }
}

impl ToolHookChain {
    /// Create a new empty hook chain.
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Add a hook to the chain. Hooks are executed in insertion order.
    pub fn add_hook(&mut self, hook: Box<dyn ToolHook>) {
        debug!(
            "Added {} hook for tool filter: {:?}",
            hook.hook_type(),
            hook.tool_filter()
        );
        self.hooks.push(hook);
    }

    /// Add a hook to the chain (builder pattern).
    pub fn with_hook(mut self, hook: Box<dyn ToolHook>) -> Self {
        self.add_hook(hook);
        self
    }

    /// Run all pre-tool-use hooks in the chain.
    ///
    /// Returns a vector of results from each hook that was executed. If any hook
    /// sets `prevent_continuation`, subsequent hooks are skipped.
    pub fn run_pre_hooks(&self, ctx: &ToolHookContext) -> Result<Vec<ToolHookResult>, ToolHookError> {
        self.run_hooks(ctx, "PreToolUse")
    }

    /// Run all post-tool-use hooks in the chain.
    ///
    /// Returns a vector of results from each hook that was executed. If any hook
    /// sets `prevent_continuation`, subsequent hooks are skipped.
    pub fn run_post_hooks(&self, ctx: &ToolHookContext) -> Result<Vec<ToolHookResult>, ToolHookError> {
        self.run_hooks(ctx, "PostToolUse")
    }

    /// Internal hook runner that filters by hook type.
    fn run_hooks(
        &self,
        ctx: &ToolHookContext,
        expected_type: &str,
    ) -> Result<Vec<ToolHookResult>, ToolHookError> {
        let mut results = Vec::new();

        for hook in &self.hooks {
            // Skip hooks that don't match the expected type
            if hook.hook_type() != expected_type {
                continue;
            }

            // Skip hooks that don't apply to this tool
            if !hook.applies_to(&ctx.tool_name) {
                continue;
            }

            // Check abort signal before executing each hook
            if ctx.is_aborted() {
                debug!(
                    "Skipping {} hook for tool '{}' due to abort signal",
                    expected_type, ctx.tool_name
                );
                break;
            }

            debug!(
                "Running {} hook for tool '{}'",
                expected_type, ctx.tool_name
            );

            match hook.execute(ctx) {
                Ok(result) => {
                    debug!(
                        "Hook result for '{}': {}{}",
                        ctx.tool_name,
                        result.decision,
                        result.message.as_ref().map(|m| format!(" - {}", m)).unwrap_or_default()
                    );
                    results.push(result.clone());

                    // Check if we should stop processing further hooks
                    if result.should_stop() {
                        debug!(
                            "Hook chain stopped for tool '{}' (prevent_continuation=true)",
                            ctx.tool_name
                        );
                        break;
                    }
                }
                Err(e) => {
                    warn!(
                        "Hook error for tool '{}': {}",
                        ctx.tool_name, e
                    );
                    return Err(ToolHookError::ExecutionFailed(format!(
                        "Hook {} for tool '{}' failed: {}",
                        expected_type, ctx.tool_name, e
                    )));
                }
            }
        }

        Ok(results)
    }

    /// Resolve the final permission decision from a list of hook results.
    ///
    /// Resolution logic:
    /// - If any result is `Deny`, return `Deny`.
    /// - If any result is `Ask`, return `Ask`.
    /// - Otherwise, return `Allow`.
    ///
    /// The first `Deny` wins. If no deny but any `Ask`, the first `Ask` wins.
    pub fn resolve_permission(results: &[ToolHookResult]) -> ToolHookDecision {
        // First pass: check for any deny
        for result in results {
            if result.decision == ToolHookDecision::Deny {
                return ToolHookDecision::Deny;
            }
        }

        // Second pass: check for any ask
        for result in results {
            if result.decision == ToolHookDecision::Ask {
                return ToolHookDecision::Ask;
            }
        }

        ToolHookDecision::Allow
    }

    /// Collect the last updated input from a list of hook results.
    ///
    /// This is used to apply input modifications from pre-hooks. If multiple
    /// hooks modify the input, the last modification wins.
    pub fn collect_updated_input(results: &[ToolHookResult]) -> Option<Value> {
        let mut last_input = None;
        for result in results {
            if let Some(ref input) = result.updated_input {
                last_input = Some(input.clone());
            }
        }
        last_input
    }

    /// Collect all additional contexts from hook results.
    pub fn collect_contexts(results: &[ToolHookResult]) -> Vec<String> {
        let mut contexts = Vec::new();
        for result in results {
            contexts.extend(result.additional_contexts.iter().cloned());
        }
        contexts
    }

    /// Collect the first stop reason from hook results.
    pub fn find_stop_reason(results: &[ToolHookResult]) -> Option<String> {
        for result in results {
            if let Some(ref reason) = result.stop_reason {
                return Some(reason.clone());
            }
        }
        None
    }

    /// Get the number of hooks in the chain.
    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    /// Check if the chain has no hooks.
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// Get hooks filtered by type.
    pub fn hooks_by_type(&self, hook_type: &str) -> Vec<&dyn ToolHook> {
        self.hooks
            .iter()
            .filter(|h| h.hook_type() == hook_type)
            .map(|h| h.as_ref())
            .collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Built-in Hooks
// ─────────────────────────────────────────────────────────────────────────────

/// A pre-tool-use hook that checks permission rules before tool execution.
///
/// This hook integrates with the permission system to deny dangerous operations
/// or ask for confirmation on risky ones.
pub struct PermissionToolHook {
    /// Tools that are always denied regardless of input.
    denied_tools: Vec<String>,
    /// Tools that require confirmation before execution.
    confirmation_tools: Vec<String>,
    /// Tools that are always allowed without checks.
    allowed_tools: Vec<String>,
    /// Input patterns that should always be denied (checked against JSON-serialized input).
    denied_patterns: Vec<String>,
}

impl PermissionToolHook {
    /// Create a new permission hook with default rules.
    pub fn new() -> Self {
        Self {
            denied_tools: Vec::new(),
            confirmation_tools: vec!["Bash".to_string(), "Write".to_string(), "Edit".to_string()],
            allowed_tools: vec!["Read".to_string(), "Glob".to_string(), "Grep".to_string()],
            denied_patterns: Vec::new(),
        }
    }

    /// Add a tool to the always-denied list.
    pub fn deny_tool(mut self, tool_name: impl Into<String>) -> Self {
        self.denied_tools.push(tool_name.into());
        self
    }

    /// Add a tool to the confirmation-required list.
    pub fn require_confirmation(mut self, tool_name: impl Into<String>) -> Self {
        self.confirmation_tools.push(tool_name.into());
        self
    }

    /// Add a tool to the always-allowed list.
    pub fn allow_tool(mut self, tool_name: impl Into<String>) -> Self {
        self.allowed_tools.push(tool_name.into());
        self
    }

    /// Add an input pattern that should always be denied.
    pub fn deny_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.denied_patterns.push(pattern.into());
        self
    }
}

impl Default for PermissionToolHook {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolHook for PermissionToolHook {
    fn hook_type(&self) -> &str {
        "PreToolUse"
    }

    fn tool_filter(&self) -> Option<&str> {
        None // Applies to all tools
    }

    fn execute(&self, ctx: &ToolHookContext) -> Result<ToolHookResult, ToolHookError> {
        // Check if the tool is in the denied list
        if self.denied_tools.contains(&ctx.tool_name) {
            return Ok(ToolHookResult::deny(format!(
                "Tool '{}' is not permitted",
                ctx.tool_name
            )));
        }

        // Check denied input patterns
        let input_str = serde_json::to_string(&ctx.tool_input).unwrap_or_default();
        for pattern in &self.denied_patterns {
            if input_str.contains(pattern) {
                return Ok(ToolHookResult::deny(format!(
                    "Input matches denied pattern: {}",
                    pattern
                )));
            }
        }

        // Check if the tool is always allowed
        if self.allowed_tools.contains(&ctx.tool_name) {
            return Ok(ToolHookResult::allow());
        }

        // Check if the tool requires confirmation
        if self.confirmation_tools.contains(&ctx.tool_name) {
            return Ok(ToolHookResult::ask(format!(
                "Tool '{}' requires confirmation",
                ctx.tool_name
            )));
        }

        // Default: allow
        Ok(ToolHookResult::allow())
    }
}

/// A hook that logs tool calls for debugging and auditing.
///
/// This hook logs tool name, input, output, and timing information. It can be
/// configured as either a pre-hook or post-hook (or both, by adding two instances).
pub struct LoggingToolHook {
    /// Whether to include full tool input in logs.
    log_input: bool,
    /// Whether to include full tool output in logs.
    log_output: bool,
    /// Maximum length of logged input/output (0 = no truncation).
    max_log_length: usize,
    /// Optional tag for identifying this logger in output.
    tag: Option<String>,
}

impl LoggingToolHook {
    /// Create a new logging hook that logs both pre and post events.
    pub fn new() -> Self {
        Self {
            log_input: true,
            log_output: false,
            max_log_length: 500,
            tag: None,
        }
    }

    /// Configure whether to log tool input.
    pub fn with_input_logging(mut self, enabled: bool) -> Self {
        self.log_input = enabled;
        self
    }

    /// Configure whether to log tool output.
    pub fn with_output_logging(mut self, enabled: bool) -> Self {
        self.log_output = enabled;
        self
    }

    /// Set the maximum length for logged input/output strings.
    pub fn with_max_length(mut self, max_len: usize) -> Self {
        self.max_log_length = max_len;
        self
    }

    /// Set a tag for this logger.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Truncate a string to the configured max length.
    fn truncate(&self, s: &str) -> String {
        if self.max_log_length == 0 || s.len() <= self.max_log_length {
            s.to_string()
        } else {
            format!("{}...", &s[..self.max_log_length - 3])
        }
    }

    fn format_tag(&self) -> &str {
        self.tag.as_deref().unwrap_or("ToolHook")
    }
}

impl Default for LoggingToolHook {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolHook for LoggingToolHook {
    fn hook_type(&self) -> &str {
        // Logging applies to both pre and post by convention.
        // Return a special type that matches both.
        // The ToolHookChain will call both run_pre_hooks and run_post_hooks,
        // but we register ourselves once with a wildcard approach.
        // In practice, two instances should be added: one as pre, one as post.
        "PreToolUse"
    }

    fn tool_filter(&self) -> Option<&str> {
        None
    }

    fn execute(&self, ctx: &ToolHookContext) -> Result<ToolHookResult, ToolHookError> {
        let tag = self.format_tag();

        if self.log_input {
            let input_str = serde_json::to_string(&ctx.tool_input).unwrap_or_else(|_| "[unserializable]".to_string());
            info!(
                "[{}] Tool: {} | Input: {}",
                tag,
                ctx.tool_name,
                self.truncate(&input_str)
            );
        } else {
            info!("[{}] Tool: {} (input logging disabled)", tag, ctx.tool_name);
        }

        if self.log_output {
            if let Some(ref output) = ctx.tool_output {
                let output_str = serde_json::to_string(output).unwrap_or_else(|_| "[unserializable]".to_string());
                info!(
                    "[{}] Tool: {} | Output: {} | Error: {}",
                    tag,
                    ctx.tool_name,
                    self.truncate(&output_str),
                    ctx.is_error
                );
            }
        }

        if ctx.is_error {
            warn!(
                "[{}] Tool '{}' completed with error",
                tag, ctx.tool_name
            );
        }

        // Logging never denies
        Ok(ToolHookResult::allow())
    }
}

/// A post-tool-use logging hook that specifically logs tool output.
///
/// This is a convenience wrapper for [`LoggingToolHook`] configured for post-use.
pub struct PostLoggingToolHook {
    inner: LoggingToolHook,
}

impl PostLoggingToolHook {
    /// Create a new post-logging hook.
    pub fn new() -> Self {
        Self {
            inner: LoggingToolHook::new()
                .with_output_logging(true)
                .with_input_logging(false)
                .with_tag("PostToolHook"),
        }
    }
}

impl Default for PostLoggingToolHook {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolHook for PostLoggingToolHook {
    fn hook_type(&self) -> &str {
        "PostToolUse"
    }

    fn tool_filter(&self) -> Option<&str> {
        None
    }

    fn execute(&self, ctx: &ToolHookContext) -> Result<ToolHookResult, ToolHookError> {
        self.inner.execute(ctx)
    }
}

/// A hook that stops the execution chain on the first Deny result.
///
/// This hook wraps another hook and, if the inner hook returns Deny, sets
/// `prevent_continuation` to ensure no further hooks are processed. This is
/// useful as a safety mechanism to ensure that once a dangerous operation is
/// denied, no later hook can accidentally override it.
pub struct StopOnDenyHook {
    inner: Box<dyn ToolHook>,
}

impl StopOnDenyHook {
    /// Create a new stop-on-deny wrapper around the given hook.
    pub fn new(inner: Box<dyn ToolHook>) -> Self {
        Self { inner }
    }
}

impl ToolHook for StopOnDenyHook {
    fn hook_type(&self) -> &str {
        self.inner.hook_type()
    }

    fn tool_filter(&self) -> Option<&str> {
        self.inner.tool_filter()
    }

    fn execute(&self, ctx: &ToolHookContext) -> Result<ToolHookResult, ToolHookError> {
        let mut result = self.inner.execute(ctx)?;

        // If the inner hook returned a deny, ensure continuation is prevented
        if result.is_denied() && !result.prevent_continuation {
            result.prevent_continuation = true;
            debug!(
                "StopOnDeny: preventing continuation after Deny for tool '{}'",
                ctx.tool_name
            );
        }

        Ok(result)
    }
}

/// A hook that modifies tool input based on a closure.
///
/// This is a convenience hook for simple input transformations without
/// needing to implement the full `ToolHook` trait.
pub struct InputTransformHook {
    tool_filter: Option<String>,
    transform: fn(&str, &Value) -> Result<Value, String>,
}

impl InputTransformHook {
    /// Create a new input transform hook.
    ///
    /// The `transform` closure receives the tool name and current input,
    /// and returns the modified input or an error string.
    pub fn new(transform: fn(&str, &Value) -> Result<Value, String>) -> Self {
        Self {
            tool_filter: None,
            transform,
        }
    }

    /// Set the tool filter for this hook.
    pub fn with_filter(mut self, tool_name: impl Into<String>) -> Self {
        self.tool_filter = Some(tool_name.into());
        self
    }
}

impl ToolHook for InputTransformHook {
    fn hook_type(&self) -> &str {
        "PreToolUse"
    }

    fn tool_filter(&self) -> Option<&str> {
        self.tool_filter.as_deref()
    }

    fn execute(&self, ctx: &ToolHookContext) -> Result<ToolHookResult, ToolHookError> {
        match (self.transform)(&ctx.tool_name, &ctx.tool_input) {
            Ok(updated) => Ok(ToolHookResult::allow_with_input(updated)),
            Err(e) => Err(ToolHookError::ExecutionFailed(e)),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── ToolHookDecision tests ──────────────────────────────────────────

    #[test]
    fn test_decision_default_is_allow() {
        assert_eq!(ToolHookDecision::default(), ToolHookDecision::Allow);
    }

    #[test]
    fn test_decision_display() {
        assert_eq!(ToolHookDecision::Allow.to_string(), "Allow");
        assert_eq!(ToolHookDecision::Deny.to_string(), "Deny");
        assert_eq!(ToolHookDecision::Ask.to_string(), "Ask");
    }

    #[test]
    fn test_decision_serialization() {
        let decision = ToolHookDecision::Deny;
        let json = serde_json::to_string(&decision).unwrap();
        assert_eq!(json, "\"Deny\"");
        let parsed: ToolHookDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ToolHookDecision::Deny);
    }

    // ── ToolHookResult tests ────────────────────────────────────────────

    #[test]
    fn test_result_allow() {
        let result = ToolHookResult::allow();
        assert_eq!(result.decision, ToolHookDecision::Allow);
        assert!(!result.is_denied());
        assert!(!result.is_ask());
        assert!(!result.should_stop());
        assert!(result.message.is_none());
    }

    #[test]
    fn test_result_deny() {
        let result = ToolHookResult::deny("not allowed");
        assert_eq!(result.decision, ToolHookDecision::Deny);
        assert!(result.is_denied());
        assert!(result.should_stop());
        assert_eq!(result.message, Some("not allowed".to_string()));
    }

    #[test]
    fn test_result_ask() {
        let result = ToolHookResult::ask("confirm?");
        assert_eq!(result.decision, ToolHookDecision::Ask);
        assert!(result.is_ask());
        assert!(!result.should_stop());
        assert_eq!(result.message, Some("confirm?".to_string()));
    }

    #[test]
    fn test_result_allow_with_input() {
        let result = ToolHookResult::allow_with_input(json!({"key": "value"}));
        assert_eq!(result.decision, ToolHookDecision::Allow);
        assert_eq!(result.updated_input, Some(json!({"key": "value"})));
    }

    #[test]
    fn test_result_with_context() {
        let result = ToolHookResult::allow().with_context("extra info");
        assert_eq!(result.additional_contexts.len(), 1);
        assert_eq!(result.additional_contexts[0], "extra info");
    }

    #[test]
    fn test_result_with_stop_reason() {
        let result = ToolHookResult::deny("fatal").with_stop_reason("agent stop");
        assert!(result.should_stop());
        assert_eq!(result.stop_reason, Some("agent stop".to_string()));
    }

    // ── ToolHookContext tests ───────────────────────────────────────────

    #[test]
    fn test_context_pre() {
        let ctx = ToolHookContext::pre(
            "Bash".to_string(),
            "id-123".to_string(),
            json!({"command": "ls"}),
        );
        assert_eq!(ctx.tool_name, "Bash");
        assert_eq!(ctx.tool_use_id, "id-123");
        assert!(ctx.tool_output.is_none());
        assert!(!ctx.is_error);
        assert!(!ctx.is_aborted());
    }

    #[test]
    fn test_context_post() {
        let ctx = ToolHookContext::post(
            "Bash".to_string(),
            "id-123".to_string(),
            json!({"command": "ls"}),
            json!({"stdout": "file.txt"}),
            false,
        );
        assert!(ctx.tool_output.is_some());
        assert!(!ctx.is_error);
    }

    #[test]
    fn test_context_abort_signal() {
        let ctx = ToolHookContext::pre("Bash".to_string(), "id".to_string(), json!({}));
        assert!(!ctx.is_aborted());
        ctx.abort();
        assert!(ctx.is_aborted());
    }

    #[test]
    fn test_context_to_post() {
        let pre = ToolHookContext::pre("Bash".to_string(), "id".to_string(), json!({"cmd": "ls"}));
        let post = pre.to_post(json!({"stdout": "ok"}), false);
        assert_eq!(post.tool_name, "Bash");
        assert_eq!(post.tool_use_id, "id");
        assert!(post.tool_output.is_some());
        // Abort signal should be shared
        pre.abort();
        assert!(post.is_aborted());
    }

    // ── ToolHookChain tests ─────────────────────────────────────────────

    #[test]
    fn test_chain_new_empty() {
        let chain = ToolHookChain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
    }

    #[test]
    fn test_chain_add_hooks() {
        let mut chain = ToolHookChain::new();
        chain.add_hook(Box::new(LoggingToolHook::new()));
        chain.add_hook(Box::new(PermissionToolHook::new()));
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn test_chain_with_hook_builder() {
        let chain = ToolHookChain::new()
            .with_hook(Box::new(LoggingToolHook::new()))
            .with_hook(Box::new(PermissionToolHook::new()));
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn test_chain_resolve_permission_all_allow() {
        let results = vec![
            ToolHookResult::allow(),
            ToolHookResult::allow(),
        ];
        assert_eq!(
            ToolHookChain::resolve_permission(&results),
            ToolHookDecision::Allow
        );
    }

    #[test]
    fn test_chain_resolve_permission_first_deny_wins() {
        let results = vec![
            ToolHookResult::allow(),
            ToolHookResult::deny("blocked"),
            ToolHookResult::allow(),
        ];
        assert_eq!(
            ToolHookChain::resolve_permission(&results),
            ToolHookDecision::Deny
        );
    }

    #[test]
    fn test_chain_resolve_permission_ask_without_deny() {
        let results = vec![
            ToolHookResult::allow(),
            ToolHookResult::ask("confirm?"),
        ];
        assert_eq!(
            ToolHookChain::resolve_permission(&results),
            ToolHookDecision::Ask
        );
    }

    #[test]
    fn test_chain_resolve_permission_deny_overrides_ask() {
        let results = vec![
            ToolHookResult::ask("confirm?"),
            ToolHookResult::deny("blocked"),
        ];
        assert_eq!(
            ToolHookChain::resolve_permission(&results),
            ToolHookDecision::Deny
        );
    }

    #[test]
    fn test_chain_resolve_permission_empty() {
        let results: Vec<ToolHookResult> = vec![];
        assert_eq!(
            ToolHookChain::resolve_permission(&results),
            ToolHookDecision::Allow
        );
    }

    #[test]
    fn test_chain_collect_updated_input() {
        let results = vec![
            ToolHookResult::allow_with_input(json!({"v": 1})),
            ToolHookResult::allow_with_input(json!({"v": 2})),
        ];
        let input = ToolHookChain::collect_updated_input(&results).unwrap();
        assert_eq!(input, json!({"v": 2})); // Last wins
    }

    #[test]
    fn test_chain_collect_updated_input_none() {
        let results = vec![ToolHookResult::allow(), ToolHookResult::allow()];
        assert!(ToolHookChain::collect_updated_input(&results).is_none());
    }

    #[test]
    fn test_chain_collect_contexts() {
        let results = vec![
            ToolHookResult::allow().with_context("ctx1"),
            ToolHookResult::allow().with_context("ctx2").with_context("ctx3"),
        ];
        let contexts = ToolHookChain::collect_contexts(&results);
        assert_eq!(contexts, vec!["ctx1", "ctx2", "ctx3"]);
    }

    #[test]
    fn test_chain_find_stop_reason() {
        let results = vec![
            ToolHookResult::allow(),
            ToolHookResult::deny("err").with_stop_reason("agent halt"),
        ];
        assert_eq!(
            ToolHookChain::find_stop_reason(&results),
            Some("agent halt".to_string())
        );
    }

    #[test]
    fn test_chain_find_stop_reason_none() {
        let results = vec![ToolHookResult::allow()];
        assert!(ToolHookChain::find_stop_reason(&results).is_none());
    }

    // ── Pre/post hook filtering tests ──────────────────────────────────

    #[test]
    fn test_chain_run_pre_hooks_filters_by_type() {
        // PostLoggingToolHook has hook_type "PostToolUse", so it should not run in pre
        let chain = ToolHookChain::new()
            .with_hook(Box::new(PostLoggingToolHook::new()))
            .with_hook(Box::new(PermissionToolHook::new()));

        let ctx = ToolHookContext::pre("Bash".to_string(), "id".to_string(), json!({"command": "ls"}));
        let results = chain.run_pre_hooks(&ctx).unwrap();
        // Only PermissionToolHook should have run (PostLoggingToolHook is PostToolUse type)
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_chain_run_post_hooks_filters_by_type() {
        // LoggingToolHook has hook_type "PreToolUse", so it should not run in post
        let chain = ToolHookChain::new()
            .with_hook(Box::new(LoggingToolHook::new()))
            .with_hook(Box::new(PostLoggingToolHook::new()));

        let ctx = ToolHookContext::post(
            "Bash".to_string(), "id".to_string(),
            json!({"command": "ls"}),
            json!({"stdout": "ok"}),
            false,
        );
        let results = chain.run_post_hooks(&ctx).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_chain_prevent_continuation_stops_chain() {
        // Deny results have prevent_continuation = true by default
        struct AlwaysDenyHook;
        impl ToolHook for AlwaysDenyHook {
            fn hook_type(&self) -> &str { "PreToolUse" }
            fn tool_filter(&self) -> Option<&str> { None }
            fn execute(&self, _ctx: &ToolHookContext) -> Result<ToolHookResult, ToolHookError> {
                Ok(ToolHookResult::deny("nope"))
            }
        }

        struct NeverRunsHook;
        impl ToolHook for NeverRunsHook {
            fn hook_type(&self) -> &str { "PreToolUse" }
            fn tool_filter(&self) -> Option<&str> { None }
            fn execute(&self, _ctx: &ToolHookContext) -> Result<ToolHookResult, ToolHookError> {
                panic!("This hook should not have been called");
            }
        }

        let chain = ToolHookChain::new()
            .with_hook(Box::new(AlwaysDenyHook))
            .with_hook(Box::new(NeverRunsHook));

        let ctx = ToolHookContext::pre("Bash".to_string(), "id".to_string(), json!({}));
        let results = chain.run_pre_hooks(&ctx).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_denied());
    }

    #[test]
    fn test_chain_tool_filter_skips_non_matching() {
        struct OnlyBashHook;
        impl ToolHook for OnlyBashHook {
            fn hook_type(&self) -> &str { "PreToolUse" }
            fn tool_filter(&self) -> Option<&str> { Some("Bash") }
            fn execute(&self, _ctx: &ToolHookContext) -> Result<ToolHookResult, ToolHookError> {
                Ok(ToolHookResult::deny("bash denied"))
            }
        }

        let chain = ToolHookChain::new()
            .with_hook(Box::new(OnlyBashHook));

        // Read tool should not match the Bash-only hook
        let ctx = ToolHookContext::pre("Read".to_string(), "id".to_string(), json!({}));
        let results = chain.run_pre_hooks(&ctx).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_chain_abort_signal_skips_hooks() {
        struct SecondHook;
        impl ToolHook for SecondHook {
            fn hook_type(&self) -> &str { "PreToolUse" }
            fn tool_filter(&self) -> Option<&str> { None }
            fn execute(&self, _ctx: &ToolHookContext) -> Result<ToolHookResult, ToolHookError> {
                panic!("Should not run due to abort signal");
            }
        }

        let chain = ToolHookChain::new()
            .with_hook(Box::new(SecondHook));

        let ctx = ToolHookContext::pre("Bash".to_string(), "id".to_string(), json!({}));
        ctx.abort();
        let results = chain.run_pre_hooks(&ctx).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_chain_hooks_by_type() {
        let chain = ToolHookChain::new()
            .with_hook(Box::new(LoggingToolHook::new()))
            .with_hook(Box::new(PostLoggingToolHook::new()))
            .with_hook(Box::new(PermissionToolHook::new()));

        assert_eq!(chain.hooks_by_type("PreToolUse").len(), 2);
        assert_eq!(chain.hooks_by_type("PostToolUse").len(), 1);
        assert_eq!(chain.hooks_by_type("Unknown").len(), 0);
    }

    // ── PermissionToolHook tests ───────────────────────────────────────

    #[test]
    fn test_permission_hook_default_allows_read() {
        let hook = PermissionToolHook::new();
        let ctx = ToolHookContext::pre("Read".to_string(), "id".to_string(), json!({"file_path": "a.rs"}));
        let result = hook.execute(&ctx).unwrap();
        assert!(!result.is_denied());
        assert!(!result.is_ask());
    }

    #[test]
    fn test_permission_hook_default_denies_unknown() {
        let hook = PermissionToolHook::new();
        let ctx = ToolHookContext::pre("Bash".to_string(), "id".to_string(), json!({"command": "ls"}));
        let result = hook.execute(&ctx).unwrap();
        assert!(result.is_ask());
    }

    #[test]
    fn test_permission_hook_explicit_deny() {
        let hook = PermissionToolHook::new().deny_tool("Dangerous");
        let ctx = ToolHookContext::pre("Dangerous".to_string(), "id".to_string(), json!({}));
        let result = hook.execute(&ctx).unwrap();
        assert!(result.is_denied());
    }

    #[test]
    fn test_permission_hook_deny_pattern() {
        let hook = PermissionToolHook::new().deny_pattern("rm -rf /");
        let ctx = ToolHookContext::pre("Bash".to_string(), "id".to_string(), json!({"command": "rm -rf /"}));
        let result = hook.execute(&ctx).unwrap();
        assert!(result.is_denied());
    }

    #[test]
    fn test_permission_hook_explicit_allow() {
        let hook = PermissionToolHook::new().allow_tool("Bash");
        let ctx = ToolHookContext::pre("Bash".to_string(), "id".to_string(), json!({"command": "ls"}));
        let result = hook.execute(&ctx).unwrap();
        assert!(!result.is_denied());
        assert!(!result.is_ask());
    }

    // ── LoggingToolHook tests ──────────────────────────────────────────

    #[test]
    fn test_logging_hook_always_allows() {
        let hook = LoggingToolHook::new();
        let ctx = ToolHookContext::pre("Bash".to_string(), "id".to_string(), json!({"command": "ls"}));
        let result = hook.execute(&ctx).unwrap();
        assert_eq!(result.decision, ToolHookDecision::Allow);
    }

    #[test]
    fn test_logging_hook_with_tag() {
        let hook = LoggingToolHook::new().with_tag("AuditLog");
        assert_eq!(hook.format_tag(), "AuditLog");
    }

    #[test]
    fn test_logging_hook_truncate() {
        let hook = LoggingToolHook::new().with_max_length(10);
        let long = "abcdefghijklmnopqrstuvwxyz";
        let truncated = hook.truncate(long);
        assert_eq!(truncated, "abcdefg...");
        assert_eq!(truncated.len(), 10);
    }

    #[test]
    fn test_logging_hook_no_truncation() {
        let hook = LoggingToolHook::new().with_max_length(0);
        let s = "hello world";
        assert_eq!(hook.truncate(s), s);
    }

    // ── StopOnDenyHook tests ───────────────────────────────────────────

    #[test]
    fn test_stop_on_deny_preserves_deny() {
        struct DenyHook;
        impl ToolHook for DenyHook {
            fn hook_type(&self) -> &str { "PreToolUse" }
            fn tool_filter(&self) -> Option<&str> { None }
            fn execute(&self, _ctx: &ToolHookContext) -> Result<ToolHookResult, ToolHookError> {
                Ok(ToolHookResult::deny("blocked"))
            }
        }

        let wrapper = StopOnDenyHook::new(Box::new(DenyHook));
        let ctx = ToolHookContext::pre("Bash".to_string(), "id".to_string(), json!({}));
        let result = wrapper.execute(&ctx).unwrap();
        assert!(result.is_denied());
        assert!(result.should_stop());
    }

    #[test]
    fn test_stop_on_deny_preserves_allow() {
        struct AllowHook;
        impl ToolHook for AllowHook {
            fn hook_type(&self) -> &str { "PreToolUse" }
            fn tool_filter(&self) -> Option<&str> { None }
            fn execute(&self, _ctx: &ToolHookContext) -> Result<ToolHookResult, ToolHookError> {
                Ok(ToolHookResult::allow())
            }
        }

        let wrapper = StopOnDenyHook::new(Box::new(AllowHook));
        let ctx = ToolHookContext::pre("Bash".to_string(), "id".to_string(), json!({}));
        let result = wrapper.execute(&ctx).unwrap();
        assert!(!result.is_denied());
        assert!(!result.should_stop());
    }

    #[test]
    fn test_stop_on_deny_delegates_hook_type_and_filter() {
        struct TypedHook;
        impl ToolHook for TypedHook {
            fn hook_type(&self) -> &str { "PostToolUse" }
            fn tool_filter(&self) -> Option<&str> { Some("Bash") }
            fn execute(&self, _ctx: &ToolHookContext) -> Result<ToolHookResult, ToolHookError> {
                Ok(ToolHookResult::allow())
            }
        }

        let wrapper = StopOnDenyHook::new(Box::new(TypedHook));
        assert_eq!(wrapper.hook_type(), "PostToolUse");
        assert_eq!(wrapper.tool_filter(), Some("Bash"));
    }

    // ── InputTransformHook tests ───────────────────────────────────────

    #[test]
    fn test_input_transform_hook() {
        fn add_safety_flag(_tool: &str, input: &Value) -> Result<Value, String> {
            let mut modified = input.clone();
            if let Some(obj) = modified.as_object_mut() {
                obj.insert("_safe".to_string(), json!(true));
            }
            Ok(modified)
        }

        let hook = InputTransformHook::new(add_safety_flag);
        let ctx = ToolHookContext::pre("Bash".to_string(), "id".to_string(), json!({"command": "ls"}));
        let result = hook.execute(&ctx).unwrap();
        assert_eq!(result.decision, ToolHookDecision::Allow);
        assert_eq!(result.updated_input, Some(json!({"command": "ls", "_safe": true})));
    }

    #[test]
    fn test_input_transform_hook_with_filter() {
        fn transform(_tool: &str, input: &Value) -> Result<Value, String> {
            Ok(input.clone())
        }

        let hook = InputTransformHook::new(transform).with_filter("Bash");
        assert_eq!(hook.tool_filter(), Some("Bash"));
        assert!(hook.applies_to("Bash"));
        assert!(!hook.applies_to("Read"));
    }

    #[test]
    fn test_input_transform_hook_error() {
        fn fail(_tool: &str, _input: &Value) -> Result<Value, String> {
            Err("transform failed".to_string())
        }

        let hook = InputTransformHook::new(fail);
        let ctx = ToolHookContext::pre("Bash".to_string(), "id".to_string(), json!({}));
        let result = hook.execute(&ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("transform failed"));
    }

    // ── ToolHookError tests ────────────────────────────────────────────

    #[test]
    fn test_error_display() {
        let err = ToolHookError::Denied("not allowed".to_string());
        assert!(err.to_string().contains("not allowed"));

        let err = ToolHookError::ExecutionFailed("hook crashed".to_string());
        assert!(err.to_string().contains("hook crashed"));

        let err = ToolHookError::InvalidHookType("BadType".to_string());
        assert!(err.to_string().contains("BadType"));

        let err = ToolHookError::Configuration("missing field".to_string());
        assert!(err.to_string().contains("missing field"));
    }

    // ── ToolHookResult serialization tests ─────────────────────────────

    #[test]
    fn test_result_serialization() {
        let result = ToolHookResult::deny("test reason");
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ToolHookResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.decision, ToolHookDecision::Deny);
        assert_eq!(parsed.message, Some("test reason".to_string()));
        assert!(parsed.prevent_continuation);
    }

    #[test]
    fn test_result_serialization_with_contexts() {
        let result = ToolHookResult::allow()
            .with_context("c1")
            .with_context("c2");
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ToolHookResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.additional_contexts, vec!["c1", "c2"]);
    }
}
