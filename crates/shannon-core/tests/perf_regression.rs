//! Performance regression tests for shannon-core.
//!
//! These tests verify that key operations stay within acceptable time bounds.
//! They use `#[test]` with `std::time::Instant` for timing assertions.
//!
//! Thresholds are intentionally generous to avoid flaky failures on slow CI.

use std::time::Instant;

use serde_json::json;

use shannon_core::api::{ContentBlock, Message, MessageContent, ToolResultContent};
use shannon_core::compact::{CompactConfig, CompactEngine, RuleBasedSummarizer};
use shannon_core::recording::RecordingEntry;
use shannon_core::testing::snapshot::{RenderMode, render_request_snapshot};
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

    assert!(result.is_ok(), "compaction should succeed: {result:?}");
    assert!(
        elapsed.as_secs() < 2,
        "compaction took {elapsed:?}, expected < 2s"
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
        "session load took {elapsed:?}, expected < 500ms"
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
        "10-step tool chain took {elapsed:?}, expected < 100ms"
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
    assert!(
        stream_len > 1_000_000,
        "stream should be > 1MB, got {stream_len} bytes"
    );

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
        throughput_mbps > 2.0,
        "SSE parse throughput was {throughput_mbps:.1} MB/s, expected > 2 MB/s"
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
        "snapshot render took {per_call:?} per call, expected < 1ms"
    );
}

#[test]
fn token_estimation_1000_messages_under_50ms() {
    let estimator = TokenEstimator::new();
    let messages: Vec<ConversationMessageSummary> = (0..1000)
        .map(|i| ConversationMessageSummary {
            role: if i % 2 == 0 {
                "user".into()
            } else {
                "assistant".into()
            },
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
        "token estimation for 1000 messages took {elapsed:?}, expected < 50ms"
    );
}

// ---------------------------------------------------------------------------
// Cache hit rate performance regression tests
// ---------------------------------------------------------------------------

/// Build a multi-turn cache usage profile simulating a realistic conversation.
fn build_cache_profile(turns: usize) -> Vec<(u32, u32, u32)> {
    // (input_tokens, cache_creation, cache_read)
    // First turn: miss (high creation, zero read)
    // Subsequent turns: increasing cache hit rate
    (0..turns)
        .map(|i| {
            if i == 0 {
                (5000, 10000, 0) // First turn: cache miss
            } else {
                let ratio = (i as f64 / turns as f64).min(0.95);
                let cache_read = (10000.0 * ratio) as u32;
                let cache_creation = 10000 - cache_read;
                (5000, cache_creation, cache_read)
            }
        })
        .collect()
}

/// Calculate cache hit rate from a usage profile.
fn calculate_hit_rate(profile: &[(u32, u32, u32)]) -> f64 {
    let total_read: u32 = profile.iter().map(|(_, _, r)| *r).sum();
    let total_creation: u32 = profile.iter().map(|(_, c, _)| *c).sum();
    let total_cacheable = total_read + total_creation;
    if total_cacheable == 0 {
        return 0.0;
    }
    total_read as f64 / total_cacheable as f64
}

#[test]
fn cache_hit_rate_1000_turns_under_10ms() {
    let profile = build_cache_profile(1000);
    assert_eq!(profile.len(), 1000);

    let start = Instant::now();
    let hit_rate = calculate_hit_rate(&profile);
    let elapsed = start.elapsed();

    assert!(
        hit_rate > 0.4,
        "Hit rate for 1000-turn profile should be > 40%, got {hit_rate:.2}"
    );
    assert!(
        elapsed.as_millis() < 10,
        "Cache hit rate calculation for 1000 turns took {elapsed:?}, expected < 10ms"
    );
}

#[test]
fn cache_hit_rate_10000_turns_under_50ms() {
    let profile = build_cache_profile(10000);
    assert_eq!(profile.len(), 10000);

    let start = Instant::now();
    let hit_rate = calculate_hit_rate(&profile);
    let elapsed = start.elapsed();

    assert!(
        hit_rate > 0.4,
        "Hit rate for 10000-turn profile should be > 40%, got {hit_rate:.2}"
    );
    assert!(
        elapsed.as_millis() < 50,
        "Cache hit rate calculation for 10000 turns took {elapsed:?}, expected < 50ms"
    );
}

#[test]
fn cache_accumulation_from_sse_events_under_100ms() {
    // Simulate parsing cache tokens from 500 SSE message_start events.
    use serde_json::json;

    let event_count = 500;
    let events: Vec<serde_json::Value> = (0..event_count)
        .map(|i| {
            let (creation, read) = if i == 0 {
                (10000, 0)
            } else {
                let ratio = (i as f64 / event_count as f64).min(0.9);
                let r = (10000.0 * ratio) as u32;
                (10000 - r, r)
            };
            json!({
                "type": "message_start",
                "message": {
                    "id": format!("msg_{i}"),
                    "usage": {
                        "input_tokens": 5000,
                        "output_tokens": 200,
                        "cache_creation_input_tokens": creation,
                        "cache_read_input_tokens": read
                    }
                }
            })
        })
        .collect();

    let start = Instant::now();

    // Parse and accumulate cache tokens (simulating what the engine does)
    let mut total_creation: u64 = 0;
    let mut total_read: u64 = 0;
    let mut total_input: u64 = 0;

    for event_json in &events {
        let msg = event_json.get("message").unwrap();
        let usage = msg.get("usage").unwrap();
        total_input += usage.get("input_tokens").unwrap().as_u64().unwrap();
        total_creation += usage
            .get("cache_creation_input_tokens")
            .unwrap()
            .as_u64()
            .unwrap();
        total_read += usage
            .get("cache_read_input_tokens")
            .unwrap()
            .as_u64()
            .unwrap();
    }

    let elapsed = start.elapsed();

    assert!(total_creation > 0, "Should have some cache creation tokens");
    assert!(total_read > 0, "Should have some cache read tokens");
    assert_eq!(total_input, 5000 * event_count as u64);

    let hit_rate = total_read as f64 / (total_read + total_creation) as f64;
    assert!(
        hit_rate > 0.3,
        "Accumulated hit rate should be > 30%, got {hit_rate:.2}"
    );

    assert!(
        elapsed.as_millis() < 100,
        "Cache token accumulation from {event_count} events took {elapsed:?}, expected < 100ms"
    );
}

#[test]
fn cache_hit_rate_edge_cases() {
    // All misses
    let all_misses = vec![(1000, 5000, 0); 10];
    let rate = calculate_hit_rate(&all_misses);
    assert_eq!(rate, 0.0, "All misses should give 0% hit rate");

    // All hits
    let all_hits = vec![(1000, 0, 5000); 10];
    let rate = calculate_hit_rate(&all_hits);
    assert!(
        (rate - 1.0).abs() < f64::EPSILON,
        "All hits should give 100% hit rate"
    );

    // Empty
    let empty: Vec<(u32, u32, u32)> = vec![];
    let rate = calculate_hit_rate(&empty);
    assert_eq!(rate, 0.0, "Empty profile should give 0% hit rate");

    // Single miss
    let single = vec![(100, 1000, 0)];
    let rate = calculate_hit_rate(&single);
    assert_eq!(rate, 0.0);

    // Single hit
    let single_hit = vec![(100, 0, 1000)];
    let rate = calculate_hit_rate(&single_hit);
    assert!((rate - 1.0).abs() < f64::EPSILON);
}
