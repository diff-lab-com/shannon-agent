#!/usr/bin/env python3
"""Dogfood loop supervisor entry point (plan §4/§5/§6).

Usage (also via `just dogfood ...`):
  run.py --once [--task-filter S] [--task-id s1-...] [--fix-mode manual]
  run.py --gate iter-03            # gate a manually-fixed worktree
  run.py --refresh-perf-baseline
  run.py                           # full loop until a stop condition
"""

from __future__ import annotations

import argparse
import json
import logging
import shutil
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from common import (REPO_ROOT, atomic_write_json, fail, load_tasks, load_yaml,
                    new_run_id, now_iso, read_json, run_cmd, setup_logging)
from ledger import Entry, Ledger
from perf import run_perf, save_baseline_from_results, timing_trends
from report import build_summary, write_report
from runner import run_task
from triage import SignatureTracker, build_fail_signature, classify_task
import fixer

ARTIFACTS_DIR = REPO_ROOT / "artifacts"
WORKTREES_DIR = REPO_ROOT / ".dogfood-worktrees"


# --------------------------------------------------------------------------
# bootstrap
# --------------------------------------------------------------------------

def bootstrap(cfg: dict, fix_mode: str, skip_provider_check: bool) -> Path:
    for tool in ("cargo", "git", "just"):
        if shutil.which(tool) is None:
            fail(f"bootstrap: {tool} not found in PATH")
    shannon_bin = REPO_ROOT / cfg["paths"]["shannon_bin"]
    if not skip_provider_check:
        if not shannon_bin.exists():
            fail(f"bootstrap: {shannon_bin} missing — run a full pass once "
                 f"(without --no-build) to build it")
        r = run_cmd([str(shannon_bin), "list-providers", "--json"],
                    timeout_s=60)
        if r.returncode != 0:
            fail("bootstrap: `shannon list-providers --json` failed — "
                 "configure a provider first (§5.1)")
        try:
            active = json.loads(r.stdout).get("active")
        except ValueError:
            active = None
        if not active:
            fail("bootstrap: no active provider — run `shannon` once and "
                 "configure /provider + credentials (§5.1)")
        logging.info("bootstrap: provider ok (%s)", active)
    if fix_mode == "auto" and shutil.which("claude") is None:
        fail("bootstrap: claude CLI not found (needed for --fix-mode auto)")
    return shannon_bin


def build_product(cfg: dict) -> tuple[bool, str]:
    cmd = [str(c) for c in cfg["build"]["command"]]
    logging.info("build: %s", " ".join(cmd))
    r = run_cmd(cmd, cwd=REPO_ROOT, timeout_s=7200)
    return r.returncode == 0, (r.stdout or "") + (r.stderr or "")


# --------------------------------------------------------------------------
# task matrix execution
# --------------------------------------------------------------------------

def select_tasks(tasks: list[dict], cfg: dict, task_filter: str | None,
                 task_ids: list[str]) -> list[dict]:
    tiers = set(cfg["matrix"]["task_tiers"])
    out = []
    for t in tasks:
        if t.get("tier", "S") not in tiers:
            continue
        if task_filter and t.get("tier") != task_filter:
            continue
        if task_ids and t["id"] not in task_ids:
            continue
        t["expected_failure"] = int((t.get("expect") or {}).get("exit_code", 0)) != 0
        out.append(t)
    return out


