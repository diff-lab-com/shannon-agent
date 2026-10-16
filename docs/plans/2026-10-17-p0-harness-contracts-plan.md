# P0 三项实施计划:工具执行管线契约化 · 事件派发语义表 · 生效装配树 dump(2026-10-17)

> **Track**: 报告 v2 §5 P0 三项的代码落地(docs-only → 代码)
> **Date**: 2026-10-17
> **Status**: Approved(ericdong 2026-10-17 指令:由 agent team 实施,完成后 PR 合并至 dev;计划不设单独评审门)
> **输入**: [报告 v2 §5](../../reviews/2026-10-15-deepseek-harness-modularity-analysis.md) P0-1/P0-2/P0-3 行(含验收判据);代码接地调研 2026-10-17(本计划全部 file:line 在 dev 基线逐条核实)
> **分支/PR**: `feat/p0-harness-contracts` → `dev`(基线 1c45a4756)

---

## Global Constraints(全任务约束,评审以此为准)

1. **行为兼容**:不改变既有默认行为——权限判定语义、hook 判定结果、registry 超时 300s 与 `ToolError::Timeout` 类型、输出上限 40k 字符,全部不变。
2. **新增能力默认保守**:工具级重试仅限 `is_read_only==true` 且结果为 `Err(ToolError::Timeout)`,最多 1 次,无退避;其余一律不重试。
3. **write-intent 门明确排除**(报告 §5 P0-1:待裁决子项,本计划不含)。
4. **不改 `Tool` trait 公共 API**;不动 `shannon-api-protocol` 冻结类型(除非 CI generated-facts 检查强制,若发生必须在实施报告标注)。
5. **每任务完成门**:`cargo fmt --check` + `cargo clippy -p <touched>... --all-targets -- -D warnings` + touched crates 的 `cargo nextest run` 全绿;每任务恰好 1 个 commit,信息 `feat(scope): ...`。
6. **不跑全仓 12,585 测试**,只跑 touched crates 与相关 integration tests;不派生子代理。
7. `PIPELINE_*` 常量(guard_nodes.rs:31,33)在本计划中被正式消费为节点名,不再是死常量。
8. 注释风格与周边代码一致;测试放既有模式位置(`#[cfg(test)]` 或 tests/ 目录)。

---

## Task 1 · 工具执行管线契约化(crates/shannon-core)

**现状(接地核实)**:"节点"是两个同形具体结构(`PermissionGateNode`/`PreToolUseHookNode`,均为 `async fn evaluate(&mut ToolGuardContext) -> Flow`,guard_nodes.rs:121/259),**无 trait**;`PIPELINE_PERMISSION`/`PIPELINE_HOOKS_PRE_TOOL_USE`(guard_nodes.rs:31,33)无人消费;构造内联于 agent_loop.rs:2813/3176。guards 只在 Phase 1 前置(agent_loop.rs:2719-3213),registry 与并行执行路径(spawn 于 3294-3298)只执行已批准调用。registry 已有 300s 默认超时(tools.rs:233,包裹于 581-591 与 737-752,超时返回 `Err(ToolError::Timeout)`),**无重试、无指标**;工具耗时从未测量——`ToolResultPayload.duration_ms` 存在且被 OTel(telemetry.rs)/投影(session_log/projections.rs:810-821)消费,但 `session_log/mod.rs:295` 恒填 `None`。`PostToolUseFailure` 事件**零生产者**(仅枚举变体与测试名单)。Phase 2 并行臂(agent_loop.rs:3294-3426)与串行臂(3457-3632)的结果处理逐行重复。

**要求(全部落地)**:

1. **Node trait 与管线结构**:guard_nodes.rs 定义 `pub trait ToolPipelineNode { fn name(&self) -> &'static str; async fn evaluate(&self, ctx: &mut ToolGuardContext) -> Flow; }`,两个既有节点实现之(name 分别取 `PIPELINE_PERMISSION`/`PIPELINE_HOOKS_PRE_TOOL_USE`);新增 `pub struct ToolExecutionPipeline { nodes: Vec<Arc<dyn ToolPipelineNode>> }` + `new(nodes)` + `run(ctx)`(顺序执行,现有 Flow 语义不变:两节点恒 Continue,判定读 ctx)。agent_loop Phase 1 改为构造管线一次(2813/3176 处)并对每个 tool_input `run`——**行为逐字节等价**,这是归位,不是重写。
2. **Around 阶段落 registry 层**:tools.rs execute/execute_streaming 的裸 timeout 包裹(581-591/737-752)升级为 around 段:① 测量 wall duration(`Instant`);② 仅当 `is_tool_read_only(name)==true` 且结果为 `Err(ToolError::Timeout)` 时重试 1 次(重试前 `tracing::warn!` 携带工具名与首次耗时);③ 超时时长/错误类型/流式语义不变。签名不变的私有辅助 `execute_with_around(...)`,两处调用。
3. **指标**:`crates/shannon-core/src/signals.rs` 新增 `ToolExecOutcome { Ok, Error, Timeout }` 与 `observe_tool_execution(tool: &str, outcome: ToolExecOutcome, duration_ms: u64)`(仿 `observe_permission_decision`,signals.rs:239-248),在 around 段调用;`QueryEvent::ToolUseResult` 增加字段 `duration_ms: Option<u64>`(字段定义处定位:core 内 QueryEvent 定义文件),两臂以 `Instant` 填充;`session_log/mod.rs:295` 的硬编码 `None` 改为透传该字段(OTel/投影无需改动即开始收到数据)。
4. **Post-execute 归一**:agent_loop 新增私有 `finalize_tool_result(...)`(或等价 helper),合并两臂重复块(ToolUseResult 事件 + `cap_tool_result` + `ToolResultEntry` + PostToolUse `publish_hook_trigger` + `triggered_routines` + `ToolProgress`);**Err 路径新增触发 `PostToolUseFailure`** 的 `publish_hook_trigger`(payload 含错误消息,风格参照 PostToolUse 的 is_error;这是该事件首个生产者);两臂改为调用,删除重复代码。
5. **契约文档**:guard_nodes.rs 顶部模块注释更新为管线契约(节点名/顺序/Flow 语义/如何加新节点),以代码注释承载,不新增 md。

**验收**:
- 契约模块注释存在;既有超时测试不改断言全绿(行为等价证据);
- 新增单测 ≥4:管线顺序执行、read-only Timeout 恰好重试 1 次、非 read-only 不重试、duration_ms 透传至 session body 非 None、Err 路径触发 PostToolUseFailure;
- `agent_loop_tests.rs`、tools.rs 既有测试全绿;clippy/fmt 干净。

---

## Task 2 · 事件派发语义表 + 机检(crates/shannon-engine)

**现状(接地核实)**:30 个 `HookEventType` 变体仅 doc 注释(events.rs:10-69),无任何机器可查的语义元数据。**派发双轨制**:六种类型经 bus 触发(adapter 解码,advisory,spawn),其余为直接 `run_hooks` 调用(REPL/compact/coordinator)。**10 个变体零生产者**;Task 1 落地后 `PostToolUseFailure` 获得生产者,**NoProducer 组剩 9 个**:StopFailure、Notification、PermissionDenied、PermissionRequest、FileChanged、CwdChanged、PostToolBatch、Elicitation、ElicitationResult。语义 gotcha(必须如实入表,但**本任务不改任何行为**):exit 2=Deny(manager.rs:303-317);blocking hook 超时=Err 传播→PreToolUseHookNode warn+Allow(超时钩子不阻塞不拒绝);`resolve_results`(772-796)首 Deny 短路、末 Modify 覆写、均 Allow;`if_condition` 已解析但从未求值(config.rs:282,318-343);matcher 正则先于管道分割(`"Edit|Write"` 作为正则而非 alternation);Stop 的 `should_continue` 恒 false(文档的 exit-2-continue 语义不可达)。CC 标准集 9 个:PreToolUse/PostToolUse/SessionStart/SessionEnd/Notification/UserPromptSubmit/Stop/SubagentStop/PreCompact,其余为 shannon 扩展。

**要求**:

