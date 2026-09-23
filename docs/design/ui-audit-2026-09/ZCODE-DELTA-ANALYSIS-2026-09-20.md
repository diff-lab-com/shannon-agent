# ZCode 新截图对比分析 · 第二轮（2026-09-20 三图）

> **证据**（原件留存 `reference/zcode/`，归档于 [screenshots/competitors/](./screenshots/competitors/)）：
> - [zcode-desktop-sidebar-groups.png](./screenshots/competitors/zcode-desktop-sidebar-groups.png)（2026-09-20 23:58，左栏项目分组特写）
> - [zcode-desktop-marketplace.png](./screenshots/competitors/zcode-desktop-marketplace.png)（2026-09-20 06:37，插件市场页）
> - [zcode-desktop-doc-viewer.png](./screenshots/competitors/zcode-desktop-doc-viewer.png)（2026-09-20 06:38，三栏完整形态：对话 + 文档展示区）
> - （2026-09-18 计划 dock 图已由 [ZCODE-DELTA-ANALYSIS-2026-09.md](./ZCODE-DELTA-ANALYSIS-2026-09.md) 覆盖，其 P0–P1 建议①–⑦已在 PR #89 落地，本文不重复。）
> **性质**：分析 + 建议，**不含已实施的代码改动**；方案供评审后排期。
> **关联**：[ZCODE-DELTA-ANALYSIS-2026-09.md](./ZCODE-DELTA-ANALYSIS-2026-09.md)（第一轮）· [UI-IMPROVEMENT-PLAN-2026-09.md](./UI-IMPROVEMENT-PLAN-2026-09.md) · 代码现状以 `dev@1c093dfe` 为准（子代理逐文件核实，锚点均为文件:行号）。

---

## 0. TL;DR

1. 三张新截图相对第一轮的**最大增量**：① 左栏「项目」分组成为默认心智且**定时/自动化任务与普通会话混排进项目树**；② **插件市场独立大页**（搜索 + 已安装图标行 + 公开/个人 + 分类两列网格）；③ 右侧展示区从「计划文档」进化为**完整文档阅读器**（面包屑 + 元数据块 + 可点击目录 TOC + 表格工具栏）。
2. Shannon 经 PR #89 后，第一轮差距（侧栏遥测 / 分组双视图 / 计划 dock / composer 模型芯片 / dock tab 化 / 工具卡遥测 / 子智能体块）**骨架已对齐**；本轮差距集中在**「文档阅读体验」「市场页形态」「运行语义完整性（等待审批/出错/相对时间）」**三个面，多为「最后一公里」打磨而非结构重构。
3. 另发现 3 个**现存缺陷**可直接快修：中文 composer 占位文案残缺（「正在 — 」）、6 个死 i18n 键、Artifact 类型标签硬编码英文在中文 UI 漏出。
4. 审美仍不建议照搬（近黑扁平 vs Liquid Glass）；吸收的是**信息组织与外显策略**，全部可在现有 M3 token + 12 主题管线上实现。

---

## 1. 截图学习总结（参考设计提炼）

### 1.1 左侧导航栏（特写图）

| 设计点 | 截图证据 | 设计意图 |
|---|---|---|
| **顶部动作区** | 新建任务（Ctrl+N）/ 搜索（Ctrl+K）/ 自动化 / 插件市场，图标+小字标签+**快捷键提示** | 高频动作置顶、零学习成本（快捷键就地可见） |
| **双组织心智切换** | 「# 分组 \| 📁 项目」segmented，分组侧带 ✨（AI 智能分组）图标 | 「自动归类」与「项目归属」两种心智并存，一键切换 |
| **项目=第一公民** | folder 图标 + 项目名作分组头，任务缩进嵌套其下；无任务项目显示「暂无任务」 | 项目是持久容器（可为空），不是会话的派生属性 |
| **行内运行语义** | 运行中=旋转 spinner / 活跃=绿点；**定时任务=时钟图标**（如「每30分钟巡检…」）混排在其项目树下；右缘**相对时间徽章**（刚刚/2分/17小时/12天） | 侧栏即运行监控面板：哪个在跑、哪个是定时、各自多久之前 |
| **状态徽章** | 「即将」徽章（即将执行的自动化） | 未来态可见 |
| **底部账号区** | 头像 + 用户名 + 套餐徽章（Max）+ 设置/快捷键入口 | 账号与套餐就地可达 |
| **密度与溢出** | 11–13px 小字、紧凑行高、单色阶、选中项微底色；长标题省略号 | 高密度扫读 |

