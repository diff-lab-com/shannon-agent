# R2-W2 跨仓手机面(mobile face)实施排期 — 2026-10-03

- **依据**: shannon-mobile `docs/cross-repo-adaptation-spec.md` §J–§M;2026-10-03 对抗性审查报告(六项声明核实 + 低估修正);当日三项裁决(Eric 拍板)。
- **规模口径**: S=≤1 天、M=2-5 天、L=1-2 周。**PR 拆分**: 本批为 1 个 mono 集成 PR(`feat/r2-w2-mobile-face`,按工作包分提交)+ 1 个 mobile 联动 PR(`feat/r2-w2-c8-mobile-face`)。

## 裁决记录(2026-10-03,已拍板,实施按此执行)

1. **revoke-other 放开(§M2)**: 已配对设备可经 `shannon/device.revoke` 吊销**任意**已注册设备(含他机);二次确认由客户端 UI 负责(error 色确认键);吊销成功后向**其他**在线设备广播 `shannon/event {type:"device.revoked", device_id}`(手机端容忍未知事件类型,渐进接线);desktop UI 既有吊销路径不变。原"仅自吊销"防偷机论证由客户端确认流承接。
2. **§J 数据面路径选 (a)**: 引擎 WS 协议新增 `sessions.list` / `session.history` 消息(shannon-core `api_server` 直接复用 L0 `SessionStore::list/load`),gateway 透传适配;**不采用** gateway 直读 `~/.shannon/sessions` 目录的路径 (b)。
3. **§K 无兼容窗口**: 无旧版手机构建,`task.dispatch`/`task.list` 一次性切 §K 形状,不做 `{text}`/`{prompt}` 双读;`task.dispatch` 的"文本代答审批"分支(`kind:"approval"`)随之移除——手机审批走签名的 `shannon/approval/decide`,hub 内部 Y/N settle(IM 适配器共用)保留。gateway 内置 PWA 页(`web/page.ts`)与 dev FakeEngine 同 PR 同步。

## 审查结论备忘(2026-10-03,清单修订依据)

原五项收敛清单事实全部属实,但按审查修正了四处:§M 从"形状确认"升格为**四重错位修复 + 吊销策略裁决**(键名 camelCase、时间戳 ISO-8601、revoke 请求键 `deviceId`、他机吊销放开);§J 补记**引擎数据面前置**(真实规模 M,非 S);§L1 从"gateway S"改记为**引擎 wire 协议变更**(Rust + gateway + additive optional);C8 拆期——第一期(会话级花费)不依赖 mono(gateway 已在 `task.progress` 流式下发 `usage{input_tokens,output_tokens,cost_usd}`),仅跨会话聚合留待 mono W2-2 之后。

## 工作项

### mono(`feat/r2-w2-mobile-face` → PR 合入 `dev`)

