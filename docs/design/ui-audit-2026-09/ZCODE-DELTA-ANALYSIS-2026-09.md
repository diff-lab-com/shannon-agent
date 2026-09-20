# ZCode 桌面端新截图对比分析（2026-09-18）

> **证据**：[zcode-desktop-plan-dock.png](./screenshots/competitors/zcode-desktop-plan-dock.png)（ZCode 桌面端真机截图，2026-09-18，版本 ≥ v3.11.2）
> **对照物**：Shannon Desktop 当前基线 `desktop/ui/e2e/visual-baseline.spec.ts-snapshots/`（page-chat / page-tasks）
> **关联文档**：[UI-IMPROVEMENT-PLAN-2026-09.md](./UI-IMPROVEMENT-PLAN-2026-09.md)（§2.4 已覆盖 v3.11.2 主视图）· [COMPONENT-LIBRARY-RESEARCH.md](./COMPONENT-LIBRARY-RESEARCH.md) · `docs/improvement-plan-2026-09.md`
> **性质**：分析 + 建议，**不含已实施的代码改动**；建议项均需评审后再排期。

## 0. TL;DR

1. 这张新截图与 2026-09-10 已归档的 v3.11.2 主视图（右侧为内置浏览器）相比，**最大增量是右侧 dock 换成了「计划」文档标签页**——ZCode 展示了「计划-执行」闭环的完整形态：计划是常驻右侧的一等公民文档面板，转录流按计划门槛推进。
2. 与 Shannon 的差距**不在能力面，而在「会话内的透明度」**：ZCode 把运行遥测（时长、行数、PID、重试链、子智能体）默认平铺在侧栏与转录流里；Shannon 引擎里有同样的数据（事件溯源），但 UI 折叠隐藏了它们。
3. Shannon 的编排面（Best-of-N、Triage、Goal、日历/DAG、成本可见）明显厚于 ZCode，本图未见对应物——**差距清单是「外显」问题，不是「能力」问题**，与 improvement-plan-2026-09「把后端能力变成用户看得见的产品」主线完全同向。
4. 审美（近黑扁平 vs 玻璃紫）是受众选择，**不建议照搬**；但 Advanced 模式可提供紧凑密度档吸收专业用户。

---

## 1. 证据解剖：新截图逐区域

> 注：截图小字部分按结构转述，不逐字引用；各区域配「置信度」标注。

### 1.0 与已归档 v3.11.2 主视图的增量

