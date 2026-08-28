#!/usr/bin/env bash
# run-batch.sh — serial n=k batch driver over Shannon's external benchmark
# suites (§4.13). Composes the existing CLI runners; adds nothing to scoring.
#
#   ┌ round 1 ─────────────────────────────────────────────────┐
#   │ eval_runner  (regression)      → <out>/<run-id>/report.* │
#   │ bench_runner  (terminal_bench, swebench_verified_50)      │
#   └ token accounting → budget gate → next round or stop ─────┘
#
# Features mandated by the multi-benchmark rollout plan:
#   - n rounds, strictly serial, one independent run directory per round.
#   - Resume: completed rounds (state markers) are skipped on re-invocation.
#   - Cross-round token/cost ledger; hard stop when the budget is exhausted.
#   - Preflight conflict check: refuses to run while another eval runner or
#     headless engine is already running (pgrep).
#   - Rate-limit self-heal (regression): a round poisoned by provider 429s is
#     quarantined and retried after a 60 s cooldown, up to --max-round-retries.
#
# Aggregation is a separate, read-only step (see --aggregate):
#   eval_runner aggregate <out-root>   → stable/flaky buckets + intervals
#   bench_runner diff a.json b.json    → remote-suite cross-round diff
#
# Exit codes: 0 done · 2 usage/config · 3 budget exhausted (stopped early)
#             · 4 rate-limit retries exhausted (resume later)
set -u

# ── defaults ────────────────────────────────────────────────────────────────
SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SELF_DIR/../.." && pwd)"

SUITE=""
OUT=""
N_RUNS=3
BUDGET_TOKENS=15000000
BUDGET_SCOPE="io"          # io = tokens_in+tokens_out; all = + cache tokens
BIN="${SHANNON_EVAL_BIN:-}"
TASKS_DIR="$REPO_ROOT/tests/eval/benchmarks/regression"
EVAL_RUNNER="${SHANNON_EVAL_RUNNER:-$REPO_ROOT/target/debug/examples/eval_runner}"
BENCH_RUNNER="${SHANNON_BENCH_RUNNER:-$REPO_ROOT/target/debug/examples/bench_runner}"
MAX_ROUND_RETRIES=2
RETRY_COOLDOWN_SECS=60
DRY_RUN=0
FORCE=0
AGGREGATE=0
EXTRA_ARGS=()

usage() {
  cat <<'EOF'
Usage:
  run-batch.sh --suite regression|terminal_bench|swebench_verified_50 \
               --out DIR [--n 3] [--bin PATH] [--tasks DIR]
               [--budget-tokens N] [--budget-scope io|all]
               [--eval-runner PATH] [--bench-runner PATH]
               [--max-round-retries 2] [--retry-cooldown-secs 60]
               [--timeout SECS] [--dry-run] [--force] [--aggregate]
               [-- extra-args passed through to the runner]

Execution bypass (documented rollout convention): point --bin at the ALREADY
BUILT engine binary or provider wrapper in the main checkout, e.g.
  --bin /tmp/shannon-zhipu/shannon-glm-plan      (glm-5.3-flash @ coding-plan)
so this worktree never pays a full rebuild. Remote suites additionally honor
SHANNON_TB_TASKS_DIR / SHANNON_SWEBENCH_HOME / SHANNON_{TB,SB}_HARNESS_CMD,
which run-batch.sh forwards untouched to bench_runner.
EOF
}

die() { echo "run-batch: $*" >&2; exit 2; }
log() { echo "[run-batch] $*"; }

