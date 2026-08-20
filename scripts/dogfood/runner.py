"""Task runner: workspaces, process groups, NDJSON capture, verdicts.

Plan §5.3/§5.4. Every task gets `artifacts/<run>/<iter>/<task>/` with
stdout.ndjson (+ events-index.jsonl timing sidecar), stderr.log, meta.json,
workspace-snapshot/. Exit codes are read from the Popen handle (never through
a pipe, per the known piped-exit-code pitfall).
"""

from __future__ import annotations

import json
import logging
import os
import shutil
import signal
import subprocess
import threading
import time
from pathlib import Path

from common import REPO_ROOT, FIXTURES_DIR, git, run_cmd

VERIFY_CMD_TIMEOUT_S = 600
CHECK_CMD_TIMEOUT_S = 120
# No NDJSON line for this long => suspected hang. Generous on purpose: a long
# LLM turn can legitimately stay silent for minutes before the first delta.
HANG_NO_OUTPUT_S = 900
KILL_GRACE_S = 10

WORKSPACE_READONLY = "readonly-main"


class TaskFailure(Exception):
    """Runner-level failure (bad workspace spec, fixture missing, ...)."""


# --------------------------------------------------------------------------
# workspace materialization
# --------------------------------------------------------------------------

def _init_ws_repo(ws: Path) -> None:
    git(ws, "init", "--quiet", check=False)
    git(ws, "add", "-A", check=False)
    git(ws, "commit", "--quiet", "-m", "dogfood workspace base", check=False)


def materialize_workspace(task: dict, ws: Path) -> dict:
    """Create the task workspace at `ws`; returns {"type":..., "cleanup": bool}."""
    spec = task.get("workspace", "temp")
    if spec == WORKSPACE_READONLY:
        r = git(REPO_ROOT, "worktree", "add", "--detach", str(ws), "HEAD")
        if r.returncode != 0:
            raise TaskFailure(f"worktree add failed: {r.stderr.strip()}")
        return {"type": "readonly-main", "cleanup": True}
    if spec.startswith("scratch:"):
        fixture = FIXTURES_DIR / spec.split(":", 1)[1]
        if not fixture.is_dir():
            raise TaskFailure(f"unknown scratch fixture: {fixture}")
        shutil.copytree(fixture, ws)
        _init_ws_repo(ws)
        return {"type": "scratch", "cleanup": True}
    if spec == "temp":
        ws.mkdir(parents=True, exist_ok=True)
        _init_ws_repo(ws)
        return {"type": "temp", "cleanup": True}
    raise TaskFailure(f"unknown workspace spec: {spec!r}")


def release_workspace(ws: Path, wsinfo: dict) -> None:
    if not wsinfo.get("cleanup") or not ws.exists():
        return
    if wsinfo["type"] == WORKSPACE_READONLY:
        git(REPO_ROOT, "worktree", "remove", "--force", str(ws), check=False)
        git(REPO_ROOT, "worktree", "prune", check=False)
    else:
        shutil.rmtree(ws, ignore_errors=True)


# --------------------------------------------------------------------------
# process execution
# --------------------------------------------------------------------------

def _stream_reader(stream, out_path: Path, index_path: Path, t0: float,
                   state: dict) -> None:
    """Drain child stdout: raw lines to file, timing index as sidecar."""
    with open(out_path, "wb") as out, open(index_path, "w", encoding="utf-8") as idx:
        for raw in iter(stream.readline, b""):
            rel_ms = int((time.monotonic() - t0) * 1000)
            out.write(raw)
            out.flush()
            try:
                event = json.loads(raw)
                etype = event.get("type", "?")
            except (json.JSONDecodeError, UnicodeDecodeError):
                etype = "?"
            idx.write(json.dumps({"rel_ms": rel_ms, "type": etype}) + "\n")
            state["last_line_ms"] = rel_ms
            state["lines"] += 1
        state["eof"] = True


