//! E2E tests for conversation compaction using a real local LLM (ollama).
//!
//! These tests are gated behind the `OLLAMA_E2E` environment variable and are

#![allow(clippy::field_reassign_with_default)]
//! marked `#[ignore]` so they only run via `scripts/test-perf.sh --ollama`.
//!
//! Required:
//!   - ollama running locally (`ollama serve`)
//!   - A model pulled (e.g. `ollama pull qwen3:4b`)
//!   - `OLLAMA_E2E=1` and `OLLAMA_MODEL=<model>` set

use shannon_core::api::{
    ContentBlock, LlmClient, LlmClientConfig, LlmProvider, Message, MessageContent,
};
use shannon_core::compact::{CompactConfig, CompactEngine};
use std::time::Instant;

/// Build an ollama-backed LLM client from env, skipping if unavailable.
fn ollama_client() -> Option<LlmClient> {
    if std::env::var("OLLAMA_E2E").is_err() {
        return None;
    }
    let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "qwen3:4b".to_string());
    let config = LlmClientConfig {
        api_key: String::new(),
        base_url: "http://localhost:11434".to_string(),
        model,
        max_tokens: 2048,
        timeout_seconds: 120,
        api_version: String::new(),
        provider: LlmProvider::Ollama,
        ..Default::default()
    };
    Some(LlmClient::new(config))
}

/// Helper: generate N conversation turns with realistic content.
fn generate_conversation(turns: usize) -> Vec<Message> {
    let mut messages = vec![Message {
        role: "system".to_string(),
        content: MessageContent::Text("You are a helpful coding assistant.".to_string()),
    }];

    for i in 0..turns {
        // User turn
        messages.push(Message {
            role: "user".to_string(),
            content: MessageContent::Text(format!(
                "Turn {i}: Please explain the concept of ownership in Rust, \
                 specifically how it relates to moves and borrows. \
                 Give me code examples with struct definitions and trait implementations."
            )),
        });
        // Assistant turn
        messages.push(Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::Text {
                text: format!(
                    "Turn {i} response: Ownership is a core Rust concept. \
                         When you assign a value, it is **moved** rather than copied. \
                         Here's an example:\n\
                         ```rust\n\
                         struct Data {{ value: i32 }}\n\
                         fn process(d: Data) {{ /* d is moved here */ }}\n\
                         ```\n\
                         Borrows let you reference data without taking ownership:\n\
                         ```rust\n\
                         fn borrow_example(x: &i32) -> i32 {{ *x + 1 }}\n\
                         ```\n\
                         Traits like `Clone` and `Copy` control these semantics."
                ),
            }]),
        });
    }
    messages
}

/// Helper: generate conversation with tool use patterns.
fn generate_tool_conversation(turns: usize) -> Vec<Message> {
    let mut messages = vec![Message {
        role: "system".to_string(),
        content: MessageContent::Text("You are a coding assistant with file tools.".to_string()),
    }];

    for i in 0..turns {
        // User asks to read a file
        messages.push(Message {
            role: "user".to_string(),
            content: MessageContent::Text(format!(
                "Turn {i}: Read the file src/main.rs and explain the entry point"
            )),
        });
        // Assistant uses tool
        messages.push(Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text {
                    text: format!("Turn {i}: Let me read that file for you."),
                },
                ContentBlock::ToolUse {
                    id: format!("tool_{i}"),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "src/main.rs"}),
                },
            ]),
        });
        // Tool result
        messages.push(Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::ToolResult {
                    tool_use_id: format!("tool_{i}"),
                    content: Some(shannon_core::api::ToolResultContent::Single(format!(
                        "fn main() {{\n    println!(\"Hello, world!\");\n    // Turn {i} file content\n    let x = 42;\n}}"
                    ))),
                    is_error: Some(false),
                },
            ]),
        });
        // Assistant explains
        messages.push(Message {
            role: "assistant".to_string(),
            content: MessageContent::Text(format!(
                "Turn {i}: The entry point is `fn main()`. It prints a greeting and defines a variable x = 42."
            )),
        });
    }
    messages
}

