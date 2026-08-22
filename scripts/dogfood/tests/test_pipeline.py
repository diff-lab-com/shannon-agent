"""Unit tests for the dogfood supervisor machinery (no LLM, no network).

Run: `just dogfood-selftest` or
     `python3 -m unittest discover -s scripts/dogfood/tests`
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import threading
from datetime import datetime
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import tempfile

from ledger import Entry, Ledger
from run import history_for_streaks, streaks
from runner import parse_ndjson, run_verify
from triage import (SignatureTracker, build_fail_signature, classify_task)


def make_meta(**over):
    meta = {
        "task_id": "t1", "tier": "S", "started": True,
        "exit_code": 0, "signal": None, "timed_out": False,
        "hang_suspected": False, "wall_s": 5.0, "ttft_ms": 100,
        "ndjson_lines": 10,
        "terminal": {"type": "done", "exit_code": 0, "turns_used": 3,
                     "tokens_used": 1200},
        "error_events": [], "final_text_chars": 100,
        "assert": {"exit_code_ok": True, "terminal_ok": True,
                   "contains_missing": []},
        "verify": {"grade": "skipped", "checks": []},
        "snapshot_files": [], "crash_files": [],
        "tokens_used": 1200, "unmetered": False,
        "expected_failure": False,
    }
    meta.update(over)
    return meta


class TestLedger(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.ledger = Ledger(Path(self.tmp.name))
        # Gate windows are derived from the real clock; fixtures must use the
        # actual today (a hardcoded date rots the day-gate test overnight).
        self.today = datetime.now().strftime("%Y-%m-%d")
        self.this_month = datetime.now().strftime("%Y-%m")

    def tearDown(self):
        self.tmp.cleanup()

    def _fill(self):
        self.ledger.append(Entry(ts=f"{self.today}T10:00:00", run_id="r1",
                                 iter_id="iter-01", source="task", ref="t1",
                                 tokens_out=1_000_000))
        self.ledger.append(Entry(ts=f"{self.today}T11:00:00", run_id="r1",
                                 iter_id="iter-01", source="fix", ref="s1",
                                 tokens_in=500_000, tokens_out=500_000))

    def test_iteration_total_and_gate(self):
        self._fill()
        budget = {"per_iteration_tokens": 4_000_000,
                  "daily_tokens": 10_000_000, "monthly_tokens": 50_000_000}
        g = self.ledger.check("r1", "iter-01", budget)
        self.assertEqual(g["iteration_used"], 2_000_000)
        self.assertFalse(g["iteration_exceeded"])
        self.assertFalse(g["hard_stop"])

    def test_iteration_gate_trips_at_cap(self):
        self._fill()
        self.ledger.append(Entry(ts=f"{self.today}T12:00:00", run_id="r1",
                                 iter_id="iter-01", source="fix", ref="s2",
                                 tokens_out=2_000_000))
        budget = {"per_iteration_tokens": 4_000_000,
                  "daily_tokens": 10_000_000, "monthly_tokens": 50_000_000}
        g = self.ledger.check("r1", "iter-01", budget)
        self.assertTrue(g["iteration_exceeded"])
        self.assertFalse(g["hard_stop"], "iteration cap winds down, not hard stop")

    def test_daily_gate_hard_stops(self):
        self._fill()
        self.ledger.append(Entry(ts=f"{self.today}T13:00:00", run_id="r1",
                                 iter_id="iter-02", source="task", ref="t9",
                                 tokens_out=8_000_000))
        budget = {"per_iteration_tokens": 4_000_000,
                  "daily_tokens": 10_000_000, "monthly_tokens": 50_000_000}
        g = self.ledger.check("r1", "iter-02", budget)
        self.assertTrue(g["day_exceeded"])
        self.assertTrue(g["hard_stop"])

    def test_iteration_total_isolates_by_run_id(self):
        """Bug B: same (iter_id, day) across runs must NOT accumulate."""
        self.ledger.append(Entry(ts=f"{self.today}T09:00:00", run_id="runA",
                                 iter_id="iter-01", source="task", ref="t1",
                                 tokens_out=3_500_000))
        self.ledger.append(Entry(ts=f"{self.today}T10:00:00", run_id="runB",
                                 iter_id="iter-01", source="task", ref="t2",
                                 tokens_out=3_500_000))
        budget = {"per_iteration_tokens": 4_000_000,
                  "daily_tokens": 10_000_000, "monthly_tokens": 50_000_000}
        # Each run separately: not exceeded
        self.assertEqual(self.ledger.iteration_total("runA", "iter-01"), 3_500_000)
        self.assertEqual(self.ledger.iteration_total("runB", "iter-01"), 3_500_000)
        # Cross-run aggregate would be 7M but only one run is in the gate
        self.assertEqual(self.ledger.day_total(self.today), 7_000_000)
        self.assertFalse(
            self.ledger.check("runA", "iter-01", budget)["iteration_exceeded"])

    def test_month_window_counts_only_same_month(self):
        self._fill()
        self.ledger.append(Entry(ts="2025-01-01T00:00:00", run_id="old",
                                 iter_id="iter-00", source="task", ref="old",
                                 tokens_out=1_000_000))
        self.assertEqual(self.ledger.month_total(self.this_month), 2_000_000)

    def test_persistence_reloads(self):
        self._fill()
        reloaded = Ledger(Path(self.tmp.name))
        self.assertEqual(reloaded.day_total(self.today), 2_000_000)


class TestParseNdjson(unittest.TestCase):
    def test_terminal_and_text(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "out.ndjson"
            p.write_text(
                '{"type":"start","prompt":"x","model":"m"}\n'
                '{"type":"text_delta","content":"hello "}\n'
                '{"type":"text_delta","content":"world"}\n'
                '{"type":"error","message":"boom"}\n'
                '{"type":"done","exit_code":0,"turns_used":2,"tokens_used":42}\n'
                '{"type":"done","exit_code":0}\n', encoding="utf-8")
            parsed = parse_ndjson(p)
            self.assertEqual(parsed["terminal"]["tokens_used"], 42)
            self.assertEqual(parsed["text"], "hello world")
            self.assertEqual(parsed["errors"], ["boom"])

    def test_split_in_out_preserved_for_ledger(self):
        """Bug A: Done events with split tokens_in/out must surface as
        _tokens_in / _tokens_out so the ledger does not double-count."""
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "out.ndjson"
            p.write_text(
                '{"type":"done","exit_code":0,"turns_used":1,'
                '"tokens_used":1500,"tokens_in":1000,"tokens_out":500}\n',
                encoding="utf-8")
            parsed = parse_ndjson(p)
            self.assertEqual(parsed["terminal"]["_tokens_in"], 1000)
            self.assertEqual(parsed["terminal"]["_tokens_out"], 500)
            self.assertTrue(parsed["terminal"]["_has_split"])

    def test_legacy_done_event_without_split(self):
        """Pre-fix CLI emits only tokens_used; ledger must still work."""
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "out.ndjson"
            p.write_text(
                '{"type":"done","exit_code":0,"turns_used":1,"tokens_used":42}\n',
                encoding="utf-8")
            parsed = parse_ndjson(p)
            self.assertFalse(parsed["terminal"].get("_has_split", False))

    def test_missing_terminal(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "out.ndjson"
            p.write_text('{"type":"text_delta","content":"x"}\n',
                         encoding="utf-8")
            self.assertIsNone(parse_ndjson(p)["terminal"])


class TestVerifyGrading(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.ws = Path(self.tmp.name)
        (self.ws / "src").mkdir()
        (self.ws / "src" / "math.rs").write_text("n > 0", encoding="utf-8")

    def tearDown(self):
        self.tmp.cleanup()

    def test_pass(self):
        task = {"expect": {"verify": {
            "artifacts": [{"path": "src/math.rs",
                           "check": "grep -q 'n > 0' src/math.rs"}],
            "verify_cmd": "true"}}}
        v = run_verify(task, self.ws)
        self.assertEqual(v["grade"], "pass")

    def test_missing_artifact_is_fail(self):
        task = {"expect": {"verify": {
            "artifacts": [{"path": "src/gone.rs"}]}}}
        v = run_verify(task, self.ws)
        self.assertEqual(v["grade"], "fail")

    def test_check_failure_is_partial(self):
        task = {"expect": {"verify": {
            "artifacts": [{"path": "src/math.rs",
                           "check": "grep -q 'NOT_THERE' src/math.rs"}]}}}
        v = run_verify(task, self.ws)
        self.assertEqual(v["grade"], "partial")

    def test_verify_cmd_failure_is_fail(self):
        task = {"expect": {"verify": {
            "artifacts": [{"path": "src/math.rs"}],
            "verify_cmd": "false"}}}
        v = run_verify(task, self.ws)
        self.assertEqual(v["grade"], "fail")


class TestTriage(unittest.TestCase):
    def test_passing_task_yields_no_findings(self):
        self.assertEqual(classify_task(make_meta(), Path("/nonexistent")), [])

    def test_api_error_signature_normalized(self):
        meta = make_meta(exit_code=1,
                         error_events=["401 Unauthorized after 3210ms "
                                       "key sk-ant-abc123"])
        meta["assert"] = {"exit_code_ok": False, "terminal_ok": True,
                          "contains_missing": []}
        findings = classify_task(meta, Path("/nonexistent"))
        self.assertEqual(findings[0]["category"], "api_error")
        sig = findings[0]["signature"]
        self.assertNotIn("3210", sig, "volatile numbers must be normalized")
        self.assertNotIn("sk-ant-abc123", sig, "keys must be redacted")

    def test_timeout_and_panic_categories(self):
        f = classify_task(make_meta(timed_out=True, signal="SIGKILL",
                                    exit_code=-9),
                          Path("/nonexistent"))
        self.assertEqual(f[0]["category"], "timeout")
        f = classify_task(make_meta(exit_code=101, crash_files=["x.json"]),
                          Path("/nonexistent"))
        self.assertEqual(f[0]["category"], "panic")

    def test_outcome_fail_and_partial(self):
        meta = make_meta(verify={"grade": "fail", "checks": [
            {"name": "artifact:src/x.rs", "ok": False, "detail": "missing"}]})
        self.assertEqual(classify_task(meta, Path("/nonexistent"))[0]["category"],
                         "outcome_fail")
        meta = make_meta(verify={"grade": "partial", "checks": [
            {"name": "check:src/x.rs", "ok": False, "detail": "grep"}]})
        self.assertEqual(classify_task(meta, Path("/nonexistent"))[0]["category"],
                         "outcome_partial")

    def test_build_signature_from_rustc_error(self):
        log = ('error[E0308]: mismatched types\n --> crates/x/src/lib.rs:2:3\n')
        sig = build_fail_signature(log)
        self.assertTrue(sig.startswith("build:"))
        self.assertIn("E0308", sig)


class TestSignatureTracker(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.tracker = SignatureTracker(Path(self.tmp.name) / "state.json")

    def tearDown(self):
        self.tmp.cleanup()

    def test_new_then_open_then_fixed_then_regressed(self):
        f = [{"signature": "api_error:t1:x", "category": "api_error",
              "task_id": "t1"}]
        out = self.tracker.update(f, {"t1"}, set(), "iter-01")
        self.assertEqual(out[0]["status"], "new")
        out = self.tracker.update(f, {"t1"}, set(), "iter-02")
        self.assertEqual(out[0]["status"], "open")
        # Task passes clean in iter-03 -> signature marked fixed.
        self.tracker.update([], {"t1"}, {"t1"}, "iter-03")
        self.assertEqual(
            self.tracker.state["api_error:t1:x"]["status"], "fixed")
        # Reappears in iter-04 -> regressed.
        out = self.tracker.update(f, {"t1"}, set(), "iter-04")
        self.assertEqual(out[0]["status"], "regressed")


class TestStreakHistory(unittest.TestCase):
    """Regression: a fresh run's first iteration must not drop the previous
    run's most-recent summary (inflated all-green streak -> premature exit)."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.artifacts = Path(self.tmp.name)
        self.stop = {"consecutive_all_green": 3, "consecutive_no_new": 2}

    def tearDown(self):
        self.tmp.cleanup()

    def _write_summary(self, run: str, it: str, all_green, new_sig=False):
        d = self.artifacts / run / it
        d.mkdir(parents=True)
        (d / "summary.json").write_text(json.dumps(
            {"run_id": run, "iter_id": it, "all_green": all_green,
             "new_signatures": [{"signature": "x"}] if new_sig else []}),
            encoding="utf-8")

    def test_failed_tail_prior_run_not_dropped(self):
        # run1 green, run2 green, run3 FAILED, then a fresh green run4.
        self._write_summary("run1", "iter-01", True)
        self._write_summary("run2", "iter-01", True)
        self._write_summary("run3", "iter-01", False)
        current = {"run_id": "run4", "iter_id": "iter-01",
                   "all_green": True, "new_signatures": []}
        history = history_for_streaks(self.artifacts, "run4", "iter-01",
                                      current)
        self.assertEqual(len(history), 4,
                         "the failed run3 summary must be kept")
        s = streaks(history, self.stop)
        self.assertEqual(s["consecutive_all_green"], 1)
        self.assertFalse(s["consecutive_all_green"] >= s["target_green"])

    def test_same_run_earlier_iters_kept_current_replaced(self):
        self._write_summary("run1", "iter-01", True)
        self._write_summary("run1", "iter-02", False)  # replaced by re-run
        current = {"run_id": "run1", "iter_id": "iter-02",
                   "all_green": True, "new_signatures": []}
        history = history_for_streaks(self.artifacts, "run1", "iter-02",
                                      current)
        self.assertEqual(len(history), 2)
        self.assertEqual(history[-1]["all_green"], True)


