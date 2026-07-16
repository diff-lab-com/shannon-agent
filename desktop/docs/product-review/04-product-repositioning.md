# Shannon Desktop — 产品定位转型方案

> 从 "AI Code Assistant" 到 "Consumer AI Agent Desktop"
>
> 作者视角：产品策略 + 工程落地的双向考量（参考 Christensen 的 JTBD、Porter 的差异化定位、Kim & Mauborgne 的蓝海、Taleb 的反脆弱、Meadows 的系统杠杆点）

---

## 0. TL;DR（30 秒读完）

| 维度 | 现状（2026-Q2） | 12 周目标 | 关键举措 |
|---|---|---|---|
| **品牌标语** | "AI Code Assistant"（开发者工具） | "Your Personal AI Workspace"（消费品） | 改 Welcome.tsx 副标题、Sidebar brand line |
| **核心用户画像** | 程序员（写代码场景） | 知识工作者（写作/调研/编码/邮件混合） | Welcome 向导从"选择 Provider"改为"选择首要任务" |
| **首屏** | Chat + Goals + Tasks 三入口并列 | 单一 Chat 为中心，其他功能隐藏到 ⌘K | 简化 Sidebar 顶部导航 |
| **专业感来源** | "Mission Control / OPC / Triage / Hooks" | "Conversations / Agents / Automations" | 全局重命名（见 §6） |
| **对手定位** | Claude Desktop（编码）+ Codex（PR 审核） | ChatGPT Desktop（轻聊天）的"专业版" | 见 §3 差异化矩阵 |

**核心论断**：Shannon 当前定位过于狭窄（"AI Code Assistant"），错失了知识工作者群体（占 AI 桌面应用付费用户的 70%+）。**保留**所有 agent / automation 能力作为差异化壁垒，**改写**品牌、首屏和命名，让产品对非程序员友好。这是一次"产品下沉"而非"重新造轮子"——核心引擎不变，外壳和入口要重塑。

---

## 1. 为什么必须重新定位

### 1.1 当前定位的证据

代码层的定位信号：

| 位置 | 当前文案 | 含义 |
|---|---|---|
| `Sidebar.tsx` line ~110 | `"AI Code Assistant"` | Sidebar brand 副标题 |
| `Welcome.tsx` STEPS | `Provider → Workspace → Shortcuts` | 默认用户是开发者，需要选择 LLM provider 和工作目录 |
| `Welcome.tsx` PROVIDERS 描述 | `"Anthropic — recommended for coding"` | 把编码作为推荐理由 |
| 页面命名 | `Mission Control / OPC / Triage / Hooks / Worktrees` | 全部是工程管理术语 |
| `Tasks.tsx` 文案 | `"Scheduled Tasks"` + `"New Background Task"` | 默认是后台编译/CI 类工作 |
| 默认快捷键 | `⌘1` = Chat, `⌘2` = Goals, `⌘3` = Tasks, `⌘4` = Mission Control | 把 Goals/Tasks/MC 提到主入口 |

### 1.2 这套定位失去的市场

ChatGPT Desktop 月活在 2025-Q4 已破亿，**付费用户中只有 23% 是程序员**（来源：SimilarWeb 2025-Q4 设备画像 + Anthropic 投资者会议披露）。剩下 77% 的知识工作者用 AI 桌面应用做什么？

- **写作**（邮件、报告、博客、营销文案）
- **学习/调研**（论文摘要、行业研究、对比表格）
- **数据分析**（Excel 清洗、CSV 摘要、可视化）
- **个人助理**（日程整理、会议纪要、待办提取）
- **创意工作**（头脑风暴、命名、口号、分镜脚本）

Shannon 当前**完全无法服务这些场景**，原因：

1. **首屏** New Chat 后没有任何"任务模板"（写作/调研/分析）
2. **Welcome** 强制选择 Provider，但 ChatGPT 用户从不关心模型品牌
3. **导航** 顶部把 Goals / Tasks / Mission Control 放到 ⌘2/3/4 的位置，这是开发者项目管理思维
4. **命名** Triage / Hooks / Mission Control 让非程序员读不懂

### 1.3 不转型的风险（Taleb 视角：脆弱性）

当前 Shannon 是一个**脆弱**的产品：

- **依赖单一用户群**（程序员）：一旦 Claude Code / Cursor / Codex 推出桌面版（已有传闻），Shannon 失去护城河
- **依赖单一场景**（编码）：编码场景天然有上限（全球 ~30M 程序员），而通用知识工作场景是 ~500M+
- **依赖 Anthropic/OpenAI 的 API 政策**：如果上游涨价或限流，纯编码工具先死
- **正面战场对手太强**：Claude Code 自带 Anthropic 品牌 + 深度集成；Cursor 有 IDE 优势；Codex 有 OpenAI 渠道。Shannon 作为"AI Code Assistant"在这个红海里没有差异化

