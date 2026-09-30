# Shannon Desktop 办公场景竞品深度调研报告

**调研日期**: 2026-09-29
**版本**: v2（§10 改进方案已按对抗性审查+复审修订，决策 Q1-Q6 已裁决，B4 移除——审查与复审见 [docs/reviews/2026-09-29-office-plan-adversarial-review.md](../reviews/2026-09-29-office-plan-adversarial-review.md)）
**基线代码**: dev @ 02214380
**视角**: 知识工作者 / 办公生产力（文档 · 表格 · 幻灯片 · 邮件 · 会议 · 办公自动化），区别于既有报告的开发者 Agent 编排器视角
**关联文档**: [desktop/COMPETITIVE-ANALYSIS.md](../../desktop/COMPETITIVE-ANALYSIS.md)（2026-06-13，开发者视角）、[docs/competitive-research-2026-09.md](../competitive-research-2026-09.md)（2026-09-05，开发者视角）、[docs/competitor-feature-matrix.md](../competitor-feature-matrix.md)
**调研方法**: 3 路代码库盘点（引擎能力 / Desktop UI / 路线图文档，结论带 file:line）+ 4 路竞品网络调研（Office 原生套件 / ChatGPT+Claude 桌面端 / 中国生态 / Agent 交付型产品），竞品结论附来源 URL
**图例**: ✅ 完整 · ⚠️ 部分/有缺口 · ❌ 缺失

---

## 0. TL;DR

**行业事实**：2025H2–2026，办公 AI 完成了从「侧边栏聊天助手」到「**桌面 Agent + 成品文件交付**」的形态收敛。ChatGPT（Work）、Claude（Cowork+Skills）、WPS 灵犀专业版、Kimi Work、豆包工作在 2026 年 2 月–9 月间密集发布独立桌面端，全部指向同一范式：**聊天下指令 → Agent 后台跑 → 交付可编辑的 docx/xlsx/pptx → 回写用户既有协作系统**。与此同时，PPT 生成收敛为「大纲确认→模板→逐页生成+局部编辑」三段式，表格收敛为「列即 Agent」（Genspark AI Sheets / 飞书字段捷径 / 钉钉 AI 表格 / Excel `=COPILOT()`），会议收敛为「录音→纪要→行动项→自动跟进」流水线。

**Shannon 现状**：自动化底座（cron / 自然语言定时 / webhook 触发 / Triage 收件箱 / 执行历史）达到一线水准；但**办公内容链路（读文档→理解→生成→预览→导出→回写）几乎全部断裂**。`desktop/README.md:5-8` 宣传 "AI workspace for knowledge workers — drafts email, summarizes docs"，而仓库内邮件能力为零实现、docx/xlsx/pptx 解析为零实现、拖入的 Office 附件模型根本看不到内容。

**最刺眼的 5 个问题**：

| # | 问题 | 严重度 |
|---|---|---|
| P1 | **Office 附件黑洞**：docx/xlsx/pptx 可拖入聊天但只生成展示 chip，不产生模型内容块、无任何不支持提示——用户以为 AI 读到了，实际什么都没发生（`desktop/src/commands.rs:703-720`、`commands_files.rs:155-157`） | P0，信任级 |
| P2 | **办公文档解析为零**：引擎无 docx/xlsx/pptx/csv 解析（无任何相关依赖），PDF 仅 pdftotext 头部 50KiB、无 OCR/表格/页范围（`crates/shannon-tools/src/file/read.rs:197-217`） | P0 |
| P3 | **文档生成链路是"暗物质"**：3 个 bundled 办公技能存在但依赖宿主 python3、无 UI 进度展示、Welcome 文档技能区被 `DOCUMENTS_SKILLS_AVAILABLE=false` 整体隐藏、目录里 6 个办公技能安装后是空壳 stub（`welcome/constants.ts:89`、`Skills.tsx:154-155`） | P0 |
| P4 | **数据源半成品且 UI 撒谎**：IMAP/Obsidian 查询直接抛 `Unknown data source kind`，UI 却挂 "Verified" 徽章；查询结果无「注入会话」通道，数据源与聊天完全割裂（`data_source_fetchers/mod.rs:99`、`DataSources.tsx:426-435`） | P1 |
| P5 | **会议场景完全空白**：录音/转写/纪要/行动项全链路缺失，而这已是 2026 年全家桶竞品（Copilot/ChatGPT/飞书/钉钉/Notion/WPS）的标配入口场景 | P1 |

**建议路线**（详见 §10）：不要追 Office 套件（打不过 Copilot/Gemini 的原生集成），走「**本地优先的办公 Agent**」——把 Shannon 已有的世界级自动化底座与（补齐后的）文档生成技能组合起来，主打「定时/事件触发 → Agent 干活 → 交付真 Office 文件」的无人值守办公，这是 Copilot/Gemini 结构上做不了、而 Shannon 架构天然适合的生态位。

---

## 1. 调研范围

**竞品（4 组 14 家）**：
- **A. Office 原生套件**：Microsoft 365 Copilot、Google Workspace with Gemini
- **B. AI 助手桌面端**：ChatGPT 桌面端（含 Work）、Claude 桌面端（含 Cowork / Skills / Artifacts）
- **C. 中国生态**：WPS 灵犀（专业版）、Kimi（Work / PPT）、豆包工作（+飞书 8.0）、飞书（知识问答 / AI 表格）、钉钉（AI 表格 / AI 助理）、专业 PPT 工具（讯飞智文 / AiPPT / MindShow / 百度文库）
- **D. Agent 交付型**：Manus、Genspark、Gamma、Notion（3.0 Agents）；附带自动化 Agent（Zapier / n8n / Lindy）

**自查面**：shannon-mono dev 分支——crates 引擎能力、desktop UI 表面、skills/routines、路线图文档。

---

## 2. 行业趋势总览（2025H2 → 2026-09）

1. **「交付物优先于回答」成共识**。ChatGPT Work 的官方定位就是「把目标变成完成品」；Claude Cowork 交付 deck/spreadsheet/memo；灵犀专业版 15 分钟交付一套文件（HTML+Word+PPT+Excel）。聊天文本不再是终点，**文件卡片（预览+下载/保存/发送三连）才是**。
2. **生成保真 Office 文件的标准解法 = 技能包 + 沙箱确定性脚本**。Anthropic Agent Skills（2025-10，2025-12 开放为 agentskills.io 标准）：SKILL.md + 脚本，在 code execution 沙箱跑 openpyxl/python-docx/OOXML 操作，产出带公式、带样式的真文件；渐进式披露（启动只载 name+description → 命中任务读正文 → 按需读脚本）。**Shannon 的 bundled skills（docx-report/xlsx-table/ppt-outline）与此完全同构**——机制对了，但缺沙箱保障、缺 UI 过程展示、缺入口曝光。
3. **PPT/长文档生成收敛为三段式**（Gamma 定标，全行业跟随）：大纲（可编辑/拖拽）→ 模板（可换、换模板不重生成）→ 逐页生成+局部编辑。两道人工确认闸口（大纲、模板）是防跑偏的关键。
4. **表格成为批量 AI 的最佳载体**：「列即 Agent」（Genspark）、字段捷径（飞书）、AI 字段 Agent（钉钉 100+）、`=AI()`/`=COPILOT()` 公式（Google/Microsoft）——AI 嵌入表格既有心智模型，用户零学习成本。
5. **会议流水线成为超级入口**：ChatGPT Record（≤120min→writing block）、Notion Meeting Notes（抓系统音频免 bot）+自动触发 Agent、Meet「Take notes for me」（2026-08 支持线下）、Teams Intelligent Recap（无需录制）、飞书妙记、钉钉听记、WPS 听记。「纪要→行动项→任务→跟进邮件」的自动接力是粘性核心。
6. **连接器从只读走向回写**：ChatGPT 直接更新 Google Drive 源文件（2026-08）、Claude for Excel/Word/Outlook 加载项在源应用内改、豆包工作把成果写回飞书云文档并继承权限。「不产生平行副本」是企业采纳的分水岭。
7. **信任设计三板斧**：过程透明（步骤流/电脑直播/回放）、预览-确认（Excel 预览卡 Apply、Notion 行级 diff 审批、Manus 命令逐条审批）、引用溯源（Copilot deep citations 段落级、飞书知识问答引用原文）。
8. **上下文即护城河**：豆包工作登录飞书后直接读群聊/文档/妙记/日程/邮件，用户零上传；Copilot Notebooks/NotebookLM 把「来源集合」做成一等公民。谁能自动拿到上下文，谁就赢。
9. **自动化内卷化**：ChatGPT Tasks 支持 webhook 触发+任务分享、Manus Automations（Schedule/Trigger/NL 三态）、Notion 会议后自动触发——「定时/事件 → Agent 干活 → 写回办公系统」正从独立产品（Zapier/Lindy）的功能变成聊天产品原生功能。**Shannon 的自动化底座恰在此赛道上且完成度高，是真实资产。**
10. **定价锚点**：国际 $20-30/月主流；中国个人 AI 办公集中在 20-50 元/月、130-250 元/年；「生成免费、导出/高级模板收费」是主流转化钩子。

