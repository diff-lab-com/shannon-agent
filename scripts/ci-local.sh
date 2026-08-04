#!/usr/bin/env bash
# scripts/ci-local.sh — local reproduction of the .github/workflows/ci.yml
# gate stack. Mirrors the same checks in the same order CI runs them, so a
# contributor can see ahead of push whether their branch is going to go red.
#
# Usage:
#   scripts/ci-local.sh         # default: fmt + clippy + nextest + deny
#   scripts/ci-local.sh --doc   # also run the doc gate
#   scripts/ci-local.sh --audit # also run rustsec-audit (requires cargo-audit)
#
# Exit code: 0 iff every gate is green; 1 if any gate fails. Echoes a
# green/red summary identical to the GitHub Actions UI table.

set -u

WITH_DOC=0
WITH_AUDIT=0
for arg in "$@"; do
    case "$arg" in
        --doc)   WITH_DOC=1 ;;
        --audit) WITH_AUDIT=1 ;;
        --help|-h)
            sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "unknown flag: $arg" >&2
            exit 2
            ;;
    esac
done

# Color shortcuts. TTY-detection avoids polluting `tee` output piped to
# CI logs. We default to NO_COLOR (https://no-color.org/) respect.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    GREEN=$'\033[0;32m'
    RED=$'\033[0;31m'
    YELLOW=$'\033[0;33m'
    BOLD=$'\033[1m'
    NC=$'\033[0m'
else
    GREEN=""; RED=""; YELLOW=""; BOLD=""; NC=""
fi

# 1) Format check (matches `fmt` job)
fmt_cmd=(cargo fmt --all -- --check)
# 2) Clippy (matches `clippy` job flags exactly)
clippy_cmd=(cargo clippy --workspace --
    -D warnings
    -A unknown-lints
    -A clippy::collapsible_if
    -A clippy::collapsible_match
    -A clippy::derivable_impls
    -A clippy::manual_is_multiple_of
    -A clippy::manual_checked_div
    -A clippy::unwrap_used
    -A clippy::unnecessary_sort_by
)
# 3) Tests (matches `test`/`insta` jobs)
test_cmd=(cargo nextest run --workspace --config-file .config/nextest.toml)
# 4) Dependency audit (matches `audit` job)
deny_cmd=(cargo deny check --hide-inclusion-graph)
# 5) Doc (matches `doc` job; gated by --doc)
doc_cmd=(cargo doc --workspace --no-deps)
# 6) RustSec audit (matches `rustsec-audit` job; gated by --audit)
audit_cmd=(cargo audit --deny warnings)

run() {
    local name="$1"; shift
    local label
    label="$(printf '%-22s' "$name")"
    echo "${YELLOW}>>> ${label}${NC}"
    if "$@" >/tmp/ci-local.out 2>&1; then
        echo "${GREEN}    PASS${NC}  ${name}"
        pass=$((pass + 1))
    else
        echo "${RED}    FAIL${NC}  ${name}"
        # Show the last 30 lines of the failing output for context.
        tail -30 /tmp/ci-local.out | sed 's/^/      | /'
        fail=$((fail + 1))
    fi
}

pass=0
fail=0

# Format first — cheap and catches 80% of style drift before clippy
# is even invoked.
run "fmt"      "${fmt_cmd[@]}"
run "clippy"   "${clippy_cmd[@]}"
run "test"     "${test_cmd[@]}"
run "deny"     "${deny_cmd[@]}"

if [ "$WITH_DOC" -eq 1 ]; then
    RUSTDOCFLAGS="-D warnings" \
        run "doc" "${doc_cmd[@]}"
fi

if [ "$WITH_AUDIT" -eq 1 ]; then
    # `cargo-audit` is not a transitive dep; gate its invocation on the
    # binary existing so `scripts/ci-local.sh` is still runnable in a
    # fresh clone.
    if command -v cargo-audit >/dev/null 2>&1; then
        run "rustsec-audit" "${audit_cmd[@]}"
    else
        echo "${YELLOW}>>> ${BOLD}rustsec-audit${NC}  (skipped — install with 'cargo install cargo-audit --locked')"
    fi
fi

echo ""
echo "${BOLD}================ ci-local summary ================${NC}"
total=$((pass + fail))
if [ "$fail" -eq 0 ]; then
    echo "${GREEN}all ${total} gates green${NC}"
    exit 0
else
    echo "${RED}${fail} of ${total} gates failed${NC}"
    echo "push blocked — fix the failures above or run: git push --no-verify"
    exit 1
fi
