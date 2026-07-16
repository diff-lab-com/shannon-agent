# Shannon — Your AI Workspace

> Chat with any model. Deploy agents on any task. Automate anything.

Shannon is a cross-platform **AI workspace** for knowledge workers — not just
coders. Same Rust engine that writes code also drafts email, researches
topics, summarizes docs, and runs on schedules. One product, many jobs to
be done.

## For users

| What | Where |
|---|---|
| Install + first chat | [`docs/user/getting-started.md`](docs/user/getting-started.md) |
| Feature walkthrough | [`docs/user/features.md`](docs/user/features.md) |
| Privacy & data layout | [`docs/user/README.md`](docs/user/README.md) |

No technical background assumed.

## Why Shannon

The AI desktop market splits between general chat tools (ChatGPT Desktop)
and single-purpose coding tools (Claude Code, Cursor). Shannon is built
around four structural advantages that are hard to copy:

| Advantage | What it means | vs. closed competitors |
|-----------|---------------|------------------------|
| **Multi-provider** | Anthropic, OpenAI, Ollama, DeepSeek — switch any time | vs Claude Desktop (Claude only), Codex (OpenAI only) |
| **Multi-agent teams** | Real agent-swarm coordination with worktree isolation | vs ChatGPT Desktop (single agent), Hermes (no team) |
| **Automations as a first-class surface** | Hooks, routines, and permission profiles have their own nav | vs Cursor (no automation), Claude Desktop (no hooks) |
| **Local-first** | Oullama runs alongside cloud models; Tauri not Electron | vs all Electron-based rivals |

These aren't features — they're **anti-fragility**. Multi-provider means
upstream API price hikes don't kill us. Multi-user-group means a single
audience shrinking doesn't kill us. Local-first means connectivity loss
doesn't kill us. Open-source means even the company shutting down doesn't
kill the product.

## What's in this repo

This is the **desktop app** (Tauri + React 19 + TypeScript). The Rust agent
engine lives in
[`shannon-code`](https://github.com/shannon-agent/shannon-code) and is
pulled in as a git subpath dependency.

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

For full setup, contribution guide, and engine architecture, see
[`shannon-code`](https://github.com/shannon-agent/shannon-code).

## Product strategy

For the full positioning analysis — competitive matrix, blue-ocean ERRC,
renaming plan, 12-week roadmap, and KPI definitions — see
[`docs/product-review/04-product-repositioning.md`](docs/product-review/04-product-repositioning.md).

The TL;DR: Shannon keeps everything developers rely on (mission control,
OPC, hooks, worktrees, permission profiles) but surfaces a simpler "AI
workspace" identity to non-coders via a dual-mode sidebar (Simple default,
Developer opt-in via the Welcome wizard).

## License

Apache-2.0. See `LICENSE` for details.
