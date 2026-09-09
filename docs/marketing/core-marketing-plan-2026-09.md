# Shannon 核心宣传方案（2026-09）

- **日期**: 2026-09-10 ｜ **视角**: 高级产品经理 × 高级广告总监
- **依据**: [competitive-research-2026-09](../competitive-research-2026-09.md)（2026-09-05）· [grok-bots-research-2026-09](../research/grok-bots-research-2026-09.md)（2026-09-08）· [multimodal-capabilities-research-2026-09](../research/multimodal-capabilities-research-2026-09.md)（2026-09-08）· [04-product-repositioning](../../desktop/docs/product-review/04-product-repositioning.md) · dev @ c3bc5647（v0.11.0-wip）代码盘点
- **状态**: ⏳ 待审核（决策点见 §9）
- **配套**: [页面与文档改进方案](./page-and-docs-improvement-plan-2026-09.md)（本方案落地到 README / website / docs 的执行方案）

---

## 0. TL;DR（30 秒版）

1. **成果一句话**：90 天内（2026-07 → 09，1,481 个非 merge commit），Shannon 从「一个终端编程工具」变成了「**一个 Rust 引擎、四个表面（终端 / headless / 服务 / 桌面）、任意模型**的开源 AI 工作台」，并补齐了多模态（视觉/语音/浏览器/computer use）、自动化（IM 入站/移动派发/定时触发器矩阵）、隐私（secret-guard 插件）与评测（SWE-bench/TB 官方判分管线）四大板块。
2. **竞争卡位**：2026 年头部竞品全部收敛到「云端订阅 + 单一模型 + 额度制」；全行业评论区第一大抱怨是**成本与失控**。Shannon 是唯一同时做到「开源 + 任意模型 + 本地优先 + 全形态同核」的产品——结构性站在了行业痛点的对面。
3. **核心叙事（Message House 屋顶）**：**「Agent 的下一个战场不是更大的模型，而是谁控制 agent 的运行时。Shannon 把控制权交还给你：任意模型、任意任务、你自己的电脑。」**
4. **主口号**：EN — **"Your AI workspace. Any model. Your machine."** ｜ CN — **「你的 AI 工作台：模型随便换，任务随便派，数据不出门。」**
5. **六大潜在爆点**：额度焦虑税（成本计算器）、云 VS 本机 agent 之争（蹭 Grok Bot 热度打对位）、agent 舰队可视化、IM/手机派活、AI 行车记录仪（trace 回放）、secret-guard 安全叙事。详见 §4。
6. **红线**：评测数字必须带 n/日期/锚点；不发无来源对比；旧文案中 VS Code 扩展、"7,889 tests"、v0.1.0 等过期事实必须先清理再宣传（详见配套改进方案）。

---

## 1. 成果盘点：过去 90 天发生了什么（宣传的证据库）

> 宣传的第一原则：**先有证据，后有文案**。以下每一条都是可演示、可截图、可 repo 取证的事实。

### 1.1 产品形态跃迁：从 CLI 到「一个引擎，四个表面」

- 7 月完成 monorepo 化与 v0.7.0 统一发布（单 tag 发全部产品、一键 install.sh）——**这是「产品矩阵」叙事的起点**。
- ADR-0011 确立「单一产品、多表面分发」：同一个 Rust 引擎，服务终端 TUI（`shannon`）、headless（`shannon -p`）、引擎守护进程（`shannon serve`）、桌面 GUI（`shannon desktop`）。
- 会话跨表面互通（事件溯源 `events.jsonl` 单一权威记录 + `--resume`）：**「终端里开始，桌面上继续」**是可演示的差异化体验，竞品无一做到同核四表面。
- Desktop 单独看也是重量级交付：Tauri 2 + React 19（非 Electron）、211 个 Tauri 命令、Simple/Advanced 双模式（对非程序员友好 vs 对开发者完整）。

### 1.2 能力里程碑（按用户价值归类）