### 1.2 中间对话区（文档查看器图 + 计划 dock 图）

| 设计点 | 截图证据 | 设计意图 |
|---|---|---|
| **多会话 tab** | 顶部 tab 栏：多会话并行 + 「+」新建 + 关闭 × | 会话如浏览器 tab 并行切换 |
| **执行日志式转录** | 可折叠工具块（检查/确认/阅读/更新/思考）、mono 命令行、attempt/重试链叙述平铺 | 过程像日志一样可回读（第一轮 G6，已部分落地） |
| **运行状态行** | 流内一行「已工作 3 分 34 秒」 | 长任务进行中的时间感知 |
| **产物卡** | 文件图标 + `hpylt-调研报告.md` + 类型徽章「**文档·MD**」+「打开」按钮 | 产物是对话内的一等对象，一键入右栏 |
| **Diff 汇总卡** | 「1 个文件已更改 **+153 −11**」+「**撤销**」按钮 | 改动量一目了然、可就地反悔 |
| **行内产物 chip** | 「编写中文终端调研报告」药丸 chip 内嵌文本流 | 过程引用产物轻量不打断 |
| **Composer** | 占位「提出后续修改要求」；附件 + / 权限模式 / 模型芯片 GLM-5.3-Flash / 作用域（局部）/ 发送 | 逐消息可控的四要素（Shannon 已对齐，占位文案语义更「续写」） |

### 1.3 右侧展示区（文档查看器图——本轮最大增量）

| 设计点 | 截图证据 | 设计意图 |
|---|---|---|
| **独立 tab 栏** | 文档 tab + 关闭 × + 「+」新建 + **窗口控制钮（可独立窗口化）** | 多文档并行、可脱离主窗 |
| **面包屑** | `ai-video-ad › hpylt-调研报告.md` + 收起钮 | 文档的归属（项目/路径）就地可见 |
| **元数据块** | 标题下的调研对象/版本/官网/日期/方式结构化摘要 | 文档「是什么、何时、为何」一眼可读 |
| **目录 TOC** | 编号彩色目录（1–8 节），点击跳转，**当前章节高亮**（第 8 节） | 长文档的导航与进度感 |
| **表格工具栏** | 悬停出现 复制/下载/展开 | 数据表可搬运 |
| **计划一等公民** | （第一轮图）计划 tab + 步骤弹层 + turn 预算门控 | 计划是可执行清单而不仅是文档 |

### 1.4 插件市场页（市场图）

| 设计点 | 截图证据 | 设计意图 |
|---|---|---|
| **市场大页形态** | H1「插件市场」+ 副标题「用插件为 ZCode 拓展技能、命令与 MCP 能力」+ 通栏搜索 | 市场是产品门面，不是设置子页 |
| **已安装图标行** | 横向图标行置顶，一眼看到已装了什么 | 已安装资产前置 |
| **公开/个人 tab** | 公开市场 vs 个人（自建/本地） | 两种来源分治 |
| **分类分区** | 生产力 / 开发者工具 / 实用工具 / 金融，每区**两列网格卡片**；区尾「以及另外 N 个」溢出链接 | 按目的浏览而非平铺 |
| **卡片语法** | 图标 + 名称 + 一行描述 + 次级「安装」钮（+ 徽章） | 低噪音、可扫读 |
| **右上动作** | 同步 / 设置 / **+ 新建**（自建插件入口） | 消费与生产在同一页 |

### 1.5 设计理念提炼（学什么）

1. **会话=运行**：侧栏回答「哪个在跑/哪个是定时/跑了多久/卡没卡」，不放静态标题列表。
2. **项目是容器不是派生物**：可为空、可承载任务/自动化，是组织的最小单位。
3. **产物链路闭环**：对话产出 → 卡片/chip → 右栏文档化阅读（面包屑/元数据/TOC）→ 可独立窗口。
4. **高频操作零跳转**：模型/权限/推理档就地切；快捷键就地可见。
5. **市场是门面页**：搜索 + 已安装 + 分类网格，而非管理下拉。
6. **不学的**：近黑单色审美（保留 Liquid Glass 方向）、订阅套餐徽章（Shannon BYOK 无套餐）、术语「任务化」（保留「对话」，Simple mode 受众对黑话敏感——第一轮 §3 结论继续有效）。

