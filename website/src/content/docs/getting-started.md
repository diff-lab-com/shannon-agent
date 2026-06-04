---
title: Getting Started
order: 2
section: getting-started
---

# Getting Started

Get Shannon Code running in under a minute. No Rust toolchain needed.

## Install

Pick your platform:

### macOS

```bash
# Apple Silicon (M1/M2/M3/M4)
curl -fsSL https://github.com/shannon-agent/shannon-code/releases/latest/download/install.sh | sh

# Intel
curl -fsSL https://github.com/shannon-agent/shannon-code/releases/latest/download/install.sh | sh

# Or with Homebrew
brew install shannon-agent/tap/shannon
```

### Linux

```bash
curl -fsSL https://github.com/shannon-agent/shannon-code/releases/latest/download/install.sh | sh
```

### Windows

Download the latest binary:

[Download shannon-cli-x86_64-pc-windows-msvc.zip](https://github.com/shannon-agent/shannon-code/releases/latest/download/shannon-cli-x86_64-pc-windows-msvc.zip)

Or in PowerShell:

```powershell
irm https://github.com/shannon-agent/shannon-code/releases/latest/download/install.ps1 | iex
```

### Other Methods

```bash
# Cargo (requires Rust toolchain)
cargo install --git https://github.com/shannon-agent/shannon-code.git
```

## Configure

Set your API key as an environment variable:

```bash
export SHANNON_API_KEY="sk-ant-..."
```

Or create a config file at `~/.shannon/config.toml`:

```toml
provider = "anthropic"
api_key = "sk-ant-..."
model = "claude-sonnet-4"
```

Shannon supports multiple providers. Switch by changing the config:

| Provider | `provider` value | `model` example |
|----------|-----------------|-----------------|
| Anthropic | `anthropic` | `claude-sonnet-4` |
| OpenAI | `openai` | `gpt-4o` |
| DeepSeek | `deepseek` | `deepseek-chat` |
| Ollama (local) | `ollama` | `llama3` |

See [Configuration](../configuration/) for all options.

## First Run

```bash
# Start interactive REPL
shannon

# Or ask a question directly
shannon --prompt "explain this codebase"

# Continue where you left off
shannon --resume
```

## Verify

Run `shannon` in a project directory. You should see the REPL prompt and can start asking questions about your code.

## What's Next?

- [Configuration](../configuration/) — customize providers, MCP servers, permissions
- [Features](../features/) — tools, agents, hooks, memory, caching
- [Migration](../migration/) — switch from Claude Code, Codex CLI, or OpenCode

---

Building from source? See [Building from Source](../building-from-source/).
