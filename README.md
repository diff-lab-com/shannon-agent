# Shannon

> **Note:** The unified `shannon` CLI replaces the former `shannon-code` product from earlier releases. Install paths, subcommands, and configuration are unchanged — only the binary name changed.

<div align="center">

**Open source. Total control. Keys never leave your machine.**

The open-source AI agent workspace — terminal, headless, server, and desktop,
one Rust engine, any LLM provider.

[![Rust](https://img.shields.io/badge/rust-1.88+-orange.svg)](https://www.rust-lang.org)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-see%20metrics.md-brightgreen.svg)](./docs/metrics.md)
<!-- metrics:start:badge -->[![Crates](https://img.shields.io/badge/crates-20-blue.svg)](./docs/metrics.md)<!-- metrics:end:badge -->

[English](#what-is-shannon) | [中文文档](./README.zh-CN.md) | [Documentation](https://shannon-agent.github.io/shannon-code/)

</div>

---

## What is Shannon?

Shannon is a fully open-source (Apache-2.0), Rust-based **AI agent workspace** that runs on your machine and works with **any LLM provider** — Anthropic, OpenAI, DeepSeek, Z.ai (GLM), Ollama, or any OpenAI-compatible endpoint. One engine, four surfaces: an interactive terminal UI, headless mode for scripts and CI, a local engine server, and a desktop app.

Two commitments shape every design decision:

### 1. Open source, total control

- **Every line auditable** — Apache-2.0, no black boxes. <!-- metrics:start:intro -->Every behavior is verified by **11,752 automated tests**.<!-- metrics:end:intro -->
- **Every agent action replayable** — sessions are event-sourced: each turn lands in an append-only `events.jsonl`, and `shannon trace show / replay / diff / export` lets you reconstruct exactly what happened, like a dashcam for your agents.
- **Every cost visible** — BYOK pay-per-use with session budget caps, context breakdown by category, cache hit-rate visibility, and no subscription quotas.
- **No vendor lock-in** — switch providers anytime; upstream price hikes and model retirements don't strand you. Claude Code ecosystem compatible: `CLAUDE.md`, `.claude/` agents, skills, hooks, and `.mcp.json` work out of the box.

### 2. Keys never leave your machine

- **Your API keys talk directly to the provider you choose** — no middleman server, no cloud-side credential pool. IM channel credentials live only in the OS keyring.
- **Outbound secret redaction** — the `secret-guard` plugin (built on the `shannon-plugin-api` content-transform contract) redacts secrets from outgoing messages before they reach the model, and restores them for local execution. Transforms are byte-stable, so your prompt cache keeps hitting.
- **OS-level sandboxing** — Landlock (Linux), macOS Seatbelt, and bubblewrap providers, plus a rule-based + LLM-assisted permission system with strict/balanced/permissive/custom profiles and per-action confirmation for high-risk tools.
- **Prompt-injection scanning and signature verification** for skills and MCP servers; webhook events are HMAC-SHA256 signed.
- **No telemetry by default** — and local voice input (whisper.rs) that never sends audio anywhere.

**How Shannon compares** (as of 2026-09; sources in [docs/competitive-research-2026-09.md](docs/competitive-research-2026-09.md)):

| | Shannon | Cloud subscription agents (Claude Code, Codex, Grok Bot) | Open-source peers (Hermes, Codex CLI, Grok Build) |
|---|---|---|---|
| License | Apache-2.0, fully open | Proprietary | Open source |
| Execution | Local-first, your machine | Cloud VMs / sandboxes | Local |
| Key & secret handling | OS keyring + outbound secret redaction + injection scanning | Vendor-managed cloud credential stores | Varies |
| LLM providers | Any (BYOK) | Single vendor | Multi / any |
| Cost model | Pay-per-use + budget caps + visible breakdown | Subscription quotas / credits | BYOK |
| Auditability | Event-sourced sessions, `trace` replay/diff | Varies, often black box | Varies |
<!-- metrics:start:diffrow -->| Test coverage | **11,752** tests across 20 workspace members | n/a (closed source) | Varies |<!-- metrics:end:diffrow -->
| Surfaces | Terminal + headless + server + desktop, one engine | Vary | Vary |

---

## One engine, four surfaces

One install, four ways in (every desktop installer bundles the `shannon` CLI too):

| Entry | What |
|---|---|
| `shannon` | Interactive TUI / REPL (default) |
| `shannon -p "..."` | Headless scripting — NDJSON streaming, `--schema` structured output |
| `shannon serve` | Engine daemon on `:33420` — the API surface gateway/mobile connect to |
| `shannon desktop` | Desktop app — `--install` downloads the platform bundle on demand |

Sessions are portable across surfaces: start in the terminal, continue on the desktop, approve from your phone.

### Shannon (Terminal)

The terminal-native coding agent: rich TUI with diff viewer and markdown rendering, tool orchestration, multi-agent teams with worktree isolation, MCP extensibility, and full replayability via `shannon trace`.

### Shannon Desktop

A native desktop workspace built on **Tauri 2 + React 19 — not Electron**. Two modes for two audiences:

- **Simple mode** — for everyone: chat with inline tool calls you can approve or revoke one by one, drag-and-drop attachments, voice input (cloud or fully local), scheduled tasks with calendar and dependency views, and a triage inbox for everything your agents did while you were away.
- **Advanced mode** — for developers: Connectors (MCP servers, skills, agents), multi-panel workspace with integrated terminal, git worktree management, memory graph, and Mission Control multi-agent orchestration.

Plus: mobile pairing (scan a QR code to dispatch and approve tasks from your phone), IM channels (Telegram / Discord / Slack / 飞书 / 钉钉), system tray, global shortcuts, auto-update, and 8 themes.

---

## Features

### Multi-Provider LLM Support

Connect to any LLM with a single config file — your key, your provider, direct connection:

| Provider | Models | Setup |
|----------|--------|-------|
| Anthropic | Claude Sonnet / Opus / Haiku families | `provider = "anthropic"` |
| OpenAI | GPT-4o and newer | `provider = "openai"` |
| Ollama | Llama, Mistral, Qwen, etc. (local) | `provider = "ollama"` (auto-detect) |
| DeepSeek | DeepSeek Chat / Coder | `provider = "openai"` + `base_url` |
| Z.ai (GLM) | GLM family | `provider = "openai"` + `base_url` |
| Any OpenAI-compatible | Any model | `provider = "openai"` + `base_url` |

Anthropic prompt caching is supported with three-layer cache breakpoint injection for maximum efficiency.

### Goals & Autonomous Tasks

Hand your agent an objective, not just a prompt:

- **`/goal`** — persistent goals with automatic resumption; the engine keeps working across context compaction, detects `GOAL_COMPLETE` / `GOAL_BLOCKED`, and backs off (30m → 1h → 2h) on blocked retries
- **Budget caps** — `--budget $N` hard limits spending; anti-spin and stall-strike guards stop runaway loops
- **`/loop` / `/ralph`** — autonomous iteration loops sharing the same guards
- **Triage inbox** — results and blockers land in the desktop triage page; continue in the original session with one click

### Automation & Triggers

- **Routines** — cron-scheduled, one-shot, and event-triggered tasks
- **API endpoint triggers** — `shannon serve` exposes a per-routine trigger URL with HMAC-SHA256 verification ("point your Slack alert at your agent")
- **GitHub event triggers** — react to issues and CI events
- **IM channels** — Telegram / Discord / Slack / 飞书 / 钉钉 inbound: DMs answer directly, group chats respond to @mentions; progress and results push back to the original chat
- **Mobile dispatch** — pair by QR code, dispatch and approve tasks from your phone

### Multi-Agent Orchestration

- **Team coordination** — `TeamCreate`, `SendMessage`, task assignment and tracking
- **Worktree isolation** — each agent works in its own git worktree
- **Per-agent config** — override model, tools, and working directory per agent
- **`/batch` best-of-N** — decompose a task, spawn parallel worktree-isolated attempts, compare diffs side by side, adopt the winner
- **Agent dashboard** — real-time status in TUI and desktop

### Tool System

- **File operations** — Read, Edit, Write, MultiEdit with three-way merge and conflict resolution
- **Code analysis** — Syntax highlighting, symbol navigation (LSP), diff rendering, repository symbol map
- **Git integration** — Status, diff, log, commit, branch management
- **Command execution** — Sandboxed Bash with streaming output and timeout control, plus background process tools
- **Web & browser** — Web search, local browser automation (drive your installed browser; no bundled binaries)
- **Computer use** — Screenshot understanding with vision models, controlled input injection, per-action confirmation; AppleScript/Shortcuts tools on macOS
- **Image & document analysis** — Screenshot understanding, batch image analysis, PDF text extraction
- **Notebook editing** — Jupyter notebook cell read/edit/insert/delete

### MCP (Model Context Protocol)

Full MCP implementation compatible with Claude Code's MCP ecosystem:

- **Transports**: stdio, SSE, streamable HTTP
- **Tool discovery**: `tools/list` with deferred schema loading — scales to 100+ tools
- **Fuzzy search**: `mcp__tool_search` for finding tools by name or description
- **Resource management**: Subscribe to resource updates, handle notifications
- **Webhook support**: HMAC-SHA256 signed events with retry and persistence
- **Configuration**: `.mcp.json` (project-level) or `~/.claude/settings.json`
- **SaaS integrations**: GitHub, Slack, Jira, Notion, Linear MCP servers included

### Session, Context & Memory

- **Event-sourced sessions** — every turn lands in an append-only `events.jsonl` per session; resume, search, replay, and diff are all projections of that single authoritative log (`shannon trace show / replay / diff / export`)
- **Context compression** — auto-compact, micro-compact, conversation phase tracking, token budget watchdog (`SHANNON_TOKEN_BUDGET`)
- **Memory system** — persistent memory with provenance, desktop memory page, auto-extraction and consolidation
- **Checkpoint/Undo** — Git-based file checkpointing with diff preview before revert (`/rewind`)
- **Plan mode** — Structured planning with approval workflows

### Plugin & Skill System

- **`shannon-plugin-api`** — a content-transform middleware contract between engine and plugins, with four invariants: byte-stable determinism (prompt-cache safe), one-way flow, idempotence, and explicit failure semantics. The built-in `secret-guard` plugin is the first implementation.
- **Plugin discovery** — load from `.shannon/plugins/` with manifest parsing
- **Command plugins** — register as slash commands in the REPL
- **Skill plugins** — prompt templates triggered by slash commands, compatible with `.claude/skills/*/SKILL.md`
- **Hook system** — 32+ events (tool execution, compaction, config changes, agent lifecycle)

### Remote Targets (SSH / Docker)

Run the entire toolchain on a remote machine or inside a container:

- **SSH hosts** — reuse `~/.ssh/config` (aliases, agent, ProxyJump); files go over SFTP, commands over the multiplexed ssh connection. First-connect trust uses the standard known_hosts TOFU flow.
- **Docker containers** — attach to a running container (`docker exec`); optionally through an SSH hop (`ssh_target`) for remote daemons.
- **Management** — `/remote` in the TUI, `--target <name>` headless, or Settings → Remotes in the desktop app. Targets live in `~/.shannon/remotes.toml` (no credentials stored; system ssh owns auth).

```bash
/remote use build-box          # TUI: switch this session to a target
shannon --target build-box -p "run the test suite"   # headless
```

### Internationalization

- 10 languages: English, Chinese, Hindi, Spanish, French, Arabic, Bengali, Portuguese, Russian, Japanese
- Community-contributable locale files in `locales/` directory
- UI language switchable at runtime

---

## Security & Privacy

Shannon is local-first: state lives in `~/.shannon/`, and nothing leaves your machine except the model API calls you configure.

| Layer | What it does |
|---|---|
| **Credentials** | Provider API keys stay on your machine and talk directly to the provider you choose. IM channel and integration credentials live in the OS keyring. No middleman server ever holds your keys. |
| **Secret redaction** | The `secret-guard` plugin (via `shannon-plugin-api`) redacts secrets from outbound messages before they reach the model and restores them for local tool execution — byte-stable, so prompt caching keeps working. Session-level redaction policies via `~/.shannon/redaction.toml`. |
| **Sandboxing** | Landlock (Linux), macOS Seatbelt, and bubblewrap providers; manifest-driven sandbox enforcement for file writes; experimental `/sandbox` flag. |
| **Permissions** | Rule-based classifier + LLM-assisted classification (confidence < 0.7 falls back), strict/balanced/permissive/custom profiles, 4-tier precedence, interactive approval for risky operations, per-action confirmation for high-risk tools (computer use, AppleScript). |
| **Supply chain** | Prompt-injection scanning and signature verification for skills and MCP servers; `cargo-deny` and `cargo-semver-checks` gates in CI. |
| **Telemetry** | None by default; any usage signal is strictly opt-in. Local voice input (whisper.rs) sends audio nowhere. |

---

## Quick Start

### 1. Install

Download the latest release for your platform:

```bash
# Linux / macOS — one line, detects platform (CLI + gateway + desktop)
curl -fsSL https://github.com/diff-lab-com/shannon-agent/releases/latest/download/install.sh | sh

# Server / headless — CLI only, no sudo
curl -fsSL https://github.com/diff-lab-com/shannon-agent/releases/latest/download/install.sh | SHANNON_COMPONENTS=cli sh

# Or with cargo (requires Rust 1.88+)
cargo install --git https://github.com/diff-lab-com/shannon-agent.git
```

<details>
<summary>Other platforms</summary>

- **Windows**: `irm https://github.com/diff-lab-com/shannon-agent/releases/latest/download/install.ps1 | iex`
  (or download `.zip` from [Releases](https://github.com/diff-lab-com/shannon-agent/releases))
- **From source**: See [Developer Guide](#developer-guide) below

</details>

### 2. Configure

Set your API key and preferred model — the key stays on your machine and is used to talk directly to your chosen provider:

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
shannon desktop                   # Or launch the desktop app
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
shannon --goal "make CI green" --budget 5       # Autonomous goal with spending cap
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
| `/batch` | Parallel worktree-isolated PR creation (best-of-N) |
| `/team` | Manage agent teams |
| `/goal` | Set a persistent, self-resuming goal |
| `/remote` | Connect SSH hosts / Docker containers as execution targets |
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
| `SHANNON_TOKEN_BUDGET` | Session token budget watchdog |

Fallback: `ANTHROPIC_API_KEY` and `OPENAI_API_KEY` are auto-detected.

</details>

---

## Project Structure

```
shannon-agent/
├── crates/
│   ├── shannon-core/          # Core engine: state, sessions, memory, permissions, secret guard
│   ├── shannon-engine/        # LLM API clients, streaming, compaction/context budget
│   ├── shannon-tools/         # Tool implementations: file ops, git, browser, computer use
│   ├── shannon-ui/            # Terminal UI: REPL, widgets, rendering
│   ├── shannon-agents/        # Multi-agent: teams, worktree isolation
│   ├── shannon-mcp/           # MCP protocol: transport, server, client, process pool
│   ├── shannon-mcp-saas/      # SaaS MCP servers (GitHub, Slack, Jira, Notion, Linear)
│   ├── shannon-commands/      # Slash commands: built-in command registry
│   ├── shannon-skills/        # Skills framework: discovery, loading, execution
│   ├── shannon-plugin-api/    # Plugin content-transform contract (secret-guard)
│   ├── shannon-server/        # HTTP API server (shannon serve)
│   ├── shannon-remote/        # Remote execution worlds (SSH hosts, Docker)
│   ├── shannon-repomap/       # Repository symbol map (tree-sitter)
│   ├── shannon-cli/           # CLI entry point (shannon binary)
│   ├── shannon-agent/         # Out-of-process agent (JSON-RPC over stdin/stdout)
│   ├── shannon-api-protocol/  # Wire protocol (serde types + TS codegen)
│   ├── shannon-types/         # Shared type definitions
│   ├── shannon-tool-interface/# Tool trait definitions
│   ├── shannon-codegen/       # Code generation utilities
│   └── shannon-stability-attr/# Stability attribute macros
├── desktop/                   # Shannon Desktop (Tauri 2 + React 19)
│   └── ui/                    # Frontend (React, Vite, Tailwind)
├── gateway/                   # Shannon Gateway (TypeScript platform bridge)
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
- **pre-push**: run `scripts/local-check.sh` — `cargo fmt --check`, `cargo build --workspace`, `cargo clippy`.

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

<!-- metrics:start:table -->
| Metric | Value |
|--------|-------|
| Total Rust code | 418,458 lines |
| Source files | 624 |
| Total tests (nextest, runnable) | **11,752** |
| Crates (workspace members) | 20 (19 crates + desktop) |
| Crates with zero tests | 2 (`shannon-server`, `shannon-stability-attr`) |
| CI lint | `cargo clippy --workspace -- -D warnings` (zero warnings) |
<!-- metrics:end:table -->

Per-crate test counts:

<!-- metrics:start:crates -->
| Crate | Tests | Responsibility |
|-------|-------|----------------|
| `shannon-core` | 3,766 | API client, query engine, permissions, tools, state |
| `shannon-tools` | 1,630 | Tool implementations: file ops, git, search, notebook |
| `shannon-ui` | 1,497 | Terminal UI, REPL, widgets, rendering |
| `shannon-engine` | 1,113 | LLM API client, streaming, compaction/context budget, permissions |
| `shannon-agents` | 897 | Multi-agent coordination: teams, worktree isolation |
| `shannon-desktop` | 599 | Tauri desktop app shell and commands |
| `shannon-mcp` | 578 | MCP protocol: transport, server, client, process pool |
| `shannon-cli` | 486 | CLI entry point (`shannon` binary) |
| `shannon-commands` | 416 | Built-in slash commands |
| `shannon-mcp-saas` | 185 | SaaS MCP servers (GitHub, Slack, Jira, Notion, Linear) |
| `shannon-skills` | 172 | Skills framework: discovery, loading, execution |
| `shannon-codegen` | 100 | Code generation utilities |
| `shannon-types` | 84 | Shared type definitions |
| `shannon-agent` | 65 | Out-of-process agent (JSON-RPC over stdin/stdout) |
| `shannon-remote` | 55 | Remote execution worlds (SSH hosts, Docker) |
| `shannon-tool-interface` | 42 | Tool trait definitions |
| `shannon-api-protocol` | 37 | Wire protocol (serde types + TS codegen) |
| `shannon-repomap` | 30 | Repository symbol map for LLM context (tree-sitter) |
| `shannon-server` | 0 | HTTP API server (`shannon serve`) |
| `shannon-stability-attr` | 0 | Stability attribute macros |
<!-- metrics:end:crates -->

---

## Binaries

- **`shannon`** — The main interactive CLI. Terminal REPL, streaming LLM responses, tool orchestration. This is what you run day-to-day.
- **`shannon-agent`** — Out-of-process agent worker (JSON-RPC over stdin/stdout). Used internally for multi-agent orchestration. Not typically run directly.

---

## Documentation

- **User & developer docs**: [shannon-agent.github.io/shannon-code](https://shannon-agent.github.io/shannon-code/)
- **Security & privacy**: see [Security & Privacy](#security--privacy) above and the docs site
- **Contributing**: [CONTRIBUTING.md](CONTRIBUTING.md) · [Security policy](SECURITY.md)

---

## License

[Apache License 2.0](LICENSE)

---

## Disclaimer

Shannon is an independent, clean-room reimplementation of AI-assisted coding tool concepts, built from publicly available documentation, open specifications (such as the [Model Context Protocol](https://modelcontextprotocol.io)), and general software engineering principles. Not affiliated with any other AI coding tool vendor. Intended for educational and research purposes.

---

<div align="center">

Built with Rust | [中文文档](./README.zh-CN.md)

</div>
