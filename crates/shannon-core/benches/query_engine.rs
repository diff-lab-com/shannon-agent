//! Benchmark for query engine operations

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use shannon_core::{QueryEngine, QueryContext, QueryMetadata};
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
                    model: "claude-3-5-sonnet-20241022".to_string(),
                },
            }
        })
    });

    group.finish();
}

criterion_group!(benches, bench_query_context_creation);
criterion_main!(benches);
