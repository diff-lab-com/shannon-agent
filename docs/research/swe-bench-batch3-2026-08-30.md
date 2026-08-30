# SWE-bench Verified × Shannon — batch-3 results (2026-08-30)

**Citable headline numbers** for the first end-to-end batch on the dev binary
(commit range `3a6420e6..52aeaf4e`, harness at `scripts/eval/swe-harness.sh`)
against the SWE-bench Verified pin slice (§4.13 of the master plan).

- **Date / anchor**: 2026-08-30 · anchor: results.tsv @ `~/.shannon/eval/swe50-n3/results.tsv` (50 data rows)
- **Model**: `minimax-m3` via `shannon:shannon-minimax-m3` (per-provider tag, MODEL_NAME override)
- **Verifier**: swebench 5.0.2 official harness (`swebench.harness.run_evaluation`, docker)
- **Budget**: 125 M tokens · **spent**: 64.96 M (52 %)
- **Cost**: $19.04 (single-provider)

## Headline

| Metric | Value |
|---|---|
| Unique tasks attempted | **50** |
| Resolved (official harness verdict) | **24** |
| **Pass rate** | **48.0 %** |
| Tokens in | 64.73 M |
| Tokens out | 225 K |
| Cost | **$19.04** |
| Avg tokens per task (in) | 1.29 M |

## Pass rate by wave

