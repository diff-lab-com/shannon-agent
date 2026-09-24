# Shannon Desktop 导航与信息架构综合改进方案（提案 v1.2，待审核）

> **状态**：提案（分析 + 方案，不含代码改动），供 ericdong 审核。v1.2：五条开放问题经评审裁定（§5）——#1–#4 采纳建议并锁定；**#5 裁决为「/opc 维持现状，本轮不动」**（撤销 v1.1 的 T9，本方案任何批次不得改动 /opc）。T8 / §3.4 / §4 已同步。
> **日期**：2026-09-23 · **分支**：`ui/nav-redesign-proposal`（基于 `dev` @ 0557f8fe）
> **定位**：第三轮 ZCode 对标研究。前两轮见 [ZCODE-DELTA-ANALYSIS-2026-09.md](../../design/ui-audit-2026-09/ZCODE-DELTA-ANALYSIS-2026-09.md)（→ PR #89）与 [ZCODE-DELTA-ANALYSIS-2026-09-20.md](../../design/ui-audit-2026-09/ZCODE-DELTA-ANALYSIS-2026-09-20.md)（→ 批次 A–F，PR #98–#104 已合并）。本轮回答前两轮**未覆盖**的三个问题：① 分组/项目导航的深化；② 「任务」与「收件箱」的职责重叠；③ 扩展（skills/MCP/plugins）入口收敛。
> **证据**：ZCode 参考截图 `reference/zcode/截图 2026-09-20 23-58-44.png`；竞品调研（Codex / Claude Code / ZCode / Cursor / Devin / Copilot / VS Code / Linear，2026-09 当日官方文档抓取，来源见 §2.4）；代码现状锚点由子代理逐文件核实（`dev@0557f8fe`）。

---

## 0. 摘要（TL;DR）

**一句话诊断**：Shannon 经过批次 A–F 已把 ZCode 的「皮肤」学到位（rail 运行语义、分组镜头、市场形态），但三个**结构性**问题没动，正是本轮三个问题的根源：

1. **项目是有心智无实体**——项目只是 `working_dir` 尾段字符串 + localStorage 改名（`SidebarSessions.tsx:139-145,41`），routine/goal 与项目无关联（`ScheduledRoutine` 无 `working_dir`，`scheduled_routines.rs:316-361`；goal `working_dir=None`，`goal_commands.rs:760`），所以「分组/项目」导航学不到 ZCode 的精髓：**自动化任务嵌进项目树**。
2. **一个运行，三个投影**——同一次例行运行同时写入 SQLite inbox（收件箱页）+ JSONL runs（任务页历史）+ GoalRunPanel/BatchRunPanel 实时卡（任务页进行中）（`inbox_commands.rs:296-330` 自证双写）；而会话侧的「需要关注」（审批/失败/技能提案）又**不进**收件箱，散在 rail 状态点、Header 铃铛、两处审批 Modal 里。这就是「任务」与「收件箱」感觉重叠的真相：**两页是同一事实的两个不完整切片**。
3. **扩展一个词盖 7 种东西、审批面三处冗余**——技能审批有 3 个触发面（Header Modal + Settings/高级 Modal + 全局 toast）、MCP/网关/权限三个「连接」概念分属三页（`/extensions/mcp-servers`、`/settings/connections`、`/settings/permissions`）；竞品 2026 年已收敛为「**插件 = skills+MCP+commands 统一打包 + 一个 tab 化统一入口**」（Claude Code `/plugin` 五 tab、Devin Customize 明确写「取代旧的三个分散页面」、Cursor Customize 同页管理）。

**方案主线**（详见 §3）：减少顶层目的地，把复杂度从导航结构移进页面内的分区与筛选——

| 问题 | 方案一句话 | 关键动作 |
|---|---|---|
| ① 分组/项目 | **项目实体化**：让项目成为引擎数据（注册表 + routine/goal 带 `working_dir`），rail 项目树嵌自动化、空项目可存在 | 阶段三（引擎依赖） |
| ② 任务/收件箱 | **按 ZCode/Codex 公约重新切分**：运行监控归 rail（已有）、配置归单一「自动化」页（现 /tasks 收缩）、产出归统一「收件箱」（扩源到会话审批/失败/技能提案） | 阶段一 + 二（纯前端可先做） |
| ③ 扩展收敛 | **一页三区 + 信任安装**：市场/已安装保持，技能提案审批收进收件箱，MCP 行内直达权限管理；长期对齐 Claude Code 插件包格式 | 阶段二 + 四 |

改后简单模式导航：**对话 / 收件箱 / 扩展 / 记忆**（自动化收进顶部动作区，与 ZCode 同构；目的地数量不增反减一）。

---

## 1. 参考对象与现状基线

### 1.1 ZCode 截图设计理念拆解（任务 1 输入）

参考截图：`reference/zcode/截图 2026-09-20 23-58-44.png`（暗色主题，中文）。整个侧边栏没有「页面导航」——它就是一张任务清单：

```
┌──────────────────────────────────────┐
│ ① 顶部动作区（4 行）                    │  新建任务 Ctrl+N
│                                      │  搜索    Ctrl+K
│                                      │  自动化
│                                      │  插件市场
├──────────────────────────────────────┤
│ ② 视图镜头切换（单选）+ 筛选/排序        │  # 分组 | 📁 项目   ⤢   ⏷ ░
├──────────────────────────────────────┤
│ ③ 内容主体：项目 → 任务 两级树          │  项目
│   项目行：folder 图标 + 目录名          │  📁 ai-video-clone
│   任务行：状态图标 + 标题 + 相对时间      │     ⟳ AI视频复刻产品PRD与方案设计  2分
│   自动化任务与普通任务同行混排            │  📁 llm-research
│   空项目内联空态 / 失焦项目置灰           │  📁 mobile   暂无任务
├──────────────────────────────────────┤
│ ④ 底部账号区                           │  (D) ddemucub [Max]   ⚙
└──────────────────────────────────────┘
```

