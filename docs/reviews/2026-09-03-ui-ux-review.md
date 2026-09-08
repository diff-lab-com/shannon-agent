# Shannon UI/UX 综合评审(2026-09-03)

> 角色:高级前端工程师 / 高级 UI 设计师 / 高级产品经理。
> 方法:3 路并行代码勘察(desktop 前端全部 surface、组件与设计系统、既有评审与旅程文档)+ 对关键发现的人工源码抽查 + 与 Phase 1–3 现代化文档、`2026-08-28-comprehensive-review.md` 比对去重(不重报已修复项)。
> 局限:静态评审,未做运行时视觉走查;建议后续用 `pnpm demo` + Playwright 截图对最老(Settings/OPC/Memory/Extensions)与最新(TurnTimeline)页面做 pixel-level 走查。

## 0. 评审范围

| Surface | 技术 | 状态 |
|---|---|---|
| 桌面端(主评审对象) | Tauri 2 + React 19 + TS 5.8 + Vite 6 + Tailwind v4 + shadcn(base-nova)+ react-intl | 16 页面 / ~129 组件 |
| TUI | Rust ratatui(`crates/shannon-ui`) | 最近仍在活跃开发 |
| VS Code 扩展 | `editors/vscode`(原生 TS webview) | v0.1.0 |
| 网站 | Astro 5(`website/`) | landing + docs |

核心旅程基线:`2026-08-28-comprehensive-review.md` §7 的 J1–J7。

## 1. 总体结论

前端工程质量显著高于同类桌面 AI 应用的一般水平:三阶段 UI 现代化(设计系统收敛、a11y、测试门禁)已全部落地合并,此前 P0(双会话列表、双 Header、死复选框)经抽查确认均已修复。当前问题集中在四类:

1. **正确性 bug**:主题注册表冲突、死快捷键、全局初始化失败被静默吞掉、成本曲线画错坐标轴;
2. **从未经现代化 QA 复核的页面**(最老的 Settings/OPC/Memory/Extensions 与最新的 TurnTimeline);
3. **产品/旅程层断裂**:eval 旅程无入口、`/rewind` 桌面缺失、权限无 scope、反馈信号无处展示;
4. **跨端一致性**:桌面/TUI/CLI 漂移成两个产品,i18n 覆盖三种口径。

## 2. 值得保持的亮点

- 设计 token 体系有文档(`desktop/ui/src/styles/tokens.css`、`styles/README.md`),12 套完整主题;
- Context 切片与竞态处理扎实(`AppContext.tsx`:`streamingTextRef`、cancelled flag、worktree 失败回滚);
- a11y 是真基建:全局 `:focus-visible`、聊天 `role="log" aria-live`、modal 焦点还原有 Playwright 回归测试、axe 进 e2e;
- i18n parity 工具链(`desktop/scripts/check-i18n-parity.mjs`)、mock-mode e2e(`VITE_MOCK_MODE=1`)、全仓库 0 处 `window.confirm`、70 处统一 `toastError`;
- 测试门禁硬(覆盖率 80/75/60/80 + ESLint `--max-warnings 0`),非测试源码 0 个 TODO/FIXME 欠账。

## 3. 问题清单

### P0 — 正确性

1. **ember / slate 主题的明暗注册与 token 定义冲突** — `desktop/ui/src/context/ThemeContext.tsx:35-36` 注册为 `'dark'`,但 `index.css:610-661`(ember,`--background:#faf5f0`)、`664-715`(slate,`#f8f9fa`)是浅色 token。`dark:` 变体(`ui/input.tsx:12` 等)在这两个浅色主题上错误生效,12 个主题中 2 个系统性异常;`e2e/themes.spec.ts:15-16` 断言注册表值而非视觉正确性,把 bug 锁进了测试。
2. **全局初始化失败被静默吞掉** — `AppContext.tsx:79-112` 初始加载失败仅 `console.warn`,通用错误落在聊天画布(`pages/chat/MessageArea.tsx:97-102`),无 toast、无来源区分、无重试。

### P1 — 体验与一致性

