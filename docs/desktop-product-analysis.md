# Shannon Desktop 产品分析与实施策略

> 调研日期: 2026-06-06
> 覆盖产品: Claude Desktop, OpenAI Codex Desktop, OpenClaw, Hermes, WorkBuddy, Cursor, Windsurf, Aider, Continue.dev, OpenCode, Cline

---

## 一、竞品全景对比

### 1.1 架构与技术栈

| 产品 | 架构 | 桌面框架 | 核心语言 | 开源 | 许可证 |
|------|------|----------|----------|------|--------|
| **Claude Desktop** | Electron + Linux VM (Apple Virtualization) | Electron | TypeScript (闭源) | 否 | 闭源 (MCP 生态开源) |
| **Codex Desktop** | Electron + Rust App Server (JSON-RPC) | Electron | Rust (核心开源) | 部分 | Apache-2.0 (CLI/Server) |
| **OpenClaw** | Bun Gateway + 多平台适配器 | Electron (Easy app) | TypeScript | 是 | MIT/Apache-2.0 |
| **Hermes** | Python runtime + Electron 桌面 | Electron | Python | 是 | MIT |
| **WorkBuddy** | 腾讯 CodeBuddy 架构 | 原生桌面 | 未公开 | 否 | 闭源 |
| **Cursor** | VS Code fork + Rust 索引引擎 | Electron | TypeScript + Rust | 否 | 闭源 |
| **Windsurf** | VS Code fork + Cascade 引擎 | Electron | TypeScript | 否 | 闭源 |
| **Aider** | Python CLI (终端优先) | 无 | Python | 是 | Apache-2.0 |
| **Continue.dev** | IDE 扩展 + YAML 配置引擎 | 无 (IDE 插件) | TypeScript | 是 | Apache-2.0 |
| **OpenCode** | Go TUI + Bun HTTP Server | 原生桌面 | Go | 是 | MIT |
| **Shannon** | Rust CLI + Tauri v2 scaffold | **Tauri** (实验) | Rust | 是 | MIT |

**关键发现**: 几乎所有桌面 AI 编码应用都使用 Electron。Shannon 是唯一使用 Tauri 的项目，这是一个潜在差异点。

### 1.2 核心功能矩阵

| 功能 | Claude | Codex | OpenClaw | Hermes | Cursor | Shannon (当前) |
|------|--------|-------|----------|--------|--------|---------------|
| 多 Provider LLM | Anthropic only | OpenAI only | 75+ | 40+ | 自定义+BYOK | **Anthropic/OpenAI/Ollama/DeepSeek** |
| 沙盒执行 | Linux VM (gVisor) | Seatbelt/Landlock | 本地 | 6种后端 | Firecracker μVM | 无 |
| 多 Agent 并行 | Claude Code 多标签 | 6 线程并行 | 隔离路由 | 子 Agent 派发 | 8 并行 VM Agent | **Team 系统** |
| Computer Use | macOS 截屏+AX树 | macOS AX API | 无 | 无 | 无 | **feature flag** |
| MCP 支持 | 原生(创建者) | 客户端+服务端 | 支持 | 支持 | 支持 | **支持** |
| 插件/扩展市场 | MCPB (90+) | Plugin (90+) | Skills Hub | Skills Hub | VS Code 市场 | 无 |
| 语音输入 | 20+ 语言 | 支持 | 支持 | 支持 | 无 | 无 |
| 图像生成 | 无 | gpt-image-1.5 | 无 | 无 | 无 | 无 |
| 后台任务 | Cowork (VM) | Cloud 容器 | cron 调度 | cron 调度 | Background Agent | 无 |
| Git worktree | 无 | 原生 | 无 | 无 | 无 | **/batch 已有** |
| 记忆系统 | Projects + Memory | Memory (预览) | MEMORY.md | MEMORY.md + USER.md | Memory | **MemoryStore** |
| IDE 集成 | 内置 Claude Code | VS Code 扩展 | 无 | 无 | 本身是 IDE | VS Code scaffold |
| 定价 | $20-200/月 | 含 ChatGPT 订阅 | 免费开源 | 免费开源 | $20-200/月 | 免费开源 |

### 1.3 设计理念对比

