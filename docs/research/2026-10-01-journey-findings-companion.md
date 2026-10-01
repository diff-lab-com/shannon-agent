# Findings: 全页面 Journey 走查 + 竞品对比 (2026-10-01)

> 6 代理并行产出（J1-J4 代码走查 + WC1-WC2 竞品 UI）。本文件为浓缩索引，完整细节见
> docs/research/2026-10-01-full-journey-competitive-review.md。基线 dev @ e8cbc2f5/#e9944896。

## J1 welcome/chat/files（Top 问题）
- **P0 附件工作目录外静默丢弃**：commands.rs:716-720 canonicalize 失败 `continue`；CWD 回退 commands_agents.rs:196-204；Dock 启动 CWD=/（commands_files.rs:1060 自认）
- **P1「未解析」横幅说谎**：ChatInput.tsx:32 UNPARSED_EXTENSIONS 仍含 docx/xlsx/pptx，后端 document_parse.rs:33 已解析注入（16KiB+缓存路径）
- **P1 无粘贴图片**：ChatInput.tsx 无 onPaste（多模态块 commands.rs:805-827 已就绪）
- **P1 PDF 截断不可恢复不可见**：commands.rs:866-895 注入块无绝对路径无缓存（office 有）；UI 零标记
- P2：流式中纯附件死点击 Chat.tsx:437-441；预算续发丢附件 Chat.tsx:289-292；大小零预检；提取信息用户不可见；companion 仅托盘入口 main.rs:790-796；companion 草稿 150ms 竞速 App.tsx:118-126；导出无 JSON 入口（后端有 export_session json）；/files 无树/无编辑桥；无 @ 引用
- 旧发现验证：附件黑洞已修（后端）、C8 引用 pill 已接、C6 时间线导出已接、页范围 UI 未做、类型过滤仍宽松
- 亮点：附件管线工程（zip 守卫/失败占位/spawn_blocking）、流式 a11y、auth 错误深链

## J2 tasks/triage/opc（Top 问题）
- **P0 例行「立即运行」假动作**：scheduled_commands.rs:794-816 trigger_task_now 只写 Running 占位不 spawn_routine_run；UI Tasks.tsx:254-272 toast 成功；占位被回填刻意跳过 inbox_commands.rs:314-319
- **P0 ResultRoutingEditor 死 UI**：email/notification/log 三通道 Rust ExecutionPolicy（crates/shannon-core/src/scheduled_routines.rs:105-131）无字段，serde 静默丢弃
- P1 例行无暂停/删除/改名 UI（hooks/scheduled-tasks.ts:72-98 toggle/remove 零调用；后端命令已注册）
- P1 policy 四字段安慰剂：max_retries（scheduled_retry.rs 零调用）/timeout_secs/budget_usd/worktree 全存而不执行；worktree 注释与实现不符 scheduled_commands.rs:1844
- P1 Simple 模式 created-then-invisible：后台任务只进 Dev-only Pipelines tab（Tasks.tsx:361,440-451）
- P1 OPC 快捷建任务不上板：OPCKanbanBoard.tsx:64-74 不写 .claude/tasks 不刷新
- P1 Pipelines(Hook/toml) vs Routines(scheduled/store) 两套体系无解释
- P2：cost/token 恒空 inbox_commands.rs:204-205；完成率口径残留 OPCTask.tsx:297 vs scheduled_commands.rs:1676-1678；中文 NL cron 不解析 nl-cron.ts:96-197；双重 toast；模板 github "0s"；trigger rerun 过时禁用 Triage.tsx:43；mock DTO 漂移 handlers.ts:1149-1163；inbox-triage-hourly 文案过期；批次无 cancel batch_commands.rs；批次无停止
- 旧发现验证：B5 productivity 模板已兑现（15 模板/4 productivity）；#17 CTA 已修（split button）；团队 UI 已接线（残余 OPC Spawn 只建定义）
- 亮点：triage 完成度最高（8 源/批量 Undo/四处互链）、调度器防呆（先持久化再触发/在飞守卫/panic guard）