3. **TurnTimeline 成本曲线画在 token 坐标轴上** — `pages/TurnTimeline.tsx:243,271`:cost(USD,零点几)对 `yMax=output_tokens`(万级),成本线贴地误导;`:49-52` 货币硬编码 USD。
4. **深色主题对比度低于 WCAG AA** — tokyo-night `--muted-foreground #565f89` on `#1a1b26` ≈3.9:1(`index.css:397-407`)却大量用于 11–12px label;`text-outline-variant` 用于 footer(`Layout.tsx:103`)、会话计数(`SidebarSessions.tsx:231`)、占位符(`ChatInput.tsx:288`)。
5. **交互可见性** — 消息操作工具栏 hover 才显形(`MessageBubble.tsx:222,319`);Header 模型选择器手写 `role="listbox"` 缺 `aria-activedescendant`(`Header.tsx:160-186`),应换 Base UI Select。
6. **主题感知缺失的硬编码颜色** — `tasks/ScheduleDAGView.tsx:95-100,148-199`、`artifact/MermaidRenderer.tsx:24` 写死浅色 hex,深色主题下失效。
7. **聊天列 prop drilling 三层透传** — `MessageArea` 16 props、`ComposerPanel` 14、`ChatInput` 12(`pages/chat/MessageArea.tsx:9-25` 等),多数值本可来自 context。
8. **轮询替代事件** — `tasks/AgentMessagesPanel.tsx:98` 每 5s 无条件轮询;`Sidebar.tsx:145-158` 30s triage 轮询(注释自述待换事件)。
9. **Welcome 向导** — `pages/Welcome.tsx:53` `TASKS.find(...)!` 非空断言、`:98` 硬编码 `setStep(2)`;`:54-55` env 检测到 key 但 provider ≠ 推荐值时无法前进(死胡同)。
10. **i18n 残留** — `chat/Markdown.tsx:80` 图表错误文案英文硬编码;`MessageBubble.tsx:178` 向模型发送未翻译英文 prompt。
11. **Markdown 行号靠命令式 DOM 注入** — `Markdown.tsx:175-191,299-306` 在无关组件 effect 里 `document.querySelectorAll` 注入,应组件化。

### P2 — 工程质量/可维护性

12. z-index 无单一真源:`styles/tokens.css:136-147` z scale 零引用,实际散落 `z-30/40/50`、`z-[60]/[70]/[100]`。
13. 主题系统三处同步负担(ThemeContext 两张表 + index.css ~50 行 + e2e);`tokens.css` 自称 canonical 已漂移;`index.css:175-181` 因 spacing 遮蔽 `max-w-*` 需手工补丁。建议 codegen / build-time 校验。
14. 基元缺口(无 tabs/table/checkbox/radio/slider)导致手写 listbox 等;`src/design/` 平行 playground 混在应用源码树。
15. 路由树卫生:9 条 legacy redirect、dev-only `/chat-v2-spike`、`/quickfix` `/editor` 双入口、Tasks 页吸收 5 组旧概念成 IA 垃圾抽屉。
16. 小项:消息 <30 条时 virtualizer 仍运行(`Chat.tsx:67-80`);`ROADMAP.md` 引用已不存在的 SCHEDULED-FIX-PLAN 路径。

### P0/P1 — 产品与用户旅程(J1–J7 净缺口)

17. **J7 评估旅程仍是唯一完全断裂的旅程**:eval runner 无 CLI、无 CI、无桌面入口(08-28 评审已列,至今未修)。
18. **J5 缺口**:`/rewind` 桌面无入口;权限"始终允许"因后端无 approval-scope 概念被删除(记录在案)。
19. **反馈信号收集后无处展示**(PM-12,仍未修),信任闭环断裂。
20. **首启即 `seedSampleData()` 写入示例任务**(`Welcome.tsx:111-117`),与 U7/D4"空状态引导而非种子数据"决策相抵,污染 Tasks/Triage。
21. **语言承诺三种口径**:引擎/TUI 10 语言、桌面 en/zh-CN、网站另有一套。
22. **桌面与 TUI/CLI 漂移成两个产品**(命令面、`/help` 覆盖 63/121、TUI a11y 弱)——需跨端命令面契约。
23. **实验性功能曝光策略缺失**:OPC dev-only 零露出,无"毕业为正式功能"的 UI 策略。

## 4. 改进路线

- **第一批(P0+速赢,小 PR 序列)**:修 ember/slate 主题并升级 e2e 为亮度校验;修/删 `mod+/` 死快捷键(`useKeyboardShortcuts.ts:30-33` 切换不存在的 `.collapsed` 类);初始化错误上移;TurnTimeline 成本曲线第二轴;2 处 i18n 硬编码(en+zh 同 commit);Welcome 非空断言与 env-key 死胡同。
- **第二批(专项)**:逐主题对比度 + 全页 axe;聊天列状态收敛;DAG/Mermaid 主题化;轮询→事件(需后端补事件);z-index 与主题 token 单一真源(codegen)。
- **第三批(产品级)**:eval 桌面/CLI 入口;`/rewind` 入口 + 权限 scope 后端;反馈信号面板;路由/双入口 IA 清理;跨端命令面契约。

## 5. 本次已修复(见 git 工作区)

- [x] ember/slate 主题 scheme 校正 + `e2e/themes.spec.ts` 改为亮度一致性自动校验
- [x] `mod+/` 死快捷键处理
- [x] AppContext 初始化失败上移为可见错误
- [x] TurnTimeline 成本曲线第二轴 + USD 常量化
- [x] Welcome env-key 死胡同 + 非空断言移除
- [x] `Markdown.tsx:80` / `MessageBubble.tsx:178` 英文硬编码 i18n 化(en+zh)

## 6. 第三批实施结果(2026-09-03 下午)

