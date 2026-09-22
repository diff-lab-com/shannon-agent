# Shannon Monorepo 全面审查报告

- **日期**：2026-09-22
- **审查基线**：`dev` @ `33308b76`（与 origin/dev 一致，最新）
- **审查分支 / worktree**：`review/comprehensive-audit` @ `../shannon-mono.worktrees/review`
- **方法**：6 路并行深度审查（Rust 核心链路 / 工具执行面 / CLI-TUI-MCP / Tauri 桌面端 / TS Gateway / 安全与工程化横切），全部路径与行号基于审查时的 worktree；所有 P0 结论均由主审二次代码复核确认。审查为只读，未修改任何源码。
- **严重度定义**：P0=可被利用的安全漏洞 / 数据丢失 / 必现的功能性中断；P1=重要 bug 或明显设计缺陷；P2=应改进的健壮性 / 质量问题；P3=风格与小问题。

---

## 基线验证结果（审查开始时实测）

| 检查 | 结果 | 归属 |
|---|---|---|
| `cargo check --workspace` | **失败**：`libspa 0.10.1`（链路 `shannon-desktop` → `xcap 0.9.8` → `pipewire`）编译错误 E0308/E0425/E0560/E0609 | 本机系统 libspa/pipewire 头文件版本与 crate 期望不匹配，属环境问题；但暴露 desktop 的屏幕捕获依赖为硬依赖、无文档、不可选（见 P2-E6） |
| `gateway && pnpm typecheck` | **失败**：`src/mobile/__tests__/protocolSchema.test.ts(41,23)` TS2352 | **仓库真实问题**，dev HEAD 即红（见 P1-E1） |
| `desktop/ui && pnpm lint` + `tsc --noEmit` | 通过（含 design-token 检查） | — |

---

## 总体评价

工程基本面显著高于同类项目平均水准：11,725 个 Rust 单测 + 118 个集成测试文件 + 630 个 TS 测试；CI action 钉 SHA、cargo-deny/audit；秘密走 OS keyring + 会话日志 RedactionPolicy + 出站拦截；remote(SSH/docker) 全程位置参数无注入；Tauri CSP 严格、能力最小化；文档注释密度高。

但本审查发现一条贯穿性问题：**大量"代码在、链路断 / 注释承诺、实现不符"的静默失效**——gateway 的 allowlist 整层实现了却没接线、配置字段被加载器丢弃、桌面 dialog 能力被 ACL 拒绝但前端在用、updater 公钥是占位符、`StreamingToolExecutor` 的并发/超时能力从未被引擎调用、repomap 的 mtime 快路径没实现。测试全绿掩盖了这些断链。另有 4 条安全主线上可实际利用的绕过（见 P0）。

**统计**：P0 × 7 · P1 × 16 · P2 × 25 · P3 × 22。

---

## 一、P0（每条均经代码复核确认）

### P0-1 Bash「只读自动批准」可被解释器与复合命令绕过 ⚠️ 安全
- **位置**：`crates/shannon-engine/src/permission_classifier.rs:1104-1126`
- **问题**：`is_read_only_bash_command` 只取**第一个 token** 判定（按空白和 `;` 切第一段），白名单包含任意代码执行器：`node`/`python3`/`ruby`/`curl`/`wget`/`make`/`npm/npx/yarn/pnpm`/`tee`/`gh`。`;`/`&&`/`||`/`|` 之后的内容完全不检查（已确认全文件无复合命令防护）。在 Auto/FullAuto 审批模式下零提示执行。
- **利用例**：`ls; pkill -f shannon`、`python3 -c "import os; os.system('...')"`、`echo x | tee /etc/cron.d/evil`。这正是 prompt injection 的直接落点，直接违反 SECURITY.md 最高优先级承诺。
- **建议**：白名单收缩为纯观察类命令（删解释器/curl/wget/make 系/tee）；含 `; && || |` 或换行的命令一律退出只读路径；分类器增加分段判定 + 对抗性表格测试。

