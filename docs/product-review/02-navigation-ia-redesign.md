# Shannon Desktop 左侧导航 IA 重构方案

> 文档版本:v1.0 · 2026-06-15
> 作者:IA + 产品设计评审
> 适用范围:Shannon Desktop (`shannon-desktop/ui`),Tauri + React 19
> 目标:从 "AI Code Assistant" 转型为面向普通消费者的 AI agent 桌面产品,
> 重构左侧导航的信息架构(IA)、命名系统、视觉层级与渐进式披露策略。

---

## 〇、执行摘要(TL;DR)

当前 Shannon Desktop 的左侧导航共有 **22 个入口(11 一级 + 11 二级)**,
对普通用户构成严重的认知过载。核心问题不在"项太多",而在 **三个根本性错误**:

1. **以功能为中心,而非以用户任务为中心** — "Mission Control / Triage / Scheduled / Tasks"
   在用户眼里几乎是一回事(都是"任务"),却被拆成 4 个并列入口。
2. **开发者向与用户向混杂** — Quick Fix、Editor、Performance、Hook Events 是给程序员
   用的工具,却和 Chat 同一级别出现,普通用户根本看不懂。
3. **命名混乱,术语黑话密集** — "OPC"、"Hook Events"、"Mission Control"、"Triage"
   这类术语对非技术用户毫无意义,即使是技术用户也需要学习成本。

本方案推荐 **"任务中心化" 重构(方案 A)**:把 22 个入口压缩到 **5 个主导航 + 2 个折叠区 + Settings**,
并给出配套的命名表、视觉层级和渐进式披露(开发者模式开关)。
备选 **方案 B(聊天优先)** 更激进,把 Shannon 变成"对话即界面"的单页产品。

预期收益:
- 主导航认知负担从 22 → 5(降低 **77%**)
- 新用户首次成功路径从"猜菜单"变成"读 5 个名词"
- 开发者功能不丢,通过 `Settings > Advanced > Developer Mode` 显式开启

---

## 一、现状诊断

### 1.1 当前导航盘点

数据来源:`ui/src/components/Sidebar.tsx` + `ui/src/App.tsx`(22 个路由,32 个组件懒加载)。

| # | 当前名称 | 路由 | 层级 | 默认状态 | 快捷键 | 目标用户 | 实际功能(代码摘要) |
|---|---------|------|------|---------|--------|---------|-------------------|
| 1 | **Chat** | `/chat` | 一级 | 显示 | ⌘1 | 全部 | 聊天会话,流式响应,工具调用 |
| 2 | **Goals** | `/goals` | 一级 | 显示 | ⌘2 | 全部 | 任务树 + agent pipeline + 人在回路审批(`Goals.tsx:9-24`) |
| 3 | **Scheduled** | `/tasks` | 一级 | 显示 | ⌘3 | 高级 | 定时任务/触发性 routines 的 CRUD + 日历 + DAG(`Tasks.tsx:1-15`) |
| 4 | **Mission Control** | `/mission-control` | 一级 | 显示 | ⌘4 | 高级 | **只读** 跨团队任务看板(`MissionControl.tsx:1-10`) |
| 5 | **Triage** | `/triage` | 一级 | 显示 | — | 全部 | 失败/预算/待审项的收件箱(`Triage.tsx:1-10`) |
| 6 | Extensions ▾ | `/extensions/*` | 分组 | 展开 | — | | |
| 6.1 | Skills | `/extensions/skills` | 二级 | | — | 全部 | 浏览技能/命令模板 |
| 6.2 | My Agents | `/extensions/agents` | 二级 | | — | 高级 | 查看/配置运行中的 agents |
| 6.3 | Data Sources | `/extensions/datasources` | 二级 | | — | 高级 | MCP server 连接管理 |
| 7 | Automation ▾ | `/routines`+`/hooks`+`/profiles` | 分组 | **折叠** | — | | |
| 7.1 | Routines | `/routines` | 二级 | | — | 高级 | 触发性 routine CRUD,触发器含 `PostToolUse` 等 15 种(`Routines.tsx:6-21`) |
| 7.2 | Hook Events | `/hooks` | 二级 | | — | 开发者 | 浏览 30+ 生命周期事件目录(`Hooks.tsx:6-14`) |
| 7.3 | Profiles | `/profiles` | 二级 | | — | 高级 | 权限配置(Read/Write/Bash/Delete/Network 白名单)(`Profiles.tsx:19-25`) |
| 8 | OPC ▾ | `/opc` | 分组 | **展开** | — | 实验 | "One Person Company",agent 编排看板 + 战略焦点(`OPC.tsx:1-15`) |
| 9 | **Quick Fix** | `/quickfix` | 一级 | 显示 | — | 开发者 | LSP 代码动作调试器,粘贴诊断查修复(`QuickFix.tsx:1-9`) |
| 10 | **Editor** | `/editor` | 一级 | 显示 | — | 开发者 | CodeMirror + LSP squiggles 调试(`Editor.tsx:1-10`) |
| 11 | **Performance** | `/perf` | 一级 | 显示 | — | 开发者 | tracing JSON 离线分析(`Perf.tsx:1-15`) |
| 12 | Settings ▾ | `/settings/*` | 分组 | 折叠 | — | 全部 | 5 个子项:General / Theme / Models / Usage & Billing / Advanced |

