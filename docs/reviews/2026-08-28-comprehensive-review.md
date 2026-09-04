# Shannon Monorepo 全面审查报告（2026-08-28）

> 审查角色：高级产品经理 × 高级架构师 × 高级 UI 设计师（三视角合并评审）
> 审查对象：`/home/ed/workspace/app/work/shannon/shannon-mono`（dev 分支主 checkout，2026-08-28 快照）
> 方法：只读源码走查（零 git 写操作），全部结论附 `文件:行号` 证据；未做运行时/浏览器实测。
> 参考（只读）：`docs/research/shannon-gap-analysis.md`（P1–P10）、根/子级 `CLAUDE.md`、`docs/metrics.md`。
> 注：行号以审查当日 HEAD 为准，后续提交会使行号漂移。

---

## 0. 执行摘要

**总体判断**：2026-08-27/28 的 L0 事件日志、统一事件总线、插件权限、Landlock 沙箱、OTLP/Turn Timeline、eval runner 六件套落地后，Shannon 的「地基质量」显著高于同类个人项目：11k+ 测试、bench 进 CI、L0 被 CLI 与 desktop 共享、权限决策落盘、redaction 写路径 fail-closed。**当前的主要矛盾不在「缺能力」，而在「表面一致性」**：命令面/help/退出码/文档四处互相说谎（dispatch 漂移已产生真实 bug）、可靠性承诺（「events.jsonl 唯一权威」）在 fsync 与 SSE 总超时两处存在缺口、插件安全的「声明即白名单」模型与「默认全允许 + 一键 git 安装」组合存在结构性风险。评测基建很强但没有用户入口，产品双面（CLI/TUI vs Desktop）正在渐行渐远。

### Top 10 问题

| # | 级别 | 问题 | 证据 | 章节 |
|---|------|------|------|------|
| 1 | P1 | `/notools` 是死分支：match 有、名单没有 → 用户输入报「未知命令」 | `crates/shannon-ui/src/repl/commands/mod.rs:328` vs `:476` | PM-1 |
| 2 | P1 | `/help` 只覆盖 121 个可分发命令中的 63 个（52%），`/provider` `/mcp` `/agents` `/routine` 等主命令无帮助条目 | `help.rs:1102`、`repl/render.rs:248` | PM-2 |
| 3 | P1 | headless 退出码文档与实现相反：文档称 2=tool denied / 3=max turns，实际 2=TurnLimit / 3=Timeout(未用) / 6=PermissionDenied | `main.rs:1600-1604` vs `:61-76`, `:1962`, `:2004` | PM-3 |
| 4 | P1 | L0 `events.jsonl` 只 flush 不 fsync —— 崩溃/断电窗口与「单一权威记录」的产品承诺不符 | `session_log/writer.rs:75,123-141,232-236` | PERF-1 |
| 5 | P1 | SSE 客户端总超时（默认 120s）覆盖整个响应体读取，长流必被掐断；重连全量重发 messages，Anthropic 不回放 Last-Event-ID → 重复生成风险 | `api/client.rs:96-106,470-476`、`api/streaming.rs:337-352,387-403` | PERF-2 |
| 6 | P1 | 插件安全三元风险：未声明 permissions = 全允许 × `install_from_git` 一键克隆任意仓库 × 安装闸门只 warn 不 block | `plugin/permissions.rs:147-169`、`plugin/registry.rs:129-183,402-406`、`plugin/validate.rs:228-232` | SEC-1 |
| 7 | P1 | 评测体系（3106 行 runner + 20 题 + 三基准）无任何用户/CI 产品入口，只能 `cargo run --example` | `testing/eval_runner.rs:1-50`；CLI `Commands` 枚举无 eval | PM-4 |
| 8 | P2 | core 面积治理（gap-analysis P10）未逆转：83 个模块，8 月仍在向 core 加 `scheduled_*×6`、`skill_loop`、`telemetry` 等 | `crates/shannon-core/src/lib.rs:83-124` | ARC-1 |
| 9 | P2 | 沙箱默认 `off`（fail-open）：Landlock 内核边界需用户显式开启，默认安装零内核约束 | `shannon-tools/src/sandbox/mod.rs:289-293,305` | SEC-2 |
| 10 | P2 | 非交互入口三轨制（positional prompt / `--prompt` / `query` 子命令）能力与文档互不一致；`query --output json` 是被静默忽略的死 flag | `main.rs:369-377,442-477,617-640,4343-4347` | PM-5 / ARC-8 |

**P0 说明**：未发现「当前必炸/数据损坏」级问题，故无 P0 项；#4/#5 在长会话与 CI 重试场景下最接近 P0，建议本周处理。

### 值得保留的优点（审查确认，避免后续误伤）

- L0 是真的跨产品共享：desktop 会话直接走 `SessionTee::open_in_container`（`desktop/src/commands_sessions.rs:36`），与 CLI 同一存储。
- 权限决策事件化落盘：`emit_decision` → `broadcast_plugin_decision` → L0（`plugin/permissions.rs:100-135`）。
- Redaction 写路径、fail-closed 内建 shape、策略快照防「运行中改文件」（`session_log/redaction.rs:1-36`）。
- 桌面页面三态（加载/空/错误）系统性齐备：`Triage.tsx:384-393`、`Tasks.tsx:211-221`、`Usage.tsx:184-193`、`TurnTimeline.tsx:114-129`。
- `ErrorBoundary` 包裹全应用（`App.tsx:55,124`）；Modal 带 `aria-modal`/焦点陷阱（`ui/modal.tsx:83`、`ui/side-panel.tsx:70`）。
- headless 无 key 时错误信息明确可操作（`main.rs:1705-1712`）。
- bench 有 CI 门（`.github/workflows/benchmarks.yml`、`bench-regression.yml`），且 L0 开销有专门 bench（`crates/shannon-core/benches/turn_log_overhead.rs`）。
- `/clear` 有确认对话框（`repl/commands/mod.rs:512-526`）；`/connect` 密钥参数在聊天/历史中脱敏（`mod.rs:751-798` 测试在位）。