### P0-2 `shannon serve --host 0.0.0.0` 无鉴权暴露完整 agent API（局域网 RCE）⚠️ 安全
- **位置**：`crates/shannon-cli/src/main.rs:2954`、`crates/shannon-server/src/lib.rs:157-164`
- **问题**：守卫只有单向约束 `if allow_nonloopback && auth_token.is_none() { bail }`——用户 `--host 0.0.0.0` 而不带 `--allow-nonloopback` 时无任何拦截；`shannon_server::run()` 直接 `TcpListener::bind((host, port))`，未走 `ShannonApiServer::validate_bind()`。token 为 None 时 auth 中间件全放行，serve 注册了含 Bash 在内的完整工具注册表。
- **建议**：改为 `if !is_loopback(host) && (!allow_nonloopback || token.is_none()) { bail }`；让 serve 复用 `validate_bind` 单一守卫；补端到端测试使 SECURITY.md 威胁模型逐条有自动化对应。

### P0-3 桌面 IPC `save_text_file` / `read_attachment(s)` 任意路径读写 ⚠️ 安全
- **位置**：`desktop/src/commands_files.rs:154`（写）、`:84`（读）
- **问题**：唯一两个无路径校验的 IPC 命令，前端可传任意绝对路径——覆盖 `~/.bashrc`/`~/.ssh/authorized_keys`，读走 `~/.ssh/id_rsa`/`~/.shannon/providers.toml`（API key）。同文件的 `apply_diff`、`send_message` 附件路径都已走 `resolve_path_in_working_dir`（注释明说威胁模型是"被攻陷的前端"），这两个命令漏网。写命令还带 `create_dir_all`。
- **建议**：两个入口统一走 `resolve_path_in_working_dir`；导出类场景由后端持有 dialog 返回的合法路径；加一个 CI lint：扫描所有含路径参数的 `#[tauri::command]`，未调用校验 helper 即 fail。

### P0-4 REPL：流式查询期间权限审批请求永不显示 → 引擎无限等待（ASK 模式必现挂起）
- **位置**：`crates/shannon-ui/src/repl/query.rs:829-1278` + `crates/shannon-ui/src/repl/mod.rs:1666`
- **问题**：唯一 drain `permission_req_rx` 的代码在主循环且条件为 `!streaming_active`；`handle_query` 的轮询循环完全不读 permission channel，而此时主循环没在运行。ASK/EDIT 模式下任何需审批的工具调用 → 永久 spinner，唯一出路是 Esc 中断 → 触发 P0-5。
- **建议**：在 `handle_query` 轮询循环内 drain permission 请求并渲染对话框（流式键位处理循环已存在可复用）；补「查询中弹审批框」集成测试。

### P0-5 REPL：Ctrl-C/ESC 中断查询后 QueryEngine 永久丢失，REPL 报废
- **位置**：`crates/shannon-ui/src/repl/query.rs:1121,1336,1565-1573`（复核确认：cancel 分支返回 `Err((None, ...))`，恢复分支 `if let Some(engine)` 永不命中）
- **问题**：引擎被 `take()` move 进后台任务，`abort()` 后引擎随 future 被 drop，没有任何路径放回 `repl.query_engine`。下一次查询显示 "Query engine unavailable. Please restart."，本回合累积的上下文全部丢失。tests.rs 注释表明这是已知历史 bug 的残留路径（只修了 Failed/StreamError，未修 abort）。
- **建议**：与 P0-4 一并重构 `handle_query` 所有权：引擎放 `Arc` 或经 channel 归还，取消改协作式 `CancellationToken`，保证任何退出路径引擎都归还。

### P0-6 gateway 配置加载器静默丢弃 `engine.authTokenKey`、`mobile.tls/relay/qrPayloadFile`
- **位置**：`gateway/src/config/loader.ts:104-106,165`（审查 agent 以 tsx 实跑验证：输入含这些字段的配置，输出全部丢失）
- **问题**：`validateConfig` 白名单重建对象时漏拷。后果链：引擎 bearer 认证永远无法经配置启用（Rust 侧非回环 bind 强制要求 token → 所有调用 401）；`mobile.tls.enabled=true` 被丢弃后**静默回落明文 ws**，v0.12 宣称的 TLS pinning 加固形同虚设；relay 远程模式无法启用。
- **建议**：改为「深校验 + 白名单透传 + 未知字段报错」（或引入 zod），补配置字段写→读往返测试。一处修复同时恢复三个已断裂特性。

