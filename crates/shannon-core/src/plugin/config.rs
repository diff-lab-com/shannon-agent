//! Plugin configuration

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

/// Plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginsConfig {
    /// Registry URL (GitHub-based index or custom)
    #[serde(default)]
    pub registry_url: Option<String>,

    /// Enabled plugins (by name)
    #[serde(default)]
    pub enabled: HashSet<String>,

    /// Disabled plugins (by name)
    #[serde(default)]
    pub disabled: HashSet<String>,

    /// Custom plugins directory
    #[serde(default)]
    pub plugins_dir: Option<PathBuf>,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            registry_url: Some("https://raw.githubusercontent.com/shannon-code/plugins-index/main/index.json".to_string()),
            enabled: HashSet::new(),
            disabled: HashSet::new(),
            plugins_dir: None,
        }
    }
}

impl PluginsConfig {
    /// Check if a plugin is enabled
    pub fn is_enabled(&self, name: &str) -> bool {
        !self.disabled.contains(name) &&
            (self.enabled.is_empty() || self.enabled.contains(name))
    }

    /// Enable a plugin
    pub fn enable(&mut self, name: String) {
        self.disabled.remove(&name);
        self.enabled.insert(name);
    }

    /// Disable a plugin
    pub fn disable(&mut self, name: String) {
        self.enabled.remove(&name);
        self.disabled.insert(name);
    }

    /// Get the effective state of a plugin
    pub fn get_state(&self, name: &str) -> PluginState {
        if self.disabled.contains(name) {
            PluginState::Disabled
        } else if self.enabled.contains(name) || self.enabled.is_empty() {
            PluginState::Enabled
        } else {
            PluginState::Disabled
        }
    }
}

/// Plugin state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginState {
    /// Plugin is enabled
    Enabled,
    /// Plugin is disabled
    Disabled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PluginsConfig::default();
        assert!(config.registry_url.is_some());
        assert!(config.is_enabled("any-plugin"));
    }

    #[test]
    fn test_enable_disable() {
        let mut config = PluginsConfig::default();
        config.disable("test".to_string());
        assert!(!config.is_enabled("test"));
        assert_eq!(config.get_state("test"), PluginState::Disabled);

        config.enable("test".to_string());
        assert!(config.is_enabled("test"));
        assert_eq!(config.get_state("test"), PluginState::Enabled);
    }

    #[test]
    fn test_whitelist_mode() {
        let mut config = PluginsConfig::default();
        config.enabled.insert("allowed".to_string());

        assert!(config.is_enabled("allowed"));
        assert!(!config.is_enabled("other"));
    }
}
