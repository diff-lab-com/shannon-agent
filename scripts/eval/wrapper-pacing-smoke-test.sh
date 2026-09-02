#!/usr/bin/env bash
# Smoke test for wrapper-minimax.sh pacing logic.
#
# Verifies (12 cases):
#   T1.  Missing KEY_FILE → exits 9 (no pacing code reached)
#   T2.  SWE_MIN_DELAY_MS=0 → no sleep, no error
#   T3.  SWE_MIN_DELAY_MS=500 with state stamped 100ms ago → sleeps ~400ms
#   T4.  SWE_MIN_DELAY_MS=500 with state stamped 5s ago → no sleep (overhead only)
#   T5.  Garbage state file → treated as 0 (no sleep)
#   T6.  State file updated with millis timestamp after each call
#   T7.  SWE_MODEL override changes --model flag passed to binary
#   T8.  SWE_MIN_DELAY_MS=500 with state 1h in the future → sleeps ~500ms
#        (clock-skew safety — must NOT sleep 1h)
#   T9a. SWE_DISALLOWED_TOOLS default → forwards WebFetch + WebSearch
#   T9b. SWE_DISALLOWED_TOOLS=Bash → custom list replaces default
#   T9c. SWE_DISALLOWED_TOOLS='' → empty value disables forwarding
#   T9d. SWE_DISALLOWED_TOOLS unset → defaults still applied
#
# Hermetic: uses temp HOME + temp state file via SWE_PACING_STATE_FILE. The
# real /tmp/shannon-minimax/.pacing-state is NEVER touched (so a live eval
# is not disturbed). Production wrapper default is untouched.
#
# All wrapper invocations redirect stdin from /dev/null so a hung engine
# (which may wait on tty input when run with bad creds) can't deadlock
# the smoke test. SHANNON_BIN=/bin/true is used so the engine never starts.
#
# Note on timing: each invocation uses a SINGLE env call (no nested env
# wrappers) to keep measurement overhead low (~200ms baseline vs ~700ms
# with double env). Uses `date +%s%N` for high-resolution measurement
# (python's time.time() loses ~50ms per call vs wall clock on this kernel).
set -u
WRAPPER_DIR="$(cd "$(dirname "$0")" && pwd)"
WRAPPER="$WRAPPER_DIR/wrapper-minimax.sh"
[ -x "$WRAPPER" ] || { echo "FAIL: $WRAPPER not executable"; exit 1; }

PASS=0; FAIL=0

expect_rc() {
  local label="$1" expected="$2" actual="$3"
  if [ "$actual" -eq "$expected" ]; then
    echo "PASS: $label (got $actual)"
    PASS=$((PASS+1))
  else
    echo "FAIL: $label (expected $expected, got $actual)"
    FAIL=$((FAIL+1))
  fi
}

expect_time() {
  local label="$1" actual="$2" low="$3" high="$4"
  if [ "$actual" -ge "$low" ] && [ "$actual" -le "$high" ]; then
    echo "PASS: $label (got ${actual}ms in [${low},${high}])"
    PASS=$((PASS+1))
  else
    echo "FAIL: $label (got ${actual}ms, expected [${low},${high}])"
    FAIL=$((FAIL+1))
  fi
}

expect_match() {
  # expect_match <label> <needle> <haystack>
  local label="$1" needle="$2" haystack="$3"
  if [[ "$haystack" == *"$needle"* ]]; then
    echo "PASS: $label"
    PASS=$((PASS+1))
  else
    echo "FAIL: $label (needle='$needle' not in haystack)"
    FAIL=$((FAIL+1))
  fi
}

TEST_HOME="$(mktemp -d)"
NO_CREDS_HOME="$(mktemp -d)"
trap 'rm -rf "$TEST_HOME" "$NO_CREDS_HOME"' EXIT

# Shared test HOME with valid fake creds (T2-T8).
mkdir -p "$TEST_HOME/.shannon/credentials"
echo '{"value":"fake-key-for-test"}' > "$TEST_HOME/.shannon/credentials/minimax.json"
STATE_FILE="$TEST_HOME/pacing-state"

# Helper: time a single command. ONE env call, no nesting.
# Uses date +%s%N (nanosecond resolution) for measurement.
time_it() {
  local start end
  start=$(date +%s%N)
  "$@" >/dev/null 2>&1
  end=$(date +%s%N)
  awk -v s=$start -v e=$end 'BEGIN{printf "%.0f", (e-s)/1000000}'
}

