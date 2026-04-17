//! MCP Tool Adapter — bridges individual MCP server tools into the ToolRegistry.
//!
//! When MCP server configurations are discovered from
//! `~/.shannon/mcp_servers.json`, each server is queried via `tools/list`
//! to discover its available tools. Each discovered tool is wrapped in an
//! [`McpToolAdapter`] that implements the `Tool` trait. At execution time,
//! the adapter spawns the server process (for stdio transport), sends a
//! JSON-RPC `tools/call` message, and returns the result.

use async_trait::async_trait;
use serde_json::Value;
use shannon_tool_interface::{Tool, ToolError, ToolOutput, ToolResult};
use std::collections::HashMap;
use std::process::Stdio;

/// Adapter that exposes a single MCP server tool through the `Tool` trait.
///
/// Each instance represents one specific tool discovered from an MCP server
/// via `tools/list`. The registry name follows the pattern
/// `mcp__{server}__{tool}` (e.g., `mcp__fetch__fetch`).
pub struct McpToolAdapter {
    /// Server name (used as prefix).
    server_name: String,
    /// The tool name on the MCP server side.
    remote_tool_name: String,
    /// Command to spawn (stdio transport).
    command: Option<String>,
    /// Arguments for the command.
    args: Vec<String>,
    /// Environment variables for the server process.
    env: HashMap<String, String>,
    /// Human-readable description of this specific tool.
    description: String,
    /// JSON Schema for this tool's input.
    input_schema: Value,
    /// Tool name in the registry (e.g. "mcp__fetch__fetch").
    tool_name: String,
}

impl McpToolAdapter {
    /// Create a new MCP tool adapter for a specific discovered tool.
    pub fn new(
        server_name: String,
        remote_tool_name: String,
        command: Option<String>,
        args: Vec<String>,
        env: HashMap<String, String>,
        description: String,
        input_schema: Value,
    ) -> Self {
        let tool_name = format!("mcp__{server_name}__{remote_tool_name}");
        Self {
            server_name,
            remote_tool_name,
            command,
            args,
            env,
            description,
            input_schema,
            tool_name,
        }
    }
}

impl std::fmt::Debug for McpToolAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpToolAdapter")
            .field("server_name", &self.server_name)
            .field("tool_name", &self.tool_name)
            .finish()
    }
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let command = match &self.command {
            Some(cmd) => cmd.clone(),
            None => {
                return Err(ToolError::ExecutionFailed(format!(
                    "MCP server '{}' has no command configured (HTTP transport not yet supported)",
                    self.server_name
                )));
            }
        };

        // Build the JSON-RPC request for tools/call
        let arguments = input;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": self.remote_tool_name,
                "arguments": arguments,
            }
        });

        let request_json = serde_json::to_string(&request)
            .map_err(|e| ToolError::InvalidInput(format!("Failed to serialize request: {e}")))?;

        // Split command into program + args
        let mut parts: Vec<String> = command
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        parts.extend(self.args.iter().cloned());

        if parts.is_empty() {
            return Err(ToolError::ExecutionFailed(format!(
                "MCP server '{}' has empty command",
                self.server_name
            )));
        }

        let program = &parts[0];
        let args = &parts[1..];

        // Spawn the server process
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set environment variables
        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "MCP server '{}' failed to spawn '{}': {}",
                self.server_name, command, e
            ))
        })?;

        // Write request to stdin
        use tokio::io::AsyncWriteExt;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(request_json.as_bytes()).await.map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "MCP server '{}' stdin write failed: {}",
                    self.server_name, e
                ))
            })?;
            // Send newline to signal end of input
            let _ = stdin.write_all(b"\n").await;
            drop(stdin);
        }

        // Wait for response with timeout
        let timeout = tokio::time::Duration::from_secs(30);
        let result = tokio::time::timeout(timeout, child.wait_with_output()).await;

        match result {
            Ok(Ok(output)) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    // Try to parse the JSON-RPC response
                    if let Ok(response) = serde_json::from_str::<Value>(&stdout) {
                        if let Some(result) = response.get("result") {
                            if let Some(content) = result.get("content") {
                                if let Some(text) = content.get(0).and_then(|c| c.get("text")).and_then(|t| t.as_str()) {
                                    return Ok(ToolOutput::success(text.to_string()));
                                }
                            }
                            return Ok(ToolOutput::success(result.to_string()));
                        }
                        if let Some(error) = response.get("error") {
                            let msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
                            return Ok(ToolOutput::error(format!(
                                "MCP server '{}' error: {}", self.server_name, msg
                            )));
                        }
                    }
                    // Fallback: return raw stdout
                    Ok(ToolOutput::success(stdout))
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    Ok(ToolOutput::error(format!(
                        "MCP server '{}' exited with code {:?}: {}",
                        self.server_name,
                        output.status.code(),
                        stderr.chars().take(500).collect::<String>()
                    )))
                }
            }
            Ok(Err(e)) => Err(ToolError::ExecutionFailed(format!(
                "MCP server '{}' I/O error: {}",
                self.server_name, e
            ))),
            Err(_) => Err(ToolError::ExecutionFailed(format!(
                "MCP server '{}' timed out after 30 seconds",
                self.server_name
            ))),
        }
    }

    fn requires_auth(&self) -> bool {
        false
    }

    fn category(&self) -> &str {
        "mcp"
    }
}

/// Result of discovering tools from an MCP server.
pub struct DiscoveryResult {
    /// The server name.
    pub server_name: String,
    /// List of discovered tool adapters ready to register.
    pub tools: Vec<McpToolAdapter>,
}

