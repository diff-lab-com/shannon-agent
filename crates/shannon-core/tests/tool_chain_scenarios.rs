//! Tool chain scenario tests over L0 session-log fixtures (§4.6 cutover).
//!
//! Same orchestration assertions as before, but trajectories now come from
//! typed `events.jsonl` rows (`tool/call` paired with its `tool/result`),
//! exercising the production reader instead of legacy recording entries.

use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

fn fixture_root(name: &str) -> PathBuf {
    // Call sites keep the legacy `.jsonl` names; strip it for the dir.
    let stem = name.strip_suffix(".jsonl").unwrap_or(name);
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/session_l0")
        .join(stem)
}

/// Load `(tool_name, parsed_input, is_error)` triples for one fixture.
///
/// Calls and results arrive as separate vocabulary rows in real logs; this
/// pairs them by `tool_use_id`, preserving call order for subsequence
/// assertions while attaching each result's error flag to its request.
fn load_tool_calls(name: &str) -> Vec<(String, Value, bool)> {
    use shannon_types::session_event::{SessionEventBody, ToolCallPayload, ToolResultPayload};

    let path = fixture_root(name).join("events.jsonl");
    let reader =
        shannon_core::session_log::SessionLogReader::open(&path).expect("open events.jsonl");
    let events = reader.read_events(false).expect("read events");

    let mut calls: Vec<(String, Value, bool)> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();

    for event in &events {
        match &event.body {
            SessionEventBody::ToolCall(ToolCallPayload {
                tool_use_id,
                tool_name,
                arguments,
                ..
            }) => {
                let input: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
                positions.insert(tool_use_id.clone(), calls.len());
                calls.push((tool_name.clone(), input, false));
            }
            SessionEventBody::ToolResult(ToolResultPayload {
                tool_use_id,
                is_error,
                ..
            }) => {
                if let Some(index) = positions.get(tool_use_id) {
                    if let Some(call) = calls.get_mut(*index) {
                        call.2 = *is_error;
                    }
                }
            }
            _ => {}
        }
    }

    calls
}

fn load_tool_names(name: &str) -> Vec<String> {
    load_tool_calls(name)
        .into_iter()
        .map(|(t, _, _)| t)
        .collect()
}

/// Check if a tool name list contains a given tool.
fn contains_tool(tools: &[String], name: &str) -> bool {
    tools.iter().any(|t| t == name)
}

/// Count occurrences of a tool name.
fn count_tool(tools: &[String], name: &str) -> usize {
    tools.iter().filter(|t| **t == *name).count()
}

// ── Search-driven fix: Grep → Read → Edit → Bash ──

#[test]
fn test_search_driven_fix_tool_chain() {
    let tools = load_tool_names("search_driven_fix.jsonl");
    assert!(
        tools.first().map(|t| t.as_str()) == Some("Grep"),
        "Expected Grep first, got {tools:?}"
    );
    assert!(contains_tool(&tools, "Read"), "Expected Read in chain");
    assert!(contains_tool(&tools, "Edit"), "Expected Edit in chain");
    assert!(contains_tool(&tools, "Bash"), "Expected Bash in chain");
}

#[test]
fn test_search_driven_fix_no_errors() {
    let calls = load_tool_calls("search_driven_fix.jsonl");
    for (tool, _, is_error) in &calls {
        assert!(!is_error, "Tool {tool} had unexpected error");
    }
}

// ── Parallel tool use: 3 Reads then Edits ──

#[test]
fn test_parallel_tool_use_has_multiple_reads() {
    let tools = load_tool_names("parallel_tool_use.jsonl");
    let read_count = count_tool(&tools, "Read");
    assert!(
        read_count >= 3,
        "Expected >= 3 parallel reads, got {read_count}"
    );
}

#[test]
fn test_parallel_tool_use_no_errors() {
    let calls = load_tool_calls("parallel_tool_use.jsonl");
    for (tool, _, is_error) in &calls {
        assert!(!is_error, "Tool {tool} had unexpected error");
    }
}

// ── Cascading edits: Grep → Read → Edit×3 ──

#[test]
fn test_cascading_edits_multiple_edits() {
    let tools = load_tool_names("cascading_edits.jsonl");
    let edit_count = count_tool(&tools, "Edit");
    assert!(
        edit_count >= 3,
        "Expected >= 3 cascading edits, got {edit_count}"
    );
    assert!(contains_tool(&tools, "Grep"), "Expected Grep in chain");
    assert!(contains_tool(&tools, "Read"), "Expected Read in chain");
    assert!(contains_tool(&tools, "Bash"), "Expected Bash verification");
}

// ── TDD cycle: Write(test) → Bash(fail) → Write(impl) → Bash(pass) ──

#[test]
fn test_tdd_cycle_has_write_and_bash() {
    let tools = load_tool_names("tdd_cycle.jsonl");
    assert!(
        contains_tool(&tools, "Write"),
        "Expected Write for test file"
    );
    assert!(
        contains_tool(&tools, "Bash"),
        "Expected Bash for running tests"
    );
    let bash_count = count_tool(&tools, "Bash");
    assert!(
        bash_count >= 2,
        "Expected >= 2 Bash calls (fail then pass), got {bash_count}"
    );
}

