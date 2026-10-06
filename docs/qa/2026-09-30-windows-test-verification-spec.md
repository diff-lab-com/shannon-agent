# Windows 测试验证规格（2026-09-30）

> 执行环境：Windows 10 x64（10.0.19045），Git Bash + PowerShell 5.1，仓库钉住工具链
> rustc 1.88.0，cargo-nextest 0.9.114，node 24 / pnpm 11。
> 关联：[macOS 真机验证结果（2026-09-10）](2026-09-10-macos-real-machine-qa-results.md)、
> [nightly-platform-tests.yml](../../.github/workflows/nightly-platform-tests.yml)、
> [SPEC.md §9.5 Platform Support](../SPEC.md)。
>
> **动机**：CI 的 `cross-platform` Windows 腿只跑 `cargo check --workspace
> --exclude shannon-desktop`（不跑测试）；nightly Windows 腿只覆盖
> `-p shannon-core -p shannon-tools` 且不带 features（`windows_real_machine.rs`
> 在该形态下编译为空）。平台条件化的**运行时差异**（路径分隔符、行尾、shell
> 解析、代码页、权限位、Job Object）从未在任何自动化环境中执行过。
> 本文档给出 Windows 真机上的分层验证规格、已知问题清单与本轮修复范围。

## 一、结论总览

| 层 | 内容 | 状态 |
|---|---|---|
| L1 | cargo check / clippy / fmt（全 workspace，含 desktop） | 本轮在真机执行 |
| L2 | nextest 单元/组件测试（workspace 或至少 core/tools/cli/commands/ui/engine） | 本轮在真机执行 |
| L3 | Windows 专项功能验证（shell 链、hooks、行尾、代码页、通知、后台进程、沙箱） | 本轮修复 + 验证 |
| L4 | computer-use / 桌面端 GUI / NSIS 安装包 | 规格给出，部分需人工，列为后续 |

## 二、L3 验证项与已知问题清单

按风险从高到低。每项含：现象、证据（文件:行号）、验证方法、本轮处置。

### W1（高）Bash 工具硬编码 `bash -c`，无 Git Bash 时流式路径报裸错误

- 现象：Windows 上文件沙箱无内核后端 → `use_streaming` 恒 true（`system.rs:1844`），
  Bash 工具实际走流式路径 `ProcessRequest::new("bash", ["-c", …])`（`system.rs:1859`）。
  无 Git Bash 时报 `Failed to spawn command: program not found`，没有任何指引；
  而捕获式路径（`system.rs:191-206`）有 Git Bash 安装引导，两条路径不一致。
  PTY 路径（`pty.rs:67`）与后台工具（`background.rs:239`）同样硬编码 bash。
- 验证：无 Git Bash 的 PATH 下调用 Bash 工具 → 确认错误文案；装 Git Bash 后三条路径
  （捕获/流式/PTY）各跑一条命令。
- 处置：**本轮修复**——流式/PTY/后台路径补齐与捕获路径一致的 Windows 引导文案；
  Windows 上未找到 bash 时引导改用 PowerShell 工具。

### W2（高）Hooks 全部经 `sh -c` 执行，Windows 无回退

- 现象：`shannon-engine/src/hooks/manager.rs:266/347/552` 三处
  `Command::new("sh")`。无 Git Bash 时 hook 全部失败（fire-and-forget 路径仅 warn）。
- 验证：配置一个 `PostToolUse: echo hello` hook，触发工具调用，观察执行结果。
- 处置：**本轮修复**——Windows 上 `sh` 不存在时回退 `cmd /C`（批处理/内建命令可用），
  并在错误信息中说明已使用的 shell。

### W3（高）Edit 工具无 CRLF 归一化

- 现象：`file/edit.rs:161` 用 `contains(old_string)` 精确匹配、`:760` 按原样写回。
  CRLF 文件 + 模型给出的 LF 多行 `old_string` → "old_string not found"；
  即使匹配成功，插入文本带 LF → 混合行尾。
- 验证：对 CRLF 文件做多行 Edit，`git diff --stat` / `file` 检查行尾。
- 处置：**本轮修复**——Edit/MultiEdit 读写时检测文件行尾风格，
  LF 输入按文件风格归一化后再匹配，写回保持原风格。