---

## 3. 竞品逐项扫描

### 3.1 Microsoft 365 Copilot —— 深度嵌入工作流的「套件内 AI」

**办公功能**（2026-09 现状）：
- **Word**：Draft with Copilot 引用最多 20 个来源（文件/邮件/会议记录）；打开即摘要（Understanding 区+建议追问）；页边距图标+选区快捷操作；音频概览；Writing Coach。
- **Excel**：「视觉轮廓」先确认数据范围→公式列/条件格式（预览卡+Apply）→图表/透视；Python in Excel 高级分析（Think Deeper）；`=COPILOT()` 单元格函数；Edit with Copilot（原 Agent Mode，2026-03 GA，聊天面板显示步骤清单）。
- **PowerPoint**：从 prompt/文件（≤5 个）/Excel 生成整套演示；Designer 模板画廊入侧栏；组织模板+品牌资产库；narrative builder（大纲→正文页）；右键 Explain 任意元素；演讲备注生成；约 40 语言翻译。
- **Outlook**：语气+长度滑杆起草；线程 Summary 横幅；Coaching 质量反馈；Schedule with Copilot（邮件线索一键成会）；Prepare 会前简报；自然语言收件箱 triage（2026-03）。
- **Teams**：Intelligent Recap（无需录制，按说话人/话题导航）；Interpreter 实时翻译。
- **横切**：Copilot Pages（聊天答复→协作画布→一键转 Word/PPT）；Copilot Notebooks（来源集合知识工作区+Audio Overview）；Agent 四级创建（SharePoint 零代码→Agent Builder 对话式→Copilot Studio→代码）；Work IQ 统一 Work/Web 模式；2026-09 对话内直接调起 Office 应用动作。

**UI 模式**：常驻应用侧栏 pane；Context IQ（`/` 唤起文件/人/会议）；引用溯源三层（内联来源 pill→deep citations 段落级下钻→hover glance 卡）；「AI 提议、人确认」的预览-Apply 范式。

**定价**：企业 $30/席/月（年承诺）；Copilot Chat 企业免费+credits PAYG（$0.01/credit）；消费者 M365 Premium $19.99/月（2025-10 发布，Copilot Pro 停售并入）。

**对 Shannon 的启发**：① 预览-确认（diff→apply）应成为 Shannon 文件操作的标准范式——现有 8 档权限模式是现成载体；② 引用溯源（回答→本地文件段落锚定）是企业/个人共同刚需；③ 「来源集合」（Notebooks）值得映射为 Shannon 的 Project+数据源组合。