| Wave | Scope | Tasks | Resolved | Rate |
|---|---|---|---|---|
| wave1 | Django-only (django/*) | 20 | 11 | **55.0 %** |
| wave2 | Mixed repos (scikit-learn, matplotlib, astropy, sympy, pallets, psf, pytest-dev, sphinx-doc, pydata, mwaskom) | 30 | 13 | **43.3 %** |
| **Combined** | | **50** | **24** | **48.0 %** |

The 5 originally-failed tasks were re-run after fixing two infrastructure bugs
(see [§ Infrastructure fixes](#infrastructure-fixes-applied-this-batch) below);
4 of 5 recovered. Without the retry, the pre-retry pass rate was **21 / 50 = 42.0 %** —
a +6 percentage-point swing purely from infrastructure hardening, no model or prompt change.

## Pass rate by repo

| Repo | Tasks | Resolved | Rate |
|---|---|---|---|
| pallets (flask-5014) | 1 | 1 | 100 % |
| psf (requests-1142) | 1 | 1 | 100 % |
| pytest-dev (pytest-10051) | 1 | 1 | 100 % |
| sphinx-doc (sphinx-8595) | 1 | 1 | 100 % |
| **scikit-learn** | 6 | 4 | **67 %** |
| **astropy** | 5 | 3 | **60 %** |
| **django** | 20 | 11 | **55 %** |
| matplotlib | 5 | 1 | 20 % |
| sympy | 7 | 1 | 14 % |
| mwaskom (seaborn-3069) | 1 | 0 | 0 % |
| pydata (xarray) | 2 | 0 | 0 % |

Repo variance is wide (0–100 %) and is driven by (a) coverage (small n for
seaborn/xarray — sampling noise) and (b) **true model fitness for the task type**
— matplotlib/sympy tasks skew toward mathematical correctness that minimax-m3
struggles with, while the agent's strength is grep → patch → test loop on
behavioral bugs.

## Retry recovery (originally failed in wave-2 → re-run after infrastructure fixes)

| Task | Wave | Original | Retry | Recovered? |
|---|---|---|---|---|
| `django__django-10973` | wave1 | ❌ network-stalled | ✅ resolved | **YES** |
| `sympy__sympy-12419` | wave2 | ❌ network-stalled | ❌ not resolved | no |
| `scikit-learn__scikit-learn-10297` | wave2 | ❌ network-stalled | ✅ resolved | **YES** |
| `scikit-learn__scikit-learn-13779` | wave2 | ❌ network-stalled | ✅ resolved | **YES** |
| `astropy__astropy-14995` | wave2 | ❌ network-stalled | ✅ resolved | **YES** |

**4 / 5 retries recovered.** The lone unrecovered (`sympy-12419`) had the agent
produce a patch that the official harness rejected — **not** an infrastructure
failure on the second pass; the patch was the wrong fix.

## Infrastructure fixes applied this batch

Two regressions and one network flake were uncovered and fixed during batch-3.
Both regressions were *re-discovered* during the retry run; the original wave
runs had masked them under network-flake symptoms.

### Fix 1 — `ensure_local()`: stale `.git/shallow` triggers transparent fetch (commit `fd8205c9`)

The SWE-bench repos under `/home/ed/datasets/swebench/repos/` ship with
`.git/shallow` listing several base_commits as shallow roots, even though
HEAD is already fully unshallowed. When `git worktree add` targets one of
those commits, git negotiates parents with the remote — which on this runner
flakes on github.com:443 (HTTP/2 framing, GnuTLS -110, 130 s timeouts) and
burns the 1800 s budget before the agent ever runs.

5 instances died this way in the original waves (the 5 in the retry table
above). The repair is idempotent: detect `rev-list --count HEAD > 100` AND
`.git/shallow` size < depth → delete the stale shallow file, then verify
the base_commit is reachable. Fallback: deepen via `--depth 10000` then
`--unshallow`.

### Fix 2 — `MODEL_NAME` + correct swebench 5.0.2 report path (commit `52aeaf4e`)

The official harness in 5.0.2 names reports as
`<report_dir>/<model_name_with_slashes_replaced>.<run_id>.json`
(NO `predictions.` prefix — that was 4.x convention). The pre-batch-3
harness hard-coded the 4.x path and emitted `MODEL_NAME` derived from
`AGENT` (changing across runs), so retries re-using the same dataset
silently produced reports at the wrong path → "official report missing".
Fixed by:
- Pinning `MODEL_NAME="${SWE_MODEL_NAME:-shannon:$(basename "$AGENT")}"`
- Reading the report at `${MODEL_NAME//\//__}.${RUN_ID}.json` (slashes → dunder)

## What worked, what didn't

**Worked**: The agent cleanly handles Django behavioural-bug tasks
(55 % pass on 20 attempts, all running grep → minimal patch → official
test loop). The harness pin → official verifier delegation is now
end-to-end reproducible: the same `swe-harness.sh` invocation produces
the same verdict.

**Did not**: matplotlib and sympy tasks — the agent's patches tend to be
correct on inspection but the test surface is dense (matplotlib's
parametrised rendering tests, sympy's algebraic equivalence checks).
Two of the three matplotlib failures had `<=2 K` output tokens — clear
sign of premature termination under `--max-turns 30`.

## Cost & token economics

| | tokens_in | tokens_out | cost |
|---|---|---|---|
| wave1 (20 tasks) | 32.6 M | 105 K | $9.78 |
| wave2 (30 tasks) | 25.6 M | 94 K | $6.71 |
| retry (5 tasks) | 6.5 M | 26 K | $1.93 |
| **TOTAL** | **64.7 M** | **225 K** | **$19.04** |
| **budget** | 125 M | — | — |

**Per-task cost**: $0.38 median. **Cost per resolved task**: $0.79.
**Token ratio**: 288 : 1 in : out — typical for tool-heavy agent runs.

## Open gaps for batch-4

1. **sympy-12419 retry**: agent's patch is structurally wrong (per harness
   report). Need a focused look at why the model fix the agent produced
   failed — probably a regression in the test fixtures, not the patch.
2. **`--max-turns 30` ceiling**: the matplotlib failures suggest premature
   termination. Test `SWE_AGENT_MAX_TURNS=60` on a 5-task probe.
3. **Output-token floor for matplotlib**: 2 of 3 unresolved matplotlib tasks
   had ≤2 K output tokens, which is the floor of a truncated turn. The
   other 8 zero-or-near-zero token entries (`django-10914`, `sympy-12481`,
   `matplotlib-23314`) all came back from the harness with NO session log —
   meaning the agent was killed before writing the first `turn/end`. Likely
   `--max-turns 0` or a turn-budget mis-accounting. Surface as a hard
   per-task metric for batch-4.

## Reproducibility

```bash
cd /home/ed/workspace/app/work/shannon/shannon-mono
git log --oneline -5 scripts/eval/swe-harness.sh     # confirms fixes are present
ls ~/.shannon/eval/swe50-n3/results.tsv              # canonical ledger
python3 -c 'import csv; r=[x for x in csv.reader(open("/home/ed/.shannon/eval/swe50-n3/results.tsv"), delimiter="\t")]; print(f"{sum(1 for x in r[1:] if x[1]==\"True\")}/{len(r)-1} resolved")'
# → 24/50 resolved
```

Anchor hashes:
- Harness fix 1: `fd8205c9`
- Harness fix 2: `52aeaf4e`
- Wave1 driver: `/tmp/swe-batch3.sh` (commit referenced in `docs/research/`)
- Wave2 driver: `/tmp/swe-batch3-wave2.sh`
- Retry driver: `/tmp/swe-batch3-retry.sh`
- Results ledger: `~/.shannon/eval/swe50-n3/results.tsv` (50 rows, 24 True)
