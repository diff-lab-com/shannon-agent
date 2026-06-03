---
title: Hooks
order: 35
section: features
---

# Hooks

The hook system lets you run custom shell commands in response to events during a session.

## Configuration

Hooks are configured in `.shannon.toml` or `~/.shannon/config.toml`:

```toml
[[hooks]]
event = "PostToolCall"
pattern = "Edit"
command = "cargo clippy --quiet 2>&1"

[[hooks]]
event = "PreCompact"
command = "echo 'Compacting...' >> /tmp/shannon.log"
```

## Available Events (32 types)

| Event | Trigger |
|-------|---------|
| `PreToolCall` / `PostToolCall` | Before/after any tool execution |
| `PreBash` / `PostBash` | Before/after bash commands |
| `PreEdit` / `PostEdit` | Before/after file edits |
| `PreWrite` / `PostWrite` | Before/after file writes |
| `PreRead` / `PostRead` | Before/after file reads |
| `PreCompact` / `PostCompact` | Context compaction |
| `SubagentStart` / `SubagentStop` | Agent lifecycle |
| `WorktreeCreate` / `WorktreeRemove` | Worktree management |
| `ConfigChange` | Settings file changed |
| `TaskCreated` / `TaskCompleted` | Task board events |
| `SessionStart` / `SessionEnd` | Session lifecycle |
| `PreMessage` / `PostMessage` | Message processing |

## Hook Execution

- Hooks run **non-blocking** via `tokio::spawn`
- Hook output is displayed in the REPL
- A hook can block an action by returning a non-zero exit code (for pre-hooks)
- Pattern matching filters hooks to specific tool names or conditions

## Example: Auto-format on edit

```toml
[[hooks]]
event = "PostEdit"
pattern = "*.rs"
command = "rustfmt {file_path}"
```
