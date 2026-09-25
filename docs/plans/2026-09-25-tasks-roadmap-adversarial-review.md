# 任务1（检测接线）/ 任务2（会话归档）/ Dream 规划——对抗性审查与改进方案（v1.1）

- 日期：2026-09-25
- 状态：供评审（v1.1 增补 §6 竞品对比与实施裁决）
- 审查对象：`docs/plans/2026-09-24-skill-detection-wiring-design.md`（任务1 v2.1，未提交）、任务2 立项建议（含于 v2.1 §5 裁决④）、`docs/plans/2026-09-24-dream-distill-feature-design.md`（已随 PR #117 合并，2026-09-25）
- 审查方法：以 origin/dev 最新代码（含 PR #116/#118/#119）为事实基准，对三项规划的假设逐条证伪

---

## 0. TL;DR

| # | 发现 | 级别 |
|---|---|---|
| 1 | **[P0] 会话输入契约断裂**：检测/摘录/清理三处代码读取的「平铺 `sessions/*.json`」布局在生产中不存在（现行是 `<id>/events.jsonl` 目录）。技能检测、dream 会话摘录、30 天 GC **当前全部空转**，且静默（报告显示 0 无人察觉） | P0 |
| 2 | **时序反转，任务1 设计稿过时**：dream（任务3）先行上线，已吸收 v2.1 约 40% 的范围（slash、状态文件、夜间触发、三动作、单飞）；v2.1 的 2.5-3 天估算与多条设计已失真 | 高 |
| 3 | **夜间触发笔记本盲区**：dream 夜间档只在「应用进程存活 × 本地 1–5 点」运行——典型笔记本合盖场景**永不触发** | 高 |

结论：原「任务1 → 任务2 → dream」的顺序已无意义。建议重排为**卡0（P0 输入适配修复，0.5-1d）→ 卡C（dream v1.1 盲区收口，0.5-1d）→ 卡A（会话归档，1.5-2d）→ 卡B（检测接线 v3 精简版，1.5-2d）**，合计约 4-6 天。

## 1. 现状盘点

### 1.1 任务1（检测接线，v2.1 设计稿）规划状态
- §1 触发：T1 启动 90s/每日 1 次；T2 会话切换 ≥6h；T3 手动 + `/detect-skills` 绕频闸；T4 归档回调；T5 删除引用卫生；T6 夜间闲时默认关
- §2 门控：`detection-state.json`（last_scan_at）
- §3 噪声预算：日 3 条 / 积压 5 背压 / 排他性带 [30%,80%)；§3.1 三动作（忽略/直接采纳/提炼并采纳）
- §5 裁决①-④（排他性带 +≥5 会话 floor、14 天自愈、夜间默认关、归档独立立项）已在 dream 方案评审时随建议获批
- §6 估算 2.5-3 天（含卡1）

### 1.2 dream（PR #117）实际已吸收的任务1 范围

| v2.1 计划项 | 现状 |
|---|---|
| T3 `/detect-skills` slash（绕频闸、启发式零 LLM） | ✅ 已上线（`detect_slash`） |
| `detection-state.json` | ✅ 已由 dream 创建（last_dream_at/last_stats；读改写约定已立） |
| T6 夜间触发 + 默认关 | ⚠️ dream 有自己的夜间档（1–5 点 + 24h 节流，`NIGHT_WINDOW` 常量），但那是 dream 的，不含检测扫描；且有发现 #3 盲区 |
| §3.1 三动作（忽略/直接采纳/提炼并采纳） | ✅ 候选队列已有 reject / approve / refine+approve |
| 单飞 | ✅ dream pass 有锁；检测扫描本身的频控未做 |
| T1/T2 扫描触发、扫描频控、排他性带、候选 14 天自愈、积压背压 | ❌ 未做（任务1 真实剩余范围） |
| T4 归档回调 | ❌ 依赖任务2；但「归档→`execute_dream_pass`」的钩子今天就可接 |

