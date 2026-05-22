//! Core performance benchmarks for shannon-core.
//!
//! Benchmarks message serialization, token estimation, compaction,
//! and system prompt construction.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use serde_json::json;

use shannon_core::api::{
    ContentBlock, Message, MessageContent, ToolDefinition, ToolResultContent,
};
use shannon_core::compact::{CompactConfig, CompactEngine, RuleBasedSummarizer};
use shannon_core::token_estimation::{ConversationMessageSummary, TokenEstimator};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a simple text message.
fn make_text_message(role: &str, text: &str) -> Message {
    Message {
        role: role.to_string(),
        content: MessageContent::Text(text.to_string()),
    }
}

/// Build a tool-use message (assistant requesting a tool call).
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

/// Build a tool-result message.
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

/// Build a conversation with interleaved user/assistant/tool messages.
fn build_conversation(turns: usize) -> Vec<Message> {
    let mut messages = Vec::with_capacity(turns * 3);
    for i in 0..turns {
        messages.push(make_text_message(
            "user",
            &format!("Please help me with task {i}. I need you to analyze the codebase and suggest improvements."),
        ));
        messages.push(make_tool_use_message(
            "Read",
            &format!("tool_{i}_a"),
            json!({"path": format!("src/module_{i}.rs")}),
        ));
        messages.push(make_tool_result_message(
            &format!("tool_{i}_a"),
            &format!("// module {i}\nfn process() -> bool {{ true }}\n// ... (simulated output)"),
        ));
        messages.push(make_text_message(
            "assistant",
            &format!("I've analyzed module {i}. The code looks good but consider adding error handling."),
        ));
    }
    messages
}

/// Build a system prompt string with embedded tool definitions.
fn build_system_prompt(tool_count: usize) -> String {
    let mut parts = Vec::with_capacity(tool_count + 3);
    parts.push("You are an expert software engineering assistant.".to_string());
    parts.push("You have access to the following tools:\n".to_string());

    for i in 0..tool_count {
        parts.push(format!(
            "## Tool: tool_{i}\nDescription: Performs operation {i}\n\
             Input schema: {{\"type\":\"object\",\"properties\":{{\"path\":{{\"type\":\"string\"}}}}}}\n"
        ));
    }

    parts.push("\nAlways follow best practices and write clean code.".to_string());
    parts.join("\n")
}

// ---------------------------------------------------------------------------
// Benchmark groups
// ---------------------------------------------------------------------------

fn bench_message_serialization(c: &mut Criterion) {
    let single_msg = make_text_message(
        "user",
        "Hello, can you help me refactor the authentication module in my Rust project?",
    );

    let single_json = serde_json::to_string(&single_msg).unwrap();
    c.bench_function("message_serialize_single", |b| {
        b.iter(|| serde_json::to_string(&single_msg))
    });
    c.bench_function("message_deserialize_single", |b| {
        b.iter(|| serde_json::from_str::<Message>(&single_json))
    });

    // Batch of 10 messages
    let batch: Vec<Message> = build_conversation(3);
    let batch_json = serde_json::to_string(&batch).unwrap();
    c.bench_function("message_serialize_batch_10", |b| {
        b.iter(|| serde_json::to_string(&batch))
    });
    c.bench_function("message_deserialize_batch_10", |b| {
        b.iter(|| serde_json::from_str::<Vec<Message>>(&batch_json))
    });
}

fn bench_context_estimation(c: &mut Criterion) {
    let estimator = TokenEstimator::new();
    let models = ["claude-3-opus", "gpt-4o", "gemini-pro"];

    let mut group = c.benchmark_group("context_estimation");

    for &msg_count in &[5, 20, 100] {
        let messages: Vec<ConversationMessageSummary> = (0..msg_count)
            .map(|i| ConversationMessageSummary {
                role: if i % 2 == 0 { "user".into() } else { "assistant".into() },
                content: format!(
                    "This is message number {i} with some typical content about code analysis."
                ),
            })
            .collect();

        group.bench_with_input(
            BenchmarkId::new("rough_estimate", msg_count),
            &messages,
            |b, msgs| {
                b.iter(|| estimator.estimate_for_messages(msgs));
            },
        );

        for &model in &models {
            let model_label = format!("precise_{model}");
            group.bench_with_input(
                BenchmarkId::new(&model_label, msg_count),
                &messages,
                |b, msgs| {
                    b.iter(|| estimator.count_precise_for_messages(msgs, model));
                },
            );
        }
    }

    group.finish();
}

fn bench_compaction_small(c: &mut Criterion) {
    // 20-turn conversation (80 messages: user + tool_use + tool_result + assistant per turn)
    let messages = build_conversation(20);
    let config = CompactConfig::default();
    let summarizer = RuleBasedSummarizer::new();

    c.bench_function("compaction_20_turns", |b| {
        b.iter_batched(
            || {
                // Setup: create a fresh engine and clone messages each iteration
                let engine = CompactEngine::new(config.clone(), Box::new(summarizer.clone()))
                    .expect("engine creation should not fail");
                (engine, messages.clone())
            },
            |(mut engine, mut msgs)| engine.compact(&mut msgs),
            criterion::BatchSize::SmallInput,
        )
    });
}

fn bench_compaction_large(c: &mut Criterion) {
    // 200-turn conversation (800 messages)
    let messages = build_conversation(200);
    let config = CompactConfig::default();
    let summarizer = RuleBasedSummarizer::new();

    c.bench_function("compaction_200_turns", |b| {
        b.iter_batched(
            || {
                let engine = CompactEngine::new(config.clone(), Box::new(summarizer.clone()))
                    .expect("engine creation should not fail");
                (engine, messages.clone())
            },
            |(mut engine, mut msgs)| engine.compact(&mut msgs),
            criterion::BatchSize::LargeInput,
        )
    });
}

fn bench_system_prompt_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("system_prompt");

    for &tool_count in &[5, 20, 50] {
        group.bench_with_input(
            BenchmarkId::new("build_prompt", tool_count),
            &tool_count,
            |b, &count| {
                b.iter(|| build_system_prompt(count));
            },
        );
    }

    // Also benchmark serializing tool definitions (as they appear in API requests)
    for &tool_count in &[5, 20, 50] {
        let tools: Vec<ToolDefinition> = (0..tool_count)
            .map(|i| ToolDefinition {
                name: format!("tool_{i}"),
                description: format!("Performs operation {i} on the codebase"),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File path"},
                        "content": {"type": "string", "description": "File content"}
                    },
                    "required": ["path"]
                }),
                strict: None,
            })
            .collect();

        group.bench_with_input(
            BenchmarkId::new("serialize_tool_defs", tool_count),
            &tools,
            |b, defs| {
                b.iter(|| serde_json::to_string(defs));
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_message_serialization,
    bench_context_estimation,
    bench_compaction_small,
    bench_compaction_large,
    bench_system_prompt_construction,
);
criterion_main!(benches);