1. 新文件 `crates/shannon-engine/src/hooks/semantics.rs`:
   ```rust
   pub enum Regime { EnforcedInline, BusTriggerAdvisory, DirectCall, NoProducer }
   pub enum ShortCircuit { DenyModify, DenyOnly, Observe, None }
   pub enum CcCompat { Standard, Extension }
   pub struct HookEventSemantics {
       pub event: HookEventType,
       pub regime: Regime,
       pub short_circuit: ShortCircuit,
       pub failure_semantics: &'static str,   // 如 "exit2=deny; timeout=err-propagates(allow)"
       pub cc_compat: CcCompat,
       pub producers: &'static [&'static str], // 模块路径,如 "agent_loop.rs"/"coordinator.rs";NoProducer 组为 &[]
       pub notes: &'static str,                // gotcha 备注可为 ""
   }
   pub fn all_event_semantics() -> &'static [HookEventSemantics]; // 30 条
   ```
2. **CI 机检 = 穷尽 match**:`all_event_semantics()` 内部对 `HookEventType` 做 `match { PreToolUse => ..., ..., }`(无通配臂)——未来新增变体不补表即**编译失败**;单测断言:`len()==30`、无重复 event、`EnforcedInline` 事件名能路由通过 custom-kind 订阅(参照 guard_nodes.rs:651-709 的 30 名单能力测试)。
3. 每条语义按上面"现状"与接地事实表填写;Regime 判定依据:PreToolUse=EnforcedInline(guard_nodes 直调);PostToolUse/Stop/UserPromptSubmit/SessionStart/SessionEnd/PreToolUse(adapter 六类)=BusTriggerAdvisory——注意 PreToolUse 兼具 EnforcedInline,取 `EnforcedInline` 并在 notes 注明另有 bus 镜像;PostCompact/其余直调=DirectCall;NoProducer 组(9 个)=NoProducer。CC 兼容标注按 CC 标准集。
4. 文档:新建 `docs/HOOK-SEMANTICS.md`,表格列=结构体字段,头部注明"机器真源:`semantics.rs`;本文档一致性由测试约束";加 smoke 测试:读该文件断言存在且包含全部 30 个事件名(不做逐字 diff)。
5. **行为零变化**:不改 manager.rs/config.rs 任何执行路径。

**验收**:报告判据(30 事件全部入表 + CI 机检上线)达成;穷尽 match 编译期强制;smoke 测试通过;shannon-engine 既有 hooks 测试全绿。

---

## Task 3 · 生效装配树 dump(crates/shannon-core + shannon-cli + shannon-skills)

**现状(接地核实)**:`config_dump.rs`(306 行)只覆盖配置层+env provenance(`DumpEntry/DumpLayer/ConfigDump`,41-73),**不含装配面**。`ToolRegistry` 无 per-tool 条目结构(tools.rs:160-163),`list_tools_info()`(527-539)缺 deferred/只读/破坏性/hidden 标志;来源只能从 `mcp__{owner}__` 名字推导(mcp_tool_adapter.rs:188;plugin/permissions.rs:387);deferred 仅计数与 search。`HookManager::hooks_file()`(809)/`configured_event_types()`(814-820)可运行时枚举挂载。`SkillRegistry::available_skills_metadata_with_budget`(registry.rs:549-604)裁剪但**不记录被裁项**;默认预算 2000。压缩阈值三处并存:agent_loop 60%/80% 硬编码(1342/1385)、`compression_threshold` 默认 0.8(types.rs:945)、CompactEngine `trigger_threshold` 0.75(compact/types.rs:46-68)。沙箱 world 句柄不保留(`AssembledWorlds` 被 `register_all_tools` 消费后即弃,shannon-tools/lib.rs:292-300),dump 需 re-`detect()` + re-`assemble` 取 kind("local"/"landlock");注意两个同名 `SandboxMode` 枚举(tool-interface 的 `Off|Local|Landlock` 才是相关者)。api_server 已有 `with_extra_routes` 合并点(296-302)与 `AppState.tools`;CLI `--dump-config` 早退模式 main.rs:820-827(旗)+5766-5791(处理)。测试范式:insta 快照(config_dump.rs:283-288)+ 纯函数字符串断言(commands_config_explain.rs:590+)。

