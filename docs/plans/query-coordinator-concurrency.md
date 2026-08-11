# QueryCoordinator 并发能力验证

> 验证日期: 2026-08-02
> 目的: 为 P2-5b 多线程管理决策提供依据

## 1. 实现位置

**关键发现: `QueryCoordinator` 在当前代码库中并不存在。** `docs/desktop-architecture.md:384-418` 描述了一个**设计草案**(`pub struct QueryCoordinator { ... }`),但代码侧实现缺失,真正承担"协调一次 chat 查询"职责的实体是:

| 角色 | 实际位置 | 路径 |
|---|---|---|
| Tauri 入口 / 单飞闸 | `desktop/src/commands.rs` (`pub struct AppState`, `pub async fn send_message`) | `desktop/src/commands.rs:57`、`desktop/src/commands.rs:415` |
| 查询引擎本身 | `shannon-core::query_engine::QueryEngine` | `crates/shannon-core/src/query_engine/engine.rs:335` |
| 引擎入口方法 | `QueryEngine::process_query` | `crates/shannon-core/src/query_engine/engine.rs:1053` |

P2-5b 的多线程决策必须基于 `desktop/src/commands.rs` + `QueryEngine` 这条真实路径,不能基于桌面架构文档中的设想类型。

```rust
// desktop/src/commands.rs:57-114 (节选,真正的"coordinator")
pub struct AppState {
    pub(crate) messages: Arc<Mutex<Vec<ChatMessage>>>,        // 单例会话消息
    pub(crate) querying: Arc<Mutex<bool>>,                    // <-- 单飞闸
    pub(crate) client_config: Arc<RwLock<LlmClientConfig>>,
    pub(crate) qe_config: Arc<RwLock<...QueryEngineConfig>>,
    pub(crate) tools: Arc<ToolRegistry>,                      // 单例 ToolRegistry
    ...
    pub(crate) cancellation_token: Arc<Mutex<Option<CancellationToken>>>, // <-- 单 token
    pub(crate) current_session_id: Arc<Mutex<Option<Uuid>>>,  // 单活动会话
    pub(crate) usage_store: Arc<crate::commands_usage::UsageStore>,
    ...
}
```

```rust
// crates/shannon-core/src/query_engine/engine.rs:335-365 (QueryEngine 内部状态)
pub struct QueryEngine {
    pub(crate) client: LlmClient,                              // 单 LLM 连接
    pub(crate) tools: Arc<ToolRegistry>,
    pub(crate) permissions: Arc<RwLock<PermissionManager>>,
    pub(crate) state: Arc<StateManager>,
    pub(crate) config: QueryEngineConfig,
    pub(crate) conversation: ConversationState,                // <-- 内嵌会话历史
    pub(crate) cost_tracker: Arc<RwLock<CostTracker>>,
    pub(crate) memory: Option<Arc<std::sync::RwLock<MemoryStore>>>,
    pub(crate) session_id: Uuid,                               // <-- 单 session_id
    pub(crate) hook_manager: Arc<tokio::sync::RwLock<HookManager>>,
    pub(crate) triggered_routines: Arc<tokio::sync::RwLock<...>>,
    pub(crate) context_injector: Option<Arc<ContextInjector>>,
    pub(crate) plan_mode_active: Arc<RwLock<bool>>,
    pub(crate) checkpoint_manager: crate::checkpoint::CheckpointManager,
    pub(crate) effective_max_context_tokens: usize,
    pub(crate) custom_profiles: Arc<tokio::sync::RwLock<...>>,
}
```

## 2. 并发模型

| 层 | 同步原语 | 性质 |
|---|---|---|
| `AppState::querying: Arc<Mutex<bool>>` (desktop:61) | tokio `Mutex<bool>` | 显式**单飞** —— 第二个并发请求被硬性拒绝("A query is already in progress") |
| `AppState::messages: Arc<Mutex<Vec<ChatMessage>>>` (desktop:59) | tokio `Mutex<Vec<ChatMessage>>` | **单例会话**,所有会话共用一条 `Vec` |
| `AppState::cancellation_token: Arc<Mutex<Option<CancellationToken>>>` (desktop:98) | tokio `Mutex<Option<CancellationToken>>` | **单 token**,只表示"当前正在跑的那一次" |
| `AppState::current_session_id: Arc<Mutex<Option<Uuid>>>` | tokio `Mutex` | 单活动 session |
| `AppState::client_config: Arc<RwLock<LlmClientConfig>>` (desktop:90) | tokio `RwLock` | 全局共享 provider/model 配置,允许并发读 |
| `AppState::tools: Arc<ToolRegistry>` (desktop:80) | 内部已 Arc 化,通常无外层锁 | 共享工具注册表 |
| `QueryEngine::conversation: ConversationState` (engine.rs:341) | **裸字段,无内部同步** | 每个 engine 实例独占一份对话历史 |
| `QueryEngine::session_id: Uuid` (engine.rs:346) | 无同步 | 引擎实例绑定单一 session |

- channel: N/A(无 `tokio::mpsc` 任务队列;`process_query` 是直接 `tokio::spawn` 的 future)
- per-query state: 严格说没有 —— `QueryEngine` 自身承载 `conversation` / `session_id` / `cost_tracker`,因此**单 `QueryEngine` 实例天然只代表一个会话**
- 多 `QueryEngine` 实例之间不共享对话(每次 `send_message` 都会在 desktop:512 现场 `QueryEngine::with_defaults_arc(...)` 重新构造一个)

## 3. 关键代码片段

