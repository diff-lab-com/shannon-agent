# 权限模式改进方案（2026-10-04，2026-10-05 终版确认）

> 依据：[docs/research/2026-10-04-permission-modes-competitive-review.md](../research/2026-10-04-permission-modes-competitive-review.md)（问题编号 A1-A6 / B1-B4 / C1-C7 / D1-D4 均见该文第 4 节）
> 状态：**已确认，终版决策见 §0 与[命名设计稿 v2 终版 §10](2026-10-04-permission-mode-naming-design.md)**。实施分支：`feat/permission-mode-4x3`。

---

## 0. 需要先拍板的设计决策

| # | 决策点 | 推荐 | 备选 |
|---|---|---|---|
| K1 | 交互面默认模式 | **`auto-edit`（终版，评审确认）**：TUI/桌面/引擎三端交互默认一致为 auto-edit；桌面 `confirm` 同步改 `auto-edit`；首次启动一次性非阻塞提示；`permissions.defaultMode` 可持久化改为 ask | ~~ask~~（v1 推荐，否：行业默认已漂向自动化且 /rewind 兜底文件改动） |
| K2 | 9 档收敛方式 | **4+3（已定，10-05 评审）**：主控制 4 档（ask / auto-edit / full-auto + plan 单列）+ 专家档 3 个（readonly/CI/bypass）；引擎变体 9→7；命名、迁移与 UI 细节见[权限模式命名与 UI 收敛设计 v2](2026-10-04-permission-mode-naming-design.md) | — |
| K3 | 分类器（原 `Auto` 模式）地位 | **从档位降为判定引擎**：`Auto` 变体删除（旧输入映射 `ask`），分类器继续为 auto-edit/full-auto 供数，LLM 回退定位为 full-auto 可选加固（详见命名设计稿 §4.3） | — |
| K4 | WS/REST 协议加审批模式字段 | **加（终版）**：session create + `/api/query` 可选 `approval_mode` + WS `set_approval_mode` 带回执；服务端权威封顶（kill switch 下 bypass 请求 403）；协议改档写审计 | 暂不加，仅文档化 fail-closed 语义 |
| K5 | 连续自动批准熔断（D2） | **做（终版）：默认关**——`permissions.max_auto_approvals`（0=关）+ `--max-auto-approvals N`；交互触发弹一次确认后计数重置；headless 触发 **exit code 8**（实施修正：rc 7 已被 NoProgress 占用且 CI 依赖，熔断取下一空闲码）；桌面 onboarding 推荐组合（全自动+25） | 不做（担心打断自动化） |

---

## 1. 目标与非目标

**目标**
1. 修掉 6 个正确性/安全性缺陷（A1-A6），使"模式显示 = 模式实际生效"。
2. 接活死配置面（B1-B4）：规则三值（allow/ask/deny）全模式生效、profiles/defaultMode 落地、审计记录真实模式。
3. 模式模型收敛为 4+3（引擎 7 变体），三端（TUI/桌面/CLI headless）默认与能力对齐（交互默认 auto-edit，headless 维持 full-auto）。
4. bypass 补齐竞品级护栏；协议层具备传递审批模式与审批决策的能力。

**非目标（另立计划）**
- OS 级沙箱轴（D1，Codex 式 sandbox × approval 两轴）——工程量大，需独立设计文档；本方案只在模式描述中预留措辞。
- 组织管理面/托管设置（D3）。
- VS Code 扩展审批消息（C7，代码在 legacy-archives，等扩展复活再议）。

---

## 2. Phase 0 — 正确性缺陷修复（约 2-3 天）

### P0-1 根治标签往返丢模式（A1）
- **改动**：REPL 状态不再以 `approval_mode_label: String` 为源（`repl/state.rs:666`、`repl/query.rs:1436-1447, 1603-1620`），改为直接持有 `ApprovalMode` 枚举，显示时才派生标签；`short_label()` 改为 9 值唯一（ASK/EDIT/PLAN/CLF/FULL/BYPASS/RO/DONTASK/PRO→RO 合并后 8 值）；`from_label` 保留仅作兼容读取。
- **文件**：`crates/shannon-engine/src/permissions.rs`、`crates/shannon-ui/src/repl/{state.rs,query.rs,helpers.rs}`、`crates/shannon-ui/src/widgets/status_bar.rs`。
- **验收**：`/mode` 任意切换 9→8 档后跑一轮流式，状态栏与 `engine.permissions()` 读数一致（新增回归测试逐一断言 roundtrip）；审计标签（见 P1-4）与实际模式一致。
- **风险**：低。UI 快照测试需同步更新。

