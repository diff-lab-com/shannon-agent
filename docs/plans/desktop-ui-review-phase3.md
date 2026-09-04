# Desktop UI 产品与设计审查与三期改进方案(2026-08-26)

> 状态:**v1.1 · D1–D4 已拍板(2026-08-26,均采纳推荐 A);本文档为更新稿,待 ed 复核** · 审查基线:本地 dev @ 045d7fff · 前置:二期 R0–R11 + A2/B1/C1–C3/D1/E 已全部完成(docs/plans/desktop-ui-modernization-phase2.md)
> 审查角色:高级产品经理 + 高级 UI 设计师 + 普通用户,三视角分别给结论
> 结论先行:**确认存在"双会话列表并存"结构问题(P0-1)** —— 应用侧栏与 Chat 页各挂一条会话列表,同屏可见;另有 1 项 P0(头部/元信息三重冗余)、5 项 P1、7 项 P2、3 项 P3。排期任务 U1–U9,估 6.5–8 人日;**D1–D4 已于 2026-08-26 全部拍板(均采纳推荐方案 A)**,U1–U9 全部解锁,可依序开工。
> 本文档留在本地工作区,不进仓库(仓库惯例);任务合入后回写第 8 节状态表。

---

## 0. 审查方法与范围

- **静态走读**(行号以 dev @ 045d7fff 为准):App.tsx 路由树 · Layout/Sidebar/Header/Footer 壳层 · pages/Chat.tsx 与 pages/chat/* 全部子组件 · components/SessionsPanel · Welcome 引导流 · ChatInput/ComposerPanel · index.css token 层 · docs/plans/chat-upgrade.md(P2-5)
- **竞品参照**:Claude Desktop / Claude Code、ChatGPT Desktop、Cursor(threads 面板)、Cherry Studio(话题式双层)—— 基于公开产品形态的既有认知做交互模式对比,非逐像素评测
- **未覆盖**(留待后续):MessageArea/MessageBubble 渲染细节、Tasks/OPC/Extensions/Memory 各页深审、真机视觉走查(本次未启动 pnpm demo)、Lighthouse/对比度量化。**建议 U1/U2 动工前先跑 pnpm demo 截基线图**

### 0.1 决策定案记录(ed,2026-08-26)

| # | 议题 | 决策 | 落点 |
|---|------|------|------|
| D1 | 会话列表唯一的"家" | **A:应用侧栏** | U1 按此执行;SessionsPanel 保留为 P2-5b spike,不升级 |
| D2 | label 等宽字体 | **A:默认 Inter,等宽仅代码/数值场景** | U8 按此执行 |
| D3 | "始终允许"勾选框 | **A:删除控件 + 记录后端缺口** | U3 按此执行;allow 规则接入另立任务 |
| D4 | 新手种子 | **A:只做空态引导,不加种子数据** | U7 按此执行 |

## 1. 三视角结论

### 1.1 高级产品经理

- 产品骨架方向是对的:聊天主轴 + Tasks/Triage/Extensions/Memory/Usage 构成"AI 工作台",simple/dev 双模式是有意识的用户分层尝试,审批弹窗/本地语音/Extensions Hub 是竞品少有的差异化。
- 但**主轴(会话管理)存在三套并行实现**,说明"会话列表属于谁"(应用壳 vs Chat 页 vs P2-5b 线)这个产品决策从未收口 —— chat-upgrade.md:87 自己写着"SessionsPanel 目前组件存在但未挂载"。
- 元信息(模型/工作目录/用量)在 3 处重复出现,产品没有回答"用户在哪看模型、在哪改工作目录"这两个最基本的问题。
- 移动端(窄窗)只有抽屉里的那条列表可切换会话,桌面端却同时给两条 —— 同一功能两种设备体验互为矛盾。

### 1.2 高级 UI 设计师

- 设计系统底子好:MD3 token 完整、13 主题、type scale 六档、Base UI 原语迁移已完成(R1),这在一期/二期已经打牢。
- 但执行层**双风格并存**:应用侧栏会话行(圆角 pill、primary/10 激活底、右拖拽柄)与 Chat 页内会话行(border-l-2 卡片、surface-container-high 激活底、hover 工具条)是两套行样式、两套交互语言,同屏并排。
- label 全局用等宽 Geist(index.css:85-90)与 Inter 正文混排:拉丁字符下有"终端感"的品牌辨识度,但 zh-CN 下等宽对 CJK 无效,中英混排基线/字重不齐,双语产品观感不一致。
- 图标语义:置顶按钮未置顶态用 keep、已置顶用 push_pin(SessionSidebar.tsx:138);drag_indicator 每行常显(Sidebar.tsx:169-171);品牌图标 hub 含义弱。

### 1.3 普通用户(走查叙事)

- 打开 /chat:左边**两条"聊天记录"列表并排**,我不知道该看哪条;屏幕上同时有两个"新建"按钮、两个搜索框。
- 我想给对话改名:不知道要**双击**标题;想删除:不知道要**右键**(或按 Delete 键);置顶/导出/打印按钮要 hover 才出现,触屏无解。
- 刷新应用后,置顶全没了(pin 不持久),但拖拽的顺序还在 —— 行为不自洽。
- 权限弹窗里"始终允许"勾选框**点了没有任何效果**(死控件)。
- 头像图标点了没反应;底部状态栏和顶部信息大量重复,不知道哪边是权威。


---

## 2. 核心实锤:双会话列表并存(P0-1)

### 2.1 证据链

1. **Layout.tsx:66-68** —— 应用壳渲染全局 Sidebar(桌面常驻,宽 200–400px 可拖,默认 280px)。
2. **Sidebar.tsx:306-313 + 60-180(SessionsSection)** —— 应用侧栏内嵌第一条会话列表:搜索框、最多 8 条(SESSIONS_LIMIT)、拖拽排序(localStorage 持久,key shannon-sessions-order)、计数徽标。**没有**重命名/删除/置顶/导出。
3. **pages/Chat.tsx:205-228 + pages/chat/SessionSidebar.tsx:62** —— Chat 页又渲染第二条会话列表(220px 常驻,md+):搜索框(带后端全文检索,q≥3 走 searchSessions)、10 条/页分页(SESSIONS_PER_PAGE)、置顶/重命名(双击)/导出 Markdown/打印/删除(右键或 Delete 键)。**置顶是组件 useState,不持久**(Chat.tsx:73);**没有**拖拽排序。
4. **components/SessionsPanel/SessionsPanel.tsx** —— 第三套实现(P2-5b spike,C2 批已补测试,生产未挂载);标题叫 "Threads",w-56 侧板,行样式又是第三种(ghost Button + 消息数副行)。chat-upgrade.md:87:"目前组件存在但未挂载,避免 ChatV2Spike 双跑"。
5. **同屏叠加(桌面 ≥768px)**:/chat = 280px 应用侧栏(内含列表①)+ 220px 页内侧栏(列表②)+ 双层头部 → 1280px 屏上聊天正文仅剩约 760px。两个"新建对话"按钮、两个会话搜索框同屏。

### 2.2 三套列表能力矩阵

| 能力 | ① 应用侧栏 SessionsSection | ② Chat 页 SessionSidebar | ③ SessionsPanel(spike) |
|------|------|------|------|
| 搜索 | 标题过滤(客户端) | 标题+全文(后端,降级客户端) | 无 |
| 条目上限 | 8 条截断 | 10/页分页 | 全量滚动 |
| 切换/新建 | 有/有 | 有/有(第二个新建按钮) | 有/有 |
| 重命名 | 无 | 双击行内编辑 | 无 |
| 删除 | 无 | 右键/Delete → 确认弹窗 | 无 |
| 置顶 | 无 | 有(不持久) | 无 |
| 导出/打印 | 无 | hover 按钮 | 无 |
| 排序持久 | 有(拖拽+localStorage) | 无(仅置顶加权) | 无 |
| 行样式 | pill + primary/10 | border-l-2 卡片 + hover 工具条 | ghost Button |
| 键盘可操作 | 否(div onClick,无 tabIndex) | 是(role=button tabIndex=0) | 是(Button) |
| 移动端 | 抽屉内可用 | hidden md:flex(不可用) | 未挂载 |

**影响**:认知负担(哪条是权威列表)、空间挤压、功能碎片(两列表功能各缺一半,用户被迫两边找)、三套实现三套测试的维护成本、术语三套(Sessions / 会话 / Threads)。

**竞品校验**:Claude Desktop、ChatGPT Desktop、Cursor、Cherry Studio **没有一家**在聊天页同时挂两条会话列表;标准形态是"应用侧栏单列表 + 时间分组 + 行内 ⋯ 菜单"。

### 2.3 根因与既定计划的关系

P2-5b(chat-upgrade.md §3.2)规划的是 ThreadSidebar + ThreadTabs 的多线程形态,SessionsPanel 是它的 spike;但生产 Chat.tsx 的 SessionSidebar 是更早的手工实现,应用侧栏 SessionsSection 又是另一条线加的 —— 三线并行,无人收口。**U1 的本质是替这个产品决策收口,而不是单纯删一个组件。**

---

## 3. 竞品对比

| 维度 | Claude Desktop | ChatGPT Desktop | Cursor | **Shannon 现状** |
|------|------|------|------|------|
| 会话列表 | 侧栏单列表 | 侧栏单列表 + Projects | 右侧 threads 单面板 | **两条并排 + 一条未挂载** |
| 分组 | 今天/昨天/前 7 天 | 时间分组 + 项目 | 时间分组 | 无分组,截断 8 条或分页 |
| 行内操作 | hover ⋯ 菜单(改名/删/归档) | 行菜单(归档/分享/改名) | hover 重命名/删 | 双击改名、右键删除、hover 工具条(三套并存) |
| 新建入口 | 侧栏顶部 1 个 | 侧栏 1 个 | 面板 1 个 | **同屏 2 个** |
| 模型选择 | composer 内单一入口 | 顶部单一入口 | composer 内单一入口 | **3 处**(Header 下拉 / ChatInput Select / footer) |
| 工作目录 | 项目切换器 1 处 | — | 上下文选择器 1 处 | **3 处**(ChatHeader / composer footer / 列表行) |
| 会话标题 | 自动生成 + 可改 | 自动生成 + 可改 | 自动生成 | 手动/默认 untitled(未见自动摘要) |
| 快捷切换 | Cmd+K / Cmd+P | Cmd+K | Cmd+P | Ctrl+K 面板(有,但与会话列表脱钩) |

**Shannon 的相对优势**(不在本次问题清单,但定调用):审批工作流、Tasks/Triage/OPC、Extensions Hub、多 provider、本地语音、本地优先架构 —— 问题不是功能少,是**主轴信息架构没收口,差异化被壳层混乱拖累**。


---

## 4. 问题清单(按严重度)

| # | 级别 | 问题 | 证据(文件:行) | 任务 |
|---|------|------|----------------|------|
| P0-1 | 🔴 | 双会话列表并存(三套实现,两套同屏) | 见第 2 节 | U1 |
| P0-2 | 🔴 | /chat 双层头部 + 元信息三重冗余:模型 3 处、工作目录 3 处、footer 7 项 | Header.tsx:93/115-153 · ChatHeader.tsx:24/35-46 · ChatInput.tsx:102-113 · ComposerPanel.tsx:66-89 · Layout.tsx:75-120 | U2 |
| P1-1 | 🟡 | 会话行操作不可发现:重命名=双击、删除=右键/Delete、pin/export/print 仅 hover;触屏无解 | SessionSidebar.tsx:102-146 | U1 |
| P1-2 | 🟡 | pin 不持久(组件 state)vs 拖拽排序持久(localStorage),行为不自洽 | Chat.tsx:73 vs Sidebar.tsx:67 | U4 |
| P1-3 | 🟡 | 权限弹窗:"始终允许"是死控件(无状态、不提交);Deny 主色+autoFocus 而 Allow 中性,视觉主次与安全默认打架;medium 与 high 同色 | Header.tsx:210-213 / 194-199 / 215-219 | U3 |
| P1-4 | 🟡 | 应用侧栏会话行不可键盘操作(div onClick,无 tabIndex/onKeyDown),与页内行(role=button)双标 | Sidebar.tsx:143-176 | U1/U5 |
| P1-5 | 🟡 | 术语三套:sessionsPanel."Threads" / sidebar.sessions.* / chat.session.*(中英双语各自扩散) | i18n 键分布 | U4 |
| P2-1 | 🟢 | Header 头像为死图标(不可点、无账户语义);通知铃只管 skill 审批,与 Triage/系统通知关系不明 | Header.tsx:52-55/169-171 | U6/U9 |
| P2-2 | 🟢 | 导航平铺:Memory/Usage(资源类)与 Chat/Tasks(工作流)同级;settingsOpen 展开态不持久(每次 false) | Sidebar.tsx:186/315-412 | U6 |
| P2-3 | 🟢 | 会话截断口径三样:8 条上限 / 10 条每页 / footer 总数 | Sidebar.tsx:68 · constants.ts:3 · Layout.tsx:99-103 | U1 |
| P2-4 | 🟢 | 空态缺失:SessionsSection 仅在有会话时渲染,新用户该区域空白;seedSampleData 只种 Tasks 不种对话 | Sidebar.tsx:306 · Welcome.tsx:106-119 | U7 |
| P2-5 | 🟢 | 移动端 Chat 页无会话切换入口(页内列表 hidden md:flex);composer bottom-6/12 魔数与 footer h-8 硬耦合 | SessionSidebar.tsx:62 · ComposerPanel.tsx:46 · Layout.tsx:75 | U1/U5 |
| P2-6 | 🟢 | label 全局等宽 Geist:CJK 无等宽效果,中英混排基线/字重不齐 | index.css:85-90 | U8(D2) |
| P2-7 | 🟢 | 图标语义:keep/push_pin 混用、drag_indicator 常显、hub 品牌图标含义弱 | SessionSidebar.tsx:138 · Sidebar.tsx:169-171/275 | U5/U8 |
| P3-1 | ⚪ | 侧栏拖宽柄命中区仅 4px(w-1),不可键盘 resize | Sidebar.tsx:267-272 | U5 |
| P3-2 | ⚪ | 快捷键提示分散:侧栏 kbd 1/2、? 帮助浮层、Ctrl+K 面板三套入口无统一清单页 | Sidebar.tsx:320/325 | U6 |
| P3-3 | ⚪ | dark 变体硬编码 8 个主题名(custom-variant),新增主题要改 index.css | index.css:11 | U9 |

补充说明:
- P0-2 的"模型 3 处"中,Header 下拉与 ChatInput 的 Select 调用链不同(前者 api.configure({key:'model'}),后者连配 provider,ChatInput.tsx:102-113),收敛时以 ChatInput 的双写逻辑为准迁移到 Header,避免丢 provider 联动 —— 已写入 U2 约束。
- P1-3 的"Deny autoFocus"本身是**正确**的安全默认(回车=拒绝),问题只在视觉主色给了 Deny;U3 保留焦点行为,只调样式,并给 Allow 加"允许一次"的明确文案(已有)。
- 会话标题自动摘要(竞品标配)后端是否已支持未核实,listSessions 返回 title 字段来源待查 —— 列入 U7 的开放决策 D4 关联项。

---

## 5. 三期任务详案(U1–U9)

统一执行协议(沿用二期第 4 节,不再逐任务重复):
1. 分支 feat/desktop-ui-u&lt;N&gt;-&lt;slug&gt;,从 dev 切出,合回 dev 删分支,不留长命侧支
2. 每步验证(desktop/ui 下):pnpm lint && pnpm test --run && pnpm exec playwright test,全绿才算完
3. i18n 规则:en.json 与 zh-CN.json 同一提交、块内字母序;ESLint warnings 只降不升(现 cap 0);coverage 闸门 CI 已机器强制
4. 提交信息 conventional commits,尾部标任务号,如 refactor(desktop-ui): collapse session lists into app sidebar (U1)
5. 本文档本地不提交,任务合入后回写第 8 节

### U1 · 会话列表归一为单列表(P0,1–1.5 天,D1=A 已定)

**目标**:全应用只剩一条会话列表;同屏不再出现两个新建按钮/两个搜索框;排序+置顶+重命名+删除+导出+打印齐备于同一列表;移动端同一组件复用。

**内容**:
1. (D1=A 定案)Sidebar.SessionsSection 升级为唯一会话 rail:
   - 行结构改 button 语义(role/aria-current/tabIndex),修 P1-4
   - 行内 hover 显 ⋯ 菜单:重命名(行内编辑)/置顶/导出 Markdown/打印/删除(带确认,复用 DeleteSessionModal)
   - 搜索保留,并把 Chat.tsx:116-132 的后端全文检索逻辑(q≥3 走 searchSessions、降级客户端标题过滤)平移过来
   - 去掉 8 条上限,改整列滚动(nav 的 ScrollArea 结构调整为:会话区与导航区各自滚动,或会话区独立 flex-1)
   - 拖拽排序保留,pin 状态并入 localStorage(key shannon-sessions-pinned)
2. pages/Chat.tsx 删除 SessionSidebar 挂载及其专属状态(sessionSearch/backendSessionHits/pinnedIds/sessionPage/editingSessionId/editTitle/deleteTarget 等约 8 个 state + 22 个 props 通道);DeleteSessionModal 保留由侧栏触发
3. pages/chat/SessionSidebar.tsx 删除;HighlightText 迁移到侧栏搜索复用或删除
4. SessionsPanel spike(D1=A 定案):保留文件与测试,头注释加"未挂载,归 P2-5b ThreadSidebar 线";U1 不升级它
5. e2e 新增:侧栏切换会话/搜索过滤/删除确认流

**约束**:
- SessionContext(useSessions)API 零改动
- 侧栏 200–400px 可变宽内,⋯ 菜单不得溢出(翻转/收缩策略)
- 移动端抽屉复用同一组件;Chat.tsx 其余功能(Artifact/ContextPanel/QuickFix/Editor modal/DiffDialog)不动
- 相关既有测试迁移更新而非删除(Sidebar/Chat/DeleteSessionModal 用例)

**验证**:
- pnpm lint && pnpm test --run && pnpm exec playwright test 全绿
- grep -rn "SessionSidebar" src/ → 0 hits;grep -c "新建" 同屏按钮数=1(e2e 断言 getByRole('button', { name: 新建对话 }) 唯一)
- pnpm demo 人工:1280px 宽 /chat 仅一条列表;Tab 可达每行、Enter 切换;重命名/删除/导出可用;窄窗(<768px)抽屉内列表可切换;拖拽排序 + 刷新后顺序保持

### U2 · /chat 头部与元信息收敛(P0,1 天,可与 U1 并行)

**目标**:/chat 单层头部;模型选择全应用一处;工作目录一处;footer 只留运行状态与用量。

**内容**:
1. /chat 时全局 Header 显示会话标题(替代固定页标题"Chat");ChatHeader 退役,其 ContextPanel 开关迁入全局 Header 右侧按钮区
2. 删除 ChatInput 内 model Select;Header 模型下拉吸收 ChatInput 的双写逻辑(configure model + provider,ChatInput.tsx:102-113),避免联动丢失
3. 工作目录仅保留 composer footer 一处(ChatHeader 内按钮删除)
4. footer 精简:留 tokens/cost、活动任务数、版本;移除 provider/model(Header 已有)与 sessions 计数(U1 后列表自见)

**约束**:Header 的 TITLE_MAP 机制对其他页不变;ChatInput 的语音/附件/审批模式/QuickFix/Editor 入口不动;ContextPanel 组件本身不动,只迁移开关挂载点;usage 数据流(useChat)不变。

**验证**:三件套全绿;e2e 断言 /chat 头部 role=banner 唯一;pnpm demo 目检:模型名全屏仅 Header 一处可见文本,工作目录仅 composer footer 一处;ContextPanel 开合正常。


### U3 · 权限审批弹窗修缮(P1,0.5–1 天,D3=A 已定)

**目标**:弹窗内控件全部真实有效;视觉语义与"安全默认拒绝"一致;风险四档可辨。

**内容**:
1. 删除"始终允许"勾选框(Header.tsx:210-213)—— D3=A 定案,不依赖后端核实;顺手确认勾选框无其他消费方(现为无状态裸 input,预期零引用)。后端缺口记录:respondPermission(request_id, approved) 无审批 scope 概念,"记住允许"需 PermissionRuleChecker allow 规则接入,另立后续任务,不在三期范围
2. 样式:Deny 保留 autoFocus(回车=拒绝,安全默认不动)但改中性容器色;Allow("允许一次")用主色 primary
3. 风险分档配色:critical=error、high=secondary、medium=tertiary、low=tertiary(现 medium/high 同用 secondary,Header.tsx:194-199);risk 文案进 aria(读屏可闻)
4. 补 RTL 用例:四档配色渲染、Deny 回车路径、勾选框行为(或删除后无勾选框)

**约束**:不改 respondPermission 现有签名与调用链;不引入新权限语义;Modal 底层(R1b 成果)不动。

**验证**:pnpm test --run 新用例过;pnpm demo mock 触发权限弹窗目检四档;键盘走查 Enter=拒绝。

### U4 · Pin 持久化 + 术语统一(P1,0.5 天,依赖 U1)

**目标**:置顶与排序同为持久行为;用户可见的"会话"称谓全应用统一。

**内容**:
1. pin 集合写 localStorage(key shannon-sessions-pinned),加载时合并;排序 override 与 pin 的优先级明确(pin 优先,同 pin 内按 order)
2. 术语统一(D1=A 附带定案):用户可见文案统一「对话 / Chats」—— sidebar.sessions.*、chat.session.*、sessionsPanel.* 三组键合并语义;代码标识符(session/SessionInfo)不强改;i18n 双语同步改,删除无主键
3. 检查 zh-CN 文案中"会话/对话/线程"混用现状并统一

**约束**:仅文案层,不动数据模型;新增/删除 i18n 键按 MIGRATION.md 规范同提交。

**验证**:vitest 用例:置顶→卸载重挂载→仍置顶;grep -i "thread" src/i18n/ → 仅 sessionsPanel spike 相关(或 0);双语目检截图。

### U5 · 会话行 a11y 与交互细节(P1,0.5 天,依赖 U1)

**目标**:会话列表完整键盘可达;拖拽有键盘替代;细节命中区达标。

**内容**:
1. 行 = 原生 button + aria-current="page";Enter/Space 切换;⋯ 菜单键盘可达(方向键+Esc)
2. 拖拽排序键盘替代:行聚焦时 Alt+↑/↓ 调序(与鼠标拖拽共用同一 order 写入路径)
3. drag_indicator 改 hover/聚焦时显示(降视觉噪音)
4. 侧栏拖宽柄命中区 4px→≥8px(仍可视觉 4px,热区加宽),双击复位 280px
5. 触屏:长按行唤出 ⋯ 菜单

**约束**:不引入新依赖(菜单复用现有 dropdown/自研,Base UI 有 menu 原语则优先);jsdom 可测(键盘事件用例)。

**验证**:vitest 键盘用例(切换/调序/菜单);playwright + axe 无 critical 违规;手动键盘走查记录进 PR 描述。

### U6 · 导航信息架构重整(P2,1 天,独立)

**目标**:侧栏导航按心智分组;simple 模式对普通用户真正"简单";展开态持久。

**内容**:
1. 分组:「工作」Chat / Tasks / Triage(徽标),「资源」Memory / Usage / Extensions,「实验」OPC(badge);simple 模式默认只显「工作」+ Extensions 入口,资源组折叠
2. settingsOpen 及各分组展开态 localStorage 持久(key shannon-nav-open)
3. 通知铃 tooltip 说明去向(有 pending→审批弹窗;无→Triage);空态 title 指向 /triage
4. 快捷键提示收敛:侧栏只留 1/2 两个 kbd,其余快捷键统一进 ? 帮助浮层(已有 KeyboardShortcutsHelp,补全清单)
5. 头像处置:可点跳 /settings(U9 兜底)或移除(随 D2 无关,独立小决策,实现时按最小改动=可点跳设置)

**约束**:路由表零改动(只动侧栏呈现);simple/dev 模式语义与 SIDEBAR_MODE_KEY 不变;现有 e2e 导航选择器同步更新。

**验证**:三件套 + e2e 导航分组用例;pnpm demo 窄窗抽屉回归;simple/dev 切换后刷新,展开态保持。

### U7 · 空态与新手衔接(P2,0.5 天,独立)

**目标**:零会话、零任务、零用量三个首屏空态有引导而非空白。

**内容**:
1. 会话列表空态:引导卡(「开始第一个对话」+ 1–2 个示例 prompt 建议,复用 empty-state 原语);与 WelcomeState(欢迎卡)衔接去重
2. 核对 Tasks / Usage / Triage 空态是否统一用 empty-state.tsx / error-state.tsx / loading-state.tsx(R1c 已核对过原语本身,本次核调用面)
3. (D4)会话标题自动摘要:核实后端 listSessions 的 title 生成时机,若已有自动摘要则在新建行不再显示 untitled 占位;若无,列后端缺口不阻塞本任务

**约束**:不新增数据种子(D4=A 定案:seedSampleData 不动);空态文案双语。

**验证**:vitest 空态渲染用例;pnpm demo 新 profile(清 localStorage)目检首屏。

### U8 · 字体与图标规范(P2,0.5–1 天,D2=A 已定)

**目标**:label 字体策略符合双语现实;图标语义统一。

**内容**(D2=A 定案):
1. --font-label-sm/md 从 Geist 等宽改 Inter Variable;等宽仅保留给真正的代码/数值场景(WD 路径、模型 id、token 计数,用 font-mono 显式标注)
2. 图标修正:未置顶态 keep→push_pin(outline,已置顶 FILL);drag_indicator hover 显示(与 U5 重叠则归 U5);hub 品牌图标评估换更语义化图形(如 cognitive/blur_on/自定义 logo,实现时给 2–3 候选截图 ed 选)
3. 图标口径补进 desktop/CLAUDE.md 图标段(FILL 用法、语义化清单)

**约束**:tokens.css 是唯一改值处;不动 @theme 结构;改后 13 主题各切一遍目检。

**验证**:三件套;双语视觉走查(zh-CN 优先)截图进 PR;grep -n "keep" 图标名 → 0。

### U9 · 杂项与主题机制(P3,0.5 天,独立)

**目标**:清理机制性小债,主题系统可扩展。

**内容**:
1. dark 变体改登记制:styles/tokens.css 集中一个 data-theme→scheme 映射表(或给主题统一加 data-theme-mode 属性,由 ThemeSettings 写),index.css:11 的硬编码名单退役;新增主题只改登记表
2. Header 铃铛与 Triage 的关系统一(见 U6-3,若 U6 已做则本项只剩验证)
3. footer.agents 计数处置:并入活动任务指标或移除(运行中 agents 对普通用户无行动意义)
4. 遗留:ComposerPanel bottom-6/12 魔数改 CSS 变量联动 footer 高度(与 P2-5 收尾)

**验证**:三件套;13 主题切换 + 明暗断言 e2e;新增一个测试主题走登记表演练(验证可扩展性后删除)。


---

## 6. 排期与依赖(建议三周,共 6.5–8 人日)

| 周 | 任务 | 级别 | 工作量 | 依赖 |
|----|------|------|--------|------|
| W1 | U1 会话列表归一 | P0 | 1–1.5d | D1 ✅ 已定 |
| W1 | U2 头部/元信息收敛 | P0 | 1d | 无(与 U1 并行,合并冒烟) |
| W2 | U3 权限弹窗 | P1 | 0.5–1d | D3 ✅ 已定 |
| W2 | U4 pin 持久化+术语 | P1 | 0.5d | U1 |
| W2 | U5 会话行 a11y | P1 | 0.5d | U1 |
| W3 | U6 导航 IA | P2 | 1d | 无 |
| W3 | U7 空态/新手衔接 | P2 | 0.5d | 无(D4 ✅) |
| W3 | U8 字体/图标 | P2 | 0.5–1d | D2 ✅ 已定 |
| W3 | U9 杂项/主题机制 | P3 | 0.5d | 无 |

依赖图:U1 → U4 → (U5 可与 U4 并行);U2/U3/U6/U7/U8/U9 互相独立。
裁剪建议:W3 全部可延后不阻塞价值;W1 两项做完即可宣布"双列表问题清零"。

## 7. 与既有计划的关系

- **chat-upgrade.md(P2-5)**:U1 归一后,P2-5b 的 ThreadSidebar/ThreadTabs 仍可叠加(顶部 tab 切换与应用侧栏列表不冲突);SessionsPanel 保留为该线 spike。U2 移除 ChatInput 内 model Select 与 P2-5d 的"输入栏 composer 化"方向一致。
- **phase2(R0–R11)**:本方案不回退任何二期成果;Modal/SidePanel(Base UI 底层)、ESLint 棘轮、coverage 闸门、check-overlays.sh 等约束全部继续生效。
- **post-v0.10.0-cleanup-handoff.md**:无重叠(那边是 provider 命名空间清理)。

## 8. 状态表(执行时回写)

| 任务 | 状态 | 分支 | 合入 dev | 备注 |
|------|------|------|----------|------|
| U1 会话列表归一 | ✅ 完成 | feature/desktop-ui-u1-session-rail | 2026-08-26 a5b2f3aa | D1 ✅;lint/vitest 1361 绿/playwright 48 绿/coverage 86.76;grep SessionSidebar=0;pin 持久化已并入 U1(U4 仅剩术语) |
| U2 头部收敛 | ✅ 完成 | feature/desktop-ui-u2-header-meta | 2026-08-26 | lint/vitest 1363 绿/playwright 52 绿/coverage 86.83;ChatHeader 退役;panel 状态入 ChatContext |
| U3 权限弹窗 | ✅ 完成 | feature/desktop-ui-u3-perm-modal | 2026-08-26 | D3 ✅;四档配色+本地化 aria;checkbox 已删;后端 scope 缺口已记码内注释;vitest 1370 绿/playwright 52 绿 |
| U4 pin+术语 | ✅ 完成 | feature/desktop-ui-u4-pin-terms | 2026-08-26 d7e46ad4 | pin 持久化已随 U1 交付(Sidebar.test remount 用例);术语 en 31+zh 30 值统一为 对话/Chats,chat.session.aria 加前缀并同步 vitest/e2e 行名查询;lint 0/vitest 1370 绿/playwright 52 绿/coverage 86.84;grep thread=0,会话残留=0 |
| U5 a11y | ✅ 完成 | feature/desktop-ui-u5-row-a11y | 2026-08-26 ae1ddaa7 (commit c8318186) | 原生 button+aria-current;⋯ 菜单键盘可达;Alt+↑/↓ 与拖拽同写 order 覆盖;长按 500ms 开菜单;拖宽柄 8px 命中区+键盘 resize+双击复位;drag_indicator 悬停/聚焦才显;axe 0 critical/serious(修 kbd/徽章两处对比度);@axe-core/playwright 4.13.0 入 devDeps;lint 0/vitest 1381+3 绿/playwright 54 绿/coverage 86.87 |
| U6 导航 IA | ✅ 完成 | feature/desktop-ui-u6-nav-ia | 2026-08-26 2961b057 (commit 0157d056) | 工作组(Chat/任务/Triage 徽章)·资源组(记忆/用量/扩展)·实验组(OPC 扁平+实验徽章);simple 默认=工作开+资源折叠+Extensions 平铺;折叠态 localStorage shannon-nav-open(含 settingsOpen,模式切换重置为该模式默认);铃铛 tooltip 说明去向;侧栏只留 1/2 快捷键,? 浮层补全含 Alt+↑/↓;头像可点→/settings;nav.onePersonCompany 删除;路由零改动;lint 0/vitest 1389+3 绿/playwright 57 绿/coverage 86.91 | |
| U7 空态 | ✅ 完成 | feature/desktop-ui-u7-empty-states | 2026-08-26 46efb42a (commit 63d1934c) | 零会话引导卡(开始第一个对话+2 示例,复用 EmptyState 新增 suggestions 原语);示例文案抽 welcomeExamples.ts 与 WelcomeState 共源去重(侧栏 2/画布 4);点示例=建首会话+/chat state prefill 复用 Editor 通道;Usage BucketTable 空态上 EmptyState 原语;Tasks/Triage 原本已用;后端缺口:listSessions 无标题自动摘要(new_session 标题=Session {uuid前缀},commands_sessions.rs:30),untitled 兜底保留,自动摘要列后端待办;lint 0/vitest 1392+3 绿/playwright 57 绿/coverage 86.94 |
| U8 字体/图标 | ✅ 完成 | feature/desktop-ui-u8-fonts-icons | 2026-08-26 12818b6d (commit 0263e4f6) | --font-label-sm/md Geist→Inter(index.css @theme 为实际值源,tokens.css 同步参考清单;计划约束写的 tokens.css 唯一改值处与现实不符,已按注释意图单点改值);font-mono 显式标注:Header 模型 pill+菜单名/Usage StatCard 值(表格数值列与 WD 路径本已 mono);Geist import+vite-env 声明+依赖全删;keep 图标 0 处(U5 已清);置顶行 push_pin FILL、菜单 outline;品牌 hub→cognitive(FILL),候选 blur_on/neurology 留码内注释待 ed 定;FILL 口径+语义清单进 desktop/CLAUDE.md;附带 U7 补漏:Usage 页级空态上 EmptyState 原语;13 主题临时 e2e 探针验证 label 计算字体=Inter Variable 无 mono 回退(zh-CN),截图目检无豆腐块基线齐;lint 0/vitest 1393+3 绿/playwright 57 绿/coverage 86.94 |
| U9 杂项/主题 | ✅ 完成 | feature/desktop-ui-u9-theme-registry | 2026-08-26 db6de0ae (commit 3f568cb7) | dark 变体登记制:ThemeContext THEME_SCHEMES 映射表+<html data-theme-mode>,index.css @custom-variant 只认该属性,8 主题硬编码名单退役(计划原案 tokens.css 放映射表不可执行,采用原案 B:data-theme-mode 属性,tokens.css 头注释记录登记处);临时 test-probe 主题走登记表演练通过后删除;footer.agents 计数移除(数的是 agent 定义非运行中,无行动意义;en+zh 删键);--spacing-footer 32px 单源,main pb-footer+ComposerPanel bottom calc 联动(现值不变);铃铛/Triage U6 已做仅验证;新 e2e themes.spec 12 主题属性断言+明暗 body 背景变化断言;lint 0/vitest 1395+3 绿/playwright 59 绿/coverage 86.96 |
| 追办 图标定案+自动标题 | ✅ 完成 | feature/session-auto-title | 2026-08-26 be22cc96 (commit a2499e69) | ed 拍板(2026-08-26):(1) 品牌图标 cognitive 定案,码内注释记"Confirmed final 2026-08-26; alternates closed"(blur_on/neurology 关闭);(2) 会话标题 Tier-1 自动摘要落地 —— send_message 在推入首条用户消息前捕获 buffer 空=首条,auto_title_from_first_message 仅当标题仍是 "Session " 占位符时改写(用户改名永不覆盖),derive_title_from_message 取首行+按字符 50 截断加 …(CJK 安全),镜像 rename_session 持久化路径+发 SESSIONS_UPDATED(前端已有监听刷新侧栏,零前端改动);save_session 元数据合并语义保证后续 title:None 自动保存不冲掉标题;6 个内联单测;Tier-2 LLM 摘要按约定缓议;门禁:cargo nextest -p shannon-desktop 586 绿(含 6 新)/clippy -D warnings 0/fmt 0/UI lint 0(仅注释改动) |

## 9. 决策记录(D1–D4 已全部拍板,2026-08-26)

**ed 拍板结果:四项均采纳推荐方案 A。**下表保留原选项分析供追溯,"推荐"列即定案结论。

| # | 议题 | 选项 | 推荐 |
|---|------|------|------|
| D1 | 会话列表唯一的"家"在哪 | A. 应用侧栏(对齐 Claude/ChatGPT,移动端抽屉天然复用) · B. Chat 页内侧栏(应用侧栏只留导航,对齐 Cursor 面板形态) · C. 升级 SessionsPanel 为准(P2-5b 提前) | **A(已定)** —— 改动最小、心智最常见、P2-5b 可叠加;B 需把侧栏导航单独撑起一列,双栏 chrome 更重;C 把未验证的 spike 提前转正,风险大 |
| D2 | label 等宽字体(Geist) | A. 默认 Inter,等宽仅代码/数值 · B. 保留为品牌特征(接受 CJK 不等宽) | **A(已定)** —— 双语产品的排版一致性优先;终端感可以用局部等宽保留 |
| D3 | "始终允许"勾选框 | A. 删控件+记录后端缺口 · B. 接线(需后端支持审批 scope) | **A(已定)** —— 死控件比没控件更伤信任;后端 allow 规则接入另立任务 |
| D4 | 新手种子 | A. 只做空态引导不加种子 · B. seedSampleData 增一条示例对话 | **A(已定)** —— 先验证引导转化,种子对话容易变成"删不掉的垃圾数据" |

## 10. 审查后记

- 本审查为代码级静态走查 + 既有认知的竞品模式对比;U1/U2 动工前务必 pnpm demo 截基线图(明/暗 × 中/英 四象限),完工后同象限对比,作为验收证据之一。
- 行号基于 dev @ 045d7fff,后续漂移以符号名为准(SessionsSection / SessionSidebar / SessionsPanel / TITLE_MAP)。
- 会话标题自动摘要的后端现状(P2-7 关联)未核实,列入 U7 内容 3。→ 已核实并补齐:后端原无自动摘要,追办交付 Tier-1 首条用户消息截断(见 §8 追办行);Tier-2 LLM 摘要缓议。
- 三套列表的测试面:Sidebar 用例、Chat 页用例、SessionsPanel 用例(C2)在 U1/U4 落地后需要一次集中清理,防止死测试残留。

## 修订记录

- v1.1(2026-08-26):D1–D4 拍板回写(均采纳推荐 A)。新增 0.1 决策定案记录;U1/U3/U7/U8 从"前置决策"改为"已定"并落定具体实施路径;第 6/8/9 节同步(状态全部解锁、决策表定案);文末加修订记录。内容性改动仅此,问题清单与任务范围未变。
- v1.2(2026-08-26):U1–U9 全部完成后,ed 拍板两项待办:品牌图标 cognitive 定案 + 会话标题 Tier-1 自动摘要实施。§8 增"追办"行(commit a2499e69 / merge be22cc96),§10 后记同步。