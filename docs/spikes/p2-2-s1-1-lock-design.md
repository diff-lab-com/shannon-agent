# P2-2 S1-1 锁定策略设计 spike(2d 目标)

> **目标**:决定 `ProviderConfigService` 接入桌面时的**双重锁死锁**避免方案。
> **范围**:只设计、不动代码。
> **下一步**:基于此设计出 `feat/p2-2-s1-1-desktop-service` 分支的实施 PR。

---

## 0. 现状(已从代码确认)

### 0.1 锁的种类

| 锁 | 类型 | 提供方 | 位置 |
|---|---|---|---|
| `Arc<tokio::sync::Mutex<ProviderConfigStore>>` | 进程内 async mutex | desktop AppState | `desktop/src/commands.rs:81` |
| `flock(LOCK_EX)` on `<providers.toml>.lock` | 跨进程文件锁 | `fs2::FileExt` | `crates/shannon-core/src/provider_config_store.rs:86`(`acquire_exclusive_lock`) |

### 0.2 桌面当前的"双层锁 + 锁感知 helper"模式

`desktop/src/commands_config.rs:66-86`(`land_profile_in_engine_store`):

```rust
async fn land_profile_in_engine_store(state, conn, model_id) -> Result<(), String> {
    let mut store = state.provider_store.lock().await;     // ① 进程内 mutex
    let path = store.last_path().ok_or_else(...)?;
    let _flock = acquire_exclusive_lock(path)              // ② 跨进程 flock
        .map_err(|e| format!("could not lock providers.toml: {e}"))?;
    store.upsert_profile(profile, model_id);               // ③ mutate(不持锁)
    store.save_locked()                                    // ④ 写入(已持 flock,不再 acquire)
        .map_err(|e| format!("could not persist to providers.toml: {e}"))?;
    Ok(())
}
```

`store.save_locked()` 内部**不**调 `acquire_exclusive_lock`,只 `atomic_write_secure` + `release_exclusive_lock` 由 `Drop` 触发。

### 0.3 Service 当前的"内部 acquire flock"模式

`crates/shannon-core/src/provider_config_service.rs:129-156`(`connect`):

```rust
pub fn connect(&mut self, ...) -> io::Result<ConnectedProvider> {
    // ...mutate via self.upsert_profile_with_active(...)
    let saved_path = self.store.save()?;   // 内部 acquire + release flock
    Ok(...)
}
```

`self.store.save()` → `pub fn save(cfg, path)` → `acquire_exclusive_lock(&path)?` + `save_locked` + `release_exclusive_lock`。

**所有 mutating 方法**(`connect` / `upsert` / `disconnect` / `set_active` / `set_tier` / `set_max_tokens`)都走这个模式。

### 0.4 调用方盘点

| 调用方 | 路径 | 是否持 flock |
|---|---|---|
| REPL `/connect` | `crates/shannon-commands/src/builtin/connect.rs` | ❌(调 service,service 内 acquire) |
| REPL `/disconnect` | `crates/shannon-commands/src/builtin/disconnect.rs` | ❌(同) |
| REPL `/model --save` | `crates/shannon-commands/src/builtin/model.rs` | ❌(同) |
| REPL `/tier --save` | `crates/shannon-commands/src/builtin/tier.rs` | ❌(同) |
| CLI `providers add` | `crates/shannon-commands/src/builtin/providers.rs:536` | ❌(同,唯一走 `from_store` 的合规路径) |
| CLI `providers remove` | `crates/shannon-commands/src/builtin/providers.rs` | ❌(绕 service,直调 store) |
| Desktop `land_profile_in_engine_store` | `desktop/src/commands_config.rs:66` | ✅(`acquire_exclusive_lock`) |
| Desktop `remove_profile_from_engine_store` | `desktop/src/commands_config.rs:188` | ✅(同) |

**只有桌面 2 个路径**当前在外部 flock + 用 `save_locked`。其它调用方都让 service 内部处理 flock。

---

## 1. 死锁场景分析

### 1.1 如果把 service 直接给桌面用会发生什么?

把 `land_profile_in_engine_store` 改成:

