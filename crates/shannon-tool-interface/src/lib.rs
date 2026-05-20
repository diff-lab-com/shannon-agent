//! # Shannon Tool Interface
//!
//! This crate provides the core trait definitions and types for the Shannon tool system.
//! It defines the interface that tools must implement, without any concrete implementations.
//!
//! ## Purpose
//!
//! Breaking circular dependencies between `shannon-core` and `shannon-tools`:
//! - `shannon-core` depends on this crate for trait definitions
//! - `shannon-tools` depends on this crate for trait implementations
//! - No circular dependency between core and tools

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

/// Sender for streaming tool progress updates.
/// Tools call `send(line)` to emit partial output during execution.
pub trait ProgressSender: Send + Sync {
    fn send(&self, line: &str);
}

/// Type-erased boxed progress sender.
pub type BoxedProgressSender = Arc<dyn ProgressSender>;

/// Errors that can occur during tool execution
#[derive(Error, Debug)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    NotFound(String),

    #[error("Invalid tool input: {0}")]
    InvalidInput(String),

    #[error("Tool execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Tool registry error: {0}")]
    RegistryError(String),
}

/// Result type for tool execution
pub type ToolResult<T> = Result<T, ToolError>;

/// Output from a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
    pub metadata: HashMap<String, Value>,
}

impl ToolOutput {
    /// Create a new successful tool output
    pub fn success(content: String) -> Self {
        Self {
            content,
            is_error: false,
            metadata: HashMap::new(),
        }
    }

    /// Create a new error tool output
    pub fn error(content: String) -> Self {
        Self {
            content,
            is_error: true,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the output
    pub fn with_metadata(mut self, key: String, value: Value) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Trait defining a tool that can be executed by the query engine
#[async_trait]
pub trait Tool: Send + Sync {
    /// Get the tool's name
    fn name(&self) -> &str;

    /// Get the tool's description
    fn description(&self) -> &str;

    /// Get the JSON schema for the tool's input parameters
    fn input_schema(&self) -> Value;

    /// Execute the tool with the given input
    async fn execute(&self, input: Value) -> ToolResult<ToolOutput>;

    /// Execute the tool with streaming progress updates.
    /// Default implementation delegates to `execute()` (non-streaming).
    /// Override in tools that support real-time output (e.g., bash).
    async fn execute_streaming(
        &self,
        input: Value,
        _progress: BoxedProgressSender,
    ) -> ToolResult<ToolOutput> {
        self.execute(input).await
    }

    /// Check if the tool requires authentication
    fn requires_auth(&self) -> bool {
        false
    }

    /// Get the tool's category
    fn category(&self) -> &str {
        "general"
    }

    /// Whether this tool only performs read-only operations (no side effects).
    ///
    /// Read-only tools can be safely batched and executed in parallel.
    /// Tools that modify files, run commands, or change state must return `false`.
    fn is_read_only(&self) -> bool {
        false
    }

    /// Whether this tool invocation is safe to run concurrently with other tools.
    ///
    /// Defaults to the value of [`is_read_only`] since read-only tools are
    /// generally concurrency-safe. Override for tools that are write-operations
    /// but still safe to parallelize (e.g. writing to independent files).
    fn is_concurrency_safe(&self) -> bool {
        self.is_read_only()
    }

    /// Whether this tool may perform destructive operations (irreversible changes).
    ///
    /// Destructive tools always require user confirmation, even in auto-approve modes.
    /// MCP servers signal this via `annotations.destructiveHint`.
    fn is_destructive(&self) -> bool {
        false
    }
}

/// Metadata about a registered tool, used for tool discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// Tool category
    pub category: String,
    /// Whether the tool requires authentication
    pub requires_auth: bool,
    /// JSON Schema describing the tool's input parameters
    pub input_schema: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_tool_output_success() {
        let output = ToolOutput::success("Done".to_string());
        assert_eq!(output.content, "Done");
        assert!(!output.is_error);
        assert!(output.metadata.is_empty());
    }

    #[test]
    fn test_tool_output_error() {
        let output = ToolOutput::error("Failed".to_string());
        assert_eq!(output.content, "Failed");
        assert!(output.is_error);
        assert!(output.metadata.is_empty());
    }

    #[test]
    fn test_tool_output_with_metadata() {
        let output = ToolOutput::success("Done".to_string())
            .with_metadata("key".to_string(), json!("value"));

        assert_eq!(output.metadata.get("key"), Some(&json!("value")));
    }

    #[test]
    fn test_tool_error_display() {
        let err = ToolError::NotFound("test_tool".to_string());
        assert_eq!(err.to_string(), "Tool not found: test_tool");

        let err = ToolError::InvalidInput("bad input".to_string());
        assert_eq!(err.to_string(), "Invalid tool input: bad input");
    }

    // ── ProgressSender and execute_streaming tests ──────────────────────

    /// Collecting ProgressSender that stores all sent lines.
    struct CollectingSender {
        lines: std::sync::Mutex<Vec<String>>,
    }

    impl CollectingSender {
        fn new() -> Self {
            Self { lines: std::sync::Mutex::new(Vec::new()) }
        }
        fn collected(&self) -> Vec<String> {
            self.lines.lock().unwrap().clone()
        }
    }

    impl ProgressSender for CollectingSender {
        fn send(&self, line: &str) {
            self.lines.lock().unwrap().push(line.to_string());
        }
    }

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str { "echo" }
        fn description(&self) -> &str { "echo tool" }
        fn input_schema(&self) -> Value {
            json!({"type": "object", "properties": {"msg": {"type": "string"}}})
        }
        async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
            let msg = input.get("msg").and_then(|v| v.as_str()).unwrap_or("");
            Ok(ToolOutput::success(msg.to_string()))
        }
    }

    #[tokio::test]
    async fn test_execute_streaming_default_delegates_to_execute() {
        let tool = EchoTool;
        let sender = Arc::new(CollectingSender::new());
        let result = tool.execute_streaming(json!({"msg": "hello"}), sender.clone()).await.unwrap();
        assert_eq!(result.content, "hello");
        assert!(!result.is_error);
        // Default implementation does NOT call the progress sender
        assert!(sender.collected().is_empty());
    }

    #[test]
    fn test_collecting_sender_captures_lines() {
        let sender = CollectingSender::new();
        sender.send("line 1");
        sender.send("line 2");
        sender.send("line 3");
        assert_eq!(sender.collected(), vec!["line 1", "line 2", "line 3"]);
    }
}
