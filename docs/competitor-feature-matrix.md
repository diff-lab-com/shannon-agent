# 综合功能对标矩阵(双赛道)

> 生成日期:2026-08-02(v2,按"双产品线"重构)
> **核心修正(v2)**:Shannon 是**两条产品线**,各自对标不同竞品集:
> - **shannon-code**(终端 REPL / CLI):面向**编程场景** → 对标 Claude Code / Codex CLI / OpenCode / Reasonix
> - **shannon-desktop**(Tauri 桌面):面向**通用办公场景** → 对标 Claude Desktop / Codex Desktop / openworker / Hermes / WorkBuddy / OpenClaw
>
> v1 把 Shannon 当单一产品、误把 openworker 归为"间接竞品"是错误;本版按双赛道重构。
> 证据:[SPEC.md](./SPEC.md)、[CLAUDE.md](../CLAUDE.md)、[desktop-architecture.md](./desktop-architecture.md)、[desktop-product-analysis.md](./desktop-product-analysis.md)、[openworker-research.md](./openworker-research.md)、[project-review-2026-08.md](./project-review-2026-08.md)、公开资料(Nous Research / Tencent / Andrew Ng)

---

## 0. 双产品线定位

| 产品线 | 形态 | 场景 | 核心交付物 | 直接竞品 |
|---|---|---|---|---|
| **shannon-code** | 终端 REPL + CLI + IDE 扩展 | 软件工程(写/改/审代码) | 代码 diff、PR、commit | Claude Code、Codex CLI、OpenCode、Reasonix |
| **shannon-desktop** | Tauri v2 桌面 app | 通用办公(文档/沟通/计划/分析) | 文档、消息、报告、artifact | Claude Desktop、Codex Desktop、openworker、Hermes、WorkBuddy、OpenClaw |

**共享引擎**:两条线共用 `crates/shannon-*` 引擎(QueryEngine / tools / MCP / 权限 / memory / agent),桌面只加 IPC + 持久化 + UI。这是 Codex 已验证的"单一核心 + 多界面"模式。

**战略含义**:桌面线让 Shannon **同时参与两个赛道**,引擎复用摊薄成本。但两条线的**产品语言不同**:编码线讲"diff/commit/工具链",办公线讲"成品/集成/自动化"。文档与计划需分开表述。

---

## 1. 赛道一:编码 CLI 对标矩阵

图例:✅ 完整 · 🟡 部分/有缺口 · ❌ 无 · ➖ 不适用

| 维度 | shannon-code | Claude Code | Codex CLI | OpenCode | Reasonix |
|---|---|---|---|---|---|
| **核心语言** | Rust | TS/Python | Rust/TS | Go/TS | Python |
| **许可** | 开源 MIT | 闭源 | 部分开源 | 开源 | 开源 |
| **多 provider** | ✅ 含国产全家桶 | ❌ Anthropic | 🟡 OpenAI 系 | ✅ | 🟡 DeepSeek 系 |
| **BYO key / 本地** | ✅ Ollama | ❌ | 🟡 | ✅ | ✅ |
| **GLM(Z.ai)** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **工具数** | 48 + Tool trait | ~40 | ~40 | ~30 | ~30 |
| **LSP** | 🟡 6 工具+cargo check | 🟡 | ❌ | ✅ 25+ 自装 | ❌ |
| **tree-sitter repo map** | ❌(计划 P1) | ✅ | ✅ | ✅ | 🟡 |
| **MCP** | ✅ 全传输+webhook | ✅ | 🟡 | ✅ | 🟡 |
| **权限分级** | ✅ 5 级+LLM 分类器 | ✅ 4 级 | 🟡 | ✅ | 🟡 |
| **Sub-agent/Team** | ✅ team+worktree+/batch | ✅ 4 机制 | 🟡 | ❌ | ❌ |
| **Worktree 隔离** | ✅ | ✅ | ❌ | ❌ | ❌ |
| **Hook 系统** | ✅ 30 事件(5 dead) | ✅ 30 | ➖ | ➖ | ➖ |
| **Routines(触发/调度)** | ✅ | ✅ | ➖ | ➖ | ➖ |
| **非交互/CI** | ✅ --prompt+--schema | ✅ -p | ✅ | ✅ serve | ➖ |
| **Record/Replay 测试** | ✅(独有) | ❌ | ❌ | ❌ | ❌ |
| **Auto-test loop** | ❌(计划 P1) | ✅ | ✅ | ✅ | 🟡 |
| **Auto-commit** | 🟡(hook 拆分问题) | ✅ | ✅ | ✅ | 🟡 |
| **IDE 扩展** | 🟡 VS Code scaffold | ✅ VS Code+JB | ❌ | ❌ | ❌ |

