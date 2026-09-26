# 阶段三·项目实体化 + 阶段四·插件打包 — 实施计划

> 依据：[2026-09-23-ui-nav-ia-redesign-proposal.md](2026-09-23-ui-nav-ia-redesign-proposal.md)（提案 v1.2，五条开放问题已裁定）§3.1（P-E1..E3 / P-U1..U4）与 §3.3（X5/X6/X7）。本计划把两期落成一个分支 `feat/projects-plugins` 的 7 个任务。
> 勘察基线：`dev@51c922fd`（PR #122 合并后）。行号锚点以该版本为准。
> 产品裁决（评审人 2026-09-26 追认）：插件包格式**兼容 Claude Code plugin 格式**（X5 建议项，用户批准方案即批准此建议）。

## Global Constraints（约束所有任务）

1. **/opc 免改区**：`pages/OPC*.tsx`、`components/opc/**`、OPC 后端模块一律不动。
2. **i18n**：新 key 只写 `desktop/ui/src/i18n/locales/en.json` + `zh-CN.json`（`i18nParity.test.ts` 强制 en↔zh-CN 奇偶；其余 8 个 locale 不动）。
3. **shannon-core / shannon-types pub API 严格加法（semver minor）**：不给已有 pub 结构体加字段、不改已有 pub 签名；新增 pub 项允许。CI 有 semver-checks（baseline `semver-baseline-2026-09-15`）。因此 **ScheduledRoutine 的 working_dir 走 core 加法式 sidecar 方法 + 桌面 DTO**，不走字段。
4. **测试永不改进程 HOME**：store 测试用 `tempfile::tempdir()` + `with_base/with_path/open_in_memory` 缝隙；config 测试注入 persist 闭包。
5. **本地 Rust 门槛**：`cargo check -p shannon-desktop --no-default-features --features tauri` 与 `cargo test -p shannon-desktop --no-default-features --features tauri`（默认特性含 preview-capture/libspa 本地编译不过）；改 core 时另跑 `cargo test -p shannon-core`。Clippy 按 CI flags（`-D warnings -A unknown-lints -A clippy::collapsible_if -A clippy::collapsible_match -A clippy::derivable_impls -A clippy::manual_is_multiple_of -A clippy::manual_checked_div -A clippy::unwrap_used -A clippy::unnecessary_sort_by`）；`cargo doc -D warnings`（pub 文档不得链接私有项）。
6. **新 Tauri 命令四件套**：`main.rs` generate_handler! + `desktop/acl/app-permissions.json`（`allow-<kebab>` 权限、恰好一条 allow、挂进已有 capability 引用的 `app-*` set、main 与 session-* 两窗口都授）+ `desktop/ui/src/lib/tauri-api.ts` 类型化包装 + `desktop/ui/src/types/index.ts` 类型。`desktop/tests/app_command_acl_coverage.rs` 双向精确匹配，多/少都红。
7. **UI 验证**：绝不动 1420 端口（用户 demo server）；需要起服务用 4173。
8. README 测试计数标记不得手改（floor 语义，新增测试天然满足）。
9. UI 惯例：会话行无 `working_dir` 时保持现状渲染；已归档会话 rail 区（卡A）行为不变；现有测试除本计划点名的迁移外保持绿。

## 现状锚点（勘察结论，任务简报可直接引用）

