# P2-2 S0 Spike — ADR-0005 Phase 2 Desktop Re-platforming 收尾路径

> Wave 6 S0 子代理产物 · 状态:草稿(只读仓库,无代码改动)
> 锚定 ADR:`docs/adr/0005-unified-provider-model-credential-management.md`(Phase 2 ✅ done 2026-07-30)、`docs/adr/0008-provider-model-command-architecture-remediation.md`
> 锚定代码:`crates/shannon-core/src/provider_config_service.rs`、`crates/shannon-core/src/provider_config_store.rs`、`desktop/src/commands_config.rs`、`crates/shannon-cli/src/commands_providers.rs`、`crates/shannon-cli/src/main.rs`、`docs/STABILITY.md`、`.github/workflows/ci.yml`

---

## 0. TL;DR

ADR-0005 Phase 2 的**引擎侧架构改造**(单写入路径、`/connect` 热加载、Welcome 重写、`test_all_providers`、DesktopConfig 字段删除)已经在 `feat/provider-model-command-remediation` 分支(HEAD `839c2b92`)上**全部落地**。

真正剩下的"收尾"是**桌面侧的两条尾巴**:

1. **桌面写入路径仍绕过 `ProviderConfigService`**——`land_profile_in_engine_store` 直接调 `ProviderConfigStore::upsert_profile`,而 CLI 的 `run_providers_add` 已经走 `ProviderConfigService::from_store(...).upsert(...)`。这是 Phase 2 显式接受的"deferred",但 ADR-0008 Acceptance 明确"REPL and CLI write `providers.toml` through the same contract",**桌面也是 contract 消费者,必须对齐**。
2. **`~/.shannon/desktop/providers.json` 写穿缓存**——`save_provider` / `delete_provider` 同时写两份(`providers.toml` + `providers.json`),`list_providers` 已经只读 `providers.toml`,但 `providers.json` 还在被 `config::load_providers()` 反复读、写。ADR-0005 Phase 7 已经裁定保留这一形态,直到 `provider_store_read_facade` 单独的 ADR 出现。

**结论**:桌面 re-platforming **不是大改**,而是**两个具体收敛任务** + STABILITY 周期化 + 一份 E2E 测试矩阵。**合并估时 ≤ 1.5 w**(S1 子任务见 §5),与任务书"≤ 2-3w"上限有大量余量。

---

## 1. 双写路径清单(穷尽)

### 1.1 矩阵

