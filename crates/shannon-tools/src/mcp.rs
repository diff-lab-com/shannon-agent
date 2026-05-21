//! MCP (Model Context Protocol) resource management tools
//!
//! Provides implementations for:
//! - ReadMcpResource: Read a specific MCP resource by URI
//! - ListMcpResources: List available resources from MCP servers
//!
//! These tools enable interaction with MCP servers for extended capabilities.

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

/// MCP resource content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceContent {
    /// Resource URI
    pub uri: String,

    /// MIME type of the content
    pub mime_type: Option<String>,

    /// Text content (for text resources)
    pub text: Option<String>,

    /// Path where binary blob was saved (for binary resources)
    pub blob_saved_to: Option<String>,
}

/// Input for reading MCP resource
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReadMcpResourceInput {
    /// MCP server name
    pub server: String,

    /// Resource URI to read
    pub uri: String,
}

/// Output from reading MCP resource
#[derive(Debug, Serialize)]
pub struct ReadMcpResourceOutput {
    /// List of resource contents
    pub contents: Vec<McpResourceContent>,
}

/// Input for listing MCP resources
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ListMcpResourcesInput {
    /// Optional server name to filter resources by
    pub server: Option<String>,
}

/// Output from listing MCP resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceInfo {
    /// Resource URI
    pub uri: String,

    /// Resource name
    pub name: String,

    /// MIME type
    pub mime_type: Option<String>,

    /// Resource description
    pub description: Option<String>,

    /// Server that provides this resource
    pub server: String,
}

/// Output from listing MCP resources
#[derive(Debug, Serialize)]
pub struct ListMcpResourcesOutput {
    /// List of available resources
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Vec<McpResourceInfo>>,
}

/// Mock MCP client state
type McpClientRegistry = HashMap<String, McpClientInfo>;

/// MCP client information
#[derive(Debug)]
struct McpClientInfo {
    /// Client name
    pub name: String,

    /// Whether client is connected
    pub connected: bool,

    /// Whether client supports resources
    pub supports_resources: bool,

    /// Available resources
    pub resources: Vec<McpResourceInfo>,
}

/// Global MCP client registry (in-memory mock)
fn get_client_registry() -> McpClientRegistry {
    // In a real implementation, this would be populated from actual MCP connections
    let mut registry = HashMap::new();

    registry.insert(
        "serena".to_string(),
        McpClientInfo {
            name: "serena".to_string(),
            connected: true,
            supports_resources: true,
            resources: vec![McpResourceInfo {
                uri: "file:///project/memory".to_string(),
                name: "Project Memory".to_string(),
                mime_type: Some("text/plain".to_string()),
                description: Some("Persistent memory for the project".to_string()),
                server: "serena".to_string(),
            }],
        },
    );

    registry.insert(
        "sequential".to_string(),
        McpClientInfo {
            name: "sequential".to_string(),
            connected: true,
            supports_resources: true,
            resources: vec![
                McpResourceInfo {
                    uri: "sequential://thoughts".to_string(),
                    name: "Thought Chain".to_string(),
                    mime_type: Some("application/json".to_string()),
                    description: Some("Sequential thinking chain".to_string()),
                    server: "sequential".to_string(),
                },
            ],
        },
    );

    registry
}

/// MCP resource tool
pub struct McpResourceTool {
    description: String,
}

impl Default for McpResourceTool {
    fn default() -> Self {
        Self::new()
    }
}

impl McpResourceTool {
    pub fn new() -> Self {
        Self {
            description: "Read and list resources from MCP (Model Context Protocol) servers".to_string(),
        }
    }