- InboxStore（SQLite，`~/.shannon/inbox.db`）：`crates/shannon-core/src/inbox_store.rs:203-293`，`SCHEMA_SQL` 幂等批 + WAL + busy_timeout + `open_in_memory`/`open_with_legacy` 缝隙。新 store 照此模式另起 `projects.db`，不与 inbox 共库。
- ScheduledRoutine：`crates/shannon-core/src/scheduled_routines.rs:314-388`（无 working_dir；`policy.worktree` 是另一概念）。持久化 `~/.shannon/scheduled-tasks/<slug>-<id>/task.json`，`ScheduledTaskStore`（core，`with_base` 缝隙 :55）。`list_scheduled_tasks` 直返 `Vec<ScheduledRoutine>`（desktop/src/scheduled_commands.rs:540-547）。desktop 测试helper `windowed_routine_utc`/`windowed_routine` 用结构体字面量构造（desktop/src/scheduled_commands.rs:2503、inbox_commands.rs:888）。
- spawn_routine_run：desktop/src/inbox_commands.rs:526-703，finalize_run :751-774 写 inbox（item 无 working_dir 字段——本计划**不改 InboxItem**，项目归属经 session join 解析）。
- goal：`create_goal_session` desktop/src/goal_commands.rs:734-768（:760 working_dir=None）；GoalRunDto :93-109；goal 会话 sidecar 持久化。
- session working_dir：`StoredSessionMeta.project_path`（core session_store.rs:288-321）↔ `SessionMeta.working_dir`（desktop/src/commands.rs:212-220），`session_meta_from_info` hydration commands_sessions.rs:241-258；`set_session_working_dir` commands_sessions.rs:1114-1160。
- rail：SidebarSessions.tsx — projectOf :146-151（尾段字符串）；`shannon-projects` localStorage :45-58,472-481；镜头 :68-109,798-822；分组 memo :374-414（≤1 项目→null 平铺）；自动化小节 :195-198,835-872（`data-testid="sidebar-automations"`，`next_fire_at-nowTick<3600_000` 即将徽章 :843-858）；goal 徽章 :699-732；项目头 :536-585；已归档会话区 :898-962（新归档区照此）；⋯ 菜单 DropdownMenu :767-776 + 长按 :270-276。
- `reveal_in_folder`（有路径范围检查 commands_surface.rs:397,462-468）；opener 插件已注册；UI 经自定义命令（tauri-api.ts:618-629）。
- 插件：core `crates/shannon-core/src/plugin/`（manifest.rs 解析 `.claude-plugin/plugin.json` Claude 方言 :248-322,414；registry.rs PluginRegistry/InstalledPlugin；installer.rs `.dxt/.mcpb`）；desktop/src/commands_plugins.rs（list_plugins :27、install_plugin :63、install_plugin_from_git :112 含 SEC-1 allow_unverified、uninstall/enable/disable/update :131-155、list_plugin_marketplace :277）；UI Plugins.tsx 只是市场浏览器，从不调 list_plugins；tauri-api.ts:1188-1216 包装已存在但无 UI 消费。
- 技能/MCP/代理安装落点：`~/.shannon/skills/<plugin>/`（SKILL.md，skill_installers.rs:21-31,128-138）、`~/.shannon/agents/<plugin>/`、`~/.shannon/commands/`（migration_commands.rs:260-261）、`~/.shannon/desktop/mcp-servers.json`（config.rs:619）。`write_mcp_server_config`/`remove_mcp_server_config` 可复用。
- 工具执行收敛点：`ToolRegistry::execute`（core tools.rs:547）；per-tool `tokens_used` 在 tool/call 事件（shannon-types/src/events.rs:95-100）；SessionQuery（core session_query.rs，PR #121）已读 events.jsonl。
- i18n 奇偶测试：desktop/ui/src/__tests__/i18nParity.test.ts。UI 测试 setup 自动包 I18nProvider；SidebarSessions 测试范式 `sidebarRunSemantics.test.tsx:31-45`。

## 任务

### Task 1 — P-E3 项目注册表（引擎）

**Core**（新文件 `crates/shannon-core/src/project_registry.rs`，`lib.rs` 加 `pub mod project_registry;`；全部为新 pub 项=minor 安全）：
- `ProjectRecord { path: String, name: Option<String>, icon: Option<String>, color: Option<String>, archived_at_ms: Option<i64>, created_at_ms: i64 }`（Serialize+Deserialize+Clone+Debug+PartialEq）。
- `ProjectRegistry`：`open(path)`（默认 `~/.shannon/projects.db`）、`open_in_memory()`、`with_path(path)` 测试缝。内部照 InboxStore：`execute_batch(SCHEMA_SQL)` 幂等（`CREATE TABLE IF NOT EXISTS projects(path TEXT PRIMARY KEY, name TEXT, icon TEXT, color TEXT, archived_at_ms INTEGER, created_at_ms INTEGER NOT NULL)`）、WAL、busy_timeout 2000、`Mutex<Connection>`。
- 方法：`list(include_archived: bool) -> Result<Vec<ProjectRecord>>`（path 排序）；`upsert(record)`（按 path 覆盖，保留原 created_at_ms）；`rename(path, name: Option<String>)`（None=清除自定义名）；`set_appearance(path, icon: Option<String>, color: Option<String>)`；`set_archived(path, archived: bool)`；`ensure_adopted(candidates: &[ProjectAdoptCandidate]) -> Result<usize>`，`ProjectAdoptCandidate { path, name_hint: Option<String> }`（`INSERT OR IGNORE`，只登记不存在的 path——含已归档行也不覆盖；返回新登记数）；`get(path)`。
- 不存在 path 的 rename/set_appearance/set_archived：upsert 语义自动建档（保持幂等简单）。

