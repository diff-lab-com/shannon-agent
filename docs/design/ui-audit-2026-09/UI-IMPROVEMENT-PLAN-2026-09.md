# Shannon Desktop UI 审计与改进方案（2026-09）

**日期**: 2026-09-10 / 11
**审计对象**: Shannon Desktop `dev` 分支（Tauri 2 + React 19，mock 模式实测截图）
**对标竞品**: Claude Desktop（消费版）· Claude Code Desktop · OpenAI Codex app · ZCode Desktop v3.11.2（真机）
**目标**: 全面参考竞品流行的 UI/页面设计，相同功能使用一致的 UI/术语/用户流程，降低用户认知成本；美术方向对齐苹果玻璃风格（Liquid Glass / Glassmorphism）。
**增补专题**: [COMPONENT-LIBRARY-RESEARCH.md](./COMPONENT-LIBRARY-RESEARCH.md)——竞品组件库取证（ZCode/Codex/ChatGPT 均为 Radix+Tailwind 系）、候选库对比与选型结论（维持 Base UI + shadcn 模式）、实施规划 v2。
**配套**: [screenshots/shannon/](./screenshots/shannon/)（18 页实测截图）· [screenshots/competitors/](./screenshots/competitors/)（竞品截图与官方资料）· [docs/competitive-research-2026-09.md](../../competitive-research-2026-09.md)（功能层竞研）

---

## 实施状态（2026-09-11 更新）

**已实施并通过验证（`dev` 分支工作区，未提交）**：

| 项 | 文件 | 验证 |
|---|---|---|
| Composer 恒见（flex 流式布局修复，P0 根因：absolute 定位逃逸视口） | ComposerPanel.tsx / WorkspacePanel.tsx / MessageArea.tsx | after 截图 |
| Composer 玻璃材质（glass-surface） | ComposerPanel.tsx | after 截图 |
| 术语统一 30+ 处 ×2 locale（任务/收件箱/连接/指挥台/多方案对比/对话·Diff·预览） | zh-CN.json / en.json | vitest 1665 ✓ |
| OPC 状态/优先级枚举 i18n（IN_PROGRESS→进行中等，含优先级四档） | OpcAnalyticsDashboard.tsx + locales | vitest ✓ |
| 审批模式滑块→等宽分段控件（描述下沉，修复文字重叠 P0） | GeneralSettings.tsx | after 截图 |
| Triage「继续会话」提升为带文字主按钮（Codex review-queue 语法） | Triage.tsx | after 截图 |
| Usage/任务等 8 文件蓝色选中态→主色统一 | Usage.tsx 等 | grep 0 残留 |
| 演示模式错误 toast 中文化 | coreMock.ts | — |
| Liquid Glass token 层（三级材质/elevation/动效/降级/性能约束） | index.css | tsc ✓ |
| 新增 i18n 键：inbox.action.resume、status.*（9 枚举）等 | locales | vitest ✓ |
| 23 个旧术语测试断言同步更新 | 5 个测试文件 | vitest ✓ |

**验收证据**: `screenshots/shannon-after/`（chat/tasks/triage/usage/opc/settings 6 页 after 对比图）
**回归**: `vitest` 1665 passed / 0 failed；`tsc --noEmit` 通过；改动文件 eslint 通过。

**第二批实施（2026-09-11，commit 待填）**：

| 项 | 文件 | 验证 |
|---|---|---|
| 深色优先：无存储默认 tokyo-night（对齐 Codex/ZCode 开箱深色；'system' 仍按 OS 解析） | ThemeContext.tsx | 02-chat-dark.png |
| Welcome 4 步→2 步：任务+模型同屏 → 完成（工具按任务推荐预填，ToolsStep 从流程退役，Stepper 支持自定义标签） | Welcome.tsx / welcome/components.tsx | 01-welcome-2step-dark.png |
| Project 分组：侧栏会话按 working_dir 尾段分组显示项目头（仅浏览态且 >1 项目时） | SidebarSessions.tsx | vitest ✓ |
| Welcome/MigrationWizard/ThemeContext 测试重写为两步流程断言 | 3 个测试文件 | 155 文件全过 |

**Wave 3 说明**：可拖拽面板（拖动/调宽/最大化）与「布局按项目（working_dir）持久化」经代码核实在 WorkspaceGrid/useWorkspaceLayout 中**已存在**（P1-1/P1-5 workspace host），本批无需改动；侧栏 Project 分组（本批）补齐了会话维度的项目心智。多窗口仍为后续项。

**遗留（Wave 2/3 未实施项）**：全站逐页深色打磨（token 已随主题切换，个别页面存在局部对比度待调）、Extensions 目录化内容扩充、多窗口——需按 §7 路线图排期。

---

## 0. TL;DR

1. **Shannon Desktop 的功能覆盖不输竞品，但「认知面」全面落后**：同样一个概念在三处叫三个名字（已排程/定时任务、分流队列/收件箱、并行方案/best-of-N），而竞品已经完成「Tasks / Inbox / Automations」的平实语言收敛。
2. **视觉上 Shannon 是「扁平白卡 + 单一紫罗兰」，竞品全部完成了「深色优先 + 材质层次 + 沉浸工作区」的迭代**：Claude 的暖色极简、Codex 的黑色指挥中心、ZCode 的近黑三栏，都给了页面明确的「层级语言」；Shannon 18 页里几乎每页都是同一层白卡，无主次。
3. **最大的结构性差距是工作区形态**：Claude Code 的可拖拽多面板（chat/diff/browser/terminal/editor）+ 预览自检、Codex 的 thread+review queue、ZCode 的 composer 三要素（权限模式/模型/推理档）已成为 2026 桌面端标配语法；Shannon 是固定三栏、composer 不可恒见、无终端无预览。
4. **改进方案分三个 Wave**：Wave 1（1–2 周）术语统一 + 导航/状态修复 + 色彩收敛；Wave 2（3–6 周）Liquid Glass 设计系统落地 + 深色优先重绘；Wave 3（6–12 周）可拖拽面板工作区 + Project 概念 + 集成终端/预览。
5. **玻璃风格不是加毛玻璃滤镜**：本文给出完整的三级材质 token 体系（base/surface/overlay）、深浅双模式、性能约束与降级策略，全部可在现有 Tailwind v4 + MD3 token 管线上实现。