### P0-2 打通 Plan 模式审批→自动执行（A2）
- **改动**：`PlanManager` 的计划批准路径（`shannon-tools/src/plan_mode.rs`，`/plan approve` 与桌面 plan 流）调用 `PermissionManager::approve_plan(session_id)`；`exit_plan_mode` 同理。计划拒绝/新议题时 `reset_plan`。`ApprovalMode::Plan` 从此按既有设计：批准前等同 Suggest，批准后自动放行（写闸仍保留为第二道防线）。
- **文件**：`crates/shannon-tools/src/plan_mode.rs`、`crates/shannon-ui/src/repl/commands/session.rs`、`crates/shannon-engine/src/permissions.rs`。
- **验收**：YAML 场景——plan 模式创建计划→approve→后续 edit/bash 不再弹窗；/plan off 后恢复弹窗。
- **风险**：中。批准后自动执行的爆炸半径由写闸 + Critical 拒绝兜底。

### P0-3 deny 规则全模式生效（A3）
- **改动**：`classify_and_check` 中 Bypass/DontAsk 分支（permissions.rs:1886-1889）前置一步"已注册 deny 规则检查"；PermissionMemory 与（P1-1 接线后的）RuleChecker 的 deny 命中一律 `Err`。与 Claude Code 行为对齐：deny 是唯一穿透 bypass 的规则。
- **验收**：settings deny `Bash(git push*)` 时，`--permission-mode bypassPermissions` 下 git push 仍被拒；其余工具不受影响。
- **风险**：低，纯收紧。

### P0-4 非 TUI 路径的模式解析容错方向反转（A4）
- **改动**：`run_team_agent_mode` 未知模式从静默 FullAuto 改为启动报错退出（与 `run_noninteractive_query` 一致）；`run_headless_query`（`--headless-prompt`）签名加 `--permission-mode`，默认 FullAuto 不变但可显式收紧。
- **文件**：`crates/shannon-cli/src/main.rs:2552-2568, 2725-2727, 3838-3846`。
- **验收**：`shannon --team-agent --permission-mode typo` 报错；`--headless-prompt --permission-mode readonly` 生效（只读跑完，exit 6 于拒绝时）。
- **风险**：低。team-agent 传错值原本就是 bug。

### P0-5 统一 `/perms` 与 `/mode` 语义（A5）
- **改动**：`/perms mode plan` 映射 `ApprovalMode::Plan`；`/perms mode readonly` 保持；`/perms` 帮助文本与 `/mode` 合并措辞，或将 `/perms mode` 直接标记为 `/mode` 别名。
- **文件**：`crates/shannon-ui/src/repl/commands/cost.rs:593-671`。
- **验收**：`/perms mode plan` 后 `/mode`（无参）显示当前 `plan`。

### P0-6 destructive MCP 注册覆盖全部入口（A6）
- **改动**：把 MCP `destructiveHint` 注册（现仅 `repl/mod.rs:982`）下沉到工具注册表构建层（`shannon-mcp` 适配器或 engine 构建处），使 headless、team-agent、shannon-server、api_server 共享同一注册逻辑。
- **验收**：headless 会话中对 destructive MCP 工具的调用在 FullAuto 下仍被拒/显式要求 `--mcp-approve`；WS 桌面审批框出现 destructive 警示文案。
- **风险**：低。fail-closed 面变大是预期行为，CHANGELOG 注明。

---

## 3. Phase 1 — 接活死配置面（约 1-1.5 周）

### P1-1 统一规则系统，allow/ask/deny 三值全模式生效（B1）
- **改动**：
  1. `PermissionRuleChecker`（deny>ask>allow，permissions.rs:1846-1881）正式接线：`PermissionManager::new` 构造时从 settings 加载（替代现在散落的 PermissionMemory 装载，`repl/mod.rs:235-312`），`settings.json` 的 `permissions.ask` 开始生效。
  2. 语义对齐 Claude Code：allow=预批准（bypass 下无额外效果）、ask=强制弹窗（DontAsk 下转拒绝）、deny=全模式拒绝。
  3. 删除/合并平行的 `ToolPermissionRule` 第二系统（permission_classifier.rs:1560-1920，仅测试用），保留 glob 语法 `Bash(...)`/`mcp__server__*`。
  4. 兼容：现有 `.shannon/settings.local.json` 中已持久化的 allow 规则原样迁移。
