# LLM 出站密钥防护调研：gitleaks 检测与「占位符替换 + 响应还原」方案

- 日期：2026-09-08
- 任务：
  1. 调研 gitleaks，分析将其应用于「请求传输给 LLM API 服务商之前」做密钥检测的可行性与必要性；
  2. 分析「出站前把密钥替换成随机码、LLM 响应再还原为真实密钥」这一可逆脱敏方案的可行性与必要性，重点是**不破坏 context 前缀缓存**；
  3. 调研竞品是否有类似功能。
- 性质：纯调研分析，不含实施。
- 输入：gitleaks / trufflehog / Kingfisher 官方仓库与文档；Anthropic / OpenAI / DeepSeek / Gemini prompt caching 官方文档；Claude Code、Cursor、Copilot、Bedrock Guardrails、Google Sensitive Data Protection、Presidio、llm-guard、LiteLLM 等产品文档（截至 2026-09-08）；Shannon 代码库现状（`shannon-mono` @ `5c0c7ba5`）。
- 结论速览：**① gitleaks 本身不建议作为内联运行时引擎直接嵌入 Rust 请求路径（Go 二进制 sidecar 可行但重，且每次调用重编译全部规则），它的核心价值是「MIT 许可的 222 条密钥规则库」（零 lookahead/backreference，移植 Rust 已有先例验证；注意 gitleaks 本体已宣布 feature-frozen，新规则流向继任项目 Betterleaks）；密钥检测本身必要性高，但应从「审计/警告」档起步。② 「随机码替换 + 还原」方案有条件可行：占位符必须用本地主密钥 HMAC 确定性派生（绝不能每请求随机，否则既击穿前缀缓存又破坏模型引用一致性），替换点应放在内容入库时而非发请求时，还原只作用于执行面（工具参数 / 显示）且绝不回写会话历史。做到这三点即可与各家 prompt caching 完全兼容（各家缓存均为字节级前缀匹配 + 后缀失效，确定性替换的缓存行为与发原文严格等价）。③ 竞品中没有任何编码 agent 内置此能力（Claude Code 官方拒绝了同类 feature request，GitGuardian 商业 hook 明确止步于阻断），唯一端到端先例是社区代理 claude-code-redact——其进程内存映射表（重启即失效）与零缓存分析恰是 Shannon 可以做出差异化的两个点。**

---

## 1. 背景与威胁模型

### 1.1 问题定义

编码类 agent 的工作方式决定了它是**凭证的高流量搬运工**：读 `.env`、`cat` 配置文件、执行 `env`/`printenv`、git 操作、调试 auth——每一次文件读取和命令执行的输出都会作为对话内容发往 LLM API。密钥（API key、云凭证、私钥、数据库连接串）一旦进入对话上下文，就会随每个后续请求反复出站，落到：

- LLM 服务商侧的日志、审计与（按政策的）训练数据；
- 用户自配的第三方 `base_url` 中转/聚合服务（Shannon 明确支持任意 OpenAI-compatible 端点，这条路径的信任级别远低于第一方大厂）；
- 服务商潜在的数据泄露面。

本报告评估两道防线：
1. **检测（detection）**——出站前识别"这里有条密钥"，用于警告、拦截或审计；
2. **可逆替换（reversible tokenization）**——识别后替换为占位符再发送，模型返回的占位符在本地还原为真实值，使"模型可以引用密钥但永远见不到真值"。

### 1.2 Shannon 的暴露面（代码事实）

| 事实 | 位置 | 对本调研的意义 |
|---|---|---|
| LLM 请求统一经 `LlmClient` 出站，已有**请求观察点** `with_request_capture(|wire: &serde_json::Value| …)`，每个 adapter 序列化后的 wire body 都会流经（当前只读，tee 进 L0 审计日志，且注释强调与线上请求字节一致） | `crates/shannon-core/src/query_engine/engine.rs:1833`、`crates/shannon-engine/src/api/client.rs` | 出站检测/替换有现成的单一咽喉点，接入成本低 |
| 工具结果经 `ToolResultEntry` 回填对话历史；文件内容、命令输出都从这里进入上下文 | `engine.rs` 主循环 | 密钥的主要入口是工具结果而非用户输入，检测必须覆盖 |
| 已有 `RedactionPolicy`：**本地 session 日志写路径**脱敏，内置 token 形状（`sk-`、`ghp_`、`github_pat_`、`xox[abp]-`、`glpat-`）、`redaction.toml` 用户扩展、以及**进程环境变量快照精确匹配**（变量名含 `KEY`/`SECRET`/`TOKEN`/`PASSWORD` 且值 ≥8 字符），内置层不可关闭（fail closed） | `crates/shannon-core/src/session_log/redaction.rs` | 规则来源、配置形态、"写时生效"哲学均可直接复用到 LLM 出站路径 |
| Anthropic 兼容端点注入三层 `cache_control` 断点；**第三方端点明确跳过**（有测试锁定） | `crates/shannon-engine/src/api/adapter.rs:150`、`engine.rs:98` 附近 | prompt caching 是 Shannon 的一等公民（README 以"无破坏缓存的动态头"为卖点），任何出站变换必须证明缓存无害 |
| Provider 枚举：Anthropic / OpenAI / Ollama / **Custom** / Gemini / Azure / Bedrock / Mistral | `crates/shannon-engine/src/api/types.rs:50` | 缓存语义因厂商而异；Custom（任意中转）是暴露风险最高、也是缓存与合规都最不可控的一条 |
| 远程目标（SSH/Docker）与 IM 渠道（Telegram/Discord/Slack）接入 | `crates/shannon-remote`、`docs/integrations/im-channels.md` | 密钥入口面更宽（远端机器的输出、IM 消息），出站检测价值随之上升 |
| 压缩（compaction）会把历史发去做摘要 | `crates/shannon-engine/src/compact/` | 脱敏必须发生在压缩请求之前——若历史已入库时替换，此问题自动消解 |

### 1.3 威胁模型与方案边界

**防护对象**：密钥值本身出站。具体威胁：
- T1 服务商留存/训练/人工审查（受隐私政策约束，Custom 中转基本不受约束）；
- T2 服务商侧日志系统被攻破（历史上有真实泄露先例）；
- T3 中转/聚合服务作恶或被攻破（TLS 只保护传输，中转就是终点）；
- T4 合规要求：企业与客户合同常要求"凭证不出边界"（SOC 2、金融、政企）。

**明确不防护**（避免高估方案价值）：
- 源代码本身仍然完整出站——本方案只处理密钥这一最高价值的窄类别；攻击者拿到源码仍可能推断架构与内部端点，只是拿不到"立即可用"的凭证；
- 密钥的**存在性、数量、位置与格式**仍会泄露（除非检测到就整体拦截该内容）；
- 本地机器被攻破的场景；
- 检测漏网的密钥——**检测召回率是整个方案唯一的安全参数**，这是所有 Regex 系方案的根本局限。

---

## 2. 调研方案

