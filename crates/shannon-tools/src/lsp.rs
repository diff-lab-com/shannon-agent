//! LSP (Language Server Protocol) integration tools
//!
//! Provides code intelligence by communicating with language servers:
//! - GoToDefinitionTool: Find where a symbol is defined
//! - FindReferencesTool: Find all usages of a symbol
//! - HoverTool: Get type information and documentation at a position
//! - DocumentSymbolTool: List all symbols in a file
//!
//! Uses a pragmatic approach: communicates with language servers via
//! JSON-RPC over stdin/stdout, with automatic server lifecycle management.

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use shannon_types::recover_lock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

// ---------------------------------------------------------------------------
// LSP Types
// ---------------------------------------------------------------------------

/// LSP position (0-based line and character)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

/// LSP range between two positions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

/// LSP location (URI + range)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LspLocation {
    pub uri: String,
    pub range: LspRange,
}

/// A single hover result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HoverResult {
    /// The hover contents (may contain markdown)
    pub contents: String,
    /// The range the hover applies to (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<LspRange>,
}

/// A document symbol (flat representation for simplicity)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentSymbolItem {
    /// Symbol name
    pub name: String,
    /// Kind of symbol (function, class, method, etc.)
    pub kind: String,
    /// Detail string (e.g. type signature)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Full range of the symbol
    pub range: LspRange,
    /// Selection range (identifier portion)
    pub selection_range: LspRange,
    /// Optional children (nested symbols)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<DocumentSymbolItem>,
}

// ---------------------------------------------------------------------------
// LSP Client
// ---------------------------------------------------------------------------

/// Manages communication with a language server process via JSON-RPC.
pub struct LspClient {
    child: Child,
    request_id: Arc<Mutex<i64>>,
    root_uri: String,
}

impl LspClient {
    /// Launch a language server process.
    pub async fn launch(
        server_command: &str,
        server_args: &[&str],
        root_path: &Path,
    ) -> Result<Self, String> {
        let child = Command::new(server_command)
            .args(server_args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to launch language server '{server_command}': {e}"))?;

        let root_uri = path_to_uri(root_path);

        let mut client = Self {
            child,
            request_id: Arc::new(Mutex::new(1)),
            root_uri,
        };

        client.initialize().await?;

        Ok(client)
    }

    /// Send the LSP `initialize` request.
    async fn initialize(&mut self) -> Result<(), String> {
        let params = json!({
            "processId": std::process::id(),
            "rootUri": self.root_uri,
            "capabilities": {
                "textDocument": {
                    "definition": { "dynamicRegistration": false },
                    "references": { "dynamicRegistration": false },
                    "hover": {
                        "contentFormat": ["markdown", "plaintext"],
                        "dynamicRegistration": false
                    },
                    "documentSymbol": {
                        "hierarchicalDocumentSymbolSupport": true,
                        "dynamicRegistration": false
                    }
                }
            }
        });

        let response = self.send_request("initialize", &params).await?;
        if response.get("error").is_some() {
            return Err(format!(
                "LSP initialize failed: {}",
                response["error"]
            ));
        }

        // Send initialized notification
        self.send_notification("initialized", &json!({}))
            .await?;

        Ok(())
    }

    /// Send a JSON-RPC request and wait for the response.
    async fn send_request(&mut self, method: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let id = {
            let mut rid = recover_lock(self.request_id.lock());
            let current = *rid;
            *rid += 1;
            current
        };

        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        self.send_message(&message.to_string()).await?;

        // Read response
        let stdout = self.child.stdout.as_mut()
            .ok_or_else(|| "stdout not available".to_string())?;
        let mut reader = BufReader::new(stdout);

        loop {
            let mut header_line = String::new();
            let bytes_read = reader.read_line(&mut header_line).await
                .map_err(|e| format!("Failed to read LSP response header: {e}"))?;

            if bytes_read == 0 {
                return Err("LSP server closed connection".to_string());
            }

            let header_line = header_line.trim();
            if header_line.is_empty() {
                continue;
            }

            if let Some(length_str) = header_line.strip_prefix("Content-Length: ") {
                let length: usize = length_str.trim().parse()
                    .map_err(|e| format!("Invalid Content-Length: {e}"))?;

                // Read the empty line after headers
                let mut sep = String::new();
                reader.read_line(&mut sep).await
                    .map_err(|e| format!("Failed to read header separator: {e}"))?;

                // Read the body
                let mut body = vec![0u8; length];
                reader.read_exact(&mut body).await
                    .map_err(|e| format!("Failed to read LSP response body: {e}"))?;

                let body_str = String::from_utf8_lossy(&body);
                let response: serde_json::Value = serde_json::from_str(&body_str)
                    .map_err(|e| format!("Failed to parse LSP response: {e} | body: {body_str}"))?;

                // Return if this is the response to our request
                if response.get("id").and_then(|v| v.as_i64()) == Some(id) {
                    return Ok(response);
                }
                // Otherwise it might be a notification; continue reading
            }
        }
    }

    /// Send a JSON-RPC notification (no response expected).
    async fn send_notification(&mut self, method: &str, params: &serde_json::Value) -> Result<(), String> {
        let message = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        self.send_message(&message.to_string()).await
    }

    /// Write a JSON-RPC message to the server's stdin.
    async fn send_message(&mut self, content: &str) -> Result<(), String> {
        let stdin = self.child.stdin.as_mut()
            .ok_or_else(|| "stdin not available".to_string())?;

        let body = content.as_bytes();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        stdin
            .write_all(header.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to LSP server: {e}"))?;
        stdin
            .write_all(body)
            .await
            .map_err(|e| format!("Failed to write body to LSP server: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("Failed to flush LSP server stdin: {e}"))?;

        Ok(())
    }

    /// Send `textDocument/didOpen` so the server knows about the file.
    async fn open_document(&mut self, file_path: &Path, language_id: &str) -> Result<(), String> {
        let content = tokio::fs::read_to_string(file_path)
            .await
            .map_err(|e| format!("Failed to read file for LSP: {e}"))?;

        let uri = path_to_uri(file_path);

        let params = json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": 0,
                "text": content,
            }
        });

        self.send_notification("textDocument/didOpen", &params).await
    }

    /// Send `textDocument/definition` request.
    pub async fn goto_definition(
        &mut self,
        file_path: &Path,
        line: u32,
        character: u32,
        language_id: &str,
    ) -> Result<Vec<LspLocation>, String> {
        self.open_document(file_path, language_id).await?;

        let uri = path_to_uri(file_path);
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });

        let response = self.send_request("textDocument/definition", &params).await?;

        if let Some(error) = response.get("error") {
            return Err(format!("LSP definition error: {error}"));
        }

        let result = response
            .get("result")
            .ok_or("No result in definition response")?;

        parse_locations(result)
    }

    /// Send `textDocument/references` request.
    pub async fn find_references(
        &mut self,
        file_path: &Path,
        line: u32,
        character: u32,
        language_id: &str,
        include_declaration: bool,
    ) -> Result<Vec<LspLocation>, String> {
        self.open_document(file_path, language_id).await?;

        let uri = path_to_uri(file_path);
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": include_declaration }
        });

        let response = self.send_request("textDocument/references", &params).await?;

        if let Some(error) = response.get("error") {
            return Err(format!("LSP references error: {error}"));
        }

        let result = response
            .get("result")
            .ok_or("No result in references response")?;

        parse_locations(result)
    }

    /// Send `textDocument/hover` request.
    pub async fn hover(
        &mut self,
        file_path: &Path,
        line: u32,
        character: u32,
        language_id: &str,
    ) -> Result<Option<HoverResult>, String> {
        self.open_document(file_path, language_id).await?;

        let uri = path_to_uri(file_path);
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });

        let response = self.send_request("textDocument/hover", &params).await?;

        if let Some(error) = response.get("error") {
            return Err(format!("LSP hover error: {error}"));
        }

        let result = response.get("result");

        match result {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(value) => {
                let contents = extract_hover_contents(value.get("contents"));
                let range = value
                    .get("range")
                    .and_then(|r| serde_json::from_value(r.clone()).ok());

                Ok(Some(HoverResult { contents, range }))
            }
        }
    }

    /// Send `textDocument/documentSymbol` request.
    pub async fn document_symbols(
        &mut self,
        file_path: &Path,
        language_id: &str,
    ) -> Result<Vec<DocumentSymbolItem>, String> {
        self.open_document(file_path, language_id).await?;

        let uri = path_to_uri(file_path);
        let params = json!({
            "textDocument": { "uri": uri }
        });

        let response = self.send_request("textDocument/documentSymbol", &params).await?;

        if let Some(error) = response.get("error") {
            return Err(format!("LSP document symbol error: {error}"));
        }

        let result = response
            .get("result")
            .ok_or("No result in document symbol response")?;

        parse_document_symbols(result)
    }

    /// Shut down the language server gracefully.
    pub async fn shutdown(&mut self) -> Result<(), String> {
        // Send shutdown request
        let _ = self.send_request("shutdown", &json!(null)).await;
        // Send exit notification
        let _ = self.send_notification("exit", &json!(null)).await;
        Ok(())
    }
}

/// Convert a file path to a file:// URI.
fn path_to_uri(path: &Path) -> String {
    // On non-Windows, simply prepend file://
    if path.starts_with("/") {
        format!("file://{}", path.display())
    } else {
        format!("file:///{}", path.display())
    }
}

