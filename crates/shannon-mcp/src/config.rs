//! MCP server configuration discovery and parsing.
//!
//! Supports Claude Code-compatible config file formats:
//!
//! - **Project-level**: `.mcp.json` in the project root
//! - **User-level**: `~/.claude/settings.json` (Claude Code) and `~/.shannon/settings.json`
//!
//! Config files use the `mcpServers` key to define MCP server entries:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "my-server": {
//!       "command": "npx",
//!       "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path"],
//!       "env": { "API_KEY": "${MY_API_KEY}" }
//!     },
//!     "remote-server": {
//!       "url": "http://localhost:3000/mcp"
//!     }
//!   }
//! }
//! ```
//!
//! Environment variable expansion (`${VAR}`) is supported in `env` values,
//! `command`, `args`, and `url` fields (see [`expand_env_vars`]).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Header source: static string or dynamic shell command
// ---------------------------------------------------------------------------

/// A header value that is either a static string or dynamically generated
/// by running a shell command.
///
/// In JSON config, headers accept either plain strings or objects:
///
/// ```json
/// {
///   "headers": {
///     "X-Static": "fixed-value",
///     "Authorization": { "command": "echo Bearer $(cat ~/.token)" }
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HeaderSource {
    /// A static header value.
    Static(String),
    /// A shell command whose stdout becomes the header value.
    Command { command: String },
}

impl HeaderSource {
    /// Resolve the header value. For static values, returns immediately.
    /// For commands, executes the shell command and returns its stdout (trimmed).
    pub async fn resolve(&self) -> Result<String, String> {
        match self {
            HeaderSource::Static(s) => Ok(s.clone()),
            HeaderSource::Command { command } => {
                let output = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .output()
                    .await
                    .map_err(|e| format!("Header command failed: {e}"))?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(format!(
                        "Header command exited with {}: {stderr}",
                        output.status
                    ));
                }
                let value = String::from_utf8_lossy(&output.stdout);
                Ok(value.trim().to_string())
            }
        }
    }

    /// Returns true if this is a dynamic (command-based) header.
    pub fn is_dynamic(&self) -> bool {
        matches!(self, HeaderSource::Command { .. })
    }
}

// ---------------------------------------------------------------------------
// Config types
// ---------------------------------------------------------------------------

/// Top-level config file structure for `.mcp.json` and `settings.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    /// MCP server definitions keyed by server name.
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: HashMap<String, McpServerConfig>,
    /// Glob patterns for allowed tools (e.g., `["mcp__*", "Bash", "!mcp__internal__*"]`).
    /// Loaded from `allowedTools` in settings.json. Empty = all tools allowed.
    #[serde(default, rename = "allowedTools")]
    pub allowed_tools: Vec<String>,
}

/// Authentication configuration for MCP servers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpAuthConfig {
    /// API key authentication: adds a header to each request.
    #[serde(rename = "api_key")]
    ApiKey {
        /// The API key value (supports `${VAR}` expansion).
        key: String,
        /// Header name (default: `X-API-Key`).
        #[serde(default)]
        header: Option<String>,
        /// Key prefix in header value, e.g. `"Bearer"` (default: none).
        #[serde(default)]
        prefix: Option<String>,
    },
    /// OAuth 2.0 PKCE authentication.
    #[serde(rename = "oauth")]
    OAuth {
        /// OAuth client ID.
        client_id: String,
        /// OAuth client secret (optional, for confidential clients).
        #[serde(default)]
        client_secret: Option<String>,
        /// Authorization endpoint URL.
        auth_url: String,
        /// Token endpoint URL.
        token_url: String,
        /// Redirect URL for the OAuth flow.
        redirect_url: String,
        /// OAuth scopes.
        #[serde(default)]
        scopes: Vec<String>,
    },
}

/// Configuration for a single MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpServerConfig {
    /// Stdio-based server: spawn a local process.
    #[serde(rename = "stdio")]
    Stdio {
        /// The command to execute.
        command: String,
        /// Command-line arguments.
        #[serde(default)]
        args: Vec<String>,
        /// Environment variables to set (supports `${VAR}` expansion).
        #[serde(default)]
        env: HashMap<String, String>,
    },
    /// SSE-based server: connect to an HTTP SSE endpoint.
    #[serde(rename = "sse")]
    Sse {
        /// The SSE endpoint URL.
        url: String,
        /// Optional HTTP headers (supports static values or `{ "command": "..." }`).
        #[serde(default)]
        headers: HashMap<String, HeaderSource>,
        /// Optional authentication configuration.
        #[serde(default)]
        auth: Option<McpAuthConfig>,
    },
    /// HTTP-based server: REST-style JSON-RPC.
    #[serde(rename = "http")]
    Http {
        /// The HTTP endpoint URL.
        url: String,
        /// Optional HTTP headers (supports static values or `{ "command": "..." }`).
        #[serde(default)]
        headers: HashMap<String, HeaderSource>,
        /// Optional authentication configuration.
        #[serde(default)]
        auth: Option<McpAuthConfig>,
    },
    /// WebSocket-based server.
    #[serde(rename = "websocket")]
    WebSocket {
        /// The WebSocket endpoint URL.
        url: String,
        /// Optional authentication configuration.
        #[serde(default)]
        auth: Option<McpAuthConfig>,
    },
}

