# 对抗性审查 · DeepSeek Harness 调研对照分析报告（redteam）

> **审查对象**：`docs/reviews/2026-10-15-deepseek-harness-modularity-analysis.md`（下称"原报告"）
> **审查日期**：2026-10-17（原报告落款 2026-10-15，见 F17）
> **方法**：三路——①**独立证伪通道**：对原报告 dsh 侧 12 项承重论断逐条对照一手来源（GitHub API/contents、npm registry API、arXiv、博客原文），先验目标是推翻而非确认；②**本地代码前提核验**：原报告引用但未验证的代码事实逐条实测；③**逻辑/结构/决策质量审查**（论证链、风险分级、与 9/21 复审的兼容性）。
> **结果概览**：12 项事实论断 **11 CONFIRMED + 1 PARTIAL**（一处归因错误），无 REFUTED——事实底盘成立；但发现 **P0×1、P1×9、P2×8、P3×3，共 21 项问题**，其中 1 项动摇核心论证（F1）。原报告未改动，全部修正建议待裁决。

---

## 0. 一段话结论（给审核人）

原报告的 dsh 事实层经受住了独立证伪（这是它最强的部分，尽管其成文方法不该被信任第二次——见 F2），但它的**核心"为什么"论证（§4.3）建立在一个已经修复的缺陷上**：N-4 所述的"registry 级超时从未接线"现已默认接线（`tools.rs:233`），bash 也有 120s 默认值（`system.rs:23,126`）——"这正是管线未正式化的代价"的现在时叙事失锚，且 registry 级超时本身就是一个原始 around 层，P0-1 的增量比报告写的小。此外，"独立趋同"措辞不可证伪（F3）、P0/P1 的风险与成本分级多为断言（F5/F7/F8/F9/F10）、P0-2 未锚定 Shannon 的 Claude Code 兼容策略（F4）。**TL;DR 五条结论在修复后：#1 成立、#2 需改措辞、#3 需重写论证、#4 中 P0-3/spill/ACP 三项分别经受住/降级/换成本表述、#5 成立**（DeepInfra 的沙箱"限写不限读"经原文逐字证实，不抄清单整体成立）。

---

## 1. 证伪通道结果（事实层验证）

### 1.1 12 项论断裁决表

| # | 论断 | 裁决 | 证据要点 |
|---|---|---|---|
| C1 | repo 存在/MIT/TS/2026-08-13 创建/~25 万 star | **CONFIRMED** | GitHub API：created 2026-08-13T11:56Z，251,080★/30,186 forks（10-16 实测，与报告 10-14 快照量级吻合） |
| C2 | architecture/cordis-primer/tool-execution-pipeline 三文档存在且内容相符 | **CONFIRMED** | "There is no privileged core to patch"、五 profile、五派发模式、八段管线逐字对上 |
| C3 | wire-extensions 文档含 `dsh_session_log` + `CLIENT_UPGRADE_REQUIRED` | **PARTIAL** | 前半属实（水位线 at-least-once、8MiB 默认均核实）；但 `CLIENT_UPGRADE_REQUIRED` **不在该文档**，出自 `docs/user/guide/providers.md`——**归因错误，必须改** |
| C4 | 不收外部 PR / 未做安全审计 | **CONFIRMED** | "We are sorry that we cannot accept external pull requests at the moment" 等原文 |
| C5 | npm 首发 08-10 / latest 0.2.0-rc.2 / ~32 版本 | **CONFIRMED** | registry：created 2026-08-10T19:41Z；现 34 版本、alpha 已到 .5（时点漂移，见 F11） |
| C6 | Cordis vendored / cordiverse/cordis / 源自 Koishi | **CONFIRMED**（Koishi 部分为二手佐证） | vendor/README 明示 cordis 4.0.0-rc.7；Koishi 渊源仅 `@cordisjs` scope + 第三方文章佐证，**cordiverse README 本身未声明**——原报告作事实陈述，应降级标注 |
| C7 | arXiv:2608.25512 / 2512.02556 存在且题名相符 | **CONFIRMED** | 两篇 abs 页均抓到，题名/机构/日期逐字吻合 |
| C8 | V3.2 §4.1 评测口径（内部框架 73.1、跨框架 72-74、tbench 46.4 经 CC vs Terminus 39.3） | **CONFIRMED** | HTML v1 §4.1 逐字核实 |
| C9 | awesome-dsh-plugin ~18.8k / topic ~19k / dsh-desktop ~30.9k | **CONFIRMED**（量级） | 实测 19.0k / 20,058（topic 页口径）/ 31.1k——**topic 计数已过 2 万，报告数字全部是时点快照** |
| C10 | DeepInfra 评测（换 loop、CC/Codex 子代理、hooks.json 兼容、**沙箱限写不限读**、clean-exit-但输出损坏） | **CONFIRMED** | 原文逐字："Reading is not confined at all"、"a run can exit clean while its output is broken" |
| C11 | deepseek.com/en/harness 产品页 | **CONFIRMED** | "public preview worldwide and open source" |
| C12 | 双协议端点 / V4.1-Flash（552B MoE、KV 节省 1/4 HBM、谷峰半价） | **CONFIRMED** | api-docs 两 base_url + news260910 原文 |

