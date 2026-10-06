#!/usr/bin/env bash
# provider-ab-test.sh — compare GLM endpoints for eval fitness.
#
# Evidence driver for the provider decision (docs/plan-tb2.1-improvements and
# the 6-min-cap RCA): zhipu-coding-plan vs standard pay-per-use endpoint.
# Measures, per endpoint, over the same k identical medium tasks:
#   wall-clock p50/p95, first-token latency, provider timeout rate, tokens, cost.
#
# Prerequisites:
#   1. Two provider profiles with real keys:
#        ~/.shannon/credentials/zhipu.json        (coding-plan key)
#        ~/.shannon/credentials/zhipu-payg.json   (pay-per-use key)
#   2. A task list file (one TB2.1 task id per line), e.g. the 21
#      provider-timeout stratum: tasks whose P1d failure was Request-timed-out.
#
# Usage:
#   scripts/eval/provider-ab-test.sh <task-list-file> [rounds]
set -euo pipefail

TASK_LIST="${1:?usage: provider-ab-test.sh <task-list-file> [rounds]}"
ROUNDS="${2:-1}"
SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WT_ROOT="$(cd "$SELF_DIR/../.." && pwd)"

read_key() { python3 -c "
import json,sys
print(json.JSONDecoder().raw_decode(open(sys.argv[1]).read())[0]['value'])
" "$1"; }

CODEG_KEY="$(read_key "$HOME/.shannon/credentials/zhipu.json")"
PAYG_KEY="$(read_key "$HOME/.shannon/credentials/zhipu-payg.json" 2>/dev/null || true)"
[ -n "$PAYG_KEY" ] || { echo "FATAL: $HOME/.shannon/credentials/zhipu-payg.json missing — provision a pay-per-use key first" >&2; exit 9; }

run_arm() { # $1 label, $2 key, $3 base_url, $4 provider
  local label="$1" key="$2" base="$3" prov="$4"
  local out="/home/ed/.shannon/eval/provider-ab/$label"
  mkdir -p "$out"
  SHANNON_API_KEY="$key" \
  SHANNON_BASE_URL="$base" \
  SWE_MIN_DELAY_MS=15000 SWE_PACING_RESET=1 \
  SWE_AGENT_MAX_TURNS=80 SWE_HARNESS_PYTHON="$HOME/datasets/swebench/venv/bin/python" \
  SHANNON_SWEBENCH_HOME="$HOME/datasets/swebench" \
  SWE_MODEL_NAME="shannon:shannon-glm-5.3-flash-ab-$label" \
  SHANNON_SB_AGENT_BIN="$SELF_DIR/wrapper-glm.sh" \
  SHANNON_SB_HARNESS_CMD="$SELF_DIR/swe-harness.sh {native_id}" \
  bash "$SELF_DIR/run-batch.sh" --suite swebench_verified_50 \
    --out "$out" --n "$ROUNDS" --force \
    --bin "$SELF_DIR/wrapper-glm.sh" \
    --bench-runner "$WT_ROOT/target/debug/examples/bench_runner" \
    --eval-runner "$WT_ROOT/target/debug/examples/eval_runner" \
    --budget-tokens 60000000 || true
  echo "[ab] arm $label done"
}

echo "[ab] NOTE: wrapper-glm.sh hardcodes --provider zhipu-coding-plan; the"
echo "[ab] SHANNON_BASE_URL/SHANNON_PROVIDER env in each arm overrides it."
echo "[ab] Verify one smoke call per arm before trusting a full sweep."

run_arm coding-plan "$CODEG_KEY" "https://open.bigmodel.cn/api/coding/paas/v4" zhipu-coding-plan
run_arm payg        "$PAYG_KEY"  "https://open.bigmodel.cn/api/paas/v4"         zhipu

echo "[ab] compare: ~/.shannon/eval/provider-ab/{coding-plan,payg}/.batch-state ledgers"
echo "[ab] metrics that matter: provider-timeout rate (verdict notes), p50/p95 wall-clock, cost/token"
