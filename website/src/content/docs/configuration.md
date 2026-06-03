---
title: Configuration
order: 10
section: reference
---

# Configuration

Shannon Code uses a layered configuration system. Later sources override earlier ones:

**CLI args > Environment variables > Project config > Global config**

## Global Config

Path: `~/.shannon/config.toml`

```toml
provider = "anthropic"
api_key = "sk-ant-..."
model = "claude-sonnet-4"
max_tokens = 16384
temperature = 1.0
permissions_mode = "auto-allow"
```

## Project Config

Path: `.shannon.toml` (in project root)

Same format as global config. Project settings override global settings.

## Environment Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `SHANNON_API_KEY` | API key for the LLM provider | `sk-ant-...` |
| `SHANNON_MODEL` | Model to use | `claude-sonnet-4` |
| `SHANNON_PROVIDER` | LLM provider | `anthropic`, `openai`, `ollama` |
| `SHANNON_BASE_URL` | Custom API base URL | `http://localhost:11434/v1` |
| `SHANNON_MAX_TOKENS` | Max response tokens | `16384` |
| `SHANNON_TEMPERATURE` | Response randomness (0-1) | `0.7` |
| `SHANNON_TIMEOUT` | Request timeout (seconds) | `120` |
| `SHANNON_DEBUG` | Enable debug logging | `true` |
| `SHANNON_PERMISSION_PROFILE` | Permission profile | `balanced` |

Fallback env vars: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`.

## CLI Flags

| Flag | Description |
|------|-------------|
| `--prompt <text>` | Run in headless/CI mode |
| `--resume [UUID]` | Resume a session |
| `--continue`, `-c` | Resume most recent session |
| `--model <name>` | Override model for this session |
| `--pipe` | Read stdin as prompt input |
| `--allowed-tools <list>` | Restrict available tools |
| `--max-turns <n>` | Limit conversation turns |
| `--diff-only` | Show only diffs, no chat |
| `--schema <file>` | Validate output against JSON Schema |
| `--yes` | Auto-approve all permissions |
| `--register-url-scheme` | Register `shannon://` URL handler |
| `--unregister-url-scheme` | Unregister URL handler |

## MCP Servers

MCP servers are configured in `.mcp.json`, `~/.claude/settings.json`, or `~/.shannon/settings.json`:

```json
{
  "mcpServers": {
    "my-server": {
      "command": "npx",
      "args": ["-y", "my-mcp-server"],
      "env": { "API_KEY": "..." }
    }
  }
}
```

Tools are auto-discovered via `tools/list`.

## Permission Profiles

| Profile | Description |
|---------|-------------|
| `strict` | Approve all tool calls |
| `balanced` | Auto-approve reads, approve writes |
| `permissive` | Auto-approve non-destructive, deny destructive |
| `auto-allow` | Auto-approve everything except critical |

Set via config or `SHANNON_PERMISSION_PROFILE` env var.
