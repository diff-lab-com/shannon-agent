//! # LSP Client
//!
//! Low-level client for communicating with a language server over stdio using JSON-RPC.

use std::process::Stdio;

use lsp_types::{
    CodeActionContext, CodeActionParams, DocumentSymbolParams, GotoDefinitionParams, Hover,
    HoverParams, InitializeParams, Location, Position, Range, TextDocumentIdentifier,
    TextDocumentPositionParams, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::Command;

use super::LspResult;

/// Error type for LSP client operations
#[derive(Error, Debug)]
pub enum LspClientError {
    #[error("Failed to spawn LSP server process: {0}")]
    SpawnError(#[from] std::io::Error),

    #[error("JSON-RPC error: {0}")]
    JsonRpcError(String),

    #[error("LSP protocol error: {0}")]
    ProtocolError(String),

    #[error("Server not found for language: {0}")]
    ServerNotFound(String),

    #[error("Invalid URI")]
    InvalidUri,

    #[error("Server returned error: {0}")]
    ServerError(String),

    #[error("Invalid response from server")]
    InvalidResponse,

    #[error("Request timed out")]
    Timeout,

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

/// JSON-RPC request sent to the language server
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    params: Value,
}

/// JSON-RPC response received from the language server
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)] // KEEP: deserialized field
    jsonrpc: String,
    id: Option<serde_json::Value>,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

/// JSON-RPC error
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)] // KEEP: deserialized field
    code: i64,
    message: String,
    data: Option<Value>,
}

/// LSP client that communicates over stdio with a language server
pub struct LspClient {
    /// The child process
    process: tokio::process::Child,
    /// Writer for sending requests
    writer: BufWriter<tokio::process::ChildStdin>,
    /// Reader for receiving responses
    reader: BufReader<tokio::process::ChildStdout>,
    /// Next request ID
    next_id: u64,
    /// Whether the client has been initialized
    initialized: bool,
}

impl LspClient {
    /// Spawn a new LSP client by starting the server process
    pub async fn spawn(server_cmd: &str, args: &[String]) -> Result<Self, LspClientError> {
        tracing::debug!("Spawning LSP server: {} with args: {:?}", server_cmd, args);

        let mut process = Command::new(server_cmd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // Pass through stderr for debugging
            .spawn()?;

        let stdin = process.stdin.take().ok_or_else(|| {
            LspClientError::SpawnError(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Failed to open stdin",
            ))
        })?;

        let stdout = process.stdout.take().ok_or_else(|| {
            LspClientError::SpawnError(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Failed to open stdout",
            ))
        })?;