**总计**:11 一级 + 11 二级 = **22 个入口**。
**默认展开**:Chat / Goals / Scheduled / Mission Control / Triage / Extensions / OPC(7 项一级,3 个分组中 2 个展开)。
**默认可见的二级**:Extensions 下 3 项 + OPC 下 1 项 = **4 项**(因为是展开状态)。
**首屏可见项**:11(一级) + 4(展开二级) = **15 项**。

### 1.2 核心问题清单

> 每条都给出代码证据。

#### P1. **任务概念被拆成 4 个并列入口,语义高度重叠**

证据:阅读 `Tasks.tsx:1-15`、`MissionControl.tsx:1-10`、`OPC.tsx:1-15` 三个文件的头部注释,
工程师自己都写了一段"和其他两个页面的区别"说明,承认三者关系混乱:

```
Tasks (this page): full CRUD for scheduled routines + history + worktrees.
MissionControl: read-only kanban across all teams (observation).
OPC: agent-orchestration workspace with optimistic DnD (write surface).
```

加上 `Goals.tsx` 也是基于 `tasks` 数组渲染(`Goals.tsx:16-18`),用户看到的是
**4 个页面消费同一个数据源(`useApp().tasks`)**。新用户的第一反应必然是:
"我现在应该点哪一个?"

**影响**:这是导航混乱的"罪魁祸首"。任何重构方案必须首先回答这 4 个的关系。

#### P2. **开发者工具与用户功能混杂在同一视觉层级**

证据:`Sidebar.tsx:253-290` 把 Quick Fix / Editor / Performance 三个**纯开发者工具**
放在 Chat / Goals / Triage 后面,使用相同的字号、间距、图标风格。
- `QuickFix.tsx:1-9`:要求用户粘贴 `/abs/path/to/src/lib.rs`,这显然不是消费者行为。
- `Editor.tsx:1-10`:CodeMirror + LSP 诊断,需要理解 `rust-analyzer` / `tsserver` 等。
- `Perf.tsx:1-15`:要求用户粘贴 `SHANNON_LOG_FORMAT=json` 的 stderr 输出。

这三项对非程序员毫无意义,却占据宝贵的"一级菜单"位置。

#### P3. **命名黑话密集,普通用户无法理解**

证据(逐项分析):

| 名称 | 用户首次理解成本 | 程度 |
|------|----------------|------|
| **OPC** | 缩写,无展开提示(需点开才看到 "One Person Company") | 极高 |
| **Mission Control** | NASA 梗,看板是干什么的? | 高 |
| **Triage** | 医学术语"分诊",翻译过来还是不懂 | 高 |
| **Hook Events** | React/Unix 黑话 | 极高(对非程序员) |
| **Profiles** | 是用户档案?权限档案?订阅档案? | 中 |
| **Routines** | 是日常例程?是定时任务? | 中 |
| **Scheduled** | 比较好,但和 Routines 重叠 | 低 |
| **Extensions** | 浏览器插件概念借用,OK | 低 |

**影响**:6/11 项一级菜单需要二次点击才能理解。

#### P4. **实验性功能(OPC)默认展开,占据视觉权重**

证据:`Sidebar.tsx:16` `useState(true)` 把 `opcOpen` 默认设为展开,带 "Experiment" badge
(`Sidebar.tsx:234`)却和成熟功能混在一起。这意味着:
- 每个新用户都会看到一个"实验"标志在主导航里晃
- 真正成熟的功能(Chat / Goals)被挤到上面更小的区域

**影响**:破坏产品成熟度感知,增加用户犹豫。

#### P5. **折叠分组初始状态不一致,无规则可循**

证据(`Sidebar.tsx:16-19`):
```ts
const [opcOpen, setOpcOpen] = useState(true);          // 展开
const [extensionsOpen, setExtensionsOpen] = useState(true);   // 展开
const [automationOpen, setAutomationOpen] = useState(false);  // 折叠
const [settingsOpen, setSettingsOpen] = useState(false);      // 折叠
```

四个分组的初始状态没有规律:为什么 Extensions 默认展开而 Automation 默认折叠?
为什么实验性的 OPC 默认展开?用户每次刷新看到的导航形态都不同(取决于上次操作),
缺乏"肌肉记忆"。

#### P6. **快捷键只覆盖前 4 项,覆盖率仅 36%**

证据:`Sidebar.tsx:119,124,129,134` — 只有 Chat / Goals / Scheduled / Mission Control 有 ⌘1-4。
Triage、Extensions、Settings、Quick Fix、Editor、Performance 全部无快捷键。
但 Quick Fix / Editor / Performance 是开发者高频调试工具,反而没有快捷键 — 优先级倒挂。

#### P7. **Command Palette(⌘K)与 Sidebar 信息架构脱节**

证据:`CommandPalette.tsx:23-32` 只列了 9 个 Pages 入口,**缺少 Mission Control / Triage /
Quick Fix / Editor / Performance / Hooks / Profiles / Routines / Extensions 子项**。
意味着用户用 ⌘K 搜不到 Sidebar 上能点的功能 — 双重信息架构,自相矛盾。

