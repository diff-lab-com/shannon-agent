# Shannon Desktop UI 视觉走查 + 桌面原生竞品深度对比(R3)

**日期**: 2026-10-07
**基线**: `dev` @ 79bbe1509(workspace v0.12.0),mock 模式(`VITE_MOCK_MODE=1`)实测
**方法**: ① 浏览器驱动视觉走查 14 个页面区、22 张截图 + a11y 树 + console 审计 + 5 处源码实证;② 3 个联网调研代理核实 Codex app / Claude Desktop(Code tab)/ Z.ai ZCode 的 UI/组件/user stories(全部带源 URL,画像见附件);③ 与 R1/R2(代码级 journey × web 竞品)互补:本轮 = **视觉级 × 桌面原生 App**。

**附件**(同目录):[Codex app 画像](2026-10-07-competitor-profile-codex-app.md) · [Claude Desktop Code tab 画像](2026-10-07-competitor-profile-claude-desktop-code.md) · [ZCode 画像](2026-10-07-competitor-profile-zcode.md)
**截图证据**:`~/.gstack/projects/diff-lab-com-shannon-agent/designs/design-audit-20261007/screenshots/`(22 张,下文按文件名引用)

---

## 0. TL;DR

1. **R2 的执行度极高**:三条 P0(remote MCP 断链 / 技能热注册错位 / cost 空壳列)经源码抽查**全部已修**;F5 keychain 迁移、welcome 首启「选起点」步、设置子导航(R1 #20)也已落地。§2 逐条给证据。
2. **视觉走查新发现 1 个 P1 + 5 类 P2**:settings/remotes 把整个设置壳打崩并裸渲染英文异常;**同一权限概念在四处有四套称呼**(执行模式/权限模式/审批模式/Strict·Balanced·Permissive);权限 popover 窄宽中文按字断行;加载态三套风格并存;mock 模式 7 个命令未 mock 导致进页即错误。
3. **桌面竞品格局**:Shannon 的**自动化面(例行+目标+best-of-N+triage)与成本治理领先三竞品**;token 管线(AA 契约 + CI 守卫)是工程级护城河。最大的产品级差距收敛为两项:**diff 行内评论→返工闭环**(Codex/Claude 的核心交互,三家都有、Shannon 没有)与**并行会话 + 新会话执行环境选择**(Claude 拖拽窗格 / Codex Local-Worktree-Cloud 三态;Shannon 引擎有 worktree/remote 能力但 composer 不暴露)。
4. **评分**:Design Score **B+**(功能面宽、词汇表与加载态拖后腿);AI Slop **A**(无模板感,是认真做过的产品 UI)。

---

## 1. 方法与范围

- **走查环境**: `desktop/ui` mock 模式(zh-CN locale,1440×900 为主,1024×700 窄宽抽查)。mock 拦截 257 个 Tauri 命令;模拟延迟使加载态可见。
- **页面区**: /chat(含 composer/a11y 树/权限菜单)、/welcome、/files、/tasks(自动化)、/triage、/usage、/opc(指挥台)、/memory、/extensions/{featured,mcp-servers,skills,agents,datasources}、/settings/{models,permissions,connections→网关,remotes,notifications,theme}。
- **已知局限**(阅读本报告时请记住):
  - mock 数据本身带来三类**非产品缺陷**的假象:未 mock 的命令报错(§3 V-09)、legacy 配置值裸显(权限 pill 显示 "standard",`ChatInput.tsx:950` 的诚实回退设计)、目录内容英文(marketplace 目录数据)——报告已逐一区分标注;
  - 单 locale(zh-CN)、单主题(Tokyo Night)、未连真实后端、未深测动效与键盘全局流;
  - 竞品事实来自公开文档/评测(2026-10 时点),非逐一实机操作;Claude Code Desktop 无独立安装包,以 Claude Desktop 内 Code tab 为对照物(判定依据见画像 §0)。

---

## 2. R2 → R3 修复状态抽查(源码实证)

| R2 项 | 结论 | 证据(2026-10-07 实测) |
|---|---|---|
| R2-P0-1 remote/OAuth MCP 死线 | ✅ **已修** | `desktop/src/mcp.rs:141`(seed 纯 remote 行走 `start_remote_server`)、`commands_mcp.rs:177`;mock 中 `linear-oauth`(https://mcp.linear.app/sse)显示「在线 · 3 个工具」(截图 09) |
| R2-P0-2 技能热注册错位 | ✅ **已修** | `skill_tools.rs` 提供 hot path wrapper(「registers chat tools for skills already hydrated…missing from state.tools」),多处调用 + 测试(`:289/:299`) |
| R2-P0-3 cost/token 空壳列 | ✅ **已修** | `scheduled_commands.rs:566`(`cost_usd: run.cost_usd`)流通 + 测试断言 `Some(0.25)`(`:2172/:2213/:2237`) |
| F5 数据源 keychain 迁移(批次2) | ✅ **已落** | `data_source_installers.rs`:keyring 优先、失败降级 owner-only 0600、读取优先 keyring、`credential_location` 字段区分 `keyring`/`plaintext_file` |
| R2 §4.1 首启「选起点」步缺失 | ✅ **已落** | /welcome 两步 stepper(任务 → 完成)+ 编程/写作/研究/通用四起点 + 隐私承诺文案(截图 19) |
| R1 #20 设置 IA 无子导航 | ✅ **已落** | 设置内左列子导航:通用/主题/模型/权限配置/网络/会话/通知/网关/远程目标/关于(截图 13b) |
| R2-P2-9 disabled MCP 渲染成 Offline | ✅ **已修** | mock 中 slack 显示「已停用」独立徽章,与「未连接」区分(截图 09) |
| R2-P2-14 内置权限档名称/描述英文硬编码 | ❌ **仍在** | 权限配置页三档卡标题仍为 Strict/Balanced/Permissive(截图 14b);顶栏同概念却是中文「严格/平衡/宽松」(截图 21)——见 §3 V-02 |
| R2-P2-B 记忆引用可回跳(chat 内) | ❌ **仍在**(与 R2 口径一致) | 记忆卡有「跳转会话」(memory→session 方向已通),chat 回答内仍零记忆信号 |

---

## 3. 本轮视觉走查发现

> 编号 R3-V-*。级别口径沿用 R1/R2:P1 = 断点/信任,P2 = 打磨,polish = 记录不排期。

### V-01 · P1 · settings/remotes 把整个设置壳打崩 + 裸渲染英文异常

直达 `/settings/remotes`,错误边界替换了**包括子导航在内的整个设置面板**,正文直接渲染 `Cannot read properties of undefined (reading 'length')`(截图 16)。mock 下 `list_remotes` 类命令返回未定义触发;即便如此也暴露两个真实问题:① 错误边界粒度在设置壳级而非子页级,一个子页数据失败殃及全部十个子页;② 裸异常字符串直出给用户(无友好文案 + 技术详情折叠)。
**建议**:错误边界下沉到子页(rotuer 级 ErrorBoundary per route);异常文案 i18n 化,原始 message 折叠为「技术详情」。

### V-02 · P1 · 同一权限概念,四处四套称呼

实测 + 源码核对(`src/lib/approvalModes.ts`、`ChatInput.tsx:950-960`、`zh-CN.json:2634-2641`):

| 位置 | 叫法 | 取值呈现 |
|---|---|---|
| 顶栏 pill | **执行模式** | 严格 / 平衡 / 宽松 / 自定义 +「切换在下一消息生效」(截图 21) |
| composer pill(a11y 标签) | **权限模式** | ask / auto-edit / full-auto 阶梯(legacy 值诚实回退裸显,截图 20) |
| 设置→通用 | **审批模式**(slider) | settings.general.approvalMode.* |
| 设置→权限配置 | **内置三档** | **Strict / Balanced / Permissive**(英文卡标题,截图 14b) |

竞品全部是**一个概念一个入口一套词**:Claude 一个 mode selector(Manual/Accept edits/Plan/Auto/Bypass,Cmd+Shift+M);Codex 沙箱+Approvals 双层但呈现单一;ZCode Shift+Tab 循环 4 档。Shannon 的 4+3 模型本身是刻意设计(`docs/plans/2026-10-04-permission-mode-naming-design.md`),但**呈现层词汇没收敛**:中文同义词漂移(执行/权限/审批)+ 英文第三套(Strict/Balanced/Permissive)+ 引擎 token 第三套(ask/auto-edit/full-auto)。
**建议**:定一张「概念→呈现词」映射表进 i18n 守卫脚本(check-design-tokens.mjs 已有术语守卫先例,顺手加一条);三档卡标题走前端 i18n 映射(即 R2-P2-14 的修法);顶栏「执行模式」改名「审批档」或与 composer pill 合一。

### V-03 · P2 · 权限菜单 popover 窄宽,CJK 按字断行

composer 权限菜单弹层宽度仅 ~130px,「只读自由执行,其余操作先询问」被断成「只读自由执 / 行,其余操作 / 先询问」三行,菜单项被裁切(截图 20)。顶栏同功能菜单宽度正常(截图 21)——两处弹层最小宽度不一致。
**建议**:弹层统一 `min-w`(≥260px)或按最长选项测量;CJK 禁用按字断行场景核(`break-words` 而非 `break-all`)。

### V-04 · P2 · 加载态三套风格并存

同一应用内:居中 spinner(tasks/usage/settings-models,截图 03/05/13)+ 纯文本「正在加载已安装技能…」(skills 页,与 spinner **同屏叠放**,且「已安装 · 0」计数在加载完成前先行渲染,截图 10)+ **完全空白**(permissions 子页空屏数秒,截图 14)。无骨架屏。
**建议**:定一个 `PageLoading` 组件(骨架形状对齐真实内容),设置子页与扩展各子页统一替换;计数徽章等数据到达后再渲染。

### V-05 · P2 · 工具错误三重冗余呈现

同一 bash 失败在一条助手消息里出现三遍:顶部「连续 2 次尝试失败」重试卡(第1次/第2次原文)+ 两张独立 bash 错误卡(各自完整重复 command+error)+ 第三张折叠卡(截图 01)。重试摘要与逐次明细信息重复,长会话里错误区占据一屏。
**建议**:重试卡与逐次卡合并(摘要卡展开即明细);同 command 连续失败折叠为一张卡 + 「重试 ×2」徽章。

### V-06 · P2 · usage 页图表与文案细节

① 柱状图柱体近黑色,深底上对比度不足,系列色未用 accent(截图 05b);② y 轴刻度混用「2万 / .1万 / 34.5 / 67.3」两种数制(同轴混排);③ 「缓存 Token」tile 副标签截断为「缓存读取 + 写…」;④ 预算区两条说明文案重复出现「80% 和 100% 提醒」。
**建议**:柱体系列色走 token(`--color-primary/series-1`);轴刻度单一格式(万或千分位,二选一);副标签缩短为「读取+写入」;两条文案合并为一条。

### V-07 · P2 · 扩展目录内容英文(marketplace/技能/智能体)

精选扩展卡描述("Search and edit your Notion workspace from Shannon.")、技能目录(brainstorm/tdd/systematic-debugging/frontend-design)、智能体目录(code-reviewer/executor/verifier)描述均英文,UI chrome 中文(截图 08/10b/11)。目录数据是静态 catalog,可译。
**建议**:catalog 条目加 `descriptionKey` 走 i18n(与 R2-P1-8 旧子树机翻同批);或至少给英文描述套 locale fallback 机制。

### V-08 · P2 · `status.idle` zh-CN 缺键(console 实证)

/opc 页 console 报 `MISSING_TRANSLATION: "status.idle" for locale "zh-CN"` ×4(`OPCAgentSwarm.tsx:219/277`),运行时回退英文 "idle"。i18n 键奇偶测试只保证 locale 文件间一致,不保证**代码引用的键存在**。
**建议**:把「代码中 `t()`/`formatMessage` 引用的键 ⊆ locale 文件」加进 `check:i18n`(AST 扫描或运行时开发警告升级);补键。

### V-09 · P2 · mock 模式 7 个命令未 mock,制造进页错误假象

`get_webhook_config` / `read_dream_state` / `list_subagents` / `list_installed_data_sources` / `list_dream_proposals` / `list_data_source_catalog` / `list_agent_authored_skills` 未在 `mock/handlers.ts` 注册 → /memory 进页弹「无法加载提案」toast、/extensions/skills 已安装区渲染失败空态、/extensions/datasources 两区全失败(截图 07/10b/12)。demo 是对外演示面,这直接影响演示完整度。
**建议**:mock handler 与 Tauri 命令注册表做一致性测试(命令清单 diff 进 CI),缺的补齐。

### V-10 · P2 · /files 顶栏标题不随页切换 + 行内元数据悬挂

/files 页顶栏标题仍是「对话」而内容区 h1 是「文件」(截图 02);其余页面顶栏标题均正确(自动化/收件箱/用量/指挥台/记忆/扩展/设置)。文件行「· 3 天 · 磁盘文件」行首出现悬挂分隔符(old-deck.md 无 size 时分隔符未收敛)。
**建议**:Files 页接 header.title 机制;元数据 join 空段时吞掉分隔符。

### V-11 · P2 · 「已安装」icon-only 行无标签

精选扩展页「已安装」一排 8 个裸 icon,无文字无 tooltip,可点击性与含义不明(截图 08)。
**建议**:icon + 短名(或至少 `title` tooltip + aria-label)。

### V-12 · P2 · 简单模式(默认)隐藏旗舰页面

`Sidebar.tsx:406-411`:`/usage` 与 `/opc`(指挥台)导航行仅 `mode === 'dev'` 渲染;默认简单模式下指挥台无侧栏入口(用量尚有预算条按钮可达)。指挥台是 R1/R2 重点投资面,默认不可发现(PCG: trunk test —— 看不到就等于不存在)。
**建议**:产品拍板:要么指挥台在简单模式保留一个入口(如「自动化」页内 tab),要么接受「简单模式=单会话」的明确定位并在欢迎卡/建议 chips 里给一次性引导。

### V-13 · P2 · 主题缩略图区分度弱

13 套主题卡的 mini 预览在深色主题间几乎同色(截图 18),选择器没有完成「一眼看出差异」的本职。
**建议**:缩略图直接用各主题真实 `--color-primary/--color-surface` 渲染(生成管线已有全部 token,`generate-themes` 可顺带产出缩略图色板)。

### polish(记录不排期)

favicon 404(dev server);顶栏最右「设置」入口用 person 图标(a11y 标签「打开设置」,图标语义错位,截图 01);composer placeholder 被工作目录文案占用(「正在 my-startup 中工作」),输入引导缺失,且与下方工作目录 chip 信息重复;状态栏品牌「Shannon Code」与侧栏 logo 重复。

### 亮点(对本轮发现的正面确认,回归时别丢)

- **token 管线是工程级护城河**:`generate-themes` 12 套主题全部过 **AA 4.5:1 对比度契约**(无候选即构建失败),`check-design-tokens.mjs` 在 CI 守术语/裸色值/动效时长三条纪律 —— 三家竞品均无公开可比机制。
- 会话行内嵌**目标运行指示**(flag + 「3/12」迭代进度,截图 01 侧栏),并行状态一眼可见,密度优于 Claude/Codex 的纯标题列表。
- 权限 legacy 值**诚实回退**设计(`approvalModes.ts`:无翻译的值 verbatim 渲染而非伪装成 Suggest)——「宁可裸也不说谎」的正确取向。
- 隐私承诺文案(「除非你要求,否则不会把你的代码发送到任何地方」)是四家中最直白的本地优先表态。
- triage 失败卡(红框 + 失败原因 + 来源回链 + 批量操作,截图 04)与 best-of-N 分支成本 chips(截图 03b)保持 R2「领先竞品」判定。

---

## 4. 三大桌面竞品画像(浓缩;全文见附件)

### 4.1 Codex app(OpenAI)

macOS(2026-02)+ Windows(2026-03),2026-07 并入 ChatGPT 桌面端三模式。三栏布局(项目 / 线程 / review);侧栏 Projects + Automations + Skills + **Triage 队列**;线程级 **Local / Worktree / Cloud** 三执行环境。核心强项 = **diff 审查环**:范围切「未提交/整分支/最后一轮」、hunk 级 stage/revert、**行内评论→agent 返工**、PR 评审侧栏。独有:computer use 虚拟光标(MacStories 评为最佳)、内嵌浏览器圈注、Appshots。弱点:仅 OpenAI 模型、GUI 相对 CLI 被判倒退、审批弹窗卡死 issues。

### 4.2 Claude Desktop · Code tab(Anthropic)

无独立安装包,Chat/Cowork/Code 三 tab 中的 Code;2026-04-14 大重构(多会话侧栏 + 拖放窗格 + 集成终端/编辑器 + Routines)。窗格全部可拖放/弹出(chat/diff/browser/terminal/editor/plan/tasks/iOS Simulator/side chat)。核心强项:**并排多会话**(Cmd+N、Cmd+点击双窗格、跨会话消息、Claude 代管会话)+ **diff 行内评论 Cmd+Enter 批量返工** + 首条消息前 Environment(Local/Cloud/SSH/WSL)/Folder/Model/Permission 四配置。权限一个 selector 五档(Cmd+Shift+M)。弱点:diff 仅 unified、无多窗口、入口深、并行会话 token 消耗 2-3x。

### 4.3 Z.ai ZCode

2026-07 发布、09-20 Apache 2.0 开源的 Electron ADE(macOS/Windows/Linux 三平台,唯一 Linux 正式支持者)。单 prompt box + 右侧可切终端/浏览器/侧聊/Goal 面板;任务分组 7 色 + 三视图。核心强项:**Goal Mode 证据化自验证**、**每条回复 Undo/Reapply**、Shift+Tab 4 执行模式、**手机扫码镜像 + 微信/飞书/Telegram Bot Channel**、Claude Code 会话迁移向导。弱点:被评「Codex 近复刻」、可靠性差(TUI 崩溃/Goal 验证器误判)、定价不透明、跨设备不同步。

---

## 5. User story 对照矩阵

✓ = 完整支持;◐ = 部分/间接;✗ = 无。Shannon 列为实测,竞品列依据附件画像(来源见各附件)。

| # | User story(作为开发者,我可以…) | Shannon | Codex | Claude | ZCode |
|---|---|---|---|---|---|
| U1 | 并行跑多个会话且互不踩(worktree/隔离) | ◐ 引擎有,桌面单窗格 | ✓ 线程+worktree/cloud | ✓ 多会话+拖放窗格 | ◐ 多任务+分组,单窗格 |
| U2 | 审查改动时给**行内评论**让 agent 返工 | ✗ | ✓ diff 行内评论 | ✓ 行内评论+批量 | ✗(无行内编辑器) |
| U3 | 按范围/hunk 挑选接受或回滚改动 | ◐ diff viewer,无 hunk 级 | ✓ 三级 stage/revert | ◐ 逐项批准(Manual) | ◐ 整回复级 Undo/Reapply |
| U4 | 定时自动化产出集中审批(triage) | ✓ 收件箱+批量+回链 | ✓ Triage 队列 | ◐ Routines 结果进会话 | ◐ Automations,无队列 |
| U5 | 同一任务多方案并跑对比择优 | ✓ **best-of-N 分支+成本对比**(独家) | ✗ | ✗ | ◐ fork 单点 |
| U6 | 目标式长任务自验证(轮数/停滞/预算) | ✓ 目标运行三指标 | ◐ 心跳式唤醒 | ✗ | ✓ Goal Mode 证据化 |
| U7 | 成本可见与预算治理 | ✓ 常驻条+双阈值+预估+审计表 | ◐ credits 模糊 | ◐ usage ring | ◐ 周额度+低谷半价 |
| U8 | 手机接管/批准桌面 agent | ◐ mobile+relay 已建,桌面呈现弱 | ✓ ChatGPT mobile 接管 | ✓ Dispatch+云会话 | ✓ 扫码镜像 |
| U9 | IM 渠道接入 | ✓ 网关 10+ 平台(最多) | ✗ | ✗ | ◐ 3 平台 |
| U10 | 记忆审查/旁路/溯源 | ✓ 梦境提炼审+临时会话+跳转 | ✗ | ◐ memory 工具 | ◐ 默认关不可浏览 |
| U11 | 扩展安装即用(MCP/技能/agent) | ✓ 三线闭环(R2 确认)+信任徽章 | ◐ Skills 商店 | ✓ connectors+plugins | ✓ Marketplace |
| U12 | 浏览器/桌面直接操作 | ◐ feature flag(computer-use)+MCP 浏览器 | ✓ 内嵌浏览器+computer use | ✓ browser 窗格+per-app 权限 | ✓ 内置浏览器 |
| U13 | 断点回滚(checkpoint/rewind) | ◐ 引擎有 /rewind,桌面无入口 | ◐ | ◐ CLI 能力,桌面未见 | ✓ 消息级 Edit+回滚文件 |
| U14 | 多语言 UI | ✓ 10 locale(旧子树 67% 残留) | ✗ 英文 | ◐ 少量 | ◐ 中/英 |

**读法**:Shannon 在 U4/U5/U6/U7/U9/U10/U11 七项上 ≥ 全部竞品(U5 独家);在 **U2/U3(diff 审查环)双项全竞品有而我无**、U1/U13 半缺。这就是 §6 改进 backlog 的排序依据。

---

## 6. 逐主题差距分析

### 6.1 diff 审查环 —— 最大产品级差距(P1,建议季度必做)

三家竞品都把「审查 agent 改动」做成了核心交互,且形态收敛:改动指示器 → diff 视图 → **行内反馈回流给 agent**。Codex 最全(范围切换 + hunk 级 + 行内评论 + PR 侧栏),Claude 最顺(`+12 -1` → 点行评论 → Cmd+Enter 批量),ZCode 用整回复级 Undo/Reapply 兜底。Shannon 桌面有 diff viewer 组件(`components/diff`)与「从此消息分支」按钮,但没有「看到改动 → 就地评论 → 定向返工」的闭环;权限弹窗批准的是「单次工具调用」,不是「一轮改动」。
**建议分两步**:①(S-M)会话内「本轮改动」入口:复用引擎 turn 边界,按轮聚合文件变更 + 文件级接受/拒绝 + 一键回滚(引擎 FileHistoryManager 已有快照);②(M-L,下季度)diff 行内评论 → 作为定向 steering 注入下一轮。注意与 R1 裁决「预览内圈选回修不做」的边界:那条裁的是**附件预览**圈注,不覆盖 diff 审查环。

### 6.2 并行会话与新会话环境选择(P1)

Claude 的答案是多会话侧栏 + 拖放窗格 + 跨会话协作;Codex 的答案是线程 + 每线程环境三态。Shannon 的特殊性:**引擎能力都在**(worktree、`shannon-remote` SSH/Docker、多会话 session store),桌面呈现没接 —— 新会话流没有环境选择,没有分屏。U1/U8 的「◐」全是呈现缺口而非能力缺口。
**建议**:新会话表单加「环境」三选(本地目录 / worktree / 远程目标——远程目标设置页已有数据);分屏可走轻量路线(会话 tabs + 双窗格只读对比)先验证需求,再谈拖拽布局。

### 6.3 模式词汇收敛(P1,纯 UI 工程)

见 §3 V-02。这张卡不做,权限档常驻 composer(R1 的胜果)的认知红利会被稀释:用户在顶栏看到「执行模式」、composer 看到「权限模式」、设置看到「审批模式」和英文三档,无法建立「这是同一件事」的心智。竞品全是一套词一个入口。

### 6.4 远程/手机/渠道:把已建的基础设施变成呈现(P2)

Shannon 的 gateway(10+ 平台)> ZCode Bot Channel(3 平台);shannon-relay(E2E 加密)+ shannon-mobile 已建成。但桌面端没有「设备对垒/接管」的呈现入口(竞品三家都有一眼可发现的手机接续入口:Codex 系统通知、Claude Dispatch 徽章、ZCode 扫码卡片)。**建议**:设置→网关或欢迎页给「手机接管」扫码卡(复用 relay 配对),成本 S。

### 6.5 其余差距(记录)

- **checkpoint/rewind 桌面化**(U13):引擎 `/rewind` 完整,桌面会话头加一个「回滚」入口即可(M);ZCode 的「消息级 Edit + 文件连带回滚」值得抄。
- **内嵌浏览器/computer use 面板**(U12):Shannon 是 feature flag + MCP 路线,暂不追;保留 flag 生态,等 Anthropic/OpenAI 把该交互定义稳定后再接。
- **云执行**(Codex Cloud/Claude 云会话):与本地优先定位冲突,不做;但「合盖继续跑」的用户价值可用既有后台任务 + 通知覆盖(例行/目标已支持),把这一点**讲出来**(onboarding/欢迎卡)比新建云基础设施划算。

---

## 7. 改进点 Backlog R3(供审核)

> 规模:S=≤1 天,M=2-5 天,L=1-2 周。P0=信任/断裂(随下个 hotfix),P1=对标差距(排期),P2=打磨(搭车)。

### 第一批 P0 —— 走查发现的修复级问题(合计 ~1 周内,全 S-M)

| # | 事项 | 修法 | 规模 |
|---|---|---|---|
| R3-P0-1 | settings/remotes 整壳崩溃 + 裸异常(§V-01) | 子页级 ErrorBoundary + 异常文案 i18n + 详情折叠 | M |
| R3-P0-2 | 权限菜单断行裁切(§V-03) | 弹层 min-width 统一 ≥260px | S |
| R3-P0-3 | `status.idle` 缺键 + 键存在性守卫(§V-08) | 补键;`check:i18n` 加「代码引用键 ⊆ locale」扫描 | S-M |
| R3-P0-4 | mock 缺口 7 命令(§V-09) | 补 handlers + 命令清单一致性测试 | S |
| R3-P0-5 | /files 顶栏标题 + 悬挂分隔符(§V-10) | 接 header.title;空段吞分隔符 | S |

### 第二批 P1 —— 对标差距(与 R2 遗留 P1 合并排期)

| # | 事项 | 来源竞品 | 说明 | 规模 |
|---|---|---|---|---|
| R3-P1-1 | 模式词汇收敛(§V-02/6.3) | 三家皆单一词汇 | 术语映射表进 i18n 守卫;三档卡 i18n(=R2-P2-14);顶栏/composer 命名合一 | M(需产品拍板词表) |
| R3-P1-2 | 「本轮改动」审查入口(§6.1①) | Codex/Claude | 按轮聚合 diff + 文件级接受/拒绝/回滚 | M-L |
| R3-P1-3 | 新会话环境选择(§6.2) | Codex/Claude | 新会话表单:本地/worktree/远程目标 | M |
| R3-P1-4 | checkpoint/rewind 桌面入口(§6.5) | Claude/ZCode | 会话头「回滚」入口接引擎 /rewind | M |
| R3-P1-5 | 手机接管扫码卡(§6.4) | ZCode/Codex | 网关设置或欢迎页配对入口(复用 relay) | S |
| R3-P1-6 | 加载态统一(§V-04) | — | PageLoading 骨架组件,替换三套风格 | M |
| R3-P1-7 | 指挥台简单模式可发现性(§V-12) | — | 产品拍板:入口 or 定位声明 | S(决策)+M(实施) |

### 第三批 P2 —— 打磨(搭车清)

工具错误卡合并(V-05)· usage 图表色/轴/文案(V-06)· 扩展目录 i18n(V-07,与 R2-P1-8 同批)· 已安装行标签(V-11)· 主题缩略图真实色(V-13)· person→gear 图标 · composer placeholder/工作目录 chip 去重 · favicon · 状态栏品牌去重。

### 不建议做(裁决建议)

- **不追拖拽窗格布局**(Claude):先用 tabs+双窗格验证并行需求;拖拽布局是 L 级投入且 Claude 用户也在抱怨「布局不保存」。
- **不做云执行、不做内嵌浏览器**:与 6.5 口径一致 —— 本地优先是定位不是缺陷。
- **不为 U2 行内评论跳过 6.1① 直接做 ②**:没有按轮聚合的改动视图,行内评论无处安放。

---

## 8. 评分(OPERATE 类 App UI 口径)

| 类别 | 等级 | 一句话依据 |
|---|---|---|
| 视觉层级 | A- | 密度控制好,会话行/卡片层级清楚;指挥台 hero 留白偏多 |
| 排版 | B+ | 中文 UI 字排版稳;目录/卡片英文混杂拉低(V-07) |
| 色彩与对比 | B | AA 契约是底牌,但 usage 柱状图近黑(V-06) |
| 间距与布局 | A- | token 化间距,窄宽 1024 无断版 |
| 交互状态 | B | hover/focus 完整;加载态三套 + 崩溃壳(V-01/04) |
| 响应式 | B+ | 桌面应用口径:1024 表现好,更窄未测 |
| 内容与文案 | B | 隐私承诺/审批诚实回退是亮点;预算文案重复、目录英文 |
| 动效 | n/a(未深测) | token 管线有时长纪律,风险低 |
| 性能感受 | B | mock 延迟下多页 spinner 可感;真实后端待测 |
| **AI Slop** | **A** | 无模板感:无渐变堆砌、无装饰卡阵、图标语义基本准确 |

**Design Score:B+**(较 R1/R2 时代的隐含评分持续上行;词汇收敛 + 加载态统一 + diff 审查环落地后可到 A-)。

---

## 9. 附录

### 9.1 截图索引(`~/.gstack/projects/diff-lab-com-shannon-agent/designs/design-audit-20261007/screenshots/`)

01-chat-first-impression · 02-files · 03/03b-tasks · 04-triage · 05/05b-usage · 06-opc · 07-memory · 08-ext-featured · 09-ext-mcp · 10/10b-ext-skills · 11-ext-agents · 12-ext-datasources · 13/13b-set-models · 14/14b-set-permissions · 15-set-connections · 16-set-remotes(崩溃) · 17-set-notifications · 18-set-theme · 19-welcome · 20-chat-permission-menu(断行) · 21-chat-execmode-menu · 22-chat-1024

### 9.2 console 审计汇总(全走查累计)

- `MISSING_TRANSLATION status.idle (zh-CN)` ×4(/opc)—— 唯一真实 i18n 缺键;
- mock 未注册命令报错 ×15(7 个命令,V-09);
- React unique-key 告警 ×1;favicon 404(dev-only);vite HMR 断连噪音(server 中途被杀重启,非应用问题)。

### 9.3 阅读顺序与方法谱系

R1(2026-10-01,代码级 journey × web 竞品,30 项)→ R2(2026-10-01,修复复审 + 残留,增 P0×3)→ **R3(本篇,视觉级 × 桌面原生竞品)**。三轮方法互补:R1/R2 回答「链路是否闭环」,R3 回答「看起来/用起来是否像 2026 年的桌面产品」。F 系列批次(R2 后续计划)与本篇 Backlog 的关系:F1-F6 是信任/安全收尾,R3-P1 系列是**呈现层对标**,两线可并行排期。

### 9.4 本轮局限(重申)

mock 模式(非真实后端)、zh-CN 单 locale、单主题、竞品事实来自公开资料而非逐台实机、动效未深测。所有「竞品有 X」的表述均可在附件画像中溯源;所有「Shannon 有/没有 X」的表述均可在截图或源码行号中溯源。
