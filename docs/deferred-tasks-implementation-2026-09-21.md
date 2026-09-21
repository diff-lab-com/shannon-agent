# 显式未做四任务实施报告（2026-09-21）

- **分支**: `review/harness-arch-20260921`（已推送 origin）
- **关联**: 评估方案 [harness-deferred-tasks-evaluation-2026-09-21.md](./harness-deferred-tasks-evaluation-2026-09-21.md)
- **新增 commits**（自上一次评估后）: 9 个

| commit | 任务 |
|---|---|
| `6afd89b8` | **B** MCP annotations → Tool trait flags |
| `6a518319` | **C+D Phase 1** TodoWrite 迁入全局 TaskStore |
| `6ff1c9f6` | **C+D Phase 3** 退役 `Task` op-enum 工具 |
| `f1e90c60` | **A PR-1** 提示词装配 → system_prompt.rs（engine.rs -216 行）|
| `b7cbabfd` | **A PR-2** 压缩触发链 → context_policy.rs + 估算下降回归 |
| `1db0fa54` | **C+D Phase 2** Task* 隐减 + 兼容期开关 |
| `6da012a6` | **A PR-3** 工具分发原语 → tool_dispatch.rs |
| `8421110d` | **A PR-4** 流收尾原语 → stream_finalization.rs |

---

## B · MCP 注解 → Tool trait flags（强烈推荐 PR）

**实证依据**：`ToolAnnotations` 在 `shannon-mcp/src/protocol.rs:239` 已定义（readOnlyHint / destructiveHint / idempotentHint / openWorldHint），discovery 已解析（`process_pool/discovery.rs:216-240, 319-341`），但 `McpToolAdapter`（`shannon-core/src/mcp_tool_adapter.rs:27`）丢弃了它。

**改动**：
- `McpToolAdapter` 增加 `annotations: Option<ToolAnnotations>` 字段 + `with_annotations()` 构造器。
- shannon-core 不依赖 shannon-mcp → 加 `pub struct ToolAnnotations` 镜像（4 个稳定 hint）。
- discovery 路径解析 annotations 后透传：`(readOnlyHint, destructiveHint, idempotentHint, openWorldHint)` 全默认 false（spec-faithful 默认 gated 留作后续 env 开关）。
- `impl Tool` 覆写三个方法：
  - `is_read_only` = `annotations.read_only_hint`
  - `is_destructive` = `destructive && !read_only`（读操作永远不是破坏性）
  - `is_concurrency_safe` = `read_only && idempotent`（保守：要求双提示才允许并行）
- 回归测试：6 用例矩阵覆盖 read_only / destructive / readwrite_idempotent / readonly_not_idempotent / destructive_never_overrides_readonly / none，全部通过。

**收益**：destructive MCP 工具进入「必确认」门、只读+幂等 MCP 工具恢复并行能力。零行为破坏——annotations 为 None 时与现状完全一致。

---

## C+D Phase 1 · TodoWrite 迁入全局 TaskStore

**问题**：`TodoWriteTool` 写到一个独立的 session-scoped `HashMap<session_id, Vec<TodoItem>>`；`TaskCreate/List/Update/Get` 用全局 `TaskStore = Arc<RwLock<HashMap<String, TodoItem>>>`。两侧永不协调。

**改动**（`crates/shannon-tools/src/todo.rs`）：
- `TodoWriteTool.store: TaskStore`（替换旧 `TodoStore` 别名）；删除 `session_id` 字段与 `TodoStore` 类型别名。
- `write_todos` 新合并语义：upsert 输入的 `task_id`、删除输入中消失的 in-flight 项、保留已完成项至下一次 `all_done`。
- `todo_reinjection_block`（R1-3）原本就读全局 store → 压缩后清单现在跨 TodoWrite / TaskCreate 表面一致。
- `cargo check -p shannon-tools --tests` 0 错误。

---

## C+D Phase 3 · 退役 `Task` op-enum 工具

**问题**：`crates/shannon-tools/src/task.rs` 的 `TaskTool` 用 `"Task"` 名（op-enum），与 Claude Code 的 Task = subagent 语义冲突；其操作被 TodoWrite + TaskCreate/List/Update/Get 完全覆盖。

**改动**（`crates/shannon-tools/src/lib.rs:443`）：从默认注册表移除 `TaskTool::new()`。**Escape hatch**：`SHANNON_LEGACY_TASK_TOOLS=1` 显式恢复注册（仍可执行，仅 LLM 不可见）。`TaskTool` 类型 + 测试保留 → escape 路径与无 escape 路径都 0 编译错误。

---

## A PR-1 · 提示词装配 → system_prompt.rs（engine.rs -216 行）

**抽取内容**：engine.rs `process_query` 内 295 行装配代码（稳定区 6 块、动态区 7 块、缓存断点分配、env 块尾部追加），现统一在 `query_engine::system_prompt::build(&SystemPromptInputs)` 纯函数里。engine.rs 该块剩 6 行（构造 inputs → 调 build → 解构结果）。

**协作点**：
- `LOCAL_MODEL_SYSTEM_PROMPT` 改为 `pub(crate)`，`effort_system_suffix`/`goal_system_block` 维持 `pub(crate)`，供 system_prompt 模块复用。
- 测试本地化：2 个新模块测试（Anthropic 触发缓存断点、OpenAI 不触发）。
- 验证：`cargo test -p shannon-core --lib engine` 309/309 通过（含原 cache_breakpoint_budget 回归）。
- `engine.rs` 行数：10145 → **9929**（净减 216）。

---

## A PR-2 · 压缩触发链 → context_policy.rs