/// Detect language ID from file extension for LSP.
pub fn detect_language_id(file_path: &Path) -> &'static str {
    match file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("rs") => "rust",
        Some("ts") | Some("tsx") => "typescript",
        Some("js") | Some("jsx") => "javascript",
        Some("py") => "python",
        Some("go") => "go",
        Some("java") => "java",
        Some("c") => "c",
        Some("h") => "c",
        Some("cpp") | Some("cc") | Some("cxx") | Some("hpp") => "cpp",
        Some("rb") => "ruby",
        Some("php") => "php",
        Some("cs") => "csharp",
        Some("swift") => "swift",
        Some("kt") | Some("kts") => "kotlin",
        Some("scala") => "scala",
        Some("html") => "html",
        Some("css") | Some("scss") | Some("less") => "css",
        Some("json") => "json",
        Some("yaml") | Some("yml") => "yaml",
        Some("md") => "markdown",
        Some("sh") | Some("bash") | Some("zsh") => "shellscript",
        Some("sql") => "sql",
        Some("r") => "r",
        Some("lua") => "lua",
        Some("zig") => "zig",
        Some("toml") => "toml",
        _ => "plaintext",
    }
}

/// Detect the appropriate language server command for a given language.
fn detect_server_command(language_id: &str) -> Option<(String, Vec<String>)> {
    // 1. Try .lsp.json config discovery first
    if let Some(cmd) = discover_lsp_config_command(language_id) {
        return Some(cmd);
    }

    // 2. Fall back to hardcoded defaults
    match language_id {
        "rust" => Some(("rust-analyzer".to_string(), vec![])),
        "typescript" | "javascript" => Some(("typescript-language-server".to_string(), vec!["--stdio".to_string()])),
        "python" => Some(("pylsp".to_string(), vec![])),
        "go" => Some(("gopls".to_string(), vec![])),
        "java" => Some(("jdtls".to_string(), vec![])),
        "c" | "cpp" => Some(("clangd".to_string(), vec![])),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// .lsp.json Configuration
// ---------------------------------------------------------------------------

/// A single language server entry in `.lsp.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    /// Command to launch the language server.
    pub command: String,
    /// Arguments to pass to the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Map from file extension (with dot) to LSP language ID.
    #[serde(default)]
    pub extension_to_language: HashMap<String, String>,
    /// Initialization options for the server.
    #[serde(default)]
    pub initialization_options: Option<serde_json::Value>,
    /// Server settings.
    #[serde(default)]
    pub settings: Option<serde_json::Value>,
    /// Startup timeout in milliseconds (default: 10000).
    #[serde(default = "default_startup_timeout")]
    pub startup_timeout: u64,
    /// Shutdown timeout in milliseconds (default: 5000).
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout: u64,
    /// Whether to restart on crash (default: true).
    #[serde(default = "default_restart_on_crash")]
    pub restart_on_crash: bool,
    /// Maximum restart attempts (default: 3).
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
}

fn default_startup_timeout() -> u64 { 10000 }
fn default_shutdown_timeout() -> u64 { 5000 }
fn default_restart_on_crash() -> bool { true }
fn default_max_restarts() -> u32 { 3 }

/// The full `.lsp.json` file structure: maps language names to server configs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspConfig {
    /// Map of language name (e.g., "rust", "typescript") to server config.
    #[serde(flatten)]
    pub servers: HashMap<String, LspServerConfig>,
}

/// Discovery paths for `.lsp.json`, in priority order (highest last).
fn lsp_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // User-level
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".shannon").join(".lsp.json"));
        paths.push(home.join(".claude").join(".lsp.json"));
    }

    // Project-level (highest priority)
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".lsp.json"));
    }

    paths
}

/// Load and merge all `.lsp.json` configs found on disk.
pub fn load_lsp_config() -> Option<LspConfig> {
    let mut merged = HashMap::new();

    for path in lsp_config_paths() {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str::<LspConfig>(&content) {
                    merged.extend(config.servers);
                }
            }
        }
    }

    if merged.is_empty() {
        None
    } else {
        Some(LspConfig { servers: merged })
    }
}

/// Resolve a language ID to a server command using `.lsp.json` config.
fn discover_lsp_config_command(language_id: &str) -> Option<(String, Vec<String>)> {
    let config = load_lsp_config()?;
    let server = config.servers.get(language_id)?;
    Some((server.command.clone(), server.args.clone()))
}

/// Parse an LSP result into a list of locations.
/// Handles both single Location and Location[] responses.
fn parse_locations(result: &serde_json::Value) -> Result<Vec<LspLocation>, String> {
    // Single location
    if let Some(_uri) = result.get("uri") {
        let loc: LspLocation = serde_json::from_value(result.clone())
            .map_err(|e| format!("Failed to parse location: {e}"))?;
        return Ok(vec![loc]);
    }

    // Array of locations
    if let Some(arr) = result.as_array() {
        let mut locations = Vec::new();
        for item in arr {
            let loc: LspLocation = serde_json::from_value(item.clone())
                .map_err(|e| format!("Failed to parse location: {e}"))?;
            locations.push(loc);
        }
        return Ok(locations);
    }

    // Null result (no definitions/references found)
    if result.is_null() {
        return Ok(Vec::new());
    }

    Err(format!("Unexpected location response format: {result}"))
}

/// Extract hover contents string from the various LSP hover content forms.
fn extract_hover_contents(value: Option<&serde_json::Value>) -> String {
    match value {
        None => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(v) if v.is_object() => {
            // Could be { language, value } or { kind, value }
            v.get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        Some(v) if v.is_array() => {
            // MarkedString[] - concatenate
            v.as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|item| match item {
                            serde_json::Value::String(s) => s.clone(),
                            obj => obj
                                .get("value")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n")
                })
                .unwrap_or_default()
        }
        Some(other) => other.to_string(),
    }
}

/// Parse document symbols from LSP response.
/// Handles both DocumentSymbol[] (hierarchical) and SymbolInformation[] (flat).
fn parse_document_symbols(result: &serde_json::Value) -> Result<Vec<DocumentSymbolItem>, String> {
    let arr = result
        .as_array()
        .ok_or("Document symbols result is not an array")?;

    let mut symbols = Vec::new();
    for item in arr {
        // Hierarchical DocumentSymbol format
        if item.get("range").is_some() && item.get("selectionRange").is_some() {
            let sym = parse_hierarchical_symbol(item);
            symbols.push(sym);
        }
        // Flat SymbolInformation format
        else if item.get("location").is_some() {
            let sym = parse_symbol_information(item);
            symbols.push(sym);
        }
    }

    Ok(symbols)
}

/// Parse a hierarchical DocumentSymbol.
fn parse_hierarchical_symbol(value: &serde_json::Value) -> DocumentSymbolItem {
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let kind_num = value
        .get("kind")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let kind = symbol_kind_to_string(kind_num);

    let detail = value
        .get("detail")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let range = value
        .get("range")
        .and_then(|r| serde_json::from_value(r.clone()).ok())
        .unwrap_or(LspRange {
            start: LspPosition { line: 0, character: 0 },
            end: LspPosition { line: 0, character: 0 },
        });

    let selection_range = value
        .get("selectionRange")
        .and_then(|r| serde_json::from_value(r.clone()).ok())
        .unwrap_or(range.clone());

    let children = value
        .get("children")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().map(parse_hierarchical_symbol).collect())
        .unwrap_or_default();

    DocumentSymbolItem {
        name,
        kind,
        detail,
        range,
        selection_range,
        children,
    }
}

/// Parse a flat SymbolInformation into a DocumentSymbolItem.
fn parse_symbol_information(value: &serde_json::Value) -> DocumentSymbolItem {
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let kind_num = value
        .get("kind")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let kind = symbol_kind_to_string(kind_num);

    let container_name = value
        .get("containerName")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let location = value
        .get("location")
        .and_then(|l| serde_json::from_value::<LspLocation>(l.clone()).ok());

    let (range, selection_range) = match &location {
        Some(loc) => (loc.range.clone(), loc.range.clone()),
        None => (
            LspRange {
                start: LspPosition { line: 0, character: 0 },
                end: LspPosition { line: 0, character: 0 },
            },
            LspRange {
                start: LspPosition { line: 0, character: 0 },
                end: LspPosition { line: 0, character: 0 },
            },
        ),
    };

    DocumentSymbolItem {
        name,
        kind,
        detail: container_name,
        range,
        selection_range,
        children: Vec::new(),
    }
}

/// Convert LSP SymbolKind number to human-readable string.
fn symbol_kind_to_string(kind: u64) -> String {
    match kind {
        1 => "File".to_string(),
        2 => "Module".to_string(),
        3 => "Namespace".to_string(),
        4 => "Package".to_string(),
        5 => "Class".to_string(),
        6 => "Method".to_string(),
        7 => "Property".to_string(),
        8 => "Field".to_string(),
        9 => "Constructor".to_string(),
        10 => "Enum".to_string(),
        11 => "Signal".to_string(),
        12 => "Function".to_string(),
        13 => "Variable".to_string(),
        14 => "Constant".to_string(),
        15 => "String".to_string(),
        16 => "Number".to_string(),
        17 => "Boolean".to_string(),
        18 => "Array".to_string(),
        19 => "Object".to_string(),
        20 => "Key".to_string(),
        21 => "Null".to_string(),
        22 => "EnumMember".to_string(),
        23 => "Struct".to_string(),
        24 => "Event".to_string(),
        25 => "Operator".to_string(),
        26 => "TypeParameter".to_string(),
        _ => format!("Unknown({kind})"),
    }
}

// ---------------------------------------------------------------------------
// Input / Output types for each tool
// ---------------------------------------------------------------------------

/// Input for GoToDefinitionTool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GoToDefinitionInput {
    /// Absolute path to the file
    pub file_path: String,
    /// 0-based line number
    pub line: u32,
    /// 0-based character offset
    pub character: u32,
}

/// Output for GoToDefinitionTool
#[derive(Debug, Serialize)]
pub struct GoToDefinitionOutput {
    /// Definition locations found
    pub locations: Vec<LspLocation>,
    /// Number of locations
    pub count: usize,
}

/// Input for FindReferencesTool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FindReferencesInput {
    /// Absolute path to the file
    pub file_path: String,
    /// 0-based line number
    pub line: u32,
    /// 0-based character offset
    pub character: u32,
    /// Whether to include the declaration itself (default true)
    #[serde(default = "default_true")]
    pub include_declaration: bool,
}

