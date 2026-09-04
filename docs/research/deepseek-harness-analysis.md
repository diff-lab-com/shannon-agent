# DeepSeek Harness（dsh）深度调研分析报告

- 日期：2026-08-27
- 性质：外部项目调研（信息来自公开仓库文档 / 架构参考站 / 社区反馈，截至 2026-08-26）
- 关联：[Pi agent 调研](pi-agent-analysis.md) · [Shannon 差距分析](shannon-gap-analysis.md) · [插件架构评审](shannon-plugin-architecture-evaluation.md) · [trace 改进方案](shannon-trace-improvement-plan.md) · [评测方案](agent-eval-landscape-and-plan.md)

## 0. 一句话结论

dsh 是目前把「agent 框架即插件系统」做得最彻底的公开实现：借 Cordis（源自 Koishi 生态）把模型适配、工具注册表、会话日志、乃至 agent loop 本身全部做成可挂载、可替换、可热卸载的插件。对 Shannon 最有价值的不是照搬这套 TS 运行时，而是四条可迁移的思想：**事件溯源式会话日志、「模型可见即已记录」不变量、capability seam（能力接缝）、可逆效应（reversible effects）**。

## 1. 项目概况

| 维度 | 事实 |
|---|---|
| 仓库 | github.com/deepseek-ai/deepseek-harness（npm：@deepseek-ai/dsh） |
| 形态 | pnpm monorepo，约 221 个包，TypeScript |
| 定位 | 开源 agent harness，为 DeepSeek V4 系模型原生优化，同时支持 Anthropic/OpenAI 等目录供应商 |
| 状态 | 2026-08 进入公开 RC（rc.2/rc.3/rc.5 密集发布），冲刺 0.1.0；Beta 报名 4 天 712 个项目 |
| 产品面 | dsh web（浏览器应用）/ dsh headless（一次性任务）；四种运行模式（Plan、Agent、Operate、审批姿态） |
| 生态 | GitHub topic dsh-plugin / deepseek-harness；不同目录站口径 74（精选）～700+（声称），官方文档称 topic 下 6600+ 仓库（含非插件项目，口径未一） |
| 跨语言 | Python SDK（ACP 桥，Python agent 可作为子 agent 参与）；native/landlock-run（Linux 内核级沙箱） |

生态目录站（非官方）：awesome-deepseek-harness-plugins（walkinglabs/vvlife/kejixiaoliang 三个版本）、dshpluginstore.com、dshplugin.io、dsh.deepseek404.com（中文站）、dsh-plugin.club、dsh-plugins.net。插件以 npm 包分发，安装命令为 dsh plugin --profile &lt;name&gt; add &lt;pkg&gt;。

## 2. 架构总览：一切皆插件

运行中的 dsh 是一棵**插件树**，启动时按序叠加各层组合而成。没有需要打补丁的特权内核。

### 2.1 Cordis 五核心理念

| 理念 | 内容 |
|---|---|
| 插件 = 服务 | 插件是带 inject / apply(ctx) 的对象或 Service 子类，生命周期挂载到上下文 |
| 上下文 = 服务仓库 | 服务声明稳定的 ctx.<key>（ctx.tools、ctx.llm、ctx.sessions…），消费者按键发现，从不 import 具体实现 |
| inject 依赖注入 | 声明所需服务的插件等待其就绪，加载顺序由依赖推导，非手工引导排序 |
| 类型化事件 | 服务用 TS declaration merging 声明事件名，按协调语义派发 |
| 可逆效应 | 一切注册（提示词片段、工具 schema、适配器、监听器）经 ctx.effect() / ctx.on() 安装，卸载时自动逆撤销 |

### 2.2 四种事件派发模式（协调契约）

| 模式 | 等待 | 顺序 | 返回值 | 场景 |
|---|---|---|---|---|
| emit | 否 | 注册序 | 无 | 观察者（遥测、日志） |
| waterfall | 否 | 注册序 | 有 | 拦截/策略（请求改写、工具守卫） |
| parallel | 是 | 并发 | 无 | 扇出广播 |
| serial | 是 | 注册序 | 有 | 顺序变换（压缩管道） |