八条设计理念：

| # | 理念 | 截图证据 |
|---|------|----------|
| Z1 | **任务中心语言**：顶层动作是「新建任务」而非「新对话」——会话即任务 | 第一行「新建任务 Ctrl+N」 |
| Z2 | **项目是唯一组织轴**：项目=工作目录/仓库，任务天然归属；没有第二套「页面树」 | 「项目」区 12 个文件夹，全部任务嵌套其中 |
| Z3 | **镜头切换而非页面切换**：`分组/项目` 是同一份数据的两个视图 | 分组/项目 segmented + 筛选图标 |
| Z4 | **自动化是一等内容但不是独立页面**：周期任务以任务行形态嵌在所属项目里（gear 图标 + 周期描述 + 时间） | 「每30分钟巡检minimind-lab… ⏱17小时」直接出现在 llm-research 下 |
| Z5 | **运行状态内联可视**：运行中 spinner+已耗时；时间（相对时间）是每行固定元数据 | 「2分」「17小时」「刚刚」「13天」 |
| Z6 | **插件市场是顶层动作**：市场形态一站式呈现 | 「插件市场」独立入口 |
| Z7 | **低装饰高信息密度**：行=图标+标题+尾缀时间；空态/失焦用置灰 | 「暂无任务」置灰 |
| Z8 | **动作在上、内容在中、身份在下** | 整体布局 |

> 注意与官方文档的交叉印证：ZCode 桌面端（v3.14.3）另有独立的「自动化」管理页（定时任务 + 闲时任务，任务详情 设定/历史 双 tab，历史每条可跳回会话），已开始执行的闲时任务会以「月亮标识」出现在侧栏分组里——即 **ZCode 也是「rail 混排展示 + 独立管理页」两层结构**，截图里看到的是展示层（详见 §2.3）。

### 1.2 Shannon 当前侧边栏：已学到的与没学到的

批次 A–F 之后（`Sidebar.tsx` / `SidebarSessions.tsx`）**已对齐**：

- ✅ Z3/Z5：三镜头 rail（按项目/智能/按会话）+ 运行绿点/审批黄点/失败红点 + 相对时间尾缀
- ✅ Z6 部分：顶部动作区（新对话 split 按钮 / 搜索 Ctrl+K / 自动化 Ctrl+2）
- ✅ Z4 雏形：rail「自动化」小节（启用 routine 前 3 条 + 下次触发，`SidebarSessions.tsx:786-826`）
- ✅ Z8：底部模型徽章（BYOK 表达）+ 模式切换 + 设置

**没学到的（= 本轮任务 1 的差距，记 G 系列）**：

| # | 差距 | 证据 |
|---|------|------|
| G1 | **项目不是实体**：`projectOf()`=dirname 尾段（`SidebarSessions.tsx:139-145`），显示名存 localStorage（`shannon-projects`），换设备即失；没有空项目（「暂无任务」形态不存在，因为没有会话的项目根本不出现）；无项目级动作（在项目中新建、打开目录、归档、图标/颜色） | 后端无 `struct Project`（全量 grep）；`COMPETITIVE-ANALYSIS.md:180-182` 2026-06 已把「Project=repo+配置+历史」列为 P0，至今未做 |
| G2 | **自动化任务不嵌项目**：`ScheduledRoutine` 无 `working_dir`（`scheduled_routines.rs:316-361`；SCHEDULED-FIX-PLAN.md §2 早已规划 `cwd` 字段未落地），goal 运行 `working_dir=None`（`goal_commands.rs:760`）→ 只能聚合成 rail 顶部小节 + 独立页 | `SidebarSessions.tsx:189-191` 注释自证 deferred |
| G3 | **「分组」镜头缺状态维度兜底**：ZCode「分组 ✨」是默认第一档；我们的「智能」分组藏在三档 segmented 第二档，默认是「项目」 | `SidebarSessions.tsx:79-85`（默认 project） |
| G4 | **顶部四件套缺「新建任务」语义**：顶层按钮是「新对话」（split 里才有 目标/例行）；Z1 的任务语言缺失 | `Sidebar.tsx:226-258` |
| G5 | **镜头切换与页面导航职责纠缠**：rail 内已有分组镜头 + 自动化小节，但主导航仍保留「任务」行（`nav.scheduled`），与顶部「自动化」按钮指向同一目的地——同一目的地两个入口、两个名字 | `Sidebar.tsx:286-295` vs `Sidebar.tsx:331`；zh-CN `nav.scheduled`=「任务」/`nav.automation`=「自动化」 |

### 1.3 现状全景：导航地图与三处结构性病灶（任务 2、3 输入）

当前路由与入口（`App.tsx:60-114`）：

```
侧边栏（简单模式）                          页内二级
────────────────────────              ────────────────────────────────
对话    /chat                          ─
任务    /tasks（顶部另有「自动化」按钮）    5 tab：进行中/历史/例行/流水线/工作空间
                                        + 日历/DAG 视图 + 批量/目标/子代理卡
收件箱  /triage（Header 铃铛同址）        状态筛选 + 来源筛选 + 批量操作
扩展    /extensions/featured             2 主 tab（市场/已安装）+「管理」下拉 5 子路由
记忆    /memory                          ─
设置    /settings                        8 子页（通用/主题/模型/权限/高级/通知/连接/远程）
（dev 模式追加：用量 /usage、指挥台 /opc）
```