- **验收**：三值规则 × 三代表格（Suggest/AutoEdit/DontAsk/Bypass）行为矩阵单测；迁移测试（旧 allow 规则加载后行为不变）。
- **风险**：中。AlwaysAllow 持久化路径（persist_allow_rule）改为写入 RuleChecker 统一存储。

### P1-2 落地 `permissions.defaultMode`（B3，含决策 K1）
- **改动**：死键 `permissionsMode`（settings.rs:126-127）废弃，新增 `permissions.defaultMode`（Claude Code 同名），启动时映射 `ApprovalMode` 并 `set_approval_mode`；优先级 CLI `--permission-mode` > 用户 settings > 项目 settings（项目级若为 bypass/dontask 则忽略并告警——防项目投毒，对齐 Claude Code）。REPL/桌面的会话内切换仍不写回该键（会话内存行为），但桌面可在 Settings→General 显式"设为默认"。
- **验收**：设置 defaultMode=readonly 启动 TUI 状态栏显示 RO；项目文件 defaultMode=bypass 被忽略且日志告警。
- **风险**：低。旧 `permissionsMode` 键读取兼容一个版本并提示迁移。

### P1-3 profiles 真正生效（B2 前半）
- **改动**：`resolve_permission_profile`（unified_config.rs:211-216）接入引擎构建路径：内置 strict/balanced/permissive 与自定义 `.shannon/profiles/*.toml` 经既有 `apply_profile`/`apply_custom_profile_def`（permissions.rs:1505-1581）应用到 PermissionManager；桌面 Settings→Permissions 页与 `activate_permission_profile` 已有 UI，接同一应用函数；顺序 = profile 先应用、`approval_mode`/defaultMode 后应用（手工选择优先，桌面 commands.rs:1550-1568 顺序已是如此，抽为公共函数）。`confirm` 列表落地为 ask 语义。
- **验收**：`permission_profile = "strict"` 启动后 Write/Bash 弹窗；桌面 ExecutionModeSwitcher 切换后引擎模式随之变化。
- **风险**：中。strict 比现默认更严，文档注明 profile 与 mode 的叠加关系。

### P1-4 审计记录真实模式 + LLM 分类器处置（B2 后半、B4）
- **改动**：
  1. `emit_decision`（guard_nodes.rs:128-131）改为记录 `ApprovalMode` 完整名（Display 串）+ 用户决策标记；session_event 消费端同步字段。
  2. LLM 分类器：**默认保持关闭**，但给出真实入口——`permissions.llm_fallback = true` 配置键 + REPL `/perms llm on`；或若决定放弃该特性则删除 `with_llm_classifier`/`classify_and_check_with_llm` 并修正 mdbook 文档。**推荐前者**（代码已含防降级与防注入加固，是现成差异化资产）。
- **验收**：审计事件中 FullAuto 与 Auto 可区分；开启 llm_fallback 后模糊用例可见 LLM 参与判定的日志与审计来源标记。
- **风险**：低。

---

## 4. Phase 2 — 模式模型收敛 + 三端对齐（约 2 周）

### P2-1 模式收敛为 7 变体、UI 收敛为 4+3（决策 K2/K3，2026-10-05 更新）
> 命名、迁移映射与 UI 呈现细节以[权限模式命名与 UI 收敛设计 v2](2026-10-04-permission-mode-naming-design.md)为准；本节为工程改动摘要。
- **改动**：
  1. `PlanReadonly` 并入 `Readonly`、`Auto` 变体删除（`auto-classifier`/`classifier` 输入映射 `ask` + 一次性提示）：变体 9→7（Ask/Plan/AutoEdit/FullAuto/BypassPermissions/DontAsk/Readonly）；serde alias 兼容旧值。
  2. `DontAsk` 语义修正：不再与 Bypass 同分支——**从不等待，弹窗一律转为拒绝**，allow 规则与只读快路径仍放行；定位为 CI 模式（配合 `--allowed-tools`）。Bypass 维持"跳过一切检查（deny 除外，P0-3）"。
  3. 主控制三档谱系：`Ask`（token `ask`，原 Suggest）→ `AutoEdit`（token `auto-edit`，`auto` 保持其别名，零破坏）→ `FullAuto`（token `full-auto`）；Shift+Tab 循环 ASK→EDIT→FULL；`PLAN/RO/CI/BYPASS` 标签唯一化，标签往返改为枚举存储（P0-1 同源修复）。
  4. plan 单列工程语义（设计稿 §5）：进入时快照当前自主权档，批准后计划内自动执行（P0-2），退出恢复快照；`ApprovalMode::Plan` 不再是循环停留点。
  5. 每档 description 重写为"自动/询问/拒绝"三列语义（文档与 `/mode` 无参输出共用）。
