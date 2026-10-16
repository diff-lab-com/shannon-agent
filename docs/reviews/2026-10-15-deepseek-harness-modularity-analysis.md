# DeepSeek Harness（dsh）模块化/插件体系调研 · 与 Shannon 对照分析（2026-10-15）

> **状态：v2（2026-10-17 修订）**，原 v1（2026-10-15 落款系笔误）经对抗性审查（redteam，21 项发现）与方案裁决（D0-D4 按推荐）后原地修订；
> **修订记录（v1→v2）**：**事实修正 4 处**——`CLIENT_UPGRADE_REQUIRED` 归因改指 `docs/user/guide/providers.md`（C3）、hook 事件变体数 ~32→30（F15）、npm"先发包后建仓"加注（F16）、落款笔误勘误（F17）。**论证重写 1 处**——§4.3 改为预防性叙事：N-4 缺陷已闭环（registry 级超时已默认接线），P0-1 改为"把已在发生的策略上移收编为显式命名契约"（F1）。**建议降级 3 处**——spill 降为前置调研项并入 9/21 复审 R1.5 工具披露策略（D1）、ACP 子代理改为定位级假设+最小 spike（D2）、插件治理降 P2 并绑定触发条件（D3）。**精度与分级若干**——全文 A/B/C 证据分级（F2/F20/D4）、TL;DR 措辞与限定（F3/F11）、P0-2 锚定 CC 兼容底座（F4）、write-intent 拆待裁决子项+取消语义风险（F5）、WASM"必然"修正（F6）、P1-1 两轴调和附注（F10）、译文标"(译)"（F12）、config_dump 已核验表述（F13）、"未见直接对应"（F14）、P0-3 双暴露（F18）、P0 三条补 size/验收（F19）、P1-4 补条件面前置（F22）、§8 补 permalink（F21）。文件名保留 2026-10-15 以配对 redteam 文档，成文 2026-10-17。
> **方法**：两路并行——（a）dsh 官方仓库/文档/npm/论文的一手核查；（b）本仓架构全量梳理。关键论断经本地代码逐条抽查（file:line 见文内，v2 已逐条复核）。
> **配套阅读**：`docs/harness-arch-review-2026-09-21.md`（§4 竞品对标、§5 路线图）。本文不重复该报告的对标结论，专注其未覆盖的维度：**模块化/插件化机制、组合与治理、生态界面**；§7 给出两文建议的显式映射。

---

## 0. TL;DR

1. **对象属实且分量极重**：DeepSeek 于 2026-08 开源了官方 agent harness——`deepseek-ai/deepseek-harness`（CLI 名 `dsh`），MIT，TypeScript/Node，基于自研插件元框架 **Cordis**（vendored 进 monorepo），口号 "Everything is a Plugin"。上线两个月 ~25 万 stars（GitHub API；2026-10-14 快照；star 仅作热度参考，生态规模按协议采用折算）。开发者预览期，README 自述将有破坏性变更，**核心仓库不收外部 PR**。
2. **基本面趋同是行业共识而非孤例**：append-only 会话日志、OS 沙箱三件套（bwrap/Landlock/Seatbelt）、技能渐进披露、MCP、审批门、压缩，同样见于 Codex/Claude Code/OpenCode——多方收敛验证了 Shannon 现有方向；真正的差距集中在**组合机制、治理与生态界面**，而非基本面能力。
3. **最大结构差异是循环的形态**：dsh 连 agent loop 都是插件（`ctx.agentLoop`），"没有需要打补丁的特权内核"；Shannon 的 `agent_loop.rs` 是 5,966 行单体（文件头自述不可 move-only 细分，已核验）。但 **Shannon 不需要"loop 即插件"**（那是 TS 动态性的产物），需要的是 loop 内职责的**命名阶段化**——`guard_nodes.rs` 已开了头，dsh 的工具执行管线文档恰好提供了目标形态。
4. **最具借鉴价值的五件事（v2）**：① 工具执行管线契约化（收编既有策略，§4.3）；② 事件派发语义表（以 Claude Code hooks 契约为兼容底座，dsh 五模式为内部描述词汇）；③ 生效装配树 dump；④ spill——**降为条件项，先取痛点证据（裁决 D1）**；⑤ 子代理 provider 化——**定位级假设，先做最小 spike（裁决 D2）**。
5. **明确不建议抄**：行级 patch 式配置层叠、"一切皆插件"的极端纯度、harness↔API 私有遥测上报（`dsh_session_log` 上传会话日志属锁定行为，与 Shannon BYOK 中立定位冲突）。

---

## 1. 调研对象与证据等级

### 1.1 dsh 是什么（一手核实）

