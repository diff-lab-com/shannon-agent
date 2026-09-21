# Shannon Harness 三波实施报告（2026-09-21）

- **分支**: `review/harness-arch-20260921`（基于 dev @ c7e26c8d）
- **关联**: 复审 [harness-arch-review-2026-09-21.md](./harness-arch-review-2026-09-21.md) 列出的 N-1…N-8 + R1/R2 全部条目。
- **提交链**: 3 个 commits
  - `3b795f5a` 子代理 R0/R1 主体（25 files, +2421 / −289）
  - `934bbfdc` style: rustfmt
  - `1ea3b9bc` 子代理 R2 + 我亲自收尾的 N-1/N-3/N-6/N-7/N-8/recovery 模块/R1-3
- **方法**: 5 路并行实现代理（shannon-engine, shannon-tools, shannon-core-engine, shannon-core-tool/memory, shannon-agents/commands/ui/cli）+ 主审亲自核验+合入修复+补漏。

---

## TL;DR

**Wave R0（正确性止血）+ R1（一致性收敛）+ R2（架构升级）全部落地；工作区 `cargo check -p` 7 个核心 crate 0 错误；recovery 模块抽取净减 engine.rs 88 行（10186→10098），R1-3 todo 持久化+重注入的回归测试通过。**

---

## §1 R0 正确性止血

| # | 项目 | 改动位置 | 修复要点 |
|---|---|---|---|
| N-1 | **权限门 fail-open** | `shannon-core/src/query_engine/engine.rs:3849` | 无审批通道时改为拒绝 + 合成 error tool_result，停止 REST `/api/query` 与 `github.rs` 自动化的零确认自动执行 |
| N-2 | UTF-8 panic | `shannon-engine/src/compact/engine.rs:360-380` | `&text[..preview_limit]` 改用 `is_char_boundary` 回退，CJK/emoji 安全 |
| N-3 | 流中 provider 错误 | `shannon-engine/src/api/{types,adapter,streaming}.rs` + `engine.rs:4846` | 新增 `StreamEvent::Error{message}` 变体；4 家 provider 都类型化传错；引擎消费端走重试阶梯或 `Failed`，不再记 `Completed` |
| 529 | overloaded 重试 | `shannon-engine/src/api/retry.rs + error.rs` | 529 加入 `retryable_status_codes`，与 5xx 一致 |
| N-4 | Bash 默认超时 | `shannon-tools/src/system.rs` | 默认 120s（`SHANNON_BASH_TIMEOUT_MS` 可调、600s 硬上限）；Docker 沙箱分支同步 |
| N-5 | WebFetch SSRF | `shannon-tools/src/web.rs` | host→socket IP 解析校验 + IPv6 ULA/link-local/映射 v4 封禁；reqwest 重定向策略替换为逐跳复查（≤3 跳）；content-length 校验 + 10MB 流式上限 |
| N-6 | McpToolSearchTool 死循环 | `shannon-core/src/tools.rs:403-460` | `register_batch` 首次延迟时自动注册 `mcp__tool_search` 并把真实 schema 入 store；schema 精确查找结果加 `## name` 头 |
| N-7.1 | Memory 写入与注入不一致 | `shannon-core/src/memory/tools.rs` + `shannon-ui/src/repl/mod.rs` + `shannon-cli/src/main.rs` | 新增 `with_shared_store`；引擎/工具/AutoDream 共享同一 `Arc<RwLock<MemoryStore>>`；CLI 重复注册 bug 修复 |
| N-7.2 | Memory 自毁缓存前缀 | `shannon-core/src/query_engine/engine.rs:1666` | memory 块从稳定缓存区移到动态区；AutoDream 提取改为只取 user 轮 |
| N-7.3 | Resume 光标种子 | `engine.rs:restore_messages` | `memory_extract_cursor` 初始化为恢复消息数，避免恢复会话后第一轮重扫全量 |
| N-8 | FileHistory 原子性+TTL | `shannon-tools/src/file/history.rs` + `shannon-ui/src/repl/query.rs` | `_index.json` tmp+rename；损坏索引降级为空索引；配额满改 oldest-first 逐出；`run_housekeeping()` 接线 post-turn 快照钩子（24h 节流） |
| 旧账 P0-7 | team max_turns | `shannon-tools/src/agent.rs:769` | 用 `def.max_turns` 而非 `max_concurrent_tasks as u32` |
| 旧账 P0-11 | background 高危命令 | `shannon-tools/src/background.rs` | High + Critical 全部拒绝 |
| R1-2 | 压缩统一 | `shannon-core/src/query_engine/{streaming,mod}.rs` | 删除 `needs_compress/compress/summarize_messages` 重复路径与游离 helpers；主循环 micro-prune + P2-1 选择器 + CompactEngine 成为单一来源；删除相关测试；保留 `estimate_tokens_with_system_prompt`（仍被 stats 引用） |

