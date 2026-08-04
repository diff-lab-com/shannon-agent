# Shannon Desktop 产品架构

> 版本: 1.0 | 日期: 2026-06-06
> 基于: Claude Desktop, Codex Desktop, OpenClaw, Hermes, WorkBuddy 竞品分析

---

## 一、产品定位

**一句话**: 最轻量、最开放、最可审计的开源 AI Agent 桌面客户端。

| 差异维度 | Shannon Desktop | Claude Desktop | Codex Desktop |
|----------|----------------|----------------|---------------|
| 安装体积 | ~15MB (Tauri) | ~300MB (Electron) | ~250MB (Electron) |
| LLM Provider | 25+ 不锁定 | Anthropic only | OpenAI only |
| 后端语言 | Rust 全栈 | TypeScript | Rust (CLI) + TypeScript (UI) |
| 开源 | MIT | 闭源 | 部分 |
| 终端 + 桌面 | 双模式共享核心 | 仅桌面 | 仅桌面 |

**目标用户**: 开发者 + 技术用户，需要多 Provider 支持、自托管、隐私可控的 AI Agent 桌面工具。

---

## 二、系统架构总览

```
┌─────────────────────────────────────────────────────────┐
│                   Shannon Desktop App                    │
│                                                          │
│  ┌────────────────────────────────────────────────────┐  │
│  │           React 19 Frontend (WebView)              │  │
│  │                                                     │  │
│  │  ┌──────────┐ ┌──────────┐ ┌────────────────────┐ │  │
│  │  │ Chat     │ │ Agent    │ │ Settings           │ │  │
│  │  │ Panel    │ │ Dashboard│ │ Panel              │ │  │
│  │  └────┬─────┘ └────┬─────┘ └────────┬───────────┘ │  │
│  │       │             │                │             │  │
│  │  ┌────▼─────────────▼────────────────▼───────────┐ │  │
│  │  │          Store Layer (AppContext)              │ │  │
│  │  │  messages | agents | config | sessions         │ │  │
│  │  └───────────────────┬───────────────────────────┘ │  │
│  │                      │ listen() / invoke()         │  │
│  └──────────────────────┼────────────────────────────┘  │
│                         │ Tauri IPC                      │
│  ┌──────────────────────▼────────────────────────────┐  │
│  │            Rust Backend (commands.rs)              │  │
│  │                                                     │  │
│  │  ┌─────────────┐ ┌──────────────┐ ┌─────────────┐ │  │
│  │  │ Query       │ │ Permission   │ │ Session     │ │  │
│  │  │ Coordinator │ │ Bridge       │ │ Manager     │ │  │
│  │  └──────┬──────┘ └──────┬───────┘ └──────┬──────┘ │  │
│  │         │               │                │        │  │
│  │  ┌──────▼───────────────▼────────────────▼──────┐ │  │
│  │  │           DesktopService Layer                │ │  │
│  │  │  AppState | DesktopConfig | EventRouter      │ │  │
│  │  └───────────────────┬──────────────────────────┘ │  │
│  └──────────────────────┼────────────────────────────┘  │
│                         │                                 │
│  ┌──────────────────────▼────────────────────────────┐  │
│  │            shannon-core (共享 Rust 核心)            │  │
│  │                                                     │  │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────┐ │  │
│  │  │ Query    │ │ Tool     │ │ MCP      │ │ Memory│ │  │
│  │  │ Engine   │ │ Registry │ │ Client   │ │ Store │ │  │
│  │  └──────────┘ └──────────┘ └──────────┘ └───────┘ │  │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────┐ │  │
│  │  │ LLM      │ │ Permis-  │ │ State    │ │ Cost  │ │  │
│  │  │ Client   │ │ sions    │ │ Manager  │ │ Track │ │  │
│  │  └──────────┘ └──────────┘ └──────────┘ └───────┘ │  │
│  └────────────────────────────────────────────────────┘  │
│                                                          │
│  ┌────────────────────────────────────────────────────┐  │
│  │         Desktop Integration (Tauri Plugins)        │  │
│  │  System Tray │ Auto-Update │ File Dialog │ Shell   │  │
│  └────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### 架构原则

1. **核心共享**: `shannon-core` 是 CLI 和 Desktop 的共同基础，改进一次全部受益（Codex 模式）
2. **事件驱动**: 前端不轮询，通过 Tauri event push 获取 streaming 数据
3. **权限桥接**: 工具执行权限通过 IPC bridge 在 UI 层展示确认对话框
4. **特性门控**: `#[cfg(feature = "tauri")]` 隔离桌面代码，CLI 构建零影响