**赛道一结论**:shannon-code 在**多 provider(含国产)、Team/worktree、权限精细度、Record/Replay** 上领先;**repo map、auto-test loop、IDE 扩展成熟度**是短板。详见 [project-review](./project-review-2026-08.md)。

---

## 2. 赛道二:桌面办公对标矩阵 ⭐(v2 重点)

### 2.1 桌面竞品全景

| 产品 | 厂商 | 桌面框架 | 后端语言 | 开源 | 许可 | 定位 |
|---|---|---|---|---|---|---|
| **shannon-desktop** | Shannon | **Tauri v2** | **Rust** | ✅ | MIT | 最轻量/最开放/最可审计的 AI 工作台 |
| **Claude Desktop** | Anthropic | Electron + Linux VM | TS(闭源) | ❌ | 闭源 | Agent 编排层(Connectors→Chrome→屏幕) |
| **Codex Desktop** | OpenAI | Electron + Rust App Server | Rust(核心开源) | 🟡 | Apache(CLI/Server) | Agent 指挥中心,异步多 Agent |
| **openworker** | Andrew Ng | **Tauri + React** | **Python(aisuite)** | ✅ | MIT | 交付成品的 AI coworker |
| **Hermes** | Nous Research | Electron + Python | Python | ✅ | MIT | 自我进化 Agent |
| **WorkBuddy** | Tencent | 原生桌面 | 未公开 | ❌ | 闭源 | 办公"AI Employee" |
| **OpenClaw** | 社区 | Electron(Bun Gateway) | TS | ✅ | MIT/Apache | 本地优先,消息平台即界面 |

**关键技术发现**:
- **框架**:绝大多数用 Electron;**只有 shannon-desktop 和 openworker 用 Tauri** —— 体积优势(~10MB vs ~300MB)是 Shannon 与 openworker 共享的差异化点。
- **后端**:Claude Desktop/Codex Desktop 用 TS/Rust;openworker/Hermes 用 Python;**shannon-desktop 是唯一的 Rust 全栈桌面**。
- **开源**:shannon-desktop、openworker、Hermes、OpenClaw 开源;Claude Desktop、WorkBuddy 闭源。

### 2.2 桌面功能矩阵

