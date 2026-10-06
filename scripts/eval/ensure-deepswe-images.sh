#!/usr/bin/env bash
# Pre-pull DeepSWE task images so per-task image pulls don't sit in the run's
# critical path. Skips images already present; sequential (one pull at a time)
# to stay polite to ECR anonymous rate limits; pauses if free disk < MIN_FREE_GB.
#
# Usage: ensure-deepswe-images.sh [tasks_dir]   (default: ~/eval-corpora/deep-swe/tasks)
set -u
TASKS_DIR="${1:-$HOME/eval-corpora/deep-swe/tasks}"
MIN_FREE_GB=60

free_gb() { df --output=avail -BG / | tail -1 | tr -dc '0-9'; }

count=0
for toml in "$TASKS_DIR"/*/task.toml; do
  image="$(grep -o 'docker_image = "[^"]*"' "$toml" | cut -d'"' -f2)"
  [ -n "$image" ] || continue
  if docker image inspect "$image" >/dev/null 2>&1; then
    continue
  fi
  while [ "$(free_gb)" -lt "$MIN_FREE_GB" ]; do
    echo "[prepull] disk below ${MIN_FREE_GB}G — sleeping 10m"
    sleep 600
  done
  count=$((count + 1))
  echo "[prepull] pulling #$count: $image"
  docker pull "$image" >/dev/null 2>&1 || echo "[prepull] WARN: pull failed: $image"
done
echo "[prepull] done; pulled $count new images"
