# 本地办公 Agent —— Shannon 的一页式产品叙事

> **受众**：官网文案作者、README 维护者、发布说明（release notes）作者。
> **用法**：写官网 feature 段落、README 办公小节或版本发布公告时，直接引用本页的定位语、价值主张与场景，不必重新发明口径。
> **依据**：[办公场景竞品调研](../research/2026-09-29-office-scenario-competitive-research.md) §10 改进方案 v2（Wave 1 → 1.5 → 2 → 3）与[对抗性审查报告](../reviews/2026-09-29-office-plan-adversarial-review.md)。
> 主体中文；关键句附英文，供英文物料直接取用。

## 一、定位

**Shannon 是一个本地优先的无人值守办公 Agent**：它装在你自己的电脑上，连着你选的模型，定时或被事件触发后自己干活，最后交付的不是一段聊天记录，而是**能直接打开的 Office 成品文件**。

> EN: *Shannon is a local-first, unattended office agent — it lives on your machine, works on a schedule or a trigger, and hands you real Office files, not chat logs.*

它同时是一个开源的 AI 编码工作台（那是它的出身），但办公叙事只讲一件事：**给知识工作者一个不需要盯着看的、交付真文件的数字同事**。编码能力是引擎，办公交付是用户看到的东西。

## 二、三段价值主张

### 1. 本地权限 + 隐私（数据不搬家）

- 模型密钥只存在本机（`~/.shannon/credentials/`，`0600`），直连你选的提供商，没有中间服务器；默认零遥测。
- 文件在本地读取与生成：你公司的报表、你的会议转写、你的收件箱摘要，都不需要上传到第三方云沙箱。
- Landlock/Seatbelt 沙箱 + 权限分级 + 出站 secret 脱敏——无人值守的前提是「它能在你看不见的时候被信任」。

> EN: *Your files never leave your machine; your keys never leave your machine. Unattended work needs trust you can audit.*

### 2. 定时与事件触发自动化（没人盯着也在干活）

- **Routines**：cron 定时、一次性、事件触发三种任务形态；`/goal` 给目标而非 prompt，预算封顶、自动续跑。
- **触发面**：API endpoint（HMAC 签名）、GitHub 事件、IM 渠道（Telegram / Discord / Slack / 飞书 / 钉钉）@ 一句即派活，手机扫码配对后随时审批。
- 结果进桌面 Triage 收件箱：你回来的时候，活已经干完了。

> EN: *Schedules, webhooks, and IM mentions wake the agent; you come back to finished work.*

### 3. 交付真 Office 文件（不是描述文件，是文件本身）

- **内置文档技能**（零依赖纯标准库）：`/docx-report`（Word 研报）、`/xlsx-table`（Excel 表格）、`/ppt-outline`（PPT 大纲 + 极简 .pptx）、`/meeting-minutes`（转写→纪要）、`/style-extract`（从已有 .pptx/.docx 反提品牌风格笔记）。
- **引擎原生 `write_xlsx`**：宿主没有 python3 也能产出带公式的 .xlsx。
- **产物即卡片**：聊天里生成文件即渲染产物卡片（打开 / 在文件夹中显示 / 另存为），Triage 收件箱里留档。
- **投递诚实**：Shannon **不直接发邮件**（不做 SMTP 直发）。自动化产物的「最后一公里」走 IM 自定义机器人 webhook，或注入 IMAP 草稿箱——由你在熟悉的邮件客户端里核对后发送。

> EN: *Deliverables are files you can open in Microsoft Office — and Shannon never sends email on its own; it routes via IM webhooks or drafts it into your IMAP Drafts for you to review.*

## 三、目标用户

**自己有电脑、天天用 Office、但不必是开发者的人**——以及愿意为他们配置好 Shannon 的家人/同事/IT 邻居：

| 人群 | 典型诉求 | 一句话价值 |
|---|---|---|
| 团队负责人 / 个人贡献者 | 周报、例会纪要、汇总 | 「周五下班前，周报已经在收件箱里」 |
| 运营 / 市场 / 编辑 | 简报、素材整理、批量表格 | 「每天早上 9 点，新闻简报自动生成」 |
| 财务 / 行政 / HR | 表格批处理、模板文档 | 「100 行数据逐行处理，产出一个 .xlsx」 |
| 远程工作者 | 收件箱摘要、日程准备 | 「出门前知道今天邮件里有什么要紧事」 |

