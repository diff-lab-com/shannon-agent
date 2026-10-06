# 权限模式命名与 UI 收敛设计（v2 终版）

> 上游：[竞品调研与现状审查](../research/2026-10-04-permission-modes-competitive-review.md) · [改进方案](2026-10-04-permission-modes-improvement-plan.md)
> v2 变更：采纳评审意见，主控制收敛为 **4 档（plan 单列 + ask / auto-edit / full-auto 三档自主权谱系）**；`Auto`（分类器模式）从档位序列中移除，分类器降为自动档的判定引擎；专家档 3 个收进高级通道。
> 状态：**已确认（2026-10-05 终版决策落定，见 §10）**。实施分支：`feat/permission-mode-4x3`。

---

## 1. 问题定义：混乱的根源是「五套词汇表 × 非单射」

一个模式今天最多有 5 个名字：Rust 枚举名、引擎显示串、状态栏标签、桌面档位名、profiles 档名。9 枚举 → 9 显示串 → **5 个标签**（四组多对一）→ 桌面另有 4 档表 + 表外默认值 `confirm` + 一套撞名 profiles。最刺眼的三处：

1. `AutoEdit` 显示为 `auto`，而事实标准（Claude Code）的 `auto` 是分类器模式——名字与行为正面冲突；
2. `DontAsk` 名字说"不问"，门控实现与 `BypassPermissions` 同分支（permissions.rs:1886-1889），实际是"全放行"；
3. `Auto` 与 `AutoEdit` 实现层重叠（见 §4.3），撑不起两个独立档位。

## 2. 竞品共识与 4 档模型的依据

| 产品 | UI 主谱系 | 对 v2 的支撑 |
|---|---|---|
| Gemini CLI | `Default → Auto-Edit → YOLO`（Shift+Tab 循环 3 档，yolo 独立通道 + Ctrl+Y） | **3 档自主权谱系完全一致** |
| Cursor | ask → auto-run+allowlist → auto-run 全部 | 同上 |
| Antigravity CLI | `default > accept-edits > plan` | plan 与自主权档并列呈现 |
| Claude Code | 循环 `default → acceptEdits → plan →(bypass)→(auto)`；dontAsk 不进循环 | plan 单列有先例；专家档走独立通道 |
| Cline/Roo | 5 个动作开关 + max-requests | 例外走规则层，不占档位 |

四条行业共识不变：日常档进循环（本设计为 3 档）、危险档独立通道加摩擦、token 与显示名最多一处文档明示的分化、例外永远走规则层。

**v2 相对 v1（5 档）的关键修正**：v1 把 Claude Code 的 `auto`（分类器模式）照搬进主循环第 4 位。但审查发现两点：CC 的 `auto` 语义是"分类器代判一切、仅极少数例外弹窗"，而 Shannon 的 `Auto` 实现是"仅 Safe/Low 放行、Medium+ 一律弹窗"（permissions.rs:268-271, 1933）——**连文件编辑（Medium）都弹窗，比 AutoEdit 还保守**，实际是 `Ask` 的一个略松变体。照搬的是概念而非可用的实现。结论：`Auto` 不配主循环档位，分类器应降为判定引擎（§4.3）。

## 3. 设计原则

- **P1 单词汇表**：每个模式 = 一个 config token = 一个显示名 = 一个状态标签；唯一允许的分化是本地化显示名（§8 词表）。
- **P2 事实标准优先**：与竞品重合的档位用通行名（ask/Auto-Edit/full-auto 均有竞品先例）；Shannon 扩展（readonly/plan-批准后自动执行）用自 descriptive 名。
- **P3 行为可推断**：看名字知道"什么会被自动做"；描述统一「自动执行 / 询问 / 拒绝」三列。
- **P4 谱系单调、plan 单列**：自主权三档严格递增（每档描述回答"比上一档多放了什么"）；plan 是工作流档而非自主权点，单独呈现、单独通道。
- **P5 规则不占档**：allow/ask/deny 例外走规则层（规则预设），永不靠新增档位解决。