# ── arg parsing ─────────────────────────────────────────────────────────────
while [ $# -gt 0 ]; do
  case "$1" in
    --suite) SUITE="${2:?}"; shift 2 ;;
    --out) OUT="${2:?}"; shift 2 ;;
    --n) N_RUNS="${2:?}"; shift 2 ;;
    --bin) BIN="${2:?}"; shift 2 ;;
    --tasks) TASKS_DIR="${2:?}"; shift 2 ;;
    --budget-tokens) BUDGET_TOKENS="${2:?}"; shift 2 ;;
    --budget-scope) BUDGET_SCOPE="${2:?}"; shift 2 ;;
    --eval-runner) EVAL_RUNNER="${2:?}"; shift 2 ;;
    --bench-runner) BENCH_RUNNER="${2:?}"; shift 2 ;;
    --max-round-retries) MAX_ROUND_RETRIES="${2:?}"; shift 2 ;;
    --retry-cooldown-secs) RETRY_COOLDOWN_SECS="${2:?}"; shift 2 ;;
    --timeout) EXTRA_ARGS+=(--timeout "${2:?}"); shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    --force) FORCE=1; shift ;;
    --aggregate) AGGREGATE=1; shift ;;
    --) shift; EXTRA_ARGS+=("$@"); break ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown flag '$1' (see --help)" ;;
  esac
done

[ -n "$SUITE" ] || { usage >&2; die "--suite is required"; }
[ -n "$OUT" ] || { usage >&2; die "--out is required"; }
case "$SUITE" in
  regression|terminal_bench|swebench_verified_50) ;;
  *) die "unknown suite '$SUITE'" ;;
esac
case "$BUDGET_SCOPE" in io|all) ;; *) die "--budget-scope must be io|all" ;; esac
case "$N_RUNS" in ''|*[!0-9]*) die "--n must be a positive integer" ;; esac

if [ "$SUITE" = "regression" ]; then
  [ -f "$EVAL_RUNNER" ] || die "eval_runner not found at $EVAL_RUNNER (build it or pass --eval-runner)"
else
  [ -f "$BENCH_RUNNER" ] || die "bench_runner not found at $BENCH_RUNNER (build it or pass --bench-runner)"
fi
if [ "$SUITE" = "regression" ] && [ ! -d "$TASKS_DIR" ]; then
  die "regression tasks dir not found: $TASKS_DIR (pass --tasks)"
fi

# ── preflight: single-owner guarantee ───────────────────────────────────────
mkdir -p "$OUT" || die "cannot create out root $OUT"
STATE="$OUT/.batch-state"
mkdir -p "$STATE"

if ! mkdir "$STATE/.lock" 2>/dev/null; then
  [ "$FORCE" -eq 1 ] || die "another run-batch holds $STATE/.lock (use --force to override)"
  rm -rf "$STATE/.lock"; mkdir "$STATE/.lock"
fi
trap 'rmdir "$STATE/.lock" 2>/dev/null' EXIT

if [ "$FORCE" -ne 1 ]; then
  # Single-owner check with EXACT process-name matches only: substring
  # cmdline greps self-match every wrapper whose command text embeds this
  # script's own arguments. A real conflict is (a) a running eval_runner /
  # bench_runner binary, or (b) a shannon engine in headless --prompt mode
  # (an interactive TUI session does not block the batch).
  conflicts=""
  for p in $(pgrep -x eval_runner 2>/dev/null) $(pgrep -x bench_runner 2>/dev/null); do
    conflicts="$conflicts $p"
  done
  for p in $(pgrep -x shannon 2>/dev/null); do
    if tr '\0' '' < "/proc/$p/cmdline" 2>/dev/null | grep -q -- '--prompt'; then
      conflicts="$conflicts $p"
    fi
  done
  if [ -n "${conflicts// /}" ]; then
    for p in $conflicts; do ps -o pid=,args= -p "$p" 2>/dev/null; done
    die "another eval/engine process is running — settle it first (or --force)"
  fi
fi