| # | 行为 | CLI 路径 | Desktop 路径 | 是否双写 | 计划收敛到 | 风险 |
|---|---|---|---|---|---|---|
| 1 | `connect` / `providers add`(添加/刷新 provider) | `shannon providers add <ID> --kind ... --model ...` → `run_providers_add` → **`ProviderConfigService::from_store(store).upsert(profile, model, true)`** → `store.toml`(ADR-0008 P2-5) | Tauri `save_provider` → `land_profile_in_engine_store` → **`ProviderConfigStore::upsert_profile(profile, model)`**(直接调,不经 Service)+ `providers.json` 写穿 | **半双写**——CLI 走 Service,桌面绕 Service 直调 `ProviderConfigStore`;另外桌面有 `providers.json` 第二副本 | P2-2-A:桌面 `save_provider` / `set_active_provider` / `delete_provider` 改走 `ProviderConfigService::connect` / `upsert` / `disconnect`(已有 `connect`/`upsert`/`disconnect` 三个 API),保证所有写入共用一份"make_active 策略"和"upsert 语义" | 🟡 中——`land_profile_in_engine_store` 的 `flock + save_locked` 写法在 Service 缺位时是必要的(Service 的 `connect/upsert` 内部走 `save()` 也拿 flock)。一旦切到 Service,Service 必须用 `save_locked` 或显式 `acquire_exclusive_lock` 以避免双锁(死锁) |
| 2 | `disconnect` / `providers remove`(删除 provider) | `shannon providers remove <ID>` → `run_providers_remove` → `ProviderConfigStore::remove_profile` + `.save()`(**不走 Service**) | Tauri `delete_provider` → `remove_profile_from_engine_store`(直接调 `ProviderConfigStore::remove_profile` + `save_locked`)+ `providers.json` 重写 + `active_provider_id` 重置 | **半双写**——CLI 与桌面都绕过 Service,但语义一致(`remove_profile` 幂等) | P2-2-B:CLI 改走 `ProviderConfigService::disconnect`,利用其 `DisconnectOutcome { next_active }` 给"删除的是当前活跃 provider"时输出友好的"将回退到 X"提示 | 🟢 低——`disconnect` 是幂等的纯删 |
| 3 | `model`(切换模型 / `--save` 持久化) | REPL `/model <provider>/<model>` + `--save` → `ProviderConfigStore::set_active + save`(**走底层 Store**) | Tauri `configure('model', X)` → `state.provider_store.lock()` → `set_active + save` + `rebuild_client_config_from_store` | 双方都不走 Service,但语义一致 | P2-2-C:REPL 与桌面都改走 `ProviderConfigService::set_active`,统一"`save()` + 可能伴随的 `connected_slugs()` 重读"流程 | 🟢 低 |
| 4 | `tier`(切换 tier) | REPL `/model --tier <fast|standard|pro> [model] --save` → `ProviderConfigStore::set_tier + save` | 无直接对应 Tauri command;Add Provider modal 通过 `ProviderInput.tiers` 写穿 → `apply_provider_update` → `providers.json` + `land_profile_in_engine_store`(整体 `upsert_profile` 替代 profile) | **CLI 与桌面都不走 Service**;语义一致(`ProviderTiers` 同 schema) | P2-2-D:Service 已有 `set_tier` 方法,CLI / 桌面都改走它 | 🟢 低 |
| 5 | `refresh`(刷新可用 provider 列表 / `models.dev` 动态目录) | `/provider refresh` / `/model refresh` → `model_registry::dynamic::refresh_models_dev_cache` | Tauri `list_models`(读 `model_registry::merged_models_for_provider`)——已是只读,不写入 | 双方都不写入 provider 配置;**没有双写** | 不需要改 | ⚪ 无 |
| 6 | `credential` 存储(本地文件 / keyring / env var) | `~/.shannon/credentials/<service>.json`(plaintext 0600,CredentialManager)+ env vars + keyring(opt-in,`CredentialRef::Keyring` stub);`/credentials store/get/delete` CLI 命令 | `~/.shannon/credentials/<id>.json`(同 CredentialManager)+ env vars(Welcome 探测 `ANTHROPIC_API_KEY` 等);**桌面没有 keyring 路径** | **统一** —— 双方都依赖同一个 `shannon_core::credential_manager::CredentialManager` + `read_credential_value_default`,keyring 由 ADR-0005 D3 裁定为 opt-in(opportunistic) | P2-2-E(可选):桌面 Welcome 流可暴露 keyring 切换(API key 旁边一个"Use OS Keychain" 复选框),但需要先行评估 D-Bus 不可用时的降级提示 | 🟢 低;⚪ P3 安全债 |
| 7 | `default` provider / 默认 active target | `default` 这个名字有两层冲突——① `provider_config::ModelProfile { name: "default" }`(providers.toml 内嵌的 profile 桶);② 用户层面的"默认 provider"(由 `active_target` 隐式表达,**没有独立的"default"字段**) | 同上 | **统一** —— 双方都不维护"default provider"概念,只有 `active_target`,由 `ProviderConfigStore::set_active` 写入 | 不需要改 | ⚪ 无,但 `/profile` 命名冲突(ADR-0008 Open Q4)是 UX 任务,非 Phase 2 范围 |
| 8 | `import` / `export` / 多 profile | **不存在** | **不存在** | —— | —— | ⚪ 无;SPEC §11 显式排除多 profile |
| 9 | `ProviderConnection` ↔ `ProviderProfile` 边界(ADR-0005 Phase 7) | 不涉及(CLI 用 `ProviderProfile` 直接) | `to_provider_profile` / `from_provider_profile` 在 `desktop/src/config.rs:272 / 347` 做翻译;`api_key` 字段 `#[serde(default, skip_serializing)]` 仅用于 `migrate_providers_to_credentials` 一次历史迁移 | **统一** —— ADR-0005 Phase 7 已决定保留 near-superset 形态 | 不需要改 | ⚪ 无;`api_key` 字段移除是 future work(ADR-0005 §Phase 7 "Future") |

### 1.2 关键双写点定位

| 写入源 | 文件 | 是否走 `ProviderConfigService` | 是否写 `providers.json` | 备注 |
|---|---|---|---|---|
| REPL `/connect` | `crates/shannon-ui/src/repl/commands/config.rs`(`apply_connect`) | ❌ 不走(直接 `ProviderConfigStore`) | n/a | ADR-0008 决策 3 描述的目标是改走 `ProviderConfigService`,但**实际代码仍走底层 Store**(从 `provider_config_service.rs` 文件注释 + `grep` 结果确认) |
| REPL `/disconnect` | 同上 | ❌ 不走 | n/a | 同上 |
| REPL `/model` / `--tier` `--save` | 同上 | ❌ 不走 | n/a | 同上 |
| CLI `providers add` | `commands_providers.rs:522` `run_providers_add` | ✅ **走** `ProviderConfigService::from_store(...).upsert(...)` | n/a | 唯一合规 |
| CLI `providers remove` | `commands_providers.rs:590` `run_providers_remove` | ❌ 不走(直接 `.save()`) | n/a | 半合规 |
| Desktop `save_provider` | `desktop/src/commands_config.rs:1345` | ❌ 不走 | ✅ 写 | 桌面绕 Service + 写双份 |
| Desktop `set_active_provider` | `desktop/src/commands_config.rs:1454` | � 不走(`land_profile_in_engine_store` 直调 `upsert_profile`)+ ✅ 写 `providers.json` | ✅ 写 | 同上 |
| Desktop `delete_provider` | `desktop/src/commands_config.rs:1429` | ❌ 不走 + ✅ 写 | ✅ 写 | 同上 |
| Desktop `configure('model')` / `configure('provider')` | `commands_config.rs:226-389` | ❌ 不走(直接 `set_active + save`)+ 重算 `client_config` | ❌ 不写(只改 store) | 同上 |
| Desktop `configure('api_key')` / `configure('base_url')` | 同上 | n/a(凭据 → CredentialManager;base_url → `upsert_profile`)| ❌ 不写 | 同上 |

