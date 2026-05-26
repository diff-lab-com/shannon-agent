//! CLI mock tests for tool pipeline and provider scenarios.
//!
//! Uses mockito to simulate LLM responses, assert_cmd to run the shannon binary.
//! No API key needed.
//!
//! Two test sections:
//!   1. Tool Pipeline — verifies individual tool execution (Write, Bash, Edit, Glob, Grep, Read)
//!      and multi-step workflows through the headless CLI pipeline.
//!   2. Provider Scenarios — validates the same tool-use scenarios across Anthropic, OpenAI,
//!      and Ollama to ensure consistent behavior regardless of provider response format.
//!
//! Run with: cargo test --test cli_mock_tests -- --test-threads=1

use assert_cmd::Command;
use mockito::Matcher;
use serde_json::json;
use serial_test::serial;
use std::fs;
use std::path::PathBuf;

const BIN: &str = "shannon";

fn shannon() -> Command {
    Command::cargo_bin(BIN).unwrap()
}

// ── SSE Response Builders (using serde_json for correct JSON) ─────────

fn sse_line(data: &serde_json::Value) -> String {
    format!("data: {data}\n\n")
}

/// Build an Anthropic SSE response with a tool_use block.
fn anthropic_tool_use_sse(tool_id: &str, tool_name: &str, tool_input: serde_json::Value) -> String {
    let mut body = String::new();
    body.push_str(&sse_line(&json!({
        "type": "message_start",
        "message": {
            "id": "msg_test", "role": "assistant", "content": [],
            "model": "test-model", "stop_reason": null,
            "usage": {"input_tokens": 50, "output_tokens": 0}
        }
    })));
    body.push_str(&sse_line(&json!({
        "type": "content_block_start", "index": 0,
        "content_block": {"type": "tool_use", "id": tool_id, "name": tool_name, "input": {}}
    })));
    body.push_str(&sse_line(&json!({
        "type": "content_block_delta", "index": 0,
        "delta": {"type": "input_json_delta", "partial_json": tool_input.to_string()}
    })));
    body.push_str(&sse_line(
        &json!({"type": "content_block_stop", "index": 0}),
    ));
    body.push_str(&sse_line(&json!({
        "type": "message_delta",
        "delta": {"stop_reason": "tool_use"},
        "usage": {"input_tokens": 50, "output_tokens": 20}
    })));
    body.push_str(&sse_line(&json!({"type": "message_stop"})));
    body
}

/// Build an Anthropic SSE response with text + tool_use blocks.
fn anthropic_text_and_tool_sse(
    text: &str,
    tool_id: &str,
    tool_name: &str,
    tool_input: serde_json::Value,
) -> String {
    let mut body = String::new();
    body.push_str(&sse_line(&json!({
        "type": "message_start",
        "message": {
            "id": "msg_test", "role": "assistant", "content": [],
            "model": "test-model", "stop_reason": null,
            "usage": {"input_tokens": 50, "output_tokens": 0}
        }
    })));
    body.push_str(&sse_line(&json!({
        "type": "content_block_start", "index": 0,
        "content_block": {"type": "text", "text": ""}
    })));
    body.push_str(&sse_line(&json!({
        "type": "content_block_delta", "index": 0,
        "delta": {"type": "text_delta", "text": text}
    })));
    body.push_str(&sse_line(
        &json!({"type": "content_block_stop", "index": 0}),
    ));
    body.push_str(&sse_line(&json!({
        "type": "content_block_start", "index": 1,
        "content_block": {"type": "tool_use", "id": tool_id, "name": tool_name, "input": {}}
    })));
    body.push_str(&sse_line(&json!({
        "type": "content_block_delta", "index": 1,
        "delta": {"type": "input_json_delta", "partial_json": tool_input.to_string()}
    })));
    body.push_str(&sse_line(
        &json!({"type": "content_block_stop", "index": 1}),
    ));
    body.push_str(&sse_line(&json!({
        "type": "message_delta",
        "delta": {"stop_reason": "tool_use"},
        "usage": {"input_tokens": 50, "output_tokens": 30}
    })));
    body.push_str(&sse_line(&json!({"type": "message_stop"})));
    body
}

