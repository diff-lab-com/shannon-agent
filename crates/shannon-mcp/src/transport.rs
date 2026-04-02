// Transport layer for MCP communication
//
// This module defines the transport abstraction and implementations
// for different communication protocols: stdio, SSE, HTTP, and WebSocket.

use crate::{McpError, McpResult};
use async_trait::async_trait;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use thiserror::Error;
use tracing::{debug, info};

/// Transport error types
#[derive(Error, Debug)]
pub enum TransportError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("SSE error: {0}")]
    Sse(String),

    #[error("connection closed")]
    ConnectionClosed,

    #[error("timeout")]
    Timeout,

    #[error("invalid message format: {0}")]
    InvalidMessage(String),

    #[error("process error: {0}")]
    Process(String),
}

/// Transport trait for different communication protocols
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a message to the server
    async fn send(&mut self, message: &str) -> Result<(), TransportError>;

    /// Receive a message from the server (returns None when closed)
    async fn receive(&mut self) -> Result<Option<String>, TransportError>;

    /// Close the transport connection
    async fn close(&mut self) -> Result<(), TransportError>;
}

/// Standard input/output transport for local MCP servers
pub struct StdioTransport {
    child: Option<std::process::Child>,
    stdin: Option<std::process::ChildStdin>,
    stdout_reader: Option<BufReader<std::process::ChildStdout>>,
}

impl StdioTransport {
    /// Create a new stdio transport by spawning a process
    pub fn new(command: &str, args: &[&str]) -> Result<Self, TransportError> {
        info!("Spawning stdio process: {} {:?}", command, args);

        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| TransportError::Process(format!("Failed to spawn process: {}", e)))?;

        let stdin = child.stdin.take().ok_or_else(|| {
            TransportError::Process("Failed to open stdin".to_string())
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            TransportError::Process("Failed to open stdout".to_string())
        })?;

        let stdout_reader = BufReader::new(stdout);

        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout_reader: Some(stdout_reader),
        })
    }

    /// Create from an already spawned child process
    pub fn from_child(mut child: std::process::Child) -> Result<Self, TransportError> {
        let stdin = child.stdin.take().ok_or_else(|| {
            TransportError::Process("Failed to open stdin".to_string())
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            TransportError::Process("Failed to open stdout".to_string())
        })?;

        let stdout_reader = BufReader::new(stdout);

        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout_reader: Some(stdout_reader),
        })
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn send(&mut self, message: &str) -> Result<(), TransportError> {
        if let Some(ref mut stdin) = self.stdin {
            writeln!(stdin, "{}", message)?;
            stdin.flush()?;
            debug!("Sent stdio message: {} bytes", message.len());
            Ok(())
        } else {
            Err(TransportError::ConnectionClosed)
        }
    }

    async fn receive(&mut self) -> Result<Option<String>, TransportError> {
        if let Some(ref mut reader) = self.stdout_reader {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => Ok(None),
                Ok(_) => {
                    let message = line.trim().to_string();
                    debug!("Received stdio message: {} bytes", message.len());
                    Ok(Some(message))
                }
                Err(e) => Err(TransportError::Io(e)),
            }
        } else {
            Err(TransportError::ConnectionClosed)
        }
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
            self.stdin = None;
            self.stdout_reader = None;
            self.child = None;
        }
        Ok(())
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Server-Sent Events (SSE) transport for remote MCP servers
/// NOTE: This is a stub implementation - full SSE support requires additional dependencies
pub struct SseTransport {
    client: reqwest::Client,
    endpoint: String,
}

impl SseTransport {
    /// Create a new SSE transport
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.into(),
        }
    }

    /// Connect to the SSE endpoint
    pub async fn connect(&mut self) -> Result<(), TransportError> {
        info!("Connecting to SSE endpoint: {}", self.endpoint);
        // Stub implementation - SSE will be implemented with proper event source
        Ok(())
    }
}

#[async_trait]
impl Transport for SseTransport {
    async fn send(&mut self, message: &str) -> Result<(), TransportError> {
        // SSE is typically unidirectional (server -> client)
        // For bidirectional communication, we might need HTTP POST alongside SSE
        debug!("SSE send (not fully implemented): {} bytes", message.len());
        Err(TransportError::Sse(
            "SSE is unidirectional, use HTTP POST for sending".to_string(),
        ))
    }

    async fn receive(&mut self) -> Result<Option<String>, TransportError> {
        // This is a stub - proper SSE implementation would use event source
        debug!("SSE receive (stub)");
        Ok(None)
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        Ok(())
    }
}

/// HTTP transport for REST-style MCP communication
pub struct HttpTransport {
    client: reqwest::Client,
    endpoint: String,
}

impl HttpTransport {
    /// Create a new HTTP transport
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.into(),
        }
    }

    /// Send an HTTP POST request
    async fn send_http(&self, message: &str) -> Result<String, TransportError> {
        let response = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .body(message.to_string())
            .send()
            .await
            .map_err(|e| TransportError::Http(format!("Request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(TransportError::Http(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let body = response
            .text()
            .await
            .map_err(|e| TransportError::Http(format!("Failed to read response: {}", e)))?;

        Ok(body)
    }
}

#[async_trait]
impl Transport for HttpTransport {
    async fn send(&mut self, message: &str) -> Result<(), TransportError> {
        // HTTP is request/response, not streaming
        self.send_http(message).await?;
        debug!("Sent HTTP message: {} bytes", message.len());
        Ok(())
    }

    async fn receive(&mut self) -> Result<Option<String>, TransportError> {
        // HTTP is request/response, not streaming
        debug!("HTTP receive (not implemented for streaming)");
        Ok(None)
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        Ok(())
    }
}

/// WebSocket transport for real-time bidirectional communication
/// NOTE: This is a stub implementation - full WebSocket support requires tokio-tungstenite
pub struct WebSocketTransport {
    endpoint: String,
}

impl WebSocketTransport {
    /// Create a new WebSocket transport
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    /// Connect to the WebSocket endpoint
    pub async fn connect(&mut self) -> Result<(), TransportError> {
        info!("Connecting to WebSocket: {}", self.endpoint);
        // Stub implementation - WebSocket will be implemented with tokio-tungstenite
        Ok(())
    }
}

#[async_trait]
impl Transport for WebSocketTransport {
    async fn send(&mut self, message: &str) -> Result<(), TransportError> {
        debug!("WebSocket send (stub): {} bytes", message.len());
        Err(TransportError::WebSocket("Not implemented".to_string()))
    }

    async fn receive(&mut self) -> Result<Option<String>, TransportError> {
        debug!("WebSocket receive (stub)");
        Ok(None)
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_transport_creation() {
        let transport = HttpTransport::new("http://localhost:3000/mcp");
        assert_eq!(transport.endpoint, "http://localhost:3000/mcp");
    }

    #[test]
    fn test_websocket_transport_creation() {
        let transport = WebSocketTransport::new("ws://localhost:3000/mcp");
        assert_eq!(transport.endpoint, "ws://localhost:3000/mcp");
    }

    #[test]
    fn test_sse_transport_creation() {
        let transport = SseTransport::new("http://localhost:3000/events");
        assert_eq!(transport.endpoint, "http://localhost:3000/events");
    }
}
