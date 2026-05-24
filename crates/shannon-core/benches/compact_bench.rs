//! Benchmarks for the message compaction system.
//!
//! Measures performance of context analysis, message grouping,
//! pruning stale tool results, and full compaction with the
//! rule-based summarizer at 100, 500, and 1000 messages.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use serde_json::json;

use shannon_core::api::{ContentBlock, Message, MessageContent, ToolResultContent};
use shannon_core::compact::{CompactConfig, CompactEngine, RuleBasedSummarizer};

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
        role: "user".to_string(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.to_string(),
            content: Some(ToolResultContent::Single(result.to_string())),
            is_error: None,
        }]),
    }
}

/// Build a realistic conversation with interleaved user/assistant/tool messages.
/// Each "turn" produces 3 messages: user query, assistant tool_use, tool result,
/// followed by an assistant text response.
fn build_conversation(turns: usize) -> Vec<Message> {
    let mut messages = Vec::with_capacity(turns * 4);
    for i in 0..turns {
        messages.push(make_text_message(
            "user",
            &format!(
                "Please help me with task {i}. I need you to analyze the codebase \
                 and suggest improvements to the module handling."
            ),
        ));
        messages.push(make_tool_use_message(
            "Read",
            &format!("tool_{i}_read"),
            json!({"file_path": format!("src/module_{i}.rs")}),
        ));
        messages.push(make_tool_result_message(
            &format!("tool_{i}_read"),
            &format!(
                "File content for module_{i}.rs:\n\
                 fn process_{i}() -> Result<(), Error> {{\n\
                     let data = fetch_data()?;\n\
                     let processed = transform(data)?;\n\
                     Ok(())\n\
                 }}\n"
            ),
        ));
        messages.push(make_text_message(
            "assistant",
            &format!(
                "I've analyzed module_{i}. The `process_{i}` function could be \
                 improved by adding proper error handling and reducing unnecessary \
                 allocations. Consider using a streaming approach for large datasets."
            ),
        ));
    }
    messages
}

fn make_engine() -> CompactEngine {
    let config = CompactConfig {
        max_context_tokens: 200_000,
        keep_recent_count: 10,
        trigger_threshold: 0.75,
        enable_micro_compact: true,
        micro_compact_threshold: 4096,
        ..Default::default()
    };
    CompactEngine::new(config, Box::new(RuleBasedSummarizer::new())).unwrap()
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_analyze_context(c: &mut Criterion) {
    let mut group = c.benchmark_group("compact/analyze_context");
    for size in &[100, 500, 1000] {
        let messages = build_conversation(*size);
        let engine = make_engine();
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| engine.analyze_context(&messages));
        });
    }
    group.finish();
}

fn bench_group_messages(c: &mut Criterion) {
    let mut group = c.benchmark_group("compact/group_messages");
    for size in &[100, 500, 1000] {
        let messages = build_conversation(*size);
        let engine = make_engine();
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| engine.group_messages(&messages));
        });
    }
    group.finish();
}

fn bench_prune_stale_tool_results(c: &mut Criterion) {
    let mut group = c.benchmark_group("compact/prune_stale_tool_results");
    for size in &[100, 500, 1000] {
        let mut messages = Vec::with_capacity(size * 2);
        for i in 0..*size {
            messages.push(make_tool_use_message(
                "Read",
                &format!("tool_{i}"),
                json!({"path": format!("f{i}.rs")}),
            ));
            messages.push(make_tool_result_message(
                &format!("tool_{i}"),
                &"x".repeat(800),
            ));
        }
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| CompactEngine::prune_stale_tool_results(&mut messages));
        });
    }
    group.finish();
}

fn bench_compact(c: &mut Criterion) {
    let mut group = c.benchmark_group("compact/compact");
    for size in &[100, 500, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter_batched(
                || {
                    let engine = make_engine();
                    let msgs = build_conversation(*size);
                    // Ensure enough messages to pass the keep_recent_count check
                    (engine, msgs)
                },
                |(mut engine, mut msgs)| engine.compact(&mut msgs),
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_micro_compact(c: &mut Criterion) {
    let mut group = c.benchmark_group("compact/micro_compact");
    for size in &[100, 500, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter_batched(
                || {
                    let engine = make_engine();
                    // Messages with oversized content (above micro_compact_threshold)
                    let msgs: Vec<Message> = (0..*size)
                        .map(|i| {
                            make_tool_result_message(
                                &format!("tool_{i}"),
                                &format!(
                                    "Detailed tool output for task {i}: {}",
                                    "data ".repeat(2000)
                                ),
                            )
                        })
                        .collect();
                    (engine, msgs)
                },
                |(engine, mut msgs)| engine.micro_compact(&mut msgs),
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_post_compact_cleanup(c: &mut Criterion) {
    let mut group = c.benchmark_group("compact/post_compact_cleanup");
    for size in &[100, 500, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter_batched(
                || {
                    let engine = make_engine();
                    let mut msgs = Vec::with_capacity(*size + 5);
                    // Prepend system messages including a summary
                    msgs.push(make_text_message(
                        "system",
                        "[Previous conversation summary - 10 messages compacted]\nUser worked on tasks.",
                    ));
                    msgs.push(make_text_message("system", "You are a helpful assistant."));
                    for i in 0..*size {
                        msgs.push(make_text_message(
                            if i % 2 == 0 { "user" } else { "assistant" },
                            &format!("Message {i} with realistic content about code analysis"),
                        ));
                    }
                    (engine, msgs)
                },
                |(engine, mut msgs)| engine.post_compact_cleanup(&mut msgs),
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_analyze_context,
    bench_group_messages,
    bench_prune_stale_tool_results,
    bench_compact,
    bench_micro_compact,
    bench_post_compact_cleanup,
);
criterion_main!(benches);
