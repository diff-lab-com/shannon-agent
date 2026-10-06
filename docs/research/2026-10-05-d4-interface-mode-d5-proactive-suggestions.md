# 调研:D4 界面模式实验 × D5 主动任务推荐(ZCode 实现拆解 + 竞品对照 + Shannon 建议)

- 日期:2026-10-05
- 背景:`docs/plans/2026-10-05-zcode-settings-comparison-and-improvement-plan.md` §7 将 D4(界面模式)/D5(主动任务推荐)列为"待调研的排除项"。本文为前置调研,供决策。
- 证据来源:本机 ZCode 安装包 `/opt/ZCode/resources/app.asar`(2026-09-29 构建)的 i18n / Zod schema / 渲染逻辑字符串提取、`~/.zcode/v2/*.json` 运行时状态、竞品公开文档与 issue、Shannon dev 分支代码盘点。两 feature 证据置信度均标注。

---

## 结论速览

| 项 | ZCode 实际实现 | 建议 |
|---|---|---|
| D4 界面模式 | **纯渲染层**的 coding/office 双模式:折叠工具卡、隐藏终端按钮/git 摘要、换问候语与建议集;**不改 system prompt、不改工具** | **不做独立"界面模式"设置项**。其价值主体(消息流降噪)由已排期的 C1-C6 覆盖;把"详略语义"作为 C6 之上的 simple 模式渲染分支顺带落地(S~M)。完整双模式设 3 个重开触发条件 |
| D5 主动任务推荐 | **静态模板卡片池**(约 35 条内置 prompt,office 22/coding 12),"换一批"= 本地洗牌;仅办公模式可见;**零模型调用** | **现在做方案①**(完成后静态 chips + 欢迎卡"换一批"+ 工作区感知过滤 + 设置开关,S,零 token);方案②(模型驱动)条件后置;方案③(空闲挖掘推荐)**不单独立项** |

两功能的"名字"都比"实现"大:D4 不是人格/模式引擎,D5 不是智能推荐引擎。按 ZCode 的实际水准对齐即可,不必按名字想象的水准过度建设。

---

## 1. D4:ZCode「界面模式」是什么(置信度:高)

### 1.1 定义与证据(引自 app.asar 内嵌 i18n / Zod schema)

| Key | 文案 |
|---|---|
| `settings.interfaceMode` | 界面模式 / Interface mode |
| `settings.interfaceMode.coding` | 编程模式(默认) |
| `settings.interfaceMode.office` | 办公模式 |
| 描述 | 办公模式侧重操作摘要与结果;编程模式显示命令、输出和代码变更详情 |

- 取值 `enum(["coding","office"])`,与 `occupation`/`memoryEnabled`/`proactiveSuggestionsEnabled` 同组(账号设置同步 payload);本地持久化于 localStorage `zcode-interface-mode`,与 theme/locale 同属渲染层 store。
- **反证**:主设置文件 `~/.zcode/v2/setting.json` 无此键;CLI bundle 中仅出现 1 次 → **桌面渲染器专属概念,CLI 侧无此概念**。
- 命名迭代痕迹:归一化函数把旧值 `concise`(简洁)/`general`(通用)都映射为 `office` —— ZCode 自己从"行为"命名迭代到了"受众"命名。

### 1.2 它实际切换什么(渲染层 6 处)

1. 工具调用卡:办公模式不可展开(`canToggle:!isOfficeMode`)、隐藏 diff 计数、用简洁状态文案("已运行命令")。
2. 会话头技术上下文:git 摘要/脏文件数隐藏。
3. 标题栏终端按钮:办公模式不渲染。
4. 空会话问候语独立文案(办公:「今天有什么工作,交给我吧」)。
5. 建议提示词/命令面板按模式切换两套列表。
6. 功能门控:**主动任务推荐仅办公模式可用**;引导中办公模式默认勾选记忆与推荐。

入口:设置页下拉、应用内快捷菜单(与主题/缩放同组)、引导流程(先选 12 种职业,再选模式)。

### 1.3 架构结论(多处交叉验证)

`ui_mode`(work/code)仅出现在遥测上下文;**没有任何证据表明它修改 system prompt 或模型请求**。"办公=摘要"完全是前端渲染层折叠:同一个 agent、同样的工具、同样的提示词,只是 UI 不给看过程。

### 1.4 竞品对照

