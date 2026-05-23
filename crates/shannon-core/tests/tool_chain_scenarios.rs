//! Tool chain scenario tests — verify tool orchestration patterns from fixtures.
//!
//! These tests load session fixtures and validate that tool call sequences
//! follow expected patterns (search-driven fix, parallel reads, cascading edits, etc.).

use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/sessions")
}

fn load_tool_calls(name: &str) -> Vec<(String, Value, bool)> {
    let path = fixtures_dir().join(name);
    let content = fs::read_to_string(&path).expect("read fixture");
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let v: Value = serde_json::from_str(line).ok()?;
            if v.get("type").and_then(|t| t.as_str()) == Some("ToolCall") {
                let tool = v
                    .get("tool")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = v.get("input").cloned().unwrap_or(Value::Null);
                let is_error = v.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false);
                Some((tool, input, is_error))
            } else {
                None
            }
        })
        .collect()
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
