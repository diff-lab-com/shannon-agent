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

### F10（w4 中期损耗分类，8/113 判分，2026-09-18 18:22）
w4 发车 ~4.5h：11 启动 / 8 判分 / 3 在跑（健康：pier 存活、网络 23 稳定、watchdog 静默）。
二值 **1/8 resolve**（happy-dom，F2P 14/14 满分）。损耗三分：
1. **近满解（2）**：kombu 71/76（95%）、tengo 22/23（96%）——patch 已提交，差隐藏边缘
   测试（F6 模式延续）。
2. **turn 耗尽（2）**：arcane（82 测试大任务）、dynamodb——150 turns 用尽 exit=2，
   A8 未触发/仅 1 次（非流故障，是轮次经济学问题）。候选：轮次效率剖析（P3）。
3. **切断风暴（2）**：expr（A8 续推 8 次=4 个 turn 各 2 次）、sqlfmt（6 次）——切断
   密度超出每轮预算 2 的吸收能力，最终一轮超预算 fatal。候选：SHANNON_TURN_RETRIES
   调高属运营参数，但**不在基线波内改**（保持单 anchor 一致性），进 P4 验证后随
   P5 复测生效。
注意：expr 的 partial=0.999 系 verifier p2p_total=66265（断言级计数）对空 patch 的
虚胖，二值口径不受影响。
**当前节奏**：~4.5h 完成 8 题（含前期治愈期），稳态约 3 题/LST~2h → 全量预计
**3 天上下**；cutoff 风暴题是主要墙钟税。

### F11（锚切换：评测改用 minimax/MiniMax-M3，2026-09-18 用户指示）
1. **工具链适配（已完成）**：wave 脚本 provider/key 参数化（`EVAL_PROVIDER_MODEL`
   /`EVAL_KEY_FILE` 环境变量）；adapter 白名单按 provider 映射
   （minimax→`api.minimax.chat`）+ `SHANNON_ALLOWLIST_DOMAINS` 覆盖。
   模型 id 为 **`MiniMax-M3`**（大小写敏感，batch-5 RCA：错误 id 48%→4%）。
2. **A13 阻断 bug（发现于 minimax 冒烟，修复实施中）**：输出 token 截断续写路径把
   user 续写提示插在 assistant(tool_calls) 与其 tool results 之间 → 非法 wire 序列；
   minimax 严格校验 → 400 (2013 "tool id not found")。27 次工具调用后死亡。
   GLM/Anthropic 宽容此序所以此前未暴露。修复方案：根因（截断路径先执行工具再插
   提示）+ 防御消毒器（OpenAI wire 序列化出口清孤儿 result/补悬空 call）。
   证据：`deepswe-smoke-mm1/`（events.jsonl 已上传，全文件无该 id 的 tool/call 事件）。
3. **对标口径重定义**：Datacurve 官方榜（21/28 models）**无 minimax 条目** →
   「与官方同模型 harness 分差 = scaffolding 损益」的方法不适用。新口径：
   ①绝对分 vs 榜单全字段（跨模型对比，未来若提交上榜即直接可比）；
   ②内部同 scaffold 跨模型 A/B（bandit 同题：GLM smoke-6=86/88 vs MiniMax-M3）；
   ③历史参照：shannon+minimax-m3 旧引擎 SWE50=33/50（batch11，仅方向参考）。
4. **切换节奏**：w4（GLM 口径）保持运行至 minimax 冒烟通过——同题 A/B 数据 +
   GLM 部分基线（9/113）都不浪费；A13 修复 → 二进制重建（含 A10/A12/A13）→
   minimax 冒烟复跑 → 通过则停 w4、发 minimax 基线波次（w5）。
5. minimax 运营注意（历史 RCA）：per-minute 限流窗口（batch-6 无 pacing 时 76% 拒绝）
   ——并发 3 下实测暂无限流；think-only 退出高发（A1 即为其建）；输出 token 截断
   高发（A13 触发源，(1/5) 续写机制已存在）。

### F12（minimax 冒烟定案 + 基线发车，2026-09-18）
1. **A13/A13-c 修复验证**：mm3（bandit 同题第三次）**2013 零出现**，124+ 次工具调用
   顺畅执行——阻断解除，harness 对 minimax 全链路验证通过
   （install/prompt/egress/tools/verifier/telemetry/sessions 上传全绿）。