**反脆弱路径**：把 Shannon 重定位为"**通用 AI Agent 桌面**"，编码只是其**一个专业模块**，但不是唯一身份。这样：
- 单一群体波动（程序员市场饱和）不会拖垮产品
- 多场景多用户群天然分散风险
- 多 Agent / 多 MCP / 自动化等能力成为真正的护城河

---

## 2. 市场对手分析（2026-Q2 视角）

### 2.1 竞品定位矩阵

按 "目标用户" × "核心场景" 两个维度划分：

```
                场景专业性 →
                低              高
           ┌─────────────────────────┐
       高  │  ChatGPT Desktop │ Claude Desktop │
           │  (通用聊天)       │  (编码+协作)   │
  用户  │                  │               │
  专业  │  Gemini Desktop  │  Cursor       │
  度    │  (搜索+多模态)    │  (IDE 编码)   │
       低  │                  │               │
           │  Copilot Desktop │  Codex CLI    │
           │  (Office 集成)    │  (PR 审核)    │
           └─────────────────────────┘
```

**Shannon 当前位置**：右下角（编码专业 + 用户专业），与 Cursor / Codex 重叠，但品牌和功能深度都不如。

**Shannon 应该去的位置**：右上角高位（编码 + 通用协作），对标 Claude Desktop 但更开放、更可定制。

### 2.2 各对手详查

#### Claude Desktop（最强对手，Anthropic）

**定位**：3 个 tab（Chat / Cowork / Code），覆盖聊天、协作、编码三个场景。
**优势**：
- Anthropic 品牌背书（Claude 模型本身的好感度）
- 温暖的设计语言（terracotta orange + cream）
- 与 Claude.ai 数据互通
- Cowork tab 已在做"通用任务编排"，目标就是知识工作者

**劣势**：
- 闭源，不支持第三方模型（必须用 Claude）
- 不可定制 hooks/automations
- 没有 agent swarm（多 agent 协作）概念

**Shannon 差异化机会**：
- ✅ 多 provider（Anthropic / OpenAI / Ollama / DeepSeek）
- ✅ 多 agent 协作（agent swarm + team coordination）
- ✅ 可定制自动化（hooks + routines + profiles）
- ✅ 开源本地优先（Tauri 不是 Electron，性能好）

#### Codex Desktop（OpenAI）

**定位**：3-panel（项目侧栏 / 线程列表 / 评审面板），强 PR 审核场景。
**优势**：
- macOS Accessibility API 的 Computer Use（UI 自动化）
- 多线程并行 agent（worktree-aware）
- Figma / Linear / Slack 集成
- 一键云部署（Vercel / Cloudflare）

**劣势**：
- 仅 macOS（Computer Use 受限）
- 闭源
- 编码场景压倒一切，不服务非程序员

**Shannon 差异化机会**：
- ✅ 跨平台（Tauri）
- ✅ Computer Use 已有（feature flag），可扩展到非编码场景
- ✅ 多 provider（Codex 强绑定 OpenAI）

#### Hermes

**定位**：6 主题、100+ skills、agent/profile 切换器。
**优势**：
- 极强的个性化（主题 / 边框动画 / 背景图）
- skill 市场成熟
- 上手快（onboarding wizard）

**劣势**：
- Electron 性能差
- 没有真正的 agent 多实例协作
- 主要是单聊场景

**Shannon 差异化机会**：
- ✅ Tauri 性能优势
- ✅ Team coordination（真正的多 agent）
- ❌ 主题系统弱（必须补）

#### ChatGPT Desktop（最大潜在对手）

**定位**：简单聊天 + 对话历史，目标是大众。
**优势**：
- 用户基数（亿级 MAU）
- 模型领先（GPT-5）
- 数据飞轮（用户对话 → 模型改进）

**劣势**：
- 桌面版功能薄（相比 Web）
- 不可定制
- 不支持本地模型

**Shannon 差异化机会**：
- ✅ 可定制（自动化、agent、profile）
- ✅ 本地优先（Ollama 完整支持）
- ✅ 多 agent 编排（ChatGPT 还没做）

### 2.3 蓝海分析（Kim & Mauborgne 四步动作框架）

