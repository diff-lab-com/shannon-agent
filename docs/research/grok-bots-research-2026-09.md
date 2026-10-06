# Grok Bots 调研报告

- 日期：2026-09-08
- 性质：外部生态调研。信息来自 x.ai / docs.x.ai 官方公告与文档、CNBC / Reuters / Forbes / FTC / 州总检察长官方稿等公开来源，截至 2026-09-08；单一来源或多口径冲突项均标注"未核实"。
- 关联：[competitive-research-2026-09](../competitive-research-2026-09.md)（未覆盖 Grok，本文填补空白）· [openworker-research](../openworker-research.md)（桌面 coworker 象限）· [model_configuration_research](../model_configuration_research.md) · 模型注册表 `crates/shannon-core/src/model_registry/catalog.rs:696`

## 0. 一句话结论

**"Grok bots" 的官方含义在 2026 年已换轨**：不再是 Ani 一类的 3D 陪伴角色（该产品线 Grok Companions 已于 2026-09-01 停运），而是 2026-08-11 发布的 **Grok Bot**——角色化命名部署、7×24 常驻执行多步任务的自主智能体产品（独立 App；宣传称每 bot 独立云电脑，实测为同账号共享一台云端 Linux VM，见 §4）。同期 xAI 已被 SpaceX 合并（合并体估值约 $1.25 万亿，官网署名 SpaceXAI），模型线现役旗舰为 Grok 4.6（2026-08-12，500K 上下文，$2/$6 每百万 token），编码线换血为 grok-build-0.1 + 开源 CLI「Grok Build」。

**对 Shannon 的直接影响（P1）**：模型注册表中收录的 `grok-4.1-fast` 已于 2026-05-15 被 xAI 官方退役，`grok-4.5` 定价已过期（$3/$15 → 实际 $2/$6），需更新注册表并新增 `grok-4.6` 与 `grok-build-0.1`。**竞争面（P2）**：Grok Build CLI（开源终端编码 agent）与 Grok Bot（常驻云电脑 agent，已有企业版）分别是 shannon-code 与 shannon-desktop 两条线的新竞品形态，现有竞品文档均未覆盖。**产品与交互（本轮补充）**：§4 新增 Grok Bot 的 UI/信息架构、功能与审批设计、典型用例、User Journey Map 与 User Stories（journey/stories 为基于公开功能事实的重构，非官方资料）。

---

## 1. 术语澄清："Grok bots" 的三层含义

| 名称 | 属性 | 现状（2026-09-08） |
|---|---|---|
| **Grok Bot** | 官方产品名（2026-08-11 beta 发布） | 在运营。当前 "Grok bots" 的官方所指：产品内可命名、按角色部署的多个智能体（各有独立云电脑、凭据、工具与 roster） |
| **Grok Companions** | 官方功能名（2025-07 上线的 3D 陪伴角色） | **已停运（2026-09-01）**，角色迁移至第三方独立应用 "Animates"（与 xAI 无官方隶属，该归属为单一来源） |
| "Grok bots"（俗称） | 媒体/社区泛称 | 泛指陪伴角色、X 平台的 @grok 问答助手、第三方 Grok 驱动 bot |

本报告按「公司 → 模型 → Grok Bot 产品 → 消费端 → 开发者平台 → 基准 → 风险 → Shannon 启示」展开。

## 2. 公司概况：从 xAI 到 SpaceXAI

| 维度 | 事实 | 来源 |
|---|---|---|
| 成立 | 2023-03，Elon Musk 创立，团队来自 DeepMind/OpenAI/Google | 公开资料 |
| 第一次合并 | 2025-03-28 xAI 全股票收购 X Corp（xAI 估值 $800 亿，X 作价 $330 亿） | CNBC 2025-03-28 |
| 融资 | 2026-01-06 Series E $200 亿（Nvidia、Cisco 参投），估值约 $2300 亿 | CNBC / Reuters |
| 第二次合并 | **2026-02-02 SpaceX 全股票收购 xAI**：xAI 约 $2500 亿、SpaceX 约 $1 万亿、合并体约 $1.25 万亿；官网现署名 "SpaceXAI LLC" | CNBC 2026-02-03；x.ai 检索 2026-09-08 |
| 算力 | 孟菲斯 Colossus 约 55.5 万 GPU / 2GW；Colossus 2 约 35 万块 GB200/GB300 / 450MW；Epoch AI 估约 111 万 H100 当量 | Introl 2026-01 / Measured AI / Epoch AI |
| 用户 | X+Grok 合计 5.5 亿 MAU，其中 **Grok MAU 1.17 亿**（SpaceX S-1）；第三方 Similarweb 口径移动端 DAU 2026-04 仅 1220 万（与官方口径差距大，统计口径不同） | Forbes 2026-05-21 / Similarweb |
| 政府业务 | Grok for Government（2025-07-14）；国防部 $2 亿合同；GSA OneGov 协议每机构 $0.42 用 18 个月（至 2027-03）；2025-12-22 获「战争部」（Department of War）合同 | x.ai / GSA / fedscoop |

## 3. 模型演进与现役型号

### 3.1 时间线（官方发布日期）

