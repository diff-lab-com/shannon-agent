# OPC & Scheduled 页面深度差距分析与改进方案

**调研日期**: 2026-06-13
**范围**: 对 `/opc` (One Person Company 多 agent 看板) 和 `/tasks` (侧栏标为 "Scheduled" 的页面) 做逐行代码审计,对照 Codex Desktop / Cursor 3 / Windsurf Spaces / Hermes Cron / OpenClaw 的同类功能。
**目的**: 修正初版报告的误判,定位真实差距,给出可执行的改进方案。

---

## 0. 修正说明(重要)

初版《竞品调研》把「多 agent 并行面板」「cron 定时 UI」列为 **P0 缺失**,这是**错的**。实际情况:

| 功能 | 初版判断 | 真相 |
|---|---|---|
| 多 agent Kanban 看板 | ❌ 缺失 | ✅ **已有**(`/opc`,5 列看板 + Agent Swarm 侧栏) |
| 多 agent 工作流详情 | ❌ 缺失 | ✅ **已有**(`/opc/task`,pipeline + 执行日志 + Human-in-Loop) |
| 定时任务 UI | ❌ 缺失 | ⚠️ **名字叫 Scheduled,实际只是 background task 列表** — 这是最大的问题 |

所以真正的差距不是「没有」,而是 **「有但假」「有但名不副实」「有但缺关键交互」**。下面分类详述。

---

## 1. OPC 页面(`/opc` + `/opc/task`)深度分析

### 1.1 已实现(初版漏掉的)

✅ **Kanban 5 列布局**:To Do / Pending / Doing / Done / Deprecated(`OPC.tsx:174-245`)
✅ **Agent Swarm 侧栏**:显示 agent 名称、model、当前任务、active 状态(`OPC.tsx:104-148`)
✅ **Quick inject task**:顶部输入框直接创建任务(`OPC.tsx:155-168`)
✅ **Strategic Focus**:可编辑的战略焦点,持久化到 config(`OPC.tsx:68-95`)
✅ **Agent Workflow pipeline**:OPCTask 页水平 stepper(`OPCTask.tsx:38-62`)
✅ **Execution Log**:垂直时间线(`OPCTask.tsx:95-127`)
✅ **Human-in-the-Loop Review**:Approve / Rollback / Request Revision + 备注(`OPCTask.tsx:130-189`)
✅ **Efficiency Metrics**:Session Cost、Token Usage、Agents count、Tasks count(`OPCTask.tsx:195-232`)

### 1.2 假数据 / 桩代码问题(必须先修)

| 问题 | 位置 | 现状 | 影响 |
|---|---|---|---|
| 🔴 **"Doing" 进度条写死 65%** | `OPC.tsx:213` | `<div className="h-full bg-primary rounded-full w-[65%]" />` | 所有进行中任务都显示 65%,纯装饰 |
| 🔴 **Agent Harmony 是任务完成率冒充** | `OPCTask.tsx:206-210` | `tasks.filter(completed).length / tasks.length * 100` | 标签叫 "Agent Harmony",算法是完成率,误导 |
| 🔴 **Agent Workflow 状态写死** | `OPCTask.tsx:42` | `const isActive = i === 0` | 只有第一个 agent 显示 active,其他全 inactive |
| 🟡 **Kanban 卡片 `cursor-grab` 但无 DnD** | `OPC.tsx:181` | CSS 有抓手光标,实际不能拖拽 | 用户以为能拖,点了没反应 |
| 🟡 **Pending 列卡片样式与 To Do 重复** | `OPC.tsx:180-198` | 自定义红色 ring,但逻辑和 To Do 一样 | 区分度低 |
| 🟡 **Deprecated 列永远空** | `OPC.tsx:241-245` | 永远显示 EmptyState | 没有归档/取消逻辑 |

### 1.3 真实功能差距(vs Codex / Cursor / Windsurf)

