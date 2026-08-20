"""Performance channel (plan §5.4 性能通道, P1b).

- Decision signal: `just perf` — 12 absolute-threshold tests, mockito-driven,
  independent of live provider latency. A test that passed in the baseline but
  fails now is a perf_regress finding.
- Observation signal: per-task wall time / ttft / turns / tokens trends
  (summary.json); NOT used to open fix briefs.
- Localization tool: criterion benches, run on demand only (never per-iter).
"""

from __future__ import annotations

import logging
import re
import subprocess
from pathlib import Path

from common import REPO_ROOT, atomic_write_json, read_json

# nextest result lines: `PASS [   0.004s] ( 4/13) shannon-core api::tests::x`
# — optional (n/m) progress counter, then a crate-qualified name WITH spaces.
LINE_RE = re.compile(r"^\s*(PASS|FAIL)\s+\[[^\]]*\](?:\s+\([^)]*\))?\s+(.+?)\s*$")


def run_perf(baseline_path: Path, enabled: bool = True,
             timeout_s: int = 1800) -> dict:
    """Run `just perf`; returns {"ran":bool,"passed":[],"failed":[],"baseline":{}}.

    Regression rule: failed now AND passed in the stored baseline.
    """
    baseline = read_json(baseline_path, default={}) or {}
    if not enabled:
        return {"ran": False, "passed": [], "failed": [],
                "baseline": baseline, "regressions": []}
    logging.info("perf: running `just perf` (threshold suite)")
    try:
        r = subprocess.run(  # noqa: S603 - fixed recipe from justfile
            ["just", "perf"], cwd=str(REPO_ROOT), capture_output=True,
            text=True, timeout=timeout_s)
    except subprocess.TimeoutExpired:
        return {"ran": True, "passed": [], "failed": ["<suite-timeout>"],
                "baseline": baseline, "regressions": ["PERF:<suite-timeout>"]}
    passed, failed = [], []
    for line in ((r.stdout or "") + (r.stderr or "")).splitlines():
        m = LINE_RE.match(line)
        if m:
            (passed if m.group(1) == "PASS" else failed).append(m.group(2))
    regressions = [f"PERF:{name}" for name in failed
                   if baseline.get(name) == "pass"]
    result = {"ran": True, "passed": passed, "failed": failed,
              "baseline_passed": [k for k, v in baseline.items() if v == "pass"],
              "regressions": regressions,
              "returncode": r.returncode}
    return result


def save_baseline_from_results(baseline_path: Path, result: dict) -> int:
    """Refresh the baseline to the current pass/fail map."""
    baseline = {name: ("pass" if name in result.get("passed", []) else "fail")
                for name in result.get("passed", []) + result.get("failed", [])}
    atomic_write_json(baseline_path, baseline)
    logging.info("perf: baseline refreshed (%d tests)", len(baseline))
    return len(baseline)


def timing_trends(summary_history: list[dict]) -> list[dict]:
    """Observation-only per-task timing trend across iterations."""
    out: list[dict] = []
    if not summary_history:
        return out
    task_ids = sorted({t["task_id"] for s in summary_history
                       for t in s.get("tasks", [])})
    for tid in task_ids:
        series = [{"iter": s["iter_id"], "wall_s": t.get("wall_s"),
                   "ttft_ms": t.get("ttft_ms"),
                   "tokens": t.get("tokens_used")}
                  for s in summary_history for t in s.get("tasks", [])
                  if t["task_id"] == tid]
        walls = [p["wall_s"] for p in series if p.get("wall_s")]
        if len(walls) >= 2 and walls[-1] > 2 * walls[0]:
            series[-1]["note"] = "wall time 2x first iteration (observe only)"
        out.append({"task_id": tid, "series": series[-5:]})
    return out
