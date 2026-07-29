# Provider/Model 支持与百万上下文改进方案

> 状态:**待审核** · 日期:2026-07-30 · 作者:ericdong / SDD 会话
> 关联:ADR-0005(统一 provider/model/credential 管理)、Phase 4 已闭环 tier read-back,本文档面向 **Phase 5(动态 catalog)+ 百万上下文补全**。
> 适用范围:Shannon Code(CLI/REPL)与 Shannon Desktop —— 二者共用 `shannon-core::MODEL_CATALOG`(`desktop/Cargo.toml` 依赖 `shannon-core .workspace = true`),故本方案对两者同时生效。

---

## 一、执行摘要

Shannon 的 provider 支持面广(`LlmProvider` enum 24 个 provider),但**模型 catalog 严重滞后且只覆盖 11 个 provider**,导致两个用户可感知的痛点:

1. **`/model` 切换时找不到模型** —— `/provider <x>` 切到 catalog 无模型的 provider(如 xAI/Grok、Perplexity、SiliconFlow、OpenRouter 等),默认 model 直接变成 `"unknown"`(config.rs:154-158);`/model` picker 也为空。
2. **百万上下文不可用** —— Shannon **没有任何 1M beta header 注入机制**(全仓零 `context-1m`/`anthropic-beta` 引用),而 Anthropic Claude 的 1M context 需 `context-1m-2025-08-07` header;竞品 OpenCode issue #12452 证明 header 没正确发送会**静默回退到 200K**。

好消息:技术基础已就绪 —— adapter 的 `extra_headers` 机制(client.rs:858)可直接注入 beta header;`resolve_model_arg` 已支持 catalog 外 model id(手动 `/model provider/xxx` 可绕过)。本方案聚焦把这两条"半成品通路"补成完整能力。

**建议优先级**:Phase A(catalog 补全,P0)→ Phase B(1M header,P1)→ Phase C(picker 手动输入,P1)→ Phase D(models.dev 动态,P2)→ Phase E(context 准确性,P2)。

---

## 二、现状盘点

### 2.1 Provider 支持矩阵

`LlmProvider` enum(`shannon-engine/src/api/types.rs:50`)定义 **24 个 provider**。`MODEL_CATALOG`(`shannon-core/src/model_registry.rs:125`)实际收录模型的有 **11 个 provider / 44 个模型**。

| Provider | enum | catalog 有模型 | 模型数 | 百万上下文模型 | 备注 |
|----------|:----:|:----:|:----:|:----:|------|
| Anthropic | ✅ | ✅ | 4 | ❌(catalog 全标 200K) | **缺 4.5/4.6;1M 需 header** |
| OpenAI | ✅ | ✅ | 4 | ❌ | **缺 GPT-5/5.5、o4** |
| Gemini | ✅ | ✅ | 2 | ✅ 2 | 2.5-pro/flash 原生 1M |
| DeepSeek | ✅ | ✅ | 4 | ✅ 2 | V4-flash/pro 1M |
| Zhipu (GLM) | ✅ | ✅ | 9 | ✅ 1 | glm-4-long 1M;5 系列 198K |
| ZhipuInternational | ✅ | ✅ | 6 | ✅ 1 | glm-4-long-intl |
| Moonshot (Kimi) | ✅ | ✅ | 5 | ❌ | K2.6/K2.5 为 256K |
| Mistral | ✅ | ✅ | 2 | ❌ | |
| DashScope (Qwen) | ✅ | ✅ | 3 | ✅ 3 | 3.7-max/3.6 系列 1M |
| Minimax | ✅ | ✅ | 3 | ✅ 2 | M2.7 系列 1M |
| Groq | ✅ | ✅ | 2 | ❌ | |
| **xAI (Grok)** | ✅ | ❌ | **0** | — | **切过去找不到任何模型** |
| **SiliconFlow** | ✅ | ❌ | 0 | — | 同上 |
| **Perplexity** | ✅ | ❌ | 0 | — | 同上 |
| **OpenRouter** | ✅ | ❌ | 0 | — | 聚合型,合理无静态列表 |
| **Together** | ✅ | ❌ | 0 | — | |
| **Fireworks** | ✅ | ❌ | 0 | — | |
| **Cohere** | ✅ | ❌ | 0 | — | |
| **Bedrock** | ✅ | ❌ | 0 | — | 应复用 Anthropic/OpenAI 模型 |
| **Azure** | ✅ | ❌ | 0 | — | 应复用 OpenAI 模型 |
| **Ai21** | ✅ | ❌ | 0 | — | |
| **Cloudflare** | ✅ | ❌ | 0 | — | |
| **Replicate** | ✅ | ❌ | 0 | — | |
| Ollama | ✅ | ❌ | 0 | — | 本地动态,合理 |
| Custom | ✅ | ❌ | 0 | — | 用户自定义,合理 |

