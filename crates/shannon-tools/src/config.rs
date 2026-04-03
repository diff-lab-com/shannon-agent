//! # ConfigTool
//!
//! Interactive configuration management for runtime settings.
//!
//! Provides a layered configuration system with:
//! - Dot-notation nested key support (e.g., "editor.theme")
//! - Type-safe get/set operations
//! - Config persistence to `~/.shannon/config.json`
//! - Default value management
//! - Change notification callbacks

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Configuration action
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigAction {
    /// Get a config value
    Get { key: String },
    /// Set a config value
    Set {
        key: String,
        value: Value,
    },
    /// List config values with optional prefix filter
    List { prefix: Option<String> },
    /// Delete a config value
    Delete { key: String },
    /// Reset a config value to its default
    Reset { key: String },
}

/// Input for the ConfigTool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigInput {
    /// The action to perform
    pub action: ConfigAction,
}

/// Change notification callback type
type ChangeCallback = Box<dyn Fn(&str, &Value) + Send + Sync>;

/// ConfigManager handles storage, retrieval, and persistence of configuration values.
pub struct ConfigManager {
    config_path: PathBuf,
    values: HashMap<String, Value>,
    defaults: HashMap<String, Value>,
    on_change: Option<ChangeCallback>,
}

impl std::fmt::Debug for ConfigManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigManager")
            .field("config_path", &self.config_path)
            .field("values", &self.values)
            .field("defaults", &self.defaults)
            .field("on_change", &self.on_change.as_ref().map(|_| "Some(callback)"))
            .finish()
    }
}

impl ConfigManager {
    /// Create a new ConfigManager with the default config path.
    pub fn new() -> Self {
        let config_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".shannon")
            .join("config.json");

