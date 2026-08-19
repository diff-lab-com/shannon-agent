#!/usr/bin/env bash
# ADR-0011 red line 1: the headless `shannon` CLI surface must NEVER link GUI
# libraries. The day shannon-cli grows a tauri/webkit dependency edge, every
# server/headless deployment silently starts pulling X11/GTK — this script is
# the tripwire.
#
# Usage:
#   scripts/check-headless-purity.sh                 # dependency-tree check (fast)
#   scripts/check-headless-purity.sh <shannon-bin>   # + ldd check on a built
#                                                   #   linux binary
#
# The tree check is metadata-only (no compile) and catches the structural
# regression; the optional ldd check verifies an actual binary on Linux.
set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "HEADLESS-PURITY FAIL: $1" >&2; exit 1; }

# 1) Dependency graph: no GUI crate may appear in the shannon-cli closure.
#    `cargo tree -i <crate>` inverts the graph — it only exits 0 when the
#    crate IS a dependency; "did not match any packages" (nonzero) is the
#    passing state.
for crate in tauri tauri-runtime gtk webkit2gtk webkit2gtk-sys wry; do
  if cargo tree -p shannon-cli -i "$crate" >/dev/null 2>&1; then
    cargo tree -p shannon-cli -i "$crate" >&2 || true
    fail "shannon-cli depends on GUI crate '$crate'"
  fi
done
echo "OK: shannon-cli dependency graph has no GUI crate edges"

# 2) Optional: ldd a built binary (linux) and assert no webkit2gtk linkage.
if [ "${1:-}" != "" ]; then
  BIN="$1"
  [ -f "$BIN" ] || fail "binary not found: $BIN"
  if ldd "$BIN" | grep -qi webkit2gtk; then
    ldd "$BIN" | grep -i webkit2gtk >&2 || true
    fail "ldd on $BIN resolves webkit2gtk"
  fi
  echo "OK: ldd on $BIN shows no webkit2gtk"
fi
