# R2 改进方案执行排期（供审核）

**依据**: [R2 复审报告 §5](../research/2026-10-01-journey-competitive-review-r2.md) + Rulings R4/R5（2026-10-01）
**规模口径**: S=≤1 天、M=2-5 天、L=1-2 周（单人串行估算；沿用上一轮「独立 PR + 审查修复循环」并行模式，日历时间可显著压缩——R1 轮 30 项即为并行执行）
**PR 拆分原则**: 每个工作包 = 一个可独立审查合并的 PR；同文件域的小修合并进同一 PR（标注「顺手」），避免二次触碰

---

## Wave 1 · Hotfix 窗口（~1 周日历；10 个工作包，全部 S 级）

目标：**说谎 UI 清零 + 一行修清账**。全部可并行，无相互依赖（W1-6 与 W1-10 同主题建议同人做）。

| 包 | 内容 | 主要落点 | 规模 | 顺手项（同文件域 P2） | 验收要点 |
|---|---|---|---|---|---|
| W1-1 | **R2-P0-1(B)** url-only MCP 诚实态：列表行「桌面端即将支持，请用 CLI」徽章；restart 对 url-only 禁用 | `commands_mcp.rs` + `McpServers.tsx` + 10 locale | S | — | OAuth 安装成功后列表显示诚实徽章而非 Offline 坏态；restart 按钮对 url-only 禁用且有 tooltip |
| W1-2 | **R2-P0-2** 技能热注册：`list_skills` 顺带幂等重跑 `register_skills_as_chat_tools`（重名跳过） | `commands_mcp.rs:266-271` + `skill_tools.rs` | S | — | **e2e 断言**：装 skill → 不重启立即对话 → 系统提示含该 skill 且 ToolRegistry 有对应工具 |
| W1-3 | **R2-P0-3(a)** cost/token 列兜底：改为「有数据才渲染」（History 已有条件渲染先例 `HistoryView.tsx:193-198`），Wave 2 数据接线落地后自然恢复 | `OPCRunsTable.tsx` + `HistoryView.tsx` | S | — | 无数据时列隐藏且无布局跳动；不删列、不重写 |
| W1-4 | **R2-P1-2** svg/bmp 诚实化：选择器 filter 移除两类 + 后端回执加 unsupported_media 兜底（防拖拽路径绕过 filter） | `ChatInput.tsx:35` + `commands.rs:1107-1112` | S | — | 拖入 svg → chip 显示「不支持发送给模型」徽标，不再静默 |
| W1-5 | **R2-P1-3** OPC 上板路径锚定：`.claude/tasks` 锚定 workspace 绝对路径 | `commands_tasks.rs:231-250` | S | R2-P2 agent 发现域 CWD 收口（`commands_agents.rs:196-204`，同主题） | GUI 从 Dock 启动（cwd=/）时快建任务仍能上板 |
| W1-6 | **R2-P1-4** 双重 toast：hook 去 toast，页面统一 | `scheduled-tasks.ts:47` + `Tasks.tsx:235` | S | R2-P2 catalog 任务 RunNow 伪指令修复、暂停态 RunNow 确认（同 Tasks.tsx） | 创建例行只出一条 toast；catalog 卡无伪 prompt RunNow |
| W1-7 | **R2-P1-6** MCP 失败可诊断：`last_error`/`last_connected` 透传 → Pending 错误区复活 | `commands_mcp.rs:220-228` + `Pending.tsx:44-69` | S | — | 连接失败的 server 在列表/Pending 显示具体错误，不再空态 |
| W1-8 | **R2-P1-7** webhook 测试语义：dirty 先保存再测或明示「使用已保存配置」 | `NotificationsSettings.tsx:186-206` | S | R2-P2 占位符不进 state、timeout 非正数 inline 错误（同文件） | 改 URL 未保存点测试 → 按钮提示或先保存；verdict 与表单所见一致 |
| W1-9 | **R2-P1-9** 命令面板补设置 6 子页 | `CommandPalette.tsx:76-78` | S | — | 8/8 设置子页全部可从命令面板直达 |
| W1-10 | **R2-P1-10** 402/quota 错误分类 | `commands_config.rs:1246-1265` | S | — | 402 → `quota_exhausted` 分类 + i18n 文案（10 locale），不再吐原始英文 |
| W1-11 | **防复发机制**：「安装→可用」端到端验收清单（MCP stdio/remote、技能、agent、数据源各一条：装→重启→列表态→聊天内工具可见→可调用）进 CI 或发版 checklist | `desktop/tests/` + 发版流程文档 | S | 与 W1-2 的 e2e 合并起步 | 清单成文且至少 MCP/技能两条已自动化 |

