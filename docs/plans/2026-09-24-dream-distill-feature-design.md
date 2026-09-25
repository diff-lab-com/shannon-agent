# 功能方案设计：梦境提炼 Dream Pass——资料与技能的会话间异步提炼（v1.0）

- 日期：2026-09-24
- 状态：供评审（依赖竞品调研结论，见 `2026-09-24-dream-competitor-research.md`）
- 关联：`2026-09-24-skill-detection-wiring-design.md`（任务1 接线）、会话归档（任务2）、2026-09-23 综合方案任务规划
- 约束沿用：/opc 免改区不涉及；新路由不新增（挂 Memory/收件箱现有页面）；i18n 新键只加 en + zh-CN；shannon-core 仅做**增量** pub API（semver minor）

---

## 0. 背景与定位

竞品调研的结论：Dream 型机制 = **「已有记忆库 + 近窗会话转录」→ 异步整理/提炼 → 影子产物 → 审查后落地**。shannon 记忆侧的 L1（实时提取 + 规则压缩）已上线，缺的是 LLM 提炼层与技能侧打通。

与接线设计稿的分工：**接线稿解决「何时扫」（触发点 T1–T6、门控、噪声预算），本方案解决「扫到之后提炼什么、产出什么、怎么安全落地」（执行层）**。两者共用 T1–T6 触发与同一套门控状态。

排期定位：综合方案任务规划中的**任务 3**，建议在任务1（接线）、任务2（会话归档）之后实施。

## 1. 功能定义与对标

| 维度 | Claude Dreams（官方） | 本方案（Dream Pass） |
|---|---|---|
| 输入 | memory store + 1–100 份会话转录 | `~/.shannon/memories/{project_hash}.jsonl` + `~/.shannon/sessions/` 近 N 天转录 + 现有 skill-candidates.jsonl |
| 输出 | 全新影子 memory store | ①影子记忆提案（merge/remove/add）②提炼报告（梦境日记）③技能候选（入既有队列） |
| 非破坏 | 输入库永不修改，审阅后替换或丢弃 | 同口径：**apply 前主库逐字节不变**，产物全部落 `~/.shannon/dreams/` |
| 触发 | 手动 `/dream`、会话间后台、平台 API | T1–T6 触发点（LLM 档仅夜间 T6 默认关 + 手动 T3） |
| 成本 | token 计费，随会话数×长度线性 | 分档：L1 免费 / L2、L3 LLM，设置页展示上次耗时与估算 |
| 审查 | 用户审阅 output 后替换/丢弃 | 记忆提案逐条 apply/discard；技能候选走既有三动作 |

**不做什么**：不做 Shadow-Frog 式「空闲自生成实验」（执行型提炼）——那是独立且重得多的方向，本轮不立项；不做跨项目全局记忆重组（先按项目走，与现有 per-project 存储对齐）。

## 2. 分层设计

沿用 OpenClaw Light/REM/Deep 的分档先例，落到 shannon 现有模块：

| 档 | 名称 | 机制 | 成本 | 状态 |
|---|---|---|---|---|
| L1 | 整理 | `AutoDreamService` 每查询后关键词提取（`agent_loop.rs:4728`）+ `MemoryConsolidator` 规则压缩（24h 或 ≥5 会话，`compaction_trigger.rs` sidecar state） | 零 LLM | **已上线，不动** |
| L2 | 记忆提炼 | 读「L1 压缩后的记忆库 + 近 7 天转录」→ LLM 产出 keep/merge/remove + 新增提案 + 提炼报告（复用 `ConsolidationPrompt` JSON 协议与 `extract_memories` 的会话读取方式） | LLM | **新增** |
| L3 | 技能提炼 | 启发式候选（`skill_pattern_detection`）进 dream pass 做 LLM 精炼（复用 `refine_skill_candidate`）；并从转录中直接提炼程序性技能候选 | LLM | **新增**，产物一律入 SkillCandidate 队列 |

原则：**LLM 档永远不是记忆/技能的写入路径**——L2 产出提案、L3 产出候选，唯一写入路径是用户审批（对标 OpenClaw「Deep Sleep 唯一写入路径」与 Claude「input store is never modified」）。

## 3. 触发与门控

