# desktop↔relay Push-to-Wake 帧契约（**ACCEPTED v1**，2026-10-05 评审定稿）

> 状态：**ACCEPTED（v1 基线）**——v0.1 提案经 mobile 侧评审定稿
> （mobile 仓 `docs/frame-contract-review-2026-10-05.md`：接受为 v1 基线，附六条
> 修正，本文已按其措辞逐条吸收；待决点 §6 全部钉定）。本文帧定义应**原样搬入
> relay 仓 `docs/wire-protocol.md`**（relay 接入后本文降级为指引，指向 relay 文档；
> 信封对齐 = relay 接入首动作，见 §6.1）。修正 1 的 gateway 侧接线已落地
> （`pushExpectedState.ts` + `engineBridge` unbind 腿 + §M2 级联 + `host_ready`
> 对账挂点，见 §3）。
> 手机可见面不受本文影响——§O2（`shannon/push.register`）与 §O1（payload 锁死
> `{handle, seq}`）已钉，本文只锁桌面与 relay 之间的事。

## 1. 传输与信任

- 帧走**桌面既有的 relay 控制面 WSS 连接**（桌面出站连 relay 的那条注册了 session 的
  连接），与内容 E2E 帧共路不同帧类型。JSON 文本帧，UTF-8；类型字段沿用 relay 控制面
  既有的 `t` 惯例（与 `{t: "register", …}` 同族）。
- `push.bind` / `push.unbind` / `push.wake` **只接受来自已认证桌面会话**的连接；未认证
  连接发这些帧 → relay 直接关闭连接（与既有控制面纪律一致）。设备身份免费获得：绑定
  指令来自持有该桌面全部 deviceId 的可信会话，relay 保持「哑绑定存储」（§O2 裁决）。
- **（修正 2）绑定属主作用域**：`push.bind` / `push.unbind` / `push.wake` 只受理
  **host 角色连接**；join（手机）连接发 `push.*` → relay 关闭该连接（与未认证同纪律）。
  绑定按 **`(sid, deviceId)` 记属主**；同 sid 的 host 重连（重 present authTag）不变
  属主；`push.wake` 对**非本 sid 名下**的 deviceId 应答 `accepted:false`。sid 的跨重启
  持久性（桌面侧持久化 sid + authTag）列 **relay 接入验证项**。
- **（修正 3）relay 终结义务（版本斜坡安全性质）**：relay **必须终结（consume）
  `push.*` 帧**，绝不论以何种形态转发入 E2E 内容管道；未知 `t` 的文本帧**不得进入
  E2E 转发路径**（静默忽略）。既有先例：`register` 是终结族、`e2e_hello` 是转发族——
  `push.*` 加入终结族。桌面侧「文本=控制、二进制=E2E」分路（`relayHost.ts`）是本义务
  的对偶；手机侧杂散帧免疫已实证（`relay_transport.dart::_onControl`），故本条为
  relay 单边义务。**接入验证项**：对照 relay 现网代码确认旧版本对未知文本帧的实际
  行为（列前置②接入清单首项）。
- v1 帧版本字段 `v: 1`；relay 对未知 `t` 静默忽略（渐进契约，§J–§M 同哲学）。

## 2. `push.bind`（注册/轮换二合一，承载 `shannon/push.register` 的转发）

请求（桌面 → relay）：

```json
{ "v": 1, "t": "push.bind", "id": "<req-id>",
  "deviceId": "<gateway deviceId>",
  "platform": "fcm" | "apns",
  "token": "<厂商设备推送 token>" }
```

响应（relay → 桌面，按 `id` 关联）：

```json
{ "v": 1, "t": "push.bind.ack", "id": "<同一 req-id>",
  "ok": true, "handle": "<b64url 22 字符>" }
```

失败：同形状 `"ok": false` + `"error": { "code": "<见 §5>", "message": "<人读>" }`。

语义：

- relay 校验 `deviceId` 非空、`platform` ∈ `fcm|apns`、`token` 非空且 ≤ 4096 字节，
  否则 `bad_request`；
- **bind 不做厂商校验**（FCM v1/APNs 均无注册期 token 校验 API，修正 5）——厂商侧
  接受与否在首次投递时才暴露（见 §4 GC 与 §5 `vendor_rejected`）；
