# Changelog

All notable changes to Shannon Code are documented here. Entries are grouped by category.

## Unreleased (dev) — notifications next phase (Bundle A + Bundle B)

### Features

- **Webhook notification sink (Bundle B, commit e697172).** New `WebhookHandler` in `shannon-core::notifier` delivers notifications to any HTTP endpoint with six template formats: Slack (`{"text": "...", "blocks": [...]}`), Discord ({"content": "...", "username": "Shannon"}), Feishu/飞书 (`{"msg_type": "text", "content": {"text": "..."}}`), WeChat Work/企业微信 (`{"msgtype": "text", "text": {"content": "..."}}`), `Custom(String)` for user-supplied templates, and `Raw` (plain JSON envelope). Optional HMAC-SHA256 signing via `X-Shannon-Signature: sha256=<hex>` header when `secret` is configured — matches GitHub/Stripe webhook convention so receivers can verify authenticity. Fire-and-forget via `tokio::spawn` so a slow or unreachable endpoint never blocks the notifier pipeline. `WebhookConfig { url, secret, template, timeout_ms = 3000, include_body = false }` lives under `[notifications.webhook]` in `.shannon.toml`. CLI (`shannon-cli::main::fire_headless_completion_notification`) and desktop (`attach_notification_handler`) both auto-attach the handler when configured. Single-pass template substitution reuses the PR #31 security pattern — substituted values are never re-scanned for placeholders.
- **Desktop click-to-foreground (Bundle A).** `shannon-desktop::main` now listens for `notification-clicked` Tauri events and calls `unminimize + show + set_focus` on the main window. macOS and Windows already focus the app via native bundle-id behavior; this listener is a defensive fallback for Linux DEs and any future Tauri plugin versions that route desktop clicks here.

### Tests

- `shannon-core::notifier`: 17 new unit tests covering all six templates, HMAC signing, sanitization, and config parsing.
- `shannon-core/tests/webhook_integration.rs` (new): 7 mockito-backed integration tests verifying HTTP delivery, HMAC header, non-blocking behavior on slow/unreachable endpoints, runtime-missing error path, and Feishu/WeChat payload schemas.

## v0.5.2 (2026-06) — notifications feature (Phase 1 + Phase 2 + wiring)

### Features

