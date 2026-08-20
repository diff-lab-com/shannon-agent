"""Triage: rule-based classification + signature clustering + cross-iteration
tracking (plan §5.5). Crash, outcome and perf findings flow through the same
signature pipeline: same signature fixed once, recurrence escalates (regress).

Signature state persists at artifacts/triage-state.json:
  {signature: {"first_iter", "last_iter", "status": open|fixed,
               "category", "task_id", "count"}}
"""

from __future__ import annotations

import json
import logging
import re
from pathlib import Path

from common import atomic_write_json, read_json

# Categories in rule order (first match wins).
CATEGORIES = [
    "build_fail", "panic", "timeout", "hang", "rate_limited",
    "context_overflow", "permission_denied", "api_error", "turn_limit",
    "bad_output", "outcome_fail", "outcome_partial", "perf_regress",
]


def _normalize_error(msg: str) -> str:
    """Strip volatile bits (numbers, uuids, keys) so the same error clusters."""
    s = re.sub(r"sk-[A-Za-z0-9_\-]+", "sk-<redacted>", msg)
    s = re.sub(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
               "<uuid>", s)
    # Long numbers (ms, sizes) are volatile — except rustc `E0xxx` codes,
    # which are the stable signal for build failures. Short codes like 401
    # stay too.
    s = re.sub(r"(?<!E)\d{4,}", "<n>", s)
    s = re.sub(r"\s+", " ", s).strip()
    return s[:200]


def _panic_location(crash_dir: Path) -> str | None:
    """First shannon frame from the newest crash file's backtrace."""
    files = sorted(crash_dir.glob("*.crash.json")) if crash_dir.exists() else []
    if not files:
        return None
    try:
        doc = json.loads(files[-1].read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return None
    loc = doc.get("location")
    if loc:
        return re.sub(r":\d+:\d+$", "", str(loc))  # file only; lines drift
    for line in str(doc.get("backtrace", "")).splitlines():
        m = re.search(r"at ([^\s(]+\.rs)", line)
        if m and "shannon" in m.group(1):
            return m.group(1)
    return None


def classify_task(meta: dict, task_dir: Path) -> list[dict]:
    """One task's meta -> findings (empty when the task passed)."""
    tid = meta["task_id"]
    expect_rc_note = "expected-failure" if meta.get("expected_failure") else ""
    findings: list[dict] = []
    a = meta.get("assert") or {}

    if meta.get("crash_files"):
        findings.append({"category": "panic",
                         "signature": f"panic:{_panic_location(task_dir / 'crashes') or tid}",
                         "task_id": tid, "evidence": meta["crash_files"]})
    if meta.get("timed_out") or meta.get("signal"):
        findings.append({"category": "timeout",
                         "signature": f"timeout:{tid}",
                         "task_id": tid,
                         "evidence": f"signal={meta.get('signal')} wall={meta.get('wall_s')}s"})
    elif meta.get("hang_suspected"):
        findings.append({"category": "hang", "signature": f"hang:{tid}",
                         "task_id": tid, "evidence": f"silent>{meta.get('wall_s')}s"})

    rc = meta.get("exit_code")
    if not findings and rc is not None:
        if rc == 4:
            findings.append({"category": "rate_limited",
                             "signature": f"rate_limited:{tid}", "task_id": tid,
                             "evidence": (meta.get("error_events") or [""])[0][:200]})
        elif rc == 5:
            findings.append({"category": "context_overflow",
                             "signature": f"ctx_overflow:{tid}", "task_id": tid,
                             "evidence": ""})
        elif rc == 6:
            findings.append({"category": "permission_denied",
                             "signature": f"perm_denied:{tid}", "task_id": tid,
                             "evidence": ""})
        elif rc == 2:
            findings.append({"category": "turn_limit",
                             "signature": f"turn_limit:{tid}", "task_id": tid,
                             "evidence": ""})
        elif rc == 101:
            pass  # panic already captured above; exit 101 alone adds nothing
        elif rc != 0 or not a.get("exit_code_ok"):
            err = _normalize_error((meta.get("error_events") or ["exit code "
                                                                f"{rc}"])[0])
            findings.append({"category": "api_error",
                             "signature": f"api_error:{tid}:{err[:80]}",
                             "task_id": tid, "evidence": err,
                             "note": expect_rc_note})

    if not findings and a.get("contains_missing"):
        findings.append({"category": "bad_output",
                         "signature": f"bad_output:{tid}:contains",
                         "task_id": tid,
                         "evidence": ",".join(a["contains_missing"])[:200]})

    verify = meta.get("verify") or {}
    if not findings and verify.get("grade") == "fail":
        failed = [c["name"] for c in verify.get("checks", []) if not c["ok"]]
        findings.append({"category": "outcome_fail",
                         "signature": f"outcome_fail:{tid}:{','.join(failed)[:60]}",
                         "task_id": tid,
                         "evidence": "; ".join(
                             f"{c['name']}: {c['detail']}" for c in verify["checks"]
                             if not c["ok"])[:300]})
    elif not findings and verify.get("grade") == "partial":
        failed = [c["name"] for c in verify.get("checks", []) if not c["ok"]]
        findings.append({"category": "outcome_partial",
                         "signature": f"outcome_partial:{tid}:{','.join(failed)[:60]}",
                         "task_id": tid, "evidence": ""})

    for f in findings:
        f.setdefault("note", "")
    return findings


def build_fail_signature(build_log: str) -> str:
    for line in build_log.splitlines():
        line = line.strip()
        if line.startswith("error[") or (line.startswith("error:") and "::" not in line):
            return f"build:{_normalize_error(line)[:120]}"
    return "build:unknown-error"


class SignatureTracker:
    """Cross-iteration signature state (new / open / fixed / regressed)."""

    def __init__(self, state_path: Path):
        self.state_path = state_path
        self.state: dict = read_json(state_path, default={}) or {}

    def update(self, findings: list[dict], ran_task_ids: set[str],
               passed_task_ids: set[str], iter_id: str) -> list[dict]:
        """Fold this iteration's findings in; annotate each with its status."""
        seen_now = {f["signature"] for f in findings}
        annotated = []
        for f in findings:
            prev = self.state.get(f["signature"])
            if prev is None:
                status = "new"
            elif prev.get("status") == "fixed":
                status = "regressed"    # fixed before, back again -> red flag
            else:
                status = "open"
            entry = self.state.setdefault(
                f["signature"],
                {"first_iter": iter_id, "last_iter": iter_id,
                 "status": "open", "category": f["category"],
                 "task_id": f["task_id"], "count": 0})
            entry["last_iter"] = iter_id
            entry["count"] += 1
            entry["status"] = "open"
            annotated.append({**f, "status": status})
        # Mark fixed: known-open signatures whose task(s) ran and passed clean.
        for sig, entry in self.state.items():
            if sig in seen_now or entry["status"] != "open":
                continue
            if entry["task_id"] in ran_task_ids and entry["task_id"] in passed_task_ids:
                entry["status"] = "fixed"
                entry["fixed_iter"] = iter_id
        self._save()
        return annotated

    def _save(self) -> None:
        atomic_write_json(self.state_path, self.state)

    def open_count(self) -> int:
        return sum(1 for e in self.state.values() if e.get("status") == "open")