---

## 2. 与当前实现的对照矩阵

> 代码锚点（`dev@1c093dfe`，子代理逐一核实）：`SIDEBAR`=desktop/ui/src/components/Sidebar.tsx，`SESSIONS`=…/SidebarSessions.tsx，`BUBBLE`=…/chat/MessageBubble.tsx，`INPUT`=…/chat/ChatInput.tsx，`DOCK`=…/pages/chat/RightDock.tsx，`DOC`=…/artifact/DocumentRenderer.tsx，`EXT`=…/pages/Extensions.tsx，`ZH`=…/i18n/locales/zh-CN.json。

### 2.1 已对齐（PR #89 红利，不再投入）

| 维度 | Shannon 现状 | 锚点 |
|---|---|---|
| 分组双视图 | 「按项目/按会话」segmented，默认项目视图，localStorage 持久化 | SESSIONS:584-607, 62-67 |
| 项目树 | 可折叠项目头 + 会话缩进 + 当前项目自动展开 | SESSIONS:379-405, 179-190 |
| 运行遥测 | 运行中绿点（含活动工具）+ elapsed 徽章（42s/8m/1h12m，5s tick） | SESSIONS:478-489, 87-93 |
| 侧栏搜索 | 常驻搜索框：本地过滤 + 后端全文 + 高亮 + 命中计数 | SESSIONS:609-616, 226-256 |
| 计划 dock | plan mode 自动停靠 + `- [x]` 进度条 + 批准状态徽章 | pages/chat/PlanPanel.tsx:27-135 |
| dock tab 化 | context/plan/live/diff + N 个 artifact tab，可拖宽（280–720px）可关 | DOCK:38-47, 248-314 |
| composer 四要素 | 附件 + / 权限五档 / 模型芯片（逐消息）/ 推理档 | INPUT:459-636 |
| 工具卡遥测 | 耗时 + token 规模 + 沙箱徽章；错误卡默认展开红色内联 | BUBBLE:549, 579-613 |
| 重试链 / 子智能体 | RetryChainBanner + SubagentBlock | BUBBLE:495-533, 734-810 |
| hunk 级 diff | 单文件 dock tab 内 accept/reject + 键盘导航 | components/diff/DiffReviewBody.tsx:81-143 |

### 2.2 差距清单（本轮行动来源）

