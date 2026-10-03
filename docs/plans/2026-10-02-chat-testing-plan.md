# AI 对话页测试体系建设方案（供审核）

日期：2026-10-02 · 基线 dev @ 828c4adb · 作者：调研会话（3×Explore 深挖 + 主会话实证）
范围：`/chat` 页及其 user journeys 的自动化测试（UI/页面/状态响应 + 用户使用历程），目标=验证功能正确 + 主动挖掘未发现问题。

---

## 0. TL;DR

**现状**：对话页单测覆盖不差（Vitest 242 文件、lines 83%），但**端到端模拟"AI 完成任务"的能力为零**——demo mock 的 `send_message` 秒回 `completed`，不产生任何 `query:*` 流事件（`src/lib/runtime/tauriBridge.ts:140-149`），连事件监听通道（`plugin:event|listen`）在 mock 层都没有实现。因此流式渲染、工具卡片、审批弹窗、预算横幅、错误恢复这些对话页最复杂的状态机，**只能在无头 Vitest 里测，浏览器 E2E 完全够不着**。

**方案核心**：建一个「**对话剧本（ChatScript）**」内核——用 YAML 声明"用户发什么 + AI 产生哪些事件（含时序/分片/故障）+ 断言"，借鉴仓库已有的 `tests/scenarios/*.yaml` 三段式格式；配一个 **ScriptedBackend**（扩展 demo mock 层：补上事件桥 + 可编程剧本播放器 + 故障注入）。一份剧本**两层消费**：

- **L1 Vitest 状态机层**：剧本 JSON → captured-listen 逐事件 flush，断言 AppContext 状态迁移与组件渲染（确定性最高、跑得最快）；
- **L2 浏览器 E2E 层**：Playwright + 增强版 demo 模式，真实点击输入，按剧本驱动流式/工具/审批/故障，断言 DOM、aria、视觉快照、console 零错误；
- **L3 真后端集成层**（二期）：fake SSE provider（`openai-compatible` + `base_url`→本地假 LLM）+ `SHANNON_HOME` 隔离，验证"前端→Tauri 命令→引擎→事件回流"全链路，防「装了→能用」类断裂在聊天域重演。

在此之上铺 **14 条 user journey 剧本**（§4 矩阵）、**4 个找未知问题的机制**（不变量探针 / 事件 fuzz / 竞态剧本 / 回归锚点），并挂进 CI 分层门禁。本次调研同时产出 **16 条新可疑点**（附录 A）作为第一轮测试的"捕虫清单"。

> **10-02 增补**：应用户要求补做两个专项调研并合并入本方案——**取消（§4.1）**与**输入缓存（§4.2）**。结论：取消"支持但有洞"（引擎不感知 token、事件边界才生效、前端不按 query_id 过滤 → stop 后立刻重发有状态污染竞态）；输入缓存"草稿/队列/外部桥齐备但策略不一致"（队列不持久化、无历史回溯）。可疑点扩至 **22 条**（A-17…A-22），拍板项扩至 7 个（新增 D6/D7）。

> **10-03 v2 深度复核与增补拍板（独立会话）**：对 §1–§8 做独立复核（3×Explore+主会话实证，同基线 828c4adb）——架构前提全部成立；修正 A-16 反证/A-14 改判/A-10 后半降级；新发现 A-23…A-26 四条（复核时 A-23/A-24 尚未修，后经主路径 R 系列修复覆盖，见 §9.3 与执行结果）；盲区扫描 G1–G22 → 剧本矩阵 14→22；新增拍板 D8/D9。v2 增量实施=本 wave（R7），范围见 §9.6。

**工作量**（最终口径以 §9.6 为准）：MVP ≈ 1.5 周；全量（22 journeys + fuzz + 视觉状态矩阵 + 回归锚点核销）≈ 3.5–4 周。决策点 D1–D9 已全部拍板（§8）。

---

## 1. 现状诊断

### 1.1 已有资产（可复用，不重复造）

| 资产 | 位置 | 对本方案的价值 |
|---|---|---|
| 统一 invoke 封装 | `desktop/ui/src/lib/tauri-api.ts`（~280 命令） | mock 的唯一注入面，组件从不直接 invoke |
| 捕获式 fake listen 范式 | `src/__tests__/AppContextStreaming.test.tsx:20-37` | L1 层驱动状态机的主控缝，直接复用 |
| 事件名常量表 | 前端 `src/types/index.ts:1529-1579`；后端 `crates/shannon-types/src/events.rs:375-447` | 剧本 DSL 的事件名合法性校验来源 |
| demo mock 层 | `src/lib/mock/`（handlers 2096 行 + tripwire 测试） | L2 层的宿主；tripwire 保证新命令必有 handler |
| Playwright 三配置 | `playwright*.config.ts` + e2e/ 18 specs | L2 骨架已在 CI（desktop-e2e / desktop-visual-audit） |
| 视觉截图管线 | `scripts/shoot-theme-gallery.mjs` + walkthrough spec | 视觉回归扩展的现成底座 |
| 声明式场景格式 | `tests/scenarios/*.yaml`（setup/mock_responses/validate 三段式） | **ChatScript DSL 的直接蓝本** |
| 录制回放假 LLM | `SHANNON_RECORD_DIR`/`SHANNON_REPLAY_DIR`（`shannon-engine/src/api/client.rs:453,458`）+ `tests/fixtures/real_tasks/*.jsonl` | L3 层素材；真实 SSE 序列可"翻译"成剧本 |
| SSE 代表性事件 fixture | `crates/shannon-core/src/query_engine/sse.rs: representative_events()` | 剧本边界样例来源 |
| 性能/a11y 守卫先例 | `glass-budget.spec.ts`（backdrop-filter 预算）、walkthrough 全规则 axe | 横切守卫的模式 |

### 1.2 四大空白（本方案要填的坑）

1. **demo 模式没有对话**：`send_message` handler（`lib/mock/handlers.ts:415`）只回 `{query_id}`；`tauriBridge` mock 路径直接 queueMicrotask 秒回 completed；`plugin:event|listen` 在 coreMock 无 handler——README 里"console 手动 emit"的写法在当前实现下根本不可用（`emit` 会走 `plugin:event|emit` 同样未处理）。
2. **E2E 无 per-test 数据注入**：mock seed 全局单例，无法构造"已有失败回合的会话""流式中途""等待审批"等场景；`plugins.marketplace.spec.ts` 文件头明写需要 override helper。
3. **chat 页主体组件零测试**：`pages/chat/` 下 MessageArea、ComposerPanel、ContextPanel、PlanPanel、QueueChips、InlinePanelModal、DeleteSessionModal、ApiKeyBanner 均无 Vitest 覆盖；jsdom 滚动尺寸为 0 导致虚拟化/滚动 FAB 分支单测够不着。
4. **中间态与故障态没有任何自动化**：视觉基线仅 3 页 × light × 空闲态；流式中工具卡、进度 pill、审批弹窗、预算横幅、错误分支无快照无 E2E；乱序/重复/跨会话事件等后端真实会发生的脏输入从未在 UI 层被系统性注入过。

### 1.3 一个关键实证（决定方案走向）

demo 模式的断裂点不在 handlers 内容，而在**事件通道本身**：`@tauri-apps/api/event` 的 `listen()` 内部走 `invoke('plugin:event|listen')`，coreMock 只替换了 core 模块、没有这条命令的 handler，`transformCallback` 也只返回随机数不注册回调。因此 ScriptedBackend 必须先补这个桥（见 §2.2），这是一切浏览器端流式测试的前置。

---

## 2. 总体设计：一份剧本，三层消费

```
                        ┌────────────────────────────┐
                        │  ChatScript YAML 剧本库     │
                        │  desktop/ui/e2e/scripts/    │
                        │  (setup + turns + assert)   │
                        └─────────┬──────────────────┘
                 loader(zod/ajv 校验) 归一为 ScriptJSON
              ┌───────────────────┼────────────────────┐
              ▼                   ▼                    ▼
   ┌──── L1 Vitest ────┐  ┌──── L2 Playwright ──┐  ┌─ L3 真后端(二期) ─┐
   │ captured-listen    │  │ demo 模式 +          │  │ fake SSE provider │
   │ flush(script 事件) │  │ ScriptedBackend      │  │ + SHANNON_HOME    │
   │ 断言状态机/渲染     │  │ 断言 DOM/截图/console│  │ 断言全链路行为     │
   └────────────────────┘  └──────────────────────┘  └───────────────────┘
```

