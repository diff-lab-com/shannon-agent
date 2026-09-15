# 移交 shannon-mono：真机联合调试发现与处理建议（2026-09-15）

> 来源：shannon-mobile 与**真实 shannon desktop 栈**的联合调试（WP-15）。
> 环境：shannon-desktop（Rust 引擎，`127.0.0.1:33420`，版本 **0.11.0**）+ 生产 TS 网关
> （`shannon-gateway` 同源代码，读 `~/.shannon/gateway/config.json`，监听 33430）+
> Android 模拟器（release APK）。配对经 Design-D 控制通道
> （`~/.shannon/mobile-pair-tokens.jsonl`），模型凭据为 MiniMax（`~/.shannon/credentials/minimax.json`，
> profile `default`）。
> shannon-mobile 侧已处理的防御见文末「mobile 侧已做」；以下为需要 mono 处理/决策的条目。

---

## P0-1 引擎未能解析 MiniMax 风格模型的工具调用（阻塞审批全链路）

**现象**：手机发起「create a file at /tmp/shannon_demo.txt containing …」，模型在
`<think>` 中确认可用工具后输出 bash 代码块，但引擎没有产生 `tool_use` /
`approval_request`，turn 反复循环。模型自己的推理流（原样流经引擎）写道：

```
The system says my previous response contained no tool calls. But I did include
a bash code block. It seems the system is expecting me to format the tool call
differently, or there's a parsing issue. Let me try again with a clearer format.
```

同一轮重复输出同一命令三次，均未被识别为工具调用。

**影响**：工具执行与审批链路（`approval_request` → 手机签名 → `/api/approval/respond`）对该模型
完全无法触发。mobile 侧审批 UI/签名/离线队列已在此前的契约测试与真网关烟测中验证过，
瓶颈只在引擎的工具调用解析。

**建议**：
1. 排查 `shannon-engine` 对 MiniMax（`profiles.default` 当前指向的 provider）的工具调用
   解析路径：该模型把工具调用作为 markdown bash 代码块输出，而非引擎期望的原生
   tool-call 格式；需要适配（提示词强制原生格式 / 代码块解析回退 / 二者兼用）。
2. 回归标准：用该 profile 发「创建文件」类 prompt，引擎应产出 `tool_use` 事件并
   （对破坏性工具）发出 `approval_request`，手机端可完成签名审批闭环。

## P0-2 引擎把 `<think>` 推理链作为普通 text 事件下发

**现象**：MiniMax 风格 reasoning 模型把思维链以内联 `<think>…</think>` 混在
`text` 事件（网关映射为 `task.progress` 的 `content`）里下发。shannon-mobile 的聊天
气泡与 mono 自带的 PWA 页（`web/page.ts`）都会把推理链原样渲染给用户。

**mobile 已做防御**：渲染层剥离（`lib/src/live/think_filter.dart`，流式安全：
闭合块/未闭合尾块/孤立闭合标签统一处理），聊天只显示正文。

**建议（二选一，推荐 1）**：
1. 引擎为推理内容发独立事件（如 `reasoning`）或在 `text` 事件上附加元数据标记，
   让客户端自行决定展示策略（Claude App 式可折叠 thinking）；
2. 或引擎在下发前剥离 `<think>` 块（最简单，但客户端永远拿不到思考内容）。

无论哪种，mobile 的渲染层过滤器都保留作为旧引擎兼容的防御。

## P1-3 网关 argv 分发：`tsx src/index.ts --config X` 会误判子命令

**现象**：开发路径直接 `tsx src/index.ts --config <path>` 报
`unknown subcommand: --config` + usage；必须写 `tsx src/index.ts run --config <path>`
才能启动。原因：`index.ts main()` 以 `process.argv[2]` 判定子命令，裸调用（预期无子命令）
携带旗标参数时，旗标被当作子命令。

**建议**：`main()` 在判定子命令前跳过位于 argv[2] 的 `--config/--profile` 及其取值
（或：未知子命令若是 `--` 开头则回落到 runGateway）。一行级修复；同时更新 README/dev 文档
为 `tsx src/index.ts run --config …`。

## P1-4 `shannon gateway run` 的报错缺少可行动指引

**现象**：无 `shannon-gateway` 二进制的开发机上，`shannon gateway run` 报
`Error: shannon-gateway not found on PATH. Install the gateway service first.` 后退出。

**建议**：报错附上下一步（`shannon gateway install` 的前提，或开发环境
`cd gateway && pnpm build:binary` / `pnpm dev` 的替代路径），避免像本次一样需要读源码定位。

## P1-5 桌面首跑配置把 mobile.host 写成 127.0.0.1（手机不可达）

**现象**：本机 `~/.shannon/gateway/config.json` 的 `mobile.host` 为 `127.0.0.1`；
网关日志会警告 "LAN direct-connect is unreachable from phones"，但配置本身不会纠正。
cross-repo spec A8b 约定桌面写入 `0.0.0.0` 默认值（iOS 直连的硬前置）。

**建议**：桌面/CLI 的 config setup 与启动时做归一化（127.0.0.1 → 0.0.0.0 迁移或显式确认），
或至少把警告升级为桌面 UI 的可点击修复项。本次调试用模拟器（`10.0.2.2` 映射宿主 loopback）
绕过了该问题，真机会直接撞上。

