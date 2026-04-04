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

    /// List all registered tools with their metadata (name, description, category, auth, schema).
    pub fn list_tools_info(&self) -> Vec<ToolInfo> {
        self.tools
            .values()
            .map(|t| ToolInfo {
                name: t.name().to_string(),
                description: t.description().to_string(),
                category: t.category().to_string(),
                requires_auth: t.requires_auth(),
                input_schema: t.input_schema(),
            })
            .collect()
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

    // ── Tool Registry Integration Tests ───────────────────────────────────

    struct AsyncTool {
        name: String,
        delay_ms: u64,
    }

    #[async_trait]
    impl Tool for AsyncTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "An async tool for testing"
        }

        fn input_schema(&self) -> Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                }
            })
        }

        async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
            // Simulate async work
            tokio::time::sleep(tokio::time::Duration::from_millis(self.delay_ms)).await;
            Ok(ToolOutput {
                content: format!("Processed: {}", input["input"].as_str().unwrap_or("")),
                is_error: false,
                metadata: HashMap::new(),
            })
        }

        fn requires_auth(&self) -> bool {
            true
        }

        fn category(&self) -> &str {
            "test"
        }
    }

    #[tokio::test]
    async fn test_concurrent_tool_execution() {
        let mut registry = ToolRegistry::new();

        // Register multiple tools
        for i in 0..5 {
            let tool = Box::new(AsyncTool {
                name: format!("async_tool_{}", i),
                delay_ms: 10,
            });
            registry.register(tool).unwrap();
        }

        let registry = std::sync::Arc::new(registry);
        let mut handles = Vec::new();

        // Execute tools concurrently
        for i in 0..5 {
            let registry_clone = registry.clone();
            let handle = tokio::spawn(async move {
                let tool_name = format!("async_tool_{}", i);
                let input = serde_json::json!({"input": format!("request_{}", i)});
                registry_clone.execute(&tool_name, input).await
            });
            handles.push(handle);
        }

        // Wait for all executions
        let results = futures::future::join_all(handles).await;

        // All should succeed
        for result in results {
            assert!(result.is_ok());
            let output = result.unwrap().unwrap();
            assert!(output.content.contains("Processed:"));
        }
    }

    #[tokio::test]
    async fn test_tool_execution_with_permission_checks() {
        use crate::permissions::{PermissionPrompt, RiskLevel};

        let mut registry = ToolRegistry::new();
        let tool = Box::new(AsyncTool {
            name: "secure_tool".to_string(),
            delay_ms: 0,
        });

        registry.register(tool).unwrap();

        // Check tool info includes auth requirement
        let tools_info = registry.list_tools_info();
        let secure_tool_info = tools_info.iter().find(|t| t.name == "secure_tool").unwrap();
        assert!(secure_tool_info.requires_auth);
        assert_eq!(secure_tool_info.category, "test");

        // Execute the tool
        let result = registry
            .execute("secure_tool", serde_json::json!({"input": "test"}))
            .await;

        assert!(result.is_ok());

        // Verify permission prompt that would be generated
        let prompt = PermissionPrompt::new(
            "secure_tool".to_string(),
            serde_json::json!({"input": "test"}),
            RiskLevel::Low,
            "Execute secure_tool".to_string(),
        );
        assert_eq!(prompt.tool_name, "secure_tool");
        assert_eq!(prompt.risk_level, RiskLevel::Low);
    }

    #[tokio::test]
    async fn test_tool_registry_with_multiple_tools() {
        let mut registry = ToolRegistry::new();

        // Register multiple tools with different characteristics
        let tools = vec![
            Box::new(AsyncTool {
                name: "read_tool".to_string(),
                delay_ms: 0,
            }) as Box<dyn Tool>,
            Box::new(AsyncTool {
                name: "write_tool".to_string(),
                delay_ms: 0,
            }) as Box<dyn Tool>,
            Box::new(AsyncTool {
                name: "network_tool".to_string(),
                delay_ms: 0,
            }) as Box<dyn Tool>,
        ];

        for tool in tools {
            registry.register(tool).unwrap();
        }

        // List all tools
        let tool_names = registry.list();
        assert_eq!(tool_names.len(), 3);
        assert!(tool_names.contains(&"read_tool".to_string()));
        assert!(tool_names.contains(&"write_tool".to_string()));
        assert!(tool_names.contains(&"network_tool".to_string()));

        // Get all tool info
        let tools_info = registry.list_tools_info();
        assert_eq!(tools_info.len(), 3);

        // Convert to JSON schema
        let json_schema = registry.to_json_schema();
        assert!(json_schema.is_array());
        assert_eq!(json_schema.as_array().unwrap().len(), 3);

        // Convert to tool definitions
        let tool_defs = registry.to_tool_definitions();
        assert_eq!(tool_defs.len(), 3);
    }

    #[tokio::test]
    async fn test_tool_unregister() {
        let mut registry = ToolRegistry::new();

        let tool = Box::new(DummyTool {
            name: "temp_tool".to_string(),
        });

        registry.register(tool).unwrap();
        assert!(registry.list().contains(&"temp_tool".to_string()));

        // Unregister
        registry.unregister("temp_tool").unwrap();
        assert!(!registry.list().contains(&"temp_tool".to_string()));

        // Unregistering non-existent tool should fail
        let result = registry.unregister("nonexistent");
        assert!(matches!(result, Err(ToolError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_duplicate_tool_registration_fails() {
        let mut registry = ToolRegistry::new();

        let tool1 = Box::new(DummyTool {
            name: "dup_tool".to_string(),
        });
        let tool2 = Box::new(DummyTool {
            name: "dup_tool".to_string(),
        });

        registry.register(tool1).unwrap();

        let result = registry.register(tool2);
        assert!(matches!(result, Err(ToolError::RegistryError(_))));
    }

    #[tokio::test]
    async fn test_execute_nonexistent_tool() {
        let registry = ToolRegistry::new();

        let result = registry
            .execute("nonexistent", serde_json::json!({}))
            .await;

        assert!(matches!(result, Err(ToolError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_tool_metadata() {
        let mut registry = ToolRegistry::new();

        let tool = Box::new(DummyTool {
            name: "metadata_tool".to_string(),
        });

        registry.register(tool).unwrap();

        // Get tool info
        let tools_info = registry.list_tools_info();
        let info = tools_info.iter().find(|t| t.name == "metadata_tool").unwrap();

        assert_eq!(info.name, "metadata_tool");
        assert_eq!(info.description, "A dummy tool for testing");
        assert_eq!(info.category, "general");
        assert!(!info.requires_auth);
        assert!(info.input_schema.is_object());
    }

    #[tokio::test]
    async fn test_concurrent_tool_registration() {
        use dashmap::DashMap;

        let registry = std::sync::Arc::new(std::sync::Mutex::new(ToolRegistry::new()));
        let num_threads = 10;

        let mut handles = Vec::new();

        // Each thread registers a unique tool
        for i in 0..num_threads {
            let registry_clone = registry.clone();
            let handle = tokio::spawn(async move {
                let tool = Box::new(DummyTool {
                    name: format!("concurrent_tool_{}", i),
                });
                registry_clone.lock().unwrap().register(tool)
            });
            handles.push(handle);
        }

        // Wait for all registrations
        let results = futures::future::join_all(handles).await;

        // All should succeed
        for result in results {
            assert!(result.is_ok());
        }

        // Verify all tools were registered
        let tool_names = registry.lock().unwrap().list();
        assert_eq!(tool_names.len(), num_threads);
    }

    #[tokio::test]
    async fn test_tool_output_with_metadata() {
        let mut registry = ToolRegistry::new();

        struct MetadataTool {
            name: String,
        }

        #[async_trait]
        impl Tool for MetadataTool {
            fn name(&self) -> &str {
                &self.name
            }

            fn description(&self) -> &str {
                "Tool with metadata"
            }

            fn input_schema(&self) -> Value {
                serde_json::json!({"type": "object"})
            }

            async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
                let mut metadata = HashMap::new();
                metadata.insert("execution_time_ms".into(), serde_json::json!(100));
                metadata.insert("timestamp".into(), serde_json::json!("2024-01-01T00:00:00Z"));

                Ok(ToolOutput {
                    content: "Success".to_string(),
                    is_error: false,
                    metadata,
                })
            }
        }

        let tool = Box::new(MetadataTool {
            name: "metadata_tool".to_string(),
        });

        registry.register(tool).unwrap();

        let result = registry
            .execute("metadata_tool", serde_json::json!({}))
            .await
            .unwrap();

        assert_eq!(result.content, "Success");
        assert!(!result.is_error);
        assert_eq!(result.metadata.get("execution_time_ms"), Some(&serde_json::json!(100)));
        assert_eq!(result.metadata.get("timestamp"), Some(&serde_json::json!("2024-01-01T00:00:00Z")));
    }
}
