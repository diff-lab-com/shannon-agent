# macOS 测试验证规格（2026-09-30，dev@9a09a5a2）

> 目标平台：macOS 15.5（darwin 24.5.0）arm64，仓库钉住工具链 rustc 1.88.0。
> 审查基线：`dev` `9a09a5a2`（Merge PR #158），即 v0.12 协议硬化 + provider 体系 +
> liquid glass UI + 移动网关 v0.13 握手之后的最新开发头。
> 关联：[QA 清单](2026-09-07-computer-use-browser-qa-checklist.md)、
> [macOS 真机结果（首批）](2026-09-10-macos-real-machine-qa-results.md)、
> [ci-gates](../ci-gates.md)。

## 一、为什么需要在 macOS 上专门验证

CI 对 macOS 的覆盖存在结构性空白（见 `docs/ci-gates.md` 与
`.github/workflows/`）：

1. `cross-platform` job 的 macOS 腿**只做 `cargo check`**（`ci.yml:768-781`），
   从不运行测试；
2. `nightly-platform-tests.yml` 每日只跑 `-p shannon-core -p shannon-tools`
   两个 crate，workspace 其余 19 个 crate（ui/cli/commands/engine/server/desktop…）
   的测试在 macOS 上**无任何自动执行**；
3. 桌面 dmg 只在 `release.yml` 构建，无 PR 级验证；
4. 大量 `#[cfg(target_os = "macos")]` 运行时路径（Seatbelt、caffeinate、osascript
   通知、Keychain、launchctl 探测等）只能真机验证。

