#!/usr/bin/env python3
"""Per-task token/turn/wall-clock aggregation for a DeepSWE wave job dir.

Reads each trial's agent/shannon.ndjson (the CLI json-stream) and merges the
engine's final-accounting `done` event (max-merge, same rule as the pier
adapter) with the verifier's reward.json. Cross-references the official
Datacurve anchors (glm-5.3-flash [max]: 73k output tokens / 123 steps).

Usage:
    python3 scripts/eval/deepswe_tokens.py [--job-dir DIR]

Defaults to the w7 job dir. Output: per-trial rows + cohort summary printed
to stdout; exit 0 always (report helper, not a gate).
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import sys

DEFAULT_JOB = os.path.expanduser(
    "~/.shannon/eval/deepswe/jobs/deepswe-base-w7"
)
OFFICIAL_OUT_TOKENS = 73_000
OFFICIAL_STEPS = 123


def merge_done(ndjson_path: str) -> dict:
    """max-merge every `done` event of a shannon json-stream file."""
    acc = {"tokens_in": 0, "tokens_out": 0, "turns_used": 0, "exit_code": None}
    if not os.path.isfile(ndjson_path):
        return acc
    with open(ndjson_path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get("type") != "done":
                continue
            acc["tokens_in"] = max(
                acc["tokens_in"], int(event.get("tokens_in") or 0)
            )
            acc["tokens_out"] = max(
                acc["tokens_out"], int(event.get("tokens_out") or 0)
            )
            acc["turns_used"] = max(
                acc["turns_used"], int(event.get("turns_used") or 0)
            )
            if event.get("exit_code") is not None:
                acc["exit_code"] = event.get("exit_code")
    return acc


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--job-dir", default=DEFAULT_JOB)
    args = ap.parse_args()

    rows = []
    for trial_dir in sorted(glob.glob(os.path.join(args.job_dir, "*/"))):
        name = os.path.basename(trial_dir.rstrip("/")).split("__")[0]
        usage = merge_done(os.path.join(trial_dir, "agent", "shannon.ndjson"))
        reward = None
        minutes = None
        reward_path = os.path.join(trial_dir, "verifier", "reward.json")
        config_path = os.path.join(trial_dir, "config.json")
        if os.path.isfile(reward_path):
            try:
                reward = json.load(open(reward_path)).get("reward")
            except json.JSONDecodeError:
                pass
        if os.path.isfile(config_path) and os.path.isfile(reward_path):
            minutes = (
                os.path.getmtime(reward_path) - os.path.getmtime(config_path)
            ) / 60
        rows.append(
            {
                "task": name,
                "reward": reward,
                "minutes": minutes,
                **usage,
            }
        )

    print(f"{'task':<40} {'rw':>3} {'min':>5} {'turns':>6} "
          f"{'in(k)':>8} {'out(k)':>7} {'exit':>5}")
    for r in rows:
        rw = "?" if r["reward"] is None else r["reward"]
        mn = f"{r['minutes']:.0f}" if r["minutes"] else "-"
        print(f"{r['task'][:40]:<40} {rw:>3} {mn:>5} {r['turns_used']:>6} "
              f"{r['tokens_in'] / 1000:>8.0f} {r['tokens_out'] / 1000:>7.1f} "
              f"{str(r['exit_code']):>5}")

    graded = [r for r in rows if r["reward"] is not None]
    solved = [r for r in graded if r["reward"] == 1]
    for label, cohort in (("solved", solved), ("graded", graded)):
        if not cohort:
            continue
        n = len(cohort)
        out_k = sum(r["tokens_out"] for r in cohort) / n / 1000
        in_k = sum(r["tokens_in"] for r in cohort) / n / 1000
        turns = sum(r["turns_used"] for r in cohort) / n
        mins = [r["minutes"] for r in cohort if r["minutes"]]
        mins_s = f"{sum(mins) / len(mins):.0f}m" if mins else "-"
        print(
            f"[{label}] n={n}  out={out_k:.1f}k tok (official {OFFICIAL_OUT_TOKENS // 1000}k)  "
            f"in={in_k:.0f}k tok  turns={turns:.0f} (official {OFFICIAL_STEPS})  "
            f"mean={mins_s}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
