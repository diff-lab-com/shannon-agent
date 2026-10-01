# Shannon Desktop 全页面 User Journey 走查 + 逐页竞品对比 + 综合改进方案

**日期**: 2026-10-01
**基线**: `dev` @ e8cbc2f5（办公场景 Wave 1/1.5/2/3 — PR #157/#161/#162/#164 — 已全部合并之后）
**方法**: 6 个并行调研代理 —— 4 个代码级旅程走查（以高级产品经理 + 普通用户双视角，按 user story 逐步追代码验证接线），2 个竞品 UI 交互层调研（Claude/ChatGPT/Codex 桌面端 + Cursor/Manus/Notion/Gamma，以官方文档为主，全部标注来源）。报告中 8 处最重的 P0/P1 断言已逐条对照源码复核。
**与既有调研的关系**: 本报告不重复 2026-09-16 全页面 UI Review（20 条 bug 级发现，17 已修）与 2026-09-29 办公场景报告（462 行）；本轮验证了那两轮的修复落地情况，并把范围扩展到全部 12 个页面区的 user journey 与逐页竞品对比。

---

## 0. TL;DR

UI 层完成度、状态诚实度（error/empty/loading 三分）、无障碍纪律都达到了竞品水准，onboarding 和收件箱两条旅程甚至优于多数竞品的公开形态。**但走查发现一个系统性断裂：用户在「扩展」里装的一切（MCP server、技能、agent）在桌面聊天里根本不存在**——装完列表看不见、状态永远离线、聊天里永远没有这些工具，三条同时成立。这是「装了 → 运行时能用」的最后一公里问题，MCP/技能/agent 三条线全部中招，根因是桌面聊天的工具装配点与扩展体系的存储约定从未接通。

第二类高频反模式是**静默失败**（7 处）：附件路径不合规则凭空消失、「添加到聊天」丢稿却报成功、例行「立即运行」点了没有任何事发生、表单配置被后端 serde 静默丢弃、导出被沙箱拒绝、已存 key 的 provider 测试连接永远假报「Invalid key」。共同点：**UI 报成功或无反馈，用户以为给了、系统其实没收到**——这类问题对信任的伤害大于缺功能。

第三类是**UI 提前承诺了执行层不存在的能力**（安慰剂字段群）：自动化的 max_retries/timeout/budget/worktree 四个字段全部可配、全部不生效；结果路由三通道（email/notification/log）被 serde 静默丢弃；重试模块写完了但零调用。

竞品对比的结论：单点交互上 Shannon 的多数设计（composer 集成度、权限弹窗信息密度、triage 批量操作）不落后；差距集中在四个产品级模式——**任务运行期的「过程可见性」**（ChatGPT Work 侧栏四要素、Manus 直播+回放）、**自动化创建的「生成前确认」**（Manus NL→结构化预览→激活确认）、**成本治理**（Manus 任务前预估、Notion 80%/100% 阈值+触顶自停）、**权限档常驻 composer**（ChatGPT/Claude 三档就地切换）。

**综合改进方案**（§5）：8 项 P0 信任修复（多数是接线级工作量）+ 12 项 P1 承诺兑现 + 10 项 P2 对标差异化，外加 1 个结构性 IA 收敛决策（三套任务存储/两套 agent 概念合一）需要产品拍板。

---

## 1. 方法与范围

### 1.1 页面清单（App.tsx 实测）

| 区域 | 路由 | 状态 |
|---|---|---|
| Onboarding | `/welcome` | 主线 |
| 核心对话 | `/chat`（+RightDock 五 tab：上下文/计划/预览/Diff/artifact） | 主线 |
| 文件 | `/files` | 主线 |
| 任务/自动化 | `/tasks`（Runs/History/Routines/Pipelines/Workspaces 五 tab） | 主线（后三 tab Dev 模式） |
| 收件箱 | `/triage` | 主线 |
| 多 agent 看板 | `/opc`、`/opc/task/:id` | Dev 折叠 |
| 扩展 | `/extensions/{featured,mcp-servers,skills,agents,datasources,plugins,installed,pending}` | 主线 |
| 记忆 | `/memory` | 主线 |
| 用量 | `/usage` | Dev 折叠 |
| 时间线 | `/timeline/:id` | 深链 |
| 设置 | `/settings/{general,theme,models,permissions,advanced,notifications,connections,remotes}` | 主线 |
| 伴随窗口 | `/companion` | 托盘入口 |

重定向已收敛：/goals /routines /hooks /profiles→/tasks，/strategic-focus /agent-swarm→/opc，/editor→/chat。

### 1.2 四条走查旅程

- **J1 交付旅程**：新用户首启 → 核心对话循环 → 附件全类型 → 会话管理 → 文件页 → 伴随窗口
- **J2 自动化旅程**：创建定时任务（NL→cron）→ 模板 → 后台任务 → 流水线/工作树 → 收件箱处理 → 多 agent 看板
- **J3 生态旅程**：MCP 安装 → 技能安装与触发 → agent → 数据源查询与注入 → 记忆 → 用量 → 时间线
- **J4 配置旅程**：Provider/multi-key → 权限预设 → 通知/webhook → IM 连接 → 远程 → 全局（i18n/a11y/导航/托盘）

### 1.3 竞品集

Claude Desktop（含 Cowork/Artifacts/Connectors）、ChatGPT Desktop（Chat+Work+Codex 三合一）、Codex Desktop（thread/Triage/Automations）、Cursor 3（Agents Window/Mission Control/Agent Review）、Manus（Computer 直播/Automations 2.0/credits）、Notion 3.0+（Custom Agents/行级审批/Insights）、Gamma（生成前对齐）。办公套件（M365 Copilot/Gemini/WPS 灵犀等）沿用 2026-09-29 报告结论，仅在 artifact 相关页面引用。

---

## 2. User Journey 走查发现

### 2.1 J1 交付旅程（welcome → chat → 附件 → 会话）

**结论：onboarding 是全产品最成熟的一段**——首启判定已修复（`Layout.tsx:125-131` 用真实 provider 状态）、provider 配置含真实连接测试（`AddProviderModal.tsx:148-160`）、无 key 时闭环兜底（环境探测 + ApiKeyBanner 只在真缺配置时出现）、auth 错误单独分类并深链设置（`MessageArea.tsx:348-375`）。核心对话循环（流式、权限弹窗四级风险徽标、RightDock 自动停靠、分支/rewind/重试）也达到竞品水准。