---

## 三、分层架构详解

### Layer 1: Frontend (React 19 + TypeScript)

```
ui/
├── src/
│   ├── index.html                   # Tauri 入口 HTML
│   ├── App.tsx                      # 根组件: 布局 + 路由状态
│   ├── main.tsx                     # 挂载 App + 初始化 Tauri listeners
│   │
│   ├── pages/                       # 路由页 (React.lazy)
│   │   ├── Chat.tsx                 # 主 chat 页(自带 ChatInput/MessageBubble)
│   │   ├── ChatV2Spike.tsx          # assistant-ui runtime adapter spike(P2-5a)
│   │   ├── Editor.tsx               # 内嵌代码编辑
│   │   ├── Extensions.tsx           # MCP / Skill / Agent 扩展管理
│   │   ├── Memory.tsx               # 记忆库视图
│   │   ├── OPC.tsx / OPCTask.tsx    # OPC 指标与任务视图
│   │   ├── QuickFix.tsx             # 一键修复(LSP 诊断聚合)
│   │   ├── Settings.tsx             # 设置面板
│   │   ├── Tasks.tsx                # 任务板
│   │   ├── Triage.tsx               # 工单/消息分流
│   │   ├── Usage.tsx                # token/费用统计
│   │   └── Welcome.tsx              # 首次启动欢迎向导
│   │
│   ├── context/
│   │   ├── AppContext.tsx           # 中心 store (useApp())
│   │   ├── CatalogContext.tsx       # 模型/Provider 目录(搜索/过滤)
│   │   ├── ChatContext.tsx          # chat 状态(P2-5a 切换 runtime 用)
│   │   ├── SessionContext.tsx       # 当前 session 信息
│   │   └── ThemeContext.tsx         # 主题/外观
│   │
│   ├── hooks/                       # 见下方 hooks
│   │   ├── useTauriEvent.ts
│   │   ├── useTauriEventValidated.ts  # 带 schema 校验的事件订阅
│   │   ├── useNotification.ts
│   │   ├── useKeyboardShortcuts.ts
│   │   ├── useDiffKeyboard.ts        # diff 视图专用快捷键
│   │   ├── useTheme.ts
│   │   ├── useModalFocus.ts          # 模态焦点陷阱(a11y)
│   │   ├── usePagedVisible.ts        # 分页可见性
│   │   ├── usePendingSkillCandidates.ts
│   │   ├── useTaskStreaming.ts       # 任务流式输出
│   │   ├── useVoice.ts               # 语音输入 hook
│   │   └── scheduled-tasks.ts        # 定时/触发式任务
│   │
│   ├── lib/
│   │   ├── tauri-api.ts             # 类型化 invoke() 封装
│   │   ├── featureFlag.ts           # 客户端特性开关(对接 window.__SHANNON_*__)
│   │   ├── diff-highlight.ts        # diff 语法高亮
│   │   ├── diff-merge.ts            # 三路合并工具
│   │   ├── errorToast.ts            # 统一错误 toast
│   │   ├── hljs.ts                  # 代码高亮语言注册
│   │   ├── nl-cron.ts               # 自然语言 → cron
│   │   ├── mock/                    # VITE_MOCK_MODE=1 时替换 invoke
│   │   └── i18n/                    # react-intl v7 (en, zh-CN)
│   │
│   ├── components/                  # 见下方组件设计
│   │   ├── chat/                    # ChatInput, MessageBubble, Markdown, Chart, StreamingResponse
│   │   ├── artifact/                # ArtifactPanel, CodeBlock, Mermaid, Svg, Html, Document
│   │   ├── editor/                  # CodeMirror 包装
│   │   ├── skills/                  # 技能列表与详情
│   │   ├── tasks/                   # 任务板
│   │   ├── settings/                # 设置面板
│   │   ├── extensions/              # MCP/Skill/Agent 扩展
│   │   ├── memory/                  # 记忆库
│   │   ├── voice/                   # 语音输入
│   │   ├── routines/                # 触发式 / 定时任务
│   │   ├── lsp/                     # LSP 诊断面板
│   │   ├── opc/                     # OPC 指标
│   │   ├── diff/                    # 文件 diff 视图
│   │   ├── shared/                  # 通用组件
│   │   ├── ui/                      # shadcn/ui 基元
│   │   ├── ai-elements/             # assistant-ui 元素
│   │   ├── self-improve/            # 自改进 hints
│   │   ├── SessionsPanel/           # 多线程侧栏(P2-5b)
│   │   ├── Sidebar.tsx
│   │   ├── Header.tsx
│   │   ├── Layout.tsx
│   │   ├── CommandPalette.tsx
│   │   ├── KeyboardShortcutsHelp.tsx
│   │   ├── ErrorBoundary.tsx
│   │   ├── SkeletonLoader.tsx
│   │   └── WelcomeState.tsx
│   │
│   ├── types/
│   │   └── index.ts                 # 后端 payload + 通用类型
│   │
│   ├── i18n/{en,zh-CN}.json
│   ├── __tests__/                   # vitest + Testing Library
│   └── styles.css                   # Tailwind 4 全局样式
│
├── index.html
├── vite.config.ts
├── tailwind.config.ts
├── tsconfig.json
├── playwright.config.ts
├── vitest.config.ts
└── package.json
```

