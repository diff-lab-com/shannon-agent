# 测试审计报告 — 2026-09-27

- **Worktree**: `/home/ed/workspace/app/work/shannon/shannon-mono.worktrees/test-audit-20260927`（分支 `test-audit/2026-09-27`，基于 `dev` @ `02214380`）
- **性质**：只跑测试 + 根因分析，**未修改任何代码**。所有结论待审核。
- 本机环境：Ubuntu 22.04（PipeWire 0.3.48 头文件）、rustc 1.88（repo 固定）、pnpm 10.33、`just` 为 snap 安装。

---

## 一、执行范围与结果总览

| # | 套件 | 命令 | 结果 |
|---|------|------|------|
| 1 | Rust 全量（CI 同款 profile，含 2 次重试） | `cargo nextest run --workspace --exclude shannon-desktop --profile ci --no-fail-fast` | **11699 跑：11693 通过 / 5 失败 / 1 超时 / 75 skipped（--ignored live 测试，按设计）** |
| 2 | Rust doctests | `cargo test --workspace --doc`（排除 desktop） | ✅ 全过 |
| 3 | eval 干跑（免 key 全 L1 套件） | `cargo run -p shannon-core --example eval_runner -- --tasks tests/eval/tasks` | ✅ 22/22 passed |
| 4 | eval CLI 入口 + harness 单测 | `shannon eval run --list` / `--task read_01` / nextest `test(eval)` | ✅ 全过 |
| 5 | feature 腿 computer-use / computer-use-libei | `cargo build -p shannon-tools --features ...`（独立 target 目录） | ❌ 均编译失败（与 #8 同根因：libspa） |
| 6 | feature 腿 local-browser | 同上 | ✅ 编译通过 |
| 7 | cargo deny / clippy `-D warnings` / fmt --check | `cargo deny check` 等 | ✅ 全过（deny 需先清障，见 P2-5） |
| 8 | desktop Rust crate 编译 | `cargo nextest run --workspace`（含 desktop） | ❌ **无法编译：libspa 0.10.1 七个编译错误**，整个 workspace 测试被阻塞 |
| 9 | gateway vitest + typecheck | `pnpm test` / `pnpm typecheck` | ✅ 443/443 通过；typecheck 通过 |
| 10 | desktop/ui vitest（CI 命令 `test:ci`） | `CI=1 vitest --run` | ✅ 2071 通过 / 9 skipped / 0 失败 |
| 11 | desktop/ui vitest --coverage（CI 阈值门禁） | `vitest run --coverage` | ❌ 1 失败（Tooltip 超时，已知问题复现）+ 2076 通过 |
| 12 | desktop/ui Playwright e2e | `pnpm test:e2e`（mock 模式自动起 webServer） | ✅ 75 通过 / 31 skipped（均为有意门控） |
| 13 | dogfood supervisor 自测 | `python3 -m unittest discover scripts/dogfood/tests` | ✅ 47/47 OK（有 ResourceWarning 噪音） |

失败全部集中在 `shannon-cli::cli_e2e_tests` 一个文件：

| 测试 | 现象 | 重试后 |
|------|------|--------|
| `test_ollama_request_has_no_tools_field` | 子进程 15s 超时被杀 → stdout 空 → JSON EOF panic（`cli_e2e_tests.rs:145`） | 3 次稳定失败 |
| `test_ollama_request_uses_short_system_prompt` | 同上 | 3 次稳定失败 |
| `test_openai_still_sends_tools_by_default` | 同上 | 3 次稳定失败 |
| `test_ollama_generic_500_retry` | 同上（30s 超时档） | 3 次稳定失败 |
| `test_rate_limit_retries_are_visible_in_headless` | 断言失败：stderr 只有 retry 1/2（部分尝试 stderr 全空） | 3 次稳定失败 |
| `test_rate_limit_exit_code` | **测试本身 180s×3 超时挂死** | 3 次超时 |

---