### 1.3 仓库新事实（本次审查逐一核实，origin/dev）

- 会话存储：`~/.shannon/sessions/<session_id>/events.jsonl` + `<session_id>/meta.json`（**meta.json 明确定位为 user-curation 字段**——归档标记的天然锚点）；E-9 会话索引 sidecar 已存在（list() O(会话数)）
- **无任何生产代码写平铺 `sessions/*.json`**（全仓 grep：仅测试 fixture）
- `housekeeping.rs`：30 天 GC 每日运行，但对目录调 `remove_file`（对现行布局无效→静默空转；若哪天「修好」则会无差别删史，且无归档概念）
- `list_recent_sessions`（检测 + dream 摘录共用）：只挑 `.json` 扩展名平铺文件、按 mtime 过滤；`load_session` 按 `{session_id, messages}` 单文档解析——布局、格式双重失配
- dream 夜间档：`NIGHT_WINDOW = 1..=5`、24h 最小间隔、30 分钟轮询——全部要求进程存活
- dream 提案的 `verified` 字段：LLM 的 true 被强制忽略（恒 false）——现为死字段
- 检测阈值现状：`DEFAULT_MIN_SESSIONS=2 / MIN_OCCURRENCES=3`；dream L3 直接用这组常量

## 2. 对抗性审查

### 2.1 跨项 [P0] 会话输入契约断裂（F1）

**发现**：三处代码以「平铺 `*.json` 单文档」为输入契约，而生产布局是「`<id>/events.jsonl` 行式事件」：
1. `skill_pattern_detection::list_recent_sessions` / `load_session` —— 检测永远扫到 0 会话 → 候选永远为空；
2. dream L2 摘录（复用同一 `list_recent_sessions`）—— 提炼输入退化为「仅记忆库」，报告 `scanned_sessions: 0`；
3. `housekeeping` 30 天 GC —— 对目录 `remove_file` 无效 → 会话无限累积（反向风险：若有人按文件语义「修复」它，会无归档概念地删史）。

**为什么没被发现**：检测无自动触发、UI 无入口按钮；dream 全部开关默认关；测试 fixture 是平铺 .json 自说自话。这是「输入层没有共享单源」的结构性后果。

**改进（→ 卡0）**：建一个**会话读取单源适配层**（读 `<id>/events.jsonl` + meta.json + E-9 索引），检测与 dream 摘录都走它；housekeeping GC 与它对齐；**加真实布局冒烟测试**（用 session_log 写真会话 → 断言检测/摘录可见）。这是卡A/B/C 的一切前提。

### 2.2 任务1（接线 v2.1）六条

- **F2 [高] 时序反转、设计稿失真**：见 §1.2，40% 已实现、夜间/状态文件/三动作语义已被 dream 先定义。→ 改进：出 v3 精简稿（本文件 §3 卡B 即其大纲），原 v2.1 标记 superseded。
- **F3 [中] T2（切换会话 ≥6h 触发）建议砍**：热路径耦合 + 边际收益低；T1 每日 + dream 夜间已覆盖「每日至少一扫」。砍掉可少一处 switch_session 耦合与测试面。
- **F4 [中] 排他性带 [30%,80%) 无观测手段**：无遥测基建任务，v2.1 说「一个月后按数据调」，但没有数据来源。→ 改进：band 阈值入 config 且**默认关**（退回绝对阈值 ≥3 次/≥2 会话），同时让 dream 报告/检测日志附带「band 命中统计」这种零 UI 观测，攒一个月数据再谈开启。
- **F5 [高] 候选 14 天自愈 × 归档耦合**：自愈丢弃候选的前提是「下次扫描还能重新检出」；一旦归档/GC 把输入会话移出窗口，自愈=永久丢失信号。→ 改进：自愈条件改为「仅当其 example_sessions 仍在输入窗口内」，该语义依赖卡A 的归档窗口定义，故卡B 排在卡A 后。
- **F6 [中] T5 删除引用卫生与任务2 重复**：删除/保留策略本就是归档功能的一半。→ 并入卡A（引用卫生：候选 `example_session_ids`、记忆 `source` 会话、dream 提案 `source_session_ids` 三处失效处理）。
- **F7 [低] 背压口径错位**：候选收件箱卡按 candidate id 去重，不是按天；「日 3 条」对它不适用。→ 改进：背压落在「pending 候选 ≥5 时不再新写卡，仅更新 JSONL」+ 可选的每日一卡「检测摘要」（复制 dream 报告卡的按天去重模式）。
- **F8 [中] 阈值双源分叉风险**：dream L3 现在直接调 `run_detection`（2,3 常量）；卡B 引入频控/band 后若两处各自演化会行为分叉。→ 卡0 一并抽共享常量/config 单源。

