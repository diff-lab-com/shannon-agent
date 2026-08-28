#!/usr/bin/env bash
# swe-harness.sh — SHANNON_SB_HARNESS_CMD adapter for the SWE-bench Verified
# pin slice (§4.13). One invocation = ONE pinned repetition; bench_runner
# calls it as `sh -c "<template>"` with {native_id} substituted, cwd set to
# the repetition workspace, and SHANNON_BENCH_VERDICT_FILE pointing at the
# verdict channel (verdict.json contract: {resolved, tokens_in, tokens_out,
# cost_usd, notes}).
#
# Pipeline (per repetition):
#   1. pull the instance record (problem_statement / base_commit / repo)
#      from the local Verified parquet — no network needed;
#   2. materialize a disposable `git worktree` of the instance repo pinned
#      at base_commit (the shared clone is never mutated);
#   3. run the Shannon agent headlessly inside that worktree, prompt =
#      issue text only (the standard SWE-bench agent contract);
#   4. `git add -A && git diff --cached <base_commit>` → model patch;
#   5. write predictions in the official v5 schema (instance_id,
#      model_name_or_path, model_patch) as JSONL;
#   6. delegate judgment to the OFFICIAL harness (swebench
#      run_evaluation, docker) — Shannon never re-implements the verifier;
#   7. read resolved_ids from the official report → verdict.json.
#
# Smoke mode (SWE_SMOKE=1): steps 3–6 are stubbed (no LLM, no docker); the
# verdict is a loudly-labeled FAKE used only to close the delegation
# protocol (mirrors the 2026-08-28 t15 probe). Never cite a smoke verdict.
#
# Known gotchas carried over from the t15 probe:
#   - eval images use DUNDER-MUNGED names: `__` → `_1776_`
#     (sweb.eval.x86_64.django_1776_django-13279). The official harness
#     handles this internally; do not "fix" the id before lookups.
#   - swebench 5.0.2 needs the NEW schema parquet; pass the local file
#     path as --dataset_name (load_swebench_dataset accepts .parquet).
#   - the verdict path is advertised by bench_runner; nothing else may
#     write it.
#
# Usage: swe-harness.sh <native_id>          (cwd = repetition workspace)
set -u

native_id="${1:?usage: swe-harness.sh <native_id>}"
ws="$PWD"
verdict="${SHANNON_BENCH_VERDICT_FILE:?SHANNON_BENCH_VERDICT_FILE required by adapter contract}"
SMOKE="${SWE_SMOKE:-0}"

# Smoke short-circuit: when a target id is named, every OTHER pin exits 0
# WITHOUT a verdict → the adapter records Ambiguous ("not judged"), keeping
# the smoke artifact honest (exactly one fake-resolved row, t15 probe style).
if [ "$SMOKE" = "1" ] && [ -n "${SWE_SMOKE_ID:-}" ] && [ "$native_id" != "$SWE_SMOKE_ID" ]; then
  echo "[swe-harness] SMOKE: $native_id not judged (target is $SWE_SMOKE_ID)"
  exit 0
fi

SB_HOME="${SHANNON_SWEBENCH_HOME:-/home/ed/datasets/swebench}"
# Dataset: swebench 5.x run_evaluation needs the v5 schema (image/eval_script
# columns). The 2026-08-27-era local dump lacks them; the HF snapshot copy is
# the v5 source (see copy command in scripts/ev?l/README batch-3 notes).
PARQUET="${SWE_DATASET_PARQUET:-}"
if [ -z "$PARQUET" ]; then
  PARQUET="$SB_HOME/SWE-bench_Verified_test_v5schema.parquet"
  [ -f "$PARQUET" ] || PARQUET="$SB_HOME/SWE-bench_Verified_test.parquet"
fi
REPOS="$SB_HOME/repos"
PYBIN="${SWE_HARNESS_PYTHON:-/tmp/swebench-probe/venv/bin/python}"
AGENT="${SHANNON_SB_AGENT_BIN:-${SHANNON_EVAL_BIN:-/tmp/shannon-zhipu/shannon-glm-plan}}"
AGENT_SECS="${SWE_AGENT_TIMEOUT_SECS:-1800}"
AGENT_MAX_TURNS="${SWE_AGENT_MAX_TURNS:-80}"
HARNESS_TEST_SECS="${SWE_TEST_TIMEOUT_SECS:-1800}"

say() { echo "[swe-harness] $*"; }

