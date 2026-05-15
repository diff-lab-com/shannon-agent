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
use shannon_types::recover_lock;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

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
    /// URL for remote HTTP/SSE transport (None for stdio).
    url: Option<String>,
    /// HTTP headers for remote transport (e.g. Authorization).
    headers: HashMap<String, String>,
    /// OAuth scopes required by this server (for 403 re-auth).
    oauth_scopes: Vec<String>,
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
            url: None,
            headers: HashMap::new(),
            oauth_scopes: Vec::new(),
        }
    }

    /// Create a new MCP tool adapter for a remote HTTP/SSE server.
    pub fn new_remote(
        server_name: String,
        remote_tool_name: String,
        url: String,
        headers: HashMap<String, String>,
        description: String,
        input_schema: Value,
    ) -> Self {
        let tool_name = format!("mcp__{server_name}__{remote_tool_name}");
        Self {
            server_name,
            remote_tool_name,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            description,
            input_schema,
            tool_name,
            url: Some(url),
            headers,
            oauth_scopes: Vec::new(),
        }
    }

    /// Swap the full input schema for a minimal stub, returning the original.
    ///
    /// Used for deferred schema loading: the tool registers with a stub schema
    /// to save context, and the real schema is retrieved on demand via
    /// `mcp__tool_search`.
    pub fn swap_schema_for_deferred(&mut self) -> Value {
        
        std::mem::replace(
            &mut self.input_schema,
            serde_json::json!({
                "type": "object",
                "properties": {},
                "description": format!("Use mcp__tool_search with tool_name=\"{}\" to get the full parameter schema.", self.tool_name)
            }),
        )
    }

    /// Get a reference to the tool's registry name (e.g. `mcp__server__tool`).
    pub fn registry_name(&self) -> &str {
        &self.tool_name
    }

    /// Set OAuth scopes for 403 insufficient_scope error reporting.
    pub fn with_oauth_scopes(mut self, scopes: Vec<String>) -> Self {
        self.oauth_scopes = scopes;
        self
    }

    /// Execute a tool call via HTTP POST to the remote MCP server.
    async fn execute_remote(&self, url: &str, input: Value) -> ToolResult<ToolOutput> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": self.remote_tool_name,
                "arguments": input,
            }
        });

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();
        let mut builder = client
            .post(url)
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&request).unwrap_or_default());

        for (key, value) in &self.headers {
            builder = builder.header(key.as_str(), value.as_str());
        }

        let timeout = tokio::time::Duration::from_secs(30);
        let result = tokio::time::timeout(timeout, builder.send()).await;

        match result {
            Ok(Ok(response)) => {
                let status = response.status();
                if !status.is_success() {
                    // Handle 403 Forbidden — check for insufficient_scope
                    if status.as_u16() == 403 {
                        let body = response.text().await.unwrap_or_default();
                        let is_insufficient_scope = body.contains("insufficient_scope")
                            || body.contains("insufficient permissions")
                            || body.contains("permission denied");

                        if is_insufficient_scope {
                            let scope_hint = if self.oauth_scopes.is_empty() {
                                String::new()
                            } else {
                                format!(" Required scopes: {}", self.oauth_scopes.join(", "))
                            };
                            return Ok(ToolOutput::error(format!(
                                "MCP server '{}' requires additional permissions (403 insufficient_scope).{scope_hint} Re-authenticate with broader scopes or check server permissions.",
                                self.server_name
                            )));
                        }
                        return Ok(ToolOutput::error(format!(
                            "MCP server '{}' HTTP 403 Forbidden: {}",
                            self.server_name,
                            body.chars().take(500).collect::<String>()
                        )));
                    }
                    return Ok(ToolOutput::error(format!(
                        "MCP server '{}' HTTP error: {}",
                        self.server_name,
                        status
                    )));
                }
                let body = response.text().await.unwrap_or_default();
                if let Ok(parsed) = serde_json::from_str::<Value>(&body) {
                    if let Some(result) = parsed.get("result") {
                        if let Some(content) = result.get("content") {
                            if let Some(text) = content.get(0).and_then(|c| c.get("text")).and_then(|t| t.as_str()) {
                                return Ok(ToolOutput::success(text.to_string()));
                            }
                        }
                        return Ok(ToolOutput::success(result.to_string()));
                    }
                    if let Some(error) = parsed.get("error") {
                        let msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
                        return Ok(ToolOutput::error(format!(
                            "MCP server '{}' error: {}", self.server_name, msg
                        )));
                    }
                }
                Ok(ToolOutput::success(body))
            }
            Ok(Err(e)) => Err(ToolError::ExecutionFailed(format!(
                "MCP server '{}' HTTP request failed: {}",
                self.server_name, e
            ))),
            Err(_) => Err(ToolError::ExecutionFailed(format!(
                "MCP server '{}' timed out after 30 seconds",
                self.server_name
            ))),
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
        // Remote HTTP/SSE transport path
        if let Some(ref url) = self.url {
            return self.execute_remote(url, input).await;
        }

        // Stdio transport path
        let command = match &self.command {
            Some(cmd) => cmd.clone(),
            None => {
                return Err(ToolError::ExecutionFailed(format!(
                    "MCP server '{}' has no command or URL configured",
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

        // Validate program name — reject shell metacharacters
        if program.contains("..") || program.starts_with('/') && program.contains("etc/passwd") {
            return Err(ToolError::ExecutionFailed(format!(
                "MCP server '{}' has invalid program path: {program}",
                self.server_name
            )));
        }

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

/// A discovered MCP prompt with its argument schema.
#[derive(Debug, Clone)]
pub struct PromptInfo {
    /// Prompt name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Argument names (for command template).
    pub argument_names: Vec<String>,
}

/// Result of discovering tools from an MCP server.
pub struct DiscoveryResult {
    /// The server name.
    pub server_name: String,
    /// List of discovered tool adapters ready to register.
    pub tools: Vec<McpToolAdapter>,
    /// List of discovered prompts from the server.
    pub prompts: Vec<PromptInfo>,
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
    timeout_secs: Option<u64>,
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
    let prompts_list_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "prompts/list",
        "params": {}
    });

    let request_json = format!(
        "{}\n{}\n{}\n",
        serde_json::to_string(&init_request).unwrap_or_default(),
        serde_json::to_string(&tools_list_request).unwrap_or_default(),
        serde_json::to_string(&prompts_list_request).unwrap_or_default()
    );

    use tokio::io::AsyncWriteExt;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(request_json.as_bytes()).await.map_err(|e| {
            format!("MCP server '{server_name}' stdin write failed: {e}")
        })?;
        drop(stdin);
    }

    // Wait for response with timeout
    let timeout = tokio::time::Duration::from_secs(timeout_secs.unwrap_or(15));
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

    let mut discovered_prompts: Vec<PromptInfo> = Vec::new();

    for line in stdout.lines() {
        if let Ok(response) = serde_json::from_str::<Value>(line) {
            let id = response.get("id").and_then(|v| v.as_u64());

            // Process tools/list response (id=2)
            if id == Some(2) {
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

            // Process prompts/list response (id=3)
            if id == Some(3) {
                if let Some(prompts_array) = response
                    .get("result")
                    .and_then(|r| r.get("prompts"))
                    .and_then(|p| p.as_array())
                {
                    for prompt_value in prompts_array {
                        let name = prompt_value
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let description = prompt_value
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string();
                        let argument_names = prompt_value
                            .get("arguments")
                            .and_then(|a| a.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|arg| arg.get("name").and_then(|n| n.as_str()).map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();

                        discovered_prompts.push(PromptInfo {
                            name,
                            description,
                            argument_names,
                        });
                    }
                }
            }
        }
    }

    Ok(DiscoveryResult {
        server_name: server_name.to_string(),
        tools: discovered_tools,
        prompts: discovered_prompts,
    })
}

/// Discover tools from a remote MCP server via HTTP transport.
///
/// Sends initialize + tools/list + prompts/list via HTTP POST requests,
/// parses the responses, and returns adapters for each discovered tool.
pub async fn discover_tools_http(
    server_name: &str,
    url: &str,
    headers: &HashMap<String, String>,
    timeout_secs: Option<u64>,
) -> Result<DiscoveryResult, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs.unwrap_or(30)))
        .build()
        .unwrap_or_default();

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
    let prompts_list_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "prompts/list",
        "params": {}
    });

    // Helper to send a JSON-RPC request
    let send_request = |req_body: Value| {
        let mut builder = client
            .post(url)
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&req_body).unwrap_or_default());
        for (key, value) in headers {
            builder = builder.header(key.as_str(), value.as_str());
        }
        builder.send()
    };

    // Send initialize
    let init_timeout = tokio::time::Duration::from_secs(timeout_secs.unwrap_or(10));
    let init_resp = tokio::time::timeout(init_timeout, send_request(init_request))
        .await
        .map_err(|_| format!("MCP server '{server_name}' init request timed out"))?
        .map_err(|e| format!("MCP server '{server_name}' init request failed: {e}"))?;

    if !init_resp.status().is_success() {
        return Err(format!("MCP server '{server_name}' init returned HTTP {}", init_resp.status()));
    }

    // Send tools/list
    let tools_timeout = tokio::time::Duration::from_secs(timeout_secs.unwrap_or(15));
    let tools_resp = tokio::time::timeout(tools_timeout, send_request(tools_list_request))
        .await
        .map_err(|_| format!("MCP server '{server_name}' tools/list request timed out"))?
        .map_err(|e| format!("MCP server '{server_name}' tools/list request failed: {e}"))?;

    let tools_body = tools_resp.text().await.unwrap_or_default();

    // Send prompts/list (best-effort)
    let prompts_resp = tokio::time::timeout(
        tokio::time::Duration::from_secs(10),
        send_request(prompts_list_request),
    ).await;
    let prompts_body = prompts_resp
        .ok()
        .and_then(|r| r.ok())
        .map(|r| tokio::runtime::Handle::current().block_on(r.text()).unwrap_or_default())
        .unwrap_or_default();

    // Parse tools
    let mut discovered_tools: Vec<McpToolAdapter> = Vec::new();
    if let Ok(parsed) = serde_json::from_str::<Value>(&tools_body) {
        if let Some(tools_array) = parsed.get("result").and_then(|r| r.get("tools")).and_then(|t| t.as_array()) {
            for tool_value in tools_array {
                let tool_name = tool_value.get("name").and_then(|n| n.as_str()).unwrap_or("unknown").to_string();
                let description = tool_value.get("description").and_then(|d| d.as_str()).unwrap_or(&format!("MCP tool: {tool_name}")).to_string();
                let input_schema = tool_value.get("inputSchema").cloned().unwrap_or(serde_json::json!({"type": "object"}));
                discovered_tools.push(McpToolAdapter::new_remote(
                    server_name.to_string(),
                    tool_name,
                    url.to_string(),
                    headers.clone(),
                    description,
                    input_schema,
                ));
            }
        }
    }

    // Parse prompts
    let mut discovered_prompts: Vec<PromptInfo> = Vec::new();
    if let Ok(parsed) = serde_json::from_str::<Value>(&prompts_body) {
        if let Some(prompts_array) = parsed.get("result").and_then(|r| r.get("prompts")).and_then(|p| p.as_array()) {
            for prompt_value in prompts_array {
                let name = prompt_value.get("name").and_then(|n| n.as_str()).unwrap_or("unknown").to_string();
                let description = prompt_value.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
                let argument_names = prompt_value.get("arguments")
                    .and_then(|a| a.as_array())
                    .map(|arr| arr.iter().filter_map(|arg| arg.get("name").and_then(|n| n.as_str()).map(String::from)).collect())
                    .unwrap_or_default();
                discovered_prompts.push(PromptInfo { name, description, argument_names });
            }
        }
    }

    Ok(DiscoveryResult {
        server_name: server_name.to_string(),
        tools: discovered_tools,
        prompts: discovered_prompts,
    })
}