### 1.3 双写冲突 / 漂移点

- **`land_profile_in_engine_store` 与 `providers.json` 写穿**:`save_provider` 先 `config::save_providers(&file)`(写 `providers.json`)→ 然后 `land_profile_in_engine_store` 写 `providers.toml`。中间任何并发 `providers add` 都会让两份文件短暂分歧。**缓解**:`providers.toml` 的 `flock` 保护了 TOML 这一半,但 `providers.json` 不在锁内。
- **`active_provider_id` vs `active_target`**:两份文件各有一个"当前活跃"指针。`providers.json` 的 `active_provider_id` 在 `save_provider` / `delete_provider` / `set_active_provider` 中维护;`providers.toml` 的 `active_target` 在 `upsert_profile` / `set_active` 中维护。`list_providers` 只读 `providers.toml`,所以前端显示与 `providers.json` 字段**可能不同步**(例如 `active_target.provider_id == "anthropic"` 但 `providers.json` 里 `active_provider_id` 仍是 `None`)。**证据**:`set_active_provider` 写完 `providers.toml` 之后才 `file.active_provider_id = Some(id); config::save_providers(&file)`,顺序正确,但 `save_provider` 不维护 `active_provider_id`——意味着"通过 save_provider 新增的 provider 不会自动设为 active",即使 `upsert_profile` 在 `providers.toml` 里把 `active_target` 钉到新 id。
- **CLI `--set-active` 行为**:`run_providers_add` 注释明确写"the flag remains a documented no-op"——`make_active` 永远传 `true`。这与 `ProviderConfigService::upsert(... make_active=true)` 一致,但**桌面 `save_provider` 不主动设置 `active_provider_id`**,所以"用桌面新增 → CLI 启动时发现 active 不对"的边界条件存在。
- **`upsert_profile` 重复设置 `active_target`**:每次 `save_provider` 都会重置 `active_target` 到当前 provider,而不是只在"用户主动激活"时改。CLI 与桌面的语义分歧:CLI 用 `upsert_profile` 也是同样行为,但 Service 的 `make_active=false` 路径(`connect_make_active_false_preserves_prior_selection` 测试覆盖)只被 CLI 的 `--set-active` 显式触发。

---

## 2. STABILITY Deprecation 时间表

### 2.1 候选弃用项(当前公开 surface)

`grep -rn "#\[deprecated" crates/shannon-core/src/` 只返回 `crates/shannon-core/src/testing/mod.rs:15` 一处(`#[deprecated(...)]` 加在 testing 工具函数上,与 Phase 2 无关)。所以**目前还没有任何"准备弃用但还保留"的 Phase 2 旧 API**。这意味着 deprecation cycle 要么:

- (a) 在本 spike 之后**新增** `#[deprecated]` 标记到不再推荐直调的入口,或
- (b) **不新增 `#[deprecated]`**,纯靠 Service 收敛自然淘汰低层 API。

**推荐 (b)**——理由:ADR-0005 D2 + ADR-0008 决策 3 都把 `ProviderConfigService` 设为"the single semantic write path",低层 `ProviderConfigStore::upsert_profile` / `set_active` / `set_tier` / `remove_profile` 已经标注"`save_locked` is for callers that already wrap a load-mutate-save in a flock"(注释明示"low-level"),**意图已经清晰**,不需要再贴 `#[deprecated]`。

### 2.2 推荐的 release schedule(基于 STABILITY §"Deprecation cycle")

