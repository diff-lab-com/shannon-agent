# Provider/Model 配置与支持深度评审(含竞品对比)

- 日期:2026-09-29
- 范围:Shannon 对 LLM provider/model 的配置、支持、UI 与使用旅程(终端 TUI / CLI / 桌面三端),并与编码代理类(Claude Code、Codex CLI、Gemini CLI、Cline、Roo Code、Continue、Aider、Zed)及 BYOK 聊天工作区类(Cherry Studio、LobeChat、Open WebUI、LibreChat、Jan、Chatbox)逐维度对比。
- 方法:三路代码探索(引擎配置层 / 桌面 UI 旅程 / TUI·CLI 流程,file:line 取证)+ 两路竞品官方文档调研 + 关键论断人工抽查复核。抽查过的论断在文中标注「已复核」。

---

## 1. 现状盘点(Shannon 当前体系)

### 1.1 引擎层支持面

- **Provider 枚举 26 个**(`crates/shannon-engine/src/api/types.rs:50-106`),4 种 wire format(Anthropic 原生 / OpenAI 兼容 / Ollama / Gemini 原生),每种有默认 base_url 与端点路径(types.rs:178-243)。base_url 可反推 provider(types.rs:113-175)。
- **模型目录三层**:静态 `MODEL_CATALOG`(~48 个模型,含 context/max_output/单价/capabilities 位掩码,`crates/shannon-core/src/model_registry/catalog.rs:121-858`)+ models.dev 动态 overlay(`/model refresh` 或 `/connect` 后拉取,24h 缓存,严格增量,`model_registry/dynamic.rs`)+ Ollama 本地 `ollama list` 探测(model_registry.rs:281-314)。
- **Tier 体系(fast/standard/pro/auto)**:别名(haiku/flash/mini→Fast、sonnet/plus→Standard、opus/ultra/max→Pro),解析顺序 = 用户 pin(providers.toml)> 目录能力+价格推断 > 旧枚举(`model_registry/tier.rs:156-226`)。这是竞品中没有直接同类的差异化设计(最接近的是 Claude Code 的 sonnet/opus/haiku 别名 + Gemini 的 auto/pro/flash)。
- **凭证**:`~/.shannon/credentials/<service>.json`,0600 强制,写入路径原子化;providers.toml 永不存明文 key(A1 决策);`/connect` 全链路脱敏(`redact_secret_command`)。Zhipu 走 JWT、ZhipuCoding 走 x-api-key、Anthropic 走 x-api-key+beta headers(client.rs:350-395)。
- **配置面五层**:CLI 参数 > `providers.toml`(connected 层)> `SHANNON_*` env > 项目 `.shannon.toml` > 全局 `config.toml` > 默认值(`unified_config.rs:460-491`)。另有 `config.json`(KV 存储)与 `preferences.json`(最近 model/provider),实际是**五个持久化位置**。

### 1.2 三端入口

| 端 | 入口 | 亮点 |
|---|---|---|
| TUI | `/connect`(存 key→探活→热加载→开 picker)、`/model`(picker/别名/--tier/--max-tokens/refresh)、`/provider`(列表/切换/`health` 并发探活)、StatusCard/StatusBar 实时显示 | `/connect` 的 1-token 验证探活与 `/provider health` 是同类产品少有的能力 |
| CLI | `shannon providers add/remove`、`list-providers`、`--model provider/model`、`--provider`、`--effort`、`--dump-config`(分层来源) | 非交互可脚本化;但 `providers add` 拒收明文 key(A1)且无 `shannon credentials` 子命令 |
| 桌面 | Welcome 2 步向导(任务类型→AddProviderModal)、Settings→Models(策略 pills/活动模型/Provider CRUD/Test·Test-all/可见性/目录列表/温度·max_tokens)、composer 模型 chip | AddProviderModal 快捷芯片 8 家预设 + Advanced(extra headers/per-tier 覆盖/fallback 列表);key 全程 write-only |

### 1.3 竞品对比总表(按旅程阶段)

图例:✅ 完整 · 🟡 部分 · ❌ 无

