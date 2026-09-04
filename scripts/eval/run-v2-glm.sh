#!/usr/bin/env bash
# P1 external-baseline launcher for the glm-5.3-flash anchor (v2).
# Freezes every control variable from docs/agent-eval-plan-2026-09.md §三:
#   - engine: snapshot binary (.eval-bin/shannon, dev 7b8f6fb8 era) so a
#     concurrent rebuild of the main checkout can never shift the anchor
#   - model/provider: wrapper-glm.sh (glm-5.3-flash @ zhipu-coding-plan)
#   - pacing 30s (SWE) / 15s (TB), budget gates per plan §五
# Usage:
#   scripts/eval/run-v2-glm.sh swe    # P1b: SWE-bench Verified 50-pin, n=3
#   scripts/eval/run-v2-glm.sh tb9    # P1c: Terminal-Bench 9-pin, n=3
set -euo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WT_ROOT="$(cd "$SELF_DIR/../.." && pwd)"
EVAL_BIN="$WT_ROOT/.eval-bin/shannon"
WRAPPER="$WT_ROOT/scripts/eval/wrapper-glm.sh"
TBHARNESS="$WT_ROOT/scripts/eval/tb-harness-glm.sh"
RUNNERS="$WT_ROOT/target/debug/examples"

export SHANNON_BIN="$EVAL_BIN"          # wrapper/tb-harness engine override
export TB_SHANNON_BIN="$EVAL_BIN"       # tb-harness-glm.sh reads this
export SWE_HARNESS_PYTHON="$HOME/datasets/swebench/venv/bin/python"
export SHANNON_SWEBENCH_HOME="$HOME/datasets/swebench"
export SHANNON_TB_TASKS_DIR="${SHANNON_TB_TASKS_DIR:-/home/ed/datasets/terminal-bench/repo/original-tasks}"
export SWE_AGENT_MAX_TURNS=80
export SWE_MIN_DELAY_MS=30000           # batch-6 RCA: 0s pacing ⇒ 76% 429s
export SWE_PACING_RESET=1
export SHANNON_TURN_CHECKPOINT=15
export SHANNON_TOKEN_BUDGET_WARNING=true
export SWE_MODEL_NAME='shannon:shannon-glm-5.3-flash-v2'
export SHANNON_SB_AGENT_BIN="$WRAPPER"
export SHANNON_SB_HARNESS_CMD="$WT_ROOT/scripts/eval/swe-harness.sh {native_id}"
export SHANNON_TB_HARNESS_CMD="$TBHARNESS --task-dir {task_dir} {native_id}"
export TB_MAX_TURNS=80

case "${1:-}" in
  swe)
    exec bash "$SELF_DIR/run-batch.sh" --suite swebench_verified_50 \
      --out /home/ed/.shannon/eval/v2-glm-swe50 --n 3 \
      --bin "$WRAPPER" --bench-runner "$RUNNERS/bench_runner" \
      --eval-runner "$RUNNERS/eval_runner" --budget-tokens 250000000
    ;;
  tb9)
    # TB harness pacing is lighter (short tasks; task-native judging is local)
    SWE_MIN_DELAY_MS=15000 exec bash "$SELF_DIR/run-batch.sh" --suite terminal_bench \
      --out /home/ed/.shannon/eval/v2-glm-tb9 --n 3 \
      --bin "$WRAPPER" --bench-runner "$RUNNERS/bench_runner" \
      --eval-runner "$RUNNERS/eval_runner" --budget-tokens 15000000
    ;;
  *)
    echo "usage: run-v2-glm.sh swe|tb9" >&2
    exit 2
    ;;
esac
