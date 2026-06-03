# Changelog

All notable changes to Shannon Code are documented here. Entries are grouped by category.

## v0.1.0 (2026-05)

Initial public release with full feature set.

### Core Features

- **Multi-provider LLM support**: Anthropic, OpenAI, Ollama, DeepSeek, any OpenAI-compatible endpoint via adapter pattern
- **Streaming query processing**: SSE byte stream → `SseStream` → `MessageStream` with chunk boundary buffering
- **Session management**: Persistence, history, search, resume by ID (`--resume`, `--continue`)
- **Context compression**: Auto-compact, micro-compact, conversation phase tracking (Initialization → Active → Extended → Critical)
- **Prompt caching**: Three-layer Anthropic cache breakpoint injection — system prompt, last tool definition, last user message
- **Extended context window**: Phase-based budget reallocation, model-aware context sizes
- **Progressive context loading**: Head/tail preservation, auto-summarize, automatic truncation of large files

### Tool System

- **File operations**: Read, Edit, Write, MultiEdit with three-way merge and conflict resolution
- **Bash execution**: Sandboxed shell commands with streaming output and timeout control
- **Git integration**: Status, diff, log, commit, branch management
- **Web search**: Real-time information retrieval
- **Image analysis**: Screenshot understanding via `AnalyzeImageTool`
- **Notebook editing**: Jupyter notebook cell read/edit/insert/delete
- **Tool result cache**: TTL-based expiration, DashMap concurrent access, file-path invalidation
- **Tool orchestration optimization**: Dedup/parallel/sequential execution analysis, intelligent call grouping

### MCP (Model Context Protocol)

- **Full protocol implementation**: stdio, SSE, streamable HTTP transports
- **Dynamic tool registration**: `tools/list` with deferred schema loading
- **On-demand tool search**: `mcp__tool_search` with exact lookup and fuzzy search
- **Resource management**: Subscription tracking, update notifications
- **Webhook/channel support**: HMAC-SHA256 signing, event filtering, exponential backoff retry

### Multi-Agent System

- **Team coordination**: `TeamCreate`, `SendMessage`, `TaskCreate/Update/List`
- **Worktree isolation**: Per-agent git worktrees with working directory isolation
- **Per-agent config**: Model override, tool restrictions, working directory
- **`/batch` command**: Parallel worktree-isolated PR creation
- **Agent dashboard**: `AgentBarWidget` with 3 views, `AgentsPanel` sidebar

### Permission & Safety

- **Rule-based classifier**: Pattern matching for known safe/dangerous operations
- **LLM auto-classifier**: Async fallback for ambiguous cases (confidence < 0.7)
- **Permission profiles**: Strict, Balanced, Permissive, Custom (`.shannon/profiles/*.toml`)
- **4-tier precedence**: Hard deny > Soft deny > Allow > Explicit intent
- **Headless permissions**: `FullAuto` by default, `BypassPermissions` only with explicit `--yes`

### Commands & Skills

- **Built-in commands**: `/help`, `/config`, `/model`, `/compact`, `/undo`, `/rewind`, `/diff`, `/batch`, `/team`, `/cost`, `/search`, `/doctor`, `/routine`, `/preset`, `/session`
- **Skill framework**: Discovery, loading, execution from `.shannon/skills/` and plugins
- **Plugin system**: Manifest parsing, tool/command/skill plugin types
- **Hook system**: 32+ events (tool execution, compaction, config changes, agent lifecycle)
- **Triggered routines**: Hook-event-driven auto-execution (e.g., auto-lint after edits)

### Terminal UI

- **Interactive REPL**: Command history, search, vim mode
- **Markdown rendering**: Syntax highlighting, collapsible thinking, tool grouping
- **Diff visualization**: Colored output with stats
- **Token counter**: Context window bar, cost tracking, cache stats
- **Virtual scroll**: Progress indicators

### CI & Non-Interactive Mode

- **`--prompt` flag**: Non-interactive mode with NDJSON streaming
- **`--schema` flag**: JSON Schema validation for structured output
- **`--pipe` flag**: Pipe mode for automated workflows
- **`--diff-only` flag**: Only output file diffs
- **Tool restrictions**: `--allowed-tools`, `--max-turns` for CI safety
- **Deep links**: `shannon://prompt?text=<>` and `shannon://resume?id=<>` URL scheme

### Infrastructure

- **LSP integration**: 6 LSP tools, automatic background `cargo check` diagnostics
- **Memory system**: Persistent store, auto-dream extraction, consolidation
- **File checkpointing**: Git-based checkpoints with diff preview before revert
- **Auto-updater**: GitHub Releases-based update checking
- **Diagnostics & Doctor**: Environment health checks, error pattern analysis
- **Performance benchmarks**: `criterion` benchmarks with regression thresholds
- **i18n**: 10 languages via `rust-i18n`
- **VS Code extension**: WebView chat panel, diff viewer, NDJSON communication
- **Error recovery**: Configurable retry with exponential backoff + jitter

### Testing

- ~7,889 tests across 12 crates
- Every `src/**/*.rs` has at least one `#[test]`
- `mockito` HTTP mocking — never hits real APIs
- YAML declarative scenario tests
- Record/replay system for real API fixtures
- Performance regression thresholds
