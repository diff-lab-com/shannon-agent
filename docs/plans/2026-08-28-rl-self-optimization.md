# Shannon RL 自优化方案：用自身评测体系 + trace 系统驱动自分析 / 自修复 / 自优化

- **日期**: 2026-08-28
- **状态**: 方案草案（research + design，未排期）
- **作者**: Claude（AI 研究员 + 系统架构师，代 ed 起草）
- **前置文档**: [autonomous-improvement-loop.md](./autonomous-improvement-loop.md)（确定性 Supervisor 外循环，§10 已定夺）
- **引用纪律**: 每个论断要么带编号引用（§11 文献表，全部有 URL），要么带 Shannon 代码证据（`文件:行`，均为本仓库当前 dev 工作区实查）。

---

## 0. 摘要

Shannon 已经拥有一个"充当 RL 环境所需"的完整基础设施：L0 事件日志忠实记录轨迹（trajectory）与状态，L1 评测套件 + 7 类失败分类器提供可验证奖励（verifiable reward），三外部基准与 `stable_digest` 提供泛化探针与回归闸门，trace CLI 提供回放诊断，Landlock 沙箱与权限体系提供安全边界。**缺的只是把"分析→修复→验证"这一环自动化成闭环的编排层与学习层。**

本方案把"自优化"形式化为一个**分层策略改进问题**，并给出四级务实路线：

- **L1** 确定性失败归因增强（零风险，纯规则）；
- **L2** Reflexion 式自举循环（RL-free：跑评测 → 读 trace/失败归档 → 生成修复假设 → 打补丁 → 回归验证），带停止条件与预算护栏；
- **L3** 从 L0 构造 `(trajectory, outcome)` 数据集，训练轨迹级过程奖励模型（PRM/verifier），用于推理时补丁筛选——不更新策略权重，先拿 verifier 的收益；
- **L4** 真正的 policy 优化（GRPO/RLEF 谱系）——明确给出"何时才值得做"的数据量与算力门槛，以及用 `stable_digest` 做灾难性回归闸门。

核心判断：**Shannon 的最优路径不是直接上 RL 训练，而是先让 L2 自举循环跑起来产生数据飞轮，L3 用 verifier 把飞轮变现，L4 只在数据与回归防护都到位后启动。** 这与文献结论一致：执行反馈的可验证奖励是软件工程 agent 改进的最强信号 [10][11]，但训练级 RL 需要数千级任务环境 [9]，而 Reflexion 式语言反馈在无训练条件下已能拿到大部分低成本收益 [1][3]。

---

## 1. 背景与目标

### 1.1 目标

让 Shannon 具备三类自优化能力，全部建立在本仓已有的评测与 trace 基础设施之上：

1. **自分析**：失败发生后，自动产出"归因报告"（哪一步、哪类失败、证据链是哪些 L0 事件）；
2. **自修复**：针对归因自动生成修复假设（源码补丁 / 提示词修改 / 规则表扩展），在沙箱内验证；
3. **自优化**：多轮迭代下，以 held-out 通过率与成本为复合目标，单调改进系统，且不引入回归。

### 1.2 与既有自主改进循环方案的关系

`docs/plans/autonomous-improvement-loop.md` 已定夺一条**确定性外循环**：自动编译 → 任务矩阵 → 三通道信号采集 → headless LLM 分析修复 → 质量门 → 合入 → 迭代，其 Supervisor 是确定性脚本、LLM 只出现在分析修复一环。本文档不重复该方案，而是回答它的**下一层问题**：

- 外循环里"LLM 分析修复"这一环，**喂给它什么信号**才能最大化修复成功率？（§5 L1/L2 的上下文构造）
- 迭代产生的轨迹数据如何**沉淀为可训练资产**（verifier / PRM），而不是用完即弃？（§5 L3）
- 什么时候才值得从"提示词级自举"升级到"权重级 RL"，门槛是什么？（§5 L4）
- 自改写自身（尤其自改评测器）的**自我欺骗风险**如何系统性封堵？（§6）

一句话定位：**autonomous-improvement-loop 是编排骨架（外循环），本文是它的学习内核（内信号）与升级路线图（RL 何时入场）。**

---

## 2. 理论框架：问题形式化

### 2.1 记号与对象

| 符号 | 含义 | Shannon 对应物 |
|---|---|---|
| `W ∈ Σ` | 仓库快照（源码树状态） | git worktree / 仓库 checkout |
| `φ = (P, D, Π, R)` | 指令集：系统提示段 `P`、实验指令 `D`、权限 profile `Π`、失败规则表 `R` | 系统提示、`EvalOptions::instruction_directive`（`crates/shannon-core/src/testing/eval_runner.rs:936-941`）、`.shannon/profiles/*.toml`、`failure_rules.toml` |
| `T = T_dev ∪ T_held` | 任务集，严格不相交 | `tests/eval/tasks/*.toml`（20 题，实查）+ 需新建的 held-out 切分（§6.3） |
| `T_reg`, `T_ext` | 回归基准与外部基准 | `tests/eval/benchmarks/regression/reg_01..10.toml`；TerminalBench / SWE-bench-Verified 钉选集（`crates/shannon-core/src/testing/eval_benchmarks.rs:1-72`） |
| `Run(τ; W, φ) → σ` | 执行算子：在仓库快照上对任务 τ 跑一次引擎 | `shannon --prompt … --output-format json-stream` 子进程（管线定义 `eval_runner.rs:9-21`，spawn 调用 `:1267-1279`），轨迹 σ 即该任务的 L0 `events.jsonl` |
| `Verify_τ(σ, W′) → {pass} ∪ Fail(c)` | 判定算子：verify 规则/脚本 + 限额分类 | 11 种 `ValidationRule`（`eval_runner.rs:5`）∪ 7 类失败分类（`crates/shannon-core/src/testing/failure_rules.toml:38-178`） |
| `m(σ) → TaskMetrics` | 度量算子 | tokens/cost/turns/tool_calls/loops/invalid_calls/permission_blocks（`crates/shannon-core/src/testing/eval_metrics.rs:147-175`） |

**关键结构事实**：`Run` 是可精确重放的——L0 日志按事件类型 `user/message, turn/start, tool/call, tool/result, request/header, turn/end` 追加记录（`crates/shannon-types/src/session_event.rs:50-148`），`request/header` 逐字保存 `wire_body` 使每个请求可字节级重建（`crates/shannon-core/src/session_log/tee.rs:13-16`），`trace show/replay/diff/export` 是其只读人类接口且渲染为事件体的纯函数（`crates/shannon-cli/src/trace.rs:1-14`）。**这意味着 Shannon 拥有文献中训练环境最稀缺的性质：环境的每次交互都是低成本可重放、可归因的** [9]。

### 2.2 MDP 形式化

把"一次自优化迭代"定义为一个 episode：

