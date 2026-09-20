# Shannon Harness 架构审查报告（2026-09）

- **分支**: `review/harness-audit-2026-09`（基于 dev @ 04f7bdee）
- **审查人视角**: 高级 AI 架构师 / 工程师
- **审查范围**: 核心 harness 架构（agent loop、上下文管理、提示词、核心工具、权限/沙箱、memory、多 agent、skills/MCP/hooks/插件、会话/事件溯源/rewind/配置）
- **方法**: 6 路并行深读代码（每条结论附 `file:line` 证据）+ 竞品 harness 机制调研（Claude Code、Codex、Gemini CLI、Cursor、Cline/Roo、OpenHands、Aider、Amp、OpenCode/Crush、Goose、pi）+ 对最高严重度结论的人工复核
- **关联文档**: 本报告聚焦 **harness 内部**，与 [docs/improvement-plan-2026-09.md](./improvement-plan-2026-09.md)（产品侧，已评审）互补，基本不重叠。

---

## TL;DR

**总体判断：工程地基扎实，但当前 harness 存在三类系统性问题，其中若干是正确性级 P0。**

做得好的地方是真好的：L0 事件日志（flock 单写者、崩溃尾部修复、边界 fdatasync）、`AbortOnDropStream` 取消语义、tool_use_id 去重、分层恢复梯队（截断续写/think-only 提醒/溢出重试，均带 eval 依据）、守卫链（权限门 + PreToolUse hook，全程审计落盘）、Edit 工具质量（唯一性校验 + git 三方合并回退）、MCP 延迟 schema（`mcp__tool_search`，Claude Code 反而没有）、skills 渐进披露带 token 预算、secret-guard 字节稳定代理设计。这些达到或超过 Claude Code 的对应设计。

三类系统性问题：

1. **正确性 P0（会直接坏）**：上下文压缩死循环（默认配置下长会话挂死）、WebFetch/WebSearch 抓到的内容从不进入模型（两大核心工具实际失效）、引擎路径无任何工具输出截断（一条 `cat` 大文件即冲爆上下文）、合成 tool_use_id 会被 Anthropic 400、截断可切断 tool_use/tool_result 配对、缓存断点超限（最多 11+2 个 vs Anthropic 上限 4）。
2. **「幽灵能力」（最独特的系统性风险）**：大量子系统已实现、有文档、有测试，但**从未接入主循环**——`context_budget.rs`、`context_pressure.rs`、`protection.rs`、`micro_compact`、`tool_orchestration.rs`、`ToolExecutionService`（40K 输出截断在这里，恰是死代码）、`preference_memory` 写路径、`LlmMemoryExtractor`、`auto_dream_consolidation`、tmux、`IsolatedContext`。README 宣称的「skill 注入扫描与签名验证」不存在。**对外叙事与运行时行为不符，对开源项目是可信度级风险。**
3. **重复分裂**：指令加载 3 套（CLAUDE.md 可被注入 2–3 次）、memory 后端 3 套、agent 定义加载器 3 套（`.claude/agents` 的 `tools:` 限制在主路径被静默丢弃）、任务模型 3 套（TodoWrite/Task*/TeamTask*）、配置系统 2 套（TOML + JSON）、输出截断实现 3 处互不相同、agent.rs 8268 行混杂 11 种职责。每套单独看都不差，叠在一起互相打架。

与竞品差距最大的四个点：**上下文治理**（无工具结果微压缩、压缩摘要不含工具结果细节、重注入不含 memory 块）、**子代理上下文经济学**（结果 4000 字符硬截断、无深度/并发策略、无 MCP 工具、限值对模型不可见）、**memory 写路径质量**（关键词正则提取 + 全量重扫 + 会污染提示词的幻觉记忆）、**安全边界**（沙箱可被兄弟工具旁路、LLM 权限分类器可被提示注入、权限决策不持久化）。

建议按三波走：Wave 0 正确性止血（约 1 周，12 项 P0）；Wave 1 一致性收敛（2–4 周：合并重复系统、接通或删除死代码）；Wave 2 架构升级（1–2 月：engine 拆分、上下文治理重构、memory LLM 化、子代理经济学）。详见 §8。

---

## §1 P0 问题清单（Top 12，按严重度排序）

> 每条均已二次核验或给出直接代码证据。修复建议给到「最小正确修法」。

### P0-1 上下文压缩死循环（livelock）——默认配置下长会话挂死

- **证据**：`query_engine/engine.rs:2662` token-based 压缩分支把结果赋给局部 `messages`（:2624 `messages = compacted_vec`）后 `continue`；而循环顶部 ：2323 `let mut messages = conversation.messages.clone()` 每轮从 `conversation.messages` 重新克隆——**压缩结果被静默丢弃**。同时 `did_compact` 将 `compaction_failures` 清零（:2655），熔断器永不触发；`turn` 不推进。同路径的旧系统 `needs_compression` 需要 `max_context_tokens: Some`（`streaming.rs:99-107`），而默认是 `None`（`types.rs:761`），所以没有兜底。
- **影响**：任何长会话一旦估算超过 0.8 阈值，producer 任务无限发射 Progress 事件、不再调用 API——用户表现为「卡死」。这是默认路径。
- **修复**：`continue` 前 `conversation.messages = messages.clone()`（或去掉 `continue` 走统一同步点 ：2769）。补一条「压缩后估算必须下降」的回归测试。

### P0-2 WebFetch / WebSearch 抓取内容从不进入模型（工具功能性失效）

- **证据**：`shannon-tools/src/web.rs:251-264` — WebFetch 把页面内容只放 `metadata["content"]`，`content` 仅是 `"Successfully fetched N bytes from ..."`；WebSearch 同样（:629-639，结果在 `metadata["results"]`）。而引擎侧 `ToolResultEntry::to_tool_result_content`（`engine.rs:262-330`）**只转发 `content`**（metadata 仅用于图片识别）；全仓无任何代码读取 `metadata["content"]`。
- **影响**：模型调用 WebFetch 后只看到一句成功提示——联网调研能力实际为零。这是 agent 的两大信息入口。
- **修复**：把 payload 放进 `content`（摘要进 metadata），并给 `to_tool_result_content` 增加「metadata 渲染兜底」。补集成测试断言「抓取的正文出现在发给 provider 的 tool_result 中」。