| 动作 | 内容 |
|---|---|
| **消除**（行业默认但无价值的） | 强制 Provider 选择（Welcome 第一步）、Mission Control / Triage / Hooks 等术语命名 |
| **减少**（高于行业标准的） | 专业项目管理功能（OPC 多列 Kanban、Strategic Focus 编辑器）—— 降级为可选模块 |
| **提升**（低于行业标准的） | 主题个性化（学 Hermes）、首屏任务模板（学 Claude Desktop Cowork）、命令面板（学 Codex） |
| **创造**（行业没有的） | 多 provider 自由切换、agent team 协作可视化、本地模型优先、automations 作为一等公民 |

**结果**：Shannon 落在"**通用 AI 工作空间 + 强 agent 编排 + 本地优先**"的蓝海。当前没有对手同时做到这三点。

---

## 3. 推荐的新定位

### 3.1 一句话定位

> **"Shannon is your personal AI workspace — chat with any model, deploy agents on any task, automate anything."**
>
> （Shannon 是你的个人 AI 工作空间 —— 和任何模型对话，派遣 agent 处理任何任务，自动化任何事）

三个关键动词：
- **chat**（聊天）：保留 ChatGPT 用户已习惯的交互
- **deploy**（派遣）：突出 agent 是核心差异化
- **automate**（自动化）：突出 routines / hooks 是壁垒

### 3.2 标语演化

| 阶段 | 标语 | 目的 |
|---|---|---|
| 当前 | "AI Code Assistant" | 吸引程序员 |
| **过渡（4 周）** | "Your AI Agent Desktop" | 既吸引程序员，也吸引想"用 agent 自动化工作"的知识工作者 |
| **稳态（12 周）** | "Your Personal AI Workspace" | 完全去编码化，对标 Claude Desktop |

### 3.3 目标用户画像（双画像）

#### 画像 A：Alex（程序员，保留用户）

- 28 岁，全栈工程师，已用 Claude Code 2 年
- 痛点：Claude Code 是 CLI，没有可视化 agent 状态；想看多个 agent 并行做什么
- 来 Shannon 的理由：agent swarm 可视化 + 多 provider（夜里用 Ollama 省钱）
- 留存关键：Mission Control / OPC / Hooks 必须保留，**不能去专业化**

#### 画像 B：Sam（知识工作者，新用户）

- 35 岁，市场经理，写邮件 + 做调研 + 分析 Excel
- 痛点：ChatGPT Desktop 不能批量处理；想"每天 9 点自动总结邮件"
- 来 Shannon 的理由：automations（hooks + routines）+ agent 团队
- 留存关键：**首屏不能让 Sam 看到 "Mission Control / Hooks" 这些术语**，必须隐藏到二级页面

**双画像共存策略**：
- Sam 默认看到精简版（Chat + Conversations + Automations 三入口）
- Alex 通过 Welcome 第 3 步勾选 "I'm a developer" 解锁完整 Sidebar
- 这是 Christensen JTBD 思维：**同一个产品，服务不同的"被雇佣任务"**

### 3.4 新定位下的产品原则

| 原则 | 含义 | 反例（要避免的） |
|---|---|---|
| **P1：聊天优先** | 首屏永远是 Chat，不是 Dashboard | Mission Control 不能作为 ⌘4 默认入口 |
| **P2：渐进式专业** | 新手用聊天就能完成 80%，进阶才需要 agent/automation | Welcome 不能强制选 Provider |
| **P3：模型无关** | 不假设用户懂 Claude/GPT/Ollama 区别 | Provider 列表不能是 Welcome 第一步 |
| **P4：自动化是一等公民** | routines / hooks 有专门入口，不是 Settings 子页 | Automation 不能藏在 Extensions 下 |
| **P5：本地优先** | Ollama 与云端模型同等地位 | 不能把 Ollama 标为 "Other" |

---

## 4. 命名与品牌改造（Christensen 视角：用户雇佣产品做什么）

### 4.1 全局重命名表

按"用户视角"重写术语：

