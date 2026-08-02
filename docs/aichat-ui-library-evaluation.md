# AI Chat UI 组件库评审(shannon-desktop)— v2

> 评审日期:2026-08-02(v2,基于用户评审反馈重构)
> 评审视角:高级产品经理 + 高级架构师
> **独立性声明**:本评审基于当前代码现状与 2026 年公开资料独立进行,不参考任何此前的 assistant-ui / 竞品 UI 评审报告或相关 ADR。
> **v2 触发条件**:用户评审确认 shannon-desktop 的 AI chat 页面**确定要做**附件上传 / 语音消息 / 多线程管理 / 整体美化升级 —— 这恰好**激活了 v1 设定的"阶段 3 全量评估"触发条件**([v1 §6.4](#))。因此 v2 重新裁决。
> 关联:[competitor-feature-matrix.md](./competitor-feature-matrix.md)、[improvement-plan-2026-08.md](./improvement-plan-2026-08.md)、[project-review-2026-08.md](./project-review-2026-08.md)

---

## 0. TL;DR(v2 结论)

| 维度 | v1 结论 | **v2 结论** |
|---|---|---|
| **是否必须迁移?** | 否(收益不显著大于成本) | **是 —— 现在应当采纳**,作为 chat 升级的骨架 |
| **为什么变了?** | — | 用户确认要做**附件/语音/多线程/美化** 4 项,其中**附件 + 多线程 + 美化 3 项正是 assistant-ui 的强项**。runtime adapter 成本从"为小优化付费"变为"摊薄到 4 项新功能",ROI 翻转。 |
| **推荐方案?** | 选择性采纳(阶段 0–2),阶段 3 暂缓 | **以 assistant-ui 为 chat 升级骨架**:① 先做 Tauri runtime adapter(关键使能件);② 用 assistant-ui Thread 重构多线程;③ 用 attachment primitives 做附件;④ 用 production 组件做美化;⑤ 语音(whisper-rs)独立做,作为输入模态接入。 |
| **可行性?** | 高(栈同源) | **高(不变)**。React 19 + Tailwind 4 + shadcn + Base UI 完全同源;runtime adapter 是核心工程。 |
| **首选库?** | assistant-ui / AI Elements | **assistant-ui**(主骨架)+ 保留自研领域组件(图表/研究报告)。 |

**一句话**:既然要大改 chat,就借这次机会上 assistant-ui,而不是在 ~2600 LOC 自研上逐个功能手搓。这是 v2 与 v1 的根本区别。

---

## 1. 候选库全景(2026,不变)

| 库 | 框架 | 定位 | 对 Shannon 适用度 |
|---|---|---|---|
| **[assistant-ui](https://www.assistant-ui.com/)** | React(TS) | 生产级 ChatGPT 风格:组件 + runtime + primitives | ⭐⭐⭐⭐⭐ **v2 首选骨架** |
| **[Vercel AI Elements](https://elements.ai-sdk.dev/)** | React(shadcn) | 可组合元素 | ⭐⭐⭐⭐ 局部补充 |
| **[CopilotKit](https://github.com/copilotkit/copilotkit)** | React | 全栈 agentic + GenUI | ⭐⭐ 方向不匹配 |
| **[NLUX](https://nlux.dev/)** | 多框架 | ChatGPT 风格 | ⭐⭐ 深度浅 |
| **[Deep Chat](https://deepchat.dev/)** | 框架无关 | widget | ⭐ 不匹配 |
| **[TanStack AI](https://tanstack.com/)** | 框架无关 | headless | ⭐⭐⭐ 观望 |

详细特性见 v1 §3(本仓历史),此处不重复。**v2 聚焦 assistant-ui 作为骨架的可行性与方案。**

---

## 2. Shannon 桌面 chat 现状(证据基线,不变)

- **技术栈**:React 19 + Vite 6 + Tailwind 4 + CVA/clsx/tailwind-merge + lucide-react + **@base-ui/react** + react-markdown/rehype/remark + @uiw/react-codemirror + @tanstack/react-virtual + motion + sonner + react-intl v7。
- **与 assistant-ui 同源度**:视图层/样式层**完全同源**(连 Base UI 都一致 —— shadcn 在 2025 从 Radix 迁到 Base UI,assistant-ui 跟进)。唯一差异在**数据/运行时层**:Shannon 用 Tauri 事件流,assistant-ui 假设 JS AI SDK / HTTP。
- **自研规模**:`~2613 LOC`(`pages/Chat.tsx` 781 行 + `components/chat/` 7 文件 + `ChatContext.tsx`),含图表/研究报告/脚注 Markdown 等**领域组件**(assistant-ui 不提供,需保留适配)。
- **已有 `components/ai-elements/index.tsx`**(薄封装)+ **`lib/voice/` 目录**(语音基建已存在!)。

---

## 3. 为什么 v2 翻转裁决(ROI 重算)

v1 的核心论据是"自研已工作 + 领域组件需重做 + runtime adapter 非平凡 → 全量迁移收益小于成本"。这个论据在**"只优化体验"**的前提下成立。但用户评审确认了 4 项**新功能**,ROI 重算如下:

| 新功能 | assistant-ui 现成度 | 自研成本 | 采纳 assistant-ui 的节省 |
|---|---|---|---|
| **附件上传** | ✅ attachment primitives + Composer 内置 | 中(文件读取/类型/预览/chip) | **高** |
| **多线程管理** | ✅ Thread 抽象是核心能力 | 高(并行会话/切换/fork UI) | **极高** |
| **整体美化** | ✅ production 组件开箱 | 高(设计 token/动画/滚动锚定) | **高** |
| **语音消息** | ❌(不提供) | 高(whisper-rs/采集/波形) | 无(独立做) |

**关键洞察**:4 项里 **3 项是 assistant-ui 的强项**。如果不采纳,意味着在 ~2600 LOC 自研上手搓 3 个 assistant-ui 已经解决的问题 —— 这是重复造轮子。而 runtime adapter 的成本(~3–5d)**一次性付清后,4 项功能全部受益**。ROI 从 v1 的"负"翻转为 v2 的"正"。

**v1 的"领域组件需重做"反对意见仍然成立**,但解法不是"放弃迁移",而是"**assistant-ui 作骨架 + 领域组件作为 custom message parts 注入**"。Shannon 的图表/研究报告不丢,作为 `<CustomPart>` 接入 assistant-ui 渲染管线。

---

## 4. 必要性分析(v2 重做)

### 4.1 支持采纳(力度显著增强)

| 理由 | v1 强度 | **v2 强度** | 说明 |
|---|---|---|---|
| 附件上传 | 低 | **高** | assistant-ui Composer 原生支持,省自研 |
| 多线程管理 | 低 | **极高** | Thread 抽象是 assistant-ui 核心差异化,自研成本最高 |
| 美化升级 | 低 | **高** | production 组件 + 滚动锚定 + 动画,直接达成 |
| 流式滚动锚定 | 中 | 中(不变) | 长流式输出抖动,assistant-ui 成熟 |
| 消息状态机 | 中 | 中(不变) | tool/thinking/artifact 多态消息 |
| 社区红利 | 低 | 中 | 4 项功能由社区持续维护 |

### 4.2 反对采纳(v1 理由,逐条复核)

| 理由 | v1 | **v2 复核** |
|---|---|---|
| 自研已工作 | 高 | **削弱** —— 既然要大改(4 项新功能),"已工作"不再是保留理由 |
| 领域组件需重做 | 高 | **可控** —— 作为 custom parts 注入,不丢弃 |
| runtime 集成非平凡 | 高 | **摊薄** —— 一次 adapter,4 功能受益 |
| 体积/依赖 | 中 | **可管理** —— assistant-ui tree-shake 友好,阶段测包体 |
| 当前优先级 | 中 | **已被用户决策覆盖** —— chat 升级已是确认任务 |

### 4.3 必要性裁决(v2)

**采纳 assistant-ui 的必要性现在成立。** 不是为"统一"而采纳,而是因为这 4 项功能里 3 项是 assistant-ui 强项,采纳是最省成本的实现路径。

---

## 5. 可行性分析(不变,要点重述)

- **视图/样式层**:✅ 几乎无障碍(Base UI 同源)。
- **运行时层**:⚠️ 非平凡但可控 —— 写 `ShannonTauriRuntime`(`ExternalStoreRuntime`),订阅 Tauri 事件 → assistant-ui 消息状态。核心工程 ~3–5d。
- **领域组件**:作为 custom message parts 注入,逐个适配。
- **风险**:runtime 接口 beta 期变动 → 锁版本 + snapshot 测试。

---

## 6. 推荐实施方案(v2,以 assistant-ui 为骨架)

### 6.1 总策略

**不是"迁移到 assistant-ui",而是"借 chat 升级的机会,以 assistant-ui 为骨架重建 chat"**。保留 Shannon 领域组件(图表/研究报告/脚注),替换通用 chat 基础设施(消息列表/输入/线程/附件/滚动)。

### 6.2 五步实施(详细工程见 [improvement-plan-2026-08.md](./improvement-plan-2026-08.md) 桌面 chat 升级任务块)

| 步骤 | 内容 | 关键产物 | 估时 |
|---|---|---|---|
| **① Runtime adapter** | 写 `ShannonTauriRuntime`,实现 `ExternalStoreRuntime`/`ChatModelAdapter`,桥接 Tauri 事件 → assistant-ui 消息状态 | `lib/runtime/shannonTauriRuntime.ts` | 3–5d |
| **② Thread 多线程** | 用 assistant-ui Thread 抽象重构;后端复用 `commands_sessions.rs` 的 branch_session;前端线程侧栏/tab | `components/chat/ThreadSidebar.tsx` | 4–6d |
| **③ 附件上传** | assistant-ui attachment primitives + Composer;后端 `commands_files.rs` 加 `read_attachment`;前端 chip + 拖放 | `components/chat/AttachmentChip.tsx` | 3–5d |
| **④ 美化升级** | assistant-ui production 组件替换 MessageBubble/Composer;设计 token 对齐;暗色模式;流式动画;滚动锚定 | 重构后的 `MessageBubble`/`Chat.tsx` | 5–7d |
| **⑤ 语音消息** | whisper-rs 本地 STT sidecar;复用已有 `lib/voice/`;mic 采集 + 波形 + 转写注入 composer | `lib/voice/whisper.ts` + mic UI | 5–7d |

**总计 ~4–6 人周**(单人)。可与 [improvement-plan](./improvement-plan-2026-08.md) Wave 3 的其他桌面任务并行。

### 6.3 为什么这个方案最优

1. **一次 adapter,五处受益**:runtime adapter 是唯一"非社区标准"的工程件,付清后 ②③④⑤ 都建立在它之上。
2. **领域组件不丢**:图表/研究报告作为 custom parts 保留,Shannon 特性不缩水。
3. **语音走 Rust**:whisper-rs 与 Shannon 的 Rust 哲学一致,离线、隐私、无 API 费用。
4. **多线程是最大赢家**:Thread 抽象是自研成本最高、最容易做错的部分,交给 assistant-ui 风险最低。
5. **可逆**:① 是 spike 性质,不满意可在步骤②前回退;② 之后才不可逆。

### 6.4 不推荐的做法(v2)

- ❌ **在自研上手搓 4 项功能**:重复造轮子,多线程尤其易碎。
- ❌ **引入 CopilotKit**:GenUI 方向不匹配,体积大。
- ❌ **为语音引入云 Whisper API**:与本地优先/隐私哲学冲突,选 whisper-rs。

---

## 7. chat 升级详细需求(任务 b 输入)

本节是用户评审点 (b) 的直接产物 —— 4 项功能的**详细需求与实现要点**,供 [improvement-plan](./improvement-plan-2026-08.md) 桌面 chat 升级任务块细化。

### 7.1 附件上传

**需求**:用户可在输入栏上传文件(图片/文档/代码/表格),作为消息上下文发给 LLM。
- 支持:图片(png/jpg/webp/gif)、文本/代码(md/js/py/rs/json/...)、PDF(提取文本)、常见文档。
- 交互:点击附件按钮选文件 + **拖放到输入区** + 多文件 + 预览 chip + 删除。
- 限制:单文件大小上限(如 25MB)、类型 allowlist、总附件数上限。
- **后端**:`desktop/src/commands_files.rs` 加 `read_attachment(path) -> {mime, base64?, text?}`;图片→`ContentBlock::Image`,文本→`ContentBlock::Text`,PDF→复用 /pdf 模块(或外部提取)。
- **前端**:assistant-ui attachment primitives;新建 `components/chat/AttachmentChip.tsx`;`lib/tauri-api.ts` 加 `readAttachment()`。
- Tauri `@tauri-apps/plugin-dialog`(已是依赖)提供文件选择器。

### 7.2 语音消息

**需求**:用户可按住/点击麦克风录音,自动转写为文本填入输入栏。
- **方案选型**:**whisper-rs(本地,离线,隐私,Rust 原生)** —— 与 Shannon 哲学一致。云 Whisper API 仅作可选回退。
- 模型档位:tiny/base/small/medium(设置可选,体积 75MB~1.5GB)。
- 语言:支持多语(含中文)。
- 交互:输入栏麦克风按钮 → 录音指示 + 波形动画(motion) → 停止 → 转写 → 注入 composer。
- **后端**:Rust 命令 `transcribe_audio(path, model, lang) -> text`,用 whisper-rs;复用已有 `desktop/ui/src/lib/voice/`。
- **前端**:Web Audio API 采集 → 编码(wav) → 发 Rust;mic UI + 波形。
- **优先级**:与附件/多线程并行,但可后置(语音不影响 chat 主干)。

### 7.3 多线程管理

**需求**:用户可同时开多个并行对话线程,各自独立上下文,可切换/新建/fork。
- 线程 = 独立 QueryEngine 会话;`commands_sessions.rs` 已有 new/list/switch/branch_session —— **branch_session 天然支持 fork**。
- 交互:左侧线程侧栏(类似 Codex Desktop 6 线程 / Claude Desktop 多标签)+ 标签 + 新建 + 切换 + fork(从某消息分叉)。
- **后端**:复用现有 session 命令,补"并行运行多个 query"的能力(当前 QueryCoordinator 是否支持多并发需验证)。
- **前端**:assistant-ui Thread 抽象是骨架;新建 `components/chat/ThreadSidebar.tsx`。
- **关键风险**:并发 query 的资源竞争(token 限流、UI 事件路由)—— 需在 adapter 层做线程隔离。

### 7.4 整体美化升级

**需求**:chat 页面视觉与交互全面升级,达到成熟产品(Claude/Codex Desktop)水准。
- **设计 token**:统一 Tailwind theme(色板/间距/圆角/字号)+ 完整暗色模式 + Shannon 品牌色。
- **消息气泡**:assistant-ui Message 组件替换自研;markdown 渲染优化(代码块复制/语言标签/行号)、引用块、表格、链接预览。
- **流式体验**:流式打字动画、滚动锚定(assistant-ui)、tool-call/thinking 折叠块、artifact 卡片。
- **输入栏**:assistant-ui Composer;多行、快捷键、斜杠命令提示、@提及、上下文预览。
- **微交互**:加载骨架、错误态、空态、通知(sonner)、过渡动画(motion)。
- **a11y**:键盘导航、ARIA、对比度、focus 管理(对标 Claude Desktop 成熟度)。

---

## 8. 风险与缓解

| 风险 | 缓解 |
|---|---|
| runtime adapter beta 接口变动 | 锁版本 + 阶段①配 snapshot 测试 + fork 准备 |
| 领域组件(图表/研究报告)适配后外观回退 | 阶段①前建视觉回归基线(insta snapshot) |
| 多线程并发资源竞争 | adapter 层线程隔离 + QueryCoordinator 并发验证 |
| whisper-rs 模型体积影响安装包 | 模型按需下载(非打包),设置选档位 |
| 适配期 chat 功能回归 | 保留旧 `Chat.tsx` 作 feature flag 回退,逐步切换 |
| 包体积增加 | 阶段①后测 delta,超阈值用 primitive 子包 |

---

## 9. v2 评审结论

**v2 独立评审结论:推荐以 assistant-ui 为 chat 升级骨架,执行五步实施(runtime adapter → Thread → 附件 → 美化 → 语音)。**

- v1 的"选择性采纳、阶段 3 暂缓"在**只优化体验**的前提下正确;但用户评审确认的 **4 项新功能(附件/语音/多线程/美化)有 3 项是 assistant-ui 强项**,ROI 翻转,阶段 3 触发条件成立。
- 推荐路径**不是抛弃自研**,而是"assistant-ui 骨架 + Shannon 领域组件 custom parts + Rust 语音",既拿到社区红利,又保住差异化。
- 详细工程(文件锚点/步骤/估时)纳入 [improvement-plan-2026-08.md](./improvement-plan-2026-08.md) 桌面 chat 升级任务块(P1–P2,跨 Wave 2–3)。

---

## 参考来源

- [assistant-ui 官网](https://www.assistant-ui.com/) · [docs](https://www.assistant-ui.com/docs) · [GitHub](https://github.com/assistant-ui/assistant-ui)
- [Vercel AI Elements](https://elements.ai-sdk.dev/)
- [whisper-rs](https://github.com/tazz4843/whisper-rs)(本地 STT)
- [Medium: UI Libraries for AI Chat 2026](https://alexander-lukashov.medium.com/the-overview-of-ui-libraries-for-ai-chat-interfaces-in-2026-146a1492114a)
- [Design Revision: Assistant-UI Alternatives (2026)](https://designrevision.com/alternatives/assistant-ui)
