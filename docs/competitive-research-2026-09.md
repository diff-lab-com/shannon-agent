# Shannon 竞品深度调研与 Gap 分析（2026-09）

**调研日期**: 2026-09-05
**基线代码**: dev @ cc663a2c
**调研对象**: Claude Code / Claude Code Desktop、Codex Desktop（OpenAI）、Hermes Desktop（Nous Research）、WorkBuddy（腾讯）、ZCode（智谱 Z.ai）
**历史基线文档**: [desktop/COMPETITIVE-ANALYSIS.md](../desktop/COMPETITIVE-ANALYSIS.md)（2026-06-13）、[docs/competitor-feature-matrix.md](./competitor-feature-matrix.md) v2（2026-08-02）、[docs/improvement-plan-2026-08.md](./improvement-plan-2026-08.md) v4（2026-08-08）
**配套文档**: [docs/improvement-plan-2026-09.md](./improvement-plan-2026-09.md)（改进实施方案，待评审）

---

## 0. TL;DR

1. **格局变化**：与 6 月基线相比，竞品全部完成了「桌面端 = Agent 编排指挥中心」的定位收敛，且三个关键能力已成 2026 桌面端标配——**可拖拽多面板工作区**（Claude Code）、**自动化结果收件箱/Triage**（Codex）、**成本可观测**（Hermes 把它做成显式设计目标）。三者 Shannon 均未闭环。
2. **好消息**：8 月矩阵里列的办公线缺口大部分已在 dev 闭合——附件、语音（云+本地 whisper）、diff viewer、worktree 创建、scheduled routines UI、hooks pipelines UI、LSP 编辑器、checkpoint/rewind、remote SSH/Docker 设置页均已落地。Shannon 的 UI 覆盖度从 6 月的约 60% 提升到约 75%。
3. **最刺眼的 5 个新 Gap**（按优先级）：
   - **G1 Goal/Loop/Ralph 无桌面入口**——CLI 刚完成 Phase 2 的自主长任务体系（goal 注入、anti-spin、budget cap、stall strikes），桌面端零暴露；而 ZCode 已把 Goal Mode 作为核心卖点、用户破百万。
   - **G2 自动化缺「最后一公里」**——routine 有 cron 调度、有 triage 页，但缺 Codex 式「结果进收件箱 + 一键回原会话续跑」闭环，也缺 Claude Routines 的 API endpoint / GitHub 事件触发器（只有 cron + webhook 通知）。
   - **G3 成本不可解释**——usage 页和 /cost 存在，但没有 Hermes 式「上下文按类别拆解（系统提示/工具定义/技能/记忆/MCP/对话）+ 缓存命中率」的状态栏，也没有通用 session 级预算上限（只有 goal 有 budget cap）。竞品评论区第一大抱怨全是限额/成本，这是 Shannon（BYOK 多 provider）最锋利的进攻武器，当前没打出来。
   - **G4 桌面工作区形态落后**——单窗口、固定布局、无集成终端、无预览自检闭环（ArtifactPanel 只能看 HTML/Mermaid，不能起 dev server 截图自检）。
   - **G5 消息渠道仍空白**——Hermes 约 20 个平台（含钉钉/飞书/WeCom）、WorkBuddy 覆盖微信/企微/飞书/钉钉且移动三端上线；Shannon 仍只有 webhook 通知 + remote_trigger，无「从 IM 派发任务」。
4. **ZCode 判定为「互补为主、桌面端次级威胁」**：无 CLI、Electron 闭源、模型绑定单一；但它验证了 Goal Mode 与「闲时免费任务」的产品价值，且其 Anthropic/OpenAI 兼容端点与 Shannon 的 GLM provider 支持互相放大。
5. **建议下一步**：按 [improvement-plan-2026-09.md](./improvement-plan-2026-09.md) 执行——Wave A 三个 P0 全部是「后端已就绪、只差最后一公里」的高性价比项（Goal 桌面入口、自动化收件箱、成本可观测），预计 2 周内可交付。

---

## 1. 调研范围与方法

- **竞品面**：5 个产品/产品族，信息来自官方文档、官方博客/changelog、GitHub issues/releases、Reuters/CNBC/VentureBeat/钛媒体/财联社等媒体、Reddit/HN/V2EX 社区口碑，时间窗 2025-09 → 2026-09-05，重点近 3 个月。
- **自查面**：dev 分支代码盘点（desktop 211 个 Tauri 命令、路由与页面结构、crates 能力清单、测试与工程指标、tech-debt 遗留）。
- **对比基线**：与 2026-06-13 / 2026-08-02 两份内部文档做 delta，标注「已闭合」避免重复投入。
- 图例：✅ 完整 · 🟡 部分/有缺口 · ❌ 无。

---

## 2. 竞品逐项深析

### 2.1 Anthropic：Claude Code CLI + Claude Code Desktop

**矩阵与定位**：Claude Code CLI（v2.1.x）→ Claude Code Desktop（Mac/Win/Linux beta，2026-04-14 完整重设计）→ Claude Code web（云端沙箱会话）→ 移动端（Remote Control / Cowork Dispatch）→ Cowork（面向非开发者的通用执行 agent，与 Code 同架构）。Anthropic 用同一 agent 内核覆盖全形态，桌面端定位是「Agentic 编排器」。