- **状态** `s_t = (W_t, φ_t, M_t, H_t)`：仓库快照、当前指令集、反思记忆 `M_t`（失败归档 + 历史假设-结果账本）、历史评测信号摘要 `H_t`。
- **动作空间** `a_t ∈ A`，按风险递增分四层：
  - `A_mem`：写入反思记忆 / 归因报告（无系统影响）；
  - `A_rules`：扩展 `failure_rules.toml`（L1，确定性）；
  - `A_prompt`：修改 `instruction_directive` / profile / 提示段；
  - `A_code`：对 `crates/**` 的源码补丁（需重编译）。
- **转移**：`s_{t+1} = Apply(a_t) → Rebuild? → Eval(n=3)`。给定补丁，评测结果是确定的；随机性只来自 LLM 采样，由 n=3 纪律（`eval_benchmarks.rs:836-858`，`N_RUNS_REQUIRED = 3`）以区间 `[min, max]` 显式呈现而非平均掩盖。
- **奖励**：

```text
r_t =  ΔResolved@n(T_held)              # 主目标：held-out 通过率增量
     − λ_c · ΔCost                      # 成本约束（TaskMetrics.cost_usd，null 即诚实未知，eval_metrics.rs:156-158）
     − λ_r · max(0, −ΔResolved(T_reg))  # 回归惩罚（reg_01..10 + cargo 测试门）
     − λ_h · HarnessTamper(a_t)         # 篡改评测器 → −∞（硬闸门，§6.4）
```

- **策略** `π(a | s)`：L2 阶段 `π` 就是"被结构化提示的 LLM"（无权重更新）；L4 阶段才是 `π_θ`（权重）。
- **目标**：`max Σ_t r_t s.t. 预算 B`（token / 时间 / 美元三层闸门，沿用 autonomous-improvement-loop §10 的预算决策）。

### 2.3 为什么 L2 不需要 RL 算法，而 L4 需要

L2 的每步动作都可以用**精确模拟**（真实评测套件）估计其真实奖励——这等价于"有完美世界模型的贪心爬山"，RL 算法（credit assignment、exploration、policy gradient）的价值来自**奖励评估太贵而必须采样近似**的场景。Shannon 一次全量评测是分钟级、美元级，20 题规模下贪心门控（每步过闸门）是正确选择；当任务集扩展到数百题、或动作空间变成连续的提示词/权重空间、或奖励评估只能抽样时，才进入 RL 算法的适用域。这一判断与 RLEP 的两阶段设计同构：先用已验证轨迹引导（免训练），再进入 RL 训练 [11]。

### 2.4 三条谱系的取舍

| 谱系 | 更新载体 | 奖励信号 | Shannon 落点 | 主要风险 |
|---|---|---|---|---|
| **RL-free**（Reflexion [1] / Self-Refine [2] / Self-Debug [3]） | 语言记忆 / 上下文 | 环境反馈转自然语言反思 | **L2**：失败归档 + trace 反思 | 自我欺骗（judge 偏差 [16]）、上下文长度上限 |
| **RLHF/RLAIF** | 权重 | 人类/AI 偏好评级 | **不推荐**：Shannon 无偏好数据管线；轨迹有客观结果，偏好建模多余 | 标注成本、奖励模型被绕过 |
| **RLEF/RLVR**（execution-feedback：CodeRL [7] / RLTF [8] / SWE-RL [10] / RLEP [11] / Kimi-Researcher [19]） | 权重 | 执行结果 / 测试通过（可验证奖励） | **L3（verifier 先行）→ L4（策略）** | 数据量门槛、灾难性遗忘、reward hacking [24] |

选择依据：软件工程任务的奖励天然可验证（编译/测试/verify 规则），是 RLVR 的理想域 [10][19]；但 RLVR 训练需要大规模可执行环境（SWE-Gym 用 2,438 个真实任务 [9]），Shannon 自有 20+10 题远不够，**训练决策必须被数据量门控**（§5.4 L4）。

### 2.5 策略载体的成本-能力阶梯

自优化的"策略"可以落在四个载体上，改进成本与风险递增：

```text
A_rules（规则表） < A_prompt（提示词/指令/profile） < A_mem（检索记忆） < A_code（源码） ≪ A_weight（权重, L4）
```

路线设计原则：**能用低层载体解决就不升级**。Agentless 的教训直接适用：确定性三阶段流水线（定位→修复→验证）以极低成本达到与复杂 agent 相当的竞争力 [12]——L1/L2 的修复器应当先做成"结构化流水线 + 少量 LLM 调用"，而不是"放开手脚的自治 agent"。

---

## 3. 相关工作调研

按主题分组；**可迁移性**评级针对 Shannon 场景（自托管评测体系 + 自修自身 + 中小规模任务集）。

### 3.1 RL-free 自我改进（L2 的直接理论来源）

