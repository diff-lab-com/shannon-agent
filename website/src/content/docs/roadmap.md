---
title: Roadmap
order: 13
section: reference
---

# Roadmap

## Current Status

Shannon is in active development with 11,752+ automated tests and 418K+ lines of Rust code.

## Completed

- Multi-provider LLM support (Anthropic, OpenAI, Ollama)
- Tool system (Read, Edit, Write, Bash, Grep, Glob)
- MCP protocol integration with Claude Code compatibility
- Terminal UI with vim mode and diff viewer
- Session persistence and resume
- Multi-agent orchestration with worktree isolation
- Hook system (32 event types)
- Permission system with LLM-based classification
- Prompt caching (three-layer Anthropic optimization)
- Vision/multimodal support
- Three-way merge with conflict markers
- Internationalization (10 languages)
- Deep link URL scheme support
- MCP webhook and channel support
- Conversation presets and session templates
- Performance benchmarks

## In Progress

- Plugin marketplace

## Desktop App

The Tauri desktop app ships alongside the CLI: a chat workspace over the
same engine, with sessions per project and an integrated terminal
(persisted settings, replayable scrollback, per-project tabs).

## Planned

- IDE extensions (VS Code, JetBrains)
- Web interface
- Computer use (screen interaction)
- Multi-surface support (web, desktop, CLI, IDE)
- Enhanced LSP integration (more languages)
- Collaborative sessions (multi-user)