---

## 1. PM 视角（产品/使用逻辑/user stories）

### PM-1 【P1·bug】`/notools` 已实际损坏——dispatch 双名单漂移的第一个实锤
- **证据**：REPL 斜杠命令先查注册表、再查硬编码名单 `repl_only_commands`（`crates/shannon-ui/src/repl/commands/mod.rs:328`），命中才进入 `match cmd_name`（`:454`）。`"notools"` 分支存在于 match（`:476`），但**不在**名单中，也不在 `shannon-commands` 注册表（`builtin.rs:62-99` 无此项）→ 用户输入 `/notools` 走 else 分支收到「未知命令」。该名单与 match 由脚本比对：121 个名单项 vs match 分支，唯此一处真实漂移（其余为多别名合并分支）。
- **影响**：功能对用户不可用；更严重的是证明了「双名单手工同步」模式必然持续产生此类 bug。
- **建议**：修名单 + 增加一个一致性测试（解析 match 分支断言 ⊆ 名单∪注册表）。工作量 **S**。

### PM-2 【P1】`/help` 覆盖率 52%——最大的自助发现面只讲了一半的故事
- **证据**：`/help` overlay 渲染 `help_utils::categorize_commands()`（`crates/shannon-ui/src/repl/render.rs:248-249`），其数据源 `all_help_entries()`（`crates/shannon-commands/src/builtin/help.rs:1102`）只含 63 个命令名。脚本比对 dispatch 名单（`mod.rs:328`，121 项）后，以下**主命令**（非别名）完全没有帮助条目：`/provider` `/mcp` `/agents` `/agent` `/session` `/routine` `/schedule` `/theme` `/usage` `/billing` `/loop` `/ralph` `/watch` `/copy` `/paste` `/stage` `/stats` `/statusline` `/suggest` `/terminal-setup` `/route` `/bind` `/project` `/add` `/files` `/select-tools` 等。
- **影响**：新用户靠 `/help` 探索产品时，一半能力不存在。这与「Shannon 命令面是核心竞争力」的定位直接冲突。
- **建议**：短期补齐缺口条目；长期把帮助数据改为从 dispatch/registry 单源生成。工作量 **S→M**。

### PM-3 【P1】headless 退出码：文档与实现相反，CI 集成者首日即踩坑
- **证据**：`run_headless_query` 文档注释称「--allowed-tools violation → exit 2、--max-turns → exit 3……2 tool denied, 3 max turns」（`crates/shannon-cli/src/main.rs:1600-1604`）；实际枚举为 `Success=0, Error=1, TurnLimit=2, Timeout=3(#[allow(dead_code)] 未使用), RateLimited=4, ContextOverflow=5, PermissionDenied=6`（`:61-76`），TurnLimit 在 `:1962` 赋值、PermissionDenied 在 `:2004` 赋值。**没有任何路径产生「tool denied → 2」**。
- **影响**：按文档写 CI 的用户会把「没跑完（2）」当成「工具被拒」，重试策略完全错误。
- **建议**：改文档注释（一处 diff）；顺手把 Timeout=3 实装或删除。工作量 **S**。

### PM-4 【P1】评测体系是内部器官，不是产品能力
- **证据**：eval 基建相当完整——`crates/shannon-core/src/testing/eval_runner.rs`（3106 行，TOML 任务套件、真跑/dry-run 双模式、七类失败归因、双报告）、`eval_benchmarks.rs`（1991 行）、`eval_metrics.rs`（1382 行）。但 CLI `Commands` 枚举（`main.rs:560-770`）**没有 eval 子命令**，唯一入口是 `crates/shannon-core/examples/eval_runner.rs`；desktop 侧只有窄化的 skill loop（`desktop/src/main.rs:147-152`）。
- **影响**：「用评测驱动 agent 迭代」的故事只对会编译 Rust 的人成立；用户无法对自己的工作流建立回归基线——这恰是 gap-analysis P6 想解决的问题的最后一步。
- **建议**：加 `shannon eval run <dir> [--report]` 子命令复用 runner；CI 加夜间真跑（已有 `--dump-config` 级别的工程素养可复用）。工作量 **M**。

### PM-5 【P2】非交互入口三轨制，能力矩阵没人说得清
- **证据**：三条非交互路径并存——①位置参数 `shannon "prompt"`（`main.rs:369-377`）；②CI 模式 `-p/--prompt` + `--schema/--output-format/--max-turns/--allowed-tools/退出码`（`:442-477`）；③`query` 子命令（`:617-640`，有 `--output text|json|markdown`、`--no-stream`，无 tools/schema/NDJSON/退出码语义），执行走 `run_noninteractive_query`（`:4343-4355`）。三者文档分散、行为不同。
- **影响**：文档与用户心智负担；`--pipe`、位置参数与 `-p` 的优先级关系只能读源码得知。
- **建议**：把 ①③ 收敛为 `-p` 的语法糖（或至少在 `--help` 中写明等价关系与能力差异表）。工作量 **M**。

