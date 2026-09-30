//! MCP manager for Shannon Desktop — bridges desktop config to MCP process pool.
//!
//! G1 runtime hookup (P0-1): the desktop previously never started the pool
//! nor registered any MCP tool into the chat `ToolRegistry`, so every
//! installed extension was dead weight. Two seams close that:
//!
//! - [`seed_pool_from_config`] — startup: spawn the persistent process pool
//!   for every enabled stdio server in the unified
//!   `~/.shannon/settings.json#mcpServers` store. A single server failing
//!   only logs — never fatal.
//! - [`assemble_mcp_tools`] — per chat turn: discover (`tools/list`) the
//!   tools of every connected server and register them into the shared
//!   [`ToolRegistry`] as `mcp__<server>__<tool>` (the same pooled-adapter
//!   pattern the TUI REPL uses). A cold/empty pool changes nothing.

use shannon_core::tools::ToolRegistry;
use shannon_mcp::McpProcessPool;
use shannon_tool_interface::Tool as _;
use std::sync::Arc;
use tracing::{debug, error, info};

/// Seed the MCP process pool from the given desktop server configs.
///
/// Disabled and url-only (no `command`) entries are skipped; stdio entries
/// that fail to start log an error and let the rest proceed. Returns which
/// servers actually started and how many tools they expose.
pub async fn seed_pool_from_config(
    pool: &Arc<McpProcessPool>,
    desktop_servers: Vec<crate::config::McpServerConfig>,
) -> McpInitResult {
    let mut servers_started = Vec::new();
    let mut total_tools = 0;

    for server_config in desktop_servers {
        if !server_config.enabled {
            debug!(server = %server_config.name, "Skipping disabled MCP server");
            continue;
        }
        if server_config.command.is_empty() {
            debug!(
                server = %server_config.name,
                "Skipping MCP server without a stdio command (url-only entry)"
            );
            continue;
        }

        let name = server_config.name.clone();
        info!(server = %name, command = %server_config.command, "Starting MCP server");

        match pool
            .start_server(
                &name,
                &server_config.command,
                &server_config.args,
                &server_config.env,
            )
            .await
        {
            Ok(_) => {
                info!(server = %name, "MCP server started");
                servers_started.push(name.clone());
                // Discover actual tools from the server
                let tools = pool.refresh_tools_for_server(&name).await;
                total_tools += tools.len();
            }
            Err(e) => {
                // Single server failure is never fatal to startup.
                error!(server = %name, error = %e, "Failed to start MCP server");
            }
        }
    }

    McpInitResult {
        servers_started,
        total_tools,
    }
}

/// Discover and register the tools of every connected pool server into the
/// chat [`ToolRegistry`], named `mcp__<server>__<tool>`.
///
/// Safe to call repeatedly (per turn): re-discovered tools whose name is
/// already registered are skipped. Returns the number of tools that were
/// newly registered this call.
pub async fn assemble_mcp_tools(pool: &Arc<McpProcessPool>, registry: &ToolRegistry) -> usize {
    let mut adapters = Vec::new();
    for (name, state) in pool.list_servers().await {
        // Only healthy servers can answer tools/list.
        if !matches!(state, shannon_mcp::ServerState::Healthy) {
            debug!(server = %name, "MCP server not healthy — skipping tool assembly");
            continue;
        }
        let tools = pool.refresh_tools_for_server(&name).await;
        debug!(server = %name, tools = tools.len(), "discovered MCP tools");
        adapters.extend(tools);
    }
    register_pool_adapters(registry, adapters)
}

/// Register pooled MCP tool adapters into the registry, skipping names that
/// are already taken (repeat assembly, bundled collisions). Returns the
/// number of newly registered tools.
fn register_pool_adapters(
    registry: &ToolRegistry,
    adapters: Vec<shannon_mcp::PooledMcpToolAdapter>,
) -> usize {
    let mut registered = 0;
    for tool in adapters {
        let name = tool.name().to_string();
        if let Err(e) = registry.register(Box::new(tool)) {
            debug!(tool = %name, error = %e, "MCP tool registration skipped");
        } else {
            registered += 1;
        }
    }
    registered
}

#[derive(Debug, Clone)]
pub struct McpInitResult {
    pub servers_started: Vec<String>,
    pub total_tools: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use shannon_mcp::config::McpServerConfig as ShannonMcpServerConfig;

