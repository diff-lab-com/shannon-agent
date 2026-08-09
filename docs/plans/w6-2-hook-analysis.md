# W6-2 · 现有 Hook / 快照行为梳理(Step 1 产出)

> **Track**: Wave 6 · **Date**: 2026-08-09
> **Status**: Step 1 完成(梳理)→ 待实施 Phase A/B
> **Parent**: [w6-2-p2-6-auto-commit-undo.md](./w6-2-p2-6-auto-commit-undo.md) §2 / §5 step 1
> **Branch**: `feat/w6-2-auto-commit-undo`(基于 `dev` @ `9ce9c09f`)

---

## 结论(TL;DR)

梳理后,W6-2 两个子任务的实际起点与原计划假设**显著不同**,整体被大幅 de-risk:

1. **文件级快照存储层已完整建成,但处于 orphan 状态**(从未被调用)。`history.rs` 的 `FileHistoryManager` 提供 snapshot / diff / rollback / cleanup / quota / 去重 + ~25 个测试,但没有任何 file tool 或命令引用它。
2. **file tools 是无状态的**——`Tool::execute(&self, input)` 不带 shared context。注入 `FileHistoryManager` 的正确方式是给 Write/Edit/MultiEdit 加一个 `Option<Arc<Mutex<FileHistoryManager>>>` 字段 + `with_history()` 构造器,**该模式已被同 crate 的 `PlanManager` 采用**(`lib.rs:533`)。
3. **`AutoCommitTool` 已注册**(`lib.rs:495`)——auto-commit 的"tool 形态"已存在(LLM 主动调用),不是 greenfield。
4. **原计划 §3.1 的 dev-workflow auto-commit 痛点(TD-5)无法由 Shannon-engine 解决**——那个 hook 是用户的 Claude Code harness hook,不在本 repo。已决定 **C(auto-commit)defer**,见 §4。

**修订估时:~5–6d(原 1–1.5w)**,因为快照存储层已完成。

---

## 1. 现有行为梳理

### 1.1 文件级快照存储 —— 建成但 orphan

**位置**:`crates/shannon-tools/src/file/history.rs`(1433 行,含测试)

**已实现能力**:

| 组件 | 能力 |
|---|---|
| `FileHistoryManager` | record / retrieve / diff / rollback / cleanup;持久化到 `~/.shannon/file_history/`(index + 每 snapshot 一个 JSON) |
| `FileSnapshot` | point-in-time content + SHA-256 去重 + `FileOperation`(Create/Edit/Delete/Read) |
| `FileHistory` | 每文件 snapshot 列表 + `max_snapshots` 数量驱逐 |
| `FileDiff` / `DiffHunk` | LCS-based 行级 diff,unified-diff 输出 |
| `rollback()` | 恢复到指定 snapshot(把恢复本身记为一次 Edit) |
| `cleanup_old_snapshots()` | 按数量驱逐(超 `max_snapshots` 删最旧) |
| quota | `max_total_history_mb`(默认 100MB),`check_storage_quota()` |

**证据(orphan)**:`grep -rn 'FileHistoryManager|record_snapshot'` 仅命中 `history.rs` 自身 + `shannon-tools/src/lib.rs:93-94`(re-export)。**没有任何 file tool、`tool_execution`、命令调用它**。

**缺失**(Phase A 要补):
- **未被 wire** 到 Edit/Write/Delete 的 pre-modify 路径。
- **无 time-based TTL**——只有数量 + 配额,没有"7 天后过期"。
- 无 Delete 工具的快照(bash 删除走 Bash tool,不经 file tool)。

### 1.2 Undo 命令 —— 不存在

- `/undo`:**不存在**(`builtin/` 无 `undo.rs`)。
- `/rewind`:REPL **消息级**命令,在 `shannon-ui/src/repl/commands/session.rs`(回退对话消息),不是文件级。`shannon-commands/src/builtin/help.rs:641` 仅文档引用。
- 现状:只有消息级回退,无文件级回退。

