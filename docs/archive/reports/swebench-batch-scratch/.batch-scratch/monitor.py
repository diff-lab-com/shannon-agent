"""batch-3 monitor: per-pin rep status across the out root + live ledger sum.

Reads every bench-* run dir's bench-report.json (final per-round truth) plus
in-flight rep workspaces (verdict.json + session logs) so the ledger stays
visible mid-round, including reps whose harness died without a verdict.
"""
import glob
import json
import pathlib
import sys

OUTROOT = pathlib.Path("/home/ed/.shannon/eval/swe50-n3")


def verdict_sum(ws: pathlib.Path):
    """(tokens_in, tokens_out, seen, resolved) from one rep workspace."""
    ti = to = 0
    seen = False
    for base in (ws / "shannon-home", ws / "sessions"):
        for path in glob.glob(f"{base}/sessions/*/events.jsonl"):
            seen = True
            for line in open(path, encoding="utf-8"):
                try:
                    ev = json.loads(line)
                except Exception:
                    continue
                u = ev.get("usage") or {}
                ti += int(u.get("input_tokens") or 0)
                to += int(u.get("output_tokens") or 0)
    resolved = None
    vpath = ws / "verdict.json"
    if vpath.exists():
        try:
            resolved = bool(json.load(open(vpath, encoding="utf-8")).get("resolved"))
        except Exception:
            resolved = None
    return ti, to, seen, resolved


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else "live"
    total_ti = total_to = 0
    rows = []
    for rundir in sorted(OUTROOT.glob("bench-*")):
        report = None
        for name in ("bench-report.json", "report.json"):
            p = rundir / name
            if p.exists():
                report = json.load(open(p, encoding="utf-8"))
                break
        if report:
            recs = report.get("records") or []
            disp = {}
            for rec in recs:
                for rep in rec.get("reps") or []:
                    d = rep.get("disposition")
                    disp[d] = disp.get(d, 0) + 1
                    em = rep.get("external_metrics") or {}
                    total_ti += int(em.get("tokens_in") or 0)
                    total_to += int(em.get("tokens_out") or 0)
            rows.append(
                f"REPORT {rundir.name}: records={len(recs)} dispositions={disp}"
            )
        # in-flight workspaces (round in progress)
        for ws in sorted(rundir.glob("*_rep*")):
            if not ws.is_dir():
                continue
            if (ws / "bench-report.json").exists():
                continue
            ti, to, seen, resolved = verdict_sum(ws)
            total_ti += ti
            total_to += to
            pin = ws.name.rsplit("_rep", 1)[0]
            rows.append(
                f"LIVE   {rundir.name}/{ws.name}: pin={pin} tin={ti} tout={to} "
                f"seen={int(seen)} verdict={resolved}"
            )
    for r in rows:
        print(r)
    print(f"LEDGER tokens_in={total_ti} tokens_out={total_to} "
          f"billed={total_ti + total_to} / 45000000 "
          f"({(total_ti + total_to) / 450000:.1f}% of budget)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
