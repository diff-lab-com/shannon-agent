//! MCP manager for Shannon Desktop — bridges desktop config to MCP process pool.

use shannon_core::tools::ToolRegistry;
use shannon_mcp::{McpProcessPool, config::McpServerConfig as ShannonMcpServerConfig};
use std::sync::Arc;
use tracing::{error, info};

/// MCP manager that wraps the process pool and handles desktop-specific concerns.
pub struct McpManager {
    pool: Arc<McpProcessPool>,
    // KEEP: working_dir is accepted by ::new() for future per-server CWD
    // control. Not yet plumbed through to McpProcessPool.spawn().
    #[allow(dead_code)]
    working_dir: String,
}

impl McpManager {
    pub fn new(working_dir: String) -> Self {
        let pool = Arc::new(McpProcessPool::new());
        Self { pool, working_dir }
    }

    pub fn pool(&self) -> Arc<McpProcessPool> {
        self.pool.clone()
    }

    pub async fn initialize_servers(
        &self,
        desktop_servers: Vec<crate::config::McpServerConfig>,
        _tool_registry: &mut ToolRegistry,
    ) -> Result<McpInitResult, String> {
        let mut servers_started = Vec::new();
        let mut total_tools = 0;

        for server_config in desktop_servers {
            if !server_config.enabled {
                continue;
            }

            let name = server_config.name.clone();
            let shannon_config = ShannonMcpServerConfig::Stdio {
                command: server_config.command,
                args: server_config.args,
                env: server_config.env,
            };

            match shannon_config {
                ShannonMcpServerConfig::Stdio { command, args, env } => {
                    info!(server = %name, command = %command, "Starting MCP server");

                    match self.pool.start_server(&name, &command, &args, &env).await {
                        Ok(_) => {
                            info!(server = %name, "MCP server started");
                            servers_started.push(name.clone());
                            // Discover actual tools from the server
                            let tools = self.pool.refresh_tools_for_server(&name).await;
                            total_tools += tools.len();
                        }
                        Err(e) => {
                            error!(server = %name, error = %e, "Failed to start MCP server");
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(McpInitResult {
            servers_started,
            total_tools,
        })
    }
}

#[derive(Debug, Clone)]
pub struct McpInitResult {
    pub servers_started: Vec<String>,
    pub total_tools: usize,
}

#[derive(Debug, Clone)]
pub struct McpServerStatus {
    pub name: String,
    pub connected: bool,
    pub tool_count: usize,
    pub last_error: Option<String>,
}
