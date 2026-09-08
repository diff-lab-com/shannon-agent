# `/goal` 功能实施方案

- 日期：2026-09-04
- 设计依据：[goal 设计方案 v2](2026-09-04-goal-design.md)
- 方法：TDD（每任务先写测试）；实施顺序即依赖顺序

## Global Constraints

- **存量行为零破坏**：不修改 `/focus`、`/ralph`、`/loop` 的现有行为（互斥守卫除外，Task 5/7）
- 注入块一律 `SystemContentBlock::text`（非 cached），置于 `/focus` 块之后
- 所有用户可见字符串走 `t!("commands.goal.*")`，10 个 locale 文件全量补 key
- 验证命令：`cargo nextest run --workspace || cargo test --workspace -- --test-threads=1`；`cargo fmt --all`；`cargo clippy --workspace --all-targets -- -D warnings`（以仓库 justfile 实际门禁为准）
- `shannon-core` 在 nextest 中属 serial 组，其测试勿依赖并行环境变量
- 命名避让：`target`（远程执行占用）、`plan`/`task`/`focus`（既有语义）；复用 `SOURCE_GOAL_RESUME` 常量则不改其名

---

### Task 1: 引擎层——GoalSpec + 注入块（shannon-core）

**文件**：`crates/shannon-core/src/query_engine/types.rs`、`engine.rs`

1. `types.rs`：`QueryEngineConfig` 新增 `pub goal: Option<GoalSpec>`（`Default` 中为 `None`；参照 `focus_area` 字段 types.rs:673/758）；定义
   ```rust
   #[derive(Debug, Clone, PartialEq, Eq)]
   pub struct GoalSpec { pub objective: String, pub paused: bool }
   ```
2. `engine.rs`：`set_goal(&mut self, goal: Option<GoalSpec>)`（紧邻 `set_focus_area`，engine.rs:748）
3. `engine.rs` `process_query` 组装（`/focus` 块之后，engine.rs:1379 后）：Active/Paused 注入 `## Current Goal` 块（设计 §6.3 全文；Paused 加 PAUSED 首行），`Complete`/`None` 不注入
4. marker 常量放 shannon-core（引擎与 UI 共用）：`pub const GOAL_COMPLETE_MARKER: &str = "GOAL_COMPLETE"; pub const GOAL_BLOCKED_MARKER: &str = "GOAL_BLOCKED";`（types.rs）

```rust
#[test] fn set_goal_roundtrip() { /* engine.set_goal(Some/None) 往返断言，test_set_focus_area 模式 */ }
#[test] fn goal_block_injected_when_active() { /* 组装后 blocks 含 "## Current Goal"、objective、GOAL_COMPLETE 指令 */ }
#[test] fn goal_block_paused_contains_pause_line() { /* paused=true 时含 "PAUSED" */ }
#[test] fn goal_block_absent_when_none_or_complete() { /* None 与 GoalSpec 语义外状态不注入——经 UI 侧映射，core 只认 Option<GoalSpec> */ }
#[test] fn goal_block_is_non_cached_text() { /* 断言该块为 SystemContentBlock::text 变体而非 cached */ }
```

### Task 2: GoalState 状态机（shannon-ui）

**文件**：`crates/shannon-ui/src/repl/state.rs`

1. `GoalStatus { Active, Paused, Complete }`（Serialize/Deserialize，小写字符串序列化）
2. `GoalState { objective, status, iterations, max_iterations }`；`Default max_iterations = 25`
3. `ReplState` 新增 `pub goal: Option<GoalState>`（`focus_area` 旁，state.rs:262 区域），构造器默认 None（state.rs:580 区域）
4. UI↔core 转换助手（评审 R9 修正）：
   ```rust
   impl GoalState {
       /// 引擎注入映射：Active → Some(GoalSpec{paused:false})；Paused → Some(paused:true)；
       /// Complete → None（已完成目标不注入，不占上下文）
       fn to_spec(&self) -> Option<shannon_core::query_engine::GoalSpec>
       /// status 字符串未知值降级 Paused（不 panic、不丢 objective）
       fn from_stored(stored: StoredGoal) -> Self
   }
   ```

```rust
#[test] fn goal_status_serde_roundtrip() { /* 小写字符串往返 */ }
#[test] fn goal_state_default_max_is_25() { /* */ }
#[test] fn from_stored_unknown_status_degrades_to_paused() { /* */ }
#[test] fn to_spec_maps_active_paused_complete() { /* Active→Some(paused:false)；Paused→Some(true)；Complete→None */ }
```

### Task 3: 命令层——/goal 解析与处理器（shannon-ui）

**文件**：新建 `crates/shannon-ui/src/repl/commands/goal.rs`（remote.rs 模式）