def run_matrix(tasks: list[dict], iter_dir: Path, shannon_bin: Path, cfg: dict,
               ledger: Ledger, run_id: str, iter_id: str) -> list[dict]:
    metas: list[dict] = []
    budget = cfg["budget"]
    models = cfg["fixer"]["tier_models"]
    hang_s = cfg["timing"]["hang_no_output_s"]

    def guard(task: dict) -> dict | None:
        g = ledger.check(run_id, iter_id, budget)
        if g["hard_stop"]:
            logging.warning("budget: day/month gate tripped — aborting matrix")
            raise _HardStop(g)
        if g["iteration_exceeded"]:
            logging.warning("budget: iteration gate tripped — skipping %s",
                            task["id"])
            return None
        return run_task(task, iter_dir / task["id"], shannon_bin,
                        model_for_tier=models, hang_s=hang_s)

    s_pool = int(cfg["matrix"]["parallel_s"])
    try:
        for tier, pool_size in (("S", s_pool), ("M", 1), ("L", 1)):
            tier_tasks = [t for t in tasks if t.get("tier") == tier]
            if not tier_tasks:
                continue
            if pool_size <= 1:
                results = [guard(t) for t in tier_tasks]
            else:
                with ThreadPoolExecutor(max_workers=pool_size) as ex:
                    results = list(ex.map(guard, tier_tasks))
            for task, meta in zip(tier_tasks, results):
                if meta is None:
                    meta = _skipped_meta(task, "budget-iteration-gate")
                metas.append(meta)
                terminal = meta.get("terminal") or {}
                # Prefer split in/out from the (current) CLI; fall back to
                # the legacy tokens_used (=in+out) by assigning the whole
                # thing to tokens_out so historical ledger math still works.
                if terminal.get("_has_split"):
                    t_in = int(terminal.get("_tokens_in") or 0)
                    t_out = int(terminal.get("_tokens_out") or 0)
                else:
                    t_in = 0
                    t_out = int(meta.get("tokens_used") or 0)
                ledger.append(Entry(
                    ts=now_iso(), run_id=iter_dir.parent.name, iter_id=iter_id,
                    source="task", ref=task["id"],
                    tokens_in=t_in, tokens_out=t_out,
                    unmetered=meta.get("unmetered", True)))
    except _HardStop:
        pass
    return metas


class _HardStop(Exception):
    def __init__(self, gate: dict):
        super().__init__("hard budget stop")
        self.gate = gate


def _skipped_meta(task: dict, reason: str) -> dict:
    return {"task_id": task["id"], "tier": task.get("tier", "S"),
            "started": False, "outcome_grade": "fail",
            "exit_code": None, "signal": None, "timed_out": False,
            "hang_suspected": False, "wall_s": 0, "ttft_ms": None,
            "ndjson_lines": 0, "terminal": None, "error_events": [],
            "final_text_chars": 0,
            "assert": {"exit_code_ok": False, "terminal_ok": False,
                       "contains_missing": []},
            "verify": {"grade": "skipped", "checks": []},
            "snapshot_files": [], "crash_files": [],
            "tokens_used": None, "unmetered": True,
            "workspace": {"spec": task.get("workspace"), "type": None},
            "skip_reason": reason}


# --------------------------------------------------------------------------
# streaks / stop conditions
# --------------------------------------------------------------------------

def recent_summaries(artifacts_dir: Path, limit: int = 10) -> list[dict]:
    out = []
    if not artifacts_dir.exists():
        return out
    for summary in sorted(artifacts_dir.glob("*/iter-*/summary.json")):
        doc = read_json(summary)
        if doc:
            out.append(doc)
    return out[-limit:]


def history_for_streaks(artifacts_dir: Path, run_id: str, iter_id: str,
                        summary: dict) -> list[dict]:
    """All prior summaries (this run's earlier iterations included) with the
    in-progress summary appended in place.

    The current iteration's summary.json is NOT on disk yet on a fresh run's
    first iteration — an unconditional `[:-1]` slice would silently drop the
    most recent *previous* summary instead (observed: inflated all-green
    streak → premature loop exit). Exclude by (run_id, iter_id), append exact.
    """
    prior = [h for h in recent_summaries(artifacts_dir)
             if (h.get("run_id"), h.get("iter_id")) != (run_id, iter_id)]
    return prior + [summary]


def streaks(history: list[dict], stop_cfg: dict) -> dict:
    green = 0
    for s in reversed(history):
        if s.get("all_green"):
            green += 1
        else:
            break
    no_new = 0
    for s in reversed(history):
        if not s.get("new_signatures"):
            no_new += 1
        else:
            break
    return {"consecutive_all_green": green, "consecutive_no_new": no_new,
            "target_green": stop_cfg["consecutive_all_green"],
            "target_no_new": stop_cfg["consecutive_no_new"]}


# --------------------------------------------------------------------------
# fix stage
# --------------------------------------------------------------------------

def brief_evidence_paths(iter_dir: Path, finding: dict) -> list[str]:
    paths = [str(iter_dir / finding.get("task_id", "n/a") / "meta.json"),
             str(iter_dir / finding.get("task_id", "n/a") / "stdout.ndjson"),
             str(iter_dir / finding.get("task_id", "n/a") / "stderr.log")]
    if finding["category"] == "perf_regress":
        paths.append(str(iter_dir / "perf.json"))
    return [p for p in paths if Path(p).exists()]


