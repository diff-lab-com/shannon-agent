# 桌面端 UI 全页面审查与改进方案（对话页之外 · 第一轮）

- 日期：2026-09-26
- 版本：**v2.1**（基线重定：对话页第二轮 B0-B4 已合入 dev@7efc12bc（PR #122/#123/#124，96 文件/+9840 行），开工前对全部 P0 与 B0/B6 相关条目重验——见下方「基线重定」块）
- 状态：**已批准 · 实施中**（v2 评审通过；实施按 §9 批次顺序，每批独立 PR 目标 `dev`）
- 范围：`desktop/ui` **对话页之外**的全部页面、骨架与组件——应用骨架/导航/主题/i18n 基础设施、设置、使用统计、扩展（MCP/技能/Agent/数据源）、记忆、任务/OPC、Triage、TurnTimeline、QuickFix、Diff 评审、编辑器、终端、Welcome/迁移向导、共享 UI
- 方法：基于 dev@ddcddab1 的全量代码审查（约 5.6 万行 TS/TSX + 关键 Rust 命令交叉核对）。六个独立审查线程（骨架导航 / 设置+Usage / 扩展+记忆+技能 / 任务集群 / Triage+diff+时间线 / 编辑器+终端+Welcome+共享UI）并行推进；经初审 + Red Team 复审两轮，**6 条 P0 + 17 条 P1 已由主审逐条读码坐实**（含一处 node 实际复现），其余 P1/P2 实现前请先复测（约 30 秒/条）。
- 关联文档：
  - `docs/plans/2026-09-26-desktop-chat-ui-round2-design.md`（对话页第二轮；本文与其零重叠，对话页问题一律不重复收录）
  - `docs/plans/2026-09-26-desktop-ui-pages-review-redteam.md`（v2 修订依据：R1~R5 全部折入本文）
  - `docs/plans/2026-09-25-desktop-chat-ui-open-and-artifact-design.md`（对话页第一轮，已合入 PR #120）

---

## 0. TL;DR

对话页之外的 UI 面积是对话页的 3 倍以上，此前的审查投入却不成比例。本轮结论：**骨架与设计系统的底子是好的**（状态分片、主题管道、凭据处理、危险操作确认体系都在水准之上），但各功能页普遍存在同一批系统性缺口——**错误态缺失（失败渲染成空态）、乐观更新不回滚、提交无防重复、i18n 三处纪律缺口、全局键盘与浮层键盘互相打架**。其中 6 条 P0 里，4 条属于「评审/审批会静默写错数据」级别：OPC 审批传错 id 导致审批必败但 UI 报成功、diff 合并按内容匹配导致被拒绝的改动也被写入、二进制文件 diff 可把原文件清空、diff 键盘 Enter 在任意焦点下触发写盘。

改进方案分 7 批（B0 止血 → B1 骨架 → B2 设置 → B3 扩展/任务 → B4 评审线 → B5 编辑器/终端 → B6 i18n/a11y 收尾），合计约 **16~18.5 人日**，每批可独立成 PR。§8 六个决策点**已全部拍板**（均采纳原建议）；Red Team 复审的 4 处修复设计修订与决策落地项已折入本 v2（正文中以 **[R1-x]/[R4-x]** 标注）。**开工待最终批准。**

---

## 基线重定（v2.1，2026-09-26 开工前）

对话页第二轮方案在本文评审期间已实施并合入 dev（PR #122 会话管理打磨、#123 对话页第二轮 B0-B4、#124 chat-round2-followups）。对本文全部 P0 与受影响条目逐条重验于 dev@7efc12bc：

| 条目 | 重验结果 |
|---|---|
| P0-1/2/3/4/5/6 | **全部仍成立**（OPCTask `respondPermission(taskId,…)`、diff-merge `hunks.find`、`commands_files.rs:549` `unwrap_or_default`、useDiffKeyboard 无门控、Editor 无守卫且 `editorInitialPath` 仍不复位、`skill_installers.rs:91,237` 裸 join） |
| P1-1 setContextPanelOpen | ✅ **已被 #123 修复**（现为 `updateContextPanelOpen`，带持久化与 prev-updater）——从 B0-7 删除 |
| P1-2 模型切换 | 仍成立（Header 仍写 `model.name`，面板仍写 id） |
| P1-3 会话切换竞态 | 仍成立（#123 加了 `switchingSession` 骨架，但 `await` 后仍无条件落地，无过期响应守卫） |
| P1-4 sidebar-w / P1-6 Escape 冲突 | 仍成立（代码未动） |
| P1-5 i18n 缺键 | **306→约 11 键/locale**（#123-B4 已同步各语言包；en=zh-CN=3327，ja=3316）。B6-35 范围缩为「残余 ~11 键 ×8 locale + CI 比对脚本」 |
| 其余 P1/P2 | 所在文件多数未被 #122-124 触碰（settings/extensions/tasks/diff/editor 均不在其改动面）；**实现时以当前 dev 内容定位为准**，行号以本文为线索 |

---

## 1. 审查范围与页面清单

