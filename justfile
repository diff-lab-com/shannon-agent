# Shannon test commands
# Install: cargo install just
# Usage: just <target>

# Default: run all mock tests (no API key needed)
test:
    cargo test --workspace -- --test-threads=1

# Fast type check
check:
    cargo check --workspace

# Lint
lint:
    cargo clippy --workspace

# YAML scenario tests only
scenarios:
    cargo test --test scenario_tests -- --test-threads=1

# Performance regression tests
perf:
    cargo test --test perf_tests -- --test-threads=1

# Record real API fixtures (needs SHANNON_API_KEY)
record:
    #!/usr/bin/bash
    if [ -z "$SHANNON_API_KEY" ]; then echo "Set SHANNON_API_KEY first"; exit 1; fi
    SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
    cargo test --test live_tests -- --ignored --test-threads=1

# Replay recorded fixtures (no API key needed)
replay:
    cargo test --test live_tests -- --test-threads=1

# Criterion benchmarks
bench:
    cargo bench

# Run everything that doesn't need a key (CI without secrets)
ci:
    cargo test --workspace -- --test-threads=1
    cargo test --test scenario_tests -- --test-threads=1
    cargo test --test perf_tests -- --test-threads=1

# Build release binary
build:
    cargo build --release -p shannon-cli