| # | 差距 | 对标竞品 | Shannon CLI 后端 | 优先级 |
|---|---|---|---|---|
| G1 | **Kanban 拖拽(DnD)** | Windsurf Spaces / Trello 式 | N/A | **P0** |
| G2 | **点击 agent 打开其对话/日志** | Codex(thread 点击) / Cursor Agents Window | ✅ 有 session 数据 | **P0** |
| G3 | **Spawn / Stop agent 入口** | Codex / Hermes Profiles | ✅ TeamCreate/SendMessage | **P0** |
| G4 | **Agent 间消息流可视化** | Codex thread 内嵌对话 | ✅ SendMessage 已记录 | P1 |
| G5 | **Worktree 信息显示** | Codex(local/worktree 标签) / Cursor | ✅ worktree 隔离已实现 | **P0** |
| G6 | **任务依赖图(DAG)** | Windsurf / Devin | ❌ | P1 |
| G7 | **并行 vs 串行执行模式** | Codex(threads 并行) | ⚠️ Team 默认并行 | P1 |
| G8 | **任务指派到特定 agent** | Hermes(profiles) | ✅ agent 配置 | P1 |
| G9 | **任务 due date / priority 编辑** | Codex / Windsurf | ⚠️ priority 字段在,无 UI | P1 |
| G10 | **Agent 负载实时可视化** | Cursor Mission Control | ❌ | P2 |
| G11 | **多 session 聚合视图** | Codex(project 下多 thread) | ✅ multi-session | P1 |
| G12 | **"Mission Control" 全屏网格视图** | Cursor Exposé 风格 | N/A | P2 |

### 1.4 OPC 改进方案

#### Phase 1(必须,2 周)— 修假数据 + 核心交互

| Item | 动作 | 文件 | 预期效果 |
|---|---|---|---|
| F1 | 移除 "65%" 写死,接 `ToolProgress` 事件流 | `OPC.tsx:213` | 进度条反映真实工具调用进度 |
| F2 | "Agent Harmony" 改名为 "Task Completion" 或换算法(基于 agent 协作消息密度) | `OPCTask.tsx:202-212` | 不再误导 |
| F3 | Agent Workflow 状态接 agent.status,移除 `i === 0` | `OPCTask.tsx:42` | 每个 agent 显示真实状态 |
| F4 | 实现 Kanban DnD(用 `@dnd-kit/core`)+ 调后端更新 task.status | `OPC.tsx:171-247` | 卡片可在列间拖拽 |
| F5 | Agent Swarm 卡片可点击 → 跳转到该 agent 的 session | `OPC.tsx:124` | 点 agent 看其对话 |
| F6 | Agent Swarm 加 ⋮ 菜单:Stop / Pause / View Logs / Reassign | `OPC.tsx:124-145` | 可控制 agent |

#### Phase 2(核心,3 周)— 接 CLI 已有能力

| Item | 动作 | 依赖 |
|---|---|---|
| C1 | Agent 卡片显示 worktree 路径(`/feature/auth` 标签) | shannon-core worktree API |
| C2 | 新建任务时可选:assignee / priority / worktree / 并行or串行 | 扩展 `startBackgroundTask` 签名 |
| C3 | OPCTask 页加 "Agent Messages" 标签,显示 agent 间 SendMessage 历史 | 接 Team 消息总线 |
| C4 | 任务依赖:卡片上加「阻塞于 X」「阻塞 Y」标记 | 后端任务依赖模型(新增) |
| C5 | "Spawn Agent" 按钮 → 打开 agent 配置 drawer(model/tools/worktree) | shannon-agents 配置 |

#### Phase 3(差异化,4 周)— 超越竞品

| Item | 动作 | 差异化点 |
|---|---|---|
| D1 | Mission Control 全屏网格视图(每个 agent 一个 tile,实时输出流) | 对标 Cursor Exposé,但开源 |
| D2 | Hook 触发的任务自动入看板(PreToolUse/PostToolUse → 创建卡片) | Shannon 32 hook 事件是独有优势 |
| D3 | 任务模板:「bug fix」「feature」「refactor」预设 agent 编排 | 对标 Hermes skills |
| D4 | Cost budget per task:预算耗尽自动 pause | 对标 Hermes 成本控制 |

