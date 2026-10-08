# Shannon Desktop 全页面 Journey 走查 + 逐页竞品对比 + 综合改进方案（R2 · 修复后复审）

**日期**: 2026-10-01
**基线**: `dev` @ 2f4d8789 —— R1 报告（[2026-10-01-full-journey-competitive-review.md](2026-10-01-full-journey-competitive-review.md)）的全部修复 PR #173-182 / #188 / #191 / #192 合并之后。
**方法**: 4 个代码级旅程走查代理（高级 PM + 普通用户双视角，逐条验证 R1 修复的完整链路 + 挖掘新问题），竞品事实沿用 R1 同日调研（§3 全部对照表以 R1 为基线，本报告只更新增量）。报告中最重的 8 条新断言已由主会话逐一对照源码实证（清单见 §6.2）。

---

## 0. TL;DR

**R1 的 30 项改进不是纸面修复**：P0×8 全部真实落地且工程质量普遍高于修法底线（拒绝路径带结构化回执、粘贴缓存带威胁模型测试、导出把「用户在原生对话框的选择=授权」写成明确契约）；P1×12 中 9 项 FIXED、3 项 PARTIAL；P2×10 中 5 项落地、2 项未动。R1 识别的「装了→能用」断裂对 **stdio MCP、技能、本地 agent 三线已真实闭合**——这是本轮最重要的确认。

但走查发现了 **3 个新的 P0 级「说谎 UI」**，全部属于 R1 已诊断过的反模式家族（UI 报成功/承诺能力，实际不成立）：

1. **OAuth/remote MCP 在桌面端是死的**：OAuth 连接成功 toast 之后，url-only server 因 desktop 结构体无 `url` 字段被 seed 显式跳过，`start_remote_server` 在 desktop/src 零调用——服务器永远「Offline」，与 Claude 主打的 remote connector 直接对撞。
2. **技能装完立即对话 = 提示词向模型承诺不存在的工具**：工具只在 app 启动时注册一次；新装技能进入 SkillRegistry 并被写进下一轮系统提示（`/name` ↔ `skill_<name>`），但 ToolRegistry 里没有——模型被指向一个不存在的调用。
3. **OPC 运行表与 History 的 cost/token 列是空壳**：UI 列已上线（对标 Notion All chats 的卖点列），但 `routine_runs` 从不写 cost（`cost_usd: None` 有测试自证「never tracked」）——表格上线反而放大了承诺落差。

第二类残留是**静默失败的最后据点**（7 处 → 2 处主残留）：svg/bmp 图片被选择器收下后遭多模态白名单静默排除（无 rejected 无徽标，用户以为模型看到了图）；webhook「发送测试」测的是**上次保存的旧配置**而非表单所见。

竞品对比格局大幅改观：R1 识别的四大产品级差距中，**权限档常驻 composer、用量治理（常驻条+双阈值+成本预估）、扩展消费闭环、跨 agent 运行表**已闭合或大幅收窄；仍在的差距收敛为四项——**NL 自动化的「结构化预览→激活确认」步（Manus）、记忆引用可回跳（Claude）、失败自动暂停+needs action（ChatGPT/Manus）、remote connectors**。另有一项 R1 遗留未兑现：中文 NL cron 已修复（确定性解析），但 i18n 整体「骨架级多语言」仅从 79-80% 降到 67%——旧设置子页（permissions/voice/theme 等）在 ja 下 90-97% 仍是英文。

**综合改进方案**（§5）：P0×3 信任修复 + P1×10 残留清账 + P2 战略项 4 个产品级模式与 10 项打磨。其中 2 个产品拍板项**已裁决**（R4 remote MCP 分段接线、R5 P2 季度取舍，见 §5 Rulings）。

---

## 1. 方法与范围

- **走查范围**：与 R1 相同的四条旅程（J1 交付 / J2 自动化 / J3 生态 / J4 配置），全部 12 个页面区 + 伴随窗口。
- **验证标准**：不看「代码存在」看「链路闭环」——前端事件 → Tauri 命令 → 后端执行 → 状态回显 → i18n，任何一环缺失即记 PARTIAL。
- **与 R1 的关系**：R1 §2/§3 是本报告的事实基线（竞品 UI 事实为同日调研，无需重采）；本报告 §2 逐条给出 R1 30 项的验证结论，§3 只记录**残留与新发现**，§4 只更新**竞品对比的增量**。
- **主会话实证**：J3 url-only 零调用、J2 cost 恒 None（测试自证）、J2 `.claude/tasks` CWD 相对、J1 svg/bmp 白名单排除、J1 预算续发机制、J3 技能注册时机与提示词注入点、J4 i18n 残留率脚本复算（de 67.9% / ja 66.9%，与走查代理数字一致）。

