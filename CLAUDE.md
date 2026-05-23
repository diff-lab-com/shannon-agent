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
- **Anthropic caching**: Three-layer cache breakpoint injection: (1) `SystemContentBlock::cached()` for system prompts, (2) last `ToolDefinition` gets `cache_control` via adapter serialization, (3) `inject_cache_control_on_last_block()` on last user message content block. `ToolDefinition` has `cache_control` field for explicit per-tool caching.
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

### HIGH — Shannon has partial support

- **Permission auto-mode**: 9 `ApprovalMode` variants with `PermissionClassifier` (2928 lines) wired into `PermissionRuleChecker`. `LlmPermissionClassifier` wraps the rule-based classifier with async LLM fallback for ambiguous cases (confidence < 0.7, Medium+ risk). 4-tier precedence: hard_deny > soft_deny > allow > explicit intent. LLM classification disabled by default, enabled via `with_llm()`.
- **Non-interactive/CI mode**: `--prompt` flag with FullAuto permissions (auto-approve non-critical, deny critical). NDJSON streaming, tool restrictions, exit codes. `--schema` flag accepts file path or inline JSON Schema for structured output validation. Deep link support via `shannon://prompt?text=<encoded>` and `shannon://resume?id=<uuid>` URL scheme with `--register-url-scheme`/`--unregister-url-scheme` commands.
- **MCP tool search**: `tools/list` works with deferred schema loading. MCP webhook/channel support with `WebhookRegistry` (HMAC-SHA256 signing, event filtering, persistence), `EventPublisher` (non-blocking delivery, retry with exponential backoff), and event firing from `McpProcessPool` (ServerConnected/Disconnected, ToolCallStarted/Completed, NotificationReceived).
- **Hook system**: `HookManager` with `HookEvent`/`HookEventType`. 32 event types fully wired: SubagentStart/Stop, WorktreeCreate/Remove, PreCompact/PostCompact, ConfigChange, TaskCreated/TaskCompleted, plus all original events. Non-blocking `fire_hook()` pattern via `tokio::spawn`.
- **LSP integration**: 6 LSP tools + `DiagnosticRegistry` + two client implementations. `DiagnosticStore.mark_stale()` called on source file changes. Background `cargo check` diagnostics auto-run via `DiagnosticWatcher` when source files change — debounce, parse, display in UI.
- **Plugin system**: `PluginRegistry` with manifest parsing. Tool plugins fully wired (MCP discovery). Command plugins register as `PromptCommand` in `CommandRegistry` (source: `Plugin`). Skill plugins register as `PromptCommand` with trigger as slash command name and entry file as template. Loading in both REPL (`new()`) and CLI headless mode.
- **Desktop app**: Scaffolded Tauri app with TODO stubs.
- **Agent creation flow**: `AgentTool` spawns sub-processes with optional model override via `AgentSpawnInput.model`, tool restriction via `AgentSpawnInput.allowed_tools`, and worktree isolation via `context.working_directory`. `/batch` command orchestrates parallel worktree-isolated PR creation.

### MEDIUM — Quality-of-life gaps

- **Multi-surface**: Claude Code runs on CLI, VS Code, JetBrains, web, desktop. Shannon has CLI + scaffolded Tauri desktop app.
- **File watching**: `SourceWatcher` watches project source files (.rs, .ts, .py, etc.) via `notify` crate, wired into REPL main loop — displays changed file names and marks `DiagnosticStore` as stale. `CustomCommandWatcher` watches command directories. `SettingsWatcher` watches config files. `DiagnosticStore.sync_from_registry()` bridges tool-layer diagnostics to UI display.
- **Vision/multimodal**: Full image analysis pipeline. `AnalyzeImageTool` accepts file_path or URL, returns base64 image via `ToolOutput` metadata. `ToolResultEntry.to_tool_result_content()` converts image results to `ContentBlock::Image` for LLM vision. Read tool supports image file reading (PNG/JPG/GIF/WebP/BMP) with base64 encoding.
- **Patch application**: Three-way merge with conflict markers. `three_way_merge(base, ours, theirs)` uses LCS-based algorithm. `parse_conflict_markers()` parses `<<<<<<<`/`=======`/`>>>>>>>` blocks. Edit tool falls back to merge when `old_string` not found (base from git HEAD). `MergeResolveTool` for conflict resolution.
- **Computer use**: Claude Code can click, type, see screen on macOS. Shannon has no equivalent.

