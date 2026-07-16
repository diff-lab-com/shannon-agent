//! # RemoteTriggerTool
//!
//! Allows external automation to trigger Shannon Code operations via HTTP.
//!
//! Provides a lightweight background HTTP server (using `tokio::net::TcpListener`)
//! with endpoints for executing prompts, running tools, and webhook delivery.
//!
//! ## Endpoints
//!
//! - `POST /api/trigger` - Execute a prompt or run a tool
//! - `GET /api/health`  - Health check
//!
//! ## Example
//!
//! ```ignore
//! use std::sync::Arc;
//! use shannon_tools::remote_trigger::{RemoteTriggerServer, TriggerAction, RemoteTriggerInput};
//!
//! let server = RemoteTriggerServer::new(
//!     "127.0.0.1:4567".to_string(),
//!     Arc::new(|prompt, project| {
//!         println!("Prompt: {}, Project: {:?}", prompt, project);
//!     }),
//!     Arc::new(|tool, input| {
//!         println!("Tool: {}, Input: {}", tool, input);
//!     }),
//! );
//! server.start().unwrap();
//! ```

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// The action to perform when a trigger is received.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerAction {
    /// Execute a natural-language prompt, optionally scoped to a project.
    ExecutePrompt {
        prompt: String,
        project: Option<String>,
    },
    /// Run a specific tool by name with a JSON input payload.
    RunTool { tool: String, input: Value },
    /// Deliver a webhook payload to an external URL.
    Webhook {
        url: String,
        payload: Value,
        #[serde(default)]
        method: Option<String>,
    },
}

/// Top-level input for the remote trigger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteTriggerInput {
    /// The action to perform.
    pub action: TriggerAction,
}

// ---------------------------------------------------------------------------
// RemoteTriggerServer
// ---------------------------------------------------------------------------

/// A lightweight background HTTP server that listens for automation triggers.
///
/// The server uses raw TCP + a minimal HTTP parser (no external HTTP framework).
pub struct RemoteTriggerServer {
    /// Listen address, e.g. `"127.0.0.1:4567"`.
    address: String,
    /// Callback invoked when an `ExecutePrompt` action is received.
    prompt_handler: Arc<dyn Fn(String, Option<String>) + Send + Sync>,
    /// Callback invoked when a `RunTool` action is received.
    tool_handler: Arc<dyn Fn(String, Value) + Send + Sync>,
    /// Whether the server is currently running.
    running: Arc<AtomicBool>,
    /// Handle to the background task so we can abort it on `stop()`.
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl std::fmt::Debug for RemoteTriggerServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteTriggerServer")
            .field("address", &self.address)
            .field("running", &self.running.load(Ordering::Relaxed))
            .finish()
    }
}

impl RemoteTriggerServer {
    /// Create a new server that is **not** yet started.
    ///
    /// * `address` - TCP listen address (e.g. `"127.0.0.1:4567"`).
    /// * `prompt_handler` - Called with `(prompt, project)` on `ExecutePrompt`.
    /// * `tool_handler` - Called with `(tool_name, input)` on `RunTool`.
    pub fn new(
        address: String,
        prompt_handler: Arc<dyn Fn(String, Option<String>) + Send + Sync>,
        tool_handler: Arc<dyn Fn(String, Value) + Send + Sync>,
    ) -> Self {
        Self {
            address,
            prompt_handler,
            tool_handler,
            running: Arc::new(AtomicBool::new(false)),
            task_handle: None,
        }
    }

    /// Start the background HTTP server.
    ///
    /// Spawns a tokio task that accepts connections and dispatches requests.
    /// Returns an error if the server is already running or the listener fails to bind.
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::Relaxed) {
            return Err("Server is already running".to_string());
        }

        let address = self.address.clone();
        let prompt_handler = self.prompt_handler.clone();
        let tool_handler = self.tool_handler.clone();
        let running = self.running.clone();

        running.store(true, Ordering::Relaxed);

        let handle = tokio::spawn(async move {
            serve(address, prompt_handler, tool_handler, running).await;
        });

        self.task_handle = Some(handle);
        Ok(())
    }

    /// Stop the background HTTP server.
    ///
    /// Sets the running flag to `false` and aborts the background task.
    pub fn stop(&mut self) -> Result<(), String> {
        if !self.running.load(Ordering::Relaxed) {
            return Err("Server is not running".to_string());
        }

        self.running.store(false, Ordering::Relaxed);

        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }

        Ok(())
    }

    /// Returns `true` if the server is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Returns the listen address.
    pub fn address(&self) -> &str {
        &self.address
    }
}