---

## 2. R1 修复验证总表

### 2.1 统计

| 批次 | FIXED | PARTIAL | NOT-FIXED |
|---|---|---|---|
| P0 ×8 | **8**（其中 P0-1 留 1 条重大残留 → 本轮新 P0） | — | — |
| P1 ×12 | 9 | 3（P1-3 终态可见性、P1-8 旧子树、P1-9 项目级 agent） | — |
| P2 ×10 | 5 | 3 | 2（P2-2 NL 确认步、P2-5 的记忆回跳半项） |

### 2.2 P0 逐项验证

| R1 # | 事项 | 结论 | 关键证据 | 残留 |
|---|---|---|---|---|
| P0-1 | MCP 三重断裂 | **FIXED** | 存储统一 `config.rs:291-297`（原子写 + 启动迁移先于任何读取 `main.rs:658-663`）；后台 seed 单点失败不阻塞（`mcp.rs:34-71`）；聊天每 turn `assemble_mcp_tools`（`commands.rs:1342-1348`）+ 权限页按 server 管理（X3） | **url-only/remote server 全链路死**（→ 本轮 R2-P0-1）；seed 串行握手期间首 turn 静默 0 工具 |
| P0-2 | 技能运行时接入 | **FIXED** | 启动注册 `main.rs:615-625` → `skill_tools.rs:149-206`；每 turn 提示词注入映射（`commands.rs:1411-1414`）；slash 补全动态化带超时兜底 | 装完不重启不可用且提示词说谎（→ 本轮 R2-P0-2） |
| P0-3 | 附件静默丢弃 | **FIXED** | 每个拒绝路径返回结构化 RejectedAttachment（`commands.rs:893-1001`）→ 逐文件 toast + chip 预检徽标；预检与发送共用同一分类器（`lib.rs:102-132`，测试钉死） | 无 working dir 硬错误英文直出（P2） |
| P0-4 | 立即运行假动作 | **FIXED** | `trigger_task_now` → `spawn_routine_run` 真执行（`scheduled_commands.rs:811-849`），SQLite-first + budget 门；UI in-flight 守卫 + toast 区分「已触发」 | disabled 例行可 RunNow、无后端并发守卫（P2） |
| P0-5 | ResultRouting 死 UI | **FIXED（下线）** | 全仓 `result_routing` 0 命中；webhook 通道真实接线且未配置跳过会注记进 run record（`inbox_commands.rs:1332-1351`）——符合 R1 裁决 | notification 通道接线留 backlog（R1 裁决） |
| P0-6 | Add-to-chat 丢稿 | **FIXED** | 先 navigate 再 push + pending 队列订阅即冲刷（`DataSourcesQuery.tsx:250-259`、`composerBridge.ts:46-88`），按序排队不重复 | 队列无 TTL（可接受） |
| P0-7 | 测试连接假报 | **FIXED** | 单卡/弹窗/Test all 收敛到 `probe_and_map` 单实现（`commands_config.rs:1236-1265`），单卡走 `testProviderCredentials` + credential store 回退；6 类错误分类 | 402/quota 无分类（P1 残留） |
| P0-8 | 时间线导出沙箱拒绝 | **FIXED** | 后端原生保存对话框+直写所选路径（`commands_files.rs:623-698`，「对话框选择即授权」契约），ACL 已登记 | — |

### 2.3 P1 / P2 验证摘要

- **已修复**（抽样证据）：例行生命周期 UI（RoutineLifecycleRow + BasicsEditor，按持久化 bool 回显防竞态）；policy 四字段全部接线（timeout→attempt abort `inbox_commands.rs:453-482`、max_retries→退避重试 `:490-530`、budget→月度聚合超限跳过 `:597-619`、worktree→建树降级+警告 `:638-688`）；Runs 页后台任务分区；OPC 快建上板三态反馈；两套自动化体系解释文案（ADR-0013 接缝修复）；粘贴图片（窄允许域+逃逸测试）；UNPARSED 横幅收窄为真实集合；PDF 截断恢复+全类型提取徽标（AttachmentExtractionReport→FileCard「查看提取文本」）；companion 激活（Header/命令面板入口+拉取式草稿桥）；@ 文件引用（工作目录树+邮箱不误触发）；**权限四档常驻 composer**（R3 裁定档位）；webhook 测试发送（SSRF 守卫+三组单测）；CONFIG_UPDATED→模型目录联动；profiles 路径锚定；侧栏用量条+80/100% 阈值通知+任务前成本预估；记忆会话级旁路（全链路含 dream 过滤）；`.mcpb` 入口；中文 NL cron（零 token 确定性解析含繁体/全角）。
- **PARTIAL**：后台任务终态仍无呈现（面板只渲染 running，不进 History/inbox）；i18n 只覆盖新增命名空间（旧设置子树骨架，见 R2-P1-8）；extensions agent 已统一扁平 TOML 但 loader 项目级目录仍 cwd 相对。
- **NOT-FIXED**：P2-2 NL「结构化预览→激活确认」步（表单直接 submit）；记忆引用回跳（chat 内零记忆信号）。另 J2-8 中的 P2 杂项多数未动（双重 toast、模板 "0s"、批次无停止），但「IMAP 开发中」旧断言经复核**不成立**（后端确无 IMAP，模板文案与实现一致，R1 该条撤回）。

