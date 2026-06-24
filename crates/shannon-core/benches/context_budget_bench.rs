//! Benchmarks for context window budget calculations.
//!
//! Measures performance of budget creation, priority allocation,
//! schema token estimation, schema budget checks, and tool deferral
//! decisions with varying numbers of tool definitions.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use serde_json::json;

use shannon_engine::api::ToolDefinition;
use shannon_engine::context_budget::{ContextBudget, PriorityBudget};
use shannon_engine::context_pressure::PressureLevel;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a set of tool definitions with realistic schema sizes.
fn make_tool_definitions(count: usize) -> Vec<ToolDefinition> {
    (0..count)
        .map(|i| ToolDefinition {
            name: format!("tool_{i:04}"),
            description: format!(
                "Tool number {i} for benchmarking purposes. \
                 This tool performs an operation on the specified input data \
                 and returns a structured result with metadata about the processing."
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": format!("The primary input parameter for tool {i}")
                    },
                    "options": {
                        "type": "object",
                        "properties": {
                            "verbose": {"type": "boolean"},
                            "timeout": {"type": "number"},
                            "retries": {"type": "integer"}
                        }
                    },
                    "path": {
                        "type": "string",
                        "description": "File system path for the operation"
                    }
                },
                "required": ["input"]
            }),
            cache_control: None,
            strict: None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_budget_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_budget/creation");
    for &tokens in &[1_000usize, 100_000, 200_000, 1_000_000] {
        group.bench_with_input(BenchmarkId::new("new", tokens), &tokens, |b, &tokens| {
            b.iter(|| ContextBudget::new(tokens));
        });
    }
    group.finish();
}

fn bench_budget_with_fractions(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_budget/with_fractions");
    for &tokens in &[1_000usize, 100_000, 200_000] {
        group.bench_with_input(
            BenchmarkId::new("custom_fractions", tokens),
            &tokens,
            |b, &tokens| {
                b.iter(|| ContextBudget::with_fractions(tokens, 0.10, 0.30, 0.60));
            },
        );
    }
    group.finish();
}

fn bench_priority_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_budget/priority_allocation");
    for &tokens in &[1_000usize, 100_000, 200_000] {
        let budget = ContextBudget::new(tokens);
        group.bench_with_input(BenchmarkId::new("allocate", tokens), &tokens, |b, _| {
            b.iter(|| budget.priority_allocation());
        });
    }
    group.finish();
}

fn bench_pressure_adjustment(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_budget/pressure_adjustment");
    let levels = [
        ("Low", PressureLevel::Low),
        ("Normal", PressureLevel::Normal),
        ("High", PressureLevel::High),
        ("Critical", PressureLevel::Critical),
        ("Emergency", PressureLevel::Emergency),
    ];
    for (label, level) in levels {
        group.bench_with_input(BenchmarkId::new("adjust", label), &level, |b, &level| {
            b.iter_batched(
                || ContextBudget::new(200_000),
                |mut budget| budget.adjust_for_pressure(level),
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_estimate_schema_tokens(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_budget/estimate_schema_tokens");
    for &n_tools in &[10usize, 50, 100, 200] {
        let defs = make_tool_definitions(n_tools);
        group.bench_with_input(
            BenchmarkId::new("total_tokens", n_tools),
            &n_tools,
            |b, _| {
                b.iter(|| ContextBudget::estimate_total_schema_tokens(&defs));
            },
        );
    }
    group.finish();
}

fn bench_check_schema_budget(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_budget/check_schema_budget");
    for &n_tools in &[10usize, 50, 100, 200] {
        let defs = make_tool_definitions(n_tools);
        let budget = ContextBudget::new(200_000);
        group.bench_with_input(BenchmarkId::new("check", n_tools), &n_tools, |b, _| {
            b.iter(|| budget.check_schema_budget(&defs));
        });
    }
    group.finish();
}

fn bench_tools_to_defer(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_budget/tools_to_defer");
    for &n_tools in &[10usize, 50, 100, 200] {
        let defs = make_tool_definitions(n_tools);
        // Use a small budget so deferral logic actually runs
        let budget = ContextBudget::new(5_000);
        group.bench_with_input(BenchmarkId::new("defer", n_tools), &n_tools, |b, _| {
            b.iter(|| budget.tools_to_defer(&defs));
        });
    }
    group.finish();
}

fn bench_priority_budget_allocate(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_budget/priority_budget_allocate");
    let pb = PriorityBudget::default();
    for &conv_budget in &[1_000usize, 10_000, 100_000, 120_000] {
        group.bench_with_input(
            BenchmarkId::new("allocate", conv_budget),
            &conv_budget,
            |b, &conv_budget| {
                b.iter(|| pb.allocate(conv_budget));
            },
        );
    }
    group.finish();
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .noise_threshold(0.03)
        .confidence_level(0.98)
        .significance_level(0.02)
        .sample_size(50)
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = bench_budget_creation,
        bench_budget_with_fractions,
        bench_priority_allocation,
        bench_pressure_adjustment,
        bench_estimate_schema_tokens,
        bench_check_schema_budget,
        bench_tools_to_defer,
        bench_priority_budget_allocate
}
criterion_main!(benches);
