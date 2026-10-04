# desktop↔relay Push-to-Wake 帧契约（提案 v0.1，待评审钉定）

> 状态：**PROPOSAL**——spec `cross-repo-adaptation-spec.md` §O3 把 desktop↔relay 的
> RPC 帧细节留给 shannon-relay × shannon-desktop（mono）两仓钉定，本文是 mono 侧的
> 钉定底稿。评审通过后，本文帧定义应**原样搬入 relay 仓 `docs/wire-protocol.md`**，
> 本文降级为指引（指向 relay 文档）。评审待决点见 §6。
> 手机可见面不受本文影响——§O2（`shannon/push.register`）与 §O1（payload 锁死
> `{handle, seq}`）已钉，本文只锁桌面与 relay 之间的事。

## 1. 传输与信任

- 帧走**桌面既有的 relay 控制面 WSS 连接**（桌面出站连 relay 的那条注册了 session 的
  连接），与内容 E2E 帧共路不同帧类型。JSON 文本帧，UTF-8；类型字段沿用 relay 控制面
  既有的 `t` 惯例（与 `{t: "register", …}` 同族）。
- `push.bind` / `push.unbind` / `push.wake` **只接受来自已认证桌面会话**的连接；未认证
  连接发这些帧 → relay 直接关闭连接（与既有控制面纪律一致）。设备身份免费获得：绑定
  指令来自持有该桌面全部 deviceId 的可信会话，relay 保持「哑绑定存储」（§O2 裁决）。
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
- **handle 分配**：bind 成功时 relay 生成 128-bit 纯随机句柄（base64url 22 字符），
  **同一 deviceId 重复 bind（token 轮换）保留既有 handle**（稳定句柄，降低手机端
  陈旧显示面）；显式 unbind 后再 bind 分配新 handle。handle 不派生自 deviceId（§O1）；
- token 轮换 = 同形重发覆盖（§O2）；
- relay 持久化 `deviceId → handle → {platform, token 密文}`——**绑定跨 relay 重启存活**
  （与 seq 相反：seq 不落盘，手机游标是真相，§O1）；token 用 AES-256-GCM 加密落盘，
  密钥来源见 §6 待决点 2；密文与明文 token 永不出现在日志。

## 3. `push.unbind`（注销，承载 `enable:false` 转发）

```json
{ "v": 1, "t": "push.unbind", "id": "<req-id>", "deviceId": "<gateway deviceId>" }
```

```json
{ "v": 1, "t": "push.unbind.ack", "id": "<同一 req-id>", "ok": true }
```

- 未绑定的 deviceId unbind → 仍 `ok: true`（诚实 no-op，§M2 同姿态）；
- unbind 同时丢弃该 handle 的待发合并窗口内容。

## 4. `push.wake`（唤醒触发，§O3）

```json
{ "v": 1, "t": "push.wake", "deviceId": "<gateway deviceId>", "seq": <live-sync 游标> }
```

受理回执（异步合并语义——ack 只代表受理，不代表厂商推送已发）：

```json
{ "v": 1, "t": "push.wake.ack", "deviceId": "<同一 deviceId>", "accepted": true }
```

- **fire-and-forget**：桌面不等厂商出站结果（§O3「厂商通道自身重试语义之外不做应用层
  重试风暴」）；`accepted: false` = 该 deviceId 未绑定（正常态，桌面侧静默）；
- relay 合并：同 handle 的多个待发 seq 取**最大值**，**10s 频控窗口**内只更新游标不重发；
  窗口到期出站一次厂商推送，体**锁死 `{handle, seq}` 两字段**（§O1，评审后不得增字段）；
- wake 只会由桌面发出（事件在桌面上产生；桌面离线时无事可推，relay 无需在线态判断）；
- relay 不解析、不存储、不转发 seq 语义之外的任何内容。

## 5. 错误码（`error.code`，字符串枚举）

| code | 场景 | 桌面侧行为 |
|---|---|---|
| `bad_request` | 字段校验失败（§2） | 映射 `shannon/push.register` 的结构化错误（手机渲染「推送不可用」） |
| `unauthorized` | 帧到达于未认证连接 | 不应发生（连接会被关）；收到即告警 |
| `vendor_rejected` | relay 已配置厂商凭据但注册被厂商拒（token 失效/项目不匹配） | 同上映射为结构化错误 |
| `not_configured` | relay 未配置该 platform 的厂商凭据（§O2 第三态） | 同上映射为结构化错误 |

## 6. 评审待决点（评审通过即钉定，本提案的默认取值已可实施）

1. **帧外壳对齐**：本文用自描述 JSON 帧；若 relay 控制面已有统一信封（type 之外的
   公共字段），以外壳包裹、帧体不变——评审时对照 relay `wire-protocol.md` 现状定。
2. **token 落盘密钥来源**：默认 = relay 配置注入（`SHANNON_RELAY_TOKEN_KEY`，32 字节
   base64；缺省时首启自动生成并 0600 落盘）；轮换策略 v1 不做（重加密延后）。
3. **绑定存储技术**：relay 既有存储选型（SQLite/文件）——本文只钉性质（跨重启存活、
   密文落盘、deviceId→handle 索引），不钉实现。
4. **handle 稳定性规则确认**：轮换保留 handle、unbind→bind 换新（§2 默认值）。
5. **限频数值确认**：bind ≤ 10 次/分钟/会话；wake 合并窗口 10s（§O3 钉定值）；
   单 handle 待发合并深度 1（只留最大 seq）。

## 7. 手机可见性（不变式）

本文所有帧对手机不可见。手机可见的仍是 §O2 的请求/响应与 §O1 的厂商推送体；本契约
任何演进不得改变 `shannon/push.register` 的 wire 形状与三态诚实降级语义（O2 渐进契约）。
