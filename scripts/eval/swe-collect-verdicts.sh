#!/usr/bin/env bash
# swe-collect-verdicts.sh — Collect per-task verdict.json files into a single
# TSV for batch reporting (§4.13). Reads `eval_label` + `provider` from each
# verdict.json (T2.2: fields added to swe-harness.sh emit()), so reports can
# be sliced per-provider / per-eval-run — e.g. "did the parse-error fix recover
# the 3 minimax batch-3 tasks?".
#
# Usage:
#   swe-collect-verdicts.sh <verdicts-dir>           # writes TSV to stdout
#   swe-collect-verdicts.sh <verdicts-dir> > out.tsv # redirect
#
# Input shape: <verdicts-dir>/rep-N-<native_id>/verdict.json (the layout
# run-batch.sh + the matplotlib-probe driver both produce). The script also
# accepts a flat dir of *.verdict.json files; in that case the `task` column
# is the filename stem.
#
# Output columns (tab-separated, header on first line):
#   task     resolved  tokens_in  tokens_out  cost_usd  eval_label  provider  notes
#
# Backward compat (3 generations):
#   1. Pre-T2.2 verdict.json (no model_id/provider) → columns = `<unknown>`
#   2. T2.2 verdict.json (model_id field) → column = old field name, mapped
#      into `eval_label` slot. This keeps batch-3/batch-4 TSVs parseable.
#   3. Current verdict.json (eval_label field) → column = `eval_label`.
# The TSV contract is therefore forward-additive — old + new verdicts can be
# mixed in the same input dir and the column is stable across generations.
set -u

dir="${1:?usage: swe-collect-verdicts.sh <verdicts-dir>}"
[ -d "$dir" ] || { echo "swe-collect-verdicts: not a directory: $dir" >&2; exit 2; }

# Header is emitted unconditionally so even an empty input produces a valid
# (header-only) TSV — callers shouldn't have to special-case that.
printf 'task\tresolved\ttokens_in\ttokens_out\tcost_usd\teval_label\tprovider\tnotes\n'

# Two layouts accepted:
#   1. <dir>/rep-N-<native_id>/verdict.json     (run-batch.sh / probe layout)
#   2. <dir>/<native_id>.verdict.json           (flat alternative)
find "$dir" -type f -name 'verdict.json' | sort | while read -r path; do
  # Default to the parent dir name; strip the "rep-N-" prefix when present
  # so the task column matches the run-batch.tsv convention (just the
  # native_id, no rep ordinal).
  task="$(basename "$(dirname "$path")")"
  case "$task" in
    rep-*-*) task="${task#rep-*-}" ;;
  esac
  python3 - "$path" "$task" <<'PYEOF'
import json, sys

path, task = sys.argv[1], sys.argv[2]
with open(path, encoding="utf-8") as fh:
    v = json.load(fh)

def cell(value, default="null"):
    """Normalize verdict.json fields to TSV-safe strings."""
    if value is None:
        return default
    return str(value)

# Tabs/newlines in notes would break the TSV — collapse them. Notes are a
# short evidence trail (≤120 chars in practice); truncation is the
# downstream consumer's job, not ours.
notes = (v.get("notes") or "").replace("\\", " ").replace("\t", " ").replace("\n", " ")
notes = notes[:200]

print(
    "\t".join([
        task,
        cell(v.get("resolved"), "false"),
        cell(v.get("tokens_in")),
        cell(v.get("tokens_out")),
        cell(v.get("cost_usd")),
        cell(v.get("eval_label") if "eval_label" in v else v.get("model_id"), "unknown"),
        cell(v.get("provider"), "unknown"),
        notes,
    ])
)
PYEOF
done
