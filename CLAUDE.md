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
| `shannon-core` | API client, query engine, permissions, tools, state management | ~2505 |
| `shannon-ui` | Terminal UI (ratatui), REPL, vim mode, widgets, rendering | ~1023 |
| `shannon-tools` | Tool implementations (bash, file ops, search, config manager) | ~870 |
| `shannon-commands` | Built-in commands (/help, /config, /pdf, /commit, etc.) | ~200 |
| `shannon-agents` | Multi-agent orchestration | ~173 |
| `shannon-cli` | CLI entry point (clap), config loading, non-interactive mode | ~91 |
| `shannon-skills` | Skill system (command templates) | ~123 |
| `shannon-mcp` | MCP (Model Context Protocol) server integration | ~111 |
| `shannon-types` | Shared types (re-exported by shannon-core) | ~22 |
| `shannon-tool-interface` | Tool trait definitions | ~24 |
| `shannon-desktop` | Tauri desktop app (scaffolded) | ~25 |
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

## Known Gaps (vs Claude Code / Codex CLI / OpenCode)

### CRITICAL — Shannon lacks entirely

- **Permission auto-mode**: Claude Code has AI-based classifier that auto-allows safe operations. Shannon has manual allow/deny only.
- **Rich subagent system**: Claude Code/Codex support spawning per-task agents with isolated context, tool restrictions, and model overrides. Shannon's `shannon-agents` has orchestration but no per-agent config or isolation.
- **Non-interactive/CI mode**: `claude -p` and `codex exec` support headless execution. Shannon has `--non-interactive` flag but limited tool approval flow.
- **Session resume by ID**: Competitors can resume past sessions. Shannon has session files but no `--resume <id>` CLI flag.
- **Worktree isolation**: Claude Code creates git worktrees for isolated agent work. No equivalent in Shannon.

### HIGH — Shannon has partial support

- **MCP tool search**: `tools/list` works, but no `tools/call` pagination or tool search/filter for large MCP server fleets.
- **Auto-trigger compaction**: UI sidebar now derives pressure tiers from `CompactConfig.trigger_threshold` (no longer hardcoded). Still no background compaction loop — compaction triggers during active streaming only.
- **Project memory (MEMORY.md)**: `MemoryStore` + `AutoDreamService` exist but no `MEMORY.md` index file pattern like Claude Code for cross-session context.
- **LSP diagnostics integration**: OpenCode has real-time `tsc --noEmit` / `cargo check` integration. Shannon has no LSP client.
- **Plugin system wiring**: Module structure exists (`crates/shannon-core/src/plugin/`) but marked "scaffolded for future use".
- **Desktop app**: Scaffolded Tauri app with TODO stubs for QueryEngine, model_registry, tool_registry.

### MEDIUM — Quality-of-life gaps

- **File watching**: Limited to skill files only; no general project file watching.
- **Vision/multimodal**: Display only; no vision model integration for image analysis.
- **Patch application**: Basic diff rendering; no three-way merge or conflict markers.
- **Tool grouping in UI**: Consecutive same-category tools not visually grouped (plan exists, not implemented).
- **Streaming thinking display**: Thinking content streams as char count only, no inline preview.
- **Inline diff stats**: Write/Edit tools don't show `+N -N` line counts in collapsed display.
- **Test coverage**: ~47 source files with zero test coverage (15 core, 28 UI, 4 tools). Recent additions: compact (96), hooks (38), streaming (27), task (20), write (8), messaging (8), skill (19), agent (23) = 239 new tests.

## Gotchas

- `edition = "2024"` requires Rust 1.85+.
- Integration tests in `shannon-core/tests/` use `mockito::Server` — never hit real APIs.
- The `mockito` server matchers are order-dependent when using `.expect(N)`.
- `LlmClientConfig` must include `max_stream_reconnects` field (all constructors have it).
- `#[allow(dead_code)]` annotations mark planned-but-unwired features — do not remove without confirmation.
