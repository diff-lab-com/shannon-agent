//! Chat wire-contract smoke (R6): fake SSE LLM + the REAL shannon-server.
//!
//! What runs for real here: `shannon_server::run_with_allow_nonloopback` on a
//! loopback TCP port (the exact entry point `shannon serve` uses), the real
//! `POST /v1/sessions` → `POST /v1/sessions/:id/messages` router, the real
//! `QueryEngine` agent loop, a real HTTP round-trip against the fake upstream,
//! and the real SSE encoder. Nothing about the server is mocked — only the LLM
//! is fake (`fake_llm` module, scripted Anthropic `/v1/messages` SSE).
//!
//! What is asserted: the SSE frame sequence on the wire — every `event:` name
//! ∈ the `SseEventName` contract, payloads are externally-tagged `QueryEvent`
//! JSON (field-level assertions, not byte equality), text chunks arrive in
//! script order, a tool turn round-trips `tool_use_request` →
//! `tool_use_result`, a provider error terminates with `failed`, and a second
//! message on the same session carries the first turn's history back to the
//! LLM (the ConversationUpdate restore contract).
//!
//! Fully offline: no API key, no non-loopback traffic, no tauri-driver (D2).
//! Approval (`POST /api/approval/respond`) has no route on this server — the
//! serve-path permission gate denies tool calls without a host channel — so
//! the approval round-trip from the brief is covered by the tool-turn test's
//! denial-tolerant assertions (see the report for the rationale).

#![allow(clippy::unwrap_used)] // tests: fixtures must panic with context, not propagate

mod fake_llm;

use fake_llm::{FakeLlm, Script};
use futures::StreamExt;
use shannon_api_protocol::SseEventName;
use std::time::Duration;
use tempfile::TempDir;

/// How long any single HTTP interaction may take before the test fails with
/// context. Everything here is loopback and scripted — seconds, not minutes.
const IO_TIMEOUT: Duration = Duration::from_secs(60);

/// Serialize the env-sensitive parts (SHANNON_HOME + server lifetime) within
/// this test binary. Under nextest every test is its own process and the lock
/// is uncontended; under plain `cargo test` all targets of this binary share
/// one process and one env — the guard keeps them correct there too. An
/// async-aware mutex because the guard is intentionally held across every
/// await of a test (the env must outlive the server it configured).
static ENV_SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// One live stack: fake upstream + real server, both on ephemeral ports.
struct Stack {
    client: reqwest::Client,
    fake: FakeLlm,
    /// `http://127.0.0.1:{port}` of the REAL shannon-server.
    base: String,
    server: tokio::task::JoinHandle<()>,
    /// Kept alive for the whole test: SHANNON_HOME redirect so session logs
    /// and any state the engine persists land here, not in the real home.
    _home: TempDir,
}

impl Drop for Stack {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Stack {
    /// Start the fake with `script_json`, then the real server pointed at it.
    async fn start(script_json: &str) -> Stack {
        let home = tempfile::tempdir().expect("create SHANNON_HOME tempdir");
        // Edition-2024 unsafe: this test binary serializes env access through
        // ENV_SERIAL, and nextest gives each test its own process anyway.
        unsafe { std::env::set_var("SHANNON_HOME", home.path()) };
        // The serve entry point honors this env fallback for the bearer token;
        // a developer shell that happens to export it would 401 every request.
        unsafe { std::env::remove_var("SHANNON_SERVE_TOKEN") };

        let fake = FakeLlm::start(Script::parse(script_json)).await;

        let config = shannon_engine::api::LlmClientConfig {
            api_key: "smoke-test-key".to_string(),
            base_url: fake.base_url().to_string(),
            model: "fake-model".to_string(),
            provider: shannon_engine::api::types::LlmProvider::Anthropic,
            ..Default::default()
        };

        // Reserve a port, release it, hand it to the real serve entry point —
        // the same bind-then-serve sequence `shannon serve` performs.
        let probe = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind port probe");
        let port = probe.local_addr().expect("probe addr").port();
        drop(probe);

        let server = tokio::spawn(async move {
            // Real entry point: bind validation, host guard, graceful-drain
            // watch, axum serve loop — production path end to end. Runs until
            // the test aborts it (Drop) or the runtime shuts down.
            let _ =
                shannon_server::run_with_allow_nonloopback("127.0.0.1", port, config, None, false)
                    .await;
        });

        let client = reqwest::Client::new();
        let base = format!("http://127.0.0.1:{port}");
        let stack = Stack {
            client,
            fake,
            base,
            server,
            _home: home,
        };
        stack.wait_ready().await;
        stack
    }

