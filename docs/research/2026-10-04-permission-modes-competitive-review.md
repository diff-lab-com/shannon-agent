# 权限模式竞品调研与 Shannon 现状审查（2026-10-04）

> 配套改进方案：[docs/plans/2026-10-04-permission-modes-improvement-plan.md](../plans/2026-10-04-permission-modes-improvement-plan.md)
> 调研范围：Claude Code、OpenAI Codex CLI、Gemini CLI、OpenCode、Cursor、Cline/Roo Code；Shannon 现状基于 dev 分支代码审查（file:line 均已核实）。

---

## 1. 竞品权限模式分级

### 1.1 Claude Code —— 6 档谱系模式 + 规则叠加（行业基准）

模式从严到宽：`plan` → `default`(Manual) → `acceptEdits` → `auto` → `dontAsk` → `bypassPermissions`。

| 模式 | 自动执行 | 弹窗询问 | 拒绝 |
|---|---|---|---|
| `plan` | 只读；计划模式下可用分类器批准的低危命令 | 编辑（计划批准前封锁） | 计划期间被拒命令 |
| `default` (Manual) | 只读 | 文件编辑、shell、网络 | — |
| `acceptEdits` | 读、文件编辑、工作目录内常见文件系统命令（mkdir/touch/rm/mv/cp/sed 等） | 范围外路径、保护路径、其余 Bash | — |
| `auto` | 一切，由后台分类器模型替代人做安全判断 | 关键路径 rm（2 分钟倒计时）、保护路径、ask 规则、需交互的 MCP 工具等 | 分类器拦截的一切 |
| `dontAsk` | 读 + 预批准工具（allow 规则、只读 Bash） | **从不等待** | 一切本该弹窗的操作（改为拒绝） |
| `bypassPermissions` | 一切（含保护路径写） | 仅"任何模式都不自动批准"的底线清单 | `--restricted` 会话整体拒绝此模式 |

关键设计（值得对标的点）：

- **规则叠加在模式之上**：allow/ask/deny 规则与模式正交。**deny 规则在所有模式生效，包括 bypassPermissions**；ask 规则即使在 auto/dontAsk 下也强制弹窗（dontAsk 下转为拒绝）。
- **任何模式都不自动批准的硬底线**：ask 规则匹配、需用户交互的 MCP 工具、关键路径 `rm/rmdir`、跨会话消息防护等。
- **bypassPermissions 护栏**：root/sudo 下拒绝运行；首次使用弹"责任自担"确认；启动时未启用则中途不可进入；组织管理面可禁用（`disableBypassPermissionsMode`）；`--restricted` 会话拒绝；文档建议容器隔离。
- **切换**：Shift+Tab 循环；`--permission-mode` 启动旗标；`permissions.defaultMode` 设置键（项目文件里的 auto/bypass 会被忽略，只能放用户级——防项目投毒）。
- **dontAsk 定位**：CI 专用，配精确 allowlist（如 `--permission-mode dontAsk --allowedTools "Bash(npm test)" "Read"`）。

### 1.2 OpenAI Codex CLI —— 双轴模型（沙箱 × 审批策略）

不分档位谱系，而是两个正交轴：

- **沙箱轴** `sandbox_mode`：`read-only` / `workspace-write`（可配额外可写根、网络开关）/ `danger-full-access`。
- **审批轴** `approval_policy`：`untrusted` / `on-failure` / `on-request` / `never`。
- 预设：`--full-auto` = workspace-write + on-failure；`--yolo` = 无沙箱无审批。
- 会话内 `/permissions` 可调。OS 级沙箱（Seatbelt/bubblewrap/Landlock）是安全口碑来源。

### 1.3 Gemini CLI —— 3 档线性谱系

`default`（每次询问）→ `auto_edit`（自动批准文件编辑，其余询问）→ `yolo`（全部自动批准）。`--approval-mode` 旗标切换。曾出过 broad-shell ALLOW 规则被拒 + AUTO_EDIT 切换通知的安全修复（issue #19799），新增 `/approved` 命令。