def fix_stage(findings: list[dict], cfg: dict, iter_dir: Path, iter_id: str,
              ledger: Ledger, run_id: str, fix_mode: str, auto_merge: bool,
              rerun_defs: list[dict]) -> dict:
    fixable = [f for f in findings if f.get("note") != "expected-failure"]
    if not fixable:
        logging.info("fix: nothing to fix")
        return {"sessions": [], "gate": None, "merged": False}

    worktree = fixer.prepare_worktree(iter_id, WORKTREES_DIR)
    brief_dir = iter_dir / "fix"
    brief_dir.mkdir(parents=True, exist_ok=True)
    sessions: list[dict] = []

    cap = int(cfg["fixer"]["max_sessions_per_iter"])
    # Cluster by signature (one session per finding, capped).
    for i, finding in enumerate(fixable[:cap], 1):
        brief_path = brief_dir / f"brief-{i:02d}.md"
        brief_path.write_text(fixer.generate_brief(
            finding, iter_dir, worktree, brief_evidence_paths(iter_dir, finding)),
            encoding="utf-8")
        if fix_mode == "manual":
            sessions.append({"brief": str(brief_path), "prepared": True})
            continue
        g = ledger.check(run_id, iter_id, cfg["budget"])
        if g["hard_stop"] or g["iteration_exceeded"]:
            logging.warning("fix: budget gate blocks further sessions")
            break
        result = fixer.run_session(worktree, brief_path,
                                   max_turns=cfg["fixer"]["max_turns"],
                                   timeout_s=cfg["fixer"]["session_timeout_s"])
        ledger.append(Entry(ts=now_iso(), run_id=iter_dir.parent.name,
                            iter_id=iter_id, source="fix", ref=f"session-{i}",
                            tokens_in=result["usage"]["tokens_in"],
                            tokens_out=result["usage"]["tokens_out"]))
        sessions.append({"brief": str(brief_path), **{k: result[k] for k in
                                                      ("ok", "blocked",
                                                       "result_tail")}})
        if result["blocked"]:
            logging.info("fix: session BLOCKED -> human queue")

    if fix_mode == "manual":
        print("\n" + "=" * 70)
        print(f"Manual fix mode: {len(sessions)} brief(s) prepared in {brief_dir}")
        print(f"Worktree ready: cd {worktree} && claude")
        print("Fix according to the briefs, commit, then re-run:")
        print(f"  just dogfood --gate {iter_id}")
        print("=" * 70 + "\n")
        return {"sessions": sessions, "gate": None, "merged": False}

    def bin_builder(wt: Path) -> Path:
        r = run_cmd([str(c) for c in cfg["build"]["command"]], cwd=wt,
                    timeout_s=7200)
        if r.returncode != 0:
            raise RuntimeError("worktree build failed")
        return wt / cfg["paths"]["shannon_bin"]

    gate = fixer.run_gate(worktree, cfg, rerun_defs, bin_builder)
    merged = False
    if gate["passed"] and auto_merge:
        # Reuse the contract run_gate already evaluated (avoid a second git
        # diff round-trip) — falls back to a fresh evaluation if the gate
        # skipped the contract step (older gate result, missing key).
        contract = (gate.get("steps", {}).get("contract", {})
                    .get("detail")) or fixer.validate_contract(worktree)
        if contract["diff_lines"] < 500:
            r = run_cmd(["git", "merge", "--no-ff", f"dogfood/{iter_id}",
                         "-m", f"merge: dogfood {iter_id}"],
                        cwd=REPO_ROOT)
            merged = r.returncode == 0
            logging.info("fix: auto-merge %s", "ok" if merged else "FAILED")
        else:
            logging.warning("fix: diff >= 500 lines, auto-merge declined")
    if not merged:
        branch = f"dogfood/{iter_id}"
        logging.info("fix: branch %s ready — open a PR: "
                     "gh pr create --head %s --base dev", branch, branch)
    return {"sessions": sessions, "gate": gate, "merged": merged}


# --------------------------------------------------------------------------
# iteration + main
# --------------------------------------------------------------------------

