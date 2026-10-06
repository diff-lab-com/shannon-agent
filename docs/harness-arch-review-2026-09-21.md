# Shannon Harness 架构复审报告（2026-09-21）

- **分支**: `review/harness-arch-20260921`（基于 dev @ c7e26c8d）
- **审查人视角**: 高级 AI 架构师 / 工程师（复审）
- **审查范围**: 核心 harness 架构（agent loop / LLM 引擎 / 流式与重试）、提示词体系、核心工具与 MCP、memory 与上下文治理、权限与沙箱、会话基础设施；并对照 2026-09 竞品 harness 设计
- **方法**: 5 路并行深读（引擎、提示词、工具、memory、竞品调研，每条结论附 `file:line`）+ 对全部 P0/P1 结论的**人工二次核验** + 与上一轮审计（[harness-review-2026-09.md](./harness-review-2026-09.md)，Wave 0/1/2 已合入）逐项对照
- **定位**: 上一轮报告验证修复落地情况 + 新一轮问题发现。与 [improvement-plan-2026-09.md](./improvement-plan-2026-09.md)（产品侧）互补。

---

## TL;DR

**总体判断：Wave 0 修复质量过硬（12 个 P0 中 9 个已验证修复），工程地基（L0 日志、缓存纪律、恢复梯队、测试文化）达到一流水平；但本轮发现 1 个新 P0 + 7 个新 P1，集中在「最后一公里」：权限 fail-open、hot-path panic、假成功退出码、安全边界缺口（SSRF/无超时/死注册）、memory 一致性与缓存自毁。**

与竞品的结构性差距不在功能广度（Shannon 的工具面、MCP、teams、LSP 均超出 Claude Code），而在三点：

1. **安全默认值**：Codex 默认 OS 级 deny-by-default 沙箱，Shannon 的 Landlock 实现优秀但**默认裸跑**；
2. **上下文经济学**：Anthropic 实测 context editing +29%、memory tool +39%，Shannon 三机制骨架俱全但质量层弱（压缩有损无校验、micro-prune 一刀切、memory 一致性 bug + 每轮自毁 KV 缓存前缀）；
3. **提示词欠规约**：基础系统提示词 ~500 tokens（约为 Claude Code 的 1/5），只覆盖 6 个工具的使用政策，74 个内置工具中一半无政策、无安全前言、无 per-model 变体。

---

## §1 上轮审计（P0-1…P0-12）修复验证状态

| 上轮问题 | 状态 | 证据（本轮核验） |
|---|---|---|
| P0-1 压缩死循环 | ✅ 已修复 | 熔断器 `MAX_COMPACTION_FAILURES=2` + pair-aware truncate 兜底（engine.rs:2456-2471），注释记录了 livelock 修复 |
| P0-2 WebFetch 内容不进模型 | ✅ 已修复 | 页面正文进 `content`（web.rs:712），有回归测试 |
| P0-3 工具输出无截断 | ✅ 已修复 | `cap_tool_result` 40K 字符、char-boundary 安全、env 可调（env_config.rs:84） |
| P0-4 合成 tool_use_id 被 400 | ✅ 已修复 | 查询域 tool_use_id 去重 + 合成错误走配对 tool_use/tool_result（engine.rs:2894-2904, 3384-3409） |
| P0-5 截断切断配对 | ✅ 已修复 | 主循环熔断回退走 `safe_split_point`（engine.rs:2458-2469） |
| P0-6 缓存断点超限 | ✅ 已修复 | 恰好 2 个 system 断点 + adapter 2 个 = 4，有回归测试（engine.rs:7161-7193） |
| P0-7 子代理 max_turns 用错字段 | ⚠️ **主路径已修，残留一处** | spawn 路径已用 `d.max_turns`（agent.rs:381-386）；**team 路径仍是 `max_concurrent_tasks as u32`（agent.rs:769）** |
| P0-8 teammate 硬编码 bypassPermissions | ✅ 已修复 | 继承配置 mode、默认 auto（coordinator.rs:665-670） |
| P0-9 rewind/compact 不落权威日志 | ✅ 已修复 | `truncate_to_turn` 持久化到 L0 日志（CLAUDE.md 记录，commit 54ccad58） |
| P0-10 memory 幻觉提取 | ⚠️ 部分修复 | auto_save_memory 客套话路径已收敛，但 AutoDream 仍对 **assistant 文本**做关键词提取（auto_dream.rs:410-424），幻觉记忆入口仍在（见 N-7） |
| P0-11 沙箱兄弟工具旁路 | ⚠️ 部分修复 | background 路径加了 `analyze_command_security` Critical 拒绝（background.rs:195-202），但只挡 Critical、不经沙箱 rewrite；PTY-先于-沙箱、无后端裸跑本轮未复查 |
| P0-12 工具名大小写击穿只读快速通道 | ✅ 已修复 | 大小写不敏感匹配 + 注释说明（permissions.rs:25-55） |

