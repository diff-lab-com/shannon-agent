# DeepSWE 评测发现与改进记录（shannon × glm-5.3-flash，2026-09 开始）

- 证据规范：所有结论附数据路径；基线波次数据回填前先固化冒烟期发现。
- 数据源：`~/.shannon/eval/deepswe-smoke/jobs/deepswe-smoke-{1..5}/`（trial 内
  agent/shannon.ndjson + shannon.stderr + verifier/reward.json + exception.txt）。

## 一、冒烟期发现（P1，2026-09-18）

### F1（infra/契约，已修复）：pier install_spec 内联构建跳过运行时安装
pier 按 adapter `install_spec()` 指纹把步骤内联进派生镜像后跳过 `install()`，二进制从未进
容器（smoke-1 exit 127）。修复：adapter 覆写 `setup()` 无条件安装。
见 `docs/research/pier-adapter-notes-2026-09.md` §五.1。

### F2（产品缺陷，已修复 A8）：上游 6 分钟网关切断杀死整个 run
**现象**：GLM coding-plan 网关对单个 LLM 调用约 6 分钟硬切断。GLM-5.3 thinking=max 的
长思考调用撞墙后，turn 的流式中途死亡 → Timeout 分类（rc=3）→ **整个 run 失败，已完成的
全部 turn/工具成果作废**（空 patch → verifier 判 F2P 0/P2P 275）。
**证据**：smoke-2（1h11m，turn 8 死，错误 "Request timed out"）；smoke-4（1h11m，turn 14 死，
A7 run 级重试 attempt 1/2/3 三连死——stderr 第 70/116 行可见 [run-retry] 日志，证明
**整跑重启式重试对确定性硬 turn 无效**）；历史 TB2.1 3/50 题同类污染；sympy-13031
~600s 超时空 patch。共 4 起独立事故。
**修复（A8，commit `a5baf851`）**：turn 级流死亡续推重试——Timeout 类错误时仅重发当前
turn（历史与工具状态全保留，turn 计数不前进），默认 2 次（`SHANNON_TURN_RETRIES`），
续推请求带一次性指引（「继续，保持响应适度集中」），Progress 事件可见。8 个新测试，
shannon-core 3922 / shannon-engine 1149 全绿，clippy 0 警告。
**排序决策**：此项属 P4 性质但提前到 P2 基线之前实施——不修则基线带 ~5-10% 纯 infra
DNF（冒烟 3/5 次运行死于该因），违反测量卫生纪律（T1 教训「infra 失败不得计入模型失败」
的前置义务）。改进有效性以 smoke-2/4（死亡）vs smoke-5+（存活）对照 + 后续波次
infra-DNF 率量化。
**诚实边界**：静默断流（无错误浮出）不在 A8 覆盖内，仍靠 A1 think-only nudge 兜底；
SMALLER 提示措辞（续推指引）按 L4 规则属「真实用户受益」类（网络中断续推），非评测特化。

### F2-b（F2 续篇，smoke-5）：静默断流变体绕过 A8，截断生成被当作完成
**现象**：smoke-5（27m52s）流中途异常 EOF，错误变体 `StreamEndedUnexpectedly`
（非 timeout 类）→ A8 不触发 → has_partial 保全路径把截断文本入库 + Warning +
Completed → headless **rc=0 假成功**。模型 Edit 完成但没走到 commit，patch 空判 0。
证据：smoke-5 stderr 末行 "Stream ended unexpectedly. Partial response preserved."、
model.patch 0 字节、reward F2P 0/88（P2P 275/275）。
**语义裁定**：异常 EOF 的截断生成不是完成的回答；对 mid-work agent 按「完成」收尾是
错误语义。修复 A8b：`is_stream_interrupted()`（类型级）并入续推触发，预算耗尽后仍回落
保全路径；Ollama malformed 保全路径与正常 text-only 完成不受影响。
**「假成功」分级**：F2 是「假失败」（作废成果，infra 噪声）；F2-b 更危险——「假成功」
（rc=0 但任务未完成），对真实用户同样是直接危害（agent 停在半路还说完成了）。