### P0-7 gateway IM 通道访问控制整层未接线——allowlist/pairing 是死代码
- **位置**：`gateway/src/bootstrap.ts:170-183`（`src/access/` 的 `AllowlistGuard`/`PairingStore` 全仓零引用，grep 确认）
- **问题**：入站消息直通引擎，默认 `dmDirect=true` 下**任何能给 bot 发私信的陌生人即可驱动 agent 执行工具**（消耗额度、读文件、发消息），审批按钮任何频道成员可点。README 明言 "DM pairing + allowlist — done"，与实现不符。
- **建议**：在 trigger 闸之前接入 `AllowlistGuard`；未配置 allowlist 时启动打 warn。

---

## 二、P1（16 项）

### 安全与权限
1. **「Always allow」三重矛盾** — `crates/shannon-engine/src/permissions.rs:902-908,1659-1675`。单条命令选 always 后，内存侧放大为**进程级、全工具全局白名单**（任何会话的任意 Bash 命令免审批）；持久化侧写入 `Bash(<前3词>:*)` 规则因 `:*` 的冒号按字面匹配**永不命中**（重启后失效）；两个 HTTP server 构造路径根本不加载持久化规则。建议以 `PermissionRuleChecker` 为唯一裁决器统一 `(tool, prefix)` 语义。
2. **桌面无人值守路径默认 `FullAuto`** — `desktop/src/commands.rs:1522`（另 inbox/goal/batch 三处）。用户全局 `confirm` 档被定时任务/goal/batch 静默绕过。建议默认继承用户 approval_mode，升级 FullAuto 需显式确认。
3. **移动配对默认 `0.0.0.0` 明文 ws** — `desktop/src/commands_mobile_pairing.rs:89`，配对 token 可被 LAN 嗅探抢配。建议默认 TLS-on 或 loopback。
4. **gateway 被吊销设备在存活连接上仍可派任务/答审批** — `gateway/src/mobile/taskHandlers.ts:39,67` 不查 `isDeviceTrusted`（engineBridge 查了），吊销防线在最高危路径不生效。
5. **install.sh 校验失败静默放行（fail-open）** — `scripts/install.sh:133-135`，且校验材料与二进制同源（CDN 被攻破则校验无效）。建议 fail-closed + 第二通道校验。