**desktop/src/commands.rs:421-435 (Tauri 入口处的硬性单飞闸)**
```rust
// Prevent concurrent queries — check and set in a single lock scope to avoid TOCTOU race
{
    let mut querying = state.querying.lock().await;
    if *querying {
        return Err("A query is already in progress".into());
    }
    *querying = true;
}

// Create cancellation token
let cancel_token = CancellationToken::new();
{
    let mut token_guard = state.cancellation_token.lock().await;
    *token_guard = Some(cancel_token.clone());
}
```

**desktop/src/commands.rs:482-512 (单例消息追加 + 临时构造 QueryEngine)**
```rust
{
    let mut messages = state.messages.lock().await;
    messages.push(ChatMessage { role: "user".into(), ... });
}
...
let engine = QueryEngine::with_defaults_arc(client, tools, permissions, StateManager::new());
let context = QueryContext { query_id, session_id: uuid::Uuid::new_v4(), ... };
```

**desktop/src/commands.rs:543-559 (spawn + 流式消费,串行绑定 query_id)**
```rust
let return_qid = qid_str.clone();
tokio::spawn(async move {
    let stream = engine.process_query(context, None).await;
    let mut final_content = String::new();
    ...
    while let Some(event_result) = pin_stream.next().await {
        if cancel_token_clone.is_cancelled() { ... }
        ...
    }
});
```

**crates/shannon-core/src/query_engine/engine.rs:1053-1066 (process_query 入口,无任何全局锁)**
```rust
pub async fn process_query(
    &self,
    context: QueryContext,
    permission_request_tx: Option<mpsc::UnboundedSender<...>>,
) -> QueryStream {
    let query_id = context.query_id;
    let config = self.config.clone();
    ...
    let (tx, rx) = mpsc::unbounded_channel();
    let tools = self.tools.clone();
    let permissions = self.permissions.clone();
    ...
    // 单个 engine 实例的内部 conversation 状态(被 this self 独占)限制了并发
}
```

## 4. 裁决

**⚠️ 串行化在调用方一层 —— P2-5b 不能"多 query 共用一个 coordinator",必须给每个并行分支分配独立 `QueryEngine` 实例(或拆出真正的 `QueryCoordinator`)**

补充裁决要点:
- `QueryEngine::process_query` 本身**没有进程级锁**,多实例多 session 跑起来是 OK 的;
- 但 `QueryEngine.conversation / session_id / cost_tracker` 都是实例内裸字段,所以**单实例天然 = 单会话**;要让 P2-5b 的"多线程 chat"工作,必须每个并发分支持有自己的 `QueryEngine`;
- 真正的瓶颈在 `AppState`:单例 `messages` / `querying` 闸 / 单 `cancellation_token` / 单 `current_session_id`,这套是为"单活动会话"假设而写的;
- `docs/desktop-architecture.md:389-397` 里规划的 `QueryCoordinator` 字段(`querying: Arc<Mutex<bool>>` + `cancel_token: Arc<Mutex<Option<CancellationToken>>>`)就是当前 `AppState` 子集的封装,本质仍是单飞设计 —— **该文档与现状一致,只是换了个名字**。

## 5. P2-5b 建议方案

**方案 A(推荐,小改动) — 多个 `QueryEngine` 实例 + `AppState` 重构**:
- `AppState` 由"单 `messages: Vec<ChatMessage>`"改为 `messages_by_session: Arc<Mutex<HashMap<SessionId, Vec<ChatMessage>>>>`,`querying` 拆为 `active_queries: Arc<Mutex<HashMap<QueryId, QueryMeta>>>`(允许同一 session 内多 query、不同 session 互不阻塞);
- 每次 `send_message` 按 `session_id` 拉/建一个 `QueryEngine`,放在 `Arc<Mutex<HashMap<SessionId, Arc<QueryEngine>>>>`(或 `Arc<RwLock<...>>`);
- `cancellation_token` 拆为 `Arc<Mutex<HashMap<QueryId, CancellationToken>>>`;
- 前端为每个并发 tab/分支独立 `query_id`;事件 `query_id` 已带,可直接路由。
- 工作量: medium(主要改 `AppState` + `commands.rs::send_message`/`cancel_query`,把"按 session 复用 engine"实现出来;`QueryEngine` 本身不动)
- 风险: medium(影响所有 chat 路径,需要把 `state.messages` 的全部调用点迁到 `messages_by_session`;`UsageStore` / `StateManager` 是否需要按 session 拆分也要复核)

**方案 B(架构干净,但大改) — 真正落 `docs/desktop-architecture.md` 中的 `QueryCoordinator`,支持 per-query 并发**:
- 把 `AppState::send_message` 的单飞闸、`QueryEngine` 实例化、流式派发改到 `QueryCoordinator`;
- `QueryCoordinator` 持 `HashMap<QueryId, QueryHandle>` + `tokio::mpsc` 任务队列,每条 `QueryHandle` 自带一个 `QueryEngine` + 自己的 `CancelToken` + 自己的 `messages`(完整封装);
- `AppState` 退化为路由层,前端按 `query_id` 寻址。
- 工作量: large
- 风险: high(touch 所有 chat 路径 + 前端事件路由;与 P2-5a/P2-5c 的 chat session 改造交叉)

**不建议**:继续扩展当前 `AppState` 加 `if let` 分支(复杂度累积,无法回到干净模型)。

**给 P2-5b 的明确结论**:
- 现有 `AppState` 串行化一切 → 多线程 chat 不可行,必须先重构;
- `QueryEngine` 本身是多线程安全的(无内部锁),可作为并发单元,但要每个 session 独立一个;
- 走方案 A,工作量和风险都可控;走方案 B 干净但与 P2-5a 重叠,建议在 P2-5a 把"多 session"先稳住,再决定是否上方案 B。
