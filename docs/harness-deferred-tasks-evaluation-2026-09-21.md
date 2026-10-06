# 显式未做任务评估与实施方案（2026-09-21）

- **分支**: `review/harness-arch-20260921`（已推送 origin）
- **关联**: [harness-wave-r0-r1-r2-implementation-2026-09-21.md](./harness-wave-r0-r1-r2-implementation-2026-09-21.md) §6 列出的四项显式跳过任务
- **性质**: 评估 + 方案。本文档不含代码改动。

## 结论速览

| 任务 | 推荐度 | 规模 | 一句话理由 |
|---|---|---|---|
| B. MCP 注解 → Tool trait flags | **强烈推荐，先做** | ~0.5-1 天 | 最小改动、直接安全收益：destructive MCP 工具进入「必确认」门、只读 MCP 工具恢复并行能力 |
| C+D. 任务面整合 + `Task` 改名/退役 | **推荐，合并为一个方案** | ~3-5 天 | 7 个默认注册的任务工具互相重叠；`Task` 与 Claude Code 语义冲突会被预训练习惯误用；R1-3 已完成一半（全局存储） |
| A. `process_query` 深度拆分 | **推荐，分 4 个 PR 渐进** | ~1-2 周 | 纯可维护性投资；mocked-SSE 测试网已就位使风险可控；每步独立可合 |

---

## B. MCP `destructiveHint`/`readOnlyHint` → Tool trait flags

### 现状（已核实）

- `crates/shannon-mcp/src/protocol.rs:239` `ToolAnnotations` 已定义 `read_only_hint` / `destructive_hint` / `idempotent_hint` / `open_world_hint`，discovery 双路径解析它（`process_pool/discovery.rs:216-240, 319-341`）。
- `McpToolAdapter`（`crates/shannon-core/src/mcp_tool_adapter.rs:27+`）**不持有 annotations**，`impl Tool` 只覆写 `name`/`description`。
- Trait 缺省（`shannon-tool-interface/src/lib.rs:160-179`）：`is_read_only=false`、`is_concurrency_safe=is_read_only()`、`is_destructive=false`。
- 后果：所有 MCP 工具 (a) 永不触发 `is_destructive` →「必确认」门（`shannon-core/src/tools.rs:709-711`）；(b) 永不并行；(c) 只读 MCP 工具在 plan 模式判定中按名字表兜底而非 trait 派生。

### 方案（1 个 PR）

1. `McpToolAdapter` 增加字段 `annotations: Option<crate_mcp::ToolAnnotations>`（`shannon-core` 已依赖 `shannon-mcp` 的类型或经 `shannon-mcp` re-export）；两个构造路径（stdio discovery、pooled adapter）透传。
2. `impl Tool` 覆写三个方法：
   - `is_read_only` = `annotations.read_only_hint`（缺省 false，保持现状）
   - `is_destructive` = `annotations.destructive_hint && !read_only_hint`
   - `is_concurrency_safe` = `is_read_only() && idempotent_hint`（幂等才允许并行，保守）
3. **规格默认值决策（显式记录）**：MCP 规范中 `destructiveHint` 缺省为 true；shannon-mcp 当前 serde default 为 false。本方案 **保持 parse 缺省 false**（避免给全部未注解 MCP 工具骤然加确认），把「spec-faithful 缺省」列为独立的可选后续（env `SHANNON_MCP_SPEC_DEFAULTS=1` 实验开关）。
4. 测试：注解三态（只读/破坏/双无）→ flags 断言；「破坏性 MCP 工具必确认」走 `PermissionGateNode` 的集成测试；只读+幂等 MCP 工具进入并行批次的测试（`partition_tool_calls`）。
5. 验收：`cargo test -p shannon-core -p shannon-mcp` 全绿；现有 MCP 集成测试无回归。

风险：低。行为变化仅对**声明了注解**的服务器生效（当前生态很少声明）。

---

## C+D. 任务面整合 + `Task` 工具退役（合并方案）

### 现状（已核实）

`register_all_tools`（`shannon-tools/src/lib.rs:443-450`）默认注册 8 个任务类工具：`TodoWrite`、`TaskCreate`、`TaskList`、`TaskUpdate`、`TaskGet`、`Task`（op-enum，`task.rs:173`，名字与 Claude Code 的 Task=子代理 spawn 冲突）、`TaskOutput`、`TaskStop`；条件注册 `team_task_create/update/list`（`shannon-agents/src/task_tools.rs`）。存储分裂：R1-3 后 `Task*` 共享进程全局 `TaskStore`，`TodoWrite` 仍是独立 session-scoped `TodoStore`（`HashMap<session_id, Vec<TodoItem>>`）。桌面端多处引用（`desktop/src/commands.rs` 等）。

### 方案（3 个阶段，可各出 1 个 PR）

**Phase 1 — 存储统一（低风险）**
1. `TodoWriteTool` 迁移到全局 `TaskStore`：其整表替换语义映射为「按 `content` 归并 upsert + 删除列表中已消失的未完成项」（保持 TodoWrite 对模型的契约不变——"每次发全量"）。
2. `todo_reinjection_block()` 与 `TaskList` 读同一份数据；`/todo`、`/tasks` 命令与桌面任务页验证一致。
3. 测试：TodoWrite→TaskUpdate 交叉可见；TodoWrite 替换语义的归并/删除规则；reinjection 块包含两路写入。