### 2.1 ChatScript 剧本 DSL（YAML）

蓝本 = `tests/scenarios/read_file.yaml` 的三段式（setup / mock_responses / validate），把"LLM 回合"换成"Tauri 事件序列"，把"exit_code/response_contains"换成"DOM 断言"。事件名/字段以两端常量表为准（§1.1），loader 做 schema 校验（ajv 已在依赖）。

```yaml
name: tool-task-approval-allow
description: "bash 任务触发审批，Allow once 后完成，产物进 dock"
seed:
  config: { provider: anthropic, hasKey: true, budgetUsd: 5 }
  sessions:
    - id: sess-a
      title: 新会话
      messages: []                    # 也可预置历史/失败回合
turns:
  - user: "帮我在工作目录创建 todo.md"
    script:
      - event: query:text
        chunks: ["好的，我", "先创建文件。"]   # 自动分片，chunkDelayMs 可选
      - event: query:thinking
        chunks: ["需要 Bash 权限…"]
      - event: permission-request        # 播放器在此暂停，等测试操作 UI
        payload: { tool: Bash, risk: high, request_id: pr-1,
                   input: { command: "echo '- 买牛奶' > todo.md" } }
      - waitFor: ui                      # 显式停点
      - event: query:tool-start
        payload: { tool_use_id: t1, tool_name: Bash, tool_input: { command: "…" } }
      - event: query:tool-progress
        payload: { tool_use_id: t1, progress: 0.5, message: "writing…" }
      - event: query:tool-result
        payload: { tool_use_id: t1, result: "ok", is_error: false }
      - event: query:usage
        payload: { input_tokens: 1200, output_tokens: 300, cost_usd: 0.012 }
      - event: query:text
        chunks: ["已完成，写在 todo.md"]
      - event: query:completed
assert:
  - after: query:completed
    visible: ["[data-testid=run-status-line]"]
    text: ["/todo.md|已完成/"]
  - consoleErrors: 0                     # watchdog，见 §5.1
```

DSL 要点：

- **事件形状与后端完全一致**（含 `query_id`/`session_id` 字段、`budget:*`/`PermissionReason` 的 camelCase 特例），剧本本身就是后端事件契约的回归样例；
- **`waitFor: ui` 停点**用于审批这类"后端等用户"的真实阻塞，其余事件按时序自动播放；
- **分片（chunks）显式建模**：流式不是一整块文本，chunk 边界/多字节截断/中英文混排都要可表达；
- 剧本可标注 `variants`（见 §5.2 fuzz：同一剧本自动生成乱序/重复/双 completed 等变体）。

### 2.2 ScriptedBackend（demo mock 层扩展）

四个改动点，全部收敛在 `src/lib/mock/`，不碰业务代码：

1. **事件桥**：coreMock/handlers 增加 `plugin:event|listen`、`plugin:event|unlisten` handler，并把 `transformCallback` 改为真正注册 `window['_'+id]` 回调（现状返回随机数）；新增 `window.__shannonMock.emit(eventName, payload)` 把事件分发给已注册监听者。这一步同时修复 demo 模式下 AppContext 全部 `listen()` 注册失败的现实问题。
2. **剧本播放器**：`window.__shannonMock.loadScript(scriptJson)` 装载剧本并接管 `send_message` handler——每次发送按当前 turn 的 script 逐事件发射（尊重 chunks/chunkDelayMs/停点）；暴露 `__shannonMock.control = { pauseAt, resume, emitNow, speed }` 给测试逐步驱动。
3. **seed 注入**：`loadScript` 同时把 `seed`（config/会话/消息/预算）写进 mock 内存 store，取代全局单例 seed，解决空白 #2。Playwright 侧封装 `loadScript(name)` fixture + page objects（`e2e/helpers/`）。
4. **console watchdog**：Playwright fixture 收集 `pageerror`/`console.error`/unhandled rejection，每条断言默认含 `consoleErrors: 0`（可豁免白名单）——低成本抓"静默失败"反模式（J3 走查认定的最高频根因）。

> 备选被否方案：直接在 `tauriBridge.ts` 加 `ScriptedShannonTauriBridge`（更薄，但只覆盖 runtime 桥消费者、绕过 AppContext 的裸 listen 注册，且 `lib/runtime/` 处于休眠状态）；后端 `SHANNON_REPLAY_DIR` 回放（请求体哈希逐字节匹配，提示词一漂移就 miss，不可编程）。选 demo mock 层方案：覆盖面（AppContext 裸 listen + 全部命令）与可控性兼得，且顺带修好 demo 模式本身。

### 2.3 一份剧本两层跑

同一 ScriptJSON：
- L1：`runScriptInVitest(script)` → captured-listen 逐事件 flush → 断言 `streamingText`/`activeToolCalls`/`permissionRequest`/`error` 等状态投影与关键组件渲染（复用 `AppContextStreaming.test.tsx` 范式）；
- L2：`loadScript(name)` → 真实浏览器交互 → DOM/aria/截图/console 断言。

价值：状态机回归修一次剧本即可双层生效；L1 定位快、L2 证明用户真看得到。**两层共享同一份断言语义**（L2 的 DOM 断言是 L1 状态断言的投影），避免漂移。

---

## 3. 分层测试方案

### L1 组件/状态机层（Vitest，最确定性）

1. **状态机剧本测试**（新增 `__tests__/chatStateMachine.scripts.test.tsx`）：把 §4 矩阵每条 journey 的 ScriptJSON 跑一遍，断言 AppContext 投影（含 per-session bucket 不串流、错误分类、queue drain、steer 停车等已有 Imp-1/2 竞态回归扩展）。
2. **零覆盖组件补测**：MessageArea（错误横幅 auth/other 分支、RunStatusLine、StreamNoticeLine、session-switch-overlay）、QueueChips、ComposerPanel（edit banner / slash 卡）、ContextPanel、PlanPanel、ApiKeyBanner、DeleteSessionModal。滚动/虚拟化分支在 jsdom 测不到的部分明确标注留给 L2（不硬造）。
3. **纯函数表驱动**：`lib/runProcess.ts` reducer、`parseSlashInput`、`diffStats`、`windowSession`、`fileMention`——现状多半已有，按剧本新增边界用例即可。

### L2 浏览器 E2E 层（Playwright + ScriptedBackend，主战场）

- 每条 user journey 一个 spec（§4），统一用 `loadScript` fixture + page objects，废除 `waitForTimeout` 手写等待（等待一律锚定 `data-testid`/aria/文本，现成 testid 已够多：`run-status-line`、`prompt-queue`、`file-changes-card`、`subagent-block`、`edit-banner`…）；
- **中间态断言**（现状完全缺失）：流式光标与 stop 按钮、工具卡 running→done 形态、进度 pill 百分比、审批 alertdialog 三按钮、预算 warning→exceeded 横幅、错误横幅 auth 深链、`StreamStatusRegion` aria-live 播报；
- **视觉快照扩展**：在现有 `visual-baseline.spec.ts` 基础上加"对话页状态矩阵"（见 §6 横切）。

### L3 真后端集成层（二期，防"最后一公里"断裂）

- **fake SSE provider**：provider store 写入 `openai-compatible` 条目指向本地假 LLM（最小 axum/wiremock，吐 `tests/fixtures/real_tasks` 同构的 SSE chunk 流，剧本可复用其事件序），`SHANNON_HOME` 指向临时目录隔离；跑通 `pnpm tauri dev` 或直接驱动 `shannon-server` 二进制 + `POST /v1/sessions/:id/messages`（SSE）做契约层冒烟。
- 覆盖点：`send_message` → 权限通道 → 事件 emit → 前端投影 → `respond_permission` 回执 → `query:completed` 落库重载。这正是 CLAUDE.md「install-to-usable acceptance」五步链在聊天域的自动化形态。
- tauri-driver/WebDriver（真壳多窗口）成本高，本期不做，列为远期（§8 D2）。

### 横切守卫（复用已有模式扩展）

