# Shannon Desktop — 综合改进方案

> 本文档是用户委托的 7 项任务的最终综合输出。
>
> **基础素材**：
> - [01-novice-user-review.md](01-novice-user-review.md)（38KB）—— 新手用户视角审查
> - [02-navigation-ia-redesign.md](02-navigation-ia-redesign.md)（36KB）—— 左侧导航 IA 重设计
> - [03-senior-pm-audit.md](03-senior-pm-audit.md)（22KB）—— 资深 PM 全面审查
> - [04-product-repositioning.md](04-product-repositioning.md)（33KB）—— 产品定位转型方案
> - Mock 数据脚本（已实现）+ Kanban 列统一（已完成）
>
> **本文档定位**：决策即用（decision-ready）。用户读完这一篇就能 ✅/❌ 每个决策点。
>
> 详细论证请看各源文档；本文只列**做什么、什么时候做、谁负责、风险**。

---

## 1. 一页纸执行摘要

### 1.1 现状诊断（一句话）

Shannon Desktop 是一个**技术领先但定位狭窄**的 AI 桌面应用：底层能力（multi-provider、agent team、automations、Tauri 本地优先）已经领先所有对手，但**外壳和入口完全为程序员设计**（"AI Code Assistant"），错失了 70%+ 的知识工作者市场。

### 1.2 改进方向（三件事）

1. **品牌重塑**：从"AI Code Assistant"转向"Your AI Workspace"，目标用户从 30M 程序员扩展到 500M+ 知识工作者
2. **入口简化**：Sidebar 双模式（Simple 默认 / Dev 可选），全局重命名 11 个术语（Mission Control → Conversations 等）
3. **差异化升格**：Automations（Hooks + Routines）从折叠子菜单升格为顶层导航，让用户第一眼看到护城河

### 1.3 时间线

| 周次 | 主题 | 关键交付 |
|---|---|---|
| W1-W2 | **品牌 + Bug 修复** | Brand 改、Welcome 重设计、P0 bugs 修复 |
| W3-W4 | **导航 + 双模式** | Sidebar 双模式、全局重命名、Conversations 视图 |
| W5-W6 | **Automation 升格** | 顶层导航、Today 视图、Chat 模板 |
| W7-W8 | **新手友好** | 任务模板库、附件修复、命令面板 ⌘K |
| W9-W10 | **生态扩展** | 邮件 MCP、Notion/Obsidian MCP、多主题 |
| W11-W12 | **GTM** | Landing page 重写、Product Hunt 发布 |

### 1.4 度量目标

**北极星**：Weekly Active Conversations（WAC）—— 12 周内从当前基线增长 5x（具体基线需埋点确认）。

**反指标**（需要警惕）：Dev Mode 启用率 < 25%（意味着程序员用户流失）。

---

## 2. 跨文档共识：10 个最高优先级行动

> 这 10 项是**4 份源文档同时指向**的问题。每项标注：来源（哪些文档提到）、严重度、成本、依赖。

### Action 1：修复 Welcome.tsx 重复段落（P0 Bug）

- **来源**：PM 审查 §top-1（行 196-209 重复段落）
- **严重度**：🔴 P0（影响每个新用户）
- **成本**：S（1 小时）
- **依赖**：无
- **行动**：删除重复的 "Prefer a different autonomy level?" 段落

### Action 2：修复 Chat.tsx Attach 按钮无 onClick（P0 Bug）

- **来源**：PM 审查 §top-2
- **严重度**：🔴 P0（用户期望能上传文件但点击无反应）
- **成本**：M（4 小时，需要接入 Tauri 文件对话框）
- **依赖**：无
- **行动**：接入 `@tauri-apps/plugin-dialog`，支持 PDF/图片/CSV 上传

### Action 3：修复 Extensions 页面 Skill 卡片不可点击（P0 Bug）

- **来源**：PM 审查 §top-3
- **严重度**：🔴 P0（用户期望点击 skill 进入详情但无反应）
- **成本**：S（2 小时）
- **依赖**：无
- **行动**：为 skill 卡片添加点击 → 跳转详情页或展开抽屉

### Action 4：品牌副标题从 "AI Code Assistant" 改为 "Your AI Workspace"

- **来源**：定位转型 §1.1、PM 审查 §naming
- **严重度**：🟡 P1（影响品牌认知）
- **成本**：S（30 分钟，改 Sidebar.tsx 一行）
- **依赖**：无
- **行动**：`Sidebar.tsx` line ~110 副标题改为 "Your AI Workspace"

### Action 5：全局重命名（11 个术语）

