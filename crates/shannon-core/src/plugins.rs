//! # Plugin System
//!
//! A plugin loading and lifecycle management system for Shannon Code.
//!
//! ## Architecture
//!
//! Plugins are discovered from two locations:
//! - **User-level**: `~/.shannon/plugins/` (user plugins)
//! - **Project-level**: `.shannon/plugins/` (project plugins)
//!
//! Each plugin is a directory containing a `plugin.json` manifest file.
//!
//! ## Plugin Lifecycle
//!
//! 1. **Discovery**: Scan plugin directories for `plugin.json` files
//! 2. **Loading**: Parse manifest, check version compatibility
//! 3. **Enabling**: Activate the plugin, register its tools and hooks
//! 4. **Disabling**: Deactivate the plugin, unregister its extensions
//! 5. **Unloading**: Remove the plugin from memory entirely
//!
//! ## Example plugin.json
//!
//! ```json
//! {
//!   "name": "my-plugin",
//!   "version": "1.0.0",
//!   "description": "A sample plugin",
//!   "author": "Plugin Author",
//!   "tools": [
//!     {
//!       "name": "my_tool",
//!       "description": "Does something useful",
//!       "input_schema": { "type": "object" },
//!       "command": "python3 /path/to/tool.py",
//!       "is_read_only": true
//!     }
//!   ],
//!   "hooks": [
//!     {
//!       "event": "PreToolUse",
//!       "matcher": "Bash",
//!       "command": "echo 'About to run bash'",
//!       "timeout_secs": 5,
//!       "blocking": true
//!     }
//!   ],
//!   "commands": [
//!     {
//!       "name": "my-command",
//!       "description": "A custom command",
//!       "prompt_template": "Do something specific"
//!     }
//!   ]
//! }
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during plugin operations
#[derive(Error, Debug)]
pub enum PluginError {
    #[error("Plugin not found: {0}")]
    NotFound(String),

    #[error("Plugin already loaded: {0}")]
    AlreadyLoaded(String),

    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("Version mismatch: requires {required}, have {current}")]
    VersionMismatch { required: String, current: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Home directory not found")]
    HomeNotFound,
}

/// Plugin manifest (plugin.json)
///
/// This is the main configuration file for a plugin, defining its
/// metadata, tools, hooks, commands, and settings schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginManifest {
    /// Plugin name (must be unique)
    pub name: String,
    /// Semantic version (e.g., "1.0.0")
    pub version: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Plugin author
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Minimum Shannon Code version required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_version: Option<String>,
    /// Tools provided by this plugin
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
    /// Hooks provided by this plugin
    #[serde(default)]
    pub hooks: Vec<HookDefinition>,
    /// Commands provided by this plugin
    #[serde(default)]
    pub commands: Vec<CommandDefinition>,
    /// Settings schema for plugin configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings_schema: Option<Value>,
}

impl PluginManifest {
    /// Validate the manifest for required fields and logical constraints
    pub fn validate(&self) -> Result<(), PluginError> {
        if self.name.trim().is_empty() {
            return Err(PluginError::InvalidManifest(
                "Plugin name must not be empty".to_string(),
            ));
        }

        if self.version.trim().is_empty() {
            return Err(PluginError::InvalidManifest(
                "Plugin version must not be empty".to_string(),
            ));
        }

        // Validate tool names are unique within the manifest
        let mut tool_names = std::collections::HashSet::new();
        for tool in &self.tools {
            if !tool_names.insert(&tool.name) {
                return Err(PluginError::InvalidManifest(format!(
                    "Duplicate tool name '{}' in plugin '{}'",
                    tool.name, self.name
                )));
            }
        }

        // Validate hook event names
        for hook in &self.hooks {
            if hook.event.trim().is_empty() {
                return Err(PluginError::InvalidManifest(format!(
                    "Hook event must not be empty in plugin '{}'",
                    self.name
                )));
            }
            if hook.command.trim().is_empty() {
                return Err(PluginError::InvalidManifest(format!(
                    "Hook command must not be empty in plugin '{}'",
                    self.name
                )));
            }
        }

        // Validate command names are unique within the manifest
        let mut cmd_names = std::collections::HashSet::new();
        for cmd in &self.commands {
            if !cmd_names.insert(&cmd.name) {
                return Err(PluginError::InvalidManifest(format!(
                    "Duplicate command name '{}' in plugin '{}'",
                    cmd.name, self.name
                )));
            }
        }

        Ok(())
    }

    /// Parse a plugin manifest from a JSON string
    pub fn from_json(json: &str) -> Result<Self, PluginError> {
        let manifest: PluginManifest = serde_json::from_str(json)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Load a plugin manifest from a file path
    pub fn load_from_file(path: &Path) -> Result<Self, PluginError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_json(&content)
    }

    /// Serialize the manifest to a JSON string
    pub fn to_json(&self) -> Result<String, PluginError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Check if this plugin is compatible with the given Shannon Code version.
    ///
    /// Uses simple semver-like comparison: the major version must match,
    /// and the plugin's required minor version must be <= the current minor version.
    pub fn check_version_compatibility(&self, current_version: &str) -> Result<(), PluginError> {
        if let Some(ref min_ver) = self.min_version {
            if !version_compatible(min_ver, current_version) {
                return Err(PluginError::VersionMismatch {
                    required: min_ver.clone(),
                    current: current_version.to_string(),
                });
            }
        }
        Ok(())
    }
}

/// Tool definition from a plugin
///
/// Each tool is executed as an external command. The tool receives
/// its input as JSON on stdin and must output its result as JSON on stdout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    /// Unique tool name
    pub name: String,
    /// Human-readable description for the LLM
    pub description: String,
    /// JSON Schema describing the tool's input parameters
    pub input_schema: Value,
    /// Shell command to execute: "command arg1 arg2"
    pub command: String,
    /// Whether this tool can modify files (false = read-only)
    #[serde(default)]
    pub is_read_only: bool,
}

/// Hook definition from a plugin
///
/// Plugin hooks are similar to user-defined hooks but are managed
/// through the plugin system. They are automatically registered
/// when the plugin is enabled and unregistered when disabled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookDefinition {
    /// Event type to hook into (e.g., "PreToolUse", "PostToolUse")
    pub event: String,
    /// Matcher pattern for the hook (tool name pattern or "*")
    pub matcher: String,
    /// Shell command to execute when the hook fires
    pub command: String,
    /// Timeout in seconds (default: 30)
    #[serde(default = "default_hook_timeout")]
    pub timeout_secs: u64,
    /// Whether the hook blocks the operation until completion
    #[serde(default = "default_hook_blocking")]
    pub blocking: bool,
}

