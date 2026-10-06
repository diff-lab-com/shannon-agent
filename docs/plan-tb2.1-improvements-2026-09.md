# P1d 后续改进计划（feat/agent-eval-bench 分支，2026-09-06 已批准实施）

## 0. 背景与目标

P1d Terminal-Bench 2.1 全量 89 题：**shannon+glm-5.3-flash 36/89 = 40.4%**，GLM 官方同模型
Claude Code harness 69.2/89 = 77.8%，**差额 33 题**。

差额分解（两份独立调研一致，详见对话记录与本目录 eval-findings 文档）：

| 损失类别 | 题数 | 占缺口 % | 成本 | 预期回收 |
|---|---:|---:|---|---|
| 上游 API 6-min 超时（Request timed out） | **21** | **63.6%** | 低 | +15–18 |
| 部署：GLIBC 不兼容（qemu-*） | **2** | **6.1%** | 中 | +2 |
| 适配器 CLI 启动 bug（`-` 开头 prompt） | **1** | **3.0%** | 极低 | +1 |
| Verifier 缺 uv/python-build-standalone | 9 | — | 跨 harness | 后续 |
| 真能力差距（模型 + 工具语义） | ~10 | — | 高 | 本轮不动 |

**目标**：40.4% → ~52–58%（+10–15 题），聚焦低成本高 ROI 的脚手架改动。

## 1. 实施阶段与落地情况

### 阶段 A — 适配器即时修复（已完成，`a91da9a9`）
1. Harbor adapter 默认注入 `SHANNON_STREAM_IDLE_SECS=180`（A1 看门狗 eval 默认开）
2. Prompt 改为 tmp 文件 + `shannon -p - < file`（stdin），修 clap 对 `-` 开头 prompt 的误解析
3. 移除 `--disallowed-tools WebFetch/WebSearch`

### 阶段 B — engine/工具改造（已完成）
4. `2944c6df`：headless 错误分类修正——"Request timed out" 归 Timeout(rc=3)，
   不再被子串 "context" 误判为 ContextOverflow(rc=5)
5. `333bb78a`：`RunBackground`/`WaitForLog`/`Kill` 工具组（`crates/shannon-tools/src/background.rs`），
   经 `ProcessProvider::spawn_piped` seam，512 行 ring buffer，同名幂等替换
6. `3a14b6db`：`SHANNON_TOKEN_BUDGET` 看门狗——超阈值合成 "Context is large" nudge，
   引导 Grep/head/offset+limit 精读替代整文件读
7. `f897e785`：headless（json-stream/json）自动启用 `no_progress_strikes`，
   `SHANNON_HEADLESS_AUTO_TEST_STRIKES` 可控

### 阶段 C — 部署/打包（已完成）
8. `0060e894`：musl 静态构建支持（README 记录 `CFLAGS_x86_64_unknown_linux_musl`
   fortify 处理与构建命令），qemu-* alpine 容器可换用静态二进制

### 阶段 D — 验证（运行中）
9. TB2.1 全量 89 题复测（3 并发，harbor job `2026-09-06__23-10-19`）——
   直接产出改进后新口径 + 31 题分层子集（control/stream-timeout/GLIBC/CLI/background/
   token-bloom 六层）的逐层 delta
10. 文档更新：发现文档 §5 + summary delta 段

## 2. 验收标准

1. 31 题分层子集 resolve rate 显著提升（目标 ≥ +10 题中的多数）
2. control 层 6 题全部仍 PASS（无回归）
3. 全量口径 ≥ 52%
4. 每项改动配单元测试（B.5×10、B.6×3、B.7×3 已绿；C.8 构建验证通过）
5. conventional commits，单项可独立回滚

## 3. 不在本期范围（后续 backlog）

- Agent subagent / Plan 引导（+2–3 题）
- DockerTool 暴露（+1–2 题）
- AnalyzeImage 批量接口
- Verifier 镜像预装 uv/python-build-standalone（+9–12 题，需推动 TB 团队）
- Harbor retry-on-NonZeroAgentExit（+3–5 题，需改 harbor 框架）
- 引擎换非 thinking 模型 anchor 的对比实验