/// Inline (untagged) config that auto-detects the server type.
///
/// If `command` is present → Stdio.
/// If `url` is present → Sse (default for URLs).
/// Otherwise falls back to Stdio with an empty command (will fail gracefully).
impl McpServerConfig {
    /// Parse from a JSON value using flexible detection.
    pub fn from_json_value(value: serde_json::Value) -> Result<Self, ConfigError> {
        // If "type" is explicitly set, use tagged deserialization
        if value.get("type").is_some() {
            return serde_json::from_value(value)
                .map_err(|e| ConfigError::ParseError(format!("Invalid server config: {e}")));
        }

        // Auto-detect: command → Stdio, url → Sse
        if value.get("command").is_some() {
            let raw: StdioRaw = serde_json::from_value(value)
                .map_err(|e| ConfigError::ParseError(format!("Invalid stdio config: {e}")))?;
            return Ok(McpServerConfig::Stdio {
                command: raw.command,
                args: raw.args.unwrap_or_default(),
                env: raw.env.unwrap_or_default(),
            });
        }

        if value.get("url").is_some() {
            let raw: UrlRaw = serde_json::from_value(value)
                .map_err(|e| ConfigError::ParseError(format!("Invalid URL config: {e}")))?;
            // Default URL-based servers to SSE
            return Ok(McpServerConfig::Sse {
                url: raw.url,
                headers: raw.headers.unwrap_or_default(),
                auth: raw.auth,
            });
        }

        Err(ConfigError::ParseError(
            "Server config must have 'command' or 'url' field".to_string(),
        ))
    }

