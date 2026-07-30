# Provider/Model 统一管理 — 后续任务实施方案

> 关联 ADR:[0005-unified-provider-model-credential-management](../adr/0005-unified-provider-model-credential-management.md)
> 分支:`dev` · 创建日期:2026-07-30 · 状态:方案待评审

本文档为 ADR-0005 在 P0–P2 完成(提交 `51941b2d`…`b64b920a`)后的 5 个后续任务提供可执行的实施方案。每个任务包含:目标、现状、范围、设计、实施步骤(带文件锚点)、风险与缓解、验收标准、工作量估算、依赖。

---

## 优先级与依赖矩阵

| 任务 | 标题 | 优先级 | 估时 | 依赖 | 建议时机 |
|------|------|--------|------|------|----------|
| 3 | Phase 4 — 配置持久化 + 变量替换 | 🟡 中 | 1–1.5 天 | 无 | 可立即开始(收尾当前 sprint) |
| 5 | 连接探针下沉到 engine | 🟡 中 | 0.5–1 天 | 无 | 可独立做,或并入任务 4 |
| 7 | `settings.toml` + `small_model` | 🟢 低 | 1 天 | 任务 3 | 任务 3 之后 |
| 6 | `/provider health` 增强 | 🟢 低 | 0.5–1 天 | 无 | 任意时机 |
| 4 | Phase 2 — Desktop 重新平台化 | 🟠 大 | 4–5 天 | 任务 5(建议先做) | 独立 sprint,需 Desktop 采用数据后再启动 |

**建议执行顺序**:`3 → 5 → 7`(engine 侧收口,均小步、低风险)→ `6`(独立增强)→ `4`(独立大 sprint,启动前确认 ROI)。

任务 5 是任务 4 的推荐前置:先把探针下沉到 engine,Desktop 重平台化时直接复用,避免在旧 desktop 代码里再写一遍。

---

## 任务 3 — Phase 4:配置持久化 + 变量替换

**关联 ADR**:Phase 4 · **优先级**:🟡 中 · **估时**:1–1.5 天

### 目标

让 `/config set` 的写回覆盖完整的 v2 结构化配置,并在配置加载层支持 `{env:VAR}` / `{file:path}` 变量替换(对齐 OpenCode),使敏感值可以"引用"而非内联——强化 A1(配置永不存明文)。

### 现状(已核实)

- ✅ **Tier 持久化**:`/model --tier --save` 通过 `ProviderConfigStore::set_active` 写 `active_target` 到 `~/.shannon/providers.toml`,重启读回(测试 `store_set_active_survives_save_load_cycle`)。
- ✅ **Flat-key 写回 `config.toml`**:`/config set` 已通过 `shannon_core::config_persist::set_global_config_key` 写回 `~/.shannon/config.toml`,`is_writable_key` 白名单拒绝 secret 类 key(A1 合规)。见 `crates/shannon-ui/src/repl/commands/config.rs:744-761`。
- ✅ **Connected profile 写回**:`/connect` 通过 `provider_config_store` 写 `providers.toml`(`CredentialRef::Store`),`ConfigBuilder::load_connected_profile` 读回。
- ❌ **v2 `ProviderModelConfig` 结构化写回 via `/config`**:flat-key 已工作,但 `/config` 尚未覆盖完整 v2 profile 字段(quirks、tiers、auxiliary 等)的结构化编辑。**需在 kick-off 时核实** `provider_config_store` 是否已暴露足够 setter;若已覆盖,本任务收窄为仅"变量替换"。
- ❌ **`{env:VAR}` / `{file:path}` 替换**:`ConfigBuilder` 加载 TOML 后对字符串值无替换(`unified_config.rs` 无 substitut 逻辑)。

### 范围

**In Scope**
1. 在 `unified_config::ConfigBuilder` 的 TOML 加载层(`load_global_toml` / `load_local_toml` / `load_connected_profile`)对字符串字段做 `{env:VAR}` 与 `{file:path}` 单遍替换。
2. (视核实结果)补全 `/config set` 对 v2 `ProviderModelConfig` 结构化字段的写回。

**Out of Scope**
- 新增配置 DSL 或条件逻辑(`{if …}`)。仅做值替换。
- 运行时热重载(替换在加载时一次性完成)。