**Wave 1 合计**: ~7-10 人日串行；10 PR。发布后对 J3（扩展生态）做一次定向走查复验。

---

## Wave 2 · 迭代 N（~2-3 周日历；7 个工作包，1 M-L + 5 M + 1 S）

目标：**承诺兑现**——remote MCP 接线、成本数据闭环、预算续发修正、终态可见性、i18n 旧账。

| 包 | 内容 | 主要落点 | 规模 | 依赖/顺手项 | 验收要点 |
|---|---|---|---|---|---|
| W2-1 | **R2-P0-1(A1)** 纯 HTTP/SSE remote MCP 接线：desktop 配置结构体补 `url` 字段 → seed 走 `start_remote_server` → 状态/重启支持 + 测试 | `config.rs:291,731-763` + `mcp.rs:39-45` | **M** | 依赖 W1-1（B 态先发）；顺手：disabled 徽章+开关、工具数/状态分离、settings.json 损坏错误态（三小修同文件域） | 纯 HTTP/SSE remote server 安装 → 重启 → 在线 → 工具聊天中可调用；OAuth url-only 行为不变（诚实态）；OAuth remote 留给 A2 |
| W2-2 | **R2-P0-3(b)** cost/token 数据接线：按 session 从 usage ledger 聚合（model 列同款 join 先例 `scheduled_commands.rs:1509-1570`），OPC/History 列恢复真实数据 | `inbox_commands.rs` finalize 链 + `join_agent_runs` | **M** | 与 W2-3 同执行器域，建议同人连做 | 建一个带消耗的 run → History/OPC 列显示真实 cost/token；W1-3 的条件渲染自动恢复显示 |
| W2-3 | **R2-P2-D** budget 触顶自动暂停（Ruling R5 提前）：执行器超限即断 + run record 落原因 + 通知 | `inbox_commands.rs` 执行器 | S-M | 依赖 W2-2 的聚合口径；顺手：retry 中间 attempt 花费入账（账本 backlog 项） | budget_usd 超限 → 当次执行中止、run record 标注超限原因、用户收到通知；不再「只通知不打断」 |
| W2-4 | **R2-P1-1** 预算「继续一次」修正：拒绝时草稿+附件退回 composer；Continue 透传被拦内容或明示「将重发上一条」；首轮超限给可用路径 | `Chat.tsx:291-294,436-438` + `AppContext.tsx:544-552` | **M** | 顺手：流式纯附件 no-op 排队（同 Chat.tsx） | 预算拦截后 composer 恢复原草稿+附件；Continue 重发的是被拦那条；首轮超限无死点 |
| W2-5 | **R2-P1-5** 后台任务终态呈现：完成态落 inbox item 或 Runs 页终态分区 | `BackgroundTasksPanel.tsx:34-52` + `inbox_commands.rs` | **M** | — | 后台任务完成后在非 Dev 模式有终态可见（成功/失败+入口），Simple 模式不再 created-then-invisible |
| W2-6 | **R2-P1-8** i18n 旧子树批量机翻：ja settings.* 472 键 + 同类（zh-TW/ja/ko/de 优先） | `desktop/ui/src/i18n/locales/*` | **M** | 前置：W2-6a profiles.* 44 死键清理（R2-P2-16，先删再翻）；顺手：权限档描述前端映射（R2-P2-14）、Remotes 限制卡（R2-P2-15 文案部分） | 8 locale 英文逐字残留率 67% → ≤40%（copy-rate 报告可量化）；机翻人工抽查 ≥10 条/语言 |
| W2-7 | **R2-P2 模板人性化**（原 P2-8 残留）：github 触发器 event/repo 语义化、interval 时长人类可读化 | `RoutineTemplatesBrowser.tsx:163-167` | S | — | 模板卡不再出现裸 "0s"/"3600s" |

