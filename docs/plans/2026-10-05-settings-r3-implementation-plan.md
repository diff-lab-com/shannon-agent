# 设置页对齐 ZCode — R3 实施计划

- 日期: 2026-10-05
- 分支: `feat/settings-parity-r3`(worktree `shannon-settings-r3`,基于 origin/dev)
- 上游方案(已获批): `docs/plans/2026-10-05-zcode-settings-comparison-and-improvement-plan.md`(对比分析 + 批次 A/B/C/D)
- 范围: 批次 A 全部 + 批次 B 全部 + 批次 C 全部 + D2(终端)+ D3 第一步(数据路径只读展示)。**不含** D1(更新签名基建,维持手动检查)、D4(界面模式实验)、D5(主动任务推荐)。

## Global Constraints(所有任务必须遵守)

1. **i18n**: 所有用户可见文案走 react-intl 扁平 dot-key;10 个 locale 文件(`desktop/ui/src/i18n/locales/*.json`)都要加;en + zh-CN 按任务给出的文案精确写入,其余 8 语言给出合理翻译(缺失回落 en,不报错)。
2. **Rust 风格**: 库 crate 用 thiserror、bin 用 anyhow;生产代码 `expect("reason")` 优先于 `unwrap()`;不引入新的外部 crate(需 controller 批准)。
3. **UI 风格**: 沿用现有设置卡样式(`bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1` + material-symbols 图标 + `font-headline-md` 标题 + `font-body-sm text-on-surface-variant` 说明);语义徽章用任务 T1 的 `EffectBadge`;控件用现有 `Switch`/`select`/Button 模式。
4. **DesktopConfig 新键必须 `#[serde(default)]`**(向后兼容旧 config.json);configure 分支写内存 → `config::save_config()` → emit `CONFIG_UPDATED`。
5. **测试**: UI 用 vitest(`desktop/ui`,`pnpm vitest run <file>`,参考 `__tests__/GeneralSettings.test.tsx` 等现有模式);Rust 用 `#[cfg(test)]` 内联或 `crates/*/tests/`,跑 `cargo nextest run -p <crate>`;**不得破坏现有测试**,任务完成前必须跑过受影响的套件。
6. **生效语义**: 每个新设置项用 `EffectBadge` 标注(即时 / 新会话生效 / 需重启应用 / 需重启网关);帮助文案说明作用域。
7. 提交信息用仓库惯例(`feat(desktop): ...` / `fix(...)` 中文或英文均可,见 git log),每个任务一个或多个原子提交。
8. 安全: 密钥/证书内容不落日志;路径输入做 `~` 展开与存在性校验。

## 控制器裁决(对上游方案的实施细化)

- R1 代理留空 = 保留现有"隐式读环境变量"兜底(不强制直连),与 ZCode 的差异写入帮助文案。
- R2 locale 缺失回落 en;新 key 必须 en+zh-CN,其余 8 语言 best-effort。
- R3 keep-awake 不引入新 crate:macOS 沿用 caffeinate;Linux 用 `systemd-inhibit` 子进程;Windows 用 windows-sys `SetThreadExecutionState`(仅当依赖图已有 windows-sys,否则 no-op + UI 标注"当前平台不支持")。
- R4 通知"提示音"用前端 Web Audio 合成 chime(跨平台一致、可测),OS 通知声保持系统默认;开关默认关。
- R5 会话 pinned 从 localStorage 迁到 curation sidecar(自动归档后端扫描需要),一次性迁移。
- R6 "已完成"判定 = `!running && 无未读 inbox 条目`;不新增 completed 字段。
- R7 本 harness 的 Agent 工具无模型参数,所有 subagent 用会话默认模型。
- R8 桌面端 ask_user 此前完全不可用(stdin handler 在 GUI 下 EOF/阻塞),T8 实现桌面问答链路是 C3 的前置,属本计划范围。
- R9 PR 目标分支 = `dev`(仓库惯例),单 PR 分任务提交。

---

## Task 1 — 设置页骨架:三分区 + EffectBadge + Advanced 守卫 + 关于区(A1/A2/A4/D3)

**目标**: 新增「网络」「会话」「关于」三个分区(先建骨架与关于区内容),统一生效语义徽章组件,补 Advanced 路由守卫,把"检查更新"从 dev-gated 高级区移到「关于」区,新增数据目录只读展示。