| 项 | 事实 | 来源 | 级别 |
|---|---|---|---|
| 仓库 | `github.com/deepseek-ai/deepseek-harness`，描述 "Everything is a Plugin."，MIT，TypeScript，创建 2026-08-13，249,872 stars / 30,048 forks（2026-10-14），持续推送 | GitHub API | A |
| 发布 | npm `@deepseek-ai/dsh`，首发 2026-08-10——**先发包，后于 08-13 建仓/公开**（时序注，防读者当矛盾），latest `0.2.0-rc.2`（2026-10-14），32 个版本；产品页 deepseek.com/en/harness（桌面/Web/源码三形态） | npm registry API | A |
| 插件内核 | **Cordis** 元框架（cordiverse/cordis，2022 年起），以 4.0.0-rc.7 **vendored** 进 dsh monorepo："完全拥有框架层（可审计、可补丁、可钉版本）"(译)。注：**"源自 Koishi 聊天机器人生态"系二手佐证**（`@cordisjs` scope + 第三方文章，cordiverse README 本身未声明），可信度中 | `vendor/README.md`、cordiverse/cordis | vendored=A；Koishi 渊源=C |
| 理论 | arXiv:2608.25512《A Programming Paradigm for Spatiotemporal Composability》（北大+DeepSeek-AI，2026-08-26，92 页），Cordis 为其参考实现 | arXiv | A |
| 成熟度 | **开发者预览**：README 明示破坏性变更；SAFETY.md 明示未做安全审计；CONTRIBUTING 明确**不接受外部 PR**，社区参与=Discussion/写插件（`dsh-plugin` topic）/写教程。原话："你可以把这个仓库当作一个理念、一个官方示范和灵感来源，而不是我们的强制规范"(译) | 仓库一手文档 | A |
| 团队背景 | 知乎：2026-05 组建的代码智能体团队，对标 Claude Code 桌面端产品（二手，可信度中） | zhihu.com/question/2071331484284220938 | C |

**证据等级说明**：本文 dsh 侧事实绝大多数出自仓库一手文档（README、docs/*.md、packages/*/README），少数生态规模数字出自 GitHub/npm API；未能抓取原文的二手材料（DataCamp×2、InfoQ、腾讯云教程、teamorouter 对比文，均 403/超时）一律标注 **UNVERIFIED** 且不进入论证链。**分级口径（v2，D4）**：A=多源或一手直读原文；B=单一一手来源；C=二手/摘要——§1.1 表逐行标注，§2 各小节标题尾部标注；reviewer 抽查 A 级即可。

### 1.2 生态快照（规模 ≠ 质量，需甄别）

- 插件发现：GitHub topic `dsh-plugin` **19,325 个仓库**（API 计数；含大量搭便车标签仓库）。
- 社区清单：`awesome-dsh-plugin`（18.8k stars，23 个分类：记忆/浏览器/语音/Git 评审/安全权限/远程移动端等）；社区市场 `dsh-market`（`dsh plugin add dshmarket` 一键装）；`dsh-find-plugin`（让 agent 自己找插件）。
- 第三方桌面客户端：anywhere-labs/dsh-desktop（30.9k stars）、dataelement/dsh-desktop（14.1k）等——均遵循 `dsh.bundle` 协议而非适配特定客户端。
- 教程：`learn-harness-engineering`（19.9k stars）；官方 `awesome-deepseek-agent`（6.2k）列出 23 个可接 DeepSeek 模型的第三方 harness。
- SDK：TypeScript（newline-delimited JSON-RPC over stdio）+ Python（wheel 内置 dsh 运行时）；另有 ACP server 供编辑器接入。

---

## 2. dsh 架构拆解（事实层）

### 2.1 "一切皆插件"：Cordis 五要素（A）

插件即 Service；context 是服务仓库（服务认领稳定 `ctx.<key>`）；`inject` 声明依赖决定加载顺序；类型化事件有**五种派发模式**；一切注册都是**可逆 effect**（卸载时回滚）。支持 YAML 配置热更新（HMR）。

关键断言（`docs/architecture.md`）：**agent 循环本身也是插件**（`core/agent-loop`，挂载为 `ctx.agentLoop`）——"没有需要打补丁的特权核心——扩展 dsh 就是在其他插件旁边挂载一个插件"(译)。模型适配器、工具注册表、会话日志、UI 全部同构。

### 2.2 组合模型：Profile → Bundle → Patch 层叠（B）

运行中的 dsh 是启动时按有序层组合的插件树：**profile**（`web`/`headless`/`sdk`/`sdk-minimal`/`acp` 五种出厂模板）→ **有序 bundles**（`dsh-base` 为共享首层：模型适配器、工具、持久化、沙箱与审批策略、设置、凭据、遥测；**例外：`sdk-minimal` 显式不套用 `dsh-base`**——五种里只有四种共享首层）→ profile 的 `cordis.patch.yml` → home patch → `--patch` 覆盖。patch 按 id 定位配置行做整行替换/插入。`dsh --profile web --dump-config` 打印本机插件树——**"打印出的每一行都能被你自己的 patch 替换"(译)**。