**病灶 A ——「一个运行，三个投影」（任务/收件箱重叠的根因）**

同一次 routine 运行：`spawn_routine_run`（`inbox_commands.rs:296-330`）同时写 ① SQLite inbox（收件箱页读）② legacy JSONL runs（任务页历史读，注释自证「so the existing History view keeps working」）③ GoalRunPanel/BatchRunPanel 实时卡（任务页进行中）。goal 运行同时进 GoalRunPanel 和 inbox（source=goal）。**两页是同一事实的两个切片，互不链接**（收件箱只有「继续会话」跳 /chat）。

反过来，会话侧的「需要关注」**不进**收件箱：rail 审批黄点/失败红点的数据源（SessionActivity）与 inbox 无关；Header 铃铛被技能审批**劫持**（有候选时优先开 SkillApprovalModal 而非 /triage，`Header.tsx:61-64`）。于是用户面对：看自动化结果要去收件箱、看审批/失败要去会话行内+铃铛、看历史要去任务页——**没有一个地方回答「有什么需要我处理」**。

**病灶 B ——Tasks 页是 5 种作业的堆栈**

`/tasks` 页一个页面 5 套数据源（catalog tasks/backgroundTasks/agents + scheduledTasks + taskExecutions + batchRuns + goalRuns，`Tasks.tsx:73-83`）、5 tab（进行中/历史/例行/流水线/工作空间）、3 种运行卡、2 种可视化（日历/DAG）、3 个创建表单。`UI-IMPROVEMENT-PLAN-2026-09.md §3.3` 早已定性 🔴「7 控件无主次」「新建任务/新建例行/并行方案三个创建入口关系不可直觉理解」，Simple 模式裁剪 tab 只是**隐藏**而非**重组**。

**病灶 C ——扩展与审批入口发散**

| 概念 | 入口数 | 具体 |
|---|---|---|
| 技能审批 | **3** | Header SkillApprovalModal（`Header.tsx:388`）+ Settings/高级区块&Modal（`AdvancedSettings.tsx:166-201,518`）+ 全局 toast/审查面板（`App.tsx:119`）；事件源两个（`skill-proposal-available` / `skill-candidates-changed`） |
| 「连接」 | **3** | MCP 服务器（/extensions/mcp-servers）、网关 gateway（/settings/connections）、MCP 工具权限 glob（/settings/permissions）——三个页面互不链接 |
| skills/MCP 全部入口 | **6 类** | /extensions 市场、Header 审批、Settings/高级 skill-loop、Welcome 引导流、迁移向导、Settings 连接/权限 |
| 术语「任务」 | **3 轨** | 侧栏「任务」(nav.scheduled) / 侧栏「自动化」(nav.automation) / 页内 tab「例行」「流水线」——同一域三个词 |

---

## 2. 竞品评审（任务 2、3 证据基础）

> 以下为 2026-09-23 当日官方文档抓取的调研结论（子代理执行，来源清单见 §2.4）。查不到确证的已标注「不确定」。

### 2.1 「任务 vs 收件箱」：三种流派

竞品**无一例外**把「管理自动化任务的页面」独立成页（Codex Automations / Claude Routines / ZCode 自动化 / Devin Automations），且都配「运行历史 → 跳转会话/PR」；但「结果消费」**从不新建独立结果页**，而是落回会话或 PR。差异在于要不要显式 inbox：

| 流派 | 产品 | 「inbox 等价物」 | 形态 |
|---|---|---|---|
| **显式 inbox（唯一一家）** | Codex 桌面 App | Automations pane 的 **Triage** 区（"acts as your inbox"），all/unread 过滤；**无发现自动归档** | 真 inbox，但范围收窄到 automation findings；普通 thread 完成无未读聚合（2026-09 仍有 feature request 佐证这是缺口） |
| **会话即结果** | Claude Code 桌面、ZCode | Claude：Projects **Overview pane**（完成/待 review/等你回答）+ agent view「waiting on you」行 + 底栏等待计数；ZCode：侧栏「闲时任务」分组（月亮标识）+ 自动化页历史跳回会话 | 状态聚合/状态分组，无未读语义；靠 OS 通知拉人 |
| **路由回宿主** | Copilot、Cursor、Devin(web) | GitHub Notifications / Slack / PR review request；Devin Desktop 2.0 把收件箱 **Kanban 化**（in flight / blocked / ready for review 三列） | 产品内不建通知系统 |

参照系（Linear Inbox）：**通知消费层（流式、可清空）与管理对象层（持久、结构化）分离，通知行只是对象的投影**——这是 inbox 的正确心智。

**各产品侧边栏组织维度速查**：

| 产品 | 侧边栏结构 | 组织维度 |
|---|---|---|
| Codex 桌面 | 项目侧栏（"A project is just a folder"）+ Chats 分组（无项目 thread）+ Automations 面板（可 pin） | 项目 → thread（Local/Worktree/Cloud 三模式同列）；归档出活跃列表 |
| Claude Code 桌面 | 三 tab（Chat/Cowork/Code）；Code tab = session 列表 + Routines 入口 | 项目文件夹 → session；归档是唯一状态管理 |
| ZCode | 项目/分组 → 任务（⌘N 新建）+ Git 面板 + 技能；闲时任务分组 | 项目 → 分组 → 任务；任务行带相对时间 |
| Copilot 桌面 App | Projects + session 列表（active sessions 按仓库分组）+ Chats 分区 + Manage sessions | 项目 → session；轻对话另列 |