---

## 3. 本轮走查发现（残留 + 新问题）

> 编号 R2-*。分级口径与 R1 相同：P0=说谎 UI/静默失败（信任级），P1=承诺兑现/旅程断点，P2=打磨。

### 3.1 J1 交付旅程

| # | 级别 | 发现 | 证据 | 建议 |
|---|---|---|---|---|
| R2-P1-1 | **P1** | **预算拦截后「继续一次」三重失真**：pre-turn 拒绝回滚乐观消息且 composer 已无条件清空 → 被拦的那条（含附件）彻底消失；Continue 取的是 `messages` 里**更早轮**的旧消息重发（「继续」变「重放旧问题」）；首轮即超限时无 user 消息 → 按钮死点 | `Chat.tsx:291-294`（reverse find lastUser）、`AppContext.tsx:544-552`（回滚）、`Chat.tsx:436-438`（清空） | 拒绝时把草稿+附件退回 composer；Continue 透传被拦内容或明示「将重发上一条」 |
| R2-P1-2 | **P1** | **svg/bmp 违反自家「无静默拒绝」新契约**：选择器 filter 允许 → 附件收下 → 多模态白名单静默排除，无 rejected 无徽标——用户以为模型看到了图 | `ChatInput.tsx:35`（IMAGE_EXTENSIONS 含 bmp/svg）vs `commands.rs:1102-1112`（注释自认 SVG excluded） | filter 移除两类，或回执加 unsupported_media 徽标 |
| R2-P2-1 | P2 | 流式中纯附件 Enter 仍静默 no-op（R1 J1-5 明知残留） | `Chat.tsx:441-443` | 排队或提示 |
| R2-P2-2 | P2 | 后端硬错误英文直出（无 working dir 附件错误等非 auth 错误裸渲染） | `commands.rs:856` → `MessageArea.tsx:383` | 后端返结构化 tag，前端映射 i18n |
| R2-P2-3 | P2 | agent 发现域仍回退进程 CWD（Dock 启动=`/`），与附件域新哲学不一致 | `commands_agents.rs:196-204` | 收口到附件域同款显式语义 |
| R2-P2-4 | P2 | 附加 100MiB PDF 即全量解析（fire-and-forget 无超时），徽标可能长期缺席且无「解析中」态 | `commands_files.rs:294-310` | 解析中占位+超时 |

亮点：预检/发送共享分类器由构造保证一致；粘贴缓存窄允许域带完整威胁模型注释+逃逸测试；提取透明度（徽标+缓存+查看全文）**已超前所有竞品**（R1 §3.3 建议 3 完整兑现）。

### 3.2 J2 自动化旅程

