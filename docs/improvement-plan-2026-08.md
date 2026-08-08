# Shannon 综合改进任务列表与实施方案(v3)

> 编制日期:2026-08-03(v3,Wave 1 收口 + 全量状态盘点)
> 编制视角:高级产品经理 + 高级架构师
> **用途:供 ericdong 评审。本文档只产出计划,不自动执行。**
> **v3 变更**:① Wave 1 收口标注(13 项已落地,2 项待办);② Wave 2/3 实际进度刷新(从 `dev` 历史倒推);③ 新增 §9「Wave 4 推荐」,给出下一阶段 8 选 N 的决策菜单;④ §5 路线图改为「已完成 / 在做 / 推荐下批」三栏。
> 输入:[openworker-research.md](./openworker-research.md)、[competitor-feature-matrix.md](./competitor-feature-matrix.md)、[project-review-2026-08.md](./project-review-2026-08.md)、[aichat-ui-library-evaluation.md](./aichat-ui-library-evaluation.md)、[metrics.md](./metrics.md)
> 前置版本:[improvement-plan-2026-08.md](./improvement-plan-2026-08.md) v1 / v2(本文件即 v3)

---

## ⚡ v4 增量更新(2026-08-08):Wave 4 候选实际落地状态

> v3(08-03)的 §8「Wave 4 评审请求」列出 8 项候选;截至 08-08,**6 项已合入 `dev`,2 项进入 spike 阶段**。下表覆盖 v3 各节(§0.5 / §5 / §9)的实际状态;各节原文保留作历史快照,不再逐行改写。

### ✅ 本周合入 dev(08-04 → 08-07)

| v3 候选 | 状态 | 证据 |
|---|---|---|
| **P0-2** 文档校对 | ✅ | `5aec9a08`;`grep "React 18" docs/desktop-architecture.md` 已无结果 |
| **P1-3c** Notion MCP | ✅ | `2417d5b2` → P1-3 **全量 5/5** |
| **P1-3d** Linear MCP | ✅ | `019ca037` |
| **P2-4** CI Rust 门禁 | ✅(剩 `-D warnings` required 一公里,见 [tech-debt](./tech-debt.md)) | `2bf92611`(doc build / rustsec-audit / cross-platform matrix) |
| **P2-5d** Chat 整体美化 | ✅ | `135b288d` → P2-5 **全量** |
| **P2-5e** 本地语音 | ✅ | `d983f527` `4946c768`(whisper-rs) |
| **P2-2** ADR-0005 Phase 2 读写收敛 | ✅ | #34 Wave 6 + ADR-0009 read facade(#41)+ C2 dual-write drop(#37)+ **C3 parity 全 6 行**(#42 / #46 / #49) |
| (附带)semver required | ✅ | #40(S1-6/B1/B2,baseline v0.8.0) |
| (附带)**v0.8.0 发布** | ✅ | `f1f84e14` |

### 🔬 进入 spike,未实施

| v3 候选 | 状态 | 文档 |
|---|---|---|
| **P2-8** VS Code 扩展 | 🔬 spike 完,待 S1 实施(依赖 P2-7 ✅) | [spikes/p2-8-vscode.md](./spikes/p2-8-vscode.md) + `pres1-validation` |
| **P3-7** 沙盒执行 | 🔬 S0 完,待评审 → D2 | [spikes/p3-7-sandbox-s0.md](./spikes/p3-7-sandbox-s0.md) |

### ❓ v3 / project-review 标为债,实际已清偿(核实 2026-08-08)

[project-review-2026-08.md](./project-review-2026-08.md) §2.1 / §2.3 把下列标为 dead code / 半成品,经核实**已清理**:

- **Compact 多策略**(§2.3):`crates/shannon-core/src/compact.rs` 中 `Strategy::TokenBased` / `SummaryBased` 均按决策动态选用(`strategy: decision.strategy` @ 521 行),非只用默认。P2-1 实接通。
- **coordinator / compact / doctor / ui_adapter**(§2.1):四个文件 **0 个 `allow(dead_code)`**,已无 dead code 沉淀。

### 📊 进度仪表盘刷新(2026-08-08)

```
Wave 1(收敛)  ██████████ 7/7  ≈ 100%  (P0-2 补齐)
Wave 2(补短)  ██████████ 9/9  ≈ 100%  (P1-3 全量)
Wave 3(扩张)  ████████░░ 7/8  ≈ 88%   (仅 P2-8 待实施)
──────────────────────────────────────
合计          █████████░ 23/24 ≈ 96%
```