| 板块 | 已交付（2026-07 → 09） | 用户价值一句话 |
|---|---|---|
| **自主任务** | `/goal` `/loop` `/ralph` Phase 2：进度制守卫、anti-spin、stall strikes、`--budget $N` 预算上限、退避重试；桌面 Goal 入口 + 运行看板 | 「派个目标而不是派个 prompt，睡觉前它自己跑」 |
| **多模态** | 截图/图片全链路（附件、`/image`、AnalyzeImages 批量视觉）、computer use 截图闭环（降采样省 4x token）、本地浏览器自动化（chromiumoxide）、PDF 附件、语音三路径（CLI whisper / 云 STT / 桌面本地 whisper-rs 零出站） | 「它第一次真正看得见你的屏幕，听得见你说话」 |
| **自动化与触达** | IM 五渠道入站（Telegram/Discord/Slack/飞书/钉钉）、移动派发 MVP（扫码配对→手机审批/派发）、触发器矩阵（cron / HMAC API endpoint / GitHub 事件）、Triage 收件箱闭环（结果→收件箱→原会话续跑）、桌面日历 + DAG 视图 | 「在飞书群里 @ 一句，家里的电脑开始干活」 |
| **编排** | 多 Agent Teams（OS 进程级 teammate + worktree 隔离）、`/batch` best-of-N 并行 + 桌面并排 diff 择优、可拖拽多面板工作区、集成终端、预览自检（起 dev server 截图自检） | 「一支跑在你电脑上的 agent 舰队，方案并行、择优合并」 |
| **安全与隐私** | **secret-guard 插件 + shannon-plugin-api**（出站脱敏中间件，字节稳定保 prompt cache——工程上独家）、会话 RedactionPolicy、Landlock/Seatbelt 沙箱、5 级权限 + LLM 分类器、提示注入扫描 + 签名校验、凭据只进 OS keyring | 「你的密钥永远不会离开你的机器，更不会进模型上下文」 |
| **可信工程** | 事件溯源会话 + `shannon trace show/replay/diff`（agent 行为全程可回放）、**agent-eval-bench**（SWE-bench Verified / Terminal-Bench 官方判分管线 + held-out 纪律）、11,752 自动化测试 / 418k 行 Rust / clippy 零警告、cargo-semver-checks API 稳定门禁、macOS 真机验证批次 | 「agent 黑盒时代的第一台行车记录仪」 |
| **生态兼容** | Claude Code 生态兼容（CLAUDE.md / `.claude/` agents / skills / hooks / `.mcp.json`）、迁移向导（Claude Code / ZCode 全量导入）、10 语言 i18n、GLM/DeepSeek/Ollama/任意 OpenAI 兼容端点 | 「5 分钟搬进来，原来的家当全都能用」 |

### 1.3 工程质量资产（对外可引用的信任背书）

- **11,752 自动化测试 · 418,458 行 Rust · 624 源文件 · 20 workspace 成员**（`docs/metrics.md` 单一事实源，脚本注入 README，CI 防漂移）。
- clippy `-D warnings` 零告警、cargo-deny 通过、semver 基线门禁——对开源项目而言，「工程纪律」本身就是卖点。
- 评测方法论可对外引用：SWE-bench Verified 50 题 pin（官方 docker 判分）、Terminal-Bench 适配器、跨模型消融；**引用纪律：任何对外数字必须带 n / 日期 / 锚点三元组**（`docs/agent-eval-plan-2026-09.md`）。内部最好成绩 SWE-bench 33/50（66%）为真实判分，可诚实引用。

---

## 2. 竞品对比：异同与位置

> 详细逐家深析见 `docs/competitive-research-2026-09.md` §2 与两份 research 文档；此处给营销视角的浓缩结论。

### 2.1 竞争格局一张图

```
                      云端执行 / 订阅额度制
                            ▲
          Claude Code/Desktop │ Codex app/ChatGPT
          （闭源·单模型·限额焦虑）│（闭源·OpenAI 系·credits 混乱）
                            │        Grok Bot
                            │        （云 VM 常驻 bot·周额度·共享凭据池）
   通用任务 ────────────────┼─────────────────────── 编码专精
                            │
          WorkBuddy         │  Hermes（开源·Electron+Python·token 失控）
          （闭源·腾讯系·办公）│  ZCode（闭源·GLM 绑定·无 CLI）
                            │  Grok Build CLI（开源·xAI 模型绑定）
                            │  ★ Shannon：开源 + 任意模型 + 本地优先 + 全形态
                            ▼
                      本机执行 / BYOK 按量付费
```

### 2.2 对比矩阵（营销浓缩版，截至 2026-09-09）

