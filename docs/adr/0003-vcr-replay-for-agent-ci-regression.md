# ADR 0003 — VCR Replay for Agent-Level CI Regression

**Status**: Accepted  
**Date**: 2026-07-12  
**Theme**: 用录制数据回放 agent 做 CI 无 key 回归 (Replay recorded fixtures to drive the agent in keyless CI)  
**Supersedes**: —  
**Related**: —

## Context

Shannon 的 record/replay 测试基建完成了约 80%，但最后一环缺失，导致 CI 无法做 agent 级回归。

**录制端**（已完成）：`LlmClient` 经 `SHANNON_RECORD_DIR` 钩子拦截请求/响应，存成 `.jsonl`（`crates/shannon-engine/src/testing/record_replay.rs`）。28 个 `record_task_*` 测试真连 API，验证 agent 完成任务并产出 fixture。

**回放端**（缺失）：`record_replay.rs:9-29` 的模块文档明确写了设计意图——"record once → mount as mockito mock → replay offline, no API key"——但文档引用的 `fixture.mount(&mut server)` 方法**不存在**。`ReplayHarness` 只有 loader（`from_dir` / `load_jsonl`），没有 mount 能力。因此当前 3 个 `replay_*` 测试只做 fixture 结构校验（可解析、非空、无密钥泄露），**不重新驱动 agent**。

结果：`just replay` 跑 14 个结构/辅助测试 + skip 42 个 `#[ignore]` 测试（28 录制 + 14 live provider）。CI 无法在没有 API key 的情况下验证 agent 端到端行为。

## Trigger

2026-07-12 一次录制会话（MiniMax-M3）暴露 8 个 bug，全部只在手动 `just record` 时才发现，CI 的 14 个结构测试一个都没抓到：

| Bug | 位置 | 单元测试能抓吗 |
| --- | --- | --- |
| Minimax 端点错误 (`chatcompletion_v2` → `chat/completions`) | `api/types.rs` | ✅ |
| OpenAI `finish_reason` + `tool_calls` 同一 chunk 互斥 | `api/adapter.rs` | ✅ |
| OpenAI tool-call 流缺 `ContentBlockStop` 合成 | `api/adapter.rs` | ✅ |
| 并行 tool call 共享 input buffer | `query_engine/engine.rs` | ⚠️ 难 |
| bwrap 把 project 绑到自身而非 `/workspace` | `sandbox.rs` | ❌ 集成层 |
| MultiEdit 同文件多编辑丢数据 | `tools/file/multiedit.rs` | ✅ |
| fixture 跨 workspace 污染（append 模式） | `live_tests.rs` | ❌ 集成层 |
| justfile `cache-stats` tab/space 混排 | `justfile` | ❌ |

大多数 bug 能在单元层抓到（当时没写对应单测）；但 bwrap 绑定、fixture 污染这类是**集成层**问题，只有 agent 级回放能覆盖。这一类是 replay 的独占价值。

## Decision

**实施 Phase 1 最小可用回放，不做 Phase 2 健壮化。** 补齐 `mount()` 这最后一块，让 `record_replay.rs` 的设计承诺兑现，为确定性任务提供 CI 无 key 的 agent 级回归。

### 交付物

1. **`RecordedExchange::mount_as_mock(&self, server) -> Mock`** —— mockito `.match_body(self.request.body.as_str())`（精确 body 匹配）+ `.with_status()` + `.with_body()`。不用 hash 匹配。
2. **`ReplayHarness::mount_all(&self, server) -> Vec<Mock>`** —— 遍历 fixtures 全部 mount，`.jsonl` 多轮 → 多个 mock。
3. **一个参数化测试 `replay_task_runs_agent_offline()`** —— 对每个确定性 fixture：起 mockito server → `mount_all` → 跑 `shannon --prompt <task>`（env `SHANNON_BASE_URL=mockito_url`）→ 断言 exit success + workspace 副作用。
4. **`just replay-agent` recipe** —— 和 `just replay`（结构校验）分开。

### 覆盖范围

只接**确定性任务**（4 个）：`create_file`、`bash_command`(echo)、`read_and_edit`、`overwrite_existing_file`。**跳过** `git_operations`、`glob_pattern`、`large_workspace`、`code_search`（非确定输出或多轮分支）。