### PM-6 【P2】桌面与 CLI/TUI 正在变成两个产品
- **证据**：desktop 独有产品面——OPC/OPCTask、Triage、QuickFix、Editor、Skill Loop、Turn Timeline（`desktop/ui/src/App.tsx:67-106` 路由 + `desktop/src/main.rs:72-220` 命令）；这些在 CLI/REPL 的 121 命令面（`mod.rs:328`）中零映射（TUI 仅有同名但不同物的 `/review` `/plan` 等）。反向亦然：`/rewind` 在 desktop UI 无入口（grep `desktop/ui/src` 无 rewind 调用面）。
- **影响**：双端学习成本、会话体验不对称（桌面用户看不到 TUI 的 /rewind 能力，TUI 用户看不到 Triage/OPC）；长期会分裂术语表。
- **建议**：先做「能力对照矩阵」文档 + 把 `/rewind`、`/cost` 等高频能力补进桌面会话菜单。工作量 **M→L**（完整收敛）；文档 **S**。

### PM-7 【P2】`Screenshot` 内部工具占据顶级子命令位
- **证据**：`shannon screenshot <dir>` 调 `shannon_ui::screenshot::render_all_scenes`（`main.rs:641-648`、`:4370-4374`），是「渲染预定义 UI 场景为文本文件供 AI 分析」的开发工具。
- **影响**：顶级命令面是产品门面，开发工具混入抬高认知成本（与 `doctor`/`trace` 这类用户工具不可同日而语）。
- **建议**：降级为 `shannon dev screenshot` 或隐藏。工作量 **S**。

### PM-8 【P2】三处权威文档已过时（名实不符的「文档债」）
- **证据**：① `shannon-mono/CLAUDE.md` 架构表列 12 个 crate 并称「Crates (18 under crates/)」，实际 `crates/` 有 18 个目录，其中 `shannon-engine`（10,744 LOC，含 API client/权限分类器/流式执行器）、`shannon-api-protocol`、`shannon-server`、`shannon-repomap`、`shannon-mcp-saas`、`shannon-stability-attr` 六个完全缺席；② 同文称 PermissionClassifier「2928 lines」，实际 `crates/shannon-engine/src/permission_classifier.rs` 已 3,038 行；③ `docs/metrics.md` 快照停在 `2026-08-08 / c90adf82 / v0.8.0-11`（`docs/metrics.md:9-13`），此后约三周的大改未刷新。
- **影响**：新协作者（人和 agent）按 CLAUDE.md 认路会直接迷路；metrics 是「CI 生成的唯一权威」却未再生成。
- **建议**：补表 + 重跑 `scripts/gen-metrics.sh` 并把 metrics 刷新挂到 CI 定时任务（已有 `metrics-update.yml`，核查其触发条件）。工作量 **S**。

### PM-9 【P3】Welcome 流的一次性门控吞掉工具安装步
- **证据**：`shouldShowWelcome` 仅在「没见过 且 没有 provider」时返回 true（`desktop/ui/src/pages/Welcome.tsx:31-37`）。用 `SHANNON_API_KEY` 等环境变量启动的用户（`detectProviderFromEnv` 命中即跳过配置步，`:88-104`）虽然看到了 Welcome，但工具/技能安装步（ToolsStep）对「下次不再出现」的既有用户永远不会补展示。
- **影响**：env-key 用户（恰是熟练用户）系统性错过 Extensions/技能安装引导。
- **建议**：把「工具步是否完成」独立成一个 seen 标记。工作量 **S**。

### PM-10 【P3】REPL 首启无引导，只有状态卡兜底 + 静默 Ollama 回退
- **证据**：REPL 用 `LlmClientConfig::default()` 构建（`crates/shannon-ui/src/repl/mod.rs:324,774`）；该 `Default` 在「无 key 且未设 `SHANNON_BASE_URL`」时静默切到 Ollama `localhost:11434`，仅 `tracing::info!`（`crates/shannon-engine/src/api/types.rs:399-421`）。状态卡确实会显示 `NO API KEY`（`types.rs:506-519` 的 Display），CLAUDE.md 也记录了状态卡含 `/connect` 提示。
- **影响**：TUI 用户首启得到的是「提示去哪修」而非「带我去修」；Ollama 回退在无 Ollama 的机器上会变成连接错误而非配置引导。
- **建议**：无 key 时 REPL 首屏直接内嵌 `/connect` 引导面板；Ollama 回退改为显式提示。工作量 **S→M**。

### PM-11 【P3】IA 合并的账单寄给了 Tasks 页
- **证据**：五条旧路由 `/mission-control /goals /routines /hooks /profiles` 全部重定向到 `/tasks`（`desktop/ui/src/App.tsx:63-80`），Tasks 页同时承载任务、后台任务、agents、routines（`Tasks.tsx:55` 一次取四类数据）。
- **影响**：重定向保书签是对的，但单页信息架构过重；「hooks/profiles 去哪了」会成为高频客服问题。
- **建议**：Tasks 内做锚点深链（`/tasks?tab=routines`）并在 redirect 时携带，减少迷路感。工作量 **S**。

### PM-12 【P3】Signals/Feedback 闭环有采集无呈现
- **证据**：`shannon feedback up|down` 与 `shannon signals status|push` 只在 CLI（`main.rs:715-770` 一带）；REPL 无 `/feedback`（`mod.rs:328` 名单无此项），desktop 无对应 UI（`desktop/src/main.rs` 命令清单无 signals）。设计上「默认纯本地、opt-in 上传」是对的（同段文档）。
- **影响**：本地攒下的信号没有查看面，数据采集的半环悬空。
- **建议**：Usage 页加一个「本地信号」卡片读 counters.jsonl。工作量 **S**。

---

## 2. 架构视角

