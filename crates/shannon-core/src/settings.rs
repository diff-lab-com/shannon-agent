//! # Settings Configuration Management
//!
//! This module provides configuration management for Shannon Code.
//!
//! ## Architecture
//!
//! Settings are loaded from three locations (later overrides earlier):
//! 1. **User-level**: `~/.shannon/settings.json` (shared across all projects)
//! 2. **Project-level**: `.shannon/settings.json` (committed to VCS, shared with team)
//! 3. **Local-level**: `.shannon/settings.local.json` (personal, gitignored, highest priority)
//!
//! Permission rules follow **deny > ask > allow** priority across all layers.
//!
//! ## Example
//!
//! ```no_run
//! use shannon_core::settings::SettingsManager;
//!
//! # fn main() -> Result<(), shannon_core::SettingsError> {
//! let mut manager = SettingsManager::new();
//! manager.load()?;
//!
//! // Get a setting value
//! if let Some(model) = manager.get("model") {
//!     println!("Model: {}", model);
//! }
//!
//! // Set a setting value
//! manager.set("model", "claude-opus-4-6");
//! manager.save()?;
//! # Ok(())
//! # }
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Current version of the settings schema
const SETTINGS_VERSION: &str = "1.0";

/// Permission rules for tool access control.
///
/// Rules are evaluated in **deny > ask > allow** priority order.
/// When merging across settings layers (user -> project -> local),
/// deny rules from any layer take highest precedence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PermissionRules {
    /// Tool patterns that are always denied (highest priority).
    #[serde(default)]
    pub deny: Vec<String>,
    /// Tool patterns that require explicit approval.
    #[serde(default)]
    pub ask: Vec<String>,
    /// Tool patterns that are always allowed.
    #[serde(default)]
    pub allow: Vec<String>,
}

impl PermissionRules {
    /// Create empty permission rules.
    pub fn new() -> Self { Self::default() }

    /// Check if these rules are empty.
    pub fn is_empty(&self) -> bool {
        self.deny.is_empty() && self.ask.is_empty() && self.allow.is_empty()
    }

    /// Merge another set of permission rules into this one.
    pub fn merge(&mut self, other: PermissionRules) {
        self.deny.extend(other.deny);
        self.ask.extend(other.ask);
        self.allow.extend(other.allow);
    }
}


/// Error type for settings operations
#[derive(Error, Debug)]
pub enum SettingsError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Home directory not found")]
    HomeNotFound,

    #[error("Invalid setting value for key '{key}': {message}")]
    InvalidValue { key: String, message: String },

    #[error("Setting key not found: {0}")]
    KeyNotFound(String),

    #[error("Invalid settings version: expected {expected}, got {got}")]
    InvalidVersion { expected: String, got: String },
}

/// Configuration settings for Shannon Code
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Schema version for migration support
    pub version: String,

    /// Default model to use for queries
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Temperature for model responses (0.0 - 1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Maximum tokens for model responses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Whether MCP tools are enabled
    #[serde(default = "default_tools_enabled")]
    pub tools_enabled: bool,

    /// Permission mode: "ask", "auto", "readonly"
    #[serde(default = "default_permissions_mode")]
    pub permissions_mode: String,

    /// Whether auto-memory is enabled
    #[serde(default = "default_auto_memory")]
    pub auto_memory: bool,

    /// UI theme: "dark", "light", "auto"
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Permission rules for tool access control (deny > ask > allow).
    #[serde(default)]
    pub permissions: PermissionRules,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION.to_string(),
            model: None,
            temperature: None,
            max_tokens: None,
            tools_enabled: default_tools_enabled(),
            permissions_mode: default_permissions_mode(),
            auto_memory: default_auto_memory(),
            theme: default_theme(),
            permissions: PermissionRules::default(),
        }
    }
}

// Default value functions
fn default_tools_enabled() -> bool {
    true
}

fn default_permissions_mode() -> String {
    "ask".to_string()
}

fn default_auto_memory() -> bool {
    true
}

fn default_theme() -> String {
    "dark".to_string()
}

impl Settings {
    /// Create a new Settings with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate the settings values
    pub fn validate(&self) -> Result<(), SettingsError> {
        // Validate temperature if present
        if let Some(temp) = self.temperature {
            if !(0.0..=1.0).contains(&temp) {
                return Err(SettingsError::InvalidValue {
                    key: "temperature".to_string(),
                    message: format!("temperature must be between 0.0 and 1.0, got {temp}"),
                });
            }
        }

        // Validate max_tokens if present
        if let Some(tokens) = self.max_tokens {
            if tokens == 0 {
                return Err(SettingsError::InvalidValue {
                    key: "max_tokens".to_string(),
                    message: "max_tokens must be greater than 0".to_string(),
                });
            }
        }

        // Validate permissions_mode
        if !["ask", "auto", "readonly"].contains(&self.permissions_mode.as_str()) {
            return Err(SettingsError::InvalidValue {
                key: "permissions_mode".to_string(),
                message: format!(
                    "permissions_mode must be one of 'ask', 'auto', 'readonly', got '{}'",
                    self.permissions_mode
                ),
            });
        }

        // Validate theme
        if !["dark", "light", "auto"].contains(&self.theme.as_str()) {
            return Err(SettingsError::InvalidValue {
                key: "theme".to_string(),
                message: format!(
                    "theme must be one of 'dark', 'light', 'auto', got '{}'",
                    self.theme
                ),
            });
        }

        Ok(())
    }

