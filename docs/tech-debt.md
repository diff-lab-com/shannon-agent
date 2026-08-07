# Shannon Technical Debt Register

> 跟踪那些**有意**留作"日后再说"的债务。
> 每条都有**触发条件**(什么时候必须还)和**预估成本**。
> 不要把"暂时不做"混同"永远不做"。

---

## P2-4.x · doc hardening(`cargo doc --no-deps -D warnings` advisory → required)

**Status**: ⏸️ **deferred**(2026-08-04)

**当前状态**:
- `.github/workflows/ci.yml` 的 `doc` job 设了 `continue-on-error: true`
- 已知警告种类:`[is_read_only]` 等断链 + 私有项 doc 引用 + 裸 URL
- `doc` job 仍跑、仍报告,但**不阻断 PR 合并**

**为什么不修**(产品+架构评审 2026-08-04):
- docs.rs 是 Shannon 用户的次要入口,不是主入口(主入口是 CLI / IDE / desktop)
- 没有公开 API 稳定性承诺,无 `#![deny(missing_docs)]` 强制策略
- 一次性修复 1.5d 即可,但开 `-D warnings` 是**长期约束**(每次 PR 都要付税)
- 当前贡献者都是内部+AI agent,熟悉代码,doc 提示价值低

**触发条件(任一即应还清)**:
1. 📦 **docs.rs 公开** — Shannon crate 上 docs.rs 准备公开托管
2. 🤝 **启动外部贡献者计划** — 第三方需要按 doc 提示理解模块
3. 📜 **启动 API 稳定性承诺** — 任何形式的"v1.0 不破坏"
4. 🚨 **CI 时间和反馈成本压力** — 让 doc 错着走是浪费 build 时间
5. 📉 **有 PR 因为 doc 警告被误拒** — 报警信号

**预估成本**:
- 一次性修复:1–2d(20–50 处断链 / 私有引用 / 裸 URL)
- 移除 `continue-on-error: true`:1 行
- **长期约束成本**(开 required 之后):每次 PR 增加小幅 doc 同步税

**还清时要做的事**:
1. `cargo doc --workspace --no-deps 2>&1 | tee /tmp/doc-errors.log` 收集所有警告
2. 按"断链 / 私有引用 / 裸 URL"分类
3. 修复 + 在 PR 描述里注明"fixes P2-4.x"
4. 改 `continue-on-error: true` → 删除该行,doc job 升级为 required
5. 同步 `docs/ci-gates.md` 表格的 required 列

**Owner**:TBD(触发时指派)

**参考文档**:`docs/ci-gates.md` §1 job catalog / 队友汇报 `2bf92611`
