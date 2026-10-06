# 桌面端 AI 对话页审查与改进方案（第二轮）

- 日期：2026-09-26
- 状态：已评审定稿；**B0–B4 五批已全部实施**（§5 决策点按建议执行：HTML 产物先诚实静态（artifact:// 交互协议已在后续 follow-up 分支落地）、回退式重新生成、流中排队、web tab 同 URL 复用、引入 KaTeX、会话切换清 chat 产物；B2 的 AppContext 流式节流与 B3 的 dock 开合持久化移入 B1 一并落地）
- 范围：`desktop/ui` 对话页全部 UI/组件（消息流渲染、流式状态、输入区、会话管理、右侧 Dock/产物系统）
- 方法：基于 dev@ddcddab1（含 PR #120 链接/产物管线、PR #121 会话归档）的全量代码审查；三个独立审查线程（输入区/会话、消息渲染管线、Dock/产物）交叉验证，关键 P0 结论已逐条抽查源码坐实。
- 关联文档：
  - `docs/plans/2026-09-25-desktop-chat-ui-open-and-artifact-design.md`（第一轮，已实施并合入 PR #120）
  - `desktop/COMPETITIVE-ANALYSIS.md`（2026-06-13 产品级竞品基线）

---

## 0. TL;DR

第一轮交付的「链接/文件引用打开 + 磁盘产物入 Dock + 网页 tab」已正常工作；本轮审查发现的问题重心从「能力缺失」转移到**「基础交互的可靠性」**：3 个 P0 级输入缺陷（附件静默失效、拖拽在 Tauri v2 已死、中文输入法回车误发送）+ 一组流式状态机缺陷（取消后幽灵气泡、失败后重试无效、流式输出时用户滚动被强拽回底部）。这些正是竞品打磨多年、用户视为理所当然的部分。

改进方案分 5 批（B0 止血 → B1 消息交互对齐竞品 → B2 渲染质量 → B3 产物面板打磨 → B4 会话与边角），合计约 **11.5~15.5 人日**，每批可独立成 PR。§5 有 6 个需要拍板的决策点。

---

## 1. 本轮基线：第一轮交付后的能力现状

对话页当前能力（均已实施并有测试覆盖）：

| 能力面 | 现状 |
|---|---|
| 链接 | 全局拦截，默认右侧面板网页 tab，右键菜单两项（面板优先），Alt=浏览器；XFO/frame-ancestors 探测降级 |
| 文件引用 | 行内代码/工具入参路径 → FileRefChip（存在性门控），artifact 类入 Dock，代码类入编辑器，右键菜单系统打开/reveal |
| 磁盘产物 | write 类工具产物自动入 Dock（chip 常驻、autoOpen 默认关、只控激活） |
| Dock | 4 固定 tab（context/plan/live/diff）+ N 产物 tab，宽度拖拽记忆、全屏、Ctrl+\ 开合、Ctrl+Shift+A 轮换 |
| 渲染器 | Markdown（TOC/表格导出/代码块）、HTML（srcdoc+CSP）、网页 iframe（沙箱+探测）、SVG、Mermaid、图片、纯文本 |
| 输入区 | 15 个斜杠命令、附件对话框（≤10）、模型/推理力度/审批模式切换、预算横幅、语音按钮 |
| 会话 | 搜索/置顶/拖拽排序/归档/导出/时间线/分支/回退检查点 |

骨架是好的（Dock 内嵌渲染层达到或超过竞品均值，维持第一轮判断）。问题集中在下面三节。

---

## 2. 竞品对比（对话页粒度，2026-09 视角）

> 校准说明：产品级事实沿用 `COMPETITIVE-ANALYSIS.md`（2026-06 实测基线）；2026-06 之后的版本演进以公开行为的常识性总结为主，标注「常识」。检索轮次（2026-09）未发现对话页粒度的新范式变化——各家重心仍在 artifacts/canvas 的**可编辑性**与**会话记忆**。落地前建议对 Claude/ChatGPT 桌面版再做一轮实测。