fn default_true() -> bool {
    true
}

/// Output for FindReferencesTool
#[derive(Debug, Serialize)]
pub struct FindReferencesOutput {
    /// Reference locations found
    pub locations: Vec<LspLocation>,
    /// Number of references
    pub count: usize,
}

/// Input for HoverTool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HoverInput {
    /// Absolute path to the file
    pub file_path: String,
    /// 0-based line number
    pub line: u32,
    /// 0-based character offset
    pub character: u32,
}

/// Output for HoverTool
#[derive(Debug, Serialize)]
pub struct HoverOutput {
    /// Hover information if available
    pub result: Option<HoverResult>,
}

/// Input for DocumentSymbolTool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DocumentSymbolInput {
    /// Absolute path to the file
    pub file_path: String,
}

/// Output for DocumentSymbolTool
#[derive(Debug, Serialize)]
pub struct DocumentSymbolOutput {
    /// Symbols found in the document
    pub symbols: Vec<DocumentSymbolItem>,
    /// Number of symbols
    pub count: usize,
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

/// Go to definition tool
pub struct GoToDefinitionTool {
    description: String,
}

impl Default for GoToDefinitionTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GoToDefinitionTool {
    pub fn new() -> Self {
        Self {
            description: "Find the definition of the symbol at a given position in a file. \
                Uses the language server (e.g., rust-analyzer) to locate where a symbol is defined."
                .to_string(),
        }
    }

    async fn execute_inner(&self, input: GoToDefinitionInput) -> Result<GoToDefinitionOutput, ToolError> {
        let file_path = PathBuf::from(&input.file_path);

        // Validate file exists
        if !file_path.exists() {
            return Err(ToolError::InvalidInput(format!(
                "File not found: {}",
                input.file_path
            )));
        }

        let language_id = detect_language_id(&file_path);
        let (server_cmd, server_args) = detect_server_command(language_id).ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "No language server configured for language '{language_id}'. \
                 Supported languages: rust, typescript, javascript, python, go, java, c, cpp."
            ))
        })?;

        // Check if language server is available
        let which_result = Command::new("which")
            .arg(&server_cmd)
            .output()
            .await;

        let which_ok = match which_result {
            Ok(output) => output,
            Err(_) => {
                return Err(ToolError::ExecutionFailed(format!(
                    "Language server '{server_cmd}' not found. Please install it."
                )));
            }
        };
        if !which_ok.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "Language server '{server_cmd}' not found. Please install it to use code intelligence features."
            )));
        }

        let root_path = find_workspace_root(&file_path);
        let args: Vec<&str> = server_args.iter().map(|s| s.as_str()).collect();

        let mut client = LspClient::launch(&server_cmd, &args, &root_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to start language server: {e}")))?;

        let result = client
            .goto_definition(&file_path, input.line, input.character, language_id)
            .await;

        // Always attempt shutdown
        let _ = client.shutdown().await;

        let locations = result.map_err(ToolError::ExecutionFailed)?;

        Ok(GoToDefinitionOutput {
            count: locations.len(),
            locations,
        })
    }
}

#[async_trait]
impl Tool for GoToDefinitionTool {
    fn name(&self) -> &str {
        "go_to_definition"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file"
                },
                "line": {
                    "type": "integer",
                    "description": "0-based line number"
                },
                "character": {
                    "type": "integer",
                    "description": "0-based character offset on the line"
                }
            },
            "required": ["file_path", "line", "character"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let def_input: GoToDefinitionInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid go_to_definition input: {e}")))?;

        let output = self.execute_inner(def_input).await?;

        let content = if output.locations.is_empty() {
            "No definition found at the given position".to_string()
        } else {
            format!("Found {} definition(s)", output.count)
        };

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("count".to_string(), json!(output.count));
                map.insert("locations".to_string(), json!(output.locations));
                map
            },
        })
    }

    fn category(&self) -> &str {
        "lsp"
    }
    fn is_read_only(&self) -> bool {        true    }
}

/// Find references tool
pub struct FindReferencesTool {
    description: String,
}

impl Default for FindReferencesTool {
    fn default() -> Self {
        Self::new()
    }
}

impl FindReferencesTool {
    pub fn new() -> Self {
        Self {
            description: "Find all references to the symbol at a given position in a file. \
                Uses the language server to locate all usages of a symbol across the codebase."
                .to_string(),
        }
    }

    async fn execute_inner(&self, input: FindReferencesInput) -> Result<FindReferencesOutput, ToolError> {
        let file_path = PathBuf::from(&input.file_path);

        if !file_path.exists() {
            return Err(ToolError::InvalidInput(format!(
                "File not found: {}",
                input.file_path
            )));
        }

        let language_id = detect_language_id(&file_path);
        let (server_cmd, server_args) = detect_server_command(language_id).ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "No language server configured for language '{language_id}'."
            ))
        })?;

        let which_result = Command::new("which")
            .arg(&server_cmd)
            .output()
            .await;

        let which_ok = match which_result {
            Ok(output) => output,
            Err(_) => {
                return Err(ToolError::ExecutionFailed(format!(
                    "Language server '{server_cmd}' not found. Please install it."
                )));
            }
        };
        if !which_ok.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "Language server '{server_cmd}' not found. Please install it."
            )));
        }

        let root_path = find_workspace_root(&file_path);
        let args: Vec<&str> = server_args.iter().map(|s| s.as_str()).collect();

        let mut client = LspClient::launch(&server_cmd, &args, &root_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to start language server: {e}")))?;

        let result = client
            .find_references(
                &file_path,
                input.line,
                input.character,
                language_id,
                input.include_declaration,
            )
            .await;

        let _ = client.shutdown().await;

        let locations = result.map_err(ToolError::ExecutionFailed)?;

        Ok(FindReferencesOutput {
            count: locations.len(),
            locations,
        })
    }
}

#[async_trait]
impl Tool for FindReferencesTool {
    fn name(&self) -> &str {
        "find_references"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file"
                },
                "line": {
                    "type": "integer",
                    "description": "0-based line number"
                },
                "character": {
                    "type": "integer",
                    "description": "0-based character offset on the line"
                },
                "include_declaration": {
                    "type": "boolean",
                    "description": "Whether to include the declaration itself (default: true)"
                }
            },
            "required": ["file_path", "line", "character"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let ref_input: FindReferencesInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid find_references input: {e}")))?;

        let output = self.execute_inner(ref_input).await?;

        let content = if output.locations.is_empty() {
            "No references found for the symbol at the given position".to_string()
        } else {
            format!("Found {} reference(s)", output.count)
        };

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("count".to_string(), json!(output.count));
                map.insert("locations".to_string(), json!(output.locations));
                map
            },
        })
    }

    fn category(&self) -> &str {
        "lsp"
    }
    fn is_read_only(&self) -> bool {        true    }
}

/// Hover information tool
pub struct HoverTool {
    description: String,
}

impl Default for HoverTool {
    fn default() -> Self {
        Self::new()
    }
}

impl HoverTool {
    pub fn new() -> Self {
        Self {
            description: "Get type information and documentation for the symbol at a given position. \
                Uses the language server to provide hover information such as type signatures, \
                doc comments, and other contextual information."
                .to_string(),
        }
    }

    async fn execute_inner(&self, input: HoverInput) -> Result<HoverOutput, ToolError> {
        let file_path = PathBuf::from(&input.file_path);

        if !file_path.exists() {
            return Err(ToolError::InvalidInput(format!(
                "File not found: {}",
                input.file_path
            )));
        }

        let language_id = detect_language_id(&file_path);
        let (server_cmd, server_args) = detect_server_command(language_id).ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "No language server configured for language '{language_id}'."
            ))
        })?;

        let which_result = Command::new("which")
            .arg(&server_cmd)
            .output()
            .await;

        let which_ok = match which_result {
            Ok(output) => output,
            Err(_) => {
                return Err(ToolError::ExecutionFailed(format!(
                    "Language server '{server_cmd}' not found. Please install it."
                )));
            }
        };
        if !which_ok.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "Language server '{server_cmd}' not found. Please install it."
            )));
        }

        let root_path = find_workspace_root(&file_path);
        let args: Vec<&str> = server_args.iter().map(|s| s.as_str()).collect();

        let mut client = LspClient::launch(&server_cmd, &args, &root_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to start language server: {e}")))?;

        let result = client
            .hover(&file_path, input.line, input.character, language_id)
            .await;

        let _ = client.shutdown().await;

        let hover_result = result.map_err(ToolError::ExecutionFailed)?;

        Ok(HoverOutput {
            result: hover_result,
        })
    }
}

#[async_trait]
impl Tool for HoverTool {
    fn name(&self) -> &str {
        "hover"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file"
                },
                "line": {
                    "type": "integer",
                    "description": "0-based line number"
                },
                "character": {
                    "type": "integer",
                    "description": "0-based character offset on the line"
                }
            },
            "required": ["file_path", "line", "character"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let hover_input: HoverInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid hover input: {e}")))?;

        let output = self.execute_inner(hover_input).await?;

        let content = match &output.result {
            None => "No hover information available at the given position".to_string(),
            Some(hover) => hover.contents.clone(),
        };

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert(
                    "hover".to_string(),
                    json!(output.result),
                );
                map
            },
        })
    }

    fn category(&self) -> &str {
        "lsp"
    }
    fn is_read_only(&self) -> bool {        true    }
}

/// Document symbol tool
pub struct DocumentSymbolTool {
    description: String,
}

impl Default for DocumentSymbolTool {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentSymbolTool {
    pub fn new() -> Self {
        Self {
            description: "List all symbols (functions, classes, methods, variables, etc.) \
                in a file. Uses the language server to provide a hierarchical view of the \
                document's symbol structure."
                .to_string(),
        }
    }