### P0-3 引擎路径无任何工具输出截断

- **证据**：40K 字符上限只存在于 `ToolExecutionService::run_tool_use`（`tool_execution.rs:765-774`），而该服务**不被引擎调用**（仅测试引用）。引擎执行走 `ToolRegistry::execute_streaming`，结果原样入对话（`engine.rs:4045-4059 → 2342-2352`）。`BashTool` 把 stdout/stderr 全量拼接（`system.rs:1229-1236`），无上限。
- **影响**：一条 `cat` 多 MB 文件 / 噪音构建日志直接冲爆上下文，并连锁触发 P0-1。也直接拉高成本。竞品均设有硬上限（Claude Code 工具结果 ~25K token 上限）。
- **修复**：把截断（含 `truncated` 标记与提示语）下沉到 `ToolRegistry::execute/execute_streaming` 或引擎结果臂，一处统一生效。

### P0-4 合成 tool_use_id 会被 provider 拒绝

- **证据**：`engine.rs:4300-4305`（`"denial-warning"`）、`:5695-5700`（`"auto_test_iter_{n}"`）伪造 tool_use_id 的 tool_result，经 ：2341-2353 排空进入请求。Anthropic 对未知 `tool_use_id` 返回 400。代码在 ：2362-2370 已修过同类问题（checkpoint 排序），说明这是已知雷区。
- **修复**：运行时提示（拒绝警告、auto-test 结果）改用普通 user 文本消息，不伪装 tool_result。

### P0-5 截断可切断 tool_use/tool_result 配对 → provider 400

- **证据**：>1.0 预发送截断从前端删任意两条消息（`engine.rs:2880-2885`）；熔断/溢出回退 `split_off(len - keep)`（:2586、:5230）都可能在 assistant(tool_use) 与 user(tool_result) 之间下刀。库中已有配对感知的 `safe_split_point`（`shannon-engine/src/compact/compact_messages.rs:40-88`）与 `safe_tail_start`（`compact.rs:755-766`），**但主循环没有使用**。
- **修复**：三处截断统一走 `safe_split_point`。与 P0-3 一并纳入「单一截断工具函数」。

### P0-6 Anthropic 缓存断点超限（上限 4）

- **证据**：每个 system block 独立打 `cache_control`（`engine.rs:1873-1987`，最多 9 个 block + env block），adapter 再加 tool 定义与 last-user 两个断点（`adapter.rs:107-133`）。Anthropic 每请求上限 4 个断点。
- **影响**：全配置下请求报错或静默丢缓存（成本上升数倍）。
- **修复**：system 前缀是连续的——只在**最后一个稳定 block**打一个断点即可覆盖全部前缀；全局（含 adapter 的 2 个）控制在 ≤4。同时把查询相关的 smart-context 移出 cached 区（`engine.rs:1896-1910` 现在被标 cached，查询一变就炸缓存）。

### P0-7 子代理 max_turns 用错字段：`max_concurrent_tasks as u32`

- **证据**：`shannon-tools/src/agent.rs:381-384` — `max_turns: agent_def.map(|d| d.max_concurrent_tasks as u32)`。内建 `explorer` 的 `max_concurrent_tasks = 1`（`agent_defs.rs:470`）→ 子代理只被允许 1 轮。`AgentConfig.max_turns`（默认 50，`sub_agent.rs:64`）从未被执行路径使用。
- **修复**：`AgentDefinition` 增加 `max_turns`（skills 加载器格式里已有该字段，`agent_loader.rs:119-120`），停止复用并发字段。

### P0-8 进程模式 teammate 硬编码 `bypassPermissions`

- **证据**：`shannon-agents/src/coordinator.rs:663` — `shannon --team-agent` 子进程固定以 `permission_mode: Some("bypassPermissions")` 启动，既无视 `TeammateConfig.permission_mode`（`teammate.rs:40-44`），也推翻了 in-process 路径刚做的权限继承修复（`agent.rs:415-423`）。
- **影响**：团队模式 = 子代理拿到比父会话更大的权限，安全模型被架空。
- **修复**：传递父会话/配置中的 mode；默认继承。

### P0-9 REPL 的 `/rewind`、`/compact` 不写权威日志 → resume 复活已删内容

- **证据**：REPL 只改内存（`shannon-ui/src/repl/commands/session.rs:820-883`、`:924-1130`）；`truncate_to_turn`（`session_store.rs:574-639`）只有桌面在调（`desktop/src/commands_rewind.rs:215`）。反向问题：桌面 `/compact` 用 `rewrite_with_conversation` **破坏性重写** L0 日志（丢 tool 调用/结果明细，`session_store.rs:650-721`）。REPL `/rewind code` 还无确认直接覆写文件（`session.rs:810-818`）且绕过 provider 用裸 `std::fs::write`（:685-697），破坏远程会话语义。
- **修复**：REPL 路径调用 `truncate_to_turn`；压缩改为「非破坏性追加 summary + 投影期折叠」（参考 OpenHands：事件层无损，投影层有损）；`/rewind code` 加确认 + 走 `FileHistoryManager::restore()`。

### P0-10 Memory 自动提取会污染提示词（幻觉记忆）

