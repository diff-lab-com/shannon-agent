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
| `shannon-core` | API client, query engine, permissions, tools, state management | ~3340 |
| `shannon-ui` | Terminal UI (ratatui), REPL, vim mode, widgets, rendering | ~1041 |
| `shannon-tools` | Tool implementations (bash, file ops, search, config manager) | ~1107 |
| `shannon-commands` | Built-in commands (/help, /config, /pdf, /commit, etc.) | ~335 |
| `shannon-agents` | Multi-agent orchestration | ~471 |
| `shannon-cli` | CLI entry point (clap), config loading, non-interactive mode | ~191 |
| `shannon-skills` | Skill system (command templates) | ~171 |
| `shannon-mcp` | MCP (Model Context Protocol) server integration | ~373 |
| `shannon-types` | Shared types (re-exported by shannon-core) | ~22 |
| `shannon-tool-interface` | Tool trait definitions | ~24 |
| `shannon-desktop` | Tauri desktop app (scaffolded) | ~24 |
| `shannon-codegen` | Code generation utilities | ~102 |
| `shannon-agent` | Single agent runtime (binary crate) | ~65 |

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

- **Permission auto-mode**: Claude Code has AI-based classifier that auto-allows safe operations. Shannon has manual allow/deny only. Headless mode uses `BypassPermissions` for all tools — no read-only auto-approve.
- **Rich subagent system**: Claude Code/Codex support spawning per-task agents with isolated context, tool restrictions, and model overrides. Shannon's `shannon-agents` has orchestration but teammate executor falls back to placeholder responses when no LLM is available.
- **Worktree isolation**: Claude Code creates git worktrees for isolated agent work. No equivalent in Shannon.

### HIGH — Shannon has partial support

- **Non-interactive/CI mode**: `--prompt` flag supports headless execution with exit codes, tool restrictions, NDJSON streaming. All tools bypass permissions in headless mode — should auto-approve read-only tools and only prompt for destructive ones.
- **MCP tool search**: `tools/list` works, but deferred tool schemas aren't loaded on demand. No `tools/call` pagination for large MCP server fleets.
- **Auto-trigger compaction**: Post-query `check_context_pressure()` uses `CompactConfig.trigger_threshold`. Defers during streaming via `pending_auto_compact`. No separate background loop needed for CLI tool.
- **Project memory (MEMORY.md)**: `MemoryStore` + `AutoDreamService` exist but no `MEMORY.md` index file pattern like Claude Code for cross-session context.
- **LSP integration**: 6 LSP tools fully implemented in `shannon-tools/src/lsp.rs` (GoToDefinition, FindReferences, Hover, DocumentSymbol, WorkspaceSymbol, RenameSymbol, CodeActions) + `DiagnosticRegistry` in `lsp_diagnostics.rs`. Two LSP client implementations: `shannon-core/src/lsp/client.rs` (lsp_types) and `shannon-tools/src/lsp.rs` (custom JSON-RPC). Gap: not wired into query engine for automatic real-time diagnostics — tools must be explicitly invoked.
- **Plugin system wiring**: Module structure exists (`crates/shannon-core/src/plugin/`) with `PluginRegistry`, `PluginManifest`, manifest parsing. CLI auto-discovers plugins from `~/.shannon/plugins/`. Tool transport works but non-tool plugin kinds (hooks, skills) are stubbed.
- **Desktop app**: Scaffolded Tauri app with TODO stubs for QueryEngine, model_registry, tool_registry.
- **Agent creation flow**: `AgentTool` spawns sub-processes but creation command is placeholder — no model override or tool restriction per agent.

### MEDIUM — Quality-of-life gaps

- **File watching**: Limited to skill files only; no general project file watching.
- **Vision/multimodal**: Display only; no vision model integration for image analysis.
- **Patch application**: Basic diff rendering; no three-way merge or conflict markers.
- **Tool grouping in UI**: Consecutive same-category tools not visually grouped (plan exists at `.claude/plans/proud-stargazing-bumblebee.md`).
- **Streaming thinking display**: Thinking content streams as char count only, no inline preview.
- **Inline diff stats**: Write/Edit tools don't show `+N -N` line counts in collapsed display.
- **Multi-agent executor**: `multi_agent.rs` has coordinator/worker split but workers fall back to placeholder text when LLM unavailable.
- **CLI tool result display**: Tool result for unknown tools returns `"Tool result displayed in UI"` placeholder in non-UI contexts.

### Resolved

- **Session resume by ID**: `--resume [<UUID>]` accepts optional UUID. `shannon --resume` for most recent, `shannon --resume <uuid>` for specific session. `--continue` / `-c` as alias.

### Test Coverage

7455 total tests. Zero-test files remain:
- **core**: `api_server`, `compact/mod`, `error` (orphan — not in module tree), `lsp/client`, `memory/mod`
- **UI**: 20+ widget/command files (renderable, sidebar, markdown, diff_view, etc.)
- **tools**: `lib.rs`

## Competitor Feature Tiers

### Tier 1 — Table Stakes (Shannon has most)
Multi-provider LLM, tool use, file read/write/edit, bash execution, MCP extensions, streaming output, session persistence, context compaction, config files, i18n.

### Tier 2 — Differentiators (Shannon partially has)
- **Subagent system**: Claude Code/Codex spawn isolated agents. Shannon has `shannon-agents` orchestration but no per-agent model/tool config.
- **Worktree isolation**: Claude Code creates git worktrees for agents. Not implemented.
- **OS sandbox**: Codex uses macOS Seatbelt/AppArmor. Shannon uses project-dir sandboxing only.
- **Auto-permission classifier**: Claude Code AI-classifies tool safety. Shannon has static categories only.
- **LSP integration**: OpenCode runs `tsc --noEmit` / `cargo check` in background. Shannon has 6 LSP tools + DiagnosticRegistry, but no automatic background diagnostics loop.
- **Non-interactive/CI mode**: Claude Code `claude -p`, Codex `codex exec`. Shannon has `--prompt` flag with NDJSON output.

### Tier 3 — Quality of Life
Multi-surface (web/desktop/CLI), hooks system, agent teams, skills system, model switching, prompt caching, token counting UI.

## Gotchas

- `edition = "2024"` requires Rust 1.85+.
- Integration tests in `shannon-core/tests/` use `mockito::Server` — never hit real APIs.
- The `mockito` server matchers are order-dependent when using `.expect(N)`.
- `LlmClientConfig` must include `max_stream_reconnects` field (all constructors have it).
- `#[allow(dead_code)]` annotations mark planned-but-unwired features — do not remove without confirmation.