```rust
// ❌ 错误:死锁
let mut svc = ProviderConfigService::from_store(store);
let _ = svc.connect(provider, None, None, true)?;  // 内部 self.store.save() → acquire_exclusive_lock
```

**Linux 行为**:`flock(2)` 不是 reentrant(同进程同 lockfile 不同 fd 会自死锁)。**永远 hang**。
**macOS 行为**:`flock(2)` 在同进程会返回 `EDEADLK`。
**Windows 行为**:`LockFileEx` 不死锁但永远等待(行为类似 Linux)。

### 1.2 即使不持 Mutex,只让 service 工作流

把 `land_profile_in_engine_store` 简化为:

```rust
// ❌ 同样错误
let mut svc = ProviderConfigService::from_store(state.provider_store.lock().await.clone());
let _ = svc.upsert(profile, model_id, true)?;  // 内部 flock,与别的 CLI 进程互斥
```

**这次死锁不会发生**(因为 service 是从头到尾自己拿锁,桌面路径里没预先拿 flock)。但:
- 桌面**失去**"在 AppState mutex 锁里做事"的能力(锁粒度变粗)
- 跨进程 race:如果在 service 拿到 flock 之前,另一线程的 service 也在跑,会出现 `state.client_config` 读旧值

### 1.3 正确路径必须满足

1. **桌面 2 个路径保持现有的"双重锁"语义**:进程内 mutex + 跨进程 flock
2. **service 单一写路径**(ADR-0008 决议)不被破坏
3. **CLI / REPL 路径不变**(它们没外部 flock,让 service 自己管就行)
4. **不能引入双重 flock**

---

## 2. 三种候选方案(再评估)

### 2.1 方案 A · 全拆 flock(service 永远不 acquire)

```rust
// 改:ProviderConfigStore::save() 内部不再 acquire
// 改:服务所有 mutating 方法改成 mutate-only,save 留给 caller
```

**改动面**:
- `ProviderConfigStore::save()` 拆成 `save_locked()`(已存在)+ 删 `save()` 的 flock acquire
- `ProviderConfigService` 6 个 mutating 方法全部改成"返回 `Result<MutationPlan, _>`,不 save"
- CLI / REPL 全部调用方都要自己 acquire+save
- 桌面 2 个路径(land/remove)用 service mutate + 自己 `save_locked()`

**问题**:
- 改 6 个 service 方法 + ~6 个 CLI/REPL 调用方,**改动面太大,风险高**
- 失去"load → mutate → save 一气呵成"的 service 抽象(当初就是为了这个设的)
- 不推荐

### 2.2 方案 B · service 加 `_locked` API

```rust
// 新增
impl ProviderConfigService {
    pub fn upsert_locked(&mut self, profile, model_id, make_active) -> io::Result<()> {
        self.upsert_profile_with_active(profile, model_id, make_active);
        // 不 save,caller 持 flock 后自己调 store.save_locked()
        Ok(())
    }

    pub fn disconnect_locked(&mut self, provider) -> io::Result<DisconnectOutcome> {
        // 不 save
    }
    // ... 6 个方法 × 2 = 12 个 API
}
```

**改动面**:
- service 加 6 个 `_locked` 方法(机械式)
- 桌面 2 个路径替换为 service.upsert_locked() + 自己 store.save_locked()
- CLI / REPL **不变**(用现有 save() 路径)

**问题**:
- API 表面 2 倍膨胀(非 add,纯复制)
- `_locked` 命名暗示"caller 必须持锁",文档负担大
- 候选

### 2.3 方案 C · service 接管整段锁(★ 推荐)

```rust
// 新增
impl ProviderConfigService {
    /// Run `f` with an exclusive flock held on the underlying providers.toml.
    /// The service mutates the in-memory config; the closure does the
    /// `save_locked` write to commit. `f` is called with the service already
    /// mutating — it can do the actual save itself OR return a MutationPlan
    /// for the service to commit.
    pub fn with_exclusive_lock<R>(
        &mut self,
        f: impl FnOnce(&mut Self) -> io::Result<R>,
    ) -> io::Result<R> {
        let path = self.store.last_path().ok_or_else(...)?;
        let _flock = acquire_exclusive_lock(&path)?;
        f(self)
    }
}
```

桌面调用方:

