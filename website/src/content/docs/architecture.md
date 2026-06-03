---
title: Architecture
order: 2
section: introduction
---

# Architecture

Shannon Code is organized as a Rust workspace with 13 crates, each with a single responsibility.

## Crate Overview

| Crate | Responsibility | Tests |
|-------|---------------|-------|
| `shannon-core` | API client, query engine, permissions, tools, state management | ~3370 |
| `shannon-ui` | Terminal UI (ratatui), REPL, vim mode, widgets, rendering | ~1089 |
| `shannon-tools` | Tool implementations (bash, file ops, search, config manager) | ~1111 |
| `shannon-commands` | Built-in commands (/help, /config, /pdf, /commit, etc.) | ~335 |
| `shannon-agents` | Multi-agent orchestration | ~471 |
| `shannon-cli` | CLI entry point (clap), config loading, non-interactive mode | ~191 |
| `shannon-skills` | Skill system (command templates) | ~171 |
| `shannon-mcp` | MCP (Model Context Protocol) server integration | ~373 |
| `shannon-types` | Shared types (re-exported by shannon-core) | ~22 |
| `shannon-tool-interface` | Tool trait definitions | ~24 |

## Data Flow

```
User Input → REPL (shannon-ui)
  → QueryEngine (shannon-core)
    → LlmClient (multi-provider adapter)
      → Anthropic / OpenAI / Ollama / Custom
    ← SSE Stream → MessageStream
  → Tool Execution (shannon-tools)
  → Permission Check (shannon-core)
  ← Response Rendering
```

## Key Patterns

### Multi-Provider Adapter

`LlmClient` normalizes different LLM APIs through an adapter pattern. All providers share the same interface:

```rust
let client = LlmClient::new(config)?;
let stream = client.stream_message(messages, tools).await?;
```

### Tool Trait

Every tool implements the `Tool` trait from `shannon-tool-interface`:

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn execute(&self, input: Value) -> Result<ToolOutput>;
    fn is_read_only(&self) -> bool;
    fn is_destructive(&self) -> bool;
}
```

### Streaming Pipeline

SSE byte stream → `SseStream` → `MessageStream` with chunk boundary buffering. The bash tool emits `ToolProgress` events for real-time output.

### Config Priority

```
CLI args > env vars (SHANNON_*) > .shannon.toml > ~/.shannon/config.toml
```

### Permission Pipeline

User request → `PermissionManager` → `PermissionRuleChecker` → `LlmPermissionClassifier` (optional LLM fallback for ambiguous cases) → approve/deny.

## Testing Architecture

- **Unit tests**: `#[cfg(test)] mod tests` within source files
- **Integration tests**: `crates/shannon-*/tests/` directories
- **Scenario tests**: YAML-driven declarative tests in `tests/scenarios/`
- **Mock HTTP**: `mockito` crate for API integration tests
- **Record/Replay**: Record real API fixtures, replay in CI