**Phase 2 — 表面收敛（中风险，兼容期一版）**
1. 默认注册表只保留 `TodoWrite` + `TaskOutput` + `TaskStop`（三者语义互不重叠：计划清单 / 后台代理读取 / 后台代理终止）。
2. `TaskCreate/List/Update/Get` 改为**隐藏别名**：仍注册但 `description` 前缀 `[deprecated] use TodoWrite`，且从 `to_tool_definitions()`（发给模型的 schema）中排除——注册表已有 deferred 机制可复用，或加 `hidden_from_llm: bool` 字段。
3. 桌面/命令路径若直接按名调用（grep 已确认 `desktop/src/commands.rs` 等引用），保留原实现不破坏；env `SHANNON_LEGACY_TASK_TOOLS=1` 可让旧工具重新对模型可见（回退开关）。
4. 更新 R1-1 提示词的扩展工具政策段（删除 TaskCreate 系列、确认 TodoWrite 表述）。

**Phase 3 — `Task` op-enum 工具退役（即任务 D）**
1. `task.rs` 的 `Task` 工具操作已被 `TodoWrite`+`TaskUpdate` 完全覆盖：默认注册表**直接移除**（不在别名清单里——它就是语义冲突源）。
2. `tests` 中 `assert_eq!(tool.name(), "Task")` 等同步删除；`pub use task::TaskTool` 保留导出一版供桌面编译，下一版删除。
3. schema 快照重生成；`/search`、skills 文档中若提及同步更新。

验收：默认模型可见工具面 -5；`cargo test -p shannon-tools -p shannon-ui -p shannon-commands` 全绿；一个手测脚本跑通「TodoWrite 建单 → 压缩 → reinjection 包含清单 → TaskUpdate 完结」全链路。

---

## A. `process_query` 深度拆分

### 现状（已核实）

`engine.rs` 10,145 行；`process_query`（1481 → ~6050）约 4,570 行，内含：系统提示词装配（1540-1810，纯函数化程度高）、producer+`'agent_loop`（2143+）、P-B checkpoint（2207）、A10 wrap-up（2259）、权限瀑布（~3600-3900，guard_nodes 已抽象但调用内联）、工具分发（~3900-4400）、流式事件处理（2995-5300s，`StreamingPhase` 状态机内联）、压缩链（2450-2620）、错误阶梯（`recovery` 模块已出，调用点仍内联）。已有独立模块：`parsers` / `routing` / `env_config` / `recovery` / `guard_nodes` / `compact`（engine crate）/ `streaming`。测试：152 个 engine 测试 + mocked-SSE DSL + 录制回放。

### 方案（4 个 PR，每步独立可合、全量测试门禁）

**PR-1：系统提示词装配 → `query_engine/system_prompt.rs`（~300 行迁出，风险低）**
- 纯函数：`fn assemble(config, context, tools, memory_injection, injector, repo_map, plan_active) -> Vec<SystemContentBlock>`，含稳定/动态分区 + 4 断点放置 + env 块。
- 现有测试（断点预算、prompt 内容 pin、memory 动态区）直接覆盖。**先做这个**：它同时是提示词迭代的活跃区域。

**PR-2：压缩触发链 → `query_engine/context_policy.rs`（~200 行）**
- `fn evaluate_context_pressure(&ConversationState, &config, effective_max) -> ContextAction{None, Warn, MicroPrune, Compact}` 把 60/70/80% 内联判定变成可单测的纯函数；CompactEngine 调用留在原位。
- 补「压缩后估算必须下降」回归测试（上轮审计遗留建议）。

**PR-3：工具分发阶段 → `query_engine/tool_dispatch.rs`（~500 行，风险中）**
- 定义 `ToolPhaseContext`（tx、session_bus、permissions、hook_manager、tool_results、consecutive_denials 等的可变借用包）+ `async fn run_tool_phase(ctx, tool_calls) -> ToolPhaseOutcome{Continue, DenyNudge, Abort}`。权限瀑布 + 并行/串行批次 + PreToolUse hook 节点整体迁入。
- guard_nodes 的 Waterfall 语义不变；用现有 mocked-SSE 用例（A13 flush、denial 熔断）做行为金标准。

**PR-4：流事件处理收尾 → `query_engine/stream_finalization.rs`（~400 行，风险中高，最后做）**
- 截断续写（A13）、think-only nudge、partial 保全、Failed/Completed 判定收拢为 `fn finalize_stream(TurnState, StreamEnd) -> LoopDirective`。
- `LoopDirective{Continue, ContinueWithNudge, Finalize}` 枚举取代散落的 `continue 'agent_loop` / `break` / `return`——这是把 4,570 行变成可读管线的关键一步。

每 PR 验收：`cargo test -p shannon-core` 全绿 + `cargo test -p shannon-cli --test trace_commands` + `just dev`；`process_query` 最终 <1,500 行；无行为变化（golden 会话回放对比）。

---

## 实施顺序建议

1. **B**（半天，安全收益即时）→ 2. **C+D Phase 1+3**（`Task` 退役 + 存储统一，1-2 天）→ 3. **A PR-1/PR-2**（与提示词/压缩的活跃开发对齐）→ 4. **C+D Phase 2**（表面收敛，需要一版兼容期）→ 5. **A PR-3/PR-4**。