waterfall 即中间件模式：监听器收到 (...args, next)，调用 next() 委托（可包装结果），或不调用直接短路。agent/pre-step、agent/request、llm/stream、tools/* 守卫管道都建立在它之上。事件沿作用域链**向上冒泡**，子作用域监听不到父作用域事件；agent 作用域化注册可实现「只影响某个 agent 的工具集」。

### 2.3 Fiber 状态机与副作用

每个插件实例有一个 fiber：PENDING（依赖未就绪，静默等待）→ LOADING（apply 执行中，收集副作用）→ ACTIVE；失败进 FAILED（依赖变化可恢复）；卸载走 UNLOADING（清理函数按注册逆序执行）→ DISPOSED。

- ctx.effect() 接受单一清理函数 / Promise / 同步、异步生成器四种返回形式；清理函数一次性、可 await。
- 内置 API（ctx.on、ctx.plugin、ctx.provide、ctx.tools.register）本身已是副作用，卸载自动逆转；只有 Cordis 不知道的资源（定时器、watch、WebSocket）才需要手写 effect。
- 服务被替换时，依赖它的插件自动卸载并针对新实现重载——这是热重载与服务替换无需重启的机制。
- 诊断：fiber.getEffects() 返回带标签的副作用树，用于定位泄漏。
- 配置经 Schema 校验失败 → FAILED，插件体不运行。

## 3. 核心 Agent 主轴

六个包构成所有组合必经的循环：

| 包 | 职责 | ctx 键 | 可替换性 |
|---|---|---|---|
| core/scope | 按 agent 作用域的注册原语 | 库 | 基础 |
| core/session | 仅追加 SessionEvent 日志 + 内存存储 | ctx.sessions | 持久化 seam |
| core/system-prompt | 提示词片段与工具 schema 组装 | ctx.systemPrompt | seam |
| core/tools | 作用域工具注册表 + 守卫执行管道 | ctx.tools | seam |
| core/agent | Agent 接口、活跃注册表、agent/* 事件 | ctx.agents | 仅公开契约 |
| core/agent-loop | 默认驱动器 | ctx.agentLoop | **完全可替换** |

agent-loop 之外无人依赖其内部；扩展针对 ctx.agents.get(id) 返回的 Agent 句柄编程。交换接缝（swap seam）设计意味着一个完全不同的循环可以替换默认循环。

### 3.1 轮次/步骤协议

一个**步骤**= 一次模型请求 + 其调用的工具；一个**轮次** = 零或多个步骤。

~~~
turn/start
  认领 next-step 输入 + 一条排队消息
  组装提示词片段 + 工具 schema
  -> agent/pre-step              拒绝 | 进入（可改写消息）
  step/start
  追加 user/message；从日志推导模型历史
  agent/request -> llm/stream -> assistant/chunk* -> assistant/message
  tool/call* -> tools/pre-execute -> tools/execute -> tools/post-execute -> tool/result*
  step/end
  -> agent/turn-stopping（serial，无 next）
turn/end
~~~

事件分两类：**持久会话事件**（turn/*、step/*、user/message、assistant/*、tool/*）追加进日志、重载后保留；**实时扩展点**（agent/pre-step、agent/request、llm/stream、tools/*）是 waterfall。划分是架构性的：**模型看到的任何内容都必须能从日志重建**（“模型可见即已记录”运行时不变量）。

## 4. 会话日志与事件溯源（对 Shannon 最关键的一节）

- Session 是类型化 SessionEvent 的**仅追加日志**，是交互历史的唯一真源；LLM 消息历史从日志**派生**（deriveMessages()），从不单独存储；回放= 同一组事件重新派生。
- seq = log.length 单调连续，**连原始 chunk 都保留**，持久化可以逐字存日志。
- 事件词汇（SessionEventMap，可 declaration merging 扩展）：turn/start、turn/end(reason)、step/start、step/end、user/message（source 区分人类提示 / agent.inject 注入 / 目标续跑）、assistant/chunk（token 级回放保真）、assistant/message（携带本步 usage；中断时以 interrupted: true 定稿前缀）、tool/call（arguments 保持模型原样的未解析 JSON 字符串）、tool/result（含 error 身份与工具私有 meta，append 时运行时校验 JSON 可序列化）、todo/write（全量快照、last-write-wins）、request/header、request/context、session/end-seed（种子/生边界）。
- **request/header（EpochHeader）**：每个请求的信封——调用配置 + 适配器默认值标记 + 渲染后的系统提示词 + 组装后的工具 schema——作为会话状态写入日志，使**每个对话请求都是日志的纯函数**。请求变化时记 reason: change 的完整快照，foldRequestHeader(events) 取最新重建。
- **Surface 模型**：三类产生消息的事件（user/message、assistant/message、tool/result）携带 surfaceOp：append（常规）或 replace{start,end}（压缩时遮蔽区间、原位插入摘要节点），sourceEventSeqs 引用被遮蔽节点——人类 transcript 与模型历史是同一日志的两个投影。
- **ignorable 标记**：未知事件类型缺省为 required（读取方必须拒绝重建而非静默丢弃），纯信息性事件才标 ignorable——「宁可过严拒绝，不可静默吞掉」。
- 持久化 seam（jsonl / sqlite 后端）、崩溃恢复、投影、遥测、标题、fork 全部从事件流派生。

### 4.1 遥测即插件

session-telemetry 协调**脱敏后**的事件转发；session-telemetry-otel 把事件桥接为 OpenTelemetry span/metric（默认禁用）。遥测从正确位置开始订阅而不重放历史；观察者失败被隔离（contained）；作用域过滤保证 agent 级监听只收自己会话的事件。

## 5. 能力接缝（capability seam）

一个 seam = 三种角色：**Service Definition**（声明接口与 ctx 键）、**Service Provider**（实现）、**Consumer**（通常是面向模型的工具）。三者一并设计才构成 seam。

已接缝化的能力：ctx.llm（适配器）、ctx.fs（local / sandbox / e2b）、ctx.subprocess、ctx.sandbox（策略 + local/e2b 后端 + 原生 Landlock）、ctx.shell、ctx.terminals、ctx.subagents（fork-in-process / spawn-in-process / **acp** / **claude-code** / **codex**——把一个轮次委派给另一个竞品）、ctx.agentTeams（实验性：持久 roster、任务板、mailbox）、ctx.commands（人工命令，无需模型轮次）、ctx.jobs（后台工作 + job_* 工具）、ctx.sessionTitle（唯一提供方）、ctx.goals（目标续跑）。

关键收益：**fs 与 subprocess 提供方共享同一执行世界**——把它们指向远程沙箱，Bash、PTY、LSP 一并迁移，无需 fork 提供方。

## 6. 组合系统：bundles + profiles + patch

- **bundle**：Cordis 配置行 + 挂载代码的分发格式（dsh-base / dsh-web-app / dsh-headless），package.json 的 dsh 字段声明。
- **profile**：Harness home 中的具名组装（web / headless 为内置模板），列出堆叠的 bundle、树外插件、cordis.patch.yml。
- 层序：空列表 → 按 profile 顺序应用各 bundle → profile patch → home 级 patch → --patch overlay。patch 按 id 定位条目整体替换或插入新条目——**运行树每一行可替换、可禁用**。
- dsh --profile web --dump-config 打印本机实际启动的配置树。确定性层级优先级，而非临时配置合并。

宿主/客户端通过传输无关的 ctx.apiProxy 网关通信；客户端 HMR 支持不重载整机的插件热更新；客户端构建带 purity gate（禁止跨插件值导入共享闭包——两个动态插件 bundle 不得因共享 workspace 包而共享 Symbol/instanceof/单例）。

## 7. 社区反馈与风险（2026-08 RC 期）

- **概念上手摩擦**：两行命令可装，但 Plan/Agent/Operate/审批姿态四种模式不直观，文档在 Cordis 插件文档而非上手界面；英文文案曾缺失（PR #2512 补）。
- **221 包表面积**：树外插件开发者需理解 pnpm workspace、dsh.profile 清单、cordis.patch.yml 组合序；有人未先源码构建框架就写插件导致构建失败（教程补了前置条件）。
- **健壮性反例**：第三方插件 conversationEvents 定义返回的 key 与声明 kind 不一致，**炸掉整个会话投影**——插件自由度的代价。
- **npm i -g 供应链顾虑**：无原生二进制计划；企业环境对 221 包 JS 依赖树有合理担忧。
- **生态兼容坑**：DeepSeek 思维模型要求多轮回传 reasoning_content，经会剥字段的代理（Copilot Chat/OpenRouter）第二轮即断——代理路径需验证字段保真。
- 社区对 dsh 的三个结构性关切：长程任务执行、记忆、安全治理；Composio 30 个真实 SaaS 工作流基准中 Pi Agent 66.7% 通过、$0.028/成功（最低成本），Claude Code 53.3%——dsh 尚无独立基准数据。
- 独立开发者反馈「独立开发的 harness 与 dsh 在语言、技术栈、扩展框架、Agent 基座上全部撞车」——设计空间收敛的旁证。

## 8. 对 Shannon 的参考/借鉴价值

按价值排序（详细落地见后续文档）：

1. **会话事件溯源 + 「模型可见即已记录」**（→ trace 方案）：Shannon 目前是快照式 session JSON + 四套互不相通的记录；DSH 证明事件日志可以同时充当持久化、回放、遥测、评测的事实源，request/header 快照使「第 N 轮模型看到了什么」可精确回答。这是本次调研对 Shannon 最重要的一条。
2. **capability seam 三角色**（→ 插件架构评审）：Shannon 的 LlmClient adapter 已是无名义 seam；fs/subprocess/sandbox/子 agent 缺统一接缝。DSH 的「共享执行世界」迁移效应（换一个 fs 提供方，bash/PTY/LSP 整体搬家）是 Shannon 桌面沙箱化、远程执行路线的蓝图。
3. **waterfall/serial/emit/parallel 协调语义**（→ 事件总线统一）：Shannon 的权限检查、工具守卫、压缩管道散在不同模块；按协调语义显式分类事件，是统一总线的设计语法。
4. **可逆效应 + fiber 生命周期**（→ Rust 插件改造）：Rust 的 RAII guard 与作用域 Drop 天然适配「注册即可逆」；getEffects() 式副作用树是可借鉴的调试面。
5. **组合配置树 + dump-config**（→ profiles 演进）：Shannon 已有 profiles/agents/routines TOML；「层序 patch + 打印实际生效树」是低成本高杠杆的配置体验升级。
6. **生态冷启动打法**：topic 标签约定 + awesome 列表 + 第三方目录站自然涌现 + dsh plugin add 一键安装。Shannon 若做插件生态可复用此路径（manifest 已兼容 .claude-plugin/plugin.json 是既有优势）。
7. **反面教训**：插件自由度会反噬内核健壮性（key/kind 不一致炸投影）；221 包 JS 依赖树的供应链风险；「一切皆插件」的文档与上手成本。**Shannon 不应照搬激进形态**——Rust 静态编译 + 显式 trait 的语言语义本身就更接近「少而硬的接缝」，取思想弃形态。

## 9. 我的思考与分析

- **DSH 的本质贡献不是插件系统，而是把「不变量」做进了产品**：seq 连续、请求可从日志重建、未知事件缺省拒绝。这些不变量让插件自由度有了安全网。Shannon 做任何插件化之前，应先把等价不变量立起来，否则只是把 if-else 换成注册表。
- **「一切皆插件」的代价被社区反馈清楚标价**：概念上手、文档负担、健壮性事故、供应链面积。DeepSeek 用形式化方法背景的团队 + 论文级文档去对冲（仓库文档「读起来像操作系统论文」）。Shannon 单团队维护，收益曲线不同：**接缝化（seam）优先于插件化（plugin）**——先让每个能力可替换，再谈让第三方替换。
- **dsh 与 Pi 的对照**（详见 Pi 报告）：dsh 用 Cordis 把组合性推到极限，Pi 用最小内核 + 单一扩展 API 把简单性推到极限；两者在 Composio 基准上的表现（Pi 已验证、dsh 未验证）提示：**架构先进性不直接兑换任务成功率**，评测体系（本组文档第 6 篇）才是把架构变成产品力的回路。

## 10. 信源

- 官方架构参考站：deepseek-harness.github.io/deepseek-harness/reference/（架构总览、会话子系统、轮次流程、能力接缝、组合系统各页）
- 仓库文档镜像（zread.ai/deepseek-ai/deepseek-harness：1-overview / 2-quick-start / 5-issues-and-feedbacks / 7-architecture-overview / 8-plugin-lifecycle-and-effects / 10-typed-events / 11-session-log-and-event-sourcing / 17-bundles-and-profiles / 18-configuration-catalog / 22-host-and-client-build-system）
- 生态目录站与 awesome 列表（见 §1）；社区反馈汇总自 GitHub PR / X / linux.do / GitHub Community（经 zread 汇编页转述）
- 注意：生态规模数字各站口径不一（74 精选 / 154 / 306+ / 700+ / 6600+ topic），引用时需注明口径。