| 功能 | shannon-desktop | Claude Desktop | Codex Desktop | openworker | Hermes | WorkBuddy |
|---|---|---|---|---|---|---|
| **多 provider** | ✅ 全家桶 | ❌ Anthropic | 🟡 OpenAI | ✅ 全家桶 | ✅ provider-agnostic | 🟡 |
| **BYO key/本地** | ✅ Ollama | ❌ | 🟡 | ✅ Ollama | ✅ | ➖ |
| **沙盒执行** | ❌ | ✅ Linux VM+gVisor | ✅ Seatbelt/Landlock | 🟡 审批网关 | 🟡 6 后端 | ➖ |
| **多 Agent 并行** | ✅ Team 系统 | ✅ Cowork VM | ✅ 6 线程 | ❌ 单 agent | ✅ 子 Agent | ✅ 并行任务 |
| **Worktree 隔离** | ✅ /batch | ❌ | ✅ 原生 | ❌ | ❌ | ➖ |
| **Computer Use** | 🟡 feature flag | ✅ 截屏+AX树 | ✅ macOS AX API | ❌ | ✅ 浏览器/桌面控制 | 🟡 |
| **MCP** | ✅ 全传输 | ✅(创建者) | ✅ 客户端+服务端 | ✅ | ✅ | 🟡 |
| **插件/扩展市场** | 🟡 Extensions Hub | ✅ MCPB 90+ | ✅ Plugin 90+ | 🟡 MCP | ✅ Skills Hub | ➖ |
| **SaaS 集成** | ❌(计划 MCP 补) | 🟡 via MCP | ❌ | ✅ **25+** | 🟡 | 🟡 |
| **语音输入** | ❌(计划 P3) | ✅ 20+ 语言 | ✅ | ✅ Rust STT | ✅ | ➖ |
| **附件/文件上传** | ❌(计划新增) | ✅ | ✅ | ✅ | ✅ | ✅ |
| **多线程/多会话** | 🟡 SessionManager | ✅ 多标签 | ✅ 6 线程 | 🟡 | 🟡 | ✅ |
| **后台任务/调度** | ✅ routines+OPC | ✅ Cowork VM | ✅ Cloud 容器 | ✅ automations | ✅ cron | ✅ |
| **记忆系统** | ✅ MemoryStore | ✅ Projects+Memory | 🟡 Memory 预览 | ✅ | ✅ MEMORY+USER.md | 🟡 |
| **跨平台** | ✅ Win/macOS+Linux 源码 | 🟡 | 🟡 | 🟡 Win 未签名 | ✅ Linux/macOS/WSL2 | ✅ 含移动端 |
| **自动更新** | 🟡 | ✅ | ✅ | ✅ | ✅ | ✅ |
| **消息平台入口** | ❌ | ❌ | ❌ | ✅ Slack @ | ❌ | ✅ Slack/Telegram 等 |
| **移动端** | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ iOS/Android/HarmonyOS |
| **i18n** | ✅ zh/en | 🟡 | 🟡 | ➖ | ➖ | ✅ 中文 |
| **定价** | 免费开源 | $20-200/月 | 含订阅 | 免费开源 | 免费开源 | 闭源 |

### 2.3 桌面设计理念对比

| 产品 | 核心哲学 | 对 shannon-desktop 的启示 |
|---|---|---|
| **Claude Desktop** | "不是 IDE 替代品,是 Agent 编排层"。三层递进:Connectors→Chrome→屏幕控制 | 编排层定位;但走闭源+VM 重路线,Shannon 走轻量开源 |
| **Codex Desktop** | "不是编辑器,是监督平台"。委托>配对,异步多 Agent 是未来默认 | 异步多 Agent + 后台是趋势;Shannon 已有 Team+routines |
| **openworker** | "交付成品>聊天"。本地优先+BYO model+25+ SaaS | **成品交付语言** + **SaaS 集成** 是最大启示 |
| **Hermes** | "与你共同成长的 Agent"。从重复工作流自动生成新 Skill | **自我进化 skill** 是差异化方向,Shannon 有 self-improve 雏形 |
| **WorkBuddy** | "AI Employee"。移动端 + 消息平台全覆盖 | **移动端派发** 是远期方向 |
| **OpenClaw** | "龙虾之道"。配置驱动、自托管、消息平台即界面 | 消息平台入口(WhatsApp/Telegram)是个有趣切角 |

### 2.4 赛道二结论

shannon-desktop 在办公赛道的**差异化护城河**:
1. **唯一 Rust 全栈桌面**(轻量 ~10MB、内存安全、可审计)—— 编码线优势自然延伸。
2. **最开放多 provider**(含国产全家桶)—— 对中文用户友好,Claude/Codex Desktop 都锁定单家。
3. **Team + worktree + /batch** —— 办公场景的"并行处理多件事"能力,openworker/Hermes/WorkBuddy 多数只有单 agent 或简单并行。
4. **开源 MIT + 可自托管** —— 企业/隐私场景优势,WorkBuddy/Claude Desktop 闭源。