### 2.3 任务2（会话归档）五条

- **F9 [高] 归档标记落点其实已有答案，但 v0 估算（1-1.5d）偏低**：`meta.json` 就是「user-curation 字段」文件——`archived: bool` 放这里天经地义。真正的工作量在**双读取路径同步尊重该标记**：UI 列表（events 侧）隐藏/归档 lens，检测/摘录输入层排除 archived（卡0 的适配层里做，一处生效）。UI 侧：Sidebar 归档入口 + 会话操作菜单 + 恢复，无新路由。→ 估 1.5-2d。
- **F10 [高] 与 housekeeping 30 天 GC 的关系必须显式设计**：现状 GC 空转（见 F1），修卡0 时会顺手让它「真正工作」——那就立刻产生「30 天无差别删史」。→ 改进：GC 改为**只清理超过保留期且已归档**的会话（或可配置档），默认档建议「永不自动删除、仅归档」（见开放问题③）。
- **F11 [中] 归档顺序语义：先提炼后归档**：归档是会话生命周期的终点，归档前若 dream 开着应触发最后一次 `execute_dream_pass`（失败不阻塞归档，warn 即可）。这个钩子今天就能接（`execute_dream_pass(app, days)` 是 pub(crate)）。
- **F12 [中] 引用卫生范围（=原 T5）**：归档不破坏引用；**删除/GC 才会**。引用面三处：候选 `example_session_ids`（→ 摘录语义降级/置 stale 标记）、记忆 `source` 会话（`get_memory_source` 跳转失效→UI 降级为不可跳转）、dream 提案 `source_session_ids`（verified 语义受影响）。卡A 内逐一定义。
- **F13 [中] 竞品校准后默认值**：ZCode 的 3/7/14/30 天是**显式选项**；Codex/Claude 是手动归档为主。shannon 默认「永不自动删」是安全且符合竞品主流的（开放问题③）。

### 2.4 Dream 已上线功能四条

- **F14 [高] 夜间触发笔记本盲区**：`NIGHT_WINDOW 1..=5` + 30 分钟轮询 + 进程存活三条件在合盖过夜场景交集为空。→ 卡C：加 **catch-up 语义**——应用启动/唤醒后，若 `last_dream_at > 24h` 且进入空闲（如无活跃查询 10 分钟）则补跑一次（仍尊重双开关与节流）；夜间窗口保留为双轨之一。
- **F15 [中] `verified` 是死字段**：恒 false 还占着 DTO/UI 语义。→ 卡C：实现最小回源验证（提案内容关键词在其 `source_session_ids` 摘录中命中 → true），否则从 UI 隐藏该字段。
- **F16 [低] 单项目单 consult**：多项目用户成本线性放大；v2 再批量化，记录不动。
- **F17 [低] 上次提炼不可回看**：`DreamState` 有 last_dream_at/last_stats 但无读取命令，面板统计仅在当次会话内可见。→ 卡C 顺手：`read_dream_state` 命令 + 面板冷启动显示。

## 3. 改进方案：路线图 v2