### 2.1 分维度对比

| 维度 | Claude 桌面/网页 | ChatGPT 桌面 | Cursor/编码类 | Shannon 现状 | 差距定级 |
|---|---|---|---|---|---|
| **流式滚动** | 用户上滚即锁定，底部悬浮「回到底部」 | 同 | 同 | ❌ 每个 token 强制拽回底部（见 P1-1） | **P1** |
| **消息操作** | 编辑用户消息→重发、重新生成（真回退重发）、复制、反馈 | 同 + 分支树 | 同 | ⚠️ 有复制/反馈/分支/回退；重新生成是**伪实现**（发一条固定文案）；无消息编辑 | **P1** |
| **流式中输入** | 可继续输入，发送后排队 | 可输入，队列 | 可输入 | ❌ 输入框整体禁用（且被**其他会话**的后台运行连带禁用，停止按钮还指向错误会话） | **P1** |
| **IME** | composition 守卫（中日韩输入回车不误发） | 同 | 同 | ❌ 无守卫，拼音组词中回车直接发送半截文本 | **P0** |
| **会话内搜索** | 有（Ctrl+F 过滤高亮） | 有 | 有 | ❌ 无 | P2 |
| **数学公式** | LaTeX 渲染 | 渲染 | — | ❌ 不渲染（原文显示 `$...$`） | P2 |
| **附件输入** | 粘贴图片、拖拽、@文件引用 | 粘贴图片、拖拽 | @文件（核心交互） | ⚠️ 仅对话框多选；**拖拽在 Tauri v2 实际已失效**（P0-2）；无粘贴图片；无 @ 引用 | **P0/P1** |
| **草稿** | 按会话保留 | 按会话保留 | — | ❌ 全局一份草稿跨会话串扰、重启即失 | P2 |
| **产物面板** | Artifacts：版本历史、可编辑（AI 改稿）、发布；同内容复用面板 | Canvas：可编辑文档/代码，AI 快捷指令 | 预览窗+Design Mode | ⚠️ 渲染器层齐平（TOC/表格导出/LivePreview 是强项），但**无去重**（同内容点 N 次叠 N 个 tab）、无版本、只读；HTML 交互产物生产环境**静默失效**（P1-9） | **P1** |
| **会话切换** | 即时、状态各会话独立 | 即时 | 即时 | ⚠️ 无 loading、流式状态互相干扰、产物 tab 跨会话残留 | P1/P2 |

### 2.2 结论

1. **Shannon 的差异化资产**（多 tab Dock、TOC、表格导出、LivePreview、Diff 审查、终端）依然领先竞品均值，不动骨架。
2. **差距集中在「每天用 50 次的基础动作」**：打字（IME）、发消息（附件、队列）、看流式输出（滚动锁定）、点产物（去重）。竞品在这些点上已经「无感」，我们在这里「有感」，伤害大于任何高级功能缺失。
3. 竞品产物面板的演进方向是**可编辑 + 版本化**（Claude artifacts、ChatGPT canvas）；Shannon 短期不必跟进编辑器形态，但「同产物复用同一 tab」这一层基础卫生必须补（P1-10/11/12）。

---

## 3. 问题清单（按严重度；均附代码证据，实现前请复测）

### P0 —— 基本期望不成立（3 项）

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| **P0-1** | **仅附件、无文本的发送是静默空操作**：发送按钮在有附件时点亮（`disabled={!value.trim() && attachedFiles.length === 0}`），但 `handleSend` 先判空文本直接 return | `ChatInput.tsx:632`；`Chat.tsx:147` | 用户拖了文件点发送，毫无反应也无提示 |
| **P0-2** | **拖拽附件在 Tauri v2 已死**：drop 处理读取 HTML `File.path`（Tauri v1 专有），v2 webview 不注入该字段；代码用 `'path' in file` 守卫，v2 下静默得到 0 个路径。UI 却展示了完整拖拽 overlay | `ChatInput.tsx:199-213`；全仓无 `onDragDropEvent` | 功能整体失效且无感知，比没有更糟 |
| **P0-3** | **无 IME composition 守卫**：Enter 发送未检查 `isComposing`，全仓 grep 无 `isComposing` | `ChatInput.tsx:237` | 中文（主语言）/日文/韩文用户组词确认的回车会把**半截拼音转换文本**直接发出去。对 zh-CN 主场景这是日常性事故 |