# ── accounting ──────────────────────────────────────────────────────────────
# Sums one persisted report. Handles BOTH report shapes:
#   RunReport    (eval_runner): records[].metrics{tokens_in,tokens_out,...}
#   BenchReport  (bench_runner): records[].reps[].metrics + .external_metrics
summarize_report() { # summarize_report <report.json> -> "tokens cost passed total"
  python3 - "$1" "$BUDGET_SCOPE" <<'PYEOF'
import json, sys
path, scope = sys.argv[1], sys.argv[2]
with open(path, encoding="utf-8") as fh:
    data = json.load(fh)
tin = tout = cache = 0
cost = 0.0
passed = total = 0
def metrics_sum(metrics, ext=None):
    global tin, tout, cache, cost
    # metrics and ext are independent sources: pipeline reps carry `metrics`,
    # delegated reps carry only `external_metrics` (metrics is null) — the
    # early-return must not skip the ext branch (TB batch-2 ledger bug).
    if isinstance(metrics, dict):
        tin += int(metrics.get("tokens_in") or 0)
        tout += int(metrics.get("tokens_out") or 0)
        if scope == "all":
            cache += int(metrics.get("cache_creation_tokens") or 0)
            cache += int(metrics.get("cache_read_tokens") or 0)
        c = metrics.get("cost_usd")
        if isinstance(c, (int, float)):
            cost += c
    if isinstance(ext, dict):
        tin += int(ext.get("tokens_in") or 0)
        tout += int(ext.get("tokens_out") or 0)
        c = ext.get("cost_usd")
        if isinstance(c, (int, float)):
            cost += c
records = data.get("records") or []
for rec in records:
    total += 1
    if rec.get("passed") or rec.get("resolved_reps", 0) > 0:
        passed += 1
    m = rec.get("metrics")
    if rec.get("reps") is not None:            # BenchReport record
        for rep in rec["reps"]:
            metrics_sum(rep.get("metrics"), rep.get("external_metrics"))
    else:                                       # RunReport record
        metrics_sum(m)
if scope == "all":
    billed = tin + tout + cache
else:
    billed = tin + tout
print(f"{billed} {cost:.6f} {passed} {total} {tin} {tout} {cache}")
PYEOF
}

rate_limit_poisoned() { # rate_limit_poisoned <run-dir> -> 0 if poisoned
  # Poison signal is the engine's own L0 error event category ("rate_limit",
  # ErrorPayload.category in session_event.rs) — deliberately NOT a loose
  # "429" text grep, task prompts legitimately contain such literals.
  grep -rlE '"category": ?"rate_limit"' \
    "$1"/*/l0-home/sessions/*/events.jsonl 2>/dev/null \
    | head -1 | grep -q .
}

record_round() { # record_round <i> <run-dir>
  local i="$1" rundir="$2" report
  report="$rundir/report.json"
  [ -f "$report" ] || report="$rundir/bench-report.json"
  [ -f "$report" ] || die "no report.json/bench-report.json in $RUN_DIR — round artifact incomplete"
  local sums
  sums="$(summarize_report "$report")"
  python3 - "$STATE/round-$i.done" "$rundir" "$sums" <<'PYEOF'
import json, sys, os
marker, rundir, sums = sys.argv[1], sys.argv[2], sys.argv[3].split()
billed, cost, passed, total, tin, tout, cache = sums[0], float(sums[1]), sums[2], sums[3], sums[4], sums[5], sums[6]
json.dump({
    "round": int(os.path.basename(marker).removeprefix("round-").removesuffix(".done")),
    "run_dir": rundir,
    "tokens_in": int(tin), "tokens_out": int(tout), "cache_tokens": int(cache),
    "tokens_billed": int(billed), "cost_usd": cost,
    "passed": int(passed), "total": int(total),
}, open(marker, "w", encoding="utf-8"), indent=2)
PYEOF
}

cumulative_tokens() {
  local total=0 f b
  for f in "$STATE"/round-*.done; do
    [ -f "$f" ] || continue
    b="$(python3 -c "import json,sys;print(json.load(open(sys.argv[1]))['tokens_billed'])" "$f")"
    total=$((total + b))
  done
  echo "$total"
}

