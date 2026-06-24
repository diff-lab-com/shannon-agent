#!/bin/bash
# Local check script for pre-push verification.
# Usage: scripts/local-check.sh [--quick]
#   --quick: skip clippy (only fmt + build)
# Bypass: git push --no-verify

set -euo pipefail

QUICK=0
if [ "${1:-}" = "--quick" ]; then
    QUICK=1
fi

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

pass=0
fail=0

run_check() {
    local name="$1"
    shift
    echo -e "${YELLOW}>>> ${name}${NC}"
    if "$@"; then
        echo -e "${GREEN}    OK${NC}"
        pass=$((pass + 1))
    else
        echo -e "${RED}    FAILED${NC}"
        fail=$((fail + 1))
    fi
}

echo "Running local checks (bypass with: git push --no-verify)"
echo ""

run_check "cargo fmt --check" cargo fmt --all -- --check
run_check "cargo build --workspace" cargo build --workspace

if [ "$QUICK" -ne 1 ]; then
    run_check "cargo clippy" cargo clippy --workspace -- -D warnings -A unknown-lints -A clippy::collapsible_if -A clippy::collapsible_match -A clippy::derivable_impls -A clippy::manual_is_multiple_of -A clippy::manual_checked_div -A clippy::unwrap_used -A clippy::unnecessary_sort_by
fi

echo ""
echo "Results: ${pass} passed, ${fail} failed"

if [ "$fail" -gt 0 ]; then
    echo -e "${RED}Push blocked. Fix the failures above or use: git push --no-verify${NC}"
    exit 1
fi

exit 0