### 2.3 事件即扩展点，派发模式是公共契约（A）

`emit` / `waterfall` / `parallel` / `serial` / `bail` 五种模式；waterfall 即 around 中间件（策略监听器可短路，观察者必须 `next()`）；事件目录用 `@mode` 标签**机检**。

### 2.4 工具执行管线（`docs/tool-execution-pipeline.md`）（A）

`tool/call` 落日志 → `tools/pre-execute` **waterfall**（hook、权限、沙箱）→ 单调守卫（deny/abstain）→ `ctx.approval` 一次性审批 → `tools/execute` **around**（超时/重试/指标）→ 工具体 → `fs/write-intent` 门 → `tools/post-execute` → 规范化 → `tools/result`。PTC 子调用带父 token 走同一管线。

### 2.5 权限与沙箱（B）

三档沙箱模式 `read-only` / `workspace-write` / `danger-full-access`；被拒调用可经**用户批准的一次性升级（escalation）**重试；权限模式（含 `plan` 只读）按会话生效；插件安装、构建脚本、版本豁免各有独立审批。本地隔离：Linux bwrap（优先）/Landlock、macOS Seatbelt、Windows 受限令牌+ACL；**容器/microVM/远程执行器定位为"替换整个能力"的 seam**（把 fs/subprocess provider 指向远程即可整体搬移 Bash/PTY/LSP）。

### 2.6 子代理 = provider 注册表（B）

统一 `ctx.subagents` 接口 + 模型工具（`subagent`/`interrupt_agent`/`list_agents`/`send_message`），provider 极宽：进程内新起 / 历史种子 fork / **ACP 出进程 / 真 Codex（app-server 协议）/ 真 Claude Code（官方 Agent SDK）/ dsh-SDK**。实验性 Agent Teams；**workflow 工具**让模型在沙箱化 PTC Node 运行时里写编排脚本扇出子代理。

### 2.7 上下文管理：三个独立插件家族（B）

- **compaction**：自动压缩 + `/compact` + 工具结果裁剪 + 图片卸载；
- **spill**：超长工具结果**落盘、返回定位符**（可回读，不丢数据）；
- **guard**：重复工具调用提醒、工具超时策略。

会话日志 append-only 且"**model-visible means logged**"（模型所见必须可从日志重建），格式版本化迁移（`session.vN.jsonl[.zstd]`）。

### 2.8 技能 / MCP（B）

技能=Claude Code 式渐进披露（目录常驻、正文经 `skill` 工具或 `/name` 按需加载），文件系统发现 + 内置办公技能。MCP：客户端按配置连接（server-qualified 命名）+ 资源共享工具；ACP 客户端可 attach 标准 MCP servers。

### 2.9 分发与治理（B）

插件=npm 包 + `package.json` 的 `dsh.bundle` 字段指向 patch 文件；安装走 pnpm，内置 **npm→npmmirror 回退**（对中国网络的显式设计）；注册前 `git ls-remote` 预检；peer 版本兼容检查 + `compatibility.json` **显式风险豁免（`acceptRisk: true`）**；运行中 HMR 换插件。

### 2.10 harness↔API 协同设计（定性为"锁定"）（A）

`docs/deepseek-llm-api-wire-extensions.md`（该文档仅涵盖请求头/`dsh_plugin_packages`/`dsh_session_log`；**`CLIENT_UPGRADE_REQUIRED` 实出自 `docs/user/guide/providers.md`**——v1 误归入前者，v2 已修正）：官方适配器向 DeepSeek 端点发送私有 wire 扩展——请求头 `x-deepseek-harness-user-id/session-id/compact`，请求体 `dsh_plugin_packages`（上报激活插件清单）与 **`dsh_session_log`**（水位线 at-least-once 增量上传会话日志，默认 8MiB/请求）；端点可返回 `CLIENT_UPGRADE_REQUIRED` **按 harness 版本门控模型**。→ 官方 harness 与官方 API 是协同设计的；换第三方端点这些扩展即不发送。配套背景：V4.1-Flash（2026-09-10）以 agent 成本为叙事（cache-hit 计费是 agent 成本大头、KV 压缩省 1/4 HBM、谷峰半价）；API 同时提供 OpenAI 与 Anthropic 双协议端点。

---

## 3. Shannon 对照基线（现状速写）

### 3.1 基本盘速览