/// Discover tools from an MCP server via stdio transport.
///
/// Spawns the server process, sends `initialize` + `tools/list` requests,
/// parses the response, and returns one [`McpToolAdapter`] per discovered tool.
pub async fn discover_tools(
    server_name: &str,
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> Result<DiscoveryResult, String> {
    // Build the full command
    let mut parts: Vec<String> = command
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();
    parts.extend(args.iter().cloned());

    if parts.is_empty() {
        return Err(format!("MCP server '{server_name}' has empty command"));
    }

    let program = &parts[0];
    let cmd_args = &parts[1..];

    // Spawn the server process
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(cmd_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (key, value) in env {
        cmd.env(key, value);
    }

    let mut child = cmd.spawn().map_err(|e| {
        format!("MCP server '{server_name}' failed to spawn '{command}': {e}")
    })?;

    // Build the initialize + tools/list request sequence (sent as two JSON-RPC messages)
    let init_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "shannon-code", "version": "0.1.0"}
        }
    });
    let tools_list_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });

    let request_json = format!(
        "{}\n{}\n",
        serde_json::to_string(&init_request).unwrap_or_default(),
        serde_json::to_string(&tools_list_request).unwrap_or_default()
    );

    use tokio::io::AsyncWriteExt;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(request_json.as_bytes()).await.map_err(|e| {
            format!("MCP server '{server_name}' stdin write failed: {e}")
        })?;
        drop(stdin);
    }

    // Wait for response with timeout
    let timeout = tokio::time::Duration::from_secs(15);
    let result = tokio::time::timeout(timeout, child.wait_with_output()).await;

    let output = match result {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => return Err(format!("MCP server '{server_name}' I/O error: {e}")),
        Err(_) => return Err(format!("MCP server '{server_name}' timed out during discovery")),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!(
            "MCP server '{server_name}' exited with code {:?}: {}",
            output.status.code(),
            stderr.chars().take(500).collect::<String>()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    // Find the tools/list response (JSON-RPC response with id=2)
    let mut discovered_tools: Vec<McpToolAdapter> = Vec::new();

    for line in stdout.lines() {
        if let Ok(response) = serde_json::from_str::<Value>(line) {
            // Only process the tools/list response (id=2)
            if response.get("id").and_then(|v| v.as_u64()) == Some(2) {
                if let Some(tools_array) = response
                    .get("result")
                    .and_then(|r| r.get("tools"))
                    .and_then(|t| t.as_array())
                {
                    for tool_value in tools_array {
                        let tool_name = tool_value
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let description = tool_value
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or(&format!("MCP tool: {tool_name}"))
                            .to_string();
                        let input_schema = tool_value
                            .get("inputSchema")
                            .cloned()
                            .unwrap_or(serde_json::json!({"type": "object"}));

                        discovered_tools.push(McpToolAdapter::new(
                            server_name.to_string(),
                            tool_name,
                            Some(command.to_string()),
                            args.to_vec(),
                            env.clone(),
                            description,
                            input_schema,
                        ));
                    }
                }
            }
        }
    }

    Ok(DiscoveryResult {
        server_name: server_name.to_string(),
        tools: discovered_tools,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_adapter_name() {
        let adapter = McpToolAdapter::new(
            "test-server".to_string(),
            "fetch".to_string(),
            Some("echo".to_string()),
            vec![],
            HashMap::new(),
            "Fetch a URL".to_string(),
            serde_json::json!({"type": "object"}),
        );
        assert_eq!(adapter.name(), "mcp__test-server__fetch");
    }

    #[test]
    fn test_mcp_tool_adapter_description() {
        let adapter = McpToolAdapter::new(
            "srv".to_string(),
            "search".to_string(),
            Some("echo".to_string()),
            vec![],
            HashMap::new(),
            "Search the web".to_string(),
            serde_json::json!({"type": "object"}),
        );
        assert_eq!(adapter.description(), "Search the web");
    }

    #[test]
    fn test_mcp_tool_adapter_category() {
        let adapter = McpToolAdapter::new(
            "srv".to_string(),
            "tool".to_string(),
            Some("echo".to_string()),
            vec![],
            HashMap::new(),
            "desc".to_string(),
            serde_json::json!({"type": "object"}),
        );
        assert_eq!(adapter.category(), "mcp");
    }

    #[test]
    fn test_mcp_tool_adapter_requires_auth() {
        let adapter = McpToolAdapter::new(
            "srv".to_string(),
            "tool".to_string(),
            Some("echo".to_string()),
            vec![],
            HashMap::new(),
            "desc".to_string(),
            serde_json::json!({"type": "object"}),
        );
        assert!(!adapter.requires_auth());
    }

    #[test]
    fn test_mcp_tool_adapter_input_schema() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "url": {"type": "string"}
            }
        });
        let adapter = McpToolAdapter::new(
            "srv".to_string(),
            "fetch".to_string(),
            Some("echo".to_string()),
            vec![],
            HashMap::new(),
            "desc".to_string(),
            schema.clone(),
        );
        assert_eq!(adapter.input_schema(), schema);
    }

    #[tokio::test]
    async fn test_mcp_tool_adapter_no_command() {
        let adapter = McpToolAdapter::new(
            "srv".to_string(),
            "tool".to_string(),
            None,
            vec![],
            HashMap::new(),
            "desc".to_string(),
            serde_json::json!({"type": "object"}),
        );
        let result = adapter.execute(serde_json::json!({})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no command"));
    }

    #[test]
    fn test_mcp_tool_adapter_debug() {
        let adapter = McpToolAdapter::new(
            "test".to_string(),
            "tool".to_string(),
            Some("cat".to_string()),
            vec![],
            HashMap::new(),
            "desc".to_string(),
            serde_json::json!({"type": "object"}),
        );
        let debug_str = format!("{adapter:?}");
        assert!(debug_str.contains("test"));
        assert!(debug_str.contains("mcp__test"));
    }
}
