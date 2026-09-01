#!/usr/bin/env bash
# SWE-bench batch driver — generalized template with --resume + RL retry.
#
# Drives the SWE-bench harness across a task list with these features:
#   - Pre-flight wrapper pacing smoke test (scripts/eval/wrapper-pacing-smoke-test.sh)
#   - Pre-flight retry+resume smoke test (scripts/eval/driver-retry-resume-smoke-test.sh)
#   - Quota probe (one known-fast task; abort if it hits RL)
#   - SWE_MIN_DELAY_MS pacing via the wrapper
#   - --resume: skip tasks whose verdict.json already exists
#   - RL retry loop: on rate-limit, sleep RL_DELAY_SECS and retry up to
#     RL_MAX_RETRIES times before counting as final fail
#   - Retry annotation: appends "[RL-retry xN]" to verdict.json notes
#     AFTER the loop (so the annotation isn't overwritten by the success path)
#   - FAIL_LIST (default: /tmp/swe-batch.failures) lists exhausted-retry tasks
#
# Configuration via env vars (with defaults) OR CLI overrides:
#   OUT_BASE         (default: ~/.shannon/eval/swe50-af5078b6)
#   MODEL            (default: shannon:shannon-minimax-m3)
#   PACING_MS        (default: 30000)
#   RL_MAX_RETRIES   (default: 3)
#   RL_DELAY_SECS    (default: 300)
#   RESUME           (default: 0; --resume to set 1)
#   SKIP_PROBE       (default: 0; --skip-probe to set 1)
#   SKIP_SMOKE       (default: 0; --skip-smoke to set 1)
#   SKIP_RETRY_SMOKE (default: 0)
#
# Tasks come from the first column of $SHANNON_HOME/eval/swe50-n3/results.tsv
# (the original 50-task SWE-bench Verified subset). To run a different task
# list, override TASK_LIST_FILE.
#
# Output:
#   $OUT_BASE/results.tsv        (T2.2 attribution schema)
#   $OUT_BASE/<batch>.log        (per-batch log; name from LOG_NAME env)
#   $OUT_BASE/per-task/rep-0-id/
#   /tmp/swe-batch.done          (success marker)
#   /tmp/swe-batch.failures      (exhausted-retry tasks)
#
# Examples:
#   # fresh run with defaults
#   ./swe-batch-driver.sh
#
#   # resume a killed batch
#   RESUME=1 ./swe-batch-driver.sh
#
#   # custom batch-9 with shorter pacing for testing
#   OUT_BASE=~/.shannon/eval/swe50-batch9 PACING_MS=5000 \
#     MODEL='shannon:shannon-minimax-m3-batch9' \
#     LOG_NAME=batch9.log \
#     ./swe-batch-driver.sh
set -u

# Default configuration (CLI/env takes precedence)
OUT_BASE="${OUT_BASE:-~/.shannon/eval/swe50-af5078b6}"
PER_TASK="$OUT_BASE/per-task"
LOG_NAME="${LOG_NAME:-batch.log}"
LOG="$OUT_BASE/$LOG_NAME"
EXIT_MARKER="/tmp/swe-batch.done"
FAIL_LIST="/tmp/swe-batch.failures"
mkdir -p "$PER_TASK"
: > "$FAIL_LIST"
exec 2> >(tee -a "$LOG" >&2)

HARNESS=/home/ed/workspace/app/work/shannon/shannon-mono/scripts/eval/swe-harness.sh
COLLECT=/home/ed/workspace/app/work/shannon/shannon-mono/scripts/eval/swe-collect-verdicts.sh
WRAPPER=/home/ed/workspace/app/work/shannon/shannon-mono/scripts/eval/wrapper-minimax.sh
SMOKE=/home/ed/workspace/app/work/shannon/shannon-mono/scripts/eval/wrapper-pacing-smoke-test.sh
RETRY_SMOKE=/home/ed/workspace/app/work/shannon/shannon-mono/scripts/eval/driver-retry-resume-smoke-test.sh
MODEL="${MODEL:-shannon:shannon-minimax-m3}"
PACING_MS="${PACING_MS:-30000}"
RL_MAX_RETRIES="${RL_MAX_RETRIES:-3}"
RL_DELAY_SECS="${RL_DELAY_SECS:-300}"
RESUME="${RESUME:-0}"
SKIP_PROBE="${SKIP_PROBE:-0}"
SKIP_SMOKE="${SKIP_SMOKE:-0}"
SKIP_RETRY_SMOKE="${SKIP_RETRY_SMOKE:-0}"
TASK_LIST_FILE="${TASK_LIST_FILE:-}"

