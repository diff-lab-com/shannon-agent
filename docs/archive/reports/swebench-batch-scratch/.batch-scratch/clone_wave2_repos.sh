#!/usr/bin/env bash
# batch-3 wave-2 prep: clone the 5 missing pin repos (t13 method:
# init + depth-1 blob:none fetch of exactly the pins' base_commits).
set -u
SB=/home/ed/datasets/swebench
LOG=$SB/clone_log_batch3.txt
: > "$LOG"

declare -A URL=(
  [seaborn]="https://github.com/mwaskom/seaborn.git"
  [flask]="https://github.com/pallets/flask.git"
  [requests]="https://github.com/psf/requests.git"
  [xarray]="https://github.com/pydata/xarray.git"
  [pytest]="https://github.com/pytest-dev/pytest.git"
)
declare -A SHAS=(
  [seaborn]="54cab15bdacfaa05a88fbc5502a5b322d99f148e"
  [flask]="7ee9ceb71e868944a46e1ff00b506772a53a4f1d"
  [requests]="22623bd8c265b78b161542663ee980738441c307"
  [xarray]="7c4e2ac83f7b4306296ff9b7b51aaf016e5ad614 1757dffac2fa493d7b9a074b84cf8c830a706688"
  [pytest]="aa55975c7d3f6c9f6d7f68accc41bb7cadf0eb9a"
)

overall_start=$SECONDS
for short in seaborn flask requests xarray pytest; do
  t0=$SECONDS
  d=$SB/repos/$short
  if [ -d "$d/.git" ]; then
    echo "[$short] already present — skipped"
    continue
  fi
  mkdir -p "$d"
  git -C "$d" init -q 2>>"$LOG"
  git -C "$d" remote add origin "${URL[$short]}" 2>>"$LOG" || true
  # shellcheck disable=SC2046
  if git -C "$d" fetch -q --depth 1 --filter=blob:none --no-tags origin ${SHAS[$short]} >>"$LOG" 2>&1; then
    first=$(echo "${SHAS[$short]}" | awk '{print $1}')
    if git -C "$d" checkout -q -B base "$first" >>"$LOG" 2>&1; then
      ok=0; bad=""
      for s in ${SHAS[$short]}; do
        if git -C "$d" cat-file -e "$s^{commit}" 2>>"$LOG"; then ok=$((ok+1)); else bad="$bad $s"; fi
      done
      echo "[$short] OK: $ok SHAs resolvable; unresolvable:${bad:- none}; elapsed $((SECONDS-t0))s; du $(du -sh "$d" | cut -f1)"
    else
      echo "[$short] CHECKOUT FAILED (see log)"
    fi
  else
    echo "[$short] FETCH FAILED (see log)"
  fi
done
echo "TOTAL elapsed $((SECONDS-overall_start))s"