| 研究问题 | 判据 | 信息来源 |
|---|---|---|
| Q1 gitleaks 能否作为 Rust 应用的内联检测引擎？ | 集成方式（CLI/库/规则移植）、单请求延迟预算（payload 100KB–2MB）、误报/漏报特性、发行影响（多平台捆绑额外二进制） | gitleaks 仓库/文档/issue、基准数据、同类工具对比 |
| Q2 密钥检测的必要性分级？ | 实际泄露路径的频率与后果；检测误报对用户任务的打扰成本 | 威胁模型推演 + 竞品做法对照 |
| Q3 占位符替换是否破坏前缀缓存？ | 各厂商缓存的匹配语义（前缀字节一致？token 一致？失效范围）、缓存命中/未命中价差 | Anthropic / OpenAI / DeepSeek / Gemini 官方 caching 文档 |
| Q4 还原的可靠性边界在哪？ | 模型对占位符的保真度（逐字复制/篡改/自造）、还原失败后果、可接受的失败模式集合 | 设计推演 + FMEA；通用 LLM 领域可逆脱敏先例的行为 |
| Q5 有无竞品先例？ | 编码 agent 是否内置；网关/DLP 层的检测与可逆替换产品 | Claude Code / Cursor / Copilot / Bedrock / GCP DLP / Presidio / llm-guard / LiteLLM 等文档 |

局限：未做实机延迟压测（gitleaks 对大 payload 的实测耗时）、未验证各中转服务的实际缓存行为、竞品部分结论依赖公开文档而非逆向。

---

## 3. gitleaks 调研与分析

### 3.1 gitleaks 能力概览

- **项目状态**：最新 v8.30.1（2026-03），MIT 许可，Go 编写，~29.2k stars。**关键动态：官方已宣布"feature complete"，后续仅出安全补丁**；原作者 Zach Rice 转向继任项目 **Betterleaks**（Aikido Security 赞助，2026-03 发布，兼容 `.gitleaks.toml` / `GITLEAKS_CONFIG`，以 token-efficiency 检测替代 Shannon 熵）。含义：把 gitleaks 规则集 vendor 进 Shannon 是安全决策，但**新规则的持续跟进要转向 Betterleaks/Kingfisher 生态**，不能指望 gitleaks 本体再增长。
- **规则模型**（默认配置实测 **222 条规则**，2026-09 master）：`regex` + **`keywords` 关键词预过滤**（222 条中 221 条定义，先做字符串查找再跑正则，大幅降低扫描成本）+ 可选 Shannon 熵阈值（129 条启用，多为 2.0–3.5）+ `secretGroup` 捕获组指定 + 规则级/全局 allowlist（v8.21+/v8.25+）+ 复合规则（v8.28+，实验性）。
- **语法兼容性（移植可行性的关键事实）**：Go RE2 方言，默认规则**零 lookahead、零 backreference**，70 处 `\b`（ASCII 语义）——与 Rust `regex` crate（同为 RE2 系：线性时间、无回溯、无环视）高度兼容；仅需对 `\b` 的 Unicode 语义差异做 `(?-u)` 对齐，并在构建期对全部 222 条模式逐条编译校验。
- **覆盖类别**：云厂商 key（AWS/GCP/Azure）、SaaS token（GitHub/GitLab/Slack/Stripe/npm/PyPI）、OpenAI/Anthropic 风格 `sk-` key、私钥/keystore、JWT、URL/DSN 内嵌密码、generic-api-key 熵兜底（keyword+分隔符+值形状，熵 3.5）。
- **工程化配套**：`gitleaks:allow` 行内抑制、`.gitleaksignore` 指纹忽略、baseline 只报新增、`--report-format json/sarif`、`git`/`dir`/`stdin` 三种扫描模式（`detect --no-git` 是旧拼写）、`--max-decode-depth`（base64/hex/percent 解码，默认关闭）。

### 3.2 集成到 LLM 出站路径的四种方式

| 方式 | 做法 | 优点 | 缺点 | 判断 |
|---|---|---|---|---|
| A. CLI 子进程 sidecar | 捆绑 gitleaks 二进制，`gitleaks stdin --report-format json` | 规则零维护 | 每平台捆绑一个 Go 二进制（Shannon 本体是单二进制 Rust 应用，属打包退化）；**每次调用重编译全部 222 条规则**（估算每请求 +10–50ms，无跨运行规则缓存）；跨进程 JSON 协议；上游已冻结 | 仅适合 Phase 0 旁路快速验证 |
| B. Go 库嵌入 | cgo FFI 调用 | —— | Rust 进程内嵌 Go runtime 不现实 | 排除 |
| C. **规则移植进 Rust（推荐）** | 构建期把 gitleaks 默认 TOML 编译为 `regex` crate 规则；一并移植 keywords 预过滤、熵检查（对 secretGroup）、allowlist 机制 | 单二进制、零跨进程开销；与既有 `RedactionPolicy` 共用引擎与 `redaction.toml` 配置形态；语法近 100% 兼容（§3.1），构建期逐条编译校验、不兼容规则报告后跳过 | 规则语料自持（上游冻结加剧此责任） | **主路径**。已有成功先例：opencode-stranger-danger 将 gitleaks TOML 构建期编译进宿主引擎（JS）落地 |
| D. 原生 Rust 引擎 | Kingfisher（MongoDB，Apache-2.0，Nosey Parker 分叉）：Vectorscan SIMD + tree-sitter 上下文解析，提供 **beta 库 crate**（kingfisher-core/rules/scanner） | 原生嵌入可行；规则目录现跟踪 Betterleaks + Veles | 依赖树重（C++ Vectorscan）；库 API 处于 beta；规则目录已脱离 gitleaks | 备选评估 |

**延迟量级**：独立实测数据点——opencode-stranger-danger 以 ~160 条 gitleaks 规则 + 熵检测做到 **<5ms/100KB**（JS 正则引擎，启动 ~0ms）；gitleaks 官方二进制的 5.5MB/s（issue #2044）是整仓端到端数字（含文件 IO、git 元数据），非引擎吞吐。Rust `regex` 的 lazy DFA 吞吐通常不低于 Go regexp。按 §5.4 的"入库时扫描"放置（新内容块 KB–100KB 级），扫描成本完全移出请求关键路径，**延迟不构成否决项**；wire 层全量 2MB 重扫估算 10–100ms，对只读审计档可接受。

### 3.3 必要性评估

