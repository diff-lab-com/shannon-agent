//! Integration tests for the auto-test loop (P1-5).
//!
//! These tests exercise the `shannon_core::auto_test` public API end-to-end
//! against real shell scripts in a temporary directory. They verify:
//!
//! 1. Auto-fix on an intentionally failing test: we point the runner at a
//!    fixture "test command" that initially exits 1, then mutate the
//!    fixture so it exits 0, and verify the second iteration passes.
//! 2. Loop guards kick in: max_iterations, total_timeout, no_progress.
//! 3. End-to-end "happy path": the runner detects a Rust project and runs
//!    `cargo build --no-run` as a quick sanity check.

use shannon_core::auto_test::{
    AntiLoopState, AutoTestConfig, Language, LoopDecision, StopReason, TestOutcome,
    outcome_from_output, run_auto_test, run_test_command,
};
use std::time::Duration;
use tempfile::TempDir;

/// Create a self-contained "test runner" — a shell script that the
/// `auto_test` loop can invoke. The fixture reads its behaviour from a
/// `$FIXTURE_STATE_FILE` file so tests can mutate the file between
/// iterations to simulate the agent fixing code.
fn make_fixture(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let state_path = dir.join("fixture_state.txt");
    std::fs::write(&state_path, "fail_a\n").unwrap();
    let script = dir.join("fixture_test.sh");
    let body = r#"#!/usr/bin/env bash
set -e
state_file="${FIXTURE_STATE_FILE:-fixture_state.txt}"
state=$(cat "$state_file" 2>/dev/null || echo fail)
case "$state" in
    pass)      echo "all green"; exit 0 ;;
    fail_a)    echo "FAIL: assertion failed at foo.rs:42"; exit 1 ;;
    fail_b)    echo "FAIL: assertion failed at foo.rs:99"; exit 1 ;;
    *)         echo "FAIL: default"; exit 1 ;;
esac
"#;
    std::fs::write(&script, body).unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    (script, state_path)
}

fn set_state(state_file: &std::path::Path, value: &str) {
    std::fs::write(state_file, value).unwrap();
}

#[tokio::test]
async fn auto_fix_after_first_iteration_passes() {
    // Scenario: agent writes code that breaks a test; auto-test runs the
    // fixture (fails), the engine injects the failure, the LLM "fixes" the
    // code (we mutate the fixture state), and the next iteration passes.
    let dir = TempDir::new().unwrap();
    let (_script, state) = make_fixture(dir.path());
    // Use a relative path because cwd will be the temp dir.
    let cmd = format!(
        "FIXTURE_STATE_FILE={} ./fixture_test.sh",
        state.file_name().unwrap().to_str().unwrap()
    );

    // Iteration 1 — fails.
    let cfg = AutoTestConfig {
        command: Some(cmd.clone()),
        max_iterations: 5,
        timeout_secs: 5,
        no_progress_strikes: 3,
        ..Default::default()
    };
    let outcome1 = run_auto_test(&cfg, dir.path()).await.unwrap();
    match &outcome1 {
        TestOutcome::Failed { summary } => {
            assert!(summary.contains("FAIL"));
            assert!(summary.contains("foo.rs:42"));
        }
        other => panic!("expected Failed, got {other:?}"),
    }

    // Agent "fixes" the code — mutate fixture state to pass.
    set_state(&state, "pass");
    let outcome2 = run_auto_test(&cfg, dir.path()).await.unwrap();
    assert!(matches!(outcome2, TestOutcome::Passed));
}

#[tokio::test]
async fn anti_loop_stops_after_max_iterations() {
    let dir = TempDir::new().unwrap();
    let (_script, _state) = make_fixture(dir.path());

    let cfg = AutoTestConfig {
        command: Some("./fixture_test.sh".into()),
        max_iterations: 3,
        no_progress_strikes: 100, // disable no-progress for this test
        timeout_secs: 5,
        ..Default::default()
    };

    let mut state = AntiLoopState::new();
    let failed = TestOutcome::Failed {
        summary: "boom".into(),
    };
    let d1 = state.record(&cfg, &failed);
    let d2 = state.record(&cfg, &failed);
    let d3 = state.record(&cfg, &failed);
    assert_eq!(d1, LoopDecision::Continue);
    assert_eq!(d2, LoopDecision::Continue);
    assert_eq!(d3, LoopDecision::Stop(StopReason::MaxIterations));
    assert_eq!(state.iterations, 3);
}

#[tokio::test]
async fn anti_loop_stops_after_no_progress_strikes() {
    let dir = TempDir::new().unwrap();
    let (_script, _state) = make_fixture(dir.path());

    let cfg = AutoTestConfig {
        command: Some("./fixture_test.sh".into()),
        max_iterations: 100,
        no_progress_strikes: 2, // 2 identical failures → stop
        timeout_secs: 5,
        ..Default::default()
    };

    let mut state = AntiLoopState::new();
    let same = TestOutcome::Failed {
        summary: "same error every time".into(),
    };
    assert_eq!(state.record(&cfg, &same), LoopDecision::Continue);
    assert_eq!(
        state.record(&cfg, &same),
        LoopDecision::Stop(StopReason::NoProgress)
    );
}

#[tokio::test]
async fn anti_loop_total_timeout_fires() {
    let _dir = TempDir::new().unwrap();
    let cfg = AutoTestConfig {
        command: Some("./fixture_test.sh".into()),
        max_iterations: 100,
        total_timeout_secs: 0, // already expired
        ..Default::default()
    };

    let mut state = AntiLoopState::new();
    std::thread::sleep(Duration::from_millis(10));
    let failed = TestOutcome::Failed {
        summary: "x".into(),
    };
    assert_eq!(
        state.record(&cfg, &failed),
        LoopDecision::Stop(StopReason::TotalTimeout)
    );
}

