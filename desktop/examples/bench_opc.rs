//! Standalone benchmark for the OPC metrics pipeline — the exact walk +
//! aggregation `get_opc_metrics` performs, composed from its public pieces
//! (`anchored_tasks_dir_base` / `list_tasks_in` / `collect_daily_buckets_in`
//! / `compute_opc_metrics`) so no Tauri state is needed. R2-P1-3: the tasks
//! root is anchored (with no working_dir it resolves to $HOME), never the
//! process CWD.
//!
//! ```sh
//! cargo run --manifest-path <repo>/Cargo.toml \
//!     --example bench_opc --features tauri -q
//! ```

use std::time::Instant;

#[tokio::main]
async fn main() {
    let start = Instant::now();
    let root = shannon_desktop::commands_tasks::anchored_tasks_dir_base(None)
        .expect("anchor tasks root")
        .join(".claude")
        .join("tasks");
    let tasks = shannon_desktop::commands_tasks::list_tasks_in(&root).expect("list_tasks_in");
    let daily = shannon_desktop::scheduled_commands::collect_daily_buckets_in(&root)
        .expect("collect_daily_buckets_in");
    let metrics = shannon_desktop::scheduled_commands::compute_opc_metrics(&tasks, daily);
    let elapsed = start.elapsed();
    println!(
        "opc metrics pipeline (root={}): {elapsed:?}",
        root.display()
    );
    println!(
        "  total={}, completion_rate={:.3}, by_status={}, by_assignee={}, daily={}",
        metrics.total,
        metrics.completion_rate,
        metrics.by_status.len(),
        metrics.by_assignee.len(),
        metrics.daily.len(),
    );
    let total_daily: u32 = metrics.daily.iter().map(|b| b.created).sum();
    println!("  daily created (last 7d): {total_daily}");
}