**剩余第 24 项 = P2-8 VS Code 扩展**(spike 已就绪)。P2-6(auto-commit + Undo)是 v3 之后新增的工程体验项,未计入上表分母。

### 🔜 下批规划

见 [plans/wave-5-followup.md](./plans/wave-5-followup.md):Wave 5(文档/度量收敛)、Wave 6(功能收尾:P2-8 / P2-6 / P2-2 deprecation)、Wave 7(P3-7 沙盒 / P3-1 artifact)。

---

## 0. 战略主线(沿用 v2,双赛道)

> **"先收敛、再补短、后扩张"** —— 两条产品线,一套引擎,分线补短。

**双产品线**:
- **shannon-code**(编程赛道):对标 Claude Code/Codex CLI/OpenCode/Reasonix。必修:repo map、auto-test loop、IDE 扩展、dead code 清理。
- **shannon-desktop**(办公赛道):对标 Claude Desktop/Codex Desktop/openworker/Hermes/WorkBuddy。必修:**SaaS 集成**、**沙盒**、**chat 升级(附件/语音/多线程/美化)**、artifact 显性化。

**三条纪律**:
1. **不追 openworker 办公赛道的全量 SaaS**(25+ 太重),用 **MCP 低成本补开发者/办公高频 SaaS**。
2. **坚持 Rust 差异化**(轻量/可审计/单二进制),语音走 whisper-rs 本地方案。
3. **chat 升级借 assistant-ui 骨架**(ROI 已翻转,见 [aichat-ui 评审 v2](./aichat-ui-library-evaluation.md)),不自研全部。

**用户已授权的决策(v1/v2 评审)**:
- ✅ 战略主线
- ✅ P1-3 SaaS 集成 5 顺序(GitHub/Slack/Jira/Notion/Linear)
- ✅ P1-1 dead code 裁决(/pdf 删、/diff+/review_pr+/export 接通)
- ✅ P2-5 Chat UI(以 assistant-ui 为骨架)
- ⏸ 资源评估暂缓
- ✅ Wave 1 收口 7 项(2026-08-03)

---

## 0.5 Wave 1 收口状态快照(2026-08-03)

> 本节为 v3 新增,记录本轮 dev 上的合入与未做项。

### ✅ 已合入 dev(7 项,本轮执行)

| 编号 | 任务 | 提交 | 文件/产物 |
|---|---|---|---|
| **P0-3** | 统一度量基线 | `2603cac3` | `docs/metrics.md`(11093 runnable / 409510 LOC / 605 files),`scripts/gen-metrics.sh`,`.github/workflows/metrics.yml` |
| **P2-5a** | 桌面 chat runtime adapter | `5ef1aaec` | `desktop/ui/src/lib/runtime/ChatV2RuntimeProvider.tsx`(feature flag),1210 前端测试通过 |
| **P1-2** | Hook events 接通 | `7327fcea` | docs 标注已 wired(UserPromptExpansion/InstructionsLoaded/ConfigChange,代码已在 main 上) |
| **P1-1** | Dead code 清理 | `448061e5` | /pdf 删除;/diff、/export、/review_pr、/debug 接通 |
| **P1-3** | Slack MCP(v2 对齐 Jira 模板) | `e11fa1b4` | 6 tools(post_message/search_messages/read_channel/thread_reply/list_channels/get_user_info);`docs/integrations/slack.md` 293 行 |
| **P2-3** | insta + 架构不变量 | `c6889242` | `crates/shannon-core/tests/architecture_invariants.rs`(5 个不变量);insta snapshot infra;MockMcpServer |
| **P0-1** | ADR-0008 交互 QA | `91e88670` | `scripts/adr-0008-qa.sh`(266 行,isolated HOME);checklist sign-off |

### 🔧 收口过程的关键 fixup

- **`f8e2bdca`** fix(mcp-saas): add KEEP marker — P2-3 不变量在 P1-3 Slack 上检出 `ApiEnvelope<T>` 缺 `// KEEP:` 标记,补注「documentation helper struct — fields probed on raw JSON elsewhere」。正是 P2-3 设立不变量想拦下的「裸 allow(dead_code)」。

### ❌ Wave 1 原计划未完成项

