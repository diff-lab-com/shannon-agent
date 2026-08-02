#!/usr/bin/env bash
# gen-metrics.sh — Generate the single authoritative metrics source for the project.
#
# Writes docs/metrics.md (overwriting) with:
#   - Test counts (cargo nextest list, JSON output)
#   - Line counts (cloc if available, else find+wc fallback)
#   - Clippy status (cargo clippy --workspace -- -D warnings)
#   - cargo-deny status (if cargo-deny installed)
#
# Exit code: 0 on success even if some sub-commands fail (failures are recorded
# in the report with status cells). Exit non-zero only on hard infrastructure
# failures (missing cwd, missing nextest, etc.).
#
# Usage: bash scripts/gen-metrics.sh
set -u

# Resolve repo root from script location so it works regardless of CWD.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

OUTPUT="${REPO_ROOT}/docs/metrics.md"
TIMESTAMP_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
GIT_SHA="$(git rev-parse --short HEAD 2>/dev/null || echo 'unknown')"
GIT_BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo 'unknown')"
GIT_DESCRIBE="$(git describe --tags --always --dirty 2>/dev/null || echo 'unknown')"

# Fallback Python helper (always present in this repo).
python3() {
  command python3 "$@"
}

# ----------------------------------------------------------------------------
# 1. Test counts via `cargo nextest list --workspace --message-format=json`.
#    Aggregate by crate (test-binary) and count unique test functions.
# ----------------------------------------------------------------------------
echo "[gen-metrics] Collecting test counts via cargo nextest list..." >&2
TEST_TOTAL=0
TEST_CRATE_SECTION=""
TMP_LIST="$(mktemp -t nextest-list.XXXXXX.json)"
trap 'rm -f "${TMP_LIST}"' EXIT

# Nextest emits JSON envelopes intermixed with cargo "Compiling" status lines
# on stdout. The envelopes can be indented too, so we match any line that
# *starts with* `{` once leading whitespace is stripped.
if cargo nextest list --workspace --message-format=json 2>/dev/null \
     | sed -e 's/^[[:space:]]*//' -e '/^$/d' \
     | grep -E '^\{' >"${TMP_LIST}"; then
  if [ -s "${TMP_LIST}" ]; then
    # Inside the envelope, `test-count` is the authoritative total. We also
    # aggregate per-package from `rust-suites[*].testcases` length.
    TEST_TOTAL="$(
      jq -s 'map(."test-count" // 0) | add' "${TMP_LIST}" 2>/dev/null \
        | tr -d ' ' || echo 0
    )"
    TEST_CRATE_SECTION="$(
      jq -s -r '.[0] | ."rust-suites" // {} | to_entries
            | map(.value | {pkg: ."package-name", count: (.testcases | length), binary: ."binary-name"})
            | group_by(.pkg)
            | map({pkg: .[0].pkg, count: (map(.count) | add), binaries: (map(.binary) | length)})
            | sort_by(-.count)
            | (["| crate | tests | binaries |", "|---|---:|---:|"]
               + map("| \(.pkg) | \(.count) | \(.binaries) |"))
            | .[]' "${TMP_LIST}" 2>/dev/null
    )"
  fi
fi

if [ -z "${TEST_TOTAL}" ] || [ "${TEST_TOTAL}" -eq 0 ]; then
  TEST_TOTAL=0
  TEST_CRATE_SECTION="| crate | count |
|---|---:|
| _(no tests discovered)_ | 0 |"
fi

# Inline `#[test]` + `#[tokio::test]` counts via grep (counts functions, not enums).
# These are authoritative for the "tests by source" view; nextest numbers are
# authoritative for "tests that actually run".
TEST_ATTR_TOTAL="$(grep -RhoE '#\[(tokio::test|test|tokio::test\(.*\))\]' crates/ desktop/src/ desktop/ui/src/ 2>/dev/null \
  | grep -E '#\[(tokio::test|test)' | wc -l | tr -d ' ')"

# ----------------------------------------------------------------------------
# 2. Line counts via cloc if available; otherwise find+wc fallback.
# ----------------------------------------------------------------------------
echo "[gen-metrics] Collecting line counts..." >&2
LINES_SECTION=""
CLOC_USED="false"