**要求**:

1. **skills 小改**(shannon-skills/registry.rs):新增 `pub struct SkillBudgetReport { included: Vec<SkillMetadata>, dropped: Vec<String>, truncated: Vec<String> }` 与 `available_skills_metadata_with_budget_report(budget)`;现有 `available_skills_metadata_with_budget` 改为调用新实现后取 included(行为不变);单测覆盖超额 dropped 与描述截断 truncated。
2. **core 新模块** `crates/shannon-core/src/assembly_dump.rs`:
   - `ToolRuntimeInfo` 由 ToolRegistry 新方法 `list_tools_runtime() -> Vec<ToolRuntimeInfo>` 返回(name/category/read_only/destructive/hidden_from_llm/deferred);
   - `AssemblyDump { generated_at, tools: Vec<ToolAssemblyInfo>, hooks, skills, compaction, sandbox, partial: bool }`;
   - `ToolAssemblyInfo` 含 `source: Source`,`Source::Builtin | Mcp { server } | Plugin { plugin } | ToolSearch`,由名字前缀推导(`mcp__{owner}__`;`mcp__tool_search`→ToolSearch;owner 在已知插件集合→Plugin;其余→Mcp;无前缀→Builtin),doc 注明"来源为名字推导,非注册时声明";
   - `build_assembly_dump(registry: Option<&ToolRegistry>, hooks: Option<&HookManager>, skills: Option<&SkillRegistry>, config: &QueryEngineConfig, sandbox: &SandboxSettings, world_kind: Option<&str>) -> AssemblyDump`——所有来源可缺,缺失项在 JSON 中标 `"unavailable"` 并置 `partial:true`;
   - compaction 投影如实并列三个阈值及各自出处;
   - sandbox:`SandboxSettings::detect()`(由调用方传入)+ world kind + degrade notices。
3. **CLI**:`--dump-assembly` 长旗(main.rs 与 --dump-config 同款注册+早退处理)。**实现裁决**:早退点若 engine/registry 尚未构建,则允许两种落地之一——(i) 早期点输出 partial dump(config+沙箱+可枚举项);(ii) 若早期点数据明显不足,改为 `shannon config` 子命令族下的 `--dump-assembly`(engine 构建后执行,full dump)。**二选一,在实施报告说明选择理由**;输出 pretty JSON。
4. **API**:`GET /api/assembly` 返回同一 JSON(优先走 `with_extra_routes`;AppState 缺的来源经 with_* builder 在 serve 组装处接线,拿不到即降级 unavailable)。
5. **测试**:assembly_dump 单测(fixtures:注册 2-3 个工具 + 临时 hooks 文件 + 合成 config;断言 source 推导、deferred 标注、来源缺失降级)+ insta golden 快照;CLI 旗解析测试(仿 main.rs `test_cli_parse_dump_config_flag`,7270-7290);/api/assembly 路由 smoke。

**验收**(报告判据逐项):CLI 输出含**工具(含来源与 deferred 状态)、hook 挂载表、skill 预算裁剪(含被裁项)、压缩参数(三处阈值并列+出处)、沙箱模式/world kind**;`/api/assembly` 同数据;全来源缺失时优雅降级不 panic。

---

## 执行顺序与集成

Task 1 → Task 2(Task 2 的 NoProducer 组按 Task 1 之后状态标注;producers 指称与 Task 1 产出的 `finalize_tool_result` 一致)→ Task 3(独立,但最终 rebase 在前两者上)。分支最终 4 个 commit:plan + 3 任务。

## 风险与回退

- Task 1 是唯一触碰 agent_loop 热路径的任务,防线:行为等价约束(Phase 1 只换构造方式)+ 既有断言不改 + 两臂合并为纯移动式抽取;若 review 发现任何行为漂移,回退该 hunk 而非修补。
- Task 3 的名字推导 source 在混用自定义 MCP server 名与插件名同名时会误标——已知的、已文档化的近似,dump 场景可接受。