**桌面端核心体验**（截至 W28/2026-07）：
- **可拖拽多面板工作区**（v1.2581.0+）：chat / diff / browser / terminal / editor / plan / tasks / subagent 面板自由拼装，布局按 repo 保存；并行会话 + git worktree 隔离（`.claude/worktrees/`，`.worktreeinclude` 复制 gitignored 文件）。
- **预览浏览器**：自动起 dev server，agent 用截图/DOM/点击/表单自检改动；支持 HTML/PDF/外部站点 tab 式预览；W28（7 月）升级为沙箱内置浏览器。
- **集成终端**（Ctrl+\`，与 agent 共享环境）；**PR 监控**：gh CLI 轮询 CI，失败自动修复（Auto-fix）、通过自动合并（Auto-merge）。
- **通知**：不在前台的会话完成/CI 完成 → OS 通知；Dispatch 任务完成推送手机。

**Agent / 自动化能力**：
- Subagent 默认后台运行（W27 起）；hooks 约 30 事件（含 TeammateIdle/TaskCreated/TaskCompleted、新增 PreModelSwitch/PostModelSwitch）；Skills + 插件（5 类组件、自动更新）。
- MCP：MCPB（原 .dxt）一键安装，第三方统计 connector 目录约 1,625 个；企业 allowlist 管控。
- **OS 级沙箱**：macOS Seatbelt、Linux/WSL2 bubblewrap，网络经代理过滤（当前仅覆盖 Bash）；云端会话全隔离。
- **Checkpoints**：每 prompt 自动快照，`/rewind` 还原代码/对话/两者。
- **Routines**（2026-04-14 研究预览）：云端托管，三类触发——定时、**API endpoint**（把 Slack/Linear 告警、webhook 指向专属端点）、GitHub 事件。已知限制：不能用 claude.ai Connectors。
- **Cowork Dispatch**：手机 QR 配对桌面，一次对话派生并管理多个任务会话，Claude 可操控本机应用。

**定价与口碑**：Pro $20 / Max 5x $100 / Max 20x $200 / Team $25-60 席；5 小时滚动 + 周限额（2026 年公告限额翻倍）。G2 均分 4.9，但社区抱怨集中：限额焦虑与行为静默劣化、权限疲劳（bypass 模式仍弹窗）、hook/sandbox bug 一串（PermissionRequest 对 subagent 不生效 #23983、excludedCommands 失效 #53012、沙箱连不上 localhost）、Routines 不支持 connectors。

**对 Shannon 的启示**：拖拽多面板 + 预览自检闭环是桌面标配要尽快对标；Routines 的「API endpoint 触发器」是 Shannon webhook 基础上低成本可抄的点；分级沙箱（本地 OS + 云端）与权限体系可以绑定成卖点；手机 QR 配对派发轻量可做。可攻击弱点：单模型锁定、限额焦虑、闭源黑盒、权限/hook 碎片化 bug。

### 2.2 OpenAI：Codex Desktop / Codex 全家桶

**矩阵与定位**：Codex CLI（Rust，Apache-2.0 开源，~114k stars）→ Codex app（桌面，macOS 2026-02-02、Windows 2026-03-04）→ 2026-07-09 并入 ChatGPT 桌面 super app（Work 模式）→ Codex Cloud / GitHub review / Slack / Linear / iOS-Android。桌面定位「Agent command center」：按项目分组的多线程（threads）并行，每 agent 独立 worktree，与 CLI/IDE 扩展共享会话与配置。

**2026-08-31「Codex for (almost) everything」大更新**：后台 computer use（macOS 伴生光标 + PiP）、in-app browser（网页划词评论→agent 处理）、gpt-image-1.5、90+ 插件、GitHub review 评论、多终端面板、SSH devbox（alpha）、文件侧栏预览、记忆（预览）、主动建议；周活开发者 300 万。

**编排/自动化细节（最值得抄）**：
- **Automations 结果进 review queue**（即 Triage）：典型用例每日 issue triage、CI 失败分析、release 简报；**Automations 可复用既有 thread**（保留原上下文续跑）。
- **Best-of-N**：`--attempts 1-4` 多方案生成择优，与 worktree 天然耦合。
- 沙箱三档 × 审批策略，`/permissions` 会话内调权。
- Agent SDK（TypeScript）2026-01-12 GA；harness 全开源但 desktop 闭源、绑 ChatGPT 账号。

**定价与口碑**：Free / Go $8 / Plus $20 / Pro $100-200 / Business；2026-04 转 token 计价后限额飙升、单任务成本被报涨 10-20x，credits 体系混乱是第一大槽点；其次 review 死循环/噪音大、模型强制退役（5.3-Codex 下架引抗议）、**多窗口/多实例仍非一等公民**（issue #33205，2026-07 仍在请求）。

**对 Shannon 的启示**：① Automations→收件箱→原会话续跑是编排灵魂，Shannon 有 routine+triage 基础，补闭环即可；② best-of-N × worktree 并排 diff 择优，Shannon 可以做得更可视化；③ 多窗口是竞品官方未满足需求，可直接做差异；④ review 严格度可调（宽松模式/噪音过滤）可攻 Codex 痛点。

### 2.3 Nous Research：Hermes Desktop

**定位**：开源（MIT）自托管「与你共同成长的 agent」，自我进化 skill（复杂任务后自动创建/改进 skill）+ 持久记忆；GitHub 241k stars（2026-09），社区规模远超同类。架构 Electron 壳 + React + Python 后端（`hermes serve` 走 JSON-RPC/WebSocket），与 CLI/TUI 同核。

**桌面端亮点**（v2026.6.5 "Surface" 首发，节奏极快）：
- **Session 即成本控制**（显式设计目标）：状态栏按类别拆解上下文占用（系统提示/工具定义/技能/记忆/规则/MCP/子代理/对话）、缓存命中率、tokens/秒、每会话 YOLO 开关；警告中途换模型会击穿 provider 缓存。
- **Profile 机制**：各 Profile 隔离配置/技能/会话，可并发、跨 Profile `@session` 引用；**整包导出 `.tar.gz`**（技能+记忆+persona+crons+主题，密钥剥离）。
- **Comment Mode**：预览浏览器点选元素打编号批注（附 CSS 选择器与计算样式、敏感属性脱敏）。
- **Artifacts 画廊**：自动索引会话产物，标注来源会话可跳回。
- Git 面板（diff/暂存/提交信息/gh PR/**worktree 管理**）、Memory Graph、HUD 悬浮、Cmd+K、6 语言（含简繁中文）、右栏持久终端。
- v0.21.0 "Pantheon"（08 月末）：**Bot Mode**——"one chat per agent" 名册、确定性头像、群聊互 @。

**Agent/自动化**：delegate_task 隔离子代理（Python RPC 把多步工具调用压成单轮，"零上下文成本"）；MEMORY.md + SQLite FTS5 + LLM 摘要 + Honcho 用户建模；Skills Hub 市集（兼容 agentskills.io）；内置 cron + **自然语言排程**；**消息网关约 20 个平台**——Telegram/Discord/Slack/WhatsApp/Signal/Matrix/iMessage/SMS/Email/LINE/SimpleX/Google Chat/MS Teams/**钉钉/飞书/WeCom**/Mattermost/Home Assistant/Webhook/A2A。模型 provider-agnostic，Ollama/LM Studio（一级集成）/vLLM 一等公民。

**商业模式**：本体 MIT 免费；Nous Portal 订阅（2026-04-27）Free/Plus $20/Super $100/Ultra $200，300+ 模型 + 10% 积分奖励——「agent 免费引流 → Portal 抽推理消费」。

**口碑**：好评在记忆/自学习闭环与平台覆盖；三大痛点——**token 消耗失控**（"cost projection is insane" 等多篇热帖）、**资源占用**（Electron+Python，VPS OOM、小模型死循环）、**Skills Hub 无安全审核**（恶意 skill 可窃取终端内容/API 密钥）。

**对 Shannon 的启示**：① 上下文按类别拆解 + 缓存命中率是「把成本变成可解释 UI」的最佳范本，直接回应竞品全网最大痛点；② Profile 整包导出（可迁移的 agent 人格）是强粘性设计；③ Artifacts 画廊 + Comment Mode 是桌面原生独占交互；④ 钉钉/飞书/WeCom 渠道对中文市场价值高；⑤ Bot Mode 名册 + 群聊是比 Team 更「产品化」的多 agent 形态。可攻击弱点：Electron+Python 重（Shannon Tauri+Rust 轻量直接对打）、token 效率差、Skills Hub 零审核（Shannon 已有提示注入扫描+签名校验）、桌面 6 月才首发。

### 2.4 腾讯：WorkBuddy

**定位核实**：腾讯云 CodeBuddy 团队出品（非微信线），「全场景职场 AI 智能体桌面工作台」，2026-02-06 内测、03-09 正式上线；完全兼容 OpenClaw Skills 生态，被媒体称「腾讯版 OpenClaw」。腾讯系分层：OpenClaw（开源开发者）→ QClaw（微信 AI 助手）→ WorkBuddy（职场办公）。

**形态**：桌面 Win/macOS + 鸿蒙 PC（7/25，品类首个）；**移动三端 iOS/Android/HarmonyOS（7/18，首个上架鸿蒙的通用智能体 App）**，8 月起手机查看/发起任务、云端同步管理多台电脑；IM 入口：微信（9 月直连全量）、企微、飞书、钉钉、QQ、Slack、Telegram、Discord + 小程序。

**核心能力**：多 Agent 并行 + 定时规则后台自动化（串+并编排、完整执行日志与错误追踪）；MEMORY.md 纯 Markdown 记忆；Skill 市场（SkillHub 最火为小红书自动化 7.8k+ 下载，**按职业角色组织**而非技术能力）；Connector 连接器；8/13 V5.3.11「资料库」升级为 AI 原生知识空间；腾讯文档「人机双写」同屏协作；**交付物一等公民**——文档/表格/PPT/图表/研报落盘交付而非对话回复。

**定价与口碑**：混元为核心 + DeepSeek/GLM/Kimi/MiniMax 等 9 款平价模型、支持自配 API；个人免费 500 Credits/月、专业版 58 元/月、国际团队 $40/席、企业版 5 月 78→198 元/人/月（+154% 引发争议）。花旗：MAU 2000 万、DAU 1300 万，公测 4 个月留存 60%+；易观 Q2：17 款桌面办公智能体月访问量第一。抱怨：平台风控（小红书自动化监控）、新手踩坑、企业版涨价。

**对 Shannon 的启示**：① IM 远程遥控桌面（微信/飞书发指令→电脑执行→进度推送）是中文市场刚需形态；② routine 产物落盘 + 执行日志（Shannon 可加 git 版本化交付）；③ 开放格式记忆（Markdown）可迁移——Shannon 应把「本地、可审计」记忆做成卖点；④ Skill 市场按「职业角色」组织内容；⑤ 办公交付物（PPT/表格）生成是 Shannon Desktop 面向知识工作者的必修课。可攻击弱点：闭源 + Credits 涨价、无 git/worktree、云端执行隐私存疑、自动化编排轻量无 hook 事件驱动。

### 2.5 智谱：ZCode

**核实**：ZCode 确为 Z.ai 官方产品（zcode.z.ai），自称「GLM-5.3 官方 Harness / 新一代氛围编程工具 / Agentic Development Environment」，与 GLM Coding Plan 订阅绑定。**桌面 Electron 为核心形态（macOS/Win/Linux，v3.11.2 2026-09-04），官方明确无 CLI**；另有移动 Remote Control、飞书/微信 Bot。

**桌面能力**：4 种执行模式（Shift+Tab：逐项确认/自动编辑/Plan/Full access）+ Safety Confirmation 敏感操作确认；**Goal Mode**（长任务目标管理、完成度验证、状态恢复）；**Thought Level** Low/High/Max 三档推理算力开关；Side Conversation（/btw 边车会话）、会话 Fork、AGENTS.md 两级 + Project Memory（默认关）；Subagents、Hooks、Automations、**Idle-time Task（闲时免费跑任务不耗套餐）**、浏览器自动化插件、Edit History、Wiki、Usage Stats；**未见**多 agent Team 编排、git worktree 隔离。Claude Code 生态兼容（Skill/Hook/Subagent/Plugin/MCP 全套），但迁移向导仅支持导入对话记录，Skills 仅部分兼容。

**定价与口碑**：2026-02-11 改 credits 积分制并涨价 ≥30%：Lite $18（¥118）/ Pro ~$72（¥538）/ Max ~$160（¥1,078），5h/周 credits 制；非高峰 5 折、凌晨 Flash 无限。英文社区共识「最便宜大厂订阅、额度大」，但顶尖质量逊于 Claude；中文社区认可上手门槛低，抱怨涨价。2026-08-11 升级 Goal/Subagents/Remote Control/闲时任务，**用户破 100 万**；GLM-5.3 于 8-14 发布。

**威胁 vs 互补判断**：**互补为主**。Shannon 已支持 GLM provider，ZCode 做大了 GLM 订阅盘，Shannon 用户可经其 Anthropic/OpenAI 兼容端点用同一份 Coding Plan。真实威胁仅「想要 GLM+桌面一体化」的人群（百万基数不可忽视）。Shannon 的差异化面：开源、多 provider 中立、CLI+桌面双形态、worktree 多 agent 隔离、hook/routine 自动化——均为 ZCode 未覆盖。可借鉴：Goal Mode 产品化、Thought Level 档位、**Idle-time Task**、（反向教训）迁移只做一半被社区吐槽。

---

## 3. 横向能力矩阵（2026-09）

| 维度 | Shannon Desktop (dev) | Claude Code Desktop | Codex app | Hermes | WorkBuddy | ZCode |
|---|---|---|---|---|---|---|
| 桌面框架 | **Tauri2+Rust** | Electron | Electron+Rust server | Electron+Python | 未公开 | Electron |
| 开源 | ✅ Apache-2.0 | ❌ | 🟡 harness 开源,app 闭源 | ✅ MIT | ❌ | ❌ |
| 多 provider | ✅ 全家桶+本地 | ❌ | 🟡 OpenAI 系 | ✅ 含本地一等公民 | 🟡 混元+9 款 | 🟡 GLM 系为主 |
| CLI 形态 | ✅(44+70 命令) | ✅ | ✅ | ✅ | ❌ | ❌ |
| 多 agent 并行 | ✅ Team(桌面 OPC/Extensions 有) | ✅ subagent 后台+面板 | ✅ threads+worktree | ✅ delegate+Bot Mode | ✅ | 🟡 subagents,无 team |
| Worktree 隔离 | ✅(桌面有创建) | ✅ | ✅ | 🟡 Git 面板管理 | ❌ | ❌ |
| **自主长任务(goal/循环)** | 🟡 CLI ✅(goal/ralph/loop+budget) / **桌面 ❌** | 🟡 routines 代偿 | ✅ automations | 🟡 cron 代偿 | ✅ 定时规则 | ✅ Goal Mode |
| **自动化结果收件箱** | 🟡 /triage 页有,闭环弱 | 🟡 routines 云端 | ✅ review queue+原 thread 续跑 | 🟡 渠道投递 | ✅ 执行日志 | 🟡 |
| 触发器类型 | 🟡 cron+webhook 通知 | ✅ cron+API endpoint+GitHub | 🟡 cron(可复用 thread) | ✅ cron+NL 排程+渠道 | ✅ 定时规则 | 🟡 automations |
| **成本可观测** | 🟡 usage 页+/cost,无拆解 | ❌(限额黑盒) | 🟡 usage stats | ✅ 按类别拆解+缓存命中 | 🟡 credits | ✅ usage+闲时免费 |
| Session 预算上限 | 🟡 仅 goal 有 | ❌ | ❌ | 🟡 per-session YOLO | ❌ | ❌ |
| **可拖拽多面板** | ❌ 固定布局 | ✅ 按 repo 保存 | 🟡 多终端面板 | ✅ HUD/Cmd+K | ❌ | 🟡 固定+侧栏 |
| 预览+自检闭环 | 🟡 ArtifactPanel 静态预览 | ✅ dev server+DOM 自检 | ✅ in-app browser+划词 | ✅ Comment Mode | 🟡 交付物预览 | ❓未查到 |
| 集成终端 | ❌ | ✅ | ✅ | ✅ | ❌ | ❓ |
| 多窗口 | ❌ | ✅ | 🟡 仍有缺陷(用户请愿) | ❓ | ❓ | ❓ |
| 消息渠道入站 | ❌(仅 webhook 通知出站) | 🟡 routines API | 🟡 GitHub/Slack/Linear | ✅ ~20 平台 | ✅ 微信/企微/飞书/钉钉/QQ… | 🟡 飞书/微信 Bot |
| 移动端派发 | 🟡 mobile pairing 命令(7 条)基础 | ✅ Dispatch+Remote Control | ✅ 经 ChatGPT app | 🟡 渠道代偿 | ✅ 三端(含鸿蒙) | ✅ Remote Control |
| OS 级沙箱 | 🟡 /sandbox flag(landlock) | ✅ Seatbelt/bwrap | ✅ Seatbelt/Landlock | 🟡 6 后端 | ➖ | 🟡 Safety Confirm |
| 记忆 | ✅ MemoryStore+桌面页 | ✅ Projects+Memory | ✅(预览) | ✅ 档案+FTS5+Graph | 🟡 MEMORY.md | 🟡 Project Memory 默认关 |
| 附件/语音 | ✅ 附件+语音(云+whisper 本地) | ✅/✅ | ✅/✅ | ✅/✅ | ✅/➖ | ❓/❓ |
| MCP 生态 | ✅ 全传输+mcpb 安装器+注入扫描 | ✅ MCPB+1625 目录 | ✅ | ✅ | 🟡 | ✅ |
| Skill 安全治理 | ✅ 提示注入扫描+签名校验(未宣传) | 🟡 allowlist | ❓ | ❌ Skills Hub 零审核(被点名) | 🟡 | ❓ |
| i18n | ✅ 10 语言 | 🟡 | 🟡 | ✅ 6 语言含简繁 | ✅ 中文 | ✅ 中英 |
| 定价 | 免费开源 BYOK | $20-200 | $8-200 | 免费+Portal $20-200 | 免费+¥58/月起 | $18-160 |

---

## 4. Shannon 当前实现快照（2026-09-05，dev @ cc663a2c）

### 4.1 相对 2026-08 基线已闭合的项（不要再投入）

| 8 月矩阵标缺的项 | 现状证据 |
|---|---|
| 附件上传 | ✅ AttachmentChip，文件/图片 |
| 语音输入 | ✅ 云 STT + 本地 whisper-rs + 模型下载管理（MicButton/VoiceOrb，11 条 voice 命令） |
| Diff viewer UI | ✅ components/diff 4 组件 + `get_session_git_diff` |
| Worktree 桌面入口 | ✅ `create_session_worktree`/`branch_session` + Tasks>worktrees 面板 |
| 定时任务 UI | ✅ scheduled_commands 21 条（cron 预览/triage/执行历史/triggered）+ 10 个内置 TOML 模板 |
| Hooks 配置 UI | ✅ Tasks>pipelines（创建对话可选 hookEvent） |
| Checkpoint/Rewind | ✅ `/rewind` 按钮 + 逐 turn checkpoint + 文件还原 |
| Remote SSH/Docker | ✅ RemotesSettings（本周新增 a2ad7777/f0ed37e0） |
| LSP | ✅ 4 命令 + Editor（CodeMirror） |
| 多会话并行 | ✅ 多会话面板 + /triage + OPC（MissionFocus/AgentSwarm/5 列 Kanban） |

### 4.2 当前能力底座（事实清单）

- **Desktop**：Tauri2+React19，211 个 `#[tauri::command]`（extensions 32、scheduled 21、sessions 13、voice 11、connections 10、plugins 9、mcp 8、agents/memory/remote/files 各 7…）；路由 welcome/chat/tasks(5 tab)/triage/usage/extensions(6 子页)/opc/editor/memory/timeline/settings(8 子页)；MCP 安装器支持 stdio/mcpb/OAuth；插件市场；数据源（Obsidian/IMAP）；**提示注入扫描+签名校验**；mobile pairing 命令；系统托盘+全局快捷键+自动更新。
- **CLI**：44 个内置 slash 命令 + TUI 专属约 70 条；headless `-p/--schema/--goal/--target/--team-agent`；**goal 系统**（system prompt 注入、存活 compaction、GOAL_COMPLETE/GOAL_BLOCKED 自动续跑、anti-spin、stall strikes、budget cap、goal_get/goal_update 工具）；**ralph/loop** 自主迭代；34 个 hook 事件中的 30 个可用（2 dead）。
- **工程**：20 个 workspace 成员；**11,273 个可运行测试 / 624 文件 / 418k LOC**（docs/metrics.md 2026-08-08 实测）；clippy -D warnings；前端 vitest+playwright 136 测试文件。