    /// An empty (cold) pool must assemble zero tools without error — the
    /// pool-not-ready behavior is identical to the pre-G1 status quo.
    #[tokio::test]
    async fn assemble_with_cold_pool_registers_nothing() {
        let pool = Arc::new(McpProcessPool::new());
        let registry = ToolRegistry::new();
        let registered = assemble_mcp_tools(&pool, &registry).await;
        assert_eq!(registered, 0);
        assert_eq!(registry.list_tools_info().len(), 0);
    }

    /// The registration chain itself: pooled adapters land in the registry
    /// under the `mcp__<server>__<tool>` names the model sees, and a repeat
    /// assembly of the same names is a clean no-op (no error, no dupes).
    #[test]
    fn register_pool_adapters_names_and_dedupes() {
        let pool = Arc::new(McpProcessPool::new());
        let registry = ToolRegistry::new();

        let make = |remote: &str| {
            shannon_mcp::PooledMcpToolAdapter::new(
                pool.clone(),
                "everything".to_string(),
                remote.to_string(),
                format!("test adapter {remote}"),
                serde_json::json!({"type": "object", "properties": {}}),
                None,
            )
        };
        let count = register_pool_adapters(&registry, vec![make("echo"), make("add")]);
        assert_eq!(count, 2);

        let names: Vec<String> = registry
            .list_tools_info()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert!(names.contains(&"mcp__everything__echo".to_string()));
        assert!(names.contains(&"mcp__everything__add".to_string()));

        // Second assembly with the same tool names: skipped, not an error.
        let count = register_pool_adapters(&registry, vec![make("echo")]);
        assert_eq!(count, 0);
        assert_eq!(registry.list_tools_info().len(), 2);
    }

    /// Seed skips disabled servers without attempting to spawn anything.
    #[tokio::test]
    async fn seed_skips_disabled_and_url_only_entries() {
        let pool = Arc::new(McpProcessPool::new());
        let result = seed_pool_from_config(
            &pool,
            vec![
                crate::config::McpServerConfig {
                    name: "off".into(),
                    command: "definitely-not-spawned".into(),
                    args: vec![],
                    env: Default::default(),
                    enabled: false,
                },
                crate::config::McpServerConfig {
                    name: "oauth-remote".into(),
                    command: String::new(),
                    args: vec![],
                    env: Default::default(),
                    enabled: true,
                },
            ],
        )
        .await;
        assert!(result.servers_started.is_empty());
        assert_eq!(result.total_tools, 0);
        assert!(pool.list_servers().await.is_empty());
    }

    /// A failing stdio server only logs — the result reports zero started
    /// and the call returns Ok.
    #[tokio::test]
    async fn seed_single_server_failure_is_not_fatal() {
        let pool = Arc::new(McpProcessPool::new());
        let result = seed_pool_from_config(
            &pool,
            vec![crate::config::McpServerConfig {
                name: "broken".into(),
                command: "/nonexistent/shannon-mcp-test-binary".into(),
                args: vec![],
                env: Default::default(),
                enabled: true,
            }],
        )
        .await;
        assert!(result.servers_started.is_empty());
        assert_eq!(result.total_tools, 0);
    }

    /// The Shannon stdio config shape the seeder consumes (parity with the
    /// old `McpManager::initialize_servers` conversion).
    #[test]
    fn stdio_conversion_shape_matches_pool_config() {
        let desktop = crate::config::McpServerConfig {
            name: "fs".into(),
            command: "npx".into(),
            args: vec![
                "-y".into(),
                "@modelcontextprotocol/server-everything".into(),
            ],
            env: [("KEY".to_string(), "v".to_string())].into_iter().collect(),
            enabled: true,
        };
        let shannon = ShannonMcpServerConfig::Stdio {
            command: desktop.command.clone(),
            args: desktop.args.clone(),
            env: desktop.env.clone(),
        };
        match shannon {
            ShannonMcpServerConfig::Stdio { command, args, env } => {
                assert_eq!(command, "npx");
                assert_eq!(args.len(), 2);
                assert_eq!(env.get("KEY").map(String::as_str), Some("v"));
            }
            _ => panic!("expected Stdio variant"),
        }
    }
}
