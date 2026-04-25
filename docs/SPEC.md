# Shannon Code — Implementation Specification

> **Version**: 0.1.0
> **Date**: 2026-04-04
> **Status**: Active Development

---

## Table of Contents

- [1. Introduction](#1-introduction)
- [2. Architecture Overview](#2-architecture-overview)
- [3. Crate Specifications](#3-crate-specifications)
  - [3.1 shannon-core](#31-shannon-core)
  - [3.2 shannon-tools](#32-shannon-tools)
  - [3.3 shannon-agents](#33-shannon-agents)
  - [3.4 shannon-ui](#34-shannon-ui)
  - [3.5 shannon-mcp](#35-shannon-mcp)
  - [3.6 shannon-commands](#36-shannon-commands)
  - [3.7 shannon-skills](#37-shannon-skills)
  - [3.8 shannon-types](#38-shannon-types)
  - [3.9 shannon-cli](#39-shannon-cli)
- [4. Core Interfaces](#4-core-interfaces)
- [5. Configuration](#5-configuration)
- [6. Data Storage](#6-data-storage)
- [7. Security Model](#7-security-model)
- [8. Error Handling Strategy](#8-error-handling-strategy)
- [9. Project Statistics](#9-project-statistics)

---

## 1. Introduction

### 1.1 Project Overview

Shannon Code is a high-performance, type-safe AI-assisted coding tool written entirely in Rust. It provides a terminal-based REPL (Read-Eval-Print Loop) interface for interacting with large language models while offering advanced capabilities including tool orchestration, multi-agent coordination, session management, MCP (Model Context Protocol) extensibility, and skill framework support.

The project is built from the ground up as an independent implementation, using only publicly available documentation, open specifications, and general software engineering principles.

### 1.2 Design Philosophy

| Principle | Description |
|-----------|-------------|
| **Memory Safety** | Guaranteed at compile time via Rust's ownership system; no data races |
| **High Performance** | Zero-cost abstractions, near-C speed with async I/O |
| **Type Safety** | Strong type system catches bugs before runtime |
| **Native Concurrency** | `tokio` async runtime for parallel operations |
| **Extensibility** | MCP protocol, skill framework, and hook system |
| **Composability** | 9 modular crates with clean separation of concerns |

### 1.3 Technology Stack

| Component | Technology | Purpose |
|-----------|-----------|---------|
| Language | Rust 1.85+ (Edition 2024) | Core implementation |
| Async Runtime | tokio 1.43 | Async I/O, task scheduling |
| CLI Framework | clap 4.5 | Argument parsing |
| Terminal UI | ratatui 0.29 + crossterm 0.28 | TUI rendering |
| HTTP Client | reqwest 0.12 | API communication |
| Serialization | serde 1.0 + serde_json 1.0 + serde_yaml 0.9 | JSON/YAML handling |
| Error Handling | thiserror 2.0 + anyhow 1.0 | Error types |
| Logging | tracing 0.1 + tracing-subscriber 0.3 | Structured logging |
| UUID | uuid 1.12 | Unique identifiers |
| Datetime | chrono 0.4 | Time handling |
| Filesystem | dirs 5.0 | Platform directories |
| Env Config | dotenvy 0.15 | .env file parsing |
| State | dashmap 6.1 | Concurrent state management |
| Pattern Matching | regex 1.11 | Hook matching, classification |
| Benchmarking | criterion 0.5 | Performance benchmarks |

### 1.4 License

Dual-licensed under **MIT** or **Apache-2.0** at the user's choice.

---

## 2. Architecture Overview

### 2.1 System Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           shannon-cli (Entry Point)                       │
│                         clap CLI: repl | version | config                  │
└──────────────────────────────┬──────────────────────────────────────────┘
                               │
┌──────────────────────────────▼──────────────────────────────────────────┐
│                           shannon-ui (Terminal)                         │
│    REPL ──┬── Vim Mode ── Markdown Renderer ── Diff Viewer ── Widgets   │
└──────────────────────────────┬──────────────────────────────────────────┘
                               │
┌──────────────────────────────▼──────────────────────────────────────────┐
│                         shannon-core (Engine)                          │
│                                                                             │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────┐  ┌────────────┐  │
│  │ QueryEngine  │  │ LlmClient   │  │ ToolRegistry │  │ Permission   │  │
│  │ (Streaming)  │  │ (Multi-LLM) │  │ (Dynamic)   │  │ Manager     │  │
│  └──────┬──────┘  └──────┬───────┘  └──────┬───────┘  └──────┬─────┘  │
│         │                │                │                 │         │
│  ┌──────▼────────────────▼────────────────▼─────────────────▼─────┐   │
│  │                  StreamingToolExecutor                          │   │
│  │            (Concurrent Tool Execution)                         │   │
│  └───────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │  State │ Settings │ Hooks │ MCP │ Memory │ Analytics │ ...  │   │
│  └──────────────────────────────────────────────────────────────────┘   │
└──────┬──────────────┬──────────────┬──────────────┬──────────────────────┘
       │              │              │              │
┌──────▼──────┐ ┌────▼───────┐ ┌────▼──────┐ ┌────▼──────────────┐
│ shannon-   │ │ shannon-  │ │ shannon-  │ │ shannon-          │
│ tools     │ │ agents    │ │ mcp      │ │ commands/skills  │
│ (28 tools)│ │ (Multi-   │ │ (MCP      │ │ (Extensibility)  │
│            │ │  Agent)   │ │  Protocol)│ │                   │
└────────────┘ └───────────┘ └──────────┘ └───────────────────┘
```

### 2.2 Workspace Layout

```
shannon-code/
├── Cargo.toml                    # Workspace root
├── Cargo.lock
├── LICENSE
├── README.md
├── docs/
│   └── SPEC.md                  # This document
└── crates/
    ├── shannon-core/             # Core engine (52 modules, ~49,000 lines)
    │   └── src/
    │       ├── lib.rs           # Module declarations + re-exports
    │       ├── query_engine/    # Query orchestrator with streaming & compression
    │       ├── tools.rs         # Tool trait + registry
    │       ├── api.rs           # API client (streaming)
    │       ├── permissions.rs   # Permission system
    │       ├── state.rs         # Session persistence
    │       ├── credential_manager.rs  # Secure credential storage
    │       ├── updater.rs       # Auto-update via GitHub Releases
    │       └── ...              # 45 more modules
    ├── shannon-tools/            # Tool implementations (25,338 lines)
    │   └── src/
    │       ├── file/            # File operations (8 files)
    │       ├── system.rs        # Bash, Sleep, PowerShell
    │       ├── git.rs           # Git integration (5 tools)
    │       ├── web.rs           # WebFetch, WebSearch
    │       ├── lsp.rs           # LSP integration (4 tools)
    │       └── ...              # 20 more tool modules
    ├── shannon-agents/           # Multi-agent system (6,580 lines)
    │   └── src/
    │       ├── coordinator.rs   # Team orchestration
    │       ├── teammate.rs      # Agent lifecycle
    │       ├── task_board.rs    # Shared task state
    │       └── ...
    ├── shannon-ui/               # Terminal UI (5,981 lines)
    │   └── src/
    │       ├── repl.rs          # Main REPL loop
    │       ├── vim.rs           # Vim keybindings
    │       ├── render.rs        # Markdown + syntax
    │       └── widgets/         # TUI components
    ├── shannon-mcp/              # MCP protocol (2,269 lines)
    │   └── src/
    │       ├── protocol.rs      # JSON-RPC + MCP types
    │       ├── transport.rs     # stdio/SSE/HTTP/WebSocket
    │       └── ...
    ├── shannon-commands/        # Slash commands (3,101 lines)
    │   └── src/
    │       ├── builtin/         # Built-in commands (commit, config, credentials, etc.)
    │       └── ...
    ├── shannon-skills/          # Skill framework (2,286 lines)
    │   └── src/
    │       ├── definition.rs   # Skill types
    │       ├── loader.rs        # Disk loading
    │       └── ...
    ├── shannon-types/            # Shared types (37 lines)
    └── shannon-cli/              # CLI entry point
```

### 2.3 Inter-Crate Dependency Graph

```
shannon-cli
  └─→ shannon-ui
        └─→ shannon-core
              ├─→ shannon-tools
              │     └─→ shannon-core (re-exports)
              ├─→ shannon-agents
              ├─→ shannon-mcp
              ├─→ shannon-commands
              └─→ shannon-skills
                    └─→ shannon-core
```

### 2.4 Data Flow

```
User Input (terminal)
    │
    ▼
┌─────────────┐
│   REPL       │  Input parsing, history, command dispatch
└─────┬───────┘
      │
      ▼
┌─────────────┐     ┌──────────────┐     ┌───────────────┐
│ QueryEngine │────→│ LlmClient   │────→│ LLM Provider  │
│             │◄────│ (SSE Stream) │◄────│ (Multi-vendor)│
└─────┬───────┘     └──────────────┘     └───────────────┘
      │                      │
      │  StreamEvent:       │  ContentBlock::ToolUse
      │  - text delta        │
      │  - tool use          ▼
      │               ┌──────────────┐
      │               │ Permission   │  RiskLevel check
      │               │ Manager     │  → Allow/Deny prompt
      │               └──────┬───────┘
      │                      │
      │                      ▼
      │               ┌──────────────┐     ┌───────────────────┐
      │               │ ToolRegistry │────→│ Tool::execute()  │
      │               │              │     │ (concurrent)      │
      │               └──────────────┘     └────────┬──────────┘
      │                                              │
      │                      ┌───────────────────────┐
      │                      │ ToolOutput (content)   │
      │                      └───────────┬───────────┘
      │                                  │
      ▼                                  ▼
┌─────────────┐     ┌──────────────┐     ┌───────────────┐
│ CostTracker │     │ StateManager │     │ MemoryStore  │
│ (tokens/USD) │     │ (persist)    │     │ (auto-dream) │
└─────────────┘     └──────────────┘     └───────────────┘
      │
      ▼
  Response to User (terminal, rendered markdown)
```

---

## 3. Crate Specifications

### 3.1 shannon-core

**Path**: `crates/shannon-core/`
**Lines**: 46,789
**Modules**: 52
**Test Files**: 52

The core engine provides query processing, tool orchestration, security, state management, and all infrastructure services.

#### 3.1.1 Query Processing

| Module | Key Types | Description |
|--------|-----------|-------------|
| `query_engine` | `QueryEngine`, `QueryContext`, `QueryEvent`, `CostTracker` | Main orchestrator. Receives user input, streams API calls, manages tool execution loop, tracks costs. Supports concurrent tool dispatch. |
| `streaming_tool_executor` | `StreamingToolExecutor`, `TrackedTool`, `ToolStatus` | Concurrent tool execution with state machine: `Queued → Executing → Completed → Yielded`. Tracks sibling abort signals. |
| `tool_execution` | `ToolExecutionService`, `ToolExecutionResult`, `ToolProgress` | Unified tool execution lifecycle: permission checks, telemetry, error handling, progress callbacks. |
| `compact` | `CompactEngine`, `CompactConfig`, `CompactResult`, `CompactStrategy`, `MessageGroup` | Context compression with multiple strategies: auto-compact, micro-compact, session-memory compact. Configurable thresholds. |

**Query Processing Flow**:
1. `QueryEngine::process()` receives user message
2. Builds `MessageRequest` with conversation history + tool definitions
3. `LlmClient` sends streaming request to LLM provider
4. Stream events are processed incrementally:
   - `ContentDelta` → accumulated response text
   - `ToolUse` → permission check → tool execution → result fed back
5. `CostTracker` records token usage and calculates cost
6. Context compression triggered when approaching token limits
7. Final response assembled and returned

#### 3.1.2 Tool System

| Module | Key Types | Description |
|--------|-----------|-------------|
| `tools` | `Tool` (trait), `ToolInfo`, `ToolOutput`, `ToolRegistry` | Dynamic tool registration and dispatch. `ToolRegistry` maintains a `HashMap<String, Arc<dyn Tool>>`. |

**Tool Trait**:
```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;  // JSON Schema
    async fn execute(&self, input: Value) -> ToolResult<ToolOutput>;
    fn requires_auth(&self) -> bool { false }
    fn category(&self) -> &str { "general" }
}
```

**ToolOutput**:
```rust
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
    pub metadata: HashMap<String, Value>,
}
```

#### 3.1.3 API Client (Multi-Provider)

| Module | Key Types | Description |
|--------|-----------|-------------|
| `api` | `LlmClient`, `LlmClientConfig`, `LlmProvider`, `MessageStream`, `ContentBlock`, `ContentDelta`, `Message`, `MessageContent`, `MessageRequest`, `MessageResponse`, `StreamEvent`, `ToolDefinition`, `Usage` | Vendor-agnostic LLM client with SSE streaming. Supports Anthropic, OpenAI, Ollama, and custom providers. Backward-compatible aliases: `ClaudeClient = LlmClient`. |

**Supported Providers**:

| Provider | Base URL | Auth Method | API Endpoint |
|----------|----------|-------------|--------------|
| `LlmProvider::Anthropic` | `api.anthropic.com` | `x-api-key` header | `POST /v1/messages` |
| `LlmProvider::OpenAI` | `api.openai.com` | `Authorization: Bearer` | `POST /v1/chat/completions` |
| `LlmProvider::Ollama` | `localhost:11434` | None | `POST /api/chat` |
| `LlmProvider::Custom` | User-defined | Configurable | Configurable |

**Environment Variable Priority** (per setting):
```
SHANNON_* → ANTHROPIC_* → OPENAI_* → built-in default
```

**Provider Auto-Detection**:
- `LlmClient::from_env()` checks base URLs and API keys to infer provider
- Custom `base_url` automatically selects `LlmProvider::Custom`

**Streaming Protocol**:
- `StreamEvent::TextDelta` — Partial text response
- `StreamEvent::ToolUse` — Request to call a tool
- `StreamEvent::EndTurn` — End of assistant turn
- `StreamEvent::Usage` — Token usage statistics

#### 3.1.4 Security

| Module | Key Types | Description |
|--------|-----------|-------------|
| `permissions` | `PermissionManager`, `Permission`, `PermissionLevel`, `RiskLevel`, `PermissionChoice`, `PermissionPrompt` | Rule-based permission system. 5 risk levels (Safe → Critical). Supports allow/deny rules, permission prompts. |
| `permission_classifier` | `PermissionClassifier`, `PermissionRule`, `PermissionRuleParser`, `ClassificationResult`, `DangerousPattern`, `RuleDecision`, `RiskLevel` | Pattern-based command classification. 10+ built-in dangerous bash patterns (rm -rf, mkfs, dd, chmod 777, etc.). Rule parser for custom rules. |
| `policy_limits` | `PolicyLimits`, `PolicyLimitsManager`, `PolicyCheckResult` | Rate limiting, request quotas, budget controls. |
| `rate_limit` | `RateLimiter`, `RateLimitConfig`, `TokenBucket`, `ExponentialBackoff` | Token bucket rate limiter with configurable refill rate. Exponential backoff for retries. |

**Permission Decision Flow**:
```
Tool Call Request
    │
    ▼
PermissionClassifier.classify()
    │
    ├─ Safe → Auto-approve
    ├─ Low → Auto-approve (configurable)
    ├─ Medium → Prompt user
    ├─ High → Prompt user (with warning)
    └─ Critical → Prompt user (with danger alert)
         │
         ▼
    PermissionChoice
    ├─ Deny → Skip tool execution
    ├─ AllowOnce → Execute once, re-prompt next time
    └─ AlwaysAllow → Add to allowlist, skip future prompts
```

#### 3.1.5 State & Session

| Module | Key Types | Description |
|--------|-----------|-------------|
| `state` | `StateManager`, `SessionState`, `SessionData`, `SessionInfo`, `SessionPersistMetadata` | Persistent session management. Sessions stored as JSON files in `~/.shannon/sessions/`. Thread-safe via `DashMap`. |
| `session_history` | `SessionHistoryManager`, `SessionHistoryEntry`, `SessionFilter`, `ResumeInfo`, `SessionMetadata` | Session listing, search, archive, and resumption. Supports filtering by date, tags, project. |
| `session_transcript` | `TranscriptStore`, `TranscriptEntry`, `TranscriptRole`, `TranscriptQuery`, `ToolCallRecord` | Full conversation transcript persistence in JSONL format. Searchable by content, role, time range. |

#### 3.1.6 Configuration

| Module | Key Types | Description |
|--------|-----------|-------------|
| `settings` | `Settings`, `SettingsManager`, `SettingsError` | Hierarchical settings with full priority chain: `settings.json` → `.env` files → environment variables → CLI `-e` flags. |
| `project_memory` | `ProjectMemoryManager`, `ProjectMemoryConfig`, `ProjectMemoryMetadata`, `MergedMemory`, `MemorySource` | Priority-based project memory loading: Global CLAUDE.md → Global SHANNON.md → Project CLAUDE.md → Project SHANNON.md (later overrides earlier). Backward-compatible aliases: `ClaudeMdManager = ProjectMemoryManager`. |
| `remote_settings` | `RemoteSettingsProvider`, `RemoteManagedSettings`, `SettingOverride`, `SettingSource` | Remote-managed configuration overrides. Supports rollout percentage, A/B testing. |
| `settings_sync` | `SettingsSyncService`, `SyncRecord`, `SyncStatus`, `DeviceRegistry` | Cross-device settings synchronization. Device registry with conflict resolution. |

**Configuration Priority Chain**:
```
1. settings.json (user: ~/.shannon/settings.json, project: .shannon/settings.json)
2. .env files (.env → .env.local → .env.production)
3. Environment variables (SHANNON_* → ANTHROPIC_* → OPENAI_*)
4. CLI flags (-e KEY=VALUE)
```

#### 3.1.7 Hooks

| Module | Key Types | Description |
|--------|-----------|-------------|
| `hooks` | `HookManager`, `HookEvent`, `HookResult`, `HookDecision`, `HookEventType` | Event-driven hook system. Events: `PreToolUse`, `PostToolUse`, `PreQuery`, `PostQuery`, `SessionStart`, `SessionEnd`. |
| `tool_hooks` | `ToolHookChain`, `ToolHook`, `ToolHookResult`, `ToolHookDecision`, `ToolHookContext` | Tool-specific hook chain. Built-in hooks: `PermissionToolHook`, `LoggingToolHook`, `StopOnDenyHook`. |
| `mcp_tool_adapter` | `McpToolAdapter`, `discover_tools`, `discover_tools_http` | MCP server tool discovery and registration. |

**Hook Execution Order**:
```
PreToolUse hooks → [PermissionHook → LoggingHook → CustomHooks]
    │
    ▼
Tool Execution
    │
    ▼
PostToolUse hooks → [AnalyticsHook → MemoryHook → CustomHooks]
```

#### 3.1.8 Memory & Knowledge

| Module | Key Types | Description |
|--------|-----------|-------------|
| `memory` | `MemoryStore`, `MemoryEntry`, `MemoryCategory`, `AutoDreamService`, `MemoryType`, `SessionMemoryConfig` | Persistent key-value memory store. Categories: project, user, session. Auto-dream service for automatic extraction. |
| `extract_memories` | `MemoryExtractor`, `ExtractionConfig`, `ExtractionResult`, `ExtractionCategory`, `ExtractedMemory` | Pipeline for extracting structured memories from conversation content. Categories: decision, pattern, preference, fact. |
| `auto_dream_consolidation` | `ConsolidationLock`, `ConsolidationGuard`, `ConsolidationPrompt`, `should_consolidate()` | Memory deduplication and consolidation. Prevents concurrent consolidation. Configurable thresholds. |
| `enhanced_suggestions` | `ContextSuggestionEngine`, `ContextualSuggestion`, `SuggestionTrigger`, `SuggestionContext` | Context-aware tool and file suggestions based on editing patterns, file types, and project structure. |

#### 3.1.9 MCP Integration

| Module | Key Types | Description |
|--------|-----------|-------------|
| `mcp_advanced` | `McpChannelManager`, `McpServerRegistry`, `ElicitationHandler`, `McpServerConfig`, `TransportType` | Advanced MCP server management. Dynamic tool registration from MCP servers. Channel lifecycle management. |
| `mcp_server_approval` | `McpApprovalManager`, `McpApprovalPolicy`, `McpServerApprovalRequest`, `ApprovalDecision`, `RiskAssessment` | User approval workflow for MCP server connections. Risk assessment based on server capabilities. |

#### 3.1.10 Observability

| Module | Key Types | Description |
|--------|-----------|-------------|
| `diagnostics` | `DiagnosticTracker`, `DiagnosticEvent`, `DiagnosticLevel`, `DiagnosticCategory`, `ErrorPattern`, `DiagnosticSummary` | Error tracking and pattern analysis. Auto-detects recurring error patterns. |
| `analytics` | `AnalyticsStore`, `AnalyticsEvent`, `AnalyticsEventType`, `ToolStats`, `SessionStats`, `DailyStats` | Usage analytics: tool call frequency, session duration, cost tracking. |
| `internal_logging` | `InternalLogEntry`, `InternalLogLevel`, `InternalLogger` | Structured internal logging with configurable verbosity. |
| `billing` | `BillingManager`, `BillingPeriod`, `UsageRecord`, `ModelUsageSummary`, `BudgetAlert`, `DailyUsage` | Per-model usage tracking. Budget alerts with configurable thresholds. Periodic billing reports. |
| `ai_limits` | `AiLimitType`, `AiUsageRecord`, `AiLimitsTracker`, `LimitStatus` | AI provider usage limits tracking. Request count, token count, rate limits. |

#### 3.1.11 Background Services

| Module | Key Types | Description |
|--------|-----------|-------------|
| `housekeeping` | `Housekeeper`, `HousekeepingTask`, `HousekeepingConfig`, `TempFileCleanupTask`, `CacheRefreshTask`, `OldSessionPruneTask`, `LogRotationTask` | Periodic background maintenance. 4 built-in tasks: temp file cleanup, cache refresh, old session pruning, log rotation. |
| `activity_manager` | `ActivityManager`, `Activity`, `ActivityStatus` | Long-running task activity tracking with progress reporting. Status transitions: Pending → Running → Paused → Completed/Failed. |
| `git_operation_tracking` | `GitOperation`, `GitOperationTracker` | Automatic tracking of all git operations for audit trail. |

#### 3.1.12 Remote & Auth

| Module | Key Types | Description |
|--------|-----------|-------------|
| `oauth` | `OAuthService`, `OAuthClient`, `OAuthToken`, `TokenEncryption` | OAuth 2.0 PKCE flow implementation. Token encryption at rest. Automatic token refresh. |
| `bridge_service` | `BridgeService`, `BridgeSession`, `BridgeConfig`, `BridgeStatus`, `SessionMessage`, `MessageDirection` | Remote session bridging. Session sharing across devices. Message synchronization. |
| `api_services` | `ApiManager`, `UsageTracker`, `ApiRequest`, `ApiResponse`, `UsageStats`, `RateLimitInfo` | API endpoint management. Usage aggregation across providers. |
| `updater` | `AutoUpdater`, `UpdateStatus`, `UpdaterConfig`, `ReleaseInfo` | GitHub Releases-based update checking. Configurable check interval. |

#### 3.1.13 Utility Modules

| Module | Description |
|--------|-------------|
| `token_estimation` | Token count estimation for context window management |
| `away_summary` | Session summary generation for away-from-keyboard detection |
| `tool_use_summary` | Human-readable summaries of tool call chains |
| `prevent_sleep` | System sleep prevention during long operations |
| `vcr` | VCR (Video Cassette Recorder) for recording/replaying API interactions in tests |
| `rate_limit_messages` | User-friendly rate limit violation messages |
| `voice_mode` | Voice input/output via system speech APIs, keyword spotting |
| `magic_docs` | Automatic documentation generation from source code analysis |
| `doctor` | Environment health checks: API connectivity, tool availability, config validation |
| `credential_manager` | Secure credential CRUD, portable export/import (encrypted bundles) |
| `tips` | Contextual tip display based on user activity patterns |
| `notifier` | Multi-channel notification system: log, file, callback |

---

### 3.2 shannon-tools

**Path**: `crates/shannon-tools/`
**Lines**: 25,338
**Modules**: 28
**Test Files**: 20

Concrete implementations of the `Tool` trait for all operations available to the AI assistant.

#### 3.2.1 File Operations (`file/`)

| Module | Tool | Description |
|--------|------|-------------|
| `read.rs` | `ReadTool` | Read file contents with line offset/limit, encoding detection, binary detection |
| `write.rs` | `WriteTool` | Create or overwrite files. Validates file size limits. |
| `edit.rs` | `EditTool` | String or regex-based find-and-replace within files. Supports unique match verification. |
| `glob.rs` | `GlobTool` | File pattern matching using glob syntax (`**/*.rs`, `src/*.ts`) |
| `sandbox.rs` | `PathSandbox` | Path validation against allowed/denied lists. Canonical path resolution. |
| `sandbox_adapter.rs` | `SandboxAdapter`, `PathSandboxAdapter` | Extended sandbox with read/write/execute/network validation. Dynamic rule management. File size limits. |
| `history.rs` | `FileHistoryManager` | File modification history tracking. Snapshot creation, diff viewing, rollback support. |
| `diff_renderer.rs` | `DiffRenderer` | Terminal-colored unified diff rendering. Support for +/- line highlighting, hunk headers. |

#### 3.2.2 System Operations (`system.rs`)

| Tool | Description |
|------|-------------|
| `BashTool` | Execute shell commands with safety checks. Validates against denied commands list. Captures stdout/stderr. Timeout support. |
| `SleepTool` | Asynchronous sleep (non-blocking). Supports interruptible sleep. |
| `PowerShellTool` | PowerShell command execution (Windows-native). |
| `REPLTool` | Batch command execution within a single tool call. |

#### 3.2.3 Git Integration (`git.rs`)

| Tool | Description |
|------|-------------|
| `GitBranchTool` | Branch listing, creation, switching, deletion. |
| `GitDiffTool` | Diff generation with staged/unstaged selection. |
| `GitLogTool` | Commit log with formatting options. |
| `GitStashTool` | Stash create, list, apply, drop. |
| `GitSafetyTool` | Pre-flight checks: protected branch warnings, force-push detection. |

#### 3.2.4 Web Operations (`web.rs`)

| Tool | Description |
|------|-------------|
| `WebFetchTool` | HTTP URL fetching with content extraction. Supports HTML-to-markdown conversion. Timeout and size limits. |
| `WebSearchTool` | Web search via API. Result ranking, snippet extraction. |

#### 3.2.5 Agent & Team Operations

| Module | Tools | Description |
|--------|-------|-------------|
| `agent.rs` | `AgentTool` | Sub-agent spawning with configuration. Isolated context. |
| `task.rs` | `TaskCreateTool`, `TaskListTool`, `TaskUpdateTool`, `TaskGetTool`, `TodoWriteTool` | Hierarchical task management with status tracking. |
| `messaging.rs` | `SendMessageTool` | Typed inter-agent messaging with priority levels. |

#### 3.2.6 Notebook (`notebook.rs`)

| Tool | Description |
|------|-------------|
| `NotebookEditTool` | Jupyter notebook (.ipynb) cell editing: add, replace, delete, reorder cells. |

#### 3.2.7 Worktree (`worktree.rs`)

| Tool | Description |
|------|-------------|
| `WorktreeTool` | Git worktree creation and management. Isolated development branches. |

#### 3.2.8 LSP Integration (`lsp.rs`, `lsp_diagnostics.rs`)

| Tool | Description |
|------|-------------|
| `GoToDefinitionTool` | Navigate to symbol definition via LSP. |
| `FindReferencesTool` | Find all references to a symbol. |
| `HoverTool` | Get hover information (type docs, signatures). |
| `DocumentSymbolTool` | List all symbols in a document. |
| (lsp_diagnostics) | `DiagnosticRegistry` | Collects and aggregates diagnostics from language servers. |

#### 3.2.9 Plan Mode (`plan_mode.rs`)

| Tool | Description |
|------|-------------|
| `EnterPlanModeTool` | Switch to read-only planning mode. Disables file edits. |
| `ExitPlanModeTool` | Exit planning mode and request approval. |

#### 3.2.10 Interactive (`ask_user.rs`)

| Tool | Description |
|------|-------------|
| `AskUserQuestionTool` | Interactive user prompts with multiple-choice or free-text input. Terminal UI for option selection. |

#### 3.2.11 Other Tools

| Module | Tool | Description |
|--------|------|-------------|
| `grep.rs` | `GrepTool` | Regex-based content search across files. |
| `skill.rs` | `SkillTool` | Skill invocation by name with arguments. |
| `cron.rs` | `CronTool` | Scheduled task management (create, delete, list). Cron expression parsing. |
| `tool_search.rs` | `ToolSearchTool` | Tool discovery by name, category, or keyword. |
| `brief.rs` | `BriefTool` | Conversation/message summarization. |
| `config.rs` | `ConfigTool` | Runtime configuration management (get, set, reset). |
| `synthetic_output.rs` | `StructuredOutputTool` | AI-generated structured JSON data with schema validation. |
| `remote_trigger.rs` | `RemoteTriggerTool` | Remote event triggering via HTTP endpoints. |
| `task_output.rs` | `TaskOutputTool` | Retrieve output from background tasks. |
| `task_stop.rs` | `TaskStopTool` | Cancel running tasks. |
| `team_delete.rs` | `TeamDeleteTool` | Clean up team resources. |
| `mcp_tools.rs` | `ListMcpResourcesTool`, `ReadMcpResourceTool` | MCP resource access. |
| `mcp_auth.rs` | `McpAuthTool` | OAuth authentication for MCP servers. |

---

### 3.3 shannon-agents

**Path**: `crates/shannon-agents/`
**Lines**: 6,580
**Test Files**: 4

Multi-agent coordination system for parallel task execution.

#### 3.3.1 Core Components

| Module | Key Types | Description |
|--------|-----------|-------------|
| `coordinator` | `AgentCoordinator`, `CoordinatorConfig`, `AssignmentStrategy`, `CoordinatorEvent` | Top-level orchestrator. Creates teams, assigns tasks, monitors progress. Strategies: round-robin, skill-based, least-loaded. |
| `teammate` | `Teammate`, `TeammateConfig`, `TeammateStatus`, `TeammateState` | Individual agent lifecycle management. Status: `Idle → Busy → Waiting → Completed/Failed`. |
| `task_board` | `TaskBoard`, `TaskAssignment`, `TaskBoardEvent`, `TaskBoardSummary` | Shared task state across team members. Dependency tracking. |
| `worktree` | `WorktreeManager`, `WorktreeConfig`, `WorktreeSession`, `WorktreeStatus` | Git worktree-based isolation for parallel development. Enter/exit workflows. |
| `message` | `AgentMessage`, `MessagePriority`, `MessageType`, `MessageContent`, `ProtocolMessage` | Typed inter-agent messaging. Priorities: Critical, High, Normal, Low. |
| `task` | `AgentTask`, `TaskStatus`, `TaskDependency`, `TaskPriority`, `DependencyType` | Dependency-aware task model. Supports `Blocking` and `Optional` dependencies. |
| `sub_agent` | `SubAgent`, `SubAgentRegistry`, `AgentSpawnTool`, `SendMessageTool`, `TeamCreateTool` | Sub-agent spawning with context inheritance. Agent registry for tracking. |
| `multi_agent` | `MultiAgentConfig`, `MultiAgentSpawner`, `MultiAgentResult`, `AgentResult` | Parallel agent dispatch with configurable concurrency limits. Result aggregation. |
| `summary` | `AgentExecutionSummary`, `SummaryStatus`, `SummaryGenerator`, `SuccessMetrics` | Execution metrics: duration, tokens used, tools called, success rate. |

**Agent Lifecycle**:
```
Spawn (AgentConfig)
  │
  ▼
Initialize (context + tools)
  │
  ▼
┌────────────────────────────┐
│  Receive Task Assignment   │
│  │                       │
│  ├─ Analyze & Plan        │
│  ├─ Execute (tool calls)  │
  ├─ Report Progress        │
  │                       │
  ├─ Await Next Task       │◄────┐
  └────────────────────────────┘     │
                                     │
  ▼                              │
Shutdown
```

---

### 3.4 shannon-ui

**Path**: `crates/shannon-ui/`
**Lines**: 5,981
**Test Files**: 8

Terminal user interface built with ratatui and crossterm.

#### 3.4.1 Core Components

| Module | Key Types | Description |
|--------|-----------|-------------|
| `repl` | `Repl` | Main interactive REPL loop. Input handling, command dispatch, output rendering. Multi-line input support. |
| `repl_enhancement` | `TurnDiff`, `ReplHistory`, `InputBuffer`, `ReplRenderer` | Enhanced REPL features: turn-level diff tracking, input history search, buffer management. |
| `render` | Markdown renderer, syntax highlighter | Terminal markdown rendering with syntax highlighting. Diff visualization. |
| `events` | UI event system | Event-driven UI updates. Keyboard, mouse, resize events. |
| `vim` | `VimHandler` | Vim emulation: normal mode, insert mode, visual mode. Key mapping. |
| `widgets/` | `Dialog`, `Progress`, `Select` | Reusable TUI widgets: confirmation dialogs, progress bars, selection menus. |

---

### 3.5 shannon-mcp

**Path**: `crates/shannon-mcp/`
**Lines**: 2,269
**Test Files**: 4

Complete implementation of the Model Context Protocol (MCP).

#### 3.5.1 Protocol Layer

| Module | Key Types | Description |
|--------|-----------|-------------|
| `protocol` | `JsonRpcMessage`, `JsonRpcRequest`, `JsonRpcResponse`, `Tool`, `Resource`, `ResourceTemplate`, `Prompt`, `Completion`, `McpCapabilities` | Full MCP protocol types. JSON-RPC 2.0 message format. Protocol version: `2024-11-05`. |
| `transport` | `Transport` (trait), `StdioTransport`, `SseTransport`, `HttpTransport`, `WebSocketTransport` | Pluggable transport layer. Each transport handles connection lifecycle and message framing. |

**Supported Transport Types**:

| Transport | URI Scheme | Use Case |
|-----------|------------|----------|
| Stdio | — | Local process communication |
| SSE | `http://` / `https://` | Remote server, server-sent events |
| HTTP | `http://` / `https://` | Request-response (streamable HTTP) |
| WebSocket | `ws://` / `wss://` | Bidirectional real-time |

#### 3.5.2 Client Layer

| Module | Key Types | Description |
|--------|-----------|-------------|
| `client` | `McpClient`, `McpClientError` | MCP client with connection management. Initialize, list tools, call tools, manage resources. |
| `auth` | `AuthProvider`, `OAuth2Provider`, `ApiKeyProvider` | Authentication for MCP servers. OAuth 2.0 PKCE support. |
| `resources` | `ResourceDescriptor`, `McpResourceManager`, `McpResourceClient` | Resource listing, reading, and subscription management. |

---

### 3.6 shannon-commands

**Path**: `crates/shannon-commands/`
**Lines**: 3,101
**Test Files**: 7

Slash command system for extending Shannon with custom commands.

#### 3.6.1 Architecture

| Module | Key Types | Description |
|--------|-----------|-------------|
| `registry` | `CommandRegistry` | Central command registration and lookup by name. |
| `parser` | `CommandParser`, `ParsedCommand` | Argument parsing and validation for commands. |
| `executor` | `CommandExecutor` | Command execution with context injection. |
| `command` | `Command`, `PromptCommand`, `LocalCommand`, `LocalJSXCommand`, `CommandResult` | Command trait with three execution modes. |

**Command Types**:

| Type | Description |
|------|-------------|
| `PromptCommand` | Generates a prompt for AI processing. User-facing. |
| `LocalCommand` | Executed locally without AI involvement. System operations. |
| `LocalJSXCommand` | Commands with rich TUI components. Interactive UI. |

#### 3.6.2 Built-in Commands

| Command | Description |
|---------|-------------|
| `/commit` | Generate a git commit message and create commit. |
| `/diff` | Show git diff with formatting options. |
| `/help` | Display help information. |
| `/pdf` | Generate PDF from project documentation. |
| `/review-pr` | Review a pull request with AI analysis. |
| `/status` | Show current session and system status. |

#### 3.6.3 Custom Commands (File-Based)

Custom slash commands are loaded from `.md` files in well-known directories. This is compatible with Claude Code's custom command format.

**Discovery Paths** (later entries override earlier):

| Path | Scope |
|------|-------|
| `~/.claude/commands/*.md` | User-level (Claude Code compatible) |
| `~/.shannon/commands/*.md` | User-level (Shannon-specific) |
| `.claude/commands/*.md` | Project-level (Claude Code compatible) |
| `.shannon/commands/*.md` | Project-level (Shannon-specific) |

**Name Resolution**:

| File | Command Name |
|------|-------------|
| `.claude/commands/review.md` | `/review` |
| `.claude/commands/project/foo.md` | `/project:foo` |
| `.claude/commands/a/b/c.md` | `/a:b:c` |

Subdirectories use `:` as separator. Directories starting with `.` are skipped.

**File Format** (Markdown with optional YAML frontmatter):

```markdown
---
description: Review code for bugs
arguments: file
model: claude-sonnet-4-6
allowed-tools: Bash, Read
agent: reviewer
---
Review the following code for potential bugs and suggest fixes:

$ARGUMENTS
```

**Frontmatter Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `description` | string | Command description shown in `/commands` list |
| `arguments` | string | Comma-separated argument names (e.g. `file, pattern`). Maps to `$ARGUMENTS[0]`, `$ARGUMENTS[1]`, etc. Alias: `args` |
| `model` | string | Model override for this command |
| `allowed-tools` | string | Comma-separated list of permitted tools. Alias: `allowed_tools` |
| `agent` | string | Agent type when executed as sub-agent |

**Template Placeholders**:

| Placeholder | Replaced With |
|-------------|---------------|
| `$ARGUMENTS` | Full user-provided text |
| `$ARGUMENTS[N]` | Nth word of user input (0-indexed) |
| `{args}` | Same as `$ARGUMENTS` |
| `{args[N]}` | Same as `$ARGUMENTS[N]` |
| `$DIR` | Current working directory |
| `$DATE` | Current date (YYYY-MM-DD) |
| `$TIME` | Current time (HH:MM:SS) |

**Management Commands** (`/commands`):

| Subcommand | Description |
|------------|-------------|
| `/commands` | List all custom commands with descriptions |
| `/commands create <name>` | Create a new command file (supports `project:foo` for subdirs) |
| `/commands edit <name>` | Open command file in `$EDITOR` |
| `/commands delete <name>` | Delete a command file and unregister it |
| `/commands reload` | Re-scan directories and re-register all commands |

- YAML frontmatter (`---` blocks) is stripped before use
- `$ARGUMENTS` or `{args}` placeholders are replaced with user-provided text
- Project-level commands override user-level commands with the same name

#### 3.6.4 MCP Prompt Commands

MCP server prompts are automatically registered as slash commands using the naming convention `/mcp__{server}__{prompt}`. These are discovered during startup and re-discovered on `/mcp reload`.

| Feature | Behavior |
|---------|----------|
| Discovery | `prompts/list` MCP protocol method |
| Registration | Automatic on startup and `/mcp reload` |
| Arguments | Mapped from MCP prompt `arguments` schema |
| Execution | Uses `get_mcp_prompt` tool to fetch and render prompt content |

---

### 3.7 shannon-skills

**Path**: `crates/shannon-skills/`
**Lines**: 2,286
**Test Files**: 8

Extensible skill framework for defining reusable prompts and commands.

#### 3.7.1 Skill Lifecycle

| Module | Key Types | Description |
|--------|-----------|-------------|
| `definition` | `Skill`, `SkillContext`, `SkillId`, `SkillPermissions`, `SkillSource`, `SkillResult` | Core skill definition with metadata, content, and permissions. |
| `frontmatter` | `ParsedSkill` | YAML frontmatter parsing: name, description, trigger patterns, permissions. |
| `loader` | Skill loader functions | Load skills from filesystem directories. Supports nested directories. |
| `registry` | `SkillRegistry` | Central skill management: register, lookup, list, search. |
| `executor` | `SkillExecutor` | Skill execution with argument substitution and context injection. |
| `bundled` | `BundledSkills`, `BundledSkillBuilder` | Built-in skills that ship with the application. |
| `discovery` | `SkillDiscovery` | Runtime skill discovery: scan paths, detect new skills. |

**Skill Definition Format** (Markdown + YAML):
```markdown
---
name: my-skill
description: Description of what this skill does
triggers:
  - "keyword pattern"
  - "another pattern"
permissions:
  - read
  - write
---

Skill content with {{argument}} placeholders...
```

---

### 3.8 shannon-types

**Path**: `crates/shannon-types/`
**Lines**: 37

Shared type definitions used across crates. Minimal crate to avoid circular dependencies.

---

### 3.9 shannon-cli

**Path**: `crates/shannon-cli/`
**Lines**: 68

CLI entry point using `clap`.

#### 3.9.1 Commands

| Command | Description |
|---------|-------------|
| `shannon repl [OPTIONS]` | Start the interactive REPL with optional configuration. |
| `shannon version [-v]` | Display version information. `-v` for verbose output (Rust version, features). |
| `shannon config [setting]` | Manage configuration. View all settings or query a specific setting. |
| `shannon -h/--help` | Display help information. |
| `shannon -V/--version` | Display version (short form). |

#### 3.9.2 REPL Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--file` | `-f` | Project file to load on startup | None |
| `--env` | `-e` | Set environment variable (KEY=VALUE, repeatable) | None |
| `--model` | `-m` | LLM model to use (e.g., claude-sonnet-4, gpt-4o) | From config |
| `--provider` | `-p` | LLM provider (anthropic, openai, ollama) | From config |
| `--max-tokens` | | Maximum tokens for response | 8192 |
| `--temperature` | | Sampling temperature (0.0-1.0) | 0.7 |
| `--timeout` | | Request timeout in seconds | 120 |
| `--debug` | `-d` | Enable debug logging | false |
| `--cwd` | | Working directory for the session | Current directory |

#### 3.9.3 Environment Variables

All REPL options can be set via environment variables:

| Variable | Description |
|----------|-------------|
| `SHANNON_MODEL` | Default model to use |
| `SHANNON_PROVIDER` | Default provider (anthropic/openai/ollama) |
| `SHANNON_MAX_TOKENS` | Default max tokens |
| `SHANNON_TEMPERATURE` | Default temperature |
| `SHANNON_TIMEOUT` | Default timeout (seconds) |
| `SHANNON_DEBUG` | Enable debug mode (1/true/yes) |
| `SHANNON_API_KEY` | API key for the provider |
| `SHANNON_BASE_URL` | Custom base URL for API |

**Priority Order**: CLI flags > `-e` env vars > `.env` files > `settings.json` > built-in defaults

---

## 4. Core Interfaces

### 4.1 Tool Trait

The `Tool` trait is the fundamental abstraction for all operations available to the AI assistant:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique tool identifier used in API calls
    fn name(&self) -> &str;

    /// Human-readable description for tool selection
    fn description(&self) -> &str;

    /// JSON Schema describing input parameters
    fn input_schema(&self) -> Value;

    /// Execute the tool with validated input
    async fn execute(&self, input: Value) -> ToolResult<ToolOutput>;

    /// Whether this tool requires API authentication
    fn requires_auth(&self) -> bool { false }

    /// Tool category for grouping
    fn category(&self) -> &str { "general" }
}
```

### 4.2 Permission Flow

```
                    ┌──────────────┐
                    │ Tool Call    │
                    │ Request      │
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │ Classifier  │
                    │             │
                    │ Pattern    │
                    │ Match?     │
                    │ Denied?    │
                    └──────┬───────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
              ▼            ▼            ▼
         Auto-Allow    Prompt User    Block
              │            │            │
              ▼            ▼            │
         ┌──────┐   ┌──────────┐      ┌────────┐
         │Exec  │   │Permission │      │ Denied │
         └──────┘   │Choice    │      └────────┘
                     └──────────┘
```

### 4.3 Query Processing Loop

```
while !done {
    response = await claude_client.send_stream(messages).collect();

    for block in response.content_blocks {
        match block {
            ContentBlock::Text(text) => {
                emit_to_user(text);
                messages.push(assistant_message(text));
            }
            ContentBlock::ToolUse(tool_use) => {
                if permission_manager.check(&tool_use).allowed {
                    result = await tool_registry.execute(tool_use).await;
                    messages.push(tool_result_message(result));
                } else {
                    emit_permission_prompt(tool_use);
                    choice = await user_choice();
                    if choice == Deny { skip; }
                }
            }
            ContentBlock::EndTurn => done = true;
        }
    }

    if context_window_near_limit() {
        compress_context(&mut messages);
    }
}
```

### 4.4 MCP Protocol Flow

```
Client                          Server
  │                               │
  ├── initialize ──────────────────→│
  │   (capabilities exchange)     │
  │◄───────────────────────────┤
  │   (server info)              │
  │                               │
  ├── list_tools ────────────────→│
  │◄───────────────────────────┤
  │   (tool definitions)          │
  │                               │
  ├── call_tool ─────────────────→│
  │   (tool name + args)         │
  │◄───────────────────────────┤
  │   (tool result)              │
  │                               │
  ├── list_resources ──────────→│
  │◄───────────────────────────┤
  │   (resource list)            │
  │                               │
  ├── read_resource ────────────→│
  │◄───────────────────────────┤
  │   (resource content)          │
  │                               │
  ├── subscribe ────────────────→│
  │   (resource URI + method)    │
  │◄───────────────────────────┤
  │   (subscription result)       │
  │                               │
  └── disconnect ───────────────→│
```

---

## 5. Configuration

### 5.1 Configuration Hierarchy

```
1. Built-in defaults (compiled into binary)
2. User config: ~/.shannon/settings.json
3. Project config: .shannon/settings.json
4. .env files: .env → .env.local → .env.production
5. Environment variables: SHANNON_* → ANTHROPIC_* → OPENAI_*
6. CLI flags: -e KEY=VALUE (highest priority)
```

### 5.2 Configuration File Format (TOML + .env)

**TOML** (`settings.json`):
```toml
[general]
model = "claude-sonnet-4-20250514"
max_tokens = 8192
temperature = 0.7

[permissions]
auto_approve_safe = true
deny_patterns = ["rm -rf /", "mkfs"]
allowed_paths = ["/home/user/projects"]

[session]
persist_directory = "~/.shannon/sessions"
auto_save_interval = "30s"
max_context_messages = 100
```

**.env** ( dotenvy format):
```env
# LLM Provider Configuration
SHANNON_MODEL=claude-sonnet-4-20250514
SHANNON_API_KEY=sk-...
SHANNON_BASE_URL=https://api.anthropic.com
SHANNON_MAX_TOKENS=8192
SHANNON_TEMPERATURE=0.7
SHANNON_PERMISSIONS_MODE=ask
```

**CLI Override**:
```bash
shannon repl -e SHANNON_MODEL=gpt-4o -e SHANNON_MAX_TOKENS=8192
```

### 5.3 Project Memory Files (SHANNON.md)

Priority-based loading with 4 layers (later overrides earlier):
- Global Shannon: `~/.shannon/SHANNON.md`
- Global Compatible: `~/.shannon/CLAUDE.md`
- Project Shannon: `./SHANNON.md`
- Project Compatible: `./CLAUDE.md`

Both `SHANNON.md` and `CLAUDE.md` are supported for cross-tool compatibility.

---

## 6. Data Storage

### 6.1 Directory Structure

```
~/.shannon/
├── sessions/          # Session persistence (JSON)
│   ├── {uuid}.json
│   └── ...
├── memory/            # Long-term memory (JSON)
│   ├── project/
│   ├── user/
│   └── session/
├── transcripts/      # Conversation transcripts (JSONL)
│   ├── {session-id}.jsonl
│   └── ...
├── mcp-state/        # MCP server state persistence
├── credentials/       # Encrypted credentials
│   └── *.enc
├── cache/             # Cached data
│   ├── completions/
│   └── diagnostics/
├── config.toml        # User configuration
└── history/           # Command history
```

### 6.2 Session Persistence Format

Each session is stored as a single JSON file:

```json
{
  "session_id": "uuid-v4",
  "created_at": "2026-04-04T00:00:00Z",
  "updated_at": "2026-04-04T00:00:00Z",
  "metadata": {
    "project_path": "/path/to/project",
    "model": "claude-sonnet-4-20250514",
    "total_cost_usd": 0.001234
  },
  "data": {
    "messages": [...],
    "permissions": {...},
    "memory_entries": [...]
  }
}
```

### 6.3 Transcript Format (JSONL)

```jsonl
{"role": "user", "content": "Fix the bug in auth.rs", "timestamp": "..."}
{"role": "assistant", "content": "I'll fix the bug...", "timestamp": "..."}
{"role": "tool", "name": "Read", "input": {...}, "output": {...}, "timestamp": "..."}
```

---

## 7. Security Model

### 7.1 Permission Levels

| Level | Description | Behavior |
|-------|-------------|----------|
| `Safe` | Read-only operations | Auto-approved |
| `Low` | Non-destructive writes | Auto-approved (configurable) |
| `Medium` | Network requests | Prompt user |
| `High` | File deletion, system changes | Prompt with warning |
| `Critical` | Destructive system operations | Prompt with danger alert |

### 7.2 Path Sandboxing

```
Allowed Paths:     /home/user/projects/*     (configurable)
Denied Paths:     /etc, /boot, /dev, /proc, /sys
Read-Only Paths:  /usr/share/*              (configurable)
Max File Size:     100 MB                  (configurable)
```

### 7.3 Command Safety

**Denied Commands** (built-in):
- `rm -rf /`
- `mkfs`
- `dd if=/dev/zero`
- `chmod 777`

**Pattern-Based Detection** (10+ patterns):
- Recursive deletion with force flag
- Disk formatting operations
- System binary modification
- Privilege escalation attempts
- Kernel module loading

### 7.4 Secret Scanning

The `SecretScanner` detects sensitive data in shared memory:
- API keys (AWS, GCP, GitHub tokens)
- Private keys (SSH, PGP, TLS)
- Database connection strings
- OAuth tokens
- Custom patterns via configurable rules

### 7.5 MCP Server Approval

Before connecting to an MCP server:
1. `RiskAssessment` evaluates server capabilities
2. Server transport type checked (local vs remote)
3. Requested permissions reviewed
4. User approval prompt displayed
5. Decision cached for trusted servers

### 7.6 Credential Encryption

- AES-256-GCM encryption for stored credentials
- Key derivation via PBKDF2
- Portable credential export (encrypted bundles)
- Configurable master password

---

## 8. Error Handling Strategy

### 8.1 Error Type Hierarchy

Each module defines its own error enum using `thiserror`:

```rust
#[derive(thiserror::Error, Debug)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    // ...
}
```

### 8.2 Error Propagation

```
Tool execution error (ToolError)
    → Wrapped in QueryError::ToolError
    → Caught by QueryEngine
    → Reported to user via ToolOutput { is_error: true }
    → Logged by DiagnosticTracker
    → Tracked by AnalyticsStore
```

### 8.3 Recovery Strategies

| Scenario | Strategy |
|----------|----------|
| Tool execution failure | Retry with exponential backoff (configurable max retries) |
| API rate limit | Automatic backoff, user notification |
| Network timeout | Retry with increased timeout |
| Invalid tool input | Return error description, suggest correction |
| Permission denied | Log event, suggest alternative |
| Context overflow | Auto-compact, notify user |

### 8.4 VCR Testing

The `Vcr` module enables deterministic API testing:

```rust
// Recording mode: captures real API interactions
let vcr = Vcr::new("fixtures/test_session.jsonl");
vcr.record_mode(true);
let client = ClaudeClient::with_vcr(vcr);
// ... run test ...

// Playback mode: replays recorded interactions
vcr.record_mode(false);
let client = ClaudeClient::with_vcr(vcr);
// ... deterministic test ...
```

---

## 9. Project Statistics

### 9.1 Code Metrics

| Metric | Value |
|--------|-------|
| Total Lines of Code | ~94,000 |
| Source Files (.rs) | ~120 |
| Test Functions | 3,180 |
| Workspace Crates | 9 |
| Public Modules | 93 |
| Public Structs | ~200 |
| Public Traits | ~15 |

### 9.2 Per-Crate Breakdown

| Crate | Lines | Key Modules |
|-------|-------|-------------|
| shannon-core | ~49,000 | 50 modules (query, tools, api, permissions, state, mcp, etc.) |
| shannon-tools | 25,338 | 28 tool modules |
| shannon-agents | 6,580 | coordinator, teammate, task_board |
| shannon-ui | 5,981 | repl, vim, render, widgets |
| shannon-commands | 3,101 | 11 builtin commands |
| shannon-mcp | 2,269 | protocol, transport, client, auth |
| shannon-skills | 2,286 | definition, loader, executor |
| shannon-types | 37 | Shared type definitions |
| shannon-cli | 83 | CLI entry point |

### 9.3 Build Configuration

```toml
[profile.release]
opt-level = 3       # Maximum optimization
lto = true          # Link-time optimization
codegen-units = 1   # Single codegen unit for better optimization
strip = true        # Strip debug symbols
panic = "abort"     # Abort on panic for smaller binary
```

**Release Binary Size**: ~3.4 MB (stripped, x86-64 Linux ELF)

### 9.4 Dependencies

| Category | Dependencies |
|----------|------------|
| Async Runtime | tokio, tokio-util, async-trait, futures, tokio-stream |
| Serialization | serde, serde_json, serde_yaml |
| CLI | clap |
| Terminal UI | ratatui, crossterm |
| HTTP | reqwest |
| Error Handling | anyhow, thiserror |
| Logging | tracing, tracing-subscriber |
| Utilities | uuid, chrono, dirs, dotenvy |
| State | dashmap |
| Pattern Matching | regex |
| Testing | tokio-test, criterion, tempfile |

### 9.5 Platform Support

| Platform | Status | Notes |
|----------|--------|-------|
| Linux (x86_64) | Supported | Primary target |
| macOS (aarch64/x86_64) | Supported | Full feature parity |
| Windows (x86_64) | Supported | PowerShell tool available |

---

## Appendix A: Tool Inventory

Complete list of all tools available to the AI assistant:

| # | Tool Name | Category | Module |
|---|-----------|----------|--------|
| 1 | Read | file | file/read.rs |
| 2 | Write | file | file/write.rs |
| 3 | Edit | file | file/edit.rs |
| 4 | Glob | file | file/glob.rs |
| 5 | Bash | system | system.rs |
| 6 | Sleep | system | system.rs |
| 7 | PowerShell | system | system.rs |
| 8 | REPL | system | repl_tool.rs |
| 9 | GitBranch | git | git.rs |
| 10 | GitDiff | git | git.rs |
| 11 | GitLog | git | git.rs |
| 12 | GitStash | git | git.rs |
| 13 | GitSafety | git | git.rs |
| 14 | WebFetch | web | web.rs |
| 15 | WebSearch | web | web.rs |
| 16 | Agent | agent | agent.rs |
| 17 | TaskCreate | task | task.rs |
| 18 | TaskList | task | task.rs |
| 19 | TaskUpdate | task | task.rs |
| 20 | TaskGet | task | task.rs |
| 21 | TodoWrite | task | todo.rs |
| 22 | SendMessage | messaging | messaging.rs |
| 23 | NotebookEdit | notebook | notebook.rs |
| 24 | Worktree | worktree | worktree.rs |
| 25 | ListMcpResources | mcp | mcp_tools.rs |
| 26 | ReadMcpResource | mcp | mcp_tools.rs |
| 27 | McpAuth | mcp | mcp_auth.rs |
| 28 | GoToDefinition | lsp | lsp.rs |
| 29 | FindReferences | lsp | lsp.rs |
| 30 | Hover | lsp | lsp.rs |
| 31 | DocumentSymbol | lsp | lsp.rs |
| 32 | Grep | search | grep.rs |
| 33 | Skill | skill | skill.rs |
| 34 | CronCreate | cron | cron.rs |
| 35 | CronDelete | cron | cron.rs |
| 36 | CronList | cron | cron.rs |
| 37 | AskUserQuestion | interactive | ask_user.rs |
| 38 | EnterPlanMode | plan | plan_mode.rs |
| 39 | ExitPlanMode | plan | plan_mode.rs |
| 40 | ToolSearch | discovery | tool_search.rs |
| 41 | Brief | utility | brief.rs |
| 42 | Config | config | config.rs |
| 43 | StructuredOutput | output | synthetic_output.rs |
| 44 | RemoteTrigger | remote | remote_trigger.rs |
| 45 | TaskOutput | team | task_output.rs |
| 46 | TaskStop | team | task_stop.rs |
| 47 | TeamDelete | team | team_delete.rs |
| 48 | AgentSpawn | team | agent.rs |

---

## Appendix B: Event Types

| Event | Trigger | Description |
|-------|--------|-------------|
| `PreToolUse` | Before tool execution | Permission check, input validation, logging |
| `PostToolUse` | After tool execution | Result capture, analytics, memory extraction |
| `PreQuery` | Before API call | Context compression, rate limit check |
| `PostQuery` | After API response | Usage tracking, cost recording |
| `SessionStart` | Session initialization | Load config, register tools, discover MCP servers |
| `SessionEnd` | Session termination | Persist state, flush analytics |
| `PermissionDenied` | Permission rejection | Audit logging, alternative suggestions |
| `ToolError` | Execution failure | Error pattern detection, retry scheduling |
| `ContextCompact` | Context compression | Compression strategy selection, summary generation |