另：上轮「接通或删除」清单**仍有残留死代码**——`context_budget.rs`、`context_pressure.rs`、`compact/protection.rs` 全部零生产引用（引擎自行内联了更粗糙的 60/70/80% 逻辑，engine.rs:2295-2456）；`extract_memories.rs` 的 LLM 提取器与 `auto_dream_consolidation.rs` 的 AI 整理提示词已完整实现但仍未接线。

---

## §2 新发现：正确性问题（P0 / P1）

> 全部 P0/P1 均经主审在源码二次核验。标注「亲验」= 主审读过代码；其余有子代理 file:line 证据。

### N-1 · P0 · 权限门 fail-open：无审批通道时非 Critical 危险操作直接放行

- **证据**：`crates/shannon-core/src/query_engine/engine.rs:3849`（亲验）——`// If no permission channel, assume auto-allow`。`PermissionGateNode` 判定为 `Prompt`（非 Critical 的 Medium/High 风险：Bash/Edit/Write…）时，若 `permission_request_tx` 为 `None`，落空穿透到执行。
- **触发面**：REST `/api/query` 构造默认（Suggest）模式的 `PermissionManager` 并传 `None`（api_server.rs:533, 627）；`shannon-server/src/github.rs:298`（自动化）同样传 `None`。**即：HTTP API 对外服务时，非 Critical 工具零确认自动执行。**
- **修法**：`permission_request_tx == None` 且判定为 `Prompt` 时，走 Critical 臂同样的拒绝路径（合成 error tool_result），并在启动时 warn。

### N-2 · P1 · 压缩 hot path UTF-8 panic（多字节字符即崩会话）

- **证据**：`crates/shannon-engine/src/compact/engine.rs:368-373`（亲验）——`text.len() > preview_limit * 2` 后直接 `&text[..preview_limit]`（字节 200 处切分）。CJK 注释、emoji 日志等在字节 200 处跨多字节边界即 panic。
- **影响**：micro-prune 在上下文 >70% 时触发；panic 杀死 producer 任务 = 会话中断。中文用户路径上这是高频雷。
- **修法**：向前回退到 `is_char_boundary`（仓库已有该模式：engine.rs:3704、cli/main.rs:2304）。补非 ASCII 回归测试。

### N-3 · P1 · 流中 provider 错误事件未类型化 → 截断的答案被记为成功

- **证据**：`StreamEvent` 无 `Error` 变体（engine types.rs:1057-1084）；Anthropic SSE `{"type":"error"}` 被 passthrough 归类为 `InvalidResponse`（adapter.rs:671-676），既不可重连（streaming.rs:537-547）也不属 timeout 类（engine.rs:4822）；若此前已有部分文本到达，循环发 **`Completed`**（engine.rs:4897）——**headless 模式以 exit 0 记录一个被截断的回答**。
- **连带**：Anthropic 529 `overloaded_error` 不在可重试集合（retry.rs:53 仅 429/500/502/503/504），请求期零退避重试。
- **修法**：增加类型化 error/overloaded 流事件 → 归类可重试；任何流中异常死亡一律发 `Failed` 而非 `Completed`；529 加入可重试集合。

### N-4 · P1 · Bash 无默认超时，registry 级超时从未接线

- **证据**：`BashInput.timeout` 为 `Option<u64>`（system.rs:845）；`None` 时 `run_shell_captured` 不加任何 `tokio::time::timeout`（system.rs:42-58，亲验）。`ToolRegistry::set_execution_timeout`（shannon-core/src/tools.rs:294）**零生产调用方**。
- **影响**：一条挂起命令（守护进程、等待 stdin）永久阻塞整个 turn，只能 Esc。
- **修法**：默认 per-call 超时（建议 120s、模型可调上限 600s，对齐 Claude Code），装配时设置 registry 执行超时。