### 1.4 OpenCode —— 无模式，纯规则

`permission` 配置块按工具分 `allow`/`ask`/`deny` 三值，bash 支持通配符模式（`"*": "deny", "git push": "ask"`），agent 级可覆盖全局。没有 OS 沙箱，规则系统是唯一控制面。已知坑：子代理继承主代理 deny 规则会覆盖自身 allow（`deriveSubagentPermission`）。

### 1.5 Cursor（GUI 系）—— 3 档 + allow/deny 清单

询问每次 → auto-run + allowlist（支持通配）→ auto-run 全部；**denylist 永远压过 allowlist**；新语法 `Shell(cmd)`/`Read(path)`/`Write(path)`/`Mcp(server:*)` 粒度规则；社区在请求时限 allowlist。

### 1.6 Cline / Roo Code —— 按动作类型的开关矩阵

五类开关（读 / 写编辑 / 执行命令 / 浏览器 / MCP 工具）逐个设置是否自动批准，外加：命令 deny 列表、**max-requests 连续自动批准熔断上限**（防失控/防烧钱）、大改动视为不安全仍弹窗。默认全关。

### 1.7 分级总览

| 产品 | 档位数 | 权限模型 | 特色 |
|---|---|---|---|
| Claude Code | 6 | 谱系模式 + allow/ask/deny 规则叠加 + 硬底线清单 | deny 全模式生效；bypass 重护栏；项目级 auto/bypass 不生效 |
| Codex CLI | 3×4 | 沙箱轴 × 审批轴 | OS 级沙箱；两轴正交 |
| Gemini CLI | 3 | 线性谱系 | 简单清晰 |
| Cursor | 3 | ask → allowlist → all + denylist 压制 | GUI 内嵌清单管理 |
| Cline/Roo | 5 开关 | 按动作类型矩阵 + 熔断上限 | max-requests 熔断 |
| OpenCode | 0 | 纯 allow/ask/deny 规则（含 bash 通配） | 极简一致 |
| **Shannon** | **9** | 模式 + 5 级风险分类器 + 规则（未接线）+ profiles（未生效） | 分类器最细；但接线率低 |

---

## 2. Shannon 现状审查

### 2.1 模式清单（`crates/shannon-engine/src/permissions.rs:101-130`）

`ApprovalMode` 9 个变体，默认 **`AutoEdit`**（permissions.rs:112-113）。执行逻辑集中在 `classify_and_check`（permissions.rs:1840-2043）。

| 变体 | 别名 | 实际语义 | 状态 |
|---|---|---|---|
| `Suggest` | default/ask | 只放行只读快路径；其余走分类器+弹窗 | ✅ 正常 |
| `Plan` | plan | 计划已批准→全放行；否则等同 Suggest | ⚠️ 审批通道是死代码，实际 ≡ Suggest |
| `AutoEdit` | auto（**默认**） | 文件工具风险≤Medium 自动批准；bash 等弹窗 | ✅ 正常 |
| `FullAuto` | full-auto | 风险<Critical 全自动；Critical 拒绝 | ✅ 正常 |
| `BypassPermissions` | bypassPermissions | 直接放行一切，跳过全部检查 | ⚠️ 连 deny 规则也跳过 |
| `DontAsk` | dontAsk | 门控上与 Bypass 完全相同（1886-1889 行同一分支） | ⚠️ 与语义不符（Claude Code 的 dontAsk 是"拒绝而非等待"） |
| `Readonly` | readonly | 仅只读快路径，其余硬拒绝 | ✅ 正常 |
| `Auto` | auto-classifier | 分类器驱动：Safe/Low 放行、Medium/High 弹窗、Critical 拒绝 | ⚠️ 不在循环内、标签与 FullAuto 冲突 |
| `PlanReadonly` | plan-readonly | 同 Readonly | ⚠️ 与 Readonly 近重复，仅 `/mode` 可达 |