#### 组件树

```
App.tsx
├── Layout.tsx                         # 侧栏 + 头部外壳
│   ├── Sidebar.tsx                    # 会话列表 + 新建 + 搜索
│   └── Header.tsx                     # 顶部导航
│
├── ChatPage.tsx                       # 主内容区
│   ├── chat/MessageBubble.tsx         # 单条消息
│   │   ├── chat/FootnoteMarkdown.tsx  # markdown 渲染
│   │   ├── artifact/CodeBlock.tsx     # 代码高亮 + 复制
│   │   └── chat/Chart.tsx             # 图表
│   │
│   ├── artifact/ArtifactPanel.tsx     # HTML/SVG/Mermaid/Document 渲染
│   │   ├── HtmlRenderer.tsx
│   │   ├── SvgRenderer.tsx
│   │   ├── MermaidRenderer.tsx
│   │   └── DocumentRenderer.tsx
│   │
│   ├── chat/ChatInput.tsx             # 输入区域
│   ├── chat/StreamingResponse.tsx     # 流式打字动画
│   └── chat/ResearchReportModal.tsx   # 调研报告弹出
│
├── AgentsPage.tsx                     # Agent 编排
│   ├── self-improve/                  # 自改进 hints
│   └── agents/                        # Agent 卡片
│
├── TasksPage.tsx                      # 任务板
│   └── tasks/                         # TaskBoard + DiffReview
│
├── SettingsPage.tsx                   # 设置面板
│   └── settings/                      # Provider/MCP/Permissions/About
│
├── ExtensionsPage.tsx
│   └── extensions/                    # MCP/Skill/Agent 扩展市场
│
└── WelcomeState.tsx                   # 空状态 / 欢迎页
```

#### Streaming 数据流 (React 19)