- **高价值**：密钥是唯一"泄露即沦陷"的内容类别——一条泄露的云密钥可直接横向移动，而源码泄露是概率性损失。检测的边际成本低（毫秒级扫描），期望收益高。该问题已被市场确认为真实痛点：Betterleaks 以"AI-agent-ready"定位发布、GitGuardian 为 AI 编码工具推出专用 hook，均以编码 agent 泄露凭证为卖点。
- **分级必要**：
  - 审计/统计档（Phase 0）：只记录不干预——**必要且无风险**，产出"真实命中率/误报率"数据支撑后续决策；
  - 警告档：高危类别（私钥、云根凭证）出站前提醒——必要，打扰可控；
  - 默认拦截档：**现阶段不必要且危险**——误报会直接破坏用户任务。误报是此类工具的头号问题且有量化证据：熵类 generic 规则 precision 仅 ~21.1%（CredData 基准，Betterleaks 作者口径）；一著名的厂商对比中 gitleaks 报出 339,275 条命中而 TruffleHog（带活体验证）仅 144 条（vendor 数据，取其量级）。工业界对该场景的默认取舍是 **fail-open**（GitGuardian 的 AI hook 在扫描器不可用时放行请求）——支持"分档起步、谨慎拦截"。
- **必须诚实的一点**：Regex 系检测对"任意密钥"的召回率是结构性不足的（运行时拼接构造、编码变形、无前缀的非标内部 token 都会漏；Shannon 熵本身被 Betterleaks 分析为召回瓶颈）。所以形状规则只适合做**分层的其中一层**（§5.7）；路径级策略（`.env`/`.pem`，确定性）与环境变量精确匹配（复用 `redaction.rs`）价值更高。

### 3.4 同类引擎对比

| 引擎 | 语言 / 嵌入形态 | 规则量 | 活体验证 | 内联适用性判断 |
|---|---|---|---|---|
| gitleaks | Go CLI；**规则 TOML 为 MIT，可移植** | 222 | 无 | 规则语料最优；引擎按 §3.2C 移植 |
| TruffleHog | Go CLI | 800+ 检测器 | **有**——对每个候选向厂商 API 发非破坏性验证调用 | 出站场景绝对禁止（对刚检出的密钥发起网络调用 + 延迟）；无验证模式即"又一个 Go 正则二进制"，无优势。排除 |
| detect-secrets | Python 库 | 插件式（熵基线工作流） | 无 | Python 解释器进请求路径不现实。排除 |
| Kingfisher | **Rust 库（beta）** | Betterleaks 目录 + Veles 检测器 | 可选（网络调用，可关） | 唯一可原生嵌入的替代；依赖重、API 未稳。备选 |
| Nosey Parker | Rust CLI/datastore | 强（Hyperscan 系，GB/s） | 无 | 未提供嵌入库。不适用 |
| ripgrep 自研规则 | Rust CLI | 自维护 | 无 | 需自建熵/allowlist 机制，被方案 C 严格支配 |

### 3.5 小结

gitleaks 用于出站检测：**规则语料必要且直接可用（MIT、RE2 系语法、可移植性已被先例验证），引擎本身不需要也不应该引入**——推荐"规则移植进 Rust + 分层检测"而非捆绑 Go 二进制。注意上游冻结：规则语料的持续更新责任（可跟踪 Betterleaks 目录）由 Shannon 侧承担。

---

## 4. Prefix caching 机制与替换方案的硬约束

### 4.1 各厂商缓存语义速览

| 厂商 | 模式 | 匹配语义 | 关键参数（2026-09 官方文档） |
|---|---|---|---|
| Anthropic | 显式 `cache_control` 断点（≤4 个）或顶层自动 | **"100% identical prompt segments"**——tools→system→messages 的最长前缀匹配；消息 k 变更只失效 k 之后（后缀失效，官方明示），早于断点的前缀继续命中 | 最低可缓存 512–4096 token（按模型档）；**read=0.1×、write(5m)=1.25×、write(1h)=2×**；TTL 5min（可选 1h），命中免费续期；响应含 `cache_read_input_tokens` 与 `cache_diagnostics`（能直接观测 `messages_changed` 等失效原因） |
| OpenAI | 自动 | **精确前缀匹配**，≥1024 token 起缓存，此后 128 token 粒度，部分命中到第一个差异 token | cached 折扣按代际 50%（GPT-4o）→75%（GPT-4.1）→**90%（GPT-5 系）**；闲置 5–10min 逐出（峰时可达 1h）；前缀中含每请求数据（时间戳/随机 ID）被官方文档点名为反模式 |
| DeepSeek | 自动（磁盘缓存） | 缓存单元完整匹配 + **公共前缀检测**把共享前缀持久化，多轮对话完整复用上轮单元 | hit $0.014–0.044/M vs miss $0.22–0.66/M（V4 系，峰谷分时，hit 约 miss 的 1/10–1/15）；闲置数小时–数天清理；官方定价页已改版，引用前需复核 |
| Gemini | 隐式（2.5+ 自动）+ 显式（CachedContent 按引用复用） | 前缀精确匹配；显式缓存即前缀本身，精确性由构造保证 | 隐式最低 ~1024 token（Flash）/2048（Pro）；cached ≈ 输入 10%（2.5 Flash）；显式 4096 token 起 + **存储费 ~$1/M token/小时**（短 TTL 下得不偿失） |

共同点：**缓存键 = 请求前缀的 token 序列，四家全部后缀失效**。字符串逐字相同 ⇔ token 序列相同（BPE 确定性），所以**字节级确定性的变换对缓存是安全的，不确定性变换是致命的**。

**字节级匹配的官方实证**：Anthropic 文档明确记录 Go/Swift SDK 序列化 `tool_use`/`tool_result` JSON 时 **key 顺序不确定会直接造成缓存 miss**——缓存匹配敏感到 JSON key 的排列。对替换方案的推论：不仅占位符必须确定性，**请求序列化本身也必须规范确定**。Shannon 以 Rust serde + 事件溯源历史（append-only events.jsonl 投影）构造请求，天然满足；需用不变量测试锁定。

**一个容易搞反的直觉**：替换并不移动缓存失效点。密钥在历史中的位置决定缓存从哪里切开；换成占位符后，切分位置与发送原文完全相同。替换方案的缓存风险**全部来自不确定性，与"是否替换"无关**——确定性替换的缓存行为与发原文严格等价。

### 4.2 什么会击穿缓存（替换方案视角）

1. **每请求重新随机**（方案最常见错误实现）：同一密钥第 1 轮是 `[SECRET_a1b2]`、第 2 轮变成 `[SECRET_c3d4]`——历史部分每轮全变，**每一轮都是全量 cache miss**。对 200K token 的会话，相当于把每次请求都变成 cache write（约 1.25×）而非 cache read（约 0.1×），输入成本放大约 10 倍以上，且 TTFT 显著劣化。这与 Shannon"缓存友好"的产品定位正面冲突，属**一票否决项**。
2. **有状态映射表的丢失/重置**：占位符存内存、映射表重启丢失 → 重启后同一密钥派生出新占位符 → 长会话恢复（resume）时历史前缀全变 → 全量 miss。若映射表按 TTL 清理同理。
3. **规则升级导致历史内容被"补检"**：新版规则在旧历史里新检出一条密钥并替换 → 该位置之后全部失效。缓解：新规则只对新增内容生效，或接受升级后首轮 miss。
4. **密钥轮换**：历史里的旧占位符不变（HMAC 以旧值为输入），不受影响——这是确定性派生的自然性质。
5. **追加式内容无害**：新增的工具结果/用户消息只追加在末尾，不触碰前缀——替换方案在"入库时替换"放置下（§5.4）天然追加式。

