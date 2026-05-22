#!/usr/bin/env bash
# Shannon Code regression testing script
# Usage: ./scripts/test-regression.sh [--update-snapshots] [--skip-perf] [--verbose]
#
# Runs:
# 1. Snapshot tests with insta (cargo test + insta review)
# 2. Fixture validation tests
# 3. Performance regression checks
# 4. Outputs summary report
#
# Options:
#   --update-snapshots    Update insta snapshots instead of comparing
#   --skip-perf          Skip performance regression tests
#   --verbose            Verbose output

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# Color output helpers
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# Defaults
UPDATE_SNAPSHOTS=false
SKIP_PERF=false
VERBOSE=""
FAILURES=0
PHASES_TOTAL=5
PHASES_PASSED=0
PHASES_FAILED=0
TIMING_LOG="/tmp/shannon-regression-timing.log"

# Parse args
for arg in "$@"; do
    case "$arg" in
        --update-snapshots|-u) UPDATE_SNAPSHOTS=true ;;
        --skip-perf)           SKIP_PERF=true ;;
        --verbose|-v)          VERBOSE="--nocapture" ;;
        --help|-h)
            echo "Usage: $0 [--update-snapshots] [--skip-perf] [--verbose]"
            echo ""
            echo "  --update-snapshots    Update insta snapshots instead of comparing"
            echo "  --skip-perf           Skip performance regression tests"
            echo "  --verbose             Show test output in real time"
            exit 0
            ;;
        *) echo "Unknown argument: $arg"; exit 1 ;;
    esac
done

# Timing helpers
phase_start() {
    PHASE_START_TIME=$(date +%s)
}

phase_end() {
    local phase_end_time
    phase_end_time=$(date +%s)
    PHASE_ELAPSED=$((phase_end_time - PHASE_START_TIME))
}

# Initialize timing log
echo "=== Shannon Code Regression Test Suite ===" > "$TIMING_LOG"
echo "Started: $(date '+%Y-%m-%d %H:%M:%S')" >> "$TIMING_LOG"
echo "Branch: $(git branch --show-current)" >> "$TIMING_LOG"
echo "Commit: $(git rev-parse --short HEAD)" >> "$TIMING_LOG"
echo "" >> "$TIMING_LOG"

OVERALL_START=$(date +%s)

# ── Phase 1: Build check ──────────────────────────────────────────────
echo -e "${YELLOW}[1/${PHASES_TOTAL}] Build check...${NC}"
phase_start
if cargo check --workspace 2>&1 | tail -5; then
    phase_end
    echo -e "  ${GREEN}PASS${NC} (${PHASE_ELAPSED}s)"
    PHASES_PASSED=$((PHASES_PASSED + 1))
    echo "Phase 1 (build check): ${PHASE_ELAPSED}s - PASS" >> "$TIMING_LOG"
else
    phase_end
    echo -e "  ${RED}FAIL${NC} (${PHASE_ELAPSED}s)"
    PHASES_FAILED=$((PHASES_FAILED + 1))
    FAILURES=$((FAILURES + 1))
    echo "Phase 1 (build check): ${PHASE_ELAPSED}s - FAIL" >> "$TIMING_LOG"
    echo -e "${RED}Build failed. Aborting.${NC}"
    exit 1
fi

# ── Phase 2: Snapshot tests ───────────────────────────────────────────
echo -e "${YELLOW}[2/${PHASES_TOTAL}] Snapshot regression...${NC}"
phase_start
if [ "$UPDATE_SNAPSHOTS" = true ]; then
    INSTA_UPDATE=always cargo test --workspace --lib snapshot -- --test-threads=1 $VERBOSE 2>&1 | tail -3
    if command -v cargo-insta &>/dev/null; then
        cargo insta review 2>&1 | tail -3 || true
    fi
    SNAPSHOT_RC=0
else
    cargo test --workspace --lib snapshot -- --test-threads=1 $VERBOSE 2>&1 | tail -3
    SNAPSHOT_RC=${PIPESTATUS[0]}
fi
phase_end

if [ "${SNAPSHOT_RC:-0}" -eq 0 ]; then
    echo -e "  ${GREEN}PASS${NC} (${PHASE_ELAPSED}s)"
    PHASES_PASSED=$((PHASES_PASSED + 1))
    echo "Phase 2 (snapshots): ${PHASE_ELAPSED}s - PASS" >> "$TIMING_LOG"