### 功能性断裂（行为与承诺不符）
6. **WS/REST 会话多轮历史静默丢失** — `crates/shannon-core/src/api_server.rs:908,995-999`、`crates/shannon-server/src/routes/mod.rs:168-199`。`process_query(&self)` 的隐式契约（宿主消费 `ConversationUpdate` 后回写）TUI 履行了、两个 HTTP server 都没履行：同一 WS 连接第二次查询不带首轮上下文，REST 每条消息都是无上下文单轮，`message_count` 恒 0。建议把回写内化到引擎（producer 结束时统一写回 `self.conversation`），加「同 engine 二次查询必须携带首轮上下文」集成测试。
7. **Bash 流式路径无超时、取消永远观察不到** — `crates/shannon-tools/src/system.rs:1596-1717`。`sleep infinity` 类命令永久挂死；远程 world（SSH/Docker）恒走此路径，与工具描述 "defaults to 120000" 矛盾。SSH 捕获路径同样无超时且超时后远端进程继续跑（`crates/shannon-remote/src/ssh/session.rs:121-129`）。
8. **RunBackground 不杀旧进程、KillBackground 不发信号却报告成功** — `crates/shannon-tools/src/background.rs:218-229,700-733`（句柄从未保存，`let _ = prev`），旧进程成永久孤儿而模型被告知端口已释放。
9. **REPL `!` 内联 shell 在 UI 线程同步执行且无超时** — `crates/shannon-ui/src/repl/commands/mod.rs:174-210`，raw mode 下不可取消，`!sleep infinity` 冻结整个 TUI。statusline（`repl/helpers.rs:340-351` 先 `wait()` 后读 stdout，超 64KB 死锁）与 skills ShellExecutor 同病。
10. **导出的 `McpClient` 请求 ID 不匹配：所有请求必超时** — `crates/shannon-mcp/src/client.rs:310-317,403`（外部 id 注册、线上请求另生成内部 id，响应永远路由失败）。生产走 process_pool 未踩中，但这是公开 API。同 crate：`SseTransport` 按 chunk 逐行解析 SSE（`transport.rs:412-443`），大结果分片即静默丢数据。
11. **桌面 dialog 能力被 ACL 拒绝但前端 7 处在用** — `desktop/tests/session_window_acl.rs:56` 固化拒绝，`ChatInput.tsx`/`sessionActions.ts`/`ArtifactPanel.tsx` 等调用必抛错：真实桌面构建里附件、导出、artifact 保存全部不可用（mock/E2E 被拦截所以测不出）。需新增 `capabilities/file-dialogs.json`。
12. **updater 公钥是占位符** — `desktop/tauri.conf.json:77`，启动即 `check()` 且失败被 info 日志吞掉，托盘「检查更新」永远无效，发布链形同虚设。
13. **gateway 引擎 WS 无重连 + lane 永久中毒 + 错误静默** — `gateway/src/router/lane.ts:29-39`、`wsClient.ts:208-217`。引擎一次抖动 = 该会话永久黑洞（实测验证：恢复后连接尝试总数仍为 1）。所有 IM 适配器审批 Promise 无超时，被忽略的审批永久卡死该会话 lane。IM 审批回传不带引擎 bearer（`approvalTurnHandler.ts:73-78`），启用 auth 后审批全 401。
14. **gateway `run --profile` 被忽略** — `gateway/src/index.ts:75-84`，systemd unit 用 `--profile` 启动却加载默认配置，且多 profile 互相覆盖 unit。Windows 单文件入口守卫 `import.meta.url === file://argv[1]` 在 Bun/Windows 必然不等 → 静默 exit 0（`index.ts:209`）。
15. **健康监控自动重启丢失 `disallowed_tools`** — `crates/shannon-agents/src/process_manager.rs:804-818` 对照 `spawn_agent:388-390`。子代理崩溃重启后重新获得被禁工具（权限提升回归），是两份 spawn 逻辑复制漂移的证据。
16. **i18n `thinking.0~8` 所有语言都不存在** — `crates/shannon-ui/src/repl/query.rs:5-17` + `locales/en.yml:169-179`。YAML 数组不被 rust-i18n 展开为 `thinking.N`，每次思考阶段状态栏显示原始 key。另：gateway dev HEAD typecheck 红（`protocolSchema.test.ts:41` 一处 as 转换，见基线）。

---

## 三、P2（25 项，按域归组）

**核心/服务器（shannon-core / shannon-server）**
1. SessionRegistry 无限增长（每 session 常驻一台 engine，webhook 每次新建，无 TTL/上限）— `shannon-server/src/sessions.rs:19-44`
2. 工具执行无默认超时（`execution_timeout` 默认 None，聚合端点 `/api/query` 无取消通道）— `engine.rs:3890-3904`、`tools.rs:190-222`
3. 同 session 并发请求历史竞态（读-改-写同一 events.jsonl，可产生 tool_use/tool_result 失配触发 provider 400）— `api_server.rs:496-533`
4. 进程级全局 decision sink 被每个 query 覆盖，并发查询跨会话串数据 — `engine.rs:1720-1745`
5. 附件合同 8×10MiB 与 axum 默认 2MB body limit 冲突（验证常量虚假承诺）— `api_server.rs:414-420`
6. SSE wire 契约不在 protocol crate 内且两个 server 事件名映射互相矛盾（`error/event` vs 十几个具体名）— `shannon-server/src/sse.rs:4-14` vs `api_server.rs:630-660`
7. gen-ts 类型清单手工维护无防漂移守卫；`skip_serializing_if` 字段被推演为 required — `gen_ts.rs:70-86,244-252`