| 维度 | Shannon | Claude Code/Desktop | Codex app | Hermes | ZCode | WorkBuddy | Grok Build/Bot |
|---|---|---|---|---|---|---|---|
| 开源 | ✅ Apache-2.0 全开源 | ❌ | 🟡 harness 开源，app 闭源 | ✅ MIT | ❌ | ❌ | 🟡 CLI 开源 |
| 模型 | ✅ 任意（Anthropic/OpenAI/DeepSeek/GLM/Ollama/兼容端点） | ❌ 仅 Claude | ❌ OpenAI 系 | ✅ 含本地 | ❌ GLM 为主 | 🟡 混元+9 款 | ❌ 仅 xAI |
| 形态 | ✅ TUI+headless+serve+桌面同核 | ✅ CLI+桌面+web | ✅ CLI+桌面+云 | ✅ CLI+桌面 | ❌ 仅桌面 | ❌ 桌面+移动 | CLI / 独立 app |
| 执行位置 | ✅ 本机 | ☁️ 云端沙箱+本机 | ☁️ 云+本机 | 本机 | 本机 | 本机+云 | ☁️ 云 VM（共享） |
| IM/移动派活 | ✅ 5 渠道入站+扫码派发 | 🟡 Dispatch | 🟡 Slack/GitHub | ✅ ~20 平台 | 🟡 飞书/微信 Bot | ✅ 微信/企微+三端 | ✅ X 集成+移动 |
| 成本模型 | ✅ BYOK 按量 + 预算上限 + 上下文拆解可见 | ❌ 订阅+滚动限额 | 🟡 credits 混乱 | ❌ token 失控投诉 | 🟡 credits 涨价 | 🟡 credits | ❌ 周额度+超额计费 |
| 可审计 | ✅ 事件溯源+trace 回放+全开源 | ❌ | 🟡 | 🟡 | ❌ | 🟡 | ❌ audit log "coming soon" |
| 密钥安全 | ✅ secret-guard 出站脱敏+keyring+注入扫描 | 🟡 | ❓ | ❌ Skills Hub 零审核 | ❓ | ❓ | ❌ 整仓上传事故 |
| 自动化触发器 | ✅ cron+API endpoint+GitHub+IM | 🟡 cron+API+GitHub | 🟡 cron | ✅ cron+NL | 🟡 | ✅ 定时规则 | ✅ routines |
| 桌面技术栈 | Tauri2+Rust | Electron | Electron+Rust | Electron+Python | Electron | 未公开 | — |

### 2.3 相同点（行业基线，宣传时不必再当卖点）

图像理解、MCP、子 agent、定时任务、git/worktree 隔离、权限确认、桌面 GUI——2026 年 9 月这些已是**入场券**而非差异化。文案里只做「也有」，不占篇幅。

### 2.4 差异点（只有 Shannon 有的组合拳）

1. **唯一「全开源 + 任意模型 + 本地优先 + 四表面同核」四项全中**的 agent 产品（§2.2 矩阵逐列可验证）。
2. **成本可解释**：BYOK 按量付费 + session 预算上限 + 上下文六类拆解 + 缓存命中率可见——竞品评论区第一大痛点（Claude 限额焦虑、Codex credits 混乱、Hermes "cost projection is insane"、ZCode/WorkBuddy 涨价争议）的结构性解药。
3. **事件溯源 + trace 回放/diff**：agent 每一步 append-only 留痕、可重放、可审计——对标 Grok Bot 官方自认「审批拦不住已完成操作、audit log coming soon」。
4. **secret-guard 引擎级脱敏**：出站消息 secret 变换且**字节稳定保 prompt cache**——「安全不牺牲成本」的独家工程细节。
5. **同核跨表面接力**：终端→桌面→手机→IM，同一会话同一状态；竞品要么无 CLI（ZCode/WorkBuddy），要么桌面闭源绑定订阅（Claude/Codex）。
6. **对中国用户的独特组合**：飞书/钉钉入站 + GLM/DeepSeek 一等公民 + 10 语言——海外开源竞品（Hermes/Codex）与国产闭源竞品（WorkBuddy/ZCode）之间的空档。

### 2.5 诚实清单（劣势与回避项——广告总监必须先知道自己不能吹什么）

| 短板 | 事实 | 传播策略 |
|---|---|---|
| 社区规模小 | 无 star 优势，Hermes 241k stars、Codex CLI ~114k | 不打「社区最大」；打「工程密度」（测试数/LOC/评测纪律） |
| 生成类多模态空白 | 无图像/视频生成、无云 TTS（multimodal research G1-G3） | 不提「全能」；被问到走 MCP/BYOK 路线图 |
| 无 IDE 扩展 | VS Code 扩展已评审永久放弃 | **所有渠道素材清除 VS Code 表述**；定位「终端+桌面双形态」 |
| 模型目录有过期项 | grok-4.1-fast 已退役仍在注册表；README 模型表停在 GPT-4o 时代 | 发布前修复（改进方案 P0-1） |
| 桌面成熟度 | 9 月刚完成 lanes A-M 大批合并 | 用「节奏」叙事（90 天交付清单）而非「成熟稳定」叙事 |
| eval 绝对分值非顶尖 | SWE-bench 33/50 是内部 harness | 只引用带 n/日期/锚点的数字，主打「方法论透明」而非「跑分第一」 |

---