### 4.3 缓存成本模型

200K-token 会话（190K 未变前缀 + 10K 新增），每轮输入成本算例：

| 模型 | 全 miss（无缓存） | 稳态命中轮 | 差距 |
|---|---|---|---|
| Claude Sonnet 4.5（$3/M 输入） | $0.60 | $0.094（190K × 0.1× + 10K × 1.25×） | **~6.4×** |
| GPT-5.1（$1.25/M，90% 折扣） | $0.25 | $0.036 | **~7×** |
| Gemini 2.5 Flash（隐式） | $0.06 | $0.0087 | **~7×** |
| DeepSeek v4-flash（谷时） | $0.044 | $0.0035 | **~13×** |

若占位符每请求随机（§4.2.1），密钥出现在前部的长会话**每一轮都落在"全 miss"列**——输入成本放大约一个数量级，TTFT 同步劣化。这是缓存约束构成一票否决项的定量版本。另注意 Anthropic 的不对称性：write 比 miss 贵 25%，所以击穿缓存的影响在 Anthropic 上比"纯 miss"还要再贵一点；而在 DeepSeek（hit 折扣最深）上代价最大。

---

## 5. 「占位符替换 + 响应还原」方案深度分析

### 5.1 方案描述

```
[入库时] 用户输入 / 工具结果 / 注入文件
    → 检测（分层，§5.7）
    → 查无此密钥则登记：surrogate = format(HMAC(master_key, secret_value))
    → 内容以占位符形态写入对话历史
[出站] 请求体只含占位符（前缀逐轮字节稳定 → 缓存照常命中）
[响应] 模型输出引用占位符
    → 还原（仅执行面）：Write/Edit 工具参数、Bash 命令参数、显示文本
    → 还原值绝不回写对话历史
```

### 5.2 设计决策 1：占位符必须确定性派生（HMAC），不能每请求随机

这是整个方案的成立前提，同时回答缓存与模型行为两个约束：

- **缓存约束**（§4.2.1）：不确定性 = 每轮全量 miss，成本放大一个数量级。
- **模型一致性约束**：模型需要在第 5 轮引用第 1 轮看到的密钥（"用上面那个 key 填进 config"）。占位符跨轮稳定，模型才能正确引用；还原映射才有意义。随机占位符下，同一密钥在历史里出现 N 个化身，模型必然混用。
- **实现：无状态 HMAC 派生完胜有状态映射表**：
  - `surrogate = truncate_n(HMAC-SHA256(master_key, secret_value))`，`master_key` 本地随机生成持久化（或机器派生）；
  - 无映射表可丢失/泄露（有状态表本身就是一个明文密钥库）、无缓存失效风暴、天然跨会话/跨重启稳定；
  - 相等性泄露（同密钥同占位符）是可接受的：模型本来就能从上下文看到相等性；
  - 离线爆破不可行（无 master key）；截断取 48–64 bit，生日碰撞界在百万密钥量级，本地场景充分。
  - 先例对照：claude-code-redact（rdx）把 secret→token 映射存进程内存，重启即丢——恢复会话时占位符永久失效且必然击穿缓存；无状态 HMAC 派生正是对此的结构性修正（Google DLP 的 cryptoDeterministic surrogate 也是同一思路：确定性加密代替映射表）。

### 5.3 设计决策 2：占位符形态——格式保持（format-preserving）优于裸标签

| 形态 | 例 | 优点 | 缺点 |
|---|---|---|---|
| A. 裸标签 | `__SK_9f3a72c1e5d8__` | 还原即精确匹配，模型明确知道是占位符 | 替换后代码语义漂移：用户代码里的长度校验/正则/切片逻辑对占位符不再成立，模型基于占位符写的代码可能是错的；markdown 下划线转义破坏精确匹配 |
| B. **格式保持**（推荐） | AWS key → `AKIA` + 16 个 [A-Z0-9]（字符从 HMAC 流取） | 代码语义不变（长度/字符集/前缀形状保持）；误报**无害化**——把非密钥误替换后还原即恢复，可逆性把误报从"数据损坏"降级为"无操作"；模型不会因为看到怪异 token 而行为畸变 | 假 key 外观逼真：若还原遗漏，假 key 混入提交，看起来像真泄露（触发密钥扫描器/误导用户）——必须有 §5.6 的"未还原密钥形状告警"兜底 |
| C. 混合 | 形状保持 + 隐藏标记位 | 兼顾 | 复杂度高，暂不推荐 |

按规则类别选形态：env 变量值、token 类 → B；私钥块（PEM）→ 结构保持（换 body 保头尾）；模型大概率**不需要**写回的类别（如一次性调试查看）可用 A 或单向遮蔽（`sk-ant-***`），根本不进还原映射。格式保持形态的两个先例：Google DLP 的 FFX 格式保持加密（原语级），claude-code-redact 的 "realistic fakes" 模式（应用级）。

### 5.4 设计决策 3：替换点在"内容入库时"，而非"请求序列化时"

- **入库时替换**（推荐）：新内容块（用户消息、工具结果、注入文件）进入对话历史前替换 → 历史本身不含真值 → 出站请求天然只含占位符且**逐轮字节稳定**（缓存安全，§4.2.5）；扫描成本在内容产生时付（毫秒级），完全移出请求延迟；session 落盘也干净（与 `redaction.rs` 的"写时生效"哲学同构）；compaction 请求自动安全。
  - 注意分层显示：权限确认 UI / diff 视图等**本地执行面**仍展示真值，替换只发生在入历史这一步。
- **序列化时替换**（wire 层，如复用 `with_request_capture` 改造成变换器）：每轮对全量历史重扫重换。确定性派生下缓存同样安全，但：每轮重复扫描成本进请求路径；规则升级会即时改动历史字节（§4.2.3 缓存击穿）；观察点当前语义是只读字节一致 tee，改造为变换器会牵动 §4.2 审计不变量。
- **结论**：入库时为主，wire 层保留**只读审计**（Phase 0 形态：复用现有 `with_request_capture`，检测命中只记日志不改字节），量化"入库检测的漏网率"——两层互补：入库层保证缓存与延迟，wire 审计层度量真实漏检。

### 5.5 设计决策 4：还原只作用于执行面，绝不回写历史

**单向性不变量**：真值只允许存在于"本地执行面"（写盘内容、子进程命令、终端显示），永远不允许进入对话历史。否则模型输出被还原后存进历史 → 下一轮请求把真值又发出去 → 方案自我瓦解。

