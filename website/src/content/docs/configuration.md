---
title: Configuration
order: 3
section: reference
---

# Configuration

## Config Priority

Shannon Code loads configuration from multiple sources, with later sources overriding earlier ones:

1. Default values
2. `~/.shannon/config.toml` (global)
3. `.shannon.toml` (project-local)
4. Environment variables (`SHANNON_*`)
5. CLI arguments

## Global Config

Create `~/.shannon/config.toml`:

```toml
[llm]
provider = "anthropic"
model = "claude-sonnet-4-20250514"
api_key_env = "SHANNON_API_KEY"
max_tokens = 4096
temperature = 0.7

[ui]
theme = "dark"
vim_mode = false

[permissions]
default_mode = "balanced"  # strict | balanced | permissive
```

## Environment Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `SHANNON_API_KEY` | LLM API key | `sk-ant-...` |
| `SHANNON_MODEL` | Override model | `gpt-4o` |
| `SHANNON_PROVIDER` | Override provider | `openai` |
| `SHANNON_BASE_URL` | Custom API endpoint | `http://localhost:11434/v1` |

## Provider Configuration

### Anthropic

```toml
[llm]
provider = "anthropic"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"
```

### OpenAI

```toml
[llm]
provider = "openai"
model = "gpt-4o"
api_key_env = "OPENAI_API_KEY"
```

### Ollama (local)

```toml
[llm]
provider = "ollama"
model = "codellama"
base_url = "http://localhost:11434"
```

### DeepSeek

```toml
[llm]
provider = "deepseek"
model = "deepseek-coder"
api_key_env = "DEEPSEEK_API_KEY"
```

### Custom Endpoint

```toml
[llm]
provider = "custom"
base_url = "https://api.example.com/v1"
model = "my-model"
api_key_env = "MY_API_KEY"
```

## MCP Extensions

MCP (Model Context Protocol) servers are configured in `.mcp.json`, `~/.claude/settings.json`, or `~/.shannon/settings.json`:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/dir"]
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "ghp_..." }
    }
  }
}
```

Tools are auto-discovered via `tools/list`.

## Permission Profiles

Named presets loaded from `.shannon/profiles/*.toml` or `.claude/profiles/*.toml`:

```toml
name = "strict"
description = "Confirm all operations"

[rules]
auto_approve = ["read_file", "search"]
confirm = ["write_file", "edit_file", "bash"]
deny = ["rm_rf"]
```

Built-in profiles: `strict`, `balanced`, `permissive`. Switch with `/profile <name>`.

## CLI Arguments

```bash
shannon [OPTIONS]

Options:
  --prompt <text>       Non-interactive mode
  --schema <path>       JSON Schema for structured output
  --model <model>       Override model
  --provider <provider> Override provider
  --config <path>       Config file path
  -h, --help            Show help
  -V, --version         Show version
```