---

## 1. 审计方法与证据集

### 1.1 证据类型与可信度

| 类型 | 来源 | 说明 |
|---|---|---|
| A 真机截图 | Shannon 18 页（mock 模式实测）；ZCode Desktop v3.11.2 真实运行窗口 | 最高可信，`screenshots/shannon/`、`competitors/zcode-desktop-main.png` |
| B 官方网页截图 | anthropic.com/claude（5 张）、zcode.z.ai（5 张） | 官方 marketing 页，展示官方想让用户看到的 UI |
| C 媒体/第三方截图 | intuitionlabs.ai、kingy.ai、proflead.dev 的 Codex 文章配图 | 中等可信，已在文件名标注来源 |
| D 官方文字资料 | code.claude.com/docs/en/desktop 全文（100KB Markdown 已存档）；docs/competitive-research-2026-09.md | 用于补齐无法截图的部分（OpenAI 域名被本机网络封锁，Codex Desktop 细节以官方文字 + 媒体交叉验证） |

> ⚠️ 本机网络限制：openai.com、developers.openai.com、web.archive.org、github.com 均不可达（403/超时）。Codex app 部分以 C+D 类证据为准，落地评审时建议由可访问 OpenAI 官网的同事复核截图。

### 1.2 Shannon 截图清单（1440×900@2x，mock 模式 `pnpm demo`）