| 页面/区域 | 入口 | 主要文件（行数） |
|---|---|---|
| 应用骨架 | — | App.tsx、Layout.tsx、Header.tsx、Sidebar.tsx、SidebarSessions.tsx、CommandPalette.tsx、AppContext（741 行）、ThemeContext、hooks×18 |
| 设置 | Mod+6 | Settings.tsx + components/settings/**（约 6400 行） |
| 使用统计 | — | Usage.tsx（565）+ components/usage/** |
| 扩展中心 | — | Extensions.tsx + components/extensions/**（约 4900 行） |
| 记忆 | — | Memory.tsx + components/memory/**（约 1700 行） |
| 任务/OPC | — | Tasks.tsx、OPCTask.tsx、OPC.tsx + components/tasks/**（约 5900 行）+ components/opc/** |
| Triage/时间线/QuickFix | — | Triage.tsx（760）、TurnTimeline.tsx（438）、QuickFix.tsx + components/diff/**（约 1000 行） |
| 编辑器/终端 | Mod+5 | Editor.tsx + pages/editor/** + components/terminal/**、editor/**、lsp/** |
| Welcome/迁移 | 首启 | Welcome.tsx + pages/welcome/** + MigrationWizard.tsx |

对话页（Chat 消息流/输入区/Dock/产物，components/chat/** + artifact/**）按第二轮文档结论执行，本文不覆盖。

---

## 2. 总体评价

### 2.1 做得好的（保持，不要在修复中破坏）

1. **骨架层状态设计扎实**：AppContext 三个 memoized slice 让流式高频更新只波及 chat 消费者；per-session 流桶 + `visibleSessionIdRef` 投影系统性解决多会话 token 交错与 StrictMode 双提交；事件注册对 async unlisten 的 cancelled 竞态处理完整（`AppContext.tsx:494-672`）。
2. **凭据处理是教科书级**：webview 永不回读已存密钥——平台 token 走 draft-only 输入 + keyring 探测 + ✓ Set 徽标（`PlatformsCard.tsx:224-255`），provider 只回传 `has_api_key` 布尔，STT key 掩码占位。
3. **危险操作确认体系完整**：工厂重置/删除 provider/删除权限 profile/吊销设备/切换 provider 的缓存失效警告全部有统一 ConfirmDialog 且 busy 期间防双击；扩展侧 MCP/技能/Agent/数据源删除同样全覆盖。
4. **主题管道单一来源 + 防漂移**：themes.css/registry 脚本生成，类型级 tripwire 强制 registry 与 union 对齐，reduced-motion/透明度降级路径齐全。
5. **局部范本可推广**：MigrationWizard 的 requestIdRef 竞态守卫与 busy 门控、DreamPanel 的「申请 N 应用 M 上报」、terminal 事件的 base64 字节流与 dispose 卫生、`packageValidation.ts` 的 stdio 命令注入防护——这些应作为 B3~B5 批次修其它组件时的**参照实现**。

### 2.2 问题重心：不是缺功能，是「失败时的表现」

本轮约 100 条发现中，纯「功能缺失」类占比很小。反复出现的模式是：**操作失败后 UI 谎报成功、或把失败渲染成空态、或让用户以为写盘了实际没写（反之亦然）**。这类问题单个都是 P1，叠加起来的用户感受是「这个应用偶尔会骗人」，比缺一个页面伤害大得多。§7 把它们归拢为 6 个横断主题，修法是统一的。

---

## 3. P0 —— 会静默写错数据 / 数据丢失（6 条，全部已主审复核）

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| **P0-1** | **OPC 任务页审批把「任务 id」当「权限请求 id」发送，审批必然失败但 UI 报成功**：`respondPermission(taskId, …)` 三处传的都是 OPC 任务 id；context 签名与后端都以 `PermissionRequest.request_id` 为键，未知 id 直接 Err | `OPCTask.tsx:27,198,295`；`AppContext.tsx:391-397`；`commands_permissions.rs:149-151` | Human-in-the-Loop 审批整个链路失效：后端找不到请求 → 引擎侧工具调用永远等待、agent 卡死；前端却无条件 `toast.success('已批准执行')`（且未 await） |
| **P0-2** | **diff 合并按「内容相同」匹配 hunk，重复出现的相同改动会共用第一个 hunk 的决策**；被拒绝的 hunk 照样写入文件（已用 node 复现：两处 `dup→DUP`，一收一拒，落盘两处都改） | `diff-merge.ts:212-233`（mergeFile 的内容查找；hunk id 本身是 `oldStart-oldEnd-newStart-newEnd` 的**位置寻址**格式，无 id 碰撞——Red Team 精确化，见 R3-5） | 评审的核心承诺「只应用接受的 hunk」失效；模板/样板代码类文件上 reject 也被落盘，**静默写错文件** |
| **P0-3** | **二进制文件 diff 渲染成「整文件删除」，Apply 把原文件清空**：Rust 侧 `read_to_string(...).unwrap_or_default()` 对二进制返回空串 → 全行 removed hunk → accept 后 merge 结果为空串 → `fs::write` 无防呆直接落盘 | `commands_files.rs:503,232`；`DiffViewer.tsx` 无二进制护栏 | 工作区里有图片/数据库等非 UTF-8 文件时，打开 diff（显示整文件删除）→「全部接受 + Apply」= 文件被清空，数据丢失 |
| **P0-4** | **diff 快捷键在整个 document 上生效且不区分焦点**：`a/r/u/Enter` 只过滤 INPUT/TEXTAREA/contentEditable；焦点在任意按钮上按 Enter 想「点按钮」，实际被 `preventDefault` 吞掉并直接触发 Apply 写盘；无 applying 防抖可双击两次 | `useDiffKeyboard.ts:50-96`；`DiffReviewBody.tsx:138-143`（`enabled` 只看有无 diff，不看焦点）；`handleApply` 自身亦无 applying 重入防护（`DiffReviewBody.tsx:118-128`），连按两次在状态刷新前双次写盘（Red Team 补充） | 模态内焦点落在「取消」按钮上按 Enter → 文件被写盘、弹窗关闭；RightDock 非模态场景下 Dock 开着即可误触 |
| **P0-5** | **编辑器无未保存守卫**：`draft` 只在组件 state；Escape/关闭按钮/加载另一文件三条路径都直接丢弃，无任何确认 | `Editor.tsx:107-116`（loadPath 直接重置 draft）；`Chat.tsx:271`（onClose 直接卸载）；`InlinePanelModal.tsx:51-56` | 用户改完代码按 Esc，全部修改无提示丢失——字面意义的数据丢失 |
| **P0-6** | **技能安装链路对 catalog 提供的名称零校验，后端用原名直接 `join` 技能根目录**：`Path::join` 遇绝对路径或 `..` 即逃逸技能根；对比 candidate 审批路径后端有 `slugify`（`commands_skill_candidates.rs:229`），安装链路漏了同样防护 | `skill_installers.rs:91,237`（`shannon_skills_root().join(&self.plugin_name)`）；`Skills.tsx:119-126`（前端也无名称格式校验） | 技能目录来自 GitHub 上游 HTTP catalog；被污染的条目可把 SKILL.md 写到 `~/.shannon/skills/` 之外的任意路径（任意文件写入）。修复成本极低，必须止血 |

---

## 4. P1 —— 显著正确性/UX 缺陷（按领域分组，已主审复核的标注 ✅）

### 4.1 骨架与导航

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| P1-1 ✅ | **Dock 关闭按钮与 Ctrl+\ 永远无法关闭面板**：context 的 `setContextPanelOpen` 被接线成 `openContextPanel`（忽略入参恒置 true），接口类型却是 `(open: boolean) => void`，编译期不报错 | `AppContext.tsx:217-219,713`；消费端 `Chat.tsx:88,283`、`RightDock.tsx:399` | 三处 `setContextPanelOpen(false)` 全部静默变成「打开」；Dock 头部关闭钮点了没反应，Ctrl+\ 只能开不能关，只有 Header 的 toggle 还能收起 |
| P1-2 ✅ | **两个模型切换入口写入的值空间互相矛盾**：Header 写 display_name（注释声称「config 的 model 键存 name」），命令面板写 id 且不写 provider；后端把该值当 model id 钉进 providers.toml 的 active target | `Header.tsx:92-104`；`CommandPalette.tsx:100-106`；`commands_config.rs:357-365`（`set_active(&provider, &new_model_id)`）；`commands_chat.rs:96-105`（id 与 display_name 是两个字段）；**Red Team 源码证实**：`provider_resolver.rs:60` 将存储字符串原样透传为 API model 参数，`model_registry.rs:790-791` 证实 display_name≠id → Header 路径对 name≠id 模型必然失效 | 两条路径必有一条错：name≠id 的模型切换后发请求报错；面板路径在目标 provider ≠ 当前激活 provider 时把别家模型 id 钉错 provider。同一操作两处行为不一致。存量用户 providers.toml 已写入 display_name，需迁移（见 B1-8） |
| P1-3 ✅ | **快速连续切换会话存在响应乱序竞态**：`switchToSession` 无序号守卫，await 期间再点另一个会话，后返回的旧响应覆盖 `currentSessionId/messages` | `AppContext.tsx:350-368`；调用端 `SidebarSessions.tsx:494-499`、`CommandPalette.tsx:96-97` 均 `void` 不等待 | 用户停在没点过的会话里继续打字，消息发进错误会话（比第二轮文档 P2-3「无 loading 态」更严重的同点变体） |
| P1-4 ✅ | **窗口跨过 767px 断点后 `--sidebar-w` 永不复原**：mobileMode 置 true 时 effect 写 0px；回到桌面时 effect 提前 return 不写回，Sidebar 的写回 effect 只依赖 `[width]` 不触发 | `Layout.tsx:86-89`；`Sidebar.tsx:168-170` | 拖窄再拖宽（Windows 吸附分屏很常见）后，280px 固定侧栏压住正文与页头页脚，直到手动拖一次分隔条或重启 |
| P1-5 ✅ | **8 个非中英语言包各缺 306 个键，且 `useT` 没有 en 兜底**——缺的是高频骨架串：`nav.search`、`nav.automation`、`sidebar.sessions.*`×19、`extensions.pending`×17、`memory.dream`×41、`chat.artifact`×11、`chat.dock`×10 | 实测：en/zh-CN 各 3286 键，ja/fr/de/ko/pt-BR/ru/es/zh-TW 各 3005；`i18n/index.tsx:130-136`（useT 直接 `formatMessage` 无 fallback） | 日/韩/德等用户界面直接渲染 `sidebar.sessions.search.placeholder` 这类原始 key；react-intl 每键打 console 错误。第二轮文档 P2 只点名了个例，此处是量化全貌 |
| P1-6 ✅ | **全局 Escape「取消查询」与浮层 Escape 打架**：全局 handler 只过滤输入类元素；DropdownMenu 的 Escape 分支 `preventDefault` 但不 `stopPropagation`，事件照样到 window | `useKeyboardShortcuts.ts:39-46`；`dropdown-menu.tsx:75-85` | 流式进行中，按 Escape 关会话行菜单/命令面板 = 同时把 query 取消（叠加对话页 P1-2 幽灵气泡，等于丢失整段回答）。Base UI Dialog 路径待复测，DropdownMenu 路径代码可证 |
| P1-7 | **ErrorBoundary 路由切换后不复位**：一个页面崩溃后，fallback 一直盖住后续导航的所有页面 | `ErrorBoundary.tsx:20-42`；`Layout.tsx:153` 无 `key` | Chat 崩溃 → 去 /settings 看到的仍是 Chat 的报错，需再点一次「重试」 |
| P1-8 | **顶层数字格式化/侧栏新建按钮/路由焦点**：侧栏「新建对话」只建会话不跳 /chat（与 Mod+N 行为不一致）；路由切换后焦点不迁移、无读屏公告 | `Sidebar.tsx:227-231` vs `useKeyboardShortcuts.ts:19-26`；`Layout.tsx:141-154` | 在 /settings 点新建会话像按钮坏了；读屏用户不知页面已切换 |

### 4.2 设置与使用统计

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| P1-9 ✅ | **会话预算保存失败静默 + 输入框跨会话残留**：try/finally 无 catch（后端对 ≤0 返回 Err）；新会话预算为 null 时不重置输入框 | `CurrentSessionCostPanel.tsx:41-43,53-62,120` | 输入 0 保存 → 转圈结束无任何反馈；从有预算会话 A 切到 B，输入框还显示 A 的值，点保存把 A 的上限写进 B |
| P1-10 | **设置页乐观更新不回滚（系统性）**：审批模式/高级开关/性能策略全部「先 setState 再发 IPC」，失败只 toast 不还原；AdvancedSettings 六个开关甚至不从刷新后的 config 回同步 | `GeneralSettings.tsx:96-105`；`AdvancedSettings.tsx:23-28,94-101`；`ModelsSettings.tsx:43-46` | **审批模式是安全开关**——用户以为切到 confirm 实际还是 auto_edit（或反之），直到某次 refreshConfig 才被纠正 |
| P1-11 | **参数滑块每拖一格写一次盘（无防抖），失败仅 console.warn** | `ParameterSlider.tsx:22-30` | temperature 0.7→0 触发几十次写盘 IPC；写失败后 UI 与磁盘静默分叉 |
| P1-12 | **权限 profile 重命名留旧文件**：前端忽略 originalName，后端文档明确要求 delete+save；重命名激活中的 profile 后 `active_permission_profile` 仍指旧名 | `PermissionsSettings.tsx:227-243` vs `automation_commands.rs:353-355` | 安全相关配置静默分叉：用户以为在改激活配置，实际在改副本 |
| P1-13 | **权限页加载失败渲染成「空列表」**：catch 里静默置空，内置三档也来自同一响应 | `PermissionsSettings.tsx:148-160` | IPC 失败时安全配置界面显示「系统没有任何权限档位」，误导性强 |
| P1-14 | **Usage 按天聚合用 UTC 而非本地时区，且与同页本地时区列自相矛盾** | `commands_usage.rs:268-272`（`DateTime::<Utc>`）vs `Usage.tsx:218-219`（fmtDay 用本地） | UTC+8 用户 0:00–8:00 的用量记到「昨天」，按日对账永远对不齐 |
| P1-15 ✅ | **Webhook 的 secret 与自定义模板 body 没有输入框；切 Custom 预设保存会清空已存模板** | `NotificationsSettings.tsx:75-76,130-139`（state 存在但 JSX 无对应控件） | HMAC 密钥从 UI 永远设不了；「自定义」保存即覆盖旧模板（不可逆数据丢失） |
| P1-16 | **Remotes「默认目标」从不回读**（代码是显式 no-op）；**VoiceLocal 语言框每击键保存一次 + 弹一个 toast** | `RemotesSettings.tsx:50-51`；`VoiceLocalSettings.tsx:309-314,125-137` | 用户反复重设默认远端；输入 "zh" 产生 2 次写盘 + 2 个 toast，中途的 "z" 被持久化 |

### 4.3 扩展 / 记忆 / 技能

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| P1-17 | **列表加载「无错误态」（6 处同模式）**：`.then().finally()` 无 `.catch`，失败既 unhandled rejection 又被渲染成「暂无xx」空态 | `McpServers.tsx:87-91`、`DataSources.tsx:75-88`、`DataSourcesQuery.tsx:29-33`、`Skills.tsx:79-83`、`Agents.tsx:66-70`、`RoutineTemplatesBrowser.tsx:32-48` | 已装有资产的用户在 IPC 失败时被告知「什么都没装」，且控制台报未捕获异常 |
| P1-18 ✅ | **技能目录「显示更多」在搜索时完全失效**：`filtered` 未 memo → 每次渲染新数组 → `usePagedVisible` 的 effect 每次把 visible 重置回 24 | `Skills.tsx:155-164`；`usePagedVisible.ts:15-17` | 搜索（最需要翻页的场景）下点「显示更多」永远弹回 24 条 |
| P1-19 ✅ | **数据源安装表单把 placeholder 示例值当真实初始值** | `DataSources.tsx:102-105`；catalog 示例 `/home/user/MyVault`、`you@example.com`（`data_source_catalog.rs:117,142,158`） | 用户直接点保存，示例串通过必填校验写入配置，适配器装好后查询必失败且难排查 |
| P1-20 | **编辑记忆时 project 字段可改但保存被静默丢弃**：update 签名根本不含 project | `MemoryEditor.tsx:99-107`；`MemoryPanel.tsx:145-151`；`tauri-api.ts:2005-2012` | 用户改完看到「已更新」，refetch 后悄悄变回原值——输入丢失且无提示 |
| P1-21 ✅ | **精选页安装成功后不刷新个人 tab/icon 行**：事件只监听不派发 | `Featured.tsx:93-124` vs 同文件 49-62（消费 `shannon:extension-installed`） | 一键安装成功后切到「个人」看不到新条目，像「装了没生效」 |
| P1-22 | **SKILL.md/agent.md 用字符串插值生成，catalog description 可注入 YAML frontmatter** | `Skills.tsx:121`；`Agents.tsx:84` | 上游 description 含换行/`---` 时可注入额外 frontmatter 字段（model、tools），构成对下游 agent 的提示注入面 |

### 4.4 任务 / OPC

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| P1-23 | **「Run Now」是固定 1.5 秒假 spinner，失败也显示「Success」，定时器不清理** | `Tasks.tsx:202-218`；`TaskCard.tsx:83-87` | 触发失败时按钮绿色「Success」与错误 toast 同屏自相矛盾 |
| P1-24 | **批量（best-of-N）/新建任务/新建例程表单无提交中防重复**：onSubmit 未 await、按钮无 busy | `BatchForm.tsx:35-43,111-120`；`NewTaskForm.tsx:41-44,105-111`；`ScheduleForm.tsx:119-135,511-517` | 双击 = 双批 N 个并行 agent 会话真金白银的花费；例程会按计划反复执行 |
| P1-25 ✅ | **任务编辑绕过 hook，保存后全页陈旧**：`onUpdated` 回调被写成空操作（注释声称 hook 自动刷新，实际 hook 无事件订阅）；OPC「实时」agents 只在启动时取一次 | `DependsOnEditor.tsx:52`、`OffpeakWindowEditor.tsx:93`、`Tasks.tsx:413`；`scheduled-tasks.ts`（全文无 useTauriEvent）；`AppContext.tsx:646,692`（BACKGROUND_TASKS_UPDATED 只刷 backgroundTasks 不刷 agents） | 改完依赖/错峰窗口，抽屉/DAG/日历全是旧值；OPC 负载/工作流图需重启应用才更新 |
| P1-26 ✅ | **日历「选中日的任务」实际渲染全局列表**，与所选日期完全无关 | `TaskCalendarView.tsx:107-142`（`filteredTasks.slice(0,5)` 无日期过滤） | 点任何一天看到的都是同样 5 条任务 |
| P1-27 | **多个动态 i18n key 不存在，状态 chip 渲染原始 key** | `OpcAnalyticsDashboard.tsx:211,237`（`status.medium/todo/...` 均缺）；`OPCTask.tsx:17,93,275`（t() helper 丢弃 defaultMessage） | 有 cancelled/todo/medium 任务时界面出现 `status.medium` 字面量 |

### 4.5 Triage / Diff 评审 / 时间线

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| P1-28 ✅ | **Triage 分组模式下键盘焦点错乱 + 操作错位**：focused 用桶内索引 j，键盘用扁平索引 → 多张卡同时亮环，Enter/a 归档的是另一张卡（归档不可逆） | `Triage.tsx:643-648` vs `:453-470` | 分组开启时 j/k 导航完全不可信 |
| P1-29 ✅ | **Triage Enter 劫持**：焦点在卡片内按钮上按 Enter，被列表 handler 吞掉变成「标记已读」 | `Triage.tsx:448-464`（BUTTON 不在过滤名单） | 「查看会话」按钮点不开，item 被误标读 |
| P1-30 | **批量操作部分失败静默 + 归档不可撤销**：`Promise.allSettled` 后全失败也无提示、选择无条件清空；UI 无「取消归档」入口 | `Triage.tsx:379-391,267-277,397-400` | 批量归档 50 条后端全挂，界面零反馈；全选+归档两下点击即把整箱条目移出视图且无法从 UI 恢复 |
| P1-31 ✅ | **Apply 无并发冲突检测**：diff 读取与保存之间文件被改（agent 继续跑/多窗口）会静默覆盖 | `DiffReviewBody.tsx:62,123-124`；`commands_files.rs:232`（write 无 mtime/hash 校验） | 评审期间 agent 同会话继续改文件，Apply 用陈旧内容合并后覆盖磁盘上的新改动 |
| P1-32 | **大 diff 无行数上限、无虚拟化，每次交互重复全量 diff 计算**；`FileDiffList` 每文件每次渲染调 3 次 `computeHunks` | `DiffViewer.tsx:124-125,172-223`；`FileDiffList.tsx:91,132,134-135`；`DiffDialogMulti.tsx:103-120` | 50 个大文件的评审每点一次 accept 卡一次；单文件数万行主线程冻结 |
| P1-33 | **j/k hunk 导航完全不可见**：currentHunkId 被丢弃（`void`），无滚动无高亮，a/r/u 作用于看不见的 hunk | `DiffReviewBody.tsx:144`；`useDiffKeyboard.ts:99-104` | 键盘评审功能名存实亡 |

### 4.6 编辑器 / 终端 / Welcome

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| P1-34 ✅ | **CodeMirror 硬编码 `theme="light"`**：12 套主题里所有暗色用户在 90vh 编辑器弹窗里看到纯白编辑区（终端面板反而做了主题跟随） | `CodeEditor.tsx:150`；对比 `TerminalPanel.tsx:217-222` | 全应用视觉割裂，暗色主题下刺眼 |
| P1-35 | **LSP quick fix 应用后编辑器不刷新**：磁盘已被 code action 改写，编辑器仍持旧 draft；一旦「编辑→保存」，**修复被旧内容整体覆盖**（与 P0-5 同链路放大） | `QuickFixDrawer.tsx:25-27`（onApplied 空实现）；`Editor.tsx:171,273-274` | 应用修复→关抽屉→随手保存 = 修复丢失 |
| P1-36 | **终端新建 tab 使用过期主题**：`ensureTerm` 空依赖闭包捕获首次 resolvedTheme | `TerminalPanel.tsx:151-164` | 浅色→切暗色→新建终端是浅色且不再被纠正 |
| P1-37 | **MigrationWizard 扫描失败伪装成「没有可导入内容」** | `MigrationWizard.tsx:113-118`（catch 里置空并进 review） | 首次运行用户遇 IPC 错误被误导为「Claude Code 里没东西可迁」，放弃迁移 |
| P1-38 | **Welcome 完成页宣称「已启用 N 个工具」，但该状态从未持久化、无 UI 可改** | `Welcome.tsx:50,73,160-165`；`DoneStep.tsx:80-83` | 摘要数字是虚构的；用户以为工具预设已生效 |

---

## 5. P2 —— 打磨项（按领域归并，约 60 条；**本节即 P2 权威基线**，实施时以此为准，不再另行附审查线程原始输出）

**设置/Usage**：硬编码英文（`Usage.tsx:206` 'Reqs'、BarChart aria 'Bar chart'/'Total'、后端 `SCHEDULED_LABEL` 直出中文界面、toast 标题英文串遍布 Connections/Remotes）；数字未全走 Intl（`toFixed`×3 处）；Switch 大量无可访问名称（AdvancedSettings 全部）；表单 label 未关联、错误无 role=alert；Usage tablist 不符合 tabs 模式、图表仅鼠标可读；三个编辑弹窗 Esc/遮罩即丢脏状态（AddProvider/Permissions/Remotes）；KIND_INFO 双份拷贝漂移（AddProviderModal 缺 gemini，编辑时下拉错位）；「查看日志」是假日志且版本号写死 v0.1.0（实际 0.11.0）；删已下载模型/清 Webhook 无确认；STT key 用 `'***'` 哨兵无法清除；Mobile 派发 URL 恒 http:// 忽略已开 TLS。

**扩展/记忆**：MCP 卸载失败复用「安装失败」文案；记忆删除确认无 busy（双击弹假「未找到」错误）；待审候选 hook 吞错误（失败与「没有待审」不可区分）；候选「拒绝」无确认无撤销；已安装页搜索无匹配误报「尚未安装任何扩展」；单一 busyId 并发安装互相清状态；JSON 粘贴导入预览隐藏 args/env、无 command 的远程服务器被静默丢弃；tab 条无 aria-controls/方向键。

**任务/OPC**：HistoryView 快速展开两行时详情串行；CancelTaskModal 双击报「Task is not running」；useTaskStreaming steps 无限追加不按 run_id 分代（当前生产未挂载，接入即暴露）；OffpeakWindowEditor 对 policy=null 的例程套危险兜底（max_retries:0、auto_archive:true）；TaskCard/日历格/DAG 节点键盘不可达；任务状态变更无 aria-live；ScheduleForm/HookRoutine/ScheduleTemplates/ResultRouting 大量硬编码英文（模板预填 prompt 亦英文）；NewGoalDialog 启动失败仍关闭并清空表单。

**Triage/diff**：筛选/排序/分组不持久化；IPC 失败渲染成「空收件箱」（error 未消费）；时间格式化用系统 locale 而非应用 locale；TurnTimeline 无分页、未知 reason 渲染原始 key 且非 completed 一律标红；mergeFile 恒补尾随换行（污染 diff 语义）；无词级高亮、空白不可见；超长单行无截断（minified JS 冻结）；diff 行对读屏无语义、决策变更无播报；多文件评审失败文件伪装「未评审」、部分成功不报成功数。

**编辑器/终端/共享**：终端 spawn 失败 unhandled rejection；无多行粘贴确认；终端键盘陷阱（唯一出口 Ctrl+`，无屏内提示）；Editor 诊断请求无竞态守卫；`editorInitialPath` 永不复位（之后从任何入口开编辑器都预载旧文件）；英文 locale 诊断计数显示双份数字（「3 3 diagnostics」）；「refresh」图标名渲染成字面文本（缺 material-symbols class）；CodeBlock 复制失败静默；welcomeExamples 四条 prompt 硬编码英文；编辑器无大文件/二进制防护（与 P0-3 同根源的后端缺口）；FileRefChip/LinkContextMenu 自绘 menu 无菜单键盘语义。

**骨架**：Header 手写模型 listbox 的 `aria-selected` 恒 false（id 比对显示名）+ Enter 双重切换竞态；快捷键帮助文档与实际绑定不符（Ctrl+/ 不存在、Mod+5 离开 /chat 是哑弹）；updater 插件占位符公钥 + 第三方 endpoint 半启用状态（**已拍板决策 6：摘除，落地于 B1-15**）；顶层 Suspense 包住整树（冷加载整骨架闪没）；footer token/费用被任意后台会话 usage 覆盖；footer 金额 `$…toFixed(4)` 绕过 Intl（`Layout.tsx:168`，并入 B6 清单）；theme='system' 依赖同值 setState 可能不生效（待复测）；权限弹窗 Esc 一次按键「拒绝权限+取消查询」双触发（待复测）；触屏长按后 `suppressClickRef` 悬挂吞掉下次点击；`role="presentation"` 抹掉折叠按钮语义；CSP `frame-src https:` 放行任意站 + assetProtocol scope 覆盖 `$HOME/**`（面偏宽）——收敛需与网页 tab/文件引用功能共同决策，**明确 deferred，不作批次任务**；`duration` 令牌 `:root` 与 `@theme` 双定义打架。

---

## 6. 横断性主题（问题的真正结构）

单个问题易修，模式会再生。以下 6 个主题解释了本轮大部分发现，改进方案按主题配统一修法：

| 主题 | 覆盖的发现 | 统一修法 |
|---|---|---|
| **T1 错误态系统性缺失** | P1-13/17/30/37 等 ≥10 处：`.catch(()=>置空)`、`.finally` 无 `.catch`、error 未消费 → 失败一律渲染成「空态」 | 约定三态强制化：列表/面板 hook 统一返回 `{data, loading, error}`，渲染层禁止「catch 即置空」；仓库已有 ErrorState 组件，接上即可。lint 层可加 eslint 规则禁 `.catch(() => set...([]))` 模式 |
| **T2 乐观更新无回滚** | P1-10/11/24、Run Now 假 spinner、未 await 的 toast、NewGoalDialog 失败清表单 | 统一 `useAsyncAction` 包装：busy 态 + 失败回滚（或 catch 中 `refreshConfig()` 回读）+ 禁止在 await 前弹 success toast。审批模式等安全开关禁止乐观更新 |
| **T3 提交/确认无防重复** | P1-24、记忆删除确认、CancelTaskModal、单 busyId 并发互清、diff Apply 无 applying 防抖 | ConfirmDialog 全量接 `busy` prop（McpServers 已有正确示范）；所有 async 提交按钮统一 busy 禁用；共享 busy 用 Set 不用单值 |
| **T4 i18n 三处纪律缺口** | P1-5（306×8 缺键 + useT 无兜底）+ 各页硬编码清单（§5 涉及 6 个领域）+ 动态 key 生成（`status.${...}`） | ① `useT` 补 en 兜底（一行，立刻止血）；② CI 加键比对脚本（en 基线 vs 各 locale，缺失即红）；③ 硬编码清单化修复；④ 禁止拼接 key，动态部分进 values |
| **T5 全局键盘与组件键盘互相打架** | P0-4、P1-6/28/29、Header Enter 双触发、权限弹窗 Esc 双触发 | 立两条规矩：① 浮层/菜单的 Escape/方向键处理必须 `stopPropagation()`；② 全局/window 级快捷键只对「焦点确实位于其管辖区域」生效（`containerRef.contains(target)` 门控），BUTTON/role=button 一律放行 |
| **T6 后端数据边界缺失** | P0-3、编辑器无大文件防护、`get_file_diff` 无上限、错误未结构化（英文子串耦合已在对话页 P2-24 记录） | 文件读取类命令统一加：字节上限、二进制嗅探（复用 `read_text_file` 的 `sniffs_as_binary`）、结构化错误 `{code, message}`；写入类命令加 mtime 冲突检测 |

---

## 7. 改进方案（7 批，每批可独立成 PR）

### B0 止血包（P0 全部 + 顺带的一行级修复）——约 2.5~3 人日

1. **P0-1**：OPCTask 改传 `permissionRequest.request_id`；`respondPermission` await 后再 toast，失败走 toastError。**[R1-1]** 确认弹窗必须展示待审请求的归属（`permissionRequest.session_id` → 会话标题），与任务运行会话不匹配时禁用并提示——否则修完仍有误批风险（主窗口对权限事件**全收**，`windowSession.ts:52-58`；聊天会话的待审请求会被当成 OPC 任务批准）。实施时确认跨窗口 pending 清理语义：session 窗口与主窗口可能同收同一请求，后端 respond 成功后需有全局事件清掉其它窗口的弹层。
2. **P0-2**：**[R3-5 根因精确化]** hunk id 本身是位置寻址格式（`oldStart-oldEnd-newStart-newEnd`，`diff-merge.ts:151`），无 id 碰撞；bug 仅在 mergeFile 的**内容查找**（`:212-218`）。修法：mergeFile 复用自身第 180 行已算出的 hunks，与 lines 段按序 1:1 对应（段序即 hunk 序），删除 `hunks.find(内容相等)`——无需动 id 方案，比 v1 描述更简单。顺带修尾随换行语义（按 old/new 是否以 `\n` 结尾决定）。回归测试：「相同内容两处、一收一拒」+ **相邻同内容双 hunk** 边界。
3. **P0-3**：后端 `get_file_diff` 对二进制返回结构化错误（复用 `sniffs_as_binary`，`commands_files.rs:268` 已存在）；前端对「纯删除且原文件非空」的 diff 显示警告并禁用 Apply；`save_text_file` 落盘前比对 fetch 时记录的 mtime——**[Red Team 确认]** 以 `Option<String> expected_mtime` 追加参数即可，Tauri 缺省参数安全、非破坏性，既有调用方不受影响（与 P1-31 同一机制，B4 复用）。
4. **P0-4**：useDiffKeyboard 加焦点门控（仅 `containerRef.contains(target)` 生效）+ BUTTON/role=button 放行 + applying 防抖。**[R4-1]** `handleApply` 自身补 `applying` 重入早退（不只靠键盘层），测试覆盖键盘连按两次路径。
5. **P0-5**：编辑器 `draft !== file.content` 时拦截关闭/换文件，ConfirmDialog 确认丢弃；`onClose` 里复位 `editorInitialPath`（P2 顺带）。
6. **P0-6**：技能/Agent 安装名称前后端双重校验（前端白名单 `^[a-z0-9][a-z0-9._-]*$`，后端 join 前 slugify + 拒绝路径分隔符，对齐 candidate 路径）；agent 侧同构问题已坐实（`agent_installers.rs:75,226`），一并修。顺带修 P1-22 的 frontmatter 注入（description 单行折叠或后端结构化生成）。
7. **一行级顺带**：~~P1-1 `setContextPanelOpen` 接线修正~~（**v2.1：已被 #123 修复，删除**）；P1-23 删 Run Now 假 spinner 改真实 pending；记忆删除确认传 `busy`。

### B1 骨架与导航正确性——约 2.5 人日（含决策 6 落地与存量迁移）

8. **P1-2**：模型切换统一写 `m.id`（**已拍板决策 1，Red Team 源码证实**：`provider_resolver.rs:60` 原样透传为 API model 参数 + `model_registry.rs:790-791` display_name≠id——Header 写 name 的路径对 name≠id 模型必然失效）。**[R1-4]** 补存量迁移：后端在读取 active_target 或 `configure('model')` 入口做一次归一化（精确匹配 id 原样；匹配 display_name/aliases 则改写为 id）——否则已用 Header 切过模型的老用户 providers.toml 里的 display_name 不会被纠正，发送持续失败；同步更新 Header 的「U2」注释（旧「config 存 name」约定被推翻）。Header 的 aria-selected 基准一并修正。
9. **P1-3**：switchToSession 加单调序号守卫，过期响应丢弃。
10. **P1-4**：**[R1-2]** 「mobileMode→false 分支恢复写入」不可实施——width 状态在 Sidebar 内、Layout 拿不到（已证实 Sidebar 单实例跨断点不重挂载，`Layout.tsx:42,137`）。改为把 `--sidebar-w` 写权统一收到 Layout：Sidebar 经 SidebarContext/回调上报 width，Layout 唯一写入（mobileMode 分支写 0px、桌面分支写上报值），顺带消除「两个 effect 各自写同一 CSS 变量」的所有权模糊。
11. **P1-6 + T5**：DropdownMenu/Base UI 浮层 Escape 加 `stopPropagation`；全局 escape 仅在无浮层打开时生效。
12. **P1-7**：`<ErrorBoundary key={location.pathname}>`。
13. **P1-8**：侧栏「新建对话」补 navigate('/chat')；路由切换焦点移到页面标题 + aria-live 公告。
14. **P1-5（止血部分）**：**[R1-3]** 不修 useT——覆盖不了 CommandPalette、Layout footer、DoneStep 等直接调 `intl.formatMessage` 的调用点；改在 `IntlProvider` 层合并：`messages={useMemo(() => ({ ...MESSAGES.en, ...MESSAGES[locale] }), [locale])}`，一行覆盖全部 formatMessage 路径并消除 react-intl 缺键 console 错误（合并对象按 locale memoize，避免子树重刷）；`messageFor`（provider 外独立路径）单独做同样合并。
15. **决策 6 落地（已拍板：摘除）**：编辑 `desktop/tauri.conf.json` 移除 updater 插件配置（占位符公钥 + 第三方 endpoint 的半启用状态既不可用又埋雷），跑 ACL 覆盖测试确认无连带；发布流水线就绪后再回填。
16. 顺带：顶层 Suspense 下沉到 Outlet 层；快捷键帮助文档删/实现 Ctrl+/；footer usage 加 `visibleKey` 过滤（比照 QUERY_TEXT）；CSP/asset scope 收敛**明确 deferred**（见 §5 注），不作批内任务。

### B2 设置与 Usage 数据正确性——约 2.5 人日

16. **P1-9**：预算保存补 catch + toast；`b == null` 时清空输入框；成功反馈。
17. **P1-10/11**：审批模式改「await 成功后再更新 UI」（安全开关禁止乐观）；其余开关失败回滚或 `refreshConfig()` 回读；AdvancedSettings 六开关补 config→state 同步 effect（照抄 dream 的写法）+ 工厂重置后 refreshConfig；ParameterSlider 改 onChange 本地、onPointerUp/防抖落盘。
18. **P1-12/13**：profile 重命名走 delete+save 序列（激活项先 deactivate）；权限页失败渲染 ErrorState + 重试。
19. **P1-14**：day_label 改本地时区（chrono `Local`），Rust 侧聚合单测同步更新。
20. **P1-15/16**：补 secret（type=password）与 custom body 文本域，Custom 未填时阻止保存；Remotes 回读默认目标（**已拍板决策 2**；优先在 `remoteListTargets` 响应里带出，省一次新命令与 ACL 工作）；VoiceLocal 改失焦/防抖保存 + 去每击键 toast。
21. 顺带：「查看日志」改「打开日志目录」+ `getVersion()`；Mobile 派发 URL 按 TLS 拼协议；KIND_INFO 合一；三个编辑弹窗 dirty 时禁 Esc/遮罩关闭。

### B3 扩展 / 记忆 / 任务页错误态与竞态——约 3 人日（含决策 3 后端子任务）

22. **P1-17 + T1**：6 处列表补 `.catch` + error 态渲染（区分「空」与「失败」）；`usePendingSkillCandidates` 暴露 error。
23. **P1-18/19/20/21**：`filtered` memo 化（或 usePagedVisible 依赖 length）；数据源初始值置空；memory project 编辑态置只读或后端支持（决策点 3）；Featured 安装成功派发既有事件。
24. **P1-24/25/26/27**：三个表单接 useAsyncAction（busy 防重复）；`useScheduledTasks`/`useTaskExecutions` 挂更新事件或编辑器改走 hook.update；BACKGROUND_TASKS_UPDATED 同时 refreshAgents；日历按 due_date 过滤 + 空态；补 `status.*` 缺失键 + t() helper 支持 defaultMessage。**[决策 3-A 落地]** 后端子任务：`commands_memory.rs` 的 update_memory 支持 project 字段 + `tauri-api.ts` 同步 + 单测（+0.5 人日），记忆编辑器解除 project 只读限制。
25. 顺带：HistoryView 详情加 cancelled 守卫；OffpeakWindowEditor 兜底值对齐 `ScheduleForm.DEFAULT_POLICY`；NewGoalDialog 失败保留表单；MCP 卸载文案独立键；已安装页区分「无匹配」。

### B4 Triage / Diff 评审线加固——约 2 人日

26. **P1-28/29/30**：分组模式用全局扁平索引；Enter 处理加 BUTTON 过滤/焦点门控；批量操作统计 rejected 并 toast、全部成功才清选择；归档加 Undo toast（调 `update_inbox_item_status(pending)`）或批量确认。
27. **P1-31/32/33**：Apply 前 mtime 冲突检测（与 B0-3 同一后端机制；`Option<String> expected_mtime` 非破坏性参数，签名/semver 顾虑已被 Red Team 排除）；FileDiffList 的 fileStatus/计数 memo 化；渲染行数上限 + 超限降级提示；j/k 映射到 hunk 行高亮 + 滚动定位（做不到就先撤 a/r/u 直到有可视焦点）。
28. 顺带：Triage 筛选状态入 searchParams；error 接 ErrorState；未知 reason 显示原文 + 中性配色；时间格式化传应用 locale。

### B5 编辑器 / 终端 / Welcome——约 1.5 人日

29. **P1-34**：CodeMirror 主题按 `data-theme` 选择（复用 xtermTheme 思路读 CSS 变量或引入 oneDark 映射表）。
30. **P1-35**：`onApplied` 触发 `loadPath(file.path)` 重读。
31. **P1-36**：`ensureTerm` 创建实例后立即用 ref 读当前主题。
32. **P1-37**：Migration 扫描 catch 分支渲染 ErrorState + 重试（照抄其内部 requestIdRef 范本）。
33. **P1-38（已拍板决策 4 = B）**：完成页摘要改文案「推荐工具 N 个（可在设置中启用）」，不接线后端；顺带清理 ToolsStep 死代码注释。
34. 顺带：Editor 诊断请求加 requestIdRef；终端 spawnTab 补 catch + toast、多行粘贴确认；英文 locale 诊断双数字；refresh 图标 class；CodeBlock 复制失败反馈；FileRefChip 菜单改 Base UI Menu 或补键盘循环。

### B6 i18n + a11y 全仓清尾——约 2.5 人日

35. **P1-5（主体，已拍板决策 5 = 混合；v2.1 缩量：#123-B4 已同步语言包，残余约 11 键/locale）**：CI 键比对脚本（en 基线 vs 各 locale，缺失即红，防新增缺口）+ 残余缺键补齐（8 locale × ~11 键）+ §10 所列 9 处「非 en/zh 语言包整键未翻译」复查；en 兜底已由 B1-14 在 provider 层先行落地。
36. §5 各领域硬编码英文清单化修复（涉及设置/扩展/任务/时间线/diff/编辑器/骨架 7 个领域，按清单逐条过），含 Layout footer 的 `$…toFixed(4)`（`Layout.tsx:168`，改走 Intl 货币格式化）。
37. a11y 批量：Switch 补 aria-label（复用既有 label key）；label htmlFor 关联 + 错误 role=alert + aria-invalid；Usage tablist 改 aria-pressed 按钮组或补全 tabs 模式 + 图表 `<title>`；`role="presentation"` 折叠钮修正；可点击 div/日历格/DAG 节点补 role=button + 键盘；状态徽章 aria-live；触屏 suppressClickRef 复位。

### 测试与门禁（每批同样适用）

- 前端：`pnpm test:ci` + `pnpm lint`（tsc --noEmit + eslint + design-token 检查）；Rust：`cargo nextest run -p shannon-desktop` + fmt/clippy。
- 新命令/能力同步 `desktop/acl/app-permissions.json` + capability 集（ACL 覆盖测试把门）。
- P0-2 需补 mergeFile 单测（重复 hunk 决策矩阵）；P0-6 需补路径穿越注入用例（`../`、绝对路径、`~`）。
- i18n：B6 落地后把键比对脚本挂进 CI。

---

## 8. 决策点（已全部拍板 · 2026-09-26，均采纳原建议列）

| # | 决策 | 选项 | 拍板结论（含落地位置） |
|---|---|---|---|
| 1 | **模型切换的值空间**（P1-2/B1-8） | A. 全部统一写 id，后端维持现状；B. 后端显式兼容 name→id 解析 | **A**。id 是稳定标识，name 是展示层；旧约定废弃。Red Team 已源码证实（`provider_resolver.rs:60` 透传 + `model_registry.rs:790-791` display_name≠id），无需再实测；含存量迁移，落地 **B1-8** |
| 2 | **Remotes 默认目标回读**（P1-16） | A. 后端加 getter / 在 list 里带出；B. 前端 localStorage 镜像 | **A**。默认目标本就持久化在 remotes.toml，前端镜像必然再分叉；落地 **B2-20**（优先 list 带出） |
| 3 | **记忆 project 字段**（P1-20） | A. 后端 update_memory 支持 project；B. 编辑态置只读 | **A**。跨项目整理是真实需求；后端子任务落地 **B3-24**（+0.5 人日） |
| 4 | **Welcome 工具选择立场**（P1-38） | A. finish 时真调配置 IPC；B. 改为纯推荐文案 | **B**。A 需要后端批量工具开关语义，超出 Welcome 页职责；B 诚实且零风险；落地 **B5-33** |
| 5 | **i18n 缺键长期策略**（P1-5/B6-35） | A. 补齐全部 306×8 + CI 门禁；B. en 兜底为长期方案，只人工补高频键 | **A 的 CI 门禁 + B 的兜底并行**：兜底立即上（落地 **B1-14**，provider 层），缺键按使用频率分档补，CI 防新增缺口（落地 **B6-35**） |
| 6 | **updater 配置处置**（§5 骨架） | A. 补全密钥与签名流水线；B. 先摘除 updater 插件配置 | **B**。占位符公钥 + 第三方 endpoint 的半启用状态既不可用又埋雷；等发布流水线就绪再回填；落地 **B1-15** |

---

## 9. 工作量汇总与顺序

| 批次 | 内容 | 人日 | 前置 |
|---|---|---|---|
| B0 | P0×6 + 一行级顺带 | 2.5~3 | 无 |
| B1 | 骨架正确性 + i18n 兜底 + 决策 6 | ~2.5 | B0-7（setContextPanelOpen 同文件） |
| B2 | 设置/Usage 数据正确性 | ~2.5 | 无 |
| B3 | 扩展/记忆/任务错误态与竞态 + 决策 3 | ~3 | 无 |
| B4 | Triage/diff 评审线 | ~2 | B0-3（mtime 机制复用） |
| B5 | 编辑器/终端/Welcome | ~1.5 | B0-5（编辑器守卫同文件） |
| B6 | i18n + a11y 清尾 | ~2.5 | B1-14（provider 层兜底） |
| **合计** | | **16~18.5** | B0 最优先 |

顺序建议：**B0 立即**（4/6 条 P0 会写错用户数据）→ B1、B2 并行 → B3、B4 并行 → B5、B6 收尾。每批独立 PR、目标 `dev`，走既有 CI 门禁。**并行约束（Red Team R3-4）**：与对话页第二轮文档的 B0/B1 中同文件条目（`AppContext.tsx`、`Chat.tsx`）串行或合入同一 PR，避免并行冲突。**开工待批准（2026-09-26 评审结论）。**

---

## 10. 信息源与置信度说明

- **代码证据**：六个审查线程全量通读各自范围（含关键 Rust 命令与语言包比对）；主审经两轮（初审 + Red Team 复审）逐条坐实 **6 条 P0 + 17 条 P1**（P1-1/2/3/4/5/6/9/15/18/19/21/25/26/28/29/31/34），并源码证实决策 1 的完整语义链（`provider_resolver.rs:60` 透传 + `model_registry.rs:790-791` display_name≠id）；P0-2 另有 node 级复现。**未复核的 P1/P2（约 40 条）在实现前请先复测（约 30 秒/条）**，个别条目已在文中标注「待复测」（theme=system 的 setState bail-out、Base UI Dialog 的 Escape 传播、终端键盘陷阱实机行为）。
- **行号基准**：dev@ddcddab1。落实现时以内容定位为准，行号可能因并行改动漂移。
- 与对话页第二轮文档的关系：零重叠；两份文档合计构成桌面 UI 的完整问题面。B0 两批止血（对话页 B0 + 本文 B0）可合并为一个发布里程碑。
