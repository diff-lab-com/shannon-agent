//! MCP resource management tools
//!
//! Provides high-level resource listing and reading tools that wrap MCP client
//! operations. These tools are designed to be used from the tool execution layer
//! (shannon-tools) and provide structured input/output types for resource interactions.
//!
//! ## Architecture
//!
//! The resource tools operate against a `McpResourceManager`, which holds references
//! to connected MCP clients keyed by server name. The manager provides the dispatch
//! layer so that callers can target a specific server or aggregate across all servers.

use crate::protocol::{ContentBlock, Resource, ResourceContent};
use crate::{McpError, McpResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Resource descriptor (enriched with server provenance)
// ---------------------------------------------------------------------------

/// Describes a single MCP resource, including which server provides it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceDescriptor {
    /// Resource URI (unique within a server)
    pub uri: String,
    /// Human-readable resource name
    pub name: String,
    /// Optional description of the resource's contents
    pub description: Option<String>,
    /// MIME type hint (e.g. "text/plain", "application/json")
    pub mime_type: Option<String>,
    /// Name of the MCP server that exposes this resource
    pub server_name: String,
}

impl ResourceDescriptor {
    /// Build a `ResourceDescriptor` from a protocol-level `Resource` and the
    /// server name that owns it.
    pub fn from_resource(resource: &Resource, server_name: &str) -> Self {
        Self {
            uri: resource.uri.clone(),
            name: resource.name.clone(),
            description: if resource.description.is_empty() {
                None
            } else {
                Some(resource.description.clone())
            },
            mime_type: resource.mime_type.clone(),
            server_name: server_name.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Input / Output types
// ---------------------------------------------------------------------------

/// Input for listing MCP resources.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ListResourcesInput {
    /// Optional server name. When provided, only resources from that server
    /// are returned. When omitted, resources from all connected servers are
    /// aggregated.
    pub server_name: Option<String>,
}

/// Output from listing MCP resources.
#[derive(Debug, Clone, Serialize)]
pub struct ListResourcesOutput {
    /// The collected resource descriptors.
    pub resources: Vec<ResourceDescriptor>,
}

/// Input for reading a specific MCP resource.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReadResourceInput {
    /// The MCP server that hosts the resource.
    pub server_name: String,
    /// The URI of the resource to read.
    pub uri: String,
}

/// A single piece of resource content returned by a read operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceReadContent {
    /// The resource URI.
    pub uri: String,
    /// MIME type, if advertised by the server.
    pub mime_type: Option<String>,
    /// Textual content (extracted from `ContentBlock::Text` entries).
    pub text: String,
}

/// Output from reading a single MCP resource.
#[derive(Debug, Clone, Serialize)]
pub struct ReadResourceOutput {
    /// The content blocks returned by the server.
    pub contents: Vec<ResourceReadContent>,
}

// ---------------------------------------------------------------------------
// Trait: McpResourceClient
// ---------------------------------------------------------------------------

/// Abstraction over an MCP client's resource-related operations.
///
/// This trait exists so that `McpResourceManager` does not need to be generic
/// over the transport type `T`. Callers provide a thin implementation that
/// delegates to `McpClient<T>`.
#[async_trait::async_trait]
pub trait McpResourceClient: Send + Sync {
    /// Return the name of this server connection.
    fn server_name(&self) -> &str;

    /// Whether this client is currently connected.
    async fn is_connected(&self) -> bool;

    /// Whether the server advertised resource support during initialization.
    async fn supports_resources(&self) -> bool;

    /// List all resources exposed by this server.
    async fn list_resources(&self) -> McpResult<Vec<Resource>>;

    /// Read a specific resource by URI.
    async fn read_resource(&self, uri: &str) -> McpResult<ResourceContent>;
}

// ---------------------------------------------------------------------------
// McpResourceManager
// ---------------------------------------------------------------------------

/// Registry of connected MCP clients that support resource operations.
///
/// The manager is cheaply cloneable (wrapped in `Arc`) and can be shared
/// across async tasks.
#[derive(Clone)]
pub struct McpResourceManager {
    inner: Arc<RwLock<HashMap<String, Arc<dyn McpResourceClient>>>>,
}

