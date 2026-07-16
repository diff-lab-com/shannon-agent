//! Live API integration tests for shannon-cli.
//!
//! Real LLM provider tests (Ollama, DeepSeek, Anthropic) with record/replay support.
//! All real API tests are #[ignore]d by default.
//!
//! **Live Ollama/DeepSeek tests:**
//!   SHANNON_RUN_LIVE_TESTS=1 cargo test --test live_tests -- --ignored
//!
//!   Prerequisites:
//!   - Ollama running locally (default http://localhost:11434)
//!   - A model pulled (e.g. `ollama pull qwen2.5:0.5b`)
//!   - For DeepSeek: set SHANNON_DEEPSEEK_API_KEY
//!
//! **Recording mode** (local, needs API key):
//!   SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
//!   SHANNON_API_KEY=sk-... \
//!   cargo test --test live_tests -- --ignored --test-threads=1
//!
//! **Replay mode** (CI, no API key):
//!   cargo test --test live_tests -- --test-threads=1
//!
//! Recording uses LlmClient's built-in SHANNON_RECORD_DIR hook to capture
//! request/response pairs. Replay loads those fixtures via mockito.

use assert_cmd::Command;
use serial_test::serial;
use std::fs;
use std::path::PathBuf;

const BIN: &str = "shannon";

fn shannon() -> Command {
    Command::cargo_bin(BIN).unwrap()
}

fn stdout_string(output: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&output.get_output().stdout).to_string()
}

// ── Shared helpers ────────────────────────────────────────────────────────

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

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("real_tasks")
}

fn create_workspace(name: &str) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let subdir = dir.path().join(name);
    fs::create_dir_all(&subdir).expect("create subdir");
    dir
}

fn require_api_key() -> String {
    if let Ok(key) = std::env::var("SHANNON_API_KEY") {
        return key;
    }
    // Fallback to provider-specific key
    let provider = record_provider();
    if let Some(key_env) = provider_key_env(&provider) {
        if let Ok(key) = std::env::var(key_env) {
            return key;
        }
    }
    eprintln!("Skipping: set SHANNON_API_KEY to run recording tests");
    std::process::exit(0);
}

fn require_record_dir() -> PathBuf {
    match std::env::var("SHANNON_RECORD_DIR") {
        Ok(dir) => {
            let path = PathBuf::from(&dir);
            // Resolve relative paths against the project root (parent of CARGO_MANIFEST_DIR),
            // not the test CWD, so they match fixtures_dir().
            let resolved = if path.is_relative() {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join(&path)
            } else {
                path
            };
            let _ = fs::create_dir_all(&resolved);
            resolved.canonicalize().unwrap_or(resolved)
        }
        Err(_) => {
            eprintln!("Skipping: set SHANNON_RECORD_DIR to record fixtures");
            std::process::exit(0);
        }
    }
}

/// Recording provider: override via SHANNON_RECORD_PROVIDER (defaults to "anthropic").
/// Supports any OpenAI-compatible provider: anthropic, minimax, deepseek, openai, etc.
fn record_provider() -> String {
    std::env::var("SHANNON_RECORD_PROVIDER").unwrap_or_else(|_| "anthropic".to_string())
}

/// Model name for recording: SHANNON_MODEL, falls back to "unknown".
fn record_model() -> String {
    std::env::var("SHANNON_MODEL").unwrap_or_else(|_| "unknown".to_string())
}

/// Optional base URL override for recording: SHANNON_RECORD_BASE_URL.
/// Falls back to SHANNON_BASE_URL, then None (use provider default).
fn record_base_url() -> Option<String> {
    std::env::var("SHANNON_RECORD_BASE_URL")
        .ok()
        .or_else(|| std::env::var("SHANNON_BASE_URL").ok())
}

/// Provider-specific API key env var name.
fn provider_key_env(provider: &str) -> Option<&'static str> {
    match provider {
        "zhipu" | "zhipu-cn" | "zhipu-coding" | "zhipu-anthropic" => Some("ZHIPU_API_KEY"),
        "zhipu-intl" | "zhipu-international" => Some("ZHIPU_INTL_API_KEY"),
        "minimax" => Some("MINIMAX_API_KEY"),
        "moonshot" | "kimi" => Some("MOONSHOT_API_KEY"),
        "deepseek" => Some("DEEPSEEK_API_KEY"),
        "dashscope" | "qwen" => Some("DASHSCOPE_API_KEY"),
        _ => None,
    }
}

/// Create a shannon command with all recording env vars set.
/// `session_name` is used for JSONL-based recording (one file per test).
fn shannon_record(
    api_key: &str,
    record_dir: &PathBuf,
    workspace: &tempfile::TempDir,
    session_name: &str,
) -> Command {
    let provider = record_provider();
    let model = record_model();
    let qualified_session = format!("{provider}_{model}_{session_name}");
    // Clear any prior fixture for this exact (provider, model, session) tuple so
    // re-running the test produces a clean fixture. The recording engine's
    // JSONL output is append-mode by design (supports interrupted/resumed
    // recordings), but each test invocation uses a fresh temp workspace — so
    // exchanges from prior runs reference paths the new workspace doesn't
    // have, which silently pollutes replay fixtures and breaks downstream
    // workspace assertions.
    let fixture_path = record_dir.join(format!("{qualified_session}.jsonl"));
    let _ = fs::remove_file(&fixture_path);
    let mut cmd = shannon();
    cmd.env("SHANNON_API_KEY", api_key)
        .env("SHANNON_RECORD_DIR", record_dir)
        .env("SHANNON_RECORD_SESSION", &qualified_session)
        .env("SHANNON_PROVIDER", &provider)
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .env_remove("ANTHROPIC_BASE_URL")
        .env_remove("SHANNON_BASE_URL")
        .current_dir(workspace.path());
    if let Some(base_url) = record_base_url() {
        cmd.env("SHANNON_BASE_URL", base_url);
    }
    if let Some(key_env) = provider_key_env(&provider) {
        cmd.env(key_env, api_key);
    }
    cmd
}

/// Write a file, creating parent directories as needed.
fn write_file(path: &std::path::Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, content).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

// ── Replay helpers: drive the agent offline against recorded fixtures ─────
// Inverse of `shannon_record`: mounts recorded exchanges as mockito mocks and
// points shannon at the mock server. Implements ADR 0003 Phase 1 — minimal
// VCR replay for deterministic tasks (no API key, no cost).

use shannon_core::testing::record_replay::RecordedExchange;

/// Resolve a recorded fixture path for a (provider, model, session) tuple.
fn replay_fixture_path(provider: &str, model: &str, session: &str) -> PathBuf {
    fixtures_dir().join(format!("{provider}_{model}_{session}.jsonl"))
}

