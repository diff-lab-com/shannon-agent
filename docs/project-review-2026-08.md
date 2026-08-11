# Shannon 项目审查报告

> 审查日期:2026-08-02
> 审查视角:高级产品经理 + 高级架构师
> 审查范围:工程架构、代码质量、文档一致性、完成度、技术债
> 证据来源:[SPEC.md](./SPEC.md)、[CLAUDE.md](../CLAUDE.md)、[desktop-architecture.md](./desktop-architecture.md)、[desktop/CLAUDE.md](../desktop/CLAUDE.md)、[ROADMAP.md](./ROADMAP.md)、[ROADMAP-FUTURE.md](./ROADMAP-FUTURE.md)、[STABILITY.md](./STABILITY.md)、[HOOK-AUDIT.md](./HOOK-AUDIT.md)、[competitor-testing-research.md](./competitor-testing-research.md)、当前分支 `fix/provider-model-command-remediation` 工作产物
> 关联:[competitor-feature-matrix.md](./competitor-feature-matrix.md)、[improvement-plan-2026-08.md](./improvement-plan-2026-08.md)

---

## 0. 总体评价

> **双产品线前提(2026-08-02 补)**:Shannon 是**两条产品线共用一套引擎**:
> - **shannon-code**(终端 REPL/CLI/IDE):编程场景,对标 Claude Code/Codex CLI/OpenCode/Reasonix。
> - **shannon-desktop**(Tauri 桌面):通用办公场景,对标 Claude Desktop/Codex Desktop/openworker/Hermes/WorkBuddy。
>
> 本报告的**工程/代码/文档问题**(§1–§3)对两条线都成立(共用 `crates/shannon-*`);但**完成度缺口**要分线看:编码线缺 repo map/auto-test loop,办公线缺 SaaS 集成/沙盒/附件/语音/多线程。详见 [competitor-feature-matrix.md](./competitor-feature-matrix.md) 双赛道矩阵。完整双赛道对标见 [competitor-feature-matrix.md](./competitor-feature-matrix.md)。

Shannon 是一个**架构成熟、工程扎实**的开源 Rust AI agent monorepo,~94K LOC、9 crate workspace、~120 源文件、48 工具,在多 provider/权限/agent/MCP 上的完整度接近 Claude Code(编码线)并在 Team/worktree 上独有差异化。**安全设计**(密钥 0600、redact 脱敏、fail-soft 校验)和**测试投入**(Record/Replay、YAML 场景)是显著优点。

但审查发现 **4 类系统性问题**:

1. **文档与代码漂移** —— 多处文档描述与实际实现不符,影响新人入门与决策追溯。
2. **Dead code / Dead events 沉淀** —— 多个命令模块、hook 事件定义了但未接通,是"半成品"的征兆。
3. **度量基线不统一** —— 测试数量、crate 行数在不同文档里数字打架,无法用作健康度指标。
4. **架构债累积** —— 部分 API 签名、文件体积、状态层(JSONL vs SQLite)有可见的技术债。

以下按严重度分级展开。

---

## 1. 🔴 严重(影响正确性 / 信任)

### 1.1 文档与实际框架描述不符

**`docs/desktop-architecture.md` 称桌面前端为 "React 18",实际是 React 19。**

证据:
- `desktop/ui/package.json`:`"react": "^19.0.1"`, `"react-dom": "^19.0.1"`
- `desktop/CLAUDE.md`(更权威):明确写 "Tauri v2 + **React 19** + TypeScript"
- `desktop/ui/src/App.tsx`、`main.tsx` 为 TSX,无任何 `.svelte` 文件,无 `$state/$derived/$props` 用法

**影响**:架构文档是新人入门和架构决策追溯的第一来源。版本错误(18 vs 19)会误导依赖兼容性判断(React 19 的 use Hook、Actions、ref-as-prop 等改变 API surface)。

**修复**:全文校对 `desktop-architecture.md`,把 "React 18" 改为 "React 19",并核查其中所有组件示例与实际 `desktop/ui/src/components/` 的一致性。

### 1.2 度量基线三处打架

| 来源 | 测试数 |
|---|---|
| `CLAUDE.md` "Test Coverage" | ~7889 |
| `competitor-testing-research.md` | ~9181 |
| `SPEC.md` | 3,180 test functions |

**影响**:测试数是工程健康度的核心指标。三处差 ~6000,说明要么有文档长期未更新,要么统计口径不一(unit vs integration vs e2e)。无法用作 release gate 或对外宣传的可信数字。