**来源**：[Release Notes](https://learn.microsoft.com/en-us/copilot/microsoft-365/release-notes)、[Copilot Pages](https://support.microsoft.com/zh-cn/microsoft-365-copilot/how-microsoft-365-copilot-pages-works)、[Copilot Chat 发布公告](https://www.microsoft.com)、[Python in Excel](https://support.microsoft.com/en-us/office/python-in-excel)

### 3.2 Google Workspace with Gemini —— 统一侧栏 + 「AI 即公式」

**办公功能**：统一 side panel 横跨 Docs/Sheets/Slides/Gmail/Drive/Meet（上下文感知，动作按钮化：Write/Refine/Summarize）；Sheets `=AI()` 公式（2025-07）；Slides 整套生成+可编辑图表（2026-03）；Gemini app Canvas 生成演示/文档并落回 Workspace、直接输出 PDF/Word/Excel；Meet「Take notes for me」实时生成共享 Doc+行动项（2026-08 支持线下会议）；NotebookLM/Gemini Notebook（来源集合+Audio/Video Overview+思维导图）；Gems 免费自定义 persona；Workspace Studio 自然语言编排跨应用流。

**UI 模式**：同一面板形态跨 7+ 应用；「AI 做成原语而非功能」（`=AI()` 公式）；产物落在协作文档而非聊天记录。

**定价**：AI 并入全部 Workspace 商业订阅（提价 ~$2.4/席）；个人 Google AI Pro $19.99 / Ultra $249.99；Gemini Enterprise $21/$30。

**对 Shannon 的启发**：① 高频动作按钮化（Write/Refine/Summarize）优于自由 prompt——Shannon 的斜杠命令/skills 应在聊天输入框做「动作芯片」曝光；② 「研究工作区（来源集合）」与「自动化流」是聊天之外的两种持久形态，Shannon 有 routines 缺前者；③ 产物默认落到用户的文件系统/协作系统。

**来源**：[Workspace AI 总览](https://workspace.google.com/solutions/ai/)、[Gemini Workspace updates March 2026](https://blog.google/products-and-platforms/products/workspace/gemini-workspace-updates-march-2026)、[Sheets =AI()](https://blog.google)、[Meet 笔记](https://support.google.com/a/users/answer/9283046)

### 3.3 ChatGPT 桌面端（含 Work）—— 伴随窗口 + 交付物卡片

**办公功能**（2026-09 现状）：
- **Work**（2026-07-09，GPT-5.6）：「目标→完成品」agent，产出 sheets/slides/docs/网页，任务中可插话纠偏/审批敏感操作；桌面端经授权可操作**本地文件与桌面应用**，内置浏览器。
- **文件**：50+ 格式上传解析（单文件 512MB/2M tokens）；**File Library**（2026-03）统一管理上传物与生成物，分档存储（Free 500MB→Pro 100GB）。
- **Office 加载项三件套 GA**：Excel+Google Sheets（2026-05）、PowerPoint（2026-07）、Word（2026-09-17）——侧边栏内对话式改表/改文档，**全档位可用（限量）**。
- **连接器**：Gmail/Outlook 聊天中直接发邮件（2026-06）；Google Drive 进 Library 且**直接回写源文件**（2026-08）；MCP 自定义连接器。
- **Record**：macOS 录会议≤120min→转写+摘要→存 writing block。
- **Scheduled Tasks**：侧栏管理页+webhook 事件触发（Gmail/Slack/GitHub PR）+任务分享链接；Free 3 个/Plus ~10 个。

**UI 模式**：主窗口（Chat/Work/Codex 三合一）+伴随窗口（Alt+Space/Option+Space 呼出、置顶小窗、截图/Appshots 前台应用窗口直发会话）；Work 执行时步骤流全透明；deliverable 卡片（预览+下载/存 Library/发邮件）；Canvas 已退役为全屏 writing blocks。

**定价**：Free（加载项可用限量、3 个任务）/ Go ~$8 / Plus $20（Work 用量计费）/ Pro $100、$200 / Business $25-30。

**对 Shannon 的启发**：① **伴随窗口是桌面端第一入口**——Shannon 已有全局快捷键+托盘，缺「置顶小窗+截图问答」形态；② 文件库（上传物+生成物统一管理、计存储配额）是 Shannon 附件「用完即弃」现状的反面教材；③ 事件触发（webhook）Shannon 后端已有，缺办公触发器模板（收到邮件/IM 消息时…）。

**来源**：[ChatGPT Release Notes](https://help.openai.com/en/articles/6825453-chatgpt-release-notes)、[ChatGPT Work 公告](https://openai.com/index/chatgpt-for-your-most-ambitious-work)、[Record](https://help.openai.com/en/articles/11487532-chatgpt-record)

### 3.4 Claude 桌面端（Cowork / Skills / Artifacts）—— 技能开放标准 + 本地文件代理

**办公功能**：
- **Agent Skills**（2025-10-16，2025-12 开放标准 agentskills.io）：SKILL.md+脚本文件夹，三层渐进式披露；官方 docx/pptx/xlsx/pdf 四技能在 code execution 沙箱**跑确定性脚本**生成带格式/公式/样式的真 Office 文件；思考链可见「用了哪个 skill、执行了哪些步骤」（生成 PPT 展示分步进度：读模板→建大纲→逐页排版→打包）；完成后消息内下载 chip（可转存 Drive）。**全档位含 Free 可用（配额随档位）**。
- **Cowork**（2026-01 preview→2026-04 GA→并入主品牌）：授权本地文件夹→多步任务→交付 deck/doc/xlsx；任意 cadence 定时调度；并行大任务；跨设备接续；权限阶梯 connectors→browser→computer use，每应用单独授权。
- **Office 加载项**：Claude for Excel（2026-03 GA，主打金融建模审计）+Outlook+Word。
- **Artifacts**：side-by-side 面板；2026 新增 **Slides 类型**（逐页编辑+放映+导出 pptx/PDF）；发布 URL+版本化回滚+Remix fork。
- **Projects/Connectors/Plugins**：项目指令+知识库；Google Drive/Gmail/Calendar/Outlook/M365 官方连接器+远程 MCP 自定义连接器（Free 起可用）；Plugins=skills+connectors+sub-agents 打包（如 Legal/Finance 角色），Enterprise 私有市场。

**UI 模式**：「聊天里长出文件」的克制式 UI（步骤化进度→下载 chip）；Artifacts 版本化发布；Cowork 计划先行+实时可视化+可中断。

**定价**：Free（文件创建可用、配额低）/ Pro $17-20 / Max $100、$200 / Team $20/席。

**对 Shannon 的启发**（最高相关度——Shannon 技能系统与之同构）：
1. **Shannon 已经兼容 SKILL.md 格式且联邦上游 anthropics/skills**（`desktop/src/extensions/skill_catalog.rs:36-56`）——这是独一无二的结构性优势，应把「办公技能包」做成一垒资产：把 docx/xlsx/pptx 技能从「宿主 python3 依赖+隐藏入口」升级为「sidecar Python 沙箱+官方技能包+进度展示+下载 chip」。
2. 生成过程的分步进度展示（读模板→建结构→逐页生成→打包）是 Shannon 聊天 UI 缺的最后一环。
3. .mcpb 一键安装 Shannon 已支持（`extensions/mcpb.rs`），但注意 2026-02 出现过 .mcpb 零点击漏洞——分发需签名/审核/权限声明。

**来源**：[Agent Skills 公告](https://claude.com/blog/skills)、[anthropics/skills](https://github.com/anthropics/skills)、[Cowork 产品页](https://claude.com/product/cowork)、[MCPB](https://github.com/modelcontextprotocol/mcpb)

### 3.5 WPS 灵犀（含专业版）—— 划选改写 + Agent 交活

- Canvas 双屏协同：左侧文档右侧 AI，**划选文字+自然语言指令+结果原地落盘**（保留修改痕迹）——文档类 AI 黄金交互。
- AI PPT 流：主题/文档→大纲（对话中边聊边改）→逐页美化渲染→模板/精调→导出。
- **灵犀专业版**（2026-07）：独立桌面端（刻意不塞进 WPS），Agent 交活 ~15 分钟交付一套文件（HTML+带图表 Word+统一风格 PPT+带公式 Excel）。
- AI 听记：语音速记/同传字幕，10 万字速记自动整理归档。
- 定价：大会员 188 元/年；AI 会员 248 元/年；专业版 48 元/月按 token。
- **启发**：「划选+指令+原地落盘」依赖「在应用内」的形态，Shannon 无法复制；但「Agent 一次交一套成套文件」与「听记→纪要→归档」两条 Shannon 都可以且应该做。

**来源**：[金山办公连发两款 AI 产品（新浪财经）](https://finance.sina.com.cn)、[WPS 社区教程](https://bbs.wps.cn)、[凤凰科技](https://tech.ifeng.com)

### 3.6 Kimi（Work / PPT）—— 长文档 + 桌面 Agent

- 长文本：200 万字上下文、单文件 100MB、50 文件批量——长文档解析是天然入口。
- **Kimi Work**（2026-06）：本地文件读写/文件夹挂载/300 Agent 并行/Cron 定时（最长自主运行 24h）/浏览器 WebBridge 复用登录态；执行过程透明（工具调用、进度、文件改动可见，文件路径可点击预览）；主打「可验证终态」（文件清单全绿）。
- Kimi PPT：一级导航入口；文件→MD 大纲（可对话修改）→模板→PPTX 导出。
- **启发**：① MD 大纲作为「人机确认中间件」成本低可控强——Shannon 的 /ppt-outline 已产 MD 大纲，缺的只是后续两段（模板+逐页生成）；② 「可验证终态」的完成标准展示适合 Shannon 的 Triage/History 已有基础。

**来源**：[Kimi 官网](https://www.kimi.com)、[Kimi Work 实战（CSDN）](https://bbs.csdn.net)

### 3.7 豆包工作 + 飞书 8.0 —— 上下文即护城河

- 豆包工作（2026-08-25 独立上线；9-15 与飞书 8.0 原生融合）：登录飞书后**直接读群聊/云文档/妙记/日程/多维表格/邮件**，用户零上传零复述；交付飞书云文档原生格式（继承权限、@协同），「从飞书来→AI 处理→回飞书→成为下次上下文」双向闭环。
- **局部修改「指哪改哪」**：框选已生成文档/表格/PPT/网页任意部分直接改，无需整篇重生成。
- Agent 有独立组织身份（入群/参会/读文档），管理员统一审计权限/成本/执行记录。
- 飞书知识问答：开箱即用（不要求先建知识库）、答案带引用来源可跳转原文；**AI 表格/字段捷径**：列级 AI 批量处理（可接 DeepSeek 等）。
- **启发**：Shannon 打不了「企业全量上下文」这张牌，但**本地版故事成立**：本地文件夹+Obsidian+IMAP+浏览器（已登录态）就是个人版上下文护城河，且隐私叙事（密钥不出本机）更强。

**来源**：[豆包工作上线（新浪财经）](https://finance.sina.com.cn)、[飞书与豆包工作协同亮相（搜狐）](https://m.sohu.com/a/1076440491_313745)、[产品经理社区分析](https://www.woshipm.com/ai/6456338.html)

### 3.8 钉钉 AI 表格 / AI 助理 —— 对话生成整个应用

- AI 表格「每个单元格都是 AI 入口」、100+ 字段 Agent、对话问数；**对话生成完整应用**（表+自动化工作流+仪表盘）；助理创建向导五步（身份→指令→知识库→技能→发布范围）。
- **启发**：「对话→生成完整产物（而非一段文字）」的价值密度思维，适用于 Shannon 的 routine 模板（一句话装好一个办公自动化流）。

**来源**：[钉钉 8.0 发布（新浪财经）](https://finance.sina.com.cn)、[白鲸技术栈](https://jishuzhan.baijing.cn)

### 3.9 专业 PPT 工具（讯飞智文 / AiPPT / MindShow / 百度文库）

- 共同交互范式：输入（一句话/文档/大纲/MD）→ 大纲编辑 → 模板 → 生成 → **换模板不重生成**（百度文库/AiPPT 的刚需共识）→ 局部编辑 → 多格式导出（PPTX/PDF/PNG/长图）。
- 模板不是皮肤而是**版式约束系统**（占位符+自动适配防溢出）。
- 定价钩子：「生成免费、导出收费」；百度文库智能 PPT 访问量全球第一（月狐数据 2025-06）。
- **启发**：Shannon 的 /ppt-outline 产出的是「极简纯文本框 slides」best-effort（`skills/ppt-outline/SKILL.md`），与行业水准差距最大的单一功能点；但「模板=版式约束系统+换模板不重生成」的机制值得照抄进技能设计。

**来源**：[AiPPT 官网](https://www.aippt.cn)、[SegmentFault 横评](https://segmentfault.com)、[51CTO 模板实测](https://www.51cto.com)

### 3.10 Manus —— 过程可视化天花板

- 左聊天流+右「Manus 的 Computer」实时直播窗口+todo 计划面板；**replay 回放+分享链接**（别人可看 Agent 工作过程）；Wide Research 数百并行 Agent 网格；任务中插话 steer；.pptx 模板导入；Automations（Schedule/Trigger/NL 三态）；Desktop 本地授权+命令逐条审批（Allow Once/Always）。
- **启发**：① Shannon 的 Mission Control 看板 + 后台任务已有「过程」的骨架，缺「回放/分享」与「终端/浏览器画面直播」级透明度；② 「控制感三支柱」=任务前成本预估+运行中 todo 可见+可中断插话，Shannon 的 History 有成本、缺前两者。

**来源**：[Manus Slides 文档](https://manus.im/docs/features/slides.md)、[Automations](https://manus.im/docs/automations.md)、[Desktop](https://manus.im/docs/features/desktop.md)

### 3.11 Genspark —— 列即 Agent

- AI Workspace 6.0（Super Agent+AI Slides/Sheets/Docs/Developer/Calls）；**AI Sheets「列即 Agent」**：列写指令对整列逐行执行（搜索富化/分类/生成），行级可触发 AI Calls 真人电话；一个 prompt 同时出 doc+deck+网页；导出 PPTX/Google Slides。
- **启发**：批量结构化任务（名单富化、批量翻译、批量摘要）在 Shannon 里目前只能靠 Bash 循环——「表格视图承载批量 Agent 执行（每行=任务，列=Agent 操作）」是一个与 Mission Control 互补的潜在页面。

**来源**：[Genspark 官网](https://www.genspark.ai)、[Taskade 评测](https://www.taskade.com/blog/best-genspark-alternatives)

### 3.12 Gamma —— 生成前对齐范式定标者

- **三段式确认**：大纲（逐条可编辑/删除/拖拽重排）→ 主题（30+，AI 可代选）→ 逐卡生成（~30 秒）；卡片抽象（非页）便于局部重生成；双轨编辑（点击直改 + Edit with AI/Agent 对话式整 deck 操作）；从已有 PPT/Drive 文件**反提品牌主题**（Brand Kit）；发布链接+viewer analytics（谁看/哪页/停留）；导出 PPTX/PDF/PNG。
- **启发**：Shannon 若做文档类生成交互，三段式是标准答案；「从用户已有文件反提风格」在本地场景可映射为「从用户的 docx/pptx 模板反提样式」。

**来源**：[Gamma 3.0 公告](https://gamma.app/insights/introducing-gamma-3-0)、[数据入 deck 文档](https://help.gamma.app/en/articles/15715171-how-can-i-get-my-data-into-gamma-decks-using-an-llm)

### 3.13 Notion 3.0 Agents —— 行级 diff 审批金标准

- AI 重构为 Agents（单次≤20min 多步任务）；**Agent 建议行级修改、人批 diff**（2026-08）——「审批式写权限」的信任设计范本；双轨写作 AI（内联 AI+侧栏 Agent 面板）；Meeting Notes 抓系统音频（无 bot 入会）→纪要→**自动触发 Custom Agent**（2026-07：更新 tracker、发 Slack recap）；**Agent Skills 团队库且可导出 SKILL.md 给 Claude Code/Codex/Cursor**（生态互通）。
- **启发**：① Shannon 对文件写操作已有权限体系，但缺「行级 diff 建议→批准→应用」的轻量 UI（`get_file_diff` 命令在、无 diff 视图，`commands_files.rs` + 2026-06 报告 §3.4 同样结论）；② Notion 验证了 SKILL.md 生态互通的价值——Shannon 应把自己的技能资产设计为可导出/可导入。

**来源**：[Notion Releases 2026-09-15](https://www.notion.com/releases/2026-09-15)、[2026-08-28 行级建议](https://www.notion.com/releases/2026-08-28)

---

## 4. UI/交互设计模式横向对比

| 模式 | 代表实现 | 一句话本质 | Shannon 现状 |
|---|---|---|---|
| **交付物卡片** | ChatGPT deliverable 卡、Claude 下载 chip、灵犀成套文件 | 聊天的终点是文件不是文本 | ❌ 生成的 md/xlsx 落盘后只有路径，无卡片/无下载动作/无转存 |
| **三段式生成确认** | Gamma（定标）、WPS/Kimi/AiPPT 全跟随 | 大纲→模板→逐页，两道人工闸口 | ⚠️ /ppt-outline 只产大纲，无后续两段、无确认流 |
| **过程可视化** | Manus 电脑直播+replay；Claude/ChatGPT 步骤流 | 让用户看见 Agent 在干嘛 | ⚠️ 工具调用折叠块有了（MessageBubble.tsx:861-930），缺技能执行分步进度、缺回放 |
| **预览-确认** | Excel 预览卡 Apply；Notion 行级 diff；Manus 逐条审批 | AI 提议、人确认 | ⚠️ 8 档权限模式+审批弹窗在；`get_file_diff` 在但无 diff UI（docs/plans/2026-09-25 报告亦确认） |
| **侧栏/伴随窗口** | M365/Google 侧栏 pane；ChatGPT Alt+Space 小窗 | AI 靠近工作现场 | ⚠️ RightDock 有（上下文/计划/预览/Diff/artifact），无全局呼出小窗、无截图问答 |
| **Artifacts 预览面板** | Claude Artifacts（版本/发布/Remix） | 产物 side-by-side 迭代 | ⚠️ RightDock artifact 标签支持 html/svg/mermaid/长文档（detectArtifact.ts:136-168），不支持 PDF/Office/CSV/图片、无版本、无发布 |
| **列即 Agent 表格** | Genspark AI Sheets、飞书字段捷径、钉钉、`=AI()` | AI 嵌入表格心智模型 | ❌ 无 |
| **引用溯源** | Copilot deep citations、飞书知识问答带来源 | 答案可验证 | ⚠️ 数据源/MCP 有来源元数据，聊天回答无统一引用锚定 UI |
| **会议流水线** | Record/妙记/听记/Meeting Notes→纪要→行动项→自动跟进 | 语音是办公超级入口 | ❌ 全链路缺失（语音输入 2026-08 已落地，仅限对话输入） |
| **回写源系统** | Drive 源文件更新、Office 加载项、豆包→飞书 | 不产生平行副本 | ❌ 只有本地文件写入 |
| **模板体系** | 组织模板/Brand Kit/反提主题/20 万模板库 | 版式约束+品牌一致性 | ⚠️ routine 模板 12 个（全研发向）+schedule 模板 5 个（全研发向）；文档/幻灯片模板零 |

---

## 5. User Journey Map：办公用户端到端走查（竞品 vs Shannon）

> 走查方法：按「用户意图→动作→系统行为→结果」逐步追踪 Shannon 代码路径，标注断点。

### J1 「把这份数据表分析一下」
- **竞品**：拖入 xlsx→自动解析（结构+数据预览卡）→对话式分析（Excel 内加载项直接改簿，或 Code Interpreter 出图表）→交付带公式/图表的成品。
- **Shannon 实际路径**：拖入 .xlsx → 生成展示 chip（`commands.rs:703-720`，仅 FileAttachment 元数据）→ **模型收到 0 个内容块** → 若模型主动用 Read 工具读该路径 → 二进制嗅探拒绝 `type:"binary"`（`read.rs:197-217`）→ 唯一活路是模型自己想到用 Bash+python3+openpyxl（**依赖宿主恰好装了**，无任何引导/技能兜底——data-analysis 技能是空壳 stub）。UI 全程无「此类型不支持」提示。
- **判定**：💔 断裂。用户输入被静默吞掉，这是信任级缺陷。

### J2 「帮我做一份季度汇报 PPT」
- **竞品**：三段式（大纲确认→模板→逐页生成）→局部改→导出 pptx，全程 2-3 分钟（Gamma ~30 秒生成）。
- **Shannon 实际路径**：模型可能命中 /ppt-outline → 产 Markdown 大纲（✅ 这段是好的）→ best-effort 生成「极简纯文本框 slides」的 pptx（`skills/ppt-outline/SKILL.md` 明示降级规则）→ 依赖宿主 python3、无模板体系、无预览、无逐页编辑、无「换模板不重生成」。若宿主无 python3 → 只剩 Markdown。
- **判定**：⚠️ 有骨架（大纲先行）无肌肉（模板/渲染/预览/迭代），与行业差距最大的单一功能。

### J3 「把昨天的会整理成纪要」
- **竞品**：录音/系统音频→转写+说话人→纪要+行动项→自动建任务/发跟进（Notion/ChatGPT/飞书全链路）。
- **Shannon 实际路径**：无录音能力、无转写管线、无纪要模板。用户只能手动拖入录音文件→大概率无处安放（音频非图片/PDF/UTF-8→黑洞）。语音输入（2026-08 落地）只覆盖「对话时说话」。
- **判定**：❌ 全链路缺失。

### J4 「帮我处理邮件」
- **竞品**：Copilot/ChatGPT（聊天中读信+直接回信）、Hermes/Cowork（MS365 连接器）。
- **Shannon 实际路径**：`desktop/README.md:6` 宣传 "drafts email"；Extensions 页有 IMAP 数据源表单→安装成功→显示 **Verified 徽章**→查询时 dispatch 抛 `Unknown data source kind`（`data_source_fetchers/mod.rs:99`）；`inbox-triage-hourly.toml` routine 依赖同一条断路。SMTP 全仓零命中。
- **判定**：💔 宣传与实现断裂+UI 撒谎，双重伤害。

### J5 「每周五自动给我出周报」
- **竞品**：ChatGPT Tasks（cron+webhook+分享）、Cowork（任意 cadence→交付文件）、Manus Automations（结果写回 Notion/Slack）。
- **Shannon 实际路径**：后端最完整的一段——cron/NL→cron 预览/webhook 触发/日历与 DAG 视图/Triage 收件箱/History 含成本 token（`scheduled_commands.rs:571-1418`）。但：模板全是研发场景（ScheduleTemplates.tsx:31-82 五个模板无一办公）；产出是 Markdown 文本进 Triage，**不能落成 docx/xlsx 成品**；结果路由无邮件/IM 通道（SCHEDULED-FIX-PLAN Sprint 5 规划了 Slack/Email adapter，未落地）。
- **判定**：⚠️ 引擎 Grade A，交付 Grade C——「最后一公里」恰好断在办公交付物上。

### J6 「基于我的笔记回答问题」
- **竞品**：Projects/Notebooks（来源集合常驻上下文）、飞书知识问答（带引用）。
- **Shannon 实际路径**：Obsidian 数据源表单可用→查询报错（同 J4）；查询面板结果卡只有外链、无「注入当前会话」按钮（DataSourcesQuery.tsx:207-251）；数据源与 Chat.tsx/AppContext 零关联。Memory 系统面向对话记忆而非文档集合。
- **判定**：💔 断裂（且已被 2026-09-25 memory/doc/RAG 审查独立证实）。

### J7 「把结果做成 Word 发出去」
- **竞品**：下载 docx（Claude 技能）、写回 Drive（ChatGPT）、转 Pages/Word（Copilot）。
- **Shannon 实际路径**：会话导出仅 Markdown/JSON（`export.rs:215-244`、UI 主入口 md）；PDF 只能借道系统打印对话框（sessionActions.ts:96-107）；docx 生成技能存在但 Welcome 入口隐藏（`welcome/constants.ts:89`）；产物文件落盘后无卡片/无「打开/发送」动作（MessageBubble.tsx:166-190 仅外部打开）。
- **判定**：⚠️ 零件都在，装配线不在。

---

## 6. User Stories 支持度矩阵（办公向）

> 「作为<角色>，我想<动作>，以便<价值>」——标注 Shannon 当前支持度与断点。

| # | User Story | Shannon | 断点/备注 |
|---|---|---|---|
| U1 | 作为知识工作者，我想拖入 office 文档直接提问，以便不用先转格式 | ❌ | 附件黑洞（J1） |
| U2 | 作为分析师，我想上传表格获得带公式的成品 xlsx，以便交付可复核结果 | ⚠️ | /xlsx-table 可产表但无解析输入、无宿主依赖保障 |
| U3 | 作为经理，我想一句话生成汇报 PPT 并逐页微调，以便快速交付 | ⚠️ | 只有大纲段（J2） |
| U4 | 作为与会者，我想把会议录音变成纪要+行动项，以便专注开会 | ❌ | 全链路缺失（J3） |
| U5 | 作为职场人，我想让 AI 读我的邮件并起草回复，以便减少收件箱时间 | ❌ | IMAP stub+徽章误导（J4） |
| U6 | 作为团队 leader，我想每周五自动生成周报并发到群里，以便无人值守 | ⚠️ | 自动化引擎 A 级，缺办公模板+成品交付+IM 路由（J5） |
| U7 | 作为研究者，我想基于我的笔记库提问且答案带出处，以便可信检索 | ⚠️ | Obsidian 查询断路+无注入通道（J6）；grep 检索本身可用 |
| U8 | 作为职场人，我想把 AI 的产出存成 Word/PDF 分享，以便对接非 AI 用户 | ⚠️ | 导出仅 md/json+打印（J7） |
| U9 | 作为自由职业者，我想授权一个工作文件夹让 AI 直接整理产出文件，以便批量处理本地资产 | ✅ | 这是 Shannon 现状里少数领先项：工作目录+文件工具+权限模式+沙箱齐备 |
| U10 | 作为重度用户，我想定义自己的办公自动化例程（触发→Agent→交付），以便复用 | ✅ | routines+webhook+NL-cron 完整；缺办公模板与交付路由 |
| U11 | 作为写作者，我想选中一段文字让 AI 改写并原地落盘，以便不离开文档 | ❌ | 无 markdown/富文本编辑器（仅 CodeMirror 代码向） |
| U12 | 作为预算敏感用户，我想看到每次任务的 token/成本并设上限，以便控制开销 | ⚠️ | History 有成本展示；session 级预算上限无（2026-09-05 报告 G3 同结论） |

---

## 7. 典型场景与用例（竞品已验证 × Shannon 机会）

| 场景 | 竞品验证度 | Shannon 现状 | 机会评级 |
|---|---|---|---|
| 会议录音→纪要→行动项→跟进 | 全家桶标配 | ❌ 无 | 高（桌面端独占优势：系统音频/文件访问权限）。**注：本计划不含录音流水线（B4 已移除）；B4' 转写导入覆盖转写后处理环节，录音环节作为已知缺口公示** |
| 长文档/PDF 阅读总结 | 全家桶标配 | ⚠️ PDF 50KiB 头部、无 OCR/页范围 | 高（改造成本低） |
| 数据表分析→图表报告 | Copilot Excel/`=AI()`/Code Interpreter | ❌ 输入侧黑洞 | 高 |
| 一句话/文档→PPT | 全行业收敛范式 | ⚠️ 只有大纲 | 高 |
| 模板化文档批量生成（合同 memo、周报） | Cowork 官方案例 | ⚠️ 技能在但入口隐藏 | 中高 |
| 定时简报（新闻/指标→文件→投递） | Manus/ChatGPT Tasks | ⚠️ 引擎好、交付断 | **最高（差异化主打）** |
| 收件箱治理（自然语言批量分类回复） | Copilot Outlook/ChatGPT | ❌ | 中（依赖 IMAP 实装） |
| 名单批量富化/翻译/打标 | Genspark/飞书字段捷径 | ❌ | 中（新交互形态） |
| 本地文件夹整理/批量改名/归档 | Manus Desktop/Cowork | ✅ 已可用 | 已领先，应曝光为卖点 |
| 网页调研→成文 | Deep Research/Manus | ✅ browser_13 工具+WebFetch/WebSearch | 已领先，缺「调研→成品文档」流水线 |

---

## 8. Shannon Desktop 办公能力现状总表（证据版）

| 能力域 | 状态 | 关键证据（file:line） |
|---|---|---|
| PDF 解析 | ⚠️ | pdftotext 依赖 poppler；50KiB 截断（`crates/shannon-ui/src/repl/at_reference.rs:283`）；100MiB 上限（`attachments.rs:29`）；扫描件明示无 OCR（`desktop/src/commands.rs:812`）；REST 拒收 PDF（`api_server.rs:1394-1403`） |
| docx/xlsx/pptx/odt/rtf/csv 解析 | ❌ | 无相关 crate 依赖；Read 拒二进制（`read.rs:197-217`）；桌面附件路径要求 UTF-8（`commands_files.rs:155-157`） |
| MIME/魔数识别 | ⚠️ | 扩展名猜测+4 种图片魔数（`read.rs:13-17`）；桌面 MIME 表无 docx/xlsx/pptx/csv（`commands_files.rs:78-105`） |
| docx/xlsx 生成 | ⚠️ | bundled 技能 `skills/docx-report`、`skills/xlsx-table`（python3 stdlib，编译内嵌 `bundled.rs:57-71`）；宿主无 python3 则降级为 MD |
| pptx 生成 | ⚠️ | `skills/ppt-outline`：MD 大纲必交付，pptx 仅极简文本框 best-effort（`bundled.rs:73-78`） |
| PDF 生成 | ⚠️ | 仅 `window.print()`（`ResearchReportModal.tsx:34-51`）；会话导出仅 md/json（`export.rs:215-244`） |
| 附件体验 | ⚠️ | 拖拽✅（`ChatInput.tsx:207-231`）；office 附件黑洞（`commands.rs:703-720`）；附件不持久化（session log 只记 attachment_count） |
| 文件预览 | ❌ | 非图片附件仅「外部打开」（`MessageBubble.tsx:166-190`）；无 PDF/Office/CSV 渲染（2026-09-25 artifact 设计文档:65 自认） |
| Artifact 面板 | ✅/⚠️ | html/svg/mermaid/长文档自动检测（`detectArtifact.ts:136-168`）+DocumentRenderer 表格导出 CSV✅；无版本/发布/Office 支持 |
| 邮件 | ❌ | IMAP 目录桩（`data_source_catalog.rs:131-134`）+fetcher coming soon（`data_source_fetchers/mod.rs:88-105`）；SMTP 零命中；Obsidian/IMAP 查询直接报错却显示 Verified（`DataSources.tsx:426-435`） |
| 日历/联系人 | ❌ | iCal 同属 coming soon 组；vCard 零命中 |
| 会议/语音 | ❌/⚠️ | 无录音/转写/纪要链路；对话语音输入 2026-08 已落地（docs/competitive-research-2026-09.md） |
| 办公自动化引擎 | ✅ | cron+NL→cron 预览+webhook+日历/DAG+Triage+History（`scheduled_commands.rs:571-1418`、`nl-cron.ts:3-96`、`inbox_commands.rs:68-148`） |
| 自动化模板 | ⚠️ | routine 12 个+schedule 5 个全研发向（`ScheduleTemplates.tsx:31-82`）；productivity 类目文案在内容缺（en.json:2336） |
| IM 渠道 | ⚠️ | 8 平台 keyring+gateway supervisor✅（`commands_connections.rs:137-180`）；Slack/Discord/Telegram/RSS/iCal 查询 coming soon（`DataSources.tsx:527`） |
| 技能系统 | ✅/⚠️ | SkillRegistry+5 路径发现+联邦上游 anthropics/skills（`skill_catalog.rs:36-56`）；但目录 6 个办公技能装后为空壳 stub（`Skills.tsx:154-155`）；Welcome 文档技能区隐藏（`welcome/constants.ts:89`） |
| 沙箱执行 | ✅ | Bubblewrap/Seatbelt/Docker/Landlock（`sandbox.rs:222,400,529`、`landlock_backend.rs`）；Bash 可跑宿主 python——但引擎不保障 curated 环境 |
| 浏览器/抓取 | ✅ | browser_* ×13（复用系统 Chrome/CDP）+WebFetch/WebSearch（`lib.rs:545-558,437-438`） |
| 文件代理（读写本地） | ✅ | Read/Write/Edit/Glob/Grep+工作目录+8 档权限模式+`get_file_diff`（无 diff UI） |
| 多 Agent/Team | ⚠️ | agent_spawn 折叠块✅（`MessageBubble.tsx:861-930`）；team 只读、无 create_team UI |
| 通知 | ✅ | 原生通知+webhook 模板含 Teams/飞书/企业微信/钉钉（CHANGELOG:1042,1067） |

---

## 9. 问题清单（发现的问题）

### P0 —— 信任与承诺层面

1. **Office 附件黑洞**（J1）：可拖入、可选入（选择器过滤器含 `*`，`ChatInput.tsx:313`），但模型收不到内容、UI 无不支持提示、MIME 表连 docx 都没有。用户每一份拖入的合同/报表都被静默丢弃。
2. **UI 状态撒谎**：IMAP/Obsidian 未实现查询却显示 "Verified" 徽章（`DataSources.tsx:426-435`，CONFIG_ONLY_KINDS 未含二者）；`inbox-triage-hourly` routine 装上即空转。
3. **宣传与实现断裂**：`desktop/README.md:6-7` "drafts email, summarizes docs" —— email 零实现；"summarizes docs" 仅覆盖 md/txt/PDF 头部。对外传播一旦被验证即成口碑反向素材。
4. **技能目录空壳销售**：skill_catalog 里 pdf/plotly-charts/data-analysis/jupyter-session/documents-open/documents-convert 六个办公技能安装后得到只有 name+description 的 stub SKILL.md（`Skills.tsx:154-155`），用户装了不能用。

### P1 —— 能力断层

5. **文档解析缺失**（§8 第一、二行）：无 OOXML/CSV 解析依赖；PDF 50KiB 静默截断、无页范围（`@doc.pdf#pages` 在 2026-09-25 审查中已建议未落地）、无 OCR。
6. **生成链路暗物质**（J2/J7）：技能有内容但——宿主 python3 无探测引导/无 sidecar 保障、执行无分步进度 UI、产物无卡片动作、Welcome 入口 `DOCUMENTS_SKILLS_AVAILABLE=false` 假门。
7. **会议场景空白**（J3）：桌面端握有系统音频+麦克风权限却无录音纪要链路，让出了 2026 年最高频的办公入口场景。
8. **数据源与聊天割裂**（J6）：查询结果无「注入会话」，Chat.tsx/AppContext 中数据源零命中——数据源成了孤岛功能。
9. **自动化最后一公里**（J5）：办公模板零、产出无法落成 Office 成品、结果无邮件/IM 路由（SCHEDULED-FIX-PLAN.md:629-634 规划未落地）。
10. **diff 审批 UI 缺失**：`get_file_diff` 命令在但无渲染——竞品已把「行级 diff 建议→批准」做成信任金标准（Notion 2026-08）。

### P2 —— 体验债

11. 附件不持久化（base64 用完即弃、compact 后图片变占位符），无法支撑「文件库」形态。
12. 会话导出无 HTML/docx；无批量导出。
13. 定时任务模板、routine 模板全研发向，productivity 类目有文案无内容（en.json:2336）。
14. 无 markdown 编辑器；DocumentRenderer 只读——「AI 产出后直接改」的迭代闭环断了。
15. i18n 缺办公概念键：sheet/slide/calendar/会议/表格类 key 不存在于桌面 i18n。
16. 无伴随窗口/全局快速呼出（有 3 个全局快捷键但无置顶小窗形态）；无截图问答（竞品 Appshots 级）。
17. ~~Usage 页无预算上限~~（**销账**：已被 improvement-plan-2026-09 KPI「会话成本可见性 Wave A 三项全有，含预算」覆盖，审查 R5 附带项）。

---

## 10. 改进方案（v2，已并入对抗性审查与复审修正，待审核）

> **版本**：v2 取代 2026-09-29 初版。修订依据：[docs/reviews/2026-09-29-office-plan-adversarial-review.md](../reviews/2026-09-29-office-plan-adversarial-review.md) §2-§8（6 红/8 黄/7 蓝修正）+ §11 复审意见（3 异议/5 修正）。B4（会议纪要录音流水线）经决策移除，以 B4'（转写导入）替代。
> **决议归属（Q5=b）**：本方案映射进 improvement-plan-2026-09 的 **P2-3 框架**——Wave 1 + Wave 1.5 立即排入（决议记录已追加 2026-09-29 修订注记）；**Wave 2 设 traction 门**（Wave 1 上线后 KPI 达标再裁决）；Wave 3 季度评估。
> **排序原则**：① 先修复信任（§9 P0 问题）；② 优先「后端已就绪、只差最后一公里」；③ 主打差异化（本地优先自动化），不打同质化（Office 套件内嵌）。

### 10.0 已裁决决策（2026-09-29，用户已确认）

| # | 决策 | 裁决 |
|---|---|---|
| Q1 | 生成路线 | **混合**：xlsx 引擎原生（rust_xlsxwriter，含公式）；docx/pptx 走技能+python3 探测引导；sidecar 打包为 stretch |
| Q2 | B2 交付物 | **极简 pptx 达标线先行**（可打开、文本不溢出）；HTML deck（reveal.js，artifact 面板 day one 可渲染）为 stretch |
| Q3 | diff-first 作用域 | **仅办公产物路径/非工作目录**（或独立 profile）；开发线默认行为不变 |
| Q4 | 文件库存储模型 | **引用+索引**（路径+元数据+收藏，原文件不动）；复制仅显式「入库存档」 |
| Q5 | 与既有计划合并方式 | **映射进 P2-3 框架**：Wave 1+1.5 立即排入，Wave 2 视 traction 再裁决 |
| Q6 | B4' 转写导入 | **纳入** Wave 2 |

### 横切工作项（所有 Wave 强制）

- **i18n**：en/zh-CN 同一变更内更新 + 8 套机翻过 i18n-check 门禁；办公线需新增 sheet/slide/calendar/会议类 key（现全缺，§9-P2#15）。
- **工程门禁**：新增 Tauri 命令同步 `desktop/acl/app-permissions.json` + capabilities；同步 `lib/mock/handlers.ts`（VITE_MOCK_MODE 演示模式）；vitest 覆盖率门禁 80/60/75/80。
- **安全护栏**：解析类项（A2'/B4'）加 zip 炸弹/条目数/解压比护栏；注入类项（A2'/B3/B4'）内容过 `desktop/src/extensions/security.rs` 注入扫描器并标注来源。
- **KPI（Wave 2 traction 门的依据）**：① 附件处理成功率（office 附件→成功注入比例）；② 办公产物生成成功率（技能执行→文件可用比例）；③ routine 办公交付打开率（Triage 条目→被打开/导出比例）。Wave 1 上线后采集 baseline。

### Wave 1 —— 止血（1-1.5 周，纯 UI/文案/小后端，立即排入）

| # | 项 | 方案 | DoD（验收） |
|---|---|---|---|
| A5 | 产物卡片（**第一周交付**） | 聊天中生成的文件以卡片渲染：图标+文件名+大小+「打开/在文件夹中显示/另存为」，复用附件 chip 与 `openWithDefaultApp` | 产物文件显示卡片且三动作可用 |
| A1a | 附件拒收明确化（拆自 A1，不含转换管线） | docx/xlsx/pptx/csv 拖入时 UI 即时提示「此类型暂不支持解析，内容未发送给模型」+转换指引；MIME 表补全（`commands_files.rs:78-105`） | 提示双语（i18n）；mock 更新；不再有静默黑洞 |
| A3' | 自有技能复活 | 只推 **stdlib 三件**（docx-report/xlsx-table/ppt-outline）；按 python3 探测结果条件显示+缺失引导；**官方四件延至 B1 之后**（R2）；内嵌前完成上游许可核验 | 探测→条件显示；安装→执行→产物可打开 |
| A4' | 诚实化 UI（与 improvement-plan P0-1 同主题/同 owner，人工核对） | IMAP/Obsidian 徽章改「已配置（查询开发中）」（`DataSources.tsx:426-435`）；README 办公宣传口径对齐（README.md/README.zh-CN.md/desktop/README.md）；`inbox-triage-hourly` 下架或标注依赖 | 徽章/文案/routine 三处落地 |
| B8a | 附件空态卡+系统打开（拆自 B8，按 2026-09-25 已确认设计执行） | PDF/Office/其余类型 → 空态卡 + 系统默认程序打开 | 非图片附件可一键系统打开，不再只有路径文本 |

### Wave 1.5 —— 解析与预览（1.5-2.5 周，A2 按 R1 修正后工期下修）

| # | 项 | 方案 | DoD（验收） |
|---|---|---|---|
| A2' | 文档解析 v1（=A1b） | 纯 Rust 解析：docx/pptx（roxml+zip 分节提取）、xlsx/ods（calamine）、csv（csv crate）。**注入策略（R1 修正）**：提取文本落 `~/.shannon/cache/extracted/<hash>.txt`（带章节标记），会话注入结构化摘要+显式截断提示（「已注入 X/共 Y 节」），模型经现有 Read/Grep 对 cache 文件分页深读——**不新建工具**；PDF 维持 pdftotext 优先+补 `-f/-l` 页范围与超出提示；Rust PDF crate 替代仅在 Windows 痛点实测后评估（Y8）。安全护栏见横切项 | 四类格式拖入内容可见；超限有提示且可分页深读；zip 炸弹样本过护栏；ACL/mock/覆盖率同步 |
| B8b | pdf.js 内联预览 | PDF 附件在 RightDock/artifact 内联渲染 | 渲染+页码导航 |

### Wave 2 —— 交付与接线（4-8 周，**前置：Wave 1 上线 traction 达标 + improvement-plan P0-3 收件箱已合**）

> 顺序为复审修正后的价值/成本排序（审查 §11.3-①）：B6' 最优先（旗舰故事闭环、依赖最薄），**B2 容量紧张时首个让位**（竞品同质化最重、差异化最轻）。

| 序 | # | 项 | 方案 | DoD（验收） |
|---|---|---|---|---|
| 1 | B1 | 执行环境（混合生成） | **xlsx 引擎原生**（rust_xlsxwriter，含公式，宿主零依赖）；docx/pptx 走技能+python3 探测引导；技能执行分步进度复用 ToolProgress（读模板→建结构→生成→打包）；sidecar uv 打包为 stretch | 无 python3 机器可产带公式 xlsx；有 python3 机器可产 docx/pptx；进度分步可见 |
| 2 | B6' | 自动化结果路由 v1 | routine 产物落 xlsx/docx（接 B1）；投递走 **Slack/飞书自定义机器人 webhook**（复用已决议的 5 渠道线）；**IMAP Drafts 注入**（用户在熟悉客户端核对后发送）；**不做 SMTP 直发**（R3） | routine 产物→Triage 带产物链接→一键存邮件草稿/投递机器人 |
| 3 | B3 v1 | 数据源接线（收窄） | IMAP：最近 N 天未读+正文 HTML 清洗+**过注入扫描器**+来源标注（不做 IDLE，轮询即可）；Obsidian：mtime 增量+关键词/近因摘录；查询结果加「注入当前会话」；**明示非 RAG**（深检索走 agentic grep，Y3）；引用锚定 UI 归 C8 | IMAP/Obsidian 查询→注入会话且带来源标注；恶意邮件样本过扫描 |
| 4 | B2 v1 | PPT 生成（降维，**首个让位项**） | 大纲确认 UI（可编辑列表）→**单一极简 OOXML 模板**→质量门槛（可打开、文本不溢出）；迭代采用「整份重生成但保留已确认大纲」；模板系统/逐页局部重生成挪 Wave 3 评估；HTML deck 为 stretch（Q2） | 大纲确认→pptx 可打开且不溢出 |
| 5 | B5 | 办公模板包（P0-3 收件箱之上的增量） | ≥3 个 productivity 例程：每周周报、每日新闻/指标简报、收件箱摘要；「一句话装好一个例程」（对话式生成 routine 草案→确认） | 创建→运行→产物进 Triage 带产物链接 |
| 6 | B9' | 文件库 v1（引用+索引） | 路径+元数据+收藏标记+原地打开，**原文件不复制**；「入库存档」为显式动作；文件失效（被移动/删除）有状态提示（Y4 修正） | 附件/产物可检索、收藏、原地打开；无重复存储；失效文件有提示 |
| 7 | B7' | Diff viewer（作用域限定） | RightDock 加 diff 标签页渲染 `get_file_diff`（渲染本身无争议）；**diff-first 仅作用于办公产物路径/非工作目录或独立 profile**，开发线默认行为不变（R4） | 写办公产物路径→diff 预览→批准应用；开发线工作流无新增摩擦 |
| 8 | B4' | 转写导入（替代已移除的 B4，零录音基建） | 拖入 .srt/.vtt/.txt 转写→「会议纪要」技能（摘要+行动项+决策提取）→行动项一键转 scheduled task；过注入扫描器 | 转写文件→纪要产物→行动项成任务 |

### Wave 3 —— 差异化（季度级，逐项带前置条件）

| # | 改进 | 方案 | 前置条件 |
|---|---|---|---|
| C1 | 「本地办公 Agent」叙事打包 | 「定时/事件触发→Agent 干活→交付 Office 成品→投递 IM/邮件」一页式产品故事；对外宣传与 website 同步 | 无 |
| C2 | 表格批量 Agent 视图 | CSV/表格视图承载批量执行（行=任务、列=Agent 操作、单元格=结果+状态），复用 agent_spawn | **Tasks IA 裁决完成**（2026-06 P0 遗留）；短期可做 Chat 内 artifact 表格交互替代新页面 |
| C3 | 伴随窗口 | 置顶小窗（全局快捷键呼出）+截图问答（复用 AnalyzeImage）+「把当前应用窗口发给 AI」 | **Tauri 多窗口 spike**（AppContext 中心化状态改造）；搭 improvement-plan P1-5「多窗口」里程碑的车 |
| C4 | 来源集合（Notebook 形态） | Project 下挂「来源集合」（本地文件夹/Obsidian/数据源/URL），常驻检索上下文+回答带出处 | B3 v1 注入通道 |
| C5 | 风格反提 | 从用户已有 docx/pptx 提取样式（字体/配色/版式）生成「品牌 kit」 | **与 B2 模板系统决策合并评估**（可能比自建模板便宜且更个性化） |
| C6 | 过程回放 | 后台任务执行轨迹（工具调用+文件变更时间线）可回放+导出分享 HTML | 无 |
| C7 | 办公技能市场类目 | Extensions Hub 增 Productivity 类目（官方 docx/pptx/xlsx/pdf 四件在 B1 后进入+社区件），安装→启用→进度→产物全链路可观测 | B1 交付 |
| C8 | 引用溯源 UI | 聊天回答中来自文件/数据源/网页的内容统一「来源 pill」→点击下钻原文位置（**吸收原 B3 引用锚定**，蓝-4） | B3 v1 |
| C9 | 逐页局部重生成（自 B2 移入） | PPT 模板系统成熟后的局部重生成能力 | B2 v1 traction + 模板系统立项 |

### 明确不建议做的

- **Office 套件内嵌加载项**（Copilot/Claude for Excel 战场）：需要 Office 生态与品牌资产，非开源桌面工具优势区。
- **企业全量上下文**（豆包读飞书全量）：需要企业 IM 权限体系，Shannon 的对位故事是「本地权限+隐私」，不做企业数据平面。
- **credits 计费体系**：BYOK 本地优先模式下无必要，把 Usage 做成成本可观测即可。
- **本轮做「逐页局部重生成」**（审查 §7）：它是模板系统成熟后的能力，B2 v1 用「整份重生成保留已确认大纲」替代，可砍掉一半以上 UI+状态机工作量。

### 排期口径（v2）

Wave 1（1-1.5 周）→ Wave 1.5（1.5-2.5 周）→ Wave 2（4-8 周，**traction 门**）→ Wave 3（季度）。均为 1-2 人假设，spike 后复核（R6）；Wave 2 启动前需按 §10.0-Q5 完成 traction 裁决。

---

## 11. 结论

Shannon Desktop 在办公场景的真实牌面是：**一套接近一线水准的自动化引擎（cron/webhook/NL 定时/Triage/History）+ 一个与 Anthropic Skills 标准同构且已兼容其上游生态的技能系统 + 完整的本地文件代理与沙箱权限体系**。坏消息是这套资产被「内容链路断裂」锁住了——用户最直觉的办公动作（拖入文档、要一份 PPT、整理会议、处理邮件）在当前版本要么静默失败要么入口隐藏。

Wave 1 的五项止血（附件拒收明确化、产物卡片、自有技能复活、诚实化 UI、附件空态卡）全部是「小工作量、高确定性」项，且互为前提；Wave 2 以 **B6'（自动化结果路由）为最优先**——它是把自动化引擎的 Grade A 价值和办公交付物接通的临门一脚，而 B2（PPT 生成）已按复审降为「首个让位项」。整套方案按 Q5=b 映射进 improvement-plan-2026-09 的 P2-3 框架执行：Wave 1+1.5 立即排入，Wave 2 视 traction 裁决。行业在 2026 年的收敛路径已经把「该做什么」验证完毕——对 Shannon 而言这反而是机会：**交互范式有标准答案可抄，而「本地优先+开源+技能生态兼容」的生态位上尚无成熟对标者**。

---

## 附录 A：信息源

**Microsoft**：[M365 Copilot Release Notes](https://learn.microsoft.com/en-us/copilot/microsoft-365/release-notes) · [Copilot Pages](https://support.microsoft.com/zh-cn/microsoft-365-copilot/how-microsoft-365-copilot-pages-works) · [Copilot Chat 发布](https://www.microsoft.com) · [Python in Excel](https://support.microsoft.com/en-us/office/python-in-excel) · [Copilot Credits 计费](https://learn.microsoft.com/en-us/microsoft-365/copilot/usage-based-billing-overview-copilot-credits)
**Google**：[Workspace AI 总览](https://workspace.google.com/solutions/ai/) · [Gemini updates 2026-03](https://blog.google/products-and-platforms/products/workspace/gemini-workspace-updates-march-2026) · [Sheets =AI()](https://blog.google) · [Meet 笔记](https://support.google.com/a/users/answer/9283046) · [Docs 侧面板](https://support.google.com/docs/answer/14206696) · [Gemini Enterprise](https://cloud.google.com/gemini-enterprise)
**OpenAI**：[Release Notes](https://help.openai.com/en/articles/6825453-chatgpt-release-notes) · [ChatGPT Work](https://openai.com/index/chatgpt-for-your-most-ambitious-work) · [Record](https://help.openai.com/en/articles/11487532-chatgpt-record) · [Scheduled tasks](https://help.openai.com/en/articles/10291617-scheduled-tasks-in-chatgpt)
**Anthropic**：[Agent Skills](https://claude.com/blog/skills) · [anthropics/skills](https://github.com/anthropics/skills) · [Cowork](https://claude.com/product/cowork) · [MCPB](https://github.com/modelcontextprotocol/mcpb)
**中国生态**：[金山办公双产品线（新浪财经）](https://finance.sina.com.cn) · [WPS 社区](https://bbs.wps.cn) · [Kimi 官网](https://www.kimi.com) · [Kimi Work 实战](https://bbs.csdn.net) · [豆包工作（新浪财经）](https://finance.sina.com.cn/stock/jdts/2026-08-28/detail-inipwnxp9009248.d.html) · [飞书 8.0 协同（搜狐）](https://m.sohu.com/a/1076440491_313745) · [钉钉 8.0（新浪财经）](https://finance.sina.com.cn) · [AiPPT](https://www.aippt.cn) · [AI PPT 横评（SegmentFault）](https://segmentfault.com) · [模板实测（51CTO）](https://www.51cto.com)
**Agent 交付型**：[Manus 文档](https://manus.im/docs/llms.txt)（Slides/Wide Research/Automations/Desktop） · [Genspark](https://www.genspark.ai) · [Gamma 3.0](https://gamma.app/insights/introducing-gamma-3-0) · [Gamma 数据入 deck](https://help.gamma.app/en/articles/15715171-how-can-i-get-my-data-into-gamma-decks-using-an-llm) · [Notion Releases](https://www.notion.com/releases/2026-09-15)
**内部基线**：`desktop/COMPETITIVE-ANALYSIS.md` · `docs/competitive-research-2026-09.md` · `docs/plans/2026-09-25-memory-doc-rag-review.md` · `docs/plans/2026-09-25-desktop-chat-ui-open-and-artifact-design.md` · `docs/plans/2026-09-06-p3-future-research.md` · `desktop/SCHEDULED-FIX-PLAN.md` · `desktop/PHASE-E-ROADMAP.md`

**可信度备注**：竞品定价为公开页面口径（2026-09 美国区/中国区），区域与渠道或有差异；Manus/Genspark 定价页为 JS 渲染，数字来自多源交叉（$19–$39 入门档有出入），引用时以官网为准；ChatGPT help 部分页面拒绝抓取，细节来自官方摘要与第三方转述交叉验证。