| Release | 动作 | CI 门禁 |
|---|---|---|
| **v0.5.6** (本 spike 周期内) | 桌面 `save_provider` / `set_active_provider` / `delete_provider` 改走 `ProviderConfigService::connect`/`upsert`/`disconnect`;CLI `providers remove` 改走 `ProviderConfigService::disconnect`(用 `next_active` 提示);REPL `apply_connect` / `apply_disconnect` / `handle_model` `--save` / `handle_model_tier` 改走 Service。**不新增 `#[deprecated]`**。 | `just test`(已有)+ 新增 `p2-2-desktop-routes-through-service` 测试;`cargo semver-checks` 仍 advisory |
| **v0.6.0**(下一个 minor) | (1) `provider_config.json` 写穿缓存移除:`save_provider` / `delete_provider` 不再写 `providers.json`;`list_providers` 不再保留"stale legacy file"warning 路径;`ProviderConnection` 收缩为 UI-only descriptor(只保留 `id`/`label`/`provider_kind`/`created_at`)。(2) `ProviderConnection.api_key` 字段移除(legacy 早已 `skip_serializing`)。(3) `migrate_providers_to_toml` / `migrate_providers_to_credentials` 转为 no-op(legacy 文件应已无新生产数据)。**首次需要 `cargo semver-checks` required** | `cargo semver-checks --baseline-rev v0.5.6` **required**(见 §2.3) |
| **v0.7.0** | 评估是否需要新增 `ProviderStoreReadFacade` ADR(把 `ProviderConfigStore` 的读访问从 `Mutex<...>` 解耦出来),让 `list_providers` 不必依赖 `state.provider_store.lock().await`。如果需要,在此 release 做。 | `cargo semver-checks --baseline-rev v0.6.0` required |

### 2.3 `cargo-semver-checks` 门禁升级路径

当前 `.github/workflows/ci.yml:269-302` 的 `semver-check` job 是 **advisory**(`continue-on-error: true`,注释明确"Pre-1.0: semver is advisory, not gating")。Phase 2 任务书要求 v0.6.0 起**门禁 required**。需要做的修改:

1. 移除 `continue-on-error: true`,并在分支保护里加为 required check。
2. 同步更新 `STABILITY.md` §"cargo-semver-checks" 段落("Promoted to blocking 2026-XX-XX" + 注明 Phase 2 起 required)。
3. `baseline-rev` 从 `v0.5.5` 升到 `v0.5.6`(在 v0.5.6 release tag 切出后),并在 `exclude` 列表里增删:`shannon-desktop` 已被 exclude(本地路径,不影响 semver);`shannon-api-protocol` 继续 exclude 直到基线赶上。
4. **先 dry-run**:`cargo install cargo-semver-checks --locked && cargo semver-checks --baseline-rev v0.5.5` 跑一次,记录当前公开 surface;v0.5.6 改动后跑 diff;v0.6.0 改动前必须先确认 v0.5.5→v0.5.6 的 diff 干净(没有意外的 break)。

### 2.4 用户迁移路径

桌面用户(从 0.5.x 升到 0.6.x):

- **没有破坏性**:`providers.json` 仍存在,`list_providers` 仍能读出来。引擎运行时只看 `providers.toml`。
- **新增 UX 提示**(0.5.6 起):"This desktop session now writes to `~/.shannon/providers.toml` only — the legacy `providers.json` cache is read-only." 一次性 toast。
- **移除 `providers.json`**(0.6.0):删除前先 0.5.6 跑一个 release 的"read-only 阶段",让所有桌面实例升上来;0.6.0 release notes 写明"`~/.shannon/desktop/providers.json` is no longer consulted. If you have hand-edits, migrate them to `providers.toml` via the new `shannon providers import <path>` CLI command"(若提供;否则 `mv` 即可)。

CLI 用户:

- **没有破坏性**:`providers add` / `providers remove` 签名不变,只是底层多绕一圈 `ProviderConfigService`(行为字节级一致,`connected_slugs()` 与 `active_target` 的输出在 T1-T9 测试已固定)。

REPL 用户:

- **没有破坏性**:`/connect` / `/disconnect` / `/model --save` / `/model --tier --save` 全部签名不变;底层 Service 调用,但写入形状已由 `provider_config_service` 的 T1-T9 测试 + `to_provider_profile_maps_known_kind_to_engine_enum` 等测试锁定。

---

## 3. 行为对齐验收清单(30–60 条)

> 全部以 P2-2 S1 子任务验收为目标,标 [ ] 表示尚未勾选(实施期间由 S1 子代理标记)。

### 3.1 写入路径单一性(Phase 2 task 4 + ADR-0008 决策 3 的 Acceptance)

