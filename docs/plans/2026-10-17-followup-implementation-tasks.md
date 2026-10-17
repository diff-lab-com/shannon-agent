# 推进任务清单:跟进项实施(2026-10-17)

> **状态:草案,待 ericdong 审核**(未提交;审核通过后按既定 agent-team 流程执行)
> **依据**:登记册 K1-K4 推荐意见(A-1+A-3 立即;A-2 独立 PR;ACP spike 有条件启动;A-4 押后、K4 不动——不在本清单)
> **执行形态**:三个独立 PR,各自走 worktree + 实施 + 评审 + 终审 + CI 全绿合并

---

## PR-1 · 维护小批(约半小时)

### Task 1 · deny 重评触发器落账(A-1)

- **范围**:仅改 `docs/tech-debt.md`,追加一条:zip 8.6.0 三条 RUSTSEC-2026-0367/0368/0369 经 calamine 只读路径引入、已按仓库惯例 ignore,触发器="任一上游(calamine/rust_xlsxwriter/utoipa-swagger-ui)发布 zip≥9 兼容版本时升级并移除 ignore",指向 `deny.toml` 对应注释块。
- **前置核查**:确认 `docs/tech-debt.md` 存在及其条目格式,追加风格与之一致;文件不存在则新建(头部说明用途)。
- **验收**:条目存在且含触发条件与 deny.toml 指引;diff 仅此文件。
- **门禁**:文档无门禁;commit `docs: tech-debt 记 zip RUSTSEC ignore 重评触发器`。

### Task 2 · `fixture_hook_manager` 测试 hermetic 化(A-3)

- **范围**:`crates/shannon-core/src/assembly_dump.rs` 既有 `fixture_hook_manager`(~:938-970)改用 `load_from_path(&project)`(同 `e3682794d` 对 fallback 测试的修法);逐断言核对该测试不依赖 `load()` 才有的语义(特别是 `load_failures`——若依赖则该断言改由专用 fixture 承担);不改动既有断言的意图。
- **验收**:测试无家目录依赖(grep 无 `load()` 残留于 fixture);shannon-core 相关测试全绿;diff 仅该测试函数。
- **门禁**:fmt/clippy/doc + `cargo nextest run -p shannon-core`(hooks_section 相关至少)。

---

## PR-2 · `HookDef.command` serde default(独立行为变更 PR,约半天)

### Task 3 · serde default + 首激活审计 + release note(A-2)

- **范围**(四件,缺一不收):
  1. `crates/shannon-engine/src/hooks/config.rs`:`HookDef.command` 加 `#[serde(default)]`;同步校验 `new_http/new_llm` 构造器与 serde 路径一致(空 command 语义 = #430 已落地的 payload 回退,行为闭环已存在)。
  2. **首激活审计**:`HookManager::load()` 完成时,对"type 为 http/llm 且 command 为空"的每个钩子打一条 `tracing::warn!`(一次性、随 load 幂等,文案注明"该钩子此前会因缺少 command 字段而解析失败,现已激活");`load_failures` 机制不受影响。
  3. **release note 段落**:PR 正文含"发布说明"小节(供 dev→main 提升 PR 引用):"此前因缺少 command 字段而无法解析的 http/llm 钩子配置,现将被解析并激活"。
  4. **测试**:无 `command` 字段的 http 与 llm 配置各一——解析成功、dump 渲染 payload 回退正确、warn 出现;既有配置(显式 command)回归不变。
- **边界**:不改 `matches()`/执行路径;不做配置迁移工具。
- **验收**:`cargo nextest -p shannon-engine -p shannon-core` 全绿;新增测试 ≥3;PR 正文含发布说明段。
- **时机**:合并入 `dev` 不受版本窗口限制(他们的发布自 dev 提升),发布说明在 dev→main 提升时引用;不 hotfix `main`。

---

## PR-3 · ACP 子代理 spike(条件启动,2-3 天 timebox)

### Task 4 · spike:ACP client 骨架 + Claude Code 子代理 round-trip(D-1)

- **前置条件**(两条同时满足才开工,否则此任务保持 pending):
  1. 9/21 复审 R0 止血项已清(以 `docs/harness-wave-r0-r1-r2-implementation-*` 或复审验收记录为准);
  2. R2 排期尚未开始(spike 产出要能进入 R2 输入)。
- **范围**(timebox 硬限 3 个工作日,超时即止损写报告):
  1. ACP client 最小骨架:会话创建/提示发送/流式结果接收三端点(spawn 真 Claude Code,ACP 协议);
  2. 凭据隔离方案落地:provider 自持凭据(Claude Code 用自己的登录/API key),Shannon 凭据链不透传;
  3. 端到端委派一次:结构化任务进 → 结果回(验收线);
  4. **主交付物 = spike 报告**(`docs/spikes/2026-XX-acp-subagent-spike.md`):可行性结论、真实工作量重估(对照报告 P1-3"代价最小"论断)、协议/版本风险清单、R2 设计输入(若可行:provider 注册表怎么接 `AgentExecutor`;若不可行:替代路径)。
- **边界**:Codex app-server、teams 集成、并发编排、生产化打磨一律不做;**原型代码默认不合并**(独立分支留存,报告合入 docs/);不接 CI 门禁。
- **验收**:报告存在且四节齐全;端到端委派成功与否有可复现证据(命令/日志);timebox 内完成或明确止损说明。
- **形态**:单实施者 + 一轮评审(评审对象是报告与证据,不是原型代码质量)。

---

## 推进顺序与依赖

```
PR-1(Task 1+2,合一个维护 PR,当天) ──无依赖
PR-2(Task 3,独立 PR,半天)        ──无依赖;发布说明在 dev→main 提升时引用
PR-3(Task 4,条件启动,2-3 天)      ──前置:R0 清账 + R2 未排期
```

PR-1/PR-2 互不依赖可并行;PR-3 由你确认前置条件满足后启动。

## 决策记录

- K1:本清单 = A-1+A-3;A-4(`/api/assembly` OpenAPI 注册)押后,触发条件见登记册。
- K2:Task 3 即 A-2,独立 PR。
- K3:Task 4 即 spike,条件启动。
- K4:无任务(hooks 扩面维持 R2.3,特定事件按需单点补)。
