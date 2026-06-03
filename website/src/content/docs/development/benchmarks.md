---
title: Benchmarks
order: 42
section: development
---

# Benchmarks

Performance benchmarks use `criterion` and are located in `benches/` directories.

## Running Benchmarks

```bash
# All benchmarks
cargo bench --workspace

# Specific crate
cargo bench -p shannon-core

# Specific benchmark
cargo bench -p shannon-core -- compact
```

## Available Benchmarks

| Crate | Benchmark | What it measures |
|-------|-----------|-----------------|
| `shannon-core` | `compact_bench` | Message compaction (100/500/1000 messages) |
| `shannon-core` | `context_budget_bench` | Context budget calculation |
| `shannon-tools` | `edit_bench` | File edit operations (small/medium/large) |
| `shannon-codegen` | `repomap_bench` | Repository map generation |

## Adding Benchmarks

1. Add to crate's `Cargo.toml`:

```toml
[dev-dependencies]
criterion = "0.5"

[[bench]]
name = "my_bench"
harness = false
```

2. Create `benches/my_bench.rs`:

```rust
use criterion::{criterion_group, criterion_main, Criterion};

fn benchmark(c: &mut Criterion) {
    c.bench_function("my_operation", |b| {
        b.iter(|| { /* code to benchmark */ })
    });
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
```