### P1 —— 显著 UX 缺陷（11 项）

**消息流与流式状态：**

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| P1-1 | **流式时滚动锁定失效**：外层容器在每次 `messages/streamingText` 变化无条件滚到底部；`StreamingResponse` 内部的近底部守卫是死代码（其内部 div 无高度约束，永不滚动） | `Chat.tsx:117-124`；`StreamingResponse.tsx:34-96` | 流式输出期间用户上滚回看，立刻被拽回底部——竞品均已解决的基础体验 |
| P1-2 | **取消/失败后的幽灵流式气泡**：`QUERY_FAILED`/`QUERY_CANCELLED` 只复位 `isQuerying`，不清 `streamingText/thinkingText/activeToolCalls`；MessageArea 渲染 StreamingResponse 也只看后者 | `AppContext.tsx:595-622`；`MessageArea.tsx:161-168` | 取消后残篇带着「流式光标」挂在屏上，工具卡保持 spinner 样式直到下次发送 |
| P1-3 | **失败后的「重试」按钮是空操作**：重试仅在输入框有文本时发送，但发送时文本已被清空 | `MessageArea.tsx:251-263`；`Chat.tsx:160` | 出错横幅上的重试点了没反应 |
| P1-4 | **「重新生成」是伪实现**：向会话追加一条本地化固定文案（"重新生成上一个回复"）作为新用户消息 | `MessageBubble.tsx:205-207`；`en.json:2107` | 污染对话记录；不针对所点消息；旧答案不替换。与同页存在的真·回退检查点形成刺眼对比 |
| P1-5 | **`isQuerying` 是窗口级单例**：后台会话运行会禁用前台会话的输入框/麦克风/斜杠菜单；停止按钮 `cancelQuery` 指向 `windowSessionId ?? currentSessionId`（前台会话），不是真正在跑的那个 | `AppContext.tsx:69,306-311`；`ComposerPanel.tsx:57` | 多会话并行时输入被无端锁死、停止无效 |
| P1-6 | **脚注预处理器破坏正常 Markdown**：正则无差别剥离 `[^x]:` 行、在任意 `[^…]` 处切分正文，代码块/行内代码/表格中的脚注样式文本同样被处理，且切分会打断列表/表格结构 | `FootnoteMarkdown.tsx:15-25,107-122` | 内容损坏类 bug，静默 |
| P1-7 | **工具卡头部不可键盘操作**：`role="button" tabIndex={0}` 只有 onClick，无 onKeyDown | `ai-elements/index.tsx:111-121` | 键盘用户无法展开/收起任何工具卡（对比 SubagentBlock 用的是真 button） |

**输入区：**

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| P1-8 | **`/` 聚焦快捷键劫持其它输入框**：窗口级 handler 只排除 TEXTAREA，在会话搜索框/命令面板输入 `/` 会被吞字符并抢焦点 | `ChatInput.tsx:291`（对比 `useKeyboardShortcuts.ts:46` 的正确写法） | 可用性 bug |