| 编号 | 任务 | 状态 | 备注 |
|---|---|---|---|
| **P0-2** | 文档与代码一致性校对 | **未做** | `docs/desktop-architecture.md` 仍写 React 18(实为 19);SPEC.md crate 行数未同步;CLAUDE.md 测试数引用应改 `metrics.md` |

### 📊 dev 分支当前基线(2026-08-03)

```
commit: f8e2bdca fix(mcp-saas): add KEEP marker to slack::api::ApiEnvelope allow(dead_code)
tests: 11084(11083 passed + 1 pre-existing scheduled_budget wall-clock drift)
clippy: cargo clippy --workspace -- -D warnings → pass
fmt: cargo fmt --all -- --check → pass
本地分支: dev(活跃), main(在 f8e2bdca 之前,等待下一次 release merge)
本地 worktree: 1(主 shannon-mono)
未跟踪: .rustup/(无关)
```

---

## 1. Wave 1:收敛与对齐(本月,~1.5w)

目标:建立可信度基线,解除合并阻塞,做实卖点。

| 编号 | 任务 | 状态 | 备注 |
|---|---|:---:|---|
| **P0-1** | ADR-0008 交互 QA | ✅ | `91e88670`, `acb6f434` 已合 dev |
| **P0-2** | 文档与代码一致性校对 | ❌ | **Wave 1 唯一未做项,推荐立即收尾** |
| **P0-3** | 统一度量基线 | ✅ | `2603cac3`, `1a552166` 已合 dev |
| **P1-1** | Dead code 清理 | ✅ | `448061e5`, `76b168de` 已合 dev(/pdf 删、4 命令接通) |
| **P1-2** | Hook dead events 接通 | ✅ | `7327fcea`, `32901a8f` 已合 dev(代码已在 main) |

### P0-2 · 文档与代码一致性校对(本轮唯一待办,0.5d)
- **描述**:修复文档漂移(最严重:`desktop-architecture.md` 写 React 18,实际 React 19)。
- **文件锚点**:
  - `docs/desktop-architecture.md`(React 18→19;核查组件示例 vs `desktop/ui/src/components/`)
  - `docs/SPEC.md`(crate 行数同步)
  - `CLAUDE.md`(测试数引用,见 P0-3)
- **实施步骤**:
  1. `grep -rn "React 18" docs/` → 全部改为 "React 19"。
  2. 对照 `desktop/ui/src/components/` 实际组件树,修正 `desktop-architecture.md` 的组件示例。
  3. `cloc crates/` 产出实际行数,同步 SPEC.md 表格。
  4. CLAUDE.md 的"~7889 测试"改为引用 `docs/metrics.md`(P0-3 产物)。
- **验收**:
  - [ ] `grep -rn "React 18" docs/` 无结果
  - [ ] SPEC.md 行数与 `cloc` 一致
- **估时**:0.5d · **依赖**:P0-3 ✅ · **风险**:低

---

## 2. Wave 2:补齐短板(~6–8w) — 实际进度刷新

目标:补两条赛道的硬短板(编码线 repo map/auto-test;办公线 SaaS 集成)。

| 编号 | 任务 | 状态 | 备注 |
|---|---|:---:|---|
| **P1-3** | MCP 化补 SaaS 集成 | 🟡 3/5 | ✅ GitHub `6eb6cf61` / ✅ Jira `9544c60c` / ✅ Slack `e11fa1b4` / ❌ Notion / ❌ Linear |
| **P1-4** | tree-sitter repo map | ✅ | Phase A `9ed62725` + Phase B(TS/Python/Go)`69797143`,增量更新,已注入 query_engine |
| **P1-5** | auto-test loop | ✅ | `3a00b207`,编辑 → 跑测试 → 修失败循环接通 |
| **P2-1** | Compact 多策略 | ✅ | `f51a504b`,token-based / summary-based / selector facade |
| **P2-3** | insta + 不变量 | ✅ | `c6889242`(本轮),5 个架构不变量 + insta + MCP mock |
| **P2-4** | CI Rust 门禁修复 | ❌ | **Gitea runner → github.com 拉依赖未通,本轮无进展** |

### P1-3 后续(2 个 SaaS 待补,~4d)