| # | 级别 | 发现 | 证据 | 建议 |
|---|---|---|---|---|
| R2-P0-3 | **P0** | **OPC 运行表 cost/token 列空壳上线**：UI 已渲染（对标 Notion All chats 的卖点列），但 `routine_runs` 从不写 cost——源码注释与测试双双自证「never tracked」；History 成本列同病（UI 条件渲染就绪、数据恒 None） | `OPCRunsTable.tsx:197-206`、`join_agent_runs scheduled_commands.rs:1465-1468`、`inbox_commands.rs:192,207`、测试 `:2585` | 按 session 从 usage ledger 聚合（model 列同款 join 已有先例 `scheduled_commands.rs:1509-1570`）；接不上就删列 |
| R2-P1-3 | **P1** | **OPC 上板链依赖 CWD 相对 `.claude/tasks`**：macOS GUI 启动 cwd=`/` 时 `create_dir_all` 失败 → 快建永远走 boardSyncFailed 分支（有 toast 不静默，但功能必坏） | `commands_tasks.rs:231-250`（`Path::new(".claude/tasks")`） | 锚定 workspace/working_dir 绝对路径 |
| R2-P1-4 | **P1** | 例行创建双重 toast 回归（hook「任务已创建」+页面「已排程」同屏两条） | `scheduled-tasks.ts:47` + `Tasks.tsx:235` | hook 去 toast，页面统一 |
| R2-P1-5 | **P1** | 后台任务终态无任何呈现（created-then-invisible 的下半截）：面板只渲染 running，完成态不进 History/inbox | `BackgroundTasksPanel.tsx:34-52` | 完成时落 inbox item 或 Runs 页终态分区 |
| R2-P2-5 | P2 | `notify_on_failure`/`auto_archive_when_empty` 仍 UI 可配零消费——R1 修 policy 四字段时漏掉的同面板两兄弟 | `ScheduleForm.tsx:438-451` | 接线或标注「规划中」 |
| R2-P2-6 | P2 | Run Now 对非例行（catalog）任务把 i18n 文案当 prompt 建后台任务（引擎收到「执行任务：X」伪指令）且绕过分配表单 | `Tasks.tsx:264-266` | catalog 任务隐藏 RunNow 或确认走 NewTaskForm |
| R2-P2-7 | P2 | 暂停后的例行仍可 RunNow；后端无并发 trigger 在飞守卫 | `spawn_routine_run:868` 无 enabled/in-flight 检查 | 暂停态给确认；trigger 侧去重提示 |
| R2-P2-8 | P2 | 模板卡 github 触发器裸 "0s"、interval 裸秒（R1 J2-8⑤ 原样残留） | `RoutineTemplatesBrowser.tsx:163-167` | event/repo 语义化 + 时长人性化 |

亮点：所有触发源汇入统一执行路径（SQLite 权威+JSONL 降级、panic/timeout 永不残留 running）；「跳过必须可见」成为正面示范（webhook 未配置注记进 run record、budget 超限以 failed+原因落 History）；中文 NL cron 零 token 确定性解析。

### 3.3 J3 生态旅程

| # | 级别 | 发现 | 证据 | 建议 |
|---|---|---|---|---|
| R2-P0-1 | **P0** | **OAuth/remote MCP（url-only）全链路死**：安装写 `{"url":...}`，desktop 配置结构体无 url 字段 → 解析成空 command 行 → seed 显式跳过；`start_remote_server` 在 desktop/src **零调用**（crate 能力存在）。用户视角：OAuth 成功 toast → 列表永远 Offline → restart 对无 command 条目也必失败，零诚实提示 | `mcp_installers.rs:295,441-445`、`config.rs:291,731-763`、`mcp.rs:39-45`；grep `start_remote_server` desktop/src 0 命中（已实证） | 二选一（见 §5 P0-1）：接 `start_remote_server`（对标 Claude remote connector，L）；或 url-only 行显示「桌面端即将支持」徽章（S） |
| R2-P0-2 | **P0** | **技能装完立即对话 = 提示词向模型承诺不存在的工具**：工具注册只在 app setup 一次；`list_skills` 卻把新技能 hydrate 进 SkillRegistry → 下一 turn 系统提示宣传 `/name`↔`skill_<name>`，ToolRegistry 里没有——模型被指向不存在的调用，用户看到幻觉失败 | `main.rs:615-625`（仅启动注册）、`commands_mcp.rs:266-271`（list_skills 只进 registry）、`commands.rs:1411-1414`（提示词读 registry） | `list_skills` 顺带幂等重跑 `register_skills_as_chat_tools`（重名跳过，S） |
| R2-P1-6 | **P1** | MCP 连接失败不可诊断：`list_mcp_servers` 不回传 last_error/last_connected（硬编码 None），Pending 错误区持续空置——R1 J3-6 的诊断断链半条未修 | `commands_mcp.rs:220-228`、`Pending.tsx:44-69` | 池句柄已有失败状态，透传 last_error 即闭环 |
| R2-P2-9 | P2 | disabled server 渲染成「Offline」坏态；restart 可强启 disabled 条目；enabled 字段 UI 完全不渲染 | `commands_mcp.rs:126-163` | Enabled/Disabled 徽章+开关 |
| R2-P2-10 | P2 | settings.json 损坏时列表静默清空（读侧静默 `Vec::new()`，写侧却报错防重置——读写不对称） | `config.rs:707-712` vs `:819-831` | load 失败返回 Err → UI 错误态 |
| R2-P2-11 | P2 | MCP 工具数徽章仍兼职在线状态（connected 且 0 工具显示「0 tools」） | `McpServers.tsx:296-306` | 状态与工具数分离 |
| R2-P2-12 | P2 | IMAP/Notion 凭据仍明文落盘（keychain 预留未接）；rss/ical 后端目录文案仍宣称「Fetch and search」与 config-only 徽章矛盾；billing 三命令死代码；agent_defs 项目级目录 cwd 相对 | `data_source_installers.rs:10`、`data_source_catalog.rs:368,392`、`main.rs:503-506`、`agent_defs.rs:325-336` | 按 R1 口径维持 |