断点按用户伤害排序：

| # | 级别 | 断点 | 证据 |
|---|---|---|---|
| J1-1 | **P0** | **工作目录外的附件被静默丢弃**。用户从 Downloads/Desktop 附加文件（文件选择器允许任意路径），发送时后端 canonicalize 失败即 `continue`——chip 发送后凭空消失、模型毫无感知、零报错。且未配置 working_dir 时回退进程 CWD，从 Dock 启动时 CWD=`/`（代码注释自认），同一文件在不同启动方式下行为不一致 | `desktop/src/commands.rs:710-723`（注释明言「stay silently dropped, as before」）、`commands_agents.rs:196-204`、`commands_files.rs:1060` |
| J1-2 | **P1** | **「内容不会发送给模型」横幅系统性说谎**：前端 `UNPARSED_EXTENSIONS` 仍含 docx/xlsx/pptx，断言这三类不会发给模型；但后端自 Wave 1.5 起已完整解析并注入（分节 + 缓存路径）。用户被系统性误导（以为白附了） | `desktop/ui/src/components/chat/ChatInput.tsx:32,141-146` vs `desktop/src/document_parse.rs:33`、`commands.rs:900-941` |
| J1-3 | **P1** | **不支持粘贴图片**：composer 无 onPaste（拖拽/对话框可用，多模态块管线已就绪）。2026 年桌面 AI 应用表格 stakes | `ChatInput.tsx`（全文无 paste handler）、`commands.rs:805-827` |
| J1-4 | **P1** | **PDF 截断后模型无法恢复、用户不知情**：>50KiB 的 PDF 注入块只带文件名不带绝对路径、不落缓存（office 类有缓存逃生门，PDF 没有）；UI 全程无「已截断」标记 | `commands.rs:866-895` vs `document_parse.rs:715-718` |
| J1-5 | P2 | 流式中「纯附件」发送是死点击（按钮可用但直接 return，无反馈）；预算「继续一次」重发丢失原附件 | `Chat.tsx:437-441,289-292` |
| J1-6 | P2 | 附件大小/类型零预检：>10MB 图片只在发送时抛生硬英文错误；svg/bmp 出现在选择器 filter 但后端多模态不收；后端无数量上限（10 仅前端） | `ChatInput.tsx:22,289-306`、`commands.rs:730-748,812-816` |
| J1-7 | P2 | 提取/截断信息只写给模型（「Showing sections 1-8 of 23」+缓存路径），用户的 chip/FileCard 无任何标记、无「查看提取文本」入口 | `document_parse.rs:719-724` |
| J1-8 | P2 | companion 窗口应用内零入口（仅托盘）；主窗懒加载慢于 150ms 时草稿丢（注释自认 accepted） | `main.rs:790-796`、`App.tsx:118-126` |
| J1-9 | P2 | 会话导出无 JSON 入口（后端 `export_session` 支持）、无复制会话入口（后端 `duplicate_session` 存在）；无 @ 文件引用补全；/files 页无树/无预览/无编辑桥 | `commands_sessions.rs:925-1010`、`ChatInput.tsx`、`FilesPage.tsx` |

**旧发现验证**：docx/xlsx/pptx 附件黑洞**已修**（zip 炸弹守卫+分节提取+失败占位，工程质量高）；C8 引用 pill、C6 时间线导出**已接**；PDF 页范围 UI **未做**（代码注释「plugs in once an entry point exists」）；附件类型过滤仍宽松。

### 2.2 J2 自动化旅程（tasks → 模板 → 后台任务 → triage → opc）

**结论：/triage 收件箱是全产品完成度最高的面**（SQLite 权威存储 + 8 类来源 + 批量 Undo + History↔Triage↔Chat↔Drawer 四处互链）；调度引擎防呆扎实（先持久化再触发、在飞守卫、panic guard）。但**自动化的「控制面」存在系统性缺口：能建、难停、跑了看不见**。

| # | 级别 | 断点 | 证据 |
|---|---|---|---|
| J2-1 | **P0** | **例行「立即运行」是假动作**：`trigger_task_now` 只写一条 Running 占位 JSONL，从不调 `spawn_routine_run`（注释自认「execution wiring lands in Sprint 3」）；UI 却 toast 成功；占位还被回填逻辑刻意跳过。点了=什么都没发生、无错误无记录 | `desktop/src/scheduled_commands.rs:790-818`、`Tasks.tsx:254-272`、`inbox_commands.rs:314-319` |
| J2-2 | **P0** | **结果路由三通道是死 UI**：email/notification/log 编辑器宣称「backend dispatches each entry」，但 Rust `ExecutionPolicy` 无 `result_routing` 字段，serde 静默丢弃——不落盘、无读取、无分发 | `ResultRoutingEditor.tsx:1-13`、`types/index.ts:1045` vs `crates/shannon-core/src/scheduled_routines.rs:105-131` |
| J2-3 | **P1** | **例行建了就不能停**：无暂停/删除/改名/改 prompt 的任何 UI；hook 里的 `toggle/remove` 写好了但全 UI 零调用（后端命令已注册）。「每 5 分钟烧钱」的例行建错后用户没有 UI 级止损手段 | `hooks/scheduled-tasks.ts:72-98`、`main.rs:412-413` |
| J2-4 | **P1** | **policy 四字段全部是安慰剂**：`max_retries`（重试模块 `scheduled_retry.rs` 完整实现但零调用）、`timeout_secs`（执行器不设超时）、`budget_usd`（宣称超支自动禁用、无任何执行逻辑）、`worktree`（注释宣称调度器自动建删，实际执行器从不使用）——UI 全部可配、执行层全部缺席 | `ScheduleForm.tsx:390-452`、`scheduled_commands.rs:1844` |
| J2-5 | **P1** | **Simple 模式创建后台任务后无处可看**：Runs 列表只渲染 `.claude/tasks`；后台任务只进 Dev-only Pipelines tab 的执行日志——默认模式「新建后台任务」toast 成功后列表纹丝不动（created-then-invisible） | `Tasks.tsx:361,440-451` |
| J2-6 | **P1** | **OPC 看板快捷建任务不上板**：`start_background_task` 既不写 `.claude/tasks` 也不刷新，toast「已创建」但看板无变化 | `OPCKanbanBoard.tsx:64-74` |
| J2-7 | **P1** | **Pipelines（hook 触发，写 project toml）与 Routines（定时，写 task store）两套自动化体系并存零解释**；工作树 tab 与执行完全脱节（同 J2-4 worktree） | `Tasks.tsx`、`scheduled_commands.rs:1432-1464` |
| J2-8 | P2 | History 成本/token 列恒为「—」（per-run usage 从未写回）；完成率口径残留矛盾（详情页只认 `completed`，分析卡认 `completed\|done`）；中文 NL cron 必然解析失败（模式表全英文）；创建例行双重 toast；模板卡片 github 触发显示 "0s"；批次运行中无停止手段；mock 与真实 DTO 漂移；inbox-triage-hourly 模板描述仍写「IMAP 开发中」（实际 fetcher 已实现）；Triage 对 trigger 源的 rerun 禁用已过时 | `inbox_commands.rs:204-205`、`OPCTask.tsx:297` vs `scheduled_commands.rs:1676-1678`、`nl-cron.ts:96-197`、`RoutineTemplatesBrowser.tsx:163-167`、`batch_commands.rs`、`Triage.tsx:43` |