#### P1-3c · Notion MCP(2d)
- **文件锚点**:`crates/shannon-mcp-saas/src/notion/`(新建);`docs/integrations/notion.md`;测试
- **实施步骤**:
  1. 复用 Slack/Jira 模板:`mod.rs` + `auth.rs`(internal integration token 走 keyring)+ `api.rs`(Notion API base `https://api.notion.com/v1`,Bearer token)+ `tools.rs`(注册)+ `tests.rs`(MockMcpServer)。
  2. 6 工具:`search_pages` / `get_page` / `append_block` / `create_page` / `list_databases` / `query_database`。
  3. 速率限制:Notion 3 req/s,内置令牌桶。
  4. docs/integrations/notion.md:配置 + 权限 + curl 例子。
- **验收**:`tools/list` 可被 Shannon 发现;mockito 测试通过;REPL 一行命令 create page 端到端。
- **估时**:2d · **依赖**:无 · **风险**:低

#### P1-3d · Linear MCP(2d)
- **文件锚点**:`crates/shannon-mcp-saas/src/linear/`(新建);`docs/integrations/linear.md`
- **实施步骤**:
  1. Linear GraphQL endpoint `https://api.linear.app/graphql`,复用模板。
  2. 5 工具:`list_issues` / `get_issue` / `create_issue` / `update_status` / `list_teams`。
  3. GraphQL 客户端复用 reqwest(无需独立 SDK),Query/Mutation 模板字符串化。
- **验收**:`tools/list` 可被发现;mutate 走 mock 回放;REPL 实测。
- **估时**:2d · **依赖**:无 · **风险**:低

---

## 3. Wave 3:扩张与差异化 — 实际进度刷新

| 编号 | 任务 | 状态 | 备注 |
|---|---|:---:|---|
| **P2-5a** | Runtime adapter | ✅ | `5ef1aaec`,feature flag + ChatProvider + insta snapshot |
| **P2-5b** | 多线程管理 | ✅ | `d77be870`,per-session event queue + SessionsPanel spike |
| **P2-5c** | 附件上传 | ✅ | `7ee92e9e` + `7fb276d3` fix,MAX_ATTACHMENT_COUNT mock 修复 |
| **P2-5d** | 整体美化升级 | ❌ | **未做,需 design tokens + MessageBubble 重构 + a11y** |
| **P2-5e** | 语音消息 | ❌ | **未做,whisper-rs 集成未启** |
| **P2-2** | ADR-0005 Phase 2 桌面 re-platforming | 🟡 | **大头已做**(P1.2-A/B/C / P1.3 / P4.11-13),Phase 2 收尾段待走 STABILITY deprecation |
| **P2-6** | auto-commit + Undo/快照 | ❌ | **未做,先要梳理现有 PostToolUse auto-commit hook 行为** |
| **P2-7** | `shannon serve` HTTP API | ✅ | `3ed22799`,authenticated v1 session API |
| **P2-8** | VS Code 扩展完善 | ❌ | **依赖 P2-7,现在可启** |

### P2-2 · ADR-0005 Phase 2 收尾(2–3w)
- **现状**:Phase 1 全部完成(`provider_config_service.rs` 单一写入路径、`/connect` 热加载、Welcome 重写、test_all_providers、DesktopConfig 字段删除)。
- **剩余**:走 STABILITY deprecation 周期 + `cargo-semver-checks` 把关;CLI 与桌面 provider/credential 行为逐项对齐。
- **文件锚点**:`crates/shannon-core/src/provider.rs`(ProviderProfile);`desktop/src/commands_config.rs`、`commands_providers.rs`
- **验收**:CLI 与桌面 connect/model/tier/refresh 行为逐项对齐;无双路径。
- **估时**:2–3w · **依赖**:无 · **风险**:中(跨 crate 签名)

### P2-8 · VS Code 扩展(2–3w)
- **现状**:`legacy-archives/` 下有早期 VS Code 扩展源。
- **文件锚点**:迁移到 `extensions/vscode/`(活跃目录);通信改用 P2-7 的 HTTP API(已 ✅,比 NDJSON 子进程更稳)。
- **依赖**:P2-7 ✅ · **风险**:中(发布流程)
- **估时**:2–3w

---

## 4. P3 — 长期(记录,不排期)