| 维度 | Shannon | Claude Code | Codex CLI | Gemini CLI | Cline / Roo | Continue | Aider | Cherry / LobeChat / Open WebUI |
|---|---|---|---|---|---|---|---|---|
| 首跑向导/引导 | 🟡 桌面 2 步;TUI 仅 overlay 快捷键提示 | ✅ 登录向导 | ✅ codex login | ✅ 首跑 auth 对话框 | ✅ 侧栏选择器 | ✅ hub 登录 | 🟡 文档引导 | 🟡 设置页直达 |
| Provider 广度 | ✅ 26 变体含国产全家桶 | ❌ 单厂商(+Bedrock/Vertex) | 🟡 OpenAI 系+内置 ollama/lmstudio | ❌ 单厂商 | ✅ 20-30 家 | ✅ | ✅ LiteLLM 全量 | ✅ 30-70 家 |
| key 存储 | 🟡 明文 0600 文件 | 🟡 env/settings.json | 🟡 auth.json(可选 keyring) | ✅ OAuth 缓存 | ✅ VS Code SecretStorage | 🟡 env/yaml | 🟡 env/.env | 🟡 服务端/浏览器存储(LibreChat 加密) |
| 测试连接/验证 key | ✅ 桌面 Test·Test-all + /connect 探活 + /provider health | 🟡 /status 查看 | 🟡 login status | ❌ | ✅ Verify 按钮 | 🟡 能力探测 | ❌ | ✅ Test/Check/Verify 全员 |
| 保存前(表单内)验证 | ❌ | — | — | — | 🟡 | — | — | 🟡(Cherry 表单外 Test) |
| **拉取 /models 模型列表** | ❌(仅静态目录+models.dev overlay;Ollama 例外) | ➖ | 🟡 model_catalog_json | ➖ | ❌(Cline 明确不做)/🟡 | 🟡 | ✅ /models 搜索 | ✅ 全员(Cherry 挑选加入、OWUI verify+allowlist) |
| 自定义 OpenAI 兼容端点 | ✅ kind+base_url+extra headers+tiers | ❌ 仅 Anthropic 兼容网关 | ✅ model_providers 表(最完整 schema) | 🟡 仅 proxy env | ✅ | ✅ | ✅ | ✅ |
| 自定义模型元数据(context/价格/能力) | 🟡 全局 default_max_tokens + tiers;无 per-model context/价格/能力录入 | ➖ | 🟡 | ➖ | ✅ Cline/Roo 高级块(逐模型) | ✅ capabilities 字段 | ✅ metadata json | ✅ Cherry 逐模型价格编辑 |
| 会话内切模型 | ✅ TUI;❌ 桌面是全局切换 | ✅ /model(Enter 默认 vs s 会话) | ✅ /model | ✅ /model 对话框 | ✅ | ✅ | ✅ /model | ✅ composer 每会话 |
| Plan/Act 或分角色模型 | 🟡 仅 effort 档位;agent 文件可指定 model | ✅ opusplan、SUBAGENT_MODEL | ✅ review_model、plan effort | ✅ auto 路由 | ✅ Cline Plan/Act 双模型、Roo profile↔mode | ✅ 六角色 | ✅ main/editor/weak 三模型 | 🟡 助手绑定模型 |
| 故障降级/failover | ❌ 字段存在但无人赋值(dead code) | ✅ fallbackModel 三连 | 🟡 | ✅ 配额错误提示切换+内部静默链 | ❌ | ❌ | ❌ | ❌ |
| Ollama/本地 | 🟡 探测硬编码 ctx=4096、quick-fill 过时 | ❌ | ✅ codex oss | 🟡 本地 Gemma 路由 | ✅ LM Studio 等 | ✅ | ✅ 自动修 num_ctx | ✅ 自动列表/下载/keep-alive/负载均衡 |
| 能力门控(vision/工具) | ❌ 标记存在但不拦截 | ➖ | ➖ | ➖ | ✅ "Not Supported" 明示 | ✅ | 🟡 model warnings | 🟡 |
| 成本可见性 | ✅✅ 三层定价表+picker 单价+statusbar/usage/budget | ❌ | 🟡 | 🟡 | 🟡 Cline usage 页 | ❌ | ✅ /tokens | 🟡 Cherry 逐模型价格 |
| 配置导入/导出 | 🟡 MigrationWizard 仅导入 | ✅ /import | ✅ /import | ❌ | ✅ Roo settings json 导出 | ✅ yaml 即代码 | ✅ | 🟡 Cherry WebDAV 备份 |
| provider 专属文档页 | ❌(configuration.md 已漂移) | ✅ | ✅ | ✅ | ✅ 逐家 | ✅ | ✅ 逐家 | ✅ LobeChat 70+ 页 |
| env var 全覆盖 | ✅ 20+ 变量链 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ OWUI 完全对等 |

