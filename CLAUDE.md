# Shannon Code

Rust-based AI code assistant (like Claude Code) with multi-provider LLM support, MCP-based extensions, and terminal UI.

## Build & Test

```bash
cargo build                    # Build workspace
cargo check --workspace        # Fast type-check
just test                      # Run all tests (nextest, faster than cargo test)
just dev                       # check + lint + test, run before commits
cargo clippy --workspace       # Lint
```

Install: `cargo install just cargo-nextest`. Config in `.config/nextest.toml` handles per-crate thread limits.

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

- **Use `just test` (nextest)** for running tests. nextest isolates each test in its own process. Config in `.config/nextest.toml` handles per-crate thread limits (shannon-core and shannon-commands run single-threaded).
- **Fallback**: `cargo test --workspace -- --test-threads=1` works without nextest but is slower.
- **Inline tests**: Most crates use `#[cfg(test)] mod tests` within source files. Tests near the code they test.
- **Integration tests**: `crates/shannon-*/tests/` directories for cross-module testing.
- **Mockito**: For HTTP API tests. Server matchers are order-dependent with `.expect(N)`.
- **Test helpers**: `CollectingSender` (progress sender), `tempfile::TempDir` (file tests), `mockito::Server` (HTTP tests).

### Test Commands (justfile)

`just test` runs everything without API keys. Install: `cargo install just`.

| Command | What | Needs key? |
|---------|------|-----------|
| `just test` | All unit + mock tests | No |
| `just scenarios` | YAML scenario tests | No |
| `just perf` | Performance regression | No |
| `just bench` | Criterion benchmarks | No |
| `just record` | Record real API fixtures | Yes |
| `just replay` | Replay recorded fixtures | No |
| `just ci` | Full CI suite (no key) | No |

### Test File Map

**shannon-core/tests/** (component-level):
- `api_integration.rs` — mockito HTTP tests
- `multi_turn_conversation.rs` — multi-turn mockito conversations
- `streaming_stress.rs` — streaming stress tests
- `snapshot_regression.rs` — insta snapshot tests
- `perf_tests.rs` — performance thresholds + E2E latency
- `scenario_tests.rs` — YAML scenario parser/validator/runner

**shannon-cli/tests/** (CLI-level):
- `cli_args_tests.rs` — argument parsing
- `cli_interactive_tests.rs` — interactive mode
- `cli_e2e_tests.rs` — comprehensive provider mock tests
- `cli_mock_tests.rs` — tool pipeline + provider scenario mocks
- `live_tests.rs` — real API tests (Ollama/DeepSeek/Anthropic) + record/replay

**YAML scenarios**: `tests/scenarios/*.yaml` — 10 declarative test scenarios

### Recording Real API Fixtures

```bash
# Record (local, needs API key):
SHANNON_API_KEY=sk-ant-... just record

# Replay (CI, no key needed):
just replay
```

## Known Gaps (vs Claude Code / Codex CLI / OpenCode)

### HIGH — Shannon has partial support

- **Permission auto-mode**: 9 `ApprovalMode` variants with `PermissionClassifier` (2928 lines) wired into `PermissionRuleChecker`. `LlmPermissionClassifier` wraps the rule-based classifier with async LLM fallback for ambiguous cases (confidence < 0.7, Medium+ risk). 4-tier precedence: hard_deny > soft_deny > allow > explicit intent. LLM classification disabled by default, enabled via `with_llm()`.
- **Non-interactive/CI mode**: `--prompt` flag with FullAuto permissions (auto-approve non-critical, deny critical). NDJSON streaming, tool restrictions, exit codes. `--schema` flag accepts file path or inline JSON Schema for structured output validation. Deep link support via `shannon://prompt?text=<encoded>` and `shannon://resume?id=<uuid>` URL scheme with `--register-url-scheme`/`--unregister-url-scheme` commands.
- **MCP tool search**: `tools/list` works with deferred schema loading. MCP webhook/channel support with `WebhookRegistry` (HMAC-SHA256 signing, event filtering, persistence), `EventPublisher` (non-blocking delivery, retry with exponential backoff), and event firing from `McpProcessPool` (ServerConnected/Disconnected, ToolCallStarted/Completed, NotificationReceived).
- **Hook system**: `HookManager` with `HookEvent`/`HookEventType`. 32 event types fully wired: SubagentStart/Stop, WorktreeCreate/Remove, PreCompact/PostCompact, ConfigChange, TaskCreated/TaskCompleted, plus all original events. Hook events automatically trigger matching routines via `TriggeredRoutineRegistry`.
- **LSP integration**: 6 LSP tools + `DiagnosticRegistry` + two client implementations. `DiagnosticStore.mark_stale()` called on source file changes. Background `cargo check` diagnostics auto-run via `DiagnosticWatcher` when source files change — debounce, parse, display in UI.
- **Plugin system**: `PluginRegistry` with manifest parsing. Tool plugins fully wired (MCP discovery). Command plugins register as `PromptCommand` in `CommandRegistry` (source: `Plugin`). Skill plugins register as `PromptCommand` with trigger as slash command name and entry file as template. Loading in both REPL (`new()`) and CLI headless mode.
- **Desktop app**: Scaffolded Tauri app with TODO stubs.

### MEDIUM — Quality-of-life gaps

- **Computer use**: Claude Code can click, type, see screen on macOS. Shannon has no equivalent.

(Resolved features moved to [CHANGELOG.md](CHANGELOG.md).)

### Test Coverage

~9500 total tests across all crates (58 e2e require API access). Every source file (`src/**/*.rs`) in every crate has at least one `#[test]`. E2e tests (`shannon-cli/tests/cli_e2e_tests.rs`) need Ollama/Anthropic — run with `--skip test_long_conversation --skip test_multiturn` to skip them. Performance benchmarks in `crates/shannon-*/benches/` run via `cargo bench`.

