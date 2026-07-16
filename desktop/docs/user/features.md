# Features

Shannon has four primary surfaces in **Simple mode** (the default). The
sidebar toggle at the bottom-left switches to **Advanced mode**, which
adds Extensions and the experimental One-Person-Company (OPC) surface.

## Chat

The main screen. Where you talk to the AI.

- **New Chat** (`Ctrl+Shift+N`) starts a fresh conversation.
- Sessions are saved automatically and listed in the sidebar.
- The AI can use tools — file read/write, web search, shell commands,
  scheduled task creation. Every tool call is visible inline; you can
  approve each one or revoke approval at any time.
- File attachments: drag a file onto the input box, or click the paper
  clip. Images are previewed inline.
- Cancel a running turn with `Esc` or the stop button.

## Scheduled Tasks

Scheduled routines and one-off background work.

- **Calendar view** — Monthly calendar showing when routines fire next.
- **DAG view** — Dependency graph for routines that chain off each
  other.
- **Active / History / Worktrees** tabs — Switch between running tasks,
  completed runs, and per-task isolated worktrees.
- **Routine types**:
  - **Scheduled** (cron) — fires on a schedule.
  - **Triggered** — fires on an event (webhook, file change, etc.).
  - **One-off** — single background task.

Routines live in `~/.shannon/scheduled-tasks/`. Run history in
`~/.shannon/scheduled-runs/`.

## Triage

A unified inbox for items that need your attention.

- Notifications from background tasks
- Errors from the engine
- Webhook deliveries
- Anything that the AI flagged for review

Each item can be marked read, snoozed, or acted on (e.g. clicking a
notification opens the related session). Items are persisted to
`~/.shannon/triage.jsonl`.

## Settings

Six sub-pages:

| Sub-page | What you change |
|---|---|
| **General** | Language (English / 简体中文), working directory |
| **Theme** | Light / Dark / System; density |
| **Models** | Provider, API key, model selection, model parameters |
| **Usage & Billing** | Token usage this month, billing status |
| **Advanced** | IPC permissions, dev-mode defaults, debug logging |
| **Notifications** | OS notification test, inbound channels (Telegram, Slack) |

## Advanced mode

Switching the sidebar toggle to Advanced reveals:

- **Extensions** — Curated catalog of MCP servers, Skills, Agents, Data
  Sources, Plugins. Install with one click for verified vendors.
- **OPC** (One Person Company) — Experimental surface for orchestrating
  multiple AI agents. Rough edges expected.

Most users can leave Advanced mode off. It exists for power users and
tinkerers.