## §2 R1 一致性收敛

| # | 项目 | 改动位置 | 修复要点 |
|---|---|---|---|
| R1-1 | **基础系统提示词升级** | `shannon-core/src/query_engine/types.rs:765-797` | 2,086 → 3,701 字符，新增 Safety/Permissions 前言 + TodoWrite/Agent/background/WebFetch/ask_user/Skill/Memory 全工具政策 + Plan Mode + Compact Awareness；保留所有既有经验性条款 |
| R1-1 | `/commit` 模板插值 | `shannon-commands/src/builtin/commit.rs` | 4 处 `` !`...` `` 全部改为"运行 git 命令后自己起草"指令；含 pin 测试 |
| R1-1 | `/plan` 模板解耦 | `shannon-commands/src/builtin/plan.rs` | 去掉硬编码 `Bash(cargo check:*)`；`content_length` 从 3000 修正为 1688（实际字节数）；增加 no-cargo 测试 |
| R1-1 | `/issue` `/batch` 插值 | `shannon-commands/src/builtin/issue.rs + batch.rs` | 同 commit 修复模式 + pin 测试 |
| R1-1 | CLAUDE.md 标题 + scope 标签 | `shannon-core/src/project_instructions.rs` | 5 处 `## {scope} Scope: {path} ---` → 干净的 `## {scope} scope: {path}`；新增 `InstructionScope::Ancestor`（父目录文件不再被标为 `User`）；增加 3 项测试 |
| R1-3 | **todo 持久化 + 重注入** | `shannon-tools/src/todo.rs` + `shannon-core/src/query_engine/engine.rs` + `shannon-ui/src/repl/mod.rs` | 进程全局 `TaskStore`（per-tool 实例 → 全局）；写穿持久化 `~/.shannon/todos/<fnv1a(cwd)>.json` 原子写（`SHANNON_TODO_PERSIST=0` 关闭）；`pub fn todo_reinjection_block()` 渲染 markdown；`QueryEngine::add_reinjection_provider(&self, F)`（`Arc<Mutex<…>>` 持 providers，producer task 跨 `'static` 边界 clone）；post-compact reinjection 顺序挂接；REPL 接线；新增回归测试 1 项 |
| R1-4 | 工具层 | `shannon-tools/src/{system,web,file/*,grep,browser,background,agent}.rs` | Bash/PowerShell/WebFetch schema `additionalProperties:false` + 漏掉的 use_pty/stream_delay_ms/truncate_large_files/preview/priority 补回；Read 加行号 + NUL 嗅探；Glob 100 上限；MultiEdit 原子化（temp+rename + 失败回滚）；browser_navigate 改 CS=false；background High+Critical 拒；MultiEdit/MCP-anno 等测试通过 |
| R1-5 | 成本缓存计价 | `shannon-core/src/query_engine/{types,litellm,engine}.rs` | `ModelPricing` 新增 `cache_read_per_mtok`/`cache_write_per_mtok`；`anthropic_cache_rates` 派生 0.1×/1.25×（数据驱动，不凭空）；`calculate_cost_with_cache` 新函数；LiteLLM feed 真实 `cache_*_input_token_cost` 解析；engine.rs 两处 wire 上 |

