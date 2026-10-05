# Provider/Model 配置与切换 R2 复审 —— 修复验证 · 竞品对照 · 改进方案

- 日期:2026-10-05 · **状态:v1.2 可执行稿(v1.1 按对抗性审查修订,见配套 [redteam 文档](2026-10-05-provider-model-config-review-r2-redteam.md);v1.2 决策①-⑪用户已全部拍板(①-⑨按建议,⑩⑪按行内默认补录),裁定记录见 §6 末尾——批准后排期稿另出)**
- 基线:`dev` @ `d9cc1428b`
- 前作:[2026-09-29 provider/model 配置与支持深度评审](2026-09-29-provider-model-config-review.md)(P0×6/P1×12/P2×24)与 [2026-09-30 后续任务路线图](../plans/2026-09-30-provider-followups-roadmap.md)(R1-R4)。两份文档所列批次(PR #154/#158/#163/#165/#171 及 R5 `fa5436f8a`)已全部合入 dev。
- 范围:Shannon Desktop(Tauri + React 前端 + Rust 命令层)的模型/provider 配置与切换——功能、UI、使用流程、user journey;引擎/网关配置层仅在与桌面旅程交汇处展开。TUI/CLI 只在旧账核销涉及。
- 方法:两路代码深查(桌面 UI 旅程走查 + 引擎/网关/命令桥接层审查,全部 file:line 取证)+ 主会话对关键论断逐条回仓抽查复核(清单见 §7)+ 两路竞品官方资料调研(2026-10 时点)。修复验证不看「代码存在」看「链路闭环」:前端交互 → Tauri 命令 → 引擎执行 → 状态回显 → i18n,任何一环缺失记 PARTIAL。

---

## 0. TL;DR

**09-29 评审的修复不是纸面工程**:P0×6 闭环(P0-1 的回落声明落在日志级 `unified_config.rs:640`,弱于路线图"对话内显式声明"的验收字面,见 §2;其余五项全闭环)、P1×12 中 8 项 FIXED、P2×6 中 3 项 FIXED。R2-R5 批次的实现质量普遍高于修法底线——failover 链(类型化资格判定+双上限+事件留痕+无嵌套预算)、多 key 轮换(先于 failover、豁免会话钉)、会话覆盖 sidecar(原子写+prune+重启恢复)、providers.toml v2(非法元数据整文件拒载+JSON Schema)都是真闭环。**引擎层能力面已达到或超过多数竞品**。

但本轮复审发现:修复批次在「最后一公里」上留下了**五项高优先问题**(初稿判 P0"信任损伤",对抗性审查证明无一满足 journey R2 的说谎 UI 定义,整体降级 P1——降级不影响任何修复的排期,见 redteam §1.1):

1. **聊天失败路径的错误分类只有 auth/other 二分类**——402 配额耗尽、429 限流、403 全炸成裸红条,无深链无建议;而探活路径早已有 5 类分类(TestConnectionResult)没被复用。引擎的 typed 错误被 Display 成字符串再靠文本匹配还原,链路脆弱。
2. **会话 pin 静默关闭 failover**(`suppress_failover = true` 零 UI 提示)——用户配了 fallback 链,pin 过会话的会话却不降级;ProviderKeysPanel 的轮换说明宣称「自动换 key」(确实开),两个自动恢复机制一开一关,用户无从分辨。
3. **dynamic 徽章仍是死 UI**:R2 修活了 tier 徽章,`dynamic: None` 却仍硬编码且无任何注释承认其是 no-op;相邻的 honest-metadata 注释(`commands_chat.rs:119`)描述的是 tier 行为,反而给读者相反印象——死徽章属实,"注释说谎"指控经红队核查撤回(redteam §1.3),降 P2。
4. **Ollama 探测的文档注释与实现矛盾**:注释写 "OLLAMA_HOST **or default localhost:11434**",实现只查 env 变量——最典型的「无 key 本地用户」画像在 Welcome 永远检测不到。
5. **quick-fill 五个过时模型 id**(`gpt-4.1-mini`/`glm-4-plus`/`moonshot-v1-8k`/`abab6.5s-chat`/`llama3.2`)——新用户第一眼看到的是 2024 年的目录,而 fetch models 已能拉真实列表却没回填。

功能半程类:**per-model 元数据桌面编辑器未落**(R2-4 只做了引擎+CLI 半程,桌面 0 入口)、**fetch models 结果是临时 datalist 不能固化为 provider 模型仓**、**declared max_output 是无消费者的死字段**、**能力门控只有 vision**(tool-use 无门控、拒绝文案指向桌面不存在的 `/model`)、**导出的 redact 不覆盖 extra_headers**(用户常在此放鉴权头)。

竞品格局(§4-§5):Shannon 的引擎广度(tier、定价三层、探活、脱敏、failover 事件流)仍是差异化优势;桌面上「会话级覆盖+设为默认」心智已与 Claude Code/Codex 对齐。剩余差距收敛为:**模型仓(verified models)与 per-model 元数据编辑**(BYOK 类全员标配)、**聊天内错误恢复建议**(Gemini CLI/ChatGPT 式「配额→换 key→换模型」链)、**切换面收敛**(竞品普遍一个 picker 内嵌层级)、**本地模型零配置发现**(Jan/Open WebUI 式自动探测)。

改进方案(§6):S1 止血批(信任修复,单快速 PR)+ S2 模型仓与元数据批(table stakes 收尾)+ S3 切换体验收敛批 + S4 打磨簇,并给出每批验收门槛与明确不做清单。

---

## 1. 现状盘点(2026-10-05)

### 1.1 配置架构(引擎层)

- **六层合并顺序(低→高)**:`builtin → ~/.shannon/config.toml → .shannon.toml → SHANNON_* env → ~/.shannon/providers.toml(connected) → CLI overlay`(`crates/shannon-core/src/unified_config.rs:462-475`,出处视图 `config_dump.rs:4-17`)。R5-5「优先级钉」改的不是文件层序,而是**查询时模型解析优先级**:
  `会话覆盖 > Plan/Act 档位 > 全局默认`;无人值守路径(goal/batch/routine/dream/skill loop)三者都不吃、只读全局(`desktop/src/commands_chat.rs:330-356` 表格注释,`unattended_paths_pin_global_config` 测试 :1264-1337 钉死)。
- **providers.toml v2**:多 profile(`active_profile` + `profiles`)+ 每 profile `active_target/providers/auxiliary/credential_scope`(`crates/shannon-types/src/provider_config.rs:287-320`);`ProviderProfile` 支持 kind/base_url/models_url/CredentialRef(永不存明文)/extra_headers/fallback_models/quirks/tiers/`models: Vec<ModelSpec>`(:215-239)。**R2-4 per-model 元数据** `ModelSpec { id, context_window, max_output, cost_per_m_input/output, capabilities }`(:157-180)带语义校验(负价/零上限/重复 id 拒载,`provider_config_store.rs:154-155,270`)。消费链四路全部接通:计费(`find_pricing` 第 1 优先,`query_engine/types.rs:410-441`)、compaction 预算(`resolve_max_context_tokens`)、tier 展示标签(`model_registry.rs:476-488`)、vision 门控(`agent_loop.rs:93-164`)。
- **会话级覆盖**:composer chip → `set_session_model`(`commands_chat.rs:519-581`,写时校验 provider 在当前 profile 名册内)→ `SessionState.model_override` → R5-1 sidecar `~/.shannon/desktop/session-model-overrides.json` 原子持久化(`session_override_store.rs:62-73,186-209`),启动 hydrate、坏文件降级为空。查询时全量重解析 provider/base_url/key(不是只换模型字符串),覆盖失效( provider 被删)降级全局+warn(:372-379)。**pin 同时置 `suppress_failover = true`**(:502-506,「钉住的目标不许被引擎二次猜测」,key 轮换豁免于此钉)。
- **failover(R3-1)**:profile `fallback_models` → `resolve_failover_chain`(`unified_config.rs:771-858`;裸 id=同 provider 换模型,`provider/model`=切到同 profile 名册内该 provider;未连接跳过+warn;上限 3)。触发=仅 429 与 5xx(401 永不);主目标先用满重试预算(默认 3 次退避),每个 hop 再拿一次完整预算,跳入时清链防嵌套(`client.rs:1420-1444`);每跳先发 `RetryNotice{Failover}` 进事件流(retry.rs:32-58),桌面渲染为弱化系统行(`MessageArea.tsx:356-367`,i18n 链路完整)。
- **多 key 轮换(R4-3)**:`Credential { value(slot 0 活跃), extra_values }`,激活=物理换位(`credential_manager.rs:60-100,399-415`);轮换在 failover **之前**、对同 provider 同模型换 key,触发=401/存活 429/auth 味 403,每 key 一次完整重试预算,每轮先发 `KeyRotation{index,total}` 通知(`client.rs:1266-1321`)。
- **导出/导入(R4-2)**:`shannon-providers-export/v1` TOML 信封,默认不含明文键(凭证只旅行引用),0600;`--redact` 替换 credential 表;导入**硬拒** redacted 文件+冲突检测(`commands_providers.rs:1137-1367`)。
- **config --explain(R4-4a)**:复用 dump provenance,「哪层赢+在哪改」,secret 键给 A1 指引(`commands_config_explain.rs`)。**CLI-only**。
- **模型目录三层**:静态 catalog **77 条**(OpenRouter 6/Bedrock 3/ZhipuCoding 各 2 已补,`gap_providers_have_catalog_entries` 钉测;**Azure/Cloudflare/Replicate 仍 0**)+ models.dev overlay(24h 缓存,桌面刷新按钮已接 R2-2)+ Ollama 探测(`ollama show` 取真实 num_ctx 已修,`model_registry.rs:279-366`)。

### 1.2 桌面旅程地图

**首跑(Welcome)**:门控改用 `get_provider_status` 快照(`Layout.tsx:126-129`,P0-2 修复)→ Step 0 任务卡(必选,`Welcome.tsx:47`)→ Step 1 ModelStep 唯一 CTA 打开规范 `AddProviderModal`(`ModelStep.tsx:55-75`)。mount 时一次性 `detectProviderFromEnv()`(3 家 key + `OLLAMA_HOST`,命中可跳过手填,`Welcome.tsx:77-94`)。保存写 providers.toml SSOT(desktop 双写已删,`commands_config.rs:1965-1997`)→ setActiveProvider → DoneStep(工作目录/迁移向导)。Skip 永远可用,skip 后空画布有 provider CTA 兜底(`WelcomeState.tsx:50-65`)。

**AddProviderModal**(Welcome 与 Settings 共用):quick-fill 8 家芯片 → label/kind/baseUrl/apiKey/model 表单 → **表单内 Test connection**(`testProviderCredentials`,分类结果+往返延迟,注释明言「save ≠ test」)→ **Fetch models** 拉 `/models` 回填 datalist(空/失败有内联态)→ Advanced(extra headers/default_max_tokens/per-tier/fallback)→ dirty 守卫(Esc/背板需确认)。

**Settings → Models**(`ModelsSettings.tsx` 自上而下):性能策略 pills → Active Model 卡+快速切换器 → **PhaseTierSection**(R3-3 plan/act 两档+实时解析预览+优先级文案,`PhaseTierSection.tsx:30-138`)→ **ProfilesSection**(R3-2 列表/切换/创建/改名/删除,空 profile 确认、最后一个禁删,`commands_profiles.rs:136-341`)→ **ProvidersSection**(卡片 CRUD/Test 用存储凭证/Test-all 并发+结果面板/可见性/激活 prompt-cache 警告)→ **ProviderKeysPanel**(R4-3 轮换列表:add/activate/remove,slot 0=active,删除语义完整)→ 模型目录(tier 徽章**已活**;dynamic 徽章**仍死**;刷新按钮四态;价格/context 行)→ 温度/max_tokens。

**聊天**:Header 在 /chat 路由不渲染模型选择器(「composer chip 独占 /chat」约定,`Header.tsx:341`),只渲染 ExecutionModeSwitcher(审批档位)与 PhaseTierSwitcher;非 chat 路由渲染 HeaderModelSelector(双写 model+provider,行内仅 context)。Composer chip(R2-1):会话覆盖优先显示,`· session` 后缀+hover 说明;菜单含 Set as default(晋级全局)/Reset to default(清覆盖);picker 元信息=vision 点+`200k · $3.00/$15.00`(未知渲染 "—");**effort 仍折叠在模型下拉内**(`name · High` 文案,P2-19 残留)。

**错误路径**:401/403 文本匹配 → `error_kind='auth'` → 专属横幅「API key rejected by {provider}」+ Update key 深链 `/settings/models`(`events.rs:110-123` → `MessageArea.tsx:390-417`);failover/轮换通知走 `query:notice` 弱化系统行(per-session 分桶,cap 20,e2e 已钉);**402/429/403 在聊天中无分类**(裸红条)。ApiKeyBanner 四象限门控(无 provider/缺 key 变体点名 provider+深链)。vision 由引擎发送前拦截(声明>catalog>models.dev 三态,未知不拦),但拒绝文案指向 TUI `/model`,桌面无预检无「切换到 X?」建议。

**用量/成本**:Header 预算徽章(80%/超支双色)+BudgetBanner(豁免续发)+SessionUsageDialog+Usage 页(model/provider/day 三维)+目录行/picker 单价。**无发送前本次上下文成本预估**。

### 1.3 值得保持并放大的优势(本轮验证为真)

| 优势 | 证据 |
|---|---|
| failover 链实现质量(类型化资格/双上限/hop 清链/事件留痕) | `client.rs:1252-1444` + 6 单测 |
| key 轮换先于 failover、豁免会话钉(钉目标不钉凭证,语义自洽) | `client.rs:1266-1271` |
| 会话覆盖 sidecar(原子写/prune/重启恢复/坏文件降级,5 测试) | `session_override_store.rs:186-209,330-353` |
| providers.toml v2 拒载非法元数据 + JSON Schema 副本 | `provider_config_store.rs:154-155,270` |
| 导入硬拒 redacted 文件 | `commands_providers.rs:1304-1314` |
| 表单内 Test + Fetch models + dirty 守卫(诚实语义:save ≠ test) | `AddProviderModal.tsx:290-378` |
| 诚实态工程:keys/profiles/刷新按钮 loading/empty/failed 三态齐备 | `ProviderKeysPanel.tsx:162-243` 等 |
| i18n 旅程命名空间 8 locale 基本全译(R4-1 兑现,实测逐键对比) | `welcome.* 118 / settings.models.* 268 / chat.banner.* 6` |
| a11y 账本清空 + axe 全扫描在 e2e | `a11yDebt.ts:149` |
| 成本可见性三层(picker 单价/预算双态/Usage 三维)仍是全竞品最强档 | §1.2 用量段 |

---

## 2. 09-29 评审修复验证总表

统计:**P0 ×6 → 6 FIXED;P1 ×12 → 8 FIXED / 4 PARTIAL;P2 ×6 → 3 FIXED / 2 PARTIAL / 1 未修(已文档化)**。

| # | 09-29 问题 | 结论 | 关键证据(commit) |
|---|---|---|---|
| P0-1 | TUI 首跑死路/静默 Ollama 回落 | **FIXED**(回落降为日志级声明,`unified_config.rs:640`;TUI onboarding overlay 含 /connect 专区 `render.rs:1262-1421`) | #154 |
| P0-2 | ApiKeyBanner 死条件(对已配置用户常显) | **FIXED** | `get_provider_status` 快照门控,`Chat.tsx:62-66,755-761` + 单测(#154) |
| P0-3 | 认证失败错误指路错误(/config) | **FIXED** | 引擎 `auth_failure_suggestion` 指向 `/connect`+provider 专属 env(`error.rs:331-341`);桌面 auth 横幅+深链(`MessageArea.tsx:412`)(#154) |
| P0-4 | `shannon config` 静默无效 | **FIXED** | CLI 四态 outcome(Mirrored/JsonOnly/RefusedSecret…,`main.rs:6421+`)(#154) |
| P0-5 | /credentials 明文进提示 + agent Config 工具无拒绝 | **FIXED** | `disable_model_invocation+is_sensitive`(`credentials.rs:41-49`);工具拒 secret 键(`shannon-tools/config.rs:308-315`)(#154) |
| P0-6 | docs/configuration.md 教死配置 | **FIXED** | 全文重写 354 行 + docs/providers/ 9 篇(#154) |
| P1-7 | Custom provider 不发 Bearer | **FIXED** | 非空 key 默认 `Authorization: Bearer`,extra_headers 可覆盖(`client.rs:393-415`)(#154) |
| P1-8 | catalog 缺口 + /provider 切换保留旧模型 | **PARTIAL** | OpenRouter/Bedrock/ZhipuCoding 已补+钉测;**Azure 仍 0 条**;切换保留旧模型改为**显式警告**(`provider.rs:54-101`)但未收紧为强制 picker(#154) |
| P1-9 | 无 fetch models | **FIXED(桌面)** | `fetch_provider_models`(7 类错误分类)+ AddProviderModal 入口 + Settings 刷新目录按钮(`commands_config.rs:1497-1595`)(#163) |
| P1-10 | 桌面切换全局而非会话级/单 tab/死徽章 | **PARTIAL** | 会话级覆盖+持久化已落(R2-1+R5-1);tier 徽章已活;**dynamic 徽章仍死**(`commands_chat.rs:130`);目录数据源仍只有 active provider——「Provider Tabs」实际永远= All+1(`ModelsSettings.tsx:79-81`)(#163/#171) |
| P1-11 | failover 死字段 | **FIXED** | 显式 opt-in 全链,见 §1.1(R3-1,58e5d00d0) |
| P1-12 | 表单内不能测 key | **FIXED** | 表单内 Test(存储凭证回退)+「save ≠ test」语义注释(`AddProviderModal.tsx:290-321`)(#163/#258 后续) |
| P1-13 | Ollama ctx=4096/Welcome 探测/quick-fill | **PARTIAL** | `ollama show` num_ctx 已修;**Welcome 仍要求导出 `OLLAMA_HOST`**(注释自相矛盾,见 P-N4);quick-fill 由 fetch models 实质替代但过时 id 未清(→ P-N5)(#163) |
| P1-14 | CLAUDE_API_KEY/ANTHROPIC_AUTH_TOKEN 不识别 | **FIXED** | 请求路径+doctor 同链(`types.rs:326-344`)(#154) |
| P1-15 | /profile 命名冲突 | **FIXED** | `/permissions` 一等命令+/profile 迁移提示;`/profiles` 上线(R1-6+R3-2) |
| P1-16 | vision 无门控 | **FIXED(引擎侧)** | 发送前拦截+三态查找+未知不拦(`agent_loop.rs:86-164`);桌面 UX 残留见 P-N9(#165) |
| P1-17 | 8 locale 值为英文 | **FIXED(基本)** | 旅程关键命名空间全译(aa91b5a32);残留:`chat.notice.*` 4 locale、de 4 键(→ P2-N21) |
| P1-18 | 持久化语义混乱 | **FIXED(大部分)** | 三写路径统一 `ProviderConfigService`;providers.toml SSOT;config.json/preferences.json 角色已文档化(决策③推迟收敛)(#171) |
| P2-19 | effort 折叠在模型下拉里(`name · High` 文案) | **PARTIAL** | Header/composer 双 picker 分裂已按路由归属解决(`Header.tsx:50-52,341`);**effort 仍嵌在模型 Select 内**(`ChatInput.tsx:1499-1514`,chip 文案 :1438-1441)——原批评的核心形态仍在 |
| P2-20 | TUI tier tab 无 all 档 | **FIXED**(测试钉) | #158 |
| P2-21 | /provider health 跳过无原因 | **FIXED**(逐家跳过行) | #158 |
| P2-22 | 定价双表漂移 | **FIXED** | 声明定价第 1 优先+双半价规则,子串扫描只剩兜底(R2-4) |
| P2-23 | 类型/UI 漂移(ProviderKind/models_url/quick-fill) | **PARTIAL** | KIND_INFO 单点化含 gemini;**TS union 仍缺 gemini**(`types/index.ts:469-475`);models_url 仍 wire 有字段无输入;quick-fill 过时 id 未动(→ P-N5) |
| P2-24 | config.toml 手写行解析器 | **未修(已缓解+文档化)** | 读路径仍行解析,嵌套表静默跳过;configuration.md:25 已明示 |

---

## 3. 新问题清单(本轮发现)

> 编号 P-N*(Problem-New)。分级(v1.1 经红队重贴):**P1 高**=伤害典型画像/语义不可见的快速修复项;P1=旅程断裂/功能半程;P2=打磨。本域**没有发现满足「说谎 UI」定义的 P0**(redteam §1.1);全部论断经主会话抽查复核(§7)。

### P1 高 —— 快速信任/卫生修复(S1 批,原判 P0 降级)

**P-N1 聊天失败路径错误分类只有 auth/other,402/429/403 炸成裸红条**。
`classify_query_error_kind` 是二分类(`desktop/src/events.rs:110-123`,文本匹配 "authentication failed"/"unauthorized" 等);402(配额耗尽)、429(限流)、403 不在其中——配额烧尽的用户看到的是 provider 原始报文红条+Retry(报文本身常自含原因,如 DeepSeek "Insufficient Balance",但无分类、无深链、无「更新 key / 查看用量 / 切换模型」建议动作)。而**探活路径已有完整 5 类分类** `TestConnectionResult{Success/InvalidKey/RateLimited/QuotaExhausted/ProviderError}`(`commands_config.rs:1176-1192`,402 分类是 9c5dfbf24 专门补的)——同一信息在两个路径两种命运。根因是链路形态:引擎 typed `ApiError` → `Display` 字符串 → 桌面再文本匹配还原,类型信息在半路被主动丢掉。403 "Forbidden" 连 auth 词表都不在(轮换侧靠 type/message 嗅探补了,`retry.rs:194-213`,展示侧没有)。

**P-N2 会话 pin 静默关闭 failover,零 UI 提示**。
`apply_session_override` 对钉住的会话目标置 `suppress_failover = true`(`commands_chat.rs:502-506`,注释自证契约)。语义本身自洽(「用户钉哪就发哪」),但:①chip 的 `· session` 徽章与 hover 文案只说「仅此会话用 X」,不说「不再自动降级」;②ProviderKeysPanel 的轮换说明宣称自动换 key(轮换确实豁免此钉、仍然开启)——同一条芯片菜单语境下,一个自动恢复机制开、另一个关,用户无从分辨;③PhaseTier 的优先级文案(`PhaseTierSection.tsx:133-136`)只讲三级优先,不讲 failover 副作用。

**P-N3 dynamic 徽章仍是死 UI(v1.1 降 P2,修辞修正)**。
`commands_chat.rs:130` `dynamic: None` 硬编码,徽章恒不亮,且无任何注释承认其是 no-op;相邻的 honest-metadata 注释(`:119`)描述的是 tier 徽章行为(「解析不出不渲染而非猜」),物理位置贴近反而给读者相反印象——「注释掩饰 no-op」的指控经红队核查不成立(redteam §1.3),但死徽章本身属实:Settings 目录行的 dynamic 徽章(`ModelsSettings.tsx:301-308`)永远不亮——用户无法区分「静态目录条目」与「models.dev overlay 条目」,而 overlay 恰恰是「刷新目录」按钮的价值所在。

**P-N4 Ollama 环境探测:注释与实现矛盾,默认端口不探测**。
`detect_env_provider` 的 doc 注释写 "detected via `OLLAMA_HOST` **or default `localhost:11434**`"(`commands_config.rs:1060-1061`),实现只查 `OLLAMA_HOST` 是否被 set(:1082-1087)。后果:裸装 Ollama(服务已在 localhost:11434 跑着,什么 env 都没导)的用户在 Welcome 的 env 预探测永远 miss,`canAdvanceFromModel` 不放行,只能手填 baseUrl——恰好伤害的是「无 key 本地用户」这个最应该被顺滑承接的画像。

**P-N5 quick-fill 五个过时模型 id,新用户第一眼是 2024 目录**。
`QUICK_FILL` 预设仍写死 `gpt-4.1-mini`/`glm-4-plus`/`moonshot-v1-8k`/`abab6.5s-chat`/`llama3.2`(`add-provider-modal/types.ts:28-34`)。这些 id 在 2026-10 均非各家现役主力;fetch models 已能拉真实列表,quick-fill 却没有与它联动(拉到列表后 model 字段的预填值不变)。首跑旅程的第一印象 = 过时产品目录。

### P1 级 —— 旅程断裂 / 功能半程

**P-N6 per-model 元数据桌面编辑器未落(R2-4 半程)**。
引擎侧 providers.toml v2 的表达力已超竞品(双半价规则/deny_unknown_fields/整文件拒载),CLI 有 `shannon providers model-meta`;但桌面 0 入口(grep `declared_models|model-meta` 于 desktop/ui、desktop/src 零命中)。路线图 R2-4 明说「桌面编辑 UI 放 R3」,R3 批次未含此项,此后无人认领。后果:自定义网关/代理用户无法在 UI 校正 context/价格/能力→ tier 推断错档、计费错价、vision 误判,而这些全都有正确的引擎消费链在等着喂。

**P-N7 fetch models 结果是临时 datalist,无「模型仓/已验证」状态**。
Fetch models 拉到的列表只回填 `<datalist>` 建议(`AddProviderModal.tsx:343-378`),不持久、不进入 picker、不阻止选一个端点不存在的 id;错误 id 只能在请求时以 ProviderError 暴露(不可重试不可 failover,`retry.rs:241-249`)。BYOK 竞品的普遍形态(Cherry 挑选加入、Open WebUI verify+allowlist)是把「端点真实服务什么」变成持久可见状态。

**P-N8 declared `max_output` 是死字段**。
解析、校验、入注册表(`declared_models.rs:183-187`)但全仓库无生产消费者;请求 max_tokens 仍来自 config/profile 默认;picker wire 的 `ModelInfo` 也没有 max_output 槽(`commands_chat.rs:121-139`)。要么接线要么删,否则 schema 在教用户填一个无效承诺。

**P-N9 能力门控只有 vision,且桌面 UX 残留**。
tool-use/computer-use 无门控(anthropic toolsets 开关是硬编码模型名单,`client.rs:323`);vision 拒绝文案指向 TUI `/model`(桌面用户无此命令);无发送前预检、无「该模型不支持视觉,切换到 X?」一键建议(竞品 Continue/Roo 的标准做法)。另:能力查找的双向前缀匹配(`model_id.starts_with(m.id)` 反向继承,`agent_loop.rs:124-140`)意味着 `glm-4.5-air` 会继承 `glm-4.5` 的 vision 位——定价类碰撞已有负向钉测先例(catalog.rs:1050-1084),能力类没有。

**P-N10 profile 切换与陈旧会话覆盖的交互不可见**。
切换 profile 后,旧会话的覆盖在**查询时**才静默降级全局(仅 tracing warn,`commands_chat.rs:372-379`),无 UI 事件;ProfilesSection 对「活跃会话覆盖会压住 profile 切换的效果」零提示(`ProfilesSection.tsx:75-94` 只有空 profile 确认)。用户视角:「切了 profile 为什么这个会话还在用旧模型?」

**P-N11 改变「生效模型」的模型级 UI 面有 4 个(另有 2 个相邻操作),优先级只在 2 处可见(v1.1 口径修正)**。
模型级四面:composer chip(会话)/Header 选择器(非 chat 页全局)/Settings 快速切换器+目录行(全局)/Profile 切换(全局指针);相邻两操作:chip 菜单 Set-as-default(晋级全局)/Provider 卡 Activate(切 provider,间接改变生效模型)。chip 与 Header 按路由分区(同一概念、两个路由,非并存),但用户跨路由感知不到这是同一件事。优先级文案只在 PhaseTierSection 与 chat.phaseTier.hint 出现;Header(非 chat 页)完全看不到会话覆盖的存在。R2-1 的「路由归属」约定(chip 独占 /chat)解决了双 picker 分裂,但代价是**同一目录两套信息密度**:Header 行仅 context(`118k`),composer 行有完整 meta——用户在非 chat 页做全局决策时恰恰看不到价格。

**P-N12 导出文件的残留敏感面(v1.1 范围收窄,见 redteam §1.2)**。
主体干净(引用-only+0600+拒导 redacted),真实缺口一项:**redact walk 只动 `credential` 表,`extra_headers` 原样导出**(`commands_providers.rs:1222-1252`)——用户在 extra_headers 放 `Authorization: Bearer sk-...` 是常见做法(Custom provider 默认 Bearer 修复后更常见)。初稿的 `InlineLegacy{masked}` 明文出门风险经红队核查**降级为加固项**:生产代码零构造点(`unified_config.rs:997` 为测试 fixture;A1 决策下所有写入路径均写 Env/Keyring/Store),仅剩手编 TOML 理论面→S4 加固(导出遇 InlineLegacy 拒绝并提示迁移)。

**P-N13 failover/轮换只覆盖「建流前」失败;预算放大无全局上限**。
闭包只包住流的建立(`client.rs:1168-1202`),流中途死亡走同 provider 重连(F11,最多 3 次),**不跨 provider 降级**——设计选择但未在文档/UI 声明;预算最坏 4×(1+keys)+3×4 ≈ 24+ 请求/一次用户调用,无全局尝试上限,慢 429 场景用户等待可达分钟级。

**P-N14 Azure catalog 仍 0 条;/provider 切换保留旧模型未收紧**。
P1-8 的残留:Azure(企业用户主路径)无任何目录条目,切换过去保留上一个 provider 的模型(有警告但语义仍是「带着错误配置走」);路线图 B9 的「强制进 picker 或要求显式 provider/model」未执行。

**P-N15 网关与桌面的「会话覆盖」同名不同义**。
网关 `shannon/model.switch` 是 gateway 进程内**全局单值**、内存态、重启即失(`engineBridge.ts:224,318,592-605`);桌面是 per-session+sidecar 持久。移动端多设备/多会话共享同一 override,且与引擎会话状态零耦合。两套机制后续接引擎会话覆盖时必须收敛,现在没有任何文档声明差异。

**P-N16 空态反馈缺口(三处)**。
①Header 模型菜单目录为空时整个 Portal 不渲染——点击触发器**没有任何反应**(`Header.tsx:90`);②空 profile 的「如何往里加 provider」不可发现(实际机制=切到该 profile 后再 Add/Activate,`commands_config.rs:2113-2130`,UI 零指引);③Provider Tabs 区无「当前只显示 active provider 目录」说明。

**P-N17 tier 推断不扫 declared_models**。
`resolve_tier` 只看静态目录+显式 tiers 钉(`tier.rs:146-226`);声明了完整元数据的代理专属模型(恰恰是最依赖 tier 的场景)进不了 tier 推断,只能手写 `[tiers]`。声明能力只影响展示标签。

**P-N18 `config --explain` 不覆盖桌面专属键**。
plan_tier/act_tier/approval_mode/enabled_providers(存 desktop config.json)不在 KNOWN_KEYS(`commands_config_explain.rs:36-99`)——桌面调过 Plan/Act 的用户在 CLI 解释时得到 unknown。与决策③口径一致,但随 R3-3 落地已变成高频键。

### P2 级 —— 打磨

**P-N19 i18n 残留**:R5 新增的 `chat.notice.failover/keyRotation` 在 fr/es/pt-BR/ru 仍为英文 "Model fallback"(主会话逐 locale 实证);de 另缺 `tierStandard/tierPro/temperature/dynamicBadge`。
**P-N20 e2e 缺口**:AddProviderModal 完整旅程(添加→表单内 Test→Fetch→保存→卡片 Test→Test-all)、刷新目录、keys 面板操作、多 provider(≥2)切换、Welcome provider 步骤均无 Playwright 覆盖(现有 journey #17 只覆盖 chip override 弧)。
**P-N21 mock 缺口**:`test_all_providers`/`get_provider_allowlist` 在 mock allowlist 显式 unmocked(`mock-handlers-coverage.test.ts:116,121`)——demo 模式点 Test-all = "not available" toast。
**P-N22 a11y 小刺**:策略 pills 无 `aria-pressed`(`ModelsSettings.tsx:114-128`);Provider Tabs 无 `role="tablist"`。
**P-N23 写法分叉**:Settings `handleModelSwitch` 只写 `model` 键(`ModelsSettings.tsx:71`),Header 写 `model+provider` 双键(`Header.tsx:63-64`)——当前无害(目录本就来自 active provider),两套约定并存是未来改目录数据源时的脚枪。
**P-N24 小刺群**:`resolve_credential_keys` 的 dedup 只去连续重复(`[a,b,a]` 尾重复空转一轮);`unified_config.rs:3-8` 模块头注释仍写旧五层与 `build()` 六层不一致;TS `ProviderKind` union 缺 gemini 靠 `| string` 消音;`models_url` wire 有字段无输入 UI。

**P-N25(v1.1 红队补回,初稿漏收)gemini/Azure 探测死胡同无前置提示**。
`is_probeable_kind` 排除 gemini 与 azure(`commands_config.rs:1306-1313`),而两者都在 KIND_INFO 可选 kind 中——gemini 用户表单内 Test 永远得 Unknown verdict、Azure 用户将来同样,UI 无「该 provider 类型不支持探测,可直接保存使用」的前置提示。诚实的拒绝,但旅程上是silent dead-end;Open WebUI 的做法是把「哪些 provider 验证必然失败、不代表不兼容」写进失败语义(§4.2)。

---

## 4. 竞品调研(2026-10 时点)

> 方法:两路官方文档调研(编码 agent/AI IDE 类 + BYOK 聊天工作区类),全部条目标注官方证实/二手;完整来源清单见 §7.4。图例:✅ 官方证实 · 🟡 二手/部分证实 · ❌ 未发现 · ⚪ 官方未提及。**注意:❌ 与 ⚪ 均为「官方文档未发现」,不等于「确认不存在」**(v1.1 红队校准);涉及该类结论的关键竞品断言(如 Cursor 无自定义端点)另有社区共识佐证。

### 4.1 编码 agent / AI IDE 类

**Cursor**:Settings > Models 贴 key 即出 picker,**仅 5 家内置 provider、无自定义 OpenAI 兼容端点**(社区长期痛点);无测试连接按钮(「无效 key 导致该 provider 请求失败直到更新」)。切换=composer picker + **Auto 档**(Cursor Router 分类器逐请求选型,Teams/Enterprise 限定)+ **Optimize For: Cost/Balance/Intelligence** 三档;管理员可 Impose Auto(Soft=新会话默认/Hard=锁死)、隐藏底层模型。picker 分「Cursor Models/Other Models」两组,带 Hidden by default/Max Mode 标记。无 failover 文档;BYOK 按 list price 计费+Teams 另收 $0.25/M 转发费。

**GitHub Copilot(VS Code)**:BYOK 可完全脱离 GitHub 登录使用;三条接入路径(内置 provider/`@tag:language-models` **扩展生态**——VS Code 1.104 开放 Language Model Chat Provider API/Custom Endpoint 支持 Chat Completions/Responses/**Anthropic Messages** 三种 wire)。添加流程=picker 齿轮 → Add Models 表单 → **自动打开 `chatLanguageModels.json` 补元数据**(toolCalling/vision/maxInputTokens/maxOutputTokens/thinking/streaming/headers);key 可用 `${input:}` 变量注入 SecretStorage。**能力硬门控:不支持 tool calling 的模型不进 agent 列表**。utility model 体系:`chat.utilityModel/utilitySmallModel` 管标题/commit message 等杂务。无连接测试、无 fetch models(手填 json);picker 显示 **×N premium 倍率**(2026-06 起迁往 AI Credits)。

**Claude Code**(切换语义标杆):`/model` **Enter=切换并保存为默认(写 settings.json)、`s`=仅本会话**——把「试一下」和「换默认」两个意图分开;优先级链 `/model 会话 > --model flag > env > settings > ANTHROPIC_DEFAULT_MODEL`。**`opusplan`**(Plan=Opus/执行=Sonnet)+`SUBAGENT_MODEL` 子代理独立配模型。**fallbackModel 逗号链≤3**,过载时触发、仅当前 turn 生效。picker 内**直连 API 时显示价格**+上下文标签(`Set by ANTHROPIC_DEFAULT_MODEL`/`Org default`/`Requires usage credits`/被排除置灰)——「为什么这个模型长这样」全部可见。`ANTHROPIC_CUSTOM_MODEL_OPTION` 在 picker **追加**自定义条目。

**OpenAI Codex**:`codex login` OAuth/`OPENAI_API_KEY` 双轨,auth.json 可拷贝;**自定义 provider schema 最完整官方范本**(name/base_url/**env_key**/wire_api=chat|responses/query_params/http_headers/env_http_headers/**request_max_retries/stream_max_retries/stream_idle_timeout_ms** per-provider 网络鲁棒性参数);profiles 聚合 model/provider/effort,`--profile` 运行时选择;effort(minimal/low/medium/high)一等参数。**无 fetch models、无测试连接**(全手写 config)。

**Windsurf**:2025-07 被 Cognition 收购,docs 已 307 到 Devin;模型目录按「模型名+推理档」组合售卖(`Claude Opus 5 Medium`/`GPT-5.2 High Thinking`+Fast≈2× credit)——**effort 从隐藏参数变成目录一等 SKU**,带推荐位与套餐倍率表。BYOK 用量由 provider 直接计费不扣 credit(二手)。

**Cline/Roo/Kilo**:30+ provider 配置页;**OpenAI Compatible 高级块=逐模型元数据录入**(Max Output Tokens/Context Window/Image Support/Computer Use/输入输出每百万价格);Cline 有显式 **Verify** 按钮。**Cline Plan/Act 双模型开关**(切换自动换绑、历史延续、官方给推荐组合);**Roo API Configuration Profiles**=profile 打包 provider+key+model+参数,**Prompts tab 把 profile 绑到每个 mode** 且记住每 mode 上次选择,**任务粘性**(任务锁定启动 profile 含 orchestrator 子任务);**Roo Export 全量 settings JSON(明文含 key+官方高亮警告)/Import 校验合并/auto-import 路径**;Kilo 有 **Fetch Models** 按钮+云同步 profiles。**Roo 硬门控:`uses native tool calling exclusively`——不支持工具的模型直接不可用**。

**Continue**:config.yaml **角色绑定**(每模型条目带 `roles: chat/edit/apply/summarize/autocomplete/embed/rerank`)+`capabilities` 覆盖自动探测(**tool_use 为 Agent 必需**)+`defaultCompletionOptions`;Hub blocks `uses:/with:/override:` 引用与分享——配置即代码范式。

**Zed**(元数据 schema 最细):五条接入路径(hosted/API/现有订阅/Gateway/Local);key **存系统钥匙串不进 settings.json**,env 优先;`openai_compatible.available_models[]` 逐模型声明 `max_tokens/max_output_tokens/reasoning_effort/capabilities{tools,images,parallel_tool_calls,prompt_cache_key,...}`;**utility models 全家桶**:`inline_assistant/commit_message/thread_summary/compaction/subagent` 五个独立槽位+`agent.default_model`。

**Gemini CLI**(failover UX 标杆):四认证路径;`/model` 对话框三选项(**Auto(Gemini 3)/Auto(2.5)/Manual**);**ModelAvailabilityService 健康监控:主模型失败(配额/服务器错误)默认先弹窗征询用户同意再切 fallback**——「询问式降级」;内部 utility 调用走静默链(flash-lite→flash→pro);实验性本地 Gemma 做路由决策。`/model` 设置**全局生效**(非会话级)、sub-agent 模型不受控。

**Trae**:Add Model **表单当场验证**——成功即加入、失败在表单内报错(编码类罕见的内联验证,Shannon 已具备)。

### 4.2 BYOK 聊天工作区类

**Cherry Studio**(provider 管理标杆):60+ 内置 provider 页;**获取模型列表→「模型管理」按 + 挑选加入,只有加入的模型才出现在所有选择器**(curated 模型仓范式),条目显示实际 API 模型 ID 防同名混选;**「检测」按钮可选 key 可选模型**;**多 key:逗号粘贴或钥匙图标进入逐 key 管理(标签/启停/删除),按列表顺序轮询**;**四槽默认模型**(默认助手/快速/翻译/绘画);逐模型可编辑名称/类型/价格;WebDAV+S3 备份同步;Ollama 专属页(keep-alive 分钟数)。

**Open WebUI**(verify 语义标杆):Connections 表单=URL+Key+**Provider 类型下拉**(告知后端启用对应行为:Azure/llama.cpp/LM Studio/LiteLLM)+**Model IDs 白名单**(留空=自动拉全部;填了=替换且不再调 /models;Prefix ID 解跨 provider 同名);**Verify Connection 语义讲得最清楚**:官方文档明列「已知会验证失败的 provider(GitHub Models/Perplexity/MiniMax 无 /models)→这不代表不兼容,手动填模型即可」;**OpenRouter 数千模型必须白名单+缓存否则模型页 10-15s**(官方教训);**Task Models**:起标题/打标签/follow-up 等后台调用路由到独立的便宜模型(本地/远程两槽);Workspace Models 可包预设(Description/Tags 直接进选择器+能力开关);Direct Connections(用户 key 只存浏览器、绕过后端直连)。

**Jan**(key fallback 语义标杆):Add Provider 先选 **API 格式(OpenAI/Anthropic 兼容)**→base URL(必须 /v1)→key,**保存时自动从 {base_url}/models 拉取**;**API key fallbacks:仅 HTTP 401/403/429 自动换下一个 key**;**Test keys 逐 key 报 OK/Invalid(401)/Forbidden(403)/Rate limited(429)/Network error**;全部烧完提示「轮换耗尽」;本地 Hub 带硬件适配 pill(Fits/May be slow/Won't fit)+量化分组+Recommended;Fit to Hardware 按显存封顶 context。

**Chatbox**:Provider 配置(**至少加一个模型并勾选能力 vision/reasoning/tool_use,不配视为纯文本**)→「检查」验证;模型 schema=modelId/nickname/**capabilities/contextWindow(算输入上限)/maxOutput(限请求参数)**;**差异化:Provider 配置 JSON+deep link(`chatbox://provider/import?config=BASE64`)——第三方网关官网一键接入,schema 带 urls.getApiKey/docs/models 直链**,已有多家网关发布此格式。

**LobeChat**:70+ provider 页(逐家「从控制台获取 key」分步教程);per-assistant 绑定模型+推理强度;**per-agent 用量统计**(费用/Token/**缓存节省含命中率**/按模型明细/7-90 天趋势);内置预置模型、选中未下载本地模型提示下载。

**LibreChat**:YAML 驱动;`models:{default:[...](fetch 失败回退),fetch:bool}`+`addParams/dropParams`(防 422)+`tokenConfig` 按模型 context+价格;apiKey 仅单值(多 key 靠前置 LiteLLM/OpenRouter);Presets(已 deprecated→Agents)JSON 导入导出。

**BoltAI**:会话级 Chat Configuration 弹层(Service/Model/参数)可 Save as Default;**Context 策略四档显式暴露(All/None/First n/Last n 默认 10)**——「发了多少历史=多少钱」直给用户;per-command 绑定 provider。

**Msty**:引导式本地流程(检测硬件→推荐引擎与模型→Light→Powerful 档→后台下载);**添加 remote provider 强制「至少配一个模型」防死配置**。

**OpenRouter**(聚合器参照):400+ 模型目录带**条件定价(超 200K 提价/峰谷时段)/弃用日期/supported_parameters 能力过滤/`:free`·`:nitro`·`:floor` 变体语义**;请求级 `models[]` fallback 链+provider 级按价格加权负载均衡+`max_price`/`require_parameters` 约束。BYOK 客户端把它当一个 Provider 接入(一个 key=数百模型),反向教训是「太多必须白名单」。

**托管型参照**:ChatGPT 桌面=顶部模型 picker(按订阅档)+2026 重构为 Chat/Codex 全局 switcher;Claude 桌面=Chat/Cowork/Code 三模式;Google AI Studio=右上角下拉**悬停显示 rate limits**。

### 4.3 横向对比总表(Shannon = 本轮复核查证后的现状)

| 维度 | Shannon | Claude Code | Codex | Cursor | Copilot | Cline/Roo/Kilo | Zed | Gemini CLI | Cherry | Open WebUI | Jan | Chatbox |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 首跑引导 | ✅ Welcome 2 步+env 预探测 | ⚪ login 向导 | ✅ login | ⚪ 设置直达 | 🟡 | 🟡 侧栏选择 | ✅ 设置页 | ✅ 认证选择 | 🟡 设置直达 | ⚪ | ✅ Add Provider | 🟡 |
| Provider 广度 | ✅ 26 变体+openai 兼容 | ❌ 单厂+云 | 🟡 自定义 schema | ❌ 5 家 | ✅ 扩展生态 | ✅ 30+ | ✅ | ❌ 单厂 | ✅ 60+ | ✅ 任意 | ✅ | ✅ |
| 自定义 openai 兼容 | ✅ kind+headers+tiers | 🟡 custom option | ✅ 最全 schema | ❌ | ✅ 3 wire | ✅ | ✅ 最细 capabilities | ❌ | ✅ | ✅ | ✅ | ✅ |
| 表单内验证 | ✅ Test+延迟+分类 | ❌ | ❌ | ❌ | ❌ | ✅ Verify | ⚪ | n/a | ✅ 检测 | ✅ Verify | ✅ Test keys | ✅ 检查 |
| Fetch models | ✅(R2-2)+刷新目录 | ❌ | ❌ | ❌ | ❌ 手填 json | 🟡(Kilo ✅) | ❌ 手填 | n/a | ✅+挑选固化 | ✅ 白名单 | ✅ 保存时拉 | ✅ |
| fetch 结果固化进 picker | ❌ 临时 datalist | — | — | — | — | ❌ | ❌ | — | ✅ | ✅ | ✅ | ✅ |
| 会话级切换+设为默认 | ✅ chip+Set as default | ✅✅ Enter/s | ✅ /model | ✅ | ✅ | ✅ | ✅ | ⚪ 全局 | ✅ 会话内 | ✅ | ✅ | ✅ |
| Plan/Act·按角色/模式绑模型 | 🟡 plan/act tier(全局) | ✅ opusplan+SUBAGENT | 🟡 profiles | ❌(custom modes 🟡) | 🟡 utilityModel | ✅✅ Cline 开关/Roo per-mode+粘性 | ✅ 5 utility 槽 | 🟡 | ✅ 助手绑定 | ✅ 预设 | ⚪ | ✅ 能力勾选 |
| 能力门控 | 🟡 仅 vision(引擎) | ⚪ | ⚪ | 🟡 标记 | ✅ tool 硬门控 | ✅ Roo 硬门控 | ✅ 声明+过滤 | ⚪ | ⚪ | ✅ 开关 | ✅ 逐模型手配 | ✅ 勾选 |
| 逐模型元数据编辑 | ❌ 桌面 0 入口(CLI/TOML) | ⚪ | ⚪ | ❌ | ✅ json | ✅ 高级块 | ✅ schema | ⚪ | ✅ 价格 | ❌ | ⚪ | ✅ ctx/maxOut |
| Failover | ✅ 显式链≤3+事件流 | ✅ fallbackModel≤3 | ❌ 仅重试参数 | ❌ | ❌ | ❌ | ❌ | ✅ 询问式 | ❌ | ❌ | ✅ key 级 | ❌ |
| 多 key 轮换 | ✅ 自动(slot0+轮询) | ❌ | ❌ | ❌ | 🟡 多 provider | ❌ | ❌ | ❌ | ✅ 管理器+轮询 | ❌ | ✅ 401/403/429 | ❌ |
| 错误分类→深链 | 🟡 探活 5 类/聊天 2 类 | ✅ 标签体系 | ⚪ | 🟡 文档 | 🟡 | 🟡 | ⚪ | ✅ 询问切换 | ✅ 指向模型列表 | ✅ 失败语义文档 | ✅ 逐 key 分类 | 🟡 |
| 配置导出/导入 | 🟡 CLI-only | 🟡 json | 🟡 auth.json | ❌ | 🟡 json | ✅ Roo+警告 | ❌ | ❌ | ✅ WebDAV/S3 | ⚪ | 🟡 | ✅ deep link |
| 本地模型(Ollama) | 🟡 num_ctx 已修/探测要 env | ❌ | ✅ oss | ❌ | ✅ 内置(弃用中) | ✅ | ✅ Local | 🟡 | ✅ keep-alive | ✅ 自动识别 | ✅✅ Hub | ✅ |
| 成本可见性 | ✅✅ 三层+预算 | ✅ picker 价格 | 🟡 | ✅ | ✅ ×N | 🟡 手填价 | 🟡 | 🟡 | ✅ 逐模型价 | ⚪ | ❌ | ❌ |
| 切换面数量(心智负担) | ❌ **4 面+2 邻操作** | ✅ 1(/model) | ✅ 1 | ✅ 1+Auto | ✅ 1 | 🟡 2 | 🟡 2 | ✅ 1 | ✅ 1 | ✅ 1 | ✅ 1 | ✅ 1 |

### 4.4 Table stakes(2026-10 双阵营合并)

1. 输入区内 model picker 是第一公民 UI;会话级切换+「设为默认」两级语义(Claude 的 Enter/s 是金标准)。
2. Provider 详情页三件套:key+base URL(留空=官方地址)+模型列表管理;**fetch 后挑选固化,用户 curated 列表决定 picker 内容**(BYOK 阵营全员)。
3. 某种形式的连接验证,失败当场报;验证失败的语义要讲清(「verify 失败≠不能用」)。
4. OpenAI 兼容最小公约数+至少一种第二协议;per-provider 网络参数(重试/超时)下沉(Codex)。
5. **模型能力显式声明+门控**:不支持 tool calling 不进 agent 列表(Copilot/Roo 硬门控,Zed schema 声明);vision/reasoning/tools 标记进 picker。
6. **utility/后台小任务模型与主模型解耦**(Cherry 四槽/Open WebUI Task Models/Zed 五槽/Copilot utilityModel/LibreChat titleModel)——2025-2026 成为双向阵营共同趋势。
7. Reasoning effort 一等参数(Codex/Claude/Zed);Devin 把它做成目录 SKU。
8. key 安全存储+多 key 管理(BYOK 阵营:Cherry 轮询、Jan 401/403/429 fallback);导出含 key 必须高亮警告(Roo 范式)。
9. 失败信息至少指明 provider 与原因;401/quota 可辨识(Claude 标签体系、Jan 逐 key 分类、Gemini 询问切换)。
10. 配置可迁移:导出/备份/同步至少一种;Roo auto-import、Chatbox deep link 是低成本高感知形态。
11. 本地模型零配置路径:localhost:11434 预填/自动识别,免 key。

### 4.5 少数派差异化亮点(值得借鉴)

| 亮点 | 出处 | 对 Shannon 的映射 |
|---|---|---|
| `/model` Enter=默认/s=会话 | Claude Code | Shannon chip 的 Set as default/Reset 已对齐,**保持** |
| picker 上下文标签(「为什么这个模型长这样」:价格/来源/额度/置灰原因) | Claude Code | P-N10/P-N11 的解法范本;dynamic 徽章(P-N3)应升级为「来源标签」体系 |
| 询问式 failover(先问再降)+utility 静默链 | Gemini CLI | S1-1/S3-4 的交互范式参照 |
| per-mode profile 绑定+任务粘性 | Roo Code | R3-2 profile 体系的二期方向;会话覆盖与 profile 的「粘性」语义可对标 |
| 逐模型 capabilities schema(tools/images/cache/…) | Zed | Shannon ModelSpec 的 capabilities 位可扩 tool_use,喂门控 |
| Provider 配置 JSON+deep link 一键接入 | Chatbox | 远期 provider 生态分发方向(本周期不做,§6 明确不做) |
| Task/utility models 独立槽位 | Open WebUI/Zed/Cherry | Shannon tier 系统天然映射(S3-3) |
| 验证失败≠不能用的语义文档 | Open WebUI | Fetch models 失败态文案应补此语义 |
| 数千模型必须白名单+缓存的性能教训 | Open WebUI | S2-1 模型仓固化的 curated 语义天然规避 |
| BYOK key 直连浏览器/localStorage 分层 | Open WebUI Direct | 不适用(桌面单机),略 |

---

## 5. 对比分析:Shannon 的位置

### 5.1 三条关键旅程的对照

**旅程 A:新用户首跑配 provider**
Cursor「Settings 贴 key 即用」、Trae「当场验证成功即加入」、Cherry「provider 页→key→获取列表→挑选→检测」。Shannon 的 Welcome 2 步+AddProviderModal(8 预设芯片+表单内 Test+Fetch models+dirty 守卫)**结构上已达 BYOK 标配水位,表单内验证甚至领先多数编码类竞品**(Cursor/Copilot/Claude/Codex 全都没有)。残留断点恰好都在「最后一公里」:quick-fill 过时 id(P-N5,首屏第一印象)、Ollama 不探测默认端口(P-N4,伤害最典型画像)、fetch 结果不固化(P-N7,拉了白拉)、无 key 获取链接(Cherry/Chatbox/LobeChat 标配)。

**旅程 B:会话中切模型**
金标准是 Claude Code:一个 `/model` 入口,Enter/s 分离两意图,picker 内价格+来源+置灰原因全部可见,opusplan 双档,SUBAGENT_MODEL 分角色。Shannon 的**机制面已经齐全甚至更丰富**(chip 会话覆盖+Set as default≈Enter/s;plan/act tier≈opusplan;profiles≈Roo;tier 别名是独有差异化),**但呈现面输在收敛度**:4 个模型级切换面分散在 3 个路由(竞品全部 1-2 个)、优先级只在 2 处可见、Header 与 composer 信息密度分裂、effort 还折叠在模型下拉里(竞品已一等化甚至 SKU 化)。**机制不缺,缺的是「一个 surface 内嵌层级」的收敛**(P-N10/P-N11)。

**旅程 C:失败恢复(401/402/429)**
引擎机制是 Shannon 强项:自动轮换(401/429/403 同 provider,先于 failover)+显式 failover 链(429/5xx,≤3)+事件流留痕——编码类竞品里只有 Claude(fallbackModel 链)和 Gemini(询问式)有同类能力,Cursor/Copilot/Cline 全线没有。**但展示面把优势埋没了**:聊天路径 auth/other 二分类(P-N1)、402 配额炸裸条、pin 静默关 failover(P-N2)、「配额→换 key→切模型」的建议链不存在(Gemini 询问式、ChatGPT 三级兜底是范式)。Jan 甚至把 key 级错误分类(Invalid/Forbidden/Rate limited/Network)做进了逐 key 测试——Shannon 的 TestConnectionResult 五类分类本来就等价,只是没接到聊天失败路径。

### 5.2 逐维度结论

- **引擎/配置架构层:领先或持平**。providers.toml v2 的表达力(多 profile/per-model 元数据/双半价规则/整文件拒载)超过 Cline 的高级块与 Copilot 的 json;failover+轮换+事件流的组合仅次于 Claude/Gemini 且工程质量更高;导出脱敏(引用-only+拒导 redacted)比 Roo 明文导出安全一个档次。短板:tier 推断不扫声明(P-N17)、max_output 死字段(P-N8)、config.toml 行解析器(P2-24)。
- **桌面 UI 旅程层:结构达标,细节失分**。table stakes 1-4/9-11 基本达标;失分点是模型仓固化(P-N7)、元数据编辑器(P-N6)、能力门控宽度(P-N9)、错误恢复建议(P-N1)、切换面收敛(P-N11)、本地零配置(P-N4)。
- **与 09-29 评审时相比的格局变化**:当时「关键旅程最后一公里大量断裂」的判断已不成立(引导/验证/会话切换/failover 都闭环了);新的格局是**「引擎能力面领先、桌面呈现面欠账」**——修的都是在 UI 上把已有引擎能力接出来的最后一米,不需要大的引擎改动(S2 的 tier 扫声明/max_output 接线是仅有的两处引擎侧小活)。
- **一个新的战略机会**:utility/后台小任务模型解耦(§4.4-6)是双阵营共同趋势,而 Shannon 的 tier 体系(fast/standard/pro)是所有竞品里最接近「场景槽位」的既有心智——竞品在用独立配置键模仿 Shannon 已经内置的概念。把 tier 从「推断标签」升级为「一等场景槽位」(对标 Zed 五槽/Open WebUI Task Models)是低成本高差异化的方向(S3-3)。

---

## 6. 改进方案(供审核;批准后按惯例另出排期稿)

> 分四批:S1 止血(单快速 PR)→ S2 模型仓与元数据 → S3 切换体验收敛 → S4 打磨簇(随批搭车)。验收门槛沿用 09-30 路线图口径(fmt/clippy/nextest/rustdoc/design-token/locale 键集/全套 desktop e2e/mock tripwire)。**决策①-⑪已全部拍板(2026-10-05,⑩⑪为行内默认补录),见 §6 末尾裁定记录表。**

### S1 止血批 —— 信任修复(W1,单 PR)

| ID | 事项 | 范围与验收 | 对应问题 |
|---|---|---|---|
| S1-1 | 聊天失败错误分类与恢复建议 | 引擎 query:failed 载荷携带结构化错误 kind(401 auth/402 quota/429 rate_limit/403 authz/其他),替换 Display-字符串-文本匹配;**分类单一实现落在引擎侧**(探活分类源于 HTTP status,查询路径桌面看不到 status),探活与查询两路消费同一语义。桌面专属横幅:402→「配额耗尽」+更新 key/查看用量/切换模型三深链,429→限流+等待提示,403→权限说明。**尺寸 M(引擎+桌面+i18n+e2e 四层,S1 批内最大项)**。验收:engine kind→桌面横幅→i18n 键全链钉测;8 locale 键齐 | P-N1 |
| S1-2 | pin×failover 关系可见化 | chip 菜单 session pin 项/hover 文案补「钉住的会话不参与自动降级」;ProviderKeysPanel 轮换说明同句补齐。验收:文案键+e2e 断言 | P-N2 |
| S1-3 | dynamic 徽章删除(v1.1 默认翻转) | **默认=删徽章+清 i18n 键**(`merge_static_and_dynamic` 无来源标记,接真数据=ModelInfo wire 变更,非 S 尺寸);「接真数据+来源徽章」并入 S2-1 一次做对。验收:UI 无此徽章且无死键 | P-N3 |
| S1-4 | Ollama 零配置探测+quick-fill 更新 | `detect_env_provider` 补 `localhost:11434` TcpStream 探测(短超时,与注释对齐)。**护栏:该函数被 `get_provider_status` 门控热路径共享,探测结果须 TTL 缓存(建议 30s)或仅在 Welcome 命令路径探测**;QUICK_FILL 五个 id 更新为现役模型并与 Fetch models 联动(拉到列表后预填第一个)。验收:裸装 Ollama 用户 Welcome env 预探测通过的 e2e;quick-fill id 与 catalog 交叉钉测;门控路径无重复探测的单测 | P-N4/P-N5 |
| S1-5 | 空目录反馈 | Header 模型菜单目录为空时渲染空态项(「无可用模型→去 Settings 添加」深链)。验收:e2e 断言空目录点击有反馈 | P-N16① |

### S2 模型仓与元数据批 —— table stakes 收尾(W2-W3)

| ID | 事项 | 范围与验收 | 对应问题 |
|---|---|---|---|
| S2-1 | provider 模型仓(fetch 固化) | Fetch models 结果**显式逐项多选**后固化为该 provider 的 `models: Vec<ModelSpec>`(写入 providers.toml v2,引擎已支持);**防 TOML 污染护栏(v1.1,容量已裁定⑥):默认零选中、软上限 50(超限警告)、「全选」二次确认、写入段加「UI 管理」注释头**;**合并优先级钉:profile.models = picker 过滤白名单,catalog/overlay = 元数据供给**(一条钉测)。picker/目录行带来源徽章(declared/fetched/manual);手输目录外 id 给软警告(不拦)。验收:fetch→挑选→picker 可见→重启持久 的 e2e;模型仓写路径单测;白名单过滤钉测 | P-N7 |
| S2-2 | per-model 元数据桌面编辑器(R2-4 收尾) | 模型仓行/目录行展开编辑 context/max_output/输入输出单价/能力位(vision/tools),写 providers.toml v2;表单校验复用引擎侧规则。验收:编辑后 tier 标签/计费/vision 门控即时变化的钉测 | P-N6 |
| S2-3 | max_output 接线或删除 | **裁定⑩(按 v1.1 行内默认):接线**——请求 max_tokens 以声明值 clamp + picker 显示(工作量小且有真实价值;删字段反而要动 schema/wire) | P-N8 |
| S2-4 | 能力门控扩宽(**v1.1 拆 a/b**) | **S2-4a(S):vision 拒绝文案桌面化(去掉 TUI /model 指向)+发送图片前预检+「该模型不支持视觉,切换到 Y?」一键建议(Y=当前 provider 内有 vision 位的模型)+双向前缀匹配负向钉测(glm-4.5-air 类碰撞)**。**S2-4b(M-L,依赖 S2-1,裁定⑧:S2 批尾部):capabilities 增 tool_use 位(catalog 位标志+wire 字段)+工具路径预检门控** | P-N9 |
| S2-5 | tier 推断扫 declared_models | resolve_tier 第三态并入声明元数据;验收:声明完整元数据的代理模型可被正确分档的钉测 | P-N17 |
| S2-6 | Azure catalog 补齐 | **前置(v1.1):验证 api-version 查询参数与 deployment 名=模型 id 语义的测试覆盖;`is_probeable_kind` 对 azure(及 gemini,见 P-N25)的排除须在表单内有前置提示或纳入可探测**。主体:≥3 条主力模型+镜像定价同步测试(沿用 OpenRouter 批次模式);**裁定⑤:随本批收紧 /provider 切换为「无目录条目时强制进 picker/要求显式 provider/model」**(09-30 B9 原案,同批避免 Azure 真空期) | P-N14/P-N25 |

### S3 切换体验收敛批 —— 差异化(W4-W6)

| ID | 事项 | 范围与验收 | 对应问题 |
|---|---|---|---|
| S3-1 | 切换面收敛+「为什么」标签 | 对标 Claude Code picker 标签体系:picker 行来源/覆盖状态标签(「会话覆盖中」「profile X 钉定」);Header 选择器对齐 composer meta(价格/vision)或降级为纯显示;Settings 快速切换器与 Header 写法统一(双写)。验收:同一模型在三个面的信息一致性钉测 | P-N10/P-N11/P-N23 |
| S3-2 | profile×会话覆盖交互提示 | 切换 profile 时若存在活跃会话覆盖,提示「N 个会话仍使用覆盖模型」;覆盖因 profile 切换失效时发 UI 事件(替换 tracing-only)。验收:e2e 断言提示出现 | P-N10 |
| S3-3 | tier 槽位化(utility models)(**v1.1 重估:L 尺寸**) | 把 tier 系统暴露为一等场景槽位:后台小任务(标题/摘要/compaction/子代理)默认走 fast tier,可逐槽覆盖。**红队修正:引擎 `AuxRole`/`auxiliary` 在 schema 存在但全仓零消费者——本项是新建消费链(解析→按角色解析目标→未配置回退),非"复用"**;且 **utility 槽须与 R5-5 交互优先级链正交**(只走后台任务通道,不进 session>phase>global,不动 unattended 钉测表,裁定⑦)。范围已裁定②:首批 compaction+会话摘要两槽。依赖 S2-5 | §5.2 战略机会 |
| S3-4 | failover 引导 | provider 卡片/Test-all 结果页提供「推荐降级链」一键建议(同目录同族:pro→standard→fast 或同模型低价变体),写入 fallback_models;事件流降级说明文档化(含流中途不降级语义,P-N13 的文档部分一并落)。验收:一键生成的链与 resolve_failover_chain 兼容钉测 | P-N13/竞品 4.5 |
| S3-5 | effort 一等化 | effort 脱离模型下拉。**裁定⑪(按 S3-1 收敛原则推定):picker 内二级交互(模型行展开 effort 子档),不加独立控件**——独立控件=新增一个切换面,与单一切换面目标矛盾;对标 Codex /model 内选 effort 的形态。`name · High` 文案随之消亡 | P2-19 残留 |
| S3-6 | 发送前成本预估 | composer 发送前按当前上下文 tokens×当前模型单价估算本次成本并展示(09-29 评审 §3-D 遗留);预算临近时联动 BudgetBanner。**验收(v1.1 重定义):估算与计费共用同一 token 计数实现,钉测断言一致性(同输入→同数字)——不做"与实结误差"钉测(发送前输出未知,无 ground truth,物理上不可钉)**;输出侧按 max_tokens 区间展示 | §1.2 |

### S4 打磨簇(随批搭车+单卫生 PR)

i18n 回填(`chat.notice.*` 4 locale+de 4 键,S1-1 新键同步 8 locale)|e2e 补齐:AddProviderModal 完整旅程/目录刷新/keys 面板操作/多 provider 切换/Welcome provider 步|mock 补齐 test_all_providers/get_provider_allowlist|a11y:aria-pressed/tablist|TS ProviderKind 补 gemini+models_url 输入框|导出安全(裁定④):extra_headers 值嗅探(Bearer/sk-/x-api-key 模式)导出告警 + `--redact` 整键值替换(保留键名),不做静默改写;导出遇 InlineLegacy 拒绝并提示迁移(v1.1 降级为加固:生产零构造点,防手编文件)|CLI/TUI 损坏 providers.toml 告警对齐|`unified_config.rs` 模块头注释修|dedup 连续重复修|`config --explain` 增补桌面键(plan_tier/act_tier/enabled_providers,CLI 读 desktop config.json 只读解释)|P-N25 gemini/Azure 探测前置提示|网关/桌面 override 语义差异写入 configuration.md(P-N15 先文档化,收敛出本周期)。

### 决策点裁定记录(①-⑪ 已全部拍板,2026-10-05)

| # | 决策点 | 裁定 | 关键理由(拍板时确认) |
|---|---|---|---|
| ① | 会话 pin 与 failover | **保持 suppress_failover,文案可见化**(S1-2);Gemini 询问式记远期演进 | 取消会让"显式选择被静默覆盖"——比"显式选择不享受降级"更伤信任;pin(就发这个模型)与 failover(不满意时替我换)语义本就矛盾 |
| ② | S3-3 首批槽位范围 | **compaction + 会话摘要两槽**;数量为可调参数,验证心智后再扩 | 两槽是验证"场景槽位"所需最小范围;两任务高频、成本可观测、失败无感 |
| ③ | 模型仓固化后 picker 语义 | **curated 过滤 + 目录外可手输(软警告),不做硬白名单** | 硬白名单重造"目录外模型无处安放";软警告覆盖拼错 id 风险,保留 power user 自由度 |
| ④ | 导出 extra_headers 处理 | **值嗅探(Bearer/sk-/x-api-key 模式)导出告警 + `--redact` 整键值替换为 `<redacted>`(保留键名);不做静默改写** | 静默改写让导出文件"看着能用其实坏了",违背诚实失败原则;规则保持简单可解释 |
| ⑤ | /provider 切换保留旧模型 | **收紧为「无目录条目时强制进 picker/要求显式 provider/model」(09-30 B9 原案),与 S2-6 同批落地** | 一次性警告=带着错误配置走;与 S2-6 同批避免 Azure 用户真空期;显式 provider/model 永远可用 |
| ⑥ | S2-1 模型仓容量策略 | **软上限 50 + 「全选」二次确认 + TOML「UI 管理」注释头**;数字为可调参数 | 50 覆盖正常 curated 用量(竞品用户实际常用 5-20),超限几乎全是误触全选 |
| ⑦ | utility 槽与优先级链关系 | **正交化:只走后台任务通道,不进 R5-5 交互优先级链,不动 unattended 钉测** | R5-5 是全仓最敏感契约;两消费路径天然分离,没有理由选会组合爆炸的那条路 |
| ⑧ | S2-4b 排期 | **S2 批尾部** | tool_use 位与 S2-1 来源徽章同动 ModelInfo wire,同批一次协调;S2-4a(vision)无依赖先行 |
| ⑨ | 严重度重贴 | **接受**(已应用于 v1.1 全文) | 不影响批次范围与排期,标签诚实化 |
| ⑩ | max_output 接线或删除(行内项,补录) | **接线**:请求 clamp+picker 显示 | v1.1 行内已注明默认;按全量拍板授权落定,如异议可推翻 |
| ⑪ | effort 一等化形态(行内项,补录) | **picker 内二级交互**,不加独立控件 | 独立控件=新增切换面,与 S3-1 单面收敛矛盾;对标 Codex /model 内选 effort |

### 明确不做(延续 09-30 决策+本轮新增)

model router(引擎设计 non-goal,auto tier 除外;Cursor Router 式组织治理不在单机产品范围)|Ollama 模型下载管理 UI|存储写入收敛(决策③,R4-4a 只读解释除外)|provider 扩展市场/插件化接入(Copilot Language Model Provider API 式,远期)|Chatbox deep link 式 provider 分发生态(远期)|failover 询问式降级(Gemini 式,裁定① 记远期演进)|per-provider 网络鲁棒性参数暴露(Codex 式 request_max_retries/stream_idle_timeout;`quirks` 字段已有位可扩展,等真实用户诉求再开放——v1.1 红队补)。

### 时间线

W1=S1(单快速 PR);W2-W3=S2(含 S2-4b 收尾);W4-W6=S3(决策已全部裁定,按期开批);S4 随批搭车,W6 收尾单卫生 PR。依赖:S2-1/S2-2 共享模型仓写路径(S2-1 先行);S2-4b 依赖 S2-1(共享 ModelInfo wire 变更,裁定⑧定于 S2 尾);S3-1 依赖 S2-1 的来源徽章;S3-3 依赖 S2-5(tier 扫声明)。

## 7. 证据索引与复核清单

### 7.1 主会话抽查复核过的关键论断

| # | 论断 | 复核方式 |
|---|---|---|
| 1 | 会话 pin 置 `suppress_failover = true` | 直读 `desktop/src/commands_chat.rs:495-508`(注释+赋值) |
| 2 | `dynamic: None` 硬编码 + 上方 honest-metadata 注释 | 直读 `commands_chat.rs:125-135` |
| 3 | `detect_env_provider` 只查 `OLLAMA_HOST`,注释承诺默认端口 | 直读 `commands_config.rs:1055-1095` |
| 4 | quick-fill 五个过时 id | 直读 `add-provider-modal/types.ts:26-36` |
| 5 | 表单内 Test 存在(P1-12 裁决依据;两路探查结论冲突,以代码为准) | 直读 `AddProviderModal.tsx:290-330`(注释「verify BEFORE saving — save ≠ test」) |
| 6 | `chat.notice.failover` 在 fr/es/pt-BR/ru 为英文 | 逐 locale grep `desktop/ui/src/i18n/locales/*.json` |
| 7 | Header 空目录 Portal 不渲染;Header 双写 vs Settings 单写 | 直读 `Header.tsx:60-95` |
| 8 | ProfilesSection 切换仅空 profile 确认、无覆盖提示 | 直读 `ProfilesSection.tsx:75-115` |
| 9 | `priceUnknown` 各 locale 为 "—" 占位符(**非**翻译缺口,从问题清单剔除) | grep zh-CN/fr |
| 10 | `InlineLegacy` 生产零构造点(P-N12 范围收窄依据;`unified_config.rs:997` 为测试 fixture) | 全仓 grep 构造点+读上下文 |
| 11 | `AuxRole`/`auxiliary` 零消费者(S3-3 重估 L 依据) | grep 全仓仅 `HashMap::new()` 构造 |
| 12 | `honest metadata` 注释描述 tier 非动态(P-N3 修辞撤回依据) | 直读 `commands_chat.rs:115-135` |
| 13 | Azure wire 支持存在(S2-6 非陷阱但需前置) | `types.rs:62,128-129,186,219` |

### 7.2 两路代码深查的关键证据锚点(桌面 UI 旅程)

| 论断 | 位置 |
|---|---|
| Welcome 门控/旅程流程 | `Layout.tsx:126-129`;`Welcome.tsx:23-118`;`ModelStep.tsx:55-75`;`DoneStep.tsx:69` |
| AddProviderModal 表单内 Test/Fetch/Advanced/dirty 守卫 | `AddProviderModal.tsx:79-93,296-417,435-444` |
| PhaseTier 优先级文案 | `PhaseTierSection.tsx:133-136` |
| Profiles 空态/最后一个禁删/切换写路径 | `ProfilesSection.tsx:325-331,392-397`;`commands_profiles.rs:136-341` |
| Provider 卡片 Test 用存储凭证(J4 修复确认) | `ProvidersSection.tsx:56-61`;`commands_config.rs:1323-1338` |
| Keys 面板轮换语义与三态 | `ProviderKeysPanel.tsx:44-306` |
| chip 会话覆盖/晋级/Reset/meta | `ChatInput.tsx:386-458,1414-1514`;`sessionModelPromotion.ts` |
| 覆盖持久化 sidecar | `session_override_store.rs:28-40,157-209,330-353` |
| 401 横幅深链 / failover·轮换通知 | `events.rs:38-153`;`AppContext.tsx:1493-1549`;`MessageArea.tsx:356-417`;`ChatContext.tsx:34-69` |
| ApiKeyBanner 四象限 | `Chat.tsx:62-66,697-702,755-761` |
| 目录刷新四态 | `ModelsSettings.tsx:231-266`;`commands_config.rs:1564-1595` |
| vision 引擎门控 | `crates/shannon-core/src/query_engine/engine/agent_loop.rs:86-164,676` |

### 7.3 引擎/网关侧证据锚点

| 论断 | 位置 |
|---|---|
| 六层合并/优先级钉/无人值守表 | `unified_config.rs:462-475,640`;`commands_chat.rs:324-370,1264-1337`;`phase_tier.rs:12-30` |
| providers.toml v2 schema/校验/拒载 | `provider_config.rs:157-320,361-376`;`provider_config_store.rs:154-155,270` |
| 声明元数据消费四路 | `query_engine/types.rs:410-441`;`engine/mod.rs:293,326`;`model_registry.rs:476-488`;`agent_loop.rs:93-164` |
| failover 链/资格/双上限/事件 | `unified_config.rs:771-858`;`retry.rs:22,32-58,161-167,253-261`;`client.rs:1252-1444` |
| key 轮换策略与豁免 | `credential_manager.rs:60-129,348-415`;`provider_resolver.rs:270-282`;`client.rs:1266-1330`;`retry.rs:191-216` |
| 导出脱敏与导入硬拒 | `commands_providers.rs:1137-1367`;`provider_config_store.rs:908-954` |
| InlineLegacy 携真键 / extra_headers 不脱敏 | `provider_resolver.rs:254`;`commands_providers.rs:1222-1252` |
| catalog 77 条/Azure 0/钉测 | `catalog.rs:858-1117` |
| Ollama num_ctx 修复 | `model_registry.rs:279-366,1815+` |
| 网关 override 单值内存态 | `gateway/src/mobile/engineBridge.ts:224,318,562-605` |
| CLI-only 的导出/导入/explain/model-meta | `main.rs:6282-6295,6332-6376,941-942` |
| max_output 死字段 | `declared_models.rs:183-187`;`commands_chat.rs:121-139` |
| tier 推断不扫声明 | `tier.rs:146-226` |

### 7.4 竞品资料来源

**编码 agent/AI IDE 类**:Cursor(cursor.com/docs:models-and-pricing/settings/api-keys/cursor-router/sdk/typescript;forum.cursor.com);GitHub Copilot(code.visualstudio.com/docs/copilot/language-models;code.visualstudio.com/blogs/2025/10/22/bring-your-own-key;github.blog Enterprise BYOK 2025-11-20;docs.github.com 模型总表);Claude Code(code.claude.com/docs/en/model-config/env-vars;claude.com;arstechnica.com);Codex(github.com/openai/codex docs/config.md@459363e、docs/authentication.md;developers.openai.com/codex 反爬未直抓;github.com/orgs/community/discussions/173272);Windsurf/Devin(docs.devin.ai/desktop/models;docs.windsurf.com 实测 307→docs.devin.ai;pulse2.com;flexprice.io;reddit r/Codeium);Cline(docs.cline.bot llms.txt、core-workflows/plan-and-act.md、provider-config/openai-compatible.md);Roo Code(roocodeinc.github.io/Roo-Code:features/api-configuration-profiles、features/settings-management、providers/openai-compatible);Kilo Code(github.com/Kilo-Org/kilocode changelog;glideflowai.com 二手);Continue(docs.continue.dev/reference、customize/deep-dives/configuration);Zed(zed.dev/docs/ai:use-api-access、agent-settings、llm-providers);Gemini CLI(github.com/google-gemini/gemini-cli docs:cli/model.md、cli/model-routing.md、cli/generation-settings.md、get-started/authentication.mdx);Trae(docs.trae.ai/ide/models)。

**BYOK 聊天工作区类**:Cherry Studio(docs.cherryai.com.cn:pre-basic/providers、providers/providers.md、zi-ding-yi-fu-wu-shang.md、settings/default-models.md、settings/data-settings/webdav.md、providers/ollama.md);LobeChat(github.com/lobehub/lobehub raw docs:usage/providers.zh-CN.mdx、providers/openai、providers/ollama、agent/agent-profile.zh-CN.mdx——站点迁移期,自仓库核实);Open WebUI(docs.openwebui.com:getting-started/quick-start/connect-a-provider/*、features/chat-conversations/direct-connections、features/workspace/models、features/administration/task-models);LibreChat(librechat.ai/docs/configuration:librechat_yaml/object_structure、custom_endpoint、dotenv、balance;user_guides/presets);Jan(github.com/janhq/jan raw docs/dev:desktop/remote-models/custom-endpoint.mdx、manage-models.mdx、model-parameters.mdx、settings.mdx);Chatbox(docs.chatboxai.app/guides:providers.md、providers/openrouter.md、providers/ollama.md、providers/lm-studio.md、providers/import-config.md);LM Studio(lmstudio.ai/docs/app、basics/download-model、api);BoltAI(docs.boltai.com:start/use-another-ai-service.md、chat-ui/chat-configuration.md);Msty(docs.msty.ai/studio/getting-started/quick-start);OpenRouter(openrouter.ai/docs:models、features/provider-routing);Google AI Studio(techpp.com、codecademy.com 三方)。

**资料受限声明**:TypingMind(docs 超时)与 AnythingLLM(Cloudflare 拦截)仅有一句话定位与三方教程证据,未纳入对比表;LobeChat 现行文档处于迁移期,对话内 picker 细节未获官方证实;Codex 官方文档站反爬,schema 取自其 GitHub 仓库完整文档;Windsurf 专项信息部分依赖二手(官方页已重定向);`:free` 限额数字来自社区资料未官方核实。