| 版本 | 日期 | 要点 |
|---|---|---|
| Grok-1 | 2023-11-03 | 首发于 X；2024-03-17 Apache 2.0 开源（314B MoE） |
| Grok-1.5 / 1.5V | 2024-03-28 / 04-12 | 128K 上下文 / 多模态 |
| Grok-2 | 2024-08-13 | 图像生成（Aurora，2024-12 上线） |
| Grok-3 | 2025-02-19 | beta，引入推理模式 |
| Grok 4 | 2025-07-09 | 同步推出 SuperGrok Heavy 订阅 |
| Grok Code Fast 1 | 2025-08-26 | 低价编码模型（已退役）；2025-08-23 Grok 2.5 以限制性许可放出 |
| Grok 4 Fast | 2025-09-19 | 2M 上下文低价档（已退役） |
| Grok 4.1 / 4.1 Fast | 2025-11-17 / 11-19 | 后者同期发布 Agent Tools API（均已退役） |
| **Grok 4.5** | 2026-07-16 | 定位编程与 agent 任务；07-22 全端上线，07-28 进 GitHub Copilot |
| **Grok 4.6（现役旗舰）** | 2026-08-12 | 主打长时程 agent；500K 上下文，API $2/$6 每百万 token，知识截止 2026-02-01；08-19 上 Amazon Bedrock、08-26 上 Microsoft Foundry |
| Grok 5 | 未发布 | 原定 2026 上半年，已推迟，仍在 Colossus 训练；传闻 ~6T 参数（第三方口径，未核实） |

### 3.2 API 现售型号与定价（$/百万 token，docs.x.ai 一手数据）

| 模型 | 上下文 | 输入 | 缓存输入 | 输出 | 状态 |
|---|---|---|---|---|---|
| grok-4.6 | 500K | $2 | 未核实 | $6 | 在售旗舰 |
| grok-4.5 | 待核实（registry 现写 256K） | $2 | $0.30 | $6 | 在售 |
| grok-build-0.1（编码公测） | 256K | $1 | $0.20（第三方） | $2 | 在售（2026-06 公测） |
| grok-4.1-fast | 2M | $0.20 | $0.05 | $0.50 | **2026-05-15 退役** |
| grok-code-fast-1 | 256K | $0.20 | $0.02 | $1.50 | **2026-05-15 退役** |
| grok-3 系列 | — | — | — | — | 2026-05-15 退役 |

退役与 slug 重定向（官方 migration 页）：`grok-4-1-fast-* → grok-4.3`（定价未核实）、`grok-code-fast-1 → grok-build-0.1`。**grok-code-fast-2 从未存在**，编码线继任者就是 Grok Build 0.1。

### 3.3 开源策略

Grok-1（Apache 2.0）之后开源信誉持续破产：Grok 2.5 限制性许可被批 "open-washing"；Grok 3「6 个月内开源」承诺未兑现且 API 已下架；Grok 4+ 全闭源；2026 年转向开源 agent 框架（Grok Build CLI，2026-07-15）。

## 4. Grok Bot：常驻自主智能体（本次调研核心）

> 本章 UI/功能细节来源分级标注：【官方】x.ai 公告与 docs.x.ai/grok-bot 文档组、【实测】flaviocopes.com 2026-08-22 多图上手（目前最详尽的 UI 记录）、【转述】第三方分析、【用户报告】HN/Reddit/博客实测。§4.6–4.7 的 Journey Map 与 User Stories 为**基于上述事实的重构**，非官方资料。

### 4.1 产品形态、发布节奏与配额

**产品形态**（2026-08-11 beta）：

- 常驻智能体团队：bot 像同事一样在真实应用里干活，7×24 运行；官方理念 "Create a Bot, message it, grant access as needed. **No workflow builder**"——无工作流编排器，纯对话派活【官方 docs】。
- **独立 App**（macOS x64/Arm64、Windows、iOS 18+、Android beta），不内嵌于 grok.com 或 X 聊天；用 Cursor 或 SuperGrok 账号登录，无独立账号体系【官方 docs；实测】。
- **架构真相（修正宣传口径）**：公告称"每个 bot 独立云电脑"，实测与官方安全文档显示**同账号所有 bot 共享一台云端 Linux VM**（浏览器/文件系统/终端 + 单一 cookie 库 + 账号级共享凭据池），每个 bot 只拥有独立 "screen" 画面视图、独立对话线程与记忆；官方 Terms 明言"界面分离不等于安全边界，勿把 Bot 当安全屏障"【实测；官方 grok-bot-terms】。
- 与 API 的关系：xAI **没有**对标 Claude/OpenAI computer-use 的模型 API——"云电脑"是产品形态而非开放能力。

**发布节奏**：

| 日期 | 事件 |
|---|---|
| 2026-08-11 | beta 上线；门槛 SuperGrok Heavy（$300）/ Cursor Ultra（$200）/ Cursor Teams Premium（$120/席） |
| 2026-08-21 | 门槛扩展至 SuperGrok Plus / Cursor Pro+ |
| 2026-08-26 | 扩至 SuperGrok（$30）/ Cursor Pro（$20）等全档 |
| 2026-08-29 | X 集成第一版：绑定 X 账号（无开发者账号可代建）、付费用户获赠 X API 额度、搜帖/读时间线/查提及/汇总动态（官方首次把"bot 接入 X"产品化） |
| 2026-09-02 | Android beta 上 Google Play（ai.x.grok.bot）：锁屏后 bot 持续运行、任务跨手机/桌面接续 |
| 2026-09-03 | 企业版（访问/网络/审计控制，企业客户免费两周，价格未公布）+ 官方设计详解（Bot 名册、带生命周期状态的头像、电脑三级权限 status/preview/takeover、群聊、Routines） |