### 4.3 内部问题清单（自查发现，非竞品对比）

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| I-1 | **README 工程指标过期** | badge 写 12 crates/7,889 tests，实测 20 成员/11,273 tests（metrics.md 已更新但 README 未同步） | 开源门面失真，损害可信度（对开源产品是转化率问题） |
| I-2 | tech-debt 登记过期 | TD-2~TD-5 中 P2-4.x 实际已被 d37af11d 解决未销账；improvement-plan-2026-08 的 /rewind 已落地仍标未做 | 排期决策失真 |
| I-3 | 桌面状态层 JSONL 非 SQLite | TD-3 | 大会话/多 agent 消息检索性能上限 |
| I-4 | 桌面 composer slash 仅 12 条 | `lib/slash/commands.ts`；CLI 44+70 条 | 桌面用户无法发现/使用引擎能力（goal/loop/batch/team/profile 全不可达） |
| I-5 | billing 页标注 Demo 模式 | desktop 源码注释 | BYOK 成本故事缺最后一环 |
| I-6 | VS Code 扩展停滞 | improvement-plan-2026-08 P2-8 spike 完成未实施，Wave 3 停在 7/8 | 编码线主入口缺位（Claude Code/Codex/ZCode 均有 IDE 扩展） |

---

## 5. Gap 分析（按主题域）

