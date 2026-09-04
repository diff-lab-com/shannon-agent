# Shannon 改进实施方案（2026-09，待评审）

**日期**: 2026-09-05 ｜ **依据**: [docs/competitive-research-2026-09.md](./competitive-research-2026-09.md)（竞品调研与 Gap 分析）
**状态**: ✅ 已评审通过（2026-09-05），进入实施 —— 决议见文末「§8 决策记录」
**与 2026-08 计划的关系**: 本计划接替 [improvement-plan-2026-08.md](./improvement-plan-2026-08.md) 中尚未完成的桌面任务块；8 月计划的 Wave 4 候选项中已被本计划吸收/取代的条目在 §9 对照表中标注。

---

## 1. 目标与原则

**目标**：把 Shannon 的差异化叙事（开源、多 provider、成本透明、安全可审计、CLI+桌面同核）从「后端能力」变成「用户看得见的产品」，补齐 2026 桌面 Agent 编排器的三个标配体验（自主任务可视化、自动化收件箱、成本可观测）。

**原则**（按优先级排序）：
1. **先暴露、后新建**：goal/ralph/loop、routine、webhook、mobile pairing、注入扫描等后端已就绪的能力优先做桌面 UI 暴露（性价比最高的一层）。
2. **打竞品已验证的需求，不打未验证的**：Goal Mode（ZCode 百万用户验证）、自动化收件箱（Codex）、成本可观测（Hermes 全网痛点）、多窗口（Codex 官方 issue 未满足）——都是被用户用脚投票过的。
3. **一条主线一个 Wave**：每个 Wave 只有一个主题，Wave 内可并行、Wave 间有依赖。
4. **不打正面**：不做 IDE（vs Cursor）、不做云端 VM（vs Anthropic）、不铺 20 个消息渠道（vs Hermes 渠道广度），只做「任务进出」的最小渠道集。

**估时口径**：1 人日 = 1 名熟悉该模块的全职工程师一天（含测试与文档）；桌面项默认含 React 组件 + Tauri command + vitest 测试。

---

## 2. 优先级总览

| ID | 事项 | 主题 | 依据(Gap) | 规模 | Wave |
|---|---|---|---|---|---|
| **P0-1** | README/文档事实一致性速赢 | 工程 | I-1/I-2 | 1d | A |
| **P0-2** | Goal/Ralph/Loop 桌面入口 + 运行看板 | 自主任务 | G1 | 6-8d | A |
| **P0-3** | 自动化收件箱：Triage 闭环 + API endpoint 触发器 + SQLite 存储 | 自动化 | G2 | 7-9d | A/B |
| **P0-4** | 成本可观测：上下文拆解 + 缓存命中 + session 预算上限 | 成本 | G3 | 6-8d | A/B |
| **P1-1** | Session/任务多窗口 | 工作区 | G4 | 5-7d | B |
| **P1-2** | /batch + best-of-N worktree 并行桌面 UI（并排 diff 择优） | 编排 | G1/G4 | 8-10d | B |
| **P1-3** | 沙箱与权限产品化（执行模式切换 + Profiles 页 + 决策解释） | 安全 | G6 | 6-8d | B |
| **P1-4** | 消息渠道入站第一批（Telegram/Discord/Slack + 飞书/钉钉） | 渠道 | G5 | 10-12d | C |
| **P1-5** | 可拖拽面板工作区 + 预览自检 + 集成终端 | 工作区 | G4 | 15-20d | C/D |
| **P1-6** | 迁移向导（Claude Code / ZCode → Shannon，全量） | 生态 | G7 | 5-6d | C |
| **P2-1** | 移动派发 MVP（pairing → 审批+派发+进度推送） | 渠道 | G5 | 8-10d | D |
| **P2-2** | Profile/人格整包导出导入 | 资产 | G8 | 4-5d | D |
| **P2-3** | 办公 skills 试水（docx/xlsx，2-3 个社区级） | 办公线 | G9 | 4-6d | D |
| **P2-4** | 记忆溯源与图谱 | 资产 | G8 | 5-6d | D+ |
| **P2-5** | Idle-time 低峰任务队列（借鉴 ZCode，BYOK 场景=低峰低价模型跑批） | 自动化 | ZCode | 4-6d | D+ |
| **P2-7** | GitHub 事件触发器（依赖公网网关方案，随渠道/网关工作评估） | 自动化 | G2 | 4-6d | D+ |
| ~~P2-6~~ | ~~VS Code 扩展重启~~ | — | — | **已否决（2026-09-05 评审）** | — |

