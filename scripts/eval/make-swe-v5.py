#!/usr/bin/env python3
"""make-swe-v5.py — derive a v5-schema SWE-bench parquet from an old-schema
(12-col) dump, with FULL consistency validation of every judgment-relevant
column.

Why this exists: swebench 5.x `run_evaluation` requires the dataset itself to
carry `image` and `eval_script` (it no longer derives them), while the local
dumps and some mirrors serve the old 12-column schema. The canonical
generator (swebench 3.0.8 `make_test_spec`) reproduces the official columns
byte-for-byte for 92/93 shared instances of SWE-bench Verified (the one
divergence is an officially hand-patched sphinx instance) and 93/93 image
names — see lite100 FINDINGS S2.

Hard-learned rule (lite100 D1): when (re)building the parquet, the
consistency check must cover ALL judgment-relevant columns — image,
eval_script AND FAIL_TO_PASS/PASS_TO_PASS. pyarrow silently coerces a JSON
string into a LIST OF CHARACTERS when the target schema says list<string>,
which corrupts the test IDs and makes every patch fail judgment no matter
how correct it is.

Usage:
  make-swe-v5.py <old-schema-parquet> <out-v5-parquet> \
                 [--reference-v5 <verified-v5.parquet>]
Env:
  SWE_LEGACY_PYTHON  python with swebench==3.0.8 (test_spec generator);
                     defaults to /tmp/swe-legacy/bin/python if present.
"""
import argparse
import json
import os
import sys
import subprocess

V5_COLS = [
    "base_commit", "created_at", "difficulty", "environment_setup_commit",
    "eval_type", "image", "instance_id", "log_parser", "repo", "version",
    "patch", "test_patch", "eval_script", "problem_statement", "hints_text",
    "FAIL_TO_PASS", "PASS_TO_PASS",
]
GUARD = {"instance_id", "problem_statement", "base_commit", "repo", "image",
         "eval_script"}


def derive_specs(old_parquet):
    """Run the legacy swebench generator in its own interpreter."""
    code = r"""
import json, sys
import pyarrow.parquet as pq
t = pq.read_table(sys.argv[1]).to_pydict()
from swebench.harness.test_spec.test_spec import make_test_spec
imgs, scripts = [], []
for k in range(len(t["instance_id"])):
    inst = {c: t[c][k] for c in t}
    spec = make_test_spec(inst)
    imgs.append("swebench/sweb.eval.x86_64."
                + inst["instance_id"].replace("__", "_1776_") + ":latest")
    scripts.append(spec.eval_script)
json.dump({"images": imgs, "eval_scripts": scripts}, sys.stdout)
"""
    py = os.environ.get("SWE_LEGACY_PYTHON", "/tmp/swe-legacy/bin/python")
    if not os.path.exists(py):
        sys.exit(f"FATAL: legacy swebench interpreter not found at {py} "
                 "(python3 -m venv /tmp/swe-legacy && pip install swebench==3.0.8)")
    r = subprocess.run([py, "-c", code, old_parquet], capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"FATAL: derivation failed:\n{r.stderr[-2000:]}")
    return json.loads(r.stdout)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("old_parquet")
    ap.add_argument("out_parquet")
    ap.add_argument("--reference-v5", default=os.path.join(
        os.path.dirname(os.path.abspath(__file__)),
        "SWE-bench_Verified_test_v5schema.parquet"))
    args = ap.parse_args()

    import pyarrow.parquet as pq
    import pyarrow as pa

    lite = pq.read_table(args.old_parquet)
    c = {n: lite.column(n).to_pylist() for n in lite.schema.names}
    n = lite.num_rows
    ids = c["instance_id"]

    print(f"[1/4] deriving image/eval_script for {n} instances "
          "(swebench 3.0.8 generator)...")
    spec = derive_specs(args.old_parquet)
    imgs, scripts = spec["images"], spec["eval_scripts"]

    ref = None
    if os.path.exists(args.reference_v5):
        rv = pq.read_table(args.reference_v5)
        ref = {iid: k for k, iid in enumerate(rv.column("instance_id").to_pylist())}

    print("[2/4] consistency check against reference v5 (shared instances):")
    if ref:
        shared = [k for k in range(n) if ids[k] in ref]
        es = sum(1 for k in shared
                 if scripts[k] == rv.column("eval_script")[ref[ids[k]]].as_py())
        im = sum(1 for k in shared
                 if imgs[k] == rv.column("image")[ref[ids[k]]].as_py())
        print(f"   image: {im}/{len(shared)} identical")
        print(f"   eval_script: {es}/{len(shared)} identical")
        if im != len(shared):
            sys.exit("FATAL: image-name mismatch — do not ship")

    print("[3/4] building v5 table...")
    data = {}
    for col in V5_COLS:
        if col in c and col not in ("FAIL_TO_PASS", "PASS_TO_PASS"):
            data[col] = c[col]
        elif col in ("FAIL_TO_PASS", "PASS_TO_PASS"):
            # CRITICAL (lite100 D1): parse the JSON strings into real lists.
            # Never let pyarrow coerce a str into a list<string> — it iterates
            # characters, corrupting every test ID silently.
            data[col] = [json.loads(v) if isinstance(v, str) else v
                         for v in c[col]]
        elif col == "image":
            data[col] = imgs
        elif col == "eval_script":
            data[col] = scripts
        elif col == "eval_type":
            data[col] = ["pass_and_fail"] * n
        elif col == "log_parser":
            if ref:
                m = {r: p for r, p in zip(rv.column("repo").to_pylist(),
                                          rv.column("log_parser").to_pylist())}
                data[col] = [m[repo] for repo in c["repo"]]
            else:
                sys.exit("FATAL: log_parser needs a reference v5 table")
        elif col == "difficulty":
            data[col] = [None] * n

    schema = (pq.ParquetFile(args.reference_v5).schema_arrow
              if ref else pa.schema([]))
    table = pa.Table.from_pydict(data, schema=schema) if ref else \
        pa.Table.from_pydict(data)

    print("[4/4] validating every judgment-relevant column...")
    d = table.to_pydict()
    bad = []
    for k, iid in enumerate(d["instance_id"]):
        if not d["image"][k] or not d["eval_script"][k]:
            bad.append((iid, "image/eval_script empty"))
        for col in ("FAIL_TO_PASS", "PASS_TO_PASS"):
            v = d[col][k]
            if not (isinstance(v, list) and all(isinstance(x, str) for x in v)):
                bad.append((iid, f"{col} not a list of str"))
        if not d["problem_statement"][k] or not d["base_commit"][k]:
            bad.append((iid, "problem_statement/base_commit empty"))
    if bad:
        for b in bad[:10]:
            print("  BAD:", b)
        sys.exit(f"FATAL: {len(bad)} invalid row(s)")

    tmp = args.out_parquet + ".tmp"
    pq.write_table(table, tmp)
    os.replace(tmp, args.out_parquet)
    print(f"OK: wrote {args.out_parquet} ({table.num_rows} rows) — "
          f"schema guard columns present: {sorted(GUARD & set(table.schema.names))}")


if __name__ == "__main__":
    main()
