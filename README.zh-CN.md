# Shannon

> **说明：** 统一的 `shannon` CLI 自早期版本起取代了原 `shannon-code` 产品名。安装路径、子命令与配置均不变——仅二进制名称变更。

<div align="center">

**完全开源，尽在掌控；密钥不出门，数据不搬家。**

开源 AI agent 工作台 —— 终端、无头、服务、桌面四种形态，
一个 Rust 引擎，任意大模型。

[![Rust](https://img.shields.io/badge/rust-1.88+-orange.svg)](https://www.rust-lang.org)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-see%20metrics.md-brightgreen.svg)](./docs/metrics.md)
<!-- metrics:start:badge -->[![Crates](https://img.shields.io/badge/crates-20-blue.svg)](./docs/metrics.md)<!-- metrics:end:badge -->

[English](./README.md) | [中文文档](#shannon-是什么) | [完整文档](https://shannon-agent.github.io/shannon-code/)

</div>

---

## Shannon 是什么？

Shannon 是完全开源（Apache-2.0）、基于 Rust 的 **AI agent 工作台**：运行在你自己的电脑上，支持**任何 LLM 提供商**——Anthropic、OpenAI、DeepSeek、智谱 GLM、Ollama 或任何 OpenAI 兼容端点。一个引擎，四种形态：交互式终端 UI、面向脚本与 CI 的无头模式、本地引擎服务，以及桌面应用。

每一个设计决策都围绕两条承诺：

### 1. 开源可控

- **每一行代码可审计** —— Apache-2.0，无黑盒。<!-- metrics:start:intro -->每一个行为都经过 **11,752** 个自动化测试验证。<!-- metrics:end:intro -->
- **每一步 agent 行为可回放** —— 会话采用事件溯源：每轮追加写入 `events.jsonl`，`shannon trace show / replay / diff / export` 让你完整还原 agent 到底做了什么——agent 的行车记录仪。
- **每一分成本可见** —— BYOK 按量付费，会话预算上限、上下文分类拆解、缓存命中率可见，没有订阅额度黑盒。
- **零供应商锁定** —— 随时切换提供商；上游涨价、型号退役都困不住你。兼容 Claude Code 生态：`CLAUDE.md`、`.claude/` agents、skills、hooks、`.mcp.json` 开箱即用。

### 2. 密钥不出门

- **API 密钥直连你选择的提供商** —— 没有中间服务器，没有云端凭据池。IM 渠道凭据只存 OS keyring。
- **出站 secret 脱敏** —— `secret-guard` 插件（基于 `shannon-plugin-api` 内容变换契约）在消息发往模型前脱敏 secret，本地执行时再还原；变换字节稳定，prompt 缓存照常命中。
- **OS 级沙箱** —— Landlock（Linux）、macOS Seatbelt、bubblewrap；规则 + LLM 辅助的权限系统，严格/均衡/宽松/自定义配置，高危工具逐动作确认。
- **提示注入扫描 + 签名校验** 覆盖 skills 与 MCP 服务器；webhook 事件 HMAC-SHA256 签名。
- **默认零遥测** —— 本地语音输入（whisper.rs）永不外发音频。

**Shannon 与竞品的对比**（截至 2026-09；来源见 [docs/competitive-research-2026-09.md](docs/competitive-research-2026-09.md)）：

| | Shannon | 云端订阅制 agent（Claude Code、Codex、Grok Bot） | 开源同类（Hermes、Codex CLI、Grok Build） |
|---|---|---|---|
| 许可 | Apache-2.0 完全开源 | 闭源 | 开源 |
| 执行位置 | 本地优先，你自己的电脑 | 云 VM / 云沙箱 | 本地 |
| 密钥与 secret 处理 | OS keyring + 出站脱敏 + 注入扫描 | 厂商托管云凭据，各不相同 | 各不相同 |
| LLM 提供商 | 任意（BYOK） | 单一供应商 | 多家 / 任意 |
| 成本模型 | 按量付费 + 预算上限 + 拆解可见 | 订阅额度 / credits | BYOK |
| 可审计性 | 事件溯源会话，`trace` 回放/diff | 各不相同，多为黑盒 | 各不相同 |
<!-- metrics:start:diffrow -->| 测试覆盖 | **11,752** 个测试，覆盖 20 个 workspace 成员 | 不适用（闭源） | 各不相同 |<!-- metrics:end:diffrow -->
| 产品形态 | 终端 + 无头 + 服务 + 桌面，一个引擎 | 各不相同 | 各不相同 |

---

## 一个引擎，四种形态

一次安装，四个入口（每个桌面安装器同时内含 `shannon` CLI）：

| 入口 | 用途 |
|---|---|
| `shannon` | 交互式 TUI / REPL（默认） |
| `shannon -p "…"` | 无头脚本化 —— NDJSON 流式输出，`--schema` 结构化输出 |
| `shannon serve` | 引擎守护进程（:33420）—— gateway / 移动端连接的 API 面 |
| `shannon desktop` | 桌面应用 —— `--install` 可按需下载当前平台安装包 |

会话跨形态互通：终端里开始，桌面上继续，手机上审批。

### Shannon（终端）

为终端而生的 AI 编程 agent：丰富的 TUI（diff 查看器、Markdown 渲染）、工具编排、worktree 隔离的多 agent 团队、MCP 扩展，以及 `shannon trace` 带来的完整可回放性。

### Shannon Desktop

基于 **Tauri 2 + React 19（而非 Electron）** 的原生桌面工作台。双模式服务两类用户：

- **Simple 模式** —— 面向所有人：聊天中的工具调用内联可见、可逐个批准或撤销；拖拽附件；语音输入（云端或完全本地）；定时任务带日历视图与依赖图；Triage 收件箱汇总你不在时 agent 干的所有活。
- **Advanced 模式** —— 面向开发者：扩展（MCP 服务器、skills、agents）、可拖拽多面板工作区 + 集成终端、git worktree 管理、记忆图谱、OPC 多 agent 编排。

还有：手机配对（扫码派发与审批）、IM 渠道（Telegram / Discord / Slack / 飞书 / 钉钉）、系统托盘、全局快捷键、自动更新、8 套主题。

---

## 功能特性

### 多提供商 LLM 支持

一个配置文件连接任意 LLM——你的密钥、你的提供商、直连：

| 提供商 | 模型 | 配置 |
|--------|------|------|
| Anthropic | Claude Sonnet / Opus / Haiku 系列 | `provider = "anthropic"` |
| OpenAI | GPT-4o 及更新 | `provider = "openai"` |
| Ollama | Llama、Mistral、Qwen 等（本地） | `provider = "ollama"`（自动检测） |
| DeepSeek | DeepSeek Chat / Coder | `provider = "openai"` + `base_url` |
| 智谱 Z.ai（GLM） | GLM 系列 | `provider = "openai"` + `base_url` |
| 任何 OpenAI 兼容端点 | 任意模型 | `provider = "openai"` + `base_url` |

支持 Anthropic 提示缓存，采用三层缓存断点注入以实现最高效率。

### 目标与自主任务

给 agent 派一个目标，而不只是一条 prompt：

- **`/goal`** —— 持久目标 + 自动续跑；跨上下文压缩持续工作，自动识别 `GOAL_COMPLETE` / `GOAL_BLOCKED`，阻塞时按 30 分钟 → 1 小时 → 2 小时退避重试
- **预算上限** —— `--budget $N` 硬性花费上限；anti-spin 与 stall-strike 守卫防止原地打转
- **`/loop` / `/ralph`** —— 共享同一套守卫的自主迭代循环
- **Triage 收件箱** —— 结果与阻塞项进入桌面收件箱，一键回到原会话续跑

### 自动化与触发器

- **Routines** —— cron 定时、一次性、事件触发任务
- **API endpoint 触发器** —— `shannon serve` 为每个 routine 暴露 HMAC-SHA256 校验的触发 URL（「把 Slack 告警指向你的 agent」）
- **GitHub 事件触发器** —— 响应 issue 与 CI 事件
- **IM 渠道** —— Telegram / Discord / Slack / 飞书 / 钉钉 入站：私聊直接响应，群聊 @ 触发；进度与结果回推原会话
- **手机派发** —— 扫码配对，手机上派活与审批

### 多 Agent 协作

- **团队协调** —— `TeamCreate`、`SendMessage`、任务分配和跟踪
- **工作树隔离** —— 每个 Agent 在独立的 git worktree 中工作
- **独立配置** —— 每个 Agent 可覆盖模型、工具和工作目录
- **`/batch` best-of-N** —— 任务分解、并行 worktree 隔离尝试、并排 diff 对比、采纳最优
- **Agent 仪表板** —— TUI 与桌面实时状态

### 工具系统

- **文件操作** —— 读取、编辑、写入、MultiEdit，支持三路合并和冲突解决
- **代码分析** —— 语法高亮、符号导航（LSP）、Diff 渲染、仓库符号地图
- **Git 集成** —— 状态、差异、日志、提交、分支管理
- **命令执行** —— 沙箱化 Bash，流式输出，超时控制，另有后台进程工具组
- **Web 与浏览器** —— Web 搜索、本地浏览器自动化（驱动你已安装的浏览器，不捆绑浏览器二进制）
- **Computer use** —— 截图理解 + 受控输入注入，高危动作逐次确认；macOS 另有 AppleScript/Shortcuts 工具
- **图片与文档分析** —— 截图理解、批量图片分析、PDF 文本提取
- **Notebook 编辑** —— Jupyter notebook 单元格读取/编辑/插入/删除

### MCP（模型上下文协议）

完整的 MCP 实现，兼容 Claude Code 的 MCP 生态：

- **传输层**：stdio、SSE、streamable HTTP
- **工具发现**：`tools/list` 延迟 Schema 加载 — 支持 100+ 工具不膨胀上下文
- **模糊搜索**：`mcp__tool_search` 按名称或描述查找工具
- **资源管理**：订阅资源更新，处理通知
- **Webhook 支持**：HMAC-SHA256 签名事件，带重试和持久化
- **配置**：`.mcp.json`（项目级）或 `~/.claude/settings.json`
- **SaaS 集成**：内置 GitHub、Slack、Jira、Notion、Linear MCP 服务器

### 会话、上下文与记忆

- **事件溯源会话** —— 每轮追加写入 `events.jsonl`；resume、搜索、回放、diff 都是对这份唯一权威日志的投影（`shannon trace show / replay / diff / export`）
- **上下文压缩** —— 自动压缩、微压缩、对话阶段跟踪、token 预算看门狗（`SHANNON_TOKEN_BUDGET`）
- **记忆系统** —— 持久化记忆（带溯源）、桌面记忆页、自动提取和整合
- **检查点/撤销** —— 基于 Git 的文件检查点，回退前显示 Diff 预览（`/rewind`）
- **计划模式** —— 结构化规划与审批工作流

### 插件与技能系统

- **`shannon-plugin-api`** —— 引擎与插件之间的内容变换中间件契约，四条不变量：字节稳定确定性（保 prompt 缓存）、单向流、幂等、显式失败语义。内置 `secret-guard` 插件是首个实现。
- **插件发现** —— 从 `.shannon/plugins/` 加载，支持清单解析
- **命令插件** —— 在 REPL 中注册为斜杠命令
- **技能插件** —— 斜杠命令触发的提示模板，兼容 `.claude/skills/*/SKILL.md`
- **钩子系统** —— 32+ 事件（工具执行、压缩、配置变更、Agent 生命周期）

### 远程执行目标（SSH / Docker）

把整套工具链跑到远程机器或容器里：

- **SSH 主机** —— 复用 `~/.ssh/config`（别名、agent、ProxyJump）；文件走 SFTP，命令走多路复用 ssh 连接。首次连接走标准 known_hosts TOFU 流程。
- **Docker 容器** —— attach 到运行中的容器（`docker exec`）；可经 SSH 跳板（`ssh_target`）连接远程 daemon。
- **管理** —— TUI 中 `/remote`，无头模式 `--target <name>`，或桌面应用 Settings → Remotes。目标存于 `~/.shannon/remotes.toml`（不存凭据；认证由系统 ssh 负责）。

```bash
/remote use build-box          # TUI：切换会话到目标
shannon --target build-box -p "run the test suite"   # 无头模式
```

### 国际化

- 10 种语言：英语、中文、印地语、西班牙语、法语、阿拉伯语、孟加拉语、葡萄牙语、俄语、日语
- 社区可贡献的 `locales/` 目录翻译文件
- UI 语言运行时可切换

---

## 安全与隐私

Shannon 本地优先：状态存于 `~/.shannon/`，除了你配置的模型 API 调用，没有任何数据离开你的电脑。

| 层 | 做什么 |
|---|---|
| **凭据** | 提供商 API 密钥只在你电脑上，直连你选择的提供商。IM 渠道与集成凭据存 OS keyring。任何中间服务器都不持有你的密钥。 |
| **Secret 脱敏** | `secret-guard` 插件（经 `shannon-plugin-api`）在消息出站到模型前脱敏 secret，本地工具执行时还原——字节稳定，prompt 缓存不受影响。会话级脱敏策略经 `~/.shannon/redaction.toml`。 |
| **沙箱** | Landlock（Linux）、macOS Seatbelt、bubblewrap provider；文件写入的清单式沙箱强制；实验性 `/sandbox` 开关。 |
| **权限** | 规则分类器 + LLM 辅助分类（置信度 < 0.7 回退），严格/均衡/宽松/自定义配置，4 级优先级，危险操作交互式审批，高危工具（computer use、AppleScript）逐动作确认。 |
| **供应链** | skills 与 MCP 服务器提示注入扫描 + 签名校验；CI 中 `cargo-deny` 与 `cargo-semver-checks` 门禁。 |
| **遥测** | 默认无；任何使用信号严格 opt-in。本地语音输入（whisper.rs）音频零出站。 |

---

## 快速开始

### 1. 安装

下载适用于您平台的最新版本：

```bash
# Linux / macOS —— 一行命令，自动识别平台（CLI + gateway + 桌面端）
curl -fsSL https://github.com/diff-lab-com/shannon-agent/releases/latest/download/install.sh | sh

# 服务器 / 无头环境 —— 只装 CLI，不需要 sudo
curl -fsSL https://github.com/diff-lab-com/shannon-agent/releases/latest/download/install.sh | SHANNON_COMPONENTS=cli sh

# 或使用 cargo（需要 Rust 1.88+）
cargo install --git https://github.com/diff-lab-com/shannon-agent.git
```

<details>
<summary>其他平台</summary>

- **Windows**：`irm https://github.com/diff-lab-com/shannon-agent/releases/latest/download/install.ps1 | iex`（或从 [Releases](https://github.com/diff-lab-com/shannon-agent/releases) 下载 `.zip`）
- **从源码构建**：见下方[开发者指南](#开发者指南)

</details>

### 2. 配置

设置 API 密钥和首选模型——密钥只在你电脑上，直连你选择的提供商：

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
shannon desktop                   # 或启动桌面应用
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
shannon --goal "让 CI 变绿" --budget 5        # 自主目标 + 花费上限
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
| `/batch` | 并行工作树隔离 PR 创建（best-of-N） |
| `/team` | 管理 Agent 团队 |
| `/goal` | 设置持久、自动续跑的目标 |
| `/remote` | 连接 SSH 主机 / Docker 容器作为执行目标 |
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
| `SHANNON_TOKEN_BUDGET` | 会话 token 预算看门狗 |

自动检测：`ANTHROPIC_API_KEY` 和 `OPENAI_API_KEY` 也可作为备用密钥。

</details>

---

## 项目结构

```
shannon-agent/
├── crates/
│   ├── shannon-core/          # 核心引擎：状态、会话、记忆、权限、secret guard
│   ├── shannon-engine/        # LLM API 客户端、流式适配、压缩/上下文预算
│   ├── shannon-tools/         # 工具实现：文件操作、Git、浏览器、computer use
│   ├── shannon-ui/            # 终端 UI：REPL、组件、渲染
│   ├── shannon-agents/        # 多 Agent 协作：团队、工作树隔离
│   ├── shannon-mcp/           # MCP 协议：传输层、服务器、客户端、进程池
│   ├── shannon-mcp-saas/      # SaaS MCP 服务器（GitHub、Slack、Jira、Notion、Linear）
│   ├── shannon-commands/      # 斜杠命令：内置命令注册表
│   ├── shannon-skills/        # 技能框架：发现、加载、执行
│   ├── shannon-plugin-api/    # 插件内容变换契约（secret-guard）
│   ├── shannon-server/        # HTTP API 服务器（shannon serve）
│   ├── shannon-remote/        # 远程执行环境（SSH 主机、Docker）
│   ├── shannon-repomap/       # 仓库符号地图（tree-sitter）
│   ├── shannon-cli/           # CLI 入口（shannon 二进制）
│   ├── shannon-agent/         # 独立 Agent（JSON-RPC over stdin/stdout）
│   ├── shannon-api-protocol/  # 线协议（serde 类型 + TS 代码生成）
│   ├── shannon-types/         # 共享类型定义
│   ├── shannon-tool-interface/# 工具 trait 定义
│   ├── shannon-codegen/       # 代码生成工具
│   └── shannon-stability-attr/# 稳定性属性宏
├── desktop/                   # Shannon Desktop（Tauri 2 + React 19）
│   └── ui/                    # 前端（React、Vite、Tailwind）
├── gateway/                   # Shannon Gateway（TypeScript 平台桥接）
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

### Git hooks（pre-push 检查）

每个 clone 一次性设置：

```bash
git config core.hooksPath .githooks
```

启用后：
- **pre-commit**：自动对暂存的 `.rs` 文件执行 `cargo fmt`。
- **pre-push**：运行 `scripts/local-check.sh` —— `cargo fmt --check`、`cargo build --workspace`、`cargo clippy`。

WIP 推送旁路：`git push --no-verify` 或 `PRE_PUSH_QUICK=1 git push`（仅 fmt + build，跳过 clippy）。

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

<!-- metrics:start:table -->
| 指标 | 数值 |
|------|------|
| Rust 代码总量 | 418,458 行 |
| 源文件数 | 624 |
| 总测试数（nextest 可运行） | **11,752** |
| Crate 数（workspace 成员） | 20（19 个 crate + desktop） |
| 零测试 Crate 数 | 2（`shannon-server`, `shannon-stability-attr`） |
| CI 代码检查 | `cargo clippy --workspace -- -D warnings`（零警告） |
<!-- metrics:end:table -->

各 Crate 测试分布：

<!-- metrics:start:crates -->
| Crate | 测试数 | 职责 |
|-------|--------|------|
| `shannon-core` | 3,766 | API 客户端、查询引擎、权限、工具、状态 |
| `shannon-tools` | 1,630 | 工具实现：文件操作、Git、搜索、Notebook |
| `shannon-ui` | 1,497 | 终端 UI、REPL、组件、渲染 |
| `shannon-engine` | 1,113 | LLM API 客户端、流式适配、压缩/上下文预算、权限 |
| `shannon-agents` | 897 | 多 Agent 协作：团队、工作树隔离 |
| `shannon-desktop` | 599 | Tauri 桌面应用外壳与命令 |
| `shannon-mcp` | 578 | MCP 协议：传输层、服务器、客户端、进程池 |
| `shannon-cli` | 486 | CLI 入口（`shannon` 二进制） |
| `shannon-commands` | 416 | 内置斜杠命令 |
| `shannon-mcp-saas` | 185 | SaaS MCP 服务器（GitHub、Slack、Jira、Notion、Linear） |
| `shannon-skills` | 172 | 技能框架：发现、加载、执行 |
| `shannon-codegen` | 100 | 代码生成工具 |
| `shannon-types` | 84 | 共享类型定义 |
| `shannon-agent` | 65 | 独立 Agent（JSON-RPC over stdin/stdout） |
| `shannon-remote` | 55 | 远程执行环境（SSH 主机、Docker） |
| `shannon-tool-interface` | 42 | 工具 trait 定义 |
| `shannon-api-protocol` | 37 | 线协议（serde 类型 + TS 代码生成） |
| `shannon-repomap` | 30 | 仓库符号地图（tree-sitter） |
| `shannon-server` | 0 | HTTP API 服务器（`shannon serve`） |
| `shannon-stability-attr` | 0 | 稳定性属性宏 |
<!-- metrics:end:crates -->

---

## 二进制文件

- **`shannon`** — 主交互式 CLI。终端 REPL、流式 LLM 响应、工具编排。日常使用。
- **`shannon-agent`** — 独立 Agent 工作进程（JSON-RPC over stdin/stdout）。内部用于多 Agent 编排。通常不直接运行。

---

## 文档

- **用户与开发者文档**：[shannon-agent.github.io/shannon-code](https://shannon-agent.github.io/shannon-code/)
- **安全与隐私**：见上方[安全与隐私](#安全与隐私)章节与文档站
- **参与贡献**：[CONTRIBUTING.md](CONTRIBUTING.md) · [安全策略](SECURITY.md)

---

## 许可证

[Apache License 2.0](LICENSE)

---

## 免责声明

Shannon 是一个独立的、基于净室方法实现的 AI 辅助编程工具，仅参考公开文档、开放规范（如 [Model Context Protocol](https://modelcontextprotocol.io)）和通用软件工程原则构建。不隶属于任何 AI 编程工具供应商。仅用于教育和研究目的。

---

<div align="center">

使用 Rust 构建 | [English](./README.md)

</div>
