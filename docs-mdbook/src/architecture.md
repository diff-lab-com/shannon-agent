# Architecture

Shannon Code is organized as a Cargo workspace with 12 crates, each with a single responsibility.

## System Overview

```
┌─────────────────────────────────────────────┐
│           shannon-cli (Entry Point)         │
│         clap: repl | version | config       │
└──────────────────┬──────────────────────────┘
                   │
┌──────────────────▼──────────────────────────┐
│           shannon-ui (Terminal)             │
│  REPL ── Vim Mode ── Markdown ── Diff View  │
└──────────────────┬──────────────────────────┘
                   │
┌──────────────────▼──────────────────────────┐
│          shannon-core (Engine)              │
│                                             │
│  QueryEngine  LlmClient  ToolRegistry  Perm │
│  (Streaming)  (Multi-LLM) (Dynamic)    Mgr  │
└──────────────────┬──────────────────────────┘
                   │
┌──────────────────▼──────────────────────────┐
│    shannon-tools ── shannon-mcp ── agents   │
│    (File/Bash)     (Protocol)    (Multi)    │
└─────────────────────────────────────────────┘
```

## Crate Map

| Crate | Responsibility | Tests |
|-------|---------------|-------|
| `shannon-core` | API client, query engine, permissions, tools, state | ~3370 |
| `shannon-ui` | Terminal UI (ratatui), REPL, vim mode, widgets | ~1089 |
| `shannon-tools` | Tool implementations (bash, file ops, search) | ~1111 |
| `shannon-commands` | Built-in commands (/help, /config, /commit, etc.) | ~335 |
| `shannon-agents` | Multi-agent orchestration | ~471 |
| `shannon-cli` | CLI entry point (clap), config loading | ~191 |
| `shannon-skills` | Skill system (command templates) | ~171 |
| `shannon-mcp` | MCP server integration | ~373 |
| `shannon-types` | Shared types (re-exported by shannon-core) | ~22 |
| `shannon-tool-interface` | Tool trait definitions | ~24 |
| `shannon-desktop` | Tauri desktop app (scaffolded) | ~24 |
| `shannon-codegen` | Code generation utilities | ~102 |

## Key Design Patterns

### Multi-Provider LLM

`LlmClient` normalizes Anthropic, OpenAI, and Ollama via an adapter pattern (`api/adapter.rs`). Adding a new provider requires implementing the adapter trait.

### Streaming

SSE byte stream → `SseStream` → `MessageStream` with chunk boundary buffering. The Bash tool emits `ToolProgress` events for real-time output.

### Tool Interface

The `Tool` trait in `shannon-tool-interface` defines:

- `execute()` — synchronous execution
- `execute_streaming()` — streaming execution with progress
- `is_read_only()` — safe for parallel execution
- `is_concurrency_safe()` — can run alongside other tools
- `is_destructive()` — modifies filesystem or external state

### Error Handling

`thiserror` for library crates (`ApiError`, `QueryError`), `anyhow` for CLI/binary crates. Production code uses `expect("reason")` over `unwrap()` for panic diagnostics.

### Config Priority

CLI args > env vars (`SHANNON_*`) > `.shannon.toml` > `~/.shannon/config.toml`

### Context Management

Three-layer context strategy:
1. **Compaction** — Summarizes old messages when approaching context limits
2. **Progressive loading** — Truncates large files preserving head/tail
3. **Prompt caching** — Three-layer Anthropic cache breakpoint injection
