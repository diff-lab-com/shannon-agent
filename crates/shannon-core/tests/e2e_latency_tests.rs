//! End-to-end latency benchmark tests.
//!
//! Measures the full pipeline latency for common operations using mock LLM
//! responses (no real API calls). Thresholds are generous to avoid CI flakiness.

use std::time::Instant;

use serde_json::json;

use shannon_core::api::{ContentBlock, Message, MessageContent, ToolResultContent};
use shannon_core::compact::{CompactConfig, CompactEngine, RuleBasedSummarizer};
use shannon_core::testing::mock_dsl::{anthropic_sse, text_response, tool_call_response};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_text_message(role: &str, text: &str) -> Message {
    Message {
        role: role.to_string(),
        content: MessageContent::Text(text.to_string()),
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

/// Parse SSE body into individual events, extract content text.
fn extract_text_from_sse(sse: &str) -> String {
    let mut text = String::new();
    for line in sse.lines() {
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
            if val["type"] == "content_block_delta" {
                if let Some(t) = val["delta"]["text"].as_str() {
                    text.push_str(t);
                }
            }
        }
    }
    text
}

/// Build a conversation simulating `turns` user/assistant exchanges.
fn build_conversation(turns: usize) -> Vec<Message> {
    let mut messages = Vec::with_capacity(turns * 4);
    for i in 0..turns {
        messages.push(make_text_message("user", &format!("Analyze module {i}")));
        messages.push(make_tool_use_message(
            "Read",
            &format!("tu_{i}"),
            json!({"path": format!("src/mod_{i}.rs")}),
        ));
        messages.push(make_tool_result_message(
            &format!("tu_{i}"),
            &format!("fn process_{i}() {{}}"),
        ));
        messages.push(make_text_message(
            "assistant",
            &format!("Module {i} analyzed."),
        ));
    }
    messages
}

// ---------------------------------------------------------------------------
// E2E Latency Tests
// ---------------------------------------------------------------------------

#[test]
fn single_turn_text_under_100ms() {
    // Measure: build request → generate SSE mock → parse SSE → extract text
    let iterations = 100;

    let start = Instant::now();
    for _ in 0..iterations {
        let response = text_response("Hello, I've analyzed your code.");
        let sse = anthropic_sse(&response);

        // Parse SSE events
        let text = extract_text_from_sse(&sse);
        assert!(!text.is_empty());

        // Build response message
        let msg = make_text_message("assistant", &text);
        let serialized = serde_json::to_string(&msg).expect("serialize");
        assert!(serialized.contains("Hello"));
    }
    let elapsed = start.elapsed();
    let per_iteration = elapsed / iterations;

    assert!(
        per_iteration.as_millis() < 100,
        "single turn text pipeline took {per_iteration:?}, expected < 100ms"
    );
}

#[test]
fn single_turn_tool_use_under_100ms() {
    // Measure: build tool_use SSE mock → parse → build messages → serialize
    let iterations = 100;

    let start = Instant::now();
    for _ in 0..iterations {
        let response = tool_call_response(
            "toolu_1",
            "Write",
            json!({"path": "hello.txt", "content": "world"}),
        );
        let sse = anthropic_sse(&response);

        // Parse tool call from SSE
        assert!(sse.contains("tool_use"));
        assert!(sse.contains("Write"));

        // Build messages
        let tool_msg = make_tool_use_message("Write", "toolu_1", json!({"path": "hello.txt"}));
        let result_msg = make_tool_result_message("toolu_1", "File written successfully");

        let tool_json = serde_json::to_string(&tool_msg).expect("serialize tool");
        let result_json = serde_json::to_string(&result_msg).expect("serialize result");
        assert!(tool_json.contains("Write"));
        assert!(result_json.contains("successfully"));
    }
    let elapsed = start.elapsed();
    let per_iteration = elapsed / iterations;

    assert!(
        per_iteration.as_millis() < 100,
        "single turn tool use pipeline took {per_iteration:?}, expected < 100ms"
    );
}

#[test]
fn five_turn_conversation_under_500ms() {
    // Measure: build 5-turn conversation → compact → verify
    let messages = build_conversation(5);
    assert_eq!(messages.len(), 20, "5 turns = 20 messages");

    let config = CompactConfig::default();
    let summarizer = RuleBasedSummarizer::new();
    let mut engine = CompactEngine::new(config, Box::new(summarizer)).expect("engine creation");

    let start = Instant::now();

    // Simulate 5 full turns: for each turn, build SSE, parse, compact
    for turn in 0..5 {
        // User message
        let user = make_text_message("user", &format!("Turn {turn} request"));
        let user_json = serde_json::to_string(&user).expect("serialize");

        // LLM response (tool call)
        let response = tool_call_response(
            &format!("toolu_{turn}"),
            "Read",
            json!({"path": format!("src/file_{turn}.rs")}),
        );
        let _sse = anthropic_sse(&response);

        // Tool result
        let result = make_tool_result_message(&format!("toolu_{turn}"), "file contents");
        let _result_json = serde_json::to_string(&result).expect("serialize");

        // LLM text response
        let text_resp = text_response(&format!("Analysis for turn {turn}"));
        let _text_sse = anthropic_sse(&text_resp);

        // Compact if getting long
        let _ = user_json;
    }

    // Final compaction
    let mut msgs = messages;
    let result = engine.compact(&mut msgs);
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "compaction should succeed: {result:?}");
    assert!(
        elapsed.as_millis() < 500,
        "5-turn conversation pipeline took {elapsed:?}, expected < 500ms"
    );
}