| 维度 | v3.11.2（`zcode-desktop-main.png`，2026-09-10 归档） | 本图（2026-09-18） |
|---|---|---|
| 右侧 dock | 内置浏览器（URL 栏 + 1440×900 视口） | **「计划」标签页**：渲染计划 Markdown 文档 + 计划步骤弹层 |
| 左栏一级入口 | 新建任务/搜索/自动化/并行任务 | 新建任务/搜索/自动化/**技能市场** |
| 左栏任务列表 | 项目分组为主 | **「计划 / 已完成」分段切换 + 「今天」日期分组 + 行内实时时长徽章** |
| 转录流 | 工具调用折叠块 | **执行日志式排版 + 遥测密度显著提高 + 子智能体折叠段** |

结论：ZCode 在两周内迭代的方向正是「任务运行可见性」与「计划文档产品化」——两者都是 Shannon 现状的薄弱侧。

### 1.1 左栏：任务即运行（置信度：高）

- 顶部四个一级入口：**新建任务 / 搜索 / 自动化 / 技能市场**（小图标+小字标签，新建置顶）。
- 其下**分段切换「计划 | 已完成」+ 新建**：列表按任务状态二分，而非仅靠文件夹/项目隐式分组。
- 列表以**「今天」日期分组**开头，每行 = 状态图标 + 任务标题 + **右对齐的实时时长徽章（「8分钟」「10分钟」「4分钟」）**——侧栏本身就是一张运行监控面板，回答「agent 正在干什么、跑了多久」。
- 底部：工作目录入口 + 账号（Pro 徽章）+ 设置。

### 1.2 中栏：执行日志式转录（置信度：高；小字不逐字引用）

- 头部为任务标题 + 工具栏（复制/分享/展开等）。
- 转录**不是气泡聊天**，而是「引擎舱日志」式排版：编号步骤 + ✓ 检查项 + 行内展开的工具调用，**每步自带遥测**：耗时（「15秒」「567 ms」）、规模（「4个文件」「91行」）、PID 等。
- **错误默认外显**：红色内联条 + 完整引擎文件路径 + `attempt 1/2/3` 重试链叙述，长程任务的「失败→重试→恢复」过程像任务日志一样可回读。
- **「子智能体」「子智能体完成」作为一级可折叠段**内嵌在转录流中（非独立页面）。
- 穿插 turn 循环统计叙述（token 从 ~2954 增至 ~4800 一类的量化自述），以及按计划门槛推进的叙述（P1 门槛项 + 「turn 1/3」类预算门控）。

### 1.3 右栏：计划文档 = 一等公民 dock（置信度：中高）

- 带标签页的 dock：**「计划」tab + 「⋯」+ 展开 + 新建 tab（+）**。
- tab 内渲染完整计划文档《DeepSWE v1.1 评测与产品改进实施方案（shannon × glm-5.3-flash）》：多级标题、列表、**带复制按钮的 bash 代码块**。
- 另有一个锚定在计划 tab 的**计划步骤弹层**：编号步骤（P1 门槛项，带 turn 统计）+「计划 4步」等汇总字样——计划不只是文档，还是可勾选、可门控的执行清单。

### 1.4 Composer（置信度：高）

- 占位文案「输入新入指令或任务」；左侧「+」附件。
- **「无头浏览器」快捷 chip**（内置浏览器能力的零门槛入口）。
- **模型芯片（GLM-5.3-Flash）+ 推理档芯片（「最高」）直接在 composer 内**，逐消息可切换。

### 1.5 全局视觉

近黑（#0d0d0d 系）扁平高密度、细灰分隔线、蓝色强调 + 红色错误、自绘标题栏、几乎无圆角装饰。专业编码工具的「仪表舱」语言。

---

## 2. 异同矩阵

### 2.1 已对齐的标配（不必再投入的「相同点」）

| 维度 | ZCode（本图） | Shannon（现状） |
|---|---|---|
| 三栏骨架 | 任务栏 + 转录流 + 右 dock | 侧栏 + 主区 + ContextPanel/ArtifactPanel/WorkspaceGrid |
| 会话列表 + 搜索 | ✓ | ✓（还多置顶/拖拽排序/全文搜索） |
| 工具调用可视化 | 折叠块 + 状态图标 | 折叠卡 + 状态图标 + Diff 直达 + "N files changed" 汇总 |
| Plan mode | 计划文档面板 + 门槛门控 | Plan mode chip + Tasks/Goal 体系 |
| 自动化一级入口 | 左栏「自动化」 | Tasks 五 tab（能力更厚） |
| 技能市场 | 左栏「技能市场」 | Extensions/Skills（含 agent 自产技能审批） |
| 工作目录绑定 | 左栏底部 | Composer footer（Ctrl/Cmd+D） |
| 暗色优先 | 近黑单主题 | tokyo-night 默认 + 12 主题 |
| composer 推理档 | 「最高」芯片 | effort 下拉（low/medium/high/max） |

### 2.2 ZCode 领先（差距清单 = 本文档的行动来源）

| # | 差距 | 截图证据 | Shannon 现状 |
|---|---|---|---|
| G1 | **会话栏 = 运行监控**：状态点 + 实时时长徽章 + 「计划/已完成」分段 + 日期分组 | §1.1 | 侧栏是静态标题列表，仅按项目分组（且项目数 >1 才显示）；运行状态要去 Tasks 页/底部 footer 找（`SidebarSessions.tsx`） |
| G2 | **计划文档一等公民 dock**：常驻右侧 tab + 计划步骤弹层 + turn 预算门控 | §1.3 | plan 只是输入框上一个 chip；ArtifactPanel 靠 `detectArtifact` 自动检测，非用户主动 dock；ContextPanel 只放指标 |
| G3 | **转录遥测密度**：耗时/规模/PID 默认外显，错误红条 + 重试链不折叠 | §1.2 | 全部折叠进工具卡，hover/展开才可见；卡片头部无耗时（`StreamingResponse.tsx` 的 ToolCallDisplay） |
| G4 | **子智能体内联折叠段** | §1.2 | 聊天流无 subagent 视图；OPC AgentSwarm 在实验页，与日常会话脱节（`desktop/COMPETITIVE-ANALYSIS.md` 自认缺口） |
| G5 | **composer 内模型芯片**：逐消息切换模型 | §1.4 | 模型在全局 Header（`Header.tsx`），composer 仅 effort + 审批模式——ui-audit §6.4 已定「composer 三要素」目标，模型位仍缺 |
| G6 | **执行日志式转录**：长程任务过程叙事（attempt/重试/恢复）可回读 | §1.2 | 气泡聊天式；Rewind/checkpoint 有，但「过程像日志」的可读性弱于竞品 |

### 2.3 Shannon 领先（本图未见对应物，不可在对比中丢失的资产）

- **编排面**：Best-of-N 多方案对比（files/$ 分支 chips + Compare）、Triage 收件箱闭环、Goal 运行面板（迭代 3/12、花费 $0.42/$5.00、卡死计数）、任务日历/DAG 视图。
- **成本可见**：底部 footer token/费用常驻、Usage 页、预算上限——BYOK「成本可见」卖点已 UI 化；ZCode 本图无任何成本呈现。
- **代码向**：diff 逐 hunk accept/reject + Review All、CodeMirror 编辑器 + LSP QuickFix、xterm 集成终端、dev-server LivePreview、Rewind/checkpoint。
- **产品面**：Simple/Advanced 双模式、12 主题 + AA 对比度门禁、10 语言 i18n、语音输入、多会话独立窗口、多 provider BYOK、事件溯源 `shannon trace` 回放。

---

## 3. 启发（战略层）

1. **「可审计/可控」卖点的最后一公里在转录里**。Shannon 的立身叙事是开源可控、可审计、成本可见；引擎侧数据齐全（事件溯源、turn 级 token/cost），但对话流把遥测折叠隐藏——用户「看见的透明度」反而低于闭源竞品。遥测应当默认外显在侧栏与转录流，这是「把后端卖点变成看得见的产品」主线（improvement-plan-2026-09）在 Chat 页的具体化。
2. **行业心智正从 Chat 迁向 Run**。ZCode 把会话叫「任务」、列表即运行监控、转录即执行日志；Claude Code Desktop 的 Cowork、Codex 的 thread 同向。Shannon 不必改术语（知识工作者受众怕黑话，Simple mode 尤其），但会话栏应升级为「可观察的运行」——改呈现，不改名词。
3. **右侧 dock 应统一为一个带标签页的面板栈**。当前 ContextPanel（指标）、ArtifactPanel（产物）、计划（缺位）、Diff（对话框）四套各自为政；ZCode 用一个「计划」tab 验证了最小形态，Claude Code Desktop 用可拖拽面板验证了完整形态。统一 dock 是通往后者的低成本中间态。
4. **子智能体是引擎已有能力的「最后一步 UI」**。多 agent/Teams/worktree 隔离引擎齐备，聊天流只差一个内联折叠块 + 跳转 Mission Control 的入口——低垂果实。
5. **审美不必照搬**。近黑扁平 vs 玻璃紫是受众选择（专业编码工具 vs 知识工作者工作台），且 ui-audit 已定「Liquid Glass」美术方向；应吸收的是**信息密度与外显策略**（Advanced 模式提供紧凑密度档），而非配色与材质。

---

## 4. 改进建议（P0 / P1 / P2，供评审排期）

> 每条含：现状锚点（文件级）→ 目标行为 → 验收口径 → 规划映射。工作量为一档估算（P0 ≈ 1–2 天/条，P1 ≈ 2–3 天/条，P2 待定）。

### P0

**① 会话栏行内运行遥测**
- 锚点：`desktop/ui/src/components/SidebarSessions.tsx`；数据源：引擎事件（turn/tool 事件已持久化，`shannon trace` 同源）。
- 目标：会话行加运行状态点（运行中/等待审批/出错）+ 相对时间徽章（「8分钟」/结束于「2小时前」）+ 当前活动工具微图标；复用底部 footer 的活跃后台任务数据。
- 验收：不打开会话即可从侧栏回答「哪个在跑、跑多久、有没有卡在审批」。
- 映射：UI-IMPROVEMENT-PLAN「侧栏状态失明」历史问题的延续；improvement-plan P0-3（自动化可见性）同向。

**② 计划 dock（Plan 文档一等公民化）**
- 锚点：`desktop/ui/src/pages/chat/ContextPanel.tsx`、`desktop/ui/src/components/artifact/`（`detectArtifact.ts`）、Chat 页布局。
- 目标：Plan mode 激活或产出计划文档时，右侧 dock 自动出现「计划」标签页，渲染计划 Markdown + 步骤勾选状态与引擎 plan/todo 同步；用户可手动 dock/关闭；与 ContextPanel 共存为 tab 栈（完整 tab 化见 ⑦）。
- 验收：长程任务执行中，用户可边看转录边读计划；计划步骤状态实时反映执行进度。
- 映射：improvement-plan P0-2（Goal/任务桌面入口+运行看板）的 Chat 页形态；对齐 ZCode 已验证形态（§1.3）。

**③ Composer 模型芯片**
- 锚点：`desktop/ui/src/components/chat/ChatInput.tsx`、`desktop/ui/src/components/Header.tsx`（现状模型切换Owner）。
- 目标：composer 增加模型芯片，与 Header 双向同步；补齐 ui-audit §6.4 已定的「composer 三要素」（审批模式 ✓ / 推理档 ✓ / 模型 ✗）。
- 验收：不离开输入框即可逐消息切换模型。

**④ 侧栏会话分组升级：按分组（时间/状态）× 按项目 双视图**（用户补充建议）
- 锚点：`desktop/ui/src/components/SidebarSessions.tsx`（现状：仅按 working_dir 项目分组，且项目数 >1 才显示分组头；置顶/拖拽排序已持久化 localStorage）。
- 目标：侧栏头部加分组切换控件，两种视图并存、记住用户偏好（localStorage，沿用现有持久化模式）：
  - **按项目**（现状默认，Codex/Claude「项目」心智，保留）；
  - **按分组**：时间分组（今天/昨天/本周/更早，对齐 ZCode「今天」）与/或状态分组（进行中/已完成，对齐 ZCode「计划/已完成」分段）。
- 验收：单项目用户（大量个人用户）也能获得时间线扫读；多项目用户可切项目视图；搜索态下两种视图行为一致（现有全文搜索不受影响）。
- 映射：UI-IMPROVEMENT-PLAN Wave 2「侧栏按 Project 分组」的补全——ZCode 证明两种心智可并存于一个切换器。

### P1

**⑤ 工具卡遥测外显**
- 锚点：`desktop/ui/src/components/chat/StreamingResponse.tsx`（ToolCallDisplay）、`MessageBubble.tsx`。
- 目标：折叠态卡片头部增加一行遥测（耗时 / 产出规模 / 成本，数据源 turn 事件）；错误卡默认展开为红色内联块（对齐 G3），含「在 Timeline 查看」入口；保留「默认收起」给 Simple mode。
- 映射：`/timeline/:id`（TurnTimeline 页）的入口前移；「可审计」叙事的 Chat 页兑现。

**⑥ 子智能体内联折叠块**
- 锚点：`MessageArea.tsx` / `MessageBubble.tsx` 新消息类型；OPC 侧 `pages/OPC.tsx`（AgentSwarm）。
- 目标：转录流内子 agent 运行渲染为可折叠段（状态 + 汇总 + 「在 Mission Control 查看」），对齐 ZCode「子智能体/子智能体完成」。
- 映射：`desktop/COMPETITIVE-ANALYSIS.md` 自认缺口的最低成本补法。

**⑦ 右侧 dock tab 化统一**
- 锚点：`ContextPanel.tsx` + `components/artifact/` + ②产出的计划 tab + DiffDialog。
- 目标：四类内容（上下文指标 / Artifact / 计划 / Diff）统一为带标签页的右侧 dock 栈，支持拖宽与全屏；为未来可拖拽面板工作区（improvement-plan P1-5）铺路。

### P2

**⑧ Advanced 紧凑密度档**：行高/间距/字号一档压缩（`tokens.css` 密度 token），吸收专业用户；Simple 保持舒适档。
**⑨ attempt/重试链视图**：重试与恢复过程聚合为时间线片段，联动 `shannon trace` 回放（G6）。
**⑩ 「新建」分裂按钮**：侧栏 New Chat 升级为 Chat / Goal / 定时任务三向分裂入口（ZCode「新建任务」置顶同位）。

---

## 5. 与既有规划的映射总表

| 建议 | improvement-plan-2026-09 | UI-IMPROVEMENT-PLAN-2026-09 | 新增性 |
|---|---|---|---|
| ① 侧栏遥测 | P0-3 同向 | 历史问题「侧栏状态失明」 | 新条目 |
| ② 计划 dock | P0-2 同向 | §6.4 Chat 重设计延伸 | **新条目（本截图核心增量）** |
| ③ 模型芯片 | — | §6.4「composer 三要素」收尾 | 已有规划的收尾 |
| ④ 分组双视图 | — | Wave 2 项目分组的补全 | **新条目（用户补充）** |
| ⑤ 工具卡遥测 | P0-4（成本可观测）同向 | §6.4 工具折叠卡对齐 | 已有规划的深化 |
| ⑥ 子智能体 | — | COMPETITIVE-ANALYSIS 自认缺口 | 新条目 |
| ⑦ dock 统一 | P1-5（面板工作区）前置 | §6.4 视图 tab 延伸 | 中间态新条目 |
| ⑧–⑩ | P2 档 | — | 新条目 |

## 附：证据与参照物清单

- 本图：`docs/design/ui-audit-2026-09/screenshots/competitors/zcode-desktop-plan-dock.png`（原件留存 `reference/zcode/`）
- 旧图：`docs/design/ui-audit-2026-09/screenshots/competitors/zcode-desktop-main.png`（v3.11.2，右侧浏览器视图）
- Shannon 基线：`desktop/ui/e2e/visual-baseline.spec.ts-snapshots/page-chat-linux.png`、`page-tasks-linux.png`
- 旧设计稿（Aether 三栏）：`desktop/ui/design-ref-screenshot.png`（右栏 FILE CONTEXT / ACTIVE SKILLS 思路与 ②/⑦ 呼应）