## J3 extensions/memory/usage/timeline（Top 问题）
- **P0 MCP 三重断裂**：①装写 settings.json extensions_commands.rs:58 vs 列表读 mcp-servers.json commands_mcp.rs:181（split-brain）；②桌面聊天从不启动 MCP/注册工具 commands.rs:456-489、mcp.rs:27 initialize_servers 零调用、shannon-tools/src/lib.rs:590 一次性空池；③重启 app 全离线
- **P0 技能装完不可触发**：register_skills_as_tools 仅 REPL（skill_bridge.rs:161）；桌面 slash 硬编码 lib/slash/commands.ts；list_skills 读 cwd 且 UI 零调用；native 条目仍装 stub（Skills.tsx:169-173）
- **P0 DataSources Add-to-chat 静默丢稿**：DataSourcesQuery.tsx:244-248 dispatch CustomEvent 时 Chat 未挂载→无人接收，toast 报成功（对比 CompanionPromptBridge 先导航 App.tsx:115-126）
- P1 extensions agent 运行时不可见：装到 ~/.shannon/agents/<plugin>/agent.md，loader agent_defs.rs:36-39,300-338 只认扁平 toml/claude md 不递归
- P1 时间线导出被沙箱挡死：TurnTimeline.tsx:157 系统保存对话框 vs save_text_file working_dir 白名单 lib.rs:65（选 Downloads 必失败）
- P1 凭据明文 TOML data_source_installers.rs:10-11；restart_mcp_server 无 UI 入口（命令在 tauri-api.ts:1122）；Pending 错误区硬编码空态 Pending.tsx:56-70
- P2：rss/ical 文案 vs 徽章不一致 data_source_catalog.rs:369,393；.mcpb 后端齐无入口 tauri-api.ts:1222；Featured/Installed 三处冗余；billing 三命令死代码；Agents 装 body 缺 system_prompt
- 亮点：诚实态工程（error/empty/loading 三分）、OAuth loopback PKCE 完整 extensions_commands.rs:204-336、Memory/Dream 审阅门控+双轨调度
- 根因总结：桌面聊天工具装配点（commands.rs:456）与扩展体系存储/目录约定从未接通——「装了→运行时能用」最后一公里系统性缺失

## J4 settings/全局（Top 问题）
- **P1 卡片 Test connection 假报 Invalid key**：ProvidersSection.tsx:56-65 传 apiKey=''（注释错称后端读 store）；test_provider_connection 无 store 回退 commands_config.rs:1231-1248（回退在 test_provider_credentials:1272-1284）；「Test all」反而正确（自相矛盾）
- **P1 webhook 无测试发送**：commands_notifications.rs 无 test 命令；桌面通知测试错位在 General 页 GeneralSettings.tsx:76-89
- **P1 R5 文案 8 locale 未翻**：41 个 keys.*/profiles.* 键 ja/ko/de/ru/pt-BR/es/fr/zh-TW 全英文；整体 8 locale 值与英文逐字相同占 79-80%（仅 zh-CN ~98%）
- P2：额度类错误无分类（402→unknown 原始英文）；CONFIG_UPDATED 不刷模型目录（CatalogContext 无监听）；profiles 路径 CWD 相对 automation_commands.rs:342-346；内置权限档描述未 i18n；timeout_ms/include_body 无控件但参与保存；缺 Discord webhook 预设；19 处 settings toast 硬编码英文；Remotes 限制零告知；命令面板设置 6/8 子页不可达 CommandPalette.tsx:71-73；托盘/主题名/PLATFORM_LABEL 硬编码；IM 消息去向无引导；failover 通知只在聊天流无入口说明
- 旧发现验证：设置内导航已修（#20 关闭）；gateway 启停状态驱动 #15 验证通过；权限模式实为 3 内置+custom（非 8）
- 亮点：R5 契约质量（mask 永不泄钥/slot-0 不变量）、持久态回读防呆纪律、设置 rail 优雅降级