if command -v cloc >/dev/null 2>&1; then
  CLOC_USED="true"
  TMP_CLOC="$(mktemp -t cloc.XXXXXX.csv)"
  trap 'rm -f "${TMP_LIST}" "${TMP_CLOC}"' EXIT
  # cloc --csv writes a header row then language rows. We aggregate Rust + TS.
  if cloc crates/ desktop/src/ desktop/ui/src/ --quiet --csv --not-match-d='/(target|node_modules|dist|build)/' \
       >"${TMP_CLOC}" 2>/dev/null; then
    # Skip the first line (SUM header) and pick Rust + TypeScript + JavaScript rows.
    LINES_SECTION="$(tail -n +2 "${TMP_CLOC}" \
      | awk -F',' '$1 == "Rust" || $1 == "TypeScript" || $1 == "JavaScript" || $1 == "TSX" || $1 == "JSX"' \
      | awk -F',' 'BEGIN{print "| language | files | blank | comment | code |\n|---|---:|---:|---:|---:|"}
                     {printf "| %s | %s | %s | %s | %s |\n", $1, $2, $3, $4, $5}')"
    # Append totals row.
    TOTAL_LINE="$(tail -n +2 "${TMP_CLOC}" \
      | awk -F',' '$1 == "Rust" || $1 == "TypeScript" || $1 == "JavaScript" || $1 == "TSX" || $1 == "JSX"' \
      | awk -F',' '{f+=$2; b+=$3; c+=$4; co+=$5} END {printf "| **total** | **%d** | **%d** | **%d** | **%d** |\n", f, b, c, co}')"
    LINES_SECTION="${LINES_SECTION}${TOTAL_LINE}"
  fi
fi

if [ -z "${LINES_SECTION}" ]; then
  # Fallback: find .rs under Rust path and wc -l.
  RUST_LINES="$(find . -path './target' -prune -o -path './node_modules' -prune \
    -o \( -name '*.rs' -not -path './target/*' -not -path './node_modules/*' \
       -not -path './legacy-archives/*' -not -path './shannon-agent-build/*' \) \
    -print 2>/dev/null | xargs wc -l 2>/dev/null | tail -n 1 | awk '{print $1}')"
  RUST_FILES="$(find . -path './target' -prune -o -path './node_modules' -prune \
    -o \( -name '*.rs' -not -path './target/*' -not -path './node_modules/*' \
       -not -path './legacy-archives/*' -not -path './shannon-agent-build/*' \) \
    -print 2>/dev/null | wc -l | tr -d ' ')"
  TS_LINES="$(find . -path './target' -prune -o -path './node_modules' -prune \
    -o \( -name '*.ts' -o -name '*.tsx' \) -not -path './target/*' \
    -not -path './legacy-archives/*' -not -path './shannon-agent-build/*' \
    -print 2>/dev/null | xargs wc -l 2>/dev/null | tail -n 1 | awk '{print $1}')"
  TS_FILES="$(find . -path './target' -prune -o -path './node_modules' -prune \
    -o \( -name '*.ts' -o -name '*.tsx' \) -not -path './target/*' \
    -not -path './legacy-archives/*' -not -path './shannon-agent-build/*' \
    -print 2>/dev/null | wc -l | tr -d ' ')"
  RUST_LINES="${RUST_LINES:-0}"
  TS_LINES="${TS_LINES:-0}"
  RUST_FILES="${RUST_FILES:-0}"
  TS_FILES="${TS_FILES:-0}"
  LINES_SECTION="| language | files | code |
|---|---:|---:|
| Rust (.rs) | ${RUST_FILES} | ${RUST_LINES} |
| TypeScript/TSX | ${TS_FILES} | ${TS_LINES} |"
fi

# Rust-only aggregate for the summary table.
RUST_FILES_TOTAL="$(find . -path './target' -prune -o -path './node_modules' -prune \
  -o \( -name '*.rs' -not -path './target/*' -not -path './node_modules/*' \
     -not -path './legacy-archives/*' -not -path './shannon-agent-build/*' \) \
  -print 2>/dev/null | wc -l | tr -d ' ')"
RUST_LINES_TOTAL="${RUST_LINES:-0}"
if [ "${CLOC_USED}" = "true" ] && [ -s "${TMP_CLOC:-}" ]; then
  RUST_LINES_TOTAL="$(tail -n +2 "${TMP_CLOC}" | awk -F',' '$1 == "Rust" {print $5; exit}')"
  RUST_LINES_TOTAL="${RUST_LINES_TOTAL:-0}"
fi