**结论**:12 个"有 provider 通道、无内置模型"中,**xAI、SiliconFlow、Perplexity、Together、Fireworks、Cohere、Bedrock、Azure、Ai21、Cloudflare、Replicate** 是真实缺失(用户配了 key 却选不到模型);Ollama/Custom/OpenRouter 属动态类型,应走另一条路径(见 Phase C/D)。

### 2.2 百万上下文现状

catalog 中 `context_window: 1_000_000` 的模型共 **11 个**:Gemini ×2、DeepSeek V4 ×2、GLM-4-long(+intl)、Qwen 3.x ×3、Minimax M2.7 ×2。

- **非 Anthropic provider 的 1M**:模型原生能力,API 直接支持,**理论可用**(前提是 provider 请求格式正确,未验证 DeepSeek/Qwen/Minimax 的 1M 是否需额外参数 —— 列为待核实)。
- **Anthropic 的 1M**:**完全缺失**。catalog 把所有 Claude 标 200K,且无 header 注入,即使用户用 Sonnet 4.5/4.6/Opus 4.6 也拿不到 1M。
- **`context_window_for` 的硬 fallback = 200K**(model_registry.rs:776):任何未收录模型被当作 200K,导致新模型 context 显示失真(如 Grok 4、GPT-5 实际 context 更大却显示 200K)。

### 2.3 Desktop

`shannon-mono/desktop` 是 Tauri app,**通过 workspace 依赖复用 `shannon-core::MODEL_CATALOG`**(已核实 `desktop/Cargo.toml:18`)。Desktop 自己只管 `~/.shannon/desktop/providers.json` 的连接管理(provider_kind、key),**模型清单与 Code 完全同源**。因此本方案对 Desktop 同等生效,无需双份维护。

> 注:`CLAUDE.md` 称 Desktop 已 extract 到独立 repo `../shannon-desktop`,但本地实际位于 `shannon-mono/desktop`(独立 repo 不存在)。建议同步修正 CLAUDE.md 该描述。

---

## 三、问题诊断

| ID | 问题 | 根因(代码位置) | 用户影响 |
|----|------|----------------|----------|
| **P1** | catalog-empty provider 选不到模型 | `config.rs:154` `models_for_provider().first().unwrap_or("unknown")`;picker 同源 | `/provider xai` 后默认 model = `"unknown"`,`/model` picker 空 |
| **P2** | catalog 模型滞后(缺 2026 新模型) | `MODEL_CATALOG` 手工维护 | Claude 4.5/4.6、GPT-5、Grok 4 等用 alias/picker 选不到 |
| **P3** | 无 1M beta header 机制 | 全仓无 `context-1m`/`anthropic-beta`;adapter 有 `extra_headers` 但无"模型→header"映射 | Anthropic 百万上下文不可用;潜在静默 200K 回退 |
| **P4** | context_window 不准 | `context_window_for` 未收录 → 硬 fallback 200K | 新模型/手动输入模型显示 200K |
| **P5** | 静态 catalog 维护成本高 | 无动态源;ADR Phase 5(models.dev)未实施 | 每次新模型发布都要改代码、发版 |

---

## 四、竞品对标