具体落点：
- `Write` / `Edit` / `MultiEdit` / Notebook 工具：在参数应用到文件系统**之前**对完整参数字符串还原（工具参数是完整结构化 JSON，天然规避流式撕裂）；
- `Bash` 等命令执行：执行前还原命令串；命令输出若含真值（如 `echo $KEY`），输出入库时再走一遍替换——HMAC 派生保证幂等，闭环成立；
- 流式文本显示：完整消息级还原（渲染层），流中短暂显示占位符可接受；IM 推送（Telegram/Discord）属于**出站面**，应还原后发送还是保持占位符需产品决策（建议还原：IM 是用户自己的通道，但要标注）；
- 历史存储（events.jsonl）：只存占位符——副作用是 `trace replay/export` 天然无密钥，审计面反而变好。

### 5.6 失败模式清单（FMEA）

| # | 失败模式 | 后果 | 缓解 |
|---|---|---|---|
| F1 | **漏检**（检测召回不足） | 密钥明文出站——方案核心风险 | 分层检测 §5.7；wire 审计量化；路径级策略兜底；Phase 0 数据说话 |
| F2 | **误报**（把非密钥当密钥） | 可逆方案下：还原即恢复，**近似无害**；单向遮蔽下：内容永久损坏 | 优先可逆；单向遮蔽仅用于模型无需写回的类别 |
| F3 | 模型篡改占位符（大小写、截断、markdown 转义、跨行拆分） | 还原 miss，占位符写进文件 | 格式保持形态 + 字符集规避 markdown 特殊字符（纯字母数字）；对输出中"密钥形状但未命中映射"的字符串做编辑距离 ≤2 的模糊修复 + 失败告警 |
| F4 | 模型自造一个"看起来像真的"值而不引用占位符 | 假密钥写进产物 | F3 的同款兜底检测：输出里任何未登记的密钥形状字符串 → 警告用户 |
| F5 | 密钥互为前缀/子串 | 替换/还原错乱 | 登记时按长度降序替换（最长优先），还原同理 |
| F6 | 占位符与真实内容撞串 | 误还原 | 占位符字符集 + 长度设计成与所在上下文正交；登记表内查重（HMAC 截断碰撞概率可忽略） |
| F7 | 还原值进入子进程后被记录（shell history、进程列表） | 本地泄露面扩大 | 本地已是信任边界内，与现状等价，不新增风险；文档明示 |
| F8 | 规则升级 / master key 重置 | 历史字节变化 → 缓存击穿；还原断链 | master key 永不轮换（或轮换=显式开新会话）；规则升级只影响新内容（§5.4 入库替换天然满足） |
| F9 | 压缩摘要、多 agent 消息传递中出现占位符语义漂移 | 子 agent 拿到占位符却没有还原映射 | 映射登记表按会话共享（master key 派生，无状态，天然全局可用）；压缩提示词中说明占位符含义 |
| F10 | 用户明确要求模型使用真值（"把这个 key 放进 curl 试试"） | 体验与安全的正面冲突 | 占位符 + 执行面还原已覆盖此场景（命令执行前还原）——模型引用占位符即可，无需看见真值 |
| F11 | 流式还原撕裂（占位符跨 delta 分片） | 显示错乱 | 还原在完整消息/完整工具参数层做，不做 chunk 级 |

### 5.7 检测层设计（分层，复用既有资产）

| 层 | 机制 | 精度 | 角色 | 与现状关系 |
|---|---|---|---|---|
| L1 形状规则 | gitleaks 规则语料移植（§3.2C）+ `redaction.rs` 内置 token 形状 | 高精度、中召回 | 主力检测 | 复用 `RedactionPolicy` 引擎与 `redaction.toml` 配置 |
| L2 环境快照精确匹配 | 进程 env 中敏感名变量值，字面匹配（Aho-Corasick，非 regex） | 极高精度 | 抓"任何从 env 流出的值"，对非标 token 也有效 | **直接复用** redaction.rs 已有机制 |
| L3 路径级策略 | `.env` / `*.pem` / `id_rsa` / credentials 文件**路径**命中的内容整体按策略处理（替换或拦截） | 确定性 | 比内容检测更可靠的兜底 | 新增，成本低收益高 |
| L4 熵扫描 | 高熵 token 检测 | 低精度高召回 | 仅警告档，不自动替换 | 可选 |

### 5.8 残余风险与攻击面

- 召回率是唯一安全参数（F1）：本方案是**降低泄露概率与爆炸半径**，不是安全边界；企业强合规场景仍需路径级 + 网关级双保险。
- 信息泄露的"缩窄"而非"消除"：占位符泄露密钥的数量、位置、相等关系与格式；HMAC 阻断离线爆破后，这些元数据的利用价值很低，但应如实写进文档。
- 信任边界转移：还原逻辑成为新的高危代码路径（它持有 master key 与映射知识）；需审计还原面的注入可能（模型诱导"把占位符替换逻辑用在攻击者路径上"的收益极低，风险可忽略）。
- 体验风险大于安全风险：F3/F4 类还原失败破坏用户产物，比漏检更容易被用户感知——工程重心应在还原可靠性 + 兜底告警。

### 5.9 小结

方案**有条件可行**：确定性 HMAC 占位符 + 入库时替换 + 执行面还原三原则确立后，缓存完全兼容、语义基本无损、误报可逆。**必要性中等偏上**，对 Shannon 的两类用户尤其成立：Custom 中转端点用户（T3）与企业合规用户（T4）。复杂度集中在还原可靠性，建议窄域起步（§7.2）。

---

## 6. 竞品调研

### 6.1 编码 agent：无一内置，需求被记录但多被搁置

| 产品 | 出站密钥防护 | 实际机制 | 来源 |
|---|---|---|---|
| Claude Code | **无**。上下文脱敏 feature request（#29434，2026-02）被官方**关闭为 not planned** | 官方 hooks（PreToolUse/PostToolUse/UserPromptSubmit）语义上只能 block/detect、**不能改写**（issue 原话）；社区以 hooks + 本地代理补位 | github.com/anthropics/claude-code/issues/29434 |
| GitHub Copilot | 无内容级检测 | **content exclusion**：按路径 glob 把文件排除出上下文（GA 2024-11）——是"排除"不是"检测"，且有已知绕过问题 | docs.github.com/copilot/.../content-exclusion |
| Cursor | 无 | Privacy Mode = 不训练 + 零保留（合同层姿态），不检查内容；`.cursorignore` 同为文件排除 | cursor.com/data-use |
| Cline | 无 | `.clineignore` 官方明言"不是安全或访问控制边界"（将弃用）；有 auto-approve 误读 `.env` 的已知讨论 | docs.cline.bot/.../clineignore |
| Aider / Continue.dev / Windsurf / Zed | 无 | 一律 ignore 文件 / 零保留（ZDR）姿态，无内容检查 | 各官方文档 |
| OpenAI Codex CLI | 无内置；issue #25585 在征集"pre-submit DLP/redaction 层"（开放中） | 另有 2026-04 发布的 **OpenAI Privacy Filter**：1.5B 开源权重 token 分类模型，8 类 PII **含 secrets/credentials**，配套遮蔽工具——**单向**、独立于 Codex、未集成 | openai.com/index/introducing-openai-privacy-filter |
| Amazon Q Developer | 不在 prompt 路径 | 仅 code review 里扫仓库内硬编码密钥 | docs.aws.amazon.com/.../code-reviews |