/// Build an Anthropic SSE response with text only (end_turn).
fn anthropic_text_sse(text: &str) -> String {
    let mut body = String::new();
    body.push_str(&sse_line(&json!({
        "type": "message_start",
        "message": {
            "id": "msg_test", "role": "assistant", "content": [],
            "model": "test-model", "stop_reason": null,
            "usage": {"input_tokens": 50, "output_tokens": 0}
        }
    })));
    body.push_str(&sse_line(&json!({
        "type": "content_block_start", "index": 0,
        "content_block": {"type": "text", "text": ""}
    })));
    body.push_str(&sse_line(&json!({
        "type": "content_block_delta", "index": 0,
        "delta": {"type": "text_delta", "text": text}
    })));
    body.push_str(&sse_line(
        &json!({"type": "content_block_stop", "index": 0}),
    ));
    body.push_str(&sse_line(&json!({
        "type": "message_delta",
        "delta": {"stop_reason": "end_turn"},
        "usage": {"input_tokens": 50, "output_tokens": 15}
    })));
    body.push_str(&sse_line(&json!({"type": "message_stop"})));
    body
}

/// Build an Anthropic SSE response with two tool_use blocks (multi-tool).
fn anthropic_multi_tool_sse(
    tool1_id: &str,
    tool1_name: &str,
    tool1_input: serde_json::Value,
    tool2_id: &str,
    tool2_name: &str,
    tool2_input: serde_json::Value,
) -> String {
    let mut body = String::new();
    body.push_str(&sse_line(&json!({
        "type": "message_start",
        "message": {
            "id": "msg_test", "role": "assistant", "content": [],
            "model": "test-model", "stop_reason": null,
            "usage": {"input_tokens": 50, "output_tokens": 0}
        }
    })));
    body.push_str(&sse_line(&json!({
        "type": "content_block_start", "index": 0,
        "content_block": {"type": "tool_use", "id": tool1_id, "name": tool1_name, "input": {}}
    })));
    body.push_str(&sse_line(&json!({
        "type": "content_block_delta", "index": 0,
        "delta": {"type": "input_json_delta", "partial_json": tool1_input.to_string()}
    })));
    body.push_str(&sse_line(
        &json!({"type": "content_block_stop", "index": 0}),
    ));
    body.push_str(&sse_line(&json!({
        "type": "content_block_start", "index": 1,
        "content_block": {"type": "tool_use", "id": tool2_id, "name": tool2_name, "input": {}}
    })));
    body.push_str(&sse_line(&json!({
        "type": "content_block_delta", "index": 1,
        "delta": {"type": "input_json_delta", "partial_json": tool2_input.to_string()}
    })));
    body.push_str(&sse_line(
        &json!({"type": "content_block_stop", "index": 1}),
    ));
    body.push_str(&sse_line(&json!({
        "type": "message_delta",
        "delta": {"stop_reason": "tool_use"},
        "usage": {"input_tokens": 50, "output_tokens": 20}
    })));
    body.push_str(&sse_line(&json!({"type": "message_stop"})));
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

// ── Mock Server Setup ──────────────────────────────────────────────────

/// Build a shannon command pointing to the mock server with Anthropic provider.
fn shannon_with_mock(server_url: &str, workspace: &PathBuf) -> Command {
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

fn shannon_openai(server_url: &str, workspace: &PathBuf) -> Command {
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

fn shannon_ollama(server_url: &str, workspace: &PathBuf) -> Command {
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

fn stdout_string(output: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&output.get_output().stdout).to_string()
}

fn parse_json_output(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON output:\n{stdout}\nParse error: {e}"))
}

// ── Helper: Create isolated workspace ──────────────────────────────────

fn create_workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shannon-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create workspace dir");
    dir
}

fn cleanup_workspace(dir: &PathBuf) {
    let _ = fs::remove_dir_all(dir);
}

// ── Mock helpers for multi-turn responses ───────────────────────────────

/// Mount a tool_use response (first call — no body matcher).
fn mount_tool_use(
    server: &mut mockito::ServerGuard,
    tool_id: &str,
    tool_name: &str,
    tool_input: serde_json::Value,
) -> mockito::Mock {
    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_tool_use_sse(tool_id, tool_name, tool_input))
        .expect(1)
        .create()
}

/// Mount a text response that matches requests containing "tool_result" (subsequent calls).
fn mount_text_after_tool(server: &mut mockito::ServerGuard, text: &str) -> mockito::Mock {
    server
        .mock("POST", "/v1/messages")
        .match_body(Matcher::Regex(r#"tool_result"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_sse(text))
        .expect(1)
        .create()
}

/// Mount a text+tool response that matches requests containing "tool_result".
#[allow(dead_code)]
fn mount_text_and_tool_after_tool(
    server: &mut mockito::ServerGuard,
    text: &str,
    tool_id: &str,
    tool_name: &str,
    tool_input: serde_json::Value,
) -> mockito::Mock {
    server
        .mock("POST", "/v1/messages")
        .match_body(Matcher::Regex(r#"tool_result"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_and_tool_sse(
            text, tool_id, tool_name, tool_input,
        ))
        .expect_at_most(2)
        .create()
}

/// Mount a final text response after tool execution. Matches "tool_result" to
/// avoid matching the initial (pre-tool) request.
#[allow(dead_code)]
fn mount_final_text(server: &mut mockito::ServerGuard, text: &str) -> mockito::Mock {
    server
        .mock("POST", "/v1/messages")
        .match_body(Matcher::Regex(r#"tool_result"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_sse(text))
        .expect(1)
        .create()
}

// ════════════════════════════════════════════════════════════════════════
// ── Tool Pipeline Tests ───────────────────────────────────────────────
// ════════════════════════════════════════════════════════════════════════

// ── Test: Write tool — create a file ──────────────────────────────────

#[tokio::test]
#[serial]
async fn test_task_write_file() {
    let workspace = create_workspace("write");
    let file_path = workspace.join("hello.txt");

    // Pre-create empty file so the path sandbox's canonicalize succeeds.
    // The Write tool will overwrite it with actual content.
    fs::write(&file_path, "").unwrap();

    let mut server = mockito::Server::new_async().await;

    // Use relative path — the workspace is the current dir, and the sandbox
    // resolves relative paths within the project directory.
    let tool_input = serde_json::json!({
        "file_path": "hello.txt",
        "content": "hello world"
    });

    let _m1 = mount_tool_use(&mut server, "toolu_1", "Write", tool_input);
    let _m2 = mount_text_after_tool(&mut server, "File created successfully.");

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "Create a file called hello.txt with content hello world",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    // Verify file was actually created
    assert!(file_path.exists(), "File should exist after tool execution");
    let content = fs::read_to_string(&file_path).expect("read created file");
    assert_eq!(
        content, "hello world",
        "File content should match tool input"
    );

    // Verify JSON output
    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    let tool_calls = json["tool_calls"]
        .as_array()
        .expect("tool_calls should be array");
    assert!(!tool_calls.is_empty(), "Should have tool calls");

    cleanup_workspace(&workspace);
}

// ── Test: Bash tool — execute a command ───────────────────────────────

#[tokio::test]
#[serial]
async fn test_task_bash_command() {
    let workspace = create_workspace("bash");

    let mut server = mockito::Server::new_async().await;

    let tool_input = serde_json::json!({
        "command": "echo task-test-output"
    });

    let _m1 = mount_tool_use(&mut server, "toolu_1", "Bash", tool_input);
    let _m2 = mount_text_after_tool(&mut server, "Command executed.");

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "Run the command echo task-test-output",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");

    // Tool result should contain the echo output somewhere in the response
    let combined = format!("{stdout}");
    assert!(
        combined.contains("task-test-output"),
        "Output should contain tool execution result, got: {combined}"
    );

    cleanup_workspace(&workspace);
}

// ── Test: Edit tool — modify an existing file ─────────────────────────

#[tokio::test]
#[serial]
async fn test_task_edit_file() {
    let workspace = create_workspace("edit");
    let file_path = workspace.join("config.toml");

    // Pre-create file with initial content
    fs::write(&file_path, "version = \"1.0\"\nname = \"test\"\n").unwrap();

    let mut server = mockito::Server::new_async().await;

    let edit_input = serde_json::json!({
        "file_path": "config.toml",
        "old_string": "version = \"1.0\"",
        "new_string": "version = \"2.0\""
    });

    let _m1 = mount_tool_use(&mut server, "toolu_1", "Edit", edit_input);
    let _m2 = mount_text_after_tool(&mut server, "Version updated to 2.0.");

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "Change the version from 1.0 to 2.0 in config.toml",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    // Verify file was edited
    let content = fs::read_to_string(&file_path).expect("read edited file");
    assert!(
        content.contains("version = \"2.0\""),
        "File should be updated to version 2.0, got: {content}"
    );
    assert!(
        !content.contains("version = \"1.0\""),
        "Old version should be replaced, got: {content}"
    );

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");

    cleanup_workspace(&workspace);
}

// ── Test: Multi-step — Edit then verify with Bash ─────────────────────

#[tokio::test]
#[serial]
async fn test_task_edit_then_verify_multi_step() {
    let workspace = create_workspace("multi");
    let file_path = workspace.join("config.toml");

    // Pre-create file
    fs::write(&file_path, "version = \"1.0\"\n").unwrap();

    let mut server = mockito::Server::new_async().await;

    // Turn 1: Edit tool_use
    let edit_input = serde_json::json!({
        "file_path": "config.toml",
        "old_string": "version = \"1.0\"",
        "new_string": "version = \"2.0\""
    });
    let _m1 = mount_tool_use(&mut server, "toolu_1", "Edit", edit_input);

    // Turn 2: text confirmation after edit
    let _m2 = mount_text_after_tool(&mut server, "Version updated to 2.0.");

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "Change version from 1.0 to 2.0 in config.toml",
            "--output-format",
            "json",
            "--max-turns",
            "5",
        ])
        .timeout(std::time::Duration::from_secs(45))
        .assert();

    // Verify file was edited
    let content = fs::read_to_string(&file_path).expect("read edited file");
    assert!(
        content.contains("version = \"2.0\""),
        "File should contain version 2.0, got: {content}"
    );

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    let tool_calls = json["tool_calls"].as_array().expect("tool_calls array");
    assert!(
        tool_calls.iter().any(|tc| tc["tool"] == "Edit"),
        "Should have an Edit tool call"
    );

    cleanup_workspace(&workspace);
}

// ── Test: Write + Bash in one session ─────────────────────────────────

#[tokio::test]
#[serial]
async fn test_task_write_then_verify() {
    let workspace = create_workspace("write_verify");
    let file_path = workspace.join("output.txt");

    // Pre-create empty file so the path sandbox's canonicalize succeeds.
    fs::write(&file_path, "").unwrap();

    let mut server = mockito::Server::new_async().await;

    // Create mocks in ORDER (FIFO): Write -> text+Bash -> text
    let write_input = serde_json::json!({
        "file_path": "output.txt",
        "content": "status: ok"
    });
    let _m1 = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_tool_use_sse("toolu_1", "Write", write_input))
        .expect(1)
        .create();

    let bash_input = serde_json::json!({
        "command": "cat output.txt"
    });
    let _m2 = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_and_tool_sse(
            "File written. Verifying...",
            "toolu_2",
            "Bash",
            bash_input,
        ))
        .expect(1)
        .create();

    let _m3 = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_sse("Verified: file contains status ok."))
        .expect(1)
        .create();

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "Create output.txt with content status ok then verify it with cat",
            "--output-format",
            "json",
            "--max-turns",
            "5",
        ])
        .timeout(std::time::Duration::from_secs(45))
        .assert();

    // Verify file exists and has correct content
    assert!(file_path.exists(), "File should exist");
    let content = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(content, "status: ok");

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");

    let tool_calls = json["tool_calls"].as_array().expect("tool_calls array");
    assert!(
        tool_calls.len() >= 1,
        "Should have at least 1 tool call, got: {tool_calls:?}"
    );

    cleanup_workspace(&workspace);
}

