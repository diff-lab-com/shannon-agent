## [Unreleased]

Waves queued for the next release, newest first:

- 缓期批 1 · secret-guard unset 默认收口（R-0）+ 启动行为双开关 + 上下文峰值 + 看板金额（2026-10-09）
- N3 per-kind 类别信任（v0.2 信任协议批，2026-10-08）· 经 PR #351 直合 main，本批回灌 dev
- mobile batch B · task 富化 + 真实 agent roster + 只读预算面（2026-10-07）
- §4.14 W1-P2 · OTLP bridge + full RedactionPolicy + desktop Turn Timeline
- Approval transparency + mobile approval scope (2026-10-05 follow-up)
- Permission-mode convergence (2026-10-05)
- Hardening pass (2026-10-07 full-repo review)
- Hardening follow-up (2026-10-07, same day)

### Breaking / behavior changes

- **`secret-guard` defaults to `redact` (was `audit`)**: with no `$SHANNON_SECRET_GUARD` and no `[secret_guard] mode` configured, secret-shaped content in outbound requests is now replaced with deterministic surrogates before it reaches the provider (restored locally for tool execution and display; byte-stable, so prompt caching is unaffected). Opt out with `SHANNON_SECRET_GUARD=audit` (observe-only), `SHANNON_SECRET_GUARD=off`, or `[secret_guard] mode` in config — env beats config, and an explicit `off` always wins. A one-time notice after the first hit explains what happened and names these switches.

### 缓期批 1（2026-10-09）

缓期清单第一批落地（调研与圈选见 `docs/reviews/2026-10-09-cleanup-plan-and-deferred-research.md`）：

- **secret-guard unset 默认收口（R-0 修复）**：上方 Breaking 条目宣告的「unset → redact」在安装点存在半翻转——`init_from_env_or_config` 的真空臂仍把 (env, config) 全缺的决策安装成 `audit`，实际生效模式与文档相悖。现已在安装位对齐 redact 默认（决策提炼为 `resolve_install_mode`），钉住测试翻转为 redact（`Audit` 保留为文档化回滚值），新增 unset ⇒ redact 的行为级回归测试；显式 `audit`/`off` 与 env 优先级行为零改动。
- **启动行为双开关（设置 → 通用 → 启动）**：新 `DesktopConfig` 键 `restore_session_windows_on_launch`（默认开 = 现行为）gate 启动时的会话窗口恢复——关闭时持久化列表不动，重新打开即恢复同一组窗口；`update_check_at_launch`（默认开）在启动后非阻塞地跑一次**仅检查**的更新比对（复用现有 `check_app_update`），有新版本才弹既有的「有可用更新」提示——仅提示，绝不自动安装。
- **上下文峰值（wire additive）**：`QUERY_USAGE` 载荷新增 `context_total`——该轮解析出的上下文窗口（config 覆盖 > 在线 `num_ctx` > providers.toml 声明 > 模型注册表，与 `/context` 同源；未知端到端保持缺省，不发明），逐 turn 在 L0 `turn/end` 按最大值合并，`SessionIndex.max_context_tokens` 持久化为会话峰值（旧 `index.json` 经 serde default 兼容）。UI：状态栏/Context 面的百分比原先除以从未有生产者的 `max_tokens`（死代码）改为 `context_total`，会话侧栏新增峰值 chip（仅在有值时出现）；TS 死字段 `max_tokens` 移除。
- **任务看板金额**：`list_tasks` 投影新增 `cost_usd`——由用量台账按「产生该任务的 agent 会话」免费 join（`spent_for_session` 口径）：有台账关联即 `Some`（零花费会话 = `Some(0.0)`），手建/`<adhoc>`/无关联任务为 `None`，绝不估算。卡片金额 chip 仅在有值时渲染，tooltip 注明口径（台账归属花费，随台账轮转保留）。

### N3 per-kind 类别信任（v0.2 信任协议批，2026-10-08）

**additive 批**：全部为新增变体/新方法/新路由，旧 peer 零破坏——不带 `scope` 的旧 decide 请求字节不变；旧 mobile 收到新枚举成员按既有诚实降级路径处理。对应 mobile 侧信任开关接线（cross-repo spec §Q）。

#### wire 变更

