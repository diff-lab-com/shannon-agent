# 桌面端 UI 审查方案 · 对抗性复审（Red Team）

- 日期：2026-09-26
- 被审对象：`docs/plans/2026-09-26-desktop-ui-pages-review.md`（下称「原方案」）
- 前提：§8 六个决策点已按原方案建议拍板（决策 1=A 统一写 id；2=A 后端回读；3=A 后端支持 project；4=B 诚实文案；5=A 门禁+B 兜底并行；6=B 摘除 updater）
- 方法：不信任原方案的未复核声明，对关键结论做源码级重验；对 B0 各修复设计做可实施性推演；对已拍板决策与方案正文的落地一致性做逐条对账。共 20 余项新验证，全部亲自读码。

---

## 0. TL;DR

**总体判定：方案主体成立，可以开工——但有 4 处修复设计需要修订、3 处决策落地缺口需要补、若干文档错误需要改。**

最重要的三个对抗性结果：

1. **决策 1（统一写 id）从「推断」升级为「源码证实」**：`resolve_active_target` 把存储字符串原样作为 `model_id` 返回（`provider_resolver.rs:60`），而模型注册表证实 display_name ≠ id（`model_registry.rs:790-791`：`claude-sonnet-4-20250514` 的显示名是 "Claude Sonnet 4"）。Header 写 name 的路径发往 API 的就是显示名，**当前对 name≠id 的模型必然失效**。但由此引出一个原方案遗漏的**存量数据迁移**问题（见 R2）。
2. **P0-2 的根因比原方案写的更精确、修法更简单**：hunk id 实际是**位置寻址**（`` `${oldStart}-${oldEnd}-${newStart}-${newEnd}` ``，`diff-merge.ts:151`），不存在 id 碰撞；bug 只在 mergeFile 的**内容查找**（`:212-218`）。修复 = 按序 1:1 对应，无需动 id 方案。原方案的修复方向正确但描述失焦，且审查线程内部「hunk id 内容寻址」的表述有误。
3. **B0-1（OPC 审批）修复后仍有歧义缺口**：主窗口对权限事件**全收**（`windowSession.ts:52-58`，`windowSessionId===null → return true`），而 OPCTask 的审批区只判 `permissionRequest !== null`（`OPCTask.tsx:148`）。只改传 `request_id` 后，聊天会话的待审请求仍会被当成 OPC 任务批准。需补「审批对象归属展示」。

抽查的 7 项未复核 P1（webhook 输入缺失、usePagedVisible 重置、DataSources 占位初值、Featured 不派发事件、日历未按日过滤、CodeEditor 硬编码 light、sniffs_as_binary 存在性）**全部坐实，零翻案**；Triage 索引错位、DiffReviewBody 单次 fetch、scheduled-tasks 无事件订阅、agent 安装 join 同样坐实。原方案的置信度声明成立。

---

## R1. 高优先：修复设计必须修订的 4 处

### R1-1　B0-1（OPC 审批）补「审批对象归属」，否则修完仍有误批风险

- 复验证据：`windowSession.ts` `isEventForCurrentWindow`——主窗口（`windowSessionId === null`）对一切事件返回 true；`AppContext.tsx:623-631` 的 PERMISSION_REQUEST 处理器因此把**所有会话**的请求都 set 进全局单例；`OPCTask.tsx:148` 审批区仅以 `permissionRequest !== null` 为门控。
- 缺口：用户开着 OPC 任务页时，若某聊天会话恰好有待审权限请求，页面显示审批区，用户点「批准执行」→ 修好后批准的是**聊天会话的请求**——对象错位从「必败」变成「误批成功」，更危险。
- 修订建议（并入 B0-1，+0.25 人日）：确认弹窗必须展示待审请求的归属（session_id → 会话标题，`permissionRequest.session_id` 已在 payload 里）；批准/回滚文案从「已批准执行」改为带归属的表述；若能建立 task↔session 关联（OPC 任务的运行会话 id），不匹配时禁用按钮并提示归属。

### R1-2　B1-10（侧栏宽度）按原文不可实施，改为「所有权统一」

