use super::*;

#[tokio::test]
async fn secret_guard_client_boundary_sends_surrogates_not_secrets() {
    use std::io::Read as _;
    use std::io::Write as _;

    const SECRET: &str = "SUPER-SECRET-VALUE";
    const TOKEN: &str = "SG1:FAKEFAKEFAKEFAKE";

    struct ReplaceSecret;
    impl shannon_plugin_api::ContextTransform for ReplaceSecret {
        fn transform_ingest(
            &self,
            block: &mut shannon_plugin_api::IngestBlock,
        ) -> shannon_plugin_api::TransformAction {
            if block.text.contains(SECRET) {
                block.text = block.text.replace(SECRET, TOKEN);
                shannon_plugin_api::TransformAction::Modified
            } else {
                shannon_plugin_api::TransformAction::Passthrough
            }
        }
        fn restore_tool_args(
            &self,
            _tool: &str,
            _args: &mut serde_json::Value,
        ) -> shannon_plugin_api::RestoreAction {
            shannon_plugin_api::RestoreAction::Unchanged
        }
        fn restore_display(&self, _text: &mut String) -> shannon_plugin_api::RestoreAction {
            shannon_plugin_api::RestoreAction::Unchanged
        }
        fn audit_wire(&self, _wire: &serde_json::Value) -> Vec<shannon_plugin_api::AuditFinding> {
            Vec::new()
        }
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let captured: std::sync::Arc<std::sync::Mutex<Option<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_clone = captured.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = match listener.accept() {
            Ok(x) => x,
            Err(_) => return,
        };
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .ok();
        let mut buf = vec![0u8; 1 << 16];
        let mut read = 0usize;
        loop {
            let n = match stream.read(&mut buf[read..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            read += n;

            let s = String::from_utf8_lossy(&buf[..read]).to_string();
            if let Some(header_end) = s.find("\r\n\r\n") {
                let cl: usize = s[..header_end]
                    .to_ascii_lowercase()
                    .split("\r\n")
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                if read >= header_end + 4 + cl {
                    break;
                }
            }
            if read == buf.len() {
                break;
            }
        }
        let body = String::from_utf8_lossy(&buf[..read]).to_string();
        *captured_clone.lock().unwrap() = Some(body);
        let resp = r#"{"id":"msg_test","role":"assistant","content":[{"type":"text","text":"ok"}],"model":"test-model","stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}"#;
        let http = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            resp.len(),
            resp
        );
        stream.write_all(http.as_bytes()).ok();
        stream.flush().ok();
    });

    let _g = crate::secret_guard::test_support::acquire();
    crate::secret_guard::set_context_transform(Some(std::sync::Arc::new(ReplaceSecret)));
    let messages = vec![Message {
        role: "user".to_string(),
        content: MessageContent::Text(format!("the key is {SECRET}")),
    }];
    let to_send = crate::secret_guard::transform_outgoing_messages(messages);
    let config = LlmClientConfig {
        api_key: "test-key".to_string(),
        base_url: format!("http://127.0.0.1:{port}"),
        model: "test-model".to_string(),
        provider: shannon_engine::api::LlmProvider::Anthropic,
        ..Default::default()
    };
    let client = LlmClient::new(config);
    let result = client.send_message(to_send, None, None).await;
    crate::secret_guard::set_context_transform(None);
    assert!(result.is_ok(), "mock must answer: {:?}", result.err());

    server.join().expect("server thread");
    let body = captured
        .lock()
        .unwrap()
        .clone()
        .expect("request body captured");
    assert!(
        body.contains(TOKEN),
        "surrogate must reach the wire: {body}"
    );
    assert!(
        !body.contains(SECRET),
        "raw secret must never reach the wire: {body}"
    );
}

// ---- T4 full loop: process_query drive against the local mock ----------
// The whole engine loop (message assembly → wire → response → events)
// with a transform installed: the mock must receive surrogates, never
// the raw secret, and the query must complete cleanly.

#[tokio::test]
async fn secret_guard_query_loop_completes_with_redacted_wire() {
    use futures::StreamExt as _;
    use std::io::Read as _;
    use std::io::Write as _;

    const SECRET: &str = "LOOP-SECRET-VALUE";
    const TOKEN: &str = "SG1:LOOPFAKELOOPFAKE";

    struct ReplaceSecret;
    impl shannon_plugin_api::ContextTransform for ReplaceSecret {
        fn transform_ingest(
            &self,
            block: &mut shannon_plugin_api::IngestBlock,
        ) -> shannon_plugin_api::TransformAction {
            if block.text.contains(SECRET) {
                block.text = block.text.replace(SECRET, TOKEN);
                shannon_plugin_api::TransformAction::Modified
            } else {
                shannon_plugin_api::TransformAction::Passthrough
            }
        }
        fn restore_tool_args(
            &self,
            _tool: &str,
            _args: &mut serde_json::Value,
        ) -> shannon_plugin_api::RestoreAction {
            shannon_plugin_api::RestoreAction::Unchanged
        }
        fn restore_display(&self, _text: &mut String) -> shannon_plugin_api::RestoreAction {
            shannon_plugin_api::RestoreAction::Unchanged
        }
        fn audit_wire(&self, _wire: &serde_json::Value) -> Vec<shannon_plugin_api::AuditFinding> {
            Vec::new()
        }
    }

    // Hermetic-ish run: the query's tee writes one session dir under the
    // real shannon home; it is removed after the assertions. Mutating the
    // process env here would race parallel tee tests that resolve their
    // own log paths from SHANNON_HOME.
    let session_for_cleanup = uuid::Uuid::new_v4();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let captured: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    // Detached: the accept loop lives until process exit; joining it
    // would block forever on the final accept().
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = vec![0u8; 1 << 16];
            let mut read = 0usize;
            loop {
                let n = match stream.read(&mut buf[read..]) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                read += n;
                let s = String::from_utf8_lossy(&buf[..read]).to_string();
                if let Some(header_end) = s.find("\r\n\r\n") {
                    let cl: usize = s[..header_end]
                        .to_ascii_lowercase()
                        .split("\r\n")
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    if read >= header_end + 4 + cl {
                        break;
                    }
                }
            }
            captured_clone
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..read]).to_string());
            // A8b: serve a proper terminal-framed SSE stream. The old
            // non-SSE JSON body produced a zero-event stream that ended
            // without any terminal frame and is now (correctly) typed as
            // an interrupted stream instead of silently completing.
            let resp = concat!(
                "event: message_start\n",
                r#"data: {"type":"message_start","message":{"id":"msg_loop","role":"assistant","content":[],"model":"test-model","stop_reason":null,"usage":{"input_tokens":2,"output_tokens":0}}}"#,
                "\n\n",
                "event: content_block_start\n",
                r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
                "\n\n",
                "event: content_block_delta\n",
                r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"done"}}"#,
                "\n\n",
                "event: content_block_stop\n",
                r#"data: {"type":"content_block_stop","index":0}"#,
                "\n\n",
                "event: message_delta\n",
                r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"input_tokens":2,"output_tokens":1}}"#,
                "\n\n",
                "event: message_stop\n",
                r#"data: {"type":"message_stop"}"#,
                "\n\n"
            );
            let http = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                resp.len(),
                resp
            );
            stream.write_all(http.as_bytes()).ok();
            stream.flush().ok();
        }
    });

    let _g = crate::secret_guard::test_support::acquire();
    crate::secret_guard::set_context_transform(Some(std::sync::Arc::new(ReplaceSecret)));
    let config = LlmClientConfig {
        api_key: "test-key".to_string(),
        base_url: format!("http://127.0.0.1:{port}"),
        model: "test-model".to_string(),
        provider: shannon_engine::api::LlmProvider::Anthropic,
        ..Default::default()
    };
    let client = LlmClient::new(config);
    let engine = QueryEngine::new(
        client,
        ToolRegistry::new(),
        PermissionManager::new(),
        StateManager::new(),
        QueryEngineConfig::default(),
    );
    let context = QueryContext {
        query_id: uuid::Uuid::new_v4(),
        session_id: session_for_cleanup,
        user_message: format!("the key is {SECRET}"),
        attachments: Vec::new(),
        metadata: QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: false,
            max_tokens: None,
            model: "test-model".to_string(),
            temperature: None,
            top_p: None,
        },
    };
    let mut stream = engine.process_query(context, None).await;
    let mut completed = false;
    let mut failed = String::new();
    while let Some(ev) = stream.next().await {
        match ev {
            Ok(QueryEvent::Completed { .. }) => {
                completed = true;
                break;
            }
            Ok(QueryEvent::Failed { error, .. }) => {
                failed = error;
                break;
            }
            Err(e) => {
                failed = e.to_string();
                break;
            }
            _ => {}
        }
    }
    crate::secret_guard::set_context_transform(None);

    assert!(completed, "query must complete; failed: {failed}");
    let bodies = captured.lock().unwrap().clone();
    assert!(
        !bodies.is_empty(),
        "at least one request must reach the mock"
    );
    assert!(
        bodies.iter().any(|b| b.contains(TOKEN)),
        "wire must carry surrogates: {bodies:?}"
    );
    assert!(
        bodies.iter().all(|b| !b.contains(SECRET)),
        "raw secret must never reach the wire: {bodies:?}"
    );
}