shannon-desktop 在办公赛道的**关键缺口**(按严重度):
| 缺口 | 严重度 | 对标 | 说明 |
|---|---|---|---|
| **SaaS 集成空白** | 🔴 高 | openworker 25+ | 最大缺口,直接影响办公可用性 |
| **沙盒执行缺失** | 🔴 高 | Claude VM / Codex Seatbelt | 办公场景执行不可信代码/脚本的安全基础 |
| **附件上传缺失** | 🔴 高 | 全员都有 | 办公场景刚需(传文档/图片/表格) |
| **语音输入缺失** | 🟠 中 | Claude/openworker/Hermes | 体验缺口,openworker 已验证 Rust STT |
| **多线程管理弱** | 🟠 中 | Codex 6 线程 | 当前 SessionManager 偏会话管理,非并行线程 |
| **chat UI 待美化** | 🟠 中 | 全员 | ~2600 LOC 自研,体验落后于成熟产品 |
| **Computer Use 半成品** | 🟡 低 | Claude/Codex | feature flag,需完善 |
| **无移动端** | 🟢 观望 | WorkBuddy | 远期,非当前优先 |

---

## 3. 跨赛道综合:Shannon 双线的战略位置

### 3.1 优势(双线共享,引擎红利)

| 优势 | 编码线 | 办公线 |
|---|---|---|
| Rust 全栈 | ✅ 性能/安全 | ✅ 轻量桌面 |
| 多 provider 含国产 | ✅ | ✅ |
| 开源 MIT | ✅ | ✅ 可自托管 |
| Team/worktree | ✅ 独有 | ✅ 并行任务 |
| 5 级权限+LLM 分类器 | ✅ | ✅ 审批网关 |
| Record/Replay 测试 | ✅ 独有 | ✅ |

### 3.2 双线各自的必修短板

**编码线(shannon-code)必修**:
1. tree-sitter repo map(P1)
2. auto-test loop(P1)
3. IDE 扩展完善(P2)
4. dead code 清理(P1)

**办公线(shannon-desktop)必修**:
1. SaaS 集成(P1,最高优先)⭐
2. 沙盒执行(P1,安全基础)
3. chat 升级:附件/语音/多线程/美化(P1–P2)⭐
4. artifact 显性化(P3,产品语言)

---

## 4. 竞品监测名单(季度复查)

| 竞品 | 赛道 | 监测重点 | 触发升级为"正面威胁"的条件 |
|---|---|---|---|
| **openworker** | 办公 | connector 新增节奏、automations 演进、是否向 coding 延伸 | 若出 coding 模式或中文 SaaS 加深 |
| **Hermes** | 办公 | self-improving skill 成熟度、记忆系统 | 若 skill 自动生成稳定可用 |
| **WorkBuddy** | 办公 | 移动端、中文 SaaS、定价 | 若开源或低价进入开发者市场 |
| **Codex Desktop** | 双线 | macOS AX 后台并行、云 Agent | 若跨平台或开放 provider |
| **Claude Desktop** | 双线 | Cowork VM、Computer Use、MCPB | 若降价或开放 |
| **OpenClaw** | 办公 | 消息平台入口、自托管 | 若企业采用增长 |

---

## 5. 给评审的结论

v1 的错误是**用编码赛道的视角看整个 Shannon**,导致 openworker 被误判为"间接竞品"。v2 修正后:

1. **openworker 是 shannon-desktop 的直接竞品**(同为 Tauri+本地优先+BYO model 的开源办公 agent),其 25+ SaaS 集成是 Shannon 桌面线最大威胁。
2. **shannon-desktop 的办公赛道必修课**是 SaaS 集成 + 沙盒 + chat 升级(附件/语音/多线程/美化),而非编码线的 repo map。
3. **两条线共享引擎**意味着补一条线的功能(如 SaaS MCP)对另一条线也有价值 —— 投入产出比高。

这些结论直接驱动 [improvement-plan-2026-08.md](./improvement-plan-2026-08.md) 的桌面任务块与优先级。
