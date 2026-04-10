//! Bridge service for external integrations

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Bridge configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    pub id: Uuid,
    pub name: String,
    pub bridge_type: BridgeType,
    pub endpoint: String,
    pub auth_token: Option<String>,
    pub enabled: bool,
}

/// Bridge type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BridgeType {
    Webhook,
    WebSocket,
    Http,
}

/// Bridge message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMessage {
    pub id: Uuid,
    pub source: String,
    pub target: String,
    pub payload: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Bridge service
pub struct BridgeService {
    bridges: Arc<RwLock<HashMap<Uuid, BridgeConfig>>>,
    name_index: Arc<RwLock<HashMap<String, Uuid>>>,
    message_queue: Arc<RwLock<Vec<BridgeMessage>>>,
}

impl BridgeService {
    pub fn new() -> Self {
        Self {
            bridges: Arc::new(RwLock::new(HashMap::new())),
            name_index: Arc::new(RwLock::new(HashMap::new())),
            message_queue: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Register a bridge
    pub async fn register_bridge(&self, config: BridgeConfig) -> Result<Uuid, BridgeError> {
        let id = config.id;

        // Check if bridge with same name exists
        {
            let name_idx = self.name_index.read().await;
            if let Some(existing_id) = name_idx.get(&config.name) {
                if existing_id != &id {
                    return Err(BridgeError::BridgeExists(config.name));
                }
            }
        }

        // Add bridge
        {
            let mut bridges = self.bridges.write().await;
            let mut name_idx = self.name_index.write().await;

            bridges.insert(id, config.clone());
            name_idx.insert(config.name.clone(), id);
        }

        Ok(id)
    }

    /// Get bridge by name
    pub async fn get_bridge(&self, name: &str) -> Option<BridgeConfig> {
        let name_idx = self.name_index.read().await;
        if let Some(id) = name_idx.get(name) {
            let bridges = self.bridges.read().await;
            bridges.get(id).cloned()
        } else {
            None
        }
    }

    /// Send message through a bridge
    pub async fn send_message(&self, bridge_name: &str, target: String, payload: serde_json::Value) -> Result<Uuid, BridgeError> {
        // Check if bridge exists and is enabled
        let bridge = self.get_bridge(bridge_name).await
            .ok_or_else(|| BridgeError::NotFound(bridge_name.to_string()))?;

        if !bridge.enabled {
            return Err(BridgeError::Disabled(bridge_name.to_string()));
        }

        let message = BridgeMessage {
            id: Uuid::new_v4(),
            source: bridge_name.to_string(),
            target,
            payload,
            timestamp: chrono::Utc::now(),
        };

        // Queue message
        {
            let mut queue = self.message_queue.write().await;
            queue.push(message.clone());
        }

        // TODO: Actually send the message via the bridge protocol
        // This would involve HTTP/websocket calls based on bridge_type

        Ok(message.id)
    }

    /// Get queued messages
    pub async fn get_queued_messages(&self) -> Vec<BridgeMessage> {
        let queue = self.message_queue.read().await;
        queue.clone()
    }

    /// Clear message queue
    pub async fn clear_queue(&self) -> Vec<BridgeMessage> {
        let mut queue = self.message_queue.write().await;
        std::mem::take(&mut *queue)
    }

    /// Enable/disable a bridge
    pub async fn set_bridge_enabled(&self, name: &str, enabled: bool) -> Result<(), BridgeError> {
        let mut bridges = self.bridges.write().await;
        let name_idx = self.name_index.read().await;

        if let Some(id) = name_idx.get(name) {
            if let Some(bridge) = bridges.get_mut(id) {
                bridge.enabled = enabled;
                return Ok(());
            }
        }

        Err(BridgeError::NotFound(name.to_string()))
    }

    /// List all bridges
    pub async fn list_bridges(&self) -> Vec<BridgeConfig> {
        let bridges = self.bridges.read().await;
        bridges.values().cloned().collect()
    }

    /// Remove a bridge
    pub async fn remove_bridge(&self, name: &str) -> Result<(), BridgeError> {
        let mut bridges = self.bridges.write().await;
        let mut name_idx = self.name_index.write().await;

        if let Some(id) = name_idx.remove(name) {
            if bridges.remove(&id).is_some() {
                return Ok(());
            }
        }

        Err(BridgeError::NotFound(name.to_string()))
    }
}

impl Default for BridgeService {
    fn default() -> Self {
        Self::new()
    }
}

/// Bridge errors
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("Bridge not found: {0}")]
    NotFound(String),

    #[error("Bridge already exists: {0}")]
    BridgeExists(String),

    #[error("Bridge is disabled: {0}")]
    Disabled(String),

    #[error("Send failed: {0}")]
    SendFailed(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Authentication failed")]
    AuthenticationFailed,
}
