# 遗留项 ②③ 回仓检查：无人值守 compaction 槽接入 + toolsets 对齐（2026-10-05）

> 出处：Provider/Model R2 复审方案（[../reviews/2026-10-05-provider-model-config-review-r2.md](../reviews/2026-10-05-provider-model-config-review-r2.md)）「遗留跟进」②③。只读调研，dev @ `63168f1e7`，全部 file:line 已核实。结论：②建议一个 S 批接入（清单与顺序见下）；③维持现状+注释钉住语义差异（B 路线否决，C 条件触发）。

---

## 项② 无人值守 runner 未接 compaction 槽

### 一、消费端机制（现状事实）

- 引擎字段与 builder：`auxiliary_compaction_client: Option<LlmClient>` 存于 `crates/shannon-core/src/query_engine/engine/mod.rs:290`；`with_auxiliary_compaction_client`（`mod.rs:801-804`）与读取 getter（`mod.rs:808-810`）。
- Agent loop 消费点：`agent_loop.rs:628` 克隆字段 → `agent_loop.rs:1616-1660` 按 `Some/None` 选 summarizer client → `agent_loop.rs:1646` `CompactEngine::with_llm_summarizer(summarizer_client)`。`None`（默认）保持历史行为：压缩请求 rides 会话自己的 client。
- 唯一已接线路径：desktop 交互发送 `desktop/src/commands.rs:1713-1720`（`utility_tier::resolve_auxiliary_client(state, AuxRole::Compression)` → `.with_auxiliary_compaction_client(...)`）。
- 重要：agent loop 的压缩有**两条臂**——token-based 臂（`agent_loop.rs:1506-1531`，`p2_compact::maybe_compact_with_policy`，纯本地 greedy drop-oldest，**无 LLM 调用**）只有 LLM summarizer 臂（`agent_loop.rs:1646`）才消费槽。触发阈值 `compression_threshold` 默认 0.75（`crates/shannon-engine/src/compact/types.rs:45-46,66`），即长任务才触发。
- 已有钉测：send 路径 wire 钉在 `agent_loop_tests.rs:2856-2980`（aux picked 时 `CompactEngine::with_llm_summarizer(picked)`）。

### 二、无人值守引擎构建点枚举（生产代码）

desktop/src 侧：

| # | Runner | 构建点 file:line | 构建方式 | 是否已持有 aux 解析入参 |
|---|---|---|---|---|
| 1 | 后台任务 `start_background_task` | `desktop/src/commands.rs:2690`（client 于 ：2662） | `QueryEngine::with_defaults_arc(LlmClient::new(client_config), …)` + `attach_shared_memory`；spawn 闭包内，`state` 在入口作用域可直取（:2613） | 是（2 行可接） |
| 2 | Goal runner `EngineGoalTurnRunner::new` | `desktop/src/goal_commands.rs:1149`（构造器 :1063） | 同上；client_config 来自 `GoalRunDeps`（:365-383，`from_state` :386-396）；**引擎整个 run 建一次**（:1044-1050），session 从 L0 恢复（:1155-1160） | 否——deps 无 `provider_store` |
| 3 | Batch 分支 `EngineBatchBranchRunner::stream_branch` | `desktop/src/batch_commands.rs:592` | 同上；`BatchRunDeps`（:456-477） | 否，同上 |
| 4 | Scheduled routine `spawn_routine_run` | `desktop/src/inbox_commands.rs:1387`（`make_engine_future` 工厂 ：1349 起，retry 每次重建引擎） | 同上；`RoutineRunDeps`（:962-987） | 否，同上 |
| 5 | `/context` `/diff` introspection | `desktop/src/commands_slash.rs:66` | 复用 stash 或最小引擎，**"the client is never contacted"**（:50-54） | 不适用——无 query、无压缩 |
| 6 | Dream | **不建引擎** | `desktop/src/commands_dream.rs:1209-1221` `consult_llm` 单发 `send_message`，全文件无 CompactEngine | 不适用 |
| 7 | Skill loop | **不建引擎** | `desktop/src/commands_skill_loop.rs:58-63,101-106`；`commands.rs:2291`；均单发调用 | 不适用 |

附带发现：desktop 手动 `/compact`（`compact_session`，`commands_slash.rs:334-338`）直接 `CompactEngine::with_llm_summarizer(engine.client().clone())`，**也没走槽**——交互路径上的同类机械增量漏网点。

crates 侧（建议缓做——无 AppState/providers.toml v2 aux store 读取链路，配置管北不同）：server `build_engine`（`crates/shannon-server/src/routes/mod.rs:141-158`）、GitHub webhook→routine（`github.rs:339-356`）、api_server 三路（`api_server.rs:738,922,1525`）、CLI headless/agent/REPL（`main.rs:2810,3957,2239`）、AgentTool 子代理（`shannon-tools/src/agent.rs:658`）。其余命中均在 `#[cfg(test)]`。

### 三、与「无人值守钉全局」的冲突核查：**不冲突**