**总量**：P0 ≈ 3 周（1-2 人）；P0+P1 ≈ 2.5-3 人月；P2 已取舍（P2-6 否决，其余保留）。

> **评审决议（2026-09-05）**：VS Code 扩展永久放弃；IM 渠道 5 个（Telegram/Discord/Slack/飞书/钉钉）一次性全做、不拆小批；其余按调研建议执行（GitHub 触发器拆出 P0-3 → P2-7；工作区分期顺序为多窗口→预览自检→面板化→集成终端；billing Demo 隐藏不做真计费；SQLite 仅收件箱/automation 运行记录）。

---

## 3. Wave A（P0，约 2 周，主题：把已有引擎能力变成桌面产品）

### P0-1 · README/文档事实一致性速赢（1d）

- **问题**：README 写 12 crates/7,889 tests，实测 20 workspace 成员/11,273 tests（metrics.md 已实测）；tech-debt 与 8 月计划中已完成的项未销账。开源产品的 README 数字失真直接损害可信度。
- **方案**：以 `docs/metrics.md` 为唯一事实源，README 徽章与表格改为生成式（脚本 `scripts/update-readme-metrics.ts` 从 metrics 生成，CI 校验漂移）；销账 TD-2/P2-4.x 与 8 月计划中已完成项。
- **涉及**：README.md、README.zh-CN.md、docs/tech-debt.md、CI。
- **验收**：README 数字与 `just test` 输出一致；CI 里有 drift 检查；tech-debt 剩余项逐条复核过状态。

### P0-2 · Goal/Ralph/Loop 桌面入口 + 运行看板（6-8d）

- **问题/依据**：G1。CLI 的 goal 体系（注入、anti-spin、stall strikes、budget cap、GOAL_COMPLETE/GOAL_BLOCKED 自动续跑）9 月刚完成 Phase 2，桌面零入口；ZCode 靠 Goal Mode 卖点用户破百万。
- **方案**：
  1. Composer 增加「目标模式」入口：goal 文本 + 可选 `max_turns`/`budget_usd` + 完成标记（默认 GOAL_COMPLETE）；headless 经 `--goal` 注入，交互会话走 `/goal` 等价命令（新增 Tauri command 转发到引擎，不复制逻辑）。
  2. Tasks>active 增 goal 运行卡：轮数/累计花费/strike 数/最近事件流 + 「暂停/停止/改写目标」（复用 `goal_get`/`goal_update` 工具契约）。
  3. GOAL_BLOCKED 时在 Triage 生成「需要你决策」条目（与 P0-3 收件箱汇合）。
  4. composer slash 补齐 `/goal` `/loop` `/ralph` 自动补全（现有 12 条 → 16 条）。
- **涉及**：`desktop/ui/src/pages/Tasks.tsx`、composer（`lib/slash/`）、新 `goal_commands.rs`（Tauri）、引擎侧零改动（Phase 2 刚合入）。
- **验收**：桌面创建 goal 任务→自动续跑→完成/阻塞可观测→阻塞项出现在 Triage；预算耗尽时按 R15 语义正确停止；vitest+playwright 覆盖创建/中断/恢复路径。

### P0-3 · 自动化收件箱：Triage 闭环 + API endpoint 触发器（7-9d，可与 P0-2 并行）

- **问题/依据**：G2。Codex 的 Automations→review queue+原 thread 续跑是编排灵魂；Claude Routines 支持 cron/API endpoint/GitHub 三类触发，Shannon 只有 cron+webhook 出站通知。
- **方案**：
  1. **收件箱**：routine/goal 产出一律进 `/triage`（已存在该页），条目带来源（哪个 routine/触发器）、产物链接、操作组：「在原会话续跑」（复用 thread 语义）/「重跑」/「归档」。
  2. **API endpoint 触发器**：`shannon serve`（127.0.0.1:33420）为每个 routine 暴露 `POST /routines/:id/trigger`，校验复用现有 HMAC-SHA256 webhook 签名体系；Claude 的「把 Slack 告警指向专属端点」场景即可成立。
  3. **SQLite 存储**（决议 D6，+2-3d）：收件箱条目与 automation 运行历史落地 SQLite（rusqlite bundled，仅此存储）；会话保持 events.jsonl 事件溯源不动。
  4. 执行历史已有（scheduled_commands），补「失败自动进收件箱并带错误摘要」。
  5. GitHub 触发器**拆出**→ P2-7（本地端接收 GitHub webhook 需公网网关/tunnel，属网关架构决策，不阻塞 Wave A）。