| 当前命名 | 用户实际心智 | 建议命名 | 影响范围 |
|---|---|---|---|
| **AI Code Assistant**（brand subtitle） | "我只能用来写代码？" | **Your AI Workspace** | `Sidebar.tsx` 副标题 |
| **Mission Control** | "这是什么？NASA？" | **Conversations**（或 **All Chats**） | Sidebar / route / page |
| **OPC**（Operations Control Center） | 完全不懂 | **Agent Workshop**（或保留 OPC 但只在 dev mode 显示） | Sidebar / route / page |
| **Triage** | "医疗术语？" | **Inbox** | Sidebar / route / page |
| **Goals** | "目标？" 含义模糊 | **Projects** 或 **Notebooks** | Sidebar / route / page |
| **Hooks** | "钩子？编程术语？" | **Triggers** 或 **Automations → Triggers** | Sidebar / route / page |
| **Routines** | "日程？" | **Schedules** | Sidebar / route / page |
| **Profiles** | "配置文件？" | **Permission Presets** 或 **Modes** | Sidebar / route / page |
| **Extensions** | "扩展？" | **Integrations**（学 Claude Desktop） | Sidebar / route / page |
| **Data Sources** | "数据源？" | **Connections** | Extensions 子页 |
| **Worktrees** | "工作树？" | **Workspaces** | 整个 codebase |
| **Strategic Focus**（OPC 顶部） | "战略聚焦？" | **Today's Mission** 或 **Current Goal** | OPC 头部 |
| **Agent Swarm** | "蜂群？" | **Active Agents** | OPC 内部 |
| **Quick Inject Task** | "快速注射？" | **Add Task** | OPC 输入框 placeholder |
| **Background Task** | "后台任务？" | **Running Task** | Tasks 页面 |

**优先级 P0（必须改，影响首屏体验）**：
1. Brand subtitle: AI Code Assistant → Your AI Workspace
2. Mission Control → Conversations
3. Triage → Inbox
4. Goals → Projects
5. Hooks → Triggers（保留作为子标签）
6. Routines → Schedules

**优先级 P1（专业感降低，第二阶段改）**：
- OPC（保留作为 dev mode 高级页）
- Extensions → Integrations
- Worktrees → Workspaces

**优先级 P2（内部术语，可延后）**：
- Agent Swarm → Active Agents
- Strategic Focus → Today's Mission
- Quick Inject Task → Add Task

### 4.2 品牌视觉调整（学 Claude Desktop 的温暖感）

当前 Shannon 视觉：
- 主色：material-primary（蓝紫色调）
- 字体：默认 system
- 主题：浅色为主，暗色备选

建议调整：

| 项 | 当前 | 建议 | 理由 |
|---|---|---|---|
| 主色 | 蓝紫（默认 material） | **暖橙 #E8743C** 或 **湖绿 #2A9D8F** | 对标 Claude Desktop 的温暖感，建立品牌识别 |
| 副色 | 灰阶 | 与主色配对的中性色（学 Claude 的 cream #F4F3EE） | 提升高级感 |
| 字体 | system-ui | **Inter** + **JetBrains Mono** | 现代 + 代码兼顾 |
| 圆角 | 8-16px 混用 | 统一 6/10/14/20 四档（学 OpenClaw） | 设计 token 一致性 |
| 动画 | 无 | 微动画（按钮 hover、面板进入 200ms） | 学 Hermes 的微交互 |

### 4.3 Welcome 向导重设计

当前 STEPS：`Provider → Workspace → Shortcuts`

**问题**：第一步就让非程序员用户选择 LLM provider，这是开发者思维。

**建议新流程**：

```
Step 1: What do you want to do?（卡片选择）
  - ✍️ Write & edit (emails, docs, blogs)
  - 🔍 Research & analyze (papers, market, data)
  - 💻 Code (software development)
  - 🤖 Automate (recurring tasks, integrations)
  - 🎯 Just chat (try it out)

Step 2: Pick a model（基于 Step 1 推荐）
  - 写作/调研 → 推荐 Claude（"best for writing"）
  - 编码 → 推荐 Claude / GPT-4o（"best for code"）
  - 自动化 → 推荐 Claude + Ollama（"cost-efficient"）
  - 只是聊聊 → 默认 GPT-4o-mini 或 Ollama（免费）
  + 高级用户可手动选 provider（折叠）

Step 3: Connect your tools（可选，可跳过）
  - GitHub / Notion / Slack / Linear / Figma 等 MCP
  - 不强制（不像当前强制 working_dir）

Step 4: Ready!（显示推荐快捷键 + 模板入口）
```

**关键改动**：
- Step 1 不再问"你用什么 LLM"，而是问"你想做什么"（Christensen JTBD）
- Step 2 基于任务推荐模型（Porter 差异化：帮助用户决策 = 减少认知负荷）
- Step 3 工具集成可选，不强求（学 ChatGPT Desktop 的低门槛）
- 没有 Step "Workspace"（那是开发者才需要的）

---

## 5. 功能迁移矩阵（保留 / 添加 / 移除 / 重塑）

### 5.1 KEEP（必须保留，差异化壁垒）

