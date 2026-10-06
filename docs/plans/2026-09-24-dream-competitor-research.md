# 竞品调研：Dream——智能体「提炼资料与技能」机制（v1.0）

- 日期：2026-09-24
- 状态：供评审
- 结论先行：**Dream 不是一个独立产品，而是 2026 年竞品阵营围绕「异步会话间提炼」形成的一类机制的总称**——以 Anthropic Claude Managed Agents 的 **Dreams** 与 Claude Code 的 **Auto Dream / `/dream`** 为源头，OpenClaw、Hermes 等开源生态快速跟进，Microsoft 发布了研究变体（Shadow-Frog）。它的本职是「整理记忆」，社区正在把它延伸到「提炼技能」。shannon 的记忆侧基建（AutoDreamService / MemoryConsolidator）已经对标了 Claude Code 的同名服务，但**LLM 深度提炼、影子输出、审查落地、技能侧打通**四环缺失——这是本调研对应的功能方案（见 `2026-09-24-dream-distill-feature-design.md`）要补的。
- 术语澄清：检索未发现名为 "Dream" 的独立竞品应用；若你指的是某个具体产品，请指出，我会补调研。

---

## 1. 起源与脉络

| 时间 | 事件 | 意义 |
|---|---|---|
| 2025-05 | Voyager（arXiv 2505.07634）：Minecraft 智能体用「梦境阶段」自主习得技能并沉淀技能库 | 学术源头：idle 时段 → 技能/知识沉淀 |
| 2025 | Agent Workflow Memory（AWM）：从过往任务轨迹归纳可复用工作流 | 「从会话归纳程序性知识」成型 |
| 2025-10 | Claude Code 上线 Agent Skills（SKILL.md）+ auto-memory（官方明说命名映射 REM 睡眠：短期记忆→长期巩固） | 记忆自动化的官方化 |
| 2026-05 | **Claude Code "Dreaming / Auto Dream"**：会话间异步的记忆巩固 sub-agent 曝光/推送；手动 `/dream` 命令可用 | Dream 机制成为行业话题（媒体广泛报道，另有泄露架构中 `autoDream` 为显式工程机制的报道） |
| 2026 | Claude Managed Agents 平台正式文档化 **Dreams**（异步整理作业，API 化） | 从 CLI 秘诀升级为一等平台能力 |
| 2026 上半年 | 开源生态跟进：OpenClaw（Agent Dream skill、Memory LanceDB Dreaming 插件）、dream-skill、Hermes Agent dreaming 提案、Bitterbot dream engine | 模式被社区复刻为「夜间整理 + 自省」 |
| 2026-06 | Microsoft Debug Gym 发布 **Shadow-Frog**：变体「dream tasks」（空闲时自生成实验） | 另一分支：不做整理，做「执行验证型」提炼 |

> 二手时间线参考：Medium《Agents Got a Sleep Phase Before It Was Consensus They Needed One》（Dreaming/Auto Dream 标注 2026-05）；Ken Huang Substack《Why AI Agents Are Starting to Dream》（定义：dreaming = 空闲/后台时段用模型推理做异步记忆策展）。

## 2. Anthropic Dreams（官方一手拆解）

来源：platform.claude.com《Dreams》（Managed Agents 文档，经 web reader 抓取全文）。

**定位**："Let Claude reflect on past sessions to curate an agent's memory and surface new insights."

**动机**：记忆写入是本地、增量的——多次会话后记忆库必然堆积重复、矛盾、过时条目；会话继续来不及时清理。Dream 负责清理。

**作业模型（最关键的设计）**：

| 维度 | 设计 |
|---|---|
| 输入 | ① 一个**已存在的 memory store**；② **1–100 份过往会话转录** |
| 输出 | 一个**全新的 output memory store**——重复合并、过时/被矛盾的条目以最新为准替换、并浮现新洞见 |
| 非破坏性 | **"The input store is never modified, so you can review the output and discard it."** 输入库永不修改 |
| 触发 | 手动/API 发起的**异步作业**（数分钟到数小时），按作业 ID 轮询；`session_id` 暴露底层管道会话可流式观察；终态后自动归档 |
| 模型 | 可指定（如 opus-5 / sonnet-5 等映射） |
| 引导 | 可选 `instructions`——高层综合方向（"侧重最近的架构决策"），不是逐行编辑指令 |
| 生命周期 | pending → running → completed/failed；failed 作业保留部分产物供检查；可 cancel / archive / list |
| 计费 | 标准 token 价，成本随**会话数量 × 长度**线性增长 |
| 限制 | 研究预览期有用量上限；官方建议**先小批量再批量** |

**Claude Code 侧**：`/dream` 手动触发 + auto-dream（后台 sub-agent，会话间运行：修剪过时笔记、去重、修矛盾、重组所学）。注意区分：auto-memory（自动保存偏好/纠正/模式到 `~/.claude/projects/<project>/memory/`）与 dream（对已积累记忆的周期性重组）是两层。

**技能侧的官方口径（对我们很重要）**：Claude 官方帮助中心明确 skills 是**用户手写**的——"If you do something more than once a day, turn it into a skill"。**Anthropic 没有官方「会话 → 技能」自动管道**；dream 只管记忆。把 dream 延伸到技能的是社区（见 §3）。

