**状态**: Draft  
**作者**: agent-team  
**最后更新**: 2026-06-23

# D4: Shannon Desktop 事件同步协议设计

## 1. 背景与问题陈述

### 1.1 当前架构

Shannon Desktop 作为 Tauri v2 桌面应用,通过 Tauri IPC 接收来自 shannon-engine 的事件流。当前事件契约定义在 `/home/ed/workspace/backup/shannon-desktop/src/events.rs` (463 行),包含:

- 21 个事件 payload struct (QueryTextPayload, ToolStartPayload 等)
- 21 个 event_names 常量 (magic string 如 `"query:text"`)
- 事件通过 `app_handle.emit()` 发送,frontend 通过 `@tauri-apps/api/event` 的 `listen()` 接收

### 1.2 存在的问题

| 问题 | 影响 | 严重性 |
|------|------|--------|
| **隐式契约** | 21 个事件 payload 在 desktop 端定义,engine 端 emit 时仅按字符串约定,无类型安全保证 | 高 |
| **无版本协商** | engine/desktop 版本不一致时静默坏掉,无任何提示机制 | 高 |
| **Magic string** | 21 个 event_names 常量是硬编码字符串,无 IDE 自动补全,易出错 | 中 |
| **测试失效** | `test_event_names_are_valid` 已坏 (events.rs:353 断言 TASK_STEP.contains(':') 但常量是 "task-step" 用 dash) | 中 |
| **重复定义** | payload 在 desktop 定义,但 shannon-cli/shannon-ui 可能也需要相同结构,违反 DRY | 中 |
| **无 schema 验证** | frontend 收到事件时无类型校验,只能依赖 TypeScript 手动维护 | 中 |

### 1.3 已坏测试分析

`test_event_names_are_valid` (events.rs:347-355) 的问题:

```rust
#[test]
fn test_event_names_are_valid() {
    assert!(event_names::QUERY_TEXT.contains(':'));    // ✅ "query:text"
    assert!(event_names::QUERY_TOOL_START.contains(':')); // ✅ "query:tool-start"
    assert!(event_names::TASK_STEP.contains(':'));       // ❌ "task-step" (无冒号)
    assert!(event_names::TASK_RETRY.contains(':'));     // ❌ "task-retry" (无冒号)
}
```

**问题根因**: TASK_STEP/TASK_RETRY 使用 dash 分隔符 (`task-step`, `task-retry`),而其他事件使用 colon 分隔符 (`query:text`, `query:tool-start`)。

**Frontend 依赖**: `ui/src/hooks/useTaskStreaming.ts` 第 103 行和 121 行硬编码监听 `'task-step'` 和 `'task-retry'` 事件名。

**修复方案**: 修改事件常量为 `task:step` 和 `task:retry` 以统一命名规范,同步更新 frontend 监听器。

---

## 2. 现有 21 个事件清单

当前 `events.rs` 定义的所有事件 payload 和常量:

| 序号 | Payload Struct | 事件名常量 | 说明 | 字段数 |
|------|----------------|------------|------|--------|
| 1 | `QueryTextPayload` | `QUERY_TEXT` = `"query:text"` | LLM 流式文本块 | 2 |
| 2 | `ToolStartPayload` | `QUERY_TOOL_START` = `"query:tool-start"` | 工具调用开始 | 4 |
| 3 | `ToolResultPayload` | `QUERY_TOOL_RESULT` = `"query:tool-result"` | 工具调用完成 | 5 |
| 4 | `ToolProgressPayload` | `QUERY_TOOL_PROGRESS` = `"query:tool-progress"` | 工具进度更新 | 5 |
| 5 | `ThinkingPayload` | `QUERY_THINKING` = `"query:thinking"` | 扩展思考内容 | 2 |
| 6 | `UsagePayload` | `QUERY_USAGE` = `"query:usage"` | Token 用量与成本 | 4 |
| 7 | `QueryCompletedPayload` | `QUERY_COMPLETED` = `"query:completed"` | 查询成功完成 | 1 |
| 8 | `QueryFailedPayload` | `QUERY_FAILED` = `"query:failed"` | 查询失败 | 2 |
| 9 | `QueryCancelledPayload` | `QUERY_CANCELLED` = `"query:cancelled"` | 查询取消 | 1 |
| 10 | `PermissionRequest` | `PERMISSION_REQUEST` = `"permission-request"` | 权限请求 | 4 |
| 11 | `SessionInfo` | `SESSIONS_UPDATED` = `"sessions-updated"` | 会话列表更新 | 7 |
| 12 | `SessionLoaded` | `SESSION_LOADED` = `"session-loaded"` | 会话加载完成 | 1 |
| 13 | `ConfigUpdatedPayload` | `CONFIG_UPDATED` = `"config-updated"` | 配置更新 | 2 |
| 14 | `HunkAction` | `DIFF_REVIEW_AVAILABLE` = `"diff-review-available"` | Diff 审查可用 | 3 |
| 15 | `BackgroundTaskUpdate` | `BACKGROUND_TASK_UPDATE` = `"background-task-update"` | 后台任务更新 | 7 |
| 16 | `BackgroundTaskInfo` | `BACKGROUND_TASKS_UPDATED` = `"background-tasks-updated"` | 后台任务列表更新 | 6 |
| 17 | `UpdateAvailablePayload` | `UPDATE_AVAILABLE` = `"update-available"` | 更新可用 | 3 |
| 18 | `UpdateProgressPayload` | `UPDATE_PROGRESS` = `"update-progress"` | 更新下载进度 | 2 |
| 19 | `DiffFileInfo` | `UPDATE_COMPLETED` = `"update-completed"` | 更新完成 (payload mismatch) | 4 |
| 20 | `TaskStepPayload` | `TASK_STEP` = `"task-step"` (需改为 `"task:step"`) | P1.2 任务步骤流 | 8 |
| 21 | `TaskRetryPayload` | `TASK_RETRY` = `"task-retry"` (需改为 `"task:retry"`) | P1.2 任务重试流 | 7 |

**注意**: 
- `ChatMessage` 是辅助结构,不是独立事件
- `DiffHunk` 是 `DiffFileInfo` 的嵌套结构
- `event_names::CHECK_UPDATES` (line 309) 有常量定义但无对应 payload
- `UPDATE_COMPLETED` 事件名与 `DiffFileInfo` payload 不匹配,疑似历史遗留错误

---

## 3. 方案选型对比

### 3.1 候选方案

| 方案 | 描述 | 优点 | 缺点 |
|------|------|------|------|
| **A. Handshake** | Desktop 启动时向 engine 发送版本协商请求,engine 返回支持的事件版本列表 | 1. 版本显式协商,兼容性明确<br>2. 可提前拒绝不兼容版本 | 1. 增加启动时延 (一次额外 IPC 往返)<br>2. 增加启动耦合:desktop 依赖 engine 可用性<br>3. 需要新增 engine 端握手逻辑<br>4. 复杂度高:需要超时/重试/降级 |
| **B. Envelope + schema_version** | 所有事件包装为 `EventEnvelope<T>`,payload 包含 `schema_version: u16` 字段 | 1. 无启动时延增加<br>2. 解耦:desktop 可独立启动<br>3. 向前兼容:旧 desktop 忽略未知版本<br>4. 向后兼容:新 desktop 支持 v0 fallback<br>5. 实现简单,无需 engine 改动 | 1. 版本不匹配时无提前检测<br>2. 需要运行时版本跟踪机制 |

### 3.2 推荐方案 B 的理由

1. **零启动时延**: 不需要额外的 IPC 往返,desktop 启动性能不受影响
2. **解耦启动**: Desktop 不依赖 engine 立即可用,符合 Tauri 离线优先理念
3. **低成本实现**: 只需修改 payload 结构,无需 engine 端新增逻辑
4. **渐进式迁移**: 可以三阶段迁移 (passthrough → envelope → versioning)
5. **用户友好**: 版本不匹配时通过 UI toast 提示,而非阻止启动

### 3.3 否决 Handshake 的理由

- **Desktop 是唯一 consumer**: Shannon-Engine 目前仅服务于 desktop,无需多端版本协商
- **版本不匹配是低频事件**: Engine/desktop 由同一 repo 发布,版本不一致仅发生在本地开发或升级期间
- **启动耦合代价高**: Handshake 失败会导致 desktop 完全无法启动,用户体验差
- **过度设计**: 单一 consumer 场景下,Handshake 的复杂性不值得

---

## 4. EventEnvelope 设计

