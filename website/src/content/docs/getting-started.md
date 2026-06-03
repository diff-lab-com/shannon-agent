---
title: Getting Started
order: 1
section: introduction
---

# Getting Started

## Install

Choose one of the following methods:

### Cargo (recommended)

```bash
cargo install --git https://github.com/shannon-agent/shannon-code.git
```

### curl

```bash
curl -fsSL https://github.com/shannon-agent/shannon-code/releases/latest/download/install.sh | sh
```

### Homebrew

```bash
brew install shannon-agent/tap/shannon
```

## Configure

Shannon Code works with any OpenAI-compatible LLM provider. Create a config file:

```bash
mkdir -p ~/.shannon
cat > ~/.shannon/config.toml << 'EOF'
[llm]
provider = "anthropic"
model = "claude-sonnet-4-20250514"
api_key_env = "SHANNON_API_KEY"
EOF
```

Set your API key:

```bash
export SHANNON_API_KEY="sk-ant-..."
```

### Other Providers

| Provider | `provider` value | Default model |
|----------|-----------------|---------------|
| Anthropic | `anthropic` | `claude-sonnet-4-20250514` |
| OpenAI | `openai` | `gpt-4o` |
| Ollama | `ollama` | `codellama` |
| DeepSeek | `deepseek` | `deepseek-coder` |
| Custom | `custom` | — |

For custom endpoints, set `base_url` in the `[llm]` section.

## Run

```bash
shannon
```

This opens the interactive terminal UI. For non-interactive mode:

```bash
shannon --prompt "Fix the auth bug in src/login.rs"
```

## Next Steps

- Read the [Architecture](/docs/architecture) overview
- Configure [MCP extensions](/docs/configuration#mcp-extensions)
- Set up [permission profiles](/docs/configuration#permission-profiles)
