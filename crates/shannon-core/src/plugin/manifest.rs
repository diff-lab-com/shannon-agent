//! Plugin manifest definition

use serde::{Deserialize, Serialize};
use std::str;

/// Plugin manifest (plugin.toml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin name (unique identifier)
    pub name: String,

    /// Plugin version (semver)
    pub version: String,

    /// Short description
    pub description: String,

    /// Optional author
    pub author: Option<String>,

    /// Optional repository URL
    pub repository: Option<String>,

    /// Plugin type: "tool", "command", or "skill"
    #[serde(rename = "type")]
    pub plugin_type: String,

    /// Entry point path (relative to plugin directory)
    pub entry: String,

    /// Transport config for tool plugins
    pub transport: Option<TransportConfig>,

    /// Command name for command plugins
    pub command_name: Option<String>,

    /// Command description for command plugins
    pub command_description: Option<String>,

    /// Trigger pattern for skill plugins
    pub trigger: Option<String>,

    /// Template for skill plugins
    pub template: Option<String>,

    /// Required permissions
    #[serde(default)]
    pub permissions: Vec<PluginPermission>,

    /// Optional minimum Shannon version
    pub min_shannon_version: Option<String>,

    /// Optional license
    pub license: Option<String>,

    /// Optional keywords for search
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// Transport configuration for MCP tool plugins
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// Transport type: "stdio" or "sse"
    #[serde(rename = "type")]
    pub transport_type: String,

    /// Command to run (stdio transport)
    pub command: Option<String>,

    /// Arguments to pass (stdio transport)
    #[serde(default)]
    pub args: Vec<String>,

    /// Server URL (sse transport)
    pub url: Option<String>,
}

impl TransportConfig {
    /// Get the command (for stdio transport)
    pub fn command(&self) -> Option<&str> {
        self.command.as_deref()
    }

    /// Get the args (for stdio transport)
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Check if this is a stdio transport
    pub fn is_stdio(&self) -> bool {
        self.transport_type == "stdio"
    }
}

/// Plugin permission
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PluginPermission {
    /// Read files from filesystem
    #[serde(rename = "read_files")]
    ReadFiles,

    /// Write files to filesystem
    #[serde(rename = "write_files")]
    WriteFiles,

    /// Execute shell commands
    #[serde(rename = "execute_commands")]
    ExecuteCommands,

    /// Network access
    #[serde(rename = "network")]
    Network,

    /// Access to MCP tools
    #[serde(rename = "mcp_tools")]
    McpTools,

    /// Access to LLM API
    #[serde(rename = "llm_api")]
    LlmApi,
}

/// Typed plugin kind, derived from the manifest fields
#[derive(Debug, Clone)]
pub enum PluginKind {
    /// MCP server tool
    Tool { transport: TransportConfig },
    /// Slash command extension
    Command { name: String, description: String },
    /// Skill/prompt template
    Skill { trigger: String, template: String },
}

impl PluginManifest {
    /// Parse manifest from TOML string
    pub fn from_toml(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    /// Parse manifest from TOML bytes
    pub fn from_toml_bytes(bytes: &[u8]) -> Result<Self, String> {
        let s = str::from_utf8(bytes)
            .map_err(|e| e.to_string())?;
        toml::from_str(s)
            .map_err(|e| e.to_string())
    }

    /// Get the typed plugin kind from the manifest fields
    pub fn kind(&self) -> Result<PluginKind, String> {
        match self.plugin_type.as_str() {
            "tool" => {
                let transport = self.transport.as_ref()
                    .ok_or_else(|| "tool plugin requires [transport] section".to_string())?;
                Ok(PluginKind::Tool { transport: transport.clone() })
            }
            "command" => {
                let name = self.command_name.as_ref()
                    .ok_or_else(|| "command plugin requires command_name".to_string())?;
                let desc = self.command_description.as_deref().unwrap_or("");
                Ok(PluginKind::Command {
                    name: name.clone(),
                    description: desc.to_string(),
                })
            }
            "skill" => {
                let trigger = self.trigger.as_ref()
                    .ok_or_else(|| "skill plugin requires trigger".to_string())?;
                let template = self.template.as_ref()
                    .ok_or_else(|| "skill plugin requires template".to_string())?;
                Ok(PluginKind::Skill {
                    trigger: trigger.clone(),
                    template: template.clone(),
                })
            }
            other => Err(format!("unknown plugin type: '{}'", other)),
        }
    }

    /// Get the display name for the plugin type
    pub fn type_display_name(&self) -> &'static str {
        match self.plugin_type.as_str() {
            "tool" => "Tool",
            "command" => "Command",
            "skill" => "Skill",
            _ => "Unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_TOOL_MANIFEST: &str = r#"
name = "example-plugin"
version = "1.0.0"
description = "An example plugin"
author = "Shannon Team"
repository = "https://github.com/shannon-code/example-plugin"
type = "tool"
entry = "src/main.rs"
permissions = ["read_files", "network"]
keywords = ["example", "demo"]

[transport]
type = "stdio"
command = "node"
args = ["index.js"]
"#;

    const SAMPLE_SKILL_MANIFEST: &str = r#"
name = "hello-skill"
version = "0.1.0"
description = "A hello skill"
type = "skill"
entry = "template.md"
trigger = "/hello"
template = "Hello {{name}}!"
"#;

    #[test]
    fn test_parse_tool_manifest() {
        let manifest = PluginManifest::from_toml(SAMPLE_TOOL_MANIFEST).unwrap();
        assert_eq!(manifest.name, "example-plugin");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.description, "An example plugin");
        assert_eq!(manifest.author, Some("Shannon Team".to_string()));
        assert!(manifest.permissions.contains(&PluginPermission::ReadFiles));
        assert!(manifest.permissions.contains(&PluginPermission::Network));
    }

    #[test]
    fn test_tool_kind() {
        let manifest = PluginManifest::from_toml(SAMPLE_TOOL_MANIFEST).unwrap();
        let kind = manifest.kind().unwrap();
        match kind {
            PluginKind::Tool { transport } => {
                assert!(transport.is_stdio());
                assert_eq!(transport.command().unwrap(), "node");
                assert_eq!(transport.args(), &["index.js".to_string()]);
            }
            _ => panic!("Expected Tool kind"),
        }
    }

    #[test]
    fn test_skill_manifest() {
        let manifest = PluginManifest::from_toml(SAMPLE_SKILL_MANIFEST).unwrap();
        assert_eq!(manifest.name, "hello-skill");
        assert_eq!(manifest.type_display_name(), "Skill");

        let kind = manifest.kind().unwrap();
        match kind {
            PluginKind::Skill { trigger, template } => {
                assert_eq!(trigger, "/hello");
                assert_eq!(template, "Hello {{name}}!");
            }
            _ => panic!("Expected Skill kind"),
        }
    }
}
