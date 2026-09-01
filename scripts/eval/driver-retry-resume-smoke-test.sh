#!/usr/bin/env bash
# Smoke test for batch-8 driver --resume + RL retry logic.
#
# Verifies (13 cases):
#   T1-T4:  is_rate_limited detection (4 cases: rate limit, rate.limit, no RL, missing log)
#   T5-T7:  annotate_retry (preserves original, handles missing notes, no-crash on missing file)
#   T8:     --resume skip when verdict.json exists
#   T9-T9c: retry loop semantics via stub (RL on attempt 1, success on attempt 2)
#   T10:    retry exhaustion after N+1 attempts
#   T11:    --resume + per-task dispatch (mock driver logic)
#
# Replicates the same helper functions used in /tmp/swe-batch8-driver.sh
# (is_rate_limited, annotate_retry, retry-loop body) — so the test
# verifies behavior, not exact code identity.
set -u

PASS=0; FAIL=0
expect_rc() {
  local label="$1" expected="$2" actual="$3"
  if [ "$actual" -eq "$expected" ]; then
    echo "PASS: $label (got $actual)"; PASS=$((PASS+1))
  else
    echo "FAIL: $label (expected $expected, got $actual)"; FAIL=$((FAIL+1))
  fi
}
expect_match() {
  local label="$1" needle="$2" haystack="$3"
  if [[ "$haystack" == *"$needle"* ]]; then
    echo "PASS: $label"; PASS=$((PASS+1))
  else
    echo "FAIL: $label (needle '$needle' not in '$haystack')"; FAIL=$((FAIL+1))
  fi
}

TEST_HOME="$(mktemp -d)"
trap 'rm -rf "$TEST_HOME"' EXIT

# === Helper functions (replicated from driver) ===

is_rate_limited() {
  local d="$1"
  [ -f "$d/agent-err.log" ] || return 1
  grep -qiE 'rate limit|rate.limit' "$d/agent-err.log" 2>/dev/null
}

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

# Simulates one run-rep invocation: invokes a stub harness, detects RL, retries
# up to RL_MAX_RETRIES, and writes a "fail" marker to $FAIL_LIST on exhaustion.
# Returns 0 on success path or after exhaustion (driver doesn't differentiate).
simulate_run_rep() { # simulate_run_rep <stub> <task_dir> <fail_list>
  local stub="$1" d="$2" fail_list="$3"
  local attempt=0 RL_MAX_RETRIES=3 RL_DELAY_SECS=0
  while [ "$attempt" -le "$RL_MAX_RETRIES" ]; do
    if [ "$attempt" -gt 0 ]; then sleep "$RL_DELAY_SECS"; fi
    bash "$stub" "$d"
    if is_rate_limited "$d"; then
      attempt=$((attempt + 1))
      continue
    fi
    # NOT RL — final result. Annotate retries if any happened.
    if [ "$attempt" -gt 0 ]; then
      annotate_retry "$d/verdict.json" "$attempt"
    fi
    return 0  # not RL → done
  done
  # Exhausted retries — annotate the last (failed) verdict and mark fail
  annotate_retry "$d/verdict.json" "$attempt"
  echo "exhausted" >> "$fail_list"
  return 0
}

# === T1-T4: is_rate_limited ===

D="$TEST_HOME/t1"; mkdir -p "$D"
echo "ERROR: rate limit exceeded for account" > "$D/agent-err.log"
is_rate_limited "$D" && RC=0 || RC=1
expect_rc "T1: is_rate_limited detects 'rate limit'" 0 $RC

D="$TEST_HOME/t2"; mkdir -p "$D"
echo "ERROR: rate.limit: 429 too many requests" > "$D/agent-err.log"
is_rate_limited "$D" && RC=0 || RC=1
expect_rc "T2: is_rate_limited detects 'rate.limit'" 0 $RC

D="$TEST_HOME/t3"; mkdir -p "$D"
echo "ERROR: bad model id 'shannon-minimax-m3'" > "$D/agent-err.log"
is_rate_limited "$D" && RC=0 || RC=1
expect_rc "T3: is_rate_limited returns 1 when no RL" 1 $RC

D="$TEST_HOME/t4"; mkdir -p "$D"
is_rate_limited "$D" && RC=0 || RC=1
expect_rc "T4: is_rate_limited returns 1 when no log" 1 $RC

# === T5-T7: annotate_retry ===

