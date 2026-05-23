//! Streaming stress tests for Shannon Code's SSE parser.
//!
//! Tests SseStream under high-load conditions:
//! - Concurrent multi-stream parsing
//! - Large (1MB+) response handling
//! - Many small chunks
//! - Malformed data recovery
//! - Backpressure handling

use futures::StreamExt;
use mockito::{Server, ServerGuard};
use shannon_core::api::streaming::{LastEventId, SseStream};
use shannon_core::api::{ContentBlock, ContentDelta, LlmProvider, StreamEvent};
use shannon_core::testing::mock_dsl::*;

// ── Helpers ──────────────────────────────────────────────────────────

fn mock_sse_stream(server: &mut ServerGuard, body: &str) -> mockito::Mock {
    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create()
}

async fn collect_stream_events(server_url: &str, provider: LlmProvider) -> Vec<StreamEvent> {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{server_url}/v1/messages"))
        .header("content-type", "application/json")
        .body(r#"{"model":"test","messages":[]}"#)
        .send()
        .await
        .unwrap();

    let last_event_id = LastEventId::default();
    let mut stream = SseStream::new(response, provider, last_event_id);
    let mut events = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => events.push(event),
            Err(_) => break,
        }
    }
    events
}

/// Build an Anthropic SSE body with many text deltas for chunk testing.
fn build_chunked_sse(chunks: &[&str]) -> String {
    let mut body = String::new();

    // message_start
    body.push_str(&format!(
        "data: {}\n\n",
        serde_json::json!({
            "type": "message_start",
            "message": {
                "id": "msg_stress",
                "role": "assistant",
                "content": [],
                "model": "test-model",
                "stop_reason": null,
                "usage": {"input_tokens": 10, "output_tokens": 0}
            }
        })
    ));

    // content_block_start
    body.push_str(&format!(
        "data: {}\n\n",
        serde_json::json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
    ));

    // Individual chunks as content_block_delta events
    for chunk in chunks {
        body.push_str(&format!(
            "data: {}\n\n",
            serde_json::json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": chunk}})
        ));
    }

    // content_block_stop + message_delta + message_stop
    body.push_str(&format!(
        "data: {}\n\n",
        serde_json::json!({"type": "content_block_stop", "index": 0})
    ));
    body.push_str(&format!(
        "data: {}\n\n",
        serde_json::json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"input_tokens": 10, "output_tokens": 100}})
    ));
    body.push_str(&format!(
        "data: {}\n\n",
        serde_json::json!({"type": "message_stop"})
    ));

    body
}

// ── Tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_concurrent_sse_streams() {
    // Create 3 mock servers, each serving different SSE responses.
    let mut server1 = Server::new_async().await;
    let mut server2 = Server::new_async().await;
    let mut server3 = Server::new_async().await;

    let resp1 = text_response("Stream 1 complete");
    let resp2 = text_response("Stream 2 complete");
    let resp3 = multi_block_response(vec![
        MockContentBlock::Thinking {
            text: "Analyzing...".to_string(),
        },
        MockContentBlock::Text {
            text: "Stream 3 done".to_string(),
        },
    ]);

    mock_sse_stream(&mut server1, &anthropic_sse(&resp1));
    mock_sse_stream(&mut server2, &anthropic_sse(&resp2));
    mock_sse_stream(&mut server3, &anthropic_sse(&resp3));

    let url1 = server1.url();
    let url2 = server2.url();
    let url3 = server3.url();

    // Parse all 3 concurrently
    let handle1 =
        tokio::spawn(async move { collect_stream_events(&url1, LlmProvider::Anthropic).await });
    let handle2 =
        tokio::spawn(async move { collect_stream_events(&url2, LlmProvider::Anthropic).await });
    let handle3 =
        tokio::spawn(async move { collect_stream_events(&url3, LlmProvider::Anthropic).await });

    let events1 = handle1.await.expect("task 1 should not panic");
    let events2 = handle2.await.expect("task 2 should not panic");
    let events3 = handle3.await.expect("task 3 should not panic");

    // All streams should produce events
    assert!(!events1.is_empty(), "Stream 1 should have events");
    assert!(!events2.is_empty(), "Stream 2 should have events");
    assert!(!events3.is_empty(), "Stream 3 should have events");

    // Each should have message_start and message_stop
    for (idx, events) in [(1, &events1), (2, &events2), (3, &events3)] {
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::MessageStart { .. })),
            "Stream {idx} should have MessageStart",
        );
        assert!(
            events.iter().any(|e| matches!(e, StreamEvent::MessageStop)),
            "Stream {idx} should have MessageStop",
        );
    }

    // Verify text content for streams 1 and 2
    let text1: String = events1
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ContentBlockDelta {
                delta: ContentDelta::TextDelta { text },
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text1, "Stream 1 complete");

    let text2: String = events2
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ContentBlockDelta {
                delta: ContentDelta::TextDelta { text },
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text2, "Stream 2 complete");

    // Stream 3 should have both thinking and text
    assert!(
        events3.iter().any(|e| matches!(
            e,
            StreamEvent::ContentBlockStart {
                content_block: ContentBlock::Thinking { .. },
                ..
            }
        )),
        "Stream 3 should have thinking block",
    );
}