// ---- A8: turn-level continuation after timeout-class stream death -----
//
// The GLM coding-plan gateway hard-cuts single LLM calls at ~6min
// (smoke-2/4 RCA). The turn loop must continue THE TURN on a
// timeout-class failure — keeping all history and prior tool state —
// instead of failing the whole run. These tests drive the full
// process_query loop against a local mock Anthropic server whose
// per-request behavior is indexed by request count.

/// Serializes tests that mutate `SHANNON_TURN_RETRIES` (plain `cargo
/// test` runs them on shared threads; nextest isolates per process).
static TURN_RETRIES_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A local mock Anthropic server; request `i` gets whatever
/// `responder(i)` returns, and every raw request body is captured for
/// wire-level assertions (nudge present/absent, history shape).
struct TurnRetryMockServer {
    captured: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    base_url: String,
}

impl TurnRetryMockServer {
    fn start(responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync>) -> Self {
        use std::io::Read as _;
        use std::io::Write as _;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let captured: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        // Detached: the accept loop lives until process exit (same
        // contract as the T4 secret-guard loop mock above).
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = vec![0u8; 1 << 16];
                let mut read = 0usize;
                loop {
                    let n = match stream.read(&mut buf[read..]) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    read += n;
                    let s = String::from_utf8_lossy(&buf[..read]).to_string();
                    if let Some(header_end) = s.find("\r\n\r\n") {
                        let cl: usize = s[..header_end]
                            .to_ascii_lowercase()
                            .split("\r\n")
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0);
                        if read >= header_end + 4 + cl {
                            break;
                        }
                    }
                    if read == buf.len() {
                        break;
                    }
                }
                let body = String::from_utf8_lossy(&buf[..read]).to_string();
                let index = {
                    let mut guard = captured_clone.lock().unwrap();
                    guard.push(body);
                    guard.len() - 1
                };
                let http = responder(index);
                stream.write_all(http.as_bytes()).ok();
                stream.flush().ok();
            }
        });
        Self {
            captured,
            base_url: format!("http://127.0.0.1:{port}"),
        }
    }

    fn bodies(&self) -> Vec<String> {
        self.captured.lock().unwrap().clone()
    }
}

/// Raw HTTP/1.1 response with a `Connection: close` header.
fn a8_http_response(status_line: &str, content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Anthropic-style 408 whose body parses into `ApiError::ProviderError`
/// carrying the words "upstream request timeout". ProviderError is not
/// retried by the client's request-level retry nor reconnected by the
/// resumable stream, so it surfaces to the engine's turn loop
/// immediately (fast, deterministic) with exactly the string the
/// headless classifier maps to `Timeout`.
fn a8_timeout_response() -> String {
    a8_http_response(
        "408 Request Timeout",
        "application/json",
        r#"{"type":"error","error":{"type":"timeout_error","message":"upstream request timeout"}}"#,
    )
}

/// Anthropic-style 401 → `ApiError::AuthenticationFailed` — a
/// non-timeout class A8 must never continue on.
fn a8_auth_response() -> String {
    a8_http_response(
        "401 Unauthorized",
        "application/json",
        r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
    )
}

/// Full Anthropic SSE stream ending in a `tool_use` block for a tool
/// that is not in the (empty) test registry — the engine records an
/// error tool_result and advances to the next turn.
fn a8_tool_call_sse() -> String {
    let sse = [
        r#"event: message_start"#,
        r#"data: {"type":"message_start","message":{"id":"msg_a8_tool","role":"assistant","content":[],"model":"test-model","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#,
        r#"event: content_block_start"#,
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_a8_1","name":"no_such_tool","input":{}}}"#,
        r#"event: content_block_delta"#,
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{}"}}"#,
        r#"event: content_block_stop"#,
        r#"data: {"type":"content_block_stop","index":0}"#,
        r#"event: message_delta"#,
        r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":10,"output_tokens":5}}"#,
        r#"event: message_stop"#,
        r#"data: {"type":"message_stop"}"#,
    ]
    .join("\n\n");
    a8_http_response("200 OK", "text/event-stream", &sse)
}

/// Full Anthropic SSE stream with a plain text answer and `end_turn`.
fn a8_text_sse(text: &str) -> String {
    let payload = format!(r#"{{"type":"text_delta","text":"{text}"}}"#);
    let sse = [
        r#"event: message_start"#,
        r#"data: {"type":"message_start","message":{"id":"msg_a8_text","role":"assistant","content":[],"model":"test-model","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#,
        r#"event: content_block_start"#,
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        "event: content_block_delta",
        format!("data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{payload}}}").as_str(),
        r#"event: content_block_stop"#,
        r#"data: {"type":"content_block_stop","index":0}"#,
        r#"event: message_delta"#,
        r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"input_tokens":10,"output_tokens":7}}"#,
        r#"event: message_stop"#,
        r#"data: {"type":"message_stop"}"#,
    ]
    .join("\n\n");
    a8_http_response("200 OK", "text/event-stream", &sse)
}

/// Drive one full `process_query` against the mock (default turn budget)
/// and return (completed, failed_error, progress_messages,
/// warning_messages, final_history).
#[allow(clippy::type_complexity)]
async fn a8_run_query(
    server: &TurnRetryMockServer,
) -> (bool, String, Vec<String>, Vec<String>, Vec<Message>) {
    a8_run_query_with(server, 20).await
}

