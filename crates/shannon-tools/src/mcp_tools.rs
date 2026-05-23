//! MCP resource tools for the shannon-tools layer
//!
//! Provides `ListMcpResourcesTool` and `ReadMcpResourceTool` that implement
//! the `Tool` trait from shannon-core and delegate to a shared
//! `McpResourceManager` for actual MCP client communication.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use shannon_tools::mcp_tools::{ListMcpResourcesTool, ReadMcpResourceTool};
//! use shannon_mcp::McpResourceManager;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() {
//!     let manager = Arc::new(McpResourceManager::new());
//!     let list_tool = ListMcpResourcesTool::new(manager.clone());
//!     let read_tool = ReadMcpResourceTool::new(manager);
//! }
//! ```

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shannon_mcp::{
    ListResourcesInput, ListResourcesOutput, McpResourceManager, ReadResourceInput,
    ReadResourceOutput,
};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

// ---------------------------------------------------------------------------
// ListMcpResourcesTool
// ---------------------------------------------------------------------------

/// Tool that lists available resources from connected MCP servers.
///
/// Accepts an optional `server_name` parameter to filter results to a single
/// server. When omitted, resources from all connected servers are returned.
pub struct ListMcpResourcesTool {
    manager: Arc<McpResourceManager>,
    description: String,
}

impl ListMcpResourcesTool {
    /// Create a new tool backed by the given resource manager.
    pub fn new(manager: Arc<McpResourceManager>) -> Self {
        Self {
            manager,
            description: "List available resources from MCP (Model Context Protocol) servers. \
                          Returns resource URIs, names, descriptions, and MIME types. \
                          Optionally filter by server_name."
                .to_string(),
        }
    }
}