## 二、问题清单

### P0-1（产品缺陷）repo map 预算对"空文件段落头"不生效 → headless 模式每次运行注入 ~900KB 系统提示词、消耗 ~48s 纯 CPU

**证据链**（用假 Ollama 端点手动复现，`--prompt hello`、cwd=/tmp）：

- 请求体 system 段 **897,488 字符**，其中 `# Repo Map: /tmp` 占绝大部分：**8,572 个 `## 路径` 段落**，几乎全部是 `_(no top-level symbols after trim)_` 空段落。
- 全程耗时 49.3s，其中 **user CPU 48.5s**（纯本地 tree-sitter 扫描/解析，非网络等待）。
- 直接原因在 `crates/shannon-repomap/src/cache.rs` `pack()` 的文档与实现：token 预算只裁剪**符号**，"Markdown 渲染时每个文件的小标题属于 structural，不计入预算"。当 cwd 下文件数巨大且大多无符号时，段落头是 O(文件数) 的无上限开销。
- 根目录解析 `RepoMapInjector::resolve_root()`（`crates/shannon-core/src/query_engine/repo_map_injector.rs:152`）在无 override 时直接取 `std::env::current_dir()`——cwd 是什么就扫什么，无文件数上限、无临时目录豁免。
- 影响：不仅拖垮测试（见 P1-1），**真实用户在任何大目录（如 `$HOME`、`/`）运行 headless 都会产生 ~250K token 的系统提示词**，会击穿任何模型上下文窗口并成倍增加费用/首 token 延迟。

### P0-2（测试卫生缺陷）shannon 自家测试向 /tmp 泄漏 fixture 且从不清理 —— 本机已积累 3,148 项，直接放大 P0-1

- `/tmp` 现有 4,661 项，其中 **3,148 项是 `shannon*` 遗留物**；仅今天跑测试期间又新增 137 项（边跑边漏）。
- 泄漏点例证：`crates/shannon-repomap/tests/incremental_tests.rs:38-50` `snapshot_multi_lang()` 用 `shannon_repomap_test_{label}_{pid}_{nanos}` 命名建立目录，**用后不删**（其中 `remove_dir_all` 只是拷贝前自清理）。同类还有 `/tmp/shannon-clipboard-*.txt`、`/tmp/shannon-api-server-*` 等。
- 这些遗留目录里恰好是 `.rs/.py/.ts/.go` 小 fixture 文件 —— 正是 repo map（P0-1）每次扫描、解析的素材，且每次运行名字都不同，导致磁盘缓存永远失效、每次全量重解析。

### P1-1（测试非密闭）cli_e2e_tests 把被测 CLI 的工作目录设为 `/tmp`，使结果依赖机器污染状态

- `crates/shannon-cli/tests/cli_e2e_tests.rs:120-130` `shannon_with_mock()`：`.current_dir(std::env::temp_dir())`。
- CI runner 的 /tmp 干净 → repo map 秒级 → 测试过；本机 /tmp 有 3 千+ 残留 → 每次子进程 15s+ → 精确在 15s/30s 子进程超时处被杀 → 空 stdout → 5 失败 1 挂死。
- **对照实验**：干净 `HOME` 下重跑 4 个失败测试，签名完全不变（15.02s/30.04s）——排除"用户 ~/.shannon 配置泄漏"假设，锁定 cwd=/tmp 的 repo map 机制。
- 附带发现（不算失败原因但属同类）：`shannon_with_mock` 也没有隔离 `HOME`（只移除了 API key env），子进程会读真实 `~/.shannon/`。

### P1-2（可移植性/文档）libspa 0.10.1 无法在本机编译 → desktop crate 与两个 feature 腿被整体阻塞