| # | 差距 | 截图证据（§1） | Shannon 现状 |
|---|---|---|---|
| G1 | **行内相对时间徽章**（刚刚/17小时/12天） | §1.1 | 会话行只有运行中 elapsed；非运行态右缘空白；全库无 time-ago（SESSIONS:87-93, 534-538） |
| G2 | **等待审批/出错状态点** | §1.1（状态语义完整性） | 行内无审批 pending / 失败红点（审批在 Header 铃铛 HEADER:281-291；错误要进会话才能看到） |
| G3 | **定时/自动化任务进项目树** | §1.1（时钟图标混排） | 侧栏只渲染 chat 会话；例行/目标在 /tasks 页，与项目无关联（SESSIONS:419-539） |
| G4 | **项目为空态/实体化** | §1.1（「暂无任务」） | 项目=working_dir 尾段派生（projectOf SESSIONS:95-100），无空项目、无重命名/图标，项目头一律通用 folder 图标（SESSIONS:280） |
| G5 | **顶部动作区 + 快捷键就地可见** | §1.1 | 一级导航在底部；Ctrl+N/K 生效但 tooltip 无 kbd 提示（SIDEBAR:45, 67） |
| G6 | **底部账号/状态区** | §1.1 | 仅模式切换 + 设置（SIDEBAR:316-339）；无用户/状态徽章（套餐徽章不适用，见 §4 不做清单） |
| G7 | **产物卡形态**（文件名+本地化类型徽章+打开钮） | §1.2 | 弱 chip：图标+标题+open_in_new；类型硬编码英文 HTML/SVG/Diagram/Document（ArtifactChip.tsx:13-39, detectArtifact.ts:87-94）；不识别 md/代码产物 |
| G8 | **Diff 卡缺 +x −y 与撤销** | §1.2 | 「N 个文件已修改」+ 路径 + 查看全部；无行数统计、无就地撤销（BUBBLE:338-371；undo 仅 /rewind BUBBLE:277-289） |
| G9 | **运行状态行**（已工作 X 分 X 秒） | §1.2 | 对话流内无 elapsed 聚合显示（只在侧栏行） |
| G10 | **文档 TOC** | §1.3 | 全库无 TOC/outline 实现（DOC 全文） |
| G11 | **面包屑 + 元数据块** | §1.3 | 无；tab 标题 max-w-24 truncate 截断（DOCK:286） |
| G12 | **表格工具栏**（复制/下载/展开） | §1.3 | 表格仅静态样式（DOC:34-42） |
| G13 | **渲染器双轨漂移** | §1.3 | 会话/计划用 CodeBlock（复制+语言+行号 ✓），artifact 文档用私有 code 组件（无复制 ✗）（code/CodeBlock.tsx:107-137 vs DOC:19-27） |
| G14 | **Artifact「重查看」能力锁死在死代码** | §1.3（全屏/窗口化） | ArtifactPanel（全屏/导出/autoOpen）未挂载仅测试引用；RightDock 实际用精简 ArtifactDocBody（artifact/ArtifactPanel.tsx:101-171 vs DOCK:350-401） |
| G15 | **dock 无「+」手动开 tab** | §1.3 | tab 全靠自动出现，不能手动打开任意文件；utility tab 不可关（DOCK:145-180, 272-302） |
| G16 | **市场大页形态** | §1.4 | hub 无大标题，5 类型页收进「管理」下拉（EXT:8-29）；平铺网格**无分类分区**（分类逻辑只存在于未挂载的遗留 ExtensionsHub.tsx:45-61）；无已安装图标行；无公开/个人二分；容器宽度 7xl/6xl/4xl 不统一 |
| G17 | 会话 tab 栏（中栏） | §1.2 | 无（侧栏 + 独立窗口替代）；低优先，见 §4 |
| G18 | Live 预览 URL 只读 | §1.3 对照 | LivePreview.tsx:135-141 地址栏只读 |

### 2.3 现存缺陷（顺手快修，非对标项）

| # | 缺陷 | 证据 |
|---|---|---|
| B1 | 中文 composer 占位文案残缺：拼出「正在 — shannon-desktop」 | ZH:3076 `"chat.input.placeholder.project": "正在"`（en 为 "Working in"） |
| B2 | 死 i18n 键 6 个：`sidebar.sessions.group.today/yesterday/thisWeek/earlier` + `sidebar.sessions.grouping.time`（2026-09 重构删时间桶后遗留） | ZH:2447-2451；代码零引用（已 grep 验证） |
| B3 | Artifact 类型标签/标题回退硬编码英文，中文 UI 漏出（"HTML document"/"Document"…） | detectArtifact.ts:28-41, 87-94 |

---

## 3. 问题清单（按优先级）

**🔴 P0——认知/信任障碍或现存缺陷**
1. B1 占位文案残缺（每日可见的破窗）
2. B3 类型标签英文漏出（中文 UI i18n 破洞，违背「术语一个词」原则）
3. G7 产物卡形态弱——Shannon 的核心叙事「事件溯源/产物可审计」在对话内没有像样的产物对象

**🟡 P1——显著体验差距**
4. G1 行内相对时间徽章（侧栏「运行监控」叙事收尾）
5. G2 等待审批/出错状态点（「侧栏状态失明」的最后两态）
6. G10+G11+G13 文档阅读三件套：TOC / 面包屑+元数据 / 渲染器统一（右栏从「预览」升「阅读器」）
7. G14 回收死代码能力（全屏/导出）进 dock
8. G16 市场页形态（大标题/分类分区/已安装行/宽度统一）
9. B2 死键清理
10. G5 快捷键就地可见（低成本高检出）
11. G8 Diff 卡 +x −y 与撤销
12. G9 运行状态行