// ============================================================================
// E2E Tests
// ============================================================================

#[test]
#[ignore] // Requires running Ollama instance with OLLAMA_E2E=1 and OLLAMA_MODEL set
fn e2e_ollama_compact_basic_summarization() {
    let Some(client) = ollama_client() else {
        eprintln!("SKIP: OLLAMA_E2E not set");
        return;
    };

    let rt = tokio::runtime::Runtime::new().expect("create runtime");
    let handle = rt.handle().clone();

    let mut engine = CompactEngine::with_llm_summarizer_on_runtime(client, handle)
        .expect("create compact engine");

    let mut messages = generate_conversation(20);
    let original_len = messages.len();
    let start = Instant::now();

    let result = engine
        .compact(&mut messages)
        .expect("compact should succeed");
    let elapsed = start.elapsed();

    println!(
        "Basic compact: {} -> {} msgs, {:.1}% reduction, {:.2}s",
        original_len,
        messages.len(),
        result.reduction_ratio * 100.0,
        elapsed.as_secs_f64()
    );

    // Summary should replace old messages
    assert!(messages.len() < original_len, "messages should be reduced");
    assert!(
        result.messages_removed > 0,
        "some messages should be removed"
    );
    assert!(
        result.reduction_ratio > 0.0,
        "should have positive reduction"
    );

    // First message should be a summary
    if let MessageContent::Text(text) = &messages[0].content {
        assert!(
            text.contains("summary") || text.contains("Summary") || text.contains("compacted"),
            "first message should be a summary, got: {}",
            &text[..text.len().min(100)]
        );
    }
}

#[test]
#[ignore] // Requires running Ollama instance with OLLAMA_E2E=1 and OLLAMA_MODEL set
fn e2e_ollama_compact_tool_use_conversation() {
    let Some(client) = ollama_client() else {
        eprintln!("SKIP: OLLAMA_E2E not set");
        return;
    };

    let rt = tokio::runtime::Runtime::new().expect("create runtime");
    let handle = rt.handle().clone();

    let mut engine = CompactEngine::with_llm_summarizer_on_runtime(client, handle)
        .expect("create compact engine");

    let mut messages = generate_tool_conversation(15);
    let original_len = messages.len();
    let start = Instant::now();

    let result = engine
        .compact(&mut messages)
        .expect("compact tool conversation");
    let elapsed = start.elapsed();

    println!(
        "Tool-use compact: {} -> {} msgs, {:.1}% reduction, {:.2}s",
        original_len,
        messages.len(),
        result.reduction_ratio * 100.0,
        elapsed.as_secs_f64()
    );

    assert!(messages.len() < original_len);
    assert!(result.reduction_ratio > 0.0);
}

#[test]
#[ignore] // Requires running Ollama instance with OLLAMA_E2E=1 and OLLAMA_MODEL set
fn e2e_ollama_micro_compact() {
    let Some(client) = ollama_client() else {
        eprintln!("SKIP: OLLAMA_E2E not set");
        return;
    };

    let rt = tokio::runtime::Runtime::new().expect("create runtime");
    let handle = rt.handle().clone();

    let mut config = CompactConfig::default();
    config.enable_micro_compact = true;
    config.micro_compact_threshold = 200; // low threshold for testing

    let engine = CompactEngine::new(
        config,
        Box::new(shannon_core::compact::LlmSummarizer::with_handle(
            client, handle,
        )),
    )
    .expect("create engine");

    // Create a single very large message
    let mut messages = vec![Message {
        role: "user".to_string(),
        content: MessageContent::Text(
            "Analyze this code thoroughly:\n".to_string()
                + &"fn process(data: &mut Vec<i32>) -> Result<(), Error> {\n".repeat(50),
        ),
    }];

    let start = Instant::now();
    let result = engine.micro_compact(&mut messages).expect("micro compact");
    let elapsed = start.elapsed();

    println!(
        "Micro compact: {} msgs compacted, {:.1}% reduction, {:.2}s",
        result.messages_compacted,
        result.reduction_ratio * 100.0,
        elapsed.as_secs_f64()
    );

    assert!(
        result.messages_compacted > 0,
        "should have compacted at least one message"
    );
}