**Wave 2 合计**: ~12-18 人日串行；7 PR。发布后对 J2（自动化）+ J3 做定向复验；W1-11 的 e2e 清单补 remote MCP 一条。

---

## Wave 3 · 季度（战略项 + 长期债）

| 包 | 内容 | 规模 | 排位依据（R5） |
|---|---|---|---|
| W3-1 | **R2-P2-A** NL 自动化「结构化预览→激活确认」步：全量预览（触发/prompt/通知/预算）+ 激活确认；可顺带 Test run 语义包装（Manus 正/负例文案） | M-L | 季度必做——自动化信任洼地的最高杠杆范式 |
| W3-2 | **R2-P2-C** 失败自动 pause + needs action：连续失败自动暂停+通知+自愈路径；triage 卡 needs-action 前置强调 | M | stretch——尽量与 W3-1 同季（同 ScheduleForm/routine 生命周期域，避免二次动 UI） |
| W3-3 | **R2-P0-1(A2)** OAuth remote + token 刷新/静默重连：**先出失败呈现设计文档再立项** | L（先 S 评估） | Ruling R4：失败呈现方案明确前不启动 |
| W3-4 | **R2-P2-B** 记忆引用可回跳：回答内记忆引用呈现+跳源会话 | M | Ruling R5：延后下季度（强面上加分，非信任修复） |
| W3-5 | 剩余 P2 打磨按域顺带：大 PDF 解析中态、后端硬错误结构化 tag、keychain 接入、rss/ical 目录文案、billing 死代码清理、@ 引用/steering 第三档 | 各 S-M | 搭对应域的 Wave 顺车，不单独立项 |
| W3-6 | IA 长期债（三套任务存储/两套 agent 概念） | — | 维持 ADR-0013 接缝口径，不迁移 |

---

## 排期总览

```
Hotfix（~1 周）      迭代 N（~2-3 周）                    季度
W1-1..11（10 PR） →  W2-1 A1 remote ─┬─ W2-3 budget 自停  → W3-1 预览确认步（必做）
说谎 UI 清零           W2-2 cost 接线 ─┘                    W3-2 失败自停（stretch）
+ e2e 清单起步         W2-4 预算续发修正                     W3-3 A2 立项评估
                       W2-5 终态呈现                         W3-4 记忆回跳（下季）
                       W2-6 i18n 旧账                        W3-5/6 顺带/维持
```

## 评审关注点（请重点确认）

1. **W1-3 兜底策略**：条件渲染隐藏 vs 直接删列——我选了条件渲染（接线落地后自动恢复，避免删了重加的翻动）。
2. **W2-3 提前**：Ruling R5 允许 D 提前至迭代 N；它与 W2-2 同执行器域，拆开反而多一次触碰，已排进 Wave 2。若你想严格留在季度，W2-3 移出即可，W2-2 不受影响。
3. **W2-6 机翻范围**：本轮只翻 ja 的 settings.* 472 键 + 同类旧子树（目标残留率 ≤40%），不做全量 100% 冲刺（维持 R1 不建议项）。若你想更激进，范围与目标率都可调。
4. **W1-11 归属**：e2e 验收清单进 CI（自动化两条起步）还是只进发版 checklist（纯人工）——影响 Hotfix 工作量约 ±0.5 天。