### 设计

**替换语义**(参考 `notifier.rs:895 substitute_single_pass` 的单遍防御模式):

| 语法 | 解析为 | 缺失时 |
|------|--------|--------|
| `{env:VAR}` | 环境变量 `VAR` 的值 | 留空字符串 + 加载告警(不阻塞启动) |
| `{file:/abs/path}` 或 `{file:~/.shannon/x}` | 文件内容(trim 后) | 留空 + 告警 |
| `{env:VAR:-default}` | `VAR`,缺失则 `default` | `default` |

**安全约束**:
- **单遍替换**:替换结果不再扫描(防止 `{env:X}` 的值本身含 `{env:Y}` 造成递归/注入)。
- **`file:` 路径限制**:解析后必须落在 `~/.shannon/` 下或绝对路径,拒绝相对路径穿越(`../`)。
- **仅替换字符串值**:数组/表/数字不动;递归进入嵌套 table 的字符串叶子。
- **A1 强化**:`api_key` 类字段强烈推荐用 `{env:…}` / `{file:…}` 引用;`is_writable_key` 白名单保持拒绝明文 secret 写入。

**放置点**:在 `ConfigBuilder` 内新增私有方法 `fn substitute_config(input: &mut ShannonConfig)`,在三个 `load_*_toml` 之后调用。或更低层:在读取 TOML 文本后、`toml::from_str` 前对文本做替换(更简单,但会误替换非字符串段——**不推荐**;推荐解析后对值替换)。

### 实施步骤

1. 在 `crates/shannon-core/src/unified_config.rs` 新增 `fn substitute_value(v: &mut toml::Value)`(递归,单遍,支持上表三种语法)+ 单元测试(env 存在/缺失/default、file 存在/缺失/路径穿越拒绝、单遍不递归)。
2. 在 `ConfigBuilder::load_global_toml` / `load_local_toml` / `load_connected_profile` 解析后调用替换。
3. (视核实)`provider_config_store` 暴露 v2 字段 setter;`/config set providers.anthropic.tiers.standard=X` 路径写回。
4. 文档:`docs/configuration.md` 增"变量替换"小节 + 示例。

### 风险与缓解

| 风险 | 缓解 |
|------|------|
| 替换递归注入 | 强制单遍;替换结果不再扫描 |
| `file:` 路径遍历 | 路径规范化后必须以 `~/.shannon/` 起始或为绝对路径;测试 `../../../etc/passwd` 被拒 |
| 现有用户的字面 `{` 值被误替换 | 仅识别 `{env:` / `{file:` 前缀;裸 `{` 不动。文档明示保留字语法 |
| 启动时 env 缺失静默留空 → 难排查 | 缺失时 `tracing::warn!` 记录 key+来源;CI 测试覆盖告警路径 |

### 验收标准

- `/config set base_url {env:MY_URL}` → 重启后 engine 实际请求 `MY_URL` 的值。
- `{file:~/.shannon/endpoints/anthropic}` 引用的文件内容被正确注入。
- `../` 路径被拒并有错误信息。
- 替换为单遍:`{env:X}` 的值 `"{env:Y}"` 最终是字面 `{env:Y}`,不是 `Y` 的值。
- 单元测试 + `just test` 通过;CI 门禁 `cargo clippy --workspace -- -D warnings` 通过。

---

## 任务 4 — Phase 2:Desktop 重新平台化

**关联 ADR**:Phase 2 · **优先级**:🟠 大 · **估时**:4–5 天 · **依赖**:建议先完成任务 5

### 目标

让 Desktop(Tauri Rust + React/TS)采纳 engine 的 `ProviderProfile` + `CredentialRef` 作为**唯一** provider 抽象,`list_models` 改走 `model_registry`,消除与 CLI 并行的旧存储,一次性补齐 P2-9 识别的全部平价缺口(定价/白名单/动态刷新/tier)。

### 现状(已核实,P2-9 评估)

