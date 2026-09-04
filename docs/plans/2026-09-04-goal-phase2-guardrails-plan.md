# 自主循环护栏体系改进方案（goal / ralph / loop × Phase 2）

- 日期：2026-09-04
- 分支：`feat/goal-phase2`（基于 dev @ 5ed493d9）
- 状态：**4 批实施完成**（commits b414314b / 5a09506e / df4beb32 / 38c07272，均在 `feat/goal-phase2` 分支）
- 前置：[goal 竞品调研](../research/2026-09-04-goal-competitive-research.md)、[goal 设计 v3](2026-09-04-goal-design.md)（§11 R13/R14）

---

## 1. 任务与范围

1. 学习 Claude Code / Codex 的 goal 设计理念：**不再以轮数上限作为主要护栏**
2. 深度分析 `/ralph` 与 `/loop` 当前 10 轮上限是否合理，对标竞品
3. 结合 Phase 2，制定 goal/ralph/loop 三循环统一的护栏改进与实施方案

不在本方案内：desktop 端 UI、goal 的多 goal 并发、routines/cron 调度本身的重构。

## 2. 竞品设计理念深拆：护栏的五个哲学

对 Claude Code（2.1.139+ `/goal`）与 Codex CLI（0.128+ Goals，`codex-rs/ext/goal/` 开源）的机制逐条对齐后，两者的共同点不是"某个具体护栏"，而是**护栏的作用维度**：

| # | 哲学 | Claude Code 的做法 | Codex 的做法 | 对轮数上限的态度 |
|---|---|---|---|---|
| Φ1 | **终止条件即主护栏**——护栏的核心是"完成必须被证明"（completion audit），而不是"跑多久必须停" | evaluator 每轮判定完成条件；不可恢复错误自动清除 goal | `update_goal` 工具契约：模型只能标 complete/blocked，blocked 需同一阻塞连续 3 轮 | 轮数上限不存在 |
| Φ2 | **预算而非轮数**——成本护栏用 token/时间记账表达，语义是"我愿意为这个目标花多少" | overlay 实时显示 elapsed/turns/tokens | `token_budget`/`time_used` 记账，`budget_limited` 为终态（需用户介入） | 轮数上限不存在 |
| Φ3 | **进展检测替代总轮数**——漂移的信号是"没有可验证进展"，不是"轮数多了" | check-in 只在阻塞/空闲时触发 | anti-spin：续跑轮无工具调用则抑制下次续跑；no-progress 分类（progress / verified wait / no progress） | 轮数上限不存在 |
| Φ4 | **节奏而非终止**——阻塞时不砍循环，而是退避回访 | 空闲 30min 后 check-in，30m→1h→2h 退避，每 goal 最多 3 次，用户消息重置额度 | blocked 状态即暂停等用户 | 轮数上限不存在 |
| Φ5 | **失败安全与透明**——致命错误清理状态；消耗全程可见 | auth 失效/credit 耗尽/context 溢出 → goal 自动清除并提示 | 中断自动转 paused；Summary 面板显示 Status/用量/预算 | — |

**一句话总结**：两家都用「**可验证终止 + 预算记账 + 进展检测 + 退避节奏**」四维护栏，轮数上限在他们那里连兜底都不算。轮数上限的固有缺陷是**单位错配**：一轮 ≠ 一个单位的进展，也不 = 一单位的成本——它既惩罚不了原地打转（每轮都调工具的假进展），又误杀长任务的合法深潜（一轮做很多事）。

有一个重要的 **in-repo 先例**：Shannon 的 `auto_test`（`crates/shannon-core/src/auto_test.rs`）已经采用三维护栏——`max_iterations`（硬上限）+ `total_timeout_secs`（墙钟）+ `no_progress_strikes`（同一失败重复 N 次即退出，默认 3）。即 Shannon 自己在别的自主循环里已经认可"进展打击 > 总轮数"。本方案是把这套理念推广到三个循环，而不是引入外来发明。

## 3. `/ralph` 与 `/loop` 现状机制（逐行核实）

