//! # LSP Client
//!
//! Low-level client for communicating with a language server over stdio using JSON-RPC.

use std::process::Stdio;

use lsp_types::{
    DocumentSymbolParams, GotoDefinitionParams, Hover, HoverParams, InitializeParams,
    Location, Position, TextDocumentIdentifier, TextDocumentPositionParams, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| LspClientError::SpawnError(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Failed to open stdin",
            )))?;

        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| LspClientError::SpawnError(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Failed to open stdout",
            )))?;

        Ok(Self {
            process,
            writer: BufWriter::new(stdin),
            reader: BufReader::new(stdout),
            next_id: 1,
            initialized: false,
        })
    }

    /// Send a JSON-RPC request and wait for the response
    async fn request<R: Serialize>(
        &mut self,
        method: &str,
        params: R,
    ) -> LspResult<Value> {
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
        self.reader
            .read_line(&mut blank_line)
            .await
            .map_err(|e| LspClientError::ProtocolError(format!("Failed to read blank line: {e}")))?;

        // Read the response body
        let mut body_buffer = vec![0u8; content_length];
        self.reader
            .read_exact(&mut body_buffer)
            .await
            .map_err(|e| LspClientError::ProtocolError(format!("Failed to read body: {e}")))?;

        let body = String::from_utf8(body_buffer)
            .map_err(|e| LspClientError::ProtocolError(format!("Invalid UTF-8 in response: {e}")))?;

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

        let init_result: lsp_types::InitializeResult = serde_json::from_value(result)
            .map_err(|_e| LspClientError::InvalidResponse)?;

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

        let locations: Vec<Location> = serde_json::from_value(result)
            .unwrap_or_default();

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

        let hover: Option<Hover> = serde_json::from_value(result)
            .ok();

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
        assert!(LspClientError::SpawnError(std::io::Error::new(std::io::ErrorKind::NotFound, "missing")).to_string().contains("Failed to spawn"));
        assert!(LspClientError::JsonRpcError("bad request".into()).to_string().contains("bad request"));
        assert!(LspClientError::ProtocolError("handshake failed".into()).to_string().contains("handshake failed"));
        assert!(LspClientError::ServerNotFound("brainfuck".into()).to_string().contains("brainfuck"));
        assert!(LspClientError::InvalidUri.to_string().contains("Invalid URI"));
        assert!(LspClientError::ServerError("crash".into()).to_string().contains("crash"));
        assert!(LspClientError::InvalidResponse.to_string().contains("Invalid response"));
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
        let json = r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32600,"message":"Invalid Request"}}"#;
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