- Desktop `list_models`(`desktop/src/commands_chat.rs:27`)是**硬编码 `match` provider 字符串**,返回手写 `ModelInfo` 字面量,与 `model_registry`/定价 SSOT/动态目录/白名单零关联。
- Provider 存于并行 `ProviderConnection` / `providers.json`(`desktop/src/config.rs:311 load_providers`),非 engine 的 `ProviderProfile` / `~/.shannon/providers.toml`。
- ✅ Desktop **已覆盖**连接探针/健康(`test_provider_connection` + `ping_provider`),等价于 CLI P0-3/P2-8。
- ❌ Desktop **未覆盖**:定价(P0-1/P0-2)、白名单(P1-5)、动态刷新(P1-6)、tier(P2-7)。

### 范围

**In Scope**
1. Desktop provider 数据模型迁移:`ProviderConnection` → engine `ProviderProfile`(字段近子集,迁移成本低)。
2. `list_models` 改为调用 `shannon_core::model_registry`(catalog + 动态 overlay),返回带定价/context 的 `ModelInfo`。
3. Provider 写操作(`save_provider` / `delete_provider` / `set_active_provider`)走 engine 统一 store(`provider_config_store`),`~/.shannon/providers.toml` 成为唯一源。
4. Desktop `api_key` 移出 `providers.json` → 共享 `~/.shannon/credentials/<service>.json`(0600),config 仅存 `CredentialRef`(P3-1,A1)。
5. 复用任务 5 下沉后的 engine 探针,删除 Desktop 的 `provider_probe_url` / `ping_provider`。
6. React UI 类型(`ui/src/types/index.ts`)与 provider/model 页面对齐新 schema。

**Out of Scope**
- Desktop 功能扩张(仅做平价,不加新功能)。
- Gateway multiplex 路由(B3,默认 off,独立)。

### 设计

**分层迁移**(降低风险):

1. **数据层(只读先行)**:新增 Desktop 命令 `list_models_v2` 走 `model_registry`,与旧 `list_models` 并存;UI 加 feature flag 切换。验证定价/白名单/动态刷新正确后再删旧。
2. **写层**:provider CRUD 改走 `provider_config_store`;`set_active_provider` 同时写 `providers.toml` 的 `active_target`。
3. **凭据迁移**:启动时一次性把 `providers.json` 中的明文 `api_key` 迁到 `credentials/<service>.json`,原位替换为 `CredentialRef::Store`;迁移后 `providers.json` 不再含明文。
4. **统一读**:Desktop 启动用 `ConfigBuilder`(connected layer)读 active provider,与 CLI 同源。

**关键文件锚点**:
- `desktop/src/commands_config.rs`(`switch_provider:355`、`test_provider_connection:613`、`list_providers:862`、`save_provider:873`、`set_active_provider:927`)
- `desktop/src/config.rs`(`providers_path:305`、`load_providers:311`、`save_providers:320`)
- `desktop/src/commands_chat.rs`(`list_models:27`)
- `ui/src/types/index.ts`、`ui/src/context/AppContext.tsx`、`ui/src/lib/tauri-api.ts`
- engine 侧:`shannon_types::provider_config::ProviderProfile`、`shannon_core::provider_config_store`、`shannon_core::model_registry`

### 实施步骤

1. **任务 5 先行**:把 `provider_probe_url` / `ping_provider` 下沉到 engine。
2. 数据模型对齐:在 Desktop 引入 `ProviderProfile` 视图类型,`ProviderConnection` 转为它的薄包装/迁移桥。
3. `list_models_v2`:调 `model_registry::merged_models_for_provider` + `pricing_for_model_opt`,映射到 UI `ModelInfo`(含 `price_in`/`price_out`/`context_window`,未知则为 `None`)。
4. provider CRUD 走 `provider_config_store`;`active_provider_id` → `active_target`。
5. 凭据迁移:启动迁移函数(幂等,已迁移则跳过)+ 测试。
6. UI:类型 + 页面 + i18n(`ui/src/i18n/{en,zh-CN}.json`,两文件同改)。
7. 删除旧 `list_models` 硬编码与 Desktop probe 副本。

### 风险与缓解