- [ ] 所有写入 `providers.toml` 的代码路径都从 `ProviderConfigService` 进入(`grep -rn "upsert_profile\|set_active\|set_tier\|remove_profile\|save_locked\|\.save()" crates/shannon-ui/ desktop/src/ crates/shannon-cli/src/commands_providers.rs` 应该 0 处非测试命中,除了 Service 实现自身与 Service 的 `from_store` 接收方)。
- [ ] REPL `apply_connect` (`crates/shannon-ui/src/repl/commands/config.rs:601-733`) 改走 `ProviderConfigService::connect(LlmProvider, Some(model), base_url, true)`;不再调 `provider_resolver::build_connect_profile`。
- [ ] REPL `apply_disconnect` 走 `ProviderConfigService::disconnect`;用 `DisconnectOutcome::next_active` 自动 `apply_model_selection`。
- [ ] REPL `/model --save` 走 `ProviderConfigService::set_active`。
- [ ] REPL `/model --tier <fast|standard|pro> <model> --save` 走 `ProviderConfigService::set_tier`。
- [ ] CLI `providers add` 保持走 `ProviderConfigService::upsert`(已合规)。
- [ ] CLI `providers remove` 改走 `ProviderConfigService::disconnect`,在 `next_active` 非空时打印"next active: <slug>"。
- [ ] 桌面 `save_provider` 改走 `ProviderConfigService::upsert`(`from_store(std::mem::take(...))` 模式),复用 `land_profile_in_engine_store` 的 `flock + save_locked` 改写为 `Service::upsert` + 在 Service 内部拿锁(若 Service 不支持,新增 `ProviderConfigService::upsert_locked`)。
- [ ] 桌面 `set_active_provider` 同样走 `ProviderConfigService::upsert`(与 CLI `--set-active` 路径对齐)。
- [ ] 桌面 `delete_provider` 走 `ProviderConfigService::disconnect`;`providers.json` 同步移除该 id。
- [ ] 桌面 `configure('model')` / `configure('provider')` 改走 `ProviderConfigService::set_active`。
- [ ] 桌面 `configure('base_url')` 改走 `ProviderConfigService::upsert`(用 `to_provider_profile` 重建整个 profile)。

### 3.2 providers.toml 写入一致性

- [ ] `shannon providers add` 后,`shannon list-providers` 输出包含新 provider(已测)。
- [ ] 桌面 `save_provider` 后,`shannon list-providers` 输出包含该 provider,且 `id` 与桌面一致(包括 `glm` / `kimi` 这类自定义 id 不被 engine 折叠成 `openai`)。
- [ ] 桌面 `set_active_provider` 后,`shannon list-providers` 的 `active_target.provider_id` 等于该 provider id。
- [ ] 桌面 `configure('model')` 后,`shannon list-providers` 的 `active_target.model_id` 等于新值。
- [ ] CLI `providers remove <id>` 后,桌面 `list_providers` 不再包含该 id(且 Welcome 页不再列出)。
- [ ] CLI `providers add --kind openai-compatible --base-url X` 后,桌面 Settings → Models 列表显示该 connection,且 `base_url == X`(从 store 投影回 `ProviderConnection`)。

### 3.3 并发 / 锁

- [ ] CLI 与桌面同时修改同一 provider:CLI `providers remove anthropic` + 桌面 `save_provider` 同 id 并发,后写者覆盖前写者,**不出现合并冲突警告**(flock 序列化,见 `flock_serializes_concurrent_rmw_in_two_threads` 测试)。
- [ ] 两个 CLI 进程同时 `providers add`:后写者覆盖前写者的"add",但已存在的 profile(不是当前 add 的那个)不丢失(因为 `flock` 让 load-mutate-save 整体序列化)。
- [ ] 桌面 `save_provider` 与桌面 `set_active_provider` 同 session 并发:不出现 `state.provider_store` 死锁(已在 Service 内部 `Mutex<ProviderConfigStore>` 保护,但桌面目前是 `state.provider_store.lock().await` 后 `save()` 二次拿 `flock`——需要确保 Service 化后**只拿一次锁**)。

### 3.4 active_target 与 active_provider_id 一致性

- [ ] 桌面 `set_active_provider` 后,`providers.toml.active_target.provider_id == providers.json.active_provider_id == <id>`(双向一致)。
- [ ] 桌面 `save_provider`(新增,非激活)后,`providers.toml.active_target.provider_id` 由 `upsert_profile` 钉到新 id(预期),`providers.json.active_provider_id` **不**被修改(因为不是 set_active_provider)。即两份文件允许在"非激活新增"场景下短暂不一致;`list_providers` 读 `providers.toml`,UI 显示以 `providers.toml` 为准。
- [ ] CLI `providers add --set-active=false`(目前 no-op,见 §1.3)若未来改为走 `make_active=false`,`providers.toml.active_target.provider_id` 保持前值,`providers.json.active_provider_id` 不变。

### 3.5 Credential 路径

- [ ] `shannon credentials store anthropic sk-...` 后,桌面 `get_config` 不返回明文 key(`get_config` 早已只读 `desktop_config`,key 不在那里;`mask_providers` 也覆盖 `providers.json`)。
- [ ] 桌面 `save_provider` 输入 `api_key: "sk-..."` 后,`~/.shannon/credentials/<id>.json` 存在且含 key;`providers.json` 不含 plaintext(由 `apply_provider_update` 测试 `apply_provider_update_never_touches_plaintext_key_field` 保护)。
- [ ] Welcome 探测到 `ANTHROPIC_API_KEY` 环境变量后,后续 `shannon` CLI 启动能正常完成请求(走 `provider_resolver::resolve_credential()` env 分支)。

### 3.6 边界

