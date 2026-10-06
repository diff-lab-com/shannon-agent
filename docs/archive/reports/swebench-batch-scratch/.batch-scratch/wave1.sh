#!/usr/bin/env bash
# batch-3 wave launcher template. WAVE=<1|2|3> selects the gate file / rounds.
#   wave 1: --n 1, gate = django 20          (round 1)
#   wave 2: --n 2, gate = non-django 30      (round 2)
#   wave 3: --n 4, gate = all (pass-through) (rounds 3-4)
set -u
WAVE="${1:?usage: wave.sh <1|2|3>}"
WT=/home/ed/workspace/app/work/shannon/shannon-mono/.claude/worktrees/agent-a1cc9ba3278f3789a
OUTROOT=/home/ed/.shannon/eval/swe50-n3
MAIN=/home/ed/workspace/app/work/shannon/shannon-mono

export SHANNON_SWEBENCH_HOME=/home/ed/datasets/swebench
export SWE_DATASET_PARQUET="$SHANNON_SWEBENCH_HOME/SWE-bench_Verified_test_v5schema.parquet"
export SHANNON_SB_HARNESS_CMD="sh $WT/scripts/eval/swe-wave-gate.sh {native_id}"
# Batch-wide agent wall-clock cap (2700 s): wave-0 attempt 1 at the 1800 s
# default was killed mid-work with a partial patch — same cap for every rep
# keeps the harness configuration uniform across the whole batch.
export SWE_AGENT_TIMEOUT_SECS=2700

case "$WAVE" in
  1)
    export SHANNON_SWE_WAVE_FILE="$OUTROOT/.batch-state/wave1-ids.txt"
    ROUNDS=1
    ;;
  2)
    export SHANNON_SWE_WAVE_FILE="$OUTROOT/.batch-state/wave2-ids.txt"
    ROUNDS=2
    ;;
  3)
    export SHANNON_SWE_WAVE_FILE=all
    ROUNDS=4
    ;;
  *) echo "unknown wave $WAVE" >&2; exit 2 ;;
esac

exec bash "$WT/scripts/eval/run-batch.sh" \
  --suite swebench_verified_50 \
  --out "$OUTROOT" \
  --n "$ROUNDS" \
  --budget-tokens 45000000 \
  --budget-scope io \
  --bin /tmp/shannon-zhipu/shannon-glm-plan \
  --bench-runner "$MAIN/target/debug/examples/bench_runner" \
  --timeout 7200
