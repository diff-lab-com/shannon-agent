# 移动派发指南（P2-1：配对 → 派发 → 看任务 → 审批 → 进度）

用**一部手机（或任何浏览器）**给 Shannon 派发任务：发一条文本即创建任务，可以随时查看最近任务状态，
引擎需要确认敏感操作时手机上批准 / 拒绝，任务的开始 / 完成 / 失败进度会实时推回手机。
**不需要原生 App**——手机打开网关自带的一个网页（PWA 式本地页）即可，全部通信走既有的
配对令牌 + 端到端加密通道。

- 页面来源：网关移动端口（默认 `127.0.0.1:33430`）的 **GET /** 直接返回页面（`gateway/src/mobile/web/page.ts`）。
- 协议：NDJSON JSON-RPC `shannon/*`（`shannon/pair`、`shannon/device.resume`、
  `shannon/task.dispatch`、`shannon/task.list`、`shannon/event` 推送；
  r2-w2 起另有 §J 会话、§L1/§L2 审批、§M 设备四面，见 §8）。
- 任务管线：与 IM 渠道（P1-4）完全同一套——按设备分会话串行（lane）→ 审批回环 → 生命周期回推
  （`router/lifecycle.ts`：🚀 已开始 / ✅ 完成 / ❌ 失败）。

---

## 1. 前置条件

1. 桌面端已登录并启动（引擎 loopback `127.0.0.1:33420` 随桌面自动拉起）。
2. **设置 → 连接 → Gateway process** 卡片中启动受管网关。
3. 手机与桌面在**同一局域网**（v1 的配对二维码是 LAN 直连模式）。

## 2. 配对（桌面 → 手机浏览器）

1. 桌面 **设置 → 连接 → 移动派发** 卡片，确认通道状态为 **运行中**（网关未运行时先启动网关）。
2. 点击 **生成配对码**：出现二维码、`ws://<局域网IP>:33430` 地址、**75 秒一次性配对令牌**。
3. 手机浏览器打开页面地址（如 `http://192.168.1.10:33430/`），
   把二维码下方的**配对令牌**粘贴进页面「配对」区，点击 **配对**。
   - **前提：网关的移动通道必须绑定在可路由地址上**——默认配置只绑定 `127.0.0.1`，
     此时手机打不开该地址（卡片会显示回环注解）。先看下面的「主机绑定」。
   - 页面会在本机生成 Ed25519 密钥对（存在手机 localStorage，服务端只见公钥），
     并用一次性令牌完成 possession 证明（`shannon/pair`）。
   - 令牌 75 秒过期、单次有效；错过就重新生成。
4. 配对成功后设备出现在桌面「已配对设备」列表（`~/.shannon/mobile-devices.json`），
   可随时 **撤销**。手机再次打开页面会自动用签名时间戳重连（`shannon/device.resume`，±60s 时钟容差）。

> 桌面 App（原生 shannon-mobile）扫二维码配对的方式同样可用——协议不变。

### 2.1 主机绑定（手机访问的前提）

桌面写入的网关默认配置把移动通道绑定在 **`127.0.0.1`（回环）** 上
（`desktop/src/commands_mobile_pairing.rs` 的 `default_mobile_config`；网关侧
`bootstrap.ts` 同样回退 `127.0.0.1`）。回环绑定下：

- 网关自带页面**只有本机浏览器能打开**（`http://127.0.0.1:33430/`）——
  「浏览器即手机」在同机验收（见 §6）开箱即用；
- 配对二维码/令牌宣告的 `ws://<局域网IP>:33430` 手机**连不上**（网关未监听该网卡）；
  桌面「移动派发」卡片检测到回环绑定时会显示注解提示。

要让另一台设备（手机）访问，需把绑定改为可路由地址并重启网关：

1. 编辑 `~/.shannon/gateway/config.json` 的 `mobile.host`，改成桌面的**局域网 IP**
   （如 `"192.168.1.10"`；建议指定具体 IP 而非 `0.0.0.0`，避免无谓地暴露到所有网卡）。
2. 重启网关（设置 → 连接 → Gateway process → 停止/启动）。
3. 卡片的地址提示此时才会指向手机可达的 `http://<局域网IP>:33430/`。

**明文传输权衡**：LAN 直连的页面与 `shannon/*` 协议帧（含一次性配对令牌）在 Wi-Fi 上
**不加密**——这是 v1 二维码 LAN 直连模式的既有姿态（令牌 75 秒一次性、消费即失效，
方法面全部要求已配对会话）。请只在**可信局域网**下开启可路由绑定；跨网访问应等
relay 端到端加密通道的浏览器支持（§7），或使用原生客户端的 relay 模式。其余安全基线见 §5。

## 3. 四个动作

| 动作 | 页面操作 | 网关行为 |
| --- | --- | --- |
| **派发** | 输入框写一句话，点 **派发**（`shannon/task.dispatch {prompt}`） | 创建任务并同步返回 §K 任务对象 `{task:{id,prompt,status,agent_id,created_at}}`（r2-w2 起 §K 形状，不再收 `{text}`/回 `{task_id}`）；按设备分会话串行执行，事件带 `session_id=task.id` 仅推发起设备 |
| **看任务** | 右上角 **任务**（`shannon/task.list`） | 返回本设备最近任务 `{id,prompt,status,agent_id,created_at}`（§K2 投影），最多 20 条 |
| **审批** | 引擎发出确认请求时页面弹出横幅，点 **✅ 批准 / ❌ 拒绝** | 走**签名** `shannon/approval/decide` v2（`request_id:choice:timestamp` Ed25519）——r2-w2 起 RPC 面不再支持文本 y/n 代答（该识别保留在钉钉等 IM 适配器内）；300 秒无响应按拒绝处理 |
| **进度推送** | 无需操作，自动收到 | `query.started` → `task.progress`（增量文本/工具/usage）→ 终态 `task.message`（失败为 `query.failed`），全部 `shannon/event` 实时推到手机 |

任务列表为网关内存日志（进程生命周期内），重启后清空——这是刻意的最小只读面。

## 4. 任务语义与执行基线（v1）

- 派发创建的是**普通会话任务**：引擎 WS 查询协议只有 `{prompt, model, session_id}`
  （见 `crates/shannon-api-protocol`），goal 循环未上协议；任务标题取文本前 30 字。
- 执行 profile：引擎 WS 查询协议**没有 profile 字段**，api_server 生效的是引擎默认基线
  `ApprovalMode::AutoEdit`——**它比 balanced 更宽松：文件写入会被自动批准**
  （balanced 的定义是「读自动批准，写/bash/删除询问」，见 `shannon-engine/src/permission_profile.rs:93`）。
  真正的 Balanced profile 需要引擎 WS 协议增加 profile 字段，已登记后续项 **P1-4b**。
  对手机这类单人私用场景风险可控；如需更严基线，请先在桌面端收紧审批配置再使用。
- 派发动作本身就是触发器：与 IM 群聊不同，无需 @提及或 `/shannon` 前缀
  （`shannon/task.dispatch` 按私聊语义直入管线）。

## 5. 安全边界

- **配对**：75 秒一次性令牌（`~/.shannon/mobile-pair-tokens.jsonl`，消费即失效，重放无效）
  + Ed25519 possession 证明；配对失败统一返回同一错误，不构成令牌枚举预言机。
- **通道**：LAN 直连为网关既有 WS 通道；经 shannon-relay 中转时为 AES-256-GCM 端到端加密
  （X25519/E2E，本指南的浏览器页面 v1 只支持 LAN 直连，见 §7）。
- **门禁**：`shannon/task.dispatch` / `shannon/task.list` / `shannon/query` / `shannon/cancel` /
  `shannon/approval/decide` 一律要求已配对会话（`PAIRING_REQUIRED`），未配对连接被直接拒绝。
- **审批**：审批决定来自**已配对设备的已认证连接**（配对/重连均验签）；浏览器页与原生手机的
  审批决定均走逐决策 Ed25519 签名的 `shannon/approval/decide`（v2 防重放时间窗），
  文本 Y/N 代答仅保留在钉钉等 IM 适配器管线内（r2-w2 起不再暴露在 RPC 面）。
- **无新明文落盘点**：设备公钥存 `~/.shannon/mobile-devices.json`（公钥非机密，F14），
  私钥只在手机本机，配置文件不新增任何密钥字段。

## 6. 手动验收清单（浏览器模拟手机）

环境：桌面 + 网关运行中。**本机验收用 `http://127.0.0.1:33430/` 开箱即用**；
两部设备（真手机）需先按 §2.1 把 `mobile.host` 改为可路由地址并重启网关。
另取一台同 Wi-Fi 设备（或桌面本机浏览器）。

1. **配对**
   - [ ] 桌面「移动派发」卡通道状态显示 **运行中**；
   - [ ] 生成配对码 → 75 秒倒计时出现，超时后提示重新生成；
   - [ ] 手机打开页面地址 → 粘贴令牌 → 配对 → 桌面「已配对设备」出现该设备；
   - [ ] 刷新手机页面 → 自动 resume，无需重新配对；
   - [ ] 桌面撤销设备 → 手机端任务派发被拒（`PAIRING_REQUIRED`）。
2. **派发 + 进度**
   - [ ] 页面输入「帮我总结当前目录结构」→ 派发 → 页面先出现 🚀 已开始任务；
   - [ ] 随后出现引擎答复文本与 ✅ 任务完成；
   - [ ] 点右上角「任务」→ 列表显示该任务为已完成；
   - [ ] 再派发一条必然失败的任务（如引擎停掉后派发）→ 列表显示失败 + 原因，页面出现 ❌。
3. **审批**
   - [ ] 派发一条会触发写文件/命令确认的任务 → 页面弹出审批横幅（含工具名与危险标记）；
   - [ ] 点 **✅ 批准** → 引擎继续执行并完成；再试一次点 **❌ 拒绝** → 引擎放弃该操作；
   - [ ] 300 秒不响应 → 任务按拒绝继续（引擎侧超时拒绝）。
4. **反向隔离**
   - [ ] 未配对的另一台设备打开页面 → 任何派发/列表操作均报「需先配对设备」；
   - [ ] IM 渠道（若已配置）行为不变：群聊仍需 @/前缀，生命周期回推照旧。

## 7. v1 限制与后续项

- 网关 mobile 默认仅绑定 **127.0.0.1**（见 §2.1）——手机访问需显式改 host 并重启网关；
  配对二维码宣告 LAN IP 与默认回环绑定的不一致是先在行为，卡片已用注解消解死路。
- 浏览器页面仅支持 **LAN 直连**；shannon-relay 的 E2E 中转模式目前面向原生客户端
  （浏览器端 relay + E2E 支持后续评估）。
- 无离线缓存 / Service Worker（可「添加到主屏幕」，图标离线缓存后续）；无系统推送通知（后续登记）。
- 页面配对暂无二维码扫一扫（用粘贴令牌）；BarcodeDetector 可用时再加。
- 任务列表为内存日志，重启清空；跨进程持久化待需要时再做。
- 执行基线比 balanced 宽松（AutoEdit），Balanced profile 落地为后续项 **P1-4b**（见 §4）。

---

## 8. 跨仓 RPC 面补遗：§J / §L1 / §L2 / §M（r2-w2 起的移动协议面）

§K 任务面随「四个动作」落地（见 §3）；本节补齐同一批跨仓适配 spec（shannon-mobile
`docs/cross-repo-adaptation-spec.md` §J–§M）中其余四面的调用说明。形状以 gateway 实现
为准（`gateway/src/mobile/`）；可选键一律「引擎缺席即省略、从不造值」，手机按诚实降级渲染。

### 8.1 §J 会话面（`shannon/session.list` / `shannon/session.history`）

落点：`engineSessions.ts`（引擎帧匹配 + wire 映射）、`engineBridge.ts`（RPC 门面，
引擎 WS 一次性 call 透传）。两个方法都要求已配对会话，否则 `PAIRING_REQUIRED`。

**`shannon/session.list`** —— 请求 `{}`（v1 无参数，未知键忽略）。响应 `{sessions: [...]}`：

- 条目 `{id, agentId?, title?, updatedAt?, totalInputTokens?, totalOutputTokens?}`：
  `id` 必填，无可用 id 的条目直接跳过；`updatedAt` 为 ISO-8601 UTC（引擎给 epoch ms 时归一）；
- `agentId`/`title` 与两个 token 总量（C8 会话级花费）都是**引擎有数才上 wire**——引擎未暴露
  会话归属时 `agentId` 键省略，手机按 §J1 的跳过规则处理（不猜归属，条目不进 Chat 列表）；
- 引擎不可达映射为 `ENGINE_ERROR`（与「空列表」可区分，死引擎不冒充空花名册）。

**`shannon/session.history`** —— 请求 `{sessionId（必填，缺省 → BAD_PARAMS）, before?, limit?}`，
响应 `{sessionId, messages: [{role, content, ts?}], hasMore}`：

- 未知 sessionId **不是错误**：回 `{sessionId, messages: [], hasMore: false}`——手机把它理解为
  「服务端还没有该会话内容」，保留本地记录而不是清空线程；
- 分页是引擎侧语义、gateway 透传：`limit` 默认 50、`<1` 钳为 1；`before` 为 ISO-8601 锚点，
  锚定该 ts 在转录中**首次出现**的条目（同 ts 相邻消息视为一体，页边界不落组中间），
  返回严格早于锚点的最新 `limit` 条，`messages` 恒按时间升序；`hasMore` 表示锚点之前
  还有更早消息（手机 `loadEarlier` 以本地最早一条的 ts 作 `before` 逐页前移）；
- `ts` 可选：引擎给 epoch ms 时归一为 ISO-8601；无 `ts` 的条目手机按收到时刻处理。

### 8.2 §L1 审批富信息（`approval.request` additive 字段）

落点：`engineBridge.ts` `mapEngineEvent`（透传）、`approvalRegistry.ts` `engineAgent`/`engineRisk`（归一）。

- 六键基线对老引擎**字节不变**：`request_id` / `tool_name` / `tool_input` / `description` /
  `is_destructive` / `diff_preview`；
- 三个 additive 富键，引擎缺席即省略键、从不造值：`ts`（引擎侧 epoch ms）、
  `agent {id, name}`、`risk {scope: local|repo|system, reversible[, destructive]}`；
- 风险带合成（gateway `approvalRegistry.ts` 与手机 mapper 同规则）：`destructive || scope==system`
  → high，`!reversible` → medium，否则 low；引擎未给 `risk` 时退回 `is_destructive` 的 high/low。

### 8.3 §L2 审批恢复面（`shannon/approval.list` / `shannon/snapshot.pendingApprovals`）

落点：`approvalRegistry.ts`（进程内注册表）、`pairing.ts`（两个 RPC 入口）、
`engineBridge.ts` 与派发 hub（两个生产者）。

- 两个入口同一形状：`shannon/approval.list` → `{pendingApprovals: [...]}`；`shannon/snapshot`
  的 `pendingApprovals` 数组逐字相同（手机重连后恢复审批队列，任选一路）；
- 条目即手机 `approvalFromMap` 契约：`approvalId` / `kind` / `headline` / `risk`（合成带）/
  `timestamp`（ISO）/ `toolInput`；富键 `agentId` / `agentName` / `scope` / `diffTitle` 缺席省略；
- 注册表生命周期：两个生产者（引擎桥的 `shannon/query` 直查流 + 派发管线的 `requestApproval`），
  三个消解点（签名 `approval/decide` 成功、300 秒超时按拒绝、hub 内部 Y/N settle）；
  清理惰性——TTL 330 秒（引擎 300 秒自拒 + 30 秒结算余量）+ 200 条环形上限（最旧先出）；
- **重启语义（2026-10-03 与 mobile 对账结论）**：注册表是**进程内存态，刻意不落盘**。
  gateway 重启后 `snapshot.pendingApprovals` 为空，直到引擎下一次 `approval.request` 重新落账。
  引擎侧审批不因此丢：引擎自己在 300 秒超时拒绝，不会留下等不到答复的僵尸审批；损失窗口
  只有「重启后 → 引擎超时前」手机暂看不到该审批。注册表落盘或引擎侧重放列为评估项，
  消费级场景下暂缓，确有需要时另立项。

### 8.4 §M 设备面（`shannon/device.list` / `shannon/device.revoke`）

落点：`pairing.ts`（`toWireDeviceEntry` + 两个 handler）、`bootstrap.ts`（`device.revoked` 广播）。

- **`device.list`** → `{devices: [{deviceId, label?, pairedAt?, lastSeenAt?}]}`：camelCase 键名 +
  ISO-8601 UTC 时间戳，**永不回 `public_key`**。重塑仅发生在 RPC 层——磁盘 registry
  （`~/.shannon/mobile-devices.json`）保持与桌面 Rust 镜像一致的 snake_case 格式不动；
- **`device.revoke`**：请求键 `deviceId`（camelCase），兼容收 `device_id`（pre-§M 手机）；
  已配对会话即信任边界，可吊销**任意**已注册设备（2026-10-03 裁决——「丢失手机」正是本面的
  存在意义，二次确认由手机 UI 承担）；已配对校验不通过一律 `PAIRING_REQUIRED`；
  结果 `{revoked: <deviceId|false>, removed: bool}`——诚实 no-op 也是 success（手机对 miss
  保持列表原样，不幻吊销）；
- 吊销成功向**其他**在线设备广播 `shannon/event {type: "device.revoked", device_id}`；
  被吊销设备自身被排除在广播外，靠下一次 RPC 的 `PAIRING_REQUIRED` 察觉。

### 8.5 §O 推送面（`shannon/push.register` + wake 触发；r2 跟进批）

落点：`engineBridge.ts`（§O2 注册面）、`hub.ts` `setWake`（§O3 触发缝）、
`relay/pushRelayBinding.ts`（desktop↔relay 帧，契约见
`docs/protocol/relay-push-wake-frames.md`）、`relay/relayHost.ts`（控制帧旁路）。

- **`shannon/push.register`** —— 请求 `{enable?: bool(默认 true), platform: "fcm"|"apns",
  token: "<厂商设备 token>"}`，响应 `{ok: true, handle: "<relay 分配的随机句柄>"}`；
  `enable:false` 为注销（本地诚实 ok）。要求已配对会话（`PAIRING_REQUIRED`）；
  token 经桌面沿 relay 控制面转发（`push.bind` 帧），relay 分配 handle 并把
  `deviceId→handle→token`（加密落盘）存为绑定；
- **三态诚实降级（§O2，契约测试钉死）**：relay host 模式未开 → `NOT_IMPLEMENTED`；
  relay 已连但厂商凭据未配置（`not_configured`）→ 同 `NOT_IMPLEMENTED`（推送不可用
  单一码）；其余 relay 拒绝 → `ENGINE_ERROR`。手机对结构化错误一律渲染「推送不可用」，
  **永不 mock 成功**；
- **唤醒触发（§O3）**：派发管线在 `approval.request` 与 turn 终态
  （`query.completed`/`failed`/`cancelled`）推送时向 relay 发 `push.wake {deviceId, seq}`
  （fire-and-forget；relay 按句柄取最大 seq、10s 窗口合并，厂商推送体锁死
  `{handle, seq}` 两字段）。交互式 `shannon/query` 不触发——用户正看着手机；
- **live-sync 协同**：wake 亮屏后手机走既有 `device.resume` + live-sync
  `resume.replayed`（§O4 环形缓冲，见 8.3 的恢复面语义）收敛离线窗口事件。
  重放的**应用语义**按 mobile spec §O4 车道幂等分流裁决（mobile PR #26）执行：
  id 键控面取数据（approval 按 `request_id` 原位 upsert）、chat 内容面失效+定向重拉
  （`session_id` 路由）、usage 面跳过（`task.progress` 的 usage 变体可凭
  「有 `usage` 无 `content`」判别）。gateway 侧 wire 逐字同形、**不加重放标记**——
  分流上下文由 resume 批次本身提供（§O4 裁决：约束落在手机扇出实现，gateway 缓冲零改动）。
