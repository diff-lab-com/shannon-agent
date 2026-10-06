//! Fake LLM upstream for the chat wire-contract smoke tests (R6).
//!
//! A local HTTP server that speaks the Anthropic `/v1/messages` SSE dialect —
//! the shortest wire path through the engine (`LlmProvider::Anthropic`
//! passthrough: the adapter serializes the request natively and
//! `normalize_anthropic_event` deserializes each `data:` line 1:1 into
//! `StreamEvent`). Everything it says comes from a JSON **script** whose
//! vocabulary mirrors the e2e ChatScript semantics (text `chunks`, one
//! `tool_use`, terminal `error`) so both playback layers read the same way.
//!
//! Multi-turn: request N is served script turn N, which is how the agentic
//! loop is driven end-to-end — turn 1 asks for a tool, the engine executes it
//! (whatever the bare registry decides), turn 2 closes with a final answer.
//!
//! Offline by construction: binds `127.0.0.1:0`, validates no credentials,
//! touches nothing outside the test process.

#![allow(clippy::unwrap_used)] // test infrastructure: malformed fixtures must panic loudly

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

/// One scripted LLM turn: how the fake answers the Nth `/v1/messages` call.
///
/// Shape (see the tests for live examples):
/// ```json
/// { "chunks": ["Hello", " world"] }
/// { "chunks": ["Checking."], "tool_use": { "id": "toolu_1", "name": "bash",
///                                          "input": { "command": "ls" } } }
/// { "provider_error": { "type": "authentication_error", "message": "nope" } }
/// { "http_error": { "status": 401, "type": "authentication_error", "message": "nope" } }
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct Turn {
    /// Narration/text chunks, streamed one `text_delta` each, in order.
    #[serde(default)]
    pub chunks: Vec<String>,
    /// When present, a `tool_use` content block follows the text and the
    /// turn's stop reason becomes `tool_use` (the engine must execute the
    /// tool and come back for the next turn).
    #[serde(default)]
    pub tool_use: Option<ToolCall>,
    /// Mid-stream Anthropic error event — the stream ends right after it.
    #[serde(default)]
    pub provider_error: Option<ProviderError>,
    /// Fail the whole HTTP request with this status + Anthropic error body.
    #[serde(default)]
    pub http_error: Option<HttpError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderError {
    #[serde(default = "default_error_type")]
    pub r#type: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpError {
    pub status: u16,
    #[serde(default = "default_error_type")]
    pub r#type: String,
    pub message: String,
}

fn default_error_type() -> String {
    "api_error".to_string()
}

/// A whole script: one entry per expected `/v1/messages` request.
#[derive(Debug, Clone, Deserialize)]
pub struct Script {
    pub turns: Vec<Turn>,
}

impl Script {
    pub fn parse(json: &str) -> Self {
        serde_json::from_str(json).expect("fake_llm script must be valid JSON")
    }
}

/// Render one scripted turn as a full Anthropic SSE response body.
///
/// The frame sequence is the canonical provider shape the engine's streaming
/// parser expects: `message_start` → `content_block_start`(text) → one
/// `content_block_delta` per chunk → `content_block_stop` → [`tool_use
/// block`] → `message_delta`(stop_reason) → `message_stop`.
pub fn render_anthropic_sse(turn: &Turn) -> String {
    if let Some(err) = &turn.provider_error {
        return format!(
            "data: {}\n\n",
            json!({
                "type": "error",
                "error": { "type": err.r#type, "message": err.message },
            })
        );
    }

    let mut frames: Vec<String> = Vec::new();
    let usage = json!({"input_tokens": 12, "output_tokens": 34});
    frames.push(format!(
        "data: {}",
        json!({
            "type": "message_start",
            "message": {
                "id": "msg_fake_1",
                "role": "assistant",
                "content": [],
                "model": "fake-model",
                "stop_reason": Value::Null,
                "usage": usage,
            },
        })
    ));

    if !turn.chunks.is_empty() {
        frames.push(format!(
            "data: {}",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": { "type": "text", "text": "" },
            })
        ));
        for chunk in &turn.chunks {
            frames.push(format!(
                "data: {}",
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": { "type": "text_delta", "text": chunk },
                })
            ));
        }
        frames.push("data: {\"type\":\"content_block_stop\",\"index\":0}".to_string());
    }

    let stop_reason = if turn.tool_use.is_some() {
        if let Some(tool) = &turn.tool_use {
            frames.push(format!(
                "data: {}",
                json!({
                    "type": "content_block_start",
                    "index": 1,
                    "content_block": {
                        "type": "tool_use",
                        "id": tool.id,
                        "name": tool.name,
                        "input": {},
                    },
                })
            ));
            // The real API streams tool arguments as partial JSON; one chunk
            // carrying the whole document is a legal specialization of it.
            frames.push(format!(
                "data: {}",
                json!({
                    "type": "content_block_delta",
                    "index": 1,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": serde_json::to_string(&tool.input)
                            .expect("tool input is valid JSON"),
                    },
                })
            ));
            frames.push("data: {\"type\":\"content_block_stop\",\"index\":1}".to_string());
        }
        "tool_use"
    } else {
        "end_turn"
    };

    frames.push(format!(
        "data: {}",
        json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason, "stop_sequence": Value::Null },
            "usage": { "input_tokens": 12, "output_tokens": 57 },
        })
    ));
    frames.push("data: {\"type\":\"message_stop\"}".to_string());

    let mut body = String::new();
    for frame in frames {
        body.push_str(&frame);
        body.push_str("\n\n");
    }
    body
}