### 1.2 需要修正的事实错误与精度问题（改报告用）

1. **C3 归因错误**（唯一实质性错误）：`CLIENT_UPGRADE_REQUIRED` 出自 `docs/user/guide/providers.md`，非 wire-extensions 文档。原报告 §2.10 与 §8 来源清单需改。
2. **sdk-minimal 例外**：§2.2 "五种出厂模板→`dsh-base` 为共享首层"漏掉 sdk-minimal **显式不套用** dsh-base（五个里只有四个共享）。
3. **Koishi 渊源降级**：§1.1 表按事实陈述，实为二手佐证（@cordisjs scope + 第三方文章），应标"二手，可信度中"。
4. **时点漂移**：star/topic/版本号全部在漂移；§8 已注时点但正文多处裸引数字，应统一"截至 2026-10-14 快照"并删过度精确数字（249,872 这类数字两天就过期）。

### 1.3 方法论缺陷（比错误本身更重要）

原报告宣称"一手核实"，实际是**单代理单通道**产出，主笔仅核验了本地代码侧，dsh 侧零复核。本次证伪通道证明这次运气好（11/12 成立），但流程上**一份供裁决的调研报告不应依赖这种运气**。改法：报告内置逐节证据分级（A=双源/一手直读原文，B=单源一手，C=二手/摘要），所有"原话"引注标明译/引（见 F12）， reviewer 只需抽查 A 级。

---

## 2. P0 级问题（动摇论证，必须修）

### F1 · §4.3 的因果故事建立在已修复的缺陷上；P0-1 的增量被高估

**问题**：原报告 §4.3 的标题级论证——"N-4（Bash 无默认超时、registry 级超时从未接线）正是没有正式 around 层时的典型缺陷"——用的是**现在时**，但实测现状：
- `ToolRegistry` 已有 `execution_timeout: Option<Duration>`，构造即默认安装 `DEFAULT_EXECUTION_TIMEOUT`，`execute()`/`execute_streaming()` 均以 `tokio::time::timeout` 包裹（`crates/shannon-core/src/tools.rs:190-233,307-318`，注释自引 "Review §P2-2"）；
- bash 工具已有 `DEFAULT_BASH_TIMEOUT_MS = 120_000` 且被使用（`crates/shannon-tools/src/system.rs:23,126`）。

即 **N-4 已闭环**（无论修复时序，现状如此），且 **registry 级超时本身就是一个原始的 around 层**——原报告 §3.1 "guard_nodes 两节点雏形"的表述与 §5 P0-1 "补 around 节点（超时/重试/指标）"的增量声明都因此失准：超时已存在，真正缺的是重试/指标/write-intent 门以及**把这些既有机制归位为显式命名节点**。

**连带的论证问题**：即便 N-4 仍然成立，"缺超时 ⇒ 需要管线正式化"也是单例推断——一个接线疏漏同样可以由"接线清单 + 测试"解决，不唯一地论证管线化。

**改法**：§4.3 重写为两类论证之一或并用——①"历史缺陷类别"叙事（N-4 曾真实发生，管线化使该类别结构性不可能，而非宣称现状仍缺）；②另找仍存在的活体反例（候选：重试/指标确实无统一施加点；write-intent 门无对应物）。P0-1 表述改为"将既有 registry 级超时与缺失的重试/指标归位为显式命名节点"，并同步修 §3.1 表。

---

## 3. P1 级问题（削弱论证，建议修）

### F2 · dsh 事实层单源未复核，报告却宣称"一手核实"
见 §1.3。这不是本案的错（本次验证救回来了），是**成文流程**的错。改法：补证据分级（§1.3 方案），并在 §1.1 表为每行加级别标签。

### F3 · "独立趋同/互不抄袭"不可证伪，且更可能是共同范式
原报告 §0 #2、§3.2 "两套 harness 互不抄袭地收敛"、§4 标题"独立趋同"。但双方都以 Claude Code 为锚：Shannon 的 Claude 兼容是明示产品目标（自家 README/CLAUDE.md），dsh 的 hook 桥兼容 Claude Code hooks.json、"迁移近乎零成本"（DeepInfra 原文证实，C10）。趋同于 append-only 日志/沙箱/skills 更可能是**行业已收敛的共识基本面**（Codex/Claude Code/OpenCode 亦然），"独立""互不抄袭"既不可证伪也无必要。改法：改"趋同于 2025-26 行业共识基本面"，删"互不抄袭"；此改动不影响"Shannon 方向无需改向"的结论，反而更强（三方以上共识）。

