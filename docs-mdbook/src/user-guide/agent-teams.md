# Agent Teams & /batch

Shannon runs multiple agents as **real OS processes**, not simulated threads — each with its own model, tool set, working directory, and (typically) its own git worktree.

## Teams

```bash
/team create refactor-crew        # create a team
TeamCreate / SendMessage          # agent-side tools for coordination
```

- Each teammate can run a **different model** — put a strong model on design, a cheap one on grunt work.
- Per-agent tool allowlists and working directories.
- The **agent dashboard** (TUI widgets and desktop panels) shows live status for every teammate.

## Worktree isolation

Every agent that edits code works in its own git worktree, so parallel agents never stomp on each other's files. Worktrees are created, tracked, and cleaned up by Shannon; the desktop **Tasks → Worktrees** panel gives you an overview.

## /batch: best-of-N

The highest-leverage use of parallel agents:

```bash
/batch 3 implement the rate limiter per docs/spec.md
```

1. Shannon decomposes the task and spawns **N worktree-isolated attempts** in parallel.
2. When they finish, you compare the resulting diffs **side by side** (desktop diff view).
3. **Adopt the winner** — merge or cherry-pick it; the losing worktrees are cleaned up.

Use it for risky refactors, tricky bug fixes, or when you want options instead of a single take.

## Related

- [Goals](goals.md) — give a whole team a standing objective.
- [Hooks](../features/hooks.md) — react to agent lifecycle events (`TeammateIdle`, `TaskCreated`, ...).
