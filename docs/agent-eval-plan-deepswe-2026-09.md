# DeepSWE v1.1 评测与产品改进实施方案（shannon × glm-5.3-flash）

- 日期：2026-09-18
- 分支：`feat/deepswe-eval`（基于 dev @ f1e1e2f5，worktree `shannon-deepswe`）
- 默认被测模型：**glm-5.3-flash @ zhipu-coding-plan**（与 v1/v2 冻结基线同 anchor）
- 状态：**已批准**（2026-09-18 评审通过；口径确认：基线 113×n=1 + 复测 113×n=1 配对对照 + 31 题分层子集 ×3 稳定性抽检；先内部报告不提交 Datacurve；P3 backlog 不设评审闸）

---

## 一、目标与竞争力判据（预先固化，防事后挪门柱）

把 Datacurve **DeepSWE v1.1**（113 题抗污染长程编码基准）接入 shannon 评测体系，跑出
shannon+glm-5.3-flash 的 DeepSWE 分数；以失败分析驱动**通用场景**产品改进，复测并交付对标报告。

| # | 判据 | 门槛 |
|---|---|---|
| 1 | 主判据（scaffolding 增益） | ≥ 官方同模型 harness 口径 **63% ±4%**（Datacurve 榜 mini-swe-agent [max]，配对逐题对照为主证据） |
| 2 | 成本判据（性价比位） | 成本/题 ≤ **$0.5**（API 折算；官方 mini-swe-agent 口径 $0.24） |
| 3 | 档位判据（超越同档） | > deepseek-v4-flash 53%、claude-sonnet-5 54%（其成本约为我们 55 倍）、glm-5.2 44% |

## 二、调研结论（方案依据）

1. **DeepSWE v1.1**：113 个原创长程任务（91 活跃仓库、TS/Go/Python/JS/Rust 五语言、参考解均
   668 行，SWE-bench Pro 的 ~5.5 倍），Harbor 任务格式（`task.toml` + `instruction.md` +
   `environment/` + `tests/` + `solution/`），docker 隔离执行；v1.1 起独立判分容器
   （`[[verifier.collect]]` 抽取 commits 成 patch → 干净容器判分 → `verifier/reward.json`）。
   运行框架 `pier`（Harbor 兼容，`uv tool install`），支持本地 docker 与 Modal。
2. **官方榜**（2026-09-03 更新）统一 mini-swe-agent harness：glm-5.3-flash [max] = **63% ±4%**
   （$0.24/题、73k 输出 tokens、约 123 步）；top 74%（gpt-6-astra / gemini-3.8-flash /
   claude-opus-5）；glm-5.3 69%；claude-sonnet-5 54%（$26.40）；deepseek-v4-flash 53%；
   glm-5.2 44%。
   **锚点校准（P0-5 已定案）**：z.ai 官方 GLM-5.3-Flash 博客自报 DeepSWE = **63.4**，与
   Datacurve 榜 63% 两个独立来源一致 → **主锚固化为 63% ±4%（Datacurve，同 runner）**，
   z.ai 63.4 为副锚（其配置：mini-swe-agent、timeout=6h、400K context，与语料默认 3h 不同，
   报告需标注）。原方案起草时引用的"53.7"经查证无公开出处，系引用误差，作废。
3. **dev @ f1e1e2f5 已含全部所需资产**：20 项产品改进（A1-A7/N1/N2/P1-3，回测 SWE50 38/50）、
   `scripts/eval/harbor-adapter/shannon_harbor_agent.py`（容器内驱动 shannon 的成熟模式）、
   run-batch.sh 预算闸/续跑/429 自愈、preflight-network.sh、retry-infra-failed.sh、
   wrapper-glm.sh、教训档案 L1-L4、反过拟合纪律（backlog §五 + 「真实用户受益」判据）。
   `feat/agent-eval-bench` 已 100% 合入 dev（0 独有提交）。

## 三、控制变量