### F4 · P0-1/P0-2 未锚定 Shannon 的 Claude Code 兼容策略
P0-2（事件/钩子语义表）的**第一参照系应是 Claude Code 已文档化的 hooks 行为**（exit-code 语义、事件名、匹配语法）——因为兼容 CC 是 Shannon 明示策略，语义表若以 dsh 五模式为纲，等于主动偏离兼容面。同理 P0-1 若引入 dsh 特有的"单调守卫（deny/abstain）""write-intent 门"等概念，需逐一回答"与 CC 行为契约是否冲突"。原报告全篇未做这一对账。改法：P0-1/P0-2 各加一行"兼容锚定"：语义表 = CC 行为为底、dsh 模式词汇为扩展描述；dsh 特有概念标注为"超出 CC 兼容面的增量，单独裁决"。

### F5 · P0-1"低风险"是断言不是论证
around 层超时对流式/长驻工具（PTY、browser、LSP、后台任务）的取消语义是出名难缠的问题（杀子进程组？取消流式输出？部分结果怎么办）；write-intent 门与现有 `WorldSandboxHandle` 写策略（`shannon-tools/src/lib.rs:268-270`）的关系（重复检查？谁先谁后？）未分析。N-4 当初拖了许久才闭环，可能正因接线难。改法：P0-1 拆出"write-intent 门"为独立条目；补一段取消语义风险；"低风险"限定为 Permission/Hook 已有节点归位部分。

### F6 · "Rust 侧等价物必然且已经指向 WASM"是假两难 + 过度声明
实测核心四 Cargo.toml（workspace/core/engine/tools）**无 wasmtime/rhai/lua 任何依赖**——"已经指向"仅是 manifest schema 预留（`manifest.rs:478`），零实现。且选项空间被原报告收窄了：**嵌入式脚本引擎（Rhai/Lua 类）是 hook 类轻扩展的合法中间态**；Shannon 现有 shell-command hooks 本就是"脚本即插件"的 CC 兼容形态。"必然"不成立——WASM 是 manifest 预留的既定终局方向，但"dynamic-extension 只能 WASM 或进程外"是假两难。改法：删"必然"；补脚本引擎作为 hook 类扩展的备选并给出不选的理由（安全面/审计）才算完整。

### F7 · P1-2 spill：价值未论证、成本未提、现状表述不全
①价值：2MiB 截断上限已远宽于同类（Claude Code ~30k 字符量级），被丢的**尾部**在多数工具输出里是噪声；原报告未给任何"用户因截断受损"的证据。②成本：spill 引入磁盘生命周期（谁清理？会话结束删不删？）、回读工具面（Read 定位符的路径语义）、与 tool-result 缓存失效（`tools.rs:822`）的交互——全部未提。③现状：§3.1 "无 spill"只对前台成立；后台任务另有 task store 输出路径（`task_output.rs`），机制相邻但报告未提。改法：要么给出痛点证据（如 eval 中截断导致失败的案例），要么降级为"与 9/21 R1 工具输出策略合并设计的一项"，不单列。

### F8 · P1-3 ACP："改造代价最小"未经论证，且无安全面/定位张力分析
实测 Shannon **零 ACP 代码**（`grep -rin "\bacp\b" crates/` 无命中）——P1-3 = 全新协议实现 + 进程管理 + 凭据透传，"代价最小"没有任何论证支撑。且缺失两个必答问题：①**安全面**：驱动真 Claude Code/Codex 意味着把子进程接上各自的凭据与审批流，逃逸面与审计责任如何划（dsh "保留各自认证"是它的沙箱/审批体系内的说法，Shannon 照搬需要自己的答案）；②**定位张力**：编排专有二进制与 BYOK 中立叙事是否冲突（也可以正向表述为"编排者天然中立"，但原报告没论证）。改法：降格为"定位级**假设**"，定义最小 spike（ACP client 骨架 + CC 子代理 round-trip + 凭据隔离方案），spike 结果回来再谈机会。

### F9 · P1-5 优先级虚高
Shannon 插件生态今天接近零（PluginIndex 刚有 `registry_url` 钩子），peer 版本豁免/`acceptRisk`/安装预检是**为不存在的生态提前建治理**。改法：降 P2，或绑定触发条件（"社区 index 上架第一个第三方插件"）。