impl McpResourceManager {
    /// Create a new empty resource manager.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an MCP client for resource operations.
    ///
    /// If a client with the same name already exists it is replaced.
    pub async fn register(&self, client: Arc<dyn McpResourceClient>) {
        let name = client.server_name().to_string();
        debug!(server = %name, "Registering MCP resource client");
        self.inner.write().await.insert(name, client);
    }

    /// Remove a previously registered client.
    pub async fn unregister(&self, server_name: &str) {
        debug!(server = %server_name, "Unregistering MCP resource client");
        self.inner.write().await.remove(server_name);
    }

    /// List all registered server names.
    pub async fn server_names(&self) -> Vec<String> {
        self.inner.read().await.keys().cloned().collect()
    }

    // -----------------------------------------------------------------------
    // list_resources
    // -----------------------------------------------------------------------

    /// List resources, optionally filtered by server name.
    pub async fn list_resources(
        &self,
        input: ListResourcesInput,
    ) -> McpResult<ListResourcesOutput> {
        let readers = self.inner.read().await;

        let target = input.server_name.as_deref();

        // If a specific server was requested, validate it exists.
        if let Some(name) = target {
            if !readers.contains_key(name) {
                let available = readers.keys().cloned().collect::<Vec<_>>().join(", ");
                return Err(McpError::InvalidRequest(format!(
                    "Server '{}' not found. Available servers: {}",
                    name, available
                )));
            }
        }

        let mut descriptors = Vec::new();

        for (server_name, client) in readers.iter() {
            // Apply server filter if present.
            if let Some(target_name) = target {
                if server_name != target_name {
                    continue;
                }
            }

            if !client.is_connected().await {
                warn!(server = %server_name, "Skipping disconnected server");
                continue;
            }

            if !client.supports_resources().await {
                debug!(server = %server_name, "Server does not support resources");
                continue;
            }

            match client.list_resources().await {
                Ok(resources) => {
                    for resource in &resources {
                        descriptors.push(ResourceDescriptor::from_resource(resource, server_name));
                    }
                }
                Err(e) => {
                    warn!(server = %server_name, error = %e, "Failed to list resources");
                }
            }
        }

        Ok(ListResourcesOutput {
            resources: descriptors,
        })
    }

    // -----------------------------------------------------------------------
    // read_resource
    // -----------------------------------------------------------------------

    /// Read a specific resource from a named MCP server.
    pub async fn read_resource(
        &self,
        input: ReadResourceInput,
    ) -> McpResult<ReadResourceOutput> {
        let readers = self.inner.read().await;

        let client = readers.get(&input.server_name).ok_or_else(|| {
            let available = readers.keys().cloned().collect::<Vec<_>>().join(", ");
            McpError::InvalidRequest(format!(
                "Server '{}' not found. Available servers: {}",
                input.server_name, available
            ))
        })?;

        if !client.is_connected().await {
            return Err(McpError::InvalidRequest(format!(
                "Server '{}' is not connected",
                input.server_name
            )));
        }

        if !client.supports_resources().await {
            return Err(McpError::InvalidRequest(format!(
                "Server '{}' does not support resources",
                input.server_name
            )));
        }

        let resource_content = client.read_resource(&input.uri).await?;

        // Convert protocol-level content blocks to our output type.
        let contents: Vec<ResourceReadContent> = resource_content
            .contents
            .iter()
            .map(|block| {
                let text = match block {
                    ContentBlock::Text { text } => text.clone(),
                    ContentBlock::Image { .. } => "[binary image data]".to_string(),
                    ContentBlock::Resource { text, .. } => {
                        text.clone().unwrap_or_else(|| "[embedded resource]".to_string())
                    }
                };
                ResourceReadContent {
                    uri: resource_content.uri.clone(),
                    mime_type: resource_content.mime_type.clone(),
                    text,
                }
            })
            .collect();

        Ok(ReadResourceOutput { contents })
    }
}

impl Default for McpResourceManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// McpClientAdapter
// ---------------------------------------------------------------------------

/// Adapter that wraps a concrete `McpClient<T>` into the `McpResourceClient` trait.
///
/// Because `McpClient<T>` is generic over the transport, we cannot store it
/// directly in the type-erased manager. The caller constructs an adapter and
/// registers it as an `Arc<dyn McpResourceClient>`.
pub struct McpClientAdapter<T: crate::transport::Transport> {
    server_name: String,
    client: crate::client::McpClient<T>,
}