#### P8. **Settings 子菜单的"Advanced"是杂物间**

证据:`Sidebar.tsx:341-348` 把 Advanced 与 General / Theme / Models / Billing 并列,
但 Advanced 里塞了"内存管理 / 数据隐私 / 调试控制台 / 系统日志 / API Keys / 工厂重置"
(`user-stories.md` US-SET-04)— 跨越"高级配置"到"危险操作",颗粒度严重不均。

#### P9. **"Extensions" 分组下的 "My Agents" 与 OPC 里的 "Agent Swarm" 概念重叠**

证据:
- `Extensions.tsx:9` 子 tab `My Agents`(`/extensions/agents`)— 看 agent 列表
- `OPC.tsx:40` `<OPCAgentSwarm agents={agents} />` — OPC 页面也有 agent 列表

两个入口看同一份数据,用户不知道去哪里管 agent。

#### P10. **Header 的页面标题映射不完整,与 Sidebar 不一致**

证据:`Header.tsx:10-18` 的 `TITLE_MAP` 只有 7 项,缺少 Mission Control / Triage /
Quick Fix / Editor / Performance / Routines / Hooks / Profiles 的标题 —
点击这些页面时 Header 显示默认的 "Chat",**用户在哪个页面都看不出来**。
这是信息架构断层的直接证据。

---

### 1.3 用户视角痛点(引用新手用户的心声)

**模拟场景**:一个不懂代码的内容创作者第一次打开 Shannon Desktop。

> "我看到左边有 Chat,这个我懂。下面是 Goals — 目标?我要写目标吗?
> Scheduled — 我要排日程?Mission Control — 这是什么 NASA 控制台?
> Triage — 急诊室?Extensions — 浏览器插件?Automation — 我要配置自动化?
> OPC — 三个字母,完全不懂。下面还有 Quick Fix、Editor、Performance……
> 我只是想让 AI 帮我写点东西,为什么要先学 22 个名词?"

**这段内心独白对应的导航失败模式**:

1. **"我不知道我现在该用哪个"** — Goals / Scheduled / Mission Control / Triage / OPC 都
   像是"管任务的",但具体差异不可言说。
2. **"我害怕点错"** — OPC 标着 Experiment,Hook Events 一堆代码字眼,
   普通用户会绕开这些区域,导致产品 70% 的功能"看不见"。
3. **"我学不会快捷键"** — 只有 4 项有快捷键,但用户连名称都记不住,更别说 ⌘3 是哪个。
4. **"我不知道自己在哪里"** — Header 标题对一半页面失效(P10),用户失去方位感。

---

## 二、竞品参考

### 2.1 ChatGPT Desktop(OpenAI)

**导航哲学**:**极简单入口,历史驱动**。
- 左侧只有:New chat / Search / Library / Sora / GPTs / Settings(齿轮,右下角)
- 主体是"会话历史列表",占据 80% 的侧边栏空间
- **学到的**:用户 90% 时间在 Chat,其他功能要么收进 `⋯` 菜单,要么是模态/抽屉

### 2.2 Claude Desktop(Anthropic)

**导航哲学**:**对话即界面,项目为单位**。
- 左侧:New chat / 最近会话 / Projects(可折叠)/ Settings(底部)
- 几乎没有"工具页",所有功能都在对话内通过 @ 提及或工具栏触发
- **学到的**:不要为每个工具开一个页面,工具应该在用户需要时出现

### 2.3 Cursor

**导航哲学**:**编辑器优先,AI 是侧边栏**。
- 左侧是 VSCode 的文件树,AI Chat 是右侧 panel
- Settings 是模态,Command Palette(⌘K)是核心导航
- **学到的**:开发者工具(Quick Fix / Editor / Perf)应该收进 Command Palette 或
  Settings,而不是占据导航

### 2.4 Notion

**导航哲学**:**以"内容"为信息架构**。
- 左侧是用户自己的 workspace 树(页面/数据库),不是"功能列表"
- Settings 是模态,几乎不可见
- **学到的**:如果 Shannon 转向消费者,**让用户的目标/任务成为导航主体**,
  而不是 Shannon 的功能清单

### 2.5 Linear

**导航哲学**:** Inbox 优先 + 视图收拢**。
- 左侧:Inbox / My Issues / Active / Backlog / Views(自定义)/ Teams(折叠)/ Settings
- 所有"工作流"汇聚到 Inbox,其他视图是辅助
- **学到的**:**单一收件箱模型** — Triage + Mission Control + Scheduled 可以借鉴
  Linear 的 Inbox 思路,合并成"今日待办"

### 2.6 综合借鉴

| 竞品 | 借鉴点 | 在 Shannon 的应用 |
|------|--------|------------------|
| ChatGPT/Claude | 主导航 ≤ 5 项 | 方案 A 把主导航压到 5 项 |
| Cursor | 开发者工具进 Command Palette | Quick Fix/Editor/Perf 移入 ⌘K 和 Developer Mode |
| Linear | Inbox 收拢多源 | Triage + 通知合并为 "Today / 收件箱" |
| Notion | 内容为 IA | 方案 B 让用户的 Goals 成为导航 |
| 所有竞品 | Settings 是模态/底部 | 保留底部,但拆出 Developer |