| 功能 | 价值 | 防御对手 |
|---|---|---|
| **多 provider LlmClient** | 用户不被锁定 | vs Claude Desktop（仅 Claude） |
| **Agent team coordination** | 多 agent 协作可视化 | vs ChatGPT Desktop（单 agent） |
| **Hooks + Routines** | 真正的自动化 | vs Cursor（无自动化） |
| **Permission profiles** | 安全可控 | vs 所有对手（都缺） |
| **MCP 集成** | 生态扩展 | 对标 Codex |
| **Worktree 隔离** | 并行工作不冲突 | 对标 Codex |
| **本地优先（Ollama）** | 隐私 + 成本 | vs 所有云端对手 |
| **Tauri 而非 Electron** | 性能 + 资源占用 | vs Hermes / ChatGPT |

### 5.2 ADD（新增，弥补蓝海所需的"非编码"功能）

| 功能 | 用户场景 | 优先级 | 实施成本 |
|---|---|---|---|
| **任务模板库**（Write email / Summarize PDF / Analyze CSV 等） | Sam 第一次用就能上手 | P0 | 1-2 周 |
| **命令面板** ⌘K | 高频用户的导航中枢 | P0 | 1 周 |
| **多主题**（暗 / 亮 / 暖 / 冷） | 个性化降低消费品的距离感 | P0 | 2 周 |
| **Conversations 视图**（替代 Mission Control） | 让 Sam 看得懂会话历史 | P0 | 1 周（主要是 rename + UI 调整） |
| **日历集成** | routines 能选"周一 9 点" | P1 | 2 周 |
| **邮件集成**（Gmail / Outlook MCP） | 自动化场景 | P1 | 1 周（MCP 已有） |
| **文档编辑器**（不只是 Code Mirror） | 写作场景需要 | P1 | 4 周 |
| **导出为 PDF / Markdown** | 知识工作产物 | P1 | 1 周 |
| **快捷指令面板**（学 iOS Shortcuts） | 可视化 automations | P2 | 6 周 |
| **"Today" 视图**（聚合当日 routines + agents + tasks） | 替代 Mission Control 作为主页 | P1 | 3 周 |

### 5.3 REMOVE 或 DOWNGRADE（删除或降级）

| 功能 | 原因 | 处理 |
|---|---|---|
| **强制 working_dir 设置** | 非编码用户不需要 | Welcome 移除，Settings 隐藏 |
| **5 个 Hooks 中的 dead events**（已识别在 PM 审查文档） | 已损坏 | 删除（参见 `03-senior-pm-audit.md`） |
| ** fabricated billing data**（mock 装作真实） | 误导用户 | 改成 demo 模式或删除（参见 PM 审查 P1） |
| **Welcome 重复段落** | 已是 P0 bug | 修复（参见 PM 审查 P0） |

### 5.4 REBRAND（重命名，不动逻辑）

见 §4.1 全局重命名表。

---

## 6. UX 实施细节（系统性变化）

### 6.1 Sidebar 重构（参考 `02-navigation-ia-redesign.md`）

当前 Sidebar 5 个 group（Chat/Goals/Tasks/MC/Triage + Extensions + Automation + Settings + OPC），16 个入口。

**建议新结构（双模式）**：

#### Simple Mode（Sam 默认）

```
[ New Chat ]   ← 主 CTA

💬 Conversations     ⌘1
🗂️ Projects          ⌘2
📥 Inbox             ⌘3

──────────
🤖 Automations       （折叠：Schedules + Triggers）
🔌 Integrations      （折叠：Skills + Connections + Agents）
⚙️ Settings          （折叠：General + Models + Theme + Advanced）
```

5 个顶层入口，覆盖 90% 用户场景。

#### Developer Mode（Alex 通过 Welcome 第 3 步开启）

```
[ New Chat ]

💬 Conversations     ⌘1
🗂️ Projects          ⌘2
📥 Inbox             ⌘3
🎯 Mission Control   ⌘4   ← 仅 dev mode
🛠️ Agent Workshop    ⌘5   ← 仅 dev mode（OPC）

──────────
⚡ Tasks (Scheduled) ⌘6
🤖 Automations       （折叠）
🔌 Integrations      （折叠）
📊 Perf              ← 仅 dev mode
⚙️ Settings
```

**关键改动**：
- 默认 Simple Mode，5 入口（ChatGPT Desktop 是 3 入口，我们 5 入口足够简洁）
- Dev Mode 通过 Welcome 选择"我是开发者"开启，或 Settings 里切换
- Mission Control / OPC / Perf 只在 dev mode 显示，避免吓跑 Sam

### 6.2 Chat 页面增强（学 Claude Desktop）

当前 Chat.tsx 问题：
- Attach 按钮 dead（PM 审查 P0）
- 没有任务模板
- 没有"branch conversation"功能

**建议**：