| 守卫 | 现状 | 动作 |
|---|---|---|
| 视觉回归 | 仅 3 页 light 空闲态 | 加"对话页状态矩阵"快照：welcome/流式中/工具运行中/审批弹窗/错误横幅 × light+dark；沿用 walkthrough 管线 |
| a11y | walkthrough 全规则 axe（14 路由静态） | 剧本执行中动态扫（审批弹窗开着、流式 aria-live 播报时） |
| 性能 | glass-budget（backdrop-filter ≤4） | 加：长会话（50+ 消息）流式帧budget、`STREAM_FLUSH_MS` 节流下重排预算 |
| i18n/token | `check:i18n`/`check:tokens` lint 已有 | 剧本内文案断言统一走 i18n key，防硬编码回归 |

---

## 4. User Story → 测试映射矩阵（v1 剧本清单）

走查报告的四条旅程（J1–J4）中，/chat 相关故事映射如下（★=PR 门禁核心集，~3min）：

| # | User Story | 剧本 | 层 | 关键断言（摘要） | 锚定的已知风险 |
|---|---|---|---|---|---|
| 1★ | 新用户首聊：welcome→示例→流式→完成 | `happy-path` | L1+L2 | 气泡落定、aria 播报、stop→send 复位、consoleErrors 0 | 首条消息 `currentSessionId=null` 半绑定（A-6） |
| 2★ | 多轮对话 + 长流式 | `multi-turn-stream` | L1+L2 | 50+ chunk 分片不丢字、stick-to-bottom、FAB | chunk 边界/UTF-8 截断 |
| 3★ | 工具任务：bash 建文件→卡片→产物→diff | `tool-task-file` | L1+L2 | RunStatusLine 计时/进度、FileChangesCard +x−y、dock 自动开、FileCard | tool-start 重复卡（A-7） |
| 4★ | 审批三分支：Allow once/Always/Deny | `approval-allow` / `approval-deny` | L1+L2 | 弹窗信息完整、Deny 后错误卡展开、Always 落盘回执、rail amber dot 清除 | 审批超时自动 Deny 语义 |
| 5★ | 失败恢复：auth 错误→深链→重试 | `auth-error` + `mid-stream-fail` | L1+L2 | auth 横幅深链、ComposerRetryButton、**重试保留附件**、failed 后无 ghost bubble | A-2/A-3 重试丢附件 |
| 6★ | 取消：stop/Escape、半截文本 | `cancel-matrix`（§4.1，9 场景） | L1+L2 | cancelled 收敛、迟到事件不污染新 turn、半截文本策略 | A-17/A-18/A-19 |
| 7 | 预算：warning→exceeded→Continue once | `budget-exceeded` | L1+L2 | 双横幅形态、Continue once 走 bypass、**附件保留**、Raise 后解除 | A-2 续发丢附件 |
| 8 | 附件：粘贴图/拖拽/预检/拒绝清单 | `attachments` | L2 | chip 警告徽章、rejected toast、**乐观 append 后 chips 不消失** | A-4 附件 chips 消失 |
| 9★ | 排队与 steer：3 条队列、Ctrl+Enter | `queue-steer` | L1+L2 | chips 增删排序、steer 停车 settle、15s 超时还稿 | Imp-1/2 竞态已有回归，扩展 |
| 10 | 编辑重发 / rewind / 分支 | `edit-rewind` | L1+L2 | edit banner、commit 失败保编辑态、checkpoint 边界 | A-14 checkpoint=turn 边界语义 |
| 11★ | 会话管理：流式中切换/返回、draft | `session-switch-race` | L1+L2 | bucket 不串流、遮罩、draft 保留、红点清除 | A-5 错误横幅跨会话残留 |
| 12 | 子代理任务卡 | `subagent-run` | L1+L2 | SubagentBlock、subagentLive pill、stop 后收敛 | — |
| 13 | 跨页旅程：chat→files→timeline 深链 | `journey-cross-page` | L2 | 产物落 /files、timeline 打开、返回状态保持 | 走查 J1/J3 互链项 |
| 14 | 上下文面板：/context、/cost、/compact、usage | `context-panels` | L1+L2 | SlashResultCard、usage dialog 随 usageTick 刷新 | — |

**回归锚点**：走查已修复的 P0/P1（附件黑洞、composer-draft 丢稿、Add-to-chat 丢稿、导出沙箱等）凡涉及对话页的，直接以剧本断言形式钉死在上表对应行（"锚定风险"列即索引）；后续每个 chat 域修复 PR 必须附带或更新对应剧本，防止 R2 式复发。

### 4.1 专项一：取消（cancel）——支持，但有三个实质性风险

**现状链路（已实证）**：

- **入口三个**：流式中 send 槽变 stop 按钮（`ChatInput.tsx:1396-1433`）；Escape（优先退编辑态、再取消查询，`ChatInput.tsx:627-702`）；steer 第一跳 cancel+settle（`useSteerSend.ts`，等终态最多 15s 超时归还草稿）。预算触顶也复用同一 token（`commands.rs:2199`）。
- **链路**：前端 `cancelQuery`（`AppContext.tsx:564-574`，目标=窗口会话，失败 toast）→ invoke `cancel_query`（`commands_chat.rs:205-237`：take 并 cancel token → **立即**清 `session.querying` 闩）→ 桌面事件循环在**每个引擎事件边界**检查 token（`commands.rs:1651-1667`）→ emit `query:cancelled` → 前端清 bucket/投影、`runEnd(false)` 不算失败（`AppContext.tsx:1022-1046`）。
- **多窗口/后台**：`session_id` 显式路由不碰共享指针（P1-1）；后台任务有独立 token（`commands.rs:250-254`）。

**三个风险（即附录 A-17/A-18/A-19）**：

| # | 风险 | 证据 |
|---|---|---|
| R1 | **引擎不感知取消**：`QueryContext` 无 cancel/abort 字段，token 只在事件循环逐事件检查——中断粒度="下一个引擎事件"。文本流中 ≈1 个 chunk 内生效；但**无 progress 事件的工具执行期间 stop 无延迟生效**，且二连 stop 是 no-op（token 已被 take，`commands_chat.rs:223-227`）、UI 无任何反馈。bash 有 ToolProgress 缓解，其余工具无 | `crates/shannon-core/src/query_engine/types.rs`（grep cancel/abort 零命中）× `commands.rs:1651` |
| R2 | **stop→立刻重发的竞态**：后端取消时立即放开 querying 闩，但旧查询循环要等下一个事件才退出——间隙内新 `send_message` 可启动，同会话双查询并存；而前端 AppContext **全程不按 `query_id` 过滤事件**（grep 零命中，streaming bucket 按 session 键）→ 旧查询迟到的 text/tool 事件与迟到的 `query:cancelled` 会污染新 turn（清空新流、误停 isQuerying） | `commands_chat.rs:229-234` × `commands.rs:1651` × `AppContext.tsx`（无 query_id） |
| R3 | **取消丢弃全部半截输出**：cancelled 与 failed 同样清 bucket 不落定（无"保留已生成部分"），与 Claude/ChatGPT"stop 保留部分回复"不一致——产品语义待拍板（D6） | `AppContext.tsx:989-1046` |

**取消测试矩阵（剧本组 `cancel-matrix`，并入 P3）**：

| 场景 | 关键断言 |
|---|---|
| 文本流中 stop | ≤2 chunk 内出 cancelled；无残留 streaming/光标；stop→send 复位；半截文本策略按 D6 结论钉死 |
| 工具执行中 stop | RunStatusLine/进度 pill 收敛；工具自然结束后迟到 cancelled 正常 settle（A-18 回归） |
| **stop 后立刻重发**（竞态） | 新 turn 流式不被旧事件打断/清空；迟到 `query:cancelled` 不误停新查询（**A-17 核心回归**） |
| 审批弹窗等待中 stop | 弹窗残留语义、后续 respond 仍生效或被拒、300s/30s 超时自动 Deny 分支、最终 cancelled settle |
| Escape 分层 | 编辑态优先退；再按才取消查询 |
| steer 取消竞态 | settle 前不发；15s 超时还稿 + toast；跨会话 parked 语义 |
| 预算触顶自动取消 | cancelled 先于横幅动作；Continue once bypass 路径 |
| 双击 stop / idle 时 stop | 无报错 toast、无状态抖动 |
| 后台会话取消 + 多窗口 | session_id 显式路由正确；另一窗口不越窗响应 |