    /// Validate that required fields are present.
    ///
    /// - Stdio: `command` must not be empty
    /// - Http / Sse: `url` must not be empty
    /// - WebSocket: `url` must not be empty
    pub fn validate(&self) -> Result<(), ConfigError> {
        match self {
            McpServerConfig::Stdio { command, .. } => {
                if command.is_empty() {
                    return Err(ConfigError::ValidationError(
                        "stdio server missing required field 'command'".to_string(),
                    ));
                }
            }
            McpServerConfig::Http { url, .. } => {
                if url.is_empty() {
                    return Err(ConfigError::ValidationError(
                        "http server missing required field 'url'".to_string(),
                    ));
                }
            }
            McpServerConfig::Sse { url, .. } => {
                if url.is_empty() {
                    return Err(ConfigError::ValidationError(
                        "sse server missing required field 'url'".to_string(),
                    ));
                }
            }
            McpServerConfig::WebSocket { url, .. } => {
                if url.is_empty() {
                    return Err(ConfigError::ValidationError(
                        "websocket server missing required field 'url'".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

impl McpConfig {
    /// Validate all server configs, returning the first error encountered.
    pub fn validate(&self) -> Result<(), ConfigError> {
        for (name, server_conf) in &self.mcp_servers {
            if let Err(e) = server_conf.validate() {
                return Err(ConfigError::ValidationError(format!(
                    "server '{name}': {e}"
                )));
            }
        }
        Ok(())
    }
}

/// Helper structs for flexible JSON parsing.
#[derive(Deserialize)]
struct StdioRaw {
    command: String,
    args: Option<Vec<String>>,
    env: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
struct UrlRaw {
    url: String,
    headers: Option<HashMap<String, HeaderSource>>,
    auth: Option<McpAuthConfig>,
}

/// Errors that can occur during config discovery and parsing.
#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("parse error: {0}")]
    ParseError(String),

    #[error("validation error: {0}")]
    ValidationError(String),

    #[error("no config files found")]
    NoConfig,
}

// ---------------------------------------------------------------------------
// Environment variable expansion (D2)
// ---------------------------------------------------------------------------

/// Expand `$VAR`, `${VAR}`, `${VAR:-default}`, and `${VAR:?error}` patterns in a string.
///
/// - `$VAR` → value of env var `VAR`, or empty string if not set
/// - `${VAR}` → value of env var `VAR`, or empty string if not set
/// - `${VAR:-default}` → value of env var `VAR`, or `default` if not set
/// - `${VAR:?error}` → value of env var `VAR`, or return error message if not set
pub fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            if chars.peek() == Some(&'{') {
                // ${VAR} / ${VAR:-default} / ${VAR:?error} form
                chars.next(); // consume '{'
                let mut var_name = String::new();
                let mut modifier = String::new();
                let mut in_modifier = false;

                loop {
                    match chars.peek() {
                        Some('}') => {
                            chars.next();
                            break;
                        }
                        Some(':') if !in_modifier => {
                            chars.next();
                            in_modifier = true;
                            // Check for modifier type
                            if chars.peek() == Some(&'-') {
                                chars.next();
                            } else if chars.peek() == Some(&'?') {
                                chars.next();
                                modifier.push('?');
                            }
                            continue;
                        }
                        Some(c) => {
                            if in_modifier {
                                modifier.push(*c);
                            } else {
                                var_name.push(*c);
                            }
                            chars.next();
                        }
                        None => break,
                    }
                }

                if var_name.is_empty() {
                    result.push_str("${");
                    if in_modifier {
                        result.push(':');
                    }
                    result.push_str(&modifier);
                    result.push('}');
                    continue;
                }

                let value = match std::env::var(&var_name) {
                    Ok(v) => v,
                    Err(_) => {
                        if modifier.starts_with('?') {
                            let err_msg = if modifier.len() > 1 {
                                &modifier[1..]
                            } else {
                                "required env var not set"
                            };
                            warn!(
                                var = %var_name,
                                error = err_msg,
                                "Required environment variable not set"
                            );
                            // Return the original pattern so it's visible in logs
                            format!("${{{var_name}:{modifier}}}")
                        } else if !modifier.is_empty() {
                            // Default value (after :-)
                            modifier.clone()
                        } else {
                            String::new()
                        }
                    }
                };
                result.push_str(&value);
            } else {
                // Bare $VAR form: consume identifier characters [A-Za-z_][A-Za-z0-9_]*
                let mut var_name = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_alphabetic() || next == '_' || (!var_name.is_empty() && next.is_ascii_digit()) {
                        var_name.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }

                if var_name.is_empty() {
                    // Lone '$' not followed by a valid identifier — keep as-is
                    result.push('$');
                } else {
                    let value = std::env::var(&var_name).unwrap_or_default();
                    result.push_str(&value);
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Expand env vars in auth config values.
fn expand_auth_config(auth: &mut Option<McpAuthConfig>) {
    if let Some(McpAuthConfig::ApiKey { key, .. }) = auth {
        *key = expand_env_vars(key);
    }
}

/// Expand env vars in all values of a server config (D2).
pub fn expand_server_config(config: &mut McpServerConfig) {
    match config {
        McpServerConfig::Stdio {
            command,
            args,
            env,
        } => {
            *command = expand_env_vars(command);
            for arg in args.iter_mut() {
                *arg = expand_env_vars(arg);
            }
            for val in env.values_mut() {
                *val = expand_env_vars(val);
            }
        }
        McpServerConfig::Sse { url, headers, auth } => {
            *url = expand_env_vars(url);
            for val in headers.values_mut() {
                match val {
                    HeaderSource::Static(s) => *s = expand_env_vars(s),
                    HeaderSource::Command { command } => *command = expand_env_vars(command),
                }
            }
            expand_auth_config(auth);
        }
        McpServerConfig::Http { url, headers, auth } => {
            *url = expand_env_vars(url);
            for val in headers.values_mut() {
                match val {
                    HeaderSource::Static(s) => *s = expand_env_vars(s),
                    HeaderSource::Command { command } => *command = expand_env_vars(command),
                }
            }
            expand_auth_config(auth);
        }
        McpServerConfig::WebSocket { url, auth } => {
            *url = expand_env_vars(url);
            expand_auth_config(auth);
        }
    }
}

// ---------------------------------------------------------------------------
// Config discovery (D1)
// ---------------------------------------------------------------------------

/// Search paths for MCP config files, in priority order.
///
/// Order: user-level configs first, then project-level configs.
/// Later entries override earlier ones when merged (last-wins).
pub fn config_search_paths(project_dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // 1. User-level: ~/.claude/settings.json (Claude Code compatibility)
    if let Some(home) = dirs_home() {
        paths.push(home.join(".claude").join("settings.json"));
    }

    // 2. User-level: ~/.shannon/settings.json
    if let Some(home) = dirs_home() {
        paths.push(home.join(".shannon").join("settings.json"));
    }

    // 3. Project-level: .mcp.json in the project root (shared via git)
    paths.push(project_dir.join(".mcp.json"));

    // 4. Project-level: .claude/settings.json (Claude Code compatibility)
    paths.push(project_dir.join(".claude").join("settings.json"));

    // 5. Project-level: .claude/settings.local.json (local overrides, not committed)
    paths.push(project_dir.join(".claude").join("settings.local.json"));

    // 6. Project-level: .shannon/settings.json
    paths.push(project_dir.join(".shannon").join("settings.json"));

    paths
}

/// Get the user's home directory.
fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

/// Discover and load MCP config from all config file locations.
///
/// Config files are loaded in the order returned by [`config_search_paths`]
/// (user-level first, project-level later). Later entries override earlier
/// ones for the same server name (last-wins semantics).
pub fn discover_config(project_dir: &Path) -> Result<McpConfig, ConfigError> {
    let search_paths = config_search_paths(project_dir);
    let mut merged = McpConfig::default();
    let mut found_any = false;

    for path in &search_paths {
        match load_config_file(path) {
            Ok(config) => {
                info!(
                    path = %path.display(),
                    servers = config.mcp_servers.len(),
                    "Loaded MCP config"
                );
                // Later configs override earlier — always insert (last-wins)
                for (name, server_conf) in config.mcp_servers {
                    merged.mcp_servers.insert(name, server_conf);
                }
                // Merge allowed tools: later config's list replaces earlier
                if !config.allowed_tools.is_empty() {
                    merged.allowed_tools = config.allowed_tools;
                }
                found_any = true;
            }
            Err(ConfigError::Io(_)) => {
                debug!(path = %path.display(), "Config file not found, skipping");
            }
            Err(e) => {
                warn!(path = %path.display(), error = %e, "Failed to parse config file");
            }
        }
    }

    if !found_any {
        debug!("No MCP config files found");
    }

    // Apply env var expansion to all server configs
    for server_conf in merged.mcp_servers.values_mut() {
        expand_server_config(server_conf);
    }

    // Validate all server configs
    for (name, server_conf) in &merged.mcp_servers {
        if let Err(e) = server_conf.validate() {
            warn!(server = %name, error = %e, "Invalid server config");
        }
    }

    Ok(merged)
}

/// Load a single config file.
fn load_config_file(path: &Path) -> Result<McpConfig, ConfigError> {
    let content = std::fs::read_to_string(path)?;

    // Try to parse as raw JSON to handle the flexible server config format
    let raw: serde_json::Value = serde_json::from_str(&content)?;

    let mcp_servers_value = match raw.get("mcpServers") {
        Some(v) => v,
        None => {
            // No mcpServers key — but might still have allowedTools
            let allowed_tools = parse_allowed_tools(&raw);
            if allowed_tools.is_empty() {
                debug!(
                    path = %path.display(),
                    "No 'mcpServers' key found in config file"
                );
            }
            return Ok(McpConfig {
                mcp_servers: HashMap::new(),
                allowed_tools,
            });
        }
    };

    let mcp_servers_obj = mcp_servers_value
        .as_object()
        .ok_or_else(|| ConfigError::ParseError("'mcpServers' must be an object".to_string()))?;

    let mut servers = HashMap::new();

    for (name, server_value) in mcp_servers_obj {
        match McpServerConfig::from_json_value(server_value.clone()) {
            Ok(config) => {
                debug!(server = %name, "Parsed MCP server config");
                servers.insert(name.clone(), config);
            }
            Err(e) => {
                warn!(server = %name, error = %e, "Failed to parse server config, skipping");
            }
        }
    }

    let allowed_tools = parse_allowed_tools(&raw);

    Ok(McpConfig {
        mcp_servers: servers,
        allowed_tools,
    })
}

// ---------------------------------------------------------------------------
// Allowed tools parsing (T5)
// ---------------------------------------------------------------------------

/// Extract `allowedTools` from a raw JSON config value.
///
/// Looks for an `"allowedTools"` array at the top level of the config file.
/// Entries are returned as-is (glob patterns like `["mcp__*", "Bash", "!mcp__internal__*"]`).
fn parse_allowed_tools(raw: &serde_json::Value) -> Vec<String> {
    match raw.get("allowedTools").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Env var expansion tests ─────────────────────────────────────────

    #[test]
    fn test_expand_simple_var() {
        unsafe { std::env::set_var("TEST_MCP_KEY", "hello"); }
        assert_eq!(expand_env_vars("${TEST_MCP_KEY}"), "hello");
        unsafe { std::env::remove_var("TEST_MCP_KEY"); }
    }

    #[test]
    fn test_expand_missing_var_empty() {
        unsafe { std::env::remove_var("TEST_MCP_MISSING_VAR"); }
        assert_eq!(expand_env_vars("${TEST_MCP_MISSING_VAR}"), "");
    }

    #[test]
    fn test_expand_default_value() {
        unsafe { std::env::remove_var("TEST_MCP_NO_SUCH_VAR"); }
        assert_eq!(
            expand_env_vars("${TEST_MCP_NO_SUCH_VAR:-fallback}"),
            "fallback"
        );
    }

    #[test]
    fn test_expand_var_present_ignores_default() {
        unsafe { std::env::set_var("TEST_MCP_EXISTS", "actual"); }
        assert_eq!(
            expand_env_vars("${TEST_MCP_EXISTS:-fallback}"),
            "actual"
        );
        unsafe { std::env::remove_var("TEST_MCP_EXISTS"); }
    }

    #[test]
    fn test_expand_in_string() {
        unsafe { std::env::set_var("TEST_MCP_HOST", "localhost"); }
        assert_eq!(
            expand_env_vars("http://${TEST_MCP_HOST}:3000"),
            "http://localhost:3000"
        );
        unsafe { std::env::remove_var("TEST_MCP_HOST"); }
    }

    #[test]
    fn test_expand_no_vars() {
        assert_eq!(expand_env_vars("plain text"), "plain text");
    }

    #[test]
    fn test_expand_empty_name() {
        // ${} with empty name should be preserved
        assert_eq!(expand_env_vars("${}"), "${}");
    }

    // ── Config parsing tests ────────────────────────────────────────────

    #[test]
    fn test_parse_stdio_config() {
        let json = serde_json::json!({
            "command": "npx",
            "args": ["-y", "some-package"],
            "env": {"KEY": "value"}
        });

        let config = McpServerConfig::from_json_value(json).unwrap();
        match config {
            McpServerConfig::Stdio {
                command,
                args,
                env,
            } => {
                assert_eq!(command, "npx");
                assert_eq!(args, vec!["-y", "some-package"]);
                assert_eq!(env.get("KEY").unwrap(), "value");
            }
            _ => panic!("Expected Stdio config"),
        }
    }

    #[test]
    fn test_parse_url_config() {
        let json = serde_json::json!({
            "url": "http://localhost:3000/mcp"
        });

        let config = McpServerConfig::from_json_value(json).unwrap();
        match config {
            McpServerConfig::Sse { url, .. } => {
                assert_eq!(url, "http://localhost:3000/mcp");
            }
            _ => panic!("Expected Sse config"),
        }
    }

    #[test]
    fn test_parse_explicit_type_sse() {
        let json = serde_json::json!({
            "type": "sse",
            "url": "http://localhost:3000/sse"
        });

        let config = McpServerConfig::from_json_value(json).unwrap();
        match config {
            McpServerConfig::Sse { url, .. } => {
                assert_eq!(url, "http://localhost:3000/sse");
            }
            _ => panic!("Expected Sse config"),
        }
    }

    #[test]
    fn test_parse_explicit_type_http() {
        let json = serde_json::json!({
            "type": "http",
            "url": "http://localhost:3000/api"
        });

        let config = McpServerConfig::from_json_value(json).unwrap();
        match config {
            McpServerConfig::Http { url, .. } => {
                assert_eq!(url, "http://localhost:3000/api");
            }
            _ => panic!("Expected Http config"),
        }
    }

    #[test]
    fn test_parse_full_mcp_json() {
        let json = serde_json::json!({
            "mcpServers": {
                "filesystem": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem"],
                    "env": {"ROOT": "/tmp"}
                },
                "remote": {
                    "url": "http://localhost:4000"
                }
            }
        });

        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

        let config = load_config_file(&path).unwrap();
        assert_eq!(config.mcp_servers.len(), 2);
        assert!(config.mcp_servers.contains_key("filesystem"));
        assert!(config.mcp_servers.contains_key("remote"));
    }

    #[test]
    fn test_parse_settings_json_without_mcp() {
        let json = serde_json::json!({
            "theme": "dark",
            "editor": "vim"
        });

        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

        let config = load_config_file(&path).unwrap();
        assert!(config.mcp_servers.is_empty());
    }

    #[test]
    fn test_env_expansion_in_config() {
        unsafe { std::env::set_var("TEST_MCP_EXPAND_PATH", "/expanded/path"); }
        let json = serde_json::json!({
            "mcpServers": {
                "test": {
                    "command": "node",
                    "args": ["${TEST_MCP_EXPAND_PATH}/server.js"],
                    "env": {"ROOT": "${TEST_MCP_EXPAND_PATH}/root"}
                }
            }
        });

        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

        let mut config = load_config_file(&path).unwrap();

        // Manually expand — discover_config does this automatically but
        // load_config_file is a private helper that skips expansion.
        for server_conf in config.mcp_servers.values_mut() {
            expand_server_config(server_conf);
        }

        let server = config.mcp_servers.get("test").unwrap();

        match server {
            McpServerConfig::Stdio {
                command,
                args,
                env,
            } => {
                assert_eq!(command, "node");
                assert_eq!(args[0], "/expanded/path/server.js");
                assert_eq!(env.get("ROOT").unwrap(), "/expanded/path/root");
            }
            _ => panic!("Expected Stdio"),
        }
        unsafe { std::env::remove_var("TEST_MCP_EXPAND_PATH"); }
    }

    #[test]
    fn test_discover_config_no_files() {
        let temp = tempfile::tempdir().unwrap();
        let config = discover_config(temp.path()).unwrap();
        assert!(config.mcp_servers.is_empty());
    }

    #[test]
    fn test_discover_config_merges() {
        let temp = tempfile::tempdir().unwrap();

        // Write project-level .mcp.json
        let project_config = serde_json::json!({
            "mcpServers": {
                "project-server": {
                    "command": "node",
                    "args": ["project.js"]
                }
            }
        });
        std::fs::write(
            temp.path().join(".mcp.json"),
            serde_json::to_string(&project_config).unwrap(),
        )
        .unwrap();

        let config = discover_config(temp.path()).unwrap();
        assert_eq!(config.mcp_servers.len(), 1);
        assert!(config.mcp_servers.contains_key("project-server"));
    }

    #[test]
    fn test_config_search_paths_order() {
        let temp = tempfile::tempdir().unwrap();
        let paths = config_search_paths(temp.path());

        // Order: user-level first, project-level later (later overrides earlier)
        // 1. ~/.claude/settings.json (or home if available)
        // 2. ~/.shannon/settings.json (or home if available)
        // 3. .mcp.json
        // 4. .claude/settings.json
        // 5. .claude/settings.local.json
        // 6. .shannon/settings.json

        // Project .mcp.json should be at index 2 (or lower if home dir exists)
        let mcp_idx = paths.iter().position(|p| p.ends_with(".mcp.json")).unwrap();
        let claude_project_idx = paths
            .iter()
            .position(|p| {
                p.ends_with(".claude/settings.json")
                    && !p.starts_with(dirs_home().unwrap_or_default())
            })
            .unwrap();
        let claude_local_idx = paths
            .iter()
            .position(|p| p.ends_with("settings.local.json"))
            .unwrap();

        // .mcp.json should come before .claude/settings.json (project)
        assert!(mcp_idx < claude_project_idx);
        // .claude/settings.json should come before settings.local.json
        assert!(claude_project_idx < claude_local_idx);
        // Should have exactly 6 paths (or fewer if no home dir)
        assert!(paths.len() <= 6);
    }

    // ── Expand server config tests ──────────────────────────────────────

    // ── Allowed tools parsing tests (T5) ─────────────────────────────────

    #[test]
    fn test_parse_allowed_tools_present() {
        let json = serde_json::json!({
            "allowedTools": ["mcp__*", "Bash", "!mcp__internal__*"]
        });
        let tools = parse_allowed_tools(&json);
        assert_eq!(tools, vec!["mcp__*", "Bash", "!mcp__internal__*"]);
    }

    #[test]
    fn test_parse_allowed_tools_missing() {
        let json = serde_json::json!({"other": "value"});
        let tools = parse_allowed_tools(&json);
        assert!(tools.is_empty());
    }

    #[test]
    fn test_parse_allowed_tools_via_config_file() {
        let json = serde_json::json!({
            "allowedTools": ["mcp__myserver__*"],
            "mcpServers": {
                "myserver": {
                    "command": "node",
                    "args": ["server.js"]
                }
            }
        });
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), serde_json::to_string(&json).unwrap()).unwrap();
        let config = load_config_file(temp.path()).unwrap();
        assert_eq!(config.allowed_tools, vec!["mcp__myserver__*"]);
    }

    // ── Auth config tests (T3) ──────────────────────────────────────────

    #[test]
    fn test_parse_api_key_auth_config() {
        let json = serde_json::json!({
            "mcpServers": {
                "remote": {
                    "url": "http://localhost:3000",
                    "auth": {
                        "type": "api_key",
                        "key": "my-secret-key",
                        "header": "Authorization",
                        "prefix": "Bearer"
                    }
                }
            }
        });
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), serde_json::to_string(&json).unwrap()).unwrap();
        let config = load_config_file(temp.path()).unwrap();
        let server = config.mcp_servers.get("remote").unwrap();
        match server {
            McpServerConfig::Sse { auth, .. } => {
                let auth = auth.as_ref().expect("auth should be set");
                match auth {
                    McpAuthConfig::ApiKey { key, header, prefix } => {
                        assert_eq!(key, "my-secret-key");
                        assert_eq!(header.as_deref(), Some("Authorization"));
                        assert_eq!(prefix.as_deref(), Some("Bearer"));
                    }
                    _ => panic!("Expected ApiKey auth"),
                }
            }
            _ => panic!("Expected Sse config"),
        }
    }

    #[test]
    fn test_parse_oauth_auth_config() {
        let json = serde_json::json!({
            "mcpServers": {
                "remote": {
                    "url": "http://localhost:3000",
                    "auth": {
                        "type": "oauth",
                        "client_id": "my-client",
                        "auth_url": "https://auth.example.com/authorize",
                        "token_url": "https://auth.example.com/token",
                        "redirect_url": "http://localhost:8080/callback",
                        "scopes": ["read", "write"]
                    }
                }
            }
        });
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), serde_json::to_string(&json).unwrap()).unwrap();
        let config = load_config_file(temp.path()).unwrap();
        let server = config.mcp_servers.get("remote").unwrap();
        match server {
            McpServerConfig::Sse { auth, .. } => {
                let auth = auth.as_ref().expect("auth should be set");
                match auth {
                    McpAuthConfig::OAuth { client_id, scopes, .. } => {
                        assert_eq!(client_id, "my-client");
                        assert_eq!(*scopes, vec!["read", "write"]);
                    }
                    _ => panic!("Expected OAuth auth"),
                }
            }
            _ => panic!("Expected Sse config"),
        }
    }

    #[test]
    fn test_auth_env_expansion() {
        unsafe { std::env::set_var("TEST_MCP_API_KEY", "expanded-key-123"); }
        let json = serde_json::json!({
            "mcpServers": {
                "remote": {
                    "url": "http://localhost:3000",
                    "auth": {
                        "type": "api_key",
                        "key": "${TEST_MCP_API_KEY}"
                    }
                }
            }
        });
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), serde_json::to_string(&json).unwrap()).unwrap();
        let mut config = load_config_file(temp.path()).unwrap();
        for server_conf in config.mcp_servers.values_mut() {
            expand_server_config(server_conf);
        }
        let server = config.mcp_servers.get("remote").unwrap();
        match server {
            McpServerConfig::Sse { auth, .. } => {
                let auth = auth.as_ref().unwrap();
                match auth {
                    McpAuthConfig::ApiKey { key, .. } => {
                        assert_eq!(key, "expanded-key-123");
                    }
                    _ => panic!("Expected ApiKey auth"),
                }
            }
            _ => panic!("Expected Sse config"),
        }
        unsafe { std::env::remove_var("TEST_MCP_API_KEY"); }
    }