| 风险 | 缓解 |
|------|------|
| 现有用户 `providers.json` 数据迁移失败/丢失 | 幂等迁移 + 迁移前备份 `.bak`;集成测试覆盖迁移;失败回退 |
| 明文 api_key 迁移期间的安全窗口 | 迁移原子化:写 credentials → 验证可读 → 才删 config 明文;迁移后立即 `shred`/覆盖 |
| React UI 类型变更破坏构建 | 分层 + feature flag;`pnpm lint` + `pnpm test`(80% 覆盖门禁)逐层验证 |
| Desktop 与 CLI 行为不一致(如默认 model) | 统一默认 model id 与 provider kind 集(ADR P4-1/P4-2);共享 engine 实现 |
| 工作量超估 | 分 sprint:数据层(读)→ 写层 → 凭据 → UI,每层可独立合并 |

### 验收标准

- 在 Desktop 切换 provider 后,`shannon` CLI 读到**同一** active provider(`providers.toml` 唯一源)。
- Desktop `list_models` 显示的定价/context 与 CLI picker 一致(同 `model_registry`)。
- `providers.json` 不再含明文 `api_key`(grep 验证);`credentials/<service>.json` 存在且 0600。
- `SHANNON_ENABLED_PROVIDERS` 在 Desktop picker 同样生效。
- 只剩一个连接测试实现(engine);Desktop 不再有自己的 probe 副本。
- Desktop `pnpm test`/`pnpm lint` + Rust `just test` 通过。

---

## 任务 5 — 连接探针下沉到 engine

**关联 ADR**:Phase 2 前置 / P4-3 / P2-2 · **优先级**:🟡 中 · **估时**:0.5–1 天 · **无依赖**

### 目标

把 Desktop 的 per-provider 连接探针(`provider_probe_url` / `ping_provider`)下沉到 engine,使 CLI 与 Desktop 共用一套实现。这是任务 4 的推荐前置,也可独立交付。

### 现状(已核实)

- Desktop:`provider_probe_url`(`desktop/src/commands_config.rs:561`)、`ping_provider`(`:637`)、`validate_base_url`(`:516`)、`is_known_kind`(`:702`)。per-provider 构造探针 URL(如 Ollama `/api/tags`、其它 provider 的 models 端点)。
- Engine:已有 `LlmClient::validate_connection`(`crates/shannon-engine/src/api/client.rs:901`,发 "ping" 消息探针)与 `QueryEngine::probe_active_health`(`engine.rs:878`,复用活跃 client 的 key)。两者都是**发消息**探针,非 per-provider 轻量端点探针。

**Gap**:engine 缺一个 per-provider 的**轻量端点探针**(命中 `/models` 或 `/api/tags`),这正是 Desktop `provider_probe_url` 实现的。下沉 = 把这段逻辑搬到 engine,双方复用。

### 范围

**In Scope**
1. engine 新增 per-provider 轻量探针(给定 provider kind + key + base_url,命中 provider 特定端点验证可达 + 凭据有效)。
2. Desktop `test_provider_connection` 改调 engine 实现;删除 Desktop 的 `provider_probe_url` / `ping_provider`。
3. CLI `/connect` 与 `/provider health` 可选复用(发消息探针与端点探针互补,不强制统一)。

**Out of Scope**
- 改变现有 `validate_connection`/`probe_active_health` 语义(保持不变,新探针并存)。

### 设计

在 `crates/shannon-engine/src/api/client.rs`(或新模块 `probe.rs`)新增:

```rust
/// Per-provider lightweight endpoint probe (hits /models or /api/tags rather
/// than sending a chat message). Validates reachability + credential without
/// a billable token. Mirrors desktop's former provider_probe_url/ping_provider.
pub async fn probe_provider_endpoint(
    provider: LlmProvider,
    api_key: &str,
    base_url: Option<&str>,
) -> Result<(), ApiError> { /* … */ }
```

`provider_probe_url` 的 provider→URL 构造逻辑(Anthropic/OpenAI/Ollama/…)整体迁入,`ping_provider` 的 HTTP GET + 状态码判定迁入。Desktop `test_provider_connection`(`commands_config.rs:613`)改为转发调用。

### 实施步骤