- **证据**：REPL `auto_save_memory` 对助手说出 `"i'll remember"`、`"saved:"` 等客套话即抓取任意 >20 字符的行写成 Preference@0.8（`shannon-ui/src/repl/query.rs:1641-1656, 1727`）——"I'll remember that!" 每轮进入未来所有会话的系统提示词。引擎侧 AutoDream **每条 query 全量重扫整个对话**（`engine.rs:5573`），重复刷新 `accessed_at`（`store.rs:258`）让噪音记忆永远「新鲜」，跨轮改述产生姊妹条目；去重用未归一化的 Jaccard（大小写/标点敏感，`store.rs:69-86`），改述去重基本无效。`prune_to_token_budget` 用**永久删除数据**来满足注入预算（`store.rs:273-303`）。
- **修复**：删除或加直链的 auto_save_memory；提取只处理增量（cursor 机制已存在于 `extract_memories.rs:564-583`）；相似度归一化；预算过滤放在注入层而非删除数据。

### P0-11 沙箱可被兄弟执行工具旁路

- **证据**：`RunBackground`/`WaitForLog`/`KillBackground` 与 `ReplTool`/`PowerShell` 走**未装饰的** process world（`background.rs:216-238`），不经过 `SandboxExecutorRewrite` 也不做 `analyze_command_security`——被沙箱拦截的命令换个工具即可执行。PTY 路径在沙箱分支**之前**执行（`system.rs:1163-1180`），`use_pty: true` 可绕过 bwrap/seatbelt/docker 包装。无后端可用时 Bash 完全裸跑仅留 warn（`sandbox.rs:1115-1119`）。
- **修复**：所有执行类工具统一过同一个 sandbox rewrite + 安全面；PTY 与进程沙箱互斥；无后端时按配置选择拒绝或显著告知。

### P0-12 工具名大小写不匹配击穿只读自动批准

- **证据**：`is_read_only_tool_name` 只匹配小写（`permissions.rs:27-53`），而注册/模型可见名是 `"Read"/"Grep"/"Glob"/"WebFetch"`；`should_auto_approve` 的文件工具表同样（`permissions.rs:250-254`）。Suggest/Readonly/PlanReadonly 模式下 Read/Edit 全部错过快速通道落入分类器。同类：plan 模式写拦截用名字列表 `FILE_MODIFYING_TOOLS`（`tool_execution.rs:489-502`），漏掉 `NotebookEdit`、`GitStash`、`Worktree`、`Cron`——plan 模式下可执行写操作。
- **修复**：大小写不敏感匹配 + 单一工具名注册表；plan 模式拦截改用 `is_read_only()/is_destructive()` 派生。

---

## §2 核心循环与上下文管理

### 现状

主循环 `QueryEngine::process_query`（`engine.rs:1764` 起）→ producer 任务 + `AbortOnDropStream`；每轮：turn 上限检查（默认 20）→ 排空 tool_results → 合成注入（checkpoint 提醒/60%/80% 上下文提醒）→ 压缩 → 预发送溢出防护 → secret-guard → 流式调用 → 流状态机（~2000 行）→ 工具分派（权限门 → hooks → 并行/串行批处理）。恢复梯队（max_tokens 续写 ≤5 次、think-only 提醒 ≤2 次、usage 哨兵排水 2s）设计克制、有 eval 注释，是同类开源项目里少见的精细度。

### 问题（P0-1/3/4/5 之外）

| # | 问题 | 证据 | 说明 |
|---|------|------|------|
| C-1 | 三套截断策略并存、阈值三套（0.6/0.8/0.9/0.95 vs context_pressure 的 0.5/0.75/0.85/0.95 vs compact 的 0.8） | `engine.rs:2477-2579, 2783-2906, 5222-5401` | 同一件事（上下文治理）有三份实现，主循环用最 ad-hoc 的那份 |
| C-2 | `timeout_seconds` 配置从不生效 | `types.rs:675,758`、仅 `engine.rs:2084` 打日志 | 流挂死只能靠消费端 drop；无任何 loop/stream/tool 批级超时 |
| C-3 | Retry-After 是死代码：429 永远 `retry_after_secs: None` | `error.rs:89-93` vs `retry.rs:246-254` | 限速时无视服务器指示 |
| C-4 | 并行批处理丢 metadata | `engine.rs:4058` `metadata: Default::default()` vs 串行 `:4229-4231` | 只读工具（恰是并行批）的 image/stderr/exit_code 元数据全部丢失 |
| C-5 | 拒绝熔断把工具报错与权限拒绝混为一谈 | `:4041-4044` `batch_had_denial` 由任意 `is_error` 置位 | 3 次真拒绝后普通工具失败也会把计数钉死，5 次即中止会话 |
| C-6 | 压缩触发不计工具 schema token | `:2461-2471` 只算 messages+system | 大 MCP 工具集下会在触发前撞硬窗口 |
| C-7 | `system_blocks_opt` 被小上下文 Ollama 置空后跨轮不恢复 | `:2449-2450, 2850-2868` | 后续轮次全部裸奔无系统提示 |
| C-8 | 会话克隆/恢复无锁，双查询 last-writer-wins | `:2047` clone + `restore_messages` :1519 | 中止的查询丢失全部历史 |
| C-9 | 模型路由是纯关键词启发 | `:877-894` | fast/plan/primary 选择与语义无关；至少应承认这是占位符并暴露指标 |

### 建议

1. **单一 ContextGovernor**：把 warn→compact→strip→truncate 收敛为一个组件、一套阈值、一个配对感知的截断函数；顺手接通 `context_budget/context_pressure/protection/micro_compact` 或删除（见 §9「接通或删除」清单）。
2. **引入工具结果微压缩**（对齐 Claude Code 的 tool-result clearing / partial compaction）：压缩时先用已有的 `prune_stale_tool_results`（把旧工具结果缩成 200 字符预览，错误结果保留全文——这个策略本身很好），而不是只在完整 `/compact` 里用。
3. **压缩摘要提示词修复**：`compact/types.rs:318` 要求保留「文件路径与代码引用」，但 `:337` 把每条消息截到 500 字符——恰好摧毁这些内容。工具结果类消息应豁免或提高上限；参考 Claude Code 的结构化摘要段（目标/决策/文件/错误/待办/全部用户消息）。
4. **重注入缺口**：压缩边界重注入（`context_injector.rs:88-113`）不含 `Project Memories` 块——应加入 `format_for_injection()`。
5. 全局 loop/stream/tool 超时落地 `timeout_seconds`。