### ARC-1 【P1】core 单 crate 83 模块，P10 面积问题仍在恶化
- **证据**：`crates/shannon-core/src/lib.rs` 中 `^pub mod|^mod` 计 83 项（`:83-124` 一带可见新增的 `scheduled_budget/retry/routines/runs/task_store/worktree`、`skill_loop`、`team_memory_sync`、`telemetry`、`auto_test` 等）；gap-analysis P10（`docs/research/shannon-gap-analysis.md:87-91`）一年内从「70+」涨到 83。对照 CLAUDE.md「~96 处 allow(dead_code)，计数漂移」的自述，面积治理处于「记录了但没执行」状态。
- **影响**：编译时间、评审面、新能力「默认进 core」的路径依赖都在变差；这正是 gap-analysis 给出的 P3 持续项。
- **建议**：执行既定策略——新能力默认进扩展位（feature crate/MCP/routine）；先切两个无依赖域做示范（`scheduled_*` 六件套可合成一个 `shannon-scheduler` crate；`memory`+`auto_dream_consolidation`+`extract_memories` 合成 `shannon-memory`）。工作量 **L**（渐进，可切片）。

### ARC-2 【P1】双命令系统存在「影子覆盖」规则，且该规则只存在于运行时
- **证据**：`shannon-commands` 注册 33 个 builtin（`crates/shannon-commands/src/builtin.rs:62-99`），REPL 侧 `repl_only_commands` 名单（`mod.rs:328-449`，121 项）与之重叠——`diff/search/status/export/config/debug/doctor/mcp/memory/team/cost/plan/review/credentials...` 同时存在于两边；分发逻辑是「先 match REPL 原生 handler（`:454-576`），落空才 `handle_other_command` 走注册表 PromptCommand（`:603`）」。直接后果：`handle_other_command` 里的原生预分析 `"diff" | "git-diff"`（`:642-643`）对 `/diff` **不可达**（被 `:468` 的 `"diff" => git::handle_diff` 截走），`/commit` `/review-pr` 等 PromptCommand 才是注册表路径的真实用户。
- **影响**：同一命令名在两个体系含义不同（如 `/diff` REPL 是本地 git diff 分析、注册表的是 prompt 模板）；行为差异无文档、无编译期检查。
- **建议**：短期在 `handle_other_command` 注释 + `/help` 中写明优先级；长期让 builtin 注册时声明「REPL 原生/可覆盖」，由 registry 统一仲裁。工作量 **M**。

### ARC-3 【P2】四个 god file 集中在交互面与分类器
- **证据**：`crates/shannon-cli/src/main.rs` 6,557 行（CLI 解析+headless 引擎+desktop 启动器+trace 全在一文件）；`crates/shannon-ui/src/repl/input.rs` 3,505 行；`crates/shannon-engine/src/permission_classifier.rs` 3,038 行；`crates/shannon-ui/src/repl/commands/mod.rs` 812 行（其中名单+match 占约 250 行）。
- **影响**：merge 冲突高发区；`main.rs` 里 headless 引擎逻辑（`:1606-2140`）无法被桌面/服务端复用。
- **建议**：先把 headless 抽成 `shannon-cli/src/headless/` 模块（不动行为），再做 `input.rs` 按键域拆分。工作量 **M**。

### ARC-4 【P2】EventBus 四模式有两个语义陷阱
- **证据**：`crates/shannon-core/src/bus.rs:454-463` 定义 Emit/Serial/Parallel/Waterfall。陷阱一：`DispatchMode::Parallel` 在无 tokio runtime 时静默降级为 Emit、有 runtime 时 fire-and-forget `tokio::spawn`（`:592-607`）——调用方既感知不到降级也感知不到完成；陷阱二：并行分支刻意吞掉 panic（`let _ = handle.await`，`:666-670`）且**无任何日志**；Waterfall 作为 raw-dispatch 参数时等同 Emit（`:592-595` 文档自认）。
- **影响**：订阅者异常静默丢失；「事件发了但没人处理/处理炸了」在生产中不可诊断——这与总线化（§4.8）想提供的可观测性目标相悖。
- **建议**：降级与 panic 各加一条 `tracing::warn!`；给 Parallel 提供 `dispatch_parallel_checked` 返回 join 结果。工作量 **S**。

### ARC-5 【P2】CI 的 clippy 门带着五条内联豁免
- **证据**：`.github/workflows/ci.yml:169-173` 在 `cargo clippy --workspace -- -D warnings` 后追加 `-A clippy::collapsible_if -A collapsible_match -A derivable_impls -A manual_is_multiple_of -A manual_checked_div`。
- **影响**：与 CLAUDE.md「clippy -D warnings enforced」的宣称不符；豁免会在后续 PR 中合法地累积。
- **建议**：清理存量后移除豁免（一条机械重构 PR），或至少在 CI 名字中标注放宽范围。工作量 **S**。

### ARC-6 【P3】`repl_only_commands` 名单需要生成而不是维护
- **证据**：同 ARC-2/PM-1；121 项字符串数组 + 250 行 match 双维护，唯一防线是人工同步。
- **建议**：用宏或 build.rs 从 handler 定义生成名单，并加 PM-1 所述一致性测试。工作量 **M**。

### ARC-7 【P3】`shannon-engine` 不在文档 crate 表中（与 PM-8③ 同源，此处记架构影响）
- **证据**：`crates/shannon-engine/` 是 API client、权限分类器、流式工具执行器（`streaming_tool_executor.rs` 942 行）所在地，但 `shannon-mono/CLAUDE.md` 架构表无此行。
- **影响**：`core vs engine` 的边界（core 编排/engine 执行）是当前最重要的分层事实，文档缺位导致新代码继续往 core 堆。
- **建议**：补表并写一句边界陈述。工作量 **S**。

### ARC-8 【P2·bug】`query --output json|markdown` 是被静默忽略的死 flag
- **证据**：`Commands::Query` 定义 `output`（默认 "text"，`main.rs:631-635`）；分发处解构为 `output: _output_format` 直接丢弃（`:4343-4347`），`run_noninteractive_query(&query, !no_stream, ...)` 不接收输出格式。
- **影响**：用户/脚本指定 `--output json` 得到纯文本，无任何报错——违反「CLI 参数要么生效要么报错」的最小契约。
- **建议**：实装或 `#[arg(hide)]`+deprecation 提示。工作量 **S**。