**同样问题**:crate 行数在 SPEC.md 和 CLAUDE.md 表格也不一致(SPEC 说 shannon-core ~49K,CLAUDE.md 说 ~3370 测试)。

**修复**:建立**单一权威度量源**(建议 CI 产物 `docs/metrics.md`,每次 CI 自动生成 nextest 计数 + cloc 行数),其它文档引用它而非写死数字。

### 1.3 ADR-0008 验收未闭环

当前分支 `fix/provider-model-command-remediation` 的 [provider-model-command-remediation.md](./plans/provider-model-command-remediation.md) 显示:P0–P3 全部条目**代码已落地 + 单测存在 + 测试门绿(10258/10258)**,但**有 20 条纯行为验收项需在交互式 REPL 里做人肉 QA** 才能关闭(清单见 `adr-0008-qa-checklist.md`)。

**影响**:这是当前未合并的高优先级工作。若不完成交互 QA 就合并,可能漏掉"卡片立即更新""切换 zh 完整翻译"等行为回归。

**修复**:合并前抽 15 分钟过完 20 条 QA 清单(详见 [improvement-plan](./improvement-plan-2026-08.md) P0)。

---

## 2. 🟠 重要(技术债 / 完成度)

### 2.1 Dead code 沉淀(命令模块未接通)

[ROADMAP.md](./ROADMAP.md) + [ROADMAP-FUTURE.md](./ROADMAP-FUTURE.md) 列出多个"已注册可达但内部 dead code"的模块:

| 模块 | 文件 | Dead 内容 | 状态 |
|---|---|---|---|
| `/diff` | `shannon-commands/src/builtin/diff.rs` | ChangeCategory 变体、DiffAnalysis 方法、DiffPattern | **接通 (P1-1)** |
| `/review_pr` | `shannon-commands/src/builtin/review_pr.rs` | ReviewSeverity、ReviewCategory、PRAnalysis | **接通 (P1-1)** |
| `/export` | `shannon-commands/src/builtin/export.rs` | ExportFormat、export_to_markdown/json | **接通 (P1-1)** |
| `/pdf` | `shannon-commands/src/builtin/pdf.rs` | PdfTable、ImageFormat | **删除 (P1-1)** |
| `/debug` | `shannon-commands/src/builtin/debug.rs` | DebugCategory、LogLevel | **接通 (P1-1)** |
| Coordinator | `shannon-agents/src/coordinator.rs` | AgentTeam、任务分配方法 | 仍未处理 |
| Compact | `shannon-core/src/compact.rs` | 5 种策略只用了默认 | 仍未处理 |
| Doctor | `shannon-core/src/doctor.rs` | DoctorError 变体 | 仍未处理 |
| UI Adapter | `shannon-core/src/ui_adapter.rs` | UiAdapter trait | 仍未处理 |

**P1-1 (2026-08-03)** 处理了用户授权的 4 接通 + 1 删除:
`/diff`、`/review_pr`、`/export`、`/debug` 全部接通并加单测;
`/pdf` 命令文件、注册、ROADMAP-FUTURE 条目一并清理。
未授权(coordinator/compact/doctor/ui_adapter)按计划 §P1 排期处理。

外加 `CLAUDE.md` "Gotchas" 承认:**61 个 `#[allow(dead_code)]`** 注解,虽有 `// KEEP:` 注释分类,但仍是"半成品模块"的信号。

**影响**:每个 dead 模块都是"看起来有、实际不能用"的功能,会让用户/贡献者困惑,也增加维护面积。

**修复策略**(二选一,见 [improvement-plan](./improvement-plan-2026-08.md) P1):
- **A. 接通**:按 ROADMAP 优先级把高价值模块(/diff、/review_pr、/export)接完。
- **B. 删除**:对长期不打算做的(/pdf 需外部 PDF 库),删掉 dead code,从 ROADMAP 移除,避免误导。
- **原则**:ROADMAP.md 自己定的"No new dead code"要执行 —— 不再新增"定义了不接通"的类型。

### 2.2 Dead hook events(5 个定义未发射)

[HOOK-AUDIT.md](./HOOK-AUDIT.md) 审计:Shannon 定义了 30 个 hook 事件(与 Claude Code 持平),但**5 个从未在生产代码发射**:

| 事件 | 应发射处 | 接通成本 |
|---|---|---|
| `UserPromptExpansion` | skill/command 模板展开后 | 低(~1h) |
| `InstructionsLoaded` | CLAUDE.md/rules 加载合并后 | 低(~1h) |
| `ConfigChange` | `.shannon.toml` 热重载 | 中(~3h,需 file watcher) |
| `Elicitation` | MCP `elicitation/create` | 中(~4h,需 UI bridge) |
| `ElicitationResult` | 同上,用户响应后 | 同上 |