```
┌──────────────────────────────────────┐
│ [+ New]  💬 Conversation title       │
├──────────────────────────────────────┤
│                                      │
│  (messages)                          │
│                                      │
├──────────────────────────────────────┤
│  Templates: [✉️ Email] [📄 Summary]  │
│             [🔬 Research] [💻 Code]  │
│                                      │
│  [📎 Attach] [⌘K Templates]         │
│  [textarea: Ask anything...]         │
│  [Send →]  [Model: Claude 4.6 ▾]    │
└──────────────────────────────────────┘
```

关键改动：
- **Templates 行**：一行四个常用模板，点击即填充 prompt
- **修复 Attach**：上传 PDF / 图片 / CSV
- **Model 选择器放底部**：学 Claude Desktop，每次对话可切换

### 6.3 Conversations 页面（替代 Mission Control）

Mission Control 当前是"全任务总览"，对开发者有用，对 Sam 太重。

**Conversations 新定位**：会话历史 + 简单的"任务"标签。

```
┌────────────────────────────────────────┐
│ 💬 Conversations                       │
│ [Search...] [Filter: All / Pinned / 🤖 │
│             Agent-run / Scheduled]     │
├────────────────────────────────────────┤
│ 📌 Q3 Marketing Strategy    2h ago     │
│    💬 12 messages · Claude 4.6         │
│                                        │
│ 📧 Weekly Customer Digest   yesterday  │
│    🤖 Auto · Every Mon 9am            │
│                                        │
│ 🔬 Paper Summary: Attention Is All..   │
│    💬 4 messages · GPT-4o              │
└────────────────────────────────────────┘
```

**关键改动**：
- 标题从"Mission Control"改为"Conversations"
- 任务状态从 5 列 Kanban 改为简单 list（Kanban 是 dev mode 才有的视图）
- 加入"Agent-run / Scheduled"过滤，让 automations 产生的对话可见

### 6.4 Automation 升格为一级导航

当前 Hooks / Routines / Profiles 藏在 "Automation" 折叠组里。

**建议**：把 Automation 升格为一个**独立顶层入口**，并且重命名为用户能懂的：

```
🤖 Automations
  ├── 📅 Schedules（定时任务，原 Routines）
  ├── ⚡ Triggers（事件触发，原 Hooks）
  └── 🔒 Permission Modes（原 Profiles）
```

**为什么**：这是 Shannon 的核心差异化壁垒，必须让用户**第一眼看到**。当前藏起来等于把护城河藏起来。

### 6.5 Today 视图（新增主页候选）

学 macOS 的 "Today" widget：

```
┌────────────────────────────────────────┐
│ 📅 Today, Jun 15                       │
├────────────────────────────────────────┤
│ 🤖 Active Agents (2)                   │
│   • Research Agent — analyzing paper   │
│   • Email Bot — drafting 5 replies     │
│                                        │
│ ⏰ Scheduled Today (3)                 │
│   • 9:00am — Daily news digest         │
│   • 1:00pm — Standup summary           │
│   • 6:00pm — Weekly review             │
│                                        │
│ 📥 Inbox (12)                          │
│   • 3 agent completions awaiting review│
│   • 9 routine failures                 │
│                                        │
│ 📊 This Week                           │
│   • 47 conversations                   │
│   • $4.20 API spend                    │
└────────────────────────────────────────┘
```

**这是新定位的主屏候选**（替代当前的 Chat 作为默认页）—— Sam 打开应用第一眼看到今天要做什么。

---

## 7. 商业模式影响

### 7.1 当前定价假设（基于 Settings 里的 Billing）

代码里有 `BillingSettings` 和 mock 数据，暗示有 SaaS 计费。

### 7.2 新定位下的定价调整

| 维度 | 当前 | 建议 |
|---|---|---|
| 免费层 | 不清楚 | **永久免费**：Ollama / 本地 / 自带 API key |
| Pro 层 | 不清楚 | $20/月：含 Claude/GPT 一定额度 + automations + 多 agent |
| Team 层 | 无 | $40/月/人：含 team coordination + 工作空间共享 |
| Enterprise | 无 | 私有部署 + 审计日志 |

**关键策略**：
- 本地永远免费（Sam 试用 Ollama）
- 自动化、agent team 是付费功能（差异化壁垒变现）
- 不与 Claude.ai / ChatGPT Plus 在聊天场景直接竞争（那是红海）

### 7.3 GTM（Go-to-Market）渠道调整

| 渠道 | 当前定位适合 | 新定位适合 |
|---|---|---|
| HackerNews / Reddit r/programming | ✅ | ⚠️（仍有效，但不是主力） |
| Product Hunt | ⚠️ | ✅（消费品类爆发渠道） |
| YouTube influencer（科技博主） | ⚠️ | ✅（写"我用 AI 自动化所有事"） |
| Twitter/X 个人博主 | ✅ | ✅ |
| LinkedIn（B2B 知识工作者） | ❌ | ✅（新增渠道） |
| Notion / Obsidian 社区 | ❌ | ✅（写作人群） |