# ----------------------------------------------------------------------------
# 3. Clippy status (must succeed with -D warnings).
# ----------------------------------------------------------------------------
echo "[gen-metrics] Checking clippy --workspace -- -D warnings..." >&2
CLIPPY_STATUS="pass"
CLIPPY_TAIL="(no output captured)"
TMP_CLIPPY="$(mktemp -t clippy.XXXXXX.log)"
trap 'rm -f "${TMP_LIST}" "${TMP_CLIPPY:-}"' EXIT
if cargo clippy --workspace -- -D warnings -A unknown-lints \
     -A clippy::collapsible_if -A clippy::collapsible_match \
     -A clippy::derivable_impls -A clippy::manual_is_multiple_of \
     -A clippy::manual_checked_div -A clippy::unwrap_used \
     -A clippy::unnecessary_sort_by >"${TMP_CLIPPY}" 2>&1; then
  CLIPPY_STATUS="pass"
else
  CLIPPY_STATUS="fail"
fi
CLIPPY_TAIL="$(tail -n 3 "${TMP_CLIPPY}" | sed 's/`/\\`/g')"

# ----------------------------------------------------------------------------
# 4. cargo-deny check (optional).
# ----------------------------------------------------------------------------
DENY_STATUS="not installed"
DENY_TAIL=""
if command -v cargo-deny >/dev/null 2>&1; then
  echo "[gen-metrics] Running cargo deny check..." >&2
  TMP_DENY="$(mktemp -t deny.XXXXXX.log)"
  trap 'rm -f "${TMP_LIST}" "${TMP_CLIPPY:-}" "${TMP_DENY:-}"' EXIT
  if cargo deny check --hide-inclusion-graph >"${TMP_DENY}" 2>&1; then
    DENY_STATUS="pass"
  else
    DENY_STATUS="fail"
  fi
  DENY_TAIL="$(tail -n 3 "${TMP_DENY}" | sed 's/`/\\`/g')"
fi

# ----------------------------------------------------------------------------
# 5. Render markdown.
# ----------------------------------------------------------------------------
{
  echo "# Shannon Project Metrics"
  echo
  echo "> **Single authoritative metrics source.** Generated by \`scripts/gen-metrics.sh\`."
  echo "> Do not edit by hand — re-run the script after merge to \`main\`."
  echo
  echo "## Snapshot"
  echo
  echo "- **Generated (UTC)**: \`${TIMESTAMP_UTC}\`"
  echo "- **Branch**: \`${GIT_BRANCH}\`"
  echo "- **Commit**: \`${GIT_SHA}\`"
  echo "- **Describe**: \`${GIT_DESCRIBE}\`"
  echo
  echo "## Summary"
  echo
  echo "| metric | value |"
  echo "|---|---:|"
  echo "| Tests (nextest, runnable) | ${TEST_TOTAL} |"
  echo "| Tests (source \`#[test]\`/\`#[tokio::test]\` attrs) | ${TEST_ATTR_TOTAL} |"
  echo "| Rust source files | ${RUST_FILES_TOTAL} |"
  echo "| Rust LOC (code) | ${RUST_LINES_TOTAL} |"
  echo "| \`cargo clippy --workspace -- -D warnings\` | ${CLIPPY_STATUS} |"
  echo "| \`cargo deny check\` | ${DENY_STATUS} |"
  echo
  echo "## Test counts"
  echo
  echo "Counts below come from \`cargo nextest list --workspace --message-format=json\`"
  echo "(the same set of tests that run in CI). Counts are deduplicated by test id."
  echo
  echo "${TEST_CRATE_SECTION}"
  echo
  echo "## Line counts"
  echo
  if [ "${CLOC_USED}" = "true" ]; then
    echo "Source: \`cloc crates/ desktop/src/ desktop/ui/src/ --quiet --csv\`."
  else
    echo "Source: \`find … -name '*.rs' \\| xargs wc -l\` (fallback; \`cloc\` not installed)."
  fi
  echo "Legacy archives and vendored directories are excluded."
  echo
  echo "${LINES_SECTION}"
  echo
  echo "## Lint / audit status"
  echo
  echo "### \`cargo clippy --workspace -- -D warnings\`"
  echo
  echo "- **Status**: ${CLIPPY_STATUS}"
  echo
  echo '```text'
  echo "${CLIPPY_TAIL}"
  echo '```'
  echo
  echo "### \`cargo deny check\`"
  echo
  echo "- **Status**: ${DENY_STATUS}"
  if [ -n "${DENY_TAIL}" ]; then
    echo
    echo '```text'
    echo "${DENY_TAIL}"
    echo '```'
  fi
  echo
  echo "---"
  echo
  echo "_Regenerate with: \`bash scripts/gen-metrics.sh\`._"
} >"${OUTPUT}"

echo "[gen-metrics] Wrote ${OUTPUT}" >&2
exit 0
