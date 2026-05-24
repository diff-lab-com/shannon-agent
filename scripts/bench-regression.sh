#!/usr/bin/env bash
# Bench regression detection using criterion baselines.
#
# Usage:
#   ./scripts/bench-regression.sh save     # Save current results as baseline
#   ./scripts/bench-regression.sh compare  # Compare against saved baseline
#   ./scripts/bench-regression.sh run      # Run benchmarks (no comparison)
#
# Thresholds are configured in each bench file via criterion_config():
#   noise_threshold: 0.03 (3%)
#   confidence_level: 0.98 (98%)
#   significance_level: 0.02 (2%)
#   sample_size: 50

set -euo pipefail

BASELINE_NAME="shannon-main"
# Specific bench targets to avoid lib test harness conflicts
BENCH_TARGETS="--bench compact_bench --bench context_budget_bench --bench core_benchmarks --bench recording_benchmarks --bench edit_bench --bench tool_benchmarks --bench repomap_bench --bench protocol_benchmarks --bench skills_benchmarks"

case "${1:-run}" in
    save)
        echo "Saving benchmark baseline as '$BASELINE_NAME'..."
        cargo bench $BENCH_TARGETS -- --save-baseline "$BASELINE_NAME"
        echo "Baseline saved. Run './scripts/bench-regression.sh compare' to check regressions."
        ;;
    compare)
        echo "Comparing benchmarks against baseline '$BASELINE_NAME'..."
        cargo bench $BENCH_TARGETS -- --baseline "$BASELINE_NAME" 2>&1 | tee bench-results.txt
        if grep -q "Performance has regressed" bench-results.txt 2>/dev/null; then
            echo ""
            echo "WARNING: Performance regressions detected!"
            echo "Review bench-results.txt for details."
            rm -f bench-results.txt
            exit 1
        fi
        echo "No regressions detected."
        rm -f bench-results.txt
        ;;
    run)
        echo "Running benchmarks..."
        cargo bench $BENCH_TARGETS
        ;;
    *)
        echo "Usage: $0 {save|compare|run}"
        exit 1
        ;;
esac