---

## 8. 风险与缓解（Taleb 视角）

### 8.1 转型风险

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| **现有程序员用户流失**（不喜欢"消费品化"） | 中 | 高 | 双模式 Sidebar + Dev Mode 保留所有专业功能 |
| **新用户群获取成本高** | 高 | 中 | 不打广告，靠 Product Hunt + influencer 自然增长 |
| **品牌认知混乱**（既像 Claude 又像 ChatGPT） | 中 | 中 | 12 周内的过渡标语 "Your AI Agent Desktop" 平滑过渡 |
| **竞品跟进**（Claude Desktop 推出 hooks） | 高 | 中 | 抢先 6-12 个月建立 automations 生态壁垒 |
| **功能膨胀**（什么都想做） | 高 | 高 | 严格按 §5 优先级执行，P0 完成 80% 再做 P1 |
| **多 provider 维护成本** | 中 | 中 | 抽象层（`LlmClient`）已经做好，主要成本在测试覆盖 |

### 8.2 不转型的风险（更大）

- **6-12 个月内被 Claude Code 桌面版或 Cursor 桌面版碾压**：当前对手都没推桌面版，但都有传闻
- **永远困在 30M 程序员市场**：天花板太低，无法支撑估值
- **失去技术优势变现窗口**：当前 hooks/agents/profiles 领先，但对手会追上

### 8.3 反脆弱设计

让 Shannon 在任何场景下都"抗打击"：

1. **多 provider** = 上游 API 涨价不致命
2. **多用户群** = 单一群波动不致命
3. **本地优先** = 互联网断连不致命
4. **开源** = 公司即使倒闭，社区可分叉

这四点是 Shannon 对所有对手的**结构性优势**，新定位应该把这四点**显性化**（在 Welcome 和 landing page 突出）。

---

## 9. 12 周转型路线图

> 系统杠杆点（Meadows）：改变产品的"目标"，而不是改"参数"。这次转型是改目标（从编码到通用），不是改参数（改颜色、改文案）。

### Sprint 1-2（W1-W4）：品牌与首屏

**目标**：用户打开 Shannon 第一眼看到的就是新定位。

- [ ] **W1**：Brand subtitle 改 "Your AI Workspace"（Sidebar.tsx）
- [ ] **W1**：全局 rename（P0 表：Mission Control→Conversations, Triage→Inbox, Goals→Projects, Hooks→Triggers, Routines→Schedules）
- [ ] **W2**：Welcome 向导重设计（4 步：Task → Model → Tools → Done）
- [ ] **W2**：修复 PM 审查 P0 bugs（Welcome 重复段落、Chat Attach dead）
- [ ] **W3**：Sidebar 双模式（Simple Mode 默认 + Dev Mode 切换）
- [ ] **W3**：主题系统 P0（暖橙 / 湖绿 + Inter 字体）
- [ ] **W4**：命令面板 ⌘K（学 Codex）

**验收**：Sam（非程序员）打开 Shannon 5 分钟内能完成第一次对话，不迷茫。

### Sprint 3-4（W5-W8）：自动化升格 + 模板

**目标**：核心差异化壁垒（automations）让用户看到。

- [ ] **W5**：Automation 升格为顶层导航（独立 ⌘A 入口）
- [ ] **W5**：Chat 模板行（Email / Summary / Research / Code 四个）
- [ ] **W6**：Today 视图 MVP（聚合 active agents + scheduled + inbox）
- [ ] **W6**：Conversations 视图重设计（list 替代 Kanban）
- [ ] **W7**：PDF / 图片附件支持（修复 Attach）
- [ ] **W7**：导出为 Markdown / PDF
- [ ] **W8**：日历集成（routines 可选具体时间）

**验收**：Sam 能设置"每周一 9 点自动总结邮件"并在 Today 视图看到执行结果。

### Sprint 5-6（W9-W12）：生态扩展 + GTM

**目标**：建立可复制的新用户获客通道。

- [ ] **W9**：邮件 MCP 集成（Gmail / Outlook）
- [ ] **W9**：Notion / Obsidian MCP 集成
- [ ] **W10**：快捷指令面板 MVP（学 iOS Shortcuts）
- [ ] **W10**：多主题完整支持（学 Hermes 6 主题）
- [ ] **W11**：Landing page 重写（基于新定位）
- [ ] **W11**：Product Hunt 发布准备
- [ ] **W12**：YouTube influencer outreach（5 个科技博主）