## 4. 目标模型：4 + 3

### 4.1 主控制 4 档（引擎枚举收敛 9 → 7）

**自主权谱系 3 档**（TUI Shift+Tab 循环 / 桌面 pill，同序同标签）：

| # | config token | Rust 变体 | 标签 | 一句话 | 比上一档多放了什么 |
|---|---|---|---|---|---|
| 1 | `ask` | `Ask`（原 `Suggest`） | `ASK` | 只读随便看，动手必问 | — |
| 2 | `auto-edit` | `AutoEdit` | `EDIT` | 文件编辑免问，命令仍问 | +项目内文件写（≤Medium 风险） |
| 3 | `full-auto` | `FullAuto` | `FULL` | 极高危以外全部自动 | +命令/网络/MCP 执行（Critical 仍拒绝；deny 规则全档生效） |

**工作流档 1 个（单列）**：

| config token | 变体 | 标签 | 语义 |
|---|---|---|---|
| `plan` | `Plan` | `PLAN` | 进入即写闸（只读分析），**必须先产出计划并获批准**；批准后自动执行（P0-2 接线）；退出 plan 恢复原自主权档 |

**专家档 3 个**（不进主控制；TUI `/mode`，桌面"高级"下拉）：

| config token | 变体 | 标签 | 语义 | 通道与摩擦 |
|---|---|---|---|---|
| `readonly` | `Readonly` | `RO` | 只读分析，其余全拒 | `/mode`（原 `plan-readonly` 并入） |
| `dontAsk` | `DontAsk` | `CI` | 从不等待：规则内放行，其余拒绝 | `/mode`/flag；行为按改进方案 P2-1 修正为"拒绝而非等待"，配合 `--allowed-tools` |
| `bypassPermissions` | `BypassPermissions` | `BYPASS` | 无检查（deny 除外） | 确认框 + 首用责任确认 + root 拒绝 + kill switch（P2-4） |

对外叙事：**"ask / auto-edit / full-auto 三档自主权 + plan 工作流档"**——与 Gemini/Cursor 主谱系同构，Claude Code 用户零学习成本（default/acceptEdits 均为兼容别名）。

### 4.2 与 v1（5 档）的差异与被否决项

| 方案 | 处置 | 原因 |
|---|---|---|
| v1 的 5 档（含 `auto` 第 4 位） | **否决** | Shannon `Auto` 实现与 `Ask`/`AutoEdit` 重叠（§2）；CC 的 auto 语义需要 CC 级分类器在线裁决支撑，Shannon 现状没有 |
| 6 档激进案（并 auto/full-auto、删 dontAsk） | **否决** | 同 v1：伤确定性与 never-wait 场景（goals/batch/routines/mobile） |
| 桌面 strict 档 | **移出主控制** | readonly 是专家档不是日常姿态；入"高级"下拉 |

### 4.3 分类器的去处：从「档位」降为「判定引擎」

`PermissionClassifier`（规则 + 风险表）本来就在为 AutoEdit/FullAuto 的 `should_auto_approve` 供数；v2 只是把这件事说清楚：

- `auto-edit`/`full-auto` 的每条自动批准都经分类器风险裁决——分类器是**层**，不是**档**；
- LLM 分类器回退（`llm_fallback`，改进方案 P1-4）定位为 full-auto 的**可选加固**（模糊用例升级裁决，只收紧不放松），默认关；
- P3-1 透明度直接受益：审批弹窗可展示"风险级 / 命中规则 / 裁决来源"，不再需要为它保留一个 UI 档位。

这同时化解了 v1 §4.3"ML 信任锚与确定性信任锚不可合并"的顾虑：两种信任锚在 full-auto 内部**分层共存**（规则表恒在，LLM 可选叠加），而不是做成两个让用户困惑的档位。

## 5. plan 单列的工程语义

