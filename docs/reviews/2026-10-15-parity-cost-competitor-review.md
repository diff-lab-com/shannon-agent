# 设计还原度 / 用量统计策略 / 竞品对比 综合评审（2026-10-15）

状态：**待审核**（含 5 个裁决点，见 §6）。本文档是评审+提案，不含任何代码改动。

取证材料：
- 设计稿 24 页 + 既有截图：`docs/design/ui-redesign-2026-10/`（规格基准 `assets/shannon-ui.css`）
- 实现侧截图 18 张：mock 模式（`pnpm demo`）+ Playwright，1440×900@2x，tokyo-night 主题，与设计稿同规格（截图脚本见附录 A，可复跑）
- 费用/token 全链路代码盘点（引擎→台账→UI 16 个展示点）
- 竞品调研 12 产品（Cherry Studio / LobeChat / Chatbox / ChatWise / Claude Desktop / ChatGPT / TypingMind / Jan / Msty / AnythingLLM / Cursor / Windsurf），来源 URL 附各结论处

---

## 0. 结论速览

| 问题 | 结论 |
|---|---|
| ① 设计稿精细还原度 | **未精细还原属实**。逐页还原度 3.5–6.5/10。根因不是单页走样，而是 **7 个跨页系统性分歧**（chip 形态、语义四色、aurora 渐变签名、按钮明暗方向、数字排版、密度/页头层级、底色层级反向）+ **4 个真缺陷**（穿帮级）。逐页打补丁没有意义，应按「设计系统对齐」一批修掉（§2、§5-G1）。 |
| ② 费用 vs token 统计 | **同意「token 精确为主口径」的方向，但建议表述修正为「费用从精确口径降级为估算口径、曝光收敛」，而不是停止统计费用**。费用管线是四套预算体系（会话/月度/routine/goal 全 USD 计价）的控制基础，拆掉是破坏性且不可逆的（历史 usage 无价格就无法重算）；用户混淆的根源是「把估算当账单」，解法是降口径+收敛曝光，不是删数据。另发现 2 个必须修的诚实性缺陷（未知费率静默按 $3/$15 兜底、provider 原始 usage 字段丢弃）。竞品证据强力支撑：**Cursor 2026-07 已把用量页美元撤掉只留 token**；Cherry Studio 价格表维护是吐槽重灾区。详见 §3。 |
| ③ PM 竞品对比 | Shannon 在**统计页丰富度、上下文预算控制、安全防线、时间线审计**上领先多数竞品；在**消息级 token 标注、tok/s 速度、输入框 token 预估、会话组织**上落后（§4）。补齐清单进了 §5-G4。 |

---

## 1. 设计还原度评审（问题一）

### 1.1 系统性分歧（跨页根因，修一次全局生效）

| # | 分歧 | 设计稿 | 实现 | 影响 |
|---|---|---|---|---|
| S1 | **chip 几何系统** | 全系统 chip：radius 8、高 28、边 `rgba(255,255,255,.07)`、底 `rgba(255,255,255,.04)`、字号 12px | composer 与页面 filter chips 全部 pill 化（radius≈999/18）、高约 40px、字号 15–16px（实measured） | 每页都在；composer 最重 |
| S2 | **语义四色被紫色吞没** | 批准=绿 `#3ddc97`、拒绝/失败=红 `#fb7185`、风险=琥珀 `#f5b83d`、强调=紫，四色各司其职 | Approve=实心紫、Deny=中性描边、HIGH 徽章=实心蓝 `#316bf3`（站内无此语义）、连接态=蓝 | 收件箱最痛；语义一致性 bug 级 |
| S3 | **aurora 渐变签名缺失** | 进度条、tab 下划线、柱图高亮柱、侧栏 meter 全走 `#8b7cf7→#6fa8e8→#56c8dd` 渐变 | 全部纯紫 `#8b7cf7` 实色（NL 卡顶部发丝线是唯一幸存者） | 品牌感最直接的载体 |
| S4 | **主按钮明暗方向反转** | 深紫 `#6d5ce8` 实底 + **白字** + 紫晕；侧栏新对话 radius-10 一体 caret | 亮紫 `#8b7cf7` 底 + **近黑字**（#8b7cf7 在设计体系里只是 hover 色）；新对话 pill+分离箭头 | 每页主 CTA |
| S5 | **数字排版** | 金额/token/百分比全 mono + tabular-nums（`.money`） | 无衬线粗体 | usage/inbox/timeline 全部数据区 |
| S6 | **页头层级与密度** | 顶栏 14px 上下文标题 + 内容区 19px 页头 + 12.5px 单行说明；卡间隙 10px、内边距 16px、settings 内容栏 720px | 顶栏 22px+ 粗体页名，内容区无页头；卡间隙 24px、内边距 28–32px；settings 卡内 22px 大标题+整段说明 | 全部页面显松、显「重」 |
| S7 | **底色层级反向** | 近黑底 `#0a0b10`，卡片 `#0f1118` **亮于页面**（正向 elevation），侧栏玻璃亮于内容 | tokyo-night 底 `#1a1b26`，卡片 `#15161f` **暗于页面**（反向 elevation），部分页（usage）又同色仅描边——方向不统一 | 整页光感偏亮偏蓝；材质语言混乱 |