# CLI overrides
while [ $# -gt 0 ]; do
  case "$1" in
    --resume) RESUME=1 ;;
    --no-resume) RESUME=0 ;;
    --skip-probe) SKIP_PROBE=1 ;;
    --skip-smoke) SKIP_SMOKE=1 ;;
    --rl-retries) RL_MAX_RETRIES="$2"; shift ;;
    --rl-delay) RL_DELAY_SECS="$2"; shift ;;
    --pacing-ms) PACING_MS="$2"; shift ;;
    --out-base) OUT_BASE="$2"; PER_TASK="$OUT_BASE/per-task"; shift ;;
    --model) MODEL="$2"; shift ;;
    --task-list) TASK_LIST_FILE="$2"; shift ;;
    -h|--help)
      sed -n '2,49p' "$0" | sed 's/^# //; s/^#//'
      exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
  shift
done

# Expand OUT_BASE tilde (CLI override may have ~)
case "$OUT_BASE" in
  "~") OUT_BASE="$HOME" ;;
  "~/"*) OUT_BASE="$HOME/${OUT_BASE#~/}" ;;
esac
PER_TASK="$OUT_BASE/per-task"
LOG="$OUT_BASE/$LOG_NAME"

echo "[batch] OUT_BASE=$OUT_BASE  MODEL=$MODEL  PACING_MS=$PACING_MS"
echo "[batch] RESUME=$RESUME  RL_MAX_RETRIES=$RL_MAX_RETRIES  RL_DELAY_SECS=$RL_DELAY_SECS  SKIP_PROBE=$SKIP_PROBE"

# Pre-flight 1: wrapper pacing smoke test
if [ "$SKIP_SMOKE" != "1" ]; then
  echo
  echo "[batch] pre-flight wrapper pacing smoke test:"
  if ! bash "$SMOKE" >/dev/null 2>&1; then
    echo "FATAL: wrapper pacing smoke test failed — aborting" >&2
    exit 8
  fi
  echo "[batch]   wrapper pacing: 8/8 PASS"
fi

# Pre-flight 2: retry+resume smoke test
if [ "$SKIP_RETRY_SMOKE" != "1" ] && [ -x "$RETRY_SMOKE" ]; then
  echo
  echo "[batch] pre-flight retry+resume smoke test:"
  if ! bash "$RETRY_SMOKE" >/dev/null 2>&1; then
    echo "FATAL: retry+resume smoke test failed — aborting" >&2
    exit 8
  fi
  echo "[batch]   retry+resume: PASS"
fi

# Pre-flight 3: binary mtime vs wrapper mtime
BIN=~/workspace/app/work/shannon/shannon-mono/target/debug/shannon
BIN_MTIME=$(stat -c %Y "$BIN" 2>/dev/null || echo 0)
WRAPPER_MTIME=$(stat -c %Y "$WRAPPER" 2>/dev/null || echo 0)
if [ "$BIN_MTIME" -lt "$WRAPPER_MTIME" ]; then
  echo "WARN: binary older than wrapper. Rebuild recommended." >&2
fi

# Pre-flight 4: credentials
KEY_FILE=~/.shannon/credentials/minimax.json
[ -r "$KEY_FILE" ] || { echo "FATAL: $KEY_FILE missing" >&2; exit 9; }
echo "[batch]   credentials: $KEY_FILE readable"

# Pre-flight 5: quota probe (skip if --resume or --skip-probe)
if [ "$RESUME" != "1" ] && [ "$SKIP_PROBE" != "1" ]; then
  PROBE_TASK=django__django-10097
  PROBE_DIR="$PER_TASK/probe"
  rm -rf "$PROBE_DIR"; mkdir -p "$PROBE_DIR"
  cd "$PROBE_DIR"
  echo
  echo "[batch] pre-flight quota probe: $PROBE_TASK"
  START_TS=$(date +%s)
  SWE_PACING_RESET=1 SHANNON_BENCH_VERDICT_FILE="$PWD/verdict.json" \
    SHANNON_SB_AGENT_BIN="$WRAPPER" \
    SWE_AGENT_MAX_TURNS=30 \
    SWE_MODEL_NAME="$MODEL" \
    SWE_MIN_DELAY_MS="$PACING_MS" \
      timeout 600 "$HARNESS" "$PROBE_TASK" > "$PROBE_DIR/harness.log" 2>&1
  PROBE_RC=$?
  END_TS=$(date +%s)
  PROBE_VERDICT="$(python3 -c "import json;print(json.load(open('$PROBE_DIR/verdict.json')).get('resolved','?'))" 2>/dev/null || echo 'NO-VERDICT')"
  PROBE_RL="$(grep -liE 'rate limit|rate.limit' "$PROBE_DIR/agent-err.log" 2>/dev/null && echo 'YES' || echo 'no')"
  echo "[batch]   probe verdict=$PROBE_VERDICT, RL=$PROBE_RL, elapsed=$((END_TS-START_TS))s, rc=$PROBE_RC"
  if [ "$PROBE_RL" = "YES" ]; then
    echo "FATAL: quota probe hit rate-limit. Quota exhausted; aborting." >&2
    date -Iseconds > "$EXIT_MARKER.failed-probe"
    exit 7
  fi
  echo "[batch]   quota probe passed"