### F5（A8/A8b 生产验证，smoke-6）：同题从 0 分 infra 死亡到 86/88 近满解
smoke-6（59m52s，bandit 同题第 6 次运行）：A8 在生产中真实触发一次
（`Turn LLM call interrupted (upstream cutoff); continuing turn 1/2`，NDJSON 有续推
Progress 事件），run 存活走完全程；交付 50706 字节真实 patch 并 commit；官方 verifier
判 F2P **86/88（97.7%）**、P2P 275/275、partial 0.994；binary reward=0（2 个 CLI
旗标优先级语义测试未过——模型能力层失败，非 infra）。
**对照**：同题 smoke-2/4/5 分别死于上游切断（×2）与静默断流（×1），均为 0 分。
**结论**：闸门 1 通过；A8+A8b 是本周期首个有对照证据的产品改进。
bandit 题 2 个 CLI 语义测试失败归因（模型 vs 提示）入 P3 分析。

### F3（产品缺陷候选，待定夺 P3）：stderr turn 计数非单调
smoke-2/4 中 stderr `turn N` 显示 7→5、12→11 回退。疑压缩/重试后的显示口径问题。
C1 修复（`55c980fd`）只修了 events.jsonl 的 turn 字段。影响：轨迹可读性与分析准确性。
动作：P3 阶段核对 stderr 计数来源与 compaction 交互。

### F4（已澄清，无需修）：默认超时 300s 与看门狗 420s 不矛盾
起初误判矛盾；实读 `client.rs:101-123` 后确认 timeout_seconds 语义是 **read_idle**
（字节空闲界，流式无总超时，PERF-2 设计），300s 字节死判定 < 420s 内容死判定，分层合理。
smoke-2 死因是上游切断（F2），非本地超时。

## 一·B、闸门 2（Go/TS/Rust 三语言并行，1h49m，2026-09-18）

| 任务（语言） | 结果 | 备注 |
|---|---|---|
| fd-deterministic-multi-key-sorting（Rust） | F2P 42/43，P2P 109/109，partial 0.993 | 差 1 个隐藏边缘测试（自然排序前导零平局 `file007` vs `file7`） |
| anko-typed-variable-bindings（Go） | F2P 8/9，P2P 1.0 | 差 1 个隐藏边缘测试（TestTypedBindingsAdditionalRepresentativeFlows） |
| cliffy-config-file-parsing（TS） | F2P 0/…，P2P 1.0，NonZeroExit | 切断风暴：A8 续推 9+ 次延长 3 倍寿命后预算终耗尽 → 空 patch |

数据：`~/.shannon/eval/deepswe-smoke/jobs/deepswe-gate2/`。

**发现 F6（竞争力杠杆，P3 主议题）：「最后一英里」缺口**。已判分的 4 个任务
（bandit/anko/fd + smoke-6）中 3 个是**差 1-2 个隐藏边缘测试的近满解**
（partial 97.7-99.4%），但 DeepSWE 榜为二值口径（reward=1 要求 F2P 全过）→
binary 全记 0。缺口不在脚手架（长程存活/工具/commit 全部工作），在**模型对
held-out 边缘语义的覆盖**：held-out 测试对 agent 不可见，无法针对性测试，
mini-swe 同样受此约束（官方 63% 即在此口径下）。P3 要回答：近满解 → 满解的
残余缺口里，有多少可由通用改进挽回（如更彻底的指令语义自查、实现后的
自我边界测试习惯），有多少是纯模型能力上限。

**发现 F7（P2 运营参数）**：切断重任务（cliffy 类）单题可烧 3h 墙钟与大量续推；
A8 预算按轮生效设计正确（cliffy 跨 9+ 次切断存活）。P2 的 113 题 3 并发下
此类任务拉长尾部但被 3h 硬顶约束；infra 分离口径需把「续推耗尽死亡」单独标记。

### F8（发车事故，已处置）：wave-1 跑在旧二进制上，7 trial 全部作废
**现象**：wave-1 发车命令漏 export `SHANNON_PIER_BIN`，deepswe-wave.sh 按旧仓库约定
回退到主 checkout 的 9-15 构建（无 char-boundary 修复、无 A8/A8b）。前 7 个完成 trial
全部 reward=0：kombu 复现 engine.rs:625 char-boundary panic（旧代码行）；expr/sqlfmt/
tengo 死于超时且 A8 触发 0 次（A7 两次耗尽）——全部为无效测量。
**处置**：停波 + 隔离 `deepswe-base-w1-stale-binary-invalid/` + 根因修复——
deepswe-wave.sh 默认二进制改为**本 worktree 构建**（SCRIPT_DIR 相对路径），发车时打印
`[wave] anchor binary` 与 `shannon --version`；波次重发。
**教训固化**：评测二进制锚点必须是**发车工具的默认行为**而不是发车人的记忆
（smoke-3 教训的工程化）；波次发车后第一个 trial 的 agent-info 版本号必须核验。

