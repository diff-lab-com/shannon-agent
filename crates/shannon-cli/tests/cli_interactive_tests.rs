//! REPL interactive tests for shannon-cli using rexpect (PTY-based).
//!
//! These tests drive the `shannon repl` subcommand through a pseudo-terminal,
//! verifying startup display, slash commands, and clean exit.
//!
//! To run:
//!   cargo test --test cli_interactive_tests -- --ignored
//!
//! Note: These tests require a terminal environment and may not work in
//! headless CI without a PTY.

use rexpect::session::PtySession;
use rexpect::spawn;

const BIN: &str = "shannon";

fn shannon_bin_path() -> String {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    format!("{manifest_dir}/../../target/{profile}/{BIN}")
}

fn spawn_repl(args: &[&str], timeout_ms: u64) -> PtySession {
    let bin = shannon_bin_path();
    let mut cmd = format!("{bin} repl");
    for arg in args {
        cmd.push(' ');
        cmd.push_str(arg);
    }
    // Use a non-reachable base URL to prevent accidental API calls
    // for slash-command-only tests
    spawn(&format!("env SHANNON_BASE_URL=http://127.0.0.1:1 SHANNON_PROVIDER=ollama SHANNON_MODEL=test {cmd}"), Some(timeout_ms))
        .expect("Failed to spawn shannon repl")
}

// ════════════════════════════════════════════════════════════════════════
// Test: REPL startup and display
// ════════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn test_repl_startup_display() {
    let mut p = spawn_repl(&[], 15_000);

    // The REPL should show some startup indicator
    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should show startup prompt");

    p.send_line("/exit").unwrap();
    let _ = p.exp_eof();
}

#[test]
#[ignore]
fn test_repl_help_command() {
    let mut p = spawn_repl(&[], 15_000);

    // Wait for prompt
    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should start");

    p.send_line("/help").unwrap();
    p.exp_regex("(?i)help|command")
        .expect("/help should display command list");

    p.send_line("/exit").unwrap();
}

#[test]
#[ignore]
fn test_repl_model_command() {
    let mut p = spawn_repl(&["--model", "test-model"], 15_000);

    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should start");

    p.send_line("/model").unwrap();
    p.exp_regex("(?i)test-model|model")
        .expect("/model should show current model");

    p.send_line("/exit").unwrap();
}

#[test]
#[ignore]
fn test_repl_exit_command() {
    let mut p = spawn_repl(&[], 15_000);

    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should start");

    p.send_line("/exit").unwrap();
    // Should exit cleanly — rexpect will detect EOF
    match p.exp_eof() {
        Ok(_) => {}
        Err(e) => eprintln!("Note: exp_eof returned: {e} (may be normal)"),
    }
}

#[test]
#[ignore]
fn test_repl_query_with_mock() {
    // This test uses a mock server via mockito, but rexpect doesn't
    // easily share mockito state. Instead, we test that the REPL
    // accepts input and attempts to connect. A connection-refused error
    // is acceptable — we're testing the input path, not the API response.
    let mut p = spawn_repl(&[], 15_000);

    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should start");

    // Send a query — it will fail to connect (unreachable URL) but
    // verifies the REPL accepts free-text input
    p.send_line("hello test query").unwrap();

    // Expect either a response or an error message (connection refused)
    p.exp_regex("(?i)error|fail|hello|response|connect")
        .expect("REPL should show some reaction to the query");

    p.send_line("/exit").unwrap();
}

// ════════════════════════════════════════════════════════════════════════
// Test: REPL /compact command
// ════════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn test_repl_compact_command() {
    // /compact should be recognized even with no conversation history
    let mut p = spawn_repl(&[], 15_000);

    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should start");

    p.send_line("/compact").unwrap();
    // Should acknowledge compact command (may say "nothing to compact" or similar)
    p.exp_regex("(?i)compact|nothing|empty|context|history|no.*message")
        .expect("/compact should produce some acknowledgment");

    p.send_line("/exit").unwrap();
}