## 3. 卖点与价值分析（高级产品经理视角）

### 3.1 核心洞察：行业的第一痛点不是「不够聪明」，是「失控」

竞品评论区抱怨排序（多份调研交叉验证）：**限额/成本 > 行为黑盒 > 数据安全 > 能力缺口**。

- Claude：5 小时滚动限额焦虑，限额黑盒；
- Codex：转 token 计价后单任务成本被报涨 10-20x，credits 体系混乱；
- Hermes：token 消耗失控热帖（"cost projection is insane"）、Electron+Python OOM；
- Grok Bot：3 小时烧掉 52% 周额度、无支出上限；
- WorkBuddy/ZCode：企业版涨价 +154%、credits 涨价 ≥30% 引众怒。

**结论**：2026 年用户换产品的第一动机是「重新获得控制权」——成本可控、行为可审计、数据不出门、模型可替换。这四个「可控」恰好全部是 Shannon 的架构属性，不是补丁。**这就是定位的裂缝所在：别人卖「更强」，我们卖「在你掌控之中」。**

### 3.2 价值主张分层（JTBD：谁在什么情况下雇佣 Shannon）

| 人群 | 被雇佣来做的任务 | 目前用什么 | 换用 Shannon 的钩子 | 一句话价值 |
|---|---|---|---|---|
| **开发者**（留存盘） | 写码、修 CI、并行开 PR | Claude Code / Codex CLI | 额度焦虑→BYOK 按量；黑盒→trace 回放；单线程→agent 舰队 + best-of-N | 「同一个活，换个模型跑、留着证据跑、六个 agent 并着跑」 |
| **独立开发者 / 小团队**（转化盘） | 用 AI 降本增效但预算敏感 | 订阅制工具 + 手动编排 | 成本对比计算器；本地模型零成本档；IM 派活 | 「为用过的 token 付费，不为没用完的额度付费」 |
| **知识工作者**（增长盘，Desktop Simple 模式） | 调研、写作、批量文档、定时摘要 | ChatGPT 桌面 / WorkBuddy | IM/手机派活；routine 日历；本地数据不出门 | 「在聊天软件里派活给家里/公司的电脑」 |
| **隐私敏感者 / 企业**（信任盘） | 合规、审计、代码不出域 | 自部署方案 | 全开源可审计 + secret-guard + Landlock/Seatbelt + 事件溯源 | 「agent 干的每一步都可回放，密钥永远不出机器」 |
| **模型厂商 / 生态**（联盟盘） | 给自家模型找中立的 harness | 各自为战 | provider 中立 + 评测管线 + Claude Code 生态兼容 | 「你的模型 + 我们的引擎，立即拥有全形态 agent」 |

### 3.3 卖点排序（先打什么，后打什么）

1. **P0 主打：掌控权三件套**——任意模型（BYOK）/ 成本透明（预算+拆解）/ 行为可审计（trace 回放）。打全行业共同痛点，每一条都有 UI 截图证据（9 月已交付）。
2. **P0 主打：本地优先 + 密钥安全**——数据不出门、secret-guard、沙箱。借 Grok Bot「共享 VM/凭据池、bot 非安全边界写进官方条款」与 Grok Build「整仓上传密钥」的舆论窗口，**只讲自己做到什么，不点名攻击**。
3. **P1 支线：agent 舰队与并行编排**——多 agent + worktree + `/batch` best-of-N。视觉冲击强，适合 GIF/视频传播。
4. **P1 支线：IM/移动派活**——「飞书群 @bot → 家里电脑干活」。中国市场的刚需形态（WorkBuddy 2000 万 MAU 验证过需求），Shannon 是唯一开源选项。
5. **P2 慢火：工程与评测纪律**——11,752 测试、eval 三元组纪律、semver 门禁。面向 HN/rust 圈层的信任资产。

### 3.4 定位声明（Positioning Statement）

> 对于**被订阅额度和云端黑盒困住的 AI 重度用户**，**Shannon** 是**开源的 AI agent 工作台**，它让你**用任意模型、在本机、以可审计的方式派遣 agent 完成任何任务**。不同于 Claude Code / Codex / Grok Bot 的「订阅 + 云端 + 单一模型」，Shannon 的引擎运行在你自己的机器上：模型随便换、成本看得见、每一步可回放、密钥不出门。Apache-2.0，Rust 打造，11,752 个自动化测试背书。

---

## 4. 潜在爆点（高级广告总监视角）

### 4.0 爆点评估框架

每个爆点按 **话题性（有没有人想吵）× 可信度（有没有证据）× 可演示性（能不能 30 秒看到）** 打分（满分 5×5×5）。

