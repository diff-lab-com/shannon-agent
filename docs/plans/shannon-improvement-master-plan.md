# Shannon 综合改进实施方案（v1.4 · 终态导向 · 待审查）

- 日期：2026-08-27 ｜ 视角：产品 + 架构 ｜ 状态：**待 ed 审查**
- v1.4 变更：**W4 WASM 组件模型试点（4.16）暂缓**（DP3 复审更新）——任务细则保留备重启，不进本轮实施；其余 15 项任务不变。
- v1.3 变更：转为**终态导向**——无时间点/阶段/里程碑；只回答**做不做**（§2）与**怎么做**（§3→§6）。任务规模用相对复杂度（S/M/L）。
- 输入：六份调研文档（docs/research/，索引见附录）；执行派发见 [执行简报](shannon-improvement-execution-briefs.md)
- 一句话：以「统一事件日志」为地基，把 Shannon 建成**可观测、可评测、可扩展（含沙箱）、安全闭环**的引擎——终态判定见 §6，逐条可验证。

---

## 1. 终态愿景（目标成果全景）

全部工作完成后，Shannon 呈现以下终态：

**数据面——一条权威记录。** 每个会话从第一条消息到最后一条工具结果，完整落在 append-only 的 SessionEvent 日志（L0）上：请求信封（模型所见的一切）、token/成本/cache、权限与 hook 决策、turn 边界全部可查。任意会话可重放（replay 与现场渲染一致）、可对比（diff）、可导出；transcript/analytics/recording/单文件快照四套旧设施不复存在，analytics 降为 L0 派生视图；telemetry 成为 L0→OTLP 桥（缺省 NOOP，永不阻塞主循环）；落盘前统一脱敏，盘上即清洁。

**评测面——一套自证体系。** 一条命令（just eval）跑出结构化评测报告：内部任务集（20+ 题分层）+ 三个外部基准（Terminal-Bench、SWE-bench Verified 50 题子集、自建回归集 10 题），每题带成本（cost-per-resolved）、轨迹指标（循环/无效调用/权限阻塞）、失败分类（7 类）与 n=3 区间；失败样本自动归档可回溯；「Shannon 变好没有」从感觉变成数字。

**扩展面——一个统一模型。** 进程内：统一事件总线（emit/waterfall/parallel/serial 四模式，权限与 hooks 皆为总线上的内置节点）+ 能力 provider（FileSystem/Process/Sandbox 三接缝，工具零改码即可整体换执行世界，Landlock 沙箱可选开启）；进程外：MCP（主轴）+ manifest v2（兼容 plugin.toml v1 与 .claude-plugin 读取，安装期校验）。（WASM 组件试点暂缓，重启条件见 4.16。）

**安全面——闭环生效。** 插件 manifest 声明的六项权限在每个执行点真实强制（未声明保持现状宽松）；沙箱为 OS 级执行世界边界，与决策层权限叠加不互替；全部拒绝路径进 L0 可审计。

## 2. 做不做：范围与既定决策

### 2.1 差距 → 工作流映射（做）

| 差距 | 收口工作流 |
|---|---|
| P1 P2 P3 P8（=T1–T5、T8：快照非事件日志 / QueryEvent 不落盘 / 无因果链 / 无请求快照） | W1 trace |
| P7、T7（权限/hook 决策不入日志） | W1+W0 |
| T6（无脱敏） | W0→W1（最小集先行，完整版收尾） |
| T3（telemetry 空转） | W1 OTLP 桥 |
| P6、E1–E6（只测 harness 逻辑 / 断言浅 / 无成本 / 无失败分类） | W2 eval |
| P4 P5（五套扩展机制 / 能力无接缝——fs/process/sandbox） | W3 extension |
| 跨语言插件（TS/Go/C 进程内扩展路径） | **暂缓**（W4 WASM 试点，见 4.16） |
| P9（权威快照同步模型） | **暂缓**（L0+surface/replace 已覆盖大半价值；移动端同步需产品输入） |
| P10（面积治理） | 总验收护栏（§6.3） |

### 2.2 不做（Non-Goals）

不做整体 Cordis 化 / 运行时 TS-JS 脚本加载 / dylib ABI / dsh 宿主 shim；不承诺 dsh 行为型插件兼容；不拆微包架构；不动 desktop 存量 UI（唯一 UI 增量是 Turn Timeline 面板）；WASM 试点暂缓（4.16）；P9 暂缓。

### 2.3 既定决策（已拍板，作为范围约束内化进 §4）

