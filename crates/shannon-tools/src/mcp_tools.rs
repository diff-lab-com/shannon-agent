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
use serde_json::{json, Value};
use shannon_mcp::{
    ListResourcesInput, ListResourcesOutput,
    ReadResourceInput, ReadResourceOutput,
    McpResourceManager,
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

        let filter_desc = list_input
            .server_name
            .as_deref()
            .unwrap_or("all servers");

        debug!(filter = filter_desc, "Listing MCP resources");

        let output: ListResourcesOutput = self
            .manager
            .list_resources(list_input)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to list resources: {e}")))?;

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
        metadata.insert(
            "resources".to_string(),
            json!(output.resources),
        );

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
    fn is_read_only(&self) -> bool {        true    }
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
    fn is_read_only(&self) -> bool {        true    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use shannon_mcp::{McpResourceClient, ResourceContent, ContentBlock};
    use shannon_mcp::protocol::Resource as ProtocolResource;

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
    }

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

        let result = tool
            .execute(json!({}))
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("0 resource"));
    }

    #[tokio::test]
    async fn test_list_tool_execute_with_clients() {
        let manager = Arc::new(McpResourceManager::new());
        let client = MockClient::new("srv1", true, true)
            .with_resource("file:///a", "A", "Resource A");
        manager.register(Arc::new(client)).await;

        let tool = ListMcpResourcesTool::new(manager);
        let result = tool
            .execute(json!({}))
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("1 resource"));
    }

    #[tokio::test]
    async fn test_list_tool_execute_filtered() {
        let manager = Arc::new(McpResourceManager::new());
        let client = MockClient::new("srv1", true, true)
            .with_resource("file:///a", "A", "Resource A");
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

        let result = tool
            .execute(json!({ "server_name": "nonexistent" }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_tool_execute() {
        let manager = Arc::new(McpResourceManager::new());
        let client = MockClient::new("srv1", true, true)
            .with_resource("file:///a", "A", "Resource A");
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