fi

# Task list resolution
TASK_LIST="$OUT_BASE/.task-list.tsv"
if [ -n "$TASK_LIST_FILE" ]; then
  cp "$TASK_LIST_FILE" "$TASK_LIST"
elif [ -f "$HOME/.shannon/eval/swe50-n3/results.tsv" ]; then
  cut -f1 "$HOME/.shannon/eval/swe50-n3/results.tsv" | tail -n +2 > "$TASK_LIST"
else
  echo "FATAL: no task list. Set TASK_LIST_FILE or ensure swe50-n3 exists." >&2
  exit 6
fi
TOTAL_TASKS="$(wc -l < "$TASK_LIST")"
echo "[batch] task list: $TOTAL_TASKS tasks"

# Helper: detect rate-limit in agent-err.log
is_rate_limited() {
  local d="$1"
  [ -f "$d/agent-err.log" ] || return 1
  grep -qiE 'rate limit|rate.limit' "$d/agent-err.log" 2>/dev/null
}

# Helper: append a retry annotation to verdict.json notes field
annotate_retry() {
  local v="$1" attempt="$2"
  python3 - "$v" "$attempt" <<'PY'
import json, sys
p, attempt = sys.argv[1], int(sys.argv[2])
try:
    d = json.load(open(p))
except Exception:
    sys.exit(0)
existing = d.get('notes', '')
d['notes'] = f"{existing} [RL-retry x{attempt}]" if existing else f"[RL-retry x{attempt}]"
json.dump(d, open(p, 'w'))
PY
}

# Main rep runner with RL retry loop + --resume skip
run_rep() {
  local native_id="$1" ridx="$2"
  local rep_dir="$PER_TASK/rep-${ridx}-${native_id//\//_}"

  # --resume: skip if verdict.json already exists
  if [ "$RESUME" = "1" ] && [ -f "$rep_dir/verdict.json" ]; then
    local SKIP_VERDICT=$(python3 -c "import json;print(json.load(open('$rep_dir/verdict.json'))['resolved'])" 2>/dev/null || echo '?')
    echo "[batch]   SKIP $native_id (verdict=$SKIP_VERDICT, already done)"
    return 0
  fi

  rm -rf "$rep_dir"
  mkdir -p "$rep_dir"
  cd "$rep_dir"
  echo "[batch] === rep $ridx: $native_id ===" >&2

  local attempt=0
  while [ "$attempt" -le "$RL_MAX_RETRIES" ]; do
    if [ "$attempt" -gt 0 ]; then
      echo "[batch]   RETRY $native_id after ${RL_DELAY_SECS}s sleep (attempt $attempt/$RL_MAX_RETRIES)" >&2
      sleep "$RL_DELAY_SECS"
    fi

    SHANNON_BENCH_VERDICT_FILE="$PWD/verdict.json" \
    SHANNON_SB_AGENT_BIN="$WRAPPER" \
    SWE_AGENT_MAX_TURNS=30 \
    SWE_MODEL_NAME="$MODEL" \
    SWE_MIN_DELAY_MS="$PACING_MS" \
      timeout 1800 "$HARNESS" "$native_id" > "$rep_dir/harness.log" 2>&1
    local rc=$?

    if is_rate_limited "$PWD"; then
      attempt=$((attempt + 1))
      echo "[batch]   $native_id: RL detected (attempt $attempt/$((RL_MAX_RETRIES+1)))" >&2
      continue
    fi

    # Not RL — final result. Annotate retries if any happened.
    if [ "$attempt" -gt 0 ]; then
      annotate_retry "$PWD/verdict.json" "$attempt"
    fi
    if [ -f "$PWD/verdict.json" ]; then
      local V=$(python3 -c "import json;print(json.load(open('$PWD/verdict.json'))['resolved'])")
      echo "[batch]   $native_id: verdict=$V (rc=$rc)"
    else
      echo "[batch]   $native_id: NO VERDICT (rc=$rc, harness.log may explain)"
      echo "$native_id" >> "$FAIL_LIST"
    fi
    return 0
  done

  # Exhausted retries — annotate last verdict and mark fail
  annotate_retry "$PWD/verdict.json" "$attempt"
  echo "[batch]   $native_id: RL persists after $((RL_MAX_RETRIES+1)) attempts — recording as fail"
  echo "$native_id" >> "$FAIL_LIST"
  return 0
}

