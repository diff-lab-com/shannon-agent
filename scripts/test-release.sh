#!/usr/bin/env bash
# Shannon Code release testing script
# Usage: ./scripts/test-release.sh [--skip-long] [--verbose] [--report <path>]
#
# Comprehensive pre-release testing:
# 1. cargo fmt --check
# 2. cargo clippy --workspace
# 3. cargo test --workspace -- --test-threads=1
# 4. Snapshot regression
# 5. Performance benchmarks
# 6. Multi-turn scenario smoke tests
# 7. Long-running tests (optional, use --skip-long to skip)
# 8. Generate test report
#
# Exit code: 0 if all pass, 1 if any fail

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# Color output helpers
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# Defaults
SKIP_LONG=false
VERBOSE=""
REPORT_FILE="target/test-report.txt"
FAILURES=0
TOTAL_PHASES=7
PHASES_PASSED=0
PHASES_FAILED=0
PHASE_RESULTS=()

# Parse args
while [[ $# -gt 0 ]]; do
    case "$1" in
        --skip-long)    SKIP_LONG=true; shift ;;
        --verbose|-v)   VERBOSE="--nocapture"; shift ;;
        --report|-r)    shift; REPORT_FILE="$1"; shift ;;
        --help|-h)
            echo "Usage: $0 [--skip-long] [--verbose] [--report <path>]"
            echo ""
            echo "  --skip-long    Skip long-running tests"
            echo "  --verbose      Show test output in real time"
            echo "  --report       Path for test report (default: target/test-report.txt)"
            exit 0
            ;;
        *) echo "Unknown argument: $1"; exit 1 ;;
    esac
done

# Adjust total phases based on flags
if $SKIP_LONG; then
    TOTAL_PHASES=6
fi

# Timing helpers
PHASE_START_TIME=0
PHASE_ELAPSED=0

phase_start() {
    PHASE_START_TIME=$(date +%s)
}

phase_end() {
    local phase_end_time
    phase_end_time=$(date +%s)
    PHASE_ELAPSED=$((phase_end_time - PHASE_START_TIME))
}

record_phase() {
    local name="$1"
    local status="$2"
    PHASE_RESULTS+=("${name}: ${status} (${PHASE_ELAPSED}s)")
    echo "Phase: ${name} - ${status} (${PHASE_ELAPSED}s)" >> "$REPORT_FILE.tmp"
}

# Ensure report directory exists
mkdir -p "$(dirname "$REPORT_FILE")"

# Initialize report
cat > "${REPORT_FILE}.tmp" <<EOF
Shannon Code Release Test Report
=================================
Date:    $(date '+%Y-%m-%d %H:%M:%S')
Branch:  $(git branch --show-current)
Commit:  $(git rev-parse --short HEAD)
Rust:    $(rustc --version 2>/dev/null || echo "unknown")
OS:      $(uname -s) $(uname -r)
Cores:   $(nproc)

EOF

echo -e "${BOLD}=== Shannon Code Release Test Suite ===${NC}"
echo -e "  Branch:  $(git branch --show-current)"
echo -e "  Commit:  $(git rev-parse --short HEAD)"
echo -e "  Phases:  ${TOTAL_PHASES}"
echo ""

OVERALL_START=$(date +%s)

# ── Phase 1: Format check ─────────────────────────────────────────────
echo -e "${YELLOW}[1/${TOTAL_PHASES}] Format check (cargo fmt)...${NC}"
phase_start
FMT_RC=0
cargo fmt --all -- --check 2>&1 | tail -3 || FMT_RC=$?
phase_end

if [ "$FMT_RC" -eq 0 ]; then
    echo -e "  ${GREEN}PASS${NC} (${PHASE_ELAPSED}s)"
    PHASES_PASSED=$((PHASES_PASSED + 1))
    record_phase "Format check" "PASS"
else
    echo -e "  ${RED}FAIL${NC} (${PHASE_ELAPSED}s)"
    PHASES_FAILED=$((PHASES_FAILED + 1))
    FAILURES=$((FAILURES + 1))
    record_phase "Format check" "FAIL"
fi

