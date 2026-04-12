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
//!     enabled: true,
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
    Stdio,
    /// HTTP-based transport (SSE or WebSocket)
    Http,
}

impl std::fmt::Display for TransportType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportType::Stdio => write!(f, "stdio"),
            TransportType::Http => write!(f, "http"),
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

    /// Transport type (stdio or http)
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

    /// Whether the server is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
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
            enabled: true,
        }
    }

    /// Create a new HTTP-based server configuration.
    pub fn new_http(name: &str) -> Self {
        Self {
            name: name.to_string(),
            transport_type: TransportType::Http,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            enabled: true,
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

        if let Some(ref cmd) = self.command {
            if cmd.is_empty() {
                return Err(McpAdvancedError::InvalidConfig(
                    "Command must not be empty".to_string(),
                ));
            }
        }

        Ok(())
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
        Ok(self.channels.get(&id).unwrap())
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
        self.requests.get(&id).unwrap().clone()
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
}

impl McpServerRegistry {
    /// Create a new server registry.
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
        }
    }

    /// Register a new MCP server configuration.
    pub fn register(&mut self, config: McpServerConfig) -> Result<(), McpAdvancedError> {
        config.validate()?;
        if self.servers.contains_key(&config.name) {
            return Err(McpAdvancedError::ServerAlreadyRegistered(
                config.name.clone(),
            ));
        }
        self.servers.insert(config.name.clone(), config);
        Ok(())
    }

    /// Unregister an MCP server by name.
    pub fn unregister(&mut self, name: &str) -> Result<McpServerConfig, McpAdvancedError> {
        self.servers
            .remove(name)
            .ok_or_else(|| McpAdvancedError::ServerNotFound(name.to_string()))
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

    /// Export all server configurations as a JSON array.
    pub fn export_to_json(&self) -> serde_json::Value {
        let configs: Vec<&McpServerConfig> = self.list_servers();
        serde_json::to_value(configs).unwrap_or(serde_json::Value::Array(vec![]))
    }

    /// Load server configurations from default file paths.
    ///
    /// Searches (in order, later wins):
    ///   1. `~/.shannon/mcp_servers.json` — global user config
    ///   2. `.shannon/mcp_servers.json` — project-local config
    ///
    /// Returns the number of servers loaded. Malformed entries are logged and
    /// skipped so the application doesn't crash.
    pub fn load_from_default_paths(&mut self) -> usize {
        let mut count = 0usize;

        let paths: Vec<std::path::PathBuf> = {
            let mut p = Vec::new();
            // Global: ~/.shannon/mcp_servers.json
            if let Some(home) = dirs::home_dir() {
                p.push(home.join(".shannon").join("mcp_servers.json"));
            }
            // Project-local: .shannon/mcp_servers.json
            p.push(std::path::PathBuf::from(".shannon/mcp_servers.json"));
            p
        };

        for path in &paths {
            if !path.exists() {
                continue;
            }
            match std::fs::read_to_string(path) {
                Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(json) => {
                        let before = self.count();
                        if let Err(e) = self.load_from_json(json) {
                            tracing::warn!("MCP config error in {:?}: {}", path, e);
                            continue;
                        }
                        let loaded = self.count().saturating_sub(before);
                        if loaded > 0 {
                            tracing::info!(
                                "Loaded {} MCP server(s) from {:?}",
                                loaded,
                                path
                            );
                            count += loaded;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Invalid JSON in {:?}: {}", path, e);
                    }
                },
                Err(e) => {
                    tracing::warn!("Cannot read {:?}: {}", path, e);
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
        let config = McpServerConfig::new_http("remote-server");
        assert_eq!(config.name, "remote-server");
        assert_eq!(config.transport_type, TransportType::Http);
        assert!(config.command.is_none());
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
            enabled: true,
        };
        assert!(config.validate().is_err());

        // Empty command
        config = McpServerConfig {
            name: "empty-cmd".to_string(),
            transport_type: TransportType::Stdio,
            command: Some(String::new()),
            args: vec![],
            env: HashMap::new(),
            enabled: true,
        };
        assert!(config.validate().is_err());

        // HTTP without command is fine
        let config = McpServerConfig::new_http("http-ok");
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
        registry.register(McpServerConfig::new_http("s3")).unwrap();

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
}