| 包 | 内容 | 主要落点 | 规模 | 依赖/顺手项 | 验收要点 |
|---|---|---|---|---|---|
| MF-1 | **§M 设备面**: device.list 改 camelCase+ISO 时间戳(去 `public_key`,磁盘 registry 格式不动);device.revoke 认 `deviceId`、放开他机吊销;吊销后向其他在线设备广播 `device.revoked` | `gateway/src/mobile/pairing.ts`(+tests) | S | 裁决 1 | 手机管理页对真实 gateway 显示 2 台设备(标签/时间正确);他机吊销成功;被吊销设备 resume 收 PAIRING_REQUIRED |
| MF-2 | **§L2 审批恢复面**: gateway 侧待决审批注册表(记录/resolve/超时清理);新增 `shannon/approval.list`(响应 `{"pendingApprovals":[...]}`,条目形状逐字对齐 mobile `approvalFromMap`);`shannon/snapshot` 填充真实 `pendingApprovals` | `gateway/src/mobile/`(新 `approvalRegistry.ts`、`pairing.ts`、`engineBridge.ts`、`bootstrap.ts`) | M | — | 审批产生后手机重连 → 队列经 snapshot/approval.list 任一路径恢复;decide/超时后条目消失 |
| MF-3 | **§J 会话面(gateway)**: 新增 `shannon/session.list`(camelCase,`id` 必填)/`shannon/session.history`(`before`/`limit`/`hasMore`,默认 50,锚点取 ts 首现、同 ts 组不切分、升序);未知 sessionId 回空;方法名同步 SHANNON_METHODS + protocol schema | `gateway/src/mobile/engineBridge.ts`、`wsClient.ts`(加一次性 call)、新 `engineSessions.ts`、`protocol.ts`、`docs/protocol/shannon-mobile-protocol.schema.json` | M | 依赖 MF-5 的引擎消息 | §J5 验收:桌面开两会话 → 手机列表两条;点入渲染全文;断网重连自动补;旧 gateway 回归不变 |
| MF-4 | **§K 任务面 + §L1 透传**: dispatch `{prompt, agent_id?}` → `{task:{id,prompt,status,agent_id,created_at}}`(agent_id 非空即 INVALID_PARAMS,诚实无花名册);task.list 改 §K2 形状;移除 approval 代答分支;派发任务的引擎事件带 `session_id=task.id` 仅推发起设备(text 增量→`task.progress`,终态→`task.message`,失败→`query.failed`);`mapEngineEvent` 透传 `ts/agent/risk`;PWA 页与 FakeEngine 同步 | `gateway/src/mobile/taskHandlers.ts`、`hub.ts`、`engineBridge.ts`、`protocol.ts`、`router/`、`web/page.ts`、`dev-standalone.ts` | M | 裁决 3;依赖 MF-5(types.gen.ts) | §K5 验收:手机发起任务 → 桌面可见执行 → 线程流式增量+终态回复;离线拒绝不入队 |
| MF-5 | **§L1/§J 引擎协议(前置)**: `shannon-api-protocol` 新增 `sessions.list`/`session.history` 消息与 `SessionsSnapshot`/`SessionTranscript` 响应;`ApprovalRequest` 增 `ts`(epoch ms)/`agent{id,name}`/`risk{scope:local\|repo\|system, reversible}`(全部 `#[serde(default)]` additive,版本不 bump);shannon-core `api_server` 实现(复用 L0 `SessionStore`);**重跑 gen-ts 同步 `types.gen.ts`** | `crates/shannon-api-protocol/src/lib.rs`、`crates/shannon-core/src/api_server.rs`、`gateway/src/engine/types.gen.ts` | M | MF-3/MF-4 的前置;codegen_drift 测试强制 | wire serde 往返测试;临时 sessions 目录下的 list/history 集成测试;history 分页锚点/同 ts 组边界用例 |

### mobile(`feat/r2-w2-c8-mobile-face` → PR 合入 `dev`,mono 合并后落地记录回写)

| 包 | 内容 | 主要落点 | 规模 | 依赖 | 验收要点 |
|---|---|---|---|---|---|
| MB-1 | **C8 第一期(会话级花费)**: 解析 `task.progress.usage`;按会话键累计 `{inputTokens,outputTokens,costUsd}`;会话页 AppBar「有数据才渲染」花费 pill;en/zh l10n | `lib/src/live/protocol_mapper.dart`、`live_chat_conversations.dart`、`live_chat_providers.dart`、`lib/features/dev/agent_detail_screen.dart`、`lib/l10n/*.arb` | S | 无(mono 已在流里带 usage) | 有 usage 数据显示、无数据零渲染(golden 零改动);累计正确 |
| MB-2 | **契约 pin 同步**: mock `_streamQuery/_streamTask` 末尾发 usage 事件;contract test 增 usage 断言;approval.request 键集 pin 扩为含 `ts/agent/risk` 十键(mock 同步发富字段);`approvalFromEvent` 读取新字段渐进降级 | `tool/mock_server.dart`、`test/gateway_contract_test.dart`、`lib/src/live/protocol_mapper.dart` | S | 对齐 mono MF-4/MF-5 | mock ≡ gateway 新契约;`tool/cross_repo_check.sh` 对 mono 新分支通过 |
| MB-3 | **文档回写**: spec §J/§K/§L1/§L2/§M2 落地记录(mono merge hash);product-review B8 勘误("协议就绪"撰写时不成立) | `docs/cross-repo-adaptation-spec.md`、`docs/product-review-2026-10-02.md` | S | mono PR 合并后 | 按 A8b/G-rev3 格式;B8 依赖列勘误注明 |

## 排期总览

```
裁决/审查(2026-10-03,已完成)
        │
MF-5 引擎协议 ──┬─ MF-3 会话面 ──┐
                └─ MF-4 任务面 ──┤
MF-1 设备面 ────────────────────┼─► mono 集成 PR ──► merge dev
MF-2 审批恢复面 ────────────────┘         │
                                          ▼
MB-1 C8 第一期 ─┬─ MB-2 契约 pin ──► mobile PR(附落地记录) ──► merge dev
MB-3 文档回写 ──┘
```