- 依赖链：`shannon-desktop → xcap 0.9.8 → pipewire 0.10.1 → libspa 0.10.1`；libspa-sys 构建期从系统头文件 `/usr/include/spa-0.2` bindgen 生成绑定。
- 本机 `libspa-0.2-dev 0.3.48`（Ubuntu 22.04）缺 `spa_meta_region_is_valid`/`spa_meta_first`/`spa_video_info_raw.flags` 等 7 处符号 → libspa 编译失败。
- 后果：`cargo nextest run --workspace` 在编译期即 101（测试一个没跑，最初那轮）；`--features computer-use` 与 `--features computer-use-libei` 两腿同样失败（xcap 被 computer-use 拉入）。主仓库 `target/debug/deps` 里 libspa 只有 `.d` 没有 `.rlib`，说明**主仓库同样从未编译成功过**，并非 worktree 特有。
- CI 侧 `ci.yml` test-features job 安装了 `libpipewire-0.3-dev libgbm-dev libdrm-dev libxdo-dev`（ubuntu-latest = 24.04，PipeWire 1.0.x，符号齐全）所以绿。但 **CONTRIBUTING.md:7 的 Linux 依赖清单缺 `libpipewire-0.3-dev libgbm-dev libdrm-dev`**，且未写版本下限——22.04 开发者按文档装齐仍会撞墙，且报错完全不给指向。

### P1-3（测试债，已复现）Tooltip 单测只在 CI 跳过、本地 coverage 下必超时

- `desktop/ui/src/__tests__/components/Tooltip.test.tsx`：`process.env.CI ? describe.skip : describe`，源内注释标注 KNOWN ISSUE（Base UI 1.8 计时器在 V8 coverage 插桩下放大 ~1000 倍）。
- 本次 `vitest run --coverage` 精确复现：`hides on mouse leave` 在 120s `testTimeout` 上限处超时（该文件级 `vi.setConfig({ testTimeout: 120_000 })`）。
- 源注释同时承认 "The Tooltip shim has zero production callers today"。零调用方的生产组件 + CI 必跳的测试 = 双重死重。

### P1-4（e2e 卫生）调试探针与退役 spec 留在 CI testDir 里随门禁一起跑

- `desktop/ui/e2e/__probe3.spec.ts`：menu path 调试探针（console.log + 裸 waitForTimeout），PR #90（2026-09-19）入库，每次 CI e2e 都在跑。
- `desktop/ui/e2e/nav-groups.spec.ts`：`test.describe.skip` 标注 "retired; sidebar is now flat NavLink" —— 2 个永久 skip 的退役用例留在树上。

### P2-1（工具链语义）nextest 默认 profile fail-fast 生效：2 个失败让 10,552 个测试没跑

- 首轮 `cargo nextest run --workspace`（默认 profile）在 1,147 个测试处停下，10,552 个未执行；`justfile` 的 `test-rust` 兜底 `|| cargo test --workspace -- --test-threads=1` 在编译失败场景同样失败、在测试失败场景则会把 11,699 个测试用单线程重跑一遍——本地排查代价极高。CI 用 `--profile ci`（fail-fast=false + retries=2）无此问题。

### P2-2（断言脆弱）rate_limit 测试设置的退避 env 与断言目标不在同一层

- 测试设 `SHANNON_RUN_RETRY_BACKOFF_BASE_MS=50`（`main.rs:198` 读，作用于 **run 级**重试），但断言的 `API retry 1/4`（`agent_loop.rs:1652`）来自 **API 级** `RetryConfig::default()`（`initial_backoff_ms=1000`，不可 env 注入）。实测子进程按 1s/2s/4s 退避，env 完全没起作用；CI 能过只是因为 60s 超时兜得住。

### P2-3（门禁一致性）`CI=1` 使 CI 与本地执行的测试集不同

- `pnpm test:ci`（= `CI=1 vitest --run`）比本地 `vitest run` 少跑 6 个测试（Tooltip 6 个）+1 个文件级差异；coverage 阈值门禁（desktop-unit job）跑的又是第三种形态。本地绿 ≠ CI 绿、CI 绿 ≠ 本地全绿。