### N-5 · P1 · WebFetch SSRF：check-then-fetch、DNS 不校验、重定向不复查、body 无上限

- **证据**：`validate_fetch_url`（web.rs:82-155，亲验）只检查**字面 host**：`127.0.0.1.nip.io` 或 DNS rebinding 指向回环/内网即可绕过；IPv6 唯一本地（fc00::/7）与 link-local（fe80::/10）未封（仅封 v6 loopback/unspecified）；reqwest 默认跟随 ≤10 次重定向且不对目标重校验（→ 云元数据可经跳板 exfil）；`response.text()`（web.rs:202, 579）无 content-length 上限，GB 级 body 全量进内存。
- **修法**：自行解析 host 并对每个 socket IP 重校验；`redirect(Policy::custom)` 每跳重跑校验；封 IPv6 ULA/link-local；流式读取 + 大小上限。

### N-6 · P1 · 延迟 MCP schema 死循环：`mcp__tool_search` 从未注册

- **证据**：延迟 schema 模式下，MCP 工具注册为 stub，描述要求模型「用 `mcp__tool_search` 取完整参数 schema」（mcp_tool_adapter.rs:192-204）；但 `McpToolSearchTool` **从未注册**——lib.rs 仅有 re-export（lib.rs:190），全仓无生产 `::new(` 调用（亲验 grep）。`ToolSearchTool`（tool_search.rs）同样未注册。
- **影响**：开启 `defer_tool_schemas` 后所有延迟 MCP 工具永久不可用；模型按提示调用会得到「工具不存在」。
- **修法**：`defer_tool_schemas` 开启时注册 `McpToolSearchTool`；补一条「延迟工具可被取回 schema 并成功执行」的集成测试。

### N-7 · P1 · Memory 注入与写入不一致 + 每轮自毁 KV 缓存前缀

- **证据（两点独立成立）**：
  1. **一致性**：引擎注入句柄在 REPL 启动时 `load()` 一次（repl/mod.rs:1023-1031，亲验；注释自称 M-1「同库多写者安全」——存储层安全，但**内存视图不同步**）。`MemorySaveTool`/`AutoDreamService` 用独立句柄写盘后，引擎句柄不重载（查询路径无 `.load()`）→ **模型看不见自己刚保存的记忆，`MemoryForget` 删除的条目继续被注入**，直到进程重启。
  2. **缓存**：memory 块位于提示词缓存**稳定区**（engine.rs:1559-1620），而 AutoDream 每次 query 后 `save()`（auto_dream.rs:442, engine.rs:5775-5790）→ 下一轮 memory 块变化，**从 memory 块起的整个缓存前缀（含 CLAUDE.md/repomap）作废重算**。这违反 Manus「前缀字节稳定」缓存纪律，也是 cache read（~0.1x 价格）收益的主要流失点。
- **修法**：引擎/工具/AutoDream 共享一个 `Arc<RwLock<MemoryStore>>`（测试用的 `with_memory_arc` 已存在，engine.rs:7893+）；memory 块移出稳定区，或会话开始时快照固定、压缩时才刷新。
- **连带 P3**：resume 后 `memory_extract_cursor` 从 0 起（engine.rs:808）→ 恢复会话后第一次提取把全部历史重新喂给关键词匹配。

### N-8 · P1 · FileHistory：TTL 清理死代码 + 配额静默停摆 + 索引非原子

- **证据**：`cleanup_old_snapshots`（history.rs:864）零生产调用方（仅测试）；配额（100MB）满时 `record_snapshot` **失败而非逐出**（history.rs:624）→ checkpoint 静默停止（桌面侧 fail-soft，commands_rewind.rs:73-75）；`_index.json` 用普通 write 非原子（history.rs:568-571），解析失败向上传播（history.rs:530-531）→ **一次崩溃即可永久瘫痪该用户的 snapshot/rewind**；多会话并发读改写无锁（history.rs:558-574）互相丢条目。
- **修法**：housekeeping 接线 TTL 清理；配额改 oldest-first 逐出；索引 tmp+rename + 解析失败降级为空索引 + flock。

### 残留旧账

- **P0-7 残留**：team 路径 `max_turns: def.map(|d| d.max_concurrent_tasks as u32)`（agent.rs:769，亲验）——explorer（max_concurrent_tasks=1）经 team 路径 spawn 只允许 1 轮。
- **P0-11 残留**：background 只做文本 Critical 分类，不经 `SandboxExecutorRewrite`；「Use Bash (sandboxed)」提示在无沙箱后端时是空头支票（system.rs:1010-1046：无后端即裸跑）。