| # | 文件 | 路由 | 页面 |
|---|---|---|---|
| 01 | 01-welcome.png | /welcome | 引导向导 |
| 02 | 02-chat.png | /chat | 对话（主页面） |
| 03 | 03-tasks.png | /tasks | 已排程/定时任务 |
| 04 | 04-triage.png | /triage | 分流队列/收件箱 |
| 05 | 05-usage.png | /usage | 用量统计 |
| 06–11 | 06…11-extensions-*.png | /extensions/* | 精选/MCP/技能/智能体/数据源/插件 |
| 12 | 12-opc.png | /opc | 单人公司（OPC） |
| 13 | 13-editor.png | /editor | 代码编辑器 |
| 14 | 14-memory.png | /memory | 记忆 |
| 15–18 | 15…18-settings-*.png | /settings/* | 通用/主题/模型/权限 |

---

## 2. 竞品 UI 基准画像

### 2.1 Claude Desktop（消费版，B 类证据 + D 类文档）

- **信息架构**：三个一级 tab——**Chat**（对话）/ **Cowork**（Dispatch 长任务与办公代理）/ **Code**（软件开发，即 Claude Code Desktop）。设置是独立窗口/面板，不占一级导航。
- **布局**：左侧会话列表（按 Projects 分组）+ 中央对话流（单列，内容 max-width 收窄）+ 右侧 **Artifact 面板**（代码/文档/网页预览，自动弹出、可全屏）。
- **视觉**：terracotta 橙 + 奶油暖白；衬线 display 字体做品牌标题、无衬线做正文；大量留白；圆角大而统一；阴影极轻、靠暖色和留白分层。
- **术语**：Chats / Projects / Artifacts / Styles / Connectors（MCP 的用户语言）/ Routines（自动化）/ Checkpoints（可回滚快照）。

### 2.2 Claude Code Desktop（A/D 类证据：官方文档全文）

- **核心语法：会话（session）为中心 + 可拖拽面板**。左侧栏 = 并行会话列表（每会话独立 git worktree）；会话内任意拼装 **chat / diff / browser / terminal / file editor / iOS Simulator** 面板，布局按 repo 保存。
- **Diff 评审是一等公民**：逐文件 diff、行内评论、`/review`；PR 创建后 **PR 监控面板**（gh 轮询 CI，失败 Auto-fix、通过 Auto-merge）。
- **预览自检闭环**：Browser pane 自动起 dev server，agent 自己截图/查 DOM/点击/填表验证改动。
- **其他标配**：Checkpoints（每个 prompt 快照，`/rewind` 还原代码/对话）；Side chat（会话内提小问题不跑题）；手机 Dispatch；Connectors 目录（1,600+ MCP 一键装）；沙箱（Seatbelt/bwrap）。
- **术语**：Sessions / Panes / Diff review / Checkpoints / Side chat / Routines / Connectors / Cowork。

### 2.3 OpenAI Codex app（C/D 类证据）

- **核心语法：thread（线程）为中心的项目级指挥中心**。按项目分组的 thread 并行列表；每 thread 独立 worktree；与 CLI/IDE 扩展共享会话。
- **Automations → Review queue → 原 thread 续跑**：定时任务跑完的结果进评审队列（Shannon 的 Triage 对位），一键回到原 thread 上下文继续——「the review queue IS the product」。
- **Best-of-N**：`--attempts 1–4` 并行多方案 × worktree，并排 diff 择优。
- **In-app browser**：划词评论 → agent 处理；多终端面板；90+ 插件；`/permissions` 会话内调权；3 档沙箱 × 审批策略。
- **视觉**：近黑背景、内容密度高、状态色克制（灰/绿/红三态为主），工具感强。
- **已知弱点**（Shannon 的机会）：多窗口非一等公民（#33205）；review 噪音大；token 计价混乱。

### 2.4 ZCode Desktop（A 类证据：真机 v3.11.2 + 官网截图）

- **布局（实测截图 `zcode-desktop-main.png`）**：近黑三栏——左窄栏（新任务/搜索/自动化/并行任务/项目分组/会话列表/底部账号设置）；中央会话流 + **composer 三要素**：权限模式下拉（「完全访问」）+ 模型选择（GLM-5.3-Flash）+ 推理档（「最高」）；右侧内置浏览器面板（可输入 URL、固定 1440×900 视口）。
- **交互标配**：Shift+Tab 四档执行模式（逐项确认/自动编辑/Plan/Full access）；Goal Mode（长任务目标管理，官网核心卖点）；闲时任务（Idle-time，官网「Noon break?」卡片）；Usage Stats；Edit History；/btw 边车会话。
- **视觉**：深灰黑 (#0d0d0d) 单色阶 + 极细分隔线 + 少量蓝紫强调；无重装饰；字体层级清晰。中文本地化完整。

### 2.5 竞品共性总结（= Shannon 的对标基线）

1. **深色优先**（4/4 产品默认或主打深色）；2. **左侧会话/任务列表 + 中央内容流 + 右侧工具面板**的三栏语法；3. **composer 三要素**：权限/执行模式 + 模型 + 推理力度；4. **diff 评审内联在会话里**；5. **自动化结果回流收件箱**；6. **Project/工作区分组**；7. **术语收敛为 Tasks/Inbox/Automations/Connectors/Diff**。

---

## 3. 逐页面对比：异同、问题、改进点

> 每页格式：Shannon 现状（截图）→ 竞品对位 → 异同 → 问题 → 改进建议。问题分级 🔴 P0（认知/流程障碍）、🟡 P1（体验/一致性）、🟢 P2（打磨）。

### 3.1 Welcome / 引导（01-welcome.png）

- **现状**：4 步向导（任务→模型→工具→完成），四选一卡片 + stepper；大片空白；无产品价值展示。
- **竞品对位**：Claude/ChatGPT 桌面端引导 2 步（登录→首问）；ZCode 登录即用；竞品都在首屏先展示「产品能干什么」的示例画廊。
- **异同**：同为向导式；Shannon 步数翻倍且每步都要求用户做「配置决策」而不是「开始干活」。
- **问题**：🔴 引导 4 步 vs 竞品 2 步，首日流失风险；🟡 左上「跳过→」是唯一逃生口且文案像链接不像按钮；🟡 空间利用率低（内容只占中央 40%）。
- **改进**：压缩为 2 步——①「用什么模型」（默认替用户选好，高级选项折叠）②「第一个任务」（输入框 + 任务模板画廊，选即开跑）；右侧/背景放产品场景演示（玻璃卡片自动播放任务示例）；完成后直接落入 Chat 并预填 prompt。

### 3.2 Chat / 对话（02-chat.png）

- **现状**：左侧栏（新对话/会话搜索/分组「工作」）+ 顶部视图 tab「聚焦聊天/评审/构建」+ 重置布局 + 消息流。**composer（输入框）在该视口不可见**。
- **竞品对位**：Claude Code = chat pane + diff/browser/terminal 面板自由拼装；Codex = thread + diff 评审内联；ZCode = 中央会话流 + composer 恒见（模式/模型/推理档三要素）。
- **异同**：信息骨架相似（侧栏+会话流）；**Shannon 缺右侧工具面板、缺 composer 恒见、缺内联 diff 评审**；竞品会话流都是单列窄栏（max-width 收窄），Shannon 消息卡通栏拉满 1160px，长文本可读性差。
- **问题**：🔴 composer 不选中会话不可见——新用户落在 /chat 看不到「能输入」的地方，不知道怎么开始；🔴 「聚焦聊天/评审/构建」是自造术语，竞品叫 Diff review / Preview，且三个视图的能力用户无从预期；🟡 消息流通栏无最大宽度；🟡 助手消息大卡片边框重、视觉噪音高（竞品是无边框流式文本 + 轻工具卡）；🟡 空会话无引导态（竞品有空状态建议 prompt）。
- **改进**：见 §6.4 Chat 重设计——composer 恒见 + 三要素对齐 ZCode；单列 max-w-3xl；工具调用折叠卡对齐 Claude Code（read-only 一行、destructive 高亮 + 确认）；视图 tab 改名「对话 / Diff / 预览」。

### 3.3 Tasks / 已排程（03-tasks.png）

- **现状**：侧栏叫「已排程」，页头 H1 叫「定时任务」，副标题「管理和监控你的自动化智能工作流」；一屏 7 个操作控件（所有团队/筛选/月历视图/关系图/新建例行任务/并行方案/新建后台任务）；5 个 tab（进行中/例行程序/执行流水线/历史/工作空间）；best-of-N 卡片显示分支成本 chips；目标运行卡显示轮数/花费/停滞计数。
- **竞品对位**：Codex = **Automations**（创建时一个主 CTA）→ 结果进 review queue；Claude = **Routines**；ZCode = **自动化**（左栏一级项）+ Goal Mode。
- **异同**：能力上 Shannon 最全（cron+best-of-N+goal），但 **页面定位混杂**：活跃任务、例行程序、流水线、历史、worktree 全堆在一页；竞品把「一次性派活」和「周期自动化」分成两个心智（Tasks vs Automations）。
- **问题**：🔴 **一词三轨**：侧栏「已排程」/ 页头「定时任务」/ 竞品叫 Automations——用户每换一页都要重新确认「这是不是同一个东西」；🔴 7 控件无主次（6 月审计已指出 5 控件问题，现在更多了）；🔴 「并行方案 (best-of-N)」英文混排，同一概念在 Triage 页又叫「并行方案」；🟡 「新建后台任务」「新建例行任务」「并行方案」三个创建入口的关系无法直觉理解。
- **改进**：页级更名 **「任务」Tasks**（一级），内部分三个子区对齐竞品心智：**自动化 Automations**（cron/例行，Codex 语言）、**目标 Goals**（ZCode Goal Mode 语言，CLI goal 体系桌面化入口）、**后台任务 Background tasks**；页面只留一个主 CTA「新建」+ 视图切换（列表/日历）；best-of-N 卡保留但中文命名「多方案对比」，分支 chips 简化为 ✓/✗ + 成本一行。

### 3.4 Triage / 分流队列（04-triage.png）

- **现状**：侧栏「分流队列」，页头 H1「收件箱」；来源筛选（例行任务/计划任务/目标/触发器/并行方案）；条目卡片带 hover 图标操作（重跑/已读/归档）；badge 显示待处理数。
- **竞品对位**：Codex **review queue**——自动化结果一键回原 thread 续跑；Claude Dispatch 完成推送进 Cowork。
- **异同**：概念对位准确（这是 Shannon 做对了的地方），但闭环差「最后一公里」：条目操作里没有「**去原会话续跑**」这个竞品最关键的动作。
- **问题**：🔴 侧栏/页头双术语（分流队列 vs 收件箱）；🔴 缺「在原会话中继续」主操作（G2 缺口在 UI 上的直接体现）；🟡 操作仅图标无文字，5 个图标需要逐个 hover 学习；🟡 条目无来源页深链（无法从结果跳到产生它的自动化）。
- **改进**：统一命名 **「收件箱 Inbox」**（侧栏、页头、badge 一致）；条目主按钮 = **「继续会话」**（跳原 session 并带结果上下文）+ 次操作（归档/重跑/查看来源自动化）；操作改「主按钮 + 溢出菜单」。

### 3.5 Usage / 用量（05-usage.png）

- **现状**：居中窄栏「用量统计」+ 总览/按会话 tab + 时间范围 chips + 空状态；demo 错误 toast 中英混排；**侧栏无入口**（只能从别处跳转）。
- **竞品对位**：ZCode = Usage Stats（官网一级卖点页）；Hermes = 状态栏常驻上下文拆解 + 缓存命中率；Claude/Codex = 订阅额度页。
- **异同**：竞品都把成本/额度放在一级可达位置（Shannon 的 BYOK 成本故事是核心差异化，却无入口）；Shannon 缺上下文按类别拆解和缓存命中率（竞研 G3 已列）。
- **问题**：🔴 侧栏无入口；🔴 错误提示英文（"This feature is not available in demo mode"）出现在中文 UI——i18n 缺口；🟡 「总览」选中态是蓝色、旁边「近 30 天」选中态是紫色——同一页面两种选中色；🟡 空状态无「去发起第一个对话」引导。
- **改进**：侧栏「资源」组加 **「用量 Usage」** 入口（badge 显示今日花费）；页面结构对位 Hermes：顶部 4 KPI 卡（今日成本/本周成本/上下文构成/缓存命中率）+ 按会话/按模型明细表 + 每会话预算上限设置；统一选中态为主色。

### 3.6 Extensions / 扩展（06–11-extensions-*.png）

- **现状**：7 个子 tab（精选/MCP 服务器/技能/智能体/数据源/插件/已安装）；精选页仅 3 张大卡（Notion/GitHub/Slack），品牌渐变按钮；搜索框独立一行。
- **竞品对位**：Claude = **Connectors** 目录（1,600+，允许企业 allowlist）；Codex = 插件 90+；Hermes = Skills Hub；ZCode = 插件市场。
- **异同**：Shannon 子页结构清晰度尚可，但目录内容单薄、无分类导航、无「安全已验证」的展示——而 **提示注入扫描 + 签名校验是 Shannon 独有能力，完全没露出**。
- **问题**：🟡 精选页 3 卡片太空（竞品目录都有数百条目 + 分类 + 搜索即首屏）；🟡 「扩展」命名 vs 竞品「Connectors/插件/技能」各成体系，Shannon 一个词盖了 7 种东西；🟡 Slack 卡渐变按钮与全站风格离群（6 月审计已记录）；🟢 已安装页与各类型 tab 信息重复。
- **改进**：一级名改 **「连接 Connectors」**（Claude 语言，覆盖 MCP/数据源）+ 二级「技能 Skills」「智能体 Agents」「插件 Plugins」；首屏做分类 + 精选 + 搜索三段式；每卡加安全徽章（「注入扫描 ✓ 已签名 ✓」）——把安全治理做成可见卖点；渐变按钮收敛到品牌色。

### 3.7 OPC / 单人公司（12-opc.png）

- **现状**：顶栏「单人公司」；「今日使命」hero 卡 + 7 天分析 + 状态/优先级分布 + 每日活动柱状图 + 按分配对象负载；**侧栏无入口**（简单模式下完全不可达）；状态显示原始枚举 `IN_PROGRESS/PENDING/QUEUED/BLOCKED/COMPLETED/FAILED`；优先级 `Critical/High/Normal/Low` 英文。
- **竞品对位**：Claude Code 有 **Mission Control** 概念（跨会话指挥视图）；无竞品有等价的「单人公司运营仪表盘」——这是 Shannon 独有资产（6 月审计结论维持）。
- **异同**：独有但被「藏」起来了：不在导航、不在快捷入口、状态枚举还是开发语言。
- **问题**：🔴 raw 枚举直接渲染（IN_PROGRESS/QUEUED 是数据库值不是 UI 文案）；🔴 无导航入口（dev 模式功能对简单模式用户=不存在）；🟡 图表棕黄+紫色组合沉闷，与品牌紫不协调；🟡 「单人公司」定位话术与「AI 工作空间」品牌话术脱节。
- **改进**：更名 **「指挥台 Mission Control」**（对齐 Claude Code 词汇），dev 模式一级入口 + 简单模式折叠进「任务」；枚举全部走 i18n 映射（进行中/排队中/已阻塞/已完成/失败）；图表色改品牌紫 + 语义色阶梯；若短期无资源打磨，可先并入 Tasks 页顶部「概览」区，避免维护孤页。

### 3.8 Editor / 代码编辑器（13-editor.png）

- **现状**：H1「代码编辑器」+ 一段说明 + 「文件路径」输入框 + 浏览按钮 + 加载文件按钮。整页其余空白。
- **竞品对位**：Claude Code = file pane（点会话里的文件路径即开，可编辑、Save/Discard）；Codex = 文件侧栏预览 + 多终端；ZCode = Edit History。
- **异同**：竞品的「看/改文件」是 **会话上下文内的面板**，Shannon 是 **孤立的工具页**（要用户手敲绝对路径）。
- **问题**：🔴 工具原型直接暴露为产品页（体验断崖）；🟡 手输路径 vs 竞品「点路径打开」差一个时代。
- **改进**：短期——会话内文件路径可点击 → 右侧文件面板（CodeMirror 已有，接 Claude Code file pane 语法）；长期并入 §6.4 的可拖拽工作区；独立 /editor 路由退役或转为「纯查看器」深链目标。

### 3.9 Memory / 记忆（14-memory.png）

- **现状**：统计卡（总数/偏好/决策/错误/项目数）+ 列表/图谱切换 + 项目/分类筛选 + 记忆卡（类型/来源会话跳转/使用次数/标签）。
- **竞品对位**：Claude = Projects memory（项目级记忆，自动注入）；Hermes = Memory Graph + 档案；ZCode = Project Memory（默认关）。
- **异同**：**全应用完成度最高的页面**，信息架构与竞品对位良好；缺口在「溯源可视化」（图谱视图较初级）与「透明度」（无「这条记忆为什么被注入」）。
- **问题**：🟢 mock 数据中英混排（真实数据同理：用户中文会话产生的记忆应存中文）；🟢 统计卡 5 个略冗余（错误=0 的卡常驻）。
- **改进**：保持结构；增加「注入预览」（下条消息会带哪些记忆，Hermes 式透明度卖点）；记忆内容语言跟随会话语言。

### 3.10 Settings / 设置（15–18-settings-*.png）

- **现状**：左滑出菜单（设置/主题/模型/权限/…8 项）+ 内容区大卡；审批模式用 5 档 slider（建议/计划/自动编辑/完全自动等），**5 档标签挤在同一行互相重叠**；服务商列表卡（Anthropic 使用中/GLM 可启用）。
- **竞品对位**：ZCode = Shift+Tab **4 档执行模式**（逐项确认/自动编辑/Plan/Full access）——composer 里恒可切换；Claude/Codex = 权限在会话内 `/permissions` 即可调。
- **异同**：Shannon 的 5 档粒度其实更细，但（a）藏在设置页深处、（b）slider 形态承载不了 5 档文案。
- **问题**：🔴 审批模式 slider 文案重叠不可读（1440px 下已重叠，1280px 更甚）；🔴 执行模式藏在设置里——竞品把「本次会话我要给 agent 多大权限」放在 composer 一键切换，这是使用频率最高的控件之一；🟡 页头「设置」vs 内容 H1「系统设置」双轨；🟡 服务商卡信息层级平（密钥状态/测试/启用等权）。
- **改进**：审批模式改 **4 档分段控件**（对齐 ZCode：逐项确认/自动编辑/计划/完全访问），文案一行一档 + 说明下沉到帮助 tooltip；composer 加模式切换 chip（见 §6.4）；设置页头统一「设置」，内容区用 Claude 式左 tab 布局 + 顶部搜索。

### 3.11 全局问题（跨页面）

| # | 问题 | 证据 | 分级 |
|---|---|---|---|
| G-1 | **术语双轨/三轨**：已排程=定时任务；分流队列=收件箱；并行方案=best-of-N；设置=系统设置 | 03/04/15 截图 | 🔴 |
| G-2 | **状态枚举 raw 英文**：IN_PROGRESS/PENDING/QUEUED/BLOCKED 直出 | 12-opc.png | 🔴 |
| G-3 | **i18n 破洞**：中文 UI 中出现英文错误 toast、英文混排 | 05-usage.png、03-tasks.png | 🔴 |
| G-4 | **选中态/语义色不统一**：Usage 蓝色 tab、OPC 棕色图表、Triage 红 badge、主题紫 | 05/12/04 | 🟡 |
| G-5 | **页面无层级**：18 页全是白底卡+1px 灰线，无主次、无玻璃/阴影层次 | 全部 | 🟡 |
| G-6 | **侧栏状态失明**：仅 Triage 有 badge；任务失败/目标阻塞/审批待办不可见 | 02–05 | 🟡 |
| G-7 | **composer 不恒见**、无模式/模型/推理档三要素 | 02-chat.png | 🔴 |
| G-8 | **无 Project/工作区分组**（竞品 4/4 都有项目维度） | 侧栏仅「工作」手动分组 | 🟡 |
| G-9 | 导航状态不持久（Tasks tab、Triage 筛选、面板开合） | 代码层 local state | 🟡 |
| G-10 | Usage/OPC/Editor/Memory 在简单模式无入口或藏匿 | 侧栏 | 🟡 |

---

## 4. User Journey / User Stories 对比

### J1 首次上手（Onboarding）

| 步骤 | Shannon 现状 | Claude/ZCode 基线 | 差距 |
|---|---|---|---|
| 下载→进入 | 安装后落 /welcome 4 步向导 | 登录即用，模型替你选好 | Shannon 多 2 步配置决策 |
| 第一个任务 | 向导完成后自行摸索 | 首屏模板/建议 prompt 直接开跑 | Shannon 无建议 prompt |
| 里程碑 | 无 | 5 分钟内见到 agent 干活 | — |

**User story**: 作为新用户，我想在 2 分钟内让 agent 完成第一件事，而不是先回答 4 个配置问题。→ §6.1

### J2 日常循环（Dispatch → Watch → Review → Merge）

| 环节 | Shannon 现状 | 竞品基线 | 差距 |
|---|---|---|---|
| 派活 | composer（但新会话才可见） | composer 恒见 + 模式三要素 | 🔴 |
| 观察 | 会话流（工具调用折叠） | 相同 + 右侧面板并行看 diff/预览 | 🟡 缺面板 |
| 评审 | 「评审」视图（自造术语） | Diff pane 内联行评论 | 🔴 缺内联 diff |
| 合并 | Tasks>worktrees 面板 | PR 监控 + Auto-merge | 🟡 |

**User story**: 作为开发者，我想在对话流里直接看到每次改动的 diff 并行内评论，而不是切到另一个视图。→ §6.4

### J3 自动化闭环（Create → Run → Inbox → Resume）

| 环节 | Shannon 现状 | Codex 基线 | 差距 |
|---|---|---|---|
| 创建 | 3 个创建入口混在一页 | Automations 单一入口 + 模板 | 🟡 |
| 运行 | 可看（例行程序/历史） | 可看 | ✓ |
| 结果 | 进 Triage 混排列表 | 进 review queue + **一键回原 thread 续跑** | 🔴 缺续跑 |
| 触发 | cron + webhook 通知 | cron + API endpoint + GitHub 事件 | 🟡 |

**User story**: 作为运维，我让机器人每晚巡检，第二天只想在收件箱里点「继续处理」让原会话拿着昨晚的结果开干。→ §6.3

### J4 成本控制（Budget → Monitor → Intervene）

| 环节 | Shannon 现状 | Hermes/ZCode 基线 | 差距 |
|---|---|---|---|
| 设预算 | 仅 goal 有 budget cap | 会话级 YOLO 开关/预算 | 🟡 |
| 监控 | usage 页（无侧栏入口） | 状态栏常驻成本/上下文/缓存 | 🔴 |
| 干预 | 无 | 超限暂停+询问 | 🔴 |

**User story**: 作为 BYOK 用户，我想给每个会话设预算上限，超限时 agent 停下来问我，而不是账单 surprises。→ §6.2

---

## 5. 问题清单汇总（按优先级）

**🔴 P0 — 直接阻碍使用/认知（Wave 1 修复）**
1. 术语双轨三轨（G-1，§3.3/3.4/3.10）
2. composer 不恒见 + 缺模式/模型/推理三要素（G-7）
3. 状态枚举 raw 英文直出（G-2）
4. i18n 破洞：英文 toast/混排（G-3）
5. Triage 缺「继续会话」闭环动作（J3）
6. 引导 4 步 → 2 步（J1）
7. 审批模式 slider 文案重叠（§3.10）

**🟡 P1 — 显著体验差距（Wave 2 主体）**
8. 视觉无层级：白扁平 → Liquid Glass 材质体系（§6.5）
9. 选中态/语义色收敛（G-4）
10. 侧栏状态徽章体系（G-6）
11. Usage/OPC 无入口（G-10）+ Usage 成本可观测升级（J4）
12. 消息流单列 max-width + 工具卡分级（§6.4）
13. Extensions → Connectors 目录化 + 安全徽章（§3.6）
14. OPC 枚举/配色/命名改造（§3.7）
15. Editor 并入会话文件面板（§3.8）
16. 导航状态 URL 持久化（G-9）

**🟢 P2 — 结构升级（Wave 3）**
17. 可拖拽多面板工作区（chat/diff/preview/terminal）按项目保存
18. Project/工作区概念（会话/任务/设置按项目分组）
19. 集成终端 + 预览自检闭环（dev server + 截图回传）
20. 多窗口（Codex 未满足需求，差异化机会）

---

## 6. 改进方案

### 6.1 术语与信息架构对齐表（Wave 1 核心）

| 现状（多轨） | 统一为（zh / en） | 对位竞品 | 路由不变 |
|---|---|---|---|
| 已排程 / 定时任务 | **任务 / Tasks**（一级） | Codex Tasks | /tasks |
| ——（Tasks 内子区） | **自动化 / Automations** | Codex Automations、Claude Routines | /tasks?tab=automations |
| ——（Tasks 内子区） | **目标 / Goals** | ZCode Goal Mode | /tasks?tab=goals |
| 分流队列 / 收件箱 | **收件箱 / Inbox** | Codex review queue | /triage |
| 并行方案 / best-of-N | **多方案对比 / Best-of-N** | Codex attempts | — |
| 扩展 | **连接 / Connectors** | Claude Connectors | /extensions |
| 单人公司（OPC） | **指挥台 / Mission Control** | Claude Code Mission Control | /opc |
| 聚焦聊天/评审/构建 | **对话 / Diff / 预览**（Chat/Diff/Preview） | Claude Code panes | — |
| 建议计划/自动编辑/完全自动（5 档 slider） | **4 档：逐项确认 / 自动编辑 / 计划模式 / 完全访问** | ZCode Shift+Tab 四档 | — |
| 系统设置 | **设置 / Settings**（唯一） | 全竞品 | /settings |

> 原则：**一个概念一个词，侧栏=页头=面包屑=文档四处一致**；路由/代码标识符不改（`/triage`、`/opc` 保留），只改显示层，成本一个 i18n PR。

### 6.2 导航与状态（Wave 1）

- 侧栏最终结构（简单模式）：

```
  [logo Shannon]
  [＋ 新对话]                       ← 主 CTA
  对话        Ctrl+1
  任务        Ctrl+2                ← 原「已排程」
  收件箱  ②                       ← 原「分流队列」，badge=待处理数
  连接                              ← 原「扩展」
  ─────────────────────────────
  资源
  记忆   用量  （badge: 今日 $0.42）
  ─────────────────────────────
  指挥台                            ← dev 模式显示
  简单/开发模式切换
  设置
  [模型 • 状态点]                    ← 常驻成本/连接状态
```

- 徽章规则：收件箱=待处理数（已有）；任务=失败/阻塞数（红点）；用量=日预算消耗 ≥80% 变琥珀色。
- 所有 tab/筛选入 URL query（对齐 Extensions 已有做法）。

### 6.3 自动化收件箱闭环（Wave 1，对位 Codex review queue）

1. 收件箱条目数据结构补 `session_id`（例行任务已有来源会话）。
2. 条目主按钮「**继续会话**」：跳转 `/chat?session=<id>&context=<triage_item_id>`，composer 预填「关于昨晚巡检结果：…」。
3. 条目次操作收进溢出菜单：归档 / 重跑 / 查看来源自动化。
4. 每条目显示来源链：`自动化名 › 运行 #42 › 9月10日 21:55`。

### 6.4 Chat 重设计（Wave 2 主体，对位 Claude Code + ZCode）

```
┌──────────┬────────────────────────────────────┬──────────────┐
│ 侧栏      │  对话标题          Diff  预览  ⌘K   │  右面板(可选)  │
│ (§6.2)   │ ┌──────────────────────────────┐   │  ┌────────┐ │
│          │ │ 单列消息流 max-w-3xl 居中      │   │  │Diff/   │ │
│          │ │ 用户: 右对齐轻底色胶囊         │   │  │Artifact│ │
│          │ │ 助手: 无边框流式文本           │   │  │/预览   │ │
│          │ │ 工具卡: 一行折叠(read)         │   │  │        │ │
│          │ │        高亮+确认(destructive) │   │  └────────┘ │
│          │ └──────────────────────────────┘   │              │
│          │ ╔════════════ composer 恒见 ══════╗ │              │
│          │ ║ [消息输入…                    ]║ │              │
│          │ ║ [逐项确认▾][claude-sonnet▾][标准▾] [附件][🎤][▶] │
│          │ ╚══════════════════════════════╝ │              │
└──────────┴────────────────────────────────────┴──────────────┘
```

- **composer 三要素 chip**：权限模式（4 档，对齐 ZCode）+ 模型 + 推理力度（轻/标准/最高，对齐 ZCode Thought Level）；本次会话记住选择。
- **内联 diff**：助手消息中的文件改动内嵌 mini-diff 卡（行级 + 展开全文件 + 「在 Diff 面板打开」）；这是 Claude Code/Codex 的核心语法，Shannon 的 diff 组件已存在（components/diff）只差接线。
- **右面板**：Diff / Artifact / 预览三态 tab；Artifact 沿用现有 ArtifactPanel；预览 Wave 3 接 dev server。
- 空会话态：居中 composer + 4 个建议任务卡（对齐 claude.ai 首屏）。

### 6.5 Liquid Glass 设计系统（Wave 2 核心，苹果玻璃风格规范）

> 原则：玻璃用于**承载内容的悬浮层**（侧栏、顶栏、composer、弹层），内容区保持实体，避免全面毛玻璃导致可读性与性能双输（Apple HIG: "Use Liquid Glass only for the navigation bar and tab bar" 精神）。**深色优先**（对齐 4 竞品），浅色同步定义。

#### 6.5.1 三级材质（Material Tiers）

```css
/* Tailwind v4 @theme 内注册；backdrop-filter 必须与半透明底色成对出现 */
:root {
  /* L0 base：窗口底——实体深色，不接受模糊 */
  --material-base: #0e0f13;

  /* L1 surface：侧栏/顶栏/composer——贴窗口边缘的第一层玻璃 */
  --material-surface: rgba(22, 24, 30, 0.55);      /* 深 */
  --material-surface-light: rgba(255,255,255,0.62); /* 浅 */
  --blur-surface: blur(28px) saturate(1.6);

  /* L2 overlay：菜单/弹窗/下拉——临时悬浮层，更透更亮 */
  --material-overlay: rgba(30, 33, 41, 0.42);
  --material-overlay-light: rgba(255,255,255,0.72);
  --blur-overlay: blur(44px) saturate(1.8);
}

