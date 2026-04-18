//! MCP Server Manager — discovers MCP config and registers tools.
//!
//! Reads config files (via [`crate::config::discover_config`]), starts server
//! processes, discovers remote tools, and returns [`McpToolAdapter`] instances
//! that implement the `Tool` trait for registration into the main `ToolRegistry`.
//!
//! Delegates to [`shannon_core::mcp_tool_adapter::discover_tools`] for the
//! actual stdio tool-discovery handshake.

use crate::config::{discover_config, McpServerConfig};
use shannon_core::mcp_tool_adapter::McpToolAdapter;
use shannon_core::mcp_tool_adapter::discover_tools;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, error, info, warn};

// ---------------------------------------------------------------------------
// discover_and_register — convenience entry point
// ---------------------------------------------------------------------------

/// Result of discovering all MCP servers from config.
pub struct McpDiscoveryResult {
    /// Successfully discovered servers: (server_name, tool_count).
    pub servers: Vec<(String, usize)>,
    /// Tool adapters ready to register in a ToolRegistry.
    pub tools: Vec<McpToolAdapter>,
}

/// Discover MCP config files, start servers, and collect tool adapters.
///
/// This is the main entry point. Call from the application startup to
/// discover all configured MCP servers and obtain tool adapters for
/// registration.
///
/// # Example
///
/// ```rust,no_run
/// use shannon_mcp::server_manager::discover_all_servers;
/// use shannon_core::tools::ToolRegistry;
///
/// async fn setup_mcp(project_dir: &std::path::Path, registry: &mut ToolRegistry) {
///     let result = discover_all_servers(project_dir).await;
///     for adapter in result.tools {
///         let _ = registry.register(Box::new(adapter));
///     }
/// }
/// ```
pub async fn discover_all_servers(project_dir: &Path) -> McpDiscoveryResult {
    let config = match discover_config(project_dir) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "Failed to discover MCP config");
            return McpDiscoveryResult {
                servers: Vec::new(),
                tools: Vec::new(),
            };
        }
    };

    if config.mcp_servers.is_empty() {
        debug!("No MCP servers configured");
        return McpDiscoveryResult {
            servers: Vec::new(),
            tools: Vec::new(),
        };
    }

    info!(
        server_count = config.mcp_servers.len(),
        "Discovering tools from MCP servers"
    );

    let mut servers = Vec::new();
    let mut tools = Vec::new();

    for (name, server_config) in &config.mcp_servers {
        match discover_server_tools(name, server_config).await {
            Ok(discovered) => {
                let tool_count = discovered.len();
                info!(
                    server = %name,
                    tools = tool_count,
                    "MCP server tools discovered"
                );
                servers.push((name.clone(), tool_count));
                tools.extend(discovered);
            }
            Err(e) => {
                error!(server = %name, error = %e, "Failed to discover MCP server tools");
            }
        }
    }

    info!(
        servers = servers.len(),
        total_tools = tools.len(),
        "MCP discovery complete"
    );

    McpDiscoveryResult { servers, tools }
}

/// Discover tools from a single MCP server.
async fn discover_server_tools(
    name: &str,
    config: &McpServerConfig,
) -> Result<Vec<McpToolAdapter>, String> {
    match config {
        McpServerConfig::Stdio {
            command,
            args,
            env,
        } => {
            let args_owned: Vec<String> = args.clone();
            let env_owned: HashMap<String, String> = env.clone();

            let result = discover_tools(
                name,
                command,
                &args_owned,
                &env_owned,
            )
            .await?;

            Ok(result.tools)
        }
        McpServerConfig::Sse { url, .. } => {
            // SSE transport: not yet supported by the adapter's discover_tools.
            // Will be added when persistent McpClient connections are integrated.
            warn!(
                server = %name,
                url = %url,
                "SSE transport MCP servers not yet supported for auto-discovery"
            );
            Ok(Vec::new())
        }
        McpServerConfig::Http { url, .. } => {
            warn!(
                server = %name,
                url = %url,
                "HTTP transport MCP servers not yet supported for auto-discovery"
            );
            Ok(Vec::new())
        }
        McpServerConfig::WebSocket { url } => {
            warn!(
                server = %name,
                url = %url,
                "WebSocket transport MCP servers not yet supported for auto-discovery"
            );
            Ok(Vec::new())
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discover_all_servers_no_config() {
        let temp = tempfile::tempdir().unwrap();
        let result = discover_all_servers(temp.path()).await;
        assert!(result.servers.is_empty());
        assert!(result.tools.is_empty());
    }

    #[tokio::test]
    async fn test_discover_all_servers_with_empty_config() {
        let temp = tempfile::tempdir().unwrap();
        let config = serde_json::json!({
            "mcpServers": {}
        });
        std::fs::write(temp.path().join(".mcp.json"), serde_json::to_string(&config).unwrap())
            .unwrap();

        let result = discover_all_servers(temp.path()).await;
        assert!(result.servers.is_empty());
        assert!(result.tools.is_empty());
    }
}