---

## §3 分域发现（P2/P3）

### 3.1 引擎与主循环

| # | 级别 | 发现 | 证据 |
|---|---|---|---|
| E-1 | P2 | 会话状态双重记账：producer 改 clone，`self.conversation` 仅在消费端回放 `ConversationUpdate` 时刷新；早弃流/REST 新建引擎路径静默分叉 | engine.rs:1791 vs shannon-ui/repl/query.rs:1344 |
| E-2 | P2 | token 热路径阻塞 IO：每个事件内联写 L0 日志（含间歇 `sync_data`）在 async producer 上；慢盘拖住流式 drain | engine.rs:163-174, tee.rs:756 |
| E-3 | P2 | 无界事件通道；消费端死亡仅 warn 继续——TUI 暂停/WS 拥塞时内存无界增长 | engine.rs:1485, 181-186 |
| E-4 | P2 | 成本模型忽略缓存计价（cache read ~0.1x / creation 1.25x）→ USD 预算熔断与成本展示在缓存密集会话中严重失真 | types.rs:88, 393-397 |
| E-5 | P3 | `tool_choice` 全链路未支持（0 处序列化）——think-only/截断恢复只能靠文本 nudge | repo grep |
| E-6 | P3 | `sanitize_tool_sequence` 只在 OpenAI 路径，Ollama 路径裸奔 | adapter.rs:381 vs 466 |
| E-7 | P3 | `process_query` ~4,300 行 / engine.rs 9,879 行：A8/A13/A14/P-M/P-B 各恢复路径内联嵌套，改动回归风险高（尽管测试好） | engine.rs:1462-5756 |

### 3.2 提示词体系

基础系统提示词仅 **~2,086 字符 ≈ 450-520 tokens**（types.rs:765-797，亲验 char 数）——问题方向是**欠规约**，不是臃肿。全装配（skills 2K + memories 2K + repomap 2K + CLAUDE.md + browser/team playbook + env）约 5-8K tokens，60-70% 在稳定断点后。