> 优先级：**P0**=护城河/高性价比速赢；**P1**=显著体验与差异化；**P2**=远期。详细实施方案见配套文档。

### G1. 自主长任务（Goal/Ralph/Loop）桌面化 —— P0

- **竞品证据**：ZCode 把 Goal Mode 当核心卖点（完成度验证+状态恢复，百万用户）；Codex 的 automations 可复用 thread 续跑；Claude 靠 routines+后台 subagent 代偿。
- **Shannon 现状**：CLI 侧 goal/ralph/loop 体系 9 月刚完成 Phase 2（anti-spin/stall strikes/budget cap/goal 工具契约），**桌面端零入口**（composer 12 条 slash 不含 goal/loop；路由 goals 已重定向进 tasks）。
- **问题**：最新差异化能力锁在 CLI 里；桌面用户（非技术向）恰恰是「派活-等结果」模式的最需要者。
- **建议改进点**：桌面任务创建器支持「目标模式」（goal 文本 + 完成标记 + max turns/budget 可选）；Tasks>active 增加运行中 goal 状态（轮数/花费/strike 数/阻塞原因）；完成后进 Triage。

### G2. 自动化「最后一公里」：收件箱 + 触发器矩阵 —— P0

- **竞品证据**：Codex Automations→review queue+原 thread 续跑；Claude Routines 三类触发（cron/**API endpoint**/GitHub 事件）；Hermes cron+自然语言排程+渠道投递。
- **Shannon 现状**：routine 后端完整（scheduled_routines/worktree/retry、21 条桌面命令、triage 页、webhook 通知出站、remote_trigger）。
- **问题**：① routine 结果与普通任务混排，没有「待处理收件箱」心智；② 触发器只有 cron+webhook 通知，无 HTTP endpoint 入站触发、无 GitHub 事件触发；③ 无「回原会话续跑」。
- **建议改进点**：Triage 升级为自动化收件箱（来源标记/一键重跑/在原会话续跑）；`shannon serve` 暴露 per-routine API endpoint（HMAC 已有基础）；GitHub webhook 触发器。

