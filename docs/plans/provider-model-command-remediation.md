# `/connect` · `/model` · `/provider` 命令整改方案

> 关联 ADR:[0008-provider-model-command-architecture-remediation](../adr/0008-provider-model-command-architecture-remediation.md)
> 分支:`dev` · 创建日期:2026-08-01 · 状态:方案待评审

本方案是 ADR-0008 的可执行落地清单。ADR 只记录 4 个跨 crate、难回滚的架构决策;**所有 bug 修复与常规重构都收敛在本文件**,按 P0→P3 分阶段,每条带文件锚点、影响、方案、验收、估时。证据来自对 `config.rs`、`mod.rs`、`select.rs`、`status_card.rs`、`status_bar.rs`、`chat.rs`、`model_registry.rs`、`provider_resolver.rs`、`commands_providers.rs` 的实际通读。

---

## 结论(TL;DR)

核心流程可用,安全设计(密钥 0600 落盘、`redact_secret_command` 脱敏、fail-soft 校验)扎实。但有三类问题:**3 个真 Bug**(P0)、**产品闭环缺口**(P1)、**架构债**(P2/P3)。P0 的根因正是 ADR-0008 决策 1+2 要解决的「重复与漂移」,因此 P0 不打补丁,而是顺着 ADR 落地。

---

## 优先级与依赖矩阵

| 阶段 | 编号 | 标题 | 优先级 | 估时 | 依赖 | 对应 ADR 决策 |
|------|------|------|--------|------|------|---------------|
| P0 | P0-1 | 接通 StatusCard tier | 🔴 必修 | 0.5 天 | — | 决策 1 |
| P0 | P0-2 | 切换命令刷新首屏卡片 | 🔴 必修 | 0.5 天 | P0-1 | 决策 2 |
| P0 | P0-3 | 统一 tier 判定(消除三套) | 🔴 必修 | 0.5 天 | P0-1 | 决策 1 |
| P1 | P1-1 | `/connect` 即时生效 | 🟠 高 | 1 天 | — | 决策 4 |
| P1 | P1-2 | 统一连接状态词汇 | 🟠 高 | 0.5 天 | — | — |
| P1 | P1-3 | `/model refresh` 后台化 | 🟠 高 | 0.5 天 | — | — |
| P1 | P1-4 | 新增 `/disconnect` | 🟡 中 | 0.5 天 | — | 决策 3 |
| P1 | P1-5 | `/profile` 命名澄清 | 🟡 中 | 0.5 天 | — | — |
| P1 | P1-6 | i18n 收口(connect/provider) | 🟡 中 | 1 天 | — | — |
| P1 | P1-7 | `/model <id>` 校验提示 | 🟡 中 | 0.3 天 | — | — |
| P2 | P2-1 | 抽 `apply_model_selection` | 🟡 中 | 1 天 | P0-2 | 决策 2 |
| P2 | P2-2 | 合并 `connected_provider_slugs` | 🟢 低 | 0.2 天 | — | 决策 3 |
| P2 | P2-3 | provider 身份单一数据源 | 🟡 中 | 1 天 | — | 决策 1 |
| P2 | P2-4 | `--tier`/`--max-tokens` 正式 parser | 🟡 中 | 0.5 天 | — | — |
| P2 | P2-5 | REPL+CLI 配置服务合并 | 🟠 高 | 2 天 | P1-4 | 决策 3 |
| P2 | P2-6 | `catch_unwind` 加日志/根治 | 🟡 中 | 0.5 天 | P2-1 | — |
| P2 | P2-7 | 首屏用 merged 模型清单 | 🟢 低 | 0.2 天 | — | — |
| P2 | P2-8 | 拆分超大文件 | 🟢 低 | 1 天 | P2-1 | — |
| P3 | P3-1 | 首屏渲染缓存(去每帧磁盘 I/O) | 🟢 低 | 0.3 天 | — | — |
| P3 | P3-2 | 删/实现 `entered_via_connect` | 🟢 低 | 0.2 天 | — | — |
| P3 | P3-3 | 超时常量集中 | 🟢 低 | 0.2 天 | — | — |
| P3 | P3-4 | 拆 `apply_connect` 六步 | 🟢 低 | 0.5 天 | P2-5 | — |
| P3 | P3-5 | 简化 `--max-tokens` clear 语义 | 🟢 低 | 0.2 天 | — | — |