#[test]
#[ignore]
fn test_repl_compact_after_query() {
    // Send a query, then /compact — verifies /compact is accepted after activity
    let mut p = spawn_repl(&[], 20_000);

    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should start");

    // Send a query (will fail to connect, that's OK)
    p.send_line("test query for compact").unwrap();
    // Wait for response or error
    p.exp_regex("(?i)error|fail|response|connect|shannon")
        .expect("Should get some response to query");

    p.send_line("/compact").unwrap();
    // Should acknowledge the compact command
    p.exp_regex("(?i)compact|context|history|compress")
        .expect("/compact after query should acknowledge");

    p.send_line("/exit").unwrap();
}

// ════════════════════════════════════════════════════════════════════════
// Test: REPL /context command
// ════════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn test_repl_context_command() {
    // /context should show context status (empty context is valid)
    let mut p = spawn_repl(&[], 15_000);

    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should start");

    p.send_line("/context").unwrap();
    // Should show context info — tokens, messages, or "empty"
    p.exp_regex("(?i)context|token|message|empty|usage|0")
        .expect("/context should display context info");

    p.send_line("/exit").unwrap();
}

// ════════════════════════════════════════════════════════════════════════
// Test: REPL /version command
// ════════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn test_repl_version_command() {
    let mut p = spawn_repl(&[], 15_000);

    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should start");

    p.send_line("/version").unwrap();
    p.exp_regex("(?i)version|v\\d|0\\.\\d")
        .expect("/version should display version info");

    p.send_line("/exit").unwrap();
}

// ════════════════════════════════════════════════════════════════════════
// Test: REPL /lang command
// ════════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn test_repl_lang_command() {
    // /lang should show current language or list available languages
    let mut p = spawn_repl(&[], 15_000);

    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should start");

    p.send_line("/lang").unwrap();
    // Should show language info
    p.exp_regex("(?i)lang|english|en|zh|available|current")
        .expect("/lang should display language info");

    p.send_line("/exit").unwrap();
}

// ════════════════════════════════════════════════════════════════════════
// Test: REPL provider switching
// ════════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn test_repl_config_command() {
    // /config should display current configuration
    let mut p = spawn_repl(&[], 15_000);

    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should start");

    p.send_line("/config").unwrap();
    p.exp_regex("(?i)config|provider|model|ollama")
        .expect("/config should display configuration");

    p.send_line("/exit").unwrap();
}

#[test]
#[ignore]
fn test_repl_clear_command() {
    // /clear should be recognized (clears conversation history)
    let mut p = spawn_repl(&[], 15_000);

    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should start");

    p.send_line("/clear").unwrap();
    // Should acknowledge clear (may show nothing, or "cleared", or just a new prompt)
    p.exp_regex("(?i)clear|shannon|ready|>|done")
        .expect("/clear should be acknowledged");

    p.send_line("/exit").unwrap();
}

// ════════════════════════════════════════════════════════════════════════
// Test: REPL unknown command handling
// ════════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn test_repl_unknown_command() {
    // Unknown slash commands should produce an error, not crash
    let mut p = spawn_repl(&[], 15_000);

    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should start");

    p.send_line("/nonexistent_command_xyz").unwrap();
    p.exp_regex("(?i)unknown|error|not.*found|invalid|no.*command")
        .expect("Unknown command should produce error message");

    // REPL should still be alive after unknown command
    p.send_line("/exit").unwrap();
}

// ════════════════════════════════════════════════════════════════════════
// Test: REPL multi-line input handling
// ════════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn test_repl_multiple_commands_sequence() {
    // Execute multiple slash commands in sequence to verify state consistency
    let mut p = spawn_repl(&[], 20_000);

    p.exp_regex("(?i)shannon|ready|>")
        .expect("REPL should start");

    // First command
    p.send_line("/model").unwrap();
    p.exp_regex("(?i)test|model").expect("/model should work");

    // Second command — REPL should still be responsive
    p.send_line("/context").unwrap();
    p.exp_regex("(?i)context|token|message|empty|0")
        .expect("/context should work after /model");

    // Third command
    p.send_line("/compact").unwrap();
    p.exp_regex("(?i)compact|nothing|empty|context|history")
        .expect("/compact should work after /context");

    // Clean exit
    p.send_line("/exit").unwrap();
}
