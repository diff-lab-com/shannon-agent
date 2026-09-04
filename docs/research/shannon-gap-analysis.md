# Shannon 现状差距分析：调研结论 × 当前实现的问题清单

- 日期：2026-08-27
- 输入：[DSH 调研](deepseek-harness-analysis.md)、[Pi 调研](pi-agent-analysis.md)、shannon-mono 本地代码勘察（2026-08-27，dev 分支）
- 关联：[插件架构评审](shannon-plugin-architecture-evaluation.md) · [trace 改进方案](shannon-trace-improvement-plan.md) · [评测方案](agent-eval-landscape-and-plan.md)
- 勘察范围：crates/ 下 shannon-core、shannon-engine、shannon-server 的模块布局与关键文件（plugin/、telemetry.rs、analytics.rs、session_transcript.rs、recording/、testing/、query_engine/types.rs、hooks/events.rs、state.rs、sse.rs），tests/scenarios/ 目录

## 0. 总判断

Shannon 的**功能面**已超过 Pi、接近 Claude Code（多 provider、team/worktree、hooks、LSP、gateway、desktop/mobile）；但**事实层**（会话即事件日志）与**反馈回路**（trace→评测→改进）落后于 DSH 与 Pi 的公开实践。当前最大的结构性风险不是缺功能，而是：**一次会话发生的事情没有唯一的、可回放的权威记录**，以及**五套扩展机制并行、互不知情**。

## 1. 现状盘点（证据）

### 1.1 与「记录」相关的五套设施（互不相通）

| 设施 | 位置 | 内容 | 落盘 |
|---|---|---|---|
| QueryEvent 流 | shannon-core/query_engine/types.rs | 17 种事件：Started/Text/ToolUseRequest/ToolUseResult/TurnCompleted(tokens)/Completed/Failed/Warning/Progress/ToolProgress/Thinking/Usage(input+output+cost+cache_creation+cache_read)/Cost/Info/ConversationUpdate(全量 messages)/RateLimit | **不落盘**（SSE/UI 消费后即弃） |
| 会话持久化 | shannon-engine/state.rs | 单个 JSON 文件（~/.shannon/sessions/&lt;uuid&gt;.json）：messages 快照 + SessionPersistMetadata（model/tokens/turn_count/title/parent_session_id/branch_point/project_path） | 快照式，非追加 |
| 会话转录 | shannon-core/session_transcript.rs | ~/.shannon/transcripts/&lt;session&gt;.jsonl，TranscriptEntry{id,role,content,timestamp,tool_calls,metadata} | 落盘，面向搜索/统计 |
| 使用分析 | shannon-core/analytics.rs | ~/.shannon/analytics/&lt;date&gt;.jsonl，事件：ToolExecution{tool,duration,success}/PromptSubmitted/ResponseReceived/FileOperation/SessionStart/End/Error/PermissionRequested | 落盘，按日聚合 |
| 录制回放 | shannon-core/recording/（+vcr.rs、engine/testing/record_replay.rs） | RecordingEntry：SessionStart/UserMessage/LlmRequest(含 request_hash+body)/LlmResponse/QueryEvent/ToolCall{tool,input,result,is_error,duration_ms}/SessionEnd | **测试专用**，生产不启用 |
| 遥测 | shannon-core/telemetry.rs | TelemetryManager：6 个原子计数器（spans_created/events_emitted/errors_reported/tool_calls/api_calls/tokens_used）+ TelemetryLayer 计数 tracing 事件 | 计数快照，无导出 |

### 1.2 扩展/定制机制盘点

