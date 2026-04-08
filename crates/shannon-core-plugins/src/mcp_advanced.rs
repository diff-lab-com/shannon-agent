//! MCP advanced channel management

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// MCP channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpChannel {
    pub id: Uuid,
    pub name: String,
    pub connection_type: McpConnectionType,
    pub status: McpChannelStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

/// MCP connection type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpConnectionType {
    Stdio,
    Sse,
    WebSocket,
}

/// MCP channel status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpChannelStatus {
    Connecting,
    Connected,
    Disconnected,
    Error,
}

/// MCP channel manager
pub struct McpChannelManager {
    channels: Arc<RwLock<HashMap<Uuid, McpChannel>>>,
    name_index: Arc<RwLock<HashMap<String, Uuid>>>,
}

impl McpChannelManager {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            name_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new channel
    pub async fn create_channel(&self, name: String, connection_type: McpConnectionType) -> Result<Uuid, McpError> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let channel = McpChannel {
            id,
            name: name.clone(),
            connection_type,
            status: McpChannelStatus::Connecting,
            created_at: now,
            last_activity: now,
        };

        // Check if channel with same name exists
        {
            let name_idx = self.name_index.read().await;
            if name_idx.contains_key(&name) {
                return Err(McpError::ChannelExists(name));
            }
        }

        // Add channel
        {
            let mut channels = self.channels.write().await;
            let mut name_idx = self.name_index.write().await;

            channels.insert(id, channel);
            name_idx.insert(name, id);
        }

        Ok(id)
    }

    /// Get channel by ID
    pub async fn get_channel(&self, id: &Uuid) -> Option<McpChannel> {
        let channels = self.channels.read().await;
        channels.get(id).cloned()
    }

    /// Get channel by name
    pub async fn get_channel_by_name(&self, name: &str) -> Option<McpChannel> {
        let name_idx = self.name_index.read().await;
        if let Some(id) = name_idx.get(name) {
            self.get_channel(id).await
        } else {
            None
        }
    }

    /// Update channel status
    pub async fn update_status(&self, id: &Uuid, status: McpChannelStatus) -> Result<(), McpError> {
        let mut channels = self.channels.write().await;

        if let Some(channel) = channels.get_mut(id) {
            channel.status = status;
            channel.last_activity = chrono::Utc::now();
            Ok(())
        } else {
            Err(McpError::NotFound(*id))
        }
    }

    /// Remove channel
    pub async fn remove_channel(&self, id: &Uuid) -> Result<(), McpError> {
        let mut channels = self.channels.write().await;
        let mut name_idx = self.name_index.write().await;

        if let Some(channel) = channels.remove(id) {
            name_idx.remove(&channel.name);
            Ok(())
        } else {
            Err(McpError::NotFound(*id))
        }
    }

    /// List all channels
    pub async fn list_channels(&self) -> Vec<McpChannel> {
        let channels = self.channels.read().await;
        channels.values().cloned().collect()
    }

    /// Get active channels
    pub async fn active_channels(&self) -> Vec<McpChannel> {
        let channels = self.channels.read().await;
        channels.values()
            .filter(|c| c.status == McpChannelStatus::Connected)
            .cloned()
            .collect()
    }
}

impl Default for McpChannelManager {
    fn default() -> Self {
        Self::new()
    }
}

/// MCP errors
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("Channel not found: {0}")]
    NotFound(Uuid),

    #[error("Channel already exists: {0}")]
    ChannelExists(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("IO error: {0}")]
    IoError(String),
}
