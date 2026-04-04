//! Comprehensive performance benchmarks for Shannon Code core components
//!
//! Benchmarks cover:
//! - CostTracker: cost calculation across multiple model providers
//! - LlmClient: message serialization/deserialization
//! - ProjectMemory: file parsing and merged loading
//! - Settings: env var parsing and priority resolution
//! - ToolRegistry: tool registration and lookup

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use serde_json::{json, Value};
use shannon_core::LlmProvider;
use shannon_core::query_engine::CostTracker;
use shannon_core::project_memory::{ProjectMemoryConfig, ProjectMemoryManager, ProjectMemoryMetadata};
use shannon_core::settings::Settings;
use std::path::PathBuf;

// ============================================================================
// CostTracker Benchmarks
// ============================================================================

fn bench_cost_tracker_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("cost_tracker");

    let models = [
        "claude-sonnet-4-20250514",
        "claude-3-5-haiku-20241022",
        "claude-3-opus-20240229",
        "gpt-4o",
        "gpt-4-turbo",
        "gpt-3.5-turbo",
        "llama3:70b",
        "mistral:7b",
        "qwen2:72b",
        "unknown-model",
    ];

    for model in models {
        group.bench_with_input(
            BenchmarkId::new("calculate_cost", model),
            model,
            |b, m| {
                b.iter(|| {
                    CostTracker::calculate_cost(black_box(m), 1_000_000, 500_000);
                })
            },
        );
    }

    group.bench_function("record_usage_1000_calls", |b| {
        b.iter(|| {
            let mut tracker = CostTracker::new("claude-sonnet-4-20250514".to_string());
            for _ in 0..1000 {
                tracker.record_usage("claude-sonnet-4-20250514", 1000, 500);
            }
            black_box(tracker);
        })
    });

    group.finish();
}

// ============================================================================
// LlmClient / Message Serialization Benchmarks
// ============================================================================

fn bench_message_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_serialization");

    // Simple text message
    let simple_message = json!({
        "role": "user",
        "content": "Hello, can you help me with my Rust project?"
    });

    group.bench_function("simple_message_serialize", |b| {
        b.iter(|| {
            let _ = serde_json::to_string(black_box(&simple_message));
        })
    });

    // Complex message with tool use
    let complex_message = json!({
        "role": "assistant",
        "content": [
            { "type": "text", "text": "I'll help you with that." },
            {
                "type": "tool_use",
                "id": "toolu_01ABC",
                "name": "Read",
                "input": { "file_path": "/src/main.rs", "offset": 0, "limit": 100 }
            }
        ]
    });

    group.bench_function("complex_message_serialize", |b| {
        b.iter(|| {
            let _ = serde_json::to_string(black_box(&complex_message));
        })
    });

    // Message request with tool definitions
    let message_request = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 8192,
        "messages": [
            { "role": "user", "content": "Read the file and fix the bug." }
        ],
        "tools": [
            {
                "name": "Read",
                "description": "Read file contents",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "file_path": { "type": "string" },
                        "offset": { "type": "integer" },
                        "limit": { "type": "integer" }
                    },
                    "required": ["file_path"]
                }
            },
            {
                "name": "Edit",
                "description": "Edit a file",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "file_path": { "type": "string" },
                        "old_string": { "type": "string" },
                        "new_string": { "type": "string" }
                    },
                    "required": ["file_path", "old_string", "new_string"]
                }
            }
        ],
        "stream": true
    });

    group.bench_function("message_request_serialize", |b| {
        b.iter(|| {
            let _ = serde_json::to_string(black_box(&message_request));
        })
    });

    // Deserialize SSE event
    let sse_event = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;

    group.bench_function("sse_event_deserialize", |b| {
        b.iter(|| {
            let _ = serde_json::from_str::<Value>(black_box(sse_event));
        })
    });

    group.finish();
}

// ============================================================================
// ProjectMemory Benchmarks
// ============================================================================

fn bench_project_memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("project_memory");

    // Generate realistic SHANNON.md content
    let memory_content = r#"---
priority: 10
disable_auto_memory: false
model: claude-sonnet-4-20250514
temperature: 0.7
max_tokens: 8192
---

# Project Instructions

This is a Rust project using the workspace pattern.

## Code Style
- Use rustfmt with default settings
- Prefer `&str` over `String` for function parameters when ownership isn't needed
- Error handling: use `anyhow::Result` for application errors, `thiserror` for library errors

## Architecture
- Core engine in `shannon-core`
- Tool implementations in `shannon-tools`
- CLI entry point in `shannon-cli`

## Testing
- Run `cargo test --workspace` before committing
- Use `cargo clippy` for lint checks