- **涉及**：`shannon-server`（routes/auth）、`shannon-core/scheduled_routines.rs`、desktop `/triage` 页、`scheduled_commands.rs`。
- **验收**：curl 带 HMAC 触发 routine→结果 30s 内出现在收件箱→一键在原会话续跑且上下文保留；收件箱支持按来源/状态过滤（SQLite 查询）；触发器文档覆盖 cron+API endpoint 两类。

### P0-4 · 成本可观测：上下文拆解 + 缓存命中 + session 预算（6-8d）

- **问题/依据**：G3。竞品评论区第一大抱怨全是成本/限额（Claude 限额焦虑、Codex token 计价混乱、Hermes "cost projection is insane"）；Hermes 的按类别拆解是唯一正面解法。Shannon 的 BYOK 故事缺 UI 证据。
- **方案**：
  1. Chat 状态栏新增「上下文构成」弹出层：系统提示/工具定义/技能/记忆/MCP/对话 各自 token 数（引擎在 compaction 预算阶段已有 phase 化预算数据，暴露即可）+ 缓存命中率 + tokens/s。
  2. **session 预算上限**：通用化 goal 的 budget 机制——per-session `budget_usd`（默认关），超限暂停并弹「继续/提升预算/停止」；usage 页显示 per-session 累计。
  3. 中途切换 provider/模型时警告「将击穿提示缓存」（Hermes 同款）。
  4. billing Demo 页降级或隐藏（I-5），以 BYOK 成本中心替代。
- **涉及**：`shannon-core`（usage 统计已有、budget 逻辑从 goal 泛化）、`shannon-ui` Chat 状态栏、desktop usage 页。
- **验收**：任一会话可看到六类 token 拆解与缓存命中率；设 $2 预算的会话在超额时确实暂停且可恢复；切换模型出现缓存警告。该功能同时是官网/README 的营销素材（截图即可对比 Hermes/Claude）。

---

## 4. Wave B（P1 上半场，约 3 周，主题：工作区形态与编排可视化）

### P1-1 · Session/任务多窗口（5-7d）

- **依据**：G4。Codex 多窗口是官方未满足需求（#33205），直接可打的差异点；Claude Code 已支持多窗口。
- **方案**：Tauri 多窗口（WebviewWindow per session/task）；托盘/任务栏窗口管理；「在 新窗口 打开」入口；窗口与会话绑定持久化（复用窗口状态持久化逻辑）。
- **风险**：Tauri 多窗口与全局状态（单例引擎连接）需梳理——引擎经 `shannon serve` 多路复用，窗口只做 surface，符合 ADR-0011 单产品多表面架构。
- **验收**：3 个会话 3 窗口并行运行互不阻塞；重启后恢复。

### P1-2 · /batch + best-of-N worktree 并行桌面 UI（8-10d）

- **依据**：G1/G4。Codex `--attempts 1-4` 多方案择优与 worktree 耦合是其口碑功能；Shannon CLI 的 /batch+worktree 隔离已有，桌面有 worktree 面板但无并行任务编排 UI。
- **方案**：任务创建器支持「并行 N 份」（N worktree × 同一 prompt 或拆解后的子任务）；完成并排 diff 对比视图（components/diff 扩展为多列）+ 「采纳此份」（merge/cherry-pick）+ 其余 worktree 清理；OPC Kanban 显示并行任务泳道。
- **涉及**：desktop agents/worktree 命令、diff 组件、`/batch` 桌面等价 command。
- **验收**：一次创建 3 份方案→3 worktree 并行→并排 diff→采纳 1 份合并、2 份清理；全程无 CLI。

### P1-3 · 沙箱与权限产品化（6-8d）