---

## 2. Scheduled 页面(`/tasks`)深度分析

### 2.1 已实现(初版漏掉的)

✅ **List / Month Calendar 视图切换**(`Tasks.tsx:131-134`)
✅ **状态筛选**:all / pending / running / completed(`Tasks.tsx:173-181`)
✅ **New Background Task 创建**(`Tasks.tsx:152-171`)
✅ **Cancel 确认弹窗**(`Tasks.tsx:567-581`)
✅ **Task Detail Drawer**(右侧抽屉,`Tasks.tsx:524-564`)
✅ **Task Execution Log**(backgroundTasks 时间线,`Tasks.tsx:398-426`)
✅ **AI Efficiency 卡片**(`Tasks.tsx:485-497`)
✅ **Agent Allocation 条形图**(`Tasks.tsx:500-517`)
✅ **分页**(`Tasks.tsx:396`)

### 2.2 🚨 名不副实问题(最严重)

> **这是整个 Desktop 当前最大的体验问题:页面叫 "Scheduled",侧栏标 "Scheduled",但完全没有「定时」功能。**

| 期望(基于竞品 + 名称) | 实际 | 差距 |
|---|---|---|
| Cron 表达式输入(`0 9 * * 1-5`) | ❌ 无 | **核心缺失** |
| 触发器类型(cron / webhook / event / interval) | ❌ 无 | **核心缺失** |
| Recurring 开关(一次性 vs 周期) | ❌ 无 | **核心缺失** |
| 下次执行时间显示 | ❌ 无 | 重要 |
| Calendar 显示未来 schedule | ❌ 只显示当天 running task 色块 | 装饰性 |
| 点 calendar 日子创建 schedule | ❌ 只筛选 | 交互缺失 |
| "Run Now" 触发原 schedule | ❌ 调 `startBackgroundTask("Execute task: X")` 新建任务 | 行为错误 |
| 执行历史 / 成功率 | ❌ 无 | 重要 |
| 结果路由(Slack / 邮件 / 通知) | ❌ 无 | 差异化机会 |
| Triage 队列(自动化产出待审) | ❌ 无 | Codex 核心卖点 |

### 2.3 假数据问题

| 问题 | 位置 | 现状 |
|---|---|---|
| 🔴 **"AI Efficiency" 是简单完成率** | `Tasks.tsx:103-105` | `completedCount / totalCount`,不是真正的「无人值守完成率」 |
| 🔴 **Agent Allocation 均分假数据** | `Tasks.tsx:108-115` | `Math.round(100 / agents.length)` 写死均分,与真实负载无关 |
| 🟡 **Calendar 高亮逻辑有 bug** | `Tasks.tsx:453-456` | `tasks.some(running)` 不绑定具体日期,所有日子都会高亮 |
| 🟡 **"Scheduled Tasks" 标题误导** | `Tasks.tsx:123` | 没有任何 schedule 概念,实际是 background tasks |

### 2.4 Shannon 后端已有哪些「定时」能力(可接入)

✅ **Scheduled Routines**:`crates/shannon-core/src/routines/` — cron-like,间隔触发
✅ **Triggered Routines**:32 hook 事件触发(PreToolUse/PostToolUse/TaskCompleted 等)
✅ **`/routine` CLI 命令**:管理 routines
✅ **WebhookRegistry**:HMAC-SHA256 签名的 webhook 触发
✅ **EventPublisher**:事件分发,带 retry

**结论**:后端能力完整,Desktop 完全没接。纯 UI 工作量。

### 2.5 真实功能差距(vs Codex Automations / Hermes Cron / OpenClaw)