**一句话结论**:Shannon 的引擎层广度(tier、定价、探活、脱敏)超过几乎所有竞品,但**关键旅程的"最后一公里"大量断裂**——引导缺失、错误指向错误出口、桌面切换是全局而非会话级、自定义 provider 不发鉴权头、没有 fetch models——导致引擎能力用户实际感知不到。

---

## 2. 问题清单

### P0 — 阻断新用户 / 静默错误(已复核标注 ✅)

1. **新用户首跑是死路(TUI)**。无向导;首跑 overlay 只教快捷键,不提 `/connect`(render.rs:1255-1420)。零配置时引擎静默回落 `ollama@localhost:11434/llama3`(provider_resolver.rs:286-292),首条消息报裸连接错误 `⚠️ Query failed: …`,没有"未配置 provider,请运行 /connect"的引导。竞品最低限度都有首跑 auth 对话框或"推荐 provider+推荐模型"步骤。
2. **桌面 ApiKeyBanner 对已配置用户永远显示(死条件)✅**。`Chat.tsx:461-465` 依赖 `config.api_key`/`config.provider`,而 Rust 端 `DesktopConfig` 已在 P1.2-B(ADR-0005)删除这两个字段(desktop/src/config.rs:13-20;已复核),生产环境该条件恒真 → "Add your API key" 横幅对所有已配置用户持续显示,直到手点关闭——狼来了效应,真正未配置的用户反而被训练性忽略。同一死代码还波及 `Layout.tsx:116` welcome 门控与 GeneralSettings 的 "Session info: Not configured"。
3. **认证失败的错误指路错误 ✅**。引擎 401 建议 `"Check your API key with /config or set SHANNON_API_KEY"`(error.rs:332;已复核)——`/config` 根本不能设置 key,正确出口是 `/connect`;且该文案未提及当前 provider 的专属 env(如 ZHIPU_API_KEY)。桌面侧更糟:聊天中 401 只显示原始红条+Retry(AppContext.tsx:826-852、MessageArea.tsx:291-301),无 401 分类、无"去 Settings 更新 key"深链——而分类探活机制(TestConnectionResult)已经存在,只是没接到聊天失败路径。
4. **`shannon config model=…` 静默无效**。CLI `config` 子命令只写引擎不读的 `config.json`,不镜像 `config.toml`(与 TUI `/config set` 行为不一致);新用户按直觉操作后毫无反馈。
5. **`/credentials` 把明文 key 送进 LLM 提示**(shannon-commands/builtin/credentials.rs:154-173,prompt-command 带 args);脱敏只覆盖 `/connect`。同时 agent 侧 `Config` 工具无 allowlist、无秘密拒绝(shannon-tools/src/config.rs:291-392)——A1 红线只在人类路径执行。
6. **文档教的是死配置**。`docs/configuration.md` 仍指导 `api_key = "..."` 写进 config.toml,但该字段已被 N1 静默忽略(unified_config.rs:10-22);整个 `/connect`+`providers.toml` 体系、providers CLI、tier、凭证目录全部未记载;provider 列表停留在 4 家(代码 26 家)。

### P1 — 功能缺失 / 一致性破坏