## 合并门

- **mono**: `cd gateway && pnpm typecheck && pnpm test` 全绿;`cargo nextest run -p shannon-api-protocol -p shannon-core`(含 `--test codegen_drift`)全绿;clippy `-D warnings` 无新增。
- **mobile**: `flutter analyze --no-pub --fatal-infos` 0 issue;`flutter gen-l10n` 幂等;`dart format`;`flutter test test/gateway_contract_test.dart test/screens_golden_test.dart` + 新增用例全绿;**golden 零改动**(MB-1 的"有数据才渲染"保证 mock 场景无花费 pill)。
- **跨仓**: mobile `tool/cross_repo_check.sh` 指向 mono 本分支通过。

## 评审关注点

1. §M 磁盘 registry(desktop Rust 镜像 snake_case)**不得**随 RPC reshaping 改动——只动 wire 响应。
2. §L1 一律 additive optional;`risk` 缺真实分类源时**宁缺勿造**(mobile 渐进降级);`is_destructive` 保留。
3. §K 事件仅推发起设备;`task.message` 为任务线程终态;失败终态用 `query.failed` + Fleet journal 迁移,不改 mock 既有 happy path。
4. §J2 分页锚点取 ts 首现、同 ts 组边界不切分——mock(`tool/mock_server.dart`)已是参考实现,engine/gateway 两侧实现以 mobile `test/session_history_paging_test.dart` 语义为准。
5. `approval.list`/snapshot 条目形状以 mobile `approvalFromMap`(`protocol_mapper.dart:84`)逐字为准:`{approvalId, kind, headline, risk:'high'|'medium'|'low', scope?:[], diffTitle?, timestamp, toolInput, agentId?, agentName?}`,响应包 `{"pendingApprovals":[...]}`。

## 遗留跟进(2026-10-03,r2-w2b 批次登记)

> 本批(r2-w2b)为 r2-w2 的遗留跟进:①dev-standalone 挂上 §K 任务面(补真 gateway 冒烟缺口);②session.list 透传 token 总量(C8 快速胜利)。以下三项为**排期外遗留**,共性是引擎/聚合侧前置缺失——mobile/gateway 两侧消费面均已就绪(r2-w2 §L1 落地),前置补上即自动点亮,无需反向改动(「宁缺勿造」降级口径不变)。登记格式沿用 `2026-10-02-r2-followup-plan.md` 的表列/验收要点风格。

| # | 项 | 内容 | 前置/依赖 | 规模 | 验收要点 |
|---|---|---|---|---|---|
| FL-1 | **审批 risk 三维落地**(W3 候选) | scope/reversible 分类源放**工具元数据**:`shannon-tool-interface`/`shannon-tools` 每工具声明;bash 类 → `system + reversible:false`,文件编辑默认 `repo`;引擎审批发射点从元数据读取填 `risk{scope,reversible}`(`destructive` 已有) | 引擎侧 wire 字段 r2-w2 §L1 已落(`#[serde(default)]` additive),只缺分类源 | M | 审批事件携带真实三维 risk;gateway 透传与 mobile 渲染自动点亮、零反向改动;**拒绝**用 permission_classifier 的严重度冒充 scope(严重度≠作用域,语义不可混用) |
| FL-2 | **agent 归因** | 引擎 WS 协议增 profile/agent 字段时,`ApprovalRequest.agent{id,name}` 同步接线发射,手机 Agent Context 卡显示真实发起 agent | 挂 **P1-4b**(WS 协议 profile 字段,见 `docs/integrations/mobile-dispatch.md` §4) | S(P1-4b 顺带) | 「审批事件可归因」列为 P1-4b 验收项;缺席时维持现降级(品牌字回退、id 空串)不回归 |
| FL-3 | **C8 二期(跨会话/按日花费聚合)** | 跨会话/按日聚合依赖 mono usage ledger 聚合;落地时按 §J4 reserve-then-enable 模式先钉 `usage.summary` 类 wire 契约再实现 | 挂 **W2-2**(usage ledger 聚合,见 `2026-10-01-r2-improvement-schedule.md`) | M | 契约先钉后实现;会话级 token 总量已随本批 `session.list` 透传(`totalInputTokens`/`totalOutputTokens`,引擎有值才带)——快速胜利完成 |