fn default_hook_timeout() -> u64 {
    30
}

fn default_hook_blocking() -> bool {
    true
}

/// Command definition from a plugin
///
/// Custom slash commands that plugins can register. When invoked,
/// they expand the prompt template and send it to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommandDefinition {
    /// Command name (used as /command-name)
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Prompt template that gets expanded when the command is invoked
    pub prompt_template: String,
}

/// Plugin state
#[derive(Debug, Clone, PartialEq)]
pub enum PluginState {
    /// Loaded but not initialized
    Loaded,
    /// Initialized and active
    Active,
    /// Disabled by user
    Disabled,
    /// Failed to load
    Failed(String),
}

impl std::fmt::Display for PluginState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Loaded => write!(f, "Loaded"),
            Self::Active => write!(f, "Active"),
            Self::Disabled => write!(f, "Disabled"),
            Self::Failed(reason) => write!(f, "Failed({})", reason),
        }
    }
}

/// A loaded plugin instance
#[derive(Debug, Clone)]
pub struct Plugin {
    /// The plugin's parsed manifest
    pub manifest: PluginManifest,
    /// Current lifecycle state
    pub state: PluginState,
    /// Filesystem path to the plugin directory
    pub path: PathBuf,
    /// Plugin-specific settings
    pub settings: Value,
}

impl Plugin {
    /// Create a new plugin instance from a manifest and path
    pub fn new(manifest: PluginManifest, path: PathBuf) -> Self {
        Self {
            manifest,
            state: PluginState::Loaded,
            path,
            settings: Value::Object(serde_json::Map::new()),
        }
    }

    /// Check if the plugin is active
    pub fn is_active(&self) -> bool {
        self.state == PluginState::Active
    }

    /// Check if the plugin is disabled
    pub fn is_disabled(&self) -> bool {
        self.state == PluginState::Disabled
    }

    /// Get the plugin's name
    pub fn name(&self) -> &str {
        &self.manifest.name
    }
}

/// Persistent plugin state stored on disk
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PluginStateFile {
    /// List of disabled plugin names
    #[serde(default)]
    pub disabled_plugins: Vec<String>,
    /// Per-plugin settings
    #[serde(default)]
    pub plugin_settings: HashMap<String, Value>,
}

impl PluginStateFile {
    /// Create a new empty state file
    pub fn new() -> Self {
        Self::default()
    }

    /// Load from a JSON string
    pub fn from_json(json: &str) -> Result<Self, PluginError> {
        Ok(serde_json::from_str(json)?)
    }

    /// Serialize to a JSON string
    pub fn to_json(&self) -> Result<String, PluginError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Check if a plugin is in the disabled list
    pub fn is_disabled(&self, name: &str) -> bool {
        self.disabled_plugins.contains(&name.to_string())
    }

    /// Add a plugin to the disabled list
    pub fn disable(&mut self, name: &str) {
        if !self.is_disabled(name) {
            self.disabled_plugins.push(name.to_string());
        }
    }

    /// Remove a plugin from the disabled list
    pub fn enable(&mut self, name: &str) {
        self.disabled_plugins.retain(|n| n != name);
    }

    /// Get settings for a specific plugin
    pub fn get_settings(&self, name: &str) -> Option<&Value> {
        self.plugin_settings.get(name)
    }

    /// Set settings for a specific plugin
    pub fn set_settings(&mut self, name: &str, settings: Value) {
        self.plugin_settings.insert(name.to_string(), settings);
    }
}

/// Plugin manager responsible for discovery, loading, and lifecycle management
pub struct PluginManager {
    /// All loaded plugins keyed by name
    plugins: HashMap<String, Plugin>,
    /// Directories to search for plugins
    plugin_dirs: Vec<PathBuf>,
    /// Path to the state file
    state_file_path: PathBuf,
    /// The current Shannon Code version for compatibility checks
    current_version: String,
}