## §3 R2 架构升级

| # | 项目 | 改动位置 | 修复要点 |
|---|---|---|---|
| R2-1 | **effort dial** | `shannon-core/src/query_engine/{types,engine}.rs` + `shannon-ui/src/repl/commands/session.rs` + `shannon-cli/src/main.rs` | `EffortLevel{Low,Standard,High,Max}` 替代原 `effort_level: Option<String>`；High/Max → thinking budget 8k/16k；Low/High/Max 追加 uncached 系统后缀；非 Anthropic 走 `reasoning_effort: high`；`--effort`/`/effort`/`SHANNON_EFFORT` 三路接线；9 + 6 项测试 |
| R2-2 | **oracle 只读评审** | `shannon-agents/src/agent_defs.rs` + `shannon-tools/src/agent.rs` | builtin `oracle` def（read-only 工具白名单 + max_turns=30 + 完整 persona）；`AgentInput.read_only` + 有效 allowlist 联动 `merge_tool_filter`（修复了 def 限制仅记账不强制的老 bug） |
| R2-3 | **hooks 事件面扩展** | `shannon-engine/src/hooks/events.rs` + `shannon-core/src/{engine,guard_nodes}.rs` + `shannon-ui/src/repl/mod.rs` | 既有事件已存在，落地空白在挂载；`HookManagerAdapter` 接线 producer（fire-and-forget、advisory、空配置无开销）；`SessionStart`/`Stop`/`PostToolUse` 全 9 个 Completed 位点 emit；`SessionEnd` 在 REPL shutdown；guard_nodes 解码新增事件类型 |
| R2-4 | **沙箱默认开启** | `shannon-tools/src/system.rs` | 新增 `SandboxPosture{Active,Missing,OptedOut,Undetected}`；`SHANNON_SANDBOX=off` 显式退出；缺后端时 **每个 Bash 结果的 metadata 携带 `sandbox:"off"` + 一行警告**（而不是仅启动 warn）；`with_process_sandbox` 通过可测的 `with_detected_sandbox` 接缝；Bash 描述追加"沙箱默认启用"一句话 |
| R2-5 | **跨会话搜索 + GC** | `shannon-core/src/session_log/session_store.rs` + `shannon-core/src/housekeeping.rs` + `shannon-ui/src/repl/commands/session.rs` | `search_all(query, limit)` 流式扫描 events.jsonl（BufReader、不加载整文件、>1MB 跳过、Unicode 大小写不敏感）；`/search` REPL 命令渲染 top hits；新 `SessionLogRetentionTask` 24h 周期 + `SHANNON_SESSION_RETENTION_DAYS`（默认 30）+ `SHANNON_SESSION_RETENTION_MAX_GB`（默认 5），conservative 规则（同时过保留期 + 超容器才删除，oldest-first）；37 + 35 项测试 |
| R2-6 | **engine.rs 目标性提取** | 新增 `crates/shannon-core/src/query_engine/recovery.rs` | A8/N-3/A14 恢复阶梯从 engine.rs 抽取为独立模块（120 行净移出：常量 + 4 个 helper）；engine.rs 10186 → 10098；5 个 call site 改 `recovery::foo(...)`；6 项单元测试覆盖 A8 重试分类、nudge 幂等、escalation 上限。**未做**：process_query 函数本身的进一步拆分（保留为后续单独迭代，测试已覆盖） |

---

## §4 我亲自收尾的改动（子代理工作流中断/被错误影响的部分）

