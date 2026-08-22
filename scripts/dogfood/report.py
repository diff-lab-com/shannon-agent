"""Iteration report (report.md) + machine-readable summary.json (plan §5.8)."""

from __future__ import annotations

import logging
from pathlib import Path

from common import atomic_write_json


def build_summary(run_id: str, iter_id: str, metas: list[dict],
                  findings: list[dict], perf_result: dict, gate: dict,
                  ledger_state: dict, fix_sessions: list[dict]) -> dict:
    by_tier: dict[str, dict[str, int]] = {}
    for m in metas:
        tier = m["tier"]
        slot = by_tier.setdefault(tier, {"total": 0, "pass": 0, "partial": 0,
                                         "fail": 0})
        slot["total"] += 1
        slot[m["outcome_grade"]] = slot.get(m["outcome_grade"], 0) + 1
    return {
        "run_id": run_id,
        "iter_id": iter_id,
        "tasks": [{"task_id": m["task_id"], "tier": m["tier"],
                   "grade": m["outcome_grade"], "exit_code": m["exit_code"],
                   "wall_s": m["wall_s"], "ttft_ms": m.get("ttft_ms"),
                   "tokens_used": m.get("tokens_used"),
                   "proc_stats": m.get("proc_stats", {}),
                   "verify_grade": (m.get("verify") or {}).get("grade")}
                  for m in metas],
        "by_tier": by_tier,
        "findings": findings,
        "perf": {k: perf_result.get(k) for k in
                 ("ran", "passed", "failed", "regressions")},
        "gate": gate,
        "budget": ledger_state,
        "fix_sessions": fix_sessions,
        # bool, never the metas list (`all(...) and metas` evaluates to the
        # list — truthy forever — when green).
        "all_green": bool(metas) and all(m["outcome_grade"] == "pass"
                                         for m in metas),
        "new_signatures": [f for f in findings
                           if f.get("status") == "new"
                           and f.get("note") != "expected-failure"],
        "counts": {
            "panics": sum(1 for f in findings if f["category"] == "panic"),
            "hangs": sum(1 for f in findings if f["category"] == "hang"),
            "timeouts": sum(1 for f in findings if f["category"] == "timeout"),
        },
    }


_GRADE_ICON = {"pass": "PASS", "partial": "PART", "fail": "FAIL"}


def _proc_cell(proc_stats: dict) -> str:
    """Compact RSS / threads / IO cell for the report table."""
    if not proc_stats or not proc_stats.get("available"):
        return "-"
    rss_mb = (proc_stats.get("peak_rss_kb") or 0) / 1024
    threads = proc_stats.get("peak_threads") or 0
    io_mb = ((proc_stats.get("io_rbytes") or 0)
             + (proc_stats.get("io_wbytes") or 0)) / (1024 * 1024)
    return f"{rss_mb:.0f}M/{threads}t/{io_mb:.1f}MiB"


def write_report(iter_dir: Path, summary: dict, fix_mode: str) -> Path:
    lines: list[str] = [f"# Dogfood report — {summary['iter_id']}",
                        f"run `{summary['run_id']}` · fix-mode `{fix_mode}`", ""]
    lines += ["## Tasks", "",
              "| task | tier | grade | exit | wall_s | tokens | rss/thr/io | verify |",
              "|------|------|-------|------|--------|--------|------------|--------|"]
    for t in summary["tasks"]:
        lines.append(
            f"| {t['task_id']} | {t['tier']} | {_GRADE_ICON.get(t['grade'], '?')}"
            f" | {t['exit_code']} | {t['wall_s']} | {t.get('tokens_used') or '-'}"
            f" | {_proc_cell(t.get('proc_stats') or {})}"
            f" | {t.get('verify_grade') or '-'} |")
    lines += ["", f"All green: **{summary['all_green']}**"]

    lines += ["", "## Findings (triage)", ""]
    if summary["findings"]:
        lines += ["| status | category | signature | task |", "|---|---|---|---|"]
        for f in summary["findings"]:
            lines.append(f"| {f.get('status', '?')} | {f['category']}"
                         f" | `{f['signature']}` | {f.get('task_id', '')} |")
    else:
        lines.append("(none)")

    if summary["perf"].get("ran"):
        lines += ["", "## Performance (just perf)", "",
                  f"- passed: {len(summary['perf'].get('passed', []))}",
                  f"- failed: {summary['perf'].get('failed', [])}",
                  f"- regressions: {summary['perf'].get('regressions', [])}"]

    # P2-6: highlight tasks whose RSS or IO looks anomalous (peak RSS > 1 GiB
    # or total IO > 256 MiB). Cheap heuristic; surfaces signal in report.md
    # without dumping every task's stats.
    notable = [t for t in summary["tasks"]
               if (t.get("proc_stats") or {}).get("peak_rss_kb", 0) > 1024 * 1024
               or ((t.get("proc_stats") or {}).get("io_rbytes", 0)
                   + (t.get("proc_stats") or {}).get("io_wbytes", 0))
                   > 256 * 1024 * 1024]
    if notable:
        lines += ["", "## Process stats (notable — P2-6)", ""]
        for t in notable:
            ps = t["proc_stats"]
            lines.append(
                f"- `{t['task_id']}`: peak RSS {ps.get('peak_rss_kb', 0)/1024:.0f} MiB, "
                f"peak threads {ps.get('peak_threads', 0)}, "
                f"IO {(ps.get('io_rbytes', 0) + ps.get('io_wbytes', 0))/(1024*1024):.1f} MiB, "
                f"{ps.get('samples', 0)} samples — "
                f"see `{iter_dir}/{t['task_id']}/proc-stats.jsonl`")

    b = summary.get("budget") or {}
    lines += ["", "## Budget (token three-layer gate)", "",
              f"- iteration: {b.get('iteration_used', 0)} used"
              f" / {b.get('iteration_left', '?')} left",
              f"- day: {b.get('day_used', 0)} used",
              f"- month: {b.get('month_used', 0)} used",
              f"- hard_stop: {b.get('hard_stop', False)}"]

    if summary.get("gate"):
        g = summary["gate"]
        lines += ["", "## Gate", "", f"passed: **{g.get('passed')}**"]
        for name, step in (g.get("steps") or {}).items():
            lines.append(f"- {name}: {'ok' if step.get('ok') else 'FAIL'}")

    if summary.get("fix_sessions"):
        lines += ["", "## Fix sessions", ""]
        for s in summary["fix_sessions"]:
            lines.append(f"- `{s.get('brief')}` → ok={s.get('ok')}"
                         f" blocked={s.get('blocked')}"
                         f" tokens={s.get('tokens_in', 0)}+{s.get('tokens_out', 0)}")

    report_path = iter_dir / "report.md"
    report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    atomic_write_json(iter_dir / "summary.json", summary)
    logging.info("report: %s", report_path)
    return report_path