- [ ] `providers add` 已存在的 id 走 upsert(不报错),`active_target` 钉到该 id。
- [ ] `providers remove` 不存在的 id 返回 exit 1 + "provider not found"(CLI)。
- [ ] 桌面 `delete_provider` 不存在的 id 返回 "provider not found: <id>" 错误(由 `remove_provider` 测试 `remove_provider_errors_on_unknown_id` 保护)。
- [ ] 桌面 `save_provider` `extra_headers` / `default_max_tokens` / `tiers` 的 `None` vs `Some(...)` 语义区分正确(由 `apply_provider_update_leaves_v2_profile_fields_untouched_when_none` 等测试保护)。
- [ ] `/connect` 无 model 参数时,`providers.toml.active_target.model_id` 为 provider 的 catalog 默认模型(由 `connect_anthropic_uses_store_credential_and_catalog_default_model` 测试保护)。
- [ ] Ollama(无 key)的 `connect`:profile 仍是 `CredentialRef::Store { service: "ollama" }`,base_url 为 `http://localhost:11434`(由 `connect_uses_provider_default_base_url_for_ollama` 测试保护)。
- [ ] 桌面 `validate_base_url` 拒绝非 http(s) scheme、嵌入凭证、空 host、解析失败——已在 `validate_base_url_*` 测试覆盖。

### 3.7 错误恢复

- [ ] `providers.toml` 被手工破坏为非法 TOML,`shannon` 启动仍能完成请求(由 `load_returns_none_and_logs_on_corrupt_file` 测试保护,回退到 synthesis)。
- [ ] `providers.toml.lock` 缺失,首次 `save()` 不 panic(由 `load_or_default_does_not_panic_when_lockfile_missing` 测试保护)。
- [ ] 两个进程的 flock 死锁:在 Linux 上 `flock(LOCK_EX)` 是阻塞语义——若用户用 `kill -9` 杀掉持有 flock 的进程,锁随 fd 释放;若用普通 kill,锁也释放(flock 是 fd-bound)。但 Windows 上行为不同——若用户报"卡死",提示重启。**已在 `save_locked_does_not_deadlock_inside_outer_lock` + 文档注释覆盖**。

---

## 4. 风险与未决

### 4.1 残留差异(必须列出)

| 差异 | 大小 | 说明 |
|---|---|---|
| 桌面写入绕过 Service | 🟡 中 | 见 §1.2 表格,**这是 Phase 2 S1 的核心工作量**(P2-2-A / B / D) |
| `providers.json` 写穿缓存 | 🟡 中 | ADR-0005 Phase 7 决议保留,直到 0.6.0 |
| `ProviderConnection.api_key` 字段保留 | 🟢 低 | `#[serde(default, skip_serializing)]`,无功能影响 |
| `--set-active` CLI flag 是 no-op | 🟢 低 | 文档化,不影响功能 |
| REPL `/connect` 完成后,desktop 端 `state.client_config` 不会自动重建 | 🟢 低 | 桌面需要重启 / 调用 `rebuild_client_config_from_store`;CLI / REPL 是同一进程,自然 OK |

### 4.2 不可消除的差异(脚本化 vs GUI 约束)

| 差异 | 原因 | 接受方式 |
|---|---|---|
| 桌面必须做 `validate_base_url`(更严格,拒绝非 http(s)) | GUI 用户可能粘贴错误,CLI 用户需要 shell-friendly | 桌面独立做,engine 在 `resolve_provider` 也有 scheme 检查(防御深度) |
| 桌面必须做 key 探测提示(`InvalidKey` / `RateLimited` / `NetworkUnreachable`) | GUI 需要分类的 toast | CLI 不需要,只用 exit code |
| 桌面 Welcome 必须探测 env vars 跳过 key 输入 | UX 简化 | 由 `detect_provider_from_env` 单一函数负责,不影响写入路径 |
| CLI `providers add` 接受 `--tier` 把当前 `--model` 钉到对应 slot;桌面无对应 `--tier` | 桌面 Add Provider modal 已直接填 `tiers` 字典 | 二者等价(都写到 `providers[tiers][<name>] = <model>`) |

### 4.3 STABILITY deprecation 周期的最短长度

**STABILITY.md §"Deprecation cycle" 写明"Wait one minor cycle"**。也就是至少 1 个 minor 版本(0.5.x → 0.6.x)。**结论**:v0.5.6 落地,0.6.0 才能移除 `providers.json`。这是不可压缩的。

### 4.4 是否需要新增 E2E 测试?

**需要**。当前测试是 in-process 单元测试(`ProviderConfigService` 6 个测试 + `ProviderConfigStore` 25+ 测试 + `commands_config.rs` 30+ 测试),但**没有跨 CLI / Desktop 进程的真 E2E**。

**建议**:在 `tests/e2e/cli_desktop_provider_consistency.rs`(新文件)加:

- 用 `tempfile::TempDir` + `XDG_CONFIG_HOME` 隔离 home,起 CLI 进程,跑 `shannon providers add anthropic --model claude-sonnet-4 --set-active`,断言文件存在 + 字段;再起一个**模拟 desktop 进程**(调用 `land_profile_in_engine_store` 等价路径的测试 double),断言读到同样内容;反之亦然。
- **或者**(更现实):在 `crates/shannon-core/tests/provider_cross_process_consistency.rs` 加一个 `tokio::process::Command` 测试,真的 fork `shannon providers add` 与一个 mock 桌面进程,断言 flock 序列化下的读写一致性。