| 卡 | 内容 | 估时 | 依赖 |
|---|---|---|---|
| **卡0（P0）** | 会话读取单源适配层（events.jsonl + meta.json + E-9 索引）；检测/摘录/GC 三处切换到它；真实布局冒烟测试；检测阈值抽共享 config 单源（F1/F8） | 0.5-1d | 无，**立即** |
| **卡C** | dream v1.1：夜间 catch-up 双轨（F14）+ verified 最小回源（F15）+ read_dream_state（F17）+ 三个小修（apply 原子标记/selected 清理/夜间循环 panic 隔离） | 0.5-1d | 卡0（否则 catch-up 空转） |
| **卡A** | 会话归档 MVP：meta.json.archived + UI（隐藏/归档 lens/恢复）+ 输入层排除 archived（卡0 适配层）+ GC 联动（默认不删，见③）+ 归档前 dream 回调（F11）+ 删除引用卫生（F12） | 1.5-2d | 卡0 |
| **卡B** | 检测接线 v3（精简版）：T1 启动 90s/每日 1 次 + detection-state 频控 + 收件箱背压 + 每日摘要卡（可选）+ 候选 14 天自愈（仅当输入仍在窗口，F5）+ band 入 config 默认关（F4）；**砍 T2**（F3）；v2.1 标记 superseded | 1.5-2d | 卡0、卡A（自愈语义） |

**顺序建议**：卡0 → 卡C → 卡A → 卡B（卡C 可与卡A 并行）。原「任务1/任务2」编号退役，以本表为准；合计 4-6 天。

## 4. 开放问题（供裁决）

| # | 问题 | 建议 |
|---|---|---|
| ① | 卡0 是否立即开工（不等本方案其余裁决） | **是**——它同时是 bug 修复，早修早止损（GC 修复后才开始真正删东西，更要先定语义） |
| ② | T2（会话切换触发）砍否 | 砍（F3）；将来有真实需求再立项 |
| ③ | GC/保留默认值 | **默认永不自动删除**（归档即可）；可选保留档（30/90 天，仅作用于已归档会话）进设置，默认关 |
| ④ | 排他性带 | 入 config **默认关**，先攒一个月 band 命中统计（dream 报告附带）再议开启 |
| ⑤ | 卡C 的 catch-up 触发条件 | 「last_dream_at >24h 且应用启动后无活跃查询 10 分钟」+ 保留原 1–5 点窗口双轨 |
| ⑥ | 卡A→卡B 顺序确认 | 确认（自愈语义依赖归档窗口定义） |

## 5. 依据

- 代码（origin/dev，含 PR #116/#118/#119）：`desktop/src/skill_pattern_detection.rs`（`list_recent_sessions` 扩展名过滤 / `DEFAULT_MIN_*`）、`crates/shannon-core/src/session_log/mod.rs`（`<id>/events.jsonl + meta.json` 布局、E-9 sidecar）、`crates/shannon-core/src/housekeeping.rs`（30d GC `remove_file`）、`desktop/src/commands_dream.rs`（`NIGHT_WINDOW`、`verified` 强制 false）、`desktop/src/commands_memory.rs`
- 文档：v2.1 接线设计稿、dream 两份文档、PR #117（含终审台账）、PR #119（floor 门禁/CI 改造）

## 6. 竞品对比与实施裁决（v1.1 增补，2026-09-25）

### 6.1 会话生命周期管理（归档）竞品对比