# ── round execution ─────────────────────────────────────────────────────────
run_one_round() { # echoes the produced run dir on stdout-log; sets RUN_DIR
  local mode_flag="" args=()
  [ "$DRY_RUN" -eq 1 ] || mode_flag="--real"

  if [ "$SUITE" = "regression" ]; then
    log "round: eval_runner --tasks $TASKS_DIR $mode_flag --bin $BIN --out $OUT"
    "$EVAL_RUNNER" --tasks "$TASKS_DIR" $mode_flag --bin "$BIN" --out "$OUT" \
      "${EXTRA_ARGS[@]}" 2>&1 | tee "$STATE/round-$ROUND.log"
    RUN_DIR="$(sed -n 's/^\[eval\] run directory: //p' "$STATE/round-$ROUND.log" | tail -1)"
  else
    log "round: bench_runner --suite $SUITE --n 1 $mode_flag --out $OUT"
    "$BENCH_RUNNER" --suite "$SUITE" --n 1 $mode_flag --out "$OUT" \
      "${EXTRA_ARGS[@]}" 2>&1 | tee "$STATE/round-$ROUND.log"
    RUN_DIR="$(sed -n "s/^\\[bench\\/$SUITE\\] report: //p" "$STATE/round-$ROUND.log" | tail -1)"
  fi
  [ -n "$RUN_DIR" ] || RUN_DIR="$(ls -1d "$OUT"/*/ 2>/dev/null | grep -v '\.batch-state' | sort | tail -1 | sed 's:/*$::')"
}

QUARANTINE="$STATE/failed-rounds"
mkdir -p "$QUARANTINE"

for ROUND in $(seq 1 "$N_RUNS"); do
  MARKER="$STATE/round-$ROUND.done"
  if [ -f "$MARKER" ]; then
    log "round $ROUND already complete ($(cat "$MARKER" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(d["run_dir"])')) — skipped"
    continue
  fi

  retries=0
  while :; do
    log "=== round $ROUND/$N_RUNS (attempt $((retries + 1))) ==="
    RUN_DIR=""
    run_one_round
    [ -n "$RUN_DIR" ] && [ -d "$RUN_DIR" ] || die "round $ROUND produced no run directory (see $STATE/round-$ROUND.log)"

    if [ "$SUITE" = "regression" ] && [ "$DRY_RUN" -ne 1 ] && rate_limit_poisoned "$RUN_DIR"; then
      if [ "$retries" -ge "$MAX_ROUND_RETRIES" ]; then
        log "round $ROUND still rate-limit-poisoned after $MAX_ROUND_RETRIES retries — stopping (state kept; resume later)"
        exit 4
      fi
      retries=$((retries + 1))
      log "provider rate limit detected in round $ROUND — quarantine + ${RETRY_COOLDOWN_SECS}s cooldown, then retry ($retries/$MAX_ROUND_RETRIES)"
      mv "$RUN_DIR" "$QUARANTINE/$(basename "$RUN_DIR")"
      sleep "$RETRY_COOLDOWN_SECS"
      continue
    fi

    record_round "$ROUND" "$RUN_DIR"
    log "round $ROUND done: $(python3 -c "import json;print(json.dumps(json.load(open('$MARKER'))))")"
    break
  done

  used="$(cumulative_tokens)"
  log "ledger: ${used}/${BUDGET_TOKENS} tokens billed so far"
  if [ "$used" -ge "$BUDGET_TOKENS" ]; then
    log "BUDGET EXHAUSTED after round $ROUND — stopping before round $((ROUND + 1))"
    touch "$STATE/BUDGET-EXHAUSTED"
    exit 3
  fi
done

# ── wrap-up ─────────────────────────────────────────────────────────────────
used="$(cumulative_tokens)"
log "all $N_RUNS rounds complete · ledger ${used}/${BUDGET_TOKENS} tokens"
if [ "$SUITE" = "regression" ]; then
  log "next: $EVAL_RUNNER aggregate $OUT"
else
  log "next: compare round reports with $BENCH_RUNNER diff <a.json> <b.json>"
fi

if [ "$AGGREGATE" -eq 1 ] && [ "$SUITE" = "regression" ]; then
  log "aggregating $OUT"
  "$EVAL_RUNNER" aggregate "$OUT"
fi