### 4.1 爆点一：「额度焦虑税」成本计算器 —— 5×5×4

- **洞察**：全行业第一大抱怨是限额/成本（§3.1），但没有一家竞品敢把「同样任务花多少钱」摆上台面。
- **引爆方式**：官网上线互动计算器「算算你在给额度焦虑交多少税」：输入每月任务量 → 对比 Claude Max $200 / Codex Pro / SuperGrok $300 vs Shannon BYOK（DeepSeek/GLM/本地 Ollama 三档）。数字全部带来源脚注。
- **配套话题**：**「为用过的 token 付费，不为没用完的额度付费。」**（EN: *Pay for the tokens you use, not the quota you fear.*）
- **风险控制**：数字必须可复现（同 prompt、同日期、带锚点），避免「10-20x」这类无来源断言升级（见 §8 红线）。

### 4.2 爆点二：「凭什么 agent 要住在别人的电脑上？」—— 5×4×4

- **时机**：Grok Bot（云 VM、共享凭据池、周额度）、ChatGPT Work（临时 VM）、Manus 全在推云端 agent；「云 VS 本机」正是舆论吵架点。
- **引爆方式**：一篇观点文 + 演示视频：同一个任务分别在云 agent（额度、黑盒、数据上云）和 Shannon（本机、回放、密钥不出门）上执行。标题即问句，天然引战引流。
- **配套话题**：**「别把 agent 租回来，把它装在自己电脑上。」**（EN: *Don't rent your agent. Own it.*）
- **优势**：不点名任何竞品，冲突由受众自行脑补；与 secret-guard、本地 whisper、Landlock 沙箱形成证据链。

### 4.3 爆点三：「agent 舰队」可视化 GIF —— 4×5×5

- **素材现成**：`/batch` best-of-N 三 worktree 并行 → 桌面并排 diff → 择优合并；多面板工作区 + 集成终端。30 秒 GIF 完整呈现。
- **引爆方式**：Twitter/X + Reddit r/programming + B 站/抖音技术区；标题「看我电脑上 6 个 agent 同时改 6 个 PR，我只需要点『采纳』」。
- **配套话题**：**「一支跑在你电脑上的 agent 舰队。」**（EN: *Your agent fleet, on your machine.*）
- **可信度**：全部真实功能，无摆拍空间；工程指标（测试数）做角标。

### 4.4 爆点四：「在飞书群里 @ 一句，家里的电脑开始干活」—— 4×4×5

- **洞察**：IM 派活是被 WorkBuddy（微信直连 + 2000 万 MAU）验证过的中国市场刚需；海外开源竞品无一覆盖飞书/钉钉。
- **引爆方式**：竖屏短视频（复用竞品调研 §4 的 Journey 叙事）：通勤路上飞书 @bot 派活 → 手机收到审批卡 → 批准 → 回家看到 Triage 收件箱里的结果与回放。
- **配套话题**：**「AI 不住在云端，住在你自己的电脑里——但它听群消息。」**
- **注意**：明确「凭据只存本地 keyring、消息经注入扫描」，把 WorkBuddy 的云端隐私疑虑转化为自己的加分项。

### 4.5 爆点五：「AI 行车记录仪」—— 4×5×4

- **洞察**：事件溯源 + `shannon trace replay/diff` 是独家能力，但「事件溯源」是工程师语言，无传播力。**「行车记录仪」是全民语言**：出了事，回放。
- **引爆方式**：对比内容：「云 agent 出了错，你只有一张截图；Shannon 出了错，你有完整行车记录仪」——演示 `trace show → replay → diff` 三步定位一次 agent 误操作并回滚。
- **配套话题**：**「Agent 黑盒时代的第一台行车记录仪。」**（EN: *A dashcam for your AI agents.*）
- **延展**：合规/企业场景软文（审计、事故复盘、新人 onboarding 时看 agent 干活的历史）。

### 4.6 爆点六：「你的 API key 值多少钱？」secret-guard 安全叙事 —— 3×5×3

- **时机**：Grok Build 整仓上传密钥事故、Grok Bot 共享凭据池写进官方条款——安全叙事有公共舆论水位。
- **引爆方式**：技术深文 + 发布注记：为什么把 secret 脱敏做成**引擎级内容变换中间件**（shannon-plugin-api 四不变量：字节稳定保 prompt cache、单向流、幂等、显式失败语义）——「安全不该以烧掉你的缓存费用为代价」。
- **配套话题**：**「密钥不出机器，安全不烧缓存。」**
- **克制品**：不点名任何事故厂商，用「行业近期事件」指代；技术向渠道为主（HN / r/rust / 安全公众号）。