    async fn execute_inner(&self, input: DocumentSymbolInput) -> Result<DocumentSymbolOutput, ToolError> {
        let file_path = PathBuf::from(&input.file_path);

        if !file_path.exists() {
            return Err(ToolError::InvalidInput(format!(
                "File not found: {}",
                input.file_path
            )));
        }

        let language_id = detect_language_id(&file_path);
        let (server_cmd, server_args) = detect_server_command(language_id).ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "No language server configured for language '{language_id}'."
            ))
        })?;

        let which_result = Command::new("which")
            .arg(&server_cmd)
            .output()
            .await;

        let which_ok = match which_result {
            Ok(output) => output,
            Err(_) => {
                return Err(ToolError::ExecutionFailed(format!(
                    "Language server '{server_cmd}' not found. Please install it."
                )));
            }
        };
        if !which_ok.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "Language server '{server_cmd}' not found. Please install it."
            )));
        }

        let root_path = find_workspace_root(&file_path);
        let args: Vec<&str> = server_args.iter().map(|s| s.as_str()).collect();

        let mut client = LspClient::launch(&server_cmd, &args, &root_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to start language server: {e}")))?;

        let result = client
            .document_symbols(&file_path, language_id)
            .await;

        let _ = client.shutdown().await;

        let symbols = result.map_err(ToolError::ExecutionFailed)?;

        Ok(DocumentSymbolOutput {
            count: symbols.len(),
            symbols,
        })
    }
}

#[async_trait]
impl Tool for DocumentSymbolTool {
    fn name(&self) -> &str {
        "document_symbol"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file"
                }
            },
            "required": ["file_path"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let sym_input: DocumentSymbolInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid document_symbol input: {e}")))?;

        let output = self.execute_inner(sym_input).await?;

        let content = if output.symbols.is_empty() {
            "No symbols found in the document".to_string()
        } else {
            format!("Found {} symbol(s)", output.count)
        };

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("count".to_string(), json!(output.count));
                map.insert("symbols".to_string(), json!(output.symbols));
                map
            },
        })
    }

    fn category(&self) -> &str {
        "lsp"
    }
    fn is_read_only(&self) -> bool {        true    }
}

// ---------------------------------------------------------------------------
// Workspace Symbol Tool
// ---------------------------------------------------------------------------

/// Input for WorkspaceSymbolTool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceSymbolInput {
    /// Query string to search for (e.g., "MyStruct", "parse_")
    pub query: String,
    /// Optional file path to determine which language server to use.
    /// If not provided, attempts to detect from workspace root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

/// Output for WorkspaceSymbolTool
#[derive(Debug, Serialize)]
pub struct WorkspaceSymbolOutput {
    /// Symbols found across the workspace
    pub symbols: Vec<WorkspaceSymbolItem>,
    /// Number of symbols
    pub count: usize,
}

/// A symbol found via workspace/symbol search
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceSymbolItem {
    /// Symbol name
    pub name: String,
    /// Kind of symbol
    pub kind: String,
    /// Location (URI + range)
    pub location: LspLocation,
    /// Container name (e.g., module or class)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
}

/// Workspace symbol tool
pub struct WorkspaceSymbolTool {
    description: String,
}

impl Default for WorkspaceSymbolTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceSymbolTool {
    pub fn new() -> Self {
        Self {
            description: "Search for symbols across the entire workspace by name. \
                Uses the language server to find functions, classes, structs, etc. \
                matching a query pattern across all files in the project."
                .to_string(),
        }
    }

    async fn execute_inner(&self, input: WorkspaceSymbolInput) -> Result<WorkspaceSymbolOutput, ToolError> {
        if input.query.trim().is_empty() {
            return Err(ToolError::InvalidInput("Query must not be empty".to_string()));
        }

        // Determine language server based on optional file_path or workspace root
        let (language_id, root_path) = if let Some(ref fp) = input.file_path {
            let path = PathBuf::from(fp);
            if !path.exists() {
                return Err(ToolError::InvalidInput(format!("File not found: {fp}")));
            }
            (detect_language_id(&path), find_workspace_root(&path))
        } else {
            // Try to detect from current directory
            let cwd = std::env::current_dir()
                .map_err(|e| ToolError::ExecutionFailed(format!("Cannot get CWD: {e}")))?;
            // Look for a common source file to detect language
            ("rust", cwd.clone())
        };

        let (server_cmd, server_args) = detect_server_command(language_id).ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "No language server configured for language '{language_id}'."
            ))
        })?;

        let which_result = Command::new("which").arg(&server_cmd).output().await;
        let which_ok = match which_result {
            Ok(output) => output,
            Err(_) => {
                return Err(ToolError::ExecutionFailed(format!(
                    "Language server '{server_cmd}' not found. Please install it."
                )));
            }
        };
        if !which_ok.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "Language server '{server_cmd}' not found. Please install it."
            )));
        }

        let args: Vec<&str> = server_args.iter().map(|s| s.as_str()).collect();
        let mut client = LspClient::launch(&server_cmd, &args, &root_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to start language server: {e}")))?;

        let params = json!({
            "query": input.query
        });

        let response = client.send_request("workspace/symbol", &params).await;
        let _ = client.shutdown().await;

        let response = response.map_err(ToolError::ExecutionFailed)?;

        if let Some(error) = response.get("error") {
            return Err(ToolError::ExecutionFailed(format!("LSP workspace symbol error: {error}")));
        }

        let result = response.get("result").ok_or_else(|| ToolError::ExecutionFailed("No result in workspace symbol response".to_string()))?;
        let symbols = parse_workspace_symbols(result)?;

        Ok(WorkspaceSymbolOutput {
            count: symbols.len(),
            symbols,
        })
    }
}

#[async_trait]
impl Tool for WorkspaceSymbolTool {
    fn name(&self) -> &str {
        "workspace_symbol"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query — symbol name or pattern to find"
                },
                "file_path": {
                    "type": "string",
                    "description": "Optional file path to determine which language server to use"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let ws_input: WorkspaceSymbolInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid workspace_symbol input: {e}")))?;

        let output = self.execute_inner(ws_input).await?;

        let content = if output.symbols.is_empty() {
            "No symbols found matching the query".to_string()
        } else {
            format!("Found {} workspace symbol(s)", output.count)
        };

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("count".to_string(), json!(output.count));
                map.insert("symbols".to_string(), json!(output.symbols));
                map
            },
        })
    }

    fn category(&self) -> &str {
        "lsp"
    }
    fn is_read_only(&self) -> bool {        true    }
}

/// Parse workspace symbol results.
fn parse_workspace_symbols(result: &serde_json::Value) -> Result<Vec<WorkspaceSymbolItem>, ToolError> {
    if result.is_null() {
        return Ok(Vec::new());
    }
    let arr = result.as_array().ok_or_else(|| {
        ToolError::ExecutionFailed("Workspace symbol result is not an array".to_string())
    })?;

    let mut symbols = Vec::new();
    for item in arr {
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let kind_num = item.get("kind").and_then(|v| v.as_u64()).unwrap_or(0);
        let kind = symbol_kind_to_string(kind_num);
        let container_name = item.get("containerName").and_then(|v| v.as_str()).map(|s| s.to_string());

        // SymbolInformation has a "location" field, not inline range
        let location = if let Some(loc) = item.get("location") {
            serde_json::from_value::<LspLocation>(loc.clone()).unwrap_or(LspLocation {
                uri: String::new(),
                range: LspRange {
                    start: LspPosition { line: 0, character: 0 },
                    end: LspPosition { line: 0, character: 0 },
                },
            })
        } else {
            LspLocation {
                uri: String::new(),
                range: LspRange {
                    start: LspPosition { line: 0, character: 0 },
                    end: LspPosition { line: 0, character: 0 },
                },
            }
        };

        symbols.push(WorkspaceSymbolItem {
            name,
            kind,
            location,
            container_name,
        });
    }

    Ok(symbols)
}

// ---------------------------------------------------------------------------
// Rename Symbol Tool
// ---------------------------------------------------------------------------

/// Input for RenameSymbolTool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RenameSymbolInput {
    /// Absolute path to the file
    pub file_path: String,
    /// 0-based line number
    pub line: u32,
    /// 0-based character offset
    pub character: u32,
    /// New name for the symbol
    pub new_name: String,
}

/// Output for RenameSymbolTool
#[derive(Debug, Serialize)]
pub struct RenameSymbolOutput {
    /// Number of files modified
    pub files_modified: usize,
    /// Total number of edits
    pub total_edits: usize,
    /// The workspace edit details
    pub changes: serde_json::Value,
}

/// Rename symbol tool
pub struct RenameSymbolTool {
    description: String,
}

impl Default for RenameSymbolTool {
    fn default() -> Self {
        Self::new()
    }
}

impl RenameSymbolTool {
    pub fn new() -> Self {
        Self {
            description: "Rename a symbol (variable, function, class, etc.) across the entire \
                codebase. Uses the language server to find all occurrences and compute the \
                correct edits for renaming."
                .to_string(),
        }
    }

    async fn execute_inner(&self, input: RenameSymbolInput) -> Result<RenameSymbolOutput, ToolError> {
        let file_path = PathBuf::from(&input.file_path);
        if !file_path.exists() {
            return Err(ToolError::InvalidInput(format!("File not found: {}", input.file_path)));
        }

        if input.new_name.trim().is_empty() {
            return Err(ToolError::InvalidInput("new_name must not be empty".to_string()));
        }

        let language_id = detect_language_id(&file_path);
        let (server_cmd, server_args) = detect_server_command(language_id).ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "No language server configured for language '{language_id}'."
            ))
        })?;

        let which_result = Command::new("which").arg(&server_cmd).output().await;
        let which_ok = match which_result {
            Ok(output) => output,
            Err(_) => {
                return Err(ToolError::ExecutionFailed(format!(
                    "Language server '{server_cmd}' not found. Please install it."
                )));
            }
        };
        if !which_ok.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "Language server '{server_cmd}' not found. Please install it."
            )));
        }

        let root_path = find_workspace_root(&file_path);
        let args: Vec<&str> = server_args.iter().map(|s| s.as_str()).collect();
        let mut client = LspClient::launch(&server_cmd, &args, &root_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to start language server: {e}")))?;

        let uri = path_to_uri(&file_path);
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": input.line, "character": input.character },
            "newName": input.new_name
        });

        let response = client.send_request("textDocument/rename", &params).await;
        let _ = client.shutdown().await;

        let response = response.map_err(ToolError::ExecutionFailed)?;

        if let Some(error) = response.get("error") {
            return Err(ToolError::ExecutionFailed(format!("LSP rename error: {error}")));
        }

        let result = response.get("result").cloned().unwrap_or(serde_json::Value::Null);

        // Count edits
        let mut files_modified = 0usize;
        let mut total_edits = 0usize;

        if let Some(changes) = result.get("changes") {
            if let Some(map) = changes.as_object() {
                files_modified = map.len();
                for (_, edits) in map {
                    if let Some(arr) = edits.as_array() {
                        total_edits += arr.len();
                    }
                }
            }
        }

        Ok(RenameSymbolOutput {
            files_modified,
            total_edits,
            changes: result,
        })
    }
}