| 竞品 | 会话归档设计 | 教训/启示 |
|---|---|---|
| **Codex Desktop** | 线程控制菜单「Archive」；**只归档不删**（社区在要求真删除）；resume=`codex resume <id>/--last`，本地 JSONL 存储 | ①归档≠删除，删除需求后置出现→我们一开始就把「归档/删除/GC」三语义分开；②**归档后 resume 曾出「Failed to resume chat」bug**→卡A 必须显式定义归档会话的 resume 行为（建议：resume 自动解除归档） |
| **Claude Code** | 原生只有 `--continue`/`--resume`/`/resume`，**无归档/删除**；社区自建 clerk（会话自动摘要成可检索知识库）补位 | 一线 CLI 也缺位→归档对桌面 agent 是差异化机会而非追赶；同时验证「先提炼再归档」（clerk 的价值主张就是别让会话消失） |
| **Claude Desktop（聊天端）** | 有归档聊天 | 聊天端标配，桌面 agent 端缺位 |
| **ZCode** | 任务归档列表 + 3/7/14/30 天**显式**保留档 | 自动清理必须是用户显式选择，不做隐式默认 |
| **shannon 现状** | GC「30 天删」但空转（F1）；无归档 | 修卡0 会让 GC 复活→必须先定归档语义再让它真正删东西 |

**裁决：会话归档 = 必要，实施（卡A，1.5-2d）。** 这是桌面 agent 的桌面级基本功（Codex/ZCode 有、Claude Code 缺），且是 T4 回调/自愈语义/安全 GC 的前置。MVP 边界收窄为：手动归档 + 隐藏与归档 lens + 恢复 + resume 解除归档 + 输入层排除 + GC 只清「已归档且超保留档」。

### 6.2 自动记忆 vs 自动技能：竞品的第一性分工

| 能力 | 一线竞品做法 | 对 shannon 的含义 |
|---|---|---|
| **记忆自动提取/巩固** | Windsurf Cascade Memories（对话中自动生成、免费）；Claude auto-memory + Dreams；Codex Memories；ZCode Memory | **全行业验证的价值主航道** → dream 方向正确，卡C（catch-up 修盲区）必要——否则功能对笔记本用户不存在 |
| **技能（Skill）自动提炼** | **无一家的官方功能**。Claude 官方口径「skills 用户手写」；社区第三方工具（Zenn 2026-02：从 Claude Code 会话史+命令史自动生成 Skills）验证了概念可行但停留在社区层 | 任务1 的核心假设（隐式频率检测→技能候选）是**未被任何一线平台背书的赌注**，且我们的检测因 F1 从未在真实数据上运行过——信号质量为零证据 |

**裁决：检测自动触发（卡B 的 T1）暂缓实施，降级为「观测期」。** 理由：①竞品全体不做的功能，我们不该在信号质量零证据时先背全套噪声机器（预算/背压/自愈都是为压制它的噪声而设计的——信号好本不需要这么多闸）；②Anthropic 自己的分层就是「记忆自动化、技能手动化」，Hermes 社区把自动技能当 feature request 而非标配。**替代路径**：修好卡0 后，用 `/detect-skills`（手动）+ dream L3（用户主动开启）当观测器，在 dream 报告里附带 band 命中统计攒一个月数据，数据好再把 T1 自动扫描立项——成本从 2 天变成 ≈0，赌注变成数据驱动。

### 6.3 四卡最终裁决表

| 卡 | 必要性 | 可行性 | 裁决 |
|---|---|---|---|
| 卡0 输入适配修复 | **绝对必要**（正确性 bug：检测/摘录/GC 全空转） | 高（session_log API 清晰、E-9 索引现成） | **立即实施** |
| 卡C dream v1.1 | 高（catch-up 不修=dream 对笔记本用户不存在；validated 主航道） | 高 | **实施**（可与卡A 并行） |
| 卡A 会话归档 | 高（桌面级基本功 + 安全 GC 前置 + T4/自愈语义依赖） | 高（meta.json 锚点现成） | **实施**（MVP 边界见 6.1） |
| 卡B 检测接线 v3 | **低-中（暂缓）**：核心假设零证据、竞品零先例、噪声机器成本前置 | 技术可行但依赖卡A 语义 | **降级为观测期**：只做阈值单源（并入卡0）+ band 统计，T1 自动扫描待数据立项 |

**建议路线**：卡0 →（卡A ∥ 卡C）→ 观测期 ≥1 个月 → 数据决定是否重启卡B。