**后端**:
- `desktop/src/commands_surface.rs`: 新增命令 `get_shannon_home() -> String`(返回 `shannon_core::data_meta::home()` 的 display 字符串);`desktop/src/main.rs` 的 `generate_handler` 注册。
- `desktop/ui/src/lib/tauri-api.ts`: 封装 `getShannonHome()`。

**前端**:
- `desktop/ui/src/components/settings/EffectBadge.tsx`(新): props `{ kind: 'instant' | 'new-session' | 'restart-app' | 'restart-gateway' }`,渲染小号 tag(颜色: instant=outline 中性、new-session=secondary、restart-app/warning=amber 调、restart-gateway=tertiary 调),文案 i18n:`settings.effect.instant` / `settings.effect.newSession` / `settings.effect.restartApp` / `settings.effect.restartGateway`。带 vitest 测试(快照或断言四类文案)。
- `desktop/ui/src/pages/Settings.tsx`: SECTIONS 增加三条(顺序: general, theme, models, permissions, network, session, notifications, connections, remotes, about;advanced 保持 dev-gated 且排最后)。labelId: `nav.network` / `nav.session` / `nav.about`,图标: network=`lan`, session=`forum`, about=`info`。
- `desktop/ui/src/App.tsx`: 三个子路由 `/settings/network`、`/settings/session`、`/settings/about`;并为 `/settings/advanced` 加守卫(非 dev 模式访问 → `<Navigate to="/settings/general" replace/>`;在路由 element 里包一个小组件 `RequireDevMode`,用 `useSidebarMode` 判断)。
- `desktop/ui/src/components/settings/AboutSettings.tsx`(新): ① 版本行(当前版本,`getAppVersion` 若已有 tauri API 则用,没有就加 ` getVersion` 封装) ② "检查更新"按钮 + 结果徽标 + "打开发布页"按钮(逻辑从 `AdvancedSettings.tsx:588-644` 迁移,复用 `api.checkAppUpdate`/`api.openReleasePage`) ③ 数据目录只读行(`getShannonHome()` + 帮助文案说明 `$SHANNON_HOME` 可覆盖、修改需另行迁移,链接/提示到日志目录卡片仍在高级区)。
- `desktop/ui/src/components/settings/AdvancedSettings.tsx`: 删除「版本与更新」卡片(逻辑迁走),原位置留一行交叉链接到「关于」分区(NavLink)。开发者选项里的"打开日志目录"保留。
- `NetworkSettings.tsx` / `SessionSettings.tsx`: 本任务先渲染占位卡("即将在本版本后续提交中提供",i18n key `settings.network.placeholder` / `settings.session.placeholder`),后续任务替换。
- GeneralSettings 里 sandbox 卡的"重启后生效"文案行(`PermissionsSettings.tsx:537-540`)替换为 `<EffectBadge kind="restart-app"/>`;Connections 页已有网关重启横幅保留不动。
- **locale**: 所有新 key 加进 10 个文件(en/zh-CN 精确,下同)。key 清单: `nav.network`(网络/Network)、`nav.session`(会话/Sessions)、`nav.about`(关于/About)、`settings.effect.*` 四条(即时生效/新会话生效/需重启应用/需重启网关 + 英文 Instant effect / Applies to new sessions / Restart required / Gateway restart required)、`settings.about.*`(title/help/version/checkUpdate/checking/updateAvailable/latest/openRelease/dataDir/dataDirHelp)、`settings.network.placeholder`、`settings.session.placeholder`、`settings.advanced.movedToAbout`(更新功能已移至「关于」/ Updates moved to About)。

**测试**: EffectBadge 测试;AboutSettings 测试(mock tauri-api,断言版本行/按钮/数据目录渲染);Settings 页 nav 渲染 10+1 项测试(更新 `__tests__/Settings.test.tsx` 若存在);守卫测试(非 dev 访问 advanced → 重定向)。

---

## Task 2 — 语言下拉 + 跟随系统(A3)

**目标**: 语言控件从 10 按钮组改为 `<select>`,新增「跟随系统」选项(显式持久化 null),首启行为不变。

**前端**:
- `desktop/ui/src/i18n/index.tsx`: `locale` 存储语义扩展 —— localStorage `shannon.locale` 值可为 `'system'`(或移除 key)表示跟随;`resolveLocale(pref)`: 'system' 或缺失 → 按 `navigator.languages` + zh-TW/HK 分支探测(把现有首启探测逻辑抽成纯函数并复用);`setLocale('system'|Locale)`;导出 `SUPPORTED_LOCALES` 不变。
- `GeneralSettings.tsx:193-218`: 按钮组 → select(选项: system + 10 locale),help 文案补"跟随系统时按操作系统语言显示"。
- **locale**: `settings.language.system`(跟随系统 / System default)、`settings.language.help` 更新措辞。