### 4.2 专项二：输入缓存（用户输入指令保留）——形态齐备但策略不一致

**四种形态现状（已实证）**：

| 形态 | 现状 | 位置 |
|---|---|---|
| 会话草稿 | ✅ 支持：localStorage `shannon.draft.<id>` 存 `{text, attachments, updatedAt}`；300ms 防抖写、切换会话同步 flush（编辑态 flush pre-edit 草稿）、发送清空、64KB 上限（超限仅内存）、quota/隐私模式 best-effort | `Chat.tsx:45-80, 188-206` |
| 流式中排队 | ✅ 支持：FIFO 容量 3（满则 toast）、chips 上移/下移/移除、drain effect 自动续发、per-session 停放 | `AppContext.tsx:81, 387-391`；`QueueChips.tsx` |
| 外部草稿桥 | ✅ 已修（R2）：`composerBridge` pending 队列 + mount 后 flush，外部入口（DataSources 等）不再丢稿 | `lib/composerBridge.ts` |
| 输入历史回溯 | ❌ 无：ArrowUp/Down 仅用于 mention/slash 菜单导航，无"上箭头调出历史指令"能力 | `ChatInput.tsx:637, 657` |

> 概念澄清：引擎层另有 Anthropic **prompt cache**（三层缓存断点注入，CLAUDE.md §Key Patterns），那是 LLM 请求侧的成本优化，与 UI 输入缓存无关，已实现——测试方案不覆盖（属 Rust 侧既有测试）。

**测试点（并入剧本 #9/#11，扩展断言）**：

- 草稿：跨会话隔离与往返保留、**附件随草稿保留**、发送清空、300ms 窗口内切换会话的 flush 竞态（不串稿）、编辑态 flush pre-edit 草稿、>64KB 静默不落盘（A-21）、重启后恢复；
- 队列：cap 3 满提示、排序/删除、跨会话 parked 后返回续发、drain 与 steer 互斥（已有 Imp-1/2 回归扩展）、**重启丢失**（A-20，是否要持久化待 D7 拍板）；
- 历史回溯：产品 gap 记录，不建测试（除非产品立项）。


---

## 5. 「找出未知问题」的四个机制

### 5.1 不变量探针（invariant prober）

L2 fixture 在每个剧本步骤后自动巡检一组跨场景不变量（不逐剧本手写）：

- `isQuerying` ⇄ stop 按钮可见 / send→Queue 切换 / edit 按钮 disabled 三者一致；
- `completed/failed/cancelled` 到达后 `streamingText` 必清空、活工具卡必收敛（防"永转圈"）;
- assistant 气泡最终文本 === 各 chunk 之和（防丢字/重复）；
- 错误横幅出现时必带可操作出口（retry/深链），下一次发送后必清除；
- 会话切换后：draft 属于目标会话、红点/遮罩清除、错误不串场（A-5）；
- console 零 error / 零未处理 rejection。

### 5.2 事件 fuzz / 变异（同一剧本自动生成变体）

后端真实会产生脏输入（事件无全局顺序保证）。对每个剧本自动派生变异并断言 **不崩溃 + 终态幂等**：

- 乱序：`tool-result` 早于 `tool-start`、`completed` 后又来 `text`；
- 重复：双 `tool-start` 同 id（A-7 重复卡）、双 `completed`；
- 跨会话串扰：事件带别的 `session_id` / 缺失 `session_id`（A-10 提到的错 commit 风险）；
- 边界载荷：空 chunk、纯 emoji、超长行、`progress: -1`/`1.5`、`tokens_used` 缺失、`meta.classification` 未知值；
- 速率：0ms 轰炸（100 chunk < 1s）与极端慢速。

变异集放 nightly，发现即转成固定回归剧本。

### 5.3 竞态剧本（人工编排的时序陷阱）

在真实交互间隙注入事件：流式中切会话/发新消息/编辑/取消后立刻重发/bypass 时来 failed/steer 停车期间 completed 到达……已有 Imp-1/Imp-2 回归是先例，扩充为固定剧本组。这是走查发现"静默失败"高发的区域，也是单元测试最难覆盖的部分。

### 5.4 捕虫清单（第一轮验收标准）

附录 A 的 16 条新可疑点即第一轮测试的验收：**每条要么被剧本证实为 bug（转修复 PR），要么写出证伪测试钉死语义**。走查报告 R2 的教训（逐环验证不够）由此制度化。

---

## 6. CI 集成与门禁

