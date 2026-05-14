#!/usr/bin/env bash
# Run performance tests only (marked with #[ignore])
# Usage:
#   ./scripts/test-perf.sh                # Run performance tests (rule-based)
#   ./scripts/test-perf.sh --ollama       # Also run e2e tests with local ollama
#   ./scripts/test-perf.sh -p <crate>     # Single crate
#   ./scripts/test-perf.sh --verbose      # Show test output
#   ./scripts/test-perf.sh --clean        # Clean build artifacts first
#   ./scripts/test-perf.sh --report       # Generate performance report
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

EXTRA_ARGS=()
DO_CLEAN=false
VERBOSE=""
USE_OLLAMA=false
GENERATE_REPORT=false

for arg in "$@"; do
    case "$arg" in
        --verbose|-v)   VERBOSE="--nocapture" ;;
        --clean|-c)     DO_CLEAN=true ;;
        --ollama)       USE_OLLAMA=true ;;
        --report|-r)    GENERATE_REPORT=true ;;
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

echo "========================================"
echo " Shannon Code — Performance Test Suite"
echo "========================================"
echo ""

# Phase 1: Rule-based performance tests (no LLM needed)
echo "Phase 1: Core performance tests (rule-based summarizer)..."
echo "These tests exercise large-scale conversation scenarios."
echo ""

START_TIME=$(date +%s)

cargo test --workspace -- --test-threads=1 --ignored $VERBOSE "${EXTRA_ARGS[@]}" 2>&1 | tee /tmp/shannon-perf-phase1.log

END_TIME=$(date +%s)
PHASE1_ELAPSED=$((END_TIME - START_TIME))
echo ""
echo "Phase 1 completed in ${PHASE1_ELAPSED}s."

# Phase 2: E2E tests with local ollama (optional)
if $USE_OLLAMA; then
    echo ""
    echo "Phase 2: E2E tests with local ollama..."

    # Check ollama is running
    if ! ollama list &>/dev/null; then
        echo "ERROR: ollama is not running or not installed."
        echo "Start it with: ollama serve"
        exit 1
    fi

    # Find available model
    OLLAMA_MODEL=$(ollama list 2>/dev/null | head -2 | tail -1 | awk '{print $1}')
    if [[ -z "$OLLAMA_MODEL" ]]; then
        echo "ERROR: No ollama models found. Pull one with: ollama pull qwen3:4b"
        exit 1
    fi

    echo "Using ollama model: $OLLAMA_MODEL"
    echo ""

    START_TIME2=$(date +%s)

    OLLAMA_E2E=1 OLLAMA_MODEL="$OLLAMA_MODEL" \
        cargo test --workspace -- --test-threads=1 e2e_ollama $VERBOSE "${EXTRA_ARGS[@]}" 2>&1 | tee /tmp/shannon-perf-phase2.log

    END_TIME2=$(date +%s)
    PHASE2_ELAPSED=$((END_TIME2 - START_TIME2))
    echo ""
    echo "Phase 2 completed in ${PHASE2_ELAPSED}s."
fi

TOTAL_ELAPSED=$((END_TIME - START_TIME + (${USE_OLLAMA:-false} && $PHASE2_ELAPSED || 0)))

echo ""
echo "========================================"
echo " Performance Test Summary"
echo "========================================"

# Parse results from phase 1
PASSED=$(grep -c "test .* ok$" /tmp/shannon-perf-phase1.log 2>/dev/null || echo "0")
FAILED=$(grep -c "test .* FAILED$" /tmp/shannon-perf-phase1.log 2>/dev/null || echo "0")
echo "  Tests passed:  $PASSED"
echo "  Tests failed:  $FAILED"
echo "  Phase 1 time:  ${PHASE1_ELAPSED}s"

if $USE_OLLAMA; then
    echo "  Phase 2 time:  ${PHASE2_ELAPSED}s"
    echo "  Ollama model:  $OLLAMA_MODEL"
fi

echo "  Total time:    ${TOTAL_ELAPSED}s"
echo "========================================"

# Generate report if requested
if $GENERATE_REPORT; then
    REPORT_FILE="scripts/perf-report-$(date +%Y%m%d-%H%M%S).md"
    echo "Generating report: $REPORT_FILE"
    cat > "$REPORT_FILE" <<REPORT_EOF
# Shannon Code Performance Report

**Date:** $(date '+%Y-%m-%d %H:%M:%S')
**Branch:** $(git branch --show-current)
**Commit:** $(git rev-parse --short HEAD)

## Environment

- **OS:** $(uname -s) $(uname -r)
- **CPU:** $(nproc) cores
- **Rust:** $(rustc --version 2>/dev/null || echo "unknown")
REPORT_EOF

    if $USE_OLLAMA; then
        echo "- **Ollama Model:** $OLLAMA_MODEL" >> "$REPORT_FILE"
        echo "- **Ollama Version:** $(ollama --version 2>/dev/null || echo "unknown")" >> "$REPORT_FILE"
    fi

    cat >> "$REPORT_FILE" <<REPORT_EOF

## Results

| Metric | Value |
|--------|-------|
| Tests passed | $PASSED |
| Tests failed | $FAILED |
| Phase 1 time (rule-based) | ${PHASE1_ELAPSED}s |
REPORT_EOF

    if $USE_OLLAMA; then
        echo "| Phase 2 time (ollama e2e) | ${PHASE2_ELAPSED}s |" >> "$REPORT_FILE"
        echo "| Ollama model | $OLLAMA_MODEL |" >> "$REPORT_FILE"
    fi

    cat >> "$REPORT_FILE" <<REPORT_EOF

## Detailed Logs

### Phase 1 (Rule-based performance tests)
REPORT_EOF

    grep -E "test .* ok$|test .* FAILED$|test result:" /tmp/shannon-perf-phase1.log >> "$REPORT_FILE" 2>/dev/null || true

    if $USE_OLLAMA && [[ -f /tmp/shannon-perf-phase2.log ]]; then
        echo "" >> "$REPORT_FILE"
        echo "### Phase 2 (Ollama E2E tests)" >> "$REPORT_FILE"
        grep -E "test .* ok$|test .* FAILED$|test result:|perf_|e2e_" /tmp/shannon-perf-phase2.log >> "$REPORT_FILE" 2>/dev/null || true
    fi

    echo ""
    echo "Report saved to: $REPORT_FILE"
fi

if [[ "$FAILED" -gt 0 ]]; then
    exit 1
fi