**测试**: i18n resolveLocale 单测(vitest,mock navigator);GeneralSettings 测试更新(选择 system/locale 各写对 localStorage)。

---

## Task 3 — 硬件加速开关 + keep-awake 开关(B3 + B2)

**目标**: ① 关闭硬件加速的逃生开关(Linux/Windows,重启生效);② "任务运行时阻止休眠"(默认开,跨平台)+ "保持电脑运行"常开开关(默认关),对齐 ZCode 语义。

**后端**:
- `desktop/src/main.rs`(`:107` 与 `:121` 之间,Builder 之前): 用 `config::load_config()` 原始读一次(纯文件读,AppState 建立前的轻量调用——若 load_config 依赖 State 则抽一个 `config::read_raw()`);若 `hardware_acceleration == false`:Linux `set_var("WEBKIT_DISABLE_COMPOSITING_MODE","1")` + `set_var("WEBKIT_DISABLE_DMABUF_RENDERER","1")`,Windows `set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS","--disable-gpu")`(均 `unsafe`,参考 main.rs:1318 先例;cfg 目标平台分支;macOS 无操作)。
- `desktop/src/config.rs`: `DesktopConfig` 加 `hardware_acceleration: bool`(default true,键名 `hardware_acceleration`)与 `power_keep_awake: bool`(default false)、`power_block_sleep_during_tasks: bool`(default true);`commands_config.rs` 三个 configure 分支(`hardware_acceleration` / `power.keep_awake` / `power.block_sleep_during_tasks`,写后 emit CONFIG_UPDATED)。
- keep-awake 跨平台: `crates/shannon-core/src/prevent_sleep.rs` 重构平台后端 —— macOS 保持 caffeinate;Linux:acquire 时 spawn `systemd-inhibit --what=idle sleep infinity` 子进程(stop 时 kill;二进制缺失→降级 no-op 并 log::warn 一次);Windows:若 Cargo 依赖图已有 `windows-sys`(查 `cargo tree -i windows-sys`),给 shannon-core 加目标平台依赖 + `SetThreadExecutionState(ES_CONTINUOUS|ES_SYSTEM_REQUIRED)`,stop 时恢复 `ES_CONTINUOUS`;若不可行则 Windows no-op。公开 API 保持 `start_prevent_sleep/stop_prevent_sleep/is_preventing_sleep` 不变。
- desktop 接线: `AppState` 持有常开 Guard 句柄;`AppState::new` 后按 `power_keep_awake` 启动;configure 切换时 start/stop;应用退出 force_stop。任务运行期自动防休眠:在 `desktop/src/commands.rs` send_message 开始/结束处调 `start_prevent_sleep()/stop_prevent_sleep()`(引用计数,对齐 TUI 用法;仅当 `power_block_sleep_during_tasks` 为 true)。
- 诊断: 「关于」区数据目录行旁不涉及;平台不支持时前端如何感知 —— `get_power_capabilities()` 命令(返回 `{platform: 'macos'|'windows'|'linux', supported: bool}`),Linux 检测 `systemd-inhibit` 是否存在(运行时 `which`),Windows 按编译期。

**前端**:
- General 区新增「系统」组两张卡: ① 硬件加速 Switch(macOS 隐藏;关闭文案说明"规避部分显卡/驱动导致的白屏、闪退、渲染异常")+ EffectBadge restart-app;② 防休眠:两个开关(任务运行时阻止休眠[默认开] / 保持电脑运行[默认关])+ 平台不支持时显示提示行;EffectBadge instant。
- tauri-api 封装 `getPowerCapabilities`。
- **locale**: `settings.system.hwAccel.*`、`settings.system.keepAwake.*`(title/help/taskToggle/alwaysToggle/unsupported)。

**测试**: prevent_sleep Linux 后端单测(spawn/kill 语义,用 mock 二进制路径注入);config 默认值兼容测试(旧 config.json 无新键可加载);GeneralSettings 测试(开关写入 configure;macOS 分支 mock 隐藏)。

---

## Task 4 — 网络分区:HTTP 代理 / NO_PROXY / 自定义 CA(B1)

