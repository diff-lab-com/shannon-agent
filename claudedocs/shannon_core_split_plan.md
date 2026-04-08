# Shannon-Core Split Plan

## Overview

The `shannon-core` crate has grown to 77+ modules, making it difficult to maintain and understand. This plan proposes splitting it into logical sub-crates based on functionality and dependencies.

## Current Structure Analysis

### Module Count: 77+ modules

Based on `crates/shannon-core/src/lib.rs`:
```
query_engine, tools, permissions, state, api, project_memory, settings,
hooks, plugins, updater, suggestions, memory, extract_memories, diagnostics,
analytics, notifier, tips, rate_limit, away_summary, tool_use_summary,
token_estimation, prevent_sleep, policy_limits, rate_limit_messages, ai_limits,
vcr, internal_logging, git_operation_tracking, voice_mode, magic_docs, oauth,
settings_sync, remote_settings, mcp_advanced, api_services, bridge_service,
session_history, compact, streaming_tool_executor, tool_execution, tool_hooks,
doctor, permission_classifier, team_memory_sync, auto_dream_consolidation,
mcp_server_approval, session_transcript, activity_manager, housekeeping,
credential_manager, billing, enhanced_suggestions
```

### Size Analysis
- `query_engine.rs`: ~3000+ lines (main orchestrator)
- `api.rs`: ~2000+ lines (LLM client, streaming)
- `tools.rs`: ~1000+ lines (tool registry)
- `memory.rs`: ~1500+ lines (memory store, auto-dream)
- Other modules: 200-800 lines each

## Proposed Split Structure

```
crates/
├── shannon-core/                    # Core foundation (remaining minimal core)
│   └── src/
│       ├── lib.rs                   # Re-exports from sub-crates
│       └── prelude.rs               # Common imports
│
├── shannon-core-base/               # Foundation types & traits
│   ├── src/
│   │   ├── lib.rs
│   │   ├── error.rs                 # Core error types
│   │   ├── state.rs                 # StateManager, SessionState
│   │   ├── settings.rs              # Settings, SettingsManager
│   │   ├── hooks.rs                 # HookManager, HookEvent
│   │   └── permissions.rs           # PermissionManager, Permission
│   └── Cargo.toml
│
├── shannon-core-api/                # LLM API & streaming
│   ├── src/
│   │   ├── lib.rs
│   │   ├── api.rs                   # LlmClient, providers, streaming
│   │   ├── api_services.rs          # ApiManager, UsageTracker
│   │   └── vcr.rs                   # Vcr (record/replay)
│   └── Cargo.toml                   # depends: shannon-core-base
│
├── shannon-core-tools/              # Tool management
│   ├── src/
│   │   ├── lib.rs
│   │   ├── tools.rs                 # ToolRegistry, Tool trait
│   │   ├── tool_hooks.rs            # ToolHookChain, PermissionToolHook
│   │   ├── tool_execution.rs        # ToolExecutionService, progress tracking
│   │   ├── streaming_tool_executor.rs
│   │   ├── tool_use_summary.rs      # Tool usage statistics
│   │   └── permission_classifier.rs # Dangerous pattern detection
│   └── Cargo.toml                   # depends: shannon-core-base
│
├── shannon-core-query/              # Query processing
│   ├── src/
│   │   ├── lib.rs
│   │   ├── query_engine.rs          # QueryEngine, QueryContext
│   │   └── compact.rs               # CompactEngine, message grouping
│   └── Cargo.toml                   # depends: shannon-core-api, shannon-core-tools
│
├── shannon-core-memory/             # Memory & persistence
│   ├── src/
│   │   ├── lib.rs
│   │   ├── memory.rs                # MemoryStore, AutoDreamService
│   │   ├── project_memory.rs        # ProjectMemoryManager
│   │   ├── extract_memories.rs      # MemoryExtractor
│   │   ├── auto_dream_consolidation.rs
│   │   ├── session_transcript.rs    # TranscriptStore
│   │   ├── session_history.rs       # SessionHistoryManager
│   │   └── team_memory_sync.rs      # TeamMemorySync, SecretScanner
│   └── Cargo.toml                   # depends: shannon-core-base
│
├── shannon-core-plugins/            # Plugin & MCP system
│   ├── src/
│   │   ├── lib.rs
│   │   ├── plugins.rs               # PluginManager, Plugin trait
│   │   ├── mcp_advanced.rs          # McpChannelManager
│   │   ├── mcp_server_approval.rs   # McpApprovalManager
│   │   └── bridge_service.rs        # BridgeService
│   └── Cargo.toml                   # depends: shannon-core-base
│
├── shannon-core-features/           # Feature modules
│   ├── src/
│   │   ├── lib.rs
│   │   ├── analytics.rs             # AnalyticsStore
│   │   ├── voice_mode.rs            # VoiceModeService
│   │   ├── magic_docs.rs            # MagicDocsService
│   │   ├── updater.rs               # AutoUpdater
│   │   ├── oauth.rs                 # OAuthService
│   │   ├── billing.rs               # Billing integration
│   │   └── credential_manager.rs    # CredentialManager
│   └── Cargo.toml                   # depends: shannon-core-base
│
├── shannon-core-maintenance/        # Background tasks & limits
│   ├── src/
│   │   ├── lib.rs
│   │   ├── housekeeping.rs          # Housekeeper, cleanup tasks
│   │   ├── activity_manager.rs      # ActivityManager
│   │   ├── rate_limit.rs            # RateLimiter
│   │   ├── rate_limit_messages.rs   # RateLimitMessageBuilder
│   │   ├── policy_limits.rs         # PolicyLimitsManager
│   │   ├── ai_limits.rs             # AiLimitsTracker
│   │   ├── away_summary.rs          # AwaySummaryService
│   │   └── prevent_sleep.rs         # PreventSleepService
│   └── Cargo.toml                   # depends: shannon-core-base
│
└── shannon-core-diagnostics/        # Diagnostics & notifications
    ├── src/
    │   ├── lib.rs
    │   ├── diagnostics.rs            # DiagnosticTracker
    │   ├── doctor.rs                 # Doctor service
    │   ├── internal_logging.rs       # InternalLogger
    │   ├── notifier.rs              # Notifier, handlers
    │   ├── tips.rs                  # TipManager
    │   ├── suggestions.rs            # SuggestionEngine
    │   ├── enhanced_suggestions.rs  # Enhanced suggestions
    │   ├── git_operation_tracking.rs
    │   ├── token_estimation.rs      # Token estimation
    │   ├── settings_sync.rs         # SettingsSyncService
    │   └── remote_settings.rs       # RemoteSettingsProvider
    └── Cargo.toml                   # depends: shannon-core-base
```