    #[test]
    fn test_expand_stdio_config() {
        unsafe { std::env::set_var("TEST_MCP_MY_CMD", "/usr/bin/my-cmd"); }
        unsafe { std::env::set_var("TEST_MCP_SECRET", "abc123"); }

        let mut config = McpServerConfig::Stdio {
            command: "${TEST_MCP_MY_CMD}".to_string(),
            args: vec!["--key".to_string(), "${TEST_MCP_SECRET}".to_string()],
            env: {
                let mut m = HashMap::new();
                m.insert("API_KEY".to_string(), "${TEST_MCP_SECRET}".to_string());
                m
            },
        };

        expand_server_config(&mut config);

        match config {
            McpServerConfig::Stdio {
                command,
                args,
                env,
            } => {
                assert_eq!(command, "/usr/bin/my-cmd");
                assert_eq!(args[1], "abc123");
                assert_eq!(env.get("API_KEY").unwrap(), "abc123");
            }
            _ => panic!("Expected Stdio"),
        }

        unsafe { std::env::remove_var("TEST_MCP_MY_CMD"); }
        unsafe { std::env::remove_var("TEST_MCP_SECRET"); }
    }

    #[test]
    fn test_expand_url_config() {
        unsafe { std::env::set_var("TEST_MCP_SVC_HOST", "my-server.example.com"); }

        let mut config = McpServerConfig::Sse {
            url: "https://${TEST_MCP_SVC_HOST}/sse".to_string(),
            headers: HashMap::new(),
            auth: None,
        };

        expand_server_config(&mut config);

        match config {
            McpServerConfig::Sse { url, .. } => {
                assert_eq!(url, "https://my-server.example.com/sse");
            }
            _ => panic!("Expected Sse"),
        }

        unsafe { std::env::remove_var("TEST_MCP_SVC_HOST"); }
    }

