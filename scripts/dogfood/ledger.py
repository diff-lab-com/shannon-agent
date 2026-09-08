"""Token ledger + three-layer budget gate (plan §6).

Append-only `ledger.jsonl` survives crashes; aggregates are cached in
`ledger-state.json` and rebuildable from the log. Metering sources:
  - task side:     NDJSON terminal {"type":"done"} -> tokens_used
  - fixer side:    claude --output-format json -> usage.input/output_tokens
Gate semantics (check-before-start, never kill in-flight work):
  - per-iteration over budget -> finish the iteration, open nothing new
  - daily/monthly over budget -> stop the whole loop immediately
Overshoot is bounded by the largest single consumer (one fix session).
"""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass, field, asdict
from pathlib import Path

from common import day_key, month_key, read_json, atomic_write_json


@dataclass
class Entry:
    ts: str
    run_id: str
    iter_id: str
    source: str            # "task" | "fix" | "gate"
    ref: str               # task id / session id
    tokens_in: int = 0
    tokens_out: int = 0
    unmetered: bool = False  # true when no terminal usage was available

    @property
    def total(self) -> int:
        return self.tokens_in + self.tokens_out


class Ledger:
    def __init__(self, artifacts_dir: Path):
        self.artifacts_dir = artifacts_dir
        self.log_path = artifacts_dir / "ledger.jsonl"
        self.state_path = artifacts_dir / "ledger-state.json"
        self.entries: list[Entry] = self._load()

    # ---------- persistence ----------

    def _load(self) -> list[Entry]:
        if not self.log_path.exists():
            return []
        out: list[Entry] = []
        with open(self.log_path, encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    out.append(Entry(**json.loads(line)))
                except (json.JSONDecodeError, TypeError):
                    logging.warning("ledger: skipping malformed line")
        return out

    def append(self, entry: Entry) -> None:
        self.entries.append(entry)
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        with open(self.log_path, "a", encoding="utf-8") as f:
            f.write(json.dumps(asdict(entry), ensure_ascii=False) + "\n")
        self._save_state()

    def _save_state(self) -> None:
        state = {
            "totals": {
                # Iteration aggregation must be (run_id, iter_id) keyed: every
                # `--once` invocation restarts at iter-01 and accumulates into
                # the same bucket otherwise, masking the per-iteration budget
                # and corrupting the state file's iter-01 totals.
                "iteration": self._by(lambda e: f"{e.run_id}/{e.iter_id}"),
                "day": self._by(lambda e: e.ts[:10]),
                "month": self._by(lambda e: e.ts[:7]),
            },
            "entries": len(self.entries),
        }
        atomic_write_json(self.state_path, state)

    def _by(self, key) -> dict[str, dict[str, int]]:
        agg: dict[str, dict[str, int]] = {}
        for e in self.entries:
            k = key(e)
            slot = agg.setdefault(k, {"in": 0, "out": 0, "total": 0})
            slot["in"] += e.tokens_in
            slot["out"] += e.tokens_out
            slot["total"] += e.total
        return agg

    # ---------- queries ----------

    def iteration_total(self, run_id: str, iter_id: str) -> int:
        """Sum tokens for one (run_id, iter_id) pair — see _save_state."""
        return sum(e.total for e in self.entries
                   if e.run_id == run_id and e.iter_id == iter_id)

    def day_total(self, day: str | None = None) -> int:
        day = day or day_key()
        return sum(e.total for e in self.entries if e.ts[:10] == day)

    def month_total(self, month: str | None = None) -> int:
        month = month or month_key()
        return sum(e.total for e in self.entries if e.ts[:7] == month)

    # ---------- gate ----------

    def check(self, run_id: str, iter_id: str, budget: dict) -> dict:
        """Return gate state; `hard_stop` true => stop opening
        tasks/sessions (iteration cap => wind down; day/month => hard stop)."""
        it = self.iteration_total(run_id, iter_id)
        day = self.day_total()
        mon = self.month_total()
        return {
            "iteration_used": it,
            "day_used": day,
            "month_used": mon,
            "iteration_left": budget["per_iteration_tokens"] - it,
            "day_left": budget["daily_tokens"] - day,
            "month_left": budget["monthly_tokens"] - mon,
            "iteration_exceeded": it >= budget["per_iteration_tokens"],
            "day_exceeded": day >= budget["daily_tokens"],
            "month_exceeded": mon >= budget["monthly_tokens"],
            "hard_stop": day >= budget["daily_tokens"] or mon >= budget["monthly_tokens"],
        }