- **MCP**：shannon-mcp，stdio/SSE，tools/list 自动发现，deferred schema 加载，webhook/channel。
- **plugin/**（shannon-core）：plugin.toml 清单（PluginKind = Tool(MCP transport)/Command/Skill）+ .claude-plugin/plugin.json 兼容解析 + git/本地/归档安装 + 远端 index + permissions 声明（read_files/write_files/execute_commands/network/mcp_tools/llm_api）。加载点在 REPL 与 headless。
- **hooks**（shannon-engine/hooks）：30 种 HookEventType（PreToolUse…TaskCompleted），触发外部命令与 routines。
- **skills / agents / profiles / routines**：.shannon/ 与 .claude/ 目录的 TOML/MD 声明式配置。
- **commands**：CommandRegistry（含 Plugin 来源的 PromptCommand）。

### 1.3 评测设施盘点

- tests/scenarios/*.yaml：10 个声明式场景（bash_command/code_search/complex_refactor/edit_file/error_recovery/multi_tool/multi_turn_edit/read_file/text_only/write_file）。
- testing/（shannon-core）：scenario.rs（YAML→mockito）、mock_dsl.rs、snapshot.rs（insta）、test_env.rs。
- 断言词汇：file_exists / file_content(contains|matches_regex) / file_not_exists / exit_code / tool_called / response_contains / max_duration_ms。
- 另有 perf_tests（阈值）、api_integration（mockito）、record/replay fixtures、desktop UI vitest。

## 2. 问题清单（P1–P10）

### P1 会话持久化是快照而非事件日志 【高】
- 现象：engine/state.rs 单 JSON 快照；无法回答「第 N 轮模型实际看到了什么」「这轮为什么失败」；崩溃丢尾部；/rewind 与 branch 依赖手工元数据而非结构。
- 对标：DSH SessionEventMap（seq 连续、request/header 快照、surface replace）；Pi 会话树（parentId 链 + durable program counter）。
- 改进：把 recording/ 的 RecordingEntry 词汇升级为生产级会话事件日志（详见 trace 方案 P0）。**这是后续所有改进的地基。**

### P2 QueryEvent 含金量高但不落盘 【高】
- 现象：Usage（含 cache_creation/cache_read token）、RateLimit、Warning、TurnCompleted(tokens) 只在 SSE/UI 通道流转；评测、成本对账、回归分析想要的历史轨迹拿不到。
- 对标：DSH「模型可见即已记录」；Pi 事件流即持久化输入。
- 改进：统一事件日志落地后，QueryEvent 成为日志的实时投影而非平行流。

### P3 trace 定位混乱：三套记录 + 一套计数器，无因果链 【高】
- 现象：transcript/analytics/recording 三种 JSONL schema 互不引用；analytics 事件无 query_id/turn 关联键；ToolCall 只有 duration 无所属 step；权限决策（PermissionChoice）与 hooks 触发不进任何记录。
- 对标：DSH 遥测=事件日志的订阅者（先脱敏再转发，OTel 桥为插件）；Pi telemetry 显式契约 NOOP 默认。
- 改进：见 trace 改进方案（本文档组第 5 篇）。

### P4 五套扩展机制无统一模型 【高】
- 现象：MCP（进程外）、plugin manifest（分发层）、hooks（外部命令）、skills/agents/profiles（声明式配置）各自为政：权限语义不同、生命周期不同、发现路径不同；plugin 的 permissions 声明未见强制执行闭环。
- 对标：DSH 把这一切收敛为「插件=服务+效应」；Pi 收敛为「扩展=工厂函数+注册副作用」。两者共同点：**一个心智模型**。
- 改进：见插件架构评审（第 4 篇）——建议 Rust 化 capability seam + 统一注册/生命周期，不照搬 Cordis。

### P5 能力接缝不显式，执行世界不可整体迁移 【中高】
- 现象：LlmClient adapter 是事实接缝；但文件读写（shannon-tools 各工具直接 std::fs）、子进程、sandbox.rs 是旁路能力，换沙箱/远程执行需要逐工具改造；desktop 与 CLI 各自装配。
- 对标：DSH ctx.fs/ctx.subprocess/ctx.sandbox 共享执行世界——换提供方即整体迁移 bash/PTY/LSP。
- 改进：定义 FileSystemProvider / ProcessProvider / SandboxProvider trait 并让工具层只依赖 trait（Pi 的 Gondolin 证明「覆写 7 个工具即可实现容器化」的前提正是工具层可替换）。

### P6 评测只测 harness 逻辑，不测 agent 能力 【高】
- 现象：YAML 场景用固定 mock 响应脚本驱动（测的是「给定模型输出，引擎/工具是否正确执行」）；没有真实模型的成功率/成本/方差；断言只有终端态（文件/退出码/包含文本），无轨迹断言；10 个场景无分层（smoke/regression/full）；CostTracker 存在但评测不采集成本。
- 对标：Composio 基准（真实 SaaS 工作流 × 8 harness）；DSH 自评 Terminal-Bench 2.1/DeepSWE/Toolathlon。
- 改进：评测金字塔 L0–L4（详见评测方案第 6 篇），把「harness 逻辑测试」与「agent 能力评测」明确分离。

### P7 权限与安全闭环缺口 【中】
- 现象：PluginPermission 六类声明存在于 manifest，**已核实（2026-08-27，W0 §4.5 端到端探针，feature/plugin-permission-probe @ 5f83afc6）全仓库零执行点强制**：read_files/write_files/execute_commands「不生效」（测试实证未声明照常写/执行）、network/llm_api「无对应执行点」（静态 grep 声明消费者为 0）；且 `permissions` 省略时反序列化为空表、语义为「未声明=default-allow」。附带发现：插件 discovery 即冷 spawn 进程且每次工具调用重复 spawn（mcp_tool_adapter.rs:312/456）。MCP 工具走 PermissionRuleChecker 但决策按工具名匹配、对 manifest 全盲；sandbox 非内核 seam。
- 对标：DSH 沙箱策略 + Landlock 原生 + 审批姿态作为产品模式；Pi 干脆不做（反例）。
- 改进：把权限执行点统一到工具执行管道的 waterfall（PreToolUse 已有 hook 位，缺 enforce 语义）。

### P8 上下文工程分散，无请求级快照 【中】
- 现象：compact（engine/compact）、context_injector、repo_map_injector、smart_context、team_prompt 各自注入；没有任何地方记录「本次请求最终拼装的系统提示词+工具 schema」。
- 对标：DSH EpochHeader（request/header 事件：配置+系统提示词+工具 schema 全量快照，请求=日志纯函数）。
- 改进：在 query 引擎派发点单点捕获请求信封入日志（trace 方案 P0 一部分）。

### P9 桌面/移动端会话同步缺权威快照模型 【中】
- 现象：sse.rs 把 QueryEvent 序列化给客户端；ConversationUpdate 在 Completed 前推全量 messages——已有快照语义雏形，但瞬态事件与状态更新混在同一流，客户端无「订阅权威快照」的显式契约。
- 对标：Pi「快照权威 + onEvent 瞬态通道分离 + 会话租用」。
- 改进：shannon-api-protocol 层区分 state/event 两类消息；mobile live sync 对齐。

### P10 面积与专注度 【中】
- 现象：shannon-core/src 单目录 70+ 模块（billing/voice_mode/magic_docs/tips/enhanced_suggestions/prevent_sleep…），「什么都在核心里」；dead_code 注释 ~96 处。
- 对标：Pi 的「PR 拒绝核心膨胀」纪律；DSH 把每个能力放独立包（代价是 221 包，需平衡）。
- 改进：新能力默认进扩展位（feature crate / MCP / hook routine）；核心模块按域拆子 crate（渐进，不搞大迁移）。

## 3. Shannon 既有优势（改造中必须保留）

1. **权限体系**：PermissionClassifier + LlmPermissionClassifier + 4-tier 优先级 + profiles——比 Pi 强，是产品差异化。
2. **多 agent 协作**：team/worktree//batch/agent view——DSH 的 agentTeams 仍是实验性，Shannon 已产品化。
3. **hooks 30 事件 + routines 自动触发**——生态位类似 DSH 能力事件，缺的是总线化。
4. **多产品装配**：CLI/TUI/desktop/gateway/mobile 共用 engine crate——天然需要 seam，动机已在。
5. **测试文化**：每个 src 文件至少一个 #[test]、mockito 场景、record/replay、insta——评测金字塔 L0/L2 的地基已存在。
6. **Claude 生态兼容**：plugin.json 清单解析、.claude/ 目录读取——冷启动生态的捷径。

## 4. 优先级路线（汇总）

| 优先级 | 问题 | 对应方案 | 依赖 |
|---|---|---|---|
| P0 | P1+P2+P3 会话事件日志统一 | trace 方案 P0 | 无（地基） |
| P0 | P6 评测分层 | 评测方案 L0→L1 | trace（部分） |
| P1 | P4+P7 扩展模型统一 + 权限闭环 | 插件评审阶段 1–2 | 事件总线 |
| P1 | P8 请求信封快照 | trace 方案 P0 | P1 |
| P2 | P5 执行世界接缝 | 插件评审阶段 3 | — |
| P2 | P9 同步协议快照化 | api-protocol 演进 | 事件日志 |
| P3 | P10 面积治理 | 持续 | — |

## 5. 方法说明与局限

- 本文档的证据来自对关键文件的抽样精读（头部 + 类型定义），未逐行核对全部 70+ 模块；P7「permissions 声明未强制」已于 2026-08-27 由 §4.5 端到端探针核实坐实（矩阵见 feature/plugin-permission-probe commit body）。
- DSH/Pi 的事实以 2026-08-26 可访问的公开文档为准；两者均在快速迭代，结论有保鲜期。
- 生态规模数字（dsh 6600+/700+）口径未一，仅作方向性参考。