**旧发现验证**：productivity 例程模板**已兑现**（15 个模板、4 个 productivity）；任务页 CTA 层级（9-16 #17）**已修**（split button）；团队 UI 只读**基本已解决**（`agent_teams.rs:98-174` 真实注入 TeamContext；残余：OPC「Spawn Agent」只建定义不启动）。

### 2.3 J3 生态旅程（extensions → memory → usage → timeline）

**结论：本旅程暴露全产品最重的问题——「装了 → 运行时能用」最后一公里系统性断裂。** UI 层的诚实态工程（error/empty/loading 严格三分、config-only 数据源诚实挂 coming-soon）和 OAuth loopback 流（PKCE 完整、失败降级 token 粘贴）都是竞品级水准，但三条安装线全部在最后一米断掉。

| # | 级别 | 断点 | 证据 |
|---|---|---|---|
| J3-1 | **P0** | **MCP 三重断裂**：① 安装写 `~/.shannon/settings.json#mcpServers`，列表/状态读 `~/.shannon/desktop/mcp-servers.json`（split-brain，装完列表永远为空）；② 桌面聊天**从不启动 MCP 进程、从不注册 MCP 工具**（`initialize_servers` 全仓库零调用；发送路径只装默认工具+preview 工具）；③ 进程池只在手动操作时建立，重启 app 后全部显示「离线」 | `extensions/mcp_installers.rs:46-50` vs `config.rs:635-640`、`mcp.rs:27`（零调用）、`commands.rs:456-489`、`shannon-tools/src/lib.rs:590` |
| J3-2 | **P0** | **技能装完无处触发**：`register_skills_as_tools` 只在 REPL 调用；桌面聊天 slash 补全是硬编码静态表；`list_skills` 读 cwd 且 UI 零调用；native 条目安装仍只写 stub SKILL.md（目录诚实标了 "[In development]"，但 Productivity 组还置顶推广） | `skill_bridge.rs:161`、`lib/slash/commands.ts`、`Skills.tsx:169-173` |
| J3-3 | **P0** | **数据源「添加到聊天」静默丢稿**：push 的 composer-draft CustomEvent 只有 ChatInput 监听，而用户在 `/extensions/datasources` 时 Chat 未挂载——事件无人接收，toast 却报成功。（对比 companion 桥会先导航到 /chat 再 push） | `DataSourcesQuery.tsx:248`、`lib/composerBridge.ts` vs `App.tsx:115-126` |
| J3-4 | **P1** | **extensions 装的 agent 运行时永远不可见**：装到 `~/.shannon/agents/<plugin>/agent.md` 子目录；运行时 loader 只认扁平 `*.toml`/`.claude/agents/*.md` 且不递归——四种路径格式一个都不匹配。且 extensions 的 InstalledAgent 与运行时 AgentDefinition 是两套互不相通的概念 | `agent_installers.rs` vs `agent_defs.rs:36-39,300-338` |
| J3-5 | **P1** | **时间线 HTML 导出被自家沙箱挡死**：导出走系统保存对话框（用户默认选 下载/文档），但 `save_text_file` 只允许 working_dir 内写入——常见路径必然失败，只 toast「导出失败」。功能上线但默认路径不可用 | `TurnTimeline.tsx:153-169` vs `lib.rs:60-70` |
| J3-6 | **P1** | IMAP 密码/Notion token **明文**写 `~/.shannon/data-sources/*.toml`（keychain 集成已预留未接）；`restart_mcp_server` 命令+绑定齐全但全 UI 无按钮；连接失败无错误详情回传（last_error 不入库）；Pending 页错误区是硬编码空态 | `data_source_installers.rs:10-11`、`tauri-api.ts:1122`、`Pending.tsx:56-70` |
| J3-7 | P2 | rss/ical 目录文案与 coming-soon 徽章不一致；`.mcpb` 一键安装后端完备但无 UI 入口；Featured/Installed/IconRow 三处冗余；billing 三命令纯死代码（UI 零调用）；MCP 工具数徽章当在线状态用（离线/在线只有颜色差）；Agents 页安装的 agent.md 缺 system_prompt 正文 | `data_source_catalog.rs:369,393`、`tauri-api.ts:1222`、`Installed.tsx:172` |

**做得好**：Memory/Dream 子系统（proposal 审阅门控、夜间+开机补偿双轨调度、跨面板同步、apply 有 skipped 回显）是自动化与用户控制权平衡的典范；记忆溯源可跳回源会话。/usage 四维分组齐全、cache>total 口径说明已修。

### 2.4 J4 配置旅程（settings 全套 + 全局横切）

**结论：R5 后端+前端契约质量高**（multi-key 的 mask 永不泄全钥、slot-0 不变量有测试钉死、profiles 重命名跟随 active 指针、session override 原子写+损坏降级）；防呆纪律好（持久态回读、脏表单确认、加载失败显式 ErrorState）；设置页内导航已补齐（9-16 #20 关闭）。但配置旅程的两个「验证」步骤是坏的。

