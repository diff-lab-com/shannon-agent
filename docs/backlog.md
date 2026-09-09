# Shannon 评测改进 — 遗留项跟踪（backlog）

- 建立日期：2026-09-07
- 维护约定：每项标注【性质】(真实能力 / eval-only / 已砍掉) 与状态；任何新项入库前先过
  「换到真实用户场景是否仍成立」判据；被砍掉的项**记录在案不删除**（含理由），防止后来者
  捡起当金科玉律。

---

## 一、事故与教训档案（本周期三次重要的方法学教训）

### L1：多变量实验失控（P4 v2/v3）
改进批次验收时同时改了**二进制 + 适配器 + 并发度（3→6）**三个变量，v2 得 18/86、v3 得
17/89，均不可归因。教训：**评测改动一次只动一个变量**；对照条件必须与基线严格一致。

### L2：看门狗阈值 ≤ 模型思考静默上限（P4 v2 的 0-token 死亡螺旋）
180s 内容级看门狗低于 GLM-5.3 thinking 的正常静默（实测 312s）：杀健康流 → 引擎重试 →
从头再思考 → 再杀 → rc=3 全灭。修复：420s + 注释警告。
**固化规则**：内容级看门狗阈值必须 > 被测模型最大思考静默（换模型时重新标定）。

### L3：prompt 传值的三次试错与验证协议（P4 复测 0/85）
`-p - < file` 把字面 `-` 当 prompt（clap 无 stdin 哨兵）；分离式 `-p "$(cat f)"` 拒绝
`-` 开头值；最终 `--prompt=$(cat f)` 附着式逐字节验证通过。
**固化规则**：发车核验必须断言 start 事件的 prompt 字段内容，响应存在性不算证据。

### L4：prompt 措辞的过拟合实锤（R1 的由来）
「尽快收工」子句动机是评测回合经济学，实害是 20 个 control 回归（过早终止）。
**固化规则**：系统提示措辞的每一条都必须能回答「真实用户是否受益」；回合经济学不是
用户价值。单题证据永不驱动 prompt 修改。

---

## 二、已落地改进总账（全部有 commit 对照）

### 真实能力/健壮性（产品代码）
| 项 | commit | 状态 |
|---|---|---|
| A1 think-only 续推 nudge（有界 2 次） | `fc984504` | ✅ |
| A2a 工具链探测指引（受限环境先 probe） | `c9434501` | ✅ |
| A2b 完成门禁（环境就绪 ≠ 任务完成） | `63daf6e1` | ✅ |
| A2c 收工子句回退 → correctness-first（R1） | `f6bac100` | ✅ |
| A3 工具输出路径别名双向回显 | `b7fcf681` | ✅ |
| A4 Bash 安全器危险动词分级 + 整改建议 | `c9434501` | ✅ |
| A5 内容级流看门狗（机制；eval 默认 420s） | `fc984504`+`0d29caf2` | ✅ |
| B.4 超时错误分类修正（rc=3 非 rc=5） | `2944c6df` | ✅ |
| B.5 RunBackground/WaitForLog/Kill 工具组 | `333bb78a` | ✅ |
| B.6 SHANNON_TOKEN_BUDGET 看门狗（默认关） | `3a14b6db` | ✅ |
| B.7 strikes 机制（**默认关**，R2 后 opt-in） | `f897e785`+`f6bac100` | ✅ |
| B.3 Write/Bash 根集合统一 | `b7fcf681` | ✅ |
| C.8 musl 静态构建 | `0060e894` | ✅ |
| A7 headless run 级重试 + infra_failure 标记 | `d723f044` | ✅ |
| C1 events.jsonl turn 字段按请求递增 | `55c980fd` | ✅ |
| C4 glm-5.3-flash 官方价目入 catalog（修复 ~60 倍成本虚高） | `59f45e67` | ✅ |
| AnalyzeImages 多图单请求 | `8bb416dd` | ✅ |
| 架构不变量 KEEP 标记 ×2 | `d7e03816` | ✅ |

### 评测设施（eval-only，不进产品）
| 项 | commit |
|---|---|
| wrapper-glm + driver 参数化 + v2 启动器 + 镜像预拉/保障 | `d3502ec1`/`c812d519` 等 |
| TB harness 重建 + 冷路径 + rc=4 重试 + compose 日志 | `ad66867a`/`a7828541`/`5285dc0f` |
| harbor adapter（libc 探测选 musl、rc=4 重试、stdin→附着式 prompt、看门狗注入） | `2a210c06`/`aa63e163`/`0d29caf2` |
| bench_runner --task-list 分片 | `6bebe579` |
| swe-harness env-prefix 修复 + hint 增强 | `e092b6b4`/`dc9298d4` |
| 回归池判分修正（MultiEdit/字面量/预算）+ reg_05 契约显式化 | `669315cf`/`6afc1759` |
| provider A/B harness（执行待按量 key） | `012bce57` |
| C2 verify_script 回写契约回归测试 | `4504736d` |
| C3 verdict token 回退提取 | `c812d519` |
| TB 上游 issue 草稿（verifier 缺 uv） | `3ca134da` |