## Competitor Feature Tiers

### Tier 1 — Table Stakes (Shannon has most)
Multi-provider LLM, tool use, file read/write/edit, bash execution, MCP extensions, streaming output, session persistence, context compaction, config files, i18n, skills/commands system.

### Tier 2 — Differentiators (Shannon has)
- **Subagent system**: Claude Code has 4 agent mechanisms. Shannon has teammate coordination with per-agent model/tool/worktree config, `/batch` for parallel worktree PRs, and agent view dashboard (`AgentBarWidget`, `AgentsPanel`). Agent definitions loaded from `.shannon/agents/*.toml` and `.claude/agents/*.md` with local-overrides-global priority.
- **Agent Teams**: Full team coordination via `TeamCreate`, `SendMessage`, `TaskCreate/Update/List`. Team prompt injection when team tools detected. `/team` REPL command for team management. Teammates self-claim tasks and auto-notify on idle.
- **Worktree isolation**: `context.working_directory` passes worktree paths to sub-agents. `/batch` creates worktrees automatically. System prompt includes isolation instructions.
- **Auto-permission classifier**: Claude Code uses LLM-based 4-tier classification. Shannon has `LlmPermissionClassifier` wired into `PermissionManager` with async `classify_and_check_with_llm()`. Rule-based by default, LLM fallback for ambiguous cases when enabled via `with_llm_classifier()`.
- **Permission profiles**: Named presets loaded from `.shannon/profiles/*.toml` and `.claude/profiles/*.toml` with local-overrides-global. `/profile` command to switch presets. `CustomProfileRegistry` with auto_approve/confirm/deny lists.
- **LSP integration**: Shannon has 6 LSP tools plus automatic background `cargo check` diagnostics on source changes. OpenCode runs `gopls`/`tsc` automatically.
- **Hook system**: Claude Code has 18+ hook events. Shannon has 32 hook events (more coverage). Hook events auto-execute matching routines from `.shannon/routines.toml`.
- **Routines**: Triggered routines (hook-event-driven, e.g., auto-lint after edits) and scheduled routines (interval-based, cron-like). `TriggeredRoutineRegistry` wired into PostToolUse hook pipeline. `/routine` command for management.
- **Non-interactive/CI mode**: Claude Code `claude -p` with structured outputs. Shannon has `--prompt` with NDJSON output, `--schema` for JSON schema validation, and `StructuredOutputConfig` for programmatic use.
- **VS Code extension**: Scaffolded extension with WebView chat panel, NDJSON subprocess communication with `shannon --prompt`.

### Tier 3 — Quality of Life
Computer use.

## Gotchas

- `edition = "2024"` requires Rust 1.85+.
- Integration tests in `shannon-core/tests/` use `mockito::Server` — never hit real APIs.
- The `mockito` server matchers are order-dependent when using `.expect(N)`.
- `LlmClientConfig` must include `max_stream_reconnects` field (all constructors have it).
- `#[allow(dead_code)]` annotations in production code: 61 remaining, all annotated with `// KEEP: <reason>` comments. Categories: cross-platform stubs, deserialized fields, command template dynamic dispatch, test-only utilities, struct ownership, watcher lifecycle fields. Dead constants, duplicate functions, and unused error types have been removed.