- **engine（Rust）**：`ApprovalDecision` 增 `always_allow_kind`（外部标签结构变体，wire 形态 `{"always_allow_kind":{"kind":"<tool_name>"}}`，`Copy` 随之移除——所有消费方本就按值使用）；新 HTTP 面 `GET /api/trust/kinds`（活跃类别集，`{kinds:[{kind,granted_at}]}`，按 kind 排序）与 `POST /api/trust/revoke`（`{kind}` → `{kind,revoked}`，未知 kind 幂等返回 `revoked:false`，不 404）。`gen-ts` 生成器学会渲染混合 unit/struct 变体枚举（此前 struct 变体会被**静默丢弃**），`types.gen.ts` 已再生；同批补齐 P3-3 两个此前漏生成的类型（`ApprovalModeState`/`ApprovalModeRequest`）修复一个 main 上既有的 codegen drift 门失败。
- **gateway（TS）**：`approval.decide` params `scope` 枚举扩 `"kind"` + 可选 `kind` 参数（必填校验：trimmed 1..=128）；`scope:"kind"` 映射引擎 `always_allow_kind` 透传。新方法 `shannon/trust.list`（params `{}`，返回 `{kinds:[{kind,grantedAt}]}`，camelCase 投影，坏载荷诚实降级空列表）与 `shannon/trust.revoke`（params `{kind}` → `{ok:true}`，幂等）——**不重载 `shannon/approval.state`**（那是 session token 语义）。协议 schema（`docs/protocol/shannon-mobile-protocol.schema.json`）方法枚举、decide 定义、trust 定义与 fixture 已同步再生。能力名 **`trust.kind`**：真实 gateway 无 capabilities 广播面，检测机制沿 `usage.budget`（§P）先例——旧 gateway 对 `shannon/trust.list` 答 METHOD_NOT_FOUND 即能力缺席，mobile 据此隐藏/禁用信任开关。

#### 边界（红线与语义，与 v2.3 拍板一致）

- **只有类别级信任，无全局 always-allow**：kind = 审批 `kind` 字段（引擎侧即 `tool_name`）**精确匹配**（大小写敏感、无通配/前缀/子串语义，不发明 pattern）；kind 集合 = 引擎实产审批的工具名，开放集（随注册工具增长，如 MCP `mcp__*`），不做封闭清单。
- **撤销即时生效**：allowlist 存 `~/.shannon/trust/kinds.toml`（engine 侧；`[kinds]` 表 kind → 授予时间 epoch-ms，写穿持久化：temp+rename，文件 0600/目录 0700 照 credential store 先例）。进程内单一共享 store（`shannon_engine::trust::shared_store`），权限门每次检查现读，revoke 路由同步改内存+落盘——下一个请求立即重新询问。
- **授予与撤销都留审计痕迹**：授予走签名 decide（决策签名绑定 kind：v1 `${request_id}:${choice}:kind:<kind>`、v2 `${request_id}:${choice}:${timestamp}:kind:<kind>`，沿 P3-3 `:session` 后缀先例，防 once 决策重放成信任授予）；agent loop 的 `permission/decision` 审计行记录授予（"user granted kind trust"）与自动放行原因（`trusted_kind_allow` → "matched trusted kind \`X\`"）。kind 与待批请求的 tool 不符时**降级为 allow-once**（既不扩大信任也不否决人已批准的操作）并 warn 留痕。
- **deny/ask 规则仍最高**：全局 deny 门（所有 mode 含 bypass）与 settings 显式 deny/ask 规则压过 kind 信任；Critical 风险提示路径不受影响。revoke 后同类请求重新询问（Rust 测试覆盖：持久化、精确匹配、revoke 即时、mismatch 降级、规则优先、无 store 降级、审计行）。

#### 旧 peer 兼容性

- 旧 mobile → 新 gateway：不带 `scope` 的 decide 请求 wire 字节不变；`scope:"forever"` 等未知值照旧 BAD_PARAMS。
- 新 mobile → 旧 gateway：`scope:"kind"` 被旧校验拒绝（BAD_PARAMS "once or session"）；`trust.list/revoke` METHOD_NOT_FOUND——两者都是 mobile 的诚实降级信号，绝不静默放行。
- 新 gateway → 旧 engine：`always_allow_kind` 未知变体被旧引擎 serde 拒绝 → gateway 报 ENGINE_ERROR（诚实失败，不回落成 once 放行）。
- mobile 离线排队（Z3）重放 kind 决策与 session 决策同路径（签名含 kind + timestamp 窗口 ±5min），hub 侧 settle 只讲 allow/deny，scope 留在引擎——无需新重放逻辑。


