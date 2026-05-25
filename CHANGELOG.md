# Changelog

## Resolved Features

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
- **Performance benchmarks**: `criterion` benchmarks for compact engine, file edit, repomap generation, and context budget calculation across relevant crate sizes. Regression thresholds configured (noise 3%, confidence 98%).
- **Tool result cache**: `ToolResultCache` with TTL-based expiration (5 min default), DashMap-backed concurrent access, file-path invalidation on SourceWatcher changes. Caches read-only tool results (Read/Glob/Grep) in the query engine. 33 tests.
- **Conversation presets**: `/preset` command with 5 built-in templates (code-review, refactor, debug, explain, test). Custom presets via `.shannon.toml` `[presets.*]` sections. Model/temperature/tools/system_prompt overrides. 16 tests.
- **Extended context window**: `ConversationPhase` enum (Initialization/Active/Extended/Critical) with phase-based budget reallocation. `model_context_window()` maps model names to context sizes. `ContextBudget::for_model()`, `current_phase()`, `adapt_for_phase()`, `compaction_threshold()`. 18 tests.
- **Tool orchestration optimization**: `ToolOrchestrationTracker` with DashMap call cache, TTL expiration. `ToolCallOptimizer` analyzing pending calls for dedup/parallel/sequential execution. `OptimizedCallPlan` with intelligent grouping. 22 tests.
- **Session template snapshots**: `/session` command (alias `/snap`) for save/load/list/delete of conversation state as TOML templates. `SessionSnapshot` with model config, messages, enabled tools, system prompt additions. 11 tests.
- **Tool permission profiles**: `PermissionProfile` enum (Strict/Balanced/Permissive/Custom). `ProfileRules` with per-category auto-approve flags. `PermissionManager.apply_profile()` integration. 7 tests.
- **Progressive context loading**: `ProgressiveLoaderConfig` with head/tail preservation, auto-summarize. Read tool enhanced with `truncate_large_files` field for automatic truncation of large files. Metadata includes `total_lines` and `truncated` flag. 15 tests.
- **MCP resource subscription**: `ResourceSubscriptionManager` with DashMap-backed concurrent tracking. `subscribe`/`unsubscribe` per server/URI. `handle_notification()` for `notifications/resources/updated`. Callback dispatch on update. 18 tests.
- **LLM output formatting**: `markdown_table` renderer with box-drawing borders, auto-detected numeric alignment, width optimization. `streaming_diff` tracker with content hash comparison and configurable threshold. 44 tests.
- **Error recovery auto-retry**: `RetryPolicy` with configurable max retries, exponential backoff + jitter, `Retry-After` header support. `ToolError::Timeout` variant. Tool execution wrapped with `tokio::time::timeout`. 23 tests.