### F9（基线连翻三次车后的干净重启，2026-09-18）
w3 波次 40 trial 全部 RuntimeError DNF——根因 **docker 网络地址池耗尽**
（`all predefined address pools have been fully subnetted`）：被杀波次（w1/w2/gate2）
残留的死 compose 网络 + 本机其它项目把默认池（~31）耗光；与镜像预拉无关。
处置与固化：
1. `docker network prune`（30→19）+ 删残留 egress-proxy 容器；原 pier 进程自愈继续。
2. 用户指示并发回退 3（本机其它任务共存）→ w2 弃用。
3. resume 与原进程混跑产生重复 trial → 杀 resume、清确认的孤儿（dynamodb 第三次
   attempt）。
4. 原 pier 最终在结算一个 trial 时 FileNotFoundError 崩溃（被杀 resume 的账目残骸）
   → w3 整体弃用（0 有效判分）。
5. **干净重启 = `deepswe-base-w4`**（3 并发、worktree 二进制，进程环境/config/容器
   三重验证），**新增网络守护**：每 10 分钟检查，>24 自动 prune（仅清无主网络，
   对运行容器安全）。镜像预拉脚本按用户指示停用（避免与本机其它任务争用）。
**教训**：长时间无人值守波次的三件事——①发车工具默认值即锚点（F8）；②kill 波次
必须收尾（compose down + network prune），否则网络/容器泄漏会毒化下一波；③resume
与原进程绝不能共存于同一 job（lock 只防配置漂移，不防双进程）。

## 二、基线波次记录（P2）

- **wave-1 处置与提速改版（2026-09-18）**：首发的 wave-1 在旧二进制事故（F8）后以
  3 并发重启，跑出 0 完成/12 启动后做并发提速改造：实测 3 并发下 **429 限流 0 次**、
  本地资源大量闲置（load 4.5/33G 空闲）→ 瓶颈是单题思考时长而非本地资源或账号窗口，
  **并发提到 6**（预期 2.4 天 → ~1.2 天）。pier 的 job lock 对 config 指纹校验，
  改并发不能 resume（FileExistsError）→ 旧波次 0 完成无保留价值，弃用
  （`deepswe-base-w1-killed-at-6concurrency-attempt/`），以 **`deepswe-base-w2`**
  6 并发全新发车（进程环境已验证 SHANNON_PIER_BIN=worktree 构建）。
- **并发定版（2026-09-18）**：6 并发版（w2）因**本机有其它任务、避免资源撞车**按用户
  指示回退 3 并发；w2 弃用（0 完成，`deepswe-base-w2-killed-concurrency-revert/`）。
  **最终波次 = `deepswe-base-w3`**，3 并发全新发车（进程环境 + config 双重验证：
  SHANNON_PIER_BIN=worktree 构建、n_concurrent=3）。镜像预拉（ensure-deepswe-images.sh）
  继续在后台与跑批重叠。预计墙钟 ~2-2.5 天。
  同并发恢复：`PYTHONPATH=scripts/eval/pier-adapter SHANNON_PIER_BIN=<worktree 二进制>
  pier job resume --job-path ~/.shannon/eval/deepswe/jobs/deepswe-base-w3`。
  聚合：`JOBS_DIR=~/.shannon/eval/deepswe/jobs scripts/eval/deepswe-wave.sh aggregate
  --job-name deepswe-base-w3`。
  **resume 分支两处修复**：`--job-path` 传参 + adapter 环境注入。
  发车前 preflight 4/4。
  中断恢复：改并发须重建 job（lock 指纹）；同并发恢复
  `PYTHONPATH=scripts/eval/pier-adapter SHANNON_PIER_BIN=<worktree 二进制>
  pier job resume --job-path ~/.shannon/eval/deepswe/jobs/deepswe-base-w2`。
  聚合：`JOBS_DIR=~/.shannon/eval/deepswe/jobs scripts/eval/deepswe-wave.sh aggregate
  --job-name deepswe-base-w2`。

## 三、改进 backlog（P3 正式化，P4 执行）

| # | 项 | 性质 | 状态 |
|---|---|---|---|
| A8 | turn 级流死亡续推重试 | 真实能力 | ✅ `a5baf851`（提前实施，理由见 F2） |
| A9 | stderr turn 计数非单调核对 | 观测性 | 候选（F3） |
| – | （P2 失败分析后按证据扩充；每项过「真实用户受益」+ L4 两道闸） | | |