### G3. 成本可观测与预算控制 —— P0

- **竞品证据**：全部竞品评论区第一大抱怨是限额/成本（Claude 限额焦虑、Codex token 计价混乱、Hermes token 失控、WorkBuddy/ZCode 涨价争议）；Hermes 的按类别上下文拆解+缓存命中率是唯一把成本做成可解释 UI 的。
- **Shannon 现状**：usage 页、/cost、goal budget cap 存在；无上下文按类别拆解、无缓存命中率展示、通用 session 无预算上限、billing 是 Demo。
- **问题**：Shannon 的 BYOK+多 provider 本该讲「成本透明可控」故事，但 UI 拿不出证据；session 中途换模型击穿 provider 缓存的警告也没有。
- **建议改进点**：Chat 底栏/状态栏显示上下文构成拆解 + 缓存命中率 + tokens/s；session 级预算上限（超限暂停+询问，复用 goal budget 机制）；换模型缓存击穿警告。

### G4. 桌面工作区形态：多窗口、可拖拽面板、集成终端、预览自检 —— P1（分步）

- **竞品证据**：Claude Code 拖拽面板按 repo 保存 + 预览浏览器 DOM 自检；Codex in-app browser+划词评论+多终端面板，但**多窗口仍是官方未满足需求**（#33205）；Hermes Comment Mode。
- **Shannon 现状**：单窗口、固定布局、无集成终端；ArtifactPanel 仅静态 HTML/Mermaid 预览。
- **问题**：桌面「编排指挥中心」的物理形态缺位；预览只能看不能用来自检。
- **建议改进点**：分期——① 多窗口（每任务/每会话独立窗口，打 Codex 短板）；② 可拖拽面板布局（chat/diff/preview/terminal 四类起步，按 project 保存）；③ 预览面板起本地 dev server + 截图回传给模型自检；④ 集成终端（xterm，共享引擎环境变量）。

