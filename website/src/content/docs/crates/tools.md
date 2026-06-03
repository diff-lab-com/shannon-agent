---
title: shannon-tools
order: 22
section: crates
---

# shannon-tools

Tool implementations for file operations, bash execution, search, and configuration management.

## Built-in Tools

| Tool | Read-Only | Description |
|------|-----------|-------------|
| `Read` | Yes | Read files (with image support, progressive loading) |
| `Edit` | No | String replacement in files (with three-way merge fallback) |
| `Write` | No | Create or overwrite files |
| `Bash` | No | Execute shell commands with real-time output |
| `Grep` | Yes | Regex search across files |
| `Glob` | Yes | File pattern matching |
| `Agent` | No | Spawn sub-agents with model/tool override |
| `AnalyzeImage` | No | Image analysis (file or URL) |
| `MergeResolve` | No | Resolve merge conflicts |

## Key Modules

### File Operations (`file/`)
- `read.rs` — File reading with image support (PNG/JPG/GIF/WebP/BMP → base64)
- `edit.rs` — String replacement with three-way merge fallback when `old_string` not found
- `write.rs` — File creation/overwrite
- `merge.rs` — LCS-based three-way merge, conflict marker parsing
- `merge_tool.rs` — Conflict resolution tool
- `diff_renderer.rs` — Unified diff rendering

### Search
- `Grep` — ripgrep-based regex search
- `Glob` — Pattern-based file discovery

### Bash
- Real-time streaming output via `ToolProgress` events
- Working directory control
- Timeout support

### Tool Trait

```rust
pub trait Tool: Send + Sync {
    fn execute(&self, input: ToolInput) -> Pin<Box<dyn Future<Output = ToolOutput>>>;
    fn execute_streaming(&self, input: ToolInput, sender: ProgressSender);
    fn is_read_only(&self) -> bool;
    fn is_concurrency_safe(&self) -> bool;
    fn is_destructive(&self) -> bool;
}
```
