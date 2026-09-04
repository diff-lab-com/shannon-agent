# multi_step 层全败 + edit_03/rec_03 稳定失败:根因分析与重设计提案

- 日期:2026-08-28
- 分析对象:glm-5.3-flash 基线(v1-official 三轮 n=3)中 8 个稳定失败题
- 性质:只读分析 + 提案,**不改题目、不跑评测、零 git 操作**
- 作者:eval 分析 agent(A4)

## 0. 数据源与判定机制(先读这节,后面所有论断依赖它)

### 0.1 数据源

| 数据 | 路径 |
|---|---|
| 三轮官方报告 | `~/.shannon/eval/v1-official/{20260827175908-6d21325f, 20260827180600-ece0f0dd, 20260827181226-e5f4516b}/<task>/`(每题含 `task.toml`、`result.json`、`stream.ndjson`、`workspace/`、`metrics.json`) |
| 复现对照 | `~/.shannon/eval/v1-fair/20260827172423-d63e0f2a/` 与 `~/.shannon/eval/ab-arm-{a,b}/20260827161031-*/`(同为 glm-5.3-flash;ab-arm-b 在 prompt 后追加中文"请直接改文件"提示) |
| 失败归档 | `~/.shannon/eval/failures/20260827/<task>/`(classification.json + 各 session 的 events-*.jsonl) |

三轮官方均为 12/20 通过,失败集完全一致:multi_01–multi_06 + edit_03 + rec_03。**这 8 题在 v1-fair 和两个 ab arm 中也全部失败**(跨 5 个独立 run、4 种 prompt 措辞),失败指纹(`failure_rules_fingerprint = dfcbe82fb06c6c87`)稳定。

### 0.2 trajectory_contains 匹配语义(关键机制)

实现在 `crates/shannon-core/src/testing/scenario.rs:681-765`(`check_subsequence`):

1. **有序子序列**匹配(`scenario.rs:287` 注释明示 "ordered subsequence")。
2. 每步先匹配工具名(`tool` 或 `tools` 候选族),`args_regex` 非空时对 **工具入参的紧凑 JSON 串**(`call.input_json`)做**无锚定正则** `is_match`(`scenario.rs:688-698`)。
3. **级联效应**:子序列指针 `si` 只有在当前步命中后才前进;若当前步因 args_regex 失配而永远无法命中,**后续所有步骤都会被报 "not found"**,即使它们实际发生了。

### 0.3 模型为什么发绝对路径(冤案的根源)

- 引擎在系统提示注入真实工作目录:`crates/shannon-core/src/query_engine/engine.rs:1352` — `"\n\n## Environment\n\nWorking directory: {cwd}"`。
- Shannon 自己的工具 schema 对 `file_path` 的描述是**"Absolute path to the file"**:`crates/shannon-tools/src/file/mod.rs:277,387,504`、`file/write.rs:12`、`file/multiedit.rs:24`。
- 于是**遵守宿主契约的模型必然发绝对路径**。glm-5.3-flash 三轮全部如此(例如 multi_02 run2 的 Write 入参:`"file_path":"/home/ed/.shannon/eval/v1-official/20260827180600-ece0f0dd/multi_02/workspace/CHANGELOG.md"`,见该题 `stream.ndjson`)。
- 而题面 `args_regex` 写的是相对路径精确子串,如 `'"file_path":"CHANGELOG.md"'`。**绝对路径里不存在这个子串**(前面有长前缀),必然失配。

### 0.4 为什么题目上架时没被发现

dry_run 预演的 stub 步骤直接用题面声明的相对路径输入(`crates/shannon-core/src/testing/eval_runner.rs:414-417`:"holds the same argument shape a real engine run would carry ... so trajectories and `args_regex` expectations stay realistic")。这个"同形"假设对**路径**恰恰不成立:预演用相对路径 → 预演绿;真实模型按 schema 发绝对路径 → 实跑红。**预演 gate 对这类缺陷零检出能力。**

