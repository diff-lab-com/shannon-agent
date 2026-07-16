# Shannon Code

<div align="center">

**高性能、开源的 AI 辅助编程工具，使用 Rust 编写**

[![Rust](https://img.shields.io/badge/rust-1.88+-orange.svg)](https://www.rust-lang.org)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-7889-brightgreen.svg)
[![Crates](https://img.shields.io/badge/crates-12-blue.svg)

[English](./README.md) | [中文文档](#什么是-shannon-code) | [完整文档](https://shannon-agent.github.io/shannon-code/)

</div>

---

## 什么是 Shannon Code？

Shannon Code 是一个完全开源的、基于 Rust 的 AI 编程助手，支持**任何 LLM 提供商** — Anthropic、OpenAI、Ollama、DeepSeek 或任何 OpenAI 兼容端点。它提供丰富的终端 UI、强大的工具编排、多 Agent 协调和模型上下文协议（MCP）扩展能力。

与闭源替代方案不同，Shannon Code **没有隐藏的计费注入**、**没有破坏缓存的动态头**、**没有供应商锁定**。每一行代码都可审计，每一个行为都经过近 8,000 个测试验证。

**核心优势：**

| 特性 | Shannon Code | 典型闭源工具 |
|------|-------------|-------------|
| LLM 提供商 | Anthropic、OpenAI、Ollama、任何 OpenAI 兼容端点 | 单一供应商 |
| 成本透明 | 无隐藏费用或缓存操纵 | 动态计费头使成本膨胀 10-20 倍 |
| 测试覆盖 | ~7,900 个测试，每个文件均有覆盖 | 通常零测试 |
| 可扩展性 | MCP 协议、插件系统、技能框架 | 有限或封闭 |
| Agent 编排 | 多 Agent 团队、工作树隔离、`/batch` 并行 PR | 基础或无 |
| 代码可审计 | 源代码完全可见 | 黑盒 |

---

## 功能特性

### 多提供商 LLM 支持

一个配置文件连接任意 LLM：

| 提供商 | 模型 | 配置 |
|--------|------|------|
| Anthropic | Claude Sonnet、Opus、Haiku | `provider = "anthropic"` |
| OpenAI | GPT-4o、GPT-4、GPT-3.5 | `provider = "openai"` |
| Ollama | Llama、Mistral、Qwen 等 | `provider = "ollama"`（自动检测） |
| DeepSeek | DeepSeek Chat、Coder | `provider = "openai"` + `base_url` |
| 任何 OpenAI 兼容端点 | 任意模型 | `provider = "openai"` + `base_url` |

支持 Anthropic 提示缓存，采用三层缓存断点注入以实现最高效率。

### 工具系统

全面的内置工具集：

- **文件操作** — 读取、编辑、写入、MultiEdit，支持三路合并和冲突解决
- **代码分析** — 语法高亮、符号导航（LSP）、Diff 渲染
- **Git 集成** — 状态、差异、日志、提交、分支管理
- **命令执行** — 沙箱化 Bash，流式输出，超时控制
- **Web 搜索** — 实时信息检索
- **图片分析** — 截图理解和视觉推理
- **Notebook 编辑** — Jupyter notebook 单元格读取/编辑/插入/删除

### MCP（模型上下文协议）

完整的 MCP 实现，兼容 Claude Code 的 MCP 生态：

- **传输层**：stdio、SSE、streamable HTTP
- **工具发现**：`tools/list` 延迟 Schema 加载 — 支持 100+ 工具不膨胀上下文
- **模糊搜索**：`mcp__tool_search` 按名称或描述查找工具
- **资源管理**：订阅资源更新，处理通知
- **Webhook 支持**：HMAC-SHA256 签名事件，带重试和持久化
- **配置**：`.mcp.json`（项目级）或 `~/.claude/settings.json`

### 多 Agent 协作

协调多个 AI Agent 处理复杂任务：

- **团队协调** — `TeamCreate`、`SendMessage`、任务分配和跟踪
- **工作树隔离** — 每个 Agent 在独立的 git worktree 中工作
- **独立配置** — 每个 Agent 可覆盖模型、工具和工作目录
- **`/batch` 命令** — 任务分解、工作树创建、Agent 调度、并行 PR 创建
- **Agent 仪表板** — `AgentBarWidget` 和 `AgentsPanel` 实时状态视图

### 权限系统

精细的安全控制：

- **规则分类器** — 已知安全/危险操作的匹配规则
- **LLM 自动分类** — 模糊场景的异步 LLM 回退（置信度 < 0.7）
- **权限配置** — 严格、均衡、宽松或自定义（从 `.shannon/profiles/*.toml` 加载）
- **4 级优先级** — 硬拒绝 > 软拒绝 > 允许 > 显式意图
- **审批工作流** — 危险操作的交互式确认

### 会话与上下文管理

- **会话持久化** — 保存、按 ID 恢复、搜索历史
- **上下文压缩** — 自动压缩、微压缩、对话阶段跟踪
- **记忆系统** — 持久化存储，自动提取和整合
- **扩展上下文** — 阶段式预算重分配（初始化 → 活跃 → 扩展 → 临界）
- **检查点/撤销** — 基于 Git 的文件检查点，回退前显示 Diff 预览
- **计划模式** — 结构化规划与审批工作流

### 插件与技能系统

通过插件和技能扩展 Shannon：

- **插件发现** — 从 `.shannon/plugins/` 加载，支持清单解析
- **工具插件** — 基于 MCP 的工具发现和注册
- **命令插件** — 在 REPL 中注册为斜杠命令
- **技能插件** — 斜杠命令触发的提示模板
- **钩子系统** — 32+ 事件（工具执行、压缩、配置变更、Agent 生命周期）

### 国际化

- 10 种语言：英语、中文、印地语、西班牙语、法语、阿拉伯语、孟加拉语、葡萄牙语、俄语、日语
- 社区可贡献的 `locales/` 目录翻译文件
- UI 语言运行时可切换

### VS Code 扩展

VS Code 配套扩展（`editors/vscode/`）：

- WebView 聊天面板，支持 Markdown 渲染
- Diff 查看器，审查文件变更（接受/拒绝）
- NDJSON 子进程通信
- 状态栏连接状态指示器
- VS Code 设置与 Shannon CLI 配置同步

---

## 快速开始

### 1. 安装

下载适用于您平台的最新版本：

```bash
# Linux / macOS（从 GitHub Releases 下载）
curl -fsSL https://github.com/shannon-agent/shannon-agent/releases/latest/download/shannon-$(uname -s)-$(uname -m).tar.gz | tar xz
sudo mv shannon /usr/local/bin/

# 或使用 cargo（需要 Rust 1.88+）
cargo install --git https://github.com/shannon-agent/shannon-agent.git
```

<details>
<summary>其他平台</summary>

- **Windows**：从 [Releases](https://github.com/shannon-agent/shannon-agent/releases) 下载 `.zip`
- **从源码构建**：见下方[开发者指南](#开发者指南)

</details>

### 2. 配置

设置 API 密钥和首选模型：

```bash
# 方式 A：环境变量（最快）
export SHANNON_API_KEY="sk-ant-..."
export SHANNON_MODEL="claude-sonnet-4-20250514"

# 方式 B：配置文件（持久化）
mkdir -p ~/.shannon
cat > ~/.shannon/config.toml << 'EOF'
provider = "anthropic"
api_key = "sk-ant-..."
model = "claude-sonnet-4-20250514"
max_tokens = 8192
EOF
```

<details>
<summary>其他提供商</summary>

**OpenAI / DeepSeek / 任何兼容端点：**
```bash
cat > ~/.shannon/config.toml << 'EOF'
provider = "openai"
model = "gpt-4o"
api_key = "sk-..."
base_url = "https://api.openai.com/v1"
EOF
```

**Ollama（本地，无需 API 密钥）：**
```bash
ollama serve
export SHANNON_MODEL="llama3"
```

</details>

### 3. 运行

```bash
shannon                          # 交互式 REPL
shannon /path/to/project         # 在项目目录打开
shannon --resume                  # 恢复上次会话
```

就这么简单。输入问题，按回车即可。

<details>
<summary>更多用法</summary>

```bash
shannon --prompt "解释auth模块"             # 非交互/CI 模式
shannon --prompt "列出TODO" --schema schema.json  # 结构化 JSON 输出
echo "修复这个bug" | shannon --pipe          # 管道模式
shannon --prompt "重构" --allowed-tools Read,Edit,Bash,Grep --max-turns 10  # CI
shannon --prompt "修复lint" --diff-only       # 仅输出 diff
```

</details>

<details>
<summary>REPL 命令</summary>

| 命令 | 说明 |
|------|------|
| `/help` | 显示可用命令 |
| `/config` | 查看/编辑配置 |
| `/model` | 切换 LLM 模型 |
| `/compact` | 压缩对话上下文 |
| `/undo list` | 列出文件检查点 |
| `/undo <n>` | 预览并回退到检查点 |
| `/rewind` | 回退对话和/或代码 |
| `/diff` | 显示文件差异查看器 |
| `/batch` | 并行工作树隔离 PR 创建 |
| `/team` | 管理 Agent 团队 |
| `/cost` | 显示 token 使用量和成本 |
| `/search` | 搜索对话历史 |
| `/doctor` | 检查 Shannon 安装状态 |
| `/routine` | 管理触发/定时例程 |
| `/preset` | 使用对话预设（review、debug 等） |
| `/session` | 保存/加载会话模板 |

</details>

<details>
<summary>MCP 服务器配置</summary>

在 `.mcp.json`（项目级）或 `~/.claude/settings.json` 中添加：

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
<summary>环境变量参考</summary>

| 变量 | 说明 |
|------|------|
| `SHANNON_API_KEY` | LLM 提供商的 API 密钥 |
| `SHANNON_MODEL` | 模型名称（如 `claude-sonnet-4-20250514`、`gpt-4o`） |
| `SHANNON_PROVIDER` | 提供商：`anthropic`、`openai`、`ollama`、`custom` |
| `SHANNON_BASE_URL` | 自定义 API 端点 URL |
| `SHANNON_MAX_TOKENS` | 最大输出 token 数 |
| `SHANNON_TEMPERATURE` | 采样温度（0.0-1.0） |
| `SHANNON_PERMISSION_PROFILE` | 权限配置：`strict`、`balanced`、`permissive` |

自动检测：`ANTHROPIC_API_KEY` 和 `OPENAI_API_KEY` 也可作为备用密钥。

</details>

---

## 项目结构

```
shannon-agent/
├── crates/
│   ├── shannon-core/          # 核心引擎：API 客户端、查询引擎、权限、状态
│   ├── shannon-tools/         # 工具实现：文件操作、Git、搜索、Notebook
│   ├── shannon-agents/        # Agent 系统：协调器、调度器、执行器
│   ├── shannon-ui/            # 终端 UI：REPL、组件、渲染
│   ├── shannon-mcp/           # MCP 协议：传输层、服务器、客户端、进程池
│   ├── shannon-commands/      # 斜杠命令：内置命令注册表
│   ├── shannon-skills/        # 技能框架：发现、加载、执行
│   ├── shannon-types/         # 共享类型定义
│   ├── shannon-tool-interface/# 工具 trait 定义
│   ├── shannon-codegen/       # 代码生成工具
│   ├── shannon-cli/           # CLI 入口（shannon 二进制）
│   ├── shannon-agent/         # 独立 Agent（JSON-RPC over stdin/stdout）
│   └── shannon-api-protocol/  # 线协议（serde 类型 + TS 代码生成）
├── desktop/                   # Shannon Desktop（Tauri + React 19）
│   └── ui/                    # 前端（React、Vite、Tailwind）
├── gateway/                   # Shannon Gateway（TypeScript 平台桥接）
├── editors/vscode/            # VS Code 扩展
├── skills/                    # 内置技能定义
├── locales/                   # 国际化翻译文件（10 种语言）
├── tests/scenarios/           # YAML 声明式测试场景
└── docs/                      # 文档
```

---

## 开发者指南

面向贡献者和高级用户的源码构建说明。

```bash
cargo build                        # 调试构建
cargo check --workspace            # 快速类型检查
just test                          # 运行所有测试（nextest）
just dev                           # check + lint + test（提交前运行）
cargo clippy --workspace           # 代码检查
cargo fmt                          # 格式化
```

安装工具链：`cargo install just cargo-nextest`。

### 测试

| 命令 | 说明 | 需要 API 密钥？ |
|------|------|---------------|
| `just test` | 所有单元测试和 Mock 测试 | 否 |
| `just ci` | 完整 CI 套件 | 否 |
| `just scenarios` | YAML 场景测试 | 否 |
| `just bench` | Criterion 基准测试 | 否 |
| `just record` | 录制真实 API 固定件 | 是 |
| `just replay` | 回放录制的固定件 | 否 |

### 发布构建

```bash
./scripts/release.sh                      # 当前平台
./scripts/release.sh --all                # 所有平台
./scripts/release.sh --target x86_64-unknown-linux-gnu
```

产物输出到 `target/dist/`，格式为 `.tar.gz`（Linux/macOS）或 `.zip`（Windows）。

---

## 可靠性与测试覆盖

| 指标 | 数值 |
|------|------|
| Rust 代码总量 | ~282,000 行 |
| 源文件数 | 355 |
| 总测试数 | **7,889** |
| Crate 数量 | 12 |
| 零测试文件数 | **0**（每个 `src/**/*.rs` 至少一个 `#[test]`） |
| CI 代码检查 | `cargo clippy --workspace -- -D warnings`（零警告） |

各 Crate 测试分布：

| Crate | 测试数 | 职责 |
|-------|--------|------|
| `shannon-core` | ~3,370 | API 客户端、查询引擎、权限、工具、状态 |
| `shannon-ui` | ~1,089 | 终端 UI、REPL、组件、渲染 |
| `shannon-tools` | ~1,111 | 工具实现 |
| `shannon-commands` | ~335 | 内置命令 |
| `shannon-agents` | ~471 | 多 Agent 协作 |
| `shannon-mcp` | ~373 | MCP 服务器集成 |
| `shannon-cli` | ~191 | CLI 入口 |
| `shannon-skills` | ~171 | 技能系统 |
| 其他 Crate | ~1,051 | Codegen、类型、工具接口、Agent、桌面 |

---

## 二进制文件

- **`shannon`** — 主交互式 CLI。终端 REPL、流式 LLM 响应、工具编排。日常使用。
- **`shannon-agent`** — 独立 Agent 工作进程（JSON-RPC over stdin/stdout）。内部用于多 Agent 编排。通常不直接运行。

---

## 许可证

[Apache License 2.0](LICENSE)

---

## 免责声明

Shannon Code 是一个独立的、基于净室方法实现的 AI 辅助编程工具，仅参考公开文档、开放规范（如 [Model Context Protocol](https://modelcontextprotocol.io)）和通用软件工程原则构建。不隶属于任何 AI 编程工具供应商。仅用于教育和研究目的。

---

<div align="center">

使用 Rust 构建 | [English](./README.md)

</div>
