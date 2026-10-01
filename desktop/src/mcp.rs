//! MCP manager for Shannon Desktop — bridges desktop config to MCP process pool.
//!
//! G1 runtime hookup (P0-1): the desktop previously never started the pool
//! nor registered any MCP tool into the chat `ToolRegistry`, so every
//! installed extension was dead weight. Two seams close that:
//!
//! - [`seed_pool_from_config`] — startup: spawn the persistent process pool
//!   for every enabled server in the unified
//!   `~/.shannon/settings.json#mcpServers` store — stdio rows via a local
//!   process, and (W2-A, R4/A1) url-only rows as remote HTTP/SSE servers
//!   via `McpProcessPool::start_remote_server`. A single server failing
//!   only logs (and leaves its error on the pool handle for the UI) —
//!   never fatal.
//! - [`assemble_mcp_tools`] — per chat turn: discover (`tools/list`) the
//!   tools of every connected server — stdio and remote alike; the pool is
//!   transport-unified — and register them into the shared [`ToolRegistry`]
//!   as `mcp__<server>__<tool>` (the same pooled-adapter pattern the TUI
//!   REPL uses). A cold/empty pool changes nothing.
//!
//! W3-B (A2, R4): OAuth url-only entries (`has_auth_headers`) connect for
//! real via the stored-credential path ([`start_oauth_remote_row`]) —
//! the W2-A honest skip is gone. A 401 during the handshake triggers the
//! provider's refresh-once-retry; a rotated token is written back into the
//! entry's `shannonOAuth` block (ruling R6: inside the existing settings
//! blob, no keychain). A refresh that still fails lands the row in the
//! "needs re-authentication" state (classified, never a generic Offline).

use shannon_core::tools::ToolRegistry;
use shannon_mcp::McpProcessPool;
use shannon_tool_interface::Tool as _;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Seed the MCP process pool from the given desktop server configs against
/// the real user `settings.json` (refreshed tokens persist there).
pub async fn seed_pool_from_config(
    pool: &Arc<McpProcessPool>,
    desktop_servers: Vec<crate::config::McpServerConfig>,
) -> McpInitResult {
    seed_pool_from_config_in(pool, desktop_servers, &crate::config::user_settings_path()).await
}

/// `seed_pool_from_config` against an explicit `settings.json` path (tests
/// inject a tempdir so they never touch the user's HOME).
///
/// Disabled entries are skipped. Url-only entries are classified by the
/// loader's single-source verdict (`McpServerConfig::has_auth_headers`,
/// since A2 an "OAuth entry"): OAuth rows connect via
/// [`start_oauth_remote_row_in`], pure remote rows via `start_remote_server`.
/// Servers that fail to start log an error and let the rest proceed.
pub async fn seed_pool_from_config_in(
    pool: &Arc<McpProcessPool>,
    desktop_servers: Vec<crate::config::McpServerConfig>,
    settings_path: &Path,
) -> McpInitResult {
    let mut servers_started = Vec::new();
    let mut total_tools = 0;
    let mut needs_auth_servers = Vec::new();

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
                // W3-B (A2): connect with the stored credential. A row with
                // no credential at all stays honestly unconnected (never
                // started anonymously).
                let outcome = start_oauth_remote_row_in(pool, &server_config, settings_path).await;
                match outcome.error {
                    None if outcome.connected => {
                        info!(
                            server = %server_config.name,
                            url = %url,
                            refreshed = outcome.refreshed.is_some(),
                            "OAuth remote MCP server started"
                        );
                        servers_started.push(server_config.name.clone());
                        let tools = pool.refresh_tools_for_server(&server_config.name).await;
                        total_tools += tools.len();
                    }
                    None => {
                        debug!(
                            server = %server_config.name,
                            "OAuth entry without stored credential — staying unconnected"
                        );
                    }
                    Some(e) => {
                        let needs_auth = shannon_mcp::classify_remote_failure(&e)
                            == shannon_mcp::RemoteFailureKind::NeedsAuth;
                        if needs_auth {
                            needs_auth_servers.push(server_config.name.clone());
                        }
                        // Single server failure is never fatal to startup;
                        // the failed handle stays in the pool as
                        // Unhealthy(err) so the UI can classify the reason.
                        error!(
                            server = %server_config.name,
                            url = %url,
                            error = %e,
                            needs_auth,
                            "Failed to start OAuth remote MCP server"
                        );
                    }
                }
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
                // `list_mcp_servers` can render `last_error`.
                error!(server = %name, error = %e, "Failed to start MCP server");
            }
        }
    }

    McpInitResult {
        servers_started,
        total_tools,
        needs_auth_servers,
    }
}