### 0.5 失败自动归类的系统性误标

`failure_class = instruction_misunderstanding` 的判定条件是"failed + 无工具错误 + 无权限拦截 + 无循环"(见各 `result.json` 的 `failure_evidence`)。**verify 冤案恰好全部满足这些条件**,因此 8 个稳定失败里 6 个被标成"误解指令"——实际逐题核对为 0 例真实误解。自动分类在 matcher 缺陷修复前不具备参考价值。

---

## 1. 逐题分析

> 表例:每题给出「期望 vs 实际」「verify 杀点」「根因标签」「重设计提案」。根因标签:`prompt-歧义` / `verify-过严` / `verify-正确-能力不足` / `任务-设计缺陷`(允许多标签)。

### multi_01 — 模块重命名(multi 层,3/3 败;v1-fair 亦败)

**题目**:`utils` 模块改名 `helpers`,更新声明与所有使用点;`forbidden_tools = ["Write", "Bash"]`,期望轨迹 Edit/MultiEdit。

**期望 vs 实际**:
- verify 脚本只检查 `src/lib.rs` 无 `utils`、含 `mod helpers`;`src/app.rs` 含 `use crate::helpers::norm`;无 `.bak`。**不检查 `src/helpers.rs` 是否存在**。
- 实际 3/3 内容全对,且做得比"理想解"更完整:run3(最干净)先 `mv src/utils.rs src/helpers.rs` 再 Edit 两个文件(见 `20260827181226-*/multi_01/stream.ndjson`),workspace 最终态是**可编译的一致重命名**(`src/helpers.rs` 存在)。而按 verify 设想的合规解(只用 Edit 改文本),`lib.rs` 声明 `mod helpers` 但 `helpers.rs` 不存在——**产物是无法编译的 Rust workspace**。
- run1/run2 中模型多次 `mv` 失败(尝试了 `/workspace`、错拼路径等,`tool_error_unrecovered=3/5`),但最终仍自愈到正确内容;run3 零错误。

**verify 杀点**:`forbidden_tool: 'Bash' was invoked`(3/3)。内容规则 0 失败。

**根因**:`任务-设计缺陷`(完整、正确的解**需要**被禁的工具——Rust 模块改名语义上包含文件改名,而 Edit/MultiEdit 无法改名/建文件)。附 `prompt-歧义` 成分:题面 "Rename the module ... across this workspace" 被模型合理解读为含文件改名。

**重设计提案**(三选一,需 ed 拍板):
1. **解禁 Bash**(保留禁 Write):预期改后 3/3。风险:模型可用 `sed -i` 绕过"练 Edit"意图——但内容 verify 仍把关结果,且本层考察点是多步任务完成而非工具服从。
2. **改题为 inline module**:setup 放 `pub mod utils { pub fn norm(...) }` 于 `lib.rs` 内,改名变成纯文本编辑,与禁令自洽。预期 3/3。风险:改题语义,需重建 dry_run;这是语义变更,须 ed 审批。
3. 维持现状:则该题实为"测服从性",且与领域正确性冲突,**不推荐**。

### multi_02 — 生成 CHANGELOG(multi 层,3/3 败;全部对照 run 亦败)

**题目**:读 `NOTES.md`,在根目录建 `CHANGELOG.md`(`# Changelog` + 每条 bullet);期望第 2 步 `Write` 且 `args_regex='"file_path":"CHANGELOG.md"'`。

**期望 vs 实际**:3/3 内容规则**全过**(file_exists、3 条 contains、diff_matches 全绿),workspace 里 CHANGELOG.md 内容与 dry-run 理想解逐字一致。轨迹是最干净的两步:`Read NOTES.md → Write CHANGELOG.md`。唯一问题:Write 的 file_path 是绝对路径(stream.ndjson 逐字证据)。

**verify 杀点**:`trajectory_contains: step 2 ('Write' matching '"file_path":"CHANGELOG.md"') not found`(3/3,含 v1-fair、ab 双臂)。

