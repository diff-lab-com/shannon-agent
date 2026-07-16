# Scheduled 页对接 Shannon Routine 后端 — 修复/改进执行计划 v3

**日期**: 2026-06-13 (v3,Sprint 1 完成后基于实战校准)
**v2 历史**: 2026-06-13 (v2,基于 Claude Code/Codex/Cowork/Hermes 竞品调研更新)
**范围**: 让 `/tasks` 页(Sidebar 标为 "Scheduled")真正对接 Shannon 后端的 routine/cron/webhook 能力,而非当前的「假 scheduled」状态。
**前置文档**: `OPC-SCHEDULED-GAP-ANALYSIS.md`(差距分析)、`COMPETITIVE-ANALYSIS.md`(竞品对标)
**Sprint 1 状态**: ✅ 完成 — 69 个测试通过,3 个新模块(`scheduled_routines` 扩展 / `scheduled_task_store` / `scheduled_runs`)

---

## 0. 竞品调研结论(v2 新增,据此调整方案)

### 0.1 Claude Code(行业标准,事实标准)

**存储格式** ⭐ 关键发现:
```
~/.claude/scheduled-tasks/<task-name>/
  ├── SKILL.md          # Prompt + YAML frontmatter (name, description)
  └── task.json         # 调度配置
```