| 编号 | 任务 | 估时 | 来源 | v3 备注 |
|---|---|---|---|---|
| P3-1 | artifact 显性化(交付成品>聊天) | 2w | openworker §9.3 | 依赖 P2-5d(美化);P2-5d 启动后可提级 |
| P3-2 | 无人值守 inbox(长跑 agent 安全闭环) | 1.5w | openworker §9.4 | 依赖 P1-3(✅ 3/5) |
| P3-3 | Computer Use 完善(跨平台) | 3w | desktop-product-analysis | macOS AX 替代方案待研 |
| P3-4 | 桌面状态层迁移 SQLite | 2w | project-review §2.4 | 当前 JSON 状态层是规模上限 |
| P3-5 | Deep LSP 集成(对标 OpenCode 25+) | 3–4w | matrix §2.2 | 与 P1-4 repo map 互补 |
| P3-6 | 移动端派发(对标 WorkBuddy) | 8–12w | WorkBuddy | 远期 |
| P3-7 | 沙盒执行(Landlock/seccomp/Seatbelt) | 4–6w | matrix §2.2 安全基础 | P2-7 已开 HTTP 面,P3-7 是执行面 |
| P3-8 | Cross-surface / Cloud / Agent SDK / Skills marketplace | 各 4–12w | ROADMAP-FUTURE P3 | 远期 |

---

## 5. 实施路线图(更新为「已完成 / 推荐下批 / 长期」三栏)

### ✅ 已完成(Wave 1 + Wave 2 主体 + Wave 3 部分)

**Wave 1(7 项 6/7 落地)**:
- P0-1 ✅ · P0-3 ✅ · P1-1 ✅ · P1-2 ✅ · P2-5a ✅ · P2-3 ✅
- 仅 P0-2(文档校对)未做

**Wave 2 主体**:
- P1-4 ✅ · P1-5 ✅ · P2-1 ✅ · P2-3 ✅
- P1-3 3/5(GitHub ✅ / Jira ✅ / Slack ✅)

**Wave 3 部分**:
- P2-5b ✅ · P2-5c ✅ · P2-7 ✅

### 🔜 推荐下批(Wave 4 候选,8 项)

| 任务 | 估时 | 优先级 | 依赖 | 一句话理由 |
|---|---|---|---|---|
| **P0-2** 文档校对 | 0.5d | 🔴 高 | P0-3 ✅ | 立即可做,关 Wave 1 |
| **P1-3c** Notion MCP | 2d | 🔴 高 | 无 | P1-3 只差这 2 个即全量完成 |
| **P1-3d** Linear MCP | 2d | 🔴 高 | 无 | 同上 |
| **P2-4** CI Rust 门禁 | 1–2w | 🟡 中 | 无 | 当前 dev 上 11083 通过无价值(无人验证),CI 通了才闭环 |
| **P2-5d** 整体美化 | 5–7d | 🟡 中 | P2-5a/b/c ✅,P2-3 ✅ | P2-5 最后一块,补完 → 触发 P3-1 |
| **P2-5e** 语音消息 | 5–7d | 🟡 中 | 无 | whisper-rs 本地方案,办公线差异化 |
| **P2-2** ADR-0005 Phase 2 收尾 | 2–3w | 🟢 稳 | 无 | Phase 1 已完成,走 deprecation 周期 |
| **P2-8** VS Code 扩展 | 2–3w | 🟢 稳 | P2-7 ✅ | 现可启 |

### ⏸ 长期(P3)

P3-1..P3-8 见 §4。**P3-7 沙盒执行** 是 P2-7 的执行面补充,可与 P2-7 联评。

---

## 6. 依赖关系图(关键路径,v3 更新)

```
✅ P0-1(ADR-0008 QA)──┬─→ P2-2(桌面 re-platforming Phase 2)
✅ P0-3(metrics)─────┴─→ P2-3(snapshot 测试)─→ P2-5d(美化,需视觉基线)
❌ P0-2(文档)──────────→ (独立,0.5d 立即收)

✅ P1-1(dead code)─────→ (独立)
✅ P1-2(hook events)────→ (独立)

🟡 P1-3(SaaS MCP)──┬─→ P3-2(无人值守 inbox)
                   └─→ P3-7(沙盒执行)
✅ P1-4(repo map)──┴─→ P2-1(Compact 多策略)

✅ P2-5a(runtime adapter)─→ ✅ P2-5b(多线程)─→ ✅ P2-5c(附件)─→ ❌ P2-5d(美化)─→ P3-1(artifact)
❌ P2-5e(语音)───────────→ (独立子任务)

❌ P2-4(CI 门禁)──────────→ 阻碍所有「外部验证」
✅ P2-7(serve)───────→ ❌ P2-8(VS Code)
```