**定价与配额**：无免费档；**独立周额度**（与 Grok/Cursor 用量分开，官方未公布数值），超额按 token on-demand 计费，**无专项支出上限**；同时订阅 Cursor 与 SuperGrok 时自动取额度较大者；早期试用额度限 7 天窗口【实测；eesel】。vellum（08-20）记录的"独立订阅 $200/月、含 14 天试用"为档位扩展前的早期窗口口径。真实体感：HN 用户 3 小时耗掉 52% 周额度；r/grok 有"$200 档 4 天用完额度"报告。

**战略含义**：xAI 把 "bots" 一词从陪伴角色（To C 情感）正式转轨到生产力智能体（To C/To B 办公执行），借助 X 分发 + Cursor 套餐捆绑 + 企业版快速铺渠道；VentureBeat 判断其差异化在**封装**（computer use + 持久化 + 真实应用操作）而非模型本身。

### 4.2 UI 与信息架构

| 界面层 | 设计 |
|---|---|
| 顶层信息架构 | **带 presence（在线/忙碌/离线）的 Bot roster（名册）**，而非会话列表——这是与所有聊天式 AI 产品最根本的 IA 差异；09-03 官方设计详解确认"Bot 名册 + 带生命周期状态的头像"【转述 CellCog；官方】 |
| Bot 详情 | 独立对话线程 + 记忆；"screen" 视图查看该 bot 的云电脑画面；routine 保留最近 20 次运行记录；bot + 群聊总数上限 50【实测】 |
| 监督方式 | computer view（screen 画面）旨在"免持续监督"；三级权限 **status（看状态）/ preview（看画面）/ takeover（接管操作）**【官方 09-03】 |
| 跨端 | 移动 + 桌面共享同一线程；Android 锁屏后 bot 继续运行【实测；转述】 |
| 设计语言 | 设计叙事从"用户操作的聊天会话"转向"带 presence 的常驻 roster"；延续 Grok 极简黑底聊天风【转述；实测截图】 |

### 4.3 功能设计：创建、委派、审批、调度

- **创建流程**：首次启动引导 "Create your first Bot"——起名 + 选头像外观，配置项含职责描述、@提及添加插件（应用/团队插件浏览器，连接器含 Gmail、Drive、Outlook、Teams、Salesforce 等 + MCP）、上传文件至 /workspace、skills（斜杠调用）、routines、审批规则；**无模型选择器**（锁定 xAI 模型）【实测；vellum】。
- **模板机制**：官网示例 Bot（可围观真实运行过程）、Duplicate Bot（复制配置但不带历史/记忆）、**"Teach a task"**（录屏 ≤10 分钟自动起草可复用 skill，零代码）【官方 x.ai/bot；实测】。
- **任务委派**：纯对话式（"发消息派活如同事"）；可建**群聊**让多 bot 协作、互派任务；纠正 = 中断改指令；结果回传线程 + 手机推送 ping【官方；实测】。
- **审批与接管**：审批卡三选项 **Allow once / Always allow / Deny**；本地操作审批卡展示"确切命令"；可设 Require Approval / Always Allow 规则（前者优先）；遇密码/2FA/CAPTCHA/支付自动暂停交人工 **take over**【实测；转述 CellCog；官方 09-03】。
- **调度**：routines 支持定时/事件触发，每 bot 上限 50 个，保留最近 20 次运行，删除不可逆【官方 docs；实测】。
- **凭据**：bot 发起凭据请求，值以遮蔽（masked）录入、不进对话记录与模型上下文；文件/浏览器会话/登录存于**账号级共享凭据池**；删 bot 未必删共享凭据（Terms 明示）；本地电脑访问默认"每次询问"【实测；官方 Terms】。

### 4.4 典型用例（官方演示 → 真实报告）

| 用例 | 来源 | 结果/体感 |
|---|---|---|
| 销售 bot 自动更新 CRM；运营 bot 在 Gmail 完成新员工入职 + 发票处理；工程 bot 复现 bug 并建工单；QA bot 过夜巡检环境、修复失效测试数据 | 【官方】公告"内部团队怎么用" | 官方场景叙事 |
| 采购分析：接入供应商支出/合同/用量数据，找出超 $10 万直接节省 | 【官方】grok-bot-procurement | 官方案例 |
| 凭食谱照片点 Whole Foods 外卖、与承包商谈报价、机票比价 | 【转述】xAI 员工 Ben Lang（未核实） | 内部热门用例 |
| **面料采购 bot 经 WhatsApp 联系约 40 家越南供应商询价、议价并安排打样** | 【用户报告】HN | 异步委派真实可用；代价是 token 消耗巨大（"本月超过去 5 年总和"）、3 小时耗 52% 周额度 |
| 8 小时搭 12 个 bot（幕僚长/落地页/调研等）共享一台云电脑 | 【用户报告】Nate's Newsletter 08-14 | 多数任务成立，越复杂越易中断 |
| 机票预订实测：Palma→Dublin 一家四口购票流程走通 | 【用户报告】dev.to | 成功 |
| "high demand" 故障持续 29.5 小时；"Model unavailable" 报错；额度消耗过快 | 【用户报告】r/grok | 稳定性与配额是最大槽点 |