### 4.1 核心结构

```rust
// shannon-types/src/events.rs

use serde::{Deserialize, Serialize};

/// 事件包装器,包含 schema 版本和类型标识
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope<T> {
    /// Schema 版本号,从 1 开始递增
    /// v0 保留用于向后兼容 (旧 engine → 新 desktop)
    pub schema_version: u16,
    
    /// 事件类型标识,便于 frontend match
    pub event: EventKind,
    
    /// 实际事件 payload
    pub payload: T,
}

/// 事件类型枚举,对应所有 21 个事件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventKind {
    // Query 相关事件 (1-9)
    QueryText,
    QueryToolStart,
    QueryToolResult,
    QueryToolProgress,
    QueryThinking,
    QueryUsage,
    QueryCompleted,
    QueryFailed,
    QueryCancelled,
    
    // 权限与会话 (10-12)
    PermissionRequest,
    SessionsUpdated,
    SessionLoaded,
    
    // 配置与 Diff (13-14)
    ConfigUpdated,
    DiffReviewAvailable,
    
    // 后台任务 (15-16)
    BackgroundTaskUpdate,
    BackgroundTasksUpdated,
    
    // 更新事件 (17-18)
    UpdateAvailable,
    UpdateProgress,
    UpdateCompleted,
    
    // 任务流 (19-20)
    TaskStep,
    TaskRetry,
}

/// 事件名称常量,用于 Tauri emit/listen
pub mod event_names {
    pub const QUERY_TEXT: &str = "query:text";
    pub const QUERY_TOOL_START: &str = "query:tool-start";
    pub const QUERY_TOOL_RESULT: &str = "query:tool-result";
    pub const QUERY_TOOL_PROGRESS: &str = "query:tool-progress";
    pub const QUERY_THINKING: &str = "query:thinking";
    pub const QUERY_USAGE: &str = "query:usage";
    pub const QUERY_COMPLETED: &str = "query:completed";
    pub const QUERY_FAILED: &str = "query:failed";
    pub const QUERY_CANCELLED: &str = "query:cancelled";
    pub const PERMISSION_REQUEST: &str = "permission-request";
    pub const SESSIONS_UPDATED: &str = "sessions-updated";
    pub const SESSION_LOADED: &str = "session-loaded";
    pub const CONFIG_UPDATED: &str = "config-updated";
    pub const DIFF_REVIEW_AVAILABLE: &str = "diff-review-available";
    pub const BACKGROUND_TASK_UPDATE: &str = "background-task-update";
    pub const BACKGROUND_TASKS_UPDATED: &str = "background-tasks-updated";
    pub const UPDATE_AVAILABLE: &str = "update-available";
    pub const UPDATE_PROGRESS: &str = "update-progress";
    pub const UPDATE_COMPLETED: &str = "update-completed";
    pub const TASK_STEP: &str = "task:step";      // 修复: 原 "task-step"
    pub const TASK_RETRY: &str = "task:retry";    // 修复: 原 "task-retry"
}
```

### 4.2 使用示例

```rust
// Engine 端发送事件 (伪代码)
let envelope = EventEnvelope {
    schema_version: 1,
    event: EventKind::QueryText,
    payload: QueryTextPayload {
        query_id: "abc".into(),
        content: "hello".into(),
    },
};
app.emit(event_names::QUERY_TEXT, envelope)?;

// Desktop 端接收事件 (伪代码)
let envelope: EventEnvelope<QueryTextPayload> = event.payload?;
match envelope.event {
    EventKind::QueryText => {
        // 处理查询文本
        println!("收到查询文本: {}", envelope.payload.content);
    }
    _ => {}
}
```

---

## 5. Payload 迁移计划

### 5.1 Schema 版本分配

所有 payload 从 schema_version = 1 开始:

| Payload | 起始版本 | 理由 |
|---------|----------|------|
| 所有 21 个 payload | v1 | 首次引入版本字段 |

### 5.2 shannon-types::events 模块结构

```
shannon-types/src/
├── lib.rs              # 现有类型 (EntityId, Message, ToolUse 等)
└── events.rs           # 新增模块
    ├── mod.rs          # 模块入口,导出 EventEnvelope, EventKind, event_names
    ├── payloads.rs     # 21 个 payload struct 定义
    ├── kinds.rs        # EventKind enum 定义
    └── names.rs        # event_names 常量定义
```

