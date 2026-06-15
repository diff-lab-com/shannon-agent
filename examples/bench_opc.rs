//! Standalone benchmark for `get_opc_metrics` against the real
//! `~/.claude/tasks/` tree. Run from $HOME so the relative path resolves.
//!
//! ```sh
//! cd ~ && cargo run --manifest-path <repo>/Cargo.toml \
//!     --example bench_opc --features tauri -q
//! ```

use std::time::Instant;

#[tokio::main]
async fn main() {
    let start = Instant::now();
    let metrics = shannon_desktop::scheduled_commands::get_opc_metrics()
        .await
        .expect("get_opc_metrics");
    let elapsed = start.elapsed();
    println!("get_opc_metrics: {:?}", elapsed);
    println!(
        "  total={}, completion_rate={:.3}, by_status={}, by_assignee={}, daily={}",
        metrics.total,
        metrics.completion_rate,
        metrics.by_status.len(),
        metrics.by_assignee.len(),
        metrics.daily.len(),
    );
    let total_daily: u32 = metrics.daily.iter().map(|b| b.created).sum();
    println!("  daily created (last 7d): {}", total_daily);
}