### Resolved

- **Session resume by ID**: `--resume [<UUID>]` accepts optional UUID. `shannon --resume` for most recent, `shannon --resume <uuid>` for specific session. `--continue` / `-c` as alias.
- **Headless permissions**: `FullAuto` by default (auto-approve non-critical, deny critical). `BypassPermissions` only with explicit `--yes` flag.
- **Tool grouping/diff stats/streaming thinking**: All implemented in UI.
- **File watching for source code**: `SourceWatcher` detects project file changes; `DiagnosticStore.sync_from_registry()` bridges diagnostics to UI.
- **Background diagnostics**: `DiagnosticWatcher` auto-runs `cargo check` on source changes, parses output, updates `DiagnosticStore` with debounce.
- **Skill plugin execution**: Skill plugins now register as executable slash commands (trigger → command name, entry file → template).
- **Structured output CLI flag**: `--schema` flag wired into headless mode — accepts file path or inline JSON Schema, instructs assistant to return valid JSON, validates response before output, exit code 1 on failure.
- **Per-agent model/tool config**: `AgentSpawnInput.model` for LLM override, `AgentSpawnInput.allowed_tools` for tool restriction.
- **Worktree isolation for agents**: `context.working_directory` passed to sub-agents with worktree path; system prompt includes isolation instructions.
- **`/batch` command**: Parallel worktree-isolated PR creation via `/batch` or `/parallel` command. Decomposes tasks, creates worktrees, spawns agents, creates PRs.
- **LLM permission classifier wiring**: `LlmPermissionClassifier` wired into `PermissionManager` via `with_llm_classifier()`. Async `classify_and_check_with_llm()` uses LLM fallback for ambiguous cases in Auto modes.
- **Structured JSON output for CI mode**: `StructuredOutputConfig` validates assistant responses against JSON Schema (type checking, required fields). System prompt generation for schema-aware responses.
- **File checkpointing/rewind with diff preview**: `CheckpointManager` creates git commits before file-modifying tools, tracks per-turn changes. `/undo` shows diff preview dialog (file list, stats, full diff viewer) before reverting. `/rewind` for conversation/code/combined restore. Four `RestoreMode` variants. Persistent checkpoint storage.
- **MCP on-demand tool search**: `mcp__tool_search` supports exact lookup (`tool_name`), fuzzy search (`query`), and listing all tools. Deferred schema loading with `deferred_descriptions` for search. Threshold raised to 100 tools for auto-activation.
- **Prompt caching**: Three-layer Anthropic cache breakpoint injection — system prompt (`SystemContentBlock::cached()`), last tool definition (`ToolDefinition.cache_control`), last user message content block (`inject_cache_control_on_last_block()`). Enables prefix-based caching of static content across conversation turns.
- **Agent view dashboard**: `AgentBarWidget` with 3 views (compact/expanded/detailed), `AgentsPanel` via Ctrl+A, sidebar tab. Background agent sessions displayed in real-time.
- **Deep link support**: `shannon://prompt?text=<encoded>` and `shannon://resume?id=<uuid>` URL scheme. Linux/macOS registration via `--register-url-scheme`/`--unregister-url-scheme`. 18 unit tests for URL parsing.
- **MCP channel/webhook**: `WebhookRegistry` with HMAC-SHA256 signing, event type filtering, JSON persistence. `EventPublisher` with non-blocking delivery, exponential backoff retry, rate limiting. 35 webhook-specific tests.
- **Hook event wiring**: All 32 `HookEventType` variants fully wired to actual code paths (SubagentStart/Stop, WorktreeCreate/Remove, PreCompact/PostCompact, ConfigChange, TaskCreated/TaskCompleted).
- **Vision/multimodal integration**: `AnalyzeImageTool` for image analysis (file/URL). Read tool supports image files with base64 encoding. `ToolResultEntry` converts image metadata to `ContentBlock::Image` for LLM vision. 30 new tests.
- **Three-way merge / conflict markers**: LCS-based `three_way_merge()` algorithm. `parse_conflict_markers()` and `resolve_conflicts()`. Edit tool fallback to merge on `old_string` mismatch. `MergeResolveTool` for conflict resolution. 44 new tests.
- **Performance benchmarks**: `criterion` benchmarks for compact engine, file edit, repomap generation, and context budget calculation across relevant crate sizes.
- **Tool result cache**: `ToolResultCache` with TTL-based expiration (5 min default), DashMap-backed concurrent access, file-path invalidation on SourceWatcher changes. Caches read-only tool results (Read/Glob/Grep) in the query engine. 33 tests.
- **Conversation presets**: `/preset` command with 5 built-in templates (code-review, refactor, debug, explain, test). Custom presets via `.shannon.toml` `[presets.*]` sections. Model/temperature/tools/system_prompt overrides. 16 tests.

