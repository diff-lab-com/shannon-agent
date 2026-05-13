// Transport layer for MCP communication
//
// This module defines the transport abstraction and implementations
// for different communication protocols: stdio, SSE, HTTP, and WebSocket.

use async_trait::async_trait;
use futures_util::{StreamExt, SinkExt};
use std::io;
use std::pin::Pin;
use std::process::Stdio;
use thiserror::Error;
use tokio_tungstenite::tungstenite::protocol::{Message, CloseFrame};
use tracing::{debug, info};

/// Validate a URL string has the expected scheme.
fn validate_url(url: &str, expected_scheme: &str) -> Result<(), TransportError> {
    if url.is_empty() {
        return Err(TransportError::Http(format!("empty URL for {expected_scheme} transport")));
    }
    if !url.starts_with(expected_scheme) {
        return Err(TransportError::Http(format!(
            "expected {expected_scheme} URL, got: {url}"
        )));
    }
    Ok(())
}

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
    child: Option<tokio::process::Child>,
    stdin: Option<tokio::process::ChildStdin>,
    stdout_reader: Option<tokio::io::BufReader<tokio::process::ChildStdout>>,
}

impl StdioTransport {
    /// Create a new stdio transport by spawning a process
    pub fn new(command: &str, args: &[&str]) -> Result<Self, TransportError> {
        info!("Spawning stdio process: {} {:?}", command, args);

        let mut child = tokio::process::Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| TransportError::Process(format!("Failed to spawn process: {e}")))?;

        let stdin = child.stdin.take().ok_or_else(|| {
            TransportError::Process("Failed to open stdin".to_string())
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            TransportError::Process("Failed to open stdout".to_string())
        })?;