| 产品 | 设计理念 | 核心哲学 |
|------|----------|----------|
| **Claude Desktop** | 透明智能 + 人在回路 | "不是 IDE 替代品，是 Agent 编排层"。三层能力递进：Connectors → Chrome → 屏幕控制 |
| **Codex Desktop** | Agent 指挥中心 | "不是编辑器，是监督平台"。委托 > 配对，异步多 Agent 是未来默认模式 |
| **OpenClaw** | 本地优先 + 隐私 | "龙虾之道"。配置驱动、自托管、不依赖云。消息平台是界面（WhatsApp/Telegram） |
| **Hermes** | 自我进化 Agent | "与你共同成长的 Agent"。从重复工作流自动生成新 Skill |
| **Cursor** | AI 原生编辑器 | "AI 作为一等公民"。从 VS Code 重建而非附加，速度和代码理解是核心价值 |
| **Aider** | Git 原生 | "Git 是主要契约"。终端伴侣，不做 IDE，你带编辑器，Aider 带模型循环 |

---

## 二、深度架构分析

### 2.1 Claude Desktop 架构 (最复杂)

```
┌─────────────────────────────────────────┐
│           Electron Shell                │
│  ┌─────────┐  ┌──────────┐  ┌────────┐ │
│  │ claude.ai│  │ Claude   │  │Connect-│ │
│  │ Web UI   │  │ Code Tab │  │ ors    │ │
│  └────┬─────┘  └────┬─────┘  └───┬────┘ │
│       │              │            │       │
│  ┌────▼──────────────▼────────────▼────┐ │
│  │         MCP Orchestrator            │ │
│  │  ┌──────────┐  ┌─────────────────┐  │ │
│  │  │ MCPB     │  │ Chrome MCP      │  │ │
│  │  │Extensions│  │ Bridge          │  │ │
│  │  └──────────┘  └─────────────────┘  │ │
│  └──────────────────────────────────────┘ │
│       │                                   │
│  ┌────▼──────────────────────────────────┐│
│  │   Linux VM (Apple Virtualization)     ││
│  │   Ubuntu 22.04 | 4CPU | 4GB RAM      ││
│  │   gVisor | MITM Proxy | Domain ACL   ││
│  │   ┌─────────────────────────────────┐ ││
│  │   │  coworkd (root daemon)          │ ││
│  │   │  File I/O | Shell | Tool exec   │ ││
│  │   └─────────────────────────────────┘ ││
│  └───────────────────────────────────────┘│
└──────────────────────────────────────────┘
```

**关键特点**: 三层网络安全 (syscall 阻断 → MITM 代理 → 域名白名单)，174 个服务端特性开关。

### 2.2 Codex Desktop 架构 (最优雅)

```
┌──────────────────────────────────────┐
│        Electron Shell (v40)          │
│  ┌───────────┐  ┌──────────────────┐ │
│  │ React UI  │  │ ProseMirror      │ │
│  │ (Renderer)│  │ Rich Text Editor │ │
│  └─────┬─────┘  └────────┬─────────┘ │
│        │                  │           │
│  ┌─────▼──────────────────▼─────────┐ │
│  │     70+ IPC Handlers             │ │
│  └─────────────┬────────────────────┘ │
│                │ JSON-RPC over stdio   │
│  ┌─────────────▼────────────────────┐ │
│  │   Rust App Server (codex-rs)     │ │
│  │   ┌──────────────────────────┐   │ │
│  │   │ Agent Core (same as CLI) │   │ │
│  │   │ Sandbox | Tools | MCP    │   │ │
│  │   │ Skills | Sessions        │   │ │
│  │   └──────────────────────────┘   │ │
│  └──────────────────────────────────┘ │
└──────────────────────────────────────┘
```

**关键特点**: 单一 Rust 二进制驱动 CLI/桌面/VS Code/Web/IDE 五个界面。共享核心 = 改进一次，全部受益。

### 2.3 Shannon 当前架构

```
┌──────────────────────────────────────┐
│       Tauri v2 Shell (实验)          │
│  ┌───────────────────────────────┐   │
│  │  index.html (基础聊天 UI)     │   │
│  │  无框架，原生 JS              │   │
│  └───────────────┬───────────────┘   │
│                  │ Tauri IPC          │
│  ┌───────────────▼───────────────┐   │
│  │  commands.rs (7 个 IPC 命令)  │   │
│  │  AppState | ChatMessage       │   │
│  │  TODO: 连接 QueryEngine      │   │
│  └───────────────────────────────┘   │
└──────────────────────────────────────┘
         │ 依赖
┌────────▼────────────────────────────┐
│  shannon-core (QueryEngine, etc.)   │
│  shannon-ui (TUI, 已有完整实现)      │
│  shannon-tools (Bash, File, etc.)   │
└─────────────────────────────────────┘
```