| 项 | 取值 | 说明 |
|---|---|---|
| model / provider | `glm-5.3-flash` @ `zhipu-coding-plan` | wrapper-glm 注入 key；**硬编码真实 API id**（batch-5 教训） |
| thinking/effort | GLM 默认档（实测 max），写进 anchor | 与官方榜 [max] 档对齐 |
| 跑批时段 | 尽量排非高峰（工作日 14:00-18:00 UTC+8 之外） | GLM Coding Plan 积分非高峰 5 折（z.ai 官方说明） |
| 轮次 | `--max-turns 150` | 榜均 123 步 + 余量；DeepSWE 长程远超 SWE50 |
| 看门狗 | `SHANNON_STREAM_IDLE_SECS=420` | L2：>312s 思考静默 |
| 并发 | 本地 docker 3 并发 | TB 验证档；Modal 为升级选项（另行确认） |
| 判分 | DeepSWE 官方 verifier（独立容器） | 不碰判分环境（§五砍单纪律） |
| 引用口径 | n=1 仅内部；最终分 = 两波 n=1 配对 + 31 题分层 ×3 抽检 | 引用纪律：n/日期/anchor 三元组 |

## 四、实施阶段

### P0 基建与前置调研（0.5 天，token 闸 2M）
1. worktree + 分支（本仓库）；本方案落盘 + commit。
2. `git clone https://github.com/datacurve-ai/deep-swe ~/eval-corpora/deep-swe`（不入库）；
   `uv tool install git+https://github.com/datacurve-ai/pier`；**核对 license 允许自托管**。
3. **pier 机制源码研读** → `docs/research/pier-adapter-notes-2026-09.md`：
   a) `--agent` 自定义注册路径（兼容 harbor `BaseInstalledAgent` 还是 pier 自有插件格式）；
   b) **commit 契约**：`[[verifier.collect]]` 期望的产物形态（commit/diff），mini-swe-agent 如何满足，
      adapter 是否需补 commit（若补须论证对所有 agent 等价）；
   c) per-agent 网络白名单配置（放行 `open.bigmodel.cn`）；
   d) pier job 断点续跑能力（不足则照 run-batch.sh 补 job 级 resume）。
4. 锚点校准：z.ai 53.7（08-14）vs Datacurve 63%（09-03）定因（版本/effort/重跑）。
5. `preflight-network.sh` 四点探针 + `just eval-real --task read_01` 连通 smoke。

**验收**：pier 装好且 113 题可枚举；adapter 设计笔记三问有答案；license 无障碍；smoke 通过。

### P1 Pier×Shannon adapter + 冒烟（1-2 天，token 闸 10M）
新建 `scripts/eval/pier-adapter/shannon_pier_agent.py`（照抄 harbor adapter 骨架）：
- install：libc 探测（musl→musl release build，否则 glibc dev build）→ upload →
  `/usr/local/bin/shannon`；`shannon --version` 入 trial agent info（anchor 禁 release fallback）。
- run：env 注入（`SHANNON_API_KEY` + `SHANNON_*` 透传 + `SHANNON_STREAM_IDLE_SECS=420`）；
  prompt 走 upload 文件 + 附着式 `--prompt=$(cat /tmp/shannon_prompt.txt)`（L3）；
  NDJSON → `/logs/agent/shannon.ndjson` + stderr 留档；rc=4 限流 60s 重试一次（沿用）。
- **DeepSWE 特有**：commit 契约落地（按 P0-b 结论）；`--max-turns 150`；白名单放行 GLM API。
- 发车核验断言 start 事件 prompt 字段（L3 硬规则）。

冒烟闸门（逐级放量，一次一个变量——L1）：
1. 单题（Python）端到端：`reward.json` 产出、verdict token 非零、NDJSON 可回放；
2. 3 语言 3 题（TS/Go/Rust 各一）全链路；
3. 单题墙钟/tokens 实测 → 校准 P2 pacing/预算。

**验收**：5 题冒烟全绿 + 冒烟数据表 + adapter commit。

### P2 基线评测：113 × n=1（2-4 天墙钟，token 闸 500M in）
- 本地 docker 3 并发；pacing + 429 自愈 + 可原位续跑（run-batch 模式移植）。
- **infra 卫生（T1 三条全上）**：发车前 preflight 门禁；infra 失败题自动重跑一轮再聚合；报告 infra 分离口径。
- 产物：`~/.shannon/eval/deepswe-v1/` + 基线分（n=1 诚实标注）+ 逐题矩阵 + 失败轨迹归档。