| 工具 | 功能 | 切换什么 | 门控 agent 行为? |
|---|---|---|---|
| Claude Code | Output Styles(`/output-style`) | system prompt 层(角色/语气/格式) | 是 |
| Claude Code | Plan mode(Shift+Tab) | 工具门控 + 计划先行提示 | 是 |
| Cursor | Agent/Ask/Edit + Custom Modes | system prompt + 工具/模型访问级别 | 是 |
| Zed | Agent Profiles(Write/Ask/Minimal) | per-profile 工具可用性 + prompts | 是 |
| Cline | Plan/Act | 每模式独立 system prompt,可绑模型 | 是 |
| VS Code | Profiles | 设置/快捷键/扩展打包组合 | 否(非 agent) |
| **ZCode** | 界面模式 | **仅渲染层表面可见性** | **否** |

规律:编程工具的"模式"绝大多数是**意图型**(plan/act)或**提示词人格型**;ZCode 的纯表现层模式是异类——它服务的是 12 职业里的 11 种非开发者,是消费级产品的**用户分群开关**。

### 1.5 Shannon 现状(积木已齐)

- `useSidebarMode`(simple/dev,`shannon-sidebar-mode`,`desktop/ui/src/components/Sidebar.tsx:40-53`)已控制导航可见性;density 已实现"auto 跟随侧栏模式"(`desktop/ui/src/lib/density.ts`:advanced→compact、simple→comfortable)——**"一轴带动多面"的雏形已在**。
- 消息流无详略控制(每个工具调用独立卡、无折叠策略,`MessageBubble.tsx:461-534`)——已排期的 C6(工具分组)正是解法。
- 提示词装配是成熟管线(`crates/shannon-core/src/query_engine/system_prompt.rs`,稳定区+动态区+缓存断点);persona 打包已有(`desktop/src/persona_pack_commands.rs`)。

### 1.6 D4 建议

**不做独立"界面模式"设置项。**理由:

1. ZCode 界面模式的本质是"消息流降噪 + 非开发者友好"的渲染层开关,其价值主体恰好是 Shannon 已排期 C1-C6(消息流行为批次)要解决的问题;单独再加一条"模式"轴会与 simple/dev、density 两轴语义打架(出现两套几乎同义的开关)。
2. 受众错配:ZCode 用 12 职业引导服务泛办公人群;Shannon 的现实用户是从 Claude Code/ZCode 迁移的开发者。
3. **具体改法**:C6 工具分组落地后,在 simple 侧栏模式下默认启用"折叠分组卡 + 隐藏原始输出 + 简洁状态标签"的渲染分支(增量约 1-2 天,选项 1 形态),保留"查看详情"逃生门;不需要灰度 flag(dev 模式天然对照)。
4. **重开触发条件**(满足其一再升级为完整双模式):① Shannon 立项主动推荐/记忆优先等非开发者功能,需要人群轴挂门控;② 反馈渠道出现 ≥3 例非开发者用户抱怨消息流过于技术化;③ 数据显示 simple 模式占比 >30%。
5. 提示词人格(回复风格三档)**不建**:persona.md + 13 主题已覆盖个性化心智,无用户呼声前属投机。

---

## 2. D5:ZCode「主动任务推荐」是什么(置信度:高)

### 2.1 定义与证据(内部名 `officeSuggestions`)

| 证据 | 原文 |
|---|---|
| 设置项 | `"chat.officeSuggestions.setting": '主动任务推荐'` |
| 适用范围 | `"仅在办公模式下支持。"`(EN: "Available only in Office mode.") |
| 交互 | `"在新对话中显示任务建议,点击后填入输入框。"`;`"换一批"` / "Show more" |
| 设置键 | `proactiveSuggestionsEnabled: boolean`,默认 `?? false` |
| 本机状态 | `setting.json`: `proactiveSuggestionsEnabled: false`;`onboarding-record.json`: `interfaceMode: "coding"`(即当前不可用,与门控互证) |

### 2.2 内容的真实形态:内置模板池,不是模型生成

- 模板结构 `{id, mode, iconUrl, label{cn,en}, prompt{cn,en}, plugin{stableId,…}}`,bundle 内约 **35 条**(office 22 / coding 12)。
- office 样例:「帮我看看电脑空间主要被什么占满了」「每周自动汇总进展并准备下周的重点工作」。
- coding 样例:「帮我看懂并运行当前仓库」「帮我运行项目现有检查并定位失败」。
- 部分卡片是**定时任务 setup 诱导**:如"每周自动汇总"卡的 prompt 是"帮我设置一个每周五下午 5 点运行的定时任务…"(顺带推广插件市场插件)。
- **未发现任何 suggestion API**;"换一批"由本地 `shuffle/sampleSize` 完成(置信度中高)。

### 2.3 竞品三流派

