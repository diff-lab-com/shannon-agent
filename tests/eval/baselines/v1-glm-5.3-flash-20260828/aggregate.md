# Shannon Eval Aggregate — 3 runs

- Runs: 20260828113352-caf9461d → 20260828115003-254e91ba → 20260828115952-f597a2b3
- Anchor: model=glm-5.3-flash · provider=zhipu-coding-plan · profile=789ca0b964e27af2
- Rules fingerprint: dfcbe82fb06c6c87
- Engine versions: 0.11.0, 0.11.0, 0.11.0
- Task set consistent: yes

## Suite summary

| bucket | tasks |
|---|---|
| stable_pass | 18 |
| flaky | 2 |
| stable_fail | 0 |

- Pass-rate conclusions cite the stable scope only; flaky tasks are quarantined below (suspected, not convicted).

- pass_rate interval (all tasks, noisy view): [0.900, 1.000] · n=3
- stable pass-rate interval (main conclusion — cite this): [1.000, 1.000] · n=3

## Per-task verdicts (resolved k/n)

| id | tier | k/n | bucket | statuses (run order) |
|---|---|---|---|---|
| edit_01 | edit | 3/3 | stable_pass | passed, passed, passed |
| edit_02 | edit | 3/3 | stable_pass | passed, passed, passed |
| edit_03 | edit | 3/3 | stable_pass | passed, passed, passed |
| edit_04 | edit | 3/3 | stable_pass | passed, passed, passed |
| edit_05 | edit | 3/3 | stable_pass | passed, passed, passed |
| multi_01 | multi_step | 3/3 | stable_pass | passed, passed, passed |
| multi_02 | multi_step | 3/3 | stable_pass | passed, passed, passed |
| multi_03 | multi_step | 3/3 | stable_pass | passed, passed, passed |
| multi_04 | multi_step | 3/3 | stable_pass | passed, passed, passed |
| multi_05 | multi_step | 3/3 | stable_pass | passed, passed, passed |
| multi_06 | multi_step | 3/3 | stable_pass | passed, passed, passed |
| read_01 | read | 3/3 | stable_pass | passed, passed, passed |
| read_02 | read | 3/3 | stable_pass | passed, passed, passed |
| read_03 | read | 3/3 | stable_pass | passed, passed, passed |
| rec_01 | recovery | 3/3 | stable_pass | passed, passed, passed |
| rec_02 | recovery | 1/3 | flaky | failed, passed, failed |
| rec_03 | recovery | 2/3 | flaky | failed, passed, passed |
| search_01 | search | 3/3 | stable_pass | passed, passed, passed |
| search_02 | search | 3/3 | stable_pass | passed, passed, passed |
| search_03 | search | 3/3 | stable_pass | passed, passed, passed |

## Flaky tasks (quarantined from the capability score)

- rec_02: 1/3 — failed, passed, failed
- rec_03: 2/3 — failed, passed, passed
