# Shannon Desktop v2 · 新版 UI 设计稿(Aurora Glass)

**日期**:2026-10-07 · **分支**:`design/ui-redesign-v2-mockups`(worktree `.worktrees/ui-redesign-v2`)
**入口**:打开 [index.html](./index.html) 逐页跳转;每页为独立静态 HTML(无 JS),设计视口 1440×900。
**快速翻阅**:[screenshots/](./screenshots/) 内有全部 14 页的预渲染 PNG(1440×900 @2x,由 `assets/shot.mjs` 生成,改稿后可重新生成)。
**共享设计系统**:[assets/shannon-ui.css](./assets/shannon-ui.css)(全部令牌与组件)+ [assets/sprite.html](./assets/sprite.html)(44 个内联 SVG 图标)。

---

## 0. 这轮设计稿解决什么

延续 [UI-IMPROVEMENT-PLAN-2026-09](../../design/ui-audit-2026-09/UI-IMPROVEMENT-PLAN-2026-09.md) 已确立的方向(深色优先 / Liquid Glass / 术语收敛)与 [R2 走查(2026-10-01)](../../research/2026-10-01-journey-competitive-review-r2.md) 的裁决(R4 诚实化、R5 季度取舍),把 **14 个界面全部重绘到成品级保真度**,作为后续 Wave 2/3 实施的视觉基准。

**对标竞品**:Codex Desktop(thread + review queue)· Claude Desktop / Claude Code(Connectors、pane 语法、暖色极简)· WorkBuddy(多智能体协作可视化)· ZCode(近黑三栏、composer 三要素、执行模式四档)· Hermes Desktop(状态栏常驻用量、Memory Graph 透明度)。

**不变的**:路由、信息架构连续性(现有功能一页不删)、Tauri 命令契约、12 套主题机制(设计稿是 `tokyo-night` 基准上的新令牌层,材质公式全局统一,各主题只换表面色)。

---

## 1. 设计语言:Aurora Glass

| 维度 | 决策 |
|---|---|
| 材质 | 三级:L0 实体底 `#0A0B10` → L1 玻璃(blur 28px,侧栏/顶栏/composer/右停靠)→ L2 玻璃(blur 44px,弹层/菜单)。内容区永远实体,同屏 backdrop-filter ≤ 4 |
| 签名 | **极光发丝线**:玻璃面板上缘 1px violet→cyan 渐变光,只出现在 composer、命令面板、激活卡、使命 hero——它是「信号从本机发向模型」的隐喻,全站唯一装饰 |
| 色彩 | 品牌紫 `#8B7CF7` 只做可交互信号;语义四色唯一:苔绿/琥珀/珊瑚/天青;图表五色上限;废除蓝色选中态与棕色图表 |
| 字体 | Inter Variable(界面)+ JetBrains Mono(数据/代码/diff/来源链/键位);中文回退 PingFang SC / Noto Sans SC;正文 13.5px、消息流 14px |
| 几何 | 8px 网格;圆角 8/12/16/18;阅读列宽 max 792px;侧栏 264px / 右停靠 328px |

---

## 2. 逐页说明(14 页)

> 每页给出:对位竞品 → 关键改动 → 实现备注(现有代码基础)。

### 01 欢迎引导 `pages/01-welcome.html`
- **对位**:ChatGPT/Claude 首启(2 步内见 agent 干活);ZCode 登录即用。
- **改动**:维持 2 步(任务+模型同屏 → 完成);首屏即 composer 形态输入框 + 4 张任务模板卡;检测到的本机密钥以「已就绪」徽章露出(BYOK 是核心差异,不再藏);隐私说明「密钥永不出机」常驻页脚。
- **实现**:现有 `Welcome.tsx` 2 步流程保留,重排布局;`AddProviderModal` 复用。

### 01b 欢迎 · 完成态 `pages/01b-welcome-done.html`
- **对位**:ChatGPT/Claude 首启第 2 步;真实实现为 `Welcome.tsx` DoneStep。
- **改动**:按任务模板逐项露出推荐工具(filesystem/git/playwright,模板驱动固定理由);「只推荐、启用在设置确认」原则不变(遵守裁决 4-B/B5-33)。
- **走查结论**:信息等价项 7 处判一致;实施 1 项(推荐工具逐项露出)。

