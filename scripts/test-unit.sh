#!/usr/bin/env bash
# Run unit and integration tests only (excludes performance/ignored tests)
# Usage:
#   ./scripts/test-unit.sh              # Full workspace unit tests
#   ./scripts/test-unit.sh -p <crate>   # Single crate
#   ./scripts/test-unit.sh --fail-fast  # Stop on first failure
#   ./scripts/test-unit.sh --clean      # Clean build artifacts first
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

FAIL_FAST="--no-fail-fast"
EXTRA_ARGS=()
DO_CLEAN=false

for arg in "$@"; do
    case "$arg" in
        --fail-fast|-f) FAIL_FAST="" ;;
        --clean|-c)     DO_CLEAN=true ;;
        -p|--package)   shift; EXTRA_ARGS+=("-p" "$1") ;;
        *)              EXTRA_ARGS+=("$arg") ;;
    esac
done

if $DO_CLEAN; then
    echo "Cleaning build artifacts..."
    cargo clean 2>/dev/null || true
fi

echo "Building test binaries..."
cargo build --tests --workspace 2>&1 | tail -1
echo ""

if command -v cargo-nextest &>/dev/null; then
    echo "Running unit tests with cargo-nextest (skipping performance tests)..."
    # Honor the header contract: exclude the 12 perf-threshold tests
    # (`just perf` runs them separately). Same names verify via
    # `cargo nextest list` in the justfile perf recipe.
    PERF_FILTER='not (test(cache_accumulation) + test(cache_hit_rate) + test(compaction_100_turns) + test(five_turn) + test(message_serialization) + test(session_load) + test(single_turn) + test(snapshot_render) + test(sse_round_trip) + test(streaming_parse) + test(token_estimation) + test(tool_chain))'
    exec cargo nextest run --workspace $FAIL_FAST -E "$PERF_FILTER" "${EXTRA_ARGS[@]}"
else
    echo "cargo-nextest not found, falling back to cargo test..."
    # Default cargo test skips #[ignore] tests
    exec cargo test --workspace -- --test-threads=1 "${EXTRA_ARGS[@]}"
fi