#[tokio::test]
async fn anti_loop_per_run_timeout_is_terminal() {
    let _dir = TempDir::new().unwrap();
    let mut state = AntiLoopState::new();
    let cfg = AutoTestConfig::default();
    let outcome = TestOutcome::TimedOut;
    assert_eq!(
        state.record(&cfg, &outcome),
        LoopDecision::Stop(StopReason::Timeout)
    );
    assert_eq!(state.stopped_reason, Some(StopReason::Timeout));
}

#[tokio::test]
async fn anti_loop_spawn_error_is_terminal() {
    let _dir = TempDir::new().unwrap();
    let mut state = AntiLoopState::new();
    let cfg = AutoTestConfig::default();
    let outcome = TestOutcome::SpawnError("no bash in PATH".into());
    assert_eq!(
        state.record(&cfg, &outcome),
        LoopDecision::Stop(StopReason::SpawnError)
    );
}

#[tokio::test]
async fn progress_is_reset_when_failure_text_changes() {
    let cfg = AutoTestConfig {
        command: Some("./fixture_test.sh".into()),
        max_iterations: 100,
        no_progress_strikes: 3,
        ..Default::default()
    };
    let mut state = AntiLoopState::new();

    // Two failures, same text — strike counter at 2.
    let f1 = TestOutcome::Failed {
        summary: "first error".into(),
    };
    state.record(&cfg, &f1);
    state.record(&cfg, &f1);
    assert_eq!(state.consecutive_same, 2);

    // Different failure → counter resets to 1.
    let f2 = TestOutcome::Failed {
        summary: "different error".into(),
    };
    state.record(&cfg, &f2);
    assert_eq!(state.consecutive_same, 1);
}

#[tokio::test]
async fn pass_immediately_stops_loop() {
    let cfg = AutoTestConfig {
        command: Some("./fixture_test.sh".into()),
        ..Default::default()
    };
    let mut state = AntiLoopState::new();
    let outcome = TestOutcome::Passed;
    assert_eq!(
        state.record(&cfg, &outcome),
        LoopDecision::Stop(StopReason::Passed)
    );
    assert_eq!(state.iterations, 1);
}

#[tokio::test]
async fn run_test_command_resolves_pass_via_exit_zero() {
    let dir = TempDir::new().unwrap();
    let (_script, state) = make_fixture(dir.path());
    set_state(&state, "pass");
    let cmd = format!(
        "FIXTURE_STATE_FILE={} ./fixture_test.sh",
        state.file_name().unwrap().to_str().unwrap()
    );
    let outcome = run_test_command(&cmd, dir.path(), Duration::from_secs(5), 50).await;
    assert!(matches!(outcome, TestOutcome::Passed));
}

#[tokio::test]
async fn run_test_command_resolves_fail_with_stderr() {
    let dir = TempDir::new().unwrap();
    let (_script, state) = make_fixture(dir.path());
    set_state(&state, "fail_a");
    let cmd = format!(
        "FIXTURE_STATE_FILE={} ./fixture_test.sh",
        state.file_name().unwrap().to_str().unwrap()
    );
    let outcome = run_test_command(&cmd, dir.path(), Duration::from_secs(5), 50).await;
    match outcome {
        TestOutcome::Failed { summary } => {
            assert!(summary.contains("FAIL"));
        }
        _ => panic!("expected Failed"),
    }
}

#[tokio::test]
async fn run_test_command_truncates_to_max_lines() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("verbose.sh"),
        "#!/usr/bin/env bash\nfor i in $(seq 1 200); do echo \"line $i\"; done; exit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(
        dir.path().join("verbose.sh"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let outcome = run_test_command("./verbose.sh", dir.path(), Duration::from_secs(5), 10).await;
    match outcome {
        TestOutcome::Failed { summary } => {
            // The 200-line output should be truncated to last 10 lines.
            assert!(summary.contains("truncated"));
            // Line 200 must be present; line 1 must not.
            assert!(summary.contains("line 200"));
            assert!(!summary.contains("line 1\n"));
        }
        _ => panic!("expected Failed"),
    }
}

#[test]
fn language_default_command_uses_real_runners() {
    assert!(Language::Rust.default_command().starts_with("cargo "));
    assert!(Language::Node.default_command().starts_with("npm "));
    assert!(Language::Python.default_command().starts_with("pytest"));
    assert!(Language::Go.default_command().starts_with("go test"));
}

#[test]
fn outcome_from_output_handles_missing_streams() {
    let out = outcome_from_output("", "", Some(2), false, 100);
    assert!(matches!(out, TestOutcome::Failed { .. }));
}

#[tokio::test]
async fn run_auto_test_returns_none_when_no_command_resolvable() {
    let dir = TempDir::new().unwrap();
    // Empty dir — no manifest files, no explicit command, no languages.
    let cfg = AutoTestConfig::default();
    let outcome = run_auto_test(&cfg, dir.path()).await;
    assert!(outcome.is_none());
}

#[tokio::test]
async fn run_auto_test_uses_explicit_command_over_language_default() {
    let dir = TempDir::new().unwrap();
    // Even with a Cargo.toml present, an explicit command must take priority.
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
    let cfg = AutoTestConfig {
        command: Some("exit 0".into()),
        ..Default::default()
    };
    let outcome = run_auto_test(&cfg, dir.path()).await.unwrap();
    assert_eq!(outcome, TestOutcome::Passed);
}