- **handle 分配**：bind 成功时 relay 生成 128-bit 纯随机句柄（base64url 22 字符），
  **同一 deviceId 重复 bind（token 轮换）保留既有 handle**（稳定句柄，降低手机端
  陈旧显示面）；显式 unbind 后再 bind 分配新 handle。handle 不派生自 deviceId（§O1）；
- token 轮换 = 同形重发覆盖（§O2）；
- relay 持久化 `(sid, deviceId) → handle → {platform, token 密文}`——**绑定跨 relay
  重启存活**（与 seq 相反：seq 不落盘，手机游标是真相，§O1）；属主记 `(sid, deviceId)`
  （修正 2）；token 用 AES-256-GCM 加密落盘，密钥来源见 §6 待决点 2；密文与明文
  token 永不出现在日志。

## 3. `push.unbind`（注销，承载 `enable:false` 转发）

```json
{ "v": 1, "t": "push.unbind", "id": "<req-id>", "deviceId": "<gateway deviceId>" }
```

```json
{ "v": 1, "t": "push.unbind.ack", "id": "<同一 req-id>", "ok": true }
```

- 未绑定的 deviceId unbind → 仍 `ok: true`（诚实 no-op，§M2 同姿态）；
- unbind 同时丢弃该 handle 的待发合并窗口内容。

### 3.1 触发表（修正 1）

| 触发路径 | 动作 | 失败语义 |
|---|---|---|
| `shannon/push.register {enable:false}`（§O2 注销） | desktop → `push.unbind(deviceId)` | 尽力而为 + 期望态对账兜底（见 3.2） |
| `shannon/device.revoke`（§M2，吊销任意已配对设备） | 对被吊销 deviceId 执行 push.unbind | 同上 |
| 解绑/unpair（手机侧单方面 forget，桌面可能永不知晓） | 不可靠触发 → 由修正 4 的厂商反馈 GC 兜底 | — |

> v0.1 期 `enable:false` 是本地 no-op、§M2 revoke 不级联——与 §O2 钉定相悖
> （mono §8.5 的旧措辞「enable:false 为注销（本地诚实 ok）」已按权威序 §O 收敛改写，
> 见 `docs/integrations/mobile-dispatch.md` §8.5）。

### 3.2 期望态对账（修正 1；mono 侧已接线）

- 桌面持久化每 deviceId 的**期望态** `{enabled, platform?, token?}`（token 为最近一次
  注册值；**加密落盘**，纪律与 relay 侧一致——桌面本就是用户信任根）。落点：
  `gateway/src/mobile/relay/pushExpectedState.ts`（AES-256-GCM，密钥首启自生成 0600，
  `directE2E.ts` 同款模式）；
- **控制链路建立/重连时对账**（挂点 = `relayHost.ts` `host_ready` → `onRegistered`）：
  期望开 → `push.bind`；期望关 → `push.unbind`。对账**幂等、末态制胜**——每设备意图
  在动作时点重读，「先关后开 / 先开后关」的断链序列由最终对账收敛，**无动作队列的
  乱序竞态**（评审因此否决「待注销标记队列」方案）；
- **双保险**：用户切换即时尝试（`enable:false` 处理器转发 `push.unbind(ctx.sessionId)`，
  deviceId = 发起会话的设备 id，与 bind 同键；手机侧诚实 ok 保留）+ 对账兜底；
- **附带自愈**：relay 侧绑定丢失（密钥丢失/换库）后，链路重连对账自动重建全部绑定；
- **§M2 级联**：revoke 处理器成功路径加同款级联（抹期望态 + 尽力 unbind，链路断由
  对账兜底）——bootstrap `onDeviceRevoked` 已接线。

## 4. `push.wake`（唤醒触发，§O3）

```json
{ "v": 1, "t": "push.wake", "deviceId": "<gateway deviceId>", "seq": <live-sync 游标> }
```

受理回执（异步合并语义——ack 只代表受理，不代表厂商推送已发；**修正 6：回带
`seq`**，排障时多并发 wake 可区分）：

```json
{ "v": 1, "t": "push.wake.ack", "deviceId": "<同一 deviceId>", "accepted": true,
  "seq": <被受理的 seq> }
```

- **fire-and-forget**：桌面不等厂商出站结果（§O3「厂商通道自身重试语义之外不做应用层
  重试风暴」），也不等 ack；`accepted: false` = 该 deviceId 未绑定（正常态，桌面侧
  静默）；
