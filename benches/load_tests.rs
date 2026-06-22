//! Synthetic load benchmarks for OPC metric aggregation.
//!
//! Exercises `compute_opc_metrics` at 100 / 1k / 10k task scales to verify the
//! aggregation loop stays well under interactive latency as the task list grows.
//!
//! Run with: `cargo bench --bench load_tests`

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use shannon_desktop::commands::TaskInfo;
use shannon_desktop::scheduled_commands::compute_opc_metrics;

fn synthetic_tasks(n: usize) -> Vec<TaskInfo> {
    let statuses = [
        "pending",
        "in_progress",
        "completed",
        "done",
        "blocked",
        "running",
    ];
    let priorities: [Option<&str>; 7] = [
        Some("P0"),
        Some("P1"),
        Some("P2"),
        Some("P3"),
        None,
        Some("P0"),
        Some("P2"),
    ];
    let assignees = [
        Some("alice"),
        Some("bob"),
        Some("carol"),
        None,
        Some("dave"),
        Some("eve"),
    ];

    (0..n)
        .map(|i| TaskInfo {
            id: format!("task-{i}"),
            title: format!("Task {i}"),
            status: statuses[i % statuses.len()].to_string(),
            assignee: assignees[i % assignees.len()].map(str::to_string),
            priority: priorities[i % priorities.len()].map(str::to_string),
            description: Some(format!("Description for task {i}")),
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            due_date: None,
            active_form: None,
            execution_mode: None,
            team: None,
        })
        .collect()
}

fn bench_compute_opc_metrics(c: &mut Criterion) {
    let mut group = c.benchmark_group("compute_opc_metrics");
    for size in [100usize, 1_000, 10_000] {
        let tasks = synthetic_tasks(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &tasks, |b, tasks| {
            b.iter(|| compute_opc_metrics(tasks, Vec::new()));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_compute_opc_metrics);
criterion_main!(benches);