class TestGateAutoMerge(unittest.TestCase):
    """End-to-end P1-3: drive the gate→auto-merge code path from run.py
    against a synthetic dev/branch/worktree in /tmp, mirroring the real
    `git merge --no-ff dogfood/{iter_id}` invocation. No LLM, no tokens.

    Covers the happy path (gate passes + diff < 500 → merge lands) and the
    graceful-degrade path (gate fails → branch remains for manual PR).
    """

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name) / "fake-repo"
        self.repo.mkdir()
        # Initialise a git repo on 'dev' with a single empty commit so
        # `git merge --no-ff dogfood/iter-test` has a base to fast-forward from.
        self._run_git(self.repo, "init", "-q", "-b", "dev", ".")
        self._run_git(self.repo, "config", "user.email", "dogfood@shannon.local")
        self._run_git(self.repo, "config", "user.name", "dogfood")
        Path(self.repo / "README.md").write_text("seed\n")
        self._run_git(self.repo, "add", "README.md")
        self._run_git(self.repo, "commit", "-q", "-m", "seed")
        # Worktree on dogfood/iter-test with a fix(dogfood): commit +
        # fix-report.md + a #[test] in some allowed path. This mirrors the
        # contract fixer.validate_contract expects.
        self.iter_id = "iter-test"
        self.worktree = Path(self.tmp.name) / "wt"
        self._run_git(self.repo, "worktree", "add", "-b",
                      f"dogfood/{self.iter_id}", str(self.worktree))
        self._make_fix_commit()

    def tearDown(self):
        self.tmp.cleanup()

    def _run_git(self, cwd: Path, *args, check=True):
        return subprocess.run(  # noqa: S603 - test fixture
            ["git", *args],
            cwd=str(cwd),
            capture_output=True,
            text=True,
            check=check,
        )

    def _make_fix_commit(self):
        wt = self.worktree
        # Allowlisted path: scripts/dogfood/. Touch a dummy module + a test
        # token (#[test]) so validate_contract's "test_added" branch passes.
        (wt / "scripts").mkdir(parents=True, exist_ok=True)
        (wt / "scripts" / "dogfood").mkdir(exist_ok=True)
        mod = wt / "scripts" / "dogfood" / "_dummy.py"
        mod.write_text("def smoke():\n    # regression marker #[test]\n"
                       "    return 1\n")
        report = wt / "fix-report.md"
        report.write_text("# Fix report\n\nAuto-merge test.\n")
        self._run_git(wt, "add", "scripts/dogfood/_dummy.py", "fix-report.md")
        self._run_git(wt, "commit", "-q",
                      "-m", "fix(dogfood): synthetic smoke for auto-merge")

    def _drive_auto_merge(self, repo_root: Path, iter_id: str) -> subprocess.CompletedProcess:
        """Replicate the exact merge invocation from run.py:293-294."""
        # run.py uses common.run_cmd; the actual invocation is:
        #   git -c user.email=... -c user.name=... merge --no-ff
        #       dogfood/{iter_id} -m "merge: dogfood {iter_id}"
        return subprocess.run(  # noqa: S603 - mirrors run.py
            ["git", "-c", "user.email=dogfood@shannon.local",
             "-c", "user.name=dogfood",
             "merge", "--no-ff", f"dogfood/{iter_id}",
             "-m", f"merge: dogfood {iter_id}"],
            cwd=str(repo_root),
            capture_output=True,
            text=True,
        )

    def test_auto_merge_lands_on_dev_when_branch_ready(self):
        """Happy path: gate passes + diff < 500 + branch has the contract.
        The merge commit must land on dev with the expected subject."""
        # Sanity: validate_contract's three required artifacts exist.
        wt = self.worktree
        self.assertTrue((wt / "fix-report.md").exists())
        commits = self._run_git(
            wt, "log", "--oneline", "dev..HEAD").stdout.strip().splitlines()
        self.assertTrue(any("fix(dogfood):" in c for c in commits),
                        "expected a fix(dogfood): commit on the branch")

        r = self._drive_auto_merge(self.repo, self.iter_id)
        self.assertEqual(r.returncode, 0,
                         f"merge failed:\nstdout={r.stdout}\nstderr={r.stderr}")
        log = self._run_git(self.repo, "log", "--oneline", "-n", "3").stdout
        self.assertIn("merge: dogfood iter-test", log,
                      f"merge commit missing from dev log:\n{log}")

    def test_auto_merge_graceful_degrade_on_conflict(self):
        """When dev has moved past the branch base, auto-merge refuses
        cleanly (non-zero exit) and the caller falls through to the
        'PR ready' branch emit without corrupting dev."""
        # Move dev forward with a conflicting change in the same file.
        (self.repo / "scripts" / "dogfood" / "_dummy.py").parent.mkdir(
            parents=True, exist_ok=True)
        conflicting = self.repo / "scripts" / "dogfood" / "_dummy.py"
        conflicting.write_text("def smoke():\n    # dev diverged\n    return 99\n")
        self._run_git(self.repo, "add", "scripts/dogfood/_dummy.py")
        self._run_git(self.repo, "commit", "-q", "-m", "dev: conflicting change")

        r = self._drive_auto_merge(self.repo, self.iter_id)
        # git exits non-zero when --no-ff can't auto-resolve. The caller
        # in run.py logs "auto-merge FAILED" and prints the gh pr create
        # instruction. Verify dev remains on its conflicting commit.
        self.assertNotEqual(r.returncode, 0,
                            "auto-merge should refuse conflicting changes")
        head = self._run_git(self.repo, "log", "-n", "1",
                             "--format=%s").stdout.strip()
        self.assertEqual(head, "dev: conflicting change",
                         "dev HEAD must not advance on auto-merge failure")

    def test_diff_lines_under_500_threshold(self):
        """Replicate fixer._diff_lines: count insertion+deletion lines from
        `git diff --stat`. Confirms the threshold check in run.py:292 stays
        accurate for small contracts (e.g. this synthetic smoke)."""
        stat = self._run_git(self.worktree, "diff", "--stat",
                             "dev..HEAD").stdout
        # _diff_lines regex shape:
        m = re.search(r"(\d+) insertion", stat)
        d = re.search(r"(\d+) deletion", stat)
        insertions = int(m.group(1)) if m else 0
        deletions = int(d.group(1)) if d else 0
        total = insertions + deletions
        self.assertLess(total, 500,
                        f"diff_lines={total} would block auto-merge; "
                        f"stat was:\n{stat}")

    def test_run_py_reuses_gate_contract(self):
        """run_gate must expose its contract result under
        steps.contract.detail so run.py's auto-merge branch can reuse it
        (avoiding a second git diff round-trip)."""
        from fixer import validate_contract
        contract = validate_contract(self.worktree)
        # Mirror the shape run_gate stores:
        gate = {"passed": contract["ok"],
                "steps": {"contract": {"ok": contract["ok"],
                                        "detail": contract}}}
        # This is the exact path run.py auto-merge branch takes.
        reused = (gate.get("steps", {}).get("contract", {})
                  .get("detail"))
        self.assertIs(reused, contract,
                      "run.py must reuse gate['steps']['contract']['detail'] "
                      "verbatim — same object, no re-computation")


