// Transport layer for MCP communication
//
// This module defines the transport abstraction and implementations
// for different communication protocols: stdio, SSE, HTTP, and WebSocket.

use crate::{McpError, McpResult};
use async_trait::async_trait;
use futures_util::{StreamExt, SinkExt};
use std::io::{self, BufRead, BufReader, Write};
use std::pin::Pin;
use std::process::{Command, Stdio};
use thiserror::Error;
use tokio_tungstenite::tungstenite::protocol::{Message, CloseFrame};
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
///
/// Note: Transports are NOT required to be Sync because streaming connections
/// (SSE, WebSocket) cannot be shared across threads safely. Use within a single
/// async task or wrap in a mutex if concurrent access is needed.
#[async_trait]
pub trait Transport: Send {
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
        if let Some(mut child) = self.child.take() {
            // Try to kill the child process gracefully
            if let Err(e) = child.kill() {
                debug!("Failed to kill child process: {}", e);
            }
            // Wait for the process to exit to prevent zombie processes
            match child.wait() {
                Ok(status) => {
                    debug!("Child process exited with status: {}", status);
                }
                Err(e) => {
                    debug!("Failed to wait for child process: {}", e);
                }
            }
            self.stdin = None;
            self.stdout_reader = None;
        }
        Ok(())
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Clean up the child process to prevent resource leaks
            if let Err(e) = child.kill() {
                debug!("Failed to kill child process during drop: {}", e);
            }
            // Wait for the process to exit to prevent zombie processes
            // Note: In Drop, we can't do much if wait() fails, but we try anyway
            let _ = child.wait();
        }
    }
}

/// Server-Sent Events (SSE) transport for remote MCP servers
///
/// Implements bidirectional communication using:
/// - SSE (GET) for server-to-client messages
/// - HTTP POST for client-to-server messages
///
/// Note: This transport is NOT Sync due to the nature of streaming connections.
/// It should be used within a single async context or wrapped in a mutex if needed.
pub struct SseTransport {
    client: reqwest::Client,
    endpoint: String,
    stream: Option<Pin<Box<dyn futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>>,
    buffer: String,
}

// SAFETY: SseTransport is not thread-safe due to the stream field.
// We explicitly do NOT implement Sync since streams can't be shared across threads.
// This is acceptable for MCP transport usage which typically runs in a single async task.