```typescript
// hooks/useTauriEvent.ts
import { useEffect, useRef } from 'react';
import { listen, type UnlistenFn, type EventCallback } from '@tauri-apps/api/event';

export function useTauriEvent<T>(event: string, handler: EventCallback<T>) {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    listen<T>(event, (e) => {
      if (!cancelled) handlerRef.current(e);
    })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [event]);
}

// 在 AppContext 中挂载流式事件处理
useTauriEvent<QueryTextEvent>('query:text', (e) => {
  const last = messages.findLast((m) => m.role === 'assistant' && m.streaming);
  if (last) {
    last.content += e.payload.content;  // setState 内部触发 UI 更新
  }
});

useTauriEvent<ToolStartEvent>('query:tool-start', (e) => {
  setMessages((prev) => [
    ...prev,
    {
      role: 'tool',
      toolUseId: e.payload.tool_use_id,
      toolName: e.payload.tool_name,
      status: 'running',
      input: e.payload.tool_input,
    },
  ]);
});

useTauriEvent<ToolResultEvent>('query:tool-result', (e) => {
  setMessages((prev) =>
    prev.map((m) =>
      m.toolUseId === e.payload.tool_use_id
        ? { ...m, status: e.payload.is_error ? 'error' : 'done', output: e.payload.result }
        : m,
    ),
  );
});

useTauriEvent('query:completed', () => setIsStreaming(false));
```

**技术选型**: React 19 搭配 `react-router-dom 7` 的 `React.lazy` 路由, 全局状态走 `AppContext`(一个 `useReducer` + `dispatch`), 流式更新通过 `useTauriEvent` hook 桥接 Tauri event 到 React state。`tailwindcss 4` + `motion` 提供样式与动画, `react-intl v7` 处理 i18n, `react-markdown` + `rehype-highlight` 渲染消息。

---

### Layer 2: Tauri IPC Bridge

#### 命令层 (Request/Response)

```rust
// commands.rs — invoke() 调用的同步命令

#[tauri::command]
async fn send_message(message: String) -> Result<SendMessageResponse, String>;

#[tauri::command]
async fn get_conversation() -> Result<Vec<ChatMessage>, String>;

#[tauri::command]
async fn list_models() -> Result<Vec<ModelInfo>, String>;

#[tauri::command]
async fn list_tools() -> Result<Vec<ToolInfo>, String>;

#[tauri::command]
async fn get_status() -> Result<StatusResponse, String>;

#[tauri::command]
async fn switch_provider(request: ProviderSwitchRequest) -> Result<(), String>;

#[tauri::command]
async fn get_config() -> Result<DesktopConfig, String>;

#[tauri::command]
async fn configure(update: ConfigUpdate) -> Result<(), String>;

#[tauri::command]
async fn cancel_query() -> Result<(), String>;

// Phase 2
#[tauri::command]
async fn list_sessions() -> Result<Vec<SessionSummary>, String>;

#[tauri::command]
async fn load_session(session_id: String) -> Result<Vec<ChatMessage>, String>;

#[tauri::command]
async fn delete_session(session_id: String) -> Result<(), String>;

#[tauri::command]
async fn list_mcp_servers() -> Result<Vec<McpServerInfo>, String>;

#[tauri::command]
async fn add_mcp_server(config: McpServerConfig) -> Result<(), String>;

#[tauri::command]
async fn respond_permission(request_id: String, choice: String) -> Result<(), String>;

// Phase 3
#[tauri::command]
async fn create_team(config: TeamConfig) -> Result<String, String>;

#[tauri::command]
async fn list_teams() -> Result<Vec<TeamInfo>, String>;

#[tauri::command]
async fn send_agent_message(team: String, agent: String, message: String) -> Result<String, String>;
```

#### 事件层 (Server Push)