# Main loop
SKIPPED=0
DONE_REPS=0
while read -r native_id; do
  local_dir="$PER_TASK/rep-0-${native_id//\//_}"
  if [ "$RESUME" = "1" ] && [ -f "$local_dir/verdict.json" ]; then
    SKIPPED=$((SKIPPED + 1))
    continue
  fi
  run_rep "$native_id" 0
  DONE_REPS=$((DONE_REPS + 1))
  echo "[batch] progress: $DONE_REPS new + $SKIPPED skipped = $((DONE_REPS + SKIPPED))/$TOTAL_TASKS"
done < "$TASK_LIST"

# Stitch verdicts
bash "$COLLECT" "$PER_TASK" > "$OUT_BASE/results.tsv"
ROWS="$(tail -n +2 "$OUT_BASE/results.tsv" | wc -l)"
echo "[batch] TSV: $ROWS rows → $OUT_BASE/results.tsv"

# Gates
echo
echo "[batch] API-rejection gate (should be 0):"
REJ_COUNT="$(grep -liE 'unknown model|invalid params' "$PER_TASK"/rep-*/agent-err.log 2>/dev/null | wc -l)"
echo "  rejected reps: $REJ_COUNT / $ROWS"
echo "[batch] rate-limit gate (with retry should be very low):"
RL_COUNT="$(grep -liE 'rate limit|rate.limit' "$PER_TASK"/rep-*/agent-err.log 2>/dev/null | wc -l)"
echo "  rate-limited reps: $RL_COUNT / $ROWS"

# Per-repo breakdown (excludes probe/)
echo
echo "[batch] pass rate by repo:"
python3 - "$OUT_BASE/results.tsv" "$PER_TASK" <<'PYEOF'
import sys, os, collections
tsv, per_task_dir = sys.argv[1], sys.argv[2]
def load_gate(d):
    ael = os.path.join(d, 'agent-err.log')
    if not os.path.exists(ael): return False, False
    txt = open(ael).read().lower()
    return ('unknown model' in txt or 'invalid params' in txt), ('rate limit' in txt)
with open(tsv) as fh:
    header = fh.readline().strip().split("\t")
    rows = [dict(zip(header, line.strip().split("\t"))) for line in fh if line.strip()]
by_repo = collections.defaultdict(lambda: [0, 0, 0, 0, 0])
for row in rows:
    if row['task'].startswith('probe'): continue
    repo = row['task'].split('__', 1)[0]
    d = os.path.join(per_task_dir, 'rep-0-' + row['task'])
    rejected, rl = load_gate(d)
    by_repo[repo][1] += 1
    if row['resolved'] == 'True':
        by_repo[repo][0] += 1
    if rejected: by_repo[repo][3] += 1
    elif rl: by_repo[repo][4] += 1
    elif row['resolved'] == 'True':
        by_repo[repo][2] += 1
print(f"  {'repo':<22}{'overall':>12}{'clean':>12}{'rej':>6}{'RL':>6}")
for repo in sorted(by_repo):
    p, t, pc, rj, rl = by_repo[repo]
    ct = t - rj - rl
    overall = f"{p}/{t} ({p*100/max(t,1):>4.0f}%)"
    clean = f"{pc}/{ct}" if ct else "n/a"
    print(f"  {repo:<22}{overall:>12}{clean:>12}{rj:>6}{rl:>6}")
overall_p = sum(p for p, _, _, _, _ in by_repo.values())
overall_t = sum(t for _, t, _, _, _ in by_repo.values())
overall_rj = sum(rj for _, _, _, rj, _ in by_repo.values())
overall_rl = sum(rl for _, _, _, _, rl in by_repo.values())
overall_clean = overall_t - overall_rj - overall_rl
overall_clean_pass = sum(pc for _, _, pc, _, _ in by_repo.values())
print()
print(f"  {'OVERALL':<22}{overall_p}/{overall_t} ({overall_p*100/max(overall_t,1):>4.0f}%)  clean: {overall_clean_pass}/{overall_clean}  rej: {overall_rj}  RL: {overall_rl}")
PYEOF

echo
echo "[batch] DONE: $DONE_REPS new, $SKIPPED skipped, $ROWS total"
date -Iseconds > "$EXIT_MARKER"
echo "[batch] exit marker: $EXIT_MARKER"
[ -s "$FAIL_LIST" ] && echo "[batch] FAILURES (exhausted retries):" && cat "$FAIL_LIST"

# Failure-mode classifier (always runs — failure modes are actionable signal)
if [ -x "scripts/eval/swe-classify-failures.py" ]; then
  echo
  echo "[batch] failure-mode classification:"
  python3 scripts/eval/swe-classify-failures.py "$PER_TASK" 2>&1 | tail -25
fi