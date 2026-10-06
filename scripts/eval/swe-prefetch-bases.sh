#!/usr/bin/env bash
# swe-prefetch-bases.sh — batch preflight for SWE-bench repo clones.
#
# Verifies every instance's base_commit is a locally-resolvable object in the
# shared clone and fetches the missing ones (with a hard per-fetch timeout —
# bare `git fetch`/`worktree add` network negotiation hangs indefinitely on
# flaky egress and used to stall whole waves; see lite100 FINDINGS D5/D7/D8).
#
# Usage:
#   swe-prefetch-bases.sh <parquet> [--repos-dir DIR] [--timeout SECS] [--jobs N]
#   parquet: SWE-bench v5-schema parquet (instance_id/repo/base_commit)
#   Output:  report to stdout; exit non-zero iff any commit remains missing.
set -u
PARQUET="${1:?usage: swe-prefetch-bases.sh <parquet> [--repos-dir D] [--timeout S] [--jobs N]}"
shift || true
REPOS_DIR="${REPOS_DIR:-$HOME/datasets/swebench/repos}"
FETCH_TIMEOUT="${FETCH_TIMEOUT:-300}"
JOBS="${JOBS:-4}"
PYBIN="${SWE_HARNESS_PYTHON:-python3}"

MISSING_LIST=$(mktemp)
trap 'rm -f "$MISSING_LIST"' EXIT

"$PYBIN" - "$PARQUET" "$REPOS_DIR" > "$MISSING_LIST" <<'PYEOF'
import sys, subprocess
import pyarrow.parquet as pq
t = pq.read_table(sys.argv[1]).to_pydict()
repos_dir = sys.argv[2]
seen = set()
for iid, repo, base in zip(t["instance_id"], t["repo"], t["base_commit"]):
    key = (repo, base)
    if key in seen:
        continue
    seen.add(key)
    name = repo.split("/")[-1]
    path = f"{repos_dir}/{name}"
    r = subprocess.run(["git", "-C", path, "cat-file", "-e", f"{base}^{{commit}}"],
                       capture_output=True)
    if r.returncode != 0:
        print(f"{name}\t{base}")
PYEOF

TOTAL=$(wc -l < "$MISSING_LIST" | tr -d ' ')
echo "[prefetch] $TOTAL base commit(s) missing locally"
[ "$TOTAL" -eq 0 ] && exit 0

fetch_one() {
  local repo="$1" sha="$2" n=1
  while [ "$n" -le 3 ]; do
    if timeout "$FETCH_TIMEOUT" git -C "$REPOS_DIR/$repo" fetch origin "$sha" >>"${PREFETCH_LOG:-/tmp/swe-prefetch.log}" 2>&1; then
      if git -C "$REPOS_DIR/$repo" cat-file -e "${sha}^{commit}" 2>/dev/null; then
        echo "[prefetch] OK $repo $sha (attempt $n)"
        return 0
      fi
    fi
    n=$((n + 1)); sleep 5
  done
  echo "[prefetch] MISS $repo $sha"
  return 1
}
export -f fetch_one
export REPOS_DIR FETCH_TIMEOUT PREFETCH_LOG="${PREFETCH_LOG:-/tmp/swe-prefetch.log}"

FAILS=$(cut -f1,2 "$MISSING_LIST" | xargs -P "$JOBS" -n2 -d'\n' bash -c 'fetch_one $(echo "$0" | tr "\t" " ")' 2>/dev/null; true)
LEFT=$(cut -f1,2 "$MISSING_LIST" | while IFS=$'\t' read -r repo sha; do
  git -C "$REPOS_DIR/$repo" cat-file -e "${sha}^{commit}" 2>/dev/null || echo "$repo $sha"
done | wc -l | tr -d ' ')
echo "[prefetch] done: $((TOTAL - LEFT))/$TOTAL fetched, $LEFT still missing"
[ "$LEFT" -eq 0 ]