| # | 决策 | 结论 |
|---|---|---|
| DP1 | 评测 API 预算上限 | **取消**——不设上限；仅保留工程护栏（单任务 max_turns/max_tokens/timeout 防挂死） |
| DP2 | Sandbox 接缝 | **做**——Landlock 后端，provider 装配，工具零改码生效 |
| DP3 | WASM 试点 | **暂缓**（v1.4 复审更新）——细则保留于 4.16 备重启；重启前置：W3 全部完成 + wasmtime 过 cargo-deny 预检 |
| DP4 | 旧记录设施退役 | **直接切换目标态**——不留双写期、不留旧格式读兼容、旧写路径随切换即删；升级即弃旧会话文件（破坏性变更已接受） |
| DP5 | 在线遥测姿态 | **默认关、匿名、只计数、不上报内容** |

## 3. 终态架构蓝图

~~~
┌─ 扩展面 W3 ────────────────────────────────────────────────────┐
│ manifest v2（tool/command/skill）→ 安装期校验 → 权限声明         │
│ 统一事件总线（Emit|Waterfall|Parallel|Serial，guard 可逆注册）    │
│   ├ 内置节点：权限守卫（waterfall 首节点）· hooks（30 事件）      │
│   └ 内置订阅者：L0 写者（分发与持久化一套 schema）                │
│ 能力 provider：FileSystem ┐ Process ┐ Sandbox(off|local|landlock) │
│ （暂缓：W4 wasmtime + WIT 组件试点，见 4.16）                    │
└──────────────────────────────────────────────────────────────┘
┌─ 数据面 W1 ────────────────────────────────────────────────────┐
│ L0 SessionEvent 日志（append-only，seq 连续，未知事件缺省拒绝）    │
│   ↑ tee 自 QueryEvent 流；request/header=adapter 序列化产物       │
│ 投影：会话恢复 / analytics 聚合 / trace show·replay·diff·export   │
│ 桥：OTLP（telemetry 重写，缺省 NOOP）；RedactionPolicy 写入时脱敏 │
└──────────────────────────────────────────────────────────────┘
┌─ 评测面 W2 ────────────────────────────────────────────────────┐
│ eval runner（任务 TOML → 沙箱工作区 → --prompt NDJSON → verify）  │
│ 指标/失败分类 ← L0；报告（JSON+md，n=3 区间，版本对比）            │
│ 三基准：Terminal-Bench / SWE-bench Verified 50 / 自建回归集 10     │
└──────────────────────────────────────────────────────────────┘
┌─ 安全面 W0 ────────────────────────────────────────────────────┐
│ PluginPermission 六字段执行点强制 + 越权测试常驻回归               │
│ 沙箱=执行世界边界（OS 级）；权限=决策层；叠加不互替                │
└──────────────────────────────────────────────────────────────┘
~~~

## 4. 任务分解与实施细则（五段式：目标/前提/约束/实施方案/验证标准）

> 复杂度 S/M/L 为相对量级，不含时长；「前提」即结构性依赖（次序由 §5 决定，与日历无关）；「验证标准」全部可执行，汇入 §6 总验收。

### 4.1 W1-P0a · SessionEvent 词汇表 v1 + append-only 日志写者（复杂度 M）

- **目标**：建立全体系唯一权威记录的词汇与写者：事件枚举 + 信封 + append-only JSONL 读写 + 损坏恢复。
- **前提**：差距 P1/P3 结论；DSH SessionEventMap 设计（research §4）作蓝本；recording/types.rs 的 RecordingEntry 作语义参照。
- **约束**：词汇放 shannon-types 新模块（类型层零 engine 依赖）；每事件带 kind 字符串，未知事件**缺省 required（拒绝）**，读取端显式 opt-in 才跳过；seq 由唯一写者分配且 = 已写入条数；日志子系统任何故障不得拖垮会话（降级为计数+告警）；遵守 edition 2024 / thiserror / expect() 惯例。
- **实施方案**：
  1. shannon-types 新增 session_event 模块：公共信封 ~~~seq, ts_ns, session_id, turn, step?, span_id?, parent_span_id?, kind, payload~~~；SessionEventKind 首版 18 个：session/start、session/end-seed、user/message、assistant/chunk、assistant/message（含 usage+interrupted）、tool/call（原始未解析参数）、tool/result（error+meta+duration）、request/header（EpochHeader：config 快照+adapter 默认+system+tools 清单含 schema hash）、request/context、permission/decision、hook/fired、turn/start、turn/end、todo/write、surface/append、surface/replace（start_seq/end_seq/source_event_seqs/reason）、error、custom（命名空间 payload）。
  2. shannon-core 新增 session_log 模块：SessionLogWriter（独占 append 句柄 + BufWriter；flush 策略：tool/result、turn/end、request/header 后强制，chunk 聚合每 50 条或 50ms）；SessionLogReader（流式逐行、未知 kind 报 UnknownEvent）；打开时校验尾行，半行截断到最近完整行并追加一条 error 事件告警。
  3. 存储路径：~~~/.shannon/sessions/<uuid>/events.jsonl~~~（每会话一目录，为投影与元数据留空间）。
  4. QueryEvent→SessionEvent 映射函数（为 4.2 的 tee 注入做准备，纯函数可单测）。
