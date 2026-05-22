//! # MCP Advanced Services
//!
//! Provides advanced MCP (Model Context Protocol) server management including
//! communication channels, the elicitation protocol, and server discovery/registry.
//!
//! ## Architecture
//!
//! - [`McpServerRegistry`]: Discovers and manages MCP server configurations
//! - [`McpChannelManager`]: Manages communication channels between MCP servers
//! - [`ElicitationHandler`]: Handles MCP elicitation protocol (user input during tool execution)
//! - [`McpServerConfig`]: Configuration for a single MCP server
//! - [`McpChannel`]: A communication channel to an MCP server
//! - [`ElicitationRequest`]: A pending elicitation (user input) request
//!
//! ## Example
//!
//! ```no_run
//! use shannon_core::mcp_advanced::{McpServerRegistry, McpServerConfig, TransportType};
//!
//! let mut registry = McpServerRegistry::new();
//! let config = McpServerConfig {
//!     name: "my-server".to_string(),
//!     transport_type: TransportType::Stdio,
//!     command: Some("node".to_string()),
//!     args: vec!["server.js".to_string()],
//!     env: Default::default(),
//!     url: None,
//!     headers: Default::default(),
//!     enabled: true,
//!     timeout_secs: None,
//!     discovery_timeout_secs: None,
//!     oauth_scopes: Vec::new(),
//! };
//! registry.register(config);
//! ```

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during MCP advanced operations.
#[derive(Error, Debug)]
pub enum McpAdvancedError {
    #[error("Server not found: {0}")]
    ServerNotFound(String),

    #[error("Channel not found: {0}")]
    ChannelNotFound(String),

    #[error("Elicitation not found: {0}")]
    ElicitationNotFound(String),

    #[error("Channel already exists: {0}")]
    ChannelAlreadyExists(String),

    #[error("Server already registered: {0}")]
    ServerAlreadyRegistered(String),

    #[error("Invalid server configuration: {0}")]
    InvalidConfig(String),

    #[error("Channel error: {0}")]
    ChannelError(String),

    #[error("Transport not supported: {0}")]
    TransportNotSupported(String),
}

// ============================================================================
// Transport Type
// ============================================================================

/// The transport mechanism used to communicate with an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TransportType {
    /// Standard input/output (subprocess communication)
    #[serde(rename = "stdio", alias = "Stdio")]
    Stdio,
    /// HTTP-based transport (streamable HTTP)
    #[serde(rename = "http", alias = "Http")]
    Http,
    /// Server-Sent Events transport
    #[serde(rename = "sse", alias = "Sse")]
    Sse,
}

impl std::fmt::Display for TransportType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportType::Stdio => write!(f, "stdio"),
            TransportType::Http => write!(f, "http"),
            TransportType::Sse => write!(f, "sse"),
        }
    }
}

// ============================================================================
// McpServerConfig
// ============================================================================

/// Configuration for a single MCP server instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpServerConfig {
    /// Unique human-readable name for the server
    pub name: String,

    /// Transport type (stdio, http, or sse)
    #[serde(default = "default_transport_stdio")]
    pub transport_type: TransportType,

    /// Command to execute (for stdio transport)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Arguments passed to the command
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables for the server process
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// URL for HTTP/SSE transports
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// HTTP headers for HTTP/SSE transports
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Whether the server is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Tool call execution timeout in seconds (default: 30)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,

    /// Discovery/initialization timeout in seconds (default: 15)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery_timeout_secs: Option<u64>,

    /// OAuth scopes required by this server (e.g., `["read", "write"]`).
    ///
    /// When a server returns HTTP 403 with `insufficient_scope`, these scopes
    /// are used to re-authenticate with broader permissions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub oauth_scopes: Vec<String>,
}

fn default_transport_stdio() -> TransportType {
    TransportType::Stdio
}

fn default_enabled() -> bool {
    true
}

/// Expand environment variables in a server config.
///
/// Supports `${VAR}` and `${VAR:-default}` syntax in command, args, env values, url, and headers.
fn expand_env_vars_in_config(config: &mut McpServerConfig) {
    if let Some(ref mut cmd) = config.command {
        *cmd = expand_env_vars(cmd);
    }
    for arg in &mut config.args {
        *arg = expand_env_vars(arg);
    }
    for val in config.env.values_mut() {
        *val = expand_env_vars(val);
    }
    if let Some(ref mut url) = config.url {
        *url = expand_env_vars(url);
    }
    for val in config.headers.values_mut() {
        *val = expand_env_vars(val);
    }
}