### G5. 消息渠道与移动派发 —— P1（渠道）/ P2（移动）

- **竞品证据**：Hermes ~20 平台（含钉钉/飞书/WeCom）；WorkBuddy 微信直连全量+移动三端+遥控多电脑；Claude Dispatch 手机派发；ZCode 飞书/微信 Bot。
- **Shannon 现状**：webhook 通知出站（slack/discord/feishu/wechat 模板+HMAC）、remote_trigger、mobile pairing 命令 7 条（基础存在）。
- **问题**：只有「通知出去」，没有「任务进来」；移动配对有命令无产品化 UX。
- **建议改进点**：第一批入站渠道做 Telegram/Discord/Slack（Bot API 成本低）+ 飞书/钉钉（中文市场）；入站消息→创建 goal 任务→进度回推；移动端复用 pairing 做审批+派发 MVP。

### G6. 沙箱与权限的产品化 —— P1

- **竞品证据**：Claude/Codex 的 OS 级沙箱（Seatbelt/bwrap/Landlock）是安全口碑来源；Codex「3 档沙箱 × 审批策略 + /permissions 会话内调权」UX 清晰；ZCode 4 模式 Shift+Tab 切换。
- **Shannon 现状**：权限体系（5 级+LLM 分类器）比竞品先进，但 `/sandbox` 是 CLI flag、permission profiles 无桌面页面（save_custom_profile 命令存在但无 UI）。
- **建议改进点**：桌面 Settings 增加沙箱档位 + 执行模式快捷切换（对标 Shift+Tab）；Profiles 独立页；把「为什么被批准/拒绝」（LLM 分类器置信度）展示出来——这是竞品没有的透明度卖点。

