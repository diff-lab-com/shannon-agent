# Shannon Agent 评测与改进实施方案（glm-5.3-flash）

- 日期：2026-09-05
- 分支：`feat/agent-eval-bench`（基于 dev @ 7b8f6fb8，worktree `shannon-eval`）
- 默认被测模型：**glm-5.3-flash @ zhipu-coding-plan**（与 v1 冻结基线同 anchor）
- 状态：**已批准**（2026-09-05 评审通过，D1-D4 决策结论见 §六；P3 开工前另需通过 backlog 评审闸）

---

## 一、现状盘点（调研结论）

### 1.1 业界基准调研结论

对 CLI 编码智能体（terminal + 文件编辑 + git 形态）适配度最高的基准，按梯队分级：

| 梯队 | 基准 | 形态 | 自定义模型 | 成本量级 | 对 shannon 的价值 |
|---|---|---|---|---|---|
| **T1 必跑** | SWE-bench Verified（repo 已 pin 50 题） | 真实 repo 修 bug，官方 docker 判分 | ✅ predictions.jsonl 与 agent 解耦 | ~$0.1-0.7/题（Flash 价位更低） | 最成熟的外部 held-out 探针，工程生态最全 |
| **T1 必跑** | Terminal-Bench（repo 已 pin 9 题；官方 2.x 全量 89 题） | 容器内真实终端任务，task-native `run-tests.sh` 判分 | ✅ | $0.1-2/题 | 与 shannon 形态完全一致；GLM 官方有参照分 |
| **T1 必跑** | 内部 L1 套件（20 题）+ regression 池（10 题） | 自有任务 TOML，eval_runner 判分 | ✅ | 极低 | 回归护栏（**不得用于调 prompt**，held-out 规则） |
| **T1 已批准追加** | Terminal-Bench 2.x 全量（harbor） | 同上，89 题 | ✅ | $10-50 | 与 GLM-5.3-Flash 官方 TB2.1=69.2 同版本直接对标（P1d，目标 `terminal-bench@2.1`） |
| T2 选跑 | Aider Polyglot | 225 题编辑精确性，无 Docker | ✅ | $5-15 | 最便宜的编辑能力冒烟 |
| T2 选跑 | SWE-bench-Live / DeepSWE v1.1 | 抗污染 repo 基准 | ✅ | 同 SWE50 | Verified 的"真实信号"续作 |
| T2 选跑 | τ²-bench / GAIA val 子集 | 通用 agent / 工具多轮 | ✅ | 中 | 非 coding 维度补充 |
| 排除 | OSWorld、WebArena、BrowseComp、TheAgentCompany、Commit0、SWE-Lancer、AgentBench、Vending-Bench 2、lm-evaluation-harness | GUI / web / 整库生成 / 封闭 harness / 静态任务 | — | 高 | 形态不符或成本远超收益（详见调研附录） |

GLM 侧参照系（官方公开口径，z.ai blog 2026-08-14）：
- GLM-5.3-Flash：Terminal-Bench 2.1 = **69.2**（Claude Code harness）、DeepSWE v1.1 = **53.7**（mini-swe-agent harness）
- 用途：shannon+glm-5.3-flash 的外部得分若显著低于官方 harness 得分，差额即为 **shannon scaffolding 损益**的估计。

### 1.2 shannon 既有评测设施（比预期完整，不重造）

| 资产 | 位置 | 状态 |
|---|---|---|
| L1 任务套件 20 题 + eval_runner + 失败分类器 + 聚合器 | `tests/eval/tasks/`、`crates/shannon-core/src/testing/eval_runner.rs`、`eval_metrics.rs`、`eval_aggregate.rs` | 已交付 |
| **v1 冻结基线（glm-5.3-flash）** | `tests/eval/baselines/v1-glm-5.3-flash-20260828/` | 18/20, 20/20, 19/20（均值 95%）；stable_pass=18 flaky=2 stable_fail=0 |
| regression 池 10 题 + run-batch.sh 批次驱动（n 轮/续跑/预算闸/429 自愈） | `tests/eval/benchmarks/regression/`、`scripts/eval/run-batch.sh` | batch-1 已跑（glm-5.3-flash）：reg_01 stable_fail（turn_limit×3）、reg_02 flaky |
| SWE-bench 50-pin + swe-harness.sh（官方 `run_evaluation` docker 判分） | `tests/eval/benchmarks/swebench_verified_50.txt`、`scripts/eval/swe-*.sh` | **管线已充分验证**：历史 8 个 batch 全部真实官方判分 |
| Terminal-Bench 9-pin + tb-prebake 预烘焙镜像 | `tests/eval/benchmarks/terminalbench_tasks.txt`、`scripts/eval/tb-prebake/` | 管线交付；**真实判分从未完整跑通**（corpus 接触失败，见 1.3） |
| provider wrapper 范式（key 注入/模型 id 陷阱/限速） | `scripts/eval/wrapper-minimax.sh` | 已验证（minimax 专用，需照抄 glm 版） |
| 失败分析管道（per-task classification.json + events.jsonl 归档） | `~/.shannon/eval/failures/20260827/` | 已验证 |