---

## 三、验收数据现状

| 评测 | 改进前 | 改进后 | 验收有效性 |
|---|---|---|---|
| SWE-bench Verified 50（3 并发，同 anchor） | 30/50 | **37/50** | ✅ **有效**（单变量，同配置对照） |
| 内部回归池 | 3/10 | 6/10 | ✅ 有效（n=1 快速验证） |
| TB 9-pin | – | 3/9×3 轮稳定 | ✅ 记录在案 |
| **TB 2.1 全量 89** | 36/89 (40.4%) | **未验收** | ❌ v2/v3 配置失控（18/86、17/89 均无效），v4 干净实验被终止 |

**结论**：SWE50 的改进有效性成立；TB2.1 的改进效果待一次干净复测（见 T1）。
v3 的 17/89 不构成「改进有害」的证据（6 并发限流 + A2 措辞回归 + 看门狗阈值三因叠加），
但 A2 收工子句本身已被回退（真实回归，与分数无关）。

---

## 三-B、改进批次全量审计裁定（2026-09-08，静态代码级）

判据：「换到真实用户场景仍成立」。

**真实改进——保留（13 项）**：A3 路径别名（双场景验证零误触发）、A4 安全分级（代码抽查：
ANSI-C 保持 critical、危险动词升级、独立检查链完整）、B.3 根集合、B.4 超时分类、B.5 后台
工具、B.6 token 预算（默认关）、C.1 turn 字段、C.4 价目修复、C.8 musl、A7 run 重试、
AnalyzeImages、B1/B2 判分修正、C.2+KEEP。

**已修正的过拟合（2 项，处置完毕）**：A2c 收工子句（评测回合经济学动机，实害 20 control，
已 correctness-first 重写 `f6bac100`）；B.7 strikes 默认自动启用（风险不对称误判，已降
opt-in `f6bac100`）。

**保留带观察（2 项）**：A1 nudge（代码审查通过；净效果待析因实验）；B.7 机制（opt-in 态，
等真实证据）。

**需要实测定谳 → 已定谳（2026-09-09，2×2 析因实验完成）**：
22 题分层样本 × 4 cell（提示 P1d/R1 × nudge on/off，全部当前引擎代码、看门狗关、3 并发）：

| cell | resolve |
|---|---|
| (P1d, off) | 7/22 = 32% |
| (R1, off) | 7/22 = 32% |
| (P1d, on) | 8/22 = 36% |
| (R1, on) | 7/22 = 32% |

**定谳**：提示主效应 **-0.5 题（零）**；nudge 主效应 **+0.5 题（零）**。
1. R1 correctness-first 提示**无分数代价**（P4 v3 的崩盘系配置事故，与本提示无关）——保留，争议关闭。
2. A1 nudge **中性**（+0.5 题在噪声内）——保留（机制有界、针对真实间歇性失败模式），不调默认。
3. **真正发现**：22 题中 10 题在全部四个 cell 恒过/恒败（稳定能力核），**~11 题处于方差带**
   （四 cell 结果 0/1 翻转）——TB2.1 上 shannon+glm 的单轮跑分噪声带 ±5 题/89，
   **±5 以下的 run 间差异不可解读，n=1 对比对该基准无统计意义**（本周期所有 ±8 级
   对比均受此影响）。后续在该基准的任何验收必须 n≥3 或配对设计。

---

## 四、待办任务

