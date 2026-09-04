# Shannon Agent Trace 评审与改进方案

- 日期：2026-08-27
- 任务：评审当前 trace 相关模块是否合理，列出问题与改进点，给出可实施的 trace 改进方案
- 输入：[差距分析](shannon-gap-analysis.md)（P1/P2/P3/P8）、[DSH 调研](deepseek-harness-analysis.md)（§4 会话日志事件溯源）、[Pi 调研](pi-agent-analysis.md)（§6 遥测）
- 结论速览：**Shannon 目前没有「trace 系统」，只有五套互不连通的记录设施。** 合理演进路径不是修补 telemetry.rs，而是以 DSH SessionEvent 日志为蓝本建立**单一权威事件日志（L0）**，其余设施全部降级为日志的投影/桥接（L1–L3）。

## 1. 现状评审：Shannon 的「trace」到底有什么

| 设施 | 位置 | 内容形态 | 判定 |
|---|---|---|---|
| telemetry | crates/shannon-core/src/telemetry.rs | 6 个 AtomicU64 计数器 + tracing 层（target shannon::），SHANNON_TELEMETRY=1 显式开启；TelemetryConfig 里 endpoint/trace_export/metrics_export 字段**未被使用** | 有名无实：无 span、无导出、无因果 |
| session 快照 | shannon-engine state.rs | ~/.shannon/sessions/&lt;uuid&gt;.json 单文件全量序列化 | 持久化用的快照，非 trace：无过程、无请求、不可回放 |
| transcript | crates/shannon-core/src/session_transcript.rs | ~/.shannon/transcripts/&lt;session_id&gt;.jsonl；role+content+tool_calls | 会话级对话记录，粒度粗（无 turn 编号、无请求信封、无耗时） |
| analytics | crates/shannon-core/src/analytics.rs | ~/.shannon/analytics/&lt;date&gt;.jsonl；8 种事件 | 统计聚合用（工具成功率、耗时），无因果链、无载荷细节 |
| recording | crates/shannon-core/src/recording/ | 录制模式专用；SessionStart/UserMessage/LlmRequest/LlmResponse/QueryEvent/ToolCall/SessionEnd | **最接近 trace 的设施**（含请求 body、QueryEvent 变体），但常关、schema 独立、无 seq/因果键 |
| QueryEvent 流 | query_engine/types.rs（17 变体，含 Usage/cache tokens/TurnCompleted/ConversationUpdate） | 运行时 mpsc 广播 → TUI/SSE | **信息最全的流恰恰只存在于内存**，进程退出即蒸发 |

综合判定：**不合理——但不是「实现差」，而是「缺一个统一模型」。** 五套设施各自为政：schema 三套、时间戳精度不一、无贯穿 ID（仅 session_id 弱关联）、无权威版本。任何「这个工具调用当时为什么被放行」「上一轮 LLM 请求原文是什么」「这个 turn 花了多少 token/钱」的问题，现有设施最多答对一半。

## 2. 问题清单

| # | 问题 | 现象与证据 | 后果 |
|---|---|---|---|
| T1 | 无因果链 | 三套 JSONL 之间没有 turn/step/request 贯穿键 | 无法把「请求→工具→结果→下个请求」串成完整链条；跨文件对账靠猜时间戳 |
| T2 | QueryEvent 不落盘 | 17 变体含 Usage/cache_creation/cache_read/TurnCompleted，只进 TUI/SSE | token/成本/cache 命中等最有评测价值的数据全部丢失 |
| T3 | telemetry 空转 | 6 计数器 + 未使用的 exporter 配置字段 | 给人「有可观测性」的错觉；OTel 生态（Jaeger/Grafana）接不上 |
| T4 | 无请求级快照 | recording 有 LlmRequest body 但仅在录制模式；正常会话不存系统提示词/工具清单/参数 | 无法复现「模型当时看到了什么」；debug 提示词回归无从下手 |
| T5 | 无耗时分布 | 工具级 duration_ms 有，turn 级/阶段级（排队/首 token/流式尾延迟）无 | 性能分析只能靠 wall-clock 大数 |
| T6 | 无脱敏 | transcript 全文落盘 | 密钥/令牌一旦进入对话即持久化；外部导出（L3）会放大风险 |
| T7 | 决策不入日志 | 权限判定（rule/classifier/mode）、hook 触发、agent 调度不写任何 JSONL | 评测与审计无法回答「为什么放行」；见差距 P7 闭环缺口 |
| T8 | 快照非事件溯源 | session 单 JSON 重写 | 无法 replay/diff/分支；与 DSH 的事件日志形成代差（见 DSH §4） |

## 3. 改进方案：四层 trace 体系

### 3.0 设计原则（承自 DSH 的三条硬不变量）

1. **模型可见即已记录**：凡进入 LLM 请求的 system/tools/messages 与参数，必须能从日志完整重建（EpochHeader 快照）。
2. **seq 单调且连续**：SessionEvent.seq = 日志当前长度，由唯一写者分配；投影层禁止写日志。
3. **未知事件缺省拒绝**：新增词汇用 ignorable=false 标注；旧版本读到新事件必须显式声明可忽略才能继续。