---

## §3 提示词体系

### 现状

无单一 prompt builder：静态 base（`types.rs:765-796`，~680 token）+ REPL/CLI 追加 + 每查询 9 个注入位的 system blocks（`engine.rs:1861-2044`）。典型稳态 4–8K token（不含工具 schema）。i18n 只覆盖 UI 文案，提示词纯英文（无质量风险）。

### 问题

| # | 问题 | 证据 |
|---|------|------|
| P-1 | **CLAUDE.md 被注入 2–3 次**：REPL `ProjectMemoryManager::load_merged` 追加 + 引擎 `load_full_context` 自动注入 + ContextInjector 再来一遍；CLI 同样双份 | `repl/mod.rs:1020-1066` + `engine.rs:1914-1931` + `context_injector.rs:120-163`；`cli/main.rs:1704` |
| P-2 | Git status 在 cached 前缀内：agent 每改一个文件缓存全炸 | `project_instructions.rs:698-702` + `repl/mod.rs:1053-1056` |
| P-3 | 提示词与运行时矛盾：宣称 markdown ```bash 「无法执行、轮次会卡住」，但 `markdown_tool_fallback` 默认 true 且真的执行 | `types.rs:780` vs `types.rs:744-750` |
| P-4 | 环境块缺日期/OS/shell——只有 cwd + 沙箱自述；「用用户语言回复」是唯一本地化线索 | `engine.rs:2025-2044` |
| P-5 | 系统 prompt 无 plan-mode 感知、无子代理委派指引、无任务管理指引（模型面对 TodoWrite/Task*/TeamTask* 三套工具无从选择） | 全文；任务工具注册见 `todo.rs:254,393`、`task.rs:188`、`lib.rs:669-672` |
| P-6 | 工具描述严重欠规格：Read/Write/Edit 各一句；无「先读后改」、old_string 唯一性规则、行数限制说明；路径语义描述互相矛盾（Read 说支持相对路径，Write/Edit 声称仅绝对路径，实际都解析相对路径） | `file/mod.rs:127,170,234,291,338,402`；`system.rs:956` |
| P-7 | team_prompt 教模型调用不存在的参数（`Agent({prompt, team_name, name, subagent_type})` vs 实际 schema `operation/agent_type/task/...`） | `team_prompt.rs:56-63` vs `agent.rs:1006-1078` |
| P-8 | 三个互相矛盾的 commit 提示词模板 | `skill.rs:96-114` / `bundled.rs:82-135` / `builtin/commit.rs:79-114` |
| P-9 | 指令文件注入无大小上限（CLAUDE.md 多大进多大；对比 skills 列表有 2000 token 预算、Claude Code 建议 <200 行） | `project_instructions.rs:704-721` |
| P-10 | watcher 缓存冻结 git context | `project_instructions.rs:1027-1033` |

### 建议

1. **单一指令加载器**：引擎自动注入与宿主追加二选一（建议引擎侧统一），REPL 的 `ProjectMemoryManager` 追加与 ContextInjector 的重复块删除。
2. **环境块补齐**（放在所有断点之后，零缓存成本）：日期、OS、平台、shell、git 摘要移到这里。
3. **系统提示词「右海拔」改写**：加 plan-mode 条件块（引擎已持有 flag，`engine.rs:1350-1363`）、子代理使用时机与成本指引、任务管理一节点名唯一规范工具、前端/简洁性/质疑用户等语气指引（对标 Claude Code 的 tone/doing-tasks 段）。
4. **工具描述对齐 Anthropic「工具即 HCI」标准**：每个工具写清行为约束、默认值、截断语义、失败样例；统一路径语义表述。这可能是**单点性价比最高**的改进——Shannon 的工具描述密度显著低于 Claude Code，而后者把更多功夫花在工具而非提示词上。
5. 缓存策略：系统前缀只在最后一个稳定 block 打一个断点（见 P0-6）。

---

## §4 核心工具

### 清单对比（vs Claude Code）

55 个已注册工具。**缺**：真正的 ripgrep Grep（现实现为 regex 全文件读入内存，无 multiline/字面量快速路径/上下文行号，`grep.rs:144-258`）、Read 的 PDF 支持、Read 行号输出（Claude Code `cat -n` 格式便于模型引用）、Bash 持久会话与 per-call 后台、WebFetch 的「prompt 摘要」步骤。**多**：PowerShell/REPL/7 个 LSP 工具/浏览器族/ComputerUse/AppleScript/cron/worktree/消息族——广度是差异化，但见 §4.2 的沙箱旁路问题。

### 问题（P0-2/3/11/12 之外）

| # | 问题 | 证据 |
|---|------|------|
| T-1 | MultiEdit 非原子：逐文件直接 `write_bytes`，中途 IO 失败留下半成品（错误信息自己承认） | `multiedit.rs:144-151`；应复用 Write 的 temp+rename（`write.rs:64-78`） |
| T-2 | Bash 安全分类器是漏洞 blocklist：`rm -fr /` 不在 Critical 表；`echo ls && rm x` 被只读子串逻辑误判只读；与权限系统另一套危险模式表并存 | `system.rs:115-131, 240-255, 424-433` vs `permission_classifier.rs:297-398` |
| T-3 | WebFetch DNS rebinding：validate 校验的是字面 IP/主机名，随后 `client.get` 独立解析 DNS；IPv6 私有段未封 | `web.rs:82-155, 196` |
| T-4 | 只读结果缓存（5min TTL）可对外部进程修改提供陈旧读；失效只挂在自己 Edit/Write 上 | `tools.rs:24, 520-583, 727-736` |
| T-5 | bwrap 绑定路径与 `/workspace` 回显别名互相矛盾（string 版绑 /workspace，Command 版绑原路径，而回显别名对任何 bwrap/docker 检测都开启） | `sandbox.rs:310-313` vs `:1242-1243` vs `shannon-tools/src/lib.rs:299-305` |
| T-6 | TodoWrite 依赖边 `blocked_by`+`active_form` 比 Claude Code 强，但无 todo/write 事件（见 S-12）且系统提示词不点名 | `todo.rs:31-60` |

### 建议

Grep 换 `ripgrep` crate（性能 + 功能一次到位）；Read 输出加行号、加 PDF 分页；WebFetch 解析后连接已验证 IP（自定义 resolver）+ 封 IPv6 私有段；MultiEdit 原子化；工具描述按 §3 建议 4 全面重写。

---

## §5 权限与沙箱

### 问题（P0-8/11/12 之外）

| # | 问题 | 证据 |
|---|------|------|
| S-1 | **LLM 权限分类器可被提示注入**：原始工具输入（≤500 字符）无定界符直接内插进分类提示词；LLM 正面结论把 tier 强制抬到 Allow（Ask 亦映射 Allow）——网页内容/命令回显可翻转安全裁决 | `llm_classifier.rs:158-178, 130-143` |
| S-2 | 权限决策不持久化：`AlwaysAllow` 是进程内 HashMap，重启即忘（Claude Code 有 settings.local.json 持久化） | `permissions.rs:816-967` |
| S-3 | hook 错误/超时 fail-open（超时→Err→Allow）；`McpTool`/`Agent` 两种 hook 类型实际只是跑命令字符串，名不符实 | `guard_nodes.rs:273-286`、`manager.rs:315-318, 189-199` |
| S-4 | Strict profile「拒绝」Write/Bash 实际只是强制弹窗（标 destructive） | `permission_profile.rs:51-56` + `permissions.rs:1740-1743` |
| S-5 | `Bash(git)` 子串匹配语义：任何包含 "git" 的命令都被匹配 | `permissions.rs:562-573` |
| S-6 | MCP header `Command` 数据源可跑任意简单命令（如 `cat ~/.aws/credentials`）作为「header」 | `config.rs:100-128` |

### 建议

LLM 分类器：输入 JSON-escape + 定界；**LLM 裁决只作建议、永不把 Ask 降为 Allow**（fail-closed）。决策持久化到 `.shannon/settings.local.json`（项目作用域）。hook 超时策略可配置 fail-closed。权限模式、规则、沙箱自述三者的语义边界写进文档（现在 approval mode 9 种 + profile + per-tool policy 三层叠加，用户无法推理出「到底会不会问我」——这正是 Claude Code 用五种模式 + 简单规则解决的事）。

---

## §6 Memory

### 现状

ADR-0010 选型「curated JSONL + 全量注入、无检索」是自洽的（≤50 条时合理），多写者正确性（append-only + flock + tombstone reconcile）做得好，桌面有完整管理 UI。但写路径与周边分裂严重。

### 问题（P0-10 之外）

| # | 问题 | 证据 |
|---|------|------|
| M-1 | 模型**没有任何 memory 工具**——不能自主 save/recall/forget（Claude Code/Letta 的模型可自编辑记忆）；`extract_memories.rs` 里等待 memory 工具的 guard 是给不存在的工具写的 | grep 全仓无 memory 工具；`extract_memories.rs:532-559` |
| M-2 | 三个项目键不一致：引擎用 `env::current_dir()`、REPL 用 `state.working_directory`、桌面接受任意字符串——worktree/多窗口下记忆碎片化或串线 | `engine.rs:1850,5579` vs `repl/commands/memory.rs:42` vs `desktop/commands_memory.rs` |
| M-3 | `DefaultHasher` 跨 Rust 版本不稳定：工具链升级 = 全部项目记忆文件变孤儿 | `store.rs:13-20` |
| M-4 | `load()` 不清 `entries`：desktop `refresh_shared_store` 会复活别的进程已删除的条目，且清空本地 tombstone | `store.rs:493-553` + `commands_memory.rs:66-72` |
| M-5 | 三个「preference」概念（JSONL category / preferences.md / AutoDream 关键词）+ 双写 `.md` 镜像从不回读 + 第三个不兼容的 `/memory` 实现（`builtin/memory.rs` 写 `~/.shannon/memory/*.json` 另一种 schema） | `project_memory.rs:718-765`、`builtin/memory.rs:107-129` |
| M-6 | `preference_memory` 写路径死代码（注入永远为空的死重）；`LlmMemoryExtractor`（含完整提取提示词）零生产调用方 | `context_injector.rs:75-80`、`extract_memories.rs:783-978` |
| M-7 | 后台提取在 tokio spawn 里做阻塞 IO + 持 std RwLock；且 `save()` 每条 query 无条件全量重写 | `engine.rs:5577-5591`、`auto_dream.rs:431-442` |
| M-8 | 团队记忆同步休眠（`enabled=false`）且 mtime 同步不传播删除 | `team_memory_sync.rs:325-334, 420-453` |

### 建议（对标竞品）

1. **给模型 memory 工具**（`memory_save/memory_forget/memory_list`），走 `SecretScanner` 门禁——这是从「关键词摘抄」升级为「agent 自主策展」的前提（Letta/Claude Code 模式）。
2. **提取 LLM 化 + 增量化**：用已写好但未接线的 `LlmMemoryExtractor`（或轻量模型）替换关键词正则；只处理 cursor 增量；产出门控（置信度阈值 + 可选用户确认，对标 Cursor memories 的 review 流和 Gemini auto-memory 的 patch 提案流）。
3. **注入预算化 + 排序**：`format_for_injection` 按 relevance 排序、token 预算内截断；memory block 移到缓存断点末端减少缓存炸裂。
4. 统一项目键为 canonical path + 稳定哈希（blake3），带迁移；修 M-4 僵尸复活；删除或接线全部死代码（M-6）与第三后端（M-5）。
5. AutoDream 名字起得很好（与 Claude Code 内部 dream 概念呼应），值得把「整会话睡眠整理」做成真正的会话结束时批量任务而非每 query 全量重扫。

---

## §7 多 Agent / Teams / Skills / MCP / Hooks

### 问题（P0-7/8 之外）

| # | 问题 | 证据 |
|---|------|------|
| A-1 | **三个 agent 定义加载器并存且能力不一**：主路径 `agent_defs.rs` 的手写 frontmatter 解析器**不支持 `tools:`**——`.claude/agents/reviewer.md` 里的工具限制被静默丢弃（全工具放行）；`shannon-skills/agent_loader.rs` 支持完整 Claude schema 但几乎无人消费；`custom_agent.rs` 第三个 | `agent_defs.rs:92-162, 292-325` vs `agent_loader.rs:108-159` vs `custom_agent.rs:119+` |
| A-2 | 无上下文 spawn 回退撒谎：无 TeamContext 时返回 "Agent spawned (no execution context)" status=initialized——模型以为 agent 跑了 | `agent.rs:477-487` |
| A-3 | 子代理限值不可见且互相矛盾：10 工具调用上限、4096 输出 token、4000 字符结果硬截断——均未写进工具描述/系统提示词；`max_turns`（默认 50）不生效 | `agent.rs:577, 597-600, 633-641` |
| A-4 | 子代理拿不到 MCP/skills/plugin 工具与 hooks（Claude Code 子代理可以） | `agent.rs:273-325` 只注册内建 |
| A-5 | 广播消息用 Debug 格式化：队友收到字面 `Text("hello")` | `sub_agent.rs:429` |
| A-6 | SendMessage 5 秒等不到真回复就返回合成 ack "Message received by X"（代码注释自认 stopgap）——团队系统最大成熟度缺口 | `agent.rs:652-712`、`sub_agent.rs:463-469` |
| A-7 | tmux 是死代码（文档宣传 agent panes） | `tmux.rs` 仅 lib.rs 转发 |
| A-8 | worktree 隔离失败**静默**降级为无隔离——并行编辑互踩风险 | `coordinator.rs:618-626` |
| A-9 | Skills 无注入扫描/签名验证（README 宣称有）；skill 内联 `` !`cmd` `` 有执行能力但信任模型未分级 | grep 验证；`executor.rs:259-330`、`installer.rs:140` |
| A-10 | MCP 协议版本钉在 2024-11-05；OAuth token 内存态（每次会话重跑授权）；工具列表无缓存（每次会话重新串行发现） | `lib.rs:103`、`auth.rs:481`、`server_manager.rs:447` |
| A-11 | 三套任务模型并存（TodoWrite / TaskCreate*/TaskTool / TeamTask*），互相注册进子代理注册表 | `lib.rs:444-446, 669-672`、`agent.rs:301-305` |