/// Extract the tempdir workspace path (e.g. `/tmp/.tmp0ZFV0J`) embedded in a
/// recorded request body. Shannon's system prompt includes the CWD; TempDir
/// uses a random name, so the recorded path differs from the replay path and
/// must be rewritten before exact-match comparison.
fn recorded_workspace_path(body: &str) -> Option<String> {
    const MARKER: &str = "/tmp/.tmp";
    let start = body.find(MARKER)?;
    let suffix: String = body[start + MARKER.len()..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    Some(format!("{MARKER}{suffix}"))
}

/// Rewrite the workspace-path suffix in `body` so that the recorded CWD's
/// random suffix becomes the replay CWD's random suffix. The function
/// finds the longest common prefix between `recorded_cwd` and
/// `replay_cwd` and replaces everything that differs. Both request and
/// response bodies go through this; SSE fragment streams get the suffix
/// rewritten in each fragment independently so the streaming parser
/// reassembles the correct replay path.
///
/// Returns the rewritten body unchanged if no `/tmp/.tmp` marker is
/// found in the body (i.e. `recorded_workspace_path` returns None).
fn rewrite_path_suffix(body: &str, recorded_cwd: &str, replay_cwd: &str) -> String {
    let common_len = recorded_cwd
        .chars()
        .zip(replay_cwd.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let recorded_suffix = &recorded_cwd[common_len..];
    let replay_suffix = &replay_cwd[common_len..];
    if !recorded_suffix.is_empty() {
        body.replace(recorded_suffix, replay_suffix)
    } else {
        body.to_string()
    }
}

/// Mount a single recorded exchange as a mockito mock.
///
/// Matches the recorded request body with its tempdir path rewritten to the
/// replay CWD (so a fresh TempDir produces an identical body). Replays status,
/// body, and `content-type`; transport headers (`transfer-encoding`, etc.) are
/// dropped to avoid conflicts with mockito's own framing.
fn mount_exchange(
    server: &mut mockito::ServerGuard,
    ex: &RecordedExchange,
    replay_cwd: &std::path::Path,
) -> mockito::Mock {
    let mut expected_body = ex.request.body.clone();
    let mut response_body = ex.response.body.clone();
    if let Some(recorded_cwd) = recorded_workspace_path(&ex.request.body) {
        let replay = replay_cwd.to_string_lossy();
        // Replace only the differing SUFFIX, not the full path. Both recorded
        // and replay tempdirs share the `/tmp/.tmp` prefix, so the variable
        // part is a short random tail. A full-path replace works on the
        // contiguous request body but FAILS on the response body: tool-call
        // arguments stream incrementally across SSE delta fragments, splitting
        // the path (e.g. fragment 1 ends `/tmp/.tmp`, fragment 2 starts with
        // the suffix). Replacing the suffix catches each fragment independently,
        // and the agent's streaming parser reassembles the correct replay path.
        // (ADR 0003)
        expected_body = rewrite_path_suffix(&ex.request.body, &recorded_cwd, &replay);
        response_body = rewrite_path_suffix(&ex.response.body, &recorded_cwd, &replay);
    }
    let mut mock = server
        .mock("POST", ex.request.path.as_str())
        .match_body(expected_body.as_str())
        .with_status(ex.response.status as usize);
    for (name, value) in &ex.response.headers {
        if name.eq_ignore_ascii_case("content-type") {
            mock = mock.with_header(name, value);
        }
    }
    mock.with_body(response_body).create()
}

/// Mount every exchange from a JSONL fixture file onto the server.
/// Returns the mock guards — keep them alive until the agent run finishes.
fn mount_fixture(
    server: &mut mockito::ServerGuard,
    path: &std::path::Path,
    replay_cwd: &std::path::Path,
) -> Vec<mockito::Mock> {
    let exchanges = RecordedExchange::load_jsonl(path)
        .unwrap_or_else(|e| panic!("load {}: {e}", path.display()));
    exchanges
        .iter()
        .map(|ex| mount_exchange(server, ex, replay_cwd))
        .collect()
}

/// Build a shannon command in REPLAY mode: pointed at the mock server with a
/// dummy API key, recording disabled. Mirrors `shannon_record`'s env hygiene.
fn shannon_replay(
    base_url: &str,
    provider: &str,
    model: &str,
    workspace: &tempfile::TempDir,
) -> Command {
    let mut cmd = shannon();
    cmd.env("SHANNON_BASE_URL", base_url)
        .env("SHANNON_API_KEY", "replay-fake-key")
        .env("SHANNON_PROVIDER", provider)
        .env("SHANNON_MODEL", model)
        .env_remove("SHANNON_RECORD_DIR")
        .env_remove("OPENAI_BASE_URL")
        .env_remove("ANTHROPIC_BASE_URL")
        .current_dir(workspace.path());
    if let Some(key_env) = provider_key_env(provider) {
        cmd.env(key_env, "replay-fake-key");
    }
    // NOTE: do NOT pass --yes. The recorded fixtures were captured under
    // FullAuto (recording uses `shannon --prompt` without --yes). Passing
    // --yes flips the agent to BypassPermissions, which changes the outgoing
    // request body (e.g. system-prompt permission instructions) and every
    // mock fails with http_501. The destructive-ops that need --yes (delete_file)
    // are excluded from the replay set for orthogonal reasons.
    cmd
}

// ══════════════════════════════════════════════════════════════════════════
// ── Live Provider Tests ──────────────────────────────────────────────────
// ══════════════════════════════════════════════════════════════════════════
// #[ignore] 原因: 需要运行中的 Ollama/DeepSeek 实例
// 目的: 验证真实 API 连通性、响应解析、流式行为
// 不适合 record/replay: 这些测试验证的是网络连通性，不是对话行为

// ── Live Ollama queries ──────────────────────────────────────────────────

#[serial]
#[test]
#[serial]
#[ignore] // Requires running Ollama instance with SHANNON_RUN_LIVE_TESTS=1
fn test_live_ollama_simple_query() {
    require_live_tests();

    let result = shannon_live_ollama()
        .args([
            "--prompt",
            "Say exactly: hello world",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON output:\n{stdout}\nParse error: {e}"));

    assert_eq!(
        json["exit_code"], "success",
        "Expected success exit code, got: {json}"
    );
    let response = json["response"].as_str().unwrap_or("");
    assert!(!response.is_empty(), "Response should not be empty");
}

#[serial]
#[test]
#[serial]
#[ignore] // Requires running Ollama instance with SHANNON_RUN_LIVE_TESTS=1
fn test_live_ollama_streaming() {
    require_live_tests();

    shannon_live_ollama()
        .args([
            "--prompt",
            "Say exactly: streaming works",
            "--output-format",
            "text",
        ])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
}

#[serial]
#[test]
#[serial]
#[ignore] // Requires running Ollama instance with SHANNON_RUN_LIVE_TESTS=1
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

#[serial]
#[test]
#[serial]
#[ignore] // Requires running Ollama instance with SHANNON_RUN_LIVE_TESTS=1
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

#[serial]
#[test]
#[serial]
#[ignore] // Requires running Ollama instance with SHANNON_RUN_LIVE_TESTS=1
fn test_live_headless_json_structure() {
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "Say: test", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON output:\n{stdout}\nParse error: {e}"));

    for field in &[
        "prompt",
        "response",
        "tool_calls",
        "total_tokens",
        "duration_ms",
        "exit_code",
    ] {
        assert!(
            json.get(*field).is_some(),
            "Missing required field '{field}' in JSON output"
        );
    }

    let tokens = json["total_tokens"].as_u64().unwrap_or(0);
    assert!(
        tokens > 0,
        "total_tokens should be > 0 for a live response, got: {tokens}"
    );
}

// ── Live Ollama context and multi-turn ────────────────────────────────────

#[serial]
#[test]
#[serial]
#[ignore] // Requires running Ollama instance with SHANNON_RUN_LIVE_TESTS=1
fn test_live_ollama_prompt_preserved() {
    // Verify the prompt is preserved exactly in the JSON output
    require_live_tests();

    let result = shannon_live_ollama()
        .args([
            "--prompt",
            "What is the capital of France?",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    let prompt = json["prompt"].as_str().unwrap_or("");
    assert!(
        prompt.contains("capital of France"),
        "Prompt should be preserved, got: {prompt}"
    );
}

#[serial]
#[test]
#[serial]
#[ignore] // Requires running Ollama instance with SHANNON_RUN_LIVE_TESTS=1
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

#[serial]
#[test]
#[serial]
#[ignore] // Requires running Ollama instance with SHANNON_RUN_LIVE_TESTS=1
fn test_live_ollama_duration_positive() {
    // Verify duration_ms is reported and positive
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "Count to 5", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    let duration = json["duration_ms"].as_u64().unwrap_or(0);
    assert!(duration > 0, "duration_ms should be > 0, got: {duration}");
}

#[serial]
#[test]
#[serial]
#[ignore] // Requires running Ollama instance with SHANNON_RUN_LIVE_TESTS=1
fn test_live_ollama_nonempty_response() {
    // Verify response content is non-empty for a simple factual query
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "What is 2+2?", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    let response = json["response"].as_str().unwrap_or("");
    assert!(!response.is_empty(), "Response should not be empty");
    // Response should contain "4" somewhere
    assert!(
        response.contains('4'),
        "Response to 2+2 should contain '4', got: {response}"
    );
}

// ── Live DeepSeek queries (optional — requires SHANNON_DEEPSEEK_API_KEY) ──

#[serial]
#[test]
#[serial]
#[ignore] // Requires SHANNON_DEEPSEEK_API_KEY and SHANNON_RUN_LIVE_TESTS=1
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
        .args([
            "--prompt",
            "Say exactly: deepseek works",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    assert_eq!(
        json["exit_code"], "success",
        "Expected success, got: {json}"
    );
    assert!(!json["response"].as_str().unwrap_or("").is_empty());
}

#[serial]
#[test]
#[serial]
#[ignore] // Requires SHANNON_DEEPSEEK_API_KEY and SHANNON_RUN_LIVE_TESTS=1
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

#[serial]
#[test]
#[serial]
#[ignore] // Requires SHANNON_DEEPSEEK_API_KEY and SHANNON_RUN_LIVE_TESTS=1
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
    let json: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    for field in &[
        "prompt",
        "response",
        "tool_calls",
        "total_tokens",
        "duration_ms",
        "exit_code",
    ] {
        assert!(
            json.get(*field).is_some(),
            "DeepSeek missing field '{field}'"
        );
    }
}

// ── Live context integrity ────────────────────────────────────────────────

#[serial]
#[test]
#[serial]
#[ignore] // Requires running Ollama instance with SHANNON_RUN_LIVE_TESTS=1
fn test_live_ollama_context_relevance() {
    // Verify the response is topically relevant to the prompt
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "List three colors", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    let response = json["response"].as_str().unwrap_or("").to_lowercase();
    // At least one common color should appear in the response
    let has_color = [
        "red", "blue", "green", "yellow", "black", "white", "orange", "purple",
    ]
    .iter()
    .any(|c| response.contains(c));
    assert!(
        has_color,
        "Response about colors should mention at least one color, got: {response}"
    );
}

#[serial]
#[test]
#[serial]
#[ignore] // Requires running Ollama instance with SHANNON_RUN_LIVE_TESTS=1
fn test_live_ollama_tool_calls_empty_by_default() {
    // Without tools enabled, tool_calls should be an empty array
    require_live_tests();

    let result = shannon_live_ollama()
        .args(["--prompt", "Hello", "--output-format", "json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert();

    let stdout = stdout_string(&result);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("Invalid JSON:\n{stdout}\n{e}"));

    let tool_calls = json["tool_calls"].as_array();
    assert!(tool_calls.is_some(), "tool_calls should be an array");
    assert!(
        tool_calls.unwrap().is_empty(),
        "Simple query without tools should have empty tool_calls"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// ── Record/Replay Tests ──────────────────────────────────────────────────
// ══════════════════════════════════════════════════════════════════════════
// #[ignore] 原因: 录制端需 API key (SHANNON_API_KEY + SHANNON_RECORD_DIR)
// 回放端不需要 key — 使用 `just replay` 运行
// 录制/回放机制: LlmClient 拦截请求/响应 → JSONL fixture → mockito mock

// ── Recording tests (require API key + SHANNON_RECORD_DIR, #[ignore]) ────

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_create_file() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_create_file");

    shannon_record(&api_key, &record_dir, &workspace, "create_file")
        .args([
            "--prompt",
            "Create a file called hello.txt with the content 'world'",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    // Verify the file was created
    assert!(
        workspace.path().join("hello.txt").exists(),
        "hello.txt should be created"
    );
    let content = fs::read_to_string(workspace.path().join("hello.txt")).unwrap();
    assert!(
        content.contains("world"),
        "hello.txt should contain 'world', got: {content}"
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_bash_command() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_bash_cmd");

    shannon_record(&api_key, &record_dir, &workspace, "bash_command")
        .args([
            "--prompt",
            "Run the command: echo hello_shannon > output.txt",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    assert!(
        workspace.path().join("output.txt").exists(),
        "output.txt should be created"
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_read_and_edit() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_read_edit");

    // Create a file to edit
    write_file(
        &workspace.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }",
    );

    shannon_record(&api_key, &record_dir, &workspace, "read_and_edit")
        .args([
            "--prompt",
            "Read src/lib.rs and add a doc comment above the add function",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    let content = fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap();
    assert!(
        content.contains("///") || content.contains("//"),
        "lib.rs should have a comment added, got: {content}"
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_code_search() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_code_search");

    write_file(
        &workspace.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }",
    );

    let output = shannon_record(&api_key, &record_dir, &workspace, "code_search")
        .args([
            "--prompt",
            "Find where the add function is defined",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        !stdout.is_empty(),
        "should produce output about the add function"
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_multi_turn() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_multi_turn");

    write_file(
        &workspace.path().join("src/main.rs"),
        "fn main() {\n    println!(\"Hello, World!\");\n}\n",
    );

    shannon_record(&api_key, &record_dir, &workspace, "multi_turn")
        .args([
            "--prompt",
            "Read src/main.rs, then change the greeting from 'Hello, World!' to 'Hello, Shannon!'",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    let content = fs::read_to_string(workspace.path().join("src/main.rs")).unwrap();
    assert!(
        content.contains("Shannon"),
        "main.rs should contain 'Shannon', got: {content}"
    );
}

// ── Tier 1: Core tool chain recordings ────────────────────────────────────

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_edit_precise_match() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_edit_precise");

    fs::write(
        workspace.path().join("config.toml"),
        "name = \"old_name\"\nversion = \"0.1.0\"\n",
    )
    .expect("write config.toml");

    shannon_record(&api_key, &record_dir, &workspace, "edit_precise_match")
        .args([
            "--prompt",
            "Read config.toml and change the name from 'old_name' to 'new_name' using an exact string replacement",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    let content = fs::read_to_string(workspace.path().join("config.toml")).unwrap();
    assert!(
        content.contains("new_name"),
        "config.toml should contain 'new_name', got: {content}"
    );
    assert!(
        !content.contains("old_name"),
        "config.toml should not contain 'old_name', got: {content}"
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_search_read_edit() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_search_read_edit");

    write_file(
        &workspace.path().join("src/lib.rs"),
        "pub fn calculate(x: i32, y: i32) -> i32 {\n    x + y\n}\n",
    );

    shannon_record(&api_key, &record_dir, &workspace, "search_read_edit")
        .args([
            "--prompt",
            "Search for 'calculate' in the codebase, read the file where it's defined, and rename the function to 'add_numbers'",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    let content = fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap();
    assert!(
        content.contains("add_numbers"),
        "lib.rs should contain 'add_numbers', got: {content}"
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_bash_verify() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_bash_verify");

    shannon_record(&api_key, &record_dir, &workspace, "bash_verify")
        .args([
            "--prompt",
            "Create a directory called 'build', then create a file build/output.txt with the content 'build successful', then verify the file exists by reading it",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_error_recovery() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_error_recovery");

    // Create a file with a syntax error for the LLM to find and fix
    write_file(
        &workspace.path().join("src/main.rs"),
        "fn main() {\n    let x = 1 + ;\n    println!(\"{}\", x);\n}\n",
    );

    shannon_record(&api_key, &record_dir, &workspace, "error_recovery")
        .args([
            "--prompt",
            "Read src/main.rs — it has a syntax error. Find and fix it so the code compiles correctly.",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    let content = fs::read_to_string(workspace.path().join("src/main.rs")).unwrap();
    // Should no longer have the broken "1 + ;" pattern
    assert!(
        !content.contains("1 + ;"),
        "main.rs should have the syntax error fixed, got: {content}"
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_glob_pattern() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_glob_pattern");

    // Create multiple files with different extensions
    fs::create_dir_all(workspace.path().join("src")).expect("create src");
    write_file(&workspace.path().join("src/main.rs"), "fn main() {}");
    write_file(&workspace.path().join("src/lib.rs"), "pub fn lib() {}");
    write_file(&workspace.path().join("src/utils.rs"), "pub fn utils() {}");
    fs::write(workspace.path().join("README.md"), "# test").expect("write README");

    shannon_record(&api_key, &record_dir, &workspace, "glob_pattern")
        .args([
            "--prompt",
            "Find all .rs files in the src/ directory using glob patterns, read each one, and add a comment '// documented' at the top of each file",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    for name in &["main.rs", "lib.rs", "utils.rs"] {
        let content = fs::read_to_string(workspace.path().join("src").join(name)).unwrap();
        assert!(
            content.contains("// documented") || content.contains("//documented"),
            "{name} should have a comment added, got: {content}"
        );
    }
}

// ── Tier 2: Multi-file / complex task recordings ──────────────────────────

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_multi_file_edit() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_multi_file");

    write_file(
        &workspace.path().join("src/types.rs"),
        "pub struct User {\n    pub name: String,\n    pub age: u32,\n}\n",
    );
    write_file(
        &workspace.path().join("src/api.rs"),
        "use crate::types::User;\n\npub fn get_user() -> User {\n    User { name: \"Alice\".into(), age: 30 }\n}\n",
    );
    write_file(
        &workspace.path().join("src/main.rs"),
        "use crate::api::get_user;\n\nfn main() {\n    let user = get_user();\n    println!(\"Name: {}\", user.name);\n}\n",
    );

    shannon_record(&api_key, &record_dir, &workspace, "multi_file_edit")
        .args([
            "--prompt",
            "Read all three source files (src/types.rs, src/api.rs, src/main.rs). Add an 'email: String' field to the User struct in types.rs, update the get_user() function in api.rs to include email, and update the println in main.rs to also print the email.",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(180))
        .assert()
        .success();

    let types = fs::read_to_string(workspace.path().join("src/types.rs")).unwrap();
    assert!(
        types.contains("email"),
        "types.rs should have email field, got: {types}"
    );
    let api = fs::read_to_string(workspace.path().join("src/api.rs")).unwrap();
    assert!(
        api.contains("email"),
        "api.rs should have email in get_user, got: {api}"
    );
    let main = fs::read_to_string(workspace.path().join("src/main.rs")).unwrap();
    assert!(
        main.contains("email"),
        "main.rs should print email, got: {main}"
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_refactor_rename() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_refactor_rename");

    write_file(
        &workspace.path().join("src/lib.rs"),
        "pub fn process_data(data: &str) -> String {\n    data.to_uppercase()\n}\n",
    );
    write_file(
        &workspace.path().join("src/main.rs"),
        "use crate::lib::process_data;\n\nfn main() {\n    let result = process_data(\"hello\");\n    println!(\"{}\", result);\n}\n",
    );

    shannon_record(&api_key, &record_dir, &workspace, "refactor_rename")
        .args([
            "--prompt",
            "Rename the function 'process_data' to 'transform_input' across all files. Make sure to update both the definition in src/lib.rs and all usages in src/main.rs.",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_create_with_tests() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_create_tests");

    shannon_record(&api_key, &record_dir, &workspace, "create_with_tests")
        .args([
            "--prompt",
            "Create a Rust library crate with cargo init. Then create src/math.rs with a function 'pub fn multiply(a: i32, b: i32) -> i32' that returns a * b. Add 'mod math;' to src/lib.rs. Then create tests/test_math.rs with a test that verifies multiply(3, 4) == 12.",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(180))
        .assert()
        .success();
}

// ── Tier 3: Edge case recordings ──────────────────────────────────────────

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_long_file_handling() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_long_file");

    // Generate a ~100 line file
    let mut content = String::from("// Auto-generated module\n\n");
    for i in 0..50 {
        content.push_str(&format!("pub fn function_{i}() -> i32 {{ {i} }}\n\n"));
    }

    write_file(&workspace.path().join("src/lib.rs"), &content);

    shannon_record(&api_key, &record_dir, &workspace, "long_file_handling")
        .args([
            "--prompt",
            "Read src/lib.rs and add a new function 'pub fn function_50() -> i32 { 50 }' at the end of the file, after function_49.",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    let content = fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap();
    assert!(
        content.contains("function_50"),
        "lib.rs should contain function_50, got: (first 200 chars) {}",
        &content[..content.len().min(200)]
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_json_schema_output() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_json_schema");

    let output = shannon_record(&api_key, &record_dir, &workspace, "json_schema_output")
        .args([
            "--prompt",
            "List the top 3 programming languages by popularity",
            "--output-format",
            "json",
            "--schema",
            "{\"type\":\"object\",\"properties\":{\"languages\":{\"type\":\"array\",\"items\":{\"type\":\"object\",\"properties\":{\"name\":{\"type\":\"string\"},\"rank\":{\"type\":\"integer\"}},\"required\":[\"name\",\"rank\"]}},\"required\":[\"languages\"]}}",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .output()
        .expect("shannon should run");

    // Schema validation may fail for weaker models — recording is still made
    if !output.status.success() {
        eprintln!(
            "NOTE: schema validation failed (exit {}), recording still saved",
            output.status.code().unwrap_or(-1)
        );
    }
}

// ── Additional recording scenarios ─────────────────────────────────────────

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_tool_error_recovery() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_tool_error");

    shannon_record(&api_key, &record_dir, &workspace, "tool_error_recovery")
        .args([
            "--prompt",
            "Try to read a file called nonexistent_file_xyz.txt. When that fails, create it with the content 'recovered'.",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(180))
        .assert()
        .success();

    // The LLM should have recovered from the error and created the file
    assert!(
        workspace.path().join("nonexistent_file_xyz.txt").exists(),
        "LLM should recover from read error by creating the file"
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_code_generation() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_code_gen");

    shannon_record(&api_key, &record_dir, &workspace, "code_generation")
        .args([
            "--prompt",
            "Create a Python file called fib.py that implements a fibonacci(n) function, then run it with python3 to verify fibonacci(10) == 55",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(180))
        .assert()
        .success();

    assert!(
        workspace.path().join("fib.py").exists(),
        "fib.py should be created"
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_nested_directory_write() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_nested_write");

    shannon_record(&api_key, &record_dir, &workspace, "nested_directory_write")
        .args([
            "--prompt",
            "Write file src/models/user.rs with content: pub struct User { pub name: String, pub id: u64, }",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    // Best-effort check: some models may not create the file, but the
    // recording fixture is still useful for replay testing.
    fn has_user_struct(dir: &std::path::Path) -> bool {
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && has_user_struct(&path) {
                return true;
            }
            if path.extension().is_some_and(|ext| ext == "rs")
                && fs::read_to_string(&path).is_ok_and(|c| c.contains("User"))
            {
                return true;
            }
        }
        false
    }
    if !has_user_struct(workspace.path()) {
        eprintln!("warning: model did not create .rs file with User struct");
    }
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_overwrite_existing_file() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_overwrite");

    // Pre-create a file
    write_file(
        &workspace.path().join("config.toml"),
        "version = 1\nname = \"old\"",
    );

    shannon_record(&api_key, &record_dir, &workspace, "overwrite_existing_file")
        .args([
            "--prompt",
            "The file config.toml exists. Update it to have version = 2 and name = \"new\"",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    let content = fs::read_to_string(workspace.path().join("config.toml")).unwrap();
    assert!(
        content.contains("version = 2")
            || content.contains("version=2")
            || content.contains("version=\"2\""),
        "config.toml should be updated to version 2, got: {content}"
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_context_compaction() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_compaction");

    // Create multiple files to generate a long conversation
    for i in 0..5 {
        write_file(
            &workspace.path().join(format!("file_{i}.txt")),
            &format!("Content of file {i}: {}", "x".repeat(100)),
        );
    }

    shannon_record(&api_key, &record_dir, &workspace, "context_compaction")
        .args([
            "--prompt",
            "Read all files file_0.txt through file_4.txt. After reading all of them, tell me which file has the most 'x' characters.",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(240))
        .assert()
        .success();
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_multi_step_reasoning() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_multi_step");

    // Create a file with a bug
    write_file(
        &workspace.path().join("calc.rs"),
        "pub fn multiply(a: i32, b: i32) -> i32 { a + b } // BUG: should be a * b",
    );

    shannon_record(&api_key, &record_dir, &workspace, "multi_step_reasoning")
        .args([
            "--prompt",
            "Read calc.rs, identify the bug in the multiply function, fix it, and verify the fix by explaining what was wrong.",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(180))
        .assert()
        .success();

    let content = fs::read_to_string(workspace.path().join("calc.rs")).unwrap();
    assert!(
        content.contains("a * b") || content.contains("a*b"),
        "multiply should be fixed to use multiplication, got: {content}"
    );
}

// ── Tier 4: Core feature recordings ────────────────────────────────────────

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_session_resume() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_session_resume");

    // First turn: create a file
    shannon_record(&api_key, &record_dir, &workspace, "session_resume_turn1")
        .args([
            "--prompt",
            "Create a file called colors.txt with the content: red, blue, green",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    assert!(
        workspace.path().join("colors.txt").exists(),
        "colors.txt should be created in first turn"
    );

    // Find the session file
    let sessions_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".shannon")
        .join("sessions");
    let latest_session = std::fs::read_dir(&sessions_dir).ok().and_then(|entries| {
        entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|e| {
                let metadata = e.metadata().ok()?;
                let modified = metadata.modified().ok()?;
                Some((modified, e.path()))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .max_by_key(|(m, _)| *m)
            .map(|(_, p)| p)
    });

    if let Some(session_path) = latest_session {
        let session_id = session_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Second turn: resume and add to the file
        shannon_record(&api_key, &record_dir, &workspace, "session_resume_turn2")
            .args([
                "--resume",
                &session_id,
                "--prompt",
                "Read colors.txt and add 'yellow' to the list of colors",
                "--output-format",
                "json",
            ])
            .timeout(std::time::Duration::from_secs(300))
            .assert()
            .success();

        let content = fs::read_to_string(workspace.path().join("colors.txt")).unwrap();
        assert!(
            content.contains("yellow"),
            "colors.txt should contain 'yellow' after resume, got: {content}"
        );
    }
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_claude_md_context() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_claude_md");

    // Create CLAUDE.md with specific instructions
    write_file(
        &workspace.path().join("CLAUDE.md"),
        "# Project Rules\n\n- Always add a doc comment with the format: `/// Calculates X`\n- Never use unwrap() in production code\n",
    );

    write_file(
        &workspace.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    );

    shannon_record(&api_key, &record_dir, &workspace, "claude_md_context")
        .args([
            "--prompt",
            "Read src/lib.rs and add documentation comments following the project rules in CLAUDE.md",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    let content = fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap();
    assert!(
        content.contains("///") || content.contains("Calculates"),
        "lib.rs should have doc comments per CLAUDE.md rules, got: {content}"
    );
}
#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_delete_file() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_delete_file");

    // Create files to be deleted
    write_file(
        &workspace.path().join("keep.txt"),
        "This file should remain",
    );
    write_file(
        &workspace.path().join("delete_me.txt"),
        "This file should be deleted",
    );
    write_file(
        &workspace.path().join("also_delete.txt"),
        "This should also go",
    );

    shannon_record(&api_key, &record_dir, &workspace, "delete_file")
        .args([
            "--prompt",
            "Delete the files called delete_me.txt and also_delete.txt, but keep keep.txt",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    assert!(
        workspace.path().join("keep.txt").exists(),
        "keep.txt should still exist"
    );
    assert!(
        !workspace.path().join("delete_me.txt").exists(),
        "delete_me.txt should be deleted"
    );
    assert!(
        !workspace.path().join("also_delete.txt").exists(),
        "also_delete.txt should be deleted"
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_ndjson_streaming() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_ndjson");

    let output = shannon_record(&api_key, &record_dir, &workspace, "ndjson_streaming")
        .args([
            "--prompt",
            "Create a file called info.txt with the text 'streaming test ok'",
            "--output-format",
            "json-stream",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .output()
        .expect("shannon should run");

    // Verify NDJSON output: each line should be valid JSON
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parsed_count = 0;
    for line in stdout.lines() {
        if !line.trim().is_empty() {
            assert!(
                serde_json::from_str::<serde_json::Value>(line).is_ok(),
                "each NDJSON line should be valid JSON, got: {line}"
            );
            parsed_count += 1;
        }
    }
    assert!(parsed_count > 0, "should produce at least one NDJSON line");

    assert!(
        workspace.path().join("info.txt").exists(),
        "info.txt should be created"
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_multi_provider() {
    // This test records using the provider specified by SHANNON_RECORD_PROVIDER.
    // When that env var is set to e.g. "deepseek", it records a deepseek fixture.
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_multi_provider");

    let provider = record_provider();

    write_file(&workspace.path().join("hello.txt"), "Hello from recording");

    shannon_record(&api_key, &record_dir, &workspace, "multi_provider")
        .args([
            "--prompt",
            "Read hello.txt and tell me what it says",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    // Verify recording was saved with the correct provider
    let qualified_session = format!("{}_{}_{}", provider, record_model(), "multi_provider");
    let fixture_path = record_dir.join(format!("{qualified_session}.jsonl"));
    assert!(
        fixture_path.exists(),
        "fixture should exist for provider '{provider}' at {}",
        fixture_path.display()
    );
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_permission_request() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_permission");

    write_file(
        &workspace.path().join("data.txt"),
        "sensitive data that should not be deleted",
    );

    // Use --prompt with FullAuto to allow the operation but still record
    // the permission interaction in the fixture
    shannon_record(&api_key, &record_dir, &workspace, "permission_request")
        .args([
            "--prompt",
            "Read data.txt, then try to run: cat /etc/shadow. If that fails due to permissions, just read data.txt again and report its contents.",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(180))
        .assert()
        .success();
}

#[serial]
#[test]
#[serial]
#[ignore]
fn record_task_large_workspace() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_large_workspace");

    // Create a workspace with 20+ files across multiple directories
    for dir in &["src", "src/models", "src/api", "src/utils", "tests"] {
        fs::create_dir_all(workspace.path().join(dir)).expect("create dir");
    }

    write_file(
        &workspace.path().join("src/lib.rs"),
        "pub mod models;\npub mod api;\npub mod utils;\n",
    );

    write_file(
        &workspace.path().join("src/models/user.rs"),
        "pub struct User { pub name: String, pub age: u32, pub email: String }\n",
    );
    write_file(
        &workspace.path().join("src/models/post.rs"),
        "pub struct Post { pub title: String, pub body: String, pub author: String }\n",
    );
    write_file(
        &workspace.path().join("src/models/comment.rs"),
        "pub struct Comment { pub text: String, pub user: String }\n",
    );

    write_file(
        &workspace.path().join("src/api/handler.rs"),
        "pub fn handle_request() -> String { \"ok\".to_string() }\n",
    );
    write_file(
        &workspace.path().join("src/api/routes.rs"),
        "pub fn routes() -> Vec<&'static str> { vec![\"/api/users\", \"/api/posts\"] }\n",
    );

    write_file(
        &workspace.path().join("src/utils/helpers.rs"),
        "pub fn trim(s: &str) -> String { s.trim().to_string() }\n",
    );
    write_file(
        &workspace.path().join("src/utils/format.rs"),
        "pub fn format_user(name: &str, age: u32) -> String { format!(\"{name} (age {age})\") }\n",
    );

    for i in 0..12 {
        write_file(
            &workspace.path().join(format!("tests/test_{i:02}.rs")),
            &format!("#[test]\nfn test_{i:02}() {{ assert!(true); }}\n"),
        );
    }

    write_file(
        &workspace.path().join("README.md"),
        "# Test Project\n\nA project with many files.\n",
    );
    write_file(
        &workspace.path().join("Cargo.toml"),
        "[package]\nname = \"testproj\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );

    shannon_record(&api_key, &record_dir, &workspace, "large_workspace")
        .args([
            "--prompt",
            "Explore this project. Find all .rs files, read the main modules, and add a 'pub fn new()' constructor to each struct in src/models/ that initializes with default values. Also update src/lib.rs to include a doc comment for each module.",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    // Verify at least one struct was updated
    let user_content = fs::read_to_string(workspace.path().join("src/models/user.rs")).unwrap();
    assert!(
        user_content.contains("fn new") || user_content.contains("fn default"),
        "at least one model struct should have a constructor, got: {user_content}"
    );
}

// ── Replay tests (no API key needed, use recorded fixtures) ───────────────

#[serial]
#[tokio::test]
#[serial]
async fn replay_fixtures_load_successfully() {
    let dir = fixtures_dir();
    if !dir.exists() {
        return;
    }
    use shannon_core::testing::record_replay::ReplayHarness;
    let harness = ReplayHarness::from_dir(&dir);
    for fixture in &harness.fixtures {
        assert!(!fixture.provider.is_empty(), "provider should not be empty");
        assert!(!fixture.request_hash.is_empty(), "hash should not be empty");
        assert!(
            !fixture.response.body.is_empty(),
            "response body should not be empty"
        );
    }
}

/// Validate a single recorded session: loadable, non-empty, no secrets leaked.
fn validate_session(path: &std::path::Path) -> Result<String, String> {
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    use shannon_core::testing::record_replay::RecordedExchange;
    let exchanges =
        RecordedExchange::load_jsonl(path).map_err(|e| format!("{name}: load failed: {e}"))?;

    if exchanges.is_empty() {
        return Err(format!("{name}: no exchanges in session"));
    }

    // Verify no secrets leaked in response headers
    for (i, ex) in exchanges.iter().enumerate() {
        for (hdr, value) in &ex.response.headers {
            let lower = hdr.to_lowercase();
            let is_sensitive = ["authorization", "x-api-key", "api-key", "cookie"]
                .contains(&lower.as_str())
                || lower.contains("token")
                || lower.contains("secret");
            if is_sensitive && value != "***REDACTED***" {
                return Err(format!("{name} exchange {i}: leaked secret in '{hdr}'"));
            }
        }
    }

    Ok(name)
}

#[serial]
#[test]
#[serial]
fn replay_each_recorded_session() {
    let dir = fixtures_dir();
    if !dir.exists() {
        return;
    }

    let mut tested = Vec::new();
    let mut errors = Vec::new();

    for entry in fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        match validate_session(&path) {
            Ok(name) => tested.push(name),
            Err(e) => errors.push(e),
        }
    }

    if tested.is_empty() && errors.is_empty() {
        eprintln!("No recorded sessions found in {}", dir.display());
        return;
    }

    assert!(
        errors.is_empty(),
        "{} of {} sessions failed:\n  {}",
        errors.len(),
        tested.len() + errors.len(),
        errors.join("\n  ")
    );
}

#[serial]
#[test]
#[serial]
fn replay_workspace_creation_works() {
    let dir = fixtures_dir();
    if !dir.exists() {
        return;
    }

    // Verify we can create a workspace and that the fixture directory
    // is accessible from test code
    let workspace = create_workspace("replay_test");
    assert!(workspace.path().exists());

    // Write a test file to verify workspace isolation
    fs::write(workspace.path().join("test.txt"), "replay").expect("write");
    assert_eq!(
        fs::read_to_string(workspace.path().join("test.txt")).unwrap(),
        "replay"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// ── Agent-level replay (ADR 0003 Phase 1) ────────────────────────────────
// Re-drives the agent against mockito-served recorded fixtures. No API key.
// Only deterministic tasks; volatile tasks (git, glob, search) are skipped.
// If a fixture is missing or the request body drifts (stale fixture), the
// test skips with a "run: just record" hint rather than failing CI.
//
// Run: just replay-agent
// ══════════════════════════════════════════════════════════════════════════

/// Provider/model pair recorded in the committed fixtures. Replay reads from
/// these; recording a different model adds a parallel set, not a replacement.
const REPLAY_PROVIDER: &str = "minimax";
const REPLAY_MODEL: &str = "MiniMax-M3";

// Local-only VCR harness test. Runs via `just replay-agent`. #[ignore]d
// for CI by design — exact-match VCR replay proved too brittle for the
// GitHub Actions runner env (see ADR 0003 Phase 1 落地回顾). Developers
// run locally to validate the harness; CI gets the structural replay
// tests (replay_*) instead, which don't drive the agent.
#[serial]
#[test]
#[ignore = "local-only VCR harness; run via `just replay-agent`"]
#[serial]
fn replay_agent_create_file() {
    let path = replay_fixture_path(REPLAY_PROVIDER, REPLAY_MODEL, "create_file");
    if !path.exists() {
        eprintln!(
            "skip: no fixture at {} — run: just record-minimax",
            path.display()
        );
        return;
    }
    let workspace = create_workspace("replay_create_file");
    let mut server = mockito::Server::new();
    let _mocks = mount_fixture(&mut server, &path, workspace.path());

    shannon_replay(&server.url(), REPLAY_PROVIDER, REPLAY_MODEL, &workspace)
        .args([
            "--prompt",
            "Create a file called hello.txt with the content 'world'",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();

    assert!(
        workspace.path().join("hello.txt").exists(),
        "hello.txt should be created"
    );
    let content = fs::read_to_string(workspace.path().join("hello.txt")).unwrap();
    assert!(
        content.contains("world"),
        "hello.txt should contain 'world', got: {content}"
    );
}

// Local-only VCR harness test. Runs via `just replay-agent`. #[ignore]d
// for CI by design — exact-match VCR replay proved too brittle for the
// GitHub Actions runner env (see ADR 0003 Phase 1 落地回顾).
#[serial]
#[test]
#[ignore = "local-only VCR harness; run via `just replay-agent`"]
#[serial]
fn replay_agent_bash_command() {
    let path = replay_fixture_path(REPLAY_PROVIDER, REPLAY_MODEL, "bash_command");
    if !path.exists() {
        eprintln!(
            "skip: no fixture at {} — run: just record-minimax",
            path.display()
        );
        return;
    }
    let workspace = create_workspace("replay_bash_cmd");
    let mut server = mockito::Server::new();
    let _mocks = mount_fixture(&mut server, &path, workspace.path());

    shannon_replay(&server.url(), REPLAY_PROVIDER, REPLAY_MODEL, &workspace)
        .args([
            "--prompt",
            "Run the command: echo hello_shannon > output.txt",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();

    assert!(
        workspace.path().join("output.txt").exists(),
        "output.txt should be created"
    );
}

// Local-only VCR harness test. Runs via `just replay-agent`. #[ignore]d
// for CI by design — exact-match VCR replay proved too brittle for the
// GitHub Actions runner env (see ADR 0003 Phase 1 落地回顾).
#[serial]
#[test]
#[ignore = "local-only VCR harness; run via `just replay-agent`"]
#[serial]
fn replay_agent_read_and_edit() {
    let path = replay_fixture_path(REPLAY_PROVIDER, REPLAY_MODEL, "read_and_edit");
    if !path.exists() {
        eprintln!(
            "skip: no fixture at {} — run: just record-minimax",
            path.display()
        );
        return;
    }
    let workspace = create_workspace("replay_read_edit");
    // Pre-create the same file the recording edited, so tool results match.
    write_file(
        &workspace.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }",
    );
    let mut server = mockito::Server::new();
    let _mocks = mount_fixture(&mut server, &path, workspace.path());

    shannon_replay(&server.url(), REPLAY_PROVIDER, REPLAY_MODEL, &workspace)
        .args([
            "--prompt",
            "Read src/lib.rs and add a doc comment above the add function",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();

    let content = fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap();
    assert!(
        content.contains("///") || content.contains("//"),
        "lib.rs should have a comment added, got: {content}"
    );
}

// Local-only VCR harness test. Runs via `just replay-agent`. #[ignore]d
// for CI by design — exact-match VCR replay proved too brittle for the
// GitHub Actions runner env (see ADR 0003 Phase 1 落地回顾).
#[serial]
#[test]
#[ignore = "local-only VCR harness; run via `just replay-agent`"]
#[serial]
fn replay_agent_overwrite_existing_file() {
    let path = replay_fixture_path(REPLAY_PROVIDER, REPLAY_MODEL, "overwrite_existing_file");
    if !path.exists() {
        eprintln!(
            "skip: no fixture at {} — run: just record-minimax",
            path.display()
        );
        return;
    }
    let workspace = create_workspace("replay_overwrite");
    write_file(
        &workspace.path().join("config.toml"),
        "version = 1\nname = \"old\"",
    );
    let mut server = mockito::Server::new();
    let _mocks = mount_fixture(&mut server, &path, workspace.path());

    shannon_replay(&server.url(), REPLAY_PROVIDER, REPLAY_MODEL, &workspace)
        .args([
            "--prompt",
            "The file config.toml exists. Update it to have version = 2 and name = \"new\"",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();

    let content = fs::read_to_string(workspace.path().join("config.toml")).unwrap();
    assert!(
        content.contains("version = 2")
            || content.contains("version=2")
            || content.contains("version=\"2\""),
        "config.toml should be updated to version 2, got: {content}"
    );
}

// NOTE: `delete_file` is intentionally NOT in the replay set. Its fixture has
// two properties that defeat exact-match VCR: (1) the model issues Bash with an
// explicit `cwd` field, whose absolute tempdir path is not visible inside the
// bwrap sandbox (bound to /workspace, ADR 0003 trigger), so the tool errors and
// diverges from the recorded request stream; (2) it is a 17-exchange exploratory
// session whose Glob/Bash results depend on filesystem ordering. Re-recording
// with a tighter prompt (single-file delete, no explicit cwd) would make it
// deterministic — left for future work. The `record_task_delete_file` test
// remains so the fixture can be regenerated.

// ── Unit tests for test helpers ───────────────────────────────────────

#[serial]
#[test]
fn test_write_file_creates_nested_dirs() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("src/deep/nested/lib.rs");
    write_file(&path, "fn main() {}");
    assert!(path.exists());
    assert_eq!(fs::read_to_string(&path).unwrap(), "fn main() {}");
}

#[serial]
#[test]
fn test_write_file_flat_path() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("README.md");
    write_file(&path, "# hello");
    assert!(path.exists());
    assert_eq!(fs::read_to_string(&path).unwrap(), "# hello");
}

#[serial]
#[test]
fn test_write_file_content_roundtrip() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("special.txt");
    let content = "line1\nline2\n\ttabs & ampersands\n\"quotes\"\n";
    write_file(&path, content);
    assert_eq!(fs::read_to_string(&path).unwrap(), content);
}

#[serial]
#[test]
fn test_write_file_empty_content() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("empty.txt");
    write_file(&path, "");
    assert!(path.exists());
    assert_eq!(fs::read_to_string(&path).unwrap(), "");
}

#[serial]
#[test]
fn test_write_file_overwrites_existing() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("file.txt");
    write_file(&path, "first");
    write_file(&path, "second");
    assert_eq!(fs::read_to_string(&path).unwrap(), "second");
}

#[serial]
#[test]
fn test_provider_key_env_all_providers() {
    assert_eq!(provider_key_env("zhipu"), Some("ZHIPU_API_KEY"));
    assert_eq!(provider_key_env("zhipu-cn"), Some("ZHIPU_API_KEY"));
    assert_eq!(provider_key_env("zhipu-intl"), Some("ZHIPU_INTL_API_KEY"));
    assert_eq!(
        provider_key_env("zhipu-international"),
        Some("ZHIPU_INTL_API_KEY")
    );
    assert_eq!(provider_key_env("minimax"), Some("MINIMAX_API_KEY"));
    assert_eq!(provider_key_env("moonshot"), Some("MOONSHOT_API_KEY"));
    assert_eq!(provider_key_env("kimi"), Some("MOONSHOT_API_KEY"));
    assert_eq!(provider_key_env("deepseek"), Some("DEEPSEEK_API_KEY"));
    assert_eq!(provider_key_env("dashscope"), Some("DASHSCOPE_API_KEY"));
    assert_eq!(provider_key_env("qwen"), Some("DASHSCOPE_API_KEY"));
}

#[serial]
#[test]
fn test_provider_key_env_unknown_returns_none() {
    assert_eq!(provider_key_env("anthropic"), None);
    assert_eq!(provider_key_env("openai"), None);
    assert_eq!(provider_key_env("ollama"), None);
    assert_eq!(provider_key_env(""), None);
}

#[serial]
#[test]
fn test_record_provider_default() {
    // Without SHANNON_RECORD_PROVIDER set, defaults to "anthropic"
    // (We can't easily test the env-var override without isolating env,
    //  but we can verify the function exists and returns a string)
    let provider = record_provider();
    assert!(!provider.is_empty());
}

#[serial]
#[test]
fn test_create_workspace_unique_paths() {
    let ws1 = create_workspace("test_a");
    let ws2 = create_workspace("test_b");
    assert_ne!(ws1.path(), ws2.path());
    assert!(ws1.path().exists());
    assert!(ws2.path().exists());
}

#[serial]
#[test]
fn test_create_workspace_subdir_exists() {
    let ws = create_workspace("my_test");
    let subdir = ws.path().join("my_test");
    assert!(subdir.exists());
    assert!(subdir.is_dir());
}

#[serial]
#[test]
fn test_all_nested_writes_use_helper() {
    // Self-check: ensure no fs::write with nested paths (src/foo, bar/baz)
    // outside the write_file() helper itself. Prevents "NotFound" errors
    // when parent directories don't exist.
    let source = include_str!("live_tests.rs");
    for (i, line) in source.lines().enumerate() {
        let line_num = i + 1;
        // Skip lines inside the write_file() helper (~line 151)
        if (147..=152).contains(&line_num) {
            continue;
        }
        // Any fs::write to a nested path (contains "/" in the join arg)
        // should use write_file() instead
        if line.contains("fs::write(") && line.contains(".join(\"") && line.contains("/") {
            panic!(
                "line {line_num}: fs::write with nested path found — use write_file() instead:\n  {line}"
            );
        }
    }
}

// ── Unit tests for rewrite_path_suffix ───────────────────────────────────
//
// These tests pin down the suffix-only rewrite logic that mount_exchange
// applies to recorded bodies before matching them against mockito. Splitting
// the rewrite out of mount_exchange lets us test it without spinning up a
// mockito server, and makes the SSE-fragmentation case (test 3) explicit.

#[cfg(test)]
mod rewrite_tests {
    use super::rewrite_path_suffix;
    use serial_test::serial;

    #[serial]
    #[test]
    fn rewrite_common_prefix_normal_ci_case() {
        // Recorded CWD and replay CWD share the `/tmp/.tmp` prefix; only the
        // random tail differs. All occurrences of the recorded suffix should
        // become the replay suffix.
        let recorded_cwd = "/tmp/.tmpJ5YFHN";
        let replay_cwd = "/tmp/.tmpnWTqOz";
        let body = r#"{"cwd": "/tmp/.tmpJ5YFHN", "msg": "Working at /tmp/.tmpJ5YFHN/sub"}"#;
        let expected = r#"{"cwd": "/tmp/.tmpnWTqOz", "msg": "Working at /tmp/.tmpnWTqOz/sub"}"#;
        assert_eq!(
            rewrite_path_suffix(body, recorded_cwd, replay_cwd),
            expected
        );
    }

    #[serial]
    #[test]
    fn rewrite_different_prefix_length_local_without_dot_tmp() {
        // Local development without `tempfile` may produce a different prefix
        // length (e.g. `/tmp/.dbgWS` instead of `/tmp/.tmpXXXXXX`). The longest
        // common prefix is `/tmp/.` (6 chars); recorded_suffix = `tmp0ZFV0J`,
        // replay_suffix = `dbgWS`. The recorded-suffix substring appears inside
        // the path and gets swapped for the replay-suffix.
        let recorded_cwd = "/tmp/.tmp0ZFV0J";
        let replay_cwd = "/tmp/.dbgWS";
        let body = r#"{"cwd": "/tmp/.tmp0ZFV0J"}"#;
        let expected = r#"{"cwd": "/tmp/.dbgWS"}"#;
        assert_eq!(
            rewrite_path_suffix(body, recorded_cwd, replay_cwd),
            expected
        );
    }

    #[serial]
    #[test]
    fn rewrite_sse_fragmentation_reassembles_path() {
        // When a tool-call argument streams across multiple SSE delta
        // fragments, the suffix can be split across fragment boundaries (e.g.
        // fragment 1 ends `/tmp/.tmp`, fragment 2 starts with the suffix).
        // Running rewrite_path_suffix on each fragment independently must
        // produce a concatenation that contains the correct replay path —
        // the agent's streaming parser reassembles it from the reassembled
        // fragments.
        let recorded_cwd = "/tmp/.tmpJ5YFHN";
        let replay_cwd = "/tmp/.tmpnWTqOz";
        let body1 = r#"data: {"args": {"path": "/tmp/.tmp"#;
        let body2 = r#"J5YFHN/file"}}"#;
        let rewritten_1 = rewrite_path_suffix(body1, recorded_cwd, replay_cwd);
        let rewritten_2 = rewrite_path_suffix(body2, recorded_cwd, replay_cwd);
        let concatenated = format!("{rewritten_1}{rewritten_2}");
        assert!(
            concatenated.contains("/tmp/.tmpnWTqOz/file"),
            "concatenated fragments should reconstruct the replay path, got: {concatenated}"
        );
    }

    #[serial]
    #[test]
    fn rewrite_noop_when_suffix_is_empty() {
        // Identical CWDs ⇒ common_len == len(recorded_cwd) ⇒ recorded_suffix
        // is empty ⇒ body is returned unchanged.
        let cwd = "/tmp/.tmpJ5YFHN";
        let body = r#"{"cwd": "/tmp/.tmpJ5YFHN"}"#;
        assert_eq!(rewrite_path_suffix(body, cwd, cwd), body);
    }
}