---

## 三、重构原则

### 原则 1:**任务为中心,不是功能为中心**
> 用户来 Shannon 是为了"完成一件事",不是为了"用一个功能"。
> 导航项应该回答 "我要做什么",而不是 "Shannon 有什么"。

应用:Goals / Scheduled / Mission Control / Triage / OPC 这五个"任务管理"页面
必须收敛为 1-2 个,以"任务视角"切分(如:今天 / 全部 / 已完成),而非"功能视角"。

### 原则 2:**渐进式披露(Progressive Disclosure)**
> 新手看到 5 个名词就够,专家通过显式开关解锁全部能力。

应用:新增 `Settings > Advanced > Developer Mode` 开关,
默认关闭 Quick Fix / Editor / Performance / Hook Events / Profiles / Routines 的可见性。

### 原则 3:**命名说人话(Speak Human)**
> 用 5 岁小孩能懂的词。删除所有内部代号、缩写、技术黑话。

应用:见第五节命名表。OPC 删除或改名,Mission Control → 任务看板,Triage → 收件箱/待处理。

### 原则 4:**视觉层级 = 重要性层级**
> 主导航 / 次级 / 隐藏 三层,每层用不同字号、颜色、间距明显区分。

应用:见第六节。一级菜单用 14px Medium,次级用 13px Regular + 缩进,
隐藏项完全不渲染(而非"灰色禁用")。

### 原则 5:**单一可信来源(Single Source of Truth)**
> Sidebar、Command Palette、Header 标题、快捷键必须共享同一份 IA 配置。

应用:抽取 `navigation.ts` 配置文件,Sidebar / Palette / Header 全部从同一处读取,
根治 P7 / P10 的不一致。

---

## 四、新 IA 方案

### 4.1 推荐方案 A:【任务中心化 IA】

**核心思想**:把 22 项压缩到 **5 个主导航 + 2 个折叠区(Workspace / Developer)+ Settings**。
普通用户看到的只是 5 个名词 + Settings 齿轮。

#### 4.1.1 ASCII 结构图

```
┌─────────────────────────────────────────────┐
│  [+]  New Chat                              │  ← 主 CTA,不变
├─────────────────────────────────────────────┤
│  💬  Chat              ⌘1                   │  主导航(每天用)
│  🎯  Today             ⌘2                   │  ← 合并 Goals + Triage
│  📋  Tasks             ⌘3                   │  ← 合并 Scheduled + Mission Control + OPC
│  🔌  Extensions        ⌘4                   │  ← Skills + Agents + DataSources 合并单页
│  📊  Activity          ⌘5                   │  ← 新增,日志/用量统一入口
├─────────────────────────────────────────────┤  ← 分隔线
│  🧩  Workspace ▾                             │  折叠区(每周用)
│      • Routines                             │
│      • Profiles                             │
│      • Hook Events  (仅 Developer Mode)     │
├─────────────────────────────────────────────┤
│  🛠  Developer ▾    [仅 Developer Mode 显示] │  折叠区(几乎不用)
│      • Quick Fix                            │
│      • Code Editor                          │
│      • Performance Tracing                  │
├─────────────────────────────────────────────┤
│  ⚙   Settings                               │  底部(偶尔用)
│      • General / Appearance / Models        │
│      • Usage & Billing                      │
│      • Advanced  (含 Developer Mode 开关)   │
├─────────────────────────────────────────────┤
│  ● claude-sonnet-4.5                        │  状态栏(不变)
└─────────────────────────────────────────────┘
```

**入口数对比**:
- 默认可见一级:5(从 11 → 5, **−55%**)
- 默认可见二级:0(从 4 → 0, **−100%**)
- 总入口:14(从 22 → 14, **−36%**;Developer Mode 关闭时为 **11**)

#### 4.1.2 合并逻辑详解

**合并 1:Goals + Triage → "Today"**
- 理由:两者都是"今天需要我关注的事"。Goals 是"我在推进的任务",
  Triage 是"出问题需要我处理的事"(`Triage.tsx:24-37` 的 failed_run / needs_review)。
- 实现:Today 页面顶部 tab 切换(`In Progress | Needs Review | Done`),
  数据源仍是 `useApp().tasks` + `useTriageItems()`。
- ⌘2 改绑 Today。

**合并 2:Scheduled + Mission Control + OPC → "Tasks"**
- 理由:三者注释里已经写明区别(`Tasks.tsx:6-10` / `MissionControl.tsx:6-9` / `OPC.tsx:6-10`),
  但对用户都是"任务"。视角不同(CRUD / 只读看板 / agent 编排)应该用 **tab/视图切换**,
  不是不同入口。
- 实现:Tasks 页面顶部 4 个视图按钮:
  `List | Calendar | Kanban | Agents`
  - List = 原 Tasks
  - Calendar = 原 Tasks 的 calendarView
  - Kanban = 原 Mission Control
  - Agents = 原 OPC(战略焦点 + Agent 编排)
