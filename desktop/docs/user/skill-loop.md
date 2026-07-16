# Skill Loop (Self-evolving Skills)

Skill Loop watches what you do, notices repeatable patterns, and offers to
turn them into reusable skills — no TOML authoring required.

This feature is **opt-in** and runs entirely locally. Nothing about your
prompts or tool usage leaves your machine except the single LLM call used
to evaluate each task.

## How it works

1. You run a task in chat (code review, report generation, data cleanup,
   etc.).
2. When the task completes, Shannon evaluates it against four signals:
   - **Duration** — did it run long enough to suggest a real workflow?
   - **Tool count** — did it touch multiple tools?
   - **Goal clarity** — does the prompt state a clear deliverable?
   - **Outcome** — did the task succeed?
3. If the evaluator says "yes, this looks like a reusable pattern", a
   toast appears in the bottom-right: **"Found 1 reusable pattern"**.
4. Click **Review** to open the proposal panel. Edit the name,
   description, triggers, and example workflow to taste.
5. **Approve** writes the skill to `~/.shannon/skills/user-proposed/`
   and deletes the draft. **Reject** discards it silently.

Shannon will not generate a proposal for tasks that fail or that don't
meet the minimum thresholds below.

## Enabling Skill Loop

**Settings → Advanced → Extract skills from completed tasks**.

Or edit `~/.shannon/desktop/config.json` directly:

```json
{
  "skill_loop_enabled": true,
  "skill_loop_min_duration_secs": 30,
  "skill_loop_min_tool_calls": 2
}
```

A task is considered for evaluation if **either** threshold is met.

## Tuning the evaluator

The defaults (`min_duration_secs: 30`, `min_tool_calls: 2`) are
deliberately low — they surface more proposals so you can decide.
Raise them if you find the toast noisy.

- `min_duration_secs: 300` + `min_tool_calls: 5` — only complex
  multi-minute workflows.
- `min_duration_secs: 0` + `min_tool_calls: 1` — every task that used
  a tool (noisy; useful while calibrating).

## Where proposals live

| Path | Purpose |
|------|---------|
| `~/.shannon/skill-loop/proposals/{id}.json` | Pending drafts (review queue) |
| `~/.shannon/skills/user-proposed/{slug}.toml` | Approved skills (live) |

Proposals are JSON; approved skills use the standard Shannon skill
TOML format and are immediately discoverable by the skill registry —
no restart needed.

## Dedup

Before writing an approved skill, Shannon scans
`~/.shannon/skills/**/*.toml` and compares the proposed name against
existing skill names using normalized Levenshtein similarity. If a
match >0.8 is found, the approval returns an error suggesting you
merge with the existing skill instead.

## Disabling

Toggle the same setting off, or set `skill_loop_enabled: false`. Pending
proposals are kept on disk so you can review them later.

## Privacy

- Task content (your prompt + tool names) is processed locally.
- A single LLM call is made **per evaluated task** using your
  configured default model — the same model that handled the original
  task.
- No usage data is sent to any telemetry endpoint.
- Proposals live only on your local disk.

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| No toast after long task | Thresholds not met, or evaluator returned `suggest=false` | Lower `min_duration_secs`, or check `~/.shannon/logs/desktop.log` |
| Toast shows but review panel empty | Storage write failed | Check disk space + permissions on `~/.shannon/skill-loop/` |
| Approve returns "similar skill exists" | Dedup matched an existing skill | Rename the proposal, or delete the old skill first |
| Skill doesn't show in `/skills` after approve | Skill registry cache stale | Restart Shannon, or run `reload_skills` |

## See also

- [E2 design doc](../architecture/e2-skill-loop.md) — internal
  architecture, data shapes, and cross-repo boundaries.
- [Skills overview](./features.md#skills) — how Shannon reads skills
  and how to author them by hand.