| | `/loop`（loop_engine.rs:122） | `/ralph`（loop_engine.rs:272） |
|---|---|---|
| 终止条件 | **无**——每轮完成后无条件再注入，直到 `max_iterations` | 关键词判定：最后一条消息 `contains(DONE/FIXED/COMPLETE/…)`（大小写不敏感 substring） |
| 默认上限 | 10（`0` = 不限） | 10（`--max N`，`0` = 不限） |
| 进展检测 | 无 | 无 |
| 预算记账 | 无 | 无 |
| 持久化 | 无（会话恢复即丢） | 无 |
| 续跑方式 | 完成挂钩内直接 `submit_input`（**递归嵌套，同 goal R14 缺陷**；`--max 0` 时栈深无界） | 同左 |
| 完成判定可靠性 | — | 差：消息正文提到 "done" 即误停；代码块/引用里的关键词同样误触 |

## 4. 10 轮上限是否合理？——不合理，且是"双重不合理"

**对标竞品的"迭代循环"类功能**（没有直接等价物，最接近的机制）：

| 竞品机制 | 机制 | 上限策略 |
|---|---|---|
| Claude Code `/loop`（bundled skill） | 按间隔定时重跑 prompt（`/loop [interval] [prompt]`） | 无轮数上限；由调度与用户停止控制 |
| Cursor `/loop` skill（3.5） | 同上，定时循环 | 无轮数上限 |
| 原版 Ralph Wiggum 技术（社区，Shannon /ralph 的命名来源） | `while true; do claude -p "<task>"; done` 的 bash 循环 | 无上限；操作者肉眼监督 |
| OpenHands 主循环 | agent loop | `max_iterations=100` **+ stuck detector（独立的无进展检测，可在上限前止损）** |
| LangGraph 递归图 | 图步进 | `recursion_limit=25`（步数语义，非 agent 轮） |
| AutoGPT continuous mode | 自主循环 | 硬上限 100，且官方文档明示不推荐使用 |

**分析结论**：

1. **作为进度护栏，10 太松**：10 轮里模型可以每轮都调工具、每轮都零进展，循环照跑到顶——轮数上限检测不了漂移。ralph 的关键词 grep 判据（P1 缺陷，调研报告 §4.2）还会让它"假完成"早停，两个方向都不可靠。
2. **作为成本护栏，10 又太紧**：一个真实的"任务迭代"（如"继续重构直到测试绿"）合法深潜 15-20 轮很常见；10 轮截断后用户只能手动重启循环，体验破碎。
3. **与竞品哲学相悖**：竞品用进展/预算做**主动**护栏，总轮数至多是**兜底**。10 在 Shannon 却是唯一护栏（主动护栏缺位）。
4. **佐证**：OpenHands 敢设 100 是因为有 stuck detector 配合；没有任何竞品把"小总轮数"当作漂移对策。

**结论**：10 不应废除而应**降级**——从"唯一护栏"降为"兜底上限"，并把默认值放宽到与 goal 对齐的 25（作用域同级），同时补上主动护栏（§6 的 P2.1/P2.2）。真正决定"何时停"的应是进展与预算信号。

## 5. Phase 2 目标形态：三循环统一护栏

goal、ralph、loop 是同一个"自主循环"问题的三个实例（goal=终止条件驱动、ralph=关键词完成驱动、loop=无终止迭代），应共享同一套护栏原语，而不是各自为政。

### 5.1 统一护栏状态 `LoopGuard`

```rust
// 新模块（建议 crates/shannon-core/src/loop_guard.rs，三个循环都可依赖）
pub struct LoopGuard {
    // 兜底层
    pub max_iterations: usize,        // 兜底上限（保留，见 §6 迁移策略）
    pub iterations: usize,
    // 进展层（主动护栏，本方案的核心增量）
    pub no_tool_turns: usize,         // 连续"零工具调用"的续跑轮数（anti-spin）
    pub stall_strikes: usize,         // 连续"无进展"打击数（no_progress_strikes 模式）
    pub max_stall_strikes: usize,     // 默认 3 —— 对齐 auto_test::no_progress_strikes 与 Magentic-One max_stalls
    // 预算层
    pub started_at: DateTime<Utc>,
    pub token_baseline: u64,          // 循环启动时的 billing 快照
    pub cost_baseline: f64,
    pub max_budget_usd: Option<f64>,  // None = 不限（跟随全局 monthly_budget 告警）
    // 节奏层
    pub checkins: usize,              // check-in 已发生次数（≤3，Claude Code 模式）
}

pub enum GuardVerdict { Continue, WarnNoProgress, PauseBlocked(String), BudgetLimited, StallPaused }
```