估时:1 天写测试 + 跑通 CI。

### 4.5 其它未决

- **Welcome 流**:Phase 2 ADR-0005 §"Deferred" 提到 "Welcome wizard still uses the legacy singular `configure` flow"——本 spike 未深入覆盖,因为 Welcome 是 React 端(`desktop/web-src/...`),不在 Rust 写入路径上。建议另起一个独立 spike(`P3-X welcome-unification`)。
- **`build_client_config` 重构**:ADR-0005 Phase 7 §"Future" 提到 `AppState::build_client_config` 仍读 `DesktopConfig` singular 字段而非 `build_client_from_resolved`——这是 desktop 内部的字段删除未尽事宜,与 ProviderConfigService 收敛正交,放到 0.6.0 同 release 一起做即可。
- **`/profile` 命名冲突**:ADR-0008 Open Q4。属于 UX/措辞任务,不在 Phase 2 范围。
- **OAuth + models.dev dynamic refresh 桌面 UI**:ADR-0005 §"Open decisions 6" 推迟;不影响 Phase 2。

---

## 5. 下一步动作(S1 子任务拆解)

> 总合并估时 **~6 人天**(对应 1.5w 节奏,有大量缓冲)。

### 5.1 S1 子任务表

| ID | 标题 | 估时 | 依赖 | 风险 | 关键证据点 |
|---|---|---|---|---|---|
| **S1-1** | 把桌面写入路径迁到 `ProviderConfigService` | 2 d | 无(独立) | 🟡 中——Service 缺 `flock + save_locked` 等价物,可能需新增 `upsert_locked` API | `land_profile_in_engine_store` 的 86 行 → 缩减到 ~30 行;`from_store` 模式 |
| **S1-2** | 把 CLI `providers remove` 改走 `ProviderConfigService::disconnect`,打印 `next_active` 提示 | 0.3 d | 无 | 🟢 低 | `commands_providers.rs:590` `run_providers_remove` 改;`DisconnectOutcome::next_active` 已在 Service 中 |
| **S1-3** | 把 REPL `apply_connect` / `apply_disconnect` / `handle_model --save` / `handle_model_tier --save` 迁到 Service | 1 d | 无 | � 中——REPL 代码路径要确认 4 处 `set_active` / `set_tier` / `save` 调用都换 | `crates/shannon-ui/src/repl/commands/config.rs:601-733` + `handle_model_tier` 等 |
| **S1-4** | 新增 E2E 测试 `crates/shannon-core/tests/provider_cross_process_consistency.rs`(CLI ↔ desktop 真 fork + flock 序列化) | 1 d | S1-1/2/3 完成 | 🟢 低(测试代码) | 复用 `flock_serializes_concurrent_rmw_in_two_threads` 的双线程模式,扩展到跨进程 |
| **S1-5** | 在 `provider_config_service.rs` 模块 doc 上加"desktop callers via `from_store + upsert`"段落,移除 `land_profile_in_engine_store` 的过时 doc;同步 `commands_config.rs` 的过时注释 | 0.3 d | S1-1 完成 | 🟢 低 | `commands_config.rs:42-86` 注释;`provider_config_service.rs:1-28` |
| **S1-6** | 升级 `cargo-semver-checks` 为 required check:移除 `continue-on-error: true`、更新 `STABILITY.md`、设 `baseline-rev: v0.5.6`、dry-run 一次确认 0.5.6 没有意外 break | 0.5 d | S1-1/2/3/5 完成;v0.5.6 tag 切出后 | 🟡 中——CI 配置改动需要单独评审 | `.github/workflows/ci.yml:269-302` + `docs/STABILITY.md:61-77` |

**合计 5.1 人天**(按 1 人 = 1 d)。Sprint 节奏建议:**Sprint 1 (W1) 完成 S1-1 / S1-2 / S1-3 / S1-5**(共 3.6 d),**Sprint 2 (W2) 完成 S1-4 + S1-6 + 合并评审 + tag v0.5.6**(共 1.5 d + buffer)。**总 ≤ 1.5w**,符合任务书"≤ 2-3w"。

### 5.2 不在 Phase 2 范围(后续 ADR / spike)

- **0.6.0 收尾**:`providers.json` 移除、`ProviderConnection.api_key` 字段移除、`migrate_providers_*` 转为 no-op。估时 1 w(独立 ADR 触发)。
- **`ProviderStoreReadFacade` ADR**:把 `ProviderConfigStore` 的读访问从 `Mutex<...>` 解耦——`list_providers` 不必 lock。估时 1 w(独立 ADR)。
- **Welcome 流 React 重写**:Welcome modal 与 ProviderConfigService 的桥接。估时 1 w。
- **`build_client_config` 重构**:从 `DesktopConfig` 字段切到 `build_client_from_resolved`。估时 0.5 w(mechanical)。