**根因**:`verify-过严`(§0.3 路径冤案)。**零能力问题,纯 harness 缺陷。**

**提案**:args_regex 放宽为 `'"file_path":"[^"]*CHANGELOG\.md"'`,或干脆删掉该步的 args_regex(4 条内容规则已完整把关产物)。预期改后 3/3,零风险。

### multi_03 — 抽取 median 公共函数(multi 层,3/3 败)

**题目**:建 `src/math_utils.rs` 容纳 median,`stats.rs`/`report.rs` 改为调用它;期望 Write(math_utils.rs) + Edit/MultiEdit;禁 Bash。

**期望 vs 实际**:
- run2/run3:内容规则**全过**,连 `pub use crate::math_utils::median;` 的写法都与 dry-run 理想解一致(见 `20260827180600-*/multi_03/workspace/src/{stats,report}.rs`)。verify 脚本三条件(新文件有 median、stats 无本地 median、report 调 math_utils::median)全部满足。
- run1:被限流(`" Rate limited — the request will be retried automatically"` violation),模型仅 2 次调用(Grep/Glob)后会话终止,未产出——**基建截断,非能力失败**。

**verify 杀点**:`trajectory_contains` step1(Write 路径冤案)+ step2 级联失配;run1 另有 exit_code/verify_script 连带失败。

**根因**:`verify-过严` + 限流基建 flake(run1)。

**提案**:同 multi_02 放宽路径。预期改后 2/3–3/3(限流重跑则 3/3)。

### multi_04 — JSON→TOML 转换(multi 层,3/3 败)

**题目**:把 `config.json` 转成 `config.toml`;禁 Edit/MultiEdit/Bash;期望 Read(optional)→ Write(config.toml)。

**期望 vs 实际**:
- run1/run3:Write 出的 config.toml 内容全对(`retries = 3` + `[server]` 段,4 条内容规则全过)。随后模型**自发用 Bash 验证 TOML 合法性**(python3 `tomllib`、node)——沙箱里 python3/node 均不存在,连续报错(`tool_error_unrecovered=3/6`),模型还写了 `.verify_toml.js` 探测解释器、最后 `rm .verify_toml.js` 清理(行为良好)。
- run2:限流,0 次工具调用,整题报废。

**verify 杀点**:step2 Write 路径冤案 + `forbidden_tool: Bash`(run1/run3);run2 全部规则连坐。

**根因**:`verify-过严`(路径)+ `任务-设计缺陷`的边缘形态——禁令把"自我验证"这一良好工程习惯定为违禁;叠加**沙箱无 python3/node**,把模型的验证尝试变成 3–6 个 unrecovered error,进一步污染 failure_class。再加限流 flake。

**提案**:①路径放宽(同上);②Bash 政策见 §3-P3(解禁,或禁令降级为 soft expectation)。预期改后 2/3(限流重跑 3/3)。可选:给 Bash 沙箱预置 python3,让"验证型 Bash"能成功而不是报错堆栈。

### multi_05 — 新增 FAQ 页并登记索引(multi 层,3/3 败)

**题目**:建 `guides/faq.md`(`# FAQ` 开头段落),并在 `docs/index.md` 表格加 `| FAQ | guides/faq.md |` 行;禁 MultiEdit/Bash;期望 Write(faq)→ Edit(index)。

**期望 vs 实际**:3/3 内容规则全过(含 diff_matches 精确行)。run3 还展现了一次**自恢复亮点**:第一次 Edit 误用了幻觉路径(指向上一个 run 目录 `20260717215057-f31ea5c9-9d39`),被路径沙箱拦截后,模型改用正确绝对路径重试成功(见 `20260827181226-*/multi_05/stream.ndjson` 的 tool_result 序列:错误 → "Successfully replaced 1 occurrence")。

**verify 杀点**:两条 args_regex(`guides/faq.md`、`docs/index.md`)绝对路径失配 + 级联。

**根因**:`verify-过严`。