// ── Test: Tool error handling — Bash command that fails ───────────────

#[tokio::test]
#[serial]
async fn test_task_bash_error_recovery() {
    let workspace = create_workspace("bash_error");

    let mut server = mockito::Server::new_async().await;

    let bash_input = serde_json::json!({
        "command": "ls /nonexistent_directory_xyz_12345"
    });

    let _m1 = mount_tool_use(&mut server, "toolu_1", "Bash", bash_input);
    let _m2 = mount_text_after_tool(
        &mut server,
        "The directory does not exist. That's expected.",
    );

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "List files in /nonexistent_directory_xyz_12345",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    // Should still succeed — tool error is handled gracefully
    assert_eq!(json["exit_code"], "success");

    cleanup_workspace(&workspace);
}

// ── Test: Text-only response — no tool use ────────────────────────────

#[tokio::test]
#[serial]
async fn test_task_text_only_no_tools() {
    let workspace = create_workspace("text_only");

    let mut server = mockito::Server::new_async().await;

    let _m1 = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_sse("The answer is 42."))
        .expect(1)
        .create();

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "What is the meaning of life?",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    assert!(
        json["response"].as_str().unwrap_or("").contains("42"),
        "Response should contain the answer"
    );
    let tool_calls = json["tool_calls"].as_array().expect("tool_calls array");
    assert!(
        tool_calls.is_empty(),
        "Should have no tool calls for text-only response"
    );

    cleanup_workspace(&workspace);
}