## 3. 开源生态跟进

| 项目 | 机制 | 可借鉴点 |
|---|---|---|
| **OpenClaw · Agent Dream skill** | "Nightly memory consolidation and self-reflection... reviewing sessions, organizing memories" —— 夜间回顾会话、整理记忆、自省 | 夜间触发 + 自省产物 |
| **OpenClaw · Memory LanceDB Dreaming 插件** | **Light / REM / Deep 三档**整合；产出 `DREAMS.md`（梦境日记，双语）；**Deep Sleep = 加权评分 + 阈值门控 + 回源验证后才写入持久记忆（唯一写入路径）** | ① 分档控制成本；② 日记作为人可读产物；③ 「唯一写入路径」的写入门控 |
| **dream-skill（GitHub）** | "Consolidates memory while you sleep"；自述受 Claude Code auto-dream 启发 | 简版夜间整理参考实现 |
| **Hermes Agent（NousResearch）** | 社区 feature request（#25309、#10771）：安静时段把短期记忆整合进长期记忆，明说受 Claude Code Auto Dream 启发；另有社区 "Dreaming Skill"、"Memory Lean Check" | 「安静时段」触发命名与需求表述；社区在争论「自写技能（程序性记忆）vs 托管 dreaming」的分工 |
| **Bitterbot Desktop** | dream engine 持续从对话挖掘新记忆；本地优先 | 持续型 vs 周期型触发的取舍 |

社区机制文章：知乎把这类「异步离线 memory 重新梳理整合」统称为 Claude Code 的 Dream 功能，指出其**慢、贵、周期性**（不利于快速记忆更新，需要与实时提取分层）；CSDN 总结 Sleep 阶段分工——提取信号（抽象思考）→ Deep Sleep（加权评分 + 阈值门控 + 回源验证后写入）。

## 4. 其他竞品对照

| 竞品 | 提炼资料/记忆 | 提炼 skill | 触发时机 | 备注 |
|---|---|---|---|---|
| **Claude（官方）** | auto-memory（实时）+ Dreams/Auto Dream（异步重组，影子输出+审查） | ❌ 官方明确 skills 手写 | 手动 `/dream`、会话间后台、平台 API 异步作业 | 生态最完整 |
| **Codex** | Memories：把合格历史会话提炼为**本地 memory files**；跳过活跃/短会话；**敏感内容脱敏** | ❌（Custom prompts 手写） | 会话后异步；社区在要求 session-end consolidation | 与 ChatGPT 记忆打通是社区诉求 |
| **ZCode** | Memory：自动提取项目偏好与团队约定，后续会话自动带入 | 闲时任务官方示例含「自动整理会话、提炼记忆」——但载体是**用户显式创建的闲时任务**，非隐式自动 | 实时提取 + 用户调度闲时 | 「闲时任务」是 dream 型工作的基础设施（2026-08-11 随 Goal/Subagents/Remote Control 发布） |
| **Devin / Copilot** | Devin Knowledge/Playbooks 手动提交；Copilot Mission Control 偏任务编排 | ❌ 未见 dream 型功能 | — | 未见跟进（截至本次检索） |
| **Microsoft Shadow-Frog（研究）** | **变体**：dream tasks = 空闲时自生成实验（新功能/重构/安全审计），在隔离分支真实实现；产物是 `.shadow/` 下**带 provenance（exploration/user/interaction）与 verification 状态（verified/uncertain/refuted）**的行为事实 | 明确**不是**技能/工作流蒸馏——与整理型 dreams（Auto-Dreamer、AWM、Claude Dreams）划清界限 | 夜间/周末空闲 | 数据：检索召回 97.6% vs 平面知识文件 36.2%；盲找 bug +25.4 分 |

## 5. 模式共性拆解（跨竞品归纳）

1. **时机**：三类——手动命令（`/dream`）、会话间异步（auto-dream）、夜间/安静时段（OpenClaw、Hermes 诉求）。没有竞品做「活跃会话内阻塞式整理」。
2. **输入**：一律是「已有记忆库 + 会话转录」双输入（Dreams 官方明示 1–100 份；Codex 跳过短会话）。
3. **产物**：记忆重组（官方主线）、人可读日记（DREAMS.md）、验证事实（Shadow-Frog）、**技能候选（社区延伸，官方均未做）**。
4. **安全共识**：影子输出/非破坏（Claude 官方 "input store is never modified"）、写入门控（OpenClaw Deep Sleep 唯一写入路径 + 回源验证）、审查后再落地——因为**自主改写自身记忆有漂移与错误强化风险，受损会话会产生「毒化教训」（poisoned lessons）**（The New Stack 等多篇评论警告）。
5. **成本**：LLM 提炼贵且慢（知乎：慢、贵、周期性）→ 分档（Light/REM/Deep）、夜间错峰、小批量先行、与零成本实时提取分层。

## 6. shannon 现状盘点 vs 差距

