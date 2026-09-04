# Agent 评测全景调研与 Shannon 评测实施方案

- 日期：2026-08-27
- 任务：调研 agent 评测方法论（数据集/任务集、指标、评测内容），结合 Shannon 现状列问题与改进点，制定评测实施/改进方案
- 输入：[差距分析](shannon-gap-analysis.md)（P6/P2）、[trace 方案](shannon-trace-improvement-plan.md)（L0 依赖）、[DSH 调研](deepseek-harness-analysis.md)、[Pi 调研](pi-agent-analysis.md)（Composio 数据）
- 结论速览：**Shannon 目前的「测试」只覆盖 harness 正确性，没有任何一层在测「agent 好不好用」。** 建议建立五层评测金字塔（L0 单元 → L1 场景集扩容 → L2 基于事件日志的轨迹评测 → L3 外部基准适配 → L4 在线信号），核心新增设施是一个离线 eval runner + 以 [trace L0](shannon-trace-improvement-plan.md) 为数据源。

## 1. 评测范式全景：agent 到底怎么评

| 范式 | 判什么 | 怎么判 | 代表 | 适用 |
|---|---|---|---|---|
| 结果判据（outcome） | 任务完成没有 | 测试通过/diff 匹配/文件状态/校验脚本 | SWE-bench、Terminal-Bench | 硬任务，客观可复现 |
| 轨迹审查（process/trajectory） | 过程是否合理 | 步骤序列断言、工具调用合法性、循环/浪费检测 | 内部回归、AgentBoard | 防回归、诊成本 |
| LLM-as-judge | 开放式质量 | 评分模型按 rubric 打分（成对比较更稳） | MT-Bench、内部 rubric | 对话/摘要/无标准答案任务 |
| 在线信号（online） | 真实体验 | 用户反馈（👍/中断率/复用率/接管率/修正率） | 各产品遥测 | 最终真相，滞后且有偏 |

成熟团队普遍四层叠加：**CI 里跑轨迹断言，夜里跑结果基准，发布前跑 judge 抽样，线上收真实信号回流。** 单一范式都有盲区：结果判据漏「做对但绕远路」（成本失控），轨迹审查漏「过程漂亮结果错」，judge 有偏，在线太慢。

## 2. 数据集/任务集地图（按与 Shannon 的相关度排序）

| 基准 | 任务形态 | 判据 | 与 Shannon 相关度 | 备注 |
|---|---|---|---|---|
| **SWE-bench Verified** | 真实 GitHub issue 修复（500 题，人工过滤） | fail-to-pass + pass-to-pass 测试 | ★★★ | coding agent 标尺；Shannon 直面该赛道 |
| SWE-bench Live / Multi | 周更新防污染 / 多仓库协同 | 同上 | ★★ | Live 抗过拟合，Multi 测多 agent——Shannon 有 teammate 体系可对标 |
| **Terminal-Bench** | 终端内多步任务（编译/数据/运维） | 自带验证脚本 | ★★★ | 与 CLI 形态完全同构，最适合先接 |
| τ-bench / τ²-bench | 用户模拟 + API 工具调用（客服/双 agent 博弈） | 任务状态比对 | ★★ | 测工具编排与澄清能力 |
| Aider polyglot | 多语言小编辑题 | 单元测试 | ★★ | 快速回归编辑能力，225 题轻量 |
| GAIA | 通用助手任务（检索/推理/多模态） | 精确答案匹配 | ★ | 覆盖面广但偏研究 |
| OSWorld / WebArena | 桌面/浏览器 GUI 操作 | 屏幕状态 | ★ | Shannon computer-use 为 feature-gated，缓议 |
| MLE-bench | Kaggle 竞赛级 ML 工程 | Kaggle 指标 | ★ | 长任务/自主性参考 |
| BFCL / Toolathlon | 函数调用准确性 / 多工具组合 | 结构化判据 | ★★ | BFCL 测工具选择，与多 provider 适配相关 |
| Composio SaaS 基准 | 30 个真实 SaaS 工作流 | 端到端成功 | ★★ | 已有横评数据可定位：Pi 66.7%/$0.028、Prime 62.5%、Claude Code 53.3%、OpenCode 46.7% |
| 自建回归集 | 本仓真实 bug 复现 | git diff + 测试 | ★★★ | 最贴近自身演进，SWE-bench 类 agent 的通行做法（用自己修过的 issue 做回归） |

