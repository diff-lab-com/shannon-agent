#!/usr/bin/env bash
# Eval-run wrapper: drive a minimax eval with the API key sourced from
# ~/.shannon/credentials/minimax.json (NOT from ambient env vars).
#
# The engine's CLI credential resolution priority is SHANNON_API_KEY >
# ANTHROPIC_API_KEY > OPENAI_API_KEY; it does NOT auto-read the Store backend
# in providers.toml when any of those are set. So we must always inject the
# correct key into SHANNON_API_KEY, overriding any ambient value (e.g. stale
# 40ee9b0c... hex left in the harness's parent shell).
#
# Model selection (corrected after batch-5 RCA — see
# memory/swe-batch5-rca-2026-08-31.md):
#   - The minimax API model id is `MiniMax-M3` (default; hardcoded).
#     $SWE_MODEL_NAME is NOT consulted here. The namespaced form
#     `shannon:shannon-minimax-m3` is a RUN LABEL, not a catalog model_id —
#     the API rejects it with `unknown model 'shannon-minimax-m3'`. An
#     earlier "T2.2 honesty" pass-through wrapper change (which stripped
#     `shannon:` and used the result as `--model`) caused batch-5 to drop
#     48% → 4% because every call was rejected by the API.
#   - $SWE_MODEL_NAME is read ONLY by the harness (swe-harness.sh) to stamp
#     the `eval_label` field in verdict.json — it identifies the run
#     (batch-3 vs batch-4 vs batch-5 etc.) and never controls the API call.
#   - $SWE_MODEL, if set, overrides the default model id. The value MUST
#     be a real catalog id (e.g. `MiniMax-M3-v2`) — NOT a `shannon:`-
#     namespaced string. Unset → `MiniMax-M3`.
#
# Pacing (added after batch-6 RCA — minimax API rate-limit hit 38/50 calls):
#   - Set SWE_MIN_DELAY_MS=<ms> to enforce a minimum delay between successive
#     wrapper invocations. The wrapper stamps its start time in
#     ${SWE_PACING_STATE_FILE:-/tmp/shannon-minimax/.pacing-state}; on the next
#     invocation, if (now - last) < SWE_MIN_DELAY_MS, it sleeps the difference
#     before exec.
#   - batch-6 used no pacing and hit 76% rate-limit rejections (38/50). The
#     minimax API's per-minute window is the likely culprit. batch-7 runs with
#     SWE_MIN_DELAY_MS=30000 (30s) to give each call room.
#   - SWE_PACING_STATE_FILE lets the smoke test use an isolated path; production
#     runs leave it unset and share /tmp/shannon-minimax/.pacing-state across
#     invocations (so pacing is continuous, not per-call).
#   - SWE_MIN_DELAY_MS=0 (or unset) disables entirely. SWE_PACING_RESET=1
#     clears the state file before stamping (use when switching providers).
#   - Caveat: read-then-write is not atomic. For the SWE-bench driver
#     (sequential per-task invocations), this is safe. Concurrent wrapper
#     invocations from multiple drivers would race; if you need that, wrap
#     the read-modify-write block in `flock`.
#
# Binary override:
#   - SHANNON_BIN overrides the engine binary path (default: target/debug/shannon
#     in this workspace). Smoke tests use SHANNON_BIN=/bin/true to skip
#     engine startup.
set -u
KEY_FILE="${HOME}/.shannon/credentials/minimax.json"
[ -r "$KEY_FILE" ] || { echo "FATAL: $KEY_FILE missing — minimax eval credentials not provisioned" >&2; exit 9; }
SHANNON_API_KEY="$(python3 -c "import json,sys;print(json.load(open(sys.argv[1]))['value'])" "$KEY_FILE")"
export SHANNON_API_KEY

# Catalog model id for the minimax provider. This is what the API actually
# receives; the harness's eval_label field is a separate run-identifier.
MODEL_FLAG="MiniMax-M3"
if [ -n "${SWE_MODEL:-}" ]; then
  # Opt-in override for future evals on a different minimax model. The value
  # MUST be a real catalog id — the wrapper does NOT translate namespace
  # prefixes. Default (unset) → MiniMax-M3.
  MODEL_FLAG="$SWE_MODEL"
fi

# Optional binary override (for smoke tests that don't want to exec the
# real engine). Production: unset → /home/ed/workspace/.../target/debug/shannon.
SHANNON_BIN="${SHANNON_BIN:-/home/ed/workspace/app/work/shannon/shannon-mono/target/debug/shannon}"

# Disallow web tools in eval. Real users need WebFetch/WebSearch; SWE-bench
# tasks don't, and the model wastes budget on `curl | head -1000` instead of
# reading code. Pass a space-separated list via $SWE_DISALLOWED_TOOLS (e.g.
# "WebFetch WebSearch"); default kills both. The flags are passed through
# `--disallowedTools` to the Shannon engine, which respects them in its tool
# allow-list. Set SWE_DISALLOWED_TOOLS="" to disable.
# Note: use ${VAR-default} (no colon) so an explicit empty value disables,
# while unset → default. ${VAR:-default} would treat empty as unset.
SWE_DISALLOWED_TOOLS="${SWE_DISALLOWED_TOOLS-WebFetch WebSearch}"
DISALLOWED_FLAGS=()
if [ -n "$SWE_DISALLOWED_TOOLS" ]; then
  for tool in $SWE_DISALLOWED_TOOLS; do
    DISALLOWED_FLAGS+=("--disallowedTools" "$tool")
  done
fi

# Pacing block — see header comment.
MIN_DELAY_MS="${SWE_MIN_DELAY_MS:-0}"
if [[ "$MIN_DELAY_MS" =~ ^[0-9]+$ ]] && [ "$MIN_DELAY_MS" -gt 0 ]; then
  STATE_FILE="${SWE_PACING_STATE_FILE:-/tmp/shannon-minimax/.pacing-state}"
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
      # Clock-skew safety: a negative ELAPSED_MS (state in the future, or
      # NTP step backward) must NOT translate to a multi-hour sleep.
      # Treat as "no time has passed" and sleep the full MIN_DELAY_MS.
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
  # Record this call's post-pacing start so the next invocation sees it.
  printf '%s' "$(python3 -c 'import time;print(int(time.time()*1000))')" > "$STATE_FILE"
fi

exec env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u ZHIPU_API_KEY -u GLM_API_KEY -u MINIMAX_API_KEY \
  "$SHANNON_BIN" \
  --provider minimax --model "$MODEL_FLAG" "${DISALLOWED_FLAGS[@]}" "$@"