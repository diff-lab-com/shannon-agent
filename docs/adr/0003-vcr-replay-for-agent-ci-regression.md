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

只接**确定性任务**（~5-8 个）：`create_file`、`bash_command`(echo)、`read_and_edit`、`overwrite_existing_file`、`delete_file`。**跳过** `git_operations`、`glob_pattern`、`large_workspace`、`code_search`（非确定输出或多轮分支）。

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

- mockito 多 mock 对同 path 的匹配顺序（LIFO vs 注册序）需验证，确认多轮请求能各得其所。若不行，回退到 `Mutex<AtomicUsize>` 顺序计数器（Phase 2，本轮不做）。
- 是否需要在重录时自动跳过非确定性任务（避免 CI 因它们 stale 而永久 skip）—— 留待 Phase 1 落地后评估。