### 1.3 关键空缺（本方案要填的坑）

1. **glm-5.3-flash 从未跑过任何外部基准**。SWE50 全部 8 个历史 batch（batch4: 5/56 → batch8: 30/50 → batch11: **33/50**）与 n=3 复测（24/50）都是 **minimax-m3**。用户要求的默认模型恰是空缺。
2. **TB9 真实判分从未完成**：最近一次 run（bench-20260828194734）9 题中大面积 `skipped_env_missing`（pin 清单与官方 corpus 布局漂移），citation gates 判定不可引用（n=1 + pin-vs-corpus 未确认）。
3. **dev HEAD 已前移**：v1 基线与 regression batch-1 停在 8/28-29（app 0.11.0），此后 dev 合入 LoopGuard P2.1/P2.2（进度守卫替换 turn-count 守卫）、goal 预算信号 P2.3 等——reg_01 的 turn_limit stable_fail 是否已被修复，需要复测回答。
4. **跨模型消融的机会窗**：同一 scaffolding 下 minimax-m3 与 glm-5.3-flash 各跑一遍 SWE50，**共同失败 → 疑似 scaffolding 问题；单模型失败 → 模型能力差异**。这是分离"改 shannon"与"换模型"的天然对照实验，此前从未做过。
5. 成本口径：coding-plan 订阅制下 `cost_usd` 上报可能失真，需要补 catalog 参考价列（与 improvement-plan P0-4 成本可观测方向一致）。

---

## 二、总体思路

```
P0 准备 → P1 glm 外部基线（SWE50 + TB9 + regression，n=3；P1d TB 2.x 全量 89 题 via harbor，n=1）
        → P2 失败分析（跨模型消融 + 归因三分法）→ backlog（先审再做，D4）
        → P3 实施改进（证据驱动，逐项验证）
        → P4 复测结题（失败题复放 + 无回归护栏）
```

原则：
- **不重造 harness**：全部复用 `run-batch.sh` / `eval_runner` / `bench_runner` / `swe-harness.sh` 管线。
- **held-out 纪律**：不在 L1 20 题上驱动 per-task prompt/harness 修改（v1 基线 README 明文规则）；外部基准才是改进探针。
- **引用纪律**：任何对外数字必须带 n / 日期 / anchor（model+provider+profile）三元组；只引 stable 分桶；n=1 仅内部。

## 三、控制变量

| 项 | 取值 | 说明 |
|---|---|---|
| model | `glm-5.3-flash` | 用户指定 |
| provider | `zhipu-coding-plan`（`https://open.bigmodel.cn/api/coding/paas/v4`） | 与 v1 基线/regression batch-1 同 anchor，保证可比（决策点 D1） |
| key 注入 | `wrapper-glm.sh`：`~/.shannon/credentials/zhipu.json` → `SHANNON_API_KEY`，`env -u` 其它家 key | 照抄 wrapper-minimax.sh 范式；**注意 batch-5 RCA 教训：`--model` 必须传真实 API id** |
| 限速 | `SWE_MIN_DELAY_MS=30000`（30s/调用） | batch-6 RCA：无限速时 76% 请求被限流 |
| 轮次 | SWE50/TB9 n=3 串行；regression n=3 | 引用最低门槛 |
| 预算硬闸 | `run-batch.sh --budget-tokens`（值见 §五） | 退出码 3=预算停，可原位续跑 |
| 二进制 | 复用主 checkout 已构建 `--bin`，worktree 不重编译 | 仓库既定 bypass 约定 |
| harbor 运行（P1d 专用） | `terminal-bench@2.1`，n=1；adapter 内固定 `--max-turns` / 超时 / `--output-format json-stream` | 与 GLM 官方 TB2.1=69.2 参照同版本（2.1 修复了 2.0 的 28 道缺陷题）；`@2.1` 不可得时降级 `@2.0` 并在报告显著标注 |
| thinking/effort | GLM-5.3 系列 thinking 默认开启且 effort 默认 max——固定默认档位并记录进 anchor | effort 档位是成本最大变量 |

