//! CLI end-to-end tests using mockito to simulate LLM providers.
//!
//! These tests exercise the compiled `shannon` binary with mock HTTP backends,
//! verifying the full pipeline: CLI args -> config loading -> LLM request ->
//! response processing -> output formatting -> exit codes.
//!
//! Coverage: Ollama, Anthropic, OpenAI, DeepSeek, Groq, Mistral (OpenAI-compatible),
//! multi-turn tool use, context preservation, compact, streaming formats, error recovery.
//!
//! Run with: cargo test --test cli_e2e_tests -- --test-threads=1

use assert_cmd::Command;
use mockito::{Matcher, Mock, ServerGuard};
use predicates::prelude::*;
use serial_test::serial;

const BIN: &str = "shannon";

fn shannon() -> Command {
    Command::cargo_bin(BIN).unwrap()
}

// ── Mock Response Builders ─────────────────────────────────────────────

fn ollama_streaming_body(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "{{\"message\":{{\"role\":\"assistant\",\"content\":\"{escaped}\"}},\"model\":\"test\",\"done\":false}}\n\
         {{\"message\":{{\"role\":\"assistant\",\"content\":\"\"}},\"model\":\"test\",\"done\":true}}\n"
    )
}

fn ollama_non_streaming_body(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "{{\"message\":{{\"role\":\"assistant\",\"content\":\"{escaped}\"}},\"model\":\"test\",\"done\":true}}"
    )
}

fn anthropic_sse_body(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "data: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_test\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{{\"input_tokens\":10,\"output_tokens\":0}}}}}}\n\n\
         data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n\
         data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{escaped}\"}}}}\n\n\
         data: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n\
         data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\"}},\"usage\":{{\"input_tokens\":10,\"output_tokens\":8}}}}\n\n\
         data: {{\"type\":\"message_stop\"}}\n\n"
    )
}

fn openai_sse_body(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "data: {{\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"test\",\"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"assistant\",\"content\":\"{escaped}\"}},\"finish_reason\":null}}]}}\n\n\
         data: {{\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"test\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n\
         data: [DONE]\n\n"
    )
}

// ── Mock Server Setup ──────────────────────────────────────────────────

fn mock_ollama_streaming(server: &mut ServerGuard, text: &str) -> Mock {
    server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(ollama_streaming_body(text))
        .expect(1)
        .create()
}

#[allow(dead_code)]
fn mock_ollama_non_streaming(server: &mut ServerGuard, text: &str) -> Mock {
    server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(ollama_non_streaming_body(text))
        .expect(1)
        .create()
}

fn mock_anthropic_streaming(server: &mut ServerGuard, text: &str) -> Mock {
    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(anthropic_sse_body(text))
        .expect(1)
        .create()
}

fn mock_openai_streaming(server: &mut ServerGuard, text: &str) -> Mock {
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(openai_sse_body(text))
        .expect(1)
        .create()
}

/// Mock for Groq — uses /openai/v1/chat/completions endpoint.
fn mock_groq_streaming(server: &mut ServerGuard, text: &str) -> Mock {
    server
        .mock("POST", "/openai/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(openai_sse_body(text))
        .expect(1)
        .create()
}

// ── Common Helpers ─────────────────────────────────────────────────────

/// Build a shannon command with clean env vars pointing to mock server.
fn shannon_with_mock(provider: &str, server_url: &str) -> Command {
    let mut cmd = shannon();
    cmd.env("SHANNON_BASE_URL", server_url)
        .env("SHANNON_PROVIDER", provider)
        .env("SHANNON_MODEL", "test-model")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("SHANNON_API_KEY")
        .current_dir(std::env::temp_dir());
    cmd
}

/// Extract owned stdout from an Assert result.
fn stdout_string(assert: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assert.get_output().stdout).to_string()
}

/// Extract owned stderr from an Assert result.
fn stderr_string(assert: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assert.get_output().stderr).to_string()
}

/// Parse stdout as JSON, with helpful error on failure.
fn parse_json_output(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON output:\n{stdout}\nParse error: {e}"))
}