**Dock 与产物：**

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| P1-9 | **HTML 交互产物在生产环境静默失效**：srcdoc iframe 继承父 CSP，多策略取交集；meta 注入的 `script-src 'unsafe-inline'` 无法放宽父级 `script-src 'self'`（生产），dev CSP 带了 unsafe-inline 所以开发时「看起来正常」。仓库自己的 MermaidRenderer 注释早已承认此行为并因此避开脚本 | `HtmlRenderer.tsx:8,25`；`tauri.conf.json:68`（生产）/`:69`（dev）；`MermaidRenderer.tsx:14-16` | `sandbox="allow-scripts"` 的设计意图完全落空：交互式 HTML 渲染为静态页，无任何报错。第一轮 §5-3 决策的「严格 CSP 第一阶段」实际等价于「完全静态」 |
| P1-10 | **chat 产物无去重**：chip 点击每次 `makeId()` 新开 tab；只有磁盘产物有 `disk:<path>` 复用 id | `ArtifactChip.tsx:36`；`ArtifactContext.tsx:55-57` | 同一内容反复点/重新生成的消息重复渲染，tab 条堆积相同 tab |
| P1-11 | **autoOpen + 虚拟化滚动 = tab 风暴（潜伏）**：autoOpen 靠组件内 `firedRef` 去重，但虚拟列表滚出屏幕即卸载、滚回重挂载，ref 复位→每次滚动周期重开一个新 tab（设置默认关，打开即触雷） | `ArtifactChip.tsx:26-31`；`MessageArea.tsx:133,139` | 打开官方设置里的 autoOpen 后，产物 tab 无上限增长 |
| P1-12 | **每次点击外链都新开网页 tab**：panel 路由 `openArtifact({kind:'web'})` 无 id | `RightDock.tsx:176-181` | 同一 URL 点两次 = 两个一模一样的 tab |
| P1-13 | **Dock 开合状态不持久化**：tab/宽度/全屏都存 localStorage，唯独 `contextPanelOpen` 是普通 useState；重启后恢复的 tab 名自愈到 context | `AppContext.tsx:79`；`RightDock.tsx:53-55,240-244` | 「恢复现场」故事只做了一半 |
| P1-14 | **产物 tab 不随会话清理**：ArtifactProvider 无会话作用域，`closeAll` 全仓无调用者 | `ArtifactContext.tsx:81-84` | 上一个会话的 chat 产物残留在新会话的 Dock 里 |

### P2 —— 打磨项（择要，共 24 项）

**输入区/会话：**

| # | 问题 | 证据 |
|---|---|---|
| P2-1 | 草稿与附件是页面级单例：跨会话串扰、不落盘、不随会话清空 | `Chat.tsx:49-50` |
| P2-2 | 斜杠结果卡跨会话残留（`/cost` 结果跟到下一个会话） | `Chat.tsx:126` |
| P2-3 | 会话切换无 loading 态（IPC await 期间旧消息原地不动），失败只进聊天页共享错误横幅 | `AppContext.tsx:350-368,724-730` |
| P2-4 | 会话列表不虚拟化（消息 >30 条虚拟化，会话永不）；初始 loading 时显示「无会话」空态闪烁 | `SidebarSessions.tsx:879-897`；`Sidebar.tsx:302` |
| P2-5 | 麦克风按钮不检查 `useVoice.supported`，无 provider 时可点开注定失败的录音；TTS `speak()` 实现完整但零调用（死能力），VoiceOrb/MicButton 的 speaking 态不可达 | `useVoice.ts:84,159-168`；`ChatInput.tsx:602-607` |
| P2-6 | 归档会话无「永久删除」出口；删除确认框不显示目标会话名、失败无 pending/反馈 | `SidebarSessions.tsx:925-972`；`DeleteSessionModal.tsx:20-31` |
| P2-7 | localStorage 置顶/排序映射不随会话删除清理，累积陈旧 id | `SidebarSessions.tsx:41-48,416-469` |
| P2-8 | 预算横幅随会话切换清空且不重读持久化状态，切回超支会话不再提示 | `useBudgetGuard.ts:41-43` |
| P2-9 | 斜杠自动补全缺 combobox a11y（无 aria-expanded/controls/activedescendant）；字符计数器 ≥2000 字后每键播报（aria-live） | `ChatInput.tsx:354-385,609-617` |
| P2-10 | ExecutionModeSwitcher 键盘死路：焦点不移入菜单，方向键/Escape 无效，无外部 Escape 监听 | `ExecutionModeSwitcher.tsx:55-64,87-121,138` |
| P2-11 | 硬编码英文串：`Remove ${name}`、`Failed to cancel query`、BudgetBanner/SessionUsageDialog 硬编码 USD 格式化 | `AttachmentChip.tsx:29`；`AppContext.tsx:310`；`BudgetBanner.tsx:36` |
| P2-12 | 生产处理器残留 `console.log('[dbg]')` ×3 | `ChatInput.tsx:123,126,128` |