### 1.3 Auto-commit —— Tool 已存在,但不是 PostToolUse hook

- **`AutoCommitTool`** 已在 `register_default_tools_with_project_dir_ex`(`lib.rs:495`)注册。它是 **LLM 主动调用的 tool**,不是"每次 Edit 自动触发"的 hook。
- §3.1 设想的"PostToolUse 自动 commit + 批量窗口"是 **不同语义**——需要新建一个 triggered routine(PostToolUse hook → 收集 diff → 批量 commit),与现有 `AutoCommitTool` 整合。
- Shannon-engine 的 PostToolUse hook 基础设施存在:`crates/shannon-engine/src/hooks/` + `crates/shannon-core/src/triggered_routines.rs` + `tool_execution.rs` 触发点。

### 1.4 dev-workflow auto-commit hook(TD-5 痛点)—— 不在本 repo

- 计划锚点 `scripts/hooks/post-tool-use-commit` **在 repo 不存在**(根目录无 `scripts/hooks/`,只有 `desktop/scripts/hooks/` 是 git pre-push)。
- `.claude/settings.local.json` 只有 5 条 permission,无 hook 注册。
- 那个"Edit/Write 后自动 commit + 拆分多文件"的 hook 是 **用户个人的 Claude Code harness hook**,不影响 Shannon 用户。
- ∴ **在 Shannon-engine 里建 auto-commit 解决不了 TD-5 的原始痛点**——两者是不同 hook 系统。这是 defer C 的核心理由。

---

## 2. Phase A 设计(注入方案,已验证)

### 2.1 约束

`Tool` trait(`shannon-tool-interface`)的 `execute(&self, input: Value)` 只带 `&self` + JSON,**无 shared context**。file tools(`WriteTool`/`EditTool`/`MultiEditTool`)仅持 `description` + `sandbox: PathSandbox`。

→ 不能把 `FileHistoryManager` 经 trait 传进来(改 trait = stable interface 破坏,blast radius 覆盖所有 tool)。

### 2.2 方案:字段注入(同 `PlanManager` 先例)

```rust
// file/mod.rs
pub struct WriteTool {
    description: String,
    sandbox: PathSandbox,
    history: Option<Arc<Mutex<FileHistoryManager>>>,  // 新增
}

impl WriteTool {
    pub fn with_history(mut self, history: Arc<Mutex<FileHistoryManager>>) -> Self {
        self.history = Some(history); self
    }
    // with_sandbox_and_history(...) 组合构造器,供 register fn 用
}
```

`execute()` 在调用 `write::execute(input)` **之前**快照原内容:

```rust
if let Some(mgr) = &self.history {
    if let Ok(old) = tokio::fs::read_to_string(&input.file_path).await {
        let _ = mgr.lock().await.record_snapshot(
            Path::new(&input.file_path), &old, FileHistoryOperation::Edit);
    }
    // 新文件(Create):无 old content,跳过或记 empty Create
}
```

- **默认 `None` = 现行为**(不快照)→ opt-in、向后兼容、零行为回归。
- `Arc<Mutex<>>` 因 `record_snapshot` 是 `&mut self`(内存 cache + 磁盘写)。file tool 非热路径,Mutex + 同步 I/O 可接受(后续可 async-ify)。

### 2.3 注入点(经 `PlanManager` 先例验证)

`lib.rs:533` 已用同样模式:
```rust
let plan_manager = PlanManager::new();
registry.register(Box::new(EnterPlanModeTool::with_manager(plan_manager.clone())))?;
```

→ 在 `register_default_tools_with_project_dir_ex`(`lib.rs:460`,项目级 sandbox 版,主入口)里:
```rust
let history = Arc::new(Mutex::new(FileHistoryManager::new(FileHistoryConfig::default())));
registry.register(Box::new(WriteTool::with_sandbox(sandbox.clone()).with_history(history.clone())))?;
registry.register(Box::new(EditTool::with_sandbox(sandbox.clone()).with_history(history.clone())))?;
registry.register(Box::new(MultiEditTool::with_sandbox(sandbox.clone()).with_history(history.clone())))?;
```
另外两个 register fn(`register_default_tools` 179、`register_default_tools_with_project_dir` 310)按需跟进。

