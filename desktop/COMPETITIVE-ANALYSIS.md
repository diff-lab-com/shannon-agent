# Shannon Desktop 竞品调研与差距分析

**调研日期**: 2026-06-13
**调研对象**: Claude Code Desktop / Codex Desktop / Hermes Desktop / OpenClaw / Cursor 3 / Windsurf(Devin Desktop)
**作者**: 产品调研

---

## 0. TL;DR

Shannon Desktop 当前是「**对话 + 会话管理**」型的 Tauri 应用:底层接入了 Shannon Core 的完整引擎(多 provider、工具调用、MCP、权限),UI 用 React 19 + MD3 token + 8 套主题,已经做得相当扎实。但相比 2026 年这批新发布的竞品,**在「Agent 编排」「自动化/Triage」「消息渠道集成」「可视化反馈」「成本可见性」五个方向上明显落后**,而这些恰好是 2026 桌面端 AI 助手的护城河。

**最值得补的 5 件事**(详见 §4):
1. 多 Agent 并行编排面板(Codex 的 thread / Hermes 的 sub-agents / Cursor 的 Agents Window 都做了)
2. 定时任务 + Triage 队列(Codex Automations / Hermes Cron)
3. 消息渠道集成(OpenClaw / Hermes 都做了 10+ 平台)
4. 预览窗 + Diff viewer 的可视化(Claude Code Desktop / Cursor Design Mode)
5. 把 CLI 已有的 Team / Worktree / Hook / Subagent 在 Desktop 暴露 UI(目前完全没接)

---

## 1. 竞品一句话定位

| 产品 | 定位 | 开源 | 本地优先 | 独特记忆点 |
|---|---|---|---|---|
| **Claude Code Desktop** | Agentic 编排器 | 否 | 否(云端) | 可拖拽面板 + 预览窗 + Cowork(操作桌面 app) |
| **Codex Desktop** | Agent command center | 否 | 否(云端) | Triage 队列 + In-app browser + 后台 Computer Use |
| **Hermes Desktop** | 开源自治 agent | **MIT** | **是** | Apple 式精致 UI + session 即成本控制 + 多渠道 |
| **OpenClaw** | 多渠道 agent gateway | **MIT** | **是(自托管)** | 10+ 消息平台统一入口(WhatsApp/Signal/iMessage…) |
| **Cursor 3** | AI-native IDE | 否 | 部分 | **Design Mode**(点选拖拽 + 语音改 UI) |
| **Windsurf / Devin Desktop** | IDE + Agent 看板 | 否 | 部分 | Kanban 式 Spaces 看板 |

> ⚠️ Anthropic 有**两个**桌面 app:面向通用聊天的「Claude Desktop」和面向编程的「Claude Code Desktop」(2026-04 重新发布)。Shannon 对标后者。

---

## 2. Shannon Desktop 现状速览

### 已实现(扎实部分)
- **底层引擎完整接入**:59 个 Tauri 命令,复用 Shannon Core 的 `QueryEngine` / `ToolRegistry` / `PermissionManager` / `McpProcessPool` / `SkillRegistry` / `StateManager`
- **多 provider LLM**:Anthropic / OpenAI / DeepSeek / Ollama,运行时切换
- **会话生命周期**:新建/列表/搜索/加载/切换/删除/重命名/复制/导出
- **工具调用流式可视化**:文本/工具调用/思考块/usage 实时事件
- **8 种权限模式** + UI 批准弹窗
- **MCP 服务器管理**:增删/重启/列表,接真实进程池
- **桌面原生**:系统托盘、全局快捷键(Ctrl+Shift+S/N/K)、窗口状态持久化、Tauri 自动更新、文件拖拽
- **UI 工程**:React 19 + TypeScript + Tailwind v4 + MD3 token,8 套主题(Material/Tokyo Night/Catppuccin/Nord/Ember/Slate),a11y(focus-visible、ARIA、语义按钮)

### 桌面端相对 CLI 的覆盖度(约 60%)
- ✅ Chat / Sessions / MCP / Permissions / Provider 切换 / 后台任务
- ⚠️ Skills(只列出,不触发) / Tasks(列表,无 team 协调) / 项目记忆(后端有,无 UI)
- ❌ Subagent / Team / Worktree / LSP / Hook / Plugin / Computer Use — **全部没暴露 UI**