### W4（中高）PowerShell 工具不设 UTF-8 输出编码，中文 Windows 乱码

- 现象：`system.rs:2079` 起 PowerShell 工具未设置
  `[Console]::OutputEncoding`，管道下 PowerShell 输出 OEM 代码页
  （zh-CN 为 CP936/GBK），而读取端一律 `String::from_utf8_lossy`（`system.rs:175`）。
- 验证：zh-CN 环境执行 `Write-Output '中文测试'`、`Get-ChildItem`（含中文文件名）。
- 处置：**本轮修复**——spawn 的 PowerShell 命令前注入
  `[Console]::OutputEncoding=[Text.Encoding]::UTF8` 与 `$OutputEncoding` 设置。

### W5（中）project_memory 规则 `paths:` glob 在 Windows 永不匹配

- 现象：`shannon-core/src/project_memory.rs:628` 用 `format!("{}/{}", dir, pattern)`
  拼接，Windows 反斜杠路径与 `/` glob 永不相等；`pattern.starts_with('/')`
  对 `C:\` 判断亦错误。
- 验证：建带 `paths: [src/**]` frontmatter 的 memory 规则，打开 src 下文件确认注入。
- 处置：**本轮修复**——用 `Path::join` + 组件归一化构造匹配路径，
  glob 匹配前统一 `/` 分隔符。

### W6（中）沙箱检测路径分叉：WindowsJob 检测到了却装配不上

- 现象：`shannon-core/src/sandbox.rs:1205` `detect_sandboxer()` 在 Windows 恒返回
  `WindowsJob`；但 `:1555` `detect_sandbox_provider()` 没有 WindowsJob 分支，
  落入 "unsupported platform" → `NoSandbox`。两条检测路径行为不一致，
  后者若为实际装配路径则 WindowsJob 从未生效。
- 验证：Windows 上执行 Bash 工具，检查结果 metadata 的 sandbox 字段与警告文案。
- 处置：**本轮修复**——`detect_sandbox_provider` 补 WindowsJob 分支，与
  `detect_sandboxer` 对齐。

### W7（中）CLI 桌面通知默认依赖未安装的 BurntToast 模块

- 现象：`shannon-cli/src/notifications.rs:59` Windows 默认命令为
  `New-BurntToastNotification`（第三方模块，裸机没有）→ spawn 成功但静默失败。
  `shannon-core/src/notifier.rs:778` 另有一套 WinForms NotifyIcon 气泡实现（可用）。
- 验证：裸机触发通知，确认是否毫无反应。
- 处置：**本轮修复**——CLI Windows 默认命令改为与 core 一致的 WinForms 气泡方案
  （不依赖第三方模块），BurntToast 保留为可配置选项。

### W8（中）后台进程 kill 无进程组/Job 语义，孤儿进程

- 现象：`background.rs:249` `kill_on_drop: false`，KillBackground 只 `child.kill()`
  杀 `bash` 本身；providers.rs 的 Job 兜底只覆盖沙箱 armed 路径。
- 验证：RunBackground 跑 `ping -t 127.0.0.1`，Kill 后 `tasklist` 查残留。
- 处置：**本轮修复**——后台工具路径在 Windows 上把子进程挂入
  KILL_ON_JOB_CLOSE 的 Job Object（复用 `windows_job` 模块），kill 时整树终止。

### W9（低中）symlink 相关测试在无特权 Windows 上 panic

- 现象：`file/sandbox.rs:973`、`tests/file_edge_case_tests.rs:556`、
  `tests/security_tests.rs:575` 用 `symlink_file(...).expect(...)`——需要
  SeCreateSymbolicLink 特权（管理员/开发者模式），普通开发机直接崩。
- 处置：**本轮修复**——无特权时 skip 并输出原因（测试内检测创建结果），
  不再 panic；有特权时照常断言。

### W10（低）REPL `!` 命令与 slash-command 本地 shell 为 `sh -c`

- 现象：`shannon-ui/src/repl/commands/mod.rs:207/287`、`repl_command.rs:130`。
- 处置：**本轮修复**——与 W2 同策略：`sh` 不存在时回退 `cmd /C`。

### W11（低，显示层）UI 侧按 `/` 切路径

- `shannon-ui/src/widgets/sidebar.rs:458/511`、`repl/render.rs:166` 用
  `split('/')` 取文件名，Windows 反斜杠路径不截断（仅显示问题）。
- 处置：本轮不改（低价值），列入规格备忘。

### W12（规格项，不在本轮修复范围）

- **computer-use 真机面**：`windows_real_machine.rs` 10 个测试需
  `--features computer-use,local-browser` + 真实桌面会话手工运行
  （`cargo test -p shannon-tools --features computer-use,local-browser
  --test windows_real_machine -- --ignored --nocapture`）。截图/UIA/剪贴板/
  窗口枚举路径 2026-09 才首次在 Windows 编译，需真机回归。
- **desktop（Tauri）Windows 分支**：artifact URL `http://artifact.localhost` 形态
  （`commands_artifact.rs:142`，已有 Windows 单测）、`\\?\` verbatim 前缀剥离
  （`commands_surface.rs:389`）、PTY 终端默认 `powershell.exe`
  （`terminal_commands.rs:120`，9 个 PTY 测试全部 `#[cfg(unix)]`——测试缺口）、
  NSIS 安装钩子 HKCU PATH 追加（`nsis/hooks.nsh`）、gateway 计划任务探针
  （`gateway_service_probe.rs:166`，`shannon gateway install` Windows 未实现）。
  需真机 GUI 会话，列后续。
- **eval_runner**：`run_verify_script` 在 Windows 恒 Err、`kill_process_tree`
  只杀直接子进程（`shannon-core/src/testing/eval_runner.rs:2093-2142`）——
  eval 全链路 Windows 不可用，需单独设计。
- **凭据文件 0600 语义**：Windows 上全部 no-op（ACL 等价物未实现），
  `credential_manager.rs` / `provider_config_store.rs` / `secret_guard.rs` /
  mobile pairing token 目录。风险已存在于文档，列后续（建议 DPAPI 或 ACL）。
- **server/gateway**：SIGTERM 处理 unix-only（Windows 只响应 Ctrl-C，可接受）；
  gateway keyring 明确降级（release.yml 注释）。L1/L2 覆盖即可。
- **GBK 等非 UTF-8 输出解码**（encoding_rs 方案）：W4 修复 PowerShell 工具
  主路径后，剩余场景（hook 输出、合并工具）列后续。

## 三、分层验证规格

### L1 编译与静态检查（PR 门禁等价）

```bash
cargo check --workspace                 # CI cross-platform Windows 腿等价（本机可含 desktop）
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

### L2 自动化测试

```bash
cargo nextest run --workspace --profile ci        # 与 just test-ci 等价
cargo nextest run -p shannon-core -p shannon-tools --profile ci   # nightly Windows 腿等价
```

- 已知 cfg 缺口（Windows 上不编译/空转，见各条目）：desktop PTY/Preview 测试
  `#[cfg(unix)]`、eval_runner 8 个、coordinator 关停、credential 权限断言、
  browser detect 环境覆盖。这些测试体的 Windows 化列后续。
- W9 修复后，symlink 用例在无特权机器应 skip 而非 panic。

### L3 专项功能验证（本轮逐项执行）

| # | 验证项 | 方法 | 通过标准 |
|---|---|---|---|
| 1 | Bash 工具（有 Git Bash） | 工具调用 `echo hello` | 正常输出，流式路径 |
| 2 | Bash 工具（无 Git Bash） | PATH 去掉 bash 后调用 | 错误文案含 Git Bash/PowerShell 引导，非裸 not found |
| 3 | PowerShell 工具 | `Write-Output '中文测试'`；`Get-ChildItem` 中文目录 | 中文不乱码（UTF-8） |
| 4 | Hooks | `PostToolUse: echo` | 无 sh 时经 cmd 回退执行成功 |
| 5 | Edit CRLF | 多行编辑 CRLF 文件 | 匹配成功且行尾风格保持 |
| 6 | project_memory paths | `paths: [src/**]` 规则 | Windows 路径下规则注入 |
| 7 | 沙箱 metadata | Bash 工具输出 sandbox 字段 | WindowsJob 被报告，NoSandbox 分叉消除 |
| 8 | 后台进程树终止 | RunBackground `ping -t` + Kill | `tasklist` 无残留子进程 |
| 9 | 通知 | headless `--notify` 或 REPL 后台完成 | 裸机弹出系统通知（WinForms） |
| 10 | headless NDJSON | `-p "…" --output-format json-stream` | CiEvent 信封 + done 行整数退出码 |
| 11 | doctor / providers | `shannon doctor`、`shannon list-providers --json` | 命令在 Windows 正常输出 |
| 12 | serve | `shannon serve --auth-token …` + Ctrl+C | TCP 监听、SSE 可连、优雅退出 |

### L4 真机/人工项（规格，不在本轮）

- computer-use 真机 10 项（见 W12）。
- 桌面端：`cargo run`（tauri dev）、NSIS 打包（`desktop` 打包流程 + HKCU PATH 验证）、
  WebView2 环境、gateway 计划任务注册（`schtasks`）。
- 安装脚本：`scripts/install.ps1`（组件选择/SHA-256/PATH）。

## 四、本轮修复范围（对应 PR）

W0、W1、W2、W3、W4、W5、W6、W7、W8、W9、W10、W13、W14、W15、W17、W18、
W19、W20、W21 + L1/L2/L3 真机执行结果。其余（W11、W12 中除 eval
verify 外的部分）列入规格备忘与 backlog。

## 五、本轮执行结果（2026-09-30，真机）

### W0（P0，审查中新发现）shannon-core 测试套件在 Windows 上编译失败

- `housekeeping.rs` 的 `unreadable_session_dirs_are_never_deleted_by_archived_gc`
  测试体直接使用 `std::os::unix::fs::PermissionsExt`（无 cfg 门）——unix-only
  API 使**整个 shannon-core lib test 目标在 Windows 编译失败**，
  `cargo test --workspace`/nextest 全量随之中止。nightly Windows 腿
  （`-p shannon-core -p shannon-tools`）在含该测试的提交上应同样红。
- 修复：测试体语义即 unix mode-bit 模拟（Windows 上 stat 不受 mode 位约束，
  只会走其自身的提前返回），整体 `#[cfg(unix)]` 门控。
- 顺带清理同族噪音：`file/sandbox.rs::test_symlink_to_system_file_blocked`
  （/etc/passwd 语义）与 `plugin_manifest_v2_ecosystem.rs` 的 unix-only
  `Tool` 导入、`preview_commands.rs` 的 unix-only `base64` 导入加 cfg 门。

### W13（低，审查中新发现）desktop/ui `test:ci` 脚本为 POSIX-only 语法

- `"test:ci": "CI=1 vitest --run"` 的 `VAR=1 cmd` 前缀在 Windows cmd/pnpm
  下直接报 "'CI' 不是内部或外部命令"。修复：改为 `vitest run`（`--run`
  语义不变；CI 变量在 GitHub Actions 上由 runner 自动设置）。
- gateway 的 `test`（`vitest run`）本就跨平台，无需修改。



### W14（P0，真机测试中新发现）Session 日志在 Windows 上完全无法写入

- **现象**：`state_integration` 两个恢复测试红（恢复读 0 条消息）；探针证实
  `events.jsonl` 落盘 0 字节——**所有 session 在 Windows 上从未持久化过**
  （resume / `trace show` / `/rewind` 全部无数据可用）。
- **根因**：`SessionLogWriter::open_path` 先经 fs2 `try_lock_exclusive`
  拿住整文件字节范围锁，随后 `scan_tail` 用**第二个句柄**
  （`File::open(path)`）读同一文件做尾部恢复。Windows 的 LockFileEx
  锁是**强制性的**（Unix flock 是 advisory），锁外句柄读取直接
  os error 33（ERROR_LOCK_VIOLATION）→ open 失败 → tee 的 best-effort
  设计把错误吞成 warn（测试环境无 tracing 订阅，完全静默）→ 所有
  `record_*` 走 no-op。
- **修复**：`scan_tail` 改为通过写句柄自身 seek(0) 读取（锁持有者读自己
  的锁范围恒被允许），读毕 seek 回 EOF。"锁下恢复"的语义在所有平台保持
  不变；fs2 锁本身（含 read+append 模式的句柄）经探针证实工作正常。
- **已知余留（backlog）**：由于锁跨 session 生命周期持有 + Windows 强制
  语义，**活跃 session 的日志对其他进程不可读**（`trace show`/并发
  resume 活跃 session 会 os error 33）——设计上可接受（活跃日志本就
  不稳定），但与 Unix 行为有差异；`ExclusiveLogLock` 重写路径（rewind/
  compact）在 Windows 上对"被自己锁住的文件做 rename 覆盖"预计同样
  失败，需专项验证与改造（见 §二 W12 清单扩充）。

#

### W15（高，真机测试中新发现，跨平台）bus 路径丢失 `turn/end.llm_steps`

- `event_bus_reconciliation` 的对账测试红：同一条 `turn/end`，bypass 路径
  `llm_steps: 2`，bus 路径 `llm_steps: 0`。根因：`SessionTee::record_bus_input`
  的 `StepUsage`/`BareTokens` 分支只折叠 usage，**漏了 `turn_steps += 1`**——
  直接路径（`record_query_event`）两个事件各计一步。引擎实际走 bus 路径，
  即所有实时会话的 `llm_steps` 都是 0（`llm-step observability`，#149 引入
  字段时的遗漏；该测试此前未跑或未提交门禁）。
- 修复：两个 coalesce 分支补计数，与 bypass 语义对齐；对账测试转绿。

### W17（低）gateway 两个测试未考虑平台契约

- `secrets.test.ts` 的三个 exec 注入用例在 Windows 上必然失败：
  `commandFor` 对 win32 **设计上**返回 `[null, []]`（无 OS keyring CLI），
  provider 短路返回 null。改 `it.skipIf(win32)` + 补一个显式断言降级
  行为的用例。
- `directE2E.test.ts` 断言私钥 `mode & 0o777 == 0o600`——POSIX 语义，
  Windows 无 chmod 等价物。加平台门。

### W18（低）`hooks_system_tests` 路径后缀断言

- `.ends_with(".shannon/hooks.json")` 假定 `/` 分隔符；改为比较
  `file_name()`/`parent().file_name()` 组件。

### W19（高，vitest 腿新发现）desktop/ui barrel 大小写碰撞 → 页面自引用

- `src/pages/Chat.tsx`（文件）与 `src/pages/chat/`（目录）在大小写不敏感
  文件系统上碰撞：`import … from './chat'` 在 Windows 解析到 `Chat.tsx`
  **自身**（`fs.existsSync` 大小写不敏感命中 `Chat.tsx`），自引用循环使
  barrel 全部绑定 undefined（`ComposerContext.Provider` 直接 TypeError）。
  `Editor.tsx`/`./editor` 同型。**共 74 个 vitest 用例失败**；且不止测试——
  Windows 上 `vite build`/`vite dev` 桌面端同样会坏（CI 的 desktop-unit
  只在 ubuntu 跑，从未暴露）。
- 修复：两处 barrel 导入改显式 `./chat/index`、`./editor/index`
  （`/index` 后缀绕过文件名歧义），Linux 行为零变化；全仓扫描确认无
  其它碰撞对。
- 修复后 desktop/ui vitest：**2086 passed / 0 failed**（此前 74 failed）。

### W20（中，vitest 腿新发现）四个 UI 脚本 `URL.pathname` 反模式

- `new URL(import.meta.url).pathname` 在 Windows 产生 `/C:/…`，再经
  `path.resolve` 锚定成 `C:\C:\…`：`i18n-check.mjs` 直接 ENOENT（连带
  i18nCheck 测试 CLI 合同腿失败），`check-design-tokens.mjs`、
  `codemods/_lib.mjs`、`i18n-coverage.mjs` 同型。统一改
  `fileURLToPath(import.meta.url)`，真机验证 `i18n-check.mjs --report`
  正常输出。

### W21（中，全量回归新发现）eval verify_script 在 Windows 直接死路

- `eval_runner::run_verify_script` 非 unix 恒 `Err("unsupported")`，导致
  eval 彩排全部拿到 verify 警告（`full_suite_metrics_are_complete_for_every_task`
  红："green rehearsals classify nothing"）——eval 全链路 Windows 不可用。
- 修复：Windows 上探测 `sh`（Git Bash/WSL）：有则按 unix 同款执行 POSIX
  verify 脚本；无则给出安装指引的明确报错（替代盲目的 cmd 回退——verify
  脚本是 POSIX shell 契约）。`shannon_types::shell` 新增 `has_sh()` 探测。
- 本机（有 Git Bash）验证：eval 彩排转绿，测试通过。无 Git Bash 的裸机
  行为 = 明确报错，与修复前相比信息量提升。

### W22（低，全量回归新发现）测试侧的 Windows 路径假设（5 处）

- `architecture_invariants::is_workspace_member`：Windows `Path::canonicalize()`
  产生 verbatim `\\?\C:\…` 前缀，与普通 root 做 `starts_with` 恒 false →
  所有 path 依赖被判"非 workspace 成员"。修：两边都 canonicalize 再比较。
- 同文件 `dead_code_allow_keep_markers`：违例 key 用 `\` 拼接而基线是 `/`，
  基线整体失配。修：key 归一化 `/`。
- `background.rs` 新增的 `#[allow(dead_code)]` 缺仓库约定的 `// KEEP:`
  注释（`dead_code_allow_keep_markers` 门）。补上。
- `eval_metrics::find_event_logs…` 与 `eval_runner` 的 Glob/Find stub：
  `ends_with("aaa/events.jsonl")`、Glob/`find .` 输出用原生分隔符——
  契约是 `/`。修：断言改路径组件比较；stub 输出归一化 `/`。
- **运维备注**：回归期间 C 盘一度 100% 满（共享 `target/` 88GB，其中
  incremental 42GB），造成约 18 个测试随机假失败（os error 33 等）；清理
  incremental 后复跑即绿。Windows 真机跑全量前请确保 ≥20GB 剩余空间，
  并定期清理 `target/debug/incremental`。

## 六、最终回归数字（2026-09-30 真机，隔离 target）

| 腿 | 命令 | 结果 |
|---|---|---|
| Rust 全量（除 desktop） | `cargo nextest run --workspace --exclude shannon-desktop --profile ci` | **11645 run: 11531 passed / 114 failed / 59 skipped** |
| dev 基线对照 | 同上（dev 分支） | **测试编译失败**（W0：unix-only API 无 cfg 门）——起点全红，0 个测试可运行 |
| desktop/ui vitest | `pnpm test:ci` | **2086 passed / 0 failed** / 9 skipped（修复前 74 failed） |
| gateway vitest | `pnpm test` | **502 passed / 0 failed** / 3 skipped（win32 降级契约跳过） |
| clippy | `cargo clippy --workspace --all-targets`（除 desktop） | 0 warning |
| fmt | `cargo fmt --all -- --check` | 干净 |
| shannon-desktop Rust 测试 | — | 见 §五 已知残留（DLL 入口点，与 CI 排除策略一致） |

### 余留 114 项失败：dev 既有的 Windows 测试债（本 PR 未引入，已列入 backlog）

基线对照（dev 无法编译测试）+ 失败机制抽样证实均属平台固有差异，主类别：

1. **强制锁语义下的测试读文件**（~15，session_log writer/store/tee 的
   truncate/rewrite/boundary 测试）：writer 持锁期间用第二句柄断言文件
   内容——Unix advisory 锁允许，Windows 强制锁 os error 33。修法：测试
   经 writer 自有句柄读（同 `scan_tail` 的修法），需小规模 API。
2. **路径分隔符断言**（~20，repomap / repl 补全 / housekeeping 等）：
   `ends_with("a/b")`、`assert_eq!(.., "src/main.rs")` 类；修法同 W18/W22
   （组件比较或归一化）。
3. **测试的用户目录/全局状态隔离缺失**（~10，mcp config discovery、cli
   profile）：读真实 `~/.shannon`，与真机环境耦合；需 SHANNON_HOME 隔离。
4. **POSIX 语义断言**（权限位 / `sh` 脚本 / 进程组，~15）。
5. **其余零散**（时间敏感、平台分支文案等）。

完整清单：[windows-remaining-failures-2026-09-30.txt](windows-remaining-failures-2026-09-30.txt)。

**注意事项（真机复现）**：不要多检出共享同一 `CARGO_TARGET_DIR`——交叉
编译会让 cargo 指纹串味（本次出现过 engine 链到旧 shannon-types 的
"could not find `shell`" 与成批假失败）；每检出独立 target，并保证
≥20GB 剩余空间。