**目标**: 企业网络三件套。语义: 显式配置优先;留空保留现有 env 兜底(R1);覆盖 LLM/MCP/gateway/命令工具子进程;webview 跟随系统(帮助文案注明)。重启生效。

**后端**:
- `desktop/src/config.rs`: `network_proxy_url: Option<String>`、`network_no_proxy: Option<String>`、`network_ca_cert_path: Option<String>`(serde default None;configure 分支 `network.proxy_url`/`network.no_proxy`/`network.ca_cert_path`,值做 trim,空串→None;CA 路径 `~` 展开 + 存在性校验,不存在返回配置错误)。
- `desktop/src/main.rs`(Builder 之前,同 T3 的早期读取点): 若 `network_proxy_url` 有值 → `set_var("HTTPS_PROXY", url)` + `set_var("HTTP_PROXY", url)`(+ `ALL_PROXY`);`network_no_proxy` 有值 → `set_var("NO_PROXY", v)`;`network_ca_cert_path` 有值 → `set_var("SHANNON_CA_BUNDLE", path)` + `set_var("NODE_EXTRA_CA_CERTS", path)` + `set_var("SSL_CERT_FILE", path)`(子进程继承:gateway sidecar、MCP stdio、命令工具)。所有 set_var 仅在对应值有值时执行(不覆盖用户已有 env?—— 裁决:配置值优先,直接覆盖)。
- `crates/shannon-engine/src/api/client.rs`: 抽公共 `fn apply_network_tuning(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder`:读 `SHANNON_CA_BUNDLE`,存在则 `reqwest::Certificate::from_pem` 逐个 `add_root_certificate`(解析失败 log::warn 并忽略该证书,不 panic);`build_client`(:136-145)与 `try_new`(:163-176)两处接入。
- desktop 自身出口客户端: 新建 `desktop/src/desktop_http.rs` helper `pub fn builder() -> reqwest::ClientBuilder`(含同样的 SHANNON_CA_BUNDLE 注入);应用到 provider 探测(`commands_config.rs:1614` 附近)与模型目录刷新(models.dev overlay 的客户端)。其余散点暂不迁移(PR 说明)。
- **locale/UI**:
- `NetworkSettings.tsx`(替换占位): 三张卡: ① HTTP 代理(text 输入,placeholder `http://127.0.0.1:7890`,help 覆盖范围说明+R1 差异) ② 不使用代理的地址(text,placeholder `localhost,127.0.0.1,::1,.example.com`) ③ 自定义 CA 证书路径(text,placeholder `~/certs/root-ca.pem`,help 说明注入 NODE_EXTRA_CA_CERTS/SSL_CERT_FILE/LLM 客户端)。每卡 EffectBadge restart-app;统一「保存」按钮(三项一起存,成功 toast);校验: 代理 URL 需 http(s) scheme、CA 路径存在性(失焦校验,错误行内提示)。
- **locale**: `settings.network.*`(title/proxy/proxyHelp/noProxy/noProxyHelp/ca/caHelp/save/saved/invalidProxy/caNotFound/placeholder 系列)。

**测试**: Rust: env 注入函数单测(给定 config → 期望 env 集,用临时 HOME/隔离 env 的测试模式,注意 set_var 进程全局性——测试串行或用独立测试 crate 策略);CA 解析单测(生成自签 PEM 写临时文件 → builder 不 panic、坏文件 warn 忽略);configure 分支测试;UI: NetworkSettings 表单测试(校验/保存调用)。

---

## Task 5 — 通知增强:审批 OS 通知 + 提示音(B4)

**目标**: ① 工具审批等待时发 OS 通知(新增 needs_attention 事件类型,可独立开关,默认开);② 任务完成/失败/需确认时前端提示音(Web Audio chime,默认关),均受 master 与 DND 约束。

**后端**:
- `desktop/src/config.rs`: `notifications_on_needs_attention: bool`(default_true)、`notifications_sound_enabled: bool`(default false)。
- `desktop/src/commands_notifications.rs`: `NotificationPrefsDto`/get/set 增两字段;`NotificationPrefs`(消费侧,:411-433)增加 `on_needs_attention` 与 `sound_enabled` 读取;`allows_level` 扩展为按 kind 三分(completed/failed/needs_attention——在 `crates/shannon-core/src/notifier.rs` 的 `Notification` 上加 `kind: NotificationKind` 字段(default Completed,serde skip?——Notification 不序列化上 wire,直接加枚举字段带 default),现有调用点补 kind)。
- `desktop/src/commands_permissions.rs` `prompt_user`(:70 emit 之后): `state.notifier.notify(&Notification{ title, body: 含会话标题/工具名, level: Warning, kind: NeedsAttention, source: Some("session_approval"), ..})`(走既有 master/DND 过滤;用 notify_dedup 防风暴,window 5s)。预算告警等现有 notify 调用点补 kind(Budget → NeedsAttention 或新增,裁决: 预算告警 kind=NeedsAttention)。
- 完成事件补 kind=Completed、失败 kind=Failed(commands_notifications.rs:304-326)。
- 点击聚焦: 已有 `notification-clicked` 监听兜底,不动。