    // ── Bare $VAR expansion tests ──────────────────────────────────────

    #[test]
    fn test_expand_bare_var() {
        unsafe { std::env::set_var("TEST_MCP_BARE", "bare-value"); }
        assert_eq!(expand_env_vars("$TEST_MCP_BARE"), "bare-value");
        unsafe { std::env::remove_var("TEST_MCP_BARE"); }
    }

    #[test]
    fn test_expand_bare_var_missing() {
        unsafe { std::env::remove_var("TEST_MCP_BARE_MISSING"); }
        assert_eq!(expand_env_vars("$TEST_MCP_BARE_MISSING"), "");
    }

    #[test]
    fn test_expand_bare_var_in_url() {
        unsafe { std::env::set_var("TEST_MCP_BARE_HOST", "api.example.com"); }
        assert_eq!(
            expand_env_vars("https://$TEST_MCP_BARE_HOST/v1/mcp"),
            "https://api.example.com/v1/mcp"
        );
        unsafe { std::env::remove_var("TEST_MCP_BARE_HOST"); }
    }

    #[test]
    fn test_expand_bare_var_adjacent_to_text() {
        unsafe { std::env::set_var("TEST_MCP_PREFIX", "hello"); }
        assert_eq!(
            expand_env_vars("${TEST_MCP_PREFIX}_world"),
            "hello_world"
        );
        unsafe { std::env::remove_var("TEST_MCP_PREFIX"); }
    }

