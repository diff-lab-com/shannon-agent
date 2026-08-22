"""Claude Code fixer: briefs, worktree isolation, headless sessions, gate.

Plan §5.6/§5.7. Fix sessions run inside .dogfood-worktrees/iter-N/ on branch
dogfood/iter-N; the main dev workspace is never touched by the LLM. Output
contract per brief: >=1 `fix(dogfood):` commit + fix-report.md + a regression
test, or a literal `BLOCKED: <reason>` (which routes to the human queue).
"""

from __future__ import annotations

import json
import logging
import re
import subprocess
from pathlib import Path

from common import REPO_ROOT, atomic_write_json, git, now_iso, read_json, run_cmd

# Paths the fixer may touch (plan §5.7 diff audit). Everything else = reject.
DIFF_ALLOWLIST = re.compile(
    r"^(crates/|tests/|docs/|scripts/dogfood/|Cargo\.toml$|Cargo\.lock$|"
    r"justfile$|deny\.toml$|rust-toolchain\.toml$|fix-report\.md$)")
FORBIDDEN_HINTS = (".github/", "scripts/release", "install.sh", "publish")

CLAUDE_SETTINGS = {
    # Minimal settings: no user hooks (the local auto-commit-on-Edit hook must
    # not shred multi-file fixes). The session is confined to the worktree.
    "hooks": {},
    "permissions": {"allow": [], "deny": []},
}


def prepare_worktree(iter_id: str, worktrees_dir: Path) -> Path:
    worktrees_dir.mkdir(parents=True, exist_ok=True)
    wt = worktrees_dir / iter_id
    if (wt / ".git").exists():
        logging.info("fixer: reusing worktree %s", wt)
        # Iteration ids repeat across runs (every --once run starts at
        # iter-01), so a reused worktree can carry a stale base and
        # leftovers from the previous run's session. Refresh to the
        # current HEAD before briefing: fix sessions must patch today's
        # code and the gate must test today's code. Untracked session
        # debris is swept; ignored files (node_modules) survive.
        head = git(REPO_ROOT, "rev-parse", "HEAD").stdout.strip()
        git(wt, "reset", "--hard", head, check=False)
        git(wt, "clean", "-fd", check=False)
        _prepare_worktree_js_deps(wt)
        return wt
    branch = f"dogfood/{iter_id}"
    r = git(REPO_ROOT, "worktree", "add", str(wt), "-b", branch, "HEAD")
    if r.returncode != 0:
        # Branch may exist from an earlier attempt; attach to it.
        r2 = git(REPO_ROOT, "worktree", "add", str(wt), branch)
        if r2.returncode != 0:
            raise RuntimeError(f"worktree add failed: {r.stderr}\n{r2.stderr}")
    _prepare_worktree_js_deps(wt)
    return wt


def _prepare_worktree_js_deps(wt: Path) -> None:
    """Install desktop/ui node_modules so `just ci` (tsc lint) can run.

    A fresh git worktree does not carry gitignored node_modules, and the
    gate's `just ci` fails at `desktop/ui lint` without them (iter-01).
    Best-effort: missing pnpm or a network failure only logs — the gate
    will surface the failure with its own diagnostics.
    """
    ui = wt / "desktop" / "ui"
    if not (ui / "package.json").exists() or (ui / "node_modules").exists():
        return
    logging.info("fixer: installing desktop/ui deps in worktree")
    r = run_cmd(["pnpm", "install", "--prefer-offline"], cwd=ui, timeout_s=600)
    if r.returncode != 0:
        logging.warning("fixer: pnpm install failed (gate may fail on ui lint): %s",
                        (r.stderr or r.stdout)[-300:])


def remove_worktree(wt: Path) -> None:
    git(REPO_ROOT, "worktree", "remove", "--force", str(wt), check=False)
    git(REPO_ROOT, "worktree", "prune", check=False)


# --------------------------------------------------------------------------
# briefs
# --------------------------------------------------------------------------

def brief_header(iter_id: str, worktree: Path) -> str:
    return (
        f"# Dogfood fix brief — {iter_id}\n\n"
        f"You are fixing Shannon (this repo, Rust workspace). Work in the\n"
        f"current directory (git worktree `{worktree}`). Follow CLAUDE.md.\n\n"
        f"## Output contract (validated by the supervisor)\n"
        f"1. At least one commit, message prefixed `fix(dogfood): <signature>`.\n"
        f"2. A `fix-report.md` at the repo root of this worktree: root cause,\n"
        f"   what changed, how verified.\n"
        f"3. A regression test for the fixed signature (CI will run it).\n"
        f"4. If you cannot fix it, output `BLOCKED: <reason>` as the final\n"
        f"   line instead of forcing a patch.\n\n"
        f"## Before finishing — self-check (sessions are rejected without this)\n"
        f"Run `git log --oneline` and `git status`. If your commits do NOT\n"
        f"include one whose message starts with `fix(dogfood):`, fix it now:\n"
        f"`git reset --soft <merge-base with dev> && git commit -m\n"
        f"'fix(dogfood): <signature> <one-line summary>'`. Working commits\n"
        f"with generic messages (e.g. `feat: update X`) are the #1 rejection\n"
        f"cause (iter-01 2026-08-23: a complete fix was rejected solely for\n"
        f"this). Also confirm fix-report.md exists and the diff contains a\n"
        f"`#[test]`.\n\n"
        f"## Constraints\n"
        f"- No public API changes; no new dependencies without justification.\n"
        f"- Only touch crates/ tests/ docs/ (the gate audits the diff).\n"
        f"- Keep the fix minimal and idiomatic with surrounding code.\n"
        f"- Use the built-in Read/Grep/Glob/Edit/Write/Bash tools. MCP plugin\n"
        f"  tools are NOT available in this session; attempts are denied.\n"
    )