### P2-4（小项）dogfood-selftest 有 `ResourceWarning`（subprocess 未显式关闭）噪音。

### P2-5（本机环境，非仓库问题）
- `just` 为 snap 安装，在受限环境报 `snap-confine ... cap_dac_override`，`just eval` 无法执行（绕过 just 用 cargo 直跑全过，CI 用 taiki 安装的 just 不受影响）。
- `cargo-deny` 的 advisory DB 锁被 9/22 起挂死的残留进程（PID 1103160）占用 5 天，导致 `just deny` 失败；已终止该进程，重跑通过（advisories/bans/licenses/sources 全 ok）。

---

## 三、修复方案（待审核，未实施）

| 问题 | 方案 | 涉及位置 |
|------|------|----------|
| P0-1 | ① `pack()`/`to_system_prompt_markdown()`：完全无符号的文件**不输出段落**（或全部折叠为一行计数 `… N more files (no symbols)`）；② 把 per-file 头纳入预算计算；③ `resolve_root()`/`from_dir()` 增加护栏：跳过 `std::env::temp_dir()` 及 `/tmp`、`/var` 等隐式临时目录，或加文件数硬上限（如 5,000）超限截断并 warn；④ headless 侧对最终 system prompt 总长设护栏 + 超限日志 | `crates/shannon-repomap/src/lib.rs:186-230`、`cache.rs:216-231`、`crates/shannon-core/src/query_engine/repo_map_injector.rs:96-104,152` |
| P0-2 | fixture 改 `tempfile::TempDir`（RAII 自动清理）；仓库内 grep 其余 `shannon_*` 临时命名模式统一改造；提供一次性清理脚本 + CI 增加"测试前后 /tmp 计数不变"探测 | `crates/shannon-repomap/tests/incremental_tests.rs:38`等 |
| P1-1 | `shannon_with_mock()` 改用专用空目录 cwd（`tempfile::TempDir`），并顺带 `.env("HOME", tmp)`/`.env("XDG_CONFIG_HOME", …)` 做全隔离；文件头 `--test-threads=1` 的注释按 nextest 语义修正（`#[serial]` 在 nextest 下不跨进程，如需串行应加 nextest test-group） | `crates/shannon-cli/tests/cli_e2e_tests.rs:120-130,13` |
| P1-2 | ① CONTRIBUTING.md:7 依赖清单补 `libpipewire-0.3-dev libgbm-dev libdrm-dev`，注明 PipeWire 头文件需 ≥1.0（Ubuntu 22.04 的 0.3.48 不够，附 libspa 编译错误特征便于对号）；② 可选：把 xcap/屏幕捕获做成 desktop 的 opt-in feature，让"跑测试"不需要多媒体头文件 | `CONTRIBUTING.md:7`、`desktop/Cargo.toml` |
| P1-3 | 三选一：删除零调用方的 Tooltip shim + 其测试；或测试整体移出 CI 门禁文件集合并显式标注；或迁移为不受 Base UI 计时器影响的实现测试 | `desktop/ui/src/__tests__/components/Tooltip.test.tsx`、`src/components/ui/tooltip` |
| P1-4 | 删除 `__probe3.spec.ts`；删除 `nav-groups.spec.ts`（git 历史可寻回） | `desktop/ui/e2e/` |
| P2-1 | `justfile` `test-rust` 改 `cargo nextest run --workspace --profile ci --no-fail-fast`（与 CI 完全同参）；删除 `||` 串行兜底或仅保留在编译失败时的提示 | `justfile:122-124` |
| P2-2 | 给 API 级 `RetryConfig` 增加 env 注入（如 `SHANNON_API_RETRY_BACKOFF_MS`），测试改设该变量；或断言改为不依赖具体等待时长 | `crates/shannon-engine/src/api/retry.rs:50`、`cli_e2e_tests.rs:860-910` |
| P2-3 | Tooltip 修复后移除 `CI` 条件；同时把 `test:ci` 与 coverage 门禁的测试集合对齐（同一 include/exclude 规则） | `desktop/ui/package.json`、`vitest.config.ts` |
| P2-4 | dogfood 测试 subprocess 用 context manager 或捕获 `ResourceWarning` | `scripts/dogfood/tests` |
| P2-5 | 本机建议（非仓库改动）：升级 `libpipewire-0.3-dev`/`libspa-0.2-dev` 至 1.0.x（或改用 24.04 工具链）；`just` 改 cargo/curl 安装替代 snap | 本机 |