`delete_file` 原计划纳入，实现时发现两个失配原因，故剔除（详见 Open Questions）。

### 失配策略

fixture 失配（系统提示 / 工具定义改动）时测试**跳过并打印 `fixture stale, run: just record`**，而不是 fail —— 避免 CI 因提示词改动而红，同时留下可见信号。

## Consequences

- **Positive**:
  - CI 无 key 可做 agent 级回归，覆盖 bwrap / fixture 这类集成层问题。
  - 兑现 `record_replay.rs` 的设计承诺，文档与代码一致。
  - 相对竞品（Claude Code / Aider / OpenCode 均未落地）形成差异化。
- **Negative**:
  - 精确 body 匹配脆弱 —— 系统提示 / 工具定义 / 序列化顺序改了就要重录。用 skip 而非 fail 缓解。
  - 只覆盖确定性子集，非确定性任务无回归保护。
  - 维护负担：重录是手动的（`just record`），无自动重录工作流。
- **Neutral**:
  - `vcr.rs` 的 `try_replay()` 与 `ReplayHarness` 两套系统并存，本 ADR 不合并（YAGNI）。

## Phase 1 落地回顾

> Status：Phase 1（PR #75，叠在 PR #73 之上）实施完成，但**未达成 CI 目标**。本文档据此重新定位。

### A. Phase 1 已交付（本地层面）

- **本地 4/4 通过**：4 个确定性任务 —— `create_file`、`bash_command`、`read_and_edit`、`overwrite_existing_file` —— 在本地 `just replay-agent` 下均通过，单次耗时 <1s，无需 API key。
- **harness 完整**：`RecordedExchange::mount_as_mock` + `ReplayHarness::mount_all`（兑现了 `record_replay.rs:9-29` 中 `fixture.mount(&mut server)` 的设计承诺）；`shannon_cli::tests::live_tests::mount_exchange` / `mount_fixture` 测试 helper；`justfile` 中 `replay-agent` recipe 与原有 `replay`（结构校验）解耦。
- **4 个 fixture 已提交**：`tests/fixtures/real_tasks/` 下 4 个 `.jsonl`，绕过 `.gitignore` 强制 add（fixture 数据不含密钥，扫描通过）。
- **后缀级路径重写 + SSE 分片兼容**：录制与回放都用 `/tmp/.tmpXXXXXX` 随机后缀，仅替换随机**后缀**而非整路径 —— 因为 OpenAI 兼容流式响应把 tool-call 参数拆成多个 SSE delta 分片，workspace 路径可能被切断在分片边界，整路径 replace 会漏匹配。
- **工具定义排序修复**（`crates/shannon-core/src/tools.rs`）：`to_tool_definitions()` 按 name 排序，消除 `HashMap.values()` 顺序不确定性导致的 body 抖动。是 VCR 匹配的根因前置条件，同时提升 Anthropic prompt-cache 命中率。
- **`delete_file` 排除维持原判**：详见下文 E 节。

### B. CI 层面结果

- **GitHub Actions Test job FAILURE**：PR #75 在 runner 上 4 个 `replay_agent_*` 测试**全部失败**，首个请求即返回 `http_501`（mockito 在无匹配 mock 时的默认响应）。
- **mockito 无法诊断**：mockito 1.7 在未匹配时返回 501，**不返回接收到的请求 body**。曾考虑自建 debug HTTP server 透出 body，对比期望/实际差异以定位具体失配字段，但 ROI 评估偏低（详见 D 节）。
- **本地 vs CI 分歧的本质**：同一份代码、同样的 fixture，本地全过、CI 全挂。已验证 **`mount_exchange` 计算出的期望 body 指纹（长度 + byte-sum）在本地与 CI 之间一致**（模去已知的后缀字节差），证明重写数学层面是确定性的；agent 在 CI 上发出的实际 body 必然与之存在某种分歧，但 mockito 看不到。

### C. 已系统排除的失配来源

逐项确认问题不在以下方面，避免后续维护者重复劳动：