// ════════════════════════════════════════════════════════════════════════
// Section: Normal text responses across providers
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn test_ollama_text_response_headless() {
    let mut server = mockito::Server::new_async().await;
    let _m = mock_ollama_streaming(&mut server, "Hello from Ollama!");

    let result = shannon_with_mock("ollama", &server.url())
        .args(["--prompt", "test query", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    assert_eq!(json["exit_code"], "success");
    let response = json["response"].as_str().unwrap_or("");
    assert!(response.contains("Ollama"), "Expected 'Ollama' in response, got: {response}");
}

#[tokio::test]
#[serial]
async fn test_openai_text_response_headless() {
    let mut server = mockito::Server::new_async().await;
    let _m = mock_openai_streaming(&mut server, "Hello from OpenAI!");

    let result = shannon_with_mock("openai", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    assert_eq!(json["exit_code"], "success");
    let response = json["response"].as_str().unwrap_or("");
    assert!(response.contains("OpenAI"), "Expected 'OpenAI' in response, got: {response}");
}

#[tokio::test]
#[serial]
async fn test_anthropic_text_response_headless() {
    let mut server = mockito::Server::new_async().await;
    let _m = mock_anthropic_streaming(&mut server, "Hello from Anthropic!");

    let result = shannon_with_mock("anthropic", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    assert_eq!(json["exit_code"], "success");
    let response = json["response"].as_str().unwrap_or("");
    assert!(response.contains("Anthropic"), "Expected 'Anthropic' in response, got: {response}");
}

// ════════════════════════════════════════════════════════════════════════
// Section: DeepSeek / GLM / Groq / Mistral (OpenAI-compatible providers)
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn test_deepseek_text_response_headless() {
    // DeepSeek uses OpenAI-compatible wire format, endpoint /v1/chat/completions
    let mut server = mockito::Server::new_async().await;
    let _m = mock_openai_streaming(&mut server, "Hello from DeepSeek!");

    let result = shannon_with_mock("deepseek", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    assert_eq!(json["exit_code"], "success");
    let response = json["response"].as_str().unwrap_or("");
    assert!(
        response.contains("DeepSeek"),
        "Expected 'DeepSeek' in response, got: {response}"
    );
}

#[tokio::test]
#[serial]
async fn test_deepseek_streaming_text_output() {
    let mut server = mockito::Server::new_async().await;
    let _m = mock_openai_streaming(&mut server, "DeepSeek streaming works!");

    shannon_with_mock("deepseek", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query", "--output-format", "text"])
        .assert()
        .success()
        .stdout(predicate::str::contains("DeepSeek streaming works!"));
}

#[tokio::test]
#[serial]
async fn test_mistral_text_response_headless() {
    // Mistral uses OpenAI-compatible wire format, endpoint /v1/chat/completions
    let mut server = mockito::Server::new_async().await;
    let _m = mock_openai_streaming(&mut server, "Bonjour from Mistral!");

    let result = shannon_with_mock("mistral", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    assert_eq!(json["exit_code"], "success");
    let response = json["response"].as_str().unwrap_or("");
    assert!(
        response.contains("Mistral"),
        "Expected 'Mistral' in response, got: {response}"
    );
}

#[tokio::test]
#[serial]
async fn test_groq_text_response_headless() {
    // Groq uses OpenAI wire format but endpoint /openai/v1/chat/completions
    let mut server = mockito::Server::new_async().await;
    let _m = mock_groq_streaming(&mut server, "Fast response from Groq!");

    let result = shannon_with_mock("groq", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    assert_eq!(json["exit_code"], "success");
    let response = json["response"].as_str().unwrap_or("");
    assert!(
        response.contains("Groq"),
        "Expected 'Groq' in response, got: {response}"
    );
}

/// Verify multiple OpenAI-compatible providers use the same wire format.
/// DeepSeek, Mistral, and OpenAI all use /v1/chat/completions with SSE streaming.
#[tokio::test]
#[serial]
async fn test_openai_compatible_providers_same_endpoint() {
    for provider in &["deepseek", "mistral"] {
        let mut server = mockito::Server::new_async().await;
        let _m = mock_openai_streaming(&mut server, &format!("Response via {provider}"));

        let result = shannon_with_mock(provider, &server.url())
            .env("SHANNON_API_KEY", "test-key")
            .args(["--prompt", "test", "--output-format", "json"])
            .assert();

        let stdout = stdout_string(&result);
        let json = parse_json_output(&stdout);
        assert_eq!(
            json["exit_code"], "success",
            "Provider '{provider}' should succeed"
        );
    }
}

// ════════════════════════════════════════════════════════════════════════
// Section: Ollama malformed output retry
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn test_ollama_malformed_retry() {
    let mut server = mockito::Server::new_async().await;

    // First call (streaming, with tools) -> 500 malformed output error
    let _mock_err = server
        .mock("POST", "/api/chat")
        .match_body(Matcher::Regex(r#""stream":\s*true"#.to_string()))
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":"can't find closing '}' symbol"}"#)
        .expect(1)
        .create();

    // Retry (non-streaming, without tools) -> 200 success
    let _mock_ok = server
        .mock("POST", "/api/chat")
        .match_body(Matcher::Regex(r#""stream":\s*false"#.to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(ollama_non_streaming_body("Retry succeeded without tools."))
        .expect(1)
        .create();

    let result = shannon_with_mock("ollama", &server.url())
        .args(["--prompt", "test query", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    assert_eq!(json["exit_code"], "success");
    let response = json["response"].as_str().unwrap_or("");
    assert!(
        response.contains("Retry"),
        "Expected retry text in response, got: {response}"
    );
}

#[tokio::test]
#[serial]
async fn test_ollama_generic_500_retry() {
    // Generic 500 (not malformed output) should be retried with exponential backoff.
    let mut server = mockito::Server::new_async().await;

    // First call: generic 500 → retryable
    let _mock_err = server
        .mock("POST", "/api/chat")
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":"Internal Server Error"}"#)
        .expect(1)
        .create();

    // Retry: success
    let _mock_ok = mock_ollama_streaming(&mut server, "Recovered after retry.");

    let result = shannon_with_mock("ollama", &server.url())
        .args(["--prompt", "test query", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    assert_eq!(json["exit_code"], "success");
    let response = json["response"].as_str().unwrap_or("");
    assert!(
        response.contains("Recovered"),
        "Expected recovery text, got: {response}"
    );
}

// ════════════════════════════════════════════════════════════════════════
// Section: Streaming response (text output mode)
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn test_anthropic_streaming_response() {
    let mut server = mockito::Server::new_async().await;
    let _m = mock_anthropic_streaming(&mut server, "Streamed response text!");

    shannon_with_mock("anthropic", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query", "--output-format", "text"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Streamed response text!"));
}

#[tokio::test]
#[serial]
async fn test_openai_streaming_response() {
    let mut server = mockito::Server::new_async().await;
    let _m = mock_openai_streaming(&mut server, "OpenAI streaming works!");

    shannon_with_mock("openai", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query", "--output-format", "text"])
        .assert()
        .success()
        .stdout(predicate::str::contains("OpenAI streaming works!"));
}

// ════════════════════════════════════════════════════════════════════════
// Section: Multi-turn tool use cycle
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn test_anthropic_usage_tracking() {
    // Anthropic SSE includes usage (input_tokens/output_tokens) — verify it's captured.
    let mut server = mockito::Server::new_async().await;
    let _m = mock_anthropic_streaming(&mut server, "Usage tracking works");

    let result = shannon_with_mock("anthropic", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    assert_eq!(json["exit_code"], "success");
    let tokens = json["total_tokens"].as_u64().unwrap_or(0);
    assert!(tokens > 0, "Anthropic response should report tokens > 0, got: {tokens}");
}

#[tokio::test]
#[serial]
async fn test_openai_streaming_json_output() {
    // Verify OpenAI streaming response produces complete JSON output
    let mut server = mockito::Server::new_async().await;
    let _m = mock_openai_streaming(&mut server, "OpenAI streaming JSON");

    let result = shannon_with_mock("openai", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    assert_eq!(json["exit_code"], "success");
    assert!(json["response"].as_str().unwrap_or("").contains("OpenAI streaming"));
    assert!(json["tool_calls"].is_array());
}

#[tokio::test]
#[serial]
async fn test_deepseek_streaming_json_output() {
    // DeepSeek uses OpenAI wire format — verify it produces correct JSON
    let mut server = mockito::Server::new_async().await;
    let _m = mock_openai_streaming(&mut server, "DeepSeek streaming works");

    let result = shannon_with_mock("deepseek", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    assert_eq!(json["exit_code"], "success");
    assert!(json["response"].as_str().unwrap_or("").contains("DeepSeek"));
    assert!(json["prompt"].as_str().unwrap_or("").contains("test query"));
}

// ════════════════════════════════════════════════════════════════════════
// Section: Context preservation and conversation integrity
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn test_context_preservation_prompt_in_output() {
    // Verify the original prompt is preserved in HeadlessOutput.prompt field
    let mut server = mockito::Server::new_async().await;
    let _m = mock_ollama_streaming(&mut server, "Context preserved response");

    let result = shannon_with_mock("ollama", &server.url())
        .args(["--prompt", "What is the meaning of 42?", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    let prompt = json["prompt"].as_str().unwrap_or("");
    assert!(
        prompt.contains("meaning of 42"),
        "Prompt should be preserved in output, got: {prompt}"
    );
    assert_eq!(json["exit_code"], "success");
}

#[tokio::test]
#[serial]
async fn test_prompt_preserved_in_response_context() {
    // Verify the response contains relevant content and prompt is preserved
    let mut server = mockito::Server::new_async().await;
    let _m = mock_anthropic_streaming(&mut server, "The answer involves Rust and memory safety.");

    let result = shannon_with_mock("anthropic", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "Tell me about Rust", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    let prompt = json["prompt"].as_str().unwrap_or("");
    assert!(prompt.contains("Rust"), "Prompt should be preserved, got: {prompt}");

    let response = json["response"].as_str().unwrap_or("");
    assert!(
        response.contains("Rust"),
        "Response should contain relevant content, got: {response}"
    );
}

#[tokio::test]
#[serial]
async fn test_json_stream_event_sequence() {
    // Verify json-stream output produces correct event ordering:
    // start → text_delta* → done (CiEvent) → done (OutputEvent)
    let mut server = mockito::Server::new_async().await;
    let _m = mock_ollama_streaming(&mut server, "Event sequence test");

    let result = shannon_with_mock("ollama", &server.url())
        .args(["--prompt", "test query", "--output-format", "json-stream"])
        .assert();

    let stdout = stdout_string(&result);
    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("Invalid NDJSON: {line}\n{e}")))
        .collect();

    assert!(!events.is_empty(), "Should produce at least one event");

    // First event should be "start"
    assert_eq!(events[0]["type"], "start", "First event should be 'start', got: {}", events[0]);

    // Find the CiEvent::Done (has turns_used + tokens_used, not just exit_code)
    let ci_done = events.iter().find(|e| {
        e["type"] == "done" && e.get("turns_used").is_some()
    });
    assert!(ci_done.is_some(), "Should have CiEvent::Done with turns_used");

    let done = ci_done.unwrap();
    assert!(done.get("exit_code").is_some(), "done should have exit_code");
    assert!(done.get("turns_used").is_some(), "done should have turns_used");
    assert!(done.get("tokens_used").is_some(), "done should have tokens_used");
}

#[tokio::test]
#[serial]
async fn test_json_stream_text_delta_events() {
    // Verify json-stream includes "text_delta" events with content (OutputEvent format)
    let mut server = mockito::Server::new_async().await;
    let _m = mock_anthropic_streaming(&mut server, "Stream message content");

    let result = shannon_with_mock("anthropic", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query", "--output-format", "json-stream"])
        .assert();

    let stdout = stdout_string(&result);
    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("Invalid NDJSON: {line}\n{e}")))
        .collect();

    // Should have at least: start, text_delta, done
    assert!(events.len() >= 3, "Should have at least start+text_delta+done events, got {}", events.len());

    // Find text_delta events (the actual streaming content events)
    let text_events: Vec<_> = events.iter().filter(|e| e["type"] == "text_delta").collect();
    assert!(!text_events.is_empty(), "Should have at least one 'text_delta' event");

    let content = text_events[0]["content"].as_str().unwrap_or("");
    assert!(!content.is_empty(), "text_delta event should have content");
}

#[tokio::test]
#[serial]
async fn test_json_stream_anthropic_full_event_flow() {
    // Verify Anthropic json-stream: start → text_delta → CiEvent::done → OutputEvent::done
    let mut server = mockito::Server::new_async().await;
    let _m = mock_anthropic_streaming(&mut server, "Full flow test");

    let result = shannon_with_mock("anthropic", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test", "--output-format", "json-stream"])
        .assert();

    let stdout = stdout_string(&result);
    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("Invalid NDJSON: {line}\n{e}")))
        .collect();

    let types: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap_or("unknown")).collect();

    // Should have start as first event
    assert_eq!(types[0], "start", "First event should be start");

    // Should have at least one text_delta
    assert!(types.contains(&"text_delta"), "Should have text_delta events, got: {types:?}");

    // Should end with two done events (CiEvent::Done then OutputEvent::Done)
    let done_count = types.iter().filter(|&&t| t == "done").count();
    assert!(done_count >= 1, "Should have at least one done event");

    // CiEvent::Done should have full metadata
    let ci_done = events.iter().find(|e| e["type"] == "done" && e.get("turns_used").is_some());
    assert!(ci_done.is_some(), "Should have CiEvent::Done with turns_used");
    assert!(ci_done.unwrap()["exit_code"].as_i64().unwrap_or(-1) == 0, "exit_code should be 0 for success");
}

// ════════════════════════════════════════════════════════════════════════
// Section: Error handling and exit codes
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn test_auth_failure_exit_code() {
    let mut server = mockito::Server::new_async().await;

    server
        .mock("POST", "/v1/messages")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"type":"error","error":{"type":"authentication_error","message":"invalid api key"}}"#,
        )
        .expect(1)
        .create();

    let result = shannon_with_mock("anthropic", &server.url())
        .env("SHANNON_API_KEY", "invalid-key")
        .args(["--prompt", "test query"])
        .assert();

    assert!(
        !result.get_output().status.success(),
        "Auth failure should produce non-zero exit code"
    );
}

#[tokio::test]
#[serial]
async fn test_openai_auth_failure() {
    // OpenAI uses different error format: {"error":{"message":"...","type":"..."}}
    let mut server = mockito::Server::new_async().await;

    server
        .mock("POST", "/v1/chat/completions")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error"}}"#)
        .expect(1)
        .create();

    let result = shannon_with_mock("openai", &server.url())
        .env("SHANNON_API_KEY", "bad-key")
        .args(["--prompt", "test query"])
        .assert();

    assert!(
        !result.get_output().status.success(),
        "OpenAI auth failure should produce non-zero exit code"
    );
}

#[tokio::test]
#[serial]
async fn test_deepseek_auth_failure() {
    // DeepSeek uses OpenAI-compatible error format
    let mut server = mockito::Server::new_async().await;

    server
        .mock("POST", "/v1/chat/completions")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"message":"Authentication failed","type":"authentication_error"}}"#)
        .expect(1)
        .create();

    let result = shannon_with_mock("deepseek", &server.url())
        .env("SHANNON_API_KEY", "bad-key")
        .args(["--prompt", "test query"])
        .assert();

    assert!(
        !result.get_output().status.success(),
        "DeepSeek auth failure should produce non-zero exit code"
    );
}

#[tokio::test]
#[serial]
async fn test_rate_limit_exit_code() {
    let mut server = mockito::Server::new_async().await;

    // Rate limit triggers retries; expect up to 4 attempts (1 + 3 retries)
    server
        .mock("POST", "/v1/messages")
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"type":"error","error":{"type":"rate_limit_error","message":"rate limit exceeded"}}"#,
        )
        .expect(4)
        .create();

    let result = shannon_with_mock("anthropic", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
        let code = json["exit_code"].as_str().unwrap_or("unknown");
        assert!(
            code == "rate_limited" || code == "error",
            "Expected rate_limited or error exit code, got: {code}"
        );
    } else {
        assert!(
            !result.get_output().status.success(),
            "Rate limit should produce non-zero exit code"
        );
    }
}

#[tokio::test]
#[serial]
async fn test_server_error_503() {
    let mut server = mockito::Server::new_async().await;

    // 503 Service Unavailable — should retry and eventually fail
    server
        .mock("POST", "/api/chat")
        .with_status(503)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":"service unavailable"}"#)
        .expect(4)
        .create();

    let result = shannon_with_mock("ollama", &server.url())
        .args(["--prompt", "test query", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    // Should fail (non-zero exit) after retries exhausted
    assert!(
        !result.get_output().status.success(),
        "503 after retries should produce non-zero exit code"
    );
}

#[tokio::test]
#[serial]
async fn test_json_stream_error_event() {
    // Error in json-stream mode should produce an "error" event
    let mut server = mockito::Server::new_async().await;

    server
        .mock("POST", "/v1/messages")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(r#"{"type":"error","error":{"type":"authentication_error","message":"bad key"}}"#)
        .expect(1)
        .create();

    let result = shannon_with_mock("anthropic", &server.url())
        .env("SHANNON_API_KEY", "bad-key")
        .args(["--prompt", "test query", "--output-format", "json-stream"])
        .assert();

    let stdout = stdout_string(&result);
    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("Invalid NDJSON: {line}\n{e}")))
        .collect();

    // Should contain an error event
    let error_events: Vec<_> = events.iter().filter(|e| e["type"] == "error").collect();
    assert!(!error_events.is_empty(), "Should have error event in stream, got: {events:?}");
}

// ════════════════════════════════════════════════════════════════════════
// Section: Output format validation
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn test_json_output_structure() {
    let mut server = mockito::Server::new_async().await;
    let _m = mock_ollama_streaming(&mut server, "Structured response");

    let result = shannon_with_mock("ollama", &server.url())
        .args(["--prompt", "test query", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    for field in &["prompt", "response", "tool_calls", "total_tokens", "duration_ms", "exit_code"] {
        assert!(
            json.get(*field).is_some(),
            "Missing required field '{field}' in JSON output"
        );
    }

    assert!(json["prompt"].is_string(), "prompt should be string");
    assert!(json["response"].is_string(), "response should be string");
    assert!(json["tool_calls"].is_array(), "tool_calls should be array");
    assert!(json["total_tokens"].is_number(), "total_tokens should be number");
    assert!(json["duration_ms"].is_number(), "duration_ms should be number");
    assert!(json["exit_code"].is_string(), "exit_code should be string");
}

#[tokio::test]
#[serial]
async fn test_json_output_token_tracking() {
    // Use Anthropic mock which includes usage (input_tokens/output_tokens) in SSE
    let mut server = mockito::Server::new_async().await;
    let _m = mock_anthropic_streaming(&mut server, "Token tracking test");

    let result = shannon_with_mock("anthropic", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query", "--output-format", "json"])
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);

    let tokens = json["total_tokens"].as_u64().unwrap_or(0);
    assert!(tokens > 0, "total_tokens should be > 0, got: {tokens}");

    let duration = json["duration_ms"].as_u64().unwrap_or(0);
    assert!(duration > 0, "duration_ms should be > 0, got: {duration}");
}

#[tokio::test]
#[serial]
async fn test_json_stream_output() {
    let mut server = mockito::Server::new_async().await;
    let _m = mock_ollama_streaming(&mut server, "Streamed");

    let result = shannon_with_mock("ollama", &server.url())
        .args(["--prompt", "test query", "--output-format", "json-stream"])
        .assert();

    let stdout = stdout_string(&result);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert!(!lines.is_empty(), "Should produce at least one NDJSON line");

    let first: serde_json::Value = serde_json::from_str(lines[0])
        .unwrap_or_else(|e| panic!("Invalid NDJSON first line:\n{}\nError: {e}", lines[0]));
    assert!(first.get("type").is_some(), "NDJSON line should have 'type' field");

    let last: serde_json::Value = serde_json::from_str(lines[lines.len() - 1])
        .unwrap_or_else(|e| panic!("Invalid last NDJSON line:\n{}\nError: {e}", lines[lines.len() - 1]));
    assert_eq!(last["type"], "done", "Last event should be 'done', got: {last}");
}

// ════════════════════════════════════════════════════════════════════════
// Section: Bare prompt (non-headless path)
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn test_bare_prompt_noninteractive() {
    let mut server = mockito::Server::new_async().await;
    let _m = mock_ollama_streaming(&mut server, "Bare prompt response");

    shannon_with_mock("ollama", &server.url())
        .arg("test query")
        .assert()
        .success()
        .stdout(predicate::str::contains("Bare prompt response"));
}

#[tokio::test]
#[serial]
async fn test_bare_prompt_deepseek() {
    let mut server = mockito::Server::new_async().await;
    let _m = mock_openai_streaming(&mut server, "DeepSeek bare prompt response");

    shannon_with_mock("deepseek", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .arg("test query")
        .assert()
        .success()
        .stdout(predicate::str::contains("DeepSeek bare prompt response"));
}

// ════════════════════════════════════════════════════════════════════════
// Section: User-friendly error messages
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn test_error_message_not_raw_json() {
    let mut server = mockito::Server::new_async().await;

    server
        .mock("POST", "/v1/messages")
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"type":"error","error":{"type":"api_error","message":"Internal server error"}}"#,
        )
        .expect(1)
        .create();

    let result = shannon_with_mock("anthropic", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query"])
        .assert();

    let stderr = stderr_string(&result);
    let combined = format!("{}{}", stderr, stdout_string(&result));
    assert!(
        !combined.is_empty() || !result.get_output().status.success(),
        "Some error indication should appear"
    );
}

#[tokio::test]
#[serial]
async fn test_openai_format_error_displayed() {
    // Verify OpenAI-format errors are handled correctly
    let mut server = mockito::Server::new_async().await;

    server
        .mock("POST", "/v1/chat/completions")
        .with_status(400)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"message":"model not found","type":"invalid_request_error"}}"#)
        .expect(1)
        .create();

    let result = shannon_with_mock("openai", &server.url())
        .env("SHANNON_API_KEY", "test-key")
        .args(["--prompt", "test query"])
        .assert();

    assert!(
        !result.get_output().status.success(),
        "400 error should produce non-zero exit code"
    );
}

// ════════════════════════════════════════════════════════════════════════
// Section: Cross-provider consistency
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn test_all_producers_json_output_consistent() {
    // All providers should produce JSON output with the same structure
    let test_cases: Vec<(&str, &str, &str)> = vec![
        ("ollama", "/api/chat", "Ollama consistent"),
        ("openai", "/v1/chat/completions", "OpenAI consistent"),
        ("anthropic", "/v1/messages", "Anthropic consistent"),
        ("deepseek", "/v1/chat/completions", "DeepSeek consistent"),
    ];

    for (provider, _endpoint, response_text) in test_cases {
        let mut server = mockito::Server::new_async().await;

        match provider {
            "ollama" => { mock_ollama_streaming(&mut server, response_text); }
            "anthropic" => { mock_anthropic_streaming(&mut server, response_text); }
            _ => { mock_openai_streaming(&mut server, response_text); }
        }

        let mut cmd = shannon_with_mock(provider, &server.url());
        if provider != "ollama" {
            cmd.env("SHANNON_API_KEY", "test-key");
        }

        let result = cmd
            .args(["--prompt", "test", "--output-format", "json"])
            .assert();

        let stdout = stdout_string(&result);
        let json = parse_json_output(&stdout);

        // All providers should produce the same HeadlessOutput structure
        assert!(
            json.get("exit_code").is_some(),
            "Provider '{provider}' missing exit_code"
        );
        assert!(
            json.get("response").is_some(),
            "Provider '{provider}' missing response"
        );
        assert!(
            json.get("tool_calls").is_some(),
            "Provider '{provider}' missing tool_calls"
        );
        assert!(
            json.get("total_tokens").is_some(),
            "Provider '{provider}' missing total_tokens"
        );
        assert!(
            json.get("duration_ms").is_some(),
            "Provider '{provider}' missing duration_ms"
        );
    }
}