2. **A10 生产触发**：mm3 打满 250 轮时 wrap-up nudge 正确出现
   （"final turn (250/250)"），验证收工协议机制生效。
3. **新发现（模型行为，P3 议题）**：MiniMax-M3 在 shannon scaffold 上呈
   「探索-测试循环」风格——250 轮中 Bash 139 / Read 50 / **Edit 仅 2** / 从未 commit
   （112 条去重命令、8 次 pytest），13m48s 烧完全部轮次（~3.3s/轮，无长思考，
   速度约为 GLM 的 20 倍）。同题 GLM smoke-6 为 86/88（实现主导）。
   归因待 P3：模型风格 vs scaffold 提示（R1 correctness-first/A2 完成门禁）与
   minimax 反应型风格的交互。A10 的单轮降落对不主动 commit 的模型不够——
   「wrap-up 后仍不 commit」的行为差异已记录。
4. **运营**：minimax 250 轮仅 13m48s + ~$0.4 → 墙钟不再是主要约束，轮次预算才是；
   3 并发下暂无限流（持续监控）。
5. **w5 发车（2026-09-18 晚）**：`deepswe-base-w5`，113×n=1，3 并发，锚 =
   minimax/MiniMax-M3 @ api.minimax.chat，二进制 = worktree 构建（000619cb，含
   A8/A8b/A10/A12/A13-c），250 轮口径。进程环境三重验证（provider/二进制/容器）。
   恢复：`EVAL_PROVIDER_MODEL=minimax/MiniMax-M3
   EVAL_KEY_FILE=$HOME/.shannon/credentials/minimax.json PYTHONPATH=scripts/eval/pier-adapter
   SHANNON_PIER_BIN=<worktree 二进制> pier job resume --job-path
   ~/.shannon/eval/deepswe/jobs/deepswe-base-w5`。
   聚合：`JOBS_DIR=~/.shannon/eval/deepswe/jobs EVAL_* 同上 scripts/eval/deepswe-wave.sh
   aggregate --job-name deepswe-base-w5`。

### F13（w5 弃用 + w6 停车：minimax Token Plan 配额耗尽，2026-09-18 深夜）
1. **w5 限流 massacre**：47 个启动 trial 中 44 个 exit=4（限流），仅 1 个非空 patch
   （kombu 74/76 near-solve，证明不受限流时模型有能力）。3 并发对 minimax 不可行
   （w5 归因：每分钟窗口 + 配额临近耗尽），`deepswe-base-w5-ratelimit-massacre/` 归档弃用。
2. **w6 串行发车 → 停车**：串行 n=1 重发后实测仍全面 429——sqlfmt（唯一产出日志的
   trial）**12 次 429、0 次成功工具调用**；停车后冷却 2 分钟，单发最小探针连续 2 次 429。
   429 响应体定论：**「已达到 Token Plan 用量上限：请升级 Token Plan 套餐或购买积分补充
   用量 (2056)」**——账号级配额耗尽，非限流窗口，等待不恢复。`deepswe-base-w6` 保留
   （11 个 error trial：8×RuntimeError=registry 瞬断、2×NonZeroAgentExitCodeError=429
   杀死 agent、1×CancelledError=停车；resume 时按 exception_type 清除重跑）：
   `EVAL_PROVIDER_MODEL=minimax/MiniMax-M3 EVAL_KEY_FILE=$HOME/.shannon/credentials/minimax.json
   scripts/eval/deepswe-wave.sh start --include '*' --job-name deepswe-base-w6 --concurrency 1
   --resume --filter-error-type RuntimeError --filter-error-type NonZeroAgentExitCodeError`
   （待 key 恢复后执行；`--filter-error-type` 可多次传，匹配 trial 内 result.json 的
   exception_info.exception_type，命中即 rmtree 重跑；默认只清 CancelledError）。
3. **运营插曲（已自愈）**：w6 发车初期 docker.io registry 瞬断（`ubuntu:24.04` 拉取
   `failed commit on ref`），约 10 个 trial 快速死于 egress-proxy 镜像构建；手动 `docker pull`
   验证已恢复，磁盘/网络/daemon 无恙。教训：crash-loop 期 wave 不自愈（pier 逐题试错），
   发现「trial 目录 mtime 连续且 agent 目录空」即应停车查因。