**工具执行面**
8. `page_text` 8000 字节切片、repomap `truncate(160)`：多字节字符跨界必 panic（中文内容必踩）— `shannon-browser/src/session.rs:482-487`、`shannon-repomap/src/parser.rs:587-593`
9. Edit 的 LCS diff 全量矩阵 O(m·n)：5 万行上限意味着最大 ~20GB 分配，1 万行已需 800MB — `shannon-tools/src/file/edit.rs:262-288`
10. 命令输出无任何字节上限（`read_to_end` 全量累积），与「harness 会截断」的工具描述矛盾 — `shannon-core/src/providers.rs:463-477`
11. DockerSandbox 把模型可控的 cwd 整个可写挂载（`cwd=/` 即根文件系统进容器）— `system.rs:775-785`
12. `find -delete`/`-exec rm` 被判 read-only/Low，可在 RunBackground 无确认批量删除 — `system.rs:485-495,302-317`
13. 子代理默认 Bash 无沙箱（Undetected posture），与主会话默认沙箱不一致 — `shannon-tools/src/agent.rs:343`
14. 系统性：`*_blocking` 文件 IO 直接跑在 tokio worker 上（Grep 遍历、SFTP 每文件一线程、undo 快照）— `grep.rs:421-469`、`ssh/fs.rs:337-380`
15. ConfigManager::save 非原子写、无 0600（崩溃留半截 JSON）；对照 `RemoteTarget::save` 已示范正确做法 — `config.rs:176-196`
16. Edit 三方合并冲突把标记直接写入用户文件后返回 error（「失败的编辑」已改文件）— `edit.rs:719-781`

**UI / 桌面**
17. 生产 CSP 使 Mermaid 预览必挂（srcdoc 继承父 CSP，只有 devCsp 放行了 CDN）；Markdown 图片被 CSP + assetProtocol 未启用双重拦截 — `MermaidRenderer.tsx:10`、`Markdown.tsx:216`、`tauri.conf.json:68`
18. 主窗口多会话并发流式文本串扰（`QUERY_TEXT` 不按 session 分桶，A/B 会话 token 混入同一缓冲）— `desktop/ui/src/context/AppContext.tsx:452`
19. `cancel_background_task` 假取消（无 CancellationToken，收尾无条件覆写回 completed）— `desktop/src/commands.rs:1664`
20. `get_file_tree` 无界同步递归跑在 async 命令里；`get_file_diff`/`get_working_dir_info` 用进程 CWD 而非会话工作目录（GUI 从 Dock 启动通常是 `/`，合法文件误报 outside workspace）— `commands_files.rs:334,208`
21. 应用级 ACL 未落地（250+ 命令对任何 webview 来源开放，与 build.rs 注释互证）；流式循环每 50ms 只处理 1 个按键且 Paste/Mouse/Resize 被丢弃 — `desktop/build.rs:44`、`repl/query.rs:1092-1093`
22. 插件在 `Repl::new()` 被完整加载两次（stdio 插件进程重复 spawn）— `repl/mod.rs:711-818 与 1204-1393`

**gateway**
23. 出站网络调用普遍无超时（WS 握手、审批 POST、平台 API、媒体下载）；五个 webhook 适配器签名校验前无上限读 body 且默认绑全部网卡 — `wsClient.ts:220-236`、`slackAdapter.ts:574-584`
24. 协议版本协商被闲置（greeting 帧直接丢弃，`PROTOCOL_VERSION` 无消费点）；Slack 顶层消息 `threadId=自身 ts` 导致每条消息各开新会话 — `wsClient.ts:190-194`、`slackAdapter.ts:196`
25. 权限请求流期间 delta 双次加锁清空竞态、失败时静默清空用户排队消息、`atomic_write_secure` 先写后 chmod（凭据短暂全局可读窗口）— `repl/query.rs:834-845,1566`、`credential_manager.rs:507-517`

---

## 四、P3（22 项，速览）