/// Result of one OAuth remote row start (A2 token lifecycle).
#[derive(Debug, Clone)]
pub struct OAuthStartOutcome {
    pub connected: bool,
    /// Tokens held by the provider after the connect — `Some` only when a
    /// refresh rotated the credential during the handshake, in which case
    /// the caller persists the diff (already done by
    /// [`start_oauth_remote_row_in`] itself; the field informs the result).
    pub refreshed: Option<shannon_mcp::OAuthTokenSnapshot>,
    /// Connection failure. Classify with
    /// `shannon_mcp::classify_remote_failure` for the UI state.
    /// `None` also covers the honest "no stored credential" skip
    /// (`connected == false`, pool untouched).
    pub error: Option<String>,
}

/// Start one OAuth remote row against the real user `settings.json`
/// (refreshed tokens persist there).
pub async fn start_oauth_remote_row(
    pool: &McpProcessPool,
    server: &crate::config::McpServerConfig,
) -> OAuthStartOutcome {
    start_oauth_remote_row_in(pool, server, &crate::config::user_settings_path()).await
}

/// Connect one OAuth entry with its stored credential.
///
/// Seeds the pool's OAuth provider from the entry's `shannonOAuth` block
/// (or the legacy in-memory block derived from the `Authorization` header)
/// and starts the remote server. On success the provider's token snapshot
/// is diffed against what was stored: a handshake-time refresh is written
/// back into the entry (ruling R6: `settings.json#mcpServers` blob, atomic
/// write) so the next start reconnects without another refresh round-trip.
pub async fn start_oauth_remote_row_in(
    pool: &McpProcessPool,
    server: &crate::config::McpServerConfig,
    settings_path: &Path,
) -> OAuthStartOutcome {
    let Some(url) = server.url.clone() else {
        return OAuthStartOutcome {
            connected: false,
            refreshed: None,
            error: Some(format!("Server '{}' has no remote url", server.name)),
        };
    };
    let Some(oauth) = server.oauth.clone().filter(|o| o.has_credential()) else {
        // Honest skip: no credential to connect with — never anonymous.
        return OAuthStartOutcome {
            connected: false,
            refreshed: None,
            error: None,
        };
    };
    let previous_access = oauth.access_token.clone().unwrap_or_default();
    let previous_refresh = oauth.refresh_token.clone();

    let creds = shannon_mcp::StoredOAuthCredentials {
        client_id: oauth.client_id.clone(),
        client_secret: None,
        token_url: oauth.token_url.clone(),
        access_token: oauth.access_token.clone().unwrap_or_default(),
        refresh_token: oauth.refresh_token.clone(),
        expires_at: oauth
            .expires_at
            .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0)),
        scopes: Vec::new(),
    };

    if let Err(e) = pool
        .start_remote_oauth_server(&server.name, &url, creds)
        .await
    {
        return OAuthStartOutcome {
            connected: false,
            refreshed: None,
            error: Some(e),
        };
    }

    // A 401 mid-handshake rotated the token (refresh-once-retry inside the
    // handle) — persist the new pair so the rotation survives restart.
    let snapshot = pool.remote_oauth_tokens(&server.name).await;
    let refreshed = snapshot
        .filter(|s| s.access_token != previous_access || s.refresh_token != previous_refresh);
    if let Some(snap) = &refreshed {
        let stored = crate::config::McpStoredOAuth {
            client_id: oauth.client_id.clone(),
            token_url: oauth.token_url.clone(),
            refresh_token: snap.refresh_token.clone(),
            access_token: Some(snap.access_token.clone()),
            expires_at: snap.expires_at.map(|e| e.timestamp()),
        };
        match crate::config::update_mcp_server_oauth_tokens_to(settings_path, &server.name, &stored)
        {
            Ok(true) => info!(server = %server.name, "Persisted refreshed OAuth tokens"),
            Ok(false) => {
                warn!(server = %server.name, "Refreshed OAuth tokens not persisted: entry vanished")
            }
            Err(e) => warn!(
                server = %server.name,
                error = %e,
                "Persisting refreshed OAuth tokens failed"
            ),
        }
    }

    OAuthStartOutcome {
        connected: true,
        refreshed,
        error: None,
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
    /// W3-B (A2): OAuth rows whose stored credential was rejected even
    /// after a refresh attempt — the "needs re-authentication" state. The
    /// startup path fires one desktop notification for these.
    pub needs_auth_servers: Vec<String>,
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
    /// the pure-remote wiring proves it connects header-less. W3-B: with
    /// `required_bearer` set, only requests carrying exactly that
    /// credential pass — anything else gets HTTP 401, proving the OAuth
    /// path sends (and refreshes) the real token.
    struct MockRemoteMcp {
        url: String,
        seen: Arc<std::sync::Mutex<Vec<String>>>,
        saw_auth_header: Arc<std::sync::Mutex<bool>>,
    }

    impl MockRemoteMcp {
        async fn start() -> Self {
            Self::start_with_auth(None).await
        }

        /// Start a mock that enforces `required_bearer` (e.g.
        /// `"Bearer fresh-token"`); requests with any other credential or
        /// none receive HTTP 401.
        async fn start_with_auth(required_bearer: Option<&str>) -> Self {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
            let saw_auth_header = Arc::new(std::sync::Mutex::new(false));

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let seen_task = seen.clone();
            let auth_task = saw_auth_header.clone();
            let required = required_bearer.map(str::to_string);

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
                    let raw_head = String::from_utf8_lossy(&buf[..header_end]).to_string();
                    let bearer = raw_head.lines().find_map(|l| {
                        let (name, value) = l.split_once(':')?;
                        // Header names are case-insensitive on the wire
                        // (hyper lowercases them in HTTP/1.1).
                        if !name.eq_ignore_ascii_case("authorization") {
                            return None;
                        }
                        Some(value.trim().to_string())
                    });
                    if bearer.is_some() {
                        *auth_task.lock().unwrap() = true;
                    }
                    // Auth gate: when required, reject any other credential.
                    if let Some(required) = &required {
                        if bearer.as_deref() != Some(required.as_str()) {
                            let payload = "{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32000,\"message\":\"unauthorized\"}}";
                            let http = format!(
                                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\n\
                                 Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                                payload.len(),
                                payload
                            );
                            let _ = socket.write_all(http.as_bytes()).await;
                            let _ = socket.flush().await;
                            continue;
                        }
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

    /// Minimal OAuth token endpoint answering every refresh POST with a
    /// fixed JSON payload. `status_400` makes every grant fail (the
    /// refresh-exhausted path).
    async fn spawn_token_endpoint(payload: &'static str, status_400: bool) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = Vec::with_capacity(512);
                let mut chunk = [0u8; 512];
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
                if header_end.is_none() {
                    continue;
                }
                let status = if status_400 {
                    "400 Bad Request"
                } else {
                    "200 OK"
                };
                let http = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    payload.len(),
                    payload
                );
                let _ = socket.write_all(http.as_bytes()).await;
                let _ = socket.flush().await;
            }
        });
        format!("http://{addr}/token")
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
            oauth: None,
        }
    }

    /// Write a `settings.json` containing one OAuth url-only entry with the
    /// given `shannonOAuth` block, then load it back through the real
    /// loader — the same parse path production uses.
    fn oauth_row_from_store(
        tmp: &tempfile::TempDir,
        name: &str,
        url: &str,
        oauth: Option<serde_json::Value>,
        bearer: Option<&str>,
    ) -> crate::config::McpServerConfig {
        let mut entry = serde_json::json!({
            "type": "http",
            "url": url,
            "enabled": true,
        });
        if let Some(bearer) = bearer {
            entry["headers"] = serde_json::json!({"Authorization": bearer});
        }
        if let Some(oauth) = oauth {
            entry["shannonOAuth"] = oauth;
        }
        let path = tmp.path().join("settings.json");
        std::fs::write(
            &path,
            serde_json::json!({"mcpServers": {name: entry}}).to_string(),
        )
        .unwrap();
        crate::config::load_mcp_servers_from(&path)
            .unwrap()
            .into_iter()
            .find(|s| s.name == name)
            .unwrap()
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

    /// W3-B (A2): an OAuth url-only entry with **no** stored credential at
    /// all (e.g. a hand-written `headers: {"X-Key": ""}` blob) stays
    /// honestly unconnected — it is never started anonymously.
    #[tokio::test]
    async fn seed_skips_credential_less_auth_entries() {
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
            "credential-less entries must not reach the pool"
        );
        assert!(result.needs_auth_servers.is_empty());
    }

    /// W3-B core acceptance ①: an OAuth entry connects with its stored
    /// token — the mock only answers requests carrying exactly that
    /// Authorization header, so a Healthy result proves the credential was
    /// sent — and its tools register into the chat registry.
    #[tokio::test]
    async fn seed_oauth_entry_connects_with_stored_token() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockRemoteMcp::start_with_auth(Some("Bearer stored-token")).await;
        let row = oauth_row_from_store(
            &tmp,
            "notion-oauth",
            &mock.url,
            Some(serde_json::json!({
                "client_id": "shannon-desktop",
                "token_url": "",
                "access_token": "stored-token",
            })),
            Some("Bearer stored-token"),
        );
        let pool = Arc::new(McpProcessPool::new());
        let result =
            seed_pool_from_config_in(&pool, vec![row], &tmp.path().join("settings.json")).await;

        assert_eq!(result.servers_started, vec!["notion-oauth".to_string()]);
        assert_eq!(result.total_tools, 1);
        assert!(result.needs_auth_servers.is_empty());
        assert!(*mock.saw_auth_header.lock().unwrap());

        let registry = ToolRegistry::new();
        assert_eq!(assemble_mcp_tools(&pool, &registry).await, 1);
        assert!(registry.get("mcp__notion-oauth__echo").is_some());
    }

    /// W3-B core acceptance ②: 401 → refresh → retry succeeds, and the
    /// rotated access + refresh tokens are written back into the
    /// settings.json entry (R6: same blob, atomic write) — both the
    /// `shannonOAuth` block and the `Authorization` header.
    #[tokio::test]
    async fn seed_oauth_refresh_rotates_and_persists_tokens() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        let mock = MockRemoteMcp::start_with_auth(Some("Bearer fresh-token")).await;
        let token_url = spawn_token_endpoint(
            r#"{"access_token":"fresh-token","refresh_token":"rotated-refresh","expires_in":3600,"token_type":"Bearer"}"#,
            false,
        )
        .await;
        let row = oauth_row_from_store(
            &tmp,
            "linear-oauth",
            &mock.url,
            Some(serde_json::json!({
                "client_id": "shannon-desktop",
                "token_url": token_url,
                "refresh_token": "stale-refresh",
                "access_token": "stale-token",
            })),
            Some("Bearer stale-token"),
        );
        let pool = Arc::new(McpProcessPool::new());
        let result = seed_pool_from_config_in(&pool, vec![row], &settings_path).await;

        assert_eq!(result.servers_started, vec!["linear-oauth".to_string()]);
        assert_eq!(result.total_tools, 1);
        assert!(result.needs_auth_servers.is_empty());

        // The rotated pair landed in the store.
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let entry = &root["mcpServers"]["linear-oauth"];
        assert_eq!(entry["shannonOAuth"]["access_token"], "fresh-token");
        assert_eq!(entry["shannonOAuth"]["refresh_token"], "rotated-refresh");
        assert!(entry["shannonOAuth"]["expires_at"].as_i64().is_some());
        assert_eq!(entry["headers"]["Authorization"], "Bearer fresh-token");

        // The mock only accepts the refreshed credential, so Healthy proves
        // the refresh + retry round-trip happened.
        let state = pool
            .list_servers()
            .await
            .into_iter()
            .find(|(name, _)| name == "linear-oauth")
            .map(|(_, state)| state);
        assert!(matches!(state, Some(ServerState::Healthy)), "{state:?}");
    }

    /// W3-B core acceptance ③: refresh failure (token endpoint refuses the
    /// grant, server keeps 401ing) lands the "needs re-authentication"
    /// state — classified NeedsAuth, reported in `needs_auth_servers`,
    /// never connected — and the stored tokens stay untouched.
    #[tokio::test]
    async fn seed_oauth_refresh_failure_reports_needs_auth() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        let mock = MockRemoteMcp::start_with_auth(Some("Bearer never-issued")).await;
        let token_url = spawn_token_endpoint(r#"{"error":"invalid_grant"}"#, true).await;
        let row = oauth_row_from_store(
            &tmp,
            "slack-oauth",
            &mock.url,
            Some(serde_json::json!({
                "client_id": "shannon-desktop",
                "token_url": token_url,
                "refresh_token": "dead-refresh",
                "access_token": "expired-token",
            })),
            Some("Bearer expired-token"),
        );
        let pool = Arc::new(McpProcessPool::new());
        let result = seed_pool_from_config_in(&pool, vec![row], &settings_path).await;

        assert!(result.servers_started.is_empty());
        assert_eq!(result.needs_auth_servers, vec!["slack-oauth".to_string()]);

        // The pool handle carries the reason, classified NeedsAuth.
        let state = pool
            .list_servers()
            .await
            .into_iter()
            .find(|(name, _)| name == "slack-oauth")
            .map(|(_, state)| state);
        let Some(ServerState::Unhealthy(msg)) = state else {
            panic!("expected Unhealthy handle, got {state:?}");
        };
        assert_eq!(
            shannon_mcp::classify_remote_failure(&msg),
            shannon_mcp::RemoteFailureKind::NeedsAuth,
            "401 failure must classify as needs_auth: {msg}"
        );

        // The store still holds the old tokens (no bogus rotation).
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let entry = &root["mcpServers"]["slack-oauth"];
        assert_eq!(entry["shannonOAuth"]["access_token"], "expired-token");
        assert_eq!(entry["shannonOAuth"]["refresh_token"], "dead-refresh");
    }

    /// W3-B: an OAuth entry whose failure is a transport problem
    /// (connection refused) classifies Unreachable — retryable, not a
    /// re-auth case.
    #[tokio::test]
    async fn seed_oauth_unreachable_classifies_unreachable() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        let row = oauth_row_from_store(
            &tmp,
            "ghost-oauth",
            "http://127.0.0.1:1/mcp",
            Some(serde_json::json!({
                "client_id": "shannon-desktop",
                "token_url": "",
                "access_token": "some-token",
            })),
            Some("Bearer some-token"),
        );
        let mut pool = McpProcessPool::new();
        pool.set_connection_timeout(std::time::Duration::from_millis(200));
        pool.set_request_timeout(std::time::Duration::from_millis(200));
        let pool = Arc::new(pool);
        let result = seed_pool_from_config_in(&pool, vec![row], &settings_path).await;

        assert!(result.servers_started.is_empty());
        assert!(result.needs_auth_servers.is_empty());
        let state = pool
            .list_servers()
            .await
            .into_iter()
            .find(|(name, _)| name == "ghost-oauth")
            .map(|(_, state)| state);
        let Some(ServerState::Unhealthy(msg)) = state else {
            panic!("expected Unhealthy handle, got {state:?}");
        };
        assert_eq!(
            shannon_mcp::classify_remote_failure(&msg),
            shannon_mcp::RemoteFailureKind::Unreachable,
            "{msg}"
        );
    }

    /// W3-B: a legacy pre-A2 entry (header bearer only, no `shannonOAuth`)
    /// still connects — the loader derives the in-memory token — and a
    /// successful connect without a refresh leaves the store untouched.
    #[tokio::test]
    async fn seed_legacy_header_entry_connects_without_rewriting_store() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        let mock = MockRemoteMcp::start_with_auth(Some("Bearer legacy-token")).await;
        let row = oauth_row_from_store(
            &tmp,
            "legacy-oauth",
            &mock.url,
            None,
            Some("Bearer legacy-token"),
        );
        assert!(row.oauth.is_some(), "loader must derive the legacy token");
        assert!(!row.oauth.as_ref().unwrap().can_refresh());

        let pool = Arc::new(McpProcessPool::new());
        let result = seed_pool_from_config_in(&pool, vec![row], &settings_path).await;
        assert_eq!(result.servers_started, vec!["legacy-oauth".to_string()]);

        let before = std::fs::read_to_string(&settings_path).unwrap();
        let root: serde_json::Value = serde_json::from_str(&before).unwrap();
        assert!(root["mcpServers"]["legacy-oauth"]["shannonOAuth"].is_null());
        assert_eq!(
            root["mcpServers"]["legacy-oauth"]["headers"]["Authorization"],
            "Bearer legacy-token"
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