// ---------------------------------------------------------------------------
// Deferred schema store for legacy (non-pooled) tool path
// ---------------------------------------------------------------------------

/// Shared store for deferred MCP tool schemas.
///
/// When many MCP tools are discovered, their full JSON schemas consume
/// significant context. This store holds the real schemas while tools
/// register with minimal stubs. The LLM retrieves full schemas on demand
/// via [`DeferredSchemaSearchTool`].
pub type DeferredSchemaStore = Arc<std::sync::Mutex<HashMap<String, Value>>>;

/// Threshold above which deferred schema loading is auto-enabled.
pub const DEFERRED_SCHEMA_THRESHOLD: usize = 20;

/// Prepare deferred schema loading for a batch of MCP tool adapters.
///
/// Swaps each adapter's full schema for a minimal stub, storing the
/// originals in the returned store. Returns the store and the number
/// of tools deferred.
pub fn prepare_deferred_schemas(
    tools: &mut [Box<McpToolAdapter>],
) -> DeferredSchemaStore {
    let store: DeferredSchemaStore = Arc::new(std::sync::Mutex::new(HashMap::new()));
    for tool in tools.iter_mut() {
        let real_schema = tool.swap_schema_for_deferred();
        let name = tool.name().to_string();
        recover_lock(store.lock()).insert(name, real_schema);
    }
    store
}

