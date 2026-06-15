# Shannon — Your AI Workspace

> One workspace for the work you do with AI: draft an email, research a topic,
> summarize a doc, write code, schedule routines that run while you sleep.
> Shannon is the AI workspace for people who do all of the above.

Shannon started life as a developer tool (an open-source Claude Code clone in
Rust). It still writes code — but the same agent now also handles email,
research, summaries, and the routine work you'd otherwise hand to an intern.
Same engine, broader surface.

## What's in this repo

This is the **desktop app** (Tauri + React 19 + TypeScript). The Rust agent
engine lives in [`shannon-code`](https://github.com/shannon-agent/shannon-code)
and is pulled in as a git subpath dependency.

| Area | Where |
|------|-------|
| Rust backend (Tauri commands, IPC, state) | `src/` |
| Frontend (React, pages, components, hooks) | `ui/src/` |
| Integration tests | `tests/` |
| Benchmarks | `benches/` |

## Quick start

```bash
# Prerequisites: Rust 1.88+, Node 20+, pnpm
pnpm install --dir ui
cargo run
```

For full setup, contributions, and architecture guides see
[`shannon-code`](https://github.com/shannon-agent/shannon-code).

## License

Apache-2.0. See `LICENSE` for details.