emit() { # emit <resolved true|false> <notes...>
  printf '{"resolved": %s, "tokens_in": %s, "tokens_out": %s, "cost_usd": %s, "notes": "%s"}\n' \
    "$1" "${2:-null}" "${3:-null}" "${4:-null}" "$5" > "$verdict"
}

fail() { emit false null null null "$1"; say "FAILED: $1"; exit 1; }

[ -f "$PARQUET" ] || fail "Verified parquet not found at $PARQUET (set SWE_DATASET_PARQUET / SHANNON_SWEBENCH_HOME)"
[ -x "$PYBIN" ] || fail "python with swebench not found at $PYBIN (set SWE_HARNESS_PYTHON)"

# v5 schema guard: the official harness (step 6) hard-needs image/eval_script;
# feeding the old-schema dump fails mid-judgment, after the agent already ran.
schema="$($PYBIN - "$PARQUET" <<'PYEOF'
import sys
import pyarrow.parquet as pq
cols = set(pq.ParquetFile(sys.argv[1]).schema_arrow.names)
need = {"instance_id", "problem_statement", "base_commit", "repo", "image", "eval_script"}
missing = sorted(need - cols)
print("OK" if not missing else "MISSING:" + ",".join(missing))
PYEOF
)" || fail "could not read parquet schema at $PARQUET"
[ "$schema" = "OK" ] || fail "parquet $PARQUET lacks v5 columns ($schema) — feed the HF snapshot as SWE_DATASET_PARQUET (v5schema copy convention)"

# ── 1. instance record from the local parquet (offline) ────────────────────
mkdir -p "$ws"
ISSUE="$ws/issue.txt"
REPO_DIR="$($PYBIN - "$PARQUET" "$native_id" "$ISSUE" <<'PYEOF'
import sys, pyarrow.parquet as pq
table = pq.read_table(sys.argv[1])
cols = {name: table.column(name).to_pylist() for name in table.column_names}
ids = cols["instance_id"]
try:
    row = ids.index(sys.argv[2])
except ValueError:
    print(f"NOT_FOUND {sys.argv[2]}", file=sys.stderr); sys.exit(3)
with open(sys.argv[3], "w", encoding="utf-8") as fh:
    fh.write(cols["problem_statement"][row])
print(cols["repo"][row])
PYEOF
)" || fail "instance $native_id not found in parquet"
repo_base="${REPO_DIR##*/}"
base_commit="$($PYBIN - "$PARQUET" "$native_id" <<'PYEOF'
import sys, pyarrow.parquet as pq
t = pq.read_table(sys.argv[1])
ids = t.column("instance_id").to_pylist()
print(t.column("base_commit").to_pylist()[ids.index(sys.argv[2])])
PYEOF
)"
[ -d "$REPOS/$repo_base/.git" ] || [ -d "$REPOS/$repo_base" ] || fail "repo clone missing: $REPOS/$repo_base"
say "instance $native_id · repo=$REPO_DIR · base=$base_commit"

# ── 2. disposable worktree at base_commit ──────────────────────────────────
WT="$ws/repo-wt"
rm -rf "$WT"
# prune stale registrations first: a deleted rep workspace leaves the path
# "lost but registered" in the shared clone and would fail the add below.
git -C "$REPOS/$repo_base" worktree prune
git -C "$REPOS/$repo_base" worktree add --detach "$WT" "$base_commit" >"$ws/worktree.log" 2>&1 \
  || fail "git worktree add failed (see worktree.log)"

# ── 3. agent run (headless, cwd = worktree) ────────────────────────────────
if [ "$SMOKE" != "1" ]; then
  mkdir -p "$ws/shannon-home" "$ws/sessions"
  t0=$(date +%s)
  ( cd "$WT" \
    && SHANNON_HOME="$ws/shannon-home" SHANNON_SESSIONS_DIR="$ws/sessions" \
       timeout "$AGENT_SECS" "$AGENT" --prompt "$(cat "$ISSUE")" \
       --output-format json --max-turns "$AGENT_MAX_TURNS" \
    > "$ws/agent-out.json" 2> "$ws/agent-err.log" )
  agent_rc=$?
  say "agent rc=$agent_rc in $(( $(date +%s) - t0 ))s"
  [ "$agent_rc" -eq 0 ] || say "WARNING: agent exited $agent_rc — patch is whatever the worktree holds"
else
  say "SMOKE: agent step stubbed"
fi

# ── 4. model patch from the worktree ───────────────────────────────────────
git -C "$WT" add -A >>"$ws/worktree.log" 2>&1
git -C "$WT" diff --cached "$base_commit" > "$ws/model.patch" 2>>"$ws/worktree.log"
say "patch bytes: $(wc -c < "$ws/model.patch")"

