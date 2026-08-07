# P2-8 Shannon VSCode 扩展选型 Spike (S0)

> **Wave 6 / P2-8 / S0 spike** — 对比 4 家 VSCode AI 编程扩展,产出选型 + MVP 设计草图。
> 日期:2026-08-04 · 编写人:Wave 6 S0 spike 子代理 · 状态:**只读仓库 + 产出本文件**

---

## 0. TL;DR(3 段)

**核心结论**:
1. **继续做"VSCode 扩展 + 本地 HTTP API"形态**,不模仿 Cursor(改 IDE)的路线。Cursor 的"内嵌 diff / Cmd-K 内联改写 / 多文件 Composer"是体验标杆,但它绑死了"必须装 Anysphere 定制版 VSCode";而 Shannon 走的是"标准 VSCode 扩展 + `shannon serve` 后端"——更可移植、跨 IDE(JetBrains 后续可共用同一份 HTTP API)、不绑死 LSP/编辑器内部 API。
2. **通信迁移到 `shannon serve` HTTP API**(commit `3ed22799`,P2-7 已落地)替代 legacy 的 NDJSON 子进程。这样可以:(a) 复用已经在 `api_server.rs` 用 axum 暴露的 `/api/query/stream` SSE + `/api/approval/respond` + WebSocket(见 shannon-core/src/api_server.rs),(b) 跨设备/远程共享同一会话,(c) 与 Shannon desktop Tauri 复用同一后端。**保留 NDJSON 子进程作为离线 fallback**(没有运行 `shannon serve` 时降级),做到兼容 legacy 与新装用户都能用。
3. **参考 Cline 的"Plan/Act + 每步审批"** 而非 Claude Code 的"长会话自主执行":Cline 是 Apache-2.0 / 5M+ 安装 / 30+ provider / 显式 per-step approval——和 Shannon 当前"命令 + 自动跑"的形态最像,且 MCP 集成方案已成熟(可继续接 Shannon 自己的 5 个 SaaS MCP)。**不直接 fork 任何一家**(license 与架构差异都太大),但分别借鉴其最强一面。

