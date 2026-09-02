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

# T2.2: stamp `eval_label` + `provider` into every verdict.json so batch
# reports can slice per-provider (e.g. "did the parse-error fix recover
# the 3 minimax batch-3 tasks?").
#
# Naming note (T2.2 honesty, corrected after batch-5 RCA — see
# memory/swe-batch5-rca-2026-08-31.md): the field is called `eval_label`
# because it is the value the driver passed in $SWE_MODEL_NAME — a *run
# label* distinguishing this eval run from prior runs (batch-3 vs batch-4
# vs batch-5 etc.). It is NOT the model the API was actually called with:
# the eval wrapper at /tmp/shannon-minimax/shannon hardcodes
# `--model MiniMax-M3` (the minimax catalog model_id) and ignores
# $SWE_MODEL_NAME. The namespaced form `shannon:shannon-minimax-m3` is a
# run-identifier convention, NOT a real catalog model id — passing it as
# `--model` to the API would return `unknown model 'shannon-minimax-m3'`
# on every call (that's what happened in batch-5 before the wrapper fix).
#
# The label still has signal — it ties a row to the run that produced it
# so TSVs can be sliced per batch. The actual API model is recoverable
# from the wrapper script (`grep MODEL_FLAG /tmp/shannon-minimax/shannon`)
# or by grepping provider config.
#
# `provider` is extracted from the canonical
# `shannon:<provider>-<model>-<rest>` form (see MODEL_NAME at step 5 for
# the equivalent name-with-prefix form used by the official harness to
# namespace its report files).
#
# Computed up front so even an early `fail()` (before step 5 sets
# MODEL_NAME) writes the attribution. Falls back to `<unknown>` when
# $SWE_MODEL_NAME is unset so a missing override never silently breaks
# the verdict contract (callers can still tell "this row has no run
# label" apart from a row with an explicit label).
EVAL_LABEL="${SWE_MODEL_NAME:-unknown}"
case "$EVAL_LABEL" in
  shannon:*) EVAL_LABEL="${EVAL_LABEL#shannon:}" ;;
esac
PROVIDER="unknown"
case "$EVAL_LABEL" in
  shannon-*)
    _after_prefix="${EVAL_LABEL#shannon-}"
    case "$_after_prefix" in
      *-*) PROVIDER="${_after_prefix%%-*}" ;;
      *) PROVIDER="$_after_prefix" ;;
    esac
    ;;
esac
say "verdict attribution: eval_label=$EVAL_LABEL provider=$PROVIDER"

