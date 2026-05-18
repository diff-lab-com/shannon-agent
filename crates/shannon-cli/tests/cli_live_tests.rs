//! Live LLM integration tests for shannon-cli.
//!
//! These tests hit a real LLM backend and are #[ignore]d by default.
//! To run:
//!   SHANNON_RUN_LIVE_TESTS=1 cargo test --test cli_live_tests -- --ignored
//!
//! Prerequisites:
//!   - Ollama running locally (default http://localhost:11434)
//!   - A model pulled (e.g. `ollama pull qwen2.5:0.5b`)
//!
//! For DeepSeek live tests (optional):
//!   SHANNON_DEEPSEEK_API_KEY=your-key SHANNON_RUN_LIVE_TESTS=1 cargo test --test cli_live_tests -- test_live_deepseek --ignored

use assert_cmd::Command;
const BIN: &str = "shannon";

fn shannon() -> Command {
    Command::cargo_bin(BIN).unwrap()
}

/// Guard that ensures live tests only run when explicitly opted in.
fn require_live_tests() {
    if std::env::var("SHANNON_RUN_LIVE_TESTS").as_deref() != Ok("1") {
        eprintln!("Skipping live test: set SHANNON_RUN_LIVE_TESTS=1 to run");
        std::process::exit(0);
    }
}

fn shannon_live_ollama() -> Command {
    let mut cmd = shannon();
    cmd.env("SHANNON_PROVIDER", "ollama")
        .env("SHANNON_MODEL", "qwen2.5:0.5b")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("SHANNON_API_KEY")
        .current_dir(std::env::temp_dir());
    cmd
}

fn shannon_live_deepseek() -> Option<Command> {
    let api_key = std::env::var("SHANNON_DEEPSEEK_API_KEY").ok()?;
    if api_key.is_empty() {
        return None;
    }
    let mut cmd = shannon();
    cmd.env("SHANNON_PROVIDER", "deepseek")
        .env("SHANNON_MODEL", "deepseek-chat")
        .env("SHANNON_API_KEY", &api_key)
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .current_dir(std::env::temp_dir());
    Some(cmd)
}

fn stdout_string(output: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&output.get_output().stdout).to_string()
}

// ════════════════════════════════════════════════════════════════════════
// Test: Live Ollama queries
// ════════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn test_live_ollama_simple_query() {
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "Say exactly: hello world", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON output:\n{stdout}\nParse error: {e}"));

    assert_eq!(json["exit_code"], "success", "Expected success exit code, got: {json}");
    let response = json["response"].as_str().unwrap_or("");
    assert!(!response.is_empty(), "Response should not be empty");
}

#[test]
#[ignore]
fn test_live_ollama_streaming() {
    require_live_tests();

    shannon_live_ollama()
        .args(["--prompt", "Say exactly: streaming works", "--output-format", "text"])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
}

#[test]
#[ignore]
fn test_live_ollama_exit_code_success() {
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "What is 1+1?", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    assert!(
        result.get_output().status.success(),
        "Simple query should exit 0"
    );
}