### 2.2 评审：Shannon 与竞品公约的偏离点

1. **Shannon 的收件箱范围「错位」**：竞品里唯一做 inbox 的 Codex 把它定义得极窄（automation findings、无发现即自动归档），而把「运行监控」交给 rail、「历史」交给自动化详情页。Shannon 的 Triage 范围与 Codex Triage 几乎重合（routine/scheduled_task/goal/trigger/batch 五源），**这本是对的**——错的是 Shannon 同时还有一个巨大的 Tasks 页展示同样的运行，且会话侧的审批/失败/技能提案反而不进收件箱。**该合并的没合并，该分开的没分开。**
2. **Tasks 页职责违背公约**：竞品的自动化管理页都是「定义 + 触发器 + 运行历史（跳回会话）」三件套（ZCode 任务详情就是 设定/历史 双 tab）。Shannon 的 /tasks 却同时是看板（进行中）、账本（历史）、配置（例行/流水线）、基础设施（工作空间）。**「进行中」的监控职责已被 rail 承接（批次 A–F 的 P0 工作），页面里再放一份就是重复投影。**
3. **rail 已是监控面板，但没有被当作「唯一监控面」设计**：rail 能显示运行中/审批/失败（会话），但 goal run 卡、batch 卡、例行执行状态只在 /tasks 页可见——同一信息两处投放，且页面那份数据更全，用户不得不学会「先看 rail，必要时去 /tasks」。

### 2.3 「扩展」：2026 年的行业收敛范式

2025-10 → 2026-09，行业明显收敛到：**「插件（plugin）= skills + MCP + commands/hooks/agents 的统一打包单位」+「一个 tab 化统一入口页」**。

| 产品 | 统一入口 | 关键形态 |
|---|---|---|
| Claude Code | `/plugin` 面板五 tab | **Discover / Installed（按 scope 分组、闲置提醒）/ Marketplaces（增删来源）/ Errors / Stats（每个 skill 的 context 成本与使用频次）**；详情页「**Will install**」预告将装入的 commands/agents/skills/hooks/MCP/LSP + scope 选择；安装时信任（命令源先展示确切 shell 命令）；企业 managed settings 锁市场白名单 |
| Claude 消费端（2026-09 最新） | Customize 三 tab | Skills / Connectors / Plugins 三 tab + **统一目录**（"Browse skills, connectors, and plugins in one directory"）；组织下发 + 同事发布；企业 scanning pass/warn/fail |
| Codex | Plugins 页 + `/plugins` 浏览器 | 按 Curated / Shared with you / Created by you 分组；插件打包 skills+apps+MCP；**权限复用全局 approval 设置，不为插件另设权限层** |
| Cursor | Customize 页 | 插件、MCP、rules、skills **同页管理**（scope 过滤）；Marketplace 卡片标注捆绑物（"8 MCP servers, 3 rules, 1 subagent…"）；**运行前逐工具审批在会话内**，企业 allowlist 在后台 |
| Devin | Customize 页五 tab | **官方文档明确写「取代了旧的 Settings→Plugins / Marketplace / Connections→MCP 三个分散页面」**：Plugins/Skills/MCPs/Hooks/Rules + scope tab；装前安全告知；来源=官方市场/git URL/zip/自建 |
| VS Code | Extensions view | 1.105 起内置 MCP marketplace **直接住进扩展面板**（`@mcp` 过滤器）——「扩展页吸收 MCP」的标志性事件 |
| ZCode | 侧栏「插件市场」入口 + 设置域管理页 | 市场浏览（分类）+ 已安装管理（启停/更新/卸载）+ 自建市场；兼容 Claude Code 插件市场 |

**公约提炼**：
- **E1 一个入口、tab 分区**：浏览（市场）/ 管理（已安装，按 scope）/ 来源（marketplaces）/ 异常（errors）四类分区是主流；Devin 证明了「把分散页合并成一个 Customize 页」是 2026 年的正确方向。
- **E2 安装时信任**：安装卡片里预告「将安装什么」（命令/工具/权限面），装前可见；**运行时工具审批不在扩展页**，留在会话内/全局审批设置。
- **E3 来源共存**：registry 市场 / git URL / 本地路径或 zip / 跨端同步，全部收进「来源管理」或「+ 添加」菜单。
- **E4 插件是打包单位**：skills 单独发、MCP 单独配是过渡态；终态是一个 plugin.json 打包多类能力（Claude Code / Codex / Cursor / Devin / Antigravity 全部如此，且 Claude Code 格式成为事实标准——ZCode 市场直接兼容它）。

**评审：Shannon 现状 vs 公约**。批次 E 已把 7 tab 收敛为「市场/已安装 + 管理」下拉，方向正确但只做了「chrome 层」。偏离点：
1. **审批流在扩展页之外**且三处冗余（§1.3 病灶 C）——竞品的「待处理」要么在安装流内（Will install），要么在统一审批队列（组织 review），没有一家把审批 Modal 挂在 Header + 设置页两处。
2. **管理下拉仍是 5 个平级类型页**，「管理」dropdown 实际是折叠的旧 7-tab（类型学没变：用户仍要先懂 MCP/Skill/Agent/数据源/插件五个名词才能管理）。
3. **MCP 三页割裂**：服务器配置（extensions）、工具授权（settings/permissions）、网关连接（settings/connections）互不链接、术语混用（「连接」同时指 gateway 和 datasources 两样东西）。
4. **skills 没有打包单位**：Shannon 的技能提案（skill loop 自动产技能候选）是自研孤岛，与市场安装的 skill 格式/管理路径不一致。

