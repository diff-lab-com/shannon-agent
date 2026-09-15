#!/usr/bin/env bash
# batch-3 image pre-pull: sequential, wave-ordered (django first), retry pass
# at the end, disk floor guard. Logs per-image result to prepull.log.
set -u
LOG=/home/ed/workspace/app/work/shannon/shannon-mono/.claude/worktrees/agent-a1cc9ba3278f3789a/.batch-scratch/prepull.log
FLOOR_GB=60

pull_one() { # pull_one <image>
  local img="$1"
  docker image inspect "$img" >/dev/null 2>&1 && { echo "CACHED $img"; return 0; }
  local free
  free=$(df --output=avail -BG /var/lib/docker | tail -1 | tr -dc '0-9')
  if [ "$free" -lt "$FLOOR_GB" ]; then
    echo "DISK-FLOOR ${free}G < ${FLOOR_GB}G — stop pre-pull at $img"
    return 2
  fi
  echo "PULL $img $(date +%H:%M:%S)"
  if docker pull "$img" >>"$LOG" 2>&1; then
    echo "DONE  $img $(date +%H:%M:%S)"
    return 0
  fi
  echo "FAIL  $img $(date +%H:%M:%S)"
  return 1
}

# Order: wave-1 django pins in pin-file order, then everything else.
ORDER=/home/ed/workspace/app/work/shannon/shannon-mono/.claude/worktrees/agent-a1cc9ba3278f3789a/.batch-scratch/prepull-order.txt
failed=()
while read -r img; do
  [ -n "$img" ] || continue
  if ! pull_one "$img"; then
    rc=$?
    [ "$rc" -eq 2 ] && exit 2
    failed+=("$img")
  fi
done < "$ORDER"

if [ "${#failed[@]}" -gt 0 ]; then
  echo "=== retry pass (${#failed[@]} failed) ==="
  sleep 60
  for img in "${failed[@]}"; do
    pull_one "$img" || true
  done
fi
echo "PREPULL-COMPLETE $(date +%H:%M:%S)"