**task.json 字段**(参照 [Claude Code #47797](https://github.com/anthropics/claude-code/issues/47797)):
```json
{
  "id": "my-task",
  "cronExpression": "*/30 9-20 * * 1-5",
  "enabled": true,
  "model": "claude-sonnet-4-6",
  "cwd": "/path/to/project",
  "permissionMode": "acceptEdits",
  "approvedPermissions": [{"toolName": "Bash"}, {"toolName": "Read"}]
}
```

**特性**:
- **5-field cron**:`minute hour day-of-month month day-of-week`(本地时区)
- **`/loop` 命令**:`/loop 5m check the deploy` / `/loop 30m /review-pr 1234`
- **Jitter**:`:00`/`:30` 触发时刻最多提前 90 秒(避免 API thundering herd)
- **7 天过期**:CLI `/loop` 任务 7 天后自动失效
- **50 任务/session 上限**
- **三档分层**:Cloud(云端)/ Desktop(本地持久)/ `/loop`(session-scoped)
- **工具**:`CronCreate` / `CronList` / `CronDelete`
- **禁用**:`CLAUDE_CODE_DISABLE_CRON=1`
- **最小间隔**:1 分钟(desktop)、1 小时(cloud)

### 0.2 Codex Desktop Automations(最完整的 Desktop 实现)

**两类自动化** ⭐ 关键设计:
| 类型 | 用途 | 上下文 | 结果去向 |
|---|---|---|---|
| **Standalone** | 每次独立运行 | 全新 prompt | Triage inbox |
| **Thread** | 心跳式回到同一对话 | 保留 thread context | 原 thread |

**Triage 队列** ⭐ Codex 杀手锏:
- 自动化运行 → 若有发现 → 进入 Triage 收件箱
- 若无发现 → **自动归档**(降低噪音)
- 过滤:全部 / 未读
- 每条 finding 展示:automation name / timestamp / status / output preview

**配置字段**:
```yaml
automation:
  name: "每日安全扫描"
  type: standalone | thread
  schedule: "0 9 * * 1-5"  # cron
  prompt: "..."
  model: gpt-5
  reasoning_effort: medium
  execution_mode: local | worktree | cloud
  sandbox: read-only | workspace-write | full-access
  approval_policy: never  # 默认 unattended
  projects: [repo-a, repo-b]  # 一个 automation 可跑多 repo
  skills: [$security-scan]  # 显式触发 skill
```

**执行模式三档**:
- **Local**:直接改主 checkout
- **Worktree**:独立 worktree,不污染主分支
- **Cloud**:24/7 云端执行(app 关闭也跑)

**已知 bug**(openai/codex#19969):cron 触发后创建空 session,prompt 未注入 — Shannon 实现时需注意这个反模式。

### 0.3 Claude Cowork(Claude Desktop 的 Schedule tab)

**简化版**(面向非开发者):
- **频率选项**:manual / hourly / daily / weekday / weekly
- **`/schedule` skill**:对话式创建
- **每次运行 = 独立 Cowork session**
- **App 关闭 → 跳过**,唤醒后自动补跑
- **Claude 自动改写 prompt**:首次运行后 Claude 学习并优化下次运行的 prompt
- **每任务可选 model** / **folder scope** / **connectors**(MCP)

**重要约束**:仅 Pro/Max/Team/Enterprise 付费计划可用。

### 0.4 Hermes Cron(开源,UI 模仿 Apple)

- **10+ 结果通知渠道**:Slack/Email/iMessage/Discord 等
- **自然语言定时**:「每天早上 9 点」→ cron
- **Apple 式精致 UI**

### 0.5 对 Shannon 的启示(决策依据)

| 模式 | 竞品来源 | Shannon 采纳? |
|---|---|---|
| **5-field cron 表达式** | Claude Code + Codex | ✅ **A1,必须**(原方案保留 `interval_secs` 向后兼容) |
| **SKILL.md + task.json 存储** | Claude Code | ✅ **F1,采纳**(比当前 `routines.json` 单文件更标准) |
| **Standalone vs Thread 两类** | Codex | ✅ **G1,采纳**(差异化) |
| **Triage inbox 队列** | Codex | ✅ **H1,P0**(Codex 杀手锏,Shannon 必须有) |
| **Worktree 执行模式** | Codex + Claude Code | ✅ **I1,P0**(Shannon 后端已有 `/batch` worktree,纯 UI) |
| **Cadence 预设 + 自定义 cron** | Cowork + Codex | ✅ 采纳(预设降低门槛,cron 满足高级用户) |
| **Jitter** | Claude Code | ✅ 采纳(Shannon 后端已有 10% jitter,需对齐到 cron) |
| **`/loop` CLI 命令** | Claude Code | ⚠️ **J1,P1**(Shannon CLI 加值,Desktop 可推后) |
| **Cloud 执行层** | Codex + Cowork + Claude Cloud | ❌ Phase 3+(Shannon 定位本地优先,cloud 非必需) |
| **自然语言定时** | Hermes + Cowork | ❌ Phase 3(增加不确定性,MVP 不做) |
| **多渠道通知(Slack/Email)** | Hermes | ⚠️ Phase 2(先做 webhook,渠道后置) |
| **App 关闭 → 唤醒补跑** | Cowork | ✅ P1(后端 RoutineManager 已支持持久化) |

---

## 1. 决策点(v2 更新,基于竞品调研)

| # | 决策 | v1 建议 | **v2 建议(基于调研)** | 理由 |
|---|---|---|---|---|
| **A** | Cron 表达式 | A1 加 | ✅ **A1 加(5-field,本地时区)** | Claude Code + Codex 都用 5-field cron |
| **B** | Webhook MVP | B2 推迟 | ✅ **B2 推迟** | Cowork/Codex 都不首推 webhook;webhook 后置 |
| **C** | 执行历史 | C1 进 MVP | ✅ **C1 进 MVP** | Triage 队列强依赖历史记录 |
| **D** | Tasks.tsx 拆分 | D2 三标签 | ✅ **D2 三标签 + Triage 作为顶级** | Codex 把 Triage 做成顶级 sidebar 入口 |
| **E** | NLP 定时 | E2 推迟 | ✅ **E2 推迟** | Hermes + Cowork 有,但 MVP 先做 cron picker |
| **F** | 存储格式 | (未提) | ✅ **F1 SKILL.md + task.json** | 对齐 Claude Code,且复用 Shannon skill 系统 |
| **G** | 两类自动化 | (未提) | ✅ **G1 Standalone + Thread** | Codex 差异化设计,Shannon CLI 有 thread 概念可复用 |
| **H** | Triage 队列优先级 | (未提) | ✅ **H1 P0(进 Sprint 3)** | Codex 杀手锏,无 Triage 则 scheduled 无价值 |
| **I** | Worktree 执行 | (未提) | ✅ **I1 P0(进 Sprint 2)** | Shannon 后端已支持 worktree,纯 UI 暴露 |
| **J** | `/loop` CLI 命令 | (未提) | ⚠️ **J1 P1(Sprint 4)** | Claude Code 杀手锏,Shannon CLI 加值 |

---

## 2. 数据模型(v2 重构,对齐 Claude Code)

### 2.1 存储格式变更(核心改动)

**v1(当前)**:`~/.shannon/routines.json`(单文件,JSON 数组)

**v2(对齐 Claude Code)**:
```
~/.shannon/scheduled-tasks/<task-kebab-name>/
  ├── SKILL.md          # Prompt + YAML frontmatter (复用 Shannon skill 渲染)
  └── task.json         # 调度 + 执行配置
```

**SKILL.md 示例**:
```markdown
---
name: daily-security-scan
description: 每日 09:00 扫描 OWASP top-10 安全风险
---

扫描当前仓库的 OWASP top-10 安全风险:
1. 检查 SQL injection / XSS / hardcoded secrets
2. 检查 unsafe Rust 操作
3. 生成 Markdown 报告到 reports/security-<date>.md
4. 若发现 critical 问题,标记为 Triage 紧急项
```

**task.json 示例**:
```json
{
  "id": "daily-security-scan",
  "name": "每日安全扫描",
  "description": "每日 09:00 扫描 OWASP top-10",
  "type": "standalone",
  "cron_expr": "0 9 * * 1-5",
  "timezone": "Asia/Shanghai",
  "enabled": true,
  "created_at": "2026-06-13T10:00:00Z",
  "last_fired_at": null,
  "next_fire_at": "2026-06-16T01:00:00Z",
  "fire_count": 0,
  "max_fires": null,
  "expires_at": null,
  "model": "claude-sonnet-4-6",
  "cwd": "/home/ed/workspace/shannon-code",
  "permission_mode": "acceptEdits",
  "approved_permissions": [
    {"tool": "Bash"},
    {"tool": "Read"},
    {"tool": "Write"}
  ],
  "execution_mode": "worktree",
  "worktree_config": {
    "branch_prefix": "scheduled/",
    "auto_merge": false,
    "auto_pr": false
  },
  "policy": {
    "max_retries": 2,
    "timeout_secs": 1800,
    "notify_on_failure": true,
    "budget_usd_monthly": 10.0,
    "auto_archive_when_empty": true
  },
  "thread_id": null,
  "projects": ["shannon-code"]
}
```

**`type` 字段**(两类自动化):
- `"standalone"`:每次新建 session,结果 → Triage
- `"thread"`:心跳式回到 `thread_id` 指定的 session

### 2.2 后端扩展(`shannon-core/src/scheduled_routines.rs`)— ✅ Sprint 1 已实施

**实际命名决策(2026-06-13)**:保留 `ScheduledRoutine` 名称(而非草案中的 `ScheduledTask`),
避免跨 crate 大规模 rename,改为通过 `trigger_type` 字段扩展能力。所有新字段都用
`#[serde(default)]` 保证老 `routines.json` 反序列化兼容。

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TriggerType {
    #[default]
    Interval,  // legacy: interval_secs
    Cron,      // v2: cron_expr (5-field)
    Webhook,   // 占位, Sprint 5
    Event,     // 占位, 对接 hook events
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledRoutine {
    // ── Identity ──
    pub id: String,                          // 8-char UUID prefix
    pub name: String,
    pub prompt: String,                      // 直接存 prompt(SKILL.md 中渲染)

    // ── Schedule ──
    #[serde(default)]
    pub interval_secs: u64,                  // legacy interval mode
    #[serde(default)]
    pub trigger_type: TriggerType,           // v2 新增
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron_expr: Option<String>,           // v2 新增, 5-field cron
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,            // v2 新增, IANA tz
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_fire_at: Option<DateTime<Utc>>, // v2 新增, 预计算
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,   // v2 新增

    // ── Lifecycle ──
    pub created_at: DateTime<Utc>,
    pub last_fired: Option<DateTime<Utc>>,
    pub enabled: bool,
    #[serde(default)]
    pub fire_count: u32,
    pub max_fires: Option<u32>,

    // ── Execution policy (v2) ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<ExecutionPolicy>,

    // ── Runtime state (v2) ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_id: Option<String>,         // 指向 scheduled-runs/ 中最新 run_id
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPolicy {
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default)]
    pub timeout_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,            // 独立 worktree 路径(None=主 checkout)
    #[serde(default)]
    pub notify_on_failure: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_usd: Option<f64>,             // 月度上限(注意字段名是 budget_usd, 不是 budget_usd_monthly)
    #[serde(default = "default_auto_archive")]
    pub auto_archive_when_empty: bool,       // 默认 true(Codex 模式)
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self {
            max_retries: 0,
            timeout_secs: 0,
            worktree: None,
            notify_on_failure: false,
            budget_usd: None,
            auto_archive_when_empty: true,  // 显式 impl, 不能用 derive(Default)
        }
    }
}
```

**待补充字段(Sprint 2+)**:`model` / `cwd` / `permission_mode` / `approved_permissions` /
`execution_mode` / `worktree_config` / `thread_id` / `projects` — 这些在 Sprint 1 demo 中
未实现,属于 UI 层对接时再加(避免提前设计无验证的字段)。

### 2.3 执行历史 — ✅ Sprint 1 已实施(JSONL 分区,非一文件一 run)

**实际布局决策(2026-06-13)**:采用 append-only JSONL,按 `YYYY/MM.jsonl` 分区,
而非原草案的「一 run 一文件」。理由:JSONL 在大量运行时磁盘友好,且 `revision` 字段
实现 last-write-wins 语义,更新 run 状态只需 append 新行。

```
~/.shannon/scheduled-runs/
├── 2026/
│   ├── 06.jsonl   # 2026-06 所有运行, 每行一个 JSON
│   └── 05.jsonl
└── 2025/
    └── 12.jsonl
```

**每行 JSON 结构**(`ScheduledRun`):
```json
{
  "run_id": "abc12345",
  "task_id": "daily-security-scan",
  "task_name": "每日安全扫描",
  "started_at": "2026-06-13T01:00:00Z",
  "finished_at": "2026-06-13T01:02:34Z",
  "status": "succeeded",  // running | succeeded | failed | cancelled | archived
  "error_message": null,
  "cost_usd": 0.023,
  "token_usage": 15420,
  "revision": 1
}
```

**关键 API**(`ScheduledRunsStore` in `scheduled_runs.rs`):
- `record(&run)` — append 一行
- `update(run_id, |r| ...)` — append 新行,`revision += 1`(last-write-wins)
- `find_by_id(run_id)` — 返回最新 revision
- `list_by_task(task_id, limit)` — 按 task 倒序
- `list_by_time_range(start, end)` — 按时间正序
- `prune_old()` — 删除 `rolling_days`(默认 90)之前的行,in-place 重写文件

**待补充字段(Sprint 3 Triage)**:`trigger_source` / `thread_id` / `worktree_branch` /
`output_summary` / `output_full_path` / `has_findings` / `triaged` / `triaged_at` /
`retry_count` — Sprint 1 仅覆盖核心状态追踪,这些字段在 Triage 子系统中按需追加
(serde default 保证向前兼容)。

**滚动**:90 天 `rolling_days`,in-place 重写文件保留窗口内最新 revision。
`archive_threshold_bytes` (10MB) 字段已留,归档动作未实施(占位 Sprint 2+)。

---

## 3. 后端改造清单(`shannon-core`)

| # | 状态 | 任务 | 文件 | 实际工作量 |
|---|---|---|---|---|
| B1 | ✅ Sprint 1 | 加 `croner = "3.0"` crate,5-field cron 解析 + `find_next_occurrence` 计算 `next_fire_at` | `scheduled_routines.rs` | 1 天(API 探索占大头) |
| B2 | ✅ Sprint 1 | `ScheduledRoutine` 原地扩展(加 9 个字段 + `TriggerType` enum,保留 `interval_secs` 兼容) | `scheduled_routines.rs` | 0.5 天 |
| B3 | ✅ Sprint 1 | 存储格式 `scheduled-tasks/<slug>-<id>/{SKILL.md, task.json}` | 新模块 `scheduled_task_store.rs` | 1 天 |
| B4 | ✅ Sprint 1 | 自动迁移:`routines.json` → 新格式(`.bak` 备份 + 幂等 skip) | `scheduled_task_store.rs` | 0.5 天 |
| B5 | ✅ Sprint 1 | `scheduled_runs.rs` JSONL 分区存储 + `revision` last-write-wins + 90 天滚动 | 新文件 `scheduled_runs.rs` | 1.5 天 |
| B6 | ⏳ **延后** | `RoutineManager::drain_due()` 集成历史记录(返回 run_id 并 record) | `scheduled_routines.rs` | 0.5 天(Sprint 2 开始时做) |
| B7 | ⬜ Sprint 4 | 失败重试(指数退避) + `auto_archive_when_empty` 逻辑 | 新文件 `scheduled_retry.rs` | 1 天 |
| B8 | ⬜ Sprint 4 | `ExecutionPolicy::budget_usd` 月度累计检查,超限 disable | 新文件 `scheduled_budget.rs` | 1 天 |
| B9 | ⬜ Sprint 2 | Worktree 执行模式:复用 `/batch` 的 worktree 创建逻辑 | 新文件 `scheduled_worktree.rs` | 1 天 |
| B10 | ⬜ Sprint 3 | Thread 模式:运行时 resume 指定 session(替代 standalone 新建) | `scheduled_routines.rs` | 0.5 天 |
| B11 | ✅ Sprint 1 | Jitter:`:00`/`:30` 触发最多提前 90s(u64 modulo 避免 i64 负数 bug) | `scheduled_routines.rs` | 0.5 天 |
| B12 | ✅ Sprint 1 | 单测覆盖:cron 解析 / 迁移 / 滚动(69 个 test passing) | 同上 | 1.5 天 |

**实际**:Sprint 1 ~7 天(含 API 探索和 i64 jitter bug 修复),比预估 8 天略短。
**剩余后端**:B6 (0.5d) + B7-B8 (2d) + B9 (1d) + B10 (0.5d) ≈ 4 天。
**总后端小计**:~11 天(与原估一致,但分布不同 — Sprint 1 略快,Sprint 2-4 略重)。

---

## 4. Desktop Tauri 命令清单(`crates/shannon-desktop/src/commands.rs`)

新增 15 个命令(v1 是 12 个,新增 3 个对应 Triage / Worktree):

### 4.1 Scheduled Tasks CRUD(7 个)

```rust
#[tauri::command]
pub async fn list_scheduled_tasks(state: tauri::State<'_, AppState>)
    -> Result<Vec<ScheduledTaskDto>, String>

#[tauri::command]
pub async fn create_scheduled_task(
    state: tauri::State<'_, AppState>,
    payload: CreateTaskPayload,  // { name, description?, type, cron_expr, model?, cwd?, permission_mode, execution_mode, policy?, prompt }
) -> Result<ScheduledTaskDto, String>

#[tauri::command]
pub async fn update_scheduled_task(
    state: tauri::State<'_, AppState>,
    id: String,
    patch: UpdateTaskPayload,
) -> Result<ScheduledTaskDto, String>

#[tauri::command]
pub async fn delete_scheduled_task(state: tauri::State<'_, AppState>, id: String)
    -> Result<(), String>

#[tauri::command]
pub async fn toggle_scheduled_task(state: tauri::State<'_, AppState>, id: String)
    -> Result<bool, String>

#[tauri::command]
pub async fn trigger_task_now(state: tauri::State<'_, AppState>, id: String)
    -> Result<String, String>  // 返回 execution_id

#[tauri::command]
pub async fn preview_cron(expr: String, timezone: String, count: u32)
    -> Result<CronPreview, String>  // { human_readable, next_runs: Vec<DateTime> }
```

### 4.2 Triage 队列(4 个,新增) ⭐ Codex 杀手锏

```rust
#[tauri::command]
pub async fn list_triage_items(
    state: tauri::State<'_, AppState>,
    filter: TriageFilter,  // { status: all|unread, task_id?: String, since?: DateTime }
) -> Result<Vec<TriageItem>, String>

#[tauri::command]
pub async fn mark_triage_read(
    state: tauri::State<'_, AppState>,
    run_ids: Vec<String>,
) -> Result<(), String>

#[tauri::command]
pub async fn archive_triage_item(
    state: tauri::State<'_, AppState>,
    run_id: String,
) -> Result<(), String>

#[tauri::command]
pub async fn get_triage_stats(state: tauri::State<'_, AppState>)
    -> Result<TriageStats, String>  // { total, unread, today, this_week }
```

### 4.3 执行历史 + Triggered Routines(4 个)

```rust
#[tauri::command]
pub async fn list_task_executions(
    state: tauri::State<'_, AppState>,
    task_id: Option<String>,
    status: Option<String>,  // success | failed | ...
    limit: u32,
    offset: u32,
) -> Result<Vec<TaskExecution>, String>

#[tauri::command]
pub async fn get_execution_detail(
    state: tauri::State<'_, AppState>,
    run_id: String,
) -> Result<TaskExecutionDetail, String>  // 含完整 output

#[tauri::command]
pub async fn list_triggered_routines() -> Result<Vec<TriggeredRoutineDto>, String>

#[tauri::command]
pub async fn toggle_triggered_routine(name: String, enabled: bool) -> Result<(), String>
```

**Tauri 命令工作量**: ~4 天(含 DTO + 错误处理 + 注册)

---

## 5. 前端改造清单(`crates/shannon-desktop/ui/`)

### 5.1 信息架构调整(对齐 Codex 的 sidebar)

```
Sidebar:
  ├── Chat            (现有)
  ├── Sessions        (现有)
  ├── Scheduled       (现有 → 改造为 Scheduled 父级)
  │     ├── Automations   (新建,Standalone + Thread)
  │     ├── Triage        ⭐ 新建顶级 tab(Codex 模式)
  │     ├── History       (新建)
  │     └── Triggered     (新建,hook routines)
  ├── OPC             (现有)
  └── Extensions      (现有)
```

**关键**:Triage 单独作为 sidebar 顶级 tab(类似 Codex 的 Triage pane),不需要点进 Scheduled。

### 5.2 组件拆分(Tasks.tsx 当前 585 行 → 拆为 12 个)

```
ui/src/pages/
  ├── Tasks.tsx                    → 改为容器,顶部四标签
  └── Triage.tsx                   ⭐ 新建,独立路由 /triage

ui/src/components/scheduled/
  ├── AutomationsTab.tsx           // Standalone + Thread 列表
  ├── TriageTab.tsx                // (备用,若用 Tasks.tsx 标签实现)
  ├── HistoryTab.tsx               // 执行历史
  ├── TriggeredTab.tsx             // hook-triggered routines
  ├── CreateAutomationDialog.tsx   // 新建表单(参照 Codex)
  ├── CronEditor.tsx               // cron 输入 + cronstrue 预览 + 下 3 次
  ├── CadencePicker.tsx            // 预设选择器(manual/hourly/daily/weekday/weekly/custom)
  ├── AutomationCard.tsx           // 单个 automation 卡片
  ├── TriageItem.tsx               // Triage 队列单条
  ├── TriageStats.tsx              // 顶部统计条
  ├── ExecutionDetail.tsx          // 单次执行详情抽屉
  ├── WorktreeBadge.tsx            // worktree/local 模式标识
  └── BudgetBar.tsx                // 月度 cost 进度条
```

### 5.3 新 hooks

```typescript
// ui/src/hooks/useScheduledTasks.ts
export function useScheduledTasks()           // list + CRUD + optimistic
export function useTriageItems(filter)        // list + mark_read + archive
export function useTaskExecutions(taskId?)
export function useCronPreview(expr, tz)      // debounce 300ms
export function useTriageStats()              // 顶部 unread 计数
```

### 5.4 关键 UI 元素(对标竞品)

| 元素 | Shannon 实现 | 对标 |
|---|---|---|
| **Cron 编辑器** | 输入框 + `cronstrue` 转「周一至周五 09:00」+ 下 3 次执行 | Claude Code + Codex |
| **Cadence 预设** | 五按钮:Manual / Hourly / Daily / Weekday / Weekly + Custom | Cowork |
| **类型切换** | Standalone vs Thread radio | Codex |
| **执行模式** | Local / Worktree radio(暂不暴露 Cloud) | Codex |
| **Triage 顶部统计** | `unread: 5  today: 12  this_week: 47` | Codex |
| **Auto-archive 开关** | 表单里 checkbox:`无发现时自动归档` | Codex |
| **Calendar 真渲染** | 按 `next_fire_at` 在日期格画点 + tooltip | Claude Desktop |
| **"Run Now"** | 调 `trigger_task_now(id)`,Triage 实时更新 | Codex |
| **Budget 进度条** | 卡片底部细条,接近上限变红 | Hermes |
| **AI Efficiency 卡片** | 改名「自动化率」= 自动成功次数 / 总次数 | — |
| **Agent Allocation** | 接 agent 实际负载(每 agent running task 数) | — |
| **失败重试标识** | 卡片上 🔁 icon + 「重试 2/3」 | Codex |

**前端工作量**: ~8 天(含 Triage 子系统)

---

## 6. CLI 改造清单(`shannon-cli`)— Sprint 4

参照 Claude Code 的 `/loop` + `CronCreate/CronList/CronDelete`:

### 6.1 新 REPL 命令

```
/loop 5m check if the deploy finished      # 短间隔循环
/loop 30m /review-pr 1234                  # 循环执行命令
/schedule                                   # 对话式创建持久任务(参照 Cowork)
/schedule list                              # 列出所有 scheduled tasks
/schedule cancel <task-id>                  # 取消
/triggered list                             # 列出 hook routines
/triggered enable <name>                    # 启用
```

### 6.2 环境变量

- `SHANNON_DISABLE_CRON=1` — 禁用所有 cron(对齐 Claude Code)
- `SHANNON_SCHEDULED_TASKS_DIR=~/.shannon/scheduled-tasks` — 自定义存储路径

**CLI 工作量**: ~3 天

---

## 7. 分阶段交付(v2,基于调研重新排序)

### Sprint 1: 后端 cron + 存储 + 历史 — ✅ 已完成(2026-06-13)
**锁定决策**:A1 / F1 / C1 — 全部落地
- ✅ B1 `croner = "3.0"` crate + 5-field cron 解析(`Cron::from_str` + `find_next_occurrence`)
- ✅ B2 `ScheduledRoutine` 原地扩展(9 字段 + `TriggerType` enum,保留 `interval_secs` 兼容)
- ✅ B3-B4 `scheduled_task_store.rs`(SKILL.md + task.json + 自动迁移 + `.bak` 备份)
- ✅ B5 `scheduled_runs.rs`(JSONL 分区 + last-write-wins + 90 天滚动)
- ⏳ **B6 延后**:`drain_due()` 集成历史记录 → Sprint 2 开始时做(需要先定 run lifecycle)
- ✅ B11 Jitter 对齐 Claude Code(`:00`/`:30` 提前 90s,u64 modulo)
- ✅ B12 单测覆盖:**69 个 test 通过**(原 target ≥15 case 大幅超额)

**实际偏差(相对草案)**:
1. 命名:保留 `ScheduledRoutine`(非 `ScheduledTask`),避免跨 crate rename
2. 历史布局:JSONL 按 `YYYY/MM.jsonl` 分区(非一文件一 run)
3. `ExecutionPolicy` 字段名:`budget_usd`(非 `budget_usd_monthly`)
4. B6 推迟:demo 阶段 focus 在存储/调度正确性,run 写入由调用方明确触发更安全

**实施学到的(用于后续 sprint)**:
- `croner` 3.x API:`Cron::from_str`(via `std::str::FromStr`)+ `find_next_occurrence(&from, false)`,不是 `Cron::new()` / `next_from()`
- `#[derive(Default)]` 会覆盖 `#[serde(default = "fn")]` 意图 — 显式 `impl Default` 才能让 `auto_archive_when_empty` 默认 true
- `rand::random::<i64>() % N` 会产生负数 — 改用 `u64` 后再 `as i64`
- croner 3.x 同时接受 5-field 和 6-field(含秒)— 测试用例只能针对语法错误,不能针对字段数

**验收(全部通过)**:
- ✅ `cargo test -p shannon-core scheduled` → 69 passed, 0 failed
- ✅ cron 解析单测(原 target ≥15 case):实际覆盖 `0 9 * * 1-5` / `*/15 * * * *` / 无效表达式 / `new_cron` 验证 / `compute_next_fire_utc` 等 23 case
- ✅ 老数据迁移单测:`migrate_from_routines_json` 覆盖 空 / 老格式 / 幂等 re-run
- ✅ 历史滚动单测:`prune_old` 删除过期 + 保留最新 revision
- ✅ `cargo check -p shannon-core` clean
- ✅ `cargo clippy -p shannon-core` 无新 warning

### Sprint 2: Tauri 命令 + UI 骨架 + Worktree 模式(1.5 周)
**锁定决策**:I1
- 15 个 Tauri 命令(§4)
- 前端 hooks + Tasks.tsx 拆四标签
- **AutomationsTab**:list + CRUD + Cadence 预设 + Cron 编辑器
- **Worktree 执行模式**(B9)
- Calendar 真实渲染(next_fire_at)

**验收**:手动创建 cron task「每分钟」,1 分钟后能在历史看到记录;worktree 模式下变更不污染主分支

### Sprint 3: Triage 队列 + Thread 模式(1 周)⭐ Codex 杀手锏
**锁定决策**:G1 / H1
- **TriageTab / Triage.tsx**:独立顶级路由
- B10 Thread 模式后端
- 类型切换 UI(Standalone vs Thread)
- Triage 顶部统计 + 过滤 + 标记已读 + 归档
- `auto_archive_when_empty` 逻辑

**验收**:跑 3 个 automation,2 个有发现进 Triage、1 个无发现自动归档;Thread 模式保持上下文

### Sprint 4: CLI `/loop` + Triggered + Budget(1 周)
**锁定决策**:J1
- `/loop` / `/schedule` CLI 命令
- B7-B8 失败重试 + Budget 月度上限
- TriggeredTab(32 hook events 列表 + toggle)
- Budget 进度条 UI

**验收**:`/loop 1m echo hi` 能跑;Budget 达上限 task 自动 disable

### Sprint 5(可选,Phase 2): Webhook + 通知渠道(1.5 周)
- Webhook → ScheduledTask 绑定层(后端已有 `WebhookRegistry`)
- Webhook URL 生成 + HMAC 密钥配置 UI + curl 示例
- Slack / Email 结果通知 adapter(参照 Hermes)

**验收**:curl POST webhook 能触发 task,验签失败返回 401;Slack channel 收到执行结果

### Sprint 6(可选,Phase 3): NLP + 模板 + Cloud(2 周)
- 决策点 E1:NLP 解析「每天 9 点」→ cron(用 Shannon 自己的 LLM)
- Schedule 模板库(每日 standup / 周报 / 依赖扫描 / PR review)
- Cloud 执行层(自建或对接外部)

---

## 8. 风险与依赖

| 风险 | 影响 | 缓解 |
|---|---|---|
| `croner` crate 选型 | 5-field vs 6-field(秒级) | 推荐 `croner`(支持 L/W/#,无 unsafe) |
| 老数据迁移失败 | 用户老 routines 丢失 | 迁移前自动备份到 `routines.json.bak`;迁移失败回滚 |
| TOML 改写破坏注释 | toggle_triggered 丢注释 | 用 `toml_edit` 替代 `toml` |
| 历史文件膨胀 | JSONL 无限增长 | 90 天滚动 + 10MB 单文件归档 |
| Schedule 阻塞主循环 | drain_due 同步执行 | 复用 `start_background_task` 异步通道 |
| Cron 时区 | UTC vs 本地 | task.json 存 IANA tz,UI 渲染转本地 |
| Codex bug 复现 | openai/codex#19969: cron 触发空 session | 集成测试:验证 prompt 真的注入到 execution |
| Worktree 泄漏 | 高频 schedule 创建大量 worktree | `auto_archive_when_empty` + 7 天未访问清理 |
| Thread 模式 session 锁 | 同一 session 被并发触发 | task.json 加 `currently_running_run_id` 锁 |
| Budget 检查时机 | 超限后才检查 | 触发前预检 + 每月底硬截止 |

---

## 9. 验收标准

### Sprint 1 — ✅ 全部通过(2026-06-13)
- [x] `cargo test -p shannon-core scheduled` 全绿 → **69 passed**
- [x] cron 解析单测 ≥ 15 case → **23 case**(含 `0 9 * * 1-5` / `*/15 * * * *` / 时区 / jitter / 过期 / 无效表达式)
- [x] 老数据迁移单测覆盖(空 / 老格式 / 幂等 re-run / `.bak` 备份验证)
- [x] 历史滚动单测(`prune_old` 删除过期 + 保留最新 revision)

### Sprint 2
- [ ] 手动创建 cron task「`*/1 * * * *` echo」,1 分钟后能在历史看到记录
- [ ] Worktree 模式下 task 在 `scheduled/<id>-<ts>` 分支跑,主分支 clean
- [ ] Calendar 显示未来 7 天的 schedule 点

### Sprint 3
- [ ] 跑 3 个 automation:2 个有发现 → 进 Triage;1 个无发现 → 自动归档
- [ ] Triage 顶部统计正确(unread / today / this_week)
- [ ] Thread 模式 task 触发时 resume 原 session,context 保留

### Sprint 4
- [ ] `/loop 1m echo hi` 在 REPL 内能跑
- [ ] Budget 月度上限触发后 task 自动 disable,UI 标红
- [ ] Triggered routines 列表显示 32 hook events

### Sprint 5
- [ ] curl POST webhook URL 能触发 task,验签失败返回 401
- [ ] Slack adapter 发送执行结果到指定 channel

### Sprint 6
- [ ] 输入「每天早上 9 点扫依赖」→ 自动生成 `0 9 * * *` + 预览
- [ ] Cloud 执行层 task 在 app 关闭时仍按 schedule 跑

---

## 10. 审核要点(2026-06-13 已审核,决策锁定)

### 已锁定决策(用户 2026-06-13 审核)

- ✅ **决策点 A-J**(§1)全部按 v2 建议通过
- ✅ **存储格式**: SKILL.md + task.json(§2.1)
- ✅ **Triage 作为 sidebar 顶级 tab**(不是 Scheduled 子标签)— 对齐 Codex 杀手锏设计
- ✅ **保留「Scheduled」命名**(不改名为「Automations」)— Shannon 已有品牌识别
- ✅ **Sprint 1-4 节奏(共 5 周)** 通过
- ✅ **15 个 Tauri 命令(§4)** 通过
- ✅ **Thread 模式 Sprint 3** 合理
- ✅ **CLI `/loop` Sprint 4** 不前移
- ✅ **Cloud 执行层 Sprint 6 可选** — Shannon 定位本地优先,可砍
- ✅ **「AI Efficiency」→「自动化率」** 改名通过

### Sprint 1 后新增决策(基于实战,2026-06-13)

- ✅ **保留 `ScheduledRoutine` 名称**(不 rename 为 `ScheduledTask`):避免跨 crate rename,字段扩展已足够
- ✅ **JSONL 按 `YYYY/MM.jsonl` 分区**(非一文件一 run):磁盘友好 + revision last-write-wins 简化更新
- ✅ **`ExecutionPolicy.budget_usd` 字段名**(非 `budget_usd_monthly`):简洁,语义已明确
- ✅ **B6 (`drain_due` 集成) 推迟到 Sprint 2 开始**:demo 阶段先验证存储/调度,run lifecycle 需先定
- ✅ **`ExecutionPolicy` 显式 `impl Default`**(`auto_archive_when_empty: true`):derive(Default) 会被 bool::default() 覆盖,必须显式实现

### 执行顺序(用户指定)

1. ✅ **Sprint 1 后端 demo 已完成**(cron + SKILL.md + 历史,实际 ~7 天)
2. ✅ **基于实战经验更新实施方案**(本文件 v3 更新,2026-06-13)
3. ⏳ ADR 文档:待 Sprint 2 开始前补(`routines.json` → `scheduled-tasks/` 变更 + JSONL 分区决策)

---

## 11. Sprint 1 实施回顾与方案校准(2026-06-13)

### 11.1 实际交付

| 模块 | 文件 | 行数 | 测试数 |
|---|---|---|---|
| `scheduled_routines.rs` 扩展 | 原文件 +344 行 | ~660 总行 | 41(18 原有 + 23 新增) |
| `scheduled_task_store.rs` 新增 | 新文件 | ~370 行 | 12 |
| `scheduled_runs.rs` 新增 | 新文件 | ~530 行 | 13 |
| `Cargo.toml` 依赖 | 加 `croner = "3.0"` | — | — |
| `lib.rs` 导出 | 新增两个 `pub mod` | — | — |
| **总计** | 3 个模块 | ~1560 行 | **66 + 3 集成 = 69 测试** |

**Sprint 1 测试输出**:
```
cargo test -p shannon-core scheduled → 69 passed, 0 failed
cargo check -p shannon-core          → clean
cargo clippy -p shannon-core         → no new warnings
```

### 11.2 关键 API 决策(供 Sprint 2+ 复用)

#### croner 3.x API(关键!)

```rust
use std::str::FromStr as _;
use croner::Cron;

// ✅ 正确(v3 API)
let cron = Cron::from_str("0 9 * * 1-5")?;
let next: DateTime<Local> = cron.find_next_occurrence(&from_local, false)?;

// ❌ 错误(不存在)
let cron = Cron::new("0 9 * * 1-5");  // 编译错误
let next = cron.next_from(&from);     // 编译错误
```

`false` 参数 = 不包含当前时刻(避免立即触发)。

#### 显式 Default impl 模式

当 struct 同时有 `#[derive(Default)]` 和 `#[serde(default = "fn")]` 时,**derive 赢** — `#[serde(default = "fn")]` 只在反序列化缺字段时调用,不影响 `Default::default()`。

```rust
// ❌ 错误:derive(Default) 会让 auto_archive_when_empty 默认 false
#[derive(Default)]
struct ExecutionPolicy {
    #[serde(default = "default_true")]
    auto_archive_when_empty: bool,
}

// ✅ 正确:移除 derive(Default),显式 impl
struct ExecutionPolicy {
    #[serde(default = "default_true")]
    auto_archive_when_empty: bool,
}
impl Default for ExecutionPolicy {
    fn default() -> Self { Self { auto_archive_when_empty: true, ... } }
}
```

#### Jitter 必须用 u64 modulo

```rust
// ❌ 错误:负数 dividend 产生负余数
let jitter = rand::random::<i64>() % 91;  // 可能是 -45

// ✅ 正确:u64 modulo 后 cast
let n: u64 = rand::random();
let jitter = (n % (CRON_ROUND_TIME_JITTER_SECS as u64 + 1)) as i64;
```

### 11.3 草案 vs 实际偏差

| 草案(§2) | 实际实施 | 理由 |
|---|---|---|
| `ScheduledTask` 新名称 | 保留 `ScheduledRoutine` | 跨 crate rename 成本高,字段扩展已足够 |
| `task_type: Standalone/Thread` | `trigger_type: Interval/Cron/Webhook/Event` | trigger_type 表达「怎么触发」,Standalone/Thread 改由 execution_mode 表达(Sprint 2+ 加) |
| `budget_usd_monthly` | `budget_usd` | 字段更简洁,语义已明确 |
| `approved_permissions: Vec<ApprovedPermission>` | (未加) | Sprint 2 UI 层对接时按需加,避免提前设计 |
| `execution_mode: Local/Worktree/Cloud` | (未加) | 同上,Sprint 2 加 |
| 一文件一 run: `<run-id>.json` | JSONL 按 `YYYY/MM.jsonl` 分区 + revision | 大量运行时磁盘友好,last-write-wins 简化更新 |
| `trigger_source` / `has_findings` / `triaged` | (未加) | Sprint 3 Triage 子系统按需加,serde default 兼容 |
| B6 `drain_due` 集成 | **推迟到 Sprint 2** | demo 阶段先验证存储/调度,run lifecycle 需先定 |
| cron 解析 ≥15 case | **23 case** | 测试比预估更密(覆盖边界条件) |
| Sprint 1 工作量预估 8 天 | **实际 ~7 天** | croner API 探索占 1 天,i64 jitter bug 修复占 0.5 天 |

### 11.4 Sprint 2-4 估算校准

基于实际 API 表面和 JSONL 分区决策,重新估算后续 sprint:

| Sprint | 范围 | 草案估算 | 校准估算 | 说明 |
|---|---|---|---|---|
| **Sprint 2** | Tauri 命令 + UI 骨架 + Worktree 模式 + **B6 drain_due 集成** | 1.5 周 | **1.5 周** | B6 仅 0.5 天,加入 Sprint 2 起始 |
| **Sprint 3** | Triage 队列 + Thread 模式 | 1 周 | **1 周** | JSONL 已就位,Triage 仅需加 `has_findings` / `triaged` 字段 |
| **Sprint 4** | CLI `/loop` + Triggered + Budget + 重试 | 1 周 | **1 周** | Budget 检查复用 `budget_usd` 字段 |

**总工期不变**:5 周(Sprint 1-4)。

### 11.5 Sprint 2 起始清单(B6 + Tauri)

Sprint 2 开始时先做 B6,然后拉 Tauri 命令层:

1. **B6 `drain_due` 集成**(0.5 天):
   - `RoutineManager::drain_due()` 改返回 `Vec<(task_id, run_id)>` 而非 `Vec<task_id>`
   - 每次 fire 先 `ScheduledRunsStore::start_run()` 拿 run_id,fire 后 `update(run_id, |r| r.finish(...))`
   - `ScheduledRoutine.last_run_id` 字段同步更新

2. **15 个 Tauri 命令**(4 天,见 §4):
   - DTO 层:用 `ScheduledRoutine` 字段名,不用 `ScheduledTask`(重要!)
   - `budget_usd` 字段名,不用 `budget_usd_monthly`
   - 历史命令对接 `ScheduledRunsStore`,不重新设计 layout

3. **UI 骨架**(3 天):
   - Tasks.tsx 拆分(参照 §5.2)
   - Triage 独立路由(暂占位,Sprint 3 实现)

### 11.6 风险新增(基于实战)

| 风险 | 影响 | 缓解 |
|---|---|---|
| croner 3.x 接受 6-field(含秒)| 用户误填秒级 cron 导致每秒触发 | UI 层 cron 编辑器强校验 5-field(空格 split count == 5) |
| `#[serde(default)]` 字段在 `derive(Default)` 时被忽略 | `auto_archive_when_empty` 等字段默认值偏离预期 | 显式 `impl Default`(已在 `ExecutionPolicy` 实施,Sprint 2+ 新 struct 同模式) |
| `i64` modulo 负数 bug 复发 | jitter 产生负数 → 立即触发 → thundering herd | 全部 jitter/random% 操作统一用 `u64`,PR review 检查 |
| JSONL 分区文件膨胀检测 | 单月 >10MB 时未触发归档动作 | `archive_threshold_bytes` 已留接口,Sprint 5 补归档逻辑 |

---

**文档版本**:v3(2026-06-13,Sprint 1 后更新)
**下一里程碑**:Sprint 2 起始 — B6 集成 + Tauri 命令层 + UI 骨架