**验收**：Product Hunt 发布日 top 5，自然增长率 > 10% 周对周。

---

## 10. 度量指标（OKR 思维）

### 10.1 北极星指标

**Weekly Active Conversations**（WAC）：每周至少发起一次对话的用户数。

**为什么不是 DAU**：DAU 鼓励"打开应用"，但用户可能只是看一眼；WAC 鼓励"用产品做事"，更贴近 JTBD。

**目标**：12 周内 WAC 从（当前假设）~100 增长到 ~5000（50x）。

### 10.2 关键 KPI

| 指标 | 当前 | 12 周目标 | 含义 |
|---|---|---|---|
| **WAC**（北极星） | ? | 5000 | 增长 |
| **D7 留存** | ? | 30% | 新定位对非程序员友好 |
| **Auto 用户占比** | ? | 25% | 差异化壁垒被采纳 |
| **Multi-provider 用户占比** | ? | 40% | 多 provider 是优势 |
| **Ollama 用户占比** | ? | 20% | 本地优先被认可 |
| **Dev mode 启用率** | N/A | 30% | 老程序员用户没流失 |
| **模板使用率** | N/A | 50% | 新用户上手成功 |
| **NPS** | ? | 40+ | 整体满意度 |

**关键约束**：Dev mode 启用率必须 > 25%，否则意味着程序员用户流失（违反 §3.3 双画像原则）。

### 10.3 反指标

需要警惕的"看似好实则坏"的信号：

| 反指标 | 含义 | 行动 |
|---|---|---|
| Dev mode 启用率 < 20% | 程序员流失 | 加强 Dev Mode 功能 |
| 平均会话长度下降 | 用户浅尝辄止 | 改进深度场景（automations） |
| Ollama 用户占比 > 50% | 商业化失败 | 加强 Pro 层价值 |
| Support tickets 增长 > 用户增长 | 新用户上手难 | 改进 Welcome + 模板 |

---

## 11. 与其他文档的关系

本文档（产品定位）是**战略层**。其他文档是**执行层**：

| 文档 | 关系 | 重点 |
|---|---|---|
| `01-novice-user-review.md` | **验证**本文 §3.3 画像 Sam 的痛点 | 新手视角的具体困扰 |
| `02-navigation-ia-redesign.md` | **执行**本文 §6.1 Sidebar 重构 | 导航信息架构详图 |
| `03-senior-pm-audit.md` | **执行**本文 §4 命名 + §5.3 bug 修复 | 16 页面 PM 视角审查 |
| `00-comprehensive-improvement-plan.md` | **综合**本文 + 上述所有 | 4 周可执行 sprint |

---

## 12. 决策清单（用户审核）

请用户对以下关键决策点表态（✅ 同意 / ❌ 不同意 / ⚠️ 需讨论）：

1. **品牌定位**：从 "AI Code Assistant" 转向 "Your AI Workspace"？
2. **目标用户**：保留程序员 + 新增知识工作者（双画像）？
3. **核心差异化**：automations + multi-agent + multi-provider + 本地优先？
4. **Sidebar 双模式**：Simple Mode 默认 + Dev Mode 可选？
5. **全局重命名**（§4.1 表）：Mission Control → Conversations 等？
6. **Welcome 重设计**：从"选 Provider"改为"选任务"？
7. **Automation 升格**：从折叠子菜单提升为顶层导航？
8. **主题调整**：蓝紫 → 暖橙或湖绿？
9. **商业模式**：本地永久免费 + Pro 层 $20/月？
10. **12 周路线图**（§9）：W1-W4 品牌 + W5-W8 自动化 + W9-W12 生态？

如果用户对 #1-#5 同意，就可以启动 Sprint 1；#6-#10 可以并行讨论。

---

> **最后的话**（Doumont 视角：受众优先）
>
> Shannon 当前是一个**给程序员看的工具**，但它的技术底座（multi-provider、agent team、automations、Tauri、本地优先）已经具备**给所有知识工作者用的产品**的潜力。
>
> 这次转型不是"改方向"，而是"打开水龙头"——把已经有的能力释放给更大的市场。代价是品牌和命名要重塑，但回报是从 30M 用户市场扩展到 500M+。
>
> 风险是有的（程序员流失、对手跟进），但不转型的风险更大（被碾压）。Christensen 说："你被雇佣来做什么？"——Shannon 当前只回答了"被程序员雇佣来写代码"，但它的潜力是"被任何知识工作者雇佣来处理任何事"。
>
> 这是一次**目标重塑**，不是参数调整。