支撑件：Shift+Tab 循环 `Suggest→AutoEdit→Plan→FullAuto`（cycle_next，permissions.rs:185-198）；状态栏标签 ASK/EDIT/PLAN/AUTO/FULL（short_label，201-213）；决策管线 `规则检查 → 模式覆盖 → 分类器 → 弹窗/拒` （guard_nodes.rs:98-186）；每次决策发 `permission/decision` 审计事件（guard_nodes.rs:194-217）。

### 2.2 决策管线

```
工具调用
 ├─ Plan 模式写闸（agent_loop.rs:2626-2679，独立于 ApprovalMode::Plan）
 ├─ PermissionGateNode → classify_and_check（permissions.rs:1840-2043）
 │    1. PermissionRuleChecker（deny>ask>allow）—— ⚠️ 生产未接线，恒为空
 │    2. 模式覆盖分支（1883-2009）
 │    3. 默认分类器路径（2011-2043，RiskLevel 5 级）
 ├─ 判定：Critical 弹窗会被 guard_nodes.rs:154-166 升级为硬拒
 ├─ 弹窗投递（agent_loop.rs:2737-2954）：Deny/AllowOnce/AlwaysAllow/EditAndRun
 │    无审批通道（REST、自动化、进程内子代理）→ fail-closed 拒绝（2955-2980）
 └─ 审计：emit_decision（guard_nodes.rs:194-217，⚠️ 记录的是有损短标签）
```

### 2.3 配置面（实际生效 vs 名义存在）

| 入口 | 键/旗标 | 实际状态 |
|---|---|---|
| CLI | `--permission-mode`（hidden） | 生效（headless/team 路径）；未知值在 noninteractive 报错，但 team-agent 静默降级 FullAuto |
| CLI | `-y/--yes` | 生效：headless 默认 FullAuto，`-y` 升 BypassPermissions |
| REPL | `/mode`、Shift+Tab、`/perms` | 生效，但仅会话内存，**不持久**；`/perms mode plan` 映射到 Readonly 与 `/mode plan` 冲突 |
| settings.json | `permissions.allow/deny` | 部分生效（走 PermissionMemory）；**`ask` 列表被忽略** |
| settings.json | `permissionsMode` | **死键**（settings.rs:126-127，仅 doctor 打印） |
| config.toml | `permission_profile` + `SHANNON_PERMISSION_PROFILE` | **死配置**（resolve 无生产调用者） |
| profiles | `.shannon/profiles/*.toml` + 内置 strict/balanced/permissive | **加载进引擎但从未应用到 PermissionManager**；`confirm` 列表无消费者 |
| 桌面 | `approval_mode`（默认 `"confirm"`→Suggest）+ 4 档切换表 | 生效 |
| 内置逐工具策略表 | computer/applescript/browser=High 等 | 生效（REPL）；⚠️ 破坏性 MCP 注册只在 REPL，headless/server 缺失 |
| SPEC.md `[permissions]` TOML schema | auto_approve_safe/deny_patterns/allowed_paths | **代码中不存在** |

---

## 3. 对比分析

### 3.1 Shannon 与竞品的结构差异