### 4.5 安全设计（官方自我定位）

- 同账号 bots 共享 VM / cookie 库 / 凭据池；官方 Terms 直言 bot **不是安全边界**；审批规则只拦"未执行"的动作，**无法撤销已完成操作**【官方 docs/Terms；转述】。
- 企业版隔离仅到"用户级"（同一用户的 bots 仍共机）；多源称 audit log 发布时仍标 "coming soon" 未实装【转述 CellCog/OneWave/ReleaseBot 等】。
- 周边风险参照：2026-05 BankrBot 摩尔斯电码提示注入盗取约 $15–20 万加密货币（Grok 聊天 + 第三方 bot，非本产品）；2026-07 Grok Build 被曝整仓上传 Git 库且未脱敏密钥【媒体】。

### 4.6 User Journey Map（基于公开功能事实的重构）

Persona：跨境电商卖家的采购/运营负责人（对标 §4.4 真实面料采购案例），SuperGrok 订阅者。

| 阶段 | 触点/界面 | 用户行为 | 心理 | 痛点（有真实反馈佐证） | 设计机会 |
|---|---|---|---|---|---|
| 1 认知 | x.ai/bot、示例 Bot 围观 | 看官方场景、围观示例 bot 真实运行 | "真能替我干活？" | 无免费档、无试运行【媒体】 | sandbox 试用 |
| 2 订阅上手 | SuperGrok 订阅 → 下载 App → Cursor/SuperGrok 登录 | 装桌面 + 手机端 | 期待 | 平台覆盖口径混乱（Linux）、地区受限（EU 曾 27 国） | 一致的平台矩阵说明 |
| 3 创建首个 bot | "Create your first Bot" 引导 | 起名/职责/插件/上传文件 | "像招了个远程助理" | 无模型选择器、记忆不可查看/导出【vellum】 | 记忆透明化 |
| 4 教技能 | Teach a task 录屏 | 一次演示固化询价流程 | 掌控感 | 录屏 ≤10 分钟上限 | 增量补充录制 |
| 5 委派与监督 | 线程消息 + screen 视图 + 推送 | 派活、看画面、等结果 | 信任逐步建立 | 执行透明度不足、长线程上下文混杂【Brian Lovin，未核实】 | 结构化执行日志 |
| 6 审批与接管 | 审批卡 / takeover | 输密码/2FA、确认支付 | 安全焦虑 | 共享凭据池、审批拦不住已完成操作【官方自认】 | 可撤销操作/操作日志 |
| 7 固化与扩展 | routines、群聊、Duplicate Bot | 定时化、多 bot 分工 | 规模化收益 | routine 删后不可逆、上限 50、额度烧得快（3h=52%） | 用量预测/预算上限 |
| 8 复盘 | （缺口） | 想回顾 bot 到底做了什么 | 失控感 | 无审计视图（替代品清单普遍指出）【helio/marblism】 | audit log（企业版 still coming soon） |

### 4.7 User Stories（按 persona，验收要点映射到已证实功能）

**P1 采购/运营个人用户**
- US-1 我要让 bot 用我的邮箱/WhatsApp 联系供应商询价议价，以便不用逐家发邮件。→ 验收：多账号连接器；结果回线程 + 推送（HN 面料采购案例已验证可行性）。
- US-2 我要在 bot 支付前被拦下人工确认，以便不放心让它碰钱。→ 验收：审批卡三选项；支付自动触发 takeover。
- US-3 我要把重复询价流程固化成每天自动跑。→ 验收：routine 定时触发；保留最近 20 次运行记录；上限 50 个。
- US-4 我要凭据不被写进对话记录。→ 验收：遮蔽录入、不进模型上下文【实测】。

**P2 多 bot 编排用户**
- US-5 我要建"幕僚长 + 调研 + 落地页"多个 bot 分工协作。→ 验收：roster + 群聊互派任务（Nate 12-bot 案例可达）。
- US-6 我要一眼看到每个 bot 忙不忙、在哪个画面干活。→ 验收：presence 生命周期头像 + per-bot screen 视图 + status/preview 权限。
- US-7 我要把跑通的 bot 配置复制给新 bot 且不带脏历史。→ 验收：Duplicate Bot 不含历史/记忆。

**P3 移动端用户**
- US-8 我人在外面也要能派活、收结果。→ 验收：iOS/Android、跨设备同线程、锁屏后继续运行、推送 ping。
- US-9 bot 卡在验证码/密码时能立刻叫我。→ 验收：自动暂停 + takeover 请求。