### 2.4 主要来源

Codex：developers.openai.com/codex（app / automations / review / cloud）；Claude：code.claude.com/docs/en（desktop / desktop-scheduled-tasks / agent-view / claude-code-on-the-web / claude-projects / sessions / routines）+ support.claude.com（统一目录 / plugin scanning / 组织管控）；ZCode：zcode.z.ai + /cn/docs（task-management / automations / idle-time-tasks / plugin）；Devin：docs.devin.ai（automations / auto-triage / agent-command-center / plugins-Customize）；Copilot：docs.github.com（cloud agent / 桌面 App agent sessions）；Cursor：cursor.com/docs（cloud-agent / plugins / mcp）+ cursor.com/marketplace；VS Code：code.visualstudio.com/updates（v1.101 / v1.105）+ extension-marketplace；Linear：linear.app/docs/inbox。完整 URL 见调研原始报告（已归档于本 PR 描述）。

---

## 3. 改进方案

> 总原则（沿袭前两轮）：全部在现有 token/i18n/Simple-Advanced 管线内实现；「一个概念一个词」；每项含验收口径；路由尽量不改、改显示层（书签兼容用 redirect）。

### 3.0 目标导航（North Star）

```
改后侧边栏（简单模式）                      对应竞品参照
────────────────────────────────    ─────────────────────────────
① 顶部动作区                          ZCode 四件套
   新对话 ⌘N（split：目标/例行/批量）
   搜索 ⌘K
   自动化 ⌘2（→ /tasks，唯一入口）
② 会话 rail（项目树：会话+自动化混排）    ZCode Z2/Z4；Codex 项目→thread
   [项目/智能/会话 镜头]
③ 主导航（4 行，减一）                  Claude 三 tab 的密度
   对话 /chat
   收件箱 /triage（badge=待处理数）      Codex Triage 范围 + 会话侧扩源
   扩展 /extensions                     ZCode 插件市场 / Devin Customize
   记忆 /memory
   （dev 追加：用量 /usage、指挥台 /opc）
④ 底部：模型徽章 / 模式切换 / 设置       不变
```

变更点：主导航去掉「任务」行（与顶部「自动化」按钮同址同义，去重去歧义，解决 G5/术语三轨）；「任务」一词退役，「自动化」（定义与触发）+「收件箱」（产出与待办）+ 会话（执行）三分。

### 3.1 方向一（任务 1）：分组/项目 → 项目实体化

**目标**：让「项目」从派生字符串变成引擎实体，使 Z2/Z4（项目为容器、自动化嵌项目）成立。

**阶段三-1 · 引擎侧（前置依赖，需单独立项）**

| 项 | 内容 | 依据 |
|---|---|---|
| P-E1 | `ScheduledRoutine` 增加 `working_dir: Option<String>`；创建表单与「在项目中新建例行」入口写入；例行运行产生的 inbox item 冗余同一字段 | SCHEDULED-FIX-PLAN.md §2 早已规划 `cwd` 未落地；`scheduled_routines.rs:316-361` 现无此字段 |
| P-E2 | goal run 记录 `working_dir`（从发起会话继承，`goal_commands.rs:760` 现为 None） | 同上 |
| P-E3 | 项目注册表：SQLite 表 `projects(id, path UNIQUE, name, icon, color, archived_at, created_at)` + Tauri 命令（list/upsert/rename/archive）；**种子策略：首次启动从现有 session working_dir 与 memory.project 标签自动登记**（adopt-not-migrate，不要求用户手动建项目） | `COMPETITIVE-ANALYSIS.md` P0；批次 F2 的 localStorage 注册表升级为引擎表；memory 已有 `list_memory_projects` 先例 |

**阶段三-2 · UI 侧（引擎就绪后）**

| 项 | 内容 | 验收 |
|---|---|---|
| P-U1 | rail 项目树嵌自动化：routine（时钟图标+周期描述+下次触发）与 goal 运行（旗标+迭代数）按 `working_dir` 归入项目，与会话混排（最近活跃排序）；「即将」徽章沿用现有 `next_fire_at<1h` 逻辑 | ZCode 截图 Z4 形态复现；`sidebar-automations` 独立小节在所有项目都有归属后退役 |
| P-U2 | 空项目与项目动作：注册表里 archived=否 且无内容的 проекта显示「暂无任务」置灰行；项目行右键/长按菜单：在此项目新建会话 / 新建例行 / 打开目录（Tauri opener）/ 重命名 / 图标颜色 / 归档 | 截图 Z7「暂无任务」复现；重命名持久化到引擎（localStorage 注册表迁移并废弃） |
| P-U3 | 项目成为筛选维度：/tasks、/triage、/memory（已有 project 过滤）支持 `?project=` 深链；rail 项目行点击=切换会话列表过滤（当前行为保持），项目名点击=展开/收起（保持） | 从项目头一键到达「该项目的自动化/收件箱/记忆」 |
| P-U4 | 「智能」镜头升为与「项目」并列的默认候选（记住用户上次选择即可，不强改默认） | G3 关闭 |