impl std::fmt::Debug for PluginManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginManager")
            .field("plugin_count", &self.plugins.len())
            .field("plugin_dirs", &self.plugin_dirs)
            .field("state_file_path", &self.state_file_path)
            .field("current_version", &self.current_version)
            .finish()
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginManager {
    /// Create a new PluginManager with default directories
    ///
    /// Default plugin search directories:
    /// - `~/.shannon/plugins/` (user plugins)
    /// - `.shannon/plugins/` (project plugins)
    pub fn new() -> Self {
        let home_dir = dirs::home_dir().expect("Home directory should exist");

        let plugin_dirs = vec![
            home_dir.join(".shannon").join("plugins"),
            PathBuf::from(".shannon").join("plugins"),
        ];

        let state_file_path = home_dir.join(".shannon").join("plugin-state.json");

        Self {
            plugins: HashMap::new(),
            plugin_dirs,
            state_file_path,
            current_version: crate::VERSION.to_string(),
        }
    }

    /// Create a PluginManager with custom configuration (useful for testing)
    pub fn with_config(
        plugin_dirs: Vec<PathBuf>,
        state_file_path: PathBuf,
        current_version: &str,
    ) -> Self {
        Self {
            plugins: HashMap::new(),
            plugin_dirs,
            state_file_path,
            current_version: current_version.to_string(),
        }
    }

    /// Add a plugin search directory
    pub fn add_plugin_dir(&mut self, dir: PathBuf) {
        if !self.plugin_dirs.contains(&dir) {
            self.plugin_dirs.push(dir);
        }
    }

    /// Discover all plugins in search directories.
    ///
    /// Scans each plugin directory for subdirectories containing a `plugin.json` file.
    /// Returns the manifests of all discovered plugins without loading them.
    pub async fn discover_plugins(&mut self) -> Vec<PluginManifest> {
        let mut manifests = Vec::new();

        for dir in &self.plugin_dirs {
            if !dir.exists() {
                continue;
            }

            let mut entries = match tokio::fs::read_dir(dir).await {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            // Collect directory entries
            let mut dir_entries = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                dir_entries.push(entry);
            }

            // Sort for deterministic ordering
            dir_entries.sort_by_key(|e| e.file_name());

            for entry in dir_entries {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let manifest_path = path.join("plugin.json");
                if !manifest_path.exists() {
                    continue;
                }

                match PluginManifest::load_from_file(&manifest_path) {
                    Ok(manifest) => {
                        tracing::debug!(
                            "Discovered plugin '{}' v{} at {}",
                            manifest.name,
                            manifest.version,
                            path.display()
                        );
                        manifests.push(manifest);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse plugin manifest at {}: {}",
                            manifest_path.display(),
                            e
                        );
                    }
                }
            }
        }

        manifests
    }

    /// Load a specific plugin by name.
    ///
    /// Searches plugin directories for a plugin with the given name,
    /// parses its manifest, checks version compatibility, and adds
    /// it to the loaded plugins map.
    pub async fn load_plugin(&mut self, name: &str) -> Result<(), PluginError> {
        if self.plugins.contains_key(name) {
            return Err(PluginError::AlreadyLoaded(name.to_string()));
        }

        // Search for the plugin in plugin directories
        for dir in &self.plugin_dirs {
            if !dir.exists() {
                continue;
            }

            let plugin_path = dir.join(name);
            let manifest_path = plugin_path.join("plugin.json");

            if !manifest_path.exists() {
                continue;
            }

            let manifest = PluginManifest::load_from_file(&manifest_path)?;

            if manifest.name != name {
                return Err(PluginError::InvalidManifest(format!(
                    "Plugin directory name '{}' does not match manifest name '{}'",
                    name, manifest.name
                )));
            }

            // Check version compatibility
            manifest.check_version_compatibility(&self.current_version)?;

            let mut plugin = Plugin::new(manifest, plugin_path);

            // Load persisted state
            let state = self.load_state_file()?;
            if state.is_disabled(name) {
                plugin.state = PluginState::Disabled;
            } else {
                plugin.state = PluginState::Active;
            }

            // Apply persisted settings
            if let Some(settings) = state.get_settings(name) {
                plugin.settings = settings.clone();
            }

            tracing::info!(
                "Loaded plugin '{}' v{} (state: {})",
                plugin.name(),
                plugin.manifest.version,
                plugin.state
            );

            self.plugins.insert(name.to_string(), plugin);
            return Ok(());
        }

        Err(PluginError::NotFound(name.to_string()))
    }

    /// Enable a previously disabled plugin.
    ///
    /// Transitions the plugin from `Disabled` to `Active` state and
    /// persists the state change.
    pub fn enable_plugin(&mut self, name: &str) -> Result<(), PluginError> {
        let plugin = self
            .plugins
            .get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        if plugin.is_active() {
            return Ok(());
        }

        if !matches!(plugin.state, PluginState::Disabled) {
            return Err(PluginError::InvalidManifest(format!(
                "Plugin '{}' is in state '{}', cannot enable (must be Disabled)",
                name, plugin.state
            )));
        }

        plugin.state = PluginState::Active;
        tracing::info!("Enabled plugin '{}'", name);
        self.save_state()
    }

    /// Disable an active plugin.
    ///
    /// Transitions the plugin from `Active` to `Disabled` state and
    /// persists the state change.
    pub fn disable_plugin(&mut self, name: &str) -> Result<(), PluginError> {
        let plugin = self
            .plugins
            .get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        if plugin.is_disabled() {
            return Ok(());
        }

        if !plugin.is_active() {
            return Err(PluginError::InvalidManifest(format!(
                "Plugin '{}' is in state '{}', cannot disable (must be Active)",
                name, plugin.state
            )));
        }

        plugin.state = PluginState::Disabled;
        tracing::info!("Disabled plugin '{}'", name);
        self.save_state()
    }

    /// Unload a plugin entirely from memory.
    ///
    /// Removes the plugin from the manager. Does not affect persisted state.
    pub fn unload_plugin(&mut self, name: &str) -> Result<(), PluginError> {
        if self.plugins.remove(name).is_none() {
            return Err(PluginError::NotFound(name.to_string()));
        }
        tracing::info!("Unloaded plugin '{}'", name);
        Ok(())
    }

    /// Get a reference to a loaded plugin by name
    pub fn get_plugin(&self, name: &str) -> Option<&Plugin> {
        self.plugins.get(name)
    }

    /// Get a mutable reference to a loaded plugin by name
    pub fn get_plugin_mut(&mut self, name: &str) -> Option<&mut Plugin> {
        self.plugins.get_mut(name)
    }

    /// List all loaded plugins
    pub fn list_plugins(&self) -> Vec<&Plugin> {
        let mut plugins: Vec<&Plugin> = self.plugins.values().collect();
        plugins.sort_by_key(|p| p.name().to_string());
        plugins
    }

    /// Get all tools from all active plugins
    pub fn get_plugin_tools(&self) -> Vec<&ToolDefinition> {
        self.plugins
            .values()
            .filter(|p| p.is_active())
            .flat_map(|p| p.manifest.tools.iter())
            .collect()
    }

    /// Get all hooks from all active plugins
    pub fn get_plugin_hooks(&self) -> Vec<&HookDefinition> {
        self.plugins
            .values()
            .filter(|p| p.is_active())
            .flat_map(|p| p.manifest.hooks.iter())
            .collect()
    }

    /// Get all commands from all active plugins
    pub fn get_plugin_commands(&self) -> Vec<&CommandDefinition> {
        self.plugins
            .values()
            .filter(|p| p.is_active())
            .flat_map(|p| p.manifest.commands.iter())
            .collect()
    }

    /// Get tools from a specific plugin
    pub fn get_tools_for_plugin(&self, name: &str) -> Result<Vec<&ToolDefinition>, PluginError> {
        let plugin = self
            .plugins
            .get(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;
        Ok(plugin.manifest.tools.iter().collect())
    }

    /// Get hooks from a specific plugin
    pub fn get_hooks_for_plugin(&self, name: &str) -> Result<Vec<&HookDefinition>, PluginError> {
        let plugin = self
            .plugins
            .get(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;
        Ok(plugin.manifest.hooks.iter().collect())
    }

    /// Save plugin state (enabled/disabled list and settings) to disk
    pub fn save_state(&self) -> Result<(), PluginError> {
        let mut state_file = self.load_state_file()?;

        // Update disabled list from current plugin states
        state_file.disabled_plugins.clear();
        for (name, plugin) in &self.plugins {
            if plugin.is_disabled() {
                state_file.disable(name);
            }
        }

        // Update settings from current plugin settings
        for (name, plugin) in &self.plugins {
            if !plugin.settings.is_null() {
                state_file.set_settings(name, plugin.settings.clone());
            }
        }

        // Create parent directory if needed
        if let Some(parent) = self.state_file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = state_file.to_json()?;
        std::fs::write(&self.state_file_path, json)?;
        Ok(())
    }

    /// Load plugin state from disk and apply to loaded plugins
    pub fn load_state(&mut self) -> Result<(), PluginError> {
        let state = self.load_state_file()?;

        for (name, plugin) in &mut self.plugins {
            if state.is_disabled(name) {
                plugin.state = PluginState::Disabled;
            } else if plugin.state == PluginState::Loaded {
                plugin.state = PluginState::Active;
            }

            if let Some(settings) = state.get_settings(name) {
                plugin.settings = settings.clone();
            }
        }

        Ok(())
    }

    /// Load and return the state file contents without applying
    fn load_state_file(&self) -> Result<PluginStateFile, PluginError> {
        if !self.state_file_path.exists() {
            return Ok(PluginStateFile::new());
        }

        let content = std::fs::read_to_string(&self.state_file_path)?;
        let state: PluginStateFile = serde_json::from_str(&content)?;
        Ok(state)
    }

    /// Discover and load all available plugins.
    ///
    /// Convenience method that combines discovery and loading.
    /// Plugins that were previously disabled will remain disabled.
    pub async fn discover_and_load_all(&mut self) -> Result<Vec<String>, PluginError> {
        let manifests = self.discover_plugins().await;
        let mut loaded = Vec::new();

        for manifest in manifests {
            // Skip already loaded plugins
            if self.plugins.contains_key(&manifest.name) {
                continue;
            }

            // Load via the normal path (which checks version compatibility)
            match self.load_plugin(&manifest.name).await {
                Ok(()) => {
                    loaded.push(manifest.name.clone());
                }
                Err(e) => {
                    tracing::warn!("Failed to load plugin '{}': {}", manifest.name, e);
                    // Create a failed entry so the user knows about it
                    let plugin = Plugin {
                        manifest,
                        state: PluginState::Failed(e.to_string()),
                        path: PathBuf::new(),
                        settings: Value::Null,
                    };
                    self.plugins.insert(plugin.name().to_string(), plugin);
                }
            }
        }

        Ok(loaded)
    }

    /// Get the list of plugin search directories
    pub fn plugin_dirs(&self) -> &[PathBuf] {
        &self.plugin_dirs
    }

    /// Get the path to the state file
    pub fn state_file_path(&self) -> &Path {
        &self.state_file_path
    }

    /// Get the current Shannon Code version used for compatibility checks
    pub fn current_version(&self) -> &str {
        &self.current_version
    }
}

/// Check if a current version satisfies a minimum version requirement.
///
/// Uses simple semver-like comparison:
/// - Compares major version (first number before '.')
/// - If major matches, compares minor version
/// - Patch version is not considered for compatibility
fn version_compatible(min_required: &str, current: &str) -> bool {
    let min_parts: Vec<&str> = min_required.split('.').collect();
    let cur_parts: Vec<&str> = current.split('.').collect();

    if min_parts.is_empty() || cur_parts.is_empty() {
        return false;
    }

    // Parse major versions
    let min_major: u32 = min_parts[0].parse().unwrap_or(0);
    let cur_major: u32 = cur_parts[0].parse().unwrap_or(0);

    if cur_major > min_major {
        return true;
    }
    if cur_major < min_major {
        return false;
    }

    // Major versions match, check minor
    let min_minor: u32 = min_parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
    let cur_minor: u32 = cur_parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);

    cur_minor >= min_minor
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── Helper functions ──────────────────────────────────────────────

    fn create_test_manifest(name: &str, version: &str) -> PluginManifest {
        PluginManifest {
            name: name.to_string(),
            version: version.to_string(),
            description: Some(format!("Test plugin {}", name)),
            author: Some("Test Author".to_string()),
            min_version: None,
            tools: vec![],
            hooks: vec![],
            commands: vec![],
            settings_schema: None,
        }
    }

    fn create_test_manifest_full(name: &str) -> PluginManifest {
        PluginManifest {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: Some(format!("Full test plugin {}", name)),
            author: Some("Test Author".to_string()),
            min_version: Some("0.1.0".to_string()),
            tools: vec![ToolDefinition {
                name: format!("{}_tool", name),
                description: "A test tool".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "input": { "type": "string" }
                    }
                }),
                command: "echo hello".to_string(),
                is_read_only: true,
            }],
            hooks: vec![HookDefinition {
                event: "PreToolUse".to_string(),
                matcher: "Bash".to_string(),
                command: "echo pre-hook".to_string(),
                timeout_secs: 5,
                blocking: true,
            }],
            commands: vec![CommandDefinition {
                name: format!("{}_cmd", name),
                description: "A test command".to_string(),
                prompt_template: "Do something".to_string(),
            }],
            settings_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "api_key": { "type": "string" }
                }
            })),
        }
    }

    fn create_plugin_dir(temp_dir: &TempDir, name: &str, manifest: &PluginManifest) -> PathBuf {
        let plugins_root = temp_dir.path().join("plugins");
        let plugin_dir = plugins_root.join(name);
        fs::create_dir_all(&plugin_dir).unwrap();
        let manifest_path = plugin_dir.join("plugin.json");
        let json = serde_json::to_string_pretty(manifest).unwrap();
        fs::write(&manifest_path, json).unwrap();
        plugin_dir
    }

    fn create_temp_manager() -> (PluginManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("plugins");
        let state_path = temp_dir.path().join("plugin-state.json");

        let manager = PluginManager::with_config(
            vec![plugin_dir],
            state_path,
            "0.1.0",
        );

        (manager, temp_dir)
    }

    // ── PluginManifest tests ──────────────────────────────────────────

    #[test]
    fn test_manifest_validation_valid() {
        let manifest = create_test_manifest("test-plugin", "1.0.0");
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn test_manifest_validation_empty_name() {
        let manifest = PluginManifest {
            name: String::new(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            min_version: None,
            tools: vec![],
            hooks: vec![],
            commands: vec![],
            settings_schema: None,
        };
        assert!(matches!(
            manifest.validate().unwrap_err(),
            PluginError::InvalidManifest(msg) if msg.contains("name")
        ));
    }

    #[test]
    fn test_manifest_validation_empty_version() {
        let manifest = PluginManifest {
            name: "test".to_string(),
            version: String::new(),
            description: None,
            author: None,
            min_version: None,
            tools: vec![],
            hooks: vec![],
            commands: vec![],
            settings_schema: None,
        };
        assert!(matches!(
            manifest.validate().unwrap_err(),
            PluginError::InvalidManifest(msg) if msg.contains("version")
        ));
    }

    #[test]
    fn test_manifest_validation_duplicate_tools() {
        let manifest = PluginManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            min_version: None,
            tools: vec![
                ToolDefinition {
                    name: "dup_tool".to_string(),
                    description: "First".to_string(),
                    input_schema: serde_json::json!({}),
                    command: "echo 1".to_string(),
                    is_read_only: true,
                },
                ToolDefinition {
                    name: "dup_tool".to_string(),
                    description: "Second".to_string(),
                    input_schema: serde_json::json!({}),
                    command: "echo 2".to_string(),
                    is_read_only: false,
                },
            ],
            hooks: vec![],
            commands: vec![],
            settings_schema: None,
        };
        assert!(matches!(
            manifest.validate().unwrap_err(),
            PluginError::InvalidManifest(msg) if msg.contains("Duplicate tool")
        ));
    }

    #[test]
    fn test_manifest_validation_duplicate_commands() {
        let manifest = PluginManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            min_version: None,
            tools: vec![],
            hooks: vec![],
            commands: vec![
                CommandDefinition {
                    name: "dup_cmd".to_string(),
                    description: "First".to_string(),
                    prompt_template: "First".to_string(),
                },
                CommandDefinition {
                    name: "dup_cmd".to_string(),
                    description: "Second".to_string(),
                    prompt_template: "Second".to_string(),
                },
            ],
            settings_schema: None,
        };
        assert!(matches!(
            manifest.validate().unwrap_err(),
            PluginError::InvalidManifest(msg) if msg.contains("Duplicate command")
        ));
    }

    #[test]
    fn test_manifest_validation_empty_hook_event() {
        let manifest = PluginManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            min_version: None,
            tools: vec![],
            hooks: vec![HookDefinition {
                event: String::new(),
                matcher: "*".to_string(),
                command: "echo test".to_string(),
                timeout_secs: 5,
                blocking: true,
            }],
            commands: vec![],
            settings_schema: None,
        };
        assert!(matches!(
            manifest.validate().unwrap_err(),
            PluginError::InvalidManifest(msg) if msg.contains("event")
        ));
    }

    #[test]
    fn test_manifest_from_json() {
        let json = r#"{
            "name": "test-plugin",
            "version": "1.0.0",
            "description": "A test plugin",
            "author": "Test Author",
            "tools": [],
            "hooks": [],
            "commands": []
        }"#;

        let manifest = PluginManifest::from_json(json).unwrap();
        assert_eq!(manifest.name, "test-plugin");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.description, Some("A test plugin".to_string()));
        assert_eq!(manifest.author, Some("Test Author".to_string()));
    }

    #[test]
    fn test_manifest_from_json_invalid() {
        let result = PluginManifest::from_json("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_manifest_serialization_roundtrip() {
        let manifest = create_test_manifest_full("ser-test");
        let json = manifest.to_json().unwrap();
        let parsed: PluginManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, parsed);
    }

    #[test]
    fn test_manifest_load_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let manifest = create_test_manifest("file-test", "2.0.0");
        let plugin_dir = create_plugin_dir(&temp_dir, "file-test", &manifest);

        let loaded = PluginManifest::load_from_file(&plugin_dir.join("plugin.json")).unwrap();
        assert_eq!(loaded.name, "file-test");
        assert_eq!(loaded.version, "2.0.0");
    }

    #[test]
    fn test_manifest_load_from_missing_file() {
        let result = PluginManifest::load_from_file(Path::new("/nonexistent/plugin.json"));
        assert!(result.is_err());
    }

    // ── Version compatibility tests ───────────────────────────────────

    #[test]
    fn test_version_compatible_same() {
        assert!(version_compatible("0.1.0", "0.1.0"));
    }

    #[test]
    fn test_version_compatible_newer_minor() {
        assert!(version_compatible("0.1.0", "0.2.0"));
    }

    #[test]
    fn test_version_compatible_newer_major() {
        assert!(version_compatible("0.1.0", "1.0.0"));
    }

    #[test]
    fn test_version_compatible_older_minor() {
        assert!(!version_compatible("0.2.0", "0.1.0"));
    }

    #[test]
    fn test_version_compatible_older_major() {
        assert!(!version_compatible("1.0.0", "0.9.0"));
    }

    #[test]
    fn test_version_compatible_patch_ignored() {
        // Patch version should not affect compatibility
        assert!(version_compatible("0.1.0", "0.1.5"));
        assert!(version_compatible("0.1.5", "0.1.0"));
    }

    #[test]
    fn test_manifest_check_version_compatibility_no_min() {
        let manifest = create_test_manifest("test", "1.0.0");
        assert!(manifest.check_version_compatibility("0.0.1").is_ok());
    }

    #[test]
    fn test_manifest_check_version_compatibility_pass() {
        let manifest = PluginManifest {
            min_version: Some("0.1.0".to_string()),
            ..create_test_manifest("test", "1.0.0")
        };
        assert!(manifest.check_version_compatibility("0.2.0").is_ok());
        assert!(manifest.check_version_compatibility("1.0.0").is_ok());
    }

    #[test]
    fn test_manifest_check_version_compatibility_fail() {
        let manifest = PluginManifest {
            min_version: Some("1.0.0".to_string()),
            ..create_test_manifest("test", "1.0.0")
        };
        let result = manifest.check_version_compatibility("0.9.0");
        assert!(matches!(result, Err(PluginError::VersionMismatch { .. })));
    }

    // ── PluginState tests ─────────────────────────────────────────────

    #[test]
    fn test_plugin_state_display() {
        assert_eq!(format!("{}", PluginState::Loaded), "Loaded");
        assert_eq!(format!("{}", PluginState::Active), "Active");
        assert_eq!(format!("{}", PluginState::Disabled), "Disabled");
        assert_eq!(format!("{}", PluginState::Failed("oops".to_string())), "Failed(oops)");
    }

    // ── Plugin tests ──────────────────────────────────────────────────

    #[test]
    fn test_plugin_new() {
        let manifest = create_test_manifest("test", "1.0.0");
        let plugin = Plugin::new(manifest, PathBuf::from("/tmp/test"));
        assert_eq!(plugin.name(), "test");
        assert_eq!(plugin.state, PluginState::Loaded);
        assert!(!plugin.is_active());
        assert!(!plugin.is_disabled());
    }

    #[test]
    fn test_plugin_is_active() {
        let manifest = create_test_manifest("test", "1.0.0");
        let mut plugin = Plugin::new(manifest, PathBuf::from("/tmp/test"));
        assert!(!plugin.is_active());
        plugin.state = PluginState::Active;
        assert!(plugin.is_active());
    }

    #[test]
    fn test_plugin_is_disabled() {
        let manifest = create_test_manifest("test", "1.0.0");
        let mut plugin = Plugin::new(manifest, PathBuf::from("/tmp/test"));
        assert!(!plugin.is_disabled());
        plugin.state = PluginState::Disabled;
        assert!(plugin.is_disabled());
    }

    // ── PluginStateFile tests ─────────────────────────────────────────

    #[test]
    fn test_state_file_new() {
        let state = PluginStateFile::new();
        assert!(state.disabled_plugins.is_empty());
        assert!(state.plugin_settings.is_empty());
    }

    #[test]
    fn test_state_file_disable_enable() {
        let mut state = PluginStateFile::new();
        assert!(!state.is_disabled("test-plugin"));

        state.disable("test-plugin");
        assert!(state.is_disabled("test-plugin"));

        // Disabling again should not add a duplicate
        state.disable("test-plugin");
        assert_eq!(state.disabled_plugins.len(), 1);

        state.enable("test-plugin");
        assert!(!state.is_disabled("test-plugin"));
    }

    #[test]
    fn test_state_file_settings() {
        let mut state = PluginStateFile::new();
        assert!(state.get_settings("test").is_none());

        state.set_settings("test", serde_json::json!({"api_key": "secret"}));
        let settings = state.get_settings("test").unwrap();
        assert_eq!(settings["api_key"], "secret");
    }

    #[test]
    fn test_state_file_serialization_roundtrip() {
        let mut state = PluginStateFile::new();
        state.disable("plugin-a");
        state.disable("plugin-b");
        state.set_settings("plugin-a", serde_json::json!({"key": "value"}));

        let json = state.to_json().unwrap();
        let parsed = PluginStateFile::from_json(&json).unwrap();
        assert_eq!(state, parsed);
    }

    // ── PluginManager tests ───────────────────────────────────────────

    #[test]
    fn test_manager_new() {
        let manager = PluginManager::new();
        assert!(!manager.plugin_dirs().is_empty());
        assert_eq!(manager.current_version(), crate::VERSION);
    }

    #[test]
    fn test_manager_with_config() {
        let manager = PluginManager::with_config(
            vec![PathBuf::from("/test/plugins")],
            PathBuf::from("/test/state.json"),
            "2.0.0",
        );
        assert_eq!(manager.plugin_dirs().len(), 1);
        assert_eq!(manager.current_version(), "2.0.0");
    }

    #[test]
    fn test_manager_add_plugin_dir() {
        let mut manager = PluginManager::with_config(
            vec![PathBuf::from("/a")],
            PathBuf::from("/state"),
            "1.0.0",
        );
        manager.add_plugin_dir(PathBuf::from("/b"));
        assert_eq!(manager.plugin_dirs().len(), 2);

        // Adding duplicate should not increase count
        manager.add_plugin_dir(PathBuf::from("/a"));
        assert_eq!(manager.plugin_dirs().len(), 2);
    }

    #[tokio::test]
    async fn test_manager_discover_plugins() {
        let (mut manager, temp_dir) = create_temp_manager();

        // Create two plugin directories
        let manifest1 = create_test_manifest("plugin-one", "1.0.0");
        create_plugin_dir(&temp_dir, "plugin-one", &manifest1);

        let manifest2 = create_test_manifest_full("plugin-two");
        create_plugin_dir(&temp_dir, "plugin-two", &manifest2);

        let manifests = manager.discover_plugins().await;
        assert_eq!(manifests.len(), 2);

        let names: Vec<&str> = manifests.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"plugin-one"));
        assert!(names.contains(&"plugin-two"));
    }

    #[tokio::test]
    async fn test_manager_discover_plugins_empty_dir() {
        let (mut manager, temp_dir) = create_temp_manager();
        // Create the plugins dir but no plugins inside
        fs::create_dir_all(temp_dir.path().join("plugins")).unwrap();

        let manifests = manager.discover_plugins().await;
        assert!(manifests.is_empty());
    }

    #[tokio::test]
    async fn test_manager_discover_plugins_missing_dir() {
        let (mut manager, _temp_dir) = create_temp_manager();
        // Don't create any directories
        let manifests = manager.discover_plugins().await;
        assert!(manifests.is_empty());
    }

    #[tokio::test]
    async fn test_manager_discover_plugins_invalid_manifest() {
        let (mut manager, temp_dir) = create_temp_manager();

        // Create a plugin dir with invalid JSON
        let plugin_dir = temp_dir.path().join("bad-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("plugin.json"), "not json").unwrap();

        // Should not panic, just skip
        let manifests = manager.discover_plugins().await;
        assert!(manifests.is_empty());
    }

    #[tokio::test]
    async fn test_manager_discover_plugins_dir_without_manifest() {
        let (mut manager, temp_dir) = create_temp_manager();

        // Create a directory without plugin.json
        let empty_dir = temp_dir.path().join("empty-dir");
        fs::create_dir_all(&empty_dir).unwrap();

        let manifests = manager.discover_plugins().await;
        assert!(manifests.is_empty());
    }

    #[tokio::test]
    async fn test_manager_load_plugin() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest_full("load-test");
        create_plugin_dir(&temp_dir, "load-test", &manifest);

        manager.load_plugin("load-test").await.unwrap();

        let plugin = manager.get_plugin("load-test").unwrap();
        assert_eq!(plugin.name(), "load-test");
        assert_eq!(plugin.state, PluginState::Active);
        assert_eq!(plugin.manifest.tools.len(), 1);
        assert_eq!(plugin.manifest.hooks.len(), 1);
        assert_eq!(plugin.manifest.commands.len(), 1);
    }

    #[tokio::test]
    async fn test_manager_load_plugin_already_loaded() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest("dup", "1.0.0");
        create_plugin_dir(&temp_dir, "dup", &manifest);

        manager.load_plugin("dup").await.unwrap();
        let result = manager.load_plugin("dup").await;
        assert!(matches!(result, Err(PluginError::AlreadyLoaded(_))));
    }

    #[tokio::test]
    async fn test_manager_load_plugin_not_found() {
        let (mut manager, _temp_dir) = create_temp_manager();
        let result = manager.load_plugin("nonexistent").await;
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_manager_load_plugin_version_mismatch() {
        let (mut manager, temp_dir) = create_temp_manager();

        // Manager version is 0.1.0, plugin requires 1.0.0
        let manifest = PluginManifest {
            min_version: Some("1.0.0".to_string()),
            ..create_test_manifest("version-test", "1.0.0")
        };
        create_plugin_dir(&temp_dir, "version-test", &manifest);

        let result = manager.load_plugin("version-test").await;
        assert!(matches!(result, Err(PluginError::VersionMismatch { .. })));
    }

    #[tokio::test]
    async fn test_manager_load_plugin_name_mismatch() {
        let (mut manager, temp_dir) = create_temp_manager();

        // Directory name is "dir-name" but manifest name is "manifest-name"
        let manifest = create_test_manifest("manifest-name", "1.0.0");
        let plugins_root = temp_dir.path().join("plugins");
        let plugin_dir = plugins_root.join("dir-name");
        fs::create_dir_all(&plugin_dir).unwrap();
        let manifest_path = plugin_dir.join("plugin.json");
        fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

        let result = manager.load_plugin("dir-name").await;
        assert!(matches!(result, Err(PluginError::InvalidManifest(_))));
    }

    // ── Lifecycle tests ───────────────────────────────────────────────

    #[tokio::test]
    async fn test_manager_enable_plugin() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest("enable-test", "1.0.0");
        create_plugin_dir(&temp_dir, "enable-test", &manifest);

        // Load (becomes Active by default)
        manager.load_plugin("enable-test").await.unwrap();
        assert!(manager.get_plugin("enable-test").unwrap().is_active());

        // Disable
        manager.disable_plugin("enable-test").unwrap();
        assert!(manager.get_plugin("enable-test").unwrap().is_disabled());

        // Re-enable
        manager.enable_plugin("enable-test").unwrap();
        assert!(manager.get_plugin("enable-test").unwrap().is_active());
    }

    #[tokio::test]
    async fn test_manager_enable_already_active() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest("test", "1.0.0");
        create_plugin_dir(&temp_dir, "test", &manifest);

        manager.load_plugin("test").await.unwrap();
        // Enabling an already active plugin should be a no-op
        assert!(manager.enable_plugin("test").is_ok());
        assert!(manager.get_plugin("test").unwrap().is_active());
    }

    #[tokio::test]
    async fn test_manager_enable_wrong_state() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest("test", "1.0.0");
        create_plugin_dir(&temp_dir, "test", &manifest);

        manager.load_plugin("test").await.unwrap();
        // Manually set to Loaded state
        manager.get_plugin_mut("test").unwrap().state = PluginState::Loaded;

        let result = manager.enable_plugin("test");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_manager_disable_already_disabled() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest("test", "1.0.0");
        create_plugin_dir(&temp_dir, "test", &manifest);

        manager.load_plugin("test").await.unwrap();
        manager.disable_plugin("test").unwrap();

        // Disabling again should be a no-op
        assert!(manager.disable_plugin("test").is_ok());
        assert!(manager.get_plugin("test").unwrap().is_disabled());
    }

    #[tokio::test]
    async fn test_manager_disable_wrong_state() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest("test", "1.0.0");
        create_plugin_dir(&temp_dir, "test", &manifest);

        manager.load_plugin("test").await.unwrap();
        // Manually set to Loaded state
        manager.get_plugin_mut("test").unwrap().state = PluginState::Loaded;

        let result = manager.disable_plugin("test");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_manager_unload_plugin() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest("unload-test", "1.0.0");
        create_plugin_dir(&temp_dir, "unload-test", &manifest);

        manager.load_plugin("unload-test").await.unwrap();
        assert!(manager.get_plugin("unload-test").is_some());

        manager.unload_plugin("unload-test").unwrap();
        assert!(manager.get_plugin("unload-test").is_none());
    }

    #[tokio::test]
    async fn test_manager_unload_not_found() {
        let (mut manager, _temp_dir) = create_temp_manager();
        let result = manager.unload_plugin("nonexistent");
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    // ── Tool/hook/command listing tests ───────────────────────────────

    #[tokio::test]
    async fn test_manager_get_plugin_tools() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest1 = create_test_manifest_full("plugin-a");
        create_plugin_dir(&temp_dir, "plugin-a", &manifest1);

        let manifest2 = PluginManifest {
            tools: vec![ToolDefinition {
                name: "plugin_b_tool".to_string(),
                description: "Tool from B".to_string(),
                input_schema: serde_json::json!({}),
                command: "echo b".to_string(),
                is_read_only: false,
            }],
            ..create_test_manifest("plugin-b", "1.0.0")
        };
        create_plugin_dir(&temp_dir, "plugin-b", &manifest2);

        manager.load_plugin("plugin-a").await.unwrap();
        manager.load_plugin("plugin-b").await.unwrap();

        let tools = manager.get_plugin_tools();
        assert_eq!(tools.len(), 2);

        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"plugin-a_tool"));
        assert!(tool_names.contains(&"plugin_b_tool"));
    }

    #[tokio::test]
    async fn test_manager_get_plugin_tools_disabled_excluded() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest_full("active-plugin");
        create_plugin_dir(&temp_dir, "active-plugin", &manifest);

        manager.load_plugin("active-plugin").await.unwrap();
        manager.disable_plugin("active-plugin").unwrap();

        let tools = manager.get_plugin_tools();
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_manager_get_plugin_hooks() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest_full("hook-plugin");
        create_plugin_dir(&temp_dir, "hook-plugin", &manifest);

        manager.load_plugin("hook-plugin").await.unwrap();

        let hooks = manager.get_plugin_hooks();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].event, "PreToolUse");
    }

    #[tokio::test]
    async fn test_manager_get_plugin_commands() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest_full("cmd-plugin");
        create_plugin_dir(&temp_dir, "cmd-plugin", &manifest);

        manager.load_plugin("cmd-plugin").await.unwrap();

        let commands = manager.get_plugin_commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "cmd-plugin_cmd");
    }

    #[tokio::test]
    async fn test_manager_get_tools_for_plugin() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest_full("specific");
        create_plugin_dir(&temp_dir, "specific", &manifest);

        manager.load_plugin("specific").await.unwrap();

        let tools = manager.get_tools_for_plugin("specific").unwrap();
        assert_eq!(tools.len(), 1);

        let result = manager.get_tools_for_plugin("nonexistent");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_manager_get_hooks_for_plugin() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest_full("specific");
        create_plugin_dir(&temp_dir, "specific", &manifest);

        manager.load_plugin("specific").await.unwrap();

        let hooks = manager.get_hooks_for_plugin("specific").unwrap();
        assert_eq!(hooks.len(), 1);

        let result = manager.get_hooks_for_plugin("nonexistent");
        assert!(result.is_err());
    }

    // ── List plugins test ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_manager_list_plugins() {
        let (mut manager, temp_dir) = create_temp_manager();

        create_plugin_dir(&temp_dir, "alpha", &create_test_manifest("alpha", "1.0.0"));
        create_plugin_dir(&temp_dir, "beta", &create_test_manifest("beta", "2.0.0"));

        manager.load_plugin("alpha").await.unwrap();
        manager.load_plugin("beta").await.unwrap();

        let plugins = manager.list_plugins();
        assert_eq!(plugins.len(), 2);
        // Should be sorted by name
        assert_eq!(plugins[0].name(), "alpha");
        assert_eq!(plugins[1].name(), "beta");
    }

    // ── State persistence tests ───────────────────────────────────────

    #[tokio::test]
    async fn test_manager_save_and_load_state() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest("persist-test", "1.0.0");
        create_plugin_dir(&temp_dir, "persist-test", &manifest);

        manager.load_plugin("persist-test").await.unwrap();
        manager.disable_plugin("persist-test").unwrap();

        // State should be saved to disk
        let state_path = manager.state_file_path();
        assert!(state_path.exists());

        let content = fs::read_to_string(&state_path).unwrap();
        let state: PluginStateFile = serde_json::from_str(&content).unwrap();
        assert!(state.is_disabled("persist-test"));
    }

    #[tokio::test]
    async fn test_manager_load_state_applies_disabled() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest("state-test", "1.0.0");
        create_plugin_dir(&temp_dir, "state-test", &manifest);

        // Pre-create state file with this plugin disabled
        let state = PluginStateFile {
            disabled_plugins: vec!["state-test".to_string()],
            plugin_settings: HashMap::new(),
        };
        let state_path = temp_dir.path().join("plugin-state.json");
        fs::write(&state_path, state.to_json().unwrap()).unwrap();

        // Load the plugin (should read state and mark as disabled)
        manager.load_plugin("state-test").await.unwrap();
        assert!(manager.get_plugin("state-test").unwrap().is_disabled());
    }

    #[tokio::test]
    async fn test_manager_load_state_applies_settings() {
        let (mut manager, temp_dir) = create_temp_manager();

        let manifest = create_test_manifest("settings-test", "1.0.0");
        create_plugin_dir(&temp_dir, "settings-test", &manifest);

        // Pre-create state file with settings
        let mut state = PluginStateFile::new();
        state.set_settings("settings-test", serde_json::json!({"api_key": "test-key"}));
        let state_path = temp_dir.path().join("plugin-state.json");
        fs::write(&state_path, state.to_json().unwrap()).unwrap();

        manager.load_plugin("settings-test").await.unwrap();
        let plugin = manager.get_plugin("settings-test").unwrap();
        assert_eq!(plugin.settings["api_key"], "test-key");
    }

    #[tokio::test]
    async fn test_manager_save_state_nonexistent_dir() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("nested").join("dir").join("state.json");

        let manager = PluginManager::with_config(
            vec![],
            state_path,
            "1.0.0",
        );

        // Should create intermediate directories
        let result = manager.save_state();
        assert!(result.is_ok());
        assert!(manager.state_file_path().parent().unwrap().exists());
    }

    // ── discover_and_load_all tests ───────────────────────────────────

    #[tokio::test]
    async fn test_manager_discover_and_load_all() {
        let (mut manager, temp_dir) = create_temp_manager();

        create_plugin_dir(&temp_dir, "auto-a", &create_test_manifest("auto-a", "1.0.0"));
        create_plugin_dir(&temp_dir, "auto-b", &create_test_manifest_full("auto-b"));

        let loaded = manager.discover_and_load_all().await.unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(manager.get_plugin("auto-a").is_some());
        assert!(manager.get_plugin("auto-b").is_some());
    }

    #[tokio::test]
    async fn test_manager_discover_and_load_all_with_failure() {
        let (mut manager, temp_dir) = create_temp_manager();

        // One valid plugin
        create_plugin_dir(&temp_dir, "good", &create_test_manifest("good", "1.0.0"));

        // One plugin with version mismatch
        let bad_manifest = PluginManifest {
            min_version: Some("99.0.0".to_string()),
            ..create_test_manifest("bad", "1.0.0")
        };
        create_plugin_dir(&temp_dir, "bad", &bad_manifest);

        let loaded = manager.discover_and_load_all().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(manager.get_plugin("good").is_some());
        // Failed plugin should still be tracked
        let bad_plugin = manager.get_plugin("bad").unwrap();
        assert!(matches!(bad_plugin.state, PluginState::Failed(_)));
    }

    // ── Error handling tests ──────────────────────────────────────────

    #[test]
    fn test_plugin_error_display() {
        let err = PluginError::NotFound("my-plugin".to_string());
        assert!(err.to_string().contains("my-plugin"));

        let err = PluginError::AlreadyLoaded("my-plugin".to_string());
        assert!(err.to_string().contains("already loaded"));

        let err = PluginError::VersionMismatch {
            required: "1.0.0".to_string(),
            current: "0.1.0".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("1.0.0") && msg.contains("0.1.0"));
    }

    #[test]
    fn test_plugin_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = PluginError::from(io_err);
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn test_plugin_error_from_json() {
        let json_err = serde_json::from_str::<Value>("invalid").unwrap_err();
        let err = PluginError::from(json_err);
        // Should contain JSON error message
        assert!(err.to_string().len() > 0);
    }

    // ── HookDefinition defaults tests ─────────────────────────────────

    #[test]
    fn test_hook_definition_defaults() {
        let json = r#"{
            "event": "PreToolUse",
            "matcher": "Bash",
            "command": "echo test"
        }"#;

        let hook: HookDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(hook.timeout_secs, 30);
        assert!(hook.blocking);
    }

    #[test]
    fn test_hook_definition_custom_values() {
        let hook = HookDefinition {
            event: "PostToolUse".to_string(),
            matcher: "*".to_string(),
            command: "echo post".to_string(),
            timeout_secs: 10,
            blocking: false,
        };
        let json = serde_json::to_string(&hook).unwrap();
        let parsed: HookDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(hook, parsed);
    }
}