- 钉的内容：`commands_chat.rs:2824-2924` `unattended_paths_pin_global_config` 只钉 (a) override/phase 不写全局 `client_config`、(b) 无人值守构造器直接读全局 Arc、(c) 仅交互解析路径应用偏好。
- 槽的正交性是**结构性的**：`lookup_auxiliary_target` 纯函数只读 active model profile 的 `auxiliary` map（`desktop/src/utility_tier.rs:107-126`），不触 session state/phase tier/`client_config`；裁定⑦契约明文在 `utility_tier.rs:36-46`。接入不改变主模型读全局 Arc 的事实，钉测三条断言原样通过。
- 唯一文档修订：`utility_tier.rs:43`「unattended run constructors … never route through this module」需加半句——主 client 仍不走本模块，仅 aux 槽读取经过。
- 小摩擦：三个 deps 结构体没有 `provider_store`，且 `resolve_auxiliary_client` 是 async 收 `&AppState`。最省事做法：在 tauri command 入口解析出 `Option<LlmClient>` 塞进 deps（新增一字段），runner 内一行 `.with_auxiliary_compaction_client(deps.aux_compaction.clone())`——语义等价于现有 `client_config` 的 spawn 时快照。

### 四、建议

**推荐做（desktop 4 个 QueryEngine 类 runner，一个 S 批 PR 打包，约 25-40 行 + 测试）**：

接入顺序（压缩频率×任务时长收益）：
1. **Goal runner**（收益最大）：引擎一次构建跑整个 run、L0 恢复历史、多轮迭代最易反复过阈值。
2. **`start_background_task`**：agent 式长跑且改动最小（state 在作用域）。
3. **Batch 分支**：中等时长。
4. **Routine**：prompt 通常短但调度频率高，机械增量顺带。顺手把 `/compact`（`commands_slash.rs:334-338`）接上（+3-5 行）。

「不接入反而正确」的两个理由（记录在案）：dream/skill-loop 的 LLM 路径不经过 CompactEngine（单发调用），没有消费点可接——接了才是造假消费点；token-based 压缩臂无 LLM 调用，槽天然无关。crates 侧各点待 desktop 落地验证后单独立项。

---

## 项③ `model_supports_toolsets` 与能力位对齐的障碍确认

### 一、依赖方向事实（#305 理由核查——只对了一半）

- 依赖边：`shannon-engine/Cargo.toml:11-12` 只依赖 `shannon-types` + `shannon-tool-interface`；`shannon-core/Cargo.toml:35` 依赖 engine。**engine→core 会成环——对「解析数据」（MODEL_CATALOG/declared/overlay 全在 core）而言环风险真实。**
- **但「位」本身已下沉**：`ModelCapability::ToolUse` 在 `shannon-types/src/provider_config.rs:140-151`（S2-4b 落位），engine 已依赖 types，消费枚举无需新边。真正的障碍：`LlmClientConfig`（engine/api/types.rs:401-448）没有能力字段，client 只看 `config.model` 字符串（`client.rs:318-324,341-347`），而解析逻辑与数据在 core。

### 二、`model_supports_toolsets` 的实际语义

- 名单：`engine/api/toolsets.rs:33-42`，小写前缀匹配 claude-opus-4/sonnet-4/opus-5/sonnet-5（注释自认 "Conservative prefix match"）；钉测 claude-3-5-sonnet→false、gpt-5→false。
- 两个消费点：① browser toolset 注入 `client.rs:318-324`（要求 provider==Anthropic **且** `enable_anthropic_toolsets` **且** 名单命中）；② computer-use beta 头追加 `client.rs:341-347`。默认关（`SHANNON_ANTHROPIC_TOOLSETS` env，opt-in）。
- **与 TOOL_USE 位回答的不是同一个问题**：位=「该模型会不会原生 tool calling」（通用能力）；名单=「该模型家族 API 面是否供 `browser_toolset_20260801` + `computer-use-2025-11-24` beta」。对齐会双向变化行为：claude-3-5/haiku 类 overlay 常 tool_use=true → 由正确拒绝翻成放行 → 400 风险；claude-4/5 若 catalog 无条目（unknown）→ fail-closed 则现有用户静默失去 toolset。

### 三、三条路线与结论

| 路线 | 内容 | 规模 | 风险 |
|---|---|---|---|
| A. 名单不动+文档标注语义差异 | `toolsets.rs` 注释：家族 API 面 ≠ 通用能力位 | S | 零 |
| B. 解析下沉后对齐 | catalog/declared/overlay 大搬迁 | L | 高（大面积重构+schema/钉测连坐），收益低 |
| C. 判定上移、调用方注入 | `LlmClientConfig` 加 `supports_browser_toolset: Option<bool>`（None=现名单），由 declared 填充 | S-M | 低（additive、默认字节不变） |

**结论：维持现状（= 路线 A，补注释钉住语义差异即可）**。「依赖方向障碍」的原始表述不精确（位已在 types），但结论恰好仍成立——对齐的是两个不同谓词，强行对齐要么引入 400 回归、要么近乎 no-op。**路线 C 作为条件触发的后续**：只有当出现真实需求（自建网关在名单外 id 上供 toolset API）时再落，约 20-40 行。