| 维度 | Claude Code | OpenAI Codex CLI | OpenCode | Cursor | **Shannon 现状** |
|------|-------------|------------------|----------|--------|------------------|
| 1M context | Opus 4.6 GA;Sonnet 4/4.5 走 beta header(2026-04-30 退役,迁移 4.6) | ~256K | 需正确发 header,否则静默回退(issue #12452) | 用 Claude 模型可达 1M | ❌ 无 header 机制 |
| 模型来源 | 自家 + 部分 | 自家 | **models.dev 动态**(75+ provider,OpenCode 团队维护) | 多家 | 静态硬编码 |
| Picker 空 provider 处理 | — | — | runtime 可用但 picker 空(Cline #11747 同类痛点) | — | 直接 `"unknown"` |
| context 显示 | 准确 | 准确 | 准确 | 准确 | 未收录 → 200K |

**关键启示**:
- OpenCode 用 **models.dev** 解决了"模型滞后 + 多 provider"问题 —— Shannon ADR-0005 Phase 5 已规划此方向,本文档 Phase D 落地。
- "1M header 静默回退"是已知陷阱,Shannon 必须在 Phase B 显式处理(发 header + 校验生效,而非假设)。
- Anthropic 1M beta 对 Sonnet 4/4.5 **2026-04-30 退役** → 方案应聚焦 **Sonnet 4.6 / Opus 4.6(1M GA,免 header)**,兼顾对 4.5 的 header 支持(过渡期)。

---

## 五、改进方案(分阶段)

### Phase A — Catalog 补全(P0,~0.5 天)

**目标**:消除 P1/P2 的即时痛点 —— 让所有"有通道"的主流 provider 至少有一个代表模型,并补全 2026 主流新模型。

**改动**:`crates/shannon-core/src/model_registry.rs` 的 `MODEL_CATALOG` 增补条目(纯数据,无逻辑改动)。

**增补清单**(model id 以各 provider 官方 API 文档为准,实施时核对):

- **Anthropic**:`claude-sonnet-4-5`(1M,需 header)、`claude-sonnet-4-6`(1M GA)、`claude-opus-4-1`、`claude-opus-4-5`、`claude-opus-4-6`(1M GA)
- **OpenAI**:`gpt-5`、`gpt-5.1`、`o4-mini`(及 o4 系列按官方名)
- **xAI**:`grok-4`、`grok-4-fast`(当前 catalog 0 个 → 补上即解决 xai "unknown")
- **SiliconFlow**:补 1-2 个代表模型(如 `Qwen/Qwen3-235B-A22B`,按平台实际 model id)
- **Perplexity**:`sonar-pro`、`sonar-reasoning-pro`
- **Together / Fireworks / Cohere / Ai21**:各补 1 个代表模型
- **Bedrock / Azure**:不新增 catalog 条目,改在 Phase C 让其复用 OpenAI/Anthropic 模型(通过 qualified 输入 `/model bedrock/anthropic.claude-...`)

**验收**:`/provider xai` 后默认 model 不再是 `"unknown"`;`/model grok` alias 解析成功;每个非动态 provider 在 picker 至少有一项。

**风险**:模型 id 笔误 → 实施时对每个新增 id 跑一次 catalog 查询测试(参照现有 `tier_label_classifies_*` 测试模式)。

---

### Phase B — 1M 百万上下文 header 机制(P1,~1-1.5 天)

**目标**:解决 P3 —— 让 Anthropic 1M 真正可用,并为所有 provider 的 1M 提供一致的声明与注入。

**设计**:

1. **`ModelInfo` 增字段**(`model_registry.rs:74`):
   ```rust
   /// Beta headers required to unlock this model's full context_window
   /// (e.g. Anthropic 1M: "context-1m-2025-08-07"). Empty = none needed.
   pub beta_headers: &'static [&'static str],
   ```
   每个模型 id 加 `beta_headers: &[]`(默认空),1M 需 header 的模型显式声明。Sonnet 4.6/Opus 4.6(1M GA)留空。

2. **adapter 自动注入**(`shannon-engine/src/api/client.rs` 请求构建处,~line 177 `extra_headers` 循环附近):
   - `LlmClient` 发请求前,查当前 model 的 `beta_headers`,合并进 `extra_headers`(`anthropic-beta` key)。
   - 已有 `extra_headers` 机制(client.rs:858 `insert` + 177 循环),**无需改架构**,只在 model 切换时补 header。

3. **生效校验**(防静默回退,呼应 OpenCode #12452):
   - 首次用 1M 模型时,在响应里探测是否真正启用(Anthropic 返回的 usage/限流头可佐证);若探测到回退,在 UI 提示"1M 未生效,请检查 API Tier"(Anthropic 1M 需 Tier 4)。
   - 最低限度:文档化"1M 需 Tier 4",picker 里对 1M 模型标注。

4. **`/model` 切换显示已有 1M 标签**(config.rs:77 `{}M`)—— 复用,无需改。

**验收**:`/model claude-sonnet-4-6` 后请求带 `anthropic-beta: context-1m-2025-08-07`(对需 header 的版本),context 显示 1M;新增测试 mockito 校验 header 发出。

**风险**:
- Anthropic 1M 需 **Tier 4 API** 账户 → 非 Tier 4 用户发 header 会被拒。方案:header 只在该模型声明了 `beta_headers` 时发,且错误可读。
- Sonnet 4/4.5 的 1M beta **2026-04-30 退役** → catalog 应优先 4.6(GA),4.5 标注"过渡期"。

---

### Phase C — Picker 手动输入逃生口(P1,~0.5 天)

**目标**:缓解 P1 对动态型 provider(Ollama/OpenRouter/Custom/Bedrock/Azure)的支持 —— 允许在 picker 里手动输入 catalog 外的 model id。

**设计**:
- `ModelPickerWidget`(`shannon-ui/src/widgets/select.rs`)在 provider 的 catalog 列表为空时,显示一个"输入自定义 model id"输入框,回车后走 `resolve_model_arg` 的 qualified 路径(已支持)。
- `/provider` 切到 catalog-empty provider 时,默认 model 从 `"unknown"` 改为**保留上一次 model id**(或提示"请 `/model <provider>/<id>` 指定"),不再写 `"unknown"` 到 preferences。

**验收**:Ollama/OpenRouter 用户能 `/model openrouter/anthropic/claude-sonnet-4-6` 直接用;切到 xai 不再出现 `"unknown"`。

**关联**:Phase A 补了静态代表模型,Phase C 兜底动态/长尾模型,两者互补。

---

### Phase D — models.dev 动态 catalog(P2,~2-3 天,= ADR-0005 Phase 5)

**目标**:根治 P2/P5 —— 用 models.dev 动态拉取,告别手工维护。

**设计**(沿用 ADR-0005 Phase 5 scope):
- 启动时(或 `/model refresh`)从 models.dev 拉取,缓存到 `~/.shannon/cache/models-dev.json`(带 TTL,如 24h)。
- **离线 fallback**:`MODEL_CATALOG` 静态表作为缓存未命中时的兜底(headless/CI 不断网不崩,呼应 CLAUDE.md "must not break headless/CI")。
- 动态结果与静态表 merge:静态表的 `beta_headers`(Phase B)等 Shannon 专属元数据保留,models.dev 补 pricing/context/provider。
- 配置项 `enabled_providers` / `disabled_providers` allowlist(ADR Phase 5 scope)。

**验收**:`/model refresh` 后新模型出现;断网仍可用静态表;`just ci` 不需要网络。

**风险**:models.dev schema 变更 → 版本 pin + schema 校验;国内网络可达性 → 失败静默回退静态表。

---

### Phase E — context_window 准确性(P2,~0.5 天)

**目标**:解决 P4 —— 去掉"未收录 = 200K"的误导。

**设计**:
- `context_window_for`(model_registry.rs:757)未收录时,返回 `Option<usize>`(或 0 表示未知),调用方显示"未知/按 provider 默认"而非硬编 200K。
- 对 `engine.pre_resolve_context()` 已能动态解析的路径(config.rs:70),优先信任动态值。
- 1M 模型(Phase B)的 context 由实际 header 生效后确定,而非静态猜。

**验收**:手动 `/model xai/grok-4` 显示"context: 未知(按 provider)"而非"200K";已知模型仍准确。

---

## 六、风险与设计决策(需 sign-off)

1. **Anthropic 1M 与 Tier 4 绑定**:1M 需 Tier 4 账户,普通用户发 header 会被拒。决策点:是否对非声明模型也允许用户手动 `--context-1m`?建议**否**(保持模型驱动),Tier 限制写进文档。
2. **Bedrock/Azure 模型来源**:它们是 OpenAI/Anthropic 模型的托管通道。决策:走 Phase C 的 qualified 输入(`/model bedrock/<id>`)而非给它们单独 catalog 条目。
3. **models.dev 国内可达性**:Phase D 失败必须静默回退,不能阻塞启动。
4. **Desktop 同步**:所有 catalog 改动自动惠及 Desktop(workspace 依赖);若 Desktop 前端有硬编码 model 列表需一并清理(Phase A 实施时 grep 确认)。
5. **catalog model id 准确性**:各 provider 命名不一(如 `claude-sonnet-4-5-20250929` vs 简写),实施时以官方 `/v1/models` 或文档为准,加 catalog 测试防回归。

---

## 七、建议优先级(供审核)

| Phase | 优先级 | 工作量 | 依赖 | 用户可感知收益 |
|-------|:------:|:------:|------|----------------|
| **A. Catalog 补全** | **P0** | 0.5d | 无 | 立即消除 "unknown" + 选到 2026 新模型 |
| **B. 1M header** | **P1** | 1-1.5d | A(部分) | Anthropic 百万上下文可用 |
| **C. Picker 手动输入** | **P1** | 0.5d | 无 | 动态/长尾 provider 可用 |
| **D. models.dev 动态** | P2 | 2-3d | A(静态兜底) | 根治模型滞后(= ADR Phase 5) |
| **E. context 准确性** | P2 | 0.5d | B | 显示不再误导 |

**最小可行集合(若资源受限)**:A + B + C ≈ 2-2.5 天,覆盖全部用户可感知痛点;D/E 作为根治性后续。

---

## 八、待核实清单(实施前确认)

- [ ] 各 provider 2026 最新 model id 官方命名(Anthropic Sonnet 4.6/Opus 4.6、OpenAI GPT-5.x、xAI Grok 4.x)
- [ ] DeepSeek/Qwen/Minimax 的 1M 是否需额外请求参数(还是纯模型能力)
- [ ] Anthropic 1M 的 Tier 4 门槛当前是否仍有效(Sonnet 4.6/Opus 4.6 已 GA,可能放宽)
- [ ] Desktop 前端是否有独立硬编码 model 列表需同步清理
- [ ] models.dev API schema 与速率限制

---

### 附:关键代码位置索引

| 文件 | 行 | 说明 |
|------|----|------|
| `shannon-engine/src/api/types.rs` | 50 | `LlmProvider` enum(24 provider) |
| `shannon-core/src/model_registry.rs` | 74 | `ModelInfo` struct(Phase B 加 `beta_headers`) |
| `shannon-core/src/model_registry.rs` | 125 | `MODEL_CATALOG`(Phase A 增补) |
| `shannon-core/src/model_registry.rs` | 641 | `models_for_provider`(P1 根源) |
| `shannon-core/src/model_registry.rs` | 757 | `context_window_for`(Phase E 改) |
| `shannon-ui/src/repl/commands/config.rs` | 36 | `handle_model` |
| `shannon-ui/src/repl/commands/config.rs` | 154 | `/provider` 默认 model `"unknown"`(P1) |
| `shannon-engine/src/api/client.rs` | 177/858 | `extra_headers` 注入(Phase B 落点) |
