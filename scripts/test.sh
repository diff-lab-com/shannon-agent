#!/usr/bin/env bash
# Quick test runner for Shannon Code workspace
# Usage:
#   ./scripts/test.sh              # Full workspace, no fail-fast
#   ./scripts/test.sh -p <crate>   # Single crate
#   ./scripts/test.sh --fail-fast  # Stop on first failure
#   ./scripts/test.sh --clean      # Clean build artifacts first
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

# Kill stale nextest/cargo processes from previous runs
STALE_PIDS=$(pgrep -f 'cargo-nextest' 2>/dev/null || true)
if [[ -n "$STALE_PIDS" ]] && [[ "$STALE_PIDS" != "$$" ]]; then
    echo "Killing stale nextest processes: $STALE_PIDS"
    echo "$STALE_PIDS" | xargs kill 2>/dev/null || true
    sleep 1
fi

if $DO_CLEAN; then
    echo "Cleaning build artifacts..."
    cargo clean 2>/dev/null || true
fi

# Ensure test binaries are compiled with latest source before running
echo "Building test binaries..."
cargo build --tests --workspace 2>&1 | tail -1
echo ""

if command -v cargo-nextest &>/dev/null; then
    echo "Running with cargo-nextest..."
    exec cargo nextest run --workspace $FAIL_FAST "${EXTRA_ARGS[@]}"
else
    echo "cargo-nextest not found, falling back to cargo test..."
    exec cargo test --workspace -- --test-threads=1 "${EXTRA_ARGS[@]}"
fi