#[test]
#[ignore] // Requires running Ollama instance with OLLAMA_E2E=1 and OLLAMA_MODEL set
fn e2e_ollama_compact_preserves_recent_messages() {
    let Some(client) = ollama_client() else {
        eprintln!("SKIP: OLLAMA_E2E not set");
        return;
    };

    let rt = tokio::runtime::Runtime::new().expect("create runtime");
    let handle = rt.handle().clone();

    let mut config = CompactConfig::default();
    config.keep_recent_count = 6; // keep last 3 turns (6 messages)

    let mut engine = CompactEngine::new(
        config,
        Box::new(shannon_core::compact::LlmSummarizer::with_handle(
            client, handle,
        )),
    )
    .expect("create engine");

    let mut messages = generate_conversation(20);
    let _last_user = messages.last().cloned();

    let result = engine.compact(&mut messages).expect("compact");

    // Last message should still be present
    assert!(
        messages.iter().any(|m| m.role == "user"
            && matches!(&m.content, MessageContent::Text(t) if t.contains("Turn 19"))),
        "last user turn should be preserved"
    );

    println!(
        "Preserve recent: {} removed, {} remaining",
        result.messages_removed,
        messages.len()
    );
}

#[test]
#[ignore] // Requires running Ollama instance with OLLAMA_E2E=1 and OLLAMA_MODEL set
fn e2e_ollama_compact_group_based() {
    let Some(client) = ollama_client() else {
        eprintln!("SKIP: OLLAM_E2E not set");
        return;
    };

    let rt = tokio::runtime::Runtime::new().expect("create runtime");
    let handle = rt.handle().clone();

    let mut engine =
        CompactEngine::with_llm_summarizer_on_runtime(client, handle).expect("create engine");

    let mut messages = generate_tool_conversation(10);
    let original_len = messages.len();
    let start = Instant::now();

    let result = engine.group_compact(&mut messages).expect("group compact");
    let elapsed = start.elapsed();

    println!(
        "Group compact: {} -> {} msgs, {:.1}% reduction, {:.2}s",
        original_len,
        messages.len(),
        result.reduction_ratio * 100.0,
        elapsed.as_secs_f64()
    );

    assert!(messages.len() < original_len);

    // Summary should mention groups
    if let MessageContent::Text(text) = &messages[0].content {
        assert!(
            text.contains("Group") || text.contains("group"),
            "group compact summary should mention groups"
        );
    }
}

#[test]
#[ignore] // Requires running Ollama instance with OLLAMA_E2E=1 and OLLAMA_MODEL set
fn e2e_ollama_compact_large_conversation_stress() {
    let Some(client) = ollama_client() else {
        eprintln!("SKIP: OLLAMA_E2E not set");
        return;
    };

    let rt = tokio::runtime::Runtime::new().expect("create runtime");
    let handle = rt.handle().clone();

    let mut engine =
        CompactEngine::with_llm_summarizer_on_runtime(client, handle).expect("create engine");

    // 50 turns = 101 messages — stress test
    let mut messages = generate_conversation(50);
    let original_len = messages.len();
    let start = Instant::now();

    let result = engine
        .compact(&mut messages)
        .expect("compact large conversation");
    let elapsed = start.elapsed();

    println!(
        "Large compact (50 turns): {} -> {} msgs, {:.1}% reduction, {:.2}s",
        original_len,
        messages.len(),
        result.reduction_ratio * 100.0,
        elapsed.as_secs_f64()
    );

    assert!(messages.len() < original_len);
    assert!(
        result.reduction_ratio > 0.3,
        "large conversation should have significant reduction, got {:.1}%",
        result.reduction_ratio * 100.0
    );
    assert!(
        elapsed.as_secs() < 120,
        "should complete within 120s, took {:.1}s",
        elapsed.as_secs_f64()
    );
}

