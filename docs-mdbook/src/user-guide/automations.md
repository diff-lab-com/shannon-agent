# Automations & Triggers

Routines turn one-off conversations into scheduled or event-driven work. Every routine runs with the same permission system, sandbox, and audit trail as an interactive session.

## Create a routine

```bash
/routine daily-standup-brief --cron "0 8 * * 1-5" "summarize yesterday's commits and open PRs"
```

Or in the desktop app: **Scheduled Tasks → New Task**, with cron preview, retry policy, and worktree isolation options. Ten built-in TOML templates cover common cases (daily digest, issue triage, dependency updates, ...).

## Trigger types

| Trigger | How it works |
|---|---|
| **Cron** | Standard cron expression, evaluated locally. |
| **API endpoint** | `shannon serve` exposes `POST /routines/:id/trigger` for each routine, verified with HMAC-SHA256 (`X-Shannon-Signature` header). Point external alerts — a Slack webhook relay, a monitoring system — at your own machine. |
| **GitHub events** | Subscribe a routine to repository webhook events (issue opened, PR failed, ...) via the gateway. |

> Security note: the trigger endpoint binds to loopback by default. Do not port-forward it; if you must expose it, keep the HMAC secret and prefer a tunnel that preserves signatures.

## The inbox: Triage

Routine results — and failures — land in **Triage**, not mixed into your chat history. Each entry shows its source (which routine / trigger), the artifacts, and three actions:

- **Continue in original session** — the routine's thread keeps its context; follow up where the work happened.
- **Rerun** — execute the same routine again immediately.
- **Archive** — clear it from the inbox.

Failures land here too, with the error summary attached, so a broken routine is a notification — not a silent gap in your automation.