**明确不做**（评审需知）：不做独立「项目详情页」——ZCode/Codex 都是 rail-first，项目详情=会话列表过滤；若未来需要，项目头下拉里的「查看全部」再说。不改「对话」术语（前两轮已定：Simple mode 受众对黑话敏感；「任务」语义通过运行监控外显吸收，而非更名）。

### 3.2 方向二（任务 2）：任务/收件箱 → 「rail 监控 / 自动化配置 / 收件箱产出」三分

**设计立场**（基于 §2.2 评审）：不照抄任何一家，取 Codex 的显式 inbox + Claude/ZCode 的「会话即结果」+ 宿主派的「状态列」心智，收束为 Shannon 自己的三分法：

```
            ┌─────────────────────────────────────────┐
            │  rail（侧边栏）＝实时监控面（已建成）          │  谁在跑/卡审批/失败/多久
            ├─────────────────────────────────────────┤
   管理什么  │  自动化页（/tasks 收缩）＝定义与触发          │  例行/目标/批量/webhook/worktree
   跑什么    │  · 每个自动化：设定 / 历史 双 tab，历史行跳回会话 │  （ZCode 设定/历史范式）
            ├─────────────────────────────────────────┤
   处理结果  │  收件箱（/triage 扩源）＝统一「需要关注」流      │  Linear 心智：流式、可清空
   的产出    │  自动化产出 + 会话审批 + 会话失败 + 技能提案    │  行=投影，点开是本体
            └─────────────────────────────────────────┘
```

**阶段一 · 止血（纯前端，无引擎依赖，1–2 天）**

| 项 | 内容 | 验收 |
|---|---|---|
| T1 | **术语一轨**：`nav.scheduled`「任务」→「自动化」（en: Automations）；Header 标题、CommandPalette、/tasks 页内 H1、tab 文案同步（「例行/流水线」→ 并入「自动化」语汇）；旧 `nav.automation`「自动化」按钮与导航行去重：**主导航删「任务」行**（§3.0） | 全 UI grep「任务」仅存于「新建任务」类动作语境；i18n 测试过 |
| T2 | **互链闭环**：inbox 卡片加「查看来源自动化」（source_id → /tasks 例行 drawer，ZCode 历史跳会话的镜像）；Tasks 历史行加「在收件箱查看」；收件箱「继续会话」保持主操作 | UI-IMPROVEMENT-PLAN §6.3 三条未竟项全部关闭 |
| T3 | **审批面收敛**：删除 AdvancedSettings 的 SkillApprovalModal 挂载（`AdvancedSettings.tsx:518`），区块保留开关+计数+「在收件箱查看待审」链接；Header 铃铛不再劫持为技能审批（`Header.tsx:61-64` 改为一律 → /triage，技能提案作为收件箱条目呈现） | 技能审批 UI 只剩收件箱内一个审查面；事件源合并为一 |
| T4 | Tasks 页 tab 重命名与归位（Simple/Advanced 两档都生效）：「进行中」→「运行」；「流水线」「工作空间」移入 dev 档或「自动化」tab 的高级区；页面顶部唯一主 CTA「新建自动化」 | §3.3「7 控件无主次」关闭；Simple 模式首屏只剩 运行/历史 两 tab + 一个 CTA |

**阶段二 · 收件箱成为统一「需要关注」流（需要少量引擎配合，2–4 天）**

| 项 | 内容 | 验收 |
|---|---|---|
| T5 | **扩源**：inbox `source` 枚举增加 `session_approval`（权限审批请求）、`session_failed`（最近 turn 失败的会话）、`skill_candidate`（技能提案）；写入点与 rail 状态点/Header 铃铛数据同源（SessionActivity / skill-candidates 事件），**去重键**=（session_id, kind, turn）防止重复刷屏；「已读」语义=用户打开过对应会话/审批后自动 mark read | 收件箱一个页面可回答「有什么需要我」；rail 黄/红点与收件箱条目一一对应 |
| T6 | **收件箱页升级**：来源筛选加入新三源；默认视图=未读(pending) 置顶 + 全部；保留 归档/继续会话/重跑；新增「查看会话」（session_approval/failed 的主操作） | Codex Triage 的 all/unread 过滤 + Linear「行是投影」心智落地 |
| T7 | **数据双写收敛**（架构清理）：SQLite inbox 成为运行记录权威源；`HistoryView` 改读 inbox store（`list_task_executions` 保留为兼容 API，内部转读）；JSONL 镜像写保留一个版本期后退役（`inbox_commands.rs:303-315` 注释同步更新） | 「一个运行一个投影」；历史数据迁移脚本 + 抽样校验 |
| T8 | GoalRunPanel/BatchRunPanel **保留**在「自动化-运行」tab，语义重定位为「操作员视图」（跨会话的 goal/批量/子代理，机器视角）；rail 保持个人会话视角（goal badge，`SidebarSessions.tsx:653-686`）。裁决理由见 §5-1；阶段三 goal 嵌项目树后复查是否降级 | /tasks 不再是监控必经之路；rail 与运行卡是同一实体的两个镜头，不算重复投影 |

**评审取舍**：收件箱**不**做 snooze（Linear 有，但 Shannon 待办量级远小于 issue tracker，先不做）；收件箱**不**吸收普通会话完成通知（竞品公约：普通完成靠 OS 通知 + rail 绿点，只有「需要你」的才进收件箱——这正是 Codex Triage 的边界，也是它被诟病不够用的地方，我们用 `session_approval/failed` 补上，但**不**补「全部完成通知」，避免收件箱退化为通知中心）。

### 3.3 方向三（任务 3）：扩展 → 一页三区 + 信任安装 + 插件打包（长期）