    /// Read a specific MCP resource
    async fn read_resource(&self, input: ReadMcpResourceInput) -> Result<ReadMcpResourceOutput, ToolError> {
        let registry = get_client_registry();

        let client = registry
            .get(&input.server)
            .ok_or_else(|| ToolError::InvalidInput(format!(
                "Server '{}' not found. Available servers: {}",
                input.server,
                registry.keys().cloned().collect::<Vec<_>>().join(", ")
            )))?;

        if !client.connected {
            return Err(ToolError::InvalidInput(format!(
                "Server '{}' ({}) is not connected",
                client.name, input.server
            )));
        }

        if !client.supports_resources {
            return Err(ToolError::InvalidInput(format!(
                "Server '{}' ({}) does not support resources",
                client.name, input.server
            )));
        }

        // In a real implementation, this would make an actual MCP request
        // For now, return mock content based on the URI
        let content = McpResourceContent {
            uri: input.uri.clone(),
            mime_type: Some("text/plain".to_string()),
            text: Some(format!(
                "Mock content for resource '{}' from server '{}'",
                input.uri, input.server
            )),
            blob_saved_to: None,
        };

        Ok(ReadMcpResourceOutput {
            contents: vec![content],
        })
    }

    /// List available MCP resources
    async fn list_resources(&self, input: ListMcpResourcesInput) -> Result<ListMcpResourcesOutput, ToolError> {
        let registry = get_client_registry();

        let mut resources = Vec::new();

        if let Some(target_server) = input.server {
            let client = registry
                .get(&target_server)
                .ok_or_else(|| ToolError::InvalidInput(format!(
                    "Server '{}' not found. Available servers: {}",
                    target_server,
                    registry.keys().cloned().collect::<Vec<_>>().join(", ")
                )))?;

            if !client.connected {
                return Err(ToolError::InvalidInput(format!(
                    "Server '{target_server}' is not connected"
                )));
            }

            // Build resource info
            for resource in &client.resources {
                resources.push(McpResourceInfo {
                    uri: resource.uri.clone(),
                    name: resource.name.clone(),
                    mime_type: resource.mime_type.clone(),
                    description: resource.description.clone(),
                    server: target_server.clone(),
                });
            }
        } else {
            // Collect resources from all connected servers
            for (server_name, client) in registry.iter() {
                if client.connected {
                    for resource in &client.resources {
                        resources.push(McpResourceInfo {
                            uri: resource.uri.clone(),
                            name: resource.name.clone(),
                            mime_type: resource.mime_type.clone(),
                            description: resource.description.clone(),
                            server: server_name.clone(),
                        });
                    }
                }
            }
        }

        Ok(ListMcpResourcesOutput { resources: Some(resources) })
    }
}

