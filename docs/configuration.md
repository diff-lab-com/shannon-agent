# Configuration Reference

Shannon Code uses a layered configuration system. Higher-priority sources override lower ones.

## Priority Order

1. **CLI arguments** (`--model`, `--provider`, `--prompt`, etc.)
2. **Environment variables** (`SHANNON_*`)
3. **Project config** (`.shannon.toml` in project root)
4. **Global config** (`~/.shannon/config.toml`)

## Global Config

Location: `~/.shannon/config.toml`

```toml
provider = "anthropic"           # anthropic | openai | ollama | custom
api_key = "sk-ant-..."           # API key (or use env var)
model = "claude-sonnet-4-20250514"
base_url = ""                    # Custom API endpoint (for OpenAI-compatible providers)
max_tokens = 8192
temperature = 0.7
timeout = 120                    # Request timeout in seconds
permission_profile = "balanced"  # strict | balanced | permissive
```

## Environment Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `SHANNON_API_KEY` | API key for the LLM provider | `sk-ant-...` |
| `SHANNON_MODEL` | Model name | `claude-sonnet-4-20250514` |
| `SHANNON_PROVIDER` | Provider type | `anthropic`, `openai`, `ollama` |
| `SHANNON_BASE_URL` | Custom API endpoint | `https://api.deepseek.com/v1` |
| `SHANNON_MAX_TOKENS` | Maximum output tokens | `8192` |
| `SHANNON_TEMPERATURE` | Sampling temperature | `0.7` |
| `SHANNON_TIMEOUT` | Request timeout (seconds) | `120` |
| `SHANNON_PERMISSION_PROFILE` | Permission profile | `strict`, `balanced`, `permissive` |
| `SHANNON_DEBUG` | Enable debug logging | `1` |

Fallback detection: `ANTHROPIC_API_KEY` and `OPENAI_API_KEY` are auto-detected.

## Provider Configuration

### Anthropic

```toml
provider = "anthropic"
api_key = "sk-ant-..."
model = "claude-sonnet-4-20250514"
```

Supports prompt caching with three-layer cache breakpoint injection.

### OpenAI

```toml
provider = "openai"
api_key = "sk-..."
model = "gpt-4o"
base_url = "https://api.openai.com/v1"
```

### Ollama (local)

```toml
provider = "ollama"
model = "llama3"
```

Auto-detects Ollama on `localhost:11434`.

### DeepSeek

```toml
provider = "openai"
api_key = "sk-..."
model = "deepseek-chat"
base_url = "https://api.deepseek.com/v1"
```

### Any OpenAI-Compatible Endpoint

```toml
provider = "openai"
api_key = "your-key"
model = "your-model"
base_url = "https://your-endpoint/v1"
```

## MCP Server Configuration

MCP servers are configured separately from the main config.

### Project-level (`.mcp.json`)

```json
{
  "mcpServers": {
    "fetch": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-fetch"]
    },
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-filesystem", "/path/to/project"]
    }
  }
}
```

### Global (`~/.claude/settings.json` or `~/.shannon/settings.json`)

Same format as `.mcp.json`. Project-level takes priority.

### Transport Types

| Transport | Config | Use case |
|-----------|--------|----------|
| stdio | `"command": "npx"` | Local process (most common) |
| SSE | `"url": "http://..."` | Remote server |
| Streamable HTTP | `"url": "http://..."` | Remote server with streaming |

## Permission Profiles

### Built-in Profiles

| Profile | Auto-approve read | Auto-approve write | Confirm destructive |
|---------|------------------|--------------------|---------------------|
| Strict | No | No | Yes |
| Balanced | Yes | No | Yes |
| Permissive | Yes | Yes | No |

### Custom Profiles

Create `.shannon/profiles/my-profile.toml`:

```toml
name = "my-profile"
auto_approve = ["Read", "Glob", "Grep"]
confirm = ["Edit", "Write", "Bash"]
deny = ["Bash:rm -rf"]
```

## CLI Arguments

```
shannon [OPTIONS] [PATH]

Options:
  --prompt <TEXT>           Non-interactive mode
  --resume [<UUID>]        Resume session
  --continue, -c           Resume most recent (alias for --resume)
  --schema <FILE|JSON>     JSON Schema for structured output
  --pipe                   Read prompt from stdin
  --diff-only              Only output file diffs
  --model <MODEL>          Override model
  --provider <PROVIDER>    Override provider
  --allowed-tools <LIST>   Restrict tool access
  --max-turns <N>          Limit conversation turns
  --output-format <FMT>    Output format: text, json
  --yes                    Bypass permissions (dangerous)
```