- **来源**：定位转型 §4.1、PM 审查 §naming、新手审查
- **严重度**：🟡 P1（影响所有非程序员用户理解）
- **成本**：M（2-3 天，涉及 Sidebar / route / 多个 page 标题 / 测试更新）
- **依赖**：Action 4（同时做）
- **P0 重命名**：
  - Mission Control → **Conversations**
  - Triage → **Inbox**
  - Goals → **Projects**
  - Hooks → **Triggers**（在 Automations 下作为子标签）
  - Routines → **Schedules**（在 Automations 下作为子标签）
- **P1 重命名**：
  - Extensions → **Integrations**
  - Worktrees → **Workspaces**
  - Data Sources → **Connections**
  - Strategic Focus → **Today's Mission**
  - Agent Swarm → **Active Agents**
  - Quick Inject Task → **Add Task**

### Action 6：Sidebar 双模式（Simple / Dev）

- **来源**：定位转型 §6.1、导航 IA 文档
- **严重度**：🟡 P1（核心架构变化）
- **成本**：L（5-7 天，需要重构 Sidebar + 添加 mode 切换 + 测试）
- **依赖**：Action 5
- **行动**：
  - 默认 Simple Mode：5 个顶层入口（Conversations / Projects / Inbox / Automations / Integrations）
  - Dev Mode 可选（Welcome 第 3 步勾选 "I'm a developer"）：解锁 Mission Control / OPC / Perf
  - localStorage 持久化用户选择

### Action 7：Welcome 向导从"选 Provider"改为"选任务"

- **来源**：定位转型 §4.3、新手审查
- **严重度**：🟡 P1（影响首日留存）
- **成本**：M（3-4 天）
- **依赖**：Action 4
- **行动**：4 步向导
  - Step 1: "What do you want to do?"（Write / Research / Code / Automate / Just chat）
  - Step 2: 基于任务推荐模型（不再强制选 provider）
  - Step 3: 连接工具（可选，可跳过）
  - Step 4: 完成 + 显示推荐快捷键 + 模板入口

### Action 8：Automation 升格为顶层导航

- **来源**：定位转型 §6.4、PM 审查
- **严重度**：🟡 P1（差异化壁垒显性化）
- **成本**：S（1-2 天，主要是 Sidebar 结构调整）
- **依赖**：Action 6
- **行动**：
  - 把 Hooks + Routines + Profiles 从折叠子组升为顶层 `🤖 Automations`
  - 内部三个子页：Schedules（Routines）/ Triggers（Hooks）/ Permission Modes（Profiles）

### Action 9：Conversations 视图（替代 Mission Control 主视图）

- **来源**：定位转型 §6.3、新手审查
- **严重度**：🟡 P1（首屏体验）
- **成本**：M（3-4 天）
- **依赖**：Action 5（先重命名）
- **行动**：
  - 从 5 列 Kanban 改为 list 视图
  - 加入 "Agent-run / Scheduled / Pinned" 过滤
  - Dev Mode 可切回 Kanban 视图

### Action 10：删除/修复 fabricated billing data + dead hook events

- **来源**：PM 审查 §top-4 / §top-5
- **严重度**：🟡 P1（数据真实性 + 系统可靠性）
- **成本**：S（半天）
- **依赖**：无
- **行动**：
  - Billing 页面：明确标注 "Demo mode" 或接入真实数据
  - 删除 5 个 dead hook events（已在 PM 审查文档列出）

---

## 3. 4 周可执行 Sprint（前 4 周细化）

> 12 周总体路线图见定位转型文档 §9。这里细化前 4 周（最关键的品牌与入口改造）。

### Sprint 1（W1）：品牌 + 关键 Bug

**目标**：新用户打开 Shannon 看到的不再是"开发者工具"。

| 任务 | 工时 | 负责 | 验收 |
|---|---|---|---|
| Action 1：修复 Welcome 重复段落 | 1h | frontend | Snapshot 测试更新 |
| Action 2：修复 Chat Attach 按钮 | 4h | frontend | 能上传 PDF/图片 |
| Action 3：修复 Extensions skill 卡片点击 | 2h | frontend | 点击跳详情 |
| Action 4：品牌副标题改为 "Your AI Workspace" | 0.5h | frontend | Sidebar 截图 |
| 文档：更新 README + landing page 文案 | 2h | writer | 评审通过 |

**Sprint 1 验收**：
- 所有 P0 bug 修复
- 新打开 Shannon 看到新品牌
- 测试覆盖率不下降

### Sprint 2（W2）：全局重命名 + Welcome 重设计

**目标**：非程序员用户能看懂所有页面名称。