**建议执行顺序**:
`P0-1 → P0-3 → P0-2`(1.5 天,先杀三个真 Bug,顺着决策 1+2 走)→
`P1-1/P1-2/P1-3`(用户体感,1~2 天)→
`P2-1`(回收四份重复,同时为后续铺路)→
`P2-3 → P2-5`(provider 身份与配置服务,架构收口)→
其余 P1/P2/P3 滚动。

---

## P0 — 真 Bug(必修,用户可见)

### P0-1 首屏 StatusCard 的 tier 永远是 `?`

**证据**:`ChatWidget::set_active(.., tier)` 的两个调用点都传 `None`:
- `crates/shannon-ui/src/repl/mod.rs:391` → `None, // tier label resolved in Task 14`
- `crates/shannon-ui/src/repl/mod.rs:1384` → 同样 `None`

`active_tier` 字段初始化为 `None`(`widgets/chat.rs:209`),除这两处外**再无人写入**;渲染端 `widgets/status_card.rs:81,146` 用 `tier.unwrap_or("?")`。

**影响**:欢迎页 `Tier: [?]` 永远是问号。"Task 14" 没落地。

**方案**:走 ADR 决策 1 —— 加 `model_registry::tier_label_for(model_id, provider) -> Option<TierLabel>`(基于 `ModelCapabilities`/`ModelTier`,非子串),在 `set_active` 调用处计算并传入。

**验收**:
- [ ] 首屏卡片对 catalog 内模型显示真实 tier(fast/standard/pro)。
- [ ] 新增单测:`tier_label_for` 覆盖 haiku/sonnet/opus/flash/mini 等代表模型。
- [ ] 现有 `status_card.rs` 三条渲染单测仍通过。

**估时**:0.5 天

---

### P0-2 首屏 StatusCard 不反映 `/connect`/`/model`/`/provider` 的切换

**证据**:`set_active` 只在 REPL init(385)和 resume(1384)调用。`handle_model`、`handle_provider`、`apply_connect`、`handle_model_tier` 都改了 `repl.state.model`/`selected_provider`,但**都没调 `repl.chat.set_active(...)`**。

**影响**:用户在空白首屏执行 `/connect anthropic sk-...`,关掉模型选择器后,卡片仍显示启动时的旧 provider/model。

**方案**:走 ADR 决策 2 —— 抽 `apply_model_selection`(见 P2-1),在其中统一调 `set_active`。P0-2 作为该函数的最小可用切片先落地(可暂不合并全部四处,但 `apply_connect` 与 `handle_model` 必须接上)。

**验收**:
- [ ] 空白首屏执行 `/model <id>` 后,卡片的 provider/model/tier 立即更新。
- [ ] 空白首屏执行 `/connect <p> <key>` 关掉 picker 后,卡片显示新连接。
- [ ] `set_active` 不再只在 init/resume 调用。

**估时**:0.5 天(依赖 P0-1 的 tier 函数)

---

### P0-3 tier 判定存在三套互不一致的逻辑

**证据**:

| 位置 | 方法 | 来源 |
|---|---|---|
| StatusBar 胶囊 | 子串启发式 `tier_label_for`(`status_bar.rs:36-54`) | id 含 haiku/flash/mini… |
| StatusCard | `active_tier`(恒 `None`,见 P0-1) | 未接线 |
| `/model --tier` + 注册表 | `resolve_tier` 走 `ModelCapabilities`(`model_registry.rs:1331`) | 权威 |

**影响**:状态栏胶囊可能把 `o3-mini` 判成 `fast`(含 mini),与 `--tier` 解析和 catalog 真实能力不符。

**方案**:StatusBar 的子串 `tier_label_for` 改调 ADR 决策 1 的统一函数;子串启发式作为「catalog 外的动态/自定义模型」的最后兜底保留(返回 `None` 时降级)。