亮点：seed 时序语义注释+「单点失败不致命」测试是教科书级修复自证；OAuth url-only 断链的定位绕过了 R1 全部检查（安装/列表/重启三环各自「看起来正常」）——说明逐环验证不够，需要**端到端安装→调用验收用例**（见 §5）。

### 3.4 J4 配置旅程

| # | 级别 | 发现 | 证据 | 建议 |
|---|---|---|---|---|
| R2-P1-7 | **P1** | webhook「Send test」测的是**上次保存的旧配置**：改了 URL 未保存即点测试，verdict 与表单所见无关 | `NotificationsSettings.tsx:186-206` + `commands_notifications.rs:217-230`（读已存 config） | dirty 态先保存再测，或按钮提示「使用已保存配置」 |
| R2-P1-8 | **P1** | **i18n 旧设置子树骨架**：整体英文残留 79-80% → **67%**（de 67.9%/ja 66.9%，主会话脚本复算一致），新增表面全翻且质量良好，但 ja 的 settings.* 902 键中 472 键（52%）仍英文：personaPack/voiceLocal/voice/skillLoop/dream 100%、theme 97%、permissions 95% 未翻 | 各 locale json 对应命名空间 | 一次批量机翻+人工校对（旧子树优先 zh-TW/ja/ko/de） |
| R2-P1-9 | **P1** | 命令面板设置子页仍 6/8 不可达（R1 P2 升格：命令面板是设置「万能入口」的承诺对 permissions/notifications/connections/remotes/advanced/general 不成立） | `CommandPalette.tsx:76-78` | 补 palette.category.settings 六项 |
| R2-P2-13 | P2 | 402/quota/欠费落 Unknown 显示原始英文——测试连接信任锚点对最容易踩的坑仍失灵 | `commands_config.rs:1246-1265`（仅 401/429/5xx/timeout 分支） | 加 402→quota_exhausted 分类 |
| R2-P2-14 | P2 | 内置权限档名称/描述引擎英文硬编码（ja 用户权限页 96% 英文） | `automation_commands.rs:293-298`、`PermissionsSettings.tsx:622` | 按 profile.id 前端映射 i18n |
| R2-P2-15 | P2 | 切预设把含 `<token>` 的占位符写进 url state 可原样保存；timeout 非正数静默忽略 | `NotificationsSettings.tsx:30-80,113-117,176-179` | 占位符仅作 placeholder；无效输入 inline 错误 |
| R2-P2-16 | P2 | Remotes 已知限制（PTY/worktree local-only）零告知；`profiles.*` 44 键死键全 locale 英文误导翻译统计 | `RemotesSettings.tsx`、en.json profiles.* | 限制卡；死键删除或接线 |

亮点：save≠verify 双路径收敛单实现三入口永远一致；i18n 三层防线（key parity 红绿测试+copy-rate 报告+CI 门禁）；托盘 BCP47 归一化测试；#191 ACL 273 命令逐一比对无缺漏且 #193-197 CI 调整未发现掩盖真实问题。

---

## 4. 逐页竞品对比（R2 增量更新）

> 竞品基准沿用 R1 §3（同日调研）。每页给出：R1 结论 → R2 现状（✅已闭合 / ◐收窄 / ❌仍在）。

### 4.1 Onboarding（/welcome）
◐ 「working_dir 是隐式概念」已部分显式化（附件错误 toast + composer 横幅 + 设置深链）；ChatGPT 式首启「选工作区步骤 + starter 用例」❌ 仍缺；Manus 式 OS 原生授权弹窗 ❌ 不适用（附件域已改为显式拒绝语义，无需授权模型）。

### 4.2 核心对话（/chat）
R1 三条差距两条已闭合：✅ 权限档常驻 composer（四档 select + high-risk 提示随档走）；✅ @ 文件引用（工作目录树补全）。◐ steering 有「立即/排队」两档，仍缺 Cursor 的「本轮结束后」档；◐ ChatGPT Work 式运行期四要素已有 runProcess slate，信息密度未对齐。新差距：R2-P1-1 预算续发失真是竞品都没有的坑（竞品触顶不截断进行中任务）。

