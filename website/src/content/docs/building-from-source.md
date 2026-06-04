---
title: Building from Source
order: 1
section: development
---

# Building from Source

Build Shannon Code from source if you want to contribute, hack on the code, or run a custom build.

## Prerequisites

- **Rust** 1.88+ (edition 2024)
- **Git**
- An LLM API key (Anthropic, OpenAI, or a local Ollama instance)

## Build

```bash
git clone https://github.com/shannon-agent/shannon-code.git
cd shannon-code
cargo build --release
```

The binary is at `target/release/shannon`.

## Configure

Create `~/.shannon/config.toml`:

```toml
provider = "anthropic"
api_key = "sk-ant-..."
model = "claude-sonnet-4"
```

Or use environment variables:

```bash
export SHANNON_API_KEY="sk-ant-..."
export SHANNON_MODEL="claude-sonnet-4"
export SHANNON_PROVIDER="anthropic"
```

See [Configuration](../configuration/) for all options.

## Run

```bash
# Interactive REPL
shannon

# One-shot prompt (headless mode)
shannon --prompt "explain this codebase"

# Resume last session
shannon --resume

# Resume specific session
shannon --resume <session-uuid>
```

## Verify

```bash
cargo test --workspace -- --test-threads=1
```

All tests should pass. Use `--test-threads=1` because some tests share environment variables.

## Next Steps

- [Architecture](../architecture/) — understand the crate structure
- [Contributing](../contributing/) — coding standards and PR workflow
- [Testing](../testing/) — test commands and writing tests