**渲染管线：**

| # | 问题 | 证据 |
|---|---|---|
| P2-13 | 流式 O(n²)：每个 token 对全文重跑脚注正则 + 全量 ReactMarkdown 解析，长回复越到后端越卡 | `AppContext.tsx:500-512`；`FootnoteMarkdown.tsx:35-39` |
| P2-14 | `detectArtifacts` 每次渲染全量正则扫描消息正文，无 memo | `MessageBubble.tsx:246` |
| P2-15 | Markdown 图片急加载、无尺寸上限、无点击放大 | `Markdown.tsx:229-255` |
| P2-16 | sanitize schema 对所有元素放开 `data-*`（面偏宽）；GitHub schema 的 img src 协议白名单可能剥掉 `convertFileSrc` 的 asset 协议，需平台实测 | `Markdown.tsx:16-33,229-255` |
| P2-17 | 流式日志与虚拟列表整体套 `aria-live="polite"`（SR 刷屏）；MessageHeader 整体 `aria-hidden`（角色/时间戳对 AT 不可见） | `StreamingResponse.tsx:95-98`；`MessageArea.tsx:130`；`MessageBubble.tsx:79` |
| P2-18 | 消息操作工具条纯 hover 显形（opacity-0 group-hover），触屏不可达、键盘不聚焦不显形 | `MessageBubble.tsx:269,422` |
| P2-19 | `QUERY_TOOL_PROGRESS` 捕获了 progress/progress_message 但无任何 UI 渲染——长工具运行只有转圈 | `AppContext.tsx:536-545` |
| P2-20 | diff-stats 缓存按路径终身缓存不失效，旧卡片显示陈旧 +x −y | `diffStats.ts:13-53` |

**Dock/产物：**

| # | 问题 | 证据 |
|---|---|---|
| P2-21 | tablist a11y 结构违规：tabpanel 的 aria-labelledby 指向不存在的 id（`dock-tab-a:<id>` vs 实际 `dock-tab-a-<id>`）；非 tab 按钮混在 role=tablist 内；tab 内嵌套 role=button 关闭 span；resizer 无键盘支持；无方向键导航 | `RightDock.tsx:290-295,315,346,354-405,410` |
| P2-22 | 拖拽调宽与 `transition-all duration-300` 打架（橡皮筋延迟）；宽度 clamp 让 MIN_WIDTH 压过 60% 视窗上限（窄窗 280px>70%）；恢复时不按当前窗宽重新 clamp；无窄窗断点 | `RightDock.tsx:56-58,75-80,142-146,275` |
| P2-23 | 切 tab 即卸载重挂：网页 iframe 丢失页面状态重新加载、文档滚动位置归零、mermaid 重渲染 | `RightDock.tsx:246-247,425-429` |
| P2-24 | Rust 错误以英文子串耦合（`msg.includes('too large')||includes('binary')`），后端改文案/本地化即把优雅降级变成失败 toast；HTML `<head>` 注入大小写敏感、单次替换，`<head id=…>` 等变体漏注入 CSP；检测层漏 `~~~` 围栏/信息串/缩进围栏/`htm`，前 200 字符去重可误并，SVG/mermaid 标题恒为空 | `ArtifactLinkHost.tsx:48`；`HtmlRenderer.tsx:12-18`；`detectArtifact.ts:24,44-50,67` |

其余：图片/SVG/mermaid 无缩放/导出；PDF 无渲染路径（落入 other 卡）；TOC 的解析器与渲染器 id 推导可能漂移、scroll-spy 用全局 getElementById、<lg 视口整体隐藏（与 Dock 宽度无关）；非 en/zh-CN 语言包存在整键未翻译（如 `chat.artifact.autoOpen.aria`）；Dock 面测试覆盖薄（仅 Mermaid/Artifact 快捷键两个测试文件）；死代码（`closeAll`、`openedAt`、`StreamingResponse.headerSlot`、`ContextPanel` 默认导出、`DropdownMenu.triggerRef`、`_currentQueryId`）；图表 spec 校验过浅（NaN 静默出图）。