.glass-surface {
  background: var(--material-surface);
  backdrop-filter: var(--blur-surface);
  -webkit-backdrop-filter: var(--blur-surface);
  border: 1px solid rgba(255,255,255,0.08);        /* hairline */
  box-shadow:
    inset 0 1px 0 rgba(255,255,255,0.06),          /* 顶部内高光——玻璃的"厚度" */
    0 8px 32px rgba(0,0,0,0.35);                   /* 环境投影 */
}
```

#### 6.5.2 层级与投影（Elevation）

| 级别 | 用途 | 阴影 | 圆角 |
|---|---|---|---|
| e0 | 内容卡片（实体，不用玻璃） | none–sm | 12px |
| e1 | 侧栏 / 顶栏 / composer | glass-surface | 16px（悬浮时） |
| e2 | 下拉 / popover / 命令面板 | glass-overlay | 14px |
| e3 | 模态 / 全局悬浮条 | glass-overlay + 大投影 | 20px |
| e4 | Toast / 临时气泡 | glass-overlay | 999px |

#### 6.5.3 色彩

- **主色**：保留品牌紫（violet-600 系），深色下调亮一档（violet-400）保证对比度；**废除蓝色选中态**（Usage 页 tab）、**废除棕色图表**（OPC），语义色只留：成功 emerald-500 / 警示 amber-500 / 危险 rose-500 / 信息 sky-400。
- **中性阶**：zinc 系 12 阶（文字 3 阶：primary/secondary/tertiary；表面 4 阶）。
- 图表色板：主紫 + 紫/青/橙/绿 4 阶辅助，同一页面不超过 5 色。

#### 6.5.4 字体与排版

- Inter Variable（已引入）+ JetBrains Mono（代码/diff/枚举值）；字号阶 12/13/15/17/20/24/30，行高 1.5/1.6；正文 15px 起（当前 13px 偏小）。
- 单列阅读宽 max-w-3xl（消息流、设置内容区、向导）。

#### 6.5.5 动效

- 统一 spring：`cubic-bezier(0.32, 0.72, 0, 1)`（Apple sheet 曲线），时长 200/280/360ms 三档；面板进出场用 transform+opacity（禁 transition-all）；按压态统一 `scale(0.98)`。
- 玻璃层进场：轻微 translateY(8px)→0 + opacity，模拟「玻璃贴上来」；禁止模糊度动画（性能杀手）。

#### 6.5.6 性能与降级（硬约束）

1. 同屏 `backdrop-filter` 元素 **≤4 个**（侧栏/顶栏/composer/当前浮层）；列表行、卡片一律实体色。
2. `will-change` 不写死；玻璃层加 `contain: paint`。
3. `@supports not (backdrop-filter: blur(1px))` 降级为 95% 不透明实体色——功能零损失。
4. 移动/低功耗档位（Tauri 窗口 <900px 宽）自动切换实体材质。
5. 验收性能预算：120fps 滚动、玻璃层遮挡区渲染耗时增加 <2ms/frame。

#### 6.5.7 与现有 token 管线的融合

- 现有 `generate:themes` 脚本 + MD3 token 全部保留；玻璃材质作为**新的 token 层**（`--material-*`、`--blur-*`、`--elevation-*`）注入 `@theme`，主题注册表每主题只需提供 base 色，材质公式全局统一——避免 12 套主题各配一套玻璃参数。
- 6 月审计的组件债在同一 PR 链清偿：Modal/ConfirmDialog/DropdownMenu/Badge/Tooltip 统一 primitive（玻璃 overlay 材质一次定义全站生效），Button 迁移消灭 7 种手搓主按钮。

### 6.6 页面级改造一览（before → after）

| 页面 | Wave 1（术语/状态） | Wave 2（视觉/组件） | Wave 3（结构） |
|---|---|---|---|
| Welcome | 4 步→2 步 | 玻璃 hero + 模板画廊 | — |
| Chat | composer 恒见；视图 tab 更名 | 单列流+工具卡分级+三要素 chip | 右面板可拖拽 + 内联 diff 完整版 |
| Tasks | 更名「任务」；7 控件→1 主 CTA；子区三段 | 卡片玻璃化；best-of-N 对比视图 | Goal 桌面入口完整化 |
| Triage | 更名「收件箱」；「继续会话」闭环 | 主按钮+溢出菜单 | 触发器矩阵（API/GitHub） |
| Usage | 侧栏入口 | KPI 玻璃卡；上下文拆解+缓存命中 | 会话预算上限 |
| Extensions | 更名「连接」 | 目录化+安全徽章 | — |
| OPC | 更名「指挥台」；枚举 i18n | 图表配色收敛 | dev 模式完整仪表盘 |
| Editor | — | 会话内文件面板 | 并入工作区 pane |
| Memory | — | 注入预览 | 记忆图谱 |
| Settings | 页头统一；审批改 4 档分段 | 左 tab + 搜索 | — |
| 全局 | 术语表落地；徽章体系 | Liquid Glass token 层 + primitives | 多窗口 |

---

## 7. 实施路线图

| Wave | 周期 | 内容 | 验收标准 |
|---|---|---|---|
| **1 快赢** | 1–2 周 | §6.1 术语表全量落地（i18n 文件）；枚举 i18n 映射；英文 toast 修复；Usage/指挥台导航入口；收件箱「继续会话」；审批模式 4 档分段控件；composer 恒见 + 三要素 chip | 术语审计脚本 0 命中（侧栏=页头=文档）；axe 无焦点环违规；全部截图重拍与本方案 after 图一致 |
| **2 设计系统** | 3–6 周 | §6.5 Liquid Glass token 层 + primitives（Modal/Menu/Badge/Card/Button）；深色优先重绘全部 18 页；色彩收敛；侧栏徽章；Chat 单列流+工具卡 | token 采用率 >95%（stylelint 规则拦截裸色值）；同屏 backdrop-filter ≤4；12 主题 × 深浅 2 模式走查通过；性能预算达标 |
| **3 结构升级** | 6–12 周 | 可拖拽面板工作区（chat/diff/preview/terminal，按项目保存）；Project 概念；预览自检闭环；多窗口 spike | 对位 Claude Code pane 语法；布局按项目持久化；预览截图回传 agent 自检可用 |

**度量**: 首日引导完成率、派活→首响应时长、收件箱「继续会话」点击率、settings→composer 模式切换迁移率、主题下滚动帧率。

---

## 8. 附录

### 8.1 截图索引

- Shannon 18 页：[screenshots/shannon/](./screenshots/shannon/)（01–18，§1.2 清单）
- ZCode Desktop 真机：[competitors/zcode-desktop-main.png](./screenshots/competitors/zcode-desktop-main.png)
- 官方/媒体：[competitors/](./screenshots/competitors/)（anthropic-claude-product-*、zcode-official-home-*、codex-*-hero、codex-cli-agents-dashboard）

### 8.2 主要引用来源

- Claude Code Desktop 官方文档（Chat/Cowork/Code 三 tab、pane 布局、diff review、browser 自检、checkpoints）：code.claude.com/docs/en/desktop（全文已存档）
- Claude 产品页：anthropic.com/claude
- Codex app 发布与能力：openai.com/index/introducing-the-codex-app/（经 WebSearch 交叉验证）；第三方实测：intuitionlabs.ai、kingy.ai、proflead.dev
- ZCode：真机 v3.11.2 + zcode.z.ai
- 功能层竞研与 Gap 编号（G1–G10）：docs/competitive-research-2026-09.md
- 历史审计基线：desktop/docs/product-review/05b-ui-design-audit-2026-06-26.md、05c-competitive-analysis-2026-06-26.md
