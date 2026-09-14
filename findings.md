
## 分支/worktree 盘点（2026-09-14，应用户要求合并前核查）

| 分支 | worktree | 相对 dev 独有提交 | 结论 |
|---|---|---|---|
| marketing/positioning-2026-09 | ../shannon-marketing | 0（历史中已合并，落后 dev 44） | 无需合并 |
| feature/swe-batch-n3 | .claude/worktrees/agent-a1cc9ba… | 0（fd8205c9 已在 dev） | 无需合并 |
| backup/*-pre-rebase ×2 | — | 备份引用 | 不动 |

**发现**：swe-batch-n3 worktree 存在一处 staged 未提交改动——回退删除了该分支
head 提交（fd8205c9 "unshallow swebench repos"）引入的 ensure_local 修复（-65/+2），
伴随 .batch-scratch/ 草稿目录。判定为调试期实验回退，**不应带入 dev**；
已保持 worktree 原状，留待该分支作者决定保留或丢弃。