**Desktop**（新 `desktop/src/commands_projects.rs`）：
- `AppState.project_registry`（`desktop/src/commands.rs` AppState 加 OnceLock 式字段，照 `inbox_store()` 模式 scheduled_commands.rs:472-490；磁盘打开失败回退 in-memory）。
- 命令：`list_projects(include_archived: Option<bool>)`、`register_project(path: String)`、`rename_project(path: String, name: Option<String>)`、`set_project_appearance(path, icon: Option<String>, color: Option<String>)`、`archive_project(path)`、`unarchive_project(path)`。
- **adopt-not-migrate 采集**：`list_projects` 在返回前调用 `ensure_adopted`，候选 =（a）session store `list()` 里非空 `project_path` 去重；（b）`scheduled_task_store` 各任务 sidecar working_dir（Task 2 交付前为空集，写成可迭代接口留缝）；首次（表空）另加 memory 项目标签（复用 commands_memory 的项目枚举）。名字 hint=路径尾段。
- `set_session_working_dir` 成功后调用 `ensure_adopted([新 dir])`（增量收养）。
- ACL/包装/类型四件套；`desktop/tests` 加命令级测试（tempdir 注入 registry，`with_path` 缝隙）。

**验收**：core 单测覆盖 list/upsert/rename/appearance/archive/ensure_adopted（含「已归档行不被再收养」「重建 created_at 保留」）；desktop 命令测试 + ACL 覆盖测试绿；`cargo doc -D warnings` 干净。

### Task 2 — P-E1/P-E2 例行与 goal 带 working_dir（引擎，core 仅加法）

**Core** `scheduled_task_store.rs` 加法式方法（不改 ScheduledRoutine 结构体）：
- `pub fn working_dir_of(&self, id: &str) -> std::io::Result<Option<String>>`；`pub fn set_working_dir(&self, id: &str, dir: Option<&str>) -> std::io::Result<()>`。落盘：任务目录下 sidecar 文件 `working_dir`（内容=路径一行；清除=删文件），tmp+rename 原子写。单测覆盖 set/get/clear/缺失任务。
- Task 1 的采集缝改为真实实现：`ScheduledTaskStore::working_dirs() -> Vec<String>`（遍历任务目录读 sidecar，去重）。

**Desktop**：
- `CreateTaskPayload`（scheduled_commands.rs:45-67）加 `working_dir: Option<String>`；`create_scheduled_task` 落 sidecar；`update_scheduled_task` 支持改 working_dir。
- `list_scheduled_tasks` 返回桌面 DTO：`RoutineDto { #[serde(flatten)] routine: ScheduledRoutine, working_dir: Option<String> }`（JSON 形状=原字段+working_dir，UI 向后兼容）。main.rs 注册处类型同步。
- **运行落点**：spawn_routine_run 链路里，例行的运行会话创建时把 `SessionMeta.working_dir` 戳成例行 working_dir（无则 None）。**禁止在后台线程 `std::env::set_current_dir`**（进程全局态，会污染前台会话）；若引擎调用链已有现成 per-run 工作目录参数则顺带传入，否则只戳 session meta（rail/triage join 已满足；执行 cwd 改进不在本计划）。
- goal：`start_goal_run` 从发起会话读 working_dir → `create_goal_session` 的 `SessionMeta.working_dir`（:760 None→继承）；`GoalRunDto` 加 `working_dir: Option<String>`（桌面类型自由）。
- UI 类型：`types/index.ts` ScheduledRoutine 加 `working_dir?: string | null`；GoalRunDto 加同形字段。
- 测试：创建/更新例行带 working_dir 往返；DTO flatten 序列化含旧字段；goal 会话继承；ACL 不变（无新命令）。

### Task 3 — P-U1/P-U2 rail 项目树（UI）