- **Notifications core types + config (Phase 1, PR #30).** New `NotificationsConfig` and `NotificationCooldownConfig` in `shannon-core::notifier` with `interactive_default()` (sound on, level=info) and `headless_default()` (disabled — opt-in via config or `--notify`). `Cooldown` struct (DashMap-backed) provides per-source dedup with configurable windows (`permission_ms=0`, `query_complete_ms=0`, `tool_complete_ms=3000`, `error_ms=5000`, `agent_idle_ms=10000`). `Notification` struct gains `source: Option<String>` and `action_id: Option<String>` for richer routing. `Notifier` gains `with_cooldown()`, `with_minimum_level()`, `notify_dedup()` which returns `Ok(false)` when suppressed. `ShannonConfig.notifications: Option<NotificationsConfig>` with merge semantics. `NotificationLevel` serde-hardened with `rename_all="snake_case"` and `alias="critical"` for back-compat.
- **CLI shell-out notifier (Phase 2, PR #31).** New `shannon-cli::notifications::ShellNotifier` fires OS-native notifications by spawning platform binaries: `notify-send` (Linux), `osascript` (macOS), `powershell BurntToast` (Windows). Spawns via `std::process::Command` args array (no shell). New `--notify` CLI flag opt-in for headless mode. `fire_headless_completion_notification` maps exit code → notification level (success/warning/error) with source key `headless:{exit_code:?}`.
- **REPL notification wiring (PR #33).** Sidebar's `refresh_agents` now routes agent-completion events through the shared `Notifier` via `notify_dedup(&notification, 10_000)` so the `notifications_enabled` gate is honored and same-agent successive refreshes coalesce within a 10s window (previously constructed a fresh `DesktopNotifier` per iteration and called `.send()` directly, bypassing both gate and cooldown). `ReplState::new()` attaches `Cooldown::new()` to the shared `Notifier` so `notify_dedup` actually dedups across all callers. `loop_engine::notify_query_complete` switches from `notify` to `notify_dedup(..., 0)` — source key already set, window=0 matches the configured `query_complete_ms` default.

### Security

- **Shell-out injection hardening (commit f0d2675 on PR #31).** Security review of the initial P2 implementation identified three issues, all fixed before merge:
  - **AppleScript command injection** (CRITICAL): macOS template wraps values in `"..."` AppleScript strings but `sanitize()` did not escape `"` or `\`. A malicious title like `Evil ") & (do shell script "rm -rf ~")` could break out and execute arbitrary shell commands. Fixed: `escape_applescript()` escapes `\` first then `"`.
  - **PowerShell command injection** (CRITICAL): Windows template wraps values in `'...'` PowerShell strings but `'` was not escaped. Fixed: `escape_powershell()` doubles single quotes (the correct PowerShell escape).
  - **Template injection** (HIGH): Chained `str::replace` calls re-scan substituted values, so a title containing literal `{body}` would have body content injected. Fixed: single-pass `substitute()` helper scans the template once — substituted values are not re-interpreted.
  - **Arbitrary binary execution** (HIGH, acknowledged): `ShellNotifier::with_spec()` accepts any binary path; documented as a developer-API trust boundary. Platform-default path (`CommandSpec::platform_default()`) is the only user-reachable path. MVP does not expose config-driven binary selection.
  - **Test coverage**: 11 new unit tests verify escaping correctness via balanced-quote counters that simulate AppleScript/PowerShell parsing. The exact malicious payloads from the security report are used as test inputs.

### Tests

- P1: +15 unit tests in `shannon-core::notifier`.
- P2: +20 unit tests in `shannon-cli::notifications` (9 original + 11 security hardening).

## v0.5.1 (2026-06) — `.mcpb` install security hardening

### Security

- **Symlink path traversal**: `shannon mcp install` now refuses to follow symlinks for the target file (`.mcp.json` or `~/.shannon/settings.json`), blocking a planted symlink from redirecting writes to arbitrary files.
- **Zip bomb DoS**: `.mcp.json` entries larger than 10 MB uncompressed are rejected before reading.
- **Data loss on parse error**: An existing settings file that fails to parse as JSON now aborts the install (preserving the original file) instead of being silently reset to `{}`.
- **Install preview + confirmation**: The CLI now prints each server's `name -> command args` with `[OVERWRITE]` markers and prompts `[y/N]` before writing. `--yes` skips the prompt for scripts; `--dry-run` previews without writing.

## v0.5.0 (2026-06) — Sprint 5: Deepen MCP Integration

### MCP

- **Elicitation TUI**: Server-initiated `elicitation/create` requests surface as a ratatui `InputDialog` in the REPL, with responses delivered back over a bounded mpsc + oneshot channel. UI prefix `[EXTERNAL MCP · <server>]` distinguishes server-originated prompts from Shannon's own dialogs, capped at 200 chars to prevent spoofing abuse.
- **MCP prompts as slash commands**: Server prompts auto-register as `/{server}:{prompt}` aliases alongside the canonical `/mcp__{server}__{prompt}` form. New `/mcp prompts` lists every server prompt with descriptions.
- **Tab autocomplete via `completion/complete`**: Typing an argument after an MCP prompt slash command queries the originating server for argument completions (800ms timeout, silent fallback to local completion on miss).
- **`.mcpb` bundle install**: `shannon mcp install <bundle> [--user]` extracts a `.mcpb` zip archive (containing `.mcp.json`) and merges `mcpServers` into either the project's `.mcp.json` or `~/.shannon/settings.json`. Preserves existing servers and non-mcp keys; overwrites same-name entries.

### Security

- **Elicitation channel hardening**: Bounded `mpsc::channel(16)` replaces unbounded sender to prevent flood-based DoS.
- **Spoofing-resistant UI**: Server-originated dialogs visually distinct from Shannon's own.

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