### 02 对话 · 主工作台 `pages/02-chat.html`(质量基准页)
- **对位**:ZCode composer 三要素 + Claude Code 面板语法 + Codex 内联 diff。
- **改动**:composer 恒见并带**三要素**——执行模式四档分段(逐项确认/自动编辑/计划/完全访问)+ 模型 + 推理档;消息流单列 792px,用户右紫泡、助手无边框流;工具调用一行折叠(只读灰、高危琥珀+「允许一次/拒绝」);**内联 diff 卡**(改动直接在会话流里,可回滚);右停靠 Diff/预览/上下文/计划四 tab;底部状态条常驻:工作目录、分支、上下文水位、缓存命中、本会话花费、预算余量。
- **实现**:组件全部已有(`ComposerPanel`/`MessageBubble`/`components/diff`/`RightDock`),本页是重排+令牌化,不含新能力。

### 03 对话 · Diff 评审态 `pages/03-chat-review.html`
- **对位**:Claude Code diff pane(逐 hunk 键盘流)、Codex review。
- **改动**:composer 上方切换到评审工作台:文件列表逐 hunk 接受/拒绝,进度条 + 键位提示(j/k/a/r/u);检查点 chips(可回滚);右侧 dock 同步摘要。
- **实现**:`DiffViewer` 已支持 per-hunk 键盘操作,补全屏评审布局。

### 04 任务 · 自动化中心 `pages/04-tasks.html`
- **对位**:Codex Automations(单 CTA)、ZCode Goal Mode、Manus 结构化预览。
- **改动**:统一名「任务」;内部分 自动化/目标/后台任务/多方案对比/历史 五个子区;唯一主 CTA「新建」;**NL 创建的「结构化预览 → 激活确认」卡**(R2-P2-A 裁决项);失败例行以「已自动暂停 + 继续处理」前置(R2-P2-C);右列月历 + 目标进度环。
- **实现**:吸收现有 38 个 tasks 组件;`ScheduleForm` 增加 NL→结构化预览态。

### 04b 多方案对比视图 `pages/04b-best-of-n.html`
- **对位**:Codex attempts 并排评审;真实实现为 `BatchDiffCompare`(全屏 Modal)。
- **改动**:分支 chips 上浮 文件数·增删·成本;冲突指引露出 worktree 路径;对比结论卡仅写可判定事实(推荐标记/结论卡涉后端数据源,设计稿先行)。
- **走查结论**:一致 8 处(含时长字段:契约无此数据,设计稿未虚构);实施 2 项;待产品裁决 3 项(添加/跳过 chip、推荐标记+结论、导出)。

### 05 收件箱 `pages/05-inbox.html`
- **对位**:Codex review queue 最后一公里。
- **改动**:统一名「收件箱」;条目主按钮 = **继续会话**(带结果上下文回原 session);审批请求内置(批准/拒绝 + 风险级);失败条目自动暂停态 + 查看日志;来源链一律 `来源 › 运行号 · 时间`。
- **实现**:Triage 已有数据结构,补 `session_id` 深链与主按钮(§6.3 既定方案)。

### 06 用量与成本 `pages/06-usage.html`
- **对位**:Hermes 状态栏常驻用量、ZCode Usage、Notion 双阈值告警。
- **改动**:4 张 KPI 玻璃卡(今日/本月 vs 预算/缓存命中率/上下文峰值);每日成本柱图 + 按模型分布环;按会话明细表;**预算与封顶卡**:月度预算 + 会话级上限 + 触顶行为「自动暂停并询问」(R2-P2-D)。
- **实现**:`Usage.tsx` 数据源不变(`usage.jsonl`),重组布局。

### 07 连接 · 扩展目录 `pages/07-connectors.html`
- **对位**:Claude Connectors 目录。
- **改动**:统一名「连接」;精选目录化(9 卡示范)+ 分类 tab;**每卡安全徽章「注入扫描 ✓ 已签名 ✓」**——把 Shannon 独有安全治理变成可见卖点;remote/OAuth 形态给**琥珀虚线「桌面端即将支持」诚实徽章**(Ruling R4-B)。
- **实现**:Extensions 各子页保留为 tab;徽章数据来自安装器已有的扫描结果。

### 08 记忆 `pages/08-memory.html`
- **对位**:Hermes Memory Graph、Claude 项目记忆。
- **改动**:新增**注入预览横幅**(下一条消息将携带哪 3 条记忆,可旁路)——透明度卖点;条目带来源会话回跳与注入次数;「昨夜整理」卡展示 dream 合并结果。
- **实现**:溯源链路后端已有(R1 P2-5 半项),本页补 UI 呈现;引用回跳(R2-P2-B)预留位置。

### 09 指挥台 · 多智能体 `pages/09-mission-control.html`
- **对位**:Claude Code Mission Control 词汇;WorkBuddy 多智能体可视化;Shannon 独有资产。
- **改动**:更名「指挥台」进一级导航;使命 hero(进度/预算/截止)+ agent 集群行(状态点+负载)+ 五列看板(排队/进行/阻塞/待评审/完成),枚举全部中文;待评审列带「评审」直达。
- **实现**:`OPC.tsx` 重排;状态枚举 i18n 映射(旧 G-2 清偿)。