**关键路径**:
- 编码线:✅ P1-4 → ✅ P2-1(repo map → 压缩质量)→ **完成**
- 办公线:✅ P2-5a → ✅ P2-5b → ✅ P2-5c → ❌ P2-5d → P3-1(差最后一块美化)
- 跨切面:**P2-4 CI 门禁** 是 dev → origin 流程最大隐患;**P0-2** 是 Wave 1 关单的最后小项

---

## 7. 风险登记册(v3 更新)

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| P2-5a runtime 接口 beta 漂移 | 低(已锁) | 高 | 已用 insta snapshot + feature flag |
| P2-4 CI runner 依赖运维 | **高(已 1+ 月未动)** | 高 | 临时:PR 模板要求贴本地 clippy/test 输出;中期评估 cargo vendor / mirror |
| 范围蔓延 | 中 | 中 | 严格按本计划;新需求进 backlog |
| auto-commit hook 拆分提交 | 高 | 低 | P2-6 先梳理现有 hook 行为 |
| **新增**:P2-5d 美化基线缺位 | 中 | 中 | P2-3 已就位 insta,视觉基线可由 P2-5d 一次性建 |
| **新增**:P2-8 VS Code 扩展发布流程 | 中 | 中 | 复用现有 desktop release 流程 |

---

## 8. Wave 4 评审请求(v3 新增)

请 ericdong 就下批(Wave 4)候选 8 项做决策:

1. **P0-2** 文档校对(0.5d)—— 是否立即收 Wave 1 的尾?
2. **P1-3c/d** Notion + Linear MCP(共 4d)—— 是否一次做完 P1-3 全量?
3. **P2-4** CI Rust 门禁(1–2w)—— 是否启动 vendor / mirror 评估?需运维配合。
4. **P2-5d** 整体美化(5–7d)—— 是否接着 P2-5a/b/c 的势头一鼓作气?
5. **P2-5e** 语音消息(5–7d)—— 是否启用 whisper-rs 路线(模型按需下载)?
6. **P2-2** ADR-0005 Phase 2 收尾(2–3w)—— 走 STABILITY deprecation 周期,需要团队评审签名。
7. **P2-8** VS Code 扩展(2–3w)—— 现依赖 P2-7 已完成,可启。
8. **P3-7** 沙盒执行(4–6w)—— 是否提前到 Wave 4 与 P2-7 联评(执行面+网络面)?

**推荐组合(按 ROI / 风险)**:
- **A · 闭环 Wave 1 + 完成 P1-3**:P0-2(0.5d)+ P1-3c(2d)+ P1-3d(2d)≈ **4.5d 拿到 Wave 1–2 全绿**
- **B · chat 升级冲刺**:A + P2-5d(5–7d)+ P2-5e(5–7d)≈ **2.5w 拿到 chat 体验跃升**
- **C · 基建加固**:A + P2-4(1–2w)+ P2-8(2–3w)≈ **5–6w 拿到 CI 闭环 + IDE 入口**
- **D · 战略收尾**:A + P2-2(2–3w)≈ **3–4w 拿到 ADR-0005 全量闭环**

**评审通过后,我会**:
- 把确认的 Wave 4 任务拆成独立 `docs/plans/<task>.md` 可执行方案(仿 `chat-upgrade.md` / `repo-map.md`);
- 建立 Wave 4 的任务跟踪(`TaskCreate` 每个任务一项);
- 按授权开始执行(**评审通过前不开工**)。

---

## 9. 当前进度仪表盘(2026-08-03)

```
Wave 1(收敛)        ████████░░ 6/7  ≈ 86%
Wave 2(补短)        ████████░░ 7/9  ≈ 78%  (P1-3 3/5 算 60%,整体按项算 78%)
Wave 3(扩张)        ████░░░░░░ 4/8  ≈ 50%
─────────────────────────────────────
合计                ██████░░░░ 17/24 ≈ 71%
```

**下一批(用户授权后)目标**:推至 **22/24 ≈ 92%**(本批 8 项中除 P2-8 外全做)。

---

*v3 由 Wave 1 收口 + 全面 git history 倒推驱动;所有 ✅ 状态对应已合入 dev 的 merge commit,所有 ❌ 状态对应 grep/git log 无证据。*

*v2 由双赛道分析 + chat 升级需求驱动;v1 是初版。请评审 Wave 4 候选组合(A / B / C / D)并选择授权。*