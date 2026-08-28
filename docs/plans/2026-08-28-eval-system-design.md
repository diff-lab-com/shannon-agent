# Shannon 评测体系设计——在已有地基上补全成体系

- 日期：2026-08-28
- 定位：**不是从零设计**。Shannon 已建成 L0 事件日志、20 题 L1 套件 + runner、11 种断言词汇、三外部基准 adapter（n=3 + citability 门控）、A/B 指令开关、版本对比看板与 L4 信号骨架。本文回答的问题是：这些点状资产如何装配成**分层、可门禁、可归因、可对外引用**的体系，缺的每一块补在哪。
- 输入：[评测全景调研](../research/agent-eval-landscape-and-plan.md)、[综合改进主计划 §4.3–4.15/§6](shannon-improvement-master-plan.md)、代码盘点（crates/shannon-core/src/testing/ 全量 + tests/eval/ + .github/workflows/）、业界调研（§9 来源）。
- 工作量记法沿用主计划：S/M/L 相对量级。

---

## 0. 现状盘点：资产清单与判定

设计前先钉死「有什么、缺什么」，所有后续章节以此为据，**已达标项列入负面清单禁止重复建设**（汇总见 §10）。

| # | 资产 | 证据（路径） | 判定 |
|---|---|---|---|
| A1 | L0 事件日志（events.jsonl 唯一权威；tool/call 原始参数、turn/end usage/cost、request/header wire_body 字节级快照） | shannon-core/src/session_log/，shannon trace show/replay/diff/export | ✅ 已达标——指标真源成立，不再造任何采集层 |
| A2 | L1 任务集 20 题（read 3/edit 5/search 3/multi 6/recovery 3），TOML 声明式：setup/verify{rules,script}/expectations{forbidden_tools,trajectory}/limits/dry_run 桩 | tests/eval/tasks/*.toml | ✅ 骨架达标；缺口在 horizon 维度（§2） |
| A3 | eval runner：run_suite/run_task、NDJSON 观测、6 态 RunStatus、双报告（report.json+md）、stable_digest、compare_reports、失败样本归档 `<home>/eval/failures/<date>/<task>/`、每任务隔离 `l0-home` SHANNON_HOME | testing/eval_runner.rs（3106 行） | ✅ 主干达标；缺口在 n=1（§4）与元锚点（§4） |
| A4 | 指标提取：TaskMetrics 13 字段（tokens in/out/cache 写读、cost_usd=Option 诚实未知、turns、tool_calls、wall_clock、loops、loop_max_streak、invalid_calls、permission_blocks、source）、METRIC_FIELDS 零缺失契约、L0 优先 + 流兜底双馈 | testing/eval_metrics.rs | ✅ 已达标 |
| A5 | 失败分类：7 类规则表外置（failure_rules.toml 为内嵌默认，--rules/环境变量可覆盖）、13 个信号词汇、首中即停、fingerprint 入报告 | testing/failure_rules.toml + eval_metrics.rs | ✅ 骨架达标；缺口在未分类残留度量与语义分界（§6） |
| A6 | 断言词汇 11 种：FileExists/FileContent(contains,matches_regex)/FileNotExists/ExitCode/ToolCalled/ResponseContains/MaxDurationMs/DiffMatches/TrajectoryContains/ForbiddenTool/CostBelow(per_turn\|total)；TrajectoryStep 支持 tools 族 + optional | testing/scenario.rs（ValidationRule） | ✅ 已达标——行为级断言层已建成，不重写 |
| A7 | 三外部基准 adapter：Terminal-Bench（10 pin）/SWE-bench Verified（50 pin 入仓防漂移）/自建回归（10）；判据一律原生（judge_statement 逐套入报告）；BenchDisposition 8 态且 is_scored 只认 resolved/failed；EnvState 探测；pin manifest + fingerprint | testing/eval_benchmarks.rs（1991 行）+ tests/eval/benchmarks/ + examples/bench_runner.rs（run/diff/validate-pins） | ✅ 已达标——「分数可引用」的纪律已在代码里强制 |
| A8 | n=3 重复 + 区间 + 机械方差归因（attribute_variance：状态翻转检测 + token spread>20% 标记，cause-blind）+ CitationBlock{citable, blockers, workload_fingerprint} citability 门控 | eval_benchmarks.rs（N_RUNS_REQUIRED=3） | ✅ L3 层达标；L1 层缺失同款（§4） |
| A9 | A/B 指令开关：--directive 注入任务 prompt，报告 stamp `directive`，digest 记 directive_present | EvalOptions.instruction_directive | ✅ 已达标 |
| A10 | 版本对比看板：静态自包含 HTML（version×metric 矩阵 + 失败类分布），eval runs 目录一键生成 | testing/dashboard.rs + examples/signals_dashboard.rs | ✅ 达标；缺口在 L4 列与 CI 呈现（§7） |
| A11 | L4 在线信号：SignalCounters（反馈/中断/权限判定/rewind 计数）、SignalsConfig 默认关、本地 analytics 落盘、CLI 侧上报 | shannon-core/src/signals.rs、shannon-cli/src/signals.rs | ✅ 骨架达标（DP5 姿势成立）；缺口在回流看板（§1） |
| A12 | OTLP 桥 + Jaeger/Grafana compose（L0→span 树） | shannon-core telemetry 路径 | ✅ 已达标（§4.14 验收完成） |
| A13 | justfile 入口：`just eval`（dry-run 桩）/ `just eval-real`（真模型）/ `just eval-diff a b` | justfile:104-113 | ✅ 达标 |
| A14 | CI：ci.yml（单测/场景）；benchmarks.yml（每周日 criterion 基线）；bench-regression.yml（周一信息性） | .github/workflows/ | ❌ **全部是工程性能 bench，没有任何 agent-eval 工作流**（§7 主缺口） |
| A15 | 元锚点：报告含 app_version / shannon_bin / failure_rules_fingerprint / directive / metrics_source | RunReport 字段 | ⚠️ 半成——**缺模型锚点**（model_id/provider 不在报告里，L0 里有但没提取）（§4） |
| A16 | 限额：代码默认 12 turns/80k tokens/300s（eval_runner.rs:252-256）；任务实测值 50 turns/1M tokens/450–600s；全部硬顶（超限→turn_limit/token_limit/timeout→失败类） | TaskLimits | ⚠️ 只有硬顶，无 horizon 分级、无预期预算软标记（§2） |

---

## 1. 分层评测金字塔

### 目标

五层各司其职、触发时机与预算门禁显式化，使「每次提交花了多少钱评、什么层挡 PR、什么层只观察」成为一张表能说清的事。

### 现状差距

层的**内容**大多已存在（§0 A1–A13），缺的是**编排与纪律的显式化**：没有文档化的层定义，没有 PR/夜间/每周/发版的触发矩阵，L4 信号（A11）不回流任何评测呈现，L2/L3 的预算差异没有按层声明。

### 方案

| 层 | 内容 | 评什么 | 触发 | 模型 | 预算护栏 | 门禁语义 |
|---|---|---|---|---|---|---|
| **L0 单元/契约** | just test + L0 不变量（seq 连续、wire_body 逐字节重建、replay 渲染一致） | harness 正确性 | 每 commit | mock（无 API） | 无 | **硬门**：红即挡 |
| **L1-mock 场景** | tests/scenarios/*.yaml（10）+ `just eval` dry-run 桩执行 20 题（确定性 stub，全管道演练免 key） | 断言/runner/任务文件本身不回归 | 每 PR | dry-run 桩 | 分钟级 CPU | **硬门**：进 ci.yml（§7） |
| **L2 真模型任务** | `just eval-real` 20 题（→ §2 扩至 23 题） | 默认装配下的解题能力 | 夜间 n=1；发版前 n=3 | 固定低成本档（夜间）/主力档（发版） | 按 horizon 限额表（§2）；软标记不拦截 | **观察**（夜间）/ **发版门**（发版：无新增 timeout_or_limit 且通过率不低于上版区间下沿） |
| **L3 外部基准** | bench_runner 三套件，n=3 + citability | 赛道标尺 + 防自嗨 | 每周 + 发版前；workflow_dispatch 随时 | 主力档 | delegation timeout 3600s（现有） | **引用门**：CitationBlock.citable=false 的分数禁止出现在任何对外材料 |
| **L4 在线信号** | signals.rs 计数（反馈/中断/接管/rewind 率） | 真实体验 | 持续（默认关，opt-in） | — | 只计数不传内容（DP5） | **无门禁**：趋势输入看板，反向喂 L1 任务设计 |

编排原则（与业界对齐）：Inspect AI 以 Task = dataset + solver + scorer 组合并按场景选沙箱与判据（§9-S6），OpenCompass 用配置层把模型/数据集/任务切分/执行/评审五层解耦（§9-S7）。Shannon 的对应物已经存在——TOML 任务 = dataset+solver，ValidationRule/verify script = scorer，EvalOptions = 配置层；**缺的只是「哪个层在什么时候由谁触发」的调度矩阵**，由 §7 的两个 workflow 落地。

L4 回流闭环：dashboard.rs 增加第四个视图「在线信号趋势」（从 analytics 聚合 JSONL 读 SignalCounters 序列），使「夜间 L2 通过率下降」可与「线上中断率上升」互相印证。工作量 S。

### 工作量

层定义文档（本文）S；L4 看板列 S；触发矩阵实现归 §7（M）。

### 验收

- 本表成为 README 级文档，任何新评测资产先声明落哪层、谁触发。
- dashboard.html 出现 L4 趋势视图且开启态才有数据（关闭态显示 opt-in 提示）。
- 每份 RunReport/BenchReport 能自证属于哪层（L2 无 suite 字段 vs L3 有 suite+pin fingerprint——现状已可区分，无需改代码）。

---

## 2. 短/中/长程任务体系（horizon 分级）

### 目标

任务按**预期完成长度**（而非能力维度）分级，使限额、预算预期、触发频率三者随 horizon 联动；20 题扩为 23 题并纳入 2–3 道 long-horizon 任务。

### 现状差距

- `EvalTier`（read/edit/search/multi_step/recovery）是**能力维度**，没有长度维度；一道 3 分钟的读题和一道 45 分钟的重构题在报告里同权。
- 限额只有硬顶：TaskLimits{max_turns, max_tokens, timeout_secs}，超限即失败类 timeout_or_limit。**没有「预期预算」概念**——一道题用了 900k tokens 到底是正常发挥还是浪费，报告无法回答。
- 20 题全部可在 600s 内完成（实测 limits 450–600s），**无 long-horizon 覆盖**；而 Terminal-Bench 的任务按论文设定可跑至 90 分钟（§9-S4），长任务能力在 L2 完全空白，只能靠 L3 代理观测。

### 方案

**① horizon 字段**（向后兼容）：`EvalTask` 增 `horizon: Option<Horizon>`（short|mid|long，缺省 short），TOML 可选字段。ResolvedLimits 改为「任务显式 limits 优先，否则取 horizon 默认表」——现有 20 题因已显式声明 limits 零行为变化。

**② 限额默认表**（horizon × 限额 × 预期预算）：

| horizon | max_turns | max_tokens | timeout_secs | expected_tokens（软） | expected_turns（软） | 首选触发 |
|---|---|---|---|---|---|---|
| short | 12 | 80k | 300 | 30k | 8 | PR 子集 |
| mid | 50 | 1M | 900 | 300k | 25 | 夜间 |
| long | 200 | 4M | 2700 | 1.5M | 80 | 每周 + 发版前 |

2700s 与 Terminal-Bench 的量级衔接（其单任务上限即此量级，§9-S4），保证 L2 long 与 L3 delegation 不出现「harness 自己先掐死」的口径差。

**③ over_expected 软标记**：TaskLimits 增 `expected_tokens: Option<u64>` / `expected_turns: Option<u32>`（缺省取上表）；run_task 收尾时比对实际值，产出 `budget_flags: Vec<String>`（如 `over_expected_tokens`、`over_expected_turns`）写入 TaskRunRecord 与 stable_view。**语义与 CostBelow 断言严格区分**：CostBelow 是任务作者的验收判据（违反即 fail，硬）；expected_* 是运行观测标记（不 fail、不改 RunStatus，软）——这与 DP1「取消预算上限、仅保留工程护栏」一致：硬顶防挂死，软标回答「值不值」。

**④ long-horizon 任务路线**（+3 题 → 23 题；约束：L2 任务保持语言无关的文本验证（grep/diff 矩阵），编译循环型长任务归 L3 Terminal-Bench delegation——这是两层的分工边界，避免 L2 沙箱被工具链拖垮）：

| id | tier | horizon | prompt 骨架 | 判据要点 |
|---|---|---|---|---|
| long_01 | multi_step | long | 跨 5 文件重命名核心类型 `RequestContext` 并同步全部 use/import 与文档引用；毒丸：tests/fixtures 下同名测试夹具**不得**改动 | verify script：全仓 grep 矩阵（改点全中 + 夹具零污染）；轨迹：LSP rename_symbol 族 optional + Edit 族 strict（复用 §3 等价类） |
| long_02 | recovery | long | 三个互相矛盾的配置文件按 README 声明的优先级规则统一，并更新 CHANGELOG 行；含一处诱导性错误优先级描述 | verify：三文件终态 + 文档行；考察「先读规则再动手」的长程一致性 |
| long_03 | multi_step | long | 新增 verbosity flag 从 CLI 参数层穿透到 3 个调用点并更新帮助文本（mock 代码库，纯文本） | verify：flag 全链路 grep + diff_matches 关键文件；expected_tokens=1.5M 软标 |

预期画像：turns 30–80、tokens 0.5–2M——与 METR 的时间地平线方法论一致（按任务的人类耗时长度建模成功率，§9-S8）：long 题不追求高通过率，追求**通过率随版本可测地爬升 + 失败分类可归因**。

### 工作量

horizon 字段 + 默认表 S；over_expected 管道 S；long_01–03 出题与 dry_run 桩 M（每题含毒丸设计）。

### 验收

- `just eval --list` 输出带 horizon 列；23 题干跑全绿。
- 一道故意超支的 dry-run 题在 report 中出现 `over_expected_tokens` 且状态仍为 passed。
- long_01 在真模型下完成首跑，报告单列其 turns/tokens 与短题不可比（按 horizon 分桶呈现）。

---

## 3. 公平断言语义（等价类 · outcome 优先 · args_regex 治理）

### 目标

轨迹断言不因模型选择了「同样合法的另一条路」而误杀；断言的严格度分级有章可循；参数正则不再依赖序列化形状。

### 现状差距

- **已达标部分（不再重做）**：TrajectoryStep 的 tools 族（`["Edit","MultiEdit","rename_symbol"]`）与 optional（means-not-contract，侦察步不缺席不失败）已落地（scenario.rs），args_regex 的紧凑 JSON 假设也已在文档注释里写明。
- **缺口 1——等价类是每题手抄的**：edit_01 手写含 rename_symbol，multi_01（模块重命名）却只有 ["Edit","MultiEdit"]——而 LSP 的 rename_symbol 同样能改 `mod` 声明（shannon-tools/lsp.rs 的 7 个 LSP 工具里 rename_symbol 就位）。同一语义操作在两题里的「合法工具集」不一致，这就是不公平断言的温床。
- **缺口 2——outcome 优先原则未成文**：轨迹断言（TrajectoryContains/ForbiddenTool）与 outcome 断言（file/exit/script）在规则表里平权，哪些轨迹要求是「行为性硬约束」（如禁止 Bash 绕过）、哪些是「风格偏好」（如必须先 Read），没有评审标准，任务作者随手写就会造成误杀。
- **缺口 3——args_regex 脆弱性**：只匹配紧凑 JSON（无 `:`/`,` 后空格），文档写了但没有任何机制防止任务作者写出只能在 pretty JSON 下命中的正则。

### 方案

**① 工具等价类注册表**：新增 `tests/eval/tool_families.toml`，声明语义族 → 工具集（`rename = ["Edit","MultiEdit","rename_symbol"]`、`read = ["Read","Grep","find_references","go_to_definition"]`…）；TrajectoryStep 增 `family = "rename"` 引用写法（与 tools 二选一）。所有 20+ 题的族引用统一改走注册表，逐题手抄消亡；族表本身入 review 范围——**改一处，全任务生效**。edit_01 与 multi_01 的 rename 族不一致问题在此自然修复。

**② outcome 优先原则成文 + severity 分级**：
- 原则：verify（outcome）是**充分判据**——outcome 全绿则任务判 passed 是终局；轨迹断言只服务两类目的：(a) 防绕过（ForbiddenTool 类硬约束）、(b) 行为回归（如「必须先验证再写」的 recovery 考点）。
- 落地：`expectations.trajectory_severity = "strict" | "advisory"`（缺省 strict，保持现有 20 题语义不变）。advisory 时轨迹规则违例记入 `advisory_violations` 字段呈现于报告但**不改 passed**。新任务默认 advisory，升 strict 需在 review 中给出「此断言是行为性约束而非风格偏好」的理由——把严格度从默认变成显式决定。
- 评审 checklist 写入任务文件头部注释模板：轨迹断言必须模型无关（不假设唯一解法）、必须可被 dry_run 桩演示。

**③ args_regex 三步治理**：
1. **加载期 lint（立即）**：`EvalTask::validate()` 扩展——若 dry_run.steps 提供了 input，则 args_regex 必须能匹配 `canonical_arguments(input 的紧凑序列化)`，否则报配置错误（干跑先红，不进真跑）。
2. **结构化替代（随后）**：TrajectoryStep 增 `args_contains: Vec<{key, value_regex}>`——对 JSON 解析后的字段值做正则匹配，不依赖序列化形状；新任务推荐 args_contains，args_regex 降级为遗留兼容。
3. **存量清理（随 ② 走）**：迁移期结束时 grep 全部任务文件，args_regex 仅允许出现在 args_contains 无法表达的场景（如跨字段联合模式）。

### 工作量

族表 + family 字段 S；severity 分级 S；lint S；args_contains M；23 题迁移与复审 M。

### 验收

- tool_families.toml 存在且 ≥2 题改用 family 引用后干跑全绿。
- 一道故意写错 args_regex 的任务在 `just eval --list`/validate 阶段报错（真跑前拦截）。
- 一道 advisory 任务在真模型下轨迹违例但 passed=true，报告可见 advisory_violations。
- 任务文件头注释模板进 tests/eval/tasks/README（或首个任务文件内注释）。

---

## 4. 统计纪律（n=3 · 方差归因 · flaky 隔离 · 元锚点 diff 协议）

### 目标

任何跨 run/跨版本的数字对比都自带归因护栏：模型变了不许混比，单次结果不许外引，flaky 不混能力分。

### 现状差距

- **已达标部分**：stable_digest 已剔除时间戳/路径、eval-diff 已能判「结构稳定 vs 枚举差异」（A3）；L3 层 n=3 + resolved_rate_interval + attribute_variance + CitationBlock 已达标（A8）。
- **缺口 1——L1 是 n=1**：run_suite 每任务单遍，20 题单次通过率没有区间；agent 高方差下（业界实测 SWE-bench 类 run-to-run 方差 5–10%，§9-S5）单次夜间结果不可引用，但我们每天都在产生这种不可引用的数字。
- **缺口 2——报告没有模型锚点（A15 半成）**：RunReport 无 model_id/provider 字段。harness 归因纪律（主计划 §4.13：模型与 harness 变更不得同 run）目前**靠自觉**——换模型跑出的 report 与昨天的 eval-diff 会安静地比出一个「退步」。L0 的 request/header wire_body 里明明有 model 字段，runner 已在读 events.jsonl，只是没提取。
- **缺口 3——flaky 无制度**：研究文档提出过隔离原则（不混能力分），代码无任何落点。
- **缺口 4——diff 协议只看结构**：stable_digest 相同即报 STABLE，但「digest 不同」时未区分「模型不同导致的差异」与「代码变更导致的差异」。

### 方案

**① 元锚点（先做，其余项的前提）**：RunReport 增 `anchor: { model_id, provider, profile_digest }`——由 enrich 阶段从该任务 l0-home 的 request/header wire_body 提取（模型字段即 wire_body 所见，最诚实）；dry-run 桩运行时记 `anchor: { model_id: "dry-run-stub" }`。stable_digest 纳入 model_id。

**② ATTRIBUTE-SPLIT diff 协议**：compare_reports 在比对 stable_digest 前**先比锚点**：
- model_id 或 failure_rules_fingerprint 不一致 → 输出 `ATTRIBUTE-SPLIT` 警告块，列明哪个维度变了，拒绝给出「STABLE/退步/进步」的单句结论（数字照列，结论不发）。
- 锚点一致 → 现有 digest 逻辑照旧。
规则表 fingerprint、directive_present、pin_manifest_fingerprint 均已在 digest 内（现状），协议只需补模型维度的显式拦截。

**③ L1 n=k 聚合**：eval_runner example 增 `--n <k>`（默认 1）：套件跑 k 遍，产出 k 份常规 report + 1 份 `aggregate.json/md`——每任务的 status 序列、通过区间 `[min,max]`（复用 resolved_rate_interval 的呈现法）、机械方差标记（把 attribute_variance 的输入从 BenchRepRecord 泛化为 status 序列 + token 序列，纯函数上移复用）。预算口径：夜间 n=1，每周一次 n=3，发版前 n=3（与 §1 矩阵一致；DP1 取消美元上限，k 只是工程纪律不是钱闸）。

**④ flaky 制度**：
- 静态：任务 TOML 增 `known_flaky = true`（人工标记位），报告分桶呈现，能力分（通过率）默认剔除 known_flaky 题并单列。
- 动态：aggregate 阶段对「同题 k 遍状态翻转」或「连续两个夜间 run 同题状态翻转」自动产出 `flaky_suspects: [task_id]`——**只怀疑不定罪**，转人工确认后升格 known_flaky 或修任务。

**⑤ 单次结果引用纪律**（文档条款）：任何引用 L2/L3 数字处必须附 n、日期、锚点三元组；n=1 只可内部参考。SWE-bench 官方要求多 run 用唯一 run_id 防缓存污染（§9-S2），我们的 run-id 已含熵（fresh_run_id），协议条文对齐即可。

### 工作量

锚点提取 + ATTRIBUTE-SPLIT M（前置）；--n 聚合 M；flaky 制度 S；方差函数上移复用 S。

### 验收

- 用两个不同模型的真跑 report 做 eval-diff，输出含 ATTRIBUTE-SPLIT 块（自动化测试）。
- `just eval-real -- --n 3` 产出 aggregate.json，含每任务 status 序列与通过区间（干跑下验证机制即可）。
- 一道标记 known_flaky 的题在报告能力分中剔除、单列呈现。
- 文档条款落进本文件 + justfile 注释。

---

## 5. 成本工程（软硬两级 · 按层定价 · 回归告警）

### 目标

成本问题在跑分当场可见（软标）、在趋势里可警（环比）、在引用里可算（cost-per-resolved），且不违反 DP1（不设预算上限）。

### 现状差距

- **已达标部分**：cost_usd 逐 turn 采集且诚实未知（Option，provider 未报即 null，绝不编造）、L3 的 cost_per_resolved_usd（分母=resolved events，任一观测缺失即整体 null）、看板 cost 列（区分 observed/未知）、CostBelow 硬断言（A4/A7/A10）。
- **缺口 1——「贵但过了」无处安放**：CostBelow 是 fail 型断言，over_expected 是 token 维度（§2）；成本维度没有软标，报告答不了「这题花得值不值」。
- **缺口 2——cost_usd 缺失时无折算**：provider 不回报成本的（部分 OpenAI 兼容端/本地模型），报告只能画「-」，跨版本 cost 趋势断档。
- **缺口 3——无成本回归告警**：tokens/task 或 cost/task 静默爬升无人知晓（看板被动呈现，无主动提醒）。

### 方案

**① 软硬两级语义表**（全体系统一口径，与 §2 over_expected 对偶）：

| 级别 | 机制 | 违反后果 | 落点 |
|---|---|---|---|
| 硬·工程护栏 | max_turns/max_tokens/timeout（现有） | 状态转 turn_limit/token_limit/timeout，计失败类 | TaskLimits |
| 硬·验收断言 | CostBelow{max_usd, per}（现有） | 规则 fail | 任务 verify |
| 软·预算标记 | expected_tokens/expected_turns（§2 新） | budget_flags 标记，不 fail | TaskRunRecord |
| 软·成本标记（新） | limits 增 `expected_cost_usd: Option<f64>` | `over_expected_cost` flag，不 fail | TaskRunRecord |

expected_cost_usd 与 DP1 兼容：它是观测刻度不是上限，语义是「超过此值时报告必须显眼」，不是「超过即停」。

**② 按层定价表**：新增 `tests/eval/pricing.toml`（model → {input, output, cache_write, cache_read} 单价），口径对齐 query_engine/types.rs 的 DEFAULT_PRICING 但外置可改。TaskMetrics 增 `cost_source: provider | estimated | unobserved`：provider 回报优先；缺失时按定价表折算为 estimated 并保留原 Option 语义（estimated 值照常参与求和，但 report 汇总行标注估算占比）；两者皆无才是 unobserved。L3 的 cost_per_resolved 汇总同步携带该标记。**禁止**在 runner 内新增第四种来源或对 unobserved 编值。

**③ 成本回归告警**：夜间 workflow（§7）收尾比较最近两次同锚点 real run：tokens/task 或 cost/task 环比涨幅 >30% → GitHub Step Summary 警告块（非阻塞）+（可选）走 notifier 的 WebhookHandler 通知。30% 阈值对齐 attribute_variance 对 token spread 的敏感度口径（>20% 即标记，告警线略宽）。

### 工作量

expected_cost_usd + flag S；pricing.toml + cost_source M；环比告警 S（随 §7 workflow 落地）。

### 验收

- 一道 provider 不回报成本的干跑/真跑题报告出现 cost_source=estimated 且数值与定价表可对账。
- 故造超支样本，报告出现 over_expected_cost 且 passed 不受影响。
- 环比告警在两次注入样本间触发一次 Step Summary 警告（workflow 干跑验证）。

---

## 6. 失败分类进化（规则表 → 规则+LLM 辅助归因混合）

### 目标

分类保持可证伪、可复现的前提下，把规则表覆盖不了的语义分界（指令误解 vs 模型上限）纳入受控的第二意见通道，并让「规则够不着」的残留可见、可收敛。

### 现状差距

- **已达标部分**：7 类规则表外置 TOML、13 信号词汇、首中即停、fingerprint 入报告、分类基于事件形状而非模型名（A5）——这套骨架的正确性设计（可证伪：每条规则是事件条件合取，命中即给证据）恰好是 LLM 归因应当对标的形态。
- **缺口 1——未分类残留无度量**：classify 返回 Option，unclassified 样本既不进 failure_class_tally 也不单独呈现，规则表的盲区不可见。
- **缺口 2——语义分界靠猜**：instruction_misunderstanding（模型没读懂题）与 model_ceiling（读了但做不到）的事件形状可以完全一致——规则表永远分不开，这正是业界把「多智能体失败自动归因」当作独立研究问题立项的原因（§9-S9）。
- **缺口 3**：直接上 LLM-as-judge 有已知可靠性坑：一致性失败普遍（有统计称 93% 团队遇到，§9-S10）、标准欠定义时 judge 失效（§9-S11）——不可证伪的 judge 比没有更糟，因为它污染分类分布的公信力。

### 方案：三阶段混合路线

**阶段 1（立即）——残留可见化**：failure_class_tally 增 `unclassified` 桶（failed 且无命中规则）；看板失败类矩阵同列。配套流程条款：每个 unclassified 归档样本人工复核时，**先尝试**把归因写成信号条件补进 failure_rules.toml（规则可表达 → 补规则，fingerprint 变更随 ATTRIBUTE-SPLIT 协议提示）；规则表达不了（需要语义理解）才进 LLM 候选池。

**阶段 2（一月档）——LLM 第二意见通道**（只建议、不裁决）：
- **触发**：仅 failed 且（规则未命中 或 命中类 ∈ {instruction_misunderstanding, model_ceiling} 这对规则无解的分界）。
- **输入摘要器**：从 events.jsonl 生成 ≤8k token 摘要——prompt + 每工具调用一行 signature（复用 call_signature）+ 错误/拒绝事件行 + verify 输出。**不送原始 chunk、不送全文日志**（成本与隐私双约束）。
- **输出契约**：`{class ∈ 7类, key_step_seq, evidence_quote, confidence}`。
- **机械校验（防不可证伪的核心）**：evidence_quote 必须能在摘要原文中逐字找到、key_step_seq 必须真实存在——任一不满足则整条丢弃（judge 说得出证据才配说话）。
- **落点**：写 `llm_suggested_class` + `llm_judge_fingerprint`（模型 ID + 摘要格式 hash + prompt hash）独立字段；**不改 failure_class 主字段、不进分类分布、不改任何分数**。人工复核确认后，转化为规则表补丁或 known 模式，主字段才变。
- **校准闸门**：人工标注集 ≥30 例（由阶段 1 归档积累），judge 与人工一致率 ≥80% 才允许其建议进复核队列；规则表或摘要格式每次变更后重测。校准数字随报告披露。

**阶段 3（季度档，评估后决定）**——是否将 judge 建议升格为分布视图的独立泳道（llm_suggested 分布 vs rule 分布并排呈现），依阶段 2 的校准数据说话。**不预设结论**。

### 工作量

阶段 1 S；阶段 2 M–L（摘要器 M、契约与校验 S、校准集运营 L——主要是人工标注精力）；阶段 3 仅评估 S。

### 验收

- 报告与看板出现 unclassified 桶且数字与归档样本数可对账。
- 注入一个 judge（或 mock）返回编造 evidence_quote 的样本：该建议被丢弃且有日志（机械校验测试）。
- 校准集 ≥30 例、一致率数字落档；llm_suggested_class 字段在真实失败样本上出现且主字段未变。
- failure_rules.toml 每次补丁的 fingerprint 变化触发 ATTRIBUTE-SPLIT 提示（§4 联动）。

---

## 7. CI/CD 集成（PR 门 · 夜间全量 · 归档与最小复现 · 看板告警）

### 目标

评测进流水线：PR 有便宜可靠的门，夜间有带锚点的全量真跑，失败样本开箱即复现，看板与告警自动产出。

### 现状差距

- A14：现有 workflow 全是工程性能 bench（criterion 基线），**没有任何 agent-eval 工作流**；`just eval`（dry-run）与 `just eval-real` 只存在于开发者本机。
- 失败归档（A3）已存 events.jsonl+result.json+stream，但**复现要靠人肉拼命令行**——归档里没有可直接执行的复现命令。
- 看板（A10）需手动生成，无 CI 呈现与告警出口。

### 方案

**① PR 快速门（进 ci.yml，零 API key）**：新增 step 跑 `just eval`（默认 dry_run 桩，20+3 题全量，分钟级）。它验证的是**评测资产本身**（任务 TOML 可解析可 validate、runner/断言/规则表不回归、dry_run 桩与新任务同步）——真模型能力永不作 PR 门（成本、方差、flaky 三重理由）。

**② eval-nightly.yml（新 workflow）**：
- 触发：cron 周一至周五深夜 + workflow_dispatch。
- job 真跑：`just eval-real --tasks …` 20+3 题 n=1，模型经 env/配置固定为低成本档并**写入锚点**（§4）；secret 注入 SHANNON_API_KEY；限流重试对齐 Inspect 的 max-retries 姿势（瞬态错误重试，样本级失败不静默重跑——重跑归 n 聚合管，§9-S6）。
- 周六 dispatch 版：`--n 3` 出 aggregate（§4）。
- 产物：runs 目录整体 upload artifact（保留 90 天）；dashboard 生成并作为 artifact +（可选）GitHub Pages 发布。
- 周日让位给既有 benchmarks.yml（工程性能与 agent 能力错峰，互不挤占）。

**③ 最小复现**：runner 写归档目录时同步生成 `repro.sh`（内容一行：`SHANNON_EVAL_BIN=<bin> SHANNON_EVAL_REAL=1 cargo run -p shannon-core --example eval_runner -- --real --task <id> --directive <若有>`），result.json 增 `repro_command` 字段。归档样本从「证据堆」升级为「一键重放」。

**④ 告警**：workflow 收尾 step——
- 通过率较上一同锚点 run 下降超区间下沿（§4 aggregate 口径）→ Step Summary 警告（非阻塞，夜间 job 保持 continue-on-error 性质，门禁只在发版流程人工执行 eval-real 时生效）；
- 成本环比 >30% → 警告（§5）；
- 可选 webhook 通知复用 notifier WebhookHandler（Slack/Discord 模板现成）。

**⑤ 发版门（人工触发，非 CI 自动）**：release 流程执行 `eval-real --n 3` + `bench_runner --real`（主力档），发版 notes 引用带 CitationBlock 的分数——与 §1 矩阵「L2/L3 发版门」对应。

### 工作量

PR 门 S；eval-nightly M（含 secret/缓存/并发控制：并发 1 防止两 run 互踩 API 配额）；repro 生成 S；告警 step S。

### 验收

- PR 上可见 dry-run 评测门并在任务文件写坏时变红（注入测试）。
- eval-nightly 手动 dispatch 首跑成功：artifact 含 runs 目录 + dashboard.html，Step Summary 出现结果矩阵。
- 任取一个失败归档目录，`bash repro.sh` 可重放该题（同锚点下）。
- 两个注入的低分/超支样本各触发一次对应警告块。

---

## 8. 路线图（两周 / 一月 / 一季）

依赖主线：**锚点（A1）→ 一切跨 run 对比**；horizon 字段 → long 任务；unclassified 度量 → 校准集积累 → LLM 归因。

### 两周档（全部 S，可并行，无外部依赖）

| # | 项 | 对应章节 | 依赖 |
|---|---|---|---|
| W1 | RunReport 元锚点（model_id/provider/profile_digest，从 wire_body 提取）+ ATTRIBUTE-SPLIT | §4①② | 无 |
| W2 | horizon 字段 + 限额默认表 + over_expected budget_flags | §2①②③ | 无 |
| W3 | tool_families.toml 等价类 + family 引用 + EvalTask::validate 的 args_regex lint | §3①③ | 无 |
| W4 | PR dry-run 门进 ci.yml | §7① | W3 完成后更稳（validate 拦截） |
| W5 | unclassified 桶 + 归档 repro.sh/repro_command | §6 阶段1、§7③ | 无 |

### 一月档（M 为主）

| # | 项 | 对应章节 | 依赖 |
|---|---|---|---|
| M1 | long_01–03 三道 long-horizon 题（含 dry_run 桩与毒丸） | §2④ | W2 |
| M2 | `--n` 聚合 + aggregate.json + flaky_suspects/known_flaky | §4③④ | W1（聚合带锚点） |
| M3 | eval-nightly.yml + dashboard artifact + 通过率/成本告警 step | §7②④ | W1、W4 |
| M4 | pricing.toml + cost_source 三态 + expected_cost_usd | §5①② | W2（flags 管道复用） |
| M5 | 失败样本 → 自建回归集回流脚手架（failures/<date> 目录提炼新回归 TOML 的工具与流程） | §6/§1 | W5 |

### 一季档

| # | 项 | 对应章节 | 依赖 | 规模 |
|---|---|---|---|---|
| Q1 | LLM 第二意见归因（摘要器 + 契约 + 机械校验 + ≥30 例校准集） | §6 阶段2 | W5、M5 积累标注 | M–L（人工标注是长杆） |
| Q2 | L4 信号趋势视图进 dashboard | §1 | 无（骨架已在） | S |
| Q3 | trajectory_severity 分级落地 + 23 题复审迁移 | §3② | W3 | M |
| Q4 | L3 扩容评估（SWE-bench Live 子集 / τ-bench 选型，**只评估不承诺**）+ args_contains 全量迁移收尾 | §3③、§9-S1 | M2 数据支撑 | M |
| Q5 | 阶段 3 评估：judge 建议是否升格独立泳道 | §6 阶段3 | Q1 校准数据 | S（评估） |

---

## 9. 业界参照与来源

本设计与业界实践的显式对齐点：

1. **分层与门禁**：OpenCompass 五层架构（配置/任务切分/执行调度/推理/评审）证明「层间解耦、配置驱动」是横评平台的通用形态；Shannon 的 TOML 任务 + EvalOptions + ValidationRule 已是同构物，本文只补调度矩阵。
2. **可复现与缓存坑**：SWE-bench harness 按 run_id 缓存结果，多 run 必须唯一 run_id——我们的 fresh_run_id 已含时间戳+熵，协议条文对齐。
3. **方差量级**：SWE-bench Verified 的 run-to-run 分辨率方差 5–10%（Epoch AI），SWE-Bench++ 报告 95% CI——支撑 §4「n=1 不可外引」与区间呈现。
4. **长任务标尺**：Terminal-Bench 任务 = 指令 + Docker 环境 + 验证脚本 + 示例解 + 时限（可达 90 分钟）——§2 的 long 档时限 2700s 与其对齐；L2 保持文本验证、编译型长任务让给 L3 的分工亦源于此。
5. **判据原生化**：Terminal-Bench/SWE-bench 判据一律用基准自带验证器、不自造——Shannon 的 judge_statement + is_scored 已在代码层强制（A7）。
6. **harness 工程姿势**：Inspect AI（UK AISI）的 Task=dataset+solver+scorer、模型级 max-retries/样本级 retry-on-error、沙箱 provider 矩阵——§7 重试策略与 §1 沙箱表述的参照。
7. **配置层解耦**：OpenCompass 的 reader_cfg/infer_cfg/eval_cfg 三段配置——对应我们 EvalOptions/TaskLimits/FailureRules 的既有拆分（已达标，不再重构）。
8. **长程能力度量**：METR 50%-time-horizon（按人类耗时建模成功概率，约 7 个月翻倍）——§2 long 题的预期画像与「通过率可测爬升」目标的方法论出处。
9. **失败归因**：「Which Agent Causes Task Failures and When?」（arXiv 2505.00212）将失败自动归因形式化为独立问题——§6 承认规则表在语义分界上的天花板、引入受控第二意见的依据。
10. **judge 可靠性**：LLM-as-judge 一致性失败普遍（Galileo：93% 团队遇到重大可靠性问题）、判据欠定义时 judge 失效（NHIMG）——§6 阶段 2 的机械校验 + 校准闸门的动机。

来源清单：

- S1: [SWE-bench Evaluation Guide](https://www.swebench.com/SWE-bench/guides/evaluation/)
- S2: [SWE-bench 仓库（run_id 缓存注意）](https://github.com/swe-bench/SWE-bench) ／ [官方实验与轨迹公开](https://github.com/swe-bench/experiments)
- S3: [SWE-Bench++（95% CI 报告实践）](https://arxiv.org/html/2512.17419v1)
- S4: [Terminal-Bench](https://www.tbench.ai/) ／ [任务结构论文](https://arxiv.org/html/2601.11868v1) ／ [Registry & Adapters](https://www.tbench.ai/news/registry-and-adapters)
- S5: [Epoch AI: What skills does SWE-bench Verified evaluate（方差 5–10%）](https://epoch.ai/publications/what-skills-does-swe-bench-verified-evaluate)
- S6: [Inspect AI](https://inspect.aisi.org.uk/) ／ [CLI 参考（max-retries/retry-on-error）](https://inspect.aisi.org.uk/reference/inspect_eval.html) ／ [Sandboxing](https://inspect.aisi.org.uk/sandboxing.html) ／ [Model Grading](https://inspect.aisi.org.uk/model-graded.html)
- S7: [OpenCompass](https://github.com/open-compass/opencompass) ／ [架构论文（五层）](https://arxiv.org/html/2605.19276v2) ／ [LLM Judge 指南](https://doc.opencompass.org.cn/advanced_guides/llm_judge.html)
- S8: [METR Time Horizons](https://metr.org/time-horizons/) ／ [长软件任务测量](https://metr.org/blog/2025-03-19-measuring-ai-ability-to-complete-long-tasks/) ／ [Time Horizon 1.1](https://metr.org/blog/2026-1-29-time-horizon-1-1/)
- S9: [Which Agent Causes Task Failures and When?（arXiv 2505.00212）](https://arxiv.org/html/2505.00212v3)
- S10: [Galileo: Why LLM-as-a-Judge Fails](https://galileo.ai/blog/why-llm-as-a-judge-fails)
- S11: [NHIMG: LLM-as-a-Judge Fails When Eval Criteria Are Underspecified](https://nhimg.org/articles/llm-as-a-judge-fails-when-eval-criteria-are-underspecified/)

---

## 10. 负面清单：现状已达标，禁止重复建设

1. **采集层**（L0 事件日志、指标提取、trace 四子命令、OTLP 桥）——A1/A4/A12 已达标。任何「为评测再加一套埋点/日志」的提案一律拒绝；评测侧只消费 L0。
2. **断言层**（11 种 ValidationRule、轨迹 tools 族 + optional）——A6 已达标。不重写 scenario.rs；扩展只走新增规则/等价类/args_contains 的增量路径。
3. **可引用纪律**（pin manifest + fingerprint、判据原生化 judge_statement、BenchDisposition 8 态、CitationBlock citability 门控、n=3 + resolved_rate_interval、attribute_variance）——A7/A8 已达标。不另造「分数引用规范」；L1 只复用这些既有机制（§4）。
4. **报告与对比机制**（双报告、stable_digest、compare_reports/eval-diff、静态看板）——A3/A10 已达标。跨版本对比只补元锚点与 ATTRIBUTE-SPLIT 拦截，不动 digest 结构本身。
5. **A/B 指令开关**——A9 已达标。策略实验直接用 --directive，不另建分支机制。
6. **分类骨架**（7 类外置规则表、信号词汇、首中即停、fingerprint）——A5 已达标。LLM 归因只能作为第二意见挂在骨架旁（§6），不允许替换主分类路径。

以上六项是本文所有方案的地基约束：**新增设施必须与它们对接，而不是平行再造。**
