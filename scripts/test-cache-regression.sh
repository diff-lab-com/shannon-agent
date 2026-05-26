#!/usr/bin/env bash
# Cache hit rate regression gate
#
# Runs cache-specific tests across all tiers:
#   - Unit: mock_dsl cache token rendering
#   - Integration: engine E2E cache token pipeline
#   - Stress: concurrent cache token streaming
#   - Perf: cache hit rate calculation time bounds
#
# Exit 1 if any test fails. Intended for CI pipelines.

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

PASS=0
FAIL=0

run_test() {
    local name="$1"
    shift
    echo -e "${YELLOW}[CACHE GATE]${NC} Running: $name"
    if "$@" 2>&1; then
        echo -e "${GREEN}[CACHE GATE]${NC} PASSED: $name"
        ((PASS++))
    else
        echo -e "${RED}[CACHE GATE]${NC} FAILED: $name"
        ((FAIL++))
    fi
}

echo "========================================"
echo " Cache Hit Rate Regression Gate"
echo "========================================"
echo ""

# Tier 1: Unit tests (mock_dsl cache token rendering)
run_test "Unit: mock_dsl cache tests" \
    cargo test -p shannon-core --lib -- mock_dsl --test-threads=1

# Tier 2: Integration tests (engine E2E cache pipeline)
run_test "Integration: cache token pipeline" \
    cargo test -p shannon-core --test api_integration -- cache_token --test-threads=1

# Tier 3: Stress tests (concurrent cache streaming)
run_test "Stress: cache token streaming" \
    cargo test -p shannon-core --test streaming_stress -- cache_token --test-threads=1

# Tier 4: Performance regression (cache hit rate time bounds)
run_test "Perf: cache hit rate regression" \
    cargo test -p shannon-core --test perf_tests -- cache_hit_rate --test-threads=1

run_test "Perf: cache accumulation regression" \
    cargo test -p shannon-core --test perf_tests -- cache_accumulation --test-threads=1

run_test "Perf: cache edge cases" \
    cargo test -p shannon-core --test perf_tests -- cache_hit_rate_edge --test-threads=1

# Tier 5: UI snapshot tests (cache display)
run_test "UI: cache hit rate snapshot" \
    cargo test -p shannon-ui --test ui_snapshot_tests -- cache --test-threads=1

echo ""
echo "========================================"
echo " Results: $PASS passed, $FAIL failed"
echo "========================================"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
