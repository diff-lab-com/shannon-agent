# P2-5 Desktop Chat Upgrade (assistant-ui 骨架)

> 状态:Planning · 估时:4–6w 单人 · 优先级:P2-5 办公线扩张核心 · 依赖:QueryCoordinator 重构(本计划新增) · 关联:docs/aichat-ui-library-evaluation.md v2

## 1. 目标

把 ~2600 LOC 自研 chat 升级为 assistant-ui 骨架 + Shannon 领域组件 custom parts;5 子任务交付 4 大功能(附件/语音/多线程/美化)。一次 runtime adapter 摊薄到 4 项功能,保留 Shannon 差异化(图表/研究报告/脚注 Markdown / Rust whisper-rs 离线语音),不引入 GenUI 冗余。详见评审文档 [aichat-ui-library-evaluation.md](../aichat-ui-library-evaluation.md) v2 §0 / §6。

## 2. 关键依赖与前提(NEW)

### 2.1 AppState 并发重构(本计划 P0)
- 详见 `docs/plans/query-coordinator-concurrency.md`
- 当前 `desktop::AppState` 单 Mutex 序列化 → 必须先拆为 session-keyed
- 工作量:1w(单 AppState 重构)· 在 5b 之前完成
- 文件: `desktop/src/commands.rs` 重构,引入 `SessionRegistry` 抽象

### 2.2 assistant-ui 锁定版本(本计划 P0)
- 选 `~0.7` 或 `@assistant-ui/react@latest` stable
- 写 `desktop/ui/src/lib/runtime/shannonTauriRuntime.ts` spike

## 3. 5 子任务实施(详细)

### 3.1 P2-5a · Runtime Adapter (3–5d) [前置所有]

#### 文件锚点
- 新增: `desktop/ui/src/lib/runtime/shannonTauriRuntime.ts`
- 新增: `desktop/ui/src/lib/runtime/chatModelAdapter.ts`
- 改: `desktop/ui/src/context/ChatContext.tsx`
- 改: `desktop/ui/package.json` — add `@assistant-ui/react` ~0.7

#### 实施步骤
1. `pnpm add @assistant-ui/react@~0.7`(spike 锁版本)
2. 实现 `ShannonTauriRuntime implements ExternalStoreRuntime`:
   - 订阅 Tauri events: `query:text`、`tool-start`、`tool-result`、`thinking`、`completed`
   - 映射到 assistant-ui ThreadMessage 状态
3. 实现 `ChatModelAdapter`:
   - `onNewMessage(msg)` → `invoke('send_message', ...)`
   - 流式事件回流到 runtime
4. `ChatContext.tsx` 用 `<AssistantRuntimeProvider runtime={shannonRuntime}>` 包裹
5. 加 insta snapshot 防接口漂移
6. feature flag `chat.v2 = true/false`(回退到旧 Chat.tsx)

#### 验收
- [ ] 单 thread happy path: assistant-ui Thread 渲染 Shannon 文本/tool/thinking
- [ ] 流式输出 + 滚动锚定正常
- [ ] feature flag 关闭时旧 Chat.tsx 行为不变

#### 风险
- 接口变动(beta 期): 锁版本 + snapshot
- Tauri event → runtime 状态机的复杂度: spike 0.5d 验证 happy path

### 3.2 P2-5b · Multi-thread (4–6d) [依赖 5a + AppState 重构]

#### 文件锚点
- 新增: `desktop/ui/src/components/chat/ThreadSidebar.tsx`
- 新增: `desktop/ui/src/components/chat/ThreadTabs.tsx`
- 改: `desktop/ui/src/pages/Chat.tsx`
- 改: `desktop/src/commands.rs`(SessionRegistry 注册新线程 / fork / 切换)
- 改: `desktop/src/commands_sessions.rs`(扩展 branch_session 支持并行 query)

#### 实施步骤
1. 前后端 session 模型对齐: 每个线程 = 一个 QueryEngine 实例(per the Q 检查报告)
2. 后端: 加 `SessionKey`、`SessionRegistry`,把现有 `Mutex<Vec<ChatMessage>>` 拆为 `DashMap<SessionKey, ChatMessage>`
3. `commands_sessions::branch_session(from_session_id, from_message_id)` 已存在,要确认能 fork 并行
4. 前端: ThreadSidebar(列表/新建/切换/fork/重命名/删除)+ ThreadTabs(顶部 tab 切换)
5. adapter 层: 线程 → session 映射 + 事件路由(避免串线)
6. UI:未读指示、最后消息预览、活跃线程高亮

#### 验收
- [ ] ≥3 线程并行,各自独立流式输出不串线
- [ ] fork 从指定消息分叉出新线程
- [ ] 切换线程不丢上下文

#### 风险
- **真阻塞**: QueryCoordinator 单 query 序列化(P2-5b 前必须 AppState 重构)
- 事件路由复杂: 单元测试覆盖

### 3.3 P2-5c · Attachments (3–5d) [依赖 5a]

#### 文件锚点
- 新增: `desktop/ui/src/components/chat/AttachmentChip.tsx`
- 改: `desktop/ui/src/components/chat/ChatInput.tsx`
- 改: `desktop/ui/src/lib/tauri-api.ts`(加 `readAttachment`)
- 改: `desktop/src/commands_files.rs`(加 `read_attachment` 命令)
- 改: `desktop/src/events.rs`