---

## 3. TTL 设计(A.1)

`history.rs` 现有:count 驱逐(`max_snapshots`)+ 配额(`max_total_history_mb`)。缺 **time-based TTL**。

A.1 增量(自包含在 `history.rs`):
- `FileHistoryConfig` 加 `ttl: Option<Duration>`(默认 `Some(7d)`)。
- `FileSnapshot` 已有 `timestamp: DateTime<Utc>`。
- 新方法 `cleanup_expired(&mut self) -> Result<usize>`:删 `timestamp < Utc::now() - ttl` 的 snapshot。
- `record_snapshot` 末尾 opportunistically 调一次(或暴露给定期清理 routine)。
- 测试:用可注入"now"或 backdated timestamp 验证过期删除。

---

## 4. 修订实施计划(C deferred)

| 阶段 | 内容 | 估时 | 状态 |
|---|---|---|---|
| A.1 | `history.rs` 加 time-based TTL + 测试 | ~0.5d | ✅ `9a9d3ac3` |
| A.2 | Write/Edit/MultiEdit 加 `history` 字段 + `with_history()` + `execute()` pre-modify 快照 + 测试 | ~1.5d | ✅ `ba2a01f6` |
| A.3 | `lib.rs` register fn 注入 manager(2 个 project-scoped fn;`register_default_tools` no-sandbox 不接,避免测试污染 HOME) | ~0.5d | ✅ `2dad59ac` |
| A.4 | config(开关 / history dir / TTL);默认开,可禁用 | ~0.5d | ✅ `c83f761d` |
| B.1 | **命令统一 + per-file 回退**:`/undo` / `/checkpoint` 收敛为 `/rewind` 别名(同一 `handle_rewind`,Claude-Code-aligned 机制);`/rewind <path>` 经 `FileHistoryManager.rollback()` + 写盘做 per-file 内容快照回退(覆盖前确认 / `--yes` 跳过);help 元数据 + 8 个单测;`code`/`both` 多文件模式本阶段仍走 git-checkpoint(过渡态) | ~2d | ✅ 本次 |
| B.2 | **turn-tagged 内容快照回退**:`FileHistoryManager` 增 `record_turn_snapshot` / `rewind_file_to_turn`(按 `turn_index` 标记的内容快照,非独立 manifest);`query.rs` 在 turn 边界捕获 post-turn 快照;`/rewind code\|both <n>` 经 `run_code_rewind` 回退到目标 turn 的文件内容(Restore / Delete / NoChange);**停掉** `engine.rs` 每次 Edit/Write/Bash 的 git auto-checkpoint + `tool_execution.rs` 死路径;**gut** `checkpoint.rs`(删 `create_checkpoint` / `revert_to` / `undo_last` / `preview_revert` / `RestoreMode` / `RevertPreview` / `FileChangePreview` + 配套集成测试;保留 `TurnCheckpoint` / `record_turn` / `list_checkpoints` / `discard_last` + 持久化,驱动 `/rewind <n>` 会话回退 + history 列表) | ~1.5d | ✅ 本次 |
| C | auto-commit —— **DEFERRED**(见下) | — | ⏸️ |

> **B.1 设计决策(2026-08-09)**:经调研 + 用户确认,采纳 Claude-Code-aligned 方案 —— canonical 命令 `/rewind`(`/undo`、`/checkpoint` 为别名,行为完全一致);per-file 回退基于 `FileHistoryManager` 内容快照(非 git)。详见记忆 `existing-undo-is-destructive-git-reset`。