emit() { # emit <resolved true|false> <tokens_in|null> <tokens_out|null> <cost_usd|null> <notes...>
  # JSON-escape every string field via python so a `"`, `\`, or control char
  # in any field (notably `notes` — failure reasons can contain quotes and
  # paths) cannot corrupt the verdict JSON. Eval_label / provider were already
  # escaped; notes was raw `%s` until 2026-08-31.
  local eval_label_json provider_json notes_json
  eval_label_json="$(printf '%s' "$EVAL_LABEL" | python3 -c 'import json,sys;print(json.dumps(sys.stdin.read()))')"
  provider_json="$(printf '%s' "$PROVIDER" | python3 -c 'import json,sys;print(json.dumps(sys.stdin.read()))')"
  notes_json="$(printf '%s' "$5" | python3 -c 'import json,sys;print(json.dumps(sys.stdin.read()))')"
  printf '{"resolved": %s, "tokens_in": %s, "tokens_out": %s, "cost_usd": %s, "eval_label": %s, "provider": %s, "notes": %s}\n' \
    "$1" "${2:-null}" "${3:-null}" "${4:-null}" \
    "$eval_label_json" "$provider_json" \
    "$notes_json" > "$verdict"
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
REPO_PATH="$REPOS/$repo_base"
# prune stale registrations first: a deleted rep workspace leaves the path
# "lost but registered" in the shared clone and would fail the add below.
git -C "$REPO_PATH" worktree prune

# Ensure base_commit is locally reachable BEFORE worktree add.
# The dataset clones under /home/ed/datasets/swebench/repos/ ship with a
# stale .git/shallow that lists several SWE-bench base_commits as shallow
# roots, even though HEAD itself is already fully unshallowed. When git
# worktree add targets one of those shallow-root commits, it tries to
# negotiate parents with the remote — which on this runner flakes on
# github.com:443 (HTTP/2 framing, GnuTLS -110, 130 s timeouts) and burns
# the 1800 s budget before the agent ever runs. We saw 5 instances die
# this way in wave-1 (django-10973) and wave-2 (sympy-12419,
# scikit-learn-10297, scikit-learn-13779, astropy-14995).
#
# Repair strategy (idempotent, one-time per repo):
#   1. if HEAD is already deep (rev-list count ≫ shallow size), the
#      .git/shallow file is stale — delete it. Worktree add then sees a
#      fully unshallowed history and skips the remote round-trip.
#   2. otherwise deepen: fetch the default branch (best-effort --depth),
#      then --unshallow if still shallow. This re-uses any blobs already
#      present locally; on the runner we measured 1–20 s per repo.
ensure_local() {
  # Step 1 — stale .git/shallow when HEAD is already deep.
  local depth revcount shallow_lines
  depth="$(git -C "$REPO_PATH" rev-list --count HEAD 2>/dev/null || echo 0)"
  shallow_lines=0
  [ -f "$REPO_PATH/.git/shallow" ] && shallow_lines=$(wc -l < "$REPO_PATH/.git/shallow")
  if [ "$depth" -gt 100 ] 2>/dev/null && [ "$shallow_lines" -lt "$depth" ]; then
    say "stale shallow (HEAD depth=$depth vs shallow_lines=$shallow_lines); removing"
    rm -f "$REPO_PATH/.git/shallow"
    git -C "$REPO_PATH" config --unset remote.origin.fetch 2>/dev/null || true
    if git -C "$REPO_PATH" merge-base --is-ancestor "$base_commit" HEAD 2>/dev/null; then
      say "base_commit $base_commit reachable after shallow removal"
      return 0
    fi
  fi

  # Step 2 — pull more history. Best-effort default-branch fetch first
  # (cheap if the pack is warm), then --unshallow as a hard fallback.
  if git -C "$REPO_PATH" merge-base --is-ancestor "$base_commit" HEAD 2>/dev/null; then
    say "base_commit $base_commit already local"
    return 0
  fi
  say "deepening $REPO_PATH from origin (depth=$depth)"
  local br
  br="$(git -C "$REPO_PATH" ls-remote --symref origin HEAD 2>/dev/null \
        | awk '/^ref:/ {print $2}' | sed -e 's|refs/heads/||' -e 's|\tHEAD||')"
  br="${br:-main}"
  if ! git -C "$REPO_PATH" fetch --depth 10000 origin \
       "+refs/heads/${br}:refs/remotes/origin/${br}" \
       >>"$ws/worktree.log" 2>&1; then
    say "depth-10000 fetch on $br failed; trying --unshallow"
    git -C "$REPO_PATH" fetch --unshallow origin "$br" \
      >>"$ws/worktree.log" 2>&1 \
      || { say "WARN: both fetches failed (network?); worktree add will retry"; return 0; }
  fi
  if git -C "$REPO_PATH" merge-base --is-ancestor "$base_commit" HEAD 2>/dev/null; then
    say "base_commit $base_commit reachable after pre-fetch"
  else
    say "WARN: $base_commit still not local; worktree add will fall back to its own fetch"
  fi
}
ensure_local

git -C "$REPO_PATH" worktree add --detach "$WT" "$base_commit" >>"$ws/worktree.log" 2>&1 \
  || fail "git worktree add failed (see worktree.log)"

# ── 3. agent run (headless, cwd = worktree) ────────────────────────────────
# SWE_AGENT_HINT (default 1): prepend a short hint block to the user prompt.
# Without it, the SWE-bench v5 container's lack of a `python` symlink sends
# the model into a 10-turn "which python / find / apt-get install" dead end
# instead of writing a fix. batch-7 RCA: 18/30 failures = context_thrash
# ($8.92 wasted). Set SWE_AGENT_HINT=0 to revert to bare issue prompt.
SWE_AGENT_HINT="${SWE_AGENT_HINT:-1}"
AGENT_PROMPT="$(cat "$ISSUE")"
if [ "$SWE_AGENT_HINT" = "1" ]; then
  AGENT_PROMPT="$(cat <<'HINT_END'
[Environment hint — overrides default assumptions]
- The eval container has `python3` (NOT `python`). Use `python3 -c '...'`.
  If `python3` is missing, skip local verification and write the fix anyway —
  the official harness will judge correctness.
- You have a finite turn budget. After ~15 turns of exploring/reading, if
  you have not called Edit/Write yet, STOP exploring and commit a fix.
  A wrong or partial fix is better than an empty patch.
- After Edit/Write, run a focused verification (e.g. `python3 -m pytest -x
  <test_file>::<test_name>` for the relevant test). This is process guidance,
  not secret info — you're only running tests you already see in the repo.
- Prefer Grep (not Bash grep) for symbol search; it indexes once and is
  far cheaper on tokens for repeated queries.

HINT_END
)$AGENT_PROMPT"
fi
if [ "$SMOKE" != "1" ]; then
  mkdir -p "$ws/shannon-home" "$ws/sessions"
  t0=$(date +%s)
  ( cd "$WT" \
    && SHANNON_HOME="$ws/shannon-home" SHANNON_SESSIONS_DIR="$ws/sessions" \
       ${SHANNON_TURN_CHECKPOINT:+SHANNON_TURN_CHECKPOINT="$SHANNON_TURN_CHECKPOINT"} \
       ${SHANNON_TOKEN_BUDGET_WARNING:+SHANNON_TOKEN_BUDGET_WARNING="$SHANNON_TOKEN_BUDGET_WARNING"} \
       timeout "$AGENT_SECS" "$AGENT" --prompt "$AGENT_PROMPT" \
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
# model_name_or_path is what the official harness namespaces reports under
# (`<model_name_with_slashes_replaced>.<run_id>.json` — see step 7). It MUST
# be stable across runs of the same agent so the report lookup below finds
# it. We pin it to a per-provider tag derived from the SWE_MODEL_NAME
# override (or `basename(AGENT)`) so multiple agents coexist in the same
# run-root without colliding on report filenames.
MODEL_NAME="${SWE_MODEL_NAME:-shannon:$(basename "$AGENT")}"
$PYBIN - "$native_id" "$ws/model.patch" "$ws/predictions.jsonl" "$MODEL_NAME" <<'PYEOF'
import json, sys
native_id, patch_path, out_path, model_name = sys.argv[1:5]
pred = {
    "instance_id": native_id,
    "model_name_or_path": model_name,
    "model_patch": open(patch_path, encoding="utf-8").read(),
}
with open(out_path, "w", encoding="utf-8") as fh:
    fh.write(json.dumps(pred) + "\n")
PYEOF
[ -s "$ws/predictions.jsonl" ] || fail "predictions.jsonl was not written"

# ── honest billing observations from the agent's L0 session log ────────────
# Prints "<tokens_in> <tokens_out> <cost> <seen>"; seen=0 means no session
# log was produced (callers must emit nulls then — never fabricate zeros).
# BOTH roots are scanned: the L0 writer resolves SHANNON_HOME/sessions while
# the resume/checkpoint store honors SHANNON_SESSIONS_DIR — whichever layout
# a given build uses, one glob hits it. Usage persists only on completed
# turns (turn/end reason=completed); an agent killed by the wall-clock cap
# leaves NO usage row — the caller's ledger then UNDERCOUNTS that rep, which
# is declared in the batch report instead of being papered over here.
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
  # POSIX-only on purpose: bench_runner templates invoke `sh <gate|harness>`,
  # and dash chokes on `<<<` herestrings — found by the batch-3 wave-0 real
  # run AFTER a full 1800 s agent pass (the expensive way to find out).
  local sums tin tout cost seen
  sums="$(usage_sums)"
  set -- $sums
  tin="$1" tout="$2" cost="$3" seen="$4"
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
# swebench 5.0.2 reporting.py constructs the report path as
#   <report_dir>/<model_name_with_slashes_replaced>.<run_id>.json
# (NO `predictions.` prefix — that was an older convention). model_name may
# also contain `:` (our namespace separator); the harness keeps those verbatim.
REPORT="$ws/report/${MODEL_NAME//\//__}.${RUN_ID}.json"
[ -f "$REPORT" ] || fail "official report missing: $REPORT (model_name=$MODEL_NAME)"
# Capture the official failure_reason into a stderr file so we can fold it
# into the verdict notes on the fail path. Stderr → file (not capture) so the
# Python heredoc stays a single-process, single-PID invocation; stdout still
# carries the resolved= true|false verdict.
"$PYBIN" - "$REPORT" "$native_id" 2>"$ws/harness.stderr" <<'PYEOF' >"$ws/harness.stdout" \
  || fail "could not read resolved_ids from $REPORT"
import json, sys
report = json.load(open(sys.argv[1], encoding="utf-8"))
inst = sys.argv[2]
resolved = inst in report.get("resolved_ids", [])
print("true" if resolved else "false")
if not resolved:
    reason = report.get("failure_reasons", {}).get(inst, "")
    if reason:
        print(f"official failure_reason: {reason}", file=sys.stderr)
PYEOF
resolved="$(cat "$ws/harness.stdout")"
failure_note=""
if [ -s "$ws/harness.stderr" ]; then
  failure_note=" — $(cat "$ws/harness.stderr")"
fi

read_usage_or_null
if [ "$resolved" = "true" ]; then
  emit true "$TIN" "$TOUT" "$COST" \
    "official harness resolved_ids contains $native_id (run_id=$RUN_ID)"
  exit 0
else
  emit false "$TIN" "$TOUT" "$COST" \
    "official harness: $native_id NOT resolved (run_id=$RUN_ID${failure_note})"
  exit 1
fi
