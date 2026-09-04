# Eval Findings — shannon × glm-5.3-flash（P2 分析，2026-09-05 进行中）

状态：**P1b 运行中**（SWE50×3，数据回填前本版先固化已有证据与 backlog）。
证据规范：所有结论附数据路径；glm SWE 列待 P1b 聚合后回填。

## 一、数据源

| 数据集 | 位置 | 状态 |
|---|---|---|
| P1a 回归池 n=3（glm） | `~/.shannon/eval/v2-glm-regression/`（3 run + aggregate） | ✅ stable 3 / flaky 1 / stable_fail 6 |
| minimax SWE 历史 8 批 | `~/.shannon/eval/swe50-batch*/`、`swe50-n3/` | ✅ 干净基线 b3=24/50 → b11=33/50 |
| minimax RCA 全集 | `~/.claude/projects/.../memory/swe-batch*.md` | ✅ 已与逐题数据交叉复核 |
| P1b glm SWE50×3 | `~/.shannon/eval/v2-glm-swe50/` | 🔄 运行中 |
| 单题 sanity | `~/.shannon/eval/v2-glm-sanity/` | ✅ astropy-12907 resolved=true（2.59M in tokens） |

## 二、核心发现

### F1（回归池，决定性）：修复能力没问题，死在「修完之后」
6 个 stable_fail × 3 轮 = 18 次运行中 **17 次 agent 在 1-2 turn 内写入了正确修复**
（file_content/script 判据几乎全过）。真正死因（按命中频率）：
1. **验证死亡螺旋**：系统提示明令"After writing code, run relevant tests to verify"
   （`crates/shannon-core/src/query_engine/types.rs`），但评测沙箱无 rustc/cargo/python3，
   模型烧尽剩余 turn 寻找工具链 → turn_limit → exit_code 规则一票否决。
2. **路径双重世界观**：Read/Edit 回显宿主绝对路径（`/home/ed/.shannon/eval/...`），
   Bash 沙箱内同一目录却是 `/workspace`（`shannon-tools/src/file/sandbox.rs:22`
   只做了单向 remap）——每轮浪费 1-3 turn，6 任务全中。
3. **Bash 安全器误拒**：`$(`、`${}`、反引号一律 critical 拒绝
   （`shannon-tools/src/system.rs:244`），`$(command -v shasum)` 这类只读探测也被拒，
   拒绝信息无整改建议 → 模型原地换马甲重试。
4. **预算错配**：edit 层 max_turns=6 零余量；timeout_secs=180 vs glm 实测单轮
   30-110s（reg_04 三连 timeout 全因此，修复本身 3/3 正确）。
5. **判分 artifact**：`trajectory_contains MultiEdit` 硬规则误杀正确单 Edit 修复
   （reg_01/03，与仓库 2026-08-28 RCA「tool choice is a means, not the contract」
   相悖）；`contains "merge"` 字面量检查误杀语义正确实现；recovery 层 strict 禁 Bash
   把「只读验证」当「作弊改文件」判死（reg_05 两轮语义全过仅死于禁令）。

### F2（glm 模型行为，实测）
- **单轮延迟 30-110s**（算法型生成），事件日志实测单次调用内部 312s 静默
  （服务端 keepalive 使 read_timeout=120s 失效）→ 30min 窗口被 3-5 次硬思考吃光。
- **验证冲动 + 预算无感**：修完不收工，reg_03 run2/3 证明其能 2-turn 收工但多数不做。
- **think-only 退出**（minimax 期即有，glm 待 P1b 回填）：响应无 tool_use 即正常结束，
  headless 当成功，空 patch。
- 小失误：Edit 缺 old_string、被拒后重试同构命令、测试垃圾写进工作区。

### F3（minimax 历史的杠杆结论）
- 6/9 批次分数被运行事故污染（限流/模型 id/限速缺失/消息序 bug/clap 拒绝）。
- 唯一统计显著能力跳变 b7→b8（+10 题，z=2.01）来自 **6 行提示**（容器内 python3-only
  提示 + 15 回合未 Edit 先提交）。b8→b11 的 +3 题不显著。
- batch11 的 17 失败 = **9 脚手架责任**（RL 事故 4、think-only 3、改测试文件 2）
  + 8 模型能力；理论上限 37-40/50。
- batch10 回落（30→22）确认是 Phase B 消息序 bug 的确定性回归（16 题同一回合同一错误码）。

### F4（成本口径）
`glm-5.3-flash` 不在静态模型目录，cost_usd 按 fallback 价目严重虚高
（sanity 单题报 $18.72）。token 数是唯一可信口径；catalog 参考价列为改进项。