def generate_brief(finding: dict, iter_dir: Path, worktree: Path,
                   evidence_paths: list[str]) -> str:
    body = [brief_header(iter_dir.name, worktree)]
    body.append(f"## Signature: `{finding['signature']}`\n")
    body.append(f"- category: {finding['category']}")
    body.append(f"- task: `{finding.get('task_id', 'n/a')}`")
    body.append(f"- status: {finding.get('status', 'open')}\n")
    body.append("## Evidence\n")
    if finding.get("evidence"):
        body.append("```\n" + str(finding["evidence"])[:1500] + "\n```\n")
    body.append("Artifacts (absolute paths; read them for full context):\n")
    for p in evidence_paths:
        body.append(f"- {p}")
    # P3-9: explicit wire-fixture hint so the fixer knows the HTTP frames
    # are available and how to replay them. Without this hint, fixers in
    # past iterations only consulted stdout.ndjson and missed provider-
    # level anomalies (truncated SSE, missing Content-Length, malformed
    # usage blocks) that the wire fixture is the only place to see.
    has_wire = any("/record/" in p or p.endswith("/record") for p in evidence_paths)
    if has_wire:
        body.append("\n## Wire-level evidence (HTTP frames)\n"
                    "The path(s) above under `record/` contain the raw "
                    "provider request/response JSON captured by "
                    "`SHANNON_RECORD_DIR`. Inspect them before guessing — "
                    "they are the only place the actual SSE byte stream "
                    "lives. To replay offline: copy one fixture under "
                    "`tests/fixtures/real_tasks/` and reference it via the "
                    "`record_replay` harness.\n")
    if finding["category"] == "perf_regress":
        body.append("\nReproduce with: `just perf` (failing threshold test is "
                    "in the signature). Localize with "
                    "`cargo bench -p <crate> -- --baseline dogfood` if needed.")
    body.append("\n## Deliverables\nFix the root cause, add the regression "
                "test, write fix-report.md, commit.")
    return "\n".join(body) + "\n"


# --------------------------------------------------------------------------
# sessions
# --------------------------------------------------------------------------

def write_minimal_settings(worktree: Path) -> Path:
    settings = worktree / ".dogfood-claude-settings.json"
    settings.write_text(json.dumps(CLAUDE_SETTINGS, indent=2), encoding="utf-8")
    return settings


def run_session(worktree: Path, brief_path: Path, max_turns: int = 40,
                timeout_s: int = 3600) -> dict:
    """One headless claude session; returns {"ok","blocked","result","usage"}."""
    brief = brief_path.read_text(encoding="utf-8")
    settings = write_minimal_settings(worktree)
    cmd = [
        "claude", "-p", brief,
        "--settings", str(settings),
        # Load ONLY project-level settings: --settings merges with rather
        # than replaces the user layer, so user-level hooks/permissions
        # would leak into the session. (Generic working commits like
        # "feat: update X" observed in iter-01 2026-08-23 were model-
        # authored, not hook-authored — countered by the self-check
        # section in the brief, not by settings.)
        "--setting-sources", "project",
        "--permission-mode", "acceptEdits",
        # Full Bash: iter-01 brief-02 burned all 40 turns on denials —
        # `Bash(cargo *:*,just *)` rejects even `ls <artifacts-dir>`, so the
        # session could not gather evidence. The worktree is the isolation
        # boundary; the gate audits the diff.
        "--allowedTools",
        "Read,Grep,Glob,Edit,Write,Bash",
        # Hide MCP/plugin tools entirely: iter-01 sessions drifted to
        # serena/python_repl/playwright (all denied) and concluded no tools
        # were granted, BLOCKING all three briefs. Without MCP servers the
        # model must use the built-ins allowed above.
        "--strict-mcp-config",
        "--max-turns", str(max_turns),
        "--output-format", "json",
    ]
    logging.info("fixer: session start (%s)", brief_path.name)
    try:
        r = subprocess.run(  # noqa: S603 - fixed argv
            cmd, cwd=str(worktree), capture_output=True, text=True,
            timeout=timeout_s)
    except subprocess.TimeoutExpired:
        return {"ok": False, "blocked": False, "error": "session timeout"}
    out_path = brief_path.parent / (brief_path.stem + ".session.json")
    out_path.write_text(r.stdout or "", encoding="utf-8")
    result_text = ""
    usage = {}
    try:
        doc = json.loads(r.stdout)
        result_text = doc.get("result", "")
        usage = doc.get("usage") or {}
    except (json.JSONDecodeError, TypeError):
        result_text = r.stdout or r.stderr or ""
    blocked = bool(re.search(r"^BLOCKED:", result_text.strip(), re.M))
    return {
        "ok": r.returncode == 0 and not blocked,
        "blocked": blocked,
        "returncode": r.returncode,
        "result_tail": result_text[-1000:],
        "usage": {
            "tokens_in": usage.get("input_tokens", 0),
            "tokens_out": usage.get("output_tokens", 0),
        },
        "session_file": str(out_path),
    }