| 触发 | 内容 | 预算 |
|---|---|---|
| PR（desktop/ui/**） | 现有 unit shards + smoke e2e + **★核心 6 剧本**（L2）+ 状态机剧本测试（L1） | +3–4min |
| nightly | 全 14 journeys + fuzz 变异集 + 视觉状态矩阵 + a11y 动态扫 + 长会话性能守卫 | 独立 workflow |
| weekly | L3 fake-SSE 链路 + （远期）真壳冒烟 | 独立 workflow |

组织上沿用现有三配置：核心剧本进 `playwright.config.ts`（`@core` tag 门禁），全量走独立 `playwright.chat.config.ts`（nightly），避免拖慢 PR。失败产物沿用 trace-on-retry + 截图/视频。

---

## 7. 落地排期（供审核调整）

| Phase | 内容 | 产出 | 估时 |
|---|---|---|---|
| P0 spike | 实证 `plugin:event\|listen` 桥方案；定 DSL 细节（YAML+类型化 loader）；用 1 个手工剧本在 demo 模式跑通"流式+工具卡" | 可行性结论 + Demo 视频/截图 | 0.5–1d |
| P1 基建 | ScriptedBackend 四件套（事件桥/播放器/seed 注入/watchdog）+ Playwright fixture + page objects + DSL loader/校验 | `e2e/helpers/` + `lib/mock/scripted/` | 3–4d |
| P2 剧本库 + L1 | ★核心 6 剧本双层跑 + 零覆盖组件补测 + 纯函数边界 | 6 journeys 全绿 + 组件测试 | 4–5d |
| P3 全量 journeys + 回归锚点 | 其余 8 剧本 + 竞态剧本组 + findings 回归断言入剧本 | 14 journeys + 锚点清单核销 | 3–4d |
| P4 横切 | fuzz 变异集 → nightly；视觉状态矩阵；a11y 动态扫；长会话性能守卫 | nightly workflow | 3–4d（可拆） |
| P5（二期） | L3 fake SSE provider 链路 + `SHANNON_HOME` 隔离 | 契约级冒烟 workflow | 3–5d |

MVP = P0+P1+P2 ≈ **1.5 周**；全量 ≈ **3 周**。每 Phase 独立成 PR，P1 落地后即产生第一批 bug 报告（用附录 A 清单核销）。

---

## 8. 需要拍板的决策点

| # | 问题 | 推荐 |
|---|---|---|
| D1 | 剧本 DSL 用 YAML 还是 TS？ | **YAML**（与 `tests/scenarios` 对齐、非工程师可写、可做 fuzz 变换）+ TS 类型化 loader 校验 |
| D2 | L3 范围 | 本期只做 **fake SSE provider + shannon-server 契约冒烟**（无头可 CI）；tauri-driver 真壳测试延后 |
| D3 | 附录 A 16 条可疑点怎么处理 | **随剧本编写逐一证实**，证实的当周走独立修复 PR（对齐既有"修复循环"节奏）；不先修后测 |
| D4 | 视觉基线范围 | light+dark × 6 个关键状态（welcome/流式中/工具运行中/审批/错误/完成），阈值先沿用 5% 跑两周再收紧 |
| D5 | Vitest 串行（maxThreads=1）+ 626 行全局 setup 的脆弱性 | 本期不治理，只记录；剧本测试尽量进 Playwright 层以绕开该瓶颈 |
| D6 | 取消/失败后**半截输出丢弃**（现状）vs 保留"已生成部分"（Claude/ChatGPT 均保留，见走查 WC1） | 无论取舍，先把现状钉成回归锚点；是否改为保留走产品决策（涉及 session log 落定格式） |
| D7 | 流式中排队队列**不持久化**（重启丢失）vs 草稿已持久化——策略不一致 | 先钉住现状为回归锚点；持久化与否与 D6 一起排 backlog |
| D8 | `voice-input`（剧本 22）CI 策略 | **nightly 可选、默认 skip**（10-03 拍板） |
| D9 | G17 死键与不可达分支 | **删不补**；但 D9-b 复核翻转：QueueChips attachmentsOnly 占位在 A-9 修复后已成可达且被依赖的正面分支，改为"钉测试不删"（10-03 拍板修订） |

---

## 附录 A：本次调研新发现的可疑点（第一轮捕虫清单）

> 来源：3×Explore 源码走查，均带行号待证实。编号 A-1…A-16。

| # | 位置 | 疑点 |
|---|---|---|
| A-1 | `pages/Chat.tsx:450-453` | idle 直发时 `sendMessage()` 的 Promise 被丢弃且无条件清空输入/draft：后端拒绝（预算/并发守卫）时用户输入丢失（对比 commitEdit L390-398 有恢复） |
| A-2 | `pages/Chat.tsx:293` | 预算 "Continue once" 重发 `lastUser.content` 但丢弃附件 |
| A-3 | `pages/chat/MessageArea.tsx:523` | ComposerRetryButton 重发同样丢附件 |
| A-4 | `context/AppContext.tsx:519` | 乐观 append 的 user 消息不含 `file_attachments`：发送后附件 chips 消失，重载才回来 |
| A-5 | `context/AppContext.tsx:629-665` | switchToSession 不清 `error/errorKind`：上一会话错误横幅（含 auth）跨会话残留 |
| A-6 | `context/AppContext.tsx:1140` | 主窗口冷启动后 `currentSessionId` 仍为 null：RunStatusLine 无计时、依赖 sessionId 的动作半绑定 |
| A-7 | `context/AppContext.tsx:837-859` | tool-start 不按 `tool_use_id` 去重：后端重发产生重复卡片 |
| A-8 | `pages/chat/MessageArea.tsx:119-142` | useToolDurationLookup 仅在会话切换时拉取：同会话新完成 turn 的历史时长拿不到 |
| A-9 | `components/chat/ChatInput.tsx:1402-1411` × `Chat.tsx:441-443` | 流式中纯附件输入仍显示 Queue 按钮，但排队分支 `if (!trimmed) return` 静默吞掉 |
| A-10 | `context/AppContext.tsx:951-987` | 后台会话 completed 时 bucket 清空且不 commit；事件缺 `session_id` 时可能 commit 到错误可见会话 |
| A-11 | `context/AppContext.tsx:544-549` | 乐观回滚按 role+content 匹配删末条：同文本发两次且一成一败时可删错条目 |
| A-12 | `hooks/useBudgetGuard.ts:38-45` | budget 事件按 `currentSessionId` 过滤 + listen 异步重订阅：切换瞬间旧会话事件可能落进新会话 |
| A-13 | `pages/Chat.tsx:150-159` | `prefillApplied` 一次性 ref：同一挂载内第二次 prefill 导航被忽略 |
| A-14 | `pages/chat/MessageArea.tsx:194-201` | checkpoint turn 恰等于当前 turn 时也判 rewindable，边界语义待确认 |
| A-15 | `components/chat/ChatInput.tsx:576-600` | drop 事件在 effect 挂载前到达会丢附件（竞态窗口） |
| A-16 | `lib/composerBridge.ts:43-46` | 同 tick 多个 draft 推送时 pending 队列顺序依赖未定义 |
| A-17 | `commands_chat.rs:229-234` × `commands.rs:1651-1667` × `AppContext.tsx`（无 query_id 过滤） | 取消立即放开 querying 闩但旧循环要等下一个引擎事件才退出：stop→立刻重发的间隙内同会话双查询并存，旧查询迟到的 text/tool 事件与迟到 `query:cancelled` 会污染新 turn 状态（清空新流、误停 isQuerying） |
| A-18 | `crates/shannon-core/src/query_engine/types.rs`（无 cancel/abort 字段）× `commands_chat.rs:223-227` | 取消仅事件边界生效，引擎/工具无中断通道：无 progress 事件的工具执行期间 stop 不生效，且二连 stop 为 no-op、UI 零反馈 |
| A-19 | `AppContext.tsx:989-1046` | 取消与失败均丢弃全部半截流式输出（无"保留已生成部分"），与 Claude/ChatGPT 行为不一致（产品决策 → D6） |
| A-20 | `AppContext.tsx:81, 387-391` | 流式中 prompt 队列（cap 3）纯内存：重启/刷新丢失；与草稿持久化策略不一致（→ D7） |
| A-21 | `Chat.tsx:63-67` | 草稿 >64KB 静默不落盘（仅内存），无任何用户提示 |
| A-22 | `ChatInput.tsx:637, 657` | 无输入历史回溯（ArrowUp 仅用于 mention/slash 菜单导航）——产品 gap 记录，非缺陷 |

## 附录 B：后续维护规则（建议写入 CONTRIBUTING）

1. chat 域修复 PR 必须附带/更新对应剧本（回归锚点纪律，防 R2 式复发）；
2. 新增 `query:*` / `permission-*` / `budget:*` 事件字段时，同步更新 DSL schema 与至少 1 个剧本；
3. nightly fuzz 发现的崩溃 → 48h 内固化为固定剧本；
4. `UNMOCKED_ALLOWLIST` 新增条目需注明原因（现有 tripwire 机制沿用）。

---

## 执行结果 (2026-10-02，全部轮次完成)

方案经用户批准（D1-D7 全按推荐执行）后由 agent 团队分 6 轮实施，12 个 PR 全部合并 dev：

| 轮次 | PR | 内容 |
|---|---|---|
| R1 基建 | #209 | Tauri v2 事件桥（实测发现 demo 模式 listen 全失败并修复）+ ChatScript 播放器 + seed 注入 + `window.__shannonMock` + Playwright 三件套 |
| R2 核心剧本 | #210 | ★6 journeys 双层（L2 E2E + L1 状态机真驱动 AppProvider）+ knownIssue 锚定机制 + 零覆盖组件 41 测 |
| R3 全量 journeys | #214 | 其余 8 journeys + cancel-matrix 9 场景 + 输入缓存锚点 + seed schema 扩展（toolCalls/spentUsd/checkpoint/send 日志） |
| R4-G1..G7 | #219-#224, #228 | 7 组证实修复：附件保留（A-2 上游已修转正向钉/A-3/A-4）、发送完整性（A-1/A-11 引用回滚）、会话绑定（A-5/A-6 新增 Rust `get_active_session_id`+ACL/S-4 mock 保真度）、工具卡（A-7 首start胜出/A-8/S-2）、composer 五连（A-9/A-12/A-13/A-16/A-21）、A-17 竞态双层修复（TS query_id 过滤 + Rust 闩时序）、取消即时化（select! + 二连 stop 反馈 + S-3 portal stop） |
| R5 横切强化 | #229 | 事件 fuzz（13 变异/五类）+ 视觉状态矩阵 12 基线 + 动态 a11y 扫（债务 rule+target 双键台账）+ 长会话 perf 守卫 + nightly workflow |
| R6 契约冒烟 | #227 | fake SSE LLM + 真实 shannon-server 的 wire 契约 7 条（全离线）+ `just test-contract` |

### 本轮测试体系的真实战果
1. **F-1（fuzz 抓获，疑似产品 bug）**：`query:completed` 盖错 session_id → 发送会话 composer 锁永不释放；已冻结剧本 + KNOWN_FUZZ_WEDGES 立案
2. **A-17 竞态（cancel-matrix 证实）**：stop 后立刻重发，迟到事件清空新流/误停——已双层修复并翻转锚点
3. **动态 a11y 扫 4 条 serious**：权限弹窗无可访问名、运行中时长徽标对比度（静态 walkthrough 扫不到）、G7 stop pill 对比度 4.41:1（匹配器收紧后现形）、第二弹窗节点——均入 KNOWN_A11Y_DEBT 台账
4. **wire 契约缺口（R6）**：`started` 帧在真实链路无生产者；server 路径无审批通道（权限以 is_error 拒绝）——契约文档待裁定
5. 16 条源码走查可疑点全部处置：证实修复 12、证伪钉语义 2（A-14 checkpoint 边界、A-16 原疑点）、上游已修 2（A-2/A-9）、park 1（A-10 需结构性改动）、产品 gap 记录 1（A-22）

### 维护规则
见 CONTRIBUTING.md「Chat page (desktop/ui) — ChatScript test discipline」；运行细节台账 `.superpowers/sdd/2026-10-02-chat-testing-plan/`（工作区，未入库）与各轮报告 task-N-report.md。

### 教训（已入规程）
- 基线漂移侦测是 CI 排查第一步：5 轮"CI 独有失败"实为并行会话改预算横幅契约（#211-213）+ CI merge checkout 含新 dev；long-lived 分支需高频 rebase
- resume 代理前必须核对 worktree 所在分支（两次提交落错分支）
- Rust 组验收三件套 = cargo check + nextest + fmt --check（fmt 违规与 ACL 缺失都从缺口漏过）
- rebase 后必须 git log 确认预期提交在分支上（测试绿可能是旧断言）

### 留存清单处置 (2026-10-02，用户批准分层方案后执行)
- **已修（P0）**：F-1 composer 锁死（#231，owner 路由）；4 条 a11y serious 债务清零（#231）；`started` 帧补发射（#240）；sidebar-sessions flake 原子化（#241）
- **已采纳实施（P1）**：D6 取消保留半截（#232，前后端一体：tee interrupted 落盘 + stopped-chip 气泡）；A-22 输入历史回溯（#232，ArrowUp 环形缓冲）
- **已采纳实施（P2）**：Vitest 并行化 spike 成功（#242，3.2-3.4x，CI 全绿即转正）
- **文档化（P2/P3）**：D7 队列不持久化为有意设计（AppContext PROMPT_QUEUE_CAP 注释，含"勿无产品裁定修复"警示）；A-10 后台完成态瞬态不可见、tauri-driver 真壳测试延后、QUERY_FAILED 半截不一致（D6 邻接项）均记录为接受现状
- **流程**：CONTRIBUTING 新增长生命分支每日 rebase 规则；Rust 验收升级为四件套（+clippy）
- **仍开放的观察项**：QUERY_FAILED 半截重载无标记不一致（D6 邻接，tee 侧扩 reason 即可统一）；`continueTarget` undefined 锐边（组件测试已钉）；3 个非矩阵主题的 on-error 对比度（contrast-audit PAIRS 外，建议另立项）
---

## 9. v2 深度复核增补（2026-10-03）

> 本章为对 §1–§8 的独立复核结果与盲区补强。基线同 HEAD 828c4adb（复核时点，先于上文执行结果 R1–R6 合并）。复核方式：3 个 Explore 代理分别核实 22 条可疑点、盘点 /chat 全功能面找 journey 盲区、核实测试基建资产表；主会话另行实证 tauriBridge demo 路径。本章所引数字为复核时点现状；R 系列落地后已变化处见条目内括注。

### 9.1 架构前提复核结论：全部成立

| 前提 | 复核证据 |
|---|---|
| demo 模式零对话能力 | 主会话实证 `src/lib/runtime/tauriBridge.ts:139-150`：mock 分支 `queueMicrotask` 秒回 `{kind:'completed'}`，零 `query:*` 事件；`installListeners` 的 9 路 `listen()` 在 mock 下注册即失败（coreMock 无 `plugin:event\|listen` handler）——ScriptedBackend 必要性与 §2.2 事件桥方案成立 |
| L1 主控缝可复用 | `AppContextStreaming.test.tsx` 的 `vi.hoisted` captured + `flush()` 范式确认存在（L21-40），`vi.mock('@tauri-apps/api/event')` 可直接承接剧本事件流 |
| e2e 唯一 mock 通道 = 构建期别名 | 18 个 spec 全部依赖 `vite.config.ts` 将 `@tauri-apps/api/core` 别名到 `coreMock.ts`，**零 `page.route` 网络拦截先例**——§2.2 选 mock 层扩展（而非 route 拦截）是唯一顺路，备选否定理由加一 |
| L3 契约入口存在 | `POST /v1/sessions/:id/messages` SSE 路由确认（`crates/shannon-server/src/routes/mod.rs:147`）；注意 shannon-server 是 **lib-only**（二进制入口经 `shannon-cli` main.rs:3571），L3 驱动方式按 §8 D2 微调 |
| Rust 侧场景/回放基建 | `tests/scenarios/` 20 个 yaml + Rust loader（`shannon-core/src/testing/{scenario,test_env,eval_runner,mock_dsl}.rs`）；`SHANNON_RECORD_DIR/REPLAY_DIR` 行号精确吻合（client.rs:453-461）；`representative_events()` 16 个事件样例（sse.rs:89）——剧本事件样例来源成立 |

### 9.2 原文修正（以当前代码为准，原文相应条目以本节为准）

| # | 原文 | 修正 |
|---|---|---|
| C1 | 附录 A-16（composerBridge 同 tick 顺序未定义） | **反证**：未订阅时逐次 `pendingDrafts.push`（调用序=数组序），订阅时 `splice(0)` 严格 FIFO 派发，同 tick 内订阅态无 await 边界不可能翻转——顺序确定。从捕虫清单移出，改为"顺序契约证伪测试"目标（钉死 FIFO 语义） |
| C2 | 附录 A-10 后半（缺 session_id 事件 commit 错会话） | **降级 latent**：后端恒发 `session_id: Some`（commands.rs:1887-1890），当前不可达，属防御性分支。fuzz 变体保留（防后端回归），不作为预期 bug |
| C3 | 附录 A-14（checkpoint 等于当前 turn 也判 rewindable） | **非缺陷**：rewind 语义=「删除该回合及其后所有回合」（commands_rewind.rs:10-12），等号恰是撤销第 N 回合的必要条件（改 `>` 则第 0 回合永不可 rewind）。改记为"边界语义回归锚点" |
| C4 | §1.1 "tauri-api.ts ~280 命令" | 实测 **301 个唯一命令**（tripwire 测试注释里的 280 已过时）；handlers.ts 2096 行 / 218 个 handler / UNMOCKED_ALLOWLIST **97 条**（注：R 系列落地后事件桥/播放器等新 handler 已并入，此为复核时点数字，以 tripwire 测试实时为准） |
| C5 | §1.1 前后端事件名常量表 | 两表**非镜像**：前端 EVENT_NAMES 29 键、后端 event_names 33 const，互有缺失（前端独有 QUERY_NOTICE/SUBAGENT_*/SESSION_AUTO_UNARCHIVED；后端独有 TASK_STEP/UPDATE_*/VOICE_* 等）。DSL schema 校验按「**前端子集 + 例外清单**」实现，不能假设 1:1 |
| C6 | §3 横切 "视觉基线仅 3 页 × light × 空闲态" | 实际 `visual-baseline.spec.ts` 仅 27 行：3 页 × **默认单主题**（maxDiffPixelRatio 0.05）；主题矩阵在 themes.spec / theme-gallery.spec / walkthrough（14 路由 × 2 主题，axe 全规则）。D4 的"现状"据此修正，目标不变（注：R5 视觉状态矩阵 12 基线落地后，本条"现状"已失效，见执行结果） |
| C7 | §1.1 "lines 83%" | vitest thresholds 红线是 **lines 80**（functions 60/branches 75/statements 80）；现存 `coverage/` 产物为陈旧空数据（lcov LH:0），83% 不可证实。表述改为"红线 80，实测待重跑"（注：R 系列新增剧本层与组件测试后覆盖数字已再变化，红线 80 不变） |
| C8 | §2.1 蓝本说明 | `tests/scenarios` 的 loader/runner 是 **Rust 侧**（shannon-core/src/testing/）；TS 侧 ChatScript loader 从零建（三段式蓝本关系不变） |
| C9 | §3 L3 工具链 | Rust 侧无 wiremock，惯例是 **mockito 1.6 + `mock_dsl.rs`**（可渲染 Anthropic/OpenAI/Ollama 三格式 SSE）。fake SSE provider 优先复用 mock_dsl 起本地 mockito server 作为 `openai-compatible` 的 base_url，而非新写 axum 服务 |
| C10 | §2.2 事件桥实现注意 | `plugin:event\|listen`/`plugin:event\|unlisten` 作为新"命令"进入 mock 层后，**必须同步登记 tripwire**（`mock-handlers-coverage.test.ts` 双向检查：新命令无 handler 失败、handler 被删也失败），或显式加入 allowlist 并注明原因（§附录 B-4） |
| C11 | §1.1 组件零覆盖清单（精确化） | **零测试引用**：AttachmentChip、SlashResultCard、QueueChips、DeleteSessionModal、InlinePanelModal、ComposerContext；**仅间接覆盖**（无专属测试）：BudgetBanner、BudgetDialog、ContextBreakdownCard、GoalStartForm、diffStats、sessionModelPromotion、MessageArea、ComposerPanel、ContextPanel、PlanPanel、ApiKeyBanner。§3 L1 补测范围据此扩展 |

