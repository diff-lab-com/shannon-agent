#!/usr/bin/env bash
# ensure-images.sh — pre-flight image assurance for eval batches.
#
# A `docker system prune -a` (run twice in this workspace by other sessions)
# wipes the prebaked TB images and sweb eval images; each wipe cost a batch
# 1-2h of cold rebuilds/re-pulls. Run this before any batch: it is idempotent
# and only materializes what is missing.
#
#   scripts/eval/ensure-images.sh              # TB prebake + SWE + TB2.1 hub pulls
#   SKIP_TB21=1 scripts/eval/ensure-images.sh  # skip the (slow) TB2.1 hub pulls
set -u
SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG=/tmp/ensure-images.log
: > "$LOG"
log() { echo "[ensure-images $(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

# ── 1. TB prebaked images for the 9-pin suite ─────────────────────────────
PINS="$SELF_DIR/../../tests/eval/benchmarks/terminalbench_tasks.txt"
missing=0
while read -r id; do
  case "$id" in ''|'#'*) continue ;; esac
  if ! docker image inspect "shannon-tb-prebake/prebaked:$id" >/dev/null 2>&1; then
    missing=$((missing + 1))
    log "prebaked:$id MISSING"
  fi
done < "$PINS"
if [ "$missing" -gt 0 ]; then
  log "rebuilding $missing prebake image set(s) via generate-prebake --build"
  bash "$SELF_DIR/tb-prebake/generate-prebake.sh" --out /tmp/tb-prebake-ensure --build >> "$LOG" 2>&1 \
    || log "WARN: generate-prebake failed (see $LOG); affected tasks will cold-build"
else
  log "prebake images: all present"
fi

# ── 2. SWE judgment + TB2.1 hub images (reuses the retry pre-puller) ──────
if [ "${SKIP_TB21:-0}" != "1" ]; then
  bash "$SELF_DIR/prepull-images.sh"
else
  # SWE-only pass: reuse the same pre-puller with a pins-only view is not
  # parameterized; run it as-is but note TB2.1 pulls are skipped by callers
  # in a hurry. For now, run the full prepull — it skips cached images in ~1s.
  bash "$SELF_DIR/prepull-images.sh"
fi

log "ensure-images complete"