| 维度 | dsh | Shannon（本仓） |
|---|---|---|
| 语言/形态 | TS/Node，单二进制 npx + Electron 桌面 | Rust，22 crates（527K LOC，12,585 tests，`docs/metrics.md`） |
| 插件内核 | Cordis，动态加载 + HMR | 编译期装配 + manifest 发现（tools/plugins/skills/hooks/MCP 多机制并存） |
| 循环 | 插件（`ctx.agentLoop`），无特权内核 | 单体：`agent_loop.rs` **5,966 行**，文件头自述不可 move-only 细分（已核验） |
| 事件 | 五种派发模式，`@mode` 机检 | EventBus pub/sub + mpsc 兼容 facade + hooks（30 个事件变体，exit-code 语义） |
| 工具管线 | 显式八段管线 | `guard_nodes.rs`（787 行，PermissionGate + PreToolUseHook 两节点）+ registry 级 `execution_timeout`（原始 around 层，`tools.rs:233`） |
| 沙箱 | read-only/workspace-write/danger-full-access + escalation | Landlock/Seatbelt/bwrap 装配 + **远程 world**（SSH/Docker，`ToolProviders` seam，`shannon-tools/src/lib.rs:260-270` 已核验） |
| 子代理 | provider 注册表（含真 CC/Codex） | `AgentExecutor` trait（进程内 / `shannon-agent` JSON-RPC 两实现） |
| 上下文 | compaction/spill/guard 三家族 | compact 双 crate + micro-prune + 辅助摘要器；**无 spill（前台）**（bash 2MiB 截断即丢弃，`system.rs:35,175-176`）；后台任务另有 task store 输出路径（`task_output.rs`） |
| 分发 | npm + npmmirror 回退 + `acceptRisk` 豁免 | crates.io + 桌面安装包 + 社区 `PluginIndex`（`registry_url`，已核验） |
| 治理 | 核心不收 PR，协议即生态 | `stability-attr` + `docs/STABILITY.md` + Claude 兼容策略 |

### 3.2 基本面趋同清单（行业共识而非孤例——验证既有方向）

两套 harness 与 Codex/Claude Code/OpenCode 收敛于同一套 2025-26 行业共识基本面：append-only 会话日志（Shannon L0 log-as-subscribe vs dsh `core/session`）、OS 沙箱三件套、技能渐进披露 + 注入扫描、MCP 客户端、审批门 + 权限模式（Shannon 4+3 阶梯 vs dsh 三档+escalation）、压缩 + 工具结果裁剪、deferred 工具加载（Shannon tool-search vs dsh 作用域注册表）。**结论：Shannon 的基本面不需要因 dsh 改向。**

### 3.3 Shannon 已有但未正式化的 seam（借鉴的落点）

1. `guard_nodes.rs` 的可组合守卫链——管线的雏形，但只有两个节点且未作为公共契约文档化；
2. EventBus 订阅者模型（"log-as-subscribe"）——可承载更多 first-party 功能为订阅者；
3. 插件 manifest v2 已**预留 `type = "wasm"` 槽位**（schema 接受、load 拒绝，"reserved §4.16 slot"；`manifest.rs:478` 已核验）——WASM 是既定终局；
4. `ContextTransform`（`shannon-plugin-api`，I1-I4 不变量）——第一个引擎↔插件中间件契约；
5. `ToolProviders` world seam——沙箱/远程/扩展的执行接缝；
6. `AgentExecutor` trait——替代子代理运行时的接缝；
7. `--dump-config` + `config explain`（`commands_config_explain.rs`，分层 provenance **已存在**，已核验）——配置可观测性的地基。

### 3.4 结构性差距（dsh 视角下）

- **循环单体**：模型路由、vision 门、压缩、重试渲染、hook 发布、secret-guard 全在一个 5,966 行嵌套函数里；
- **装配点多点手工**：TUI（`shannon-ui/src/repl/mod.rs:321,449`）、desktop（内嵌 engine）、server 各自装配 registry，靠 CI"headless purity"检查防漂移；
- **first-party 工具硬编码注册**：`register_all_tools` 约 90 个 `register()` 调用（`shannon-tools/src/lib.rs:292-636`），无 manifest 化发现（skills/plugins 有，内置工具没有）。

---

## 4. 深度对比：五个本质差异

### 4.1 动态性边界决定形态——"一切皆插件"在 Rust 不成立，纪律却可移植

dsh 的极端插件化建立在 TS 运行时的廉价动态性上（HMR、pnpm 装载、模块热替换）。Rust 做不到同等成本的动态加载；Shannon 的等价物：manifest v2 已预留 WASM 槽（仅 schema，零 runtime 依赖，实测无 wasmtime/rhai），进程外已有（MCP/`shannon-agent` JSON-RPC）；嵌入式脚本引擎是 hook 类轻扩展的备选，本报告不推荐（安全面/审计成本）但列示以保论证完整。因此正确的姿势不是复制形态，而是移植 **Cordis 的四条纪律**：① 每个能力有稳定名字（`ctx.<key>` ↔ Shannon 的 crate/模块边界）；② 依赖显式声明（`inject` ↔ trait 依赖）；③ 注册可逆（安装/卸载对称）；④ **配置可打印**（见 4.2）。

### 4.2 可观测性是架构目标，不是调试附件