**前端**:
- 新建 `desktop/ui/src/lib/notificationChime.ts`: Web Audio 合成两音 chime(如 E6→G6 正弦,150ms 间隔,音量 0.15,AudioContext 懒创建,播放失败静默);导出 `playTaskChime(kind)`。
- `AppContext` 或 `Chat.tsx` 已有 QUERY 完成/失败事件监听处(找到现有 query complete/error 事件处理)挂 chime;`permission-request` 事件处挂 chime(kind: attention)。前置条件: `api.getNotificationPrefs()` 结果缓存(master && sound_enabled && !withinDnd && 对应事件开关);prefs 变化(CONFIG_UPDATED key=notifications)时失效缓存。
- `NotificationsSettings.tsx` DndSection: 增"需要确认时通知"开关(on_needs_attention,默认开,主开关关时禁用)与"提示音"开关(sound,默认关,help: 浏览器合成提示音,与系统通知声独立);保存走现有 Save;「发送测试通知」若触发后端通知则同时触发前端 chime(仅 sound 开时)。
- **locale**: `settings.notifications.needsAttention.*`、`settings.notifications.sound.*`(title/help/开关 label)。

**测试**: Rust: NotificationPrefs allows 三分单测;prompt_user 发通知单测(mock notifier 或断言调用);DTO 往返。UI: chime util 单测(AudioContext mock,断言不抛错/调用序列);NotificationsSettings 新开关渲染与保存测试。

---

## Task 6 — 会话分区:GC 迁入 + 自动压缩开关(C1)

**目标**: 新「会话」分区承接会话生命周期设置;自动压缩可关(完整保留模型 I/O)。

**后端**:
- `crates/shannon-core/src/query_engine/types.rs`: `QueryEngineConfig` 加 `auto_compact_enabled: bool`(default true)。
- `crates/shannon-core/src/query_engine/engine/agent_loop.rs`: 压缩入口处(:1431-1479 一带)`config.auto_compact_enabled == false` 时跳过 compact 分支(60%/80% 预警注入与 micro-compaction 一并跳过;保留 token 统计)。加注释说明与 context_policy 的关系。
- `desktop/src/config.rs`: `context_auto_compact: bool`(default true)+ configure 分支 `context.auto_compact`。
- `desktop/src/commands.rs` send 构造点(:1588-1736 一带): 构造 QueryEngineConfig 时读 desktop config 写入(引擎每轮重建 → 实时生效;goal/batch 路径 :2573-2596 同步)。注意 QueryEngine::with_defaults_arc 是否接受 config 参数——若 config 字段 pub(crate) 不可外设,按调研给的最小方案:为 with_defaults_arc 加 `with_config` 变体或公开 builder 方法(shannon-core 内修改,保持向后兼容默认)。
- **前端**:
- `SessionSettings.tsx`(替换占位): ① 「自动压缩上下文」卡: Switch(默认开),help="关闭后完整保留模型请求与响应,不自动压缩、截断旧记录;上下文耗尽时该轮会失败,可随时用 /compact 手动压缩"+ EffectBadge new-session(下一轮生效——裁决: 引擎每轮重建,实测为下一条消息生效,文案写"对下一条消息生效"); ② 「会话存储管理」卡: 从 `AdvancedSettings.tsx:370-408` 整卡迁入(开关+保留期下拉,补 7 天档),原位置留交叉链接; ③ 后续任务(T7/T8/T10/T11)在此页追加卡片,本任务先留注释锚点。
- AdvancedSettings 删除 GC 卡。
- **locale**: `settings.session.*`(title/compaction: title/help/offHint…)、`settings.session.gc.*`(沿用原 key 若已有则复用)、`settings.advanced.movedToSession`。