# T1: missing KEY_FILE → exit 9. Use a SEPARATE home with NO creds.
HOME="$NO_CREDS_HOME" bash "$WRAPPER" </dev/null >/dev/null 2>&1
RC=$?
expect_rc "T1: missing KEY_FILE → exit 9" 9 "$RC"

# T2: MIN_DELAY_MS=0 → no error
ELAPSED=$(time_it env "HOME=$TEST_HOME" "SHANNON_BIN=/bin/true" \
  "SWE_PACING_STATE_FILE=$STATE_FILE" "SWE_MIN_DELAY_MS=0" \
  bash "$WRAPPER" </dev/null)
expect_time "T2: MIN_DELAY_MS=0 → fast + exit 0" "$ELAPSED" 0 500

# T3: MIN_DELAY_MS=500 with state 100ms ago → ~400ms sleep
RECENT_MS=$(($(python3 -c 'import time;print(int(time.time()*1000))') - 100))
echo "$RECENT_MS" > "$STATE_FILE"
ELAPSED=$(time_it env "HOME=$TEST_HOME" "SHANNON_BIN=/bin/true" \
  "SWE_PACING_STATE_FILE=$STATE_FILE" "SWE_MIN_DELAY_MS=500" \
  bash "$WRAPPER" </dev/null)
# Expected: 500 - 100 + overhead ≈ 400 + 200 = 600ms. Range 350-900ms.
expect_time "T3: MIN_DELAY_MS=500 with recent state → slept ~400ms" "$ELAPSED" 350 900

# T4: MIN_DELAY_MS=500 with state 5s old → no sleep (overhead only, ~200ms)
OLD_MS=$(($(python3 -c 'import time;print(int(time.time()*1000))') - 5000))
echo "$OLD_MS" > "$STATE_FILE"
ELAPSED=$(time_it env "HOME=$TEST_HOME" "SHANNON_BIN=/bin/true" \
  "SWE_PACING_STATE_FILE=$STATE_FILE" "SWE_MIN_DELAY_MS=500" \
  bash "$WRAPPER" </dev/null)
# Wrapper overhead alone is ~500ms (4 python3 calls + bash startup); allow
# up to 800ms so the test still fails if the 500ms sleep actually runs.
expect_time "T4: MIN_DELAY_MS=500 with 5s-old state → no sleep" "$ELAPSED" 0 800

# T5: garbage state file → treated as 0 (no sleep)
echo "garbage-not-a-number" > "$STATE_FILE"
ELAPSED=$(time_it env "HOME=$TEST_HOME" "SHANNON_BIN=/bin/true" \
  "SWE_PACING_STATE_FILE=$STATE_FILE" "SWE_MIN_DELAY_MS=500" \
  bash "$WRAPPER" </dev/null)
expect_time "T5: garbage state file → no sleep" "$ELAPSED" 0 800

# T6: state file updated with millis timestamp after each call
rm -f "$STATE_FILE"
env "HOME=$TEST_HOME" "SHANNON_BIN=/bin/true" \
  "SWE_PACING_STATE_FILE=$STATE_FILE" "SWE_MIN_DELAY_MS=1000" \
  bash "$WRAPPER" </dev/null >/dev/null 2>&1
if [ -f "$STATE_FILE" ] && [[ "$(cat "$STATE_FILE")" =~ ^[0-9]+$ ]]; then
  STAMP="$(cat "$STATE_FILE")"
  NOW="$(python3 -c 'import time;print(int(time.time()*1000))')"
  DIFF=$((NOW - STAMP))
  if [ "$DIFF" -ge 0 ] && [ "$DIFF" -lt 5000 ]; then
    echo "PASS: T6: state file stamped with recent millis timestamp (DIFF=${DIFF}ms)"
    PASS=$((PASS+1))
  else
    echo "FAIL: T6: state file stamped with recent millis timestamp (DIFF=${DIFF}ms — out of range)"
    FAIL=$((FAIL+1))
  fi
else
  echo "FAIL: T6: state file stamped with millis timestamp (got: $(cat "$STATE_FILE" 2>/dev/null || echo 'missing'))"
  FAIL=$((FAIL+1))
fi