    /// Get a setting value as a JSON Value
    pub fn get_value(&self, key: &str) -> Option<Value> {
        match key {
            "version" => Some(Value::String(self.version.clone())),
            "model" => self.model.as_ref().map(|v| Value::String(v.clone())),
            "temperature" => self.temperature.map(|v| {
                let num = serde_json::Number::from_f64(v as f64)
                    .expect("temperature f64 should be a valid JSON number");
                Value::Number(num)
            }),
            "max_tokens" => self.max_tokens.map(|v| Value::Number(v.into())),
            "tools_enabled" => Some(Value::Bool(self.tools_enabled)),
            "permissions_mode" => Some(Value::String(self.permissions_mode.clone())),
            "auto_memory" => Some(Value::Bool(self.auto_memory)),
            "theme" => Some(Value::String(self.theme.clone())),
            _ => None,
        }
    }

    /// Set a setting value from a JSON Value
    pub fn set_value(&mut self, key: &str, value: Value) -> Result<(), SettingsError> {
        match key {
            "model" => {
                match value {
                    Value::Null => { self.model = None; }
                    Value::String(ref s) if s.is_empty() => { self.model = None; }
                    Value::String(ref s) => { self.model = Some(s.to_string()); }
                    _ => { return Err(SettingsError::InvalidValue { key: key.to_string(), message: "model must be a string".to_string() }); }
                }
            }
            "temperature" => {
                self.temperature = match value {
                    Value::Null => None,
                    Value::Number(n) => n.as_f64().map(|f| f as f32),
                    _ => return Err(SettingsError::InvalidValue {
                        key: key.to_string(),
                        message: "temperature must be a number".to_string(),
                    }),
                };
            }
            "max_tokens" => {
                self.max_tokens = match value {
                    Value::Null => None,
                    Value::Number(n) => n.as_u64().map(|v| v as u32),
                    _ => return Err(SettingsError::InvalidValue {
                        key: key.to_string(),
                        message: "max_tokens must be a number".to_string(),
                    }),
                };
            }
            "tools_enabled" => {
                if let Some(b) = value.as_bool() {
                    self.tools_enabled = b;
                } else {
                    return Err(SettingsError::InvalidValue {
                        key: key.to_string(),
                        message: "tools_enabled must be a boolean".to_string(),
                    });
                }
            }
            "permissions_mode" => {
                if let Some(s) = value.as_str() {
                    self.permissions_mode = s.to_string();
                } else {
                    return Err(SettingsError::InvalidValue {
                        key: key.to_string(),
                        message: "permissions_mode must be a string".to_string(),
                    });
                }
            }
            "auto_memory" => {
                if let Some(b) = value.as_bool() {
                    self.auto_memory = b;
                } else {
                    return Err(SettingsError::InvalidValue {
                        key: key.to_string(),
                        message: "auto_memory must be a boolean".to_string(),
                    });
                }
            }
            "theme" => {
                if let Some(s) = value.as_str() {
                    self.theme = s.to_string();
                } else {
                    return Err(SettingsError::InvalidValue {
                        key: key.to_string(),
                        message: "theme must be a string".to_string(),
                    });
                }
            }
            _ => {
                return Err(SettingsError::KeyNotFound(key.to_string()));
            }
        }
        Ok(())
    }

    /// Merge another Settings instance into this one
    /// Values from `other` take precedence
    pub fn merge(&mut self, other: Settings) {
        if other.model.is_some() {
            self.model = other.model;
        }
        if other.temperature.is_some() {
            self.temperature = other.temperature;
        }
        if other.max_tokens.is_some() {
            self.max_tokens = other.max_tokens;
        }
        // Always take the override for these fields
        self.tools_enabled = other.tools_enabled;
        self.permissions_mode = other.permissions_mode;
        self.auto_memory = other.auto_memory;
        self.theme = other.theme;
        self.permissions.merge(other.permissions);
    }
}

/// Manager for loading, saving, and manipulating settings
#[derive(Debug, Clone)]
pub struct SettingsManager {
    settings: Settings,
    user_config_path: PathBuf,
    project_config_path: PathBuf,
    local_config_path: PathBuf,
}

