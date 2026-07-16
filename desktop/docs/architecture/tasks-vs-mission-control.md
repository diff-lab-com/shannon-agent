# ADR: Tasks vs Mission Control vs OPC

**Status**: Accepted (2026-06)
**Context**: Sprint 1 PM roadmap — three task surfaces with overlapping UI.

## Decision

Shannon has **three** task surfaces. They look similar but solve different
problems. Splitting them lets each optimize for its own job; merging them
would force one UX to serve three audiences.

| Surface | Route | Job | Writes |
|---------|-------|-----|--------|
| **Tasks** | `/tasks` | Calendar + CRUD for scheduled routines and background tasks. The "I want to schedule a thing" surface. | Full create / edit / cancel / trigger |
| **Mission Control** | `/mission-control` | Read-only kanban across all teams. The "what's going on right now" surface. | None (observation) |
| **OPC** | `/opc` | Agent-orchestration workspace with optimistic drag-and-drop. The "I want to shuffle work between agents" surface. | Optimistic DnD writes |

## Why three?

The use cases have incompatible defaults:

- **Tasks** is form-driven. The user is setting up future work — they need
  cron expressions, retries, budgets, dependencies. A kanban here would bury
  the schedule.
- **Mission Control** is glanceable. The user is on a daily check-in — they
  need to see all teams' status in one screen, no clicks. Edit buttons here
  would clutter the read.
- **OPC** is interactive. The user is reacting to a stuck agent — they need
  drag-and-drop reassignment, instant feedback, optimistic UI. A calendar
  here would slow them down.

## What to avoid

- **Don't add write actions to Mission Control.** It's the read-only view.
  If users want to act on something they see there, they click through to
  Tasks or OPC.
- **Don't add a calendar to OPC.** The kanban is the point. Scheduling
  belongs in Tasks.
- **Don't merge Tasks and OPC.** Same data, different mental models. The
  user picks the tool that matches their intent.

## Open questions

- Should Mission Control grow a "quick reassign" action? Defer until users
  ask for it; the current click-through path works.
- Should Tasks show agent-assignment columns? No — that's OPC's job. Tasks
  shows schedules and runs, not assignments.

## Migration

Legacy routes (`/strategic-focus`, `/agent-swarm`, `/quick-inject`,
`/background-tasks`) redirect to `/opc` or `/tasks` (see `App.tsx`). No
content is lost — the old paths were aliases for these surfaces anyway.

## References

- `ui/src/pages/Tasks.tsx` (header comment has the same distinction list)
- `ui/src/pages/MissionControl.tsx`
- `ui/src/pages/OPC.tsx`