def run_process(cmd: list[str], task_dir: Path, ws: Path, env: dict,
                timeout_s: int, hang_s: int = HANG_NO_OUTPUT_S) -> dict:
    """Run one headless process; returns {"exit_code","signal","timed_out","hang"}."""
    stdout_path = task_dir / "stdout.ndjson"
    stderr_path = task_dir / "stderr.log"
    index_path = task_dir / "events-index.jsonl"
    state = {"last_line_ms": 0, "lines": 0, "eof": False}
    t0 = time.monotonic()

    merged_env = os.environ.copy()
    merged_env.update({k: str(v) for k, v in env.items()})

    with open(stderr_path, "wb") as errf:
        proc = subprocess.Popen(  # noqa: S603 - cmd built by this package
            cmd, cwd=str(ws), env=merged_env,
            stdout=subprocess.PIPE, stderr=errf,
            start_new_session=True,  # own process group: kill the whole tree
        )
        reader = threading.Thread(
            target=_stream_reader,
            args=(proc.stdout, stdout_path, index_path, t0, state),
            daemon=True,
        )
        reader.start()

        deadline = t0 + timeout_s
        timed_out = hang = False
        while True:
            rc = proc.poll()
            if rc is not None:
                break
            now = time.monotonic()
            if now >= deadline:
                timed_out = True
                break
            silent_s = now - t0 - state["last_line_ms"] / 1000
            if (state["lines"] > 0 and silent_s > hang_s
                    or state["lines"] == 0 and now - t0 > hang_s):
                hang = True
                break
            time.sleep(0.5)

        if timed_out or hang:
            _signal_group(proc, signal.SIGINT)
            try:
                proc.wait(timeout=KILL_GRACE_S)
            except subprocess.TimeoutExpired:
                _signal_group(proc, signal.SIGKILL)
                proc.wait()
        reader.join(timeout=5)
        time.sleep(0.2)

    rc = proc.returncode
    sig = None
    if rc is not None and rc < 0:
        sig = signal.Signals(-rc).name
    return {
        "exit_code": rc,
        "signal": sig,
        "timed_out": timed_out,
        "hang_suspected": hang,
        "wall_s": round(time.monotonic() - t0, 1),
        "ttft_ms": _first_event_ms(index_path),
    }


def _signal_group(proc: subprocess.Popen, sig: int) -> None:
    try:
        os.killpg(os.getpgid(proc.pid), sig)
    except (ProcessLookupError, PermissionError):
        proc.send_signal(sig)


def _first_event_ms(index_path: Path) -> int | None:
    try:
        with open(index_path, encoding="utf-8") as f:
            for line in f:
                return json.loads(line).get("rel_ms")
    except (OSError, json.JSONDecodeError, StopIteration):
        return None


# --------------------------------------------------------------------------
# NDJSON parsing
# --------------------------------------------------------------------------

def parse_ndjson(path: Path) -> dict:
    """Extract terminal done event, error events, and concatenated text."""
    terminal, errors, text = None, [], []
    if not path.exists():
        return {"terminal": None, "errors": [], "text": ""}
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                ev = json.loads(line)
            except json.JSONDecodeError:
                continue
            etype = ev.get("type")
            if etype == "done" and "tokens_used" in ev:
                terminal = ev  # CiEvent::Done carries the token/turn summary
            elif etype == "done":
                terminal = terminal or ev
            elif etype == "error":
                errors.append(ev.get("message", "")[:500])
            elif etype == "text_delta":
                text.append(ev.get("content", ""))
    return {"terminal": terminal, "errors": errors, "text": "".join(text)}


# --------------------------------------------------------------------------
# verify block (plan §5.2)
# --------------------------------------------------------------------------

def run_verify(task: dict, ws: Path) -> dict:
    """Grade the task's products. pass | partial | fail | skipped."""
    verify = (task.get("expect") or {}).get("verify")
    if not verify:
        return {"grade": "skipped", "checks": []}
    checks: list[dict] = []
    grade = "pass"

    for art in verify.get("artifacts") or []:
        path = ws / art["path"]
        if not path.exists():
            checks.append({"name": f"artifact:{art['path']}", "ok": False,
                           "detail": "missing"})
            grade = "fail"
            continue
        if art.get("check"):
            r = run_cmd(["bash", "-c", art["check"]], cwd=ws,
                        timeout_s=CHECK_CMD_TIMEOUT_S)
            ok = r.returncode == 0
            checks.append({"name": f"check:{art['path']}", "ok": ok,
                           "detail": (r.stderr or r.stdout).strip()[:300]})
            if not ok and grade == "pass":
                grade = "partial"   # exists but content check failed

    if verify.get("golden"):
        golden = Path(verify["golden"])
        target = ws / str(verify.get("golden_target", "OUTPUT.txt"))
        if golden.exists() and target.exists():
            norm = lambda s: " ".join(s.split())
            if norm(golden.read_text(encoding="utf-8")) == norm(
                    target.read_text(encoding="utf-8")):
                checks.append({"name": "golden", "ok": True, "detail": ""})
            else:
                checks.append({"name": "golden", "ok": False, "detail": "diff"})
                grade = "fail"
        else:
            checks.append({"name": "golden", "ok": False, "detail": "missing file"})
            grade = "fail"

    if verify.get("verify_cmd"):
        r = run_cmd(["bash", "-c", verify["verify_cmd"]], cwd=ws,
                    timeout_s=VERIFY_CMD_TIMEOUT_S)
        ok = r.returncode == 0
        tail = ((r.stdout or "") + (r.stderr or ""))[-500:]
        checks.append({"name": "verify_cmd", "ok": ok,
                       "detail": tail.strip()[:300]})
        if not ok:
            grade = "fail"          # deliverable exists but does not work

    return {"grade": grade, "checks": checks}