| 任务 | 工时 | 负责 | 验收 |
|---|---|---|---|
| Action 5：全局重命名（11 个术语） | 16h | frontend + writer | Sidebar/route/test 全更新，649 测试不退化 |
| Action 7：Welcome 向导 4 步重设计 | 24h | frontend + designer | Welcome.tsx + test 重写 |
| 路由更新（`/mission-control` → `/conversations` 等保留重定向） | 4h | frontend | 老链接仍可用 |
| Sidebar 文案与图标对齐 | 2h | designer | 视觉一致 |

**Sprint 2 验收**：
- 所有页面用新名称
- Welcome 第 1 步问"你想做什么"而非"用什么 LLM"
- 老用户从老 URL 访问能自动重定向

### Sprint 3（W3）：Sidebar 双模式 + Automation 升格

**目标**：Sam（非程序员）看到的简洁版，Alex（程序员）能切换完整版。

| 任务 | 工时 | 负责 | 验收 |
|---|---|---|---|
| Action 6：Sidebar 双模式实现 | 32h | frontend | Simple/Dev 切换可用 |
| Action 8：Automation 升格为顶层导航 | 8h | frontend | ⌘A 快捷键 |
| Mode 切换 UI（Settings 里） | 4h | frontend | 用户可手动切换 |
| Welcome 第 4 步："I'm a developer" checkbox | 2h | frontend | 选择后启用 Dev Mode |
| 测试：双模式各自有覆盖 | 8h | qa | Sidebar.test 双模式 |

**Sprint 3 验收**：
- 默认 Simple Mode 5 入口
- Dev Mode 解锁 Mission Control / OPC / Perf
- 用户选择持久化

### Sprint 4（W4）：Conversations 视图 + Today 视图 MVP

**目标**：首屏不再是开发者式的 Kanban，而是用户式的"今天的事"。

| 任务 | 工时 | 负责 | 验收 |
|---|---|---|---|
| Action 9：Conversations 视图 list 化 | 24h | frontend | 替代 Mission Control 主视图 |
| Today 视图 MVP（聚合 agents + scheduled + inbox） | 16h | frontend | 路由 `/today` |
| Kanban 视图降级为 Dev Mode 子视图 | 4h | frontend | Dev Mode 可切回 |
| Chat 模板行（4 个：Email / Summary / Research / Code） | 8h | frontend + writer | Chat.tsx 模板区 |

**Sprint 4 验收**：
- Conversations 默认 list 视图
- Today 视图作为可选主页
- Chat 底部有模板入口

---

## 4. 决策矩阵（用户审核）

> 用户对每个决策点 ✅/❌/⚠️（同意/不同意/讨论）。任何 ❌ 都需要明确替代方案。

### 4.1 战略层决策

| # | 决策 | 推荐 | 影响 |
|---|---|---|---|
| **D1** | 品牌定位从"AI Code Assistant"转向"Your AI Workspace" | ✅ | 12 周转型 |
| **D2** | 目标用户扩展到非程序员（双画像） | ✅ | GTM 渠道调整 |
| **D3** | 核心差异化：automations + multi-agent + multi-provider + 本地优先 | ✅ | 工程优先级 |
| **D4** | 商业模式：本地永久免费 + Pro 层 $20/月 | ⚠️ | 需财务模型验证 |

### 4.2 命名层决策

| # | 当前 | 推荐 | 用户决定 |
|---|---|---|---|
| **D5** | AI Code Assistant | Your AI Workspace | ___ |
| **D6** | Mission Control | Conversations | ___ |
| **D7** | Triage | Inbox | ___ |
| **D8** | Goals | Projects | ___ |
| **D9** | Hooks | Triggers | ___ |
| **D10** | Routines | Schedules | ___ |
| **D11** | Extensions | Integrations | ___ |
| **D12** | Strategic Focus | Today's Mission | ___ |
| **D13** | Quick Inject Task | Add Task | ___ |

### 4.3 架构层决策

| # | 决策 | 推荐 | 代价 |
|---|---|---|---|
| **D14** | Sidebar 双模式（Simple 默认 / Dev 可选） | ✅ | 5-7 天工时 |
| **D15** | Automation 升格为顶层导航 | ✅ | 1-2 天工时 |
| **D16** | Conversations 视图 list 替代 Kanban（Kanban 降级 Dev Mode） | ✅ | 3-4 天工时 |
| **D17** | Welcome 4 步向导（Task → Model → Tools → Done） | ✅ | 3-4 天工时 |
| **D18** | Today 视图作为可选主页 | ⚠️ | 可选，3-5 天 |

### 4.4 执行层决策