/// Variant with an explicit turn budget (A10 tests drive small budgets).
#[allow(clippy::type_complexity)]
async fn a8_run_query_with(
    server: &TurnRetryMockServer,
    max_turns: usize,
) -> (bool, String, Vec<String>, Vec<String>, Vec<Message>) {
    use futures::StreamExt as _;
    let config = LlmClientConfig {
        api_key: "test-key".to_string(),
        base_url: server.base_url.clone(),
        model: "test-model".to_string(),
        provider: shannon_engine::api::LlmProvider::Anthropic,
        ..Default::default()
    };
    let client = LlmClient::new(config);
    let engine = QueryEngine::new(
        client,
        ToolRegistry::new(),
        PermissionManager::new(),
        StateManager::new(),
        QueryEngineConfig {
            max_turns,
            ..Default::default()
        },
    );
    let context = QueryContext {
        query_id: uuid::Uuid::new_v4(),
        session_id: uuid::Uuid::new_v4(),
        user_message: "original user task".to_string(),
        attachments: Vec::new(),
        metadata: QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: false,
            max_tokens: None,
            model: "test-model".to_string(),
            temperature: None,
            top_p: None,
        },
    };
    let mut stream = engine.process_query(context, None).await;
    let mut completed = false;
    let mut failed = String::new();
    let mut progress: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut history: Vec<Message> = Vec::new();
    while let Some(ev) = stream.next().await {
        match ev {
            Ok(QueryEvent::Completed { .. }) => {
                completed = true;
                break;
            }
            Ok(QueryEvent::Failed { error, .. }) => {
                failed = error;
                break;
            }
            Ok(QueryEvent::Progress { message, .. }) => {
                progress.push(message);
            }
            Ok(QueryEvent::Warning { message, .. }) => {
                warnings.push(message);
            }
            Ok(QueryEvent::ConversationUpdate { messages, .. }) => {
                history = messages;
            }
            Err(e) => {
                failed = e.to_string();
                break;
            }
            _ => {}
        }
    }
    (completed, failed, progress, warnings, history)
}

/// Env contract: default 2; "0" legitimately disables; unparseable and
/// negative values fall back to the default (same contract as the
/// run-level `SHANNON_RUN_RETRIES` in the CLI).
#[test]
fn a8_turn_retries_env_parse_contract() {
    let _guard = TURN_RETRIES_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let saved = env::var("SHANNON_TURN_RETRIES").ok();

    unsafe { env::remove_var("SHANNON_TURN_RETRIES") };
    assert_eq!(
        recovery::turn_retries_max(),
        2,
        "unset must yield the default of 2"
    );

    for garbage in ["0", "abc", "-1", " 3 "] {
        unsafe { env::set_var("SHANNON_TURN_RETRIES", garbage) };
        let expected = garbage.trim().parse::<u32>().unwrap_or(2);
        assert_eq!(
            recovery::turn_retries_max(),
            expected,
            "SHANNON_TURN_RETRIES={garbage:?} must parse like SHANNON_RUN_RETRIES"
        );
    }

    match saved {
        Some(v) => unsafe { env::set_var("SHANNON_TURN_RETRIES", v) },
        None => unsafe { env::remove_var("SHANNON_TURN_RETRIES") },
    }
}

// ---- review §P2-2: hung tools are interrupted by the registry's
// execution timeout and surface as an error tool_result to the model ----

/// A registered tool that never finishes (60s sleep). The registry's
/// execution timeout (shortened for the test) must interrupt it and the
/// engine must record an error `ToolUseResult` instead of hanging.
struct HangingTool;

#[async_trait::async_trait]
impl crate::tools::Tool for HangingTool {
    fn name(&self) -> &str {
        "hanging_tool"
    }
    fn description(&self) -> &str {
        "A tool that never finishes (test double)"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
    ) -> crate::tools::ToolResult<crate::tools::ToolOutput> {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        Ok(crate::tools::ToolOutput::success("done".to_string()))
    }
}

#[tokio::test]
async fn p2_2_hanging_tool_interrupted_into_error_tool_result() {
    use futures::StreamExt as _;

    // Turn 1: a tool_use block for the hanging tool; turn 2: final text
    // so the query completes after the timed-out tool_result round-trips.
    let tool_call_sse = {
        let sse = [
            r#"event: message_start"#,
            r#"data: {"type":"message_start","message":{"id":"msg_p2_2_tool","role":"assistant","content":[],"model":"test-model","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#,
            r#"event: content_block_start"#,
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_p2_2","name":"hanging_tool","input":{}}}"#,
            r#"event: content_block_delta"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{}"}}"#,
            r#"event: content_block_stop"#,
            r#"data: {"type":"content_block_stop","index":0}"#,
            r#"event: message_delta"#,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":10,"output_tokens":5}}"#,
            r#"event: message_stop"#,
            r#"data: {"type":"message_stop"}"#,
        ]
        .join("\n\n");
        a8_http_response("200 OK", "text/event-stream", &sse)
    };
    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(move |i| match i {
            0 => tool_call_sse.clone(),
            _ => a8_text_sse("recovered after tool timeout"),
        });
    let server = TurnRetryMockServer::start(responder);

    let config = LlmClientConfig {
        api_key: "test-key".to_string(),
        base_url: server.base_url.clone(),
        model: "test-model".to_string(),
        provider: shannon_engine::api::LlmProvider::Anthropic,
        ..Default::default()
    };
    let client = LlmClient::new(config);

    // The short execution timeout stands in for the §P2-2 300s default
    // (structural default asserted in tools.rs); the wiring under test —
    // timeout fires → error tool_result → turn continues — is the same.
    let mut tools = ToolRegistry::new();
    tools.register(Box::new(HangingTool)).unwrap();
    tools.set_execution_timeout(std::time::Duration::from_millis(100));

    let mut permissions = PermissionManager::new();
    // Always-allow so the unattended test never stalls on an approval
    // prompt.
    permissions.allow_tool("hanging_tool");

    let engine = QueryEngine::new(
        client,
        tools,
        permissions,
        StateManager::new(),
        QueryEngineConfig {
            max_turns: 5,
            ..Default::default()
        },
    );
    let context = QueryContext {
        query_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        user_message: "call the hanging tool".to_string(),
        attachments: Vec::new(),
        metadata: QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: true,
            max_tokens: None,
            model: "test-model".to_string(),
            temperature: None,
            top_p: None,
        },
    };

    let mut stream = engine.process_query(context, None).await;
    let mut timeout_error_seen: Option<(bool, String)> = None;
    let mut completed = false;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    while let Some(ev) = tokio::time::timeout_at(deadline, stream.next())
        .await
        .unwrap_or(None)
    {
        match ev {
            Ok(QueryEvent::ToolUseResult {
                tool_name,
                result,
                is_error,
                ..
            }) if tool_name == "hanging_tool" => {
                timeout_error_seen = Some((is_error, result));
            }
            Ok(QueryEvent::Completed { .. }) => {
                completed = true;
                break;
            }
            Ok(QueryEvent::Failed { error, .. }) => {
                panic!("query must survive the tool timeout, got Failed: {error}");
            }
            Err(e) => panic!("query must survive the tool timeout, got error: {e}"),
            _ => {}
        }
    }

    let (is_error, result) =
        timeout_error_seen.expect("hanging_tool must produce a ToolUseResult event");
    assert!(
        is_error,
        "the timed-out tool result must be flagged as an error"
    );
    assert!(
        result.contains("timed out after"),
        "result must carry the timeout message, got: {result}"
    );
    assert!(
        completed,
        "the turn must continue after the interrupted tool and complete"
    );
}

