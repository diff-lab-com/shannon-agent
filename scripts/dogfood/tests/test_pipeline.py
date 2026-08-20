"""Unit tests for the dogfood supervisor machinery (no LLM, no network).

Run: `just dogfood-selftest` or
     `python3 -m unittest discover -s scripts/dogfood/tests`
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import tempfile

from ledger import Entry, Ledger
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

    def tearDown(self):
        self.tmp.cleanup()

    def _fill(self):
        self.ledger.append(Entry(ts="2026-08-20T10:00:00", run_id="r1",
                                 iter_id="iter-01", source="task", ref="t1",
                                 tokens_out=1_000_000))
        self.ledger.append(Entry(ts="2026-08-20T11:00:00", run_id="r1",
                                 iter_id="iter-01", source="fix", ref="s1",
                                 tokens_in=500_000, tokens_out=500_000))

    def test_iteration_total_and_gate(self):
        self._fill()
        budget = {"per_iteration_tokens": 4_000_000,
                  "daily_tokens": 10_000_000, "monthly_tokens": 50_000_000}
        g = self.ledger.check("iter-01", budget)
        self.assertEqual(g["iteration_used"], 2_000_000)
        self.assertFalse(g["iteration_exceeded"])
        self.assertFalse(g["hard_stop"])

    def test_iteration_gate_trips_at_cap(self):
        self._fill()
        self.ledger.append(Entry(ts="2026-08-20T12:00:00", run_id="r1",
                                 iter_id="iter-01", source="fix", ref="s2",
                                 tokens_out=2_000_000))
        budget = {"per_iteration_tokens": 4_000_000,
                  "daily_tokens": 10_000_000, "monthly_tokens": 50_000_000}
        g = self.ledger.check("iter-01", budget)
        self.assertTrue(g["iteration_exceeded"])
        self.assertFalse(g["hard_stop"], "iteration cap winds down, not hard stop")

    def test_daily_gate_hard_stops(self):
        self._fill()
        self.ledger.append(Entry(ts="2026-08-20T13:00:00", run_id="r1",
                                 iter_id="iter-02", source="task", ref="t9",
                                 tokens_out=8_000_000))
        budget = {"per_iteration_tokens": 4_000_000,
                  "daily_tokens": 10_000_000, "monthly_tokens": 50_000_000}
        g = self.ledger.check("iter-02", budget)
        self.assertTrue(g["day_exceeded"])
        self.assertTrue(g["hard_stop"])

    def test_month_window_counts_only_same_month(self):
        self._fill()
        self.ledger.append(Entry(ts="2025-01-01T00:00:00", run_id="old",
                                 iter_id="iter-00", source="task", ref="old",
                                 tokens_out=1_000_000))
        self.assertEqual(self.ledger.month_total("2026-08"), 2_000_000)

    def test_persistence_reloads(self):
        self._fill()
        reloaded = Ledger(Path(self.tmp.name))
        self.assertEqual(reloaded.day_total("2026-08-20"), 2_000_000)


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


if __name__ == "__main__":
    unittest.main()