**验收**:
- [ ] StatusBar 胶囊 tier 与首屏卡片 tier 对同一模型一致。
- [ ] 删除 `status_bar.rs:36-54` 的纯子串实现(或仅保留为兜底分支并注释)。
- [ ] `status_bar.rs::tier_label_for_classifies_models` 单测更新并通过。

**估时**:0.5 天(依赖 P0-1)

---

## P1 — 产品 / UX 闭环

### P1-1 `/connect` 成功后提示「restart shannon to apply the new credential」

**证据**:`config.rs:706` `"✓ Switched to ... (restart shannon to apply the new credential)"`;`config.rs:651-653` 注释承认「当前 client 保留启动时的 credential」。

**影响**:用户刚 `✓ Credential verified`,紧接着被告知要重启——首启体验割裂。竞品(Claude Code / Codex)连接即生效。

**方案**:走 ADR 决策 4 —— engine 加 `reload_credential(service)`,`apply_connect` 在探测成功后调用;删除「restart」提示。若该 provider 还需切换 base URL,降级为「重建 client」并明确提示(见 ADR Open Questions 1)。

**验收**:
- [ ] `/connect <p> <key>` 后立即用新 key 可发消息,无需重启。
- [ ] 「restart」字样从成功提示移除。
- [ ] 新增 engine 层 `reload_credential` 单测。

**估时**:1 天

---

### P1-2 provider 连接状态有三套词汇

**证据**:
- `/connect` dashboard(`config.rs:464`):`no auth` / `✓ connected` / `key stored` / `no key`
- StatusCard(`status_card.rs`):`●`/`○` + `X connected / Y supported`
- `/provider`(`config.rs:198-202`):`key OK` / `no key` / `no auth`

**影响**:`key stored` vs `key OK` 含义不同,用户在三个入口间对照时困惑。

**方案**:定义 `ProviderConnectionStatus` 枚举(`NoAuth`/`Connected`/`KeyStored`/`NoKey`)+ 统一 `Display`,三处共用。`/provider` 的 `key OK` 对齐为 `Connected`/`KeyStored` 之一。

**验收**:
- [ ] 三处状态词来自同一枚举的 `Display`。
- [ ] `/connect`、`/provider`、首屏卡片对同一 provider 显示同一词。
- [ ] 枚举纯函数单测覆盖四种分支。

**估时**:0.5 天

---

### P1-3 `/model refresh` 阻塞 UI 线程,与 `/connect` 后台 refresh 相悖

**证据**:`handle_model_refresh`(`config.rs:113-122`)用 `block_on` **串行**跑 catalog(`DEFAULT_FETCH_TIMEOUT` = **15s**,`dynamic.rs:40`)+ LiteLLM pricing,最长卡 ~30s。而 `apply_connect`(`config.rs:717`)是 `runtime.spawn` 非阻塞 5s。

**影响**:用户主动 `/model refresh` 反而冻住界面;两条 refresh 路径行为相反、超时不一致。

**方案**:`/model refresh` 改成 `runtime.spawn` + 进度提示(复用 ADR-0007 已埋的 `connect.refresh_*` i18n key);超时统一(见 P3-3)。

**验收**:
- [ ] `/model refresh` 不阻塞输入;完成后在 chat 输出结果。
- [ ] refresh 超时不再两处各写。
- [ ] 离线时静默回退行为不变。

**估时**:0.5 天

---

### P1-4 缺 `/disconnect`,REPL 配置闭环不完整

**证据**:全局搜不到 `disconnect`;移除 provider 只能去 CLI `shannon providers remove` 或手改 toml。

**方案**:走 ADR 决策 3 —— 加 `/disconnect <provider>`,调 `ProviderConfigService::disconnect`(清 credential 引用 + 从 profile 移除 + 切回首个可用 provider)。

**验收**:
- [ ] `/disconnect <p>` 后,该 provider 在首屏/`/connect` dashboard 变为未连接。
- [ ] 当前 provider 被断开时自动切到下一个已连接 provider(或回到未配置态)。
- [ ] `/help disconnect` 有条目。