### 6.2 网关 / DLP 层：到 block 与单向 mask 为止，可逆只存在于原语与社区项目

| 产品 | 检测 | 脱敏语义 | 备注 |
|---|---|---|---|
| AWS Bedrock Guardrails | 50+ PII 实体（含 PASSWORD、AWS_ACCESS_KEY/SECRET_KEY）+ 自定义 regex | **单向** ANONYMIZE：替换为 `{PII_TYPE}` 占位（原值仅存 guardrails trace 供调试） | 不评估 tool-use 输入/输出 |
| Google Sensitive Data Protection (DLP) | 150+ infoType + 自定义规则（密钥需自定义） | **可逆原语金标准**：`deidentifyContent` + cryptoDeterministic（AES-SIV，同输入同 surrogate）或 FFX 格式保持加密 + surrogate infoType；`reidentifyContent` 还原；token 形如 `PHONE_TOKEN(10):9624870384` | 原语层面与本报告 §5 设计同构，但需自行接线到 LLM 请求/响应 |
| Microsoft Presidio | PII recognizer 为主 | encrypt/decrypt 操作器 + DeanonymizeEngine **可逆**；AES-CBC 往返 | 密钥需自定义 recognizer；经 LiteLLM 广泛嵌入 LLM 管线 |
| llm-guard（Protect AI） | Presidio + 自带 SecretsPatternDetector；另有 Secrets 扫描器（detect-secrets 内核） | **Anonymize→Deanonymize 库级可逆往返**：稳定 surrogate 占位符 + 输出端模式匹配还原 | 通用 LLM 应用向，非编码 agent；Secrets 扫描器本身单向 |
| LiteLLM Proxy | Presidio（PII 向，无密钥专用实体） | 默认单向 `<PERSON>` 式遮蔽；`output_parse_pii: True` 可在响应中还原——**已知往返损坏**（issue #6247） | 网关形态 |
| TrueFoundry AI Gateway | **密钥感知度最高的网关**：LLM 输入/输出 + MCP pre/post-tool，覆盖云凭证/AI key/JWT/连接串/高熵上下文 | 单向 Mutate `***REDACTED***`，文档明言"无还原机制" | 证明网关层检测密钥已产品化 |
| GitGuardian ggshield AI hook（2026-04） | **与 Shannon 场景最接近的厂商产品**：挂进 Cursor/Claude Code/Codex/Copilot CLI/VS Code，扫 prompt 提交前 + pre/post-tool-use，500+ 检测器 | **仅阻断 + 通知，明确不做 redaction/tokenization**；fail-open（扫描不可用即放行） | 验证了请求路径是正确的拦截点，同时止步于可逆方案之前 |
| Cloudflare AI Gateway DLP / Portkey / Nightfall / Palo Alto AI Access / Skyhigh / Zscaler / Prompt Security（SentinelOne）/ Lakera / Lasso | 各式 prompt 级 DLP（PII/凭证分类器） | 检测 + 阻断；部分声称 redact（一手文档未证实细节），**无任何还原机制** | 需流量改道网关，面向企业 |

### 6.3 关键结论：可逆 tokenization 在编码 agent 场景是无人区，但可行性已被三重验证

1. **没有任何出货产品**实现"密钥专用检测 + 稳定占位符发给模型 + 输出端还原 + 打包进编码 agent"。Claude Code 官方拒绝、Codex 还在征求意见、GitGuardian 商业产品明确止步于 block。相邻能力（Copilot content exclusion、Cursor privacy mode、各种 ignore 文件）全部是**文件粒度排除**，与内容级检测/替换是两个物种。
2. **可行性三重验证**：
   - 原语成熟——Google DLP 的确定性加密 surrogate + FPE + re-identify 是工业级可逆 tokenization 的完整参考实现；
   - 库级往返存在——llm-guard Anonymize/Deanonymize、Presidio encrypt/decrypt；
   - **端到端社区先例——claude-code-redact（rdx）**：本地代理对 Claude Code/OpenCode 的双向流量做确定性 SHA-256 token（`__RDX_KEY_<hash8>__`）或逼真假值替换，输出端 un-redact 后再交给本地工具，映射表存进程内存。与本报告 §5 方案同构，证明技术路线走得通。
3. **rdx 的两个缺口恰是 Shannon 的机会**：其一，映射表只存进程内存——重启即丢，恢复会话时占位符永久失效且必然击穿缓存（§5.2 的 HMAC 无状态派生同时解决两个问题）；其二，**完全没有 prompt caching 兼容性分析**——§4 的缓存语义约束（确定性序列化、后缀失效、成本模型）没有任何先行者做过，这正是 Shannon"缓存友好"品牌定位能做出的差异化。
4. **市场信号**：OpenAI 以独立开源模型（Privacy Filter）入场做单向遮蔽、GitGuardian 把 AI 编码工具 hook 产品化、Betterleaks 以"AI-agent-ready"立项——"agent 出站脱敏"已是公认需求；但企业网关方案要求流量改道，对个人/小团队用户，编码 agent **进程内**方案是更自然的交付形态，且目前空白。

---

## 7. 结论与建议

### 7.1 对三个问题的直接回答

1. **gitleaks 出站检测：可行，必要性分档。** 检测本身必要（审计档立刻有价值、警告档高价值、默认拦截档暂缓）；gitleaks 的价值在规则语料与上游跟进，引擎应以"规则移植进 Rust"形态落地，不捆绑 Go 二进制、不做每请求子进程。
2. **占位符替换 + 还原：有条件可行，必要性中上。** 成立条件 = §5.2/5.4/5.5 三原则；每请求随机码方案一票否决（缓存 + 模型一致性双杀）。收益最大的场景是 Custom 中转与企业合规。
3. **缓存：可以完全不破坏。** 确定性派生 + 追加式入库替换下，出站字节逐轮稳定，缓存语义与现状等价；需在实现中用测试锁定"同一会话连续请求前缀字节一致"这条不变量。

### 7.2 分阶段建议

- **Phase 0（审计，零风险）**：复用 `with_request_capture` 做只读检测 + 本地命中统计。回答"我们的用户实际会泄露多少密钥、什么类别"，为一切后续决策供数。
- **Phase 1（警告）**：高危类别（私钥/云根凭证，L1+L3）出站前提示，用户可放行单次。
- **Phase 2（窄域可逆替换）**：env 类 + token 类走 HMAC 占位符 + 执行面还原；私钥类保持警告不自动替换。带 F3/F4 兜底告警。
- **Phase 3（策略化）**：`.env`/`.pem` 路径策略、`managed-settings` 式企业下发、IM/远程场景策略对齐。

### 7.3 开放问题

- gitleaks 规则 Go regex → Rust regex 的逐条兼容性差异需要 spike 验证（预计少量规则需降级）。
- 占位符在第三方模型的实际行为保真度（F3 发生率）需要 Phase 2 前做小规模实验。
- Gemini/Bedrock 等第二梯队 provider 的缓存语义实测。
- 还原面在 MultiEdit/三方合并内部的字符串替换次序边界条件。

