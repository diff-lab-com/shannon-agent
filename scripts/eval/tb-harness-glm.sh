#!/usr/bin/env bash
# Terminal-Bench delegation harness (rebuilt 2026-09-05; the t15 original in
# /tmp/tb-probe was lost to tmp cleanup — this follows the runtime contract
# documented in scripts/eval/tb-prebake/README.md).
#
# Invoked by bench_runner via:
#   SHANNON_TB_HARNESS_CMD='<this script> --task-dir {task_dir} {native_id}'
#   SHANNON_BENCH_VERDICT_FILE=<path>   (written by us)
#
# Flow per rep (prebaked-image path, --no-build is mandatory or compose
# rebuilds from the task Dockerfile and silently erases the prebake):
#   1. docker compose up the task's own compose with the prebaked client image
#   2. docker cp: tests -> $TEST_DIR, run-tests.sh + shannon binary -> container
#   3. agent headless run on the task instruction (exit code + NDJSON captured)
#   4. the task's OWN run-tests.sh decides the verdict (pytest exit code)
#   5. verdict.json {resolved, tokens_in, tokens_out, notes} for bench_runner
#
# Provider anchor: zhipu-coding-plan / glm-5.3-flash via TB_PROVIDER/TB_MODEL
# (defaults match wrapper-glm.sh; keep them in sync with the batch anchor).
set -u

NATIVE_ID=""
TASK_DIR=""
while [ $# -gt 0 ]; do
  case "$1" in
    --task-dir) TASK_DIR="$2"; shift 2 ;;
    --*) echo "tb-harness-glm: unknown flag $1" >&2; exit 2 ;;
    *) NATIVE_ID="$1"; shift ;;
  esac
done
[ -n "$NATIVE_ID" ] || { echo "tb-harness-glm: native_id required" >&2; exit 2; }
[ -n "$TASK_DIR" ] || TASK_DIR="${SHANNON_TB_TASKS_DIR:-}/$NATIVE_ID"
[ -f "$TASK_DIR/task.yaml" ] || { echo "tb-harness-glm: $TASK_DIR/task.yaml missing" >&2; exit 3; }

VERDICT_FILE="${SHANNON_BENCH_VERDICT_FILE:?SHANNON_BENCH_VERDICT_FILE required}"
SHANNON_BIN="${TB_SHANNON_BIN:-/home/ed/workspace/app/work/shannon/shannon-mono/target/debug/shannon}"
[ -x "$SHANNON_BIN" ] || { echo "tb-harness-glm: engine binary $SHANNON_BIN not executable" >&2; exit 3; }

PROVIDER="${TB_PROVIDER:-zhipu-coding-plan}"
MODEL="${TB_MODEL:-glm-5.3-flash}"
MAX_TURNS="${TB_MAX_TURNS:-80}"
KEY_FILE="${TB_KEY_FILE:-$HOME/.shannon/credentials/zhipu.json}"
KEEP="${TB_KEEP_CONTAINER:-0}"

API_KEY="$(python3 -c "
import json,sys
print(json.JSONDecoder().raw_decode(open(sys.argv[1]).read())[0]['value'])
" "$KEY_FILE" 2>/dev/null)"
[ -n "$API_KEY" ] || { echo "tb-harness-glm: no credential in $KEY_FILE" >&2; exit 3; }

INSTRUCTION="$(awk '/^instruction: \|-?$/{flag=1;next} /^[A-Za-z_][A-Za-z0-9_-]*:/{flag=0} flag' "$TASK_DIR/task.yaml")"
[ -n "$INSTRUCTION" ] || { echo "tb-harness-glm: no instruction block in $TASK_DIR/task.yaml" >&2; exit 3; }
AGENT_TIMEOUT="$(awk '/^max_agent_timeout_sec:/{print $2; exit}' "$TASK_DIR/task.yaml")"
AGENT_TIMEOUT="${AGENT_TIMEOUT%%.*}"
AGENT_TIMEOUT="${AGENT_TIMEOUT:-900}"

WORK="$(mktemp -d /tmp/tb-harness-${NATIVE_ID}.XXXXXX)"
LOGS="$WORK/logs"; AGENT_LOGS="$WORK/agent-logs"; mkdir -p "$LOGS" "$AGENT_LOGS"
CONTAINER="shannon-tb-${NATIVE_ID}-$$"
TEST_DIR="/tests"