- 触发点复用接线稿 T1–T6；**L2/L3 仅由 T3（手动/`/dream`）与 T6（夜间闲时，默认关）承载**；T4（归档）只跑轻量统计不入 LLM 档。
- 单飞与节流：直接复用 `ConsolidationLock`（`auto_dream_consolidation.rs`，已实现 try_acquire/最小间隔/RAII），dream 锁以 `ConsolidationLock::new(Duration::hours(6))` 配置——即 6 小时内不重复跑 LLM 档；并发第二次调用返回「已在进行」。
- 状态扩展：接线稿的 `detection-state.json` 增加 `last_dream_at`、`last_dream_stats` 字段（向后兼容，`#[serde(default)]`）。
- 噪声预算沿用接线稿 §3：日 3 条新增收件箱卡、积压 5 背压、**14 天未处理的影子提案自愈 discard**（与自愈裁决②对齐）。

## 4. 产物设计

### 4.1 影子记忆提案（L2）
- 落盘：`~/.shannon/dreams/{project_hash}/proposal-{ts}.json`，结构 = `{ keep: [], merge: [[..]], remove: [], add: [{category, content, confidence, source_session_ids}] }`（`ConsolidationPrompt` 协议的超集）。
- 应用：UI 逐条/整批 apply / discard；apply 后写主库并打 `source_kind = "dream"`（沿用 P2-4 provenance 机制，String 值新增，无 schema 破坏）。
- 每条提案带 `source_session_ids` 与 `verified: bool`（LLM 能否回源引用原会话；借鉴 Shadow-Frog 的 provenance/verification 最小集）。

### 4.2 提炼报告（梦境日记，L2 附产物）
- 落盘：`~/.shannon/dreams/{project_hash}/report-{ts}.md`：扫描了几个会话、合并/移除/新增计数、出现的新模式 Top 5、下次建议；人可读、可分享。
- 入口：Memory 页「上次提炼」+ 收件箱 `dream_report` 卡（复用 Triage 八源 writer 机制，`inbox_session_events.rs` 加一个 writer，dedup 键 `(source, source_id)` 不变）。

### 4.3 技能候选（L3）
- 全部走既有 `SkillCandidate` 结构与三动作（忽略/直接采纳/提炼并采纳，接线稿 §3.1），**不存在任何绕过审批的 promote 路径**。
- 收件箱卡与 extensions/skills 待处理联动沿用现状，不新增 Surface。

## 5. 数据模型与接口（增量）

新 Tauri commands（`desktop/src/commands_dream.rs`，全部新增、无签名变更涉及面）：

```
run_dream_pass(scope: Option<String>, days_back: Option<u32>) -> DreamPassResult
read_dream_report(project: String, ts: Option<String>) -> DreamReport
list_dream_proposals(project: String) -> Vec<DreamProposal>
apply_dream_proposal(project: String, proposal_id: String, entry_indices: Vec<usize>) -> ApplyResult
discard_dream_proposal(project: String, proposal_id: String) -> ()
```

- 事件：`dream-pass-finished`（payload: detected/proposals/report），Header 徽标与 Memory 页刷新沿用推送模式（对标 `skill-candidates-changed`）。
- config（`config.rs`，全部 `#[serde(default)]` 向后兼容）：
  - `dream_enabled: bool`（L2，默认 **false**）
  - `dream_skill_distill_enabled: bool`（L3，默认 **false**；主开关仍受既有 `skill_detection_enabled` 与 `skill_loop_enabled` 管辖——双开关分组沿用接线稿 B2 裁决）
- 引擎侧不动：`extract_memories.rs`（LLM 管道）作为库被 dream pass 复用，不改引擎链路。

## 6. UX 设计

| 触点 | 内容 |
|---|---|
| Memory 页（`pages/Memory.tsx`） | 顶部「立即提炼」按钮（转圈/进度）+「上次提炼：时间 · 报告」入口；提案审查面板（keep/merge/remove/add 分组，逐条勾选 apply） |
| 收件箱 Triage | `dream_report` 卡：「梦境提炼完成：合并 4 · 移除 2 · 新洞察 3」→ 点开看报告；来源展示与其他八源一致 |
| 设置 | 隐私区两开关（L2/L3）+ 上次成本与耗时；文案对齐「夜间闲时时自动提炼（默认关）」 |
| Slash | `/dream [days]`——与接线稿 `/detect-skills` 同组注册（`commands_slash.rs`），绕频闸的口径一致 |
| 扩展页 | 不新增 tab；技能候选自然出现在 extensions/skills 待处理（现状） |