| # | 级别 | 断点 | 证据 |
|---|---|---|---|
| J4-1 | **P1** | **卡片级「测试连接」对已存 key 的 provider 永远假报 Invalid key**：前端硬编码 `apiKey=''`（注释错称「后端会读 credential store」），而 `test_provider_connection` 恰恰**不做** store 回退（回退只存在于 `test_provider_credentials`/`test_all_providers`）——空 key→401→「Invalid key」。用户会误删/重输正确的 key；「Test all」结果又正确，自相矛盾 | `components/settings/models-settings/ProvidersSection.tsx:60-65`、`commands_config.rs:1231-1248` vs `:1272-1284` |
| J4-2 | **P1** | **Webhook 配完无法测试**：无 test 命令无按钮（桌面通知的测试按钮还错位在 General 页）；`timeout_ms`/`include_body` 参与保存但页面无控件 | `commands_notifications.rs`、`NotificationsSettings.tsx:76-77` |
| J4-3 | **P1** | **R5 卖点文案 8 个 locale 全英文**：multi-key 管理 + profiles 重命名/删除共 67 键中 41 键在 ja/ko/de/ru/pt-BR/es/fr/zh-TW 全部为英文原文（仅 zh-CN 已翻）。更广泛地：8 个 locale 的值与英文逐字相同占 79-80%——「10 语言支持」对绝大多数语言是骨架级 | `desktop/ui/src/i18n/locales/*.json`（复核：41/67 精确命中） |
| J4-4 | P2 | 额度类错误（402/quota）落 unknown 显示原始英文报文；`CONFIG_UPDATED` 不刷模型目录（CatalogContext 无监听，外部改配置后 chat 模型列表过期）；权限自定义档案写 **CWD 相对路径**（`PathBuf::from(".shannon")`，macOS 启动 CWD=/ 时可能写丢）；内置权限档描述直接渲染引擎英文 | `commands_config.rs:1196-1219`、`AppContext.tsx:991-996`、`automation_commands.rs:342-346`、`PermissionsSettings.tsx:622` |
| J4-5 | P2 | 19 处 settings toast 硬编码英文；托盘菜单/主题名/IM 平台名硬编码；命令面板设置 6/8 子页不可达；Remotes 已知限制（PTY/worktree local-only）零告知；IM 消息去向无引导文案（配好 Slack 后会话其实会出现在侧栏，但页面不说）；demo 模式三入口不可发现；failover/key 轮换通知只在聊天流内、无入口告知用户去哪看 | `RemotesSettings.tsx` 等、`CommandPalette.tsx:71-73`、`MessageArea.tsx:87-107` |

**做得好**：gateway 启停状态驱动（9-16 #15 验证通过）、needsRestart 脏标+事件回拉闭环、连接器 keyring 只探测存在性永不回显明文、multi-key「最后一把钥匙禁删」类防呆文案。

### 2.5 走查横向统计

- P0 ×4：J1-1 附件静默丢弃、J2-1 立即运行假动作、J2-2 结果路由死 UI、J3-1/2/3 扩展三断路（按 3 计则 P0 ×6）
- P1 ×13、P2 ×25+
- **共同根因**（详见 §4）：桌面聊天工具装配点与扩展体系从未接通；「UI 先画、执行后补」欠账；静默失败反模式

---

## 3. 逐页竞品对比

> 对比维度：功能覆盖 / 交互模式 / 信息架构。竞品事实来源以官方文档为主（来源清单见 §6）。办公套件竞品（M365 Copilot/Gemini/WPS 灵犀/豆包）已在 2026-09-29 报告覆盖，此处只在相关页面引用其结论。

### 3.1 Onboarding（/welcome）

| | Shannon | 竞品做法 |
|---|---|---|
| 流程 | 两步：选任务→配 provider（真实连接测试）→发首条消息；无 key 有环境探测兜底 | **ChatGPT**：4 步——安装→登录→**「Choose where to work」（选工作区做进首启）**→直接发消息（Chat/Work 选择）；Work 首次进入给 3 个 starter 用例+可直发的示例 prompt。**Claude**：账号即订阅、零配置。**Manus Desktop**：My Computer tab→Add Folder→**OS 原生授权弹窗**。**Notion**：给 agent 起名/配饰+明示「Let it learn」——把记忆机制做成 onboarding 卖点 |
| 异同 | Shannon 是竞品中唯一需要「配 provider/key」的（BYOK 定位使然，合理）；连接测试真实可用是加分项 | 差距：① 首启没有「选工作区」步骤（working_dir 是后端隐式概念，用户不知其存在——这正是 J1-1 附件 P0 的认知根源）；② 任务卡「继续」实为滚动锚点，与两步 stepper 隐喻错位 |
| 建议 | 1) 把「选择工作目录」提为首启显式步骤（同步解决附件路径问题的用户认知）；2) Work 模式借鉴 ChatGPT 的「starter 用例+可直发示例 prompt」；3) 若做本地文件夹授权，借 Manus 的 OS 原生弹窗建立信任 |

### 3.2 核心对话（/chat composer + 流式 + 权限）

| | Shannon | 竞品做法 |
|---|---|---|
| 输入聚合 | 斜杠组合框（ARIA 完整）、模型选择带会话级 override chip、拖拽/对话框附件 | **ChatGPT**：composer 上方 Chat/Work toggle、**下方权限三档常驻**（Ask/Auto/Full）、@ 插件提及、本地/云开关。**Claude**：框内 Output 选择器（Docs/Slides/Design 模板）+连接器图标+权限模式选择器（**删除文件永远弹窗**）。**Cursor**：steering 三级（立即/排队本轮后/顺序化），统一在「下个 tool call 边界」生效 |
| 过程可视化 | 思考折叠块、工具卡、进度百分比+时长 pill、aria 只报状态迁移——细节好 | **ChatGPT Work**：任务运行期**侧栏常驻 plan/sources/files/summary 四要素**。**Codex**：diff 右侧常驻面板（第三方评测称「没见过别家做得这么干净」）。**Manus**：直播窗口+分享含逐帧 replay |
| 异同 | 单点交互（流式、工具卡、权限弹窗四级风险徽标+触发原因）不落后；**缺三样**：权限档不常驻 composer（要进设置）、无 @ 文件/工具引用、无 steering 分级（Stop/Esc 是硬中断） | |
| 建议 | 1) 权限三档收进 composer 就近切换（对标 ChatGPT/Claude，设置页只负责「解锁」）；2) @ 文件引用补全（竞品标配）；3) 插话分级「立即/排队」两档起步（照搬 Cursor 键位语义）；4) 长任务考虑右侧常驻「plan/来源/产物」摘要条 |