### Test Coverage

8326 total tests across all crates (58 e2e require API access). Every source file (`src/**/*.rs`) in every crate has at least one `#[test]`. E2e tests (`shannon-cli/tests/cli_e2e_tests.rs`) need Ollama/Anthropic — run with `--skip test_long_conversation --skip test_multiturn` to skip them. Performance benchmarks in `crates/shannon-*/benches/` run via `cargo bench`.

## Competitor Feature Tiers

### Tier 1 — Table Stakes (Shannon has most)
Multi-provider LLM, tool use, file read/write/edit, bash execution, MCP extensions, streaming output, session persistence, context compaction, config files, i18n, skills/commands system.

### Tier 2 — Differentiators (Shannon partially has)
- **Subagent system**: Claude Code has 4 agent mechanisms. Shannon has teammate coordination with per-agent model/tool/worktree config, `/batch` for parallel worktree PRs, and agent view dashboard (`AgentBarWidget`, `AgentsPanel`).
- **Worktree isolation**: `context.working_directory` passes worktree paths to sub-agents. `/batch` creates worktrees automatically. System prompt includes isolation instructions.
- **OS sandbox**: Codex uses macOS Seatbelt/AppArmor/Docker. Shannon uses project-dir sandboxing only.
- **Auto-permission classifier**: Claude Code uses LLM-based 4-tier classification. Shannon has `LlmPermissionClassifier` wired into `PermissionManager` with async `classify_and_check_with_llm()`. Rule-based by default, LLM fallback for ambiguous cases when enabled via `with_llm_classifier()`.
- **LSP integration**: Shannon has 6 LSP tools plus automatic background `cargo check` diagnostics on source changes. OpenCode runs `gopls`/`tsc` automatically.
- **Hook system**: Claude Code has 18+ hook events. Shannon has 32 hook events (more coverage).
- **Non-interactive/CI mode**: Claude Code `claude -p` with structured outputs. Shannon has `--prompt` with NDJSON output, `--schema` for JSON schema validation, and `StructuredOutputConfig` for programmatic use.

### Tier 3 — Quality of Life
Multi-surface (web/desktop/CLI/IDE), computer use.

## Gotchas

- `edition = "2024"` requires Rust 1.85+.
- Integration tests in `shannon-core/tests/` use `mockito::Server` — never hit real APIs.
- The `mockito` server matchers are order-dependent when using `.expect(N)`.
- `LlmClientConfig` must include `max_stream_reconnects` field (all constructors have it).
- `#[allow(dead_code)]` annotations removed from production code (24 files, 45 annotations). Test files may still have them.
