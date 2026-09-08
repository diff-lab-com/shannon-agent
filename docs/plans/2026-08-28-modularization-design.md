# Shannon 模块化设计方案：是否 DSH 化、如何改、生态如何成长

- 日期：2026-08-28 ｜ 性质：架构决策文档（供 ed 拍板） ｜ 主 checkout 只读，零 git 操作
- 输入：[DSH 调研](../research/deepseek-harness-analysis.md) · [插件架构评审](../research/shannon-plugin-architecture-evaluation.md) · [差距分析](../research/shannon-gap-analysis.md) · [master plan v1.4](shannon-improvement-master-plan.md)（§2.2 Non-Goals、§4.8–4.12）+ dev 分支（@aaeedb05）代码勘察 + 2026-08-28 DSH 生态复核
- **结论速览：有条件做。** 不做「全盘 DSH 化」（重申 Non-Goals，理由见 §3/§6）；条件 = ①闭合既有 SEM（统一扩展模型）的三个执行面缺口（§4.2 M1–M3），②P10 core 面积治理按域渐进启动（§4.1），③插件 API 稳定面正式圈定（§4.3），④生态走「发行约定」冷启动而非运行时绑定（§4.4）。一页路线图见 §7。

---

## 1. DSH 的设计理念与 plugin 系统到底是什么

### 1.1 事实基线（2026-08-28 复核，含与 08-26 调研的差异）

[DSH 调研](../research/deepseek-harness-analysis.md)（08-26/27 快照）的架构结论本次复核**全部成立**，无需修正；有变化的是发布节奏与生态信号：