**设计立场**：Devin 的 Customize 页证明了「合并分散页」是 2026 共识；Claude Code 的 `/plugin` 五 tab 是最成熟的分区语法。Shannon 不必照搬五 tab，但应吸收：**市场 / 已安装 / 待处理 三区 + 安装时信任 + 权限就近直达**。

**阶段二（与 3.2 阶段二同批，纯前端为主）**

| 项 | 内容 | 验收 |
|---|---|---|
| X1 | **「待处理」区**：/extensions 加第三个主 tab「待处理」（badge=数量）——聚合技能提案审查面板（现全局 toast/panel 的内容）+ 失败的 MCP 连接/安装错误（吸收 Claude Code Errors tab 语义）；审查操作与收件箱条目互通（同一动作两处入口，一份状态） | 技能提案从「toast 突袭 + 两个 Modal」收敛为一处队列；`SkillProposalsManager` 的 toast 降级为「有 N 条待审」轻提示，点进待处理区 |
| X2 | **安装时信任卡**：市场/已安装的安装流显示「将启用」清单（该 MCP 的工具数、该 skill 的触发描述、所需权限类别），确认后安装；数据源=现有 install-dialog 的 manifest（如无则读 manifest 摘要） | E2 公约落地；安装前可见权限面 |
| X3 | **权限就近直达**：MCP 服务器行内菜单加「工具权限」→ `/settings/permissions?scope=mcp:<server>` 深链（Permissions 页支持按前缀过滤）；「数据源」页头说明与网关的区别并互链 | 三页割裂（病灶 C）打通；术语按 UI-IMPROVEMENT-PLAN §6.1 收敛：网关页更名「网关」，「连接」归数据源（Connectors） |
| X4 | **管理下拉瘦身**：5 个类型管理页在「已安装」tab 内提供锚点分区（点击滚动/抽屉），「管理」下拉保留但标注为「高级管理」；ExtensionsHub.tsx 死代码删除（仅测试引用） | 类型学不再是唯一路径；死代码 grep 零引用 |

**阶段四 · 插件打包（长期，单独立项）**

| 项 | 内容 | 依据 |
|---|---|---|
| X5 | 定义 Shannon 插件包（建议兼容 Claude Code plugin 格式：`skills/ + agents/ + commands/ + .mcp.json` + manifest），市场条目=插件；迁移向导（Claude Code/ZCode 导入）产出直接落为插件 | E4 公约 + Claude Code 格式已成事实标准（ZCode 市场直接兼容它）；MigrationWizard 已在这两源导入，格式顺理成章 |
| X6 | 「来源管理」：市场（官方 registry）/ git URL / 本地目录 三来源共存于「+ 添加」菜单（现 McpAddServerDialog 已有手动/JSON/搜索三式，范式平移到插件层） | E3 公约 |
| X7 | 扩展 Stats（Claude Code 式）：每个 skill/MCP 的 context 成本与调用次数——数据源 /usage 已有 token 分类（ContextBreakdownCard），平移到扩展详情页 | Stats tab 是 Claude Code 差异化能力，成本低 |

### 3.4 改动后的导航全景（评审用对照）