```rust
async fn land_profile_via_service(state, conn, model_id) -> Result<(), String> {
    let mut svc = ProviderConfigService::from_store(state.provider_store.lock().await.clone());
    let profile = connection_to_profile(conn);
    svc.with_exclusive_lock(|svc| {
        svc.upsert_profile_with_active(profile.clone(), model_id, true);
        svc.into_inner_save_locked()
    }).map_err(|e| format!("...: {e}"))?;
    Ok(())
}
```

**改动面**:
- service 加 1 个新方法 `with_exclusive_lock`
- 桌面 2 个路径替换为 service 模式
- 桌面 AppState 的 `provider_store: Arc<Mutex<...>>` 暂时保留(读路径仍直接用),但写路径走 service
- CLI / REPL **不变**(用现有 save() 路径)

**优点**:
- API 表面增加最小(只 1 个新方法)
- "持锁"语义由 service 封装,桌面调用方不需理解 flock 细节
- 与现有注释(`land_profile_in_engine_store` "MUST use the no-lock variants inside the critical section")在心智上一致
- 死锁不可能:只有一个 `with_exclusive_lock` 入口

**缺点**:
- 桌面现有的 `Mutex<ProviderConfigStore>` 与 service 的 `with_exclusive_lock` 仍是两层锁,需要确认锁顺序(Mutex 先于 flock)
- service 的 `with_exclusive_lock` 仍要求 caller 自己 mutate(因为 service 不能在 closure 内安全地调 `&mut self` mutating 方法同时 borrow `&mut self.store`)

### 2.4 ★ 推荐方案 C

理由:
- 改动面最小
- 死锁路径单一,易于审计
- 不破坏 ADR-0008 的"service 单一写路径"决议
- CLI / REPL 不动
- 与现有 helper 文档(`land_profile_in_engine_store` 注释)语义一致

---

## 3. 锁顺序契约(所有方案必须遵守)

桌面调用方必须**先**持 `Mutex<ProviderConfigStore>`,**再**让 service 持 flock:

```rust
let mut store = state.provider_store.lock().await;  // ①
let mut svc = ProviderConfigService::from_store(/* moved out of store */);
// service.with_exclusive_lock 内部 acquire flock ②
// drop svc,再 drop store
```

**禁止**反向(flock 先,Mutex 后):那会让别的 desktop 命令 hang 在 `lock().await`。

---

## 4. 详细设计(方案 C 实施细节)

### 4.1 新增 service API

```rust
impl ProviderConfigService {
    /// Acquire an exclusive `flock` on the underlying `providers.toml`
    /// and run `f` while holding it. `f` receives `&mut self` so it can
    /// drive the service's in-memory mutators (e.g.
    /// `upsert_profile_with_active`); the closure is expected to commit
    /// the changes via `Self::save_locked` (a new method, see below) or
    /// by calling a mutator that persists through the held lock.
    ///
    /// This is the **only** public entry point that exposes the
    /// cross-process flock — the bare `connect` / `upsert` / `disconnect`
    /// methods all acquire the flock internally and panic / deadlock if
    /// called while another flock is held on the same path.
    pub fn with_exclusive_lock<R>(
        &mut self,
        f: impl FnOnce(&mut Self) -> io::Result<R>,
    ) -> io::Result<R> {
        let path = self.store.last_path().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no providers.toml path")
        })?;
        let _flock = acquire_exclusive_lock(&path)?;
        f(self)
    }

    /// Persist the in-memory config to `last_path` **without** acquiring
    /// the cross-process flock. Caller MUST hold the flock (typically
    /// via `with_exclusive_lock`). Panics if `last_path` is `None`.
    pub fn save_locked(&mut self) -> io::Result<PathBuf> {
        self.store.save_locked()
    }
}
```

### 4.2 桌面调用方迁移

`desktop/src/commands_config.rs`:

```rust
async fn land_profile_via_service(
    state: &tauri::State<'_, AppState>,
    conn: &ProviderConnection,
    model_id: &str,
) -> Result<(), String> {
    let store = state.provider_store.lock().await;
    let mut svc = ProviderConfigService::from_store(store);
    let profile = connection_to_profile(conn);
    svc.with_exclusive_lock(|svc| {
        svc.upsert_profile_with_active(profile, model_id, true);
        svc.save_locked()
    })
    .map_err(|e| format!("could not persist to providers.toml: {e}"))?;
    Ok(())
}
```