### ARC-9 【P3】测试体量分布健康的反面：单测文件过大
- **证据**：`crates/shannon-ui/src/repl/tests.rs` 4,889 行单文件（全仓 11,273 测试，`docs/metrics.md:17-30`），`mod.rs` 里 `tests.rs` 与 8,000+ 行源码同目录。
- **影响**：测试可导航性差，失败定位靠文件内搜索。
- **建议**：按命令域拆 `tests/` 子模块。工作量 **S**。

---

## 3. UI/UX 视角

### 3.1 Desktop（React/Tauri）

#### UI-1 【P2】zh-CN 缺失 1 个键，会向中文用户暴露 raw key
- **证据**：脚本比对 `desktop/ui/src/i18n/locales/`：`en.json` 2,376 键 vs `zh-CN.json` 2,375 键，缺失键为 `chat.input.attach.tooMany`；该文案用于附件超限提示，属于高频错误路径。
- **影响**：中文用户在报错时刻看到英文原文或键名（取决于回退策略），恰是最差时刻的体验裂缝。
- **建议**：补键 + CI 加「键集合一致性」检查（10 行脚本）。工作量 **S**。

#### UI-2 【P3】Chat 虚拟化的魔法数与滚动预估
- **证据**：`desktop/ui/src/pages/Chat.tsx:66-80`：`estimateSize: () => 200` 硬编码、阈值 `messages.length > 30` 才启用虚拟化；已用 `measureElement`（ResizeObserver）校准，缓解了长消息误差。
- **影响**：30→31 条切换瞬间布局跳变；超长代码块消息在测量前会闪动。
- **建议**：接受现状（已记录权衡注释），如需改进则对 `estimateSize` 按消息类型分档。工作量 **S**。

#### UI-3 【P3】spike 路由存活于主路由树
- **证据**：`/chat-v2-spike` 挂在正式 `<Routes>` 内（`App.tsx:103`），有条件渲染门控。
- **影响**：深链可直达未定稿 UI；测试与 i18n 键的维护面被动扩大。
- **建议**：spike 期挂在 feature flag 配置而非路由常量文件，或加 `dev` sidebar 模式门控（Welcome 已有 `devMode` 概念可复用，`Welcome.tsx:139-141`）。工作量 **S**。

#### UI-4 【P3】Welcome 步进器的脆弱点
- **证据**：`const currentTask = TASKS.find(t => t.id === task)!` 非空断言（`Welcome.tsx:53`）；`setStep(2)` 硬编码跳步（`:113` 一带）。
- **影响**：`TASKS` 常量增删任务 id 时运行时才炸；步骤重排时跳步错位。
- **建议**：find 失败回退首项；步进用语义常量。工作量 **S**。

#### UI-5 【正面确认】三态/a11y/设计系统登记制已系统性落地
- 页面三态：`Triage.tsx:384-393`（CardSkeleton+EmptyState+CTA）、`Tasks.tsx:211-221`（错误 Banner）、`Usage.tsx:184-193`、`TurnTimeline.tsx:114-129`（ErrorState 组件）。
- 无障碍：`App.tsx:55` ErrorBoundary；`ui/modal.tsx:83`、`ui/side-panel.tsx:70` 均 `role="dialog"`+`aria-modal`+焦点陷阱+Esc。
- 规范：Material Symbols 工具类（`index.css:183-184`）、`styles/tokens.css` 主题登记、en/zh-CN 双语齐备（除 UI-1 一键）。
- **建议**：无需行动；把「新页面必须三态齐备」写进贡献文档即可。

#### UI-6 【P3】`/quickfix` `/editor` 双入口并存
- **证据**：两页既是顶级路由（`App.tsx:94-95`），又被 Chat 作为内联面板 lazy-load（`Chat.tsx:26-29` 注释明确「no longer top-level routes」——但路由仍在）。
- **影响**：注释与现实不符；书签/深链入口与内联入口的状态语义可能分叉。
- **建议**：删路由或让路由重定向到 `/chat` 并带 prefill state（项目已有该机制，`Chat.tsx:46-58`）。工作量 **S**。

### 3.2 TUI/REPL

#### UI-7 【P3】TUI 无障碍是「字符替换」级别
- **证据**：`crates/shannon-ui/src/a11y.rs:1-30`：thread-local 开关 + 把进度条字符换成 `#`/空格；`/accessibility` 命令切换（`mod.rs:328` 名单含 `a11y`）。VIM 四模式完整（`vim.rs:1-30`），help overlay 双栏+Esc（`widgets/help_overlay.rs:14-40`）。
- **影响**：对屏幕阅读器场景（如经 brltty/tmux 事实上的纯文本化）有帮助，但无语义层；与桌面端的 aria 投入不对等。
- **建议**：接受现状并在文档标注边界；如需提升，优先做「可关闭所有装饰字符」的全局开关而非逐点适配。工作量 **S**。

#### UI-8 【P3】命令名的用户认知负担：121 个名字、大量单字母/俚语别名
- **证据**：名单（`mod.rs:328-449`）含 `st/creds/cred/dbg/dev/perf/img/clip/memo/adddir/gh-actions...` 及俚语命令 `ralph`、`loop`、`bind`。
- **影响**：`/help`（只覆盖 52%，见 PM-2）之外的别名无从发现；`/st` 这类缩写无文档。
- **建议**：别名只在补全菜单出现、不进公开文档；`/help --all` 显示别名表。工作量 **S**。

---