#[async_trait]
impl Tool for McpResourceTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        // Parse operation type from input
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing operation field".to_string()))?;

        match operation {
            "Read" => {
                let read_input: ReadMcpResourceInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid read input: {e}")))?;
                let server = read_input.server.clone();
                let output = self.read_resource(read_input).await?;
                let resource_count = output.contents.len();
                Ok(ToolOutput {
                    content: format!("Read {resource_count} MCP resource(s) from server {server}"),
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("server".to_string(), json!(server));
                        map.insert("contents".to_string(), json!(output.contents));
                        map
                    },
                })
            }
            "List" => {
                let list_input: ListMcpResourcesInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid list input: {e}")))?;
                let output = self.list_resources(list_input).await?;
                let resource_count = output.resources.as_ref().map(|r| r.len()).unwrap_or(0);
                Ok(ToolOutput {
                    content: format!("Found {resource_count} resources on MCP servers"),
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        if let Some(ref resources) = output.resources {
                            map.insert("resources".to_string(), json!(resources));
                        }
                        map
                    },
                })
            }
            _ => Err(ToolError::InvalidInput(format!(
                "Unknown operation: {operation}"
            ))),
        }
    }

    fn name(&self) -> &str {
        "McpResource"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "description": "Operation type",
                    "enum": ["Read", "List"]
                },
                "server": {
                    "type": "string",
                    "description": "MCP server name"
                },
                "uri": {
                    "type": "string",
                    "description": "Resource URI"
                }
            },
            "required": ["operation"]
        })
    }
    fn is_read_only(&self) -> bool {        true    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── McpResourceContent ──────────────────────────────────────

    #[test]
    fn mcp_resource_content_serde_roundtrip() {
        let content = McpResourceContent {
            uri: "file:///project/memory".to_string(),
            mime_type: Some("text/plain".to_string()),
            text: Some("hello world".to_string()),
            blob_saved_to: None,
        };
        let json = serde_json::to_string(&content).unwrap();
        let deserialized: McpResourceContent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.uri, "file:///project/memory");
        assert_eq!(deserialized.mime_type, Some("text/plain".to_string()));
        assert_eq!(deserialized.text, Some("hello world".to_string()));
        assert!(deserialized.blob_saved_to.is_none());
    }

    #[test]
    fn mcp_resource_content_minimal() {
        let content = McpResourceContent {
            uri: "test://resource".to_string(),
            mime_type: None,
            text: None,
            blob_saved_to: None,
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("test://resource"));
        let back: McpResourceContent = serde_json::from_str(&json).unwrap();
        assert!(back.mime_type.is_none());
        assert!(back.text.is_none());
    }

    #[test]
    fn mcp_resource_content_with_blob() {
        let content = McpResourceContent {
            uri: "file:///data.bin".to_string(),
            mime_type: Some("application/octet-stream".to_string()),
            text: None,
            blob_saved_to: Some("/tmp/blob.dat".to_string()),
        };
        let json = serde_json::to_string(&content).unwrap();
        let back: McpResourceContent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.blob_saved_to, Some("/tmp/blob.dat".to_string()));
    }

    // ── ReadMcpResourceInput ────────────────────────────────────

    #[test]
    fn read_mcp_resource_input_serde() {
        let input = ReadMcpResourceInput {
            server: "serena".to_string(),
            uri: "file:///project/memory".to_string(),
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("serena"));
        assert!(json.contains("file:///project/memory"));
        let back: ReadMcpResourceInput = serde_json::from_str(&json).unwrap();
        assert_eq!(back.server, "serena");
        assert_eq!(back.uri, "file:///project/memory");
    }

    #[test]
    fn read_mcp_resource_input_from_json_value() {
        let val = serde_json::json!({
            "server": "sequential",
            "uri": "sequential://thoughts"
        });
        let input: ReadMcpResourceInput = serde_json::from_value(val).unwrap();
        assert_eq!(input.server, "sequential");
    }

    // ── ListMcpResourcesInput ───────────────────────────────────

    #[test]
    fn list_mcp_resources_input_with_server() {
        let input = ListMcpResourcesInput {
            server: Some("serena".to_string()),
        };
        let json = serde_json::to_string(&input).unwrap();
        let back: ListMcpResourcesInput = serde_json::from_str(&json).unwrap();
        assert_eq!(back.server, Some("serena".to_string()));
    }

    #[test]
    fn list_mcp_resources_input_without_server() {
        let input = ListMcpResourcesInput { server: None };
        let json = serde_json::to_string(&input).unwrap();
        let back: ListMcpResourcesInput = serde_json::from_str(&json).unwrap();
        assert!(back.server.is_none());
    }

    // ── McpResourceInfo ─────────────────────────────────────────

    #[test]
    fn mcp_resource_info_serde_roundtrip() {
        let info = McpResourceInfo {
            uri: "file:///project/memory".to_string(),
            name: "Project Memory".to_string(),
            mime_type: Some("text/plain".to_string()),
            description: Some("Persistent memory".to_string()),
            server: "serena".to_string(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: McpResourceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.uri, "file:///project/memory");
        assert_eq!(back.name, "Project Memory");
        assert_eq!(back.server, "serena");
        assert_eq!(back.mime_type, Some("text/plain".to_string()));
    }

    #[test]
    fn mcp_resource_info_minimal() {
        let info = McpResourceInfo {
            uri: "test://x".to_string(),
            name: "Test".to_string(),
            mime_type: None,
            description: None,
            server: "test".to_string(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: McpResourceInfo = serde_json::from_str(&json).unwrap();
        assert!(back.mime_type.is_none());
        assert!(back.description.is_none());
    }

    // ── McpResourceTool trait ───────────────────────────────────

    #[test]
    fn mcp_resource_tool_name() {
        let tool = McpResourceTool::new();
        assert_eq!(tool.name(), "McpResource");
    }

    #[test]
    fn mcp_resource_tool_description() {
        let tool = McpResourceTool::new();
        assert!(tool.description().contains("MCP"));
    }

    #[test]
    fn mcp_resource_tool_default() {
        let tool = McpResourceTool::default();
        assert_eq!(tool.name(), "McpResource");
    }

    #[test]
    fn mcp_resource_tool_input_schema() {
        let tool = McpResourceTool::new();
        let schema = tool.input_schema();
        let ops = schema["properties"]["operation"]["enum"].as_array().unwrap();
        assert!(ops.iter().any(|v| v.as_str() == Some("Read")));
        assert!(ops.iter().any(|v| v.as_str() == Some("List")));
    }

    #[test]
    fn mcp_resource_tool_is_read_only() {
        let tool = McpResourceTool::new();
        assert!(tool.is_read_only());
    }

    // ── McpResourceTool read_resource (mock) ────────────────────

    #[tokio::test]
    async fn read_resource_serena() {
        let tool = McpResourceTool::new();
        let input = ReadMcpResourceInput {
            server: "serena".to_string(),
            uri: "file:///project/memory".to_string(),
        };
        let output = tool.read_resource(input).await.unwrap();
        assert_eq!(output.contents.len(), 1);
        assert_eq!(output.contents[0].uri, "file:///project/memory");
        assert!(output.contents[0].text.is_some());
    }

    #[tokio::test]
    async fn read_resource_unknown_server() {
        let tool = McpResourceTool::new();
        let input = ReadMcpResourceInput {
            server: "nonexistent".to_string(),
            uri: "test://x".to_string(),
        };
        let err = tool.read_resource(input).await.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    // ── McpResourceTool list_resources (mock) ───────────────────

    #[tokio::test]
    async fn list_resources_all_servers() {
        let tool = McpResourceTool::new();
        let input = ListMcpResourcesInput { server: None };
        let output = tool.list_resources(input).await.unwrap();
        let resources = output.resources.unwrap();
        assert!(resources.len() >= 2); // serena + sequential
    }

    #[tokio::test]
    async fn list_resources_filtered_by_server() {
        let tool = McpResourceTool::new();
        let input = ListMcpResourcesInput {
            server: Some("serena".to_string()),
        };
        let output = tool.list_resources(input).await.unwrap();
        let resources = output.resources.unwrap();
        assert!(resources.iter().all(|r| r.server == "serena"));
    }

    #[tokio::test]
    async fn list_resources_unknown_server() {
        let tool = McpResourceTool::new();
        let input = ListMcpResourcesInput {
            server: Some("nonexistent".to_string()),
        };
        let err = tool.list_resources(input).await.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    // ── Tool execute ────────────────────────────────────────────

    #[tokio::test]
    async fn execute_read_operation() {
        let tool = McpResourceTool::new();
        let input = json!({
            "operation": "Read",
            "server": "serena",
            "uri": "file:///project/memory"
        });
        let output = tool.execute(input).await.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("MCP resource"));
    }

    #[tokio::test]
    async fn execute_list_operation() {
        let tool = McpResourceTool::new();
        let input = json!({
            "operation": "List"
        });
        let output = tool.execute(input).await.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("resources"));
    }

    #[tokio::test]
    async fn execute_unknown_operation() {
        let tool = McpResourceTool::new();
        let input = json!({
            "operation": "Delete"
        });
        let err = tool.execute(input).await.unwrap_err();
        assert!(err.to_string().contains("Unknown operation"));
    }

    #[tokio::test]
    async fn execute_missing_operation() {
        let tool = McpResourceTool::new();
        let input = json!({"server": "serena"});
        let err = tool.execute(input).await.unwrap_err();
        assert!(err.to_string().contains("operation"));
    }

    #[test]
    fn send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<McpResourceTool>();
        assert_send_sync::<McpResourceContent>();
        assert_send_sync::<ReadMcpResourceInput>();
        assert_send_sync::<ListMcpResourcesInput>();
        assert_send_sync::<McpResourceInfo>();
    }
}