# --------------------------------------------------------------------------
# contract validation + gate (plan §5.7)
# --------------------------------------------------------------------------

def validate_contract(worktree: Path) -> dict:
    """Check the fix session's deliverables on the worktree branch."""
    base = git(worktree, "merge-base", "HEAD", "dev").stdout.strip() \
        if _branch_exists(worktree, "dev") else "HEAD"
    diff = git(worktree, "diff", "--name-only", base + "..HEAD")
    files = [l.strip() for l in diff.stdout.splitlines() if l.strip()]
    commits = git(worktree, "log", "--oneline",
                  f"{base}..HEAD").stdout.strip().splitlines()
    fix_commits = [c for c in commits if "fix(dogfood):" in c]
    out_of_policy = [f for f in files if not DIFF_ALLOWLIST.match(f)]
    test_added = any("test" in f.lower() for f in files) or _adds_test_call(worktree, base)
    return {
        "commits": commits,
        "fix_commits": fix_commits,
        "files": files,
        "diff_lines": _diff_lines(worktree, base),
        "ok": bool(fix_commits) and not out_of_policy
              and (worktree / "fix-report.md").exists() and test_added,
        "out_of_policy": out_of_policy,
        "fix_report": (worktree / "fix-report.md").exists(),
        "test_added": test_added,
    }


def _branch_exists(repo: Path, name: str) -> bool:
    return git(repo, "rev-parse", "--verify", "--quiet", name,
               check=False).returncode == 0


def _diff_lines(worktree: Path, base: str) -> int:
    r = git(worktree, "diff", "--stat", f"{base}..HEAD")
    m = re.search(r"(\d+) insertion", r.stdout)
    m2 = re.search(r"(\d+) deletion", r.stdout)
    return (int(m.group(1)) if m else 0) + (int(m2.group(1)) if m2 else 0)


def _adds_test_call(worktree: Path, base: str) -> bool:
    diff = git(worktree, "diff", f"{base}..HEAD", "--", "*.rs").stdout
    return "#[test]" in diff or "#[tokio::test]" in diff


def run_gate(worktree: Path, cfg: dict, rerun_failed: list[dict] | None = None,
             shannon_bin_builder=None) -> dict:
    """Full quality gate (plan §5.7). Returns {"passed": bool, "steps": {...}}."""
    steps: dict[str, dict] = {}

    r = run_cmd(["just", "ci"], cwd=worktree, timeout_s=7200)
    steps["just_ci"] = {"ok": r.returncode == 0,
                        "tail": (r.stdout or r.stderr)[-2000:]}
    if r.returncode != 0:
        return {"passed": False, "steps": steps}

    r = run_cmd(["just", "guard-headless"], cwd=worktree, timeout_s=600)
    steps["guard_headless"] = {"ok": r.returncode == 0}
    if r.returncode != 0:
        return {"passed": False, "steps": steps}

    contract = validate_contract(worktree)
    steps["contract"] = {"ok": contract["ok"], "detail": contract}
    if not contract["ok"]:
        return {"passed": False, "steps": steps}

    if rerun_failed and shannon_bin_builder:
        # Rebuild the product binary from the WORKTREE (fix under test) and
        # rerun the failed tasks with identical asserts, N times.
        bin_path = shannon_bin_builder(worktree)
        ok_all = True
        detail = []
        for finding in rerun_failed:
            task = finding.get("task_def")
            if not task:
                continue
            for attempt in range(1, cfg.get("gate", {}).get("rerun_count", 2) + 1):
                from runner import run_task  # late import: run_task needs repo ctx
                meta = run_task(task, worktree / ".dogfood-gate" /
                                f"{task['id']}-attempt{attempt}", bin_path)
                ok = meta["outcome_grade"] == "pass"
                detail.append({"task": task["id"], "attempt": attempt,
                               "grade": meta["outcome_grade"]})
                ok_all = ok_all and ok
        steps["rerun_x2"] = {"ok": ok_all, "detail": detail}
        if not ok_all:
            return {"passed": False, "steps": steps}

    return {"passed": True, "steps": steps}