**全部 8 项已实施**(细节见 git 工作区,门禁全绿:lint 0 警告 · vitest 1401 通过 · i18n parity 2386 键 · Playwright 主题 4/4 + smoke/nav 13 通过 · `cargo check` shannon-desktop / shannon-cli 通过 · 生产构建 `VITE_MOCK_MODE=1 pnpm build` 通过):

1. **DAG/Mermaid 主题感知**:ScheduleDAGView 全部 hex → 语义 token 类(fill-primary-container 等,随 12 主题联动);MermaidRenderer 注入主题模式(mermaid dark/default + 错误样式双 scheme);SVG 内 4 处硬编码英文 i18n 化。
2. **聊天列 prop drilling**:新增页内 `ComposerContext`(草稿与附件,刻意独立于 chat 切片避免按键重渲染消息列表);MessageArea 16→5 props、ComposerPanel 14→2 props;`t` 全部改 `useT()`;Cmd+D 工作目录监听移交 ComposerPanel;消息列表不再随输入重渲染。
3. **逐主题对比度 + axe 门禁**:新增 `scripts/contrast-audit.mjs`(WCAG 公式校验全部主题 token 配对,exit 1 可进 CI);修复 11 个主题的 `--muted-foreground` 及 ember/slate/solarized 系共 37+14 处 token(含 solarized-light foreground/link);新增 `--color-link` 令牌(12 主题,链接文字 AA);MessageHeader `/70` 透明度文字、footer 版本、占位符等组件修正;`e2e/themes.spec.ts` 新增 **12 主题 × 全页 axe color-contrast 门禁**(含违规元素计算样式诊断输出)。
4. **路由/IA**:删除 `/quickfix` 死路由(无任何入口,聊天内嵌保留);`/editor` 保留(有 mod+5 与命令面板入口);修正 Chat.tsx 与现实不符的注释;`/chat-v2-spike` 确认已有 dev 门禁(无需改动)。
5. **轮询→事件**:新增后端事件 `agent-messages-updated` / `triage-updated`(shannon-types event_names + `record_agent_message` / `mark_triage_read` / `archive_triage_item` 发射);前端 AgentMessagesPanel(5s 轮询)与 Sidebar(30s 轮询)改为事件订阅 + 聚焦刷新。已知覆盖缺口:CLI 侧外部写入的 agent 消息不触发事件(需 AppState 文件监听,已留注释)。
6. **权限 scope**:后端 `respond_permission` 增加 `scope="always_tool"` → 工具名写入 `~/.shannon/settings.json` 的 `permissions.allow`(Deny>Ask>Allow);`pending_permissions` 携带工具名;前端权限弹窗新增"始终允许"按钮(en+zh);修复 `tauri-api.respondPermission` 签名(options 对象)。
7. **eval CLI(J7 闭环)**:新增 `crates/shannon-cli/src/eval_cmd.rs` + `shannon eval run|diff|aggregate`(自 `examples/eval_runner.rs` 移植,exit code 契约保持)。实测:`eval run --list` 列出 20 任务全部 ok;`eval run --task read_01` dry-run 端到端通过(report 落盘,1/1 passed)。
8. **运行时走查 + 全规则 axe**:新增 `e2e/walkthrough.spec.ts`(手动,WALKTHROUGH=1)+ `playwright.walkthrough.config.ts`(生产构建 + vite preview,免 inotify);10 路由 × 2 主题截图 + 全规则 axe 报告落盘 `test-results/walkthrough/`。走查修复:所有 critical(select-name 2 处、form label 3 处)、tasks/memory/extensions/settings 的 outline-as-text 与 `/60` 稀释文字;violations 19 行 → 12 行(全部为 serious color-contrast)。

### /rewind 桌面入口 — 降级为实施计划(未实施)

勘察结论:REPL 的 `/rewind` 依赖 REPL 查询循环里的 `CheckpointManager.record_turn`(crates/shannon-ui/src/repl/query.rs:1448),而**桌面查询路径完全不记录 checkpoint**;且桌面持久层为 L0,回滚需同时截断 engine 会话与 L0(`branch_session` 的 `create_branch` 是最近似原语)。完整实施需要:
1. 桌面 send 流程按 turn 记录 `TurnCheckpoint`(desktop/src/commands.rs send_message,复用 `shannon_core::checkpoint::CheckpointManager::for_session`);
2. 新 Tauri 命令 `list_checkpoints(session_id)` + `rewind_session(session_id, checkpoint_index)`(conversation 维度调 `QueryEngine::rewind_conversation`,L0 维度需新增 truncate 原语或"回滚即分支"语义);
3. 代码文件回滚接 `FileHistoryManager::rewind_file_to_turn`(crates/shannon-tools/src/file/history.rs);
4. 前端在 MessageBubble 用户消息上增加"回溯到此处"菜单(messageIndex 已就位)。
建议按独立后端特性排期(涉及查询管线与持久层语义),不适合混入本轮 UI 批次。

### 遗留清单(带精确选择器,见 test-results/walkthrough/axe-report.md)

