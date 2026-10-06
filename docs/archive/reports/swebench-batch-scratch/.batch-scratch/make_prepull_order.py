"""Build the pre-pull order: wave-1 django pins first (pin-file order), then the rest."""
import json

imgs = json.load(open("/tmp/swe50-images.json", encoding="utf-8"))
by_pin = {}
for image, pins in imgs.items():
    for p in pins:
        by_pin[p] = image

pin_file = (
    "/home/ed/workspace/app/work/shannon/shannon-mono/.claude/worktrees/"
    "agent-a1cc9ba3278f3789a/tests/eval/benchmarks/swebench_verified_50.txt"
)
pins = [
    line.split("#")[0].strip()
    for line in open(pin_file, encoding="utf-8")
    if line.split("#")[0].strip()
]
django = [p for p in pins if p.startswith("django__")]
rest = [p for p in pins if not p.startswith("django__")]
order = [by_pin[p] for p in django + rest]
with open(
    "/home/ed/workspace/app/work/shannon/shannon-mono/.claude/worktrees/"
    "agent-a1cc9ba3278f3789a/.batch-scratch/prepull-order.txt",
    "w",
    encoding="utf-8",
) as fh:
    fh.write("\n".join(order) + "\n")
print(f"{len(order)} images ordered (django first: {len(django)})")
