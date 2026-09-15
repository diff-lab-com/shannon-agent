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

## 2026-09-15 追加（journey 驱动二次联合调试的新证据与新条目）

本轮按 `docs/user-journeys-2026-09-15.md` 的 9 条旅程逐条实测（模拟器 + 真实栈 + CLI 0.11.0），
以下为 mono/desktop/CLI 侧新增/升级的条目，请一并转交。

### P0-1 升级：走 mobile 网关的会话引擎侧 tools=[]（根因级证据）

- 引擎 L0 日志 `request/header` 事件显示：
  - 桌面/CLI 创建的会话：`tools=[60+ 个完整工具目录]`；
  - 经 mobile 网关路径的会话（mobile 触发的每个 query 一个新 session id）：
    **`tools=[]`——引擎根本没给模型下发任何工具定义**。
- 后果链：模型只能以**文本形式**输出工具调用 → 引擎不解析 → 不产生 tool_use →
  永远不会触发 approval_request → 手机审批链路（J4）结构性不可达；
  模型的工具调用文本（含 MiniMax 厂商特殊 token 原文
  `]<]minimax[>[<tool_call>…<invoke name="Write">…`）直接漏进对话气泡
  （截图证据在调试记录中）。
- 附带损耗：模型在 think 中自述 "the system says my responses weren't recognized as
  final answers" → 引擎 continue-loop 反复重试，单个一问一答 turn 累计
  input_tokens 7 万–21 万；turn 常以半句话截断收尾。
  **CLI 自建会话同样出现该循环**（`shannon query "Reply with exactly: cli-ok"`
  → 19.8k tokens，think 文本自述同一句话），说明这是引擎对 MiniMax-M3 收束判定
  的通病，不是 mobile 特有。
- 修复方向（mono 侧定夺）：mobile 路径的 engine query 带上工具目录；引擎对
  MiniMax 厂商 token 形态的 tool_call 做解析；重审 "final answer 未识别" 的
  continue-loop 触发条件。

### CLI 新增 P2：`shannon trace show latest` 挂起

- `shannon 0.11.0`：`trace show latest` 必挂起（30s+ 无输出，exit 124），
  而 `trace show <完整UUID>`（含 mobile 创建的会话）秒回且内容正确
  （E9-S1：手机触发的会话在 CLI 可追溯 ✓）。
- 另：`shannon doctor` 在"桌面在跑 + 网关在跑"的真实部署下报
  "shannon-gateway not found on PATH / shannon-desktop not found"，误导排障
  （原有条目，本轮实测再确认）。

### 设计问题（低优先级，请拍板）

- 手机端断开后（Z1 清凭据），网关侧 `~/.shannon/mobile-devices.json` 的设备记录
  **不摘除**。丢机场景目前没有任何入口能吊销旧设备——建议桌面端补一个设备管理/
  吊销入口，或手机断开时携带吊销语义。
- 上游已修：`mobile.host=127.0.0.1` 现在有启动告警（原 P1-5，本轮看到
  "mobile server bound to loopback … set mobile.host to 0.0.0.0"，谢谢）。

### mobile 侧本轮新增防御（无需 mono 处理，知会）

- 多 think-block turn 只渲染最后一个 `</think>` 之后的正文
  （`lib/src/live/think_filter.dart`，7 项单测）——中间被模型自己否决的草稿不再上屏；
- 会话列表预览同样过 think 过滤器；
- 断线自愈阶梯（1s→2s→…→15s 退避自动重连，pairingRequired 即停）——
  网关重启后手机 ≤15s 无感恢复（实测通过）；
- forget/断开现在真正回到欢迎页（原来卡在 mock 数据壳，重配无入口）。

---

## mono 处理记录（2026-09-15 第二轮，journey 驱动追加条目）

| 条目 | 状态 | 落点 |
| --- | --- | --- |
| P0-1 升级：mobile/WS 路径 `tools=[]` | ✅ 已修（根因） | api_server.rs 三个 query 处理器（REST/SSE/WS）原先各自 new 空 `ToolRegistry`，现改用 server 注册表（desktop loopback 经 `with_tools` 注入完整目录）。mobile 路径模型即刻拿到全部工具 |
| P0-1 升级：MiniMax 厂商 token 文本 tool_call | ✅ 已修 | 引擎新增 `parse_text_tool_calls`（`<tool_call><invoke name=…><parameter …>`，容忍 `]<]minimax[>[` 前缀噪声），无原生调用时优先恢复；裸 bash 块降为次级回退。均过权限/审批门 |
| P0-1 升级：final-answer continue-loop | ✅ 已修 | `SHANNON_THINK_ONLY_MIN_ANSWER_CHARS` 默认 200→0：仅可见正文**空白**才 nudge。"cli-ok" 这类短真答不再触发"no final answer"循环（7k–21k token 损耗的来源）。env 可调回旧行为；截断续写上限（5 次）维持不变 |
| CLI P2：`trace show latest` 挂起 | ✅ 已修 | 新增 `SessionStore::latest_id()`（目录 mtime 排序，不解析任何日志）；`list()` 的全量解析路径保留给需要完整元数据的调用方 |
| CLI：`shannon doctor` 误报 not found | ✅ 已修 | 二进制不在 PATH 时改探测运行中的服务（网关 33430 / 引擎 33420 TCP），在跑则报 "service detected (binary not on PATH)" |
| 设计问题：设备吊销入口 | ✅ 已存在 | 桌面 设置 → 连接 → 移动调度卡 已有配对设备列表 + 逐设备吊销（确认对话框，走 `mobile_revoke_device`）；mobile 侧反馈与桌面代码现状不符，未做改动 |

回归：core lib 2926 ✓ / cli 15 ✓ / desktop check ✓ / clippy 干净。附带：release matrix 新增 AppImage 腿（通用 Linux 产物）；deb/rpm 依赖声明 + 容器门禁见 CHANGELOG「Linux packaging hardening」。