## 四、改进建议（优先级递减）

1. **给 repo map 加"上下文护栏"回归测试**：构造 N 个无符号文件的目录，断言注入块 ≤ 预算对应字节数——P0-1 属于"没有测试守护的隐式契约"。
2. **cli_e2e 增加时长断言/预算报告**：本轮多套件在 60s 红线边缘"侥幸绿"（如 `test_all_producers_json_output_consistent` 285s），建议对 headless 单轮耗时设预期上限并在超阈时打点。
3. **文档**：CONTRIBUTING 增加"测试入口矩阵"（本次共发现 13 类入口，其中 `dogfood-selftest`、`just scenarios`、`just replay` 不在任何 CI workflow 里——要么纳入 CI，要么标注"本地专用"）。
4. **nextest 串行组**：`shannon-cli` 既有 `#[serial]` 意图，可加 test-group 使其串行化（当前 core/commands 有、cli 没有）。
5. **/tmp 泄漏巡检**：纳入 CI（计数探测）或 `cargo tidy` 类脚本。

## 五、复现命令备查

```bash
# 失败测试复现（本机，cwd 污染下 ~2 分钟）
cargo nextest run -p shannon-cli --profile ci --no-fail-fast \
  -E 'test(test_ollama_request_has_no_tools_field)'
# P0-1 复现（独立于测试套件）
python3 /tmp/fake_ollama.py &            # 假端点
cd /tmp && SHANNON_BASE_URL=http://127.0.0.1:18471 SHANNON_PROVIDER=ollama \
  SHANNON_MODEL=test-model shannon --prompt hello --output-format json
# → duration_ms≈49000, 请求体 system 段 ≈897KB
```

完整日志：`/tmp/shannon-test-audit/`（rust-nextest*.log、ui-*.log、chain.log、req_body.json 等）。

---

## 六、对抗性审查（2026-09-27 二轮，审查对象：上文第三节修复方案）

审查标准：每个修复必须 (a) 解决独立于测试的真实问题，(b) 不是为了让特定失败测试变绿而做的特判，(c) 不引入新风险。补充一个决定性实验后，对原方案做如下修正。

### 6.0 补充实验：归因链闭合 + 一个重要修正

| cwd | 端点 | 结果 |
|-----|------|------|
| /tmp（3,148 项 shannon 遗留） | 假 Ollama | 49.3s（user CPU 48.5s），system 段 897KB |
| 空临时目录 | 假 Ollama | **0.36s（user CPU 0.30s），正常返回** |
| 任意（连接拒绝，事故性数据） | 无 | ~2min 空转（run 级重试 20s 退避 ×3），CPU≈0 —— 顺带证实退避常量 |

结论：48s 完全来自"扫描+解析被污染的 cwd"，归因链闭合。但由此暴露原方案的一个**逻辑缺口**：

> **P0-1（提示词尺寸修复）本身不会让失败的测试变绿。** 15s/30s 超时杀掉子进程时，耗时发生在 `from_dir` 的遍历+tree-sitter 解析（延迟），不是发生在渲染 897KB（尺寸）。尺寸修复解决的是"上下文窗口爆掉"这个产品问题；测试解靠的是 P1-1（测试隔离）。两者各自独立必要，不能互相替代——原方案把它们并列在同一节，容易造成"修了 P0-1 测试就该绿"的错觉。

