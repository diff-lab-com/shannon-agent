#!/usr/bin/env bash
# Eval-run wrapper for the GLM (zhipu-coding-plan) provider: drive a Shannon
# eval with the API key sourced from ~/.shannon/credentials/zhipu.json (NOT
# from ambient env vars). Mirrors wrapper-minimax.sh so swe-batch-driver.sh /
# swe-harness.sh / run-batch.sh can switch provider by swapping WRAPPER/--bin.
#
# Credential parsing (differs from wrapper-minimax on purpose):
#   The stock zhipu.json on this machine carries the credential JSON object on
#   line 1 followed by a pasted providers.toml snippet, so plain
#   `json.load(...)` dies with "Extra data". raw_decode() takes the first JSON
#   value and ignores trailing garbage — and still works on clean files.
#
# Provider/model (batch-5 RCA rule applies verbatim):
#   - Provider is `zhipu-coding-plan` (GLM Coding Plan subscription,
#     https://open.bigmodel.cn/api/coding/paas/v4). The engine resolves its
#     key from SHANNON_API_KEY (canonical fallback ZHIPU_API_KEY stays unset
#     via the `env -u` line below).
#   - The API model id is `glm-5.3-flash` (default; hardcoded). This is the
#     exact id the v1 L1 baseline (tests/eval/baselines/v1-glm-5.3-flash-*)
#     ran under — do NOT "normalize" it to a catalog name (e.g. glm-5-flash);
#     anchor comparability depends on the literal id.
#   - $SWE_MODEL_NAME is ONLY a run label consumed by swe-harness.sh (verdict
#     attribution); it never reaches the API call.
#   - $SWE_MODEL, if set, overrides the model id and MUST be a real
#     zhipu-coding-plan catalog id. Unset → glm-5.3-flash.
#
# Pacing (batch-6 RCA — coding-plan enforces per-5-minute prompt windows):
#   - SWE_MIN_DELAY_MS=<ms> enforces a minimum delay between wrapper
#     invocations, state in ${SWE_PACING_STATE_FILE:-/tmp/shannon-glm/.pacing-state}.
#   - SWE_PACING_RESET=1 clears the state file before stamping.
#   - Unset/0 disables. Read-then-write is not atomic; the SWE driver is
#     sequential so this is safe (same caveat as the minimax wrapper).
#
# Env hygiene (beyond the minimax wrapper): SHANNON_MODEL / SHANNON_PROVIDER /
# SHANNON_BASE_URL are unset too — ambient exports would otherwise fight the
# wrapper's --provider/--model flags depending on config layering.
#
# Binary override: SHANNON_BIN (default: main checkout's target/debug/shannon).
set -u
KEY_FILE="${HOME}/.shannon/credentials/zhipu.json"
[ -r "$KEY_FILE" ] || { echo "FATAL: $KEY_FILE missing — zhipu eval credentials not provisioned" >&2; exit 9; }
SHANNON_API_KEY="$(python3 -c "
import json,sys
value=json.JSONDecoder().raw_decode(open(sys.argv[1]).read())[0]['value']
print(value)
" "$KEY_FILE")"
[ -n "$SHANNON_API_KEY" ] || { echo "FATAL: no 'value' credential in $KEY_FILE" >&2; exit 9; }
export SHANNON_API_KEY

MODEL_FLAG="glm-5.3-flash"
if [ -n "${SWE_MODEL:-}" ]; then
  # Opt-in override for a different zhipu-coding-plan model. MUST be a real
  # API id — the wrapper does NOT translate namespace prefixes.
  MODEL_FLAG="$SWE_MODEL"
fi

SHANNON_BIN="${SHANNON_BIN:-/home/ed/workspace/app/work/shannon/shannon-mono/target/debug/shannon}"

# Disallow web tools in eval (same rationale as wrapper-minimax: SWE/TB tasks
# don't need the web and the model burns budget crawling). Override with
# SWE_DISALLOWED_TOOLS ("" disables).
SWE_DISALLOWED_TOOLS="${SWE_DISALLOWED_TOOLS-WebFetch WebSearch}"
DISALLOWED_FLAGS=()
if [ -n "$SWE_DISALLOWED_TOOLS" ]; then
  for tool in $SWE_DISALLOWED_TOOLS; do
    DISALLOWED_FLAGS+=("--disallowed-tools" "$tool")
  done
fi

# Pacing block — see header comment.
MIN_DELAY_MS="${SWE_MIN_DELAY_MS:-0}"
if [[ "$MIN_DELAY_MS" =~ ^[0-9]+$ ]] && [ "$MIN_DELAY_MS" -gt 0 ]; then
  STATE_FILE="${SWE_PACING_STATE_FILE:-/tmp/shannon-glm/.pacing-state}"
  STATE_DIR="$(dirname "$STATE_FILE")"
  mkdir -p "$STATE_DIR"
  if [ -n "${SWE_PACING_RESET:-}" ]; then
    rm -f "$STATE_FILE"
  fi
  T0_MS="$(python3 -c 'import time;print(int(time.time()*1000))')"
  LAST_MS=0
  if [ -f "$STATE_FILE" ]; then
    LAST_MS="$(cat "$STATE_FILE" 2>/dev/null | tr -d '[:space:]')"
    [[ "$LAST_MS" =~ ^[0-9]+$ ]] || LAST_MS=0
  fi
  if [ "$LAST_MS" -gt 0 ]; then
    ELAPSED_MS=$((T0_MS - LAST_MS))
    if [ "$ELAPSED_MS" -lt "$MIN_DELAY_MS" ]; then
      if [ "$ELAPSED_MS" -lt 0 ]; then
        SLEEP_MS=$MIN_DELAY_MS
        echo "[pacing] state ahead of clock by ${ELAPSED_MS}ms (skew/reset); sleeping full ${MIN_DELAY_MS}ms" >&2
      else
        SLEEP_MS=$((MIN_DELAY_MS - ELAPSED_MS))
        echo "[pacing] sleeping ${SLEEP_MS}ms (elapsed=${ELAPSED_MS}ms, min=${MIN_DELAY_MS}ms since last call)" >&2
      fi
      SLEEP_S=$(awk -v ms=$SLEEP_MS 'BEGIN{printf "%.3f", ms/1000.0}')
      sleep "$SLEEP_S"
    fi
  fi
  printf '%s' "$(python3 -c 'import time;print(int(time.time()*1000))')" > "$STATE_FILE"
fi

exec env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u ZHIPU_API_KEY -u GLM_API_KEY -u MINIMAX_API_KEY \
  -u SHANNON_MODEL -u SHANNON_PROVIDER -u SHANNON_BASE_URL \
  "$SHANNON_BIN" \
  --provider zhipu-coding-plan --model "$MODEL_FLAG" "${DISALLOWED_FLAGS[@]}" "$@"