// ── Test: Multi-step — Read then Edit (3 turns) ──────────────────────

#[tokio::test]
#[serial]
async fn test_task_read_then_edit() {
    let workspace = create_workspace("read_edit");
    let file_path = workspace.join("app.rs");

    // Pre-create file with initial content
    fs::write(&file_path, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

    let mut server = mockito::Server::new_async().await;

    // Turn 1: Read tool_use
    let _m1 = mount_tool_use(
        &mut server,
        "toolu_1",
        "Read",
        serde_json::json!({"file_path": "app.rs"}),
    );
    // Turn 2: Edit tool_use (body matcher "tool_result" to match after Read)
    let _m2 = server
        .mock("POST", "/v1/messages")
        .match_body(Matcher::Regex(r#"tool_result"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_tool_use_sse(
            "toolu_2",
            "Edit",
            serde_json::json!({
                "file_path": "app.rs",
                "old_string": "hello",
                "new_string": "world"
            }),
        ))
        .expect(1)
        .create();
    // Turn 3: text confirmation
    let _m3 = mount_final_text(&mut server, "File updated successfully.");

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "Read app.rs, then change hello to world",
            "--output-format",
            "json",
            "--max-turns",
            "5",
        ])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    // Verify file was edited
    let content = fs::read_to_string(&file_path).expect("read edited file");
    assert!(
        content.contains("world"),
        "File should contain 'world', got: {content}"
    );

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    let tool_calls = json["tool_calls"].as_array().expect("tool_calls array");
    assert!(
        tool_calls.iter().any(|tc| tc["tool"] == "Read"),
        "Should have a Read tool call"
    );
    assert!(
        tool_calls.iter().any(|tc| tc["tool"] == "Edit"),
        "Should have an Edit tool call"
    );

    cleanup_workspace(&workspace);
}