dsh 把"运行中系统 = 一份可打印、可逐行替换的插件树"作为核心卖点。Shannon 已有 `--dump-config`（分层 provenance；`config_dump` 覆盖配置层与 env 映射，不含装配面——本次已核验，`config_dump.rs`），**缺的是"生效装配树"视图**：实际挂载了哪些工具（含 deferred/MCP/插件来源）、哪些 hook 挂在哪些事件、哪些 skill 被预算裁掉、压缩策略当前参数、生效的沙箱模式与 world。多装配点（TUI/desktop/server）的漂移风险恰恰需要这个视图来审计——比 CI purity 检查更根本。**注意**：行级 patch 层叠不值得抄（Shannon 的 typed config 更稳），要抄的是"**生效视图一等公民**"这个标准。

### 4.3 特权内核 vs 阶段化宿主——从一次已闭环的缺陷说起

9/21 复审 N-4 记录过一类真实发生的缺陷：Bash 无默认超时、registry 级超时未接线——超时本应由宿主统一施加，当时却依赖工具体自律。该缺陷现已闭环：registry 级 `execution_timeout` 构造即默认安装，`execute()`/`execute_streaming()` 均以 `tokio::time::timeout` 包裹（`tools.rs:190-233,307-318`）；bash 亦有 120s 默认值且已使用（`system.rs:23,126`）。

这段历史的可迁移结论不是"缺超时要补"，而是：**策略上移到宿主/注册表是可行且已经在发生的**——registry 超时本质上就是一个原始的 around 层。当前的真实缺口有三：
1. **策略分散且未契约化**：超时在 registry、重试/故障切换在 LLM client（`retry.rs`）、沙箱在 `ToolProviders`——没有统一的"工具执行管线"命名契约，新策略（写意图门、审计采样、限流）无处安放，N-4 类"漏接"仍可能以别的形式复发；
2. **write-intent 门无对应物**：与 `WorldSandboxHandle` 写策略的协同未定义；
3. **节点序列与失败语义未文档化**：权限→hook→执行→后处理的行为语义只能读源码（与 P0-2 同根）。

因此 P0-1 的准确表述是：**把已经在发生的策略上移收编为显式命名契约**（guard_nodes 归位 + 缺口节点），使"策略漏接"类缺陷结构性不可能——这是预防性论证，不是修复性论证。

### 4.4 事件语义不声明就是坑

Shannon 的 EventBus + hooks 已有相当事件面（30 个变体，枚举至 `events.rs:70`），但派发语义（顺序？可短路？失败即断还是观察？）散在实现里（hooks 的 exit-code-2 语义、EventBus 的 L0 镜像），hook/plugin 作者只能读源码。dsh 把五种派发模式做成公共契约并机检 `@mode`。9/21 复审 R2.3（hooks 事件面扩展）的前置条件正是**语义表先行**——否则事件越多，行为越不可预测。

### 4.5 子代理 provider 化是生态定位的反转点（定位级假设，待 spike 验证）

dsh 能把**真 Claude Code / 真 Codex** 当子代理（ACP / Agent SDK provider，保留各自认证）。Shannon 的 `AgentExecutor` trait 就是为"换运行时"留的缝，但目前只有自家两实现。补一个 ACP provider 的含义：Shannon 从"Claude Code 生态文件兼容者"（读 `.claude/agents`、hooks.json、`.mcp.json`）升级为"**Claude Code 的编排者**"——用 BYOK 多 provider 底座做 lead，把 CC/Codex 当可插拔 worker。这是 dsh 已经验证过需求的定位级机会（9/21 复审差距 #6 cross-model oracle 也可以顺路由它实现）。

但 v2 起按**假设**对待，不再当作"现成可做"：实测 Shannon 零 ACP 代码（`grep -rin "\bacp\b" crates/` 无命中）——这意味着全新协议实现 + 进程管理 + 凭据隔离；且有两个必答问题待答：①**安全面**——驱动真 CC/Codex 意味着把子进程接上各自的凭据与审批流，逃逸面与审计责任如何划分；②**定位张力**——编排专有二进制与 BYOK 中立叙事是否冲突（也可正向表述为"编排者天然中立"，但需要论证）。

定位级假设——"子代理 provider 化可把 Shannon 从生态兼容者升级为编排者"。最小 spike：ACP client 骨架（会话/提示/流式结果）+ 真 Claude Code 子代理 round-trip + 凭据隔离方案（provider 自持凭据，Shannon 不透传自身凭据链）；验收：一次端到端委派；out-of-scope：Codex app-server、teams、并发编排。spike 结果回填后重估"代价最小"论断。

---

## 5. 借鉴建议（供裁决，不构成排期承诺）

### P0 · 直接落地，低风险

