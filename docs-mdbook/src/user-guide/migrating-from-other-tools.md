# Migrating from Other Tools

## From Claude Code CLI

Shannon is CLI-compatible with Claude Code's MCP configuration and tool interface.

### Config Migration

Claude Code uses `~/.claude/settings.json`. Shannon reads the same MCP config:

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

| Claude Code | Shannon |
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

| Codex CLI | Shannon |
|-----------|-------------|
| `codex "text"` | `shannon --prompt "text"` |
| `codex --model gpt-4o` | `shannon --model gpt-4o` |
| `codex --full-auto` | `shannon --yes` |
| Sandbox (Seatbelt/Docker) | Project-dir sandboxing |

## From OpenCode

| OpenCode | Shannon |
|----------|-------------|
| `opencode` | `shannon` |
| Go-based | Rust-based |
| Limited tool set | Full tool set + MCP |

## Version Migration Notes (within Shannon)

### v0.11: event-sourced sessions

Starting with v0.11, sessions are stored exclusively as append-only `events.jsonl` logs
(see [Trace](trace.md)). Legacy `sessions/<uuid>.json` snapshots and transcript files are no
longer read. If you upgrade from an earlier release, old sessions cannot be resumed — export
anything you need before upgrading.