- **验收**：7 档行为矩阵集成测试（每档 × {只读工具/文件编辑/bash/destructive MCP/deny 规则}）；旧序列化值与全部旧别名输入的兼容测试；plan 快照/恢复往返测试。
- **风险**：中。`dontAsk` 行为变化为唯一破坏性语义变更（CI 用户群小），CHANGELOG 置顶标注。

### P2-2 三端默认与能力对齐（C1，决策 K1 终版：auto-edit）
- **改动**：
  1. 引擎默认维持 `AutoEdit`（token 更名 `auto-edit`）；桌面默认 `confirm` 改为 `auto-edit`；headless 维持 FullAuto——三端交互默认一致为 auto-edit。
  2. 首次启动一次性**非阻塞提示**（TUI toast/桌面横幅）：告知"文件修改自动执行、命令仍询问、Shift+Tab 可切换、/rewind 可回滚"；`permissions.defaultMode`（P1-2）供偏好 ask 的用户持久化，不再做阻塞式 onboarding 问答。
  3. UI 主控制收敛为 **4 档**（呈现细节以命名设计稿 v2 §7 为准）：自主权 3 档 `ask → auto-edit → full-auto`（TUI Shift+Tab 循环与桌面 pill 同序同标签，状态栏 pill 蓝/绿/橙）+ **plan 单列**（桌面现有 plan chip 保留对齐；TUI 走现有 `/plan`，plan 激活时 pill 互斥呈现 PLAN）。专家档 `readonly`/`dontAsk`/`bypassPermissions` 不进主控制，仅 `/mode`（TUI）与 Settings→General"高级"下拉（桌面）可达。桌面 ExecutionModeSwitcher（profiles）与审批谱系明确分层：profiles 改名"规则预设"并归入 Rules/Permissions 面板，消除与审批档同名不同义的两套 strict/balanced/permissive；桌面默认值 `confirm` 迁移为 `ask`（现为表外孤值）；桌面 `strict` 档下沉为 readonly 高级项。
  4. `/project set permissions`（loop_engine.rs:740-755）改走 `from_str_ci` 全集，未知值报错不再静默 Suggest。
- **验收**：TUI/桌面/headless 三端各跑一遍默认值冒烟；桌面切到 plan/auto 档后引擎读数一致。
- **风险**：中。默认变严是行为变更，README/CHANGELOG 置顶说明；老用户经 onboarding 一次性迁移。

### P2-3 协议层补审批模式与决策（C2，决策 K4）
- **改动**（api-protocol 下个版本）：
  1. `POST /v1/sessions` body 加可选 `approval_mode`；`POST /api/query` 加可选 `approval_mode`（缺省维持服务端默认并文档化 fail-closed）。
  2. WS `WsClientMessage` 加 `set_approval_mode`，服务端回执当前生效模式（防只写不读）。
  3. shannon-server/api_server 构建引擎不再裸 `PermissionManager::new()`，接受配置注入（routes/mod.rs:112-126）。
- **验收**：WS 客户端切 readonly 后写工具被拒；REST 会话带 approval_mode=readonly 冒烟；协议 snapshot 测试更新。
- **风险**：中。gen-ts 消费方（gateway/desktop）需同步类型。

### P2-4 bypass 护栏（A3 姊妹项）
- **改动**（对齐 Claude Code，裁剪为 CLI 可落地的集）：
  1. root/sudo 下拒绝进入 Bypass（env `SHANNON_ALLOW_ROOT_BYPASS=1` 显式覆盖）；
  2. 首次交互进入时"责任自担"确认（接受后持久标记，headless `-y` 等价于显式指定、免弹但打 stderr 警告 + 审计标记）；
  3. env `SHANNON_DISABLE_BYPASS=1` kill switch（CI/共享机用），命中时 `--permission-mode bypassPermissions` 与 `-y` 均报错退出。