## 4. 安全

### SEC-1 【P1】插件权限模型的三元组合风险
- **证据**：①语义上「未声明 permissions = 全允许」（`crates/shannon-core/src/plugin/permissions.rs:147-169`，`unspecified_policy_allows_everything` 测试坐实，兼容旧行为）；②桌面提供 `install_plugin_from_git` 一键克隆任意 URL（`desktop/src/commands_plugins.rs:106-116` → `plugin/registry.rs:129-183`，`git clone --depth 1` 直执行；arg-vector 无注入，但无 host/scheme 校验、无版本 pin）；③安装闸门 `admit_for_install` 仅 `warn_about`（`registry.rs:402-406` → `validate.rs:228-232` 只 `tracing::warn!`），硬失败仅限 name/description/version 缺失（`validate.rs:94-109`）。`write_files` 强制点保持 OFF 是**已声明的脚手架**（`permissions.rs:16-19`），不计为新问题。
- **影响**：一个 typo-squat 仓库 + 默认全能力 = 安装即获 `execute_commands/network/mcp_tools`。决策事件虽落盘（`permissions.rs:100-135`），但落盘是取证不是阻断。
- **建议**：安装完成后把「未声明权限=全允许」作为一级 UI 警示展示（数据已在 manifest 里）；中期把 git 安装默认收紧为「未声明 → 拒绝未知面」的显式确认流。工作量 **S（警示）/M（收紧）**。

### SEC-2 【P2】沙箱默认 off
- **证据**：`SandboxSettings::default()` mode = `SandboxMode::Off`（`crates/shannon-tools/src/sandbox/mod.rs:289-293`），`detect()` 无 env/TOML 时保持 off（`:305+`）；文档自述「fail-open for configuration」（`:272`）。
- **影响**：Landlock 后端已可用（`:476` 装配路径完整）但默认不设防；用户不知道需要 `SHANNON_SANDBOX=landlock`。
- **建议**：`/sandbox` 命令与桌面设置里给出三态开关及「当前无内核边界」的常驻提示；Linux 上默认 `local` 档可评估。工作量 **S（提示）/M（默认值变更）**。

### SEC-3 【P2】凭据静态明文是既定架构（A1），但 keyring 支名不副实
- **证据**：`CredentialRef` 注释自述「Env 是默认/下界；Keyring 机会性可选（探测失败静默降级）」（`crates/shannon-types/src/provider_config.rs:66-86`）；Store 后端是 `~/.shannon/credentials/<service>.json` 0600 原子写（`credential_manager.rs:477-512`，实现干净）。真正用 OS keyring 的只有 desktop 网关连接（`desktop/Cargo.toml:46`）与 `shannon-mcp-saas` OAuth。
- **影响**：与竞品（OS keychain 为默认）相比，进程内任意代码执行即可读走全部 provider key；文件 0600 挡不住同 UID 进程。
- **建议**：不推翻 A1，但把「keyring 可用即优先」从「机会性」升级为「可见开关 + 状态显示」（`/credentials` 处展示后端类型）。工作量 **M**。

### SEC-4 【P2】扩展签名验证是自声明比对（实现诚实，UI 承诺需对齐）
- **证据**：`desktop/src/extensions/security.rs:250-303`：`verify_signature` 只解析 `signer:` 行并与 `KNOWN_SIGNERS` 字符串比对，文档明说「No cryptographic verification」并返回 `SelfDeclared` 状态；note 文案诚实。
- **影响**：若 Extensions UI 把 `SelfDeclared` 渲染成「已验证」级别的徽章（需 UI 走查确认），用户会高估保障；当前至少数据层是诚实的。
- **建议**：审计 Extensions 页徽章文案与 `SignatureStatus` 的映射；中长期接 minisig/sigstore。工作量 **S（文案）/L（真验签）**。

### SEC-5 【P2】headless 错误分类靠错误文本子串匹配
- **证据**：`main.rs:1997-2005`：`err_lower.contains("permission") || contains("denied")` → 退出码 6（PermissionDenied）、`contains("rate limit")||contains("429")` → 4、`contains("context")...` → 5。
- **影响**：普通工具错误（如 git 输出 `permission denied`、HTTP 429 出现在 curl 输出里）会把退出码从 1 误升为 4/6，CI 重试逻辑被误导；这也放大了 PM-3 的文档错位。
- **建议**：优先用 `ApiError` 结构化变体分类，子串匹配只作最后兜底并日志标注。工作量 **S→M**。

### SEC-6 【P3】redaction 内建 shape 未覆盖部分主流 token 形态
- **证据**：`BUILTIN_PREFIX_REGEX` 仅 `sk- / ghp_ / github_pat_ / xox[abp]- / glpat-`（`session_log/redaction.rs:45-56`）；AWS `AKIA…`、Google `AIza…`、`npm_`、Anthropic 旧式 key 等不在内建（用户可经 `redaction.toml` 自补，`redaction.rs:76-110`）。
- **影响**：云厂商 key 落进 events.jsonl 的窗口仍在（例如用户把 AWS key 放进环境变量名不含 KEY/SECRET/TOKEN/PASSWORD 的变量）。
- **建议**：扩充内建正则（与 env-name 启发式 `env_name_looks_secret`（`:59-66`）互补），保持「只加不减」原则不变。工作量 **S**。

### SEC-7 【P3】`install_from_git` 无版本 pin
- **证据**：`git clone --depth 1` 默认分支即安装（`plugin/registry.rs:141-158`），无 tag/commit 锁定，`update_plugin` 拉取同理（`desktop/src/commands_plugins.rs:128` 一带）。
- **影响**：上游被投毒时自动直达用户（供应链时序攻击面）。
- **建议**：安装时记录 resolved commit hash，更新时展示 diff 提示确认。工作量 **S**。

