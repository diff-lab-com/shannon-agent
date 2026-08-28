# v1 Official Baseline — glm-5.3-flash (frozen 2026-08-28)

First citable capability baseline of the Shannon L1 suite (20 tasks) under
fair evaluation semantics (path-normalized trajectory matching, soft
forbidden-tool mode outside recovery, L0-authoritative trajectories).

- Anchor: model=glm-5.3-flash · provider=zhipu-coding-plan · profile=789ca0b964e27af2
- Rules fingerprint: dfcbe82fb06c6c87
- Date: 2026-08-28 · n=3 serial runs
- Runs: 18/20, 20/20, 19/20 (mean 95%); buckets stable_pass=18 flaky=2 stable_fail=0
- Raw runs: ~/.shannon/eval/v2-official/ (anchor mismatch vs later runs ⇒ ATTRIBUTE-SPLIT refuses verdicts)

Citation rules: any reference must carry n / date / anchor triple; single-run
numbers are internal-only; do not drive per-task prompt/harness changes from
this 20-task set (external benchmarks are the held-out probe).