- 复验证据：`Layout.tsx:42` 注释明确「Single Sidebar instance」，`:137` `<Sidebar mobile={mobileMode} ...>` 单实例跨断点翻转，**不重挂载** → Sidebar 的 `[width]` effect 不重跑，bug 坐实（这一点原方案判对了）。但原方案的快速修法「mobileMode→false 分支恢复 `--sidebar-w` 为当前 width」**不可实施**：width 状态在 Sidebar 组件内，Layout 拿不到。
- 修订建议：把 `--sidebar-w` 的写权统一收到 Layout（Sidebar 通过 SidebarContext 或回调上报 width，Layout 唯一写入；mobileMode 分支写 0px、桌面分支写上报值）。这样也顺带消除「两个 effect 各自写同一个 CSS 变量」的所有权模糊。

### R1-3　B1-14（i18n 兜底）修在 useT 是错的层级，改 IntlProvider 层一行合并

- 复验证据：`i18n/index.tsx:95` `<IntlProvider locale={locale} defaultLocale="en" messages={MESSAGES[locale]}>`——无 en 合并、无 onError。而直接调 `intl.formatMessage` 的调用点不止 useT：CommandPalette（`:104`）、Layout footer（`:168,175,180`）、DoneStep（`intl.formatMessage`）等。只修 useT 覆盖不了它们。
- 修订建议：`messages={{ ...MESSAGES.en, ...MESSAGES[locale] }}` 并按 locale `useMemo`（避免每渲染新对象导致 IntlProvider 子树重刷）。一行修复覆盖全部 formatMessage 路径 + 消除 react-intl 缺键 console 错误。`messageFor`（provider 外独立路径）单独做同样合并。B6-35 的 CI 门禁不变。

### R1-4　B1-8（统一写 id）必须补存量数据迁移，否则老用户切完模型仍是坏的

- 复验证据：`commands_config.rs:357-365` 把传入字符串直接 `set_active` 持久化进 providers.toml；用 Header 切过模型的存量用户，文件里存的是 display_name。前端改为只写 id 后，**旧值不会被纠正**——直到用户再切一次模型，期间发送持续失败。
- 修订建议：B1-8 增加后端一步——读取 active_target 时（或 configure('model') 入口处）经模型注册表做一次归一化：值能精确匹配某模型 id 则原样；匹配某 display_name/别名则改写为 id（`model_registry` 已有 `aliases` 字段可复用）。同时更新 Header 处「U2」注释（其「config 存 name」的旧约定被本次源码验证推翻）。

---

## R2. 已拍板决策的落地对账（3 个缺口）

| 决策 | 方案内落点 | 对账结果 |
|---|---|---|
| 1 统一写 id | B1-8 | ✅ 且经 R1-4 加强（补迁移） |
| 2 Remotes 回读 | B2-20 | ✅ 需后端 getter，注意 `remoteListTargets` 带出比新加命令更省 ACL 工作 |
| 3 记忆 project | B3-24 | ⚠️ 选了 A（后端支持）但 B3 未列后端子任务：`commands_memory.rs` 改 update_memory 签名 + `tauri-api.ts` 同步 + 单测，**+0.5 人日**，B3 估算应上调 |
| 4 Welcome 文案 | B5-33 | ✅ B 选项即原文案 |
| 5 i18n 混合策略 | B1-14 + B6-35 | ✅ 且按 R1-3 修正修法层级；B6-35 范围明确为「高频档补齐 + CI 比对脚本」，与 2.5 人日估算匹配 |
| **6 摘除 updater** | **无批次归属** | ❌ 原方案 §5 骨架第 9 条只在 P2 清单里，未进任何批次。建议入 B1（编辑 `desktop/tauri.conf.json` 摘除 updater 插件配置 + 跑 ACL 覆盖测试确认无连带，0.25 人日） |

另一处悬空：§5 骨架 P2 的「CSP frame-src 收敛 + assetProtocol scope 收敛」也无批次归属。**且不宜直接收敛**——`frame-src https:` 是网页 tab 功能（对话页第一轮交付）的存在前提，asset scope `$HOME/**` 服务任意工作目录的文件 chip；盲目收敛会砍掉已交付功能。建议改写为「记录为已知风险，收敛方案需与网页 tab/文件引用功能一起做产品决策」，明确 deferred，避免执行者误当成批内任务。

---

## R3. 原方案文档自身的错误（改文档即可）