---

## 3. 功能差距清单

> 优先级:**P0** = 桌面端护城河,没就掉队;**P1** = 显著体验提升;**P2** = 锦上添花。

### 3.1 「Agent 编排」类(P0,2026 桌面端的核心战场)

| 功能 | 谁做了 | Shannon 现状 | 优先级 |
|---|---|---|---|
| 多 agent 并行面板(线程/标签) | Codex(threads) / Cursor(Agents Window) / Hermes(sub-agents) / Claude Code(Mission Control 侧栏) | ❌ 单会话单线程 | **P0** |
| Agent 任务看板(Kanban/网格) | Windsurf(Spaces) / Cursor(Mission Control Exposé 视图) | ⚠️ Tasks 页是只读列表 | **P0** |
| Cloud ↔ Local session 迁移 | Cursor 3 | ❌ | P1 |
| 并行 agent 多模型对比 | Cursor(多 LLM 并排) | ❌ | P1 |
| 可拖拽/可保存的面板布局 | Claude Code(按 repo 保存) | ❌ 固定布局 | P1 |

### 3.2 「自动化与 Triage」类(P0,编排器心智模型的关键 UX)

| 功能 | 谁做了 | Shannon 现状 | 优先级 |
|---|---|---|---|
| 定时(cron)任务 UI | Codex(Automations) / Hermes(Cron) / OpenClaw | ❌(后端有 routine,无 UI) | **P0** |
| Triage 队列(集中处理自动化产出) | Codex | ❌ | **P0** |
| Webhook 触发 | 后端有 `WebhookRegistry`,Shannon CLI 支持 | ❌ 桌面无 UI | P1 |
| 自然语言定时任务 | Hermes | ❌ | P2 |
| 无人值守后台 worktree 执行 | Codex(专用 background worktree) | ❌ | P1 |

### 3.3 「消息渠道集成」类(P0,Shannon 的低成本高价值差异化)

| 功能 | 谁做了 | Shannon 现状 | 优先级 |
|---|---|---|---|
| 多消息平台桥接(Slack/GitHub/Linear 等) | OpenClaw(10+) / Hermes(10+) / Codex(GitHub/Slack/Linear) / Cursor(Slack/GitHub/Linear) | ❌ | **P0** |
| 从消息触发 agent / 回复路由 | OpenClaw / Hermes | ❌ | **P0** |
| 移动端远程派发(Cowork Dispatch) | Claude Code | ❌ | P2 |
| 邮件/iMessage/IM 集成 | Claude Code(MS365/iMessage) / Hermes | ❌ | P2 |

> 💡 Shannon 的 **hook + routine + MCP webhook** 后端已经搭好,只差 UI 和几个主流平台 adapter。这是性价比最高的差异化切入点。

### 3.4 「可视化反馈」类(P0,桌面端独占壁垒)

| 功能 | 谁做了 | Shannon 现状 | 优先级 |
|---|---|---|---|
| 预览窗(HTML/PDF/dev server) | Claude Code / Cursor | ❌ | **P0** |
| In-app browser + 元素级评论 | Codex(独有) / Cursor(Design Mode) | ❌ | **P0** |
| Design Mode(点选拖拽 + 语音改 UI) | Cursor 3.7 | ❌ | P1(壁垒级) |
| Diff viewer(大型 changeset) | Claude Code / Cursor / Windsurf | ⚠️ `get_file_diff` 命令在,但无 diff UI | **P0** |
| 应用内文件编辑 | Claude Code | ❌ | P1 |
| 集成终端 | Claude Code | ❌ | P1 |

### 3.5 「Computer Use / 桌面控制」类(P1)

| 功能 | 谁做了 | Shannon 现状 | 优先级 |
|---|---|---|---|
| 后台 Computer Use(不抢焦点) | Codex(独有) | ⚠️ CLI 有 feature flag,桌面没暴露 | P1 |
| 浏览器自动化(Playwright MCP) | 全员 | ⚠️ MCP 接了但无 UI 反馈 | P1 |
| 屏幕/截图集成 | Codex / Claude Cowork | ❌ | P2 |

