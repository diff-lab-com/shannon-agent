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
#[derive(Debug, Clone, Serialize)]
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
                "Server '{}' is not connected",
                input.server
            )));
        }

        if !client.supports_resources {
            return Err(ToolError::InvalidInput(format!(
                "Server '{}' does not support resources",
                input.server
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
                    "Server '{}' is not connected",
                    target_server
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
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid read input: {}", e)))?;
                let server = read_input.server.clone();
                let output = self.read_resource(read_input).await?;
                let resource_count = output.contents.len();
                Ok(ToolOutput {
                    content: format!("Read {} MCP resource(s) from server {}", resource_count, server),
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
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid list input: {}", e)))?;
                let output = self.list_resources(list_input).await?;
                let resource_count = output.resources.as_ref().map(|r| r.len()).unwrap_or(0);
                Ok(ToolOutput {
                    content: format!("Found {} resources on MCP servers", resource_count),
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
                "Unknown operation: {}",
                operation
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
}
