# Introduction

**Shannon Code** is a high-performance, open-source AI-assisted coding tool written in Rust. It provides a terminal-based interface for interacting with large language models with tool orchestration, multi-agent coordination, session management, and MCP extensibility.

## Why Shannon Code?

| Feature | Shannon Code |
|---------|-------------|
| Language | Rust (memory-safe, zero-cost abstractions) |
| LLM Support | Multi-provider (Anthropic, OpenAI, Ollama, any OpenAI-compatible) |
| Extensions | MCP (Model Context Protocol) — Claude Code compatible |
| Tools | Read, Edit, Write, Bash, Grep, Glob + MCP tools |
| Agents | Multi-agent orchestration with per-agent model/tool config |
| UI | Terminal UI with vim mode, diff viewer, markdown rendering |
| Tests | 8,600+ tests across 12 crates |
| i18n | 10 languages |

## Design Principles

- **Memory Safety** — Guaranteed at compile time via Rust's ownership system
- **High Performance** — Zero-cost abstractions, async I/O with tokio
- **Type Safety** — Strong type system catches bugs before runtime
- **Extensibility** — MCP protocol, skill framework, hook system
- **Composability** — 12 modular crates with clean separation of concerns

## License

Dual-licensed under MIT or Apache-2.0 at your option.

## Project Status

Shannon Code is in active development. See the [Roadmap](roadmap.md) for planned features.