### 5.3 迁移三阶段

#### Phase 1: 创建共享模块 (不破坏现有代码)

在 `shannon-types` 创建 `events` 模块,原始 desktop `events.rs` 保持不变:

```rust
// shannon-types/src/events.rs
pub mod payloads;
pub mod kinds;
pub mod names;

pub use payloads::*;
pub use kinds::*;
pub use names::*;

// Passthrough: 重新导出 desktop 的类型作为别名
pub use shannon_desktop_events::{
    QueryTextPayload, ToolStartPayload, // ... 全部 21 个
};
```

**验证**: 
- Desktop 编译通过
- `cargo test` 通过
- 无任何运行时行为变化

#### Phase 2: Desktop 切换 import 来源

```rust
// src/events.rs (desktop)
// 旧: 直接定义 payload
// 新: 从 shannon-types 导入

use shannon_types::events::*;

// 本地保留 emit helper 函数 (如 emit_task_step)
// 但 payload 类型从共享模块导入
```

**验证**:
- Desktop 编译通过
- 所有测试通过
- 无运行时行为变化

#### Phase 3: 加 schema_version 字段

将所有 payload 包装为 `EventEnvelope<T>`:

```rust
// 旧: app.emit(event_names::QUERY_TEXT, payload)
// 新: app.emit(event_names::QUERY_TEXT, EventEnvelope { schema_version: 1, event, payload })
```

**验证**:
- Desktop 编译通过
- 所有测试更新并通过
- Frontend TypeScript 类型同步更新

---

## 6. 向前兼容策略

### 6.1 加字段 (Minor 变更)

新 desktop 可忽略旧 engine 未提供的新字段:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryTextPayload {
    pub query_id: String,
    pub content: String,
    /// v2 新增字段
    #[serde(default)]
    pub metadata: Option<String>,
}
```

**处理**: 旧 engine 发送的 payload 缺少 `metadata` 字段,serde `#[serde(default)]` 自动填充 `None`。

### 6.2 删字段 / 改语义 (Major 变更)

删除字段或改变字段语义必须递增 schema_version:

```rust
// v1
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultPayload {
    pub query_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub result: String,
    pub is_error: bool,
}

// v2: 删除 tool_use_id,改用 tool_use_id_list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultPayload {
    pub query_id: String,
    /// v2: 批量工具调用
    pub tool_use_id_list: Vec<String>,
    pub tool_name: String,
    pub result: String,
    pub is_error: bool,
}
```

**处理**: Desktop 收到 `schema_version > SUPPORTED_MAX` 时触发 mismatch 提示 (见第 8 节)。

### 6.3 改类型 (禁止)

禁止直接修改字段类型,必须使用新字段名:

```rust
// ❌ 错误: 直接改类型
pub status: String,  // 旧: "running" | "completed"
pub status: Status,  // 新: enum

// ✅ 正确: 新字段名
pub status: String,           // 保留兼容
pub status_v2: Option<Status>, // v2 新增
```

**理由**: 避免旧 JSON 反序列化失败。

---

## 7. 向后兼容策略

### 7.1 V0 兼容分支

新 desktop 支持旧 engine 发送的裸 payload (无 envelope):

```rust
// Desktop 接收事件处理
pub fn handle_tauri_event<T>(event: TauriEvent<T>) {
    // 尝试解析为 envelope
    if let Ok(envelope) = serde_json::from_str::<EventEnvelope<T>>(&event.payload) {
        // v1+: 有 envelope 的事件
        match envelope.schema_version {
            1 => handle_v1(envelope.payload),
            _ if envelope.schema_version > SUPPORTED_MAX => {
                record_mismatch(envelope.schema_version);
                handle_v1_fallback(envelope.payload); // 尝试按 v1 处理
            }
            _ => {}
        }
    } else {
        // v0: 旧 engine 发送的裸 payload
        let payload: T = serde_json::from_str(&event.payload)?;
        handle_v0(payload);
    }
}
```

### 7.2 版本跟踪

Desktop 维护 `observed_schema_versions` set:

```rust
// src/state.rs
pub struct SchemaTracker {
    pub observed_versions: HashSet<u16>,
    pub supported_max: u16,
}

impl SchemaTracker {
    pub fn record(&mut self, version: u16) {
        self.observed_versions.insert(version);
    }
    
    pub fn has_mismatch(&self) -> bool {
        self.observed_versions.iter().any(|&v| v > self.supported_max)
    }
}
```

启动后 30 秒检查是否触发 mismatch:

```rust
// src/main.rs::setup()
std::thread::spawn(move || {
    std::thread::sleep(Duration::from_secs(30));
    if schema_tracker.has_mismatch() {
        app.emit("protocol-mismatch", ProtocolMismatchPayload {
            max_observed: *schema_tracker.observed_versions.iter().max().unwrap(),
            supported_max: schema_tracker.supported_max,
        })?;
    }
});
```

---

## 8. UI Mismatch 提示机制

### 8.1 新事件

```rust
/// 协议版本不匹配事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMismatchPayload {
    /// 观察到的最高 schema 版本
    pub max_observed: u16,
    /// Desktop 支持的最高版本
    pub supported_max: u16,
}

pub mod event_names {
    // ... 现有常量
    pub const PROTOCOL_MISMATCH: &str = "protocol-mismatch";
}
```

### 8.2 UI Toast 组件

```tsx
// ui/src/components/ProtocolMismatchToast.tsx
import { useTauriEvent } from '@/hooks/useTauriEvent'

interface MismatchEvent {
  maxObserved: number
  supportedMax: number
}

export function ProtocolMismatchToast() {
  const [visible, setVisible] = useState(false)
  const [event, setEvent] = useState<MismatchEvent | null>(null)

  useTauriEvent<MismatchEvent>('protocol-mismatch', (e) => {
    setEvent(e.payload)
    setVisible(true)
  })

  if (!visible || !event) return null

  return (
    <div className="fixed bottom-4 right-4 bg-yellow-500 text-white p-4 rounded shadow-lg">
      <h3>协议版本不匹配</h3>
      <p>
        Engine 版本 (v{event.maxObserved}) 高于 Desktop 支持版本 (v{event.supportedMax})。
        某些功能可能不可用。请升级 Shannon Desktop。
      </p>
      <button onClick={() => setVisible(false)}>关闭</button>
    </div>
  )
}
```

### 8.3 i18n Key 草案

```json
// ui/src/i18n/en.json
{
  "protocolMismatch": {
    "title": "Protocol Version Mismatch",
    "message": "Engine version (v{maxObserved}) is higher than Desktop supported version (v{supportedMax}). Some features may be unavailable. Please upgrade Shannon Desktop.",
    "close": "Close"
  }
}

// ui/src/i18n/zh-CN.json
{
  "protocolMismatch": {
    "title": "协议版本不匹配",
    "message": "Engine 版本 (v{maxObserved}) 高于 Desktop 支持版本 (v{supportedMax})。某些功能可能不可用。请升级 Shannon Desktop。",
    "close": "关闭"
  }
}
```

---

## 9. JSON Schema 自动生成

### 9.1 build.rs 实现

```rust
// build.rs
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/events.rs");
    
    // 生成 schema 文件路径
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let schema_path = out_dir.join("events.schema.json");
    
    // 生成 JSON Schema
    let schema = generate_event_schema();
    
    // 写入文件
    fs::write(&schema_path, schema).unwrap();
    
    // 复制到 docs/ 目录
    let docs_schema = PathBuf::from("docs/schema/events.schema.json");
    fs::create_dir_all(docs_schema.parent().unwrap()).unwrap();
    fs::copy(&schema_path, &docs_schema).unwrap();
}

fn generate_event_schema() -> String {
    // 这里使用 schemars 库自动生成
    // 实际实现需要引入 schemars 依赖
    r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "Shannon Desktop Events",
  "description": "Schema for all Tauri events emitted by Shannon Desktop",
  "definitions": {
    "EventEnvelope": {
      "type": "object",
      "properties": {
        "schemaVersion": { "type": "integer", "minimum": 1 },
        "event": { "$ref": "#/definitions/EventKind" },
        "payload": { "type": "object" }
      },
      "required": ["schemaVersion", "event", "payload"]
    }
    // ... 其他 21 个 payload 定义
  }
}"#.to_string()
}
```

### 9.2 Schema 文件位置

```
docs/schema/events.schema.json   # 人类可读的 schema
target/debug/build/events.schema.json  # build 输出
```

