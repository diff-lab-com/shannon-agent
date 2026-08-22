# Shannon 自主改进循环— 实现方案

- **状态**: §10 于 2026-08-20 定夺,并两次增补(第 8 项渐进坡道、第 9 项分析范围扩展),全文已同步,待终审
- **日期**: 2026-08-19(2026-08-20 三次更新:① 复核 §2 现状盘点,对齐 Phase B(ADR-0011)落地后的仓库现状;② §10 定夺落地——预算改 **token 三层闸门**、新增**渐进任务坡道**(S→M→L)、P1 拆 P1a/P1b、§6 终止条件 tier 条件化;③ **分析范围扩展**:从"日志/崩溃"扩为"日志/崩溃 + 产物/成果 + 性能"三通道,ed 增补)
- **作者**: Claude(代 ed 起草)
- **目标读者**: ed(审核)、后续实现者

---

## 0. 摘要

构建一条"**自动编译 → 跑任务矩阵(短/长期)→ 采集三类信号(日志/崩溃、任务产物、性能指标)→ Claude Code headless 分析/修复 → 质量门 → 合入 → 迭代**"的闭环流水线,持续运行直到 Shannon 达到量化的质量水位。

核心决策:**用确定性的外部 Supervisor(Python 脚本)做编排,按迭代调用无状态的 Claude Code headless 做修复**。被测系统与测试框架分离;LLM 只出现在"分析+修复"一环,其余全部是可复现的确定性步骤。

方案分四期落地(P0 基础设施 → P1 CLI 闭环 → P2 desktop + 长任务 → P3 可选强化),P1 内部再按任务难度分两级坡道(P1a 简单 → P1b 中等,见 §5.2/§7),每级有明确验收标准。P0+P1 即可形成最小可用闭环。

---

## 1. 背景与目标

### 1.1 目标