**测试**: Rust: context_policy/agent_loop 层面单测(auto_compact_enabled=false 不触发 compact 路径——按现有测试模式 mock);config 往返;UI: SessionSettings 渲染 + GC 迁移后 Advanced 不再含该卡。

---

## Task 7 — 自动归档 + pinned 后端化(C5)

**目标**: 定时扫描,将 `!running && 无未读 inbox && 未置顶 && 最后更新早于保留期` 的会话自动归档(可撤销);pinned 迁到 curation sidecar(R5)。

**后端**:
- curation sidecar 扩展: `desktop/src/commands_sessions.rs` 的 curation 读写(:178-180 一带)加 `pinned: bool`(serde default);新命令 `set_session_pinned(id, pinned)`(写 sidecar + emit 事件 `session-pins-changed`);`session_wire_info`/列表 DTO 若前端需要可带 `pinned`(裁决定: 列表 DTO 加 `pinned: bool` 字段,serde default false)。
- 自动归档: 新函数 `run_auto_archive_scan()`(同文件): 枚举活跃(未归档)会话 → 跳过 running(registry)、跳过 pinned、跳过存在未读 inbox 条目的会话(inbox_store 查询;未读定义沿用 inbox 侧现有状态字段)、`updated_at`(events.jsonl mtime)早于 now - retention_days → 调既有 `apply_archived_flag`(归档动作记 inbox/事件 `session-auto-archived`,前端可 toast)。配置: `session_auto_archive_enabled`(default false)、`session_auto_archive_days`(default 7,clamp 1..365)。调度: 照抄 `spawn_session_gc` 骨架(:666-690):启动 15 分钟后首轮,此后每 6h;enabled=false 时直接 return。
- **前端**:
- `SidebarSessions.tsx`: pin/unpin 改走后端命令(保留 localStorage 读一次迁移:启动时若 localStorage 有 pin 列表 → 逐个 set_session_pinned → 清除 key);列表渲染读 DTO 的 pinned。
- `SessionSettings.tsx`: 新卡「自动归档」: 开关(默认关)+ 保留期下拉(1/7/30/90,默认 7)+ help(已完成、无未读、未置顶且超过保留期的会话会自动归档,可在侧栏归档区恢复)+ EffectBadge instant(扫描周期说明)。归档事件 toast(监听 `session-auto-archived`)。
- **locale**: `settings.session.autoArchive.*`、`sessions.autoArchivedToast` 等。

**测试**: Rust: 扫描逻辑单测(临时 sessions 目录 + inbox store:满足/不满足各条件用例、pinned 跳过、running 跳过);sidecar pinned 往返;UI: SidebarSessions pin 调用后端(mock)、SessionSettings 卡测试。

---

## Task 8 — ask_user 桌面问答链路 + 超时自动继续(C3 + R8)

**目标**: 桌面端可回答 agent 提问(UI 卡片);可选"5 分钟未回答自动继续"(默认关)。

**后端**:
- `crates/shannon-tools/src/ask_user.rs`: 无需改(QuestionHandler trait 已有);确认 `AskUserError` 变体足够(超时→返回预设答案字符串而非错误,见下)。
- `desktop/src/ask_user_handler.rs`(新): `DesktopQuestionHandler` 实现 `QuestionHandler`:
  - `ask_question`: 生成 request_id → 存 oneshot sender 进 `state.pending_questions`(AppState 新 DashMap)→ `app.emit("ask-user-request", AskUserRequest{request_id, session_id?, question, options?…})`(payload 按Question 结构字段裁剪)→ 等待应答;若 `chat.ask_user_auto_continue`(desktop config,默认 false)→ `tokio::time::timeout(5min, rx)`,超时发 `ask-user-resolved`(request_id, 'timeout')事件并返回 `vec!["(用户未回答,请按你的最佳判断继续)".into()]`(与 permission 侧 timeout 模式一致,commands_permissions.rs:101 先例);关闭则无限等待。
  - tauri 命令 `respond_ask_user(request_id, answers: Vec<String>)`(校验 request_id 存在,send 后清理;不存在→静默 ok + log)。