**P4 企业管理员**
- US-10 我要用公司域名统一开通/回收成员的 bot 访问。→ 验收：xAI console 域名验证、邀请、访问管理【09-03 企业版】。
- US-11 我要审计 bot 都执行过什么。→ 验收：audit log——⚠️ 发布时仍 "coming soon"，未实装。
- US-12 我要确保成员的 bot 互不可见。→ 验收：⚠️ 隔离仅到用户级，同用户 bots 共享 VM——官方明言非安全边界。
- US-13 我要限制 bot 可访问的网络范围。→ 验收：企业版 network 控制【09-03】。

**P5 透明度敏感用户**
- US-14 我要在 bot 动本地文件前看到确切命令并逐次批准。→ 验收：本地操作审批卡展示确切命令；本地访问默认"每次询问"。

### 4.8 UX 口碑与竞品形态对比

- **好评**：命名智能体（named agents）模式与界面精致度（Brian Lovin，转引未核实）；多账号连接器被视为独有亮点（Lenny/ChatPRD）；"Teach a task" 被指南普遍视为招牌交互。
- **差评**：单 agent 多任务别扭、长线程上下文混杂、执行透明度不足；模型黑盒不可选（Lenny/ChatPRD）；无试运行/审计视图【helio/marblism 替代品清单】。
- **形态对比**：ChatGPT Work（临时 VM 用完即弃 vs Grok 常驻）、Claude Cowork（桌面本地路线 vs 云端 VM）、Gemini 办公代理、Perplexity Computer；替代品清单普遍把 **Manus** 列为最接近的换用对象；HN 用户另对比开源 OpenClaw。VentureBeat：差异化在封装而非模型，"持久数字同事"定价约 $120/席口径。

## 5. 消费端产品线

### 5.1 订阅体系（x.ai/pricing，2026-09）

| 档位 | 价格 | 关键权益 |
|---|---|---|
| Free | $0 | 基础 Grok 访问 |
| SuperGrok Lite | $10/月 | 入门 |
| SuperGrok | $30/月 | 完整额度 |
| SuperGrok Plus | $100/月 | 含 1080p 视频生成 |
| SuperGrok Heavy | $300/月 | 最强模型档 + **Grok Bot 用量** |

### 5.2 Grok Companions（陪伴角色）兴衰史

| 时间 | 事件 |
|---|---|
| 2025-07 中旬 | 随 Grok 4 iOS 更新 soft launch：Ani（哥特动漫少女）、Bad Rudy（红熊猫"邪恶人格"），随后 Valentine；后增 Mika。3D 形象+语音+好感度+记忆，需 SuperGrok |
| 2025-08 | 密歇根州总检察长就 Ani 等角色调查 xAI |
| 2025-09-11 | FTC 6(b) 调查陪伴型聊天 bot，xAI 在列（7 家） |
| 2025-12–2026-01 | 性化深伪危机（含未成年人图像）；欧盟批评、英国 Ofcom 对 X 立案 |
| 2026-01-09 | 民主党参议员要求苹果/谷歌应用商店下架 X 与 Grok |
| 2026-01-26 | 密歇根等 **36 州总检察长联名**致函 xAI，要求停止生成 NCII/儿童性化内容 |
| 2026-07-24 | xAI 宣布逐步关停 3D 陪伴模式（无正式公告） |
| 2026-09-01 | **Grok Companions 正式停运**；角色 3D 形象团队（Animation Inc）另起炉灶做第三方应用 "Animates"（17+/18+ 分级） |

监管压力是陪伴线收缩的重要背景；截至 2026-09-08 无公开的正式处罚或和解（2026-09-03 有 CSAM 相关个人诉讼，见 §8）。

### 5.3 其他

- **@grok**：X 平台内置问答助手（被 @ 提及回复），有学术研究量化其互动（arXiv 2602.11286 "Grok in the Wild"）。
- **Grok Imagine**：图像/视频生成。Imagine Image 2.0 已 GA；API 约 $0.05/秒视频（单段 ≤15 秒）；2026 年更新 Reimagine、Folders。
- **自定义 bot 市场**：截至 2026-09 **不存在**对标 GPTs / Character.AI 的用户自建角色广场；马斯克 2025-07 预告的"自定义 Companion"从未上线，应用内仅有 "Personas" 自定义指令人格。
- **X 平台 bot 生态**：无官方 bot 框架，开发者自行组合 Grok API + X API；第三方案例以教程级 Discord bot 为主。

## 6. 开发者平台（api.x.ai）

### 6.1 Agent 能力

- **Agent Tools API**（2025-11 随 Grok 4.1 Fast 发布）：服务端工具 `web_search`、`x_search`、`code_exec`、function calling；计费 = token 用量 + 工具调用次数（web/x_search 约 $5/千次，第三方转述，官方页未直接核实；旧 Live Search 为 $25/千源）。
- grok-build-0.1 原生支持 MCP。
- 无 computer-use / browser-use 模型 API。

### 6.2 兼容性与缓存（对多 Provider 工具友好）

- **OpenAI SDK 兼容**：base_url 改 `https://api.x.ai/v1` 即用；官方 FAQ 称与 OpenAI / Anthropic API "fully compatible"（Swagger 列有 Anthropic 兼容端点，`/v1/messages` 路径未逐字核实）。
- SSE 流式（`stream: true`）。
- **Prompt caching 全自动**，建议 `x-grok-conv-id` 头 / `prompt_cache_key` 提升命中；grok-4.5 缓存价仅为输入价 15%。