# ── Phase 2: Clippy ───────────────────────────────────────────────────
echo -e "${YELLOW}[2/${TOTAL_PHASES}] Clippy lint...${NC}"
phase_start
CLIPPY_RC=0
cargo clippy --workspace -- -D warnings 2>&1 | tail -5 || CLIPPY_RC=$?
phase_end

if [ "$CLIPPY_RC" -eq 0 ]; then
    echo -e "  ${GREEN}PASS${NC} (${PHASE_ELAPSED}s)"
    PHASES_PASSED=$((PHASES_PASSED + 1))
    record_phase "Clippy" "PASS"
else
    echo -e "  ${RED}FAIL${NC} (${PHASE_ELAPSED}s)"
    PHASES_FAILED=$((PHASES_FAILED + 1))
    FAILURES=$((FAILURES + 1))
    record_phase "Clippy" "FAIL"
fi

# ── Phase 3: Workspace tests ──────────────────────────────────────────
echo -e "${YELLOW}[3/${TOTAL_PHASES}] Workspace tests...${NC}"
phase_start
TEST_RC=0
cargo test --workspace -- --test-threads=1 $VERBOSE 2>&1 | tee /tmp/shannon-release-tests.log | tail -5 || TEST_RC=$?
phase_end

# Parse test results
TESTS_PASSED=$(grep -c "test .* ok$" /tmp/shannon-release-tests.log 2>/dev/null || echo "0")
TESTS_FAILED=$(grep -c "test .* FAILED$" /tmp/shannon-release-tests.log 2>/dev/null || echo "0")
TESTS_TOTAL=$((TESTS_PASSED + TESTS_FAILED))

if [ "$TEST_RC" -eq 0 ]; then
    echo -e "  ${GREEN}PASS${NC} ${TESTS_PASSED} tests (${PHASE_ELAPSED}s)"
    PHASES_PASSED=$((PHASES_PASSED + 1))
    record_phase "Workspace tests (${TESTS_PASSED} passed)" "PASS"
else
    echo -e "  ${RED}FAIL${NC} ${TESTS_FAILED}/${TESTS_TOTAL} tests failed (${PHASE_ELAPSED}s)"
    PHASES_FAILED=$((PHASES_FAILED + 1))
    FAILURES=$((FAILURES + 1))
    record_phase "Workspace tests (${TESTS_FAILED}/${TESTS_TOTAL} failed)" "FAIL"
fi

# ── Phase 4: Snapshot regression ──────────────────────────────────────
echo -e "${YELLOW}[4/${TOTAL_PHASES}] Snapshot regression...${NC}"
phase_start
SNAP_RC=0
cargo test --workspace --lib snapshot -- --test-threads=1 $VERBOSE 2>&1 | tail -3 || SNAP_RC=$?
phase_end

if [ "$SNAP_RC" -eq 0 ]; then
    echo -e "  ${GREEN}PASS${NC} (${PHASE_ELAPSED}s)"
    PHASES_PASSED=$((PHASES_PASSED + 1))
    record_phase "Snapshot regression" "PASS"
else
    echo -e "  ${RED}FAIL${NC} (${PHASE_ELAPSED}s)"
    PHASES_FAILED=$((PHASES_FAILED + 1))
    FAILURES=$((FAILURES + 1))
    record_phase "Snapshot regression" "FAIL"
fi

# ── Phase 5: Performance benchmarks ───────────────────────────────────
echo -e "${YELLOW}[5/${TOTAL_PHASES}] Performance benchmarks...${NC}"
phase_start
BENCH_RC=0
# Run only if bench targets exist
if cargo bench --workspace 2>&1 | tail -5; then
    BENCH_RC=0
else
    BENCH_RC=${PIPESTATUS[0]}
fi
phase_end

if [ "$BENCH_RC" -eq 0 ]; then
    echo -e "  ${GREEN}PASS${NC} (${PHASE_ELAPSED}s)"
    PHASES_PASSED=$((PHASES_PASSED + 1))
    record_phase "Performance benchmarks" "PASS"