/// A14: the helper computes the right Duration for every escalation
/// step. Index 0 (the original attempt before any continuation) yields
/// `None` — pre-A14 semantics. Index 1 = ×2, index 2 = ×3, both capped
/// at `STREAM_IDLE_ESCALATION_CAP_SECS`. A `None` base is also `None`
/// (watchdog was disabled → A14 does not turn it on).
#[test]
fn a14_stream_idle_escalation_math() {
    // base unset → never escalates.
    assert_eq!(recovery::stream_idle_escalated_budget(None, 0), None);
    assert_eq!(recovery::stream_idle_escalated_budget(None, 1), None);

    // retry 0 = original attempt = no override.
    assert_eq!(recovery::stream_idle_escalated_budget(Some(420), 0), None);

    // retry 1 = base × 2 = 840s.
    assert_eq!(
        recovery::stream_idle_escalated_budget(Some(420), 1),
        Some(std::time::Duration::from_secs(840))
    );

    // retry 2 = base × 3 = 1260s, capped at 1200s.
    assert_eq!(
        recovery::stream_idle_escalated_budget(Some(420), 2),
        Some(std::time::Duration::from_secs(1200))
    );

    // retry 10 would be huge but the cap holds.
    assert_eq!(
        recovery::stream_idle_escalated_budget(Some(420), 10),
        Some(std::time::Duration::from_secs(1200))
    );

    // base below the cap is preserved (no spurious uplift).
    assert_eq!(
        recovery::stream_idle_escalated_budget(Some(60), 1),
        Some(std::time::Duration::from_secs(120))
    );
}

/// Core A8 behavior: attempt 1 dies with a timeout-class error, the
/// turn is continued in place (attempt 2 returns a tool_call, attempt 3
/// the final answer). The query must COMPLETE with the full history
/// preserved, a visible Progress event, and the nudge present only on
/// retry requests.
#[tokio::test]
async fn a8_turn_retry_continues_after_timeout_class_stream_death() {
    let _guard = TURN_RETRIES_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let saved = env::var("SHANNON_TURN_RETRIES").ok();
    unsafe { env::set_var("SHANNON_TURN_RETRIES", "2") };

    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(|i: usize| match i {
            0 => a8_timeout_response(),
            1 => a8_tool_call_sse(),
            _ => a8_text_sse("final answer after continuation"),
        });
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, progress, _warnings, history) = a8_run_query(&server).await;

    match saved {
        Some(v) => unsafe { env::set_var("SHANNON_TURN_RETRIES", v) },
        None => unsafe { env::remove_var("SHANNON_TURN_RETRIES") },
    }

    assert!(
        completed,
        "the turn must complete after continuation; failed: {failed}"
    );
    let bodies = server.bodies();
    assert_eq!(
        bodies.len(),
        3,
        "attempt 1 + continued attempt 2 + post-tool turn 3; got {} requests",
        bodies.len()
    );

    // Nudge only on retry requests (attempts 2+), exactly once each;
    // the first attempt must be nudge-free.
    let nudge = recovery::TURN_CONTINUATION_NUDGE_PROMPT;
    assert!(
        !bodies[0].contains(nudge),
        "the first attempt must not carry the continuation nudge"
    );
    for (idx, body) in bodies.iter().enumerate().skip(1) {
        assert_eq!(
            body.matches(nudge).count(),
            1,
            "retry request {idx} must carry the nudge exactly once: {body}"
        );
    }

    // Progress visibility, formatted like the existing API-retry surfacing.
    assert!(
        progress
            .iter()
            .any(|m| m
                .contains("Turn LLM call interrupted (upstream cutoff); continuing turn 1/2")),
        "expected an A8 continuation Progress event; got: {progress:?}"
    );

    // History integrity: original task first, tool round-trip intact,
    // final assistant answer last.
    assert!(!history.is_empty(), "a ConversationUpdate must have fired");
    assert_eq!(history[0].role, "user");
    let first_text = match &history[0].content {
        MessageContent::Text(t) => t.clone(),
        other => panic!("expected text first message, got {other:?}"),
    };
    assert_eq!(first_text, "original user task");
    let has_tool_result = history.iter().any(|m| {
        matches!(
            &m.content,
            MessageContent::Blocks(blocks)
                if blocks.iter().any(|b| matches!(b, shannon_engine::api::ContentBlock::ToolResult { .. }))
        )
    });
    assert!(
        has_tool_result,
        "tool round-trip must be preserved: {history:?}"
    );
    let last = history.last().expect("non-empty history");
    assert_eq!(last.role, "assistant", "final message must be the answer");
    let last_text = match &last.content {
        MessageContent::Blocks(blocks) => blocks.iter().find_map(|b| match b {
            shannon_engine::api::ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        }),
        MessageContent::Text(t) => Some(t.clone()),
    }
    .unwrap_or_default();
    assert!(
        last_text.contains("final answer after continuation"),
        "final answer must land in history; got: {last_text:?}"
    );
}

/// Budget exhaustion: with SHANNON_TURN_RETRIES=2 and a server that
/// always times out, the engine makes 1 + 2 attempts, emits two
/// continuation Progress events (1/2, 2/2), then fails through the
/// EXISTING path with the original error text (so the headless A7
/// classifier still sees "timeout").
#[tokio::test]
async fn a8_turn_retry_budget_exhaustion_falls_through_to_failed() {
    let _guard = TURN_RETRIES_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let saved = env::var("SHANNON_TURN_RETRIES").ok();
    unsafe { env::set_var("SHANNON_TURN_RETRIES", "2") };

    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(|_i: usize| a8_timeout_response());
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, progress, _warnings, _history) = a8_run_query(&server).await;

    match saved {
        Some(v) => unsafe { env::set_var("SHANNON_TURN_RETRIES", v) },
        None => unsafe { env::remove_var("SHANNON_TURN_RETRIES") },
    }

    assert!(!completed, "an always-timeout server must not complete");
    assert!(
        failed.contains("upstream request timeout"),
        "the original error text must survive into Failed for A7 classification; got: {failed}"
    );
    let bodies = server.bodies();
    assert_eq!(
        bodies.len(),
        3,
        "initial attempt + 2 retries, then fail; got {}",
        bodies.len()
    );
    let nudge = recovery::TURN_CONTINUATION_NUDGE_PROMPT;
    assert!(!bodies[0].contains(nudge), "first attempt is nudge-free");
    assert_eq!(
        bodies[1].matches(nudge).count(),
        1,
        "retry 1 carries one nudge"
    );
    assert_eq!(
        bodies[2].matches(nudge).count(),
        1,
        "retry 2 carries one nudge — never accumulated"
    );
    let continuations = progress
        .iter()
        .filter(|m| m.contains("Turn LLM call interrupted (upstream cutoff)"))
        .count();
    assert_eq!(
        continuations, 2,
        "one Progress per continuation; got {progress:?}"
    );
}

