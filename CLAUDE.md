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

- **Rich subagent system**: Claude Code supports 4 agent mechanisms: subagents (isolated context, tool restrictions, model overrides), agent teams (shared task list, inter-agent messaging), agent view (background sessions), and `/batch` (5-30 worktree-isolated PRs). Shannon's `shannon-agents` has teammate coordination with shared executor but no per-agent model/tool config, no `/batch`, no agent view dashboard.
- **Worktree isolation**: Claude Code auto-isolates agent work into git worktrees. Shannon has no equivalent.
- **File checkpointing/rewind**: Claude Code tracks and restores any prior file state mid-session. Shannon has `CheckpointManager` but no file-level rewind.

### HIGH — Shannon has partial support

- **Permission auto-mode**: 9 `ApprovalMode` variants with `PermissionClassifier` (2928 lines) wired into `PermissionRuleChecker`. Gap: Claude Code's auto mode uses an LLM to judge each tool call's safety against org policy with 4-tier precedence (hard_deny > soft_deny > allow > explicit intent). Shannon has static rules only.
- **Non-interactive/CI mode**: `--prompt` flag with FullAuto permissions (auto-approve non-critical, deny critical). NDJSON streaming, tool restrictions, exit codes. Gap: no structured outputs (validated JSON return like Claude Code SDK), no deep links (`claude-cli://` URLs).
- **MCP tool search**: `tools/list` works with deferred schema loading. Gap: Claude Code has on-demand tool search that scales to thousands of MCP tools. No MCP channel support (push webhooks/alerts into live sessions).
- **Hook system**: `HookManager` with `HookEvent`/`HookEventType`. Gap: Claude Code has 18+ hook events including `SubagentStart`, `SubagentStop`, `TaskCompleted`, `TeammateIdle`, `PreCompact`, `WorktreeCreate`, `WorktreeRemove`, `ConfigChange`. Shannon has fewer event types.
- **LSP integration**: 6 LSP tools + `DiagnosticRegistry` + two client implementations. Gap: not wired for automatic background diagnostics — tools must be explicitly invoked (OpenCode runs `gopls`/`tsc` automatically).
- **Plugin system**: `PluginRegistry` with manifest parsing. Tool plugins fully wired (MCP discovery). Command plugins register as `PromptCommand` in `CommandRegistry` (source: `Plugin`). Skill plugins register as `PromptCommand` with trigger as slash command name and entry file as template. Loading in both REPL (`new()`) and CLI headless mode.
- **Desktop app**: Scaffolded Tauri app with TODO stubs.
- **Agent creation flow**: `AgentTool` spawns sub-processes but no model override or tool restriction per agent.

### MEDIUM — Quality-of-life gaps

- **Multi-surface**: Claude Code runs on CLI, VS Code, JetBrains, web, desktop. Shannon has CLI + scaffolded Tauri desktop app.
- **File watching**: `SourceWatcher` watches project source files (.rs, .ts, .py, etc.) via `notify` crate. `CustomCommandWatcher` watches command directories. `SettingsWatcher` watches config files. `DiagnosticStore.sync_from_registry()` bridges tool-layer diagnostics to UI display.
- **Vision/multimodal**: Display only; no vision model integration for image analysis.
- **Patch application**: Basic diff rendering; no three-way merge or conflict markers.
- **Computer use**: Claude Code can click, type, see screen on macOS. Shannon has no equivalent.

### Resolved

- **Session resume by ID**: `--resume [<UUID>]` accepts optional UUID. `shannon --resume` for most recent, `shannon --resume <uuid>` for specific session. `--continue` / `-c` as alias.
- **Headless permissions**: `FullAuto` by default (auto-approve non-critical, deny critical). `BypassPermissions` only with explicit `--yes` flag.
- **Tool grouping/diff stats/streaming thinking**: All implemented in UI.
- **File watching for source code**: `SourceWatcher` detects project file changes; `DiagnosticStore.sync_from_registry()` bridges diagnostics to UI.
- **Skill plugin execution**: Skill plugins now register as executable slash commands (trigger → command name, entry file → template).

### Test Coverage

7560 total tests across all crates (7502 passing, 58 e2e require API access). Every source file (`src/**/*.rs`) in every crate has at least one `#[test]`. E2e tests (`shannon-cli/tests/cli_e2e_tests.rs`) need Ollama/Anthropic — run with `--skip test_long_conversation --skip test_multiturn` to skip them.

## Competitor Feature Tiers

### Tier 1 — Table Stakes (Shannon has most)
Multi-provider LLM, tool use, file read/write/edit, bash execution, MCP extensions, streaming output, session persistence, context compaction, config files, i18n, skills/commands system.

### Tier 2 — Differentiators (Shannon partially has)
- **Subagent system**: Claude Code has 4 agent mechanisms. Shannon has teammate coordination but no per-agent config, `/batch`, or agent view.
- **Worktree isolation**: Claude Code auto-isolates agents. Not implemented.
- **OS sandbox**: Codex uses macOS Seatbelt/AppArmor/Docker. Shannon uses project-dir sandboxing only.
- **Auto-permission classifier**: Claude Code uses LLM-based 4-tier classification. Shannon has rule-based `PermissionClassifier`.
- **LSP integration**: OpenCode runs language servers in background. Shannon has 6 LSP tools but no automatic diagnostics loop.
- **Hook system**: Claude Code has 18+ hook events. Shannon has 32 hook events (more coverage).
- **Non-interactive/CI mode**: Claude Code `claude -p` with structured outputs. Shannon has `--prompt` with NDJSON output.

### Tier 3 — Quality of Life
Multi-surface (web/desktop/CLI/IDE), computer use, file checkpointing/rewind, agent SDK (library mode), deep links, MCP channels, model switching, prompt caching, token counting UI.

## Gotchas

- `edition = "2024"` requires Rust 1.85+.
- Integration tests in `shannon-core/tests/` use `mockito::Server` — never hit real APIs.
- The `mockito` server matchers are order-dependent when using `.expect(N)`.
- `LlmClientConfig` must include `max_stream_reconnects` field (all constructors have it).
- `#[allow(dead_code)]` annotations mark planned-but-unwired features — do not remove without confirmation.