## 三、改进 Backlog（P3 执行清单）

### A. 引擎/脚手架（shannon 代码）
| # | 项 | 证据 | 预期收益 |
|---|---|---|---|
| A1 | **think-only 续推 nudge**：响应无 tool_use 且非最终答复时合成续推消息 | minimax b11 3 题 + 理论上限 +4-8pp | SWE +2-4 题/批 |
| A2 | **系统提示验证指引**：受限环境先探测工具链；不可用则阅读推演验证、及时收工 | 回归池 6 任务全中 | 回归池 +3~5 stable_fail 转 pass |
| A3 | **路径别名回显**：工具结果回显沙箱可见路径（或在系统提示说明映射） | 同上，每轮省 1-3 turn | 同上 |
| A4 | **Bash 安全器分级**：含展开但无危险动词 → 警告放行；拒绝时给替代写法 | reg_01/05/07/09 | 同上 |
| A5 | **流停滞看门狗**：N 秒无内容 chunk 即 abort+重试（字节 keepalive 不算） | P1b 事件日志 312s 静默 | 消除 30min 窗口被单次挂流吃光 |
| A6 | 最终 turn 收工 nudge（复用 Phase B 双写机制） | b8 hint 先例 | 边际 |

### B. 评测设施（scripts/eval + 任务 TOML）
| # | 项 | 证据 |
|---|---|---|
| B1 | 回归池判分修正：删 MultiEdit 硬规则（reg_01/03）、"merge" 改语义 script、recovery 禁令改「按是否产生文件修改」 | F1.5 |
| B2 | 预算分档：edit 层 turns 6→10、timeout 180→600（按模型速度分档留口） | F1.4 |
| B3 | Write/Bash 沙箱根集合统一，拒绝信息列出可用根 | reg_05/09 |
| B4 | SWE hint 追加：禁止修改 tests/ 下文件；Edit 后跑相关模块不只跑目标测试 | minimax C1/C2 类 5 题 |
| B5 | 成本列：glm-5.3-flash catalog 参考价（或标注成本不可信） | F4 |

### C. 观测性（P2 尾声可选）
- events.jsonl turn 字段恒 1（turn 计数靠 request/header 数）→ 修复回写
- result.json 缺 verify_script rule_outcomes 行 → 核对回写路径

## 四、P4 验证计划（改进后复测）

### 4.1 快速验证结果（2026-09-05，n=1，改进后二进制 + v2 规则 + STREAM_IDLE_SECS=360）

运行：`~/.shannon/eval/v3-glm-regression-postfix/20260904225307-ee60175e`。
**回归池 3/10 → 6/10**；三个被救回的任务正是靶向任务：

| 任务 | P1a | postfix | 机制 |
|---|---|---|---|
| reg_01 | turn_limit×3 | **pass**（4t） | 判分修正 + A2 收工指引 |
| reg_03 | 0/3 | **pass**（2t） | MultiEdit 硬规则移除 |
| reg_09 | turn_limit×3 | **pass**（3t） | A4 安全器放行 + A3 路径别名 |
| reg_04 | timeout×3 | turn_limit（8t 用尽） | 不再超时；生成效率仍不足 |
| reg_05 | 0/3 | failed | recovery 禁令仍被 Bash 验证冲动违反 ×2 |
| reg_06 | flaky | failed（本轮） | 与 flaky 分桶一致 |

n=1 数字仅内部参考（引用规范不变）。剩余问题（进入下一轮 backlog）：
- **reg_04**：turn 效率——模型在该任务上轮数内未收工（思考延迟 + 全文件重写倾向）。
- **reg_05**：recovery 契约遵循——A2 提示的「探测工具链」措辞可能反向鼓励了 Bash 调用；
  需要「recovery/受限层完全不调用被禁工具」的显式分层指引（或 eval 提示注入声明约束）。

### 4.2 正式 P4 复测（P1b/P1c/P1d 完成后）
1. 回归池 n=3（改进二进制 + v2 规则）→ stable 分桶 vs v2 规则 + 旧二进制的对照
   （需补一次旧二进制 × v2 规则的 n=3，隔离判分修正贡献）
2. SWE50 n=1（改进二进制，STREAM_IDLE_SECS=360）→ 逐题对照 P1b
3. 靶题复放：think-only 三题（minimax b11：astropy-14508 / matplotlib-20676 / django-11066）
4. 引用规范不变：n / date / anchor（app_version + 规则指纹区分改进前后）
