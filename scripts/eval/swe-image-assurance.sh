#!/usr/bin/env bash
# swe-image-assurance.sh — ensure every instance's judgment image is present
# BEFORE the batch starts, so docker pulls never stall the run (the mirror
# path is slow/flaky: hub direct EOFs, daemon mirrors throttle; lite100
# FINDINGS S7/S8/S9).
#
# Usage:
#   swe-image-assurance.sh <instance-image.tsv> [--jobs N] [--retries N] [--give-up-retry]
#     instance-image.tsv: "<instance_id>\t<image_ref>" per line
#   --give-up-retry  : also re-attempt entries recorded as GIVE-UP in prior logs
# Idempotent: already-present images are skipped in ~1s.
set -u
TSV="${1:?usage: swe-image-assurance.sh <instance-image.tsv> [--jobs N] [--retries N] [--give-up-retry]}"
shift || true
JOBS=4; RETRIES=20; RETRY_GIVEUP=0
while [ $# -gt 0 ]; do
  case "$1" in
    --jobs) JOBS="$2"; shift 2;;
    --retries) RETRIES="$2"; shift 2;;
    --give-up-retry) RETRY_GIVEUP=1; shift;;
    *) echo "unknown arg: $1" >&2; exit 2;;
  esac
done
LOG="${SWE_IMAGE_LOG:-/tmp/swe-image-assurance.log}"

pull_one() {
  local img="$1" n=1
  docker image inspect "$img" >/dev/null 2>&1 && return 0
  while [ "$n" -le "$RETRIES" ]; do
    if docker pull -q "$img" >>"$LOG" 2>&1; then
      echo "[$(date +%H:%M:%S)] OK $img" >> "$LOG"; return 0
    fi
    echo "[$(date +%H:%M:%S)] retry $n/$RETRIES $img" >> "$LOG"
    n=$((n + 1)); sleep 30
  done
  echo "[$(date +%H:%M:%S)] GIVE-UP $img" >> "$LOG"; return 1
}
export -f pull_one; export LOG RETRIES

cut -f2 "$TSV" | xargs -P "$JOBS" -n1 -I{} bash -c 'pull_one "$@"' _ {}

# Second chance for give-ups: alternate explicit mirror host + retag.
GIVEUPS=$(grep "GIVE-UP" "$LOG" 2>/dev/null | awk '{print $NF}' | sort -u || true)
if [ -n "$GIVEUPS" ]; then
  echo "$GIVEUPS" | while read -r img; do
    docker image inspect "$img" >/dev/null 2>&1 && continue
    host="docker.m.daocloud.io/$img"
    if docker pull -q "$host" >>"$LOG" 2>&1; then
      docker tag "$host" "$img" && docker rmi "$host" >/dev/null 2>&1
      echo "[$(date +%H:%M:%S)] OK(daocloud) $img" >> "$LOG"
    else
      echo "[$(date +%H:%M:%S)] STILL-MISSING $img" >> "$LOG"
    fi
  done
fi
MISSING=0
while read -r img; do
  docker image inspect "$img" >/dev/null 2>&1 || { echo "MISSING $img"; MISSING=$((MISSING + 1)); }
done < <(cut -f2 "$TSV")
echo "assurance done: $MISSING image(s) still missing (see $LOG)"
[ "$MISSING" -eq 0 ]