1. **进入**：`/plan`（TUI，现有命令）或桌面 plan chip（现有控件）→ 引擎记 `plan_pending` 档位快照 = 当前自主权档（ask/auto-edit/full-auto），置 Plan 模式；写闸（agent_loop.rs:2626-2679）与只读放行立即生效。
2. **批准**：`/plan approve`、`exit_plan_mode`、桌面批准按钮 → `approve_plan(session_id)`（打通现有死代码，改进方案 P0-2）→ 计划范围内自动执行（写闸保留为第二道防线）。
3. **退出**：`/plan off` / 拒绝计划 / 新议题 → 恢复快照的自主权档。`Plan` 不再是 Shift+Tab 循环的一个停留点，规避"循环到 PLAN 后批准即全自动、用户失去档位感"的 v1 缺陷。
4. 边界：plan 批准后的自动执行仍受 deny 规则与 Critical 拒绝约束（与 full-auto 同底线）。
5. **plan 期间的 Shift+Tab 细则（终版）**：plan 激活时，第一次按 Shift+Tab = 退出 plan 并恢复快照档（toast「已退出规划模式，恢复 <档名>」），之后恢复正常循环——杜绝用户在 plan 期间切自主权档造成快照歧义。

## 6. 命名迁移映射 v2（兼容一版）

| 旧输入 | 新 token | 处理 |
|---|---|---|
| `suggest`/`ask`/`default`、桌面 `confirm`/`balanced` | `ask` | 别名保留；`confirm` 迁移告警一次 |
| `auto-edit`/`auto_edit`/`acceptedits`/`permissive`（桌面）、**`auto`** | `auto-edit` | **`auto` 保持指向 AutoEdit**（现状 Display 即 `auto`，零破坏）；桌面 `permissive` 别名保留 |
| `auto-classifier`/`classifier` | `ask`（安全方向）+ 一次性提示 | 变体 `Auto` 删除；提示建议改用 `auto-edit`/`full-auto` |
| `full-auto`/`fullauto`/`full_auto`、桌面 `full` | `full-auto` | 别名保留 |
| `readonly`/`read-only`、桌面 `strict`、`plan-readonly`/`plan_ro` | `readonly` | 别名保留；变体 `PlanReadonly` 删除 |
| `plan` | `plan` | 不变（语义按 §5 充实） |
| `dontask`/`dont_ask`/`dont-ask` | `dontAsk` | 行为修正为"拒绝而非等待"（P2-1） |
| `bypass*` | `bypassPermissions` | 不变 |
| serde 旧值 `Suggest`/`AutoReadonly` 等（session/配置反序列化） | 新变体 | serde alias 兼容读取；`Auto`/`PlanReadonly` 旧值分别映射 `Ask`/`Readonly` |

**v2 相对 v1 的迁移简化**：撤销 v1 的 D-2（`auto` 别名重定向到分类器模式）——`auto` 本来就解析到 AutoEdit，token 定为 `auto-edit` 后 `auto` 顺势成为正式别名，**该项破坏性更名整项消失**；唯一行为变更是 `dontAsk`（CI 用户极少数）与 `auto-classifier` 映射（仅 `/mode` 显式输入者）。

## 7. UI 呈现（逐端）

### 7.1 TUI
- Shift+Tab 循环 **3 档**：`ASK → EDIT → FULL`；状态栏 pill 三色（蓝/绿/橙），非 default 档常驻可见。
- plan 单列：`/plan` 命令（现有）进入；plan 激活时状态栏显示紫色 `PLAN` pill **替代**自主权 pill（单列 = 互斥呈现），批准后 pill 显示 `PLAN✓`。
- `/mode` 无参列表分组：**日常（ask/auto-edit/full-auto/plan）** + **高级（readonly/CI/bypass，标注风险）**。
- 专家档：`/mode readonly|ci|bypass`；`ci` 作为 `dontAsk` 的 UI 别名；bypass 保留确认框 + 新增首用责任确认（P2-4）。
- 审批对话框顶部显示当前档标签 + "为什么询问"留位（P3-1）。

