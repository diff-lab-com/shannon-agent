# Skill Loop Smoke Checklist

End-to-end checklist for the skill loop feature. The evaluator
contract requires real LLM reasoning on an actual task, so this
cannot be automated -- you must run it manually in the desktop app.

Estimated time: 10 minutes.

## Pre-flight

1. **Enable skill loop.** Settings -> Advanced -> "Extract skills
   from completed tasks". Or set `skill_loop_enabled: true` in
   `~/.shannon/desktop/config.json`.
2. **Verify provider credentials.** Settings -> Models. Confirm a
   provider is selected and the API key field is populated. The
   evaluator uses the same model that handles the task, so both
   calls must succeed.
3. **Lower thresholds for faster testing.** Set
   `skill_loop_min_duration_secs: 0` and
   `skill_loop_min_tool_calls: 1` so every tool-using task is
   evaluated. Raise them after the smoke test to reduce noise.
4. **Pick a test task.** Use a multi-step prompt that exercises at
   least 2 tools, for example: "Read the README in the current
   directory and summarize it in 3 bullet points."

## Test run

1. Start a new chat and send the test prompt.
2. Wait for the task to complete (the AI finishes its response and
   the stop button disappears).
3. Watch for the toast in the bottom-right: "Found 1 reusable
   pattern." This appears ~5-30 seconds after completion (one
   evaluator LLM call runs in the background).
4. If the task was too simple or the evaluator returned
   `suggest=false`, no toast appears. Check the log (see Verify
   step 4) and retry with a more complex prompt.

## Verify (disk side-effects)

Run these in a terminal. Paths assume `$HOME` resolves to your
home directory.

1. **Evaluator output.** Tracing logs go to stderr (not a file).
   If you launched Shannon from a terminal, scroll back for lines
   containing `skill loop`. If you set `SHANNON_LOG_FORMAT=json`,
   pipe stderr through `jq 'select(.msg | contains("skill"))'`.
   On failure you will see `skill loop evaluate failed (non-blocking)`.

2. **Proposal file.** After the toast, check whether a proposal
   draft was written:
   ```sh
   ls -la ~/.shannon/skill-loop/proposals/
   ```
   **Known gap:** the auto-evaluation hook currently emits the
   `skill-proposal-available` event with a hardcoded count of 1
   but does **not** call `skill_loop_generate`, so this directory
   may be empty. The toast fires based on the event, not on disk
   state. If the directory is empty, the review panel will show
   "No proposals" -- this is expected with the current build.

3. **Scheduled run trace.** The task itself is logged in the
   scheduled-runs store:
   ```sh
   ls -la ~/.shannon/scheduled-runs/  # YYYY/MM.jsonl files
   ```
   Note: this records routine executions, not chat turns. If you
   ran the test as a chat message (not a scheduled routine), this
   will be empty -- that is normal.

4. **Tracing warnings.** Scan stderr output for non-blocking
   errors. With JSON format, filter for warn/error level:
   ```sh
   # If launched from terminal with SHANNON_LOG_FORMAT=json:
   # stderr is already captured in your terminal scrollback.
   # Look for: "skill loop evaluate failed"
   ```

## Approval flow

1. Click **View** on the toast. The review panel opens.
   - If proposals exist on disk, you see name, description,
     triggers, and workflow.
   - If the directory is empty (see Known gap above), the panel
     shows "No proposals" -- the evaluation signal worked but
     generation was not triggered.
2. Click **Approve** on a proposal.
   - On success: the TOML file is written to
     `~/.shannon/skills/user-proposed/{slug}.toml` and the
     proposal draft is deleted.
   - On duplicate: you see "Similar skill already exists"
     (Levenshtein similarity > 0.8 against an existing skill).
3. Click **Reject** to discard a proposal. The draft is deleted
   silently.

## Cleanup

Disable the loop after testing to stop the background evaluator
LLM call on every completed task:

- Toggle off in Settings -> Advanced, or
- Set `skill_loop_enabled: false` in config.json.

Pending proposal drafts are kept on disk under
`~/.shannon/skill-loop/proposals/`. Delete them manually:

```sh
rm -rf ~/.shannon/skill-loop/proposals/
```

## What to report back

1. **Evaluator result.** Copy the `EvaluationResult` struct fields
   (`suggest`, `reason`, `confidence`, `scores`) from the stderr
   tracing output. This tells us whether the LLM correctly
   identified the task as reusable.
2. **Console warnings.** Any `skill loop evaluate failed` or
   `tracing::warn` lines from stderr, with the full error message.
   This pinpoints where the pipeline broke.
3. **Proposal file (if generated).** The contents of the
   `{slug}.toml` written to `~/.shannon/skills/user-proposed/` --
   check whether the name, triggers, and workflow are reasonable.
