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

    /// Manifest schema version (§4.10 W3-2).
    ///
    /// Absent / `1` = the historical v1 vocabulary; `"2"` opts the plugin
    /// into the tightened v2 contract (mcp references, hook subscriptions,
    /// permission-completeness enforcement at install time, version compat
    /// range). Parsing never rejects v1 shapes because of this field.
    #[serde(default)]
    pub manifest_version: ManifestVersion,

    /// v2: MCP servers bundled or proxied by this plugin.
    ///
    /// Shannon-native v2 writes an array of tables (`[[mcp]]`); the Claude
    /// ecosystem writes a top-level `mcpServers` object map — both parse
    /// into this list (`alias = "mcpServers"`).
    #[serde(
        default,
        alias = "mcpServers",
        deserialize_with = "deserialize_mcp_refs",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub mcp: Vec<McpServerRef>,

    /// v2: **reserved** hook-subscription declarations.
    ///
    /// Schema slot only — the runtime binding into `HookManager` lands with
    /// the MCP hook-subscription protocol (research doc item 121). Names are
    /// validated against [`shannon_engine::hooks::HookEventType`] at install
    /// time so typos surface immediately, not years later.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<HookSubscription>,

    /// v2: supported Shannon version range (e.g. `min = "0.10.0"`,
    /// exclusive `max = "0.12.0"`). Outside-range installs degrade to a
    /// loud warning, not a refusal — the author's optimism about future
    /// versions must not brick mid-version loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<CompatRange>,
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

/// Plugin permission — one capability face of the manifest allow-set.
///
/// A declared list grants exactly the faces it names at Shannon-side execution
/// points (see `plugin/permissions.rs` and `plugin/PERMISSIONS.md`); an empty
/// or omitted list is the pre-enforcement lenient default (allow-all).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum PluginPermission {
    /// Read files from filesystem
    #[serde(rename = "read_files")]
    ReadFiles,

    /// Write files to the filesystem. Declaring this face runs the plugin's
    /// stdio server processes inside a manifest-derived execution world
    /// (writable roots converge to the install dir + workspace; see
    /// `PERMISSIONS.md`), so the declaration is enforced by the OS, not just
    /// honored by host gates.
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
/// Wire names mirror the serde renames so denials and logs always quote the
/// exact token a plugin author writes in `plugin.toml`.
impl PluginPermission {
    /// The manifest string form of this permission (e.g. `"read_files"`).
    pub fn wire_name(&self) -> &'static str {
        match self {
            PluginPermission::ReadFiles => "read_files",
            PluginPermission::WriteFiles => "write_files",
            PluginPermission::ExecuteCommands => "execute_commands",
            PluginPermission::Network => "network",
            PluginPermission::McpTools => "mcp_tools",
            PluginPermission::LlmApi => "llm_api",
        }
    }
}

/// Manifest schema generation selected by the `manifest_version` key.
///
/// The **absent** key means v1: that is the whole backward-compatibility
/// story (§4.10 constraint — plugin.toml v1 and `.claude-plugin/plugin.json`
/// remain readable forever).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManifestVersion {
    /// Historical schema; everything optional beyond the base fields.
    #[default]
    #[serde(rename = "1")]
    V1,
    /// Tightened schema: mcp/hooks/compat available, permission completeness
    /// enforced at install time.
    #[serde(rename = "2")]
    V2,
}

impl ManifestVersion {
    /// Wire form written into manifests (`"1"` / `"2"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "1",
            Self::V2 => "2",
        }
    }
}

/// Default transport for MCP references omitted from shorthand rows.
fn default_mcp_transport() -> String {
    "stdio".to_string()
}

/// One MCP server referenced from a v2 manifest (`[[mcp]]` row or one
/// `mcpServers` map entry in the Claude dialect).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerRef {
    /// Server handle; becomes `<server_name>` inside `mcp__<server>__*` tools.
    pub name: String,

    /// Transport kind: `"stdio"` (default), `"sse"`, or `"http"`.
    #[serde(rename = "type", default = "default_mcp_transport")]
    pub transport_type: String,

    /// Executable command (stdio transports).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Command arguments (stdio transports).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,

    /// Remote endpoint (sse / http transports).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Deserialize the `mcp` / `mcpServers` field in either dialect:
///
/// - Shannon v2: sequence of full rows (the natural TOML array-of-tables
///   shape), each carrying its `name`;
/// - Claude ecosystem: object map `"<name>" -> {command|url|args|type}`,
///   where `type` may be omitted (inferred: `url` without `command` =
///   `"sse"`, otherwise `"stdio"`).
fn deserialize_mcp_refs<'de, D>(deserializer: D) -> Result<Vec<McpServerRef>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = Vec<McpServerRef>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a list of MCP references or an mcpServers-style object map")
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(r) = seq.next_element::<McpServerRef>()? {
                if r.name.trim().is_empty() {
                    return Err(serde::de::Error::custom(
                        "[[mcp]] reference is missing its 'name'",
                    ));
                }
                out.push(r);
            }
            Ok(out)
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            #[derive(serde::Deserialize)]
            struct Entry {
                #[serde(rename = "type", default)]
                transport_type: Option<String>,
                #[serde(default)]
                command: Option<String>,
                #[serde(default)]
                args: Vec<String>,
                #[serde(default)]
                url: Option<String>,
            }
            let mut out = Vec::new();
            while let Some((name, entry)) = map.next_entry::<String, Entry>()? {
                let transport_type = entry.transport_type.unwrap_or_else(|| {
                    if entry.url.is_some() && entry.command.is_none() {
                        "sse".to_string()
                    } else {
                        "stdio".to_string()
                    }
                });
                out.push(McpServerRef {
                    name,
                    transport_type,
                    command: entry.command,
                    args: entry.args,
                    url: entry.url,
                });
            }
            Ok(out)
        }
    }

    deserializer.deserialize_any(Visitor)
}

/// One **reserved** hook subscription declared by a v2 manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookSubscription {
    /// Hook event name in `HookEventType` spelling (PascalCase,
    /// e.g. `"PostToolUse"`).
    pub event: String,

    /// Handler artifact path relative to the plugin directory
    /// (e.g. `"hooks/lint.sh"`). Reserved — not yet executed.
    pub handler: String,
}

/// Supported Shannon version window declared by a v2 manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatRange {
    /// Inclusive minimum, e.g. `"0.10.0"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,

    /// Exclusive maximum, e.g. `"0.12.0"` means `< 0.12.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
}

impl CompatRange {
    /// Does `current` satisfy this range?
    ///
    /// Uses a deliberately tiny semver reader (numeric `MAJOR.MINOR.PATCH`,
    /// optional leading `v`, anything after `-` treated as pre-release and
    /// ignored) so plugin parsing pulls in zero dependencies.
    pub fn contains(&self, current: &str) -> bool {
        let Some(cur) = parse_semver(current) else {
            return false;
        };
        if let Some(min) = self.min.as_deref() {
            match parse_semver(min) {
                Some(m) if cur < m => return false,
                None => return false,
                _ => {}
            }
        }
        if let Some(max) = self.max.as_deref() {
            match parse_semver(max) {
                Some(m) if cur >= m => return false,
                None => return false,
                _ => {}
            }
        }
        true
    }
}