### 3.6 「成本与可见性」类(P1,Hermes 的设计哲学)

| 功能 | 谁做了 | Shannon 现状 | 优先级 |
|---|---|---|---|
| Session 即成本控制(每会话 context 隔离 + 用量可见) | Hermes(显式设计目标) | ⚠️ Chat 显示 usage,但无配额/预算 | P1 |
| 多 Profile(按模型分工:Opus 策略 / GPT 编码 / 本地模型研究) | Hermes | ⚠️ 有 provider 切换,无 profile 概念 | P1 |
| 配额耗尽告警 / 预算限制 | Claude Code(用户抱怨"8 分钟烧 5 小时") | ❌ | P1 |
| 本地模型优先路由(Mac Studio/DGX) | Hermes | ⚠️ 支持 Ollama,但无自动路由 | P2 |

### 3.7 「桌面原生」缺失(P1)

| 功能 | 谁做了 | Shannon 现状 | 优先级 |
|---|---|---|---|
| 多窗口 / 多 session 独立窗口 | Codex / Claude Code | ❌ 单窗口 | P1 |
| 原生通知中心集成 | 全员 | ❌ | P1 |
| 剪贴板历史 | Hermes(Artifacts) | ❌ | P2 |
| 开机自启 / 托盘驻留配置 UI | 全员 | ⚠️ 后端有,无 UI 开关 | P2 |
| 自定义全局快捷键录制 | 多数 | ❌(硬编码 3 个) | P2 |
| 文件系统监听 UI | Codex | ❌ | P2 |

### 3.8 「MCP 生态」类(P1,对标 Claude 标杆)

| 功能 | 谁做了 | Shannon 现状 | 优先级 |
|---|---|---|---|
| 一键安装包(`.dxt` / `.mcpb`) | Claude Code(标杆) | ❌ 只能手动 JSON | **P0** |
| Connector 目录(浏览安装) | Claude Code | ❌ | P1 |
| 跨设备同步 remote connector | Claude Code | ❌ | P2 |
| Extension manifest 可视化编辑 | Claude Code | ❌ | P2 |

### 3.9 「Shannon CLI 已有但桌面端未暴露」(P1,纯 UI 工作量)

| 功能 | CLI 状态 | 桌面端 | 优先级 |
|---|---|---|---|
| Subagent 系统 | ✅(teammate 协调、`/batch`、agent view) | ❌ | **P0** |
| Agent Team(Create/SendMessage/Task) | ✅(`/team` 命令) | ❌ | **P0** |
| Worktree 隔离 | ✅(`/batch` 自动创建) | ❌ | P1 |
| Hook 系统(32 事件) | ✅(超过 Claude Code) | ❌ 无配置 UI | P1 |
| Plugin 系统 | ✅(PluginRegistry) | ❌ 无管理 UI | P1 |
| LSP 集成(6 工具 + 后台 cargo check) | ✅ | ❌ 无代码智能 UI | P1 |
| 权限 Profile(命名预设) | ✅(`/profile` 命令) | ❌ | P1 |
| Routine(触发 + 定时) | ✅(`/routine`) | ❌ | P1 |
| 项目记忆(CLAUDE.md) | ✅ 后端 | ❌ 无查看/编辑 UI | P1 |

---

## 4. UI/UX 改进清单

### 4.1 交互层

| 问题 | 现状 | 建议 | 优先级 |
|---|---|---|---|
| 键盘快捷键不可发现 | `KeyboardShortcutsHelp` 组件存在但仅 3 个全局快捷键 | 补齐会话内快捷键(Ctrl+K palette、Ctrl+N、Ctrl+P 切换会话),做 ⌘? 帮助面板 | P0 |
| 长任务无进度反馈 | 后端有 `ToolProgress` 事件,但部分场景未显示 | 工具调用统一加进度条/步骤指示 | P1 |
| 错误恢复弱 | 基础 error 显示,无 retry | API/工具失败加「重试」「降级 provider」按钮 | P1 |
| 空状态不完整 | 部分页面缺失 | Goals / Tasks 等页补引导式空状态 | P2 |
| 多模型切换摩擦 | 顶部下拉 | 仿 Hermes 底栏状态栏快速切换 + per-session 记忆 | P1 |