/// Mid-stream death (the production shape of the GLM ~6min hard cut):
/// attempt 1 returns HTTP 200, streams message_start and a partial text
/// delta, then dies on an upstream timeout error frame. The A8 check
/// must precede the partial-content preservation in the mid-stream
/// error arm: the partial tail is discarded (a hard-cut tail is what
/// produced the truncated/empty patches in the RCA) and the turn is
/// retried to a clean completion.
#[tokio::test]
async fn a8_turn_retry_covers_mid_stream_death_and_discards_partial() {
    let _guard = TURN_RETRIES_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let saved = env::var("SHANNON_TURN_RETRIES").ok();
    unsafe { env::set_var("SHANNON_TURN_RETRIES", "2") };

    // Dead stream: valid frames, then an error frame our StreamEvent
    // model cannot parse. The parse failure surfaces as InvalidResponse
    // whose Display embeds the provider's "upstream request timeout"
    // wording — timeout-class by the pinned word list, and NOT
    // reconnectable, so it reaches the engine's mid-stream error arm
    // immediately (no reconnect backoff in the test).
    let dead_sse = [
        r#"event: message_start"#,
        r#"data: {"type":"message_start","message":{"id":"msg_dead","role":"assistant","content":[],"model":"test-model","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#,
        r#"event: content_block_start"#,
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        r#"event: content_block_delta"#,
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial answer before the cut"}}"#,
        r#"event: error"#,
        r#"data: {"type":"error","error":{"type":"timeout_error","message":"upstream request timeout"}}"#,
    ]
    .join("\n\n");
    let dead = a8_http_response("200 OK", "text/event-stream", &dead_sse);

    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(move |i: usize| match i {
            0 => dead.clone(),
            _ => a8_text_sse("recovered after mid-stream continuation"),
        });
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, progress, _warnings, history) = a8_run_query(&server).await;

    match saved {
        Some(v) => unsafe { env::set_var("SHANNON_TURN_RETRIES", v) },
        None => unsafe { env::remove_var("SHANNON_TURN_RETRIES") },
    }

    assert!(
        completed,
        "the turn must complete after a mid-stream continuation; failed: {failed}"
    );
    let bodies = server.bodies();
    assert_eq!(bodies.len(), 2, "dead attempt + one continuation");
    let nudge = recovery::TURN_CONTINUATION_NUDGE_PROMPT;
    assert!(!bodies[0].contains(nudge), "first attempt is nudge-free");
    assert_eq!(bodies[1].matches(nudge).count(), 1);
    assert!(
        progress.iter().any(|m| m.contains("continuing turn 1/2")
            && (m.contains("Turn LLM call interrupted (upstream cutoff)")
                // N-3: provider-reported error events use their own
                // Progress wording but the same continuation ladder.
                || m.contains("Provider stream error (upstream)"))),
        "expected A8 Progress; got {progress:?}"
    );
    let history_text: String = history
        .iter()
        .map(|m| match &m.content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .map(|b| match b {
                    shannon_engine::api::ContentBlock::Text { text } => text.clone(),
                    _ => String::new(),
                })
                .collect(),
        })
        .collect();
    assert!(
        !history_text.contains("partial answer before the cut"),
        "the hard-cut partial tail must NOT be committed as a complete response: {history_text:?}"
    );
    assert!(
        history_text.contains("recovered after mid-stream continuation"),
        "the continuation's answer must land in history: {history_text:?}"
    );
}

/// Truncated stream (A8b / smoke-5 shape): valid partial frames, then
/// the connection ends WITHOUT any terminal frame (no message_delta
/// stop reason, no message_stop) — an abnormal EOF, never a completion.
fn a8_truncated_sse() -> String {
    let sse = [
        r#"event: message_start"#,
        r#"data: {"type":"message_start","message":{"id":"msg_dead","role":"assistant","content":[],"model":"test-model","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#,
        r#"event: content_block_start"#,
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        r#"event: content_block_delta"#,
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial answer before the cut"}}"#,
    ]
    .join("\n\n");
    a8_http_response("200 OK", "text/event-stream", &sse)
}

/// A8b core behavior (smoke-5): a mid-stream abnormal EOF
/// (`ApiError::StreamEndedUnexpectedly`) must continue THE TURN — the
/// truncated partial is discarded as an unusable tail, the continuation
/// nudge is injected, and the retried stream completes the turn.
/// Without A8b this query "completes" with the partial saved as a final
/// answer (rc=0, empty patch — exactly smoke-5). The engine's default
/// config sends structured system blocks, so the stream is the plain
/// (non-reconnecting) SseStream: the typed EOF surfaces on the first
/// dead response and the retry is immediate.
#[tokio::test]
async fn a8_stream_interrupted_triggers_continuation_and_discards_partial() {
    let _guard = TURN_RETRIES_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let saved = env::var("SHANNON_TURN_RETRIES").ok();
    unsafe { env::set_var("SHANNON_TURN_RETRIES", "2") };

    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(|i: usize| match i {
            0 => a8_truncated_sse(),
            _ => a8_text_sse("recovered after interrupted continuation"),
        });
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, progress, warnings, history) = a8_run_query(&server).await;

    match saved {
        Some(v) => unsafe { env::set_var("SHANNON_TURN_RETRIES", v) },
        None => unsafe { env::remove_var("SHANNON_TURN_RETRIES") },
    }

    assert!(
        completed,
        "the turn must complete after the interrupted-stream continuation; failed: {failed}"
    );
    let bodies = server.bodies();
    assert_eq!(
        bodies.len(),
        2,
        "dead attempt + 1 A8 continuation; got {}",
        bodies.len()
    );
    let nudge = recovery::TURN_CONTINUATION_NUDGE_PROMPT;
    assert!(
        !bodies[0].contains(nudge),
        "the first attempt must be nudge-free"
    );
    assert_eq!(
        bodies[1].matches(nudge).count(),
        1,
        "the A8 continuation request must carry the nudge exactly once"
    );
    assert!(
        progress
            .iter()
            .any(|m| m
                .contains("Turn LLM call interrupted (upstream cutoff); continuing turn 1/2")),
        "expected A8 Progress for the interrupted stream; got {progress:?}"
    );
    assert!(
        warnings.is_empty(),
        "no partial-preserve Warning may fire when the turn was continued: {warnings:?}"
    );
    let history_text: String = history
        .iter()
        .map(|m| match &m.content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .map(|b| match b {
                    shannon_engine::api::ContentBlock::Text { text } => text.clone(),
                    _ => String::new(),
                })
                .collect(),
        })
        .collect();
    assert!(
        !history_text.contains("partial answer before the cut"),
        "the truncated tail must NOT be committed as a complete response: {history_text:?}"
    );
    assert!(
        history_text.contains("recovered after interrupted continuation"),
        "the continuation's answer must land in history: {history_text:?}"
    );
}