else
    echo -e "  ${YELLOW}WARN${NC} Benchmarks failed or not configured (${PHASE_ELAPSED}s)"
    # Benchmarks failing is a warning, not a hard failure for release
    record_phase "Performance benchmarks" "WARN"
fi

# ── Phase 6: Smoke tests ──────────────────────────────────────────────
echo -e "${YELLOW}[6/${TOTAL_PHASES}] Smoke tests (multi-turn scenarios)...${NC}"
phase_start
SMOKE_RC=0
cargo test --workspace -- --test-threads=1 smoke_ $VERBOSE 2>&1 | tail -3 || SMOKE_RC=$?
phase_end

if [ "$SMOKE_RC" -eq 0 ]; then
    echo -e "  ${GREEN}PASS${NC} (${PHASE_ELAPSED}s)"
    PHASES_PASSED=$((PHASES_PASSED + 1))
    record_phase "Smoke tests" "PASS"
else
    echo -e "  ${RED}FAIL${NC} (${PHASE_ELAPSED}s)"
    PHASES_FAILED=$((PHASES_FAILED + 1))
    FAILURES=$((FAILURES + 1))
    record_phase "Smoke tests" "FAIL"
fi

# ── Phase 7: Long-running tests ───────────────────────────────────────
if ! $SKIP_LONG; then
    echo -e "${YELLOW}[7/${TOTAL_PHASES}] Long-running tests...${NC}"
    phase_start
    LONG_RC=0
    cargo test --workspace -- --test-threads=1 test_long_conversation test_multiturn $VERBOSE 2>&1 | tail -3 || LONG_RC=$?
    phase_end

    if [ "$LONG_RC" -eq 0 ]; then
        echo -e "  ${GREEN}PASS${NC} (${PHASE_ELAPSED}s)"
        PHASES_PASSED=$((PHASES_PASSED + 1))
        record_phase "Long-running tests" "PASS"
    else
        echo -e "  ${RED}FAIL${NC} (${PHASE_ELAPSED}s)"
        PHASES_FAILED=$((PHASES_FAILED + 1))
        FAILURES=$((FAILURES + 1))
        record_phase "Long-running tests" "FAIL"
    fi
fi

# ── Generate final report ─────────────────────────────────────────────
OVERALL_END=$(date +%s)
OVERALL_ELAPSED=$((OVERALL_END - OVERALL_START))

cat >> "${REPORT_FILE}.tmp" <<EOF

=================================
Results Summary
=================================
Phases passed:  ${PHASES_PASSED}/${TOTAL_PHASES}
Phases failed:  ${PHASES_FAILED}/${TOTAL_PHASES}
Tests run:      ${TESTS_TOTAL}
Tests passed:   ${TESTS_PASSED}
Tests failed:   ${TESTS_FAILED}
Total time:     ${OVERALL_ELAPSED}s
EOF

# Append per-phase details
echo "" >> "${REPORT_FILE}.tmp"
echo "Phase Details" >> "${REPORT_FILE}.tmp"
echo "-------------" >> "${REPORT_FILE}.tmp"
for result in "${PHASE_RESULTS[@]}"; do
    echo "  ${result}" >> "${REPORT_FILE}.tmp"
done

# Move temp report to final location
mv "${REPORT_FILE}.tmp" "$REPORT_FILE"

echo ""
echo -e "${CYAN}=== Release Test Summary ===${NC}"
echo -e "  Phases passed:  ${PHASES_PASSED}/${TOTAL_PHASES}"
echo -e "  Phases failed:  ${PHASES_FAILED}/${TOTAL_PHASES}"
echo -e "  Tests passed:   ${TESTS_PASSED}"
echo -e "  Tests failed:   ${TESTS_FAILED}"
echo -e "  Total time:     ${OVERALL_ELAPSED}s"
echo -e "  Report:         ${REPORT_FILE}"
echo ""

if [ "$FAILURES" -gt 0 ]; then
    echo -e "${RED}RELEASE BLOCKED: ${FAILURES} phase(s) failed.${NC}"
    echo "See report: ${REPORT_FILE}"
    exit 1
else
    echo -e "${GREEN}All release tests passed! Ready for release.${NC}"
    exit 0
fi