        Ok(Self {
            process,
            writer: BufWriter::new(stdin),
            reader: BufReader::new(stdout),
            next_id: 1,
            initialized: false,
        })
    }

    /// Send a JSON-RPC request and wait for the response
    async fn request<R: Serialize>(&mut self, method: &str, params: R) -> LspResult<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(id)),
            method: method.to_string(),
            params: serde_json::to_value(params)?,
        };

        // Send the request with Content-Length header
        let request_body = serde_json::to_string(&request)
            .map_err(|e| LspClientError::JsonRpcError(e.to_string()))?;

        let header = format!("Content-Length: {}\r\n\r\n", request_body.len());

        tracing::trace!("Sending LSP request: {}", request_body);

        self.writer
            .write_all(header.as_bytes())
            .await
            .map_err(|e| LspClientError::ProtocolError(format!("Failed to write header: {e}")))?;

        self.writer
            .write_all(request_body.as_bytes())
            .await
            .map_err(|e| LspClientError::ProtocolError(format!("Failed to write body: {e}")))?;

        self.writer
            .flush()
            .await
            .map_err(|e| LspClientError::ProtocolError(format!("Failed to flush: {e}")))?;

        // Read the response
        self.read_response(id).await
    }

    /// Read a JSON-RPC response from the server
    async fn read_response(&mut self, expected_id: u64) -> LspResult<Value> {
        // Read Content-Length header
        let mut header_line = String::new();
        self.reader
            .read_line(&mut header_line)
            .await
            .map_err(|e| LspClientError::ProtocolError(format!("Failed to read header: {e}")))?;

        if !header_line.starts_with("Content-Length:") {
            return Err(LspClientError::ProtocolError(format!(
                "Expected Content-Length header, got: {}",
                header_line.trim()
            )));
        }

        let content_length: usize = header_line
            .trim()
            .strip_prefix("Content-Length:")
            .ok_or_else(|| LspClientError::ProtocolError("Missing Content-Length prefix".into()))?
            .trim()
            .parse()
            .map_err(|e| LspClientError::ProtocolError(format!("Invalid content length: {e}")))?;

        // Skip the blank line
        let mut blank_line = String::new();
        self.reader.read_line(&mut blank_line).await.map_err(|e| {
            LspClientError::ProtocolError(format!("Failed to read blank line: {e}"))
        })?;

        // Read the response body
        let mut body_buffer = vec![0u8; content_length];
        self.reader
            .read_exact(&mut body_buffer)
            .await
            .map_err(|e| LspClientError::ProtocolError(format!("Failed to read body: {e}")))?;

        let body = String::from_utf8(body_buffer).map_err(|e| {
            LspClientError::ProtocolError(format!("Invalid UTF-8 in response: {e}"))
        })?;

        tracing::trace!("Received LSP response: {}", body);

        // Parse the response
        let response: JsonRpcResponse = serde_json::from_str(&body)
            .map_err(|e| LspClientError::JsonRpcError(format!("Failed to parse response: {e}")))?;

        // Check for errors
        if let Some(error) = response.error {
            return Err(LspClientError::ServerError(format!(
                "{}: {}",
                error.message,
                error.data.map(|d| d.to_string()).unwrap_or_default()
            )));
        }

        // Verify the request ID matches
        if let Some(id) = response.id {
            if id.as_u64() != Some(expected_id) {
                return Err(LspClientError::ProtocolError(format!(
                    "Response ID mismatch: expected {expected_id}, got {id:?}"
                )));
            }
        }

        Ok(response.result)
    }

    /// Initialize the LSP server with the given root URI
    pub async fn initialize(&mut self, root_uri: &Url) -> LspResult<lsp_types::InitializeResult> {
        tracing::debug!("Initializing LSP server with root: {}", root_uri);

        // Use workspace_folders instead of deprecated root_uri when possible
        let workspace_folder = lsp_types::WorkspaceFolder {
            uri: root_uri.clone(),
            name: root_uri
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .unwrap_or("project")
                .to_string(),
        };

        let params = InitializeParams {
            process_id: Some(std::process::id()),
            workspace_folders: Some(vec![workspace_folder]),
            capabilities: lsp_types::ClientCapabilities {
                ..Default::default()
            },
            ..Default::default()
        };

        let result = self.request("initialize", params).await?;

        let init_result: lsp_types::InitializeResult =
            serde_json::from_value(result).map_err(|_e| LspClientError::InvalidResponse)?;

        self.initialized = true;

        // Send initialized notification
        self.notify("initialized", json!({})).await?;

        Ok(init_result)
    }

    /// Send a notification (no response expected)
    async fn notify<R: Serialize>(&mut self, method: &str, params: R) -> LspResult<()> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None::<Value>,
            method: method.to_string(),
            params: serde_json::to_value(params)?,
        };

        let request_body = serde_json::to_string(&request)
            .map_err(|e| LspClientError::JsonRpcError(e.to_string()))?;

        let header = format!("Content-Length: {}\r\n\r\n", request_body.len());

        self.writer
            .write_all(header.as_bytes())
            .await
            .map_err(|e| LspClientError::ProtocolError(format!("Failed to write header: {e}")))?;

        self.writer
            .write_all(request_body.as_bytes())
            .await
            .map_err(|e| LspClientError::ProtocolError(format!("Failed to write body: {e}")))?;

        self.writer
            .flush()
            .await
            .map_err(|e| LspClientError::ProtocolError(format!("Failed to flush: {e}")))?;

        Ok(())
    }

    /// Go to definition at the given position
    pub async fn goto_definition(
        &mut self,
        uri: &Url,
        position: Position,
    ) -> LspResult<Vec<Location>> {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = self.request("textDocument/definition", params).await?;

        // Handle the response as a Value and convert appropriately
        if result.is_null() {
            return Ok(vec![]);
        }

        // Try to parse as a single location
        if let Ok(location) = serde_json::from_value::<Location>(result.clone()) {
            return Ok(vec![location]);
        }

        // Try to parse as an array of locations
        if let Ok(locations) = serde_json::from_value::<Vec<Location>>(result.clone()) {
            return Ok(locations);
        }

        // Try to parse as LocationLink[] (though less common)
        // For now, just return empty if we can't parse
        Ok(vec![])
    }

    /// Find all references to the symbol at the given position
    pub async fn find_references(
        &mut self,
        uri: &Url,
        position: Position,
    ) -> LspResult<Vec<Location>> {
        let params = lsp_types::ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            context: lsp_types::ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = self.request("textDocument/references", params).await?;

        let locations: Vec<Location> = serde_json::from_value(result).unwrap_or_default();

        Ok(locations)
    }

    /// Get hover information at the given position
    pub async fn hover(&mut self, uri: &Url, position: Position) -> LspResult<Option<Hover>> {
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: Default::default(),
        };

        let result = self.request("textDocument/hover", params).await?;

        let hover: Option<Hover> = serde_json::from_value(result).ok();

        Ok(hover)
    }

    /// Get document symbols for the given URI
    pub async fn document_symbols(
        &mut self,
        uri: &Url,
    ) -> LspResult<Vec<lsp_types::DocumentSymbol>> {
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = self.request("textDocument/documentSymbol", params).await?;

        // Handle both flat and hierarchical symbol responses
        let symbols: Vec<lsp_types::DocumentSymbol> = serde_json::from_value(result.clone())
            .or_else(|_| {
                // Try parsing as hierarchical (array of arrays)
                let nested: Vec<Vec<lsp_types::DocumentSymbol>> = serde_json::from_value(result)?;
                Ok(nested.into_iter().flatten().collect())
            })
            .map_err(|_: serde_json::Error| LspClientError::InvalidResponse)?;

        Ok(symbols)
    }

    /// Request code actions (quick-fixes, refactors) at a position or range.
    ///
    /// Passes the current diagnostics so servers can return context-aware
    /// fixes (e.g. "prefix with _" for an unused-variable warning). Returns the
    /// raw `CodeAction` list — call `resolve_code_action` for any entry that
    /// has `is_preferred == true` and `edit == None` to lazily load the
    /// workspace edit before applying.
    pub async fn code_actions(
        &mut self,
        uri: &Url,
        range: Range,
        diagnostics: &[lsp_types::Diagnostic],
    ) -> LspResult<Vec<lsp_types::CodeAction>> {
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range,
            context: CodeActionContext {
                diagnostics: diagnostics.to_vec(),
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = self.request("textDocument/codeAction", params).await?;

        // Servers may return either `CodeAction[]` or `Command[]`. We surface
        // only CodeAction entries (Commands require server-side execution we
        // can't replicate here) and silently skip the rest.
        let arr = match result {
            Value::Array(a) => a,
            Value::Null => return Ok(Vec::new()),
            _ => return Err(LspClientError::InvalidResponse),
        };
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            // CodeAction objects have a `kind` field or `edit`/`command`.
            if item.get("title").is_some() {
                if let Ok(action) = serde_json::from_value::<lsp_types::CodeAction>(item) {
                    out.push(action);
                }
            }
        }
        Ok(out)
    }

    /// Resolve a partially-loaded CodeAction (e.g. populate its `edit` field).
    pub async fn resolve_code_action(
        &mut self,
        mut action: lsp_types::CodeAction,
    ) -> LspResult<lsp_types::CodeAction> {
        let result = self
            .request::<Value>(
                "codeAction/resolve",
                serde_json::to_value(&action).unwrap_or(Value::Null),
            )
            .await?;
        if let Ok(resolved) = serde_json::from_value::<lsp_types::CodeAction>(result) {
            action = resolved;
        }
        Ok(action)
    }

    /// Open a document in the server's workspace via `textDocument/didOpen`.
    /// Most servers publish diagnostics asynchronously shortly after this
    /// notification — follow up with [`collect_diagnostics`] to drain them.
    pub async fn did_open(&mut self, uri: &Url, language_id: &str, content: &str) -> LspResult<()> {
        let params = json!({
            "textDocument": {
                "uri": uri.to_string(),
                "languageId": language_id,
                "version": 1,
                "text": content,
            }
        });
        self.notify("textDocument/didOpen", params).await
    }

    /// Read messages from the server until either a `publishDiagnostics`
    /// notification arrives for `uri` (returning those diagnostics) or
    /// `timeout` elapses (returning whatever was collected so far, possibly
    /// empty). Non-matching notifications and stray responses are discarded.
    ///
    /// Servers may send multiple `publishDiagnostics` batches for the same
    /// URI (incremental updates); this method returns the **last** batch
    /// received before timeout.
    pub async fn collect_diagnostics(
        &mut self,
        uri: &Url,
        timeout: std::time::Duration,
    ) -> LspResult<Vec<lsp_types::Diagnostic>> {
        let target = uri.to_string();
        let deadline = std::time::Instant::now() + timeout;
        let mut last: Vec<lsp_types::Diagnostic> = Vec::new();
        loop {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .ok_or(LspClientError::Timeout)?;
            // Read one message (header + body), with a deadline.
            let msg = match tokio::time::timeout(remaining, self.read_message()).await {
                Ok(Ok(value)) => value,
                Ok(Err(e)) => return Err(e),
                Err(_) => break, // timeout elapsed
            };
            // Only notifications have a `method` field.
            let method = msg.get("method").and_then(|v| v.as_str());
            if method != Some("textDocument/publishDiagnostics") {
                continue;
            }
            let params = match msg.get("params") {
                Some(p) => p,
                None => continue,
            };
            let notif_uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            if notif_uri != target {
                continue;
            }
            // Replace with the latest batch — servers send incremental updates.
            let diags_val = params
                .get("diagnostics")
                .cloned()
                .unwrap_or(Value::Array(vec![]));
            last = serde_json::from_value(diags_val).unwrap_or_default();
            // Continue the loop in case a fresher batch arrives.
        }
        Ok(last)
    }

    /// Read a single JSON-RPC message (request, response, or notification)
    /// from the server's stdout. Returns the parsed message as a serde_json
    /// Value so callers can inspect `method`, `id`, `params`, or `result`.
    async fn read_message(&mut self) -> LspResult<Value> {
        // Header loop: read line-by-line until blank line, parse Content-Length.
        let mut content_length: Option<usize> = None;
        loop {
            let mut header = String::new();
            let n = self
                .reader
                .read_line(&mut header)
                .await
                .map_err(|e| LspClientError::ProtocolError(format!("read header: {e}")))?;
            if n == 0 {
                return Err(LspClientError::ProtocolError("server closed stream".into()));
            }
            let trimmed = header.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break; // end of headers
            }
            if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
                content_length = rest.trim().parse().ok();
            }
        }
        let len = content_length
            .ok_or_else(|| LspClientError::ProtocolError("no Content-Length".into()))?;
        let mut buf = vec![0u8; len];
        self.reader
            .read_exact(&mut buf)
            .await
            .map_err(|e| LspClientError::ProtocolError(format!("read body: {e}")))?;
        let body = String::from_utf8(buf)
            .map_err(|e| LspClientError::ProtocolError(format!("non-utf8 body: {e}")))?;
        serde_json::from_str(&body)
            .map_err(|e| LspClientError::JsonRpcError(format!("parse message: {e}")))
    }

    /// Shutdown the server gracefully
    pub async fn shutdown(&mut self) -> LspResult<()> {
        if self.initialized {
            self.request::<Value>("shutdown", json!(null)).await?;
            self.notify("exit", json!(null)).await?;
            self.initialized = false;
        }
        Ok(())
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // Kill the child process to prevent leaked file descriptors.
        // We can't do async shutdown in Drop, so we send SIGKILL directly.
        let _ = self.process.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsp_client_error_display_messages() {
        assert!(
            LspClientError::SpawnError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "missing"
            ))
            .to_string()
            .contains("Failed to spawn")
        );
        assert!(
            LspClientError::JsonRpcError("bad request".into())
                .to_string()
                .contains("bad request")
        );
        assert!(
            LspClientError::ProtocolError("handshake failed".into())
                .to_string()
                .contains("handshake failed")
        );
        assert!(
            LspClientError::ServerNotFound("brainfuck".into())
                .to_string()
                .contains("brainfuck")
        );
        assert!(
            LspClientError::InvalidUri
                .to_string()
                .contains("Invalid URI")
        );
        assert!(
            LspClientError::ServerError("crash".into())
                .to_string()
                .contains("crash")
        );
        assert!(
            LspClientError::InvalidResponse
                .to_string()
                .contains("Invalid response")
        );
        assert!(LspClientError::Timeout.to_string().contains("timed out"));
    }

    #[test]
    fn lsp_client_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
        let err: LspClientError = io_err.into();
        assert!(matches!(err, LspClientError::SpawnError(_)));
    }

    #[test]
    fn lsp_client_error_from_serde_json() {
        let json_err = serde_json::from_str::<i32>("not json").unwrap_err();
        let err: LspClientError = json_err.into();
        assert!(matches!(err, LspClientError::SerializationError(_)));
    }

    #[test]
    fn json_rpc_request_serialization() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: json!({"capabilities": {}}),
        };
        let serialized = serde_json::to_string(&req).unwrap();
        assert!(serialized.contains("\"jsonrpc\":\"2.0\""));
        assert!(serialized.contains("\"method\":\"initialize\""));
        assert!(serialized.contains("\"id\":1"));
    }

    #[test]
    fn json_rpc_response_deserialization() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.jsonrpc, "2.0");
        assert!(resp.error.is_none());
    }

    #[test]
    fn json_rpc_response_with_error() {
        let json =
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32600,"message":"Invalid Request"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32600);
        assert_eq!(err.message, "Invalid Request");
    }

    #[test]
    fn send_sync_bounds() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<LspClientError>();
    }
}
