//! Snapshot regression tests using insta for deterministic API request shape verification.
//!
//! These tests construct API request bodies and compare them against stored snapshots,
//! catching unintended changes in prompt construction, context management,
//! and tool definition structure.

use insta::assert_snapshot;
use serde_json::{json, Value};
use shannon_core::testing::snapshot::{render_request_snapshot, RenderMode, snapshot_tool_chain};

// ── Basic request shape snapshots ──

#[test]
fn test_basic_text_request_snapshot() {
    let request = json!({
        "model": "test-model",
        "system": "You are a helpful coding assistant.",
        "messages": [
            {"role": "user", "content": "Hello, how are you?"},
            {"role": "assistant", "content": "I'm doing well! How can I help you today?"}
        ]
    });
    let snapshot = render_request_snapshot(&request, RenderMode::FullText);
    assert_snapshot!("basic_text_request", snapshot);
}

#[test]
fn test_tool_use_request_snapshot() {
    let request = json!({
        "model": "test-model",
        "system": "You are a helpful coding assistant.",
        "tools": [
            {
                "name": "Read",
                "description": "Read the contents of a file at the given path",
                "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}}
            },
            {
                "name": "Edit",
                "description": "Edit a file by replacing old text with new text",
                "input_schema": {"type": "object", "properties": {"path": {"type": "string"}, "old": {"type": "string"}, "new": {"type": "string"}}}
            },
            {
                "name": "Bash",
                "description": "Execute a bash command",
                "input_schema": {"type": "object", "properties": {"command": {"type": "string"}}}
            }
        ],
        "messages": [
            {"role": "user", "content": "Fix the bug in main.rs"}
        ]
    });
    let snapshot = render_request_snapshot(&request, RenderMode::KindOnly);
    assert_snapshot!("tool_use_request", snapshot);
}

#[test]
fn test_multi_turn_context_snapshot() {
    let request = json!({
        "model": "test-model",
        "messages": [
            {"role": "user", "content": "Read src/main.rs"},
            {"role": "assistant", "content": [
                {"type": "text", "text": "Let me read that file."},
                {"type": "tool_use", "id": "tu_1", "name": "Read", "input": {"path": "src/main.rs"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "tu_1", "content": "fn main() { println!(\"hello\"); }"}
            ]},
            {"role": "assistant", "content": "The file looks good. It prints hello."},
            {"role": "user", "content": "Now add error handling"}
        ]
    });
    let snapshot = render_request_snapshot(&request, RenderMode::RedactedText);
    assert_snapshot!("multi_turn_context", snapshot);
}

#[test]
fn test_compacted_context_snapshot() {
    // Simulate a compacted context with summary
    let request = json!({
        "model": "test-model",
        "system": "Previous context summary: User asked to fix a bug in parser.rs. Read the file, found the issue was a missing null check. Applied the fix and verified tests pass.",
        "messages": [
            {"role": "user", "content": "now do the same for serializer.rs"}
        ]
    });
    let snapshot = render_request_snapshot(&request, RenderMode::FullText);
    assert_snapshot!("compacted_context", snapshot);
}

#[test]
fn test_system_prompt_with_tools_snapshot() {
    let tools: Vec<Value> = (0..8).map(|i| {
        json!({
            "name": format!("Tool{}", i),
            "description": format!("Description for tool {}", i)
        })
    }).collect();

    let request = json!({
        "model": "test-model",
        "system": [
            {"type": "text", "text": "You are a coding assistant with access to the following tools."},
            {"type": "text", "text": "Always explain your reasoning before making changes."}
        ],
        "tools": tools,
        "messages": [
            {"role": "user", "content": "Refactor the auth module"}
        ]
    });
    let snapshot = render_request_snapshot(&request, RenderMode::KindOnly);
    assert_snapshot!("system_prompt_with_tools", snapshot);
}

#[test]
fn test_tool_result_injection_snapshot() {
    // After tool execution, the result is injected back
    let request = json!({
        "model": "test-model",
        "messages": [
            {"role": "user", "content": "Fix the failing test"},
            {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "Let me check the test file first"},
                {"type": "text", "text": "Let me check the test file."},
                {"type": "tool_use", "id": "tu_1", "name": "Read", "input": {"path": "tests/test.rs"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "tu_1", "content": "assert_eq!(add(1,1), 3); // wrong expected value"}
            ]}
        ]
    });
    let snapshot = render_request_snapshot(&request, RenderMode::RedactedText);
    assert_snapshot!("tool_result_injection", snapshot);
}

// ── Tool chain snapshots ──

#[test]
fn test_tool_chain_snapshot() {
    let calls = vec![
        ("Read".to_string(), json!({"path": "src/main.rs"}), "fn main() {}".to_string(), false),
        ("Edit".to_string(), json!({"path": "src/main.rs", "old": "fn main() {}", "new": "fn main() { println!(\"hello\"); }"}), "ok".to_string(), false),
        ("Bash".to_string(), json!({"command": "cargo test"}), "1 test passed".to_string(), false),
    ];
    let snapshot = snapshot_tool_chain(&calls);
    assert_snapshot!("tool_chain_basic", snapshot);
}

#[test]
fn test_tool_chain_with_errors_snapshot() {
    let calls = vec![
        ("Bash".to_string(), json!({"command": "cargo build"}), "error[E0425]: unresolved".to_string(), true),
        ("Read".to_string(), json!({"path": "src/lib.rs"}), "pub fn broken() {}".to_string(), false),
        ("Edit".to_string(), json!({"path": "src/lib.rs"}), "ok".to_string(), false),
        ("Bash".to_string(), json!({"command": "cargo build"}), "Finished dev".to_string(), false),
    ];
    let snapshot = snapshot_tool_chain(&calls);
    assert_snapshot!("tool_chain_with_errors", snapshot);
}