### 建议

1. **合并 agent 定义加载器**：以 `agent_loader.rs` 的完整 schema 为准（它已是 Claude Code 兼容格式），`agent_defs.rs` 与 `custom_agent.rs` 收敛为别名；`.claude/agents` 的 `tools:` 语义必须有测试守护。
2. **子代理上下文经济学**（对标 Anthropic 多 agent 研究：子代理返回 1–2k token 蒸馏、传文件引用不传内容、深度上限 3、并发上限 20）：`SummaryGenerator` 已写好未接线——接上；限值写进 Agent 工具描述；支持 MCP 工具与 hooks 继承；提供 `isolation: worktree` 全链路（含失败显式报错而非静默）。
3. 团队系统要么把「真回复」补完（spawn 时自动起 work loop——注释里已写明方向），要么把 messaging 降级为任务板同步；A-5/A-6 修复前不宜对外宣传 teammate 对话能力。
4. Skills/MCP：实现 README 宣称的注入扫描（轻量标记扫描 + 按来源分级信任）与校验和签名；MCP 升协议版本、持久化 token、缓存工具列表（延迟 schema 已是亮点，别让启动期串行发现拖垮首响）。
5. 任务模型收敛到两个：TodoWrite（会话内计划）+ TeamTask（跨 agent 看板），删 TaskTool/TaskCreate 族。