- relay 合并：同 handle 的多个待发 seq 取**最大值**，**10s 频控窗口**内只更新游标不
  重发；窗口到期出站一次厂商推送，体**锁死 `{handle, seq}` 两字段**（§O1，评审后不得
  增字段）；
- wake 只会由桌面发出（事件在桌面上产生；桌面离线时无事可推，relay 无需在线态判断）；
- relay 不解析、不存储、不转发 seq 语义之外的任何内容；
- **（修正 6）已知无害注记**：网关重启 seq 回退 × relay max 合并，理论上压后一次唤醒
  ——手机游标是真相（§O1），拉取自愈，**不得当 bug 修**；
- **（修正 6）共路取舍注记**：push.* 与 E2E 帧共路为 v1 自觉取舍（零新连接、复用既有
  register 纪律），备选的独立控制连接被否——前提是修正 3 的终结义务成立；
- **（修正 4）孤儿绑定 GC（厂商反馈摘除）**：厂商投递反馈 **UNREGISTERED（FCM v1）/
  410 Unregistered（APNs）** → relay 摘除该绑定（火忘路径顺手 GC，零额外协议面）；被
  GC 后该 deviceId 的后续 wake 应答 `accepted:false`，直至手机重新注册。

## 5. 错误码（`error.code`，字符串枚举）

| code | 场景 | 桌面侧行为 |
|---|---|---|
| `bad_request` | 字段校验失败（§2） | 映射 `shannon/push.register` 的结构化错误（手机渲染「推送不可用」） |
| `unauthorized` | 帧到达于未认证连接 | 不应发生（连接会被关）；收到即告警 |
| `vendor_rejected` | **保留码**：bind 不做厂商校验（厂商无此 API，修正 5），校验语义推迟到首次投递；该码留给投递期日志 / 未来 ack 扩展 | 映射 `ENGINE_ERROR`（以实现钉定，`pushRelayBinding.ts` 错误映射表） |
| `not_configured` | relay 未配置该 platform 的厂商凭据（§O2 第三态） | 映射 `NOT_IMPLEMENTED`——与未接线 sink 同码，手机「推送不可用」单一渲染 |

> 修正 5 钉定记录：v0.1 表曾述 `vendor_rejected`「同上映射为结构化错误（推送不可用）」
> 而实现映射 `ENGINE_ERROR`，两处不一致；按实现钉定（`vendor_rejected` 不入
> `not_configured` 单码族——「已配置但被拒」与「未配置」语义不同，前者不值得伪装成
> 未接线）。

## 6. 评审待决点（2026-10-05 全部钉定，取值 = v0.1 默认值）

1. **帧外壳对齐**：自描述 JSON 帧沿用；若 relay 控制面另有统一信封，以外壳包裹、帧体
   不变——**钉定为 relay 接入首动作**（对照 `wire-protocol.md` 现状：`register` 终结
   族 / `e2e_hello` 转发族即既有惯例，大概率直接沿用）。
2. **token 落盘密钥来源**：relay 配置注入（`SHANNON_RELAY_TOKEN_KEY`，32 字节 base64；
   缺省时首启自动生成并 0600 落盘）。补注：密钥丢失 = 绑定不可读，修正 1 的对账自愈
   路径兜底（手机下次开推送重注册）；v1 单实例假设——多实例部署需外置同源密钥。
3. **绑定存储技术**：relay 既有存储选型（SQLite/文件），本文只钉性质：跨重启存活、
   密文落盘、deviceId→handle 索引、**`(sid, deviceId)` 属主**（修正 2）、**厂商反馈
   GC**（修正 4）。
4. **handle 稳定性规则**：轮换保留 handle；unbind 后再 bind 换新（换新附带防厂商长期
   关联的隐私红利，与 §O1 一致）。
5. **限频数值**：bind ≤ 10 次/分钟/会话；wake 合并窗口 10s（§O3 钉定值）；单 handle
   待发合并深度 1（只留最大 seq）。

## 7. 手机可见性（不变式）

本文所有帧对手机不可见。手机可见的仍是 §O2 的请求/响应与 §O1 的厂商推送体；本契约
任何演进不得改变 `shannon/push.register` 的 wire 形状与三态诚实降级语义（O2 渐进契约）。
修正 1 落地后手机可见面亦不变：`enable:false` 的响应仍是诚实 `{ok:true}`——变的只是
桌面侧随之发生的 relay 摘除动作。