# --------------------------------------------------------------------------
# workspace snapshot (plan §5.4 产物通道)
# --------------------------------------------------------------------------

def snapshot_workspace(ws: Path, snap_dir: Path, max_bytes: int = 5_000_000) -> dict:
    """Copy git-visible changes (new/modified files) into the snapshot dir."""
    snap_dir.mkdir(parents=True, exist_ok=True)
    if not (ws / ".git").exists():
        return {"files": [], "note": "workspace not a git repo"}
    r = git(ws, "status", "--porcelain")
    files: list[str] = []
    for line in r.stdout.splitlines():
        rel = line[3:].strip().strip('"')
        if not rel or "->" in rel:
            continue
        src = ws / rel
        if not src.is_file():
            continue
        try:
            if src.stat().st_size > max_bytes:
                continue
            dst = snap_dir / rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst)
            files.append(rel)
        except OSError as e:
            logging.debug("snapshot skip %s: %s", rel, e)
    (snap_dir / ".status").write_text(r.stdout, encoding="utf-8")
    return {"files": files}


# --------------------------------------------------------------------------
# top-level task entry
# --------------------------------------------------------------------------

def run_task(task: dict, task_dir: Path, shannon_bin: Path,
             model_for_tier: dict | None = None,
             extra_env: dict | None = None,
             hang_s: int = HANG_NO_OUTPUT_S) -> dict:
    """Execute one task end-to-end; returns meta dict (also written to disk)."""
    task_dir.mkdir(parents=True, exist_ok=True)
    ws = task_dir / "ws"
    crash_dir = task_dir / "crashes"
    crash_dir.mkdir(exist_ok=True)

    wsinfo = materialize_workspace(task, ws)

    cmd = [str(shannon_bin), "-p", task["prompt"],
           "--output-format", "json-stream"]
    if task.get("allowed_tools") is not None:
        cmd += ["--allowed-tools", ",".join(task["allowed_tools"])]
    if task.get("max_turns"):
        cmd += ["--max-turns", str(task["max_turns"])]
    if task.get("schema"):
        cmd += ["--schema", str(task["schema"])]

    env = {
        "RUST_LOG": "shannon_cli=info,shannon_core=info",
        "SHANNON_CRASH_DIR": str(crash_dir),
    }
    tier_model = (model_for_tier or {}).get(task.get("provider_tier", ""))
    if tier_model:
        env["SHANNON_MODEL"] = str(tier_model)
    env.update(task.get("env") or {})
    env.update(extra_env or {})

    try:
        proc_info = run_process(cmd, task_dir, ws, env,
                                int(task.get("timeout_s", 600)),
                                hang_s=int(hang_s))
        parsed = parse_ndjson(task_dir / "stdout.ndjson")
        verify = run_verify(task, ws)
        snap = snapshot_workspace(ws, task_dir / "workspace-snapshot")

        expect = task.get("expect") or {}
        want_rc = int(expect.get("exit_code", 0))
        got_rc = proc_info["exit_code"]
        exit_ok = got_rc == want_rc
        terminal = parsed["terminal"]
        terminal_ok = bool(terminal) if expect.get("ndjson_terminal", True) else True
        text = parsed["text"]
        missing = [c for c in (expect.get("contains") or []) if c not in text]

        if not exit_ok or not terminal_ok or missing:
            grade = "fail"
        else:
            grade = verify["grade"] if verify["grade"] != "skipped" else "pass"

        meta = {
            "task_id": task["id"], "tier": task.get("tier", "S"),
            "workspace": {"spec": task.get("workspace"), "type": wsinfo["type"]},
            "cmd": cmd, "env": {k: v for k, v in env.items()
                                if k != "SHANNON_API_KEY"},
            "started": True,
            "exit_code": got_rc, "signal": proc_info["signal"],
            "timed_out": proc_info["timed_out"],
            "hang_suspected": proc_info["hang_suspected"],
            "wall_s": proc_info["wall_s"], "ttft_ms": proc_info["ttft_ms"],
            "ndjson_lines": _count_lines(task_dir / "stdout.ndjson"),
            "terminal": terminal,
            "error_events": parsed["errors"],
            "final_text_chars": len(text),
            "assert": {"exit_code_ok": exit_ok, "terminal_ok": terminal_ok,
                       "contains_missing": missing},
            "verify": verify,
            "snapshot_files": snap["files"],
            "crash_files": [p.name for p in crash_dir.glob("*.crash.json")],
            "tokens_used": (terminal or {}).get("tokens_used"),
            "unmetered": terminal is None or "tokens_used" not in (terminal or {}),
            "outcome_grade": grade,
        }
    finally:
        release_workspace(ws, wsinfo)

    from common import atomic_write_json
    atomic_write_json(task_dir / "meta.json", meta)
    return meta


def _count_lines(path: Path) -> int:
    try:
        with open(path, "rb") as f:
            return sum(1 for _ in f)
    except OSError:
        return 0
