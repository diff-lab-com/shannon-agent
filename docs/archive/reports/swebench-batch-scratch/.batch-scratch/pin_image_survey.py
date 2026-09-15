"""Survey the 50 SWE pins against the v5-schema parquet: image names, repos, pins coverage."""
import json
import sys

import pyarrow.parquet as pq

PARQUET = sys.argv[1]
PINS = sys.argv[2]
OUT = sys.argv[3]

t = pq.read_table(PARQUET, columns=["instance_id", "image", "repo", "base_commit"])
d = {k: t.column(k).to_pylist() for k in t.column_names}
pins = []
for line in open(PINS, encoding="utf-8"):
    line = line.split("#")[0].strip()
    if line:
        pins.append(line)
print("pins:", len(pins))
missing = [i for i in pins if i not in set(d["instance_id"])]
print("pins missing from parquet:", missing)
idx = {v: i for i, v in enumerate(d["instance_id"])}
imgs = {}
repos = {}
for i in pins:
    j = idx[i]
    imgs.setdefault(d["image"][j], []).append(i)
    repos.setdefault(d["repo"][j], []).append(i)
print("distinct images:", len(imgs))
for r, v in sorted(repos.items()):
    print(f"{r}: {len(v)} pins")
with open(OUT, "w", encoding="utf-8") as fh:
    json.dump(imgs, fh, indent=1)