| 流派 | 代表 | 触发 | 数据源 | 成本 |
|---|---|---|---|---|
| ① 静态 starter 卡片 | **ZCode**、Devin Playbooks、Copilot starters | 新对话/会话开始 | 内置模板 | 零 |
| ② 每轮后模型生成 follow-up | Claude Code CLI(ghost text, Tab 接受)、Copilot followups | 每轮结束 | 小模型调用 | 真实 token,计用户额度 |
| ③ 不做 | Cursor、Cline、Aider | — | 靠 rules 让主模型自带"next steps" | 零 |

**没有任何一家在编码工具里做"分析历史行为→推送通知式推荐"**——打扰重灾区,竞品集体回避。

### 2.4 Shannon 现状:地基比 ZCode 的实现更厚

| 已有资产 | 位置 | 关联 |
|---|---|---|
| 静态 starter 卡 | `desktop/ui/src/components/welcomeExamples.ts`(4 张,点击填入) | 就是 ZCode 该功能的呈现面,缺"换一批"/工作区感知/开关 |
| 例行模板库 | `desktop/routines/*.toml` 15 个 + `commands_routine_templates.rs` 一键实例化 | 与 ZCode"每周自动汇总"卡语义同构(`weekly-report.toml` 几乎逐句对应),缺曝光层 |
| inbox 管道 | `crates/shannon-core/src/inbox_store.rs`(10 种 source,含去重)+ `desktop/src/inbox_session_events.rs` | 加 `suggestion` source 即获全套 UI |
| 行为模式挖掘 | `desktop/src/skill_pattern_detection.rs` + `commands_skill_candidates.rs` | 已是"基于历史的主动推荐"(推荐物是技能) |
| 空闲/错峰调度 | `OffpeakConfig` + `ExecutionWindow`(`desktop/src/scheduled_commands.rs`) | 低成本推荐生成的挂载点 |
| 隐私边界 | `commands_dream.rs` 的 `redact` + SessionQuery 只读适配 | 模型驱动建议读会话内容的强制管道 |
| 成本台账 | `commands_usage.rs` | 建议生成 token 可单独记账 |

缺失:① 完成后无"建议下一步"chips;② 建议类功能无统一开关;③ welcomeExamples 无工作区感知与刷新;④ routine 模板无主动曝光。

### 2.5 D5 建议

**方案①(现在做,S,1-2 天,纯前端 + 少量 Rust 事件负载字段)**
- 任务完成后按运行元数据规则生成 2-3 个静态后续动作 chips:失败→"重试/查看失败日志";改了 N 个文件→"提交这些改动";测试失败→"修复失败的测试";例程完成→"查看运行历史"。零模型调用。
- 欢迎卡加"换一批"(本地洗牌)+ 工作区轻量探测过滤(`Cargo.toml`/`package.json` 决定"跑测试/查依赖"卡是否展示)。
- 同一卡片位顺带曝光 routine 模板卡(解决模板藏在 /routines 页无人发现的问题)。
- 开关:`suggestions.enabled`(默认开;沿用 `OffpeakConfig` 冻结键 + serde default + 迁移测试套路)。

**方案②(条件后置,M,3-5 天)**:完成后一次 flash 级小模型调用(优先 `offpeak.model_override` 便宜模型),输入为会话元数据(末条用户消息 + 工具清单 + 收尾摘要,截断 + 强制 `redact`),输出严格 JSON 2-3 条;3 秒超时静默失败;chips 点击填入不自动发送;token 计入 usage 单独一行。开关 `suggestions.model_driven` **默认关**。触发条件:①上线后观察到 chips 点击率;确认便宜模型可用。外部依据充分(Claude Code 已原生做、Cursor 用户在论坛催),但不值得默认烧用户额度。不推荐 piggyback 变体(主模型回复尾部自带建议块):污染回复格式、依赖模型依从性,脆弱。

**方案③(不单独立项)**:offpeak + 会话历史挖掘 → inbox `suggestion` 条目("你每周五手写周报,试试 weekly-report 模板?")。仅当 skill detection cron 自然扩展时搭车实现(S-M 边际);在此之前**不做**任何通知推送式主动行为。

设置页归属:三案共用一个 `suggestions` 配置组,开关描述注明"模型驱动建议默认关闭、产生少量 token 费用"。

---

## 3. 待决策点(供拍板)

1. D4:接受"不做独立界面模式,改写为 C6 附属渲染策略 + 3 个重开触发条件"?(本报告建议:是)
2. D5:是否立项方案①(静态完成后 chips + 欢迎卡刷新/过滤 + `suggestions.enabled` 开关,S)?方案②/③按上述条件后置?(本报告建议:立项①)