#[async_trait]
impl Tool for ListMcpResourcesTool {
    fn name(&self) -> &str {
        "list_mcp_resources"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server_name": {
                    "type": "string",
                    "description": "Optional MCP server name to filter resources by. When omitted, resources from all connected servers are returned."
                }
            }
        })
    }

    fn category(&self) -> &str {
        "mcp"
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let list_input: ListResourcesInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid list resources input: {e}")))?;

        let filter_desc = list_input.server_name.as_deref().unwrap_or("all servers");

        debug!(filter = filter_desc, "Listing MCP resources");

        let output: ListResourcesOutput =
            self.manager.list_resources(list_input).await.map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to list resources: {e}"))
            })?;

        let resource_count = output.resources.len();
        let server_names: Vec<&str> = output
            .resources
            .iter()
            .map(|r| r.server_name.as_str())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let mut metadata = HashMap::new();
        metadata.insert("count".to_string(), json!(resource_count));
        metadata.insert("servers".to_string(), json!(server_names));
        metadata.insert("resources".to_string(), json!(output.resources));

        Ok(ToolOutput {
            content: format!(
                "Found {} resource(s) across {} server(s)",
                resource_count,
                server_names.len()
            ),
            is_error: false,
            metadata,
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// ReadMcpResourceTool
// ---------------------------------------------------------------------------

/// Tool that reads the content of a specific MCP resource.
///
/// Requires both `server_name` and `uri` parameters.
pub struct ReadMcpResourceTool {
    manager: Arc<McpResourceManager>,
    description: String,
}

impl ReadMcpResourceTool {
    /// Create a new tool backed by the given resource manager.
    pub fn new(manager: Arc<McpResourceManager>) -> Self {
        Self {
            manager,
            description: "Read the content of a specific MCP (Model Context Protocol) resource \
                          by providing the server name and resource URI. Returns the resource \
                          content as text."
                .to_string(),
        }
    }
}

/// Deserialized input for the read tool.
#[derive(Debug, Deserialize, Serialize)]
struct ReadToolInput {
    server_name: String,
    uri: String,
}

#[async_trait]
impl Tool for ReadMcpResourceTool {
    fn name(&self) -> &str {
        "read_mcp_resource"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server_name": {
                    "type": "string",
                    "description": "Name of the MCP server that hosts the resource."
                },
                "uri": {
                    "type": "string",
                    "description": "URI of the resource to read."
                }
            },
            "required": ["server_name", "uri"]
        })
    }

    fn category(&self) -> &str {
        "mcp"
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let tool_input: ReadToolInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid read resource input: {e}")))?;

        debug!(
            server = %tool_input.server_name,
            uri = %tool_input.uri,
            "Reading MCP resource"
        );

        let mcp_input = ReadResourceInput {
            server_name: tool_input.server_name.clone(),
            uri: tool_input.uri.clone(),
        };

        let output: ReadResourceOutput = self
            .manager
            .read_resource(mcp_input)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read resource: {e}")))?;

        // Concatenate all content blocks into a single text string.
        let combined_text: String = output
            .contents
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        let mut metadata = HashMap::new();
        metadata.insert("server_name".to_string(), json!(tool_input.server_name));
        metadata.insert("uri".to_string(), json!(tool_input.uri));
        metadata.insert("content_count".to_string(), json!(output.contents.len()));
        if let Some(mime) = output.contents.first().and_then(|c| c.mime_type.as_ref()) {
            metadata.insert("mime_type".to_string(), json!(mime));
        }

        Ok(ToolOutput {
            content: combined_text,
            is_error: false,
            metadata,
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// ListPromptsTool
// ---------------------------------------------------------------------------

/// Tool that lists available prompts from MCP servers.
///
/// Optionally filter by `server_name`. Returns prompt names, descriptions,
/// and argument schemas.
pub struct ListPromptsTool {
    pool: Arc<shannon_mcp::McpProcessPool>,
    description: String,
}

impl ListPromptsTool {
    /// Create a new tool backed by the given process pool.
    pub fn new(pool: Arc<shannon_mcp::McpProcessPool>) -> Self {
        Self {
            pool,
            description: "List available prompts from MCP (Model Context Protocol) servers. \
                          Returns prompt names, descriptions, and argument schemas. \
                          Optionally filter by server_name."
                .to_string(),
        }
    }
}

#[async_trait]
impl Tool for ListPromptsTool {
    fn name(&self) -> &str {
        "list_mcp_prompts"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server_name": {
                    "type": "string",
                    "description": "Optional MCP server name to filter prompts by."
                }
            }
        })
    }

    fn category(&self) -> &str {
        "mcp"
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let server_name: Option<String> = input
            .get("server_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let prompts = match server_name {
            Some(ref name) => {
                let list = self.pool.list_prompts(name).await.map_err(|e| {
                    ToolError::ExecutionFailed(format!("Failed to list prompts: {e}"))
                })?;
                vec![(name.clone(), list)]
            }
            None => self.pool.list_all_prompts().await,
        };

        let total_count: usize = prompts.iter().map(|(_, p)| p.len()).sum();
        let server_count = prompts.len();

        let mut metadata = HashMap::new();
        metadata.insert("count".to_string(), json!(total_count));
        metadata.insert("servers".to_string(), json!(server_count));
        metadata.insert("prompts".to_string(), json!(prompts));

        Ok(ToolOutput {
            content: format!("Found {total_count} prompt(s) across {server_count} server(s)"),
            is_error: false,
            metadata,
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// GetPromptTool
// ---------------------------------------------------------------------------

/// Tool that gets a prompt from a specific MCP server.
///
/// Requires `server_name`, `prompt_name`, and optionally `arguments`.
pub struct GetPromptTool {
    pool: Arc<shannon_mcp::McpProcessPool>,
    description: String,
}

impl GetPromptTool {
    /// Create a new tool backed by the given process pool.
    pub fn new(pool: Arc<shannon_mcp::McpProcessPool>) -> Self {
        Self {
            pool,
            description: "Get a prompt from an MCP (Model Context Protocol) server. \
                          Provide server_name, prompt_name, and optional arguments to \
                          retrieve the prompt messages."
                .to_string(),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct GetPromptInput {
    server_name: String,
    prompt_name: String,
    arguments: Option<std::collections::HashMap<String, String>>,
}

#[async_trait]
impl Tool for GetPromptTool {
    fn name(&self) -> &str {
        "get_mcp_prompt"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server_name": {
                    "type": "string",
                    "description": "Name of the MCP server."
                },
                "prompt_name": {
                    "type": "string",
                    "description": "Name of the prompt to get."
                },
                "arguments": {
                    "type": "object",
                    "description": "Optional arguments for the prompt.",
                    "additionalProperties": { "type": "string" }
                }
            },
            "required": ["server_name", "prompt_name"]
        })
    }

    fn category(&self) -> &str {
        "mcp"
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let tool_input: GetPromptInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid get prompt input: {e}")))?;

        let result = self
            .pool
            .get_prompt(
                &tool_input.server_name,
                &tool_input.prompt_name,
                tool_input.arguments,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to get prompt: {e}")))?;

        let mut metadata = HashMap::new();
        metadata.insert("server_name".to_string(), json!(tool_input.server_name));
        metadata.insert("prompt_name".to_string(), json!(tool_input.prompt_name));
        metadata.insert("result".to_string(), result.clone());

        let content_str =
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());

        Ok(ToolOutput {
            content: content_str,
            is_error: false,
            metadata,
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// McpToolSearchTool
// ---------------------------------------------------------------------------

/// Tool that retrieves the full input schema for an MCP tool on demand.
///
/// When deferred tool schema loading is enabled, MCP tools register with
/// minimal schemas to reduce context usage. This tool allows the LLM to
/// fetch the full parameter specification before calling the actual tool.
///
/// Input: `{ "tool_name": "mcp__fetch__fetch" }`
/// Output: The full JSON Schema for that tool's input parameters.
pub struct McpToolSearchTool {
    pool: Arc<shannon_mcp::McpProcessPool>,
    description: String,
}

impl McpToolSearchTool {
    /// Create a new tool search tool backed by the given process pool.
    pub fn new(pool: Arc<shannon_mcp::McpProcessPool>) -> Self {
        Self {
            pool,
            description: "Search for an MCP tool's full parameter schema by tool name. \
                          Use this before calling any mcp__ tool to discover its required \
                          and optional parameters. Input: {\"tool_name\": \"mcp__server__tool\"}"
                .to_string(),
        }
    }
}

#[async_trait]
impl Tool for McpToolSearchTool {
    fn name(&self) -> &str {
        "mcp__tool_search"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tool_name": {
                    "type": "string",
                    "description": "Full tool name (e.g., \"mcp__fetch__fetch\") to retrieve the schema for."
                }
            },
            "required": ["tool_name"]
        })
    }

    fn category(&self) -> &str {
        "mcp"
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let tool_name = input
            .get("tool_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing 'tool_name' parameter".to_string()))?;

        match self.pool.get_deferred_schema(tool_name) {
            Some(schema) => {
                let schema_str =
                    serde_json::to_string_pretty(&schema).unwrap_or_else(|_| schema.to_string());
                Ok(ToolOutput::success(schema_str))
            }
            None => {
                // List available tools to help the LLM discover valid names.
                let available = self.pool.deferred_schema_tool_names();
                if available.is_empty() {
                    Ok(ToolOutput::error(format!(
                        "No deferred schema found for '{tool_name}'. \
                         Deferred tool loading may not be enabled, or this is not an MCP tool."
                    )))
                } else {
                    let mut suggestions: Vec<&str> = available
                        .iter()
                        .filter(|t| t.contains(tool_name) || tool_name.contains(t.as_str()))
                        .map(|t| t.as_str())
                        .collect();
                    suggestions.sort();
                    if suggestions.is_empty() {
                        suggestions = available.iter().take(10).map(|t| t.as_str()).collect();
                    }
                    Ok(ToolOutput::error(format!(
                        "No deferred schema found for '{tool_name}'. \
                         Similar tools: {}",
                        suggestions.join(", ")
                    )))
                }
            }
        }
    }

    fn is_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use shannon_mcp::protocol::Resource as ProtocolResource;
    use shannon_mcp::{ContentBlock, McpResourceClient, ResourceContent};

    /// A mock client that implements the McpResourceClient trait.
    struct MockClient {
        name: String,
        connected: bool,
        supports_res: bool,
        resources: Vec<ProtocolResource>,
    }

    impl MockClient {
        fn new(name: &str, connected: bool, supports_res: bool) -> Self {
            Self {
                name: name.to_string(),
                connected,
                supports_res,
                resources: Vec::new(),
            }
        }

        fn with_resource(mut self, uri: &str, name: &str, desc: &str) -> Self {
            self.resources.push(ProtocolResource {
                uri: uri.to_string(),
                name: name.to_string(),
                description: desc.to_string(),
                mime_type: Some("text/plain".to_string()),
            });
            self
        }
    }

    #[async_trait::async_trait]
    impl McpResourceClient for MockClient {
        fn server_name(&self) -> &str {
            &self.name
        }

        async fn is_connected(&self) -> bool {
            self.connected
        }

        async fn supports_resources(&self) -> bool {
            self.supports_res
        }

        async fn list_resources(&self) -> shannon_mcp::McpResult<Vec<ProtocolResource>> {
            Ok(self.resources.clone())
        }

        async fn read_resource(&self, uri: &str) -> shannon_mcp::McpResult<ResourceContent> {
            Ok(ResourceContent {
                uri: uri.to_string(),
                mime_type: Some("text/plain".to_string()),
                contents: vec![ContentBlock::Text {
                    text: format!("mock content for {uri}"),
                }],
            })
        }

        async fn supports_subscribe(&self) -> bool {
            false
        }

        async fn subscribe_resource(&self, _uri: &str) -> shannon_mcp::McpResult<bool> {
            Ok(false)
        }

        async fn unsubscribe_resource(&self, _uri: &str) -> shannon_mcp::McpResult<bool> {
            Ok(false)
        }
    }

    #[allow(dead_code)]
    fn make_manager() -> Arc<McpResourceManager> {
        let manager = Arc::new(McpResourceManager::new());
        let client = MockClient::new("test-server", true, true)
            .with_resource("file:///readme", "Readme", "Project readme")
            .with_resource("file:///config", "Config", "Configuration file");
        // We need to register in an async context, so we return the manager
        // and register in the test body.
        drop(client); // Don't need it here
        manager
    }

    #[tokio::test]
    async fn test_list_tool_name_and_schema() {
        let manager = Arc::new(McpResourceManager::new());
        let tool = ListMcpResourcesTool::new(manager);
        assert_eq!(tool.name(), "list_mcp_resources");
        assert_eq!(tool.category(), "mcp");

        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["server_name"].is_object());
    }

    #[tokio::test]
    async fn test_read_tool_name_and_schema() {
        let manager = Arc::new(McpResourceManager::new());
        let tool = ReadMcpResourceTool::new(manager);
        assert_eq!(tool.name(), "read_mcp_resource");
        assert_eq!(tool.category(), "mcp");

        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("server_name")));
        assert!(required.contains(&json!("uri")));
    }

    #[tokio::test]
    async fn test_list_tool_execute_empty() {
        let manager = Arc::new(McpResourceManager::new());
        let tool = ListMcpResourcesTool::new(manager);

        let result = tool.execute(json!({})).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("0 resource"));
    }

    #[tokio::test]
    async fn test_list_tool_execute_with_clients() {
        let manager = Arc::new(McpResourceManager::new());
        let client =
            MockClient::new("srv1", true, true).with_resource("file:///a", "A", "Resource A");
        manager.register(Arc::new(client)).await;

        let tool = ListMcpResourcesTool::new(manager);
        let result = tool.execute(json!({})).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("1 resource"));
    }

    #[tokio::test]
    async fn test_list_tool_execute_filtered() {
        let manager = Arc::new(McpResourceManager::new());
        let client =
            MockClient::new("srv1", true, true).with_resource("file:///a", "A", "Resource A");
        manager.register(Arc::new(client)).await;

        let tool = ListMcpResourcesTool::new(manager);
        let result = tool
            .execute(json!({ "server_name": "srv1" }))
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("1 resource"));
    }

    #[tokio::test]
    async fn test_list_tool_execute_unknown_server() {
        let manager = Arc::new(McpResourceManager::new());
        let tool = ListMcpResourcesTool::new(manager);

        let result = tool.execute(json!({ "server_name": "nonexistent" })).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_tool_execute() {
        let manager = Arc::new(McpResourceManager::new());
        let client =
            MockClient::new("srv1", true, true).with_resource("file:///a", "A", "Resource A");
        manager.register(Arc::new(client)).await;

        let tool = ReadMcpResourceTool::new(manager);
        let result = tool
            .execute(json!({
                "server_name": "srv1",
                "uri": "file:///a"
            }))
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("mock content for file:///a"));
    }

    #[tokio::test]
    async fn test_read_tool_missing_fields() {
        let manager = Arc::new(McpResourceManager::new());
        let tool = ReadMcpResourceTool::new(manager);

        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_tool_unknown_server() {
        let manager = Arc::new(McpResourceManager::new());
        let tool = ReadMcpResourceTool::new(manager);

        let result = tool
            .execute(json!({
                "server_name": "nope",
                "uri": "file:///x"
            }))
            .await;

        assert!(result.is_err());
    }
}