1. **模式过多且语义重叠**：9 档中 FullAuto / DontAsk / BypassPermissions 三个"全放行"变体只有临界风险处理的细微差别，且在门控里 DontAsk 与 Bypass 完全同分支；Readonly / PlanReadonly 近重复；`Auto`（分类器驱动）是最能体现 Shannon 卖点的模式，却不在循环、标签与 FullAuto 冲突、状态栏不可区分。Claude Code 6 档语义互斥、谱系清晰；Codex 用两轴正交解耦"能力范围"与"审批策略"。
2. **规则与模式未正交**：竞品（Claude Code/OpenCode/Cursor）的 allow/ask/deny 规则是独立于模式的叠加层；Shannon 的规则层一是没接线，二是 Bypass 分支直接短路，deny 规则不能全模式生效（Claude Code 明确 deny 全模式生效）。
3. **无硬底线清单**：Claude Code 有"任何模式都不自动批准"清单 + ask 规则强制弹窗；Shannon 的对应物（destructive 注册、Critical 拒绝）只在部分 surface 生效。
4. **bypass 护栏差距大**：Claude Code 有 root 拒绝/首用确认/中途禁入/组织开关/`--restricted`；Shannon 仅 REPL 有确认框，`-y` 在 headless 静默升 Bypass。
5. **Shannon 独有优势**：5 级风险分类器（竞品多为 2-3 级判定）、MCP server 身份指纹审批库、逐工具策略表（computer/browser=High）、权限决策审计事件已入 session log。这些是"透明权限"卖点的基础，但**接线不完整使其没有转化为用户可感知的价值**。

### 3.2 默认值对比

| surface | Shannon | 竞品参照 |
|---|---|---|
| 引擎/TUI/WS/REST | **AutoEdit**（自动批准文件写） | Claude Code 交互默认 Manual（编辑也问）；Roo 默认全关 |
| 桌面 | Suggest（confirm） | — |
| headless `--prompt` | **FullAuto**；`-y`→Bypass | Claude Code `-p` 默认 Manual；Codex `-p` 也要求审批策略显式化 |

同一产品三个入口三种默认，且最常用的 TUI 默认比所有竞品交互默认都激进。

---

## 4. 问题清单

### A 类：正确性/安全性缺陷（应尽快修）

| # | 问题 | 证据 |
|---|---|---|
| A1 | **状态栏标签有损往返，会静默改变模式**：short_label 碰撞（DontAsk→FULL 与 Bypass 同、Readonly→ASK 与 Suggest 同、Auto→AUTO 与 FullAuto 同、PlanReadonly→PLAN），REPL 每轮流式结束后按 label 反向同步引擎，Auto→FullAuto、Readonly→Suggest、DontAsk→Bypass、PlanReadonly→Plan | permissions.rs:201-225；repl/query.rs:1436-1447, 1603-1620 |
| A2 | **Plan 模式"批准后自动执行"是死代码**：approve_plan/is_plan_approved 无调用者；实际 Plan ≡ Suggest，真正的计划流程（PlanManager + 写闸）与权限门不通 | permissions.rs:1584-1596, 1940-1944；plan_mode.rs:185 |
| A3 | **BypassPermissions 跳过一切检查，包括 deny 规则**：竞品明确 deny 全模式生效 | permissions.rs:1886-1889 |
| A4 | **team-agent 未知模式静默降级 FullAuto**（危险方向的容错）；`--headless-prompt` 完全忽略 `--permission-mode`，硬编码 FullAuto | main.rs:3838-3846, 2552-2568, 2725-2727 |
| A5 | **`/perms mode plan` 映射到 Readonly**，与 `/mode plan` 同词不同义 | cost.rs:647-660 vs config_kv.rs:219 |
| A6 | **破坏性 MCP 工具注册只在 REPL**：headless/team/server 缺失，"destructive 永远弹窗"防线在这些面失效 | repl/mod.rs:982（唯一调用点） |

### B 类：死配置面（宣传了但没接）

| # | 问题 | 证据 |
|---|---|---|
| B1 | PermissionRuleChecker（deny>ask>allow）生产未接线；settings `ask` 列表被忽略；同文件里还有第二套平行的 ToolPermissionRule 系统仅测试用 | permissions.rs:1846-1881；repl/mod.rs:235-312 |
| B2 | `permissionsMode` settings 键死；`permission_profile` 配置键 + env 死；自定义 profiles 加载但从未应用；mdbook 宣称 LLM 分类器回退生效，实际 `with_llm_classifier` 零调用者 | settings.rs:126-127；unified_config.rs:211-216；llm_classifier.rs；docs-mdbook/src/features/permissions.md |
| B3 | 模式选择不持久：无 `permissions.defaultMode` 等价物，每次启动重置 | permissions.rs:1474-1477 |
| B4 | 审计事件记录有损短标签，审计行无法区分 Bypass/DontAsk、Suggest/Readonly、FullAuto/Auto | guard_nodes.rs:128-131 |

