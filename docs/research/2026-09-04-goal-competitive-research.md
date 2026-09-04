# `/goal` 功能竞品深度调研与 Shannon 差距分析

- 日期：2026-09-04
- 调研范围：11 个竞品产品 + 3 条框架线，双 agent 并行深调（开源读源码、闭源读官方文档/CHANGELOG/论坛），Shannon 侧逐文件核验证代码库
- 性质：只读调研 + 差距分析，为 [goal 设计方案](../plans/2026-09-04-goal-design.md) 提供依据
- 证据标注：【源码】=直接读取开源仓库源码；【文档】=官方文档/CHANGELOG 已验证；【推断】=二手来源或合理推断

---

## 0. 核心结论（TL;DR）

1. **`/goal` 已经是被头部产品做成一等公民的真实范式，不再是概念**。Claude Code 于 2.1.139（2026-05）添加 `/goal`；OpenAI Codex CLI 0.128.0+ 有完整开源的 Goals 体系（`codex-rs/ext/goal/`）。两家定义高度一致：**"设定一个完成条件，agent 跨轮持续工作直到满足"**——goal 的本质不是"目标描述"，而是**终止条件的对象化**。
2. 行业数据模型收敛于：`{ objective 文本, status 生命周期枚举, 预算/用量记账, timestamps }`，且 goal 一律是 **session/thread 作用域**（非项目级、非全局）。
3. 2026 年的创新集中区是**防漂移**（anti-drift）：完成审计、无进展分类、阻塞阈值、反 goal-shrinking、anti-spin、check-in 退避。反面教材是 AutoGPT/BabyAGI 时代"静态注入 + 无完成判据 + 无循环检测"导致的必然漂移。
4. **Shannon 目前完全没有 goal 功能**（无命令/无状态/无持久化），但存在 `SOURCE_GOAL_RESUME = "goal.resume"` 预留常量（`shannon-types/src/session_event.rs:263`，从未引用）和 `/ralph` 完成循环原型（关键词 grep 判完成，无审计、无持久化、无防失控记账）。
5. Shannon 的差距不在"有没有"，而在**完成判定的工程纪律**与**跨会话持久化**。现有 `/plan`（计划层）、TodoWrite/Task 工具（执行层）、`/focus`（注意力层）已就位，缺的正是竞品定义中的**终止条件层**。

---

## 1. 两条 first-class 路线：Claude Code 与 Codex CLI

### 1.1 Claude Code（闭源）——evaluator + check-in 工程化

**功能定义**【文档】：官方命令文档存在字面 `/goal [condition|clear]`——"Sets a goal so that Claude keeps working across turns until the condition is met"。CHANGELOG 2.1.139（2026-05-12）原文："Added `/goal` command: set a completion condition and Claude keeps working across turns until it's met. Works in interactive, `-p`, and Remote Control. Shows live elapsed/turns/tokens as an overlay panel."

**UX**【文档】：
- 设置：`/goal <完成条件>`（如 `/goal All tests passing in CI, no TypeScript errors`）
- 查看：`/goal` 无参数显示当前或最近达成的 goal
- 清除：`clear` / `stop` / `off` / `reset` / `none` / `cancel` 六个别名全部可用
- UI：footer 状态 chip（2.1.236）+ 实时 overlay panel（elapsed/turns/tokens）
- `--resume` 恢复会话时恢复 active goal（2.1.239）

**实现机制**【文档+推断】：每轮结束后 evaluator 判定完成条件（2.1.143 曾修"evaluator 在后台 shell/subagent 仍运行时误触发"，推断为 hook 类机制）；goal 死于不可恢复错误（auth 失效、credit 耗尽、context 溢出）时自动清除并提示（2.1.234）；**check-in 退避**：goal 被长时后台任务阻塞时，空闲 30 分钟后自动 check-in，之后 30min→1h→2h 退避，每个 goal 最多 3 次 check-in，用户下一条消息重置额度（2.1.236/239/246），`CLAUDE_CODE_GOAL_CHECKIN_MINUTES=0` 可关闭。社区实证：有用户用 4 个链式 `/goal` 跑了 9h27m/45 commits【二手 Reddit】；长任务存在 context rot 批评【二手 YouTube】。

### 1.2 OpenAI Codex CLI（开源 Rust）——唯一完整可读的参考实现

**功能定义**【文档】：官方 Cookbook："Goals are persistent objectives in Codex that keep a thread working toward a defined outcome across turns"。**thread 级持久化状态**，非全局 memory、非项目指令。

**数据模型**【源码 `codex-rs/state/src/model/thread_goal.rs`】：