```rust
pub(crate) enum GoalAction { Show, Set { objective: String, max_iterations: usize }, Pause, Resume, Clear }
pub(crate) fn parse_goal_args(args: &str) -> GoalAction          // 纯函数
pub(crate) fn goal_completion_marker(msg: &str) -> Option<GoalMarker>  // 纯函数：最后一个非空行判定
pub(crate) enum GoalMarker { Complete, Blocked(String) }
pub(crate) fn handle_goal(repl: &mut Repl, args: &str) -> Result<()>
pub(crate) fn check_goal_continuation(repl: &mut Repl) -> bool
// R14 修正：Continue 路径压入 queued_messages（扁平排水，栈深 O(1)），
// 不得在 handle_query 框架内直接 submit_input（递归 → 栈溢出）
pub(crate) fn continuation_prompt(goal: &GoalState) -> String     // 纯函数，设计 §6.4 文案
pub(crate) fn save_goal_sidecar(repl: &Repl)                      // l0_store().save_sidecar，merge 语义
```

语义要点：
- 别名：`clear|off|stop|cancel|reset|none` → Clear；`status` → Show；空参 → Show
- `--max N` 解析同 ralph 风格（`strip_prefix("--max ")`）；N=0 表示不限；非法 N 忽略用默认
- Set 守卫：`ralph_state.is_some() || loop_state.is_some()` → `set_error` + `t!("commands.goal.conflict_loop")`
- Set/Pause/Resume/Clear 后立即 `save_goal_sidecar`
- Show 显示 status/iteration/max/objective；Complete 态标注 "completed"
- `check_goal_continuation`：仅 `Active` 响应；marker 判定 → Complete（通知 + sidecar）/ Blocked(String)（Paused + 原因通知 + sidecar）；未完成 → `iterations += 1`，`max>0 && iterations>=max` → Paused + 通知，否则 `iterations += 1` 并把 `continuation_prompt` 压入 `queued_messages`（R14：由 submit_input 扁平排水接续，禁止框架内递归 submit）→ true

```rust
#[test] fn parse_goal_empty_is_show() { /* */ }
#[test] fn parse_goal_clear_aliases() { /* clear/off/stop/cancel/reset/none 六别名 */ }
#[test] fn parse_goal_set_with_max() { /* "--max 5 xxx" → Set{max:5}；"xxx" → Set{max:25}；"--max 0 xxx" → Set{max:0} */ }
#[test] fn parse_goal_max_invalid_uses_default() { /* "--max abc xxx" */ }
#[test] fn marker_last_line_exact_match_case_insensitive() { /* "...\nGOAL_COMPLETE\n" 命中 */ }
#[test] fn marker_mid_text_not_detected() { /* marker 出现在中间行/代码块内不判定 */ }
#[test] fn marker_blocked_extracts_reason() { /* "GOAL_BLOCKED: need creds" → Blocked("need creds") */ }
#[test] fn marker_prefix_junk_not_complete() { /* "GOAL_COMPLETE-ish" 不判 Complete，可判 Blocked 前缀仅限 GOAL_BLOCKED */ }
#[test] fn handler_set_stores_state_and_saves_sidecar() { /* Repl::new + HomeGuard；state.goal==Some；meta.json 含 goal */ }
#[test] fn handler_set_rejected_when_ralph_active() { /* */ }
#[test] fn handler_pause_resume_clear_transitions() { /* 三态迁移断言 */ }
#[test] fn continuation_only_when_active_and_incomplete() { /* Complete→false；Paused→false；无 marker→iterations+1 且 prompt 入 input */ }
#[test] fn continuation_max_reached_pauses() { /* iterations 达 max → Paused，不再续跑 */ }
#[test] fn continuation_complete_marker_stops_and_notifies() { /* */ }
```

### Task 4: 命令注册（shannon-ui commands/mod.rs）

1. `mod goal;`（文件头模块声明区，mod.rs:3-15）
2. `repl_only_commands` 数组加 `"goal"`（"focus" 条目旁，mod.rs:444 区域）
3. match 分发臂：`"goal" => goal::handle_goal(repl, args)?`（"focus" 臂旁，mod.rs:549 区域）
4. 现有守卫测试 `every_dispatch_match_arm_is_reachable_from_the_gate`（mod.rs:824）作为回归

```rust
#[test] fn goal_dispatch_reachable_from_gate() { /* 复用守卫测试逻辑，显式断言 parse 可达（守卫测试本身已覆盖） */ }
```

### Task 5: 查询链路接线（shannon-ui query.rs）

1. 入口同步（query.rs:296-299 旁）：
   ```rust
   query_engine.set_goal(repl.state.goal.as_ref().and_then(|g| g.to_spec()));
   ```
2. 完成挂钩（query.rs:1509 旁，**在 `check_loop_iteration` 之前**）：
   ```rust
   let goal_continued = super::commands::goal::check_goal_continuation(repl);
   let loop_continued = goal_continued || super::commands::check_loop_iteration(repl);
   if !loop_continued {
       super::commands::check_ralph_iteration(repl);
   }
   ```