1. 一次命令(或定时任务)启动后,**无人值守**地完成:编译 → 运行预定义任务(秒级~小时级)→ 采集日志/崩溃/**产物/性能** → 修复 → 验证 → 迭代。
2. 覆盖两个产品形态:`shannon` CLI(headless 模式)与 `shannon-desktop`(engine serve + UI)。
3. 每个迭代产出**可追溯的工件**(NDJSON 流、stderr、崩溃转储、**产物快照、性能基准结果**、修复报告、指标),人可以随时审计 LLM 做了什么。
4. "改进到合理级别"被**量化定义**(见 §6),达到即自动停止并出总结报告。

### 1.2 非目标(本期不做)

- 不做通用 CI 平台/在线看板;报告先落 markdown + JSON。
- 不自动 `git push` 到远端;合入止步于本地分支/PR(见 §5.7)。
- 不用 Shannon 自己当修复器(P3 才做自举实验,见 §7)。
- 不改动产品的对外 API/架构;只加测试基建与少量可观测性代码。

---

## 2. 现状盘点(已验证的仓库事实)

方案建立在以下已核实的能力之上(✓ = 在代码中确认;2026-08-20 按 Phase B 落地后复核实):

| 能力 | 现状 | 位置 |
|------|------|------|
| Headless 模式 | ✓ `shannon -p "<prompt>"`,`--output-format text\|json\|json-stream`(json-stream = NDJSON 事件流),`--allowed-tools`、`--max-turns`、`--schema`、`--notify` | `crates/shannon-cli/src/main.rs:426-470` |
| 退出码契约 | ✓ 0 成功 / 1 引擎或 API 错误 / 2 轮数上限 / 3 超时(**保留未实现**,`KEEP: future use`)/ 4 限流 / 5 上下文溢出 / 6 权限拒绝 | `crates/shannon-cli/src/main.rs:54-79` |
| 结构化结果 | ✓ `--output-format json` 末尾输出 `HeadlessOutput{prompt, response, tool_calls[], total_tokens}` | `crates/shannon-cli/src/main.rs:96` |
| 日志 | ✓ `--debug` 时初始化 tracing,写 **stderr**(不污染 stdout NDJSON),级别走 `RUST_LOG` | `crates/shannon-cli/src/main.rs:3657/3856` |
| 会话持久化 | ✓ `~/.shannon/sessions/`,30 天清理;`--resume <UUID>` 续跑 | `crates/shannon-core/src/housekeeping.rs:271` |
| Engine API server | ✓ `shannon serve --port 33420 [--auth-token]`,UI 走 `ws://127.0.0.1:33420/api/ws` | `main.rs:561`(定义)、`main.rs:3989`(分发) |
| Provider 配置 | ✓ `~/.shannon/providers.toml` + 凭据在 OS keyring;`shannon list-providers --json` 可脚本化校验(**不吐明文 key**) | `main.rs:4119` |
| 诊断(Phase B 后增强) | ✓ `shannon doctor` 文本/`--json` 双模式:工具链/端口/服务自检 + **surface 身份 + installations 双装清单 + 双装版本漂移 WARN**(B7)——bootstrap 一条命令覆盖大部分校验 | `main.rs:3498-3561` |
| **Headless 纯度守卫(新增)** | ✓ `just guard-headless` / `scripts/check-headless-purity.sh`:断言 CLI 六 crate 依赖树零 GUI 库(ADR-0011 红线);**已是 dev/main 的 GitHub required check**(`Headless purity (ADR-0011)`)——dogfood PR 天然被门控 | `justfile:117-118` |
| **desktop 引导(新增)** | ✓ `shannon desktop --install`(下载+SHA256 校验+平台安装)/ `--build`(开发构建)——P2 desktop 任务的环境准备可脚本化(B5) | `main.rs:2690-2697` |
| Desktop e2e | ✓ Playwright(`desktop/ui/e2e/*.spec.ts`,`pnpm test:e2e`);mock 模式 `VITE_MOCK_MODE=1` | `desktop/ui/package.json` |
| 构建入口 | ✓ `just build-code` / `just build-desktop` / `just ci`(fmt+lint+deny+gen-protocol+test;**不含 purity**,本地补跑 `just guard-headless`) | `justfile:123` |
| 真实任务录制/回放 | ✓ `just record` / `just replay`(ADR 0003,fixture 走 .gitignore + 精选入库) | `justfile:130-174` |
| **性能阈值回归** | ✓ `just perf`:nextest 过滤 **12 个绝对阈值测试**(compaction<2s、session_load<500ms、streaming>10MB/s、snapshot_render<1ms 等),mockito 驱动**不依赖真实 provider 延迟**、无需 key | `justfile:90-91`、`crates/shannon-core/tests/perf_tests.rs` |
| **criterion 微基准** | ✓ `cargo bench --workspace`(6 个 crate 带 benches,支持 `--save-baseline`/`--baseline` 对比) | `justfile:86-87` |

**已识别的缺口(需要在本方案中补;2026-08-20 复核:四项仍然成立)**:

| 缺口 | 影响 | 补法 |
|------|------|------|
| headless 路径**无 panic hook** | 崩溃只有 stderr 文本,无结构化崩溃工件 | P0 小 PR:加 panic hook,把 backtrace + 上下文写入 `$SHANNON_CRASH_DIR`(可复用 `shannon-ui` TUI 已有 hook 的模式) |
| 退出码 3(超时)**未实现** | 任务挂死不会自杀 | Supervisor 外部强制超时(`timeout --kill-after` + 进程组 kill),不改产品 |
| 长任务无断点语义 | 数小时任务失败即全废 | 用 `--resume <UUID>` 在 Supervisor 层做 checkpoint 重试(P2) |
| desktop 真实引擎 e2e profile 不存在 | 现有 e2e 全在 mock 模式 | P2 新增 `e2e/dogfood.*.spec.ts` + 环境变量 profile |

---

## 3. 核心设计决策

### 决策 1:确定性 Supervisor + 按迭代无状态的 LLM 修复器

- **编排(编译/跑任务/采集/计时/超时/重试/合入)全部确定性**:bash/Python + just,不依赖 LLM,跑 100 次行为一致。
- **LLM(Claude Code)只负责"分析工件 → 改代码"**,每次调用都是全新上下文(无状态),输入是一份**分诊后的迭代简报**,而不是原始日志海洋。这样:单次会话上下文小、可审计、失败可重试,且天然规避"agent 记忆漂移"。

### 决策 2:Supervisor 用 Python 3 标准库,放 `scripts/dogfood/`,不进 Cargo workspace

理由:进程组管理、信号处理、超时 kill、JSON/YAML 处理在 Python 里最省事;不进 workspace 就不碰 semver/clippy 门,迭代快。入口用 `just dogfood` 包装,与仓库习惯一致。(备选:Rust `xtask` 风格二进制——更重,见 §8 对比。)

### 决策 3:被测系统与测试框架分离

循环**不用 Shannon 跑 Shannon**。被测对象失效(挂死/崩溃)时框架必须仍然活着并如实记录——所以框架是外部进程。自举实验(用 shannon headless 替换 claude 当修复器)放到 P3,且仍然在这套框架内运行。

### 决策 4:git 隔离用 worktree + 专用分支

每个修复会话在独立 git worktree(`.dogfood-worktrees/iter-N/`,gitignore)里改代码,产出 `dogfood/iter-<N>-<slug>` 分支。主工作区 `dev` 永远不被 LLM 直接触碰。合入走 PR(默认人审,可配置自动合,见 §5.7)。

### 决策 5:任务在"一次性 scratch 仓库"里跑,不在本仓库里跑

任务的目标是测 Shannon(引擎+工具链),不是让 AI 改 Shannon 仓库本身。短任务在本仓库的 **只读快照**上跑(测 Read/Grep/分析类),写类任务在预生成的 scratch repo(`tests/dogfood/fixtures/scratch-repo/`,含预置 bug 的小 Rust/TS 项目)上跑。这样任务结果可判定的同时,不会误改源码。

---

## 4. 总体架构

```
┌────────────────────────── just dogfood (scripts/dogfood/run.py) ──────────────────────────┐
│                                                                                          │
│  bootstrap ──> build ──> run matrix ──> collect+triage ──┬─(全绿)─> 收紧/延长任务 ──> loop │
│     │           │          │                │           │                                │
│     │           │          │                └─(有失败)─> fix stage(Claude Code headless) │
│     │           │          │                             │  └─> 质量门 ──> 合入 ─> loop   │
│     │           │          │                                                            │
│  配置校验     cargo build  task runner                                                   │
│  (doctor/     release      ├─ CLI 任务: shannon -p ... --output-format json-stream       │
│   list-       justfile     └─ Desktop 任务: shannon serve + UI + Playwright (P2)         │
│   providers)                                                                           │
│                                                                                          │
│  artifacts/<run-id>/  ← 所有工件;report.md + summary.json ← 每迭代指标                     │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

一次迭代(iteration)的生命周期:

1. **bootstrap**:校验 provider 配置、API key 可用性(跑一个 1-turn 冒烟)、`shannon doctor`。
2. **build**:`cargo build --workspace`(`--release` 仅构建被测产品二进制),构建失败本身即第一类待修失败。
3. **run matrix**:按任务清单执行(短任务并行 ≤3,长任务串行),每任务独立 run 目录。
4. **collect + triage**:汇总工件(NDJSON/崩溃/workspace 产物快照)→ 跑 `just perf` 基准 → 规则分类 → 签名去重聚类。
5. **fix**(仅有失败时):生成迭代简报 → Claude Code headless 在 worktree 中修复(可多会话,按聚类分批)。
6. **gate**:`just ci` + 失败任务复跑通过 ×2。
7. **commit/merge**:过门 → squash 成一个 commit → `dogfood/iter-N` 分支 → PR。
8. **loop or exit**:未达终止条件 → 下一迭代;达到 → 生成总结报告并退出。

---

## 5. 组件规格

### 5.1 配置引导(一次性)

```bash
# 1) CLI 侧 provider/model(交互式一次配好,持久化到 ~/.shannon/providers.toml)
shannon            # REPL 内: /provider, /model --tier standard --save
# 2) 凭据入 keyring(不在任何文件里)
shannon providers set anthropic --api-key   # 或桌面端 Add Provider(同一存储,ADR-0005 已统一)
# 3) 校验(脚本化,不吐明文)
shannon list-providers --json | jq '.active'
shannon doctor
# 4) Claude Code 侧:修复器用自己的认证(claude login 或 ANTHROPIC_API_KEY)
claude -p "reply ok"   # 冒烟
```

要点:CLI 与 desktop 共享同一 `ProviderConfigService`/凭据存储(ADR-0005 Phase 2 已完成),**配置一次两边生效**,无需两套。Supervisor 启动时把上述校验作为 bootstrap 步骤,任何一步失败立即退出并报告,不带病运行。Phase B 后 `shannon doctor --json` 已含 surface 身份与 installations 双装清单(B7),bootstrap 可直接消费其 JSON 而不必自行解析文本输出。

### 5.2 任务清单(Task Manifest)

`tests/dogfood/tasks.yaml`(入库,任务定义是资产);每个任务:

```yaml
- id: s1-read-analyze            # 唯一 ID
  tier: S                        # S(<2min) | M(2–15min) | L(0.5–4h)
  kind: cli                      # cli | desktop(P2)
  workspace: readonly-main       # readonly-main | scratch:<name> | temp(全新空目录)
  prompt: "阅读 crates/shannon-types/src/lib.rs,总结导出类型清单,只输出列表"
  expect:
    exit_code: 0
    ndjson_terminal: result      # 流以 result 事件收尾
    contains: ["EntityId"]       # 文本断言(可选)
    verify:                      # 产物判定(2026-08-20 增补)
      artifacts:                 # 期望产出:存在性 + 可选确定性检查命令
        - path: "out/types.md"
          check: "grep -q 'EntityId' out/types.md"   # check 可省 = 仅存在性
      golden: "goldens/s1.md"    # 可选:金标准 diff(只读分析类适用)
      verify_cmd: null           # 写类任务必配:在任务 workspace 执行,非零即失败
                                  # 例:scratch repo 修 bug 任务 → "cargo test -q"
  timeout_s: 120
  retries: 0                     # L 任务可配 resume 重试
  provider_tier: fast            # 用哪个 tier 跑(省钱的用 fast)
```

**任务集设计**(初版 ~12 个;**渐进启用**,见下方坡道):

| 类别 | 示例 | 测什么 |
|------|------|--------|
| 只读分析(S) | 读文件总结、grep 定位、架构问答 | 流式、上下文、退出码 0 |
| 写操作(S/M) | 在 scratch repo 修预置 bug、加单测、跑 `cargo test` | 工具链、文件历史、bash 沙箱 |
| 多轮长会话(M) | scratch repo 上"加一个小 feature 端到端完成" | 多轮状态、压缩、token 结算 |
| 压力(L) | 1–4h 持续任务(大重构/多文件迁移),`--resume` 断点续跑 | 内存/句柄泄漏、重连、限流恢复 |
| 故意失败(S) | 无效 key、上下文溢出 prompt、拒绝权限的操作 | 退出码 4/5/6、错误路径、崩溃 |
| desktop(P2) | serve + UI:发消息、看 diff、批准/拒绝权限流 | WS 层、UI 状态机、Tauri 边界 |

**判定**:优先用确定性断言(exit_code + NDJSON 终态 + 文本 contains + **verify 产物块**)。写类任务(tier S 写操作/M/L)**必须至少配一条 `verify_cmd`**——产出必须通过目标 workspace 自己的测试才算成功。产物判定分三级:**pass / partial(产出存在但检查未全过)/ fail**;partial 也是信号("干了一半"是真实 bug 面,如工具链中途静默放弃)。LLM-as-judge 只作为可选辅助(默认关闭,避免裁判本身引入噪声)。

**渐进任务坡道(2026-08-20 定夺)**:任务不一次全上,按"**简单 → 中等 → 复杂**"三级分阶段启用,每级先**测+修稳定**再进下一级:

| 阶段 | 启用 tier | 进入条件(上一级毕业) |
|------|-----------|----------------------|
| P1a | S(简单,~6 个含 1 故意失败) | P0 验收通过 |
| P1b | S+M(中等,扩到 ~12 个) | P1a 连续 2 迭代全绿且 0 新签名 |
| P2 | S+M+L(复杂/长任务) | P1b 连续 2 迭代全绿且 0 新签名(即 §6"无新发现"条件) |

- 启用开关在 `lib.yaml`(`task_tiers: [S]` → `[S,M]` → `[S,M,L]`),tasks.yaml 不动——任务定义是资产,启用节奏是策略;
- `--task-filter` 保留为手动覆盖(调试用);坡道晋级由 Supervisor 按 stage 毕业条件自动判定,并在 `report.md` 记录"晋级"事件(可审计);
- 晋级后首轮若立即冒出 **≥3 个新签名 → 自动回退一级**并标记"粒度跨太大",提示补中间难度任务。

### 5.3 Supervisor(执行器)

`scripts/dogfood/`(Python ≥3.10,仅标准库 + `pyyaml`):

```
scripts/dogfood/
  run.py          # 主循环 + CLI(argparse): --max-iters --task-filter --fix-mode --gate
  ledger.py       # token 台账: ledger.jsonl 追加 + 三层闸门检查(§6)
  runner.py       # 任务执行: spawn、进程组、超时、重试、resume、产物快照
  triage.py       # 分类 + 签名聚类(崩溃/产物/性能三类)
  perf.py         # 性能采集: 每迭代 just perf 基准 + 基线对比; 任务时序趋势(观察项)
  fixer.py        # Claude Code headless 调用
  report.py       # markdown + summary.json 生成
  lib.yaml        # 阈值/路径/并行度等(可被 CLI 覆盖)
```

关键实现要点(含本仓库已知坑):

- **进程组与超时**:`start_new_session=True` 起 POSIX 进程组,超时先 `SIGINT`(给 Shannon 清理窗口)再 `SIGKILL` 整组;杀 Playwright/浏览器进程树同理。
- **退出码必须落文件再读**(已知坑:管道会吃掉真实退出码;一律 `> stdout.ndjson 2> stderr.log; echo $? > exit_code`)。
- **stdout/stderr 分流**:stdout 只收 NDJSON(结构化),stderr 收 tracing/panic(需 `--debug` + `RUST_LOG=shannon_cli=debug,shannon_core=info`)。
- **长任务心跳**:NDJSON 每事件即心跳;N 秒无事件且 CPU≈0 判 hang(hang 与 timeout 分开分类)。
- **磁盘治理**:artifacts 按 `<run-id>/<task-id>/` 组织,单 run 上限(默认 2GB)+ 保留最近 N 个 run(默认 10),超限先清最大的 stdout/stderr。

每次任务运行的工件:

```
artifacts/2026-08-19T1400+0800/iter-03/
  s1-read-analyze/
    meta.json        # 任务定义快照、起止时间、exit_code、signal、判定结果(pass/partial/fail)
    stdout.ndjson    # 原始事件流
    stderr.log       # tracing + panic(+ backtrace)
    result.json      # HeadlessOutput(--output-format json 单独再跑? 否——json-stream 的终态事件即等价)
    workspace-snapshot/  # 任务产物快照(git status/diff + 新增文件副本;verify 判定对象)
  summary.json       # 本迭代全任务汇总(含每任务时序/token 观察指标)
  perf.json          # just perf 12 项阈值结果 + 基线对比(P1b 起)
  triage.json        # 分类 + 签名聚类
  fix/               # 修复会话简报、claude 输出、patch、fix-report.md
  report.md          # 人读报告
```

> 说明:`--output-format json-stream` 的终态事件已含 token/turn 汇总,不需要同任务跑两遍格式。

### 5.4 信号采集:日志、崩溃、产物、性能

**CLI 通道**(P1):

1. stdout NDJSON(完整对话/工具事件流)——主证据。
2. stderr tracing(`--debug` + `RUST_LOG`)——内部路径证据。
3. **新增(P0 小 PR)**:headless 初始化时装 panic hook:
   - 把 `panic message + backtrace + 最近 20 条事件摘要` 写入 `$SHANNON_CRASH_DIR/<ts>-<task>.crash.json`;
   - 进程仍按 Rust 默认语义退出(exit 101),不影响现有契约;
   - 默认关闭,仅设置环境变量时启用——零行为变更,无 semver 风险。
4. `~/.shannon/sessions/<uuid>.jsonl` 被引用复制到工件目录(供跨迭代对比会话质量)。

**产物通道**(P1a;2026-08-20 增补):

1. runner 在任务结束后进 workspace 取证:scratch repo 本身是 git 仓库 → `git status --porcelain` + `git diff` 即产物清单,新增/修改文件复制进 `<task>/workspace-snapshot/`;readonly-main/temp workspace 同法(temp 需先 `git init` 打底)。
2. 按 §5.2 `verify` 块判定:artifacts 存在性/检查命令、`verify_cmd`、golden diff。
3. 用到 `--schema` 的任务,**结构化校验本身即产物判定**(引擎内置能力,零新代码)。
4. 判定分级 pass/partial/fail 落 meta.json,进 triage(§5.5)。

**性能通道**(P1b 起;确定性优先,2026-08-20 增补):

1. **判定信号 = `just perf`**:每迭代跑一次(12 个绝对阈值测试,mockito 驱动,**不依赖真实 provider 延迟**);阈值测试失败 → `perf_regress` 签名。
2. **观察信号 = 任务时序**:wall time、TTFT、事件间隔、turns、tokens 全从 NDJSON 时间戳推导,进 summary.json 趋势表——但**不据此开修复简报**(真实 provider 延迟抖动会淹没产品侧回归)。
3. **定位工具 = criterion**:不每迭代全跑(慢、共享机器噪声大);仅在 perf 签名需要定位时,对相关 crate 定向跑 `cargo bench -- --baseline dogfood`(基线用 `--save-baseline dogfood` 存)。
4. **P2 追加 /proc 资源采样**(L 任务):RSS/线程数/句柄数每 30s 采样,报告末端斜率(泄漏检测——长任务才有信号)。

**Desktop 通道**(P2):

1. `shannon serve --port 33420 --auth-token <token>` 前台拉起,stderr 落文件(与 CLI 同法)。
2. UI 用 vite dev server 指向该 engine(`VITE_ENGINE_WS` 类 env;mock 模式不开)。
3. Playwright 驱动:收集 console 消息、page errors、network 失败、失败时 screenshot + trace(`use: { trace: 'retain-on-failure' }`)。
4. 已知坑:welcome 门控会把新上下文弹到 `/welcome`——dogfood profile 需预置已完成 onboarding 的 storageState,或提供 e2e 专用跳过开关;浏览器安装失败时用 npmmirror 镜像。

**通知**(可选,复用现有能力):Supervisor 迭代结束时用 Shannon 自己的 `--notify`/webhook 模板发飞书/Slack 摘要——顺便也是 dogfood。

### 5.5 分诊(Triage)

规则优先,LLM 兜底(默认关闭):

| 类别 | 判定 | 签名(signatute) |
|------|------|------|
| build_fail | build 步骤非零 | 首个 rustc error 行规范化 |
| api_error | exit 1 + NDJSON error 事件 | `provider + error code` |
| rate_limited | exit 4 | provider + 窗口 |
| panic | exit 101 / .crash.json | panic 位置(`file:line` of backtrace 第 1-3 帧) |
| timeout / hang | 被外杀 / 心跳停滞 | task_id(结构性问题,量少) |
| bad_output | exit 0 但 contains 断言失败 | 断言 ID |
| outcome_fail | verify_cmd 非零 / 期望产物缺失 | task_id + 检查 ID |
| outcome_partial | 产出存在但 verify 部分未过 | task_id + 检查 ID |
| perf_regress | `just perf` 阈值测试失败 | `PERF:<阈值测试名>` |
| turn_limit | exit 2 | task_id + max-turns |

签名做**跨迭代去重与追踪**:同一签名只修一次,复现即升级(回归!标红)。这是"迭代有进展"的客观度量。

产物/性能签名与崩溃签名走**同一条管道**(简报→修复→回归测试):"exit 0 但干错活/干一半"的静默失败与性能回归,和 panic 一样值得修——且往往是更高价值的 bug 面(panic 至少响,静默失败不响)。

### 5.6 Claude Code 修复器

每个迭代最多 K 个修复会话(默认 3,预算闸门之一),按签名聚类分批:

```bash
# 在专用 worktree 中(主工作区不受影响)
cd .dogfood-worktrees/iter-03/
claude -p "$(cat artifacts/.../fix/brief-01.md)" \
  --permission-mode acceptEdits \
  --allowedTools "Read,Grep,Glob,Bash(cargo *:*,just *),Edit,Write" \
  --max-turns 40 \
  --output-format json > fix/session-01.json
```

- **输入(迭代简报,brief-*.md)**:本批签名的最小复现(命令 + 工件路径)、相关日志片段(截断)、建议排查的代码区域(triage 的启发式定位)、**输出契约**(见下)、约束(不动公共 API、遵循 CLAUDE.md、必须加回归测试)。perf_regress 简报额外附基线数字与可复现的基准命令;outcome_fail 简报附 verify 失败输出与期望产出。
- **输出契约**(修复器必须遵守,Supervisor 校验):
  1. 产出 ≥1 个 commit(message 前缀 `fix(dogfood): <signature>`);
  2. 附带 `fix-report.md`:根因、改动点、验证方式;
  3. 为该签名添加一个**回归测试**(失败任务的浓缩版);CI 门会跑它。
  4. 无法修复时输出 `BLOCKED: <原因>` 而不是硬凑补丁——BLOCKED 签名转人工队列,不再消耗会话。
- **隔离与防串扰**:修复器运行带 `--settings` 指向最小配置,避免继承用户级 hooks(已知坑:本机有 Edit/Write 后自动 commit 的 hook,会把多文件修复拆成碎片 commit);worktree 内 `git` 操作仅限 add/commit,**无 push 权限**(allowlist 不含 push)。
- 模型路由:默认修复会话用主力模型;triage/简报生成可用低档模型(可配)。

**修复触发模式(`--fix-mode`,2026-08-20 补)**:

| 模式 | 行为 | 适用 |
|------|------|------|
| `auto`(默认) | fixer.py 自动按简报调 claude headless 修复 | P1 目标形态 |
| `manual` | 跑到简报生成为止:备好 worktree + `brief-*.md`,打印可粘贴的 claude 命令;人在 worktree 里开交互式 Claude Code 按简报修;修完 `just dogfood --gate iter-N` 过质量门 | **推荐首发**——与人审 PR 哲学一致,首批签名边修边校准简报模板 |
| `off` | 不生成简报,只跑 build→任务→triage→报告(纯采集) | 只想看现状不想修 |

- `--dry-run` 保留为 `--fix-mode manual` 的别名(简报生成 + 不调 claude);
- manual 模式下简报头部印执行提示:`cd .dogfood-worktrees/iter-N && claude` + 简报路径 + 修完后的 gate 命令——**零新知识也能接手**;
- `--gate iter-N` 子命令对 worktree 里已落的 commits 跑 §5.7 全部门(含复跑 ×2),与 auto 模式走同一段代码——手动修不会被放低标准。

### 5.7 质量门与 git 流转

```
修复会话产出(worktree 分支 dogfood/iter-03)
  │
  ├─ just ci 全绿(fmt + clippy -D warnings + deny + gen-protocol + 全测试)
  ├─ just guard-headless 全绿(ADR-0011 红线:CLI 不得链接 GUI 库;本地先跑省一轮 CI)
  ├─ 失败任务复跑 ×2 全过(同任务同断言)
  ├─ perf_regress 修复:just perf 相关阈值测试复跑通过 ×2(P1b 起)
  ├─ diff 审计:只触碰白名单路径(crates/ tests/ docs/;禁碰 .github/、release 脚本)
  │
  ├─ (默认)开 PR 到 dev,人审后合 —— 推荐首发姿势
  └─ (可选 --auto-merge)仅当以上全过 且 diff < 500 行 且 无公共 API 变更 时自动合
```

> 注:dev 的 GitHub required checks 已含 `Headless purity (ADR-0011)`(2026-08-20 加入),dogfood PR 在远端也会被该门强制;本地 `just ci` 不含 purity,故门里单列 `just guard-headless`。

默认**人审 PR**:审计成本每次几分钟,换来对 LLM 改动的完全把关。跑几轮建立信任后再开 `--auto-merge`。

### 5.8 报告与指标

每迭代 `report.md`(人读)+ `summary.json`(机器读),核心指标:

- 任务通过率(按 tier)、首次全绿迭代号
- **产物判定分布**(pass/partial/fail,按 tier)、partial 首次归零迭代
- **性能**:`just perf` 通过数(≤12)、任务时序趋势(观察项)、P2 资源斜率
- **新签名数 / 已修复签名数 / 回归签名数**(最重要的趋势线;含崩溃/产物/性能三类签名)
- panic/hang/timeout 计数
- token 消耗(任务侧 NDJSON 终态汇总 + 修复会话侧 usage,**三层闸门累计口径**)与 USD 参考估算(仅展示不控闸)、修复会话数、墙钟时间
- 每签名:首次出现迭代 → 修复迭代 → 复发情况

---

## 6. 迭代控制与终止条件("合理级别"的量化)

循环在满足**任一**停止条件时退出并写总结:

| 条件 | 默认值 | 说明 |
|------|--------|------|
| 全绿保持 | 连续 **3** 个迭代全矩阵通过;**L 任务启用后**其中须含 2 次 L(tier 可用性从任务清单推导——P1 期按 S 或 S+M 矩阵判定) | 质量水位的主判据 |
| 无新发现 | 连续 **2** 个迭代 0 新 P0/P1 签名(**口径含崩溃/产物/性能三类签名**) | 边际收益枯竭(兼作 §5.2 坡道毕业条件) |
| 迭代上限 | 50 | 防死循环 |
| 预算上限 | **token 三层闸门**:每迭代 4M / 每日 10M / 每月 50M(输入+输出合计) | 超限即停(见下方计量口径) |
| 人工急停 | `artifacts/STOP` 文件存在 | 每迭代开头检查 |

"合理级别"默认定义 = 前两条同时满足。数值都放在 `lib.yaml`,审核时可调。

**token 计量口径(2026-08-20 定夺)**:不使用 USD 作闸门(单价表随 provider 漂移、无法精确控制),改 **token 总数三层闸门**(每迭代/每日/每月)。计量来源:任务侧取每个 NDJSON 终态事件的 token 汇总,修复会话侧取 claude `--output-format json` 的 usage 字段,由 Supervisor 累加落 `summary.json` 并跨迭代持久化(日月额度才有意义)。闸门语义:超**迭代**额度 → 本迭代收尾(已开修复会话跑完)后停,不开新会话;超**日/月**额度 → 立即停并写总结。USD 仅作参考估算列展示,不参与判定。附注:自家 stack-shannon-go(LiteLLM 计费)通道暂不可用,全部流量走外部 provider 凭据;该通道恢复后再评估是否迁移(可把 dogfood 成本转为内部 credit)。数值是首周观察前的保守起点,按真实曲线调整。

**渐进收紧策略**:全绿不是终点——达到后自动延长任务时长/加大 `--max-turns`/加入混沌注入(P3),直到再次稳定,防止"任务太简单导致假绿"。

### 6.1 晋级证据(M-tier,2026-08-22)

`task_tiers: [S, M]` 启用后首个完整跑通周期,真实账目(commit 47e94a94 修了 ledger 三处 bug:in/out 分账、run_id 隔离、M3 wire 零值污染,所以下面的数字是可靠的):

| 迭代 | S+M 任务通过 | 全部任务 | new sigs | 累计 token (in+out) |
|------|--------------|----------|----------|---------------------|
| iter-01 | 10/11 (m1-scratch-feature verify-fail) | 11 | 1 | 518,528 |
| iter-02 | 11/11 ✓ | 11 | 0 | 298,990 |
| iter-03 | 11/11 ✓ | 11 | 0 | 362,987 |
| iter-04 | 11/11 ✓ | 11 | 0 | 467,895 |

streaks: all-green ×3、no-new ×3 — 达到 §6 双停条件,Supervisor 自停。

日累计 1,648,400 token(in 1.62M / out 25.9K),远低于 10M 日闸门;迭代 cap 4M 仅 iter-01 占 13% 触发后续收紧,余下均 8–12%。每个 M 任务(含多轮 bash + scratch repo 端到端)约 50–75K token,与 P1b 估算一致。

iter-01 的 1 个新签名是 m1-scratch-feature 的 verify 失败(task 跑了但产物不对)— fixer session 产出 PR-ready 分支(`fix: branch dogfood/iter-01 ready — open a PR: gh pr create`),后续三轮同任务全部 pass,说明 fixer 一次写穿。

artifacts: `artifacts/2026-08-22T194847+0800/iter-{01..04}/{summary.json,report.md,triage.json}`;ledger `artifacts/ledger.jsonl` 17+ 条入账均带正确 in/out 拆分。

P2 L-tier 启用前置条件(P2-5 设计 L 任务集 + P2-6 perf 通道增强)尚未完成,本晋级证据仅证明 [S, M] 段稳定。

---

## 7. 分阶段实施计划

### P0 — 基础设施(约 0.5–1 天)

1. 配置引导按 §5.1 完成 + 冒烟通过。
2. panic hook 小 PR(`SHANNON_CRASH_DIR`,默认关闭)+ 合入。
3. `scripts/dogfood/` 骨架:bootstrap + build + 任务执行 + 工件落盘 + 退出码/超时语义。
4. `tests/dogfood/tasks.yaml` v1:5 个 S 任务(含 1 个故意失败),每个自带 `verify` 产物块;summary.json 含每任务时序/token 观察指标。
5. `.gitignore` 加 `artifacts/`、`.dogfood-worktrees/`(仿 fixtures 模式)。

**验收**:`just dogfood --once --task-filter S` 一条命令完成 build→3 个任务→artifacts + report.md 生成;故意失败任务被正确分类。

### P1a — CLI 闭环·简单任务(约 1–2 天)

1. triage(规则 + 签名去重 + 跨迭代追踪;含 outcome_fail/partial 分类)。
2. 产物通道:workspace 产物快照 + `verify` 判定分级(§5.4)。
3. fixer:Claude headless 调用 + worktree 隔离 + 简报生成 + 输出契约校验。
4. 质量门 + PR 流转 + 迭代主循环 + 全部停止条件(token 三层闸门在此生效)。
5. 任务集:S 级 ~6 个(含 1 个故意失败)。

**验收**:`just dogfood --once --task-filter S` 单迭代跑通全链路(build→任务→triage→fix→gate→PR);故意失败任务被正确分类。
**毕业条件(进 P1b)**:连续 2 个迭代 S 矩阵全绿且 0 新签名。

### P1b — CLI 闭环·中等任务(约 1 天)

1. 任务集扩到 ~12 个(+M 级:多轮长会话、scratch repo 端到端小 feature)。
2. 性能通道:每迭代 `just perf` + 基线存档 + `perf_regress` 签名(§5.4)。
3. 坡道自动晋级/回退逻辑挂钩(§5.2)。

**验收(金标准)**:人工在 dev 上**注入**一个已知 bug(如让某工具返回错误),跑 `just dogfood`,系统自动:发现→定位→修复→加回归测试→过 CI→开 PR。全程无需人工。

### P2 — Desktop + 长任务(约 2–4 天)

1. serve 模式 runner + UI real-engine profile + onboarding storageState。
2. Playwright 采集(console/pageerror/network/trace/截图)。
3. L 级任务 + `--resume` 断点续跑 + 心跳/hang 检测 + /proc 资源采样(§5.4)。
4. 并行矩阵调度(S 并行 ≤3,L 串行)。

**验收:desktop 任务(发消息→批准权限→看到 diff)全流程可判定地跑通;一个 ≥1h 任务被外杀后能 resume 续跑。**

### P3 — 可选强化(按需)

- **自举实验**:fixer 从 claude 换成 `shannon -p`(同简报),对比修复成功率——真正的 dogfood,也检验 headless 模式成熟度。
- 夜间定时:systemd timer / cron,白天人审 PR。
- 混沌注入:代理层注入 429/断网/慢流,测重连与降级(复用 record/replay fixture 思路)。
- 成本看板:summary.json 聚合趋势图。

---

## 8. 备选方案对比

| 方案 | 优点 | 缺点 | 结论 |
|------|------|------|------|
| **A. Python supervisor + claude headless(本方案)** | 确定性、可审计、LLM 上下文小、实现快 | 多一个 Python 组件 | ✅ 采用 |
| B. 纯 Claude Code 技能/skill 当外层循环 | 零新代码 | 小时级任务可靠性差、上下文膨胀、不可复现、框架与被测系统同生命周期 | ❌ |
| C. Rust xtask 二进制进 workspace | 单一技术栈、可发布 | 进 semver/clippy 门,迭代慢,进程管理代码量更大 | 备选(P3 若要产品化再迁) |
| D. GitHub Actions 自托管 runner 当 supervisor | 免费 UI/秘钥管理 | 小时级任务占 runner;desktop GUI/浏览器依赖麻烦;迭代速率受队列限制 | ❌(夜间批处理可考虑) |
| E. 用 Shannon 的 scheduled routines 自驱 | 复用最多 | 框架=被测系统,崩溃即失明;先有鸡还是先有蛋 | ❌(P3 自举时局部采用) |

---

## 9. 风险与缓解

| 风险 | 缓解 |
|------|------|
| LLM 修复引入回归 | CI 门 + 复跑门 + 回归测试强制 + 默认人审 PR;签名复发标红升级 |
| 成本失控 | 每迭代修复会话数上限 + token 三层闸门(每迭代/日/月,精确计量不依赖单价表);短任务用 fast tier |
| 长任务 flaky(API 抖动误判为产品 bug) | api_error/rate_limited 单独归类,重试 1 次后才算失败;infra 噪声不计入"新签名" |
| 修复器乱改(越权文件/公共 API) | diff 白名单审计 + allowlist 最小工具集 + worktree 隔离 + 无 push 权限 |
| artifacts 膨胀 | 磁盘配额 + run 保留 N 份 + 大文件先清(§5.3) |
| desktop e2e 环境脆弱 | 已知坑已列(§5.4);P2 单独验收,不阻塞 P1 价值 |
| 用户级 hooks 串扰 headless claude | `--settings` 隔离(已知坑,§5.6) |

---

## 10. 已定夺事项(2026-08-20,ed 拍板)

1. **合入策略**:**首发人审 PR**。毕业条件 = 连续 10 个 PR 人审零实质修正(仅 typo 级)后,方可开 `--auto-merge`(保留 §5.7 约束:diff <500 行 + 无公共 API 变更)。理由:自动合省的是几分钟人审,赌上的是一次坏合入的清理成本——首发信任期不值得;触发条件量化,不靠感觉。
2. **预算控制**:**token 总数三层闸门**(每迭代 4M / 每日 10M / 每月 50M,输入+输出合计),不用 USD(不可精确控制、单价表漂移)。计量口径见 §6。自家 stack-shannon-go(LiteLLM 计费)**暂不可用**,流量全走外部 provider 凭据;通道恢复后再评估迁移。数值为首周保守起点,按真实曲线调整。
3. **修复器模型**:**主力模型全跑修复会话**;fast tier 只做 triage/简报生成。理由:修复质量是循环瓶颈,弱修复器浪费的重试远贵于模型差价;签名分级路由省小钱、引入新故障面。跑满 20 个迭代有数据后重评。
4. **长任务载体**:**写类 L 任务仅 scratch repo;只读分析型 L 允许在本仓库只读快照**(决策 5 已隔离,零污染)。时长 **2h 起步**——resume 断点语义是 P2 未验证的新代码(退出码 3 至今未实现),2h 稳定 + resume 验证后升 4h。
5. **运行窗口**:**P0/P1 纯手动触发**(调参期:任务集、阈值、简报模板都要人看着改);夜间定时的前置条件 = 金标准通过 + 连续 2–3 次完整无人值守循环零人工干预。前提:夜间本机不关机(ed 确认)。
6. **desktop 优先级**:**等 CLI 循环跑出价值再排 P2**——触发条件 = 发现并修复 **≥1 个非注入的真实 bug**。理由:engine/tool 层与桌面共享,CLI 循环的修复桌面直接受益,P2 边际价值在 WS/UI 状态机层;desktop e2e 是全家最脆一环(welcome 门控/浏览器安装/mock 依赖),且 Phase B 刚动过 desktop 需先稳定。
7. **终止数值**:§6 默认值保留,但修复了 **P1 期逻辑洞**——"含 2 次 L"改为 tier 条件化(L 启用后才要求;P1 按 S 或 S+M 矩阵判定),否则 P1 期永远到不了质量水位,只能靠迭代上限/预算/急停退出。
8. **(新增)渐进任务坡道**:任务按"**简单 → 中等 → 复杂**"分阶段启用,每级先测+修稳定再进下一级,晋级条件量化、支持自动回退——见 §5.2 坡道表与 §7 P1a/P1b 拆分。
9. **(2026-08-20 增补)分析范围扩展**:循环不只分析崩溃/日志,还分析**任务产物(成果正确性)与性能**——产物判定进 triage 与简报,分 pass/partial/fail 三级;性能以确定性 `just perf` 阈值测试为**判定信号**、任务时序仅作**观察**、criterion 只做定位、资源采样随 L 任务(P2);三类签名同管道、同终止条件口径。见 §5.2/§5.4/§5.5。

---

## 附录 A:快速命令预览(实现后的使用方式)

```bash
just dogfood                    # 完整循环,直到终止条件
just dogfood --once             # 单次迭代(调试用)
just dogfood --task-filter S    # 只跑 S 级
just dogfood --fix-mode manual  # 跑到简报为止,人接手修(= --dry-run)
just dogfood --fix-mode off     # 纯采集,不生成简报
just dogfood --gate iter-3      # 对 worktree 已落 commits 跑质量门(手动修后)
just dogfood --refresh-perf-baseline  # 重存性能基线(坡道晋级/换机后)
just dogfood --auto-merge       # 开自动合入
touch artifacts/STOP            # 人工急停
```