### 9.3 前端集成

```json
// ui/package.json
{
  "scripts": {
    "generate-types": "json-schema-to-typescript docs/schema/events.schema.json src/types/events.schema.d.ts"
  },
  "devDependencies": {
    "json-schema-to-typescript": "^13.0.0"
  }
}
```

```bash
# 生成 TypeScript 类型定义
pnpm generate-types

# 在 ui/src/lib/tauri-api.ts 中引用
/// <reference types="../types/events.schema.d.ts" />
```

### 9.4 类型一致性校验

在 `ui/src/lib/tauri-api.ts` 中使用生成的类型:

```typescript
// 自动生成的类型 (events.schema.d.ts)
export interface EventEnvelope<T> {
  schemaVersion: number;
  event: EventKind;
  payload: T;
}

// tauri-api.ts 手动维护的类型
export interface QueryTextPayload {
  queryId: string;
  content: string;
}

// 类型校验脚本
import { EventEnvelope } from '../types/events.schema.d.ts';
import { QueryTextPayload } from './types';

// 编译时校验:确保手动类型与 schema 类型一致
type Test = EventEnvelope<QueryTextPayload>; // 如果不匹配会报错
```

---

## 10. 迁移路径

### 10.1 Phase 1: 创建共享模块

**目标**: 在 `shannon-types` 创建 `events` 模块,不破坏现有代码

**步骤**:
1. 在 `shannon-types/src/events.rs` 创建模块结构
2. 将 21 个 payload struct 复制到 `shannon-types::events::payloads`
3. 在 `shannon-types` 重新导出,在 desktop 中 import 作为类型别名
4. 运行 `cargo test` 确保无破坏

**验收标准**:
- `cargo build --workspace` 通过
- `cargo test --workspace` 通过
- 无运行时行为变化

### 10.2 Phase 2: Desktop 切换 import 来源

**目标**: Desktop 从 `shannon-types` 导入 payload 类型

**步骤**:
1. 修改 `src/events.rs`,从 `use shannon_types::events::*` 导入 payload
2. 删除 desktop 本地的 payload 定义
3. 保留 emit helper 函数 (`emit_task_step`, `emit_task_retry`)
4. 运行所有测试

**验收标准**:
- Desktop 编译通过
- 所有测试通过
- 无运行时行为变化

### 10.3 Phase 3: 加 schema_version 字段

**目标**: 所有事件包装为 `EventEnvelope<T>`

**步骤**:
1. 修改所有 `app.emit()` 调用,包装 payload 为 envelope
2. 修改 frontend 事件监听器,解析 envelope
3. 实现 `schema_tracker` 版本跟踪
4. 实现 `protocol-mismatch` 事件和 UI toast
5. 更新所有测试用例
6. 修复 `test_event_names_are_valid` (TASK_STEP → "task:step")

**验收标准**:
- 所有测试通过
- UI toast 正确显示版本不匹配
- 向前兼容:旧 engine → 新 desktop 降级处理
- 向后兼容:新 engine → 旧 desktop 忽略未知字段

---

## 11. 测试策略

