# scripts/eval — external benchmark batch tooling (§4.13)

Three pieces that turn the §4.13 benchmark trio (regression pool,
Terminal-Bench pins, SWE-bench Verified 50) from "one CLI invocation" into
repeatable, resumable, budgeted n=3 batches. Pure orchestration: all scoring
stays in the existing runners (`eval_runner`, `bench_runner`) and, for
foreign corpora, in the corpora's own verifiers.

## The pieces

| file | role | batch status |
|---|---|---|
| `run-batch.sh` | serial n-round batch driver: one independent run dir per round, resume via state markers, cross-round token/cost ledger with a hard budget gate, single-owner preflight (pgrep + lock), 60 s rate-limit self-heal | **delivered + executed** (batch-1: regression n=3) |
| `swe-harness.sh` | `SHANNON_SB_HARNESS_CMD` adapter: parquet → issue text, disposable worktree at base_commit, agent run, `git diff` → patch, official v5 predictions, OFFICIAL `swebench.harness.run_evaluation` judgment → verdict.json | delivered; **smoke-tested with fake verdicts only** (no real docker judgment yet) |
| `tb-prebake/` | prebaked-image generator for the 9 TB pins (kills the ~936 s/rep cold uv+pytest tax measured by t15) + runtime contract + risks | delivered; **not executed** (TB batch = successor) |

## run-batch.sh in one paragraph

`run-batch.sh --suite regression --out ~/.shannon/eval/v1-regression --n 3
--bin /tmp/shannon-zhipu/shannon-glm-plan` runs 3 strictly serial rounds; a
regression round is one `eval_runner --real` pass over
`tests/eval/benchmarks/regression` (10 pinned defect tasks), producing a
full-suite `report.json` per round — exactly the layout
`eval_runner aggregate <out-root>` consumes for stable/flaky buckets and
pass-rate intervals. Remote suites (`terminal_bench`,
`swebench_verified_50`) run one `bench_runner --n 1 --real` round each,
forwarding `SHANNON_TB_TASKS_DIR` / `SHANNON_SWEBENCH_HOME` /
`SHANNON_{TB,SB}_HARNESS_CMD` untouched. Completed rounds are marked in
`<out>/.batch-state/round-N.done` and skipped on re-invocation; every round
is ledgered (tokens in/out/cache, cost when the provider reports it) and the
batch hard-stops — before the next round — once `--budget-tokens` is hit.
Exit 3 = budget stop, exit 4 = rate-limit retries exhausted; both are
resumable in place.

Execution bypass convention (rollout plan): `--bin` points at the MAIN
checkout's already-built binary or provider wrapper so worktrees never pay a
full rebuild; runner binaries likewise via `--eval-runner` /
`--bench-runner` (or `SHANNON_EVAL_RUNNER` / `SHANNON_BENCH_RUNNER`).

## Batch-1 record (2026-08-29)

- Suite: regression (10 tasks) × 3 rounds, glm-5.3-flash @ coding-plan.
- Out root: `~/.shannon/eval/v1-regression/` (3 run dirs + aggregate.json/md).
- Results, ledger and the aggregate output are in the batch report; the
  citable conclusion is the aggregate's STABLE pass-rate interval only
  (flaky tasks are quarantined, never averaged in).