### 3.3 附件与文件（chat 附件 + /files）

| | Shannon | 竞品做法 |
|---|---|---|
| 能力 | 图片/PDF/office 解析注入（Wave 1.5 后）、pdf.js 预览 200 页、FileCard Review 进 Diff、/files 索引收藏 | **ChatGPT**：50+ 格式单文件 512MB、**File Library 自动归集所有上传+生成物**、预览内**区域级标注→定向回修**（选中 nav bar→改字体）、任务完成自动打开产物。**Claude**：Skills 产物消息内下载 chip、Artifacts Export 四格式。**Codex**：交付=worktree+diff 审阅，**discard 是一等操作** |
| 异同 | 解析深度（zip 守卫、分节、诚实失败占位）达到第一梯队；差距在**「文件作为资产」的产品化**：无跨会话文件库（/files 只索引附件+产物，且缺失文件只灰化）、无预览内标注回修、大小上限（10MB/图、PDF 100MB、office 无明确上限）远小于竞品口径、PDF 截断无恢复路径（J1-4） | |
| 建议 | 1) PDF 对齐 office 的缓存逃生门（补绝对路径+缓存副本）；2) /files 演进为「文件库」：时间线视图+「在编辑器打开」桥；3) chip 加解析摘要徽标（「已提取 23 段/只读前 50KiB」）——透明度本身可以是卖点（竞品都不标注截断）；4) 中期考虑预览内圈选→回修（ChatGPT 模式，与 RightDock 预览天然契合） |

### 3.4 会话管理与时间线（侧栏 + /timeline）

| | Shannon | 竞品做法 |
|---|---|---|
| 能力 | 三视图+置顶+拖拽、全文搜索、分支（branch_session）、消息级 rewind（无 checkpoint 则不渲染按钮——防假按钮）、时间线累计曲线+工具瀑布+HTML 导出 | **Codex**：thread=「上下文+隔离工作区+review 流」三件套心智；后台产物带 **replayable logs**；cloud↔local 迁移。**Cursor**：checkpoint 时间线点击预览+一键恢复（**只回滚文件不动消息**）。**Manus**：分享链接默认含 replay 回放。**Claude**：编辑旧消息=产生新 chat 版本（分支式时间线） |
| 异同 | 分支/rewind 能力完整甚至超出部分竞品；时间线可视化（累计 token/cost+瀑布）是差异化亮点；差距：导出被沙箱挡死（J3-5）、无 replay 语义（时间线是读不仅要看）、分支语义未在侧栏可视化 | |
| 建议 | 1) 修 J3-5 导出白名单（低危高感知）；2) 时间线导出/分享时考虑附带「工具瀑布」截图语义（对标 Manus replay 的过程即交付物）；3) rewind 已有的「只动文件」语义在 UI 文案里显式化（Cursor 文案是个好范例） |

### 3.5 任务/自动化（/tasks）