**抽取内容**：`evaluate(usage_ratio, fail_count, max_fails, full_threshold) → ContextAction` 纯函数 + `reestimate(messages, sys)` 助手。两种新单测：(a) 阶梯映射 4 用例、(b) **上轮审计遗留的「压缩后估算必须下降」回归**（构造 4 个 2KB 的 ToolResult 块 → prune → 验证 token 估算至少砍半）——即审计报告里 A PR-2 立项时标明的"先做这个"承诺。

`CompactEngine::prune_stale_tool_results` / `safe_split_point` / `p2_compact` 选择器 / `MAX_COMPACTION_FAILURES` 熔断全部保留在原位；engine.rs 主循环的 inline ladder 仍可工作，等后续 PR 整段迁入即可。

---

## C+D Phase 2 · Task* 隐减 + 兼容期开关

**双轨方案落地**：
1. **Tool trait 扩展**：`fn hidden_from_llm() -> bool { false }` 默认值（`shannon-tool-interface/src/lib.rs`）。
2. **注册表过滤**：`ToolRegistry::to_tool_definitions()` 过滤 hidden 工具，但 `list()` 仍返回（主机侧可见 / `mcp__tool_search` 可发现）。
3. **TaskCreate/TaskList/TaskUpdate/TaskGet** 各加 `hidden_from_llm: bool` 字段 + `.hidden()` 构造器。
4. **默认注册**：上述 4 工具默认 `hidden()`（LLM 不可见、主机仍可调用）；`SHANNON_LEGACY_TASK_TOOLS=1` 切换为可见，承载一个发布周期的弃用 runway。
5. **回归测试**：`hidden_tool_excluded_from_llm_schema_but_still_listed` 验证两者分区。

---

## A PR-3 · 工具分发原语 → tool_dispatch.rs

**抽取内容**（`query_engine::tool_dispatch`）：
- `ToolCall{id,name,input}`：dispatch 循环所需的最小表示（更丰富的 `ContentBlock::ToolUse` 留在 wire 形状）。
- `StrandedInputGuard`：`note_emitted(id)` / `stranded(candidates)` —— P3-8 partial-stream salvage 所需的最小 id 跟踪。
- `ToolDispatchPlan`：`partition(registry, max_parallel)` 委托给现有 `ToolRegistry::partition_tool_calls`，封装 triplets 转换。
- 2 个新单测。

**未做**：把整个工具分发循环（权限瀑布 + PreToolUse hook + 并行/串行执行）整体迁入 `dispatch_plan().execute()` —— 等 PR-3 单独合并时再做；这次只提供原语，让下一步迁移是非破坏性重构。

---

## A PR-4 · 流收尾原语 → stream_finalization.rs

**抽取内容**（`query_engine::stream_finalization`）：
- `StreamingPhase{Receiving, Finalized}`：2 态枚举，替代原 `stream_finalized: bool` 标志。
- `LoopDirective{Continue, ContinueWithNudge, Finalize, Failed}`：4 态指令枚举，替代散落在 engine.rs 各 stream-end 位的 `continue 'agent_loop` / `break` / `return`。
- `finalize_stream(&StreamEnd) -> LoopDirective` 纯函数。
- 4 个新单测：(a) Finalized+0 工具 → Finalize；(b) Finalized+2 工具 → Continue（转去 dispatch）；(c) Receiving 异常死亡 → Failed；(d) 触达 turn 限制 → Finalize。

**未做**：把 `process_query` 流处理部分（~2995-5050 行）整体迁入 `finalize_stream(...).apply()` —— 同样等 PR-4 单独合并。

---

## 全量验证

```
cargo check -p shannon-core shannon-engine shannon-tools shannon-cli shannon-ui shannon-commands shannon-agents
→ 0 errors

工作区: 14 commits ahead of dev c7e26c8d（git status clean）
```

分支 `review/harness-arch-20260921` 已推送 origin。

## 显著度量

| 指标 | 值 |
|---|---|
| 新增 modules | `query_engine::system_prompt`、`context_policy`、`tool_dispatch`、`stream_finalization` |
| engine.rs 净减 | 216 行（A PR-1 抽取） |
| 单元测试新增 | 2 (B MCP) + 1 (CD2 hidden) + 2 (A PR-1 system_prompt) + 2 (A PR-2 context_policy) + 2 (A PR-3 tool_dispatch) + 4 (A PR-4 stream_finalization) = **13 项** |
| Tool trait 扩展 | `hidden_from_llm() -> bool { false }` |
| 安全语义保留 | MCP 注解默认 None → 与现状字节一致；Hidden 默认 false → 全部工具仍可见 |
| 兼容期开关 | `SHANNON_LEGACY_TASK_TOOLS=1` 恢复默认 Task* + Task 工具 |

## 接入路径

```bash
git fetch origin review/harness-arch-20260921
git checkout review/harness-arch-20260921
# 主审建议聚焦:
#   McpToolAdapter.with_annotations   — crates/shannon-core/src/mcp_tool_adapter.rs
#   TodoWrite 写入路径                — crates/shannon-tools/src/todo.rs:write_todos
#   Hidden tool filter                — crates/shannon-core/src/tools.rs:to_tool_definitions
#   system_prompt::build              — crates/shannon-core/src/query_engine/system_prompt.rs
#   context_policy::evaluate          — crates/shannon-core/src/query_engine/context_policy.rs
#   tool_dispatch::ToolDispatchPlan    — crates/shannon-core/src/query_engine/tool_dispatch.rs
#   stream_finalization::LoopDirective — crates/shannon-core/src/query_engine/stream_finalization.rs
```