另核实：磁盘缓存键控于根路径、存在 `~/.shannon/repomap/`（`cache.rs:354-359`），对真实用户的重复运行有摊销。因此**延迟问题对真实用户仅首Run支付**（缓存后秒级），优先级低于尺寸问题——修正原报告"48s 即真实用户体验"的表述：真实用户在固定项目里只有首次冷启动付全价，但**尺寸上限缺失每次运行都支付**（缓存不影响提示词组装）。

### 6.1 逐条裁定

**P0-1（repo map 上限）——方向成立，实现方案按"通用上限优先、路径特判靠后"重排，且质量风险需验收**

- ❌ **原方案③的"跳过 temp_dir"单独拿出来就是过度优化**：它只修测试环境，用户在 `$HOME`、`/`、巨大的未跟踪目录下照样爆。真正的修复是**无条件的内容预算**（见下），路径特判最多算纵深防御，且会引入新问题（`env::temp_dir()` 可被 `TMPDIR` 改变、macOS 是 `/var/folders/...`、确实有人在 /tmp 做真实项目）。
- ⚠️ **原方案①"无符号文件不输出段落"有信息损失风险**：文件路径本身是 repo map 的核心价值（模型靠它发现文件），aider 系的地图会把被裁剪文件以纯路径清单保留。直接删除段落 = 用"尺寸达标"换"能力退化"——这是反向的过度优化（为过测试削弱产品功能）。修正为两选一，倾向 (b)：
  - (a) 无符号文件完全不输出（最简单，损失文件发现能力）；
  - (b) 被裁剪文件折叠为**紧凑纯路径清单**（一行 N 个路径，无 markdown 头），且该清单计入预算。保守估计修复后空段落场景输出 ≤ 预算 + O(200) 个保留符号文件的头部。
- ❌ **原方案④"headless 对最终 system prompt 总长设护栏"收回**：对组装后的总提示词做静默截断会破坏三层缓存断点设计（CLAUDE.md：system 块上的缓存断点注入），截断点落在哪里不可控，属于"用另一个隐性行为掩盖一个显性缺陷"。护栏应设在**生产者**（repo map 块本身），且超限时打 warn 日志而非静默剪。
- ⚠️ **需要质量验收，不只是单元测试**：repo map 输出格式变更会改变所有用户的提示词内容。验收必须包含：真实仓库快照的 golden map 对比 + 触发一次 `agent-eval-nightly`（真实模型 L1 评测）确认无能力回退。只跑单测就合并 = 把格式回归留给用户发现。
- ✅ 顺带核实了一个"修尺寸却不修延迟"的陷阱：由于空 cwd 下 0.36s，遍历/解析的性能问题在缓存摊销下可接受，**不需要**为延迟做异步构建/时间预算遍历（原方案没提，这里明确记录为"考虑过并否决"，防止未来有人当真去做）。

**P0-2（/tmp 泄漏）——成立，但必须与 P1-1 绑定，单独做就是一个"棘轮"**

- ⚠️ **只修泄漏不修测试隔离 = 过拟合到干净环境**：CI 机器 /tmp 干净所以绿，本地跑 N 次后又积到超时——测试"在新机器上过、用着用着坏"，等于没修。顺序必须是 P1-1（解耦机器状态）为主，P0-2（磁盘卫生）为辅。
- ⚠️ **清理脚本范围要收窄**：`/tmp/shannon*` 里混有**应用运行时产物**（`shannon-clipboard-*.txt`、`shannon-api-server-*`），一刀切 `rm -rf /tmp/shannon*` 可能伤到正在运行的 desktop 实例。只清测试 fixture 前缀（如 `shannon_repomap_test_*` 等，逐一 grep 确认生产者后列白名单）。
- ⚠️ CI 泄漏探测同样按前缀计数，不做全 /tmp 计数（CI 上有其他进程并发写 /tmp，会 flaky）。