```rust
#[test] fn goal_set_before_query_reaches_engine_config() { /* handle_query 前后 config.goal 断言（可经 set_goal 直接单测 + 集成 smoke） */ }
```

### Task 6: 持久化与恢复（shannon-core + shannon-ui）

**文件**：`crates/shannon-core/src/session_log/session_store.rs`、`crates/shannon-ui/src/repl/session.rs`、`commands/session.rs`

1. `SessionSidecar` 新增：
   ```rust
   #[serde(default, skip_serializing_if = "Option::is_none")]
   pub goal: Option<StoredGoal>,
   ```
   `StoredGoal { objective: String, status: String, iterations: usize, max_iterations: usize }`（Default + serde）
2. 恢复：`/resume` 与自动恢复路径读取 `store.sidecar(session_id).goal` → `GoalState::from_stored` → `repl.state.goal`（恢复后仅锚定，不自动续跑——设计 §6.6）
3. query.rs 完成路径已有的 save_sidecar 调用携带 `goal: GoalState→StoredGoal`（若 state 有）

```rust
#[test] fn stored_goal_serde_roundtrip() { /* */ }
#[test] fn sidecar_without_goal_omits_field() { /* skip_serializing_if 生效，旧 meta.json 零 diff */ }
#[test] fn sidecar_unknown_status_string_loads_as_paused() { /* 容错 */ }
#[test] fn resume_restores_goal_state() { /* /resume handler 集成：sidecar 有 goal → state.goal 还原 */ }
```

### Task 7: /ralph、/loop 对偶守卫（shannon-ui commands/loop_engine.rs）

`handle_ralph` 与 `handle_loop` 启动分支加守卫：goal `Active|Paused` 存在时拒绝启动（`set_error`，提示先 `/goal clear|pause`）。`stop/status` 分支不动。

```rust
#[test] fn ralph_rejected_while_goal_set() { /* */ }
#[test] fn loop_rejected_while_goal_set() { /* */ }
```

### Task 8: UI——pill + i18n + help

1. `RenderContext` 新增 `pub goal: Option<&'a crate::repl::state::GoalState>`（widgets/mod.rs:89 区域；`minimal()` 构造器补 None）；render.rs ctx 填充处（render.rs:201-243 区域）挂 `repl.state.goal`
2. status_bar.rs 远程 pill 之后（status_bar.rs:251 后）追加 goal pill：
   - Active：`[◎ {trunc(objective,24)} {n}/{max}]` theme.primary；max=0 不显示 max
   - Paused：`[⏸ …]` theme.warning；Complete：`[✓ …]` theme.success
   - 截断用 `truncate_visual`（render.rs:1055）；a11y 模式输出纯文本无图标
3. i18n：10 个 locale（en/zh/ja/ar/bn/es/fr/hi/pt/ru）加 `commands.goal.*` 9 key：`set`、`current`、`current_none`、`cleared`、`paused_blocked`、`paused_max`、`resumed`、`complete`、`conflict_loop`
4. help.rs `categorize_commands`：`HelpCategory::System` 加 `"goal"` 条目（arg_hint `[<objective>|show|pause|resume|clear] [--max N]`，related `["focus","plan","ralph"]`），对齐 focus 条目格式（help.rs:1053）

```rust
#[test] fn goal_pill_renders_for_active_goal() { /* Buffer 断言含 objective 片段与颜色风格 */ }
#[test] fn goal_pill_absent_without_goal() { /* */ }
#[test] fn categorize_commands_contains_goal() { /* help.rs 现有 categorize 测试模式 */ }
```

### Task 9: headless `--goal`

**文件**：`crates/shannon-cli/src/main.rs`

1. `Cli` 加 `--goal <OBJECTIVE>`（`--prompt` 区，main.rs:458 区域）
2. headless 引擎构建处（main.rs:1448-1466 区域）：`config.goal = Some(GoalSpec { objective, paused: false })`
3. 注入-only：headless 不做续跑（`--max-turns` 已有独立语义），`--help` 文案说明

```rust
#[test] fn goal_flag_parses() { /* clap 解析断言 */ }
```

### Task 10: 文档与全量验证

1. CHANGELOG.md 加条目（对齐现有格式）
2. 设计/实施/调研三文档已在本次提交
3. 全量：`cargo fmt --all` → `cargo clippy --workspace --all-targets` → `cargo nextest run --workspace`
4. 手工冒烟清单（TUI）：/goal 设置 → pill 出现 → 普通消息注入生效（日志）→ /goal pause/resume/clear → /ralph 互斥 → /resume 恢复 pill

---

## 依赖顺序

Task 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10；Task 1/2 可并行起步；Task 6 依赖 2/3；Task 8 依赖 2。

## 明确不做（回显设计 §2 非目标）

update_goal 工具契约、预算记账面板、check-in 调度、anti-spin、desktop UI。
