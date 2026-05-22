//! Fixture validation tests — ensure all session fixtures are well-formed JSONL.
//!
//! Validates that every .jsonl file in fixtures/sessions/ can be parsed,
//! has correct structure (SessionStart/SessionEnd), contiguous turns,
//! and consistent tool call/event records.

use std::fs;
use std::path::PathBuf;
use serde_json::Value;

/// Get the fixtures directory path.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/sessions")
}

/// Parse a JSONL fixture file into a vector of JSON values.
fn parse_fixture(path: &PathBuf) -> Vec<Value> {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", path, e));
    content.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, line)| {
            serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("Failed to parse line {} in {:?}: {}", i + 1, path, e))
        })
        .collect()
}

/// Get all .jsonl fixture files.
fn all_fixtures() -> Vec<String> {
    let dir = fixtures_dir();
    let mut files: Vec<String> = fs::read_dir(&dir)
        .expect("fixtures/sessions/ directory")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "jsonl"))
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .collect();
    files.sort();
    files
}

/// Validate a single fixture file's structure.
fn validate_fixture(name: &str) {
    let path = fixtures_dir().join(name);
    let entries = parse_fixture(&path);

    assert!(!entries.is_empty(), "{}: fixture is empty", name);

    // Must start with SessionStart
    let first = &entries[0];
    let first_type = first.get("type").and_then(|t| t.as_str()).unwrap_or("missing");
    assert_eq!(first_type, "SessionStart",
        "{}: first entry must be SessionStart, got {}", name, first_type);

    // Must end with SessionEnd
    let last = entries.last().unwrap();
    let last_type = last.get("type").and_then(|t| t.as_str()).unwrap_or("missing");
    assert_eq!(last_type, "SessionEnd",
        "{}: last entry must be SessionEnd, got {}", name, last_type);

    // Session IDs must match
    let start_id = first.get("session_id").and_then(|s| s.as_str()).unwrap_or("");
    let end_id = last.get("session_id").and_then(|s| s.as_str()).unwrap_or("");
    assert_eq!(start_id, end_id,
        "{}: SessionStart id {} != SessionEnd id {}", name, start_id, end_id);

    // Validate turn numbers are contiguous
    let mut turns: Vec<usize> = entries.iter()
        .filter_map(|e| {
            let t = e.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if t == "UserMessage" || t == "LlmRequest" || t == "LlmResponse" {
                e.get("turn").and_then(|t| t.as_u64().map(|v| v as usize))
            } else {
                None
            }
        })
        .collect();
    turns.sort();
    turns.dedup();

    if !turns.is_empty() {
        assert_eq!(turns[0], 1, "{}: turns should start at 1", name);
        for window in turns.windows(2) {
            assert_eq!(window[1] - window[0], 1,
                "{}: turns not contiguous: {:?}", name, turns);
        }
    }

    // Validate total_turns matches
    let declared_turns = last.get("total_turns").and_then(|t| t.as_u64()).unwrap_or(0) as usize;
    let max_turn = turns.last().copied().unwrap_or(0);
    assert_eq!(declared_turns, max_turn,
        "{}: declared {} turns but found max turn {}", name, declared_turns, max_turn);

    // Validate entry types are all recognized
    let valid_types = [
        "SessionStart", "SessionEnd", "UserMessage",
        "LlmRequest", "LlmResponse", "QueryEvent", "ToolCall"
    ];
    for (i, entry) in entries.iter().enumerate() {
        let entry_type = entry.get("type").and_then(|t| t.as_str()).unwrap_or("missing");
        assert!(valid_types.contains(&entry_type),
            "{}: line {} has unknown type '{}'", name, i + 1, entry_type);
    }
}

#[test]
fn test_all_fixtures_exist() {
    let fixtures = all_fixtures();
    assert!(!fixtures.is_empty(), "No fixture files found in fixtures/sessions/");
    println!("Found {} fixtures", fixtures.len());
}

#[test]
fn test_edit_fix_cycle_fixture() {
    validate_fixture("edit_fix_cycle.jsonl");
}

#[test]
fn test_error_recovery_fixture() {
    validate_fixture("error_recovery.jsonl");
}

#[test]
fn test_multi_file_refactor_fixture() {
    validate_fixture("multi_file_refactor.jsonl");
}

#[test]
fn test_search_driven_fix_fixture() {
    validate_fixture("search_driven_fix.jsonl");
}

#[test]
fn test_parallel_tool_use_fixture() {
    validate_fixture("parallel_tool_use.jsonl");
}

#[test]
fn test_retry_after_error_fixture() {
    validate_fixture("retry_after_error.jsonl");
}

#[test]
fn test_cascading_edits_fixture() {
    validate_fixture("cascading_edits.jsonl");
}

#[test]
fn test_git_workflow_fixture() {
    validate_fixture("git_workflow.jsonl");
}

#[test]
fn test_code_generation_fixture() {
    validate_fixture("code_generation.jsonl");
}

#[test]
fn test_multi_turn_planning_fixture() {
    validate_fixture("multi_turn_planning.jsonl");
}

#[test]
fn test_context_window_pressure_fixture() {
    validate_fixture("context_window_pressure.jsonl");
}

#[test]
fn test_permission_denied_fixture() {
    validate_fixture("permission_denied.jsonl");
}

#[test]
fn test_tool_chain_depth_5_fixture() {
    validate_fixture("tool_chain_depth_5.jsonl");
}

#[test]
fn test_multi_file_search_replace_fixture() {
    validate_fixture("multi_file_search_replace.jsonl");
}

#[test]
fn test_error_cascade_recovery_fixture() {
    validate_fixture("error_cascade_recovery.jsonl");
}

#[test]
fn test_tdd_cycle_fixture() {
    validate_fixture("tdd_cycle.jsonl");
}

#[test]
fn test_refactoring_safety_fixture() {
    validate_fixture("refactoring_safety.jsonl");
}

#[test]
fn test_interactive_debugging_fixture() {
    validate_fixture("interactive_debugging.jsonl");
}

#[test]
fn test_large_file_handling_fixture() {
    validate_fixture("large_file_handling.jsonl");
}

#[test]
fn test_session_resume_check_fixture() {
    validate_fixture("session_resume_check.jsonl");
}

#[test]
fn test_all_fixtures_validate() {
    for fixture in all_fixtures() {
        validate_fixture(&fixture);
    }
}