/// A8b budget exhausted / disabled: the abnormal EOF falls through to
/// the EXISTING has_partial path — partial preserved, Warning fired,
/// query Completed — so the degraded behavior is byte-identical to
/// pre-A8b when SHANNON_TURN_RETRIES=0.
#[tokio::test]
async fn a8_stream_interrupted_budget_exhausted_falls_back_to_partial_preserve() {
    let _guard = TURN_RETRIES_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let saved = env::var("SHANNON_TURN_RETRIES").ok();
    unsafe { env::set_var("SHANNON_TURN_RETRIES", "0") };

    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(|_i: usize| a8_truncated_sse());
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, progress, warnings, history) = a8_run_query(&server).await;

    match saved {
        Some(v) => unsafe { env::set_var("SHANNON_TURN_RETRIES", v) },
        None => unsafe { env::remove_var("SHANNON_TURN_RETRIES") },
    }

    assert!(
        completed,
        "the preserve fallback must complete the query as before; failed: {failed}"
    );
    assert_eq!(
        server.bodies().len(),
        1,
        "SHANNON_TURN_RETRIES=0: single dead attempt, no continuation"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("Partial response preserved")),
        "the existing partial-preserve Warning must fire; got {warnings:?}"
    );
    assert!(
        !progress
            .iter()
            .any(|m| m.contains("Turn LLM call interrupted (upstream cutoff)")),
        "no A8 continuation may fire when disabled; got {progress:?}"
    );
    let history_text: String = history
        .iter()
        .map(|m| match &m.content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .map(|b| match b {
                    shannon_engine::api::ContentBlock::Text { text } => text.clone(),
                    _ => String::new(),
                })
                .collect(),
        })
        .collect();
    assert!(
        history_text.contains("partial answer before the cut"),
        "with the budget exhausted the partial must be preserved exactly as before: {history_text:?}"
    );
}

/// A13 truncated-with-tool-call stream (path 1): partial text, then a
/// tool_use whose JSON was cut by the output limit (ContentBlockStop
/// fires on the truncated frame and pairs a synthetic "Malformed tool
/// input" result with a null-input ToolUse block), then
/// message_delta(stop_reason "length"). This is the exact minimax-M3
/// orphan shape: the truncation continuation must not strand the
/// tool_result after itself.
fn a13_truncated_text_plus_tool_sse(zero_usage: bool) -> String {
    a13_truncated_tool_sse_impl(zero_usage, true)
}

/// Variant without the tool block's ContentBlockStop: the partial JSON
/// never parses at block scope and fails again in the MessageDelta
/// post-stream flush (A13-c site 2).
fn a13_truncated_unclosed_tool_sse(zero_usage: bool) -> String {
    a13_truncated_tool_sse_impl(zero_usage, false)
}

fn a13_truncated_tool_sse_impl(zero_usage: bool, close_tool_block: bool) -> String {
    let usage = if zero_usage {
        r#"{"input_tokens":0,"output_tokens":0}"#
    } else {
        r#"{"input_tokens":10,"output_tokens":9}"#
    };
    let stop_frame = if zero_usage {
        // Sentinel zero-usage frame defers finalization (MiniMax splits
        // usage across frames); no message_stop follows — the stream is
        // cut, so the safety net finalizes instead (path 2).
        format!(
            "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"length\"}},\"usage\":{usage}}}\n\n"
        )
    } else {
        format!(
            "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"length\"}},\"usage\":{usage}}}\n\nevent: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
        )
    };
    let sse = [
        r#"event: message_start"#,
        r#"data: {"type":"message_start","message":{"id":"msg_trunc","role":"assistant","content":[],"model":"test-model","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#,
        r#"event: content_block_start"#,
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        r#"event: content_block_delta"#,
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial text before the cut"}}"#,
        r#"event: content_block_stop"#,
        r#"data: {"type":"content_block_stop","index":0}"#,
        r#"event: content_block_start"#,
        r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_trunc_1","name":"no_such_tool","input":{}}}"#,
        r#"event: content_block_delta"#,
        r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"na"}}"#,
    ]
    .join("\n\n");
    let tool_stop = if close_tool_block {
        "\n\nevent: content_block_stop\n\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n"
    } else {
        ""
    };
    let sse = format!("{sse}{tool_stop}");
    // Splice the stop frame (keeps the usage variants in one place).
    let sse = format!("{sse}\n\n{stop_frame}");
    a8_http_response("200 OK", "text/event-stream", &sse)
}

#[tokio::test]
async fn a13_truncation_with_tool_use_flushes_results_before_continuation() {
    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(|i: usize| match i {
            0 => a13_truncated_text_plus_tool_sse(false),
            _ => a8_text_sse("wrapped up after continuation"),
        });
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, _progress, _warnings, _history) = a8_run_query(&server).await;

    assert!(
        completed,
        "the continuation turn must complete; failed: {failed}"
    );
    let bodies = server.bodies();
    assert_eq!(bodies.len(), 2, "truncated turn + continuation turn");
    assert!(
        !bodies[0].contains(TRUNCATION_CONTINUATION_PROMPT),
        "the truncated request itself carries no continuation prompt"
    );
    let body = &bodies[1];
    assert!(
        body.contains(TRUNCATION_CONTINUATION_PROMPT),
        "the continuation request must carry the continuation prompt"
    );
    // Wire order: the assistant's tool_call declaration first, then its
    // tool_result, then the continuation prompt. The id appears exactly
    // twice (declaration + result); anything else is an orphan.
    let first = body
        .find("toolu_trunc_1")
        .expect("tool call id must reach the wire");
    let second = body
        .rfind("toolu_trunc_1")
        .expect("tool result id must reach the wire");
    let cont = body
        .find(TRUNCATION_CONTINUATION_PROMPT)
        .expect("continuation prompt must reach the wire");
    assert_ne!(
        first, second,
        "the id must appear as BOTH a tool_call declaration and a tool_result"
    );
    assert!(
        first < second && second < cont,
        "wire order must be assistant(tool_call) → user(tool_result) → \
         user(continuation); got call@{first}, result@{second}, cont@{cont}"
    );
    // A13-c: minimax parses tool_calls arguments and requires a JSON
    // object — "null" is rejected 400 (2013) while "{}" succeeds. The
    // malformed synthesis must therefore serialize as an empty object.
    assert!(
        body.contains("\"input\":{}"),
        "the synthesized tool_use input must be {{}} on the wire"
    );
    assert!(
        !body.contains("\"input\":null"),
        "a null tool_use input is wire-illegal for minimax (2013)"
    );
}

/// A13 path 2 (safety net): the same truncated text+tool stream, but the
/// only message_delta carries the sentinel zero-usage frame (MiniMax
/// splits usage across frames) and the stream is cut before message_stop.
/// The safety net finalizes; the same wire-order contract applies.
#[tokio::test]
async fn a13_safety_net_truncation_with_tool_use_flushes_results_before_continuation() {
    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(|i: usize| match i {
            0 => a13_truncated_text_plus_tool_sse(true),
            _ => a8_text_sse("wrapped up after continuation"),
        });
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, _progress, _warnings, _history) = a8_run_query(&server).await;

    assert!(
        completed,
        "the continuation turn must complete; failed: {failed}"
    );
    let bodies = server.bodies();
    assert_eq!(bodies.len(), 2);
    let body = &bodies[1];
    assert!(body.contains(TRUNCATION_CONTINUATION_PROMPT));
    let first = body
        .find("toolu_trunc_1")
        .expect("tool call id must reach the wire");
    let second = body
        .rfind("toolu_trunc_1")
        .expect("tool result id must reach the wire");
    let cont = body
        .find(TRUNCATION_CONTINUATION_PROMPT)
        .expect("continuation prompt must reach the wire");
    assert_ne!(first, second);
    assert!(
        first < second && second < cont,
        "safety-net wire order must be assistant(tool_call) → user(tool_result) → \
         user(continuation); got call@{first}, result@{second}, cont@{cont}"
    );
}

