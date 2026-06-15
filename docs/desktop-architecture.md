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
│  │           React 18 Frontend (WebView)              │  │
│  │                                                     │  │
│  │  ┌──────────┐ ┌──────────┐ ┌────────────────────┐ │  │
│  │  │ Chat     │ │ Agent    │ │ Settings           │ │  │
│  │  │ Panel    │ │ Dashboard│ │ Panel              │ │  │
│  │  └────┬─────┘ └────┬─────┘ └────────┬───────────┘ │  │
│  │       │             │                │             │  │
│  │  ┌────▼─────────────▼────────────────▼───────────┐ │  │
│  │  │          Store Layer (React Runes)            │ │  │
│  │  │  $state messages | agents | config | sessions  │ │  │
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

### Layer 1: Frontend (React 18 + TypeScript)

```
ui/
├── src/
│   ├── app.html                     # Tauri 入口 HTML
│   ├── app.css                      # Tailwind 全局样式
│   ├── main.ts                      # 挂载 App + 初始化 Tauri listeners
│   │
│   ├── App.svelte                   # 根组件: 布局 + 路由状态
│   │
│   ├── lib/
│   │   ├── stores/
│   │   │   ├── messages.ts          # $state<Message[]> 消息列表
│   │   │   ├── session.ts           # $state 当前会话
│   │   │   ├── config.ts            # $state provider/model/apikey
│   │   │   ├── agents.ts            # $state agent 状态
│   │   │   └── ui.ts                # $state sidebar/settings 面板状态
│   │   │
│   │   ├── services/
│   │   │   ├── tauri-events.ts      # listen() 封装, 事件→store 更新
│   │   │   ├── tauri-commands.ts    # invoke() 封装, 类型安全
│   │   │   └── markdown.ts          # marked + highlight.js 配置
│   │   │
│   │   ├── types/
│   │   │   ├── messages.ts          # ChatMessage, ToolCall, Thinking
│   │   │   ├── events.ts            # Tauri event payload 类型
│   │   │   └── config.ts            # DesktopConfig, ProviderInfo
│   │   │
│   │   └── components/              # 见下方组件设计
│   │       ├── chat/
│   │       ├── tools/
│   │       ├── agents/
│   │       ├── settings/
│   │       └── common/
│   │
│   └── routes/                      # (可选) 多页面用
│       └── +layout.svelte
│
├── static/
│   └── fonts/                       # 本地字体 (避免 CSP 外部请求)
├── index.html
├── vite.config.ts
├── svelte.config.js
├── tailwind.config.ts
├── tsconfig.json
└── package.json
```

#### 组件树

```
App.svelte
├── Sidebar.svelte                    # 会话列表 + 新建 + 搜索
│   ├── SessionList.svelte
│   └── SessionItem.svelte
│
├── MainPanel.svelte                  # 主内容区
│   ├── ChatPanel.svelte              # 消息流
│   │   ├── MessageBubble.svelte      # 单条消息
│   │   │   ├── MarkdownContent.svelte     # markdown 渲染
│   │   │   ├── CodeBlock.svelte           # 代码高亮 + 复制
│   │   │   └── ThinkingBlock.svelte       # 思考过程折叠
│   │   │
│   │   ├── ToolCallBlock.svelte      # 工具调用展示
│   │   │   ├── BashOutput.svelte          # bash 执行输出
│   │   │   ├── FileDiff.svelte            # 文件编辑 diff 视图
│   │   │   └── SearchResult.svelte        # grep/glob 结果
│   │   │
│   │   ├── ArtifactViewer.svelte     # iframe 沙盒渲染
│   │   ├── PermissionDialog.svelte   # 工具执行权限确认
│   │   └── StreamingIndicator.svelte # 打字动画 + 进度
│   │
│   └── InputBar.svelte              # 输入区域
│       ├── TextInput.svelte              # 多行输入 + 快捷键
│       ├── FileAttachment.svelte         # 文件拖放 + 附件
│       └── ModelSelector.svelte          # 模型快速切换
│
├── AgentPanel.svelte                 # Agent 编排 (Phase 2)
│   ├── AgentCard.svelte
│   ├── TaskBoard.svelte
│   └── DiffReview.svelte
│
├── SettingsOverlay.svelte            # 设置面板
│   ├── ProviderConfig.svelte
│   ├── McpServers.svelte
│   ├── PermissionRules.svelte
│   └── AboutPanel.svelte
│
└── StatusBar.svelte                  # 底部: provider/model/cost/status
```

#### Streaming 数据流 (React 18 Runes)