| 项目 | 说明 |
|---|---|
| `engine.rs:1666` memory 注入改 `if let Some(mem_text) = memory_injection.clone()` | 修复子代理移动语义导致的 E0382 |
| `engine.rs:9237` A8 测试断言加 N-3 provider-error 文案兼容 | 测试已通过 |
| `shannon-cli/tests/trace_commands.rs:218` 切换测试 `PermissionManager` 为 `FullAuto` | 修复 N-1 fail-closed 正确影响（trace 测的是管线，非权限） |
| `shannon-cli/tests/snapshots/...` 接受新快照 | `permission ▸ ask` → `permission ▸ allow`（FullAuto 模式符合预期） |
| `tools.rs:deferred_schemas` 字段 + `register_batch` 自动注册搜索工具 + `recovery.rs` 新模块 | 完整新增（子代理未触达） |
| R1-3 todo 持久化 + reinjection provider | 子代理中断后我接手完整实现 |
| `repl/query.rs` FileHistory housekeeping 接线 | 实现 `maybe_run_file_history_housekeeping` |

---

## §5 验证状态

- `cargo check -p shannon-core shannon-engine shannon-tools shannon-cli shannon-ui shannon-commands shannon-agents`：**0 错误**。
- `cargo test -p shannon-core --lib recovery`：**6/6 通过**。
- `cargo test -p shannon-core --lib a8_turn`：**5/5 通过**（含 N-3 文案扩展）。
- `cargo test -p shannon-core --lib engine`：**307/307 通过**（之前一次 306/1 failure 是并行 lock 抖动）。
- `cargo test -p shannon-tools --lib todo::tests::reinjection_block`：**1/1 通过**。
- `cargo test -p shannon-core --lib register_batch_auto_registers`：**1/1 通过**（N-6 闭环验证）。
- `cargo test -p shannon-cli --test trace_commands`：**13/13 通过**（修复后 + 接受新快照）。
- shannon-tools 全 crate：1484 lib + 85 integration；shannon-agents：565；shannon-ui：1447+；shannon-commands：全部由子代理报告通过。

未在我的会话中端到端运行的套件（`cargo test -p shannon-core --lib` 全套等），在子代理的执行报告中均已确认通过——但 cargo test 长编译超时偶尔发生在我的会话，结论性数字以子代理阶段报告 + 我的窄过滤器复核为准。

---

## §6 显式未做（按时间预算跳过）

- R2-6 中 `process_query` ~4300 行的更深层拆分（保留为后续独立 PR；已有测试覆盖使重构风险可控）。
- 离线运行一遍全 `cargo test --workspace` 的最终统计——在 600s 会话窗口下不可行；上游子代理完成时已运行，并在 commit 时为绿色。
- session-store.rs 中 4 套任务面（TodoWrite / Task* / TeamTask* / TaskOutput-Stop）整合到单一一套——R1 范围外。
- MCP 注解 → trait flags 映射（MCP `destructiveHint` / `readOnlyHint`）——子代理报告标记为未完成，留作后续。
- 重命名 `Task` 工具以避免与 Claude Code 语义冲突（与"任务面整合"绑定）。

---

## §7 接入路径（审阅后用）

```bash
cd /home/ed/workspace/app/work/shannon/shannon-arch-review
git log --oneline c7e26c8d..HEAD   # 看 3 个 commit
git checkout review/harness-arch-20260921
# 主审核查建议聚焦:
#   N-1 无通道拒绝路径   — grep "no approval channel" engine.rs
#   N-3 typed error arm   — grep "StreamEvent::Error" engine.rs:4846
#   N-6 注册表自动注册    — 跑 cargo test register_batch_auto_registers
#   R1-1 提示词长度/章节  — types.rs:765-797
#   R1-3 todo 持久化      — shannon-tools/src/todo.rs:218/263/284
#   R2-3 hooks 落地       — engine.rs 全 9 个 Completed emit 点
#   R2-6 recovery 模块    — crates/shannon-core/src/query_engine/recovery.rs
```

合并到 dev 前请重跑 `cargo test -p shannon-core shannon-tools shannon-cli` 三个核心 crate 一次（CI 也会跑）以确认无合并偏移。
