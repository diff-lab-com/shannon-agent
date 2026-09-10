# Goals: Objective-Driven Agents

A prompt asks one question. A **goal** is an objective the agent keeps working towards — across context compaction, across turns, across hours. Shannon's goal system is built for unattended long tasks with hard guard rails.

## Create a goal

```bash
# Interactive REPL
/goal make the flaky test suite green

# Headless, with a spending cap (USD)
shannon --goal "migrate the auth module to the new API" --budget 5
```

In the desktop app, use the **goal mode** entry in the task composer; running goals show a live card with turn count, accumulated spend, strike count, and the latest event stream — plus pause / stop / restate-the-goal controls.

## How it works

1. The objective is injected into the system prompt and **survives context compaction** — the agent re-reads what "done" means even after the conversation is compressed.
2. The agent can call `goal_get` / `goal_update` tools to inspect and restate its own goal.
3. Completion and blockage are explicit markers: `GOAL_COMPLETE` and `GOAL_BLOCKED`. On completion the run winds down; on blockage the engine audits recent turns (3-turn window) and retries with backoff: **30 min → 1 h → 2 h**.
4. Guards run throughout: **anti-spin** (detect repeated no-progress loops) and **stall strikes** (no useful action for N turns ends the run) replace naive turn counting.
5. The **budget cap** (`--budget $N`) is a hard limit: when spend exceeds it, the run stops instead of burning money.

## Where results go

- `GOAL_COMPLETE` — the result and artifacts land in the desktop **Triage** inbox.
- `GOAL_BLOCKED` — a "needs your decision" entry lands in Triage; continue in the original session with full context.

## Related loops

- `/loop` — repeat a prompt on a cadence, sharing the same anti-spin guards.
- `/ralph` — autonomous iteration for longer self-directed runs.
- `SHANNON_TOKEN_BUDGET` — a token-level watchdog that nudges the model toward targeted reads instead of wholesale re-reading.

> Cost safety: goals are the most powerful — and most spend-hungry — feature. Pair every long goal with `--budget`, and review the cost breakdown (see [Cost Control](cost.md)) after unattended runs.