本规格即本次（2026-09-30）在真机上执行验证的依据；执行结果记录在
[§五](#五执行结果)。

## 二、验证矩阵总览

| # | 验证项 | 方法 | 通过标准 | 前置条件 |
|---|---|---|---|---|
| R1 | Rust workspace 全量测试（默认 feature） | `cargo nextest run --workspace`（nextest 不可用时 `cargo test --workspace -- --test-threads=1`） | 0 失败 | 无 |
| R2 | `computer-use` feature 形态 | `cargo test -p shannon-tools -p shannon-cli --features computer-use`（编译+单测；ignored 真机用例除外） | 编译通过，0 失败 | 无 |
| R3 | clippy 双形态归零 | `cargo clippy --workspace --all-targets` ×（默认 / computer-use） | 0 warning | 无 |
| D1 | 桌面 Rust 后端编译 | `cargo build -p shannon-desktop` | 成功 | 无 |
| D2 | 桌面前端 vitest + 构建 | `cd desktop/ui && pnpm test:ci && pnpm build` | 全绿 | pnpm |
| D3 | 桌面应用可启动（dev 或 bundle） | `externalBin` 就位后 `cargo run -p shannon-desktop` / `tauri build` | 窗口出现、无 panic | 见 §四 |
| G1 | gateway vitest | `cd gateway && pnpm test`（或仓库 `just test-gateway`） | 全绿 | bun/pnpm |
| P1 | Seatbelt 沙箱后端 | 沙箱相关单测 + `detect_platform_sandbox`/后端检测真机断言 | macOS 识别为 Seatbelt，spawn 插件经 `sandbox-exec` 落地 | 无 |
| P2 | osascript 通知（core + CLI） | 真机触发 `DesktopNotifier::send_macos` / `ShellNotifier` 模板转义用例 | 通知横幅出现；AppleScript 注入用例转义正确 | 通知权限 |
| P3 | caffeinate 防休眠 | `prevent_sleep` 真机冒烟（持有 Guard 期间 `pgrep caffeinate`） | caffeinate 进程随 Guard 生存/退出 | 无 |
| P4 | applescript 工具 | `return 1+1`（osascript）、JXA、`shortcuts list`、30s 超时 | 各路径按契约成功/超时 | 无（shortcuts 走 env 门控） |
| P5 | 浏览器检测（macOS 路径） | `detect_system_browser()` 真机探测 + 会话层 e2e（如本机有 Chrome） | /Applications 候选命中 | 可选 Chrome |
| P6 | pngpaste 粘贴截图 | TUI 粘贴路径（安装 pngpaste 则真跑，否则确认缺失时的引导文案） | 有依赖→成功；无依赖→明确报错+brew 提示 | pngpaste（可选） |
| P7 | Keychain 凭据（keyring apple-native） | desktop connections 凭据写入/读取/删除真机冒烟 | Keychain 条目创建且读取一致 | 可能弹 Keychain 授权框 |
| P8 | launchctl 网关探测 | `gateway_service_probe` 对非存在服务的探测（负路径）+ 有服务时正路径 | 探测按 `launchctl print` 结果正确判定 | 无 |
| P9 | `shannon desktop install` / URL scheme | CLI 代码路径检查（无 dmg 产物时验证错误引导） | 明确报错而非 panic | 可选 dmg |
| P10 | 真机 QA harness（ignored 用例） | `cargo test -p shannon-tools --features computer-use --test macos_real_machine -- --ignored`（TCC 门控项按环境取舍） | 非 TCC 项全绿 | TCC 授权（可跳过） |
| N1 | providers.toml 存储与跨进程一致性 | `shannon-core` 单测（store 28 例 + `provider_cross_process_consistency` + precedence） | 全绿 | 无 |
| N2 | `shannon providers` CLI 真机 smoke | `providers_binary_smoke` + 隔离 HOME 手工 add/list/remove | providers.toml 写入符合 ADR-0005（0600、密钥仅引用） | 隔离 `HOME` |
| N3 | TUI `/connect` 七步编排 | repl 单测 + 真机 REPL 走错误路径（无 key/错 key 的报错与首次运行指引） | 报错路径清晰、secret 不落盘 | 可选真实 key |
| N4 | `/config set` secret 键拒绝 | `config_kv` 单测 + 真机 REPL 尝试 set api_key | 拒绝并给出 `/credentials` 指引 | 无 |
| N5 | `/provider health` skip 原因展示 | provider.rs 单测 + 真机 REPL | skipped 行按原因展示（i18n 中英） | 无 |
| N6 | 模型 picker All tab | `select.rs` 单测 + 真机 REPL 打开 picker | All/Fast/Standard/Pro 分页可用 | 无 |
| N7 | `/permissions` 一等命令与别名 | repl 单测 + 真机 REPL `/permissions`、`/perms`、`/perm` | 三拼写均可达同一命令 | 无 |
| N8 | desktop provider-status/test/fetch-models | desktop/ui vitest（AddProviderModal 等）+ 真机桌面设置页操作 | 状态、测试连接、模型拉取按 provider 返回 | 桌面可启动；可选 key |
| N9 | auth-failure 横幅 | `Chat.test.tsx` auth 用例 + 真机配错 key 触发 `query:failed` | 横幅出现、分类为 auth | 桌面可启动 |
| N10 | 网关配对 + approval.decide v2 + direct E2E | gateway vitest（pairing / accessRpc / approvalDecideSigGolden / directE2E） | 全绿 | bun/pnpm |
| N11 | liquid glass 主题 + sidebar 分组 | desktop/ui vitest + （可行则）Playwright e2e glass-budget/themes | 全绿；宽度收窄无溢出回归 | pnpm |
| N12 | i18n 8 语种全 key 覆盖 | i18n 相关测试 + `just check` 内校验 | 210 keys × 8 语种无缺失 | 无 |

## 三、重点风险项说明（为什么列这些）

- **P1/P3/P7/P8**：纯 macOS 机制（Seatbelt / caffeinate / Keychain / launchd），
  Linux CI 永远测不到运行时行为，nightly 也只覆盖 core+tools 的单测子集。
- **N2/N4/N5/N7**：本周期（0575e0af..9a09a5a2）provider 体系重构的核心用户路径，
  涉及本机文件（`~/.shannon/providers.toml`、credentials）——真实文件系统语义
  （权限位、flock、macOS 目录）只有真机能确认。
- **R1**：仓库历史上已有"macOS 首编即炸"前科（见 2026-09-10 结果 F1/F3/F11），
  每次大合并后全量真机跑一遍是最低成本的防回归手段。
- **D3/N8/N9**：桌面 dmg 仅 release 构建一次，日常回归为零；至少验证 dev 启动
  与前端测试门。

## 四、执行注意事项

1. **测试隔离**：涉及 `~/.shannon` 的手工验证一律用独立 `HOME`
   （`HOME=$(mktemp -d) shannon …`），避免污染真机用户配置。
2. **TCC 弹窗**：P2/P4/P10 可能触发 通知/自动化/辅助功能 授权框；无人值守时
   允许跳过对应用例并在结果中标注 `⏸ 待人工`。
3. **真机副作用**：`shortcuts run`、真实 provider 调用（计费）一律不自动执行；
   shortcuts 走 `SHANNON_QA_SHORTCUT` env 门控，provider 真连仅在提供测试 key 时。
4. **串行组**：shannon-core/shannon-commands 有 nextest 串行组
   （`.config/nextest.toml`）；fallback 单线程参数见 `CLAUDE.md`。

## 五、执行结果

执行环境补充：Node v26.3.1（CI 用旧版 LTS——多项前端问题因此只在真机暴露）、
真实 `~/.shannon` 已配置 Keychain 后端的 provider（zcode-glm）。

| # | 结果 | 说明 |
|---|---|---|
| R1 | ✅（修复后） | 首轮 3110 例串行暴露 4 个失败（见 §六 T7/T9 + P1 佐证）；最终 core+commands 串行 **4549 例 0 失败** |
| R2 | ✅（修复后） | `--features shannon-tools/computer-use` 形态 tools+cli 全绿（计算机使用形态最终跑 258+ 通过，2 个零星失败见文末注记） |
| R3 | ✅ | clippy `--workspace --all-targets` 双形态 0 warning |
| D1 | ✅ | `cargo build -p shannon-desktop` 成功 |
| D2 | ✅（修复后） | 首轮 vitest 1750 失败（T1/T2）；修复后 **2101 通过** / build ✓ / lint ✓ |
| D3 | ✅ | dev 模式真机启动：窗口出现、webview 加载、后端命令流动（日志）；菜单栏正确显示 provider 状态（zhipu-coding / glm-5.3）；桌面全 targets **1138 例 0 失败** |
| G1 | ✅（修复后） | 首轮 3 个 Ed25519 互操作用例失败（T3）；修复后 **504/504** |
| P1 | ✅ | Seatbelt 后端检测单测真机通过；`sandbox-exec` 在位 |
| P2 | ✅/⏸ | osascript 通知命令退出码 0、转义单测通过；横幅目视确认阻塞于 Screen Recording TCC |
| P3 | ✅ | caffeinate 原语真机验证（持有期间进程存活、超时后退出）；prevent_sleep 单测通过 |
| P4 | ✅ | osascript AppleScript/JXA `1+1` 真机通过；TCC 弹窗类保持 env 门控 |
| P5 | ✅ | Chrome 在 `/Applications/Google Chrome.app` 标准路径命中；detect 单测通过 |
| P6 | ⏸ | pngpaste 未安装——缺失引导路径属 TUI 交互面（见 N3-N7 注记） |
| P7 | ✅/⚠ | keyring `apple-native` 编译通过；`providers add` 实测写 store 引用且未弹 Keychain 框；真机读写冒烟待桌面手工 |
| P8 | ✅ | `launchctl print user/501/shannon.gateway` 负路径输出明确，probe 单测通过 |
| P9 | ✅ | 代码路径核查 + 单测通过 |
| P10 | ⏸ | `macos_real_machine` ignored harness 需 TCC 授权（Automation/Accessibility），保持人工 |
| N1 | ✅ | providers.toml 存储单测 + 跨进程一致性 + precedence 全绿（R1 内） |
| N2 | ✅ | 真机 `shannon providers add/remove`（隔离 HOME）：0600 ✓、密钥不落盘（store 引用）✓、remove 清 active pointer ✓、重复 remove 走干净错误路径 ✓ |
| N3 | ✅/⏸ | 真机 headless 无 provider → 回落 ollama 明确报错重试不 panic；首次运行 TUI 指引与 `/connect` 交互属 TUI 面（⏸ 注记） |
| N4 | ✅ | 真机 CLI 镜像 `shannon config -s providers.*.api_key=sk-…` → 拒绝并指引 `/credentials`；`sk-leak` 全 HOME 树 0 命中；交互 TUI 面由 macOS 绿单测钉住 |
| N5-N7 | ✅（单测）/⏸ | health skip / picker All tab / `/permissions` 别名的契约由 repl、select、provider 单测在 macOS 真机跑绿；交互渲染观察 ⏸（Screen Recording TCC + crossterm 光标探测限制） |
| N8/N9 | ✅ | desktop/ui vitest 全绿（AddProviderModal、auth banner 用例）；真机桌面操作待手工 |
| N10 | ✅ | gateway pairing / approvalDecideSigGolden / directE2E vitest 504 全绿 |
| N11 | ✅ | vitest 全绿；Playwright e2e 依赖浏览器授权未跑（CI 覆盖） |
| N12 | ✅ | i18n-check：en 3561 keys × 10 locales 全匹配 |
| 附 | ✅ | 真机 `shannon doctor`（隔离 HOME）：surface/port/installs 检查全通过，exit 0 |

### 注记（⚠ 观察项与残余零星失败）

1. `shannon config` CLI 对「不可写的非 secret 键」（如 `secret_guard.mode`）的拒绝
   文案与「secret 键」混同（都提示 secrets belong in /credentials）——建议拆分文案；
   拒绝时 EXIT=0 也值得商榷。未动（需产品决策）。
2. workspace 余量段（C）与 computer-use 形态（D）在最终收尾跑中各有 1-2 个零星
   失败；C/D 复跑定位记录于同日 PR 描述。C 段的 language_tests 长跑（>10min）
   为已知慢测试。

## 六、发现与修复

> 编号前缀：P=产品级缺陷（影响真实用户）、T=测试/设施缺陷（CI 进程隔离掩盖）。

### P1（严重）notify 的 `macos_kqueue` feature 使 macOS 全部目录监听静默失效

- 影响：`shannon-core` ConfigWatcher（`.shannon.toml` 变更 hook）、`shannon-ui`
  SourceWatcher（自定义命令热载）、desktop agent-message watcher——kqueue 只能
  监听文件 vnode，不能监听目录内容；`watch(dir)` 不报错但**永远不投递事件**。
  Linux CI（inotify）与 nightly（core+tools 单测不在事件路径）均不可见。
- 修复：三处 `Cargo.toml`（core/ui/desktop）移除 `macos_kqueue`，恢复默认
  FSEvents 后端；core 侧留防回归注释。佐证：`test_watcher_fires_on_modify`
  从"等 3 秒也不触发"变绿。

### P2（严重）`DockerSandbox::docker_available` 无超时——引擎查询可无限挂起

- 探测在**每次引擎查询**的 sandbox self-description 路径上运行 `docker info`
  并同步 `wait`。本机 Docker Desktop 未运行但 CLI 在位时，`docker info` 在
  socket 连接上无限阻塞（实测 0% CPU 挂死）→ 每次查询挂起，routine_run 测试
  因此挂死（`sample` 采样栈钉死在 `wait4` ← `docker_available`）。
- 修复：spawn + `try_wait` 轮询，5s 超时 kill 并视为不可用。

### P3 `-c`/`--continue` 会话选择用原始路径字符串比较（/private 别名）

- macOS `getcwd` 报物理路径 `/private/var/...`，而记录的 cwd 可能是 `/var/...`
  别名（TempDir、/tmp 启动、旧版本记录）。`resolve_resume` 的选择过滤裸比较
  → 选错会话/拒绝恢复；headless 下表现为空输出。`current_cwd_matches` 早已
  规范化，选择逻辑没有跟上。
- 修复：提取 `same_cwd()`（先裸比较快路径，再双侧 canonicalize）供选择与
  比较共用。

### P4 desktop 文件范围检查不识别 /tmp|/var 别名拼写

- `allowed_path_bases` 只保留 canonicalize 后的 base（`/private/...`）；
  `is_probable_path_in_scope` 对**尚不存在**的路径无法 canonicalize，
  `/tmp/x.md`、`/var/folders/...` 拼写被判出界（macOS-only 可用性缺陷）。
- 修复：base 同时收录 raw 与 canonical 两种拼写（两个调用点语义均安全）。

### T1 desktop/ui vitest 在 Node ≥ 25 全军覆没（1750/2110 失败）

- Node ≥ 25 暴露实验性全局 `localStorage`（无 `--localstorage-file` 时为
  undefined 的 own property），vitest 的 jsdom 全局填充跳过已存在的 key →
  测试里 `localStorage` 全部 undefined。CI 用旧 Node 不可见。
- 修复：`src/__tests__/setup.ts` 垫片——delete 后用一次性 JSDOM 窗口的真实
  Storage 重新定义（不能用 getter：jsdom 环境下 `window === globalThis`，
  getter 自引用栈溢出）。

### T2 `pages/Editor.tsx`+`editor/`、`pages/Chat.tsx`+`chat/` 大小写文件/目录冲突

- 无扩展名导入 `'./editor'` 在大小写不敏感文件系统（macOS/Windows）上先命中
  `Editor.tsx` 自身 → 自引用，`tsc`/`vite build`/vitest 全挂；Linux CI 精确
  命中 `editor/index.ts` 永远绿。Chat 同款（`'./chat'`）。
- 修复：`Editor.tsx` → `EditorPage.tsx`（git mv，2 处引用更新）；
  `Chat.tsx` 的 `'./chat'` → `'./chat/index'`；新增
  `scripts/check-import-case-collisions.mjs`（只在"精确大小写文件未命中 +
  大小写不敏感文件命中 + 目录命中"三条件同立时拒绝，避免误报精确命中），
  接入 `pnpm lint` 与 `pnpm check:fs`。

### T3 gateway Ed25519 互操作测试用了 Node ≥ 24 拒绝的 JWK 形态

- `{ kty: OKP, crv: Ed25519, x: "", d }` 派生写法在 Node 24+ 抛
  `Invalid JWK OKP key`。修复：改用 RFC 8410 PKCS#8 DER（固定前缀 + seed）
  从种子独立派生，Node 20-26 全兼容，且保留 x/d 一致性校验语义。

### T4 desktop `session_window_acl` 双 `generate_context!` 展开在 macOS 链接失败

- 每次展开内嵌同名 `_EMBED_INFO_PLIST` 静态（embed-plist，macOS-only），
  同一二进制两次展开 → 链接期 `symbol already defined`。修复：收敛为单一
  `fresh_context()` 展开。

### T5 terminal `process_gone` 在 macOS 把僵尸进程当存活

- 无 /proc 回退到 `kill(pid, 0)`，而 SIGKILL 后未被收割的僵尸对信号 0 探测
  仍"存活"→ 4 个 kill/kill_all/drop 测试 5s 超时。修复：macOS 分支用
  `waitpid(WNOHANG)`（测试进程即父进程，可安全观察并收割僵尸）。

### T6 bursts 测试断言 bash 风格提示符

- 交互 shell 提示符断言 `text.contains('$')`，macOS 默认 `$SHELL=zsh` 的
  提示符是 `%`。修复：`$`/`%`/`#` 三拼写。

### T7 secret_guard ENABLED OnceLock 被前序测试锁存

- nextest 每测试独立进程掩盖了共享进程 `cargo test`（CLAUDE.md 文档化回退
  方式）下的跨测试污染：任一测试触发 `init_from_env_or_config` 后 ENABLED
  永久锁存为 Audit，后续"unset 状态"断言（外部 transform 不建议 redact）
  必然失败。修复：ENABLED 改 AtomicU8（生产语义不变：仍一次性 install），
  测试 seam `reset_redaction_suggestion` 一并清零。

### T8 routine_run 引擎测试读真实 `~/.shannon`（双向污染）

- 测试不隔离 HOME：既会**写入**真实 `~/.shannon`（meta.json、desktop/，
  本次真机 timestamps 实证），也会被真机已配置的 Keychain 后端 provider
  反向卡死（凭据解析阻塞）。CI 无 `~/.shannon` 所以绿。
- 修复：两个 routine_run 测试将 `SHANNON_HOME` 指向 fixture（RestoreHome
  RAII 恢复，CWD_LOCK 串行化下 set_var）。

### T9 providers 测试的两个 macOS 假设

- `/bin/true` 在 macOS 不存在（`/usr/bin/true`）→ spawn ENOENT；
- `sh` 的 `pwd` 输出物理路径与 TempDir 的 `/var` 拼写失配 → 断言改规范化比较。

### T10 repomap 缓存键拼写不一致（/private 别名，第 4 例）

- `RepoMapCache` 构造时把 root 规范化（`/private/var/...`）作为键拼写，但
  `update_file`/`remove_file`/查找用 `absolutize` 原样接受 raw `/var/...`
  拼写 → macOS 上 remove/lookup 静默 miss（产品影响：别名路径下缓存更新
  失效，repomap 退化）。修复：`absolutize` 统一解析到与构造一致的 canonical
  拼写（存在则 canonicalize；不存在则 canonical(root)+relative 回退）。
- 同套件的 `pack_snapshot_matches_pack` 断言了具体符号名
  (`snap_func_0`)，而 60-token 预算下哪个文件的符号存活取决于 `read_dir`
  目录序（APFS 与 ext4 不同）——预先存在的顺序敏感断言，CI 碰巧绿。改为
  顺序无关断言（稳定性 + 与破坏性 pack 等价 + 缓存不被破坏这三个不变量
  已覆盖契约）。

### T11 shannon-tools 的两处 macOS 断言假设

- `test_docker_build_args_bind_is_read_only`：`starts_with("/tmp:")` 在
  macOS 同时命中容器的 /tmp tmpfs 挂载（`/tmp:rw,...`）而非 workspace bind
  （canonical 拼写 `/private/tmp:...:ro`，产品行为正确）。改为按 canonical
  拼写查找。
- `project_registration_echo_matches_command_sandbox_view`：Read 工具回显的
  file_path 是 canonical 拼写（历史键规范化，正确行为），测试按 Linux raw
  拼写假设 → 改为比较规范化形式。

### 已知残余（未修，记录）

- `shannon-cli` 的 `signals::unknown_direction_is_rejected_without_counting`
  偶发失败（全局计数器 + 后台 flush 竞态；复现率低，nextest 进程隔离下
  CI 不可见）。同套 `crash_hook::test_panic_writes_structured_crash_json`
  间歇性失败（约 20-30%，特征：0.00s 即 FAILED 且无 panic 消息——疑似
  hook/backtrace 路径 abort；重跑即过）。两者均为预先存在，非本次改动引入；
  最终收尾跑中 C/D 段各出现的 1 个失败即此两项。
- `shannon config` CLI 拒绝文案把「不可写的非 secret 键」与「secret 键」
  混同，且拒绝时 EXIT=0——需产品决策，未动。

### 改进（非修复）

- `desktop/ui/scripts/check-import-case-collisions.mjs`：文件/目录大小写
  冲突守卫（T2 防回归），接入 `lint`。
- core Cargo.toml 注明 `macos_kqueue` 禁用原因（P1 防回归）。
- `DockerSandbox::docker_available` 探测超时（P2）同时保护 Linux 主机上
  daemon 卡死的场景。

