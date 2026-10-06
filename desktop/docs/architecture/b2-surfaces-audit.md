# B2 Surfaces Audit — TeamContext 覆盖矩阵（PR #94 / #95 / #96 后）

> 每个 runner 的工具注册来源 + 是否共享 chat 的 `TeamContext`。
> 用于确认 B2 收尾后没有遗漏的占位符路径。改任何 runner 的注册方式时更新本表。

| Surface | 工具注册来源 | TeamContext | team_task_* | agent_spawn | 备注 |
|---|---|---|---|---|---|
| **Chat**（interactive） | `AppState.tools`（`register_default_tools_with_providers`，启动一次） | ✅ `agent_teams::enable` 注入 `state.agent_tool_context`，`AgentTool` 经共享 handle 读取 | ✅ `register_team_tools_arc` | ✅ 真执行（子 QueryEngine + 继承 approval_mode/denylist） | `subagent:start\|stop` 事件桥 → UI |
| **Goal run**（`EngineGoalTurnRunner`） | per-run `ToolRegistry::new()`（PR #95） | ✅ `swap_agent_tool_context` 共享 chat handle（#95） | ✅ `register_team_tools_when_enabled`（#95） | ✅ 真执行（#95 + #94 executor） | goal_get/goal_update 仍 per-run 隔离 |
| **Routine / Inbox run**（`spawn_routine_run`） | `RoutineRunDeps.tools = state.tools.clone()` | ✅ 自动（共享 `state.tools` 的 Arc） | ✅ `agent_teams::enable` 注册到 `state.tools` 时自动生效 | ✅ 真执行 | 无需额外接线 |
| **Batch run**（`spawn_batch_run`） | `BatchRunDeps.tools = state.tools.clone()` | ✅ 自动 | ✅ 自动 | ✅ 真执行 | 同上 |
| **Trigger loop / scheduled**（`scheduled_commands`） | `RoutineRunDeps::from_state` → `state.tools` | ✅ 自动 | ✅ 自动 | ✅ 真执行 | off-peak model override 不受影响 |
| **Loopback API**（IM/mobile 触发） | `build_server` 独立 `ToolRegistry` | ✅ `swap_agent_tool_context` 共享 chat handle（#96）——handle 而非快照，启动后开启开关对下一个 loopback turn 即时生效 | ✅ `register_team_tools_when_enabled`（#96） | ✅ 真执行（#96 + #94 executor） | 事件桥在共享 registry 上，loopback 子智能体同样上浮 UI |
| **Mobile dispatch / TUI / CLI**（process-mode） | `shannon --team-agent` 子进程 | n/a | n/a | n/a | **并行架构，非占位符**：子进程自带完整工具注册与 JSON-RPC 主循环（stdin/stdout），服务于 TUI/CLI 多进程部署。桌面默认 `AgentMode::InProcess`（有单测锁定），有意不消费此路径 |

## 关键不变量

1. **单一 coordinator**：chat、goal、routine、batch、trigger、loopback 的
   `team_task_*` 与 `send_message` 落在同一个 `AgentCoordinator`
   （`TeamContext.coordinator`）。Tasks 页面板（`list_subagents`）读到的是同一
   registry。
2. **关闭开关 = 完全旧行为**：`agent_teams_enabled=false`（默认）时
   `agent_tool_context` 为空——所有 surface 的 `agent_spawn` 走占位符，
   `team_task_*` 不注册。零成本。
3. **Executor 下沉**（PR #94）：`TeamContext::with_executor` →
   `SubAgentRegistry::set_executor` → `add_teammate(..., Some(executor))` →
   `Teammate::handle_chat_message` 走真 LLM。`send_message` 的
   `extract_real_reply` 由此收到真回复（此前是 5s 超时落 synthetic ack）。
4. **Handle 共享优先于快照**：跨 registry 的 team 状态共享一律 swap handle
   （`Arc<Mutex<Option<TeamContext>>>`），不快照其中的 coordinator 值——这样
   启动后才发生的 `agent_teams::enable` 注入对每个 surface 的下一次调用即时
   可见（goal runner #95 与 loopback #96 均如此；PR #93 的 build-time 快照
   已被 #96 替代）。

## 非目标（有意不做）

- **桌面消费 process-mode**：`shannon --team-agent` 是 TUI/CLI 的并行部署
  架构（子进程自治 LLM 循环），不是桌面 in-process 路径的缺口。桌面要走
  process-mode 需补进程生命周期管理、worktree 隔离、跨进程权限转发与崩溃
  恢复——只有当 mobile-dispatch 项目明确提出需求时再随其传输层设计。
- **Loopback 独立 agent-teams 开关**：loopback 与 chat 共用同一个
  `agent_teams_enabled` 设置；不引入第二个开关（费用面一致，语义更简单）。