// ── Test: Glob tool — find files by pattern ───────────────────────────

#[tokio::test]
#[serial]
async fn test_task_glob_files() {
    let workspace = create_workspace("glob");

    // Create mixed files
    fs::write(workspace.join("main.rs"), "fn main() {}").unwrap();
    fs::write(workspace.join("lib.rs"), "pub fn lib() {}").unwrap();
    fs::write(workspace.join("cargo.toml"), "[package]").unwrap();

    let mut server = mockito::Server::new_async().await;

    let glob_input = serde_json::json!({
        "pattern": "**/*.rs"
    });
    let _m1 = mount_tool_use(&mut server, "toolu_1", "Glob", glob_input);
    let _m2 = mount_text_after_tool(&mut server, "Found 2 Rust source files.");

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "Find all .rs files in the workspace",
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
        tool_calls.iter().any(|tc| tc["tool"] == "Glob"),
        "Should have a Glob tool call"
    );

    cleanup_workspace(&workspace);
}

// ── Test: Grep tool — search for pattern in files ─────────────────────

#[tokio::test]
#[serial]
async fn test_task_grep_search() {
    let workspace = create_workspace("grep");

    fs::write(
        workspace.join("code.rs"),
        "fn process_data() {}\nfn handle_request() {}\n",
    )
    .unwrap();
    fs::write(workspace.join("other.rs"), "fn compute() {}\n").unwrap();

    let mut server = mockito::Server::new_async().await;

    let grep_input = serde_json::json!({
        "pattern": "fn process"
    });
    let _m1 = mount_tool_use(&mut server, "toolu_1", "Grep", grep_input);
    let _m2 = mount_text_after_tool(&mut server, "Found process_data function in code.rs.");

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "Search for 'fn process' in the codebase",
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
        tool_calls.iter().any(|tc| tc["tool"] == "Grep"),
        "Should have a Grep tool call"
    );

    cleanup_workspace(&workspace);
}

