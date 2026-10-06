# Introduction

**Shannon** is a fully open-source (Apache-2.0), Rust-based **AI agent workspace**. It runs on your machine, works with **any LLM provider** — Anthropic, OpenAI, DeepSeek, Z.ai (GLM), Ollama, or any OpenAI-compatible endpoint — and ships as one engine with four surfaces: an interactive terminal UI, headless mode, a local engine server, and a desktop app.

Two commitments shape every design decision:

1. **Open source, total control** — every line auditable, every agent action replayable (`shannon trace`), every cost visible (budget caps, context breakdown), no vendor lock-in (BYOK, Claude Code ecosystem compatible).
2. **Keys never leave your machine** — API keys talk directly to your chosen provider, the `secret-guard` plugin redacts secrets from outbound messages, integration credentials live in the OS keyring, and there is no telemetry by default.

## At a Glance

| Feature | Shannon |
|---------|-------------|
| Language | Rust (memory-safe, zero-cost abstractions) |
| Surfaces | Terminal TUI · headless (`shannon -p`) · engine server (`shannon serve`) · desktop app (`shannon desktop`) |
| LLM Support | Multi-provider BYOK (Anthropic, OpenAI, DeepSeek, Z.ai/GLM, Ollama, any OpenAI-compatible) |
| Extensions | MCP (Model Context Protocol) — Claude Code compatible; skills; plugins (`shannon-plugin-api`) |
| Agents | Multi-agent teams with worktree isolation; `/batch` best-of-N |
| Autonomy | `/goal`, `/loop`, `/ralph` with budget caps, anti-spin and stall-strike guards |
| Automation | Routines with cron / API-endpoint / GitHub-event triggers; IM channels; mobile dispatch |
| Auditability | Event-sourced sessions (`events.jsonl`), `shannon trace show / replay / diff / export` |
| Security | OS keyring, secret-guard outbound redaction, Landlock/Seatbelt sandbox, permission profiles, prompt-injection scanning |
| Tests | 11,752+ automated tests across 20 workspace members |
| i18n | 10 languages |

## Getting Around This Book

- **Getting Started** — install, configure, first run (terminal and desktop).
- **User Guide** — desktop app, goals, automations, agent teams, IM channels, mobile dispatch, cost control, trace, security & privacy, and migration from other tools.
- **Configuration** — every config knob.
- **Features** — deep dives into tools, MCP, permissions, memory, caching, i18n.
- **Developer Reference** — architecture, crate reference, testing, contributing.

## Design Principles

- **Local first** — state lives in `~/.shannon/`; nothing leaves your machine except the model API calls you configure.
- **Auditability by construction** — sessions are append-only event logs; every capability is a projection of that log.
- **Provider neutrality** — the engine is BYOK and provider-agnostic; switching models or providers is a config change, not a migration.
- **Memory safety and performance** — guaranteed at compile time via Rust; Tauri (not Electron) on the desktop.
- **Extensibility** — MCP protocol, skill framework, hook system, and the `shannon-plugin-api` content-transform contract.

## License

Apache-2.0. See [LICENSE](https://github.com/diff-lab-com/shannon-agent/blob/main/LICENSE) in the repository.

## Project Status

Shannon is in active development. See the [Roadmap](roadmap.md) for planned features.