### G7. 扩展生态与迁移 —— P1/P2

- **竞品证据**：Claude MCPB+1,625 connector 目录；Codex 90+ 插件；ZCode 靠「Claude Code 迁移向导（仅对话记录）」降低换入门槛——做得不彻底被社区吐槽；Hermes Skills Hub 零审核被安全点名。
- **Shannon 现状**：mcpb 安装器+OAuth 已有（对标 MCPB 已及格）；提示注入扫描+签名校验已有（安全治理**超过** Hermes）；缺 connector 目录策展与迁移向导。
- **建议改进点**：① 「从 Claude Code / ZCode 迁移」向导（全量：settings/MCP/skills/commands，一次做全超越 ZCode）；② Extensions 首页做策展目录（复用 featured 子页）；③ 把注入扫描+签名做成发布页可展示的安全徽章。

### G8. 记忆与 Agent 资产可迁移 —— P2

- **竞品证据**：Hermes Profile 整包导出（技能+记忆+persona+crons，密钥剥离）、Memory Graph；WorkBuddy MEMORY.md 开放格式。
- **Shannon 现状**：MemoryStore+桌面 memory 页；无导出/导入、无图谱可视化。
- **建议改进点**：persona/profile 打包导出导入；记忆条目溯源（来源会话跳转，对标 Artifacts 画廊思路）。

### G9. 办公交付物 —— P2

- **竞品证据**：WorkBuddy 交付物落盘（PPT/表格/研报）+ 按职业角色的 Skill 市场，MAU 2000 万验证了办公线需求。
- **Shannon 现状**：引擎是通用的（文档/邮件/研究可做），artifact 预览有，但无「成品交付」语言与模板资产。
- **建议改进点**：内置办公交付 skills（docx/xlsx/pptx 生成）+ 「交付物」视图（会话产物聚合，对标 Hermes Artifacts 画廊）。

### G10. 工程与发布基建 —— P0(速赢)/持续

- I-1 README 指标更新（0.5d 速赢）；I-2 tech-debt/改进计划销账；I-3 桌面状态层 SQLite 化（TD-3，随 G1/G2 的数据需求一并做）；I-6 VS Code 扩展重启决策。

---

## 6. Shannon 应保持/放大的独有优势