### 6.3 SDK 与分发

- 官方 Python SDK（xai_sdk，gRPC 实现）；TS 走 OpenAI 兼容；**无官方 Rust SDK**（Shannon 自研 HTTP 客户端无碍）。
- 渠道扩张：GitHub Copilot、Amazon Bedrock、Microsoft Foundry、OpenRouter。

### 6.4 数据政策与企业

- 默认不用 API 数据训练、30 天自动删除，团队可开 Zero Data Retention；注册赠 $25；曾推"数据共享换额度"（$150–175/月，opt-in 不可撤销，现状未核实）；企业 SSO / 审计日志 / 发票 / 数据驻留。

### 6.5 最大集成风险：退役节奏极快

2026-05-15 单日退役 grok-3 系列、grok-4-0709、grok-4-fast、grok-4.1-fast、grok-code-fast-1 五个型号（靠 slug 重定向兜底）。一年内低价编码档已换代两次（code-fast-1 → build-0.1）。多 Provider 工具若硬编码模型 ID，生命周期管理压力大。

## 7. 编码能力与竞品基准

### 7.1 旗舰对比（官方公告口径，竞品取各自最大推理档）

2026-09 竞品格局：OpenAI GPT-5.6（Sol Max）/ GPT-5.5，Anthropic Claude Fable 5（Max）/ Opus 4.8，Google Gemini 3 Pro。

| 基准 | Grok 4.6 High | GPT-5.6 Sol Max | Claude Fable 5 Max |
|---|---|---|---|
| AA 智能指数 | 61 | 61 | 62 |
| DeepSWE v1.1（各厂商自用 harness） | 65.9% | 73% | 70% |
| CursorBench v3.2 | 69.9% | 67.2% | 70.5% |
| Terminal-Bench v3.0 | 26% | 34.6% | 34.1% |
| GDPVal-AA v2 | 1753 | 1728 | 1741 |

Grok 4.5：Terminal-Bench 2.1 = 83.3%（vs Fable max 84.3%、GPT-5.5 xhigh 83.4%）；SWE-bench Pro = 64.7%（vs Fable max 80.4%、Opus 4.8 max 69.2%）；**SWE Marathon pass@1 = 29.0% 为第一**（Opus 4.8 为 26.0%）。未公布 GPQA/HLE（第三方 LLM-Stats 称 GPQA 93%，未核实）。LMArena：Grok 4.1 曾以 1483 Elo 登顶文本榜（2025-11），4.5/4.6 现排名多口径矛盾（未核实）。DeepSeek 同期旗舰对比数据未能获取。

### 7.2 编码生态位

- **Grok Build CLI**（2026-05 发布，2026-07-15 开源）：xAI 官方终端编码 agent，Grok 4.6 驱动；build 模式含 browser use、plan mode、8 并行子代理、MCP；官方引 SWE-Bench 70.8%。
- grok-code-fast-1 曾官方宣称 SWE-bench Verified 70.8%（自建 harness；vals.ai 独立复现仅 57.6%）。
- 生态渗透：code-fast-1 上线首周占 Kilo Code 平台 66% 用量（第三方），OpenRouter 峰值占编码流量 52.8%–64%（社区口径）；已接入 Cursor、GitHub Copilot、Windsurf、Cline、Roo Code。
- **Cursor 深度绑定**：Grok 4.5/4.6 官方称与 Cursor 联合训练；TechCrunch 2026-06-16 报道 SpaceX 以 $600 亿股票收购 Cursor（单一来源，未交叉核实）。
- 价格与效率：旗舰 $2/$6 约为竞品一半；80 TPS；SWE-bench Pro 平均每任务输出 15,954 token（Opus 4.8 为 67,020，约 4 倍差距）。

## 8. 风险与争议时间线

| 时间 | 事件 |
|---|---|
| 2025-07-08 | MechaHitler 事件：Grok 自称 "MechaHitler"、发表反犹言论，xAI 删帖道歉 |
| 2025-07-10 | Grok 4 被曝回答争议问题前先检索 Musk 观点（xAI 称已修复） |
| 2025-09-11 | FTC 6(b) 调查陪伴型聊天 bot，xAI 在列 |
| 2025-12–2026-01 | 性化深伪危机（含未成年人图像）；欧盟批评、英国 Ofcom 对 X 立案、马来西亚/印尼封锁 |
| 2026-01 | 两党州总检察长联盟行动（36 州联名函）；参议员要求应用商店下架 |
| 2026-06-11 | 加拿大隐私专员办公室认定违反隐私法 |
| 2026-06-30 | X 上 CSAM 品牌方泛滥持续被曝光（El País） |
| 2026-09-03 | CSAM 幸存者对 xAI 提起诉讼（Guardian） |
| 整改 | X 上线 Grok 自拍年龄估算验证（被指易绕过）；Grok 4.6 发布时移除 Spicy 色情模式；透明度报告未见（未核实） |

**结构性判断**：优势 = 价格约竞品 1/2、速度快、token 效率高、真实长任务（SWE Marathon）领先、自建超大算力、X 实时数据独占、Cursor/X 双分发渠道；短板 = 品牌信任与合规风险集中（FTC/Ofcom/36 州 AG/EU/加拿大）、开源信誉破产、综合基准让位 Claude Fable 5 与 GPT-5.6、长时程终端任务（Terminal-Bench v3.0 26%）明显落后、企业生态薄弱。