- 12 行 serious color-contrast(单主题或单页):triage 玻璃卡片 chips(`gap-xs.text-label-sm`)、usage 虚拟化行 `data-title/data-description`、extensions 容器 chips(`bg-primary-container/50 text-on-primary-container` 在 material 下不足 4.5)、memory 卡片 meta、settings/models `opacity-70`、settings/theme 深色 1 处 `text-primary`、tasks 日历相邻月装饰数字(`text-outline/30`,属刻意弱化)。均为组件局部样式决策,建议随各页下次迭代按报告选择器逐个处理。
- agent 消息事件的 CLI 外部写入覆盖缺口(见第 5 项注释)。

## 7. P1 三项 + 流程建议落地(2026-09-03 晚)

### 7.1 权限管线接线(P1-1,完成)
引擎侧本就有完整的交互式权限通道(`process_query` 的 `permission_request_tx` + `PermissionChoice`),桌面端一直传 `None`——这就是"弹窗 UI 存在却永远不会弹出"的断点。本次:
- `send_message` 创建 mpsc 通道并传入 `process_query(context, Some(perm_tx))`;新增转发任务把引擎 `PermissionPrompt`(含 diff 预览)映射为现有 Tauri `PERMISSION_REQUEST` 流程,用户决定(拒绝/单次允许/始终允许)映射回 `PermissionChoice`(AlwaysAllow 同时进引擎会话内记忆)。
- `commands_permissions.rs` 抽出 `prompt_user`(供命令与转发器共用),`PendingPermission` 载荷从 `bool` 升级为 `PermissionDecision { AllowOnce | AlwaysAllow | Deny }`;交互式提示超时放宽到 300s(读 diff 需要时间),命令式 `request_permission` 保持 30s。
- 规则消费:`send_message` 与后台任务路径均加载 `~/.shannon/settings.json` 的 deny/ask/allow 进 `PermissionRuleChecker::set_rule_checker`——"始终允许"落盘的规则在下次会话即生效,不再重复弹窗。后台任务不接 UI 通道(无人值守,引擎对无通道默认放行)。