| # | 建议 | 落点 | 说明 |
|---|---|---|---|
| P0-1 | **工具执行管线契约化**：既有 registry 级超时（`tools.rs:233`）与 guard_nodes 两节点归位为命名节点；补 around 缺口（重试/指标统一施加点）与 post-execute 归一化；write-intent 门单独裁决 | `guard_nodes.rs` + `tools.rs` | 兼容锚定见 P0-2；低风险部分=归位，缺口部分需先做流式/长驻工具取消语义评估（PTY/browser/LSP）。size S-M（归位 S；缺口 M，视取消语义评估）。验收：节点契约文档 + 既有超时行为回归全绿 + 新增节点各 ≥1 单测 |
| P0-2 | **事件/钩子派发语义表**：每个 HookEventType 声明 mode（可短路/必须放行/串并行/失败语义），CI 机检。语义表参照系：Claude Code 已文档化 hooks 行为为**兼容底座**（事件名、exit-code 0/2、matcher——Shannon 兼容承诺的一部分）；dsh 五模式仅作内部实现语义的**描述词汇**，不进入对外承诺。表结构：事件 × {派发模式, 可短路?, 失败语义, CC 兼容注记}。 | `shannon-engine/src/hooks/events.rs` + 文档 | 是 R2.3 hooks 扩展的前置；对齐 Claude Code hooks 兼容面时同样需要。size S。验收：30 个事件全部入表 + CI 机检标签上线 |
| P0-3 | **生效装配树 dump**：`--dump-config` 之上加"装配视图"——生效工具清单（含来源 builtin/MCP/plugin/deferred 状态）、hook 挂载表、skill 预算裁剪结果、压缩参数、沙箱模式/world | `config_dump` 扩展 | 多装配点漂移的审计工具；对用户排障（"我的工具呢"）同样是一等答案。core 侧生成装配视图，CLI flag 与 `/api` 端点双暴露（desktop 经内嵌 engine 可达，gateway 经 WS 取同数据）。size M。验收：CLI 输出含工具（含来源/deferred 状态）、hook 挂载、skill 预算裁剪、压缩参数、沙箱模式；`/api` 暴露同一数据 |

### P1 · 中期，借形不借体

| # | 建议 | 落点 | 说明 |
|---|---|---|---|
| P1-1 | **agent_loop 阶段化**（吸收管线目标形态，不做"loop 即插件"） | 并入 9/21 复审 R2.1 engine 拆分 | 目标是"命名阶段的宿主"，阶段间以事件/节点通信；不是把 loop 拆成 Cordis 式动态插件。附注（F10）：R2.1 是状态机拆分（相位轴），本条是管线宿主（节点轴），需先一页纸调和再并入，阶段化为第二步。size L（两步走）。验收：一页纸目标形态调和文档 + 阶段化后既有 engine 测试全绿 |
| P1-2 | **spill**——前置调研项，并入 9/21 复审 R1.5 工具披露策略统一设计；进入实现前须先取痛点证据（eval/场景日志中截断事件与后续失败率统计）（裁决 D1） | bash/exec/web 工具输出面（`system.rs:35,175-176` 现有 2MiB 截断即丢弃；后台任务另有 task store 输出路径 `task_output.rs`） | 价值未证：2MiB 上限远宽于同类（CC ~30k 字符量级），被丢尾多为噪声；成本面（磁盘生命周期/回读定位符/与 tool-result 缓存交互）见 redteam F7。size：调研 S（实现 M，若立项）。验收（调研）：截断事件与后续失败率统计报告，给出立项/放弃结论 |
| P1-3 | **子代理 provider 注册表——定位级假设，先做最小 spike（裁决 D2）**：最小 spike = ACP client 骨架（会话/提示/流式结果）+ 真 Claude Code 子代理 round-trip + 凭据隔离方案（provider 自持凭据，Shannon 不透传自身凭据链）；out-of-scope：Codex app-server、teams、并发编排 | `shannon-agents` | 定位级机会（§4.5）；cross-model oracle（9/21 差距 #6）待 spike 通过后顺路评估。size S（spike）。验收：一次端到端委派 |
| P1-4 | **first-party 工具 manifest 化注册**：注册数据（name/category/risk/default-on）从代码调用剥离为声明表，`register_all_tools` 变为解释器 | `shannon-tools/src/lib.rs:292-636` | 与 R1.5 工具披露策略（默认核心集+ToolSearch 门控）共用同一张表；为未来 WASM 工具统一语义。**前置=条件面盘点（F22）**：`register_all_tools` ~90 个调用含条件注册与平台门控（remote world 相关 background 工具按 `is_remote` 条件注册，`lib.rs:438-447`；applescript/windows_platform 注册无条件、执行按 OS 门控，`lib.rs:584-601`），声明表必须能表达条件注册，否则静默回归。size M。验收：声明表与现注册面逐项对账零差异（含条件项）+ 注册顺序回归全绿 |

### P2 · 观察 / 选配