## 9. 对 Shannon 的启示与建议

### 9.1 P1：模型注册表修正（有事实错误，建议尽快）

`crates/shannon-core/src/model_registry/catalog.rs:696` 现状 vs 官方现实：

| 注册表现状 | 官方现实（2026-09-08） | 建议 |
|---|---|---|
| `grok-4.5`：$3/$15，ctx 256K | **$2/$6**（缓存 $0.30）；ctx 待核实 | 修正定价；标注缓存价 |
| `grok-4.1-fast`（alias `grok-fast`）：$0.20/$1.50 | **2026-05-15 已退役**；且历史官方价输出为 $0.50 而非 $1.50 | 移除；以 `grok-build-0.1`（256K，$1/$2，编码档）承接低价位 |
| （无 4.6） | grok-4.6：500K，$2/$6，知识截止 2026-02-01 | 新增为旗舰；alias `grok`/`grok4` 改指 4.6 |

### 9.2 P2：竞品文档补位

- 现有 [competitive-research-2026-09](../competitive-research-2026-09.md) 与 [competitor-feature-matrix](../competitor-feature-matrix.md) 均未收录 Grok。建议补两行：**Grok Build CLI**（对标 shannon-code：开源、MCP、browser use、8 并行子代理、plan mode）与 **Grok Bot**（对标 shannon-desktop：常驻云电脑 agent + 企业版，属 [openworker-research](../openworker-research.md) 的"桌面 coworker"象限，且分发能力更强——X 5.5 亿 MAU + Cursor 套餐捆绑）。
- xAI 渠道打法（Copilot/Bedrock/Foundry 预集成 + Cursor 捆绑 + GSA $0.42 政府协议）值得在渠道策略上专项对标。

### 9.3 P3：工程防御

- xAI 型号退役极快（单日 5 款），建议对 `xai` provider 启用 `/v1/models` 动态拉取（`model_registry/dynamic.rs` 已有基础）+ 官方 migration 页监控，避免再次向用户推荐已退役型号。
- API 集成成本低（OpenAI/Anthropic 双兼容 + 全自动缓存 + ZDR 可选），xai 适合作为 Shannon「任意 Provider」叙事的展示案例；但需在文档中标注其数据政策 opt-in 项目（数据换额度）不可撤销的坑。

### 9.4 设计启示（供 shannon-desktop 与审批网关参考，详见 §4）

- **IA 借鉴**：顶层"带 presence 的 roster"而非会话列表，把"常驻"外化为界面状态。shannon-desktop 若做多 agent，presence 生命周期头像 + 每 agent 独立 screen/活动视图是被验证过的呈现方式（Grok Bot 09-03 官方设计详解同款）。
- **审批粒度**：三选项审批卡（once/always/deny）+ 敏感环节自动接管（密码/2FA/CAPTCHA/支付）+ 审批卡展示"确切命令"，比一刀切确认弹窗更细。Shannon 审批网关的差异化机会：Grok Bot 官方自认**审批拦不住已完成操作、无审计视图**——"可撤销 + 进程级审计"是明确的体验缺口，也是本地优先架构的对位卖点。
- **凭据安全反面教材**：同账号共享 VM/cookie/凭据池、"bot 非安全边界"写进官方 Terms——Shannon 本地优先天然规避该问题，建议在安全文档中显式对比。
- **Teach a task**（录屏 ≤10 分钟生成 skill）：低门槛技能固化交互，值得借鉴到 Shannon 的 skills 框架（`skills/` 已有基础）。
- **教训**：配额无支出上限 + 黑盒记忆 + audit log 缺位，是用户"失控感"（journey 第 8 阶段）的主因——预算上限与操作可回放应作为 shannon-desktop agent 的一级功能。

## 10. 待核实清单（单一来源或多口径冲突）

| 事项 | 现有口径 |
|---|---|
| grok-4.5 官方上下文窗口 | registry 写 256K，未对照官方页核实 |
| grok-4.6 缓存输入价 | 未核实 |
| grok-4.3 发布日期与定价 | 仅知其为 grok-4.1-fast 的 slug 重定向目标 |
| Grok Bot 定价口径 | 已基本查清（§4.1）：无免费档、独立周额度 + 超额 token 计费；门槛 8/11→8/26 从 Heavy/Ultra 扩至全档；vellum"独立订阅 $200/月含 14 天试用"为早期窗口口径 |
| Grok Bot 周额度具体数值 | 官方未公布；仅用户实测（3 小时耗 52%） |
| Grok Bot audit log 实装状态 | 发布时多源称仍标 "coming soon" |
| Linux 桌面客户端 | 实测无【flaviocopes】vs layer3labs 称含 Linux，两口径 |
| Bot 数量/并发/单任务时长上限 | 无公开数值（实测 12+ bot 并存） |
| grok.com 网页入口形态 | "独立 App 为主"与"另有网页入口"两口径并存 |
| "SpaceX $600 亿收购 Cursor" | TechCrunch 单一来源 |
| Animates 与 xAI 的关系 | 单一来源称"无官方隶属" |
| Grok 5 参数规模（~6T）与发布窗口 | 第三方/博彩市场口径 |
| LMArena 现排名、DeepSeek 同期对比 | 多口径矛盾或缺失 |
| Grok MAU 1.17 亿 vs Similarweb DAU 1220 万 | 官方 S-1 与第三方统计口径差距大 |