## 7. 隐私、安全与成本

- **全审批制**：主库与技能目录的唯一写入路径是用户确认（§2 原则）；这同时是对「自主改写记忆漂移/毒化教训」风险的行业共识性防御。
- **脱敏**：进入 LLM prompt 的转录先过敏感键过滤（对齐 Codex Memories「skips short-lived sessions / redacts sensitive content」口径；过滤器复用引擎现有敏感信息处理，如无则先做 key 类匹配最小版）。
- **开关语义**：`dream_enabled=false` 时 `run_dream_pass` 直接返回 0 且不读任何会话文件（与 `skill_detection_enabled` 隐私口径一致）。
- **成本可见**：报告与设置页展示 token 估算与耗时；官方建议「先小批量」→ 手动触发默认 `days_back=3`，夜间档 7 天。

## 8. 验收标准

1. `run_dream_pass` 单飞：并发第二次调用返回进行中错误；6 小时最小间隔生效（`ConsolidationLock` 测试已在库，补集成测试）。
2. **非破坏**：dream 全程 `~/.shannon/memories/*.jsonl` 逐字节不变；apply 之前产物只存在于 `~/.shannon/dreams/`。
3. 提案审查：Memory 页可按 keep/merge/remove/add 分组逐条 apply/discard；apply 后条目 `source_kind="dream"` 且携带来源会话 id。
4. 提炼报告生成且人可读，含扫描/合并/移除/新增计数与成本估算；收件箱出现 `dream_report` 卡（日 3 条预算内）。
5. L3 产物全部进入 SkillCandidate 三动作队列，代码路径上不存在直接 promote。
6. 开关全关：`run_dream_pass` 返回 0，不读会话文件（测试断言目录未被触碰）。
7. L2/L3 仅由手动与夜间（默认关）触发承载；归档触发不进 LLM 档。
8. 14 天未处理提案自愈 discard 并留日志。
9. 脱敏过滤在 prompt 组装前生效（单测覆盖）。
10. i18n 新键仅 en + zh-CN；`pnpm test:ci`、lint、`cargo fmt --check`、`cargo check --no-default-features --features tauri` 全绿。

## 9. 工作量与排期

| 阶段 | 内容 | 估时 |
|---|---|---|
| PR-A（L2 记忆提炼） | commands_dream + ConsolidationPrompt 接线 + 影子落盘 + 审查 API + Memory 页审查面板 + 报告 + 收件箱卡 | 2.5–3 天 |
| PR-B（L3 技能提炼 + 夜间档） | LLM 精炼接线 + 转录直提炼 + T6 夜间触发（依赖接线稿 T6）+ /dream slash + 设置开关 | 1.5–2 天 |
| 合计 | 含测试与 i18n | **4–5 天** |

依赖：任务1（触发接线，含 T3/T6 与 detection-state.json）→ 任务2（会话归档，为 dream 提供稳定输入窗）→ 本方案。可并行做的不依赖项：无（建议串行，避免与接线 PR 冲突同文件）。

## 10. 开放问题（供裁决）

| # | 问题 | 建议裁决 |
|---|---|---|
| ① | L2/L3 默认开关 | **默认全关**（对标：无竞品默认开后台 LLM 分析；接线稿裁决③同口径） |
| ② | 报告与提案的呈现位置 | 双入口：卡进收件箱（发现性）+ 全文/审查在 Memory 页（深度）；不新增路由 |
| ③ | `/dream` 与 `/detect-skills` 关系 | 合并为一组 slash：`/dream` 跑全档，`/detect-skills` 只跑启发式扫描；互相独立注册 |
| ④ | 影子提案保留期 | 14 天自愈 discard（与接线稿裁决②对齐） |
| ⑤ | 排期 | 任务1 → 任务2 → 本方案（PR-A → PR-B）；如需提前，PR-A 与任务2 可换序但需接受接线稿 T4 联调延后 |