7. **Custom(openai 兼容)provider 不注入鉴权头 ✅**。`LlmProvider::Custom` 只发 extra_headers,`/connect` 存的 key 完全不用(client.rs:375-380;已复核)——自建网关/NewAPI/OneAPI 用户按引导填 key 后必然 401,且错误文案指向 `/config`。竞品(含 Codex 的 env_key、Zed 的 Add Provider)均默认 Bearer。
8. **catalog 覆盖不全**:26 个 provider 枚举只有 ~18 个有目录条目;OpenRouter(首要 BYOK 聚合器)、Azure、Bedrock、Cloudflare、Replicate、ZhipuCoding(Plan) 为 0 条目。`/provider openrouter` 切换后**保留上一个 provider 的模型**(provider.rs:47-82),属于静默错误配置。
9. **没有"从 provider 拉取模型列表"**。模型输入是自由文本+过时 quick-fill(`glm-4-plus`、`moonshot-v1-8k`、`llama3.2`),拼错只在请求时以 provider 404 暴露;models.dev overlay 刷新只有 CLI `/model refresh`,桌面无入口。fetch models 是聊天类竞品的全员 table stakes,连 Shannon 自己的 `/provider health` probe 基建都已在。
10. **桌面模型选择是全局而非会话级**。composer chip 切换写引擎 `model`+`provider`(ChatInput.tsx:148-159 → commands_config.rs:409-452),无 per-session 覆盖;所有竞品(含 Aider/Claude/Codex)都是会话级。Settings 的 provider tabs 因此永远只有一个 tab,tier/dynamic 徽章因 `list_models_for` 硬编码 `tier: None, dynamic: None`(commands_chat.rs:106-107;已复核)成为死 UI;TiersEditor 编辑的 per-tier 覆盖在 picker 中无任何体现。
11. **failover 是陷阱**:`fallback_provider`/`fallback_base_url`/`ProviderProfile.fallback_models` 贯穿引擎与重试路径,但没有任何代码路径赋值(全部 None);同时文档宣称"no model router"是设计原则——死字段与设计声明互相矛盾。竞品:Claude 三连 fallback、Gemini 配额错误主动提示切换。
12. **表单内不能测 key**:Test/Test-all 按钮只在保存后的卡片上;AddProviderModal 内输入 key 无法验证,保存本身也不验证。Open WebUI 的"保存≠验证,Verify 才算数"语义 + Cherry 表单旁 Test 是更好的范式。
13. **Ollama/本地模型体验薄弱**:探测到的模型硬编码 `context_window=4096`(model_registry.rs:305)与内部 fallback 200k 自相矛盾;Welcome 仅在导出 `OLLAMA_HOST` 时才探测(默认安装检测不到);无"检测本地服务/拉取已装模型"按钮。对比 Aider 自动修 Ollama num_ctx、Open WebUI/Jan/Cherry 的一键列表+下载+keep-alive。
14. **`doctor` 与环境变量识别以 Anthropic 为中心**:`doctor` 检查 `sk-ant-` 前缀、`CLAUDE_API_KEY` 仅 doctor 认识而请求路径不认,`ANTHROPIC_AUTH_TOKEN` 完全不识别——Claude Code 迁移用户的 env 静默失效,报错文案却只提 SHANNON/ANTHROPIC/OPENAI(types.rs:494-501)。
15. **`/profile` 命名冲突**:StatusCard 底部推荐 `/profile`,但它管理的是权限 profile 且经 LLM 解析参数;寻找 provider profile 的用户会得到模型生成的答案。v2 的多 profile + 网关路由数据结构已就绪但只有单 `"default"` 生效(provider_resolver.rs:49-62)。
16. **vision 能力无门控**:capabilities::VISION 与 `supports_vision` 只用于展示,发图前不检查,非视觉模型收到 provider 报错。竞品 Continue/Roo 均有"模型不支持"前置明示。
17. **i18n 结构完整、事实上英文**:10 个 locale 结构 0 缺失,但 zh-CN 之外 8 个 locale 约 3181-3200/3533 个值仍是英文——provider 配置与首跑的"本地化"对绝大多数语言用户不成立。
18. **持久化语义混乱(五个存储、三种写法)**:`/model`·`/provider` 每次切换静默写 `preferences.json`;tier pin 要显式 `--save`;`--max-tokens` 不 `--save` 会提示 "(not saved)" 但 model/provider 从不提供该选择;TUI `/config set` 镜像 config.toml 而 CLI 不镜像;`providers.toml` 损坏仅在 `shannon providers` 路径告警,REPL 启动静默降级。

### P2 — 打磨项

19. **effort 折叠在模型下拉里**(ChatInput.tsx:626-688),`name · High` 易被当模型名;Header 与 composer 双 picker 按路由分裂是审计文档自己都批评过的形态。
20. **TUI picker tier tab 循环无 "all" 档**(select.rs:1104-1126),切到某 tier 后回不到全量列表。
21. **`/provider health` 跳过 Gemini/Bedrock/Azure/Replicate** 但输出无逐家原因说明。
22. **定价双表漂移**:`MODEL_CATALOG.cost_per_m_*`(驱动 tier 推断)与 `DEFAULT_PRICING`/LiteLLM(驱动计费)独立维护,子串匹配脆弱(glm-5.3-flash 已因此专门补条目,catalog.rs:396-408)。目录含大量未来/推测模型 ID 与猜测定价(如 MiniMax M3 "mirrors M2.7")。
23. **类型/UI 漂移**:前端 `ProviderKind` union 缺 `gemini`(types/index.ts:390-396 vs models-settings/types.ts:19);`models_url` 有 wire 字段无输入 UI;quick-fill 预设 id 多处过时。
24. **`config.toml` 加载器是手写行解析器**(unified_config.rs:504-596),非 JSON 路径下的完整 TOML 表会被静默跳过。

