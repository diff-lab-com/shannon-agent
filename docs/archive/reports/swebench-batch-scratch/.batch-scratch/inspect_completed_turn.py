"""Find one completed-turn session and print its usage-bearing event shape."""
import glob
import json
import os


def find_sessions(root):
    for path in glob.glob(f"{root}/**/sessions/*/events.jsonl", recursive=True):
        yield path


roots = ["/home/ed/.shannon/eval/v1-regression", "/home/ed/.shannon/eval/tb9-n3"]
shown = 0
for root in roots:
    for path in find_sessions(root):
        kinds = set()
        for line in open(path, encoding="utf-8"):
            try:
                ev = json.loads(line)
            except Exception:
                continue
            kinds.add(ev.get("kind", ""))
        if not any(k.startswith("turn/") and k != "turn/start" for k in kinds):
            continue
        for line in open(path, encoding="utf-8"):
            ev = json.loads(line)
            if ev.get("kind", "").startswith("turn/") and ev.get("kind") != "turn/start":
                if any("token" in k or "usage" in k for k in ev):
                    print("FILE", path)
                    print(json.dumps(ev)[:800])
                    shown += 1
                    break
        if shown >= 2:
            raise SystemExit(0)
print("no completed-usage session found" if shown == 0 else "")
