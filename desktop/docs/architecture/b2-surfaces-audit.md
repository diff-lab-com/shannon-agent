# B2 Surfaces Audit — TeamContext 覆盖矩阵（PR #94 / #95 后）

> 每个 runner 的工具注册来源 + 是否共享 chat 的 `TeamContext`。
> 用于确认 B2 收尾后没有遗漏的占位符路径。改任何 runner 的注册方式时更新本表。

| Surface | 工具注册来源 | TeamContext | team_task_* | agent_spawn | 备注 |
|---|---|---|---|---|---|
| **Chat**（interactive） | `AppState.tools`（`register_default_tools_with_providers`，启动一次） | ✅ `agent_teams::enable` 注入 `state.agent_tool_context`，`AgentTool` 经共享 handle 读取 | ✅ `register_team_tools_arc` | ✅ 真执行（子 QueryEngine + 继承 approval_mode/denylist） | `subagent:start\|stop` 事件桥 → UI |
| **Goal run**（`EngineGoalTurnRunner`） | per-run `ToolRegistry::new()`（PR #95） | ✅ `swap_agent_tool_context` 共享 chat handle（#95） | ✅ `register_team_tools_when_enabled`（#95） | ✅ 真执行（#95 + #94 executor） | goal_get/goal_update 仍 per-run 隔离 |
| **Routine / Inbox run**（`spawn_routine_run`） | `RoutineRunDeps.tools = state.tools.clone()` | ✅ 自动（共享 `state.tools` 的 Arc） | ✅ `agent_teams::enable` 注册到 `state.tools` 时自动生效 | ✅ 真执行 | 无需额外接线 |
| **Batch run**（`spawn_batch_run`） | `BatchRunDeps.tools = state.tools.clone()` | ✅ 自动 | ✅ 自动 | ✅ 真执行 | 同上 |
| **Trigger loop / scheduled**（`scheduled_commands`） | `RoutineRunDeps::from_state` → `state.tools` | ✅ 自动 | ✅ 自动 | ✅ 真执行 | off-peak model override 不受影响 |
| **Loopback API**（IM/mobile 触发） | `build_server` 独立 `ToolRegistry` | ❌（有意） | ✅ 显式 `register_team_tools(&mut tools, chat_coordinator)`（PR #93） | ❌ 占位符（有意） | 无 `subagent:*` 事件桥，保持 pre-B2 行为；任务板共享 |
| **Mobile dispatch**（process-mode） | `shannon --team-agent` 子进程 | n/a | n/a | n/a | 独立交付物，不属 desktop in-process 路径 |

## 关键不变量

1. **单一 coordinator**：chat、goal、routine、batch、trigger 的 `team_task_*` 与
   `send_message` 落在同一个 `AgentCoordinator`（`TeamContext.coordinator`）。
   Tasks 页面板（`list_subagents`）读到的是同一 registry。
2. **关闭开关 = 完全旧行为**：`agent_teams_enabled=false`（默认）时
   `agent_tool_context` 为空——所有 surface 的 `agent_spawn` 走占位符，
   `team_task_*` 不注册。零成本。
3. **Executor 下沉**（PR #94）：`TeamContext::with_executor` →
   `SubAgentRegistry::set_executor` → `add_teammate(..., Some(executor))` →
   `Teammate::handle_chat_message` 走真 LLM。`send_message` 的
   `extract_real_reply` 由此收到真回复（此前是 5s 超时落 synthetic ack）。

## 已知占位符（有意保留）

- Loopback `agent_spawn`：见上表备注。
- `AgentMode::Process` 路径（`shannon --team-agent`）：独立项目。
