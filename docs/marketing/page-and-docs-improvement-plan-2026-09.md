# Shannon 仓库页 · 产品主页 · 用户文档 改进方案（2026-09，待审核）

- **日期**: 2026-09-10 ｜ **定位**: 宣传方案的落地执行案（只做「门面」，不改产品代码）
- **依据**: dev @ c3bc5647 实测盘点 · [核心宣传方案](./core-marketing-plan-2026-09.md)（口号与信息屋的落点） · 现有约定：`docs/metrics.md` 为指标单一事实源、`scripts/gen-metrics.sh` 注入 README
- **范围**: ① GitHub 仓库页（README 双语 + 仓库元信息）② 产品主页 `website/`（Astro 落地页）③ 用户文档 `docs-mdbook/`（mdBook）与 `desktop/docs/user/` 的关系
- **状态**: ✅ 已评审通过（2026-09-10），按 §10 决策表执行中；**核心叙事锚定「开源可控 + 密钥安全」双核心**（见 [核心宣传方案 §9](./core-marketing-plan-2026-09.md)）；实施记录见 §11

---

## 0. TL;DR

### 0.1 三个触点的诊断一句话

| 触点 | 现状 | 核心问题 |
|---|---|---|
| **GitHub 仓库页** | 定位停在「AI-assisted **coding** tool」，只有终端产品 | 桌面产品完全缺席；对照表只打「闭源工具」稻草人；模型表停在 GPT-4o 时代；VS Code 扩展仍在 README（已决议放弃） |
| **产品主页 website/** | 文案为早期版本而写 | Hero 停在 "AI coding, without limits"；终端演示写 **v0.1.0**；Feature 列 **7,889 tests / 12 crates**（实测 11,752 / 20）；**第 07 项仍在宣传 VS Code 扩展**；无桌面板块、无截图、无新口号 |
| **用户文档 docs-mdbook/** | 开发者向（Crate Reference 是一级目录） | 桌面、自动化（goal/routine）、IM/移动派活、语音、computer use/浏览器、安全（secret-guard/redaction/沙箱）、trace 回放等已交付能力**全部无文档**；`book.toml` 仓库地址指向 `github.com/ericdong/shannon-code`（与实际仓库不符）；与 `desktop/docs/user/` 双轨并行无整合策略 |

### 0.2 决策点（已全部评审通过，2026-09-10，详见 §10）

1. 产品名口径：统一「Shannon」，淡化 "Shannon Code"；2. README/官网对比表点名竞品（事实型+标来源）；3. mdBook 为唯一文档门面，desktop 内嵌帮助 include 收编；4. Wave 1 立即执行；5. "10-20x" 删除；6. 素材按 §6 清单生产；7. 发布锚定 v0.12；8. **核心宣传点 =「开源可控 + 密钥安全」**。

> **双核心落地原则**（本版新增）：三处门面的首屏叙事、章节顺序、Feature 排序一律以「开源可控」「密钥安全」为第一、第二卖点；「任意模型 × 全形态」作为支撑证据后置。文案必须承诺与证据同屏（测试徽章 / trace / secret-guard 文档链接）。

---

## 1. 逐触点诊断（问题清单，含证据）

### 1.1 GitHub 仓库页（README.md / README.zh-CN.md / 仓库元信息）

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| R1 | 标题与 Hero 定位过窄：「Shannon Code」+「AI-assisted coding tool」 | `README.md:1-7` | 桌面产品与知识工作者受众在门面上直接不可见 |
| R2 | VS Code 扩展仍列于 Features（editors 小节） | `README.md:156-164` | 与 2026-09-05 评审决议（永久放弃）矛盾，损害可信度 |
| R3 | Provider 模型表过期（GPT-4o / GPT-4 / GPT-3.5） | `README.md:56-62` | 与模型注册表现状脱节（另有已退役型号待清理，见 grok research §9.1） |
| R4 | 差异化对照表只对照「Typical closed-source tools」 | `README.md:29-37` | 信息量低；对 Hermes/Codex CLI/Grok Build 等开源同类无解释力 |
| R5 | 无桌面产品任何章节/截图/链接 | 全文检索无 "desktop" 功能章节 | 第二产品零曝光 |
| R6 | 无视觉素材（Hero 图/GIF） | 全文无图片 | 开源门面转化率低（对比：Hermes/Codex 均有首屏 GIF） |
| R7 | 仓库元信息（description/topics/social preview/release 节奏）未与「双产品」叙事对齐 | GitHub 仓库设置 | 搜索与社交分享卡片过期 |
| R8 ✅ | 指标数字已脚本化（crates-20 徽章、metrics 注入） | `README.md:14-16,24` | 良性机制，本方案**沿用并扩展到 website** |

### 1.2 产品主页（website/）

| # | 问题 | 证据（`website/src/i18n/index.ts`） | 影响 |
|---|---|---|---|
| W1 | Hero 主张停在编码工具：「AI coding, without limits」 | `:14-15, 81-82` | 与「AI 工作台」定位脱节；"without limits" 语义空泛 |
| W2 | 终端演示写 **v0.1.0** | `:23, 92` | 版本号暴露项目幼稚期（实际 v0.11.0-wip） |
| W3 | Feature 07 = VS Code Extension | `:55, 121` | 宣传已放弃的产品 |
| W4 | 统计条 7,889 tests / 12 crates | `:60-65, 126-131` | 事实错误（11,752 / 20），且违反自家「指标单一事实源」机制 |
| W5 | 对比表风险表述「Dynamic billing headers inflate costs 10-20x」 | `:73, ~139` | 无公开来源；广告与口碑双重风险（宣传方案 §8.1） |
| W6 | 无桌面板块/截图/下载入口；DownloadButtons 仅 CLI 口径 | 组件结构 `Hero/Terminal/FeatureGrid/ComparisonTable/DownloadButtons/CTABanner` | 主页不知道自己有两个产品 |
| W7 | 无信任状区（测试数/评测纪律/安全特性）与口号体系 | — | 信息屋三支柱无处安放 |
| W8 | SEO/OG 元数据未更新（title/description/社交卡） | `index.astro` / `LandingLayout.astro` | 分享卡片与搜索摘要过期 |

### 1.3 用户文档（docs-mdbook/ + desktop/docs/user/）

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| D1 | 文档站定位与命名过时：「Shannon Code Documentation」 | `docs-mdbook/book.toml:2` | 与双产品现实不符 |
| D2 | `book.toml` 仓库地址错误：`github.com/ericdong/shannon-code` | `docs-mdbook/book.toml:15-16` | 「Edit on GitHub」链到错误仓库；文档贡献入口断裂 |
| D3 | 内容结构以开发者为主（Crate Reference 一级目录），用户旅程缺失 | `docs-mdbook/src/SUMMARY.md` | 非开发者（Desktop Simple 模式受众）无文档可读 |
| D4 | 9 月交付能力零文档：goal/loop/routines、IM 五渠道、移动派发、trace、secret-guard/RedactionPolicy、computer use、浏览器、本地语音、成本面板、迁移向导 | SUMMARY 无对应页 | 功能做完了用户不知道（对开源项目=没做） |
| D5 | `desktop/docs/user/`（面向非技术用户、质量高）与 mdBook 双轨，无互相引用 | `desktop/docs/user/README.md` 等 | 内容漂移风险；SEO 分散 |
| D6 | 已有 `docs/integrations/` 8 篇（IM/语音/Slack/Jira/Notion/GitHub 触发器等）未进入文档站导航 | `docs-mdbook/src/SUMMARY.md` | 好内容埋在 repo 里 |

---

## 2. 改造原则

1. **先清障，再装修**：过期/矛盾事实（VS Code、7,889、v0.1.0、错误仓库地址）P0 清零，然后才上新叙事——宣传期任何被抓到的过期事实都会反噬「可审计」人设。
2. **事实单一源扩展到全门面**：README 已有 `metrics:start/end` 机制，扩展出 `website/src/data/facts.json`（构建时从 `docs/metrics.md` 生成，CI 校验漂移），主页统计条、对比表数字全部走生成，不手写。
3. **双受众双入口**：门面与文档站均分「User track（桌面优先、非程序员可读）」与「Developer track（终端优先）」，与 Desktop Simple/Advanced 双模式心智一致。
4. **口号只换一层皮**：信息屋三支柱（模型自由/全形态同核/一切可控）成为 README、主页、文档站共同的章节骨架；各触点复用宣传方案 §6 文案库，不各自发明。
5. **诚实即卖点**：对照表给来源与「截至日期」；不支持的能力明写「走 MCP/路线图」；评测数字带 n/日期/锚点——把「诚实」做成可感知的品牌特征。

---

## 3. GitHub 仓库页方案

### 3.1 README.md 结构重构（新骨架）

```
# Shannon
<div align=center>
  主口号（EN）: Your AI workspace. Any model. Your machine.
  副标: The open-source AI agent workspace — terminal + desktop, one Rust engine,
        any LLM provider. Apache-2.0.
  徽章行: License | Release | Rust | Tests(生成) | Crates(生成) | Docs | Discord*
  [截图区]: 桌面全貌 1 张 + 终端 1 张（或 1 张合成横幅 GIF）
  English | 中文文档 | Documentation
</div>

## What is Shannon?            ← 由 §1.1「一个引擎四个表面」改写，双产品各一段+跳转
## Two ways to use it          ← TUI 卡片 | Desktop 卡片（各: 3 卖点 + 截图 + 安装/下载按钮）
## Highlights (6 条功能一句话)  ← 宣传方案 §6.3 表格直译为列表（多 provider/自主任务/舰队/
                                  成本透明/trace 回放/隐私安全）
## Why open source matters     ← 现差异化表保留，但对照列改为「开源同类 vs 云端订阅制」两列
## Quick start (CLI + Desktop) ← install.sh 一行 + 桌面下载链接 + 3 条最短路径示例
## What's new (90 天)           ← §1.2 里程碑表精简 6 行，链 CHANGELOG
## Documentation / Community / Roadmap / License & clean-room 声明
```

- `README.zh-CN.md` 完全平行重构（非机翻，中文文案直接取宣传方案 §6 CN 列）。
- R2/R3 修复：删除 editors/VS Code 小节；provider 表改为「从模型注册表生成」（新增生成脚本或 CI 检查注册表与 README 同步），过期型号随注册表清理一并修复。
- R7：仓库 description 改为 `Open-source AI agent workspace — terminal + desktop in one Rust engine. Any LLM. Apache-2.0.`；topics 补 `ai-agent` `tauri` `ratatui` `llm` `mcp` `byok`；social preview 卡用新横幅图。

### 3.2 对比表（R4 修复，决策点 #2）

以「事实型 + 标来源 + 标日期」为前提，改为三列组：**Shannon ｜ 云端订阅制代表（Claude Code/Codex）｜ 开源同类（Hermes/Codex CLI/Grok Build）**，行保留现有六行并补两行：`执行位置（本机/云）`、`许可`。每格内容为可验证事实；表底注明「截至 2026-09，来源链接见 docs/competitive-research-2026-09.md」。不出现拉踩形容词。

### 3.3 验收标准

- [ ] README 中不再出现 VS Code 扩展、"7,889"、GPT-3.5 等过期事实（CI 加禁词检查，见 §7）。
- [ ] 首屏 30 秒内（截图 + 两张产品卡）能看懂「两个产品、一个引擎、任意模型」。
- [ ] 中英 README 结构与数字完全对齐；数字均来自生成标记。

---

## 4. 产品主页（website/）方案

### 4.1 页面新结构（组件级）

```
Navbar（不变，+ "Download" 锚点）
Hero
  badge: Open-source · Rust · Apache-2.0 · Claude Code compatible
  title: Your AI workspace. [Any model.] [Your machine.]   ← 主口号，斜体强调换_model_
  subtitle: One Rust engine, four surfaces — terminal, headless, server, and desktop.
            Chat with any model, deploy agents on any task, automate anything.
  costHint 保留但改口径: "DeepSeek ~$0.14/M · Claude ~$15/M — you choose" （去掉对比暗示，改「你选」）
  CTA: Get started / Download Desktop / Star on GitHub
StatsBar（新组件）: 11,752+ tests · 20 crates · 4 surfaces · 5 IM channels · 10 languages （facts.json 生成）
Terminal（改造）: v0.11 演示脚本换为「goal 任务 + 多 provider 切换」叙事（见 4.2）
TwoProducts（新组件）: 左右卡片 TUI vs Desktop，各 3 卖点 + 截图 + 下载按钮
FeatureGrid（重写 8 项）: §6.3 功能一句话表 8 条（替换 VS Code 项 → 桌面/IM 派活）
TrustSection（新组件）: 三支柱（Any model / Every surface / Total control）各配 1 证据截图
ComparisonTable（重写）: 行=§3.2 同款八行；数字与 README 同源 facts.json；表底来源注
DownloadButtons（扩展）: CLI 一行安装 + macOS/Win/Linux 桌面包 +包体积标注（Tauri 卖点）
CTABanner / Footer（口号收尾；Footer 仓库链接修正）
```

### 4.2 关键文案替换表（i18n 中英对照，直接可实施）

| 位置 | 现文案 | 新文案（EN / CN） |
|---|---|---|
| hero.title | AI coding, without limits / AI 编程，不受限 | Your AI workspace. Any model. Your machine. / 你的 AI 工作台：模型随便换，任务随便派，数据不出门 |
| hero.subtitle | Claude Code compatible. Works with DeepSeek… | One Rust engine, four surfaces — terminal to desktop. Any LLM provider, local-first, fully open source. / 一个 Rust 引擎，四种形态——从终端到桌面。任意大模型、本地优先、完全开源。 |
| terminal v0.1.0 行 | Shannon Code v0.1.0 · Rust · Multi-provider | shannon v0.11 · Rust · any-model engine |
| terminal 剧情 | 修 bug 单线程演示 | 换为：`/goal` 派目标 → agent 自续跑 → `$ shannon trace replay` 回放 →「desktop」接力提示（展示两大独有卖点） |
| features[07] | VS Code Extension | IM & Mobile dispatch — @ your bot on Telegram/Discord/Slack/Feishu/DingTalk; approve from your phone / IM 与手机派活——在飞书 @ 一句，地铁上手机批准 |
| features[08] | …7,889 tests… | Apache-2.0, 11,752+ tests, event-sourced sessions, zero telemetry. （数字走 facts.json） |
| comparison.items | 7,889 Tests / 12 Modular Crates | 11,752+ Tests / 20 Crates（生成） |
| comparison.rows[COST] | Dynamic billing headers inflate costs 10-20x | Subscription quotas vs pay-per-use with visible context breakdown（删除无来源倍数） |
| 新增 statsbar | — | 6 个数字 + 链 metrics.md |
| 新增 two-products | — | TUI 卡：终端原生·多 agent 舰队·可回放；Desktop 卡：Simple/Advanced 双模式·拖拽面板·IM 派活 |
| meta.title/description | （现状过期） | Shannon — open-source AI workspace…（配 OG 图） |

### 4.3 验收标准

- [ ] 页面所有数字来自 facts.json，CI 校验与 metrics.md 一致；
- [ ] 桌面产品在首屏 StatsBar 之后 2 屏内出现（TwoProducts 组件）；
- [ ] 中英全量对齐（i18n 每个新增 key 双语齐备）；
- [ ] Lighthouse 移动端 ≥ 90；OG 分享卡渲染正确。

---

## 5. 用户文档方案（docs-mdbook/ + desktop/docs/user/ 整合）

### 5.1 整合策略（决策点 #3）

- **mdBook = 唯一对外文档门面**（发布至 shannon-agent.github.io）；
- `desktop/docs/user/` 保留为产品内嵌帮助源（桌面内打开），对外页面改为 mdBook 对应页（用 mdbook `{{#include}}` 引用同一 markdown 源，避免双写漂移）；
- `docs/integrations/` 8 篇以 include 方式收编进文档站导航。

### 5.2 新 SUMMARY（信息屋 = 文档 IA）

```
# Shannon Documentation（改 book.toml: title + 仓库地址 ← R2/D2 修复）
- Introduction（重写：一个引擎四个表面 + 双受众入口卡）
- Getting Started
  - Install（CLI 一行 + 桌面下载）
  - Choose your surface（TUI 5 分钟 / Desktop 5 分钟，Simple 模式优先）
  - Choose a model（BYOK：Anthropic/OpenAI/DeepSeek/GLM/Ollama/兼容端点）
  - Migrate from Claude Code / ZCode（迁移向导，对应已交付功能）
- User Guide（面向所有用户）
  - Chat & attachments（图片/PDF/@ 引用）
  - Voice input（云 + 本地 whisper）
  - Automations: Routines & triggers（cron / API endpoint / GitHub / IM）
  - Goals: 派目标而非派 prompt（/goal、预算上限、收件箱）
  - Agent teams & /batch（worktree、best-of-N）
  - Computer use & browser（截图闭环、本地浏览器、权限档位）
  - IM channels（五渠道接入指南 ← 收编 docs/integrations/im-channels.md）
  - Mobile dispatch（扫码配对、审批）
  - Memory & sessions（记忆、/rewind、跨表面 resume）
- Visibility & Trust（「一切可控」支柱）
  - Cost: 预算上限与上下文拆解、缓存命中率
  - Trace: 事件溯源会话与回放/diff
  - Security & privacy: 权限系统、沙箱、secret-guard、RedactionPolicy、注入扫描、零遥测
- Developer Reference（现 Crate Reference/Architecture/Testing 整体降级至此）
- Integrations（Slack/Jira/Linear/Notion/GitHub triggers，收编现有 8 篇）
- FAQ & Troubleshooting（新增：provider 常见错、模型退役迁移、沙箱限制、成本排查）
```

### 5.3 内容生产优先级

| 级别 | 页面 | 说明 |
|---|---|---|
| P0 | Getting Started 四页、Goals、Automations、IM channels、Migrate | 发布传播直接引流页 |
| P1 | Cost、Trace、Security、Agent teams、Computer use | 承接爆点一/二/五/六的落地质疑 |
| P2 | 其余 User Guide 与 FAQ、Developer Reference 迁移 | 长尾完善 |

### 5.4 验收标准

- [ ] `book.toml` 仓库地址/标题修复，Edit 链接可用；
- [ ] 新 SUMMARY 全部页面存在且有至少 1 张截图或 1 个可复制命令；
- [ ] desktop/docs/user 与 mdBook 无重复维护页（include 或跳转）；
- [ ] 文档站搜索可命中 "secret" "goal" "飞书" "whisper"。

---

## 6. 素材生产清单（三触点共用）

| 素材 | 规格 | 用于 | 生产方式建议 |
|---|---|---|---|
| 桌面全貌截图 | 2560×1440，Simple 模式+任务运行中 | README 首屏 / Hero / PH | 真机截图 + 统一浏览器窗框 |
| 终端会话截图/GIF | 1200×720，goal→trace replay 剧情 | README / Terminal 组件 | asciinema → GIF |
| 舰队 GIF | ≤30s：/batch 三 worktree 并行→并排 diff→采纳 | 爆点三 / 社媒 | 真机录屏加速 |
| 跨表面接力图 | 三格：终端→桌面→手机 | 支柱二 / README | 截图 + 流程图（统一模板色） |
| 成本面板截图 | 上下文拆解 + 缓存命中率弹出层 | 爆点一 / TrustSection | 真机 |
| OG 社交卡 | 1200×630，主口号 + 双产品小图 | 仓库/官网 meta | 模板一次生产 |
| IM 派活竖屏视频 | ≤45s，飞书场景 | 爆点四 / 抖音-视频号 | 真机录屏 |

---

## 7. 防漂移机制（防止门面再次腐烂）

1. **facts.json**：`website/src/data/facts.json` 由脚本从 `docs/metrics.md` 生成（沿用 gen-metrics 思路）；主页统计条/对比表/README 徽章同源；CI job 校验三处一致性，漂移即红。
2. **禁词 CI 检查**：对 `README*.md`、`website/src`、`docs-mdbook/src` 跑 greplint，初版禁词表：`VS Code Extension`、`7,889`、`v0.1.0`、`GPT-3.5`、`10-20x`、`ericdong/shannon-code`；新禁词随决议追加。
3. **对比表保鲜**：仓库 topics/对比表页标注「截至 YYYY-MM」；与竞品研究季度复查（competitive-research §7 监测名单）联动，季度 PR 更新一次。
4. **发布 checklist**：版本发布模板中加「门面四查」：README 数字生成态 / 官网 facts.json / 文档站构建 / OG 卡。

---

## 8. 分期、工作量与里程碑

| Wave | 内容 | 规模 | 门槛 |
|---|---|---|---|
| **Wave 1（1-1.5 天，清障速赢）** | R2/R3（删 VS Code 节、模型表口径）、W3/W4/W5（官网 VS Code 项/7,889/10-20x）、D1/D2（book.toml 修复）、R7 仓库 description/topics | 1 人 1-1.5 天 | 无依赖，**建议立即执行** |
| **Wave 2（3-4 天，README + 主页重构）** | 新 README 双语骨架、对比表、facts.json + CI、website 组件改造与文案替换表落地 | 1 人 3-4 天 + 设计 0.5 天 | 依赖 §6 素材第一批（截图×3） |
| **Wave 3（5-8 天，文档站）** | 新 SUMMARY 全部 P0 页 + include 收编、desktop/docs 整合、FAQ | 1-2 人 5-8 天 | 依赖 Wave 2 文案定稿 |
| **Wave 4（持续）** | 素材补全（GIF/视频）、对比页季度保鲜机制、SEO/OG、计算器落地（联动宣传方案爆点一） | 按宣传节奏排期 | 宣传方案 §7 档期 |

> 建议发布锚点：与 v0.12 发布对齐（同宣传方案 §7.1），Wave 1 在任何传播动作之前完成。

---

## 9. 风险

| 风险 | 缓解 |
|---|---|
| 素材生产阻塞 Wave 2 | Wave 1 先行；截图可用真机 + 标准窗框模板快速产出 |
| 对比表点名竞品引发争议 | 事实型+来源+日期三件套；宣传口径「讲自己做到什么」（宣传方案 §8.3） |
| mdBook include 收编 desktop/docs 产生构建路径耦合 | 仅 include 相对路径 markdown；CI 加文档站构建 job |
| 官网改版引入回归 | 纯静态组件替换，保留现有组件测试；i18n key 全量对照表 review |

---

## 10. 决策记录表（待用户审核填写）

| # | 决策点 | 建议 | 用户裁定 |
|---|---|---|---|
| 1 | 产品名口径：README/官网/文档站统一「Shannon」，"Shannon Code" 仅作历史注 | 统一 | ☐ |
| 2 | README/官网对比表是否点名竞品（事实型+来源） | 点名 | ☐ |
| 3 | 文档整合：mdBook 唯一门面 + desktop/docs include 收编 | 是 | ☐ |
| 4 | Wave 1 立即执行（不等整体评审） | 是 | ☐ |
| 5 | "10-20x" 处置：删除 | 删除 | ☐ |
| 6 | 素材生产排期（截图/GIF 谁做、何时） | 按 §6 清单，Wave 2 前 3 张 | ☐ |
| 7 | 发布锚点：v0.12 "Workspace" | 是 | ☐ |
| 8 | 核心宣传点 =「开源可控 + 密钥安全」 | 是（用户指定，2026-09-10） | ✅ |

---

## 11. 实施记录（2026-09-10，本分支）

**双核心叙事已全量落地到三处门面，构建验证通过。**

### GitHub 仓库页（README.md / README.zh-CN.md）

- Hero 改为双核心主口号：EN "Open source. Total control. Keys never leave your machine." ｜ CN「完全开源，尽在掌控；密钥不出门，数据不搬家」；产品名统一为 **Shannon**。
- 新增「Two commitments」（开源可控 / 密钥不出门）双小节置于首屏，各带证据要点；R5 修复：新增 One engine four surfaces + Terminal / Desktop 双产品章节。
- 对比表改为三列事实型（Shannon ｜ 云端订阅制 agent（Claude Code、Codex、Grok Bot）｜ 开源同类（Hermes、Codex CLI、Grok Build）），删除「10-20x」无来源表述，标注截至日期与来源文档。
- R2 修复：删除 VS Code 扩展章节及项目结构树中的 `editors/vscode` 行。
- R3 修复：provider 表更新（Claude Sonnet/Opus/Haiku、GPT-4o 及更新、DeepSeek、**新增智谱 GLM 行**）。
- 新增章节：Goals & Autonomous Tasks（/goal、预算上限、anti-spin）、Automation & Triggers（cron/API endpoint/GitHub/IM/手机）、Security & Privacy 六层表格；补 `/goal` headless 示例、`SHANNON_TOKEN_BUDGET` 环境变量、`shannon desktop` 运行方式。
- metrics 生成标记（badge/intro/diffrow/table/crates）全部保留，CI 生成链路不受影响。
- 双语结构完全对齐；免责声明等处 "Shannon Code" → "Shannon"。

### 产品主页（website/）

- i18n 全量重写（EN+ZH）：Hero 双核心标题与副标、终端演示换为 `/goal` 派活 + `trace replay` 回放剧情（v0.11）、Feature 8 项重排（01 任意模型 / 02 开源可审计 / 03 密钥安全 / 04 自主目标 / 05 多 agent / 06 桌面 / 07 IM 手机派活 / 08 工程纪律）。
- **新增 TrustSection 组件**（「Two commitments」双卡片，置于终端演示之后、FeatureGrid 之前），`id="security"` 锚点。
- 统计条改为 11,752+ tests / 20 crates / 4 surfaces / 5 IM channels；对比表换为六行事实型 + 列名「Typical Cloud Agent」+ 来源脚注（ComparisonTable 组件已扩展 footnote 渲染）。
- W8 修复：`<title>` / meta description / OG 标签更新为双核心定位。
- 链接修复：Hero / Navbar / DownloadButtons 中 `github.com/shannon-agent/shannon-code` → `github.com/diff-lab-com/shannon-agent`；Navbar/页脚 "Shannon Code" → "Shannon"。
- website 内置 docs（`src/content/docs/`）修复：introduction / roadmap 过期测试数（8,600 → 11,752+）。
- 验证：`astro build` 29 页构建成功；产物抽查确认双核心文案与数字渲染（Keys never leave ×3、secret-guard ×2、11,752+ ×2、Two commitments ×1）。

### 用户文档（docs-mdbook/）

- D1/D2 修复：`book.toml` 标题改 "Shannon Documentation"，仓库地址 `ericdong/shannon-code` → `diff-lab-com/shannon-agent`（edit 模板同步）。
- SUMMARY 重构：新增 **User Guide** 一级分区（Desktop / Goals / Automations & Triggers / Agent Teams & /batch / IM Channels / Mobile Dispatch / Cost Control / Trace / Security & Privacy / Migrating from Other Tools），原 Migration Guide 移入 User Guide；Developer Reference（Architecture / Crate Reference / Development）整体降级。
- 新增 9 个用户文档页（grounded in 代码事实，含命令与配置示例）：`desktop.md`、`goals.md`、`automations.md`、`agent-teams.md`、`im-channels.md`、`mobile-dispatch.md`、`cost.md`、`trace.md`、`security-and-privacy.md`（安全页含「Honest limits」诚实边界小节）。
- introduction.md 重写：工作台定位 + 双承诺 + At a Glance 表（修正错误的双许可表述为 Apache-2.0）。
- 全局修复：`Shannon Code` → `Shannon`（约 20 处）、`ericdong` 链接清零、测试数 8,600 → 11,752+、"12 crates" 表述移除。
- 验证：`mdbook build` 干净构建（处理了一个 SUMMARY 缺文件被 mdbook 自动补桩的问题：删除桩文件 `user-guide/version-migration.md`、SUMMARY 指向真实文件）。

### 遗留事项（不阻塞，移交后续）

- ~~facts.json 生成机制 + 禁词 CI 检查（§7）~~ → **已实施，见 §11.1**。
- 素材生产（§6）：README/官网截图与 GIF 占位待真实素材替换（需真机录屏，无法在本环境生产）。
- ~~GitHub 仓库元信息~~ → **已更新，见 §11.1**（description + topics）。
- `docs-mdbook/src/configuration.md`、`architecture.md`、`crates/*` 等开发者向页面内容未逐页更新（本次只修正错误事实与命名）。
- ~~模型注册表中已退役型号清理~~ → **已实施，见 §11.1**。

### 11.1 第二批实施（2026-09-10，合并 dev 前）

- **facts.json 防漂移机制**：新增 `scripts/gen-facts.mjs`（从 `docs/metrics.md` Summary 表生成 `website/src/data/facts.json`，支持 `--check`）；官网统计条（ComparisonTable）前两位数字改为从 facts.json 读取，i18n 仅保留标签与无机器源的项（4 surfaces / 5 IM channels）。
- **门面卫生检查**：新增 `scripts/check-facade.sh`（禁词表：`7,889`、`8,600`、`v0.1.0`、`10-20x`、`10-20 倍`、`github.com/shannon-agent/shannon-code`、`ericdong/shannon-code`，覆盖 README 双语 + website/src + docs-mdbook/src）。
- **CI 接入**：`ci.yml` 新增 `facade-facts` job（gen-facts --check + check-facade），push dev/main 与 PR 均触发；actions SHA 沿用仓库既有 pin 约定。
- **website 内置文档链接修复**：`src/content/docs/getting-started.md`、`building-from-source.md` 中 `github.com/shannon-agent/shannon-code` → `github.com/diff-lab-com/shannon-agent`。
- **模型注册表 2026-09 刷新**（`crates/shannon-core/src/model_registry/catalog.rs`，依据 grok-bots-research §9.1）：新增旗舰 `grok-4.6`（500K ctx，$2/$6，`grok`/`grok4` 别名改指 4.6）；`grok-4.5` 定价修正 $3/$15 → $2/$6；退役型号 `grok-4.1-fast`（2026-05-15 退役）移除，低价位槽位由 `grok-build-0.1`（256K，$1/$2，承接官方 grok-code-fast-1 重定向）顶替，别名 `grok-fast` 随之改指。配套更新 `model_registry.rs` 两个单测；`cargo test -p shannon-core --lib model_registry` 通过。
- **GitHub 仓库元信息**（gh CLI）：description 更新为双核心口径；topics 新增 `ai-agent`、`ai-workspace`、`byok`、`mcp`、`ratatui`（保留既有 `ai`/`bun`/`coding-agent`/`llm`/`monorepo`/`rust`/`tauri`/`typescript`）。
- **VS Code 扩展目录处置**（2026-09-10 追加决策）：删除 `editors/`（仅含 `editors/vscode`，18 个文件）与 ci.yml 的 `vscode` job——扩展已于 2026-09-05 评审永久放弃，目录属存量死代码；已确认分支保护 required checks 不含 "VS Code Extension"，移除 job 不影响 PR 门禁。历史文档（docs/spikes/p2-8-*、docs/reviews/）按惯例保留不动。
- **仍待办**：真实素材（截图/GIF/竖屏视频）需真机生产；social preview 卡片需在 GitHub 网页端上传；`docs-mdbook` 开发者向页面逐页刷新。
