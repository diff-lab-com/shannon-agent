# Shannon Technical Debt Register

> 跟踪那些**有意**留作"日后再说"的债务。
> 每条都有**触发条件**(什么时候必须还)和**预估成本**。
> 不要把"暂时不做"混同"永远不做"。

---

## P2-4.x · doc hardening(`cargo doc --no-deps -D warnings` advisory → required)

**Status**: 🟡 **大部分已做**(2026-08-08 更新)—— P2-4 已落地 doc build / rustsec-audit / cross-platform matrix(`2bf92611`);`doc` job 仍带 `continue-on-error: true`,即 `-D warnings` required 这最后一公里未关。

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

---

## TD-2 · `pre_resolve_context` 签名债(返回 `()` 非 `Result`)

**Status**: ⏸️ **deferred**(ADR-0008 P2-6,登记 2026-08-08;2026-08-10 复审维持)

**当前状态**:
- `crates/shannon-core/src/query_engine/engine.rs:420` 的 `pub async fn pre_resolve_context(&mut self)` 返回 `()`。**2026-08-10 复审核实**:实际实现是 Ollama-only 的 `check_ollama_capabilities().await` → `Option`,`if let Some` 命中才更新 `effective_max_context_tokens`,None 静默跳过 —— **无 `catch_unwind`、无 error 路径**(原登记描述"catch_unwind 兜底,错误靠日志"不准确,已修正)。
- ADR-0008 已识别为 deferred。

**为什么不修**:函数无失败路径,强改 `Result` 会是恒定 `Ok(())`;签名改动还牵动 3 个 `block_on` 调用点。"有信息就更新、没有就跳过"是设计意图,非 fail-soft 缺陷。

**触发条件**(任一即应还清):
1. 出现因 `pre_resolve_context` 静默吞错导致的真实 bug(经核实当前无 error 路径,短期内不适用);
2. 启动一轮 query engine 错误处理统一化(panic vs `Result` 收口)。

**预估成本**:1–2d(签名 + 全调用点 + 测试调整)。

**Owner**:TBD(触发时指派)。

---

## TD-3 · 桌面状态层用 JSONL 而非 SQLite

**Status**: ⏸️ **deferred**(登记 2026-08-08;对应 improvement-plan P3-4)

**当前状态**:
- 桌面会话历史 / scheduled-runs / triage / skill-candidates 均为 append-only **JSONL**(`desktop/src/scheduled_commands.rs`、`skill_pattern_detection.rs` 等);`shannon-core` 侧 `SessionManager` 同样 JSONL。

**为什么不修**:小规模会话够用,JSONL 可审计、实现简单。

**触发条件**:
1. 会话数上升到搜索 / 分页 / 并发写出现可感延迟;
2. 需要结构化跨会话查询(按时间 / 标签 / agent)。

**预估成本**:~2w(`rusqlite` 引入 + 迁移 + 双写过渡 + 回归)。

**Owner**:TBD。

---

## TD-4 · ADR-0009 Phase 2 — retire `ProviderConnection` wire type

**Status**: ✅ **已完成**(PR #54,合并至 dev `07913d97`,2026-08-10)

**落地结果**:
- `ProviderConnection` 现镜像 `ProviderProfile`(`label`→`display_name`、`provider_kind`→`kind` 保留为 String slug;删除死字段 `api_key`/`model`/`created_at`;新增 `has_api_key: bool` 派生自 `credential_manager::read_credential_value_default(id)` —— 修复 Phase 1 起的 dead signal:UI `hasKey = !!conn.api_key` 恒为 false)。
- 边界 = 薄 wire DTO 对齐 `ProviderProfile` + 派生 `has_api_key`,**不是**删除 struct(设计修正:`ProviderProfile` 带 `#[serde(deny_unknown_fields)]` + `CredentialRef`(后端细节 UI 不消费)+ 无 has_api_key 信号,不能直接当 wire type)。
- legacy `providers.json` 读路径用独立 `LegacyProviderConnection`/`LegacyProvidersFile` 保留(一次性 `migrate_providers_to_toml`)。
- `mask_providers` 已删除;`save_provider` 加 model_id 级联、`set_active_provider` 硬编码 `"default"`(无回归:`conn.model` 在该路径恒为 None)。

**参考**:设计记录 `docs/plans/td-4-retire-provider-connection.md`;spike `docs/spikes/p2-2-s1-1-lock-design.md` §7;ADR-0009。

---

## TD-5 · auto-commit hook 拆分多文件提交(UX 债)

**Status**: ⏸️ **known,暂不改**(登记 2026-08-08;2026-08-10 复审维持)

**当前状态**:
- PostToolUse hook 在每次 Edit/Write 后触发,把多文件重构拆成多个泛化 commit;多文件 PR 需手动 `git reset --soft HEAD~N && git commit` 合并。
- **2026-08-10 复审澄清**:这是**开发工具链 hook**(用 Claude Code/OMC 开发 Shannon 时的 auto-commit),**非 Shannon 产品债** —— 不进 Shannon 代码库。W6-2 P2-6(PR #52,产品 file-history/`/rewind`)与 `AutoCommitTool`(Shannon agent 工具)均不解决本项。

**为什么不修**:hook 对单文件编辑体验是正向的;关掉会损失自动提交便利。这是工作流偏好,非 bug;解法在开发工具链(OMC hook 配置批量窗口 / `--squash`),不在 Shannon 仓库。

**触发条件**:
1. P2-6(auto-commit + Undo)启动时一并设计 `--squash` / 批量提交选项(注:P2-6 已 done,指产品侧,未触及本开发体验项);
2. 贡献者普遍反馈合并成本过高。

**预估成本**:0.5–1d(开发工具链 hook 改造,非 Shannon 代码)。

**Owner**:TBD。**参考**:memory `auto-commit-hook-splits-edits`。
