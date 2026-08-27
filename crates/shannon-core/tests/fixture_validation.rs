//! Fixture validation tests — L0 events.jsonl sessions (§4.6 cutover).
//!
//! The legacy RecordingEntry fixtures were converted once (dev-time script)
//! into authoritative-format session logs. These tests pin their structure
//! through the real typed reader: parseability, `session/start` first row,
//! strictly continuous seqs, known kinds only, and tool pairing.

use shannon_core::session_log::SessionLogReader;
use std::path::PathBuf;

/// Get the L0 fixtures root.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/session_l0")
}

/// Read one converted fixture through the real typed reader.
fn load_fixture(name: &str) -> Vec<shannon_types::session_event::SessionEvent> {
    let path = fixtures_dir().join(name).join("events.jsonl");
    let reader =
        SessionLogReader::open(&path).unwrap_or_else(|e| panic!("Failed to open {path:?}: {e}"));
    reader
        .read_events(false)
        .unwrap_or_else(|e| panic!("Failed to read {path:?}: {e}"))
}

/// Every converted session directory.
fn all_fixtures() -> Vec<String> {
    let dir = fixtures_dir();
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .expect("fixtures/session_l0/ directory")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .collect();
    files.sort();
    files
}

/// Validate a single converted fixture.
fn validate_fixture(name: &str) {
    let events = load_fixture(name);

    assert!(!events.is_empty(), "{name}: fixture is empty");

    // First row is the typed session/start.
    assert_eq!(
        events[0].kind().as_str(),
        "session/start",
        "{name}: first row must be session/start"
    );

    // seqs are strictly continuous from 0 and session ids agree with dir name.
    for (expected_seq, event) in events.iter().enumerate() {
        assert_eq!(
            event.seq as usize, expected_seq,
            "{name}: seq broke continuity at {expected_seq}"
        );
        assert_eq!(
            event.session_id,
            format!("fx-{name}"),
            "{name}: session_id drifts mid-log"
        );
    }

    // Tool calls/results pair one-to-one in order.
    let mut open_calls = 0usize;
    let mut tool_calls = 0usize;
    let mut tool_results = 0usize;
    for event in &events {
        match event.kind().as_str() {
            "tool/call" => {
                open_calls += 1;
                tool_calls += 1;
            }
            "tool/result" => {
                assert!(open_calls > 0, "{name}: result before any call");
                open_calls -= 1;
                tool_results += 1;
            }
            _ => {}
        }
    }
    assert_eq!(
        tool_calls, tool_results,
        "{name}: unpaired call/result rows"
    );
}

#[test]
fn test_all_fixtures_exist() {
    let fixtures = all_fixtures();
    assert!(
        !fixtures.is_empty(),
        "No fixture directories found in fixtures/session_l0/"
    );
    println!("Found {} fixtures", fixtures.len());
}

#[test]
fn test_edit_fix_cycle_fixture() {
    validate_fixture("edit_fix_cycle");
}

#[test]
fn test_error_recovery_fixture() {
    validate_fixture("error_recovery");
}

#[test]
fn test_multi_file_refactor_fixture() {
    validate_fixture("multi_file_refactor");
}

#[test]
fn test_search_driven_fix_fixture() {
    validate_fixture("search_driven_fix");
}

#[test]
fn test_parallel_tool_use_fixture() {
    validate_fixture("parallel_tool_use");
}

#[test]
fn test_retry_after_error_fixture() {
    validate_fixture("retry_after_error");
}

#[test]
fn test_cascading_edits_fixture() {
    validate_fixture("cascading_edits");
}

#[test]
fn test_git_workflow_fixture() {
    validate_fixture("git_workflow");
}

#[test]
fn test_code_generation_fixture() {
    validate_fixture("code_generation");
}

#[test]
fn test_multi_turn_planning_fixture() {
    validate_fixture("multi_turn_planning");
}

#[test]
fn test_context_window_pressure_fixture() {
    validate_fixture("context_window_pressure");
}

#[test]
fn test_permission_denied_fixture() {
    validate_fixture("permission_denied");
}

#[test]
fn test_tool_chain_depth_5_fixture() {
    validate_fixture("tool_chain_depth_5");
}

#[test]
fn test_multi_file_search_replace_fixture() {
    validate_fixture("multi_file_search_replace");
}

#[test]
fn test_error_cascade_recovery_fixture() {
    validate_fixture("error_cascade_recovery");
}

#[test]
fn test_tdd_cycle_fixture() {
    validate_fixture("tdd_cycle");
}

#[test]
fn test_refactoring_safety_fixture() {
    validate_fixture("refactoring_safety");
}

#[test]
fn test_interactive_debugging_fixture() {
    validate_fixture("interactive_debugging");
}

#[test]
fn test_large_file_handling_fixture() {
    validate_fixture("large_file_handling");
}

#[test]
fn test_session_resume_check_fixture() {
    validate_fixture("session_resume_check");
}

#[test]
fn test_all_fixtures_validate() {
    for fixture in all_fixtures() {
        validate_fixture(&fixture);
    }
}