- OPC 的 "Experiment" badge 保留在 Agents tab 上,直到毕业。

**合并 3:Extensions 子页 → 单一 Extensions 页 + tab**
- 理由:`Extensions.tsx:6-10` 本来就是 tab 切换(Skills/Agents/DataSources)。
  Sidebar 重复列出 3 个子项是冗余。
- 实现:Sider 只放一个 Extensions 入口,页内 tab 不变。

**合并 4:Quick Fix + Editor + Performance → "Developer" 折叠区**
- 理由:三者都是开发者调试工具,普通用户从不使用。
- 实现:折叠区**默认不渲染**,只在 `Settings > Advanced > Developer Mode = on` 时出现。

**合并 5:Settings 子项 5 → 3**
- General 不变
- Theme → Appearance(更通用)
- Models 不变
- Usage & Billing 不变
- Advanced 不变,但**新增 "Developer Mode" 开关**作为入口

**删除 / 重命名 1:OPC 折叠区**
- 理由:OPC 是单页,没必要做"折叠区 + 1 个子项"(原 `Sidebar.tsx:240-249`)。
- 处理:整体并入 Tasks > Agents 视图。

#### 4.1.3 适用场景

- Shannon 想同时服务**普通消费者 + 开发者**(主推方案)
- 保留所有现有功能,通过 Developer Mode 解锁
- 迁移成本中等:主要是路由合并 + Sidebar 重排

#### 4.1.4 路由变更表

| 旧路由 | 新路由 | 状态 |
|--------|--------|------|
| `/chat` | `/chat` | 不变 |
| `/goals` + `/triage` | `/today` | 合并(旧路由 301 重定向) |
| `/tasks` + `/mission-control` + `/opc` | `/tasks?view=list\|calendar\|kanban\|agents` | 合并 |
| `/extensions/skills\|agents\|datasources` | `/extensions?tab=...` | 简化(子路由保留兼容) |
| `/routines` `/hooks` `/profiles` | `/workspace/routines` 等 | 移入 Workspace |
| `/quickfix` `/editor` `/perf` | `/dev/quickfix` 等 | 移入 Developer(隐藏) |
| `/settings/*` | `/settings/*` | General/Appearance/Models/Billing/Advanced |

---

### 4.2 备选方案 B:【聊天优先单页 IA】

**核心思想**:借鉴 Claude Desktop,**Chat 是唯一入口**,其他功能全部按需浮现。

#### 4.2.1 ASCII 结构图

```
┌─────────────────────────────────────────────┐
│  [+]  New Chat                              │
├─────────────────────────────────────────────┤
│  💬  Chats              ⌘1                   │  ← 唯一主导航
│  📜  History            ⌘2                   │  ← 会话历史(从 Chat 抽出)
├─────────────────────────────────────────────┤
│  🧩  More ▾                                  │  折叠区(全部其他功能)
│      • Today(任务/分诊)                    │
│      • Tasks(定时/看板/agents)             │
│      • Extensions                           │
│      • Activity                             │
├─────────────────────────────────────────────┤
│  ⚙   Settings                               │
└─────────────────────────────────────────────┘
```

**入口数**:2 主 + 4 次 + Settings = **7 项**。

#### 4.2.2 关键差异

- **Tasks / Goals / Triage 全部塞进 More**,通过模态/抽屉打开,不是常驻页面
- Chat 内嵌"任务卡片",用户在对话里创建/查看任务,不离开 Chat
- Extensions 通过 Chat 输入框的 `@` 触发(类似 Claude Desktop)
- Developer 功能只能通过 ⌘K Command Palette 访问

#### 4.2.3 适用场景

- Shannon **完全转型消费者产品**,放弃开发者定位
- 工程量大:需要重写 Tasks/Goals/OPC 为 Chat 内嵌组件
- 风险高:现有用户(以开发者为主)会强烈反对

#### 4.2.4 对比表

| 维度 | 方案 A(任务中心) | 方案 B(聊天优先) |
|------|------------------|------------------|
| 主导航数 | 5 | 2 |
| 改动量 | 中(路由合并) | 大(Chat 内嵌任务) |
| 用户群体 | 消费者 + 开发者 | 仅消费者 |
| 现有用户冲击 | 低 | 高 |
| 转型彻底度 | 中 | 高 |
| 推荐度 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ |

**建议**:**先上方案 A**(2-3 周工作量),观察用户数据 1-2 个月。
如果消费者留存明显优于开发者,再渐进向方案 B 演进。

---

## 五、命名重构表