**影响**:hook 系统是 32 事件卖点之一,dead event 削弱可信度。前 3 个低成本,应立即接通。

**修复**:见 [improvement-plan](./improvement-plan-2026-08.md) P1-2。

### 2.3 Compact 引擎只用默认策略

`shannon-core/src/compact.rs` 定义了 5 种 `CompactStrategy`,但**只有默认策略被调用**。token-based、summary-based 等策略是 dead code。

**影响**:context 压缩是长对话质量的关键。只用一种策略意味着不同对话类型(代码 review vs 闲聊)用同一种压缩,可能次优。

**修复**:见 [improvement-plan](./improvement-plan-2026-08.md) P2-1。

### 2.4 桌面状态层用 JSONL 而非 SQLite

[desktop-architecture.md](./desktop-architecture.md) 明确:`SessionManager` 用 **JSONL** 持久化,**不是 SQLite**。

**影响**:JSONL 对小规模会话够用,但会话数上去后,搜索/分页/并发写性能会成瓶颈。这是已知的架构选择,不是 bug,但应列入长期债。

**修复**:评估何时迁移 SQLite(见 [improvement-plan](./improvement-plan-2026-08.md) P3)。

### 2.5 桌面引擎 re-platforming 未完成

[CLAUDE.md](../CLAUDE.md) "Known Gaps":`shannon-desktop` 的 Provider/credential UX 与 CLI 的 parity **正在验证**(未完成),且"Full engine re-platforming onto the shared `ProviderProfile`/credential store is **ADR-0005 Phase 2(deferred)**"。

**影响**:桌面与 CLI 的 provider/credential 路径若未统一,会出现"CLI 能连、桌面连不上"或反之的体验割裂。ADR-0005 已记录此决策,但 Phase 2 推迟意味着债务在累积。

**修复**:见 [improvement-plan](./improvement-plan-2026-08.md) P2-2(把 ADR-0005 Phase 2 提上日程)。

---

## 3. 🟡 改进点(质量 / 体验)

### 3.1 测试工程缺口

[competitor-testing-research.md](./competitor-testing-research.md) 指出 Shannon 相对 Claude Code 的测试缺口:

| 缺口 | 说明 |
|---|---|
| **insta snapshot 测试** | 无,UI/输出回归靠人工 |
| **变异测试(mutation testing)** | 无,无法度量测试有效性 |
| **架构不变量测试** | 无,无法防"crate 间非法依赖"等架构腐蚀 |
| **mockito 顺序敏感** | `.expect(N)` matcher 顺序依赖,易脆 |
| **MCP mocking 薄弱** | 测试覆盖不足 |

**修复**:见 [improvement-plan](./improvement-plan-2026-08.md) P2-3。

### 3.2 文件体积 / 模块拆分

ADR-0008 P2-8 已识别并处理"超大文件拆分"(如 `config.rs`)。但审查建议把**文件体积门禁**常态化(CI 检查单文件 >N 行告警),避免再次累积。

### 3.3 `pre_resolve_context` 签名债

ADR-0008 明确 deferred:`pre_resolve_context` 返回 `()` 而非 `Result`,是长期项需改签名 + 全调用点。这是错误处理债(panic vs Result 的选择问题,关联 ADR-0008 P2-6 的 catch_unwind 日志)。

### 3.4 auto-commit hook 行为

工作流层面(非代码 bug):存在一个 PostToolUse hook 会在 Edit/Write 后触发,把多文件重构拆成多个泛化提交。多文件 PR 需 `git reset --soft HEAD~N && git commit` 合并。这是开发者体验问题,应在 hook 文档里显式说明(或提供 `--squash` 选项)。

### 3.5 CI 与本地门禁不对称

[desktop/CLAUDE.md](../desktop/CLAUDE.md) 承认:**CI 是 UI-only,因为 Gitea runner 连不上 github.com**;Rust 的 clippy/test 门禁靠**本地 pre-push hook**(`scripts/hooks/pre-push`)。这意味着:贡献者若忘了 `git config core.hooksPath scripts/hooks`,Rust 质量门就**被绕过**。

**影响**:开源协作场景下,外部 PR 可能跳过 Rust 检查。

**修复**:评估自托管 runner 或镜像依赖,让 CI 能跑 Rust 门禁(见 [improvement-plan](./improvement-plan-2026-08.md) P2-4)。

---

## 4. 🟢 未完成 / 推荐任务清单(汇总)