4. **影响与决策点**：minimax 锚（F11）在用户升级 Token Plan / 充值 / 换 key 前不可用。
   切换机制本身已双向验证（GLM=w4+smoke 系、minimax=mm3+w6 发车链路），
   `EVAL_PROVIDER_MODEL`/`EVAL_KEY_FILE` 一组环境变量即完成切锚；job 内不混模型
   （pier lock 指纹 + 方法学要求），切模型 = 新 job。GLM 基线（同二进制、250 轮口径）
   随时可发，minimax 恢复后补跑即可形成内部 A/B。

### F14（切回 GLM 锚：w7 基线发车，2026-09-19 用户指示）
1. **决策**：minimax 配额耗尽（F13）期间，基线切回主锚
   zhipu-coding-plan/glm-5.3-flash 继续推进；minimax 恢复后补跑形成内部 A/B。
2. **w7 发车**：`deepswe-base-w7`，113×n=1，3 并发（w2/w3 验证档，GLM 限流 0 命中），
   **250 轮口径（A11）**，二进制 = worktree 构建 000619cb（含 A8/A8b/A10/A12/A13-c）。
   后台任务 exec_eb6935d5，网络 watchdog（>24 修剪）同窗运行。
3. **发车核验（全绿）**：3 trial 容器 + egress proxy Up；容器内 `shannon --version`
   = 0.11.0 且 pier 进程 `SHANNON_PIER_BIN` 指向 worktree 二进制（锚完整性链闭合）；
   **L3 断言通过**（start 事件 prompt 字段 3068 字符）；首 trial 5 分钟内推至 turn 36
   /61k tokens、43 次工具调用成对、0×429——GLM 连通性与 A8 续推环境正常。
4. **口径关系**：w4（GLM 10 题，150 轮 + 旧二进制）保持冻结为历史数据
   （F6 last-mile 分析 + 新旧口径对照用）；**正式 GLM 基线 = w7**（250 轮 + 当前
   二进制），预计墙钟 2-4 天。聚合：`JOBS_DIR=~/.shannon/eval/deepswe/jobs
   scripts/eval/deepswe-wave.sh aggregate --job-name deepswe-base-w7`。
   同并发恢复：`EVAL_PROVIDER_MODEL=zhipu-coding-plan/glm-5.3-flash
   EVAL_KEY_FILE=$HOME/.shannon/credentials/zhipu.json PYTHONPATH=scripts/eval/pier-adapter
   SHANNON_PIER_BIN=<worktree 二进制> pier job resume --job-path
   ~/.shannon/eval/deepswe/jobs/deepswe-base-w7`。

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
| A8b | 静默断流（异常 EOF）并入续推触发 | 真实能力 | ✅ `b512db61` |
| A10 | **轮次耗尽收工协议**：末 turn 前注入 wrap-up（完成当前工作/commit/总结），硬停变降落。证据：w4 arcane+dynamodb turn 耗尽死（exit=2），工作未落地=0 分；硬停语义对真实用户同样有害 | 真实能力 | 🔄 实施中（2026-09-18 批准） |
| A11 | **评测轮次口径对齐官方**：adapter `--max-turns` 150→250。官方榜 mini-swe-agent step_limit=250（+$3 成本上限），榜均 123 步、头部 268 步；150 卡在官方口径 60% 处，测的是轮次预算而非 scaffolding 质量。受 3h 墙钟天然封顶 | eval 口径 | ✅（`SHANNON_MAX_TURNS` 可覆盖；w4 口径 150 不动，P5 复测用 250） |
| A12 | **headless 默认轮次差异化**：产品默认 max_turns=20（types.rs:756）对交互合理（用户在场），对 headless 长任务是真实用户的坑（对照：Claude Code headless 默认无上限、mini-swe 250、OpenHands 100）→ headless 未显式时默认 100 | 真实能力 | 🔄 实施中（2026-09-18 批准） |
| A9 | stderr turn 计数非单调核对 | 观测性 | 候选（F3） |
| – | 轮次经济学遥测（per-turn 时长/token/工具分布）支撑 P3 效率归因 | 观测性 | ✅ `turn_economics.py` + adapter 上传 sessions 事件日志 |
| – | （P2 失败分析后按证据扩充；每项过「真实用户受益」+ L4 两道闸） | | |
