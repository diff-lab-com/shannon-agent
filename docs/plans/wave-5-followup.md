# Wave 5 / 6 / 7 跟进规划(2026-08-08)

> **Track**: Wave 4 收口后的下一阶段执行方案
> **Date**: 2026-08-08
> **Status**: Proposed(供 ericdong 评审;评审通过后才开工)
> **Foundation**: [improvement-plan-2026-08.md](../improvement-plan-2026-08.md) §v4、[tech-debt.md](../tech-debt.md)、[spikes/](../spikes/)
> **用途**: 可执行方案,仿 `chat-upgrade.md` / `repo-map.md`。每个 task 独立 PR,目标分支 `dev`。

---

## 0. Context — 为什么是这三波

Wave 4(08-04 → 08-07)把 provider 读写收敛、parity 矩阵、semver 强制门禁、v0.8.0 全部关闭,双赛道主线功能齐备(23/24 ≈ 96%)。当前最大的风险不是缺功能,而是**文档/度量与代码不一致**——规划文档还停留在 08-03 之前的状态,会让后续决策建立在错误前提上。

因此三波按"**先对齐、再收尾、后扩张**"排序:

- **Wave 5(对齐)**:刷新度量 + 回写文档 + 补全 tech-debt。低成本、高信息价值,解除"文档撒谎"。
- **Wave 6(收尾)**:关掉编码赛道最后一块(P2-8 VS Code)+ 工程体验(P2-6 auto-commit/Undo)+ trust 债(P2-2 deprecation 收尾)。
- **Wave 7(扩张)**:安全护城河(P3-7 沙盒)+ 差异化交付(P3-1 artifact)。

---

## 1. Wave 5 — 文档 / 度量收敛(0.5–1d,立即可做)

> 大部分已在本次会话执行;此处记录为可复现的清单,剩余项见各 task 验收。

### W5-1 · 刷新 `docs/metrics.md`

