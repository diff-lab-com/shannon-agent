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

exec env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u ZHIPU_API_KEY -u GLM_API_KEY -u MINIMAX_API_KEY \
  /home/ed/workspace/app/work/shannon/shannon-mono/target/debug/shannon \
  --provider minimax --model "$MODEL_FLAG" "$@"