#[test]
#[ignore] // Requires running Ollama instance with OLLAMA_E2E=1 and OLLAMA_MODEL set
fn e2e_ollama_compact_summary_quality() {
    let Some(client) = ollama_client() else {
        eprintln!("SKIP: OLLAMA_E2E not set");
        return;
    };

    let rt = tokio::runtime::Runtime::new().expect("create runtime");
    let handle = rt.handle().clone();

    let mut engine =
        CompactEngine::with_llm_summarizer_on_runtime(client, handle).expect("create engine");

    // Create conversation with specific topics to verify summarizer captures them
    // Must have enough messages to trigger compaction (default keep_recent_count=10)
    let mut messages = vec![
        Message { role: "system".to_string(), content: MessageContent::Text("Test assistant".to_string()) },
        Message { role: "user".to_string(), content: MessageContent::Text("Let's discuss authentication in web applications using JWT tokens".to_string()) },
        Message { role: "assistant".to_string(), content: MessageContent::Text("JWT tokens have three parts: header, payload, signature. They're stateless and good for microservices.".to_string()) },
        Message { role: "user".to_string(), content: MessageContent::Text("Now explain database indexing strategies for PostgreSQL".to_string()) },
        Message { role: "assistant".to_string(), content: MessageContent::Text("PostgreSQL supports B-tree, GIN, GiST, and hash indexes. B-tree is default, GIN is great for full-text search.".to_string()) },
        Message { role: "user".to_string(), content: MessageContent::Text("What about caching with Redis?".to_string()) },
        Message { role: "assistant".to_string(), content: MessageContent::Text("Redis supports strings, lists, sets, sorted sets, hashes. Use it for session storage, rate limiting, and pub/sub.".to_string()) },
        // Additional turns to ensure compaction triggers
        Message { role: "user".to_string(), content: MessageContent::Text("Explain Docker container networking".to_string()) },
        Message { role: "assistant".to_string(), content: MessageContent::Text("Docker networking uses bridge, host, overlay, and macvlan drivers. Bridge is default for standalone containers.".to_string()) },
        Message { role: "user".to_string(), content: MessageContent::Text("How do Kubernetes deployments work?".to_string()) },
        Message { role: "assistant".to_string(), content: MessageContent::Text("K8s deployments manage ReplicaSets with rolling updates, rollbacks, and scaling via kubectl scale.".to_string()) },
        Message { role: "user".to_string(), content: MessageContent::Text("Tell me about GraphQL vs REST APIs".to_string()) },
        Message { role: "assistant".to_string(), content: MessageContent::Text("GraphQL offers typed schemas, single endpoint, and client-driven queries. REST is simpler with HTTP verbs and resource URLs.".to_string()) },
        Message { role: "user".to_string(), content: MessageContent::Text("What's the current topic? Remember what we discussed.".to_string()) },
        Message { role: "assistant".to_string(), content: MessageContent::Text("We discussed JWT auth, PostgreSQL indexes, Redis caching, Docker networking, K8s deployments, and GraphQL vs REST.".to_string()) },
    ];

    let _result = engine.compact(&mut messages).expect("compact");

    // The summary should capture at least some key topics
    if let MessageContent::Text(summary) = &messages[0].content {
        let lower = summary.to_lowercase();
        let topics_found = [
            ("jwt", "auth"),
            ("postgres", "database"),
            ("redis", "cache"),
        ]
        .iter()
        .filter(|(a, b)| lower.contains(a) || lower.contains(b))
        .count();

        println!("Summary quality: {topics_found}/3 topics found");
        println!("Summary preview: {}...", &summary[..summary.len().min(300)]);

        // At least some topics should be captured (not all, LLM may vary)
        assert!(
            topics_found >= 1,
            "summary should capture at least 1 topic from 3, got summary: {}",
            &summary[..summary.len().min(200)]
        );
    }
}
