//! # Plugin Tool Adapter
//!
//! Wraps a plugin's `ToolDefinition` as a `Tool` trait implementation,
//! allowing plugin-defined tools to be registered in the `ToolRegistry`
//! and invoked through the normal tool execution pipeline.
//!
//! Plugin tools execute shell commands, receiving JSON input on stdin
//! and producing JSON output on stdout.

use async_trait::async_trait;
use serde_json::Value;
use shannon_tool_interface::{Tool, ToolError, ToolOutput, ToolResult};
use std::process::Stdio;

use crate::plugins::{PluginManager, ToolDefinition};

/// Adapter that wraps a plugin `ToolDefinition` as a `Tool` trait object.
///
/// When `execute()` is called, the adapter spawns the shell command defined
/// in the `ToolDefinition`, pipes the tool input as JSON on stdin, and
/// captures stdout as the tool result.
pub struct PluginTool {
    /// The plugin tool definition (command, schema, etc.)
    definition: ToolDefinition,
}

impl PluginTool {
    /// Create a new PluginTool from a ToolDefinition
    pub fn new(definition: ToolDefinition) -> Self {
        Self { definition }
    }

    /// Get the tool name
    pub fn name(&self) -> &str {
        &self.definition.name
    }
}

impl std::fmt::Debug for PluginTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginTool")
            .field("name", &self.definition.name)
            .field("command", &self.definition.command)
            .field("is_read_only", &self.definition.is_read_only)
            .finish()
    }
}

#[async_trait]
impl Tool for PluginTool {
    fn name(&self) -> &str {
        &self.definition.name
    }

    fn description(&self) -> &str {
        &self.definition.description
    }

    fn input_schema(&self) -> Value {
        self.definition.input_schema.clone()
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let input_json = serde_json::to_string(&input)
            .map_err(|e| ToolError::InvalidInput(format!("Failed to serialize input: {}", e)))?;

        // Split command string into program + args.
        // Use basic shell splitting: split by whitespace, respecting no quoting for simplicity.
        // Plugin commands are expected to be simple "program arg1 arg2" strings.
        let parts = shell_words_split(&self.definition.command);
        if parts.is_empty() {
            return Err(ToolError::ExecutionFailed(format!(
                "Plugin tool '{}' has empty command",
                self.definition.name
            )));
        }

        let program = &parts[0];
        let args = &parts[1..];

        // Spawn the command with a 30-second timeout
        let child = tokio::process::Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                return Err(ToolError::ExecutionFailed(format!(
                    "Plugin tool '{}' failed to spawn command '{}': {}",
                    self.definition.name, self.definition.command, e
                )));
            }
        };

        // Write input JSON to stdin
        use tokio::io::AsyncWriteExt;
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(input_json.as_bytes()).await {
                return Err(ToolError::ExecutionFailed(format!(
                    "Plugin tool '{}' failed to write to stdin: {}",
                    self.definition.name, e
                )));
            }
            drop(stdin); // Close stdin to signal EOF
        }

        // Wait for completion with timeout
        let timeout = tokio::time::Duration::from_secs(30);
        let result = tokio::time::timeout(timeout, child.wait_with_output()).await;

        match result {
            Ok(Ok(output)) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    Ok(ToolOutput::success(stdout))
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let msg = if stderr.is_empty() {
                        format!(
                            "Plugin tool '{}' exited with code {:?}. Output: {}",
                            self.definition.name,
                            output.status.code(),
                            stdout.chars().take(500).collect::<String>()
                        )
                    } else {
                        format!(
                            "Plugin tool '{}' exited with code {:?}: {}",
                            self.definition.name,
                            output.status.code(),
                            stderr.chars().take(500).collect::<String>()
                        )
                    };
                    Ok(ToolOutput::error(msg))
                }
            }
            Ok(Err(e)) => Err(ToolError::ExecutionFailed(format!(
                "Plugin tool '{}' I/O error: {}",
                self.definition.name, e
            ))),
            Err(_) => {
                // Timeout elapsed, try to kill the process
                Err(ToolError::ExecutionFailed(format!(
                    "Plugin tool '{}' timed out after {} seconds",
                    self.definition.name,
                    timeout.as_secs()
                )))
            }
        }
    }

    fn requires_auth(&self) -> bool {
        !self.definition.is_read_only
    }

    fn category(&self) -> &str {
        "plugin"
    }
}