/// Read a loose semver triple; `None` when it is not `x.y.z[-suffix]`.
pub(crate) fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.trim().trim_start_matches('v');
    let core = core
        .split(['-', '+'])
        .next()
        .expect("split yields >=1 part");
    let mut it = core.split('.');
    let major = it.next()?.parse::<u64>().ok()?;
    let minor = it.next()?.parse::<u64>().ok()?;
    let patch = it.next()?.parse::<u64>().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
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
        let s = str::from_utf8(bytes).map_err(|e| e.to_string())?;
        toml::from_str(s).map_err(|e| e.to_string())
    }

    /// Parse manifest from a `.claude-plugin/plugin.json` string.
    ///
    /// This enables Shannon to load Claude Code ecosystem plugins directly
    /// without requiring a separate `plugin.toml`. Field names mirror the
    /// TOML form (snake_case) so the same in-memory representation works
    /// for both formats.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// Parse manifest from JSON bytes (e.g. `.claude-plugin/plugin.json`).
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, String> {
        let s = str::from_utf8(bytes).map_err(|e| e.to_string())?;
        serde_json::from_str(s).map_err(|e| e.to_string())
    }

    /// Schema generation of this manifest (v1 when the key is absent).
    pub fn schema_version(&self) -> ManifestVersion {
        self.manifest_version
    }

    /// Is this manifest inside its own declared compatibility window for
    /// Shannon `current`? Undeclared windows (both v1 `min_shannon_version`
    /// and v2 `[compat]`) always answer true.
    pub fn is_within_compat(&self, current: &str) -> bool {
        if let Some(range) = &self.compat {
            if !range.contains(current) {
                return false;
            }
        }
        true
    }

    /// Get the typed plugin kind from the manifest fields
    pub fn kind(&self) -> Result<PluginKind, String> {
        match self.plugin_type.as_str() {
            "tool" => {
                let transport = self
                    .transport
                    .as_ref()
                    .ok_or_else(|| "tool plugin requires [transport] section".to_string())?;
                Ok(PluginKind::Tool {
                    transport: transport.clone(),
                })
            }
            "command" => {
                let name = self
                    .command_name
                    .as_ref()
                    .ok_or_else(|| "command plugin requires command_name".to_string())?;
                let desc = self.command_description.as_deref().unwrap_or("");
                Ok(PluginKind::Command {
                    name: name.clone(),
                    description: desc.to_string(),
                })
            }
            "skill" => {
                let trigger = self
                    .trigger
                    .as_ref()
                    .ok_or_else(|| "skill plugin requires trigger".to_string())?;
                let template = self
                    .template
                    .as_ref()
                    .ok_or_else(|| "skill plugin requires template".to_string())?;
                Ok(PluginKind::Skill {
                    trigger: trigger.clone(),
                    template: template.clone(),
                })
            }
            "wasm" => Err(
                "plugin type 'wasm' is a reserved schema slot (master plan \u{a7}4.16 WASM pilot; deferred) \u{2014} this build cannot load it"
                    .to_string(),
            ),
            other => Err(format!("unknown plugin type: '{other}'")),
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
#[allow(clippy::unwrap_used)]
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

    #[test]
    fn test_command_manifest() {
        let toml = r#"
name = "my-cmd"
version = "1.0.0"
description = "A custom command"
type = "command"
entry = "cmd.md"
command_name = "review"
command_description = "Review code"
"#;
        let manifest = PluginManifest::from_toml(toml).unwrap();
        assert_eq!(manifest.type_display_name(), "Command");
        let kind = manifest.kind().unwrap();
        match kind {
            PluginKind::Command { name, description } => {
                assert_eq!(name, "review");
                assert_eq!(description, "Review code");
            }
            _ => panic!("Expected Command kind"),
        }
    }

    #[test]
    fn test_command_kind_missing_name() {
        let toml = r#"
name = "broken"
version = "1.0.0"
description = "Missing command_name"
type = "command"
entry = "x.md"
"#;
        let manifest = PluginManifest::from_toml(toml).unwrap();
        assert!(manifest.kind().is_err());
    }

    #[test]
    fn test_skill_kind_missing_trigger() {
        let toml = r#"
name = "broken"
version = "1.0.0"
description = "Missing trigger"
type = "skill"
entry = "x.md"
template = "hello"
"#;
        let manifest = PluginManifest::from_toml(toml).unwrap();
        assert!(manifest.kind().is_err());
    }

    #[test]
    fn test_unknown_plugin_type() {
        let toml = r#"
name = "bad"
version = "1.0.0"
description = "Unknown type"
type = "widget"
entry = "x.md"
"#;
        let manifest = PluginManifest::from_toml(toml).unwrap();
        let err = manifest.kind().unwrap_err();
        assert!(err.contains("unknown plugin type"));
    }

    #[test]
    fn test_tool_kind_missing_transport() {
        let toml = r#"
name = "bad-tool"
version = "1.0.0"
description = "Missing transport"
type = "tool"
entry = "x.md"
"#;
        let manifest = PluginManifest::from_toml(toml).unwrap();
        let err = manifest.kind().unwrap_err();
        assert!(err.contains("transport"));
    }

    #[test]
    fn test_sse_transport() {
        let toml = r#"
name = "remote-tool"
version = "1.0.0"
description = "SSE transport"
type = "tool"
entry = "x.md"

[transport]
type = "sse"
url = "http://localhost:8080/sse"
"#;
        let manifest = PluginManifest::from_toml(toml).unwrap();
        let kind = manifest.kind().unwrap();
        match kind {
            PluginKind::Tool { transport } => {
                assert!(!transport.is_stdio());
                assert!(transport.command().is_none());
            }
            _ => panic!("Expected Tool kind"),
        }
    }

    #[test]
    fn test_from_toml_bytes() {
        let toml_str = r#"
name = "bytes-test"
version = "2.0.0"
description = "From bytes"
type = "skill"
entry = "x.md"
trigger = "/test"
template = "ok"
"#;
        let manifest = PluginManifest::from_toml_bytes(toml_str.as_bytes()).unwrap();
        assert_eq!(manifest.name, "bytes-test");
    }

    #[test]
    fn test_from_toml_bytes_invalid_utf8() {
        let bad_bytes: &[u8] = &[0xff, 0xfe, 0x00];
        assert!(PluginManifest::from_toml_bytes(bad_bytes).is_err());
    }

    #[test]
    fn test_command_default_description() {
        let toml = r#"
name = "cmd-no-desc"
version = "1.0.0"
description = "No desc"
type = "command"
entry = "x.md"
command_name = "build"
"#;
        let manifest = PluginManifest::from_toml(toml).unwrap();
        let kind = manifest.kind().unwrap();
        match kind {
            PluginKind::Command { description, .. } => {
                assert_eq!(description, "");
            }
            _ => panic!("Expected Command kind"),
        }
    }

    // ---------- JSON manifest tests (Claude Code ecosystem compatibility) ----------

    const SAMPLE_TOOL_MANIFEST_JSON: &str = r#"{
  "name": "example-plugin",
  "version": "1.0.0",
  "description": "An example plugin",
  "author": "Shannon Team",
  "repository": "https://github.com/shannon-code/example-plugin",
  "type": "tool",
  "entry": "src/main.rs",
  "permissions": ["read_files", "network"],
  "keywords": ["example", "demo"],
  "transport": {
    "type": "stdio",
    "command": "node",
    "args": ["index.js"]
  }
}"#;

    #[test]
    fn test_parse_tool_manifest_json() {
        let manifest = PluginManifest::from_json(SAMPLE_TOOL_MANIFEST_JSON).unwrap();
        assert_eq!(manifest.name, "example-plugin");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.description, "An example plugin");
        assert_eq!(manifest.author.as_deref(), Some("Shannon Team"));
        assert!(manifest.permissions.contains(&PluginPermission::ReadFiles));
        assert!(manifest.permissions.contains(&PluginPermission::Network));
        assert_eq!(manifest.keywords, vec!["example", "demo"]);
    }

    #[test]
    fn test_json_tool_kind() {
        let manifest = PluginManifest::from_json(SAMPLE_TOOL_MANIFEST_JSON).unwrap();
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
    fn test_json_skill_manifest() {
        let json = r#"{
  "name": "hello-skill",
  "version": "0.1.0",
  "description": "A hello skill",
  "type": "skill",
  "entry": "template.md",
  "trigger": "/hello",
  "template": "Hello {{name}}!"
}"#;
        let manifest = PluginManifest::from_json(json).unwrap();
        assert_eq!(manifest.name, "hello-skill");
        assert_eq!(manifest.type_display_name(), "Skill");
        match manifest.kind().unwrap() {
            PluginKind::Skill { trigger, template } => {
                assert_eq!(trigger, "/hello");
                assert_eq!(template, "Hello {{name}}!");
            }
            _ => panic!("Expected Skill kind"),
        }
    }

    #[test]
    fn test_json_command_manifest() {
        let json = r#"{
  "name": "my-cmd",
  "version": "1.0.0",
  "description": "A custom command",
  "type": "command",
  "entry": "cmd.md",
  "command_name": "review",
  "command_description": "Review code"
}"#;
        let manifest = PluginManifest::from_json(json).unwrap();
        assert_eq!(manifest.type_display_name(), "Command");
        match manifest.kind().unwrap() {
            PluginKind::Command { name, description } => {
                assert_eq!(name, "review");
                assert_eq!(description, "Review code");
            }
            _ => panic!("Expected Command kind"),
        }
    }

    #[test]
    fn test_json_sse_transport() {
        let json = r#"{
  "name": "remote-tool",
  "version": "1.0.0",
  "description": "SSE transport",
  "type": "tool",
  "entry": "x.md",
  "transport": {"type": "sse", "url": "http://localhost:8080/sse"}
}"#;
        let manifest = PluginManifest::from_json(json).unwrap();
        match manifest.kind().unwrap() {
            PluginKind::Tool { transport } => {
                assert!(!transport.is_stdio());
                assert!(transport.command().is_none());
            }
            _ => panic!("Expected Tool kind"),
        }
    }

    #[test]
    fn test_from_json_bytes_invalid_utf8() {
        let bad_bytes: &[u8] = &[0xff, 0xfe, 0x00];
        assert!(PluginManifest::from_json_bytes(bad_bytes).is_err());
    }

    #[test]
    fn test_from_json_bytes_invalid_json() {
        assert!(PluginManifest::from_json_bytes(b"{not valid").is_err());
    }

    #[test]
    fn test_json_and_toml_produce_equivalent_manifest() {
        let toml_manifest = PluginManifest::from_toml(SAMPLE_TOOL_MANIFEST).unwrap();
        let json_manifest = PluginManifest::from_json(SAMPLE_TOOL_MANIFEST_JSON).unwrap();
        assert_eq!(toml_manifest.name, json_manifest.name);
        assert_eq!(toml_manifest.version, json_manifest.version);
        assert_eq!(toml_manifest.plugin_type, json_manifest.plugin_type);
        assert_eq!(toml_manifest.entry, json_manifest.entry);
        assert_eq!(toml_manifest.permissions, json_manifest.permissions);
        assert_eq!(toml_manifest.keywords, json_manifest.keywords);
    }

    // ---------- §4.10 W3-2: v2 schema, three-dialect matrix, breakage ----

    /// Per-kind body rendered into each of the three dialects.
    const V2_KIND_BODIES: [(&str, &str); 3] = [
        (
            "tool",
            "type = \"tool\"\nentry = \"bin/srv.js\"\n\n[transport]\ntype = \"stdio\"\ncommand = \"node\"\nargs = [\"bin/srv.js\"]",
        ),
        (
            "command",
            "type = \"command\"\nentry = \"cmd.md\"\ncommand_name = \"review\"\ncommand_description = \"Review code\"",
        ),
        (
            "skill",
            "type = \"skill\"\nentry = \"template.md\"\ntrigger = \"/hello\"\ntemplate = \"Hello {{name}}!\"",
        ),
    ];

    fn v2_toml(kind_body: &str) -> String {
        format!(
            "manifest_version = \"2\"\nname = \"v2-plugin\"\nversion = \"1.0.0\"\ndescription = \"v2 sample\"\npermissions = [\"read_files\", \"llm_api\"]\n{kind_body}\n\n[[mcp]]\nname = \"sidecar\"\ntype = \"stdio\"\ncommand = \"npx\"\nargs = [\"-y\", \"thing\"]\n\n[[hooks]]\nevent = \"PostToolUse\"\nhandler = \"hooks/lint.sh\"\n\n[compat]\nmin = \"0.10.0\"\nmax = \"99.0.0\""
        )
    }

    fn v2_json(kind_body_fields: &str) -> String {
        format!(
            "{{\"manifest_version\": \"2\", \"name\": \"v2-plugin\", \"version\": \"1.0.0\", \
             \"description\": \"v2 sample\", \"permissions\": [\"read_files\", \"llm_api\"], \
             {kind_body_fields}, \
             \"mcpServers\": {{\"remote-api\": {{\"url\": \"http://api.example.test/sse\"}}}}, \
             \"hooks\": [{{\"event\": \"PostToolUse\", \"handler\": \"hooks/lint.sh\"}}], \
             \"compat\": {{\"min\": \"0.10.0\", \"max\": \"99.0.0\"}}}}"
        )
    }

    /// The full §4.10 parsing matrix: {v1-TOML, v2-TOML, claude-JSON} x
    /// {tool, command, skill} all load through the same in-memory shape,
    /// only the schema generation and optional-slot defaults differ.
    #[test]
    fn parsing_matrix_covers_all_three_dialects_and_kinds() {
        for (kind, body) in V2_KIND_BODIES {
            // v1 TOML: same kind vocabulary, v2 slots absent -> defaults.
            let v1 = format!("name = \"p\"\nversion = \"1.0.0\"\ndescription = \"d\"\n{body}");
            let m1 = PluginManifest::from_toml(&v1)
                .unwrap_or_else(|e| panic!("v1/{kind} must parse: {e}"));
            assert_eq!(m1.schema_version(), ManifestVersion::V1);
            assert!(m1.mcp.is_empty() && m1.hooks.is_empty() && m1.compat.is_none());
            assert!(m1.kind().is_ok(), "v1/{kind} resolves a kind");

            // v2 TOML
            let toml = v2_toml(body);
            let m2 = PluginManifest::from_toml(&toml)
                .unwrap_or_else(|e| panic!("v2-toml/{kind} must parse: {e}"));
            assert_eq!(m2.schema_version(), ManifestVersion::V2);
            assert_eq!(m2.name, "v2-plugin");
            assert_eq!(m2.hooks.len(), 1);
            assert!(m2.kind().is_ok(), "v2-toml/{kind} resolves a kind");
            // [[mcp]] bundles apply to every kind (a skill may ship a helper
            // MCP server); assert the parsed reference either way.
            assert_eq!(m2.mcp.len(), 1);
            assert_eq!(m2.mcp[0].name, "sidecar");
            assert!(m2.mcp[0].command.is_some());
            assert!(
                m2.is_within_compat(env!("CARGO_PKG_VERSION")),
                "fixture window covers the build version"
            );

            // Claude JSON dialect
            let fields = match kind {
                "tool" => {
                    "\"type\": \"tool\", \"entry\": \"bin/srv.js\", \
                    \"transport\": {\"type\": \"stdio\", \"command\": \"node\", \"args\": [\"bin/srv.js\"]}"
                }
                "command" => {
                    "\"type\": \"command\", \"entry\": \"cmd.md\", \
                    \"command_name\": \"review\", \"command_description\": \"Review code\""
                }
                _ => {
                    "\"type\": \"skill\", \"entry\": \"template.md\", \
                    \"trigger\": \"/hello\", \"template\": \"Hello {{name}}!\""
                }
            };
            let json = v2_json(fields);
            let mj = PluginManifest::from_json(&json)
                .unwrap_or_else(|e| panic!("claude-json/{kind} must parse: {e}"));
            assert_eq!(mj.schema_version(), ManifestVersion::V2);
            assert!(mj.kind().is_ok(), "claude-json/{kind} resolves a kind");

            // The claude map-style mcpServers entry inferred its transport.
            assert_eq!(mj.mcp.len(), 1);
            assert_eq!(mj.mcp[0].name, "remote-api");
            assert_eq!(mj.mcp[0].transport_type, "sse");
            assert!(mj.mcp[0].url.as_deref().unwrap().starts_with("http"));
        }
    }

    #[test]
    fn v2_mcp_servers_map_infers_stdio_when_command_present() {
        let json = r#"{
          "name": "map-infer", "version": "1.0.0", "description": "d",
          "type": "skill", "entry": "t.md", "trigger": "/t", "template": "t",
          "mcpServers": {"local-py": {"command": "python", "args": ["srv.py"]}}
        }"#;
        let m = PluginManifest::from_json(json).expect("parses");
        assert_eq!(m.mcp.len(), 1);
        assert_eq!(m.mcp[0].transport_type, "stdio");
        assert_eq!(m.mcp[0].args, vec!["srv.py".to_string()]);
    }

    #[test]
    fn manifest_version_is_backward_and_forward_readable() {
        assert_eq!(ManifestVersion::default(), ManifestVersion::V1);
        assert_eq!(ManifestVersion::V2.as_str(), "2");
        // Absent key round-trips as v1.
        let v1 = PluginManifest::from_toml(
            "name = \"a\"\nversion = \"1.0.0\"\ndescription = \"d\"\ntype = \"skill\"\nentry = \"t.md\"\ntrigger = \"/t\"\ntemplate = \"t\"",
        )
        .unwrap();
        assert_eq!(v1.manifest_version, ManifestVersion::V1);
    }

    #[test]
    fn compat_range_semantics() {
        use crate::plugin::manifest::CompatRange;
        let range = CompatRange {
            min: Some("0.9.0".into()),
            max: Some("0.11.0".into()),
        };
        assert!(range.contains("0.9.0") && range.contains("0.10.5"));
        assert!(!range.contains("0.11.0") && !range.contains("0.8.9"));
        let open = CompatRange {
            min: None,
            max: None,
        };
        assert!(open.contains("1.2.3"));
        // an unparseable running version can never be proven inside a window
        assert!(!open.contains("not-a-version"));
        // unparsable bound is treated as unsatisfiable rather than guessing
        let bad = CompatRange {
            min: Some("bananas".into()),
            max: None,
        };
        assert!(!bad.contains("1.0.0"));
    }

    #[test]
    fn broken_manifests_report_explicit_errors() {
        // syntactic breakage — TOML
        assert!(PluginManifest::from_toml_bytes(b"name = \"x\"\nnonsense").is_err());
        // unknown permission token names the offending face at parse time
        let err = PluginManifest::from_toml(
            "name = \"p\"\nversion = \"1.0.0\"\ndescription = \"d\"\ntype = \"skill\"\nentry = \"t.md\"\ntrigger = \"/t\"\ntemplate = \"t\"\npermissions = [\"root_access\"]",
        );
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("root_access"), "{msg}");
        // required base field missing
        assert!(
            PluginManifest::from_toml(
                "version = \"1.0.0\"\ndescription = \"d\"\ntype = \"skill\"\nentry = \"t.md\"\ntrigger = \"/t\"\ntemplate = \"t\""
            )
            .is_err()
        );
        // syntactic breakage — JSON
        assert!(PluginManifest::from_json_bytes(b"{\"name\":").is_err());
    }

    /// Wire names used by enforcement denials must match serde renames.
    #[test]
    fn wire_names_match_serde_renames() {
        let all = [
            ("read_files", PluginPermission::ReadFiles),
            ("write_files", PluginPermission::WriteFiles),
            ("execute_commands", PluginPermission::ExecuteCommands),
            ("network", PluginPermission::Network),
            ("mcp_tools", PluginPermission::McpTools),
            ("llm_api", PluginPermission::LlmApi),
        ];
        for (wire, variant) in all {
            assert_eq!(variant.wire_name(), wire);
            assert_eq!(
                serde_json::to_string(&variant).unwrap(),
                format!("\"{wire}\"")
            );
        }
    }
}