1. **Reflexion** [arXiv:2303.11366](https://arxiv.org/abs/2303.11366)（NeurIPS 2023）— 不更新权重，把环境反馈转成语言"反思"存入情景记忆，后续尝试以记忆为条件。**可迁移性：高** — Shannon 的失败归档（`eval/failures/<date>/<task>/`，`eval_runner.rs:1129-1172`）就是现成的情景记忆存储，L2 循环几乎是它的系统级实现。
2. **Self-Refine** [arXiv:2303.17651](https://arxiv.org/abs/2303.17651)（NeurIPS 2023）— 同一 LLM 自反馈-自精炼迭代，无需训练。**可迁移性：中** — 迭代骨架可借，但"无外部执行反馈的自反馈"在代码域弱：Shannon 的反馈必须来自评测执行而非模型自评（Self-Debug 的消融显示执行反馈是主要增益来源 [3]）。
3. **Self-Debug** [arXiv:2304.05128](https://arxiv.org/abs/2304.05128)（ICLR 2024）— 模型执行代码 + 解释代码来调试自身，最难任务上提升约 9%。**可迁移性：高** — "让修复器读自己的 trace 再解释"正是 L2 的核心动作；Shannon 的 `trace replay`（时间压缩重放）就是为这一步准备的反馈通道。
4. **Agent-R** [arXiv:2501.11425](https://arxiv.org/abs/2501.11425) — MCTS + 模型引导反思，把修正步骤拼接进错误轨迹，迭代自训练。**可迁移性：中** — 依赖轨迹语料与训练轮次，对应 Shannon 的 L3/L4 阶段；其"修正轨迹拼接"思路可用于构造 L3 的正样本对。
5. **Draft, Sketch, and Prove**（autoformalization）[arXiv:2210.12283](https://arxiv.org/abs/2210.12283)（ICLR 2023）— 非形式草稿引导形式化证明器。**可迁移性：低** — 域相距远（定理证明），但"非形式假设 → 形式检验"的两段式与 L2 的"修复假设 → 评测验证"同构，提示词设计可借鉴。

### 3.2 SWE agent 框架与环境（接口与工程形态参考）

6. **SWE-agent** [arXiv:2405.15793](https://arxiv.org/abs/2405.15793)（NeurIPS 2024）— 提出 Agent-Computer Interface（ACI）：为 agent 设计的工具接口显著影响其编辑/导航能力。**可迁移性：中** — Shannon 修的是自己而非外部仓库，但 ACI 结论反哺自身工具面：L1 归因报告的格式就是"给修复器的 ACI"，应按 token 效率与可定位性设计。
7. **OpenHands (OpenDevin)** [arXiv:2407.16741](https://arxiv.org/abs/2407.16741) — 事件流架构 + 沙箱运行时的开源 agent 平台。**可迁移性：中** — 架构印证 Shannon 的 L0 事件流 + Landlock 沙箱选型；其评测集成方式（SWE-bench 工作流）是三外部基准 adapter 的同行参照。
8. **Agentless** [arXiv:2407.01489](https://arxiv.org/abs/2407.01489) — 定位→修复→验证三阶段确定性流水线，不让 LLM 自主决策，成本仅为 agent 方案零头。**可迁移性：高** — 是 L1/L2 修复器的直接设计模板：失败分类器已给出"定位"信号（哪类失败、哪个工具环节），修复器应消费该信号而非自己重新探索仓库。
9. **SWE-Search** [arXiv:2410.20285](https://arxiv.org/abs/2410.20285)（ICLR 2025）— MCTS + 价值 agent 在轨迹空间搜索，推理时扩展替代训练。**可迁移性：中** — 证明"推理时搜索"是训练的前置替代品，但 token 成本高；适合作为 L3 verifier 就位后的推理策略（verifier 当价值函数剪枝），不适合作为第一步。

### 3.3 RLEF/RLVR 训练（L3/L4 的方法来源）

10. **CodeRL** [arXiv:2207.01780](https://arxiv.org/abs/2207.01780)（NeurIPS 2022）— 用单元测试结果训练 critic 模型，推理时 actor-critic 筛选候选程序。**可迁移性：高（L3）** — Shannon 的对应物：从 L0 轨迹 + 7 类失败分类构造 critic 训练集；推理时生成 n 个候选补丁、verifier 择优——把 n=3 的"重试纪律"升级为"生成-筛选"。
11. **RLTF** [arXiv:2307.04349](https://arxiv.org/abs/2307.04349)（TMLR）— 多粒度单元测试反馈的在线 RL 框架，训练时实时生成数据。**可迁移性：中** — Shannon 的 verify 规则粒度比单元测试粗（文件级 diff_regex），但"多粒度反馈塑形"适用于 L3 的步骤级标注（§5.3）。
12. **SWE-Gym** [arXiv:2412.21139](https://arxiv.org/abs/2412.21139) — 首个真实软件工程 agent 训练环境（2,438 任务 + 可执行环境 + 单测验证），并训练轨迹 verifier 提升解析率。**可迁移性：高（整体路线模板）** — 其"环境 + verifier + 筛选"三件套正是 Shannon L3 的蓝图；差异仅在规模：Shannon 先用自有 20+10 题 + 外部钉选集起步。
13. **SWE-RL** [arXiv:2502.18449](https://arxiv.org/abs/2502.18449)（Meta，NeurIPS 2025）— 在开源软件演化（真实 PR）数据上 GRPO，规则奖励为生成补丁与真实补丁的序列相似度。**可迁移性：中（L4 奖励设计）** — 训练基础设施 Shannon 不具备，但其"规则奖励不用神经奖励模型"的原则直接写入 L4 设计：奖励只来自评测执行，不用 LLM 打分。
14. **RLEP** [arXiv:2507.07451](https://arxiv.org/abs/2507.07451) — 两阶段：先收集已验证成功轨迹，再在 RL 中回放，加速收敛并提升峰值。**可迁移性：中** — L2 的"已接受补丁账本"就是它的第一阶段；未来 L4 启动时该账本即回放缓冲区，无需改数据格式。
15. **DeepSeek-R1** [arXiv:2501.12948](https://arxiv.org/abs/2501.12948) — 纯 RLVR（GRPO）诱发自演化推理（反思/验证行为涌现，"aha moment"）。**可迁移性：中** — 基座训练域，Shannon 不训基座；可迁移的是两点：①奖励设计极简（正确性 + 格式），②其蒸馏路线说明小模型可以从强轨迹中学到修复行为——L3 的 verifier/修复器可用开源小模型 LoRA。
16. **Kimi K2** [arXiv:2507.20534](https://arxiv.org/abs/2507.20534)（Moonshot）— 合成工具调用轨迹 + 联合 RL（VRPO），证明"工具使用可以被 RL 训练而非仅靠提示"。**可迁移性：中** — 工业级规模的佐证；对 Shannon 的具体启示是 L3 数据构造中的"轨迹合成与筛选"环节。
17. **Kimi-Researcher** [moonshotai.github.io/Kimi-Researcher](https://moonshotai.github.io/Kimi-Researcher/)（Moonshot，2025-06）— 端到端 agentic RL，"零结构"设计（无硬编码流水线），HLE pass@1 8.6%→26.9%。**可迁移性：中** — 反向启示：在其数据量级下端到端 RL 能吃掉手工流水线收益，但 Shannon 当前规模恰恰相反——**应保留结构化流水线，把 RL 留给 L4**。
18. **Llama-Nemotron** [arXiv:2505.00949](https://arxiv.org/abs/2505.00949)（NVIDIA）+ NeMo-RL 开放工具链 — 开放 RL 后训练管线与推理开关（reasoning on/off）。**可迁移性：中（工具链）** — 若 L4 启动，NeMo-RL / verl 类开源栈是现成工程底座；其"高信号后训练数据集"的组织方式可参考 L3 数据 schema。

### 3.4 奖励建模、评测可证伪性与安全（贯穿性约束）

19. **Let's Verify Step by Step** [arXiv:2305.20050](https://arxiv.org/abs/2305.20050)（OpenAI）— 过程监督（PRM）在难题上一致优于结果监督（ORM）。**可迁移性：高（L3）** — Shannon 的 L0 轨迹天然切成过程单元（tool/call 步），从失败归档可自动导出步骤级标注（§5.3），这是把 20 题的稀疏结果信号放大成稠密过程信号的正确路径。
20. **From Generation to Judgment (LLM-as-a-judge 综述)** [arXiv:2411.16594](https://arxiv.org/abs/2411.16594) + **A Survey on LLM-as-a-Judge** [arXiv:2411.15594](https://arxiv.org/abs/2411.15594) — 系统目录位置偏差、权威偏差、自我偏好、误信息容忍等判定偏差及缓解。**可迁移性：高（可证伪性设计）** — 结论写入 §6 硬规则：**LLM 判定永远只做"提出"，执行判定（verify 规则/编译/测试）才有资格计为奖励**；LLM 自评分数不得进入 `r_t`。
21. **SWE-Bench+** [arXiv:2410.06992](https://arxiv.org/abs/2410.06992) — 实证 SWE-bench 的题面泄答案（检出准确率 86%）与弱测试问题，确认记忆化风险。**可迁移性：高（评测纪律）** — 直接支撑 §6.3：自优化系统对训练/评测泄漏必须比常规评测更偏执，因为被优化的对象有持续动机去拟合评测集。
22. **Absolute Zero** [arXiv:2505.03335](https://arxiv.org/abs/2505.03335) — 零数据自博弈（自 propose 任务 + 执行器验证），代码域跨域泛化。**可迁移性：低** — 思想诱人但与本方案的评测集隔离原则冲突（自 propose 任务 = 自改评测分布）；仅在 held-out 完全外置（外部基准）的前提下可作为远期研究方向。
23. **Concrete Problems in AI Safety** [arXiv:1606.06565](https://arxiv.org/abs/1606.06565) — reward hacking / 负面副作用 / 可扩展监督五问题分类。**可迁移性：高（§6 的威胁模型骨架）** — "清洁机器人把污迹盖住"在本方案的具体化身是"修复器改宽 `diff_matches` 正则"——奖励函数与修复器同仓，必须工程隔离。
24. **R2E-Gym** [arXiv:2504.07164](https://arxiv.org/abs/2504.07164) — 程序化生成可执行环境 + 混合 verifier 训练 SWE agent。**可迁移性：低-中** — 环境生成路线对 Shannon 的启示是负面的：生成环境的判据可信度低于真实缺陷复现（Shannon 的 reg_* 套件源自真实 CHANGELOG 缺陷，`eval_benchmarks.rs:11-13`），不追求扩题速度而牺牲判据真实性。

---

## 4. Shannon 已有基础设施 → RL 组件映射

方案成立的前提是：RL 管线的每个组件都已有落点，不需要从零造环境。逐项映射（全部为实查代码证据）：

| RL 组件 | Shannon 已有物 | 证据 | 本方案用法 |
|---|---|---|---|
| **Environment / State** | L0 事件日志：`events.jsonl` 追加型 JSONL，`flock` 独占写 + 崩溃尾部恢复；事件类型覆盖 `user/message, turn/start, tool/call（原始参数）, tool/result, request/header（wire_body 逐字）, turn/end（usage/cost）` | `crates/shannon-core/src/session_log/mod.rs:1-12`；`crates/shannon-types/src/session_event.rs:50-148`；`session_log/tee.rs:13-16` | 轨迹 σ 与状态转移的权威记录；`wire_body` 使"策略看到什么"可字节级复现，是 L3 步骤级标注的原料 |
| **Reward oracle** | L1 评测套件：20 题 TOML（setup/verify.rules/expectations/limits/dry_run），11 种 `ValidationRule`，真实模式以隔离子 `SHANNON_HOME` 捕获每题真实 L0 日志，限额三分类与断言失败区分 | `eval_runner.rs:1-77`；`tests/eval/tasks/`（20 文件实查） | `Verify_τ` 的实现；`cost_usd`/tokens 直接进入奖励的成本项 |
| **自动标注器** | 7 类失败分类的纯规则表（事件形状谓词，首条全中即胜出，证据链随行），信号词表含 `loops/invalid_calls/tool_error_unrecovered/compaction_events/permission_blocks` 等 13 种 | `crates/shannon-core/src/testing/failure_rules.toml:1-38, 38-178`；`eval_metrics.rs:147-175` | 失败类别 = 廉价的过程标注；`A_rules` 动作空间的对象；归因报告的骨架 |
| **泛化探针** | 三外部基准 adapter：TerminalBench（原生 run-tests.sh 判据）、SWE-bench-Verified 50 题钉选、自建 regression 套件（真实 CHANGELOG 缺陷复现）；钉选 ID 的 SHA-256 指纹入报告；缺环境跳过而非伪绿 | `eval_benchmarks.rs:1-72`；`tests/eval/benchmarks/regression/reg_01..10.toml` | held-out 的外置部分；防止"只对 20 题过拟合"的泛化量尺 |
| **回归闸门** | `BenchReport::stable_digest`：丢弃时间戳/时长/路径的挥发性投影后 SHA-256，`bench_runner diff` 输出 STABLE/UNSTABLE | `eval_benchmarks.rs:938-968, 1075-1101` | L2 每步接受条件；L4 的灾难性回归哨兵（§5.4） |
| **回放诊断通道** | `trace show/replay/diff/export`，渲染为事件体纯函数，回放输出与进程内流字节一致 | `crates/shannon-cli/src/trace.rs:1-14` | 修复器的"眼睛"：把 σ 变成可读反馈（A1/A2 实验的处理组信号，§7.1）；OTLP span 树（`crates/shannon-core/src/telemetry.rs`）补齐时序视图 |
| **A/B 动作通道** | `EvalOptions::instruction_directive`：附加到每题 prompt 的实验指令，报告顶层盖 `directive` 章，eval-diff/dashboard 区分武器 | `eval_runner.rs:936-941, 718-720, 780` | `A_prompt` 动作的载体与消融实验的实现机制 |
| **n=3 方差纪律** | `N_RUNS_REQUIRED = 3`，逐 case 状态直方图、通过率区间 `[min, max]`、引用块强制带 n 与日期 | `eval_benchmarks.rs:836-858, 66` | 统计显著性方法的基础（§7.2）；防 flaky 补丁被偶然通过洗白 |
| **沙箱边界** | Landlock：fork-exec 间隙给每个子进程装 ruleset，装不上即拒绝启动（fail-closed），拒绝带 `SANDBOX_DENIED_PREFIX` 可被分类器识别；非 Linux 显式 `Unsupported` | `crates/shannon-tools/src/sandbox/landlock_backend.rs:1-20` | 修复器与其产物的一切执行都在边界内；`tests/eval/**` 不进写授权表（§6.1） |
| **失败归档（情景记忆）** | 失败任务的 L0 日志副本 + `classification.json`（status/失败类/证据/指标/工作区路径）落 `eval/failures/<yyyymmdd>/<task>/` | `eval_runner.rs:1129-1172` | Reflexion 式记忆存储；MVP 循环的输入（§7.4） |
| **编排骨架** | 确定性 Supervisor 方案（已定夺）：编译→评测→信号→headless 修复→质量门→合入，token 三层闸门 | `docs/plans/autonomous-improvement-loop.md` §0/§10 | L2 循环的宿主：本文的伪代码是其中"分析修复"环节的展开 |
| **修复器执行面** | headless 契约：`--prompt/--output-format json-stream/--allowed-tools/--max-turns/--schema`，退出码 0-6 全分类 | `crates/shannon-cli/src/main.rs:61-80, 1600-1601`；`eval_runner.rs:50-65` | 修复器以同款 headless 接口运行；`--schema` 保证修复假设是可解析 JSON |
| **权限与升级路径** | 9 档 ApprovalMode + `PermissionClassifier`（2928 行）+ `LlmPermissionClassifier`；32 类 hook 事件；plugin registry；worktree 隔离（`context.working_directory`） | 项目 `CLAUDE.md`（Tier-2 差异化清单） | 修复器的写权限收窄；`A_code` 动作强制走 worktree + PR 门（§6.2） |

**结论**：环境（E）、奖励（R）、标注（L）、记忆（M）、安全边界（S）五件套全部在仓。缺的只有：①编排层把上述组件串成循环（autonomous-improvement-loop 已规划）；②held-out 切分与 harness 防篡改机制（§6.3/6.4，需新建）；③L3 的数据导出器与训练脚本（需新建）。

---

## 5. 分层路线

### 5.1 L1 — 失败归因的确定性增强（纯规则，零模型风险）

**目标**：把"7 类失败 + 13 种信号"升级为修复器可直接消费的**归因报告**，不引入任何 LLM 判定。

内容（全部可移植自现有组件）：

1. **规则表扩展**：向 `failure_rules.toml` 增补信号与类目——例如 `cache_collapse`（`cache_read_tokens` 骤降 + tokens_in 骤增，信号已有 `eval_metrics.rs:152-155`）、`compaction_stranded`（`compaction_events > 0` 且最后一个 `turn/end` 前无后续 `tool/call`）、`retry_storm`（同签名调用重试 ≥ k 次）。纯 TOML 修改，`FailureRules::embedded` 默认表 + `--rules` 覆盖机制已就绪（`failure_rules.toml:7-9`）。
2. **类目→修复提示映射**：新增 `remediation_hints.toml`（class → 修复器应优先检查的代码区域/提示段落），作为 `A_prompt` 的初始检索空间。
3. **归因报告生成器**：`classification.json`（已有）+ `trace diff`（失败轨迹 vs 最近一次通过轨迹）+ 失败点前 k 个 L0 事件的紧凑渲染。产出固定 schema 的 `attribution.md`，落与失败归档同目录——**这份报告就是修复器的 ACI**（按 SWE-agent 的教训：接口设计直接决定 agent 能力上限 [6]）。
4. **验收**：对既有失败归档样本回放，人工评估归因命中率的提升；全部确定性可测试。

### 5.2 L2 — Reflexion 式自举循环（RL-free，本方案的核心交付）

**目标**：跑评测 → 读 trace/失败归档 → 生成修复假设 → 打补丁 → 回归验证的多轮自举，全部动作过闸门。

循环伪代码（宿主为确定性 Supervisor，修复器是 headless Shannon）：

```text
function bootstrap_loop(T_dev, T_reg, T_held, B):        # B = token/时间/美元预算
    φ ← current_instructions()
    M ← load_failure_archive(recent_days=30)              # 情景记忆：归档 + 历史账本
    baseline ← eval(T_dev, φ, n=3); baseline_held ← eval(T_held, φ, n=3)
    ledger ← []                                           # 假设-结果账本（JSONL，本身走 L0 风格记录）
    while B.remaining() > 0:
        ctx ← build_context(M, ledger, k=5)               # 归因报告 + 相关源码 + 历史 reflection
        H ← fixer.propose_hypotheses(ctx, schema=PatchProposal)   # headless + --schema
        for h in H:
            Δ ← h.materialize(worktree)                   # A_rules | A_prompt | A_code，一律先落 worktree
            if not static_gates(Δ):                       # fmt / clippy -D warnings / 编译 / dry-run 自检绿
                ledger.reject(h, "static"); revert(Δ); continue
            r_dev ← eval(T_dev, φ∘Δ, n=3)                 # 区间 [min,max] 非退化才有效
            r_reg ← eval(T_reg, φ∘Δ, n=1)                 # 回归套件 + cargo test 关键面
            if improves(r_dev, baseline) and no_regression(r_reg) and cost_within(r_dev, baseline):
                r_held ← eval(T_held, φ∘Δ, n=3)           # 延迟评估：候选通过才动 held-out（§6.3）
                if not worse(r_held, baseline_held):
                    φ ← φ∘Δ; baseline ← r_dev; baseline_held ← r_held
                    ledger.accept(h, r_dev, r_held); M.append(reflection(h, r_dev))
                else:
                    ledger.reject(h, "held-out 不支持"); revert(Δ); M.append(negative_reflection(h))
            else:
                ledger.reject(h, "dev 无改进或有回归"); revert(Δ); M.append(negative_reflection(h))
        if no_accept_in_last(K=3) rounds: break           # 停止条件②
    return φ, ledger, morning_report(ledger)
```

**停止条件与预算护栏**（缺一不可）：

| 护栏 | 触发 | 动作 |
|---|---|---|
| 预算 B | token / 墙钟 / 美元任一超限（autonomous-improvement-loop §10 的三层闸门） | 立即停，产出晨报 |
| 无改进窗口 | 连续 K=3 轮无 accept | 停（防提示词空间里的随机游走） |
| 回归熔断 | 任一 reg_* 任务翻转 fail，或 `stable_digest` UNSTABLE 且无解释 | 立即停并回滚本 worktree（`eval_benchmarks.rs:1093-1101`） |
| 单假设上限 | 单个 Δ 的 dev 评测成本 > 当前基线均值的 c 倍 | 拒绝（防"用指数成本换一题"） |
| 人工闸门 | `A_code` 类 accept | 只进 PR 队列，绝不自动合入主线（§6.2） |

**上下文构造（`build_context`）的消融维度**——这正是文献结论的直接应用：

- 执行反馈是主要增益来源（Self-Debug [3]）：trace replay / 失败点事件 = 必选；
- 反思记忆跨轮累积是 Reflexion 的核心（[1]）：ledger + 历史负反思 = 必选；
- 无环境根据的自反馈是已知弱项（Self-Refine 的局限 [2]）：修复器不得在无 trace 证据时凭空改代码——提示词层面强制"每个假设必须引用至少一条 L0 证据事件"。

**验收标准（MVP 级）**：一晚循环内 ≥1 个假设过全部闸门被接受，且 held-out 无回归；全程账本可审计。

### 5.3 L3 — 轨迹过程奖励模型 / verifier（数据飞轮变现）

**目标**：不更新策略，训练一个"给轨迹/补丁打分"的小模型，用于推理时筛选——CodeRL 的 critic [7] 与 SWE-Gym 的 verifier [9] 是同型物。

**数据构造（全部可从 L0 导出）**：

1. **样本单元**：`(τ, σ, outcome)`，σ = 一个任务的完整 L0 事件序列，outcome ∈ {pass, fail:class}。`extract_from_events_log` 已是现成抽取器（`eval_runner.rs:50-52`）。
2. **过程级标注**（PRM 路线，依据"过程监督优于结果监督" [15]）：把 σ 切成步序列（tool/call 粒度），每步标签由两种廉价途径导出（`extract_from_events_log` 抽取器现成可用，`crates/shannon-core/src/testing/eval_metrics.rs:357`）：
   - **结果条件化（Monte-Carlo）**：同任务多次重复（n=3 已强制）中，该步之后仍通过的比例 → 步骤好值；
   - **分类器条件化**：失败类的证据链事件（`failure_rules.toml` 的 signal 证据）标记负步骤。
3. **正负对合成**：Agent-R 式"修正轨迹拼接" [4]——同一任务的失败轨迹 + 其后的成功轨迹（跨日期归档可配对）天然构成偏好对，可直接训 preference/DPO 式 verifier。
4. **数据量诚实预估**：当前吞吐 ≈ (20 题 + 10 reg + 外部钉选) × n=3 × 每晚一轮 ≈ 10² 量级轨迹/晚；小模型 verifier（7-8B LoRA）经验上需 10³–10⁴ 轨迹起步、10⁴+ 才稳。**结论：3–12 个月的数据飞轮期；L3 的工程（导出器 + schema）现在就做，训练决策延后到数据量到位。** 不虚构"立即能训"。

**推理时变现（无需等训练）**：先上"确定性 verifier"——静态闸门 + reg 套件 + n=3 区间，就是 verifier 的退化形式；训练出的模型 verifier 就位后，替换 §5.2 伪代码中 `improves()` 的判定为"verifier 预筛 → 评测确认"，把每轮候选数从 k=5 提到 k=20 而预算不涨。

**防泄漏红线**：verifier 的训练集与 `T_held`/外部钉选集严格不相交；held-out 的失败归档不进训练集（§6.3）。

### 5.4 L4 — 真正的 policy 优化（何时值得、代价、防遗忘）

**何时值得**（三条同时满足才启动）：

1. **数据**：≥10⁴ 条已验证轨迹（含 ≥10³ 修正对），且 L3 verifier 在 held-out 上 AUC/选择增益显著（verifier 择优 > 随机择优，n=3 区间不重叠）；
2. **动作空间**：`A_prompt/A_rules` 的改进空间实测枯竭（连续多轮 L2 无 accept），且归因显示剩余失败类集中在策略行为（如 `model_ceiling`、`context_loss` 类持续占比 > 阈值）而非环境/工具缺陷；
3. **回报**：失败类别构成证明"行为改了评测就能过"，而不是"需要更大的基座模型"。

**代价预估**（诚实区间，按开源栈现状）：7–8B 级修复器 LoRA/SFT ≈ 数百 GPU 时；agentic GRPO（多环境并行 rollout + 每步评测作为奖励，RLEF 谱系 [7][10][11][19]）≈ 10³–10⁴ GPU 时量级。**以 Shannon 的自有任务规模（20+10 题），L4 不可行；可行前提是先把 T 扩到数百题**（外部基准 adapter 已打通接入面，`eval_benchmarks.rs:1-72`）或接入社区训练集（SWE-Gym 数据资产 [9]）。规则奖励只用执行结果（SWE-RL 原则 [10]），不用神经奖励模型打分。

**灾难性遗忘与回归防护**：

- **闸门**：任何策略快照上线前必须通过——① `stable_digest` 结构比对（`eval_benchmarks.rs:938-968`：时间戳/路径挥发项剥离后应 STABLE）；② reg_01..10 全绿；③ 外部钉选集通过率区间不降（n=3）；④ cargo 全量测试。四道任一失败即回滚到上一快照。
- **快照管理**：策略版本与 `app_version` + workload 指纹锚定进报告（既有溯源纪律，`eval_benchmarks.rs:70-72`），保证"哪个模型产生的哪条轨迹"永可追溯。
- **冻结核心**：评测 harness 本身永不参与训练/微调（§6.4）。

---

## 6. 安全与可证伪性

### 6.1 执行边界

- 修复器及其产物的一切执行走 Landlock：fork-exec 间隙装 ruleset、装不上即拒绝启动、非 Linux 显式失败（`landlock_backend.rs:1-20`）。授权表原则：**修复器的写授权仅覆盖其 worktree；`tests/eval/**`、`crates/shannon-core/src/testing/**`、`failure_rules.toml` 的仓库正本不在写授权内**——修改这些文件只能通过"显式提交 diff 评审"的人类闸门。
- 退出码契约（0-6 全分类，`main.rs:61-80`）让 Supervisor 能把 `permission denied`/`sandbox_denied` 与普通失败区分，越界尝试自动升级为账本中的红色事件并计入 `permission_blocks`（`eval_metrics.rs:171-172`）。

### 6.2 补丁评审门

| 动作层 | 落地方式 | 自动化上限 |
|---|---|---|
| `A_rules` / `A_mem` | 直接进循环 | 全自动（规则表有 schema 校验 + dry-run 回归） |
| `A_prompt` | 循环内自动，accept 后生成 diff 摘要 | 全自动，晨报必列 |
| `A_code` | worktree + PR + CI 全门（`just dev`：check+clippy+test） | 自动建 PR，**合入永远人工** |
| harness / 评测文件 | 仅人工 | 循环内只读 |

### 6.3 评测集隔离与防过拟合（20 题的宿命问题）

- **切分**：20 题按 tier 分层抽样出 `T_dev(15) / T_held(5)`，held-out 轮换（每两周与 dev 互换一次，防止 dev 也被耗尽信息量）；外部钉选集（TerminalBench / SWE-bench-Verified 50 + reg_10）充当第二层 held-out，其 ID 指纹已随报告流转（`eval_benchmarks.rs:17-20, 287-288`）。
- **信息流规则**：`T_held` 与外部基准的**失败归档不进入 `build_context`，不进入 L3 训练集**；held-out 每 episode 只评两次（基线一次、候选通过后一次——即伪代码中的"延迟评估"），既防选择偏差又防信息侧漏。
- **泄漏警觉**：SWE-Bench+ 的实证（题面泄答案检出率 86%、记忆化显著 [21]）说明评测集会被一切优化过程被动拟合；自优化系统里被优化对象与评测集长期共存，泄漏风险是**主动的**——因此 held-out 之外再加"第三层探针"：季度性从真实 CHANGELOG 缺陷新增 reg_* 题（与既有 reg 套件同源同构，`eval_benchmarks.rs:11-13`），新题在加入前对当期系统是盲的。

### 6.4 防 reward hacking：威胁模型与工程对策

依据 [24] 的分类逐条落地：

| # | 威胁 | 具体化身 | 对策 |
|---|---|---|---|
| H1 | 篡改奖励函数 | 修复器改宽 `diff_matches` 的 `expected_diff_regex`、给 verify 规则放水、编辑 `failure_rules.toml` 正本 | harness 与评测文件在 Landlock 写授权外（§6.1）；`A_rules` 只允许写**副本 + schema 校验 + dry-run 全绿 + 人工 diff 评审**；harness 自检：每晚循环前置 dry-run 套件必须全绿（`eval_runner.rs` 的 dry-run 管线本身就是端到端自检，诚实性声明见 `:67-77`），红了说明 harness 被动过 |
| H2 | 特化解 | 补丁里 grep 任务 id / 提示词里背题 | 静态闸门加 lint：禁止 `A_code` 补丁引用 `tests/eval` 路径与任务 id 字面量；held-out 延迟评估天然惩罚特化（dev 过 held-out 不过 → 拒） |
| H3 | 钻 flaky 空子 | 靠偶然通过洗白坏补丁 | n=3 区间纪律：候选 accept 要求 `[min,max]` 不含基线区间（`eval_benchmarks.rs:836-858` 的呈现方式直接可判） |
| H4 | 判定模型被讨好 | LLM judge 给自己的补丁打高分 | 硬规则：**LLM 判定不进入 `r_t`**。judge（若引入）只做"提出怀疑点"，一切计数判定来自执行（verify 规则/编译/测试）——LLM-as-judge 的位置/自我偏好偏差目录 [20][17] 是这条红线的文献依据 |
| H5 | 自改写自身评测器（元层级） | 修复器把"让评测变绿"学成目标而系统性地把 harness 变形 | harness 仓库子集 digest 固化（复用 stable_digest 思想于 harness 文件集），Supervisor 每轮校验指纹；指纹不符 → 全循环熔断并告警 |
| H6 | 成本套利 | 用超长上下文/超多轮硬磨过题 | `limits.max_turns/max_tokens/timeout` 三限额已是任务一等公民且失败类独立（`eval_runner.rs:27-31`）；奖励的成本项 `λ_c·ΔCost` 使"磨"得不偿失 |

### 6.5 可证伪性纪律

每轮循环的每个假设，accept 前必须写成**可证伪预言**（falsifiable prediction）落账本：

```json
{"task": "edit_03", "claim": "在 directive D 下 3/3 通过且 mean cost ≤ 基线×1.1",
 "falsified_if": "任一重复失败，或成本超限", "verified_at": "<run-id>"}
```

晨报按"预言成立/被证伪"统计，**被证伪的预言与被证实的同等展示**——这是把 LLM 提出者从"自我说服者"约束为"假说机器"的制度化手段（针对 [20] 的误信息容忍/自我偏好偏差）。

---

## 7. 实验设计

### 7.1 基线对照（三臂）

| 臂 | 修复器可见信号 | 对应机制 |
|---|---|---|
| **A0 无反馈** | 任务描述 + 失败事实（pass/fail 位） | 消融下界 |
| **A1 trace 反馈** | + `trace replay` 失败轨迹 + TaskMetrics + 失败类标签 | 执行反馈的价值（对照 Self-Debug [3] 的主张） |
| **A2 全信号** | + 归因报告（`trace diff` vs 最近通过 + 证据链 + remediation hints）+ 反思记忆 | L1+L2 全量 |

每臂相同预算 B、相同候选数 k、相同闸门；`instruction_directive` 机制盖章区分武器（`eval_runner.rs:936-941`）。

### 7.2 指标与统计方法

- **主指标**：`Resolved@n(T_held)`（n=3 区间呈现）；**次指标**：每 accept 的 token/成本、每轮 accept 率、`T_reg` 回归数、迭代轮数-修复数曲线、被证伪预言占比。
- **方法**：任务级配对比较（同一任务在两臂的 n=3 结果），bootstrap（10,000 次重采样，按任务分层）给差值 95% CI；多重比较（消融族）用 Holm 校正。
- **诚实的功效声明**：20 题 × n=3 下，1 题翻转 = 5 个百分点；功效不足以支撑"<3 题翻转"的强结论——凡 CI 含 0 一律只作探索性报告，不写"显著"。需要确证时升级到外部钉选集（50+ 题）或延长观察期。这是 n=3 区间纪律 [eval_benchmarks.rs:66] 在统计上的自然延伸，不是新的发明。

### 7.3 消融

1. 反思记忆开/关（Reflexion 要素 [1]）；
2. 执行反馈开/关（把 trace 换成纯自评——预期显著变差，验证"自反馈不可信" [2][3]）；
3. 失败类提示（remediation hints）开/关（Agentless 式确定性信号 [12] 的贡献）;
4. 候选数 k ∈ {1, 5, 20}（verifier 就位前后）；
5. 停止条件灵敏度（K=1/3/5）。

### 7.4 第一个最小可行实验（MVP：一晚，5 题）

**选题**：从 `$SHANNON_HOME/eval/failures/<最近日期>/` 取失败样本最多的 5 题（若归档为空，先跑一轮 real eval 造数据）；同时把这 5 题从 `T_held` 排除（它们已是 dev 信息）。

**可执行步骤**：

1. **环境**（一次）：
   - `cargo build --release`；`SHANNON_HOME=$PWD/.eval-home`（隔离评测家目录，不污染真实会话）；
   - dry-run 自检：`cargo run -p shannon-core --example eval_runner`（嵌入式 dry_run 契约全绿 = harness 未被动的基线证据）。
2. **基线**：对 5 题 + reg_01..10 跑 real eval n=3（`crates/shannon-core/examples/eval_runner.rs`、`bench_runner.rs` 已有 CLI 入口），落 `eval/runs/<run-id>/`，记录基线区间与 `stable_digest`。
3. **循环**（Python/just 脚本，即 §5.2 伪代码的 200 行实现；Supervisor 侧复用 autonomous-improvement-loop 的预算闸门设计）：
   - `build_context`：读 `classification.json` + `trace show --turn N` 输出 + 任务 TOML；
   - 修复器调用：`shannon --prompt "<结构化假设请求>" --output-format json-stream --schema patch_proposal.json --allowed-tools Read,Grep,Glob --max-turns 30`（`A_prompt` 首期只允许产出指令补丁，不碰源码）；
   - 闸门：`cargo fmt --all -- --check` + `cargo clippy --workspace -D warnings`（与 CI 同严，凭既有教训：宽松门会漏）+ dry-run 全绿 + reg 套件 + digest 比对；
   - 全程账本 `ledger.jsonl`（含每假设的可证伪预言）。
4. **晨报**：accept/reject 明细、预算消耗、held-out 延迟评估结果、被证伪预言清单。
5. **成功判据**：≥1 accept 且 held-out 不降；否则输出"失败也有效"的归因（哪类假设系统性不过闸门——这本身就是 L1 的改进输入）。

---

## 8. 风险评估与伦理

| 风险 | 等级 | 缓解 |
|---|---|---|
| **自我欺骗**（自改评测器 / 判定讨好） | 高 | §6.4 H1/H4/H5 三层工程隔离；LLM 判定零奖励权重；harness 指纹每轮校验 |
| **过拟合 20 题** | 高 | 三层探针：held-out 切分 + 外部钉选集 + 季度新 reg 题（§6.3）；泄漏实证警示来自 [21] |
| **成本失控** | 中 | token/时间/美元三层闸门 + 单假设成本上限 + 无改进窗口停止（§5.2） |
| **统计误读**（小样本幻觉进步） | 中 | n=3 区间 + bootstrap CI + "CI 含 0 只作探索"纪律（§7.2） |
| **回归引入主线** | 中 | `A_code` 只进 PR、合入永远人工；`stable_digest` + reg 套件双重闸门 |
| **沙箱逃逸 / 越权写** | 低-中 | Landlock fail-closed（装不上即拒启）；越权自动分类并熔断（§6.1） |
| **自改进的双用性** | 低 | 修复器权限走既有 ApprovalMode/profile 体系；全部动作留 L0 审计；Supervisor 是确定性脚本，随时 Ctrl-C 即全局停止（无自治常驻进程） |
| **L4 过早启动** | 中 | 三条启动前提缺一不可（§5.4）；数据量门槛写成硬数字，不接受"感觉差不多了" |
| **归因报告误导修复器**（garbage-in） | 中 | 归因本身可测（L1 验收：对历史归档回放评分）；负反思机制让错误归因进入记忆并被后续轮次证伪 |

伦理要点：自优化系统的每一步都保持**人类可审计**（账本 + L0 双记录）、**可中止**（确定性 Supervisor、无自治进程）、**可回滚**（worktree + 快照 + digest）。本方案不涉及对外部系统的自主行动；修复器的全部写操作域限于本仓 worktree。

---

## 9. 一周可启动的 MVP 清单

| 天 | 事项 | 产出 |
|---|---|---|
| D1 | 评测基线：隔离 `SHANNON_HOME` 下跑 real eval（20 题 n=1 摸底 → 失败集中题 n=3）；确认失败归档落盘 | 基线区间 + 首批评失败归档 |
| D2 | held-out 切分：T_dev(15)/T_held(5) 分层抽样清单成文（不进 harness 代码，先做成 run 配置）；外部钉选集指纹记录 | 切分表 + 指纹快照 |
| D3 | L1 归因报告生成器 v0：`classification.json` + `trace show` 失败点片段 + 最近通过轨迹 `trace diff`，产出 `attribution.md` | 对历史失败归档的归因样张 ≥5 份 |
| D4 | L2 循环骨架：Supervisor 脚本（预算三层闸门 + 静态门 + reg 门 + digest 门），`A_prompt`-only（--schema 结构化假设 + --allowed-tools 收窄） | `bootstrap.py` 可跑通一轮空转 |
| D5 | MVP 一晚实验：5 题（失败最多的 dev 题）× k=3 假设 × 预算闸门 | `ledger.jsonl` + 晨报 |
| D6 | 复盘：归因命中率、闸门误杀率、每 accept 成本；修正 remediation hints 与 build_context | 实验小记（写进本文档附录） |
| D7 | 评审 + 决策：是否扩到全 15 个 dev 题 / 是否开始 L3 导出器 schema 设计 | go/no-go 与下轮预算 |

**MVP 明确不做**：不动 `crates/**` 源码（首期 `A_code` 关闭）、不训练任何模型、不自动合入任何东西、不改 harness 正本。

---

## 10. 与上游文献的差距声明（诚实的边界）

- 本方案的 L2 在方法论上不新颖（Reflexion [1] + Self-Debug [3] 的组合），其价值在于**与 Shannon 既有基础设施的零缝隙对接**与闸门纪律；
- L3/L4 的数据量与算力估算基于公开工作的数量级外推（[7][9][10][19]），未经本仓实证；启动前必须用 D1-D7 的实测吞吐重新校准；
- 文献中"端到端 agentic RL 吃掉手工流水线"的结论 [19] 在 Shannon 当前规模下**不成立也不追求**；若未来任务集扩到数百题且 L2/L3 收益递减，应重读该结论再决策。

---

## 11. 参考文献

| # | 工作 | 链接 | 一句话 |
|---|---|---|---|
| [1] | Reflexion (NeurIPS 2023) | <https://arxiv.org/abs/2303.11366> | 语言反思存入情景记忆替代权重更新 |
| [2] | Self-Refine (NeurIPS 2023) | <https://arxiv.org/abs/2303.17651> | 同一 LLM 自反馈-精炼迭代，无需训练 |
| [3] | Self-Debug (ICLR 2024) | <https://arxiv.org/abs/2304.05128> | 执行反馈 + 代码解释自调试，最难任务 +~9% |
| [4] | Agent-R | <https://arxiv.org/abs/2501.11425> | MCTS 反思构造修正轨迹，迭代自训练 |
| [5] | Draft, Sketch, and Prove (ICLR 2023) | <https://arxiv.org/abs/2210.12283> | 非形式草稿引导形式化证明（autoformalization） |
| [6] | SWE-agent (NeurIPS 2024) | <https://arxiv.org/abs/2405.15793> | Agent-Computer Interface 设计决定 agent 能力 |
| [7] | CodeRL (NeurIPS 2022) | <https://arxiv.org/abs/2207.01780> | 单测结果训练 critic，actor-critic 推理时筛选 |
| [8] | RLTF (TMLR) | <https://arxiv.org/abs/2307.04349> | 多粒度单测反馈的在线 RL |
| [9] | SWE-Gym | <https://arxiv.org/abs/2412.21139> | 首个真实 SWE agent 训练环境（2,438 任务）+ verifier |
| [10] | SWE-RL (Meta, NeurIPS 2025) | <https://arxiv.org/abs/2502.18449> | 开源软件演化数据 GRPO + 补丁相似度规则奖励 |
| [11] | RLEP | <https://arxiv.org/abs/2507.07451> | 已验证成功轨迹回放进 RL，加速收敛 |
| [12] | Agentless | <https://arxiv.org/abs/2407.01489> | 定位→修复→验证确定性流水线，低成本高竞争力 |
| [13] | SWE-Search (ICLR 2025) | <https://arxiv.org/abs/2410.20285> | MCTS + 价值 agent 的推理时轨迹搜索 |
| [14] | SWE-Bench+ | <https://arxiv.org/abs/2410.06992> | SWE-bench 题面泄答案（检出 86%）与记忆化实证 |
| [15] | Let's Verify Step by Step | <https://arxiv.org/abs/2305.20050> | 过程监督（PRM）一致优于结果监督（ORM） |
| [16] | LLM-as-a-judge 综述 | <https://arxiv.org/abs/2411.16594> | 位置/权威/自我偏好等判定偏差目录与缓解 |
| [17] | A Survey on LLM-as-a-Judge | <https://arxiv.org/abs/2411.15594> | 可靠 LLM judge 系统的构建问题 |
| [18] | Kimi K2 | <https://arxiv.org/abs/2507.20534> | 合成工具轨迹 + VRPO：工具使用可被 RL 训练 |
| [19] | Kimi-Researcher | <https://moonshotai.github.io/Kimi-Researcher/> | 端到端 agentic RL，零结构设计，HLE 8.6→26.9 |
| [20] | DeepSeek-R1 | <https://arxiv.org/abs/2501.12948> | 纯 RLVR 自演化，反思/验证行为涌现 |
| [21] | Absolute Zero | <https://arxiv.org/abs/2505.03335> | 零数据自博弈 + 执行器可验证奖励 |
| [22] | Llama-Nemotron | <https://arxiv.org/abs/2505.00949> | NVIDIA 开放 RL 后训练管线（NeMo-RL） |
| [23] | R2E-Gym | <https://arxiv.org/abs/2504.07164> | 程序化生成可执行 SWE 训练环境 + 混合 verifier |
| [24] | Concrete Problems in AI Safety | <https://arxiv.org/abs/1606.06565> | reward hacking 等五类安全问题的经典分类 |