/// Lightweight tool that retrieves deferred MCP tool schemas on demand.
///
/// Works with [`DeferredSchemaStore`] (simple HashMap) instead of requiring
/// the full `McpProcessPool`. Used by the legacy one-shot discovery path.
pub struct DeferredSchemaSearchTool {
    schemas: DeferredSchemaStore,
}

impl DeferredSchemaSearchTool {
    /// Create a new search tool backed by the given schema store.
    pub fn new(schemas: DeferredSchemaStore) -> Self {
        Self { schemas }
    }
}

impl std::fmt::Debug for DeferredSchemaSearchTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeferredSchemaSearchTool").finish()
    }
}

#[async_trait]
impl Tool for DeferredSchemaSearchTool {
    fn name(&self) -> &str {
        "mcp__tool_search"
    }

    fn description(&self) -> &str {
        "Search for an MCP tool's full parameter schema by tool name. \
         Use this before calling any mcp__ tool to discover its required \
         and optional parameters."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "tool_name": {
                    "type": "string",
                    "description": "Full tool name (e.g., \"mcp__fetch__fetch\") to retrieve the schema for."
                }
            },
            "required": ["tool_name"]
        })
    }

    fn category(&self) -> &str {
        "mcp"
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let tool_name = input
            .get("tool_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing 'tool_name' parameter".to_string()))?;

        let schemas = recover_lock(self.schemas.lock());
        match schemas.get(tool_name) {
            Some(schema) => {
                let schema_str = serde_json::to_string_pretty(schema)
                    .unwrap_or_else(|_| schema.to_string());
                Ok(ToolOutput::success(schema_str))
            }
            None => {
                let available: Vec<&String> = schemas.keys().collect();
                Ok(ToolOutput::error(format!(
                    "No deferred schema found for '{tool_name}'. Available: {:?}",
                    available.iter().take(10).collect::<Vec<_>>()
                )))
            }
        }
    }

    fn requires_auth(&self) -> bool {
        false
    }
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
