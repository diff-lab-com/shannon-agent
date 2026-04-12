# Shannon Code

Rust-based AI code assistant (like Claude Code) with multi-provider LLM support, plugin system, and terminal UI.

## Build & Test

```bash
cargo build                    # Build workspace
cargo check --workspace        # Fast type-check
cargo test --workspace -- --test-threads=1  # Run all tests (threading avoids env contention)
cargo clippy --workspace       # Lint
```

Tests use `--test-threads=1` because some tests share environment variables and file paths.

## Architecture

| Crate | Responsibility |
|-------|---------------|
| `shannon-core` | API client, query engine, permissions, plugins, tools, state management |
| `shannon-ui` | Terminal UI (ratatui), REPL, vim mode, widgets, rendering |
| `shannon-cli` | CLI entry point (clap), config loading, non-interactive mode |
| `shannon-commands` | Built-in commands (/help, /config, /pdf, /commit, etc.) |
| `shannon-tools` | Tool implementations (bash, file ops, search, config manager) |
| `shannon-mcp` | MCP (Model Context Protocol) server integration |
| `shannon-agents` | Multi-agent orchestration |
| `shannon-skills` | Skill system (command templates) |
| `shannon-types` | Shared types (re-exported by shannon-core) |
| `shannon-tool-interface` | Tool trait definitions |

## Key Patterns

- **Error handling**: `thiserror` for library crates (`ApiError`, `QueryError`), `anyhow` for CLI/bin.
- **Multi-provider**: `LlmClient` normalizes Anthropic/OpenAI/Ollama via adapter pattern (`crates/shannon-core/src/api/adapter.rs`).
- **Streaming**: SSE byte stream → `SseStream` → `MessageStream` with chunk boundary buffering.
- **Config priority**: CLI args > env vars (`SHANNON_*`) > `.shannon.toml` > `~/.shannon/config.toml`.
- **Plugin tools**: `PluginManager` discovers `.so`/`.dylib` plugins, registers via `register_plugin_tools()`.
- **Tests with HTTP mocking**: Use `mockito` crate for API integration tests (see `crates/shannon-core/tests/api_integration.rs`).

## Gotchas

- `edition = "2024"` requires Rust 1.85+.
- Integration tests in `shannon-core/tests/` use `mockito::Server` — never hit real APIs.
- The `mockito` server matchers are order-dependent when using `.expect(N)`.
- `LlmClientConfig` must include `max_stream_reconnects` field (all constructors have it).