（P2 为观察/选配项，未立项，故不附 size/验收；若立项按 P0/P1 格式补齐。）

- **P2-1 workflow/PTC 编排工具**（模型写脚本扇出子代理）：Shannon 有 teams/coordinator，缺"代码即编排"；等子代理 provider 化后再评估。
- **P2-2 权限一次性升级（escalation）UX**：被拒调用经用户批准原样重试一次；与 `max_auto_approvals` 熔断器语义互补。
- **P2-3 评测 harness 披露惯例**：V3.2 报告逐基准披露所用 harness 并做跨框架一致性检验（SWE-bench 72–74 跨框架稳定）；Shannon 接 Terminal-Bench（R2.6）时采用同样口径，公开可比。
- **P2-4 harness↔API 协同的"可移植版"**：不抄遥测，但把 effort 档位、缓存断点、toolsets 等**可协商能力**继续沿 provider 能力面标准化（已有雏形：`toolsets.rs`、三层缓存断点注入）。
- **P2-5 插件治理补齐**（原 P1-5，裁决 D3 降级）：peer 版本兼容检查 + 显式风险豁免（`acceptRisk` 式）+ 安装预检；Shannon 已有 PluginIndex/权限门，缺 dsh 的版本豁免与预检机制。**触发条件：社区 index 上架首个第三方插件。**

### 明确不抄清单

1. **行级 patch 配置层叠**：Shannon 的 typed 分层 config + provenance 更稳；patch 机制的价值已被 P0-3 的"生效视图"吸收。
2. **"一切皆插件"极端纯度**：TS 专属；Rust 侧强行模仿只会得到 FFI 边界税与安全面扩大。社区对 dsh 的批评（"太极客"、npx 门槛、"真正该卷的是模型能力"）值得引以为戒。
3. **私有 wire 遥测**（`dsh_session_log` 上传会话日志、按 harness 版本门控模型）：与 BYOK 中立定位正面冲突；Shannon 的差异化恰是"不锁定"。
4. **dsh 的沙箱松点**：第三方评测指出其沙箱"限写不限读"（可读 workspace 外文件）。Shannon 的 world 实现更完整，不要在对齐 dsh 三档命名时倒退语义。
5. **插件秀优先级**：dsh 自身处于预览期、破坏性变更频繁、官方不收 PR——其治理姿态适合"理念示范"，不适合 Shannon 当前"可日常使用"的产品阶段。

---

## 6. 风险与反方观点

- **重构风险**：P1-1 依赖 5,966 行 loop 的阶段化，9/21 复审已把它列为 R2.1 并注明"测试已就位"；本文不另立排期，仅提供目标形态参照。注意两轴调和（F10）：R2.1 是状态机拆分（相位轴），本条是管线宿主（节点轴），需先一页纸调和再并入。
- **取消语义风险（P0-1 缺口部分，F5）**：around 层超时对流式/长驻工具（PTY、browser、LSP、后台任务）的取消语义是出名难缠的问题——超时触发时是杀整个进程组还是仅断开流？已产出的部分结果如何呈现？write-intent 门与现有 `WorldSandboxHandle` 写策略（`shannon-tools/src/lib.rs:269`）的关系（重复检查？谁先谁后？）亦未定义。故 P0-1 的"低风险"仅限既有节点归位部分，缺口部分须先做取消语义评估。
- **条件注册静默回归（F22）**：first-party 工具注册含条件分支与平台门控（remote world 相关 background 工具按 `is_remote` 条件注册，`lib.rs:438-447`；applescript/windows_platform 注册无条件、执行按 OS 门控）——P1-4 的声明表必须能表达条件注册，否则 manifest 化时注册面静默回归。
- **ROI 反方**："管线化/事件表/dump 视图"都是工程内功，不直接产出用户可见能力；反方观点是先把 R0/R1 的正确性与质量债还完。本文的回应：P0 三项都搭在已有机制（N-4 的闭环成果、R2.3 路线）之上，归位部分是"顺路收益"；缺口部分不再是"零风险新战场"，须先过取消语义评估（见上）。
- **生态现实**：dsh 19,325 个 topic 仓库含大量搭便车；awesome-dsh-plugin 的 18.8k star 不等于可用插件密度。评估 dsh 生态时按"协议采用数"而非"仓库数"折算。
- **UNVERIFIED 项**：DataCamp×2、InfoQ、腾讯云教程、teamorouter 对比文未能抓取原文，相关说法未进入论证链。

---

## 7. 与 9/21 复审的映射