| # | 差距 | 对标竞品 | 后端就绪? | 优先级 |
|---|---|---|---|---|
| S1 | **Cron 表达式编辑器 + 预览** | Hermes / Codex / OpenClaw | ✅ | **P0** |
| S2 | **触发器类型选择**(cron/webhook/event/interval) | 全员 | ✅ | **P0** |
| S3 | **Recurring vs One-shot 开关** | 全员 | ✅ | **P0** |
| S4 | **下次执行时间预览**("下次:2026-06-14 09:00") | Codex / Hermes | ⚠️ 需计算 | **P0** |
| S5 | **Calendar 真正显示未来 schedule** | 全员 | ✅ | **P0** |
| S6 | **点 calendar 日子创建 schedule** | Hermes | N/A | P1 |
| S7 | **执行历史 + 成功率 + 平均耗时** | Codex / Hermes | ⚠️ 需记录 | P1 |
| S8 | **Triage 队列**(自动化产出集中 review) | Codex(独家) | ✅ 可做 | **P0** |
| S9 | **结果路由**(Slack/邮件/通知/日志) | Hermes(10+ 渠道) / OpenClaw | ⚠️ 需 adapter | P1 |
| S10 | **Webhook URL 生成 + 签名配置** | OpenClaw / Codex | ✅ WebhookRegistry | P1 |
| S11 | **自然语言定时**("每天早上 9 点") | Hermes | ⚠️ 需 LLM 解析 | P2 |
| S12 | **无人值守 worktree 执行**(Codex 专用 background worktree) | Codex | ✅ worktree | P1 |
| S13 | **Schedule 模板库**(每日 standup / 周报 / 依赖扫描) | Hermes skills | N/A | P2 |

### 2.6 Scheduled 改进方案

#### Phase 1(必须,2 周)— 让 "Scheduled" 名副其实

| Item | 动作 | 文件 |
|---|---|---|
| P1.1 | 新建 `useRoutines()` hook,封装 list/create/update/delete routine | 新文件 `hooks/useRoutines.ts` |
| P1.2 | Tauri 命令暴露:`list_routines / create_routine / update_routine / delete_routine / trigger_routine_now` | `src-tauri/src/commands.rs` |
| P1.3 | Tasks.tsx 拆分:顶部加「Schedules / History / Triage」三标签 | `Tasks.tsx:121` |
| P1.4 | Schedule 创建表单:名称 / 触发器类型(cron/webhook/event/interval) / cron 表达式 + 人话预览 / assignee / worktree / prompt | 新组件 `CreateScheduleDialog.tsx` |
| P1.5 | Cron 预览:输入 `0 9 * * 1-5` 显示「周一至周五 09:00」+ 下 3 次执行时间 | 用 `cron-parser` npm 包 |
| P1.6 | Calendar 真正渲染 schedule:未来日期格显示 schedule 点 + tooltip | `Tasks.tsx:184-232` |
| P1.7 | "Run Now" 改为调 `trigger_routine_now(id)`,而非新建 task | `Tasks.tsx:65-74` |
| P1.8 | 修 Calendar 高亮 bug:`tasks.some(running)` → 按 schedule 的 next_run 筛选 | `Tasks.tsx:453-456` |

#### Phase 2(核心,3 周)— Triage + 历史 + 路由

| Item | 动作 |
|---|---|
| P2.1 | **Triage 标签页**:列出所有自动化产出(Draft PR / 生成的报告 / 待 merge 的变更),支持批量 Approve / Reject / 改派 |
| P2.2 | **History 标签页**:每个 schedule 的运行历史(时间/状态/耗时/cost/token),可点开看 output |
| P2.3 | **结果路由配置**:schedule 创建时可选「结果发送到 → Slack #channel / 邮件 / 系统通知 / 日志文件」 |
| P2.4 | **Webhook schedule**:生成唯一 URL + 配置签名密钥,展示 curl 示例 |
| P2.5 | **无人值守 worktree**:schedule 绑定专属 background worktree,避免污染主分支 |
| P2.6 | **失败重试策略**:指数退避 + 最大重试次数 + 失败通知 |

#### Phase 3(差异化,3 周)— 超越竞品