/// Try multiple environment variable names in priority order.
/// Returns the value of the first one found.
fn env_priority(names: &[&str]) -> Option<String> {
    for name in names {
        if let Ok(value) = std::env::var(name) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

impl Default for SettingsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsManager {
    /// Create a new SettingsManager with default settings
    pub fn new() -> Self {
        let home_dir = dirs::home_dir()
            .expect("Home directory should exist");

        let user_config_path = home_dir
            .join(".shannon")
            .join("settings.json");

        let project_config_path = PathBuf::from(".shannon/settings.json");
        let local_config_path = PathBuf::from(".shannon/settings.local.json");

        Self {
            settings: Settings::new(),
            user_config_path,
            project_config_path,
            local_config_path,
        }
    }

    /// Load settings from disk with full priority chain:
    /// 1. settings.json (user + project)
    /// 2. .env files (.env → .env.local → .env.production)
    /// 3. Environment variables (highest priority)
    pub fn load(&mut self) -> Result<(), SettingsError> {
        self.load_from_files()?;
        self.load_from_dotenv()?;
        self.load_from_env()?;
        Ok(())
    }

    /// Load settings from JSON files only (user + project + local).
    /// Skips .env and environment variable overrides.
    /// Useful for testing file I/O round-trips in isolation.
    ///
    /// Priority order (later overrides earlier):
    /// 1. User settings (`~/.shannon/settings.json`)
    /// 2. Project settings (`.shannon/settings.json`)
    /// 3. Local settings (`.shannon/settings.local.json`)
    pub fn load_from_files(&mut self) -> Result<(), SettingsError> {
        // Start with user settings
        if self.user_config_path.exists() {
            let content = std::fs::read_to_string(&self.user_config_path)?;
            let user_settings: Settings = serde_json::from_str(&content)?;

            // Validate version
            if user_settings.version != SETTINGS_VERSION {
                return Err(SettingsError::InvalidVersion {
                    expected: SETTINGS_VERSION.to_string(),
                    got: user_settings.version,
                });
            }

            user_settings.validate()?;
            self.settings = user_settings;
        }

        // Merge project settings if they exist
        if self.project_config_path.exists() {
            let content = std::fs::read_to_string(&self.project_config_path)?;
            let project_settings: Settings = serde_json::from_str(&content)?;

            // Validate version
            if project_settings.version != SETTINGS_VERSION {
                return Err(SettingsError::InvalidVersion {
                    expected: SETTINGS_VERSION.to_string(),
                    got: project_settings.version,
                });
            }

            project_settings.validate()?;
            self.settings.merge(project_settings);
        }

        // Merge local settings if they exist (highest file priority)
        if self.local_config_path.exists() {
            let content = std::fs::read_to_string(&self.local_config_path)?;
            let local_settings: Settings = serde_json::from_str(&content)?;

            if local_settings.version != SETTINGS_VERSION {
                return Err(SettingsError::InvalidVersion {
                    expected: SETTINGS_VERSION.to_string(),
                    got: local_settings.version,
                });
            }

            local_settings.validate()?;
            self.settings.merge(local_settings);
        }

        Ok(())
    }

    /// Load settings from .env files in the current directory.
    /// Searches: .env → .env.local → .env.production
    /// Later files override earlier ones.
    fn load_from_dotenv(&mut self) -> Result<(), SettingsError> {
        for name in &[".env", ".env.local", ".env.production"] {
            let path = std::path::Path::new(name);
            if path.exists() {
                let content = std::fs::read_to_string(path)?;
                self.parse_env_content(&content)?;
            }
        }
        Ok(())
    }

    /// Parse KEY=VALUE lines from an env file content string.
    ///
    /// Handles:
    /// - Comments (`#`) and blank lines
    /// - Quoted values (single or double quotes)
    /// - Multi-line values: a line ending with `\` continues on the next line
    fn parse_env_content(&mut self, content: &str) -> Result<(), SettingsError> {
        // Join continuation lines (trailing backslash)
        let mut joined = String::with_capacity(content.len());
        for raw_line in content.lines() {
            let trimmed_end = raw_line.trim_end_matches([' ', '\t']);
            if let Some(continued) = trimmed_end.strip_suffix('\\') {
                joined.push_str(continued);
                joined.push(' ');
            } else {
                joined.push_str(raw_line);
                joined.push('\n');
            }
        }

        for line in joined.lines() {
            let line = line.trim();
            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Split on first '=' only
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                // Remove surrounding quotes if present (only if both prefix and suffix match)
                let value = if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                    inner
                } else if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
                    inner
                } else {
                    value
                };
                self.apply_env_var(key, value);
            }
        }
        Ok(())
    }

    /// Load settings from environment variables with priority chain:
    /// SHANNON_* → ANTHROPIC_* → OPENAI_* → bare name
    fn load_from_env(&mut self) -> Result<(), SettingsError> {
        // SHANNON_MODEL → ANTHROPIC_MODEL → OPENAI_MODEL → MODEL
        if let Some(v) = env_priority(&["SHANNON_MODEL", "ANTHROPIC_MODEL", "OPENAI_MODEL", "MODEL"]) {
            self.settings.model = Some(v);
        }
        // SHANNON_API_KEY → ANTHROPIC_API_KEY → OPENAI_API_KEY (handled by LlmClient, but store model hint)
        // SHANNON_MAX_TOKENS → MAX_TOKENS
        if let Some(v) = env_priority(&["SHANNON_MAX_TOKENS", "MAX_TOKENS"]) {
            if let Ok(tokens) = v.parse::<u32>() {
                self.settings.max_tokens = Some(tokens);
            }
        }
        // SHANNON_TEMPERATURE → TEMPERATURE
        if let Some(v) = env_priority(&["SHANNON_TEMPERATURE", "TEMPERATURE"]) {
            if let Ok(temp) = v.parse::<f32>() {
                self.settings.temperature = Some(temp);
            }
        }
        // SHANNON_PERMISSIONS_MODE → PERMISSIONS_MODE
        if let Some(v) = env_priority(&["SHANNON_PERMISSIONS_MODE", "PERMISSIONS_MODE"]) {
            if ["ask", "auto", "readonly"].contains(&v.as_str()) {
                self.settings.permissions_mode = v;
            }
        }
        // SHANNON_THEME → THEME
        if let Some(v) = env_priority(&["SHANNON_THEME", "THEME"]) {
            if ["dark", "light", "auto"].contains(&v.as_str()) {
                self.settings.theme = v;
            }
        }
        // SHANNON_AUTO_MEMORY → AUTO_MEMORY
        if let Some(v) = env_priority(&["SHANNON_AUTO_MEMORY", "AUTO_MEMORY"]) {
            if let Ok(b) = v.parse::<bool>() {
                self.settings.auto_memory = b;
            }
        }
        // SHANNON_TOOLS_ENABLED → TOOLS_ENABLED
        if let Some(v) = env_priority(&["SHANNON_TOOLS_ENABLED", "TOOLS_ENABLED"]) {
            if let Ok(b) = v.parse::<bool>() {
                self.settings.tools_enabled = b;
            }
        }
        Ok(())
    }

    /// Apply a single env variable key-value pair to settings.
    fn apply_env_var(&mut self, key: &str, value: &str) {
        match key {
            "SHANNON_MODEL" | "ANTHROPIC_MODEL" | "OPENAI_MODEL" | "MODEL" => {
                self.settings.model = Some(value.to_string());
            }
            "SHANNON_MAX_TOKENS" | "MAX_TOKENS" => {
                if let Ok(tokens) = value.parse::<u32>() {
                    self.settings.max_tokens = Some(tokens);
                }
            }
            "SHANNON_TEMPERATURE" | "TEMPERATURE" => {
                if let Ok(temp) = value.parse::<f32>() {
                    self.settings.temperature = Some(temp);
                }
            }
            "SHANNON_PERMISSIONS_MODE" | "PERMISSIONS_MODE" => {
                if ["ask", "auto", "readonly"].contains(&value) {
                    self.settings.permissions_mode = value.to_string();
                }
            }
            "SHANNON_THEME" | "THEME" => {
                if ["dark", "light", "auto"].contains(&value) {
                    self.settings.theme = value.to_string();
                }
            }
            "SHANNON_AUTO_MEMORY" | "AUTO_MEMORY" => {
                if let Ok(b) = value.parse::<bool>() {
                    self.settings.auto_memory = b;
                }
            }
            "SHANNON_TOOLS_ENABLED" | "TOOLS_ENABLED" => {
                if let Ok(b) = value.parse::<bool>() {
                    self.settings.tools_enabled = b;
                }
            }
            _ => {
                // Ignore unknown env vars
            }
        }
    }

    /// Apply a list of KEY=VALUE pairs (from CLI -e flags).
    /// Each pair overrides settings, with later entries winning.
    pub fn apply_env_overrides(&mut self, overrides: &[String]) -> Result<(), SettingsError> {
        for pair in overrides {
            if let Some((key, value)) = pair.split_once('=') {
                let value = value
                    .strip_prefix('"').and_then(|v| v.strip_suffix('"'))
                    .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                    .unwrap_or(value);
                self.apply_env_var(key.trim(), value.trim());
            }
        }
        self.settings.validate()?;
        Ok(())
    }

    /// Save current settings to user config file
    pub fn save(&self) -> Result<(), SettingsError> {
        // Validate before saving
        self.settings.validate()?;

        // Create parent directory if it doesn't exist
        if let Some(parent) = self.user_config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Serialize with pretty printing
        let json = serde_json::to_string_pretty(&self.settings)?;

        std::fs::write(&self.user_config_path, json)?;
        Ok(())
    }

    /// Get a reference to the current settings
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Get a mutable reference to the current settings
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// Get a setting value as a string
    pub fn get(&self, key: &str) -> Option<Value> {
        self.settings.get_value(key)
    }

    /// Set a setting value
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), SettingsError> {
        let json_value: Value = serde_json::from_str(value)
            .unwrap_or_else(|_| Value::String(value.to_string()));

        self.settings.set_value(key, json_value)?;
        Ok(())
    }

    /// Merge project-level settings into current settings
    pub fn merge(&mut self, project_settings: Settings) {
        self.settings.merge(project_settings);
    }

    /// Get the user config path
    pub fn user_config_path(&self) -> &Path {
        &self.user_config_path
    }

    /// Get the project config path
    pub fn project_config_path(&self) -> &Path {
        &self.project_config_path
    }

    /// Get the local config path
    pub fn local_config_path(&self) -> &Path {
        &self.local_config_path
    }

    /// Load settings from a specific path
    pub fn load_from_path(&mut self, path: &Path) -> Result<(), SettingsError> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            let loaded_settings: Settings = serde_json::from_str(&content)?;

            // Validate version
            if loaded_settings.version != SETTINGS_VERSION {
                return Err(SettingsError::InvalidVersion {
                    expected: SETTINGS_VERSION.to_string(),
                    got: loaded_settings.version,
                });
            }

            loaded_settings.validate()?;
            self.settings = loaded_settings;
        }
        Ok(())
    }

    /// Save settings to a specific path
    pub fn save_to_path(&self, path: &Path) -> Result<(), SettingsError> {
        self.settings.validate()?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(&self.settings)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_temp_manager() -> (SettingsManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let user_path = temp_dir.path().join("user_settings.json");
        let project_path = temp_dir.path().join("project_settings.json");
        let local_path = temp_dir.path().join("local_settings.json");

        let mut manager = SettingsManager::new();
        // Override paths for testing
        manager.user_config_path = user_path.clone();
        manager.project_config_path = project_path.clone();
        manager.local_config_path = local_path.clone();

        (manager, temp_dir)
    }

    #[test]
    fn test_settings_default_values() {
        let settings = Settings::new();

        assert_eq!(settings.version, SETTINGS_VERSION);
        assert_eq!(settings.model, None);
        assert_eq!(settings.temperature, None);
        assert_eq!(settings.max_tokens, None);
        assert!(settings.tools_enabled);
        assert_eq!(settings.permissions_mode, "ask");
        assert!(settings.auto_memory);
        assert_eq!(settings.theme, "dark");
        assert!(settings.permissions.is_empty());
    }

    #[test]
    fn test_settings_validation() {
        let mut settings = Settings::new();

        // Valid settings
        assert!(settings.validate().is_ok());

        // Invalid temperature (too high)
        settings.temperature = Some(1.5);
        assert!(settings.validate().is_err());
        settings.temperature = Some(0.5);

        // Invalid max_tokens (zero)
        settings.max_tokens = Some(0);
        assert!(settings.validate().is_err());
        settings.max_tokens = Some(1000);

        // Invalid permissions_mode
        settings.permissions_mode = "invalid".to_string();
        assert!(settings.validate().is_err());
        settings.permissions_mode = "auto".to_string();

        // Invalid theme
        settings.theme = "invalid".to_string();
        assert!(settings.validate().is_err());
        settings.theme = "light".to_string();

        // All valid again
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn test_settings_get_value() {
        let settings = Settings {
            version: SETTINGS_VERSION.to_string(),
            model: Some("claude-opus-4-6".to_string()),
            temperature: Some(0.5),
            max_tokens: Some(4096),
            tools_enabled: false,
            permissions_mode: "auto".to_string(),
            auto_memory: false,
            theme: "light".to_string(),
            permissions: PermissionRules::default(),
        };

        assert_eq!(
            settings.get_value("model"),
            Some(Value::String("claude-opus-4-6".to_string()))
        );
        assert_eq!(settings.get_value("temperature"), Some(Value::Number(serde_json::Number::from_f64(0.5).unwrap())));
        assert_eq!(settings.get_value("max_tokens"), Some(Value::Number(4096.into())));
        assert_eq!(settings.get_value("tools_enabled"), Some(Value::Bool(false)));
        assert_eq!(
            settings.get_value("permissions_mode"),
            Some(Value::String("auto".to_string()))
        );
        assert_eq!(settings.get_value("auto_memory"), Some(Value::Bool(false)));
        assert_eq!(settings.get_value("theme"), Some(Value::String("light".to_string())));
        assert_eq!(settings.get_value("invalid_key"), None);
    }

    #[test]
    fn test_settings_set_value() {
        let mut settings = Settings::new();

        // Set model
        settings
            .set_value("model", Value::String("claude-opus-4-6".to_string()))
            .unwrap();
        assert_eq!(settings.model, Some("claude-opus-4-6".to_string()));

        // Set temperature
        settings.set_value("temperature", Value::Number(serde_json::Number::from_f64(0.5).unwrap())).unwrap();
        assert_eq!(settings.temperature, Some(0.5));

        // Set max_tokens
        settings.set_value("max_tokens", Value::Number(8192.into())).unwrap();
        assert_eq!(settings.max_tokens, Some(8192));

        // Set tools_enabled
        settings.set_value("tools_enabled", Value::Bool(false)).unwrap();
        assert!(!settings.tools_enabled);

        // Set permissions_mode
        settings
            .set_value("permissions_mode", Value::String("auto".to_string()))
            .unwrap();
        assert_eq!(settings.permissions_mode, "auto");

        // Set auto_memory
        settings.set_value("auto_memory", Value::Bool(false)).unwrap();
        assert!(!settings.auto_memory);

        // Set theme
        settings
            .set_value("theme", Value::String("light".to_string()))
            .unwrap();
        assert_eq!(settings.theme, "light");

        // Test invalid key
        assert!(settings
            .set_value("invalid_key", Value::String("test".to_string()))
            .is_err());

        // Test invalid type
        assert!(settings
            .set_value("model", Value::Bool(true))
            .is_err());
    }

    #[test]
    fn test_settings_merge() {
        let mut base = Settings {
            version: SETTINGS_VERSION.to_string(),
            model: Some("claude-sonnet-4-6".to_string()),
            temperature: Some(0.5),
            max_tokens: Some(4096),
            tools_enabled: true,
            permissions_mode: "ask".to_string(),
            auto_memory: true,
            theme: "dark".to_string(),
            permissions: PermissionRules {
                allow: vec!["Read(*)".to_string()],
                ..Default::default()
            },
        };

        let override_settings = Settings {
            version: SETTINGS_VERSION.to_string(),
            model: Some("claude-opus-4-6".to_string()),
            temperature: Some(0.8),
            max_tokens: Some(8192),
            tools_enabled: false,
            permissions_mode: "auto".to_string(),
            auto_memory: false,
            theme: "light".to_string(),
            permissions: PermissionRules {
                deny: vec!["Bash(rm -rf /)".to_string()],
                allow: vec!["Bash(git *)".to_string()],
                ..Default::default()
            },
        };

        base.merge(override_settings);

        assert_eq!(base.model, Some("claude-opus-4-6".to_string()));
        assert_eq!(base.temperature, Some(0.8));
        assert_eq!(base.max_tokens, Some(8192));
        assert!(!base.tools_enabled);
        assert_eq!(base.permissions_mode, "auto");
        assert!(!base.auto_memory);
        assert_eq!(base.theme, "light");
        assert!(base.permissions.allow.contains(&"Read(*)".to_string()));
        assert!(base.permissions.allow.contains(&"Bash(git *)".to_string()));
        assert!(base.permissions.deny.contains(&"Bash(rm -rf /)".to_string()));
    }

    #[test]
    fn test_manager_new() {
        let manager = SettingsManager::new();

        assert_eq!(manager.settings.version, SETTINGS_VERSION);
        assert!(manager.settings.tools_enabled);
        assert_eq!(manager.settings.permissions_mode, "ask");
        assert!(manager.settings.auto_memory);
        assert_eq!(manager.settings.theme, "dark");
    }

    #[test]
    fn test_manager_save_and_load() {
        let (mut manager, _temp_dir) = create_temp_manager();

        // Modify settings
        manager.settings.model = Some("claude-opus-4-6".to_string());
        manager.settings.temperature = Some(0.7);
        manager.settings.max_tokens = Some(8192);
        manager.settings.tools_enabled = false;
        manager.settings.permissions_mode = "auto".to_string();

        // Save
        manager.save().unwrap();

        // Create new manager and load from files only (no env overrides)
        let mut manager2 = SettingsManager::new();
        manager2.user_config_path = manager.user_config_path.clone();
        manager2.project_config_path = manager.project_config_path.clone();
        manager2.local_config_path = manager.local_config_path.clone();
        manager2.load_from_files().unwrap();

        // Verify loaded settings
        assert_eq!(manager2.settings.model, Some("claude-opus-4-6".to_string()));
        assert_eq!(manager2.settings.temperature, Some(0.7));
        assert_eq!(manager2.settings.max_tokens, Some(8192));
        assert!(!manager2.settings.tools_enabled);
        assert_eq!(manager2.settings.permissions_mode, "auto");
    }

    #[test]
    fn test_manager_load_nonexistent() {
        let (mut manager, _temp_dir) = create_temp_manager();

        // Load should succeed even if file doesn't exist (uses defaults)
        assert!(manager.load().is_ok());
        assert_eq!(manager.settings.version, SETTINGS_VERSION);
    }

    #[test]
    fn test_manager_get_and_set() {
        let (mut manager, _temp_dir) = create_temp_manager();

        // Test get on default settings
        assert_eq!(
            manager.get("model"),
            manager.settings.get_value("model")
        );
        assert_eq!(
            manager.get("tools_enabled"),
            Some(Value::Bool(true))
        );

        // Test set
        manager.set("model", "claude-opus-4-6").unwrap();
        assert_eq!(manager.settings.model, Some("claude-opus-4-6".to_string()));

        manager.set("temperature", "0.7").unwrap();
        assert_eq!(manager.settings.temperature, Some(0.7));

        manager.set("max_tokens", "8192").unwrap();
        assert_eq!(manager.settings.max_tokens, Some(8192));

        // Test set with JSON object
        manager.set("tools_enabled", "false").unwrap();
        assert!(!manager.settings.tools_enabled);
    }

    #[test]
    fn test_manager_merge() {
        let (mut manager, _temp_dir) = create_temp_manager();

        let project_settings = Settings {
            version: SETTINGS_VERSION.to_string(),
            model: Some("claude-opus-4-6".to_string()),
            temperature: Some(0.9),
            max_tokens: Some(16000),
            tools_enabled: false,
            permissions_mode: "readonly".to_string(),
            auto_memory: false,
            theme: "light".to_string(),
            permissions: PermissionRules::default(),
        };

        manager.merge(project_settings);

        assert_eq!(manager.settings.model, Some("claude-opus-4-6".to_string()));
        assert_eq!(manager.settings.temperature, Some(0.9));
        assert_eq!(manager.settings.max_tokens, Some(16000));
        assert!(!manager.settings.tools_enabled);
        assert_eq!(manager.settings.permissions_mode, "readonly");
        assert!(!manager.settings.auto_memory);
        assert_eq!(manager.settings.theme, "light");
    }

    #[test]
    fn test_manager_invalid_version() {
        let (mut manager, _temp_dir) = create_temp_manager();

        // Create a settings file with invalid version
        let invalid_json = r#"{
            "version": "0.1",
            "model": "claude-opus-4-6",
            "toolsEnabled": true
        }"#;

        fs::write(&manager.user_config_path, invalid_json).unwrap();

        let result = manager.load();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SettingsError::InvalidVersion { .. }));
    }

    #[test]
    fn test_manager_load_from_path() {
        let (mut manager, temp_dir) = create_temp_manager();

        let custom_path = temp_dir.path().join("custom_settings.json");
        let custom_json = r#"{
            "version": "1.0",
            "model": "claude-opus-4-6",
            "temperature": 0.8,
            "toolsEnabled": false,
            "permissionsMode": "auto",
            "autoMemory": false,
            "theme": "light"
        }"#;

        fs::write(&custom_path, custom_json).unwrap();

        manager.load_from_path(&custom_path).unwrap();

        assert_eq!(manager.settings.model, Some("claude-opus-4-6".to_string()));
        assert_eq!(manager.settings.temperature, Some(0.8));
        assert!(!manager.settings.tools_enabled);
    }

    #[test]
    fn test_manager_save_to_path() {
        let (manager, temp_dir) = create_temp_manager();

        let custom_path = temp_dir.path().join("custom_save.json");

        manager.save_to_path(&custom_path).unwrap();

        assert!(custom_path.exists());

        let content = fs::read_to_string(&custom_path).unwrap();
        let parsed: Value = serde_json::from_str(&content).unwrap();

        assert_eq!(parsed["version"], SETTINGS_VERSION);
        assert_eq!(parsed["toolsEnabled"], true);
    }

    #[test]
    fn test_settings_set_null_clears_optional() {
        let mut settings = Settings {
            version: SETTINGS_VERSION.to_string(),
            model: Some("claude-opus-4-6".to_string()),
            temperature: Some(0.7),
            max_tokens: Some(4096),
            tools_enabled: true,
            permissions_mode: "ask".to_string(),
            auto_memory: true,
            theme: "dark".to_string(),
            permissions: PermissionRules::default(),
        };

        // Clear optional fields with null
        settings.set_value("model", Value::Null).unwrap();
        assert_eq!(settings.model, None);

        settings.set_value("temperature", Value::Null).unwrap();
        assert_eq!(settings.temperature, None);

        settings.set_value("max_tokens", Value::Null).unwrap();
        assert_eq!(settings.max_tokens, None);

        // Non-optional fields remain unchanged
        assert!(settings.tools_enabled);
    }

    #[test]
    fn test_parse_env_content() {
        let (mut manager, _temp_dir) = create_temp_manager();

        let env_content = r#"
# Comment line
SHANNON_MODEL=gpt-4o
SHANNON_MAX_TOKENS=8192
SHANNON_TEMPERATURE=0.9
SHANNON_PERMISSIONS_MODE=auto
SHANNON_THEME=light
"#;

        manager.parse_env_content(env_content).unwrap();

        assert_eq!(manager.settings.model, Some("gpt-4o".to_string()));
        assert_eq!(manager.settings.max_tokens, Some(8192));
        assert_eq!(manager.settings.temperature, Some(0.9));
        assert_eq!(manager.settings.permissions_mode, "auto");
        assert_eq!(manager.settings.theme, "light");
    }

    #[test]
    fn test_parse_env_content_with_quotes() {
        let (mut manager, _temp_dir) = create_temp_manager();

        let env_content = r#"SHANNON_MODEL="gpt-4o"
SHANNON_THEME='dark'"#;

        manager.parse_env_content(env_content).unwrap();

        assert_eq!(manager.settings.model, Some("gpt-4o".to_string()));
        assert_eq!(manager.settings.theme, "dark");
    }

    #[test]
    fn test_parse_env_content_ignores_unknown() {
        let (mut manager, _temp_dir) = create_temp_manager();

        let env_content = "UNKNOWN_KEY=value\nSHANNON_MODEL=my-model\n";

        manager.parse_env_content(env_content).unwrap();

        assert_eq!(manager.settings.model, Some("my-model".to_string()));
    }

    #[test]
    fn test_apply_env_overrides() {
        let (mut manager, _temp_dir) = create_temp_manager();

        let overrides = vec![
            "SHANNON_MODEL=claude-opus-4-6".to_string(),
            "SHANNON_MAX_TOKENS=16000".to_string(),
            "SHANNON_TEMPERATURE=0.3".to_string(),
        ];

        manager.apply_env_overrides(&overrides).unwrap();

        assert_eq!(manager.settings.model, Some("claude-opus-4-6".to_string()));
        assert_eq!(manager.settings.max_tokens, Some(16000));
        assert_eq!(manager.settings.temperature, Some(0.3));
    }

    #[test]
    fn test_apply_env_overrides_rejects_invalid() {
        let (mut manager, _temp_dir) = create_temp_manager();

        let overrides = vec![
            "SHANNON_MODEL=valid-model".to_string(),
            "SHANNON_TEMPERATURE=2.0".to_string(), // invalid
        ];

        let result = manager.apply_env_overrides(&overrides);
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_env_var_individual() {
        let (mut manager, _temp_dir) = create_temp_manager();

        manager.apply_env_var("MODEL", "test-model");
        assert_eq!(manager.settings.model, Some("test-model".to_string()));

        manager.apply_env_var("MAX_TOKENS", "4096");
        assert_eq!(manager.settings.max_tokens, Some(4096));

        manager.apply_env_var("TEMPERATURE", "0.5");
        assert_eq!(manager.settings.temperature, Some(0.5));

        manager.apply_env_var("PERMISSIONS_MODE", "auto");
        assert_eq!(manager.settings.permissions_mode, "auto");

        manager.apply_env_var("THEME", "light");
        assert_eq!(manager.settings.theme, "light");

        manager.apply_env_var("AUTO_MEMORY", "false");
        assert!(!manager.settings.auto_memory);

        manager.apply_env_var("TOOLS_ENABLED", "false");
        assert!(!manager.settings.tools_enabled);

        // Invalid values should be ignored
        manager.apply_env_var("PERMISSIONS_MODE", "invalid");
        assert_eq!(manager.settings.permissions_mode, "auto"); // unchanged
    }

    #[test]
    fn test_load_from_dotenv_file() {
        let (mut manager, _temp_dir) = create_temp_manager();
        let temp_dir = _temp_dir;

        // Create a .env file
        let env_path = temp_dir.path().join(".env");
        let env_content = "SHANNON_MODEL=ollama-llama3\nSHANNON_MAX_TOKENS=4096\n";
        std::fs::write(&env_path, env_content).unwrap();

        // Change to temp dir to find the .env file
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();
        let result = manager.load_from_dotenv();
        std::env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        assert_eq!(manager.settings.model, Some("ollama-llama3".to_string()));
        assert_eq!(manager.settings.max_tokens, Some(4096));
    }

    #[test]
    fn test_env_priority_helper() {
        // SAFETY: env var operations before any threads spawn in tests
        unsafe {
            std::env::remove_var("SHANNON_MODEL_TEST");
            std::env::remove_var("ANTHROPIC_MODEL_TEST");
            std::env::remove_var("OPENAI_MODEL_TEST");
        }

        // No vars set
        assert!(env_priority(&["SHANNON_MODEL_TEST", "ANTHROPIC_MODEL_TEST", "OPENAI_MODEL_TEST"]).is_none());

        // Set lowest priority
        unsafe { std::env::set_var("OPENAI_MODEL_TEST", "gpt-4o"); }
        assert_eq!(
            env_priority(&["SHANNON_MODEL_TEST", "ANTHROPIC_MODEL_TEST", "OPENAI_MODEL_TEST"]),
            Some("gpt-4o".to_string())
        );

        // Set middle priority (should win)
        unsafe { std::env::set_var("ANTHROPIC_MODEL_TEST", "claude-sonnet-4"); }
        assert_eq!(
            env_priority(&["SHANNON_MODEL_TEST", "ANTHROPIC_MODEL_TEST", "OPENAI_MODEL_TEST"]),
            Some("claude-sonnet-4".to_string())
        );

        // Set highest priority (should win)
        unsafe { std::env::set_var("SHANNON_MODEL_TEST", "my-model"); }
        assert_eq!(
            env_priority(&["SHANNON_MODEL_TEST", "ANTHROPIC_MODEL_TEST", "OPENAI_MODEL_TEST"]),
            Some("my-model".to_string())
        );

        // Clean up
        unsafe {
            std::env::remove_var("SHANNON_MODEL_TEST");
            std::env::remove_var("ANTHROPIC_MODEL_TEST");
            std::env::remove_var("OPENAI_MODEL_TEST");
        }
    }

    // ── Three-layer settings merge tests ────────────────────────────────

    #[test]
    fn test_three_layer_settings_merge() {
        let (mut manager, _temp_dir) = create_temp_manager();

        let user_json = r#"{"version": "1.0", "model": "claude-sonnet-4", "theme": "dark", "permissions": {"allow": ["Read(*)"]}}"#;
        fs::write(&manager.user_config_path, user_json).unwrap();

        let project_json = r#"{"version": "1.0", "model": "claude-opus-4", "theme": "light", "permissions": {"allow": ["Bash(git *)"], "ask": ["Bash(rm *)"]}}"#;
        fs::write(&manager.project_config_path, project_json).unwrap();

        let local_json = r#"{"version": "1.0", "theme": "auto", "permissions": {"deny": ["Bash(rm -rf /)"]}}"#;
        fs::write(&manager.local_config_path, local_json).unwrap();

        manager.load_from_files().unwrap();

        assert_eq!(manager.settings.theme, "auto");
        assert_eq!(manager.settings.model, Some("claude-opus-4".to_string()));
        assert!(manager.settings.permissions.allow.contains(&"Read(*)".to_string()));
        assert!(manager.settings.permissions.allow.contains(&"Bash(git *)".to_string()));
        assert!(manager.settings.permissions.ask.contains(&"Bash(rm *)".to_string()));
        assert!(manager.settings.permissions.deny.contains(&"Bash(rm -rf /)".to_string()));
    }

    #[test]
    fn test_local_settings_override_project_settings() {
        let (mut manager, _temp_dir) = create_temp_manager();

        let project_json = r#"{"version": "1.0", "model": "claude-opus-4", "toolsEnabled": false}"#;
        fs::write(&manager.project_config_path, project_json).unwrap();

        let local_json = r#"{"version": "1.0", "toolsEnabled": true}"#;
        fs::write(&manager.local_config_path, local_json).unwrap();

        manager.load_from_files().unwrap();
        assert_eq!(manager.settings.model, Some("claude-opus-4".to_string()));
        assert!(manager.settings.tools_enabled);
    }

    #[test]
    fn test_permission_rules_default_empty() {
        let rules = PermissionRules::default();
        assert!(rules.is_empty());
        assert!(rules.deny.is_empty());
        assert!(rules.ask.is_empty());
        assert!(rules.allow.is_empty());
    }

    #[test]
    fn test_permission_rules_merge() {
        let mut rules = PermissionRules::new();
        rules.deny.push("Bash(rm -rf /)".to_string());
        rules.allow.push("Read(*)".to_string());

        let other = PermissionRules {
            deny: vec!["Bash(dd *)".to_string()],
            ask: vec!["Bash(sudo *)".to_string()],
            allow: vec!["Bash(git *)".to_string()],
        };
        rules.merge(other);

        assert_eq!(rules.deny, vec!["Bash(rm -rf /)", "Bash(dd *)"]);
        assert_eq!(rules.ask, vec!["Bash(sudo *)"]);
        assert_eq!(rules.allow, vec!["Read(*)", "Bash(git *)"]);
    }

    #[test]
    fn test_settings_without_permissions_field() {
        let (mut manager, _temp_dir) = create_temp_manager();

        let user_json = r#"{"version": "1.0", "model": "claude-sonnet-4"}"#;
        fs::write(&manager.user_config_path, user_json).unwrap();

        manager.load_from_files().unwrap();
        assert_eq!(manager.settings.model, Some("claude-sonnet-4".to_string()));
        assert!(manager.settings.permissions.is_empty());
    }

    #[test]
    fn test_local_settings_only() {
        let (mut manager, _temp_dir) = create_temp_manager();

        let local_json = r#"{"version": "1.0", "model": "local-model", "permissions": {"deny": ["Bash(*)"]}}"#;
        fs::write(&manager.local_config_path, local_json).unwrap();

        manager.load_from_files().unwrap();
        assert_eq!(manager.settings.model, Some("local-model".to_string()));
        assert_eq!(manager.settings.permissions.deny, vec!["Bash(*)"]);
    }

    #[test]
    fn test_local_config_path_is_set() {
        let manager = SettingsManager::new();
        assert!(manager.local_config_path.to_string_lossy().contains("settings.local.json"));
    }
}