### 4.7 爆点排序与档期建议

| 档期 | 爆点 | 理由 |
|---|---|---|
| T0（发布日） | 爆点三（舰队 GIF）+ 品牌主线 | 发布需要视觉锤；舰队是最直观的「产品力证明」 |
| T+1 周 | 爆点二（云 VS 本机）+ 爆点一（成本计算器） | 争议话题需要发布势能垫底；计算器给吵架的人递数据 |
| T+2 周 | 爆点四（IM 派活） | 中文市场专属弹药，错峰投放 |
| T+3 周 | 爆点五（行车记录仪）+ 爆点六（安全深文） | 深度内容承接长尾流量与信任 |

---

## 5. 核心信息屋（Message House）

```
                    ┌──────────────────────────────────────────────┐
 屋顶（品牌叙事）    │  Agent 时代的真正问题不是模型不够强，           │
                    │  而是它跑在谁的电脑里、花谁的钱、听谁的。         │
                    │  Shannon 把控制权交还给你。                     │
                    └──────────────────────────────────────────────┘
       ┌────────────────────┬────────────────────┬────────────────────┐
 支柱  │ 支柱一：模型自由     │ 支柱二：全形态同核   │ 支柱三：一切可控     │
       │ Any model          │ Every surface      │ Total control      │
       ├────────────────────┼────────────────────┼────────────────────┤
 分论点 │ BYOK：Anthropic/    │ 同一 Rust 引擎：     │ 成本：预算上限+六类   │
       │ OpenAI/DeepSeek/   │ 终端 TUI·headless·  │ token 拆解+缓存命中  │
       │ GLM/Ollama/任意     │ serve·桌面 GUI      │ 可见                │
       │ 兼容端点            │                    │                    │
       │ 上游涨价/退役不殃及  │ 终端开始，桌面继续，  │ 行为：事件溯源+trace │
       │ 你（BYOK 反脆弱）    │ 手机审批，IM 派活    │ 回放/diff（行车记录仪)│
       │                    │                    │                    │
       │ Claude Code 生态    │ 90 天交付节奏：      │ 数据：本地优先+      │
       │ 兼容，5 分钟迁移     │ 1,481 commits      │ secret-guard+沙箱+  │
       │                    │                    │ keyring+注入扫描     │
 证据  │ 多 provider 表格、   │ 四表面截图、跨表面    │ 成本面板截图、trace  │
       │ 迁移向导、模型目录   │ 接力 demo、ADR-0011  │ 回放 demo、安全白皮  │
       └────────────────────┴────────────────────┴────────────────────┘
 地基（信任状）: Apache-2.0 全开源 · 11,752 自动化测试 · clippy 零警告 ·
                SWE-bench/TB 官方判分管线（引用带 n/日期/锚点） · 10 语言
```

---

## 6. 口号与文案库（中英双语）

> 用法约定：**主线口号**用于官网 Hero / README 首屏 / 发布标题；**场景口号**用于各爆点投放；**功能一句话**用于 Feature Grid / 社交卡片。所有 EN 主口号已检查与现有素材（desktop README "Your AI Workspace. Chat with any model. Deploy agents on any task. Automate anything."）的延续性——是升级而非断裂。

### 6.1 品牌主线

| # | EN | CN | 评注 |
|---|---|---|---|
| **主推** | **Your AI workspace. Any model. Your machine.** | **你的 AI 工作台：模型随便换，任务随便派，数据不出门。** | 三段式，覆盖三大支柱；CN 版押「换/派/门」口语节奏 |
| 备选 A | The open AI workspace. Any model, any task, your machine. | 开源 AI 工作台：任意模型，任意任务，就在你的电脑上。 | 更描述性，适合 SEO/HN 标题 |
| 备选 B | Don't rent your agent. Own it. | 别租 agent，把它装回自己电脑。 | 攻击型，适合爆点二投放，不作主线 |
| 备选 C | One engine. Every surface. Any model. | 一个引擎，四种形态，任意模型。 | 架构叙事，适合开发者渠道 |
| 备选 D | Agents on your terms. | Agent，按你的规矩来。 | 短且 attitude 足，适合周边/T-shirt |

### 6.2 产品线文案

**终端（TUI/CLI）**
- EN: *The terminal-native coding agent — any model, multi-agent teams, every step replayable.*
- CN: 「为终端而生的 AI 编程 agent：任意模型、多 agent 编排、每一步可回放。」

**桌面（Desktop）**
- EN: *Your AI workspace — chat with any model, deploy agents on any task, automate anything. No terminal required (unless you want one).*
- CN: 「你的 AI 工作台：和任意模型聊天，把任务派给 agent，把重复的事自动化——不需要会写代码。」
- （延续既有 "Chat with any model. Deploy agents on any task. Automate anything." 的动词三连，保留认知资产）