/// A13-c site 2: the tool_use frame is cut WITHOUT a ContentBlockStop —
/// the partial JSON fails again in the MessageDelta post-stream flush.
/// The synthetic result must be PAIRED with an assistant ToolUse block
/// carrying an empty-object input (previously no block was pushed at
/// all, so the flushed result reached the wire orphaned).
#[tokio::test]
async fn a13_post_stream_flush_malformed_call_pairs_with_empty_object() {
    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(|i: usize| match i {
            0 => a13_truncated_unclosed_tool_sse(false),
            _ => a8_text_sse("wrapped up after continuation"),
        });
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, _progress, _warnings, _history) = a8_run_query(&server).await;

    assert!(
        completed,
        "the continuation turn must complete; failed: {failed}"
    );
    let bodies = server.bodies();
    assert_eq!(bodies.len(), 2);
    let body = &bodies[1];
    assert!(body.contains(TRUNCATION_CONTINUATION_PROMPT));
    let first = body
        .find("toolu_trunc_1")
        .expect("tool call id must reach the wire");
    let second = body
        .rfind("toolu_trunc_1")
        .expect("tool result id must reach the wire");
    let cont = body
        .find(TRUNCATION_CONTINUATION_PROMPT)
        .expect("continuation prompt must reach the wire");
    assert_ne!(
        first, second,
        "the id must appear as BOTH a tool_call declaration and a tool_result"
    );
    assert!(
        first < second && second < cont,
        "wire order must be assistant(tool_call) → user(tool_result) → \
         user(continuation); got call@{first}, result@{second}, cont@{cont}"
    );
    assert!(
        body.contains("\"input\":{}") && !body.contains("\"input\":null"),
        "the paired tool_call input must be an empty object (A13-c)"
    );
}

/// Regression: a PURE-TEXT truncation keeps its existing shape — the
/// partial text lands before the continuation prompt, no tool traffic
/// is introduced, and the query completes.
#[tokio::test]
async fn a13_truncated_text_only_path_unchanged() {
    let sse = [
        r#"event: message_start"#,
        r#"data: {"type":"message_start","message":{"id":"msg_ttext","role":"assistant","content":[],"model":"test-model","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#,
        r#"event: content_block_start"#,
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        r#"event: content_block_delta"#,
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial reasoning was cut"}}"#,
        r#"event: content_block_stop"#,
        r#"data: {"type":"content_block_stop","index":0}"#,
        r#"event: message_delta"#,
        r#"data: {"type":"message_delta","delta":{"stop_reason":"length"},"usage":{"input_tokens":10,"output_tokens":9}}"#,
        r#"event: message_stop"#,
        r#"data: {"type":"message_stop"}"#,
    ]
    .join("\n\n");
    let truncated = a8_http_response("200 OK", "text/event-stream", &sse);
    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(move |i: usize| match i {
            0 => truncated.clone(),
            _ => a8_text_sse("final answer"),
        });
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, _progress, _warnings, history) = a8_run_query(&server).await;

    assert!(
        completed,
        "text-only truncation must complete; failed: {failed}"
    );
    let bodies = server.bodies();
    assert_eq!(bodies.len(), 2);
    let body = &bodies[1];
    assert!(body.contains(TRUNCATION_CONTINUATION_PROMPT));
    assert!(
        body.contains("partial reasoning was cut"),
        "the partial text must be preserved"
    );
    assert!(
        body.find("partial reasoning was cut").unwrap()
            < body.find(TRUNCATION_CONTINUATION_PROMPT).unwrap(),
        "partial text must precede the continuation prompt"
    );
    assert!(
        !body.contains("toolu_"),
        "text-only truncation must not introduce tool traffic"
    );
    let _ = history;
}

/// Boundary guard for the terminal-frame latch: a stream that ends with
/// a `message_delta` carrying a stop reason but NO `message_stop` (the
/// only terminal signal several providers emit — Ollama's done chunk,
/// Gemini's final chunk) is a CLEAN completion. It must finish in one
/// request with no reconnects and no A8 continuation.
#[tokio::test]
async fn a8_clean_end_without_message_stop_is_not_continued() {
    let _guard = TURN_RETRIES_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let saved = env::var("SHANNON_TURN_RETRIES").ok();
    unsafe { env::set_var("SHANNON_TURN_RETRIES", "2") };

    let sse = [
        r#"event: message_start"#,
        r#"data: {"type":"message_start","message":{"id":"msg_clean","role":"assistant","content":[],"model":"test-model","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#,
        r#"event: content_block_start"#,
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        r#"event: content_block_delta"#,
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"clean end without message_stop"}}"#,
        r#"event: content_block_stop"#,
        r#"data: {"type":"content_block_stop","index":0}"#,
        r#"event: message_delta"#,
        r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"input_tokens":10,"output_tokens":7}}"#,
    ]
    .join("\n\n");
    let body = a8_http_response("200 OK", "text/event-stream", &sse);
    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(move |_i: usize| body.clone());
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, progress, warnings, history) = a8_run_query(&server).await;

    match saved {
        Some(v) => unsafe { env::set_var("SHANNON_TURN_RETRIES", v) },
        None => unsafe { env::remove_var("SHANNON_TURN_RETRIES") },
    }

    assert!(
        completed,
        "a clean text completion must complete; failed: {failed}"
    );
    assert_eq!(
        server.bodies().len(),
        1,
        "a terminal-frame stream must not be reconnected or continued"
    );
    assert!(
        !progress
            .iter()
            .any(|m| m.contains("Turn LLM call interrupted (upstream cutoff)")),
        "no A8 continuation for a clean completion; got {progress:?}"
    );
    assert!(
        warnings.is_empty(),
        "no warnings expected for a clean completion: {warnings:?}"
    );
    let history_text: String = history
        .iter()
        .map(|m| match &m.content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .map(|b| match b {
                    shannon_engine::api::ContentBlock::Text { text } => text.clone(),
                    _ => String::new(),
                })
                .collect(),
        })
        .collect();
    assert!(
        history_text.contains("clean end without message_stop"),
        "the answer must land in history exactly once: {history_text:?}"
    );
    assert_eq!(
        history_text
            .matches("clean end without message_stop")
            .count(),
        1,
        "content must not be duplicated by reconnect replays"
    );
}

// ---- A10: wrap-up protocol before the final turn ----------------------
//
// w4: arcane / dynamodb died at the turn limit with work done but
// nothing committed (exit 2, empty patch, F10). The nudge entering the
// final turn lands the work; the hard stop keeps its semantics.

/// ① The wrap-up nudge is injected exactly once, and only in the wire
/// body of the request that OPENS the final turn. ② After the budget
/// is exhausted the query still completes through the unchanged hard
/// stop (Completed, no Failed).
#[tokio::test]
async fn a10_wrap_up_nudge_injected_once_before_final_turn() {
    // Every turn ends in a tool_use (the empty registry answers with an
    // error tool_result), so no response ever ends the query early and
    // the budget genuinely exhausts through the hard stop.
    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(|_i: usize| a8_tool_call_sse());
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, progress, warnings, _history) = a8_run_query_with(&server, 3).await;

    assert!(
        completed,
        "budget exhaustion must still complete via the hard stop; failed: {failed}"
    );
    assert!(failed.is_empty());
    let bodies = server.bodies();
    assert_eq!(
        bodies.len(),
        3,
        "exactly max_turns requests must run; got {}",
        bodies.len()
    );
    let wrap = WRAP_UP_NUDGE_PROMPT;
    assert!(!bodies[0].contains(wrap), "turn 1 of 3 must be nudge-free");
    assert!(!bodies[1].contains(wrap), "turn 2 of 3 must be nudge-free");
    assert_eq!(
        bodies[2].matches(wrap).count(),
        1,
        "the final-turn request must carry the wrap-up nudge exactly once"
    );
    assert!(
        progress
            .iter()
            .any(|m| m.contains("turn budget") && m.contains("final turn")),
        "a wrap-up Progress event must fire; got {progress:?}"
    );
    assert_eq!(
        progress
            .iter()
            .filter(|m| m.contains("turn budget") && m.contains("final turn"))
            .count(),
        1,
        "the wrap-up Progress event is one-shot; got {progress:?}"
    );
    // Note: tool-only mock turns legitimately emit the pre-existing
    // "Model produced no text output" Warning — not an A10 concern.
    let _ = warnings;
}