### 4.3 附件与文件
✅ PDF 截断恢复路径闭合（绝对路径+缓存+徽标）；✅ 提取透明度徽标**超前竞品**（Claude/ChatGPT 均不标注截断，Shannon 已做「已提取 N 段/查看全文」）。❌ 仍在：大小上限口径（10MiB 图/100MiB PDF vs ChatGPT 512MB）、跨会话文件库（/files 仍索引式）、预览内标注回修（R1 裁决不做，维持）。新增：R2-P1-2 svg/bmp 静默排除违背「附件诚实」人设。

### 4.4 会话管理与时间线
✅ 导出沙箱断链闭合（原生对话框=授权契约）+ 新增打印→PDF；◐ replay 语义（时间线是读不仅要看）、分支可视化 ❌ 仍缺（维持 R1 建议优先级）。

### 4.5 任务/自动化（/tasks）
**R1「控制面断代级差距」判定已过时**：✅ Pause/Delete/Edit 齐备（竞品及格线跨过）；✅ 中文 NL cron。❌ 仍差的收敛为两项：**对话式创建+「结构化预览→激活确认」步**（Manus「review the generated timing/conditions/actions」范式，R1 P2-2 未动）与**失败自动 pause + needs action 前置强调**（ChatGPT/Manus 有，Shannon 失败只落 run record + triage error 折叠）。◐ RunNow 已真但无 Test run 语义包装（Manus 正/负例文案可直接抄）。新发现：R2-P0-3 成本列空壳——对标 Notion 的表格反成承诺落差放大器。

### 4.6 收件箱（/triage）
✅ 「高于所有竞品公开形态」结论成立且更强（摘要前置✅、错误原因卡上✅、双向往链✅）；◐ 状态词/「needs action」级前置强调仍缺一半。

### 4.7 多 agent 看板（/opc）
✅ 「无跨 agent 运行表」已过时——runs 表对齐 Notion All chats（status/model/session/耗时+跳会话）；✅ 快建上板。❌ 新落差：cost/token 列空壳（R2-P0-3）；◐ Cursor 式 diff/分支徽标、Spawn 名实不符（R1 口径维持，本轮未复测）。

### 4.8 扩展生态（/extensions/*）
**R1「决定性差距不在安装、在消费」需改写为「消费已通，唯 remote 断」**：✅ stdio MCP 安装→启动 seed→每 turn 装配→权限页管理全链路闭合；✅ 技能启动注册+动态 slash；✅ 本地 agent 格式统一+幂等迁移；✅ `.mcpb` 入口+工具级权限管理（对 Claude 两项建议落地）。❌ **remote/OAuth connector 是唯一死线**（R2-P0-1）——恰是 Claude 连接器目录的主打形态；◐ 工具级 toggle（Claude 卡片开关）仍未做；❌ 技能热装错位（R2-P0-2，竞品装完即用）。

### 4.9 记忆（/memory）
✅ ChatGPT 式「临时对话旁路」已上线且语义更严（旁路会话不进提取）；◐ Manus 式生效边界声明由旁路横幅部分覆盖。❌ Claude 式「回答引用记忆可回跳源对话」仍缺（chat 内零记忆信号）——R1 P2-5 的半项，溯源链路后端已有，缺 UI 呈现。

### 4.10 用量（/usage）
**R1 三条建议落地两条半**：✅ 侧栏常驻用量条（Claude 模式）；✅ 80/100% 双阈值告警（Notion 模式）；✅ 任务前成本预估（Manus-lite）。◐ 仍在：双池显示（订阅/按量）、**budget 触顶自动暂停**（目前只通知不打断——与 P2-1 联动的执行器单点）；❌ 告警依赖 app 前台轮询（app 不开不告警）。

### 4.11 设置（/settings/*）
✅ R1 两个坏掉的「信任锚点」全部修复且高于修法底线（错误 6 分类、SSRF 防线、save≠verify 收敛）；✅ 模型目录即时联动（Cursor 式「保存即出现在选择器」）；✅ Discord webhook 预设。◐ 「多语言名不副实」收窄未关闭（67% 残留，旧子树骨架）。❌ 仍在：权限档描述 i18n、命令面板设置可达性、Remotes 限制告知。BYOK+multi-key+failover 深度优势随 keys.* 面板+rotationNote 更实。

### 4.12 全局横切
✅ 19 处硬编码 settings toast → 0；✅ 托盘 10 locale+BCP47；✅ IM 去向引导文案。◐ failover 通知仍无聚合入口。❌ a11y 缺口维持 R1 口径（ModelsSettings/ThemeSettings aria-pressed）。i18n 从「键满分值不及格」演进为「新表面及格、旧子树不及格」。