#[async_trait]
impl Tool for RenameSymbolTool {
    fn name(&self) -> &str {
        "rename_symbol"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file containing the symbol"
                },
                "line": {
                    "type": "integer",
                    "description": "0-based line number of the symbol"
                },
                "character": {
                    "type": "integer",
                    "description": "0-based character offset on the line"
                },
                "new_name": {
                    "type": "string",
                    "description": "The new name for the symbol"
                }
            },
            "required": ["file_path", "line", "character", "new_name"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let rn_input: RenameSymbolInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid rename_symbol input: {e}")))?;

        let output = self.execute_inner(rn_input).await?;

        let content = if output.files_modified == 0 {
            "Cannot rename symbol at the given position (no edits produced)".to_string()
        } else {
            format!(
                "Rename: {} edit(s) across {} file(s)",
                output.total_edits, output.files_modified
            )
        };

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("files_modified".to_string(), json!(output.files_modified));
                map.insert("total_edits".to_string(), json!(output.total_edits));
                map.insert("changes".to_string(), output.changes);
                map
            },
        })
    }

    fn category(&self) -> &str {
        "lsp"
    }
}

// ---------------------------------------------------------------------------
// Code Actions Tool
// ---------------------------------------------------------------------------

/// Input for CodeActionsTool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CodeActionsInput {
    /// Absolute path to the file
    pub file_path: String,
    /// 0-based line number (start)
    pub start_line: u32,
    /// 0-based character offset (start)
    pub start_character: u32,
    /// 0-based line number (end)
    pub end_line: u32,
    /// 0-based character offset (end)
    pub end_character: u32,
}

/// Output for CodeActionsTool
#[derive(Debug, Serialize)]
pub struct CodeActionsOutput {
    /// Available code actions
    pub actions: Vec<CodeActionItem>,
    /// Number of actions
    pub count: usize,
}

/// A single code action (quick fix, refactoring, etc.)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeActionItem {
    /// Title of the action
    pub title: String,
    /// Kind of action (e.g., "quickfix", "refactor.extract")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Whether this action is preferred
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_preferred: Option<bool>,
    /// Diagnostic message that triggered this action (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<serde_json::Value>>,
}

/// Code actions tool
pub struct CodeActionsTool {
    description: String,
}

impl Default for CodeActionsTool {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeActionsTool {
    pub fn new() -> Self {
        Self {
            description: "Get available code actions (quick fixes, refactorings) for a range in a file. \
                Uses the language server to suggest fixes for diagnostics and available refactorings."
                .to_string(),
        }
    }

    async fn execute_inner(&self, input: CodeActionsInput) -> Result<CodeActionsOutput, ToolError> {
        let file_path = PathBuf::from(&input.file_path);
        if !file_path.exists() {
            return Err(ToolError::InvalidInput(format!("File not found: {}", input.file_path)));
        }

        let language_id = detect_language_id(&file_path);
        let (server_cmd, server_args) = detect_server_command(language_id).ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "No language server configured for language '{language_id}'."
            ))
        })?;

        let which_result = Command::new("which").arg(&server_cmd).output().await;
        let which_ok = match which_result {
            Ok(output) => output,
            Err(_) => {
                return Err(ToolError::ExecutionFailed(format!(
                    "Language server '{server_cmd}' not found. Please install it."
                )));
            }
        };
        if !which_ok.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "Language server '{server_cmd}' not found. Please install it."
            )));
        }

        let root_path = find_workspace_root(&file_path);
        let args: Vec<&str> = server_args.iter().map(|s| s.as_str()).collect();
        let mut client = LspClient::launch(&server_cmd, &args, &root_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to start language server: {e}")))?;

        // Open the document first
        client.open_document(&file_path, language_id).await
            .map_err(ToolError::ExecutionFailed)?;

        let uri = path_to_uri(&file_path);
        let params = json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": input.start_line, "character": input.start_character },
                "end": { "line": input.end_line, "character": input.end_character }
            },
            "context": {
                "diagnostics": [],
                "only": null
            }
        });

        let response = client.send_request("textDocument/codeAction", &params).await;
        let _ = client.shutdown().await;

        let response = response.map_err(ToolError::ExecutionFailed)?;

        if let Some(error) = response.get("error") {
            return Err(ToolError::ExecutionFailed(format!("LSP code action error: {error}")));
        }

        let result = response.get("result").ok_or_else(|| ToolError::ExecutionFailed("No result in code action response".to_string()))?;
        let actions = parse_code_actions(result)?;

        Ok(CodeActionsOutput {
            count: actions.len(),
            actions,
        })
    }
}

#[async_trait]
impl Tool for CodeActionsTool {
    fn name(&self) -> &str {
        "code_actions"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file"
                },
                "start_line": {
                    "type": "integer",
                    "description": "0-based start line"
                },
                "start_character": {
                    "type": "integer",
                    "description": "0-based start character"
                },
                "end_line": {
                    "type": "integer",
                    "description": "0-based end line"
                },
                "end_character": {
                    "type": "integer",
                    "description": "0-based end character"
                }
            },
            "required": ["file_path", "start_line", "start_character", "end_line", "end_character"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let ca_input: CodeActionsInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid code_actions input: {e}")))?;

        let output = self.execute_inner(ca_input).await?;

        let content = if output.actions.is_empty() {
            "No code actions available for the given range".to_string()
        } else {
            let titles: Vec<&str> = output.actions.iter().map(|a| a.title.as_str()).collect();
            format!("Found {} code action(s): {}", output.count, titles.join(", "))
        };

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("count".to_string(), json!(output.count));
                map.insert("actions".to_string(), json!(output.actions));
                map
            },
        })
    }

    fn category(&self) -> &str {
        "lsp"
    }
    fn is_read_only(&self) -> bool {        true    }
}