## Dependency Graph

```
                    ┌─────────────────────┐
                    │  shannon-core-base  │ (foundation)
                    └──────────┬──────────┘
                               │
        ┌──────────────────────┼──────────────────────┐
        │                      │                      │
        ▼                      ▼                      ▼
┌───────────────┐    ┌────────────────┐    ┌────────────────┐
│ shannon-core- │    │ shannon-core-  │    │ shannon-core-  │
│     api       │    │    tools       │    │   plugins      │
└───────┬───────┘    └────────┬───────┘    └────────┬───────┘
        │                     │                     │
        │    ┌────────────────┴─────────────────────┤
        │    │                                  │
        ▼    ▼                                  ▼
┌─────────────────┐                    ┌─────────────────┐
│ shannon-core-   │                    │ shannon-core-   │
│    query        │                    │    memory       │
└─────────────────┘                    └─────────────────┘

        ┌────────────────┐
        │ shannon-core-  │
        │  features      │
        │ maintenance    │
        │ diagnostics    │
        └────────────────┘
```

## Migration Steps

### Phase 1: Create new crate structure
1. Create `shannon-core-base` crate
2. Move `error.rs`, `state.rs`, `settings.rs`, `hooks.rs`, `permissions.rs`
3. Update workspace `Cargo.toml`

### Phase 2: Extract API & Tools
4. Create `shannon-core-api` crate (move `api.rs`, `api_services.rs`, `vcr.rs`)
5. Create `shannon-core-tools` crate (move tool-related modules)
6. Update imports in dependent crates

### Phase 3: Extract Query Engine
7. Create `shannon-core-query` crate
8. Move `query_engine.rs`, `compact.rs`
9. Wire up dependencies on `-api` and `-tools`

### Phase 4: Extract Memory & Plugins
10. Create `shannon-core-memory` crate
11. Create `shannon-core-plugins` crate
12. Update all imports

### Phase 5: Extract Feature/Maintenance/Diagnostics
13. Create remaining crates
14. Move modules to appropriate crates
15. Final import updates

### Phase 6: Cleanup
16. Update `shannon-core` to only re-export
17. Run tests and fix any issues
18. Update documentation

## Benefits

1. **Clearer separation of concerns**: Each crate has a focused purpose
2. **Better compile times**: Changes only rebuild affected crates
3. **Easier testing**: Can test individual components in isolation
4. **Flexible dependencies**: External projects can depend on specific features
5. **Better documentation**: Each crate can have its own docs

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Breaking changes for downstream crates | Re-export everything from `shannon-core` initially |
| Circular dependencies | Carefully design dependency graph; use traits |
| Increased complexity | Keep workspace structure clean; use clear naming |
| Longer build times (initially) | Parallel builds offset this over time |

## Estimated Effort

- **Planning**: 1 day (analysis, dependency mapping)
- **Implementation**: 3-5 days (create crates, move modules, fix imports)
- **Testing**: 2-3 days (run tests, fix regressions)
- **Documentation**: 1 day (update README, migration guide)
- **Total**: 7-10 days

## Next Steps

1. ✅ Complete analysis (this document)
2. ⏳ Create `shannon-core-base` crate
3. ⏳ Migrate modules incrementally
4. ⏳ Update workspace dependencies
5. ⏳ Run full test suite
6. ⏳ Update documentation
