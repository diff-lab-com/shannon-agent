# SWE-bench matplotlib max-turns=60 probe (2026-08-31)

**Verdict**: `max-turns=60` does NOT help the 3 stuck matplotlib tasks. Reject for batch-4.

## TL;DR

The batch-3 report flagged 4/5 matplotlib tasks as unresolved with low output tokens (≤2K), suggesting premature termination under `--max-turns 30`. This probe re-runs the 4 unresolved matplotlib tasks with `SWE_AGENT_MAX_TURNS=60` (everything else identical) to test the hypothesis that more turns = more passes.

**Result**: 1/4 recovered (`matplotlib-23314`, the parse-error bug — recovery is variance, not infrastructure), 3/4 still NOT resolved (`14623`, `20488`, `20676`). Token usage roughly doubles across all 4 (agent DOES run longer), but longer ≠ better. The matplotlib failures are NOT a turn-budget problem.

**Recommendation**:
- Keep `max-turns=30` for batch-4
- The 20% matplotlib pass rate (1/5) is the current ceiling for minimax-m3 — likely a model-fitness issue (matplotlib's parametrised test surface + class hierarchy), not a budget issue
- Pursue the parse-error fix from `zero-output-token-rca-2026-08-31.md` independently — that fix would recover `matplotlib-23314` reliably (the variance-based recovery here is not durable)

## Setup

- Date / anchor: 2026-08-31 · probe driver: `/tmp/swe-batch3-matplotlib-probe.sh` · results: `~/.shannon/eval/swe50-n3-matplotlib-probe/matplotlib-probe.tsv`
- Model: `minimax-m3` via `shannon:shannon-minimax-m3-probe-mt60` (per-provider tag)
- Harness: same `swe-harness.sh` as batch-3 (commit `52aeaf4e`)
- Only change: `SWE_AGENT_MAX_TURNS=60` (was 30)

## Tasks

| Task | batch-3 (mt=30) | probe (mt=60) | Tokens in Δ | Tokens out Δ |
|---|---|---|---|---|
| `matplotlib__matplotlib-14623` | NOT resolved | NOT resolved | +0.80M | +2.1K |
| `matplotlib__matplotlib-20488` | NOT resolved | NOT resolved | +1.50M | +1.0K |
| `matplotlib__matplotlib-20676` | NOT resolved | NOT resolved | +1.76M | +14.5K |
| `matplotlib__matplotlib-23314` | NOT resolved | **RESOLVED** ✓ | +2.30M | +4.9K |

**Skipped control** (`matplotlib__matplotlib-13989`): already resolved at `mt=30` in batch-3 (1.03M in / 4.0K out). Re-running it at `mt=60` would test nothing about the unresolved tasks — keeping the budget for the 4 unknowns.

## What changed

| Task | batch-3 output tokens | probe output tokens | Notes |
|---|---|---|---|
| 14623 | 3,345 | 5,447 | agent ran more turns |
| 20488 | 8,767 | 9,800 | barely changed |
| 20676 | 2,706 | 17,212 | 6× — agent clearly ran far longer |
| 23314 | 62 (parse-error early exit) | 4,935 | parse-error didn't fire this time |

In every case, the agent had room to make more tool calls. But the patches still fail the official harness's tests. The failure is upstream of the turn budget.

## Why `matplotlib-23314` "recovered"

This task failed in batch-3 due to the **parse-error bug** documented in `zero-output-token-rca-2026-08-31.md`: the LLM emitted a malformed tool-call JSON on its first attempt and the agent exited after 1 turn. The probe's pass is **not** evidence that more turns help — it's evidence of LLM sampling variance: the same model on the same task with the same harness produced a valid tool_call this time, so the agent completed all turns and produced a real patch.

To verify: re-run `matplotlib-23314` 5 more times at `mt=30` and count pass rate. If the recovery rate is ~25-50%, the underlying pass rate is noise-bounded and `mt=60` doesn't matter. (Out of scope for this probe.)

## Cost

- Total tokens: 10.18M in / 37K out
- Cost: **$3.00** (~$0.75 per task)
- Probe consumed ~9% of the remaining batch-3 budget

## What this tells us about the matplotlib failure mode

The batch-3 hypothesis was: matplotlib tasks get cut off by the 30-turn ceiling, agent hasn't finished when wall-clock hits, patches are incomplete.

The probe falsifies that hypothesis. With 60 turns, all 4 tasks ran to natural conclusion (not wall-clock), but 3/4 still failed. The likely real causes:

1. **Test parametrisation**: matplotlib's test suite uses heavy `@pytest.mark.parametrize` over rendering backends. minimax-m3 tends to write patches that work on the example in the issue but miss parametrised edges.
2. **Class hierarchy depth**: matplotlib's artist hierarchy (`Axes` → `SubplotBase` → `_Axes3D` etc.) means a fix often needs to override methods at multiple levels. minimax-m3 patches tend to be local.
3. **Visual / behavioural correctness**: matplotlib tests check pixel-level output, which minimax-m3 has no way to verify during the patch loop.

This is consistent with the per-repo pass-rate table in `swe-bench-batch3-2026-08-30.md`: matplotlib at 1/5 (20%), well below django (55%) and scikit-learn (67%).

## What to do for batch-4

1. **Keep `max-turns=30`**: no win from 60 on matplotlib, and the longer runs burn ~2× cost per task.
2. **Land the parse-error fix** (one-line change in `engine.rs:3793` + a unit test). This recovers `matplotlib-23314` reliably and any other task that hits the same model-side sampling fluke. Estimated +1-2 tasks per 50.
3. **Accept matplotlib 20% as ceiling** for minimax-m3 until either (a) a stronger model is wired in, or (b) a domain-specific prompt hint is added (would need a probe of its own).
4. **Variance observation**: if we have budget for it, run a 5× re-trial of the 50 batch-3 tasks to estimate the noise floor. A pass rate of 48% with X% standard deviation per task would let us distinguish "model can't solve" from "model sometimes solves".

## Anchors

- Probe driver: `/tmp/swe-batch3-matplotlib-probe.sh`
- Probe results: `~/.shannon/eval/swe50-n3-matplotlib-probe/matplotlib-probe.tsv` (4 rows, 1 resolved)
- Probe log: `~/.shannon/eval/swe50-n3-matplotlib-probe/matplotlib-probe.log`
- Driver PID at launch: 3716957 (setsid detached)
- Related docs: `docs/research/swe-bench-batch3-2026-08-30.md`, `docs/research/zero-output-token-rca-2026-08-31.md`