---

## 3. 改进方案

### A. 止血(1-2 周,P0)

1. **首跑引导**:
   - TUI:首次无 provider 时,首条消息前插入引导卡(而非裸错误):"未连接任何 provider → `/connect <provider> <key>` 或 `/connect` 查看列表;本地模型 `/connect ollama`";overlay 加入 `/connect`。把静默 Ollama 回落改为**显式声明**:"未检测到配置,正在尝试本地 Ollama(llama3)…"。
   - 桌面:Welcome skip 后的空 chat 状态(WelcomeState)加 provider CTA;修复 ApiKeyBanner 条件改用 `has_api_key`/active provider 快照;修复 Layout welcome 门控与 GeneralSettings 死字段。
2. **错误路由正确化**:error.rs 401 建议改为 `"/connect <provider> <new-key> 更新凭证"` 并列出当前 provider 的专属 env 名;桌面 `QUERY_FAILED` 分类 401/403 → 专属横幅"key 无效 → 更新 key"深链 `/settings/models`(复用 TestConnectionResult 分类)。
3. **Custom provider 默认 Bearer**:`LlmProvider::Custom` 在未提供 Authorization extra_header 时,以存储的 key 发 `Authorization: Bearer`(保留 extra_headers 覆盖能力)。
4. **`shannon config` CLI 与 TUI 对齐**:同样镜像可写键到 config.toml,或在输出中明示"该键仅存于 config.json,引擎不读取"。
5. **安全红线一致化**:`/credentials store` 改为不落 LLM 提示(直接调用 CredentialManager 的内置命令);agent Config 工具加上与 `/config set` 相同的秘密拒绝。
6. **重写 docs/configuration.md**:以 `/connect` + `providers.toml` + tier 为主线,补 provider 全表、env 全表、providers CLI、凭证目录;为 Top 8 provider(Anthropic/OpenAI/DeepSeek/GLM/Kimi/MiniMax/Ollama/OpenRouter)各写一页"获取 key→连接→验证"(对标 LobeChat 的 per-provider 页)。

### B. 补齐 table stakes(1 个月,P1)

7. **Fetch models 全端打通**:引擎已有 list-models probe(probe.rs)与 `/api/models`;桌面 AddProviderModal 加"获取模型列表"按钮(provider `/models` → 可选列表 → 选中回填),失败回退自由文本+手动添加;Settings 加"刷新目录"(接 `/model refresh` 的 models.dev/LiteLLM 拉取)。失败语义学 Open WebUI:"保存不验证,获取失败不代表不能用"。
8. **会话级模型覆盖(桌面)**:composer chip 只改当前会话的 modelOverride(网关移动端已有 `shannon/model.switch` 同款机制可复用),"设为默认"才是全局;Settings provider tabs 显示全部已连接 provider(数据源改为全部 store 而非仅 active)。
9. **catalog 补齐 OpenRouter/Azure/Bedrock/ZhipuCoding** 至少各 3-5 个主力模型;`/provider <无目录 provider>` 切换时强制进入 picker 或要求显式 `provider/model`,不再保留旧模型。
10. **模型元数据编辑(自定义模型)**:对 openai-compatible 的每个模型允许录入 context window / max output / 输入输出单价 / 支持 vision·工具(对标 Cline/Roo 高级块),录入值同时喂给计费与 tier 推断,消除定价双表漂移。
11. **表单内验证**:AddProviderModal 保存前调用与 Test 相同的 probe,结果内联展示;key 格式软警告(非 `sk-` 前缀提示而非拦截)。
12. **Ollama 升级**:启动/Welcome 主动探测 localhost:11434(不要求 OLLAMA_HOST);`ollama show` 取真实 num_ctx 替换硬编码 4096;quick-fill 改为探测到的实际已装模型。
13. **env 兼容面**:请求路径识别 `CLAUDE_API_KEY`、`ANTHROPIC_AUTH_TOKEN`;doctor 按 active provider 选择 key 前缀与连通性检查目标。
14. **i18n**:对 8 个未翻译 locale 至少补齐 `welcome.*`、`settings.models.*`、`chat.banner.*` 三个旅程关键命名空间(约 250 键),其余允许英文回退。

### C. 差异化(季度,P1.5-P2)