```rust
// events.rs — emit() 推送到前端的事件

// Query streaming
pub const QUERY_TEXT: &str = "query:text";
pub const QUERY_THINKING: &str = "query:thinking";
pub const QUERY_TOOL_START: &str = "query:tool-start";
pub const QUERY_TOOL_RESULT: &str = "query:tool-result";
pub const QUERY_TOOL_PROGRESS: &str = "query:tool-progress";
pub const QUERY_COMPLETED: &str = "query:completed";
pub const QUERY_FAILED: &str = "query:failed";
pub const QUERY_USAGE: &str = "query:usage";
pub const QUERY_COST: &str = "query:cost";

// Permission (Phase 2)
pub const PERMISSION_REQUEST: &str = "permission:request";

// Agent (Phase 2)
pub const AGENT_STARTED: &str = "agent:started";
pub const AGENT_MESSAGE: &str = "agent:message";
pub const AGENT_COMPLETED: &str = "agent:completed";

// MCP (Phase 2)
pub const MCP_SERVER_CONNECTED: &str = "mcp:server-connected";
pub const MCP_SERVER_DISCONNECTED: &str = "mcp:server-disconnected";
pub const MCP_TOOL_DISCOVERED: &str = "mcp:tool-discovered";

// Desktop lifecycle
pub const DESKTOP_READY: &str = "desktop:ready";
pub const DESKTOP_CONFIG_CHANGED: &str = "desktop:config-changed";
```

---

### Layer 3: Rust Service Layer

```
src/
├── main.rs                  # Tauri 启动, register plugins + commands
├── lib.rs                   # 模块声明
├── config.rs                # DesktopConfig 持久化 (已有)
├── events.rs                # 事件类型定义 (已有)
│
├── commands/                # Tauri IPC 命令
│   ├── mod.rs
│   ├── chat.rs              # send_message, get_conversation, cancel
│   ├── config_cmd.rs        # configure, switch_provider, get_config
│   ├── session.rs           # list/load/delete sessions (Phase 2)
│   ├── mcp.rs               # MCP server management (Phase 2)
│   ├── permission.rs        # permission bridge (Phase 2)
│   ├── agent.rs             # team/agent management (Phase 3)
│   └── system.rs            # list_tools, list_models, get_status
│
├── services/                # 业务逻辑层 (非 Tauri 依赖)
│   ├── mod.rs
│   ├── query_coordinator.rs # QueryEngine 包装 + 事件派发
│   ├── session_manager.rs   # 会话持久化 (SQLite)
│   ├── permission_bridge.rs # 权限请求→UI→回调
│   ├── mcp_manager.rs       # MCP server 生命周期
│   └── agent_coordinator.rs # Team/Agent 编排
│
├── state.rs                 # AppState (全局共享状态)
└── desktop_error.rs         # 统一错误类型
```

#### 核心服务: QueryCoordinator

QueryCoordinator 是整个桌面端的核心编排层，连接 QueryEngine 和 Tauri 事件系统：

```rust
pub struct QueryCoordinator {
    app_handle: AppHandle,
    client_config: Arc<RwLock<LlmClientConfig>>,
    tools: Arc<ToolRegistry>,
    state_manager: Arc<StateManager>,
    qe_config: Arc<RwLock<QueryEngineConfig>>,
    querying: Arc<Mutex<bool>>,
    cancel_token: Arc<Mutex<Option<CancellationToken>>>,
}

impl QueryCoordinator {
    /// 发送消息并流式派发事件到前端
    pub async fn send_message(
        &self,
        message: String,
        messages_arc: Arc<Mutex<Vec<ChatMessage>>>,
    ) -> Result<String, DesktopError> {
        // 1. 防并发
        // 2. 构建 LlmClient + QueryEngine
        // 3. 创建 QueryContext
        // 4. spawn tokio task: 消费 QueryStream → emit Tauri events
        // 5. 返回 query_id
    }

    /// 取消当前查询
    pub async fn cancel(&self) -> Result<(), DesktopError> {
        // 通过 CancellationToken 取消
    }
}
```

#### 权限桥接: PermissionBridge

工具执行需要用户确认时，通过事件系统桥接到 UI：

```rust
pub struct PermissionBridge {
    app_handle: AppHandle,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<PermissionChoice>>>>,
}

impl PermissionBridge {
    /// QueryEngine 调用: 请求权限
    pub async fn request(&self, prompt: PermissionPrompt) -> PermissionChoice {
        let (tx, rx) = oneshot::channel();
        let id = Uuid::new_v4().to_string();
        self.pending.lock().await.insert(id.clone(), tx);

        // 推送到 UI
        self.app_handle.emit("permission:request", PermissionRequestPayload {
            request_id: id,
            prompt,
        }).ok();

        // 等待 UI 回调
        rx.await.unwrap_or(PermissionChoice::Deny)
    }

    /// UI 回调: 用户选择
    pub async fn respond(&self, request_id: String, choice: PermissionChoice) {
        if let Some(tx) = self.pending.lock().await.remove(&request_id) {
            tx.send(choice).ok();
        }
    }
}
```