### 3.1 L0：统一 SessionEvent 日志（核心交付物）

存储：沿用 ~/.shannon/ 目录约定，每会话一个 JSONL（append-only，独占写句柄；每次 append 后按策略 fsync——工具结果后强制、纯 chunk 可聚合）。事件词汇（首版映射）：

| SessionEvent | 来源（现有设施收敛） |
|---|---|
| session/start, session/end-seed | SessionPersistMetadata + recording SessionStart/End |
| user/message, assistant/chunk, assistant/message(含 usage+interrupted) | transcript + QueryEvent 流 |
| tool/call(原始未解析参数), tool/result(含 error+meta+duration) | transcript.tool_calls + analytics ToolExecution + QueryEvent Tool* |
| request/header(EpochHeader: config 快照+adapter 默认+system+tools 清单), request/context | recording LlmRequest（升格为常开） |
| permission/decision(rule/mode/classifier verdict/elapsed) | **新增**（补 T7） |
| hook/fired(type, handler, outcome) | **新增**（补 T7） |
| turn/start, turn/end(usage/cost/cache tokens/wall-clock) | QueryEvent Usage/TurnCompleted（补 T2） |
| todo/write, surface/append, surface/replace(start,end,sourceEventSeqs) | 会话树/摘要压缩投影（补 T8 的 replace 语义） |
| error(带分类) | analytics Error |

每条事件公共字段：seq, ts(纳秒), session_id, turn, step?, span_id?, parent_span_id?（因果链补 T1）。span 语义轻量化：不需要完整 OTel，只需 (parent, name, ts_start/ts_end) 三元组可重建嵌套耗时（补 T5 的 turn 级分布）。

### 3.2 L1：投影层

- transcript.jsonl / analytics.jsonl / session 快照全部改为**从 L0 派生**的缓存视图（读旧格式保持兼容，只读）。
- TUI/desktop 渲染改从 L0 投影会话状态（先旁路后切换），session 快照退化为 L0 的物化视图 + 可重建校验。

### 3.3 L2：消费层

- shannon trace 子命令：replay（重放渲染）、show（按 turn/工具/权限过滤）、diff（两会话或两次运行对比）、export（评测输入封装）。
- desktop 增加 Turn Timeline 视图（工具瀑布图 + token/成本累积曲线）——数据全部来自 L0，无新增采集成本。
- /rewind 与 FileHistoryManager 的 turn 边界对齐 L0 的 turn/end 事件，语义统一。

### 3.4 L3：OTLP 桥 + 脱敏

- telemetry.rs 重写为「L0 → OTLP/OTLP-http 导出器」：span 由事件链折叠生成，metrics 由计数投影生成；endpoint/trace_export 配置字段真正生效（补 T3）。
- RedactionPolicy 前置：环境变量密钥、keyring 引用、可配置正则；L0 落盘前统一过一层（补 T6）。原则：**脱敏在写入时做，不在导出时做**（盘上数据视为已清洁）。

## 4. 分阶段实施

| 阶段 | 内容 | 验收 |
|---|---|---|
| P0（~2 周） | L0 日志 + 事件词汇首版 + request/header 快照 + QueryEvent 落盘；双写期（旧设施并行） | ①「模型可见即已记录」集成测试：从日志重建请求与实际发送逐字节等价（除时间戳）；② seq 连续性 fuzz 测试；③ 断电截断恢复（损坏尾行丢弃并告警） |
| P1（~1–2 周） | 权限/hook/turn 事件入日志；投影切换（transcript/analytics 派生化）；shannon trace show/replay | 同一会话 replay 渲染与现场 TUI 快照一致；analytics 数字与 L0 聚合一致 |
| P2（~2 周） | L3 OTLP 桥 + RedactionPolicy + desktop Turn Timeline；旧格式只读退役计划 | Jaeger/Grafana 可看到含工具调用的完整 span 树；注入密钥的会话盘上无明文 |

## 5. 风险与对策

| 风险 | 对策 |
|---|---|
| 全量落盘的 I/O/体积 | chunk 聚合 fsync；日志轮转（按大小+天数）；大载荷（读文件结果）可配 truncate 阈值（header 记录 hash+长度） |
| 双写期数据不一致 | 以 L0 为准的夜间对账测试；不一致告警而非静默 |
| 隐私放大（日志含全部对话） | RedactionPolicy P0 就带最小集（env 密钥）；文档明示日志位置与清除命令 |
| 事件词汇过早冻结 | 词汇表放 shannon-types 独立模块 + serde untagged 兼容窗；ignorable 机制兜底 |

## 6. 与其他方案的关系

- 本方案 L0 是 [评测方案](agent-eval-landscape-and-plan.md) 的**硬前置**：轨迹级评测、成本归集、失败分类全部消费 L0。
- 事件总线/waterfall 通道（[插件架构方案](shannon-plugin-architecture-evaluation.md) §6.2 阶段 1）复用 L0 的词汇表，进程内分发与落盘共用一套 SessionEvent 定义，避免再次出现两套 schema。
- Pi 的启示（callback 即遥测、NOOP 默认）体现在 L3 桥：导出器可插拔、缺省 NOOP，trace 采集本身永远不阻塞主循环。
