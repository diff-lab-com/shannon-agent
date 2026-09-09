# IM 渠道集成指南（P1-4：Telegram / Discord / Slack / 飞书 / 钉钉）

Shannon 桌面端可以通过 **shannon-gateway** 把五个 IM 平台的消息接入引擎：
在 IM 里发一条消息即可创建任务，任务的开始 / 完成 / 失败会自动回推到原会话。
微信个人号不在支持范围内（企业微信 WeCom 走独立的已有适配器，不属于本指南）。

- 入站 → 任务：`gateway/src/adapters/*`（每平台一个适配器）→ `router/trigger.ts`（触发策略）→ `router/router.ts`（按会话分车道）→ 引擎 WS（`ws://127.0.0.1:33420/api/ws`）。
- 出站回推：`router/lifecycle.ts`（🚀 已开始 / ✅ 完成 / ❌ 失败）与流式回复。
- 桌面配置入口：**设置 → Social Connections**（平台卡片、触发开关、状态点、重启网关）。

---

## 1. 前置条件

1. 桌面端已登录并启动（引擎 loopback `127.0.0.1:33420` 随桌面自动拉起）。
2. 设置 → Social Connections → **Gateway process** 卡片中启动受管网关（或已安装系统服务）。
3. 每个平台需要把凭据写入 **OS keyring**（桌面端保存卡片表单即完成；也可用
   `gateway_set_secret` 命令）。配置文件只存 keyring 的**键名**，永不存明文密钥。

## 2. 触发策略（入站 → 任务创建）

| 场景 | 默认行为 | 可配置 |
| --- | --- | --- |
| 私聊 / DM | 直接创建任务 | 每平台开关「私聊直接响应」（`dmDirect`） |
| 群聊 | 需要 @机器人 **或** 以 `/shannon` 开头 | 每平台开关「群聊需 @提及 或 /shannon」（`groupMode`） |
| 其他群聊消息 | 忽略 | `groupMode: "any"` 时全部响应 |

- 前缀可用 `options.trigger.prefix` 换成别的（如 `/sh`）。
- 平台原生的 @提及会被识别并**从提示词中剔除**（Slack `<@U…>`、Telegram
  message entities、Discord `mentions[]`、飞书 `mentions[].key`、钉钉群聊投递即 @）。
- 保存触发开关后需**重启网关**才生效（卡片会在网关运行时给出「立即重启」按钮）。

## 3. 任务语义（v1）

- IM 消息创建的是**普通会话任务**：网关经引擎 WS 只能投递
  `{prompt, model, session_id}`（见 `crates/shannon-api-protocol`），
  goal 循环（`set_goal`）目前只有进程内入口，未上协议——因此 v1 以会话任务承载，
  消息前 30 字作为回推标题。
- 执行 profile：引擎 WS 查询协议没有 profile 字段，api_server 生效的是引擎默认
  基线 `ApprovalMode::AutoEdit`——**它比 balanced 更宽松：文件写入会被自动批准**
  （balanced 的定义是「读自动批准，写/bash/删除询问」，见
  `shannon-engine/src/permission_profile.rs:93`）。真正的 Balanced profile 需要
  引擎 WS 协议增加 profile 字段，已登记为后续项 **P1-4b**。
- **敏感操作确认回 IM**：引擎的 `approval_request` 会渲染成平台卡片 /
  按钮（Telegram 内联键盘、Slack Block Kit、飞书交互卡、Discord 按钮行、
  钉钉文本「回复 allow/deny」）；300 秒无响应按拒绝处理。
- 回推：每个任务回合向原聊天推送 `🚀 已开始任务：<标题>` /
  `✅ 任务完成：<标题>` / `❌ 任务失败：<标题>+原因`；
  可用网关配置 `im.taskLifecycle: false` 关闭。

## 4. 安全基线

- **凭据只进 OS keyring**（`gateway_set_secret` / 桌面卡片表单）；
  `~/.shannon/gateway/config.json` 只记录键名映射（如 `botToken → telegram/bot-token`）。
- **Webhook 类入站全部验签**：
  - Slack：`X-Slack-Signature`（HMAC-SHA256 `v0:` 串）+ 5 分钟时间戳防重放；
  - 钉钉：`sign = base64(HMAC-SHA256(timestamp+"\n"+secret, secret))`，timing-safe 比较；
  - 飞书：可选 Encrypt Key（AES-256-CBC，PKCS7）解密事件体；
  - 企业微信：AES + SHA1 签名（该适配器为既有交付面）。
- Telegram / Discord 为**出站**连接（long polling / Gateway WebSocket），
  无需公网回调地址，也不存在 webhook 验签面。
- 桌面表单 `type=password`，只回显「已设置」，不回读明文。
- 默认执行基线比 balanced 宽松（文件写入自动批准，见 §3）：
  **对不可信的群聊来源，建议先在桌面端收紧审批基线，再启用群聊触发。**

## 5. 各平台接入步骤

### 5.1 Telegram

1. 在 Telegram 中找 **@BotFather** → `/newbot`，拿到 bot token（`123456:AA…`）。
2. 桌面设置 → Social Connections → Telegram 卡片：粘贴 token → Save
   （写入 keyring `telegram/bot-token`）→ 打开「启用」。