选型结论：**先 Terminal-Bench（同构、自带判据）+ SWE-bench Verified 子集（赛道标尺）+ 自建回归集（自身演进），其余按需扩。**

## 3. 指标体系：评什么数

- **能力**：resolved rate（主指标）、pass@k、按任务难度/类别分桶的通过率。
- **效率/成本**：cost-per-resolved（总花费/解决数——Composio 横评用的就是它）、tokens（输入/输出/cache 命中）、turns、wall-clock、工具调用数。
- **稳定性**：同任务 n≥3 次重跑的方差与通过率区间（agent 输出高方差，单次结果不可引用——这是最常见的评测错误）。
- **过程质量**：循环检测（同工具同参数重复）、无效工具调用率、上下文压缩次数、权限阻塞率、用户接管率。
- **失败分类学**（对改进最有行动价值）：① 指令误解 ② 工具失败未恢复 ③ 上下文丢失/过早压缩 ④ 权限误拒 ⑤ 超时/预算耗尽 ⑥ 编辑冲突 ⑦ 模型能力上限。每次失败样本归档并打标，版本间看分布迁移。

前提：以上除 resolved rate 外**全部依赖事件级数据**（token/cache/工具序列/耗时），这正是 [trace L0](shannon-trace-improvement-plan.md) 的产出——没有 L0，只能数出 resolved rate 和 wall-clock。

## 4. Shannon 现状评审（结合代码）