### 5.3 依赖 / 风险登记

- **依赖 ADR-0008 Acceptance 的 Phase 1 验证**(ADR-0008 §"Acceptance" 的 7 条 check)。若 Phase 1 未签收,Phase 2 应顺延——这是 Wave 6 QA 流程的依赖。
- **风险**:桌面 `save_provider` 改成 Service 化后,如果发现 `Service::upsert` 内部 `save()` 拿不到 `state.provider_store` 已持有的锁,会触发 deadlock——必须**先用 `Mutex<ProviderConfigStore>` 包裹 `Service`**(即 `state.provider_service: Mutex<ProviderConfigService>`,在 `AppState` 初始化时把 `ProviderConfigStore::load_or_default()` 直接喂给 `from_store`),或在 Service 上新增 `upsert_locked` / `disconnect_locked` API。**S1-1 必须先决定这条路,不能写一半换**。

---

## 附录 A:证据锚点(代码引用)

| 事实 | 引用 |
|---|---|
| ADR-0005 Phase 2 已完成 | `docs/adr/0005-unified-provider-model-credential-management.md:8` "Phase 2 ✅ done (task 4 commits)" |
| `ProviderConfigService` 是单一语义写入路径 | `crates/shannon-core/src/provider_config_service.rs:68-78` 文档注释 |
| CLI `providers add` 走 Service | `crates/shannon-cli/src/commands_providers.rs:522-552` `run_providers_add` |
| CLI `providers remove` 不走 Service | `crates/shannon-cli/src/commands_providers.rs:590+` |
| 桌面 `land_profile_in_engine_store` 直接调 `upsert_profile` | `desktop/src/commands_config.rs:66-86` |
| 桌面写 `providers.json` | `desktop/src/commands_config.rs:1399, 1435, 1500` 三处 `config::save_providers(&file)` |
| `ProviderConnection.api_key` skip_serializing | `desktop/src/config.rs:201` |
| `providers.json` 路径 | `desktop/src/config.rs:623-626` `providers_path()` |
| `migrate_providers_to_credentials` / `migrate_providers_to_toml` | `desktop/src/config.rs:430, 670` |
| STABILITY deprecation cycle | `docs/STABILITY.md:86-97` |
| `cargo-semver-checks` 当前 advisory | `.github/workflows/ci.yml:269-282` |
| ADR-0008 决策 3:Service 是 CLI/REPL 共用 contract | `docs/adr/0008-provider-model-command-architecture-remediation.md:156-179` |
| ADR-0008 Acceptance 7 项 | `docs/adr/0008-provider-model-command-architecture-remediation.md:328-347` |
| `apply_model_selection` 单写入路径(决策 2) | `crates/shannon-ui/src/repl/commands/config.rs:118` |
| `connected_provider_slugs` 已合并(P2-2) | `docs/plans/provider-model-command-remediation.md:281-291` |
| `/provider health` 命令(Phase 6) | `docs/adr/0005-...md:415-428` |
| Phase 7 `ProviderConnection` 边界决策 | `docs/adr/0005-...md:430-536` |
| desktop Welcome 探测 env vars | `desktop/src/commands_config.rs:736-760` `detect_provider_from_env` |
| desktop 桌面 keyring 仅 gateway social | `docs/adr/0005-...md:56` |
| 当前无 `#[deprecated]` 在 `shannon-core/src/`(除 testing) | `grep -rn "#\[deprecated" crates/shannon-core/src/` 仅命中 `crates/shannon-core/src/testing/mod.rs:15` |

## 附录 B:术语表

- **SSOT** = single source of truth。本 spike 指 `~/.shannon/providers.toml`。
- **A1** = ADR-0005 §"Decision A1":config 文件只携带 `CredentialRef` 引用,绝不存明文 key。
- **flock** = `flock(LOCK_EX)` on `providers.toml.lock` sidecar file;跨进程互斥。
- **make_active** = `ProviderConfigStore::upsert_profile` 总是把 `active_target` 钉到新 id;Service 的 `connect`/`upsert` 加 `make_active: bool` 参数,`false` 时还原旧 `active_target`。
- **ProviderConnection ↔ ProviderProfile** = 桌面侧 UI 类型 ↔ 引擎侧 schema 类型;翻译函数在 `desktop/src/config.rs:272` (`to_provider_profile`) 与 `:347` (`from_provider_profile`)。
- **P1.2-A** / **P1.2-B** = ADR-0005 §"Phase 2 / Deferred" 的子编号;P1.2-A 是"写路径走 engine store",P1.2-B 是"读路径(client_config)走 engine store"。

---

**Spike 完成**。等待 Wave 6 评审委员会确认 S1 子任务拆分与 §2.3 的 semver 门禁升级路径。