/// Simple shell-like word splitting.
///
/// Splits on whitespace. Handles basic double-quote delimited strings.
/// Does not handle escaping or complex shell syntax -- plugin commands
/// are expected to be straightforward.
fn shell_words_split(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
            }
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

/// Register all active plugin tools into the given ToolRegistry.
///
/// This function discovers all tools from active plugins in the PluginManager
/// and creates `PluginTool` adapter instances for each one, registering them
/// with the provided registry.
///
/// Plugin tool registration errors (e.g., name collisions with built-in tools)
/// are logged as warnings but do not prevent other plugin tools from being registered.
pub fn register_plugin_tools(
    plugin_manager: &PluginManager,
    registry: &mut crate::tools::ToolRegistry,
) {
    let plugin_tools = plugin_manager.get_plugin_tools();

    if plugin_tools.is_empty() {
        return;
    }

    let tool_count = plugin_tools.len();
    for tool_def in plugin_tools {
        let name = tool_def.name.clone();
        let plugin_tool = PluginTool::new(tool_def.clone());

        match registry.register(Box::new(plugin_tool)) {
            Ok(()) => {
                tracing::info!(
                    "Registered plugin tool '{}' (command: {})",
                    name,
                    tool_def.command
                );
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to register plugin tool '{}': {}. Skipping.",
                    name,
                    e
                );
            }
        }
    }

    tracing::info!("Registered {} plugin tool(s) total", tool_count);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::{PluginManifest, ToolDefinition};
    use crate::tools::ToolRegistry;

    fn make_tool_def(name: &str, command: &str, is_read_only: bool) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: format!("Test tool {}", name),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                }
            }),
            command: command.to_string(),
            is_read_only,
        }
    }

    #[test]
    fn test_plugin_tool_name_and_description() {
        let def = make_tool_def("test_tool", "echo hello", true);
        let tool = PluginTool::new(def);
        assert_eq!(tool.name(), "test_tool");
        assert_eq!(tool.description(), "Test tool test_tool");
    }

    #[test]
    fn test_plugin_tool_category() {
        let def = make_tool_def("test_tool", "echo hello", true);
        let tool = PluginTool::new(def);
        assert_eq!(tool.category(), "plugin");
    }

    #[test]
    fn test_plugin_tool_requires_auth_read_only() {
        let def = make_tool_def("test_tool", "echo hello", true);
        let tool = PluginTool::new(def);
        assert!(!tool.requires_auth());
    }

    #[test]
    fn test_plugin_tool_requires_auth_write() {
        let def = make_tool_def("test_tool", "echo hello", false);
        let tool = PluginTool::new(def);
        assert!(tool.requires_auth());
    }

    #[test]
    fn test_plugin_tool_input_schema() {
        let def = make_tool_def("test_tool", "echo hello", true);
        let tool = PluginTool::new(def);
        let schema = tool.input_schema();
        assert!(schema.is_object());
        assert_eq!(schema["type"], "object");
    }

    #[tokio::test]
    async fn test_plugin_tool_execute_cat() {
        // `cat` reads stdin and writes it to stdout, matching our stdin-pipe model
        let def = make_tool_def("cat_tool", "cat", true);
        let tool = PluginTool::new(def);
        let input = serde_json::json!({"input": "hello world"});

        let result = tool.execute(input).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("hello world"));
    }

    #[tokio::test]
    async fn test_plugin_tool_execute_failing_command() {
        // `cat` with a nonexistent path exits nonzero but still reads stdin
        let def = make_tool_def("fail_tool", "cat /nonexistent_path_xyz_123", true);
        let tool = PluginTool::new(def);
        let input = serde_json::json!({});

        let result = tool.execute(input).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.is_error);
        assert!(output.content.contains("fail_tool"));
    }

    #[tokio::test]
    async fn test_plugin_tool_execute_nonexistent_command() {
        let def = make_tool_def("bad_tool", "nonexistent_command_xyz_123", true);
        let tool = PluginTool::new(def);
        let input = serde_json::json!({});

        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed(_)));
    }

    #[tokio::test]
    async fn test_plugin_tool_execute_empty_command() {
        let mut def = make_tool_def("empty_tool", "echo ok", true);
        def.command = String::new();
        let tool = PluginTool::new(def);
        let input = serde_json::json!({});

        let result = tool.execute(input).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_plugin_tool_debug() {
        let def = make_tool_def("debug_tool", "echo test", true);
        let tool = PluginTool::new(def);
        let debug_str = format!("{:?}", tool);
        assert!(debug_str.contains("debug_tool"));
        assert!(debug_str.contains("echo test"));
    }

    #[test]
    fn test_shell_words_split_simple() {
        let parts = shell_words_split("echo hello world");
        assert_eq!(parts, vec!["echo", "hello", "world"]);
    }

    #[test]
    fn test_shell_words_split_quoted() {
        let parts = shell_words_split("python3 \"/path with spaces/tool.py\"");
        assert_eq!(parts, vec!["python3", "/path with spaces/tool.py"]);
    }

    #[test]
    fn test_shell_words_split_empty() {
        let parts = shell_words_split("");
        assert!(parts.is_empty());
    }

    #[test]
    fn test_shell_words_split_single_word() {
        let parts = shell_words_split("echo");
        assert_eq!(parts, vec!["echo"]);
    }

    #[test]
    fn test_shell_words_split_multiple_spaces() {
        let parts = shell_words_split("echo   hello");
        assert_eq!(parts, vec!["echo", "hello"]);
    }

    #[test]
    fn test_shell_words_split_tabs() {
        let parts = shell_words_split("echo\thello");
        assert_eq!(parts, vec!["echo", "hello"]);
    }

    #[test]
    fn test_register_plugin_tools_empty_manager() {
        let manager = crate::plugins::PluginManager::with_config(
            vec![],
            std::path::PathBuf::from("/tmp/test-state.json"),
            "0.1.0",
        );
        let mut registry = ToolRegistry::new();
        register_plugin_tools(&manager, &mut registry);
        assert!(registry.list().is_empty());
    }

    #[tokio::test]
    async fn test_register_plugin_tools_with_tools() {
        // Create a plugin manager with a test plugin
        let temp_dir = tempfile::TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("plugins").join("test-plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();

        let manifest = PluginManifest {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            description: Some("Test".to_string()),
            author: None,
            min_version: None,
            tools: vec![make_tool_def("plugin_echo", "echo", true)],
            hooks: vec![],
            commands: vec![],
            settings_schema: None,
        };
        let manifest_path = plugin_dir.join("plugin.json");
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

        let mut manager = crate::plugins::PluginManager::with_config(
            vec![temp_dir.path().join("plugins")],
            temp_dir.path().join("plugin-state.json"),
            "0.1.0",
        );

        // Discover and load the plugin
        manager.discover_and_load_all().await.unwrap();

        // Register plugin tools
        let mut registry = ToolRegistry::new();
        register_plugin_tools(&manager, &mut registry);

        assert!(registry.list().contains(&"plugin_echo".to_string()));

        // Verify the tool works
        let result = registry
            .execute("plugin_echo", serde_json::json!({"input": "test"}))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_register_plugin_tools_skips_duplicate_names() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("plugins").join("dupe-plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();

        let manifest = PluginManifest {
            name: "dupe-plugin".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            min_version: None,
            tools: vec![make_tool_def("read", "echo dupe", true)],
            hooks: vec![],
            commands: vec![],
            settings_schema: None,
        };
        let manifest_path = plugin_dir.join("plugin.json");
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

        let mut manager = crate::plugins::PluginManager::with_config(
            vec![temp_dir.path().join("plugins")],
            temp_dir.path().join("plugin-state.json"),
            "0.1.0",
        );
        manager.discover_and_load_all().await.unwrap();

        // Pre-register a "read" tool (simulating a built-in tool)
        let mut registry = ToolRegistry::new();
        struct FakeReadTool;
        #[async_trait]
        impl Tool for FakeReadTool {
            fn name(&self) -> &str { "read" }
            fn description(&self) -> &str { "Built-in read" }
            fn input_schema(&self) -> Value { serde_json::json!({}) }
            async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
                Ok(ToolOutput::success("built-in".to_string()))
            }
        }
        registry.register(Box::new(FakeReadTool)).unwrap();

        // Register plugin tools -- "read" should be skipped due to name collision
        register_plugin_tools(&manager, &mut registry);

        // The built-in tool should still be there and working
        let result = registry.execute("read", serde_json::json!({})).await.unwrap();
        assert_eq!(result.content, "built-in");
    }
}