---

### Layer 4: Desktop Integration (Tauri Plugins)

```rust
// main.rs — 插件注册
tauri::Builder::default()
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_dialog::init())       // 文件选择对话框
    .plugin(tauri_plugin_fs::init())           // 文件系统访问
    .plugin(tauri_plugin_clipboard::init())    // 剪贴板
    .plugin(tauri_plugin_process::init())      // 进程管理
    // Phase 2:
    // .plugin(tauri_plugin_autostart::init()) // 开机启动
    // .plugin(tauri_plugin_updater::init())   // 自动更新
    // .plugin(tauri_plugin_notification::init()) // 系统通知
    // .plugin(tauri_plugin_global_shortcut::init()) // 全局快捷键
```

---

## 四、数据流详解

### 4.1 用户发送消息 → 流式响应

```
用户输入 "解释这个函数"
    │
    ▼
ChatInput.tsx → invoke("send_message", { message })
    │
    ▼
commands/chat.rs::send_message()
    │  1. 防并发检查
    │  2. 存储 user message
    │  3. 构建 LlmClient + QueryEngine
    │  4. 返回 { query_id }
    │  5. spawn 后台 task:
    │
    ▼  ┌──────────────────────────────────────┐
       │ QueryEngine::query(context)           │
       │   → QueryStream                       │
       │                                       │
       │   while let Some(event) = stream.next │
       │     match event:                      │
       │       Text → emit("query:text")       │──→ ChatPanel 更新
       │       ToolUseRequest → emit(...)      │──→ ToolCallBlock 显示
       │       ToolUseResult → emit(...)       │──→ ToolCallBlock 完成
       │       Thinking → emit(...)            │──→ ThinkingBlock 折叠
       │       Usage → emit(...)               │──→ StatusBar 更新
       │       Completed → emit(...)           │──→ 流结束, 保存消息
       │       Failed → emit(...)              │──→ 错误提示
       └──────────────────────────────────────┘
```

### 4.2 工具权限确认

```
QueryEngine 需要执行 bash("rm -rf /tmp/test")
    │
    ▼
PermissionBridge::request(Bash { command: "rm -rf ..." })
    │  emit("permission:request", { request_id, tool, input })
    │
    ▼
PermissionDialog.tsx 显示确认对话框
    │  用户点击 [允许] / [拒绝]
    │
    ▼
invoke("respond_permission", { request_id, choice: "allow" })
    │
    ▼
PermissionBridge::respond() → oneshot::Sender
    │
    ▼
QueryEngine 继续执行 (或中止)
```

### 4.3 Provider 切换

```
SettingsPanel → invoke("switch_provider", { provider: "openai", api_key, model })
    │
    ▼
commands/config_cmd.rs::switch_provider()
    │  1. 更新 client_config (RwLock)
    │  2. 更新 model, provider (Mutex)
    │  3. 保存到 ~/.shannon/desktop.json
    │  4. emit("desktop:config-changed")
    │
    ▼
StatusBar.tsx 更新显示 "OpenAI / gpt-4.1"
```

---

## 五、会话持久化设计

```
~/.shannon/
├── desktop.json              # 全局配置 (已有)
├── sessions/
│   ├── {session_id}.json     # 会话元数据
│   └── {session_id}/
│       ├── messages.jsonl    # 消息流 (append-only)
│       └── state.json        # QueryEngine 状态快照
└── mcp.json                  # MCP server 配置
```