**P1-1（测试 cwd 隔离）——成立，升级为失败测试的主修复；但两个子项降级**

- ✅ cwd 从 `/tmp` 改为每测试专用空目录：这是让 5+1 个失败与机器状态解耦的正解，且**不弱化任何断言**（这些测试验证的是请求载荷格式，cwd 只是产品读取的环境）。
- ⚠️ **HOME 隔离降级为"待审计后决定"**：干净 HOME 实验证明它不是失败原因；而且 87+ 个测试共用 `shannon_with_mock`，其中可能有刻意验证配置读取行为的用例，一键改共享 helper 会改变它们验证的内容。正确做法：逐个审计该文件对 HOME 的实际依赖后再动，或只对需要的测试加隔离。原则：**修复不得改变测试所验证的东西**。
- ❌ **收回"给 shannon-cli 加 nextest 串行组"的建议**：测试密闭化之后，串行化解决的是一个不复存在的共享状态问题，还白白拖慢最慢的套件。`#[serial]` 在 nextest 下是 no-op，留着无害；文件头 `--test-threads=1` 的注释更新为 nextest 语义即可。

**P1-2（libspa / 依赖文档）——文档部分成立；feature-gating 从修复方案中摘除**

- ⚠️ **不要在文档里写未经证实的版本下限**：我只验证了"0.3.48 失败、CI 的 24.04（1.0.x）成功"，没有二分出真实的最小版本。文档写法应为"已验证矩阵"（24.04 ✅ / 22.04 的 0.3.48 ❌ + 报错特征），不写"需要 ≥1.0"这种拍脑袋数字。
- ❌ **"把 xcap 做成 opt-in feature"从测试修复中移除**：这是产品能力矩阵的变更（桌面端屏幕捕获是否随构建默认提供），不该搭在测试修复的便车上。若要做，单独立项 + 产品决策。文档补齐即可解除 22.04 开发者的排障困境。

**P1-3（Tooltip）——问题成立，首选方案从"删除"改为"诚实跳过"**

- ⚠️ "删除零调用方的 shim"是产品/设计系统决策（`components/ui/` 里的原语可能等 Base UI 修复后启用），不是测试修复能顺带决定的。**最小诚实修复**：`describe.skip` 无条件化（现在是 `process.env.CI ? skip : run`——CI 绿本地红的根源）+ 注释挂 issue 跟踪。等 Base UI 修复或组件有了调用方再启。
- ⚠️ 诚实备注：我的 coverage 复现是在 Rust 巨型套件并发运行时做的，CPU 争用可能夸大了超时；源注释声称的 CI runner 100s/用例与本机复现方向一致，但不是干净环境的受控复现。

**P1-4（删 probe/退役 spec）——成立**。唯一叮嘱：删前 `git log` 确认 `__probe3.spec.ts` 无活跃引用（本次已查：独立文件，无引用）。

**P2-1（justfile test-rust）——收回原方案，问题重新定性**

- ❌ 原方案"`test-rust` 改 `--profile ci --no-fail-fast`"是**为审计场景优化常态化开发路径**：fail-fast 对开发者的快速反馈是有价值的行为，retries=2 还会掩盖开发者本应看到的 flake。真正的问题是：① `|| cargo test --test-threads=1` 这个兜底在测试失败时会把 11,699 个测试单线程重跑（惩罚性的慢），语义混乱；② 本地与 CI 参数静默分叉。修正：保留快路径默认 fail-fast；兜底改为提示性报错（告诉用户"改用 `just test-ci` 复现 CI 行为"）；新增 `test-ci` recipe（= CI 同参）。这样审计/复现 CI 时用 `test-ci`，日常开发保持快。

**P2-2（退避 env 错配）——优先级对调**

- 首选不再是"给 `RetryConfig` 加 env 注入"（往生产代码里加只为测试服务的配置面），而是**改断言**：测试的契约是"重试过程对用户可见"（stderr 出现 `API retry k/4` 序列、json-stream 带 progress 事件、exit_code=4），**与具体等待时长无关**。断言去掉对 env 的依赖即可；env 注入仅当未来确需验证退避时长语义时再立项。

