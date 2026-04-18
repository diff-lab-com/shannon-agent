//! MCP Server Manager — discovers MCP config and registers tools.
//!
//! Reads config files (via [`crate::config::discover_config`]), starts server
//! processes, discovers remote tools, and returns tool adapter instances
//! that implement the `Tool` trait for registration into the main `ToolRegistry`.
//!
//! Two modes are available:
//! - **Pooled** (`discover_all_servers_pooled`): Persistent processes via
//!   [`crate::process_pool::McpProcessPool`] — zero-overhead after startup.
//! - **Legacy** (`discover_all_servers`): One-shot process per tool call.

use crate::config::{discover_config, McpServerConfig};
use crate::process_pool::{
    discover_pooled_tools, McpProcessPool, PooledMcpToolAdapter,
};
use shannon_core::{McpToolAdapter, discover_tools};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

// ---------------------------------------------------------------------------
// Pooled discovery (preferred)
// ---------------------------------------------------------------------------

/// Result of discovering all MCP servers using persistent connections.
pub struct PooledMcpDiscoveryResult {
    /// Successfully discovered servers: (server_name, tool_count).
    pub servers: Vec<(String, usize)>,
    /// Tool adapters ready to register in a ToolRegistry.
    pub tools: Vec<PooledMcpToolAdapter>,
    /// Shared process pool — keep alive for the application lifetime.
    pub pool: Arc<McpProcessPool>,
}

/// Discover MCP servers using persistent connections (preferred).
///
/// Starts each server process once, keeps it alive via the pool,
/// and returns pooled adapters for zero-overhead tool execution.
pub async fn discover_all_servers_pooled(
    project_dir: &Path,
) -> PooledMcpDiscoveryResult {
    let config = match discover_config(project_dir) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "Failed to discover MCP config");
            return PooledMcpDiscoveryResult {
                servers: Vec::new(),
                tools: Vec::new(),
                pool: Arc::new(McpProcessPool::new()),
            };
        }
    };

    if config.mcp_servers.is_empty() {
        debug!("No MCP servers configured");
        return PooledMcpDiscoveryResult {
            servers: Vec::new(),
            tools: Vec::new(),
            pool: Arc::new(McpProcessPool::new()),
        };
    }

    info!(
        server_count = config.mcp_servers.len(),
        "Discovering tools from MCP servers (pooled)"
    );

    let pool = Arc::new(McpProcessPool::new());
    let mut servers = Vec::new();
    let mut tools = Vec::new();

    for (name, server_config) in &config.mcp_servers {
        match server_config {
            McpServerConfig::Stdio { command, args, env } => {
                match discover_pooled_tools(
                    pool.clone(),
                    name,
                    command,
                    args,
                    env,
                )
                .await
                {
                    Ok(discovered) => {
                        let tool_count = discovered.tools.len();
                        info!(
                            server = %name,
                            tools = tool_count,
                            "MCP server tools discovered (pooled)"
                        );
                        servers.push((name.clone(), tool_count));
                        tools.extend(discovered.tools);
                    }
                    Err(e) => {
                        error!(
                            server = %name,
                            error = %e,
                            "Failed to discover MCP server tools (pooled)"
                        );
                    }
                }
            }
            McpServerConfig::Sse { url, .. } => {
                warn!(
                    server = %name,
                    url = %url,
                    "SSE transport not yet supported for pooled discovery"
                );
            }
            McpServerConfig::Http { url, .. } => {
                warn!(
                    server = %name,
                    url = %url,
                    "HTTP transport not yet supported for pooled discovery"
                );
            }
            McpServerConfig::WebSocket { url, .. } => {
                warn!(
                    server = %name,
                    url = %url,
                    "WebSocket transport not yet supported for pooled discovery"
                );
            }
        }
    }

    pool.start_health_checks();

    info!(
        servers = servers.len(),
        total_tools = tools.len(),
        "Pooled MCP discovery complete"
    );

    PooledMcpDiscoveryResult {
        servers,
        tools,
        pool,
    }
}

// ---------------------------------------------------------------------------
// Legacy discovery (one-shot processes)
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
/// This is the legacy entry point using one-shot processes.
/// Prefer [`discover_all_servers_pooled`] for persistent connections.
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

/// Discover tools from a single MCP server (legacy one-shot).
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
        McpServerConfig::WebSocket { url, .. } => {
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