1. **Prompt 不匹配**：fixture 录制时记录的 user prompt 与 `replay_*` 测试传入参数已逐字比对，一致。
2. **工具数组顺序**：fixture 已提交版本中 tools 顺序确定（`sorted=True`），agent 通过 `tools.rs` 排序后顺序一致。
3. **后缀碰撞风险**：录制时记录的后缀在 body 中仅出现在 workspace 路径（`/tmp/.tmpXXXXXX`），未与模型名 / 工具名 / prompt 字面量碰撞。
4. **重写数学错误**：纯函数确定性输入，当录制与回放的后缀同长度时长度稳定；Python 离线 reproducer 验证本地行为正确。
5. **权限模式差异**：曾怀疑 `--yes` / `BypassPermissions` 与 FullAuto 不一致，移除 `--yes` 仍失败。
6. **`MessageRequest` 隐式字段**：环境无关字段（model / max_tokens / system / messages / tools / stream / temperature / top_p / top_k / stop_sequences / budget_tokens）已逐项审阅，不含时间戳 / request ID / nonce。
7. **系统提示词环境差异**：系统提示词中唯一环境相关部分为 `Working directory: <CWD>`，已被 workspace 重写逻辑覆盖。
8. **fixture 内容漂移**：本地与 remote 的同一 fixture git blob 哈希一致；tools 顺序一致。
9. **缓存 nonce / cache_control 注入**：从输入确定性派生，不引入额外随机性。
10. **路径规范/符号链接**：Linux TempDir 非符号链接，无路径规范化差异需要考虑。

### D. 架构层面结论（核心教训）

**精确匹配 VCR 回放对 CI 太脆弱，不适合作为 CI 保护层。**

理由：mockito 要求 body 字节级一致才能匹配，而 CI runner 引入了某种无法隔离子问题空间的差异（rustc 版本、依赖更新、环境变量、prompt 微调、甚至 SSE 缓冲时序都可能成为不稳定的源头）。即便后续 debug session 找到了**这次**的具体失配字段，下次 rustc 升级 / 依赖更新 / prompt 调整 / 环境变化都可能再次打破 —— 精确匹配本质上是把 CI 稳定性绑定到了所有上游依赖的"恰好不动"假设上，违反 YAGNI 与系统脆性的工程常识。

**Phase 1 重新定位为本地开发者 harness**：`just replay-agent` 是 <1s 无 key 的本地开发反馈工具，覆盖集成层 bug 的快速回归。CI 价值推迟到 **Phase 2** —— 需要根本不同的匹配策略（宽松 body 匹配 / 序列号计数器 / 结构化匹配 / 响应模板），届时单独立 ADR。

### E. `delete_file` 双重排除（维持原判）

排除原因与 VCR 机制**正交**：

1. **沙箱 cwd 不可见**：录制时 fixture 中 Bash 工具调用带有显式 `cwd: <绝对 tempdir 路径>`，该绝对路径在 bwrap 沙箱内不可见 —— 沙箱把 workspace 绑到 `/workspace`（见提交 `7e8dda0`），导致 Bash 工具报错，请求流转偏离录制流。
2. **本质非确定性**：该 fixture 是 17 轮探索式 Bash/Glob 会话，Glob/Bash 结果依赖文件系统列举顺序，**精确字节匹配从原理上不可复现**。

修复路径：用更紧的 prompt 重录（单文件删除、不带显式 `cwd`），但当前 ROI 不足，已 deferred。

### F. 后续走向

- **本地 harness 保留**：`just replay-agent` 仍是开发者工具，4 个 `replay_agent_*` 测试本地仍可通过 `--run-ignored ignored-only` 运行。
- **CI 仍有部分 VCR 保护**：CI 跑 `replay_*` 结构校验测试（fixture 可解析、非空、无密钥泄露），这部分不依赖 agent 执行，不受本次 CI 失败影响。
- **Phase 2 工作原则**（写给未来接手者）：**不要再尝试让精确匹配通过 CI**。换根本策略 —— 顺序匹配回退、宽松匹配、模板响应、或者直接转向 contract testing，每条路都需要独立评估，必须有具体 CI 需求驱动，否则不进 ADR。

## Alternatives Considered

