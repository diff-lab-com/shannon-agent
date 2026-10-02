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
//!
//! F6 (R7-④ batch 2): rotations are no longer persisted only at connect —
//! the pool's token-rotation callback ([`install_token_rotation_hook`])
//! persists every 401-triggered refresh (tool calls included) into the
//! keyring via the F5 keyring-first write path. Both writers (connect-time
//! diff, rotation callback) share one the private `OAuthTokenPersister`: same-value
//! snapshots dedup to a single write, and the shared lock serializes them so
//! a stale value can never overwrite a fresh rotation.

use shannon_core::tools::ToolRegistry;
use shannon_mcp::McpProcessPool;
use shannon_tool_interface::Tool as _;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Seed the MCP process pool from the given desktop server configs against
/// the real user `settings.json` (refreshed tokens persist there or in the
/// OS keyring, per the startup-secret-store resolution).
pub async fn seed_pool_from_config(
    pool: &Arc<McpProcessPool>,
    desktop_servers: Vec<crate::config::McpServerConfig>,
) -> McpInitResult {
    seed_pool_from_config_in(
        pool,
        desktop_servers,
        &crate::config::user_settings_path(),
        crate::secret_store::global().as_deref(),
    )
    .await
}

/// `seed_pool_from_config` against an explicit `settings.json` path and an
/// injected secret store (tests pass a
/// [`MockSecretStore`](crate::secret_store::MockSecretStore) or `None` —
/// never the real keyring).
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
    store: Option<&dyn crate::secret_store::SecretStore>,
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
                let outcome =
                    start_oauth_remote_row_in(pool, &server_config, settings_path, store).await;
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
/// (refreshed tokens persist there or in the OS keyring).
pub async fn start_oauth_remote_row(
    pool: &McpProcessPool,
    server: &crate::config::McpServerConfig,
) -> OAuthStartOutcome {
    start_oauth_remote_row_in(
        pool,
        server,
        &crate::config::user_settings_path(),
        crate::secret_store::global().as_deref(),
    )
    .await
}

