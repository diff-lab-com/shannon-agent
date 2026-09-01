#!/usr/bin/env python3
"""
SWE-bench failure-mode classifier.

Reads a batch's per-task directory and assigns each failed task a mode:

  no_action       — agent ran 0 edits and produced an empty patch
                    (could be: gave up early, never read the issue, or
                    context-thrashed exploring without committing)
  context_thrash  — agent hit max_turns (30) with 0 edits and empty patch;
                    strongly suggests the model explored/tested for the
                    whole budget without writing a fix
  max_turns_edit  — agent hit max_turns but DID produce a non-empty patch;
                    ran out of context mid-debug (likely Edit happened
                    but verification was truncated)
  test_fail       — non-empty patch, agent rc=0, but the FAIL_TO_PASS test
                    in test_output.txt did not pass (wrong fix)
  test_passed     — non-empty patch, all FAIL_TO_PASS tests passed but
                    PASS_TO_PASS broke (regression — separate code path)
  api_error       — agent-err.log contains rate-limit, unknown model,
                    invalid params, or connection errors
  timeout         — harness.log shows the harness itself timed out
  unknown         — no clean match; surfaced for manual review

Usage:
  python3 swe-classify-failures.py <per-task-dir> [--all]
    --all  also classify passing tasks (defaults to failed only)

Output:
  - Table sorted by task id
  - Per-mode counts
  - Average tokens / cost for each mode
"""
import argparse
import json
import os
import re
import sys
from collections import defaultdict
from pathlib import Path

MAX_TURNS_RE = re.compile(r"Max turns \((\d+)\) reached")
# pytest-style: "1 failed, 9 passed"
TEST_FAIL_RE = re.compile(r"(\d+)\s+failed", re.IGNORECASE)
TEST_PASS_RE = re.compile(r"(\d+)\s+passed", re.IGNORECASE)
# unittest-style: "FAILED (failures=1)" / "OK" / "Ran N tests"
UNITTEST_FAIL_RE = re.compile(r"FAILED\s*\(errors?=(\d+)|FAILED\s*\(failures=(\d+)|failures=(\d+)|errors=(\d+)", re.IGNORECASE)
UNITTEST_RUN_RE = re.compile(r"Ran\s+(\d+)\s+tests?", re.IGNORECASE)
AGENT_RC_RE = re.compile(r"agent rc=(\d+)")
RL_RE = re.compile(r"rate limit|rate\.limit", re.IGNORECASE)
REJECT_RE = re.compile(r"unknown model|invalid params", re.IGNORECASE)
TIMEOUT_RE = re.compile(r"harness.*timeout|TimeoutExpired", re.IGNORECASE)
SWEBENCH_EXIT_RE = re.compile(r"SWEBENCH_TEST_EXIT_CODE=(\d+)")