1. **§5 标题计数错误**：「择要，共 24 条」——实际罗列约 60 条。改为「择要（全量约 60 条，按领域归并）」。
2. **§10 复核计数过时**：写「13 条核心 P1 已坐实」（实际原文为 12， Red Team 复审时误记为 13）。两轮复审后实际为 **P0×6 + P1×17**（初审坐实 P1-1/2/3/4/5/6/9，Red Team 新增坐实 P1-15/18/19/21/25/26/28/29/31/34 + P0-6 的 agent 半边）。已在 v2 更新。
3. **§5 引用悬空**：「全量清单见各审查线程原始输出，可另附」——原始输出只存在于审查会话，未落盘。两个选项：把 P2 全量清单补为文档附录 A（推荐，约 +80 行），或在 §5 开头声明「本节即 P2 权威清单」。
4. **§9 缺并行冲突提示**：对话页第二轮的 B0（流式状态机）与本文 B1-9（switchToSession 竞态）都改 `AppContext.tsx`，对话页 B1 与本文 B0-5 都动 `Chat.tsx`。两份 B0 可并行开工，但**同文件条目应串行或合入同一 PR**，建议 §9 补一句。
5. **B0-2 描述失焦**（不影响结论）：根因是 mergeFile 内容查找而非 id 方案（见 TL;DR #2）；修法改述为「mergeFile 复用自己第 180 行已算出的 hunks，与 lines 段按序 1:1 对应（段序=hunk 序）」，回归测试补「相邻同内容双 hunk」边界。顺带修正审查线程内部「hunk id 内容寻址」的错误表述，避免后人据此做错推断。

---

## R4. 复审过程中的新增发现（原方案未收录）

1. **diff Apply 双击无重入防护（并入 P0-4）**：`DiffReviewBody.tsx` `handleApply` 只有 `if (!diff || !filePath) return`，无 `applying` 早退；键盘 Enter 连按两次在状态刷新前会通过两次守卫 → 双次写盘。B0-4 的修法已含「applying 防抖」，但测试用例应明确覆盖键盘双击路径（handleApply 自身也要加守卫，不只靠键盘层）。
2. **Layout footer 金额硬编码**（并入 B6 i18n 清单）：`Layout.tsx:168` `${usage.cost_usd.toFixed(4)}`——`$` 前缀 + toFixed 绕过 Intl，与 §5 已列的 fmtCost 同类。
3. **OPC 审批必败的另一层原因待查**：OPC 任务的执行会话若在独立窗口（session window），主窗口审批区的 `isEventForCurrentWindow` 会放行（全收），但 session 窗口自己也会收到同一请求弹自己的审批 UI——**同一请求两处可批**，先批者赢，后者静默失效。低频但值得在 B0-1 实施时确认 `setPermissionRequest(null)` 的跨窗口同步语义（后端 respond 成功后是否有全局事件清掉其它窗口的 pending UI）。

---

## R5. 工作量与排期修订

| 批次 | 原估算 | 修订 | 修订原因 |
|---|---|---|---|
| B0 | 2~2.5 | **2.5~3** | R1-1 审批归属 +0.25；P0-2/3/4 回归测试实际工作量 |
| B1 | ~2 | **~2.5** | R1-2 所有权重构略大于单点修复；R1-4 存量迁移 +0.25；决策 6 入 B1 +0.25 |
| B2 | ~2.5 | 不变 | |
| B3 | ~2.5 | **~3** | 决策 3-A 后端子任务 +0.5 |
| B4 | ~2 | 不变 | save_text_file 加 `Option<String> expected_mtime` 参数（Tauri 缺省参数安全，非破坏性），原方案担心的签名/semver 问题不成立 |
| B5 | ~1.5 | 不变 | |
| B6 | ~2.5 | 不变 | 决策 5 混合策略下范围匹配 |
| **合计** | 15~17.5 | **16~18.5** | |

顺序不变：B0 立即 → B1、B2 并行 → B3、B4 并行 → B5、B6 收尾。新增约束：与对话页第二轮 B0/B1 的同文件条目（AppContext.tsx、Chat.tsx）串行或同 PR（R3-4）。

---

## R6. 结论与建议动作

1. **方案可执行**，P0 清单与批次结构经对抗复验后无一翻案；按 R1 四处修订修复设计、R2 补决策落地、R3 改文档、R5 调估算后即为终稿。
2. 建议动作序列：① 我把 R1~R5 的修订直接落回原方案文档（一次编辑，产出 v2，状态改「已评审·按 Red Team 修订」）；② B0 按修订后设计开工。
3. 遗留给实施者的两条纪律：P2 未复核条目（约 40 条）实现前每条 30 秒复测；所有「修法」先读 R1 是否已改写该条。