        Self {
            config_path,
            values: HashMap::new(),
            defaults: HashMap::new(),
            on_change: None,
        }
    }

    /// Create a ConfigManager with a specific config path (for testing).
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            config_path: path,
            values: HashMap::new(),
            defaults: HashMap::new(),
            on_change: None,
        }
    }

    /// Set a change notification callback.
    pub fn set_on_change(&mut self, callback: ChangeCallback) {
        self.on_change = Some(callback);
    }

    /// Get a config value by key. Supports dot notation for nested keys.
    /// Returns a clone of the value if found.
    pub fn get(&self, key: &str) -> Option<Value> {
        // Check user values first, then defaults
        if let Some(val) = self.values.get(key) {
            return Some(val.clone());
        }
        self.defaults.get(key).cloned()
    }

    /// Set a config value by key.
    pub fn set(&mut self, key: String, value: Value) {
        self.values.insert(key.clone(), value.clone());
        if let Some(ref callback) = self.on_change {
            callback(&key, &value);
        }
    }

    /// Delete a config value by key.
    pub fn delete(&mut self, key: &str) -> bool {
        self.values.remove(key).is_some()
    }

    /// List all config keys, optionally filtered by prefix.
    pub fn list(&self, prefix: Option<&str>) -> Vec<String> {
        let all_keys: Vec<String> = self
            .values
            .keys()
            .chain(self.defaults.keys())
            .filter(|k| {
                if let Some(p) = prefix {
                    k.starts_with(p)
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        // Deduplicate
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for key in all_keys {
            if seen.insert(key.clone()) {
                result.push(key);
            }
        }
        result.sort();
        result
    }

    /// Reset a config value to its default.
    pub fn reset(&mut self, key: &str) -> bool {
        if self.defaults.contains_key(key) {
            self.values.remove(key);
            true
        } else {
            false
        }
    }

    /// Set a default value for a key.
    pub fn set_default(&mut self, key: String, value: Value) {
        self.defaults.insert(key, value);
    }

    /// Save the current config to disk.
    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        let data = json!({
            "values": self.values,
            "defaults": self.defaults,
        });

        let serialized = serde_json::to_string_pretty(&data)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        std::fs::write(&self.config_path, serialized)
            .map_err(|e| format!("Failed to write config file: {}", e))?;

        Ok(())
    }

    /// Load config from disk.
    pub fn load(&mut self) -> Result<(), String> {
        if !self.config_path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.config_path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        let data: Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;

        if let Some(values) = data.get("values").and_then(|v| v.as_object()) {
            self.values = values
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
        }

        if let Some(defaults) = data.get("defaults").and_then(|v| v.as_object()) {
            self.defaults = defaults
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
        }

        Ok(())
    }

    /// Get the config file path.
    pub fn config_path(&self) -> &PathBuf {
        &self.config_path
    }
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared config manager state
pub type SharedConfigManager = Arc<Mutex<ConfigManager>>;

/// ConfigTool provides interactive configuration management.
pub struct ConfigTool {
    description: String,
    manager: SharedConfigManager,
}

impl ConfigTool {
    pub fn new() -> Self {
        Self {
            description: "Manage runtime configuration with get, set, list, delete, and reset operations".to_string(),
            manager: Arc::new(Mutex::new(ConfigManager::new())),
        }
    }

    /// Create a ConfigTool with a specific config manager (for testing or shared state).
    pub fn with_manager(manager: SharedConfigManager) -> Self {
        Self {
            description: "Manage runtime configuration with get, set, list, delete, and reset operations".to_string(),
            manager,
        }
    }

    /// Create a ConfigTool with a specific config path (for testing).
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            description: "Manage runtime configuration with get, set, list, delete, and reset operations".to_string(),
            manager: Arc::new(Mutex::new(ConfigManager::with_path(path))),
        }
    }

    /// Execute a config action.
    fn execute_action(&self, action: ConfigAction) -> Result<(String, HashMap<String, Value>), ToolError> {
        let mut manager = self.manager.lock().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire config lock: {}", e))
        })?;

        match action {
            ConfigAction::Get { key } => {
                let value = manager.get(&key).ok_or_else(|| {
                    ToolError::InvalidInput(format!("Config key not found: {}", key))
                })?;

                let mut metadata = HashMap::new();
                metadata.insert("key".to_string(), json!(key));
                metadata.insert("value".to_string(), value.clone());
                metadata.insert("type".to_string(), json!(value_type_name(&value)));

                let content = format!("{}: {}", key, format_value(&value));
                Ok((content, metadata))
            }

            ConfigAction::Set { key, value } => {
                manager.set(key.clone(), value.clone());

                let mut metadata = HashMap::new();
                metadata.insert("key".to_string(), json!(key));
                metadata.insert("value".to_string(), value.clone());
                metadata.insert("type".to_string(), json!(value_type_name(&value)));

                let content = format!("Set {} = {}", key, format_value(&value));
                Ok((content, metadata))
            }

            ConfigAction::List { prefix } => {
                let keys = manager.list(prefix.as_deref());

                let mut metadata = HashMap::new();
                let key_values: Vec<Value> = keys
                    .iter()
                    .map(|k| {
                        json!({
                            "key": k,
                            "value": manager.get(k).unwrap_or(Value::Null)
                        })
                    })
                    .collect();
                metadata.insert("keys".to_string(), json!(keys));
                metadata.insert("count".to_string(), json!(keys.len()));
                metadata.insert("values".to_string(), json!(key_values));

                let prefix_info = prefix
                    .map(|p| format!(" with prefix '{}'", p))
                    .unwrap_or_default();
                let content = if keys.is_empty() {
                    format!("No config keys found{}", prefix_info)
                } else {
                    format!(
                        "Found {} config key(s){}: {}",
                        keys.len(),
                        prefix_info,
                        keys.join(", ")
                    )
                };
                Ok((content, metadata))
            }

            ConfigAction::Delete { key } => {
                let existed = manager.delete(&key);

                let mut metadata = HashMap::new();
                metadata.insert("key".to_string(), json!(key));
                metadata.insert("deleted".to_string(), json!(existed));

                let content = if existed {
                    format!("Deleted config key: {}", key)
                } else {
                    format!("Config key not found: {}", key)
                };
                Ok((content, metadata))
            }

            ConfigAction::Reset { key } => {
                let existed = manager.reset(&key);

                let mut metadata = HashMap::new();
                metadata.insert("key".to_string(), json!(key));
                metadata.insert("reset".to_string(), json!(existed));

                if existed {
                    let value = manager.get(&key).unwrap_or(Value::Null);
                    metadata.insert("value".to_string(), value.clone());
                    let content = format!("Reset {} to default: {}", key, format_value(&value));
                    Ok((content, metadata))
                } else {
                    let content = format!("No default found for key: {}", key);
                    Ok((content, metadata))
                }
            }
        }
    }
}