def run_iteration(n: int, run_id: str, tasks_all: list[dict], cfg: dict,
                  args, ledger: Ledger, tracker: SignatureTracker,
                  shannon_bin: Path) -> dict:
    iter_id = f"iter-{n:02d}"
    iter_dir = ARTIFACTS_DIR / run_id / iter_id
    iter_dir.mkdir(parents=True, exist_ok=True)
    logging.info("=== %s ===", iter_id)

    findings: list[dict] = []
    build_ok, build_log = True, ""
    if not args.no_build:
        build_ok, build_log = build_product(cfg)
        if not build_ok:
            findings.append({"category": "build_fail",
                             "signature": build_fail_signature(build_log),
                             "task_id": "build", "status": "new",
                             "evidence": build_log[-2000:]})

    metas: list[dict] = []
    if build_ok:
        selected = select_tasks(tasks_all, cfg, args.task_filter, args.task_id)
        if not selected:
            fail("no tasks selected (check task_tiers / --task-filter / --task-id)")
        metas = run_matrix(selected, iter_dir, shannon_bin, cfg, ledger, run_id, iter_id)

    for m in metas:
        if m["outcome_grade"] != "pass" or m.get("crash_files"):
            findings += classify_task(m, iter_dir / m["task_id"])

    perf_result = {"ran": False, "passed": [], "failed": [], "regressions": []}
    if build_ok and cfg["perf"]["enabled"]:
        perf_result = run_perf(ARTIFACTS_DIR / "perf-baseline.json")
        atomic_write_json(iter_dir / "perf.json", perf_result)
        for sig in perf_result["regressions"]:
            findings.append({"category": "perf_regress", "signature": sig,
                             "task_id": "perf", "status": "new", "evidence":
                                 f"failed: {perf_result['failed']}"})

    ran = {m["task_id"] for m in metas if m.get("started")}
    passed = {m["task_id"] for m in metas if m["outcome_grade"] == "pass"}
    if perf_result.get("ran"):
        # Perf signatures have task_id "perf"; a clean suite run marks them
        # fixed, a regressed one keeps them open (tracker treats "perf" as
        # ran+not-passed when regressions exist).
        ran.add("perf")
        if not perf_result.get("regressions"):
            passed.add("perf")
    findings = tracker.update(findings, ran, passed, iter_id)
    triage_doc = {"findings": findings,
                  "open_signatures": tracker.open_count()}
    atomic_write_json(iter_dir / "triage.json", triage_doc)

    gate_state = ledger.check(run_id, iter_id, cfg["budget"])
    summary = build_summary(run_id, iter_id, metas, findings, perf_result,
                            gate={}, ledger_state=gate_state, fix_sessions=[])
    history = history_for_streaks(ARTIFACTS_DIR, run_id, iter_id, summary)
    summary["timing_trends"] = timing_trends(
        [{**h, "iter_id": h["iter_id"]} for h in history])
    summary["streaks"] = streaks(history, cfg["stop"])

    fix_out: dict = {"sessions": [], "gate": None, "merged": False}
    if findings and args.fix_mode != "off":
        rerun_defs = [t for t in tasks_all
                      if t["id"] in {f["task_id"] for f in findings}
                      and not t.get("expected_failure")]
        fix_out = fix_stage(findings, cfg, iter_dir, iter_id, ledger, run_id,
                            args.fix_mode, args.auto_merge, rerun_defs)
        summary["gate"] = fix_out["gate"]
        summary["fix_sessions"] = fix_out["sessions"]

    write_report(iter_dir, summary, args.fix_mode)
    print(f"report: {iter_dir / 'report.md'}")
    return summary


def cmd_gate(iter_id: str, cfg: dict, tasks_all: list[dict]) -> None:
    worktree = WORKTREES_DIR / iter_id
    if not (worktree / ".git").exists():
        fail(f"no worktree at {worktree} (manual fix not prepared?)")

    def bin_builder(wt: Path) -> Path:
        r = run_cmd([str(c) for c in cfg["build"]["command"]], cwd=wt,
                    timeout_s=7200)
        if r.returncode != 0:
            raise RuntimeError("worktree build failed")
        return wt / cfg["paths"]["shannon_bin"]

    # Rerun the tasks that had findings in that iteration.
    iter_dirs = sorted(ARTIFACTS_DIR.glob(f"*/{iter_id}"))
    if not iter_dirs:
        fail(f"no artifacts for {iter_id}")
    triage_doc = read_json(iter_dirs[-1] / "triage.json", default={}) or {}
    ids = {f["task_id"] for f in triage_doc.get("findings", [])
           if f.get("task_id") not in ("build", "perf")}
    defs = [t for t in tasks_all if t["id"] in ids]
    gate = fixer.run_gate(worktree, cfg, defs, bin_builder)
    atomic_write_json(iter_dirs[-1] / "gate.json", gate)
    print(json.dumps({"passed": gate["passed"],
                      "steps": {k: v.get("ok") for k, v in gate["steps"].items()}},
                     indent=2))
    if not gate["passed"]:
        sys.exit(1)