---

## 4. 改进方案（5 批，每批可独立成 PR）

### B0 止血包（P0 全部 + 低成本高危项）——约 1.5~2 人日

1. **P0-1**：`handleSend` 允许「附件 + 空文本」发送（先验证后端 query 接口接受空 text + files；若不支持则按钮恢复真实禁用态——二选一，不允许现在的假可用）。
2. **P0-2**：拖拽改走 Tauri v2 `getCurrentWebview().onDragDropEvent`（`payload.paths` 已是真实绝对路径），移除 `File.path` 读取；保留 HTML overlay 视觉。注意与 webview `dragDropEnabled` 默认值的关系（Tauri v2 默认拦截 HTML5 DnD，正好单一数据源）。
3. **P0-3**：输入框接 `compositionstart/end`（或 `e.nativeEvent.isComposing`），组合中 Enter/Tab 只提交组词不发送；补 zh 输入法回归用例。
4. **P1-2 + P1-3（流式尾部状态机）**：`QUERY_FAILED/CANCELLED` 清理 `streamingText/thinkingText/activeToolCalls`（残篇按普通 assistant 消息落一条带「已取消」标记，或丢弃——推荐落盘保留残篇，与竞品一致）；「重试」改为重发**最后一条用户消息**（`messages` 里倒查），与 budget「继续一次」同机制。
5. **P1-1（滚动锁定）**：外层滚动容器加「用户是否在底部附近」跟踪（scroll 监听 + 阈值），仅贴底时自动跟随；恢复 MessageArea 已有的回到底部 FAB 作为解锁后的显式操作。删除 StreamingResponse 里的死守卫。
6. **P1-8 + P2-2 + P2-12**：`/` 快捷键排除所有可编辑元素（对齐 `useKeyboardShortcuts.ts:46`）；会话切换时清 `slashResult`；删 debug log。

### B1 消息交互对齐竞品——约 3~4 人日

7. **P1-4 真·重新生成**：复用回退检查点机制（`rewindSessionAction` 已存在）——回退到所点 assistant 消息之前的检查点并自动重发；按钮只出现在最后一条 assistant 消息上（竞品语义）。带确认或可直接执行（建议直接执行 + toast 可撤销提示）。
8. **消息编辑**：用户消息增加「编辑」→ 回退到该 turn + 输入框预填 → 用户确认重发。与 7 共享回退管线。
9. **流式中输入队列**（P1-5 的体验面）：`isQuerying` 不再禁用 textarea，发送时若正在流式则入队（会话内 FIFO，1~3 条上限），完成后依序自动发送；队列 chip 可删。
10. **P1-5 `isQuerying` 按会话化**：改为 `Map<sessionId, QueryState>`；前台会话只受自身运行影响；`cancelQuery` 目标改为「该会话自己的运行」。这一项是 9 的地基，也是多会话并行正确性的前提。
11. **P2-1/P2-3 草稿与切换体验**：草稿按 `draft:<sessionId>` 存 localStorage（切换保留、发送清除）；会话切换加骨架/mini spinner。
12. **会话内搜索**（竞品标配）：Ctrl+F 打开聊天内搜索条（过滤或高亮 + 上下条跳转），虚拟列表下用消息索引定位。

### B2 渲染质量——约 2.5~3.5 人日