1. engine 新增 `probe_provider_endpoint` + 迁入 URL 构造表 + HTTP 探测逻辑。
2. 单元/集成测试:每 provider kind 的 URL 构造 + mockito 可达/不可达/401。
3. Desktop `test_provider_connection` 改调 engine;删除 `provider_probe_url`/`ping_provider`(保留 `validate_base_url`/`is_known_kind` 若 engine 未覆盖)。
4. Desktop 测试更新。

### 风险与缓解

| 风险 | 缓解 |
|------|------|
| provider→URL 构造迁移漏 case | 先列全 provider kind 表,逐 kind 测试;对照 Desktop 现有测试 |
| engine 依赖变化 | 探针仅用现有 `reqwest`(已在 client.rs 用);无新重度依赖 |
| Desktop 行为回归 | 保留 Desktop 现有测试断言,改为经 engine 路径 |

### 验收标准

- 只剩一个 per-provider 探针实现(engine)。
- Desktop `test_provider_connection` 行为不变(通过既有测试)。
- engine 新增探针对每 provider kind 有测试覆盖。

---

## 任务 6 — `/provider health` 增强

**关联 ADR**:Phase 6 增强 · **优先级**:🟢 低 · **估时**:0.5–1 天 · **无依赖**

### 目标

当前 `/provider health` 仅探活**活跃** provider。增强为:并发探活所有已配置 provider,并在活跃 provider 不可达时提示切换候选。**自动 failover 仍是明确非目标**(Shannon 不内置 model router,spec §11)。

### 现状

- `QueryEngine::probe_active_health`(`engine.rs:878`):探活活跃 client。
- `handle_provider_health`(`config.rs:276`):探活活跃 provider + inventory 其它 provider 的**凭据状态**(不探活)。

### 范围

**In Scope**
1. engine 新增 `probe_all_health`(并发探活所有 `available_providers()`,带每-provider 超时)。
2. `/provider health` 输出全部 provider 的实时可达性表格(非仅活跃)。
3. 活跃 provider 不可达时,在输出中标注"建议切换"候选(已配置且有有效凭据的 provider),**不自动切换**。

**Out of Scope**
- 自动 failover / 路由。
- 后台周期性健康监控(可作为后续独立任务)。

### 设计

```rust
/// Concurrently probe all allowed providers (lightweight endpoint probe from
/// task 5, or message probe). Returns per-provider verdict with bounded latency.
pub async fn probe_all_health(&self) -> Vec<ProviderHealth> { /* join_all + per-probe timeout */ }
```

`ProviderHealth { provider, status: Reachable|Unreachable|AuthFailed|NotConfigured, latency_ms: Option<u32> }`。并发用 `futures::future::join_all` + `tokio::time::timeout` 包每个探针(如 5s),避免一个慢 provider 拖住整表。输出格式:

```
Provider health:
  anthropic  ● reachable  (142ms)  *
  openai     ○ auth rejected
  ollama     ○ unreachable (timeout)
建议:活跃 provider anthropic 不可达时可切换至 openai(/provider openai)。
```

### 实施步骤

1. engine `probe_all_health` + `ProviderHealth` 类型(依赖任务 5 的端点探针,或退化为消息探针)。
2. `handle_provider_health` 改用 `probe_all_health`;活跃不可达时计算并显示切换候选。
3. 测试:多 provider mockito(混合可达/401/超时);候选提示逻辑。

### 风险与缓解

| 风险 | 缓解 |
|------|------|
| N 个并发请求慢/耗资源 | 每 provider 严格超时(5s);`join_all` 并发但有上限;只探已配置 provider |
| 未配置 key 的 provider 探活报噪声 | `NotConfigured` 状态区分,不当作故障 |
| 误报(瞬时网络抖动) | 报告定位为"即时快照",非持续状态;文档说明 |

### 验收标准

- `/provider health` 显示全部已配置 provider 的可达性 + 延迟。
- 活跃 provider 不可达时显示切换候选(不自动切换)。
- 单个慢 provider 不阻塞整表(超时生效)。
- 测试覆盖三种状态 + 候选逻辑。

---

## 任务 7 — `settings.toml` 持久化 + `small_model` 字段

**关联 ADR**:Phase 5 遗留 · **优先级**:🟢 低 · **估时**:1 天 · **依赖**:任务 3

### 目标