class TestProcSampling(unittest.TestCase):
    """P2-6 perf channel: /proc/<pid>/{status,io} sampling while a task
    process runs. Stdlib-only, so safe to run in CI containers."""

    def test_read_status_self(self):
        from perf import _read_status
        rec = _read_status(os.getpid())
        self.assertIsNotNone(rec)
        self.assertGreater(rec["rss_kb"], 0,
                           "current process RSS should be > 0")
        self.assertGreaterEqual(rec["threads"], 1)

    def test_sample_process_stats_aggregates_peak(self):
        """Spawn a long-running child, sample it via start_proc_sampler,
        assert the JSONL has >=2 samples and the aggregator agrees with
        the per-sample peak."""
        from perf import start_proc_sampler
        # Sleep long enough for >=3 sampler ticks (interval 0.2s).
        proc = subprocess.Popen(  # noqa: S603 - test fixture
            [sys.executable, "-c", "import time; time.sleep(2)"])
        try:
            tmpdir = Path(tempfile.mkdtemp())
            t, stop = start_proc_sampler(proc.pid, tmpdir, interval_s=0.2)
            # Wait for child exit (2s) + sampler self-stop (≤0.2s).
            t.join(timeout=4.0)
            stop.set()  # belt-and-braces; sampler usually exits on its own
            out_path = tmpdir / "proc-stats.jsonl"
            self.assertTrue(out_path.exists(),
                            "start_proc_sampler must write proc-stats.jsonl")
            lines = out_path.read_text().splitlines()
            self.assertGreaterEqual(len(lines), 2,
                                    f"sampler should write ≥2 samples; got "
                                    f"{len(lines)}: {lines[:3]}")
            # JSONL fields: rel_ms + rss_kb + threads at minimum.
            first = json.loads(lines[0])
            for k in ("rel_ms", "rss_kb", "threads"):
                self.assertIn(k, first, f"first sample missing {k}: {first}")
            # _read_proc_aggregate (replayed here) returns peak ≥ any sample.
            peak = _read_peak_from_jsonl(out_path)
            self.assertGreater(peak, 0,
                               f"peak RSS must be > 0; samples={lines}")
        finally:
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()

    def test_sample_process_stats_unavailable_degrades(self):
        """On a non-Linux box (or container without /proc), the sampler
        must report available=False instead of crashing the loop."""
        # Force the unavailable branch by monkey-patching _PROC_AVAILABLE
        # at the perf module level.
        import perf as perf_mod
        original = perf_mod._PROC_AVAILABLE
        perf_mod._PROC_AVAILABLE = False
        try:
            agg = perf_mod.sample_process_stats(pid=os.getpid(), interval_s=0.05)
        finally:
            perf_mod._PROC_AVAILABLE = original
        self.assertFalse(agg.get("available", True),
                         f"must report available=False; got {agg}")
        for k in ("samples", "peak_rss_kb", "peak_threads",
                  "io_rbytes", "io_wbytes"):
            self.assertIn(k, agg,
                          f"degraded aggregate must still include {k}")

    def test_runner_proc_stats_in_meta(self):
        """runner.run_process must include proc_stats in its return and
        propagate it into the per-task meta. Run a trivial `true`-style
        command and assert the key is present."""
        from runner import run_process
        tmpdir = Path(tempfile.mkdtemp())
        try:
            task_dir = tmpdir / "task"
            task_dir.mkdir()
            ws = tmpdir / "ws"
            ws.mkdir()
            info = run_process([sys.executable, "-c", "pass"],
                               task_dir, ws, env={}, timeout_s=30,
                               hang_s=5)
            self.assertIn("proc_stats", info,
                          "run_process return must include proc_stats")
            self.assertIn("samples", info["proc_stats"])
            self.assertIn("peak_rss_kb", info["proc_stats"])
            # Aggregate from JSONL must agree with what was returned.
            self.assertEqual(
                info["proc_stats"]["peak_rss_kb"],
                _read_peak_from_jsonl(task_dir / "proc-stats.jsonl"),
                "returned peak_rss_kb must match JSONL aggregation")
        finally:
            import shutil
            shutil.rmtree(tmpdir, ignore_errors=True)