**P2-3（CI/本地测试集对齐）——跟随 P1-3 的决策，不单独强推**：Tooltip 修好前强行对齐 = CI 门禁挂上 15 分钟的必超时文件。coverage 只在 desktop-unit 强制、本地从宽是合理分层，文档写清楚即可，不为一致而一致。

**P2-4 / P2-5**：维持。补充一句诚实修正：snap 版 `just` 的失败发生在我的**沙箱化自动化 shell** 里，用户常规终端很可能不受影响（CI 用 taiki 安装的 just，确定不受影响）——不应记为仓库问题，仅作环境备注。cargo-deny 挂死进程已终止（真实环境事件，与仓库无关）。

### 6.2 被明确否决的"捷径修复"（防过度优化清单）

以下做法都能让测试变绿，全部拒绝：

1. **调大子进程超时**（15s→120s）：掩盖 P0，测试变慢，规模再大 10 倍时原地爆炸。
2. **CI 里测试前清 /tmp**：同时掩盖泄漏（P0-2）和无上限扫描（P0-1），本地继续烂。
3. **只在产品里特判 temp_dir**：修好测试环境，对真实用户的大目录问题零帮助。
4. **删掉/ignore 6 个失败测试**：等于把 P0 产品的哨兵撤掉。
5. **cli_e2e 里关掉 repo_map_enabled**：测试继续绿，但 headless+repo map 这条真实产品路径从此无测试守护。
6. **把 repo map 砍到固定极小预算/直接禁用**：尺寸达标了，功能废了（反向过拟合：为过测试阉割产品）。
7. **给失败路径加 retry 到不再失败**：6 个失败全是 3/3 稳定复现的确定性失败，不是 flake，重试不是修复。

### 6.3 修复后的验收标准（反过拟合口径）

- **A1 脏机器验收**：不清理本机 /tmp（保持 3,148+ 项污染），全量 Rust 套件绿。测试必须对污染免疫，而不是依赖干净环境。
- **A2 合成对抗目录**：构造 2 万个无符号文件（含嵌套）的目录，headless hello：(i) repo map 块 ≤ 预算推导上界；(ii) 冷启动/缓存两种状态下的耗时均在声明 SLO 内。
- **A3 质量回归**：真实仓库 golden map 对比 + 一次真实模型 L1 评测（复用 `agent-eval-nightly`）确认 repo map 格式变更无能力回退。评测集只用于验证，不对它调参。
- **A4 无断言弱化**：cli_e2e 全文件（90+ 用例）在新 helper 下全绿；除 P2-2 明确废除的时长耦合假设外，不得删除任何断言。
- **A5 免疫性差分**：修复后，同一项目在"干净 cwd"与"污染 cwd"下产出的 map 尺寸均有界；合法仓库的顶层符号仍然完整呈现。

### 6.4 实施顺序（按依赖关系，非报告原顺序）

1. **测量已毕**（6.0），直接进 P1-1 cwd 隔离 —— 立刻解除 6 个失败对机器状态的依赖，恢复测试信号可信度。
2. **P0-1 尺寸预算**（方案 (b) 折叠清单版）+ 生产者护栏 + A2/A3 验收 —— 修产品。
3. **P0-2 泄漏治理**（tempfile 改造 + 前缀白名单清理 + CI 前缀计数探测）。
4. **P1-2 文档**（已验证矩阵写法）。
5. **P1-3 / P1-4 / P2-2**（诚实跳过 + 删死测试 + 断言解耦时长）。
6. **P2-1 重定义后的 justfile 调整**、P2-3 跟随 P1-3 落地。
7. 每步跑 A1 验收，全部完成后跑一次完整矩阵（含 desktop 编译本机仍受限，以 CI 为准）。