impl<T: crate::transport::Transport + 'static> McpClientAdapter<T> {
    /// Create a new adapter wrapping the given client.
    pub fn new(server_name: impl Into<String>, client: crate::client::McpClient<T>) -> Self {
        Self {
            server_name: server_name.into(),
            client,
        }
    }
}

#[async_trait::async_trait]
impl<T: crate::transport::Transport + Send + Sync + 'static> McpResourceClient
    for McpClientAdapter<T>
{
    fn server_name(&self) -> &str {
        &self.server_name
    }

    async fn is_connected(&self) -> bool {
        // The client is considered connected if it has server capabilities set.
        self.client.capabilities().await.is_some()
    }

    async fn supports_resources(&self) -> bool {
        self.client.supports_resources().await
    }

    async fn list_resources(&self) -> McpResult<Vec<Resource>> {
        self.client.list_resources().await
    }

    async fn read_resource(&self, uri: &str) -> McpResult<ResourceContent> {
        self.client.read_resource(uri).await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Mock client for testing ------------------------------------------

    struct MockResourceClient {
        name: String,
        connected: bool,
        has_resources: bool,
        resources: Vec<Resource>,
    }

    impl MockResourceClient {
        fn new(name: &str, connected: bool, has_resources: bool) -> Self {
            Self {
                name: name.to_string(),
                connected,
                has_resources,
                resources: Vec::new(),
            }
        }

        fn with_resource(mut self, uri: &str, name: &str, description: &str) -> Self {
            self.resources.push(Resource {
                uri: uri.to_string(),
                name: name.to_string(),
                description: description.to_string(),
                mime_type: Some("text/plain".to_string()),
            });
            self
        }
    }

    #[async_trait::async_trait]
    impl McpResourceClient for MockResourceClient {
        fn server_name(&self) -> &str {
            &self.name
        }

        async fn is_connected(&self) -> bool {
            self.connected
        }

        async fn supports_resources(&self) -> bool {
            self.has_resources
        }

        async fn list_resources(&self) -> McpResult<Vec<Resource>> {
            Ok(self.resources.clone())
        }

        async fn read_resource(&self, uri: &str) -> McpResult<ResourceContent> {
            Ok(ResourceContent {
                uri: uri.to_string(),
                mime_type: Some("text/plain".to_string()),
                contents: vec![ContentBlock::Text {
                    text: format!("content of {}", uri),
                }],
            })
        }
    }

    // ---- Tests -----------------------------------------------------------

    #[tokio::test]
    async fn test_list_all_resources() {
        let manager = McpResourceManager::new();

        let client_a = MockResourceClient::new("server-a", true, true)
            .with_resource("file:///a/readme", "Readme", "Project readme");
        let client_b = MockResourceClient::new("server-b", true, true)
            .with_resource("db:///users", "Users", "User database");

        manager.register(Arc::new(client_a)).await;
        manager.register(Arc::new(client_b)).await;

        let output = manager
            .list_resources(ListResourcesInput { server_name: None })
            .await
            .unwrap();

        assert_eq!(output.resources.len(), 2);
    }

    #[tokio::test]
    async fn test_list_resources_filtered_by_server() {
        let manager = McpResourceManager::new();

        let client_a = MockResourceClient::new("server-a", true, true)
            .with_resource("file:///a/readme", "Readme", "Project readme");
        let client_b = MockResourceClient::new("server-b", true, true)
            .with_resource("db:///users", "Users", "User database");

        manager.register(Arc::new(client_a)).await;
        manager.register(Arc::new(client_b)).await;

        let output = manager
            .list_resources(ListResourcesInput {
                server_name: Some("server-a".to_string()),
            })
            .await
            .unwrap();

        assert_eq!(output.resources.len(), 1);
        assert_eq!(output.resources[0].server_name, "server-a");
        assert_eq!(output.resources[0].uri, "file:///a/readme");
    }

    #[tokio::test]
    async fn test_list_resources_skips_disconnected() {
        let manager = McpResourceManager::new();

        let client_offline = MockResourceClient::new("offline", false, true)
            .with_resource("file:///x", "X", "Should not appear");
        let client_online = MockResourceClient::new("online", true, true)
            .with_resource("file:///y", "Y", "Should appear");

        manager.register(Arc::new(client_offline)).await;
        manager.register(Arc::new(client_online)).await;

        let output = manager
            .list_resources(ListResourcesInput { server_name: None })
            .await
            .unwrap();

        assert_eq!(output.resources.len(), 1);
        assert_eq!(output.resources[0].name, "Y");
    }

    #[tokio::test]
    async fn test_list_resources_unknown_server() {
        let manager = McpResourceManager::new();
        let client = MockResourceClient::new("alpha", true, true);
        manager.register(Arc::new(client)).await;

        let result = manager
            .list_resources(ListResourcesInput {
                server_name: Some("nonexistent".to_string()),
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("nonexistent"));
        assert!(err.contains("alpha"));
    }

    #[tokio::test]
    async fn test_read_resource() {
        let manager = McpResourceManager::new();
        let client = MockResourceClient::new("my-server", true, true);
        manager.register(Arc::new(client)).await;

        let output = manager
            .read_resource(ReadResourceInput {
                server_name: "my-server".to_string(),
                uri: "file:///readme.md".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(output.contents.len(), 1);
        assert_eq!(output.contents[0].text, "content of file:///readme.md");
        assert_eq!(output.contents[0].uri, "file:///readme.md");
    }

    #[tokio::test]
    async fn test_read_resource_unknown_server() {
        let manager = McpResourceManager::new();
        let client = MockResourceClient::new("alpha", true, true);
        manager.register(Arc::new(client)).await;

        let result = manager
            .read_resource(ReadResourceInput {
                server_name: "beta".to_string(),
                uri: "file:///x".to_string(),
            })
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("beta"));
    }

    #[tokio::test]
    async fn test_read_resource_disconnected_server() {
        let manager = McpResourceManager::new();
        let client = MockResourceClient::new("offline", false, true);
        manager.register(Arc::new(client)).await;

        let result = manager
            .read_resource(ReadResourceInput {
                server_name: "offline".to_string(),
                uri: "file:///x".to_string(),
            })
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not connected"));
    }

    #[tokio::test]
    async fn test_read_resource_no_resource_support() {
        let manager = McpResourceManager::new();
        let client = MockResourceClient::new("no-res", true, false);
        manager.register(Arc::new(client)).await;

        let result = manager
            .read_resource(ReadResourceInput {
                server_name: "no-res".to_string(),
                uri: "file:///x".to_string(),
            })
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("does not support resources"));
    }

    #[tokio::test]
    async fn test_unregister_client() {
        let manager = McpResourceManager::new();
        let client = MockResourceClient::new("temp", true, true);
        manager.register(Arc::new(client)).await;

        assert!(manager.server_names().await.contains(&"temp".to_string()));
        manager.unregister("temp").await;
        assert!(!manager.server_names().await.contains(&"temp".to_string()));
    }

    #[tokio::test]
    async fn test_resource_descriptor_from_resource() {
        let resource = Resource {
            uri: "file:///test".to_string(),
            name: "Test Resource".to_string(),
            description: "A test".to_string(),
            mime_type: Some("text/plain".to_string()),
        };

        let desc = ResourceDescriptor::from_resource(&resource, "my-server");
        assert_eq!(desc.uri, "file:///test");
        assert_eq!(desc.name, "Test Resource");
        assert_eq!(desc.description, Some("A test".to_string()));
        assert_eq!(desc.mime_type, Some("text/plain".to_string()));
        assert_eq!(desc.server_name, "my-server");
    }

    #[tokio::test]
    async fn test_resource_descriptor_empty_description() {
        let resource = Resource {
            uri: "file:///test".to_string(),
            name: "Test".to_string(),
            description: String::new(),
            mime_type: None,
        };

        let desc = ResourceDescriptor::from_resource(&resource, "srv");
        assert_eq!(desc.description, None);
        assert_eq!(desc.mime_type, None);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let input = ListResourcesInput {
            server_name: Some("my-server".to_string()),
        };
        let json_str = serde_json::to_string(&input).unwrap();
        let parsed: ListResourcesInput = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.server_name, Some("my-server".to_string()));

        let read_input = ReadResourceInput {
            server_name: "my-server".to_string(),
            uri: "file:///test".to_string(),
        };
        let json_str = serde_json::to_string(&read_input).unwrap();
        let parsed: ReadResourceInput = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.server_name, "my-server");
        assert_eq!(parsed.uri, "file:///test");
    }
}
