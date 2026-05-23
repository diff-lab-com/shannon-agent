//! Integration tests for session recording & replay infrastructure.
//!
//! Tier 2: Record → replay round-trip tests with mockito-backed LLM
//! Tier 3: Fixture-based replay verification tests

use serde_json::json;
use shannon_core::QueryEvent;
use shannon_core::recording::{SessionRecorder, SessionReplayer, ToolChainTest};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("sessions")
}

// ── Tier 2: Record & Replay Round-Trip Tests ──────────────────────────

#[test]
fn test_record_and_replay_session() {
    let dir = std::env::temp_dir().join(format!("shannon_replay_test_{}", uuid::Uuid::new_v4()));

    // Record a session
    let mut recorder = SessionRecorder::new("round-trip-test", "test-model", dir.clone());
    recorder.record_user_message("read the file");
    recorder.record_llm_exchange(
        &json!({"model": "test", "messages": [{"role": "user", "content": "read"}]}),
        &json!({"content": [{"type": "text", "text": "checking"}]}),
    );

    let qid = uuid::Uuid::new_v4();
    recorder.record_query_event(&QueryEvent::Started { query_id: qid });
    recorder.record_query_event(&QueryEvent::Text {
        query_id: qid,
        content: "Let me check".to_string(),
    });
    recorder.record_query_event(&QueryEvent::ToolUseRequest {
        query_id: qid,
        tool_use_id: "tu_1".to_string(),
        tool_name: "Read".to_string(),
        tool_input: json!({"path": "src/main.rs"}),
    });
    recorder.record_query_event(&QueryEvent::ToolUseResult {
        query_id: qid,
        tool_use_id: "tu_1".to_string(),
        tool_name: "Read".to_string(),
        result: "fn main() {}".to_string(),
        is_error: false,
    });
    recorder.record_query_event(&QueryEvent::Completed { query_id: qid });

    let path = recorder.finish(300).unwrap();

    // Replay and verify
    let replayer = SessionReplayer::load_from_file(&path).unwrap();
    assert_eq!(replayer.total_turns(), 1);
    assert_eq!(replayer.user_messages().len(), 1);
    assert_eq!(replayer.user_messages()[0].1, "read the file");
    assert_eq!(replayer.llm_responses().len(), 1);
    assert_eq!(replayer.tool_calls().len(), 1);
    assert_eq!(replayer.tool_calls()[0].tool, "Read");
    assert_eq!(replayer.tool_calls()[0].result, "fn main() {}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_multi_turn_record_and_replay() {
    let dir = std::env::temp_dir().join(format!("shannon_replay_test_{}", uuid::Uuid::new_v4()));
    let mut recorder = SessionRecorder::new("multi-turn-test", "test-model", dir.clone());

    // Turn 1
    recorder.record_user_message("hello");
    recorder.record_llm_exchange(&json!({"turn": 1}), &json!({"text": "hi there"}));

    // Turn 2
    recorder.record_user_message("fix the error");
    recorder.record_llm_exchange(&json!({"turn": 2}), &json!({"text": "let me check"}));

    let qid = uuid::Uuid::new_v4();
    recorder.record_query_event(&QueryEvent::Started { query_id: qid });
    recorder.record_query_event(&QueryEvent::ToolUseRequest {
        query_id: qid,
        tool_use_id: "tu_1".to_string(),
        tool_name: "Bash".to_string(),
        tool_input: json!({"command": "cargo check"}),
    });
    recorder.record_query_event(&QueryEvent::ToolUseResult {
        query_id: qid,
        tool_use_id: "tu_1".to_string(),
        tool_name: "Bash".to_string(),
        result: "error[E0425]".to_string(),
        is_error: true,
    });
    recorder.record_query_event(&QueryEvent::Completed { query_id: qid });

    // Turn 3
    recorder.record_user_message("thanks");
    recorder.record_llm_exchange(&json!({"turn": 3}), &json!({"text": "you're welcome"}));

    let path = recorder.finish(1000).unwrap();
    let replayer = SessionReplayer::load_from_file(&path).unwrap();

    assert_eq!(replayer.total_turns(), 3);
    assert_eq!(replayer.user_messages().len(), 3);
    assert_eq!(replayer.llm_responses().len(), 3);
    assert_eq!(replayer.tool_calls().len(), 1);
    assert!(replayer.tool_calls()[0].is_error);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_error_recovery_record_and_replay() {
    let dir = std::env::temp_dir().join(format!("shannon_replay_test_{}", uuid::Uuid::new_v4()));
    let mut recorder = SessionRecorder::new("error-test", "test-model", dir.clone());

    recorder.record_user_message("run tests");

    let qid = uuid::Uuid::new_v4();
    recorder.record_query_event(&QueryEvent::Started { query_id: qid });

    // First tool call fails
    recorder.record_query_event(&QueryEvent::ToolUseRequest {
        query_id: qid,
        tool_use_id: "tu_1".to_string(),
        tool_name: "Bash".to_string(),
        tool_input: json!({"command": "cargo test"}),
    });
    recorder.record_query_event(&QueryEvent::ToolUseResult {
        query_id: qid,
        tool_use_id: "tu_1".to_string(),
        tool_name: "Bash".to_string(),
        result: "test failed".to_string(),
        is_error: true,
    });

    // Second tool call succeeds
    recorder.record_query_event(&QueryEvent::ToolUseRequest {
        query_id: qid,
        tool_use_id: "tu_2".to_string(),
        tool_name: "Edit".to_string(),
        tool_input: json!({"path": "tests/main.rs"}),
    });
    recorder.record_query_event(&QueryEvent::ToolUseResult {
        query_id: qid,
        tool_use_id: "tu_2".to_string(),
        tool_name: "Edit".to_string(),
        result: "ok".to_string(),
        is_error: false,
    });

    recorder.record_query_event(&QueryEvent::Completed { query_id: qid });

    let path = recorder.finish(500).unwrap();
    let replayer = SessionReplayer::load_from_file(&path).unwrap();

    let tools = replayer.tool_calls();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].tool, "Bash");
    assert!(tools[0].is_error);
    assert_eq!(tools[1].tool, "Edit");
    assert!(!tools[1].is_error);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_into_vcr_creates_replayable_recordings() {
    let dir = std::env::temp_dir().join(format!("shannon_replay_test_{}", uuid::Uuid::new_v4()));
    let mut recorder = SessionRecorder::new("vcr-test", "test-model", dir.clone());

    recorder.record_user_message("hello");
    recorder.record_llm_exchange(
        &json!({"model": "test", "messages": [{"role": "user", "content": "hello"}]}),
        &json!({"content": [{"type": "text", "text": "hi"}]}),
    );
    recorder.record_user_message("goodbye");
    recorder.record_llm_exchange(
        &json!({"model": "test", "messages": [{"role": "user", "content": "goodbye"}]}),
        &json!({"content": [{"type": "text", "text": "bye"}]}),
    );

    let path = recorder.finish(200).unwrap();
    let replayer = SessionReplayer::load_from_file(&path).unwrap();

    let vcr = replayer.into_vcr();
    assert_eq!(
        vcr.len(),
        2,
        "Vcr should have 2 recordings (one per exchange)"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ── Tier 3: Fixture-Based Replay Tests ────────────────────────────────

#[test]
fn test_edit_fix_cycle_fixture() {
    let path = fixtures_dir().join("edit_fix_cycle.jsonl");
    let replayer =
        SessionReplayer::load_from_file(&path).expect("Failed to load edit_fix_cycle fixture");

    assert_eq!(replayer.total_turns(), 1);
    assert_eq!(replayer.user_messages().len(), 1);
    assert_eq!(replayer.user_messages()[0].1, "fix the compilation error");

    let tools = replayer.tool_calls();
    assert_eq!(tools.len(), 2, "Should have Read + Edit");
    assert_eq!(tools[0].tool, "Read");
    assert_eq!(tools[1].tool, "Edit");
    assert!(!tools[0].is_error);
    assert!(!tools[1].is_error);

    let events = replayer.query_events();
    assert!(
        events.len() >= 4,
        "Should have Started, Text, tool events, Completed"
    );

    let vcr = replayer.into_vcr();
    assert_eq!(vcr.len(), 1, "Should have 1 LLM exchange");
}

#[test]
fn test_error_recovery_fixture() {
    let path = fixtures_dir().join("error_recovery.jsonl");
    let replayer =
        SessionReplayer::load_from_file(&path).expect("Failed to load error_recovery fixture");

    assert_eq!(replayer.total_turns(), 1);

    let tools = replayer.tool_calls();
    assert_eq!(tools.len(), 3, "Should have Bash(fail) + Read + Edit");
    assert_eq!(tools[0].tool, "Bash");
    assert!(tools[0].is_error, "First tool call should be an error");
    assert_eq!(tools[1].tool, "Read");
    assert_eq!(tools[2].tool, "Edit");

    // Verify error result contains useful info
    assert!(tools[0].result.contains("error"));
}

#[test]
fn test_multi_file_fixture() {
    let path = fixtures_dir().join("multi_file_refactor.jsonl");
    let replayer =
        SessionReplayer::load_from_file(&path).expect("Failed to load multi_file_refactor fixture");

    assert_eq!(replayer.total_turns(), 1);

    let tools = replayer.tool_calls();
    assert_eq!(tools.len(), 6, "Should have 3 Reads + 3 Edits");

    let read_count = tools.iter().filter(|t| t.tool == "Read").count();
    let edit_count = tools.iter().filter(|t| t.tool == "Edit").count();
    assert_eq!(read_count, 3);
    assert_eq!(edit_count, 3);

    // Verify read-before-edit ordering: all reads come before edits
    let first_edit_idx = tools.iter().position(|t| t.tool == "Edit").unwrap();
    let last_read_idx = tools.iter().rposition(|t| t.tool == "Read").unwrap();
    assert!(
        last_read_idx < first_edit_idx,
        "All reads should come before edits"
    );

    // Verify Vcr has the LLM exchange
    let vcr = replayer.into_vcr();
    assert_eq!(vcr.len(), 1);
}

// ── Tool Chain Verification with Fixtures ─────────────────────────────

#[test]
fn test_fixture_tool_chain_matches_edit_fix() {
    let path = fixtures_dir().join("edit_fix_cycle.jsonl");
    let replayer = SessionReplayer::load_from_file(&path).unwrap();

    let tool_calls = replayer.tool_calls();
    let r1 = json!({"path": "src/main.rs"});
    let e1 = json!({"path": "src/main.rs","old":"let x = 1","new":"let y = 1"});
    let actual: Vec<(&str, &serde_json::Value, &str, bool)> = vec![
        (
            &tool_calls[0].tool,
            &tool_calls[0].input,
            &tool_calls[0].result,
            tool_calls[0].is_error,
        ),
        (
            &tool_calls[1].tool,
            &tool_calls[1].input,
            &tool_calls[1].result,
            tool_calls[1].is_error,
        ),
    ];

    let chain = ToolChainTest::new()
        .expect_tool("Read", r1)
        .respond_with("fn main() { let x = 1\n}")
        .expect_tool("Edit", e1)
        .respond_with("edited successfully");

    let result = chain.verify_against(&actual);
    assert!(
        result.passed,
        "Fixture tool calls should match: {:?}",
        result.errors
    );
}

#[test]
fn test_all_fixtures_loadable() {
    let dir = fixtures_dir();
    let fixtures = vec![
        "edit_fix_cycle.jsonl",
        "error_recovery.jsonl",
        "multi_file_refactor.jsonl",
    ];

    for fixture_name in &fixtures {
        let path = dir.join(fixture_name);
        let replayer = SessionReplayer::load_from_file(&path)
            .unwrap_or_else(|e| panic!("Failed to load {}: {e}", fixture_name));
        assert!(
            replayer.total_turns() > 0,
            "{} should have at least 1 turn",
            fixture_name
        );
        assert!(
            !replayer.tool_calls().is_empty(),
            "{} should have tool calls",
            fixture_name
        );
    }
}