        let stdout_reader = tokio::io::BufReader::new(stdout);

        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout_reader: Some(stdout_reader),
        })
    }

    /// Create from an already spawned child process
    pub fn from_child(mut child: tokio::process::Child) -> Result<Self, TransportError> {
        let stdin = child.stdin.take().ok_or_else(|| {
            TransportError::Process("Failed to open stdin".to_string())
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            TransportError::Process("Failed to open stdout".to_string())
        })?;

        let stdout_reader = tokio::io::BufReader::new(stdout);

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
            use tokio::io::AsyncWriteExt;
            stdin.write_all(message.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
            debug!("Sent stdio message: {} bytes", message.len());
            Ok(())
        } else {
            Err(TransportError::ConnectionClosed)
        }
    }

    async fn receive(&mut self) -> Result<Option<String>, TransportError> {
        if let Some(ref mut reader) = self.stdout_reader {
            let mut line = String::new();
            use tokio::io::AsyncBufReadExt;
            match reader.read_line(&mut line).await {
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
            if let Err(e) = child.kill().await {
                debug!("Failed to kill child process: {}", e);
            }
            // Wait for the process to exit to prevent zombie processes
            match child.wait().await {
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
            // Clean up the child process to prevent resource leaks.
            // kill_on_drop(true) is set, so the child will be killed when dropped.
            // Best-effort synchronous kill — start_kill is non-async.
            if let Err(e) = child.start_kill() {
                tracing::warn!("Failed to kill child process during drop: {e}");
            }
        }
    }
}

/// Server-Sent Events (SSE) transport for remote MCP servers
///
/// Implements bidirectional communication using:
/// - SSE (GET) for server-to-client messages
/// - HTTP POST for client-to-server messages
///
/// Features automatic reconnection with exponential backoff when the
/// SSE stream disconnects unexpectedly.
///
/// Note: This transport is NOT Sync due to the nature of streaming connections.
/// It should be used within a single async context or wrapped in a mutex if needed.
type ByteStream = Pin<Box<dyn futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>;

pub struct SseTransport {
    client: reqwest::Client,
    endpoint: String,
    stream: Option<ByteStream>,
    buffer: String,
    /// Maximum reconnection attempts before giving up.
    max_reconnects: usize,
    /// Whether a reconnect is currently in progress (prevents nested reconnects).
    reconnecting: bool,
    /// Last event ID received (for resumability via `Last-Event-ID` header).
    last_event_id: Option<String>,
    /// MCP session ID (Streamable HTTP).
    session_id: Option<String>,
}

// SAFETY: SseTransport is not thread-safe due to the stream field.
// We explicitly do NOT implement Sync since streams can't be shared across threads.
// This is acceptable for MCP transport usage which typically runs in a single async task.

impl SseTransport {
    /// Create a new SSE transport
    pub fn new(endpoint: impl Into<String>) -> Self {
        let endpoint = endpoint.into();
        if let Err(e) = validate_url(&endpoint, "http") {
            tracing::warn!("SSE transport URL validation: {e}");
        }
        Self {
            client: reqwest::Client::new(),
            endpoint,
            stream: None,
            buffer: String::new(),
            max_reconnects: 3,
            reconnecting: false,
            last_event_id: None,
            session_id: None,
        }
    }

    /// Set the maximum number of reconnection attempts.
    pub fn with_max_reconnects(mut self, max: usize) -> Self {
        self.max_reconnects = max;
        self
    }

    /// Get the current MCP session ID.
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Connect to the SSE endpoint.
    ///
    /// Sends `Last-Event-ID` header if reconnecting after a previous connection
    /// (SSE Resumability per MCP spec). Captures `Mcp-Session-Id` from response.
    pub async fn connect(&mut self) -> Result<(), TransportError> {
        info!("Connecting to SSE endpoint: {}", self.endpoint);

        let mut builder = self
            .client
            .get(&self.endpoint)
            .header("Accept", "text/event-stream");

        // Send Last-Event-ID for resumability if we have a previous event ID.
        if let Some(ref eid) = self.last_event_id {
            debug!(last_event_id = %eid, "SSE reconnecting with Last-Event-ID");
            builder = builder.header("Last-Event-ID", eid.as_str());
        }

        let response = builder
            .send()
            .await
            .map_err(|e| TransportError::Sse(format!("Connection failed: {e}")))?;

        // Capture MCP session ID from response.
        if let Some(sid) = response.headers().get("Mcp-Session-Id") {
            if let Ok(s) = sid.to_str() {
                debug!(session_id = %s, "SSE transport received MCP session ID");
                self.session_id = Some(s.to_string());
            }
        }

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

    /// Attempt to reconnect with exponential backoff.
    ///
    /// Returns `Ok(())` if reconnection succeeded, or the last error if all
    /// attempts are exhausted.
    async fn reconnect(&mut self) -> Result<(), TransportError> {
        if self.reconnecting {
            // Prevent nested reconnects
            return Err(TransportError::Sse("Already reconnecting".to_string()));
        }
        self.reconnecting = true;

        let mut last_err = TransportError::Sse("Reconnection failed".to_string());

        for attempt in 0..self.max_reconnects {
            let delay_ms = 100 * 2u64.pow(attempt as u32);
            info!(
                attempt = attempt + 1,
                max = self.max_reconnects,
                delay_ms,
                "Attempting SSE reconnection"
            );
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;

            match self.connect().await {
                Ok(()) => {
                    info!(attempt = attempt + 1, "SSE reconnection succeeded");
                    self.reconnecting = false;
                    return Ok(());
                }
                Err(e) => {
                    last_err = e;
                }
            }
        }

        self.reconnecting = false;
        Err(last_err)
    }

}

#[async_trait]
impl Transport for SseTransport {
    async fn send(&mut self, message: &str) -> Result<(), TransportError> {
        // For bidirectional SSE, send via HTTP POST
        let mut builder = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(message.to_string());

        // Include MCP session ID if available (Streamable HTTP).
        if let Some(ref sid) = self.session_id {
            builder = builder.header("Mcp-Session-Id", sid.as_str());
        }

        let response = builder
            .send()
            .await
            .map_err(|e| TransportError::Sse(format!("POST request failed: {e}")))?;

        // Capture session ID from response.
        if let Some(sid) = response.headers().get("Mcp-Session-Id") {
            if let Ok(s) = sid.to_str() {
                self.session_id = Some(s.to_string());
            }
        }

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
        loop {
            // Take the stream out to avoid borrow conflicts when reconnecting
            let mut stream = match self.stream.take() {
                Some(s) => s,
                None => {
                    // Not connected — try to connect
                    self.connect().await?;
                    continue;
                }
            };

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
                                    // Put stream back
                                    self.stream = Some(stream);
                                    return Ok(Some(data));
                                }
                            } else if let Some(rest) = line.strip_prefix("data:") {
                                // Accumulate data line
                                let data = rest.trim();
                                if !self.buffer.is_empty() {
                                    self.buffer.push('\n');
                                }
                                self.buffer.push_str(data);
                            } else if let Some(rest) = line.strip_prefix("id:") {
                                // Capture event ID for SSE resumability.
                                let id = rest.trim().to_string();
                                if !id.is_empty() {
                                    self.last_event_id = Some(id);
                                }
                            }
                            // Ignore other SSE fields (event:, retry:)
                        }
                    }
                    Some(Err(e)) => {
                        // Stream error — attempt reconnection
                        debug!(error = %e, "SSE stream error, attempting reconnection");
                        // stream is consumed, don't put it back
                        drop(stream);
                        self.reconnect().await?;
                        break; // back to outer loop to get new stream
                    }
                    None => {
                        // Stream closed gracefully — return any accumulated data first
                        if !self.buffer.is_empty() {
                            let data = self.buffer.clone();
                            self.buffer.clear();
                            // stream is consumed
                            drop(stream);
                            // Try reconnect in background for next receive
                            let _ = self.reconnect().await.map_err(|e| {
                                tracing::warn!("SSE reconnect after stream close failed: {e}");
                                e
                            });
                            return Ok(Some(data));
                        }
                        // Attempt reconnection for graceful close
                        drop(stream);
                        if self.reconnect().await.is_ok() {
                            break; // back to outer loop to get new stream
                        }
                        return Ok(None);
                    }
                }
            }
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
///
/// Uses a request/response pattern: `send()` POSTs a JSON-RPC message and
/// buffers the response. `receive()` returns the buffered response.
///
/// Supports MCP Streamable HTTP (spec 2025-03-26):
/// - Sends `Accept: application/json, text/event-stream` header
/// - Handles both JSON and SSE responses
/// - Tracks `Mcp-Session-Id` for session management
pub struct HttpTransport {
    client: reqwest::Client,
    endpoint: String,
    /// Buffered response from the last POST request.
    pending_response: Option<String>,
    /// MCP session ID (Streamable HTTP).
    session_id: Option<String>,
}