impl Default for ConfigTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Get a human-readable type name for a JSON value.
fn value_type_name(value: &Value) -> &str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => {
            if value.is_i64() {
                "integer"
            } else {
                "number"
            }
        }
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Format a JSON value for human-readable display.
fn format_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            if arr.len() <= 3 {
                format!("[{}]", arr.iter().map(|v| format_value(v)).collect::<Vec<_>>().join(", "))
            } else {
                format!("[{} items]", arr.len())
            }
        }
        Value::Object(obj) => {
            if obj.len() <= 3 {
                let pairs: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, format_value(v)))
                    .collect();
                format!("{{{}}}", pairs.join(", "))
            } else {
                format!("{{{}}}", obj.len())
            }
        }
    }
}

#[async_trait]
impl Tool for ConfigTool {
    fn name(&self) -> &str {
        "Config"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "object",
                    "description": "The configuration action to perform",
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "get": { "type": "object", "properties": { "key": { "type": "string", "description": "Config key to get" } }, "required": ["key"] }
                            },
                            "required": ["get"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "set": { "type": "object", "properties": { "key": { "type": "string", "description": "Config key to set" }, "value": { "description": "Value to set" } }, "required": ["key", "value"] }
                            },
                            "required": ["set"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "list": { "type": "object", "properties": { "prefix": { "type": "string", "description": "Optional key prefix filter" } } }
                            },
                            "required": ["list"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "delete": { "type": "object", "properties": { "key": { "type": "string", "description": "Config key to delete" } }, "required": ["key"] }
                            },
                            "required": ["delete"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "reset": { "type": "object", "properties": { "key": { "type": "string", "description": "Config key to reset to default" } }, "required": ["key"] }
                            },
                            "required": ["reset"]
                        }
                    ]
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let config_input: ConfigInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid config input: {}", e)))?;

        let (content, metadata) = self.execute_action(config_input.action)?;

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_tool() -> ConfigTool {
        ConfigTool::with_path(TempDir::new().unwrap().path().join("config.json"))
    }

    fn make_tool_with_defaults() -> ConfigTool {
        let tool = make_tool();
        {
            let mut manager = tool.manager.lock().unwrap();
            manager.set_default("editor.theme".to_string(), json!("dark"));
            manager.set_default("editor.font_size".to_string(), json!(14));
            manager.set_default("server.port".to_string(), json!(8080));
            manager.set_default("server.host".to_string(), json!("localhost"));
            manager.set_default("features.auto_save".to_string(), json!(true));
        }
        tool
    }

    // --- ConfigManager unit tests ---

    #[test]
    fn test_get_set_config_values() {
        let mut manager = ConfigManager::with_path(TempDir::new().unwrap().path().join("config.json"));

        manager.set("app.name".to_string(), json!("shannon"));
        manager.set("app.version".to_string(), json!("1.0.0"));

        assert_eq!(manager.get("app.name"), Some(json!("shannon")));
        assert_eq!(manager.get("app.version"), Some(json!("1.0.0")));
        assert_eq!(manager.get("nonexistent"), None);
    }

    #[test]
    fn test_delete_config_value() {
        let mut manager = ConfigManager::with_path(TempDir::new().unwrap().path().join("config.json"));

        manager.set("key".to_string(), json!("value"));
        assert_eq!(manager.get("key"), Some(json!("value")));

        let deleted = manager.delete("key");
        assert!(deleted);
        assert_eq!(manager.get("key"), None);

        let deleted_again = manager.delete("key");
        assert!(!deleted_again);
    }

    #[test]
    fn test_nested_key_dot_notation() {
        let mut manager = ConfigManager::with_path(TempDir::new().unwrap().path().join("config.json"));

        manager.set("editor.theme".to_string(), json!("dark"));
        manager.set("editor.font.size".to_string(), json!(14));
        manager.set("server.host".to_string(), json!("localhost"));
        manager.set("server.port".to_string(), json!(8080));

        assert_eq!(manager.get("editor.theme"), Some(json!("dark")));
        assert_eq!(manager.get("editor.font.size"), Some(json!(14)));
        assert_eq!(manager.get("server.host"), Some(json!("localhost")));
        assert_eq!(manager.get("server.port"), Some(json!(8080)));
    }

    #[test]
    fn test_default_values() {
        let mut manager = ConfigManager::with_path(TempDir::new().unwrap().path().join("config.json"));

        manager.set_default("theme".to_string(), json!("dark"));
        manager.set_default("font_size".to_string(), json!(14));

        // Getting a default value returns it
        assert_eq!(manager.get("theme"), Some(json!("dark")));
        assert_eq!(manager.get("font_size"), Some(json!(14)));

        // User values override defaults
        manager.set("theme".to_string(), json!("light"));
        assert_eq!(manager.get("theme"), Some(json!("light")));
    }

    #[test]
    fn test_list_with_prefix_filter() {
        let mut manager = ConfigManager::with_path(TempDir::new().unwrap().path().join("config.json"));

        manager.set("editor.theme".to_string(), json!("dark"));
        manager.set("editor.font_size".to_string(), json!(14));
        manager.set("server.port".to_string(), json!(8080));
        manager.set("server.host".to_string(), json!("localhost"));
        manager.set_default("editor.default_font".to_string(), json!("monospace"));

        // List all keys
        let all = manager.list(None);
        assert_eq!(all.len(), 5);

        // List with prefix filter
        let editor_keys = manager.list(Some("editor."));
        assert_eq!(editor_keys.len(), 3);
        assert!(editor_keys.contains(&"editor.theme".to_string()));
        assert!(editor_keys.contains(&"editor.font_size".to_string()));
        assert!(editor_keys.contains(&"editor.default_font".to_string()));

        let server_keys = manager.list(Some("server."));
        assert_eq!(server_keys.len(), 2);

        let nonexistent = manager.list(Some("nonexistent."));
        assert!(nonexistent.is_empty());
    }

    #[test]
    fn test_reset_to_default() {
        let mut manager = ConfigManager::with_path(TempDir::new().unwrap().path().join("config.json"));

        manager.set_default("theme".to_string(), json!("dark"));
        manager.set("theme".to_string(), json!("light"));

        assert_eq!(manager.get("theme"), Some(json!("light")));

        let reset = manager.reset("theme");
        assert!(reset);
        assert_eq!(manager.get("theme"), Some(json!("dark")));

        // Reset a key with no default
        let no_default = manager.reset("nonexistent");
        assert!(!no_default);
    }

    #[test]
    fn test_save_load_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("config.json");

        let mut manager = ConfigManager::with_path(path.clone());
        manager.set("app.name".to_string(), json!("shannon"));
        manager.set("app.version".to_string(), json!("2.0.0"));
        manager.set("server.port".to_string(), json!(9090));
        manager.set_default("theme".to_string(), json!("dark"));

        manager.save().unwrap();

        let mut manager2 = ConfigManager::with_path(path);
        manager2.load().unwrap();

        assert_eq!(manager2.get("app.name"), Some(json!("shannon")));
        assert_eq!(manager2.get("app.version"), Some(json!("2.0.0")));
        assert_eq!(manager2.get("server.port"), Some(json!(9090)));
        assert_eq!(manager2.get("theme"), Some(json!("dark")));
    }

    #[test]
    fn test_type_conversions() {
        let mut manager = ConfigManager::with_path(TempDir::new().unwrap().path().join("config.json"));

        manager.set("string_val".to_string(), json!("hello"));
        manager.set("int_val".to_string(), json!(42));
        manager.set("float_val".to_string(), json!(3.14));
        manager.set("bool_val".to_string(), json!(true));
        manager.set("array_val".to_string(), json!([1, 2, 3]));
        manager.set("object_val".to_string(), json!({"key": "value"}));

        assert_eq!(manager.get("string_val"), Some(json!("hello")));
        assert_eq!(manager.get("int_val"), Some(json!(42)));
        assert_eq!(manager.get("float_val"), Some(json!(3.14)));
        assert_eq!(manager.get("bool_val"), Some(json!(true)));
        assert_eq!(manager.get("array_val"), Some(json!([1, 2, 3])));
        assert_eq!(manager.get("object_val"), Some(json!({"key": "value"})));
    }

    #[test]
    fn test_invalid_key_handling() {
        let mut manager = ConfigManager::with_path(TempDir::new().unwrap().path().join("config.json"));

        assert_eq!(manager.get("nonexistent.key"), None);
        assert!(!manager.delete("nonexistent.key"));
        assert!(!manager.reset("nonexistent.key"));
    }

    #[test]
    fn test_config_change_callback() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("config.json");

        let changed_keys = Arc::new(Mutex::new(Vec::new()));
        let changed_keys_clone = changed_keys.clone();

        let mut manager = ConfigManager::with_path(path);
        manager.set_on_change(Box::new(move |key: &str, _value: &Value| {
            changed_keys_clone.lock().unwrap().push(key.to_string());
        }));

        manager.set("key1".to_string(), json!("value1"));
        manager.set("key2".to_string(), json!("value2"));

        let keys = changed_keys.lock().unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], "key1");
        assert_eq!(keys[1], "key2");
    }

    #[test]
    fn test_load_nonexistent_file() {
        let mut manager = ConfigManager::with_path(TempDir::new().unwrap().path().join("nonexistent.json"));
        // Should succeed without error
        let result = manager.load();
        assert!(result.is_ok());
    }

    // --- ConfigTool integration tests ---

    #[tokio::test]
    async fn test_tool_set_action() {
        let tool = make_tool();
        let input = json!({
            "action": {
                "set": { "key": "app.name", "value": "shannon" }
            }
        });

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Set app.name"));
        assert_eq!(result.metadata.get("key"), Some(&json!("app.name")));
    }

    #[tokio::test]
    async fn test_tool_get_action() {
        let tool = make_tool();

        // First set a value
        tool.execute(json!({
            "action": { "set": { "key": "theme", "value": "dark" } }
        })).await.unwrap();

        // Then get it
        let result = tool.execute(json!({
            "action": { "get": { "key": "theme" } }
        })).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("theme: dark"));
        assert_eq!(result.metadata.get("value"), Some(&json!("dark")));
    }

    #[tokio::test]
    async fn test_tool_get_nonexistent_key() {
        let tool = make_tool();
        let result = tool.execute(json!({
            "action": { "get": { "key": "nonexistent" } }
        })).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_tool_list_action() {
        let tool = make_tool();

        tool.execute(json!({
            "action": { "set": { "key": "editor.theme", "value": "dark" } }
        })).await.unwrap();
        tool.execute(json!({
            "action": { "set": { "key": "server.port", "value": 8080 } }
        })).await.unwrap();

        let result = tool.execute(json!({
            "action": { "list": {} }
        })).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("2 config key(s)"));
    }

    #[tokio::test]
    async fn test_tool_list_with_prefix() {
        let tool = make_tool_with_defaults();

        // Add some values
        tool.execute(json!({
            "action": { "set": { "key": "editor.theme", "value": "light" } }
        })).await.unwrap();

        let result = tool.execute(json!({
            "action": { "list": { "prefix": "editor." } }
        })).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("editor."));
    }

    #[tokio::test]
    async fn test_tool_delete_action() {
        let tool = make_tool();

        tool.execute(json!({
            "action": { "set": { "key": "temp.key", "value": "temp" } }
        })).await.unwrap();

        let result = tool.execute(json!({
            "action": { "delete": { "key": "temp.key" } }
        })).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("Deleted"));
    }

    #[tokio::test]
    async fn test_tool_reset_action() {
        let tool = make_tool_with_defaults();

        // Override a default
        tool.execute(json!({
            "action": { "set": { "key": "editor.theme", "value": "light" } }
        })).await.unwrap();

        let result = tool.execute(json!({
            "action": { "reset": { "key": "editor.theme" } }
        })).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("Reset"));
    }

    #[tokio::test]
    async fn test_tool_invalid_input() {
        let tool = make_tool();
        let result = tool.execute(json!({ "invalid": "data" })).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_name_and_description() {
        let tool = make_tool();
        assert_eq!(tool.name(), "Config");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_input_schema_validity() {
        let tool = make_tool();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["action"].is_object());
    }

    #[test]
    fn test_value_type_name() {
        assert_eq!(value_type_name(&json!("hello")), "string");
        assert_eq!(value_type_name(&json!(42)), "integer");
        assert_eq!(value_type_name(&json!(3.14)), "number");
        assert_eq!(value_type_name(&json!(true)), "boolean");
        assert_eq!(value_type_name(&json!(Value::Null)), "null");
        assert_eq!(value_type_name(&json!([1, 2])), "array");
        assert_eq!(value_type_name(&json!({"a": 1})), "object");
    }

    #[test]
    fn test_format_value() {
        assert_eq!(format_value(&json!("hello")), "hello");
        assert_eq!(format_value(&json!(42)), "42");
        assert_eq!(format_value(&json!(true)), "true");
        assert_eq!(format_value(&json!([1, 2, 3])), "[1, 2, 3]");
        assert_eq!(format_value(&json!([1, 2, 3, 4, 5])), "[5 items]");
    }

    #[test]
    fn test_config_manager_default() {
        let manager = ConfigManager::default();
        assert!(manager.config_path().to_string_lossy().contains(".shannon"));
    }
}
