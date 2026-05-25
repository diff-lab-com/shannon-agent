# Shannon Code

<div align="center">

**A high-performance, open-source AI-assisted coding tool, written in Rust**

[![Rust](https://img.shields.io/badge/rust-1.88+-orange.svg)](https://www.rust-lang.org)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-8516-brightgreen.svg)
[![Lines](https://img.shields.io/badge/code-300K-blue.svg)

[English](#what-is-shannon-code) | [中文文档](#中文文档) | [Documentation](https://shannon-agent.github.io/shannon-code/)

</div>

---

## Disclaimer

**Shannon Code is an independent, clean-room reimplementation of AI-assisted coding tool concepts.** This project was built from the ground up using only publicly available documentation, open specifications (such as the [Model Context Protocol](https://modelcontextprotocol.io)), and general software engineering principles.

- No proprietary source code, binaries, or confidential materials were accessed or used in the development of this project.
- All code, architecture, and design decisions are original work.
- Any functional similarities to other products are the result of implementing open standards and common software patterns, not copying.
- This project is **not affiliated with, endorsed by, or connected to** any other AI coding tool vendor.
- The developers of Shannon Code assume **no legal liability** for how this software is used.

This project is intended solely for educational and research purposes.

---

## Why Open Source Matters for AI Coding Tools

Closed-source AI coding tools create fundamental problems of trust, cost transparency, and user autonomy. Recent reverse-engineering analyses have revealed multiple issues that directly harm users:

### Cache Destruction Causes 10-20x Cost Inflation

Anthropic's Claude Code injects dynamic billing headers (`x-anthropic-billing-header` with a random `cch` value) directly into the system prompt. Because cache matching is prefix-based, this volatile element **invalidates the entire prompt cache on every request**. For a typical 68,000-token system prompt:

| Metric | Header ON | Header OFF | Impact |
|--------|-----------|------------|--------|
| Cache Read | 512 tokens | 68,096 tokens | **133x less efficient** |
| Cost per turn | $0.204 | $0.021 | **90% more expensive** |
| API Latency | 17.5s | 2.1s | **8x slower** |

> Source: [Claude Code 调用国内模型的缓存大坑](https://blog.deepai.wiki/posts/claude-code-cache-pitfall/)

The `cc_version` suffix uses a SHA256 hash derived from the user's first message, preventing cross-session cache sharing. Third-party API users see cache hit rates drop from **90% to under 20%**.

### Anti-Distillation Mechanisms Harm Legitimate Users

A leaked source code analysis (March 31, 2026 — ~510,000 lines of TypeScript published via npm build misconfiguration) revealed the `ANTI_DISTILLATION_CC` feature flag, which silently injects fake tool definitions into the system prompt to thwart competitor model training. These fake tools vary between requests, **destroying cache performance** as a side effect.

> Source: [Why Claude Code Burns Through Tokens So Fast](https://smartscope.blog/en/blog/claude-code-token-consumption-cache-bug/)

### Seven Multiplicative Bugs Create a "Death Spiral"

Reverse-engineering of Claude Code's compiled `cli.js` identified seven cache-related bugs that stack **multiplicatively, not additively**. The most severe: when Extra Usage (overage billing) mode is detected, the client **silently downgrades cache TTL from 1 hour to 5 minutes** without notification. A Max 20x subscriber burned through **43% of a week's token quota in a single day**.

| Bug | Impact | Status |
|-----|--------|--------|
| Native installer cache corruption | Every request cache miss | Workaround: use npm install |
| Session resume attachment loss | Full cache miss on resume | Fixed after 28 days, 20 versions |
| Compaction infinite retry | 1,279 sessions with 50+ failures | Fixed |
| Tool output truncation | Corrupts cache prefix | Unfixed |
| Synthetic rate limit errors | Fake errors, no API call made | Unfixed |
| Server-side tool result deletion | Breaks cache silently | Unfixed |
| Cache TTL silent downgrade (1hr → 5min) | 1.8x more expensive | Unfixed |

> Source: [Claude Code 偷偷烧钱？逆向工程揭露 7 个叠加 Bug](https://www.cnblogs.com/jeecg158/p/19831682)

### Third-Party API Lock-In

After upgrading to Claude Code v2.1.37, users routing to third-party Anthropic-compatible upstreams saw cache hit rates drop from **above 90% to 30-40%, sometimes near 0%**. The volatile `cch=<random>` in the billing header — embedded in the system prompt, not an HTTP header — is the cause.

> Source: [Bug: cch random value causes severe prompt-cache miss on third-party upstreams](https://github.com/router-for-me/CLIProxyAPI/issues/1592)

### Quality Assurance Gap

The leaked source code contained **510,000 lines of TypeScript** with **zero tests for 64,464 lines of code**. A single function spanned 3,167 lines with 486 branch points. Issues that persisted for weeks under closed-source conditions were identified within hours once the code became public.

### Shannon Code's Answer

| Problem | Closed-Source Tool | Shannon Code |
|---------|--------------------|--------------|
| Cache destruction | Dynamic billing headers break cache | No hidden injections; cache-friendly prompts |
| Cost transparency | Bugs inflate costs 10-20x | 8,516 tests verify behavior; every line auditable |
| User lock-in | Third-party APIs degraded | Multi-provider: Anthropic, OpenAI, Ollama, any OpenAI-compatible endpoint |
| Quality assurance | 0 tests for 64K+ lines | Every source file has `#[test]`; test-to-code ratio verified |
| Silent degradation | Cache TTL downgraded without notice | All behavior visible in source code |
| Vendor control | `ANTI_DISTILLATION_CC`, attestation data | No anti-user mechanisms |

**Further Reading:**
- [Claude Code Cache Crisis: A Complete Reverse-Engineering Analysis](https://medium.com/@marianski.jacek/claude-code-cache-crisis-a-complete-reverse-engineering-analysis-9a6f4e03fae4)

---

## What is Shannon Code?

Shannon Code is a feature-rich, type-safe AI-assisted coding tool written entirely in Rust. It provides a terminal-based REPL interface for interacting with large language models (LLMs) while offering advanced capabilities like tool orchestration, session management, plugin systems, and MCP (Model Context Protocol) support.

### Why Rust?

- **Memory safety** — guaranteed at compile time, no data races
- **High performance** — zero-cost abstractions, near-C speed
- **Type safety** — a strong type system catches bugs before runtime
- **Concurrency** — native `async/await` support for parallel operations
- **Cross-platform** — compile once, run on Linux, macOS, and Windows

---

## Binaries

Shannon Code ships two binaries:

- **`shannon`** — The main interactive CLI. Provides the terminal REPL, processes user input, streams LLM responses, and orchestrates tool calls. This is what you run day-to-day.
- **`shannon-agent`** — An out-of-process agent worker. Communicates via JSON-RPC over stdin/stdout. Used internally by `shannon` for multi-agent orchestration — when the main process needs to dispatch work to a separate agent (e.g., parallel research, code review), it spawns `shannon-agent` as a child process. You don't typically run this directly.

---

## Features

### AI-Powered Coding
- Streaming query processing with real-time output
- Multi-turn conversation with context management
- Code generation, refactoring, and optimization suggestions
- Automated test generation

### Tool System
- **File operations**: read, edit, create, notebook editing
- **Code analysis**: syntax highlighting, symbol navigation, diff rendering
- **Git integration**: version control, branch management, commit operations
- **Command execution**: sandboxed shell command running
- **Image support**: screenshot analysis, visual understanding
- **Web search**: real-time information retrieval

### MCP (Model Context Protocol)
- Full MCP protocol implementation (stdio, SSE, streamable HTTP)
- Dynamic tool registration from MCP servers
- Deferred schema loading with fuzzy search — scales to 100+ MCP tools without bloating context
- Resource management and subscription support
- Elicitation and completion support

### Advanced Capabilities
- **Multi-Agent orchestration**: parallel agent dispatch, team coordination, task delegation, per-agent model/tool config
- **Permission system**: rule-based classification, LLM-powered auto-classifier, dangerous pattern detection, approval workflows
- **Session management**: persistence, history, search, resume by ID
- **Plugin system**: discovery, loading, lifecycle management, marketplace
- **Skills & Commands**: extensible skill framework, bundled implementations
- **Context compression**: advanced strategies with auto-compact, micro-compact, session memory
- **Hooks system**: 32+ hook events for tool execution, compaction, config changes
- **Memory system**: persistent memory store, auto-dream extraction, consolidation
- **LSP integration**: 6 LSP tools, automatic background `cargo check` diagnostics
- **Plan mode**: structured planning with approval workflows
- **Worktree support**: isolated git worktrees for parallel development, `/batch` for parallel PRs
- **Checkpoint/Undo**: git-based file checkpointing with diff preview before revert
- **Auto-updater**: GitHub Releases-based update checking
- **Diagnostics & Doctor**: environment health checks, error pattern analysis
- **Voice mode**: voice input/output with keyword spotting
- **OAuth**: token management with encryption
- **Team memory sync**: bidirectional sync with secret scanning
- **Streaming tool executor**: concurrent tool execution with progress tracking

### Terminal UI
- Interactive REPL with command history and search
- Markdown rendering with syntax highlighting
- Diff visualization with colored output, collapsible thinking, tool grouping
- Token counter, context window bar, cost tracking, cache stats
- Virtual scroll and progress indicators

### Internationalization (i18n)
- Multi-language UI support via `rust-i18n`
- 10 languages: English, Chinese, Hindi, Spanish, French, Arabic, Bengali, Portuguese, Russian, Japanese
- Community-contributable locale files in `locales/` directory

---

## Quick Start

### Prerequisites

- **Rust** 1.88+ (edition 2024)
- **Operating System**: Linux / macOS / Windows
- **Memory**: 4 GB+ recommended
- **API Key**: Any OpenAI-compatible API key (Anthropic, OpenAI, Ollama, etc.)

### Build

```bash
# Clone the repository
git clone https://github.com/shannon-agent/shannon-code.git
cd shannon-code

# Build in release mode
cargo build --release

# The binary is at target/release/shannon
```

### Configure

Shannon Code supports multiple LLM providers. Configuration priority: CLI args > environment variables (`SHANNON_*`) > `.shannon.toml` (project-local) > `~/.shannon/config.toml` (global).

#### Option 1: Anthropic (direct)

```bash
mkdir -p ~/.shannon
cat > ~/.shannon/config.toml << 'EOF'
provider = "anthropic"
api_key = "sk-ant-..."
model = "claude-sonnet-4-20250514"
max_tokens = 8192
EOF
```

#### Option 2: OpenAI-compatible endpoint (any provider)

```bash
mkdir -p ~/.shannon
cat > ~/.shannon/config.toml << 'EOF'
provider = "openai"
model = "gpt-4o"
api_key = "sk-..."
base_url = "https://api.openai.com/v1"
max_tokens = 8192
EOF
```

#### Option 3: Local model via Ollama

```bash
# Start Ollama first
ollama serve

# Shannon auto-detects Ollama on localhost:11434
export SHANNON_MODEL="llama3"
shannon
```

#### Option 4: Environment variables

```bash
export SHANNON_API_KEY="sk-ant-..."
export SHANNON_MODEL="claude-sonnet-4-20250514"
shannon
```

#### Available environment variables

| Variable | Description |
|----------|-------------|
| `SHANNON_API_KEY` | API key for the LLM provider |
| `SHANNON_MODEL` | Model name (e.g., `claude-sonnet-4-20250514`, `gpt-4o`) |
| `SHANNON_PROVIDER` | Provider: `anthropic`, `openai`, `ollama`, `custom` |
| `SHANNON_BASE_URL` | Custom API endpoint URL |
| `SHANNON_MAX_TOKENS` | Maximum output tokens |
| `SHANNON_TEMPERATURE` | Sampling temperature (0.0–1.0) |
| `SHANNON_TIMEOUT` | Request timeout in seconds |
| `SHANNON_DEBUG` | Enable debug logging |
| `SHANNON_PERMISSION_PROFILE` | Permission profile: `strict`, `balanced`, `permissive` |

Fallback: `ANTHROPIC_API_KEY` and `OPENAI_API_KEY` are also detected automatically.

#### MCP Server Configuration

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

### Run

```bash
# Start Shannon Code (interactive REPL)
shannon

# Start in a specific project directory
shannon /path/to/project

# Non-interactive / CI mode
shannon --prompt "Explain the auth module" --output-format json

# Resume most recent session
shannon --resume

# Resume specific session
shannon --resume <session-uuid>

# Continue most recent session (alias for --resume)
shannon --continue

# Structured output with JSON Schema validation
shannon --prompt "List all TODOs" --schema schema.json

# Pipe mode
echo "fix this bug" | shannon --pipe

# Limit tool access in CI
shannon --prompt "refactor" --allowed-tools Read,Edit,Bash,Grep --max-turns 10

# Only output diff (for automated workflows)
shannon --prompt "fix lint" --diff-only
```

### Key Commands (inside REPL)

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/undo list` | List file checkpoints |
| `/undo <n>` | Preview and revert to checkpoint n |
| `/rewind` | Rewind conversation and/or code |
| `/models` | Switch LLM model |
| `/compact` | Compress conversation context |
| `/diff` | Show file diff viewer |
| `/batch` | Parallel worktree-isolated PR creation |
| `/config` | View/edit configuration |
| `/cost` | Show token usage and cost |
| `/search` | Search conversation history |

---

## Project Structure

```
shannon-code/
├── crates/
│   ├── shannon-core/          # Core engine: query processing, tools, permissions, state
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
│   └── shannon-agent/         # Out-of-process agent (JSON-RPC over stdin/stdout)
├── skills/                    # Bundled skill definitions
├── locales/                   # i18n translation files (en, zh, hi, es, fr, ar, bn, pt, ru, ja)
├── scripts/
│   └── release.sh             # Cross-platform release script
├── Cargo.toml                 # Workspace configuration
├── Cargo.lock                 # Dependency lock file
├── LICENSE
└── README.md
```

---

## Development

```bash
# Debug build
cargo build

# Run all tests (single-threaded to avoid env contention)
cargo test --workspace -- --test-threads=1

# Run a specific module's tests
cargo test -p shannon-core -- tool_execution

# Check compilation without building
cargo check --workspace

# Format code
cargo fmt

# Lint (production code only)
cargo clippy --workspace -- -D warnings
```

### Adding a New Crate

```bash
cd crates
cargo new --lib shannon-new-feature
```

Then add it to the workspace in the root `Cargo.toml`.

### Release Builds

```bash
# Build for current platform
./scripts/release.sh

# Build for all platforms (requires cross-rs or docker)
./scripts/release.sh --all

# Build for a specific target
./scripts/release.sh --target x86_64-unknown-linux-gnu

# Override version string
./scripts/release.sh --version 0.2.0
```

Artifacts are placed in `target/dist/` as `.tar.gz` (Linux/macOS) or `.zip` (Windows).

---

## Reliability & Test Coverage

| Metric | Value |
|--------|-------|
| Total Rust code | ~300,000 lines |
| Total tests | **8,516** |
| Crates | 12 |
| Files with zero tests | **0** (every `src/**/*.rs` has at least one `#[test]`) |
| CI lint | `cargo clippy --workspace -- -D warnings` (zero warnings) |
| Thread safety | Tests run with `--test-threads=1` to avoid env contention |

Every source file in every crate has test coverage. Integration tests use `mockito` for HTTP mocking — never hit real APIs. Test counts per major crate:

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

## Statistics

| Metric | Value |
|--------|-------|
| Total lines of Rust code | ~300,000 |
| Total tests | 8,516 |
| Number of crates | 12 |
| Supported languages (UI) | 10 (en, zh, hi, es, fr, ar, bn, pt, ru, ja) |
| Supported platforms | Linux, macOS, Windows |
| LLM providers | Anthropic, OpenAI, Ollama, any OpenAI-compatible |
| MCP transport types | stdio, SSE, streamable HTTP |

---

## License

This project is released under the [Apache License 2.0](LICENSE).

## Documentation

Full documentation is available at [shannon-agent.github.io/shannon-code](https://shannon-agent.github.io/shannon-code/).

### Acknowledgments

This project was inspired by publicly available AI-assisted coding tools and open specifications. No proprietary materials were used in its creation.

---

<div align="center">

Built with Rust

[Back to top](#shannon-code)

</div>

---

<a id="中文文档"></a>

# 中文文档

## 免责声明

**Shannon Code 是一个独立的、基于净室（clean room）方法重新实现的 AI 辅助编程工具。** 本项目完全从零开始构建，仅参考了公开文档、开放规范（如 [Model Context Protocol](https://modelcontextprotocol.io)）和通用软件工程原则。

- 开发过程中**未访问、未使用**任何专有源代码、二进制文件或机密材料
- 所有代码、架构和设计决策均为原创
- 与其他产品的功能相似性来自对开放标准和通用软件模式的实现，而非复制
- 本项目**不隶属于、不受认可、也不与**任何其他 AI 编程工具供应商关联
- Shannon Code 的开发者**不承担任何法律责任**

本项目仅用于教育和研究目的。

---

## 为什么开源 AI 编程工具至关重要

闭源 AI 编程工具带来了信任、成本透明度和用户自主权方面的根本性问题。近期的逆向工程分析揭示了多个直接损害用户利益的问题：

### 缓存破坏导致成本膨胀 10-20 倍

某闭源工具将动态计费头（`x-anthropic-billing-header`，包含随机 `cch` 值）直接注入系统提示词。由于缓存匹配基于前缀，这个易变元素**在每次请求时使整个提示缓存失效**。对于典型的 68,000 token 系统提示：

| 指标 | 计费头开启 | 计费头关闭 | 影响 |
|------|-----------|------------|------|
| 缓存读取 | 512 tokens | 68,096 tokens | **效率降低 133 倍** |
| 每轮成本 | $0.204 | $0.021 | **贵 90%** |
| API 延迟 | 17.5s | 2.1s | **慢 8 倍** |

> 来源：[Claude Code 调用国内模型的缓存大坑：一个环境变量省 90% 的钱](https://blog.deepai.wiki/posts/claude-code-cache-pitfall/)

`cc_version` 后缀使用从用户首条消息派生的 SHA256 哈希，阻止跨会话缓存共享。第三方 API 用户缓存命中率从 **90% 降至 20% 以下**。

### 反蒸馏机制损害合法用户

泄露的源代码分析（2026 年 3 月 31 日——通过 npm 构建配置错误发布了约 51 万行 TypeScript）揭示了 `ANTI_DISTILLATION_CC` 功能标志，它静默地向系统提示注入虚假工具定义以阻止竞争对手模型训练。这些虚假工具在请求之间变化，**附带破坏缓存性能**。

> 来源：[Why Claude Code Burns Through Tokens So Fast — 3 Causes and the Cache Bug Confirmed by a Source Code Leak](https://smartscope.blog/en/blog/claude-code-token-consumption-cache-bug/)

### 七个叠加 Bug 形成"死亡螺旋"

对编译后 `cli.js` 的逆向工程发现了七个缓存相关 Bug，它们**乘法叠加**而非简单相加。最严重的：当检测到超额计费模式时，客户端**静默将缓存 TTL 从 1 小时降级为 5 分钟**，且不通知用户。一名 Max 20x 订阅用户在**一天内耗尽了 43% 的周配额**。

| Bug | 影响 | 状态 |
|-----|------|------|
| 原生安装包缓存损坏 | 每次请求缓存未命中 | 变通：使用 npm 安装 |
| 会话恢复附件丢失 | 恢复时完全缓存未命中 | 28 天 20 个版本后修复 |
| 压缩无限重试 | 1,279 个会话连续失败 50+ 次 | 已修复 |
| 工具输出截断 | 损坏缓存前缀 | 未修复 |
| 合成限速错误 | 伪造错误，未发起 API 调用 | 未修复 |
| 服务端静默删除工具结果 | 静默破坏缓存 | 未修复 |
| 缓存 TTL 静默降级（1小时→5分钟） | 成本增加 1.8 倍 | 未修复 |

> 来源：[Claude Code 偷偷烧钱？逆向工程揭露 7 个叠加 Bug，Max 20x 一天耗尽 43% 周配额](https://www.cnblogs/jeecg158/p/19831682)

### 第三方 API 锁定

升级到 v2.1.37 后，路由到第三方兼容上游的用户缓存命中率从 **90% 以上降至 30-40%，有时接近 0%**。原因：嵌入系统提示（而非 HTTP 头）中的易变 `cch=<random>` 计费头。

> 来源：[Bug: cch random value causes severe prompt-cache miss on third-party upstreams](https://github.com/router-for-me/CLIProxyAPI/issues/1592)

### 质量保证缺口

泄露的源代码包含 **51 万行 TypeScript**，其中 **64,464 行代码零测试**。单个函数跨越 3,167 行，包含 486 个分支点。在闭源条件下持续数周的问题，一旦代码公开，数小时内即被识别。

### Shannon Code 的回答

| 问题 | 闭源工具 | Shannon Code |
|------|----------|-------------|
| 缓存破坏 | 动态计费头破坏缓存 | 无隐藏注入；缓存友好提示 |
| 成本透明度 | Bug 使成本膨胀 10-20 倍 | 8,516 个测试验证行为；每行代码可审计 |
| 用户锁定 | 第三方 API 被降级 | 多提供商：Anthropic、OpenAI、Ollama、任何 OpenAI 兼容端点 |
| 质量保证 | 64K+ 行零测试 | 每个源文件都有 `#[test]`；测试覆盖率验证 |
| 静默降级 | 缓存 TTL 被降级不通知 | 所有行为在源代码中可见 |
| 供应商控制 | `ANTI_DISTILLATION_CC`、认证数据 | 无反用户机制 |

**延伸阅读：**
- [Claude Code Cache Crisis: A Complete Reverse-Engineering Analysis](https://medium.com/@marianski.jacek/claude-code-cache-crisis-a-complete-reverse-engineering-analysis-9a6f4e03fae4)

---

## 什么是 Shannon Code？

Shannon Code 是一个功能丰富、类型安全的 AI 辅助编程工具，完全使用 Rust 编写。它提供基于终端的 REPL 界面，支持与大语言模型交互，同时提供工具编排、会话管理、插件系统和 MCP（模型上下文协议）支持等高级功能。

## 特性

### AI 辅助编程
- 流式查询处理与实时输出
- 多轮对话与上下文管理
- 代码生成、重构和优化建议
- 自动测试生成

### 工具系统
- **文件操作**：读取、编辑、创建、Notebook 编辑
- **代码分析**：语法高亮、符号导航、Diff 渲染
- **Git 集成**：版本控制、分支管理、提交操作
- **命令执行**：沙箱化的 Shell 命令运行
- **图片支持**：截图分析、视觉理解
- **Web 搜索**：实时信息检索

### MCP 协议
- 完整的 MCP 协议实现（stdio、SSE、streamable HTTP）
- 从 MCP 服务器动态注册工具
- 延迟 Schema 加载与模糊搜索——支持 100+ MCP 工具不膨胀上下文
- 资源管理和订阅支持
- 请求/响应交互支持

### 高级能力
- **多 Agent 协作**：并行 Agent 调度、团队协调、任务委托、每 Agent 独立模型/工具配置
- **权限系统**：基于规则的分类、LLM 自动分类器、危险模式检测、审批工作流
- **会话管理**：持久化、历史记录、搜索、按 ID 恢复
- **插件系统**：发现、加载、生命周期管理
- **技能与命令**：可扩展的技能框架
- **上下文压缩**：自动压缩、微压缩、会话记忆策略
- **钩子系统**：32+ 钩子事件
- **记忆系统**：持久化存储、自动提取、整合
- **LSP 集成**：6 个 LSP 工具，自动后台 `cargo check` 诊断
- **计划模式**：结构化规划与审批工作流
- **Worktree 支持**：隔离的 Git 工作树，`/batch` 并行 PR 创建
- **检查点/撤销**：基于 Git 的文件检查点，回退前显示 Diff 预览

## 快速开始

### 环境要求

- **Rust** 1.88+ (edition 2024)
- **操作系统**：Linux / macOS / Windows
- **内存**：建议 4GB 以上
- **API 密钥**：任何 OpenAI 兼容 API 密钥（Anthropic、OpenAI、Ollama 等）

### 构建

```bash
git clone https://github.com/shannon-agent/shannon-code.git
cd shannon-code
cargo build --release
```

### 配置

Shannon Code 支持多个 LLM 提供商。配置优先级：CLI 参数 > 环境变量（`SHANNON_*`）> `.shannon.toml`（项目级）> `~/.shannon/config.toml`（全局）。

#### 方式一：Anthropic（直连）

```bash
mkdir -p ~/.shannon
cat > ~/.shannon/config.toml << 'EOF'
provider = "anthropic"
api_key = "sk-ant-..."
model = "claude-sonnet-4-20250514"
max_tokens = 8192
EOF
```

#### 方式二：OpenAI 兼容端点（任意提供商）

```bash
mkdir -p ~/.shannon
cat > ~/.shannon/config.toml << 'EOF'
provider = "openai"
model = "gpt-4o"
api_key = "sk-..."
base_url = "https://api.openai.com/v1"
max_tokens = 8192
EOF
```

#### 方式三：通过 Ollama 使用本地模型

```bash
# 先启动 Ollama
ollama serve

# Shannon 自动检测 localhost:11434 上的 Ollama
export SHANNON_MODEL="llama3"
shannon
```

#### 方式四：环境变量

```bash
export SHANNON_API_KEY="sk-ant-..."
export SHANNON_MODEL="claude-sonnet-4-20250514"
shannon
```

#### 可用环境变量

| 变量 | 说明 |
|------|------|
| `SHANNON_API_KEY` | LLM 提供商的 API 密钥 |
| `SHANNON_MODEL` | 模型名称（如 `claude-sonnet-4-20250514`、`gpt-4o`） |
| `SHANNON_PROVIDER` | 提供商：`anthropic`、`openai`、`ollama`、`custom` |
| `SHANNON_BASE_URL` | 自定义 API 端点 URL |
| `SHANNON_MAX_TOKENS` | 最大输出 token 数 |
| `SHANNON_TEMPERATURE` | 采样温度（0.0–1.0） |
| `SHANNON_TIMEOUT` | 请求超时秒数 |
| `SHANNON_DEBUG` | 启用调试日志 |
| `SHANNON_PERMISSION_PROFILE` | 权限配置：`strict`、`balanced`、`permissive` |

自动检测：`ANTHROPIC_API_KEY` 和 `OPENAI_API_KEY` 也可作为备用密钥。

#### MCP 服务器配置

在 `.mcp.json`（项目级）或 `~/.claude/settings.json` 中添加 MCP 服务器：

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

### 运行

```bash
shannon                          # 启动交互式 REPL
shannon /path/to/project         # 在指定目录启动
shannon --prompt "解释auth模块"   # 非交互/CI 模式
shannon --resume                  # 恢复最近的会话
shannon --resume <session-uuid>   # 恢复指定会话
shannon --continue                # 恢复最近的会话（--resume 别名）
shannon --prompt "列出所有TODO" --schema schema.json  # 结构化输出
echo "修复这个bug" | shannon --pipe  # 管道模式
shannon --prompt "重构" --allowed-tools Read,Edit,Bash,Grep --max-turns 10  # CI 限制工具
shannon --prompt "修复lint" --diff-only  # 仅输出 diff
```

### 常用命令（REPL 内）

| 命令 | 说明 |
|------|------|
| `/help` | 显示可用命令 |
| `/undo list` | 列出文件检查点 |
| `/undo <n>` | 预览并回退到检查点 n |
| `/rewind` | 回退对话和/或代码 |
| `/models` | 切换 LLM 模型 |
| `/compact` | 压缩对话上下文 |
| `/diff` | 显示文件差异查看器 |
| `/batch` | 并行工作树隔离 PR 创建 |
| `/config` | 查看/编辑配置 |
| `/cost` | 显示 token 使用量和成本 |
| `/search` | 搜索对话历史 |

## 可靠性与测试覆盖

| 指标 | 数值 |
|------|------|
| Rust 代码总量 | ~300,000 行 |
| 总测试数 | **8,516** |
| Crate 数量 | 12 |
| 零测试文件数 | **0**（每个 `src/**/*.rs` 至少一个 `#[test]`） |
| CI 代码检查 | `cargo clippy --workspace -- -D warnings`（零警告） |

## 许可证

本项目采用 [Apache License 2.0](LICENSE) 发布。

## 文档

完整文档请访问 [shannon-agent.github.io/shannon-code](https://shannon-agent.github.io/shannon-code/)。

---

<div align="center">

使用 Rust 构建

[返回顶部](#shannon-code)

</div>