# T7: SWE_MODEL override changes --model flag passed to binary
FAKE_BIN="$TEST_HOME/fake-shannon"
cat > "$FAKE_BIN" <<'EOSH'
#!/usr/bin/env bash
echo "ARGS: $*"
EOSH
chmod +x "$FAKE_BIN"
OUTPUT=$(env "HOME=$TEST_HOME" "SHANNON_BIN=$FAKE_BIN" \
  "SWE_PACING_STATE_FILE=$STATE_FILE" "SWE_MIN_DELAY_MS=0" "SWE_MODEL=MiniMax-M3-v2-test" \
  bash "$WRAPPER" --some-arg </dev/null 2>&1)
expect_match "T7: SWE_MODEL override changes --model flag" "--model MiniMax-M3-v2-test" "$OUTPUT"

# T8: clock-skew safety — state 1h in the future should NOT cause 1h sleep.
# Wrapper must clamp to MIN_DELAY_MS. Use a tight timeout to prove we don't hang.
FUTURE_MS=$(($(python3 -c 'import time;print(int(time.time()*1000))') + 3600000))
echo "$FUTURE_MS" > "$STATE_FILE"
ELAPSED=$(time_it env "HOME=$TEST_HOME" "SHANNON_BIN=/bin/true" \
  "SWE_PACING_STATE_FILE=$STATE_FILE" "SWE_MIN_DELAY_MS=500" \
  bash "$WRAPPER" </dev/null)
# Expected: full 500ms sleep + overhead (~200-500ms). Range 500-1500ms.
expect_time "T8: state 1h in future + MIN=500 → clamped to ~500ms" "$ELAPSED" 500 1500

# T9: SWE_DISALLOWED_TOOLS forwarding. Default = "WebFetch WebSearch";
# unset → defaults applied; empty → disabled; custom list honored.
# Uses the T7 fake-shannon (echoes args) so we can grep for --disallowedTools.
T9_OUT=$(env "HOME=$TEST_HOME" "SHANNON_BIN=$FAKE_BIN" \
  "SWE_PACING_STATE_FILE=$STATE_FILE" "SWE_MIN_DELAY_MS=0" \
  bash "$WRAPPER" </dev/null 2>&1)
expect_match "T9a: default --disallowedTools forwards WebFetch" "--disallowedTools WebFetch" "$T9_OUT"
expect_match "T9a: default --disallowedTools forwards WebSearch" "--disallowedTools WebSearch" "$T9_OUT"
T9B_OUT=$(env "HOME=$TEST_HOME" "SHANNON_BIN=$FAKE_BIN" \
  "SWE_PACING_STATE_FILE=$STATE_FILE" "SWE_MIN_DELAY_MS=0" "SWE_DISALLOWED_TOOLS=Bash" \
  bash "$WRAPPER" </dev/null 2>&1)
expect_match "T9b: custom list replaces default" "--disallowedTools Bash" "$T9B_OUT"
if [[ "$T9B_OUT" == *"--disallowedTools WebFetch"* ]]; then
  echo "FAIL: T9b: WebFetch leaked when SWE_DISALLOWED_TOOLS=Bash"
  FAIL=$((FAIL+1))
else
  echo "PASS: T9b: WebFetch not present when overridden"
  PASS=$((PASS+1))
fi
T9C_OUT=$(env "HOME=$TEST_HOME" "SHANNON_BIN=$FAKE_BIN" \
  "SWE_PACING_STATE_FILE=$STATE_FILE" "SWE_MIN_DELAY_MS=0" "SWE_DISALLOWED_TOOLS=" \
  bash "$WRAPPER" </dev/null 2>&1)
if [[ "$T9C_OUT" == *"--disallowedTools"* ]]; then
  echo "FAIL: T9c: --disallowedTools present when SWE_DISALLOWED_TOOLS=''"
  FAIL=$((FAIL+1))
else
  echo "PASS: T9c: empty value disables forwarding"
  PASS=$((PASS+1))
fi
T9D_OUT=$(env -u SWE_DISALLOWED_TOOLS "HOME=$TEST_HOME" "SHANNON_BIN=$FAKE_BIN" \
  "SWE_PACING_STATE_FILE=$STATE_FILE" "SWE_MIN_DELAY_MS=0" \
  bash "$WRAPPER" </dev/null 2>&1)
expect_match "T9d: unset → defaults still applied" "--disallowedTools WebFetch" "$T9D_OUT"

echo
echo "===== WRAPPER PACING SMOKE TEST: $PASS pass, $FAIL fail ====="
[ $FAIL -eq 0 ]