### C 类：跨端一致性问题

| # | 问题 | 证据 |
|---|---|---|
| C1 | 默认模式三处不一致（引擎 AutoEdit / 桌面 Suggest / headless FullAuto），TUI 默认比全部竞品激进 | permissions.rs:112；desktop/config.rs:665；main.rs:2172-2184 |
| C2 | WS 协议无审批模式字段，客户端只能吃服务端构造的 AutoEdit；REST 无审批通道（`process_query(None)`），REST 会话静默自动批准文件写 | api-protocol lib.rs:269-321；routes/mod.rs:195-198 |
| C3 | "允许"作用域三种：TUI 控件 Allow Session=UI 本地、TUI A 键=项目 settings.local.json、桌面 Always allow=**用户级** ~/.shannon/settings.json | input.rs:1734-1769；permissions.rs:1716-1754；commands_permissions.rs:165-180 |
| C4 | 桌面 4 档切换表 vs 引擎 9 模式：dont_ask/bypass/auto/plan_ro 无 UI；模式名解析器各端拼写集不一致 | desktop/ui approvalModes.ts:26-30 |
| C5 | 手机端仅 allow/deny：无 always-allow、无模式控制；网关会话固定 AutoEdit（写自动批准，文档自己已标注风险） | gateway protocol.ts:108-112；docs/integrations/mobile-dispatch.md:78-79 |
| C6 | 双 `ApprovalMode` 枚举（shannon-commands 的 Auto|Manual|Smart 与引擎枚举并存），维护隐患 | context.rs:76-86 |
| C7 | VS Code 扩展 NDJSON 协议无审批消息，完全不能展示审批 | legacy-archives shannonClient.ts:95-146 |

### D 类：与竞品的能力差距（战略层）

| # | 差距 | 竞品参照 |
|---|---|---|
| D1 | 无 OS 级沙箱轴（权限检查 ≠ 沙箱隔离） | Codex 沙箱三档；Claude Code Seatbelt/bwrap |
| D2 | 无连续自动批准熔断（FullAuto 长跑无上限） | Cline/Roo max-requests |
| D3 | 无组织/管理面控制（禁 bypass、托管设置） | Claude Code managed settings |
| D4 | 无"为什么批准/拒绝"的用户可见透明度（分类器置信度等） | 竞品均无——Shannon 可做成差异化卖点（前次竞研 G7 已提出） |

### 保留并放大的优势

- 5 级风险分类器 + 逐工具策略表（比竞品判定更细）；
- MCP server 身份指纹审批库 + 项目/用户信任域分离（防投毒设计正确）；
- 权限决策审计已进 session log（竞品未宣传此能力）；
- 无审批通道时 fail-closed 拒绝的方向正确。

---

## 5. 结论

Shannon 的权限**分类器层**（风险分级、bash 命令分析、MCP 动词分类）是竞品中最细的；但**模式层**（9 档、语义重叠、标签冲突）和**接线层**（规则/profiles/默认模式/LLM 回退大面积死配置）落后于 Claude Code 的模式×规则正交模型与 Codex 的双轴模型。用户可感知的等级表面上看有 9 档，实际有效的只有 5-6 档，且三端默认不一致、部分防线只在 TUI 生效。改进方案见配套文档，分四个阶段：先修 A 类正确性缺陷，再接活 B 类死配置，然后收敛模式模型并对齐三端默认与协议，最后补透明度/熔断等差异化能力。
