# shannon-core

The central engine crate. Handles LLM communication, query processing, tool orchestration, permissions, and session state.

## Key Modules

### API Client (`api/`)
- `LlmClient` — Multi-provider LLM client (Anthropic, OpenAI, Ollama)
- `adapter.rs` — Provider-specific adapters normalizing request/response formats
- `types.rs` — ContentBlock, ToolDefinition, StreamEvent, ContentDelta

### Query Engine (`query_engine/`)
- `engine.rs` — Main query processing loop: user message → LLM → tool calls → response
- Manages streaming, tool execution, and context window

### Permissions (`permission.rs`)
- `PermissionManager` — 9 approval modes with `PermissionClassifier`
- `LlmPermissionClassifier` — LLM-based fallback for ambiguous cases
- 4-tier precedence: hard_deny > soft_deny > allow > explicit intent

### Context Management
- `compact/` — Message compaction engine with custom summarizers
- `context_budget.rs` — Phase-based context budget allocation
- `progressive_loader.rs` — Large file truncation with head/tail preservation
- `checkpoint.rs` — Git-based file checkpointing for undo/rewind

### Tool Orchestration
- `tool_orchestration.rs` — Call deduplication, parallel/sequential optimization
- `tool_result_cache.rs` — TTL-based cache for read-only tool results

### State & Session
- `state.rs` — Session persistence (save/load/resume)
- `activity_manager.rs` — Activity tracking across sessions

### Hooks
- `hooks/` — HookManager with 32 event types, fully wired

## Error Types

```rust
pub enum ApiError {
    RateLimit { retry_after: Option<u64> },
    Authentication(String),
    ContextOverflow { tokens: usize, limit: usize },
    Provider(String),
    Timeout,
}
```