13. **P1-6 脚注重构**：放弃自研正则预处理器，直接用 remark-gfm 原生脚注（react-markdown 渲染 `section[data-footnotes]`，用 components 定制样式）。同时解决代码块误伤与切分破坏结构两个 bug，删 ~120 行代码。DocumentRenderer 已用 remark-gfm，聊天管线对齐。
14. **数学公式**：`remark-math + rehype-katex`，KaTeX 走 lazy chunk（约 +300KB 按需加载）；chat 与 DocumentRenderer 共用。竞品标配，研究型对话高频。
15. **P2-13/14 流式性能**：`streamingText` 更新节流（rAF 或 50ms 合帧）；`detectArtifacts` 以消息内容为 key useMemo；虚拟izer key 去掉 index 分量。
16. **P2-15 图片**：`loading="lazy"` + `max-h` 约束 + 点击转入 Dock 图片 tab（复用第一轮的 image renderer，天然获得 reveal/外开/缩放位）。
17. **P1-7 + P2-9/17/18 键盘与 SR**：工具卡头改真 `<button>`；工具条补 focus-visible 常显；流式 aria-live 收敛为状态性播报（开始/结束），消息头部可见性恢复。

### B3 产物面板打磨——约 2.5~3.5 人日

18. **P1-10/11/12 去重三连**：
    - 网页 tab 以 `web:<normalized-url>` 为显式 id，同 URL 复用并激活；
    - chat 产物以内容散列（首 200 字符已有，升级为真正 hash）为 id；
    - autoOpen 去重从组件 `firedRef` 上移为 ArtifactProvider 内按 id 的全局 set（虚拟化重挂载免疫）。
19. **P1-14 会话作用域 + P1-13 持久化**：切换会话清空 `origin==='chat'` 的产物 tab（磁盘产物与网页 tab 保留，磁盘产物带来源标注）；`contextPanelOpen` 入 localStorage，与 tab/宽度/全屏同一套恢复逻辑。
20. **P1-9 HTML 交互产物立场**（§5 决策点 1）：短期先「诚实静态」（sandbox 去掉 allow-scripts、UI 说明 + 突出「用系统打开」），长期按决策引入 `artifact://` 自定义协议承载独立 CSP 的真交互页。
21. **P2-21/22 tablist 与 resize**：aria-labelledby 修正、方向键 tab 导航、关闭按钮移出 tab、resizer 加键盘（左右步进）与 tabIndex；拖拽时禁用 transition；恢复宽度按当前窗宽 re-clamp；MIN/MAX 与 60% 上限统一取交集而非 max 优先。
22. **视觉类渲染器补齐**：图片/SVG/mermaid 统一缩放条（步进按钮 + Ctrl+滚轮）；mermaid 导出 SVG；PDF 点击 → `open_with_default_app` 为主按钮（`other` 卡已具备，改文案突出）。
23. **P2-23 tab 保活（可选）**：仅 iframe 类 tab（web/html）以 `hidden` 保活，文档类仍卸载——iframe 状态丢失代价最大，文档重渲染代价可接受。
24. **P2-24 健壮性**：Rust 侧错误结构化（`{code: 'file_too_large'|'binary', message}`），前端按 code 分支；`<head>` 注入改正则 `/‌<head[^>]*>/i`；检测层补 `~~~` 围栏与 `htm`，SVG/mermaid 从内容提取标题。

### B4 会话与边角——约 2 人日

25. **P2-4/6/7**：会话列表虚拟化（复用 @tanstack/react-virtual）+ loading 骨架；归档区补「永久删除」（二次确认）；pin/order 映射在删除/归档时同步清理。
26. **P2-5 语音收口**：`supported` 为 false 时隐藏麦克风；TTS 二选一——接线（assistant 完成 TTS 播放按钮）或整体删除（含 tts.ts）。
27. **P2-11 等 i18n 清理**：硬编码英文串清单化修复；补其它语言包缺失键（脚本比对 en 基线）。
28. **其余**：diffStats 缓存按会话失效；预算横幅切回重读；错误码化后顺带删 `BudgetDialog` aria 缺口、GoalStartForm 静默吞错等小项。

### 测试与门禁（每批同样适用）

