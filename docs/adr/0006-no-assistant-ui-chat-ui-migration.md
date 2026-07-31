# ADR 0006 — Do Not Migrate Shannon Desktop Chat UI to assistant-ui

**Status**: Accepted
**Date**: 2026-07-31
**Theme**: 评估将 `desktop/ui/` 的 Agent 对话 UI 替换为
[assistant-ui](https://www.assistant-ui.com/) 的可行性与必要性
**Supersedes**: —
**Related**: ADR-0005 (Unified Provider/Model/Credential Management —
确立了"engine is source of truth, desktop is projection"原则;
本 ADR 是该原则在 UI 层的延伸)

---

## TL;DR

**不迁移。**

| 维度 | 评级 |
|------|------|
| 必要性 (necessity) | ⚠️ 中低 |
| 可行性 (feasibility) | ⚠️ 中等(有条件) |
| 战略契合度 | ❌ 低 |
| ROI | ❌ 负 |

**取而代之**:保留 Shannon 自研 Chat UI 作为差异化主航道,从 assistant-ui 中
**抽取特定模式**(composable primitives、attachment 处理、generative UI 思路)
补强自研代码。

**重评估触发条件**(见 `Open Questions`):
- assistant-ui 推出**原生 multi-event transport**(而非 fetch-only)
- 用户研究显示 **UI 体验** 而非**功能集**才是 Shannon 增长瓶颈
- Shannon 决定开放 **HTTP 端点**(不只 Tauri IPC)

---

## Context

### Why this evaluation now

截至 2026-07-31,Shannon Desktop 自研 Chat UI 已稳定但**有功能性缺失**:

- ❌ 无 message edit
- ❌ 真 regenerate(当前 `MessageBubble.tsx:141-143` 实际发送固定字符串
  `"Regenerate the previous response"`)
- ❌ 无 slash-command 菜单(`/model`, `/provider` 等没有 UI 触发)
- ❌ 无 in-thread branching(只有 session-level)
- ❌ 无 typing indicator
- ❌ tool-call 渲染为纯 JSON `<pre>`(可读性差)

assistant-ui([官网](https://www.assistant-ui.com/) ·
[GitHub](https://github.com/assistant-ui/assistant-ui))作为 YC 投资、MIT 许可、
周下载量数十万级的 chat UI 库,提供:
> "Production-grade chat UI components, streaming state management, retries,
> attachments, markdown, code highlighting, voice dictation, accessibility,
> Generative UI (tool calls → React components), inline human-in-the-loop
> approval."

值得评估:**是否应该用它替换自研 Chat UI 来补齐以上缺失?**

### Current state — Shannon Desktop Chat UI (audit 2026-07-31)

| 指标 | 数值 |
|------|------|
| 核心 chat 文件 | 7 组件 + 3 context + 158 i18n keys × 2 语言 |
| 代码量 | `Chat.tsx` 781 行 / `ChatInput.tsx` 446 行 / `MessageBubble.tsx` 365 行 / `ai-elements/index.tsx` 自研仿 Vercel AI Elements |
| 已具备的 ChatGPT 风格能力 | streaming / markdown / code copy / file attach / voice / tool card / approval mode / model picker / branching / token usage / context window bar / virtual scroll / artifacts / charts / research report / Markdown+PDF 导出 / session sidebar |
| 缺失能力 | message edit / 真 regenerate / slash-command / in-thread fork / typing indicator / inline approval / 子 agent 可视化 |
| 测试覆盖 | 8 chat 测试文件,但**自 import 后无新增**;vitest 总体覆盖率 **34%**(门槛 80%) |
| 依赖与 assistant-ui 兼容性 | React 19 / TS strict / Vite / vitest / tailwind v4 — **全部匹配** |

**Tauri 事件桥(13 个事件)**:
```
query:text              (streamed token delta)
query:thinking          (reasoning delta)
query:tool-start        (tool invocation)
query:tool-progress     (phase update)
query:tool-result       (final result)
query:usage             (token counts)
query:completed         (terminal commit)
query:failed            (terminal error)
query:cancelled         (terminal cancel)
permission-request      (engine → UI approval prompt)
sessions-updated        (cache invalidation)
config-updated          (cache invalidation)
background-tasks-updated
```

**传输本质**: Rust `commands_chat.rs` 处理 `send_message`,然后通过
`@tauri-apps/api/event::listen` 推送事件流 — **不是 SSE,不是 WebSocket,
不是 fetch**。这是**多事件类型并发推送**,不是单一 HTTP 响应流。

---

## assistant-ui 能力盘点

来源:[官网](https://www.assistant-ui.com/) ·
[GitHub README](https://github.com/assistant-ui/assistant-ui) ·
[Docs](https://www.assistant-ui.com/docs)

| 维度 | 现状 |
|------|------|
| 定位 | "React primitives for building AI chat interfaces" |
| 核心抽象 | `Thread` / `Message` / `Composer` / `ThreadList` / `ActionBar` — composable React primitives |
| 后端 adapter | Vercel AI SDK / LangGraph / LangChain / AG-UI / A2A / Google ADK / OpenCode / custom data-stream / Assistant Cloud |
| Runtime 模型 | 自带 thread state + streaming + retries + attachments |
| 默认 transport | `useChatRuntime`(fetch-based,SSE 响应流) |
| Generative UI | "Render tool calls and JSON as React components, collect inline human approvals" |
| CLI | `npx assistant-ui@latest init` — 复制 styled components 到项目 |
| 样式 | 注入自家 CSS(基于 `tailwindcss-animate`);默认 Base UI(也是 Base UI)/ Radix 主题 |
| License | MIT,可选 Assistant Cloud(managed persistence + analytics) |

**关键架构事实**: assistant-ui runtime 默认要求**单一响应流**(fetch 或 SSE)。
Shannon 是**多事件类型并发推送** — 需要写自定义 Runtime adapter。

---

## Evaluation

### 1. Tech-stack 兼容性

| 维度 | Shannon | assistant-ui 要求 | 兼容 |
|------|--------|------------------|------|
| React | 19.0.1 | ≥19 | ✅ |
| TypeScript | strict | strict | ✅ |
| 状态管理 | Context only | 自带 runtime | ✅ |
| 路由 | react-router 7 | 无路由依赖 | ✅ |
| Markdown | react-markdown + GFM + sanitize + highlight | 同款 | ✅ |
| 样式 | tailwind v4 + base-ui | 注入 tailwind classes,需要 `tailwindcss-animate` + `tailwindcss-animate-css` | ⚠️ 需合并 config |
| Icons | lucide-react + Material Symbols | 不绑定 | ✅ |
| 测试 | vitest + @testing-library/react | 支持 | ✅ |
| i18n | react-intl v7 | 自带 i18n.tsx provider | ⚠️ 需桥接或重写 |

### 2. Gap analysis — assistant-ui 加什么 / 减什么

#### ✅ assistant-ui 能提供的现成能力

| 能力 | Shannon 现状 | assistant-ui 提供方式 | 净收益 |
|------|-------------|--------------------|--------|
| Message edit | ❌ | 内置 `<MessagePart>` 编辑 API | 显著 |
| 真 regenerate | 🟡 stub | 内置 retry API | 显著 |
| Slash-command 菜单 | ❌ | `Composer` 内置 `/` 触发器 | 显著 |
| In-thread branching | 🟡 session-level | 内置 message-level fork | 中等 |
| Typing indicator | ❌ | 内置 | 微小 |
| Generative UI (tool → React) | ❌ 纯 JSON | 一等公民 | 战略级 |
| Inline human-in-the-loop approval | 🟡 全局 modal | 内嵌 message 流 | 中等 |
| 真 retry / 错误恢复 | 🟡 stub | 一等公民 | 中等 |

#### ❌ assistant-ui 不能覆盖 / 必须自建的引擎耦合部分

| Shannon 独有特性 | 为什么 assistant-ui 帮不上忙 |
|----------------|--------------------------|
| **Rust engine 事件桥**(13 个 Tauri events) | assistant-ui 是 HTTP/SSE 请求-响应模型;**必须写自定义 transport adapter** |
| **Worktree 隔离**(`createSessionInWorktree` + `context.working_directory`) | 引擎概念,assistant-ui 无对应 |
| **Approval mode 5 档**(readonly/plan/suggest/auto/full_auto) | Shannon 引擎独有 |
| **Engine-side branching**(`branchSession(parentId, messageIndex)`) | 不是 message-level fork |
| **Files-changed diff bar** + `FILE_MUTATING_TOOLS` regex | 引擎 tool 元数据驱动 |
| **Permission modal 流**(`permission-request` 事件) | 引擎 4-tier classification |
| **Hook event 系统**(32 个 event types) | Shannon 自有架构 |
| **Plan-mode banner + Ctrl+Shift+P** | 引擎策略 |
| **Voice input via `useVoice` + STT** | Shannon 已集成;assistant-ui dictation API 不同 |
| **Print/PDF + Markdown export** | Shannon 现有 |
| **158 个 `chat.*` i18n keys**(en + zh-CN 配对) | 需重写为 assistant-ui `useTranslations` 格式 |
| **离线 demo mode**(`VITE_MOCK_MODE=1` + `coreMock.ts`) | 必须保留 |
| **Code-sign / context-window bar / token usage** | Shannon 现有但细节不同 |

### 3. 可行性分析

#### 3.1 关键架构差异 — 传输层

这是最大的设计摩擦:

| | Shannon | assistant-ui |
|--|--------|-------------|
| 启动 | `invoke('send_message')` | `fetch('/api/chat')` |
| 推送方式 | **13 个并发 Tauri 事件** | **单一 SSE 响应流** |
| 取消 | `invoke('cancel_query')` + 等待 `query:cancelled` | `AbortController` |
| Tool calls | 独立 `query:tool-start/result/progress` 事件 | 内嵌在 SSE 流里的 tool_calls |
| 状态 | 服务端持久 + 客户端 mirror | 客户端持有完整 thread |

**结论**:必须写 `TauriAssistantRuntime implements ChatModelAdapter`,
监听 13 个 Tauri events 并翻译为 assistant-ui 的 `MessageChunk` 流。
**估算工作量**: 2 周(含事件→chunk 翻译 + 错误协议对齐 + 取消协议对齐)。

#### 3.2 i18n 重写

158 keys × 2 语言 = **316 条翻译需重写**或写 `react-intl` ↔ assistant-ui 桥。
**零增量价值**。

#### 3.3 自研 vs 复用边际收益

| 模块 | 自研 LOC | assistant-ui 复用 | 节省 | 风险 |
|------|---------|------------------|------|------|
| ChatInput / Composer | 446 行 | ~0(高定制) | 小 | 失去 voice / attach / approval mode |
| MessageBubble / Message | 365 行 | 部分(themed Message) | 中 | 失去 file-mutating diff bar |
| StreamingResponse | 53 行 | 内置 | 中 | — |
| ai-elements (8 组件) | ~200 行 | 全部替换 | 大 | API 学习成本 |
| FooterMarkdown | 48 行 | 内置 Markdown | 中 | — |
| ToolCallDisplay | 60 行 | 内置 Tool UI | 大 | 失去"Files changed"bar |
| WelcomeState | 50 行 | 内置 | 小 | — |
| **合计 chat 核心** | **~1200 行** | **~600 行** | **~50%** | **高** |

**节省 600 行,工程改造 ≈ 8-12 周。不划算**。

### 4. 必要性分析

#### 4.1 用户痛点 vs 阻塞性

| 痛点 | 当前 | 是否阻塞? | assistant-ui 解决? |
|------|------|----------|------------------|
| 无法编辑已发送消息 | ❌ | 🟡 中(常见需求) | ✅ |
| Regenerate 假实现 | ❌ | 🟡 中 | ✅ |
| 没有 slash-command | ❌ | 🟢 低(高端用户偏好) | ✅ |
| 没有 inline approval | 🟡 全局 modal | 🟢 低 | ✅ |
| 没有 message-level fork | 🟡 session-level | 🟢 低 | ✅ |
| Tool-call 可读性差(纯 JSON) | 🟡 | 🟡 中 | ✅(Generative UI) |
| 性能 / 滚动 / 流畅度 | ✅(虚拟滚动) | — | — |
| 视觉一致性 / 主题 | ✅(Tailwind + Material) | — | — |

**结论**:有缺失但**无阻塞性功能缺口**。所有缺失项可在 2-4 周内独立补齐。

#### 4.2 战略层面

Shannon **核心差异化 ≠ ChatGPT 风格对话**:

- 多 agent 编排(`TeamCreate` / `SendMessage` / `TaskCreate`)
- Worktree 隔离(`/batch` + 自动 worktree)
- Hook system(32 事件)
- Routine system(triggered + scheduled)
- Permission profiles(named presets)
- Engine ↔ Desktop 边界清晰(ADR-0005 完整治理)

**Chat 表面做得"像 ChatGPT"对 Shannon 不是差异化** — 它只是**及格线**。
把工程资源花在"做到 95% ChatGPT UI"是**追赶**而不是**领跑**。

---

## Three options compared

### Option A — Keep status quo + incremental improvements (RECOMMENDED ⭐⭐⭐⭐⭐)

**做法**:
1. 把现有 1181 个 vitest 覆盖率从 34% 提升到 60%(优先 chat surface)
2. 补齐缺失能力:`message edit` / `real regenerate` / `slash-command menu`
   — **3 个独立 PR,各 1 周**
3. **Generative UI** 升级 tool-call 展示:扩展 `ai-elements/index.tsx`,
   自研 tool → mini-component 渲染(借鉴 Vercel AI Elements + LangChain
   generative UI 思路,**不引入** assistant-ui)
4. 保留所有 engine-coupled 定制

**ROI**:高。零 vendor 锁定。复用 781 行 `Chat.tsx` 现有架构。

### Option B — Hybrid: assistant-ui as message-part renderer only (⭐⭐⭐)

**做法**:
1. 保留 Shannon 自研 ChatInput / ToolCallDisplay / Approval modal / Voice
   等 engine-coupled 部分
2. 引入 assistant-ui 的 `<Thread>` / `<Message>` / Markdown 渲染 pipeline
   作为**只读渲染层**
3. 在 transport 层写 Tauri → assistant-ui chunks 适配器(2 周)
4. 158 个 i18n keys 迁移到 assistant-ui 格式(1 周)
5. 保留 `AppContext` 中所有事件订阅,作为 assistant-ui runtime 的输入源

**ROI**:中。获得部分 UX 提升,保留控制权。
**风险**:长期会被 assistant-ui 演进方向带跑。

### Option C — Full migration (⭐⭐)

**做法**:砍掉所有自研 chat 代码,用 assistant-ui 全套 primitives 重建。

**ROI**:**负**。预计 8-12 周工作量,获得 1-2 个 UX 改进
(message edit / slash command),同时**失去**:

- Worktree 隔离 UX
- Files-changed diff bar
- Plan-mode UI
- Engine-coupled approval
- 多 diff 模态框
- Session-level branching UI 整合
- i18n 完整控制
- engine 事件桥的透明可观测性
- mock-mode 离线 demo

---

## Decision

**采用 Option A(保持现状 + 增量改进)**。

**核心理由**:
1. **必要性不足**:无阻塞性功能缺失;现有 UX 已达到 ChatGPT 风格及格线
2. **可行性有条件**:技术栈兼容,但需写 Tauri↔assistant-ui transport bridge(2 周),
   i18n 全量重写(1 周),测试体系半重建(2 周)
3. **战略契合度低**:Shannon 差异化是 agent 编排 + 多工具链 + worktree,assistant-ui
   是 L1 对话体验库 — **错位**
4. **架构方向冲突**:Shannon 是事件驱动 Rust engine;assistant-ui 是 HTTP
   request/response — **直接对立**
5. **ROI 负**:8-12 周工程 vs. 1-2 个 UX 改进,代价超过收益
6. **Vendor 锁定**:核心 chat 交互交给第三方运行时,长期受上游 breaking change 影响

**采用 Option A 的子项**:

| PR | 内容 | 优先级 | 估算 |
|----|------|-------|------|
| 短期 | 提升 chat surface 覆盖率 34% → 60% | P0 | 1 周 |
| 短期 | 真正的 message edit | P1 | 3 天 |
| 短期 | 真正的 regenerate(替换字符串 hack) | P1 | 2 天 |
| 短期 | Slash-command 菜单(`/model`, `/provider`, `/clear`, `/help`) | P1 | 1 周 |
| 短期 | Inline typing indicator + tool 阶段时间线 | P2 | 3 天 |
| 中期 | Generative UI 升级 tool 渲染(bash/read_file/edit_file/web_search/...) | P1 | 2 周 |
| 中期 | 真 in-thread branching | P2 | 1 周 |

**中期 Generative UI 自研路径**:
扩展 `desktop/ui/src/components/ai-elements/index.tsx`,无需引入 assistant-ui。
新增 tool-specific component 库:

| Tool | 渲染策略 |
|------|---------|
| `bash` | 输出卡片 + exit code badge + duration |
| `read_file` | 折叠 syntax-highlighted 预览 |
| `web_search` | 结果卡片网格 |
| `edit_file` / `apply_patch` | 完整 diff(unified / split) |
| `list_directory` | 文件树 / 表格 |
| Task / Agent spawn | 子 agent 状态卡 |

---

## Consequences

### Positive

- ✅ 保留所有 engine-coupled 能力(worktree、approval mode、diff bar、
  branching、permission modal、hooks、routines、voice、export)
- ✅ 零 vendor 锁定 — Shannon 拥有完整 chat UI 所有权
- ✅ 架构一致性 — 事件驱动 Rust engine ↔ 事件桥接 UI,transport 不扭曲
- ✅ 完整 i18n 控制权
- ✅ 维持 mock-mode(`pnpm demo`)离线开发能力
- ✅ 短期补齐缺失(message edit / regenerate / slash)的 ROI 远高于迁移
- ✅ Generative UI 自研可针对 Shannon 工具链定制(更可读)

### Negative

- 🟡 失去 assistant-ui 上游的 ChatGPT-style 持续打磨(滚动/键盘/a11y 细节)
- 🟡 必须自己实现 typing indicator / 真 regenerate / slash-command —
  但这些是已知范围,3 个 PR 即可
- 🟡 失去 assistant-ui 社区 visibility(对**外部**用户不是问题,对**潜在贡献者**是)
- 🟡 自研 chat surface 的设计债务持续累积,需持续投资 UI/UX 细节

### Neutral

- 🟢 Shannon Chat UI 与 assistant-ui 的 feature gap 会**暂时扩大**,但补齐 PR
  会快速缩小
- 🟢 Tailwind v4 + assistant-ui 默认 tailwind config **不引入** — 无样式冲突
- 🟢 测试覆盖提升与功能补齐**同步推进**,有助于把覆盖率从 34% 推至 60%+

---

## Alternatives Considered

### Option B (hybrid — assistant-ui as renderer only)

**拒绝理由**:
- 长期被 assistant-ui 演进方向带跑,失去对 chat 演进的主动权
- 自研 input + 第三方 render 的混合架构会增加后续维护成本
- i18n 桥接层是持续维护负担
- 测试体系半重建

### Option C (full migration)

**拒绝理由**:
- 8-12 周工作量
- 失去所有 engine-coupled UX 能力(worktree/diff bar/approval mode/branching/
  permission modal/hooks/export)
- vendor 锁定核心交互
- 架构方向冲突(事件桥 → HTTP request/response)

### Defer entirely

**拒绝理由**:用户痛点虽不阻塞但**真实存在**;完全不做会让用户在 ChatGPT
/ Claude.ai 体验持续提升的背景下感知到 Shannon 的 UX 落后。

---

## Implementation References

### Code surfaces (current state)

| Surface | Path | LOC |
|---------|------|-----|
| Chat page | `desktop/ui/src/pages/Chat.tsx` | 781 |
| Composer | `desktop/ui/src/components/chat/ChatInput.tsx` | 446 |
| Message bubble | `desktop/ui/src/components/chat/MessageBubble.tsx` | 365 |
| Streaming response | `desktop/ui/src/components/chat/StreamingResponse.tsx` | 53 |
| Markdown | `desktop/ui/src/components/chat/Markdown.tsx` | ~150 |
| Footnotes | `desktop/ui/src/components/chat/FootnoteMarkdown.tsx` | 48 |
| Chart | `desktop/ui/src/components/chat/Chart.tsx` | ~250 |
| Research report | `desktop/ui/src/components/chat/ResearchReportModal.tsx` | ~300 |
| ai-elements primitives | `desktop/ui/src/components/ai-elements/index.tsx` | ~200 |
| State slice | `desktop/ui/src/context/AppContext.tsx:107-308` | ~200 |
| State slice (chat only) | `desktop/ui/src/context/ChatContext.tsx` | 30 |
| Tauri API | `desktop/ui/src/lib/tauri-api.ts` | 1403 |
| i18n (chat keys) | `desktop/ui/src/i18n/locales/{en,zh-CN}.json` | 158 keys × 2 |
| Engine bridge | `desktop/src/{commands.rs, commands_chat.rs, events.rs}` | — |

### Chat tests

| Test | Path |
|------|------|
| Chat page | `desktop/ui/src/__tests__/Chat.test.tsx` (499 LOC) |
| Composer | `desktop/ui/src/__tests__/ChatInput.test.tsx` (307 LOC) |
| Streaming | `desktop/ui/src/__tests__/StreamingResponse.test.tsx` (83 LOC) |
| Footnotes | `desktop/ui/src/__tests__/FootnoteMarkdown.test.tsx` (48 LOC) |
| Chart | `desktop/ui/src/__tests__/Chart.test.tsx` (81 LOC) |
| Research report | `desktop/ui/src/__tests__/ResearchReportModal.test.tsx` (106 LOC) |
| Files-changed bar | `desktop/ui/src/__tests__/components/FilesChangedBar.test.tsx` (164 LOC) |
| Plan-mode toggle | `desktop/ui/src/__tests__/components/PlanModeToggle.test.tsx` (153 LOC) |

### External references

- assistant-ui: [官网](https://www.assistant-ui.com/) ·
  [GitHub](https://github.com/assistant-ui/assistant-ui) ·
  [Docs](https://www.assistant-ui.com/docs)
- 对比基线:[Vercel AI Elements](https://vercel.com/ai-elements) ·
  [LangChain generative UI](https://langchain.com/) ·
  [OpenCode chat](https://opencode.ai/)
- 项目 ADR: ADR-0005(`docs/adr/0005-unified-provider-model-credential-management.md`)

---

## Open Questions / Re-evaluation Triggers

如果以下任一条件满足,**应重新评估**是否迁移到 assistant-ui(或类似 library):

1. **assistant-ui 推出原生 multi-event transport adapter** — 而非 fetch-only
2. **用户研究/反馈显示 UI 体验**(而非功能集)是 Shannon 增长瓶颈
3. **Shannon 决定开放 HTTP 端点**(不再只 Tauri IPC),允许远程访问 engine
4. **assistant-ui 引入 first-class worktree / branch / diff-aware tool UI**
   (目前没有)
5. **Shannon 决定将 desktop 拆为 web frontend + Rust backend**,那时 HTTP
   request/response 是正确的传输层

在这些条件触发**之前**,本 ADR 的决策保持有效。

---

## Appendix A: 工程估算(Option A vs Option C)

| 项目 | Option A (推荐) | Option C (full migration) |
|------|-----------------|---------------------------|
| 写 transport adapter | 0 | 2 周 |
| i18n 重写 | 0 | 1 周 |
| chat surface 重写 | 0 | 4 周 |
| 测试重写 | 1 周(增量) | 3 周 |
| Generative UI tool 渲染 | 2 周(自研) | 0(库自带) |
| Message edit / regenerate / slash | 1 周(自研) | 0(库自带) |
| 设计/UX 打磨 | 持续 | 0(库自带) |
| **合计 to "ChatGPT parity"** | **4-5 周** | **10-12 周** |
| **剩余 engine-coupled 能力保留** | **全部** | **部分丢失** |
| **vendor 锁定风险** | **无** | **高** |

---

## Appendix B: Information sources

### Shannon local audit

- `desktop/ui/src/{pages/Chat.tsx, components/chat/*, context/AppContext.tsx,
  lib/tauri-api.ts, __tests__/*}`
- `desktop/src/{commands.rs, commands_chat.rs, events.rs}`
- ADR-0005:`docs/adr/0005-unified-provider-model-credential-management.md`
- 自上次 import(`ed9a8ceb`)以来 **chat 测试零新增** — surface 未被主动打磨

### External sources

- [assistant-ui 官网](https://www.assistant-ui.com/)
- [assistant-ui GitHub README](https://github.com/assistant-ui/assistant-ui)
- [assistant-ui 文档](https://www.assistant-ui.com/docs)
- Web search 确认:周下载量级别([npm stats](https://www.npmjs.com/package/@assistant-ui/react)),
  MIT 许可,YC 投资