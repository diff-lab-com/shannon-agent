//! MCP manager for Shannon Desktop — bridges desktop config to MCP process pool.
//!
//! G1 runtime hookup (P0-1): the desktop previously never started the pool
//! nor registered any MCP tool into the chat `ToolRegistry`, so every
//! installed extension was dead weight. Two seams close that:
//!
//! - [`seed_pool_from_config`] — startup: spawn the persistent process pool
//!   for every enabled server in the unified
//!   `~/.shannon/settings.json#mcpServers` store — stdio rows via a local
//!   process, and (W2-A, R4/A1) header-less url-only rows as pure remote
//!   HTTP/SSE servers via `McpProcessPool::start_remote_server`. A single
//!   server failing only logs (and leaves its error on the pool handle for
//!   the UI) — never fatal.
//! - [`assemble_mcp_tools`] — per chat turn: discover (`tools/list`) the
//!   tools of every connected server — stdio and remote alike; the pool is
//!   transport-unified — and register them into the shared [`ToolRegistry`]
//!   as `mcp__<server>__<tool>` (the same pooled-adapter pattern the TUI
//!   REPL uses). A cold/empty pool changes nothing.
//!
//! Auth-bearing url-only entries (`has_auth_headers`, today the OAuth
//! remote installer's `Authorization: Bearer …` product) stay skipped in
//! the W1-A honest state until OAuth support lands (A2).

use shannon_core::tools::ToolRegistry;
use shannon_mcp::McpProcessPool;
use shannon_tool_interface::Tool as _;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info};

