# Migration Guide

## From Claude Code CLI

Shannon Code is CLI-compatible with Claude Code's MCP configuration and tool interface.

### Config Migration

Claude Code uses `~/.claude/settings.json`. Shannon Code reads the same MCP config:

```json
{
  "mcpServers": {
    "my-server": {
      "command": "npx",
      "args": ["-y", "my-mcp-server"]
    }
  }
}
```

No changes needed — Shannon reads `.mcp.json`, `~/.claude/settings.json`, and `~/.shannon/settings.json`.

### CLI Equivalence

| Claude Code | Shannon Code |
|-------------|-------------|
| `claude` | `shannon` |
| `claude -p "text"` | `shannon --prompt "text"` |
| `claude --resume` | `shannon --resume` |
| `claude --model opus` | `shannon --model opus` |
| `CLAUDE.md` | `CLAUDE.md` (same) |

### Key Differences

- Shannon uses `~/.shannon/config.toml` (not `settings.json`) for app config
- Shannon has `/batch` for parallel worktree PR creation
- Shannon has 32 hook events vs Claude Code's 18+
- Shannon's permission system has 9 modes vs Claude Code's 3

## From Codex CLI

| Codex CLI | Shannon Code |
|-----------|-------------|
| `codex "text"` | `shannon --prompt "text"` |
| `codex --model gpt-4o` | `shannon --model gpt-4o` |
| `codex --full-auto` | `shannon --yes` |
| Sandbox (Seatbelt/Docker) | Project-dir sandboxing |

## From OpenCode

| OpenCode | Shannon Code |
|----------|-------------|
| `opencode` | `shannon` |
| Go-based | Rust-based |
| Limited tool set | Full tool set + MCP |