/// Connect one OAuth entry with its stored credential.
///
/// Seeds the pool's OAuth provider from the entry's `shannonOAuth` block
/// (or the legacy in-memory block derived from the `Authorization` header)
/// and starts the remote server. On success the provider's token snapshot
/// is diffed against what was stored: a handshake-time refresh is persisted
/// through [`crate::config::update_mcp_server_oauth_tokens_with_store`] —
/// into the keyring (`shannon/mcp-oauth/<name>`) when `store` is `Some`,
/// else into the settings.json entry (R6 0600 atomic write) — so the next
/// start reconnects without another refresh round-trip.
pub async fn start_oauth_remote_row_in(
    pool: &McpProcessPool,
    server: &crate::config::McpServerConfig,
    settings_path: &Path,
    store: Option<&dyn crate::secret_store::SecretStore>,
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
        // F6: through the shared persister — the rotation callback (tool-call
        // path) may already have written exactly this snapshot while the
        // handshake was in flight; the shared lock + same-value dedup turn
        // the double write into one keyring/0600-file write.
        match oauth_token_persister().persist(settings_path, &server.name, &stored, store) {
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

// ---------------------------------------------------------------------------
// F6 — tool-call-time token rotation persistence
// ---------------------------------------------------------------------------

/// Serializes and de-duplicates OAuth token-block persistence across the two
/// writers that target the same keyring entry / `settings.json` row (F6):
/// the connection-time diff write (W3-B, [`start_oauth_remote_row_in`]) and
/// the tool-call-time rotation callback ([`install_token_rotation_hook`]).
///
/// One `Mutex` is held across the dedup check **and** the underlying
/// [`crate::config::update_mcp_server_oauth_tokens_with_store`] write, so
/// the writers can never interleave — and a late writer holding an older
/// snapshot can never overwrite a freshly rotated one, because equal
/// snapshots dedup to a no-op and a genuinely newer rotation always wins
/// the lock order it observed.
#[derive(Default)]
pub(crate) struct OAuthTokenPersister {
    /// Per-server token block last successfully persisted (dedup memory).
    /// The mutex is held across the dedup check and the write itself.
    last_written: std::sync::Mutex<HashMap<String, crate::config::McpStoredOAuth>>,
}

impl OAuthTokenPersister {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Persist `stored` unless it is byte-identical (struct equality) to what
    /// this persister already wrote for `server` (F6 debounce: a rotation
    /// observed twice — callback + connect-time diff — is one write).
    ///
    /// `Ok(false)` (entry vanished) and `Err` leave the dedup memory
    /// untouched, so the next rotation (or retry) attempts the write again —
    /// the in-memory provider handle keeps the new token either way.
    pub(crate) fn persist(
        &self,
        settings_path: &Path,
        server: &str,
        stored: &crate::config::McpStoredOAuth,
        store: Option<&dyn crate::secret_store::SecretStore>,
    ) -> Result<bool, String> {
        let mut last = self.last_written.lock().expect("oauth persister lock");
        if last.get(server).is_some_and(|prev| prev == stored) {
            debug!(server = %server, "OAuth tokens unchanged since last persist — skipping write");
            return Ok(true);
        }
        let outcome = crate::config::update_mcp_server_oauth_tokens_with_store(
            settings_path,
            server,
            stored,
            store,
        );
        if matches!(&outcome, Ok(true)) {
            last.insert(server.to_string(), stored.clone());
        }
        outcome
    }
}

/// The process-wide persister both writers share. A `OnceLock` (not AppState
/// state) because the rotation callback is installed on the pool whose
/// handle outlives any single Tauri command scope — and because the
/// connection-time writer must reach the *same* instance the hook captured.
static OAUTH_TOKEN_PERSISTER: std::sync::OnceLock<std::sync::Arc<OAuthTokenPersister>> =
    std::sync::OnceLock::new();

pub(crate) fn oauth_token_persister() -> std::sync::Arc<OAuthTokenPersister> {
    OAUTH_TOKEN_PERSISTER
        .get_or_init(|| std::sync::Arc::new(OAuthTokenPersister::new()))
        .clone()
}

/// Subscribe the pool's token-rotation events (F6) against the real user
/// `settings.json` and the process-global secret store. Installed once at
/// startup, before the pool is seeded, so handshake-time rotations are
/// persisted through the same path too.
pub async fn install_token_rotation_hook(pool: &McpProcessPool) {
    install_token_rotation_hook_in(
        pool,
        crate::config::user_settings_path(),
        crate::secret_store::global(),
    )
    .await;
}

/// `install_token_rotation_hook` against an explicit `settings.json` path and
/// an injected secret store (tests pass a
/// [`MockSecretStore`](crate::secret_store::MockSecretStore) — never the real
/// keyring). The shared the private `OAuthTokenPersister` is reachable through
/// [`oauth_token_persister`].
///
/// The callback resolves the credential **origin** (client id, token
/// endpoint) through the F5 keyring-first loader, overlays the rotated
/// snapshot, and writes via the F5 keyring-first write path — keyring entry
/// updated, plaintext stripped from the file; without a store the legacy
/// 0600 in-file shape is written instead. There are no separate non-secret
/// file fields to sync: `expires_at` lives inside the credential block, and
/// no UI surface displays it.
pub(crate) async fn install_token_rotation_hook_in(
    pool: &McpProcessPool,
    settings_path: std::path::PathBuf,
    store: Option<std::sync::Arc<dyn crate::secret_store::SecretStore>>,
) -> std::sync::Arc<OAuthTokenPersister> {
    let persister = oauth_token_persister();
    let hook: shannon_mcp::TokenUpdateCallback = {
        let persister = persister.clone();
        std::sync::Arc::new(
            move |server: &str, snapshot: &shannon_mcp::OAuthTokenSnapshot| {
                persist_rotated_snapshot(
                    &persister,
                    &settings_path,
                    store.as_deref(),
                    server,
                    snapshot,
                );
            },
        )
    };
    pool.set_on_token_refresh(hook).await;
    info!("OAuth token-rotation persistence hook installed");
    persister
}

/// The rotation-callback body: overlay `snapshot` onto the stored credential
/// origin and persist. Every failure path only warns — the provider keeps
/// the new token in memory, so the next rotation retries the write; nothing
/// is lost by deferring.
fn persist_rotated_snapshot(
    persister: &OAuthTokenPersister,
    settings_path: &Path,
    store: Option<&dyn crate::secret_store::SecretStore>,
    server: &str,
    snapshot: &shannon_mcp::OAuthTokenSnapshot,
) {
    // The snapshot carries only the rotated secrets; the stable origin
    // fields (client id, token endpoint) come from the F5 keyring-first
    // loader — never guessed, never written as an empty-client block that
    // could not refresh next session.
    let rows = match crate::config::load_mcp_servers_with_store(settings_path, store) {
        Ok(rows) => rows,
        Err(e) => {
            warn!(
                server = %server,
                error = %e,
                "OAuth token rotation not persisted: settings.json failed to load"
            );
            return;
        }
    };
    let Some(origin) = rows
        .into_iter()
        .find(|r| r.name == server)
        .and_then(|r| r.oauth)
    else {
        warn!(
            server = %server,
            "OAuth token rotation not persisted: no stored credential record for this server"
        );
        return;
    };
    let stored = crate::config::McpStoredOAuth {
        client_id: origin.client_id,
        token_url: origin.token_url,
        refresh_token: snapshot.refresh_token.clone(),
        access_token: Some(snapshot.access_token.clone()),
        expires_at: snapshot.expires_at.map(|e| e.timestamp()),
    };
    match persister.persist(settings_path, server, &stored, store) {
        Ok(true) => info!(server = %server, "Persisted rotated OAuth tokens (tool-call refresh)"),
        Ok(false) => warn!(
            server = %server,
            "Rotated OAuth tokens not persisted: entry vanished"
        ),
        Err(e) => warn!(
            server = %server,
            error = %e,
            "Persisting rotated OAuth tokens failed — the in-memory token stays new; \
             the next rotation retries the write"
        ),
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
        /// The bearer the mock currently accepts (F6: shared + mutable, so a
        /// test can invalidate the stored credential between connect and a
        /// tool call, forcing the 401 → refresh → retry rotation).
        required: Arc<std::sync::Mutex<Option<String>>>,
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
            let required = Arc::new(std::sync::Mutex::new(required_bearer.map(str::to_string)));

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let seen_task = seen.clone();
            let auth_task = saw_auth_header.clone();
            let gate = required.clone();

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
                    // Auth gate: when required, reject any other credential
                    // (read per request — tests rotate the bearer mid-run).
                    let required_now = gate.lock().unwrap().clone();
                    if let Some(required) = required_now {
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
                required,
            }
        }

        /// Rotate the bearer the mock accepts — requests with the old
        /// credential now get 401, forcing the client's refresh path.
        fn set_required_bearer(&self, bearer: Option<&str>) {
            *self.required.lock().unwrap() = bearer.map(str::to_string);
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
            seed_pool_from_config_in(&pool, vec![row], &tmp.path().join("settings.json"), None)
                .await;

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
        let result = seed_pool_from_config_in(&pool, vec![row], &settings_path, None).await;

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
        let result = seed_pool_from_config_in(&pool, vec![row], &settings_path, None).await;

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
        let result = seed_pool_from_config_in(&pool, vec![row], &settings_path, None).await;

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
        let result = seed_pool_from_config_in(&pool, vec![row], &settings_path, None).await;
        assert_eq!(result.servers_started, vec!["legacy-oauth".to_string()]);

        let before = std::fs::read_to_string(&settings_path).unwrap();
        let root: serde_json::Value = serde_json::from_str(&before).unwrap();
        assert!(root["mcpServers"]["legacy-oauth"]["shannonOAuth"].is_null());
        assert_eq!(
            root["mcpServers"]["legacy-oauth"]["headers"]["Authorization"],
            "Bearer legacy-token"
        );
    }

    /// F5 (R7-④ batch 2) W3-B regression: a **migrated** OAuth entry —
    /// plaintext secret material deleted, token only in the (mock)
    /// keyring — connects with the keyring token (the mock only accepts
    /// exactly that bearer), and a handshake-time refresh rotates the pair
    /// into the keyring while the file stays plaintext-free.
    #[tokio::test]
    async fn migrated_oauth_entry_connects_and_refreshes_from_keyring() {
        use crate::secret_store::{MockSecretStore, SecretStore, mcp_oauth_key};

        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        let mock = MockRemoteMcp::start_with_auth(Some("Bearer fresh-token")).await;
        let token_url = spawn_token_endpoint(
            r#"{"access_token":"fresh-token","refresh_token":"rotated-refresh","expires_in":3600,"token_type":"Bearer"}"#,
            false,
        )
        .await;

        // The migrated on-disk shape: NO shannonOAuth, NO Authorization —
        // everything a downgrade would need to pretend otherwise is gone.
        std::fs::write(
            &settings_path,
            serde_json::json!({
                "mcpServers": {
                    "linear-oauth": {
                        "type": "http",
                        "url": mock.url,
                        "enabled": true,
                        "shannon:transport": "oauth_remote",
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        // The keyring holds the pre-migration (stale) token pair.
        let store = MockSecretStore::new();
        store
            .put(
                &mcp_oauth_key("linear-oauth"),
                &format!(
                    r#"{{"client_id":"shannon-desktop","token_url":{token_url_json},"refresh_token":"stale-refresh","access_token":"stale-token"}}"#,
                    token_url_json = serde_json::to_string(&token_url).unwrap(),
                ),
            )
            .unwrap();

        // Load through the real loader (keyring-first), then seed.
        let row = crate::config::load_mcp_servers_with_store(&settings_path, Some(&store))
            .unwrap()
            .into_iter()
            .find(|s| s.name == "linear-oauth")
            .unwrap();
        assert_eq!(
            row.oauth.as_ref().unwrap().access_token.as_deref(),
            Some("stale-token"),
            "the token must come from the keyring"
        );

        let pool = Arc::new(McpProcessPool::new());
        let result = seed_pool_from_config_in(&pool, vec![row], &settings_path, Some(&store)).await;

        assert_eq!(result.servers_started, vec!["linear-oauth".to_string()]);
        assert_eq!(result.total_tools, 1);
        assert!(result.needs_auth_servers.is_empty());

        // The refresh rotated the pair into the keyring…
        let raw = store.value(&mcp_oauth_key("linear-oauth")).unwrap();
        let rotated: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(rotated["access_token"], "fresh-token");
        assert_eq!(rotated["refresh_token"], "rotated-refresh");

        // …and the file stayed plaintext-free.
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let entry = &root["mcpServers"]["linear-oauth"];
        assert!(entry.get("shannonOAuth").is_none(), "{entry}");
        assert!(entry.get("headers").is_none(), "{entry}");

        // Healthy proves the refreshed keyring credential was actually sent.
        let state = pool
            .list_servers()
            .await
            .into_iter()
            .find(|(name, _)| name == "linear-oauth")
            .map(|(_, state)| state);
        assert!(matches!(state, Some(ServerState::Healthy)), "{state:?}");
    }

    /// F5 honesty: an OAuth entry whose keyring entry vanished (restored
    /// backup / reset keychain) has no stored credential of its own — the
    /// row falls back to the W2-A header-less remote handling, the server
    /// rejects the anonymous handshake, and the row lands in the classified
    /// "needs re-authentication" state. Never a fake success.
    #[tokio::test]
    async fn migrated_entry_without_keyring_token_lands_in_honest_needs_auth() {
        use crate::secret_store::MockSecretStore;

        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        // The mock rejects every anonymous handshake with 401 — exactly
        // what a real OAuth remote does.
        let mock = MockRemoteMcp::start_with_auth(Some("Bearer never-issued")).await;
        std::fs::write(
            &settings_path,
            serde_json::json!({
                "mcpServers": {
                    "ghost-oauth": {
                        "type": "http",
                        "url": mock.url,
                        "enabled": true,
                        "shannon:transport": "oauth_remote",
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        let store = MockSecretStore::new();
        let row = crate::config::load_mcp_servers_with_store(&settings_path, Some(&store))
            .unwrap()
            .into_iter()
            .find(|s| s.name == "ghost-oauth")
            .unwrap();
        assert!(
            row.oauth.is_none(),
            "no keyring entry, no stored credential"
        );
        assert!(
            !row.has_auth_headers,
            "honest: no credential in the file either"
        );

        let pool = Arc::new(McpProcessPool::new());
        let result = seed_pool_from_config_in(&pool, vec![row], &settings_path, Some(&store)).await;
        assert!(result.servers_started.is_empty());

        // The 401 surfaces as the classified NeedsAuth state — the same
        // honest presentation an expired credential gets.
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
            shannon_mcp::RemoteFailureKind::NeedsAuth,
            "{msg}"
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

    // ── F6: tool-call-time token rotation persistence ─────────────────────

    /// SecretStore double delegating to a [`MockSecretStore`](crate::secret_store::MockSecretStore)
    /// while recording every `put` (value order + enter/exit instants) — the
    /// F6 debounce and serialization assertions need evidence the plain mock
    /// does not expose. Never touches the real keyring.
    struct CountingStore {
        inner: crate::secret_store::MockSecretStore,
        puts: std::sync::atomic::AtomicUsize,
        writes: std::sync::Mutex<Vec<(std::time::Instant, std::time::Instant, String)>>,
    }

    impl CountingStore {
        fn new() -> Self {
            Self {
                inner: crate::secret_store::MockSecretStore::new(),
                puts: std::sync::atomic::AtomicUsize::new(0),
                writes: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn put_count(&self) -> usize {
            self.puts.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn written_values(&self) -> Vec<String> {
            self.writes
                .lock()
                .unwrap()
                .iter()
                .map(|(_, _, value)| value.clone())
                .collect()
        }

        /// The stored value under `key` (test assertion helper).
        fn value(&self, key: &str) -> Option<String> {
            self.inner.value(key)
        }

        /// True when any two puts ran concurrently (their intervals overlap).
        fn has_overlapping_puts(&self) -> bool {
            let writes = self.writes.lock().unwrap();
            for (i, (start_a, end_a, _)) in writes.iter().enumerate() {
                for (start_b, end_b, _) in writes.iter().skip(i + 1) {
                    if *start_a < *end_b && *start_b < *end_a {
                        return true;
                    }
                }
            }
            false
        }
    }

    impl crate::secret_store::SecretStore for CountingStore {
        fn get(&self, key: &str) -> Result<Option<String>, String> {
            self.inner.get(key)
        }

        fn put(&self, key: &str, value: &str) -> Result<(), String> {
            let start = std::time::Instant::now();
            // A small critical section so interleaved (unserialized) writers
            // would actually overlap and the assertion would have teeth.
            std::thread::sleep(std::time::Duration::from_millis(5));
            let out = self.inner.put(key, value);
            let end = std::time::Instant::now();
            self.puts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.writes
                .lock()
                .unwrap()
                .push((start, end, value.to_string()));
            out
        }

        fn delete(&self, key: &str) -> Result<(), String> {
            self.inner.delete(key)
        }
    }

    /// Write a migrated-shape `settings.json` (no plaintext credential) for
    /// one OAuth url-only row and seed the (mock) keyring with a stale
    /// token block — the F6 test starting point.
    fn migrated_shape_with_stale_keyring(
        tmp: &tempfile::TempDir,
        name: &str,
        url: &str,
        token_url: &str,
    ) -> (std::path::PathBuf, Arc<CountingStore>) {
        use crate::secret_store::{SecretStore, mcp_oauth_key};

        let settings_path = tmp.path().join("settings.json");
        std::fs::write(
            &settings_path,
            serde_json::json!({
                "mcpServers": {
                    name: {
                        "type": "http",
                        "url": url,
                        "enabled": true,
                        "shannon:transport": "oauth_remote",
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        let store = Arc::new(CountingStore::new());
        // Seed through the inner mock directly — the counter exists to
        // observe F6 persistence writes, not the fixture setup.
        store
            .inner
            .put(
                &mcp_oauth_key(name),
                &format!(
                    r#"{{"client_id":"shannon-desktop","token_url":{token_url_json},"refresh_token":"stale-refresh","access_token":"stale-token"}}"#,
                    token_url_json = serde_json::to_string(token_url).unwrap(),
                ),
            )
            .unwrap();
        (settings_path, store)
    }

    /// F6 core acceptance: a tool-call-time 401 rotates the credential, the
    /// pool's rotation callback fires, and the rotated pair lands in the
    /// (mock) keyring — origin fields (client id, token endpoint) preserved,
    /// the file stays plaintext-free, and the connect wrote nothing at all.
    #[tokio::test]
    async fn tool_call_401_rotates_and_persists_to_keyring_via_callback() {
        use crate::secret_store::mcp_oauth_key;

        let tmp = tempfile::tempdir().unwrap();
        // The mock accepts the stored (stale) credential, so the connect
        // itself never rotates — the rotation is forced by the tool call.
        let mock = MockRemoteMcp::start_with_auth(Some("Bearer stale-token")).await;
        let token_url = spawn_token_endpoint(
            r#"{"access_token":"call-token","refresh_token":"call-refresh","expires_in":3600,"token_type":"Bearer"}"#,
            false,
        )
        .await;
        let (settings_path, store) =
            migrated_shape_with_stale_keyring(&tmp, "f6-rotator", &mock.url, &token_url);

        let row = crate::config::load_mcp_servers_with_store(&settings_path, Some(&*store))
            .unwrap()
            .into_iter()
            .find(|s| s.name == "f6-rotator")
            .unwrap();
        assert_eq!(
            row.oauth.as_ref().unwrap().access_token.as_deref(),
            Some("stale-token")
        );

        let pool = Arc::new(McpProcessPool::new());
        install_token_rotation_hook_in(
            &pool,
            settings_path.clone(),
            Some(store.clone() as Arc<dyn crate::secret_store::SecretStore>),
        )
        .await;
        let result =
            seed_pool_from_config_in(&pool, vec![row], &settings_path, Some(&*store)).await;
        assert_eq!(result.servers_started, vec!["f6-rotator".to_string()]);
        assert_eq!(
            store.put_count(),
            0,
            "connect used the stored token — no rotation, no write"
        );

        // The vendor invalidated the stored token: the tool call gets 401 →
        // refresh → retry, and the rotation must persist immediately.
        mock.set_required_bearer(Some("Bearer call-token"));
        let out = pool
            .call_tool("f6-rotator", "echo", serde_json::json!({}))
            .await
            .unwrap();
        assert!(!out.is_error, "retry with the refreshed token must succeed");

        let raw = store.value(&mcp_oauth_key("f6-rotator")).unwrap();
        let rotated: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(rotated["access_token"], "call-token");
        assert_eq!(rotated["refresh_token"], "call-refresh");
        // Origin fields survive the overlay — the next session can refresh.
        assert_eq!(rotated["client_id"], "shannon-desktop");
        assert_eq!(rotated["token_url"], token_url);
        assert!(rotated["expires_at"].as_i64().is_some());

        // The file stays plaintext-free (keyring mode).
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let entry = &root["mcpServers"]["f6-rotator"];
        assert!(entry.get("shannonOAuth").is_none(), "{entry}");
        assert!(entry.get("headers").is_none(), "{entry}");

        // Exactly one keyring write; the provider keeps the new token.
        assert_eq!(store.put_count(), 1);
        let snap = pool.remote_oauth_tokens("f6-rotator").await.unwrap();
        assert_eq!(snap.access_token, "call-token");
    }

    /// F6 debounce across the two writers: a handshake-time rotation is
    /// written exactly once — the rotation callback persists it during the
    /// connect, and the W3-B connect-time diff observes the same snapshot
    /// and dedups to a no-op.
    #[tokio::test]
    async fn handshake_rotation_is_persisted_once_with_hook_installed() {
        use crate::secret_store::mcp_oauth_key;

        let tmp = tempfile::tempdir().unwrap();
        // The mock rejects the stale credential — the handshake itself 401s,
        // refreshes and retries (the W3-B flow, now through F6).
        let mock = MockRemoteMcp::start_with_auth(Some("Bearer fresh-token")).await;
        let token_url = spawn_token_endpoint(
            r#"{"access_token":"fresh-token","refresh_token":"rotated-refresh","expires_in":3600,"token_type":"Bearer"}"#,
            false,
        )
        .await;
        let (settings_path, store) =
            migrated_shape_with_stale_keyring(&tmp, "f6-handshake", &mock.url, &token_url);

        let row = crate::config::load_mcp_servers_with_store(&settings_path, Some(&*store))
            .unwrap()
            .into_iter()
            .find(|s| s.name == "f6-handshake")
            .unwrap();

        let pool = Arc::new(McpProcessPool::new());
        install_token_rotation_hook_in(
            &pool,
            settings_path.clone(),
            Some(store.clone() as Arc<dyn crate::secret_store::SecretStore>),
        )
        .await;
        let result =
            seed_pool_from_config_in(&pool, vec![row], &settings_path, Some(&*store)).await;
        assert_eq!(result.servers_started, vec!["f6-handshake".to_string()]);

        assert_eq!(
            store.put_count(),
            1,
            "callback + connect-time diff must collapse into one keyring write; got {:?}",
            store.written_values()
        );
        let raw = store.value(&mcp_oauth_key("f6-handshake")).unwrap();
        let rotated: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(rotated["access_token"], "fresh-token");
        assert_eq!(rotated["refresh_token"], "rotated-refresh");

        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let entry = &root["mcpServers"]["f6-handshake"];
        assert!(entry.get("shannonOAuth").is_none(), "{entry}");
        assert!(entry.get("headers").is_none(), "{entry}");
    }

    /// F6 persister contract: same-value snapshots dedup to one write, a new
    /// value writes again, and a failed write leaves the dedup memory clean
    /// so the next attempt really retries (no lost rotation).
    #[test]
    fn persister_debounces_same_value_and_retries_after_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        std::fs::write(
            &settings_path,
            serde_json::json!({
                "mcpServers": {"deb": {"type": "http", "url": "https://mcp.example/mcp"}}
            })
            .to_string(),
        )
        .unwrap();
        let store = CountingStore::new();
        let persister = OAuthTokenPersister::new();
        let block = crate::config::McpStoredOAuth {
            client_id: "c".into(),
            token_url: "https://token".into(),
            refresh_token: Some("r1".into()),
            access_token: Some("a1".into()),
            expires_at: Some(1_000),
        };

        assert!(
            persister
                .persist(&settings_path, "deb", &block, Some(&store))
                .unwrap()
        );
        assert!(
            persister
                .persist(&settings_path, "deb", &block, Some(&store))
                .unwrap()
        );
        assert_eq!(store.put_count(), 1, "same-value snapshot must not rewrite");

        let mut rotated = block.clone();
        rotated.access_token = Some("a2".into());
        assert!(
            persister
                .persist(&settings_path, "deb", &rotated, Some(&store))
                .unwrap()
        );
        assert_eq!(store.put_count(), 2, "a new value writes again");

        // A failing attempt (unreadable settings.json) must not enter the
        // dedup memory — the retry through a good path really writes.
        let bogus = tmp.path().join("not-a-settings-file");
        std::fs::create_dir(&bogus).unwrap();
        let mut rotated_again = rotated.clone();
        rotated_again.access_token = Some("a3".into());
        assert!(
            persister
                .persist(&bogus, "deb", &rotated_again, Some(&store))
                .is_err()
        );
        assert!(
            persister
                .persist(&settings_path, "deb", &rotated_again, Some(&store))
                .unwrap()
        );
        assert_eq!(
            store.put_count(),
            4,
            "failed attempt recorded nothing — the retry wrote (keyring put on both attempts)"
        );
    }

    /// F6 failure presentation: a rotation that cannot be persisted warns
    /// (with the server context) and never crashes the tool-call path.
    #[test]
    fn rotated_snapshot_persist_failure_warns_with_server_context() {
        use crate::secret_store::test_support::capture_warnings;

        let tmp = tempfile::tempdir().unwrap();
        let bogus = tmp.path().join("unreadable"); // a directory: load fails
        std::fs::create_dir(&bogus).unwrap();
        let persister = OAuthTokenPersister::new();
        let snapshot = shannon_mcp::OAuthTokenSnapshot {
            access_token: "new-token".into(),
            refresh_token: Some("new-refresh".into()),
            expires_at: None,
        };
        let (capture, ()) = capture_warnings(|| {
            persist_rotated_snapshot(&persister, &bogus, None, "f6-failing", &snapshot);
        });
        let warnings = capture.warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("not persisted"),
            "warn must name the failure: {warnings:?}"
        );
    }

    /// F6 failure shape end to end: the rotation callback cannot persist —
    /// the tool call still succeeds and, critically, the provider keeps the
    /// new token in memory (nothing is lost; the next rotation retries).
    #[tokio::test]
    async fn rotation_persist_failure_keeps_new_token_in_memory() {
        use crate::secret_store::mcp_oauth_key;

        let tmp = tempfile::tempdir().unwrap();
        let mock = MockRemoteMcp::start_with_auth(Some("Bearer stored-token")).await;
        let token_url = spawn_token_endpoint(
            r#"{"access_token":"call-token","refresh_token":"call-refresh","expires_in":3600,"token_type":"Bearer"}"#,
            false,
        )
        .await;
        let store = Arc::new(crate::secret_store::MockSecretStore::new());

        let pool = Arc::new(McpProcessPool::new());
        // An unreadable settings path forces the callback's origin load to
        // fail — the pure "persist impossible" shape.
        let bogus = tmp.path().join("unreadable");
        std::fs::create_dir(&bogus).unwrap();
        install_token_rotation_hook_in(
            &pool,
            bogus.clone(),
            Some(store.clone() as Arc<dyn crate::secret_store::SecretStore>),
        )
        .await;

        // Row built in memory (the file is unreadable by design): valid
        // stored token, no rotation at connect.
        let row = crate::config::McpServerConfig {
            name: "f6-lossy".into(),
            command: "".into(),
            args: vec![],
            env: Default::default(),
            enabled: true,
            url: Some(mock.url.clone()),
            has_auth_headers: true,
            oauth: Some(crate::config::McpStoredOAuth {
                client_id: "shannon-desktop".into(),
                token_url,
                refresh_token: Some("stale-refresh".into()),
                access_token: Some("stored-token".into()),
                expires_at: None,
            }),
        };
        let result = seed_pool_from_config_in(&pool, vec![row], &bogus, Some(&*store)).await;
        assert_eq!(result.servers_started, vec!["f6-lossy".to_string()]);

        mock.set_required_bearer(Some("Bearer call-token"));
        let out = pool
            .call_tool("f6-lossy", "echo", serde_json::json!({}))
            .await
            .unwrap();
        assert!(!out.is_error, "the rotation itself still succeeds");

        let snap = pool.remote_oauth_tokens("f6-lossy").await.unwrap();
        assert_eq!(
            snap.access_token, "call-token",
            "memory keeps the new token despite the failed persist"
        );
        assert_eq!(snap.refresh_token.as_deref(), Some("call-refresh"));
        assert!(
            !store.contains(&mcp_oauth_key("f6-lossy")),
            "nothing landed in the store — the write never happened"
        );
    }

    /// F6 concurrency contract: the connect-time writer and the rotation
    /// callback share one persister — concurrent persists of distinct
    /// snapshots are serialized (no overlapping writes, final value = last
    /// write: a stale value can never overwrite a fresh rotation).
    #[test]
    fn concurrent_persist_through_one_persister_never_overlaps() {
        use crate::secret_store::mcp_oauth_key;

        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        std::fs::write(
            &settings_path,
            serde_json::json!({
                "mcpServers": {"f6-par": {"type": "http", "url": "https://mcp.example/mcp"}}
            })
            .to_string(),
        )
        .unwrap();
        let persister = Arc::new(OAuthTokenPersister::new());
        let store = Arc::new(CountingStore::new());

        std::thread::scope(|scope| {
            for i in 0..8 {
                let persister = persister.clone();
                let store = store.clone();
                let settings_path = settings_path.clone();
                scope.spawn(move || {
                    let stored = crate::config::McpStoredOAuth {
                        client_id: "c".into(),
                        token_url: "https://token".into(),
                        refresh_token: Some(format!("r-{i}")),
                        access_token: Some(format!("a-{i}")),
                        expires_at: Some(i),
                    };
                    persister
                        .persist(&settings_path, "f6-par", &stored, Some(&*store))
                        .unwrap();
                });
            }
        });

        assert_eq!(
            store.put_count(),
            8,
            "eight distinct snapshots → eight writes"
        );
        assert!(
            !store.has_overlapping_puts(),
            "writes through one persister must be serialized"
        );
        let values = store.written_values();
        let last: serde_json::Value = serde_json::from_str(values.last().unwrap()).unwrap();
        let final_block: serde_json::Value =
            serde_json::from_str(&store.value(&mcp_oauth_key("f6-par")).unwrap()).unwrap();
        assert_eq!(
            final_block, last,
            "the entry ends on the last write — no stale overwrite"
        );
    }

    /// Same-value contention: eight writers persisting one identical
    /// snapshot collapse into a single keyring write (debounce under races).
    #[test]
    fn same_value_concurrent_persist_writes_once() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        std::fs::write(
            &settings_path,
            serde_json::json!({
                "mcpServers": {"f6-same": {"type": "http", "url": "https://mcp.example/mcp"}}
            })
            .to_string(),
        )
        .unwrap();
        let persister = Arc::new(OAuthTokenPersister::new());
        let store = Arc::new(CountingStore::new());
        let block = crate::config::McpStoredOAuth {
            client_id: "c".into(),
            token_url: "https://token".into(),
            refresh_token: Some("r".into()),
            access_token: Some("a".into()),
            expires_at: Some(42),
        };

        std::thread::scope(|scope| {
            for _ in 0..8 {
                let persister = persister.clone();
                let store = store.clone();
                let settings_path = settings_path.clone();
                let block = block.clone();
                scope.spawn(move || {
                    persister
                        .persist(&settings_path, "f6-same", &block, Some(&*store))
                        .unwrap();
                });
            }
        });

        assert_eq!(
            store.put_count(),
            1,
            "identical snapshots → exactly one write"
        );
    }
}