/// Seed the MCP process pool from the given desktop server configs.
///
/// Disabled entries are skipped. Url-only entries are classified by the
/// loader's single-source verdict (`McpServerConfig::has_auth_headers`):
/// header-bearing rows stay skipped (auth-gated, A2 scope) while pure
/// remote rows connect via `start_remote_server`.
/// Servers that fail to start log an error and let the rest proceed.
/// Returns which servers actually started and how many tools they expose.
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
            // W2-A (R4/A1): url-only rows are remote HTTP/SSE servers. The
            // auth verdict is the loader's single-source `has_auth_headers`.
            let Some(url) = server_config.url.clone() else {
                debug!(
                    server = %server_config.name,
                    "Skipping MCP row with neither command nor url"
                );
                continue;
            };
            if server_config.has_auth_headers {
                debug!(
                    server = %server_config.name,
                    "Skipping auth-gated remote MCP server (OAuth product; \
                     desktop connect lands with A2)"
                );
                continue;
            }
            info!(server = %server_config.name, url = %url, "Starting remote MCP server");
            match pool
                .start_remote_server(&server_config.name, &url, HashMap::new(), None)
                .await
            {
                Ok(()) => {
                    info!(server = %server_config.name, "Remote MCP server started");
                    servers_started.push(server_config.name.clone());
                    // Discover actual tools from the server (same unified
                    // pool path as stdio).
                    let tools = pool.refresh_tools_for_server(&server_config.name).await;
                    total_tools += tools.len();
                }
                Err(e) => {
                    // Single server failure is never fatal to startup; the
                    // failed handle stays in the pool as Unhealthy(err) so
                    // the UI can show the reason.
                    error!(
                        server = %server_config.name,
                        url = %url,
                        error = %e,
                        "Failed to start remote MCP server"
                    );
                }
            }
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
                // Single server failure is never fatal to startup; the
                // failed handle stays in the pool as Unhealthy(err) so
                // the UI can show the reason.
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
    use shannon_mcp::ServerState;
    use shannon_mcp::config::McpServerConfig as ShannonMcpServerConfig;

    /// W2-A test double: a minimal Streamable-HTTP MCP server speaking just
    /// enough JSON-RPC over a raw TCP listener — `initialize`,
    /// `notifications/initialized`, `tools/list`. Records the methods it
    /// saw and whether any request carried an `Authorization` header, so
    /// the pure-remote wiring proves it connects header-less.
    struct MockRemoteMcp {
        url: String,
        seen: Arc<std::sync::Mutex<Vec<String>>>,
        saw_auth_header: Arc<std::sync::Mutex<bool>>,
    }

    impl MockRemoteMcp {
        async fn start() -> Self {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
            let saw_auth_header = Arc::new(std::sync::Mutex::new(false));

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let seen_task = seen.clone();
            let auth_task = saw_auth_header.clone();

            tokio::spawn(async move {
                loop {
                    let Ok((mut socket, _)) = listener.accept().await else {
                        break;
                    };
                    // One request per connection (we answer with
                    // `Connection: close`); read headers + body, reply.
                    let mut buf = Vec::with_capacity(1024);
                    let mut chunk = [0u8; 1024];
                    let header_end = loop {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => break None,
                            Ok(n) => {
                                buf.extend_from_slice(&chunk[..n]);
                                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                    break Some(pos);
                                }
                            }
                        }
                    };
                    let Some(header_end) = header_end else {
                        continue;
                    };
                    let head = String::from_utf8_lossy(&buf[..header_end]).to_lowercase();
                    let content_length = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    let mut body = buf[header_end + 4..].to_vec();
                    while body.len() < content_length {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => body.extend_from_slice(&chunk[..n]),
                        }
                    }
                    if head.contains("authorization:") {
                        *auth_task.lock().unwrap() = true;
                    }
                    let request: serde_json::Value =
                        serde_json::from_slice(&body).unwrap_or(serde_json::json!({}));
                    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    seen_task.lock().unwrap().push(method.to_string());

                    let result = match method {
                        "initialize" => serde_json::json!({
                            "protocolVersion": "2025-03-26",
                            "capabilities": {"tools": {"listChanged": false}},
                            "serverInfo": {"name": "mock-remote", "version": "0.0.1"}
                        }),
                        "tools/list" => serde_json::json!({
                            "tools": [{
                                "name": "echo",
                                "description": "Echo the input back",
                                "inputSchema": {"type": "object", "properties": {}}
                            }]
                        }),
                        _ => serde_json::json!({}),
                    };
                    let response = if request.get("id").is_some() {
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": request["id"].clone(),
                            "result": result,
                        })
                    } else {
                        // A notification: acknowledge with an empty object.
                        serde_json::json!({})
                    };
                    let payload = serde_json::to_string(&response).unwrap();
                    let http = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        payload.len(),
                        payload
                    );
                    let _ = socket.write_all(http.as_bytes()).await;
                    let _ = socket.flush().await;
                }
            });

            Self {
                url: format!("http://{addr}/mcp"),
                seen,
                saw_auth_header,
            }
        }
    }

    fn cfg(
        name: &str,
        command: &str,
        url: Option<String>,
        has_auth_headers: bool,
    ) -> crate::config::McpServerConfig {
        crate::config::McpServerConfig {
            name: name.into(),
            command: command.into(),
            args: vec![],
            env: Default::default(),
            enabled: true,
            url,
            has_auth_headers,
        }
    }

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

    /// Seed skips disabled servers without attempting to spawn anything,
    /// and rows with neither command nor url.
    #[tokio::test]
    async fn seed_skips_disabled_and_shapeless_entries() {
        let pool = Arc::new(McpProcessPool::new());
        let mut off = cfg("off", "definitely-not-spawned", None, false);
        off.enabled = false;
        let result = seed_pool_from_config(&pool, vec![off, cfg("void", "", None, false)]).await;
        assert!(result.servers_started.is_empty());
        assert_eq!(result.total_tools, 0);
        assert!(pool.list_servers().await.is_empty());
    }

    /// W2-A core acceptance (R4/A1): a pure remote url-only entry seeds →
    /// connects (Healthy) → its tools register into the chat registry under
    /// `mcp__<server>__<tool>` — the same unified pool path as stdio. The
    /// mock also proves the connection carries no `Authorization` header.
    #[tokio::test]
    async fn seed_pure_remote_connects_and_registers_tools() {
        let mock = MockRemoteMcp::start().await;
        let pool = Arc::new(McpProcessPool::new());
        let result = seed_pool_from_config(
            &pool,
            vec![cfg("mock-remote", "", Some(mock.url.clone()), false)],
        )
        .await;

        assert_eq!(result.servers_started, vec!["mock-remote".to_string()]);
        assert_eq!(result.total_tools, 1, "the mock exposes exactly one tool");

        let state = pool
            .list_servers()
            .await
            .into_iter()
            .find(|(name, _)| name == "mock-remote")
            .map(|(_, state)| state);
        assert!(
            matches!(state, Some(ServerState::Healthy)),
            "remote server must be Healthy after seed, got {state:?}"
        );

        // Chat assembly (commands.rs per-turn path) picks remote tools up
        // through the same unified pool.
        let registry = ToolRegistry::new();
        let registered = assemble_mcp_tools(&pool, &registry).await;
        assert_eq!(registered, 1);
        assert!(registry.get("mcp__mock-remote__echo").is_some());

        // Pure remote = header-less: the mock never saw an Authorization.
        assert!(
            !*mock.saw_auth_header.lock().unwrap(),
            "pure remote connection must not send credentials"
        );
        assert!(
            mock.seen
                .lock()
                .unwrap()
                .contains(&"initialize".to_string())
        );
        assert!(
            mock.seen
                .lock()
                .unwrap()
                .contains(&"tools/list".to_string())
        );
    }

    /// W2-A honesty (A2 scope): an OAuth-product url-only entry (headers on
    /// the store blob → `has_auth_headers`) is still skipped at seed time —
    /// never connected header-less.
    #[tokio::test]
    async fn seed_skips_auth_gated_remote_entries() {
        let pool = Arc::new(McpProcessPool::new());
        let result = seed_pool_from_config(
            &pool,
            vec![cfg(
                "oauth-remote",
                "",
                Some("https://mcp.example/mcp".into()),
                true,
            )],
        )
        .await;
        assert!(result.servers_started.is_empty());
        assert_eq!(result.total_tools, 0);
        assert!(
            pool.list_servers().await.is_empty(),
            "auth-gated entries must not reach the pool"
        );
    }

    /// A failing stdio server only logs — the result reports zero started
    /// and the call returns Ok. W2-A: the failed handle stays in the pool
    /// as Unhealthy(reason) so `list_mcp_servers` can render `last_error`.
    #[tokio::test]
    async fn seed_single_server_failure_is_not_fatal() {
        let pool = Arc::new(McpProcessPool::new());
        let result = seed_pool_from_config(
            &pool,
            vec![cfg(
                "broken",
                "/nonexistent/shannon-mcp-test-binary",
                None,
                false,
            )],
        )
        .await;
        assert!(result.servers_started.is_empty());
        assert_eq!(result.total_tools, 0);

        let state = pool
            .list_servers()
            .await
            .into_iter()
            .find(|(name, _)| name == "broken")
            .map(|(_, state)| state);
        assert!(
            matches!(&state, Some(ServerState::Unhealthy(msg)) if msg.contains("failed to spawn")),
            "failed stdio start must be observable as Unhealthy, got {state:?}"
        );
    }

    /// W2-A: a remote connection failure is equally observable — the seeder
    /// keeps going (not fatal) and the pool reports Unhealthy with the
    /// reason, which `list_mcp_servers` maps to `last_error`.
    #[tokio::test]
    async fn seed_remote_failure_surfaces_last_error() {
        let mut pool = McpProcessPool::new();
        pool.set_request_timeout(std::time::Duration::from_millis(200));
        pool.set_connection_timeout(std::time::Duration::from_millis(200));
        let pool = Arc::new(pool);

        let result = seed_pool_from_config(
            &pool,
            vec![cfg(
                "dead-remote",
                "",
                Some("http://127.0.0.1:1/mcp".into()),
                false,
            )],
        )
        .await;
        assert!(result.servers_started.is_empty());
        assert_eq!(result.total_tools, 0);

        let state = pool
            .list_servers()
            .await
            .into_iter()
            .find(|(name, _)| name == "dead-remote")
            .map(|(_, state)| state);
        assert!(
            matches!(&state, Some(ServerState::Unhealthy(msg)) if !msg.is_empty()),
            "failed remote start must surface a reason, got {state:?}"
        );
    }

    /// The Shannon stdio config shape the seeder consumes (parity with the
    /// old `McpManager::initialize_servers` conversion).
    #[test]
    fn stdio_conversion_shape_matches_pool_config() {
        let desktop = cfg("fs", "npx", None, false);
        let shannon = ShannonMcpServerConfig::Stdio {
            command: desktop.command.clone(),
            args: desktop.args.clone(),
            env: desktop.env.clone(),
        };
        match shannon {
            ShannonMcpServerConfig::Stdio { command, args, env } => {
                assert_eq!(command, "npx");
                assert!(args.is_empty());
                assert!(env.is_empty());
            }
            _ => panic!("expected Stdio variant"),
        }
    }
}
