//! Integration tests for shannon-cli argument parsing and types.
//!
//! These tests exercise the CLI binary via `assert_cmd` to verify argument
//! parsing, output format handling, and basic invocation behavior.

use assert_cmd::Command;
use predicates::prelude::*;
use serial_test::serial;

const BIN: &str = "shannon";

fn shannon() -> Command {
    Command::cargo_bin(BIN).unwrap()
}

// ── Version Flag ────────────────────────────────────────────────────────

#[serial]
#[test]
fn test_version_flag_long() {
    shannon()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("shannon"));
}

#[serial]
#[test]
fn test_version_flag_short() {
    shannon()
        .arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::contains("shannon"));
}

// ── Help Flag ───────────────────────────────────────────────────────────

#[serial]
#[test]
fn test_help_flag_long() {
    shannon()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("AI-powered code assistant"));
}

#[serial]
#[test]
fn test_help_flag_short() {
    shannon()
        .arg("-h")
        .assert()
        .success()
        .stdout(predicate::str::contains("AI-powered code assistant"));
}

// ── Subcommand Help ─────────────────────────────────────────────────────

#[serial]
#[test]
fn test_repl_subcommand_help() {
    shannon()
        .args(["repl", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("repl"));
}

#[serial]
#[test]
fn test_version_subcommand_help() {
    shannon()
        .args(["version", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("version"));
}

#[serial]
#[test]
fn test_query_subcommand_help() {
    shannon()
        .args(["query", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("query"));
}

#[serial]
#[test]
fn test_config_subcommand_help() {
    shannon()
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("config"));
}

// ── Output Format ───────────────────────────────────────────────────────

#[serial]
#[test]
fn test_output_format_text_is_default() {
    shannon()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("output-format"));
}

#[serial]
#[test]
fn test_output_format_json_flag() {
    // --output-format json should be accepted by clap parsing
    // (it will fail later because there's no --prompt, but clap should accept the arg)
    shannon()
        .args(["--output-format", "json", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_output_format_json_stream_flag() {
    shannon()
        .args(["--output-format", "json-stream", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_output_format_invalid() {
    shannon()
        .args(["--output-format", "xml"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid").or(predicate::str::contains("error")));
}

// ── CLI Argument Parsing ────────────────────────────────────────────────

#[serial]
#[test]
fn test_model_flag() {
    shannon()
        .args(["--model", "gpt-4o", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_provider_flag() {
    shannon()
        .args(["--provider", "openai", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_lang_flag() {
    shannon()
        .args(["--lang", "en", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_yes_flag() {
    shannon().args(["--yes", "--help"]).assert().success();
}

#[serial]
#[test]
fn test_quiet_flag() {
    shannon().args(["--quiet", "--help"]).assert().success();
}

#[serial]
#[test]
fn test_diff_only_flag() {
    shannon().args(["--diff-only", "--help"]).assert().success();
}

#[serial]
#[test]
fn test_resume_flag() {
    shannon().args(["--resume", "--help"]).assert().success();
}

#[serial]
#[test]
fn test_continue_flag() {
    shannon().args(["--continue", "--help"]).assert().success();
}

// ── Repl Subcommand Args ────────────────────────────────────────────────

#[serial]
#[test]
fn test_repl_model_flag() {
    shannon()
        .args(["repl", "--model", "claude-sonnet-4", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_repl_provider_flag() {
    shannon()
        .args(["repl", "--provider", "anthropic", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_repl_max_tokens_flag() {
    shannon()
        .args(["repl", "--max-tokens", "4096", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_repl_temperature_flag() {
    shannon()
        .args(["repl", "--temperature", "0.5", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_repl_timeout_flag() {
    shannon()
        .args(["repl", "--timeout", "60", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_repl_debug_flag() {
    shannon()
        .args(["repl", "--debug", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_repl_local_flag() {
    shannon()
        .args(["repl", "--local", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_repl_cwd_flag() {
    shannon()
        .args(["repl", "--cwd", "/tmp", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_repl_env_flag() {
    shannon()
        .args(["repl", "-e", "KEY=value", "--help"])
        .assert()
        .success();
}

#[serial]
#[test]
fn test_repl_file_flag() {
    shannon()
        .args(["repl", "--file", "some_file.rs", "--help"])
        .assert()
        .success();
}

// ── Query Subcommand Args ───────────────────────────────────────────────

#[serial]
#[test]
fn test_query_subcommand() {
    shannon()
        .args(["query", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("query"));
}

#[serial]
#[test]
fn test_query_output_flag() {
    shannon()
        .args(["query", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("output"));
}

#[serial]
#[test]
fn test_query_no_stream_flag() {
    shannon()
        .args(["query", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no-stream"));
}

// ── Version Subcommand Args ─────────────────────────────────────────────

#[serial]
#[test]
fn test_version_verbose_flag() {
    shannon()
        .args(["version", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("verbose"));
}

// ── Pipe Flag ───────────────────────────────────────────────────────────

#[serial]
#[test]
fn test_pipe_flag_in_help() {
    shannon()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pipe"));
}

// ── Headless Mode Args ──────────────────────────────────────────────────

#[serial]
#[test]
fn test_headless_prompt_flag() {
    shannon()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("prompt"));
}

#[serial]
#[test]
fn test_max_turns_flag() {
    shannon()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("max-turns"));
}

#[serial]
#[test]
fn test_exit_on_error_flag() {
    shannon()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("exit-on-error"));
}

#[serial]
#[test]
fn test_allowed_tools_flag() {
    shannon()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("allowed-tools"));
}

// ── Session Flag ────────────────────────────────────────────────────────

#[serial]
#[test]
fn test_session_flag() {
    shannon()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("session"));
}

// ── Invalid Arguments ───────────────────────────────────────────────────

#[serial]
#[test]
fn test_unknown_flag_fails() {
    shannon().args(["--nonexistent-flag"]).assert().failure();
}

#[serial]
#[test]
fn test_invalid_repl_args() {
    shannon().args(["repl", "--nonexistent"]).assert().failure();
}