---

## 8. 参考资料

### 8.1 gitleaks 与检测引擎

- gitleaks 仓库 / 发布 / 默认规则：<https://github.com/gitleaks/gitleaks> · <https://github.com/gitleaks/gitleaks/releases> · <https://github.com/gitleaks/gitleaks/blob/master/config/gitleaks.toml>
- 性能实测（issue #2044）：<https://github.com/gitleaks/gitleaks/issues/2044>
- Betterleaks（继任项目）：<https://thenewstack.io/betterleaks-open-source-secret-scanner/> · <https://news.ycombinator.com/item?id=47353454> · 熵检测 precision/recall 分析 <https://rafter.so/blog/secrets/betterleaks-replaces-gitleaks>
- 误报量级对比（vendor 数据）：<https://securityboulevard.com/2021/02/how-to-reduce-false-positives-while-scanning-for-secrets/>
- Kingfisher（Rust 库，beta）：<https://github.com/mongodb/kingfisher> · <https://www.mongodb.com/company/blog/product-release-announcements/introducing-kingfisher-real-time-secret-detection-validation>
- Nosey Parker：<https://github.com/praetorian-inc/noseyparker>
- TruffleHog 活体验证机制：<https://trufflesecurity.com/blog/how-trufflehog-verifies-secrets>
- detect-secrets：<https://github.com/Yelp/detect-secrets>
- Rust regex crate（RE2 系语义）：<https://docs.rs/regex/latest/regex/>
- **opencode-stranger-danger（gitleaks 规则移植 + agent 上下文扫描先例，<5ms/100KB）**：<https://github.com/gitdamnit/opencode-stranger-danger>

### 8.2 Prompt caching

- Anthropic：<https://platform.claude.com/docs/en/docs/build-with-claude/prompt-caching> · <https://platform.claude.com/docs/en/docs/about-claude/pricing>
- OpenAI：<https://developers.openai.com/api/docs/guides/prompt-caching> · <https://developers.openai.com/api/docs/pricing>
- DeepSeek：<https://api-docs.deepseek.com/guides/kv_cache> · <https://api-docs.deepseek.com/quick_start/pricing>
- Gemini：<https://ai.google.dev/gemini-api/docs/caching> · <https://ai.google.dev/gemini-api/docs/generate-content/caching> · <https://developers.googleblog.com/gemini-2-5-models-now-support-implicit-caching/> · <https://ai.google.dev/gemini-api/docs/pricing>
- 交叉验证：<https://portkey.ai/blog/openais-prompt-caching-a-deep-dive> · <https://gingerlabs.ai/blog/openai-vs-anthropic-prompt-caching> · 前缀稳定性实证 <https://arxiv.org/html/2607.19214v2>

### 8.3 竞品

- Claude Code 上下文脱敏 request（closed not planned）：<https://github.com/anthropics/claude-code/issues/29434>；社区 hook 先例：<https://dev.to/chataclaw/stop-claude-code-from-leaking-your-secrets-introducing-sensitive-canary-826>
- Codex CLI pre-submit DLP request：<https://github.com/openai/codex/issues/25585>；OpenAI Privacy Filter：<https://openai.com/index/introducing-openai-privacy-filter/>
- Copilot content exclusion：<https://docs.github.com/en/copilot/concepts/context/content-exclusion>；Cursor：<https://cursor.com/data-use>；Cline：<https://docs.cline.bot/customization/clineignore>
- **GitGuardian ggshield AI coding tools（阻断式商业先例）**：<https://docs.gitguardian.com/ggshield-docs/integrations/ai-coding-tools/secret-scanning-for-ai-coding-tools>
- Bedrock Guardrails 敏感信息过滤：<https://docs.aws.amazon.com/bedrock/latest/userguide/guardrails-sensitive-filters.html>
- Google Sensitive Data Protection 可逆 de-identify/re-identify：<https://docs.cloud.google.com/sensitive-data-protection/docs/samples/dlp-reidentify-fpe> · <https://docs.cloud.google.com/sensitive-data-protection/docs/samples/dlp-reidentify-deterministic>
- Presidio encrypt/decrypt：<https://presidio.dataprivacystack.org/samples/python/encrypt_decrypt/>；llm-guard Anonymize/Deanonymize：<https://github.com/protectai/llm-guard/blob/main/docs/input_scanners/anonymize.md> · <https://protectai.github.io/llm-guard/output_scanners/deanonymize/>
- LiteLLM PII masking（往返已知损坏）：<https://docs.litellm.ai/docs/proxy/guardrails/pii_masking_v2> · <https://github.com/BerriAI/litellm/issues/6247>
- TrueFoundry secrets detection（单向）：<https://www.truefoundry.com/docs/ai-gateway/secrets-detection>；Cloudflare AI Gateway DLP：<https://developers.cloudflare.com/ai-gateway/features/dlp/>
- **claude-code-redact / rdx（可逆 tokenization 端到端社区先例）**：<https://github.com/paroque28/claude-code-redact>
- 风险框架：OWASP GenAI LLM07：<https://genai.owasp.org/llmrisk/llm07-insecure-plugin-design/>

> 未核实项备忘：DeepSeek 现行定价表（官方页改版，数字来自搜索快照 + 第三方交叉）、Gemini 2.5 Pro 显式缓存存储费率、OpenAI 缓存隔离 scope（org vs project）、Cloudflare DLP 在流量中的改写语义。对外引用前应复核。

---

## 9. 附录：架构决策——三制品拆分蓝图（2026-09-09 评审定稿）

评审结论：采用**三部分拆分**——功能（a）、集成机制（b）、集成契约（c）各自独立演进；并附两条关键约束。本节为 a/b/c 三个制品的立项依据。

### 9.1 决策记录

- **采纳**：三制品拆分。a = 独立 repo 纯函数库；b = 实现 c 契约、包装 a 的插件层 crate；c = shannon-mono 发布的插件接口（内容变换中间件契约）+ 引擎接线。
- **两条约束**：
  1. **b 的"插件"是契约形态，不是进程形态**——shannon 以 cargo git 依赖在进程内消费 b；进程外实现（hook 命令、未来的 daemon/WASM）是同一契约的后续演化形态，不是 v1。现有 hook 命令通道保留给社区/企业的警告与阻断自定义（延迟容忍路径）。
  2. **依赖方向**——契约先行：c 作为薄 trait 包从 shannon-mono 发布（semver 严格），b 在外部 repo 依赖 c 并包装 a；a 保持零 shannon 依赖。
- **否决的备选**：全放 shannon-mono 内部 crate（牺牲复用与规则语料独立版本化）；进程外 daemon 插件从第一天（安全路径引入 IPC 失败模式与密钥过进程边界暴露面）；推迟插件化、仅进程内直连（本方案 C 形态，保留为 c 延期时的降级路径）。

### 9.2 制品划分与依赖图