### 4.3 测试矩阵(必须通过)

| 测试 | 验证 |
|---|---|
| 现有 service 测试 6 个 + 桌面测试 ~40 个 | 不破坏 |
| 新增:`with_exclusive_lock_runs_closure` | 基础 happy path |
| 新增:`nested_with_exclusive_lock_deadlocks_or_errors` | 故意在 closure 里再调 service.connect,确认死锁不会被引入(走 service.connect 仍然会试图 acquire flock,但与 with_exclusive_lock 的 _flock 同 fd,会立刻 EDEADLK / hang)— 这个测试要 `#[ignore]` 默认不跑,只手动跑 |
| 新增:跨进程 E2E(CLI providers add vs desktop 同时操作) | flock 互斥生效 |
| 新增:多线程桌面 R-M-W 并发(`spawn` 10 个 land_profile) | Mutex 互斥生效 |

### 4.4 S1-1 估时(2d → 实际切分)

| 时间 | 任务 |
|---|---|
| 0.5d | 实施 `with_exclusive_lock` + `save_locked` API + 2 个单测 |
| 0.5d | 迁移 `land_profile_in_engine_store` → `land_profile_via_service` + 验证原行为 |
| 0.5d | 迁移 `remove_profile_from_engine_store` + 验证 |
| 0.5d | 多线程并发测试 + 跨进程 E2E(spawn `shannon providers add` 同时点桌面按钮) + clippy / fmt / nextest |

---

## 5. 风险与未决

| 风险 | 概率 | 缓解 |
|---|---|---|
| `Mutex<ProviderConfigStore>` 持锁 + `with_exclusive_lock` 内 `acquire_exclusive_lock` → 反向锁顺序 | 低 | S1-1 PR review 显式检查锁顺序;在 `with_exclusive_lock` doc 写明 |
| 别的 desktop 路径偷偷持 `provider_store` 同时进 service 写路径 | 中 | S1-2 一次性 audit `commands_config.rs` 所有路径 |
| 现有 REPL `/connect` 在 Tauri 桌面被调用时(可能没有),会与桌面 service 路径冲突 | 低 | S1-1 后跑 E2E 验证 |
| `with_exclusive_lock` 的 closure 语义 + 借 `&mut self` 与 `&self.store` 同时 → borrow checker 报错 | 中 | 实施时用 `&mut ProviderConfigService` 透传,store 是字段不需要显式重借 |

---

## 6. 下一步(评审后)

1. **S1-1.1** — 实施 `with_exclusive_lock` + `save_locked` + 2 个单测(0.5d)
2. **S1-1.2** — 迁移桌面 2 个写路径(0.5d)
3. **S1-1.3** — 并发 + 跨进程测试(0.5d)
4. **S1-1.4** — clippy / fmt / nextest 全绿 + commit(0.5d)

**预计 2d 收口**,与 spike 报告 S1-1 估时一致。

---

## 7. 实施结果与验收(S1-1..S1-4 收口)

> 分支 `feat/p2-2-s1-1-desktop-service`。本节记录相对 §4 方案 C 的偏离与
> 验收清单的实际落点。

### 7.1 设计偏离:closure → RAII `LockedService` + 集中 lock-then-reload

方案 C 设想的 `with_exclusive_lock(|svc| { ... })` 闭包在实施时遇到 §5 预言
的 borrow-checker 摩擦(闭包内同时 `&mut self` 与读 `&self.store`)。改为
更符合 Rust 习惯的 **RAII guard**:

- `ProviderConfigService::lock() -> io::Result<LockedService<'_>>` — 唯一
  暴露跨进程 flock 的入口,`LockedService::drop` 释放锁(`File::close` 触发
  OS 级 `flock` 释放,panic-safe)。
- `LockedService` 镜像 `MutexGuard` 形态,持锁期间用 `upsert` /
  `disconnect_by_slug` / `set_active` 等"已持锁"变体 + `save_locked` 提交。