**🟢 P2——结构升级（待排期）**
13. G3 定时/自动化任务进项目树（依赖引擎数据模型确认：routine 是否携带 working_dir）
14. G4 项目实体化（注册表/重命名/图标/空项目）
15. G15 dock「+」手动开 tab
16. G12 表格工具栏
17. G6 底部账号区改造（provider/模型状态徽章形态）
18. G18 Live 预览 URL 可编辑
19. G17 中栏会话 tab 栏（与独立窗口方案冲突，先评估）
20. 智能分组（ZCode「分组 ✨」对位：会话自动聚类）

---

## 4. 改进实施方案（供评审排期）

> 原则：**全部在现有 token/i18n/Simple-Advanced 管线内实现**；不引入新依赖框架；每项含验收口径；工作量为一档估算（S≈半天，M≈1–2 天，L≈3–5 天）。
> 分 5 个可独立合并的批次 + 1 个待排期批次；批次内按序依赖，批次间可并行。

### 批 A · 缺陷快赢（半天–1 天，随任一 PR 搭车）

| 项 | 改动 | 锚点 | 验收 |
|---|---|---|---|
| A1 | `chat.input.placeholder.project` 改完整文案：「正在 {workingDirName} 中工作」（en: "Working in {workingDirName}"）；核对插值参数传递 | ZH:3076 / en.json:3076 / ChatInput.tsx 占位拼接处 | 中文 UI 无「正在 —」破窗；i18n 测试含插值断言 |
| A2 | 删除 6 个死键 `sidebar.sessions.group.today/yesterday/thisWeek/earlier`、`sidebar.sessions.grouping.time`（×2 locale） | ZH:2447-2451 / en.json 同位 | 全 locale grep 零残留；vitest 全绿 |
| A3 | Artifact 类型标签 i18n 化：`artifact.kind.html/svg/mermaid/document` + 标题回退文案本地化；`detectArtifact` 返回 kind，展示层查 intl | detectArtifact.ts:28-41, 87-94；ArtifactChip.tsx | 中文 UI 无英文类型漏出；Artifact.test 补断言 |

### 批 B · 侧栏运行语义收尾（2–3 天）

| 项 | 改动 | 锚点 | 验收 |
|---|---|---|---|
| B1 | **行内相对时间徽章**：非运行态会话行右缘显示 lastActivity 相对时间（刚刚/N分/N小时/N天/日期），复用 5s tick 与徽章样式；运行态保持 elapsed 不变 | SESSIONS:87-93, 534-538 | 不打开会话即可答「最近活跃是什么时候」；新增 `formatRelativeTime` 单测（含中文单位） |
| B2 | **等待审批/出错状态点**：琥珀点（审批 pending，数据源与 Header 铃铛同源 Header:281-291）+ 红点（最近 turn 失败）；点击直达审批 Modal / 会话内错误卡 | SESSIONS:478-489 扩展 | 侧栏可答「哪个卡在审批/哪个失败」；a11y：aria-label 三态可辨 |
| B3 | **快捷键就地可见**：NavRow tooltip 追加 kbd（对话 Ctrl+1 / 任务 Ctrl+2 / 搜索 Ctrl+K）；「新对话」按钮 hover 提示 Ctrl+N | SIDEBAR:45, 67, 213-244；useKeyboardShortcuts.ts:18-41 | 侧栏可见控件 tooltip 均含快捷键（axe 通过） |
| B4 | **顶部动作区微调**：品牌区下新增一行图标动作「搜索（Ctrl+K）」「自动化（→/tasks?tab=automations）」；底部导航不变（不做大迁移，保留 Shannon 既有 IA） | SIDEBAR:196-244 | 两入口可达且 tooltip 含 kbd；Simple/Advanced 行为一致 |
| B5 | **底部状态区**：设置行上方加当前 provider·模型徽章（点击 → /settings/models），替代「套餐徽章」的产品化表达（BYOK 叙事） | SIDEBAR:316-339 | 徽章随模型芯片双向同步；离线/未配置显示引导态 |

### 批 C · 对话区产物与状态（2–3 天）