SidebarSessions.tsx 为主战场，`Sidebar.tsx` 传入项目注册表数据（AppContext 加载 `listProjects`，随 `sessions-updated`/启动刷新）。

1. **分组键升级**：`projectOf` 保留导出（dock 在用）另加 `projectKeyOf = working_dir 去尾斜杠`（全路径）；组显示名 = 注册表 name ?? 尾段 basename。分组 memo（:374-414）改为键=全路径。无 working_dir 的会话维持现状处理（当前 null 键行为不动）。
2. **自动化嵌树（P-U1）**：例行行（working_dir 非空）按 key 并入项目组、与会话混排（组内最近活跃排序，例行用 `next_fire_at`）；行渲染=时钟图标+name+相对时间/「即将」徽章（复用 :843-858 逻辑），点击→`/tasks`。goal 无需新行（goal 会话行+徽章已在树中——勘察确认）。**自动化小节只渲染 unhoused（working_dir 空）例行**；空则整节隐藏（`sidebar-automations` testid 保留在该节上）。
3. **空项目（P-U2）**：注册表非归档项目 ∪ 派生组=树全集；仅注册表的项目渲染「暂无任务」置灰行（非交互）。
4. **项目动作菜单**：项目头 ⋯（DropdownMenu）+长按，项：在此项目新建会话（`newSession` → `setSessionWorkingDir(path)` → /chat）；新建例行（→`/tasks`）；打开目录（`revealInFolder(path)`，若其路径范围检查拒绝则改 `openWithDefaultApp`，二者都拒绝则不渲染该项——实现时验证 :397 范围）；重命名（沿用双击内联编辑，提交走 `renameProject`）；颜色（6 色色板 popover，写入 `setProjectAppearance`——icon 字段留 API 层，UI 只做颜色，范围裁决见 ledger）；归档项目（`archiveProject`）。
5. **已归档项目区**：rail 底部折叠区（照已归档会话区 :898-962 范式），行内「恢复」（`unarchiveProject`）。
6. **localStorage 迁移与退役**：挂载一次性 effect：读 `shannon-projects`，逐条 `renameProject(dir, name)`（注册表已有同名则跳过），完成后 `localStorage.removeItem('shannon-projects')`；删除 `readProjectRegistry` 消费路径，组名只认注册表。`shannon-sessions-folded` 的键随全路径键自然迁移（UI 糖，不需数据迁移）。
7. i18n（en+zh-CN）：项目菜单/空态/归档区等 key；vitest：新 `sidebarProjects.test.tsx`（嵌树、unhoused 小节、空项目、菜单动作调用、归档区、localStorage 迁移）+ 更新 structuralUpgrades.test.tsx 受影响断言。

### Task 4 — P-U3/P-U4 项目筛选维度（UI）

1. `/tasks?project=<encoded path>`：Tasks.tsx 读 searchParams——例行 tab 按 working_dir 过滤；goal 运行卡按 dto.working_dir；执行历史按其 routine 的 working_dir；命中时顶部显示可移除的项目筛选 chip（×→移除参数）。项目菜单加「查看自动化」→ `/tasks?project=`。
2. `/triage?project=`：收件箱条目经 session_id → session working_dir join 过滤（复用 list_sessions 数据），同样 chip。
3. `/memory?project=`：preset 现有项目过滤器。
4. P-U4 回归测试：镜头选择持久化（readGrouping/persistGrouping 已实现）——补一条跨 remount 断言锁行为，不改产品代码。
5. vitest 覆盖三个页面的过滤与 chip。

### Task 5 — X5 插件包安装落料（引擎+UI）

Claude Code 兼容包 = 目录含 `.claude-plugin/plugin.json`（core manifest.rs 已解析）+ 可选 `skills/ agents/ commands/` 子目录 + manifest `mcpServers`。**落料（materialize）**进既有 per-type 落点，运行时零新机制。

