# 全面审查与加固方案 (2026-09-28)

> 五路并行审查(核心引擎/安全面/UI-CLI-Agents/服务器-gateway 边界/构建-CI-文档-i18n),
> 全部发现均经审查 agent 逐行验证(file:line 级)。本方案同时是实施计划:
> 按 Batch 分派给并行实施 agent,文件所有权互斥,最后统一验证、单 PR 合入 dev。

## 方法与基线

- 基于 `origin/dev` @ `e421c78a`,worktree `shannon-mono.worktrees/review-20260928`,分支 `review/2026-09-28`。
- 排除范围:CLAUDE.md "Known Gaps" 已列明的能力缺口;2026-09-25 memory/doc/RAG 审查已修复项(PR #116)。
- 基线状态:
  - `cargo fmt --all --check` ✅ 干净
  - `cargo clippy --workspace -- -D warnings`(CI 同口径,lib+bin)✅ 通过(desktop 因本机 libspa/pipewire 头文件不匹配需 `--no-default-features --features tauri` 等效验证,环境预存问题,CI 不受影响)
  - nextest 全量:见 PR 描述(基线数字在实施前记录)
  - 已知:**测试目标(--all-targets)有 ~200 条 clippy warning**,属既有 house style(justfile 注释明确 lib+bin 口径),不在本 PR 范围

## 发现清单

严重级:P0=数据丢失/崩溃/安全,P1=重大缺陷,P2=次要缺陷/健壮性,P3=nit。✅=本 PR 修复,⏸=列为后续项。

### A. 核心引擎与会话层

| ID | 级 | 状态 | 问题 | 位置 |
|----|----|------|------|------|
| F1 | P0 | ✅ | `/rewind` 把**移除的消息数**当作**保留的 turn 数**传给日志截断,工具密集会话 `/rewind 1` 会永久销毁 events.jsonl 大部分前缀 | `shannon-ui/src/repl/commands/session.rs:935` + `shannon-core/src/query_engine/engine/mod.rs:839` + `session_log/session_store.rs:829` |
| F2 | P1 | ✅ | `ResumableSseStream` 只认 `MessageStop` 为终帧,而 Ollama/Gemini 以带 stop_reason 的 `MessageDelta` 结束 → 每次正常完成被判为过早 EOF,**整请求重放最多 3 次**(答案重复、4x token 成本) | `shannon-engine/src/api/streaming.rs:542-580` vs `:71-78` 终帧契约 |
| F3 | P1 | ✅ | SSE 按网络 chunk 逐个 `from_utf8_lossy`,多字节字符跨 chunk 边界即变 U+FFFD → CJK/emoji 流式文本静默损坏(所有 provider) | `streaming.rs:320-321` |
| F4 | P1 | ✅ | 两条摘要式压缩路径(`do_compact` 的 `split_point`、`run_token_based` 的贪心 drop)不做 tool_use/tool_result 配对对齐;Anthropic wire 无 A13 sanitizer → 下一个请求 400 或静默丢上下文。truncate 路径已有 `safe_split_point`,摘要路径没有用 | `shannon-engine/src/compact/engine.rs:272` + `shannon-core/src/compact.rs:387-454` |
| F8 | P2 | ✅ | 会话日志重写(`truncate_to_turn`/`rewrite_with_conversation`/`delete`)绕过 writer 的 flock + 固定 tmp 名可碰撞 → 活跃 writer 的 fd 被重命名架空,后续事件静默写入已 unlink 的 inode | `session_log/session_store.rs:829-982` |
| F9 | P2 | ✅ | `rewind_conversation` 把 tool result(内部为 user role)和 P-M 合成提醒都当 turn 开头 → 截断点错位、turn_count 错、dangling tool_use | `query_engine/engine/mod.rs:817-844` |
| F11 | P2 | ✅ | 流中断重连新建的 client 丢失 request tee(会话日志无法字节级重建)、A14 idle override、reasoning_effort | `streaming.rs:635` vs `client.rs:669-674` |
| F12 | P3 | ✅ | `StateManager::update_session` 非原子 get→mutate→insert,并发丢更新 → 用 `DashMap::entry` | `shannon-engine/src/state.rs:166-178` |
| F13 | P3 | ⏸ | SessionTee 边界 fsync 在 tokio worker 上同步执行(降级正确,仅性能) | `tee.rs:756` |

### B. 安全面(工具/权限/密钥)

| ID | 级 | 状态 | 问题 | 位置 |
|----|----|------|------|------|
| F5 | P1 | ✅ | **headless 三条路径(`--prompt`/`run_headless`/team agent)完全跳过 MCP 服务器审批门**:恶意仓库 `.mcp.json` 声明任意命令,克隆后 `shannon --prompt` 即在首 token 前执行(RCE)。interactive 有 `McpApprovalManager`,headless 零调用 | `shannon-cli/src/main.rs:1606/2093/3134` vs `shannon-ui/src/repl/mod.rs:479-546` |
| F6 | P1 | ✅ | 审批状态从**项目目录** `.shannon/mcp_approvals.json` 读取并按名字短路所有策略 → 恶意仓库预置 `{"approved":["evil"]}` 即可让 interactive 用户无提示放行。信任域倒置 | `repl/mod.rs:490` + `mcp_server_approval.rs:339/543` |
| F14 | P2 | ✅ | BashTool 流式路径 `stdout_buf/stderr_buf` 无上限累积(captured 路径有 2MiB 上限)→ `yes | head -c 100G` 类命令把 agent 打成 OOM;远程 world 强制走流式 | `shannon-tools/src/system.rs:1833/1860/1887` |
| F15 | P2 | ✅ | 本机回环 `shannon serve`:无 Host/Origin 校验 → DNS rebinding 可驱动会话/读 SSE/烧 provider token(Write/Edit AutoEdit 仅限 cwd+/tmp,bash fail-closed,故 P2) | `shannon-server/src/lib.rs:101-113` + `auth.rs:33`;`api_server.rs` 同样补 |
| F16 | P2 | ✅ | macOS 通知 AppleScript 只转义引号不转义反斜杠 → 尾部 `\` 使脚本编译错乱,距真实注入只差一次"简化" | `notifier.rs:713-727` |
| F17 | P2 | ✅ | `is_read_only_tool_name` 名单快路径先于 classifier 与 destructive 检查,所有模式生效 → 插件工具注册名 `file_info` 即可在只读计划模式静默执行任意行为 | `shannon-engine/src/permissions.rs:1848-1921, 27-57` |
| F18 | P2 | ✅ | 插件 manifest `name` 无字符集/`..`/绝对路径校验,`install_from_path` 直接 join → **插件目录外任意写,uninstall 时 `remove_dir_all` 任意删**(远端安装已有 consent 门,故 P2);`load_all` 重名静默 last-wins | `shannon-core/src/plugin/registry.rs:261/183/288/423` + `validate.rs:95` |
| F19 | P3 | ✅ | `LlmClientConfig` derive `Debug` 含明文 `api_key` → 手写 Debug 掩码 | `shannon-engine/src/api/types.rs:372` |
| F20 | P3 | ✅ | 沙箱纵深防御:`check_raw_traversal` 只按 `/` 切分(Windows `\..\` 漏过描述性检查);home 边界不认 `/Users`、`/root` | `shannon-tools/src/file/sandbox.rs:664-681/771-798` |
| F21 | P3 | ✅ | Telegram bot token 嵌在 URL,transport 错误 Display 连 token 进日志 → 错误净化后再入 Notification/tracing | `shannon-remote/src/telegram.rs:114-116` |
| F22 | P3 | ✅ | Docker `path_arg` 直通(现靠调用方约束防 option 注入)→ 助手统一加 `--` 分隔 | `shannon-remote/src/docker/fs.rs:138` |
| F23 | P3 | ⏸ | SecretGuard 默认 audit-only,密钥值可进 provider/events.jsonl(有意的 CLI 默认)→ 后续做首次命中提示 opt-in | `secret_guard.rs:741` |

**已验证稳固**(供记录,不需动):SSH/Docker exec 纯 argv 组装、路径沙箱 TOCTOU 感知、webhook HMAC 常时校验、回环绑定守卫、MCP 结果 25K 截断、附件双重上限、凭证 0600、WebFetch SSRF 防(含 IPv6-mapped)。

### C. UI / CLI / Agents / Skills

| ID | 级 | 状态 | 问题 | 位置 |
|----|----|------|------|------|
| F24 | P1 | ✅ | 技能列表预算截断用**字节偏移** `String::truncate`,CJK/emoji 描述切中多字节字符即 **panic(查询主路径全 app 崩溃)**;同类 bug 在 repomap 已修,此处漏掉 | `shannon-skills/src/registry.rs:592` |
| F25 | P1 | ✅ | 技能 `` !`cmd` `` 替换用 `while` 循环对**含上一次命令 stdout 的全文**重跑正则 → 输出中再现模式即被当作命令执行(注入/死循环) | `shannon-skills/src/executor.rs:274-301` |
| F26 | P1 | ✅ | 技能 shell 执行门 `allow_shell` 恒为 true(唯一构造点传 `SkillPermissions::default()`)→ 第三方仓库 SKILL.md 内嵌命令零审批执行。修复:project 来源默认 false,user/builtin 来源保持 true + 单遍替换(与 F25 同修) | `definition.rs:269-278` + `skill_bridge.rs:86-92` |
| F27 | P1 | ✅ | teammate 自认领工作环:`complete_task` 只改内存,磁盘任务永远 `in_progress`/owned → 永不可再认领、依赖永不解锁、看板永久卡死;**executor Err 分支也标记完成** | `shannon-agents/src/teammate.rs:893-919/600-613` |
| F28 | P1 | ✅ | `/team add` 用一次性 `WorktreeManager` 建工作树即丢弃(session 表随之丢失)→ 工作树/分支永久泄漏;分支名无唯一后缀,同名 agent 二次添加必失败 | `shannon-ui/src/repl/commands/extensions.rs:1481` + `shannon-agents/src/worktree.rs:197-223` |
| F29 | P2 | ✅ | inbox 读取器对解析失败的行静默跳过后**清空文件** → 版本漂移/半行即永久丢消息 → dead-letter 保留 | `shannon-agents/src/persistence.rs:517-537` |
| F30 | P2 | ✅ | `cleanup_all` 走 `git status --porcelain`(含 untracked)判定脏 → agent 工作树几乎总有构建产物,**清理静默失败**留下孤儿;错误被 `let _ =` 吞掉 | `worktree.rs:319-337/271-316` |
| F31 | P2 | ✅ | headless NDJSON:tool input >500 字节时 `input_summary` 被截断成非法 JSON → **ToolCall/ToolUse 事件整体不发**,恰恰丢掉最重要的调用 | `shannon-cli/src/main.rs:2351-2376` |
| F32 | P2 | ✅ | `--diff-only` 在工具**执行后**读文件当 old_content → 单次编辑 diff 为空、双次编辑只显示第二段 → 改为请求侧预快照(复用 FileHistoryManager) | `main.rs:2400-2409/2733-2752` |
| F33 | P2 | ✅ | repomap 预算裁剪每 `pop()` 全量重算 token 总数(O(n²)),且每 turn 深克隆整图再从头裁剪 → 运行总数维护 | `shannon-repomap/src/budget.rs:63-81` + `repo_map_injector.rs:101-186` |
| F34 | P3 | ✅ | Ctrl+A 三重匹配死代码:readline 行首跳转与 dashboard 展开均不可达 → 保留 readline 语义,面板内用独立键展开 | `shannon-ui/src/repl/input.rs:250/406/539` |
| F35 | P3 | ✅ | headless `exit_code` 在 json 格式序列化为 snake_case 字符串、json-stream 为整数、文档承诺 0-7 → 统一序列化为 i32 | `main.rs:60-90` |
| F36 | P3 | ✅ | 命令链分割器不支持引号转义、frontmatter 结束符 `find("\n---")` 误判 `----` 行 | `shannon-commands/src/parser.rs:170-206` + `shannon-skills/src/frontmatter.rs:145-161` |
| F37 | P3 | ⏸ | 内存任务板 `get_next_task` HashMap 序(随机)且不认领 → 排序 + 原子 claim(文件路径已原子,不受影响) | `task_board.rs:477` — 小,顺手修 |
| F38 | P3 | ⏸ | headless 同时输出 CiEvent 与 OutputEvent 两套 NDJSON 词表 + 双 done 行 → 统一 envelope 是破坏性变更,列后续 | `main.rs:2364-2423` |
| F39 | P3 | ⏸ | repl 命令输出残留硬编码英文(mod.rs:491、/team help 等)→ 批量迁移到 t!() 列后续 | 多处 |

### D. 服务器 / gateway / 协议边界

| ID | 级 | 状态 | 问题 | 位置 |
|----|----|------|------|------|
| F40 | P1 | ✅ | gateway 任一 turn 失败 → 未处理 promise rejection **整个进程退出**(Node/Bun 默认),8 个 channel 全死;恰在引擎重启时最易触发 | `gateway/src/bootstrap.ts:220` + `router.ts:79-84` |
| F41 | P1 | ✅ | gateway lane `clientPromise` 失败后永久缓存 rejection → 引擎恢复后该会话仍永久黑洞(上次评审 §P1-13 声称修了,实际没修) | `gateway/src/router/lane.ts:29-39` |
| F42 | P1 | ✅ | IM 访问控制配对循环是**死路**:PairingStore 纯内存,无任何代码路径能审批/持久化 allowlist → 修复后"陌生人进不来,自己人也进不来",提示语还在骗用户去桌面端批一个不存在的码。最小修复:allowlist 持久化到 `~/.shannon/gateway/allowlist.json` + 启动时明示"无审批通道"警告 + 修提示语(完整桌面审批 RPC 列后续) | `bootstrap.ts:183` + `access/allowlist.ts:31` |
| F43 | P1 | ✅ | systemd/launchd unit 带 `--profile <p>` 启动,但 `runGateway` 从不解析 `--profile` → **headless 部署主路径静默跑错配置**(上次评审 §P1-14 未修) | `gateway/src/service/units.ts:79` vs `index.ts:74-84` |
| F44 | P2 | ✅ | 畸形引擎 WS 帧在事件回调里裸 `JSON.parse` → 进程死(同 F40 爆炸半径) | `gateway/src/engine/wsClient.ts:85-102/157` |
| F45 | P2 | ✅ | shannon-server 同会话并发请求:`try_lock` 失败即**静默跳过** ConversationUpdate 回写 → 下一轮丢上下文的错误回答,无日志无重试(core 的孪生 bug 已修,这里漏了)→ 缓存最新更新、流末重试锁 | `shannon-server/src/routes/mod.rs:194-210` |
| F46 | P3 | ⏸ | server 无 graceful shutdown/请求超时,SIGTERM 切断在途 SSE → 列后续 | `lib.rs:215/230` |
| F47 | P3 | ✅ | env token 时 bearer 中间件与 trigger 端点判定不一致 → 统一计算 effective token | `lib.rs:85/110` vs `routes/mod.rs:283` |

### E. 构建 / CI / 版本 / 文档 / i18n

| ID | 级 | 状态 | 问题 | 位置 |
|----|----|------|------|------|
| F48 | P1 | ✅ | **五处版本源失锁**:workspace=0.12.0,但 desktop/Cargo.toml、tauri.conf.json、gateway/package.json、desktop/ui/package.json、shannon-plugin-api 仍=0.11.0 → 打 `v0.12.0` tag 直接挂 release 守卫;`release-prep` 的 sed 逻辑性无法自愈 | `Cargo.toml:22` vs 五文件 |
| F49 | P1 | ✅ | `.cargo/config.toml` 的 `[profile.*]` 被 cargo **静默忽略**(profile 只认根 Cargo.toml)→ 意图的 codegen-units/debug 调优从未生效 | `.cargo/config.toml:8-14` |
| F50 | P2 | ✅ | 8 个非 en/zh locale 各缺 178 键(仅 46% 翻译);**zh 缺 12 键**(整个 /help overlay + 首屏 StatusCard 回退英文)+ 1 个孤儿键。本 PR:补齐 zh 12 键 + 清孤儿键;8 locale 完成翻译列后续 | `locales/*.yml`(en=332 键基准) |
| F51 | P2 | ✅ | **~13MB 构建产物入库**:40 个 tracked 文件含两个 6MB ELF(`tests/dogfood/fixtures/scratch-clean/target/`);`.gitignore` 只挡根 `/target/` | `.gitignore:2` + dogfood fixtures |
| F52 | P2 | ✅ | semver 基线 tag 被反复重指向而注释声称"新 TAG NAME 使缓存失效" → prefix-key 与 tag 名绑定,实际在用陈旧 baseline rustdoc 做 semver-checks → key 里加入独立日期变量 + 修注释 | `ci.yml:1026/1049` |
| F53 | P2 | ✅ | CHANGELOG 无 0.11.0/0.12.0 条目,928 行 giant [Unreleased] → 切出 v0.12.0 段(要点式) | `CHANGELOG.md:5-929` |
| F54 | P2 | ✅ | README(中英)`cargo install --git` 对 virtual workspace 不可靠 → 改 `--tag v0.12.0 --bin shannon` 或指向 install.sh | `README.md:239` |
| F55 | P2 | ✅ | rustsec-audit "pinned" 安装没带 `--version`,每次现编译无缓存 → 固定版本或换 taiki-e/install-action | `ci.yml:623-624` |
| F56 | P3 | ✅ | 根 `build.rs` 是死代码(virtual manifest 永不执行,零消费者)→ 删 | `/build.rs` |
| F57 | P3 | ✅ | coverage.yml / publish-crates.yml 缺 concurrency 组(其余 10 个 workflow 都有)→ 补齐 | 两 workflow |
| F58 | P3 | ✅ | CLAUDE.md crate 表缺 `shannon-browser`、`shannon-plugin-api` 两行 → 补 | `CLAUDE.md:30-50` |
| F59 | P3 | ✅ | `scripts/build.sh` 无 shebang;dated 报告误放 scripts/ → 补 shebang / 移动 | `scripts/` |
| F60 | P3 | ⏸ | docs/metrics.md 快照滞后(周任务 opt-in)→ 发布前 `just metrics`,列后续 | `docs/metrics.md` |

## 实施 Batch 划分(文件所有权互斥)

| Batch | 范围(F) | 拥有文件 | 验证 |
|-------|---------|----------|------|
| B1 engine | F2 F3 F4 F11 F12 F19 F22 | `shannon-engine/src/{api/*, compact/*, hooks/*, state.rs}` + `shannon-core/src/compact.rs` | nextest -p shannon-engine,shannon-core |
| B2 core/session | F1 F8 F9 F16 F20 | `shannon-core/src/{query_engine/engine/mod.rs, session_log/*, notifier.rs}` + `shannon-ui/src/repl/commands/session.rs` + `shannon-tools/src/file/sandbox.rs` | nextest -p shannon-core,shannon-ui,shannon-tools |
| B3 cli/security | F5 F6 F14 F15(server 侧另见 B5) F17 F31 F32 F35 | `shannon-cli/src/*` + `shannon-engine/src/permissions.rs` + `shannon-core/src/mcp_server_approval.rs` + `shannon-ui/src/repl/mod.rs` + `shannon-tools/src/system.rs` | nextest -p shannon-cli,shannon-engine,shannon-tools |
| B4 skills/agents | F24-30 F33 F34 F36 F37 | `shannon-skills/*` + `shannon-agents/*` + `shannon-ui/src/{skill_bridge.rs, repl/commands/extensions.rs, repl/input.rs}` + `shannon-repomap/src/budget.rs` | nextest -p shannon-skills,shannon-agents,shannon-ui,shannon-repomap |
| B5 server/plugin | F15 F18 F45 F47 F21 | `shannon-server/*` + `shannon-core/src/{plugin/*, api_server.rs}` + `shannon-remote/src/*` | nextest -p shannon-server,shannon-core,shannon-remote |
| B6 gateway | F40-44 | `gateway/src/*` | gateway pnpm test/typecheck/build |
| B7 build/i18n | F48-59 | 根 Cargo.toml/.cargo/CI/locales/README/CHANGELOG/CLAUDE.md/scripts + 五个版本文件 | cargo check 全 workspace + YAML 解析 + 版本一致性脚本 |

规则:
1. 并发 ≤ 2 个 batch(B1+B2 → B3+B4 → B5+B6 → B7),避免账户限流与 target 目录争用放大。
2. 每 batch 完成后由主线跑定向测试,**通过即单独 commit**(fault isolation),消息格式 `fix(<scope>): ...`。
3. 所有 agent 不得改动 CHANGELOG/locales/根 Cargo.toml(归 B7);不得 `git commit`。
4. 每个修复须带回归测试(参照既有测试风格;F1/F2/F3/F24 类 bug 必须有能捕获原缺陷的测试)。
5. 实施前先读 finding 指向的代码再动手:若发现 finding 描述与代码不符(已被修/误报),跳过并在报告中说明。

## 验证门槛(合并前)

1. `cargo fmt --all --check` 干净
2. `cargo clippy --workspace -- -D warnings` + desktop `--no-default-features --features tauri` 通过
3. nextest 全量(排除 desktop 默认特性;desktop 用 tauri 特性跑)0 失败,且不低于基线通过数
4. gateway:`pnpm typecheck` + `pnpm test` + `pnpm build` 通过
5. 版本五源一致;locales YAML 全部可解析;zh 与 en 键集对齐(允许 en 多 `_en` 元键)
6. F51 移除产物后 dogfood 测试仍绿

## 列为后续(不在本 PR)

- F13(fsync spawn_blocking)、F23(SecretGuard opt-in 提示)、F38(NDJSON envelope 统一,破坏性)、F39(i18n 批量迁移)、F46(graceful shutdown)、F50(8 locale 完整翻译,178 键 × 8)、F60(metrics 周刷新)、gateway 桌面端配对审批 RPC(F42 的完整形态)、repomap 裁剪结果缓存。

## PR 计划

- 分支 `review/2026-09-28` → PR 到 `dev`,标题 `fix: comprehensive review hardening (2026-09-28) — P0×1 P1×12 P2×N`。
- 仓库禁 auto-merge:CI 全绿后手动 `gh pr merge --merge`(house pattern,同 #116)。