```rust
ThreadGoal { thread_id, goal_id, objective, status,
             token_budget: Option<i64>, tokens_used, time_used_seconds,
             created_at, updated_at }
// status: active | paused | blocked | usage_limited | budget_limited | complete
// 终态 = complete | budget_limited
```

存储为 `$CODEX_HOME/goals_1.sqlite`（SQLite 表）【源码 `codex-rs/state/src/sqlite.rs`】。

**UX**【源码 `codex-rs/tui/src/chatwidget/goal_menu.rs`】：`/goal <outcome>` 设置、`/goal` 查看、`/goal edit`、`/goal pause`、`/goal resume`、`/goal clear`；状态栏按状态显示可用命令；状态标签把 blocked 显示为 "stalled"、budget_limited 显示为 "limited by budget"（面向用户去术语化）；Summary 面板显示 Status/Objective/Time used/Tokens used/Token budget。

**模型侧工具契约**【源码 `codex-rs/ext/goal/src/spec.rs`】：3 个 Responses API tools——`get_goal` / `create_goal` / `update_goal`：
- `create_goal`："Create a goal only when explicitly requested by the user...do not infer goals from ordinary tasks"；已有未完成 goal 时失败
- `update_goal`：**模型只能设 `complete` 或 `blocked`**；blocked 要求"同一阻塞条件连续至少 3 个 goal turn"才可标记；明确禁止"因为预算快用完就标记 complete"；pause/resume/budget 归用户/系统所有

**上下文注入**【源码 `codex-rs/ext/goal/src/steering.rs`】：continuation prompt 作为带 `InternalContextSource("goal")` 标记的 user-turn fragment 注入（**不是 system prompt**），只在安全边界（thread idle、无排队输入、无 pending work）触发；三套模板（continuation / budget_limit / objective_updated），变量含 XML 转义的 objective、tokens_used、token_budget、remaining_tokens。

**continuation 模板的防漂移提示词工程**【源码 `templates/goals/continuation.md` 全文已核】——这是全部竞品中最详尽的：
1. objective 显式声明为"user-provided data，非高优先级指令"（**防 prompt injection**）
2. **no-progress check**：每轮把上一轮分类为 progress / verified wait / no progress，仅凭 conversation/intent/锁文件不算 verified wait
3. **completion audit**："treat completion as unproven"，逐条需求找权威证据，"The audit must prove completion, not merely fail to find obvious remaining work"
4. **fidelity**（反 goal-shrinking）："Do not substitute a narrower, safer, smaller...solution"
5. 要求完成时调用 `update_goal` 保留用量记账

**运行时防护**【源码 `codex-rs/ext/goal/src/runtime.rs` + Cookbook】：token/时间记账（GoalAccountingState）；中断自动转 paused，resume 恢复；**anti-spin**：continuation 轮若无工具调用则抑制下一次自动续跑；plan-only 轮不触发续跑；`GoalExtensionConfig { enabled }` 可热切换。

### 1.3 生态佐证

- **pi-codex-goal**（pi 生态包）【文档】：把 Codex goal 契约完整移植到 pi（`/goal` 五个子命令 + 3 工具），存储改用 pi 的 session custom entries——"follows session history, resume, fork, tree navigation, reload, and compaction behavior without an external database"。证明 Codex 契约已是可复用模式，且**会话内嵌存储**是 SQLite 之外被验证的第二条路。
- **Cursor**【文档】：官方明确拒绝 first-class Goal Mode（forum #160374，2026-05），指向 Skills/Plan Mode/Cloud Agents/Automations/`/loop` skill 组合。社区精华论断："**Plans help the agent know what to do; Goal Mode would define when the agent is allowed to stop.**"——这句话精确划清了 plan 层与 goal 层的边界。
- **OpenCode**【文档】：`/goal` 功能请求被官方关闭（not_planned，issue #31762），但该 issue 本身是优秀设计参考：evaluator 用可配置小/快模型、仅从 transcript 判定、返回 yes/no + 短理由、状态存 session DB、明确 "Independent from todos: Goal is a session-level objective"。
- **Amp**【文档】：无 goal，用 Handoff（`/handoff`）把"持久目标"转化为"目标的序列化重建"——跨线程序列化路线。
- **Gemini CLI**【源码】：无 `/goal`，走任务图路线（`write_todos` 工具 + 实验性 `tracker_*` DAG 六工具套件 + plan 文件"single source of truth"强注入）；官方教程直言动机："Standard LLMs have a limited context window and can 'forget' the original goal after 10 turns"；**compaction snapshot 指令强制保留 plan 路径 + 每步状态 + 活动约束**——针对压缩丢目标的显式对策。
- **Aider**【文档】：完全无 goal/todo/plan，只有 CONVENTIONS.md。