```rust
struct SessionManager {
    base_dir: PathBuf,
}

impl SessionManager {
    fn create_session(&self) -> Session;          // 新建
    fn list_sessions(&self) -> Vec<SessionSummary>; // 列表
    fn load_session(&self, id: &str) -> Session;   // 加载
    fn save_message(&self, id: &str, msg: &ChatMessage); // 追加
    fn delete_session(&self, id: &str);            // 删除
}
```

格式选择 JSONL (而非 SQLite)：单文件追加写入，无依赖，符合 Shannon 终端优先的哲学。

---

## 六、Artifact 渲染方案

Claude Desktop 的 Artifact 是核心差异化功能。Shannon 的实现方案：

```tsx
// components/artifact/ArtifactPanel.tsx
import { useState, useRef, useEffect, useCallback } from 'react';
import { HtmlRenderer } from './HtmlRenderer';
import { SvgRenderer } from './SvgRenderer';
import { MermaidRenderer } from './MermaidRenderer';
import { DocumentRenderer } from './DocumentRenderer';
import { CodeBlock } from './CodeBlock';

type Tab = 'preview' | 'code';

export function ArtifactPanel() {
  const [tab, setTab] = useState<Tab>('preview');
  // artifact.type: 'html' | 'svg' | 'mermaid' | 'document' | 'code'
  // 根据 type 选择对应 Renderer (HtmlRenderer / SvgRenderer / MermaidRenderer / DocumentRenderer)
  // Panel 支持拖拽改宽度, localStorage 持久化 (shannon.artifact.panelWidth)

  function buildDom(a: Artifact) {
    switch (a.type) {
      case 'html':
        return a.code;
      case 'svg':
        return a.code;
      case 'mermaid':
        return `<!DOCTYPE html><html><head>
          <script src="https://cdn.jsdelivr.net/npm/mermaid/dist/mermaid.min.js"></script>
        </head><body><pre class="mermaid">${a.code}</pre>
          <script>mermaid.initialize({startOnLoad:true});</script>
        </body></html>`;
    }
  }

  return (
    <div className="artifact-container">
      <div className="artifact-header">
        <span>{artifact.title}</span>
        <button onClick={() => copyToClipboard(artifact.code)}>Copy</button>
      </div>
      <iframe srcDoc={buildDom(artifact)} sandbox="allow-scripts" />
    </div>
  );
}
```

**安全**: `sandbox="allow-scripts"` 限制 iframe 能力（无网络、无弹出、同源隔离）。

---

## 七、安全模型

### CSP (Content Security Policy)

```
default-src 'self';
script-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net https://esm.sh;
style-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net;
font-src 'self';
img-src 'self' data: blob:;
connect-src 'self' https://*.anthropic.com https://*.openai.com;
```

### 权限分层

| 层级 | 机制 | 说明 |
|------|------|------|
| L1: 工具权限 | PermissionBridge → UI 确认 | bash/写文件/网络请求需确认 |
| L2: 沙盒 | Tauri shell plugin + CSP | 前端无法直接执行系统命令 |
| L3: API Key 隔离 | 内存存储 + 磁盘加密 | OS keychain (Phase 3) |
| L4: MCP 沙盒 | 独立进程 + 权限配置 | MCP server 隔离执行 |

---

## 八、前端技术栈决策

| 组件 | 选型 | 理由 |
|------|------|------|
| 框架 | **React 19** | 生态成熟, `useTransition` + `useOptimistic` 适合 streaming 增量更新 |
| 语言 | **TypeScript** | 类型安全, invoke()/listen() 类型推导 |
| 构建 | **Vite 6** | Tauri 官方推荐, HMR 快 |
| 样式 | **Tailwind CSS 4** | utility-first, 主题系统用 CSS variables |
| Markdown | **react-markdown** + **rehype-highlight** + **remark-gfm** | 与 Vite/React 19 集成简单, 已用 `rehype-sanitize` 兜底 XSS |
| 图标 | **Lucide React** | 轻量图标库, tree-shakable |
| 状态 | **React Context + useReducer** + **react-intl** | 全局 store + i18n, 流式数据靠 `useEffect` + `useState` |
| 测试 | **Vitest** + **@testing-library/react** | 单元 + 组件测试 (jsdom) |