- **步骤**:`bash scripts/gen-metrics.sh`(CI 的 `Generate Metrics` job 同款:Tauri/libdbus deps + `cargo test --no-run` + nextest list + clippy + deny)。
- **验收**:`metrics.md` 的 Snapshot 时间戳、commit、测试数、LOC 反映当前 `dev` tip;`cargo clippy --workspace -- -D warnings` 与 `cargo deny check` 双 pass。
- **注意**:CI 周更 workflow(#47)用 `peter-evans/create-pull-request` 开 PR,但需人工 merge;本地手跑更快。`schedule`/`dispatch` 只在 `main` 触发,dev→main 传播后才自动。

### W5-2 · 回写规划文档完成态

- **步骤**:
  1. `improvement-plan-2026-08.md` 顶部 v4 增量块(✅ 已加)。
  2. `ROADMAP-FUTURE.md` 顶部状态说明(✅ 已加)。
  3. `CLAUDE.md` dead_code 数字修正(✅ 已改:61 → ~96 + 复核命令)。
- **验收**:`grep "React 18" docs/` 仅命中 archive 与"描述问题"的文档;ROADMAP-FUTURE 读者一眼能看到"哪些已实现"。

### W5-3 · 补全 tech-debt register

- **步骤**:`docs/tech-debt.md` 追加 TD-2(`pre_resolve_context` 签名)、TD-3(JSONL→SQLite)、TD-4(ADR-0009 Phase 2 retire `ProviderConnection`)、TD-5(auto-commit hook UX);P2-4.x 状态从 deferred 改为"大部分已做"(✅ 已加)。
- **验收**:每条债有触发条件 + 估时 + owner 占位;project-review 识别的债全部登记或注明"已清偿"。

---

## 2. Wave 6 — 功能收尾(2–4w)

### W6-1 · P2-8 VS Code 扩展(2–3w,编码赛道最后一块)

- **现状**:spike 完成([spikes/p2-8-vscode.md](../spikes/p2-8-vscode.md) + `pres1-validation`);legacy 源在 `legacy-archives/shannon-code/editors/vscode/`(9 命令 + NDJSON 架构)。
- **详细方案**:[w6-1-p2-8-vscode-extension.md](./w6-1-p2-8-vscode-extension.md)
- **文件锚点**:迁移到 `extensions/vscode/`;通信改用 P2-7 HTTP API(`crates/shannon-core/src/api_server.rs`,`3ed22799`),弃用 NDJSON 子进程。
- **实施步骤**:
  1. 迁移 legacy 扩展骨架到 `extensions/vscode/`,换通信层为 HTTP(`shannon serve` 的 v1 session API)。
  2. 命令面收敛到核心:启动/停止 session、发消息、读流式输出、中止。
  3. 发布流程复用 desktop release(vsync-action / vsce)。
- **验收**:VS Code 里能起 session、发 prompt、看流式工具输出;发布到 Marketplace(或侧载 .vsix)。
- **依赖**:P2-7 ✅。**风险**:中(发布流程 / secretStorage)。

### W6-2 · P2-6 auto-commit + Undo / 快照(1–1.5w)

- **现状**:未做;先要梳理现有 PostToolUse auto-commit hook 行为(见 [tech-debt.md](../tech-debt.md) TD-5、memory `auto-commit-hook-splits-edits`)。
- **详细方案**:[w6-2-p2-6-auto-commit-undo.md](./w6-2-p2-6-auto-commit-undo.md)
- **文件锚点**:`scripts/hooks/`(现有 hook);新增 `crates/shannon-core/src/snapshot.rs`(文件级快照)或扩展 `session_transcript`。
- **实施步骤**:
  1. **梳理**:记录现有 hook 何时触发、生成什么 commit message、为何拆分多文件。
  2. **auto-commit 上下文消息**:Aider 风格——把本次编辑的 diff 摘要喂回 LLM 生成 commit message(而非泛化"edit file X")。
  3. **批量 / squash 选项**:hook 加批量窗口(同一逻辑变更的多文件合并一个 commit)或 `--squash` flag,解决 TD-5。
  4. **文件级 Undo / 快照**:扩展现有消息级 `/rewind` 到文件级——Edit/Write 前快照原文件到临时区,`/undo` 恢复。
- **验收**:一次多文件重构产生 1 个有意义的 commit;`/undo` 能恢复上一文件状态。
- **依赖**:无。**风险**:中(快照存储策略 / 误删保护)。

### W6-3 · P2-2 STABILITY deprecation 收尾(3–5d)

- **现状**:读写路径 + C3 parity 全闭环;剩余"legacy 路径 deprecation 标注 + CLI/桌面 provider/credential 逐项行为对齐"的收尾。
- **详细方案**:[w6-3-p2-2-deprecation-tail.md](./w6-3-p2-2-deprecation-tail.md)
- **文件锚点**:`crates/shannon-core/src/provider.rs`、`desktop/src/commands_config.rs`、`desktop/src/provider_read_snapshot.rs`、[STABILITY.md](../STABILITY.md)。
- **实施步骤**:
  1. 审计 `ProviderConnection` wire type 的残留消费方(为 TD-4 Phase 2 retire 做铺垫)。
  2. 给 legacy 读写路径加 `#[deprecated(note = "...")]` + cargo-semver-checks 验证 minor bump 路径。
  3. 对照 C3 matrix 的 6 行,补 CLI/桌面 provider/credential 行为差异表(若全对齐则归档)。
- **验收**:无未标注 legacy 路径;STABILITY.md 的 deprecated 清单与代码一致;C3 matrix 无悬空差异。
- **依赖**:无。**风险**:低。

---

## 3. Wave 7 — 安全 / 差异化(评审驱动)

### W7-1 · P3-7 沙盒执行(4–6w,spike S0 done)

- **现状**:S0 研究完成([spikes/p3-7-sandbox-s0.md](../spikes/p3-7-sandbox-s0.md));既有 `crates/shannon-core/src/sandbox.rs`(2570 行)+ `landlock = "0.4"` optional dep。
- **下一步**:评审通过 → D2(1d 验证编译错误)→ Phase A 实施。
- **范围**:Landlock(Linux)/ seccomp / Seatbelt(macOS,注意 15 弃用)/ Windows Job Object。
- **战略意义**:P2-7 HTTP 面的**执行面**补充,与安全定位联评;对标 Claude Code / OpenCode 的 Landlock-first。
- **依赖**:无。**风险**:中高(跨平台 LSM API 差异)。

### W7-2 · P3-1 artifact 显性化(2w,可提前)

- **现状**:未做;依赖 P2-5d 美化(✅ 已完成),可提级。
- **思路**:把 agent 产出的代码 / 文档 / 图当作**一等交付物**显性呈现(而非埋在聊天里),对标 openworker §9.3。
- **文件锚点**:`desktop/ui/src/components/`(artifact 面板)、`crates/shannon-ui/src/widgets/`。
- **依赖**:P2-5d ✅。**风险**:低。

---

## 4. 依赖关系图

```
W5(对齐,独立)── 解除"文档撒谎"

W6-1 P2-8 VS Code ── 依赖 P2-7 ✅
W6-2 P2-6 auto-commit/Undo ── 独立(先梳理 hook)
W6-3 P2-2 deprecation 收尾 ── 独立 ── 为 TD-4 Phase 2 铺垫

W7-1 P3-7 沙盒 ── 评审驱动,独立
W7-2 P3-1 artifact ── 依赖 P2-5d ✅
```

**关键路径**:W5(对齐)→ W6-1(VS Code,IDE 入口闭环)→ W7-1(沙盒,安全护城河)。
**可并行**:W6-2 / W6-3 / W7-2 互不阻塞,可分配给不同 worktree / agent。

---

## 5. 推荐执行顺序与估时

| 顺序 | Task | 估时 | 优先级 | 备注 |
|---|---|---|---|---|
| 1 | W5-1/2/3 文档收敛 | 0.5–1d | 🔴 高 | 大部分已做,补 metrics 刷新即可 |
| 2 | W6-3 P2-2 deprecation 收尾 | 3–5d | 🟡 中 | 低风险,清 trust 债,为 TD-4 铺垫 |
| 3 | W6-1 P2-8 VS Code | 2–3w | 🟡 中 | 编码赛道最后一块,IDE 入口 |
| 4 | W6-2 P2-6 auto-commit/Undo | 1–1.5w | 🟡 中 | 先梳理 hook,再做 squash + Undo |
| 5 | W7-2 P3-1 artifact | 2w | 🟢 稳 | 依赖已满足,可提前 |
| 6 | W7-1 P3-7 沙盒 | 4–6w | 🟢 稳 | 评审驱动,长期 |

**总估时**:W5(1d)+ W6(4–5w)+ W7(6–8w),分批推进,W5 立即、W6 本月、W7 评审后。

---

## 6. 风险登记

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| metrics 本地手跑依赖完整(Tauri/libdbus) | 中 | 低 | 失败则等 CI 周更 PR(#47) |
| P2-8 VS Code 发布流程(secretStorage / Marketplace) | 中 | 中 | 复用 desktop release 流程;先侧载 .vsix 验证 |
| P2-6 快照存储误删 | 中 | 中 | 快照写 tempdir + TTL;Undo 前确认 |
| P3-7 跨平台 LSM 差异 | 高 | 中 | S0 spike 已分平台评估;Phase A 先 Linux Landlock |

---

## 7. 评审请求

请 ericdong 决策:

1. **W5**:立即收尾(刷 metrics)— 已基本执行,确认即可。
2. **W6 三项**:是否本月启动?推荐顺序 W6-3 → W6-1 → W6-2(低风险先行)。
3. **W7**:P3-7 沙盒是否评审通过进 D2?P3-1 artifact 是否提前?

评审通过后,每个 task 拆成 `docs/plans/<task>.md` 独立方案 + TaskCreate 跟踪 + 按授权执行。

---

*本规划由 2026-08-08 全量状态盘点驱动;所有状态对应 `dev` tip `c90adf82` 与已合并 PR(#34/#36/#37/#38/#40/#41/#42/#45/#46/#47/#49)。*