## 四、五个一日场景（发布物料可直接改写）

1. **周报自动化**：周五 17:00 的 routine 拉取本周 git 提交、任务看板与笔记 → 生成 `weekly-report.docx` → 落 Triage 收件箱，附 IM 机器人通知。你核对一遍再发。
2. **新闻简报**：工作日 09:00 抓取你指定的信息源 → 生成 `daily-brief.docx`（或 .md），带来源列表。IMAP 数据源接入后，可并上未读邮件摘要。
3. **转写纪要**：把会议录制工具导出的 `.srt`/`.vtt`/`.txt` 拖进桌面 → `/meeting-minutes` 产出「摘要 / 决议 / 行动项（负责人+期限）/ 待决问题」四段式纪要 → 行动项一键转定时任务。
4. **表格批量**：拖入 CSV 或直接指路一个 .xlsx → 按行跑任务（逐行翻译、打标、计算）→ `/xlsx-table` 或引擎原生 `write_xlsx` 产出带公式的成品表格。
5. **收件箱摘要**：早晨的 routine 汇总 IM 渠道隔夜消息与（配置后的）IMAP 未读 → 一页摘要 + 待办清单，重要项直接派成新任务。

> 注意（诚实清单，宣传时不得越线）：**邮件不可直发**；IMAP/Obsidian 属于已配置数据源的查询接线；转写纪要**不处理录音**，需要现成文本转写。

## 五、生态位对比：三类竞品，一个空位

| | Office 套件内嵌 AI<br>(M365 Copilot / Gemini for Workspace / WPS 灵犀) | 聊天助手桌面端<br>(ChatGPT Desktop / Claude Desktop / Kimi Work) | Agent 交付型<br>(Manus / Genspark / Gamma) | **Shannon** |
|---|---|---|---|---|
| 运行位置 | Office 云服务 | 本地 App，推理在云 | 云虚拟机/沙箱 | **你的电脑，本地优先** |
| 无人值守 | 弱（跟人走） | 无会话级定时（依赖平台） | 有（云托管） | **Routines + 事件触发，本地调度** |
| 交付物 | 套件内文档 | 聊天卡片/Artifact | 云端链接/文件 | **真 .docx/.xlsx/.pptx 落本地磁盘** |
| 隐私边界 | 数据进租户云 | 对话出站到模型云 | 任务数据进云沙箱 | **文件与密钥不出机，可审计**（事件溯源回放） |
| 模型 | 绑定微软/谷歌/WPS | 绑定自家模型 | 绑定平台额度 | **BYOK，任意提供商** |
| 成本 | 每席位订阅 | 订阅额度 | credits 计费 | **按量付费 + 预算上限** |
| 空位判断 | 我们不打（不做套件内嵌加载项） | 它们有用户没交付闭环 | 它们有交付没本地信任 | **「本地信任 × 无人值守 × 真文件」三者同时成立** |

一句话版本：*套件 AI 住在别人的云上，聊天助手停在对话框里，交付型 Agent 跑在别人的沙箱里——Shannon 把「干活的 agent」放回你自己的电脑，并交给你能打开的文件。*

## 六、与路线图的关系

本叙事是办公线改进方案 **§10 v2** 中 C1（叙事打包）的落地物。进度口径（截至 2026-09）：

- **已交付**：Wave 1（产物卡片、附件诚实提示、stdlib 三技能复活）、Wave 1.5（docx/pptx/xlsx/ods/csv 解析注入、PDF 预览）、Wave 2（引擎原生 write_xlsx、PPT 极简生成、IMAP/Obsidian 数据源接线、办公模板例程、Triage 产物路由、diff-first 写入、转写纪要导入）。
- **Wave 3 进行中**：C5 风格反提（`/style-extract`，从已有文件提取品牌风格笔记）与本页 C1 叙事打包。
- **明确不做**（对齐 §10「不建议做」清单）：Office 套件内嵌加载项、企业全量上下文数据平面、credits 计费体系、SMTP 直发邮件。

版本公告引用模板：*「v0.13 让 Shannon 从编码 agent 长出办公叙事：定时任务交付真 .docx/.xlsx 文件，`/style-extract` 从你的旧模板反提品牌风格——全部本地完成。」*
