//! # Plugin Framework
//!
//! Discovers and loads plugins from well-known directories.
//! Plugins provide additional tools, skills, and MCP server configurations
//! via a `plugin.json` manifest file.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PluginError {
    #[error("Plugin manifest not found: {0}")]
    ManifestNotFound(String),
    #[error("Invalid plugin manifest: {0}")]
    InvalidManifest(String),
    #[error("Plugin already loaded: {0}")]
    AlreadyLoaded(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Plugin manifest (plugin.json) describing a plugin's capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Unique plugin identifier (e.g. "com.example.my-plugin").
    pub id: String,
    /// Human-readable plugin name.
    pub name: String,
    /// Plugin version (semver).
    pub version: String,
    /// Plugin description.
    #[serde(default)]
    pub description: String,
    /// Author name or organization.
    #[serde(default)]
    pub author: Option<String>,
    /// Minimum Shannon version required.
    #[serde(default)]
    pub min_shannon_version: Option<String>,
    /// MCP servers this plugin provides.
    #[serde(default)]
    pub mcp_servers: Vec<PluginMcpServer>,
    /// Skills this plugin provides.
    #[serde(default)]
    pub skills: Vec<PluginSkill>,
    /// Hooks this plugin registers.
    #[serde(default)]
    pub hooks: Vec<PluginHook>,
    /// Plugin homepage or repository URL.
    #[serde(default)]
    pub homepage: Option<String>,
    /// Plugin license identifier.
    #[serde(default)]
    pub license: Option<String>,
}

/// An MCP server provided by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMcpServer {
    /// Server name (used as key in MCP config).
    pub name: String,
    /// Command to start the server.
    pub command: String,
    /// Command arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

/// A skill provided by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSkill {
    /// Skill name (invoked via /<name>).
    pub name: String,
    /// Skill description.
    pub description: String,
    /// Prompt template file path (relative to plugin dir).
    pub prompt_file: String,
}

/// A hook registered by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHook {
    /// Event type (e.g. "pre_tool_use", "post_query").
    pub event: String,
    /// Command to run.
    pub command: String,
}

/// A loaded plugin with its resolved directory path.
#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    /// The parsed manifest.
    pub manifest: PluginManifest,
    /// Absolute path to the plugin directory.
    pub directory: PathBuf,
}

/// Registry of loaded plugins.
#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    plugins: Vec<LoadedPlugin>,
}

impl PluginRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Discover and load plugins from well-known directories.
    pub fn discover() -> Self {
        let mut registry = Self::new();
        let dirs = plugin_search_dirs();

        for dir in &dirs {
            if !dir.exists() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let plugin_dir = entry.path();
                        if let Ok(plugin) = load_plugin_from_dir(&plugin_dir) {
                            let _ = registry.register(plugin);
                        }
                    }
                }
            }
        }

        registry
    }

    /// Register a plugin. Returns Err if a plugin with the same ID is already loaded.
    pub fn register(&mut self, plugin: LoadedPlugin) -> Result<(), PluginError> {
        if self.plugins.iter().any(|p| p.manifest.id == plugin.manifest.id) {
            return Err(PluginError::AlreadyLoaded(plugin.manifest.id.clone()));
        }
        self.plugins.push(plugin);
        Ok(())
    }

    /// Get all loaded plugins.
    pub fn plugins(&self) -> &[LoadedPlugin] {
        &self.plugins
    }

    /// Find a plugin by ID.
    pub fn get(&self, id: &str) -> Option<&LoadedPlugin> {
        self.plugins.iter().find(|p| p.manifest.id == id)
    }

    /// Collect all MCP servers from all plugins.
    pub fn all_mcp_servers(&self) -> Vec<&PluginMcpServer> {
        self.plugins.iter().flat_map(|p| &p.manifest.mcp_servers).collect()
    }

    /// Collect all skills from all plugins.
    pub fn all_skills(&self) -> Vec<&PluginSkill> {
        self.plugins.iter().flat_map(|p| &p.manifest.skills).collect()
    }
}

/// Load a plugin from a directory by reading its plugin.json.
pub fn load_plugin_from_dir(dir: &Path) -> Result<LoadedPlugin, PluginError> {
    let manifest_path = dir.join("plugin.json");
    if !manifest_path.exists() {
        return Err(PluginError::ManifestNotFound(
            manifest_path.display().to_string(),
        ));
    }
    let content = std::fs::read_to_string(&manifest_path)?;
    let manifest: PluginManifest = serde_json::from_str(&content)?;

    if manifest.id.is_empty() {
        return Err(PluginError::InvalidManifest("id is required".to_string()));
    }

    Ok(LoadedPlugin {
        manifest,
        directory: dir.to_path_buf(),
    })
}

/// Return well-known directories where plugins are searched.
pub fn plugin_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // Project-local: .shannon/plugins/
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join(".shannon").join("plugins"));
    }

    // User-global: ~/.shannon/plugins/
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".shannon").join("plugins"));
    }

    // XDG data dir: ~/.local/share/shannon/plugins/
    if let Some(data) = dirs::data_local_dir() {
        dirs.push(data.join("shannon").join("plugins"));
    }

    dirs
}

// Use the `dirs` crate pattern; inline a simple fallback if not available.
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(PathBuf::from)
    }

    pub fn data_local_dir() -> Option<PathBuf> {
        home_dir().map(|h| h.join(".local").join("share"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_minimal_manifest() {
        let json = r#"{
            "id": "com.example.test",
            "name": "Test Plugin",
            "version": "1.0.0"
        }"#;
        let m: PluginManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.id, "com.example.test");
        assert_eq!(m.name, "Test Plugin");
        assert!(m.mcp_servers.is_empty());
        assert!(m.skills.is_empty());
    }

    #[test]
    fn test_parse_full_manifest() {
        let json = r#"{
            "id": "com.example.full",
            "name": "Full Plugin",
            "version": "2.0.0",
            "description": "A full plugin",
            "author": "Example Corp",
            "mcp_servers": [{"name": "my-server", "command": "node", "args": ["server.js"]}],
            "skills": [{"name": "my-skill", "description": "Does stuff", "prompt_file": "prompt.md"}]
        }"#;
        let m: PluginManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.mcp_servers.len(), 1);
        assert_eq!(m.skills.len(), 1);
        assert_eq!(m.mcp_servers[0].name, "my-server");
    }

    #[test]
    fn test_registry_register_and_dedup() {
        let mut reg = PluginRegistry::new();
        let plugin = LoadedPlugin {
            manifest: PluginManifest {
                id: "test".to_string(),
                name: "Test".to_string(),
                version: "1.0.0".to_string(),
                description: String::new(),
                author: None,
                min_shannon_version: None,
                mcp_servers: vec![],
                skills: vec![],
                hooks: vec![],
                homepage: None,
                license: None,
            },
            directory: PathBuf::from("/tmp/test-plugin"),
        };
        assert!(reg.register(plugin.clone()).is_ok());
        assert!(reg.register(plugin).is_err()); // duplicate
        assert_eq!(reg.plugins().len(), 1);
    }

    #[test]
    fn test_load_from_dir_missing_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let result = load_plugin_from_dir(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_load_from_dir_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let json = r#"{"id":"test","name":"Test","version":"1.0.0"}"#;
        fs::write(tmp.path().join("plugin.json"), json).unwrap();
        let plugin = load_plugin_from_dir(tmp.path()).unwrap();
        assert_eq!(plugin.manifest.id, "test");
    }
}