// ── Test: Multi-step — Grep then Read (3 turns) ──────────────────────

#[tokio::test]
#[serial]
async fn test_task_grep_then_read() {
    let workspace = create_workspace("grep_read");

    fs::write(
        workspace.join("mod.rs"),
        "pub fn calculate(x: i32) -> i32 {\n    x * 2\n}\n",
    )
    .unwrap();

    let mut server = mockito::Server::new_async().await;

    // Turn 1: Grep
    let _m1 = mount_tool_use(
        &mut server,
        "toolu_1",
        "Grep",
        serde_json::json!({ "pattern": "calculate" }),
    );
    // Turn 2: Read (body matcher for tool_result)
    let _m2 = server
        .mock("POST", "/v1/messages")
        .match_body(Matcher::Regex(r#"tool_result"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_tool_use_sse(
            "toolu_2",
            "Read",
            serde_json::json!({ "file_path": "mod.rs" }),
        ))
        .expect(1)
        .create();
    // Turn 3: text summary
    let _m3 = mount_final_text(&mut server, "The calculate function multiplies by 2.");

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "Search for 'calculate', then read the file containing it",
            "--output-format",
            "json",
            "--max-turns",
            "5",
        ])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    let tool_calls = json["tool_calls"].as_array().expect("tool_calls array");
    assert!(
        tool_calls.iter().any(|tc| tc["tool"] == "Grep"),
        "Should have a Grep tool call"
    );
    assert!(
        tool_calls.iter().any(|tc| tc["tool"] == "Read"),
        "Should have a Read tool call"
    );

    cleanup_workspace(&workspace);
}

// ── Test: Multi-step — Edit two files sequentially (3 turns) ──────────