| 旧名(英) | 旧名(中) | 新名(英) | 新名(中) | 理由 / 用户测试 |
|----------|---------|---------|---------|---------------|
| Chat | 聊天 | **Chat** | **聊天** | 不变,通用 |
| Goals | 目标 | **Today** | **今日** | "Goals" 名不副实(其实是任务树);"Today" 明确时间范围 |
| Scheduled | 计划任务 | (合并到 Tasks) | — | 与 Routines 重叠 |
| Mission Control | 任务控制台 | (合并到 Tasks > Kanban) | — | NASA 梗,普通用户不懂 |
| Triage | 分诊 | **Today > Needs Review** | **今日 > 待审** | 医学术语,移入 Today 的 tab |
| Tasks (旧) | 任务 | **Tasks** | **任务** | 保留,但承载更多视图 |
| Extensions | 扩展 | **Extensions** | **扩展** | 不变,通用 |
| Skills | 技能 | **Skills** | **技能** | 不变 |
| My Agents | 我的 Agents | **Agents** | **智能体** | 去"My"前缀(冗余) |
| Data Sources | 数据源 | **Data Sources** | **数据源** | 不变 |
| Automation ▾ | 自动化 | **Workspace ▾** | **工作区 ▾** | "Automation" 误导(里面 Profiles 不是自动化) |
| Routines | 例程 | **Routines** | **自动化例程** | 加"自动化"前缀,明确含义 |
| Hook Events | Hook 事件 | **Hook Events** (Dev only) | **生命周期事件**(仅开发者模式) | 移入 Developer,加副标题 |
| Profiles | 配置文件 | **Permission Profiles** | **权限配置** | 明确是"权限",不是"用户档案" |
| OPC ▾ | OPC | (删除,并入 Tasks > Agents) | — | 缩写无意义,实验性 |
| One Person Company | 一人公司 | **Tasks > Agents** | **任务 > Agents 视图** | 作为 Tasks 的一个视图保留 |
| Quick Fix | 快速修复 | **Quick Fix** (Dev) | **代码修复**(开发者) | 移入 Developer |
| Editor | 编辑器 | **Code Editor** (Dev) | **代码编辑器**(开发者) | 移入 Developer,加 Code 前缀 |
| Performance | 性能 | **Performance Tracing** (Dev) | **性能追踪**(开发者) | 移入 Developer,加 Tracing 后缀 |
| Settings | 设置 | **Settings** | **设置** | 不变 |
| General | 通用 | **General** | **通用** | 不变 |
| Theme | 主题 | **Appearance** | **外观** | 含主题 + 字号 + 密度 |
| Models | 模型 | **Models** | **模型** | 不变 |
| Usage & Billing | 用量与计费 | **Usage & Billing** | **用量与计费** | 不变 |
| Advanced | 高级 | **Advanced** | **高级** | 不变,但加 Developer Mode 开关 |

**新增**:
| 新名(英) | 新名(中) | 用途 |
|---------|---------|------|
| **Activity** | **活动** | 日志/事件流/Token 用量历史,统一入口 |
| **Workspace** | **工作区** | 收纳 Routines / Profiles / Hook Events |
| **Developer** | **开发者** | 收纳 Quick Fix / Code Editor / Performance |
| **Developer Mode** | **开发者模式** | Settings > Advanced 里的开关 |
| **Today** | **今日** | 合并 Goals + Triage 的新主页 |

---

## 六、视觉与交互设计

### 6.1 分组与分隔

**三段式纵向分隔**(借鉴 Linear / Notion):

```
┌───────────────────┐
│ [New Chat]        │  ← Action 区
├───────────────────┤  ← 1px 分隔线 (border-outline-variant/30)
│ 主导航 5 项       │  ← Primary 区(每项 14px Medium)
│   💬 Chat         │
│   🎯 Today        │
│   📋 Tasks        │
│   🔌 Extensions   │
│   📊 Activity     │
├───────────────────┤  ← 1px 分隔线
│ Workspace ▾       │  ← Secondary 区(13px Regular,默认折叠)
│ Developer ▾       │     (仅 Dev Mode 显示)
├───────────────────┤
│ (弹性空白)        │  ← flex-1,把 Settings 推到底
├───────────────────┤
│ ⚙ Settings        │  ← Footer 区
│ ● model status    │
└───────────────────┘
```

关键改动(对比当前 `Sidebar.tsx:84-360`):
- 删除"Extensions / Automation / OPC / Settings"四个分组混排,
  改为 **Primary / Secondary / Footer** 三段
- 用 `mt-auto`(已存在于 `Sidebar.tsx:294`)把 Settings 推到底,确保视觉锚点
- Secondary 区**默认全部折叠**(改变当前 OPC/Extensions 默认展开的状态)

### 6.2 图标系统

**现状问题**:`Sidebar.tsx` 用 `material-symbols-outlined`,但风格不统一:
- 主导航用 outlined + fill(`Sidebar.tsx:97` hub 用 `FILL 1`)
- 子项用纯 outlined
- OPC 用 emoji-like 的 `auto_awesome`
- Quick Fix 用 `build`,Editor 用 `code`,Perf 用 `bar_chart` — 风格混杂

**新方案**:**统一使用 Material Symbols Outlined(Rounded 变体)**,无 Fill,
对所有主导航项采用相同视觉权重。子项不用图标,改用 **彩色圆点**(已有的 `Sidebar.tsx:158` 模式),
颜色编码:
- 蓝色(Primary):激活
- 灰色(Outline):未激活
- 红色(Error):有错误(Triage 有未读时)
- 黄色(Warning):有警告

### 6.3 默认折叠状态规则