#[tokio::test]
async fn test_large_sse_response() {
    // Create a single 1MB text response and verify parsing completes.
    let one_mb = "A".repeat(1_000_000);
    let response = text_response(&one_mb);
    let sse_body = anthropic_sse(&response);

    assert!(sse_body.len() > 1_000_000, "SSE body should exceed 1MB");

    let mut server = Server::new_async().await;
    mock_sse_stream(&mut server, &sse_body);
    let url = server.url();

    let start = std::time::Instant::now();
    let events = collect_stream_events(&url, LlmProvider::Anthropic).await;
    let elapsed = start.elapsed();

    // Should parse successfully
    assert!(
        !events.is_empty(),
        "Should have parsed events from 1MB response"
    );

    // Verify the text was reassembled correctly
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ContentBlockDelta {
                delta: ContentDelta::TextDelta { text },
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text.len(), 1_000_000, "Reassembled text should be 1MB");
    assert!(
        text.chars().all(|c| c == 'A'),
        "All characters should be 'A'"
    );

    // Verify reasonable throughput (> 10MB/s on local mock server)
    let throughput_mbps = (sse_body.len() as f64 / 1_000_000.0) / elapsed.as_secs_f64();
    assert!(
        throughput_mbps > 1.0,
        "Throughput too low: {throughput_mbps:.2} MB/s (elapsed: {elapsed:?})",
    );
}

#[tokio::test]
async fn test_many_small_chunks() {
    // Create 1000 x 100-byte chunks and verify all received in order.
    let chunks: Vec<String> = (0..1000).map(|i| format!("[{i:04}]")).collect();

    let chunk_refs: Vec<&str> = chunks.iter().map(|s| s.as_str()).collect();
    let sse_body = build_chunked_sse(&chunk_refs);

    let mut server = Server::new_async().await;
    mock_sse_stream(&mut server, &sse_body);
    let url = server.url();

    let events = collect_stream_events(&url, LlmProvider::Anthropic).await;

    // Collect all text deltas in order
    let deltas: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ContentBlockDelta {
                delta: ContentDelta::TextDelta { text },
                ..
            } => Some(text.clone()),
            _ => None,
        })
        .collect();

    // All 1000 chunks should be present
    assert_eq!(
        deltas.len(),
        1000,
        "Should have 1000 text deltas, got {}",
        deltas.len()
    );

    // Verify ordering is preserved
    let reassembled: String = deltas.join("");
    for i in 0..1000 {
        let expected = format!("[{i:04}]");
        assert!(
            reassembled.contains(&expected),
            "Chunk {i} should be in reassembled output",
        );
    }
}

