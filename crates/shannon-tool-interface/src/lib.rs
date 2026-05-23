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
            Self {
                lines: std::sync::Mutex::new(Vec::new()),
            }
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
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echo tool"
        }
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
        let result = tool
            .execute_streaming(json!({"msg": "hello"}), sender.clone())
            .await
            .unwrap();
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

    // ── Additional comprehensive tests ──────────────────────────────────

    #[test]
    fn test_tool_output_serialization_roundtrip() {
        let output = ToolOutput::success("hello".to_string())
            .with_metadata("lines".to_string(), json!(42))
            .with_metadata("files".to_string(), json!(["a.rs", "b.rs"]));
        let json = serde_json::to_string(&output).unwrap();
        let deserialized: ToolOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.content, "hello");
        assert!(!deserialized.is_error);
        assert_eq!(deserialized.metadata.get("lines"), Some(&json!(42)));
    }

    #[test]
    fn test_tool_output_error_serialization() {
        let output = ToolOutput::error("permission denied".to_string());
        let json = serde_json::to_string(&output).unwrap();
        let deserialized: ToolOutput = serde_json::from_str(&json).unwrap();
        assert!(deserialized.is_error);
        assert_eq!(deserialized.content, "permission denied");
    }

    #[test]
    fn test_tool_output_empty_metadata() {
        let output = ToolOutput::success("".to_string());
        assert!(output.metadata.is_empty());
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"metadata\":{}"));
    }

    #[test]
    fn test_tool_output_with_metadata_overwrites() {
        let output = ToolOutput::success("ok".to_string())
            .with_metadata("key".to_string(), json!(1))
            .with_metadata("key".to_string(), json!(2));
        assert_eq!(output.metadata.get("key"), Some(&json!(2)));
    }

    #[test]
    fn test_tool_error_variants() {
        let cases = vec![
            (ToolError::NotFound("x".into()), "Tool not found: x"),
            (ToolError::InvalidInput("y".into()), "Invalid tool input: y"),
            (
                ToolError::ExecutionFailed("z".into()),
                "Tool execution failed: z",
            ),
            (
                ToolError::RegistryError("w".into()),
                "Tool registry error: w",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn test_tool_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ToolError>();
    }

    #[test]
    fn test_tool_info_serialization() {
        let info = ToolInfo {
            name: "bash".to_string(),
            description: "Run shell commands".to_string(),
            category: "system".to_string(),
            requires_auth: false,
            input_schema: json!({"type": "object"}),
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: ToolInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "bash");
        assert_eq!(deserialized.category, "system");
        assert!(!deserialized.requires_auth);
    }

    #[test]
    fn test_tool_info_requires_auth() {
        let info = ToolInfo {
            name: "github".to_string(),
            description: "GitHub API".to_string(),
            category: "remote".to_string(),
            requires_auth: true,
            input_schema: json!({"type": "object"}),
        };
        assert!(info.requires_auth);
    }

    #[test]
    fn test_tool_result_ok_and_err() {
        let ok: ToolResult<ToolOutput> = Ok(ToolOutput::success("ok".into()));
        assert!(ok.is_ok());

        let err: ToolResult<ToolOutput> = Err(ToolError::ExecutionFailed("boom".into()));
        assert!(err.is_err());
        match err {
            Err(ToolError::ExecutionFailed(msg)) => assert_eq!(msg, "boom"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_echo_tool_trait_methods() {
        let tool = EchoTool;
        assert_eq!(tool.name(), "echo");
        assert_eq!(tool.description(), "echo tool");
        assert_eq!(tool.category(), "general");
        assert!(!tool.requires_auth());
        assert!(!tool.is_read_only());
        assert!(!tool.is_concurrency_safe());
        assert!(!tool.is_destructive());
    }

    #[tokio::test]
    async fn test_echo_tool_execute() {
        let tool = EchoTool;
        let result = tool.execute(json!({"msg": "test"})).await.unwrap();
        assert_eq!(result.content, "test");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_echo_tool_execute_missing_field() {
        let tool = EchoTool;
        let result = tool.execute(json!({})).await.unwrap();
        assert_eq!(result.content, "");
    }

    // ── Tool trait default method tests ──────────────────────────────────

    struct ReadOnlyTool;

    #[async_trait]
    impl Tool for ReadOnlyTool {
        fn name(&self) -> &str {
            "readonly"
        }
        fn description(&self) -> &str {
            "read-only tool"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }
        fn is_read_only(&self) -> bool {
            true
        }
        async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
            Ok(ToolOutput::success("read".into()))
        }
    }

    #[test]
    fn test_readonly_tool_defaults() {
        let tool = ReadOnlyTool;
        assert!(tool.is_read_only());
        assert!(tool.is_concurrency_safe()); // defaults to is_read_only
        assert!(!tool.is_destructive());
        assert!(!tool.requires_auth());
        assert_eq!(tool.category(), "general");
    }

    struct DestructiveTool;

    #[async_trait]
    impl Tool for DestructiveTool {
        fn name(&self) -> &str {
            "rm_rf"
        }
        fn description(&self) -> &str {
            "destructive tool"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }
        fn is_destructive(&self) -> bool {
            true
        }
        async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
            Ok(ToolOutput::success("deleted".into()))
        }
    }

    #[test]
    fn test_destructive_tool() {
        let tool = DestructiveTool;
        assert!(tool.is_destructive());
        assert!(!tool.is_read_only());
        assert!(!tool.is_concurrency_safe());
    }

    struct ConcurrentWriteTool;

    #[async_trait]
    impl Tool for ConcurrentWriteTool {
        fn name(&self) -> &str {
            "concurrent_write"
        }
        fn description(&self) -> &str {
            "concurrent write tool"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }
        fn is_read_only(&self) -> bool {
            false
        }
        fn is_concurrency_safe(&self) -> bool {
            true
        }
        async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
            Ok(ToolOutput::success("wrote".into()))
        }
    }

    #[test]
    fn test_concurrent_write_overrides_safety() {
        let tool = ConcurrentWriteTool;
        assert!(!tool.is_read_only());
        assert!(tool.is_concurrency_safe()); // overridden independently
    }

    struct StreamingTool;

    #[async_trait]
    impl Tool for StreamingTool {
        fn name(&self) -> &str {
            "streaming"
        }
        fn description(&self) -> &str {
            "streaming tool"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }
        async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
            Ok(ToolOutput::success("base".into()))
        }
        async fn execute_streaming(
            &self,
            _input: Value,
            progress: BoxedProgressSender,
        ) -> ToolResult<ToolOutput> {
            progress.send("line 1");
            progress.send("line 2");
            Ok(ToolOutput::success("streamed".into()))
        }
    }

    #[tokio::test]
    async fn test_streaming_tool_overrides_default() {
        let tool = StreamingTool;
        let sender = Arc::new(CollectingSender::new());
        let result = tool
            .execute_streaming(json!({}), sender.clone())
            .await
            .unwrap();
        assert_eq!(result.content, "streamed");
        assert_eq!(sender.collected(), vec!["line 1", "line 2"]);
    }

    #[test]
    fn test_progress_sender_thread_safety() {
        use std::thread;
        let sender = Arc::new(CollectingSender::new());
        let mut handles = vec![];
        for i in 0..4 {
            let s = sender.clone();
            handles.push(thread::spawn(move || {
                s.send(&format!("thread-{i}"));
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let lines = sender.collected();
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn test_boxed_progress_sender_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BoxedProgressSender>();
    }
}