#### 实施步骤
1. 后端 `read_attachment(path) -> AttachmentPayload { mime, base64?, text?, name, size }`
2. 内容映射:
   - 图片(png/jpg/webp/gif)→ `ContentBlock::Image`
   - 文本 / 代码 → `ContentBlock::Text`
   - PDF → 文本提取(pdftotext 回退 / 后期接回 P1-1 /pdf 类的能力)
3. 限制:单文件 25MB、类型 allowlist、总附件数 ≤10(配置化)
4. 前端: assistant-ui attachment primitives + Composer
5. 拖放 + chip 预览 + 多文件 + 删除
6. 发送: ContentBlock[] 拼到消息

#### 验收
- [ ] 图片/文本/PDF 三类可上传并发给 LLM(vision 看图)
- [ ] 拖放 + 点选 + 多文件 + 删除可用
- [ ] 超限/超类型拒绝提示清晰

### 3.4 P2-5d · Beauty Upgrade (5–7d) [依赖 5a,可与 5c 并行]

#### 文件锚点
- 改: `desktop/ui/src/components/chat/MessageBubble.tsx`
- 改: `desktop/ui/src/components/chat/Markdown.tsx`
- 改: `desktop/ui/src/components/chat/StreamingResponse.tsx`
- 改: `desktop/ui/src/pages/Chat.tsx`
- 改: `desktop/ui/tailwind.config.cjs`(设计 token)
- 新增: `desktop/ui/src/styles/tokens.css`

#### 实施步骤
1. **设计 token**: Tailwind theme(色板/间距/圆角/字号)+ 暗色模式 + Shannon 品牌色
2. **消息气泡**: assistant-ui Message 组件替换自研
3. **Markdown 优化**: 代码块复制 / 语言标签 / 行号 / 引用 / 表格 / 链接预览
4. **流式体验**: 打字动画 + 滚动锚定 + tool-call/thinking 折叠 + artifact 卡片
5. **输入栏**: assistant-ui Composer + 多行 + Ctrl+Enter + 斜杠提示 + @提及 + 上下文预览
6. **微交互**: sonner 通知 + motion 过渡 + 骨架屏 + 错误/空态
7. **a11y**: 键盘导航 + ARIA + 对比度 AA + focus 管理

#### 验收
- [ ] insta 视觉回归基线(明/暗模式)
- [ ] Lighthouse a11y ≥90
- [ ] 主观对标 Claude/Codex Desktop 不落后

### 3.5 P2-5e · Voice (5–7d) [独立,可启动即并行]

#### 文件锚点
- 改: `desktop/ui/src/lib/voice/`(已存在)
- 新增: `desktop/src/commands_voice.rs`(whisper-rs)
- 改: `desktop/ui/src/components/chat/ChatInput.tsx`(mic UI)
- 改: `desktop/Cargo.toml`(加 whisper-rs)

#### 实施步骤
1. 后端加 `whisper-rs`;`transcribe_audio(path, model, lang) -> { text, confidence }`
2. 模型按需下载 → `~/.shannon/models/whisper/{tiny,base,small,medium}/`
3. 设置 UI: 档位 + 语言 + 本地优先
4. 前端: Web Audio API 采集 → wav → 转写 → 注入 composer
5. 录音 mic UI + 波形(motion)
6. 错误:无权限 / 模型未下 / 低置信度

#### 验收
- [ ] 中英文录音可转写填入输入栏
- [ ] 模型按需下载,不打包
- [ ] 离线本地模型可用

#### 风险
- whisper-rs 模型体积(75MB~1.5GB): 按需下载缓解
- 首次下载慢: 进度条 + 引导用户选档位

## 4. 关键路径与依赖

```
AppState 重构(2.1)
   ↓
P2-5a runtime adapter ──┬─→ P2-5b multi-thread
                         ├─→ P2-5c attachments
                         ├─→ P2-5d beauty
                         └─→ P2-5e voice (与 5a 并行启动)
                              ↓
                          P3-1 artifact 卡片 (Plan)
```

## 5. Rollback 策略

- 每个子任务结束点都可回退
- feature flag `chat.v2` 整体开关
- 旧 Chat.tsx 不删,留作回退通道

## 6. 整体验收

- [ ] P0 AppState 重构先期完成
- [ ] 5 子任务全绿,REPL + Desktop 双 path
- [ ] Lighthouse a11y ≥90
- [ ] chat.v2 = true 的人测对标 Claude Desktop 不落后
- [ ] whisper-rs 模型管理 UX 完整

## 7. 估时汇总(单人)

| 子任务 | 估时 | 依赖 |
|---|---|---|
| 2.1 AppState 重构(P0) | 1w | 无 |
| P2-5a runtime adapter | 3–5d | 2.1 |
| P2-5b multi-thread | 4–6d | 5a |
| P2-5c attachments | 3–5d | 5a |
| P2-5d beauty | 5–7d | 5a |
| P2-5e voice | 5–7d | 无(独立) |
| **总计** | **4–6w** | |