else
    echo -e "  ${RED}FAIL${NC} (${PHASE_ELAPSED}s)"
    PHASES_FAILED=$((PHASES_FAILED + 1))
    FAILURES=$((FAILURES + 1))
    echo "Phase 2 (snapshots): ${PHASE_ELAPSED}s - FAIL" >> "$TIMING_LOG"
fi

# ── Phase 3: Fixture validation ───────────────────────────────────────
echo -e "${YELLOW}[3/${PHASES_TOTAL}] Fixture validation...${NC}"
phase_start
FIXTURE_RC=0
cargo test --workspace --test fixture_validation -- --test-threads=1 $VERBOSE 2>&1 | tail -3 || FIXTURE_RC=$?

# Also validate fixture JSONL files parse correctly
if [ -d "crates/shannon-core/fixtures/sessions" ]; then
    for f in crates/shannon-core/fixtures/sessions/*.jsonl; do
        if [ -f "$f" ]; then
            if ! python3 -c "
import json, sys
for i, line in enumerate(open('$f'), 1):
    json.loads(line)
" 2>/dev/null; then
                echo -e "  ${RED}Invalid JSONL:${NC} $f"
                FIXTURE_RC=1
            fi
        fi
    done
fi
phase_end

if [ "$FIXTURE_RC" -eq 0 ]; then
    echo -e "  ${GREEN}PASS${NC} (${PHASE_ELAPSED}s)"
    PHASES_PASSED=$((PHASES_PASSED + 1))
    echo "Phase 3 (fixtures): ${PHASE_ELAPSED}s - PASS" >> "$TIMING_LOG"
else
    echo -e "  ${RED}FAIL${NC} (${PHASE_ELAPSED}s)"
    PHASES_FAILED=$((PHASES_FAILED + 1))
    FAILURES=$((FAILURES + 1))
    echo "Phase 3 (fixtures): ${PHASE_ELAPSED}s - FAIL" >> "$TIMING_LOG"
fi

# ── Phase 4: Performance regression ───────────────────────────────────
if [ "$SKIP_PERF" != true ]; then
    echo -e "${YELLOW}[4/${PHASES_TOTAL}] Performance regression...${NC}"
    phase_start
    PERF_RC=0
    cargo test --workspace -- --test-threads=1 --ignored $VERBOSE 2>&1 | tail -3 || PERF_RC=$?
    phase_end

    if [ "$PERF_RC" -eq 0 ]; then
        echo -e "  ${GREEN}PASS${NC} (${PHASE_ELAPSED}s)"
        PHASES_PASSED=$((PHASES_PASSED + 1))
        echo "Phase 4 (perf): ${PHASE_ELAPSED}s - PASS" >> "$TIMING_LOG"
    else
        echo -e "  ${RED}FAIL${NC} (${PHASE_ELAPSED}s)"
        PHASES_FAILED=$((PHASES_FAILED + 1))
        FAILURES=$((FAILURES + 1))
        echo "Phase 4 (perf): ${PHASE_ELAPSED}s - FAIL" >> "$TIMING_LOG"
    fi
else
    echo -e "${YELLOW}[4/${PHASES_TOTAL}] Performance regression (skipped)${NC}"
    echo "Phase 4 (perf): skipped" >> "$TIMING_LOG"
fi

# ── Phase 5: Summary ──────────────────────────────────────────────────
echo ""
OVERALL_END=$(date +%s)
OVERALL_ELAPSED=$((OVERALL_END - OVERALL_START))

echo -e "${CYAN}=== Regression Test Summary ===${NC}"
echo "  Phases passed:  ${PHASES_PASSED}/${PHASES_TOTAL}"
echo "  Phases failed:  ${PHASES_FAILED}/${PHASES_TOTAL}"
echo "  Total time:     ${OVERALL_ELAPSED}s"
echo "  Branch:         $(git branch --show-current)"
echo "  Commit:         $(git rev-parse --short HEAD)"
echo ""

# Write summary to timing log
echo "" >> "$TIMING_LOG"
echo "Summary: ${PHASES_PASSED}/${PHASES_TOTAL} passed, ${PHASES_FAILED} failed, ${OVERALL_ELAPSED}s total" >> "$TIMING_LOG"

if [ "$FAILURES" -gt 0 ]; then
    echo -e "${RED}REGRESSION DETECTED: ${FAILURES} phase(s) failed.${NC}"
    echo "See timing log: $TIMING_LOG"
    exit 1
else
    echo -e "${GREEN}All regression tests passed!${NC}"
    exit 0
fi