| 项 | 改动 | 锚点 | 验收 |
|---|---|---|---|
| C1 | **产物卡升级**：chip → 卡片（类型图标 + 文件名 + 本地化类型徽章「文档 · MD」+「打开」主按钮）；detectArtifact 增加 ```markdown 产物卡形态与文件名提取（首个 H1）；尺寸克制（单行高）保持消息流密度 | ArtifactChip.tsx:13-39；detectArtifact.ts:44-76 | 视觉对齐参考 §1.2；点击开 dock tab；Artifact.test 更新 |
| C2 | **Diff 汇总卡增强**：头部加 `+x −y` 行数统计（数据源：现有 diff 统计，与 DiffDialog 同源）；卡尾加「撤销」按钮 → 该 turn checkpoint 的 /rewind 确认流 | BUBBLE:338-371, 277-289 | 撤销有二次确认；行数与 Diff 视图一致 |
| C3 | **运行状态行**：流式期间消息流尾部 sticky 一行「已工作 X 分 X 秒 · 正在 {tool}」，复用侧栏 SessionActivity 与 elapsed formatter；完成后消失 | MessageArea.tsx；SESSIONS:87-93 复用 | 运行中可见、结束即隐；不遮挡滚到最新 FAB |

### 批 D · 右栏从「预览」升「阅读器」（3–5 天，本轮核心）

| 项 | 改动 | 锚点 | 验收 |
|---|---|---|---|
| D1 | **文档 TOC**：DocumentRenderer/PlanPanel 解析 h1–h3，文档体右缘渲染编号 TOC（当前节 scroll-spy 高亮、点击平滑滚动）；窄宽度折叠为「目录」弹出 | DOC:45-57；新组件 artifact/DocumentToc.tsx | 对 3 级以上长文档可用；高亮随滚动同步；a11y 导航地标 |
| D2 | **面包屑 + 元数据块**：文档体头部 `项目名 › 标题`（项目复用 projectOf 逻辑）+ 元数据行（类型徽章 / 生成时间 / 来源会话链接） | DOCK:350-401（ArtifactDocBody） | 元数据「来源会话」可跳回原会话 |
| D3 | **渲染器统一**：DocumentRenderer 代码块改接共享 CodeBlock（复制/语言标签/行号），消除双轨 | DOC:19-27；code/CodeBlock.tsx:107-137 | artifact 文档代码块具备复制按钮；会话内/文档内代码块视觉一致 |
| D4 | **回收死代码能力**：RightDock 文档 tab 增加 全屏（dock 最大化覆盖主区）与 导出到磁盘（复用 ArtifactPanel 既有实现与 Tauri 文件对话框）；autoOpen 偏好挪入 Settings；随后删除未挂载的 ArtifactPanel 死代码 | ArtifactPanel.tsx:101-171（能力来源）；DOCK:350-401（落点） | 全屏/导出在 dock 内可用；死组件移除（grep 零引用）；bundle 减重 |
| D5 | **计划面板交互补全**：计划 tab 步骤条目支持勾选回写（`.shannon/plans/*.md` 复选框写回 + 引擎事件刷新），在「engine-owned 只读」边界内提供人工勾选 | PlanPanel.tsx:7, 84-135 | 勾选持久化且与进度条一致；引擎重写不冲突（last-write-wins + 提示） |

### 批 E · 扩展市场形态（3–4 天 + 内容运营依赖）

| 项 | 改动 | 锚点 | 验收 |
|---|---|---|---|
| E1 | **Hub 市场化**：hub 头部加 H1「扩展市场」（en: Extensions Marketplace）保留既有副标题；搜索框保持 sticky；容器统一 max-w-7xl | EXT:8-29, 65-75 | hub 首屏有大标题+副标题+搜索三段式；全子页宽度一致 |
| E2 | **分类分区**：复活 ExtensionsHub.tsx 分类逻辑（productivity/design/data/code 图标配色已有）；hub 按类分区、区内两列卡片；类型页（MCP/技能/Agent/数据源/插件）保持直达 tab 行（「管理」下拉保留为溢出） | extensions/ExtensionsHub.tsx:45-61（复活改造）；EXT:23-29 | hub 可按类扫读；无分类数据时按类型分区兜底 |
| E3 | **已安装图标行**：hub 搜索下方横向图标行（已装扩展 icon + tooltip 名称，点击 → 已安装页）；空态隐藏 | Installed.tsx:119-147（数据源） | 图标行 ≤1 行高，hover 有名称 |
| E4 | **公开/个人二分**：hub tab「公开（精选目录）/ 个人（本地自建 + agent 产出待审批）」，映射既有 all/curated/agent 筛选与插件来源筛选；个人 tab 保留「技能创建器」式自建入口 | Skills.tsx:236-260；Plugins.tsx:336-346 | 两条来源路径清楚；agent 自产技能审批入口可达 |
| E5 | **内容扩充**（依赖项，非纯 UI）：精选目录条目扩容与分类元数据补齐——需要 skills/插件注册表运营，先以类型分区兜底上线 | — | 首屏 ≥3 个分区、每区 ≥2 卡（数据到位后） |

### 批 F · 结构升级（待排期，需评审后单独立项）

| 项 | 内容 | 前置依赖 |
|---|---|---|
| F1 | 定时/自动化任务进项目树：例行/目标按 working_dir 归入侧栏项目（时钟图标 + 「即将」徽章对齐参考） | 引擎确认 routine/goal 与 working_dir 的关系数据 |
| F2 | 项目实体化：项目注册表（重命名/图标/颜色/归档）、空项目与「暂无任务」空态、跨会话/例行/目标聚合 | F1 数据模型 |
| F3 | dock「+」手动开 tab：菜单（打开文件/计划/Diff）；多文件 Diff 从 Modal 迁 dock tab | DiffDialogMulti 复用 DiffReviewBody |
| F4 | 表格工具栏（复制/CSV 下载/展开） | DOC 表格节点 |
| F5 | Live 预览 URL 可编辑（保持 sandbox iframe 白名单约束） | 安全评审 |
| F6 | 智能分组（「分组 ✨」对位）：按主题自动聚类会话为「分组」视图第三模式 | 产品决策 + 分组质量评估 |
| F7 | 中栏会话 tab 栏 / Artifact 独立窗口（Tauri WebviewWindow） | 与多窗口方案合并评估 |

### 不做 / 谨慎（评审需知的取舍）

1. **不照搬近黑单色审美**：保留 Liquid Glass 方向与 12 主题体系（UI-IMPROVEMENT-PLAN §6.5 已定）；吸收的仅是密度与外显策略。
2. **不改术语「对话→任务」**：知识工作者受众 + Simple mode（第一轮 §3 结论）；ZCode 的「任务」心智以「运行语义外显」吸收而非更名。
3. **不恢复时间桶分组**：2026-09 重构已刻意移除（SESSIONS:258-267 注释给出理由）；以 B1 行内 time-ago 徽章吸收参考的时间扫读价值。
4. **不引入订阅套餐徽章**：Shannon BYOK 无套餐概念；以 B5 的 provider/模型状态徽章做产品化表达。
5. **执行日志式转录不做整体替换**：气泡+折叠卡是 Simple mode 的正确密度；如需日志密度，走 F6 同批的 Advanced 密度档评估，不双轨维护两套转录。

---

## 5. 与既有规划的映射

| 本轮批次 | improvement-plan-2026-09 | 第一轮 delta（PR #89 已实施） | 新增性 |
|---|---|---|---|
| 批 A | G-3 i18n 破洞的收尾 | — | 缺陷修复 |
| 批 B | §6.2 徽章规则延伸 | ① 侧栏遥测的收尾（补审批/出错/相对时间） | 新增 |
| 批 C | §6.4 内联 diff 延伸 | ⑤ 工具卡遥测的产物侧补全 | 新增 |
| 批 D | §3.8「点路径打开」语法 / Wave 2 阅读体验 | ②⑦ dock 统一的阅读能力补全 | **新条目（本轮核心）** |
| 批 E | §3.6 Extensions 目录化 | — | 已有规划的「市场形态」具体化 |
| 批 F | §7 Wave 3（Project/多窗口/可拖拽） | ④⑧⑨⑩ 的深化 | 结构升级 |

## 6. 附录：证据与参照物

- 本轮三图：`docs/design/ui-audit-2026-09/screenshots/competitors/zcode-desktop-{sidebar-groups,marketplace,doc-viewer}.png`（原件 `reference/zcode/`）
- 第一轮两图：同目录 `zcode-desktop-main.png`（v3.11.2）、`zcode-desktop-plan-dock.png`（= 09-18 图，md5 已核对一致）
- Shannon 代码现状：`dev@1c093dfe`，四个子代理探查报告（侧栏/对话/右栏/扩展），关键锚点已在 §2 内联
- 实施验证基线：`pnpm --filter desktop-ui test`（vitest）、`tsc --noEmit`、`pnpm demo` 截图对照、axe a11y 抽查