- **依据**：G6。竞品安全 UX 标配：Codex 3 档沙箱×审批策略+`/permissions`、ZCode Shift+Tab 四模式；Shannon 权限引擎（5 级+LLM 分类器）更强但 UI 缺席。
- **方案**：① 全局/会话执行模式切换器（严格/平衡/宽松/自定义，对标 Shift+Tab 位置与交互）；② Settings 新增 Profiles 页（列 `save_custom_profile` 已有后端，补 CRUD UI）；③ 权限批准弹窗展示「规则命中 / LLM 分类（置信度）」原因；④ `/sandbox` 档位暴露为开关（Linux landlock 现状标注 experimental）。
- **验收**：不写配置文件即可完成「切模式→看原因→存 profile→复用」闭环；LLM 分类理由在弹窗可见。

---

## 5. Wave C（P1 下半场，约 3-4 周，主题：生态与触达）

### P1-4 · 消息渠道入站第一批（10-12d）

- **依据**：G5。Hermes ~20 渠道、WorkBuddy 微信直连+IM 入口验证「任务从 IM 进来」的需求；Shannon 只有出站通知。
- **决议（2026-09-05）**：5 个渠道一次性全做，不拆「2+3」小批；微信个人号仍明确不做。
- **方案**：第一批只做 5 个，全部走既有 webhook/HMAC/remote_trigger 底座：
  - **Telegram/Discord/Slack**：Bot 长连接或 Events API → 入站消息 → 创建会话/goal 任务 → 进度回推（复用出站模板）。
  - **飞书/钉钉**：开放平台事件订阅（国内企业场景，与 WorkBuddy 正面交锋点）。
  - 权限约束：IM 触发的任务默认挂「平衡」profile，敏感操作回 IM 卡片确认（复用审批流）。
- **不做**：微信个人号（合规风险）、iMessage/SMS/语音。
- **验收**：飞书群里 @bot 派发任务→桌面端执行→群里收到完成卡片+收件箱条目；密钥仅存本地 remotes.toml 同级配置。

### P1-5 · 可拖拽面板工作区 + 预览自检 + 集成终端（15-20d，可跨 Wave C/D 分期）

- **依据**：G4。Claude Code 拖拽面板（按 repo 保存布局）+ 预览 DOM 自检是 2026 桌面标配；Shannon 单窗口固定布局。
- **决议（2026-09-05）**：确认分期且顺序为——多窗口（P1-1，Wave B）→ 预览自检（C-1）→ 面板化（C-2）→ 集成终端（D）；面板化排在最后。
- **分期**：
  1. **C-1 预览自检**（5d）：ArtifactPanel 升级——检测项目 dev server（`package.json`/`launch.json` 约定）、内嵌 webview、截图回传给模型自检（复用图像分析工具）。
  2. **C-2 面板化**（8-10d）：chat/diff/preview/terminal 四类面板自由拖拽布局，按 project 持久化。
  3. **D 集成终端**（4-5d）：xterm.js + PTY（Tauri shell 插件），与 agent 共享环境。
- **验收**：改一个前端 bug→预览自动刷新→agent 截图确认修复；布局保存后重开还原。

### P1-6 · 迁移向导（5-6d）

- **依据**：G7。ZCode 迁移只支持对话记录+部分 skills，被社区吐槽——「一次做全」即可形成口碑差。
- **方案**：Welcome 流程新增「从 Claude Code / ZCode 导入」：`~/.claude/settings.json`（MCP servers）、`.mcp.json`、skills/commands 目录、`CLAUDE.md`→项目记忆；逐项勾选导入 + 冲突预览。README 放迁移文档。
- **验收**：Claude Code 用户 5 分钟完成迁移且 MCP/skills 全部可用；ZCode(AGENTS.md) 路径同样覆盖。

---

## 6. Wave D（P2，按评审取舍，主题：资产与远期）

| ID | 事项 | 方案要点 | 规模 |
|---|---|---|---|
| P2-1 | 移动派发 MVP | 基于 7 条 mobile pairing 命令做产品化：扫码配对→手机看任务/审批/派发；先做 PWA/本地 web（shannon serve 已有 HTTP+SSE），不急原生 App | 8-10d |
| P2-2 | Profile 整包导出导入 | 对标 Hermes tar.gz（技能+记忆+persona+routines，密钥剥离）；也是团队分发 agent 的雏形 | 4-5d |
| P2-3 | 办公 skills 试水 | 2-3 个社区级 docx/xlsx 生成 skills 试水办公线（不做交付物大屏；若 traction 好，交付物视图下季度再评估） | 4-6d |
| P2-4 | 记忆溯源+图谱 | 记忆条目→来源会话跳转；关系可视化（对标 Memory Graph） | 5-6d |
| P2-5 | 低峰任务队列 | 对标 ZCode Idle-time Task：BYOK 场景下低峰把排队 routine 跑在更便宜模型/时段；与 P0-3 触发器共用队列 | 4-6d |
| P2-7 | GitHub 事件触发器 | routine 订阅 repo webhook（issue_opened/pr_failed 等）；前置依赖：公网网关/tunnel 方案，随 P1-4 渠道与网关工作一并评估 | 4-6d |
| ~~P2-6~~ | ~~VS Code 扩展重启~~ | **已否决（2026-09-05 评审）：放弃成为 VS Code 扩展**；编码线入口让位给 CLI+桌面双形态 | — |