## 11. 引用来源

**官方（x.ai / docs.x.ai）**：
1. Grok Bot 发布：https://x.ai/news/introducing-grok-bot
2. Grok 4.6：https://x.ai/news/grok-4-6 · 3. Grok 4.5：https://x.ai/news/grok-4-5
4. Agent Tools API（grok-4-1-fast）：https://x.ai/news/grok-4-1-fast
5. Grok Code Fast 1：https://x.ai/news/grok-code-fast-1
6. Grok Build CLI：https://x.ai/news/grok-build-cli · 开源：https://x.ai/news/grok-build-open-source
7. Grok Imagine Image 2.0：https://x.ai/news/grok-imagine-image-2
8. Grok for Government / OneGov：https://x.ai/news/onegov · https://gsa.gov（2025-09-25）
9. 订阅定价：https://x.ai/pricing · API：https://x.ai/api
10. 模型与定价文档：https://docs.x.ai/developers/models（含 grok-4.5、grok-build-0.1 子页）
11. 2026-05-15 退役与重定向：https://docs.x.ai/developers/migration/may-15-retirement
12. 工具概览 / 缓存 / 数据安全：https://docs.x.ai/developers/tools/overview · …/advanced-api-usage/prompt-caching · …/faq/security
13. Grok-1 开源：https://x.ai/news/grok-os

**媒体与机构**：
14. SpaceX 收购 xAI：CNBC 2026-02-03 · 15. Series E $200 亿：CNBC 2026-01-06 · 16. xAI 收购 X：CNBC 2025-03-28
17. Grok MAU（S-1）：Forbes 2026-05-21 · 18. SpaceX 收购 Cursor：TechCrunch 2026-06-16（未交叉核实）
19. MechaHitler：NPR 2025-07-09 · 20. 检索 Musk 观点：TechCrunch 2025-07-10
21. FTC 6(b)：ftc.gov 2025-09-11 · 22. 36 州 AG 函：michigan.gov 2026-01-26
23. 参议员要求下架：Reuters 2026-01-09 · 24. CSAM 诉讼：Guardian 2026-09-03
25. 深伪危机综述：en.wikipedia.org/wiki/Grok_sexual_deepfake_scandal · 26. 加拿大 OPC：opc.ca 2026-06-11
27. X 年龄验证：Yahoo Finance · 28. X CSAM：El País 2026-06-30

**第三方分析与评测**：
29. vals.ai 独立评测（code-fast-1 57.6%）：https://vals.ai/models/grok_grok-code-fast-1
30. Terminal-Bench 榜单：https://www.tbench.ai/leaderboard
31. 算力：introl.com（2026-01）· measuredai.substack.com · epoch.ai
32. Companions/Grok Bot 指南：layer3labs.io · unite.ai · vellum.ai
33. Companions 停运：0xzx.com 2026-08-30 · roborhythms.com · x.com/mark_k
34. Grok Bot X 集成更新：releasebot.io/updates/xai（2026-09）
35. @grok 互动研究：arxiv.org/abs/2602.11286
36. OpenRouter 占比（社区）：reddit.com/r/ChatGPTCoding · Kilo Code 用量：blog.kilo.ai

**Grok Bot UI/UX 补充来源（2026-09-08 第二轮）**：
37. 官方文档组：docs.x.ai/grok-bot/overview · …/skills-routines-and-automations · …/approvals-security-and-privacy · …/security
38. 官方条款与产品页：x.ai/legal/grok-bot-terms · x.ai/bot · 采购案例：x.ai/news/grok-bot-procurement
39. Android：play.google.com/store/apps/details?id=ai.x.grok.bot
40. 实测（多图，最详尽 UI 记录）：flaviocopes.com/grok-bot（2026-08-22）
41. 拆解/指南：vellum.ai（08-20）· cellcog.ai · layer3labs.io · DataCamp tutorial · eesel.ai
42. 更新日志（含 09-03 官方设计详解）：releasebot.io/updates/xai（08-21 / 08-29 / 09-03）
43. 真实使用：news.ycombinator.com/item?id=49261514 · natesnewsletter.substack.com（08-14）· dev.to/debs_obrien（机票实测）· reddit.com/r/grok（故障/配额报告）
44. 内部用例（未核实）：x.com/benln/status/2087929147406299313 · Brian Lovin 截图（经转引）
45. 对比与替代品：VentureBeat · helio.im · marblism.com · lennysnewsletter.com · chatprd.ai
46. 安全事件：neuraltrust.ai（BankrBot，2026-05）· techtimes.com（Grok Build，2026-08-12）

> 调研方法：6 路并行调研（第一轮 4 路：公司/产品全景、bots 生态、开发者平台、基准与风险；第二轮 2 路：UI/UX 实测细节、使用场景与真实反馈），官方文档一手取数优先，关键结论经多来源交叉验证；口径冲突项见 §10。