### mobile batch B（v2.3 additive，2026-10-07）

三个向后兼容的 `shannon/*` 协议变更：全部是可选字段/新方法，引擎 Rust 侧零改动；协议 schema 的方法枚举已同步（`docs/protocol/shannon-mobile-protocol.schema.json`）。

- **B1 task 富化（§K2 additive）**：`shannon/task.dispatch` / `shannon/task.list` 的任务对象在原五键（`id`/`prompt`/`status`/`agent_id`/`created_at`）之上新增可选键 `title`（非空即带）、`finished_at`（ISO-8601，仅终态出现）、`error`（仅 `failed` 且有错误时出现）。同批把 §L1 引擎富字段（`ts`/`agent`/`risk`）对齐进派发审批链路：hub 的 `approval.request` push 事件与 approvalRegistry 记录都携带引擎值（缺失不发明——旧引擎 wire 字节不变；`ts` 仅引擎提供时上 push，记录回落 hub 时钟），派发任务的审批因此在 `shannon/approval.list` 投影（agentId/agentName/scope）与直连查询路径完全一致。
- **B0 `agent.list` 真实 roster**：从空 stub 改为读取 `~/.shannon/agents/*.toml`（新依赖 `smol-toml`；解析失败的文件跳过、目录缺失返回空数组），返回 `{id, name, role?, model?, status: "idle", activity: []}`——`id` 取 toml `name` 原文，`role`/`model` 缺省不带。`status`/`activity` 是前向占位（配置面而非进程面）；`~/.claude/agents/*.md` 与项目级目录 v1 不做。注意：元素旧形状 `{session_id, platform, active}` 是 mock 时代遗留、真实 gateway 从未产出过，本次为**有意的契约修正**（非 additive）。`shannon/agent.detail` 仍 NOT_IMPLEMENTED；`task.dispatch` 的 `agent_id` 仍恒拒（引擎无 per-agent 路由面，接受即撒谎）。
- **B0 后续（本批追加，取代上一条的「agent_id 恒拒」）——roster 判定派发 + session.list 归属富化 + 任务引擎 session 修正**：`task.dispatch` 的非空 `agent_id` 改为按 `shannon/agent.list` 同一份 roster 校验——命中即受理并记录为任务**归属**（wire `agent_id` 从恒 null 变为校验通过的 roster agent 名，未传/空仍 null，向后兼容），未命中维持 INVALID_PARAMS；**边界**：引擎无 per-agent 路由面，归属只表示"该配置 agent 名下有此任务"，turn 仍由默认引擎执行，不发明引擎 persona。`shannon/session.list` 对会话摘要预留的 `agent_id` 字段按 hub journal（task id → agent，受理即记）做网关侧填补：引擎自带值优先、journal 查不到归属的 session 不填（mobile 回落 host，现有行为）。任务 turn 的引擎 session_id 由 lane 默认 `mobile:<deviceId>`（非 UUID，引擎 WS 帧校验 `Uuid::parse_str` 直接拒帧——隐患）修正为**任务自身的 UUID**（§K3 会话键；IM/query 路径不动）。
- **B2 预算只读面**：新方法 `shannon/usage.budget`（params `{}`），返回 `{month, monthCostUsd, budgetUsd, sessionCapUsd}`——月度花费按本地时区当月 1 日 0 点聚合 `~/.shannon/usage.jsonl`（口径同 desktop `usage_governance.rs` 的 `summarize_windows`/`month_start_ms`；坏行跳过，缺文件/不可读诚实地报 0，不报错），预算读 `~/.shannon/desktop/config.json`（`monthly_budget_usd`/`monthlyBudgetUsd` 双拼写容错，缺失/非数为 null），`sessionCapUsd` v1 恒 null（per-session cap 在 session sidecar，不读）。**边界**：usage.jsonl 只含桌面引擎会话花费，gateway/mobile 侧任务花费尚未入账——`monthCostUsd` 是下界。
- **P2-9 agent 活跃状态推送（B 批后续，2026-10-08）**：带 `agent_id` 的任务受理/终态时，gateway 向发起设备推 `shannon/agent.state` 通知（mobile roster 由恒 idle 变真实运行态）：受理即 `{agent: {…roster 全量, status: "running", currentTask: <prompt>}}`，首个终态（completed/failed/cancel/abort/❌ stamp）推 `{…, status: "idle", currentTask: null}`——该 agent 名下还有排队任务则保持 running 并把 currentTask 换成下一个（不闪 idle）。形状以 mobile 消费端为唯一契约源：独立通知方法 `shannon/agent.state`（非 `shannon/event` 类型）、agent map 为 agent.list 全量形状（mobile 消费端按 id 整行替换，缺 name/role/model 会在 Fleet 页降级；`currentTask` 是 camelCase）、无 seq 不进 replay ring（Fleet 下次 bind 以 agent.list 收敛）。未带 `agent_id` 的任务不推（无归属归 host）；roster 中途删掉的 agent 跳过推送（mobile 会把未知 id 加进 roster，不发明 ghost 行）。

