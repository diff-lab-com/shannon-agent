"""Performance channel (plan §5.4 性能通道, P1b).

- Decision signal: `just perf` — 12 absolute-threshold tests, mockito-driven,
  independent of live provider latency. A test that passed in the baseline but
  fails now is a perf_regress finding.
- Observation signal: per-task wall time / ttft / turns / tokens trends
  (summary.json); NOT used to open fix briefs.
- Localization tool: criterion benches, run on demand only (never per-iter).
- /proc sampling (P2-6): per-task peak RSS / thread count / IO bytes, sampled
  while the task process runs. Recorded to <task_dir>/proc-stats.jsonl and
  summarised into meta.json. Used to localise where a perf regression comes
  from when `just perf` only tells you "X got slow" without saying why.
"""

from __future__ import annotations

import json
import logging
import os
import re
import subprocess
import threading
import time
from pathlib import Path

from common import REPO_ROOT, atomic_write_json, read_json

# nextest result lines: `PASS [   0.004s] ( 4/13) shannon-core api::tests::x`
# — optional (n/m) progress counter, then a crate-qualified name WITH spaces.
LINE_RE = re.compile(r"^\s*(PASS|FAIL)\s+\[[^\]]*\](?:\s+\([^)]*\))?\s+(.+?)\s*$")

# /proc sampling defaults. 1s is enough to catch slow leaks (e.g. fd/RSS creep
# over a 60-120s task run); finer would inflate proc-stats.jsonl for no gain.
DEFAULT_PROC_SAMPLE_INTERVAL_S = 1.0
# Linux-only feature. When /proc is absent (macOS dev box, CI container without
# procfs), sample_process_stats returns {"available": False, ...} and callers
# degrade gracefully (no row in report.md).
_PROC_AVAILABLE = os.path.exists("/proc/self/status")


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


# --------------------------------------------------------------------------
# /proc sampling (P2-6): localise perf regressions to process state
# --------------------------------------------------------------------------


def _read_status(pid: int) -> dict | None:
    """Parse /proc/<pid>/status into a small dict; None if the process is gone."""
    try:
        with open(f"/proc/{pid}/status", "r") as f:
            data = f.read()
    except (FileNotFoundError, ProcessLookupError, PermissionError):
        return None
    fields: dict[str, int] = {}
    for line in data.splitlines():
        if ":" not in line:
            continue
        key, _, val = line.partition(":")
        # Val shapes: "1234 kB", "5678", " 99 %"
        val = val.strip().split()
        if not val:
            continue
        try:
            fields[key.strip()] = int(val[0])
        except ValueError:
            continue
    rss_kb = fields.get("VmRSS")
    threads = fields.get("Threads")
    return {"rss_kb": rss_kb, "threads": threads}


def _read_io(pid: int) -> dict | None:
    """Parse /proc/<pid>/io (root-readable on most Linux distros); None if absent."""
    try:
        with open(f"/proc/{pid}/io", "r") as f:
            data = f.read()
    except (FileNotFoundError, ProcessLookupError, PermissionError, OSError):
        return None
    out: dict[str, int] = {}
    for line in data.splitlines():
        if ":" not in line:
            continue
        key, _, val = line.partition(":")
        try:
            out[key.strip()] = int(val.strip())
        except ValueError:
            continue
    return out


def sample_process_stats(pid: int, interval_s: float = DEFAULT_PROC_SAMPLE_INTERVAL_S,
                         stop: threading.Event | None = None,
                         out_path: Path | None = None) -> dict:
    """Sample /proc/<pid>/{status,io} until `stop` is set or the pid exits.

    Returns an aggregate dict: peak RSS, peak thread count, total IO bytes,
    number of samples taken. Empty fields when /proc is unavailable so callers
    degrade gracefully on macOS / non-procfs containers.

    If `out_path` is given, raw samples are JSONL-appended there for
    postmortem (one line per sample, rel_ms + rss_kb + threads + io bytes).
    """
    if not _PROC_AVAILABLE:
        return {"available": False, "samples": 0, "peak_rss_kb": 0,
                "peak_threads": 0, "io_rbytes": 0, "io_wbytes": 0}
    stop = stop or threading.Event()
    peak_rss = peak_threads = 0
    io_rbytes = io_wbytes = 0
    samples = 0
    t0 = time.monotonic()
    fh = open(out_path, "a", encoding="utf-8") if out_path else None
    try:
        while not stop.is_set():
            status = _read_status(pid)
            io = _read_io(pid)
            now = time.monotonic() - t0
            if status is None and io is None:
                # Process exited; stop sampling.
                break
            if status:
                rss = status.get("rss_kb") or 0
                threads = status.get("threads") or 0
                peak_rss = max(peak_rss, rss)
                peak_threads = max(peak_threads, threads)
            if io:
                io_rbytes = max(io_rbytes, io.get("read_bytes", 0))
                io_wbytes = max(io_wbytes, io.get("write_bytes", 0))
            if fh is not None and (status or io):
                rec = {"rel_ms": int(now * 1000)}
                if status:
                    rec.update({"rss_kb": status.get("rss_kb"),
                                "threads": status.get("threads")})
                if io:
                    rec.update({"rss_io_r": io.get("read_bytes"),
                                "rss_io_w": io.get("write_bytes")})
                fh.write(json.dumps(rec) + "\n")
                fh.flush()
            samples += 1
            # Wait in small slices so stop event is responsive.
            slept = 0.0
            while slept < interval_s and not stop.is_set():
                time.sleep(min(0.05, interval_s - slept))
                slept += 0.05
    finally:
        if fh is not None:
            fh.close()
    return {"available": True, "samples": samples,
            "peak_rss_kb": peak_rss, "peak_threads": peak_threads,
            "io_rbytes": io_rbytes, "io_wbytes": io_wbytes}


def start_proc_sampler(pid: int, task_dir: Path,
                       interval_s: float = DEFAULT_PROC_SAMPLE_INTERVAL_S
                       ) -> tuple[threading.Thread, threading.Event]:
    """Start a background sampler for `pid`; returns (thread, stop_event).

    Caller MUST set stop_event (and join) once the process exits. Output
    JSONL goes to <task_dir>/proc-stats.jsonl so triage briefs can link
    directly to the sample series.
    """
    task_dir.mkdir(parents=True, exist_ok=True)
    out_path = task_dir / "proc-stats.jsonl"
    # Truncate any stale file from a previous iter's reused task_dir.
    out_path.write_text("")
    stop = threading.Event()

    def _run():
        sample_process_stats(pid, interval_s=interval_s, stop=stop,
                             out_path=out_path)

    t = threading.Thread(target=_run, name=f"proc-sampler-{pid}", daemon=True)
    t.start()
    return t, stop