---

## §8 会话 / 事件溯源 / Rewind / 配置

L0 事件日志是全仓质量最高的子系统（flock 单写者、崩溃尾部修复、seq 连续性、边界 fdatasync、投影为纯函数、trace show/replay/diff/export 实用且被 eval 使用）。问题集中在**宿主层不一致**：

| # | 问题 | 证据 |
|---|------|------|
| E-1 | REPL 会话级 rewind/compact 不落日志（P0-9）；桌面 compact 破坏性重写 | 见 P0-9 |
| E-2 | `RewindAction::Delete` 会删掉**会话前就存在**的文件：无 pre-session 基线快照，「首个快照晚于目标轮」被推断为「当时不存在」 | `history.rs:780-790` |
| E-3 | Bash 修改的文件不进 `files_changed`——code rewind 对 Bash 副作用不可见（Claude Code/Gemini/Cline 都只追文件工具，但 Claude Code 有 git HEAD 兜底） | `desktop/src/commands_rewind.rs:24` |
| E-4 | 三个 FileHistoryManager 实例并存、`_index.json` 无锁整文件重写 last-writer-wins | `history.rs:553-568`、`query.rs:73-79`、`session.rs:718` |
| E-5 | TTL 清理是死代码；配额满后快照静默停止 | `history.rs:928-941`、`query.rs:58-60` |
| E-6 | 并发第二查询 tee 静默禁用，无日志标记 | `tee.rs:377-382` |
| E-7 | `todo/write`、`surface/*`、span_id 全是只有定义没有生产者的事件——resume 不恢复 todos、日志无子代理溯源 | grep 验证；`writer.rs:353-356` |
| E-8 | 降级写路径可丢 tool/result → 投影出孤儿 tool_use → resume 后 Anthropic 400（投影无修复） | `writer.rs:220-235`、`projections.rs:196-216` |
| E-9 | `SessionStore::list()` O(全部日志字节)，picker 每次打开全量调用 | `session_store.rs:392-407`、`repl/session.rs:89` |
| E-10 | 手写 TOML 解析器丢掉所有 `[section]` 配置与 `enable_tools`（静默错误配置）；TOML ShannonConfig 与 JSON Settings 两套系统重叠；env 优先级在 trace 与 session_log 两处相反 | `unified_config.rs:461-557, 549`、`trace.rs:34-44` vs `session_log/mod.rs:139-147` |