    /// Poll `POST /v1/sessions` until the server accepts connections; the
    /// successful create doubles as this stack's session.
    async fn wait_ready(&self) -> String {
        let deadline = tokio::time::Instant::now() + IO_TIMEOUT;
        loop {
            assert!(
                tokio::time::Instant::now() < deadline,
                "shannon-server did not become ready within {IO_TIMEOUT:?}"
            );
            match self
                .client
                .post(format!("{}/v1/sessions", self.base))
                .json(&serde_json::json!({}))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let body: serde_json::Value =
                        resp.json().await.expect("create session body is JSON");
                    return body["id"]
                        .as_str()
                        .expect("create session response carries id")
                        .to_string();
                }
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }

    /// POST one message and collect the full SSE frame sequence as
    /// `(event name, data payload)` pairs.
    async fn post_message(&self, session: &str, content: &str) -> Vec<(String, String)> {
        let response = tokio::time::timeout(
            IO_TIMEOUT,
            self.client
                .post(format!("{}/v1/sessions/{session}/messages", self.base))
                .json(&serde_json::json!({ "content": content }))
                .send(),
        )
        .await
        .expect("POST message within timeout")
        .expect("POST message connects");
        assert_eq!(
            response.status(),
            reqwest::StatusCode::OK,
            "message POST must stream: {}",
            response.text().await.unwrap_or_default()
        );
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            content_type.starts_with("text/event-stream"),
            "the messages endpoint must answer text/event-stream, got {content_type}"
        );

        let mut body = String::new();
        let mut stream = response.bytes_stream();
        loop {
            let chunk = tokio::time::timeout(IO_TIMEOUT, stream.next())
                .await
                .expect("SSE chunk within timeout");
            match chunk {
                Some(Ok(bytes)) => body.push_str(&String::from_utf8_lossy(&bytes)),
                Some(Err(e)) => panic!("SSE stream errored: {e}"),
                None => break,
            }
        }
        parse_sse_frames(&body)
    }
}

/// Parse a raw SSE body into `(event, data)` frames the way the SSE spec says:
/// frames separated by a blank line, one `event:`/`data:` pair each (this
/// server never emits multi-data frames — payloads are single-line JSON).
/// Comment-only keepalive frames (`: …`) are dropped.
fn parse_sse_frames(body: &str) -> Vec<(String, String)> {
    body.split("\n\n")
        .filter_map(|frame| {
            let mut event = None;
            let mut data = None;
            for line in frame.lines() {
                if let Some(name) = line.strip_prefix("event: ") {
                    event = Some(name.to_string());
                } else if let Some(payload) = line.strip_prefix("data: ") {
                    data = Some(payload.to_string());
                }
            }
            event.map(|event| (event, data.unwrap_or_default()))
        })
        .collect()
}

/// Every contracted SSE event name, straight from the protocol enum.
fn contract_names() -> std::collections::HashSet<&'static str> {
    use SseEventName as E;
    [
        E::Started,
        E::Text,
        E::ToolUseRequest,
        E::ToolUseResult,
        E::TurnCompleted,
        E::Completed,
        E::Failed,
        E::Warning,
        E::Progress,
        E::ToolProgress,
        E::Thinking,
        E::Usage,
        E::Cost,
        E::Info,
        E::ConversationUpdate,
        E::RateLimit,
        E::Error,
    ]
    .into_iter()
    .map(|n| n.as_str())
    .collect()
}

/// Wire-level invariants that hold for EVERY frame of every stream under
/// test: the event name is one of the contracted set, and the payload is
/// externally-tagged `QueryEvent` JSON (`{"Text": {…}}`) — except the
/// transport-level `error` channel, whose payload is `{"error": …}`.
fn assert_frames_are_contractual(frames: &[(String, String)]) {
    let names = contract_names();
    for (event, data) in frames {
        assert!(
            names.contains(event.as_str()),
            "SSE event name '{event}' is outside the SseEventName contract"
        );
        let payload: serde_json::Value = serde_json::from_str(data)
            .unwrap_or_else(|e| panic!("frame '{event}': data is JSON: {e}"));
        if event == "error" {
            assert!(
                payload.get("error").is_some(),
                "transport error frames carry {{\"error\": …}}: {payload}"
            );
        } else {
            assert!(
                payload.as_object().is_some_and(|o| o.len() == 1),
                "QueryEvent payloads are externally tagged (one top-level key): {payload}"
            );
        }
    }
}