| 能力 | Claude Dreams 对应 | shannon 现状 | 差距 |
|---|---|---|---|
| 实时记忆提取 | auto-memory | ✅ `AutoDreamService`：每查询后 fire-and-forget 关键词式提取（`agent_loop.rs:4728`，类别 preference/decision/error/pattern） | 零 LLM、无会话语义级提炼 |
| 定期整理 | dream 重组 | ✅ `MemoryConsolidator` + `compaction_trigger`：24h 或 ≥5 会话节奏的规则压缩（去重/过期/分类上限 + token 预算裁剪，sidecar state） | 规则式；无 LLM 合并提案 |
| LLM 深度整理 + 影子输出 | 官方核心设计 | ⚠️ `ConsolidationPrompt`（keep/merge/remove JSON 协议，对标 Claude Code consolidationPrompt.ts）与 `ConsolidationLock`（最小间隔/单飞）**已在 shannon-core 落库但未接桌面链路**；无「影子提案→审查→应用」语义 | **本方案核心缺口** |
| LLM 会话提炼管道 | — | ⚠️ `extract_memories.rs`（对标 Claude Code extractMemories.ts，cursor + 锁）在库；桌面主链路走的是关键词式 AutoDreamService | 未接线 |
| 提炼报告/日记 | DREAMS.md（OpenClaw） | ❌ 无 | 新增 |
| 技能提炼 | ❌ 官方无（skills 手写）；社区延伸 | ✅ **领先点**：`skill_pattern_detection`（启发式候选）→ `refine_skill_candidate`（LLM 精炼）→ promote 全链已通，含收件箱卡与三动作 | 缺触发接线（待审的接线设计稿）与「从会话直接提炼程序性技能」 |
| 手动命令 | `/dream` | ❌（接线设计稿已提案 `/detect-skills`） | 待接线 |
| UI 入口 | 官方无专门 UI（Console/API） | ✅ `Memory.tsx` 页面已存在；收件箱 Triage 八源机制现成 | 挂载点现成，缺 Dream 专属 UI |

**定位结论**：shannon 不需要从零造 Dream——记忆侧「实时提取 + 规则压缩」两层已上线且对标正确；缺口集中在**第三层（LLM 异步提炼：影子提案 + 日记 + 审查落地）和技能侧打通**，且这两块恰好是竞品官方都没做完、生态刚刚验证过的部分。按 2026-09-23 综合方案的任务规划，本项排在「技能检测接线（任务1）」「会话归档（任务2）」之后作为任务3。

## 7. 对 shannon 的启示

1. **影子输出 + 审查落地是行业安全共识**，与我们技能候选的「先候选后采纳」链完全同构——记忆提案应复用同一交互范式（见设计稿 §4）。
2. **分档控制成本**（OpenClaw 三档 → 我们：L1 规则整理免费 / L2 记忆提炼 LLM / L3 技能提炼 LLM），夜间与手动触发默认承担 LLM 档。
3. **日记型人可读产物**（DREAMS.md）值得抄：它是唯一能让用户「看得见」提炼价值的东西。
4. **provenance + verification 状态**（Shadow-Frog）：每条提炼产物标注来源会话与 verified/uncertain，是应对「毒化教训」的最小机制。
5. **技能侧是 shannon 的差异化机会**：Claude 官方不做自动技能提炼，社区刚起步；我们的候选链 + 接线设计落地后即是领先布局。

## 8. 参考链接

一手：
- Claude Platform Docs · Dreams: https://platform.claude.com/docs/en/managed-agents/dreams （经 reader 抓全文）
- Claude Code power user tips（/memory、/dream、skills 手写口径）: https://support.claude.com/en/articles/14554000-claude-code-power-user-tips
- Microsoft Debug Gym · Shadow-Frog: https://microsoft.github.io/debug-gym/blog/2026/06/shadow-frog （经 WebFetch 全文）
- shannon 代码库：`crates/shannon-core/src/memory/auto_dream.rs`、`memory/compaction_trigger.rs`、`memory/consolidator.rs`、`auto_dream_consolidation.rs`、`extract_memories.rs`、`preference_memory.rs`、`desktop/src/commands_memory.rs`

二手（媒体/社区，标注置信度中）：
- Ken Huang · Why AI Agents Are Starting to Dream: https://kenhuangus.substack.com/p/why-ai-agents-are-starting-to-dream
- Medium · Agents Got a Sleep Phase...: https://medium.com/@roanmonteiro/agents-got-a-sleep-phase-before-it-was-consensus-they-needed-one-dcdccf8e081d
- OpenClaw Skills Directory（Agent Dream）: https://openclawai.io/skills ；Memory LanceDB Dreaming: https://hub.openclaw.ai
- dream-skill: https://github.com/grandamenium/dream-skill
- Hermes Agent dreaming 需求：NousResearch 仓库 issue #25309、#10771
- ChatGPT Learn docs · Memories（Codex 本地记忆文件/脱敏）；OpenAI Community #12567（Codex memory 讨论）
- ZCode 四大功能与闲时任务：证券时报（stcn.com，2026-08-11）、ai-bot.cn ZCode 词条
- arXiv 2505.07634（Voyager 系梦境技能习得）、arXiv 2602.20867（SoK: Agentic Skills）