---

## 2. IDE 工具线与框架线：goal 的操作化方式

### 2.1 IDE 产品线速览

| 产品 | 字面 goal 命令 | 对应机制 | 存储 | 注入方式 | 完成判定 | 防漂移 |
|---|---|---|---|---|---|---|
| Cline | ❌ | Focus Chain md 清单 | `{taskDir}/focus_chain_taskid_{id}.md`【源码】 | 每 6 条消息 re-sync + 压缩后存活【文档】 | 清单全勾（隐式） | 周期提醒 |
| Roo Code | ❌ | `update_todo_list` 工具 | 内存（Task 对象）【源码】 | 编入 system prompt【源码】 | 清单全勾（隐式） | 校验失败纠偏 + **审批时用户可编辑清单并回传 diff**【源码】 |
| Windsurf | ❌ | Planning Mode | `~/.windsurf/plans` md【文档】 | 持续回读计划文件【文档】 | 无 | 社区 workaround |
| Devin | ❌ | Interactive Planning + Playbooks + Knowledge | 会话对象（闭源）【文档】 | 计划即会话目标 | 无 | 前置审批门（30s/等批准） |
| Goose | ❌（有 `/plan`） | `/plan` + 清历史执行 | 会话目录【文档】 | 清空历史后计划作为首条上下文 | 无 | **planner/executor 异构模型**（`GOOSE_PLANNER_*`） |
| OpenHands | ❌ | TaskTracker 工具 | `TASKS.json` 可跨会话【源码】 | tool result 回流 | 无 | view-before-plan 规则 |
| Warp | ❌（有 `/plan`） | 计划版本化 + mid-run 热编辑 | Warp Drive【文档】 | 计划随上下文【推断】 | 无 | 计划可回滚 |
| Copilot | ❌（有 `/plan`） | Plan agent + todo 工具 | session plan.md，**不跨会话**【文档】 | prompt 重注入【推断】 | 无 | 无公开机制 |

### 2.2 框架线：goal 建模的三代演进

1. **AutoGPT/BabyAGI（2023，反面教材）**：`ai_goals` 静态注入、运行期不可变、无完成判据、无循环检测 → 必然漂移/死循环/成本失控【多方文档】。改目标需手改 yaml 重启。
2. **Magentic-One（微软，学术化最完整）**【源码】：双账本——Task Ledger（事实四节：GIVEN OR VERIFIED FACTS / FACTS TO LOOK UP / FACTS TO DERIVE / EDUCATED GUESSES + 计划）+ Progress Ledger（每步强制 JSON 自省：`is_request_satisfied` / `is_in_loop` / `is_progress_being_made` / `next_speaker` / `instruction_or_question`）。**stall 计数器**：坏步 +1、好步 -1（下限 0），达 `max_stalls` 触发"根因分析 → 重写事实（猜测升级为已验证）→ 重规划"。状态可序列化恢复。唯一同时解决完成判定、循环检测、防漂移、重规划四问题的公开设计。
3. **LangChain TodoListMiddleware（2025-2026 主力形态）**【源码】：`write_todos` 整表替换 + 每轮最多一次；完成纪律全靠工具 description 里的 prompt 规则——"IMMEDIATELY 标记完成（不许批量打勾）"、"有未解决错误/阻塞时绝不许标 completed"、"**最终答案必须出现在最后一次 write_todos 之后的消息里**"（堵"打勾即收工"假完成）。

---

## 3. 横向综合：六个设计维度的行业收敛

| 维度 | 收敛结论 | 分歧点 |
|---|---|---|
| **数据模型** | `{objective, status 枚举, 用量记账, timestamps}`；session/thread 作用域 | 结构化 DB（Codex SQLite）vs 会话内嵌（pi entries、Claude 未公开）vs 文件（Cline md） |
| **生命周期** | Active→(Paused↔)→Complete；budget/blocked 为受控分支 | Claude Code 3 态简洁 vs Codex 6 态精细 |
| **注入策略** | 每轮可见是底线 | system prompt 块（Roo/Shannon /focus 式）vs 带来源标记的 user fragment（Codex）vs 账本广播（Magentic-One） |
| **完成判定** | 显式条件 + 审计纪律；"完成必须被证明而非假设" | 工具上报（Codex 3 连轮 blocked 阈值）vs 关键词/marker（ralph 式）vs 独立 evaluator（OpenCode 提案） |
| **防失控** | 迭代/预算上限 + anti-spin + 中断转暂停 | check-in 退避（Claude）vs stall 计数重规划（Magentic-One） |
| **持久化** | 跨会话恢复 = 落盘（sidecar/文件/DB）；compaction 存活 = 每查询重注入 | markdown 人可共编 vs JSON/DB 可靠解析 |