def main() -> None:
    ap = argparse.ArgumentParser(description="Shannon dogfood loop supervisor")
    ap.add_argument("--once", action="store_true", help="single iteration")
    ap.add_argument("--max-iters", type=int, default=None)
    ap.add_argument("--task-filter", choices=["S", "M", "L"],
                    help="restrict to one tier (manual override)")
    ap.add_argument("--task-id", action="append", default=[],
                    help="restrict to task id (repeatable)")
    ap.add_argument("--fix-mode", choices=["auto", "manual", "off"],
                    default="auto")
    ap.add_argument("--dry-run", action="store_true",
                    help="alias for --fix-mode manual")
    ap.add_argument("--gate", metavar="ITER",
                    help="run the quality gate on a manually-fixed worktree")
    ap.add_argument("--refresh-perf-baseline", action="store_true")
    ap.add_argument("--auto-merge", action="store_true",
                    help="merge locally when gate passes + diff < 500 lines")
    ap.add_argument("--skip-provider-check", action="store_true",
                    help="bootstrap without provider validation (machinery tests)")
    ap.add_argument("--no-build", action="store_true",
                    help="skip product build (use existing target/release/shannon)")
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()

    setup_logging(args.verbose)
    if args.dry_run:
        args.fix_mode = "manual"

    cfg = load_yaml(Path(__file__).resolve().parent / "lib.yaml")
    tasks_all = load_tasks()
    ARTIFACTS_DIR.mkdir(exist_ok=True)

    if args.refresh_perf_baseline:
        result = run_perf(ARTIFACTS_DIR / "perf-baseline.json", enabled=True,
                          timeout_s=cfg["perf"]["timeout_s"])
        save_baseline_from_results(ARTIFACTS_DIR / "perf-baseline.json", result)
        return
    if args.gate:
        cmd_gate(args.gate, cfg, tasks_all)
        return

    shannon_bin = bootstrap(cfg, args.fix_mode, args.skip_provider_check)
    ledger = Ledger(ARTIFACTS_DIR)
    tracker = SignatureTracker(ARTIFACTS_DIR / "triage-state.json")
    run_id = new_run_id()

    stop = cfg["stop"]
    max_iters = args.max_iters or (1 if args.once else stop["max_iters"])
    exit_reason = "iteration cap (--once/--max-iters)"
    for n in range(1, max_iters + 1):
        if (ARTIFACTS_DIR / "STOP").exists():
            exit_reason = "STOP file"
            break
        if ledger.check(run_id, f"iter-{n:02d}", cfg["budget"])["hard_stop"]:
            exit_reason = "budget day/month gate"
            break
        summary = run_iteration(n, run_id, tasks_all, cfg, args, ledger,
                                tracker, shannon_bin)
        if args.fix_mode == "manual" and summary["findings"]:
            exit_reason = "manual fix handoff"
            break
        s = summary["streaks"]
        if (s["consecutive_all_green"] >= s["target_green"]
                and s["consecutive_no_new"] >= s["target_no_new"]):
            exit_reason = ("quality水位 reached: all-green x"
                           f"{s['target_green']} + no-new x{s['target_no_new']}")
            break
        if ledger.check(run_id, summary["iter_id"], cfg["budget"])["hard_stop"]:
            exit_reason = "budget day/month gate"
            break
        time.sleep(2)

    print(f"\nloop exit: {exit_reason}")
    history = recent_summaries(ARTIFACTS_DIR)
    if history:
        s = streaks(history, cfg["stop"])
        print(f"streaks: all-green x{s['consecutive_all_green']} "
              f"(target {s['target_green']}), "
              f"no-new x{s['consecutive_no_new']} (target {s['target_no_new']})")


if __name__ == "__main__":
    main()