impl HttpTransport {
    /// Create a new HTTP transport
    pub fn new(endpoint: impl Into<String>) -> Self {
        let endpoint = endpoint.into();
        if let Err(e) = validate_url(&endpoint, "http") {
            tracing::warn!("HTTP transport URL validation: {e}");
        }
        Self {
            client: reqwest::Client::new(),
            endpoint,
            pending_response: None,
            session_id: None,
        }
    }

    /// Get the current MCP session ID.
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }
}

#[async_trait]
impl Transport for HttpTransport {
    async fn send(&mut self, message: &str) -> Result<(), TransportError> {
        let mut builder = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(message.to_string());

        // Include MCP session ID if available (Streamable HTTP).
        if let Some(ref sid) = self.session_id {
            builder = builder.header("Mcp-Session-Id", sid.as_str());
        }

        let response = builder
            .send()
            .await
            .map_err(|e| TransportError::Http(format!("Request failed: {e}")))?;

        // Capture session ID from response.
        if let Some(sid) = response.headers().get("Mcp-Session-Id") {
            if let Ok(s) = sid.to_str() {
                debug!(session_id = %s, "HTTP transport received MCP session ID");
                self.session_id = Some(s.to_string());
            }
        }

        if !response.status().is_success() {
            return Err(TransportError::Http(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        // Detect SSE vs JSON response (Streamable HTTP).
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        if content_type.contains("text/event-stream") {
            // SSE response — extract first JSON-RPC payload from events.
            let body = Self::extract_sse_payload(response).await?;
            debug!("HTTP sent+received (SSE): {} bytes request, {} bytes response", message.len(), body.len());
            self.pending_response = Some(body);
        } else {
            // Standard JSON response.
            let body = response
                .text()
                .await
                .map_err(|e| TransportError::Http(format!("Failed to read response: {e}")))?;
            debug!("HTTP sent+received: {} bytes request, {} bytes response", message.len(), body.len());
            self.pending_response = Some(body);
        }

        Ok(())
    }

    async fn receive(&mut self) -> Result<Option<String>, TransportError> {
        // Return the buffered response from the last send()
        Ok(self.pending_response.take())
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        self.pending_response = None;
        // Send DELETE to terminate session if we have a session ID.
        if self.session_id.is_some() {
            let _ = self
                .client
                .delete(&self.endpoint)
                .send()
                .await;
            self.session_id = None;
        }
        Ok(())
    }
}

impl HttpTransport {
    /// Extract the first JSON-RPC payload from an SSE response body.
    async fn extract_sse_payload(response: reqwest::Response) -> Result<String, TransportError> {
        use futures_util::StreamExt;

        let byte_stream = response.bytes_stream();
        let mut stream = Box::pin(byte_stream);
        let mut buffer = String::new();
        let mut event_data = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| TransportError::Sse(format!("Stream error: {e}")))?;
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.is_empty() {
                    if !event_data.is_empty() {
                        return Ok(event_data);
                    }
                } else if let Some(rest) = line.strip_prefix("data:") {
                    let data = rest.trim();
                    if !event_data.is_empty() {
                        event_data.push('\n');
                    }
                    event_data.push_str(data);
                }
            }
        }

        // Return remaining data if any.
        if !event_data.is_empty() {
            return Ok(event_data);
        }

        Err(TransportError::Sse("SSE stream ended without data payload".to_string()))
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
        let endpoint = endpoint.into();
        if let Err(e) = validate_url(&endpoint, "ws") {
            tracing::warn!("WebSocket transport URL validation: {e}");
        }
        Self {
            endpoint,
            stream: None,
        }
    }

