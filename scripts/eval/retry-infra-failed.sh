#!/usr/bin/env bash
# retry-infra-failed.sh — extract the task list a previous TB2.1 sweep lost
# to INFRASTRUCTURE (host-network connection failures, verifier uv-bootstrap
# download errors) so it can be re-run separately from real capability
# failures. Evidence: docs/backlog.md §一 — T1 lost 30/61 failures to host
# egress; a manual retry pass recovered 4 tasks that the noise had masked.
#
# Usage:
#   retry-infra-failed.sh --job <harbor-job-dir> [--out <retry-list-file>]
#                         [--all-failed] [--dry-run]
#
#   --job <dir>      Harbor job directory to scan (per-trial result.json)
#   --out <file>     Retry list output (default: <job>/retry-tasks.txt)
#   --all-failed     Include every failed task, not just infra-classified ones
#   --dry-run        Print the classification summary and exit
#
# Exit codes: 0 ok · 1 usage · 2 nothing to retry
#
# The re-run itself is caller-supplied (keeps this script policy-free):
#   harbor run -p /home/ed/datasets/tb21/terminal-bench-2-1 \
#     $(for t in $(cat <retry-list>); do echo -n "-i $t "; done) \
#     -a shannon_harbor_agent:Shannon -m glm-5.3-flash ...
set -euo pipefail

JOB=""; OUT=""; ALL_FAILED=0; DRY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --job) JOB="${2:?}"; shift 2 ;;
    --out) OUT="${2:?}"; shift 2 ;;
    --all-failed) ALL_FAILED=1; shift ;;
    --dry-run) DRY=1; shift ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done
[ -d "$JOB" ] || { echo "job dir not found: $JOB" >&2; exit 1; }
OUT="${OUT:-$JOB/retry-tasks.txt}"

VERIFIER_SIGNS='failed to download|curl: \(|could not resolve|uvx: command not found|no such file or directory: /root/.local/bin'
CONN_SIGNS='error sending request'

python3 - "$JOB" "$OUT" "$VERIFIER_SIGNS" "$CONN_SIGNS" "$ALL_FAILED" "$DRY" <<'PYEOF'
import json, glob, os, re, sys
job, out, vsign, csign, all_failed, dry = (
    sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5] == "1",
    sys.argv[6] == "1")
infra, real = [], []
for f in glob.glob(os.path.join(job, '*', 'result.json')):
    name = os.path.basename(os.path.dirname(f)).split('__')[0]
    try:
        d = json.load(open(f))
    except Exception:
        continue
    reward = ((d.get('verifier_result') or {}).get('rewards') or {}).get('reward')
    if reward == 1.0:
        continue
    msg = str((d.get('exception_info') or {}).get('exception_message', ''))
    if re.search(csign, msg):
        infra.append(name); continue
    ts = os.path.join(os.path.dirname(f), 'verifier', 'test-stdout.txt')
    if os.path.exists(ts):
        tail = open(ts, errors='ignore').read()[-4000:].lower()
        if any(s.lower() in tail for s in vsign.split('|')):
            infra.append(name); continue
    real.append(name)
targets = infra if not all_failed else infra + real
targets = sorted(set(targets))
open(out, 'w').write('\n'.join(targets) + ('\n' if targets else ''))
mode = 'all-failed' if all_failed else 'infra-only'
print(f"[retry-infra] mode={mode} infra={len(infra)} clean={len(real)} "
      f"-> {len(targets)} tasks -> {out}")
if dry:
    for t in targets:
        print("  ", t)
sys.exit(0 if targets or not dry else 2)
PYEOF