**推荐方案**:
- UI:`WebviewView` (Sidebar chat) + 内联命令面板(quick action)
- 通信:`shannon serve` HTTP API(主)+ NDJSON 子进程(降级)
- 上下文:`@-mention` 选区 + active file + tree-sitter repo map(P1-4 已有)
- 认证:VSCode SecretStorage(替代已弃用的 keytar,见 [VSCode v1.80 release notes](https://code.visualstudio.com/updates/v1_80))
- MCP:复用 Shannon 现有 5 个 SaaS MCP server(Slack / GitHub / Jira / Notion / Linear),扩展通过 `claude_desktop_config.json` 风格的 JSON

**风险**:
- HTTP API 鉴权:目前只确认 `test_app_with_auth` 测试分支,需 S1 验证 token 实际签发流程
- tree-sitter repo map 的 VSCode 端是否需要重复实现 / 直接调用 Rust HTTP 端点
- 跨平台(ubuntu/macos/windows)SecretStorage 兼容性需在 CI 矩阵(`.github/workflows/ci.yml` 已配)实测
- 与 shannon-desktop 共享后端时端口/进程生命周期需明确

---

## 1. 对比表(4 列 × 12 行)

> 数字与时间口径见 §2 详细引用(2025-2026 资料,部分官方页面以官方 docs 为准)。

| 维度 | **Claude Code VSCode 扩展** | **Cline**(原 Claude Dev) | **Continue.dev** | **Cursor**(非扩展,标杆) |
|---|---|---|---|---|
| **1. 通信架构** | 复用 CLI 子进程(`claude` CLI 内置,共享 `~/.claude/settings.json` 与同一 session store);[VSCode 扩展是 CLI 的 UI 外壳](https://www.eesel.ai/blog/claude-code-vs-code-extension) | 独立进程:扩展直接 HTTP 调 LLM provider(30+);每条工具调用是 round-trip JSON-RPC;无中间服务 | 独立进程:扩展直接 HTTP 调 provider;`config.yaml` 声明 `mcpServers`([docs.continue.dev](https://docs.continue.dev/customize/deep-dives/mcp)) | 自家 fork 的 VSCode(Cursor 3 + Agents Window);后端是闭源 HTTP API + Cloud VM(Background Agents) |
| **2. 配置方式** | Claude 账户 / Pro $20 · Anthropic API key / Bedrock / Vertex;`~/.claude/settings.json` 跨 CLI/扩展共享 | BYOK 任意 provider(Anthropic / OpenAI / Gemini / 30+);"Cline Provider"按 token cost 转售;**$0/月 订阅** | 单一 `config.yaml`(`~/.continue/config.yaml` 或 `.continue/config.yaml` per-repo);支持 Ollama / vLLM 本地 | Cursor 账户 / Pro $20 / Business $40;Cursor Settings UI;BYOK 不开放 |
| **3. 会话状态** | `~/.claude/` 下 JSONL 持久化;CLI 与 VSCode 共享同一 store,可一开一关 | Cline 任务 = 临时任务;无跨设备同步;`Checkpoints` 是 git 风格的本地快照 | 关闭即丢,无云端;`config.yaml` 持久化"角色"配置 | Cursor Cloud 同步会话(Pro+);Background Agents 在云 VM 跑 |
| **4. 编辑集成** | Sidebar + `@-mention` 选区 + VSCode 原生 diff viewer([eesel.ai](https://www.eesel.ai/blog/claude-code-vs-code-extension));部分功能(全 diff、bash `!` shortcut、Tab)只 CLI 有 | Sidebar + Plan/Act 双模式 + per-file diff + 自动 Apply 按钮;Computer Use 浏览器验证 | Sidebar + Chat / Edit / Agent 三模式;每模式可路由不同模型(per-role routing) | **Tab(自研 <100ms 预测)+ Cmd+K(内联)+ Cmd+L(Chat)+ Cmd+I(Composer)四件套**;Composer 2 一次改多文件,先 diff 后 apply |
| **5. 上下文收集** | `@file` / `@-mention` 选区;`CLAUDE.md` 项目级指令;`/add-dir` 跨目录;不直接用 LSP symbols | `@` 提及文件 / 选区 / URL;Plan mode 会自动 regex 扫全仓;**不直接接 LSP** | `@` 提及;内置 `code` / `docs` / `diff` / `folder` / `problems` 等 context provider;`config.yaml` 可声明自定义 | 整个 repo 做 embedding + AST indexing("context engine" 宣传点);Composer 2 训练目标含"codebase-wide semantic search" |
| **6. MCP 支持** | 完整支持,6 种 transport(stdio / SSE / HTTP streaming / WebSocket / IDE-internal / SDK);plugin 可贡献 MCP servers | 完整支持 + **MCP Marketplace**;可在扩展内"add tool"自动装;stdio / SSE / streamable-HTTP([docs.continue.dev MCP guide](https://docs.continue.dev/customize/deep-dives/mcp)) | 完整支持,`mcpServers` 段在 `config.yaml`;stdio / SSE / streamable-HTTP | **Per-agent scoping** MCP server;`Tools & Integrations` 面板;模型可用 MCP 工具 |
| **7. 认证** | OAuth(Pro 账户)或 `ANTHROPIC_API_KEY`;需 `code .` 从带 env 的 shell 启动(否则 loop login) | BYOK:API key 存扩展配置(需 SecretStorage);`Cline Provider` 是代收代付 token | BYOK:API key 存 `config.yaml` 引用 `${{ secrets.X }}`(团队场景)或明文(本地) | Cursor 账户 OAuth;Cloud 同步会话与设置 |
| **8. 离线模式** | CLI 可配本地 / 自定义 endpoint(智谱/方舟教程存在);**扩展完全依赖 CLI,CLI 必须能跑** | 完全支持本地模型(Ollama / LM Studio / llama.cpp);无 cloud 依赖 | **主打**:Ollama / vLLM / llama.cpp 全自托管,零出网 | 闭源;不能离线(必须连 Cursor Cloud) |
| **9. 发布与盈利** | Marketplace `anthropic.claude-code`;Anthropic 官方;[Anthropic 2.5B ARR / 130K+ stars / 10M+ 扩展下载](https://www.eesel.ai/blog/claude-code-vs-code-extension)(2026-02) | Marketplace `saoudrizwan.claude-dev`;**Apache-2.0**;**[4.8M+ 安装 / 61.2K GitHub stars / v3.81](https://marketplace.visualstudio.com/items?itemName=saoudrizwan.claude-dev)**;企业版(SSO/VPC)单独卖 | Marketplace `Continue.continue`;Apache-2.0;继续支持 per-role 模型路由,无订阅 | Cursor 1.x / 2.0(2025-10) / 3.0(2026-04)/ Composer 2(2026-03);闭源;[$50B 估值 / $2B ARR / 1M+ 付费](https://tech-insider.org/cline-vs-cursor-2026) |
| **10. 性能** | CLI 启动快;Opus 4.8 1M context;流式输出走 CLI → 扩展渲染 | 1-2s 启动;per-step 审批会让长任务显得"啰嗦"但每步可控 | 启动 < 1s;per-role 路由让 Tab 走本地小模型(快)、Agent 走大模型 | Tab 模型宣称 < 100ms;Composer 2 4× 快于同级模型;Agents Window 多 agent 并行 |
| **11. 扩展点(API)** | Skills / Subagents / Hooks / Plugins / MCP(官方 6 transport);permission system 多层;Checkpoints | `.clinerules/` + MCP Marketplace + Computer Use;开发者可以造"add tool" | `config.yaml` 全部:models / rules / prompts / docs / mcpServers / context providers;**扩展点 = YAML** | 闭源,无第三方扩展;只暴露 Composer MCP 与 BugBot API |
| **12. 失败模式** | 网络断:CLI retry;token 失效:login loop 需重启 shell;大文件:1M context 仍可能超;取消:`Esc` 触发 hook | 网络断:每步失败需手动 retry;token 失效:Settings 重输;大文件:逐文件审批可中断;**YOLO 模式可一次过** | 网络断:配置错立即报;token 失效:config 改;大文件:`config.json` 加载偶有 bug([issue #9587](https://github.com/continuedev/continue/issues/9587));取消:无原子 | 网络断:Composer 卡死需刷新;token 失效:重登;Composer diff 偶发"missing apply button"([forum 52319](https://forum.cursor.com/t/no-longer-seeing-inline-diffs-from-composer/52319));大文件:1M context |

> 表格只是"一句话定位",细节见 §2。**Cursor 不是扩展**这一事实本表已注明,但它的体验设计反向影响我们对 VSCode 扩展的取舍(见 §3.4)。

---

## 2. 每家深入分析(200–400 字 + "对 Shannon 的启发")

### 2.1 Claude Code VSCode 扩展(Anthropic 官方)

Claude Code 扩展在 2026-02 突破 [10M+ 下载、$2.5B ARR、130K+ GitHub stars](https://www.eesel.ai/blog/claude-code-vs-code-extension),与 Cursor / Codex 形成三足鼎立。它的核心架构是 **"CLI 是引擎、扩展是 UI 外壳"**:两者共享 `~/.claude/settings.json` 与同一 session store,扩展几乎不存任何业务状态。

- 通信:扩展是 Terminal 端 `claude` 进程的 1:1 包装,所有 LLM/MCP 调用都走 CLI 子进程。
- 差异化特性:VSCode 原生 diff viewer(在 CLI 不可用)、`@-mention` 选区(CLI 没有)、session history with tabs;`--dangerously-skip-permissions`、bash `!` shortcut、Tab、multi-repo `/add-dir` 仅 CLI 端有。
- MCP 体系成熟:6 种 transport(stdio / SSE / HTTP streaming / WebSocket / IDE-internal / SDK)、plugin 可贡献 MCP、subagent 可调子 skill,security model 显式分层。
- 局限:(a) login 经常循环(从非 shell 启动 VSCode 时不继承 `ANTHROPIC_API_KEY`);(b) "Spark icon 消失"是 FAQ 头部条目,只在文件打开时显示。

**对 Shannon 的启发**:
- ✅ "扩展 = 薄壳,后端 = 厚引擎"是正确分层,Shannon 也应如此:扩展不存业务状态,所有逻辑在 `shannon serve` HTTP API。
- ✅ `settings.json` 单点配置跨 CLI/扩展/Desktop 共享——Shannon 已有 `~/.config/shannon/config.toml`,可继续作为唯一 source of truth。
- ❌ 不照搬"长会话 1M context 自主执行"——Cline 的"per-step approval"更符合 Shannon 现有"命令驱动 + 审批"哲学,且更安全。

来源:[Inside Claude Code, The Architecture Behind Tools, Memory, Hooks, and MCP (penligent.ai)](https://www.penligent.ai/hackinglabs/inside-claude-code-the-architecture-behind-tools-memory-hooks-and-mcp),[How Claude Code works (code.claude.com)](https://code.claude.com/docs/en/how-claude-code-works),[eesel.ai blog 2026-04](https://www.eesel.ai/blog/claude-code-vs-code-extension)。

---

### 2.2 Cline(原 Claude Dev,Apache-2.0)

[Marketplace 4.8M+ 安装、61.2K GitHub stars、v3.81、Apache-2.0](https://marketplace.visualstudio.com/items?itemName=saoudrizwan.claude-dev),企业版(SAML/OIDC/VPC/自托管)单独卖。扩展 ID 仍是 `saoudrizwan.claude-dev`——因为改名时已经积累了大量用户。

- 通信:扩展直接 HTTP 调 LLM provider(30+ 支持),**无中间 server 进程**;每条工具调用是显式 round-trip。
- 核心 UX:**Plan/Act 双模式**——Plan 只读、生成方案;Act 执行,每步需用户审批(类似 Shannon 当前"命令确认")。`.clinerules/` 支持文件级治理,conditional rules 把团队规范入库。
- 上下文:`@-mention` 选区 / 文件 / URL;Plan mode 自动 regex 扫全仓;**不直接接 LSP**(依赖 grep)。
- MCP Marketplace 是杀手锏——可以在扩展 UI 内"add tool",自动创建 + 配置 + 安装一个 MCP server。Computer Use 让 agent 跑真实浏览器做验证。
- 限制:每步审批对长任务偏慢,虽然有 YOLO / Lazy Teammate 模式缓解。

**对 Shannon 的启发**:
- ✅ **MCP Marketplace 模式值得参考**——Shannon 已有 5 个 SaaS MCP(Slack/GitHub/Jira/Notion/Linear,P1-3 完成),可以做一个"已集成 SaaS"列表,用户在扩展侧一键启用。
- ✅ Plan/Act 与 Shannon 现有"ask_user / send_prompt"流程契合:Plan 阶段 = 调 `shannon api plan`;Act = 用户确认后调 `query/stream`。
- ✅ Apache-2.0 + 企业版(SSO/VPC)双轨 = Shannon 未来 ToB 路径样板。
- ❌ 不照搬"无中间 server"——Shannon 已有 P2-7 HTTP API,直接复用,免去每条工具调用走 provider 的延迟与凭据管理。

来源:[Cline for VS Code: Free AI Coding Agent Setup Guide (deployhq.com)](https://www.deployhq.com/guides/cline),[Cline vs Cursor 2026 (tech-insider.org)](https://tech-insider.org/cline-vs-cursor-2026),[Cline vs Cursor (morphllm.com)](https://www.morphllm.com/comparisons/cline-vs-cursor),[Marketplace listing](https://marketplace.visualstudio.com/items?itemName=saoudrizwan.claude-dev)。

---

### 2.3 Continue.dev(Apache-2.0,Y Combinator)

Continue 是"最可配置的开源 AI 编码助手",主张"One YAML file, full control"。VSCode + JetBrains 双 IDE 同一后端,15+ provider,支持 Ollama / vLLM / llama.cpp 自托管。

- 配置:`~/.continue/config.yaml` 单一文件声明 models / rules / prompts / docs / mcpServers / context providers;per-repo `.continue/config.yaml` 可覆盖。`config.json` 已被弃用(legacy 仍能加载)。
- 角色路由:per-role(chat / edit / apply / autocomplete / embed / rerank)分别指派不同模型——例如 Tab autocomplete 走本地小模型、Agent 走 frontier hosted 模型。
- 上下文:context providers 是一等公民;`docs` 段可声明 doc 站点自动 crawl;`problems` 接 VSCode 诊断。
- MCP:`mcpServers` 在 `config.yaml`;stdio / SSE / streamable-HTTP 三种 transport;`connectionTimeout` / `requestOptions` 可调。
- 局限:docs 偶有 stale(StackOverflow 上"Z.ai GLM 不在 dropdown"典型问题反映"必须手动 select Local Config"——扩展 UI 与 config 同步的稳定性问题),Linux 上没目录时加载失败([issue #9587](https://github.com/continuedev/continue/issues/9587))。

**对 Shannon 的启发**:
- ✅ "per-role 路由"概念在 Shannon 可降维:同一次 `shannon serve` 会话里,quick action(改一函数)走 Sonnet,长任务(全仓重构)走 Opus,通过 `shannon.toml` 配置。
- ✅ context provider 抽象可对 Shannon 的 tree-sitter repo map(P1-4)友好——直接当一个"repo-map" provider,与其他 providers(code / docs / problems)并列。
- ❌ 不要 100% 学"YAML 一统天下"——Shannon 用户已经在 `shannon.toml` 写好配置,VSCode 端用 GUI 包装 `package.json` contributes.configuration 即可,不要引入第二份 YAML 事实来源。

来源:[Continue.dev Rules & Config Complete Setup Guide (cursor-alternatives.com)](https://cursor-alternatives.com/blog/continue-dev-rules),[Continue.dev Deep Dive (digitalapplied.com)](https://www.digitalapplied.com/blog/continue-dev-deep-dive-open-source-ai-coding-assistant-2026),[config.yaml reference (docs.continue.dev)](https://docs.continue.dev/reference),[MCP guide (docs.continue.dev)](https://docs.continue.dev/customize/deep-dives/mcp),[issue #9587 (github.com/continuedev/continue)](https://github.com/continuedev/continue/issues/9587)。

---

### 2.4 Cursor(非扩展,作为体验标杆)

Cursor **不是 VSCode 扩展**——它是 [Anysphere 维护的 VSCode fork](https://tech-insider.org/cline-vs-cursor-2026),需要单独下载安装(像 IDE),运行自己的 Electron build、自己的 settings sync、自己的 marketplace(继承大部分 VSCode 扩展)。但它对"AI-first 编辑器"应该长什么样定义了一套事实标准:

- **Tab(自研 < 100ms 预测)**——预测"下一个编辑"而不是"下一个 token",与 Copilot 截然不同。
- **Cmd+K(Inline Edit)**——选中代码 + 一句话指令,Cursor 返回 unified diff,Apply 后才写盘。
- **Cmd+L(Chat)**——对话,可 `@file` 提及;Chat 不默认改文件,需点 Apply 才会变成 Inline Edit。
- **Cmd+I(Composer)**——多文件编辑,先 diff 后 apply,per-file accept/reject。Cursor 3 之后"Composer 先 preview 再 approve"成为默认。
- **Composer 2(2026-03 自研模型)——宣称 4× 快于同级 frontier 模型,30s 内完成大多数 turn**;"trained with codebase-wide semantic search"。
- **Background Agents(云 VM)**——分配任务,VM 跑,回 PR;多 agent 并行。
- **Context engine**——embedding + AST indexing 全仓;query 时召回 top-k。
- **局限**(2026-04 论坛热帖):Composer diff 偶发"no apply button"([forum 52319](https://forum.cursor.com/t/no-longer-seeing-inline-diffs-from-composer/52319))。

**对 Shannon 的启发**:
- ✅ **"先 diff 后 apply"是铁律**——legacy 扩展已经实现 `DiffViewer`(`shannon-diff-original` / `shannon-diff-proposed` 两个 content provider),要保留并强化。
- ✅ **per-file accept/reject 多文件编辑**——Shannon 后端 `shannon serve` 返回"changes"列表(每项含 path + diff),扩展侧用 VSCode diff editor 一个个展示、批 accept / reject,正是 Composer 的工作流。
- ✅ **"Chat 默认不改文件,显式 Apply 才改"**——这条原则非常重要,legacy 扩展 `acceptAllChanges` 命令能保留。
- ✅ **@-mention + 选区 + 全仓 search** 三件套——Shannon P1-4 tree-sitter repo map 正好对应 Cursor 的"全仓 search"。
- ❌ **不复制"自研模型"或"云 VM agent"**——Shannon 不在这个赛道,不做自研 LLM,云 VM 也不在 roadmap。

来源:[Cursor 2026: Composer, Agent Mode, MCP & Background (deployhq.com)](https://www.deployhq.com/guides/cursor),[Cursor 3 Deep Dive (digitalapplied.com)](https://www.digitalapplied.com/blog/cursor-3-deep-dive-agents-composer-review-2026),[App Development with Cursor 2026 (dev.to)](https://dev.to/asad1/app-development-with-cursor-in-2026-the-definitive-technical-guide-27m6),[What Is Cursor 2026 (developersdigest.tech)](https://www.developersdigest.tech/blog/what-is-cursor-ai-editor-2026),[Cline vs Cursor 2026 (tech-insider.org)](https://tech-insider.org/cline-vs-cursor-2026),[Composer inline diff 失效 forum](https://forum.cursor.com/t/no-longer-seeing-inline-diffs-from-composer/52319)。

---

## 3. Shannon 推荐方案(MVP)

### 3.1 通信架构(双通道:HTTP 主 + NDJSON 降级)

```
                          ┌────────────────────────────────────────┐
                          │        shannon serve (axum)           │
                          │  POST /api/query/stream  (SSE)        │
                          │  POST /api/approval/respond            │
                          │  WS   /ws  (增量 token + 工具事件)     │
                          │  GET  /v1/sessions  (跨设备)           │
                          │  ★ 现有 P2-7 落地(3ed22799)            │
                          └──────────────────┬─────────────────────┘
                                             │ HTTP+Token
                ┌────────────────────────────┼────────────────────────────┐
                │                            │                            │
        ┌───────▼────────┐           ┌───────▼────────┐         ┌───────▼────────┐
        │  VSCode 扩展    │           │  Desktop Tauri │         │  CLI / Web     │
        │  (P2-8 新做)    │           │  (P2-5 已有)   │         │  (P2-7 同源)   │
        │  WebviewView   │           │  assistant-ui  │         │                │
        └────────────────┘           └────────────────┘         └────────────────┘
                ▲ 离线降级 ↓
                │  检测 shannon serve 不可达 →
                │  fallback `shannon` CLI 子进程 + NDJSON (legacy v3 模式)
                ▼
        ┌──────────────────────────────┐
        │  shannon CLI (Rust)          │
        │  stdin/stdout NDJSON stream  │
        │  ★ legacy 已实现,保留兼容   │
        └──────────────────────────────┘
```

**关键设计点**:
- **优先 `shannon serve`**:扩展启动时 `GET http://127.0.0.1:<port>/healthz`(端口从 `shannon.toml` `[serve] port` 读),3 次重试 × 200ms 失败后转 NDJSON fallback。
- **认证**:`shannon serve` 已支持 token(见 `test_app_with_auth` 测试用例,`shannon-core/src/api_server.rs`);扩展把 token 存 VSCode SecretStorage,每次请求 `Authorization: Bearer <token>` 头。
- **流式输出**:SSE 优先(`/api/query/stream`),如果 `shannon serve` 版本过老还没 SSE,降级到 WebSocket(`/ws`)。
- **取消**:`AbortController` + `delete` SSE 流;映射到 legacy `Esc → 发送 stop NDJSON 消息`。

### 3.2 UI 形态

| Surface | 实现 | 触发 | 用途 |
|---|---|---|---|
| **Sidebar Chat** | `WebviewView` + `viewType=shannon.chat` | Activity Bar 图标 / `Ctrl+Shift+L` | 主对话界面,聊需求、读回答、看 plan |
| **Inline Quick Action** | 注册 `shannon.quickAction` command + 选区右键菜单 | `Ctrl+K` (类 Cursor Cmd+K) | 选中代码 → 弹小输入 → 出 diff → Apply/Reject |
| **Diff Editor** | `vscode.diff` + 临时 content provider | "View Diff" 按钮 | 复用 legacy `DiffContentProvider` 改造 |
| **Status Bar** | `StatusBarItem` | 永久 | 显示"已连接 / 离线 / token 失效"状态 |
| **Output Channel** | `createOutputChannel('Shannon')` | 自动 | 流式输出、错误日志、Panic 记录(对应 ADR-0008 P2-6) |

> **不**做 Composer 风格"多文件 agent panel"——那需要云 VM 或本地多 worker 编排,**留给 P3**;MVP 阶段所有多文件编辑通过 Chat → Plan → Accept/Reject 流程完成。

### 3.3 命令清单(对照 legacy 9 个)

| Legacy 命令 | MVP 决策 | 新命令(可选) |
|---|---|---|
| `shannon.openChat` | ✅ 保留 → 触发 Sidebar Chat 显示 | |
| `shannon.sendPrompt` | ✅ 保留 → 在 Webview 内 Send / `Enter` | |
| `shannon.stopGeneration` | ✅ 保留 → `Esc` + AbortController | |
| `shannon.showPendingChanges` | ✅ 保留 → 改名为 `shannon.reviewChanges`,弹 diff editor 列表 | |
| `shannon.acceptAllChanges` | ⚠️ 改为 `shannon.acceptChange` + `shannon.acceptAllChanges`;单文件优先 | |
| `shannon.rejectAllChanges` | ⚠️ 同上,`shannon.rejectChange` + `shannon.rejectAllChanges` | |
| `shannon.openSettings` | ✅ 保留 → 打开 VSCode Settings `@modified:shannon` | |
| `shannon.newChat` | ✅ 保留 → `Ctrl+Shift+N` | |
| `shannon.clearHistory` | ✅ 保留 → 触发 `/clear` 到 `shannon serve` | |
| | | `shannon.login`(OAuth / API key 双流程) |
| | | `shannon.enableMcp`(弹 picker,启用/禁用某个 SaaS MCP) |
| | | `shannon.explainSelection`(选区解释,Inline Quick Action 子命令) |
| | | `shannon.refactorSelection`(同上) |
| | | `shannon.runRepoMap`(手动触发 tree-sitter 全仓 refresh) |

### 3.4 配置面板内容(package.json contributes.configuration)

```jsonc
{
  "contributes": {
    "configuration": {
      "title": "Shannon",
      "properties": {
        "shannon.serve.url":       { "type": "string",  "default": "http://127.0.0.1:8742", "description": "shannon serve HTTP endpoint" },
        "shannon.serve.authToken": { "type": "string",  "default": "",                      "description": "Bearer token;留空则用 SecretStorage 中的 'shannon.apiKey'" },
        "shannon.serve.useFallbackCli": { "type": "boolean", "default": true,             "description": "shannon serve 不可达时是否降级到 NDJSON 子进程" },
        "shannon.cli.path":        { "type": "string",  "default": "shannon",                "description": "shannon CLI 路径(降级用)" },
        "shannon.model.provider":  { "type": "string",  "default": "auto",                  "enum": ["auto","anthropic","openai","ollama"], "description": "默认 provider" },
        "shannon.model.name":      { "type": "string",  "default": "auto",                  "description": "默认 model 名;auto 走 shannon serve 端的选择" },
        "shannon.context.repoMap": { "type": "boolean", "default": true,                   "description": "自动调用 tree-sitter repo map" },
        "shannon.context.maxFiles":{ "type": "number",  "default": 20,                     "description": "@-mention 一次最多带几个文件" },
        "shannon.ui.streaming":    { "type": "boolean", "default": true,                   "description": "SSE 流式输出" },
        "shannon.ui.diffStyle":    { "type": "string",  "default": "split",                "enum": ["split","unified"] },
        "shannon.approval.mode":   { "type": "string",  "default": "perChange",            "enum": ["perChange","perSession","auto"], "description": "类 Cline 的 per-step approval" },
        "shannon.mcp.enabled":     { "type": "array",   "default": ["slack","github","jira","notion","linear"], "description": "启用的 SaaS MCP server ID" }
      }
    }
  }
}
```

### 3.5 上下文收集策略

| 来源 | 触发 | 端到端路径 |
|---|---|---|
| **活动编辑器选区** | `Ctrl+K` / Inline Action | 扩展读 `vscode.window.activeTextEditor.selection` → 加入 prompt 头部 |
| **`@-mention` 文件** | Sidebar Chat 输入 `@` | 触发 `vscode.window.showQuickPick`,用户选文件 → 注入 `{type: file, path, snippet}` |
| **`@-mention` 目录** | Sidebar Chat 输入 `@folder:` | 树形 picker → 注入 folder 列表(限制 `maxFiles`) |
| **Tree-sitter repo map** | 每次 send prompt(开关可配) | 扩展调 `GET /v1/repo-map?workspace=...&focus=paths=...`,后端用 P1-4 已有实现 |
| **VSCode 诊断** | `@problems` | 读 `vscode.languages.getDiagnostics()`,注入 errors / warnings 摘要 |
| **Git 状态** | `@git` | `git status` + `git diff --stat` 注入(避免扩展端直接 git 命令,改走 `shannon serve` 的 `repo_info` 端点) |
| **终端输出** | `@terminal:last` | 读 `vscode.window.activeTerminal`,可选注入(用户敏感,默认 off) |

> **不**直接用 `vscode-languageclient` 跑 LSP——LSP 诊断已通过 `vscode.languages.getDiagnostics` 暴露,不重复发明轮子。

### 3.6 认证方式

- **API key / Bearer token** → 存 VSCode **SecretStorage**(key:`shannon.apiKey`),**不**走 keytar([已弃用,见 VSCode v1.80 release notes](https://code.visualstudio.com/updates/v1_80))。
- **OAuth(若有)** → 扩展用 `vscode.authentication.getSession('shannon', [...])`,系统 keychain 存储 refresh token,跨 macOS Keychain / Linux libsecret / Windows Credential Vault 统一。
- **多账户切换** → `vscode.authentication.getSession(..., {forceNewSession: true})` 走 OAuth 重新登录;API key 走 Settings UI 重输。
- **不跨设备同步凭据**——token 是设备本地;若要跨设备,走 `shannon serve` 自己的账户体系。

### 3.7 离线 fallback

| 触发 | 行为 |
|---|---|
| `shannon serve` 启动成功 | 走 HTTP API(主路径) |
| `shannon serve` 不可达 + `useFallbackCli=true` | spawn `shannon` 子进程 + NDJSON(legacy 模式) |
| `shannon serve` 不可达 + `useFallbackCli=false` | Sidebar 显红色 banner:"Shannon serve 未运行,启动 `shannon serve` 或启用 fallback" |
| HTTP 401/403 | 清 SecretStorage 里的 token,触发 `shannon.login` 命令 |
| HTTP 5xx / 网络断 | 每条请求 retry 3 次 × 指数退避,失败转 fallback |
| `shannon` CLI 也不在 PATH | 显错误提示用户安装 Shannon |

### 3.8 测试与 CI

- 单元测试:vitest + vscode mock(legacy 已有 1 个 test 目录,沿用)
- 集成测试:`@vscode/test-electron` 跑真实 VSCode 实例,在 CI 的 ubuntu/macos/windows 矩阵(`.github/workflows/ci.yml` 已配)上验证
- e2e:启动 `shannon serve`,用 Playwright / `@vscode/test-web` 跑"发送 prompt → 收到流式 → accept change"路径

---

## 4. 风险与未决(5 条)

1. **`shannon serve` 鉴权真实签发流程未在 S0 验证**:只看到 `test_app_with_auth` 测试分支,S1 需确认 token 是启动时生成 / 用户显式 `shannon login` 生成 / 临时匿名。决定扩展侧"哪里取 token"的代码路径。
2. **Tree-sitter repo map 端点暴露**:`shannon-core` 已有 repo map 逻辑(P1-4),但 HTTP 路由是否已暴露 `GET /v1/repo-map` 待 S1 确认;若未暴露,需评估"扩展端直接调 `shannon` 子命令"还是"先在 serve 端加路由"——后者更干净。
3. **跨平台 SecretStorage**:macOS / Windows 验证充分,Linux libsecret 在 headless CI 环境需 mock 或装 gnome-keyring。CI 矩阵需额外步骤。
4. **与 shannon-desktop 端口/进程冲突**:Tauri desktop 也会启 `shannon serve`;扩展需识别"是 desktop 启的"还是"用户手动启的",避免重复 spawn。短期方案:扩展优先 attach 已存在的 serve(查端口 8742 是否 listen),不主动拉起。
5. **NDJSON ↔ HTTP 语义差**:legacy 一些自定义消息类型(如"pending change with metadata")在 HTTP API 可能没有等价物,S1 需在 spike 中列差异表,决定哪些 NDJSON 消息不进 MVP。

---

## 5. 下一步动作(S1 MVP 拆任务,目标 2w 内交付)

> 全部是"待办",S0 不执行;S1 启动时按此分解到 TodoWrite。

| # | 任务 | 估时 | 依赖 | 验收 |
|---|---|---|---|---|
| S1.1 | 验证 `shannon serve` token 实际签发流程,产出 1 页 handoff | 0.5d | 无 | 文档含 "我用什么命令拿 token" 一节 |
| S1.2 | 在 `shannon serve` 端补 / 验证 `GET /v1/repo-map` 端点(若未存在) | 1d | S1.1 | curl 拿到 JSONL,字段对齐 P1-4 |
| S1.3 | 列出 NDJSON → HTTP 消息差异表,标"进 MVP / 推迟 / 砍" | 0.5d | S1.1 | markdown 表格 + 决策理由 |
| S1.4 | 起 `crates/shannon-vscode/` 新 crate(或 `editors/vscode/` 复用 legacy 目录),写 package.json + tsconfig + .vscodeignore;**改造** legacy 9 个命令为新命令清单 | 1d | 无 | 扩展能 `pnpm install && code --extensionDevelopmentPath=.` 起来 |
| S1.5 | 实现 `ShannonServeClient` (HTTP / SSE / WS),含 fallback 探活 | 2d | S1.1, S1.4 | 单测覆盖健康检查 / 重试 / 取消 / 401 |
| S1.6 | 实现 Sidebar Chat `WebviewView`,复用 assistant-ui 设计 token(P2-5 已建) | 2d | S1.5 | Webview 渲染 chat 流,Send 按钮可用 |
| S1.7 | 实现 Inline Quick Action + Diff Editor 集成(改造 legacy `DiffViewer`) | 1.5d | S1.5 | "选中 + Ctrl+K + 指令 + Apply" 端到端跑通 |
| S1.8 | 写 SecretStorage 包装 + 登录/登出命令 | 0.5d | 无 | 单元测试 3 平台 mock |
| S1.9 | 配置面板(直接编辑 `package.json` contributes.configuration)+ Settings UI 跳转 | 0.5d | 无 | 11 个 setting 全部生效 |
| S1.10 | CI 矩阵跑 vitest + @vscode/test-electron(e2e 3 平台) | 1d | S1.5–S1.8 | CI green on ubuntu/macos/windows |
| S1.11 | 写 marketplace 描述、icon、README、CHANGELOG,准备 first publish | 0.5d | 全部 | marketplace listing 草稿就位 |
| **合计** | | **11 人天 ≈ 2.2w** | | |

---

## 6. 引用清单(2025–2026,公开资料)

### Claude Code
- [eesel.ai - Claude Code VS Code extension: a complete guide (2026)](https://www.eesel.ai/blog/claude-code-vs-code-extension)
- [code.claude.com - How Claude Code works](https://code.claude.com/docs/en/how-claude-code-works)
- [code.claude.com - Hooks reference](https://code.claude.com/docs/en/hooks)
- [penligent.ai - Inside Claude Code: The Architecture Behind Tools, Memory, Hooks, and MCP](https://www.penligent.ai/hackinglabs/inside-claude-code-the-architecture-behind-tools-memory-hooks-and-mcp)

### Cline
- [Marketplace listing - saoudrizwan.claude-dev (4,821,857 installs, 2026-06)](https://marketplace.visualstudio.com/items?itemName=saoudrizwan.claude-dev)
- [deployhq.com - Cline for VS Code: Free AI Coding Agent Setup Guide (2026)](https://www.deployhq.com/guides/cline)
- [tech-insider.org - Cline vs Cursor 2026](https://tech-insider.org/cline-vs-cursor-2026)
- [morphllm.com - Cline vs Cursor (2026)](https://www.morphllm.com/comparisons/cline-vs-cursor)
- [augmentcode.com - Cline vs Intent (2026)](https://www.augmentcode.com/tools/intent-vs-cline)
- [buildthisnow.com - Claude Code vs Cline in 2026](https://www.buildthisnow.com/blog/tools/extensions/claude-code-vs-cline)

### Continue
- [docs.continue.dev - config.yaml Reference](https://docs.continue.dev/reference)
- [docs.continue.dev - MCP deep-dive](https://docs.continue.dev/customize/deep-dives/mcp)
- [cursor-alternatives.com - Continue.dev Rules & Config: Complete Setup Guide (2026-04)](https://cursor-alternatives.com/blog/continue-dev-rules)
- [digitalapplied.com - Continue.dev Deep Dive: Open-Source AI Coding 2026](https://www.digitalapplied.com/blog/continue-dev-deep-dive-open-source-ai-coding-assistant-2026)
- [github.com/continuedev/continue issue #9587 - config load bug on Linux](https://github.com/continuedev/continue/issues/9587)

### Cursor
- [deployhq.com - Cursor 2026: Composer, Agent Mode, MCP & Background](https://www.deployhq.com/guides/cursor)
- [digitalapplied.com - Cursor 3 Deep Dive: Agents + Composer Review 2026](https://www.digitalapplied.com/blog/cursor-3-deep-dive-agents-composer-review-2026)
- [developersdigest.tech - What Is Cursor? The AI Code Editor Explained (2026)](https://www.developersdigest.tech/blog/what-is-cursor-ai-editor-2026)
- [petronellatech.com - Cursor AI IDE 2026: Setup, Agents, Security Guide](https://petronellatech.com/blog/cursor-ai-ide-setup-guide)
- [forum.cursor.com - No longer seeing inline diffs from composer (issue thread)](https://forum.cursor.com/t/no-longer-seeing-inline-diffs-from-composer/52319)
- [builder.io - Cursor Alternatives in 2026](https://www.builder.io/blog/cursor-alternatives-2026)

### VSCode / VSCode 扩展生态
- [code.visualstudio.com - SecretStorage API uses Electron API over keytar (v1.80 release notes, 2023-06)](https://code.visualstudio.com/updates/v1_80)
- [code.visualstudio.com - MCP developer guide](https://code.visualstudio.com/api/extension-guides/ai/mcp)
- [code.visualstudio.com - Add and manage MCP servers in VS Code](https://code.visualstudio.com/docs/agent-customization/mcp-servers)
- [arxiv.org/html/2412.00707v2 - Protect Your Secrets: Understanding and Measuring Data Exposure in VSCode Extensions](https://arxiv.org/html/2412.00707v2)

### Shannon 内部资料
- `docs/improvement-plan-2026-08.md` v4(2026-08-04,P2-7 `3ed22799` + P2-8 状态)
- `docs/ROADMAP-FUTURE.md` - HTTP API Server 段
- `docs/project-review-2026-08.md` - HTTP API server 条目
- `crates/shannon-core/src/api_server.rs` - axum 路由 + 测试分支
- `legacy-archives/shannon-code/editors/vscode/src/extension.ts` - legacy 9 命令 + NDJSON 架构

### 综合对比
- [secondtalent.com - Top 7 Coding AI Agents for VS Code in 2026](https://www.secondtalent.com/resources/top-coding-ai-agents-vs-code)
- [builder.io - The Best AI Coding Assistants for VS Code in 2025](https://www.builder.io/blog/best-ai-coding-assistants-vs-code)
- [turing.com - Best AI Coding Tools 2026: Cline, Continue, Claude Code Ranked](https://www.turing.com/blog/best-ai-coding-tools-2026)
- [visualstudiomagazine.com - VS Code AI Extensions: 2025 Year in Review (2026-01)](https://visualstudiomagazine.com/articles/2026/01/vs-code-ai-extensions-review)
- [infoq.com - Claude Code Extension for VS Code: First Look Review (2025-11)](https://www.infoq.com/articles/claude-code-vscode-extension-review/)
- [producthunt.com - The best VS Code extensions to use in 2026](https://www.producthunt.com/p/vscode/the-best-vs-code-extensions-to-use-in-2026)

---

**S0 收口**:本文件是 spike 报告,**不含实施代码**;所有实现细节留给 S1 按 §5 拆任务推进。