### Hardening follow-up (2026-10-07, same day)

- **Git tools off the runtime**: the five git tools ran blocking child spawns directly inside async `execute` — a slow `git log -p` or a wedged credential prompt held a tokio worker; `run_blocking` also left child stdin inherited, so a passphrase prompt could wait on the host TTY forever. All five now run on the blocking pool, and the provider closes child stdin (mirroring `run_async`).
- **Bounded captures**: `run_async` `read_to_end`'d output without limit before the caller's 2 MiB truncate, and the PTY reader buffer was unbounded — both retain at most 8 MiB per stream now, while still draining to EOF.
- **Mouse capture is real**: the REPL never issued `EnableMouseCapture`, so wheel scrolling — and F8's toggle — operated on a path that could never fire. Capture is enabled at startup and restored at every exit path; F8 flips the terminal mode (not just the flag); the streaming input loop routes wheel events instead of dropping them; Ctrl+E's external editor runs under a suspend/restore pair (`tui::restore_terminal_for_external`, adopted from the dead `Tui` wrapper) so clicks inside the editor no longer leak escape sequences.
- **providers.toml lock retry**: `acquire_exclusive_lock` could hit ENOENT when the lockfile's parent vanished between create and open (e.g. a redirected-HOME tempdir deleted by a parallel test) — recreate and retry once.
- **Credentials never land in /tmp**: `CredentialManager::default()` had three fallback hops ending at `/tmp/.shannon/credentials`; all production callers use the hard-erroring `new()`, and the zero-caller `default()` now degrades to a non-persisting in-memory store.
- **Small fixes**: the background-process registry prunes entries finished over an hour (was unbounded growth); `kill_agent` actually fires the exit watcher's signal channel (previously only the sender's drop woke it); MCP batch responses match numeric-string ids instead of piling them onto id 0; the webhook receiver reports its own death (`is_alive()` + a synthetic event that wakes blocked `recv()`).
- **Desktop/ui**: the dead `@assistant-ui/react` adapter layer (chat-model adapter in `src/lib/runtime` + its tests) is removed along with the dependency; `terminalEvents` stays (live).
- **Wire-type drift gate**: CI and `ci-local.sh` regenerate `gateway/src/engine/types.gen.ts` from the Rust protocol crate and require byte-identity — protocol edits without regeneration now fail loudly.

### Hardening pass (2026-10-07 full-repo review)

Six-dimension review (core correctness, security, tools/MCP, UI/CLI, TS, build/docs) with the fixes landed:

- **REPL crash (P0)**: command hot-reload and `/mcp` prompt registration used `block_in_place` + `Handle::current()` on the runtime-less event-loop thread and panicked the TUI on any `.claude/commands`/`.shannon/commands` file change. The `CommandRegistry` maps are plain `std::sync::RwLock` now — sync registration is safe on any thread.
- **Config mojibake**: the `{env:}`/`{file:}` substitution byte-scanner re-encoded every non-ASCII config string as Latin-1 on load (`配置` → `é…ç½®`). Char-safe scan + regression tests.
- **Session/key file permissions**: `events.jsonl` transcripts and the `secret_guard.key` master key are created `0600` from the first write (session dirs `0700`); the credentials dir is tightened to `0700` and `validate_file_permissions` now auto-fixes loose credential files instead of warn-only.
- **Timeout/leak sweep**: fire-and-forget HTTP hooks honor their timeout; the PTY path gets the resolved Bash timeout (a hung PTY command no longer leaks a blocking thread + live child); a failed MCP `initialize` tears down the spawned server process; the updater never falls back to a no-timeout client; webhook receiver crashes are logged instead of swallowed.
- **MCP stdio routing**: server→client requests (spec `ping` and others) no longer consume the client's pending request with the same small integer id — answered `-32601`/empty-result like the WebSocket handle; progress tokens mint via `fetch_add` (no collision between concurrent calls).
- **Agent process manager**: stdin writes happen outside the agents map lock — one stalled agent can no longer deadlock `kill_agent`/`spawn_agent` for every other agent.
- **Security gates**: RunBackground refuses outbound-network one-liners (`curl`/`wget`/`nc`/…); the command analyzer flags credential stores (`~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config/gcloud`, `~/.shannon/credentials`) as critical; `is_loopback_host` parses IP literals (a `127.evil.com` DNS name can no longer get a token-less bind).
- **Pairing-RPC TLS pinning**: the desktop's `shannon/pairing.*` calls now pin the gateway certificate by its published SHA-256 fingerprint (`~/.shannon/mobile-tls/tls-info.json` — the same value the QR carries) via a custom rustls verifier instead of accepting any cert; a mismatch fails loudly with remediation guidance, and the accept-any fallback (no pin file present) warns once. Closes the machine-in-the-middle path on the approve channel.
- **Skill `!`cmd`` hard bounds**: a hanging command (`!`sleep 3600``) parked the calling thread forever. Commands now get BashTool's 120s wall-clock budget (killed on expiry), stdout/stderr are drained on dedicated readers so a full pipe can never block the child, output is capped at 1 MiB with a truncation marker naming the true byte count, and both the REPL and desktop bridges run the executor on the blocking pool instead of a runtime worker.
- **Secret-guard audit posture**: the opt-in suggestion no longer claims audit mode writes secrets to the session log — the L0 tee masks under the same policy the guard detects with, so the message now names the real residual exposure (the provider side); a regression test locks the composition end-to-end (audit guard + secret in user message and wire body → on-disk log clean).
- **Sandbox parity for the sibling process tools (architecture)**: Bash carried the legacy argv-level wrapper (bubblewrap/Seatbelt/Docker) in its dedicated slot while PowerShell / Repl / RunBackground spawned through the RAW provider — a prompt-injected command that Bash would sandbox ran unsandboxed one tool over. `register_all_tools` now decorates the siblings with the new `LocalArgvSandbox` wrapper, which applies the same rewrite **only while the world is local** (it consults `capabilities()` per spawn, so a `/remote use` swap bypasses it automatically) and never stacks on §4.12-assembled worlds (`ToolProviders::sandbox_assembled`).
- **CLI/agent**: `--yes` is honored in `--prompt` headless mode (bypass with the root/`SHANNON_DISABLE_BYPASS` guardrails) instead of being silently ignored; `shannon-agent --workdir` is applied; `shannon "x" --prompt "y"` is now a clap conflict instead of silently dropping the positional; auto-commit git failures are logged.
- **TUI streaming**: the streaming input loop drains all pending events (typing no longer capped at ~20 keys/sec; bracketed pastes no longer dropped); the `<think>` fallback matches real tags; `/goal pause` renders its `%{max}` placeholder.
- **Gateway/desktop**: oversized mobile POST bodies get a real 413 (the socket is paused, not destroyed); pair-token comparison is timing-safe; `listen()` rejections are caught (`useBudgetGuard`, `Layout`).
- **Worktree tooling**: `ExitWorktree remove` honors `discard_changes: true` (`--force`) and deletes the `worktree/*` branch instead of leaking it.
- **Build/docs**: the three clippy gates (justfile, ci-local.sh, local-check.sh pre-push) now share CI's exact flag list; `update-readme-metrics.mjs --check` understands the README floor claims and is green again; CHANGELOG has one `[Unreleased]` block; README crate trees include `shannon-browser`; `desktop/ui/index.html.bak` untracked; `.cargo/audit.toml` gitignore whitelisted; stale `Generate Metrics`/`facade-facts` CI job references corrected.

### Approval transparency + mobile approval scope (2026-10-05 follow-up)