### F10 · P1-1"并入 R2.1"掩盖了两种分解方向的可能冲突
9/21 复审 R2.1 的目标是**状态机拆分**（恢复阶梯/压缩/权限相位各自成状态机）；原报告要注入的目标形态是**管线宿主**（pre/around/post 节点 + 事件）。两者不证明兼容：状态机相位与管线节点是两种切分轴，先拆相位再上管线是两步重构。原报告用"并入 R2.1"当免排期牌，实际上是把一次调和决策藏进了引用里。改法：明确"阶段化是状态机化之上的第二步"或先做一次目标形态调和（一页纸），再谈并入。

---

## 4. P2 级问题（精度/表述）

| # | 问题 | 证据 | 改法 |
|---|---|---|---|
| F11 | TL;DR 用 star 数支撑"分量极重"，§6 又自我限定 star 水分——口径不一；且数字两天已漂移（251,080@10-16 vs 249,872@10-14；topic 已 20,058） | §0 #1 vs §6 | TL;DR 加"规模按协议采用折算，star 仅作热度参考"；数字统一标时点 |
| F12 | 中文引号"原话"实为英译未标注（§1.1/§2.1/§2.2 多处） | 证伪 C2/C4：英文原文存在，译法忠实，但标"原话"失真 | 统一标"（译）" |
| F13 | "config_dump 工程化程度其实更高"比较无据——未实际看过 dsh dump 输出形态 | §4.2 | 删或举证（本次核验倒是确认了 P0-3 的前提为真：`config_dump.rs` 确不含 hooks/plugins/skills/mcp 装配面——但这是审查补的验证，报告成文时是断言） |
| F14 | §7 两处"(无对应)"绝对化 | §7 表 | 改"未见直接对应" |
| F15 | hook 事件数"~32"不准，实测 **30**（枚举至 events.rs:70 行，原报告底层调研给的 9-54 行范围也错） | 实测计数 | 改 30；顺带全文行号引用复核 |
| F16 | npm 首发（08-10）早于 repo 创建（08-13）未解释 | §1.1 表 | 注一句"先发包后建仓/公开"防读者当矛盾 |
| F17 | 文档日期失实：落款/文件名 2026-10-15，实际成文 2026-10-17 | 本文件头 | 更名或加勘误注；"数字时点"表述连带统一 |
| F18 | P0-3 未说明非 CLI surface 的暴露方式（desktop 内嵌 engine、gateway 走 WS，均不经 shannon-cli）——落点写了 config_dump（core 侧，可达性没问题）但没写各 surface 怎么拿到这份视图 | §5 P0-3 | 补一行：core 生成 + CLI flag / `/api` endpoint 双暴露 |

---

## 5. P3 级问题（编辑）

- **F19** 建议条目全部无成本估算与验收标准——对比 9/21 复审的 Wave 估时（~1 周 / 2-4 周），本报告作为裁决文档却让裁决人无从权衡（P0-1 多大？P0-2 是一周还是一月？）。每条补 rough size + 一句验收判据（如 P0-2："语义表覆盖全部 30 个事件 + CI 机检上线"）。
- **F20** "独家发现"（§2.10）属自评措辞；"证据等级说明"（§1）未落成逐节 A/B/C 分级（与 F2 同根）。
- **F21** 来源清单多数只有域名/仓库名，无 permalink 或快照链接，第三方读者不可复现（star 数、topic 计数尤其需要存档）。

---

## 6. 审查未发现问题的部分（对等披露）

- **不抄清单整体成立**：DeepInfra 的"沙箱限写不限读"经原文逐字证实（C10），遥测/锁定判断的事实部分经 C3 证实（仅归因需改 1 处）；"不抄 patch 配置层叠""不抄插件秀优先级"论证充分。
- **P0-3 的前提为真**（config_dump 确不含装配面，本次核验补证）。
- **与 9/21 复审的映射表方向正确**（除 F14 措辞与 F10 的"并入"问题）。
- **事实底盘**：12 项论断 11 项 CONFIRMED，对一份单通道调研而言质量高于预期——但见 F2，这是运气不是流程。

## 7. 修复优先级建议

1. **必须**：F1（§4.3 重写 + P0-1 表述修正 + §3.1 表同步）、C3 归因、F3（措辞）、F15（计数）。
2. **应改**：F4（兼容锚定）、F5（风险段）、F6（假两难）、F7（spill 降级或补证）、F8（ACP 换假设+spike）、F9（降级）、F10（调和说明）、F2/F20（证据分级）、F11-F18 按表。
3. **可选**：F19-F21。

## 附 · 证伪通道局限声明

证伪亦为单代理单通道（与被审报告同构），但做了两点对冲：①只采信可直接抓取的一手端点（GitHub API/contents、npm registry API、arXiv abs/HTML、博客原文），拒绝搜索摘要；②raw.githubusercontent 被沙箱阻断时改走同一源的 contents API。若需更高置信度，建议对 C2/C3/C8 三项做人工二次直读。