3. 私聊你的 bot 发消息即可建任务；群聊中先把它加群，然后 @它 或用 `/shannon` 前缀。
- 入站方式：`getUpdates` 长轮询（1 s 间隔），**无需公网地址**。

### 5.2 Discord

1. [Discord Developer Portal](https://discord.com/developers/applications) →
   New Application → Bot → Reset Token 复制 token。
2. 开启 **MESSAGE CONTENT / SERVER MEMBERS / DIRECT Messages** Intent
   （适配器 IDENTIFY 需要 `MESSAGE_CONTENT`、`GUILD_MESSAGES`、`DIRECT_MESSAGES`）。
3. 用 OAuth2 URL（`bot` scope）把 bot 拉进服务器。
4. 桌面卡片粘贴 token → Save（keyring `discord/bot-token`）→ 启用。
- 入站方式：Gateway WebSocket 出站连接，**无需公网地址**；@bot 或 `/shannon` 触发。

### 5.3 Slack

1. [api.slack.com/apps](https://api.slack.com/apps) → Create New App（From scratch）。
2. **OAuth & Permissions**：加 `chat:write`、`channels:history`、`groups:history`,
   `im:history`、`app_mentions:read`；Install to Workspace 得到
   `xoxb-…` bot token。
3. **Basic Information → App Credentials** 复制 **Signing Secret**。
4. **Event Subscriptions**：开关打开，Request URL 填
   `http://<你的主机IP>:9873/slack`（Slack 会先发 `url_verification` 挑战，
   网关自动应答）；订阅 `message.im`、`message.channels`、`message.groups`、`app_mention`。
5. **Interactivity & Shortcuts**：Request URL 同样填 `http://<主机IP>:9873/slack`
   （审批按钮回调走这里）。
6. 桌面卡片填 Bot Token + Signing Secret → Save（keyring `slack/bot-token`、
   `slack/signing-secret`）→ 启用。
- 回调监听端口默认 `9873`，路径 `/slack`（`options.webhookPort` / `options.webhookPath` 可改）。

### 5.4 飞书（Feishu / Lark）

1. [飞书开放平台](https://open.feishu.cn/app) → 创建企业自建应用，记录 **App ID**。
2. 「凭证与基础信息」复制 **App Secret**；（可选）「事件订阅」页生成
   **Encrypt Key**（启用后事件体加密传输）。
3. 权限：`im:message`（收发消息）、`im:message.group_at_msg` 等按需开通并发布版本。
4. **事件订阅**：Request URL 填 `http://<你的主机IP>:9875/feishu`
   （网关自动应答 `url_verification` 挑战）；订阅事件 `im.message.receive_v1`。
5. 桌面卡片填 App Secret（+ 可选 Encrypt Key）→ Save（keyring `feishu/app-secret`、
   `feishu/encrypt-key`）→ 启用。
6. **App ID 不进 keyring**，属非敏感配置：在 `~/.shannon/gateway/config.json`
   对应 adapter 的 `options` 里加 `"appId": "cli_xxx"`（出站发消息的
   `tenant_access_token` 需要它）。
- 回调监听端口默认 `9875`，路径 `/feishu`；交互卡片按钮用于审批确认。

### 5.5 钉钉（DingTalk）

1. 群设置 → 智能群助手 → 添加 **自定义机器人**（或开发者后台「自定义机器人」），
   安全设置选择 **加签**，复制 `SEC…` 密钥。
2. 机器人模式选「HTTP 模式 / 消息接收模式」，消息接收地址填
   `http://<你的主机IP>:9874/dingtalk`。
3. 桌面卡片粘贴加签密钥 → Save（keyring `dingtalk/robot-secret`）→ 启用。
- 入站：群内 @机器人时钉钉 POST 消息体（带 `timestamp` + `sign`，网关验签）；
  出站走消息体里的 `sessionWebhook`（约 2 小时有效，每次入站自动刷新）。
- 回调监听端口默认 `9874`，路径 `/dingtalk`。
- 审批确认是**文本回复**式（机器人按钮回调需要额外端点）：收到审批提示后
  在会话里回复 `allow` / `deny`。

## 6. 验证与排查

- 私聊 bot 发一句「你好」→ 应收到 `🚀 已开始任务：你好` 然后是回答。
- 群里**不带** @/前缀发消息 → 应被忽略（网关日志 `ignored by trigger policy`）。
- 改动凭据 / 触发开关后无效果 → Social Connections 页顶部点「立即重启」。
- 查看网关日志：受管模式下随桌面日志输出；`SHANNON_GATEWAY_CONFIG`
  默认指向 `~/.shannon/gateway/config.json`。
- 平台状态点：灰=未配置、蓝=已配置、绿=运行中、红=网关进程退出（错误）。

## 7. 已知限制（v1）

- 「测试连接」按钮为占位（网关尚无控制面 API），可向 bot 发消息代替验证。
- 任务为会话任务，不含 goal 循环；引擎协议提供 goal 注入后可升级。
- 执行 profile 不可配置：引擎 WS 协议暂无 profile 字段（Balanced 落地为后续项 P1-4b）。
- 群聊 trigger 配置目前不区分多个群；钉钉群聊投递即视为 @机器人（平台限制）。
- 微信个人号、iMessage、语音入站不在支持范围。