def classify(task_dir: Path) -> dict:
    verdict_p = task_dir / "verdict.json"
    err_p = task_dir / "agent-err.log"
    harness_p = task_dir / "harness.log"
    patch_p = task_dir / "model.patch"
    if not verdict_p.exists():
        return {"mode": "unknown", "reason": "no verdict.json"}
    verdict = json.loads(verdict_p.read_text())
    resolved = verdict.get("resolved")
    err = err_p.read_text() if err_p.exists() else ""
    harness = harness_p.read_text() if harness_p.exists() else ""
    patch_bytes = patch_p.stat().st_size if patch_p.exists() else 0

    edit_calls = sum(err.count(c) for c in ("invoking Edit", "invoking Write", "invoking MultiEdit"))
    bash_calls = err.count("invoking Bash")
    read_calls = err.count("invoking Read")
    tool_errors = err.count("tool-error")
    max_turns_hit = bool(MAX_TURNS_RE.search(err))
    agent_rc_m = AGENT_RC_RE.search(harness)
    agent_rc = int(agent_rc_m.group(1)) if agent_rc_m else -1
    rl_hit = bool(RL_RE.search(err))
    reject_hit = bool(REJECT_RE.search(err))
    timeout_hit = bool(TIMEOUT_RE.search(harness)) or bool(TIMEOUT_RE.search(err))

    # Find test_output.txt and grep for the FAIL_TO_PASS verdict.
    test_summary = None
    exit_code = None
    test_outputs = list(task_dir.glob("logs/run_evaluation/*/*/*/test_output.txt"))
    if test_outputs:
        text = test_outputs[0].read_text()
        # pytest-style: "N failed, M passed"
        m_fail = TEST_FAIL_RE.search(text)
        m_pass = TEST_PASS_RE.search(text)
        # unittest-style: "FAILED (failures=N)" or "Ran N tests"
        m_ufail = UNITTEST_FAIL_RE.search(text)
        m_urun = UNITTEST_RUN_RE.search(text)
        # SWEBENCH_TEST_EXIT_CODE is the authoritative signal
        m_exit = SWEBENCH_EXIT_RE.search(text)
        if m_exit:
            exit_code = int(m_exit.group(1))
        if m_fail and m_pass:
            test_summary = f"{m_fail.group(1)} failed / {m_pass.group(1)} passed (pytest, exit={exit_code})"
        elif m_ufail:
            n = next((g for g in m_ufail.groups() if g), "?")
            ran = m_urun.group(1) if m_urun else "?"
            test_summary = f"unittest failures={n} (ran={ran}, exit={exit_code})"

    # === Classification (ordered: most specific first) ===
    mode = None
    reason = None

    # === Classification (ordered: most specific first) ===
    # SWEBENCH_TEST_EXIT_CODE is the authoritative harness verdict.
    # If we have it, prefer it over pattern matching.
    if exit_code == 0:
        mode = "test_passed"
        reason = f"SWEBENCH_TEST_EXIT_CODE=0 ({test_summary or 'no summary line'})"
    elif exit_code is not None and exit_code != 0:
        mode = "test_fail"
        reason = f"SWEBENCH_TEST_EXIT_CODE={exit_code} ({test_summary or 'no summary line'})"
    elif rl_hit or reject_hit:
        mode = "api_error"
        reason = "rate-limit" if rl_hit else "model-rejected"
    elif timeout_hit:
        mode = "timeout"
        reason = "harness timeout"
    elif max_turns_hit and patch_bytes > 0:
        mode = "max_turns_edit"
        reason = "hit 30 turns after producing patch (ran out mid-debug)"
    elif max_turns_hit and patch_bytes == 0 and edit_calls == 0:
        mode = "context_thrash"
        reason = "30 turns / 0 edits / empty patch (explored only)"
    elif patch_bytes == 0 and edit_calls == 0 and not max_turns_hit:
        mode = "no_action"
        reason = f"no edits, no patch (turns={bash_calls + read_calls})"
    elif patch_bytes == 0 and edit_calls > 0:
        mode = "edit_dropped"
        reason = f"made {edit_calls} edits but patch empty (rc={agent_rc})"
    elif patch_bytes > 0 and test_summary is None and agent_rc == 0:
        mode = "test_infra"
        reason = "patch present, rc=0, but no SWEBENCH_TEST_EXIT_CODE (eval missing?)"
    elif patch_bytes > 0:
        mode = "test_fail"
        reason = f"patch present but no exit-code ({test_summary or 'no summary'})"
    else:
        mode = "unknown"
        reason = f"patch={patch_bytes}B rc={agent_rc} turns={bash_calls + read_calls}"

    return {
        "mode": mode,
        "reason": reason,
        "resolved": resolved,
        "tokens_in": verdict.get("tokens_in"),
        "cost_usd": verdict.get("cost_usd"),
        "patch_bytes": patch_bytes,
        "agent_rc": agent_rc,
        "edit_calls": edit_calls,
        "bash_calls": bash_calls,
        "read_calls": read_calls,
        "tool_errors": tool_errors,
        "max_turns_hit": max_turns_hit,
        "test_summary": test_summary,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("per_task_dir", type=Path,
                    help="path to <batch>/per-task (each subdir = one rep)")
    ap.add_argument("--all", action="store_true",
                    help="classify every task, not just failed ones")
    ap.add_argument("--exclude", default="probe",
                    help="comma-separated task-name prefixes to exclude")
    args = ap.parse_args()
    if not args.per_task_dir.is_dir():
        print(f"FATAL: not a directory: {args.per_task_dir}", file=sys.stderr)
        return 2

    excludes = set(args.exclude.split(","))
    rows = []
    for sub in sorted(args.per_task_dir.iterdir()):
        if not sub.is_dir() or sub.name in excludes:
            continue
        if sub.name.startswith("rep-0-"):
            task_id = sub.name[len("rep-0-"):]
        elif sub.name.startswith("rep-"):
            task_id = sub.name
        else:
            continue
        cls = classify(sub)
        cls["task"] = task_id
        rows.append(cls)

    if not args.all:
        rows = [r for r in rows if not r.get("resolved")]
    rows.sort(key=lambda r: (r["mode"], r["task"]))

    # === Per-task table ===
    print(f"\n{'task':<48}{'mode':<18}{'patch':>6}{'rc':>4}{'edits':>6}{'turns':>6}  reason")
    print("-" * 130)
    for r in rows:
        turns = (r.get("bash_calls", 0) + r.get("read_calls", 0))
        print(f"{r['task']:<48}{r['mode']:<18}{r['patch_bytes']:>6}{r['agent_rc']:>4}"
              f"{r['edit_calls']:>6}{turns:>6}  {r['reason']}")

    # === Per-mode summary ===
    by_mode = defaultdict(list)
    for r in rows:
        by_mode[r["mode"]].append(r)
    print()
    print(f"{'mode':<18}{'count':>6}{'cost':>10}{'tok_M':>8}{'patch_B':>10}{'edits':>8}")
    print("-" * 60)
    grand_count = 0
    grand_cost = 0.0
    grand_tok = 0
    for mode in sorted(by_mode):
        items = by_mode[mode]
        count = len(items)
        cost = sum(i.get("cost_usd") or 0 for i in items)
        tok = sum(i.get("tokens_in") or 0 for i in items)
        patch = sum(i["patch_bytes"] for i in items)
        edits = sum(i["edit_calls"] for i in items)
        print(f"{mode:<18}{count:>6}{'$'+format(cost, '.2f'):>10}"
              f"{tok/1e6:>7.2f}M{patch:>10}{edits:>8}")
        grand_count += count
        grand_cost += cost
        grand_tok += tok
    print("-" * 60)
    print(f"{'TOTAL':<18}{grand_count:>6}{'$'+format(grand_cost, '.2f'):>10}"
          f"{grand_tok/1e6:>7.2f}M")
    return 0


if __name__ == "__main__":
    sys.exit(main())