> S7 说明：tokyo-night 基底是 W3 既定裁决，**换底与否单列为裁决点 A（§6）**。但无论换不换底，S1–S6 都应修——它们与基底无关。

### 1.2 逐页还原度评分与 Top 分歧

| 页面 | 设计稿 | 实现 | 还原度 | 前 3 分歧 |
|---|---|---|---|---|
| Welcome | 01-welcome | /welcome | **5.5** | 品牌 tile 无渐变无光晕且挪位；标题无 aurora 渐变词；无 ambient 光晕；路径 chip 缺失；底部密钥状态行缺 |
| Chat | 02-chat + 02c | /chat | **6 / 6.5** | chip pill 化+字号大（S1）；⊕ 无极光青角标；占位区 sparkles+会话名替代教学文案；侧栏搜索无 ⌘K、导航无 kbd chips、无「指挥台/用量」项；消息无头像列、工具调用为多行卡（设计为一行折叠条+状态 chip）；顶栏页名替代会话名+分支 crumb |
| 状态条 | 02c D5 | — | — | **双状态条并存**：ChatStatusBar（目录+分支，D5 合规 ✓）之外，Layout 全局 footer（`Layout.tsx:243`，U2 旧裁决：品牌/任务数/版本/**费用**）仍在——设计稿只有一条。见裁决点 D |
| Command Palette | 14 | Ctrl+K | **6** | 独立输入框替代 flush pin 行；选中态内缩非通栏紫底；33vh 非 14vh 定位；键位条纯文本非 kbd chip；面板多了 aurora 顶线（设计只给 composer） |
| Tasks | 04 (+09) | /tasks | **6**（09 视角 **3**） | 右列（迷你月历+目标卡）整体缺失；Best-of-N 56px 实心琥珀图标砖偏离 tile 语言；节标题 16px+ 过重；tab 无计数徽标；**Mission Control 五列看板/agent 集群/使命进度卡/dock 全部不存在**，且横幅「Open Mission Control」按钮指向重定向路由=循环入口 |
| Inbox/Triage | 05 | /triage | **5.5** | Approve/Deny 语义色缺失（S2）；HIGH 实心蓝徽章；统计卡无 22px 大数字层级；行图标圆形非 32px 方 tile；卡底暗于页面（S7） |
| Usage | 06 | /usage | **5.5** | KPI 8 小 tile 替代 4 大 stat 卡（无语义色 delta、无 mono）；模型分布环图首屏不可见；柱图无渐变高亮柱；预算卡从页尾只读态变页首内联编辑；数字非 mono（S5） |
| Connectors | 07 | /extensions/skills·mcp | **6** | 目录卡无图标砖、无卡足安全徽章、动作按钮左下（设计右置）；「高级管理」紫色实心 pill 抢眼（设计 ghost 下拉）；下划线 tab 变 pill 组；Online=蓝（设计绿）；mono 大面积滥用 |
| Memory | 08 | /memory | **5** | 右栏「分类分布」环图卡缺失；页头无大标题；记忆行解剖反转（meta 前置、内容非粗体标题、无来源会话/注入次数溯源）；分类 chip 一律灰（设计四色）；hashtag chip 截断 |
| Files | 11 | /files | **6.5** | 文件卡无内嵌预览缩略区；过滤 chip 未激活项裸文本（设计全 pill+计数）；顶栏无路径/文件数元信息与项目切换 |
| Companion | 13 | /companion | **3.5** | 全窗平铺替代 356×260 浮窗；**底部说明文案被视口裁切 + DEMO chip 遮挡（真 bug）**；原生 macOS 蓝 checkbox 跳主题；最近捕获历史列表缺失 |
| Settings ×6 | 12-* | /settings/* | **5.5–6** | Connections 两处**悬空孤儿图标**（只有图标无标题，渲染残缺）；主题卡全是同一套紫色线框 mock（应渲染各主题真实双色色板）+无「✓当前」徽标；页头/卡片标题层级（S6）；分段控件选中态过饱和实心（设计紫暗 chip）；设备配对/远程主机表格骨架缺失 |
| Permissions | （12-security） | /settings/permissions | 未评分 | 补拍确认：与 general 同款「大卡+大标题+段落说明」模式（S6 适用）；执行模式 seg 四档在，规则表格式高密度组件存疑 |

### 1.3 确认的真缺陷（非风格，G0 批次）

1. **Companion 页底部文案被视口裁切**，且 DEMO chip 遮挡（impl/13-companion.png 实证）；
2. **Connections 页两处悬空孤儿图标**——只有图标没有标题的卡片节渲染残缺；
3. **Connectors 页侧栏激活态丢失**——Memory/Files 有选中填充，Connectors 无（像素扫描确认）；
4. **Mission Control 循环入口**——横幅按钮指向被 302 回 /tasks 的路由，永远不可达。

### 1.4 甄别：以下**不算**风格分歧（避免误修）

- composer 用量 chip 无 % 数字：`contextPct` 为 null（mock 无 usage 事件）时 icon-only 是诚实契约（`ChatInput.tsx:621-627`），有数据时会显示 %。逻辑符合 02c D4。
- 状态条 branch 段隐藏：非 git 目录/查找中隐藏是既定行为。
- 列表/图表空态：mock 数据所限的内容差异。
- 右侧 Diff dock 常驻缺席：T7 批次已裁决「评审=主区整宽形态」（PR #414），设计稿的常驻 dock 概念已被替代，不翻案。
- 侧栏 CHATS/Automations 分组、Settings IA 收敛（通用吸收会话等）：有意的信息架构决策，保持。

### 1.5 覆盖缺口（设计 ↔ 路由映射）

- 设计有、实现形态不同：09-mission-control（重定向 /tasks，仪表功能未落地）、12-settings-security（→permissions 承载）。
- 实现有、设计无：/opc（OPC 看板）、/extensions/agents·datasources·plugins、Automations 侧栏组。竞品对比视角下这些是差异化资产，不是债。
- 未截图比对：TurnTimeline（需真实会话 id，建议 G 批实施时用 e2e 补）;visual-baseline 仅覆盖 chat/tasks/models 三页，建议扩容。

---

## 2. 用量统计策略评审（问题二）

### 2.1 现状一句话

费用在引擎事件发射时按**四层费率表**（内置 catalog → DEFAULT_PRICING 硬编码 → `.shannon-pricing.json` 覆盖 → `SHANNON_PRICING_JSON` env，再叠 LiteLLM 社区价 24h 缓存，未命中 fallback $3/$15）现算，随事件存入 `usage.jsonl` 台账与 turn/end；**四套预算（会话/月度/routine/goal）全部以 USD 计价**；前端 16 个展示点几乎处处有 $。token 侧 4 字段（in/out/cache_read/cache_creation）记录完整，`peak_context_used_tokens`、composer %chip、timeline per-turn chips 形态正确。

### 2.2 竞品证据（详 URL 见调研底稿）

| 证据 | 含义 |
|---|---|
| **Cursor 2026-07-31 起用量页撤掉美元只显示 token**（2026-02 已先撤单请求 $；forum 有用户不满但官方执行） | 与你的直觉完全同向：连以 $ 计费的商业产品都认为 $ 展示弊大于利 |
| Chatbox / Claude Desktop / ChatGPT 桌面版：**不显示 $**（后两者连 token 都不显示，只给限额） | 主流桌面端默认不暴露费用 |
| Cherry Studio 有估算 $ 统计页，但价格表配置是吐槽重灾区（#8558 价格消失、#20373 免费模型计费口径），文案明示「估算≠账单」 | 做 $ 统计的代价=永久的费率表维护+用户信任损耗 |
| TypingMind/Msty：$ 均为估算且明示以账单为准 | 没有任何竞品宣称「精确费用」——**「精确费用」在客户端侧本来就是伪命题** |
| LobeChat 消息级「生成明细」（in/out/cache 写读/reasoning/TPS/TTFT）是 token 展示的天花板 | token 方向的精细度空间还很大，且无费率维护负担 |

### 2.3 评审结论

**方向同意，表述修正：不是「不再精确统计费用」，而是「承认费用从来只能是估算，把它从主口径降位」。**

1. **保留费用计算管线与 `cost_usd` 存储**（不拆）：① 四套预算体系全 USD，拆费用=拆预算控制面；② token→费用随时可后补，反之不可逆——留着原始数据就是保留未来自由度（改口径件、导出、对账都靠它）；③ 计算发生在事件时点，边际成本为零。
2. **展示层收敛（核心动作）**：token + 上下文% 升为主口径，$ 从 16 个曝光点收缩到 3 类场景，且全部明标「估算」：
   - **保留 $**：/usage 审计视图与 CSV（数据完整性）、预算控制语境（BudgetBanner/SessionUsageDialog 的已花与上限、月度预算卡、GoalRunPanel——控制面需要货币单位）、timeline 审计区。
   - **$ 降为次要/删除**：Layout footer（只留 token 数）、composer 发送前预估（**$ 区间改为「≈ N tokens」**——这是发送前用户真正能对照的量，也是 Chatbox 验证过的形态）、TaskCard/HistoryView 的 $ 列（改 token 或预算%）、ContextBreakdownCard 会话累计 $（token 为主）。
   - **术语统一**：页面标题「Usage & Cost」→「Usage」；所有残留 $ 一律带「估算」徽标；KPI 首排改 token 系（Tokens/Cache/Requests/命中率/峰值），$ 卡后移。
3. **预算体系保持 USD 不动**：「一个月 10 美元」的用户心智无法用 token 替代；展示上以 % 为主数字（侧栏 meter 已经是 `62% of monthly budget` ✓）。
4. **两个必修的诚实性缺陷（无论裁決如何都要修）**：
   - 未知模型费率**静默 fallback $3/$15**（`types.rs:288`）→ 违反「absent = hidden，绝不编造」契约。改为 `cost_usd = None`，UI 显示「费率未知 · 仅 token」，并在 /usage 标注「N 条记录费率未知」。
   - provider 原始 usage 只保留 4 字段，`total_tokens/reasoning_tokens/service_tier` 等全部丢弃、原始 JSON 不落盘 → 在 usage.jsonl/turn-end 增补 `raw_usage` 可选字段（一次写入，永久保留重算能力）。

---

## 3. PM 竞品对比（问题三）

### 3.1 信息架构对照

| 维度 | Shannon | 竞品主流 | 评注 |
|---|---|---|---|
| 一级页面 | Chat/Files/Tasks/Inbox/Connectors/Memory/Settings(/opc/triage/timeline) | 多数=会话列表+聊天区+设置三件套；Cherry Studio 多 绘画/翻译/知识库 | Shannon 页面更接近「工作台」，差异化资产是 Tasks(Automations/Best-of-N/Goal)/Inbox/Timeline |
| 用量统计页 | /usage：KPI+日柱图+模型环图+4 张审计表+CSV（仅竞品 Cherry/Msty/TypingMind 有对应物） | 多数无统计页；Cherry 嵌在设置内 | **领先**；且「一级功能页」形态独一份 |
| 消息级 token | 无（会话级为主，timeline per-turn 有） | Cherry/Chatbox/ChatWise 每消息标注；LobeChat 明细最细 | **落后**，补齐清单 G4-1 |
| 输入框发送前预估 | $ 区间（estimate_send_cost） | Chatbox：token 数+上下文%；Cherry：预估 token | 形态应改 token（§2.3-2） |
| tok/s 速度 | 无 | Cherry/LobeChat/Jan 有 | 补齐清单 G4-3 |
| 会话组织 | 项目分组+Automations 列表 | ChatWise/Claude 有文件夹/Projects | 基本持平 |
| 预算控制 | 四套 USD 预算+80/100% 通知+自动暂停 routine | 竞品几乎空白（仅企业版管理端） | **独有优势**，宣传点 |
| 安全 | 出站防线读态、注入审计、签名校验、审批四档 | 竞品基本没有系统化防线 | **独有优势** |
| MCP/扩展 | 独立 Connectors 一级页（市场+已安装管理） | Claude 设置内扩展、ChatWise MCP 页 | 持平偏好 |
| Command Palette | 有（细节待打磨 §1.2） | 多数竞品无 | 领先 |

### 3.2 页面级问题与改进点

除 §1.2 表内的还原度分歧外，从竞品使用习惯出发的增量改进：

1. **侧栏缺「用量」入口**：设计稿有「资源·用量」导航组，实现只能从 meter 点击进入——统计页是 Shannon 的领先资产，入口应升级（挂回侧栏或 meter 强化）。
2. **消息 hover 明细**：对齐 LobeChat 但做轻量版（hover 消息气泡 → in/out/cache 三数 + 耗时 + tok/s），不进主视觉。
3. **免费/本地模型正反馈**：Cherry #20373 的启示——本地模型跑分时显示「本次 ¥0 · 走本地模型」是正向激励，Shannon 有 ollama 支持顺路可做。
4. **Palette 右侧 meta 列**（+38 −11 / 时间 / 当前模型）：设计稿有槽位，实现无——检索价值高（Cursor 的 palette 同款）。

---

## 4. 改进方案（批次划分，供审核）

> 实施方式沿用惯例：worktree + agent team + PR；每批独立可合并、可回滚。总量约 **5.5–7.5 天**，G0/G1 性价比最高。

### G0 真缺陷修复（0.5 天）
Companion 裁切+checkbox 主题化+浮窗比例；Connections 孤儿图标；Connectors 侧栏激活态；Mission Control 循环按钮（先摘除，看板立票）；**费率未知不再 $3/$15 兜底**；usage.jsonl 增补 `raw_usage` 字段。
验收：4 缺陷截图对照消失；未知费率模型会话显示「费率未知」。

### G1 设计系统一致性（1.5–2 天，主题无关项）
S1 chip 几何（radius-8/高 28/12px/hair 边，composer+页面 chips）；S2 语义四色恢复（approve 绿/deny 红/风险琥珀/去实心蓝）；S3 aurora 渐变签名（进度条/tab 下划线/柱图高亮/meter）；S4 主按钮深紫白字+新对话一体 caret；S5 数字 mono+tabular-nums；S6 页头 19px 层级+密度收紧+settings 卡片标题降级；图标 tile 语言（32px 圆角方+dim 底细线）；双状态条按裁决点 D 执行；palette 细节（flush pin 行/通栏选中/14vh/kbd 键位条）。
验收：18 张截图重拍对照，S1–S6 逐项消失；i18n 10 locale 键 parity 通过。

### G2 主题基底【裁决点 A】（1 天+回归）
- 方案甲（保守）：维持 tokyo-night，仅统一 elevation 方向（卡片亮于页面）+侧栏/内容明度关系。还原度封顶约 7/10。
- **方案乙（推荐）**：把设计稿 ink 基底（`#0a0b10`+ambient 双光源+玻璃侧栏）做成第 14 个主题「Aurora Ink」并设为默认（theme-source.json 管线现成，tokyo-night 保留可切）。还原度上限打开到 9/10。
验收：全路由双主题截图回归；对比度抽查（正文/次要文字/边框）。

### G3 用量口径落地【裁决点 B】（1 天）
§2.3 曝光收敛表执行：footer 撤 $、pre-send 改 token 预估、TaskCard/HistoryView 换 token、KPI 重排、全 $ 标「估算」、「Usage & Cost」改名；顺路补 §3.2-1 用量入口。
验收：全局 grep 无未标注「估算」的 $ 展示（预算控制面除外）；e2e 用量相关 spec 更新全绿。

### G4 竞品补齐（2–3 天，可拆 4a/4b）
4a 页面骨架：Memory 右栏环图+页头+记忆行解剖；Files 预览缩略区+过滤 pill；Connectors 目录卡解剖+下划线 tab；Tasks 右列；Welcome hero 氛围；主题卡真实色板+「✓当前」。
4b 消息级统计：消息 hover token 明细（in/out/cache+耗时+tok/s）；palette meta 列；本地模型零成本提示。
验收：逐页对照设计稿重拍；新增指标在 mock 与真实会话各验证一次。

### 回归方式
- 截图基线：附录 A 脚本扩入 e2e（visual-baseline 从 3 页扩到全路由）；
- 既有门：`scripts/ci-local.sh`（含 doc gate）、axe walkthrough、i18n parity。

---

## 5. 裁决点清单（需要你拍板）

| # | 问题 | 选项 | 建议 |
|---|---|---|---|
| A | 主题基底（S7） | 甲：tokyo-night 上修细节 / 乙：新增 Aurora Ink 默认主题 | **乙**（G1 之后再做，回归一次到位） |
| B | 费用口径档位 | 激进：$ 仅存 /usage+预算 / 中道：§2.3 收敛表 / 温和：全保留只加「估算」标注 | **中道** |
| C | Mission Control | 独立视图（五列看板+agent 集群，2–3d）/ 摘除循环按钮维持 /tasks / 维持现状 | 先摘按钮；看板按需立票 |
| D | 双状态条 | Layout footer（U2）与 ChatStatusBar（D5）并存 / footer 撤内容仅留异常态+DEMO 标识（并入 D5 单条语义） | **后者**（与 G3 撤 footer $ 联动） |
| E | 消息级 token 标注 | 做轻量 hover 明细 / 不做 | **做**（G4-4b，竞品普遍有的最低水位） |

---

## 附录 A：实现侧截图脚本（可复跑）

```js
// 用法：先起 mock server（cd desktop/ui && pnpm demo），再 node 本脚本
import { chromium } from 'file://<repo>/desktop/ui/node_modules/.pnpm/playwright@1.63.0/node_modules/playwright/index.mjs';
const ROUTES = ['/welcome','/chat','/tasks','/triage','/usage','/extensions/skills',
  '/extensions/mcp-servers','/memory','/files','/settings/general','/settings/models',
  '/settings/connections','/settings/theme','/settings/notifications','/settings/permissions','/companion'];
const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport:{width:1440,height:900}, deviceScaleFactor:2, locale:'en-US' });
await ctx.addInitScript(() => localStorage.setItem('shannon-theme','tokyo-night'));
const page = await ctx.newPage();
for (const p of ROUTES) {
  await page.goto('http://localhost:1420'+p, {waitUntil:'domcontentloaded'});
  await page.waitForTimeout(2500);
  await page.screenshot({ path: `/tmp/ui-parity/impl${p.replace(/\//g,'-')}.png` });
}
await browser.close();
```

## 附录 B：本次评审的证据截图

实现侧 18 张：`/tmp/ui-parity/impl/`（ephemeral，可按附录 A 复现）；设计稿截图在仓库内 `docs/design/ui-redesign-2026-10/screenshots/`。关键局部放大（composer chips 40px/15px 实测）在评审过程记录中。