/// ③ With max_turns = 1 the very first request IS the final turn and
/// must already carry the nudge.
#[tokio::test]
async fn a10_wrap_up_nudge_fires_on_first_turn_when_max_turns_is_one() {
    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(|_i: usize| a8_tool_call_sse());
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, progress, _warnings, _history) = a8_run_query_with(&server, 1).await;

    assert!(completed, "single-turn run must complete; failed: {failed}");
    let bodies = server.bodies();
    assert_eq!(bodies.len(), 1, "exactly one request must run");
    assert_eq!(
        bodies[0].matches(WRAP_UP_NUDGE_PROMPT).count(),
        1,
        "with max_turns=1 the first request is the final turn"
    );
    assert!(
        progress
            .iter()
            .any(|m| m.contains("turn budget") && m.contains("final turn")),
        "wrap-up Progress must fire; got {progress:?}"
    );
}

/// ④ A8 stacking: the wrap-up turn's stream dies with a timeout-class
/// error and the A8 continuation re-enters the SAME turn (turn count
/// unchanged, never越过 max_turns). The wrap-up nudge must NOT be
/// re-injected on the retry request, and the A8 nudge rides after it.
#[tokio::test]
async fn a10_wrap_up_and_a8_stacking_no_double_injection() {
    let _guard = TURN_RETRIES_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let saved = env::var("SHANNON_TURN_RETRIES").ok();
    unsafe { env::set_var("SHANNON_TURN_RETRIES", "2") };

    // turn 0: dead → A8 retry succeeds (tool turn). turn 1 (final):
    // dead → A8 retry succeeds → budget exhausted at max_turns=2 → hard
    // stop. Recoveries are tool turns so no response ends the query.
    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(|i: usize| match i {
            0 | 2 => a8_timeout_response(),
            _ => a8_tool_call_sse(),
        });
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, progress, warnings, _history) = a8_run_query_with(&server, 2).await;

    match saved {
        Some(v) => unsafe { env::set_var("SHANNON_TURN_RETRIES", v) },
        None => unsafe { env::remove_var("SHANNON_TURN_RETRIES") },
    }

    assert!(completed, "both turns must land; failed: {failed}");
    let bodies = server.bodies();
    assert_eq!(
        bodies.len(),
        4,
        "2 turns × (dead attempt + A8 retry); got {}",
        bodies.len()
    );
    let wrap = WRAP_UP_NUDGE_PROMPT;
    let a8nudge = recovery::TURN_CONTINUATION_NUDGE_PROMPT;
    assert!(!bodies[0].contains(wrap) && !bodies[0].contains(a8nudge));
    assert!(
        !bodies[1].contains(wrap) && bodies[1].matches(a8nudge).count() == 1,
        "turn-0 retry carries only the A8 nudge (wrap-up not yet due)"
    );
    // A8/A1 nudges are session-persistent by design, so turn 1's
    // requests legitimately still carry turn 0's A8 nudge. The A10
    // contract: the wrap-up nudge appears EXACTLY once per request —
    // the in-turn A8 re-entry must not stack a second copy.
    assert!(
        bodies[2].matches(wrap).count() == 1 && bodies[2].matches(a8nudge).count() == 1,
        "the final-turn request carries one wrap-up nudge (plus turn 0's persistent A8 nudge)"
    );
    assert!(
        bodies[3].matches(wrap).count() == 1 && bodies[3].matches(a8nudge).count() == 2,
        "the in-turn A8 retry must NOT re-inject the wrap-up nudge; the A8 nudge \
         grows by exactly the one new continuation"
    );
    let continuations = progress
        .iter()
        .filter(|m| m.contains("Turn LLM call interrupted (upstream cutoff)"))
        .count();
    assert_eq!(continuations, 2, "one A8 continuation per turn");
    assert_eq!(
        progress
            .iter()
            .filter(|m| m.contains("turn budget") && m.contains("final turn"))
            .count(),
        1,
        "wrap-up Progress stays one-shot across A8 re-entries"
    );
    let _ = warnings;
}

/// SHANNON_TURN_RETRIES=0 disables continuation entirely: one request,
/// immediate failure through the existing path.
#[tokio::test]
async fn a8_turn_retry_zero_disables_continuation() {
    let _guard = TURN_RETRIES_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let saved = env::var("SHANNON_TURN_RETRIES").ok();
    unsafe { env::set_var("SHANNON_TURN_RETRIES", "0") };

    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(|_i: usize| a8_timeout_response());
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, progress, _warnings, _history) = a8_run_query(&server).await;

    match saved {
        Some(v) => unsafe { env::set_var("SHANNON_TURN_RETRIES", v) },
        None => unsafe { env::remove_var("SHANNON_TURN_RETRIES") },
    }

    assert!(!completed, "disabled continuation must fail");
    assert!(
        failed.contains("upstream request timeout"),
        "existing failure path must be preserved: {failed}"
    );
    assert_eq!(
        server.bodies().len(),
        1,
        "SHANNON_TURN_RETRIES=0 must not re-send the turn"
    );
    assert!(
        !progress
            .iter()
            .any(|m| m.contains("Turn LLM call interrupted (upstream cutoff)")),
        "no continuation Progress may fire when disabled; got {progress:?}"
    );
}

/// Non-timeout errors (auth failure) are never continued: A8 must not
/// swallow deterministic failures.
#[tokio::test]
async fn a8_non_timeout_errors_do_not_continue_turn() {
    let _guard = TURN_RETRIES_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let saved = env::var("SHANNON_TURN_RETRIES").ok();
    unsafe { env::set_var("SHANNON_TURN_RETRIES", "2") };

    let responder: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync> =
        std::sync::Arc::new(|_i: usize| a8_auth_response());
    let server = TurnRetryMockServer::start(responder);

    let (completed, failed, progress, _warnings, _history) = a8_run_query(&server).await;

    match saved {
        Some(v) => unsafe { env::set_var("SHANNON_TURN_RETRIES", v) },
        None => unsafe { env::remove_var("SHANNON_TURN_RETRIES") },
    }

    assert!(!completed, "auth failure must not complete");
    assert!(!failed.is_empty(), "auth failure must surface Failed");
    assert_eq!(
        server.bodies().len(),
        1,
        "an AuthenticationFailed must not be continued"
    );
    assert!(
        !progress
            .iter()
            .any(|m| m.contains("Turn LLM call interrupted (upstream cutoff)")),
        "no A8 Progress for non-timeout errors; got {progress:?}"
    );
}