| | Shannon | 竞品做法 |
|---|---|---|
| 创建 | 表单（4 触发器+NL→cron 预览 350ms 防抖）+ 15 模板画廊 | **Manus**（最完整）：三类型（Schedule/Trigger/Advanced）；**Advanced=NL 描述→激活前强制确认步**（「review the generated timing, conditions, actions, and accounts」，输入框字段名就叫「Manus will」）；**Test run 一等公民**（官方建议负例测试）；多次失败自动 pause+自愈路径。**ChatGPT**：对话式创建+消息流内确认卡；任务卡 ⋮：Edit/Pause/Delete/**Share（快照链接一键复制为自己的）**；失败任务标 paused/needs action 并通知。**Codex**：对话式起草、confirm 前才露最小表单（只管 Environment/时间）；落盘 `.codex/automations/*.toml` 可入 git，管理 UI 只读 |
| 异同 | Shannon 的 cron 预览/日历/DAG 视图有独到之处（竞品公开形态多无 DAG）；**控制面差距是断代级的**：竞品任务卡都有 Pause/Delete，Shannon 一个都没有（J2-3）；竞品都有「对话式创建+确认」，Shannon 是纯表单；竞品都有失败可见性（needs action），Shannon 的失败只进 run record | |
| 建议 | 1) **先补生命周期**：任务卡加 Pause/Delete/Edit（hook 已写好，纯 UI 接线）——这是自动化产品及格线；2) NL 创建加「结构化预览确认步」（cron 预览已有，扩展到 prompt/触发器/通知全量预览+激活确认，对齐 Manus 黄金范式）；3) 「立即运行」接真执行+Test run 语义（Manus 的正例/负例建议直接可抄进文案）；4) 中文 NL cron：接 LLM 解析或先在 placeholder 诚实声明仅英文；5) 任务分享（快照链接）是与 IM 连接器协同的差异化机会 |

### 3.6 收件箱（/triage）

| | Shannon | 竞品做法 |
|---|---|---|
| 能力 | 8 类来源、批量已读/归档+Undo+部分失败如实上报、rerun 真执行、与 History/Chat 双向互链 | **Codex Triage Inbox**：任务完成统一入队，处置三键 **accept/modify/reject**+丢弃 worktree；用户社区在请求「按 thread 未读组织」（说明 Codex 也是全局队列）。**ChatGPT**：无收件箱，任务卡状态+推送。**Manus**：Updates 流（按日分组+状态+一句话摘要+「需要关注的原因」前置）+日历视图 |
| 异同 | **Shannon 的 triage 完成度高于所有竞品的公开形态**（批量+Undo+rerun+互链是独有组合）；差距：条目卡无「状态+一句话摘要+原因」三件套（要点开看）、失败项无「needs action」级文案、与 IM 收件的概念边界未说明（J4-5） | |
| 建议 | 1) 条目卡摘要前置（多数竞品用户必须点开，Shannon 有数据条件做得更好）；2) 保持现有互链优势，考虑把「继续会话」升级为带上下文引用的续聊；3) 文档/空态里显式区分「自动化收件箱（triage）」与「IM 消息（connections 会话）」两个概念 |

### 3.7 多 agent 看板（/opc）

| | Shannon | 竞品做法 |
|---|---|---|
| 能力 | 看板 DnD+乐观更新、agent 负载/消息面板、任务详情含权限审批人审（ownershipMismatch 防误批）、Spawn Agent（建定义） | **Cursor Agents Window**：跨 repo/环境混排、新 diffs view（审改动+commit+管 PR）、subscriptions（watch Slack/schedule/follow PR）、8 agent 并行各占 worktree+统一 change view。**Notion**：单 agent Chat/Activity/Settings 三 tab；**All chats 跨 agent 运行表**（status/triggers/credits/model/次数）+Charts+CSV 导出。**Manus**：Wide Research 数百并行网格 |
| 异同 | 权限人审环节是差异化亮点（竞品公开形态未见）；**差距**：快捷建任务不上板（J2-6）、Spawn 只建定义不启动（名实不符）、卡片信息层级无 diff 预览/分支徽标、无跨任务运行表 | |
| 建议 | 1) 快捷建任务改写 `.claude/tasks`+刷新（修 J2-6）；2) 借 Notion「All chats」做跨 agent 运行表（status/成本/模型列——数据在 usage ledger 已有）；3) Spawn 改名「注册 Agent 模板」或接真启动；4) 卡片加分支/diff 摘要徽标（对齐 Cursor diffs view） |

### 3.8 扩展生态（/extensions/*）

| | Shannon | 竞品做法 |
|---|---|---|
| 能力 | MCP 三路安装（registry 搜索/JSON 粘贴/手动）、OAuth loopback、技能联邦目录+安全扫描、数据源 6 真 fetcher、Plugins 四路安装+enable/update | **Claude**：**会话内 grid 图标直达连接器目录**（不离开上下文）、目录卡片带来源/已装态/**工具级 toggle**、.mcpb 一键装、企业 allowlist。**Codex**：技能目录带「OpenAI Curated」来源标记、已安装状态、可 check 进 repo。**ChatGPT**：Plugins 库+@提及引用 |
| 异同 | 安装路径的丰富度（JSON 粘贴兼容 Cursor/Claude 格式、四路 plugin 安装）不落后甚至更全；OAuth 流质量高；**决定性差距不在安装、在消费**（J3-1/2/4 三断路）：竞品装完即可用，Shannon 装完=不存在。这是「扩展市场」页面存在的意义本身 | |
| 建议 | 1) **修三断路是本页最高优先级**（统一存储→启动 seed 进程池→聊天装配点挂接）；2) 连接器卡片加工具级开关+最近错误（对齐 Claude）；3) `.mcpb` 入口补上（后端已备）；4) skills 目录的 "[In development]" 条目要么实现要么移出置顶推广位 |

### 3.9 记忆（/memory）

| | Shannon | 竞品做法 |
|---|---|---|
| 能力 | 列表/搜索/过滤/编辑/删除、项目作用域、AutoDream 夜间提取+人工审阅门控、溯源跳源会话、上下文注入有 token 预算+ContextBreakdownCard 色块 | **ChatGPT**：Memory 与 Reference chat history **双开关**、单条管理/forgot、临时对话旁路、合适时机主动请求「记住某偏好」。**Claude**：记忆引用回答附「From your previous conversations」**可回跳链接**。**Notion**：记忆=可编辑页面。**Manus**：知识库变更**只对新任务生效**（边界写进 UI） |
| 异同 | 审阅门控+溯源是强项（ChatGPT 都没有人工审阅提取）；**差距**：对话流里没有任何「本条回复用了哪些记忆」的可见信号（竞品至少有回跳链接）、无「临时不带记忆」旁路、记忆生效边界（何时注入/何时不注入）用户不可感知 | |
| 建议 | 1) 回答引用记忆时加可回跳引用（对齐 Claude「From your previous conversations」——溯源链路后端已有，缺 UI 呈现）；2) 会话级「不带记忆」开关（临时对话模式）；3) 记忆注入边界写进 Memory 页文案（对齐 Manus 的生效边界声明） |

### 3.10 用量（/usage）

| | Shannon | 竞品做法 |
|---|---|---|
| 能力 | 总览+审计双模式、7/30/90 天、model/provider/day/session 四维分组、cache 口径说明 | **Claude**：侧栏常驻 % 条+hover 重置时间；触顶提示降档链（Opus→Sonnet→Haiku，可自动降档）。**ChatGPT**：触顶不截断进行中任务；三级处置：降档→credits 续命→API key 兜底。**Cursor**：双池（订阅/按量）+请求级下钻+重置日期同屏。**Manus**：**任务前 credits 预估**（参考同类历史任务）。**Notion**：**80%/100% 双阈值告警**+agent 级 credit 上限+触顶自动暂停并通知创建者 |
| 异同 | 分组维度齐（session 级审计超出多数竞品）；**差距是「治理」整层缺失**：无常驻用量条、无阈值告警、无预算联动（budget_usd 字段还是安慰剂 J2-4）、无任务前预估、触顶无降级链 | |
| 建议 | 1) 侧栏常驻轻量用量指示（Claude 模式，hover 给重置/明细）；2) 80%/100% 双阈值告警+例行预算触顶自动暂停（把 budget_usd 从安慰剂变成真功能——Notion 模式，工程量集中在执行器一处）；3) 任务前成本预估（历史 run 聚合即可给区间估计，Manus 模式）；4) 双池显示（订阅/API key 分开）随 BYOK 定位天然成立 |

### 3.11 设置（/settings/*）

| | Shannon | 竞品做法 |
|---|---|---|
| 能力 | 8 段 rail 内部导航、provider/multi-key/tier、权限 3 内置+custom、13 主题、10 locale、webhook 模板、IM 8 平台、remotes | **Cursor**：Models（BYOK 表单化→「保存后出现在模型选择器」）/Rules & Memories 三源合一/Agents（Agent Review Quick/Deep——**审批档位=成本档位**）。**ChatGPT**：**解锁式权限**（设置只把模式加入菜单，不改运行态）；通知页内嵌任务管理入口。**Claude**：Personal/Org 双层；key 管理独立在 Console（app 内无 provider 页——Shannon 的 BYOK 完整 UI 反而是差异化空间） |
| 异同 | 设置信息架构不落后（rail 导航已补齐）；BYOK+multi-key+failover 的深度超出订阅制竞品（它们没有这个问题域）；**差距**：两个验证步骤是坏的（J4-1 测试连接假报、J4-2 webhook 无测试）、设置与运行态的联动有暗礁（模型目录不随 CONFIG_UPDATED 刷新）、多语言名不副实（J4-3） | |
| 建议 | 1) 修 J4-1/J4-2（测试是配置旅程的信任锚点）；2) 借 Cursor「保存后出现在模型选择器」的即时反馈模式，修 CONFIG_UPDATED→CatalogContext 联动；3) webhook 预设补 Discord；4) 权限描述 i18n 化（引擎描述经一层映射表） |

### 3.12 全局横切（导航/托盘/i18n/a11y/伴随窗口）

- **侧栏 IA**（ZCode 式扁平 4+1）与竞品趋势一致（ChatGPT 侧栏也是 chat/tasks/library 扁平）；命令面板对 settings 覆盖 2/8 需补全。
- **i18n**：键集 100% 对齐但 8 locale 值 80% 为英文——对比竞品（ChatGPT/Claude 多语言由官方运营），「骨架级多语言」要么补翻译（优先 zh-TW/ja/ko——R5 卖点文案先行），要么诚实收缩宣传口径为「en/zh-CN 完整支持，其余 beta」。
- **a11y**：大面积良好（role/aria-invalid/sr-only/radiogroup/focus-visible/Esc 仲裁），缺口集中在 ModelsSettings/ThemeSettings 的按钮组缺 `aria-pressed`——小修。
- **伴随窗口**：ChatGPT 的 Alt+Space 伴随窗口是其桌面端核心心智；Shannon 的 companion 功能在（草稿桥设计干净）但应用内零入口+150ms 丢稿竞速——补入口+改拉取式即可激活这个沉睡功能。

---

## 4. 横切综合发现（结构性根因）

1. **「装了→能用」最后一公里系统性断裂**（MCP/技能/agent 三线全中招）。根因：桌面聊天的工具装配点（`commands.rs:456` 只装默认工具）与扩展体系的存储/目录约定（settings.json vs mcp-servers.json；`~/.shannon/skills` 只有 REPL 读；agent 子目录格式不匹配）从未接通。**扩展市场是首页默认 tab，用户装的第一件事大概率落空**——这直接架空 J3 整个旅程与 §3.8 的全部竞品对比。
2. **静默失败是最高频反模式**（7 处：J1-1 附件丢弃、J3-3 丢稿、J2-1 假运行、J2-2 serde 丢弃、J3-5 导出拒绝、J4-1 假报、J2-5/J2-6 created-then-invisible）。共同形态：**操作返回成功或无反馈，实际什么都没发生**。修法有公共模板：所有「跳过/拒绝/未执行」路径必须产生用户可见信号（toast/横幅/条目状态三选一）。
3. **三套任务存储、两套自动化体系、两套 agent 概念**：scheduled task store（SQLite/JSONL）/ background task 内存表 / `.claude/tasks` 文件并存；triggered routine（project toml）与 scheduled routine（task store）并存；InstalledAgent 与 AgentDefinition 并存。这是 J2-5/J2-6/J3-4 三个 P1 的共同上游，也是用户「任务建了在哪看」困惑的根源。
4. **UI 提前承诺执行层欠账**（安慰剂清单）：policy 四字段、ResultRouting 三通道、restart 按钮、`.mcpb`、billing 三命令、JSON 导出、companion 入口。共同建议：**一个迭代内二选一——接线或下线**，长期挂着比不做更伤（用户配置了不生效的东西=说谎的表单）。
5. **i18n「键满分、值不及格」**：工程上做到了键集零缺失，但 8 locale 80% 值为英文。R5 卖点界面大面积英文是本次发布最可见的破绽。

---

## 5. 综合改进方案（供审核）

> 排序原则：P0=信任修复（静默失败/断路，多数是接线级工作量）；P1=承诺兑现+旅程补全；P2=对标差异化。规模估算：S=≤1 天、M=2-5 天、L=1-2 周。

### 第一批 P0 —— 信任修复（建议一个 hotfix 窗口，合计约 2-3 周）

| # | 事项 | 修法 | 规模 |
|---|---|---|---|
| P0-1 | MCP 三重断裂（J3-1） | 统一到单一存储（建议 `~/.shannon/desktop/mcp-servers.json` 或 settings.json 二选一）；app 启动时按配置 seed 进程池；聊天发送路径挂接同一池注册 MCP 工具 | L |
| P0-2 | 技能运行时接入（J3-2） | `AppState` 启动时调 `register_skills_as_tools`（home 路径）；slash 补全并入已装技能；native 目录条目实现或移出置顶 | M |
| P0-3 | 附件路径静默丢弃（J1-1） | 保持安全边界不变，拒绝时返回带路径的错误→前端 toast 标注哪个文件没发出去；attach 时预检就地告警；未设 working_dir 时明确回退语义（提示用户当前可附范围） | M |
| P0-4 | 例行「立即运行」假动作（J2-1） | `trigger_task_now` 改调 `spawn_routine_run`（复用 loopback trigger 逻辑）；UI 区分「已触发」与「已完成」 | M |
| P0-5 | ResultRoutingEditor 死 UI（J2-2） | 短期下线编辑器（避免说谎表单）；中期补 ExecutionPolicy 字段+分发 | S（下线）/L（接线） |
| P0-6 | 数据源 Add-to-chat 丢稿（J3-3） | 仿 CompanionPromptBridge：先 `navigate('/chat')` 再延迟 push（一次性队列） | S |
| P0-7 | 测试连接假报（J4-1） | `testProviderConnection` 改传 `conn.id` 走 `resolve_probe_key`（或前端改调 `testProviderCredentials`） | S |
| P0-8 | 时间线导出沙箱拒绝（J3-5） | 导出命令走 `$HOME/$TEMP` 白名单 scope 或专用 export 命令 | S |

### 第二批 P1 —— 承诺兑现 + 旅程补全（一个迭代）

| # | 事项 | 说明 |
|---|---|---|
| P1-1 | 例行生命周期 UI | 任务卡 Pause/Delete/Edit（hook 已写好，纯接线）——自动化产品及格线 |
| P1-2 | policy 字段接线或标注 | `max_retries` 接 `scheduled_retry.rs`（模块已写完）；`budget_usd` 接执行器（联动 P2-6）；`timeout_secs` 执行器设超时；worktree 接 spawn cwd——标注「规划中」是下策但可接受 |
| P1-3 | created-then-invisible | Runs tab 增加后台任务分区（Simple 模式可见）；OPC 快捷建任务写 `.claude/tasks`+刷新 |
| P1-4 | PDF 截断恢复+全类型提取可见性 | PDF 注入块补绝对路径+缓存副本（对齐 office）；chip/FileCard 加「已提取 N 段/截断」徽标（透明度做成卖点） |
| P1-5 | 附件横幅修正 | `UNPARSED_EXTENSIONS` 收窄为真实集合（doc/xls/ppt/odt/rtf），已解析类型改为展示提取信息 |
| P1-6 | 粘贴图片 | composer onPaste→临时文件→现有路径管线 |
| P1-7 | Webhook 测试发送 | `test_webhook` 命令+Notifications 页按钮；桌面通知测试按钮归位 Notifications 页 |
| P1-8 | R5 i18n 翻译补齐 | 41 个 R5 键 8 locale 机翻+人工抽查；顺带修 19 处硬编码 toast/托盘/主题名 |
| P1-9 | extensions agent 格式统一 | installer 改写扁平 TOML 或 loader 兼容子目录 agent.md（与 P2-9 IA 收敛联动） |
| P1-10 | 权限档案路径锚定 | `local_profiles_dir()` 显式锚定工作区/主目录（一行修） |
| P1-11 | companion 激活 | Header/命令面板加入口；草稿改 Chat 挂载后拉取（一次性队列） |
| P1-12 | CONFIG_UPDATED→模型目录联动 | CatalogContext 监听刷新（借 Cursor「保存后即出现在选择器」的即时性） |

### 第三批 P2 —— 对标差异化（按 ROI 排序，建议季度内挑选）

| # | 事项 | 来源竞品 | 落点 |
|---|---|---|---|
| P2-1 | 用量治理：侧栏常驻 % 条+80%/100% 阈值告警+budget 触顶自动暂停 | Claude/Notion | 侧栏+执行器 |
| P2-2 | NL 自动化「结构化预览→激活确认」步 | Manus | ScheduleForm |
| P2-3 | 任务运行期侧栏「plan/来源/产物/摘要」四要素 | ChatGPT Work | /chat RightDock |
| P2-4 | 权限三档常驻 composer（设置页只解锁） | ChatGPT/Claude | composer |
| P2-5 | 记忆引用可回跳（回答附来源链接）+会话级「不带记忆」旁路 | Claude/ChatGPT | MessageBubble+/memory |
| P2-6 | 任务前成本预估（历史 run 聚合区间） | Manus | 后台任务确认 |
| P2-7 | Triage 条目卡「状态+一句话摘要+原因」前置+失败「needs action」文案 | Manus/ChatGPT | /triage |
| P2-8 | 跨 agent 运行表（status/成本/模型/次数） | Notion All chats | /opc |
| P2-9 | IA 收敛：任务存储三合一、自动化两体系合并入口+解释文案、agent 概念统一 | 结构性 | 需产品拍板 |
| P2-10 | 中文 NL cron（LLM 解析）+ @ 文件引用 + steering 分级插话 | —/竞品标配/Cursor | composer |

### 不建议做（明确排除）

- **预览内圈选→局部回修**（ChatGPT 标注模式）与 **Gamma 式逐卡生成**：上一轮办公报告已裁决不做逐页重生成（C9），标注回修依赖的预览-回写链路成本高、当前 artifact 面板已有 Review 入口，等 P1-4 落地后再评估。
- **消息渠道 inbox 化**（IM 收件箱独立页面）：connections 会话入侧栏的机制已成立，缺的只是文案（J4-5），不需要新页面。
- **全 locale 100% 翻译冲刺**：8 locale 80% 残留的全量翻译 ROI 低，优先 R5 卖点界面 + zh-TW/ja/ko 三个市场，其余诚实标注 beta。

### 排期建议

- **Hotfix 窗口（~2 周）**：P0 全部 8 项。P0-7/P0-6/P0-8/P0-5(下线) 四个 S 级可以先发。
- **迭代 N（~3-4 周）**：P1 全部；其中 P1-2 的 budget 接线可与 P2-1 合并做。
- **季度**：P2 按 ROI 挑 4-5 项；P2-9 IA 收敛需要先出决策文档（三存储合一牵扯 migration）。

---

## 6. 附录

### 6.1 竞品来源（本轮新增）

Claude：support.claude.com（Cowork/安装/设置/Artifacts/Projects/记忆检索/Skills/MCP connectors）；ChatGPT：learn.chatgpt.com（app/Work/permission-modes/automations/pricing/artifacts-viewer/codex memories）+ help.openai.com（Scheduled tasks/release notes）+ openai.com（Codex app 官宣）；Cursor：cursor.com（agents-window/agent-review/changelog/usage-limits/api-keys）；Manus：manus.im（docs automations/desktop/projects/slides/wide-research + blog schedules/plan-mode/branch/2.0）；Notion：notion.com/help（custom-agents/personal agent/3.0）；Gamma：gamma.app + 第三方评测。完整 URL 清单见调研代理原始产出（findings-journey.md 同级归档）。

### 6.2 问题编号索引（供逐条跟踪）

J1-1..J1-9（§2.1）、J2-1..J2-8（§2.2）、J3-1..J3-7（§2.3）、J4-1..J4-5（§2.4）；P0-1..P0-8、P1-1..P1-12、P2-1..P2-10（§5）。全部断言含 file:line 证据，最重 8 处已复核。
