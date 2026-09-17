#!/usr/bin/env bash
# DeepSWE v1.1 wave driver: preflight gate + pier job launch/resume + token
# ledger + result aggregation. Pier owns concurrency and per-trial state
# (pier job resume skips completed trials), so this wrapper only adds the
# shannon eval conventions: wrapper-glm key injection, preflight gate,
# budget gate (hard-stop; resume in place), and a reward.json aggregate.
#
# Usage:
#   deepswe-wave.sh start  --include <glob>  --job-name <name>  [--concurrency N]
#                          [--budget-tokens M] [--tasks-dir DIR] [--resume]
#   deepswe-wave.sh aggregate --job-name <name>
#
# Examples:
#   deepswe-wave.sh start --include '*' --job-name deepswe-base-w1
#   deepswe-wave.sh start --include 'bandit-*' --job-name deepswe-smoke
#   deepswe-wave.sh aggregate --job-name deepswe-base-w1
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TASKS_DIR_DEFAULT="${HOME}/eval-corpora/deep-swe/tasks"
JOBS_DIR_DEFAULT="${HOME}/.shannon/eval/deepswe/jobs"
SHANNON_BIN_DEFAULT="/home/ed/workspace/app/work/shannon/shannon-mono/target/debug/shannon"
KEY_FILE="${HOME}/.shannon/credentials/zhipu.json"
PROVIDER_MODEL="zhipu-coding-plan/glm-5.3-flash"
DEFAULT_CONCURRENCY=3
# Full-wave gate: 113 tasks x observed ~1M in-tokens upper bound per task is
# far above the official $0.24/task cost; 150M in is ~3x the SWE50 batch11
# wave (59M) scaled to 113 long-horizon tasks. Hard-stops before the next
# unstarted trial via monitor; resume re-enters without re-running done work.
DEFAULT_BUDGET_TOKENS=450000000

die() { echo "FATAL: $*" >&2; exit 9; }

need_pier() { command -v pier >/dev/null || die "pier not installed (uv tool install datacurve-pier)"; }
need_key() {
  [ -r "$KEY_FILE" ] || die "$KEY_FILE missing"
  SHANNON_API_KEY="$(python3 -c "
import json,sys
print(json.JSONDecoder().raw_decode(open(sys.argv[1]).read())[0]['value'])
" "$KEY_FILE")"
  [ -n "$SHANNON_API_KEY" ] || die "no 'value' credential in $KEY_FILE"
  export SHANNON_API_KEY
}

jobs_dir_for() { echo "${JOBS_DIR:-$JOBS_DIR_DEFAULT}"; }

ledger_file_for() { echo "$(jobs_dir_for)/${1}.tokens-ledger"; }

# Sum n_input_tokens over every trial result.json of a job (best effort;
# trials still running simply lack fields).
ledger_total() {
  local job_name="$1" total=0 f
  for f in "$(jobs_dir_for)/${job_name}"/*/result.json; do
    [ -f "$f" ] || continue
    t="$(python3 -c "
import json,sys
try:
    d=json.load(open(sys.argv[1]))
    agent=(d.get('agent_context') or d.get('agent_result') or {})
    print(int(agent.get('n_input_tokens') or 0))
except Exception:
    print(0)
" "$f")"
    total=$((total + t))
  done
  echo "$total"
}

cmd_start() {
  local include="" job_name="" concurrency="$DEFAULT_CONCURRENCY"
  local budget="$DEFAULT_BUDGET_TOKENS" tasks_dir="$TASKS_DIR_DEFAULT" resume=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --include) include="$2"; shift 2;;
      --job-name) job_name="$2"; shift 2;;
      --concurrency) concurrency="$2"; shift 2;;
      --budget-tokens) budget="$2"; shift 2;;
      --tasks-dir) tasks_dir="$2"; shift 2;;
      --resume) resume=1; shift;;
      *) die "unknown flag: $1";;
    esac
  done
  [ -n "$job_name" ] || die "--job-name required"
  need_pier; need_key

  # T1 lesson: never launch into a degraded egress window.
  "$SCRIPT_DIR/preflight-network.sh" || die "preflight failed — do not launch"

  [ -d "$tasks_dir" ] || die "tasks dir missing: $tasks_dir"

  local launcher=(pier run -p "$tasks_dir"
    --agent-import-path shannon_pier_agent:Shannon
    -m "$PROVIDER_MODEL"
    --ae "SHANNON_API_KEY=$SHANNON_API_KEY"
    -n "$concurrency"
    --job-name "$job_name"
    -o "$(jobs_dir_for)")
  [ -n "$include" ] && [ "$include" != "*" ] && launcher+=(-i "$include")

  if [ "$resume" = 1 ]; then
    echo "[wave] resuming job $job_name (completed trials are skipped by pier)"
   pier job resume "$(jobs_dir_for)/$job_name" 2>&1 | grep -v LiteLLM
  else
    echo "[wave] launching job=$job_name include='$include' n=$concurrency budget=${budget}"
    env PYTHONPATH="$SCRIPT_DIR/pier-adapter" \
      SHANNON_PIER_BIN="${SHANNON_PIER_BIN:-$SHANNON_BIN_DEFAULT}" \
      "${launcher[@]}" 2>&1 | grep -v LiteLLM
  fi

  local total; total="$(ledger_total "$job_name")"
  echo "$total" > "$(ledger_file_for "$job_name")"
  echo "[wave] job $job_name ledger: ${total} input tokens (budget ${budget})"
  [ "$total" -le "$budget" ] || echo "[wave] WARNING: budget exceeded — aggregate and inspect before resuming"
}

# Aggregate: per-trial reward + infra-failure separation (T1 reporting rule).
cmd_aggregate() {
  local job_name="" i=0 resolved=0 failed=0 infra=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --job-name) job_name="$2"; shift 2;;
      *) die "unknown flag: $1";;
    esac
  done
  [ -n "$job_name" ] || die "--job-name required"
  local root; root="$(jobs_dir_for)/$job_name"
  [ -d "$root" ] || die "job dir missing: $root"
  printf "%-60s %8s %8s %8s %s\n" TRIAL REWARD F2P P2P NOTE
  for d in "$root"/*/; do
    [ -d "$d" ] || continue
    i=$((i + 1))
    local reward="DNF" f2p="-" p2p="-" note=""
    if [ -f "$d/verifier/reward.json" ]; then
      eval "$(python3 -c "
import json,sys
r=json.load(open(sys.argv[1]))
print('reward=%s; f2p=%s; p2p=%s' % (r.get('reward'), r.get('f2p'), r.get('p2p')))
" "$d/verifier/reward.json")"
    else
      reward="DNF"
      if [ -f "$d/exception.txt" ]; then
        note="infra/exception: $(head -c 80 "$d/exception.txt" | tr '\n' ' ')"
        infra=$((infra + 1))
      fi
    fi
    if [ "$reward" = "1" ]; then resolved=$((resolved + 1))
    elif [ "$reward" != "DNF" ]; then failed=$((failed + 1)); fi
    printf "%-60s %8s %8s %8s %s\n" "$(basename "$d")" "$reward" "$f2p" "$p2p" "$note"
  done
  echo "---"
  echo "trials=$i resolved=$resolved failed=$failed dnf/infra=$infra"
  echo "resolve-rate (graded only) = $resolved / $((resolved + failed))"
}

case "${1:-}" in
  start) shift; cmd_start "$@";;
  aggregate) shift; cmd_aggregate "$@";;
  *) sed -n '2,20p' "$0"; exit 1;;
esac