### 9.3 捕虫清单扩充（A-23…A-26，复核新发现）

| # | 位置 | 疑点 |
|---|---|---|
| A-23 | `Chat.tsx:292` × `AppContext.tsx:537-549` | **P1，用户可直接感知**：budget 预检拒绝会回滚乐观 append（"rejected BEFORE recording"），此后 `continuePastBudget` 取到的 `lastUser` 是上一（**已回答过的**）回合——"Continue once" 重发的是旧消息，被拒的那条输入彻底丢失 |
| A-24 | `useBudgetGuard.ts:38-45` | 切换会话后、旧监听器异步 unlisten 完成前，旧闭包收到旧会话 budget 事件会 `setWarning/setExceeded`，把旧会话横幅污染到当前界面；与 L52 重 derive 是 promise 竞速，胜负不确定 |
| A-25 | `Chat.tsx:431-434` | editing 状态下附件-only 发送（`!trimmed`）被静默 return——与 A-9 同类问题但位于独立路径（idle+editing） |
| A-26 | `Chat.tsx:229` × `Chat.tsx:390` | `startEdit` 只 `setInput(msg.content)` 不清空当前 `attachedFiles`：编辑期间 composer 显示与目标消息无关的附件 chips，且 commitEdit 用原消息附件整体替换——用户编辑期间新加的附件被静默丢弃 |

