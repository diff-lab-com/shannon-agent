#!/usr/bin/env bash
# Run performance tests only (marked with #[ignore])
# Usage:
#   ./scripts/test-perf.sh                # Run all performance tests
#   ./scripts/test-perf.sh -p <crate>     # Single crate
#   ./scripts/test-perf.sh --verbose      # Show test output
#   ./scripts/test-perf.sh --clean        # Clean build artifacts first
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

EXTRA_ARGS=()
DO_CLEAN=false
VERBOSE=""

for arg in "$@"; do
    case "$arg" in
        --verbose|-v)   VERBOSE="--nocapture" ;;
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

echo "Running performance tests (#[ignore] tests only)..."
echo "These tests exercise large-scale scenarios and may take several minutes."
echo ""

START_TIME=$(date +%s)

cargo test --workspace -- --test-threads=1 --ignored $VERBOSE "${EXTRA_ARGS[@]}"

END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))
echo ""
echo "Performance tests completed in ${ELAPSED}s."