cleanup() {
  if [ "$KEEP" != "1" ]; then
    T_BENCH_TASK_DOCKER_CLIENT_IMAGE_NAME="$IMG" \
    T_BENCH_TASK_DOCKER_CLIENT_CONTAINER_NAME="$CONTAINER" \
    T_BENCH_TEST_DIR="$TEST_DIR" \
    T_BENCH_TASK_LOGS_PATH="$LOGS" T_BENCH_CONTAINER_LOGS_PATH="/logs" \
    T_BENCH_TASK_AGENT_LOGS_PATH="$AGENT_LOGS" T_BENCH_CONTAINER_AGENT_LOGS_PATH="/agent-logs" \
      docker compose -f "$TASK_DIR/docker-compose.yaml" down -v --remove-orphans >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

# Image source: prebaked -> base -> compose build (cold; warn loudly).
IMG="shannon-tb-prebake/prebaked:$NATIVE_ID"
docker image inspect "$IMG" >/dev/null 2>&1 || IMG="shannon-tb-prebake/base:$NATIVE_ID"
docker image inspect "$IMG" >/dev/null 2>&1 || IMG=""
IMG_NOTE="prebaked-or-base"
if [ -z "$IMG" ]; then
  IMG="shannon-tb-cold:$NATIVE_ID"
  IMG_NOTE="cold-build"
  echo "[tb-harness] WARN: no prebake for $NATIVE_ID — cold compose build" >&2
else
  IMG_NOTE="$IMG"
fi

export T_BENCH_TASK_DOCKER_CLIENT_IMAGE_NAME="$IMG"
export T_BENCH_TASK_DOCKER_CLIENT_CONTAINER_NAME="$CONTAINER"
export T_BENCH_TEST_DIR="$TEST_DIR"
export T_BENCH_TASK_LOGS_PATH="$LOGS" T_BENCH_CONTAINER_LOGS_PATH="/logs"
export T_BENCH_TASK_AGENT_LOGS_PATH="$AGENT_LOGS" T_BENCH_CONTAINER_AGENT_LOGS_PATH="/agent-logs"

UP_START=$(date +%s)
if ! docker compose -f "$TASK_DIR/docker-compose.yaml" up -d --no-build >/dev/null 2>&1; then
  printf '{"resolved": false, "notes": "compose up failed (image=%s)"}\n' "$IMG" > "$VERDICT_FILE"
  exit 0
fi
UP_SECS=$(( $(date +%s) - UP_START ))

# Provision: tests, verifier, engine.
docker cp "$TASK_DIR/tests" "$CONTAINER:$TEST_DIR" >/dev/null 2>&1 || \
  echo "[tb-harness] WARN: tests cp failed" >&2
docker cp "$TASK_DIR/run-tests.sh" "$CONTAINER:/app/run-tests.sh" >/dev/null
docker exec "$CONTAINER" chmod +x /app/run-tests.sh "$TEST_DIR"/*.sh >/dev/null 2>&1 || true
docker cp "$SHANNON_BIN" "$CONTAINER:/usr/local/bin/shannon" >/dev/null
docker exec "$CONTAINER" chmod +x /usr/local/bin/shannon >/dev/null

# Agent run (headless; NDJSON kept as forensic evidence).
printf '%s' "$INSTRUCTION" > "$WORK/instruction.txt"
docker cp "$WORK/instruction.txt" "$CONTAINER:/tmp/tb-instruction.txt" >/dev/null
AGENT_START=$(date +%s)
timeout "$AGENT_TIMEOUT" docker exec \
  -e SHANNON_API_KEY="$API_KEY" \
  -w /app "$CONTAINER" \
  sh -c "shannon --provider $PROVIDER --model $MODEL \
      --disallowed-tools WebFetch --disallowed-tools WebSearch \
      --output-format json-stream --max-turns $MAX_TURNS \
      -p \"\$(cat /tmp/tb-instruction.txt)\" > /agent-logs/agent.ndjson 2>/tmp/agent-stderr.log"
AGENT_RC=$?
AGENT_SECS=$(( $(date +%s) - AGENT_START ))
docker cp "$CONTAINER:/agent-logs/agent.ndjson" "$WORK/agent.ndjson" >/dev/null 2>&1 || true
docker cp "$CONTAINER:/tmp/agent-stderr.log" "$WORK/agent-stderr.log" >/dev/null 2>&1 || true

# Token usage from the NDJSON `done` event (lenient).
TOKENS_IN=""; TOKENS_OUT=""
if [ -f "$WORK/agent.ndjson" ]; then
  read -r TOKENS_IN TOKENS_OUT <<< "$(python3 - "$WORK/agent.ndjson" <<'PYEOF'
import json, sys
ti = to = None
for line in open(sys.argv[1]):
    try:
        ev = json.loads(line)
    except Exception:
        continue
    if isinstance(ev, dict) and ev.get("type") == "done":
        ti = ev.get("tokens_in", ti); to = ev.get("tokens_out", to)
print(ti if ti is not None else "", to if to is not None else "")
PYEOF
)"
fi

# Verdict: the task's own verifier only.
TESTS_START=$(date +%s)
docker exec -e TEST_DIR="$TEST_DIR" -w /app "$CONTAINER" bash /app/run-tests.sh \
  > "$WORK/run-tests.log" 2>&1
TEST_RC=$?
TEST_SECS=$(( $(date +%s) - TESTS_START ))

if [ "$AGENT_RC" -eq 124 ]; then
  NOTE="agent timeout after ${AGENT_SECS}s; run-tests rc=$TEST_RC; image=$IMG_NOTE"
elif [ "$AGENT_RC" -ne 0 ]; then
  NOTE="agent rc=$AGENT_RC (${AGENT_SECS}s); run-tests rc=$TEST_RC (${TEST_SECS}s); image=$IMG_NOTE"
else
  NOTE="agent rc=0 (${AGENT_SECS}s); run-tests rc=$TEST_RC (${TEST_SECS}s); up=${UP_SECS}s image=$IMG_NOTE"
fi

RESOLVED=false
[ "$TEST_RC" -eq 0 ] && RESOLVED=true

python3 - "$VERDICT_FILE" "$RESOLVED" "${TOKENS_IN:-0}" "${TOKENS_OUT:-0}" "$NOTE" <<'PYEOF'
import json, sys
verdict = {
    "resolved": sys.argv[2] == "true",
    "tokens_in": int(sys.argv[3] or 0),
    "tokens_out": int(sys.argv[4] or 0),
    "notes": sys.argv[5],
}
open(sys.argv[1], "w").write(json.dumps(verdict, indent=2) + "\n")
PYEOF
echo "[tb-harness] $NATIVE_ID resolved=$RESOLVED ($NOTE)" >&2
cp "$WORK/run-tests.log" "${VERDICT_FILE%verdict.json}run-tests.log" 2>/dev/null || true
exit 0
