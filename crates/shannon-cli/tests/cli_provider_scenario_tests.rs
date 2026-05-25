//! Goose-style provider scenario tests.
//!
//! Validates the same tool-use scenarios across Anthropic, OpenAI, and Ollama
//! to ensure consistent behavior regardless of provider response format.
//!
//! Scenarios tested per provider:
//!   1. Text-only response (no tools)
//!   2. Write tool_use -> text response (2 turns)
//!   3. Bash tool_use -> text response (2 turns)
//!
//! Run with: cargo test --test cli_provider_scenario_tests -- --test-threads=1

use assert_cmd::Command;
use serde_json::json;
use serial_test::serial;
use std::fs;

const BIN: &str = "shannon";

fn shannon() -> Command {
    Command::cargo_bin(BIN).unwrap()
}

// ── SSE Response Builders ─────────────────────────────────────────────

// --- Anthropic format (reference) ---

fn anthropic_text_sse(text: &str) -> String {
    let mut body = String::new();
    body.push_str(&sse(&json!({"type":"message_start","message":{"id":"msg_sc","role":"assistant","content":[],"model":"test","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}})));
    body.push_str(&sse(
        &json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
    ));
    body.push_str(&sse(
        &json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
    ));
    body.push_str(&sse(&json!({"type":"content_block_stop","index":0})));
    body.push_str(&sse(&json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"input_tokens":10,"output_tokens":10}})));
    body.push_str(&sse(&json!({"type":"message_stop"})));
    body
}

fn anthropic_tool_use_sse(tool_id: &str, tool_name: &str, tool_input: serde_json::Value) -> String {
    let mut body = String::new();
    body.push_str(&sse(&json!({"type":"message_start","message":{"id":"msg_sc","role":"assistant","content":[],"model":"test","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}})));
    body.push_str(&sse(&json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":tool_id,"name":tool_name,"input":{}}})));
    body.push_str(&sse(&json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":tool_input.to_string()}})));
    body.push_str(&sse(&json!({"type":"content_block_stop","index":0})));
    body.push_str(&sse(&json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":10,"output_tokens":15}})));
    body.push_str(&sse(&json!({"type":"message_stop"})));
    body
}

// --- OpenAI format ---

fn openai_text_sse(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "data: {{\"id\":\"chatcmpl-sc\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"test\",\"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"assistant\",\"content\":\"{escaped}\"}},\"finish_reason\":null}}]}}\n\n\
         data: {{\"id\":\"chatcmpl-sc\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"test\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n\
         data: [DONE]\n\n"
    )
}

fn openai_tool_use_sse(tool_id: &str, tool_name: &str, tool_input: serde_json::Value) -> String {
    let args = tool_input.to_string();
    let escaped_args = args.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "data: {{\"id\":\"chatcmpl-sc\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"test\",\"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"assistant\",\"content\":null,\"tool_calls\":[{{\"index\":0,\"id\":\"{tool_id}\",\"type\":\"function\",\"function\":{{\"name\":\"{tool_name}\",\"arguments\":\"{escaped_args}\"}}}}]}},\"finish_reason\":null}}]}}\n\n\
         data: {{\"id\":\"chatcmpl-sc\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"test\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"tool_calls\"}}]}}\n\n\
         data: [DONE]\n\n"
    )
}

// --- Ollama format (NDJSON) ---

fn ollama_text_ndjson(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "{{\"message\":{{\"role\":\"assistant\",\"content\":\"{escaped}\"}},\"model\":\"test\",\"done\":false}}\n\
         {{\"message\":{{\"role\":\"assistant\",\"content\":\"\"}},\"model\":\"test\",\"done\":true}}\n"
    )
}

fn ollama_tool_use_ndjson(tool_name: &str, tool_input: serde_json::Value) -> String {
    let args = tool_input.to_string();
    format!(
        "{{\"message\":{{\"role\":\"assistant\",\"content\":\"\",\"tool_calls\":[{{\"function\":{{\"name\":\"{tool_name}\",\"arguments\":{args}}}}}] }},\"model\":\"test\",\"done\":false}}\n\
         {{\"message\":{{\"role\":\"assistant\",\"content\":\"\"}},\"model\":\"test\",\"done\":true}}\n"
    )
}

fn sse(data: &serde_json::Value) -> String {
    format!("data: {data}\n\n")
}

// ── Helpers ────────────────────────────────────────────────────────────

fn create_workspace(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("shannon-provider-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create workspace");
    dir
}

fn cleanup_workspace(dir: &std::path::PathBuf) {
    let _ = fs::remove_dir_all(dir);
}

fn stdout_string(output: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&output.get_output().stdout).to_string()
}

fn parse_json_output(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout).unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\nError: {e}"))
}

fn shannon_anthropic(server_url: &str, workspace: &std::path::PathBuf) -> Command {
    let mut cmd = shannon();
    cmd.env("SHANNON_BASE_URL", server_url)
        .env("SHANNON_PROVIDER", "anthropic")
        .env("SHANNON_MODEL", "test-model")
        .env("SHANNON_API_KEY", "test-key")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .current_dir(workspace);
    cmd
}

fn shannon_openai(server_url: &str, workspace: &std::path::PathBuf) -> Command {
    let mut cmd = shannon();
    cmd.env("SHANNON_BASE_URL", server_url)
        .env("SHANNON_PROVIDER", "openai")
        .env("SHANNON_MODEL", "test-model")
        .env("SHANNON_API_KEY", "test-key")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .current_dir(workspace);
    cmd
}

fn shannon_ollama(server_url: &str, workspace: &std::path::PathBuf) -> Command {
    let mut cmd = shannon();
    cmd.env("SHANNON_BASE_URL", server_url)
        .env("SHANNON_PROVIDER", "ollama")
        .env("SHANNON_MODEL", "test-model")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("SHANNON_API_KEY")
        .current_dir(workspace);
    cmd
}

// ════════════════════════════════════════════════════════════════════════
// Scenario 1: Text-only response across all providers
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn scenario_anthropic_text_only() {
    let workspace = create_workspace("a_text");
    let mut server = mockito::Server::new_async().await;

    let _m = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_sse("Hello from Anthropic!"))
        .expect(1)
        .create();

    let result = shannon_anthropic(&server.url(), &workspace)
        .args(["--prompt", "Say hello", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    assert!(
        json["response"]
            .as_str()
            .unwrap_or("")
            .contains("Anthropic"),
        "Response should contain 'Anthropic'"
    );

    cleanup_workspace(&workspace);
}

#[tokio::test]
#[serial]
async fn scenario_openai_text_only() {
    let workspace = create_workspace("o_text");
    let mut server = mockito::Server::new_async().await;

    let _m = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(&openai_text_sse("Hello from OpenAI!"))
        .expect(1)
        .create();

    let result = shannon_openai(&server.url(), &workspace)
        .args(["--prompt", "Say hello", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    assert!(
        json["response"].as_str().unwrap_or("").contains("OpenAI"),
        "Response should contain 'OpenAI'"
    );

    cleanup_workspace(&workspace);
}

#[tokio::test]
#[serial]
async fn scenario_ollama_text_only() {
    let workspace = create_workspace("ol_text");
    let mut server = mockito::Server::new_async().await;

    let _m = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(&ollama_text_ndjson("Hello from Ollama!"))
        .expect(1)
        .create();

    let result = shannon_ollama(&server.url(), &workspace)
        .args(["--prompt", "Say hello", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    assert!(
        json["response"].as_str().unwrap_or("").contains("Ollama"),
        "Response should contain 'Ollama'"
    );

    cleanup_workspace(&workspace);
}

// ════════════════════════════════════════════════════════════════════════
// Scenario 2: Write tool_use across all providers
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn scenario_anthropic_write_tool() {
    let workspace = create_workspace("a_write");
    let file_path = workspace.join("out.txt");
    fs::write(&file_path, "").unwrap();

    let mut server = mockito::Server::new_async().await;

    let write_input = json!({"file_path": "out.txt", "content": "hello anthropic"});

    let _m1 = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_tool_use_sse("toolu_1", "Write", write_input))
        .expect(1)
        .create();

    let _m2 = server
        .mock("POST", "/v1/messages")
        .match_body(mockito::Matcher::Regex(r#"tool_result"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_sse("File written."))
        .expect(1)
        .create();

    let result = shannon_anthropic(&server.url(), &workspace)
        .args([
            "--prompt",
            "Write 'hello anthropic' to out.txt",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let content = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(content, "hello anthropic");

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");

    cleanup_workspace(&workspace);
}

#[tokio::test]
#[serial]
async fn scenario_openai_write_tool() {
    let workspace = create_workspace("o_write");
    let file_path = workspace.join("out.txt");
    fs::write(&file_path, "").unwrap();

    let mut server = mockito::Server::new_async().await;

    let write_input = json!({"file_path": "out.txt", "content": "hello openai"});

    // Turn 1: tool_use
    let _m1 = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(&openai_tool_use_sse("call_1", "Write", write_input))
        .expect(1)
        .create();

    // Turn 2: text after tool
    let _m2 = server
        .mock("POST", "/v1/chat/completions")
        .match_body(mockito::Matcher::Regex(r#"tool"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(&openai_text_sse("File written successfully."))
        .expect(1)
        .create();

    let result = shannon_openai(&server.url(), &workspace)
        .args([
            "--prompt",
            "Write 'hello openai' to out.txt",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let content = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(content, "hello openai");

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");

    cleanup_workspace(&workspace);
}

#[tokio::test]
#[serial]
async fn scenario_ollama_write_tool() {
    let workspace = create_workspace("ol_write");
    let file_path = workspace.join("out.txt");
    fs::write(&file_path, "").unwrap();

    let mut server = mockito::Server::new_async().await;

    let write_input = json!({"file_path": "out.txt", "content": "hello ollama"});

    // Turn 1: tool_use
    let _m1 = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(&ollama_tool_use_ndjson("Write", write_input))
        .expect(1)
        .create();

    // Turn 2: text after tool
    let _m2 = server
        .mock("POST", "/api/chat")
        .match_body(mockito::Matcher::Regex(r#"tool"#.to_string()))
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(&ollama_text_ndjson("File written."))
        .expect(1)
        .create();

    let result = shannon_ollama(&server.url(), &workspace)
        .args([
            "--prompt",
            "Write 'hello ollama' to out.txt",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let content = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(content, "hello ollama");

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");

    cleanup_workspace(&workspace);
}

// ════════════════════════════════════════════════════════════════════════
// Scenario 3: Bash tool_use across all providers
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn scenario_anthropic_bash_tool() {
    let workspace = create_workspace("a_bash");
    let mut server = mockito::Server::new_async().await;

    let bash_input = json!({"command": "echo scenario_anthropic"});

    let _m1 = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_tool_use_sse("toolu_1", "Bash", bash_input))
        .expect(1)
        .create();

    let _m2 = server
        .mock("POST", "/v1/messages")
        .match_body(mockito::Matcher::Regex(r#"tool_result"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_sse("Command executed."))
        .expect(1)
        .create();

    let result = shannon_anthropic(&server.url(), &workspace)
        .args([
            "--prompt",
            "Run echo scenario_anthropic",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    let tool_calls = json["tool_calls"].as_array().expect("tool_calls array");
    assert!(
        tool_calls.iter().any(|tc| tc["tool"] == "Bash"),
        "Should have a Bash tool call"
    );

    cleanup_workspace(&workspace);
}

#[tokio::test]
#[serial]
async fn scenario_openai_bash_tool() {
    let workspace = create_workspace("o_bash");
    let mut server = mockito::Server::new_async().await;

    let bash_input = json!({"command": "echo scenario_openai"});

    let _m1 = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(&openai_tool_use_sse("call_1", "Bash", bash_input))
        .expect(1)
        .create();

    let _m2 = server
        .mock("POST", "/v1/chat/completions")
        .match_body(mockito::Matcher::Regex(r#"tool"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(&openai_text_sse("Command completed."))
        .expect(1)
        .create();

    let result = shannon_openai(&server.url(), &workspace)
        .args([
            "--prompt",
            "Run echo scenario_openai",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    let tool_calls = json["tool_calls"].as_array().expect("tool_calls array");
    assert!(
        tool_calls.iter().any(|tc| tc["tool"] == "Bash"),
        "Should have a Bash tool call"
    );

    cleanup_workspace(&workspace);
}

#[tokio::test]
#[serial]
async fn scenario_ollama_bash_tool() {
    let workspace = create_workspace("ol_bash");
    let mut server = mockito::Server::new_async().await;

    let bash_input = json!({"command": "echo scenario_ollama"});

    let _m1 = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(&ollama_tool_use_ndjson("Bash", bash_input))
        .expect(1)
        .create();

    let _m2 = server
        .mock("POST", "/api/chat")
        .match_body(mockito::Matcher::Regex(r#"tool"#.to_string()))
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(&ollama_text_ndjson("Command ran."))
        .expect(1)
        .create();

    let result = shannon_ollama(&server.url(), &workspace)
        .args([
            "--prompt",
            "Run echo scenario_ollama",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    let tool_calls = json["tool_calls"].as_array().expect("tool_calls array");
    assert!(
        tool_calls.iter().any(|tc| tc["tool"] == "Bash"),
        "Should have a Bash tool call"
    );

    cleanup_workspace(&workspace);
}