**估时**:0.5 天(依赖决策 3 服务存在;可先用现有 store 接口做最小版)

---

### P1-5 `/profile` 一词三义

**证据**:
- permission profile(`.shannon/profiles/*.toml`,`shannon-commands/src/builtin/profile.rs`)
- providers.toml 的 `"default"` **model profile**
- preset 命令把 `profile` 当 `template` 的别名(`preset.rs:299`)

**方案**:provider 侧的「profile」在 UI 文案与代码注释里改称 `connection` 或 `provider-config`;`preset` 的 `profile` 别名标注 deprecated(保留兼容)。文档(配置手册 + help)统一术语。

**验收**:
- [ ] 用户可见文案不再把 provider 配置叫「profile」。
- [ ] 代码注释/文档对齐。
- [ ] `preset` 的 `profile` 别名仍可用但 help 标注已废弃。

**估时**:0.5 天

---

### P1-6 i18n 半完成

**证据**:只有 `commands.model.set` 和 `commands.model.context_unknown` 走 `t!()`(`config.rs:97,148`);`/connect`、`/provider`、dashboard、health、refresh、所有错误提示全是硬编码英文。

**方案**:把 `connect/provider` 两个高频命令的用户可见字符串抽 `t!()`(en + zh 配对);其余命令分批。复用 ADR-0007 已埋的 `connect.*` namespace。

**验收**:
- [ ] `/connect`、`/provider` 全部输出走 `t!()`,en/zh 均有。
- [ ] 切换到 zh 后这两条命令完整翻译。

**估时**:1 天

---

### P1-7 `/model <id>` 不校验模型是否存在

**证据**:`resolve_model_arg`(`config.rs:27-31,69`)对裸 id 直接 `state.model = Some(args.trim())`,catalog 查不到就当字面量存。

**影响**:typo 静默通过,直到 query 时才报错。

**方案**:不在 catalog(及 merged 动态层)时,输出一行 warning(`model 'x' not in catalog; using as-is`),保留 escape hatch(仍允许设置)。

**验收**:
- [ ] `/model typo-id` 出现 warning 且仍设置成功。
- [ ] catalog 内模型无 warning。

**估时**:0.3 天

---

## P2 — 架构 / 可维护性

### P2-1 抽 `apply_model_selection`(四份重复收敛)

**证据**:`handle_model:60-99`、`handle_provider:213-265`、`apply_connect:651-667`、`handle_model_tier:1765-1787` 都做:set state → `set_model_for_provider` → `catch_unwind(pre_resolve)` → resolve ctx → `save_preferences`。

**方案**:见 ADR 决策 2。签名 `apply_model_selection(repl, provider, model_id, tier: Option<TierName>, persist_tier: bool)`;四处复用;内部调 `set_active`(回收 P0-2)。

**验收**:
- [ ] 四处切换不再各自重复 engine 同步/preference 持久化逻辑。
- [ ] 行为对齐(切换后 context_window、preferences、首屏卡片一致)。
- [ ] 新增针对该函数的单测(用测试 double 替换 engine)。

**估时**:1 天(依赖 P0-2)

---

### P2-2 合并 `connected_provider_slugs`

**证据**:`config.rs:478` 与 `chat.rs:755`(注释自述「Mirrors …」)。

**方案**:走 ADR 决策 3 —— 下沉到 `ProviderConfigService::connected_slugs()`,两处复用。

**验收**:
- [ ] 两处内联实现删除,改调统一函数。
- [ ] 行为不变(同一 toml 解析)。

**估时**:0.2 天

---

### P2-3 provider 身份单一数据源

**证据**:`parse_provider_name`(`config.rs:153-186`)25+ arm 手维护 match,重复 `LlmProvider`(Display / `llm_provider_id` / catalog)。

**方案**:见 ADR 决策 1 —— `LlmProvider::from_slug` + 唯一别名表;`parse_provider_name` 与 CLI `parse_kind`(`commands_providers.rs:60`)改调它。**迁移前先把现有所有 arm 提为穷举测试**,防回归。

