# Architecture

Shannon Code is organized as a Cargo workspace with 12 crates, each with a single responsibility.

## Crate Dependency Graph

```
shannon-cli
├── shannon-core
│   ├── shannon-types (re-exported)
│   ├── shannon-tool-interface
│   └── shannon-mcp
├── shannon-ui
│   └── shannon-core
├── shannon-tools (implements shannon-tool-interface)
├── shannon-commands
├── shannon-skills
└── shannon-agents
    └── shannon-core

shannon-agent (standalone binary, JSON-RPC)
└── shannon-core
```

## Core Data Flow

```
User Input (REPL)
    │
    ▼
shannon-ui (Terminal UI)
    │
    ▼
shannon-core / QueryEngine
    │
    ├──► LlmClient (API adapter)
    │       ├── AnthropicAdapter
    │       ├── OpenAiAdapter
    │       └── OllamaAdapter
    │
    ├──► ToolExecutor
    │       ├── Built-in tools (shannon-tools)
    │       └── MCP tools (shannon-mcp)
    │
    ├──► PermissionManager
    │       ├── Rule-based classifier
    │       └── LLM auto-classifier
    │
    └──► SessionManager
            ├── Persistence
            ├── History search
            └── Context compaction
```

## Key Design Patterns

### Multi-Provider Adapter

`LlmClient` in `shannon-core/src/api/` normalizes different LLM providers through an adapter pattern:

- `AnthropicAdapter` — Native Anthropic API with prompt caching
- `OpenAiAdapter` — OpenAI-compatible (also used for DeepSeek, custom endpoints)
- `OllamaAdapter` — Local Ollama with auto-detection

Each adapter implements the same streaming interface. The adapter is selected by `provider` config.

### Tool Trait

All tools implement `Tool` from `shannon-tool-interface`:

```rust
trait Tool {
    fn name(&self) -> &str;
    fn execute(&self, input: Value) -> ToolResult<ToolOutput>;
    fn execute_streaming(&self, input: Value, sender: Box<dyn ToolProgressSender>) -> ToolResult<ToolOutput>;
    fn is_read_only(&self) -> bool;
    fn is_concurrency_safe(&self) -> bool;
    fn is_destructive(&self) -> bool;
}
```

### Streaming Pipeline

```
SSE byte stream
    → SseStream (chunk boundary buffering)
    → MessageStream (parsed events)
    → UI rendering (streaming text, tool calls, thinking blocks)
```

### Config Priority Chain

```
CLI args > SHANNON_* env vars > .shannon.toml (project) > ~/.shannon/config.toml (global)
```

MCP servers are configured separately in `.mcp.json` or `~/.claude/settings.json`.

### Permission Pipeline

```
Tool call request
    → PermissionManager
    → PermissionClassifier (rule-based)
    → LLM fallback (if ambiguous, confidence < 0.7)
    → 4-tier decision: hard_deny > soft_deny > allow > explicit intent
    → Interactive approval (if needed)
```

## Testing Architecture

- **Unit tests**: `#[cfg(test)] mod tests` within each source file
- **Integration tests**: `crates/<crate>/tests/` for cross-module testing
- **HTTP mocking**: `mockito::Server` — never hits real APIs
- **YAML scenarios**: `tests/scenarios/*.yaml` — declarative test definitions
- **Record/replay**: Real API responses recorded as JSONL fixtures, replayed via mockito
- **Benchmarks**: `criterion` in `crates/*/benches/`

## Error Handling

- Library crates use `thiserror` for typed error enums (`ApiError`, `QueryError`, `ToolError`)
- CLI/bin crates use `anyhow` for ergonomic error handling
- Production code uses `expect("reason")` over `unwrap()` for panic diagnostics
