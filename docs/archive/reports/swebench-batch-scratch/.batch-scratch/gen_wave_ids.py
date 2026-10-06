"""Generate wave1/wave2 gate id files (django 20 / non-django 30) in the out root."""
import pathlib

wt = pathlib.Path(
    "/home/ed/workspace/app/work/shannon/shannon-mono/.claude/worktrees/"
    "agent-a1cc9ba3278f3789a"
)
out = pathlib.Path("/home/ed/.shannon/eval/swe50-n3/.batch-state")
out.mkdir(parents=True, exist_ok=True)

pins = [
    line.split("#")[0].strip()
    for line in (wt / "tests/eval/benchmarks/swebench_verified_50.txt").read_text().splitlines()
    if line.split("#")[0].strip()
]
django = [p for p in pins if p.startswith("django__")]
rest = [p for p in pins if not p.startswith("django__")]
(out / "wave1-ids.txt").write_text("\n".join(django) + "\n")
(out / "wave2-ids.txt").write_text("\n".join(rest) + "\n")
print(f"wave1: {len(django)} ids, wave2: {len(rest)} ids -> {out}")