### 10 回放时间线 `pages/10-timeline.html`
- **对位**:无竞品等价物(Shannon 独有:事件溯源行车记录仪)。
- **改动**:执行瀑布(read/write/net/risk 四色)+ 累计成本/Tokens 双曲线 + **检查点 chips(/rewind)**;导出 HTML 保留。
- **实现**:`TurnTimeline.tsx` 已有瀑布与曲线,补检查点还原入口。

### 11 文件库 `pages/11-files.html`
- **改动**:网格 + 右侧预览面板;来源会话回跳;提取状态徽章(已提取 N 段);丢失文件琥珀虚线徽章。
- **实现**:`FilesPage.tsx` 重排;提取徽章复用 `AttachmentExtractionReport`。

### 12 设置 `pages/12-settings.html` + 6 张子页(S1–S6)
- **IA 重组 11 → 8**(分析与逐分区盘问见 [ADVERSARIAL-REVIEW.md §2](./ADVERSARIAL-REVIEW.md)):通用(+会话)/ 外观 / 模型 / 权限与安全(+审批默认档)/ 通知 / 连接(+远程执行+网络)/ 关于 / 高级(dev)。原「网络」「会话」「远程执行」三页因内容过薄或概念重叠被并入;路由保留做重定向。
- **模型页**(12-settings.html):服务商卡带密钥健康(上次验证时间/402 缺额提示)、参数滑杆、执行模式四档分段控件;安全说明常驻。
- **子页设计稿**:
  | 文件 | 分区 | 要点 |
  |---|---|---|
  | `12-settings-general.html` | 通用 | 语言/模式、启动行为、会话默认、数据导出 |
  | `12-settings-appearance.html` | 外观 | 12 主题网格、字号/密度、玻璃强度降级档 |
  | `12-settings-security.html` | 权限与安全 | 默认执行档、工具规则表、出站密钥扫描、沙箱档 |
  | `12-settings-notifications.html` | 通知 | 事件矩阵、Webhook +「测试用已保存配置」诚实 hint |
  | `12-settings-connections.html` | 连接 | 引擎/网关、手机配对审批、SSH/Docker、代理 |
  | `12-settings-advanced.html` | 关于与高级 | 版本/协议/更新、开发开关、功能 flag |
- **实现**:`Settings.tsx` 左轨重组;`approvalModes.ts` 四档已定(R3 裁决);`/settings/*` 旧子路由重定向。

### 13 伴随窗 · 快速捕获 `pages/13-companion.html`
- **改动**:356px 置顶小窗:捕获 → 只写主窗口草稿(不自动发送),目标会话可选,全局快捷键 ⌘⇧Space。
- **实现**:`CompanionPage.tsx` 既有契约(拉取式草稿桥)不变,重绘视觉。

### 14 命令面板(叠加态)`pages/14-command-palette.html`
- **改动**:⌘K 分组直达(页面/会话/模型/设置子页/操作);**设置 6 个子页全部可达**(R2-P1-9);底部键位提示。
- **实现**:`CommandPalette.tsx` 补 settings 分类项。

---

## 3. 全局横切改动

