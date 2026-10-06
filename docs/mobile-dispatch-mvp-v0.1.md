# 移动端派发 MVP — 需求与接线说明（v0.1 草案）

> 状态：规划稿（2026-09-18，批次 D9）。对应 `docs/improvement-plan-2026-09.md` P2-1；
> 差异化依据：ZCode 无手机派发/审批面，Shannon 的 gateway（Telegram/Discord/Slack/飞书/钉钉）
> 与事件溯源引擎已具备服务端基础。

## 1. 目标（MVP 边界）

**一句话**：用户在手机上把一个任务派发给家里的 Shannon 桌面/引擎执行，并在手机上收到进度与审批请求。

MVP 做：
1. 扫码配对（复用现有 mobile pairing：`commands_mobile_pairing.rs` 的 MobilePairToken / QR）。
2. 手机 Web 页（无原生 App）：提交一段任务文本 → 桌面引擎新会话执行。
3. 进度回传：turn 级完成通知（不做逐 token 流）。
4. 审批：permission-request 在手机上批准/拒绝（复用 gateway 审批按钮回流模式）。

MVP 不做：原生 App、逐 token 流式、语音、多任务看板。

## 2. 现有资产映射（缺口小）

| 能力 | 现状 | 缺口 |
|---|---|---|
| 配对 | `mobile_pairing` 命令 + QR token | 手机端扫码页 |
| 通道 | gateway 六渠道 + WS 到 api_server | 手机走哪条通道：建议 **gateway 新增 Web 通道**（或复用 Telegram MVP） |
| 任务下发 | `send_message` 显式 session 路由（P1-1） | 手机→桌面的新会话创建 API 暴露 |
| 审批 | `permission-request` 事件 + respondPermission | gateway 已做按钮回流；手机复用 |
| 进度 | `query:completed/failed` + turn 事件 | 手机推送（渠道内置通知即可） |
| 成本 | UsagePayload / budget | 手机展示 spentUsd（GoalRunDto 同源） |

## 3. 桌面端接线（Tauri 侧，本次未实现）

1. `mobile_pairing_token` → 返回带 token 的 URL（`https://<gateway>/m/<token>`）。
2. gateway：`/m/:token` 渲染极简派发页（表单：任务文本 + 目标工作目录（配对时绑定）+ 预算上限）。
3. gateway → 桌面 WS 下发 `mobile_dispatch` → 桌面调用 `send_message(message, targetSessionId=新会话)`，标记 `source: mobile`（SessionInfo 可加 `is_agent_run` 同款字段——wire 加法）。
4. 审批请求按 gateway 既有按钮回流路径发手机。
5. 通知内容纪律：进度文案带成本（"第 3 轮 · $0.12"），呼应"成本可见"叙事。

## 4. 非功能

- 配对 token 一次性 + 24h 过期（沿用现有 pairing 语义）。
- 手机页无密钥输入（密钥永不出机——核心叙事红线）。
- 审批超时（5 分钟无响应）= 拒绝并取消 turn（与现有 permission 超时一致）。

## 5. 验收口径

- 手机扫码 → 30 秒内完成配对并在桌面看到"已配对设备"。
- 手机提交任务 → 桌面新会话开始执行；手机收到轮次完成通知（含成本）。
- 触发需要权限的工具 → 手机收到审批卡片 → 拒绝 → 桌面 turn 取消。