# ── 5. predictions (official v5 schema) ────────────────────────────────────
$PYBIN - "$native_id" "$ws/model.patch" "$ws/predictions.jsonl" "$AGENT" <<'PYEOF'
import json, sys, os
native_id, patch_path, out_path, agent = sys.argv[1:5]
pred = {
    "instance_id": native_id,
    "model_name_or_path": f"shannon:{os.path.basename(agent)}",
    "model_patch": open(patch_path, encoding="utf-8").read(),
}
with open(out_path, "w", encoding="utf-8") as fh:
    fh.write(json.dumps(pred) + "\n")
PYEOF
[ -s "$ws/predictions.jsonl" ] || fail "predictions.jsonl was not written"

# ── honest billing observations from the agent's L0 session log ────────────
# Prints "<tokens_in> <tokens_out> <cost> <seen>"; seen=0 means no session
# log was produced (callers must emit nulls then — never fabricate zeros).
# The engine's sessions container follows SHANNON_SESSIONS_DIR (this harness
# sets it to <ws>/sessions), while SHANNON_HOME only relocates other state —
# so BOTH roots are scanned; missing both is the only `seen=0` path.
usage_sums() {
  python3 - "$ws/shannon-home" "$ws/sessions" <<'PYEOF'
import json, glob, sys
ti = to = 0
cost = 0.0
seen = False
for base in sys.argv[1:]:
    for path in glob.glob(f"{base}/sessions/*/events.jsonl"):
        seen = True
        for line in open(path, encoding="utf-8"):
            try:
                ev = json.loads(line)
            except Exception:
                continue
            u = ev.get("usage") or {}
            ti += int(u.get("input_tokens") or 0)
            to += int(u.get("output_tokens") or 0)
            c = u.get("cost_usd")
            if isinstance(c, (int, float)):
                cost += c
print(f"{ti} {to} {cost:.6f} {int(seen)}")
PYEOF
}

read_usage_or_null() { # sets TIN/TOUT/COST (null when no session log)
  local tin tout cost seen
  read -r tin tout cost seen <<<"$(usage_sums)"
  if [ "${seen:-0}" = "1" ]; then
    TIN="$tin" TOUT="$tout" COST="$cost"
  else
    TIN="null" TOUT="null" COST="null"
  fi
}

# ── 6. official harness (docker) — the ONLY judge ──────────────────────────
if [ "$SMOKE" = "1" ]; then
  say "SMOKE: official harness skipped — protocol closure only, NOT a judgment"
  read_usage_or_null
  emit true "$TIN" "$TOUT" "$COST" \
    "SMOKE fake verdict — delegation plumbing closure only (t15 probe convention); no model, no docker, no judgment"
  exit 0
fi

RUN_ID="sb-${native_id}-${RANDOM}"
mkdir -p "$ws/report"
t1=$(date +%s)
"$PYBIN" -m swebench.harness.run_evaluation \
  --dataset_name "$PARQUET" --split test \
  --instance_ids "$native_id" \
  --predictions_path "$ws/predictions.jsonl" \
  --run_id "$RUN_ID" --report_dir "$ws/report" \
  --max_workers 1 --timeout "$HARNESS_TEST_SECS" \
  > "$ws/run_evaluation.log" 2>&1
harness_rc=$?
say "official harness rc=$harness_rc in $(( $(date +%s) - t1 ))s (see run_evaluation.log)"

# ── 7. resolved_ids from the official report → verdict ─────────────────────
REPORT="$ws/report/predictions.$RUN_ID.json"
[ -f "$REPORT" ] || fail "official report missing: $REPORT"
resolved="$($PYBIN - "$REPORT" "$native_id" <<'PYEOF'
import json, sys
report = json.load(open(sys.argv[1], encoding="utf-8"))
print("true" if sys.argv[2] in report.get("resolved_ids", []) else "false")
print(report.get("failure_reasons", {}).get(sys.argv[2], ""), file=sys.stderr)
PYEOF
)" || fail "could not read resolved_ids from $REPORT"
resolved="$(echo "$resolved" | head -1)"

read_usage_or_null
if [ "$resolved" = "true" ]; then
  emit true "$TIN" "$TOUT" "$COST" \
    "official harness resolved_ids contains $native_id (run_id=$RUN_ID)"
  exit 0
else
  emit false "$TIN" "$TOUT" "$COST" \
    "official harness: $native_id NOT resolved (run_id=$RUN_ID)"
  exit 1
fi
