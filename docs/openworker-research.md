# OpenWorker 竞品调研报告

> 调研日期:2026-08-02
> 调研对象:[andrewyng/openworker](https://github.com/andrewyng/openworker)
> 调研者:高级产品经理 + 高级架构师视角
> 关联文档:[competitor-feature-matrix.md](./competitor-feature-matrix.md)、[project-review-2026-08.md](./project-review-2026-08.md)

---

## 0. TL;DR

OpenWorker 是 Andrew Ng 于 2026-07-23 发布的开源 **桌面 AI coworker**(MIT,Open Beta)。它的核心差异化不是"聊天",而是**交付成品**:一份排好版的文档、一条带数据的 Slack 回复、一个改好的日历、一个分拣过的收件箱。技术上走 **Python(aisuite)后端 + React/Tauri GUI + Rust STT 旁路** 的本地优先架构,BYO model,内置 25+ SaaS 集成。

**与 Shannon 的关系:shannon-desktop(桌面办公线)的直接竞品。**

> **v2 修正(2026-08-02)**:v1 把 Shannon 当单一编码产品、误判 OpenWorker 为"间接竞品"是错误的。Shannon 是**双产品线** —— `shannon-code` 打编程赛道(对标 Claude Code/Codex CLI/OpenCode),`shannon-desktop` 打通用办公赛道(对标 Claude Desktop/Codex Desktop/**OpenWorker**/Hermes/WorkBuddy)。OpenWorker 与 shannon-desktop **同属"开源 Tauri/本地优先 + BYO model + 办公交付"赛道**,是**直接竞品**。完整双赛道对标见 [competitor-feature-matrix.md](./competitor-feature-matrix.md)。

- **直接竞品(办公线)**:OpenWorker 与 shannon-desktop 同为开源、本地优先、多 provider、带审批网关的桌面 AI agent,目标用户重叠(日常办公任务),交付物重叠(文档/消息/报告)。OpenWorker 凭 **25+ SaaS 集成** + **Andrew Ng 背书** + **更早发布(2026-07)** 目前在办公赛道领先。
- **架构同构**:两者都用 Tauri 桌面壳 + 本地优先 + BYO model + MCP + 审批网关。区别在后端 —— OpenWorker 用 **Python(aisuite)**,shannon-desktop 用 **Rust**。说明 Shannon 桌面线的技术路线方向正确,但**执行节奏落后**(OpenWorker 已签名分发 + 自动更新,shannon-desktop 仍在完善)。
- **最大威胁 —— 集成护城河**:OpenWorker 的 25+ SaaS 集成(Slack/Jira/Notion/Linear/HubSpot/Outlook/Gmail/Calendar)是 shannon-desktop **完全空白**的核心能力象限,也是办公赛道的胜负手。这是 [improvement-plan-2026-08.md](./improvement-plan-2026-08.md) P1-3(MCP 化补 SaaS)成为最高优先级的根因。

---

## 1. 基本信息

| 项 | 内容 |
|---|---|
| 项目 | OpenWorker |
| 作者 | Andrew Ng([@andrewyng](https://github.com/andrewyng)) |
| 仓库 | https://github.com/andrewyng/openworker |
| 官网 | https://openworker.com |
| 许可证 | **MIT**(完全开源,可商用) |
| 状态 | **Open Beta** —— 可用、自动更新、积极打磨中,欢迎 issue |
| 发布 | 2026-07-23(MarkTechPost 报道日期) |
| 热度 | Trendshift 上榜,Trending repositories #91434(daily) |
| 起源 | 原本在 aisuite 仓库内开发,后独立成仓 |

**作者背书**:Andrew Ng 是 aisuite、DeepLearning.AI、Landing AI 创始人,斯坦福兼职教授。OpenWorker 直接建立在 aisuite 之上,可视为 aisuite 的"参考实现 + 产品化包装"。

---

## 2. 产品定位

> "AI that gets your everyday tasks done. OpenWorker is an open-source AI coworker that lives on your desktop and delivers **finished work**, not just chat."

定位三要素:

1. **桌面 resident**(不是 CLI,不是 Web):常驻用户桌面,像一个"坐在旁边的同事"。
2. **交付成品**(deliverables,不是 to-do list):输出的不是"建议你这样做",而是"已经做好的文件/消息/日历项"。
3. **跨工具协作**:能同时操作本地文件、终端、25+ 第三方 SaaS。

**工作流范例**(官方):
1. 告诉 OpenWorker 你想要的结果 —— "准备一份客户简报"、"理清我的日历"、"起草一份报告"、"查一下发布在 Jira 和 GitHub 上的进展"。
2. 它把任务拆成步骤,跨桌面、文件、已连接 App 执行。
3. 任何有后果的动作(发消息、改日历、跑命令)之前**先请示**,你批准或重定向。
4. 你拿到的是**完成的交付物**。

**对照 Claude Code / Codex CLI / Shannon**:这些都是"在终端/IDE 里改代码"的 coding agent;OpenWorker 是"在桌面里处理办公杂事"的 productivity agent。赛道不同。

---

## 3. 架构

```
┌────────────────────────────────────────────────┐
│              OpenWorker desktop app            │  native shell + GUI
├────────────────────────────────────────────────┤
│           local agent server (Python)          │  engine · tools · connectors
│                                                │  - built on aisuite
├───────────────┬────────────────────────┬───────┤
│  your files   │   your tools           │  your model   │  everything runs with your keys,
│  & terminal   │ 25+ connectors         │  any provider │  on your machine
└───────────────┴────────────────────────┴───────┘
```

### 3.1 三层结构

| 层 | 技术 | 职责 |
|---|---|---|
| **桌面壳 + GUI** | **React + Tauri** | 窗口、原生集成、自动更新;监督本地 server |
| **本地 agent server** | **Python,基于 aisuite** | agent 引擎、模型 provider、connectors、MCP client、memory、automations |
| **STT 旁路** | **Rust** | 语音输入(speech-to-text sidecar) |

### 3.2 关键设计

- **Tauri 监督 server**:`npm run tauri dev` 时,Tauri 壳负责启动并看护本地 Python server;独立 server 模式则用一次性 token(`<state-dir>/sidecar-<port>.token`,user-only 权限)鉴权,Vite 启动时读取该文件;桌面 app 用内存 token,**绝不落盘**。
- **aisuite 作为引擎底座**:aisuite 是 Andrew Ng 自己的轻量 Python 库,提供跨 provider 的统一 chat-completions API + agents 层(tools / toolkits / MCP)。OpenWorker 官方表述:"如果你想搭自己的 agent harness 而不是用我们的,从 aisuite 开始;本仓是 aisuite 能承载什么的参考实现。"
- **本地优先**:agent loop、对话、connector token、model key 全部存在 app 本地 secret store。唯一上云的是 broker OAuth 握手的小服务,且**可以不登录使用**(用手动创建的凭证/API key 走 connector)。

### 3.3 仓库结构

| 目录 | 内容 |
|---|---|
| `coworker/` | Python 后端 —— agent engine、model providers、connectors、MCP client、memory、automations |
| `surfaces/gui/` | 桌面 app —— React UI + Tauri 壳(监督 server) |
| `stt/` | 语音转文字旁路(Rust) |
| `packaging/` | 安装器构建(macOS DMG、Windows)、自动更新清单、开发引导 |
| `docs/` | 设计规格与决策日志 |
| `tests/` | 后端测试套件 |

---

## 4. 核心能力

### 4.1 交付真实成品
文档、表格、报告、网页 —— 作为**可打开、可分享的文件**落地。这是与"只聊天的 chatbot"的根本区别。

### 4.2 从 Slack 触发
在频道里 @OpenWorker,桌面端打开一个会话,用你的工具完成工作,**答案以 thread reply 回到 Slack**。这是一个"消息入口 → 桌面执行 → 消息出口"的闭环。

### 4.3 25+ 集成
GitHub、Slack、Jira、Notion、Linear、HubSpot、Outlook、monday.com、Gmail、Google Calendar,加上**终端和本地文件**。任何 MCP 可达的工具也能插进来,**按工具粒度控制**。

### 4.4 调度执行(automations)
面向**周期性工作**的自动化:每日 brief、每周 report、对某个频道的常驻 watch。每次 run 落在 app 里,**带完整 transcript**。

### 4.5 审批网关
写、发送、shell 命令都是 approval-gated。**无人值守**的 run 把请求停进 inbox,**不会自己动手**。这个"无人值守也安全"的设计和 Shannon 的 FullAuto / 风险分级思路一致。

---

## 5. 模型支持(BYO key)

开箱即用:

> OpenAI · Anthropic · Google Gemini · Inkling(Thinking Machines)· **GLM(Z.ai)** · DeepSeek · Kimi(Moonshot)· Qwen · MiniMax · Mistral · Grok(xAI)

外加:
- **Together / Fireworks** 走 open-weight 模型
- **Ollama** 走全本地模型

**策划的模型列表**标注了"已验证 tool-calling"的型号;也允许任意填一个 model string(自担风险)。

**注意**:OpenWorker 把 GLM(Z.ai)列为一等公民 provider —— 这对 Shannon(同样支持 GLM)是同向信号,说明 GLM 在国际开源 agent 生态里已被默认纳入。

---

## 6. 平台与分发

| 平台 | 状态 |
|---|---|
| **macOS(Apple Silicon)** | macOS 12+,**已签名 + 公证**,自动更新 |
| **Windows 10/11(x64)** | 可用,**尚未代码签名**(SmartScreen 会警告,签名进行中) |
| Linux | README 未提(可从源码跑) |

**自动更新**是产品默认能力("app updates itself, so fixes reach installs quickly")。

---

## 7. 运行方式

前置:Python 3.10+、Node 20+、(桌面壳)Rust toolchain。

```bash
git clone https://github.com/andrewyng/openworker
cd openworker

# 1. 一次性引导,创建 .venv
bash packaging/setup_dev_env.sh

# 2. 启动本地 agent server
.venv/bin/openworker-server --cwd ~/some/project --port 8765

# 3. 第二个终端起 UI
cd surfaces/gui && npm install && npm run dev   # 浏览器 UI(Vite 端口)
# 或:npm run tauri dev  ← 完整桌面 app,Tauri 壳自己拉起并监督 server
```

测试:`.venv/bin/pytest`(server);`npm test` + `npm run e2e`(`surfaces/gui`,hermetic)。桌面包:`packaging/build_dmg.sh` / `build_windows.ps1`。

---

## 8. 隐私与安全姿态

- **本地优先**:agent loop、对话、connector token、model key 全在本地 secret store。
- **唯一上云件**:broker OAuth 握手的小服务。
- **可免登录**:用 connector 时可用手动创建的凭证/API key,不强制账号。
- **审批网关 + 无人值守 inbox**:与 Shannon 的"风险分级 + FullAuto + LLM 分类器回退"是同族设计,但 OpenWorker 的"inbox 暂存"是个产品化的细节。

---

## 9. 对 Shannon 的启示(产品 + 架构)

> 视角前提:本节启示同时作用于两条产品线 —— 编码线(`shannon-code`)和办公线(`shannon-desktop`)。OpenWorker 作为 **shannon-desktop 的直接竞品**,其每一个设计选择都是 Shannon 桌面线的对标基准。

### 9.0 桌面线直接对标(新增,v2)

OpenWorker 与 shannon-desktop 的关键差异(办公赛道视角):

| 维度 | OpenWorker | shannon-desktop | Shannon 落差 |
|---|---|---|---|
| 分发成熟度 | ✅ macOS 签名+公证 + 自动更新 | 🟡 待完善 | 落后 |
| SaaS 集成 | ✅ 25+ 原生 | ❌ 空白 | **严重落后** |
| 后端语言 | Python(aisuite) | Rust | Shannon 更优(轻量/安全) |
| 桌面壳 | Tauri + React | Tauri v2 + React 19 | 同代 |
| 语音输入 | ✅ Rust STT | ❌(计划) | 落后 |
| 附件/多线程 | ✅ | ❌(计划新增) | 落后 |
| Multi-agent | ❌ 单 agent | ✅ Team+worktree | **Shannon 领先** |
| 权限精细度 | 🟡 approval-gated | ✅ 5 级+LLM 分类器 | **Shannon 领先** |
| 开源/i18n | ✅ MIT / ➖ | ✅ MIT / ✅ zh+en | Shannon 更优(中文) |

**裁决**:shannon-desktop 在**引擎层(多 agent、权限、Rust 性能、i18n)**领先,但在**产品完成度(分发、SaaS 集成、附件、语音、多线程)**落后。追赶路径见 [improvement-plan-2026-08.md](./improvement-plan-2026-08.md) 桌面任务块。

### 9.1 架构理念:被权威背书 ✅
Shannon 选的 **Tauri 桌面壳 + 本地优先 + BYO model + MCP + 审批网关** 路线,和 Andrew Ng 团队的选择几乎一致。区别仅在:OpenWorker 的后端是 **Python(aisuite)**,Shannon 是 **Rust**。Rust 在性能、内存安全、单文件分发上更优;Python 在 connector 生态、迭代速度、AI 库覆盖面上更优。**这不是谁对谁错,是不同的工程权衡**——Shannon 应坚持 Rust(差异化点:轻量、可审计、单二进制),但要正视 Python 生态在 SaaS 集成上的速度优势。

### 9.2 SaaS 集成:最大的能力缺口 ⚠️
OpenWorker 的 **25+ SaaS connector** 是 Shannon 完全空白的象限。Shannon 目前的"集成"基本是 MCP(协议层)+ git/文件/终端(本地工程)。如果 Shannon 的产品愿景里有"开发者日常工作流"(读 Jira ticket、写 PR 描述、发 Slack 周报、更新 Notion 文档),那这是**必须补的课**。建议:
- **短期**:把高频 SaaS(GitHub Issues/PR、Slack、Jira、Notion、Linear)做成 **MCP server**(Shannon 已有 MCP 能力,生态现成),而不是自研 connector 框架。
- **长期**:评估是否需要一个 Shannon 自己的"connector gateway"(Shannon 已有 `shannon-gateway` 雏形,见 desktop 的 `commands_connections.rs` / `gateway_supervisor.rs`)。

### 9.3 "交付成品"作为产品语言 💡
OpenWorker 的"成品 > 聊天"是个值得借鉴的产品 framing。Shannon 的桌面端已有 `components/artifact/`(artifact viewer),可以把"artifact 作为一等交付物"做更显性 —— 例如 query 完成后,产出物(代码 diff、文档、报告)以可分享/可导出的 artifact 卡片呈现,而不是淹没在 chat 流里。

### 9.4 调度 + 无人值守 inbox 💡
OpenWorker 的 automations(周期 brief/report)+ 无人值守 inbox,和 Shannon 已有的 scheduled routines / triggered routines 高度同构。Shannon 的 OPC(Scheduled tasks)已在做,但**"无人值守请求进 inbox"**这个安全闭环值得参考——它让"长跑 agent"真正可部署。

### 9.5 STT 语音输入 💡
OpenWorker 用 Rust 写了一个 STT sidecar。Shannon 的 ROADMAP-FUTURE 里 Voice Input 是 P3(whisper-rs / 系统 whisper)。既然 OpenWorker 已验证"Rust STT 旁路"可行,Shannon 可把这个优先级提前,作为桌面端的差异化体验。

### 9.6 aisuite 兼容性 🔍
aisuite 是 Python 生态的事实标准之一。Shannon 虽是 Rust,但可考虑**在 MCP 层提供 aisuite 兼容的 tool schema 或 runtime adapter**,让 aisuite 生态的 toolkit 能被 Shannon 复用(降低 connector 缺口的补救成本)。

---

## 10. 风险与不确定性

| 项 | 说明 |
|---|---|
| **Beta 状态** | 官方说"actively polishing rough edges",API/行为可能变 |
| **PR 政策保守** | 官方明确"按内部列表和目标开发,可能与已开发中功能重复或偏离愿景的 PR 不一定接受"——社区贡献门槛高 |
| **Windows 未签名** | SmartScreen 警告影响首次体验 |
| **无 Linux 官方包** | Shannon 的跨平台定位相对更完整 |
| **未见公开路线图** | README 无 roadmap,战略意图需从 blog/X 推断 |
| **依赖 aisuite 单点** | 引擎绑死 aisuite,若 aisuite 方向偏移会连带影响 |

---

## 11. 结论

OpenWorker 是一个**定位清晰、作者背书强、架构理念先进**的开源桌面 AI coworker,是 **shannon-desktop 在办公赛道的直接竞品**(v2 修正)。它在引擎层不如 Shannon(单 agent、权限粗、Python 后端),但在**产品完成度**(分发、25+ SaaS 集成、附件、语音、自动更新)明显领先。它给 Shannon 三件事上的警钟与灵感:

1. **SaaS 集成护城河** —— shannon-desktop 应尽快用 MCP 把高频办公 SaaS 补上(P1-3,最高优先)。
2. **"交付成品"产品语言** —— shannon-desktop 的 artifact 能力可以更显性化(P3-1)。
3. **本地优先 + Tauri + BYO model** 路线被权威验证 —— Shannon 桌面线坚持 Rust 差异化是对的,但**执行节奏必须加快**(分发、自动更新、附件/语音/多线程)。

建议把 OpenWorker 列入**季度监测名单**(见 [competitor-feature-matrix.md](./competitor-feature-matrix.md) §4),重点关注:connector 新增节奏、automations 演进、中文 SaaS 支持、是否向 coding 场景延伸。

---

## 参考来源

- [GitHub: andrewyng/openworker](https://github.com/andrewyng/openworker)
- [Andrew Ng on X: Announcing OpenWorker](https://x.com/AndrewYNg/status/2080333504446108104)
- [MarkTechPost: Andrew Ng Just Released OpenWorker](https://www.marktechpost.com/2026/07/23/andrew-ng-just-released-openworker-an-open-source-local-first-desktop-ai-coworker-that-returns-finished-deliverables-instead-of-chat/amp/)
- [Reddit r/LocalLLaMA: OpenWorker discussion](https://www.reddit.com/r/LocalLLaMA/comments/1v8wbes/openworker_opensource_local_ai_agent_by_andrew_ng/)
- [Medium: Andrew NG's OpenWorker — AI Coworker](https://medium.com/data-science-in-your-pocket/andrew-ngs-openworker-ai-coworker-for-personal-needs-dd179d613bb7)
- [Trendshift: andrewyng/openworker](https://trendshift.io/repositories/91434)