- 前端：`pnpm test:ci`（一次性模式）+ `pnpm lint` + `scripts/check-overlays.sh`（新覆盖层先查白名单）；
- Rust：`cargo nextest run -p shannon-desktop` + `cargo fmt`/`clippy`；
- 新命令/能力必须同步 `desktop/acl/app-permissions.json` + capability 集（ACL 覆盖测试会拦）；
- i18n：en 与 zh-CN 同步补键（CI 无强校验，人工纪律）；
- IME/拖拽/缩放类需真机手测清单（Windows/macOS 重点：canonicalize、拖拽路径、组词回车）。

---

## 5. 决策点（待拍板）

| # | 决策 | 选项 | 建议 |
|---|---|---|---|
| 1 | **HTML 交互产物的终局**（P1-9） | A. `artifact://` 自定义协议 serve 独立 CSP 页面（真交互，Rust 侧 1~1.5 人日）；B. 明确「静态只读」立场：去 allow-scripts + 文案说明 + 系统打开逃生门 | **先 B 止血（B0 顺带），A 列为 P1 目标**。Claude artifacts 支持交互，长期应对齐；但 A 涉及协议注册与安全评审，不宜混在打磨批次里 |
| 2 | **重新生成/编辑的语义**（B1-7/8） | 回退重发（丢旧答案、耗 token，竞品语义） vs 追加式（现状伪实现） | **确认回退重发**。伪实现比不做更伤信任；回退管线现成 |
| 3 | **流式中输入**（B1-9） | A. 队列化（Claude 语义）；B. 保持禁用但文案改为「生成中——输入将在完成后发送」 | **A**。队列 chip 成本低，且是 P1-5 会话化之后的自然收益 |
| 4 | **网页 tab 复用策略**（B3-18） | 同 URL 复用激活 vs 一律新 tab + 手动合并 | **同 URL 复用**；需要并存多副本时用右键「在新 tab 打开」兜底 |
| 5 | **数学公式**（B2-14） | 引入 KaTeX（lazy +300KB） vs 暂缓 | **引入**。研究型对话高频需求，lazy chunk 不伤首屏 |
| 6 | **会话切换的产物清理**（B3-19） | A. chat 产物清空、磁盘/网页保留；B. 全保留 + 来源会话标注 | **A**。标签成本低易错、B 容易变成垃圾堆积；磁盘产物本就跨会话有效 |

---

## 6. 工作量汇总与顺序

| 批次 | 内容 | 人日 | 前置 |
|---|---|---|---|
| B0 | P0×3 + 流式尾部状态机 + 滚动锁定 + 杂项 | 1.5~2 | 无 |
| B1 | 重新生成/编辑/队列/isQuerying 会话化/草稿/会话内搜索 | 3~4 | B0-4（状态机） |
| B2 | 脚注重构/数学/流式性能/图片/键盘 SR | 2.5~3.5 | 无（13 依赖无） |
| B3 | 产物去重/会话作用域/持久化/HTML 立场/tab a11y/缩放导出 | 2.5~3.5 | 无 |
| B4 | 会话列表/语音/i18n/边角 | ~2 | 无 |
| **合计** | | **11.5~15.5** | B0 最优先 |

顺序建议：B0 立即 → B3（去重/持久化是日常使用摩擦最大的 Dock 项）与 B2 并行 → B1（体量最大、依赖状态机）→ B4 收尾。每批独立 PR、目标 `dev`，走既有 20 项 CI 门禁。

---

## 7. 本报告的信息源与置信度说明

- 代码证据：三个独立审查线程全量读取约 25 个源文件并交叉比对；P0 全部 6 条关键断言（按钮禁用条件、`File.path`、无 isComposing、`isQuerying` 单例、FAILED 不清理流式态、生产/开发 CSP 差异）已由主审逐条重读源码确认。P1/P2 条目在实现时请先复测（约 30 秒/条）。
- 竞品信息：`COMPETITIVE-ANALYSIS.md`（2026-06 实测）+ 公开行为常识性总结；2026-09 检索轮未发现对话页交互范式级变化。§2 表中标注「常识」的单元格建议在实施 B1/B3 前对 Claude/ChatGPT 桌面版做一轮 30 分钟实测校准。
