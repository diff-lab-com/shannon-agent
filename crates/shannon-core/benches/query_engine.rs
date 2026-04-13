//! Benchmark for query engine operations

use criterion::{criterion_group, criterion_main, Criterion};
use shannon_core::{QueryContext};
use shannon_core::query_engine::QueryMetadata;
use uuid::Uuid;

fn bench_query_context_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_context");

    group.bench_function("creation", |b| {
        b.iter(|| {
            QueryContext {
                query_id: Uuid::new_v4(),
                session_id: Uuid::new_v4(),
                user_message: "Test query".to_string(),
                metadata: QueryMetadata {
                    timestamp: chrono::Utc::now(),
                    tools_allowed: true,
                    max_tokens: Some(4096),
                    model: "claude-sonnet-4-20250514".to_string(),
                    temperature: None,
                    top_p: None,
                },
            }
        })
    });

    group.finish();
}

criterion_group!(benches, bench_query_context_creation);
criterion_main!(benches);
