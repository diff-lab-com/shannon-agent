#!/usr/bin/env bash
# Tripwire for test-suite /tmp litter.
#
# Why: shannon's tests used to create fixture directories directly under
# /tmp (e.g. `shannon_repomap_test_*`), a fresh set on every test run, and
# never cleaned them up. 3,148 leftovers piled up on one dev machine —
# enough stale entries that product code scanning the temp dir slowed to a
# crawl (repo-map scans at 48s/pass). All test-side producers now use
# `tempfile::TempDir` (RAII cleanup); this script is the tripwire that
# keeps them closed.
#
# Usage:
#   scripts/check-tmp-leaks.sh --scan              # count leftovers per whitelisted prefix
#   scripts/check-tmp-leaks.sh --snapshot <file>   # write counts to <file>
#   scripts/check-tmp-leaks.sh --check <file>      # exit 1 if any prefix grew vs <file>
#   scripts/check-tmp-leaks.sh --clean             # delete whitelisted leftovers (opt-in!)
#
# The whitelist below maps 1:1 to test-side producers in the repo.
# Product-runtime temp files (e.g. `shannon-clipboard-*.txt`) are
# deliberately NOT listed: they are app behavior, not test litter.
set -euo pipefail

TMP_ROOT="${TMPDIR:-/tmp}"

PREFIXES=(
  "shannon_repomap_test_"            # repomap integration snapshots (tests/incremental_tests.rs)
  "shannon_repomap_speedup_"         # repomap speed benchmark fixture (tests/incremental_tests.rs)
  "shannon_repomap_injector_"        # query-engine injector tests (repo_map_injector.rs)
  "shannon_repomap_cache_test_"      # repomap disk-cache tests (cache.rs)
  "shannon_pcs_"                     # provider config store tests (provider_config_store.rs)
  "shannon-crash-hook-test-"         # crash hook tests (crash_hook.rs)
  "shannon-test-housekeeping"        # housekeeper tests (housekeeping.rs)
  "shannon-session-search-test"      # session search tool tests (session_search_tool.rs)
  "shannon-session-test"             # session roundtrip tests (session_tests.rs, builtin/session.rs)
  "shannon-test-"                    # misc test workspaces (cli_mock_tests.rs, project_memory.rs)
)

count_prefix() {
  local prefix=$1
  local matches=()
  shopt -s nullglob
  # shellcheck disable=SC2206  # intentional glob expansion
  matches=("$TMP_ROOT/$prefix"*)
  shopt -u nullglob
  echo "${#matches[@]}"
}

print_samples() {
  local prefix=$1 limit=$2
  local matches=()
  shopt -s nullglob
  matches=("$TMP_ROOT/$prefix"*)
  shopt -u nullglob
  local m
  for m in "${matches[@]:0:$limit}"; do
    echo "    $m"
  done
}

scan() {
  local prefix n
  for prefix in "${PREFIXES[@]}"; do
    n=$(count_prefix "$prefix")
    echo "$prefix $n"
  done
}

cmd="${1:-}"

case "$cmd" in
  --scan)
    scan
    ;;
  --snapshot)
    [[ $# -eq 2 ]] || { echo "usage: $0 --snapshot <file>" >&2; exit 2; }
    scan >"$2"
    ;;
  --check)
    [[ $# -eq 2 ]] || { echo "usage: $0 --check <file>" >&2; exit 2; }
    # current counts keyed by prefix
    declare -A now
    while read -r prefix n; do
      now["$prefix"]=$n
    done < <(scan)
    leaked=0
    while read -r prefix base; do
      n=${now["$prefix"]:-0}
      if (( n > base )); then
        leaked=1
        echo "TMP-LEAK FAIL: prefix '$prefix' grew from $base to $n entries" >&2
        print_samples "$prefix" 5 >&2
      fi
    done <"$2"
    if (( leaked )); then
      echo "A test run left new fixture entries in $TMP_ROOT — find the producer and use tempfile::TempDir." >&2
      exit 1
    fi
    echo "OK: no /tmp litter growth across ${#PREFIXES[@]} whitelisted prefixes"
    ;;
  --clean)
    total=0
    for prefix in "${PREFIXES[@]}"; do
      stale=()
      shopt -s nullglob
      stale=("$TMP_ROOT/$prefix"*)
      shopt -u nullglob
      for m in "${stale[@]}"; do
        rm -rf -- "$m"
        total=$((total + 1))
      done
    done
    echo "removed $total leftover entries under $TMP_ROOT (whitelisted prefixes only)"
    ;;
  *)
    echo "usage: $0 --scan | --snapshot <file> | --check <file> | --clean" >&2
    exit 2
    ;;
esac
