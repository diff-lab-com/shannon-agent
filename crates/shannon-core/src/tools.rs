//! # Tool System
//!
//! Dynamic tool registration, execution, and result handling.

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

/// Registry for managing available tools
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Create a new empty tool registry
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a new tool
    pub fn register(&mut self, tool: Box<dyn Tool>) -> ToolResult<()> {
        let name = tool.name().to_string();
        if self.tools.contains_key(&name) {
            return Err(ToolError::RegistryError(format!(
                "Tool {} already registered",
                name
            )));
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    /// Unregister a tool
    pub fn unregister(&mut self, name: &str) -> ToolResult<()> {
        self.tools
            .remove(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        Ok(())
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// List all registered tool names
    pub fn list(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Execute a tool by name
    pub async fn execute(&self, name: &str, input: Value) -> ToolResult<ToolOutput> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        tool.execute(input).await
    }

    /// Get all tools as JSON schema for Claude API
    pub fn to_json_schema(&self) -> Value {
        let tools: Vec<Value> = self
            .tools
            .values()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "input_schema": tool.input_schema(),
                })
            })
            .collect();
        serde_json::json!(tools)
    }

    /// Get all tools as ToolDefinition for Claude API
    pub fn to_tool_definitions(&self) -> Vec<crate::api::ToolDefinition> {
        self.tools
            .values()
            .map(|tool| crate::api::ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
                strict: Some(false), // Default to non-strict for compatibility
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyTool {
        name: String,
    }

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "A dummy tool for testing"
        }

        fn input_schema(&self) -> Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                }
            })
        }

        async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
            Ok(ToolOutput {
                content: "Executed".to_string(),
                is_error: false,
                metadata: HashMap::new(),
            })
        }
    }

    #[tokio::test]
    async fn test_tool_registration() {
        let mut registry = ToolRegistry::new();
        let tool = Box::new(DummyTool {
            name: "test_tool".to_string(),
        });

        registry.register(tool).unwrap();
        assert_eq!(registry.list(), vec!["test_tool".to_string()]);
    }

    #[tokio::test]
    async fn test_tool_execution() {
        let mut registry = ToolRegistry::new();
        let tool = Box::new(DummyTool {
            name: "test_tool".to_string(),
        });

        registry.register(tool).unwrap();
        let result = registry
            .execute("test_tool", serde_json::json!({"input": "test"}))
            .await
            .unwrap();
        assert_eq!(result.content, "Executed");
    }
}