> 待证实清单口径更新：A-16 反证移出、A-14 改判语义锚点、A-10 后半降级 latent，净变化后 **24 条待证实**（A-1…A-13、A-15、A-17…A-26）。§5.4 的"第一轮验收"以此为准。
>
> 状态更新（v1 R 系列合并后）：**A-23/A-24 上游 R 系列已修**（Chat.tsx 预检 blockedPayload / useBudgetGuard ref 过滤），移出待证实清单；**A-25/A-26 与 D9 由 R7 quick-win 处理**（见 §9.6）。

### 9.4 剧本矩阵 v2：14 → 22（盲区扫描 G1–G22）

> 实施标注：新增剧本 15–22 与既有剧本扩展由 **本 wave R7 实施**（journeys-composer / journeys-chrome / journeys-env 三个 PR，见 §9.6）；本节为复核时点的设计定稿。

对 /chat 全功能面（Chat.tsx 705 行、ChatInput.tsx 1457 行、RightDock.tsx 849 行、SidebarSessions.tsx 1819 行、AppContext.tsx 1189 行、slash 15 命令、10 locale、全局快捷键全集）逐区盘点，得 22 个 gap（G1–G22），归并为 8 条新增剧本 + 8 条既有剧本扩展：

**新增剧本（编号续 §4）**：

| # | 剧本 | 覆盖 gap | 优先级 | 关键断言（摘要） |
|---|---|---|---|---|
| 15 | `slash-commands` | G1 | 高 | /goal 表单必填校验与内联报错、`goal-start-success`；/diff 四态（notRepo/noChanges/truncated/patch 展开）；/export 保存对话框取消/失败；/new 建会话；/dream skipped toast 矩阵；**parse 规则**：`/name args`、未知 token 作纯文本、别名（parse 先做 L1 纯函数表驱动） |
| 16 | `file-mention` | G2 | 高 | @ 菜单开合、fuzzy 排序、键盘导航、插入相对路径 + caret 复位、mid-text mention、email 不触发、Escape 重 arm、无 working dir 降级横幅（与 J8 附件是不同管线：纯文本无预检） |
| 17 | `model-mode-switch` | G3+G4 | 高（前置 testid） | 模型 chip 会话 override（"· session"后缀/Set as default 晋级/Reset 回继承）、effort 四档 label、ExecutionModeSwitcher 四档写 approval_mode + 未知值 rawLabel 回显、PhaseTierSwitcher、**切换后下一 turn 才生效**语义、approval 模式对审批弹窗出现与否的影响 |
| 18 | `composer-draft-bridge` | G6+G20 | 高（R2 修复回归高危区） | 五个草稿入口（companion 窗口/终端 fenced prefill/PPT 大纲/csv Batch run/Session sources cite）契约：**永不自动发送、追加不覆盖、未挂载暂存 mount 后 flush**；InlinePanelModal 脏确认三路关闭 |
| 19 | `session-lifecycle` | G7 | 中 | 内联重命名、pin + 拖拽排序持久化、三分组切换持久化、归档→重开 unarchive toast、DeleteSessionModal（pending/失败保持打开/permanent 变体）、导出 md、后端全文搜索防抖、50 条 cap |
| 20 | `dock-interactions` | G8+G9 | 中 | 拖宽/键盘调宽 clamp（280-720 + 60% 视口）、全屏进出、`shannon.dock.*` 持久化恢复、Ctrl+\ 一次性 hint、自动停靠四源（artifact/plan/diff/run）、**PlanPanel 人工勾选写回 save_text_file 失败回滚**、html 交互注册失败降级静态 hint |
| 21 | `multi-window` | G5 | 中 | windowSession 事件过滤（只收本会话）、审批弹窗只弹本窗、双页实例并发流式/取消互不串扰、reveal→主窗切会话导航、关窗收敛（L2 用 `/?windowSession=` 起两 page；真壳归 L3） |
| 22 | `voice-input` | G12 | 低（nightly 可选） | voice.supported 门禁、VoiceOrb 录音态、转写合并进草稿、流式中禁用、local/cloud provider 切换（mock provider；→ D8 拍板 CI 策略） |

**既有剧本扩展**：