/// Handle to a running fake: its base URL and every request body it saw.
#[derive(Clone)]
pub struct FakeLlm {
    base_url: String,
    requests: Arc<Mutex<Vec<Value>>>,
    served: Arc<AtomicUsize>,
}

impl FakeLlm {
    /// Bind `127.0.0.1:0`, install `script`, and serve until the returned
    /// task is aborted (dropping the test's tokio runtime stops it).
    pub async fn start(script: Script) -> Self {
        let script = Arc::new(script);
        let requests: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let served = Arc::new(AtomicUsize::new(0));

        let state = FakeState {
            script: Arc::clone(&script),
            requests: Arc::clone(&requests),
            served: Arc::clone(&served),
        };
        let app = Router::new()
            .route("/v1/messages", post(messages))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("fake LLM binds an ephemeral loopback port");
        let addr = listener.local_addr().expect("fake LLM local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("fake LLM serve");
        });

        Self {
            base_url: format!("http://{addr}"),
            requests,
            served,
        }
    }

    /// Base URL for `LlmClientConfig::base_url` (no trailing slash — the
    /// engine appends the provider endpoint path itself).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Every `/v1/messages` request body, in arrival order.
    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }

    /// How many turns have been served so far.
    pub fn served(&self) -> usize {
        self.served.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
struct FakeState {
    script: Arc<Script>,
    requests: Arc<Mutex<Vec<Value>>>,
    served: Arc<AtomicUsize>,
}

/// Serve script turn N for the Nth request. Exhausting the script is a loud
/// 500 — a test that silently loops against the last turn would lie.
async fn messages(State(state): State<FakeState>, Json(body): Json<Value>) -> impl IntoResponse {
    let index = {
        let mut requests = state.requests.lock().unwrap();
        requests.push(body);
        requests.len() - 1
    };
    state.served.fetch_add(1, Ordering::SeqCst);

    let Some(turn) = state.script.turns.get(index) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            format!(
                "{{\"error\":\"fake_llm script exhausted: request #{} but only {} turn(s)\"}}",
                index + 1,
                state.script.turns.len()
            ),
        );
    };

    if let Some(http_err) = &turn.http_error {
        let status =
            StatusCode::from_u16(http_err.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = json!({
            "type": "error",
            "error": { "type": http_err.r#type, "message": http_err.message },
        });
        return (
            status,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            body.to_string(),
        );
    }

    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        render_anthropic_sse(turn),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_turn_renders_canonical_anthropic_sse() {
        let script = Script::parse(r#"{"turns":[{"chunks":["Hello"," from"," the fake"]}]}"#);
        let body = render_anthropic_sse(&script.turns[0]);
        let data_lines: Vec<&str> = body.lines().filter(|l| l.starts_with("data: ")).collect();
        let types: Vec<String> = data_lines
            .iter()
            .map(|l| {
                serde_json::from_str::<Value>(&l["data: ".len()..])
                    .expect("every data line is JSON")["type"]
                    .as_str()
                    .expect("every frame has a type")
                    .to_string()
            })
            .collect();
        assert_eq!(
            types,
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ]
        );
        assert!(body.contains("\"text\":\"Hello\""));
        assert!(body.contains("\"stop_reason\":\"end_turn\""));
        // SSE framing: exactly one blank line between frames.
        assert!(!body.contains("\n\n\n"));
    }

    #[test]
    fn tool_turn_sets_tool_use_stop_reason_and_emits_the_block() {
        let script = Script::parse(
            r#"{"turns":[{"chunks":["Checking."],"tool_use":{"id":"toolu_1","name":"bash","input":{"command":"ls"}}}]}"#,
        );
        let body = render_anthropic_sse(&script.turns[0]);
        assert!(body.contains("\"type\":\"tool_use\""));
        assert!(body.contains("\"id\":\"toolu_1\""));
        assert!(body.contains("\"name\":\"bash\""));
        assert!(body.contains("\"partial_json\":\"{\\\"command\\\":\\\"ls\\\"}\""));
        assert!(body.contains("\"stop_reason\":\"tool_use\""));
    }

    #[test]
    fn provider_error_turn_is_a_single_error_frame() {
        let script = Script::parse(
            r#"{"turns":[{"provider_error":{"type":"authentication_error","message":"invalid x-api-key"}}]}"#,
        );
        let body = render_anthropic_sse(&script.turns[0]);
        assert_eq!(body.lines().filter(|l| !l.is_empty()).count(), 1);
        assert!(body.contains("\"type\":\"error\""));
        assert!(body.contains("authentication_error"));
        assert!(!body.contains("message_start"));
    }
}
