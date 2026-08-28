#!/usr/bin/env bash
# swe-wave-gate.sh — wave-scoped gate in front of swe-harness.sh (batch-3).
#
# Staged execution without touching the canonical pin file: the pins stay the
# workload fingerprint of record; the gate only decides WHICH pins execute in
# the current wave. bench_runner calls this as the SHANNON_SB_HARNESS_CMD
# template (`sh <abs-path>/swe-wave-gate.sh {native_id}`).
#
#   SHANNON_SWE_WAVE_FILE  file with one instance_id per line (`#` comments).
#                          Unset / empty / "all" / not-a-file ⇒ pass-through.
#
# Contract for a gated-out pin: exit 0 WITHOUT a verdict. Per the delegation
# contract (eval_benchmarks::run_remote_rep) that records `Ambiguous` —
# "exit 0 without usable verdict.json — not counted" — the same honest
# zero-cost convention the t15 smoke probe used. It is never resolved and
# never failed; the rep simply did not run in this wave.
#
# Usage: swe-wave-gate.sh <native_id>
set -u

native_id="${1:?usage: swe-wave-gate.sh <native_id>}"
wave="${SHANNON_SWE_WAVE_FILE:-}"

if [ -n "$wave" ] && [ "$wave" != "all" ] && [ -f "$wave" ]; then
  if ! grep -qx "$native_id" "$wave"; then
    echo "[swe-wave-gate] $native_id not in wave $(basename "$wave") — skipped (no verdict ⇒ Ambiguous, not counted)"
    exit 0
  fi
fi

exec "$(cd "$(dirname "$0")" && pwd)/swe-harness.sh" "$native_id"