| 本文建议 | 9/21 复审对应 | 关系 |
|---|---|---|
| P0-1 管线契约化 | N-4（超时未接线，现已闭环）、R2.1（engine 拆分） | 提供目标形态并收编 N-4 闭环成果；与 R2.1 的两轴调和见 P1-1 附注 |
| P0-2 事件语义表 | R2.3（hooks 事件面扩展） | 前置条件 |
| P0-3 装配树 dump | 未见直接对应 | 新增；服务多装配点漂移治理 |
| P1-1 agent_loop 阶段化 | R2.1（engine 拆分） | 两轴调和：R2.1 是状态机拆分（相位轴），本条是管线宿主（节点轴）；先一页纸调和，阶段化为第二步 |
| P1-2 spill | §3.4 压缩有损关切、R1.5 工具披露策略 | 互补机制；前置调研项，并入 R1.5 统一设计（D1） |
| P1-3 ACP 子代理 | 差距 #6 cross-model oracle | 实现路径（定位级假设，待 spike 验证） |
| P1-4 工具 manifest 化 | R1.5 工具披露策略 | 同一张声明表；前置=条件面盘点（F22） |
| P2-5 插件治理 | 未见直接对应 | 新增；触发条件=社区 index 上架首个第三方插件（D3） |
| P2-3 评测披露 + tbench | R2.6 Terminal-Bench | 口径惯例 |
| 不抄清单 #4 沙箱语义 | R2.2 沙箱默认化 | 防倒退护栏 |

---

## 8. 来源清单

**dsh 一手**（文档 permalink 均走 GitHub master 路径）：
- 仓库 <https://github.com/deepseek-ai/deepseek-harness>（README、CONTRIBUTING.md、SAFETY.md、vendor/README.md）
- 文档 <https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/>：architecture.md、cordis-primer.md、tool-execution-pipeline.md、tool-catalog.md、config-catalog.md、deepseek-llm-api-wire-extensions.md（**仅含请求头/`dsh_plugin_packages`/`dsh_session_log`**；`CLIENT_UPGRADE_REQUIRED` 实出自 <https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/user/guide/providers.md>）；packages/{sandbox,subagent,skill,boot/plugin-manager}/README
- npm：<https://www.npmjs.com/package/@deepseek-ai/dsh>；产品页 <https://deepseek.com/en/harness>
- Cordis：<https://github.com/cordiverse/cordis>；理论：arXiv:2608.25512 <https://arxiv.org/abs/2608.25512>

**模型/API 一手**：arXiv:2512.02556（V3.2，§4.1 评测口径）<https://arxiv.org/abs/2512.02556>；api-docs.deepseek.com（双协议端点、thinking/reasoning_effort、news260910 V4.1-Flash）<https://api-docs.deepseek.com>

**生态/第三方**（API 计数抓取端点=api.github.com 搜索/topic 页，**2026-10-14 快照**；redteam 2026-10-16 复测：topic 20,058、repo 251,080 star——量级不变，下列数字均为时点值）：awesome-dsh-plugin 18.8k（<https://github.com/search?q=awesome-dsh-plugin&type=repositories>）、anywhere-labs/dsh-desktop 30.9k（<https://github.com/anywhere-labs/dsh-desktop>）、dataelement/dsh-desktop 14.1k（<https://github.com/dataelement/dsh-desktop>）、learn-harness-engineering 19.9k（<https://github.com/search?q=learn-harness-engineering&type=repositories>）、awesome-deepseek-agent 6.2k（<https://github.com/search?q=awesome-deepseek-agent&type=repositories>）、dsh-plugin topic 19,325（<https://github.com/topics/dsh-plugin>）；DeepInfra 评测 <https://deepinfra.com/blog/deepseek-harness-review>（2026-09-09）；团队背景 <https://www.zhihu.com/question/2071331484284220938>（2026-08-13；二手，v1 §8 链接少了几位数字，v2 以 §1.1 为准统一）。

**Shannon 本仓**：`crates/shannon-core/src/query_engine/engine/agent_loop.rs`（5,966 行，头注释）；`crates/shannon-core/src/query_engine/guard_nodes.rs`（787 行）；`crates/shannon-core/src/tools.rs:190-233,307-318,581-582,737-741`；`crates/shannon-tool-interface/src/{lib.rs,sandbox.rs}`；`crates/shannon-tools/src/{lib.rs:259-276,292-636,438-447,584-601,task_output.rs,system.rs:23,126,175-176}`；`crates/shannon-core/src/plugin/{manifest.rs:478,registry.rs:573,ECOSYSTEM.md}`；`crates/shannon-core/src/config_dump.rs`；`crates/shannon-engine/src/{hooks/events.rs:9-70,permissions.rs,api/adapter.rs}`；`crates/shannon-ui/src/repl/mod.rs:321,449`；`crates/shannon-cli/src/commands_config_explain.rs`；`docs/metrics.md`；`docs/harness-arch-review-2026-09-21.md`。

*（dsh 侧数字统一为 2026-10-14 快照（redteam 2026-10-16 复测量级不变）；本仓 file:line 以修订时工作区 docs/deepseek-harness-analysis-r2（基线 2f89e518a，2026-10-17 复核）为准。）*
