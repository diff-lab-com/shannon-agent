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

use crate::config::{McpServerConfig, discover_config};
use crate::process_pool::{McpProcessPool, PooledMcpToolAdapter, discover_pooled_tools};
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
pub async fn discover_all_servers_pooled(project_dir: &Path) -> PooledMcpDiscoveryResult {
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
                match discover_pooled_tools(pool.clone(), name, command, args, env).await {
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
            McpServerConfig::Sse { url, headers, auth } => {
                match pool
                    .start_remote_server(name, url, headers.clone(), auth.clone())
                    .await
                {
                    Ok(()) => {
                        // Discover tools from the remote server.
                        match discover_remote_pooled_tools(pool.clone(), name).await {
                            Ok(discovered) => {
                                let tool_count = discovered.tools.len();
                                info!(
                                    server = %name,
                                    tools = tool_count,
                                    "Remote MCP server tools discovered (SSE)"
                                );
                                servers.push((name.clone(), tool_count));
                                tools.extend(discovered.tools);
                            }
                            Err(e) => {
                                error!(
                                    server = %name,
                                    error = %e,
                                    "Failed to discover remote MCP server tools (SSE)"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            server = %name,
                            url = %url,
                            error = %e,
                            "Failed to start remote MCP server (SSE)"
                        );
                    }
                }
            }
            McpServerConfig::Http { url, headers, auth } => {
                match pool
                    .start_remote_server(name, url, headers.clone(), auth.clone())
                    .await
                {
                    Ok(()) => match discover_remote_pooled_tools(pool.clone(), name).await {
                        Ok(discovered) => {
                            let tool_count = discovered.tools.len();
                            info!(
                                server = %name,
                                tools = tool_count,
                                "Remote MCP server tools discovered (HTTP)"
                            );
                            servers.push((name.clone(), tool_count));
                            tools.extend(discovered.tools);
                        }
                        Err(e) => {
                            error!(
                                server = %name,
                                error = %e,
                                "Failed to discover remote MCP server tools (HTTP)"
                            );
                        }
                    },
                    Err(e) => {
                        error!(
                            server = %name,
                            url = %url,
                            error = %e,
                            "Failed to start remote MCP server (HTTP)"
                        );
                    }
                }
            }
            McpServerConfig::WebSocket { url, auth } => {
                match pool.start_websocket_server(name, url, auth.clone()).await {
                    Ok(()) => match discover_remote_pooled_tools(pool.clone(), name).await {
                        Ok(discovered) => {
                            let tool_count = discovered.tools.len();
                            info!(
                                server = %name,
                                tools = tool_count,
                                "WebSocket MCP server tools discovered"
                            );
                            servers.push((name.clone(), tool_count));
                            tools.extend(discovered.tools);
                        }
                        Err(e) => {
                            error!(
                                server = %name,
                                error = %e,
                                "Failed to discover WebSocket MCP server tools"
                            );
                        }
                    },
                    Err(e) => {
                        error!(
                            server = %name,
                            url = %url,
                            error = %e,
                            "Failed to start WebSocket MCP server"
                        );
                    }
                }
            }
        }
    }

    let pool_for_health = pool.clone();
    tokio::spawn(async move {
        pool_for_health.start_health_checks().await;
    });

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

/// Non-blocking variant of [`discover_all_servers_pooled`].
///
/// Returns immediately with an empty tool list and a shared pool. Each server
/// is connected in a background tokio task. When a server's tools are
/// discovered, the `on_tools_ready` callback is invoked with the tool adapters,
/// allowing the caller to register them dynamically.
///
/// This avoids blocking REPL startup on slow MCP servers.
pub fn discover_all_servers_pooled_nonblocking(
    project_dir: &Path,
    on_tools_ready: Arc<dyn Fn(Vec<PooledMcpToolAdapter>) + Send + Sync>,
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
        "Starting non-blocking MCP server discovery"
    );

    let pool = Arc::new(McpProcessPool::new());

    // Start health checks in background.
    let pool_for_health = pool.clone();
    tokio::spawn(async move {
        pool_for_health.start_health_checks().await;
    });

    // Spawn a background task for each server.
    for (name, server_config) in config.mcp_servers {
        let pool = pool.clone();
        let on_tools_ready = on_tools_ready.clone();
        let server_name = name.clone();

        let task = async move {
            let result = match server_config {
                McpServerConfig::Stdio { command, args, env } => {
                    discover_pooled_tools(pool.clone(), &server_name, &command, &args, &env).await
                }
                McpServerConfig::Sse { url, headers, auth } => {
                    match pool
                        .start_remote_server(&server_name, &url, headers.clone(), auth)
                        .await
                    {
                        Ok(()) => discover_remote_pooled_tools(pool.clone(), &server_name).await,
                        Err(e) => Err(e),
                    }
                }
                McpServerConfig::Http { url, headers, auth } => {
                    match pool
                        .start_remote_server(&server_name, &url, headers.clone(), auth)
                        .await
                    {
                        Ok(()) => discover_remote_pooled_tools(pool.clone(), &server_name).await,
                        Err(e) => Err(e),
                    }
                }
                McpServerConfig::WebSocket { url, auth } => {
                    match pool
                        .start_websocket_server(&server_name, &url, auth.clone())
                        .await
                    {
                        Ok(()) => discover_remote_pooled_tools(pool.clone(), &server_name).await,
                        Err(e) => Err(e),
                    }
                }
            };

            match result {
                Ok(discovered) => {
                    let tool_count = discovered.tools.len();
                    info!(
                        server = %server_name,
                        tools = tool_count,
                        "Non-blocking MCP server connected"
                    );
                    on_tools_ready(discovered.tools);
                }
                Err(e) => {
                    error!(
                        server = %server_name,
                        error = %e,
                        "Non-blocking MCP server discovery failed"
                    );
                }
            }
        };

        tokio::spawn(task);
    }

    PooledMcpDiscoveryResult {
        servers: Vec::new(),
        tools: Vec::new(),
        pool,
    }
}

// ---------------------------------------------------------------------------
// Legacy discovery (one-shot processes)
// ---------------------------------------------------------------------------

/// Discover tools from a remote (HTTP/SSE) server already started in the pool.
///
/// Sends `tools/list` via the pool's persistent connection and returns
/// pooled adapters for each discovered tool.
async fn discover_remote_pooled_tools(
    pool: Arc<McpProcessPool>,
    server_name: &str,
) -> Result<crate::process_pool::PooledDiscoveryResult, String> {
    use crate::process_pool::{PooledDiscoveryResult, PooledMcpToolAdapter};

    // Check capabilities before attempting tools/list.
    if !pool.has_tools(server_name).await {
        tracing::debug!(
            server = %server_name,
            "Remote server does not advertise tools capability; skipping tools/list"
        );
        return Ok(PooledDiscoveryResult {
            server_name: server_name.to_string(),
            tools: Vec::new(),
        });
    }

    // Send tools/list via the pool's send_server_request.
    let response = pool
        .send_server_request(server_name, "tools/list", serde_json::json!({}))
        .await?;

    let mut tools = Vec::new();

    if let Some(tools_array) = response
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
    {
        for tool_value in tools_array {
            let name = tool_value
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();
            let description = tool_value
                .get("description")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("MCP tool: {name}"));
            let input_schema = tool_value
                .get("inputSchema")
                .cloned()
                .unwrap_or(serde_json::json!({"type": "object"}));

            let annotations: Option<crate::ToolAnnotations> = tool_value
                .get("annotations")
                .and_then(|a| serde_json::from_value(a.clone()).ok());

            // Parse per-tool output limit from _meta.maxResultSizeChars.
            let max_output_chars: Option<usize> = tool_value
                .get("_meta")
                .and_then(|m| m.get("maxResultSizeChars"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);

            // Parse per-tool timeout from _meta.timeoutSeconds.
            let tool_timeout_secs: Option<u64> = tool_value
                .get("_meta")
                .and_then(|m| m.get("timeoutSeconds"))
                .and_then(|v| v.as_u64());

            tools.push(PooledMcpToolAdapter::with_output_limit(
                pool.clone(),
                server_name.to_string(),
                name,
                description,
                input_schema,
                annotations,
                max_output_chars,
                tool_timeout_secs,
            ));
        }
    }

    Ok(PooledDiscoveryResult {
        server_name: server_name.to_string(),
        tools,
    })
}

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
        McpServerConfig::Stdio { command, args, env } => {
            let args_owned: Vec<String> = args.clone();
            let env_owned: HashMap<String, String> = env.clone();

            let result = discover_tools(name, command, &args_owned, &env_owned, None).await?;

            Ok(result.tools)
        }
        McpServerConfig::Sse { url, .. } | McpServerConfig::Http { url, .. } => {
            // Remote servers require persistent connections — use pooled discovery instead.
            warn!(
                server = %name,
                url = %url,
                "Remote MCP servers require pooled discovery; skipping in legacy path"
            );
            Ok(Vec::new())
        }
        McpServerConfig::WebSocket { url, .. } => {
            // WebSocket servers require persistent connections — use pooled discovery instead.
            warn!(
                server = %name,
                url = %url,
                "WebSocket MCP servers require pooled discovery; skipping in legacy path"
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
        std::fs::write(
            temp.path().join(".mcp.json"),
            serde_json::to_string(&config).unwrap(),
        )
        .unwrap();

        let result = discover_all_servers(temp.path()).await;
        assert!(result.servers.is_empty());
        assert!(result.tools.is_empty());
    }

    #[tokio::test]
    async fn test_discover_all_servers_with_invalid_config() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join(".mcp.json"), "not valid json {{{").unwrap();

        let result = discover_all_servers(temp.path()).await;
        assert!(result.servers.is_empty());
        assert!(result.tools.is_empty());
    }

    #[tokio::test]
    async fn test_discover_all_servers_pooled_no_config() {
        let temp = tempfile::tempdir().unwrap();
        let result = discover_all_servers_pooled(temp.path()).await;
        assert!(result.servers.is_empty());
        assert!(result.tools.is_empty());
        // Pool should exist even with no servers
        assert!(Arc::strong_count(&result.pool) >= 1);
    }

    #[tokio::test]
    async fn test_discover_all_servers_pooled_empty_config() {
        let temp = tempfile::tempdir().unwrap();
        let config = serde_json::json!({ "mcpServers": {} });
        std::fs::write(
            temp.path().join(".mcp.json"),
            serde_json::to_string(&config).unwrap(),
        )
        .unwrap();

        let result = discover_all_servers_pooled(temp.path()).await;
        assert!(result.servers.is_empty());
        assert!(result.tools.is_empty());
    }

    #[test]
    fn test_discover_nonblocking_no_config() {
        let temp = tempfile::tempdir().unwrap();
        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_clone = called.clone();
        let on_ready = Arc::new(move |_tools: Vec<PooledMcpToolAdapter>| {
            called_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        });

        let result = discover_all_servers_pooled_nonblocking(temp.path(), on_ready);
        assert!(result.servers.is_empty());
        assert!(result.tools.is_empty());
        assert!(!called.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn test_discovery_result_structs() {
        let result = McpDiscoveryResult {
            servers: vec![("test-server".to_string(), 5)],
            tools: Vec::new(),
        };
        assert_eq!(result.servers.len(), 1);
        assert_eq!(result.servers[0].0, "test-server");
        assert_eq!(result.servers[0].1, 5);
    }

    #[test]
    fn test_pooled_discovery_result_struct() {
        let pool = Arc::new(McpProcessPool::new());
        let result = PooledMcpDiscoveryResult {
            servers: vec![("server-a".to_string(), 3), ("server-b".to_string(), 7)],
            tools: Vec::new(),
            pool,
        };
        assert_eq!(result.servers.len(), 2);
        assert_eq!(result.servers[0].1, 3);
        assert_eq!(result.servers[1].1, 7);
    }
}