**给 Shannon 的四条最重要启示**：
1. goal ≠ 计划 ≠ todo：goal 是**终止条件层**，与 Shannon 已有的 `/plan`（计划层）、TodoWrite/Task（执行层）正交互补，不应重复造清单。
2. 完成判定是整个功能的工程核心——ralph 式关键词 substring grep 是已被行业超越的判据（消息里提一句 "DONE" 就误停）。
3. 续跑提示词的防漂移纪律（completion audit / fidelity / no-progress 分类）是 Codex 已验证可直接移植的资产。
4. 防失控四件套（迭代上限、anti-spin、中断即停、用户可随时 pause/clear）是 MVP 必须有的安全网。

---

## 4. Shannon 现状盘点与差距分析

### 4.1 现状：goal 完全缺失，但地基齐全

**已核实（逐文件验证）**：
- ❌ 无 `/goal` 命令、无 goal 状态类型、无 goal 持久化（全 crates 大小写不敏感 grep）
- ✅ 预留常量：`SOURCE_GOAL_RESUME = "goal.resume"`（`crates/shannon-types/src/session_event.rs:262-263`，"Content injected to resume a goal"，**从未被引用**——功能早有预留但从未落地）
- ✅ `/focus` 全链路可克隆：`repl.state.focus_area`（state.rs:262）→ query.rs:297-299 同步 → `set_focus_area`（engine.rs:748）→ `process_query` 组装 `SystemContentBlock::text` 非缓存块（engine.rs:1370-1379）
- ✅ 命令注册三件套模式（`/remote` 为参照）：`repl_only_commands` 门（commands/mod.rs:444 附近）+ match 分发臂 + `categorize_commands` 帮助条目（help.rs:1053 `/focus` 为模板）+ 10 locale 文件 i18n key
- ✅ 持久化挂点：`SessionSidecar`（`session_store.rs:58`，meta.json，serde default 字段可直接扩展）；resume/auto-restore 链路完整（session.rs:13,145）
- ✅ 续跑挂点：query.rs:1502-1510 查询完成后调 `check_loop_iteration` / `check_ralph_iteration`（`set_input` + `submit_input` 续跑模式）；`RalphState`（state.rs:349）是完成循环的现有原型
- ✅ UI 挂点：远程目标 pill（status_bar.rs:239-251，VS Code 式指示器）是 goal pill 的一比一模板
- ✅ headless 挂点：`--prompt` 管道 + `QueryEngineConfig` 直接可用（CLI `--goal` flag 零障碍）
- ✅ compaction 天然存活：goal 走 `process_query` 每查询重组装（与 `/focus` 同路），压缩后自动重新注入；`ContextInjector::reinjection_context`（context_injector.rs:88）可作 belt-and-braces

### 4.2 现有近邻功能的不足（问题清单）

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| P1 | `/ralph` 完成判据是最后一条消息的 substring 大写匹配，消息正文提及 "DONE"/"COMPLETE" 即误停 | loop_engine.rs:285-291 `msg.contains(&kw.to_uppercase())` | 误判率高，无完成审计；与 Codex "完成必须被证明" 的行业纪律差距最大 |
| P2 | `/ralph`、`/loop` 状态不持久化，会话恢复即丢失 | RalphState/LoopState 均不在 SessionSidecar | 与 Claude Code "resume 恢复 active goal" 差距 |
| P3 | `/focus` 只注入一行 focus 指令，无目标语义、无生命周期、无 UI 可见性 | engine.rs:1370-1379 | 用户无法知道当前锚定的关注点，跨会话即丢 |
| P4 | goal 类状态无进度记账（迭代数/耗时），失控时用户无感知 | ralph 只有 iteration 计数，无面板 | Codex/Claude 均有 elapsed/turns/tokens 面板 |
| P5 | 无 compaction 后的目标重锚定语义 | Gemini 有显式对策（§1.3）；Shannon `/plan` 内容压缩后依赖模型记忆 | 长任务压缩后漂移风险 |
| P6 | `SOURCE_GOAL_RESUME` 预留常量悬空 | session_event.rs:263 | 事件模型层面未闭环 |

### 4.3 改进建议（与设计方案的映射）