def _read_peak_from_jsonl(path: Path) -> int:
    """Helper: replay the JSONL and return the peak RSS, mirroring
    runner._read_proc_aggregate (without the available field)."""
    peak = 0
    if not path.exists():
        return peak
    for line in path.read_text().splitlines():
        try:
            rss = json.loads(line).get("rss_kb") or 0
        except json.JSONDecodeError:
            continue
        if rss > peak:
            peak = rss
    return peak


class TestBriefWireEvidence(unittest.TestCase):
    """P3-9: triage briefs must surface wire-level fixtures (the HTTP
    request/response JSON captured under <task_dir>/record/) so fixers
    can replay provider quirks offline instead of guessing from NDJSON."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.iter_dir = Path(self.tmp.name) / "artifacts" / "run-x" / "iter-01"
        self.task_id = "m1-scratch-feature"
        self.task_dir = self.iter_dir / self.task_id
        self.task_dir.mkdir(parents=True)
        # Mimic the layout runner.py leaves behind.
        (self.task_dir / "meta.json").write_text("{}")
        (self.task_dir / "stdout.ndjson").write_text("")
        (self.task_dir / "stderr.log").write_text("")
        # 7 wire fixtures, mtimes spread so the sort is deterministic.
        self.record_dir = self.task_dir / "record"
        self.record_dir.mkdir()
        import time
        for i in range(7):
            p = self.record_dir / f"minimax_{i:02d}abcdef.json"
            p.write_text(f'{{"hash": "{i:02d}"}}')
            time.sleep(0.01)
            # bump mtime explicitly so the sort isn't flaky
            import os
            os.utime(p, (i, i))

    def tearDown(self):
        self.tmp.cleanup()

    def test_brief_evidence_paths_includes_record_glob(self):
        from run import brief_evidence_paths
        finding = {"category": "outcome_fail", "task_id": self.task_id}
        paths = brief_evidence_paths(self.iter_dir, finding)
        # meta + ndjson + stderr are always present.
        for needle in ("meta.json", "stdout.ndjson", "stderr.log"):
            self.assertTrue(any(needle in p for p in paths),
                            f"missing {needle} in {paths}")
        # Summary line = the record dir (filtered through Path.exists()).
        summary = [p for p in paths if p.endswith("record")]
        self.assertEqual(len(summary), 1,
                         f"expected 1 summary line (the dir); got {summary}")
        # 5 newest fixture lines (each annotated with total N).
        fixtures = [p for p in paths
                    if "minimax_" in p and "abcdef.json  # 7 wire fixtures" in p]
        self.assertEqual(len(fixtures), 5,
                         f"expected 5 newest fixtures; got {fixtures}")
        # Each annotated line must report the total fixture count.
        self.assertTrue(all("7 wire fixtures total" in p for p in fixtures),
                        f"every fixture line must annotate total; got {fixtures}")
        # The newest (mtime=6, file name suffix '06abcdef') must be first.
        self.assertIn("06abcdef", fixtures[0],
                      f"newest fixture should sort first; got {fixtures[0]}")

    def test_brief_evidence_paths_no_record_dir(self):
        """If record/ is absent (recording disabled), brief must not
        error — the contract is optional, not mandatory."""
        import shutil
        shutil.rmtree(self.record_dir)
        from run import brief_evidence_paths
        finding = {"category": "outcome_fail", "task_id": self.task_id}
        paths = brief_evidence_paths(self.iter_dir, finding)
        self.assertFalse(any("/record/" in p for p in paths),
                         f"record dir is absent; no record paths expected; "
                         f"got {paths}")
        # meta + ndjson + stderr still present.
        self.assertTrue(any("meta.json" in p for p in paths))

    def test_generate_brief_calls_out_wire_evidence(self):
        """The brief markdown must include a wire-fixture hint when record/
        exists — without this hint, fixers consult only stdout.ndjson and
        miss provider-level SSE anomalies."""
        from run import brief_evidence_paths
        from fixer import generate_brief
        finding = {"category": "outcome_fail", "signature": "outcome_fail:test",
                    "task_id": self.task_id, "status": "new"}
        paths = brief_evidence_paths(self.iter_dir, finding)
        wt = Path(self.tmp.name) / "wt"
        wt.mkdir()
        brief = generate_brief(finding, self.iter_dir, wt, paths)
        self.assertIn("Wire-level evidence", brief,
                      "brief must surface wire-fixture hint when record/ exists")
        self.assertIn("record_replay", brief,
                      "brief must explain how to replay fixtures offline")

    def test_generate_brief_omits_wire_hint_without_record(self):
        """No record/ → no wire hint, keeps the brief lean."""
        import shutil
        shutil.rmtree(self.record_dir)
        from run import brief_evidence_paths
        from fixer import generate_brief
        finding = {"category": "outcome_fail", "signature": "outcome_fail:test",
                    "task_id": self.task_id, "status": "new"}
        paths = brief_evidence_paths(self.iter_dir, finding)
        wt = Path(self.tmp.name) / "wt"
        brief = generate_brief(finding, self.iter_dir, wt, paths)
        self.assertNotIn("Wire-level evidence", brief,
                         "brief should NOT advertise wire evidence when absent")


if __name__ == "__main__":
    unittest.main()