### 建议

1. **统一宿主语义**：REPL/桌面/server 三宿主对 rewind/compact 的持久化行为必须一致（「权威日志非破坏性 + 投影期折叠」是正解，顺带解决 E-1 双向问题）。
2. Rewind 引入**会话起始基线快照**（会话首写前对所有将被跟踪文件拍基线），Delete 仅在基线证明「目标轮前不存在」时允许；可选项：git repo 存在时用 HEAD 做 fallback（Claude Code/Gemini 均如此）。
3. FileHistory 单例共享 + flock 合并写；housekeeping 任务接上 TTL 清理（`housekeeping.rs` 已存在）。
4. 事件补全：发 `todo/write`、给子代理执行加 span——这让 trace/replay 真正覆盖多 agent 场景。
5. 换 `toml` crate 解析（`config_persist.rs` 已在用）；TOML/JSON 两套配置合并为一张优先级表。

---

## §9 竞品对比与定位

### 机制对比（2026-09 竞品调研，来源见附录）

| 维度 | 行业现状（2026 最佳实践） | Shannon 现状 | 差距 |
|---|---|---|---|
| 上下文治理 | 工具结果硬上限（CC ~25K token）+ 微压缩/工具结果清除 + 结构化摘要 + 压缩后重注入持久上下文 + 部分压缩 | 硬上限缺失（死代码）、微压缩未接线、摘要截 500 字符、重注入缺 memory | **大** |
| 子代理 | 新窗口隔离、1–2k 蒸馏回传、深度 3/并发 20 上限、MCP 继承、worktree 隔离、prompt 内 effort 规则 | 4000 字符硬截断、限值错乱不可见、无 MCP、深度 0/1 二值、无蒸馏 | **大** |
| Memory | 后台挖掘会话→生成**可审阅提案**（Codex/Gemini）、模型自编辑记忆工具（CC/Letta）、注入预算（200 行/25KB 索引） | 关键词正则 + 全量重扫 + 幻觉污染 + 无模型工具 | **大** |
| Checkpoints | 文件工具 diff 快照 + git 兜底 + 会话基线 + 保留策略生效 | 有快照体系但基线缺失（会误删）、TTL 死代码、REPL 不落盘 | 中 |
| 沙箱 | OS 级 + 出网代理域名白名单 + 保护路径 + 凭据代理注入 | 三后端质量不错但兄弟工具旁路 + PTY 旁路 + fail-open | 中 |
| 渐进披露 | skills 三级披露 + MCP 延迟加载 + glob 条件规则 | skills token 预算披露 ✅、MCP 延迟 schema ✅（超 CC）、规则 paths 门控 ✅ | **持平或领先** |
| 提示词 | 右海拔 + 环境感知 + 工具描述即 HCI + 结构化 system-reminder | 4-8K 注入但重复 2-3 次、缺日期/OS、工具描述一句化 | 中大 |
| 事件溯源 | Codex rollout / OpenHands event stream | L0 + trace 工具链 **领先开源同类** | **领先** |
| 权限 | 五模式 + 持久化 allow + 规则简单可推理 | 9 模式 + profile + 分类器 + LLM 分类器（可注入）不持久化 | 复杂度更高、保障更弱 |
| Handoff | Amp：生成可编辑新线程 prompt 替代原地压缩 | 无 | 差异化机会 |

### 结论性判断

1. Shannon 的**基础设施层**（事件日志、取消、恢复、缓存感知序列化、多写者存储）达到一线水准；**harness 策略层**（上下文治理、子代理经济学、memory 策展、权限可推理性）落后竞品一至两代。
2. 「幽灵能力」问题（§10 清单）意味着**当前 README/文档在多个点上过度承诺**——对以「开源、可审计」为核心叙事的项目，这比缺功能更伤。
3. 工具广度（55 个）是差异化，但在「少而精的工具 + 25K 上限」成为共识的 2026（Anthropic 工具设计原则、pi 的极简反证），**先收敛语义再扩张数量**更划算：三套任务工具、四个 tracking 概念、三个 agent 定义格式都在消耗模型注意力与维护成本。
4. 差异化机会（建议强化而非补短板）：L0+trace 的「可回放」叙事（唯一做到 byte 级 wire 捕获的开源 harness）；MCP 延迟 schema；成本可观测；多 provider 缓存感知；secret-guard 字节稳定代理。这些与 improvement-plan-2026-09 的产品主线（Goal/收件箱/成本/多窗口）正好互补。

---

## §10 接通或删除（Wire-or-Delete）清单

死代码是本仓最大的维护负债——每项都有测试在「验证」运行时并不具备的行为：