- **验证标准**：① 词汇 serde roundtrip 测试逐 kind 通过；② fuzz：随机交错 append 10k 次后读回 seq 严格连续；③ 截断恢复：写 100 行+半行 → 读回 100 事件+告警事件；④ 二次打开同文件写者被拒（独占测试）；⑤ just dev 全绿。

### 4.2 W1-P0b · EpochHeader 请求快照 + QueryEvent 落盘（复杂度 M）

- **目标**：正常会话（非录制模式）全量落 L0，实现「模型可见即已记录」。
- **前提**：4.1 完成；query_engine/types.rs 的 17 个 QueryEvent 变体已盘点。
- **约束**：本任务落盘后旧路径暂不动（直切由 4.6 执行）；日志开销预算 P95 turn 延迟增幅 <2%；对 TUI/SSE 消费方零行为变化（旁路 tee）；单事件 payload >256KB 截断（留头 64KB + sha256 + 原始长度），**request/header 例外必须完整**；密钥形态最小脱敏先行（env 密钥值、sk-/ghp_/xoxb- 等前缀模式），完整 RedactionPolicy 在 4.14。
- **实施方案**：
  1. query_engine 主循环单一注入点 tee：QueryEvent 广播的同时映射为 SessionEvent 写入（避免散落 hook）。
  2. 每 turn 开始写 request/header：直接取 adapter 序列化产物本身（不重新构造），保证与线上请求字节一致。
  3. Usage/TurnCompleted→turn/end（tokens/cost/cache 三项 token）；RateLimit→error（分类 rate_limit）。
  4. 开关 SHANNON_SESSION_LOG=off（默认 on），文档记录位置与清除命令。
  5. benches 新增 turn-log-overhead 基准，**改动前先取基线**。
- **验证标准**：① mockito 集成测试：会话结束后从 events.jsonl 重建请求信封与 mockito 捕获的实际 body 逐字节等价（时间戳除外）；② 现有 cli_e2e/scenario 全绿（零行为变化）；③ 基准达 <2%；④ 10 轮会话磁盘 <5MB。

### 4.3 W2-M1a · 断言词汇扩展（复杂度 S）

- **目标**：scenario 断言从 outcome-only 扩到行为级，为轨迹评测铺路。
- **前提**：testing/scenario.rs 现有 ValidationRule 7 种（FileExists/FileContent/FileNotExists/ExitCode/ToolCalled/ResponseContains/MaxDurationMs）已盘点。无其他依赖，可最先做。
- **约束**：YAML schema 向后兼容（新字段全 optional）；不引入新依赖。
- **实施方案**：ValidationRule 新增 4 种——DiffMatches{path, expected_diff_regex}（基于 FileHistoryManager 快照或 git diff）；TrajectoryContains{sequence}（工具调用子序列匹配：名字+参数模式）；ForbiddenTool{tool}；CostBelow{max_usd, per}。ScenarioResult 每规则独立 pass/fail + 轨迹摘要字段（供 runner 报告）。L0 就绪前轨迹数据源接 recording 产物，之后切 L0。
- **验证标准**：① 每条新规则 ≥1 正 ≥1 反 YAML 用例；② 现有 10 场景回归无变化；③ just scenarios 全绿；④ scenario.rs 模块文档更新。

### 4.4 W2-M1b · L1 任务集（20 题）+ eval runner（复杂度 M）