- `desktop/src/commands_plugins.rs` 安装路径（install_plugin :63 本地/压缩包、install_plugin_from_git :112）：注册表安装后按 manifest 落料——`skills/<n>/SKILL.md` → `~/.shannon/skills/<plugin>/<n>/`；`agents/*` → `~/.shannon/agents/<plugin>/`；`commands/*.md` → `~/.shannon/commands/`；`mcpServers` → `write_mcp_server_config`（键 `<plugin>-<server>` 防撞）。落料清单（每条目标路径）写插件目录 sidecar `materialized.json`（**不改 core InstalledPlugin 结构体**；registry 加法式读写 sidecar 的自由函数即可，放 desktop）。
- 语义：uninstall=按清单反落料+注册表卸载（清单缺失则只卸注册表并告警返回）；disable=反落料（保留插件目录+清单）；enable=重新落料；update=重装+重落料。部分失败：尽力而为 + 逐条错误收集返回。
- 信任预览：新命令 `inspect_plugin_source(path: String)`（本地目录/压缩包：解 manifest 返回 `PluginBundleSummary { name, source_format, skills: Vec<String>, agents: Vec<String>, commands: Vec<String>, mcp_servers: Vec<String> }`；git URL 走浅克隆到 tempdir 再读）+ 四件套。InstallDialog 插件类条目渲染「将启用」清单（现有 X2 信任卡扩展）。
- 市场接线：Plugins.tsx 卡片 Install → InstallDialog → installPlugin/installPluginFromGit（确认现走线，补 bundle 摘要），成功后 `shannon:extension-installed` + 刷新。
- 迁移导入落为插件（薄版）：`migration_apply` 完成后注册一条 InstalledPlugin（`imported-<source>`，manifest 如实罗列导入的 skills/mcp/commands，source_format=claude-json，标注来源=迁移导入）；该条目 UI 禁用 卸载/启停（避免「卸载导入」的破坏语义）。完整迁移→插件包装**不做**（ledger 裁决）。
- 测试：core/桌面 tempdir 落料-反落料往返（含 enable/disable/update 循环）；inspect 本地+摘要正确；ACL 四件套；InstallDialog 测试扩展。

### Task 6 — X6 插件来源与已安装管理（UI）

Plugins.tsx（`/extensions/plugins`）从纯市场浏览器升级：
1. 「+ 添加插件」菜单：从 Git URL（输入框+未签名需 `allow_unverified` 勾选 SEC-1 → installPluginFromGit）；从本地目录（plugin-dialog 选目录 → installPlugin）；从压缩包（.dxt/.mcpb/.zip → installPlugin）。
2. 「已安装」分区（listPlugins 首次真实上 UI）：行=名称、来源徽标（registry/git/local/migration）、source_format、启用开关（enable/disable）、更新、卸载（确认框列将移除的落料）；migration 来源行动作禁用+说明。
3. 市场卡标注来源 upstream。i18n（en+zh-CN）；vitest：添加菜单三分支、已装行动作、migration 行禁用。

### Task 7 — X7 扩展 Stats（引擎+UI）

- Core `SessionQuery` 加法式：`pub fn tool_call_stats(&self, days_back: u64) -> Result<Vec<ToolCallStat>>`，`ToolCallStat { name: String, calls: u64, total_tokens: u64 }`——扫 `list_recent(days_back, include_archived=false)` 各会话 events.jsonl 的 tool/call 事件，名字计数、`tokens_used`（shannon-types events.rs:95-100）求和。fixture 日志单测。
- Desktop `get_extension_stats(days: u32)`（extensions_commands.rs 或 cost_commands.rs）：聚合分桶 `skill_<id>`→skills、`mcp__<server>__<tool>`→按 server 汇总+按 tool 明细、其余 other；四件套。
- UI：Installed.tsx 行内缀「30 天调用 N 次 · ~X tokens」（能匹配上的条目）；数字缺省不渲染。i18n；core 适配器测试 + 命令测试 + vitest。

## 依赖与顺序

T1 → T2 → T3 → T4（引擎→UI 链）；T5 → T6；T7 独立（UI 落点在 Installed.tsx，不依赖 T5/T6）。顺序执行：T1, T2, T3, T4, T5, T6, T7。每任务独立可合并、可单独回滚。

## 明确不做

- 不做独立项目详情页（提案 §3.1 明确）；「对话」术语不改。
- 不改 `InboxItem` 表（项目归属经 session join）；不做 routine 执行进程 cwd 切换；icon UI（色板已含，icon 图形编辑不做）。
- 迁移→插件的完整包装（卸载即删导入物）不做，只做如实登记的薄版。
- 不动 /opc、不动 1420。