满足方案 C 的全部目标(单一锁入口、无双 flock、死锁路径单一、CLI/REPL 不
动),且规避了闭包的 borrow 问题。

### 7.2 S1-4 发现的 stale-read 窗口与 `reload_locked` 修复

实施跨进程测试时发现:`ProviderConfigService` 在构造时读快照,但 flock 在
之后才 acquire — 两个写者竞态时会丢失更新(`load → lock` 之间的 stale
read,经典 R-M-W lost update)。`save()` 的原子 rename 只防 torn write,不
防 lost update。

**修复**:把 `lock → reload → mutate → save_locked` 烘进 7 个 bare 方法
(`connect` / `upsert` / `disconnect` / `disconnect_by_slug` / `set_active`
/ `set_tier` / `set_max_tokens`),使 6 个 bare-Service 调用方(CLI + REPL)
零改动自动获益;`LockedService::reload_locked()` 供桌面 5 个 `configure()`
写路径在显式持锁后、mutate 前调用。

### 7.3 验收清单映射(§4.3 测试矩阵)

| §4.3 计划测试 | 实际落点 | 状态 |
|---|---|---|
| 现有 service 测试 + 桌面测试不破坏 | `provider_config` 模块 47 测 + desktop 既有测全绿 | ✅ |
| `with_exclusive_lock_runs_closure`(happy path) | `locked_service_path_composes_on_fresh_state` + `locked_service_drop_releases_the_flock`(`tests/provider_cross_process_consistency.rs`) | ✅ |
| `nested_with_exclusive_lock_deadlocks_or_errors` | 未单列 `#[ignore]` 测试;改为在 `lock()` doc 写明禁用项,且 bare 方法自身经 `lock` 路由 → 持锁内调 bare 方法会自死锁,故 `LockedService` 提供等价方法 | ✅(文档 + 设计规避) |
| 跨进程 E2E(CLI vs desktop)flock 互斥 | `concurrent_writers_do_not_lose_updates`(8 线程,见下注)+ `bare_upsert_picks_up_another_writers_commit`(确定性回归) | ✅ |
| 多线程桌面 R-M-W 并发,Mutex 互斥 | `concurrent_writers_do_not_lose_updates`;桌面进程内 `Mutex<ProviderConfigStore>` 由 desktop 既有测试覆盖 | ✅ |

> **为什么用线程而非 spawn 真子进程**:`flock(2)` 按 open-file-description
> 序列化(每次 `open()` 独立),与调用方是否同进程无关。scoped-thread 各自
> `load_at` + `lock`,行为与两个 `shannon` 子进程等价,且快、不 flaky。
> 详见 `tests/provider_cross_process_consistency.rs` 模块文档。

### 7.4 锁顺序契约(§3)落实

桌面 5 个写路径(`land_profile_in_engine_store` /
`remove_profile_from_engine_store` / `configure('model')` /
`configure('base_url')` / `configure('provider')`)统一遵循
`provider_store.lock().await → mem::take → from_store → svc.lock() →
reload_locked → mutate → into_inner 回填`,mutex 先于 flock,与 §3 一致。
注释见 `desktop/src/commands_config.rs`。

### 7.5 实施切分(实际)

| 步骤 | 内容 | commit |
|---|---|---|
| S1-1 | `ProviderConfigService::lock` + `LockedService` RAII | `7290836b` |
| S1-2 | CLI `providers remove` 走 service(原绕 store 直调) | `c2542b84` |
| S1-3 | REPL `/model` `/tier` `--save` 走 service | `d685d758` |
| S1-1b | 桌面 5 个写路径迁移到 `LockedService` | `b6dbf652` |
| S1-4 | `reload_locked` 修复 stale-read + 跨进程一致性测试 | `223f589e` |
| (附) | `dead_code` KEEP 标注 + invariant 基线收缩 | `a1a5ea44` |

**验证**:shannon-core 3556/3557 通过(唯一失败 `scheduled_budget::tests::roll_over_resets_spend`
为预存日期腐烂,与本工作无关);`cargo fmt --all --check` + `cargo clippy -p shannon-cli --lib -- -D warnings`
+ `cargo clippy -p shannon-core --lib -- -D warnings` 全绿。
