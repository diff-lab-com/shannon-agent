"""Shared helpers for the dogfood supervisor (stdlib + pyyaml only).

Spec: docs/plans/autonomous-improvement-loop.md. This package is deliberately
outside the Cargo workspace (plan decision 2): no semver/clippy gates apply.
"""

from __future__ import annotations

import json
import logging
import os
import subprocess
import sys
from datetime import datetime
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DOGFOOD_DIR = REPO_ROOT / "scripts" / "dogfood"
TASKS_FILE = REPO_ROOT / "tests" / "dogfood" / "tasks.yaml"
FIXTURES_DIR = REPO_ROOT / "tests" / "dogfood" / "fixtures"

LOG_FMT = "%(asctime)s %(levelname)-7s %(message)s"
LOG_DATE_FMT = "%H:%M:%S"


def setup_logging(verbose: bool = False) -> None:
    logging.basicConfig(
        level=logging.DEBUG if verbose else logging.INFO,
        format=LOG_FMT,
        datefmt=LOG_DATE_FMT,
    )


def load_yaml(path: Path) -> dict:
    try:
        import yaml  # pyyaml is the single third-party dependency
    except ImportError as e:  # pragma: no cover - environment error
        raise SystemExit(
            f"missing dependency: {e}. Install with: pip install pyyaml"
        ) from e
    with open(path, encoding="utf-8") as f:
        data = yaml.safe_load(f) or {}
    if not isinstance(data, dict):
        raise SystemExit(f"{path}: expected a top-level mapping")
    return data


def load_tasks(path: Path = TASKS_FILE) -> list[dict]:
    data = load_yaml(path)
    tasks = data.get("tasks") or []
    ids = [t.get("id") for t in tasks]
    if None in ids or len(ids) != len(set(ids)):
        raise SystemExit(f"{path}: task ids must be unique and non-null")
    return tasks


def atomic_write_json(path: Path, obj) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(obj, f, indent=2, ensure_ascii=False, sort_keys=False)
        f.write("\n")
    os.replace(tmp, path)


def read_json(path: Path, default=None):
    if not path.exists():
        return default
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (json.JSONDecodeError, OSError):
        return default


def now_iso() -> str:
    return datetime.now().astimezone().isoformat(timespec="seconds")


def new_run_id() -> str:
    """Run id used as the artifacts directory name (filesystem-safe)."""
    return datetime.now().astimezone().strftime("%Y-%m-%dT%H%M%S%z")


def day_key() -> str:
    return datetime.now().strftime("%Y-%m-%d")


def month_key() -> str:
    return datetime.now().strftime("%Y-%m")


def run_cmd(
    args: list[str],
    cwd: Path | None = None,
    env: dict | None = None,
    timeout_s: int | None = None,
    check: bool = False,
) -> subprocess.CompletedProcess:
    """Run a command, capturing output; never raises unless check=True."""
    logging.debug("run: %s (cwd=%s)", " ".join(args), cwd)
    merged_env = os.environ.copy()
    if env:
        merged_env.update({k: str(v) for k, v in env.items()})
    return subprocess.run(  # noqa: S603 - args are constructed in this package
        args,
        cwd=str(cwd) if cwd else None,
        env=merged_env,
        timeout=timeout_s,
        capture_output=True,
        text=True,
        check=check,
    )


def git(repo: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess:
    return run_cmd(
        ["git", "-c", "user.email=dogfood@shannon.local", "-c", "user.name=dogfood",
         *args],
        cwd=repo,
        check=check,
    )


def fail(msg: str) -> "NoReturn":  # type: ignore[name-defined]
    logging.error(msg)
    sys.exit(1)