## Important Notes
- Never commit API keys or secrets
- Always use `PathBuf` for file paths, not `String`
- Prefer `Arc<dyn Tool>` for tool registration to enable dynamic dispatch
"#;

    let manager = ProjectMemoryManager::new(PathBuf::from("."));
    let path = PathBuf::from("SHANNON.md");

    group.bench_function("parse_memory_file", |b| {
        b.iter(|| {
            let _ = manager.parse_memory_file_content(black_box(memory_content), black_box(&path));
        })
    });

    // Large content parsing (simulating a large project instruction file)
    let large_content: String = std::iter::repeat("# Section\nSome instruction text here.\n")
        .take(500)
        .collect();

    group.bench_function("parse_large_memory_file", |b| {
        b.iter(|| {
            let _ = manager.parse_memory_file_content(black_box(&large_content), black_box(&path));
        })
    });

    // Metadata default construction
    group.bench_function("metadata_default", |b| {
        b.iter(ProjectMemoryMetadata::default)
    });

    // Config construction
    group.bench_function("config_construction", |b| {
        b.iter(|| {
            let _ = ProjectMemoryConfig {
                metadata: ProjectMemoryMetadata::default(),
                content: "test instructions".to_string(),
                instructions: "test instructions".to_string(),
            };
        })
    });

    // Serialization round-trip
    let config = ProjectMemoryConfig {
        metadata: ProjectMemoryMetadata::default(),
        content: memory_content.to_string(),
        instructions: memory_content.to_string(),
    };

    group.bench_function("config_serialize", |b| {
        b.iter(|| {
            let _ = serde_json::to_string(black_box(&config));
        })
    });

    group.finish();
}

// ============================================================================
// Settings Benchmarks
// ============================================================================

fn bench_settings(c: &mut Criterion) {
    let mut group = c.benchmark_group("settings");

    group.bench_function("settings_default", |b| {
        b.iter(Settings::default)
    });

    group.bench_function("settings_json_roundtrip", |b| {
        b.iter(|| {
            let settings = Settings::default();
            let json_str = serde_json::to_string(&settings).unwrap();
            let _ = serde_json::from_str::<Settings>(&json_str).unwrap();
        })
    });

    // Env content parsing
    let env_content = r#"# Shannon Code Configuration
SHANNON_MODEL=claude-sonnet-4-20250514
SHANNON_API_KEY=sk-ant-api03-test-key-here
SHANNON_MAX_TOKENS=8192
SHANNON_TEMPERATURE=0.7
SHANNON_PERMISSIONS_MODE=ask

# Optional settings
SHANNON_BASE_URL=https://api.anthropic.com
SHANNON_TIMEOUT=120
"#;

    group.bench_function("parse_env_content", |b| {
        b.iter(|| {
            let mut settings = Settings::default();
            for line in black_box(env_content).lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim();
                    // Simulate the apply logic
                    match key {
                        "SHANNON_MODEL" => settings.model = Some(value.to_string()),
                        "SHANNON_MAX_TOKENS" => {
                            if let Ok(v) = value.parse::<u32>() {
                                settings.max_tokens = Some(v);
                            }
                        }
                        "SHANNON_TEMPERATURE" => {
                            if let Ok(v) = value.parse::<f32>() {
                                settings.temperature = Some(v);
                            }
                        }
                        _ => {}
                    }
                }
            }
            black_box(settings);
        })
    });

    // CostTracker summary generation
    group.bench_function("cost_tracker_summary", |b| {
        b.iter(|| {
            let tracker = CostTracker {
                total_input_tokens: 1_500_000,
                total_output_tokens: 750_000,
                total_cost_usd: 15.75,
                model_name: "claude-sonnet-4-20250514".to_string(),
            };
            let _ = tracker.summary();
        })
    });

    group.finish();
}

// ============================================================================
// Provider Detection Benchmarks
// ============================================================================

fn bench_provider_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("provider_detection");

    let urls = [
        ("https://api.anthropic.com", LlmProvider::Anthropic),
        ("https://api.openai.com", LlmProvider::OpenAI),
        ("http://localhost:11434", LlmProvider::Ollama),
        ("https://custom.example.com/api", LlmProvider::Custom),
        ("http://192.168.1.100:8080", LlmProvider::Custom),
        ("https://api.anthropic.com/v1", LlmProvider::Anthropic),
    ];

    for (url, expected) in &urls {
        group.bench_with_input(
            BenchmarkId::new("detect", url),
            *url,
            |b, u| {
                b.iter(|| {
                    let detected = match black_box(u) {
                        s if s.contains("anthropic") => LlmProvider::Anthropic,
                        s if s.contains("openai") => LlmProvider::OpenAI,
                        s if s.contains("localhost:11434") || s.contains("ollama") => {
                            LlmProvider::Ollama
                        }
                        _ => LlmProvider::Custom,
                    };
                    assert_eq!(detected, *expected);
                    black_box(detected);
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// JSON Processing Benchmarks
// ============================================================================

fn bench_json_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_processing");

    // Large tool definitions array (simulating 48 tools)
    let tools_array: Vec<Value> = (0..48)
        .map(|i| {
            json!({
                "name": format!("Tool{}", i),
                "description": format!("Tool number {} description", i),
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "param1": { "type": "string", "description": "First parameter" },
                        "param2": { "type": "integer", "description": "Second parameter" },
                        "param3": { "type": "boolean", "description": "Third parameter" },
                    },
                    "required": ["param1"]
                }
            })
        })
        .collect();

    let tools_json = serde_json::to_string(&tools_array).unwrap();

    group.bench_function("serialize_48_tools", |b| {
        b.iter(|| {
            let _ = serde_json::to_string(black_box(&tools_array));
        })
    });

    group.bench_function("deserialize_48_tools", |b| {
        b.iter(|| {
            let _ = serde_json::from_str::<Vec<Value>>(black_box(&tools_json));
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_cost_tracker_calculation,
    bench_message_serialization,
    bench_project_memory,
    bench_settings,
    bench_provider_detection,
    bench_json_processing,
);
criterion_main!(benches);