- **不做（维持现状）** —— 拒绝。8 个 bug 证明集成层回归有真实价值，且 80% 基建已投入。
- **全量 28 任务覆盖 + 健壮化** —— 拒绝。非确定性任务需顺序匹配回退、宽松匹配、自动重录，3-4 天 ROI 不够（solo-dev 定位）。
- **只写针对性单元测试，不搞 agent 回放** —— 部分采纳。本次 8 个 bug 已 / 将补单元测试；但 agent 回放独占集成层价值，二者互补不互斥。
- **用 `vcr.rs` 的 `try_replay()` 路径** —— 拒绝。它读 `.json`（单轮单文件），与 `record_task_*` 的 `.jsonl`（多轮 append）格式不匹配，且几乎无人使用；统一格式反而是更大工程。

## Competitor Landscape

| 工具 | 做法 | 有 record/replay agent 测试吗 |
| --- | --- | --- |
| Claude Code | Agent 测试全部 mock LLM 响应（[issue #11770](https://github.com/anthropics/claude-code/issues/11770) 直言"测的是 mock 基础设施而非真实 LLM 行为"） | 没有，和 Shannon 同样的缺口 |
| Aider | 真实 API benchmark（测模型能力，不测 aider 代码回归）；单测用简单 mock | 没有 VCR cassette 系统 |
| OpenCode / Goose | mock 单测为主 | 没有成熟 record/replay |
| 业界范式 | VCR.py / Reel / LangGraph Cassette / go-vcr —— "record once, replay forever" | 模式成熟，但**没有主流 CLI coding agent 落地** |

Shannon 补上是差异化点；模式本身不新。

## Implementation References

- 设计文档（已存在）：`crates/shannon-engine/src/testing/record_replay.rs:9-29` —— 模块 doc comment 描述了 mount 用法，但方法未实现。
- `RecordedExchange` 结构：`record_replay.rs:38-55`。
- `ReplayHarness`（待加 `mount_all`）：`record_replay.rs:237-240`。
- 录制钩子：`crates/shannon-engine/src/api/client.rs:218-231`（`SHANNON_RECORD_DIR`）。
- 录制测试入口：`crates/shannon-cli/tests/live_tests.rs:169`（`shannon_record` helper）。
- 当前结构校验测试：`live_tests.rs:1617`（`replay_fixtures_load_successfully`）。

## Open Questions

- mockito 多 mock 对同 path 的匹配顺序（LIFO vs 注册序）需验证，确认多轮请求能各得其所。**已验证**：mockito 按注册序匹配，多轮 `.match_body` 精确匹配各得其所，4 个 replay_agent 测试通过。
- 是否需要在重录时自动跳过非确定性任务（避免 CI 因它们 stale 而永久 skip）—— 留待 Phase 1 落地后评估。
- **`delete_file` 剔除原因**（实现期发现）：(1) 模型在 Bash 工具调用里显式带 `cwd: /tmp/.tmpXXXX`，该绝对路径在 bwrap 沙箱内不可见（沙箱把 workspace 绑到 `/workspace`），工具报错导致请求流转偏离录制流；(2) 该 fixture 是 17 轮探索式 Bash/Glob 会话，Glob/Bash 结果依赖文件系统顺序，本质上不可精确复现。修复路径：用更紧的 prompt 重录（单文件删除、不带 cwd），属未来工作。

### 实现期关键技术决策（Phase 1 落地补充）

1. **工具定义排序**（根因修复）：`to_tool_definitions()` 遍历 `HashMap.values()`，顺序非确定，导致请求 body 里 tools 数组顺序每次不同，精确匹配必然失配。已在 `crates/shannon-core/src/tools.rs` 按 name 排序——既是 VCR 前提，也提升 Anthropic prompt-cache 命中率（cache breakpoint 位置敏感）。
2. **workspace 路径按后缀重写**：录制与回放的 TempDir 都用 `/tmp/.tmp` + 随机后缀。只替换随机**后缀**而非整路径——因为 OpenAI 兼容流式响应把 tool-call 参数拆成多个 SSE delta 分片，workspace 路径会被切断在分片边界（如分片 1 结尾 `/tmp/.tmp`、分片 2 开头是后缀），整路径 replace 抓不到，后缀 replace 逐分片生效。
3. **`--yes` 绕过权限**：回放是复现已批准的录制会话，应用 `BypassPermissions` 而非让 headless FullAuto 拒绝危险操作导致偏离。