以下整合 ROADMAP / ROADMAP-FUTURE / ADR / 当前分支的所有 in-flight 与 deferred 项,**按"应做"优先级排序**(完整实施方案见 [improvement-plan-2026-08.md](./improvement-plan-2026-08.md))。

### 4.1 当前分支 in-flight
- [ ] **ADR-0008 交互 QA**:20 条行为验收(合并前必做)

### 4.2 高价值未完成(原 ROADMAP-FUTURE P1)
- [ ] **Auto-commit + 上下文消息**(Aider 风格)
- [ ] **Undo / 快照系统**(文件级,现仅有消息级 `/rewind`)
- [ ] **tree-sitter repo map**(代码全局理解,对标 Aider/Claude Code)
- [ ] **Auto-test loop**(编辑→测试→修复→循环)
- [ ] **Deep LSP 集成**(对标 OpenCode 25+ server 自装)
- [ ] **IDE 扩展**(VS Code 已 scaffold,需完善;JetBrains)

### 4.3 差异化未完成(原 ROADMAP-FUTURE P2)
- [ ] **HTTP API server**(`shannon serve`,程序化集成)
- [ ] **Architect 模式**(planner + editor 双模型)
- [ ] **接通 dead 命令**:/export、/diff、/review_pr、/debug
- [ ] **Compact 多策略接通**

### 4.4 工程质量未完成
- [ ] **Hook dead events 接通**(UserPromptExpansion / InstructionsLoaded / ConfigChange)
- [ ] **insta snapshot 测试引入**
- [ ] **度量基线统一**(单一权威 metrics 源)
- [ ] **文档与代码一致性校对**(desktop-architecture.md 等)
- [ ] **CI Rust 门禁**(解决 runner 连不上 github)
- [ ] **文件体积门禁常态化**

### 4.5 长期(原 ROADMAP-FUTURE P3)
- [ ] **Cross-surface continuity**(终端/IDE/Web/Mobile 会话连续)
- [ ] **Cloud 执行基础设施**(对标 Codex Cloud / Cursor background agents)
- [ ] **Agent SDK**(对标 Claude Code Agent SDK)
- [ ] **Skills marketplace**
- [ ] **Voice input**(whisper-rs;openworker 已验证 Rust STT 可行,可提前)
- [ ] **桌面 SQLite 迁移**
- [ ] **ADR-0005 Phase 2**(桌面引擎 re-platforming)

### 4.6 竞品驱动新增(来自 [openworker 调研](./openworker-research.md))
- [ ] **MCP 化补 SaaS 集成**(GitHub Issues/Slack/Jira/Notion/Linear)
- [ ] **artifact 显性化**(交付成品 > 聊天)
- [ ] **无人值守 inbox**(长跑 agent 安全闭环)

---

## 5. 值得肯定的优点(保持)

为避免只批不赞,以下强项应保持:

1. **安全设计扎实**:密钥 0600 落盘、`redact_secret_command` 脱敏、fail-soft 校验、5 级权限 + LLM 分类器。这是企业可用性的基础。
2. **测试投入领先**:Record/Replay(JSONL fixtures)、YAML 场景测试、mockito HTTP mock —— 编码 agent 里独有,CI 无需 API key。
3. **架构分层清晰**:9 crate 职责单一,Tool trait 抽象干净,adapter 模式让多 provider 可扩展。
4. **稳定度策略明确**:[STABILITY.md](./STABILITY.md) 的 3 tier 标记 + cargo-semver-checks blocking + 基线 pin,是少见的 pre-1.0 项目有正式 API 稳定度承诺。
5. **审计文化**:HOOK-AUDIT、ADR、QA checklist 显示团队有"记录决策与验收"的纪律。
6. **Rust 差异化**:`~3.4MB 单二进制、内存安全、跨平台最完整` —— 这是开源编码 agent 里唯一的 Rust 实现,是真实护城河。

---

## 6. 审查结论

Shannon 处于**"功能宽度足够、深度待打磨"**的阶段。架构和安全设计是强项,但**文档漂移、dead code 沉淀、度量基线不统一**三类问题正在侵蚀项目可信度,需要一轮"收敛与对齐"工作。

建议的下一步不是"加更多功能",而是:
1. **先合并 ADR-0008**(完成交互 QA)。
2. **做一轮 dead code 清理 + 文档校对**(收敛)。
3. **统一度量基线**(对齐)。
4. 再按 [improvement-plan-2026-08.md](./improvement-plan-2026-08.md) 推进高价值功能。

这三步是 [improvement-plan](./improvement-plan-2026-08.md) P0–P1 的核心。