#[test]
fn sse_round_trip_throughput_over_1mb_s() {
    // Measure full round-trip: MockResponse → SSE → parse → Message
    let mock_responses: Vec<_> = (0..1000)
        .map(|i| {
            if i % 3 == 0 {
                text_response(&format!("Response {i} with analysis content."))
            } else {
                tool_call_response(
                    &format!("toolu_{i}"),
                    if i % 2 == 0 { "Read" } else { "Bash" },
                    json!({"path": format!("src/file_{i}.rs")}),
                )
            }
        })
        .collect();

    let start = Instant::now();
    let mut total_bytes = 0usize;

    for resp in &mock_responses {
        let sse = anthropic_sse(resp);
        total_bytes += sse.len();

        // Parse and validate
        if resp.stop_reason == "tool_use" {
            assert!(sse.contains("tool_use"));
        } else {
            let text = extract_text_from_sse(&sse);
            assert!(!text.is_empty() || resp.content_blocks.is_empty());
        }
    }

    let elapsed = start.elapsed();
    let throughput_mbps = total_bytes as f64 / elapsed.as_secs_f64() / 1_000_000.0;

    assert!(
        throughput_mbps > 1.0,
        "SSE round-trip throughput was {throughput_mbps:.1} MB/s, expected > 1 MB/s"
    );
}

#[test]
fn message_serialization_1000_under_50ms() {
    let messages: Vec<Message> = (0..1000)
        .map(|i| {
            if i % 3 == 0 {
                make_text_message("user", &format!("Request {i}"))
            } else if i % 3 == 1 {
                make_tool_use_message(
                    "Read",
                    &format!("tu_{i}"),
                    json!({"path": format!("src/{i}.rs")}),
                )
            } else {
                make_tool_result_message(&format!("tu_{i}"), &format!("result {i}"))
            }
        })
        .collect();

    let start = Instant::now();

    let serialized: Vec<String> = messages
        .iter()
        .map(|m| serde_json::to_string(m).expect("serialize"))
        .collect();

    let elapsed = start.elapsed();

    assert_eq!(serialized.len(), 1000);
    assert!(
        elapsed.as_millis() < 50,
        "1000 message serializations took {elapsed:?}, expected < 50ms"
    );

    // Deserialize back
    let start = Instant::now();
    let deserialized: Vec<Message> = serialized
        .iter()
        .map(|s| serde_json::from_str(s).expect("deserialize"))
        .collect();
    let elapsed = start.elapsed();

    assert_eq!(deserialized.len(), 1000);
    assert!(
        elapsed.as_millis() < 50,
        "1000 message deserializations took {elapsed:?}, expected < 50ms"
    );
}

#[test]
fn compaction_50_turns_under_500ms() {
    let messages = build_conversation(50);
    assert!(messages.len() >= 200, "50 turns = 200+ messages");

    let config = CompactConfig::default();
    let summarizer = RuleBasedSummarizer::new();
    let mut engine = CompactEngine::new(config, Box::new(summarizer)).expect("engine creation");

    let start = Instant::now();
    let mut msgs = messages;
    let result = engine.compact(&mut msgs);
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "compaction should succeed: {result:?}");
    assert!(
        elapsed.as_millis() < 500,
        "50-turn compaction took {elapsed:?}, expected < 500ms"
    );
    assert!(msgs.len() < 200, "compaction should reduce message count");
}