**提案**:路径放宽。预期 3/3。

### multi_06 — 端口双文件同步(multi 层,3/3 败)

**题目**:9100→9443,同时改 `src/net.rs` 与 `proxy.conf`;禁 Write/Bash;期望 Read(optional)→ Edit → Edit(proxy.conf)。

**期望 vs 实际**:3/3 内容规则全过;run2 的两个 Edit 与 dry-run 理想解逐字相同;改完还 Grep 自查 `9100` 残留(良好习惯,且 Grep 不违禁)。

**verify 杀点**:仅 step3(`Edit|MultiEdit` matching `"file_path":"proxy.conf"`)——注意 step2 因无 args_regex 被正常消费,所以本题只报一条 missing,是"级联"的对照样本: matcher 语义本身工作正常,**坏的只有路径 regex**。

**根因**:`verify-过严`。

**提案**:路径放宽。预期 3/3。

### edit_03 — 版本号原位升级(edit 层,3/3 败;v1-fair/ab 双臂亦败)

**题目**:`APP_VERSION` 由 `"3.1.4"` 改 `"3.2.0"`,别无他改;禁 Write/Bash;期望轨迹 `Edit|MultiEdit` 且 `args_regex='"old_string":"\"3\.1\.4\""'`——即要求 old_string **恰好等于**带引号的 `"3.1.4"` 七个字符。

**期望 vs 实际**:3/3 内容全过,含最严的 `diff_matches`(`-pub const APP_VERSION: &str = "3.1.4";` → `+... "3.2.0";` 精确两行 diff)。模型用的 old_string 是**整行** `pub const APP_VERSION: &str = "3.1.4";`(stream.ndjson 证据)——这是防误替换的常见且更稳的 Edit 用法,题面完全允许(题目只约束结果,未约束 old_string 粒度)。本题 1–2 次调用即完成,是全套件最省的题之一。

**verify 杀点**:仅 trajectory args_regex。

**根因**:`verify-过严`(对 old_string 粒度的过度指定,无任务学意义——同一 diff 两种粒度都"最小且正确")。

**提案**:args_regex 放宽为 `'"old_string":".*3\.1\.4'`(无锚定子串匹配,兼容两种粒度),或删 args_regex 保留 diff_matches 把关。预期 3/3,零风险。

### rec_03 — 合并冲突解决(recovery 层,3/3 败)

**题目**:`src/api_options.rs` 有冲突标记,要求两个函数都保留;禁 Write/MultiEdit/Bash;期望 step1 `Read` with `args_regex='"file_path":"src/api_options.rs"'`(**非 optional**)→ step2 `Edit`(无 regex)。

**期望 vs 实际**:3/3 内容全过(两函数保留、无标记行、无 .orig;run2 虽被限流导致 exit_code error,内容规则仍全绿)。模型 run1/run3 轨迹 `Read → Edit → Grep(自查) → Read(复核)`,教科书式恢复流程。

**verify 杀点**:step1 路径冤案 → **step2 级联误报**。报告里 "step 2 ('Edit') not found" 是假的——Edit 明明发生了,只是子序列指针卡死在 step1(§0.2 第 3 条)。这是级联放大效应最清晰的样本。

**根因**:`verify-过严`(+ matcher 级联放大)。

**提案**:路径放宽。预期 3/3。另建议 matcher 报告层只把**首个**未命中步骤报为真因、其余标 `(cascade)`(见 §3-P6)。

---

## 2. 横向模式

### 2.1 内容达成率 vs 判定通过率:系统性背离

8 题 × 3 官方轮 = 24 个 task-run 中,**内容规则(file_content/file_exists/diff_matches/verify_script)失败仅 1 次**——v1-fair multi_05,且该 run 同样带 "Rate limited" violation、exit_code=4、模型只发了 Read+`ls` 就被截断(result.json 逐字证据)。其余 23/23 内容全对。**multi 层不是能力层塌方,是判定层塌方。**

