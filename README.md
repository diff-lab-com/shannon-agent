# Shannon Code

> **Note:** The unified `shannon` CLI replaces the former `shannon-code` product from earlier releases. Install paths, subcommands, and configuration are unchanged — only the binary name changed.

<div align="center">

**A high-performance, open-source AI-assisted coding tool, written in Rust**

[![Rust](https://img.shields.io/badge/rust-1.88+-orange.svg)](https://www.rust-lang.org)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-7889-brightgreen.svg)
[![Crates](https://img.shields.io/badge/crates-12-blue.svg)

[English](#what-is-shannon-code) | [中文文档](./README.zh-CN.md) | [Documentation](https://shannon-agent.github.io/shannon-code/)

</div>

---

## What is Shannon Code?

Shannon Code is a fully open-source, Rust-based AI coding assistant that works with **any LLM provider** — Anthropic, OpenAI, Ollama, DeepSeek, or any OpenAI-compatible endpoint. It provides a rich terminal UI, powerful tool orchestration, multi-agent coordination, and the Model Context Protocol (MCP) for extensibility.

Unlike closed-source alternatives, Shannon Code has **no hidden billing injections**, **no cache-destroying dynamic headers**, and **no vendor lock-in**. Every line of code is auditable, and every behavior is verified by nearly 8,000 tests.

**Key differentiators:**

| Feature | Shannon Code | Typical closed-source tools |
|---------|-------------|---------------------------|
| LLM providers | Anthropic, OpenAI, Ollama, any OpenAI-compatible | Single vendor |
| Cost transparency | No hidden fees or cache manipulation | Dynamic billing headers inflate costs 10-20x |
| Test coverage | ~7,900 tests, every file covered | Often zero tests |
| Extensibility | MCP protocol, plugin system, skills framework | Limited or closed |
| Agent orchestration | Multi-agent teams, worktree isolation, `/batch` PRs | Basic or none |
| Code auditability | Every line visible in source code | Black box |

---

## Features

### Multi-Provider LLM Support

Connect to any LLM with a single config file:

| Provider | Models | Setup |
|----------|--------|-------|
| Anthropic | Claude Sonnet, Opus, Haiku | `provider = "anthropic"` |
| OpenAI | GPT-4o, GPT-4, GPT-3.5 | `provider = "openai"` |
| Ollama | Llama, Mistral, Qwen, etc. | `provider = "ollama"` (auto-detect) |
| DeepSeek | DeepSeek Chat, Coder | `provider = "openai"` + `base_url` |
| Any OpenAI-compatible | Any model | `provider = "openai"` + `base_url` |

Anthropic prompt caching is supported with three-layer cache breakpoint injection for maximum efficiency.

### Tool System

A comprehensive suite of built-in tools for code manipulation:

- **File operations** — Read, Edit, Write, MultiEdit with three-way merge and conflict resolution
- **Code analysis** — Syntax highlighting, symbol navigation (LSP), diff rendering
- **Git integration** — Status, diff, log, commit, branch management
- **Command execution** — Sandboxed Bash with streaming output and timeout control
- **Web search** — Real-time information retrieval
- **Image analysis** — Screenshot understanding and visual reasoning
- **Notebook editing** — Jupyter notebook cell read/edit/insert/delete

### MCP (Model Context Protocol)

Full MCP implementation compatible with Claude Code's MCP ecosystem:

- **Transports**: stdio, SSE, streamable HTTP
- **Tool discovery**: `tools/list` with deferred schema loading — scales to 100+ tools
- **Fuzzy search**: `mcp__tool_search` for finding tools by name or description
- **Resource management**: Subscribe to resource updates, handle notifications
- **Webhook support**: HMAC-SHA256 signed events with retry and persistence
- **Configuration**: `.mcp.json` (project-level) or `~/.claude/settings.json`

### Multi-Agent Orchestration

Coordinate multiple AI agents for complex tasks:

- **Team coordination** — `TeamCreate`, `SendMessage`, task assignment and tracking
- **Worktree isolation** — Each agent works in its own git worktree
- **Per-agent config** — Override model, tools, and working directory per agent
- **`/batch` command** — Decompose tasks, create worktrees, spawn agents, create PRs in parallel
- **Agent dashboard** — Real-time status view with `AgentBarWidget` and `AgentsPanel`

### Permission System

Sophisticated safety controls with multiple modes:

- **Rule-based classifier** — Pattern matching for known safe/dangerous operations
- **LLM auto-classifier** — Async fallback for ambiguous cases (confidence < 0.7)
- **Permission profiles** — Strict, Balanced, Permissive, or Custom (loadable from `.shannon/profiles/*.toml`)
- **4-tier precedence** — Hard deny > Soft deny > Allow > Explicit intent
- **Approval workflows** — Interactive confirmation for risky operations

### Session & Context Management

- **Session persistence** — Save, resume by ID, search history
- **Context compression** — Auto-compact, micro-compact, conversation phase tracking
- **Memory system** — Persistent memory store with auto-extraction and consolidation
- **Extended context** — Phase-based budget reallocation (Initialization → Active → Extended → Critical)
- **Checkpoint/Undo** — Git-based file checkpointing with diff preview before revert
- **Plan mode** — Structured planning with approval workflows

### Plugin & Skill System

Extend Shannon with plugins and skills:

- **Plugin discovery** — Load from `.shannon/plugins/` with manifest parsing
- **Tool plugins** — MCP-based tool discovery and registration
- **Command plugins** — Register as slash commands in the REPL
- **Skill plugins** — Prompt templates triggered by slash commands
- **Hook system** — 32+ events (tool execution, compaction, config changes, agent lifecycle)

### Internationalization

- 10 languages: English, Chinese, Hindi, Spanish, French, Arabic, Bengali, Portuguese, Russian, Japanese
- Community-contributable locale files in `locales/` directory
- UI language switchable at runtime

### VS Code Extension

A companion extension for VS Code is available in `editors/vscode/`:

- WebView chat panel with Markdown rendering
- Diff viewer for reviewing file changes (accept/reject)
- NDJSON subprocess communication with `shannon --prompt`
- Status bar indicator for connection state
- Configuration sync between VS Code settings and Shannon CLI

---

## Quick Start

### 1. Install

Download the latest release for your platform:

```bash
# Linux / macOS (from GitHub Releases)
curl -fsSL https://github.com/shannon-agent/shannon-agent/releases/latest/download/shannon-$(uname -s)-$(uname -m).tar.gz | tar xz
sudo mv shannon /usr/local/bin/

# Or with cargo (requires Rust 1.88+)
cargo install --git https://github.com/shannon-agent/shannon-agent.git
```

<details>
<summary>Other platforms</summary>

- **Windows**: Download `.zip` from [Releases](https://github.com/shannon-agent/shannon-agent/releases)
- **From source**: See [Developer Guide](#developer-guide) below

</details>

### 2. Configure

Set your API key and preferred model:

```bash
# Option A: Environment variable (fastest)
export SHANNON_API_KEY="sk-ant-..."
export SHANNON_MODEL="claude-sonnet-4-20250514"

# Option B: Config file (persistent)
mkdir -p ~/.shannon
cat > ~/.shannon/config.toml << 'EOF'
provider = "anthropic"
api_key = "sk-ant-..."
model = "claude-sonnet-4-20250514"
max_tokens = 8192
EOF
```

<details>
<summary>Other providers</summary>

**OpenAI / DeepSeek / Any compatible:**
```bash
cat > ~/.shannon/config.toml << 'EOF'
provider = "openai"
model = "gpt-4o"
api_key = "sk-..."
base_url = "https://api.openai.com/v1"
EOF
```

**Ollama (local, no API key needed):**
```bash
ollama serve
export SHANNON_MODEL="llama3"
```

</details>

<details>
<summary>Notifications (optional)</summary>

Shannon fires notifications on query completion / errors / tool-use events.
The REPL renders them via the terminal's native notifier; headless mode
(`--notify` flag) shells out to `notify-send` / `osascript` / BurntToast.

To also push notifications to a chat webhook (Slack, Discord, Feishu,
WeChat Work, or any custom endpoint), add a `[notifications.webhook]`
block to your `.shannon.toml`:

```toml
[notifications.webhook]
url = "https://hooks.slack.com/services/T.../B..."
template = "slack"      # slack | discord | feishu | wechat | raw | custom = "<template>"
include_body = true      # include notification body in the payload (default false)
# Optional HMAC-SHA256 signing — receivers verify via X-Shannon-Signature header
secret = "your-shared-secret"
timeout_ms = 3000
```

**Verifying HMAC signatures on the receiver side** (GitHub/Stripe
convention; the signature is sent as `X-Shannon-Signature: sha256=<hex>`):

```python
import hmac, hashlib

def verify(raw_body: bytes, sig_header: str, secret: str) -> bool:
    if not sig_header.startswith("sha256="):
        return False
    expected = hmac.new(secret.encode(), raw_body, hashlib.sha256).hexdigest()
    return hmac.compare_digest(sig_header.removeprefix("sha256="), expected)
```

</details>

### 3. Run

```bash
shannon                          # Interactive REPL
shannon /path/to/project         # Open in a project directory
shannon --resume                  # Resume last session
```

That's it. Type your question and press Enter.

<details>
<summary>More usage examples</summary>

```bash
shannon --prompt "Explain the auth module"    # Non-interactive / CI mode
shannon --prompt "List TODOs" --schema schema.json  # Structured JSON output
echo "fix this bug" | shannon --pipe           # Pipe mode
shannon --prompt "refactor" --allowed-tools Read,Edit,Bash,Grep --max-turns 10  # CI
shannon --prompt "fix lint" --diff-only         # Only output diff
```

</details>

<details>
<summary>REPL commands</summary>

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/config` | View or edit configuration |
| `/model` | Switch LLM model |
| `/compact` | Compress conversation context |
| `/undo list` | List file checkpoints |
| `/undo <n>` | Preview and revert to checkpoint |
| `/rewind` | Rewind conversation and/or code |
| `/diff` | Show file diff viewer |
| `/batch` | Parallel worktree-isolated PR creation |
| `/team` | Manage agent teams |
| `/cost` | Show token usage and cost |
| `/search` | Search conversation history |
| `/doctor` | Check Shannon installation health |
| `/routine` | Manage triggered/scheduled routines |
| `/preset` | Use conversation presets (review, debug, etc.) |
| `/session` | Save/load session templates |

</details>

<details>
<summary>MCP server setup</summary>

Add MCP servers in `.mcp.json` (project-level) or `~/.claude/settings.json`:

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

</details>

<details>
<summary>Environment variables reference</summary>

| Variable | Description |
|----------|-------------|
| `SHANNON_API_KEY` | API key for the LLM provider |
| `SHANNON_MODEL` | Model name (e.g. `claude-sonnet-4-20250514`, `gpt-4o`) |
| `SHANNON_PROVIDER` | Provider: `anthropic`, `openai`, `ollama`, `custom` |
| `SHANNON_BASE_URL` | Custom API endpoint URL |
| `SHANNON_MAX_TOKENS` | Maximum output tokens |
| `SHANNON_TEMPERATURE` | Sampling temperature (0.0-1.0) |
| `SHANNON_PERMISSION_PROFILE` | Permission profile: `strict`, `balanced`, `permissive` |

Fallback: `ANTHROPIC_API_KEY` and `OPENAI_API_KEY` are auto-detected.

</details>

---

## Project Structure

```
shannon-agent/
├── crates/
│   ├── shannon-core/          # Core engine: API client, query engine, permissions, state
│   ├── shannon-tools/         # Tool implementations: file ops, git, search, notebook
│   ├── shannon-agents/        # Agent system: coordinator, dispatcher, executor
│   ├── shannon-ui/            # Terminal UI: REPL, widgets, rendering
│   ├── shannon-mcp/           # MCP protocol: transport, server, client, process pool
│   ├── shannon-commands/      # Slash commands: built-in command registry
│   ├── shannon-skills/        # Skills framework: discovery, loading, execution
│   ├── shannon-types/         # Shared type definitions
│   ├── shannon-tool-interface/# Tool trait definitions
│   ├── shannon-codegen/       # Code generation utilities
│   ├── shannon-cli/           # CLI entry point (shannon binary)
│   ├── shannon-agent/         # Out-of-process agent (JSON-RPC over stdin/stdout)
│   └── shannon-api-protocol/  # Wire protocol (serde types + TS codegen)
├── desktop/                   # Shannon Desktop (Tauri + React 19)
│   └── ui/                    # Frontend (React, Vite, Tailwind)
├── gateway/                   # Shannon Gateway (TypeScript platform bridge)
├── editors/vscode/            # VS Code extension
├── skills/                    # Bundled skill definitions
├── locales/                   # i18n translations (10 languages)
├── tests/scenarios/           # YAML declarative test scenarios
└── docs/                      # Documentation
```

---

## Developer Guide

Building from source for contributors and advanced users.

```bash
cargo build                        # Debug build
cargo check --workspace            # Fast type-check
just test                          # Run all tests (nextest)
just dev                           # check + lint + test (run before commits)
cargo clippy --workspace           # Lint
cargo fmt                          # Format
```

Install tooling: `cargo install just cargo-nextest`.

### Git hooks (pre-push checks)

One-time setup per clone:

```bash
git config core.hooksPath .githooks
```

This enables:
- **pre-commit**: auto-format staged `.rs` files with `cargo fmt`.
- **pre-push**: run `scripts/local-check.sh` — `cargo fmt --check`, `cargo build --workspace`, `cargo clippy`. Catches issues CI would reject (like the landlock rustdoc failure that blocked semver-checks in PR #60).

Bypass for WIP pushes: `git push --no-verify` or `PRE_PUSH_QUICK=1 git push` (fmt + build only, skip clippy).

### Testing

| Command | What | Needs API key? |
|---------|------|---------------|
| `just test` | All unit + mock tests | No |
| `just ci` | Full CI suite | No |
| `just scenarios` | YAML scenario tests | No |
| `just bench` | Criterion benchmarks | No |
| `just record` | Record real API fixtures | Yes |
| `just replay` | Replay recorded fixtures | No |

### Release Builds

```bash
./scripts/release.sh                      # Current platform
./scripts/release.sh --all                # All platforms
./scripts/release.sh --target x86_64-unknown-linux-gnu
```

Artifacts go to `target/dist/` as `.tar.gz` (Linux/macOS) or `.zip` (Windows).

---

## Reliability & Test Coverage

| Metric | Value |
|--------|-------|
| Total Rust code | ~282,000 lines |
| Source files | 355 |
| Total tests | **7,889** |
| Crates | 12 |
| Files with zero tests | **0** (every `src/**/*.rs` has at least one `#[test]`) |
| CI lint | `cargo clippy --workspace -- -D warnings` (zero warnings) |

Per-crate test counts:

| Crate | Tests | Responsibility |
|-------|-------|----------------|
| `shannon-core` | ~3,370 | API client, query engine, permissions, tools, state |
| `shannon-ui` | ~1,089 | Terminal UI, REPL, widgets, rendering |
| `shannon-tools` | ~1,111 | Tool implementations |
| `shannon-commands` | ~335 | Built-in commands |
| `shannon-agents` | ~471 | Multi-agent orchestration |
| `shannon-mcp` | ~373 | MCP server integration |
| `shannon-cli` | ~191 | CLI entry point |
| `shannon-skills` | ~171 | Skill system |
| Other crates | ~1,051 | Codegen, types, tool interface, agent, desktop |

---

## Binaries

- **`shannon`** — The main interactive CLI. Terminal REPL, streaming LLM responses, tool orchestration. This is what you run day-to-day.
- **`shannon-agent`** — Out-of-process agent worker (JSON-RPC over stdin/stdout). Used internally for multi-agent orchestration. Not typically run directly.

---

## License

[Apache License 2.0](LICENSE)

---

## Disclaimer

Shannon Code is an independent, clean-room reimplementation of AI-assisted coding tool concepts, built from publicly available documentation, open specifications (such as the [Model Context Protocol](https://modelcontextprotocol.io)), and general software engineering principles. Not affiliated with any other AI coding tool vendor. Intended for educational and research purposes.

---

<div align="center">

Built with Rust | [中文文档](./README.zh-CN.md)

</div>
