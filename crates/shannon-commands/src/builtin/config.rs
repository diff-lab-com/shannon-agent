//! /config command - Manage configuration settings

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Config prompt template
const CONFIG_PROMPT: &str = r##"
Manage Shannon Code configuration settings.

Arguments: {args}

Subcommands:
- **list** — Show all configuration keys, types, and defaults
- **get <key>** — Get the current value of a config key (model, max_tokens, temperature, timeout, debug, provider)
- **set <key> <value>** — Set a config key to a new value (validates type: integer, float, boolean, string)
- **reset <key>** — Reset a key to its default value
- **help** — Show usage information

Known keys: model (string), max_tokens (integer), temperature (float), timeout (integer), debug (boolean), provider (string)

If no subcommand is given, default to listing all settings.
"##;

/// Create the /config command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "config".to_string(),
            aliases: vec!["cfg".to_string(), "settings".to_string()],
            description: "View and manage configuration settings".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[get|set|list|reset] [key] [value]".to_string()),
            when_to_use: Some(
                "Use to view or modify configuration values such as model, temperature, max_tokens".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "".to_string(),
        content_length: 2000,
        arg_names: vec!["action".to_string(), "key".to_string(), "value".to_string()],
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(CONFIG_PROMPT.to_string()),
    }))
}

/// Configuration actions
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigAction {
    /// Get a config value
    Get,
    /// Set a config value
    Set,
    /// List all config values
    List,
    /// Reset a config value to default
    Reset,
    /// Show help
    Help,
}

/// Parse config action from argument string
pub fn parse_config_action(arg: &str) -> ConfigAction {
    match arg.to_lowercase().as_str() {
        "get" => ConfigAction::Get,
        "set" => ConfigAction::Set,
        "list" | "ls" | "show" => ConfigAction::List,
        "reset" | "unset" | "default" => ConfigAction::Reset,
        "help" | "?" => ConfigAction::Help,
        _ => ConfigAction::List,
    }
}

/// Known configuration keys with their descriptions and default values
pub struct ConfigKey {
    pub key: &'static str,
    pub description: &'static str,
    pub default: &'static str,
    pub value_type: &'static str,
}

/// Returns all known configuration keys
pub fn known_config_keys() -> Vec<ConfigKey> {
    vec![
        ConfigKey {
            key: "model",
            description: "Default AI model to use",
            default: "claude-sonnet-4-6",
            value_type: "string",
        },
        ConfigKey {
            key: "max_tokens",
            description: "Maximum tokens for responses",
            default: "4096",
            value_type: "integer",
        },
        ConfigKey {
            key: "temperature",
            description: "Response randomness (0.0-1.0)",
            default: "0.7",
            value_type: "float",
        },
        ConfigKey {
            key: "timeout",
            description: "Request timeout in seconds",
            default: "120",
            value_type: "integer",
        },
        ConfigKey {
            key: "debug",
            description: "Enable debug logging",
            default: "false",
            value_type: "boolean",
        },
        ConfigKey {
            key: "provider",
            description: "AI provider (anthropic, openai, local)",
            default: "anthropic",
            value_type: "string",
        },
    ]
}

/// Format config list output
pub fn format_config_list() -> String {
    let keys = known_config_keys();
    let mut output = String::from("Configuration Settings:\n\n");

    for key in &keys {
        output.push_str(&format!(
            "  {} ({}) - {}\n    Default: {}\n\n",
            key.key, key.value_type, key.description, key.default
        ));
    }

    output.push_str("\nUsage:\n");
    output.push_str("  /config list          - Show all settings\n");
    output.push_str("  /config get <key>     - Get a specific value\n");
    output.push_str("  /config set <key> <value> - Set a value\n");
    output.push_str("  /config reset <key>   - Reset to default\n");

    output
}

/// Format a config get response
pub fn format_config_get(key: &str) -> String {
    let keys = known_config_keys();
    match keys.iter().find(|k| k.key == key) {
        Some(k) => format!(
            "{} = {} (default: {})\nDescription: {}",
            k.key, k.value_type, k.default, k.description
        ),
        None => format!("Unknown config key: '{}'\nKnown keys: {}", key,
            keys.iter().map(|k| k.key).collect::<Vec<_>>().join(", ")),
    }
}

/// Format a config set response
pub fn format_config_set(key: &str, value: &str) -> String {
    let keys = known_config_keys();
    match keys.iter().find(|k| k.key == key) {
        Some(k) => {
            // Basic type validation
            let valid = match k.value_type {
                "integer" => value.parse::<i64>().is_ok(),
                "float" => value.parse::<f64>().is_ok(),
                "boolean" => value == "true" || value == "false",
                _ => true,
            };
            if valid {
                format!("Set {} = {} (was: {})", key, value, k.default)
            } else {
                format!("Invalid value '{}' for key '{}' (expected: {})", value, key, k.value_type)
            }
        }
        None => format!("Unknown config key: '{key}'"),
    }
}

/// Format a config reset response
pub fn format_config_reset(key: &str) -> String {
    let keys = known_config_keys();
    match keys.iter().find(|k| k.key == key) {
        Some(k) => format!("Reset {} to default: {}", key, k.default),
        None => format!("Unknown config key: '{key}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config_action() {
        assert_eq!(parse_config_action("get"), ConfigAction::Get);
        assert_eq!(parse_config_action("set"), ConfigAction::Set);
        assert_eq!(parse_config_action("list"), ConfigAction::List);
        assert_eq!(parse_config_action("ls"), ConfigAction::List);
        assert_eq!(parse_config_action("reset"), ConfigAction::Reset);
        assert_eq!(parse_config_action("help"), ConfigAction::Help);
        assert_eq!(parse_config_action("unknown"), ConfigAction::List);
    }

    #[test]
    fn test_format_config_list() {
        let output = format_config_list();
        assert!(output.contains("model"));
        assert!(output.contains("max_tokens"));
        assert!(output.contains("temperature"));
        assert!(output.contains("/config list"));
    }

    #[test]
    fn test_format_config_get() {
        let output = format_config_get("model");
        assert!(output.contains("model"));
        assert!(output.contains("claude-sonnet"));

        let unknown = format_config_get("nonexistent");
        assert!(unknown.contains("Unknown"));
    }

    #[test]
    fn test_format_config_set_validation() {
        // Valid integer
        let valid = format_config_set("max_tokens", "8192");
        assert!(valid.contains("Set"));

        // Invalid integer
        let invalid = format_config_set("max_tokens", "abc");
        assert!(invalid.contains("Invalid"));

        // Valid boolean
        let bool_valid = format_config_set("debug", "true");
        assert!(bool_valid.contains("Set"));

        // Invalid boolean
        let bool_invalid = format_config_set("debug", "yes");
        assert!(bool_invalid.contains("Invalid"));
    }

    #[test]
    fn test_known_config_keys() {
        let keys = known_config_keys();
        assert!(keys.len() >= 6);
        assert!(keys.iter().any(|k| k.key == "model"));
        assert!(keys.iter().any(|k| k.key == "temperature"));
    }
}
