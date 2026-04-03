# Shannon Code

<div align="center">

**A high-performance AI-assisted coding tool, rewritten in Rust**

[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-2341-brightgreen.svg)
[![Lines](https://img.shields.io/badge/code-78K-blue.svg)

[中文文档](#中文文档)

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

## What is Shannon Code?

Shannon Code is a feature-rich, type-safe AI-assisted coding tool written entirely in Rust. It provides a terminal-based REPL interface for interacting with large language models (LLMs) while offering advanced capabilities like tool orchestration, session management, plugin systems, and MCP (Model Context Protocol) support.

### Why Rust?

- **Memory safety** — guaranteed at compile time, no data races
- **High performance** — zero-cost abstractions, near-C speed
- **Type safety** — a strong type system catches bugs before runtime
- **Concurrency** — native `async/await` support for parallel operations
- **Cross-platform** — compile once, run on Linux, macOS, and Windows

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
- Resource management and subscription support
- Elicitation and completion support

### Advanced Capabilities
- **Multi-Agent orchestration**: parallel agent dispatch, team coordination, task delegation
- **Permission system**: rule-based classification, dangerous pattern detection, approval workflows
- **Session management**: persistence, history, search, resume
- **Plugin system**: discovery, loading, lifecycle management, marketplace
- **Skills & Commands**: extensible skill framework, bundled implementations
- **Context compression**: advanced strategies with auto-compact, micro-compact, session memory
- **Hooks system**: pre/post tool execution hooks, custom event handlers
- **Memory system**: persistent memory store, auto-dream extraction, consolidation
- **LSP integration**: language server protocol for intelligent code navigation
- **Plan mode**: structured planning with approval workflows
- **Worktree support**: isolated git worktrees for parallel development
- **Auto-updater**: GitHub Releases-based update checking
- **Diagnostics & Doctor**: environment health checks, error pattern analysis
- **Voice mode**: voice input/output with keyword spotting
- **OAuth**: token management with encryption
- **Team memory sync**: bidirectional sync with secret scanning
- **Streaming tool executor**: concurrent tool execution with progress tracking

### Terminal UI
- Interactive REPL with command history and search
- Markdown rendering with syntax highlighting
- Diff visualization with colored output
- Virtual scroll and progress indicators

---

## Quick Start

### Prerequisites

- **Rust** 1.75+
- **Operating System**: Linux / macOS / Windows
- **Memory**: 4 GB+ recommended
- **API Key**: Anthropic API key (or compatible endpoint)

### Build

```bash
# Clone the repository
git clone https://github.com/your-username/shannon-code.git
cd shannon-code

# Build in release mode
cargo build --release

# The binary is at target/release/shannon
```

### Configure

First run requires an API key:

```bash
mkdir -p ~/.config/shannon
cat > ~/.config/shannon/config.toml << 'EOF
[anthropic]
api_key = "your-api-key-here"

[general]
model = "claude-sonnet-4-20250514"
max_tokens = 8192
temperature = 0.7
EOF
```

### Run

```bash
# Start Shannon Code
shannon

# Start in a specific project directory
shannon /path/to/project

# Show help
shannon --help

# Show version
shannon --version
```

---

## Project Structure

```
shannon-code/
├── crates/
│   ├── shannon-core/          # Core engine: query processing, tools, permissions, state
│   ├── shannon-tools/         # Tool implementations: file ops, git, search, notebook
│   ├── shannon-agents/        # Agent system: coordinator, dispatcher, executor
│   ├── shannon-ui/            # Terminal UI: REPL, widgets, rendering
│   ├── shannon-mcp/           # MCP protocol: transport, server, client
│   ├── shannon-commands/      # Slash commands: built-in command registry
│   ├── shannon-skills/       # Skills framework: discovery, loading, execution
│   ├── shannon-types/         # Shared type definitions
│   └── shannon-cli/           # CLI entry point
├── skills/                    # Bundled skill definitions
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

# Run all tests (2,341 tests)
cargo test --workspace

# Run a specific module's tests
cargo test -p shannon-core -- tool_execution

# Check compilation without building
cargo check --workspace

# Format code
cargo fmt

# Lint
cargo clippy
```

### Adding a New Crate

```bash
cd crates
cargo new --lib shannon-new-feature
```

Then add it to the workspace in the root `Cargo.toml`.

---

## Statistics

| Metric | Value |
|--------|-------|
| Total lines of code | ~78,000 |
| Number of tests | ~2,341 |
| Number of crates | 9 |
| Supported platforms | Linux, macOS, Windows |

---

## License

This project is released under the MIT License. See [LICENSE](LICENSE) for details.

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
- 本项目**不隶属于、不由...认可、也不与**任何其他 AI 编程工具供应商关联
- Shannon Code 的开发者**不承担任何法律责任**

本项目仅用于教育和研究目的。

## 什么是 Shannon Code？

Shannon Code 是一个功能丰富、类型安全的 AI 辅助编程工具，完全使用 Rust 编写。它提供了基于终端的 REPL 界面，用于与大语言模型（LLM）交互，同时提供工具编排、会话管理、插件系统和 MCP（模型上下文协议）支持等高级功能。

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
- 资源管理和订阅支持
- 请求/响应交互支持

### 高级能力
- **多 Agent 协作**：并行 Agent 调度、团队协调、任务委托
- **权限系统**：基于规则的分类、危险模式检测、审批工作流
- **会话管理**：持久化、历史记录、搜索、恢复
- **插件系统**：发现、加载、生命周期管理
- **技能与命令**：可扩展的技能框架
- **上下文压缩**：自动压缩、微压缩、会话记忆策略
- **钩子系统**：工具执行前后钩子、自定义事件处理
- **记忆系统**：持久化存储、自动提取、整合
- **LSP 集成**：语言服务协议实现智能代码导航
- **计划模式**：结构化规划与审批工作流
- **Worktree 支持**：隔离的 Git 工作树

## 快速开始

### 环境要求

- **Rust** 1.75+
- **操作系统**：Linux / macOS / Windows
- **内存**：建议 4GB 以上
- **API 密钥**：Anthropic API 密钥（或兼容端点）

### 构建

```bash
git clone https://github.com/your-username/shannon-code.git
cd shannon-code
cargo build --release
```

### 配置

```bash
mkdir -p ~/.config/shannon
cat > ~/.config/shannon/config.toml << 'EOF'
[anthropic]
api_key = "your-api-key-here"

[general]
model = "claude-sonnet-4-20250514"
max_tokens = 8192
temperature = 0.7
EOF
```

### 运行

```bash
shannon              # 启动
shannon /path/to/project  # 在指定目录启动
shannon --help        # 查看帮助
shannon --version     # 查看版本
```

## 许可证

本项目采用 MIT 许可证发布。详见 [LICENSE](LICENSE)。

---

<div align="center">

使用 Rust 构建

[返回顶部](#shannon-code)

</div>