每次续跑判定 = `guard.evaluate(last_turn: TurnFacts) -> GuardVerdict`，TurnFacts = `{ had_tool_calls: bool, progress_class: ProgressClass, cost_delta }`。三个循环的 `check_*_iteration` 各自消费 verdict，但停止/告警语义统一。

### 5.2 实施项（P2.1–P2.6，依赖顺序即编号顺序）

| 项 | 内容 | 关键设计 | 验收标准 | 规模 |
|---|---|---|---|---|
| **P2.0** | **ralph/loop 递归修复**（goal R14 同款） | `check_loop_iteration`/`check_ralph_iteration` 的续跑从 `submit_input` 改为 `queued_messages.push`；附带把 ralph/loop 状态纳入 sidecar 持久化（补 P2 差距） | 循环续跑任意轮栈深 O(1)（回归测试同 goal）；resume 恢复活跃循环 | S |
| **P2.1** | **anti-spin（零工具调用检测）** | 数据源：REPL 侧扫描本轮（最后一条 user 消息之后）的 tool 消息（`ChatMessage.tool_name` 已有）；判定并入续跑决策纯函数。第 1 次零工具 → 注入警告（"没有可验证动作"）；**连续 2 次 → 自动暂停**（Codex 是抑制一次，我们取更强的一档，因为 Shannon 无预算护栏兜底） | 单元：连续零工具轮计数与暂停判定；集成：构造零工具回复触发暂停 | S |
| **P2.2** | **stall strikes（无进展打击）** | 续跑 prompt 增加自报分类（progress / verified wait / no progress——Codex continuation 模板原样移植），与 P2.1 的确定性检测**取或**（模型自报 no progress 或检测器判零工具都算 strike，好进展减一、下限 0）；`stall_strikes ≥ 3` → 暂停 + 输出根因重规划提示（Magentic-One 式："解释上次失败的根因，新计划须避免重复同样的错误"） | 单元：strike 加减与阈值；注入文案含重规划指令 | M |
| **P2.3** | **预算记账（token/成本/时长）** | `LoopGuard` 启动时从 `BillingManager` 取快照（`get_period_summary` 已有 per-model 汇总）；`/goal <obj> --budget $5` / `--tokens 500k` 设限；每次续跑评估 `cost_delta ≥ max_budget_usd` → **BudgetLimited 终态**（区别于 Paused：需要用户显式 `/goal resume` 或提高预算）；pill 扩展显示已耗预算（对标 Codex Summary 面板：Status/Objective/Tokens/预算） | 单元：快照差值、终态判定；UI：pill/面板显示用量 | M |
| **P2.4** | **check-in 退避（阻塞回访）** | **实施推迟至独立 PR**：RoutineManager 当前只有 cron/interval 调度，无原生一次性 at-time 调度；需要新增 `RoutineManager::schedule_once(at: Instant, payload)` 与主循环驱动，整体工程量大于单批容量。在 `feat/goal-phase2` 分支的设计文档中保留为待办项。 | — | — |
| **P2.5** | **goal 工具契约（get_goal/update_goal）** | Codex spec 移植：模型只能标 complete/blocked（blocked 需连续 3 轮同一阻塞）；marker 契约保留为**兜底**（工具缺失/未调用时）；headless 同样可用 | 工具注册、状态回传、与 marker 判定的优先级合并逻辑；全分支测试 | L |
| **P2.6** | **ralph/loop 判据与默认值升级** | ① ralph 关键词 grep → goal 同款**严格末行 marker 契约**（详见 §6 兼容策略）；② `/ralph`/`/loop` 默认 10 → **25**（与 goal 对齐）；③ 状态注入块复用 goal 的防漂移纪律（fidelity/no-progress）；④ loop 增加"建议退出"提示（连续 stall 时建议用户确认是否继续） | marker 契约测试套件（goal 已有，复制适配）；关键词兼容路径测试 | M |