| # | 决策 | 推荐 | 备注 |
|---|---|---|---|
| **D19** | 主题色从蓝紫改为暖橙或湖绿 | ⚠️ | 需要 designer 输入 |
| **D20** | 12 周路线图（W1-W4 品牌 + W5-W8 自动化 + W9-W12 生态） | ✅ | 见定位转型 §9 |
| **D21** | 北极星指标：Weekly Active Conversations | ✅ | 需要埋点 |
| **D22** | 反指标：Dev Mode 启用率 < 25% 触发警报 | ✅ | 双画像平衡 |

---

## 5. 风险登记表

> 来源：定位转型 §8，结合 PM 审查 + 新手审查的具体问题。

| 风险 ID | 描述 | 概率 | 影响 | 缓解 | 预警信号 |
|---|---|---|---|---|---|
| **R1** | 程序员用户流失（不喜欢消费品化） | 中 | 高 | 双模式 Sidebar 保留所有专业功能 | Dev Mode 启用率 < 25% |
| **R2** | 命名重命名破坏老用户书签 | 高 | 低 | 老路由保留重定向 | 404 报警 |
| **R3** | 测试覆盖率在重构中下降 | 高 | 中 | Sprint 1-2 每日检查 649 测试 | 测试数 < 600 |
| **R4** | 新用户仍看不懂（命名改了但 UX 没改） | 中 | 高 | 新手审查迭代 + 用户访谈 | D7 留存 < 20% |
| **R5** | Automation 升格后用户觉得"过于复杂" | 中 | 中 | 默认隐藏子项，渐进式展示 | Automation 跳出率 > 50% |
| **R6** | 品牌重塑后 SEO 退化 | 低 | 中 | 老域名 + 老关键词保留 | 自然搜索流量下降 |
| **R7** | Welcome 重设计后设置失败率上升 | 中 | 高 | 默认值合理（不需要任何选择就能进入 Chat） | Welcome 完成率 < 80% |
| **R8** | fabricated billing data 被用户发现 | 已存在 | 高 | Sprint 1 立即修复 | 用户投诉 |
| **R9** | 12 周内对手（Claude Desktop）推出 hooks | 中 | 高 | 抢先 6 个月建立 automations 生态 | 对手发布公告 |
| **R10** | 工程资源不足（4 周完成 4 sprint） | 高 | 高 | 严格按优先级砍 P2 | 延期 > 1 周/sprint |

---

## 6. 度量与验证计划

### 6.1 埋点清单（W1 第一周完成）

| 事件 | 触发 | 目的 |
|---|---|---|
| `welcome_step_complete` | 完成 Welcome 某步 | 漏斗分析 |
| `welcome_skip` | 跳过 Welcome | 流失分析 |
| `chat_first_message` | 首次发消息 | 北极星前置 |
| `conversation_create` | 创建会话 | 北极星 |
| `automation_create` | 创建 automation | 差异化采纳 |
| `dev_mode_toggle` | 切换 Dev Mode | 双画像平衡（反指标） |
| `template_use` | 使用模板 | 新手友好度 |
| `provider_switch` | 切换 provider | 多 provider 价值 |
| `mcp_install` | 安装 MCP | 生态扩展 |

### 6.2 验证里程碑

| 周次 | 验证内容 | 通过标准 |
|---|---|---|
| W2 末 | 5 个非程序员用户试用 Welcome | ≥ 4/5 能在 3 分钟内完成首对话 |
| W4 末 | Sidebar 双模式 A/B 测试 | Simple Mode 用户 D7 留存 > Dev Mode |
| W6 末 | Today 视图用户访谈 | ≥ 70% 用户认为"比 Mission Control 更直观" |
| W8 末 | 自动化创建漏斗 | ≥ 25% 新用户在 D7 创建过 automation |
| W12 末 | 北极星对比 | WAC 较 W0 增长 ≥ 3x |

---

## 7. 资源与角色

### 7.1 推荐团队配置（最小可行）

| 角色 | 职责 | 工时占比 |
|---|---|---|
| **Tech Lead**（你） | 决策、代码审查、架构 | 30% |
| **Frontend Engineer × 1.5** | 实施 Sprint 任务 | 100% × 1.5 |
| **Designer × 0.5** | Welcome 重设计、主题、图标 | 50% |
| **Tech Writer × 0.3** | 文案、模板、文档 | 30% |
| **QA × 0.3** | 测试覆盖、用户研究协调 | 30% |

**总计**：~3 FTE × 12 周 = 36 人周。

### 7.2 外部依赖