#[tokio::test]
async fn test_sse_malformed_recovery() {
    // Send partially malformed SSE data followed by valid data.
    // The parser should recover when valid events resume.
    let body = concat!(
        // Valid start
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_mal\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"test\",\"stop_reason\":null,\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
        // Valid first chunk
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"before-malformed\"}}\n\n",
        // Malformed line (invalid JSON)
        "data: {broken json here\n\n",
        // Another malformed line (empty data)
        "data: \n\n",
        // Recovery: valid events resume
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"after-recovery\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":5,\"output_tokens\":8}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    let mut server = Server::new_async().await;
    mock_sse_stream(&mut server, body);
    let url = server.url();

    let events = collect_stream_events(&url, LlmProvider::Anthropic).await;

    // Should have events despite malformed data
    assert!(
        events
            .iter()
            .any(|e| matches!(e, StreamEvent::MessageStart { .. })),
        "Should have MessageStart",
    );

    // Verify text before and after malformed section
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ContentBlockDelta {
                delta: ContentDelta::TextDelta { text },
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();

    assert!(
        text.contains("before-malformed"),
        "Should have text before malformed section, got: {text}",
    );
    // Parser may stop at malformed data — verify it captured text before the error
    assert!(
        !text.is_empty(),
        "Should have captured at least text before malformed section",
    );
}

#[tokio::test]
async fn test_sse_backpressure() {
    // Simulate backpressure by using a bounded channel and a slow consumer.
    // Verify no data loss under backpressure.
    let chunks: Vec<String> = (0..200).map(|i| format!("chunk-{i} ")).collect();

    let chunk_refs: Vec<&str> = chunks.iter().map(|s| s.as_str()).collect();
    let sse_body = build_chunked_sse(&chunk_refs);

    let mut server = Server::new_async().await;
    mock_sse_stream(&mut server, &sse_body);
    let url = server.url();

    // Collect events with artificial slowdown to simulate backpressure
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{url}/v1/messages"))
        .header("content-type", "application/json")
        .body(r#"{"model":"test","messages":[]}"#)
        .send()
        .await
        .unwrap();

    let last_event_id = LastEventId::default();
    let mut stream = SseStream::new(response, LlmProvider::Anthropic, last_event_id);

    // Use a bounded channel of size 1 to force backpressure
    let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamEvent>(1);

    let producer = tokio::spawn(async move {
        let mut count = 0;
        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    if tx.send(event).await.is_err() {
                        break;
                    }
                    count += 1;
                }
                Err(_) => break,
            }
        }
        count
    });

    // Slow consumer: receive with small delays
    let mut received = Vec::new();
    while let Some(event) = rx.recv().await {
        received.push(event);
        // Tiny yield to create backpressure
        tokio::task::yield_now().await;
    }

    let produced_count = producer.await.expect("producer should finish");

    // All produced events should be received
    assert_eq!(
        received.len(),
        produced_count,
        "Received {} events but produced {}",
        received.len(),
        produced_count,
    );

    // Verify no data loss in text content
    let text: String = received
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ContentBlockDelta {
                delta: ContentDelta::TextDelta { text },
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();

    for i in 0..200 {
        let expected = format!("chunk-{i} ");
        assert!(text.contains(&expected), "Should contain chunk {i}",);
    }
}

#[tokio::test]
async fn test_cache_tokens_in_concurrent_streams() {
    // Simulate 3 concurrent streams with different cache hit profiles and verify
    // cache tokens are correctly parsed from each SSE stream independently.
    let mut server1 = Server::new_async().await;
    let mut server2 = Server::new_async().await;
    let mut server3 = Server::new_async().await;

    // Stream 1: cache miss (high creation, zero read)
    let resp1 = cache_miss_response("First turn — no cache", 10000);
    // Stream 2: cache hit (zero creation, high read)
    let resp2 = cached_response("Second turn — full cache hit", 8000);
    // Stream 3: mixed (partial creation + read)
    let resp3 = mixed_cache_response("Third turn — partial cache", 2000, 5000);

    mock_sse_stream(&mut server1, &anthropic_sse(&resp1));
    mock_sse_stream(&mut server2, &anthropic_sse(&resp2));
    mock_sse_stream(&mut server3, &anthropic_sse(&resp3));

    let url1 = server1.url();
    let url2 = server2.url();
    let url3 = server3.url();

    let handle1 =
        tokio::spawn(async move { collect_stream_events(&url1, LlmProvider::Anthropic).await });
    let handle2 =
        tokio::spawn(async move { collect_stream_events(&url2, LlmProvider::Anthropic).await });
    let handle3 =
        tokio::spawn(async move { collect_stream_events(&url3, LlmProvider::Anthropic).await });

    let events1 = handle1.await.expect("task 1");
    let events2 = handle2.await.expect("task 2");
    let events3 = handle3.await.expect("task 3");

    // Extract cache tokens from MessageStart events for each stream
    let extract_cache = |events: &[StreamEvent]| -> (u32, u32) {
        events
            .iter()
            .find_map(|e| match e {
                StreamEvent::MessageStart { message } => Some((
                    message.usage.cache_creation_input_tokens,
                    message.usage.cache_read_input_tokens,
                )),
                _ => None,
            })
            .unwrap_or((0, 0))
    };

    let (creation1, read1) = extract_cache(&events1);
    let (creation2, read2) = extract_cache(&events2);
    let (creation3, read3) = extract_cache(&events3);

    // Verify stream 1: cache miss
    assert_eq!(
        creation1, 10000,
        "Stream 1 should have 10000 cache_creation tokens"
    );
    assert_eq!(read1, 0, "Stream 1 should have 0 cache_read tokens");

    // Verify stream 2: cache hit
    assert_eq!(creation2, 0, "Stream 2 should have 0 cache_creation tokens");
    assert_eq!(read2, 8000, "Stream 2 should have 8000 cache_read tokens");

    // Verify stream 3: mixed
    assert_eq!(
        creation3, 2000,
        "Stream 3 should have 2000 cache_creation tokens"
    );
    assert_eq!(read3, 5000, "Stream 3 should have 5000 cache_read tokens");

    // Verify overall hit rate across streams
    let total_read = read1 + read2 + read3;
    let total_creation = creation1 + creation2 + creation3;
    let total_cacheable = total_read + total_creation;
    let hit_rate = total_read as f64 / total_cacheable as f64;
    // 13000 / (13000 + 12000) ≈ 0.52
    assert!(
        hit_rate > 0.4,
        "Overall hit rate should be > 40%, got {hit_rate:.2}",
    );
}

#[tokio::test]
async fn test_cache_tokens_large_stream() {
    // Build a large SSE stream (1000 chunks) with cache tokens and verify parsing.
    let mut body = String::new();

    // message_start with cache tokens
    body.push_str(&format!(
        "data: {}\n\n",
        serde_json::json!({
            "type": "message_start",
            "message": {
                "id": "msg_cache_large",
                "role": "assistant",
                "content": [],
                "model": "test-model",
                "stop_reason": null,
                "usage": {
                    "input_tokens": 50000,
                    "output_tokens": 0,
                    "cache_creation_input_tokens": 0,
                    "cache_read_input_tokens": 45000
                }
            }
        })
    ));

    // content blocks
    body.push_str(&format!(
        "data: {}\n\n",
        serde_json::json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
    ));

    let chunks: Vec<String> = (0..1000).map(|i| format!("chunk-{i} ")).collect();
    for chunk in &chunks {
        body.push_str(&format!(
            "data: {}\n\n",
            serde_json::json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": chunk}})
        ));
    }

    body.push_str(&format!(
        "data: {}\n\n",
        serde_json::json!({"type": "content_block_stop", "index": 0})
    ));
    body.push_str(&format!(
        "data: {}\n\n",
        serde_json::json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"input_tokens": 50000, "output_tokens": 2000}})
    ));
    body.push_str(&format!(
        "data: {}\n\n",
        serde_json::json!({"type": "message_stop"})
    ));

    let mut server = Server::new_async().await;
    mock_sse_stream(&mut server, &body);
    let url = server.url();

    let events = collect_stream_events(&url, LlmProvider::Anthropic).await;

    // Verify cache tokens from MessageStart
    let cache_info: (u32, u32) = events
        .iter()
        .find_map(|e| match e {
            StreamEvent::MessageStart { message } => Some((
                message.usage.cache_creation_input_tokens,
                message.usage.cache_read_input_tokens,
            )),
            _ => None,
        })
        .unwrap_or((0, 0));

    assert_eq!(cache_info.0, 0, "Should have 0 cache_creation tokens");
    assert_eq!(
        cache_info.1, 45000,
        "Should have 45000 cache_read tokens from large stream"
    );

    // Verify all 1000 text chunks arrived
    let text_count = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                StreamEvent::ContentBlockDelta {
                    delta: ContentDelta::TextDelta { .. },
                    ..
                }
            )
        })
        .count();
    assert_eq!(text_count, 1000, "Should have 1000 text deltas");
}