### 6.3 功能一句话卖点（Feature Grid 用）

| 功能 | EN one-liner | CN 一句话 |
|---|---|---|
| 多 provider | Any LLM, one config. Upstream price hikes are someone else's problem. | 任意模型一个配置，上游涨价与你无关 |
| 自主任务 | Give it a goal, not a prompt. Budget-capped, self-resuming, anti-spin. | 派目标，不派 prompt：带预算上限、自动续跑、防原地打转 |
| agent 舰队 | An OS-process agent team on worktrees. Best-of-N, merge the winner. | 进程级 agent 舰队跑在 worktree 上，多方案并行、择优合并 |
| 成本透明 | Six-way context breakdown, cache hit rate, session budget caps. | 上下文六类拆解、缓存命中率、会话预算上限——钱花哪了一目了然 |
| trace 回放 | An append-only event log for everything your agent did. Replay it like a dashcam. | agent 干的每一步都留痕，像行车记录仪一样回放 |
| 隐私 | Keys in your OS keyring. Secrets redacted before they leave. Telemetry: none. | 密钥进系统钥匙串，出站先脱敏，遥测：没有 |
| computer use / 浏览器 | It sees the screen, drives the browser — locally. | 看得见屏幕、开得动浏览器——全程本机 |
| IM / 移动派活 | @ your bot from Feishu at 9am; approve from your phone on the subway. | 早上在飞书 @ 一句，地铁上用手机批准 |
| 桌面 | Tauri, not Electron. Your RAM will notice. | Tauri 而非 Electron，你的内存条会感谢你 |
| 工程质量 | 11,752 tests, zero clippy warnings, semver-gated API. | 11,752 个测试、零 clippy 告警、semver 门禁 |

### 6.4 发布文案骨架

**Show HN 标题候选**
1. *Show HN: Shannon – open-source AI workspace (TUI + desktop), any LLM, Rust*
2. *Show HN: I open-sourced my AI agent desktop – local-first, BYOK, event-sourced sessions*
3. *Show HN: An open-source Claude Code alternative with a Tauri desktop, 11.7k tests*

**首评（TLDL 模板）**：三段——① 90 天发生了什么（§1.2 表格精简版）；② 和 X/Y/Z 的区别（只讲差异事实：任意模型/同核四表面/trace 回放/secret-guard）；③ 诚实清单（无 IDE 扩展、生成类多模态走 MCP 路线图、eval 数字引用纪律）。**HN 受众奖励诚实，惩罚营销腔。**

**Product Hunt 一句话**：*Shannon — the open-source AI workspace. Chat with any model, deploy agents on any task, on your own machine.*

**V2EX / 即刻 / 少数派标题候选**
1. 「开源了一个本地优先的 AI 工作台：飞书派活、手机审批、agent 舰队，Rust 写的」
2. 「受够了额度焦虑：我把 AI 编程 agent 搬回了自己电脑（开源）」
3. 「从 Claude Code 5 分钟搬到 Shannon：配置、MCP、skills 全量迁移」

**Twitter/X 发布推文**
> 90 days. 1,481 commits. Shannon went from a terminal AI tool to a full open-source AI workspace:
> – TUI + desktop + headless, one Rust engine
> – any model (Anthropic/OpenAI/DeepSeek/GLM/Ollama…)
> – goal-based agents w/ budget caps, best-of-N fleets, IM dispatch, trace replay
> Your AI workspace. Any model. Your machine. 🧵

### 6.5 危险话术（不要说的话）

| 禁语/风险表述 | 原因 | 替代表述 |
|---|---|---|
| 「动态计费 header 让成本膨胀 10-20x」不加来源 | 无公开可引用来源，法律与口碑风险 | 「订阅制 + 黑盒限额 vs 按量 + 全程可见」（不引具体倍数） |
| 「SWE-bench XX% 超越 Y」 | 内部 harness 数字必须带 n/日期/锚点；绝对分非顶尖 | 「公开评测管线与全部判分记录，数字自己看」（附 repo 链接） |
| 「替代 Claude Code」 | clean-room 声明与品牌风险 | 「Claude Code 生态兼容的开源替代选项」 |
| 「企业级就绪 / production-ready」 | 桌面 9 月刚大批合并、billing 未商业化 | 「工程纪律严明的早期项目，节奏见 changelog」 |
| VS Code 扩展相关任何表述 | 已评审永久放弃 | 「终端 + 桌面双形态」 |
| 「最快/最强/第一」类绝对化用语 | 广告法与社区反噬 | 用可验证数字（测试数、渠道数、语言数） |