---

## 5. 综合改进方案 R2（供审核）

> 排序原则同 R1。规模：S=≤1 天、M=2-5 天、L=1-2 周。R1 未冻结项（notification 通道接线、多次失败自动 pause）继续保留在对应批次。

### Rulings（已拍板，2026-10-01）

> 延续上一轮裁决账本编号（R1 结果路由分阶段 / R2 IA 接缝修复不迁移 / R3 审批档合并为引擎真实 4 档），本轮新增两条：

- **R4 · remote/OAuth MCP（R2-P0-1）**：B（诚实化）**立即发布** + A 分两段——**A1**（下迭代，M）：desktop 配置结构体补 `url` 字段 → seed 走 `start_remote_server`，先支持无 OAuth 的纯 HTTP/SSE remote（覆盖当前 remote 生态主流形态，无 token 生命周期问题）；**A2**（单独立项，L）：OAuth remote + token 刷新/静默重连——失败呈现方案（token 过期不再造「昨天能用今天 Offline」说谎态）明确前不启动。B 的诚实态在 A 落地后**长期保留**（用于 token 失效等 genuinely 不可用场景）。
- **R5 · P2 产品级季度取舍**：**R2-P2-D（budget 触顶自动暂停）与 R2-P2-A（NL 结构化预览确认步）季度必做**——D 把已建成 80% 的成本治理故事补完最后一针（与 R2-P0-3 同执行器域，顺手一起做）；A 是自动化这个信任洼地的最高杠杆范式。**R2-P2-C（失败自动 pause）stretch**，尽量与 A 同季（同域避免二次动 ScheduleForm/routine 生命周期 UI）；**R2-P2-B（记忆引用回跳）延后下季度**——是在已很强的记忆面上加分而非修信任回路，后端链路已在，晚做不吃亏。

### 第一批 P0 —— 说谎 UI 清零（建议一个 hotfix 窗口，合计约 1-2 周）

| # | 事项 | 修法 | 规模 |
|---|---|---|---|
| R2-P0-1 | remote/OAuth MCP 死线 | **已裁决（Ruling R4）**：B（诚实化，S）立即发——url-only 行显示「桌面端即将支持，请使用 CLI」徽章、restart 对 url-only 禁用；A1（M）下迭代接纯 HTTP/SSE remote；A2（L）OAuth remote 单独立项 | B=S / A1=M / A2=L |
| R2-P0-2 | 技能热注册错位 | `list_skills` 顺带幂等重跑 `register_skills_as_chat_tools`（重名跳过）；顺带补一个「装完即聊」的 e2e 测试 | S |
| R2-P0-3 | cost/token 空壳列 | 按 session 从 usage ledger 聚合 cost/token（model 列同款 join 先例在 `scheduled_commands.rs:1509-1570`）；若一个迭代内接不上则**先删列**（R1 P0-5 同款裁决：挂着比不做更伤） | M（接）/S（删） |

**防复发机制建议**（本轮最大教训）：R2-P0-1 三环各自正常、整体断裂，逃过了 R1 的逐环走查。建议给「安装→可用」建端到端验收清单（每类扩展一条：装→重启→列表态→聊天内工具可见→可调用），进 CI 或发版 checklist。

### 第二批 P1 —— 残留清账（一个迭代）

| # | 事项 | 说明 | 规模 |
|---|---|---|---|
| R2-P1-1 | 预算「继续一次」修正 | 拒绝时草稿+附件退回 composer；Continue 透传被拦内容或明示「将重发上一条」；首轮超限给可用路径 | M |
| R2-P1-2 | svg/bmp 诚实化 | filter 移除或 unsupported_media 回执徽标 | S |
| R2-P1-3 | OPC 上板路径锚定 | `.claude/tasks` 锚定 workspace 绝对路径（与 R1 J4-4 同款一行修） | S |
| R2-P1-4 | 双重 toast | hook 去 toast | S |
| R2-P1-5 | 后台任务终态呈现 | 完成落 inbox item 或 Runs 页终态分区 | M |
| R2-P1-6 | MCP 失败可诊断 | last_error/last_connected 透传 → Pending 错误区复活（顺带 J3-6 闭环） | S |
| R2-P1-7 | webhook 测试语义 | dirty 先保存再测，或明示「使用已保存配置」 | S |
| R2-P1-8 | i18n 旧子树批量机翻 | ja 的 settings.* 472 键 + 同类，优先 zh-TW/ja/ko/de；顺带 R2-P2-14/16 死键清理 | M |
| R2-P1-9 | 命令面板设置 6 子页 | palette.category.settings 补齐 | S |
| R2-P1-10 | 402/quota 错误分类 | probe_and_map 加 402 分支 | S |