### P3 失败分析 → 通用改进 backlog（2-3 天）
- 归因三分法：a) shannon scaffolding b) 模型能力 c) infra/判分 artifact；A1-A7 已修项不重复归因。
- **长程新信号面专项**：①~120 步轮次经济学与通用收工信号（非评测措辞，A2c/R1 教训）；
  ②长上下文 compaction 在 100+ turn 轨迹的保真；③大跨度多文件 Edit（参考解均 668 行）；
  ④工具链探测指引的五语言泛化；⑤commit/工作区卫生。
- 每项过两道闸：「真实用户场景仍成立」判据 + L4 规则；**禁止 DeepSWE 题面/verifier 任何适配**。
- 产物：`docs/deepswe-eval-findings-2026-09.md` + backlog（证据→假设→修复→验证）。
  按既定授权不设闸，直接进 P4。

### P4 实施改进（1-2 周，规模以 backlog 为准）
- 正常 TDD 开发流（12k+ 测试全绿 + clippy 零警告），每项独立 commit。
- **每项验证三件套**：①DeepSWE 失败题复放转绿；②regression 池 n=3 无回归；
  ③SWE50 分层抽样 10 题 spot-check 无回归（跨基准不伤害闸门）。

### P5 复测 + 对标结题（1-2 天，token 闸 500M + 150M）
- 复测与基线同 anchor/并发/prompt 逐字（L1），113×n=1 配对对照 + 31 题分层 ×3 抽检。
- 交付 `docs/deepswe-final-report-2026-09.md`：分数±方差带、成本/tokens/步数 vs 官方
  （$0.24/73k/123）、榜单对照表（§一判据逐条裁定）、scaffolding 增益结论、改进 commit 清单、
  infra 分离口径、复现指引。**不提交 Datacurve**（如需上榜另行确认）。
- 收尾：更新 backlog.md、scripts/eval/README.md、metrics.md。

## 五、预算与风险

| 阶段 | token 闸 | 墙钟 |
|---|---|---|
| P0 | 2M（smoke） | 0.5 天 |
| P1 | 10M（5 题） | 1-2 天 |
| P2 基线 | 500M in | 2-4 天 |
| P3 | – | 2-3 天 |
| P4 复放 | 按题（单题 ≤ 全量均值 2 倍） | 1-2 周 |
| P5 | 500M in + 子集 150M | 1-2 天 |

| 风险 | 对策 |
|---|---|
| pier 不支持自定义 agent / 契约不符 | P0 源码研读先行；fallback 按 pier agent YAML/插件格式实现；最坏自写 driver 复刻判分流（仅 P1 前定夺，不带病放全量） |
| commit 契约误伤判分 | P0-b 确认 + 冒烟闸验证 reward.json 真实产出 |
| 限流窗口（batch-6 曾 76% 拒绝） | 30s pacing + rc=4 自愈 + 续跑；必要时降并发 |
| 长程墙钟超预期（单题 1-4h） | 冒烟实测校准；预算闸硬停；Modal 为升级选项（另行确认） |
| egress 噪声毒化验收（T1 两次前科） | preflight 门禁 + infra 题自动重跑 + 分离口径 |
| 改进过拟合 | 「真实用户受益」判据 + L4 + 跨基准三件套 + backlog §五纪律 |

## 六、全局纪律（继承）

held-out（内部 L1 20 题不得用于调 prompt）；引用纪律（n/日期/anchor 三元组，n=1 仅内部）；
评测改动一次一个变量（L1）；看门狗阈值 > 模型最大思考静默（L2，换模型重新标定）；发车核验
断言 start 事件 prompt 字段（L3）；prompt 措辞只由多题/真实使用证据驱动（L4）。

## 附：主要外部参考

- DeepSWE（datacurve-ai/deep-swe；deepswe.datacurve.ai；arXiv 2607.07946）
- Pier（datacurve-ai/pier；Harbor 兼容 + per-agent 网络白名单）
- GLM-5.3 官方基准（z.ai/blog/glm-5.3，2026-08-14：DeepSWE v1.1 = 53.7，历史口径）
- mini-swe-agent（swe-agent/mini-swe-agent，官方榜统一 harness）