impl SseTransport {
    /// Create a new SSE transport
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.into(),
            stream: None,
            buffer: String::new(),
        }
    }

    /// Connect to the SSE endpoint
    pub async fn connect(&mut self) -> Result<(), TransportError> {
        info!("Connecting to SSE endpoint: {}", self.endpoint);

        let response = self
            .client
            .get(&self.endpoint)
            .header("Accept", "text/event-stream")
            .send()
            .await
            .map_err(|e| TransportError::Sse(format!("Connection failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(TransportError::Sse(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let byte_stream = response.bytes_stream();
        self.stream = Some(Box::pin(byte_stream));

        info!("SSE connection established");
        Ok(())
    }

}

#[async_trait]
impl Transport for SseTransport {
    async fn send(&mut self, message: &str) -> Result<(), TransportError> {
        // For bidirectional SSE, send via HTTP POST
        let response = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .body(message.to_string())
            .send()
            .await
            .map_err(|e| TransportError::Sse(format!("POST request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(TransportError::Sse(format!(
                "POST HTTP error: {}",
                response.status()
            )));
        }

        debug!("SSE POST sent: {} bytes", message.len());
        Ok(())
    }

    async fn receive(&mut self) -> Result<Option<String>, TransportError> {
        if let Some(stream) = &mut self.stream {
            loop {
                match stream.next().await {
                    Some(Ok(bytes)) => {
                        let chunk = String::from_utf8_lossy(&bytes);
                        // Process lines and update buffer
                        for line in chunk.lines() {
                            let line = line.trim();
                            if line.is_empty() {
                                // End of event, return accumulated data
                                if !self.buffer.is_empty() {
                                    let data = self.buffer.clone();
                                    self.buffer.clear();
                                    debug!("SSE received: {} bytes", data.len());
                                    return Ok(Some(data));
                                }
                            } else if line.starts_with("data:") {
                                // Accumulate data line
                                let data = line[5..].trim();
                                if !self.buffer.is_empty() {
                                    self.buffer.push('\n');
                                }
                                self.buffer.push_str(data);
                            }
                            // Ignore other SSE fields (event:, id:, retry:)
                        }
                    }
                    Some(Err(e)) => {
                        return Err(TransportError::Sse(format!("Stream error: {}", e)));
                    }
                    None => {
                        // Stream closed - return any accumulated data
                        if !self.buffer.is_empty() {
                            let data = self.buffer.clone();
                            self.buffer.clear();
                            return Ok(Some(data));
                        }
                        return Ok(None);
                    }
                }
            }
        } else {
            // Not connected
            Ok(None)
        }
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        self.stream = None;
        self.buffer.clear();
        debug!("SSE connection closed");
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
pub struct WebSocketTransport {
    endpoint: String,
    stream: Option<tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >>,
}

impl WebSocketTransport {
    /// Create a new WebSocket transport
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            stream: None,
        }
    }

    /// Connect to the WebSocket endpoint
    pub async fn connect(&mut self) -> Result<(), TransportError> {
        info!("Connecting to WebSocket: {}", self.endpoint);

        let (stream, _) = tokio_tungstenite::connect_async(&self.endpoint)
            .await
            .map_err(|e| TransportError::WebSocket(format!("Connection failed: {}", e)))?;

        self.stream = Some(stream);
        info!("WebSocket connection established");
        Ok(())
    }
}

#[async_trait]
impl Transport for WebSocketTransport {
    async fn send(&mut self, message: &str) -> Result<(), TransportError> {
        if let Some(stream) = &mut self.stream {
            let msg = Message::Text(message.into());
            stream
                .send(msg)
                .await
                .map_err(|e| TransportError::WebSocket(format!("Send failed: {}", e)))?;
            debug!("WebSocket sent: {} bytes", message.len());
            Ok(())
        } else {
            Err(TransportError::ConnectionClosed)
        }
    }

    async fn receive(&mut self) -> Result<Option<String>, TransportError> {
        if let Some(stream) = &mut self.stream {
            match stream.next().await {
                Some(Ok(msg)) => match msg {
                    Message::Text(text) => {
                        // Utf8Bytes has an as_bytes() method that returns &[u8]
                        match std::str::from_utf8(text.as_bytes()) {
                            Ok(s) => {
                                debug!("WebSocket received: {} bytes", s.len());
                                Ok(Some(s.to_string()))
                            }
                            Err(_) => Err(TransportError::InvalidMessage("Invalid UTF-8 in text message".to_string()))
                        }
                    }
                    Message::Binary(data) => {
                        // Try to convert binary to string
                        Ok(std::str::from_utf8(&data)
                            .map(|s| Some(s.to_string()))
                            .map_err(|_| TransportError::InvalidMessage("Invalid UTF-8 in binary message".to_string()))?)
                    }
                    Message::Close(_) => Ok(None),
                    Message::Ping(data) => {
                        // Respond to ping automatically
                        let _ = stream.send(Message::Pong(data)).await;
                        // Continue waiting for real message
                        self.receive().await
                    }
                    Message::Pong(_) => {
                        // Ignore pong, continue waiting
                        self.receive().await
                    }
                    _ => {
                        // Other message types, continue waiting
                        self.receive().await
                    }
                }
                Some(Err(e)) => Err(TransportError::WebSocket(format!("Receive error: {}", e))),
                None => Ok(None),
            }
        } else {
            Err(TransportError::ConnectionClosed)
        }
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        if let Some(stream) = &mut self.stream {
            let _ = stream.close(Some(CloseFrame {
                code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal,
                reason: "".into(),
            })).await;
            self.stream = None;
            debug!("WebSocket connection closed");
        }
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