## WC1 竞品交互（Claude/ChatGPT/Codex）——最值得借鉴
1. 首启第 3 步选工作区（ChatGPT「Choose where to work」）+ 第 4 步直接发消息
2. 权限三档常驻 composer（ChatGPT Ask/Auto/Full；Claude Manually/Auto/Skip，删除永远弹窗）
3. 任务运行期侧栏固定 plan/sources/files/summary 四要素（ChatGPT Work）
4. diff 右侧常驻面板（Codex）；交付=worktree+diff 审阅，discard 一等操作（Codex）
5. 预览内区域级标注→定向回修（ChatGPT）；Artifacts Edit with Claude/Try fixing（Claude）
6. 自动化对话式创建+确认环节才露最小表单（Codex）；任务卡 ⋮ Pause/Edit/Delete/Share+下次运行时间（ChatGPT）；任务分享=快照链接一键复制（ChatGPT）
7. 连接器：会话内 grid 图标直达目录（Claude）；卡片带来源/已装态/工具级开关（Claude/Codex）
8. 记忆：引用可回跳源对话（Claude）；记忆与历史引用双开关+临时对话旁路（ChatGPT）
9. 用量：常驻 % 条+hover 重置时间（Claude）；触顶不截断+降档→credits→API key 兜底三级链（ChatGPT）
10. Triage：完成即入队+accept/modify/reject 三键+按未读组织（Codex）

## WC2 竞品交互（Cursor/Manus/Notion/Gamma）——最值得借鉴
1. Notion 三 tab（Chat/Activity/Settings）+ All chats 跨 agent 运行表（status/triggers/credits/model）
2. Cursor steering 分级：立即/排队本轮后/顺序化队列，统一在下个 tool call 边界生效
3. Cursor checkpoint 只回滚文件不动消息；时间线点击预览
4. Manus replay 回放作为分享第一公民
5. Manus Allow Once/Always Allow + "You are always in command" 文案；folder-scoped OS 原生授权
6. Cursor Agent Review 审批档位=成本档位（Quick/Deep）
7. Manus 自动化：NL→结构化预览→激活前强制确认（"review timing/conditions/actions/accounts"，字段名 "Manus will"）；Updates 流+日历双形态；Test run 一等公民（含负例建议）；多次失败自动暂停+自愈路径
8. Gamma 大纲先行主题后置（生成后一键换肤）
9. Notion 记忆=可编辑页面；Manus 知识库变更只对新任务生效（边界写进 UI）
10. Manus credits 任务前预估+参考同类历史；Notion 80%/100% 双阈值+触顶自动暂停通知创建者
11. Cursor Spending 双池（订阅/按量）+请求级下钻；BYOK key 客户端/策略后台分层

## 交叉综合（跨 journey 的结构性发现）
1. **「装了→能用」最后一公里系统性断裂**（MCP/技能/agent 三线全中招）：根因 commands.rs:456 工具装配点与扩展存储/目录约定未接通
2. **静默失败是最高频反模式**（7 处）：附件路径丢弃、Add-to-chat 丢稿、立即运行假动作、ResultRouting serde 丢弃、导出沙箱拒绝、测试连接假报、created-then-invisible —— 共同点：UI 报成功/无反馈
3. **三套任务存储两套自动化体系两套 agent 概念**：scheduled task store / background task 内存表 / .claude/tasks；triggered(toml) vs scheduled(store)；InstalledAgent vs AgentDefinition
4. **执行层欠账被 UI 提前承诺**（安慰剂字段群）：policy 四字段、ResultRouting、restart 按钮、.mcpb、billing、JSON 导出、companion 入口 —— 后端有或无，UI 都先画了
5. **i18n「键满分值不及格」**：键集 100% 对齐但 8 locale 80% 值为英文