**验收**:
- [ ] `from_slug` 覆盖现有所有 arm(穷举测试对照)。
- [ ] `parse_provider_name` 缩为薄封装。
- [ ] 加 provider 只需改 1~2 处。

**估时**:1 天

---

### P2-4 `--tier`/`--max-tokens` 改正式 parser

**证据**:`config.rs:38,47` 用 `starts_with` 前缀分发;项目已有 `shannon-commands/src/parser.rs`。`config.rs:44-46` 注释声称「`--tier` 会匹配 `--max-tokens`」是**错误的**(二者无前缀关系),注释误导。`--tierfoo` 会被误路由。

**方案**:改用 `parser.rs` 的 flag 解析;订正/删除错误注释。

**验收**:
- [ ] `--tier`/`--max-tokens`/`--save` 经正式 parser 解析。
- [ ] `--tierfoo` 不再误进 tier handler。
- [ ] 误导注释删除。

**估时**:0.5 天

---

### P2-5 REPL+CLI provider 配置服务合并

**证据**:REPL `/connect` 走 `build_connect_profile`(`provider_resolver.rs:390`);CLI `shannon providers add`(`commands_providers.rs`,1181 行)走 `ProviderKind` upsert;两路径写同一 `providers.toml`,shape 不同。

**方案**:见 ADR 决策 3 —— `shannon-core::ProviderConfigService`,REPL 与 CLI 都改调它。**最大爆炸半径**:依赖现有 `ProviderConfigStore` 测试套件防回归。

**验收**:
- [ ] 两条路径都通过 `ProviderConfigService::connect` 写入。
- [ ] `providers.toml` round-trip 测试全绿。
- [ ] CLI `list-providers` / `providers add` / `providers remove` 行为不变。

**估时**:2 天(依赖 P1-4,建议同 sprint 做)

---

### P2-6 `catch_unwind(pre_resolve_context)` 静默吞 panic

**证据**:`config.rs:85,233,658,1773` 四处。

**方案**:随 P2-1 收敛到 `apply_model_selection` 一处;至少 `tracing::error!` 记录 panic;长期根治 `pre_resolve_context` 的 panic 源。

**验收**:
- [ ] 仅剩一处 `catch_unwind`,且 panic 有日志。
- [ ] (可选)注入一个会 panic 的 double,验证日志输出。

**估时**:0.5 天(依赖 P2-1)

---

### P2-7 首屏用 merged 模型清单

**证据**:首屏 `chat.rs:742` 用裸 `MODEL_CATALOG`;picker(`select.rs:1103`)用 `merged_models_for_provider`(含 models.dev 动态层)。

**影响**:首屏可能显示陈旧模型,picker 显示最新——同一产品两套清单。

**方案**:首屏 `available` 改用 `merged_models_for_provider(p)`。

**验收**:
- [ ] `/model refresh` 后,首屏卡片模型列表与 picker 一致。
- [ ] 静态 catalog 行为不变。

**估时**:0.2 天

---

### P2-8 拆分超大文件 ✅ 已完成

**证据**:`config.rs` 2099 行;`model_registry.rs` 2803 行。

**方案**:`config.rs` 拆成 `commands/model.rs` / `provider.rs` / `connect.rs` / `config_kv.rs` 等(在 P2-1 抽函数之后做,顺势归位);`model_registry.rs` 按静态目录 / 动态层 / tier 解析拆分。

**落地结果**:
- `config.rs` 2444 → **583 行**,拆出 5 个子模块(按命令域):`model.rs` / `provider.rs` / `connect.rs` / `config_kv.rs` / `appearance.rs`;父级保留共享 helper 与全部单测,通过 `pub(crate) use` 再导出 handler,调用点零改动。
- `model_registry.rs` 2803 → **1652 行**,拆出 `catalog.rs`(静态目录 + 类型,792 行)与 `tier.rs`(tier/alias 解析 + `ModelRouter`,389 行),与既有 `dynamic.rs`(动态层)三足鼎立;父级 `pub use` 再导出全部 9 个公开项,`model_registry::resolve_tier` / `::ModelRouter` 等路径不变。
- 两处均按 review 约定**集中保留单测于父级**(`use super::*` glob),故 `model_registry.rs` 父级停在与 config.rs 不同的 ~1650 行(非测试代码约 430 行),未强压到 600 以内。

