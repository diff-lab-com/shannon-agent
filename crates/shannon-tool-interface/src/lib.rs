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
use thiserror::Error;

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

    /// Check if the tool requires authentication
    fn requires_auth(&self) -> bool {
        false
    }

    /// Get the tool's category
    fn category(&self) -> &str {
        "general"
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
}