### 11.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_envelope_serialization() {
        let envelope = EventEnvelope {
            schema_version: 1,
            event: EventKind::QueryText,
            payload: QueryTextPayload {
                query_id: "abc".into(),
                content: "hello".into(),
            },
        };
        let json = serde_json::to_string(&envelope).unwrap();
        let parsed: EventEnvelope<QueryTextPayload> = 
            serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.payload.content, "hello");
    }

    #[test]
    fn test_event_names_format() {
        // 修复后的测试:统一使用冒号分隔符
        assert!(event_names::QUERY_TEXT.contains(':'));
        assert!(event_names::TASK_STEP.contains(':'));
        assert!(event_names::TASK_RETRY.contains(':'));
    }

    #[test]
    fn test_schema_tracker() {
        let mut tracker = SchemaTracker::new();
        tracker.record(1);
        tracker.record(2);
        tracker.record(3);
        assert_eq!(tracker.observed_versions.len(), 3);
        assert!(tracker.has_mismatch()); // 假设 SUPPORTED_MAX = 2
    }
}
```

### 11.2 集成测试

```rust
#[test]
fn test_end_to_end_event_flow() {
    // 模拟 engine 发送事件
    let envelope = EventEnvelope {
        schema_version: 1,
        event: EventKind::QueryText,
        payload: QueryTextPayload {
            query_id: "test".into(),
            content: "world".into(),
        },
    };
    
    // 模拟 Tauri emit
    let json = serde_json::to_string(&envelope).unwrap();
    
    // 模拟 desktop 接收
    let received: EventEnvelope<QueryTextPayload> = 
        serde_json::from_str(&json).unwrap();
    
    assert_eq!(received.payload.content, "world");
}
```

### 11.3 契约测试

使用 JSON Schema 校验真实事件:

```rust
#[test]
fn test_event_conformance() {
    // 读取生成的 schema
    let schema = serde_json::from_str::<serde_json::Value>(
        &fs::read_to_string("docs/schema/events.schema.json").unwrap()
    ).unwrap();
    
    // 验证每个 payload 符合 schema
    let envelope = create_test_envelope();
    let json = serde_json::to_value(&envelope).unwrap();
    
    let validator = jsonschema::Validator::new(&schema).unwrap();
    assert!(validator.validate(&json).is_ok());
}
```

---

## 12. 修复已坏测试

### 12.1 问题分析

`test_event_names_are_valid` 失败原因:

```rust
// events.rs:353
assert!(event_names::TASK_STEP.contains(':'));  // ❌ "task-step" 无冒号
assert!(event_names::TASK_RETRY.contains(':')); // ❌ "task-retry" 无冒号
```

### 12.2 修复方案

**方案 A: 修改事件常量** (推荐)

```rust
// events.rs:312-315
pub const TASK_STEP: &str = "task:step";   // 修改: 原 "task-step"
pub const TASK_RETRY: &str = "task:retry"; // 修改: 原 "task-retry"
```

**Frontend 同步修改**:

```typescript
// ui/src/hooks/useTaskStreaming.ts:103,121
unlistenStep = await listen<RawTaskStepEvent>('task:step', e => {  // 修改: 原 'task-step'
  // ...
})
unlistenRetry = await listen<RawTaskRetryEvent>('task:retry', e => { // 修改: 原 'task-retry'
  // ...
})
```

**影响分析**:
- `useTaskStreaming.ts` 是唯一监听这两个事件的前端代码
- 修改后测试通过,且统一命名规范

**方案 B: 修改测试断言** (不推荐)

```rust
// 仅修改测试,不修改常量
assert!(event_names::TASK_STEP.contains('-'));  // 改为检查 dash
assert!(event_names::TASK_RETRY.contains('-'));
```

**不推荐理由**:
- 保留命名不一致问题
- 违反其他事件使用冒号的约定

### 12.3 验证步骤

1. 修改 `events.rs` 常量定义
2. 修改 `useTaskStreaming.ts` 监听器
3. 运行 `cargo test test_event_names_are_valid`
4. 运行 `pnpm test` 验证前端测试通过

---

## 13. 实施检查清单

### 13.1 准备阶段

- [ ] 创建 `docs/architecture/d4-state-sync-protocol.md` (本文档)
- [ ] 团队评审方案 B (Envelope + schema_version)
- [ ] 确认否决 Handshake 方案
- [ ] 评估迁移工作量

### 13.2 Phase 1: 共享模块

- [ ] 在 `shannon-types/src/events.rs` 创建模块
- [ ] 复制 21 个 payload 到 `shannon-types`
- [ ] 在 `shannon-types` 重新导出类型
- [ ] Desktop 添加 `shannon-types` 依赖
- [ ] 运行 `cargo test --workspace` 验证

### 13.3 Phase 2: 切换 Import

- [ ] 修改 `src/events.rs` 从 `shannon-types` 导入
- [ ] 删除 desktop 本地 payload 定义
- [ ] 保留 emit helper 函数
- [ ] 运行所有测试

### 13.4 Phase 3: 加版本字段

- [ ] 实现 `EventEnvelope<T>` 包装
- [ ] 修改所有 `app.emit()` 调用
- [ ] 实现 `schema_tracker` 版本跟踪
- [ ] 实现 `protocol-mismatch` 事件
- [ ] 添加 UI toast 组件
- [ ] 添加 i18n 翻译
- [ ] 修复 `test_event_names_are_valid`
- [ ] 修改 `useTaskStreaming.ts` 事件名
- [ ] 更新所有测试用例

### 13.5 Schema 生成

- [ ] 实现 `build.rs` schema 生成
- [ ] 添加 `schemars` 依赖
- [ ] 生成 `docs/schema/events.schema.json`
- [ ] 前端集成 `json-schema-to-typescript`
- [ ] 添加类型校验脚本

### 13.6 验证与发布

- [ ] 运行完整测试套件
- [ ] 手动测试版本不匹配场景
- [ ] 更新 CHANGELOG.md
- [ ] 提交 PR 到 `main`
- [ ] 发布新版本

---

## 14. 风险与缓解

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| Frontend 硬编码事件名遗漏 | 中 | 高 | 全面 Grep 搜索 `useTauriEvent` 调用,逐个检查 |
| 向后兼容分支覆盖不全 | 中 | 中 | 增加 v0 payload 测试用例 |
| Schema 生成失败 | 低 | 中 | 提前验证 `schemars` 兼容性 |
| 性能回归 (envelope 序列化) | 低 | 低 | 基准测试,确保无显著时延 |
| 类型生成工具不兼容 | 低 | 低 | 备选方案:手动维护 TypeScript 类型 |

---

## 15. 后续优化方向

1. **自动迁移脚本**: 编写脚本自动将现有事件调用包装为 envelope
2. **版本迁移指南**: 为每个 major 版本升级编写迁移文档
3. **契约测试 CI**: 在 CI 中集成 schema 校验,防止类型漂移
4. **事件文档生成**: 从 schema 自动生成 Markdown 事件文档
5. **向前兼容性测试**: 测试新 desktop 对旧 engine 的兼容性

---

## 附录 A: 相关文件清单

| 文件路径 | 作用 | 修改范围 |
|----------|------|----------|
| `src/events.rs` | 当前事件定义 | 删除 payload,保留 emit helper |
| `src/commands.rs` | 命令处理,emit 事件 | 修改 emit 调用 |
| `ui/src/hooks/useTaskStreaming.ts` | 任务流监听 | 修改事件名 |
| `ui/src/hooks/useTauriEvent.ts` | 通用事件监听 | 无修改 |
| `ui/src/lib/tauri-api.ts` | Tauri API 包装 | 添加 envelope 解析 |
| `shannon-types/src/events.rs` | 新增模块 | 新增文件 |
| `shannon-types/Cargo.toml` | 依赖配置 | 无修改 |
| `build.rs` | Schema 生成 | 新增 schema 生成逻辑 |
| `docs/schema/events.schema.json` | JSON Schema | 新增文件 |

---

## 附录 B: 类型映射表

| Events.rs 原类型 | shannon-types 类型 | serde rename_all |
|------------------|-------------------|------------------|
| `QueryTextPayload` | `QueryTextPayload` | camelCase |
| `ToolStartPayload` | `ToolStartPayload` | camelCase |
| `ToolResultPayload` | `ToolResultPayload` | camelCase |
| `ToolProgressPayload` | `ToolProgressPayload` | camelCase |
| `ThinkingPayload` | `ThinkingPayload` | camelCase |
| `UsagePayload` | `UsagePayload` | camelCase |
| `QueryCompletedPayload` | `QueryCompletedPayload` | camelCase |
| `QueryFailedPayload` | `QueryFailedPayload` | camelCase |
| `QueryCancelledPayload` | `QueryCancelledPayload` | camelCase |
| `PermissionRequest` | `PermissionRequest` | camelCase |
| `SessionInfo` | `SessionInfo` | camelCase |
| `SessionLoaded` | `SessionLoaded` | camelCase |
| `ConfigUpdatedPayload` | `ConfigUpdatedPayload` | camelCase |
| `HunkAction` | `HunkAction` | camelCase |
| `BackgroundTaskUpdate` | `BackgroundTaskUpdate` | camelCase |
| `BackgroundTaskInfo` | `BackgroundTaskInfo` | camelCase |
| `UpdateAvailablePayload` | `UpdateAvailablePayload` | camelCase |
| `UpdateProgressPayload` | `UpdateProgressPayload` | camelCase |
| `DiffFileInfo` | `DiffFileInfo` | camelCase |
| `TaskStepPayload` | `TaskStepPayload` | camelCase |
| `TaskRetryPayload` | `TaskRetryPayload` | camelCase |