    #[test]
    fn test_expand_dollar_sign_alone() {
        assert_eq!(expand_env_vars("price is $5"), "price is $5");
    }

    #[test]
    fn test_expand_dollar_digit() {
        // $5 should not be treated as a variable (starts with digit)
        assert_eq!(expand_env_vars("$5"), "$5");
    }

    #[test]
    fn test_expand_bare_var_in_bearer_header() {
        unsafe { std::env::set_var("TEST_MCP_TOKEN_VAR", "tok123"); }
        assert_eq!(
            expand_env_vars("Bearer $TEST_MCP_TOKEN_VAR"),
            "Bearer tok123"
        );
        unsafe { std::env::remove_var("TEST_MCP_TOKEN_VAR"); }
    }

    // ── Multi-file merging (last-wins) tests ──────────────────────────

    #[test]
    fn test_discover_config_last_wins_override() {
        let temp = tempfile::tempdir().unwrap();

        // .mcp.json defines "my-server" with command "node"
        let mcp_json = serde_json::json!({
            "mcpServers": {
                "my-server": {
                    "command": "node",
                    "args": ["first.js"]
                }
            }
        });
        std::fs::write(
            temp.path().join(".mcp.json"),
            serde_json::to_string(&mcp_json).unwrap(),
        )
        .unwrap();

        // .claude/settings.local.json overrides "my-server" with command "python"
        let claude_dir = temp.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let local_json = serde_json::json!({
            "mcpServers": {
                "my-server": {
                    "command": "python",
                    "args": ["second.py"]
                }
            }
        });
        std::fs::write(
            claude_dir.join("settings.local.json"),
            serde_json::to_string(&local_json).unwrap(),
        )
        .unwrap();

        let config = discover_config(temp.path()).unwrap();

        // The later file (settings.local.json) should override .mcp.json
        let server = config.mcp_servers.get("my-server").unwrap();
        match server {
            McpServerConfig::Stdio { command, args, .. } => {
                assert_eq!(command, "python");
                assert_eq!(args[0], "second.py");
            }
            _ => panic!("Expected Stdio"),
        }
    }