### T1 验收结果（2026-09-08，全量 89 题，P1d 等条件，含 R1/R2）
**28/89 = 31.5%——未通过验收**（P1d 40.4%，-8；control 层 3/6 失败）。
已确认生效的修复：pytorch-model-recovery（CLI bug）✓、distribution-search（流超时层）✓、
sanitize-git-repo（A2 门禁）✓、mteb-retrieve ✓。但 AgentTimeout 24（P1d 15）+ control 翻红
表明**改进批中存在至少一项净负向改动**（候选：A2 提示改动改变 agent 行为节奏、A3 别名在
TB 容器内误触发、A4 放行改变探索路径、A1 nudge 干扰）。
**根因已定位（2026-09-08，推翻二分假设）**：对 T1 全部 61 个失败逐题分类——
**30 个是宿主机网络基础设施故障**（18 个第一调连接失败 `error sending request`，
A7 正确标记 infra_failure；12 个 verifier uv bootstrap 下载失败），与产品改动无关；
其余为 AgentTimeout 24（harbor 900s 墙钟，thinking 延迟所致）+ turn_limit 等。
四个嫌疑特性（A1/A2/A3/A4）经轨迹取证**全部排除致害**：A3 零 /workspace 误回显、
A1 nudge 零误触发、A4 放行无异常路径、A2 门禁靶题 sanitize ✓。
净效应重估：T1 排除 infra 后 28/77 ≈ 36.4% vs P1d 排除后 36/77 ≈ 46.8%——差异的主部
是**两次跑批撞上不同的网络窗口 + 各不相同的 verifier 受害题集**（非确定性噪声），
其次才是模型方差。**宿主机 egress 劣化（uv CDN 不通、docker hub EOF）是本评测环境
最大的测量噪声源**（已两次毒化验收），后续验收必须：①先测网络窗口；②infra 失败题
自动重跑一轮再聚合；③报告 infra 分离口径。
**T1-retry 结果（2026-09-08）**：4 题被救回（cancel-async-tasks、db-wal-recovery、
large-scale-text-editing、pypi-server——确认网络窗口噪声），11 个 uv-CDN 类 verifier
失败依旧（该 CDN 在本网络不可达，非瞬态）。**最终修正口径**：32/87 = 36.8%
（2 题 qemu glibc DNF 另计）；排除 verifier-infra 后 T1-final 32/76 = 42.1% vs
P1d 36/70 = 51.4%——**排除 infra 后仍有 ~9pp 差距，主嫌疑是 A2 正确性优先措辞使
agent 更彻底但更慢（AgentTimeout 15→24），需 A2 原版 vs R1 版的对照二分定谳**（T1b）。
在 T1b 定谳前，TB2.1 上本批改进记为「质量换取速度、净分数未证实」。

### T0（进行中）
- [ ] **dev 合并（延后，需协作）**：feat/agent-eval-bench 与 dev 已深度分叉
      （branch 领先 ~951 / dev 领先 ~1048 提交，含 desktop 导入、schema 迁移等并行重构），
      实测合并冲突 167 文件。**不宜单方面强解**——会损及并行会话工作。建议：
      与 dev 侧负责人协调时间窗，或经 PR 评审合并；合并前本分支已自洽完整，无丢失风险。
- [ ] **T1 TB2.1 验收复测**（R1/R2 落地后）：优先 31 题分层子集（`~/.shannon/eval/shards/p4-30.txt`）
      × 3 并发 × 无看门狗（`SHANNON_STREAM_IDLE_SECS=0`）；对照 P1d 同题集逐层 delta。
      通过标准：control 6 题无回归 + 流超时层显著收敛。

### 建议排队（真实能力/健壮性）
- [ ] musl 二进制的 CI 产出（`CFLAGS_x86_64_unknown_linux_musl` fortify 处理已文档化，待接 CI）
- [ ] provider A/B 执行（需按量 key：`~/.shannon/credentials/zhipu-payg.json`；脚本就绪 `012bce57`）
- [ ] regression 池加入 nightly 工作流（现 nightly 只跑 L1 read/edit/search 层）
- [ ] A2 措辞的持续观察：若真实使用再现「过早收工/该停不停」新证据，按 L4 规则处理

### 观测性
- [ ] TB harness 简化 token 估计仍为近似值（stderr 回退），待引擎侧提供中间事件用量
- [ ] harbor job 级 cost 聚合依赖 catalog 价（C4 修复后已可信），补进聚合报告

### 基建
- [ ] `ensure-images.sh` 接入批次前自动执行（现为手动）
- [ ] dev 上并行工作的协调约定（本周期 docker prune 两次清空评测镜像）

---

## 五、已砍掉项（记录在案，含理由——勿捡起）

| 项 | 砍掉理由 |
|---|---|
| A2 措辞微调「patch 共享基类方法时追踪子类 override」 | 单基准题（sympy-13031）证据，教科书式过拟合；已由 L4 规则覆盖 |
| 「尽快收工」提示子句（R1 已回退） | 评测回合经济学动机；实害 20 control 回归（见 §一 L4） |
| B.7 strikes headless 默认自动启用（R2 已降级 opt-in） | 过早终止 > 徘徊的风险不对称性；默认应保守 |
| DockerTool 暴露（以 TB 类别为动机） | 真实用户经 Bash 已可用 docker；按用户需求排期，不按评测题目 |
| Agent subagent / Plan 引导（按 TB 失败模式设计） | 需真实使用数据驱动，非评测失败驱动 |
| Verifier 镜像自行魔改预装 | 改考卷式提分；正道是向上游提 issue（草稿 `docs/tb-verifier-images-issue-draft.md`） |
| OSWorld/WebArena/TheAgentCompany/Commit0/SWE-Lancer 等基准 | 形态不符或成本远超收益（详见实施方案 §1.1） |
| lm-evaluation-harness / LangSmith 类 | 静态任务框架/商业托管，与 agent 评测目标错位 |
