---
title: Introduction
order: 1
section: getting-started
---

# Introduction

**Shannon** is a high-performance, open-source AI-assisted coding tool written in Rust. It provides a terminal-based interface for interacting with large language models with tool orchestration, multi-agent coordination, session management, and MCP extensibility.

## Why Shannon?

| Feature | Shannon |
|---------|-------------|
| Language | Rust (memory-safe, zero-cost abstractions) |
| LLM Support | Multi-provider (Anthropic, OpenAI, Ollama, any OpenAI-compatible) |
| Extensions | MCP (Model Context Protocol) — Claude Code compatible |
| Tools | Read, Edit, Write, Bash, Grep, Glob + MCP tools |
| Agents | Multi-agent orchestration with per-agent model/tool config |
| UI | Terminal UI with vim mode, diff viewer, markdown rendering; desktop app with an integrated terminal |
| Tests | 11,752+ automated tests across 20 workspace members |
| i18n | 10 languages |

## Desktop Integrated Terminal

The desktop app embeds a real terminal next to the chat. Press ``Ctrl+` `` to open a bottom-drawer session backed by a PTY and rendered with xterm.js: up to 4 terminals at once, tabs scoped to the current project, and a theme that follows the app. Pasting multi-line text asks for confirmation first, since a pasted newline executes in a shell.

Defaults (shell, font size, scrollback, drawer height, screen-reader mode) live under `[terminal]` in `~/.shannon/config.toml`, editable in Settings → Advanced. The last 1 MiB of output is kept per terminal, so scrollback replays when the panel remounts or reconnects. Chat integration: fenced code blocks offer a **Run in terminal** button, and terminal output can be selected and sent to the agent composer.

## Design Principles

- **Memory Safety** — Guaranteed at compile time via Rust's ownership system
- **High Performance** — Zero-cost abstractions, async I/O with tokio
- **Type Safety** — Strong type system catches bugs before runtime
- **Extensibility** — MCP protocol, skill framework, hook system
- **Composability** — 12 modular crates with clean separation of concerns

## License

Dual-licensed under MIT or Apache-2.0 at your option.

## Project Status

Shannon is in active development. See the [Roadmap](roadmap/) for planned features.