    #[test]
    fn test_discover_config_merges_from_multiple_files() {
        let temp = tempfile::tempdir().unwrap();

        // .mcp.json has "server-a"
        let mcp_json = serde_json::json!({
            "mcpServers": {
                "server-a": {
                    "command": "node",
                    "args": ["a.js"]
                }
            }
        });
        std::fs::write(
            temp.path().join(".mcp.json"),
            serde_json::to_string(&mcp_json).unwrap(),
        )
        .unwrap();

        // .claude/settings.json has "server-b"
        let claude_dir = temp.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let claude_json = serde_json::json!({
            "mcpServers": {
                "server-b": {
                    "url": "http://localhost:4000"
                }
            }
        });
        std::fs::write(
            claude_dir.join("settings.json"),
            serde_json::to_string(&claude_json).unwrap(),
        )
        .unwrap();

        // .shannon/settings.json has "server-c"
        let shannon_dir = temp.path().join(".shannon");
        std::fs::create_dir_all(&shannon_dir).unwrap();
        let shannon_json = serde_json::json!({
            "mcpServers": {
                "server-c": {
                    "type": "http",
                    "url": "http://localhost:5000"
                }
            }
        });
        std::fs::write(
            shannon_dir.join("settings.json"),
            serde_json::to_string(&shannon_json).unwrap(),
        )
        .unwrap();

        let config = discover_config(temp.path()).unwrap();

        // All three servers should be present
        assert_eq!(config.mcp_servers.len(), 3);
        assert!(config.mcp_servers.contains_key("server-a"));
        assert!(config.mcp_servers.contains_key("server-b"));
        assert!(config.mcp_servers.contains_key("server-c"));
    }