| Item | 动作 | 差异化点 |
|---|---|---|
| P3.1 | **自然语言定时**:输入「每天早上 9 点扫一遍 dependencies」→ LLM 生成 cron | 对标 Hermes,但用 Shannon 自己的 LLM |
| P3.2 | **Schedule 模板库**:内置「每日 standup 总结」「每周依赖扫描」「PR 自动 review」「Changelog 生成」 | 对标 Hermes 150+ skills |
| P3.3 | **Hook → Schedule 联动**:PostToolUse 触发的 routine 在 Triage 显示来源 hook | Shannon 32 hook 事件独家 |
| P3.4 | **Budget per schedule**:每个 schedule 月度 cost 上限,超限自动停 | 对标 Hermes 成本哲学 |
| P3.5 | **Schedule DAG**:schedule A 成功 → 触发 schedule B(依赖链) | 超越竞品 |

---

## 3. 跨页面共性问题

| 问题 | 影响 | 建议 |
|---|---|---|
| **"Agent Allocation" 假均分数据** | 两个页面都有(`Tasks.tsx:108` / 间接影响 OPC) | 后端加 agent 负载统计 API |
| **进度条/百分比都是装饰** | 用户信任度受损 | 统一接真实事件流 |
| **Calendar 是装饰性的** | Scheduled 页特别严重 | 改为真正的 schedule 视图 |
| **无键盘快捷键(⌘N 新建 / ⌘K 搜索 / Space 暂停)** | 效率低 | 加全局快捷键 |
| **OPC 和 Scheduled 数据模型重叠** | task / backgroundTask / routine 三个概念混 | 统一为「Task」(含 schedule 字段) |
| **点任务后无明确的「打开对话」入口** | OPC 和 Scheduled 都不能跳回 Chat | 加深链接 |

---

## 4. 实施路线图(建议)

### Sprint 1(2 周)— 修信任
- OPC:移除 65% 写死、修 Agent Harmony、修 Agent Workflow 状态(F1-F3)
- Scheduled:修 Calendar 高亮 bug、改 "Run Now" 行为(P1.7-P1.8)
- 共性:Agent Allocation 接真实负载

### Sprint 2(3 周)— 让 Scheduled 名副其实
- 完成 Scheduled Phase 1(P1.1-P1.8)
- 交付物:能创建真正的 cron schedule,日历显示未来执行

### Sprint 3(3 周)— Kanban 可交互
- 完成 OPC Phase 1(F4-F6)+ Phase 2(C1-C2)
- 交付物:可拖拽 Kanban + agent 可点击 + worktree 显示

### Sprint 4(3 周)— Triage + 自动化闭环
- Scheduled Phase 2(P2.1-P2.6)
- 交付物:Triage 队列 + 结果路由 + 历史

### Sprint 5(4 周)— 差异化
- Scheduled Phase 3(P3.1-P3.5)+ OPC Phase 3(D1-D4)
- 交付物:自然语言定时、schedule DAG、Mission Control 视图

---

## 5. 关键判断

1. **「Scheduled 页根本没定时」是当前最严重的体验问题**,优先级高于 Kanban DnD。因为用户看到名字会期待,点进去发现是假的,信任崩塌。
2. **OPC 的假数据(65% / Agent Harmony / `i===0`)是第二严重**,因为它假装在工作。
3. **Shannon CLI 后端能力完整**(routine / hook / worktree / team),Desktop 差距主要是 **UI 没接**,不是能力缺失。这意味着改进的 ROI 很高。
4. **差异化机会**:Hook → Schedule 联动、Schedule DAG、Mission Control 是竞品没有或做得弱的,Shannon 的 32 hook 事件是天然壁垒。

---

**审核要点**:
- [ ] Phase 划分是否符合你的迭代节奏?
- [ ] P0 优先级是否需要调整?
- [ ] 是否有未列出的竞品特性需要补?
- [ ] 数据模型统一(task/routine)是否需要单独 ADR?
