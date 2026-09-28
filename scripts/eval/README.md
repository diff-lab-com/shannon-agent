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

## Runbook rules (hard-won, 2026-09 cycle — see docs/backlog.md §一)

1. **One variable per acceptance run.** Never change binary + adapter +
   concurrency in the same sweep; a multi-variable result cannot be attributed.
2. **Content-idle watchdog threshold must exceed the model's max thinking
   silence.** GLM-5.3-flash: 312 s measured. 180 s killed healthy streams and
   the engine retry re-thought from scratch (rc=3 death spiral).
3. **Launch verification asserts the start event's `prompt` field**, not
   response existence. (`-p - < file` assigns the literal "-" as prompt.)
4. **No prompt wording changes from single-task evidence.**
   (docs/backlog.md §五 records the reverted counterexample.)
5. **Batch drivers must hard-fail on zero tasks** — a silently empty task
   list produces a green "completed" run with no data (lite100 S6).
6. **Consistency checks on rebuilt datasets must cover ALL judgment-relevant
   columns** (image, eval_script AND FAIL_TO_PASS/PASS_TO_PASS). pyarrow
   coerces a JSON string into a per-character list against a list<string>
   schema, corrupting every test ID silently (lite100 D1 — 7 false failures).
7. **Batch prefetch base_commits + judgment images before launching waves**;
   a give-up list needs an automated second-chance sweep. Git network ops
   (fetch/ls-remote/worktree add) MUST run under a timeout — bare git hangs
   indefinitely on bad egress and one hung slot stalls the pipeline (D5–D8).
8. **Benchmarks verify no-regression; they do not select product behavior.**
   An A/B on an eval pool may validate that a knob causes no harm, but a
   behavior default (e.g. thinking on/off) needs a user-scenario rationale —
   score deltas within run-to-run variance are not evidence (P2-1 postmortem:
   +10pp was inside the ±4-task round variance; the gate passed on wall-clock,
   not on quality).

## Lite100 hardening package (2026-09-28, feat/eval-hardening-thinking-knob)

Full evidence chain: `~/.shannon/eval/lite100-glm-dev/{FINDINGS,REPORT}.md`.

- `swe-harness.sh` — two patches: (a) SIGKILLed `worktree add` leaves a
  LOCKED registration that `worktree prune` preserves; unlock + remove
  --force before prune so retries work. (b) `ensure_local` short-circuits on
  `cat-file -e <base>^{commit}` — a fetched-by-sha commit is not an ancestor
  of HEAD, and the old path paid a hung network fetch per task (D5/D8).
- `swe-prefetch-bases.sh` — batch preflight: verify/fetch every
  base_commit with a hard per-fetch timeout before the run.
- `swe-image-assurance.sh` — parallel judgment-image pull with retries,
  give-up report and a second-chance explicit-mirror sweep (S7/S9).
- `make-swe-v5.py` — offline v5 parquet derivation via the swebench 3.0.8
  generator + full-column validation (D1: check F2P/P2P too — pyarrow
  coerces JSON strings to per-character lists against list<string> schemas).
- `profile-zhipu-coding-plan.sh` — frozen channel profile: 3 workers /
  15 s stagger / stream-idle 360 s (3-way had zero 429s over 100 tasks;
  6-way tripped rate limits). Optional `SHANNON_THINKING=disabled` knob
  (engine P1-2) to trade thinking latency for wall-clock, pending A/B.
- Engine (P1-1/P1-2): `turn/end.llm_steps` now counts LLM calls folded into
  each turn (per-call efficiency denominator; turn/end itself remains one
  per user-visible round by design); `SHANNON_THINKING` env knob maps to
  the GLM `thinking.type` request field (Zhipu-family providers only; an
  explicit toggle overrides reasoning_effort); cutoff Progress events carry
  the error class (`class=timeout|stream_interrupted, err=…`).
- Thinking-toggle guidance (user-facing tradeoff, NOT a default change):
  disabling thinking trades solution depth for wall-clock — measured on the
  10-task regression pool: wall −9%, tokens −6%, resolved +10pp (within
  round variance). Reasonable for latency-sensitive interactive runs on
  small/mechanical tasks; keep thinking ON for hard multi-step work and
  generous budgets. Default remains the provider default (thinking on).
- Flaky-test note: the two malformed-call agent_loop tests serialize on
  `MALFORMED_STREAK_ENV_LOCK` — they read `SHANNON_MAX_CONSECUTIVE_MALFORMED_CALLS`,
  which another test mutates process-globally (3→2 mid-run caused false
  stop-loss failures under full-suite parallelism).
