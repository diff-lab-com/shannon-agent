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
4. **响应式降级(裁决 F-9)**:<1360px 右停靠自动折叠;<1200px 执行模式四档折叠为「当前档 chip」。三要素恒可达,只变形态。
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