### 4.2 视觉层

| 问题 | 现状 | 建议 | 优先级 |
|---|---|---|---|
| 布局固定不可重组 | 三栏固定 | 仿 Claude Code 可拖拽 + 按 repo/project 保存布局 | P1 |
| Diff 没有专用 viewer | 命令在但无渲染 | 加 monaco-style diff,支持 stage/unstage/commit | P0 |
| 工具调用结果折叠粗糙 | 文本展示 | 区分 read-only / destructive,destructive 加确认 + 高亮 | P1 |
| 思考块(thinking)展示 | 有事件流 | 加可折叠、按 turn 分组、可复制 | P2 |

### 4.3 信息架构

| 问题 | 现状 | 建议 | 优先级 |
|---|---|---|---|
| Tasks 页定位模糊 | 任务板 + 日历 + 效率指标,但数据来源是后台任务 | 明确:是「后台任务看板」还是「Agent Team 任务」?当前混在一起 | P0 |
| Extensions 页层级深 | skills/agents/datasources 嵌套路由 | 加统一 Hub 首页 + 分类标签 | P1 |
| 项目/工作区概念缺失 | 无 | 引入「Project」(= repo + 配置 + 历史),对标 Codex/Cursor | P0 |

---

## 5. Shannon 的独有优势(应保持/放大)

竞品调研中也印证了几点 Shannon 不应放弃的底牌:

1. **Rust + Tauri 原生**:vs Electron 竞品的性能/内存优势。Cursor/Claude Code 都是 Electron。
2. **Hook 事件覆盖(32 个)> Claude Code(18+)**:桌面端应把 hook 配置做成可视化规则引擎,这是差异化。
3. **Team 协调 + `/batch` worktree**:Codex 的 worktree 是核心卖点,Shannon CLI 已有,只差 UI。
4. **LLM 权限分类器(4 层 + confidence < 0.7 回退 LLM)**:比 Claude Code 的纯规则更先进,应在 UI 上让用户看到「为什么被批准/拒绝」。
5. **多 provider 中立**:Hermes/OpenClaw 走的开源多 provider 路线正热,Shannon 已天然站这条线。

---

## 6. 差异化定位建议

不要正面硬刚:
- ❌ vs Cursor(IDE 体验):Shannon 不是 IDE,不该假装是
- ❌ vs Claude Code Desktop(编排器 + Cowork):Anthropic 云端 + 闭源护城河太深

建议路线(对标 Hermes / OpenClaw):
- ✅ **开源 + 本地优先 + 多 provider + Rust 性能**
- ✅ **消息渠道集成**(Shannon 有 hook/routine/MCP webhook 基础,补 UI 即可)
- ✅ **Agent Team 可视化**(CLI 已有 team,桌面做看板,对标 Windsurf Spaces)
- ✅ **Hook 可视化规则引擎**(差异化卖点,32 事件 > Claude Code)

---

## 附录 A:平台与价格对照

| 产品 | Mac | Windows | Linux | 价格 |
|---|---|---|---|---|
| Claude Code Desktop | ✅ | ✅ | ❌ | $20+/Max |
| Codex Desktop | ✅ | ✅ | ❌ | Plus+ |
| Hermes Desktop | ✅ | ✅ | ✅ | **免费(MIT)** |
| OpenClaw | ✅ | ✅ | ✅ | **免费(MIT)** |
| Cursor 3 | ✅ | ✅ | ✅ | $20-200 |
| Windsurf | ✅ | ✅ | ✅ | $15 |
| **Shannon Desktop** | ✅ | ✅ | ✅ | **免费(开源)** |

## 附录 B:调研信息源

完整 URL 列表见 deep-research-agent 输出。关键来源:
- Anthropic 官方 / OpenAI 官方 / Cursor Blog
- Hermes Agent Docs(hernes-agent.nousresearch.com)
- OpenClaw Docs(docs.openclaw.ai)+ Wikipedia
- VentureBeat / The New Stack / Ars Technica 2026 评测

---

**下一步**:本文档为差距清单基线。建议从 §3.1(多 agent 并行)+ §3.2(自动化/Triage)+ §3.4(预览/diff viewer)三块 P0 中各挑 1 项进入下个迭代。