- 注册: `desktop/src/commands.rs` AppState::new 的工具注册处(:772-806)——注册后用同名工具覆盖:`AskUserQuestionTool::new(Arc::new(DesktopQuestionHandler{..}))` 注册进 registry(shannon-tools 的 `register_default_tools_with_providers` 注册后,desktop 紧接着 register 同名覆盖;若 registry 拒绝重名,给 registry 加 `register_override` 或先 unregister——按最小侵入实现并说明)。
- config: `chat_ask_user_auto_continue: bool`(default false)+ configure 分支 `chat.ask_user_auto_continue`。
- 事件常量: `crates/shannon-types/src/events.rs` event_names 加 `ask-user-request`/`ask-user-resolved`;payload 结构体在 `desktop/src/events.rs`。
- **前端**:
- 聊天流新组件 `desktop/ui/src/components/chat/AskUserCard.tsx`: 监听 `ask-user-request` → 在消息流(或 composer 上方,与权限卡同区域——找到 permission-request 卡的渲染位置,并排)渲染问题卡(问题文本 + 选项按钮 + 自由文本输入 + 发送按钮);提交 → `api.respondAskUser(...)`;卡上显示"已超时自动继续"态(监听 resolved timeout)。自动继续开启时卡显示 5:00 倒计时。
- `SessionSettings.tsx`: 新开关「提问自动继续」(help: Agent 提问 5 分钟未回答时,自动以"按最佳判断继续"应答;关闭则一直等待)。
- **locale**: `chat.askUser.*`(title/optionsLabel/customPlaceholder/send/timedOut/countdownAria)、`settings.session.askAutoContinue.*`。

**测试**: Rust: handler 单测(应答解析/超时路径用小超时时间注入——timeout 时长从 config 或参数读,便于测试;重复应答幂等);registry 覆盖注册后 `list_tools` 含 ask_user_question。UI: AskUserCard 渲染/提交/倒计时测试(mock tauri events——按现有 useTauriEventValidated 测试模式)。

---

## Task 9 — 显示思考过程(C2)

**目标**: 历史消息渲染 thinking;三档开关(全部显示[默认,折叠]/仅每轮第一次/关闭)。

**前端**(display-only,localStorage `shannon.chat.showThinking`: 'all'|'first'|'none'):
- `MessageBubble.tsx`: `ChatMessage.thinking` 有值时渲染折叠 Reasoning 块(复用 `ai-elements` 的 Reasoning,defaultOpen=false,样式对齐流式版);'none' 不渲染;'first' 仅当该消息是本轮(相邻 user 消息之后)第一条 assistant 时渲染——实现为父层遍历时传入 `isFirstAssistantOfTurn` 布尔(在消息列表 map 处计算)。
- `StreamingResponse.tsx:73-76`: 'none' 时不渲染 Reasoning;'first' 流式期间照常(第一轮概念在流式期间即当前轮)。
- 历史数据: 调研会话历史加载是否恢复 `thinking`(AppContext 水合路径)——若事件流已带 thinking 块则接上;若历史确实无数据,在 PR 描述注明"历史 thinking 依赖引擎事件持久化,本轮先覆盖流式+已水合数据"并不做引擎改动(裁决: 不扩引擎持久化)。
- 设置入口: General 区「显示思考过程」三段按钮组(全部/仅第一次/关闭)+ help("关闭时每轮仍展示第一次思考"语义并入 'first' 说明)。 
- **locale**: `settings.general.showThinking.*`(title/help/all/first/none)。

**测试**: MessageBubble 三档渲染测试(thinking 有/无 × 三档);first-of-turn 计算函数单测;GeneralSettings 控件测试。

---

## Task 10 — 发送行为设置(C4)

**目标**: 运行中发送 = 插话打断(steer)/加入队列(queue)用户可选(默认维持现状行为)。

**前端**(localStorage `shannon.chat.sendBehavior`: 'steer'|'queue',默认现状——实施时先读 `Chat.tsx:548-559`+`useSteerSend.ts` 确认现状是哪档,默认值设为现状):
- 发送路径按 pref 分流: queue → 走现有 enqueue;steer → 走 useSteerSend。
- SessionSettings 新卡「运行中发送消息」: 两档按钮组(插话打断/加入队列)+ help;EffectBadge instant。
- **locale**: `settings.session.sendBehavior.*`。

**测试**: 发送分流单测/组件测试(pref 两档调用不同路径——mock hooks)。

---

## Task 11 — 工具调用分组(C6)

**目标**: 连续同类工具调用聚合为可折叠组:Explore(只读:读/搜/列)、Terminal(非只读 shell)、Changes(写文件类);三开关,默认开。

**后端**(小):
- `desktop/src/commands.rs` `ToolInfo`(:630-635)加 `read_only: bool`(来自 `Tool::is_read_only()`);`list_tools` 组装处补;wire serde default true 兼容。