### 2.2 唯一的系统性 verify 缺陷:相对路径 args_regex

- 表现:12 处题面 args_regex 用相对路径精确子串;遵守 "Absolute path" 工具契约(§0.3)的模型 100% 失配。
- 波及:multi_02/03/05/06、edit_03(old_string 粒度,同类但不同字段)、rec_03(路径+级联)。**修复一处即可翻盘 6 题**。
- 修复后预期:12/20 → **16/20–17/20**(80–85%)。

### 2.3 第二杀点:forbidden_tools 里的 Bash

| 题 | Bash 用途 | 轮次 |
|---|---|---|
| multi_01 | `mv`(改名的必要步骤) | 3/3 |
| multi_04 | 自验 TOML(python/node,沙箱无解释器→报错堆栈) | 2/3 |
| search_01 | `grep -rn`/`find` 兜底自验 | 1/3 |
| edit_04 | (flaky 轮) | 1/3 |
| rec_02 | (flaky 轮) | 2/3 |

同一模型行为模式:**把 shell 当万能验证/兜底器**。真实工程里这是好习惯;在禁 Bash 的题里它一票否决,且常常连带把 unrecovered tool errors 拉高(沙箱没有 python3/node,验证尝试必然失败)。edit_04/rec_02 的"flaky"本质与此相同——模型有时用 Bash 自验、有时不用,通过率就随机抖动。这是**政策问题不是能力问题**:需要 ed 决策(§3-P3)。

### 2.4 限流基建 flake 侵蚀样本

"Rate limited" violation 共 4 处:官方 multi_03 r1(2 调用后弃局)、multi_04 r2(0 调用)、rec_03 r2(exit_code=4 但内容绿)、v1-fair multi_05(截断,唯一内容失败)。占 32 个相关 task-run 的 12.5%。重试机制未真正续命(stream 里只有一条提示语,会话直接终止)。**建议 runner 把「done.exit_code=4 + rate-limit 痕迹」判为 infra_flake 并自动重跑**,否则 n=3 的均值被基础设施噪声污染。

### 2.5 其他指标缺陷(顺带发现)

- **turns 恒为 1**:所有 task-run 的 `metrics.turns`/`turns_used` 均为 1(哪怕 16 次工具调用),题面 `limits.max_turns = 50` 形同虚设,事实上限只有 `timeout_secs`。
- **冤案烧钱**:失败题均成本显著高于通过题(通过题 ~$0.67;multi_01 r2 $3.23、multi_04 r3 $3.29)——禁令→报错→多轮修补的死亡螺旋。matcher 修复同时省钱。

### 2.6 tokens/turns 与失败的相关性

失败与 token 量**无**稳健相关(失败题 run 间 token 差 3–29 万,取决于 Bash 修补轮数);与"是否触发禁令/路径冤案"完全相关(24/24 可预言)。即:当前套件对 glm-5.3-flash 的失败是**可静态预测的判定产物**,不是能力测量。

---

## 3. 重设计提案汇总