| # | 级别 | 发现 | 证据 |
|---|---|---|---|
| P-1 | P1 | 工具使用政策只覆盖 Read/Grep/Glob/Edit/Write/Bash 六件套；**TodoWrite/Agent/WebFetch/Skill/后台工具/ask_user 无任何政策**——74 个工具近半数无使用次序指引 | types.rs:776-784 |
| P-2 | P1 | 无安全/权限前言与 git 安全规则（Claude Code 有「不擅自 commit」「破坏性命令三思」段）——git 安全只存在于 /commit 模板内 | types.rs:765-797 |
| P-3 | P2 | 工具描述深度两极分化：Bash/Read 接近最佳实践；Edit/Write/Grep/Glob/TodoWrite/Agent 是一句话，无 when-not-to-use、无失败模式；Docker sandbox 构造器把 Bash 好描述换成一句 slogan（system.rs:983） | file/mod.rs:250,354,605 |
| P-4 | P2 | 子代理 Spawn 把 persona+task+context JSON 全塞 **user message**（engine 保持默认系统提示词）；默认 persona 一句话——指令与数据混排削弱遵循度 | agent.rs:555-570 |
| P-5 | P2 | `/commit` 模板使用 Claude Code 的 `` !`git log --oneline -10` `` 插值语法，**Shannon 未实现该展开** → 字面文本到达模型 | commit.rs:78 |
| P-6 | P2 | 无 per-model 提示词变体：GLM/MiniMax 已知失败模式（think-only、全文 cat）用运行时 nudge 打补丁而非调基础提示词；仅本地小模型有变体 | env_config.rs:8-44 |
| P-7 | P2 | `/plan` 硬编码 Rust 工具链 `Bash(cargo check:*)` | plan.rs:93 |
| P-8 | P3 | CLAUDE.md 包装标题畸形（`## project Scope: CLAUDE.md ---`）；向上遍历时父目录文件 scope 被改标为 User | project_instructions.rs:181-186, 127-132 |
| P-9 | P3 | 全部提示词硬编码英文；10 个 locale 文件只喂 UI 不喂模型 | locales/*.yml |

### 3.3 工具层与 MCP

| # | 级别 | 发现 | 证据 |
|---|---|---|---|
| T-1 | P2 | MCP `destructiveHint`/`readOnlyHint` 注解解析了但**未映射到 Tool trait flags** → MCP 工具永不触发「必确认」、永不并行 | discovery.rs:215-240 vs mcp_tool_adapter.rs |
| T-2 | P2 | Bash 自算的 `requires_confirmation` 被计算后丢弃（只挡 Critical）；引擎层另有一套会漂移的黑名单；分类器子串匹配漏报（`find -delete` 判只读）误报（`clang-format` 判高危） | system.rs:134-139, 269-490 vs permissions.rs:297 |
| T-3 | P2 | MultiEdit 顺序 `fs.write_bytes` 非原子、失败不回滚（错误信息自认） | multiedit.rs:144-150 |
| T-4 | P2 | 8 个 browser 工具全部 `is_concurrency_safe: true`，共享单 ChromeSession——并行调度器会让 click/navigate/tabs 同帧竞跑 | browser_tools.rs:48…427 |
| T-5 | P2 | Read 无行号（模型只能靠字符串重定位）、无二进制嗅探（.so 喂进上下文）、无 PDF/notebook 读 | read.rs:195-214 |
| T-6 | P2 | Glob 无结果上限（`**/*` 冲爆上下文）；schema 普遍不严格（无 additionalProperties、Bash/Read/Edit 各漏自有参数——模型无法发现 `preview`/`use_pty` 等能力） | glob.rs:202-246; system.rs:1109 vs 850 |
| T-7 | P2 | 74 个内置工具全量 schema 常驻广播：上下文税 + 权限面扩大；发现型门控（ToolSearch）恰好是未注册的那个（N-6） | lib.rs:269-535 |
| T-8 | P3 | Grep 描述谎称 "built on ripgrep"（实为 regex+ignore）；context 行无行号；异步路径阻塞 IO 在 SSH/Docker world 被网络 RTT 放大 | grep.rs:290, 136-158, 413 |
| T-9 | P3 | 命名不统一：`Read`（Pascal）vs `go_to_definition`（snake）vs `mcp__x__y`；`Task` 工具是 todo-store 操作枚举而非子代理 spawn，与竞品语义冲突 | lib.rs 注册表 |
| T-10 | P3 | 任务面 5 套并存（TodoWrite / TaskCreate-List-Update-Get / Task / team_task_* / TaskOutput-Stop）无 TodoRead | todo.rs, task.rs |

### 3.4 Memory 与上下文治理

| # | 级别 | 发现 | 证据 |
|---|---|---|---|
| M-1 | P2 | AutoDream 关键词匹配**含 assistant 文本**（"let's use Redis" 探索性讨论被存为 Decision 注入未来所有会话）；已写好的 LLM 提取器未接线 | auto_dream.rs:410-424; extract_memories.rs |
| M-2 | P2 | micro-prune 一刀切：>70% 时一次性的、所有 >400 字符非错误工具结果一律砍到 200 字符预览——无年龄分层、无保护名单，模型可能仍需要的 Read 被砍 | engine.rs:2418-2434 |
| M-3 | P2 | 压缩有损且无覆盖校验：摘要输入每条截 500/4000 字符、总预算 2000 tokens，唯一质量门是「退化输出检查」，坏摘要静默销毁全部细节 | compact/types.rs:341-372, engine.rs:288-301 |
| M-4 | P2 | 三套压缩配置漂移：`CompactConfig`(0.75/10) vs `p2_compact::Policy`(0.75/10/24) vs `streaming.rs`(0.8/keep 2)，主循环交叉调用两套 | types.rs:62-74; compact.rs:109-120; streaming.rs:323-324 |
| M-5 | P2 | Todo/plan 状态 RAM-only：不随会话持久化、resume 不恢复、**压缩后不重注入**（重注入只覆盖 instructions+memories）→ 压缩后 agent 丢清单 | todo.rs:256, 395; engine.rs:2555-2585 |
| M-6 | P2 | 无跨会话搜索（`search_events` 单会话；picker 只有 80 字符预览）；无任何语义检索/嵌入设施 | session_store.rs:609 |
| M-7 | P2 | CLAUDE.md 总大小无上限（仅 @import 有 100KB cap）；根 CLAUDE.md 任意大全量进稳定区 | project_instructions.rs:22 |
| M-8 | P3 | memory 项目键 = 原始 cwd 字符串 → 项目目录改名即孤儿化；`/memory` 命令文档与实际存储路径/触发机制不符 | engine.rs:1534; builtin/memory.rs:5-9 |
| M-9 | P3 | 会话 events.jsonl 无轮转/GC；会话日志与文件历史无界增长 | writer.rs |

### 3.5 权限与沙箱

- **P2**：Bash `cwd` 参数不经校验直接传给子进程（`cwd:"/etc"` 可用）；真正的边界是可选进程沙箱，而**无后端时默认裸跑**仅留 warn（system.rs:528, 1010-1046）。Landlock 后端 fail-closed 实现是仓库里最好的沙箱代码（sandbox/landlock_backend.rs），但它不是默认。
- **P3**：`ask_user_question` 标记 `is_read_only=true`（RO 快速通道放行合理但语义上值得复查）；`strip_ansi` 每行编译正则（system.rs:1326-1329）。

---

## §4 竞品对标（2026-09）

> 完整调研（含来源 URL）见工作记录；此处为能力结论。竞品事实基于公开文档/泄漏/官方博客，_cursor 内部未公开处不臆断_。

### 4.1 Shannon 已达到或领先的点

| 能力 | 状态 |
|---|---|
| 会话持久化/崩溃安全 | **领先**：L0 append-only + flock 单写者 + 尾部修复 + fdatasync 边界 + E-9 索引 sidecar——优于多数竞品的 JSON 会话文件 |
| KV 缓存友好装配（设计） | 与 Manus/Claude Code 纪律一致：稳定/动态分区、恰好 4 断点、env 放最后（**但被 N-7 memory churn 破坏**） |
| 工具广度 | 超 Claude Code：LSP 7 件套（对齐 OpenCode 实践）、git/gh 包装、8 个 CDP browser 工具、teams、Cron、repomap |
| MCP 客户端 | 一流：握手能力捕获、三段超时、健康检查+重启退避、OAuth 含 insufficient_scope 恢复、`mcp__` 前缀免碰撞 |
| Provider 中立 | 4 家 wire format + 录制/回放测试设施 + mock SSE DSL——测试文化领先 |
| 多 agent teams | 与 Claude Code agent teams 同代（lead/worker/互消息） |
| 子代理上下文隔离 | 有（Agent tool），但 persona/上下文注入方式弱（P-4） |
| Skills | SKILL.md + 渐进披露 + 2K token 预算 + 第三方注入扫描（A-9）——踩中开放标准 |
| 恢复梯队 | A8/A13/A14 等带事故记录的恢复路径 + mock SSE 回归测试——战斗检验充分 |
| headless 协议 | api-protocol crate + server 先行（对齐 Codex App Server 方向）——很多竞品后补 |

### 4.2 差距 Top 8（按杠杆排序）

| # | 能力 | 最佳实践 | Shannon 现状 | 杠杆 |
|---|---|---|---|---|
| 1 | **OS 沙箱默认化** | Codex：Seatbelt/Landlock/seccomp deny-by-default 默认开；沙箱与审批双轴解耦 | Landlock 代码优秀但默认裸跑（T-3.5）；单轴审批 | Codex 体验好的一半原因在此；Rust 实现成本低 |
| 2 | **上下文三机制质量** | Anthropic 实测：context editing +29%、+memory 共 +39% | 骨架全有但质量弱（M-2/M-3/N-7） | 补校验与分层即得收益，无需新机制 |
| 3 | **缓存纪律执行** | Manus：前缀字节稳定、append-only | 设计对、执行被 memory churn 打穿（N-7.2） | 一处架构修即可 |
| 4 | **安全前言 + 全工具政策提示词** | Claude Code ~2.5-3K tokens 条件化组装 | 500 tokens 欠规约、半数工具无政策（P-1/P-2） | 提示词是最高 ROI 的修改面 |
| 5 | **effort/thinking 档位** | Claude Code 四档 effort dial | 无 | 实现便宜、成本/质量旋钮 |
| 6 | **cross-model oracle** | Amp Oracle：只读第二模型评审 | 有 LLM 权限分类器，无 oracle 子代理 | 跨模型互评审捕捉系统性盲区 |
| 7 | **hooks 事件面** | Claude Code ~18 lifecycle 事件、模型不可绕过 | 有 PreToolUse + Pre/PostCompact，面窄 | 合规/格式化/审计场景的确定性保证 |
| 8 | **权限队列作为 workspace 状态** | Crush：多客户端共享权限队列、决策持久 | 决策不持久化；REST 路径 fail-open（N-1） | 多窗口/桌面/远程场景基础 |

其余对标点：handoff-替代-压缩（Amp，便宜可加）、shadow-git checkpoint（Cline；Shannon FileHistory 思路相同但需修 N-8）、AGENTS.md 已在支持列表（好）、Terminal-Bench 挂榜（Shannon 有 deepswe eval 文档，建议接 tbench 拿公开可比数字）。

---

## §5 建议路线图（复审版）

### Wave R0 · 正确性止血（~1 周，全部点修复）

1. **N-1** 权限 fail-open → 无通道即拒绝（对齐 Critical 臂）。
2. **N-2** UTF-8 panic → char-boundary 回退 + 非 ASCII 回归测试。
3. **N-3** 流中错误类型化 + 假 Completed 修正 + 529 可重试。
4. **N-4** Bash 默认超时 + registry 超时接线。
5. **N-5** WebFetch：DNS 解析校验 + 重定向复查 + IPv6 ULA/link-local + body 上限。
6. **N-6** 注册 `McpToolSearchTool`（defer 开启时）+ 集成测试。
7. **N-7.1** 共享 `Arc<RwLock<MemoryStore>>`；**N-8** FileHistory 原子索引 + TTL 接线 + 配额逐出。
8. 旧账残留：P0-7 team 路径 max_turns（agent.rs:769）；P0-11 background 纳入沙箱 rewrite 面。

### Wave R1 · 质量与一致性收敛（2-4 周）

1. **提示词升级**：安全/权限前言 + 全工具使用政策（含 TodoWrite/Agent/WebFetch/后台/ask_user）+ 工具描述补全到 Bash/Read 的水准 + 修 `/commit` 插值 + per-model 变体（从 env_config 的 nudge 经验反推）+ 修 CLAUDE.md 包装标题。
2. **Memory 架构**：memory 块移出缓存稳定区（或会话期快照）；提取只处理 user 轮 + 接线已实现的 LLM 提取器（或删除）；resume 光标种子化。
3. **压缩统一**：单一政策源；摘要覆盖校验（文件路径/符号覆盖率，不达标升配重试一次）；todo 持久化 + 压缩后重注入。
4. **工具层**：MCP 注解 → trait flags；MultiEdit 原子化回滚；browser 工具 CS=false（snapshot/screenshot 除外）；Read 行号+二进制嗅探；Glob 上限；schema 从 serde struct 生成。
5. **工具披露策略**：默认装配收敛核心集（~20 个），长尾走 ToolSearch 门控（与 N-6 一并设计）。
6. 「接通或删除」第二轮：context_budget/context_pressure/protection、LLM 提取器、AI 整理器。
7. 成本模型补缓存计价（E-4）。

### Wave R2 · 架构升级（1-2 月）

1. **engine.rs 拆分**（恢复阶梯/压缩/权限相位各自成状态机——测试已就位，重构风险可控）。
2. **OS 沙箱默认化**：Landlock/seccomp 默认开 + 沙箱模式 × 审批策略双轴（Codex 模型）；无后端时显式降级声明。
3. **Hooks 事件面扩展** + 权限决策持久化为 workspace 状态（多客户端共享队列）。
4. **effort dial + cross-model oracle 子代理**（复用现有 Agent tool + 多 provider 底座，Shannon 做这个有天然优势）。
5. **跨会话搜索**（E-9 索引扩展或容器级 grep）+ 会话日志 GC。
6. **Terminal-Bench 接入**：公开可比的 harness 分数（竞品已互相咬合在 tbench 上）。

---

## 附录 · 置信度说明

- §2 全部 P0/P1 与 §1 表格中的状态判定经主审在 worktree 源码逐条亲验；§3 各表来自子代理深读（均带 file:line），修复前请先以测试复现。
- 竞品事实基于公开来源（官方文档、changelog、官方博客、泄漏分析），Cursor/内部实现细节处已标注不臆断；benchmark 仅采信 tbench.ai 与 epoch.ai。
- 本报告与上轮报告（harness-review-2026-09.md）配套阅读：§1 是对上轮 12 个 P0 的验收。