### 第三批 P2 —— 产品级模式（对标差距，按 ROI 排序）+ 打磨

**产品级（季度取舍已裁决，见 Ruling R5）**：

| # | 事项 | 来源竞品 | 说明 | 排位（R5） |
|---|---|---|---|---|
| R2-P2-A | NL 自动化「结构化预览→激活确认」步 | Manus | R1 P2-2 未动；cron 预览已有，扩展到 prompt/触发器/通知全量预览+确认 | **季度必做** |
| R2-P2-B | 记忆引用可回跳 | Claude | 溯源链路后端已有，只缺回答内引用呈现；R1 P2-5 另半项已完成 | 下季度 |
| R2-P2-C | 失败自动 pause + needs action | ChatGPT/Manus | 例行连续失败自动暂停+通知+自愈路径；Test run 正/负例文案可同步抄 | stretch（尽量与 A 同季） |
| R2-P2-D | budget 触顶自动暂停 | Notion | budget_usd 已接月度聚合（R1 P1-2），差执行器「超限即断」单点；与 R2-P0-3 同域 | **季度必做**（可提前至迭代 N） |

**打磨（顺手清）**：notify_on_failure/auto_archive 接线或标注；disabled MCP 徽章+开关；settings.json 损坏错误态；工具数/状态分离；模板 "0s" 人性化；catalog 任务 RunNow 伪指令；暂停态 RunNow 确认；流式纯附件 no-op；后端硬错误结构化 tag；大 PDF 解析中态；timeout 无效输入 inline 错误；webhook 占位符不进 state；Remotes 限制卡；profiles 死键清理；keychain 接入；agent 发现域 CWD 收口。

### 不建议做（维持/新增裁决）

- 维持 R1 三条：预览内圈选回修、IM 消息 inbox 化、全 locale 100% 翻译冲刺。
- 新增：**不为 67% 残留做全量人翻**——机翻旧子树 + 诚实 beta 标注即可（R2-P1-8 口径）；**不为 cost 列做实时估算**——usage ledger 聚合是唯一真源，估算会再造一个说谎 UI。

### 排期建议（已按 Rulings R4/R5 更新）

- **Hotfix（~1 周）**：R2-P0-2、R2-P0-1(B 诚实化)、R2-P0-3(删列兜底)、R2-P1-2/3/4/6/7/9/10（全是 S）。
- **迭代 N（~2-3 周）**：R2-P0-1(A1 纯 HTTP/SSE remote 接线)、R2-P0-3(usage ledger 聚合接线)、R2-P1-1/5/8、R2-P2-D（与 P0-3 同执行器域，顺手做）。
- **季度**：R2-P2-A（必做）+ R2-P2-C（stretch，尽量与 A 同季）；R2-P2-B 延后下季度；R2-P0-1(A2 OAuth remote) 单独立项评估；IA 长期债维持 ADR-0013 接缝口径。

---

## 6. 附录

### 6.1 编号索引

R1 30 项验证结论见 §2；本轮新发现 R2-P0-1..3（§3.2/§3.3）、R2-P1-1..10（§3.1-3.4）、R2-P2-1..16 + R2-P2-A..D（§3/§5）；裁决 R4/R5 见 §5 Rulings。

### 6.2 主会话实证清单（本报告最重 8 条断言）

1. `start_remote_server` 在 desktop/src 零调用（grep 实证）→ R2-P0-1
2. 技能注册仅在 setup（main.rs:615-625），提示词注入点 commands.rs:1411-1414 → R2-P0-2
3. `cost_usd: None` + 测试自证「never tracked in routine_runs」（inbox_commands.rs:192,207,2585）→ R2-P0-3
4. `Path::new(".claude/tasks")` CWD 相对（commands_tasks.rs:231-250）→ R2-P1-3
5. IMAGE_EXTENSIONS 含 bmp/svg（ChatInput.tsx:35）vs SVG excluded 注释（commands.rs:1102）→ R2-P1-2
6. continuePastBudget 取 messages 内 lastUser 重发（Chat.tsx:291-294）→ R2-P1-1（回滚链路由走查代理锚定 AppContext.tsx:544-552）
7. i18n 残留率脚本复算：de 67.9% / ja 66.9%（与走查代理一致）→ R2-P1-8
8. webhook 测试读已存 config（commands_notifications.rs:217-230）→ R2-P1-7
