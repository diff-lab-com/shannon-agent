# v0.12 发布 Checklist — 三端协议硬化（X25519 / TLS pinning / relay tag / scope）

> 2026-09-17。覆盖本批四仓改动的上线顺序、验收信号与回退预案。
> 原则：**relay 先发（compat）→ gateway+desktop → mobile → 翻 strict**。
> 任何一步出现下述"回退信号"，先回退再排查，不带故障上线。

## 0. 前置（发布前必须全绿）

- [ ] shannon-mono CI：gateway vitest 全套 + **desktop `cargo test`（本批改动在无 pipewire 环境无法本地编译，CI 是第一道真实编译验证）**
- [ ] shannon-relay：`cargo test`（20 通过）
- [ ] shannon-mobile：`flutter test`（重点：`tls_pinning` / `e2e_v2_golden` / `e2e_kdf_golden` / `protocol_schema_contract` / `relay_*`）
- [ ] 三仓 CHANGELOG / 版本号按各自流程 bump

## 1. 发 relay（compat 模式，零破坏）

**部署**：新镜像上线，`.env` **不加** `REQUIRE_AUTH`（默认 off）。本批 relay 变更（role 校验 / 有界队列 / per-IP 限速 / tag 钉住）对不带 tag 的老客户端全部透明。

**验收**：
- [ ] 老版本 gateway + mobile 跑一轮远程配对 + 收发消息回归
- [ ] `/metrics`：`relay_errors_total{code="bad_tag"}=0`；`relay_handshakes_rate_limited_total` 无异常增长；`relay_dropped_frames_total` 基线记录

## 2. 发 gateway + desktop（先于 mobile，硬性）

**为什么必须先于 mobile**：新 gateway 对老手机全兼容（resume 走 2-part 水印分支；无 hello 走 legacy 键控）。反过来，**新手机对老 gateway** 会发 3-part resume 签名 → 老网关验签失败 → 用户被踢回重新配对。

**内容**：relay tag 发送、X25519（`hostE2EPubKey` + `e2e_hello`）、TLS 支持（`mobile.tls.enabled` 默认关）、CLOCK_SKEW 专用错误码、scope 收口（revoke 仅自吊 / query 会话归属）、relay host 自动重连、QR 日志脱敏。

**验收**：
- [ ] 老 mobile（未升级 app）对新 gateway：配对、resume、收发全通（兼容窗口验证）
- [ ] `shannon/health` 返回 version 0.11.0+；QR 日志无 token 明文

## 3. 发 mobile

**内容**：nonce resume、e2e_hello、join 带 tag、`relayAuthTag` 持久化、TLS pinning、CLOCK_SKEW 码优先判别。

**验收**：
- [ ] 真机一轮：配对 → 杀 App 冷启 resume → 断网自愈重连（计数器续传）→ 吊销自测
- [ ] relay `/metrics`：`bad_tag=0`（tag 链路通了）

## 4. 翻 strict（全端对齐后）

- [ ] 确认线上已无老版本 gateway/mobile 在用 relay（步骤 2/3 发布覆盖率达到预期）
- [ ] relay `.env` 加 `REQUIRE_AUTH=strict`，低峰滚动重启（重启断会话：gateway 自动重连 + 手机自愈梯子即实战检验）
- [ ] 观察 24h：`bad_tag` 应为 0；任何非零 → **立即回退 off** 并排查版本覆盖

## TLS 灰度（独立于 1–4 的节奏）

1. 本版本 `mobile.tls.enabled` **默认关**；桌面 设置 → 连接 → 移动派发 卡片提供「局域网加密 (wss)」开关（写入 `mobile.tls.enabled`，指纹展示来自 `~/.shannon/mobile-tls/tls-info.json`）
2. 真机验收（需要 iPhone / Android 各一）：
   - `.local` 主机名 + 自签 wss 连通；raw-IP 场景 `rawIpOnIos` 预检文案不回归
   - 杀 App 冷启 → pinning 重连；开关关掉后回退明文 + 确认对话框
   - **证书轮换 = 重新配对**：删除 `~/.shannon/mobile-tls/` 会使所有已配手机 pin 失效（UI 文案已注明）
3. 翻默认的判据：开关放出两周内 pinning 路径无"连不上"类反馈 → `mobile.tls.enabled` 默认 true（desktop `default_mobile_config` 同步）+ 收紧 Android `usesCleartextTraffic`

## 回退预案速查

| 信号 | 动作 |
|---|---|
| relay `bad_tag` 非零 | `REQUIRE_AUTH` 翻回 off，重启 relay |
| 手机大面积 resume 失败（clockSkew/authFailed） | 检查是否 mobile 先于 gateway 上线；回滚 mobile 或加速 gateway 发布 |
| gateway 启动报 TLS 证书错误 | `mobile.tls.enabled` 置 false（证书生成失败会 fail-loud，属预期保护） |
| pinning 手机连不上自签 wss | 关闭开关回到明文 + 确认对话框；排查指纹链路（QR → info 文件） |