| 分组 | 默认状态 | 规则 |
|------|---------|------|
| Primary(5 项) | 始终展开 | 不可折叠 |
| Workspace | **折叠** | 二级功能,需要时点开 |
| Developer | **不渲染** | Developer Mode = off 时不渲染 |
| Settings | 折叠 | 点击展开子项 |

**记忆性**:用户手动展开/折叠的状态写入 `localStorage`(key: `shannon-nav-workspace-open`),
刷新后恢复。当前代码每个分组独立 `useState`(`Sidebar.tsx:16-19`),不持久化 — 修复。

### 6.4 新手模式 vs 专家模式

**新手模式**(默认,Developer Mode = off):
- 主导航 5 项可见
- Workspace 折叠(可见但收起)
- Developer 完全不渲染
- Triage 有未读时,Today 图标显示红点 + badge 数字
- 首次打开显示 3 步引导("先从 Chat 开始 / 任务在 Tasks / 设置在右下")

**专家模式**(Developer Mode = on):
- 新增 Developer 折叠区
- Quick Fix / Editor / Perf 有快捷键(⌘⇧F / ⌘⇧E / ⌘⇧P)
- Hook Events 在 Workspace 内可见
- 显示高级 toast(包含 token / latency)

**切换入口**:`Settings > Advanced > Developer Mode` 开关,
首次开启时弹确认对话框("这将显示开发者工具,继续?")。

---

## 七、迁移路径

### P0 — 立刻能改(1 周内,纯 UI 重构,不动路由)

| 动作 | 文件 | 工作量 |
|------|------|--------|
| 1. 抽取 `navigation.ts` 配置(单一 IA 来源) | 新建 `ui/src/config/navigation.ts` | 0.5 天 |
| 2. Sidebar / CommandPalette / Header 全部从 `navigation.ts` 读 | 3 个文件 | 0.5 天 |
| 3. 补全 Header 的 `TITLE_MAP`(P10) | `Header.tsx:10-18` | 0.5 天 |
| 4. 补全 CommandPalette 的 Pages 列表(P7) | `CommandPalette.tsx:23-32` | 0.5 天 |
| 5. 把 Quick Fix / Editor / Perf 用视觉降级(灰色 + 小字) | `Sidebar.tsx:253-290` | 0.5 天 |
| 6. 折叠状态写入 localStorage | `Sidebar.tsx:16-19` | 0.5 天 |
| 7. 重命名:Goals → Today(label 改,Sidebar.tsx:121-125) | `Sidebar.tsx` | 0.5 天 |
| 8. Sidebar 副标题从 "AI Code Assistant" 改为 "AI Agent"(配合转型) | `Sidebar.tsx:102` | 5 分钟 |

### P1 — 配合产品转型(2-3 周,路由合并)

| 动作 | 影响范围 | 风险 |
|------|---------|------|
| 1. 路由合并:Goals + Triage → `/today`(旧路由 301) | App.tsx + 新 Today.tsx | 低(数据源相同) |
| 2. 路由合并:Mission Control + OPC → Tasks 的 view tab | App.tsx + Tasks.tsx | 中(视图切换逻辑) |
| 3. 新增 `Settings > Advanced > Developer Mode` 开关 | AdvancedSettings.tsx | 低 |
| 4. Developer Mode 控制Sidebar 渲染 | Sidebar.tsx + AppContext | 低 |
| 5. 重命名 Automation → Workspace,移入 Routines/Profiles/Hooks | Sidebar.tsx | 低 |
| 6. 删除 OPC 折叠区,OPC 内容移入 Tasks > Agents | Sidebar.tsx + Tasks.tsx | 中 |

### P2 — 长期演进(1-3 个月,产品形态升级)

| 动作 | 触发条件 |
|------|---------|
| 1. 评估是否走向方案 B(聊天优先) | 消费者留存 > 开发者留存 2 倍 |
| 2. Chat 内嵌任务卡片(不离开 Chat 管任务) | 方案 A 用户反馈"切换太频繁" |
| 3. 自定义导航(用户可以 pin 常用页到 Primary) | 高级用户抱怨 5 项不够 |
| 4. 上下文感知导航(在 Chat 里时,显示相关 Extensions) | AI 能力支持 |
| 5. 删除 OPC(毕业或废弃) | 实验数据评估后决定 |

---

## 八、风险与权衡

### 8.1 方案 A 的风险

**风险 1:现有开发者用户流失**
- 症状:Quick Fix / Editor / Perf 被隐藏后,开发者找不到
- 缓解:Developer Mode 一键开启;首次启动检测用户身份(配置过 LSP / 有 `.shannon/`)自动开启
- 严重度:中

**风险 2:Goals + Triage 合并后,高级用户失去快捷入口**
- 症状:原本 ⌘2 进 Goals 看进度,现在要切 tab
- 缓解:Today 默认 tab 记忆用户上次选择;⌘2 = Today(进度),⇧⌘2 = Today(待审)
- 严重度:低

**风险 3:Tasks 页面承载 4 个视图,变得臃肿**
- 症状:`Tasks.tsx` 已经有 46 行 imports(`Tasks.tsx:26-44`),再加 Mission Control / OPC 会爆炸
- 缓解:每个视图拆成独立组件(`TasksListView` / `TasksCalendarView` / `TasksKanbanView` / `TasksAgentsView`),
  页面本身只做路由分发