依赖关系：P2.0 → P2.1 → P2.2 →（P2.3、P2.4、P2.6 可并行）→ P2.5。

### 5.3 护栏信号优先级（每次续跑判定顺序）

```
1. 终止信号（marker / update_goal / 关键词）→ 完成
2. 预算信号（cost/token delta 超限）        → BudgetLimited（终态）
3. 进展信号（no-tool 连续 2 / stall ≥ 3）    → 暂停 + 根因提示
4. 阻塞信号（GOAL_BLOCKED）                  → Paused + check-in 退避（P2.4）
5. 兜底信号（iterations ≥ max）              → 暂停（现有行为）
6. 否则 → 续跑
```

预算放在进展之前：烧钱比原地打转更不可逆。

## 6. 迁移与兼容策略

| 变更 | 破坏性 | 策略 |
|---|---|---|
| ralph 关键词 grep → 严格 marker | **是**：现有用户脚本/习惯依赖 DONE 等关键词 | **OR 兼容期**：marker 命中 OR 关键词末行命中任一即完成（关键词仅认可**末行**，消除 substring 误触）；两条路径都注入提示"关键词判定将在下个版本移除"；文档标注弃用时间表 |
| ralph/loop 默认 10 → 25 | 低（放宽方向） | CHANGELOG + 循环启动消息显示当前上限与新增护栏说明 |
| goal `--max` 语义变化（从主护栏降为兜底） | 否（新增信号只是更早触发暂停） | 不变；P2.3 落地后 `--max` 帮助文案改述为"兜底上限" |
| 新护栏触发的暂停（anti-spin/stall/budget） | 新行为 | 每种暂停首次触发时输出一行解释（可关）；pill 状态色区分 paused 原因 |

## 7. 实施顺序与工作量

```
第 1 批（安全修复，先行独立可发）：P2.0                       [S，~0.5 天]
第 2 批（主动护栏）：              P2.1 → P2.2               [S+M，~1.5 天]
第 3 批（成本与节奏，可并行）：    P2.3 ∥ P2.4 ∥ P2.6        [M×3，~3 天]
第 4 批（体验收尾）：              P2.5                       [L，~2-3 天]
```

每批独立提交、独立可回滚；P2.0 修复生产栈溢出隐患，建议审核后最先发。

## 8. 风险与缓解

| 风险 | 缓解 |
|---|---|
| 进展自报被模型滥用（谎报 progress 骗过 stall 检测） | 自报与确定性 no-tool 检测**取或**而非取信；verified wait 需给出证据（Codex 模板原语义：仅凭 intent/锁文件不算） |
| 预算快照与 billing 的记账时点偏差（轮内成本计入下一轮） | 快照在续跑判定时读取（轮已完结）；偏差 ≤ 1 轮成本，可接受并在 UI 标注"估算" |
| check-in 回访打扰用户 | 默认仅 blocked 暂停后启用；上限 3 次；环境变量可关（对标 Claude Code） |
| ralph 兼容期双判据维护成本 | 弃用时间表明确（下个 minor 移除关键词路径），代码用 feature-gated 注释标记删除点 |
| 默认值放宽（10→25）放大失控成本 | 放宽的前提恰是 P2.1/P2.2 主动护栏已就位（第 2 批先于第 3 批）；顺序不可调换 |

## 9. 决策点（已批准 2026-09-04）

1. **P2.0 是否先行单独发**：✅ 是。生产栈溢出隐患与护栏体系解耦，最先发。
2. **ralph/loop 默认 10 → 25 时机**：✅ P2.1/P2.2 先合入（提供主动护栏兜底），再放宽——即默认值变更并入 P2.6。
3. **ralph 关键词兼容策略**：✅ OR 兼容期 + 弃用时间表（关键词仅认可末行以消除 substring 误触；下个 minor 移除关键词路径）。
4. **goal 预算默认值**：✅ 默认不设 `--budget`、仅告警；不引入隐式终止。
5. **`--max` 的最终归宿**：✅ 推迟到 P2.3/P2.5 全部落地后再定；本轮仅做"降级为兜底"的事实注解（已在 R13 文档落实）。
6. **`LoopGuard` 落位**：✅ `shannon-core`（headless/引擎侧未来可用）。