> **B.2 设计决策(2026-08-09)**:实施时核实,B.1 笔记里"保留 `checkpoint.rs` 作库原语(供 `AutoCommitTool` 等)"的前提**不成立** —— `AutoCommitTool` 并未使用 `CheckpointManager`,且 engine 侧两个 `CheckpointManager` 实例本就断连(REPL 的 `new()` 记录合成空哈希 checkpoint,`code`/`both` 回退早已失效)。故采纳 **gut** 方案:删除 git-commit / git-reset 原语,`/rewind code\|both` 改由 `FileHistoryManager` 的 turn-tagged 内容快照驱动(post-turn 捕获,`turn_index` 与 `record_turn` 对齐,经 `list_checkpoints()[index].turn_index` 解析列表位置≠turn_index 的偏差)。`checkpoint.rs` 保留会话级 turn 元数据(非 git),供 `/rewind <n>` 会话回退 + history 列表。

### A.4 配置开关(env vars,默认开)

`FileHistoryConfig::from_env()` 在 register 时读取,遵循 CLAUDE.md 的 `SHANNON_*` 约定(同 `web.rs` 先例):

| 环境变量 | 作用 | 默认 |
|---|---|---|
| `SHANNON_FILE_HISTORY` | `0`/`false`/`off`/`no` 关闭快照(tools 不挂 manager = pre-W6-2 行为) | 开启 |
| `SHANNON_FILE_HISTORY_DIR` | 覆盖 history 目录 | `~/.shannon/file_history` |
| `SHANNON_FILE_HISTORY_TTL` | 覆盖 TTL(秒);`0` = 关闭时间过期(仅 count + quota) | `604_800`(7d) |

未设/不可解析 → 保持默认。TOML 层接入(从 `.shannon.toml` `[file_history]` 读)留待后续(非 A.4 范围)。

### C(auto-commit)为何 defer

1. **解决不了原始痛点**:TD-5 的 dev-workflow hook 是 Claude Code harness hook(§1.4),Shannon-engine auto-commit 是另一套系统。
2. **Tool 已存在**:`AutoCommitTool`(`lib.rs:495`)已提供 LLM 主动调用的 commit 能力;"PostToolUse 自动 + 批量窗口"是增量语义,需求未明。
3. **高风险**:自动 commit 改 git 历史(向外 / 难撤销),需谨慎 opt-in + 批量窗口正确性。
4. **替代(A/B 后再评估)**:增强现有 `/commit`(`builtin/commit.rs`)用 LLM 从 staged diff 生成 conventional message(按需,非自动,~1d)。

---

## 5. 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| `Arc<Mutex<FileHistoryManager>>` 在 async 里同步 I/O 阻塞 runtime | 低 | 中 | file tool 非热路径;快照写小;后续 `spawn_blocking` |
| 快照存储膨胀 | 中 | 中 | 已有 count + quota;A.1 加 TTL |
| 默认开启快照→磁盘写放大,用户无感 | 中 | 低 | A.4 config 默认开 + 文档;可禁用 |
| MultiEdit 多文件部分失败后快照不一致 | 低 | 中 | MultiEdit 已 atomic(全成或全不成);快照在 execute 前 |

---

## 6. 验收(Phase A+B)

- [ ] Edit/Write/MultiEdit 执行前在 `~/.shannon/file_history/` 记下 pre-modify snapshot(含 hash 去重)。
- [ ] TTL 到期的 snapshot 被清理;数量超限按现驱逐逻辑删最旧。
- [ ] `/undo` 恢复最近一次文件变更(或带参数恢复 N 次/指定文件);覆盖未提交工作前确认。
- [ ] `with_history` 默认 `None` 时行为与现状字节一致(回归零)。
- [ ] history.rs / file tools / undo 新增测试全覆盖;`cargo clippy -p shannon-tools -- -D warnings` + `shannon-commands` 通过。

---

*证据日期:2026-08-09,基于 `dev` @ `9ce9c09f`。grep / 文件锚点见各节。*
