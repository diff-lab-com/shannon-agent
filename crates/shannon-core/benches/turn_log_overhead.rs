//! turn-log-overhead benchmark (plan §4.2, verification standard ③).
//!
//! Measures the per-turn cost the L0 session-log tee adds to the event
//! pipeline: one "turn" = the same ~60 `QueryEvent`s (text chunks, tool
//! call/result, usage, turn completion) pushed through an unbounded channel
//! *without* the tee (baseline) and *with* the tee (mapping + redaction +
//! truncation + buffered JSONL write), mirroring exactly what the engine's
//! `EventTx` does at its single injection point. The ratio of the two is
//! the pipeline overhead; the absolute delta per turn is compared against
//! real turn latency (seconds) for the <2% P95 budget.

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use uuid::Uuid;

use shannon_core::QueryEvent;
use shannon_core::session_log::TeeHandle;

/// Events of a representative single turn (one LLM step with one tool call).
fn one_turn(query_id: Uuid) -> Vec<QueryEvent> {
    let mut events = Vec::with_capacity(60);
    for i in 0..40 {
        events.push(QueryEvent::Text {
            query_id,
            content: format!("streaming delta number {i} of the assistant answer"),
        });
    }
    events.push(QueryEvent::ToolUseRequest {
        query_id,
        tool_use_id: "toolu_bench".into(),
        tool_name: "Bash".into(),
        tool_input: serde_json::json!({"command": "cargo check --workspace"}),
    });
    events.push(QueryEvent::ToolUseResult {
        query_id,
        tool_use_id: "toolu_bench".into(),
        tool_name: "Bash".into(),
        result: "Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.34s".repeat(4),
        is_error: false,
        meta: Box::new(serde_json::Value::Null),
    });
    events.push(QueryEvent::ToolProgress {
        query_id,
        tool_use_id: "toolu_bench".into(),
        tool_name: "Bash".into(),
        progress: 0.5,
        message: "running".into(),
    });
    events.push(QueryEvent::Usage {
        query_id,
        input_tokens: 12_345,
        output_tokens: 678,
        cost_usd: 0.042,
        cache_creation_tokens: 1_024,
        cache_read_tokens: 9_000,
    });
    events.push(QueryEvent::TurnCompleted {
        query_id,
        turn_number: 1,
        tokens_used: 678,
    });
    events
}

fn bench_turn_log_overhead(c: &mut Criterion) {
    let query_id = Uuid::new_v4();
    let events = one_turn(query_id);
    let turns_per_iter = 20;

    let dir = tempfile::TempDir::new().expect("tempdir for bench log");
    // New session per measurement so the log does not grow across samples.
    let mut session_counter = 0u32;

    let mut plain = c.benchmark_group("turn_log_overhead/plain_channel");
    plain.throughput(Throughput::Elements((events.len() * turns_per_iter) as u64));
    plain.bench_function("send_events", |b| {
        b.iter(|| {
            for _ in 0..turns_per_iter {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                for event in &events {
                    let _ = tx.send(Ok::<_, std::convert::Infallible>(event.clone()));
                }
                drop(tx);
                while rx.try_recv().is_ok() {}
            }
        })
    });
    plain.finish();

    let mut logged = c.benchmark_group("turn_log_overhead/tee_logged");
    logged.throughput(Throughput::Elements((events.len() * turns_per_iter) as u64));
    logged.bench_function("send_events", |b| {
        b.iter(|| {
            session_counter += 1;
            let tee = TeeHandle::open_in_dir(
                dir.path(),
                &format!("bench-session-{session_counter}"),
                "claude-sonnet-4",
                Some("anthropic"),
            );
            for _ in 0..turns_per_iter {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                for event in &events {
                    // Exactly the work EventTx::send performs.
                    tee.record_query_event(event);
                    let _ = tx.send(Ok::<_, std::convert::Infallible>(event.clone()));
                }
                drop(tx);
                while rx.try_recv().is_ok() {}
            }
            tee.close();
        })
    });
    logged.finish();
}

criterion_group!(benches, bench_turn_log_overhead);
criterion_main!(benches);