**当前状态**: Scaffold 阶段，7 个 IPC 命令，UI 是纯 HTML/CSS/JS，未连接 QueryEngine。

---

## 三、关键趋势与洞察

### 3.1 行业趋势

1. **Agent 指挥中心化**: 桌面应用不再是"聊天窗口"，而是 Agent 编排层 (Codex)、任务监督平台 (Claude Cowork)、多 Agent 协调器 (Cursor Background Agents)

2. **沙盒安全成为标配**: Claude 用 Linux VM + gVisor，Codex 用 Seatbelt/Landlock，Cursor 用 Firecracker μVM。用户信任 = 安全执行

3. **MCP 成为通用扩展标准**: 所有主要工具都支持 MCP。Claude 的 MCPB 格式提供了最佳安装体验

4. **多 Agent 并行执行**: 从实验性功能变为必需功能。Cursor 8 并行，Codex 6 线程，Shannon 已有 Team 系统

5. **Electron 主导但 Tauri 崛起**: 几乎所有桌面应用都用 Electron，但 Tauri 的体积优势 (~10MB vs ~300MB) 和性能优势正在被注意

### 3.2 Shannon 的独特优势

| 优势 | 说明 |
|------|------|
| **Rust 全栈** | 唯一的 Rust 全栈 AI 编码助手。性能、安全、内存效率天然优势 |
| **Tauri 框架** | 安装包 ~10MB vs Electron ~300MB。启动快，资源占用低 |
| **多 Provider** | 原生支持 Anthropic/OpenAI/Ollama/DeepSeek/GLM 等，不锁定单一 LLM |
| **终端优先** | 完整 TUI (ratatui) 已实现，桌面是补充而非替代 |
| **开源自托管** | MIT 许可，可审计，可自托管，适合企业和隐私敏感场景 |
| **已有基础设施** | Team 系统、MCP 支持、Hook 系统、LSP 集成、Permission 分类器 |

### 3.3 Shannon 的关键差距

| 差距 | 优先级 | 竞品参照 |
|------|--------|----------|
| **沙盒执行** | P0 | Codex (Seatbelt), Claude (VM) |
| **桌面 GUI** | P0 | Codex (Electron+React), Claude (Electron) |
| **后台任务** | P1 | Codex (Cloud), Claude (Cowork VM) |
| **插件市场** | P1 | Codex (90+ plugins), Claude (MCPB) |
| **Computer Use** | P2 | Codex (macOS AX), Claude (截屏+AX) |
| **语音输入** | P3 | Claude (20+ 语言), Codex |

---

## 四、项目结构决策分析

### 4.1 方案对比

| 维度 | 同一项目 (monorepo) | 新项目 |
|------|---------------------|--------|
| **代码复用** | 直接依赖 shannon-core/types/tools | 需要 crate 依赖或复制 |
| **构建复杂度** | 增加 (Tauri 需要 web 前端构建) | 隔离，不影响 CLI 构建 |
| **发布独立** | 与 CLI 耦合 | 独立版本、独立发布周期 |
| **团队协作** | 一个 repo | 可以但增加协调成本 |
| **CI/CD** | 共享管线 | 需要独立管线 |
| **Cargo workspace** | 自然集成 | 需要跨 repo 依赖管理 |

### 4.2 推荐方案: 同一项目 (monorepo)

**理由**:

1. **Codex 模式已验证**: OpenAI Codex Desktop 和 CLI 在同一 monorepo (`openai/codex`)，共享 Rust App Server 核心。这是最成功的先例

2. **Shannon 已是 workspace**: 当前 14 个 crate 的 workspace 结构天然支持添加 `shannon-desktop`。已有 scaffold

3. **核心复用最大化**: `shannon-core` (QueryEngine)、`shannon-tools` (工具)、`shannon-ui` (TUI 组件/渲染逻辑)、`shannon-mcp` (MCP 客户端) 都可直接复用