| 现有设施 | 证据 | 判定 |
|---|---|---|
| YAML 场景测试 | crates/shannon-core/src/testing/scenario.rs + tests/scenarios/*.yaml（10 个） | 只测 harness 逻辑：mock 固定回复 + 文件存在/内容包含断言。**模型决策从未被评**（mock 里已写死行为） |
| 断言词汇 | ValidationRule: FileExists/FileContent/FileNotExists/ExitCode/ToolCalled/ResponseContains/MaxDurationMs | outcome-only 且浅：无 diff 断言、无轨迹断言、无成本断言 |
| record/replay | SHANNON_API_KEY=... just record / just replay | 是 fixture 机制不是评测：录制请求响应做回放，无打分 |
| perf_tests | 阈值式延迟测试 | 工程性能回归，与 agent 能力无关 |
| 成本采集 | CostTracker/DEFAULT_PRICING 在 QueryEvent 里 | 有计量但**不落盘**（差距 P2），评测无从归集 |
| 无外部基准适配 | 无 Terminal-Bench/SWE-bench harness | --prompt + NDJSON + --schema 其实已具备 headless 接入条件，缺的只是适配层 |

问题清单（E1–E6）：E1 无任何真实模型参与的回归评测；E2 断言只到文件级，无行为级；E3 成本/token 不入评测；E4 无失败分类与样本归档；E5 10 个场景无难度分层、无禁止事项（如「不得使用 bash」约束类）；E6 结果不进报告（无趋势、无版本对比）。

## 5. 深度思考：Shannon 应该怎么评

三个产品形态（CLI/desktop/gateway）共享一个引擎，评测对象应是**引擎 + 默认配置组合**，而非 UI。定位分层：

1. **Harness 正确性**（现有 just test/scenarios 覆盖）：确定性、mock、CI 每次跑——已达标，保持。
2. **Agent 能力**（新增主战场）：真实模型 + 真实工具 + 沙箱化任务环境，夜间跑。测的是「Shannon 默认装配（系统提示词 + 工具集 + 权限模式）下的解题能力」。这是所有竞品横评实际在测的东西——**换模型带来的分数变化 ≠ 换 harness 带来的分数变化，必须分开归因**（固定模型跑 harness 变更、固定 harness 跑模型变更）。
3. **产品体验**：启动延迟、审批打断次数、通知噪音——用在线信号（L4）+ 人工抽样。

关键取舍：不追「全基准矩阵」——每个外部基准维护成本不低，先三个（Terminal-Bench、SWE-bench Verified 子集 ~50 题、自建回归集），跑通「数据→判据→报告→归因」闭环再扩。自建回归集是最被低估的一项：本仓修过的每个 bug 都可以低成本转成「issue 描述 + 验证脚本」任务，天然防自身回归且零许可问题。

## 6. 实施方案：评测金字塔

### 6.1 结构

| 层 | 内容 | 频率 | 模型 | 判据 |
|---|---|---|---|---|
| L0 单元/场景 | 现有 just test + 10 场景（扩断言词汇：diff、轨迹、禁止工具） | 每 commit | mock | 确定性断言 |
| L1 场景集扩容 | 50+ 任务：分层（读/编/搜/多步/恢复），每任务带 verify 脚本 + 期望轨迹模板 + 禁止项 | 每夜 | 真实（低成本档） | 结果 + 轨迹 |
| L2 轨迹评测 | 消费 [trace L0](shannon-trace-improvement-plan.md) 日志：循环检测、无效调用率、token/成本归集、失败分类打标 | 每夜（与 L1 同跑） | — | 日志分析器 |
| L3 外部基准 | Terminal-Bench adapter + SWE-bench Verified 50 题子集 + 自建回归集；经 --prompt headless + NDJSON 接入 | 每周 / 发版前 | 主力档 | 基准原生判据 |
| L4 在线信号 | desktop/CLI 埋点：会话满意度、中断率、接管率、/rewind 使用率（匿名、opt-in） | 持续 | — | 趋势 |

### 6.2 核心设施

- **eval runner（新 crate 或 tools/ 子命令）shannon eval**：输入任务清单（TOML：prompt、setup、verify、expectations、budget），拉起沙箱工作区（复用 sandbox.rs），headless 跑 --prompt，收集 L0 日志，执行 verify，输出结构化结果 JSON。
- **报告与趋势**：每夜 run 汇总为单页 markdown（按层/任务/指标矩阵）+ 版本间 diff；失败样本（prompt+日志+分类）归档到固定目录供回归。
- **预算护栏**：runner 级别每日美元上限、单任务 turn/token 上限；超限记为「预算耗尽」失败类别而非挂死。
- **方差纪律**：L3 任务默认 n=3，报告通过率区间而非单值；任何对外引用的分数必须附 n 与日期。

### 6.3 依赖与顺序

1. [trace L0](shannon-trace-improvement-plan.md) P0（否则 L2/L3 只剩 resolved rate 与 wall-clock——成本与轨迹指标全缺）。
2. L1 断言词汇扩展（scenario.rs 增加 DiffMatches/TrajectoryContains/ForbiddenTool/CostBelow）——不依赖 L0，可先行。
3. L3 adapter（Terminal-Bench 先行，判据脚本现成）。
4. L4 最晚（需要遥测合规姿势：默认关、匿名、无对话内容上报）。

### 6.4 里程碑

| 里程碑 | 内容 | 验收 |
|---|---|---|
| M1（1 周） | 断言词汇扩展 + 20 个新 L1 任务 + runner 雏形 | 夜间 job 出首份结构化报告 |
| M2（2–3 周，随 trace P0） | L0 日志接入 runner；成本/轨迹指标上线；失败分类打标 | 报告含 cost-per-task、循环检测、分类分布 |
| M3（2 周） | Terminal-Bench adapter + SWE-bench Verified 50 题子集 + 自建回归集 10 题 | 三基准首跑分数 + n=3 区间 |
| M4（持续） | L4 在线信号 + 版本对比看板 | 发版 notes 可引用「本版 vs 上版」能力变化 |

## 7. 风险与对策

| 风险 | 对策 |
|---|---|
| 真实模型评测的 API 成本 | 预算护栏 + 低成本档夜间跑 + 主力档只在发版前；L3 子集固定 50 题 |
| 基准污染/过拟合 | 外部基准只取子集且不针对其调参；自建回归集持续换新 |
| 环境不稳定（网络/沙箱）导致 flaky | 任务容器化（复用 sandbox）；flaky 任务单独隔离标记，不混入能力分 |
| 分数被模型升级淹没 | 归因纪律：模型与 harness 变更不同时进一次评测 run |
| 数据合规（L4） | 默认 opt-out、只上报计数不报内容、文档明示 |

## 8. 总结

Shannon 的测试文化（每文件有测试、nextest 隔离、mockito 回放）在「harness 正确性」层是超出平均水平的；缺口全部集中在「agent 能力」层——而这恰是产品的主张所在。补齐路径清晰且不推翻现有设施：**一个 eval runner、一条事件日志（trace L0）、三个基准、一套失败分类**，即可把「Shannon 变好没有」从感觉变成数字。
