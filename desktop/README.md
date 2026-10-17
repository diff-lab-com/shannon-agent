# Shannon — Your AI Workspace

> Chat with any model. Deploy agents on any task. Automate anything.

Shannon is a cross-platform **AI workspace** for knowledge workers — not just
coders. Same Rust engine that writes code also researches topics, summarizes
documents (PDF, Office files, and plain text), generates Word/Excel/PPT
deliverables via built-in skills plus the engine-native `write_xlsx` tool,
extracts brand styles from your existing .pptx/.docx templates
(`/style-extract`), and runs work on schedules that hand you finished files in
the triage inbox. One product, many jobs to be done — see the
[local office agent story](../docs/product/office-agent-story.md).

> **Desktop surface of the single `shannon` product** ([ADR-0011](../docs/adr/0011-single-product-multi-surface-distribution.md)):
> every desktop bundle ships the `shannon` CLI alongside the GUI — `/usr/bin`
> via deb/rpm, user PATH via the NSIS installer, and on macOS via
> Settings → Advanced → Command line. Diagnose both with `shannon doctor`.

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
engine lives in `crates/` at the monorepo root and is consumed as a
Cargo workspace member.

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

For full setup, contribution guide, and engine architecture, see the
[monorepo root README](../README.md).

## Voice input (speech-to-text)

The chat composer's mic button records via the webview and transcribes
through the STT provider configured in **Settings → Advanced → Voice**
(Groq / OpenAI Whisper-compatible, or local whisper-rs when built with the
`voice-local` Cargo feature). Platform notes — when capture fails the
composer surfaces the specific cause and a Settings shortcut (AI-chat-ux
V1–V4):

- **macOS**: microphone access is TCC-gated for the *signed, bundled* app —
  the packager merges `macos/InfoPlist.plist` (NSMicrophoneUsageDescription)
  via `bundle.macOS.infoPlist`. `tauri dev` is unsigned and typically never
  prompts; test voice on a packaged build.
- **Linux**: getUserMedia in WebKitGTK needs the GStreamer media backend
  (`gstreamer1.0-plugins-good` / `gstreamer1.0-libav` on Debian-likes,
  `gstreamer1-plugins-good` / `gstreamer1-libav` on Fedora) plus a running
  audio server (PipeWire ≥ 0.3.65 recommended — the deb/rpm depends already
  pull `libpipewire-0.3-0` / `pipewire-libs`). wry's permission bridge is
  still incomplete upstream (tauri-apps/wry#1195): if the webview path
  cannot capture, the mic reports the platform cause instead of a generic
  error.
- **Windows**: WebView2 handles capture natively; the first click asks for
  the system microphone toggle (Settings → Privacy → Microphone).

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