1. **Rust+Tauri 轻量**：对打 Hermes(Electron+Python OOM 抱怨)/ZCode(Electron)/Claude(Electron) 的性能与体积牌，应在官网与 README 量化（安装包/内存对比）。
2. **成本透明组合拳**：BYOK+多 provider+本地模型+（新增 G3）上下文拆解/缓存命中/预算上限 —— 正面进攻全行业限额焦虑。
3. **安全治理**：5 级权限+LLM 分类器+提示注入扫描+签名校验+landlock flag —— Hermes Skills Hub 零审核、Claude hook bug 一串的对照组；缺的只是 UI 呈现（G6）。
4. **Hook/routine 事件驱动自动化**：30 事件 × 多触发器路线（G2 做完后超过 Claude Routines 的触发器矩阵）。
5. **双形态同核**：CLI+Desktop+HTTP API 共享引擎，ZCode 无 CLI、WorkBuddy 无开发向能力，Shannon 是唯一全形态开源选项。
6. **Record/Replay 测试体系**：独有能力，是「可靠、可审计」叙事的工程证据。

---

## 7. 威胁评估与监测名单（季度复查）

| 竞品 | 威胁级别 | 主要威胁面 | 升级为正面威胁的条件 |
|---|---|---|---|
| Claude Code/Desktop | 高（全维度） | 生态+品牌+全形态 | 若开放 provider 或大幅降价 |
| Codex app | 高 | threads/best-of-N/开源 harness | 若 desktop 开源或支持第三方模型接入 |
| Hermes | 中高（同赛道开源） | 社区规模(241k star)+渠道广度+自进化 skill | 若解决 token 失控与资源占用问题 |
| WorkBuddy | 中（办公线） | 移动+IM 入口+交付物+腾讯渠道 | 若出开发者向能力（git/worktree）或开源 |
| ZCode | 中低（互补为主） | GLM 桌面一体化+价格 | 若补齐 CLI/多 provider 或 Shannon 用户被其端点锁死 |
| （保留监测）openworker / OpenClaw / Cursor | 中低 | 同 8 月矩阵结论 | 同 8 月矩阵 |

---

## 附录 A：主要信息源

**Claude**: [code.claude.com/docs/en/desktop](https://code.claude.com/docs/en/desktop) · [whats-new 2026-w27/w28](https://code.claude.com/docs/en/whats-new/2026-w28) · [routines](https://code.claude.com/docs/en/routines) · [sandboxing](https://code.claude.com/docs/en/sandboxing) · [checkpointing](https://code.claude.com/docs/en/checkpointing) · [Cowork 发布](https://venturebeat.com/technology/anthropic-launches-cowork-a-claude-desktop-agent-that-works-in-your-files-no) · [桌面重设计 2026-04-14](https://venturebeat.com/orchestration/we-tested-anthropics-redesigned-claude-code-desktop-app-and-routines-heres-what-enterprises-should-know) · [限额明细](https://portkey.ai/blog/claude-code-limits) · [hook bug #23983](https://github.com/anthropics/claude-code/issues/23983) / [#53012](https://github.com/anthropics/claude-code/issues/53012)

**Codex**: [introducing the codex app](https://openai.com/index/introducing-the-codex-app/) · [codex for almost everything 2026-08-31](https://openai.com/index/codex-for-almost-everything/) · [Agent SDK GA](https://developers.openai.com/blog/agent-sdk-ga/) · [multi-window issue #33205](https://github.com/openai/codex/issues/33205) · [限额讨论](https://community.openai.com/t/codex-rate-limits-discussion-thread/1378553) · [Reuters 2026-02-02](https://www.reuters.com/business/media-telecom/openai-launches-codex-app-gain-ground-ai-coding-race-2026-02-02/)

**Hermes**: [官网](https://hermes-agent.nousresearch.com/) · [桌面文档](https://hermes-agent.nousresearch.com/docs/user-guide/desktop) · [配置/渠道](https://hermes-agent.nousresearch.com/docs/user-guide/configuration) · [GitHub releases](https://github.com/NousResearch/hermes-agent) · [changelog](https://hermes-ai.net/changelog/) · [Portal 定价](https://portal.nousresearch.com/) · [token 失控帖](https://www.reddit.com/r/hermesagent/comments/1tf3f2f/) / [1sac9rk](https://www.reddit.com/r/hermesagent/comments/1sac9rk/)

**WorkBuddy**: [官网](https://www.workbuddy.ai/) · [定价](https://www.workbuddy.cn/docs/workbuddy/Pricing) · [企业涨价 财联社](https://www.cls.cn/detail/2357934) · [移动三端](https://news.mydrivers.com/1/1137/1137317.htm) · [市场数据 钛媒体](https://www.tmtpost.com/8107000.html) · [OpenClaw/QClaw/WorkBuddy 分层](https://cloud.tencent.com/developer/article/2669724)

**ZCode**: [zcode.z.ai docs](https://zcode.z.ai/en/docs/welcome) · [Coding Plan](https://docs.z.ai/devpack/overview) · [GLM-5.3](https://z.ai/blog/glm-5.3) · [定价对比](https://www.aipricing.guru/z-ai-subscription-pricing/) · [V2EX 涨价讨论](https://www.v2ex.com/t/1231115) · [r/ZaiGLM](https://www.reddit.com/r/ZaiGLM/comments/1ui1p56/)

**内部**: desktop/COMPETITIVE-ANALYSIS.md (2026-06-13) · docs/competitor-feature-matrix.md v2 (2026-08-02) · docs/improvement-plan-2026-08.md v4 · docs/metrics.md (2026-08-08) · dev 分支代码盘点（211 Tauri commands、goal/ralph Phase 2 等）
