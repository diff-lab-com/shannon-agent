//! Performance regression tests for shannon-core.
//!
//! These tests verify that key operations stay within acceptable time bounds.
//! They use `#[test]` with `std::time::Instant` for timing assertions.
//!
//! Thresholds are intentionally generous to avoid flaky failures on slow CI.

use std::time::Instant;

use serde_json::json;

use shannon_core::api::{
    ContentBlock, Message, MessageContent, ToolResultContent,
};
use shannon_core::compact::{CompactConfig, CompactEngine, RuleBasedSummarizer};
use shannon_core::recording::RecordingEntry;
use shannon_core::testing::snapshot::{render_request_snapshot, RenderMode};
use shannon_core::token_estimation::{ConversationMessageSummary, TokenEstimator};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_text_message(role: &str, text: &str) -> Message {
    Message {
        role: role.to_string(),
        content: MessageContent::Text(text.to_string()),
    }
}

fn make_tool_result_message(tool_use_id: &str, result: &str) -> Message {
    Message {
        role: "tool".to_string(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.to_string(),
            content: Some(ToolResultContent::Single(result.to_string())),
            is_error: None,
        }]),
    }
}

fn make_tool_use_message(tool_name: &str, tool_use_id: &str, input: serde_json::Value) -> Message {
    Message {
        role: "assistant".to_string(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
            id: tool_use_id.to_string(),
            name: tool_name.to_string(),
            input,
        }]),
    }
}

/// Build a conversation with `turns` user/assistant exchanges, each including
/// a tool call and result.
fn build_conversation(turns: usize) -> Vec<Message> {
    let mut messages = Vec::with_capacity(turns * 4);
    for i in 0..turns {
        messages.push(make_text_message(
            "user",
            &format!("Please analyze module {i} and suggest improvements to error handling."),
        ));
        messages.push(make_tool_use_message(
            "Read",
            &format!("tu_{i}"),
            json!({"path": format!("src/mod_{i}.rs")}),
        ));
        messages.push(make_tool_result_message(
            &format!("tu_{i}"),
            &format!("fn process_{i}() -> Result<(), Box<dyn std::error::Error>> {{ Ok(()) }}"),
        ));
        messages.push(make_text_message(
            "assistant",
            &format!("Module {i} looks good. Consider adding more specific error types."),
        ));
    }
    messages
}

/// Build recording entries for a session with `count` entries.
fn build_recording_entries(count: usize) -> Vec<RecordingEntry> {
    let mut entries = Vec::with_capacity(count);
    entries.push(RecordingEntry::SessionStart {
        session_id: "perf-test-session".to_string(),
        model: "claude-3-opus".to_string(),
        timestamp: "2026-05-22T10:00:00Z".to_string(),
    });
    for i in 1..count {
        match i % 5 {
            0 => entries.push(RecordingEntry::UserMessage {
                content: format!("Help with task {i}"),
                turn: i / 5,
            }),
            1 => entries.push(RecordingEntry::ToolCall {
                tool: "Read".to_string(),
                input: json!({"path": format!("src/file_{i}.rs")}),
                result: format!("// file {i} contents"),
                is_error: false,
                duration_ms: 10,
            }),
            2 => entries.push(RecordingEntry::LlmResponse {
                turn: i / 5,
                body: json!({"content": "response text"}),
            }),
            3 => entries.push(RecordingEntry::LlmRequest {
                turn: i / 5,
                request_hash: format!("hash_{i}"),
                body: json!({"model": "claude-3-opus"}),
            }),
            _ => entries.push(RecordingEntry::ToolCall {
                tool: "Bash".to_string(),
                input: json!({"command": format!("echo {i}")}),
                result: format!("{i}"),
                is_error: false,
                duration_ms: 5,
            }),
        }
    }
    entries
}

// ---------------------------------------------------------------------------
// Regression tests
// ---------------------------------------------------------------------------

#[test]
fn compaction_100_turns_under_2s() {
    let messages = build_conversation(100);
    assert!(messages.len() >= 400, "should have 400+ messages");

    let config = CompactConfig::default();
    let summarizer = RuleBasedSummarizer::new();
    let mut engine =
        CompactEngine::new(config, Box::new(summarizer)).expect("engine creation failed");

    let mut msgs = messages;
    let start = Instant::now();
    let result = engine.compact(&mut msgs);
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "compaction should succeed: {:?}", result);
    assert!(
        elapsed.as_secs() < 2,
        "compaction took {:?}, expected < 2s",
        elapsed
    );
}

#[test]
fn session_load_200_entries_under_500ms() {
    let entries = build_recording_entries(200);
    let dir = tempfile::tempdir().expect("tempdir");

    // Write JSONL
    let path = dir.path().join("session.jsonl");
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&path).expect("create file");
        for entry in &entries {
            let line = serde_json::to_string(entry).expect("serialize entry");
            writeln!(file, "{line}").expect("write line");
        }
    }

    // Benchmark reading back
    let start = Instant::now();
    let content = std::fs::read_to_string(&path).expect("read file");
    let parsed: Vec<RecordingEntry> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse entry"))
        .collect();
    let elapsed = start.elapsed();

    assert_eq!(parsed.len(), 200, "should parse all 200 entries");
    assert!(
        elapsed.as_millis() < 500,
        "session load took {:?}, expected < 500ms",
        elapsed
    );
}

