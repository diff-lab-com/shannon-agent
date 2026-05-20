# Shannon Code

Rust-based AI code assistant (like Claude Code) with multi-provider LLM support, MCP-based extensions, and terminal UI.

## Build & Test

```bash
cargo build                    # Build workspace
cargo check --workspace        # Fast type-check
cargo test --workspace -- --test-threads=1  # Run all tests (threading avoids env contention)
cargo clippy --workspace       # Lint
```

Tests use `--test-threads=1` because some tests share environment variables and file paths.

## Architecture

| Crate | Responsibility | Tests |
|-------|---------------|-------|
| `shannon-core` | API client, query engine, permissions, tools, state management | ~2256 |
| `shannon-ui` | Terminal UI (ratatui), REPL, vim mode, widgets, rendering | ~1018 |
| `shannon-tools` | Tool implementations (bash, file ops, search, config manager) | ~576 |
| `shannon-commands` | Built-in commands (/help, /config, /pdf, /commit, etc.) | ~200 |
| `shannon-agents` | Multi-agent orchestration | ~173 |
| `shannon-cli` | CLI entry point (clap), config loading, non-interactive mode | ~91 |
| `shannon-skills` | Skill system (command templates) | ~123 |
| `shannon-mcp` | MCP (Model Context Protocol) server integration | ~111 |
| `shannon-types` | Shared types (re-exported by shannon-core) | ~22 |
| `shannon-tool-interface` | Tool trait definitions | ~20 |
| `shannon-desktop` | Tauri desktop app (scaffolded) | ~11 |
| `shannon-codegen` | Code generation utilities | ~8 |
| `shannon-agent` | Single agent runtime (binary crate) | 0 |

## Key Patterns

- **Error handling**: `thiserror` for library crates (`ApiError`, `QueryError`), `anyhow` for CLI/bin. Production code uses `expect("reason")` over `unwrap()` for panic diagnostics.
- **Multi-provider**: `LlmClient` normalizes Anthropic/OpenAI/Ollama via adapter pattern (`crates/shannon-core/src/api/adapter.rs`).
- **Anthropic caching**: `inject_cache_control_on_last_block()` adds `cache_control: {type: "ephemeral"}` via JSON post-processing. `SystemContentBlock` has `cache_control` field for system prompts.
- **Streaming**: SSE byte stream → `SseStream` → `MessageStream` with chunk boundary buffering. Bash tool emits `ToolProgress` events for real-time output.
- **Config priority**: CLI args > env vars (`SHANNON_*`) > `.shannon.toml` > `~/.shannon/config.toml`.
- **Extensions**: MCP (Model Context Protocol) — Claude Code compatible. Servers configured in `.mcp.json`, `~/.claude/settings.json`, `~/.shannon/settings.json` via `mcpServers` key. Tools auto-discovered via `tools/list`.
- **Memory subsystem**: `MemoryStore` with Jaccard similarity dedup, `MemoryConsolidator` for merge/prune, `AutoDreamService` for conversation→memory extraction.
- **Tool interface**: `Tool` trait in `shannon-tool-interface` with `execute()`, `execute_streaming()`, `is_read_only()`, `is_concurrency_safe()`, `is_destructive()`.
- **Tests with HTTP mocking**: Use `mockito` crate for API integration tests (see `crates/shannon-core/tests/api_integration.rs`).

## Testing Guidelines

- **Always use `--test-threads=1`** for workspace tests (shared env vars, file paths).
- **Inline tests**: Most crates use `#[cfg(test)] mod tests` within source files. Tests near the code they test.
- **Integration tests**: `crates/shannon-*/tests/` directories for cross-module testing.
- **Mockito**: For HTTP API tests. Server matchers are order-dependent with `.expect(N)`.
- **Test helpers**: `CollectingSender` (progress sender), `tempfile::TempDir` (file tests), `mockito::Server` (HTTP tests).

## Known Gaps (compared to Claude Code/Codex/OpenCode)

- **shannon-desktop**: Scaffolded Tauri app with TODO stubs for QueryEngine, model_registry, tool_registry.
- **Plugin system**: Module structure exists (`crates/shannon-core/src/plugin/`) but marked "scaffolded for future use".
- **File watching**: Limited to skill files only; no general project file watching.
- **Vision/multimodal**: Display only; no vision model integration for image analysis.
- **Patch application**: Basic diff rendering; no three-way merge or conflict markers.

## Gotchas

- `edition = "2024"` requires Rust 1.85+.
- Integration tests in `shannon-core/tests/` use `mockito::Server` — never hit real APIs.
- The `mockito` server matchers are order-dependent when using `.expect(N)`.
- `LlmClientConfig` must include `max_stream_reconnects` field (all constructors have it).
- `#[allow(dead_code)]` annotations mark planned-but-unwired features — do not remove without confirmation.