**不选 Svelte 5 Runes / SolidJS 的理由**: React 19 已是桌面端事实标准,生态成熟(motion / react-intl / react-markdown / react-virtual / react-router),组件库与 Tauri 集成示例更完整。React 19 的 `useTransition` + `useOptimistic` 已足够处理 streaming chunk 的频繁更新, 心智模型与 ESLint/TS 静态检查匹配。

---

## 九、分阶段实施路线

### Phase 1: 核心聊天 (4-6 周)

**目标**: 替代当前 vanilla JS MVP, 成为可日常使用的桌面聊天工具

```
Week 1-2: React 项目搭建 + 核心组件
  - Vite + React 19 + Tailwind CSS 4 配置
  - ChatPage + MessageBubble + ChatInput
  - Markdown (react-markdown + rehype-highlight) + CodeBlock (代码高亮 + 复制)
  - StreamingResponse (打字动画)
  - StatusBar (provider/model/cost)

Week 3-4: QueryEngine 集成完善
  - QueryCoordinator 服务层
  - ToolCallDisplay (bash 输出, 文件 diff)
  - ThinkingBlock (思考过程折叠)
  - ChatInput 模型快速切换
  - 错误处理 + 重试

Week 5-6: 桌面集成 + 打磨
  - 会话持久化 (SessionManager)
  - SettingsPage 完整实现
  - 系统托盘 (tauri-plugin-tray 替代)
  - 自动更新 (tauri-plugin-updater)
  - 跨平台测试 (macOS/Windows/Linux)
```

### Phase 2: Agent 编排 (6-8 周)

```
  - Sidebar 会话列表
  - PermissionBridge + PermissionDialog
  - AgentsPage (Agent Dashboard)
  - TasksPage (Team 任务面板)
  - DiffReview (文件修改审查)
  - extensions/ McpManager (MCP server 管理 UI)
  - 全局快捷键
  - 文件拖放输入
```

### Phase 3: 差异化功能 (8-12 周)

```
  - ArtifactPanel (iframe 沙盒渲染)
  - 后台 Agent (系统托盘 + 通知)
  - 插件/Skill 浏览器 UI
  - 语音输入 (Whisper API)
  - OS Keychain 集成 (API key 安全存储)
  - Computer Use UI (截图 + 点击确认)
```

---

## 十、与 CLI 的关系

```
                    ┌──────────────────┐
                    │  shannon-core    │
                    │  QueryEngine     │
                    │  LlmClient       │
                    │  ToolRegistry    │
                    │  MCP Client      │
                    │  MemoryStore     │
                    │  Permissions     │
                    └────────┬─────────┘
                             │
                 ┌───────────┴───────────┐
                 │                       │
        ┌────────▼────────┐    ┌────────▼────────┐
        │  shannon-cli    │    │ shannon-desktop  │
        │  (TUI/Headless) │    │ (Tauri v2)       │
        │                 │    │                  │
        │  ratatui UI     │    │  React 19 UI     │
        │  REPL loop      │    │  WebView         │
        │  Terminal out   │    │  Tauri IPC       │
        └─────────────────┘    └──────────────────┘
```

**核心原则**: 改进 `shannon-core` 一次，CLI 和 Desktop 同时受益。桌面端不复制逻辑，只做 UI 层。

---

## 十一、CI/CD

```yaml
# .github/workflows/desktop-release.yml
matrix:
  os: [macos-latest, windows-latest, ubuntu-22.04]

steps:
  - Setup Rust + Node.js
  - cargo test -p shannon-desktop          # 单元测试
  - npm ci && npm run check                 # React 类型检查
  - npm run test                            # Vitest 组件测试
  - tauri build                             # 打包 .dmg / .msi / .AppImage
  - Upload to GitHub Release
```

测试策略：
- **Rust 层**: `cargo test -p shannon-desktop` (commands, services, config)
- **TS 层**: `vitest` (store 逻辑, 组件渲染, event handler)
- **E2E**: Tauri WebDriver (Phase 2, 关键流程验证)
