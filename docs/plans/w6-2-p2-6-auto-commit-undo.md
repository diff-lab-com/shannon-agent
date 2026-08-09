# W6-2 · P2-6 Auto-commit + Undo / 快照

> **Track**: Wave 6
> **Date**: 2026-08-08
> **Status**: Proposed
> **Estimate**: 1–1.5w · **Priority**: 🟡 中
> **Dependencies**: 无(先梳理现有 hook)
> **Parent**: [wave-5-followup.md](./wave-5-followup.md) §2 W6-2 · **Related**: [tech-debt.md](../tech-debt.md) TD-5

---

## 1. Context

两个交织的工程体验需求:

1. **auto-commit 上下文消息**:现有 PostToolUse hook 在 Edit/Write 后触发,但生成泛化 commit message(如 "edit file X"),且把多文件重构拆成多个 commit(见 [TD-5](../tech-debt.md))。要做成 Aider 风格——用 diff 摘要喂 LLM 生成有意义的 message + 批量合并。
2. **文件级 Undo / 快照**:当前只有消息级 `/rewind`;需要文件级——Edit/Write 前快照原文件,`/undo` 恢复。

## 2. 先做:梳理现有 hook 行为(0.5d)

- **文件**:`scripts/hooks/`(PostToolUse hook)、`.claude/settings.json`(hook 注册)。
- **产出**:文档记录 hook 何时触发、生成什么 message、为何拆分多文件。这是后续设计的前提。

## 3. 设计

### 3.1 上下文感知 auto-commit
- Edit/Write 后,收集本次变更的 diff,喂 LLM 生成 conventional-commit message。
- **批量窗口**:同一逻辑变更(短时间内 + 同一 task 上下文)的多文件合并为一个 commit;或显式 `--squash` flag。
- 解决 TD-5:多文件 PR 不再需要手动 `git reset --soft HEAD~N && git commit`。

### 3.2 文件级 Undo / 快照
- **快照存储**:Edit/Write 前把原文件复制到 tempdir(`~/.shannon/snapshots/<session>/`),带 TTL(默认 7d)。
- **`/undo`**:恢复最近一次文件变更(或带参数恢复 N 次)。
- **保护**:快照不覆盖未提交的工作;Undo 前确认。

## 4. 文件锚点

| 产物 | 路径 |
|---|---|
| hook 改造 | `scripts/hooks/post-tool-use-commit`(或等价) |
| 快照存储 | `crates/shannon-core/src/snapshot.rs`(新建)或扩展 `session_transcript` |
| `/undo` 命令 | `crates/shannon-commands/src/builtin/undo.rs`(新建) |
| `/rewind` 现状 | 参考(消息级,不动) |

## 5. 实施步骤

1. **梳理**(0.5d):记录现有 hook 行为 → `docs/plans/w6-2-hook-analysis.md`。
2. **auto-commit message**(2–3d):diff 摘要 → LLM → conventional-commit;批量窗口逻辑。
3. **`--squash` 选项**(1d):hook 支持 flag / 配置项。
4. **快照 + `/undo`**(3–4d):snapshot 存储 + 命令 + TTL 清理 + 测试。

## 6. 验收

- [ ] 一次多文件重构产生 **1 个**有意义的 conventional-commit。
- [ ] `/undo` 恢复上一文件状态;快照带 TTL 自动清理。
- [ ] Undo 不覆盖未提交工作(有确认)。
- [ ] hook 行为文档化(`w6-2-hook-analysis.md`)。

## 7. 风险

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| 快照存储误删 / 膨胀 | 中 | 中 | tempdir + TTL;Undo 前确认 |
| LLM 生成的 commit message 不准 | 中 | 低 | 可配置回退到规则模板;人工可 amend |
| 批量窗口误合并无关变更 | 中 | 中 | 窗口基于时间 + task 上下文;默认保守 |

## 8. 参考

- [tech-debt.md](../tech-debt.md) TD-5(auto-commit UX)
- 现有 `/rewind`:消息级回退
- Aider auto-commit 模式(每 edit 一 commit + 上下文 message)