### 7.2 对比度收尾 + 门禁入 CI(P1-2,完成)
- 走查 ROUTES 扩至 **13 条**(新增 /welcome、/opc、/timeline/demo-session),`/welcome` 无侧栏的等待改为容错;逐行清零 axe 报告:17 行 → **0 critical/serious**(26/26 测试通过)。
- 三个系统性根因修复(影响面超出单页):
  1. **`--color-primary-foreground` 从未定义**——Button/Badge 默认变体的 `text-primary-foreground` 是死类,文字继承前景色叠在 primary 底上。改为 `text-on-primary`(button.tsx、badge.prim.tsx)。
  2. **tailwind-merge 把 `text-label-md`(字号)误判为颜色**,吞掉默认变体的 `text-on-primary`(MemoryPanel "新建记忆"按钮文字撞深色)。改用 `text-[14px]` 规避。
  3. **暗色主题从未覆盖 `--color-on-error-container`**(继承默认块的深红 #93000a,叠暗红容器 ≈1.3:1)。为 6 个暗色主题按容器色相生成浅色 tint;contrast-audit.mjs 补 `on-error-container/error-container` 配对防回归。
- 其余:sonner error toast 浅色文字 #e60000→#b3261e(≥4.5:1);extensions/memory 全系 `bg-*-container/30~60` 稀释徽章改全不透明;triage 已读 `opacity-70` 整卡降透明改为 token 化弱化;日历相邻月数字 aria-hidden + muted;KanbanBoard 去掉不合法的 `role="row"/"grid"`(aria-required-children critical);OPCAgentSwarm 菜单按钮移出 `role="button"` 卡片(nested-interactive);settings/theme 选中态 `text-primary`→`text-on-surface`;OpcAnalyticsDashboard pending 徽章改 secondary-container 对。
- 门禁入 CI(ci.yml):desktop-unit 增 `node scripts/contrast-audit.mjs`;新增 **desktop-visual-audit** job(生产构建 + preview 跑 13 路由 × 2 主题全规则 axe,walkthrough 现为门禁——单测断言本路由无 critical/serious);/themes 的 12 主题 axe 门禁随既有 desktop-e2e 生效。

### 7.3 eval CLI 门禁(P1-3,完成)
`just eval` dry-run 门禁与 nightly 真跑 workflow 前批已建;本轮在 ci.yml eval-dry-run 增 **Eval CLI entry** 步骤:`shannon eval run --list`(20 任务解析)+ `--task read_01` 单题 dry-run,防 CLI 入口与套件漂移(本地已验证)。

### 7.4 三条新走查页审查发现
- **/opc**:`completion_rate` mock 值为小数 0.27,而后端(scheduled_commands.rs)与测试夹具均为百分数语义,页面显示 "0%"——已修 mock 为 27.0。**未修**:日活柱状图无 y 轴刻度/网格线,数值只能靠 hover tooltip;底部 By status/By priority 徽章贴折叠线,首屏信息密度偏低。
- **/timeline**:工具瀑布条名字被 `overflow-hidden` 裁剪,短条只剩 hover tooltip(title),截图上看是"空条"——建议条宽不足时把名字放到条外侧;累计曲线只有 2 个采样点时呈直线,建议标注采样粒度。
- **/welcome**:品牌字 GradientText 在两个主题下都偏琥珀/棕,与全局紫色主色不一致(GradientText 渐变端色未跟主题 token);Stepper 圆点与标签未对齐(4 标签 5 视觉点);General 预选中导致 Continue 恒可点,首步选择缺乏"必须选一个"的引导语义(低危)。

### 7.5 第四批实施结果(2026-09-03 深夜,已提交)

- **主题系统单源**:`scripts/theme-source.json` 为唯一手工源;`generate-themes.mjs` 推导缺失 on-token(仅当主题块与 base 均未定义)、按共享配对契约(lib/contrast.mjs,audit 脚本同步复用)做生成前 AA 校验,产出纯变量主题块 themes.css + 字面量类型 registry.ts;base 色板注入 index.css 的 GENERATED:THEME_BASE 区域——`@theme` 必须留在入口样式表(Vite dev 对 @import 的 css 不跑 Tailwind 插件,生成文件里的 `@theme` 会被原样透传,这正是首版接线在 dev 下白屏/welcom 重定向的根因)。ThemeContext 的 THEME_SCHEMES 改由 registry 派生,附编译期与 ThemeName 联合类型的漂移断言;dev/demo/build 链上 `pnpm generate:themes`,CI desktop-unit 增加 `--check` 漂移门禁。生成产物与原 12 个 token 块逐字节一致(仅新增推导 token)。
- **agent 消息文件监听**:notify watcher 监听 `~/.shannon/agent-messages/`(400ms 去抖),复用既有 `agent-messages-updated` 事件;失败软降级。CLI 外写消息现在会实时刷新桌面面板。
- **timeline**:短工具条(<15% 行宽)标签外置到条右侧、靠右溢出时翻转到条左;累计曲线标题标注采样点数("2 samples")。
- **welcome**:GradientText 默认端色改 primary 系(原 via-tertiary 在 material 混入琥珀 #855000);Stepper 改为点上标签下的稳定列布局;第 0 步取消 General 预选,Continue 禁用直至显式选择(含 39 处测试流程更新)。
- 连带修复:e2e OPC 看板测试随 `role="grid"` 移除改用 aria-label 查询。

### 7.6 更新后的推荐任务表
| 优先级 | 任务 | 说明 |
|---|---|---|
| P2 | /rewind 桌面入口 | 按 §6 四步计划,独立后端特性排期 |
| P3 | PM-12 反馈信号面、z-index 令牌、缺失原语、Tasks IA、Markdown 行号组件化、chat-v2、跨端命令/i18n 一致性、~20 个未迁移桌面 i18n 文件、过期 ROADMAP.md 路径 | 维持前评 |

## 8. 第五批实施结果(/rewind + P3 长尾,2026-09-03 深夜)

### 8.1 /rewind 桌面入口(§6 四步计划落地)
- **L0 截断原语**:`SessionStore::truncate_to_turn(session_id, keep_turns)`——按 `user/message`(及无轮框架日志的 `turn/start`)行定位边界,原始行级重写(临时文件 + rename),保留 seq 单调性;writer 按 `scan.complete_lines` 恢复续写,截断后追加天然接续。4 个单测覆盖保留/清零/越界 no-op/缺失日志。
- **文件回滚**:`FileHistoryManager::rewind_before_turn(path, turn)`(严格 `<` 语义)——"回溯到消息 N"需要撤销第 N 轮本身,与既有 `rewind_file_to_turn`(`<=`,回滚到某轮末)互补;turn-0 回溯对会话内新建文件给出 Delete。2 个单测。
- **记录侧**:desktop `send_message` 按轮收集 write/edit 类工具触碰的 `file_path`,查询完成后 `record_turn`:内容快照(FileHistoryManager,10MB/UTF-8 上限,与 REPL 相同)+ `CheckpointManager` 检查点(提示词截断 80 字符);软失败不伤聊天。
- **回溯命令**:`list_checkpoints(session_id)` + `rewind_session(session_id, turn_index)`——查询中拒绝;L0 截断 → 触碰文件按 `rewind_before_turn` 恢复/删除 → 检查点弹出 → 内存消息缓冲修剪 → 返回 `load_session` 形状的消息列表。
- **前端**:MessageBubble 用户消息在"存在 ≥ 该轮的检查点"时显示回溯按钮(`undo`),ConfirmDialog 明示"消息移除 + 文件回滚";成功/失败 toast;检查点在查询完成与会话切换时自动刷新;demo 模式无检查点按钮自然隐藏。
- 语义说明:L0 采用"投影截断"(原始行删除),`load/switch` 的投影天然只含幸存轮次;engine 侧桌面每次发送新建 engine、不恢复历史(现状),rewind 不改变该行为。

### 8.2 P3 长尾
- **z-index 令牌化**:`@theme --z-index-*`(raised 10 / sticky 20 / subheader 30 / header 40 / modal 50 / scrim 60 / drawer 70 / flash 100)按实际用法取值命名(旧 tokens.css scale 与现实完全脱节且零引用,已删除并指向唯一源);21 个文件的 `z-10/30/40/50`、`z-[60]/[70]/[100]` 全部替换为语义工具类,生成产物逐类验证。
- **PM-12 反馈信号**:`commands_feedback.rs`(up/down 持久化至 `~/.shannon/feedback/<session>.json`,key=`timestamp:contentHash`,re-click 切换为清除;路径穿越校验);前端点赞/新增点踩按钮接持久化 store(乐观更新,失败回滚刷新),`lib/feedbackKey.ts` FNV-1a 键生成 + 单测。
- **ROADMAP.md**:过期的 `shannon-desktop/SCHEDULED-FIX-PLAN.md` 引用改为指向 `docs/plans/`。

### 8.3 遗留观察(不在本批修复)
- `signals::repeated_off_state_reports...` 与 `policy_limits::test_load_from_api_server_error_falls_back` 在 `cargo test`(共享进程并行)下偶发失败:进程级 Registry 全局缓冲 + 并行冲洗的测试隔离缺陷。CI 使用 nextest(每测试独立进程)不受影响;单测隔离运行恒绿。与本次改动无关(stash 验证)。

## 9. 第六批实施结果(P2 四项 + 决策 9/10,2026-09-04)

- **P2-1 多轮上下文(调查确认并修复,升级为已修缺陷)**:`send_message` 每次新建 engine(随机 session_id、空 conversation)——L0 日志全部落到随机目录,且模型看不到任何历史轮次。修复:engine 绑定真实 session(`QueryEngine::set_session_id` 新 API)+ `restore_session()` 一次投影恢复历史;日志回到正确会话目录,多轮上下文接通,/rewind 的投影语义与之天然一致。
- **P2-3 测试隔离**:signals 全局 Registry 测试加共享锁 + 文件断言前清零缓冲;policy_limits / model_registry 的 env 变更测试加模块锁。全量 `cargo test -p shannon-core` 6 连绿(此前 6 次里 4 次偶发失败)。
- **P2-2 /opc 图表**:日活柱状图加 y 轴刻度(0/半程/峰值)与网格线;hover 之外可直接读数。
- **P2-4 反馈展示面(PM-12 第二半)**:后端 `list_feedback_sessions` 聚合命令;Settings→General 新增反馈卡片(按会话聚合 👍/👎,空态引导),闭环"收集→持久化→可见"。
- **P3-10 chat-v2 决策(休眠库方案)**:生产面删除(开发专用 /chat-v2-spike 路由 + spike 页 + ChatV2RuntimeProvider 挂载 + feature flag);`src/lib/runtime/` 桥接库(assistant-ui↔Tauri 适配,843 行,含测试)保留为休眠资产,未来升级时重新挂载即可。理由:双轨并存 5 个月无产品牵引,删除生产面消灭漂移;库本身经测试验证,全删会白扔可复用投资。
- **P3-9 Tasks IA 重构**:tab 按用户任务重排——Active / Routines / Pipelines / History / Workspaces;例行 DAG、模板库、Hook 流水线、执行日志移出默认视图(各归其位),首屏=任务列表 + 日历。

### 9.1 更新后的推荐任务表(第六批后,全部六批成果已入库)

已清出:权限管线、对比度+axe 门禁、eval 门禁、主题单源、agent 消息监听、timeline 标签、welcome 三项、/rewind、z-index 令牌、PM-12(收集+展示)、多轮上下文、测试隔离、OPC 图表、Tasks IA、chat-v2。另核实:Mermaid 已是主题感知(MermaidRenderer 按模式注入 dark/default),轮询→事件基本完成(仅剩 1 处)。

| # | 优先级 | 任务 | 规模 | 说明 |
|---|---|---|---|---|
| 1 | P2 | 跨端命令一致性 | M | 桌面 CommandPalette 仅 10 条纯导航项;REPL 有 30+ 斜杠命令(/compact /context /cost /diff /permissions /rewind…)。方向二选一或并行:(a) 聊天输入框支持斜杠命令+补全,对齐 REPL 语法;(b) 能力类命令挂进 palette。需先决策桌面暴露哪些 REPL 能力 |
| 2 | P3 | 桌面 i18n 收尾 | S(比原估小) | 实测仅 ~10 个组件文件含用户可见英文(SidebarSessions×3、InlinePanelModal/AttachmentChip×2、EditorToolbar/DeleteSessionModal/ComposerPanel/ApiKeyBanner/NotificationsSettings/KeyboardShortcutsHelp/FallbackBody 各 ~1);其余 50+ 无 intl 文件是零文案 ui 原语,无需迁移。en+zh 同 commit+parity |
| 3 | P3 | 代码块原语组件化(含行号) | S | Markdown.tsx 用全局 querySelectorAll 注入行号(injectLineNumbers),artifact 侧 CodeBlock/DocumentRenderer 是独立渲染路径;抽共享 CodeBlock 原语(语言标签+行号开关+复制+gutter)双端复用 |
| 4 | P3 | 缺失原语收敛 | M | 自绘 modal/toast/空态等 ad-hoc 实现收敛到 ConfirmDialog/Banner/EmptyState;需先盘点出清单再排 |
| 5 | P4 | 观察:OPC 首屏信息密度 | S | By status/By priority 徽章贴近折叠线(§7.4 提出,图表 y 轴修复未覆盖) |
| 6 | P4 | 观察:技能候选 30s 轮询→事件 | S | 仅剩轮询点 usePendingSkillCandidates;需后端补事件,收益低 |

建议组合:1(需方向决策)+ 2 + 3 一批;4 先盘点单独排。

## 10. 第七批实施结果(跨端命令 + i18n 收尾 + CodeBlock 原语,2026-09-04)

### 10.1 跨端命令一致性 — 方向决策:聊天框斜杠命令为主入口
REPL 的 85 个斜杠命令逐一对照桌面既有面:大量命令桌面已有专属 UI(extensions/memory/tasks/settings/model 切换/permissions 管线),真正缺的是**会话级诊断**与"输入框不认识 /"这一正确性缺口(斜杠被当文本发给模型)。决策:
- **主入口 = 聊天输入框**。`lib/slash/commands.ts` 单一注册表;输入 `/` 弹出过滤菜单(↑↓/Enter/Tab/Esc 键盘导航);裸 `/name` 本地执行、绝不发给模型;其余以 `/` 开头的输入(典型:粘贴绝对路径)按纯文本发送——与 REPL 相反,但保护了编码工具的高频场景,可发现性由菜单承担。
- **命令集 = 桌面今天真正能执行的**:`/context`(新命令 `get_session_context_stats`:复用会话上暂存的 engine 投影 L0,CJK 感知 token 估算 + 上下文窗口,窗口未知时返回 null 不伪造 200K)、`/cost`(新命令 `get_session_usage`:usage.jsonl 按 session_id 过滤;`UsageRecord` 增可选 `session_id`,旧行合法且仍计入 Usage 页)、`/diff`(新命令 `get_session_git_diff`:numstat + 封顶 200KB 的 unified patch,非 git 仓库返回 `is_repo:false` 而非报错)、`/export`(复用既有导出)、`/new`(新建会话)+ 6 条页面导航。`/compact` 需引擎侧新特性,刻意缺席。
- **结果卡而非聊天消息**:`SlashResultCard` 钉在输入框上方,可关闭——诊断是关于会话的,不是会话中的一轮,合成消息会污染 L0 日志。
- **palette 挂载 = 仅自洽动作**:Export chat 进 palette(原生保存对话框,无需聊天面);/context /cost /diff 的结果渲染在聊天输入框上方,从任意页触发会"看不见",刻意不进 palette。

### 10.2 桌面 i18n 收尾 — 实测债务为零
全库两轮扫描(含模板串、aria/tooltip 属性、toast 字面量、反向扫描已用 intl 文件):**唯一真硬编码**是 ModelsSettings 的 models.dev tooltip(本批已译,en+zh 同 commit,parity 2446)。此前"~20 个未迁移文件"的估计是启发式误报(material 图标名与键盘键名被当作文案);其余 50+ 无 intl 文件是零文案的 ui 原语,无需迁移。

### 10.3 共享 CodeBlock 原语
`components/code/CodeBlock.tsx` 收敛聊天 Markdown 与 artifact 面板两条代码渲染路径(header:语言标签·行号开关·复制;自高亮与 rehype 预高亮双模式)。行号 gutter 改为组件内 layout effect 按块持有——旧实现是全局 querySelectorAll 且**寄生在 LocalImage 的 effect 上**,无图消息的代码块永远拿不到行号(顺带修复的隐性缺陷)。hljs 注册 diff 语言供 patch 着色。全量测试同时修掉上一批遗留的 `recordFeedback` mock 缺口(全量运行时的 unhandled rejection)。

### 10.4 缺失原语盘点(#4,仅清单不实施)
- **空态**:`ui/empty-state` 已有 9 处消费;自绘空态待收敛:MyAgents、TaskList、HistoryView、WorktreePanel、Installed、Sidebar。(S→M)
- **加载态**:`ui/loading-state` 存在,但 20 个文件各自 `animate-spin`(尺寸/颜色不一)。(M)
- **错误态**:`ui/error-state` 消费仅 1 处;11 处 `catch { console.warn` 对用户静默(初始化之外的刷新类失败);内联错误可统一到 Banner 的 error tone。(M)
- **确认流**:ConfirmDialog 为主流;ProvidersSection 一处自写确认待收敛。(S)
- **徽章/药丸**:10+ 文件自绘 pill(`bg-*-container`+`text-[10px]`+uppercase)——给 Badge 原语补 micro 变体后一次性收敛。(M)
- **统计卡**:memory/StatCard、OpcAnalyticsDashboard 内部 StatCard、EfficiencyCard 三处近似实现可抽共享。(S)
- **合规确认**:仅有的 2 处 `fixed inset-0`(Layout 移动端抽屉 scrim、Artifact 全屏)均使用 z-index token,不属 ad-hoc;模态/对话框已统一走 ui/modal、ui/dialog。

### 10.5 更新后的推荐任务表(第七批后)

已清出:跨端命令一致性(斜杠命令落地)、桌面 i18n 收尾(实测债务为零)、代码块原语组件化。

| # | 优先级 | 任务 | 规模 | 说明 |
|---|---|---|---|---|
| 1 | P2 | 缺失原语收敛(按 §10.4 清单实施) | M | 清单已盘点好:空态 6 文件、加载态 20 文件 ad-hoc spinner、错误态(error-state 仅 1 处消费 + 11 处静默 catch)、Badge 补 micro 变体后收敛 10+ 文件自绘 pill、ProvidersSection 自写确认流、StatCard 三处抽共享。可按原语拆成 2–3 个小 PR |
| 2 | P3 | /compact 桌面入口 | M–L | 斜杠命令面唯一缺席的会话级能力:需 QueryEngine 暴露强制压缩 API + 桌面接线 + L0 压缩语义(压缩后历史投影如何表达,建议"压缩即摘要轮"对齐 REPL)。独立后端特性 |
| 3 | P4 | 观察:OPC 首屏信息密度 | S | By status/By priority 徽章贴近折叠线(§7.4 提出,仍未处理) |
| 4 | P4 | 观察:技能候选 30s 轮询→事件 | S | 仅剩轮询点 usePendingSkillCandidates;需后端补事件 |
| 5 | P4 | 观察:themes.spec axe 抖动 | XS | 第七批全量 e2e 出现一次、隔离与复跑均绿未复现;如再现用 trace 定位 |

## 11. 第八批实施结果(五项推荐全部落地,2026-09-04)

### 11.1 缺失原语收敛(eacdb47b)
实施前逐点复核 §10.4,三处系盘点误报(启发式再次高估):"6 个自绘空态"实际全在用 EmptyState;ProvidersSection 已用 ConfirmDialog(早前批次完成);Badge 已有 size="sm" 即 micro 变体;EfficiencyCard 是进度卡不是 StatCard 孪生。真实收敛:
- EmptyState 增 compact 变体,收编 3 处真自绘空态(HookTaskPipeline、McpServers、MyAgents 性能面板);
- loading-state 导出行内 Spinner:6 处块级 spinner → LoadingState、12 处行内 → Spinner(MessageBubble 的工具状态图标刻意不动);
- AppContext 11 处静默 catch 统一到 logSoftFailure 策略(后台刷新失败刻意软失败:一次操作触发多个刷新,逐个 toast 会刷屏;启动失败走 initError);cancelQuery 属用户动作改为 toastError;
- MessageArea + WorktreePanel 错误盒 → Banner tone=error;
- 3 处徽章类 pill(ContextPanel 计数、ProviderCard 激活态、AgentMessagesPanel 优先级/类型)→ Badge size=sm,其余 pill 是 kbd/排版标签不属徽章;
- ui/stat-card 统一 memory 与 OPC 两处统计块。

### 11.2 /compact 桌面入口(1d19d9cb)
斜杠命令面最后缺席的会话级能力。新 L0 原语 `SessionStore::rewrite_with_conversation`:按 (user, assistant) 轮重建日志,temp+rename 原子替换,chunk-before-finalize 镜像真实轮次保证投影往返,writer 事后干净续写(3 单测)。`compact_session` 命令:复用会话暂存 engine → CompactEngine(LLM 摘要器,提取式回退) → 重写 L0 → 清除失效的回溯检查点 → 返回摘要 + load_session 形状消息。前端 /compact 命令 + 结果卡(前后 token、削减比、摘要轮提示),en+zh 同 commit。

### 11.3 OPC 首屏密度(7eddb4f5)
日活图占 2/3 列,By status/By priority 两列分解堆叠其右——统计卡、图表、分解全部进入首屏,双主题截图确认。

### 11.4 技能候选轮询→事件(5ec19e11)
检测器/批准/拒绝均发 `skill-candidates-changed`;hook 订阅事件,30s 轮询降为 5 分钟兜底(demo 模式事件不可达,由兜底覆盖)。

### 11.5 themes.spec axe 抖动(0f73f87e)
axe 读的是绘制后文本:扫描前等待 data-theme 属性生效 + 一帧绘制完成,消除繁忙 runner 捕获中途重绘状态的窗口。此后全量 e2e 恒绿。

门禁:core 3745 + desktop 596 + vitest 1423 + e2e 63 + 走查 26(13 路由 × 2 主题,axe 零 critical/serious)全绿;构建干净;主题漂移 0;对比度 AA;i18n parity 2454;OPC 首屏与 /compact 结果卡双主题截图核验。