/// Expand `${VAR}` and `${VAR:-default}` in a string.
fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut var_name = String::new();
            let mut default_val = String::new();
            let mut in_default = false;

            while let Some(c) = chars.next() {
                if c == '}' {
                    break;
                } else if c == ':' && chars.peek() == Some(&'-') {
                    chars.next(); // consume '-'
                    in_default = true;
                } else if in_default {
                    default_val.push(c);
                } else {
                    var_name.push(c);
                }
            }

            if !var_name.is_empty() {
                match std::env::var(&var_name) {
                    Ok(val) => result.push_str(&val),
                    Err(_) => {
                        if !default_val.is_empty() {
                            result.push_str(&default_val);
                        }
                        // If no default and var not set, leave as-is (empty)
                    }
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

impl McpServerConfig {
    /// Create a new stdio-based server configuration.
    pub fn new_stdio(name: &str, command: &str) -> Self {
        Self {
            name: name.to_string(),
            transport_type: TransportType::Stdio,
            command: Some(command.to_string()),
            args: Vec::new(),
            env: HashMap::new(),
            url: None,
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: None,
            discovery_timeout_secs: None,
            oauth_scopes: Vec::new(),
        }
    }

    /// Create a new HTTP-based server configuration.
    pub fn new_http(name: &str, url: &str) -> Self {
        Self {
            name: name.to_string(),
            transport_type: TransportType::Http,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            url: Some(url.to_string()),
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: None,
            discovery_timeout_secs: None,
            oauth_scopes: Vec::new(),
        }
    }

    /// Create a new SSE-based server configuration.
    pub fn new_sse(name: &str, url: &str) -> Self {
        Self {
            name: name.to_string(),
            transport_type: TransportType::Sse,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            url: Some(url.to_string()),
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: None,
            discovery_timeout_secs: None,
            oauth_scopes: Vec::new(),
        }
    }

    /// Validate the server configuration.
    pub fn validate(&self) -> Result<(), McpAdvancedError> {
        if self.name.is_empty() {
            return Err(McpAdvancedError::InvalidConfig(
                "Server name must not be empty".to_string(),
            ));
        }

        if self.transport_type == TransportType::Stdio && self.command.is_none() {
            return Err(McpAdvancedError::InvalidConfig(
                "Stdio transport requires a command".to_string(),
            ));
        }

        if matches!(self.transport_type, TransportType::Http | TransportType::Sse)
            && self.url.is_none()
        {
            return Err(McpAdvancedError::InvalidConfig(format!(
                "{:?} transport requires a URL",
                self.transport_type
            )));
        }

        if let Some(ref cmd) = self.command {
            if cmd.is_empty() {
                return Err(McpAdvancedError::InvalidConfig(
                    "Command must not be empty".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Compute a content-based signature for deduplication.
    ///
    /// Two configs pointing to the same server should produce the same signature
    /// even if they have different names (e.g., plugin-provided vs manually configured).
    /// - Stdio: `stdio:{command} {args}`
    /// - HTTP/SSE: `http:{url}` or `sse:{url}`
    pub fn content_signature(&self) -> String {
        match self.transport_type {
            TransportType::Stdio => {
                let cmd = self.command.as_deref().unwrap_or("");
                let args = self.args.join(" ");
                if args.is_empty() {
                    format!("stdio:{cmd}")
                } else {
                    format!("stdio:{cmd} {args}")
                }
            }
            TransportType::Http => {
                format!("http:{}", self.url.as_deref().unwrap_or(""))
            }
            TransportType::Sse => {
                format!("sse:{}", self.url.as_deref().unwrap_or(""))
            }
        }
    }
}

// ============================================================================
// McpChannel
// ============================================================================

/// Status of an MCP communication channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ChannelStatus {
    /// Channel is being established
    Connecting,
    /// Channel is active and ready
    Active,
    /// Channel is paused
    Paused,
    /// Channel has been closed
    Closed,
    /// Channel encountered an error
    Error(String),
}

/// Capabilities advertised by an MCP server on a channel.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ChannelCapabilities {
    /// Server supports elicitation (requesting user input)
    pub elicitation: bool,
    /// Server supports streaming responses
    pub streaming: bool,
    /// Server supports progress notifications
    pub progress: bool,
    /// Server supports cancellation
    pub cancellation: bool,
    /// Additional capabilities as key-value pairs
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// A communication channel to an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpChannel {
    /// Unique channel identifier
    pub id: String,
    /// Human-readable channel name
    pub name: String,
    /// The server this channel connects to
    pub server_id: String,
    /// Current channel status
    pub status: ChannelStatus,
    /// Capabilities advertised by the server
    pub capabilities: ChannelCapabilities,
    /// When the channel was created
    pub created_at: DateTime<Utc>,
    /// When the channel was last active
    pub last_active_at: DateTime<Utc>,
    /// Number of messages sent through this channel
    pub message_count: u64,
}

impl McpChannel {
    /// Create a new MCP channel.
    pub fn new(name: &str, server_id: &str) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            server_id: server_id.to_string(),
            status: ChannelStatus::Connecting,
            capabilities: ChannelCapabilities::default(),
            created_at: now,
            last_active_at: now,
            message_count: 0,
        }
    }

    /// Mark the channel as active.
    pub fn activate(&mut self) {
        self.status = ChannelStatus::Active;
        self.last_active_at = Utc::now();
    }

    /// Pause the channel.
    pub fn pause(&mut self) {
        self.status = ChannelStatus::Paused;
    }

    /// Close the channel.
    pub fn close(&mut self) {
        self.status = ChannelStatus::Closed;
    }

    /// Record an error on the channel.
    pub fn set_error(&mut self, message: &str) {
        self.status = ChannelStatus::Error(message.to_string());
    }

    /// Record a message sent through this channel.
    pub fn record_message(&mut self) {
        self.message_count += 1;
        self.last_active_at = Utc::now();
    }
}

// ============================================================================
// ElicitationRequest
// ============================================================================

/// Status of an elicitation request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ElicitationStatus {
    /// Awaiting user response
    Pending,
    /// User has responded
    Responded,
    /// Elicitation was cancelled
    Cancelled,
    /// Elicitation timed out
    Expired,
}

/// A request for user input during MCP tool execution (elicitation protocol).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationRequest {
    /// Unique request identifier
    pub id: String,
    /// Human-readable message describing what input is needed
    pub message: String,
    /// JSON Schema describing the expected response format
    pub requested_schema: serde_json::Value,
    /// The user's response, if provided
    pub response: Option<serde_json::Value>,
    /// Current status of the request
    pub status: ElicitationStatus,
    /// The channel/server that issued the request
    pub source_channel_id: String,
    /// When the request was created
    pub created_at: DateTime<Utc>,
    /// When the request was responded to
    pub responded_at: Option<DateTime<Utc>>,
}

impl ElicitationRequest {
    /// Create a new elicitation request.
    pub fn new(
        message: &str,
        requested_schema: serde_json::Value,
        source_channel_id: &str,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            message: message.to_string(),
            requested_schema,
            response: None,
            status: ElicitationStatus::Pending,
            source_channel_id: source_channel_id.to_string(),
            created_at: Utc::now(),
            responded_at: None,
        }
    }

    /// Provide a response to this elicitation request.
    pub fn respond(&mut self, response: serde_json::Value) -> Result<(), McpAdvancedError> {
        if self.status != ElicitationStatus::Pending {
            return Err(McpAdvancedError::ChannelError(format!(
                "Cannot respond to elicitation in {:?} state",
                self.status
            )));
        }
        self.response = Some(response);
        self.status = ElicitationStatus::Responded;
        self.responded_at = Some(Utc::now());
        Ok(())
    }

    /// Cancel this elicitation request.
    pub fn cancel(&mut self) -> Result<(), McpAdvancedError> {
        if self.status != ElicitationStatus::Pending {
            return Err(McpAdvancedError::ChannelError(format!(
                "Cannot cancel elicitation in {:?} state",
                self.status
            )));
        }
        self.status = ElicitationStatus::Cancelled;
        Ok(())
    }

    /// Mark the elicitation as expired.
    pub fn expire(&mut self) {
        if self.status == ElicitationStatus::Pending {
            self.status = ElicitationStatus::Expired;
        }
    }
}