- **验收**：root 下 `-y` 拒绝；kill switch 生效；确认框只出现一次。
- **风险**：低。CI 脚本若以 root 跑 `-y` 会 break——CHANGELOG 置顶。

### P2-5 "允许"作用域统一（C3）
- **改动**：三级作用域显式化并统一入口：`Allow once`（内存）/ `Allow session`（会话内存，进 PermissionMemory 不落盘）/ `Always allow`（落盘，**默认项目级** `.shannon/settings.local.json`；桌面改为同落点，用户级需修饰键或设置开关）。TUI 控件本地 `AutoApproveRule`（input.rs:1734-1769）并入 PermissionMemory 会话层，消除双轨。
- **验收**：桌面"Always allow"后规则出现在项目 settings.local.json；TUI 三种选项行为与桌面一致。
- **风险**：低。落盘位置变化需迁移提示。

---

## 5. Phase 3 — 差异化能力（约 1-2 周，可与反馈并行）

| 项 | 内容 | 来源 |
|---|---|---|
| P3-1 透明度卖点 | 审批弹窗/`/permissions history` 展示"为何批准/拒绝"：命中规则、风险级、分类器置信度、模式；数据源即 P1-4 的审计事件（竞品均无） | D4；前次竞研 G7 |
| P3-2 熔断上限 | FullAuto/AutoEdit/DontAsk 下连续 N 次自动批准后强制一次人工确认；`permissions.max_auto_approvals`（**默认 0=关**）+ `--max-auto-approvals`；交互触发弹确认后计数重置；headless 达限 **exit code 8**；桌面 onboarding"自动化偏好"给推荐组合（全自动+25 次） | D2（Cline/Roo） |
| P3-3 移动端补能力 | gateway `shannon/approval/decide` 增加 `scope: session`（always-allow 会话级）；移动端只读展示当前模式 + readonly 一键收紧 | C5 |
| P3-4 文档一次性纠偏 | mdbook permissions.md 重写为 8 档真实语义；configuration.md 补 defaultMode/profiles；SPEC.md 删除不存在的 `[permissions]` TOML schema（1053-1056）或实现之；README 同步 | B2 |
| P3-5 清理双枚举 | 删除 shannon-commands 的 `ApprovalMode{Auto,Manual,Smart}`（context.rs:76-86），改用引擎枚举 | C6 |

---

## 6. 战略备忘（不在本计划内）

- **沙箱轴（D1）**：Codex 式 `sandbox_mode × approval_policy` 是安全口碑的方向；Shannon 已有 `shannon-remote`（SSH/Docker 世界）可作为"远程=天然沙箱"的叙事起点。建议单独立项调研。
- **组织管理面（D3）**：managed settings + disableBypass，等 SaaS/团队场景需求明确后做。

## 7. 里程碑与验证

| 阶段 | 内容 | 预估 | 出口标准 |
|---|---|---|---|
| M0 | P0-1~P0-6 | 2-3 天 | 行为矩阵测试全绿；标签 roundtrip 回归测试 |
| M1 | P1-1~P1-4 | 1-1.5 周 | 规则三值 × 模式矩阵测试；defaultMode/profiles 冒烟 |
| M2 | P2-1~P2-5 | 2 周 | 三端对齐冒烟；协议 snapshot 更新；护栏测试 |
| M3 | P3-1~P3-5 | 1-2 周 | 透明度 UI 上线；熔断场景测试 |

全程约束：`just dev`（check + clippy + nextest）通过；行为变更项在 CHANGELOG 置顶；api-protocol 变更走版本锁定流程（gen-ts 同步 gateway/desktop）。

## 8. 兼容与迁移清单

- `permissionsMode`（旧键）→ `permissions.defaultMode`：读兼容一版 + 迁移告警。
- `PlanReadonly` serde 旧值 → `Readonly`：反序列化别名。
- `/perms mode plan` 行为变化：帮助文本标注。
- 默认 AutoEdit → Suggest：onboarding 一次性引导 + README 置顶。
- 桌面 Always allow 落盘位置 用户级 → 项目级：首次落盘时提示。
- root + `-y`：CI 场景 break 风险，release notes 置顶。