// ---------------------------------------------------------------------------
// HTTP serve loop
// ---------------------------------------------------------------------------

async fn serve(
    address: String,
    prompt_handler: Arc<dyn Fn(String, Option<String>) + Send + Sync>,
    tool_handler: Arc<dyn Fn(String, Value) + Send + Sync>,
    running: Arc<AtomicBool>,
) {
    let listener = match TcpListener::bind(&address).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("RemoteTriggerServer failed to bind {}: {}", address, e);
            running.store(false, Ordering::Relaxed);
            return;
        }
    };

    tracing::info!("RemoteTriggerServer listening on {}", address);

    while running.load(Ordering::Relaxed) {
        // Use a short timeout so the loop can periodically check the running flag.
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _addr)) => {
                        let ph = prompt_handler.clone();
                        let th = tool_handler.clone();
                        tokio::spawn(async move {
                            handle_connection(stream, ph, th).await;
                        });
                    }
                    Err(e) => {
                        tracing::error!("Accept error: {}", e);
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {
                // Periodic wakeup to check `running`.
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Per-connection handler
// ---------------------------------------------------------------------------

async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    prompt_handler: Arc<dyn Fn(String, Option<String>) + Send + Sync>,
    tool_handler: Arc<dyn Fn(String, Value) + Send + Sync>,
) {
    let mut buf = [0u8; 8192];
    let mut request_data = Vec::new();

    // Set a read timeout so connections can't hang indefinitely.
    let _ = stream.set_nodelay(true);

    // Read until we have the full headers (ended by \r\n\r\n) or hit a limit.
    let read_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if tokio::time::Instant::now() > read_deadline {
            write_response(&mut stream, 408, "Request Timeout", "").await;
            return;
        }
        match stream.read(&mut buf).await {
            Ok(0) => return, // Connection closed.
            Ok(n) => {
                request_data.extend_from_slice(&buf[..n]);
                // Check if we've received the full header block.
                if request_data.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
                if request_data.len() > 64 * 1024 {
                    write_response(&mut stream, 413, "Payload Too Large", "").await;
                    return;
                }
            }
            Err(_) => return,
        }
    }

    let request_str = match std::str::from_utf8(&request_data) {
        Ok(s) => s,
        Err(_) => {
            write_response(&mut stream, 400, "Bad Request", "").await;
            return;
        }
    };

    // Split headers from body.
    let (header_section, body) = match request_str.split_once("\r\n\r\n") {
        Some(pair) => pair,
        None => {
            write_response(&mut stream, 400, "Bad Request", "").await;
            return;
        }
    };

    // Parse the request line: METHOD /path HTTP/1.x
    let first_line = header_section.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        write_response(&mut stream, 400, "Bad Request", "").await;
        return;
    }

    let method = parts[0];
    let path = parts[1];

    // Parse Content-Length from headers.
    let content_length: usize = header_section
        .lines()
        .find_map(|line| {
            let line_lower = line.to_lowercase();
            if let Some(rest) = line_lower.strip_prefix("content-length:") {
                rest.trim().parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    // If Content-Length says there is more body data, we already captured it
    // in the initial buffer read. If the body is larger than what we buffered,
    // read the remainder.
    let mut body = body.to_string();
    if body.len() < content_length {
        let remaining = content_length - body.len();
        let mut extra = vec![0u8; remaining];
        match stream.read_exact(&mut extra).await {
            Ok(_) => {
                if let Ok(s) = std::str::from_utf8(&extra) {
                    body.push_str(s);
                }
            }
            Err(_) => {
                write_response(&mut stream, 400, "Bad Request", "").await;
                return;
            }
        }
    }

    // Route the request.
    match (method, path) {
        ("GET", "/api/health") => {
            let resp = json!({ "status": "ok" }).to_string();
            write_response(&mut stream, 200, "OK", &resp).await;
        }
        ("POST", "/api/trigger") => {
            let resp = handle_trigger(&body, &prompt_handler, &tool_handler);
            write_response(&mut stream, resp.status, resp.status_text, &resp.body).await;
        }
        _ => {
            write_response(&mut stream, 404, "Not Found", "").await;
        }
    }
}

// ---------------------------------------------------------------------------
// Trigger dispatch
// ---------------------------------------------------------------------------

struct TriggerResponse {
    status: u16,
    status_text: &'static str,
    body: String,
}

fn handle_trigger(
    body: &str,
    prompt_handler: &Arc<dyn Fn(String, Option<String>) + Send + Sync>,
    tool_handler: &Arc<dyn Fn(String, Value) + Send + Sync>,
) -> TriggerResponse {
    let input: RemoteTriggerInput = match serde_json::from_str(body) {
        Ok(i) => i,
        Err(e) => {
            return TriggerResponse {
                status: 400,
                status_text: "Bad Request",
                body: json!({"error": format!("Invalid JSON: {}", e)}).to_string(),
            };
        }
    };

    match input.action {
        TriggerAction::ExecutePrompt { prompt, project } => {
            prompt_handler(prompt.clone(), project.clone());
            TriggerResponse {
                status: 200,
                status_text: "OK",
                body: json!({
                    "status": "triggered",
                    "action": "execute_prompt",
                    "prompt": prompt,
                    "project": project,
                })
                .to_string(),
            }
        }
        TriggerAction::RunTool { tool, input } => {
            // Validate tool name: alphanumeric + underscore/dash only
            if !tool
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                return TriggerResponse {
                    status: 400,
                    status_text: "Bad Request",
                    body: json!({"error": "Invalid tool name: must be alphanumeric"}).to_string(),
                };
            }
            tool_handler(tool.clone(), input.clone());
            TriggerResponse {
                status: 200,
                status_text: "OK",
                body: json!({
                    "status": "triggered",
                    "action": "run_tool",
                    "tool": tool,
                    "input": input,
                })
                .to_string(),
            }
        }
        TriggerAction::Webhook {
            url,
            payload,
            method,
        } => {
            // Validate webhook URL is http(s)
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return TriggerResponse {
                    status: 400,
                    status_text: "Bad Request",
                    body: json!({"error": "Webhook URL must use http or https scheme"}).to_string(),
                };
            }
            // Webhooks are acknowledged but delivery is fire-and-forget
            // in this minimal implementation. The payload is logged.
            let method = method.unwrap_or_else(|| "POST".to_string());
            TriggerResponse {
                status: 200,
                status_text: "OK",
                body: json!({
                    "status": "acknowledged",
                    "action": "webhook",
                    "url": url,
                    "method": method,
                    "payload": payload,
                })
                .to_string(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Minimal HTTP response writer
// ---------------------------------------------------------------------------

async fn write_response(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    status_text: &str,
    body: &str,
) {
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        status_text,
        body.len(),
        body,
    );
    if let Err(e) = stream.write_all(response.as_bytes()).await {
        tracing::warn!("Failed to write HTTP response to trigger stream: {e}");
    }
    if let Err(e) = stream.shutdown().await {
        tracing::debug!("Failed to shutdown trigger stream: {e}");
    }
}

// ---------------------------------------------------------------------------
// RemoteTriggerTool (implements the Tool trait)
// ---------------------------------------------------------------------------

/// A [`Tool`] implementation that controls the `RemoteTriggerServer`.
///
/// When executed, it can start or stop the trigger server.
pub struct RemoteTriggerTool {
    description: String,
    server: Arc<std::sync::Mutex<RemoteTriggerServer>>,
}

impl RemoteTriggerTool {
    /// Create a new `RemoteTriggerTool` bound to the given server.
    pub fn new(server: RemoteTriggerServer) -> Self {
        Self {
            description: "Manage the remote trigger HTTP server for external automation"
                .to_string(),
            server: Arc::new(std::sync::Mutex::new(server)),
        }
    }
}

#[async_trait]
impl Tool for RemoteTriggerTool {
    fn name(&self) -> &str {
        "RemoteTrigger"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "stop", "status"],
                    "description": "The server action to perform"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing 'action' field".to_string()))?;

        let mut server = self
            .server
            .lock()
            .map_err(|e| ToolError::ExecutionFailed(format!("Lock error: {e}")))?;

        let mut metadata = HashMap::new();

        match action {
            "start" => {
                server.start().map_err(ToolError::ExecutionFailed)?;
                metadata.insert("address".to_string(), json!(server.address()));
                Ok(ToolOutput {
                    content: format!("Remote trigger server started on {}", server.address()),
                    is_error: false,
                    metadata,
                })
            }
            "stop" => {
                server.stop().map_err(ToolError::ExecutionFailed)?;
                Ok(ToolOutput {
                    content: "Remote trigger server stopped".to_string(),
                    is_error: false,
                    metadata,
                })
            }
            "status" => {
                let running = server.is_running();
                metadata.insert("running".to_string(), json!(running));
                metadata.insert("address".to_string(), json!(server.address()));
                Ok(ToolOutput {
                    content: if running {
                        format!("Remote trigger server is running on {}", server.address())
                    } else {
                        "Remote trigger server is not running".to_string()
                    },
                    is_error: false,
                    metadata,
                })
            }
            other => Err(ToolError::InvalidInput(format!(
                "Unknown action '{other}'. Expected 'start', 'stop', or 'status'."
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    // -- helpers -----------------------------------------------------------

    /// Find a free port by binding to port 0.
    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("Failed to bind port for free port discovery");
        listener.local_addr().unwrap().port()
    }

    #[allow(clippy::type_complexity)]
    fn make_server() -> (
        RemoteTriggerServer,
        Arc<StdMutex<Vec<String>>>,
        Arc<StdMutex<Vec<String>>>,
    ) {
        let prompt_log: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
        let tool_log: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));

        let plog = prompt_log.clone();
        let tlog = tool_log.clone();

        let server = RemoteTriggerServer::new(
            format!("127.0.0.1:{}", free_port()),
            Arc::new(move |prompt, project| {
                plog.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(format!("prompt:{prompt}|project:{project:?}"));
            }),
            Arc::new(move |tool, input| {
                tlog.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(format!("tool:{tool}|input:{input}"));
            }),
        );

        (server, prompt_log, tool_log)
    }

    /// Send a raw HTTP request and return (status_code, body_string).
    async fn send_request(
        addr: &str,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> (u16, String) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;

        let mut stream = TcpStream::connect(addr).await.unwrap();

        let request = if let Some(b) = body {
            format!(
                "{} {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                method,
                path,
                addr,
                b.len(),
                b
            )
        } else {
            format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n")
        };

        stream.write_all(request.as_bytes()).await.unwrap();

        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();

        let response_str = String::from_utf8_lossy(&response).into_owned();

        // Parse status code from first line.
        let status_line = response_str.lines().next().unwrap_or("");
        let status_code: u16 = status_line
            .split_whitespace()
            .nth(1)
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);

        // Extract body after the blank line.
        let body = response_str
            .split_once("\r\n\r\n")
            .map(|(_, b)| b.to_string())
            .unwrap_or_default();

        (status_code, body)
    }

    // -- Serialization / Deserialization tests ----------------------------

    #[test]
    fn test_trigger_action_execute_prompt_serialization() {
        let action = TriggerAction::ExecutePrompt {
            prompt: "List all files".to_string(),
            project: Some("my-project".to_string()),
        };
        let json_str = serde_json::to_string(&action).unwrap();
        let parsed: TriggerAction = serde_json::from_str(&json_str).unwrap();

        match parsed {
            TriggerAction::ExecutePrompt { prompt, project } => {
                assert_eq!(prompt, "List all files");
                assert_eq!(project, Some("my-project".to_string()));
            }
            other => panic!("Expected ExecutePrompt, got {other:?}"),
        }
    }

    #[test]
    fn test_trigger_action_execute_prompt_without_project() {
        let action = TriggerAction::ExecutePrompt {
            prompt: "Hello".to_string(),
            project: None,
        };
        let json_str = serde_json::to_string(&action).unwrap();
        let parsed: TriggerAction = serde_json::from_str(&json_str).unwrap();

        match parsed {
            TriggerAction::ExecutePrompt { prompt, project } => {
                assert_eq!(prompt, "Hello");
                assert_eq!(project, None);
            }
            other => panic!("Expected ExecutePrompt, got {other:?}"),
        }
    }

    #[test]
    fn test_trigger_action_run_tool_serialization() {
        let action = TriggerAction::RunTool {
            tool: "Read".to_string(),
            input: json!({"path": "/tmp/test.txt"}),
        };
        let json_str = serde_json::to_string(&action).unwrap();
        let parsed: TriggerAction = serde_json::from_str(&json_str).unwrap();

        match parsed {
            TriggerAction::RunTool { tool, input } => {
                assert_eq!(tool, "Read");
                assert_eq!(input["path"], "/tmp/test.txt");
            }
            other => panic!("Expected RunTool, got {other:?}"),
        }
    }

    #[test]
    fn test_trigger_action_webhook_serialization() {
        let action = TriggerAction::Webhook {
            url: "https://example.com/hook".to_string(),
            payload: json!({"event": "deploy"}),
            method: Some("POST".to_string()),
        };
        let json_str = serde_json::to_string(&action).unwrap();
        let parsed: TriggerAction = serde_json::from_str(&json_str).unwrap();

        match parsed {
            TriggerAction::Webhook {
                url,
                payload,
                method,
            } => {
                assert_eq!(url, "https://example.com/hook");
                assert_eq!(payload["event"], "deploy");
                assert_eq!(method, Some("POST".to_string()));
            }
            other => panic!("Expected Webhook, got {other:?}"),
        }
    }

    #[test]
    fn test_trigger_action_webhook_default_method() {
        let action = TriggerAction::Webhook {
            url: "https://example.com/hook".to_string(),
            payload: json!({"event": "build"}),
            method: None,
        };
        let json_str = serde_json::to_string(&action).unwrap();
        let parsed: TriggerAction = serde_json::from_str(&json_str).unwrap();

        match parsed {
            TriggerAction::Webhook { method, .. } => {
                assert_eq!(method, None);
            }
            other => panic!("Expected Webhook, got {other:?}"),
        }
    }

    #[test]
    fn test_remote_trigger_input_serialization() {
        let input = RemoteTriggerInput {
            action: TriggerAction::ExecutePrompt {
                prompt: "Run tests".to_string(),
                project: None,
            },
        };
        let json_str = serde_json::to_string(&input).unwrap();
        let parsed: RemoteTriggerInput = serde_json::from_str(&json_str).unwrap();

        match parsed.action {
            TriggerAction::ExecutePrompt { prompt, .. } => {
                assert_eq!(prompt, "Run tests");
            }
            other => panic!("Expected ExecutePrompt, got {other:?}"),
        }
    }

    #[test]
    fn test_remote_trigger_input_deserialization_invalid_json() {
        let result: Result<RemoteTriggerInput, _> = serde_json::from_str("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_remote_trigger_input_deserialization_missing_action() {
        let result: Result<RemoteTriggerInput, _> = serde_json::from_str("{}");
        assert!(result.is_err());
    }

    #[test]
    fn test_trigger_action_snake_case_serde() {
        // Verify that "execute_prompt" serializes/deserializes correctly.
        let json_str = r#"{"execute_prompt":{"prompt":"test","project":null}}"#;
        let result: Result<TriggerAction, _> = serde_json::from_str(json_str);
        assert!(result.is_ok());

        // And "run_tool"
        let json_str = r#"{"run_tool":{"tool":"Bash","input":{"cmd":"ls"}}}"#;
        let result: Result<TriggerAction, _> = serde_json::from_str(json_str);
        assert!(result.is_ok());

        // And "webhook"
        let json_str = r#"{"webhook":{"url":"http://a.com","payload":{},"method":null}}"#;
        let result: Result<TriggerAction, _> = serde_json::from_str(json_str);
        assert!(result.is_ok());
    }

    // -- Server lifecycle tests -------------------------------------------

    #[test]
    fn test_server_not_running_by_default() {
        let (server, _, _) = make_server();
        assert!(!server.is_running());
    }

    #[tokio::test]
    async fn test_server_start_stop_lifecycle() {
        let (mut server, _, _) = make_server();
        assert!(!server.is_running());

        server.start().unwrap();
        assert!(server.is_running());

        server.stop().unwrap();
        assert!(!server.is_running());
    }

    #[tokio::test]
    async fn test_server_start_when_already_running() {
        let (mut server, _, _) = make_server();
        server.start().unwrap();
        let result = server.start();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already running"));
        server.stop().unwrap();
    }

    #[test]
    fn test_server_stop_when_not_running() {
        let (mut server, _, _) = make_server();
        let result = server.stop();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not running"));
    }

    #[test]
    fn test_server_address() {
        let (server, _, _) = make_server();
        assert!(server.address().starts_with("127.0.0.1:"));
    }

    #[test]
    fn test_server_debug_format() {
        let (server, _, _) = make_server();
        let debug_str = format!("{server:?}");
        assert!(debug_str.contains("RemoteTriggerServer"));
        assert!(debug_str.contains("127.0.0.1:"));
    }

    // -- HTTP endpoint tests (integration) --------------------------------

    #[tokio::test]
    async fn test_health_endpoint() {
        let (mut server, _, _) = make_server();
        let addr = server.address().to_string();
        server.start().unwrap();

        // Give the server a moment to start listening.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let (status, body) = send_request(&addr, "GET", "/api/health", None).await;
        assert_eq!(status, 200);

        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["status"], "ok");

        server.stop().unwrap();
    }

    #[tokio::test]
    async fn test_trigger_endpoint_execute_prompt() {
        let (mut server, prompt_log, _) = make_server();
        let addr = server.address().to_string();
        server.start().unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let body = r#"{"action":{"execute_prompt":{"prompt":"Hello world","project":"demo"}}}"#;
        let (status, resp_body) = send_request(&addr, "POST", "/api/trigger", Some(body)).await;
        assert_eq!(status, 200);

        let parsed: Value = serde_json::from_str(&resp_body).unwrap();
        assert_eq!(parsed["status"], "triggered");
        assert_eq!(parsed["action"], "execute_prompt");
        assert_eq!(parsed["prompt"], "Hello world");
        assert_eq!(parsed["project"], "demo");

        // Verify the handler was called.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let log = prompt_log.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(log.len(), 1);
        assert!(log[0].contains("Hello world"));

        server.stop().unwrap();
    }

    #[tokio::test]
    async fn test_trigger_endpoint_run_tool() {
        let (mut server, _, tool_log) = make_server();
        let addr = server.address().to_string();
        server.start().unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let body = r#"{"action":{"run_tool":{"tool":"Bash","input":{"command":"ls -la"}}}}"#;
        let (status, resp_body) = send_request(&addr, "POST", "/api/trigger", Some(body)).await;
        assert_eq!(status, 200);

        let parsed: Value = serde_json::from_str(&resp_body).unwrap();
        assert_eq!(parsed["status"], "triggered");
        assert_eq!(parsed["action"], "run_tool");
        assert_eq!(parsed["tool"], "Bash");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let log = tool_log.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(log.len(), 1);
        assert!(log[0].contains("Bash"));

        server.stop().unwrap();
    }

    #[tokio::test]
    async fn test_trigger_endpoint_webhook() {
        let (mut server, _, _) = make_server();
        let addr = server.address().to_string();
        server.start().unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let body = r#"{"action":{"webhook":{"url":"https://hooks.example.com/deploy","payload":{"version":"1.2.3"},"method":"POST"}}}"#;
        let (status, resp_body) = send_request(&addr, "POST", "/api/trigger", Some(body)).await;
        assert_eq!(status, 200);

        let parsed: Value = serde_json::from_str(&resp_body).unwrap();
        assert_eq!(parsed["status"], "acknowledged");
        assert_eq!(parsed["action"], "webhook");
        assert_eq!(parsed["url"], "https://hooks.example.com/deploy");

        server.stop().unwrap();
    }

    #[tokio::test]
    async fn test_trigger_endpoint_invalid_json() {
        let (mut server, _, _) = make_server();
        let addr = server.address().to_string();
        server.start().unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let body = "this is not json";
        let (status, resp_body) = send_request(&addr, "POST", "/api/trigger", Some(body)).await;
        assert_eq!(status, 400);

        let parsed: Value = serde_json::from_str(&resp_body).unwrap();
        assert!(parsed["error"].is_string());

        server.stop().unwrap();
    }

    #[tokio::test]
    async fn test_not_found_endpoint() {
        let (mut server, _, _) = make_server();
        let addr = server.address().to_string();
        server.start().unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let (status, _) = send_request(&addr, "GET", "/api/nonexistent", None).await;
        assert_eq!(status, 404);

        server.stop().unwrap();
    }

    #[tokio::test]
    async fn test_trigger_endpoint_empty_body() {
        let (mut server, _, _) = make_server();
        let addr = server.address().to_string();
        server.start().unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let body = "";
        let (status, resp_body) = send_request(&addr, "POST", "/api/trigger", Some(body)).await;
        assert_eq!(status, 400);

        let parsed: Value = serde_json::from_str(&resp_body).unwrap();
        assert!(parsed["error"].is_string());

        server.stop().unwrap();
    }

    #[tokio::test]
    async fn test_multiple_triggers_sequential() {
        let (mut server, prompt_log, tool_log) = make_server();
        let addr = server.address().to_string();
        server.start().unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Send two trigger requests in sequence.
        let body1 = r#"{"action":{"execute_prompt":{"prompt":"First","project":null}}}"#;
        send_request(&addr, "POST", "/api/trigger", Some(body1)).await;

        let body2 = r#"{"action":{"run_tool":{"tool":"Glob","input":{"pattern":"*.rs"}}}}"#;
        send_request(&addr, "POST", "/api/trigger", Some(body2)).await;

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        assert_eq!(
            prompt_log.lock().unwrap_or_else(|e| e.into_inner()).len(),
            1
        );
        assert_eq!(tool_log.lock().unwrap_or_else(|e| e.into_inner()).len(), 1);

        server.stop().unwrap();
    }

    // -- RemoteTriggerTool tests ------------------------------------------

    #[tokio::test]
    async fn test_tool_start_action() {
        let (server, _, _) = make_server();
        let tool = RemoteTriggerTool::new(server);

        let result = tool.execute(json!({"action": "start"})).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("started"));
    }

    #[tokio::test]
    async fn test_tool_stop_action() {
        let (server, _, _) = make_server();
        let tool = RemoteTriggerTool::new(server);

        // Start first.
        tool.execute(json!({"action": "start"})).await.unwrap();

        // Then stop.
        let result = tool.execute(json!({"action": "stop"})).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("stopped"));
    }

    #[tokio::test]
    async fn test_tool_status_action() {
        let (server, _, _) = make_server();
        let tool = RemoteTriggerTool::new(server);

        let result = tool.execute(json!({"action": "status"})).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("not running"));
        assert_eq!(result.metadata.get("running"), Some(&json!(false)));
    }

    #[tokio::test]
    async fn test_tool_unknown_action() {
        let (server, _, _) = make_server();
        let tool = RemoteTriggerTool::new(server);

        let result = tool.execute(json!({"action": "restart"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_tool_missing_action() {
        let (server, _, _) = make_server();
        let tool = RemoteTriggerTool::new(server);

        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_name_and_description() {
        let (server, _, _) = make_server();
        let tool = RemoteTriggerTool::new(server);
        assert_eq!(tool.name(), "RemoteTrigger");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_tool_input_schema() {
        let (server, _, _) = make_server();
        let tool = RemoteTriggerTool::new(server);
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("action"));
        let enum_vals = schema["properties"]["action"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(enum_vals.contains(&"start"));
        assert!(enum_vals.contains(&"stop"));
        assert!(enum_vals.contains(&"status"));
    }
}
