#!/usr/bin/env bash
# batch-3 wave 0: single REAL link-validation run — django__django-13279.
# shannon produces a patch; the OFFICIAL swebench harness judges it.
set -u
WS=/home/ed/.shannon/eval/swe50-n3/wave0-django-13279/ws
mkdir -p "$WS"
export SHANNON_SWEBENCH_HOME=/home/ed/datasets/swebench
export SWE_DATASET_PARQUET="$SHANNON_SWEBENCH_HOME/SWE-bench_Verified_test_v5schema.parquet"
export SHANNON_BENCH_VERDICT_FILE="$WS/verdict.json"
cd "$WS"
export SWE_AGENT_TIMEOUT_SECS=2700
exec bash /home/ed/workspace/app/work/shannon/shannon-mono/.claude/worktrees/agent-a1cc9ba3278f3789a/scripts/eval/swe-harness.sh django__django-13279