#[tokio::test]
#[serial]
async fn test_task_multi_file_edit() {
    let workspace = create_workspace("multi_edit");

    fs::write(workspace.join("a.txt"), "alpha = 1\n").unwrap();
    fs::write(workspace.join("b.txt"), "beta = 1\n").unwrap();

    let mut server = mockito::Server::new_async().await;

    // Turn 1: Edit a.txt
    let _m1 = mount_tool_use(
        &mut server,
        "toolu_1",
        "Edit",
        serde_json::json!({
            "file_path": "a.txt",
            "old_string": "alpha = 1",
            "new_string": "alpha = 2"
        }),
    );
    // Turn 2: Edit b.txt (after tool_result)
    let _m2 = server
        .mock("POST", "/v1/messages")
        .match_body(Matcher::Regex(r#"tool_result"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_tool_use_sse(
            "toolu_2",
            "Edit",
            serde_json::json!({
                "file_path": "b.txt",
                "old_string": "beta = 1",
                "new_string": "beta = 2"
            }),
        ))
        .expect(1)
        .create();
    // Turn 3: text confirmation
    let _m3 = mount_final_text(&mut server, "Both files updated.");

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "Change alpha to 2 in a.txt and beta to 2 in b.txt",
            "--output-format",
            "json",
            "--max-turns",
            "5",
        ])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    // Verify both files edited
    let a = fs::read_to_string(workspace.join("a.txt")).expect("read a.txt");
    assert!(a.contains("alpha = 2"), "a.txt should be updated, got: {a}");
    let b = fs::read_to_string(workspace.join("b.txt")).expect("read b.txt");
    assert!(b.contains("beta = 2"), "b.txt should be updated, got: {b}");

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");

    cleanup_workspace(&workspace);
}

// ════════════════════════════════════════════════════════════════════════
// ── Provider Scenario Tests ───────────────────────────────────────────
// ════════════════════════════════════════════════════════════════════════