---

## 7. 传播节奏与渠道

### 7.1 节奏（以版本发布为锚，建议 v0.12 = "Workspace" 发布）

| 阶段 | 动作 | 渠道 | 素材 |
|---|---|---|---|
| **T-1 周（清障）** | 事实清理（README/官网过期数字、VS Code 表述）+ 素材生产（双产品截图×6、舰队 GIF×2、trace 回放短视频） | — | 配套改进方案 Wave 1/2 |
| **T0（发布）** | 版本发布 + 品牌主线亮相 + 舰队 GIF | GitHub Release / HN Show / X / r/rust / V2EX | §6.4 文案骨架 |
| **T+1 周** | 爆点二观点文 + 成本计算器上线 | HN / X 长推 / 少数派 | 计算器 + 观点文 |
| **T+2 周** | IM 派活短视频（飞书/钉钉场景）+ Product Hunt（Desktop 消费向角度） | PH / B 站 / 抖音技术区 / 即刻 | 竖屏视频×2 |
| **T+3 周** | 行车记录仪 demo + secret-guard 技术深文 | HN / r/rust / 安全公众号 / 掘金 | 技术长文×2 |
| **持续** | eval 系列月报（带三元组）、社区 benchmark 征集、对比页季度更新 | 博客 / Newsletter | 数据内容 |

### 7.2 渠道优先级

1. **开发者国际线**：HN（Show HN + 诚实首评）→ r/rust、r/LocalLLaMA（本地模型角度）→ X 技术圈。
2. **中文开发者线**：V2EX、掘金、B 站技术区、微信公众号（Rust 领域 KOL 撬动）。
3. **效率/知识工作者线**：少数派、即刻、Product Hunt（Desktop Simple 模式是这条线的产品凭证）。
4. **生态联盟线**：GLM/DeepSeek/Ollama 社区互推（「你的模型 + 我们的引擎」）；模型注册表及时收录新旗舰（grok-4.6 等）本身就是生态信号。

### 7.3 度量

- 北极星：GitHub star 增速 + 官网→安装转化（需在 website 加埋点，匿名 opt-in 与产品遥测口径一致）。
- 过程指标：HN 首页停留、PH 当日排名、计算器使用量、视频完播、Discord/issue 社区增长。
- 品牌指标：搜索 "shannon agent/workspace" 趋势；对比页（vs Claude Code / vs Hermes）落地占比。

---

## 8. 风险与合规红线

1. **数字纪律**：一切评测/成本数字带 n/日期/锚点；竞品价格引用标「截至日期 + 来源」；无来源倍数一律不写（含存量文案清理，见配套方案 §3）。
2. **商标与 clean-room**：不使用竞品 logo 做对比图（表格文字可）；保留并前置 clean-room 独立实现声明（README 已有）。
3. **安全叙事克制**：讲自己做到什么，不点名攻击事故厂商；「本地优先」不夸大为「绝对安全」（沙箱 experimental 标注保留）。
4. **开源纯度**：宣传「开源」时不得暗示含托管服务免费——BYOK 自付模型费用要在定价文案里说清，避免「隐性成本」反噬（这正是我们攻击别人的点）。
5. **社区规模**：在 star/用户数起来之前，全部信任状用工程指标替代，不编造「trusted by」。

---

## 9. 决策点（待拍板）

| # | 决策点 | 建议 | 备选 |
|---|---|---|---|
| 1 | 品牌主线口号 | 主推 §6.1「Your AI workspace. Any model. Your machine.」 | 备选 A/B/C/D |
| 2 | 产品名口径 | README/官网统一以「Shannon」为产品名，TUI/桌面为 surface 名（对齐 ADR-0011），逐步淡化 "Shannon Code" | 维持 "Shannon Code"（改动成本小，但与桌面叙事冲突） |
| 3 | 爆点档期是否锚定 v0.12 发布 | 是（建议 2-3 周内出 v0.12，宣传与版本绑定） | 先宣传后发版 |
| 4 | 成本计算器是否上线 | 上线（爆点一的载体），数字全部带来源 | 只做静态对比表 |
| 5 | 中文市场优先级 | 与国际线并行（飞书/钉钉 + GLM/DeepSeek 是独家组合） | 国际线先行，中文 T+1 月 |
| 6 | 是否点名竞品做对比页 | 做事实型对比页（逐项可验证，标来源与日期），不做拉踩文案 | 只做「闭源工具」泛指（现状，信息量低） |
| 7 | "10-20x" 存量表述 | 删除或降级为有来源引用 | 保留原状（有口碑风险） |