| # | 提案 | 改动面 | 预期收益 | 风险 | 决策 |
|---|---|---|---|---|---|
| P1 | **matcher 侧路径归一**:`check_subsequence` 匹配时先试原始 `input_json`,失配再试"剥掉 workspace 绝对前缀"的归一串(只影响 path 类字段) | `crates/shannon-core/src/testing/scenario.rs`(`check_subsequence`)或采集处 | 一次修复 6 题;对未来所有题免疫 | 低:两次匹配取或,原串优先,零误伤 | 推荐,可直接采纳 |
| P2 | **题面 regex 放宽**:12 处 `'"file_path":"X"'` → `'"file_path":"[^"]*X"'`;edit_03 的 old_string regex → `'"old_string":".*3\.1\.4'` | `eval` 题面 TOML(改题面 verify 属于评测修正,不改任务语义) | 与 P1 双保险;不修代码也能解锁 | 无实质风险;缺点是每道新题都要记得 | 推荐,P1 落地前可先行 |
| P3 | **Bash 禁令政策**:方案 a) 全部解禁 Bash(内容 verify 兜底);b) 禁令降级为 soft expectation(违规标记 `over_expected` 式观察项,不判死);c) 保留硬禁但只禁**写类** shell(sed/tee/`>` 重定向) | 题面 `forbidden_tools` + 可能的 runner 支持 | multi_01、multi_04、search_01、edit_04、rec_02 全部去噪 | a 有"sed 作弊绕过 Edit 考察"风险(b/c 无);b 需要新增 soft 语义 | **需 ed 拍板**;个人推荐 b |
| P4 | **multi_01 重设计**:inline module 化(纯文本可完成)或解禁 Bash(见该题提案 1/2) | 该题 task.toml | 该题从"测服从性"变回"测多步编辑" | 改题语义,**须 ed 审批** | **需 ed 拍板** |
| P5 | **限流即 infra_flake**:检测 exit_code=4 + rate-limit 痕迹 → 标记并自动重跑一次,不进均值 | eval runner | 消灭 12.5% 的样本污染 | 低(仅多一次重跑成本) | 推荐,可直接采纳 |
| P6 | **报告可信度三件套**:① 级联标注(首个未命中步骤为真因,其余 `(cascade)`);② failure_class 增加 `verify_artifact` 类(内容规则全绿而 trajectory/forbidden 失败时不得归 instruction_misunderstanding);③ dry_run 预演把 stub 的 file_path 随机改成绝对路径,让此类题目在上架 gate 就红 | scenario.rs / eval_runner.rs | 报告不再系统性说谎;同类题目未来无法再溜进套件 | 低 | 推荐,可直接采纳 |
| P7 | turns 指标修复(或文档说明 headless 下恒 1、max_turns 无效) | eval_runner 指标采集 | 报表准确 | 无 | 可采纳(低优先) |

**修复后 glm-5.3-flash 画像预测**:12/20(60%)→ P1+P2 后 16–17/20(80–85%)→ P3/P4 落地后 18–19/20(90–95%)。

## 4. meta 结论:真实能力缺口 vs 评测缺陷

**8 个稳定失败 = 0 例真实能力缺口 + 8 例评测缺陷。**

- 纯 matcher 冤案(内容 3/3 全对):multi_02、multi_03(run1 为限流)、multi_05、multi_06、edit_03、rec_03 —— 6 题
- 设计缺陷(完整正确解需要被禁工具):multi_01 —— 1 题(且模型给出了比 verify 理想解更完整、可编译的产物)
- 政策误伤 + 基建叠加:multi_04 —— 1 题(内容全对;Bash 自验被禁 + 沙箱无解释器 + 限流)
- 唯一一次"内容失败"(v1-fair multi_05)同样是限流截断,不构成能力反证

因此:**这批数据不能用于给 glm-5.3-flash 的能力画像下结论,也不能用于模型横向对比**——trajectory_contains 的现状对"遵守工具 schema"的模型构成系统性反向选择(越守契约越冤)。模型在 24/24 个 task-run 中把工作做对了;真正的产出是这份套件缺陷清单。建议 matcher 修复后重跑一次基线,以修正画像并作为套件可信度的回归验证。

## 5. 建议 ed 决策点

1. **P1+P2 是否采纳**(路径归一 + regex 放宽)——可直接采纳,收益确定。
2. **Bash 禁令政策(P3)**——有争议:全解禁 / soft 化 / 只禁写类 shell,三案取舍需拍板;关联 flaky 题 edit_04、rec_02 与 search_01 的去噪。
3. **multi_01 重写(P4)**——改题语义(inline module)vs 解禁,需审批。
4. **限流重跑预算(P5)**——每 run 最多多花 1–2 个 task-run 的成本,是否接受。
5. **是否在修复后重跑 glm-5.3-flash 基线**——用于修正能力画像 + 验证套件修复有效性(建议:重跑,并保留本轮数据作为 matcher 缺陷的存证)。