## P2-6 PairTokenStore 文件模式 `issue()` 不清理过期 token

**现象**：`pairing.ts` 的 `PairTokenStore`：内存模式 `issue()` 会 prune 过期项；
文件模式只在 `consume()` 命中时顺带清理。桌面每次出 QR 都 append 一条，若 token 常无人消费
（用户取消配对），`mobile-pair-tokens.jsonl` 无限增长。

**建议**：`issue()`（文件模式）追加前顺手重写过期行（已有 tmp+rename 原子写基建，
复用 `rewriteFile` 即可）。

## P2-7 `ApprovalDecideParams.signature` 类型可选与运行时强制不一致

**现象**：`protocol.ts` 中 `signature?: string | null`（可选），而
`createMobileHandlers`（requireSession）运行时强制非空签名。类型上给了客户端
"可以不带签名"的错误暗示。

**建议**：类型改为 `signature: string`（requireSession 关闭的 P1.1b 开放模式可在
bridge 内部自行容忍缺失），或至少在 JSDoc 标注运行时强制条件。

## P2-8（可选）`task.progress` 事件可考虑携带路由键

**现状**：`task.progress` 不带 `turn_id`/`session_id`，客户端必须按
"query.started 声明 + socket 在途 turn" 的约定做关联（mobile 已实现，协议注释已写明）。
多 turn 并发或同 socket 多会话场景下该约定较脆弱。

**建议**：协议演进时给 `task.progress` 补 `turn_id`（向后兼容的增量字段）。
非必须——当前契约 mobile 已完整实现。

---

## mobile 侧已做（无需 mono 重复处理）

- 网关契约全量对齐（流式应答顺序 `{ok:true}` 后置、无路由键事件的在途 turn 关联、
  OkResult/错误码表）——`test/gateway_contract_test.dart` 12 项钉住；
- `<think>` 渲染剥离（P0-2 的客户端防御）；
- 审批列表 METHOD_NOT_FOUND 降级 + 推送订阅、`{ok:true}` 判定、签名决策；
- `tool_github_smoke` 等价的 13 项真网关烟测（`tool/gateway_smoke.dart` +
  mono 侧 `gateway/src/mobile/dev-standalone.ts`）。

## 复现环境速查

- 引擎：shannon-desktop pid（`127.0.0.1:33420`，`/api/health` → `{"status":"ok","version":"0.11.0"}`）
- 网关：`cd gateway && pnpm tsx src/index.ts run --config ~/.shannon/gateway/config.json`
- 配对 token：追加 `{"token","issuedAt","expiresAt"}` 到 `~/.shannon/mobile-pair-tokens.jsonl`
- 手机侧：release APK，手动配对粘贴
  `{"v":1,"scheme":"ws","host":"10.0.2.2","port":33430,"token":"…","exp":…}`
- 触发 P0-1 的 prompt：`create a file at /tmp/shannon_demo.txt containing the text hello from my phone`

---

## mono 处理记录（2026-09-15，同日修复）

| 条目 | 状态 | 落点 |
| --- | --- | --- |
| P0-1 | ✅ 已修 | 提示词强制原生工具格式（默认 system prompt）+ 裸 shell 代码块回退：`markdown_bash_command`（保守启发式：单块、shell 语言、块外 prose ≤200 字符）合成 `Bash` ToolUseRequest，走完整权限/审批链；开关 `markdown_tool_fallback` / `SHANNON_MARKDOWN_TOOL_FALLBACK=false` |
| P0-2 | ✅ 已修（方案 1） | 引擎加流式 `ThinkStreamSplitter`（跨 chunk 分裂安全），内联 `<think>` 路由到既有 `QueryEvent::Thinking`（桌面 `query:thinking` 已有消费者）；WS 协议 0.6.0→0.7.0 新增 `thinking` 变体（此前被丢弃）；gateway `mapEngineEvent` 暂不转发给手机（mobile 渲染层过滤器保留作旧引擎防御） |
| P1-3 | ✅ 已修 | `main()` 对 `-` 开头的 argv[2] 回落 runGateway；`tsx src/index.ts --config X` 直接可用 |
| P1-4 | ✅ 已修 | 报错附 install.sh 一键安装 + `shannon gateway install` + 开发替代（`cd gateway && pnpm build:binary` / `pnpm dev`） |
| P1-5 | ✅ 已修 | 桌面 `gateway_read_config` 读时自愈迁移 `127.0.0.1`→`0.0.0.0` 并持久化（`localhost`/`::1` 等疑似人为值不动）；service 健康探测对通配 bind 改拨 127.0.0.1 |
| P2-6 | ✅ 已修 | 文件模式 `issue()` 追加前 prune 过期/畸形行（复用 tmp+rename，无移除不重写） |
| P2-7 | ✅ 已修 | `signature: string` 必填，运行时防御式校验保留（线上字节不可信） |
| P2-8 | ✅ 已做 | `task.progress` 增加 `turn_id?`（网关在流中盖章，向后兼容）；mobile 侧可选消费 |

回归测试：`cargo test -p shannon-core --lib`（2923 ✓，含 splitter/回退启发式新用例）、`cargo test -p shannon-api-protocol`（37 ✓）、desktop connections 单测 ✓、`cd gateway && pnpm typecheck && pnpm test`（380 ✓，含 issue() prune / thinking 丢弃 / turn_id 盖章新用例）。