### SEC-8 【正面确认】安全基线的正确部分
- 权限决策双源落 L0 单 schema（`permissions.rs:100-135`）；redaction「写时净化 + 内建不可关闭 + 策略快照」（`redaction.rs:22-36`）；Signals 遥测默认本地、双 env 显式 opt-in（`main.rs:715-733`）；desktop 非回环绑定强制要求 token（`main.rs:650-657` 的 `allow_nonloopback` + `auth_token` 约束）；`session_log::SessionSidecar` 权限校验 0600（`credential_manager.rs:420-459` 同型检查在 desktop 会话侧沿用）。

---

## 5. 性能与可靠

### PERF-1 【P1】L0 日志无 fsync：权威性有崩溃窗口
- **证据**：`crates/shannon-core/src/session_log/writer.rs` 全文无 `sync_all`/`sync_data`（grep 证实）；持久化 = `BufWriter`（`:75`）+ 聚合 flush 策略（chunk 聚合阈值/时间阈值/边界事件立即 flush，`:17-30,:232-236`）。flush 只保证到页缓存。
- **影响**：进程被 kill -9 时日志通常完好（内核仍会刷盘），但断电/内核 panic 会丢尾部事件；而产品叙事（CLAUDE.md §4.6、trace 命令族）已把 events.jsonl 立为「单一权威记录」。权威 = 可恢复，当前不严格成立。
- **建议**：在 turn 边界（已有 flush 边界处，`:233-236`）追加 `sync_data()`；bench `turn_log_overhead.rs` 顺手复测开销。工作量 **S→M**。

### PERF-2 【P1】SSE 总超时掐断长流；重连语义是「整轮重生成」
- **证据**：`LlmClient` 的 reqwest client 带**总超时** `Client::builder().timeout(timeout_seconds)`（`crates/shannon-engine/src/api/client.rs:96-106`，`try_new` 同 `:119-121`），流式请求复用该 client（`:470-476`）。reqwest 的 client 级 timeout 覆盖「发起连接到响应体读完」全程——默认 120s（`api/types.rs:557` 等，Zhipu 档 300s `:533`）意味着任何超过 120s 的流式 turn 会被硬切断。兜底是 `ResumableSseStream`：Timeout 属于可重连错误（`api/streaming.rs:337-352`），但重连=用**完整 messages/tools/system 重发**（`:387-403`），`Last-Event-ID` 对 Anthropic 类 API 并无回放契约（模块文档自述意图，`:246-250`）。
- **影响**：长 turn（大文件生成、深度推理）在 120s 处被周期性打断并重新计费/重新生成，UI 侧文本可能重复；max_stream_reconnects 耗尽后整轮失败。
- **建议**：流式请求改用无总超时 client + 读空闲超时（idle timeout 30-60s），非流式保留总超时；重连策略对「已收到部分内容」的场景默认不重发或去重。工作量 **M**。

### PERF-3 【P2】headless 冷启动串行做 MCP 发现
- **证据**：`run_headless_query` 对每个 enabled MCP server 顺序 `discover_tools().await`（`main.rs:1636-1669`，for 循环内逐个 spawn+握手），server 多时启动延迟线性叠加，且每个失败只 eprintln 警告继续。
- **影响**：CI 场景每次 `shannon -p` 重复支付发现成本；3 个 2s 的 server = 6s 纯启动开销。
- **建议**：`futures::join_all` 并行发现；中期加发现结果缓存（TTL 版本化）。工作量 **S（并行）**。

### PERF-4 【P3】token 统计是「每请求取 max」的下界口径
- **证据**：`main.rs:1966-1970` 注释自述「Fallback accounting only — max-per-request undershoots」；headless 输出的 `total_tokens` 即此口径。
- **影响**：CI 报告与 `--output-format json` 的 token 数系统性偏低；用户据此估算成本会失真（desktop Usage 页走 engine_usage 口径，两处数字可能对不上）。
- **建议**：统一从 engine_usage 汇总（代码注释已指明方向），headless 输出标注口径。工作量 **S**。

### PERF-5 【正面确认】性能基建在位
- 8 个 bench 文件（core：compact/context_budget/event_bus/query_engine/turn_log_overhead/core_benchmarks；tools：edit/tool_benchmarks）+ `benchmarks.yml`/`bench-regression.yml` CI 门；Chat 列表虚拟化带阈值权衡注释（`Chat.tsx:62-80`）；`/health` 探测 5s 硬超时防仪表盘卡死（`repl/commands/config.rs:14-16`）。

---

## 6. 功能完整度对标（CLAUDE.md「Competitor Feature Tiers」名实核对）

| 自评宣称 | 核对结果 | 证据 |
|---|---|---|
| Non-interactive/CI 模式（Tier 2 差异化） | **名过实**：能力在，但退出码文档错误（PM-3）、错误分类靠子串（SEC-5）、token 口径偏低（PERF-4）——「能用于 CI」与「可信用于 CI」之间有缝 | `main.rs:1600-1604,1997-2005` |
| Hook system 32 事件（Tier 2） | 基本名实相符：事件类型+routine 触发+总线化在位 | `bus.rs:676-705`、CLAUDE.md 表 |
| 插件系统（Tier 2） | **半名实**：manifest v2/注册表/权限强制已落地，但默认全允许+warn-only 安装闸门（SEC-1）使「权限面」宣传弱于实际保障 | `permissions.rs:147-169` |
| LSP 集成 6 工具+后台诊断（Tier 2） | 名实相符：`repl/lsp_bridge.rs`、`diagnostic_watcher.rs`、LSP 命令族（`shannon-commands/src/builtin/lsp.rs`） | 文件在库 |
| Computer use（Tier 3） | 名实相符：feature-gated，`shannon-tools/src/computer_use.rs` 存在 | 文件在库 |
| 评测（本次新增） | **实过名**：基建 6.5K 行强于文档宣传，但无用户入口（PM-4） | `testing/eval_runner.rs` |
| Session persistence 事件溯源 | 名实基本相符，fsync 缺口除外（PERF-1） | `session_log/writer.rs` |