#[test]
fn tool_chain_10_steps_under_100ms() {
    // Simulate a 10-step tool chain (no LLM) — serialize inputs, build
    // responses, check types.
    let steps: Vec<(String, serde_json::Value, String, bool)> = (0..10)
        .map(|i| {
            let tool = if i % 2 == 0 { "Read" } else { "Bash" };
            let input = if i % 2 == 0 {
                json!({"path": format!("src/file_{i}.rs")})
            } else {
                json!({"command": format!("cargo test --test {i}")})
            };
            let result = format!("output_{i}");
            (tool.to_string(), input, result, false)
        })
        .collect();

    let start = Instant::now();

    // Simulate the processing: serialize each step, deserialize, validate
    let mut results = Vec::with_capacity(10);
    for (tool_name, input, result, is_error) in &steps {
        let input_json = serde_json::to_string(input).expect("serialize input");
        let parsed: serde_json::Value =
            serde_json::from_str(&input_json).expect("deserialize input");
        assert!(parsed.is_object());

        let msg = make_tool_use_message(tool_name, &format!("id_{tool_name}"), parsed);
        let msg_json = serde_json::to_string(&msg).expect("serialize msg");
        let _: Message = serde_json::from_str(&msg_json).expect("deserialize msg");

        let result_msg = make_tool_result_message(&format!("id_{tool_name}"), result);
        let result_json = serde_json::to_string(&result_msg).expect("serialize result");
        let _: Message = serde_json::from_str(&result_json).expect("deserialize result");

        results.push((*is_error, result.clone()));
    }

    let elapsed = start.elapsed();

    assert_eq!(results.len(), 10, "all 10 steps should complete");
    assert!(
        elapsed.as_millis() < 100,
        "10-step tool chain took {:?}, expected < 100ms",
        elapsed
    );
}

#[test]
fn streaming_parse_throughput_over_10mb_s() {
    // Build a simulated SSE byte stream with data events.
    // Each event is "data: {json}\n\n" format.
    let event_count = 10_000;
    let mut sse_stream = Vec::with_capacity(event_count * 120);
    for i in 0..event_count {
        let json = format!(
            "{{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"chunk {i} with some realistic content about code analysis.\"}}}}"
        );
        let event = format!("data: {json}\n\n");
        sse_stream.extend_from_slice(event.as_bytes());
    }

    let stream_len = sse_stream.len();
    assert!(stream_len > 1_000_000, "stream should be > 1MB, got {stream_len} bytes");

    let start = Instant::now();

    // Parse: split on "data: " prefix, extract JSON
    let content = std::str::from_utf8(&sse_stream).expect("valid utf8");
    let parsed_count = content
        .split("data: ")
        .skip(1)
        .filter(|chunk| {
            if let Some(json_str) = chunk.lines().next() {
                serde_json::from_str::<serde_json::Value>(json_str).is_ok()
            } else {
                false
            }
        })
        .count();

    let elapsed = start.elapsed();
    let elapsed_secs = elapsed.as_secs_f64();
    let throughput_bps = stream_len as f64 / elapsed_secs;
    let throughput_mbps = throughput_bps / 1_000_000.0;

    assert_eq!(parsed_count, event_count, "should parse all events");
    assert!(
        throughput_mbps > 10.0,
        "SSE parse throughput was {:.1} MB/s, expected > 10 MB/s",
        throughput_mbps
    );
}

#[test]
fn snapshot_render_under_1ms() {
    // Build a realistic API request snapshot
    let request = json!({
        "model": "claude-3-opus",
        "system": "You are an expert Rust developer.",
        "messages": (0..20).map(|i| {
            if i % 2 == 0 {
                json!({"role": "user", "content": format!("Help me with task {i}")})
            } else {
                json!({"role": "assistant", "content": [{"type": "text", "text": format!("Sure, here is the solution for task {i}...")}]})
            }
        }).collect::<Vec<_>>(),
        "tools": (0..10).map(|i| {
            json!({
                "name": format!("tool_{i}"),
                "description": format!("Tool number {i} for code operations"),
                "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}}
            })
        }).collect::<Vec<_>>(),
        "max_tokens": 4096
    });

    // Warm up
    for _ in 0..100 {
        let _ = render_request_snapshot(&request, RenderMode::KindOnly);
    }

    // Measure
    let iterations = 1000;
    let start = Instant::now();
    for _ in 0..iterations {
        let _snapshot = render_request_snapshot(&request, RenderMode::KindOnly);
    }
    let elapsed = start.elapsed();
    let per_call = elapsed / iterations;

    assert!(
        per_call.as_micros() < 1000,
        "snapshot render took {:?} per call, expected < 1ms",
        per_call
    );
}

#[test]
fn token_estimation_1000_messages_under_50ms() {
    let estimator = TokenEstimator::new();
    let messages: Vec<ConversationMessageSummary> = (0..1000)
        .map(|i| ConversationMessageSummary {
            role: if i % 2 == 0 { "user".into() } else { "assistant".into() },
            content: format!(
                "This is message {i} with typical code analysis content discussing \
                 refactoring patterns and best practices in Rust."
            ),
        })
        .collect();

    let start = Instant::now();
    let tokens = estimator.count_precise_for_messages(&messages, "claude-3-opus");
    let elapsed = start.elapsed();

    assert!(tokens > 0, "should estimate some tokens");
    assert!(
        elapsed.as_millis() < 50,
        "token estimation for 1000 messages took {:?}, expected < 50ms",
        elapsed
    );
}