- **用户研究**：5-10 个非程序员志愿者（W2 / W4 / W6 / W8 各一轮）
- **设计资源**：暖橙 / 湖绿主题的视觉规范（W1 完成）
- **法务**：Billing 模式调整的合规审查（W9 前）

---

## 8. 与现有产出的关系

### 8.1 已完成（本次委托的 1-2 任务）

✅ **Mock 数据脚本**（任务 1）—— `ui/src/lib/mock/` 下完整 mock 系统，支持 `VITE_MOCK_MODE=1` 启动
✅ **Kanban 列统一**（任务 2）—— `task-status.ts` + `KanbanBoard.tsx` 共享 primitive，Mission Control 和 OPC 共享 taxonomy（5 family: queued/active/blocked/done/failed），649 UI 测试通过

### 8.2 待决策（本文档 §4）

⏸️ **22 个决策点**等待用户 ✅/❌/⚠️

### 8.3 决策后启动

🚀 **Sprint 1（W1）**：品牌 + 关键 Bug（5 项任务，~9.5h 工时）

---

## 9. 推荐的下一步

> 用户读完本文档后，建议按顺序：

1. **审阅 §4 决策矩阵**，对每个 D1-D22 标注 ✅/❌/⚠️
2. **优先回答 D1-D3**（战略层）—— 这三个决定其他所有决策
3. **回答 D5-D13**（命名层）—— 如果有反对，提出替代命名
4. **回答 D14-D18**（架构层）—— 如果有反对，讨论替代方案
5. **回答 D19-D22**（执行层）—— 这些可以延后讨论
6. **批准 Sprint 1**（W1 任务清单）—— 立即可启动
7. **资源确认**：§7.1 团队配置是否可行？

如果用户对 D1-D3 全部 ✅，建议立即启动 Sprint 1 的 5 项任务（共 ~9.5 小时工时，可在 1-2 天内完成第一波品牌与 bug 修复）。

---

## 10. 总结：一句话

> Shannon 已经是技术领先的产品；这次改进**不是修 bug**，而是**打开水龙头**——把已经具备的能力（multi-provider、agents、automations、Tauri、本地优先）释放给更大的市场。
>
> 4 份源文档（130KB 详细论证）→ 本综合方案（决策即用）→ 22 个决策点 → 等用户 ✅/❌ → Sprint 1 立即启动。

---

## 附录 A：源文档索引

| 文档 | 大小 | 核心论点 |
|---|---|---|
| [01-novice-user-review.md](01-novice-user-review.md) | 38KB | 非程序员视角的具体困扰，验证定位转型的必要性 |
| [02-navigation-ia-redesign.md](02-navigation-ia-redesign.md) | 36KB | Sidebar 信息架构详图，支撑双模式设计 |
| [03-senior-pm-audit.md](03-senior-pm-audit.md) | 22KB | 16 页面 PM 审查，12 项最高优先级 finding，P0 bugs |
| [04-product-repositioning.md](04-product-repositioning.md) | 33KB | 战略层转型方案，12 周路线图，OKR 度量 |
| **00-comprehensive-improvement-plan.md**（本文档） | 20KB | 综合决策即用方案 |

## 附录 B：术语对照速查

| 旧 | 新 | 出现位置 |
|---|---|---|
| AI Code Assistant | Your AI Workspace | Sidebar subtitle |
| Mission Control | Conversations | Sidebar / route / page |
| Triage | Inbox | Sidebar / route / page |
| Goals | Projects | Sidebar / route / page |
| Hooks | Triggers | Sidebar / route / page |
| Routines | Schedules | Sidebar / route / page |
| Extensions | Integrations | Sidebar / route / page |
| Worktrees | Workspaces | 全 codebase |
| Data Sources | Connections | Extensions 子页 |
| Strategic Focus | Today's Mission | OPC 头部 |
| Agent Swarm | Active Agents | OPC 内部 |
| Quick Inject Task | Add Task | OPC 输入框 placeholder |
| Background Task | Running Task | Tasks 页面 |

## 附录 C：Sprint 1 立即可做的 5 件事

> 即使是用户只同意 D1-D3 战略层决策，以下 5 件事（共 9.5h 工时）可以立即启动，因为它们无论如何都是改进：

1. 修复 Welcome.tsx 重复段落（1h）—— Action 1
2. 修复 Chat Attach 按钮 onClick（4h）—— Action 2
3. 修复 Extensions skill 卡片点击（2h）—— Action 3
4. 修复 fabricated billing data（2h）—— Action 10
5. 删除 5 个 dead hook events（0.5h）—— Action 10

这 5 项都是 **bug 修复**，与定位无关，可以视为**风险最低的第一步**。