| 维度 | 08-26 快照 | 08-28 复核 | 可信度 |
|---|---|---|---|
| 官方定位 | 开源 agent harness，「一切皆插件」 | 不变——官方页仍以 "Everything is a plugin" 为题，plugins 覆盖 models/tools/skills/sessions/sandboxes/storage/loops/scheduling/UI（[deepseek.com/harness](https://deepseek.com/harness/en/)） | 高（官方一手） |
| 架构内核 | Cordis（Koishi 生态）插件树 | 不变——repo 自述「运行实例 = 启动时按序层叠组合的插件树」，第三方分析确认 vendored Cordis fork、~453K 行插件运行时（[repo](https://github.com/deepseek-ai/deepseek-harness)、[developersdigest 拆解](https://www.developersdigest.tech/blog/deepseek-harness-dsh-first-look)） | 高 |
| 版本状态 | RC 密集发布（rc.2–rc.5），冲刺 0.1.0 | **仍在预发布**：rc.8 于 2026-08-19 发布，0.1.0 稳定版未出；README 保留全大写 "THERE WILL BE COMPATIBILITY-BREAKING CHANGES"（[releases](https://github.com/deepseek-ai/deepseek-harness/releases)、[rc.8 解读](https://news.qiniu.com/archives/1787189242737)、[OfoxAI 稳定性综述](https://ofox.ai/blog/deepseek-harness-dsh-version-updates-stability-production-2026/)） | 高 |
| 插件 API 兼容性 | 未稳定 | **rc.7→rc.8 已发生内部 API 破坏**，官方不保证 RC 间插件向后兼容；社区共识 = 锁版本、升级前审计插件（同上 rc.8 来源 + [MyClaw 实践指南](https://myclaw.ai/blog/deepseek-harness)） | 高 |
| 生态规模 | 74 精选～6600+ topic，口径不一 | 进一步膨胀且**口径更乱**：awesome 列表多版本（[walkinglabs](https://github.com/walkinglabs/awesome-deepseek-harness-plugins)/[vvlife](https://github.com/vvlife/awesome-deepseek-harness-plugins)/[0xsline](https://github.com/0xsline/awesome-deepseek-harness)）、第三方目录站成型（[dshpluginstore.com](https://dshpluginstore.com/blog/everything-is-a-plugin-architecture)、[dsh.deepseek404.com](https://dsh.deepseek404.com)）；自媒体声称「1800+ 插件」「2 天 95K star」 | **低**（数字全部按口径存疑，只作方向性参考：目录站自然涌现为真，具体规模不可引用） |
| 新增信号 | — | rc.8 把 **Claude Code 与 Codex 作为子代理**接入（[七牛云 rc.8 解读](https://news.qiniu.com/archives/1787189242737)）——DSH 在向其他 harness 伸手，竞争是相互的融合而非替代 | 中 |

### 1.2 设计理念的准确表述（核校后）

DSH 的 plugin 系统 = **五个机制**叠加在一个**同语言、可运行时加载代码**的宿主上：

1. **插件 = 服务 + 效应**：插件是带生命周期挂载的服务对象；一切注册（提示词片段、工具 schema、适配器、监听器）经 `ctx.effect()` 安装、卸载时自动逆撤销（调研 §2.1，官方 [reference 站](https://deepseek-harness.github.io/deepseek-harness/reference/)）。
2. **上下文 = 服务仓库**：`ctx.<key>` 声明稳定服务面，消费者按键发现、从不 import 具体实现；inject 依赖激活决定加载序（同上）。
3. **类型化事件四模式**：emit / waterfall / parallel / serial 四种协调语义，waterfall 即中间件守卫链，事件沿作用域链冒泡（调研 §2.2）。
4. **不变量先行**：会话是 append-only SessionEvent 日志、seq 连续、「模型可见即已记录」、未知事件缺省拒绝（调研 §4）——这是 DSH 真正的护城河，比插件形态更重要（调研 §9）。
5. **组合系统**：bundle/profile/patch 层序 + `--dump-config` 打印实际生效树（调研 §6）。

**关键判断（本方案立论）**：DSH 形态成立的前提是 Node/TS 的运行时代码加载；其社区反馈已把代价标价清楚——概念上手摩擦、221 包供应链面积、第三方插件 key/kind 不一致炸掉会话投影的事故（调研 §7）。**Shannon 该取的是五机制背后的不变量与接缝，不是插件树运行时本身。** 这一结论与[插件架构评审](../research/shannon-plugin-architecture-evaluation.md) §2/§3 一致，且被 08-28 新证据（RC 间持续破坏插件 API）进一步强化。

---

## 2. Shannon 现状：五套扩展机制的真实边界，以及「已经 DSH 化」的部分

### 2.1 五套机制的边界与重叠（v1.4 落地后的现状）

master plan v1.4 的 15 项任务已全部合入 dev（W1 trace / W2 eval / W3 extension / W0 权限 / 4.14 OTLP+脱敏+Timeline / 4.15 信号看板，见 dev 提交序列 `240b806e`、`bd0e94f7`、`ad83eec0`、`9b5009f4`、`23814f29`）。五套机制的边界如今是：

| 机制 | 位置 | 进程模型 | 职责 | 与其他机制的重叠点 |
|---|---|---|---|---|
| MCP | `crates/shannon-mcp/`（+ `shannon-mcp-saas/` SaaS 服务器族） | 进程外（stdio/HTTP JSON-RPC） | 第三方工具生态；`tools/list` 自动发现、deferred schema | 工具进入同一条 host 工具管道（`mcp_tool_adapter.rs`），权限走同一守卫链 |
| plugin manifest | `crates/shannon-core/src/plugin/`（manifest.rs / validate.rs / permissions.rs / registry.rs / index*.rs / installer.rs） | 分发层（无运行时） | 安装/发现/校验；三方言读取矩阵（v1 TOML / v2 TOML / `.claude-plugin/plugin.json`）；远端 index（`registry_url`，config.rs:12） | v2 的 `[[mcp]]` 行把 MCP server 变成插件载荷；`[[hooks]]` 行声明 hook 订阅（见 §3 缺口） |
| hooks | `crates/shannon-engine/src/hooks/events.rs:9`（**30 个** HookEventType 变体，CLAUDE.md 的「32」为历史口径漂移） | 进程外一次性命令 + 进程内总线订阅 | 生命周期扩展点；routines 自动触发 | **已总线化**：触发经总线 routing-only custom 事件（`NS_HOOK_TRIGGER`，bus.rs:69），PreToolUse 是工具守卫瀑布第二节点（guard_nodes.rs:32–35） |
| skills | `crates/shannon-skills/`（SkillRegistry，列入 [STABILITY.md](../../docs/STABILITY.md) 稳定面） | 进程内声明式 | 命令模板（提示词注入） | plugin 的 skill 型即其分发外壳 |
| profiles/agents/routines | `.shannon/`+`.claude/` 声明式配置（CustomProfileRegistry 等） | 进程内声明式 | 权限预设、agent 定义、触发/定时例程 | routines 是 hooks 的配置化消费方 |

**重叠的真实规模**：五套机制如今共享**一个**判定与记录内核——所有机制的工具调用都过同一条权限守卫瀑布，所有决策/触发都落同一份 L0 日志。剩余的「各自为政」只剩安装/发现层（manifest 管 plugin、skills/profiles 走目录扫描），这一层不统一是可接受的（分发语义本就不同），**不是** DSH 意义上的结构性缺陷。

### 2.2 已经 DSH 化的部分（逐条对 DSH 五机制，附证据）

| DSH 机制 | Shannon 对应物 | 状态 |
|---|---|---|
| 四模式类型化事件 | `crates/shannon-core/src/bus.rs`（1022 行）：`DispatchMode::{Emit,Serial,Parallel,Waterfall}`（bus.rs:449–468），waterfall = next() 链守卫管道（bus.rs:17、280–317），词汇直接复用冻结的 L0 `SessionEventKind`，「本模块不发明新 kind」（bus.rs:5–8） | ✅ 已达成 |
| 可逆效应（RAII） | 每个注册返回 guard，Drop 即注销（bus.rs:35–40、402–448）；比 JS 清理函数更强（编译器保证） | ✅ 已达成 |
| 事件词汇治理 | 封闭核心 enum + 命名空间 custom payload（`shannon.internal.` 前缀永不持久化，bus.rs:71–78）——精确实现了评审文档 §6.1 第 4 条「封闭核心词汇 + 开放 payload 通道」，规避 DSH 的 key/kind 事故 | ✅ 已达成 |
| 守卫管道（waterfall 实战） | 工具预执行两段瀑布：权限门第一节点（`PIPELINE_PERMISSION`，guard_nodes.rs:30）→ PreToolUse hooks 第二节点（guard_nodes.rs:32–35）；每个判定落 `permission/decision` / `hook/fired` 持久行 | ✅ 已达成 |
| 会话事件溯源 +「模型可见即已记录」 | `session_log/`（writer/reader/tee/l0_subscriber/projections/redaction）：request/header 快照、seq 连续、**未知 kind 缺省报错（required-by-default）**、显式 opt-in 才跳过（reader.rs:23、32、168）；词汇 18 kind 冻结于 `shannon-types/src/session_event.rs:50` | ✅ 已达成（这正是调研 §9 说的「先立不变量」） |
| capability seam 三接缝 | `shannon-tool-interface/src/providers.rs`：`FileSystemProvider`（providers.rs:72，同步+异步双面、明确不做无消费者的 watch——YAGNI 注记 providers.rs:13–18）、`ProcessProvider`（providers.rs:251）+ `SpawnRewrite` 链（providers.rs:219–243）+ `prepare_spawn` 沙箱包装点（providers.rs:266）；`SandboxProvider`/`SandboxPolicy`/`ForkInitHost`（sandbox.rs:334/132/321） | ✅ 已达成 |
| 执行世界整体迁移 | `LocalFs`/`LocalProcess`（shannon-core/src/providers.rs）为缺省实现；`SandboxedFs`/`SandboxedProcess` 装饰器 + Landlock 内核后端（shannon-tools/src/sandbox/mod.rs:50/77/189/197、landlock_backend.rs）；shannon-tools 全部工具经 provider 注入（grep.rs、write.rs、lsp.rs、notebook.rs、config.rs、cron.rs、image_analysis.rs），注入证明测试 `tests/provider_seam_injection.rs`——「换 provider、工具零改码」的 DSH 核心收益已落地 | ✅ 已达成 |
| 遥测即日志订阅者 | L0 写者是总线内置订阅者（Serial 模式保证落盘序=广播序，bus.rs:15）；OTLP 桥为 L0 投影（提交 `9b5009f4`） | ✅ 已达成 |
| 组合系统 + dump-config | `shannon --dump-config` 六层合并序（builtin→user-global→project→env→connected→cli-overlay），每项带 `overridden_by` 来源标注（config_dump.rs:1–22）——对齐 DSH `--dump-config` 的可解释性 | ✅ 已达成（patch 层序无，见 §3） |
| 插件分发/发现 | git/本地/归档安装 + 远端 index + GitHub topic `shannon-plugin` 约定 + 三方言读取矩阵 + 安装期 schema/权限完整性校验（validate.rs）+ `[compat]` 版本窗口（manifest.rs:96、428） | ✅ 基础达成（缺种子资产，§4.4） |
| API 稳定性治理 | `shannon-stability-attr` crate（`#[stable_api]`/`#[unstable_api]` proc-macro）+ [STABILITY.md](../../docs/STABILITY.md) 三层稳定性承诺 + cargo-semver-checks 阻断 CI + `tests/architecture_invariants.rs`（crate 依赖方向、stable API 文档、dead_code 纪律的结构性测试） | ✅ 机制已达成（稳定面清单需扩容，§4.3） |
| 权限闭环 | `plugin/permissions.rs`：六字段中 5 个在 Shannon 侧执行点真实强制（execute_commands/network/mcp_tools/read_files/llm_api），空声明=维持宽松现状，拒绝统一走 `PluginPermissionError` + permission/decision 入 L0（permissions.rs:1–30） | ⚠️ 5/6（`write_files` 未闭合，§3） |

---

## 3. 判断：全盘 DSH 化的边际价值——三桶清单

判定方法：把 DSH 的每个能力面对照 Shannon 现状，按「已达成 / 差距小不值得 / 差距大（建议或明确不建议）」归桶。**差距大 ≠ 建议做**；是否建议看边际价值与 Non-Goals 纪律（master plan §2.2）。

### 3.1 已达成（不需要再动）

§2.2 表中全部 ✅ 项。特别强调三点：Shannon 的**事件不变量**（seq 连续、请求可重建、未知拒绝）已按 DSH 教训先于扩展点开放而确立；**三接缝 + Landlock** 已让「换执行世界、工具零改码」从蓝图变成装配测试覆盖的事实；**总线化 hooks** 让 30 个事件与 L0、权限共用一套词汇与一次分发。

### 3.2 差距小，不值得做（做了为改造而改造）

| DSH 能力 | Shannon 缺口 | 不做的理由 |
|---|---|---|
| 泛化服务仓库（ctx.* 键空间）+ inject 依赖激活 | 无 ServiceRegistry/PluginContext（全仓 grep 为零）；装配是编译期 `Arc<dyn Provider>` 注入 | 编译期 DI 已被 Landlock 装饰器实战证明可换实现；泛化 registry 是解决「运行时才知道依赖谁」的问题，Shannon 启动序是确定的。providers.rs 文档注释「deliberately no watch method…YAGNI」就是本仓的抽象纪律 |
| bundle/patch 层序替换 | 只有 profile 层序 + dump-config，无逐条 patch | patch 解决「不改发行版改组合树」，前提是插件树运行时；Shannon 组合面小（六层配置+profiles），`--dump-config` 已给可解释性。等出现真实组合冲突需求再议 |
| 热更新/HMR | 无 | 评审文档 §2 已判定：CLI 生命周期短、desktop 可重启，无刚性需求 |
| 持久化 seam（jsonl/sqlite 可换后端） | jsonl 固定（session_store.rs） | DP4 已拍板直切单一格式；多后端是 YAGNI |
| surface/replace 全语义 | 词汇已冻结（`surface/append`/`surface/replace`，session_event.rs），投影层按需消费 | 词汇先行、消费滞后是正确次序；无当前消费需求 |
| dsh 行为型插件运行时兼容 | 无（正确） | 等于在 Rust 里重实现 Cordis 宿主（评审 §4 路径 1，已否决） |

### 3.3 差距大——其中三个建议做（§4/§5 的 M 任务），两个明确不建议做

**建议做（小而确定，收益/风险比高）：**

1. **`write_files` 权限闭合**：permissions.rs:19 自述「enforcement stays OFF — hook point ready, scaffolding only」。接缝（FileSystemProvider）与 guard 脚手架（`WriteFilesPolicyGuard`）都在，差一次「翻开关 + 回归矩阵」。这是 P7 的最后一块。
2. **manifest v2 `[[hooks]]` 行从「校验过的保留位」变「真执行」**：validate.rs:121 现状是「reserved schema slot, but validate now so typo'd…」，ECOSYSTEM.md 模板自注 handler「reserved (not yet executed)」。声明了订阅却静默不执行，是当前五套机制间**唯一一处语义欺瞒**（用户以为声明生效了）。
3. **插件 MCP server 生命周期治理**：`mcp_tool_adapter.rs:379–389` 注释自认「cold-spawns on every invocation」——discovery 与每次工具调用都冷 spawn 进程。既有 `McpProcessPool`（shannon-mcp，已列 STABILITY.md 稳定面）就是现成的池化方案。注释同时确认权限门「不假设 spawn 次数」，池化不破坏权限语义。

**明确不建议做（差距大但边际价值为负，见 §6 展开反方论证）：**

4. **插件树运行时 / 「一切皆插件」内核重构**：把 agent loop、会话投影等内核面做成可替换插件。无需求信号；DSH 自身的健壮性事故与概念成本是反例；Shannon 的 agent loop/权限内核是产品差异本体，不该变成可被第三方替换的面。
5. **进程内 WASM 提前重启**：DP3 已暂缓且重启条件明确（W3 完成 + wasmtime 过 cargo-deny）。当前 W3 尚有三缺口（上条 1–3），条件未满足，不重启。

---

## 4. 方案

### 4.1 模块边界：P10 core 面积治理的渐进切分

**现状**：`crates/shannon-core/src/` 单目录 **85 个条目**（约 70 个 .rs + 15 个子目录），其中相当部分是低耦合生活质量功能（billing、voice_mode、magic_docs、tips、enhanced_suggestions、prevent_sleep、away_summary、auto_test…）。[STABILITY.md](../../docs/STABILITY.md) 的措辞「lockstep until the **D1 crate split** completes」说明拆分早已是既定方向，缺的是切分图与次序。

**三条规则**（先立规则再动刀，避免拆出 221 包）：

1. **依赖方向单向**：被拆出的 crate 只准依赖 `shannon-types` / `shannon-tool-interface`（以及总线），`shannon-core` 不得反向依赖任何被拆出 crate——由 `tests/architecture_invariants.rs` 的 `metadata_dependency_separation`（architecture_invariants.rs:74）扩展一条断言来机械守护。
2. **一 minor 一批**：按既有 semver 纪律（版本联动改 3 处：workspace.package + 内部 dep 要求 + desktop 硬编码；cargo-semver-checks 对 pinned baseline 阻断），每个 minor 最多拆一个域，拆完跑全绿再拆下一个。
3. **新能力默认进扩展位**：新功能先进 feature crate / MCP / hook routine，不再进 shannon-core/src 平铺（master plan §2.1 P10 改进方向的延续）；先例是 `shannon-repomap`、`shannon-mcp-saas` 两个域 crate。

**切分图（按耦合度从低到高分四批；每批独立可发布，可随时停在任意批）**：

```
shannon-core/src (85 条目)
│
├─ 批 1 · QoL/外围 → shannon-extras（或逐个评估直接删除）
│   voice_mode · tips · magic_docs · enhanced_suggestions · suggestions
│   away_summary · auto_test · prevent_sleep · updater · oauth
│   （Pi 纪律对照：这些是否都该存在本身存疑——先审后拆，能删不拆）
│
├─ 批 2 · 调度域 → shannon-scheduler
│   scheduled_budget · scheduled_retry · scheduled_routines · scheduled_runs
│   scheduled_task_store · scheduled_worktree · housekeeping
│   （7+1 个同族文件，入边集中，是最干净的一批）
│
├─ 批 3 · 记忆域 → shannon-memory（现有 memory/ 子目录升格 + 同族收拢）
│   memory/ · preference_memory · project_memory · team_memory_sync
│   extract_memories · auto_dream_consolidation
│
├─ 批 4 · 供应商域 → shannon-provider-kit（评估后可保持原地不动——耦合最高，允许不拆）
│   provider_config_service · provider_config_store · provider_resolver
│   credential_manager · rate_limit · model_registry/
│
└─ 留守 core（不可再瘦的内核）
    lib · error · bus · session_log/ · query_engine/ · plugin/ · tools*
    providers · sandbox · mcp_tool_adapter · mcp_advanced · telemetry
    settings*/config_* · unified_config · testing/ · ui_adapter
```

预期效果：core 从 85 条目降到约 55–60（批 1–3 执行后），每个拆出 crate 带着自己的 `#[cfg(test)]` 走，`just test` 并行度反而提升。**不做大爆炸迁移**：任何一批发现入边比预期多，允许整批退回（wrapper 起步，同评审文档 §7 的总线策略）。

### 4.2 插件 API 稳定面：哪些 trait 是公共 API

原则：**稳定面 = 第三方与跨产品（desktop/gateway）允许依赖的最小集合**；其余默认 unstable（STABILITY.md 现行规则）。建议按三层圈定并在下一 minor 用 `#[stable_api(since=…)]` 落标：

| 层 | 纳入 stable 的面 | 理由 | 现状 |
|---|---|---|---|
| 工具契约 | `shannon-tool-interface::{Tool, ToolOutput, ToolInfo, ProgressSender, ToolError}`（lib.rs:42–172） | 任何工具作者（含未来 WASM guest 的 WIT 投影）依赖的第一面 | 未标 tier |
| 执行世界契约 | `FileSystemProvider`、`ProcessProvider`、`SpawnRewrite`、`PipedChild`（providers.rs:72/251/219/199）、`SandboxProvider`/`SandboxPolicy`（sandbox.rs:334/132） | 「两种实现可互换」已被 Local↔Landlock 证明（sandbox_matrix.rs），具备冻结资格；M1 闭合后一起标 | 未标 tier |
| 总线契约 | `EventBus::{subscribe, subscribe_fn, dispatch, guard_pipeline}`、`DispatchMode`、`BusSubscriber`、`GuardNode`、`RegistrationGuard`/`NodeGuard`（bus.rs:248–636）+ **custom payload 命名空间规则**（`shannon.internal.` 前缀语义） | 进程内扩展的唯一入口；词汇已冻结，总线 API 可随词汇一起承诺 | 未标 tier |
| 清单契约 | `plugin::manifest` 三方言解析 + `PluginPermission` 六字段语义（PERMISSIONS.md 为准）+ `[compat]` 窗口语义（manifest.rs:428） | 分发层兼容承诺；dsh/claude 桥接（§4.4）也建立在这上面 | 部分（ECOSYSTEM.md 文档化，代码未标） |
| 已 stable（维持） | `QueryEngine/QueryEvent/QueryContext`、`LlmClient` 族、`StateManager`、`SkillRegistry`、`McpProcessPool`、`shannon-types::events` | STABILITY.md 既有清单 | ✅ |

**不进稳定面**：`query_engine::guard_nodes`（内置节点可自由重排）、`session_log::projections`（投影词汇演进中）、bus 内部锁/registry 结构、`PluginDecisionFrame`（bus.rs:707，诊断用）。

配套动作：`docs/STABILITY.md` 增补「扩展 API 稳定面」一节，并在 cargo-semver-checks 的 baseline 升到当前 release tag 后生效（breaking 需 minor bump 的既有纪律自动适用于新稳定面）。

### 4.3 进程模型：维持 hybrid 现状，不引入第三态

结论：**进程内 = 编译期内置（守卫节点、hooks 适配器）；进程外 = MCP（第三方默认）；WASM 维持暂缓。**

- DSH 的四形态对照（评审 §5 表）今日依然成立，且 08-28 新证据（rc.7→rc.8 破坏插件 API）说明**连 DSH 自己都还没冻结进程内 ABI**——Shannon 没有理由在此刻替它提前下注。
- MCP 补强清单（从评审 §5 机会点中保留一项、降级两项）：
  - **保留（条件触发）**：MCP 侧 hook 订阅协议——让进程外 server 订阅生命周期事件。**仅当生态出现真实需求**（有第三方表示要在进程外做守卫/遥测）再立项（M6），现在做是为想象中的用户写协议。
  - **降级**：server 健康度/重启策略——M3 池化（§3.3）顺带覆盖大半，不单独立项。
- WASM：维持 DP3。重启条件不变（W3 三缺口闭合 + cargo-deny 预检）；manifest 的 `type="wasm"` 保留位继续作为「schema 已知、本构建拒绝加载」的显式报错（manifest.rs:474–475），这个姿态是对的。

### 4.4 生态冷启动：发行约定，不是运行时

**DSH 生态增长的解剖**（口径存疑但机制可信）：官方叙事（一切皆插件）→ 一键安装（`dsh plugin add <npm 包>`）→ topic 标签 → awesome 列表 → 第三方目录站在数周内自然涌现。**驱动力是 npm 零摩擦分发 + 官方叙事，不是 Cordis 运行时本身。**

Shannon 的对应打法（低成本四件套，M5）：

1. **三个模板仓**（skill / command / tool 各一，内容直接取自 ECOSYSTEM.md §2 的 v2 模板，配 CI = `shannon /plugin install` 冒烟）——把「写第一个插件」的时间降到 30 分钟内。
2. **awesome-shannon-plugins 种子列表** + topic `shannon-plugin`（ECOSYSTEM.md §1 已约定，缺的是第一个被收录的例子）。
3. **静态 index 上线**：`plugin/index_builder.rs` 已有 index 构建，`registry_url` 有默认值（config.rs:30）——把 index 发布为一个静态 JSON（GitHub Pages 即可），`/plugin install` 走通「搜→装→用」闭环。
4. **两条借力通道**（Shannon 相对 dsh 的不对称优势，优先级高于 dsh 桥接）：
   - **Claude 生态兼容是现成的冷启动杠杆**：`.claude-plugin/plugin.json` 读取、`.claude/` skills/agents/profiles 目录已支持（manifest.rs:709 起 JSON 方言测试）——存量 Claude Code 插件/skill 零移植即潜在 Shannon 插件。把这个事实写进 README/ECOSYSTEM 首屏，比任何 dsh 转换器都值钱。
   - **MCP 是跨 harness 通用语**：任何 MCP server 天然是 Shannon 插件；把 Shannon 收录进 MCP registry/目录站生态的成本≈0。
5. **dsh 桥接纪律（重申+更新）**：只做声明型 +「MCP 包装型」的 manifest 级映射（评审 §4 路径 2+3）；**在 dsh 0.1.0 稳定版发布前不绑定其任何版本化行为**（rc.8 仍在破坏内部 API）。dsh 侧 Claude Code/Codex 子代理化（§1.1）说明它也在向兼容生态靠拢——观察即可，不必追赶。

**预期管理**：dsh 的生态数字（1800+ 插件等）口径不可信（§1.1）；Shannon 单团队维护，生态是「服务自己」的副产品（评审 §7）——M5 的验收是「三个模板能装能跑 + index 可搜索」，不是「N 个第三方插件」。

---

## 5. 迁移路线（每步独立可发布，标工作量与风险）

| # | 任务 | 内容 | 规模 | 关键风险与对策 | 验收 |
|---|---|---|---|---|---|
| M1 | `write_files` 权限闭合 | 启用 `WriteFilesPolicyGuard` 于 FileSystemProvider seam；语义沿用「非空声明才收紧」 | S（1–2 天） | 误伤现有插件 → 缺省宽松不变；§4.5/§4.9 矩阵用例转常驻回归 | 越权写被拒、未声明插件行为逐字节不变（permissions.rs 既有测试族扩展） |
| M2 | `[[hooks]]` 行真执行 | 插件加载时把 v2 `[[hooks]]` 行注册进 HookManager（词汇已在安装期校验，validate.rs:121）；文档同步去掉「reserved」注记 | M（2–3 天） | handler 路径逃逸 → 复用既有 hooks 的路径解析与审批；加载失败显式报错 | 声明即触发 e2e；hook/fired 行带插件来源 |
| M3 | 插件 server 池化 | 复用 `McpProcessPool` 模式承接 plugin stdio server；健康度+重启策略顺带落地 | M（3–5 天） | 权限门假设 spawn 次数 → 代码注释已确认「不假设」（mcp_tool_adapter.rs:379–389），补一条池化下的权限回归即可 | 同插件二次调用不再冷 spawn（延迟对照表）；权限用例全绿 |
| M4 | P10 批 1（QoL 域） | §4.1 批 1：先审后拆（能删不拆），拆出者进 `shannon-extras`；architecture_invariants.rs 加依赖方向断言 | M | 入边超预期 → 整批退回或 wrapper 起步 | `just dev` 全绿；semver-checks 过（minor bump 3 处联动） |
| M5 | 生态种子 | 三模板仓 + awesome 种子 + 静态 index 发布 + Claude 兼容首屏宣传 | S–M | 无人来 → 验收锚定自服务（模板可装可跑），不锚定第三方数 | 端到端：搜→装→用一条命令跑通（§4.10 原验收延续） |
| M6（条件） | MCP hook 订阅协议 | 进程外 server 订阅生命周期事件（评审 §5 机会点） | L | 为想象用户写协议 → **仅当出现真实第三方需求才启动** | 有需求时再定义 |
| M7（暂缓） | WASM 试点 | 维持 DP3；重启条件：M1–M3 完成 + wasmtime 过 cargo-deny | — | R11 不变 | 4.16 保留细则 |

**次序约束**：M1→M2→M3 是 W3 收口链（M1 最先，权限语义是 M2 的前置共识）；M4/M5 与 M1–M3 无依赖可并行；M6/M7 不排期。每步落一个 minor，符合 R6 的版本联动流程。

---

## 6. 反方论证：不该做的清单及理由

1. **不该做「一切皆插件」内核重构**（插件树运行时、ctx 服务仓库、inject 激活、可替换 agent loop）。
   理由：① 语言语义——Rust 静态编译 + 显式 trait 本就是「少而硬的接缝」，运行时代码加载要么上 dylib（ABI 地狱，评审 §3 已否）要么嵌 JS 引擎（把 dsh 的供应链面积拖进 Shannon 信任边界）；② 反例在 DSH 自己身上——概念上手摩擦、221 包面积、第三方插件炸投影、RC 间破坏 API（§1.1/§3.3）；③ 无需求信号——Shannon 的三接缝已被 Landlock 实战证明「可换实现」无需运行时形态；④ 机会成本——评测面（W2）刚闭环，把架构先进性兑换成任务成功率的回路（eval → 改进）才是当前杠杆（Composio 数据：Pi 最小内核 66.7% 通过 > Claude Code 53.3%，架构先进性不直接兑换成功率，调研 §9）。
2. **不该现在绑定 dsh 生态**（宿主 shim、行为型兼容、跟随其 manifest 语义演进）。
   理由：0.1.0 未发、RC 间持续破坏（§1.1）；绑定一个未冻结的外部 ABI 等于把自己的 minor 节奏交给别人。只做无版本耦合的声明型映射。
3. **不该泛化抽象先行**（提前建 ServiceRegistry/PluginContext、提前把 `watch` 加进 FileSystemProvider、提前立 MCP hook 订阅协议）。
   理由：违反本仓已验证的「两种实现可互换才抽象」纪律（providers.rs:13–18 的 YAGNI 注记、评审 §7 R10）；每个抽象等第二个真实实现出现再立，M6 的条件触发就是这条纪律的应用。
4. **不该为「生态繁荣」松动权限/安全姿态**。
   理由：权限体系是 Shannon 对 Pi/DSH 的差异化（gap analysis §3.1；Pi 干脆没有权限系统）；dsh 生态事故（插件炸投影）恰恰说明「先有安全网再有自由度」。M1–M2 的语义都是「声明才收紧、缺省宽松」，不引入易用性回退。
5. **不该把 P10 拆分做成一次性大迁移**。
   理由：单团队维护 + lockstep 版本（3 处联动）使大爆炸的合并冲突面不可控；「一 minor 一批 + 允许整批退回」把风险切成可回滚单元；dead_code ~96 处的现状也提示部分模块（批 1）的正确归宿是删除而非搬运。
6. **不该追 dsh 的形态性新功能**（HMR、bundle/patch、作用域树冒泡等）。
   理由：全部在 §3.2「差距小不值得」桶里——它们解决的是插件树运行时的自有问题，Shannon 没有这些问题的载体。

---

## 7. 结论与一页路线图

**结论：有条件做。** Shannon 不需要「改造成」类 DSH 系统——v1.4 落地后它已经以 Rust 自己的方式拥有了 DSH 值得抄的全部实质（四模式总线 + RAII 可逆注册、冻结事件词汇 + 未知拒绝、三能力接缝 + Landlock 执行世界、manifest v2 + compat 窗口 + 安装期校验、dump-config、L0「模型可见即已记录」、稳定面治理 + semver 阻断）。剩下的不是换形态，而是**把已建系统的三个未闭合点闭掉、把 core 面积治理启动、把生态用发行约定冷启动**。同时以 Non-Goals 纪律明确拒绝插件树运行时、运行时脚本加载、dylib、dsh 运行时兼容、提前 WASM 五件事。

```
┌─ W3 收口链（一 minor 内完成，M1→M2→M3）──────────────────────┐
│ M1 write_files 权限闭合 (S)  → P7 六字段全生效                │
│ M2 [[hooks]] 声明真执行 (M)  → 消除唯一语义欺瞒点             │
│ M3 插件 server 池化 (M)      → 冷 spawn 消除 + 健康度        │
├─ 并行带（无依赖，各自一个 minor）────────────────────────────┤
│ M4 P10 批1 QoL 域拆分 (M)    → 批2 调度域 → 批3 记忆域…        │
│ M5 生态种子 (S–M)            → 模板仓×3 + awesome + 静态 index│
│                                + Claude/MCP 兼容首屏          │
├─ 稳定面（随首个含 M1–M3 的 minor 落标）──────────────────────┤
│ #[stable_api] 圈定：Tool 族 / 三 Provider / 总线契约 /        │
│ manifest 契约；STABILITY.md 增补「扩展 API 稳定面」节          │
├─ 条件与暂缓（不排期）────────────────────────────────────────┤
│ M6 MCP hook 订阅协议：仅在真实第三方需求出现时启动             │
│ M7 WASM：维持 DP3，重启条件不变（W3 收口 + cargo-deny 预检）   │
└──────────────────────────────────────────────────────────────┘
不做：插件树运行时 · 服务仓库泛抽象 · HMR/patch 层 · dsh 运行时
兼容/版本绑定 · dylib · P10 大爆炸迁移 · 为生态松动权限姿态
```

**成功判据**（对应验收）：六字段权限矩阵全「生效」；manifest 声明的 hook 端到端触发；同插件二次调用零冷 spawn；core 条目数 ≤60 且依赖方向有测试守护；三个模板插件一条命令装跑；上述全部在 cargo-semver-checks 与 `just dev` 全绿下落地。

---

## 附录 · 信源

**外部（2026-08-28 访问）**
- 官方：[deepseek-ai/deepseek-harness](https://github.com/deepseek-ai/deepseek-harness) · [官方预览页](https://deepseek.com/harness/en/) · [architecture.md](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/architecture.md) · [releases](https://github.com/deepseek-ai/deepseek-harness/releases) · [架构 reference 站](https://deepseek-harness.github.io/deepseek-harness/reference/)
- 版本/稳定性：[OfoxAI 稳定性综述](https://ofox.ai/blog/deepseek-harness-dsh-version-updates-stability-production-2026/) · [七牛云 rc.8 解读](https://news.qiniu.com/archives/1787189242737) · [MyClaw 实践指南](https://myclaw.ai/blog/deepseek-harness)
- 生态（规模口径存疑，仅机制参考）：[walkinglabs awesome](https://github.com/walkinglabs/awesome-deepseek-harness-plugins) · [vvlife awesome](https://github.com/vvlife/awesome-deepseek-harness-plugins) · [0xsline awesome](https://github.com/0xsline/awesome-deepseek-harness) · [dshpluginstore](https://dshpluginstore.com/blog/everything-is-a-plugin-architecture) · [dsh.deepseek404.com](https://dsh.deepseek404.com) · [developersdigest 拆解](https://www.developersdigest.tech/blog/deepseek-harness-dsh-first-look)

**内部（dev @ aaeedb05，2026-08-28 勘察）**
- 总线/守卫：`crates/shannon-core/src/bus.rs`（:5–8 词汇冻结、:15 Serial、:35–40 RAII、:69–78 命名空间、:449–468 四模式、:707）、`crates/shannon-core/src/query_engine/guard_nodes.rs`（:30–35 两段瀑布）
- 接缝：`crates/shannon-tool-interface/src/providers.rs`（:72/:251/:219/:266）、`…/sandbox.rs`（:132/:321/:334）、`crates/shannon-core/src/providers.rs`（Local 实现）、`crates/shannon-tools/src/sandbox/mod.rs`（:50/:77/:189/:197）、`…/landlock_backend.rs`、`crates/shannon-tools/tests/provider_seam_injection.rs`、`…/tests/sandbox_matrix.rs`
- 插件：`crates/shannon-core/src/plugin/manifest.rs`（:96 compat、:428 窗口、:474–475 wasm 保留、:709 JSON 方言）、`…/validate.rs`（:116/:121 保留位校验）、`…/permissions.rs`（:1–30 强制矩阵、:19 write_files OFF）、`…/config.rs`（:12/:30 registry_url）、`…/ECOSYSTEM.md`、`…/PERMISSIONS.md`
- L0：`crates/shannon-types/src/session_event.rs`（:50 起 18 kind）、`crates/shannon-core/src/session_log/reader.rs`（:23/:32/:168 未知拒绝）
- 治理：`crates/shannon-stability-attr/src/lib.rs`、`docs/STABILITY.md`、`crates/shannon-core/tests/architecture_invariants.rs`（:74 依赖分离）
- 配置：`crates/shannon-core/src/config_dump.rs`（:1–22 六层 provenance）
- 其他：`crates/shannon-engine/src/hooks/events.rs`（:9 起 30 变体）、`crates/shannon-core/src/mcp_tool_adapter.rs`（:379–389 冷 spawn 注记）