| 剧本 | 扩展（gap） | 新增断言 |
|---|---|---|
| J2 multi-turn-stream | G11+G15 | Ctrl+F 计数 0 态/Enter 环游/虚拟化（>30 条）两跳转路径/flash ring/Esc 归还焦点；流式中切语言不丢流 |
| J3 tool-task-file | G19 | FileCard 全动作面：PDF 预览懒加载失败态、csv Batch run、save-as 取消静默、reveal/open 失败 toast、抽取明细 |
| J4 approval-* | G4+G22 | risk 四级配色与 aria、reason 三源（rule 名/llm 置信度/default）、**背板关闭=Deny**、legacy 无 reason 兼容 |
| J5 auth-error | G13 | 错误分类全集（仅 auth/other 两类）、Retry 无历史消息时按钮消失、StreamNoticeLine（failover/key_rotation）存活到下次发送、goal-owned 发送阻断、initError banner |
| J8 attachments | G18 | issue/extraction 徽章随 chips 剪枝时序、detach-all、no-working-dir 横幅出现/消失、`registerFileIndexEntry`→Files 页联动 |
| J10 edit-rewind | G10 | branch（fork 确认→**切换 currentSessionId**，与 J11 竞态面交叉）、regenerate（仅最后一条 assistant + idle 门禁）、👍/👎 持久化往返 |
| J13 journey-cross-page | G21 | 扩为四角旅程 chat↔files↔timeline↔**terminal**（Ctrl+` 切换、代码块 run-in-terminal、选中发送 prefill、drawer 关闭态事件三连） |
| P4 横切 | G14+G15+G16 | 视觉矩阵加入空态/骨架（WelcomeState、sidebar skeleton、切换遮罩、dock/plan 空态）；主题/语言即时反映（data-theme、`<html lang>`、Toaster 跟随）；shortcuts↔help 面板一致性断言（L1 可先行） |

**转 finding 不建剧本（G17）**：① WelcomeState 广告 "Alt+Up history" 快捷键全仓无 handler（与 A-22 同根）；② QueueChips 的 attachmentsOnly 占位在现 UI 不可达（流式中纯附件 no-op）——转产品/修复清单，→ D9（D9-b 修订：②经 A-9 修复后已成可达且被依赖的正面分支，改判"钉测试不删"，见 §8 D9）。

### 9.5 前置工作 P1.5：testid 补齐

盲区扫描确认 **testid 无集中注册表**（全部组件内联），且以下高频交互面**完全没有 testid**（只有 aria-label/class），剧本 17 与 J4/J11 扩展锚定前需先补：

- Header：预算徽章、**审批弹窗（permission dialog）**、ExecutionModeSwitcher、PhaseTierSwitcher
- ChatInput：模型 chip 按钮、plan 模式横幅、审批模式 pill、"+" 菜单
- ApiKeyBanner：现为 class（`shannon-apikey-banner`）非 testid

建议顺带建立 `e2e/helpers/testids.ts` 常量表（新增锚点登记，避免字符串漂移），工作量并入 P1。

> 状态更新（2026-10-03）：P1.5 由本 wave R7 的 testids PR 实施（集中注册表 `e2e/helpers/testids.ts` + 补齐缺失 testid），见 §9.6。

### 9.6 v2 增量剩余范围与实施（R7, 2026-10-03）

v1 执行结果（R1–R6）已覆盖基建与 §9.2 修正面；v2 增量剩余范围（§9.3 收尾 + §9.4 矩阵 + §9.5 前置）拆为 6 个 PR，自 dev@7e5203a90 切出，分 worktree 并行实施、串行合并：

| # | PR | 内容 | 文件域（互斥） |
|---|---|---|---|
| 1 | `fix/chat-quickwin-v2` | §9.3 收尾：A-25（编辑态附件-only 提交，语义对齐 A-9 修复后主路径）、A-26（编辑期间附件丢弃，语义="所见即所发"）、D9-a（删 WelcomeState Alt+Up 广告）；D9-b 翻转后为 QueueChips attachmentsOnly 分支钉测试（不删）。每项附 failing-then-passing 锚点 | `pages/Chat.tsx`、`WelcomeState.tsx` + 十语 i18n、edit-rewind/queue-steer/first-chat 剧本、新 L1 文件 |
| 2 | `feat/chat-testids-registry` | §9.5 P1.5：集中 testid 注册表 `e2e/helpers/testids.ts` + 补齐 7 处缺失 testid + 权限弹窗可访问名（KNOWN_A11Y_DEBT ①同步清台账） | `Header.tsx`、`ExecutionModeSwitcher.tsx`、`ChatInput.tsx`（仅 testid 行）、`ApiKeyBanner.tsx`、`a11yDebt.ts`、`helpers/testids.ts`（新） |
| 3 | `feat/chat-journeys-composer` | §9.4 剧本 15/16/18（slash-commands / file-mention / composer-draft-bridge）+ J2/J5/J8 扩展 | 新 spec+yaml（composer 域）、`lib/slash` L1 新文件、errors/multi-turn-stream/attachments 扩展 |
| 4 | `feat/chat-journeys-chrome` | §9.4 剧本 17/19/20（model-mode-switch / session-lifecycle / dock-interactions）+ J3/J4/J13 扩展 + seed schema 扩展（铁律 2 三件套） | `mock/scripted/schema.ts`+`seed.ts`、新 spec+yaml（chrome 域）、approval/tool-task-file/cross-page 扩展 |
| 5 | `feat/chat-journeys-env` | §9.4 剧本 21/22（multi-window / voice-input）+ i18n-theme 横切，全部 nightly-only 家族（D8：voice-input 默认 skip） | 新 nightly spec+yaml（env 域）；不碰 `schema.ts` |
| 6 | `docs/chat-test-plan-v2` | 本 PR：v2 复核与 D8/D9 拍板入档（§8 增行 + 本章）+ CONTRIBUTING KNOWN_FUZZ_WEDGES 引用漂移修正 | 本文档、`CONTRIBUTING.md` |

实施约束：

- **文件域互斥**：六个 PR 文件域两两不相交（上表第 4 列）；在飞禁区（budget/cancel-matrix 两 spec、AppContext 事件路由、fuzz-found、`mock/scripted/player.ts`）本 wave 全员不碰。
- **合并顺序**：`testids` 先于 `journeys-chrome`（chrome 域 spec 的 permission-dialog/budget-badge 等锚点依赖注册表；注册表合并前 chrome spec 以集中常量占位并注释来源）；quickwin 与两条 journeys 线可并行；`docs`（本 PR）最后合并，合并前补写 §10。

---

## 10. R7 执行结果 (2026-10-03，v2 增量 wave 全部合并)

§9 的 v2 增量由独立会话的 agent 团队实施（6 实现分域 worktree 并行 + 5 审查 + 修复轮，全 PR 审查 Approved 零 Critical），7 个 PR 全部合并 dev：

| PR | 内容 |
|---|---|
| #235 | quickwin：A-25/A-26（编辑态附件所见即所发 + attachments-only 可提交）、D9-a（删 Alt+Up 广告+十语 key）、D9-b 翻转钉测；额外证实并修复 handleAttach append→replace 契约违约 |
| #236 | P1.5：e2e/helpers/testids.ts 注册表（51 静态+7 模板）、七处 testid 补齐、权限弹窗可访问名（与 #231 殊途同归，rebase 取并集） |
| #237 | J15 slash（9 剧本+L1 parse 表）、J16 file-mention、J18 draft-bridge 三契约；errors/Ctrl+F/attachments 扩展；flushSync 出 commit phase 产品修复（G11 揭示，failing-then-passing） |
| #238 | seed schema 五字段三件套（modelOverride/approvalMode/workingDir/deleteFails/saveTextFileFails）；J17/J19/J20；G19/G21/G22 扩展 |
| #239 | J21 multi-window 双页投影、J22 voice（7 个 STT mock）、G15 i18n-theme——全部 nightly-only（D8） |
| #244 | 急救：#243 误删 PROMPT_QUEUE_CAP 致 dev tsc 红，Fast checks 过后 admin 合入 |
| #246 | D9-a 后记：#232 在并行 wave 实现了 A-22 历史回溯，D9-a 删除的广告变成假话——revert 恢复提示行与十语 key（见下"并行裁定"） |

### 与并行 wave 的交织（记录在案）
本 wave 执行期间 dev 先后合入 #231/#232（D6+A-22）/ #233 / #240 / #241 / #242 / #243。三次 rebase 冲突全部集中在 mock 层（handlers/schema/coverage allowlist/player imports）——**教训：并行改 mock 层时冲突必须 grep 重复键**（git auto-merge 会让双 `save_text_file`/双 `plugin:dialog|save` 静默共存、后者覆盖前者）；#236 与 #231 对权限弹窗可访问名的修法殊途同归，取 dev 侧+叠加 testId。

### 并行裁定（controller 代裁，供用户否决）
- **D9-a 部分翻转**：D9-a 删除广告时 A-22 未实现（真话删除）；#232 随后实现了历史回溯，恢复广告才诚实 → #246 revert。
- **D7 维持 dev 侧裁定**：#243 把"队列不持久化"文档化为有意设计（PROMPT_QUEUE_CAP 注释含勿无产品裁定修复警示）——与本 wave §8 D7"补持久化"拍板冲突，以 dev 侧现状为准、持久化维持 backlog，待用户明确否决再动。

### R7 新发现的待修缺陷（已钉现状断言，修复时翻转）
- **F-voice-gate**：voice factory stub `isSupported()` 恒真——无 MediaRecorder 环境也渲染 MicButton 并吐 stub 文本（ChatInput 门禁永真，与注释宣称矛盾）。
- **F-theme-system**：ThemeContext 同值 setState eager bailout——system 主题不随 OS 切换即时重算（代码自注释"triggers re-render"不成立）。
- a11yDebt 候选：Header 两个下拉展开后被虚拟化消息气泡赢 hit-test（现 spec 以 dispatchEvent workaround 驱动）。

### Parked minors（不阻塞，随手清）
visual-matrix welcome 基线语义（下次 nightly `--update-snapshots`）；slash spec 一处负断言 waitForTimeout(800)；MessageArea 内联 ref→useCallback；emitWebviewDrop helper 两 spec 重复可入 helpers；CONTRIBUTING 的 KNOWN_FUZZ_WEDGES 位置引用已在本 wave 修正。
