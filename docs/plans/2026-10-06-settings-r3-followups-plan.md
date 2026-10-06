# 设置页 R3 后续跟进 — 实施计划(F1-F6)

- 日期: 2026-10-06
- 分支: `feat/settings-r3-followups`(基于 origin/dev @ 784b088d4,含已合并的 PR #310/#317)
- 上游: `docs/plans/2026-10-05-settings-r3-implementation-plan.md`(R3 主体,已合并)+ 其最终审查"跟进建议"
- 范围: 最终审查与 T7/T8/T11 延后项中可本地实施的全部项;**D1(更新签名基建)与 D4(界面模式实验)继续排除**;⑤ 的"真实设备验证"以 CI 原生 runner 冒烟替代(本机无法运行 macOS/Windows)。

## Global Constraints(继承 R3 计划)

同 R3:i18n 10 文件(en+zh-CN 精确);Rust expect 优先、serde default、不引新 crate;vitest 全量 0 失败 + pnpm lint;desktop crate 本地验证 = `cargo check -p shannon-desktop --no-default-features --features tauri`;所有行为变更带测试;`cargo fmt --check` 与 clippy(`-D warnings` + CI allow 集)必须干净;**rustdoc:文档注释不得链接私有项**(CI Build docs 步骤会挂)。

## 任务(5 个并发 worktree,文件所有权互斥)

### F1 — 自动归档锁范围(rust worktree `shannon-f1`)
问题(T7 延后):`desktop/src/commands_sessions.rs` `run_auto_archive_scan_with`(约 :840-895)全程持显示列表 tokio::Mutex 做逐行同步 fs I/O(curation 读 + stat + 写),大会话库每 6h 阻塞侧栏刷新数秒。
方案:三段化——① 锁内快速快照候选 id 列表(仅活跃、未归档);② **锁外**逐候选做 fs 读(curation pinned / events.jsonl mtime)+ inbox pending 判定 + 保留期判定;③ 逐个归档动作时重新短暂取锁,并**复验**(running / pinned / 已归档任一变化则跳过)。
测试:既有扫描单测全绿(适配新内部结构);新增"扫描中途变为 running 的会话不被归档"复验用例。`effective_auto_archive_days` clamp、fail-closed 语义、事件发射(session-auto-archived)不变。

### F2+F4 — ask_user session 作用域 + resolved 广播;压缩关 sweep(rust+ui worktree `shannon-f24`)
F2 问题(T8 延后):AskUserCard 单槽且 payload 无 session_id——后台会话的提问弹出在所有窗口;一个窗口回答后其他窗口卡片滞留。
方案:
- **session_id 接线**:在 send 路径既有 choke point(commands.rs,与 PreventSleepGuard 同位)注册"活跃运行会话"映射(运行开始入表、结束出表,RAII guard);`DesktopQuestionHandler` 的 `AskUserRequest` payload 增加 `session_id: Option<String>`——恰好一个活跃运行时填其 id,多运行并存或无法判定时 `None`(卡片回退为全员可见,即现状)。事件常量/payload/前端 types 同步。
- **resolved 全路径广播**:应答路径(`respond_ask_user` 成功后)与超时路径都 emit `ask-user-resolved`(payload `{request_id, timed_out: bool}`),所有窗口的卡片收到即清理(已答/超时态),消除跨窗口滞留。
- **前端作用域**:AskUserCard 监听侧按 `session_id` 匹配当前可见会话——匹配或 `None` 才显示;已答/超时清理对两路径生效。vitest 补:匹配显示/不匹配不显示/None 显示/应答广播清理/超时广播清理。
F4 问题(T6 延后):压缩关设置未传播到冷构造点——`desktop/src/commands_slash.rs` restored_engine(约 :66)与 `desktop/src/commands_memory.rs` 5 处(约 :915-1087)仍用 `with_defaults_arc`。
方案:逐点核查——用户可见会话路径改为 `with_defaults_arc_and_config(|c| c.auto_compact_enabled = …)` 同型接线;纯本地估算器(不跑阶梯)保持原样并在报告列明。测试:若可单测,断言构造点读取 toggle。

### F3 — 多窗口 chime 去重(ui worktree `shannon-f3`)
问题(T5 延后):每窗口 AppContext 各播一次完成/失败/审批提示音。
方案:`desktop/ui/src/lib/notificationChime.ts` 加去重层——① **可见性规则**:`document.visibilityState === 'visible'` 的窗口才本地播放;② 全部窗口都不可见时,事件接收窗口播放兜底;③ `BroadcastChannel('shannon-chime')` 发布 `{key}`(key = 事件名+会话+payload 时间戳),收到他窗已播记录的 key 在 2s TTL 内不重复播。保持既有门控(master/sound/事件开关/DND)不变,`playTaskChime` 签名不变或向后兼容。vitest:BroadcastChannel mock 下同 key 不双播、可见性切换行为、门控组合不受影响。

### F5 — 防休眠平台 CI 冒烟(rust+ci worktree `shannon-f5`)
问题(T3 parked):Windows/macOS 后端未经真实平台运行验证(本机仅 Linux;CI 交叉编译只证明可编译)。
方案:
- `crates/shannon-core/src/prevent_sleep/` 增加**平台门控冒烟测试**:`#[cfg(target_os = "macos")]` acquire→断言 caffeinate 子进程存在(pgrep)→release→断言消失;`#[cfg(windows)]` acquire/release 往返断言 `is_preventing_sleep` 翻转且无错误(PowerRequest API 成功即冒烟);Linux 既有测试不动。
- CI:`.github/workflows` 中 macOS/Windows 交叉平台 job(macos-latest/windows-latest 的 Cross-platform Check)增加一步 `cargo nextest run -p shannon-core prevent_sleep -- --nocapture`(native runner 上原生后端真实执行)。注意 runner 上测试可能无桌面会话——若 systemd-inhibit/Power API 在 runner 环境不可用导致必然失败,改为 `--ignored`/feature 门控并在报告说明取舍;**不得让 CI 变红**。
- 文档:prevent_sleep 模块头补平台验证矩阵说明(CI 冒烟范围 + 平台局限)。

### F6 — OffpeakWindow flake 修复(ui worktree `shannon-f6`)
问题(预存,非本分支引入):`CostEstimateHint` 350ms 定时器在组件卸载后触发,调用 mock 的 `estimateTaskCost` 抛未处理错误 → OffpeakWindow.test 偶发全量 vitest 噪音。
方案:定位 `CostEstimateHint` 组件的 setTimeout,卸载时 clearTimeout(组件修复,非测试回避);vitest 补"卸载后定时器不触发"回归用例(fake timers);全量 vitest 连续 3 次零 unhandled error。

## 执行模型

5 个并发 worktree(f1 / f24 / f3 / f5 / f6),文件所有权互斥;完成后各自任务级审查 → 依序合并回 `feat/settings-r3-followups` → 集成验证(vitest/lint/clippy/escape-hatch/rustdoc -D warnings)→ 最终审查 → PR(#base dev)→ CI 全绿合并。