- **core/server**：nil-UUID 兜底把失败归因到 `0000…` 会话；GitHub webhook check-then-insert 双重执行窗口；GET stream 用 URL query 传 prompt（泄漏进访问日志）；序列化失败发空 SSE 载荷；engine.rs 单文件 9969 行、`agent_loop` ~4300 行嵌套 20+ 层，且 `StreamingToolExecutor`/`ToolExecutionService` 两套基础设施从未被引擎调用；事件通道 unbounded 无背压。
- **工具/agent**：browser 临时 profile 不清理、`list_tabs` 持锁做 CDP 往返；DynamicWorld 毒锁 `expect`、`block_on_anywhere` panic 升级；coordinator shutdown 不通知 process 子进程；`ProcessExited` 双路发出重复计数；repomap mtime 快路径未实现 + 全量 flush；Glob 无沙箱校验且先全量收集再截断；SubAgentRegistry check-then-insert 同名竞态；`BLOCKED_ENV` 缺 `BASH_ENV`/`NODE_OPTIONS` 等注入向量；coordinator 后台任务表重复 spawn 覆盖旧句柄。
- **UI**：`parse_multiple` 链式执行是死功能（永远只返回一条）；`parse_flags` 把正文连字符词解析为 flag；「卡流式 5 分钟强制复位」启发式中途改变回合内行为；多处硬编码英文串绕过 t!（zh.yml 缺 13 key、en 缺 1 key）；MCP 审批状态文件用相对路径随启动目录漂移。
- **commands**：`CommandExecutor::execute` 是返回占位串的 stub，敏感命令确认 no-op。
- **工程化**：CHANGELOG 三个 `[Unreleased]` 段、0.11.0 无条目（发版审计失效）；CI 主 Test job 无 timeout（17 job 仅 4 个有），coverage.yml 用浮动 tag 违反自家钉扎注释；CONTRIBUTING 引用不存在的 release-desktop.yml 与已弃用的 cargo-dist；`auto` 权限模式名三义（文档/AutoEdit/Auto-classifier）；ROADMAP 停更三个月；docs/ 与 docs-mdbook 双轨重复；根目录 AI 会话产物入库（hello.txt、notes.txt、findings.md、.playwright-mcp/、.serena/、reference/zcode/截图*.png、desktop/claudedocs/）；gateway 双锁文件并存（package-lock 陈旧）；`.gitignore` 缺 `.env.production`；desktop/ 下 deny.toml、rust-toolchain.toml、CHANGELOG.md 旧仓副本漂移；覆盖率仅 nightly 无 PR 级反馈。
- **桌面**：send_message 流任务 panic 卡死 querying 标志；`open_release_page` 未校验 URL scheme；MermaidRenderer srcdoc 未转义 `</script>`；两处 `eprintln!` 混入 tracing；乐观回滚按文本匹配可能误删。

---

## 五、系统性主题（根因分析）

1. **「代码在、链路断」的静默失效**是最大风险模式：allowlist 未接线、配置字段被丢弃、dialog capability 被拒、updater 占位符、mtime 快路径未实现、`StreamingToolExecutor` 未接线、协议版本协商闲置。共同点：组件各自有测试（全绿），但**集成链路无端到端断言**。
2. **安全边界实现不一致**：同一威胁模型（`lib.rs` 明示"被攻陷的前端"）下部分 IPC 严格校验、两个命令裸奔；权限语义内存侧/持久化侧/server 侧三处不一致；无人值守路径绕过用户审批档。
3. **隐式所有权/回写契约**：`process_query(&self)` + "宿主负责回写"的契约让 TUI 打了补丁、两个 server 漏打，直接导致多轮历史丢失；REPL 引擎 move 进后台任务导致 abort 即丢失。
4. **文档与现实的系统性漂移**：README（gateway allowlist "done"）、SECURITY.md（绑定守卫）、工具描述（超时/截断）、CONTRIBUTING（workflow 名）、CHANGELOG（结构损坏）均与实现不符。文档承诺反而放大了缺陷（用户按文档信任了不存在的防护）。

---

## 六、推荐改进方案（供审核，分三批）

### 第一批：安全止血（建议 1-2 周内，均可独立小 PR）
| # | 事项 | 验收标准 |
|---|---|---|
| 1 | P0-1 Bash 分类器：复合命令退出只读路径 + 白名单收缩 | 对抗性表格测试：`ls; <写操作>` 全量枚举全部 Denied/Ask |
| 2 | P0-2 serve 绑定守卫收敛到 `validate_bind` 单一实现 | E2E：`--host 0.0.0.0` 无 token 必 bail；有 token 必 401 |
| 3 | P0-3 桌面两个 IPC 命令接入路径校验 | 新增 CI lint：路径参数命令未调校验 helper 即 fail |
| 4 | P0-6 gateway loader 透传字段 | 配置字段写→读往返测试 |
| 5 | P1-E1 gateway typecheck 修复（一处 as 转换）+ **CI 补 gateway job**（typecheck + vitest；ci.yml 注释已承诺但实际不存在） | PR CI 覆盖 gateway，HEAD 转绿 |
| 6 | P0-4/P0-5 REPL 事件泵最小修复：`handle_query` 循环内 drain permission；abort 路径归还引擎 | 集成测试：ASK 模式查询中弹审批框；Ctrl-C 后可继续查询 |