### 7.2 桌面
- 审批 pill = **3 段**（询问 / 自动编辑 / 全自动），与 TUI 标签一致；Settings→General radiogroup 同步。
- plan 独立 chip **保留在 pill 旁**（现状已是单列控件，语义按 §5 对齐：批准/退出恢复原档），Cmd/Ctrl+Shift+P 快捷键不变。
- "高级"下拉：只读 / 无人值守 / 跳过审批；bypass 红色警示 + 责任确认。
- ExecutionModeSwitcher 改名「规则预设」移入 Settings→Permissions（或头部保留但 tooltip 明示"规则包，叠加于审批档之上"），消除与审批档撞名的 strict/balanced/permissive。

### 7.3 移动 / 远端
- 快照带当前档 token，只读展示；一键收紧（切 readonly）+ 审批卡（allow/deny/会话内允许）。不放完整切换器（误触代价不对称）。

### 7.4 API / 协议
- token 全链路与引擎一致（P2-3 字段直接用 §4.1 token）；WS `set_approval_mode` 回执生效 token；审计记录 token。

## 8. 文案规范与本地化

- 描述三列式（自动/询问/拒绝），主动语态、能力导向。
- zh 显示名词表（en token 不变，唯一允许的分化层）：

| token | zh 显示名 | 备注 |
|---|---|---|
| `ask` | 询问 | |
| `auto-edit` | 自动编辑 | |
| `full-auto` | **全自动**（推荐） | 若用「完全访问」，必须接受文案注明"极高风险操作仍拦截"；若要求名实完全一致则需并入 bypass 语义，**不推荐** |
| `plan` | 规划 | |
| `readonly` | 只读 | |
| `dontAsk` | 无人值守 | |
| `bypassPermissions` | 跳过审批 | |

## 9. 与改进计划的衔接

- P2-1（引擎收敛）目标从 8 档改为 **7 变体**；P2-2（UI 对齐）从 5 档谱系改为 **3 档 + plan 单列**；本文替代两节的设计细节。
- 工作量较 v1 净减少：撤销 `auto` 重定向迁移提示、桌面档位映射与现状（balanced/permissive/full）几乎一一对应，改动集中在改名与 strict 下沉。
- 里程碑不变（M0 修缺陷 → M1 接线 → M2 收敛对齐 → M3 差异化）；§5 的档位快照/恢复并入 M2。

## 10. 终版决策（2026-10-05 确认）

| # | 决策 | 终版结论 |
|---|---|---|
| D-1 | 4+3 模型 | **采纳**（10-05 评审提案） |
| D-2 | 第 3 档命名 | **`full-auto` / 全自动**（名实相符；"完全访问/Full Access"留给 bypass 俗名） |
| D-3 | TUI plan | **单列**：`/plan` 通道进入，不进 Shift+Tab；plan 中首次 Shift+Tab = 退出恢复快照档（§5.5） |
| D-4 | 状态标签集 | **ASK / EDIT / FULL / PLAN / RO / CI / BYPASS**（七标签互斥，审计行记 token 不记标签） |
| D-5 | zh 显示名词表 | **询问 / 自动编辑 / 全自动 / 规划 / 只读 / 无人值守 / 跳过审批**（第 1 档取"询问"：token `ask`、标签 ASK、显示"询问"同源，P1 单词汇表） |
| K1 | 交互默认档 | **`auto-edit`**（评审确认）：TUI/桌面/引擎三端交互默认一致；桌面 `confirm` 默认同步改 `auto-edit`；首次启动一次性非阻塞提示（教 Shift+Tab 与 /rewind）；`permissions.defaultMode` 可持久化改为 ask |
| K4 | 协议字段 | **加**：REST/WS 可选 `approval_mode` + 回执生效 token + 服务端权威封顶（kill switch 下请求 bypass 返回 403）+ 协议改档写审计 |
| K5 | 熔断 | **做，默认关**：`permissions.max_auto_approvals`（默认 0=关）+ `--max-auto-approvals N`；交互触发弹一次确认后计数重置；headless 触发 **exit code 8**（rc 7 已被 NoProgress 占用，实施时取 8）；桌面 onboarding"自动化偏好"给推荐组合（全自动 + 25 次） |
