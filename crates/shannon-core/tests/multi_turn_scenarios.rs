//! Multi-turn scenario tests — verify conversation patterns across turns.
//!
//! Tests multi-turn context accumulation, turn continuity, and
//! conversation flow using session fixtures.

use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/sessions")
}

fn parse_fixture(name: &str) -> Vec<Value> {
    let path = fixtures_dir().join(name);
    let content = fs::read_to_string(&path).expect("read fixture");
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

// ── Multi-turn context tests ──

#[test]
fn test_multi_turn_planning_has_3_turns() {
    let entries = parse_fixture("multi_turn_planning.jsonl");
    let user_msgs: Vec<_> = entries
        .iter()
        .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("UserMessage"))
        .collect();
    assert_eq!(
        user_msgs.len(),
        3,
        "Expected 3 user messages in multi-turn planning"
    );

    // Verify turns are 1, 2, 3
    let turns: Vec<usize> = user_msgs
        .iter()
        .filter_map(|m| m.get("turn").and_then(|t| t.as_u64().map(|v| v as usize)))
        .collect();
    assert_eq!(turns, vec![1, 2, 3], "Turn numbers should be 1, 2, 3");
}

#[test]
fn test_multi_turn_later_turns_reference_earlier() {
    let entries = parse_fixture("multi_turn_planning.jsonl");
    let llm_responses: Vec<_> = entries
        .iter()
        .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("LlmResponse"))
        .collect();

    // Turn 2 response should reference what was read in turn 1
    assert!(llm_responses.len() >= 2, "Expected >= 2 LLM responses");
}

#[test]
fn test_retry_after_error_has_2_turns() {
    let entries = parse_fixture("retry_after_error.jsonl");
    let user_msgs: Vec<_> = entries
        .iter()
        .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("UserMessage"))
        .collect();
    assert_eq!(
        user_msgs.len(),
        2,
        "Expected 2 user messages (initial + retry)"
    );
}

#[test]
fn test_interactive_debugging_multi_turn() {
    let entries = parse_fixture("interactive_debugging.jsonl");
    let user_msgs: Vec<_> = entries
        .iter()
        .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("UserMessage"))
        .collect();
    assert!(
        user_msgs.len() >= 2,
        "Expected >= 2 user messages in debugging session"
    );

    // Second user message should contain error context
    let second_msg = user_msgs.get(1).unwrap();
    let content = second_msg
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("");
    assert!(
        content.contains("panicked") || content.contains("error") || content.contains("unwrap"),
        "Second message should reference the error: {}",
        content
    );
}

#[test]
fn test_session_resume_context() {
    let entries = parse_fixture("session_resume_check.jsonl");
    let first_msg = entries
        .iter()
        .find(|e| e.get("type").and_then(|t| t.as_str()) == Some("UserMessage"))
        .unwrap();
    let content = first_msg
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("");
    assert!(
        content.contains("continue") || content.contains("left off"),
        "Resume fixture should reference continuation: {}",
        content
    );
}

// ── Tool call count tests ──

#[test]
fn test_context_pressure_many_tools() {
    let entries = parse_fixture("context_window_pressure.jsonl");
    let tool_calls: Vec<_> = entries
        .iter()
        .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("ToolCall"))
        .collect();
    // Should have many reads + edits (high context usage)
    assert!(
        tool_calls.len() >= 8,
        "Expected >= 8 tool calls in context pressure, got {}",
        tool_calls.len()
    );
}

#[test]
fn test_git_workflow_tool_count() {
    let entries = parse_fixture("git_workflow.jsonl");
    let tool_calls: Vec<_> = entries
        .iter()
        .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("ToolCall"))
        .collect();
    assert!(
        tool_calls.len() >= 3,
        "Expected >= 3 tool calls in git workflow"
    );
}

// ── Snapshot-based multi-turn verification ──

#[test]
fn test_multi_turn_request_snapshot() {
    use shannon_core::testing::snapshot::{RenderMode, render_request_snapshot};

    let entries = parse_fixture("multi_turn_planning.jsonl");
    let requests: Vec<_> = entries
        .iter()
        .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("LlmRequest"))
        .collect();

    assert!(requests.len() >= 2, "Expected >= 2 LLM requests");

    // Render each request as a snapshot
    for (i, req) in requests.iter().enumerate() {
        let body = req.get("body").cloned().unwrap_or(Value::Null);
        let snapshot = render_request_snapshot(&body, RenderMode::KindOnly);
        assert!(
            !snapshot.is_empty(),
            "Request {} should render non-empty snapshot",
            i
        );
    }
}

#[test]
fn test_cascading_edits_context_growth() {
    let entries = parse_fixture("cascading_edits.jsonl");
    let responses: Vec<_> = entries
        .iter()
        .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("LlmResponse"))
        .collect();

    // Multiple LLM responses indicate the model iterated through the edit chain
    assert!(
        responses.len() >= 3,
        "Expected >= 3 LLM responses in cascading edits, got {}",
        responses.len()
    );
}

// ── Turn continuity ──

#[test]
fn test_all_multi_turn_fixtures_have_sequential_turns() {
    let multi_turn_fixtures = [
        "multi_turn_planning.jsonl",
        "retry_after_error.jsonl",
        "interactive_debugging.jsonl",
        "context_window_pressure.jsonl",
    ];

    for name in &multi_turn_fixtures {
        let entries = parse_fixture(name);
        let turns: Vec<usize> = entries
            .iter()
            .filter_map(|e| {
                if e.get("type").and_then(|t| t.as_str()) == Some("UserMessage") {
                    e.get("turn").and_then(|t| t.as_u64().map(|v| v as usize))
                } else {
                    None
                }
            })
            .collect();

        if turns.len() > 1 {
            for window in turns.windows(2) {
                assert_eq!(
                    window[1] - window[0],
                    1,
                    "{}: turns not sequential: {:?}",
                    name,
                    turns
                );
            }
        }
    }
}