/// Parse code action results from LSP response.
fn parse_code_actions(result: &serde_json::Value) -> Result<Vec<CodeActionItem>, ToolError> {
    if result.is_null() {
        return Ok(Vec::new());
    }
    let arr = result.as_array().ok_or_else(|| {
        ToolError::ExecutionFailed("Code actions result is not an array".to_string())
    })?;

    let mut actions = Vec::new();
    for item in arr {
        // CodeAction objects have a "title" field
        if let Some(title) = item.get("title").and_then(|v| v.as_str()) {
            let kind = item.get("kind").and_then(|v| v.as_str()).map(|s| s.to_string());
            let is_preferred = item.get("isPreferred").and_then(|v| v.as_bool());
            let diagnostics = item.get("diagnostics")
                .and_then(|v| v.as_array()).cloned();

            actions.push(CodeActionItem {
                title: title.to_string(),
                kind,
                is_preferred,
                diagnostics,
            });
        }
        // Command objects have "command" and "title" — extract title
        else if let Some(title) = item.get("title").and_then(|v| v.as_str()) {
            actions.push(CodeActionItem {
                title: title.to_string(),
                kind: None,
                is_preferred: None,
                diagnostics: None,
            });
        }
    }

    Ok(actions)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Walk up from `file_path` looking for a workspace root directory
/// (directory containing Cargo.toml, package.json, go.mod, etc.)
fn find_workspace_root(file_path: &Path) -> PathBuf {
    let mut dir = file_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| file_path.to_path_buf());

    let markers = [
        "Cargo.toml",
        "package.json",
        "go.mod",
        "pyproject.toml",
        "setup.py",
        "pom.xml",
        "build.gradle",
        ".git",
    ];

    loop {
        for marker in &markers {
            let candidate = dir.join(marker);
            if candidate.exists() {
                return dir;
            }
        }

        if !dir.pop() {
            return dir;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- LSP type serialization tests --

    #[test]
    fn test_lsp_position_serialization() {
        let pos = LspPosition { line: 10, character: 5 };
        let json_str = serde_json::to_string(&pos).unwrap();
        assert!(json_str.contains("\"line\":10"));
        assert!(json_str.contains("\"character\":5"));
    }

    #[test]
    fn test_lsp_position_deserialization() {
        let pos: LspPosition = serde_json::from_str("{\"line\":3,\"character\":7}").unwrap();
        assert_eq!(pos.line, 3);
        assert_eq!(pos.character, 7);
    }

    #[test]
    fn test_lsp_position_equality() {
        let a = LspPosition { line: 1, character: 2 };
        let b = LspPosition { line: 1, character: 2 };
        let c = LspPosition { line: 1, character: 3 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_lsp_range_serialization() {
        let range = LspRange {
            start: LspPosition { line: 0, character: 0 },
            end: LspPosition { line: 5, character: 10 },
        };
        let json_str = serde_json::to_string(&range).unwrap();
        assert!(json_str.contains("\"start\""));
        assert!(json_str.contains("\"end\""));
    }

    #[test]
    fn test_lsp_range_deserialization() {
        let range: LspRange = serde_json::from_str(
            "{\"start\":{\"line\":0,\"character\":0},\"end\":{\"line\":5,\"character\":10}}"
        ).unwrap();
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 5);
        assert_eq!(range.end.character, 10);
    }

    #[test]
    fn test_lsp_location_serialization() {
        let loc = LspLocation {
            uri: "file:///test.rs".to_string(),
            range: LspRange {
                start: LspPosition { line: 10, character: 0 },
                end: LspPosition { line: 15, character: 1 },
            },
        };
        let json_str = serde_json::to_string(&loc).unwrap();
        assert!(json_str.contains("file:///test.rs"));
        assert!(json_str.contains("\"line\":10"));
    }

    #[test]
    fn test_lsp_location_deserialization() {
        let loc: LspLocation = serde_json::from_str(
            "{\"uri\":\"file:///test.rs\",\"range\":{\"start\":{\"line\":1,\"character\":0},\"end\":{\"line\":1,\"character\":10}}}"
        ).unwrap();
        assert_eq!(loc.uri, "file:///test.rs");
        assert_eq!(loc.range.start.line, 1);
    }

    #[test]
    fn test_hover_result_serialization() {
        let hover = HoverResult {
            contents: "fn main()".to_string(),
            range: Some(LspRange {
                start: LspPosition { line: 0, character: 0 },
                end: LspPosition { line: 0, character: 7 },
            }),
        };
        let json_str = serde_json::to_string(&hover).unwrap();
        assert!(json_str.contains("fn main()"));
        assert!(json_str.contains("\"range\""));
    }

    #[test]
    fn test_hover_result_without_range() {
        let hover = HoverResult {
            contents: "some type info".to_string(),
            range: None,
        };
        let json_str = serde_json::to_string(&hover).unwrap();
        assert!(json_str.contains("some type info"));
        assert!(!json_str.contains("\"range\"")); // skip_serializing_if = None
    }

    #[test]
    fn test_document_symbol_item_serialization() {
        let sym = DocumentSymbolItem {
            name: "main".to_string(),
            kind: "Function".to_string(),
            detail: Some("fn main()".to_string()),
            range: LspRange {
                start: LspPosition { line: 0, character: 0 },
                end: LspPosition { line: 10, character: 1 },
            },
            selection_range: LspRange {
                start: LspPosition { line: 0, character: 3 },
                end: LspPosition { line: 0, character: 7 },
            },
            children: vec![],
        };
        let json_str = serde_json::to_string(&sym).unwrap();
        assert!(json_str.contains("\"name\":\"main\""));
        assert!(json_str.contains("\"kind\":\"Function\""));
        assert!(json_str.contains("\"detail\":\"fn main()\""));
        assert!(!json_str.contains("\"children\"")); // skip empty
    }

    #[test]
    fn test_document_symbol_item_with_children() {
        let child = DocumentSymbolItem {
            name: "inner".to_string(),
            kind: "Variable".to_string(),
            detail: None,
            range: LspRange {
                start: LspPosition { line: 2, character: 4 },
                end: LspPosition { line: 2, character: 8 },
            },
            selection_range: LspRange {
                start: LspPosition { line: 2, character: 4 },
                end: LspPosition { line: 2, character: 8 },
            },
            children: vec![],
        };
        let sym = DocumentSymbolItem {
            name: "outer".to_string(),
            kind: "Function".to_string(),
            detail: None,
            range: LspRange {
                start: LspPosition { line: 0, character: 0 },
                end: LspPosition { line: 5, character: 1 },
            },
            selection_range: LspRange {
                start: LspPosition { line: 0, character: 3 },
                end: LspPosition { line: 0, character: 8 },
            },
            children: vec![child],
        };
        let json_str = serde_json::to_string(&sym).unwrap();
        assert!(json_str.contains("\"children\""));
        assert!(json_str.contains("\"name\":\"inner\""));
    }

    // -- Language detection tests --

    #[test]
    fn test_detect_language_id_rust() {
        assert_eq!(detect_language_id(Path::new("main.rs")), "rust");
        assert_eq!(detect_language_id(Path::new("/home/user/project/lib.rs")), "rust");
    }

    #[test]
    fn test_detect_language_id_typescript() {
        assert_eq!(detect_language_id(Path::new("app.ts")), "typescript");
        assert_eq!(detect_language_id(Path::new("component.tsx")), "typescript");
    }

    #[test]
    fn test_detect_language_id_javascript() {
        assert_eq!(detect_language_id(Path::new("index.js")), "javascript");
        assert_eq!(detect_language_id(Path::new("App.jsx")), "javascript");
    }

    #[test]
    fn test_detect_language_id_python() {
        assert_eq!(detect_language_id(Path::new("main.py")), "python");
    }

    #[test]
    fn test_detect_language_id_go() {
        assert_eq!(detect_language_id(Path::new("main.go")), "go");
    }

    #[test]
    fn test_detect_language_id_c_cpp() {
        assert_eq!(detect_language_id(Path::new("main.c")), "c");
        assert_eq!(detect_language_id(Path::new("main.cpp")), "cpp");
        assert_eq!(detect_language_id(Path::new("main.cc")), "cpp");
        assert_eq!(detect_language_id(Path::new("header.hpp")), "cpp");
    }

    #[test]
    fn test_detect_language_id_unknown() {
        assert_eq!(detect_language_id(Path::new("data.xyz")), "plaintext");
        assert_eq!(detect_language_id(Path::new("noextension")), "plaintext");
    }

    // -- Server detection tests --

    #[test]
    fn test_detect_server_command_rust() {
        let result = detect_server_command("rust");
        assert!(result.is_some());
        let (cmd, args) = result.unwrap();
        assert_eq!(cmd, "rust-analyzer");
        assert!(args.is_empty());
    }

    #[test]
    fn test_detect_server_command_typescript() {
        let result = detect_server_command("typescript");
        assert!(result.is_some());
        let (cmd, args) = result.unwrap();
        assert_eq!(cmd, "typescript-language-server");
        assert_eq!(args, vec!["--stdio"]);
    }

    #[test]
    fn test_detect_server_command_python() {
        let result = detect_server_command("python");
        assert!(result.is_some());
        let (cmd, _) = result.unwrap();
        assert_eq!(cmd, "pylsp");
    }

    #[test]
    fn test_detect_server_command_unknown() {
        let result = detect_server_command("plaintext");
        assert!(result.is_none());
    }

    // -- URI conversion tests --

    #[test]
    fn test_path_to_uri_absolute() {
        let uri = path_to_uri(Path::new("/home/user/project/main.rs"));
        assert_eq!(uri, "file:///home/user/project/main.rs");
    }

    #[test]
    fn test_path_to_uri_relative() {
        let uri = path_to_uri(Path::new("relative/path.rs"));
        assert!(uri.starts_with("file:///"));
        assert!(uri.ends_with("relative/path.rs"));
    }

    // -- Symbol kind tests --

    #[test]
    fn test_symbol_kind_to_string() {
        assert_eq!(symbol_kind_to_string(1), "File");
        assert_eq!(symbol_kind_to_string(5), "Class");
        assert_eq!(symbol_kind_to_string(6), "Method");
        assert_eq!(symbol_kind_to_string(12), "Function");
        assert_eq!(symbol_kind_to_string(13), "Variable");
        assert_eq!(symbol_kind_to_string(23), "Struct");
        assert_eq!(symbol_kind_to_string(26), "TypeParameter");
        assert_eq!(symbol_kind_to_string(99), "Unknown(99)");
    }

    // -- Location parsing tests --

    #[test]
    fn test_parse_locations_single() {
        let value = json!({
            "uri": "file:///test.rs",
            "range": {
                "start": {"line": 10, "character": 0},
                "end": {"line": 10, "character": 5}
            }
        });
        let locations = parse_locations(&value).unwrap();
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].uri, "file:///test.rs");
        assert_eq!(locations[0].range.start.line, 10);
    }

    #[test]
    fn test_parse_locations_array() {
        let value = json!([
            {
                "uri": "file:///a.rs",
                "range": {
                    "start": {"line": 1, "character": 0},
                    "end": {"line": 1, "character": 5}
                }
            },
            {
                "uri": "file:///b.rs",
                "range": {
                    "start": {"line": 5, "character": 0},
                    "end": {"line": 5, "character": 3}
                }
            }
        ]);
        let locations = parse_locations(&value).unwrap();
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0].uri, "file:///a.rs");
        assert_eq!(locations[1].uri, "file:///b.rs");
    }

    #[test]
    fn test_parse_locations_null() {
        let value = json!(null);
        let locations = parse_locations(&value).unwrap();
        assert!(locations.is_empty());
    }

    #[test]
    fn test_parse_locations_invalid() {
        let value = json!("not a valid location");
        let result = parse_locations(&value);
        assert!(result.is_err());
    }

    // -- Hover content extraction tests --

    #[test]
    fn test_extract_hover_contents_string() {
        let value = json!("fn main() -> Result<()>");
        let contents = extract_hover_contents(Some(&value));
        assert_eq!(contents, "fn main() -> Result<()>");
    }

    #[test]
    fn test_extract_hover_contents_marked_string() {
        let value = json!({"language": "rust", "value": "struct MyStruct"});
        let contents = extract_hover_contents(Some(&value));
        assert_eq!(contents, "struct MyStruct");
    }

    #[test]
    fn test_extract_hover_contents_array() {
        let value = json!([
            {"language": "rust", "value": "pub fn foo()"},
            "Some documentation text"
        ]);
        let contents = extract_hover_contents(Some(&value));
        assert!(contents.contains("pub fn foo()"));
        assert!(contents.contains("Some documentation text"));
    }

    #[test]
    fn test_extract_hover_contents_none() {
        let contents = extract_hover_contents(None);
        assert!(contents.is_empty());
    }

    // -- Document symbol parsing tests --

    #[test]
    fn test_parse_document_symbols_hierarchical() {
        let value = json!([
            {
                "name": "MyStruct",
                "kind": 23,
                "detail": "struct MyStruct",
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 10, "character": 1}
                },
                "selectionRange": {
                    "start": {"line": 0, "character": 7},
                    "end": {"line": 0, "character": 15}
                },
                "children": [
                    {
                        "name": "field",
                        "kind": 8,
                        "detail": "pub u32",
                        "range": {
                            "start": {"line": 1, "character": 4},
                            "end": {"line": 1, "character": 14}
                        },
                        "selectionRange": {
                            "start": {"line": 1, "character": 4},
                            "end": {"line": 1, "character": 9}
                        },
                        "children": []
                    }
                ]
            }
        ]);
        let symbols = parse_document_symbols(&value).unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "MyStruct");
        assert_eq!(symbols[0].kind, "Struct");
        assert_eq!(symbols[0].detail, Some("struct MyStruct".to_string()));
        assert_eq!(symbols[0].children.len(), 1);
        assert_eq!(symbols[0].children[0].name, "field");
        assert_eq!(symbols[0].children[0].kind, "Field");
    }

    #[test]
    fn test_parse_document_symbols_flat() {
        let value = json!([
            {
                "name": "my_function",
                "kind": 12,
                "containerName": "MyModule",
                "location": {
                    "uri": "file:///test.rs",
                    "range": {
                        "start": {"line": 5, "character": 0},
                        "end": {"line": 10, "character": 1}
                    }
                }
            }
        ]);
        let symbols = parse_document_symbols(&value).unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "my_function");
        assert_eq!(symbols[0].kind, "Function");
        assert_eq!(symbols[0].detail, Some("MyModule".to_string()));
        assert!(symbols[0].children.is_empty());
    }

    // -- Input deserialization tests --

    #[test]
    fn test_go_to_definition_input_deserialization() {
        let input: GoToDefinitionInput = serde_json::from_value(json!({
            "file_path": "/test.rs",
            "line": 10,
            "character": 5
        })).unwrap();
        assert_eq!(input.file_path, "/test.rs");
        assert_eq!(input.line, 10);
        assert_eq!(input.character, 5);
    }

    #[test]
    fn test_find_references_input_deserialization() {
        let input: FindReferencesInput = serde_json::from_value(json!({
            "file_path": "/test.rs",
            "line": 10,
            "character": 5,
            "include_declaration": false
        })).unwrap();
        assert_eq!(input.file_path, "/test.rs");
        assert_eq!(input.line, 10);
        assert_eq!(input.character, 5);
        assert!(!input.include_declaration);
    }

    #[test]
    fn test_find_references_input_default_include_declaration() {
        let input: FindReferencesInput = serde_json::from_value(json!({
            "file_path": "/test.rs",
            "line": 10,
            "character": 5
        })).unwrap();
        assert!(input.include_declaration); // defaults to true
    }

    #[test]
    fn test_hover_input_deserialization() {
        let input: HoverInput = serde_json::from_value(json!({
            "file_path": "/test.rs",
            "line": 10,
            "character": 5
        })).unwrap();
        assert_eq!(input.file_path, "/test.rs");
        assert_eq!(input.line, 10);
        assert_eq!(input.character, 5);
    }

    #[test]
    fn test_document_symbol_input_deserialization() {
        let input: DocumentSymbolInput = serde_json::from_value(json!({
            "file_path": "/test.rs"
        })).unwrap();
        assert_eq!(input.file_path, "/test.rs");
    }

    // -- Tool trait tests --

    #[test]
    fn test_go_to_definition_tool_name() {
        let tool = GoToDefinitionTool::new();
        assert_eq!(tool.name(), "go_to_definition");
    }

    #[test]
    fn test_go_to_definition_tool_description() {
        let tool = GoToDefinitionTool::new();
        assert!(tool.description().contains("definition"));
    }

    #[test]
    fn test_go_to_definition_tool_schema() {
        let tool = GoToDefinitionTool::new();
        let schema = tool.input_schema();

        let properties = schema.get("properties").unwrap().as_object().unwrap();
        let required = schema.get("required").unwrap().as_array().unwrap();

        assert!(properties.contains_key("file_path"));
        assert!(properties.contains_key("line"));
        assert!(properties.contains_key("character"));
        assert!(required.contains(&json!("file_path")));
        assert!(required.contains(&json!("line")));
        assert!(required.contains(&json!("character")));
    }

    #[test]
    fn test_find_references_tool_name() {
        let tool = FindReferencesTool::new();
        assert_eq!(tool.name(), "find_references");
    }

    #[test]
    fn test_find_references_tool_description() {
        let tool = FindReferencesTool::new();
        assert!(tool.description().contains("references"));
    }

    #[test]
    fn test_find_references_tool_schema() {
        let tool = FindReferencesTool::new();
        let schema = tool.input_schema();

        let properties = schema.get("properties").unwrap().as_object().unwrap();
        assert!(properties.contains_key("file_path"));
        assert!(properties.contains_key("line"));
        assert!(properties.contains_key("character"));
        assert!(properties.contains_key("include_declaration"));

        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(!required.contains(&json!("include_declaration"))); // optional
    }

    #[test]
    fn test_hover_tool_name() {
        let tool = HoverTool::new();
        assert_eq!(tool.name(), "hover");
    }

    #[test]
    fn test_hover_tool_description() {
        let tool = HoverTool::new();
        assert!(tool.description().contains("type"));
        assert!(tool.description().contains("documentation"));
    }

    #[test]
    fn test_hover_tool_schema() {
        let tool = HoverTool::new();
        let schema = tool.input_schema();

        let properties = schema.get("properties").unwrap().as_object().unwrap();
        assert!(properties.contains_key("file_path"));
        assert!(properties.contains_key("line"));
        assert!(properties.contains_key("character"));
    }

    #[test]
    fn test_document_symbol_tool_name() {
        let tool = DocumentSymbolTool::new();
        assert_eq!(tool.name(), "document_symbol");
    }

    #[test]
    fn test_document_symbol_tool_description() {
        let tool = DocumentSymbolTool::new();
        assert!(tool.description().contains("symbols"));
    }

    #[test]
    fn test_document_symbol_tool_schema() {
        let tool = DocumentSymbolTool::new();
        let schema = tool.input_schema();

        let properties = schema.get("properties").unwrap().as_object().unwrap();
        let required = schema.get("required").unwrap().as_array().unwrap();

        assert!(properties.contains_key("file_path"));
        assert!(required.contains(&json!("file_path")));
    }

    #[test]
    fn test_all_tools_category() {
        let def = GoToDefinitionTool::new();
        let refs = FindReferencesTool::new();
        let hover = HoverTool::new();
        let sym = DocumentSymbolTool::new();
        assert_eq!(def.category(), "lsp");
        assert_eq!(refs.category(), "lsp");
        assert_eq!(hover.category(), "lsp");
        assert_eq!(sym.category(), "lsp");
    }

    // -- Error handling tests --

    #[tokio::test]
    async fn test_go_to_definition_missing_file() {
        let tool = GoToDefinitionTool::new();
        let input = GoToDefinitionInput {
            file_path: "/nonexistent/path/file.rs".to_string(),
            line: 0,
            character: 0,
        };
        let result = tool.execute_inner(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("File not found"));
    }

    #[tokio::test]
    async fn test_find_references_missing_file() {
        let tool = FindReferencesTool::new();
        let input = FindReferencesInput {
            file_path: "/nonexistent/path/file.rs".to_string(),
            line: 0,
            character: 0,
            include_declaration: true,
        };
        let result = tool.execute_inner(input).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_hover_missing_file() {
        let tool = HoverTool::new();
        let input = HoverInput {
            file_path: "/nonexistent/path/file.rs".to_string(),
            line: 0,
            character: 0,
        };
        let result = tool.execute_inner(input).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_document_symbol_missing_file() {
        let tool = DocumentSymbolTool::new();
        let input = DocumentSymbolInput {
            file_path: "/nonexistent/path/file.rs".to_string(),
        };
        let result = tool.execute_inner(input).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_go_to_definition_invalid_input() {
        let tool = GoToDefinitionTool::new();
        // Missing required fields
        let result = tool.execute(json!({"file_path": "/test.rs"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_find_references_invalid_input() {
        let tool = FindReferencesTool::new();
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_hover_invalid_input() {
        let tool = HoverTool::new();
        let result = tool.execute(json!({"file_path": "/test.rs"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_document_symbol_invalid_input() {
        let tool = DocumentSymbolTool::new();
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    // -- Workspace root detection tests --

    #[test]
    fn test_find_workspace_root_with_cargo_toml() {
        let path = Path::new("/home/user/project/src/main.rs");
        // We can't test the actual filesystem walk easily, but we can
        // verify it returns a PathBuf (doesn't panic)
        let _root = find_workspace_root(path);
    }

    // -- Output serialization tests --

    #[test]
    fn test_go_to_definition_output_serialization() {
        let output = GoToDefinitionOutput {
            count: 1,
            locations: vec![LspLocation {
                uri: "file:///test.rs".to_string(),
                range: LspRange {
                    start: LspPosition { line: 5, character: 0 },
                    end: LspPosition { line: 5, character: 10 },
                },
            }],
        };
        let json_str = serde_json::to_string(&output).unwrap();
        assert!(json_str.contains("\"count\":1"));
        assert!(json_str.contains("file:///test.rs"));
    }

    #[test]
    fn test_find_references_output_serialization() {
        let output = FindReferencesOutput {
            count: 2,
            locations: vec![
                LspLocation {
                    uri: "file:///a.rs".to_string(),
                    range: LspRange {
                        start: LspPosition { line: 1, character: 0 },
                        end: LspPosition { line: 1, character: 5 },
                    },
                },
                LspLocation {
                    uri: "file:///b.rs".to_string(),
                    range: LspRange {
                        start: LspPosition { line: 3, character: 0 },
                        end: LspPosition { line: 3, character: 5 },
                    },
                },
            ],
        };
        let json_str = serde_json::to_string(&output).unwrap();
        assert!(json_str.contains("\"count\":2"));
    }

    #[test]
    fn test_hover_output_serialization() {
        let output = HoverOutput {
            result: Some(HoverResult {
                contents: "pub fn test()".to_string(),
                range: None,
            }),
        };
        let json_str = serde_json::to_string(&output).unwrap();
        assert!(json_str.contains("pub fn test()"));
    }

    #[test]
    fn test_document_symbol_output_serialization() {
        let output = DocumentSymbolOutput {
            count: 1,
            symbols: vec![DocumentSymbolItem {
                name: "main".to_string(),
                kind: "Function".to_string(),
                detail: Some("fn main()".to_string()),
                range: LspRange {
                    start: LspPosition { line: 0, character: 0 },
                    end: LspPosition { line: 5, character: 1 },
                },
                selection_range: LspRange {
                    start: LspPosition { line: 0, character: 3 },
                    end: LspPosition { line: 0, character: 7 },
                },
                children: vec![],
            }],
        };
        let json_str = serde_json::to_string(&output).unwrap();
        assert!(json_str.contains("\"count\":1"));
        assert!(json_str.contains("\"name\":\"main\""));
    }

    // -- Workspace symbol tests --

    #[test]
    fn test_workspace_symbol_tool_name() {
        let tool = WorkspaceSymbolTool::new();
        assert_eq!(tool.name(), "workspace_symbol");
    }

    #[test]
    fn test_workspace_symbol_tool_description() {
        let tool = WorkspaceSymbolTool::new();
        assert!(tool.description().contains("workspace"));
    }

    #[test]
    fn test_workspace_symbol_tool_schema() {
        let tool = WorkspaceSymbolTool::new();
        let schema = tool.input_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(props.contains_key("query"));
        assert!(required.contains(&json!("query")));
    }

    #[test]
    fn test_workspace_symbol_input_deserialization() {
        let input: WorkspaceSymbolInput = serde_json::from_value(json!({
            "query": "MyStruct"
        })).unwrap();
        assert_eq!(input.query, "MyStruct");
        assert!(input.file_path.is_none());
    }

    #[test]
    fn test_workspace_symbol_input_with_file_path() {
        let input: WorkspaceSymbolInput = serde_json::from_value(json!({
            "query": "parse_",
            "file_path": "/project/src/main.rs"
        })).unwrap();
        assert_eq!(input.query, "parse_");
        assert_eq!(input.file_path, Some("/project/src/main.rs".to_string()));
    }

    #[test]
    fn test_workspace_symbol_output_serialization() {
        let output = WorkspaceSymbolOutput {
            count: 1,
            symbols: vec![WorkspaceSymbolItem {
                name: "MyStruct".to_string(),
                kind: "Struct".to_string(),
                location: LspLocation {
                    uri: "file:///test.rs".to_string(),
                    range: LspRange {
                        start: LspPosition { line: 0, character: 0 },
                        end: LspPosition { line: 5, character: 1 },
                    },
                },
                container_name: Some("my_module".to_string()),
            }],
        };
        let json_str = serde_json::to_string(&output).unwrap();
        assert!(json_str.contains("\"count\":1"));
        assert!(json_str.contains("\"name\":\"MyStruct\""));
    }

    #[test]
    fn test_parse_workspace_symbols() {
        let result = json!([
            {
                "name": "MyStruct",
                "kind": 23,
                "containerName": "my_module",
                "location": {
                    "uri": "file:///test.rs",
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 5, "character": 1}
                    }
                }
            }
        ]);
        let symbols = parse_workspace_symbols(&result).unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "MyStruct");
        assert_eq!(symbols[0].kind, "Struct");
        assert_eq!(symbols[0].container_name, Some("my_module".to_string()));
    }

    #[test]
    fn test_parse_workspace_symbols_null() {
        let result = json!(null);
        let symbols = parse_workspace_symbols(&result).unwrap();
        assert!(symbols.is_empty());
    }

    #[tokio::test]
    async fn test_workspace_symbol_empty_query() {
        let tool = WorkspaceSymbolTool::new();
        let input = WorkspaceSymbolInput {
            query: "  ".to_string(),
            file_path: None,
        };
        let result = tool.execute_inner(input).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_workspace_symbol_missing_file() {
        let tool = WorkspaceSymbolTool::new();
        let input = WorkspaceSymbolInput {
            query: "test".to_string(),
            file_path: Some("/nonexistent/path/file.rs".to_string()),
        };
        let result = tool.execute_inner(input).await;
        assert!(result.is_err());
    }

    // -- Rename symbol tests --

    #[test]
    fn test_rename_symbol_tool_name() {
        let tool = RenameSymbolTool::new();
        assert_eq!(tool.name(), "rename_symbol");
    }

    #[test]
    fn test_rename_symbol_tool_description() {
        let tool = RenameSymbolTool::new();
        assert!(tool.description().contains("Rename"));
    }

    #[test]
    fn test_rename_symbol_tool_schema() {
        let tool = RenameSymbolTool::new();
        let schema = tool.input_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(props.contains_key("file_path"));
        assert!(props.contains_key("line"));
        assert!(props.contains_key("character"));
        assert!(props.contains_key("new_name"));
        assert!(required.contains(&json!("new_name")));
    }

    #[test]
    fn test_rename_symbol_input_deserialization() {
        let input: RenameSymbolInput = serde_json::from_value(json!({
            "file_path": "/test.rs",
            "line": 10,
            "character": 5,
            "new_name": "better_name"
        })).unwrap();
        assert_eq!(input.file_path, "/test.rs");
        assert_eq!(input.new_name, "better_name");
    }

    #[test]
    fn test_rename_symbol_output_serialization() {
        let output = RenameSymbolOutput {
            files_modified: 2,
            total_edits: 5,
            changes: json!({"file:///a.rs": [], "file:///b.rs": []}),
        };
        let json_str = serde_json::to_string(&output).unwrap();
        assert!(json_str.contains("\"files_modified\":2"));
        assert!(json_str.contains("\"total_edits\":5"));
    }

    #[tokio::test]
    async fn test_rename_symbol_missing_file() {
        let tool = RenameSymbolTool::new();
        let input = RenameSymbolInput {
            file_path: "/nonexistent/path/file.rs".to_string(),
            line: 0,
            character: 0,
            new_name: "new_name".to_string(),
        };
        let result = tool.execute_inner(input).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rename_symbol_empty_name() {
        let tool = RenameSymbolTool::new();
        let input = RenameSymbolInput {
            file_path: "/test.rs".to_string(),
            line: 0,
            character: 0,
            new_name: "  ".to_string(),
        };
        let result = tool.execute_inner(input).await;
        assert!(result.is_err());
    }

    // -- Code actions tests --

    #[test]
    fn test_code_actions_tool_name() {
        let tool = CodeActionsTool::new();
        assert_eq!(tool.name(), "code_actions");
    }

    #[test]
    fn test_code_actions_tool_description() {
        let tool = CodeActionsTool::new();
        assert!(tool.description().contains("code actions"));
    }

    #[test]
    fn test_code_actions_tool_schema() {
        let tool = CodeActionsTool::new();
        let schema = tool.input_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(props.contains_key("file_path"));
        assert!(props.contains_key("start_line"));
        assert!(props.contains_key("start_character"));
        assert!(props.contains_key("end_line"));
        assert!(props.contains_key("end_character"));
        assert!(required.contains(&json!("end_line")));
    }

    #[test]
    fn test_code_actions_input_deserialization() {
        let input: CodeActionsInput = serde_json::from_value(json!({
            "file_path": "/test.rs",
            "start_line": 10,
            "start_character": 0,
            "end_line": 10,
            "end_character": 20
        })).unwrap();
        assert_eq!(input.file_path, "/test.rs");
        assert_eq!(input.start_line, 10);
        assert_eq!(input.end_character, 20);
    }

    #[test]
    fn test_code_action_item_serialization() {
        let item = CodeActionItem {
            title: "Add import".to_string(),
            kind: Some("quickfix".to_string()),
            is_preferred: Some(true),
            diagnostics: None,
        };
        let json_str = serde_json::to_string(&item).unwrap();
        assert!(json_str.contains("\"title\":\"Add import\""));
        assert!(json_str.contains("\"kind\":\"quickfix\""));
        assert!(json_str.contains("\"is_preferred\":true"));
    }

    #[test]
    fn test_parse_code_actions() {
        let result = json!([
            {
                "title": "Add import",
                "kind": "quickfix",
                "isPreferred": true
            },
            {
                "title": "Extract function",
                "kind": "refactor.extract"
            }
        ]);
        let actions = parse_code_actions(&result).unwrap();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].title, "Add import");
        assert_eq!(actions[0].kind, Some("quickfix".to_string()));
        assert_eq!(actions[1].title, "Extract function");
    }

    #[test]
    fn test_parse_code_actions_null() {
        let result = json!(null);
        let actions = parse_code_actions(&result).unwrap();
        assert!(actions.is_empty());
    }

    #[tokio::test]
    async fn test_code_actions_missing_file() {
        let tool = CodeActionsTool::new();
        let input = CodeActionsInput {
            file_path: "/nonexistent/path/file.rs".to_string(),
            start_line: 0,
            start_character: 0,
            end_line: 0,
            end_character: 10,
        };
        let result = tool.execute_inner(input).await;
        assert!(result.is_err());
    }

    // -- All LSP tools share category --

    #[test]
    fn test_new_tools_category() {
        assert_eq!(WorkspaceSymbolTool::new().category(), "lsp");
        assert_eq!(RenameSymbolTool::new().category(), "lsp");
        assert_eq!(CodeActionsTool::new().category(), "lsp");
    }

    #[test]
    fn test_all_tools_default() {
        // Verify Default trait works
        let _: WorkspaceSymbolTool = WorkspaceSymbolTool::default();
        let _: RenameSymbolTool = RenameSymbolTool::default();
        let _: CodeActionsTool = CodeActionsTool::default();
    }
}
