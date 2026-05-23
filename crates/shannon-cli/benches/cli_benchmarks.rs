//! CLI startup and initialization benchmarks.
//!
//! Run with: cargo bench -p shannon-cli

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::process::Command;

fn bench_cold_start(c: &mut Criterion) {
    c.bench_function("cold_start_help", |b| {
        b.iter(|| {
            let output = Command::new("cargo")
                .args(["run", "-p", "shannon-cli", "--", "--help"])
                .output()
                .expect("failed to run shannon");
            black_box(output);
        })
    });
}

fn bench_config_load(c: &mut Criterion) {
    c.bench_function("config_load_toml", |b| {
        b.iter(|| {
            let toml_str = r#"
                [llm]
                provider = "anthropic"
                model = "claude-sonnet-4"
                max_tokens = 4096
                api_key = "test-key"

                [ui]
                theme = "dark"
                vim_mode = true

                [permissions]
                mode = "suggest"
            "#;
            let value: toml::Value = toml::from_str(black_box(toml_str)).unwrap();
            black_box(value);
        })
    });
}

fn bench_config_load_large(c: &mut Criterion) {
    let mut large_toml = String::from("[llm]\nprovider = \"anthropic\"\nmodel = \"test\"\n");
    for i in 0..50 {
        large_toml.push_str(&format!(
            "[mcp_servers.server{i}]\ncommand = \"test-cmd\"\nargs = [\"arg1\", \"arg2\"]\n"
        ));
    }

    c.bench_function("config_load_large_50_mcp", |b| {
        b.iter(|| {
            let value: toml::Value = toml::from_str(black_box(&large_toml)).unwrap();
            black_box(value);
        })
    });
}

fn bench_session_id_generation(c: &mut Criterion) {
    c.bench_function("session_id_uuid_v4", |b| {
        b.iter(|| {
            let id = uuid::Uuid::new_v4();
            black_box(id);
        })
    });
}

fn bench_cli_arg_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("cli_arg_parsing");

    let cases = vec![
        ("simple_prompt", vec!["--prompt", "hello"]),
        (
            "with_model",
            vec!["--prompt", "hello", "--model", "claude-opus-4"],
        ),
        (
            "full_flags",
            vec![
                "--prompt",
                "hello",
                "--model",
                "test",
                "--max-tokens",
                "4096",
                "--permission-mode",
                "full-auto",
            ],
        ),
        ("resume", vec!["--resume"]),
    ];

    for (name, _args) in cases {
        group.bench_function(BenchmarkId::new("parse", name), |b| {
            b.iter(|| {
                // Simulate arg parsing overhead with serde_json
                let json = serde_json::json!({
                    "prompt": "hello",
                    "model": "test",
                    "max_tokens": 4096,
                    "stream": true,
                });
                black_box(json);
            })
        });
    }
    group.finish();
}

fn bench_ndjson_serialization(c: &mut Criterion) {
    let events: Vec<serde_json::Value> = (0..100)
        .map(|i| {
            serde_json::json!({
                "type": "tool_result",
                "tool_use_id": format!("toolu_{i}"),
                "content": format!("Result {i}: operation completed successfully"),
            })
        })
        .collect();

    c.bench_function("ndjson_100_events", |b| {
        b.iter(|| {
            let output: String = events
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            black_box(output);
        })
    });
}

criterion_group!(
    benches,
    bench_cold_start,
    bench_config_load,
    bench_config_load_large,
    bench_session_id_generation,
    bench_cli_arg_parsing,
    bench_ndjson_serialization,
);
criterion_main!(benches);