---

## 7. User Journey 走查表

| # | Journey | 状态 | 关键证据 | 断点/摩擦 |
|---|---------|------|---------|-----------|
| J1 | 安装（CLI/桌面组件化安装脚本） | ✅ | 桌面缺失时打印一键安装指引（`main.rs:3082-3090`），`--build/--install` 分离（B5 决议） | 无明显断点；Windows NSIS 链路依赖既有发布流程 |
| J2 | 配 provider（CLI/desktop/REPL 三端） | ✅⚠ | CLI `providers add` 走 `ProviderConfigStore`、凭据恒为 `Store{}`（`main.rs:782-800`）；desktop Welcome 四步+env 探测（`Welcome.tsx:74-104`）；REPL `/connect` 带密钥脱敏（`mod.rs:751-798`） | 摩擦：REPL 无引导面板（PM-10）；desktop 双端配置一致性已由 ADR-0005 收口 |
| J3 | 首启 | ✅⚠ | desktop Welcome stepper 完整（任务→模型→工具→完成）；TUI StatusCard 显示 provider/model/tier+命令提示（CLAUDE.md） | 摩擦：Welcome 一次性门控吞 ToolsStep（PM-9）；env-key 用户的 Ollama 静默回退（PM-10） |
| J4 | 对话/工具使用 | ✅ | REPL 121 命令+补全（`input.rs:1202-1284`）；desktop Chat 虚拟化+附件+QuickFix/Editor 内联 | **断点**：`/notools` 损坏（PM-1）；`/help` 52% 覆盖（PM-2） |
| J5 | 改代码（权限→编辑→回滚） | ✅⚠ | desktop 权限命令对 `request/respond_permission`（`main.rs:158-159`）；`/rewind` 三模式+`/undo` `/checkpoint` 别名（`mod.rs:489-490`）；文件历史非 git 快照 | 摩擦：desktop 无 /rewind 入口（PM-6）；沙箱默认 off（SEC-2） |
| J6 | 恢复会话/审计 | ✅⚠ | `--resume/--resume-id/-c`（`main.rs:396-412`）；`trace show/replay/diff/export` 四命令（`:560-612`）；desktop sessions+Turn Timeline（`main.rs:140-143`）与 CLI 同一 L0（`desktop/src/commands_sessions.rs:36`） | **缺口**：L0 无 fsync（PERF-1）；`--resume` 与 headless 组合在文档中不可见（PM-5） |
| J7 | 评测自己 | ❌（CLI）/△（desktop） | runner+dry-run+报告完备（`eval_runner.rs:1-50`）；desktop skill loop 窄覆盖（`main.rs:147-152`） | **断点**：无 CLI 入口、无 CI 载体（PM-4）——七步旅程中唯一完全断的 |

---

## 8. 快赢清单（一周内可完成，按 ROI 排序）

| # | 动作 | 对应问题 | 工作量 | 验证方式 |
|---|------|---------|--------|---------|
| 1 | 修 `/notools` 名单漂移 + 加 match↔名单一致性测试 | PM-1/ARC-6 | S（半行+测试） | `cargo nextest run -p shannon-ui` |
| 2 | 改正 headless 退出码 doc comment（2=TurnLimit,6=denied） | PM-3 | S（一处注释） | 读 `main.rs:1600` |
| 3 | 补 zh-CN 缺键 + CI 键集合一致性检查 | UI-1 | S | i18n 脚本 |
| 4 | `query --output` 实装或删除 | ARC-8 | S | 手跑三条输出 |
| 5 | L0 writer 在 turn 边界 `sync_data()` | PERF-1 | S→M | bench 复测 + kill -9 演练 |
| 6 | 流式请求去总超时、改 idle 超时 | PERF-2 | M | 长流 >150s 演练 |
| 7 | EventBus 降级/吞 panic 两处加 warn 日志 | ARC-4 | S | 单测断言日志 |
| 8 | `/help` 补 30 个缺失主命令条目（可脚本生成初稿） | PM-2 | S | `/help` 走查 |
| 9 | `install_from_git` 成功后展示权限面+未声明=全允许警示 | SEC-1 | S | desktop 手测 |
| 10 | CLAUDE.md 补 6 个缺失 crate + 重跑 gen-metrics.sh | PM-8 | S | 文档 diff |
| 11 | headless MCP 发现并行化 | PERF-3 | S | 计时对比 |
| 12 | `shannon eval run` 薄封装 runner | PM-4 | M | 20 题套件 dry-run |

---

## 9. 方法与局限

- 证据来自对约 40 个关键文件的定点精读与 15 组脚本化交叉比对（dispatch 名单 vs match 分支、help 覆盖、i18n 键集合、exit-code 文档 vs 枚举等）；未逐行读全部 62 万行。
- 未运行任何测试/浏览器会话；涉及运行时行为的结论（reqwest 超时语义、git clone 行为）基于库的既定契约标注，建议以 PERF-2/S处 的演练脚本复核。
- 已知背景修正：`write_files` 权限强制保持 OFF、`/undo`=`/rewind` 别名、events.jsonl 取代旧快照均为 2026-08-27/28 有意变更，本报告不将其计为缺陷，仅在 SEC-1/PERF-1 讨论其残留风险。
- 行号基于 2026-08-28 dev HEAD；合并后引用请以符号名/函数名为准。