#[test]
#[ignore]
fn test_live_ollama_model_not_found() {
    require_live_tests();

    let mut cmd = shannon();
    cmd.env("SHANNON_PROVIDER", "ollama")
        .env("SHANNON_MODEL", "nonexistent-model-xyz-123")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("SHANNON_API_KEY")
        .current_dir(std::env::temp_dir());

    let result = cmd
        .args(["--prompt", "test", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    assert!(
        !result.get_output().status.success(),
        "Non-existent model should produce non-zero exit code"
    );
}

#[test]
#[ignore]
fn test_live_headless_json_structure() {
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "Say: test", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON output:\n{stdout}\nParse error: {e}"));

    for field in &["prompt", "response", "tool_calls", "total_tokens", "duration_ms", "exit_code"] {
        assert!(
            json.get(*field).is_some(),
            "Missing required field '{field}' in JSON output"
        );
    }

    let tokens = json["total_tokens"].as_u64().unwrap_or(0);
    assert!(tokens > 0, "total_tokens should be > 0 for a live response, got: {tokens}");
}

// ════════════════════════════════════════════════════════════════════════
// Test: Live Ollama context and multi-turn
// ════════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn test_live_ollama_prompt_preserved() {
    // Verify the prompt is preserved exactly in the JSON output
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "What is the capital of France?", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    let prompt = json["prompt"].as_str().unwrap_or("");
    assert!(
        prompt.contains("capital of France"),
        "Prompt should be preserved, got: {prompt}"
    );
}

#[test]
#[ignore]
fn test_live_ollama_json_stream_events() {
    // Verify json-stream produces valid NDJSON with expected event types
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "Say: hello", "--output-format", "json-stream"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    assert!(!events.is_empty(), "Should produce events");
    assert_eq!(events[0]["type"], "start", "First event should be 'start'");

    let has_done = events.iter().any(|e| e["type"] == "done");
    assert!(has_done, "Should have 'done' event");
}

#[test]
#[ignore]
fn test_live_ollama_duration_positive() {
    // Verify duration_ms is reported and positive
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "Count to 5", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    let duration = json["duration_ms"].as_u64().unwrap_or(0);
    assert!(duration > 0, "duration_ms should be > 0, got: {duration}");
}

#[test]
#[ignore]
fn test_live_ollama_nonempty_response() {
    // Verify response content is non-empty for a simple factual query
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "What is 2+2?", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    let response = json["response"].as_str().unwrap_or("");
    assert!(!response.is_empty(), "Response should not be empty");
    // Response should contain "4" somewhere
    assert!(
        response.contains('4'),
        "Response to 2+2 should contain '4', got: {response}"
    );
}

// ════════════════════════════════════════════════════════════════════════
// Test: Live DeepSeek queries (optional — requires SHANNON_DEEPSEEK_API_KEY)
// ════════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn test_live_deepseek_simple_query() {
    require_live_tests();
    let mut cmd = match shannon_live_deepseek() {
        Some(cmd) => cmd,
        None => {
            eprintln!("Skipping: set SHANNON_DEEPSEEK_API_KEY to run DeepSeek live tests");
            return;
        }
    };

    let result = cmd
        .args(["--prompt", "Say exactly: deepseek works", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    assert_eq!(json["exit_code"], "success", "Expected success, got: {json}");
    assert!(!json["response"].as_str().unwrap_or("").is_empty());
}

#[test]
#[ignore]
fn test_live_deepseek_streaming() {
    require_live_tests();
    let mut cmd = match shannon_live_deepseek() {
        Some(cmd) => cmd,
        None => {
            eprintln!("Skipping: set SHANNON_DEEPSEEK_API_KEY to run DeepSeek live tests");
            return;
        }
    };

    cmd.args(["--prompt", "Say: streaming test", "--output-format", "text"])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
}

#[test]
#[ignore]
fn test_live_deepseek_json_structure() {
    require_live_tests();
    let mut cmd = match shannon_live_deepseek() {
        Some(cmd) => cmd,
        None => {
            eprintln!("Skipping: set SHANNON_DEEPSEEK_API_KEY to run DeepSeek live tests");
            return;
        }
    };

    let result = cmd
        .args(["--prompt", "What is 1+1?", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    for field in &["prompt", "response", "tool_calls", "total_tokens", "duration_ms", "exit_code"] {
        assert!(
            json.get(*field).is_some(),
            "DeepSeek missing field '{field}'"
        );
    }
}

// ════════════════════════════════════════════════════════════════════════
// Test: Live context integrity
// ════════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn test_live_ollama_context_relevance() {
    // Verify the response is topically relevant to the prompt
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "List three colors", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    let response = json["response"].as_str().unwrap_or("").to_lowercase();
    // At least one common color should appear in the response
    let has_color = ["red", "blue", "green", "yellow", "black", "white", "orange", "purple"]
        .iter()
        .any(|c| response.contains(c));
    assert!(
        has_color,
        "Response about colors should mention at least one color, got: {response}"
    );
}

#[test]
#[ignore]
fn test_live_ollama_tool_calls_empty_by_default() {
    // Without tools enabled, tool_calls should be an empty array
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "Hello", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    let tool_calls = json["tool_calls"].as_array();
    assert!(tool_calls.is_some(), "tool_calls should be an array");
    assert!(
        tool_calls.unwrap().is_empty(),
        "Simple query without tools should have empty tool_calls"
    );
}