    /// Connect to the WebSocket endpoint
    pub async fn connect(&mut self) -> Result<(), TransportError> {
        info!("Connecting to WebSocket: {}", self.endpoint);

        let (stream, _) = tokio_tungstenite::connect_async(&self.endpoint)
            .await
            .map_err(|e| TransportError::WebSocket(format!("Connection failed: {e}")))?;

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
                .map_err(|e| TransportError::WebSocket(format!("Send failed: {e}")))?;
            debug!("WebSocket sent: {} bytes", message.len());
            Ok(())
        } else {
            Err(TransportError::ConnectionClosed)
        }
    }

    async fn receive(&mut self) -> Result<Option<String>, TransportError> {
        if let Some(stream) = &mut self.stream {
            loop {
                match stream.next().await {
                    Some(Ok(msg)) => match msg {
                        Message::Text(text) => {
                            // Utf8Bytes has an as_bytes() method that returns &[u8]
                            match std::str::from_utf8(text.as_bytes()) {
                                Ok(s) => {
                                    debug!("WebSocket received: {} bytes", s.len());
                                    return Ok(Some(s.to_string()));
                                }
                                Err(_) => return Err(TransportError::InvalidMessage("Invalid UTF-8 in text message".to_string()))
                            }
                        }
                        Message::Binary(data) => {
                            // Try to convert binary to string
                            return std::str::from_utf8(&data)
                                .map(|s| Some(s.to_string()))
                                .map_err(|_| TransportError::InvalidMessage("Invalid UTF-8 in binary message".to_string()));
                        }
                        Message::Close(_) => return Ok(None),
                        Message::Ping(data) => {
                            // Respond to ping automatically
                            if let Err(e) = stream.send(Message::Pong(data)).await {
                                tracing::debug!("Failed to send websocket pong: {e}");
                            }
                            continue;
                        }
                        _ => continue,
                    }
                    Some(Err(e)) => return Err(TransportError::WebSocket(format!("Receive error: {e}"))),
                    None => return Ok(None),
                }
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