// ════════════════════════════════════════════════════════════════════════
// Scenario 1: Text-only response across all providers
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn scenario_anthropic_text_only() {
    let workspace = create_workspace("prov_a_text");
    let mut server = mockito::Server::new_async().await;

    let _m = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_sse("Hello from Anthropic!"))
        .expect(1)
        .create();

    let result = shannon_with_mock(&server.url(), &workspace)
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
    let workspace = create_workspace("prov_o_text");
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
    let workspace = create_workspace("prov_ol_text");
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
    let workspace = create_workspace("prov_a_write");
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
        .match_body(Matcher::Regex(r#"tool_result"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_sse("File written."))
        .expect(1)
        .create();

    let result = shannon_with_mock(&server.url(), &workspace)
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
    let workspace = create_workspace("prov_o_write");
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
        .match_body(Matcher::Regex(r#"tool"#.to_string()))
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
    let workspace = create_workspace("prov_ol_write");
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
        .match_body(Matcher::Regex(r#"tool"#.to_string()))
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
    let workspace = create_workspace("prov_a_bash");
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
        .match_body(Matcher::Regex(r#"tool_result"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_sse("Command executed."))
        .expect(1)
        .create();

    let result = shannon_with_mock(&server.url(), &workspace)
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
    let workspace = create_workspace("prov_o_bash");
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
        .match_body(Matcher::Regex(r#"tool"#.to_string()))
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
    let workspace = create_workspace("prov_ol_bash");
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
        .match_body(Matcher::Regex(r#"tool"#.to_string()))
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

// ════════════════════════════════════════════════════════════════════════
// Scenario 4: Read tool + text summary across all providers
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn scenario_anthropic_read_file() {
    let workspace = create_workspace("prov_a_read");
    let file_path = workspace.join("src.rs");
    fs::write(&file_path, "fn main() { println!(\"hello\"); }").unwrap();

    let mut server = mockito::Server::new_async().await;

    let read_input = json!({"file_path": "src.rs"});

    let _m1 = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_tool_use_sse("toolu_1", "Read", read_input))
        .expect(1)
        .create();

    let _m2 = server
        .mock("POST", "/v1/messages")
        .match_body(Matcher::Regex(r#"tool_result"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_sse("The file contains a main function."))
        .expect(1)
        .create();

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "Read src.rs and describe it",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    assert!(
        json["response"]
            .as_str()
            .unwrap_or("")
            .contains("main function"),
        "Response should describe the file content"
    );

    cleanup_workspace(&workspace);
}

#[tokio::test]
#[serial]
async fn scenario_openai_read_file() {
    let workspace = create_workspace("prov_o_read");
    let file_path = workspace.join("src.rs");
    fs::write(&file_path, "fn main() { println!(\"hello\"); }").unwrap();

    let mut server = mockito::Server::new_async().await;

    let read_input = json!({"file_path": "src.rs"});

    let _m1 = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(&openai_tool_use_sse("call_0", "Read", read_input))
        .expect(1)
        .create();

    let _m2 = server
        .mock("POST", "/v1/chat/completions")
        .match_body(Matcher::Regex(r#"tool"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(&openai_text_sse("The file contains a main function."))
        .expect(1)
        .create();

    let result = shannon_openai(&server.url(), &workspace)
        .args([
            "--prompt",
            "Read src.rs and describe it",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    assert!(
        json["response"]
            .as_str()
            .unwrap_or("")
            .contains("main function")
    );

    cleanup_workspace(&workspace);
}

#[tokio::test]
#[serial]
async fn scenario_ollama_read_file() {
    let workspace = create_workspace("prov_ol_read");
    let file_path = workspace.join("src.rs");
    fs::write(&file_path, "fn main() { println!(\"hello\"); }").unwrap();

    let mut server = mockito::Server::new_async().await;

    let read_input = json!({"file_path": "src.rs"});

    let _m1 = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(&ollama_tool_use_ndjson("Read", read_input))
        .expect(1)
        .create();

    let _m2 = server
        .mock("POST", "/api/chat")
        .match_body(Matcher::Regex(r#"tool"#.to_string()))
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(&ollama_text_ndjson("The file contains a main function."))
        .expect(1)
        .create();

    let result = shannon_ollama(&server.url(), &workspace)
        .args([
            "--prompt",
            "Read src.rs and describe it",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    assert!(
        json["response"]
            .as_str()
            .unwrap_or("")
            .contains("main function")
    );

    cleanup_workspace(&workspace);
}

// ════════════════════════════════════════════════════════════════════════
// Scenario 5: Multi-tool (Write + Bash) in one response
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[serial]
async fn scenario_anthropic_multi_tool() {
    let workspace = create_workspace("prov_a_multi");
    let file_path = workspace.join("out.txt");
    fs::write(&file_path, "").unwrap();

    let mut server = mockito::Server::new_async().await;

    let write_input = json!({"file_path": "out.txt", "content": "multi-tool works"});
    let bash_input = json!({"command": "echo parallel"});

    let _m1 = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_multi_tool_sse(
            "toolu_1",
            "Write",
            write_input,
            "toolu_2",
            "Bash",
            bash_input,
        ))
        .expect(1)
        .create();

    let _m2 = server
        .mock("POST", "/v1/messages")
        .match_body(Matcher::Regex(r#"tool_result"#.to_string()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_header("anthropic-version", "2023-06-01")
        .with_body(&anthropic_text_sse("Both operations completed."))
        .expect(1)
        .create();

    let result = shannon_with_mock(&server.url(), &workspace)
        .args([
            "--prompt",
            "Write 'multi-tool works' to out.txt and run echo parallel",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let content = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(content, "multi-tool works");

    let stdout = stdout_string(&result);
    let json = parse_json_output(&stdout);
    assert_eq!(json["exit_code"], "success");
    let tool_calls = json["tool_calls"].as_array().expect("tool_calls array");
    assert!(
        tool_calls.iter().any(|tc| tc["tool"] == "Write"),
        "Should have a Write tool call"
    );
    assert!(
        tool_calls.iter().any(|tc| tc["tool"] == "Bash"),
        "Should have a Bash tool call"
    );

    cleanup_workspace(&workspace);
}