**验收**:
- [x] 单文件行数降至 ~600 以内(`config.rs` 583;`model_registry.rs` 因集中保留 ~85 条单测停于 1652,非测试代码 ~430,经 review 同意)。
- [x] 模块边界清晰,public API 不变(父级 `pub use` 再导出,10258/10258 工作区测试通过)。

**估时**:1 天(依赖 P2-1)

---

## P3 — 打磨

### P3-1 首屏每帧磁盘 I/O
**证据**:`chat.rs:738,755` 每渲染帧 `available_providers()` + `provider_config_store::load(None)` 解析 toml。
**方案**:状态变更时缓存(provider/model 切换、`/connect`、`/disconnect` 触发刷新),非每帧。**估时**:0.3 天

### P3-2 `entered_via_connect` 死旗标
**证据**:`select.rs:998,1080` 自述「目前仅装饰」。
**方案**:ADR-0007 Open Questions 已留口——要么实现「Esc 回退到 connect 时默认」让它转正,要么删除。建议先删,YAGNI。**估时**:0.2 天

### P3-3 超时常量集中
**证据**:refresh 用 `DEFAULT_FETCH_TIMEOUT`(15s);connect 硬编码 5s(`config.rs:720`);health probe 硬编码 5s(`config.rs:301`)。
**方案**:集中到 `model_registry::dynamic` 或 config 常量,三处引用。**估时**:0.2 天

### P3-4 拆 `apply_connect` 六步
**证据**:`config.rs:601-733` 一函数做存 key/存 profile/切引擎/验证/spawn refresh/开 picker。
**方案**:随 P2-5 拆为 `store_credential` / `persist_profile` / `apply_model_selection` / `validate_credential` / `spawn_refresh` / `open_picker` 步骤函数。**估时**:0.5 天(依赖 P2-5)

### P3-5 `--max-tokens` clear 语义绕
**证据**:`config.rs:1629-1644` —— `0` 和 `clear` 都 map 到 `None`,但裸 `0` 又被单独拒绝要用户改打 `clear`。
**方案**:`0` 直接等于 `clear`(不再报错);仅 `clear` 与正整数两条路径。**估时**:0.2 天

---

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| P2-3 别名表遗漏某个 arm,导致 provider 解析回归 | 迁移前把现有 `parse_provider_name` + `parse_kind` 全部 arm 提为穷举对照测试 |
| P2-5 两条配置路径合并,`providers.toml` 写入 shape 改变 | 依赖现有 `ProviderConfigStore` round-trip 测试;合并后补「REPL 写 → CLI 读」与反向交叉测试 |
| P1-1 热重载 credential 与在飞 query 竞态 | `reload_credential` 定义并发顺序(排队或拒绝);先做 key 替换(轻),base URL 变更降级为重建 client |
| P0 修复动了首屏渲染,可能引出布局回归 | 现有 `status_card.rs` 三条渲染单测 + 新增 tier/刷新用例守门 |

---

## 总估时

- **Phase 1(P0,杀 bug)**:~1.5 天
- **Phase 2(P1,UX 闭环)**:~4 天
- **Phase 3(P2,架构债)**:~6 天
- **Phase 4(P3,打磨)**:~1.5 天
- **合计**:~13 天(可按阶段交付,P0 先合)

---

## 验收门槛(全局)

每个阶段合入前必须:
- [ ] `just dev`(`cargo clippy --workspace -- -D warnings` + `cargo fmt --all -- --check`)clean
- [ ] `just test` 全绿
- [ ] 触及用户可见文案的条目,en/zh locale 同步
- [ ] ADR-0008 Acceptance 清单逐项勾选(架构决策项落地时)
