# Trace: Replay Agent Actions

Every Shannon session is **event-sourced**: each turn is appended to an immutable `events.jsonl` for that session. Resume, search, replay, and diff are all projections of that single authoritative log — a dashcam for your agents.

## Commands

```bash
shannon trace show --session <id>     # inspect the event log
shannon trace replay --session <id>   # replay the session, step by step
shannon trace diff --session <id>     # what changed, turn by turn
shannon trace export --session <id>   # export for archiving / incident review
```

`--resume` in the TUI picks the last session automatically; `shannon --resume <session-uuid>` restores a specific one.

## Why it matters

- **Debugging** — when an agent went off the rails, replay shows exactly which tool call, which output, which turn.
- **Audits** — reconstruct what happened on a machine, without trusting anyone's summary of it.
- **Incident review** — export the log and attach it to a postmortem.
- **Learning** — replay a session to see how a complex task was decomposed.

> Version note (v0.11): legacy `sessions/<uuid>.json` snapshots and transcript files are no longer read; `events.jsonl` is the only authoritative record. See [Version Migration Notes](migrating-from-other-tools.md).