### 第二批：结构性修复（建议 3-6 周，按依赖排序）
1. **会话回写内化到引擎**（P1-6 根因）：producer 结束（含 Failed/Cancel/abort）统一回写 `self.conversation`，`ConversationUpdate` 降级为通知事件；一个「同 engine 二次查询携带首轮上下文」的集成测试锁死。
2. **权限判定单一化**（P1-1）：`PermissionRuleChecker` 为唯一裁决器，always-allow 落为 `(tool, prefix)` 规则并即时注入 + 持久化 + server 加载；补「批准 A 不得放行 B」「重启后规则生效」回归测试。
3. **进程生命周期契约下沉**（P1-7/8 + P2-8/10 根因）：provider 层提供带 deadline 的执行（到点 kill：本地 SIGKILL / SSH 关 channel / docker 转发）；`PipedChild` 全实现保证 kill 生效；RunBackground 持久化句柄；统一 `truncate_utf8_safe` 消灭字符边界 panic；全局输出字节预算。
4. **gateway 连接生命周期**（P1-13/14）：WS 自动重连 + lane 重置 + 失败对用户可见；审批统一 300s 超时 race；出站 fetch 全部 `AbortSignal.timeout`；`run --profile` 与 systemd unit 对齐；Windows 入口守卫改 `import.meta.main`。
5. **桌面「必坏」配置修复**（P1-11/12）：dialog capability + ACL 测试同步更新；真 updater 公钥进签名流水线，占位符启动时报错；mermaid/字体本地打包，prod CSP 按实际资源清单生成并入 CI 校验；无人值守路径继承 approval_mode。
6. **SSE 协议收敛**（P2-6/7）：SSE 事件名与载荷进 `shannon-api-protocol`，两个 server 共享；gen-ts 加「清单覆盖全部导出类型」守卫 + CI diff 校验。

### 第三批：工程化收口（持续）
- CHANGELOG 自动化（发布时截断 Unreleased、CI 校验 tag↔版本段）；CONTRIBUTING 以 justfile/CI 为唯一事实源并脚本校验；清理迁移期遗留（desktop 三个副本文件、PHASE n 注释、根目录会话产物 `git rm --cached` + ignore）。
- 全部 workflow 加 `timeout-minutes` 与最小 `permissions:`；action 钉扎交给 renovate 统一维护；gateway 删除 package-lock 并声明 `packageManager`；`.gitignore` 补 `.env.*` + `!.env.example`。
- PR 级轻量覆盖率（llvm-cov 增量包）或正式放弃 nightly；desktop 屏幕捕获依赖（xcap→pipewire/libspa）改为可选 feature 或在 README 写明系统依赖版本要求（本机 `cargo check` 即因此失败）。
- engine.rs（9969 行）按 phase 拆模块；删除或接线 `StreamingToolExecutor`/`ToolExecutionService` 两套未用基础设施。

---

## 附录：值得保留的正面实践

- 秘密体系完整：keyring + 0600 + RedactionPolicy + `secret_guard` 出站拦截 + Debug 不打印 secret（仅 `atomic_write_secure` 的先写后 chmod 一处瑕疵）。
- `ShannonApiServer` 的 validate_bind + 常数时间 token 比较 + deny-by-default CORS；remote SSH/docker 全位置参数无注入；文件工具 canonicalize 防 TOCTOU。
- 测试规模与纪律（11.7k 单测、协议 round-trip、取消路径专门测试）；CI 钉 SHA + cargo-deny/audit；fixture 治理规则清晰。
- 桌面前端 XSS 防线正确（rehype-sanitize 在 highlight 之后、白名单扩展）；事件监听器均有 unlisten 清理；10 语种 i18n + parity 脚本 + axe-core e2e。

## 附录：审查覆盖与方法说明

- 覆盖：20 个 Rust crate、desktop(Rust+前端)、gateway(TS)、CI/构建/文档/供应链。`shannon-codegen`/`shannon-stability-attr` 未发现实质问题；website/ 仅工程化视角粗查。
- 局限：静态审查 + 少量只读实验（tsx 实跑、Bun 行为佐证），未运行完整测试套件（cargo check 因本机 libspa 环境问题未完成，建议在 CI 环境复核）；行号基于 `33308b76`，后续提交可能偏移。
- P0 全部经主审二次代码复核；P1 均有代码证据；P2/P3 建议修复前先复现行号。
