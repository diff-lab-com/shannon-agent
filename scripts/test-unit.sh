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
    exec cargo nextest run --workspace $FAIL_FAST "${EXTRA_ARGS[@]}"
else
    echo "cargo-nextest not found, falling back to cargo test..."
    # Default cargo test skips #[ignore] tests
    exec cargo test --workspace -- --test-threads=1 "${EXTRA_ARGS[@]}"
fi