- 严重度:中(技术债,可管控)

**风险 4:命名"Today"过度承诺**
- 症状:用户期望"Today 里只有今天的事",但实际可能包含历史任务
- 缓解:副标题明确("Active + Needs Review"),或改名 "Inbox" / "Dashboard"
- 严重度:低

### 8.2 方案 B 的风险

**风险 1:工程量爆炸**
- 症状:把 Tasks / Goals / OPC 全部内嵌到 Chat,相当于重写产品
- 缓解:不做,除非有明确数据支持
- 严重度:高

**风险 2:失去工具型产品的"力量感"**
- 症状:消费者觉得"这只是个聊天框",对比 ChatGPT 没差异化
- 缓解:保留方案 A 的 Extensions 作为差异化
- 严重度:高

### 8.3 共同风险

**风险:命名改动破坏用户记忆**
- 任何重命名都会让现有文档/教程失效
- 缓解:旧名保留 6 个月作为搜索别名(CommandPalette 搜 "Mission Control" 仍能跳到 Tasks > Kanban)

**风险:Command Palette 与 Sidebar 不同步**
- 已存在的问题(P7),重构时必须同时改
- 缓解:原则 5(单一可信来源)— 抽取 `navigation.ts` 配置

### 8.4 权衡决策表

| 决策点 | 选项 A | 选项 B | 推荐 | 理由 |
|--------|--------|--------|------|------|
| 主导航数量 | 5 项 | 7 项(保留 Triage 独立) | **5** | 越少越好,tab 能解决 |
| Developer Mode 默认 | off | on(检测开发者) | **off + 检测自动开启** | 默认极简,但给开发者无缝路径 |
| OPC 处理 | 删除 | 并入 Tasks | **并入 Tasks > Agents tab** | 保留实验,但降权 |
| 方案 A vs B | 渐进 | 激进 | **A 优先,B 长期** | 风险可控 |
| 命名 Today vs Inbox | Today | Inbox | **Today**(对消费者更亲切) | "Inbox" 太工作向 |
| Settings 子项 | 3 项 | 5 项(保留现状) | **4 项**(合并 Theme→Appearance) | 折中 |

---

## 九、附录

### 9.1 实施前的快速胜利(Quick Wins)

**今天就可以改的 5 件事**(零风险,纯文案/视觉):

1. `Sidebar.tsx:102` 把 "AI Code Assistant" 改成 "AI Agent"(配合转型)
2. `Sidebar.tsx:233-237` OPC 折叠区**默认折叠**(`useState(false)`)
3. `Header.tsx:10-18` 补全 TITLE_MAP(8 个缺失项)
4. `CommandPalette.tsx:23-32` 补全 Pages 列表(13 个缺失项)
5. `Sidebar.tsx:253-290` Quick Fix / Editor / Perf 加 "Dev" badge,视觉降级(灰色 + 12px 字号)

**预期效果**:用户感知"导航清爽了一些",即使没有大重构,也能缓解痛点 30-40%。

### 9.2 度量指标(上线后跟踪)

| 指标 | 当前(估) | 目标 | 衡量方式 |
|------|----------|------|---------|
| 主导航项数 | 11 | 5 | 静态 |
| 默认可见入口 | 15 | 5 | 静态 |
| 新用户首次成功发送消息时间 | ? | < 60s | 埋点 |
| 用户访问 Developer 功能比例 | ? | < 20% | 埋点 |
| Command Palette 使用率 | ? | > 30% | 埋点 |
| 用户满意度(NPS) | ? | +10 提升 | 调研 |

### 9.3 开放问题(需产品确认)

1. **OPC 的最终命运**:毕业成正式功能 / 并入 Tasks / 完全删除?
2. **Hook Events 是否对外**:消费者永远不会用,是否考虑完全移除 UI(仅保留后端)?
3. **Profiles 是否进 Settings**:权限配置本质上是一种 Settings,是否应该归入 `Settings > Permissions`?
4. **Activity 是新页面还是合并到 Footer**:Footer 已经显示 token 数(`Layout.tsx:67-89`),
   是否值得单独开一个 Activity 页?
5. **方案 A 与方案 B 的 A/B 测试**:是否有资源做?

---

## 十、结论

Shannon Desktop 的导航问题**不是"项太多",而是"架构错了"**。
当前以功能为中心的 IA 反映了产品从开发者工具生长的痕迹,每个新功能都加一个 Sidebar 入口,
没有人在加之前问"这应该放哪一级"。

**推荐路径**:
1. **立即执行 第九节 Quick Wins**(1 天,零风险)
2. **2-3 周内完成方案 A**(任务中心化 IA,路由合并,Developer Mode)
3. **观察 1-2 个月用户数据**,决定是否走向方案 B

**核心信念**:
> 好的导航不是"把所有功能都列出来",而是"让用户在 5 秒内知道下一步该点什么"。
> Shannon 现在的导航需要用户花 30 秒理解 22 个名词 — 这就是失败。

---

**文档结束**。