// ============================================================================
// McpChannelManager
// ============================================================================

/// Manages communication channels between MCP servers.
#[derive(Debug, Clone, Default)]
pub struct McpChannelManager {
    /// Map of channel ID to channel
    channels: HashMap<String, McpChannel>,
    /// Map of channel name to channel ID (for lookup by name)
    name_index: HashMap<String, String>,
}

impl McpChannelManager {
    /// Create a new channel manager.
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
            name_index: HashMap::new(),
        }
    }

    /// Create and register a new channel.
    pub fn create_channel(
        &mut self,
        name: &str,
        server_id: &str,
    ) -> Result<&McpChannel, McpAdvancedError> {
        if self.name_index.contains_key(name) {
            return Err(McpAdvancedError::ChannelAlreadyExists(name.to_string()));
        }

        let channel = McpChannel::new(name, server_id);
        let id = channel.id.clone();
        self.name_index.insert(name.to_string(), id.clone());
        self.channels.insert(id.clone(), channel);
        Ok(self.channels.get(&id).expect("channel was just inserted after contains_key check"))
    }

    /// Get a channel by ID.
    pub fn get_channel(&self, id: &str) -> Result<&McpChannel, McpAdvancedError> {
        self.channels
            .get(id)
            .ok_or_else(|| McpAdvancedError::ChannelNotFound(id.to_string()))
    }

    /// Get a mutable channel by ID.
    pub fn get_channel_mut(&mut self, id: &str) -> Result<&mut McpChannel, McpAdvancedError> {
        self.channels
            .get_mut(id)
            .ok_or_else(|| McpAdvancedError::ChannelNotFound(id.to_string()))
    }

    /// Get a channel by name.
    pub fn get_by_name(&self, name: &str) -> Result<&McpChannel, McpAdvancedError> {
        let id = self
            .name_index
            .get(name)
            .ok_or_else(|| McpAdvancedError::ChannelNotFound(name.to_string()))?;
        self.get_channel(id)
    }

    /// Activate a channel by ID.
    pub fn activate_channel(&mut self, id: &str) -> Result<(), McpAdvancedError> {
        let channel = self.get_channel_mut(id)?;
        channel.activate();
        Ok(())
    }

    /// Close a channel by ID.
    pub fn close_channel(&mut self, id: &str) -> Result<(), McpAdvancedError> {
        let channel = self.get_channel_mut(id)?;
        channel.close();
        Ok(())
    }

    /// Remove a channel by ID.
    pub fn remove_channel(&mut self, id: &str) -> Result<McpChannel, McpAdvancedError> {
        let channel = self
            .channels
            .remove(id)
            .ok_or_else(|| McpAdvancedError::ChannelNotFound(id.to_string()))?;
        self.name_index.remove(&channel.name);
        Ok(channel)
    }

    /// List all channels.
    pub fn list_channels(&self) -> Vec<&McpChannel> {
        self.channels.values().collect()
    }

    /// List channels for a specific server.
    pub fn channels_for_server(&self, server_id: &str) -> Vec<&McpChannel> {
        self.channels
            .values()
            .filter(|c| c.server_id == server_id)
            .collect()
    }

    /// Get the number of active channels.
    pub fn active_count(&self) -> usize {
        self.channels
            .values()
            .filter(|c| c.status == ChannelStatus::Active)
            .count()
    }

    /// Get the total number of channels.
    pub fn total_count(&self) -> usize {
        self.channels.len()
    }
}

// ============================================================================
// ElicitationHandler
// ============================================================================

/// Handles the MCP elicitation protocol for requesting user input during tool execution.
#[derive(Debug, Clone, Default)]
pub struct ElicitationHandler {
    /// Pending and completed elicitation requests
    requests: HashMap<String, ElicitationRequest>,
}

impl ElicitationHandler {
    /// Create a new elicitation handler.
    pub fn new() -> Self {
        Self {
            requests: HashMap::new(),
        }
    }

    /// Create a new elicitation request.
    pub fn create_request(
        &mut self,
        message: &str,
        requested_schema: serde_json::Value,
        source_channel_id: &str,
    ) -> ElicitationRequest {
        let request =
            ElicitationRequest::new(message, requested_schema, source_channel_id);
        let id = request.id.clone();
        self.requests.insert(id.clone(), request);
        self.requests.get(&id).cloned().expect("request was just inserted")
    }

    /// Get an elicitation request by ID.
    pub fn get_request(&self, id: &str) -> Result<&ElicitationRequest, McpAdvancedError> {
        self.requests
            .get(id)
            .ok_or_else(|| McpAdvancedError::ElicitationNotFound(id.to_string()))
    }

    /// Respond to an elicitation request.
    pub fn respond(
        &mut self,
        id: &str,
        response: serde_json::Value,
    ) -> Result<ElicitationRequest, McpAdvancedError> {
        let request = self
            .requests
            .get_mut(id)
            .ok_or_else(|| McpAdvancedError::ElicitationNotFound(id.to_string()))?;
        request.respond(response)?;
        Ok(request.clone())
    }

    /// Cancel an elicitation request.
    pub fn cancel(&mut self, id: &str) -> Result<ElicitationRequest, McpAdvancedError> {
        let request = self
            .requests
            .get_mut(id)
            .ok_or_else(|| McpAdvancedError::ElicitationNotFound(id.to_string()))?;
        request.cancel()?;
        Ok(request.clone())
    }

    /// Expire all pending requests older than a given threshold.
    pub fn expire_pending(&mut self, threshold: chrono::Duration) -> usize {
        let now = Utc::now();
        let mut expired = 0;
        for request in self.requests.values_mut() {
            if request.status == ElicitationStatus::Pending
                && now - request.created_at > threshold
            {
                request.expire();
                expired += 1;
            }
        }
        expired
    }

    /// List all pending requests.
    pub fn pending_requests(&self) -> Vec<&ElicitationRequest> {
        self.requests
            .values()
            .filter(|r| r.status == ElicitationStatus::Pending)
            .collect()
    }

    /// List all requests for a specific channel.
    pub fn requests_for_channel(&self, channel_id: &str) -> Vec<&ElicitationRequest> {
        self.requests
            .values()
            .filter(|r| r.source_channel_id == channel_id)
            .collect()
    }