15. **接通 failover**:给 `ProviderProfile.fallback_models` 接线(请求失败 429/5xx/529 时按序重试,UI 显示"已降级到 X"),把死字段变成对标 Claude `fallbackModel` 的真功能;`auto` tier 输出降级说明(对标 Gemini)。
16. **Profile 二期(多 profile + 绑定)**:providers.toml 已有多 profile/路由权重结构,补 UI/命令(`/profile list|use|new`——解决与权限 profile 的命名冲突后);Roo 的 profile↔mode 绑定、Cline 的 Plan/Act 双模型直接映射到 Shannon 的 tier+tiers 覆盖,建议在 chat 头部暴露"规划用 Pro / 执行用 Fast"开关。
17. **能力门控**:发送图片/启用计算机使用前检查 VISION 位,无能力时给出"该模型不支持视觉,切换到 X?"建议。
18. **多 key 与轮换(可选)**:对标 Cherry Studio 的 per-provider key 管理(标签/启停/轮换),对被限流用户价值大。
19. **配置导出**:providers.toml + 脱敏快照的一键导出/导入(对标 Roo settings json),与现有 MigrationWizard(仅导入)互补。
20. **单一事实源整合(工程债)**:长期把 `config.json`/`config.toml` 扁平键/preferences.json 收敛进 providers.toml v2 + 显式 UI 偏好,`--dump-config` 的分层来源保留为调试视图;统一 TUI/CLI/桌面三条写路径为同一个 `ProviderConfigService` API。

### D. 应保持并放大的既有优势

- **成本可见性**(picker 单价、statusbar 实时费用、session/monthly 双预算、三层定价表)是全竞品最强,建议在模型 chip hover 与 /cost 之外增加 per-task 预估(发送前估算本次上下文成本)。
- **`/connect` 探活 + `/provider health`** 超出编码类竞品,补齐逐家跳过原因说明后可作为宣传点。
- **tier 别名系统**(fast/standard/pro + haiku/sonnet/opus 兼容)与 Claude/Gemini 的别名习惯兼容,继续作为核心心智模型,但需要把"tier=模型映射"在桌面真正可视化(修 P1-10 后即成立)。

---

## 4. 附:证据索引(关键 file:line)

| 论断 | 位置 |
|---|---|
| 26 provider 枚举/4 wire format | crates/shannon-engine/src/api/types.rs:50-106, 38-47 |
| 静态目录 ~48 模型 | crates/shannon-core/src/model_registry/catalog.rs:121-858 |
| models.dev overlay | crates/shannon-core/src/model_registry/dynamic.rs:34-37 |
| Ollama 探测硬编码 ctx=4096 | crates/shannon-core/src/model_registry.rs:281-314 |
| 配置优先级 | crates/shannon-core/src/unified_config.rs:460-491 |
| 凭证 0600 | crates/shannon-core/src/credential_manager.rs:422-524 |
| Custom 不发 Bearer ✅复核 | crates/shannon-engine/src/api/client.rs:375-380 |
| 401 建议指向 /config ✅复核 | crates/shannon-engine/src/api/error.rs:332 |
| 静默 Ollama 回落 | crates/shannon-core/src/provider_resolver.rs:286-292 |
| ApiKeyBanner 死条件 ✅复核 | desktop/ui/src/pages/Chat.tsx:461-465 + desktop/src/config.rs:13-20 |
| tier/dynamic 死徽章 ✅复核 | desktop/src/commands_chat.rs:106-107 |
| 桌面切换=全局 | desktop/ui/src/components/chat/ChatInput.tsx:148-159 |
| TUI /connect 七步 | crates/shannon-ui/src/repl/commands/config/connect.rs:169-240 |
| /provider health | crates/shannon-ui/src/repl/commands/config/provider.rs:98-250 |
| CLI config 只写 json | crates/shannon-cli/src/main.rs:5373-5430 |
| /credentials 明文进提示 | crates/shannon-commands/src/builtin/credentials.rs:154-173 |
| docs/configuration.md 漂移 | docs/configuration.md(全篇)vs unified_config.rs:10-22 |

竞品调研原始材料:Cline(docs.cline.bot)、Roo Code(docs.roocode.com)、Continue(docs.continue.dev)、Aider(aider.chat)、Codex CLI(developers.openai.com/codex)、Gemini CLI(github.com/google-gemini/gemini-cli)、Claude Code(code.claude.com)、Zed(zed.dev/docs)、Cherry Studio(github.com/CherryHQ/cherry-studio)、LobeChat(lobehub.com/docs)、Open WebUI(docs.openwebui.com)、LibreChat(librechat.ai)、Jan(jan.ai)、Chatbox(chatboxai.app)。