交付 ADR-0005 Phase 5 标记为 deferred 的两项:`settings.toml` 持久化变体,以及 `small_model` 字段(轻量辅助模型角色)。

### 现状

- ADR-0005 Phase 5(`docs/adr/0005-…md:352-353`)明确:"`settings.toml` persistence variant and the `small_model` field remain **deferred**"。
- `shannon-types/src/provider_config.rs` 已有 `AuxRole` 枚举:`Vision`/`WebExtract`/`Compression`/`TitleGeneration`/`SessionSearch`,以及 `ModelProfile.auxiliary: HashMap<AuxRole, ActiveTarget>`。
- **关键洞察**:`small_model` 的语义(压缩/标题生成等轻量任务)与现有 `AuxRole::Compression` / `TitleGeneration` **高度重叠**。直接新增 `small_model` 字段会制造两套表达同一概念的冗余。

### 范围

**In Scope**
1. 评估并决定:`small_model` 复用 `AuxRole`(推荐)还是新增独立字段。
2. `settings.toml` schema 与加载/写回路径(区别于 `providers.toml` 的 profile 存储)。

**Out of Scope**
- 完整 auxiliary 模型路由执行逻辑(仅做配置 + 解析)。

### 设计

**决策点(需评审拍板)**:

- **方案 A(推荐)**:不新增 `small_model` 字段。`small_model` 作为 `auxiliary[Compression]` / `auxiliary[TitleGeneration]` 的别名/糖,在加载层把旧 `small_model` key 映射到 `auxiliary`。复用已有 schema,零新字段。
- **方案 B**:新增 `small_model: Option<ActiveTarget>` 到 `ProviderProfile`。语义重叠风险,仅在确认 `small_model` 有独立于 `AuxRole` 的用途时采用。

**`settings.toml` 定位**:存放跨 provider 的用户偏好(tier 偏好默认、通知、UI 等),与 `providers.toml`(provider/profile 定义)分离。加载层 `ConfigBuilder` 新增 `load_settings_toml`。若评估发现现有 `~/.shannon/config.toml` 已能承载这些偏好,则 `settings.toml` 可能不必要——**kick-off 时需核实,避免新增冗余存储**。

### 实施步骤

1. **决策记录**:在 ADR-0005 Phase 5 增"small_model ↔ AuxRole 映射"决策(方案 A/B)。
2. (方案 A)加载层 `small_model` → `auxiliary[Compression]` 映射 + 测试。
3. (若必要)`settings.toml` schema + `load_settings_toml`;否则记录"config.toml 已覆盖,无需 settings.toml"。
4. 文档更新。

### 风险与缓解

| 风险 | 缓解 |
|------|------|
| `small_model` 与 `AuxRole` 语义重叠致混淆 | 优先方案 A(复用);方案 B 需明确独立用途论证 |
| `settings.toml` 与 `config.toml`/`providers.toml` 职责重叠 | 先核实现有存储覆盖范围,避免新增冗余 |
| 破坏现有 auxiliary 用法 | 映射只增不改;现有 `auxiliary` 测试保持绿 |

### 验收标准

- `small_model` 配置(无论方案 A/B)可设置、读回、用于辅助任务路由。
- 存储职责清晰(无冗余 toml)。
- ADR-0005 Phase 5 记录决策;deferred 项变为 ✅ 或明确取消。
- 测试通过。

---

## 跨任务原则

- **CI 门禁**:每个任务的代码提交前必须通过 `cargo fmt --all -- --check` && `cargo clippy --workspace -- -D warnings`(lib+bin,勿加 `--tests`/`--all-targets`);Desktop 改动额外过 `pnpm lint` + `pnpm test`。
- **提交规范**:`<type>(<scope>): <subject>`,types: feat/fix/refactor/test/docs;每个任务独立提交。
- **A1 红线**:明文 secret 永不进 config 文件;只进 `~/.shannon/credentials/<service>.json`(0600);`is_writable_key` 白名单是防线。
- **Tier 红线**:只有 `fast`/`standard`/`pro` 持久化;`auto` 与所有别名(haiku/sonnet/opus/flash/…)均为 input-only。
- **非目标红线**:不内置 model router / 自动 failover(spec §11);健康检查与路由仅信息性。