    /// Remove a completed request.
    pub fn remove_request(&mut self, id: &str) -> Result<ElicitationRequest, McpAdvancedError> {
        self.requests
            .remove(id)
            .ok_or_else(|| McpAdvancedError::ElicitationNotFound(id.to_string()))
    }

    /// Get the count of pending requests.
    pub fn pending_count(&self) -> usize {
        self.requests
            .values()
            .filter(|r| r.status == ElicitationStatus::Pending)
            .count()
    }
}

// ============================================================================
// McpServerRegistry
// ============================================================================

/// Discovers and manages MCP server configurations.
#[derive(Debug, Clone, Default)]
pub struct McpServerRegistry {
    /// Map of server name to configuration
    servers: HashMap<String, McpServerConfig>,
    /// Content signatures for deduplication (command+args for stdio, url for remote)
    signatures: std::collections::HashSet<String>,
}

impl McpServerRegistry {
    /// Create a new server registry.
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
            signatures: std::collections::HashSet::new(),
        }
    }

    /// Register a new MCP server configuration.
    ///
    /// Skips if a server with the same content signature already exists
    /// (different name but same command/args or URL).
    pub fn register(&mut self, config: McpServerConfig) -> Result<(), McpAdvancedError> {
        config.validate()?;
        if self.servers.contains_key(&config.name) {
            return Err(McpAdvancedError::ServerAlreadyRegistered(
                config.name.clone(),
            ));
        }
        let sig = config.content_signature();
        if self.signatures.contains(&sig) {
            tracing::debug!(
                "Skipping MCP server '{}': content signature already registered ({})",
                config.name, sig
            );
            return Ok(());
        }
        self.signatures.insert(sig);
        self.servers.insert(config.name.clone(), config);
        Ok(())
    }

    /// Unregister an MCP server by name.
    pub fn unregister(&mut self, name: &str) -> Result<McpServerConfig, McpAdvancedError> {
        let config = self.servers
            .remove(name)
            .ok_or_else(|| McpAdvancedError::ServerNotFound(name.to_string()))?;
        self.signatures.remove(&config.content_signature());
        Ok(config)
    }

    /// Get a server configuration by name.
    pub fn get(&self, name: &str) -> Result<&McpServerConfig, McpAdvancedError> {
        self.servers
            .get(name)
            .ok_or_else(|| McpAdvancedError::ServerNotFound(name.to_string()))
    }

    /// Get a mutable server configuration by name.
    pub fn get_mut(&mut self, name: &str) -> Result<&mut McpServerConfig, McpAdvancedError> {
        self.servers
            .get_mut(name)
            .ok_or_else(|| McpAdvancedError::ServerNotFound(name.to_string()))
    }

    /// Update an existing server configuration.
    pub fn update(
        &mut self,
        name: &str,
        config: McpServerConfig,
    ) -> Result<(), McpAdvancedError> {
        config.validate()?;
        if !self.servers.contains_key(name) {
            return Err(McpAdvancedError::ServerNotFound(name.to_string()));
        }
        // Remove old signature, add new one
        if let Some(old) = self.servers.get(name) {
            self.signatures.remove(&old.content_signature());
        }
        self.signatures.insert(config.content_signature());
        self.servers.insert(name.to_string(), config);
        Ok(())
    }

    /// Enable or disable a server by name.
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> Result<(), McpAdvancedError> {
        let config = self.get_mut(name)?;
        config.enabled = enabled;
        Ok(())
    }

    /// List all registered servers.
    pub fn list_servers(&self) -> Vec<&McpServerConfig> {
        self.servers.values().collect()
    }

    /// List only enabled servers.
    pub fn enabled_servers(&self) -> Vec<&McpServerConfig> {
        self.servers.values().filter(|s| s.enabled).collect()
    }

    /// List servers by transport type.
    pub fn servers_by_transport(&self, transport: &TransportType) -> Vec<&McpServerConfig> {
        self.servers
            .values()
            .filter(|s| s.transport_type == *transport)
            .collect()
    }

    /// Check if a server is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.servers.contains_key(name)
    }

    /// Get the number of registered servers.
    pub fn count(&self) -> usize {
        self.servers.len()
    }

    /// Get the number of enabled servers.
    pub fn enabled_count(&self) -> usize {
        self.servers.values().filter(|s| s.enabled).count()
    }

    /// Load server configurations from a JSON value (e.g., parsed from a config file).
    pub fn load_from_json(&mut self, json: serde_json::Value) -> Result<(), McpAdvancedError> {
        if let Some(servers) = json.as_array() {
            for server_value in servers {
                let config: McpServerConfig = serde_json::from_value(server_value.clone())
                    .map_err(|e| {
                        McpAdvancedError::InvalidConfig(format!(
                            "Failed to parse server config: {e}"
                        ))
                    })?;
                if self.servers.contains_key(&config.name) {
                    self.update(&config.name.clone(), config)?;
                } else {
                    self.register(config)?;
                }
            }
        }
        Ok(())
    }

    /// Load from Claude Code format: `{"mcpServers": {"name": {...}, ...}}`.
    ///
    /// Each entry maps to an `McpServerConfig`. The `type` field determines
    /// transport (default: stdio). If `command` is present, transport is stdio.
    /// If `url` is present with `type: "http"` or `type: "sse"`, that transport is used.
    pub fn load_from_mcp_json_value(&mut self, json: &serde_json::Value) -> usize {
        let servers_map = match json.get("mcpServers").and_then(|v| v.as_object()) {
            Some(map) => map,
            None => return 0,
        };

        let mut count = 0usize;
        for (name, config_val) in servers_map {
            match Self::parse_claude_code_server(name, config_val) {
                Ok(mut config) => {
                    expand_env_vars_in_config(&mut config);
                    let config_name = config.name.clone();
                    if self.servers.contains_key(&config_name) {
                        if self.update(&config_name, config).is_ok() {
                            count += 1;
                        }
                    } else if self.register(config).is_ok() {
                        count += 1;
                    }
                }
                Err(e) => {
                    tracing::warn!("Skipping MCP server '{}': {}", name, e);
                }
            }
        }
        count
    }

    /// Parse a single Claude Code MCP server entry into `McpServerConfig`.
    fn parse_claude_code_server(
        name: &str,
        val: &serde_json::Value,
    ) -> Result<McpServerConfig, McpAdvancedError> {
        let transport_type = match val.get("type").and_then(|v| v.as_str()) {
            Some("http") => TransportType::Http,
            Some("sse") => TransportType::Sse,
            _ => {
                // Default: stdio if command is present, otherwise http if url is present
                if val.get("command").is_some() {
                    TransportType::Stdio
                } else if val.get("url").is_some() {
                    TransportType::Http
                } else {
                    TransportType::Stdio
                }
            }
        };

        let command = val.get("command").and_then(|v| v.as_str()).map(String::from);
        let args = val
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let env = val
            .get("env")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let url = val.get("url").and_then(|v| v.as_str()).map(String::from);
        let headers = val
            .get("headers")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        let config = McpServerConfig {
            name: name.to_string(),
            transport_type,
            command,
            args,
            env,
            url,
            headers,
            enabled: true,
            timeout_secs: None,
            discovery_timeout_secs: None,
            oauth_scopes: Vec::new(),
        };
        config.validate()?;
        Ok(config)
    }

    /// Export all server configurations as a JSON array.
    pub fn export_to_json(&self) -> serde_json::Value {
        let configs: Vec<&McpServerConfig> = self.list_servers();
        serde_json::to_value(configs).unwrap_or(serde_json::Value::Array(vec![]))
    }

    /// Load server configurations from default file paths.
    ///
    /// Searches (in order of priority, later overrides earlier):
    ///
    /// **Legacy Shannon paths** (flat JSON array format):
    ///   1. `~/.shannon/mcp_servers.json` — global user config
    ///
    /// **Claude Code compatible paths** (later wins):
    ///   2. `~/.claude/settings.json` — user-level (mcpServers key)
    ///   3. `.claude/settings.json` — project-level (mcpServers key)
    ///   4. `.claude/settings.local.json` — project local (mcpServers key)
    ///   5. `.mcp.json` — project-level MCP config (Claude Code standard)
    ///   6. `.shannon/settings.json` — Shannon project-level (mcpServers key)
    ///   7. `.shannon/mcp_servers.json` — Shannon project-local (flat array)
    ///
    /// Returns the number of servers loaded.
    pub fn load_from_default_paths(&mut self) -> usize {
        self.load_from_default_paths_with_base(std::path::PathBuf::from("."))
    }

    /// Same as [`load_from_default_paths`] but with an explicit project base directory.
    pub fn load_from_default_paths_with_base(&mut self, base: std::path::PathBuf) -> usize {
        let mut count = 0usize;
        let home = dirs::home_dir();

        // Helper: load a JSON file and try both array and mcpServers formats
        let try_load_file = |path: &std::path::PathBuf| -> Option<serde_json::Value> {
            if !path.exists() {
                return None;
            }
            match std::fs::read_to_string(path) {
                Ok(content) => serde_json::from_str(&content).ok(),
                Err(e) => {
                    tracing::warn!("Cannot read MCP config {:?}: {}", path, e);
                    None
                }
            }
        };

        // Phase 1: Legacy Shannon flat-array format (lowest priority)
        if let Some(ref home) = home {
            let legacy_global = home.join(".shannon").join("mcp_servers.json");
            if let Some(json) = try_load_file(&legacy_global) {
                let before = self.count();
                if json.is_array() {
                    let _ = self.load_from_json(json);
                } else {
                    // Also try mcpServers key for backward compat
                    self.load_from_mcp_json_value(&json);
                }
                let loaded = self.count().saturating_sub(before);
                if loaded > 0 {
                    tracing::info!("Loaded {} MCP server(s) from {:?}", loaded, legacy_global);
                    count += loaded;
                }
            }
        }

        // Phase 2: Claude Code compatible paths (increasing priority)
        let claude_code_paths: Vec<std::path::PathBuf> = {
            let mut p = Vec::new();
            // User-level: ~/.claude/settings.json
            if let Some(ref home) = home {
                p.push(home.join(".claude").join("settings.json"));
            }
            // Project-level settings
            p.push(base.join(".claude").join("settings.json"));
            p.push(base.join(".claude").join("settings.local.json"));
            p.push(base.join(".shannon").join("settings.json"));
            // .mcp.json (Claude Code project standard)
            p.push(base.join(".mcp.json"));
            // Shannon project-local (highest priority)
            p.push(base.join(".shannon").join("mcp_servers.json"));
            // Machine-local only — not committed to VCS (highest priority)
            p.push(base.join(".shannon").join("mcp.local.json"));
            p
        };

        for path in &claude_code_paths {
            if let Some(json) = try_load_file(path) {
                let before = self.count();
                // Try mcpServers key first (Claude Code format)
                let loaded_mcp = self.load_from_mcp_json_value(&json);
                // If no mcpServers, try flat array (Shannon legacy)
                if loaded_mcp == 0 && json.is_array() {
                    let _ = self.load_from_json(json);
                }
                let loaded = self.count().saturating_sub(before);
                if loaded > 0 {
                    tracing::info!("Loaded {} MCP server(s) from {:?}", loaded, path);
                    count += loaded;
                }
            }
        }

        count
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- McpServerConfig tests ----

    #[test]
    fn test_stdio_config_creation() {
        let config = McpServerConfig::new_stdio("test-server", "node");
        assert_eq!(config.name, "test-server");
        assert_eq!(config.transport_type, TransportType::Stdio);
        assert_eq!(config.command, Some("node".to_string()));
        assert!(config.args.is_empty());
        assert!(config.enabled);
    }

    #[test]
    fn test_http_config_creation() {
        let config = McpServerConfig::new_http("remote-server", "https://example.com/mcp");
        assert_eq!(config.name, "remote-server");
        assert_eq!(config.transport_type, TransportType::Http);
        assert!(config.command.is_none());
        assert_eq!(config.url.as_deref(), Some("https://example.com/mcp"));
    }

    #[test]
    fn test_config_validation() {
        // Valid stdio config
        let config = McpServerConfig::new_stdio("valid", "python");
        assert!(config.validate().is_ok());

        // Empty name
        let mut config = McpServerConfig::new_stdio("", "python");
        assert!(config.validate().is_err());

        // Stdio without command
        config = McpServerConfig {
            name: "no-cmd".to_string(),
            transport_type: TransportType::Stdio,
            command: None,
            args: vec![],
            env: HashMap::new(),
            url: None,
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: None,
            discovery_timeout_secs: None,
            oauth_scopes: Vec::new(),
        };
        assert!(config.validate().is_err());

        // Empty command
        config = McpServerConfig {
            name: "empty-cmd".to_string(),
            transport_type: TransportType::Stdio,
            command: Some(String::new()),
            args: vec![],
            env: HashMap::new(),
            url: None,
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: None,
            discovery_timeout_secs: None,
            oauth_scopes: Vec::new(),
        };
        assert!(config.validate().is_err());

        // HTTP without command is fine
        let config = McpServerConfig::new_http("http-ok", "https://example.com");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_serialization() {
        let config = McpServerConfig::new_stdio("ser-test", "node");
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: McpServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    // ---- McpChannel tests ----

    #[test]
    fn test_channel_creation() {
        let channel = McpChannel::new("test-channel", "server-1");
        assert_eq!(channel.name, "test-channel");
        assert_eq!(channel.server_id, "server-1");
        assert_eq!(channel.status, ChannelStatus::Connecting);
        assert_eq!(channel.message_count, 0);
    }

    #[test]
    fn test_channel_lifecycle() {
        let mut channel = McpChannel::new("lifecycle", "srv");
        assert_eq!(channel.status, ChannelStatus::Connecting);

        channel.activate();
        assert_eq!(channel.status, ChannelStatus::Active);

        channel.pause();
        assert_eq!(channel.status, ChannelStatus::Paused);

        channel.activate();
        assert_eq!(channel.status, ChannelStatus::Active);

        channel.set_error("connection lost");
        assert_eq!(channel.status, ChannelStatus::Error("connection lost".to_string()));

        channel.close();
        assert_eq!(channel.status, ChannelStatus::Closed);
    }

    #[test]
    fn test_channel_message_recording() {
        let mut channel = McpChannel::new("msg-test", "srv");
        channel.activate();

        channel.record_message();
        assert_eq!(channel.message_count, 1);

        channel.record_message();
        channel.record_message();
        assert_eq!(channel.message_count, 3);
    }

    // ---- McpChannelManager tests ----

    #[test]
    fn test_channel_manager_create_and_get() {
        let mut manager = McpChannelManager::new();
        let channel = manager.create_channel("ch1", "srv1").unwrap();
        let id = channel.id.clone();

        assert_eq!(channel.name, "ch1");
        assert_eq!(channel.server_id, "srv1");

        // Get by ID
        let fetched = manager.get_channel(&id).unwrap();
        assert_eq!(fetched.name, "ch1");

        // Get by name
        let fetched = manager.get_by_name("ch1").unwrap();
        assert_eq!(fetched.id, id);
    }

    #[test]
    fn test_channel_manager_duplicate_name() {
        let mut manager = McpChannelManager::new();
        manager.create_channel("ch1", "srv1").unwrap();

        let result = manager.create_channel("ch1", "srv2");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), McpAdvancedError::ChannelAlreadyExists(_)));
    }

    #[test]
    fn test_channel_manager_remove() {
        let mut manager = McpChannelManager::new();
        let channel = manager.create_channel("ch1", "srv1").unwrap();
        let id = channel.id.clone();

        let removed = manager.remove_channel(&id).unwrap();
        assert_eq!(removed.name, "ch1");

        // Should no longer be accessible
        assert!(manager.get_channel(&id).is_err());
        assert!(manager.get_by_name("ch1").is_err());
    }

    #[test]
    fn test_channel_manager_server_channels() {
        let mut manager = McpChannelManager::new();
        manager.create_channel("ch1", "srv-a").unwrap();
        manager.create_channel("ch2", "srv-a").unwrap();
        manager.create_channel("ch3", "srv-b").unwrap();

        let srv_a = manager.channels_for_server("srv-a");
        assert_eq!(srv_a.len(), 2);

        let srv_b = manager.channels_for_server("srv-b");
        assert_eq!(srv_b.len(), 1);
    }

    // ---- ElicitationRequest tests ----

    #[test]
    fn test_elicitation_request_creation() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "confirmation": { "type": "boolean" }
            },
            "required": ["confirmation"]
        });

        let request = ElicitationRequest::new("Continue?", schema, "ch-1");
        assert_eq!(request.status, ElicitationStatus::Pending);
        assert!(request.response.is_none());
        assert_eq!(request.source_channel_id, "ch-1");
    }

    #[test]
    fn test_elicitation_respond() {
        let schema = serde_json::json!({ "type": "string" });
        let mut request = ElicitationRequest::new("Enter value:", schema, "ch-1");

        let response = serde_json::json!("hello");
        request.respond(response.clone()).unwrap();

        assert_eq!(request.status, ElicitationStatus::Responded);
        assert_eq!(request.response, Some(response));
        assert!(request.responded_at.is_some());
    }

    #[test]
    fn test_elicitation_double_respond_fails() {
        let schema = serde_json::json!({ "type": "string" });
        let mut request = ElicitationRequest::new("Enter value:", schema, "ch-1");

        request.respond(serde_json::json!("first")).unwrap();
        let result = request.respond(serde_json::json!("second"));
        assert!(result.is_err());
    }

    #[test]
    fn test_elicitation_cancel() {
        let schema = serde_json::json!({ "type": "string" });
        let mut request = ElicitationRequest::new("Enter value:", schema, "ch-1");

        request.cancel().unwrap();
        assert_eq!(request.status, ElicitationStatus::Cancelled);

        // Cannot cancel again
        let result = request.cancel();
        assert!(result.is_err());
    }

    // ---- ElicitationHandler tests ----

    #[test]
    fn test_elicitation_handler_create_and_respond() {
        let mut handler = ElicitationHandler::new();
        let schema = serde_json::json!({ "type": "boolean" });

        let request = handler.create_request("Confirm?", schema, "ch-1");
        assert_eq!(request.status, ElicitationStatus::Pending);
        assert_eq!(handler.pending_count(), 1);

        let response = handler
            .respond(&request.id, serde_json::json!(true))
            .unwrap();
        assert_eq!(response.status, ElicitationStatus::Responded);
        assert_eq!(handler.pending_count(), 0);
    }

    #[test]
    fn test_elicitation_handler_pending_requests() {
        let mut handler = ElicitationHandler::new();
        let schema = serde_json::json!({ "type": "string" });

        handler.create_request("Q1", schema.clone(), "ch-1");
        handler.create_request("Q2", schema.clone(), "ch-2");
        handler.create_request("Q3", schema, "ch-1");

        assert_eq!(handler.pending_count(), 3);

        let for_ch1 = handler.requests_for_channel("ch-1");
        assert_eq!(for_ch1.len(), 2);

        let for_ch2 = handler.requests_for_channel("ch-2");
        assert_eq!(for_ch2.len(), 1);
    }

    // ---- McpServerRegistry tests ----

    #[test]
    fn test_registry_register_and_get() {
        let mut registry = McpServerRegistry::new();
        let config = McpServerConfig::new_stdio("test", "node");

        registry.register(config).unwrap();
        assert!(registry.contains("test"));

        let fetched = registry.get("test").unwrap();
        assert_eq!(fetched.name, "test");
    }

    #[test]
    fn test_registry_duplicate_registration() {
        let mut registry = McpServerRegistry::new();
        registry
            .register(McpServerConfig::new_stdio("dup", "node"))
            .unwrap();

        let result = registry.register(McpServerConfig::new_stdio("dup", "python"));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            McpAdvancedError::ServerAlreadyRegistered(_)
        ));
    }

    #[test]
    fn test_content_signature_stdio() {
        let mut config = McpServerConfig::new_stdio("test", "npx");
        config.args = vec!["-y".to_string(), "@modelcontextprotocol/server".to_string()];
        assert_eq!(config.content_signature(), "stdio:npx -y @modelcontextprotocol/server");
    }

    #[test]
    fn test_content_signature_http() {
        let config = McpServerConfig::new_http("test", "https://api.example.com/mcp");
        assert_eq!(config.content_signature(), "http:https://api.example.com/mcp");
    }

    #[test]
    fn test_content_based_dedup() {
        let mut registry = McpServerRegistry::new();

        // Register a manual config
        let mut manual = McpServerConfig::new_stdio("manual-fetch", "npx");
        manual.args = vec!["-y".to_string(), "@modelcontextprotocol/server-fetch".to_string()];
        registry.register(manual).unwrap();
        assert_eq!(registry.count(), 1);

        // Plugin provides same server under different name — should be silently skipped
        let mut plugin = McpServerConfig::new_stdio("plugin-fetch", "npx");
        plugin.args = vec!["-y".to_string(), "@modelcontextprotocol/server-fetch".to_string()];
        let result = registry.register(plugin);
        assert!(result.is_ok()); // not an error, just skipped
        assert_eq!(registry.count(), 1); // still only 1
        assert!(registry.contains("manual-fetch"));
        assert!(!registry.contains("plugin-fetch"));
    }

    #[test]
    fn test_content_dedup_different_servers_allowed() {
        let mut registry = McpServerRegistry::new();
        let mut fetch = McpServerConfig::new_stdio("fetch", "npx");
        fetch.args = vec!["-y".to_string(), "@mcp/server-fetch".to_string()];
        let mut memory = McpServerConfig::new_stdio("memory", "npx");
        memory.args = vec!["-y".to_string(), "@mcp/server-memory".to_string()];
        registry.register(fetch).unwrap();
        registry.register(memory).unwrap();
        assert_eq!(registry.count(), 2); // different args → both registered
    }

    #[test]
    fn test_registry_unregister() {
        let mut registry = McpServerRegistry::new();
        registry
            .register(McpServerConfig::new_stdio("rm-me", "node"))
            .unwrap();

        let removed = registry.unregister("rm-me").unwrap();
        assert_eq!(removed.name, "rm-me");
        assert!(!registry.contains("rm-me"));
    }

    #[test]
    fn test_registry_enabled_servers() {
        let mut registry = McpServerRegistry::new();

        let cfg1 = McpServerConfig::new_stdio("enabled-srv", "node");
        registry.register(cfg1).unwrap();

        let mut cfg2 = McpServerConfig::new_stdio("disabled-srv", "python");
        cfg2.enabled = false;
        registry.register(cfg2).unwrap();

        assert_eq!(registry.count(), 2);
        assert_eq!(registry.enabled_count(), 1);

        let enabled = registry.enabled_servers();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].name, "enabled-srv");
    }

    #[test]
    fn test_registry_filter_by_transport() {
        let mut registry = McpServerRegistry::new();
        registry.register(McpServerConfig::new_stdio("s1", "node")).unwrap();
        registry.register(McpServerConfig::new_stdio("s2", "python")).unwrap();
        registry.register(McpServerConfig::new_http("s3", "https://example.com/mcp")).unwrap();

        let stdio = registry.servers_by_transport(&TransportType::Stdio);
        assert_eq!(stdio.len(), 2);

        let http = registry.servers_by_transport(&TransportType::Http);
        assert_eq!(http.len(), 1);
    }

    #[test]
    fn test_registry_json_export_import() {
        let mut registry = McpServerRegistry::new();
        registry
            .register(McpServerConfig::new_stdio("exp-srv", "node"))
            .unwrap();

        let json = registry.export_to_json();
        assert!(json.is_array());
        assert_eq!(json.as_array().unwrap().len(), 1);

        // Import into a fresh registry
        let mut registry2 = McpServerRegistry::new();
        registry2.load_from_json(json).unwrap();
        assert!(registry2.contains("exp-srv"));
    }

    #[test]
    fn test_load_from_default_paths_no_files() {
        // With no config files, should return 0 and not panic
        let mut registry = McpServerRegistry::new();
        let count = registry.load_from_default_paths();
        assert_eq!(count, 0);
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_load_from_json_array() {
        let mut registry = McpServerRegistry::new();
        let json = serde_json::json!([
            {
                "name": "test-stdio",
                "transport_type": "Stdio",
                "command": "node",
                "args": ["server.js"],
                "enabled": true
            },
            {
                "name": "test-http",
                "transport_type": "Http",
                "url": "https://mcp.example.com",
                "enabled": true
            }
        ]);
        registry.load_from_json(json).unwrap();
        assert_eq!(registry.count(), 2);
        assert!(registry.contains("test-stdio"));
        assert!(registry.contains("test-http"));
    }

    #[test]
    fn test_load_from_json_merges_existing() {
        let mut registry = McpServerRegistry::new();
        registry.register(McpServerConfig::new_stdio("existing", "python")).unwrap();

        let json = serde_json::json!([
            {
                "name": "existing",
                "transport_type": "Stdio",
                "command": "node",
                "enabled": true
            }
        ]);
        registry.load_from_json(json).unwrap();
        // Should update, not duplicate
        assert_eq!(registry.count(), 1);
        assert_eq!(registry.get("existing").unwrap().command, Some("node".to_string()));
    }

    // ---- Claude Code compatibility tests ----

    #[test]
    fn test_load_from_mcp_json_stdio() {
        let mut registry = McpServerRegistry::new();
        let json = serde_json::json!({
            "mcpServers": {
                "filesystem": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
                    "env": {}
                }
            }
        });
        let loaded = registry.load_from_mcp_json_value(&json);
        assert_eq!(loaded, 1);
        let config = registry.get("filesystem").unwrap();
        assert_eq!(config.transport_type, TransportType::Stdio);
        assert_eq!(config.command.as_deref(), Some("npx"));
        assert_eq!(config.args, vec!["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]);
    }

    #[test]
    fn test_load_from_mcp_json_http() {
        let mut registry = McpServerRegistry::new();
        let json = serde_json::json!({
            "mcpServers": {
                "remote-api": {
                    "type": "http",
                    "url": "https://mcp.example.com/api",
                    "headers": {
                        "Authorization": "Bearer token123"
                    }
                }
            }
        });
        let loaded = registry.load_from_mcp_json_value(&json);
        assert_eq!(loaded, 1);
        let config = registry.get("remote-api").unwrap();
        assert_eq!(config.transport_type, TransportType::Http);
        assert_eq!(config.url.as_deref(), Some("https://mcp.example.com/api"));
        assert_eq!(config.headers.get("Authorization").unwrap(), "Bearer token123");
    }

    #[test]
    fn test_load_from_mcp_json_sse() {
        let mut registry = McpServerRegistry::new();
        let json = serde_json::json!({
            "mcpServers": {
                "events": {
                    "type": "sse",
                    "url": "https://events.example.com/sse"
                }
            }
        });
        let loaded = registry.load_from_mcp_json_value(&json);
        assert_eq!(loaded, 1);
        let config = registry.get("events").unwrap();
        assert_eq!(config.transport_type, TransportType::Sse);
    }

    #[test]
    fn test_load_from_mcp_json_no_mcp_servers_key() {
        let mut registry = McpServerRegistry::new();
        let json = serde_json::json!({"hooks": {"PreToolUse": []}});
        let loaded = registry.load_from_mcp_json_value(&json);
        assert_eq!(loaded, 0);
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_load_from_mcp_json_invalid_server_skipped() {
        let mut registry = McpServerRegistry::new();
        let json = serde_json::json!({
            "mcpServers": {
                "valid": {
                    "command": "node",
                    "args": ["server.js"]
                },
                "invalid": {
                    "type": "http"
                    // Missing required URL
                }
            }
        });
        let loaded = registry.load_from_mcp_json_value(&json);
        assert_eq!(loaded, 1); // Only the valid one
        assert!(registry.contains("valid"));
        assert!(!registry.contains("invalid"));
    }

    #[test]
    fn test_expand_env_vars_simple() {
        unsafe { std::env::set_var("SHANNON_TEST_VAR", "hello"); }
        let result = expand_env_vars("prefix_${SHANNON_TEST_VAR}_suffix");
        assert_eq!(result, "prefix_hello_suffix");
        unsafe { std::env::remove_var("SHANNON_TEST_VAR"); }
    }

    #[test]
    fn test_expand_env_vars_with_default() {
        unsafe { std::env::remove_var("SHANNON_NONEXISTENT_VAR"); }
        let result = expand_env_vars("${SHANNON_NONEXISTENT_VAR:-fallback}");
        assert_eq!(result, "fallback");
    }

    #[test]
    fn test_expand_env_vars_no_braces() {
        let result = expand_env_vars("$NOT_EXPANDED plain text");
        assert_eq!(result, "$NOT_EXPANDED plain text");
    }

    #[test]
    fn test_expand_env_vars_multiple() {
        unsafe { std::env::set_var("SHANNON_HOST", "example.com"); }
        unsafe { std::env::set_var("SHANNON_PORT", "8080"); }
        let result = expand_env_vars("https://${SHANNON_HOST}:${SHANNON_PORT}/path");
        assert_eq!(result, "https://example.com:8080/path");
        unsafe { std::env::remove_var("SHANNON_HOST"); }
        unsafe { std::env::remove_var("SHANNON_PORT"); }
    }

    #[test]
    fn test_expand_env_vars_in_config() {
        unsafe { std::env::set_var("SHANNON_API_KEY", "secret123"); }
        let mut config = McpServerConfig {
            name: "test".to_string(),
            transport_type: TransportType::Http,
            command: None,
            args: vec![],
            env: HashMap::new(),
            url: Some("https://api.example.com/mcp".to_string()),
            headers: HashMap::from([
                ("Authorization".to_string(), "Bearer ${SHANNON_API_KEY}".to_string()),
            ]),
            enabled: true,
            timeout_secs: None,
            discovery_timeout_secs: None,
            oauth_scopes: Vec::new(),
        };
        expand_env_vars_in_config(&mut config);
        assert_eq!(config.headers.get("Authorization").unwrap(), "Bearer secret123");
        unsafe { std::env::remove_var("SHANNON_API_KEY"); }
    }

    #[test]
    fn test_load_from_settings_with_mcp_servers() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = McpServerRegistry::new();

        // Create .mcp.json in temp dir
        let mcp_json = serde_json::json!({
            "mcpServers": {
                "local-server": {
                    "command": "node",
                    "args": ["mcp-server.js"]
                }
            }
        });
        std::fs::write(
            dir.path().join(".mcp.json"),
            serde_json::to_string_pretty(&mcp_json).unwrap(),
        ).unwrap();

        let loaded = registry.load_from_default_paths_with_base(dir.path().to_path_buf());
        assert_eq!(loaded, 1);
        assert!(registry.contains("local-server"));
    }

    #[test]
    fn test_transport_type_deserialization_aliases() {
        // Old format (PascalCase)
        let old: TransportType = serde_json::from_str("\"Stdio\"").unwrap();
        assert_eq!(old, TransportType::Stdio);

        // New format (lowercase)
        let new: TransportType = serde_json::from_str("\"stdio\"").unwrap();
        assert_eq!(new, TransportType::Stdio);

        // Claude Code format
        let cc: TransportType = serde_json::from_str("\"http\"").unwrap();
        assert_eq!(cc, TransportType::Http);

        let sse: TransportType = serde_json::from_str("\"sse\"").unwrap();
        assert_eq!(sse, TransportType::Sse);
    }
}
