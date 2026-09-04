#!/usr/bin/env bash
# Pre-pull external-benchmark docker images so batch judgments never wait on a
# cold registry pull (docker hub direct is EOF here; the daemon's mirror list
# serves but is slow/flaky — hence the retry loop).
#
#   1. SWE-bench Verified 50-pin judgment images
#      name = "swebench/sweb.eval.x86_64.<instance_id>" with "__" -> "_1776_"
#      (docker hub forbids dunders; see swebench/image_builder/image_spec.py)
#   2. Terminal-Bench 2.1 task images (docker_image = ... in each task.toml)
#
# Safe to run concurrently with a live batch: pulls are idempotent, layers
# dedupe, and an image already present is skipped in ~1s.
set -u
PINS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../tests/eval/benchmarks" && pwd)"
TB21_DIR="${TB21_DIR:-/home/ed/datasets/tb21/terminal-bench-2-1}"
ATTEMPTS="${PREPULL_ATTEMPTS:-20}"
LOG="${PREPULL_LOG:-/tmp/prepull-images.log}"
log() { echo "[prepull $(date +%H:%M:%S)] $*" >> "$LOG"; }

pull_retry() {
  local img="$1" n=1
  docker image inspect "$img" >/dev/null 2>&1 && { log "SKIP (cached) $img"; return 0; }
  while [ "$n" -le "$ATTEMPTS" ]; do
    if docker pull -q "$img" >> "$LOG" 2>&1; then log "OK $img"; return 0; fi
    log "retry $n/$ATTEMPTS failed: $img"; n=$((n + 1)); sleep 20
  done
  log "GIVE UP $img"; return 1
}

fail=0
# ── 1. SWE judgment images ────────────────────────────────────────────────
grep -vE '^\s*(#|$)' "$PINS_DIR/swebench_verified_50.txt" | while read -r id; do
  [ -n "$id" ] || continue
  img="swebench/sweb.eval.x86_64.$id"
  img="${img//__/_1776_}"
  pull_retry "$img" || true
done

# ── 2. TB 2.1 task images ─────────────────────────────────────────────────
find "$TB21_DIR" -name task.toml | while read -r toml; do
  img="$(grep -E '^docker_image\s*=' "$toml" | head -1 | sed -E 's/^docker_image\s*=\s*"([^"]+)".*/\1/')"
  [ -n "$img" ] && { [ "$img" = "None" ] || pull_retry "$img" || true; }
done

log "pre-pull pass complete"