## 四、实施阶段

### P0 准备（约 0.5 天，预算 ≤ 2M tokens）
1. 写 `scripts/eval/wrapper-glm.sh`（key 注入 + `--provider zhipu-coding-plan --model glm-5.3-flash` + env 清理 + 限速状态文件 `/tmp/shannon-glm/.pacing-state`）。
2. 连通性 smoke：`just eval-real --task read_01` 1 题实跑通过。
3. **TB corpus 修复**：定位 `skipped_env_missing` 根因（pin 清单 vs 官方 corpus 布局），重建 corpus view，`pin_validation` 达成 `confirmed`；单题端到端真实判分 1 次通过（复用 tb-prebake 镜像）。
4. 确认订阅额度与限流窗口（coding-plan 每 5 分钟 prompts 上限），据此微调 pacing。

**验收**：wrapper smoke 通过；TB 1 题真实 verdict.json 产生。

### P1 glm-5.3-flash 外部基线（3-5 天，含 P1d）
| 顺序 | 套件 | 规模 | 目的 |
|---|---|---|---|
| 1a | regression 池 × n=3 | 10 题×3 | 当前 HEAD 回归状态；回答 reg_01 是否已被 LoopGuard 修复 |
| 1b | SWE-bench Verified 50-pin × n=3 | 50×3 | glm 外部主榜；与 minimax batch11 33/50 直接对照 |
| 1c | TB 9-pin × n=3 | 9×3 | 首个可引用 TB 分数 |
| 1d | TB 2.x 全量（harbor，`@2.1`） | 89×n=1 | 与 GLM 官方 TB2.1=69.2 同版本精确对标（含 adapter 开发） |

产物：`~/.shannon/eval/v2-glm-*/` 三份 aggregate（stable/flaky 分桶 + pass-rate 区间）+ **跨模型对照表**（glm vs minimax-m3 逐题 resolved 矩阵）+ P1d harbor 对标报告。

#### P1d Terminal-Bench 2.x 全量（harbor harness，89 题 × n=1）

1. **adapter 开发**（约 1 天）：按 harbor 自定义 agent adapter 规范实现 `shannon-adapter`——在任务容器内收到指令后调用 `shannon -p <instruction> --output-format json-stream --max-turns <N>`；需解决：shannon 二进制进容器（bind-mount 或镜像注入）、容器内 GLM API 出网、env 注入（`SHANNON_API_KEY` / `SHANNON_PROVIDER` / `SHANNON_MODEL`，沿用 wrapper-glm 的 key 注入规则）。
2. **安装与数据集**：`pip install harbor`；`harbor datasets` 拉取 `terminal-bench@2.1`（不可得则 `@2.0`，报告显著标注版本差异）。
3. **试跑闸门**：先 2-3 题端到端（含 task-native 判分）通过，才放全量。
4. **全量 89 题 × n=1**：输出 resolved rate + cost + tokens；与 GLM 官方 TB2.1=69.2（Claude Code harness）对标，差额记为 scaffolding tax 估计。

产物：harbor job 结果目录 + `docs/` 下 TB2.x 对标报告。**n=3 不在本期预算内**（已批准 envelope 为 $10-50 / +1 天的单轮口径），如需加轮次另行审批。

### P2 失败分析与归因（1-2 天）
方法：
1. **跨模型消融**：共同失败题 → scaffolding 嫌疑；glm-only 失败 → 模型能力；minimax-only 失败 → 反向信号。
2. **官方参照校准**：glm 得分 vs GLM 官方 harness 得分（TB2.1 69.2 / DeepSWE 53.7），估计 scaffolding tax。
3. **归因三分法**（复用 failures/ 管道 + trajectory 人工复核）：
   - a) shannon scaffolding（系统提示/工具实现/权限/上下文压缩/turn 预算/编辑格式）
   - b) 模型能力（glm-5.3-flash 本身）
   - c) 任务/判分 artifact（corpus 漂移、镜像、超时）