    // ── Validation tests ──────────────────────────────────────────────

    #[test]
    fn test_validate_stdio_ok() {
        let config = McpServerConfig::Stdio {
            command: "npx".to_string(),
            args: vec![],
            env: HashMap::new(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_stdio_empty_command() {
        let config = McpServerConfig::Stdio {
            command: "".to_string(),
            args: vec![],
            env: HashMap::new(),
        };
        let err = config.validate().unwrap_err();
        assert!(
            matches!(err, ConfigError::ValidationError(ref msg) if msg.contains("command")),
            "Expected ValidationError mentioning 'command', got: {err:?}"
        );
    }

    #[test]
    fn test_validate_http_ok() {
        let config = McpServerConfig::Http {
            url: "http://localhost:3000".to_string(),
            headers: HashMap::new(),
            auth: None,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_http_empty_url() {
        let config = McpServerConfig::Http {
            url: "".to_string(),
            headers: HashMap::new(),
            auth: None,
        };
        let err = config.validate().unwrap_err();
        assert!(
            matches!(err, ConfigError::ValidationError(ref msg) if msg.contains("url")),
            "Expected ValidationError mentioning 'url', got: {err:?}"
        );
    }

    #[test]
    fn test_validate_sse_empty_url() {
        let config = McpServerConfig::Sse {
            url: "".to_string(),
            headers: HashMap::new(),
            auth: None,
        };
        let err = config.validate().unwrap_err();
        assert!(
            matches!(err, ConfigError::ValidationError(ref msg) if msg.contains("url")),
            "Expected ValidationError mentioning 'url', got: {err:?}"
        );
    }

    #[test]
    fn test_validate_websocket_empty_url() {
        let config = McpServerConfig::WebSocket {
            url: "".to_string(),
            auth: None,
        };
        let err = config.validate().unwrap_err();
        assert!(
            matches!(err, ConfigError::ValidationError(ref msg) if msg.contains("url")),
            "Expected ValidationError mentioning 'url', got: {err:?}"
        );
    }

    #[test]
    fn test_validate_mcp_config_all_ok() {
        let mut servers = HashMap::new();
        servers.insert(
            "my-server".to_string(),
            McpServerConfig::Stdio {
                command: "node".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
        );
        let config = McpConfig {
            mcp_servers: servers,
            allowed_tools: vec![],
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_mcp_config_invalid_server() {
        let mut servers = HashMap::new();
        servers.insert(
            "bad-server".to_string(),
            McpServerConfig::Stdio {
                command: "".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
        );
        let config = McpConfig {
            mcp_servers: servers,
            allowed_tools: vec![],
        };
        let err = config.validate().unwrap_err();
        assert!(
            matches!(err, ConfigError::ValidationError(ref msg) if msg.contains("bad-server")),
            "Expected ValidationError mentioning 'bad-server', got: {err:?}"
        );
    }

    #[test]
    fn test_validate_no_command_field_at_all() {
        // JSON with neither command nor url should fail
        let json = serde_json::json!({
            "args": ["just-args"]
        });
        let result = McpServerConfig::from_json_value(json);
        assert!(result.is_err(), "Expected error when neither command nor url present");
    }
}