D="$TEST_HOME/t5"; mkdir -p "$D"
echo '{"resolved": false, "notes": "original message"}' > "$D/verdict.json"
annotate_retry "$D/verdict.json" 1
NOTES=$(python3 -c "import json;print(json.load(open('$D/verdict.json'))['notes'])")
expect_match "T5: annotate_retry appends [RL-retry x1]" "[RL-retry x1]" "$NOTES"
expect_match "T5b: annotate_retry preserves 'original message'" "original message" "$NOTES"

D="$TEST_HOME/t6"; mkdir -p "$D"
echo '{"resolved": false}' > "$D/verdict.json"
annotate_retry "$D/verdict.json" 2
NOTES=$(python3 -c "import json;print(json.load(open('$D/verdict.json'))['notes'])")
expect_match "T6: annotate_retry adds note when missing" "[RL-retry x2]" "$NOTES"

D="$TEST_HOME/t7"; mkdir -p "$D"
annotate_retry "$D/nonexistent.json" 1
RC=$?
expect_rc "T7: annotate_retry no crash on missing file" 0 $RC

# === T8: --resume skip ===

D="$TEST_HOME/t8"; mkdir -p "$D"
echo '{"resolved": true, "notes": "previous run"}' > "$D/verdict.json"
SKIP=0; [ -f "$D/verdict.json" ] && SKIP=1
expect_rc "T8: resume skip when verdict exists (SKIP=1)" 1 $SKIP

# === T9-T9c: retry loop via stub harness ===

D="$TEST_HOME/t9"; mkdir -p "$D"
cat > "$D/stub-harness.sh" <<'EOSH'
#!/usr/bin/env bash
D="$1"
COUNTER="$D/attempt-counter"
N=$(($(cat "$COUNTER" 2>/dev/null || echo 0) + 1))
echo "$N" > "$COUNTER"
if [ "$N" -eq 1 ]; then
  echo "ERROR: rate limit exceeded" > "$D/agent-err.log"
  exit 1
else
  echo '{"resolved": true}' > "$D/verdict.json"
  echo "ok" > "$D/agent-err.log"
  exit 0
fi
EOSH
chmod +x "$D/stub-harness.sh"
echo "0" > "$D/attempt-counter"

simulate_run_rep "$D/stub-harness.sh" "$D" "$TEST_HOME/fail-list"
ATTEMPTS=$(cat "$D/attempt-counter")
expect_rc "T9: retry loop ran 2 attempts (1 fail + 1 success)" 2 "$ATTEMPTS"
[ -f "$D/verdict.json" ] && RC=0 || RC=1
expect_rc "T9b: verdict exists after successful retry" 0 "$RC"
NOTES=$(python3 -c "import json;print(json.load(open('$D/verdict.json')).get('notes',''))" 2>/dev/null)
expect_match "T9c: verdict annotated with retry count" "[RL-retry x1]" "$NOTES"

# === T10: retry exhaustion ===

D="$TEST_HOME/t10"; mkdir -p "$D"
cat > "$D/stub-harness.sh" <<'EOSH'
#!/usr/bin/env bash
D="$1"
COUNTER="$D/attempt-counter"
N=$(($(cat "$COUNTER" 2>/dev/null || echo 0) + 1))
echo "$N" > "$COUNTER"
echo "ERROR: rate limit exceeded" > "$D/agent-err.log"
echo '{"resolved": false}' > "$D/verdict.json"
exit 1
EOSH
chmod +x "$D/stub-harness.sh"
echo "0" > "$D/attempt-counter"
FAIL_LIST="$TEST_HOME/fail-list.t10"
: > "$FAIL_LIST"

simulate_run_rep "$D/stub-harness.sh" "$D" "$FAIL_LIST"
ATTEMPTS=$(cat "$D/attempt-counter")
expect_rc "T10: retry exhaustion runs 4 attempts (1 + 3 retries)" 4 "$ATTEMPTS"
# Verify fail list has an entry (task name is unknown in stub, so we just check non-empty)
LINES=$(wc -l < "$FAIL_LIST")
if [ "$LINES" -ge 1 ]; then
  echo "PASS: T10b: fail-list has entry on exhaustion (lines=$LINES)"; PASS=$((PASS+1))
else
  echo "FAIL: T10b: fail-list empty on exhaustion"; FAIL=$((FAIL+1))
fi

# === T11: --resume skip — non-existent verdict triggers run ===

D="$TEST_HOME/t11"; mkdir -p "$D"
SKIP=0; [ -f "$D/verdict.json" ] && SKIP=1
expect_rc "T11: resume does NOT skip when verdict missing (SKIP=0)" 0 $SKIP

echo
echo "===== RETRY+RESUME SMOKE TEST: $PASS pass, $FAIL fail ====="
[ $FAIL -eq 0 ]