1. **新增 `/goal` 命令 + GoalState + 状态机**（解决 P1/P3 的目标语义缺失）：`/goal <objective>`、`/goal` 查看、`/goal pause|resume|clear|status`，对齐 Claude Code/Codex 命令面。
2. **严格完成契约替换关键词 grep**：续跑提示词要求模型完成后在**最后一行**输出精确 marker（`GOAL_COMPLETE` / `GOAL_BLOCKED`），REPL 按"最后一个非空行全等"判定（解决 P1）；工具契约（`get_goal`/`update_goal`）作为 Phase 2。
3. **续跑提示词移植 Codex 防漂移纪律**：objective 是用户数据非指令（防注入）、completion audit、fidelity（反 goal-shrinking）、no-progress 分类（解决 P1 的审计缺失）。
4. **SessionSidecar 持久化 + resume 恢复**（解决 P2）。
5. **状态 pill + 完成通知**（解决 P4 的可见性；elapsed/tokens 面板 Phase 2）。
6. **防失控四件套**：`--max` 迭代上限（默认 25）、中断即停（走 Err 分支天然成立）、`/goal pause`、blocked marker 两连即暂停。anti-spin（无工具调用轮检测）Phase 2。
7. **headless `--goal`**：注入-only。
8. **`/ralph` 保留不动**（原型友好共存；`/goal` 与 ralph/loop 互斥防双重续跑循环）。

---

## 5. 结论

`/goal` 是 2026 年已被 Claude Code 与 Codex CLI 验证的一等公民范式，本质是**终止条件的对象化**：session 级状态机 + 每轮上下文注入 + 显式完成审计 + 防失控护栏。Shannon 的代码地基（`/focus` 注入链路、sidecar、pill、续跑挂钩、`SOURCE_GOAL_RESUME` 预留）使 MVP 能以小触及实现对标功能；真正的设计重心应放在**完成契约与防漂移纪律**上——这是竞品拉开差距的地方，也是 ralph 原型的短板。

设计方案见 [2026-09-04-goal-design.md](../plans/2026-09-04-goal-design.md)，实施方案见 [2026-09-04-goal-implementation.md](../plans/2026-09-04-goal-implementation.md)。

---

## 附：主要来源

**Claude Code**：https://code.claude.com/docs/en/commands ｜ https://code.claude.com/docs/en/goal ｜ https://github.com/anthropics/claude-code (CHANGELOG.md) ｜ https://explainx.ai/blog/claude-code-goal-command-long-running-agents-2026 ｜ https://www.reddit.com/r/ClaudeCode/comments/1tmm4sd/
**Codex**：https://developers.openai.com/cookbook/examples/codex/using_goals_in_codex ｜ https://github.com/openai/codex (`codex-rs/ext/goal/`, `codex-rs/state/src/model/thread_goal.rs`, `codex-rs/tui/src/chatwidget/goal_menu.rs`)
**Gemini CLI**：https://github.com/google-gemini/gemini-cli (`write-todos.ts`, `snippets.ts`, `docs/tools/tracker.md`)
**Cursor**：https://cursor.com/docs/agent/plan-mode ｜ https://forum.cursor.com/t/add-autonomous-goal-mode-similar-to-claude-code-s-goal/160374
**OpenCode**：https://github.com/anomalyco/opencode/issues/31762
**Amp**：https://ampcode.com/manual ｜ https://ampcode.com/news/handoff
**Aider**：https://aider.chat/docs/usage/conventions.html
**Cline**：https://cline.bot/blog/focus-attention-isnt-enough ｜ https://github.com/cline/cline (`FocusChainSettings.ts`, `file-utils.ts`)
**Roo Code**：https://github.com/RooCodeInc/Roo-Code (`UpdateTodoListTool.ts`, `system.ts`)
**Windsurf**：https://devin.ai/blog/windsurf-wave-10-planning-mode
**Devin**：https://docs.devin.ai/work-with-devin/interactive-planning
**Goose**：https://github.com/block/goose (`environment-variables.md` Planning Mode Configuration)
**OpenHands**：https://github.com/OpenHands/software-agent-sdk (`task_tracker/definition.py`, issue #2335)
**Warp**：https://docs.warp.dev/agents/capabilities/planning/
**Copilot**：https://code.visualstudio.com/updates/v1_103 ｜ https://code.visualstudio.com/docs/agents/run/planning
**Magentic-One**：https://github.com/microsoft/autogen (`_magentic_one_orchestrator.py`, `_prompts.py`) ｜ https://arxiv.org/abs/2411.04468
**LangChain**：https://github.com/langchain-ai/langchain (`middleware/todo.py`) ｜ https://github.com/langchain-ai/deepagents
**AutoGPT/BabyAGI**：https://maartengrootendorst.com/blog/autogpt/ ｜ https://www.ibm.com/think/topics/babyagi
**pi 生态**：https://pi.dev/packages/pi-codex-goal