// ── Error cascade recovery: Bash(err) → Read → Edit → Bash(ok) ──

#[test]
fn test_error_cascade_has_error_then_recovery() {
    let calls = load_tool_calls("error_cascade_recovery.jsonl");
    let has_error = calls.iter().any(|(_, _, e)| *e);
    assert!(has_error, "Expected at least one error in cascade");
    // Last Bash should succeed
    let last_bash = calls.iter().rev().find(|(t, _, _)| t == "Bash");
    assert!(last_bash.is_some(), "Expected final Bash");
    assert!(!last_bash.unwrap().2, "Expected final Bash to succeed");
}

// ── Permission denied: Bash(deny) → Read → Bash(ok) ──

#[test]
fn test_permission_denied_has_error_then_alternative() {
    let calls = load_tool_calls("permission_denied.jsonl");
    let first_error = calls.iter().find(|(_, _, e)| *e);
    assert!(first_error.is_some(), "Expected permission denied error");
    assert_eq!(first_error.unwrap().0, "Bash", "Expected Bash to be denied");
    // Should have successful Read after
    let has_read = calls.iter().any(|(t, _, e)| t == "Read" && !e);
    assert!(has_read, "Expected successful Read after denied Bash");
}

// ── Tool chain depth 5: Grep → Read → Grep → Edit → Bash ──

#[test]
fn test_tool_chain_depth_5_has_5_tools() {
    let tools = load_tool_names("tool_chain_depth_5.jsonl");
    assert!(
        tools.len() >= 5,
        "Expected >= 5 tools in depth-5 chain, got {}",
        tools.len()
    );
    assert!(contains_tool(&tools, "Grep"), "Expected Grep");
    assert!(contains_tool(&tools, "Read"), "Expected Read");
    assert!(contains_tool(&tools, "Edit"), "Expected Edit");
    assert!(contains_tool(&tools, "Bash"), "Expected Bash");
}

// ── Multi-file search replace: Grep → Read×4 → Edit×4 ──

#[test]
fn test_multi_file_search_replace() {
    let tools = load_tool_names("multi_file_search_replace.jsonl");
    let read_count = count_tool(&tools, "Read");
    let edit_count = count_tool(&tools, "Edit");
    assert!(read_count >= 2, "Expected >= 2 reads, got {read_count}");
    assert!(edit_count >= 2, "Expected >= 2 edits, got {edit_count}");
    assert!(contains_tool(&tools, "Grep"), "Expected Grep");
}

// ── Refactoring safety: Read → Edit → Bash(pass) ──

#[test]
fn test_refactoring_safety_tests_pass() {
    let calls = load_tool_calls("refactoring_safety.jsonl");
    let last_bash = calls.iter().rev().find(|(t, _, _)| t == "Bash");
    assert!(last_bash.is_some(), "Expected Bash verification");
    assert!(!last_bash.unwrap().2, "Tests should pass after refactor");
}

// ── Git workflow: Bash(git status) → Bash(git diff) → Bash(git commit) ──

#[test]
fn test_git_workflow_has_git_commands() {
    let calls = load_tool_calls("git_workflow.jsonl");
    let git_calls: Vec<_> = calls
        .iter()
        .filter(|(t, _, _)| t == "Bash")
        .filter_map(|(_, input, _)| input.get("command").and_then(|c| c.as_str()))
        .collect();
    assert!(
        git_calls.iter().any(|c| c.contains("git status")),
        "Expected git status"
    );
    assert!(
        git_calls.iter().any(|c| c.contains("git diff")),
        "Expected git diff"
    );
    assert!(
        git_calls.iter().any(|c| c.contains("git commit")),
        "Expected git commit"
    );
}

// ── Code generation: Write → Bash(check) ──

#[test]
fn test_code_generation_creates_file() {
    let calls = load_tool_calls("code_generation.jsonl");
    assert!(
        calls.iter().any(|(t, _, _)| t == "Write"),
        "Expected Write to create new file"
    );
    assert!(
        calls.iter().any(|(t, _, _)| t == "Bash"),
        "Expected Bash to verify compilation"
    );
}

// ── Snapshot tool chain test using snapshot helpers ──

#[test]
fn test_snapshot_tool_chain_from_fixture() {
    use shannon_core::testing::snapshot::snapshot_tool_chain;

    let calls: Vec<(String, Value, String, bool)> = load_tool_calls("search_driven_fix.jsonl")
        .into_iter()
        .map(|(tool, input, is_error)| (tool, input, String::new(), is_error))
        .collect();

    let snapshot = snapshot_tool_chain(&calls);
    assert!(
        snapshot.contains("tool_chain:"),
        "Expected tool_chain header"
    );
    assert!(snapshot.contains("Grep"), "Expected Grep in snapshot");
    assert!(snapshot.contains("[OK]"), "Expected OK status");
}
