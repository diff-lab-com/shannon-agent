#!/usr/bin/env bash
# Quick test runner for Shannon Code workspace
# Usage:
#   ./scripts/test.sh              # Full workspace, no fail-fast
#   ./scripts/test.sh -p <crate>   # Single crate
#   ./scripts/test.sh --fail-fast  # Stop on first failure
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

FAIL_FAST="--no-fail-fast"
EXTRA_ARGS=()

for arg in "$@"; do
    case "$arg" in
        --fail-fast|-f) FAIL_FAST="" ;;
        -p|--package)   shift; EXTRA_ARGS+=("-p" "$1") ;;
        *)              EXTRA_ARGS+=("$arg") ;;
    esac
done

if command -v cargo-nextest &>/dev/null; then
    echo "Running with cargo-nextest..."
    exec cargo nextest run --workspace $FAIL_FAST "${EXTRA_ARGS[@]}"
else
    echo "cargo-nextest not found, falling back to cargo test..."
    exec cargo test --workspace -- --test-threads=1 "${EXTRA_ARGS[@]}"
fi