| 顶层目的地 | 改前职责 | 改后职责 | 参照 |
|---|---|---|---|
| rail（会话区） | 会话列表+监控点+自动化小节 | **唯一实时监控面**：会话+自动化+goal 混排在项目树 | ZCode rail / Codex 项目栏 |
| 对话 /chat | 会话执行 | 不变（+审批 Modal 触发源不变） | Claude「会话即结果」 |
| 自动化（原 /tasks） | 5 tab 堆栈：监控+账本+配置+基础设施 | **定义与触发**：运行/历史 双视角、每自动化 设定/历史、唯一 CTA | ZCode/Codex/Claude/Devin 自动化页公约 |
| 收件箱 /triage | 自动化产出（5 源） | **统一「需要关注」流**：+审批/失败/技能提案（8 源） | Codex Triage + Linear 投影心智 |
| 扩展 /extensions | 市场/已安装 + 管理下拉 5 类型 | 市场/已安装/**待处理** 三区 + 信任安装 + 权限直达；长期插件包 | Devin Customize / Claude Code /plugin |
| 记忆 /memory | 独立页 | 不变（+`?project=` 深链） | — |
| 用量 / 指挥台（dev） | — | **均不动**（评审裁决 §5-5：/opc 维持现状，本轮任何批次不触及）。用量页的收敛由「运行观测」专项处理，且该专项范围明确排除 /opc（§5-4） | — |

---

## 4. 分期实施与验收

| 阶段 | 内容 | 依赖 | 估算 | 交付判据 |
|---|---|---|---|---|
| **一 · 止血** | T1 术语一轨+导航去重；T2 互链；T3 审批面收敛；T4 Tasks tab 归位 | 无（纯前端） | 1–2 天 | 「任务」术语退役；技能审批单一入口；全量 vitest+tsc+axe 绿 |
| **二 · 收件箱统一** | T5 扩源；T6 收件箱升级；X1 待处理区；X2 信任卡；X3 权限直达；X4 管理瘦身；T7 数据收敛 | T5 需后端写入点（desktop crate 内，非 shannon-core） | 2–4 天 | 收件箱一页回答「有什么需要我」；SQLite 成权威源 |
| **三 · 项目实体化** | P-E1/E2/E3 引擎；P-U1..U4 UI | 引擎排期（working_dir + projects 表） | 引擎 2–3 天 + UI 2–3 天 | 自动化嵌项目树；空项目；localStorage 注册表退役 |
| **四 · 插件打包** | X5 格式兼容；X6 来源管理；X7 Stats | 产品决策（格式兼容策略） | 另立项 | 与 Claude Code 市场互通 |

每阶段独立可合并、可单独回滚；阶段一/二不依赖阶段三（项目实体未就绪时，收件箱条目的 project 维度留空即可）。

## 5. 风险与开放问题

**风险**：
1. **T7 数据收敛**是唯一的破坏性重构——HistoryView 读路径切换需迁移脚本与回滚开关（保留 JSONL 读 fallback 一个版本）。
2. **T5 扩源的刷屏风险**——审批/失败条目必须有去重键与自动已读，否则收件箱会重蹈「通知中心」覆辙（竞品公约明确：只有「需要你」的进 inbox）。
3. **主导航删「任务」行**对老用户是习惯迁移——缓解：顶部「自动化 ⌘2」位置显眼 + CommandPalette 双词命中（搜「任务」「自动化」都到 /tasks）。
4. **引擎字段（routine.working_dir）跨 crate 改动**——SCHEDULED-FIX-PLAN 已有设计稿，风险可控但需引擎侧评审。

**开放问题 → 评审裁决**（2026-09-23 全部裁定；#1–#4 采纳建议并锁定，#5 由评审人改判。结论已同步进上文对应条目）：

| # | 问题 | 裁决 | 说明 |
|---|---|---|---|
| 1 | GoalRun/BatchRun 运行卡去留（T8） | ✅ **采纳：保留**在「自动化-运行」，重定位为操作员视图；阶段三后复查降级 | rail=个人会话镜头，运行卡=机器视角：best-of-N 的 N 分支对比、跨会话 goal 聚合本就无法压进单行会话；同一实体的两个镜头≠重复投影（要消灭的是「同一切片出现在两个页面」，见病灶 A）。参照 Devin Desktop 的 Agent Command Center：看板/操作员视图是真实需求，但应长在自动化页 |
| 2 | 技能提案主审查面 | ✅ **采纳：「扩展-待处理」为主面**，收件箱只放发现条目（「N 条待审」+ 跳转），不做卡内审批 | 技能提案是「资产审批」而非「运行结果」，心智归扩展域；审查需要 manifest 上下文（触发条件、内容、来源会话），收件箱卡太薄，塞进去只会催生第二个 Modal；竞品映射：Claude Code 的安装信任在 /plugin 流内、组织审核在技能管理域，没有一家把 rich review 塞进通知流 |
| 3 | 「智能」镜头是否升为默认 | ✅ **采纳：不改**——维持「项目」出厂默认 + 记住上次选择，可挂起 | ZCode 截图里用户实际停在「项目」档（分组 ✨ 只是排在第一位的备选，非默认）；≤1 个项目时项目视图自动退化为平铺（现有 `groups===null` 分支），新用户体验无损；智能分组的「运行中/需关注」空区不渲染，对轻用户本就不可见。升默认收益不可见、有迁移成本，待有使用数据再说 |
| 4 | 观测碎片化是否另立专项 | ✅ **采纳：另立「运行观测」专项，排阶段三之后**，本文档不含 | 时序依赖：T7 之后才有权威运行数据源，阶段三之后才有项目维度，per-project/per-run 成本聚合到那时才可做；且观测面多为 dev 档，优先级低于 Simple 模式 IA。范围建议：/usage 为唯一用量页（吸收 SessionUsageDialog 成会话下钻、timeline 成会话详情 tab）；ContextBreakdownCard 保留（职责是「当前上下文构成」不是账本）。**范围明确排除 /opc 页面及其仪表盘**（见 #5 裁决；未来如需整合 OPC 仪表盘，须单独提案） |
| 5 | /opc 长期定位 | ⛔ **评审人改判：维持现状，本轮不动**（不同意并入自动化页，不同意退役；v1.1 的 T9 撤销） | 评审人裁量（2026-09-23）：操作员看板作为独立工作台的价值优先于目的地数量收敛；/opc 仅 dev 档可见，不影响 Simple 模式 IA，本轮不动它没有代价。备注留档（不影响本轮实施）：OPC 与 Tasks 共用 catalog 数据的重复看板问题依然存在（OPC-SCHEDULED-GAP-ANALYSIS 原话「有但假」），待自动化页「运行/历史」新形态稳定后如需再议，须单独提案；在本方案全部批次（一~四）及「运行观测」专项中，/opc 页面均为免改区 |

## 6. 附录

- 前两轮分析：`docs/design/ui-audit-2026-09/ZCODE-DELTA-ANALYSIS-2026-09{,-20}.md`、`UI-IMPROVEMENT-PLAN-2026-09.md`
- 现状锚点核实：`dev@0557f8fe`，`Sidebar.tsx` / `SidebarSessions.tsx` / `Tasks.tsx` / `Triage.tsx` / `Extensions.tsx` / `Header.tsx` / `AdvancedSettings.tsx` / `inbox_commands.rs` / `scheduled_routines.rs` / `goal_commands.rs`（行号见文内）
- 竞品调研两份原始报告（含完整 URL 与不确定项标注）：Codex 桌面 Automations-Triage、Claude Desktop 三 tab/Routines/Overview、ZCode v3.14.3 自动化/闲时任务/插件市场、Devin Customize 五 tab、Copilot Mission Control、Cursor Customize、VS Code `@mcp`、Linear Inbox
- ZCode 截图原件：`reference/zcode/截图 2026-09-20 23-58-44.png`
