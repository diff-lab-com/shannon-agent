#!/usr/bin/env python3
"""Turn-economics report for Shannon eval trials (events.jsonl -> summary).

P3 attribution tool: for each trial directory (or a whole jobs dir) produce
per-turn economics — turn count, wall-clock per turn, tokens per turn, tool
call distribution, nudge/continuation/compaction counts — so "turn limit
death: inefficiency or genuine horizon?" can be answered from data instead of
impressions.

Usage:
  turn_economics.py <trial_dir>              # one trial (dir with events.jsonl)
  turn_economics.py <jobs_dir>/*/            # glob of trial dirs

Shannon events consumed (crates/shannon-types/src/session_event.rs):
  turn/start, turn/end (TokenUsage + cost), tool/call, request/header,
  plus A8/A1 marker strings inside Progress events.
Output: one JSON object per trial on stdout (lines), or a cross-trial table
with --table.
"""
from __future__ import annotations

import json
import sys
from collections import Counter
from datetime import datetime
from pathlib import Path


def parse_ts(value) -> float | None:
    if value is None:
        return None
    try:
        return datetime.fromisoformat(str(value).replace("Z", "+00:00")).timestamp()
    except (ValueError, TypeError):
        return None


def find_events_files(trial_dir: Path) -> list[Path]:
    """Locate events.jsonl files for a trial.

    Preferred: the adapter uploads SHANNON_HOME/sessions into
    /logs/agent/shannon-sessions (A7 retries yield several session files —
    all are returned, caller merges). Fallbacks cover local runs and older
    layouts.
    """
    out: list[Path] = []
    sessions = trial_dir / "agent" / "shannon-sessions"
    if sessions.is_dir():
        out.extend(sorted(sessions.rglob("events.jsonl")))
    for candidate in (
        trial_dir / "agent" / "events.jsonl",
        trial_dir / "events.jsonl",
    ):
        if candidate.is_file():
            out.append(candidate)
    return out


def analyze(trial_dir: Path) -> dict:
    events_files = find_events_files(trial_dir)
    if not events_files:
        return {"trial": trial_dir.name, "error": "events.jsonl not found"}
    turns: list[dict] = []
    turn_start_ts: float | None = None
    tools: Counter = Counter()
    n_tool_calls = 0
    tokens_in = tokens_out = 0
    cost_usd = 0.0
    a8_continues = 0
    a1_nudges = 0
    compactions = 0
    first_ts = last_ts = None

    for events_path in events_files:
        with open(events_path, encoding="utf-8", errors="replace") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    ev = json.loads(line)
                except json.JSONDecodeError:
                    continue
                kind = ev.get("type") or ev.get("event") or ""
                ts = parse_ts(ev.get("timestamp") or ev.get("ts"))
                if ts is not None:
                    first_ts = first_ts or ts
                    last_ts = ts
                if kind == "turn/start":
                    turn_start_ts = ts if ts is not None else turn_start_ts
                    turns.append({"tokens_in": 0, "tokens_out": 0, "wall_s": None})
                elif kind == "turn/end":
                    if not turns:
                        turns.append({"tokens_in": 0, "tokens_out": 0, "wall_s": None})
                    cur = turns[-1]
                    usage = ev.get("usage") or {}
                    cur["tokens_in"] = int(usage.get("input_tokens") or 0)
                    cur["tokens_out"] = int(usage.get("output_tokens") or 0)
                    tokens_in += cur["tokens_in"]
                    tokens_out += cur["tokens_out"]
                    cost_usd += float(usage.get("cost_usd") or 0)
                    if ts is not None and turn_start_ts is not None:
                        cur["wall_s"] = round(ts - turn_start_ts, 1)
                    turn_start_ts = ts if ts is not None else turn_start_ts
                elif kind == "tool/call":
                    n_tool_calls += 1
                    tools[str(ev.get("tool") or ev.get("name") or "?")] += 1
                elif kind in ("progress", "warning"):
                    msg = str(ev.get("message") or "")
                    if "continuing turn" in msg:
                        a8_continues += 1
                    elif "nudge" in msg.lower():
                        a1_nudges += 1
                    elif "compact" in msg.lower():
                        compactions += 1

    walls = sorted(t["wall_s"] for t in turns if t["wall_s"] is not None)
    median_wall = walls[len(walls) // 2] if walls else None
    return {
        "trial": trial_dir.name,
        "turns": len(turns),
        "tool_calls": n_tool_calls,
        "top_tools": dict(tools.most_common(6)),
        "tokens_in": tokens_in,
        "tokens_out": tokens_out,
        "cost_usd": round(cost_usd, 4),
        "turn_wall_s": {"median": median_wall, "max": walls[-1] if walls else None},
        "tokens_out_per_turn": round(tokens_out / len(turns), 1) if turns else None,
        "a8_continues": a8_continues,
        "a1_nudges": a1_nudges,
        "compactions": compactions,
        "wall_total_s": round(last_ts - first_ts, 1) if first_ts and last_ts else None,
    }


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    table = "--table" in sys.argv
    if not args:
        print(__doc__)
        return 1
    rows = []
    for pattern in args:
        for d in sorted(Path().glob(pattern)) if any(ch in pattern for ch in "*?") else [Path(pattern)]:
            if not d.is_dir():
                continue
            row = analyze(d)
            rows.append(row)
            if not table:
                print(json.dumps(row, ensure_ascii=False))
    if table:
        cols = ["trial", "turns", "tool_calls", "tokens_in", "tokens_out_per_turn",
                "a8_continues", "a1_nudges", "compactions", "turn_wall_s"]
        print("\t".join(cols))
        for r in rows:
            wall = r.get("turn_wall_s") or {}
            print("\t".join(str(r.get(c)) if c != "turn_wall_s" else
                            f"{wall.get('median')}/{wall.get('max')}" for c in cols))
    return 0


if __name__ == "__main__":
    sys.exit(main())