| 子系统 | 位置 | 建议 |
|---|---|---|
| `context_budget.rs`（三桶预算 + 工具 schema 延迟） | `shannon-engine/src/context_budget.rs` | 接入 ContextGovernor（它的 tools_to_defer 设计比现状好） |
| `context_pressure.rs`（五级压力） | 同上 | 同上（或删，收敛为一级阈值表） |
| `protection.rs`（消息保护级 + 优先级驱逐） | `compact/protection.rs` | 接入压缩路径替代 ad-hoc 保护 |
| `micro_compact` / `prune_stale_tool_results` | `compact/engine.rs:436-487, 346-374` | 接入主循环（微压缩） |
| `ToolExecutionService`（唯一有 40K 截断的实现） | `tool_execution.rs` | 截断下沉 registry 后删除或降级为测试夹具 |
| `tool_orchestration.rs`（读写依赖分析调度器） | `shannon-core/src/tool_orchestration.rs` | 二选一：替代简单 partitioner 或删 |
| `preference_memory` 写路径 | `preference_memory.rs` | 接线到 turn 循环或删除注入 |
| `LlmMemoryExtractor`（完整 LLM 提取管线） | `extract_memories.rs:783-978` | 接线替换关键词提取（推荐）或删 |
| `auto_dream_consolidation.rs`（AI 整理提示） | `auto_dream_consolidation.rs` | 接线为会话末批量任务或删 |
| `IsolatedContext` / `SummaryGenerator` | `shannon-agents/src/isolation.rs, summary.rs` | 接入子代理路径（推荐）或删 |
| tmux（agent panes） | `shannon-agents/src/tmux.rs` | 接 `CoordinatorEvent::AgentOutput` 或删 |
| `builtin/memory.rs` 第三记忆后端 | `shannon-commands/src/builtin/memory.rs` | 删除，REPL 已有实现且被 shadow |
| `resolve_conflicts` / `search_by_relevance` 生产调用 | `memory/store.rs:907-948, 987-1017` | 接线或删 |

---

## §11 值得保持的差异化优势

1. **L0 事件溯源 + trace 工具链**（「dashcam」叙事是真实且唯一的）：字节级 wire 捕获、结构化 diff、导出 eval。
2. **`AbortOnDropStream` 取消 + tool_use_id 去重 + 流式状态机**：多数开源 loop 做错的地方这里做对了。
3. **守卫链**（权限门 → PreToolUse hook → 审计落盘）+ hook 事件面（28 种，超 CC）。
4. **MCP 延迟 schema + `mcp__tool_search`**：上下文经济学上领先 Claude Code。
5. **Skills 渐进披露 + token 预算** + `.claude/` 生态兼容（CLAUDE.md/agents/skills/MCP 开箱）。
6. **secret-guard 契约**（I1-I4 不变量、缓存安全、显示还原）。
7. **Edit 质量**（唯一性 + 三方合并回退 + 撤销快照）与 PathSandbox TOCTOU 纪律。
8. **恢复梯队与 Ollama 弱模型防御**（带 eval 证据的分层恢复）——多 provider 场景独有。
9. **成本可观测**（usage 三层统计、缓存命中可见、预算熔断）——与产品主线一致。
10. 多写者 Memory 存储正确性（append-only + flock + tombstone）。

---

## §12 改进路线图建议

### Wave 0 · 正确性止血（~1 周，全部是点修复）

P0-1 压缩死循环 → P0-2 WebFetch/WebSearch 内容 → P0-3 输出截断 → P0-4 合成 tool_use_id → P0-5 配对感知截断 → P0-6 缓存断点 ≤4 → P0-7 子代理 max_turns → P0-8 bypassPermissions → P0-12 大小写匹配 + plan 模式拦截派生化 → A-2 spawn 谎报 → A-5 Debug 格式化 → C-5 熔断语义。
**验收**：每项一个复现测试；跑一轮长会话 + 大文件 + 断网 + 限速的组合冒烟。

### Wave 1 · 一致性收敛（2–4 周）

1. 单一指令加载器 + 提示词改写（§3 建议 1-4）+ 工具描述全面重写。
2. ContextGovernor 落地（§2 建议 1-2）：微压缩接入、三套截断归一、结构化摘要提示词、压缩后重注入补 memory。
3. Memory：模型 memory 工具 + 增量提取 + 注入预算 + 项目键统一 + M-4 僵尸修复（§6）。
4. 安全：LLM 分类器防注入（§5）、执行类工具统一沙箱面（P0-11）、权限决策持久化、skills 注入扫描。
5. 会话：REPL/桌面 rewind-compact 语义统一（P0-9/E-1）、rewind 基线快照（E-2）、FileHistory 单例（E-4）、todo/write + span 事件（E-7）。
6. §10 wire-or-delete 清单执行完毕（预计净删 5-8K 行）。

### Wave 2 · 架构升级（1–2 月）

1. **engine.rs 拆分**（8268 行 → PromptAssembler / ContextGovernor / TurnStateMachine / ToolDispatcher / RecoveryPolicy / RecoveryParsers / HealthProber 七模块，§2 现状已给出边界）。
2. 子代理经济学完整版：蒸馏回传、深度/并发配置、MCP/skills 继承、worktree 隔 Vol 全链路、effort 规则进提示词。
3. Memory LLM 化 + 会话末「dream」批量整理 + 可审阅提案流（对标 Codex 两阶段）。
4. `/handoff`（Amp 式）：把当前线程蒸馏成可编辑新线程 prompt——与 compaction 互补，实现成本低（压缩摘要管线可复用）。
5. 事件日志索引 sidecar（offset 表）解决 E-9；配置系统合一（E-10）。
6. 权限模型简化：9 模式 → 5 模式 + profile 映射表，文档给「何时会问我」的决策树。

---

## 附录 · 审查方法与置信度说明

- 六路深读覆盖：`query_engine`（含 8268 行 engine.rs 全量）、提示词全链路、shannon-tools 工具逐个审计、memory 全家、shannon-agents 全家 + skills/MCP/hooks/plugins、session_log/checkpoint/config。
- 竞品信息来自官方文档、已发布/泄露的系统提示词汇编、源码（Codex rust 工作区、Gemini CLI）、Anthropic 工程博客（工具设计/上下文工程/多 agent），未证实处已标注。
- P0 清单中的 P0-1、P0-2、P0-3、P0-7、A-9（缺失类）经主审二次核验；其余各条均有直接 `file:line` 证据，建议修复时以测试先复现。
- 行号基于 dev @ 04f7bdee，后续提交可能漂移。