产物：`docs/eval-findings-2026-09-glm.md` + 改进 backlog（P0/P1/P2，每项含证据→假设→修复方案→验证方式）。**backlog 提交用户评审（D4 闸），通过前不进入 P3。**

### P3 实施改进（1-2 周，规模以 backlog 为准）
- **评审闸（D4）**：backlog 经用户评审通过后才开工；评审意见直接修订 backlog 条目。
- 只实施 P2 有证据支撑的改进项；走正常 TDD 开发流。
- 每项验证 = **失败题复放转绿** + regression 池无回归。
- 禁止用 L1 20 题调 prompt（held-out 规则）。
- 产物：本分支上的改进 commits + 逐项验证记录。

### P4 复测结题（1 天）
- P3 完成后：失败题集复跑 + regression n=3 复跑 → `v2-glm-post` anchor 报告，量化改进前后 delta。
- 更新 `scripts/eval/README.md` batch 记录与 CHANGELOG。

## 五、预算与风险

预算（以 batch11 实测 59M in/266K out per 50 题为基准，glm flash 价位更低；订阅制下实际约束是限流窗口而非现金）：

| 阶段 | token 预算闸（--budget-tokens） | 备注 |
|---|---|---|
| P0 | 2M | smoke |
| P1a regression n=3 | 30M | 参照 batch-1 |
| P1b SWE50 n=3 | 250M | 3×(59M+开销) |
| P1c TB9 n=3 | 15M | 9 题×3 |
| P1d TB2.x 全量 n=1 | 120M | 对应已批准 $10-50 envelope；n=3 需另行审批 |
| P3 复放 | 按题计 | 单题 ≤ SWE50 单题均值 2 倍 |

| 风险 | 对策 |
|---|---|
| coding-plan 限流（batch-6 前科：76% 拒绝） | 30s pacing + run-batch 429 自愈 + 必要时降并发/暂停续跑 |
| 模型 id 陷阱（batch-5 RCA：错误 id → 48%→4%） | wrapper 硬编码 `glm-5.3-flash`，smoke 验证后才放量 |
| TB corpus 漂移 | P0-3 专项修复 + pin_validation 必须确认后才进 P1c |
| harbor adapter 容器化（二进制注入 / 容器出网 / env） | P1d 试跑 2-3 题先行，端到端通过才放全量；失败则回退 bind-mount 注入方案 |
| TB2.x 版本口径（官方参照为 @2.1） | 数据集版本写入报告 anchor；`@2.1` 不可得时降级 `@2.0` 并显著标注，不与 69.2 直接相减 |
| 改进引入回归 | 每项改动跑 regression 池 + 失败题复放；引用只认 stable 分桶 |
| 订阅额度不足 | 预算闸提前硬停；决策点 D1 可切换按量端点（换 anchor） |

## 六、评审结论（2026-09-05，已批准）

| 决策点 | 结论 |
|---|---|
| **D1 端点** | **zhipu-coding-plan**（与 v1 基线 / regression batch-1 同 anchor，历史数据可比） |
| **D2 范围** | **批准追加 Terminal-Bench 2.x 全量 89 题**：harbor harness + shannon adapter，+1 天 / $10-50，列入 P1d |
| **D3 预算** | **认可 SWE50 n=3 = 250M tokens 预算闸**；其余阶段闸值按 §五 执行 |
| **D4 授权** | **P2 backlog 产出后先审再做**：P3 开工前，backlog 必须提交用户评审通过 |

---

## 附：本轮调研主要外部参考

- SWE-bench（swebench.com；OpenAI 2026-02 弃用 Verified 声明；mini-swe-agent；Epoch AI 镜像加速）
- Terminal-Bench（tbench.ai；harbor framework）
- GLM-5.3 官方基准（z.ai/blog/glm-5.3）；GLM Coding Plan 端点（docs.z.ai/devpack/quick-start）
- Aider Polyglot（aider.chat/docs/leaderboards）；SWE-bench-Live（microsoft/swe-bench-live）；τ²-bench（sierra-research/tau2-bench）