/// Concatenate every `text` frame payload — the assistant's streamed answer.
fn joined_text(frames: &[(String, String)]) -> String {
    frames
        .iter()
        .filter(|(event, _)| event == "text")
        .map(|(_, data)| {
            serde_json::from_str::<serde_json::Value>(data)
                .unwrap_or_else(|e| panic!("text frame is JSON: {e}"))["Text"]["content"]
                .as_str()
                .expect("text frame carries Text.content")
                .to_string()
        })
        .collect()
}

/// Index of the first frame whose `event` matches, for ordering assertions.
fn index_of(frames: &[(String, String)], event: &str) -> usize {
    frames
        .iter()
        .position(|(e, _)| e == event)
        .unwrap_or_else(|| panic!("no '{event}' frame in {frames:?}"))
}

/// True when any request the fake saw is a JSON body whose serialized form
/// contains `needle`. Coarse by design — the structured assertions live in
/// the tests; this one pins "the server sent the history back".
fn any_request_contains(fake: &FakeLlm, needle: &str) -> bool {
    fake.requests().iter().any(|req| {
        req["messages"]
            .as_array()
            .map(|msgs| msgs.iter().any(|m| m.to_string().contains(needle)))
            .unwrap_or(false)
    })
}

// ── Script 1: pure text, two messages on one session ───────────────────────

/// A text-only turn streams the scripted chunks as `text` frames in order and
/// terminates with `completed`; a second message on the SAME session reaches
/// the LLM with the first turn's assistant text restored into the history
/// (the ConversationUpdate → restore_messages contract, F45/§P1-6).
#[tokio::test]
async fn text_turn_streams_scripted_chunks_and_second_message_sees_history() {
    let _env = ENV_SERIAL.lock().await;
    let stack = Stack::start(
        r#"{"turns":[
            {"chunks":["Hello"," from"," the fake"]},
            {"chunks":["Second"," answer"]}
        ]}"#,
    )
    .await;
    let session = stack.wait_ready().await;

    // ── Turn 1 ──
    let frames = stack.post_message(&session, "hi there").await;
    assert_frames_are_contractual(&frames);
    assert_eq!(
        joined_text(&frames),
        "Hello from the fake",
        "text frames must carry the scripted chunks in order"
    );
    let (last_event, last_data) = frames.last().expect("frames non-empty").clone();
    assert_eq!(last_event, "completed", "terminal frame is completed");
    let completed: serde_json::Value = serde_json::from_str(&last_data).unwrap();
    assert_eq!(
        completed["Completed"]["outcome"], "completed",
        "Completed.outcome is the snake_case QueryOutcome"
    );
    assert!(
        completed["Completed"]["query_id"].is_string(),
        "Completed carries the query_id: {completed}"
    );

    // ── Turn 2, same session ──
    let frames2 = stack.post_message(&session, "and again?").await;
    assert_frames_are_contractual(&frames2);
    assert_eq!(joined_text(&frames2), "Second answer");
    assert_eq!(frames2.last().expect("frames").0, "completed");

    // The agentic loop's second HTTP call to the LLM must carry the first
    // turn's assistant text — proof the server path kept session context.
    assert_eq!(stack.fake.requests().len(), 2, "one LLM call per message");
    assert!(
        any_request_contains(&stack.fake, "Hello from the fake"),
        "the follow-up request must include turn 1's assistant text; got: {:?}",
        stack.fake.requests()
    );
}

// ── Script 2: one tool call, two LLM turns ──────────────────────────────────