**前端**:
- types `ToolInfo` 加 `read_only`;`getTools()` 消费处存 map(tool_name → read_only),分组时查表,查不到按启发式集合兜底(现有 `FILE_MUTATING_TOOLS` + bash/shell 名单 + 其余视为只读)。
- 新组件 `desktop/ui/src/components/chat/ToolGroupCard.tsx`: props {kind:'explore'|'terminal'|'changes', children: ToolCallDisplay[]};折叠头(图标+计数+工具名摘要),展开渲染原卡片;样式对齐 RetryChainBanner/SubagentBlock 的折叠模式。
- `MessageBubble.tsx`: 渲染工具卡列表前做连续分组(相邻且同 kind 且开关开 → 包 ToolGroupCard;分类: shell/bash 类→terminal(仅非只读);FILE_MUTATING_TOOLS→changes;其余→explore);`StreamingResponse.tsx` 流式同样处理(或流式保持逐卡——裁决: 流式期间逐卡,完成后按组渲染,避免抖动;PR 注明)。
- 开关: SessionSettings「消息流分组」三开关(探索工具/终端命令/文件更改,localStorage `shannon.chat.grouping.*`,默认 true)。
- **locale**: `settings.session.grouping.*`、`chat.toolGroup.*`(exploreTitle/terminalTitle/changesTitle/count)。

**测试**: 分组函数单测(序列→组分段,含开关关闭直通);ToolGroupCard 渲染测试;MessageBubble 集成测试(mock ToolCall 序列)。

---

## Task 12 — 终端:login shell 继承 + 字体族(D2)

**目标**: ① 「继承登录 shell 环境」开关(login shell 启动);② 终端字体族可配(留空 = 现有等宽栈)。

**后端**:
- `desktop/src/terminal_commands.rs`: `TerminalSettings` 加 `login_shell: bool`(default false)、`font_family: Option<String>`;`sanitized()`(:301-313)处理(trim 空串→None;font_family 长度 clamp 200);DTO(:321-351)+load/set(:403)同步;`spawn`(:776-832):`login_shell==true` 且 program basename ∈ {bash,zsh,fish,ksh} → args 前插 `-l`(zsh/fish/bash 支持;Windows 忽略)。
- **前端**:
- `types/index.ts` TerminalSettings 类型加两字段;`TerminalPanel.tsx`: `fontFamily: settingsRef.current?.font_family || FONT_FAMILY`(:263 附近)。
- `TerminalSettings.tsx`: 新增「继承登录 shell 环境」Switch + 「终端字体」text 输入(placeholder 现有等宽族示例,help: 留空自动使用内置等宽栈)+ Save;现有"对之后打开的终端生效"文案沿用。
- **locale**: `settings.terminal.loginShell.*`、`settings.terminal.fontFamily.*`。

**测试**: Rust: sanitized 单测(新字段)、spawn args 单测(login_shell 开/关 × 平台分支——按现有 spawn 测试模式,若无则抽 args 构造纯函数测);UI: TerminalSettings 表单测试。

---

## 任务依赖与文件共享矩阵(预检)

| 任务 | 共享文件 | 关系 |
|---|---|---|
| T1→T6/T7/T8/T10/T11 | `SessionSettings.tsx`(T1 建占位) | 后续任务替换占位、追加卡片,顺序执行无冲突 |
| T1→T6 | `AdvancedSettings.tsx`(T1 删更新卡,T6 删 GC 卡) | 不同卡片,顺序无冲突 |
| T3→T4 | `main.rs` Builder 前早期块(T3 硬件加速,T4 网络 env) | 同一插入区域,T4 在 T3 代码之后追加 |
| T3/T4/T5/T6/T7/T8 | `config.rs`/`commands_config.rs`(各加自己的键) | 键不重叠,顺序无冲突 |
| T5/T6 | `commands.rs`(T5 无,T6 send 构造点;T8 注册处) | 不同函数 |
| 全部 | locale 10 文件 | 各任务加各自 key,顺序执行无冲突 |

无任务间矛盾;与 Global Constraints 无冲突。上游方案 §7 各项与本任务映射: A1/A2/A4/D3→T1,A3→T2,B3+B2→T3,B1→T4,B4→T5,C1→T6,C5→T7,C3→T8,C2→T9,C4→T10,C6→T11,D2→T12;A5(设置搜索)按上游方案可后置,本轮不做(ledger 记录)。