4. **Tauri 特性门控**: 通过 `features = ["tauri"]` 门控，CLI 构建不受影响 (`dist = false` 已设置)

5. **参考架构**:
   - Codex: `codex-rs/` (CLI + App Server) + Electron shell (同一 repo)
   - Hermes: Python runtime + Electron shell (同一 repo)
   - 都没有将桌面作为独立项目

**不推荐新项目的场景**: 如果桌面应用要使用完全不同的技术栈 (如 Swift/SwiftUI 原生 macOS)，则应独立项目。但 Shannon 已选择 Tauri，与 Rust workspace 天然兼容。

---

## 五、实施路线图

### 5.1 Phase 1: MVP 桌面应用 (4-6 周)

**目标**: 可用的桌面聊天应用，连接已有 QueryEngine

```
优先级:
P0 - 连接 QueryEngine (替换 TODO placeholder)
P0 - 流式响应显示 (SSE → Tauri events)
P0 - 基础对话 UI (Markdown 渲染、代码高亮)
P1 - Provider/Model 选择器
P1 - 工具执行展示 (Bash 输出、文件 diff)
P1 - 配置面板 (API key、provider)
```

**技术选型建议**:

| 组件 | 推荐方案 | 理由 |
|------|----------|------|
| 前端框架 | **React + TypeScript** | Codex/Claude 都用 React，生态最大 |
| Markdown 渲染 | react-markdown + remark-gfm | 代码高亮用 rehype-highlight |
| 状态管理 | Zustand | 轻量，适合桌面应用 |
| Tauri IPC | 事件驱动 (tauri::Emitter) | 流式响应需要 push 模式 |
| 构建工具 | Vite | Tauri 官方推荐 |

### 5.2 Phase 2: Agent 编排界面 (6-8 周)

```
P0 - Agent 面板 (查看运行中的 agent)
P0 - Team 管理 UI (创建团队、分配任务)
P0 - Diff 审查视图 (agent 修改的文件)
P1 - 多标签会话 (并行对话)
P1 - MCP 服务器管理 UI
P1 - 拖放文件/图片输入
```

### 5.3 Phase 3: 差异化功能 (8-12 周)

```
P0 - Tauri 沙盒 (Landlock/seccomp Linux, Seatbelt macOS)
P1 - 后台 Agent 执行 (系统托盘、通知)
P1 - 插件/Skill 市场 UI
P2 - 跨平台 Computer Use (不依赖 macOS AX)
P2 - 多模态输入 (语音 → Whisper → 文本)
P3 - 移动端派发 (手机发任务到桌面)
```

### 5.4 Shannon Desktop 差异化定位

| 差异点 | 实现方式 | 对标优势 |
|--------|----------|----------|
| **轻量级** | Tauri (~10MB) vs Electron (~300MB) | 安装快、占用少、启动快 |
| **Rust 原生安全** | 内存安全、无 GC 停顿 | 比 TypeScript/Python 后端更可靠 |
| **多 Provider 不锁定** | 原生支持 10+ LLM provider | Claude 只支持 Anthropic，Codex 只支持 OpenAI |
| **终端 + 桌面双模式** | 同一核心，TUI + GUI 两种界面 | 其他产品通常只有一种 |
| **开源自托管** | MIT 许可，可审计 | 企业可部署在私有环境 |
| **跨平台原生体验** | Tauri 原生窗口 vs Electron Chromium | 更接近原生应用的体验 |

---

## 六、结论

### 项目结构: 同一 monorepo

Shannon Desktop 应在当前 `shannon-code` 项目的 `crates/shannon-desktop/` 中实现，复用 Codex 的 "单一核心 + 多界面" 模式。

### 技术路线: Tauri v2 + React

保留 Tauri v2 (已有 scaffold)，前端升级为 React + TypeScript + Vite，后端通过 Tauri IPC 事件驱动连接 `shannon-core` 的 QueryEngine。

### 差异化策略: 轻量 + 多Provider + 开源

不与 Claude/Codex 拼功能广度 (Computer Use、云 Agent)，而是聚焦:
1. **最轻量的桌面 AI 编码助手** (Tauri ~10MB)
2. **最开放的 LLM 支持** (不锁定单一 Provider)
3. **最可审计的开源方案** (Rust 全栈，可自托管)

这是 Cursor/Claude/Codex 都没有覆盖的市场空白。