---

## 7. KPI 与验证方式

| KPI | 基线(2026-09) | Wave A 目标 | Wave C 目标 |
|---|---|---|---|
| 桌面可触达引擎能力比（桌面 slash+UI 入口 / CLI 能力） | ~60-75%（composer 仅 12 条 slash） | goal/loop/ralph/batch 可达，+4 | 迁移向导上线 |
| routine 结果→收件箱闭环率 | 0%（无收件箱语义） | 100%（含失败） | +API/GitHub 触发器 |
| 会话成本可见性（拆解/命中率/预算） | 无 | 三项全有 | usage 页按项目聚合 |
| 官网/README 可展示的对比素材数 | 少（多为表格） | +3（成本面板/收件箱/goal 看板截图） | +迁移指南 |
| dogfood：用 Shannon 自身仓库验证 G2 触发器 | — | GitHub issue 触发 demo | 飞书群派发 demo |

---

## 8. 决策记录（2026-09-05 评审结论）

| # | 决策点 | 结论 |
|---|---|---|
| 1 | P0 范围 | ✅ 三项全进 Wave A；GitHub 触发器拆出 P0-3 → P2-7，P0-3 缩至 7-9d（含 SQLite） |
| 2 | P1-5 工作区分期 | ✅ 接受分期，顺序：多窗口 → 预览自检 → 面板化 → 集成终端；面板化排最后 |
| 3 | 渠道名单 | ✅ 5 个渠道（Telegram/Discord/Slack/飞书/钉钉）一次性全做，不拆小批；微信个人号不做 |
| 4 | P2 取舍 | ✅ **放弃 VS Code 扩展（P2-6 否决）**；办公线按瘦身方案保留（P2-3 降为 2-3 个社区级 docx/xlsx skills） |
| 5 | billing Demo 页 | ✅ 隐藏/降级（I-5），不做真计费；真计费待商业化路线立项 |
| 6 | SQLite 化 | ✅ 窄范围纳入 Wave A：仅收件箱/automation 运行记录迁 SQLite；会话保持 events.jsonl，全量迁移不做 |

---

## 9. 与 2026-08 计划的对照

| 8 月计划条目 | 本计划处置 |
|---|---|
| P1-3c/3d Notion/Linear MCP | 未吸收——降级为「走 MCP 目录自然解决」，不再自研 adapter |
| P2-2 ADR-0005 Phase 2 收尾 | 沿用 8 月计划排期，不重复列 |
| P2-8 VS Code 扩展 | 已否决（2026-09-05 评审：放弃成为 VS Code 扩展），从两期计划中移除 |
| Wave 4 候选未列项（goal 桌面化/收件箱/成本面板/多窗口） | 本计划新增（调研结论驱动） |

---

## 10. 风险登记

| 风险 | 概率 | 缓解 |
|---|---|---|
| Goal 桌面化诱导非技术用户跑长任务→成本事故 | 中 | P0-4 预算上限与 P0-2 同 Wave 交付；默认预算提示 |
| `shannon serve` 暴露触发 endpoint 的安全面扩大 | 中 | 仅 loopback + HMAC（复用 webhook 体系）；文档明确不要端口转发 |
| IM 渠道 token/密钥管理引入泄露面 | 中 | 密钥本地存储、不入会话上下文；注入扫描覆盖入站消息 |
| 多窗口破坏桌面单例状态假设 | 中 | 引擎走 serve 多路复用；窗口层无状态化 |
| 与 8 月计划并行推进导致重复施工 | 低 | §9 对照表 + 以本计划为准归档 8 月桌面块 |