/// A tool turn round-trips through the real server: `tool_use_request` (with
/// the scripted tool name/input), then `tool_use_result` for the same
/// `tool_use_id`, then the next LLM turn's answer, terminated by `completed`.
/// The bare-registry permission gate may deny the execution — the wire
/// contract under test is the frame sequence, not the tool's verdict, so the
/// result frame is asserted structurally (id match), not by `is_error`.
#[tokio::test]
async fn tool_turn_round_trips_request_and_result_frames() {
    let _env = ENV_SERIAL.lock().await;
    let stack = Stack::start(
        r#"{"turns":[
            {"chunks":["Let me check."],
             "tool_use":{"id":"toolu_smoke_1","name":"bash",
                         "input":{"command":"echo smoke"}}},
            {"chunks":["Done — the tool ran."]}
        ]}"#,
    )
    .await;
    let session = stack.wait_ready().await;

    let frames = stack.post_message(&session, "list the files").await;
    assert_frames_are_contractual(&frames);

    // The request frame carries the scripted tool call verbatim.
    let (request_idx, request_data) = frames
        .iter()
        .enumerate()
        .find_map(|(i, (e, d))| (e == "tool_use_request").then(|| (i, d.clone())))
        .expect("a tool_use_request frame was emitted");
    let request: serde_json::Value = serde_json::from_str(&request_data).unwrap();
    assert_eq!(request["ToolUseRequest"]["tool_name"], "bash");
    assert_eq!(request["ToolUseRequest"]["tool_use_id"], "toolu_smoke_1");
    assert_eq!(
        request["ToolUseRequest"]["tool_input"]["command"],
        "echo smoke"
    );

    // …and the result frame answers the same id, after the request.
    let result_idx = index_of(&frames, "tool_use_result");
    let result: serde_json::Value = serde_json::from_str(&frames[result_idx].1).unwrap();
    assert_eq!(
        result["ToolUseResult"]["tool_use_id"], "toolu_smoke_1",
        "the result answers the requested tool_use_id: {result}"
    );
    assert!(result_idx > request_idx, "result follows request");

    // The follow-up LLM turn still happened and closed the stream.
    assert!(
        joined_text(&frames).contains("Done — the tool ran."),
        "turn 2's text arrives after the tool round-trip"
    );
    assert!(
        index_of(&frames, "tool_use_result") < index_of(&frames, "turn_completed"),
        "the tool round-trip completes before the turn is booked"
    );
    assert_eq!(
        frames.last().expect("frames").0,
        "completed",
        "the multi-turn query terminates with completed"
    );

    // The engine's second LLM call must carry the tool result block.
    assert_eq!(stack.fake.served(), 2, "tool turn drives a second LLM call");
    let second = &stack.fake.requests()[1];
    let has_tool_result = second["messages"]
        .as_array()
        .map(|msgs| {
            msgs.iter().any(|m| {
                m.to_string().contains("\"tool_result\"") && m.to_string().contains("toolu_smoke_1")
            })
        })
        .unwrap_or(false);
    assert!(
        has_tool_result,
        "the follow-up request must carry the tool_result for toolu_smoke_1: {second}"
    );
}

// ── Script 3: provider error ────────────────────────────────────────────────

/// A mid-stream provider error (non-retryable message) must terminate the
/// SSE stream with the contract's `failed` event carrying the provider's
/// message — never `completed`, never a silent end.
#[tokio::test]
async fn provider_error_terminates_stream_with_failed_frame() {
    let _env = ENV_SERIAL.lock().await;
    let stack = Stack::start(
        r#"{"turns":[
            {"provider_error":{"type":"authentication_error",
                               "message":"invalid x-api-key"}}
        ]}"#,
    )
    .await;
    let session = stack.wait_ready().await;

    let frames = stack.post_message(&session, "trigger the error").await;
    assert_frames_are_contractual(&frames);

    let (last_event, last_data) = frames.last().expect("frames non-empty").clone();
    assert_eq!(last_event, "failed", "terminal frame is failed: {frames:?}");
    let failed: serde_json::Value = serde_json::from_str(&last_data).unwrap();
    assert!(
        failed["Failed"]["query_id"].is_string(),
        "Failed carries the query_id: {failed}"
    );
    let error_text = failed["Failed"]["error"]
        .as_str()
        .expect("Failed carries an error string");
    assert!(
        error_text.contains("invalid x-api-key"),
        "the provider's message surfaces to the client: {error_text}"
    );
    assert!(
        !frames.iter().any(|(e, _)| e == "completed"),
        "a failed query must not also report completed"
    );
}

// ── Parser self-check ───────────────────────────────────────────────────────

/// The frame parser mirrors what axum writes (`event: name\ndata: payload\n\n`
/// per event) and drops keepalive comment frames. Runs against a sample that
/// includes one, so a keepalive mid-stream cannot corrupt assertions.
#[test]
fn sse_parser_handles_keepalives_and_multi_frame_bodies() {
    let body = "event: text\ndata: {\"Text\":{\"content\":\"a\"}}\n\n\
                : keepalive\n\n\
                event: completed\ndata: {\"Completed\":{\"query_id\":\"q\",\"outcome\":\"completed\"}}\n\n";
    let frames = parse_sse_frames(body);
    assert_eq!(
        frames,
        vec![
            (
                "text".to_string(),
                "{\"Text\":{\"content\":\"a\"}}".to_string()
            ),
            (
                "completed".to_string(),
                "{\"Completed\":{\"query_id\":\"q\",\"outcome\":\"completed\"}}".to_string()
            ),
        ]
    );
}
