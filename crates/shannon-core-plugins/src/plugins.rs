//! Plugin system

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Plugin trait
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Get plugin name
    fn name(&self) -> &str;

    /// Get plugin version
    fn version(&self) -> &str;

    /// Get plugin description
    fn description(&self) -> &str;

    /// Initialize the plugin
    async fn initialize(&mut self) -> Result<(), PluginError>;

    /// Shutdown the plugin
    async fn shutdown(&mut self) -> Result<(), PluginError>;

    /// Execute a command
    async fn execute(&mut self, command: &str, args: serde_json::Value) -> Result<serde_json::Value, PluginError>;

    /// Get plugin health status
    async fn health_check(&self) -> Result<PluginHealth, PluginError>;
}

/// Plugin health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHealth {
    pub status: PluginStatus,
    pub message: String,
    pub last_check: chrono::DateTime<chrono::Utc>,
}

/// Plugin status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

/// Plugin metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub id: Uuid,
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
}

/// Plugin registry
pub struct PluginRegistry {
    plugins: HashMap<String, Box<dyn Plugin>>,
    metadata: HashMap<Uuid, PluginMetadata>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            metadata: HashMap::new(),
        }
    }

    /// Register a plugin
    pub fn register(&mut self, plugin: Box<dyn Plugin>) -> Result<Uuid, PluginError> {
        let name = plugin.name().to_string();
        let id = Uuid::new_v4();

        let metadata = PluginMetadata {
            id,
            name: name.clone(),
            version: plugin.version().to_string(),
            description: plugin.description().to_string(),
            enabled: true,
            created_at: chrono::Utc::now(),
            last_used: None,
        };

        self.plugins.insert(name.clone(), plugin);
        self.metadata.insert(id, metadata);

        Ok(id)
    }

    /// Get plugin by name
    pub fn get(&self, name: &str) -> Option<&Box<dyn Plugin>> {
        self.plugins.get(name)
    }

    /// Get plugin mutably
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Box<dyn Plugin>> {
        self.plugins.get_mut(name)
    }

    /// List all plugins
    pub fn list(&self) -> Vec<&PluginMetadata> {
        self.metadata.values().collect()
    }

    /// Enable/disable a plugin
    pub fn set_enabled(&mut self, id: &Uuid, enabled: bool) -> Result<(), PluginError> {
        if let Some(metadata) = self.metadata.get_mut(id) {
            metadata.enabled = enabled;
            Ok(())
        } else {
            Err(PluginError::NotFound(*id))
        }
    }

    /// Get plugin metadata by ID
    pub fn get_by_id(&self, id: &Uuid) -> Option<&PluginMetadata> {
        self.metadata.get(id)
    }
}

/// Plugin manager
pub struct PluginManager {
    registry: PluginRegistry,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            registry: PluginRegistry::new(),
        }
    }

    /// Register a plugin
    pub async fn register_plugin(&mut self, plugin: Box<dyn Plugin>) -> Result<Uuid, PluginError> {
        let name = plugin.name().to_string();
        let id = self.registry.register(plugin)?;

        // Initialize the plugin by name
        if let Some(p) = self.registry.get_mut(&name) {
            p.initialize().await?;
        }

        Ok(id)
    }

    /// Execute a plugin command
    pub async fn execute(&mut self, plugin_name: &str, command: &str, args: serde_json::Value) -> Result<serde_json::Value, PluginError> {
        let plugin = self.registry.get_mut(plugin_name)
            .ok_or_else(|| PluginError::NotFoundByName(plugin_name.to_string()))?;

        let result = plugin.execute(command, args).await?;

        // Update last used time
        for metadata in self.registry.metadata.values_mut() {
            if metadata.name == plugin_name {
                metadata.last_used = Some(chrono::Utc::now());
                break;
            }
        }

        Ok(result)
    }

    /// Get plugin health
    pub async fn health_check(&self, plugin_name: &str) -> Result<PluginHealth, PluginError> {
        let plugin = self.registry.get(plugin_name)
            .ok_or_else(|| PluginError::NotFoundByName(plugin_name.to_string()))?;

        plugin.health_check().await
    }

    /// Shutdown all plugins
    pub async fn shutdown_all(&mut self) -> Result<(), PluginError> {
        for (name, plugin) in &mut self.registry.plugins {
            tracing::info!("Shutting down plugin: {}", name);
            let _ = plugin.shutdown().await;
        }

        Ok(())
    }

    /// Get registry reference
    pub fn registry(&self) -> &PluginRegistry {
        &self.registry
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Plugin errors
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Plugin not found: {0}")]
    NotFound(Uuid),

    #[error("Plugin not found by name: {0}")]
    NotFoundByName(String),

    #[error("Initialization failed: {0}")]
    InitializationFailed(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Plugin error: {0}")]
    PluginError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin {
        name: String,
    }

    #[async_trait]
    impl Plugin for TestPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            "1.0.0"
        }

        fn description(&self) -> &str {
            "Test plugin"
        }

        async fn initialize(&mut self) -> Result<(), PluginError> {
            Ok(())
        }

        async fn shutdown(&mut self) -> Result<(), PluginError> {
            Ok(())
        }

        async fn execute(&mut self, command: &str, _args: serde_json::Value) -> Result<serde_json::Value, PluginError> {
            Ok(serde_json::json!({"result": format!("Executed: {}", command)}))
        }

        async fn health_check(&self) -> Result<PluginHealth, PluginError> {
            Ok(PluginHealth {
                status: PluginStatus::Healthy,
                message: "OK".to_string(),
                last_check: chrono::Utc::now(),
            })
        }
    }

    #[tokio::test]
    async fn test_plugin_registration() {
        let mut manager = PluginManager::new();
        let plugin = TestPlugin { name: "test".to_string() };

        let id = manager.register_plugin(Box::new(plugin)).await.unwrap();
        assert_ne!(id, Uuid::nil());
    }

    #[tokio::test]
    async fn test_plugin_execution() {
        let mut manager = PluginManager::new();
        let plugin = TestPlugin { name: "test".to_string() };

        manager.register_plugin(Box::new(plugin)).await.unwrap();

        let result = manager.execute("test", "test_command", serde_json::json!({})).await.unwrap();
        assert_eq!(result["result"], "Executed: test_command");
    }
}