```
a: secret-guard（独立 repo；纯函数库：无状态、无 I/O、无策略）
        ↑                        ↑
b: secret-guard-plugin       其他消费者（CI CLI / pre-commit / 网关直接用 a）
   （实现 c 的 trait，包装 a）
        ↑
c: shannon-plugin-api（shannon-mono 发布；ContextTransform 契约 + 引擎接线点）
        ↑
shannon-mono 引擎（三个接线点接线 + 策略/主密钥/注册表持久化 + 配置面）
```

| 制品 | 所在 repo | 形态 | 职责 | 版本策略 |
|---|---|---|---|---|
| a secret-guard | 独立 repo | Rust lib + 薄 CLI | 检测、HMAC 占位符派生、还原原语；gitleaks 规则语料 vendor（MIT + NOTICE） | 占位符格式跨版本兼容是其 semver 核心承诺 |
| b secret-guard-plugin | 独立 crate（可与 a 同 repo 分包） | Rust lib | 实现 c 契约；策略解释读宿主传入配置；自身近似无状态 | 跟随 c 的契约版本 |
| c shannon-plugin-api | shannon-mono | 薄 trait crate + 引擎接线 | 内容变换中间件契约；失败语义；不变量测试套件 | 契约变更需重大版本 |
| 宿主接线与状态 | shannon-mono | 引擎内 | 三接线点调用、master key 存储、surrogate→secret 注册表（可重建缓存）、Phase 分档策略、与 `RedactionPolicy` 配置统一 | 产品节奏 |

### 9.3 制品 a：API 草案

```rust
/// 检测：gitleaks 规则移植 + 关键词预过滤 + 熵检查（构建期编译全部规则，不兼容规则跳过并报告）
pub struct Finding { rule_id: String, class: SecretClass, span: Range<usize>, /* ... */ }
pub fn scan(text: &str, cfg: &ScanConfig) -> Vec<Finding>;

/// 占位符：HMAC-SHA256(master_key, secret) 截断 48–64 bit，按 SecretShape 做格式保持
/// 格式版本化，如 `SG1:<base32>`——写进历史即永久承诺可识别
pub fn surrogate(secret: &[u8], master_key: &[u8], shape: SecretShape) -> String;

/// 还原：精确匹配反查 + 编辑距离 ≤2 模糊修复建议；返回未解析的密钥形状串（F3/F4 兜底告警输入）
pub fn restore(text: &str, pairs: &[(String, String)]) -> RestoreOutcome;
```

设计不变量：无状态（派生确定性）；注册表（surrogate→secret）是**可重建缓存**而非权威状态——丢失可从本地密钥源（env 快照、`redaction.toml` 声明值、路径策略命中文件）重扫重建；权威持久状态只有 master key。修正 rdx 先例的内存态映射缺陷。

### 9.4 制品 c：契约草案与不变量

```rust
pub trait ContextTransform: Send + Sync {
    /// 接线点 1：内容入库（用户消息 / 工具结果 / 注入文件 / compaction 输入）
    fn transform_ingest(&self, block: &mut ContentBlock, ctx: &IngestCtx) -> TransformAction;
    /// 接线点 2：工具参数执行前还原（Write/Edit/MultiEdit/Bash/Notebook 完整参数，天然规避流式撕裂）
    fn restore_tool_args(&self, tool: &str, args: &mut serde_json::Value) -> RestoreAction;
    /// 接线点 3：显示面还原（渲染层/IM 推送）；还原值绝不回写历史
    fn restore_display(&self, text: &mut String) -> RestoreAction;
    /// 只读 wire 审计（Phase 0 载荷，量化入库层漏检率）
    fn audit_wire(&self, wire: &serde_json::Value) -> Vec<Finding>;
}
/// 失败语义显式化：Closed = 插件不可用即阻断并明确报错；Open = 放行 + 记录（对齐 GitGuardian fail-open 先例）
pub enum FailMode { Closed, Open }
```

**契约级一致性测试（任何实现必须通过，含跨版本矩阵）**：
- I1 同一会话连续请求的前缀字节一致（缓存安全，§4 的落地形式）；
- I2 改写结果进入对话历史，还原值绝不回写历史（单向性，§5.5）；
- I3 变换幂等：对已替换文本再 scan 无新命中（HMAC 派生 + 占位符不命中检测规则的字符集设计）；
- I4 FailMode 语义：Closed 下插件不可用必须阻断；Open 下必须放行且留痕。

### 9.5 hook 事件覆盖表（Phase 0/1 即刻可用路径）

| 内容入口 | 现有 hook 事件（已验证存在） | 覆盖情况 |
|---|---|---|
| 用户输入 | `UserPromptSubmit` | ✓ 可检测/阻断/modify |
| 工具参数（执行前，还原点） | `PreToolUse` | ✓ 检测/阻断；modify 需 spike |
| 工具结果（入库前） | `PostToolUse` | ✓ 检测/阻断；modify 需 spike |
| 系统/注入文件构建、compaction 输入、wire 审计 | 无对应事件 | c 要新增的接线能力 |

已验证：hook 系统为命令式（JSON stdin/stdout），具备 allow/deny/**modify** 语义（`hooks/manager.rs` 的 `modified_input`/`modified_output`、deny 覆盖 modify 解析）。**待 spike**：PreToolUse/PostToolUse 的 modify 改写是否落入对话历史且逐轮字节稳定（即既有接口上的 I1/I2）——决定 Phase 0/1 能否完全走 hook 形态，还是直接从 c 接线点起步。

### 9.6 分阶段实施映射

| 阶段 | 载荷 | 制品依赖 | 接口路径 |
|---|---|---|---|
| Phase 0 审计 | 只读检测 + 本地命中/误报统计 | a | c 的 `audit_wire` + ingest 检测（首个接口载荷，零风险验证契约设计） |
| Phase 1 警告/阻断 | 高危类别（私钥/云根凭证）提醒 | a | hook 通道（社区可用）或 c 接线点 |
| Phase 2 可逆替换 | env/token 类 HMAC 占位符 + 执行面还原 | a + b + c 全链路 | c 契约 + I1–I4 测试锁定 |
| Phase 3 策略化 | 路径策略、企业下发、IM/远程对齐 | 宿主侧 | 配置统一（`redaction.toml` 一份规则、日志与出站两个消费面） |

### 9.7 风险与机制

- **版本漂移**（shannon ↔ c ↔ b ↔ a）：lockfile 钉版本 + I1–I4 的跨版本矩阵 CI；占位符格式 `SG1` 版本化承诺。
- **规则语料自持**：gitleaks 已 feature-frozen，建立对 Betterleaks/Kingfisher 规则目录的定期同步流程；构建期逐条编译校验，不兼容规则显式报告降级。
- **既有 hook modify 语义未验证**：§9.5 spike 先行，结论决定 Phase 0/1 的接口路径。
- **多 repo 工作流成本**：以 c 的契约测试套件作为两个 repo 的共同 CI 门禁，接口漂移在 PR 阶段暴露。