```typescript
// stores/messages.ts
export let messages = $state<Message[]>([]);
export let isStreaming = $state(false);
export let currentAssistantContent = $state('');

// services/tauri-events.ts
import { listen } from '@tauri-apps/api/event';

export function initEventListeners() {
  listen('query:text', (e: QueryTextEvent) => {
    // 找到最后一条 assistant 消息, append content
    const last = messages.findLast(m => m.role === 'assistant' && m.streaming);
    if (last) {
      last.content += e.payload.content;  // $state 自动触发 UI 更新
    }
  });

  listen('query:tool-start', (e: ToolStartEvent) => {
    messages.push({
      role: 'tool',
      toolUseId: e.payload.tool_use_id,
      toolName: e.payload.tool_name,
      status: 'running',
      input: e.payload.tool_input,
    });
  });

  listen('query:tool-result', (e: ToolResultEvent) => {
    const tool = messages.findLast(m => m.toolUseId === e.payload.tool_use_id);
    if (tool) {
      tool.status = e.payload.is_error ? 'error' : 'done';
      tool.output = e.payload.result;
    }
  });

  listen('query:completed', () => {
    isStreaming = false;
  });
}
```

**为什么 React 18 而不是 React**: `$state` 赋值即更新，不需要 `useState`/`useEffect`/`useRef`/`useCallback` 这套。Streaming 场景下，每次 `last.content += chunk` 自动 diff 更新 DOM，零心智负担。

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
InputBar.svelte → invoke("send_message", { message })
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
PermissionDialog.svelte 显示确认对话框
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
StatusBar.svelte 更新显示 "OpenAI / gpt-4.1"
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

```svelte
<!-- ArtifactViewer.svelte -->
<script lang="ts">
  let { artifact } = $props();
  // artifact.type: "react" | "html" | "svg" | "mermaid" | "code"

  let srcdoc = $derived(buildSrcdoc(artifact));

  function buildSrcdoc(a) {
    switch (a.type) {
      case 'react':
        return `<!DOCTYPE html>
          <html><head>
            <script type="importmap">
              { "imports": {
                  "react": "https://esm.sh/react@18",
                  "react-dom": "https://esm.sh/react-dom@18/client"
                }
              }
            </script>
          </head><body><div id="root"></div>
            <script type="module">
              import React from 'react';
              import { createRoot } from 'react-dom';
              ${a.code}
            </script>
          </body></html>`;
      case 'svg':
      case 'html':
        return a.code;
      case 'mermaid':
        return `<!DOCTYPE html><html><head>
          <script src="https://cdn.jsdelivr.net/npm/mermaid/dist/mermaid.min.js"></script>
        </head><body><pre class="mermaid">${a.code}</pre>
          <script>mermaid.initialize({startOnLoad:true});</script>
        </body></html>`;
    }
  }
</script>

<div class="artifact-container">
  <div class="artifact-header">
    <span>{artifact.title}</span>
    <button onclick={() => copyToClipboard(artifact.code)}>Copy</button>
  </div>
  <iframe srcdoc={srcdoc} sandbox="allow-scripts" />
</div>
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
| 框架 | **React 18** | 最小 runtime (~3KB), runes 天然适合 streaming |
| 语言 | **TypeScript** | 类型安全, invoke()/listen() 类型推导 |
| 构建 | **Vite 6** | Tauri 官方推荐, HMR 快 |
| 样式 | **Tailwind CSS 4** | utility-first, 主题系统用 CSS variables |
| Markdown | **marked** + **highlight.js** | 轻量, 已在 MVP 验证 |
| 图标 | **Lucide React** | 轻量图标库, tree-shakable |
| 状态 | **React Runes** ($state/$derived) | 无需外部库, 流式数据天然响应 |
| 测试 | **Vitest** + **@testing-library/svelte** | 单元 + 组件测试 |

**不选 React 的理由**: 对于 Chat UI 这个场景, React 的 useState/useEffect/useRef/useCallback 是过度工程。React 18 Runes 的 `let x = $state(); x += chunk` 直接触发更新, 更符合 streaming 的心智模型。

---

## 九、分阶段实施路线

### Phase 1: 核心聊天 (4-6 周)

**目标**: 替代当前 vanilla JS MVP, 成为可日常使用的桌面聊天工具

```
Week 1-2: React 项目搭建 + 核心组件
  - npm create svelte + Vite + Tailwind 配置
  - ChatPanel + MessageBubble + InputBar
  - MarkdownContent + CodeBlock (代码高亮 + 复制)
  - StreamingIndicator (打字动画)
  - StatusBar

Week 3-4: QueryEngine 集成完善
  - QueryCoordinator 服务层
  - ToolCallBlock (bash 输出, 文件 diff)
  - ThinkingBlock (思考过程折叠)
  - ModelSelector 快速切换
  - 错误处理 + 重试

Week 5-6: 桌面集成 + 打磨
  - 会话持久化 (SessionManager)
  - SettingsOverlay 完整实现
  - 系统托盘 (tauri-plugin-tray 替代)
  - 自动更新 (tauri-plugin-updater)
  - 跨平台测试 (macOS/Windows/Linux)
```

### Phase 2: Agent 编排 (6-8 周)

```
  - Sidebar 会话列表
  - PermissionBridge + PermissionDialog
  - AgentPanel (Agent Dashboard)
  - TaskBoard (Team 任务面板)
  - DiffReview (文件修改审查)
  - McpManager (MCP server 管理 UI)
  - 全局快捷键
  - 文件拖放输入
```

### Phase 3: 差异化功能 (8-12 周)

```
  - ArtifactViewer (iframe 沙盒渲染)
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
        │  ratatui UI     │    │  React 18 UI     │
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