1. **术语一处一词**(侧栏=页头=面包屑=键位提示):任务/收件箱/连接/指挥台/多方案对比;状态枚举永不直出数据库值。
2. **侧栏状态徽章体系**:收件箱=待处理数(珊瑚);任务=失败/阻塞(琥珀点);用量=月预算消耗(≥80% 琥珀)。
3. **导航分区规则(裁决 F-6 = A′)**:侧栏导航区 7 项 —— 对话/文件/任务/收件箱/连接/记忆 + **指挥台(开发模式第 7 项,全站常驻)**;简单模式隐藏指挥台,任务页顶部提供入口卡。指挥台内部的任务容器以副标题「任务在这里并行执行」消歧(裁决 F-15 = 保留「任务」一词)。
4. **响应式降级(裁决 F-9;02c D3 二次修正后口径,R2 §7-C2 同句回改)**:<1360px 右停靠自动折叠(图标呼出);composer 控件**各宽度同一形态**——分段控件与 1200px 断点折叠已随 02c D3 移除,<1200px 仅 effort gauge 四点收缩为 icon+档位词。三要素恒可达,只变形态。
5. **诚实化模式**:不支持/未启用的能力用琥珀虚线徽章明说,禁止「看起来可用」的死入口(R2-P0 家族防复发)。
6. **性能硬约束**:同屏 backdrop-filter ≤ 4;**弹层打开时右停靠降级为实体色**(`.dock.solid`);玻璃层 `contain: paint`;`@supports not (backdrop-filter)` 降级 95% 实体色;12 套主题只换表面色,材质公式不动。
7. **可访问性**:文字三阶过 AA(t3 #7E839A ≈4.6:1,t3 仅用于 ≥12px);`:focus-visible` 品牌紫焦点环;reduced-motion 降级保留。

## 4. 审核清单建议

- [ ] 术语表是否认可(index.html 底部)
- [ ] composer 三要素的档位命名与排序
- [ ] 收件箱「继续会话」作为唯一主按钮是否符合预期
- [ ] 连接页安全徽章的露出强度
- [ ] 指挥台进一级导航(简单模式可见性)
- [ ] 注入预览横幅的默认开关状态
- [ ] 玻璃材质强度(当前 L1 透明度 0.60 / blur 28px)是否合口味

## 5. 走查裁决与设计稿勘误(2026-10-08)

> 来自 `docs/reviews/2026-10-08-design-parity-audit-r1.md` 走查(20 页对照)与三波整改的裁决记录。本稿是 spec,但以下 6 处以实现/既定裁决为准——**设计稿回改记入此处,后续重绘时按本节修正**。

1. **连接页 IA 保留「3 主 tab + 高级管理下拉」**:2026-09 IA X1/X4 裁决(内容过薄页并入)优先于本稿 07 页的 8 平铺 tab;对应地,设置→连接分区也是「引擎/网关 + 远程目标 + 网络」三组合并形态,不再回退平铺。
2. **remote/OAuth「桌面端即将支持」琥珀徽章已过时**:实现已完整支持 OAuth loopback(`Featured.tsx` 的 `installMcpOAuthLoopback`,5 个 OAuth 厂商一键授权)。设计稿 07 页相关徽章视为撤销;新稿应画真实连接状态,不是「即将支持」。
3. **04 页 NL 通知 chip 的「诚实置灰」是有意偏离**:R4 诚实化优先——规则引擎暂无 NL 通知的编译能力,置灰+说明优于可点但无效。后续引擎支持后再恢复可交互态。
4. **04b 对比 chips 为「多选并排评审」而非本稿的单选切换**:并排对照效率更高(与「不建议做」清单一致:不恢复每分支单选)。重绘 04b 时按多选并排形态画。
5. **composer 执行模式四档分段采纳,但两个引擎概念不能完全合一**:「权限档(严格/平衡/宽松)」(auto-approve 基线,`permission_profile`)与「审批梯(ask/auto-edit/full-auto)」(`approval_mode`)是两份配置;设置里模型页放执行模式入口卡、权限与安全放权限档,通用放审批模式,三处互链。完全合一只待统一命名设计(参考 2026-10-04 权限命名文档)。
6. **命令面板极光线 = 采纳**:静态稿未画,但实现为面板加了 aurora-line(与 composer「信号发出」隐喻一致,§3-B6 裁决);本稿 14 页重绘时补上。

---

## 6. 2026-10-14 回改记录(设计稿回改批次 D1-D8)

> 依据 [2026-10-14 设计保真度审查 R2](../../reviews/2026-10-14-design-parity-audit-r2.md) §3.4(「设计稿自身过时,裁决已定,HTML 未回改」清单)对本稿 `pages/*.html` 的回改。原则:**实现是事实标准,设计稿向实现对齐**;每处回改均在页内留有「按 2026-10-14 裁决回改」HTML 注记。

1. **D1 · 07-connectors**:8 平铺 tab →「3 主 tab(精选 / 已安装 / 待处理)+ 高级管理下拉(MCP 服务器 / 技能 / 智能体 / 数据源 / 插件)」,与实现 `Extensions.tsx`(primaryTabs + manageEntries,管理路由不变)一致;即本文件 §5-1 的落地。
2. **D2 · 07-connectors**:撤销 remote/OAuth「桌面端即将支持 · 请先使用 CLI」琥珀徽章,改画真实连接状态形态(已授权「已连接 · 管理」/ 未连接「去授权」主操作),对齐实现 `Featured.tsx` 的 OAuth loopback 一键授权(`installMcpOAuthLoopback`);即 §5-2 的撤销落地。
3. **D3 · 04-tasks**:五子区平铺 tab(自动化/目标/后台任务/多方案对比/历史)→ 主 tab「运行 / 历史」,目标 / 后台任务 / 多方案对比降为「运行」tab 内分区块(group-label 分区);页头「新建」补分裂按钮二级项(新建后台任务 / 多方案对比),对齐实现 `Tasks.tsx` + `TasksHeader.tsx` createCluster。
4. **D4 · 04b-best-of-n**:分支单选切换 →「多选并排评审」(chips 带 checkbox 语义、默认全选已完成分支、勾选分支并排成列、每列各自「采纳此份」;>3 列横向滚动),chips 上浮信息(文件数 · 增删 · 成本 · 推荐)保留,对齐实现 `BatchDiffCompare.tsx`;即 §5-4 的落地。
5. **D5 · 08-memory**:删除条目级「注入时机:…自动携带」行——引擎无触发器数据,维持缺失是诚实态;列表底部补设计注记「注入时机由引擎检索决定,不做人工触发器」。
6. **D6 · 10-timeline**:独立「检查点行」卡撤销,改为每轮卡内「还原到此轮」chip(与实现 `TurnTimeline.tsx` 一致);/rewind 语义、确认流守卫与「仅有检查点的轮次可还原」诚实门控注记保留在瀑布卡内。
7. **D7 · 02c-chat-composer-tidy**:权限档词汇全线对齐《权限命名设计 v2》(2026-10-04)——「逐项确认」→「询问」、「完全访问」→「全自动」;S4 弹层四档主词 + 副语同步实现 `settings.general.approvalMode.*`(只读自由执行,其余操作先询问 / 自动批准文件编辑 / 共享上下文,批准计划 / 批准除高危外的全部操作);effort S2 四档副语维持不变(实现已按此落地);页头裁决注记补「2026-10-14 R2:词汇按权限命名 v2 回改」。出处:R2 审查 §3.4-D7 + §4-G1。
8. **D8 · 07-connectors**:市场卡不再画「需认证 / 去认证」(市场卡无实时认证状态,`Featured.tsx` F-11 诚实契约);「去认证」移入新增的「已安装 · 管理行」注记区(已装行提供 重新认证 / 去认证,重新认证 = OAuth loopback 重放,对齐 `McpServers.tsx` W3-B),卡解剖注释同步。

**截图**:改动页面(07 / 04 / 04b / 08 / 10 / 02c)已用 `assets/shot.mjs` 重新生成同名 PNG 覆盖 `screenshots/`。
**§3 横切核对**:与「3 主 tab」相关的横切条目(§3-1 术语、§3-3 导航分区)经核对无矛盾,未改动。

### 6.1 2026-10-14 收口补记(D7 残留清账 + 两处顺带对齐)

> 首轮回改(上列 D1-D8)完成后的收口批次,依据同为 [R2 审查](../../reviews/2026-10-14-design-parity-audit-r2.md) §3.1-W3/§3.2-M3/M5 与 §4-G1,口径与 D7 完全一致。

1. **D7 残留清账(四页)**:权限档旧词全线清零——`02-chat.html`(composer 四档分段、高危工具行注释与说明句)、`03-chat-review.html`(composer 四档分段)、`12-settings.html`(执行模式卡)、`12-settings-security.html`(默认执行模式卡)的「逐项确认」→「询问」、「完全访问」→「全自动」;两处设置卡副语同步实现 `settings.general.approvalMode.*`(询问 = 只读自由执行,写入、网络外发等其余操作先询问)。各页均留「2026-10-14 R2:词汇按权限命名 v2 回改」注记。
2. **顺带① · 12-settings-general 会话默认卡对齐已落实现**:「默认工作目录」行升级为可写形态(input + 浏览… + 保存 + 生效说明,= `GeneralSettings.tsx` M3a);「记忆旁路默认」行按实现措辞更名「新会话默认开启临时会话(不写入记忆)」并同步帮助句(= `SessionSettings.tsx` M3b,`settings.session.memoryBypassDefault.*`)。
3. **顺带② · 06-usage 页头「导出 CSV」核对**:稿内已有(`06-usage.html:146`,与实现 `Usage.tsx` M5a / `usage.export.csv` 一致),未改动。

**截图(收口批)**:02-chat / 03-chat-review / 12-settings / 12-settings-security / 12-settings-general 已重新出图覆盖同名 PNG(06-usage 未改动故不出图)。

---

## 7. 2026-10-15 重绘收口批(设计稿向已落地实现对账)

> 依据 [视觉收敛 R4](../../reviews/2026-10-15-visual-convergence-r4.md) §RD:PR #411(视觉收敛批,2026-10-16 合入,CI 20 checks green)把 W1-W3/T1/T2 与历轮裁决落进了实现;本批把**已在实现里成立的形态**对账回 14 页设计稿——原则不变:**实现是事实标准,设计稿向实现对齐**。逐项对账表见 R4 §RD。

1. **02 / 03 · composer 02c D1-D5 折叠**:主对话页与评审页的 composer 从「权限四档分段 + 推理 chip + @ chip」旧形制,折叠为 02c 提案 ② 的收敛形态——`+` 菜单(附件/临时会话角标)、权限当前档 chip(下拉四档见 02c S4)、用量 38% chip(数字入弹框,02c S3)、模型 chip、effort 可点 gauge chip;`@` 引用经 placeholder 教学,不占常驻位。规格标注句按 02c D3「各宽度同一形态」口径回改(解除 R2 §7-C2 的 F-9 句式张力;<1200px 仅 gauge 四点收缩)。
2. **02 · 状态条 D5 瘦身**:移除 上下文 38% / 缓存命中 92% / 本会话 $0.087 / 预算余 $19.91 四段数字——02c 裁决 D5 有意移入 SessionUsageDialog(r4 W4 已登记「非缺口」),状态条仅留 目录 + 分支。
3. **02 · 用户气泡按 R3 §4 收口**(shannon-ui.css `.msg-user`):实体亮紫底 `#e9ddff` / 深紫字 `#23005c`(主题无关 fixed token),`rounded-2xl rounded-tr-none`(16px、右上直角),去描边——与 `MessageBubble.tsx:690` 一致;原「半透明紫底+描边、右下 4px」旧形制撤销。
4. **02 · 思考头分态补齐**:完成态「已思考 6 秒」维持(T1,时长可推导才显示,缺席 → 素「已思考」);进行中「正在整理升级注意事项…」→「思考中…」(`chat.streaming.thinking`,R3 §4 分态裁决)。
5. **02c · S3 用量弹框 T2 形态**:「窗口峰值 96k」→「会话峰值 9.6万 / 20万(48%)」(`chat.input.usage.sessionPeak.pct`,peak_used+窗口预算同在才渲染百分比,皆缺整行隐藏);预算行改「$0.087 / $19.91 预算余」+「设置会话预算…」入口(花费/预算余口径,W7)。
6. **06 · 上下文峰值 KPI 对齐 T2**:值 = 峰值已用 token(跨会话取最大,「7.6万」),副行 = 「{持有会话}({峰值/窗口}%)」;原「61% · 200k 窗口」旧口径撤销——绝不拿窗口当用量。
7. **11 · B1/B2 注记与文案**:双栏预览卡与提取徽章此前已在稿内;补「≥1280px 双栏(1fr + 320px 预览卡,格内操作回撤进卡)· <1280px 降级单栏」规格注记(`FilesPage.tsx:69` WIDE_QUERY);丢失徽章文案对齐 `office.files.missing`「原文件已移动或删除」。
8. **12-appearance · tokyo-night 色板紫化(W3)**:主题卡 swatch accent `#7aa2f7` → `#8b7cf7`(主色族已紫化,`themes.css:80`);`#7aa2f7` 仅存于 xterm ANSI 终端蓝,不代表主题 accent。
9. **13 · companion 文案微差收口(R3 §4 遗留)**:「发到主窗口」→「发送」;底部契约句 →「只写入主窗口草稿,不自动发送」(`companion.footer.hint`);置顶图标钮 →「窗口置顶」勾选项;原「# 会话」目标 chip 实现不存在,删除,改为行内状态「已发送到主窗口」(回执/错误/空 至多一条)。
10. **shannon-ui.css · 顺笔注记**:`.ambient` 加注 W1 已落地与浅色主题 50% alpha 属应用侧细节;`--mono`(JetBrains Mono)与品牌紫 `#8b7cf7` 核对不变。

**刻意不改**(登记不回改):03 主区整宽评审形态与「已丢失卡整卡虚线」稿内已画(app 侧仍挂账 R3 §4,见 R4 §RD);11 提取徽章按已落地形态只画在预览卡(格行不带徽章);05 stat 大卡 / 06 KPI 横纵解剖 / 10 瀑布结构 / 12-models 供应商卡等 r4 W4 已登记的呈现层差异维持设计稿原样;07 卡足安全话术措辞、安全白皮书按钮注记、06「每日成本」首屏口径三项 R3 顺笔项不在本批批准清单,留待下批。

**截图**:02-chat / 03-chat-review / 02c-chat-composer-tidy / 06-usage / 11-files / 12-settings-appearance / 13-companion 已用 `assets/shot.mjs`(1440×900 @2x,playwright 取自 desktop/ui)重新出图;其中 02c 的改动(S3 弹框)位于首屏折叠线以下,视口截图像素不变,文件字节一致故 git 无 diff。

---

## 8. 2026-10-15 收尾顺笔批(R3 §4 设计稿侧三项 + r4 §E.3 观察)

> 依据 [R3 §4「设计稿侧(下次重绘顺笔)」清单](../../reviews/2026-10-15-design-parity-r3.md)与 [r4 §RD.2 留待下批](../../reviews/2026-10-15-visual-convergence-r4.md)的挂账项;出站防线实时读态已由 PR #413 落地(r4 §E.0),本批把设计稿对齐已落地的诚实语义。原则不变:**实现是事实标准,设计稿向实现对齐**。

1. **07 · 卡足措辞诚实化**:8 张目录卡足「注入扫描 ✓ 已签名 ✓」→「安装时扫描 · 发布者自声明」(= `extensions.featured.securityBadge`,`Featured.tsx` SecurityBadge:图标 ok 色 + 正文 t2)——扫描是安装期对目录/README 的建议性静态扫描(securityBadgeScanHelp)、「已签名」是清单自声明非密码学校验(securityBadgePublisherHelp),静态双勾暗示运行时防护,双重失真。底部安全横幅文案同步对齐 `extensions.hub.securityBanner`「目录内所有条目安装前执行本机提示注入扫描;凭据只写入本机,永不出机。」(批次 2 诚实化已把「签名校验」从横幅撤下;凭据句对齐「密钥永不出机」叙事)。
2. **07 · 「去授权」文案**:Slack / Claude 远程连接两张市场卡主操作「去授权」→「添加」(= `Featured.tsx` F-11 诚实契约:后端无 connected/needs-auth API,市场卡不画认证态;未安装 OAuth 厂商只画「添加」——安装即授权,已装 → 「管理」);D2/D8 页内注记同步改写(F-11 表述由「添加/去授权」订正为「只画添加」)。去认证/重新认证仍只在「已安装 · 管理行」。
3. **07 · 安全白皮书按钮注记**:横幅按钮为设计先行元素,实现刻意不做——无白皮书可指(`Extensions.tsx` M1 注记 "intentionally absent — there is no whitepaper to point at");稿内保留按钮作安全叙事视觉锚,页内注记在案,白皮书产出后再接线(届时稿随落地同步)。
4. **06 · 「每日成本」首屏口径**:图表卡对齐已落地首屏形态——标题「每日 token · 近 7 天」+ 卡头 Token/成本切换段控(`usage.chart.mode.tokens/cost`;`Usage.tsx:335` 默认 tokens,柱状堆叠输入/输出);成本为切换口径(`by_day cost_usd` 单系列,切换后标题「每日成本」= `usage.chart.byDay.costTitle`);旧「每日成本」单口径标题撤销。
5. **02 · 侧栏「开发模式」mode-pill 移除**(r4 §E.3 观察顺笔):应用侧栏模型行无模式指示物(`Sidebar.tsx` 模型徽章 = 模型名 + 大写 provider),词汇亦非权限命名 v2;用量 meter(`SidebarUsageMeter`)与模型徽章均真实在位,保留。
6. **F-9 同族规格句补齐(R2 §7-C2 收尾)**:C2 点名的「README §3-4 同句」按 02c D3「各宽度同一形态」口径回改(§3-4)。页面侧逐一核实:F-9 两款行为在 app 侧仅 /chat 挂载(<1360px 右停靠折叠 = `RightDock.tsx:73` `F9_COLLAPSE_BREAKPOINT_PX`,仅 `Chat.tsx` 挂 RightDock;composer 折叠 = `ChatInput`,仅 /chat 使用)——02/03 已持规格句;01 的 hero 输入卡是独立简形提交(草稿转入 /chat),非 ChatInput;09/10 的稿内 dock 为登记的呈现差异(E.1),app 侧无 RightDock;11 已持 ≥1280 WIDE_QUERY 句。**无其余合资格页面,不画未落地行为。**

**刻意不改**(登记):02 之外 18 页侧栏同款「开发模式」mode-pill(03/04/04b/05/06/07/08/09/10/11/12×7)——R3/r4 记录仅点名 02,同族清批须整批重出截图,留待下批顺笔;07 卡 1「已连接」状态章(GitHub)属 F-11 同族呈现问题但无登记记录,不顺笔;11「已丢失卡整卡虚线」为 R3 §4.1 挂账的 app 侧待办、稿内已画目标形态(RD.1-8 / §7 已登记刻意不改),核实实现仍为灰态+opacity(FilesPage `data-missing` 行 opacity-50 + 琥珀虚线徽章),设计不动。12-settings-security「出站防线」卡稿仍画模式 seg/开关——与 r4 §S1 裁决 B 及 E.0 读态实现均不符,属新发现残留,超本批授权,登记待下批(需按 E.0 四态诚实渲染重画:模式徽章 + Findings 计数 + 「安装时校验 · 自声明语义」注记,无开关)。

**截图**:02-chat / 06-usage / 07-connectors 已用 `assets/shot.mjs`(1440×900 @2x)重出同名 PNG;未改动页一律不出图。

---

## 9. 2026-10-15 backlog 顺笔批(侧栏 mode-pill 同族清批 + 出站防线读态重绘 + GitHub「已连接」章撤除)

> 落实 §8「刻意不改」登记的三项挂账:同族 mode-pill 清批(r4 §E.3 观察登记)、07 GitHub「已连接」章(F-11 同族)、12-settings-security「出站防线」卡(r4 §S1 裁决 B + PR #413 §E.0 shipped 读态)。原则不变:**实现是事实标准,设计稿向实现对齐**。

1. **A2 · 侧栏「开发模式」mode-pill 同族清批(17 页 17 枚)**:`grep -ln "mode-pill"` 精确枚举后,陈旧 pill 实为 17 页——03 / 04 / 04b / 05 / 06 / 07 / 08 / 09 / 10 / 11 / 12-settings ×7(§8 记「18 页」系把 02-chat 计入;其匹配仅为 T2 批留下的页内注记文字,pill 已于该批移除;02c 两处匹配为 `approval-mode-pill`(composer 权限档 chip,testid/IPC 真实在位)文案,均不动)。逐页删除侧栏模型行 `<span class="mode-pill">开发模式</span>`,用量 meter(`SidebarUsageMeter`)与模型徽章真实在位照旧保留;rationale 注记落在首触页 03-chat-review(同 02-chat 先例,引 r4 §E.3)。
2. **A3 · 12-settings-security「出站防线」卡按 shipped 读态重绘(PR #413 / r4 §E.0)**:撤除全部虚构控件——「审计/改写/关闭」模式段控与两枚 switch(应用侧无任何模式 toggle,S1 裁决 B 刻意不做;E.0 刻意不做项同),改画只读读态行:出站密钥扫描 = 实心徽章「改写中 · 本次运行发现:4」、提示注入扫描 =「审计中 · 本次运行发现:2」(shannon-ui `.st.vio`;计数为 findings 非 blocked,自进程启动起累计),签名校验行 = 中性能力注记「安装时校验 · 自声明语义」(静态,非 pending 非实时),卡头加幽灵刷新钮(title「刷新防线状态」),卡首 desc 与卡尾 statusLive 脚注逐字镜像 `settings.permissions.outbound.desc` / `.statusLive`(zh-CN;auditHits 冒号按源串保留半角)。琥珀虚线「读取中…/状态未知」与「未启用」中性态为应用侧真实形态(PermissionsSettings.tsx `renderGuardBadge` 四态),静态帧取默认点亮快照(redact/4 + audit/2,同 E.0 mock 口径),在途/失败态不画入 still——由卡头注记登记。页内 rationale 注记在案。
3. **A4 · 07 GitHub「已连接」章撤除(F-11 对账)**:应用核实——市场卡唯一逐卡状态位 = 已安装与否:`Featured.tsx:43-46/314-321` F-11 注记「后端无 connected/needs-auth API,卡面从不声称 已连接/去认证」,未装 → 「添加」、已装 → 「管理」;`extensions.featured.connected`「已连接」仅作安装/连接成功后的 toast 文案,非卡面常驻章;数据源目录卡(`DataSources.tsx` AdapterCard)亦只画 已验证/查询即将上线,无连接态。裁定:**市场卡无连接态 → 撤章**(「管理」按钮保留 = 已安装态唯一合法展示,与 Notion/PostgreSQL 未装卡「添加」对照);卡上 D2「已授权态示范:已连接 · 管理」旧注一并改写(D2 形态已被 D8 + R3 §4 取代)。

**刻意不改**(登记):07 Slack / Claude 远程连接 两张市场卡「未连接」mute 章(另 code-reviewer「已安装」、本地 Ollama「未运行」章同属市场卡自绘状态,app 侧 Featured 卡无此物)——同 F-11 族(app 侧未装市场卡不画任何状态章),本批授权仅点名 GitHub 卡,留待下批顺笔;07「已安装 · 管理行」Slack 行状态章「已连接」——该区 app 侧真实存在状态章(`McpServers.tsx` statusBadge:`srv.connected` → `extensions.mcp.statusOnline`「在线」、needs_auth →「登录已过期」、已停用/不可达/服务器错误各态),稿内画状态章方向正确,仅措辞「已连接」与 shipped「在线」有差,超本批授权留待顺笔;其余未动。

**截图**:本批改动 17 页(§9-1 的 17 页含 07 与 12-settings-security)全部用 `assets/shot.mjs`(1440×900 @2x)重出同名 PNG;未改动页一律不出图。