- **目标**：真实模型参与的分层任务集 + 一条命令跑出结构化评测报告。
- **前提**：4.3 完成；--prompt headless + NDJSON 可用；sandbox 机制可复用作任务隔离。
- **约束**：任务环境可重置（临时工作区），失败不得污染仓库；工程护栏内置（单任务 max_turns/max_tokens/timeout，超限记「超时/限额」类别）；报告机器可读（JSON）+人可读（md）双输出。
- **实施方案**：
  1. 任务清单 TOML（tests/eval/tasks/*.toml）：~~~{id, tier: read|edit|search|multi_step|recovery, prompt, setup{files, git_init}, verify{script|rules}, expectations{trajectory, forbidden_tools}, limits{max_turns, max_tokens, timeout}}~~~。
  2. runner（shannon-core/src/testing/eval_runner.rs）：准备沙箱工作区 → 子进程跑 shannon --prompt 采 NDJSON → 执行 verify → 汇总 report.json + report.md；输出落 ~~~/.shannon/eval/runs/<run-id>/~~~，每任务子目录保留现场。
  3. 20 题分布：read 3 / edit 5 / search 3 / multi_step 6 / recovery 3；每题带期望轨迹模板与禁止项。
  4. justfile 增 just eval（全量）与 just eval --task <id>。
- **验证标准**：① just eval 一条命令出双报告；② 人工抽查 3 题报告与现场一致；③ 连续两次 run 可 diff（含指标稳定性 sanity）。

### 4.5 W0-T0 · 权限强制验证测试（复杂度 S，一天时间盒）

- **目标**：端到端坐实/证伪差距 P7 的推断（manifest 权限声明大概率未在执行点强制），产出矩阵供 4.9 使用。
- **前提**：plugin/manifest.rs 的 PluginPermission 六字段（read_files/write_files/execute_commands/network/mcp_tools/llm_api）与 PermissionRuleChecker 已知。无其他依赖，可最先做。
- **约束**：**只写测试不改实现**；时间盒超时即按「有缺口」保守假设推进。
- **实施方案**：① 构造测试插件：manifest 声明 read_files=true、write_files=false、execute_commands=false，提供尝试写文件/执行命令的工具路径；② 断言写/执行被拦截（或确认未被拦截→缺口坐实）；③ 输出「权限字段 × 执行点」矩阵（生效/不生效/无对应执行点），补进差距分析附录。
- **验证标准**：测试可重复；矩阵结论三态明确；若发现已强制则回改差距分析 P7 表述。

### 4.6 W1-P1 · 决策入日志 + 直切目标态 + trace 子命令（复杂度 L）

- **目标**：L0 成为唯一权威并**直切**：旧记录设施（transcript 写路径 / analytics 写路径 / 单文件会话快照 / recording 采集路径）删除或改为 L0 派生；CLI 可查/回放/对比/导出。
- **前提**：4.1+4.2 完成；hooks/events.rs 30 事件与 analytics 8 事件类型已盘点；DP4 已决（不留兼容层）。
- **约束**：
  - 直切语义：会话持久化/恢复 = events.jsonl 唯一来源；恢复会话 = 读 L0 重建内存状态（物化快照仅作加速缓存，可随时从 L0 重建）；transcript/analytics 文件由投影器派生输出（analytics 保留为聚合视图，transcript 文件形态取消——由 trace show/export 取代）。
  - **破坏性变更明示**：升级后旧 sessions/&lt;uuid&gt;.json / 旧 transcripts、analytics 文件不再可读不迁移；CHANGELOG 与 README 写清（已按 DP4 接受）。
  - 对账只作为**开发期验证手段**：切换前跑一次性 golden 对比（旧路径 vs L0 投影），一致后删除旧路径，对账代码不留仓（转为一组固定快照测试）。
  - replay 与现场渲染一致率必须 100%；/rewind 行为零变化（仅 turn 边界口径对齐 turn/end）。
- **实施方案**：
  1. 事件扩充：permission/decision{tool, mode, rule_hit?, classifier_verdict?, allowed, elapsed_ms}（挂 PermissionManager 判定点）；hook/fired{event_type, handler, outcome, duration}；turn/start。
  2. 会话恢复改造：SessionState 加载路径改读 events.jsonl（投影重建消息历史/工具记录/usage 累计），删除单文件快照写路径；列表/搜索命令改走 L0 目录扫描 + 会话目录内 sidecar 元数据（从 session/start 与 end-seed 投影）。
  3. 投影器 session_log/projections.rs：L0→analytics 聚合 JSONL（保留该产物）；transcript 的全文搜索/统计功能改为 L0 上的索引函数。
  4. 删除：session_transcript.rs 写路径、analytics.rs 散点采集调用、recording 模块（其 LlmRequest 捕获语义已被 request/header 取代）——全部在读路径切换后的同一 PR 系列完成。
  5. shannon-cli 增 trace 子命令：show <session> [--turn N] [--tool X] [--permission]；replay <session>（复用 shannon-ui 渲染层，时间压缩）；diff <a> <b>（seq/kind/payload 摘要级）；export <session>（events+元数据打包，供评测与分享）。
- **验证标准**：① insta 快照：replay 渲染与现场一致；② golden 对比通过后旧路径删除，全测试绿（含恢复会话往返：写→进程退出→重进→状态等价）；③ trace 四子命令各有 CLI 测试；④ /rewind 现有测试零变化；⑤ just dev 全绿；⑥ CHANGELOG 破坏性说明落盘。

### 4.7 W2-M2 · runner 接入 L0 + 指标 + 失败分类（复杂度 L）

- **目标**：评测报告从「过/不过」升级为成本/轨迹/失败类型三维。
- **前提**：4.2（L0 有数据）+ 4.4（runner 存在）。
- **约束**：分类规则外置 TOML（不硬编码模型名）；循环判定严格化：同工具+参数哈希连续 ≥3 次；指标字段零缺失（缺数据即 bug）。
- **实施方案**：
  1. 指标提取器：消费 events.jsonl → per-task {tokens in/out/cache, cost_usd, turns, tool_calls, wall_clock, loops, invalid_calls（错误后原样重试）, permission_blocks}。
  2. 失败分类器 7 类（规则版先行）：指令误解/工具失败未恢复/上下文丢失/权限误拒/超时或限额/编辑冲突/模型上限；规则表 TOML，每类定义事件模式样例。
  3. 报告升级：成本矩阵 + 分类分布 + 与上一 run 的版本对比字段。
  4. 失败样本归档：失败任务 events.jsonl+分类 → ~~~/.shannon/eval/failures/<date>/<task>/~~~。
- **验证标准**：① 20 题全跑指标字段完整；② 分类抽查 5 例人工确认；③ 版本对比字段可与上一 run diff 出报告。

### 4.8 W3-1 · 统一事件总线（复杂度 L）

- **目标**：进程内分发收敛为一条带四模式语义的总线；权限判定与 hooks 迁入；与 L0 共用一套 SessionEvent 词汇。
- **前提**：4.1 词汇冻结；HookManager 与 PermissionRuleChecker 现状已知。
- **约束**：旧 QueryEvent mpsc 通道保留为兼容外观，TUI/SSE/desktop events.rs 消费方不改或浅改；行为等价以「现有 hooks/scenario/权限测试全绿」为证；分发性能每事件 <100µs 量级（基准）；注册返回 guard（Drop 注销，RAII 可逆效应）。
- **实施方案**：
  1. shannon-core/src/bus.rs：EventBus.dispatch(kind, payload, mode: Emit|Waterfall|Parallel|Serial)；Waterfall 为 next() 链，用于 tool/pre-execute 守卫与请求改写。
  2. 权限迁入：PermissionManager 判定改为守卫链首节点；permission/decision 由总线统一发（与 4.6 汇合）。
  3. hooks 迁入：30 个 HookEvent 映射为 Emit 订阅，HookManager 对外 API 不变。
  4. 落盘即订阅：L0 写者是总线的内置订阅者——进程内分发与持久化一套 schema。
  5. 切换策略：bus 并行旁路双分发 → 对账（N 千事件零差异）→ 切换 → 删旧分发路径。
- **验证标准**：① 双分发对账测试零差异；② 迁移后 hooks/scenario/权限测试全绿；③ 分发基准达标；④ guard Drop 后不再收事件的测试。

### 4.9 W0 · 权限强制修复（复杂度 M，条件任务）

- **目标**：闭合 P7——PluginPermission 六字段在执行点真实生效。
- **前提**：4.5 矩阵结论存在。
- **约束**：缺省语义不变（未声明=现状宽松），显式声明才收紧；只接执行点不重构权限内核；拒绝必须走统一错误+permission/decision 事件（记插件来源）。
- **实施方案**：按矩阵逐点接线——write_files→文件类工具执行前、execute_commands→bash 工具、network→MCP transport 层、llm_api→API client 层、read_files/mcp_tools 按矩阵缺口定；4.5 用例转常驻回归，每字段 ≥1 用例；补 manifest 权限语义文档（声明=允许集）。
- **验证标准**：矩阵全「生效」；越权用例绿；未声明插件行为与修复前一致（兼容测试）。

### 4.10 W3-2 · manifest v2 + 权限收口 + dump-config + 生态约定（复杂度 M）

- **目标**：统一扩展描述格式；安装期校验；配置可调试可解释。
- **前提**：4.8（权限有统一判定点）+ 4.9 完成。
- **约束**：向后兼容 plugin.toml v1 与 .claude-plugin/plugin.json 的**读取**（生态存量入口，与 DP4 的内部记录设施无关）；解析失败显式报错（修掉 registry.rs load_all 的静默跳过）；dump-config 输出需含每项来源标注；schema 预留扩展位（wasm 类型等，供暂缓项重启时启用）。
- **实施方案**：① v2 schema=v1+mcp 引用+hooks 订阅声明+permissions（映射 PluginPermission）+版本兼容范围；② 安装期 schema 与权限声明完整性校验；③ shannon --dump-config：输出层序树（内置→项目 .shannon/→用户全局→CLI overlay）；④ 生态约定：GitHub topic shannon-plugin + 三类样例模板仓（skill/command/tool）；⑤ 测试：v1/v2/claude 三格式解析矩阵 + 损坏 manifest 报错用例。
- **验证标准**：三类样例插件端到端装跑（clone→load→使用）；越权拒绝；dump-config 黄金快照测试。

### 4.11 W3-3a · FileSystem/Process 接缝化（复杂度 L）

- **目标**：bash/edit/read/lsp 等工具经 provider 抽象执行，执行环境可整体替换（沙箱与未来远程执行世界的共同地基）。
- **前提**：shannon-tools 工具实现盘点完成；sandbox.rs 现状作参照。
- **约束**：Tool trait 对外接口不变；设计验收=「两种实现可互换」；本地路径零额外拷贝（性能不回退）；provider 内嵌执行世界迁移点（bash/PTY/LSP 随 provider 走）。
- **实施方案**：① shannon-tool-interface 增 FileSystemProvider（read/write/edit/list/watch）与 ProcessProvider（spawn/pty+argv 包装钩子）trait；② LocalFs/LocalProcess=现有逻辑平移；③ 工具构造注入 Arc&lt;dyn Provider&gt;（经 ToolRegistry 装配）；④ ProcessProvider 的 spawn 接口显式暴露沙箱包装点（供 4.12 实现 SandboxProcess 装饰器）。
- **验证标准**：① 现有工具测试零变化全绿（行为等价）；② mock provider 注入测试证明工具不再直呼 std::fs/std::process::Command；③ grep 审计（工具 crate 内直呼点清零）+测试双证。

### 4.12 W3-3b · Sandbox 接缝 + Landlock 后端（复杂度 L）

- **目标**：沙箱成为可插拔 provider：默认本地直通不变，开启后 bash/edit/LSP 整体进入 Landlock 限制的执行世界，工具代码零改动。
- **前提**：4.11 完成（provider 注入点就绪）；现有 sandbox.rs 行为已盘点。
- **约束**：Linux 优先（Landlock 需内核 ≥5.13，运行时探测降级）；macOS/旧内核回退到现有 sandbox 策略或明确报「不支持」，不静默假沙箱；沙箱内行为与未沙箱一致性（允许的操作结果一致，被拒操作得到可理解错误）；策略可配（工作区可写、网络开关、只读路径白名单）。
- **实施方案**：
  1. SandboxProvider trait：policy{writable_roots, readable_roots, network: bool} + 包装 FileSystemProvider/ProcessProvider 的装饰器实现（SandboxedFs/SandboxedProcess）。
  2. Landlock 后端：rust.landlock crate（或 bindings 自写）；应用规则集 FS read/write/execute + network ABI（内核支持时）；规则在 provider 构造期安装，失败降级路径显式告警事件。
  3. 装配：profile/配置项 sandbox = off|local|landlock（缺省 off 保持现状）；开启后 ToolRegistry 用装饰后的 provider 组装工具。
  4. 与权限体系的关系：沙箱是**执行世界边界**（os 级），权限是**决策层**（模型行为级），二者叠加不互替代；permission/decision 照常记录，沙箱拒绝产生 tool/result error（分类 sandbox_denied）。
  5. 测试矩阵：off/local/landlock × {写工作区内, 写工作区外, 网络, 执行}。
- **验证标准**：① 配置切换 sandbox=landlock 后，bash/edit/lsp **零代码改动**进入沙箱（装配测试）；② Landlock 下写工作区外被内核拒绝的 e2e 测试（Linux 跑，非 Linux 跳过并标注）；③ off 模式与现状逐字节等价（回归快照）；④ 沙箱拒绝错误分类正确进 L0；⑤ 现有测试全绿。

### 4.13 W2-M3 · 外部基准三件套（复杂度 L）

- **目标**：Terminal-Bench、SWE-bench Verified 50 题子集、自建回归集 10 题可跑、分数可对外引用。
- **前提**：4.7 完成；--prompt NDJSON 稳定；预算不设限（DP1）→ 直接跑足 n=3。
- **约束**：只取子集不追全量；判据一律用基准原生（不自造）；对外引用必须附 n 与日期；模型与 harness 变更不得同 run（归因纪律）。
- **实施方案**：① Terminal-Bench adapter：官方任务集→runner 任务格式（prompt+verify 脚本直用），环境按其容器约定；② SWE-bench Verified：固定 50 题题号清单入仓（防漂移），判据=fail-to-pass+pass-to-pass；③ 自建回归集：从本仓 CHANGELOG/PR 历史缺陷提炼 10 题（issue 描述+验证脚本）；④ 报告三基准分列、n=3 区间、与 4.7 指标同表（cost-per-resolved）。
- **验证标准**：三基准各出首份报告；同版本重跑区间可解释（方差归因记录）；报告可版本间 diff。

### 4.14 W1-P2 · OTLP 桥 + RedactionPolicy 完整版 + desktop Turn Timeline（复杂度 L）

- **目标**：接 OTel 生态；脱敏完整化；desktop 可视化 turn 内部。
- **前提**：L0 稳定（4.6 直切完成）；telemetry.rs 现状（6 计数器+未用 endpoint/trace_export/metrics_export 字段）已盘点。
- **约束**：导出器缺省 NOOP、永不阻塞主循环（Pi 契约）；脱敏在**写入时**做（盘上即清洁）；UI 遵守 desktop 规范（Material Symbols、Inter、i18n en+zh-CN 同 commit、不 import lucide）。
- **实施方案**：① telemetry.rs 重写为 L0→OTLP 桥（opentelemetry crate）：span 由 parent_span_id 树折叠，metrics 由投影计数；既有配置字段生效，SHANNON_TELEMETRY 开关保留；② RedactionPolicy：env 密钥集+前缀模式+用户正则（~/.shannon/redaction.toml），替换 4.2 最小集；③ desktop 新增 Turn Timeline 面板：desktop 增 trace_timeline(session) command，消费 L0 投影；工具瀑布+token/成本累积曲线；④ Jaeger+Grafana docker compose 进 docs 供验收。
- **验证标准**：① Jaeger 见「会话→turn→工具」完整 span 树；② 注入密钥的会话盘上扫描无明文（自动测试）；③ Timeline e2e（VITE_MOCK_MODE）通过；④ desktop lint/vitest 全绿。

### 4.15 W2-M4 · 在线信号 + 版本对比看板（复杂度 M）

- **目标**：匿名计数回流 + 静态看板呈现版本趋势。
- **前提**：DP5 已决（默认关/匿名/只计数）；4.7 报告格式稳定。
- **约束**：不上报对话内容/文件路径/仓库名；默认关闭；文档明示开关与全部数据项；关闭态零外发。
- **实施方案**：① 计数事件：会话反馈（显式 👍/👎 或 CLI 反馈）、中断率、接管率（人工接管 turn 占比）、/rewind 使用率——全部聚合计数；② 传输复用 notifier/webhook 管道（opt-in endpoint），本地先落 analytics 投影；③ 看板：eval runs 目录生成静态 HTML（无服务端），版本×指标矩阵。
- **验证标准**：① 开启态抓包仅见聚合计数字段（测试断言无内容字段）；② 关闭态零外发断言；③ 看板可渲染历史 run 序列。

### 4.16 W4 · WASM 组件模型试点【**暂缓——不进本轮实施**】

> 状态：暂缓（DP3，v1.4 复审）。任务细则全文保留备重启；重启前置条件：4.11+4.12 完成且 wasmtime 过 cargo-deny 预检。以下为保留细则。

- **目标**：验证第三条扩展路径：非 Rust 工具以 WASM 组件在**进程内受限运行**。
- **前提（重启时）**：4.11+4.12 完成；4.10 已预留 wasm 插件类型位；wasmtime 选型过 cargo-deny。
- **约束**：试点限定 tool 型扩展；host 能力只能经显式 grant 映射到 provider（无裸 fs/net/syscall）；wasi 仅开最小面；延迟与 MCP stdio 对比记录（决策数据，非硬门槛）；试点结论可负。
- **实施方案（要点）**：WIT 接口（shannon:tool / shannon:host.{fs,process}）→ shannon-wasm 宿主（wasmtime + fuel/时限）→ TS guest 样例（jco/wasm-tools 构建）→ manifest v2 接线（plugin_type=wasm + grants）→ 对照基准 → 试点报告。
- **验证标准（重启时）**：WASM 工具可装可调；grant 外 fs 写/网络被拒；fuel 超限安全终止；延迟对照表落报告；cargo-deny/clippy/fmt 全绿。

## 5. 依赖与实施次序

~~~
可立即开始（无依赖）：4.3 断言词汇 · 4.5 权限验证测试
4.1 词汇表+写者 → 4.2 落盘 ─┬→ 4.6 直切+trace 命令 ─→ 4.14 OTLP/脱敏/Timeline
                            ├→ 4.7 指标/失败分类 ─→ 4.13 三基准 ─→ 4.15 在线信号
                            └→ 4.8 事件总线 ─→ 4.10 manifest v2
4.4 任务集+runner（依赖 4.3）─→ 4.7
4.5 ─→ 4.9 权限修复 ─→ 4.10（权限收口）
4.11 fs/process 接缝 → 4.12 sandbox/Landlock
（4.16 WASM 暂缓，不在依赖图内）
~~~

关键路径：4.1 → 4.2 → 4.7 → 4.13（评测闭环）与 4.8 → 4.10（扩展收敛）；4.6 直切是数据面收敛点；4.11 → 4.12 是接缝纵深。次序仅由依赖决定，不由日历决定。

## 6. 总体验收标准（终态判定）

全部工作完成的判定——逐条可执行，取代任何阶段性里程碑：

### 6.1 不变量（必须持续为真）
- 「模型可见即已记录」：任一会话可从 events.jsonl 重建请求信封，与实际发送逐字节等价（时间戳除外）——常驻集成测试。
- seq 严格连续、append-only；未知事件缺省拒绝（ignorable 机制显式 opt-in）。
- replay 渲染与现场一致率 100%（insta 快照）。
- 恢复会话往返等价：写→退出→重进→内存状态等价。

### 6.2 能力（必须演示通过）
- shannon trace show/replay/diff/export 四子命令可用且各有测试。
- just eval 一条命令产出 report.json+md：内部 20 题 + 三外部基准，含成本矩阵、轨迹指标、7 类失败分布、n=3 区间、版本对比。
- 插件权限矩阵六字段全部「生效」，越权用例常驻绿；未声明插件行为不变。
- sandbox=landlock 配置切换后 bash/edit/lsp 零改码进沙箱；Landlock 拒绝写工作区外（Linux e2e）；off 模式与现状逐字节等价。
- Jaeger/Grafana 见「会话→turn→工具」完整 span 树；注入密钥会话盘上扫描无明文。
- 在线信号关闭态零外发、开启态仅聚合计数（测试断言）。

### 6.3 工程护栏（全程不得回退）
- just test / just dev / CI 全绿；每源文件至少一测纪律不变。
- 日志开销 P95 turn 延迟增幅 <2%；单会话日志 <5MB/百轮。
- dead_code 注解计数不净增（每次引用前重数）。
- 新依赖（opentelemetry/landlock）全部过 cargo-deny。

### 6.4 交付物清单
- 代码与测试（4.1–4.15 全部）；manifest v2 权限语义文档；三基准首份报告；CHANGELOG 破坏性说明（旧会话文件弃用）；升级路径在真实 ~/.shannon 上的演练记录。（4.16 试点报告为暂缓项，重启时交付。）

## 7. 风险与对策

| # | 风险 | 涉及 | 对策 | 兜底 |
|---|---|---|---|---|
| R1 | 日志写入拖慢 turn | W1 | 开销基准进验收（<2% P95）；chunk 聚合 fsync | 异步缓冲档 |
| R2 | 磁盘体积增长 | W1 | 轮转+大载荷 truncate（hash+长度留头） | TTL 清理 |
| R3 | 事件词汇过早冻结 | W1/W3 | 词汇独立模块+ignorable 机制 | untagged 兼容窗 |
| R4 | 总线重构影响面大 | W3-1 | 旁路双分发→对账→切换 | 回退开关 |
| R5 | 隐私放大（全对话落盘） | W1 | 最小脱敏先行；完整版收尾；文档明示 | SHANNON_SESSION_LOG=off |
| R6 | 多处破坏性变更（会话格式、QueryEngine API） | 发布时 | 按既有 minor bump 流程（3 处版本位置）；可按完成度拆多个 minor | — |
| R7 | 直切伤及存量会话可用性 | W1-P1 | **已按 DP4 接受**：CHANGELOG 明示；发布前在真实 ~/.shannon 演练升级路径 | — |
| R8 | 真实模型评测 flaky | W2 | 任务沙箱化；flaky 隔离标记不混能力分 | n=3 报区间 |
| R9 | Landlock 内核/平台差异 | W3-3b | 运行时探测+显式降级告警；非 Linux 跳过并标注 | 回退现有 sandbox 策略 |
| R10 | 接缝抽象返工 | W3 | 「两种实现可互换」为设计验收；3a 先行 3b 验证 | 降级为 wrapper |
| R11 | wasmtime 体积/审计/组件模型演进 | 暂缓项 4.16 | 重启前置 cargo-deny 预检；WIT 契约自持；结论可负 | 继续暂缓 |
| R12 | 精力挤占常规交付 | 全局 | 产品火情优先；次序仅由依赖决定，可暂停续作不塌 | 任务间不锁死 |

## 8. 与周边工作的边界

post-v0.10.0 清理尾巴在开工前清完（缩 rebase 面）；desktop UI 现代化已完成（be22cc96），本方案 UI 增量仅 Turn Timeline；mobile/gateway 不在范围（L0 若供移动端同步即 P9，另行立项）；六份调研文档留 docs/research/ 本地互为索引。

## 9. 审查通过后的启动次序（先后即依赖，无时间承诺）

1. 清 post-v0.10.0 尾巴。
2. 4.5 权限验证测试（时间盒一天，矩阵直接进 4.9 计划）。
3. 4.3 断言词汇扩展 PR（独立，不等 trace）。
4. 4.1 SessionEvent 词汇表 PR（shannon-types，纯类型+roundtrip 测试，先冻结词汇再动引擎）。
5. 日志开销基准的改动前基线测量（first measurement）。
6. 其余任务按 §5 依赖推进；派发规程见 [执行简报](shannon-improvement-execution-briefs.md)。

---

## 附录 · 文档索引

| 文档 | 位置 | 引用 |
|---|---|---|
| DSH 深度调研 | docs/research/deepseek-harness-analysis.md | §1 愿景；W1 蓝本 |
| Pi 深度调研 | docs/research/pi-agent-analysis.md | §1 愿景；4.14 遥测契约 |
| Shannon 差距分析 | docs/research/shannon-gap-analysis.md | §2.1 映射源 |
| 插件架构评审 | docs/research/shannon-plugin-architecture-evaluation.md | W3 细则源；Non-Goals |
| Trace 改进方案 | docs/research/shannon-trace-improvement-plan.md | W1/W0 细则源 |
| 评测方案 | docs/research/agent-eval-landscape-and-plan.md | W2 细则源 |
