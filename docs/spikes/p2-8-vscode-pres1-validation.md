# P2-8 VSCode 扩展前置验证 spike(0.5d)

> **Wave 6 / P2-8 / S1 pre-implementation validation**
> 日期:2026-08-04 · 编写人:S1 验证 spike 子代理 · 只读仓库 + 产出本文件
> 验证锚点:HEAD `6b7ecafb`,基于 P2-7 commit `3ed22799`
> 上游 S0 spike:`docs/spikes/p2-8-vscode.md`(本仓库已存在,2026-08-04)

---

## TL;DR

| 项 | 结论 | 一句话 |
|---|---|---|
| **Q1 token** | ⚠️ 半成品 | 协议已落地,签发端由用户在 `shannon serve --auth-token <token>` 显式提供;`SHANNON_SERVE_TOKEN` 环境变量也可。无「启动时自动签发 + 落盘」的现成机制。VSCode 端需要"用户先在 shell 跑一次 `shannon serve`、手动复制 token"才能联通,**UX 不达 MVP 标准**。|
| **Q2 repo map** | ❌ 未暴露 | repo-map 引擎(`crates/shannon-repomap`)是完整 Rust crate,但 P1-4 的 HTTP 接入**未在任何 server crate 中实现**。`shannon-server` 只有 `/v1/sessions`、`/v1/sessions/:id`、`/v1/sessions/:id/messages`、`/openapi.json`;`shannon-core::api_server` 的 `/api/*` 也没有 repo-map 路由。 |
| **Q3 端口** | ⚠️ 隐含冲突 | 默认端口 `33420`,**`shannon serve`(CLI,落 `shannon-server`)与 `shannon-desktop`(内嵌 `shannon-core::api_server`)撞同一个端口**。Desktop 有 `engine_discovery::probe_existing_engine`(Q4-A,`desktop/src/engine_discovery.rs`)做的 250ms HTTP probe,主动让位;CLI 端**没有**对称逻辑,只会 `TcpListener::bind` 后报错。|

**总体建议:** **需补 0.8–1.0 d 修补再开 MVP**,而非 S0 §5 直接进 S1。

理由如下 3 个具体缺口,详情见下文各章节。

---

## Q1. token 签发 — 详细

### 端点列表(实际存在)

`shannon-server`(P2-7 新增,`3ed22799`,CLI `shannon serve` 调用):

| 路由 | 方法 | 文件:行 | 鉴权 |
|---|---|---|---|
| `/v1/sessions` | POST | `crates/shannon-server/src/routes/mod.rs:27-53` | bearer(`auth.rs:28-47`) |
| `/v1/sessions/{id}` | GET | `crates/shannon-server/src/routes/mod.rs:55-72` | bearer |
| `/v1/sessions/{id}/messages` | POST | `crates/shannon-server/src/routes/mod.rs:74-114` | bearer |
| `/openapi.json` | GET | `crates/shannon-server/src/lib.rs:39-41` | bearer |

`shannon-core::api_server`(P0.2,aacdea4 + 802abf7,desktop 与旧 gateway 使用):

| 路由 | 方法 | 文件:行 | 鉴权 |
|---|---|---|---|
| `/api/health` | GET | `crates/shannon-core/src/api_server.rs:190` | **公开**(`auth_middleware` 例外,line 294) |
| `/api/models` | GET | `crates/shannon-core/src/api_server.rs:191` | bearer |
| `/api/query` | POST | `crates/shannon-core/src/api_server.rs:192` | bearer |
| `/api/query/stream` | GET (SSE) | `crates/shannon-core/src/api_server.rs:193` | bearer |
| `/api/tools/list` | POST | `crates/shannon-core/src/api_server.rs:194` | bearer |
| `/api/ws` | GET (WS upgrade) | `crates/shannon-core/src/api_server.rs:195` | bearer |
| `/api/approval/respond` | POST | `crates/shannon-core/src/api_server.rs:196` | bearer |

> ⚠️ **关键事实**:这两套 server 是**两套独立 crate**,`shannon-cli` 用前者,`shannon-desktop` 内嵌后者。`shannon-api-protocol`(`crates/shannon-api-protocol/src/lib.rs`)只共享 wire 类型,不统一路由。VSCode 扩展**必须选其一**,但本 spike 看到的两者在端点完整性、streaming 支持、文档/测试覆盖上差距很大(见下文)。

### token 获取路径(3 步)

1. **用户在 shell 显式提供**
   `crates/shannon-cli/src/main.rs:560-577` —— `shannon serve --auth-token <token> [--host ... --allow-nonloopback]`,默认 port `33420`(`main.rs:564`)、默认 bind loopback、未提供 token 时**完全无 auth**(任意同机进程可调)。
   ```rust
   // main.rs:564-577
   Serve {
       #[arg(short, long, default_value_t = 33420)]
       port: u16,
       #[arg(long)] host: Option<String>,
       #[arg(long)] auth_token: Option<String>,
       #[arg(long)] allow_nonloopback: bool,
   },
   ```

2. **转发到 server crate**
   `main.rs:1931-1978`(run_serve_command)→ `shannon_server::run(host, port, client_config, auth_token)`(`main.rs:1975`)。
   `shannon-server/src/lib.rs:48-57` 最终 `tokio::net::TcpListener::bind((host, port)).await`。`auth_token` 直接被读为 `Option<String>`,无任何派生/编码:
   ```rust
   // lib.rs:42-45
   .layer(middleware::from_fn_with_state(
       auth::AuthConfig::new(token.or_else(|| std::env::var("SHANNON_SERVE_TOKEN").ok())),
       auth::bearer_middleware,
   ))
   ```
   第二个来源是 `crates/shannon-server/src/auth.rs:21-25` 的 `AuthConfig::from_env` / `SHANNON_SERVE_TOKEN` 环境变量。

3. **server 端验签**
   `crates/shannon-server/src/auth.rs:28-47` —— `subtle::ConstantTimeEq` 比对 `Authorization: Bearer <token>`,不匹配返 `StatusCode::UNAUTHORIZED`。
   `shannon-core/src/api_server.rs:289-311` 实现完全一致(`ct_eq` + constant-time),**只比较原始字符串**,不做 token 类型/过期/刷新判断。

### 验证方式(从代码引用)

- 测试覆盖:`shannon-core/src/api_server.rs:1035-1068`(7 个 bearer 测试用例,含 `secret` token 的匹配/不匹配)。
- `shannon-server` 端**没有任何 token 单元测试**(`grep "fn test" crates/shannon-server/src/auth.rs` 返回空)。

### ⚠️ 缺口

| 项 | 现状 | S0 假设 | 真实差距 |
|---|---|---|---|
| **自动签发** | 不存在 | S0 §3.6 期望"扩展存 SecretStorage + 从 `shannon serve` 取 token" | token 是**用户自己敲 `--auth-token`**,服务只验证;**无** start-time 随机签发、落盘、可被 client 拉取的机制。 |
| **token 存哪** | 不存在 | 同上 | 没有任何"已签发 token 列表"端点;扩展要么(1)要求用户手动复制,(2)解码 `SHANNON_SERVE_TOKEN` 环境变量(跨进程不可靠),(3)VSCode 端"随机生成一个并改 daemon 配置"——三条路都不优雅。 |
| **过期/失效** | 不存在 | S0 §3.7 设计"401 → 清 token + 触发 `shannon.login`" | server 端无 TTL、无 refresh、无 rotation;`constant_time_eq` 永远比对同字符串。token 泄露 = 永久泄露直到用户重启 server。 |
| **多 token / 角色** | 不存在 | S0 未提及 | 仅一个全局 `--auth-token`,desktop + CLI + 扩展共用同一字符串;无 per-client scopes。 |
| **跨实例发现** | 不存在 | S0 风险 §1 已列 | 没任何 `/api/whoami`、`/api/server-info`、`/.well-known/shannon` 之类的探查端点。 |
| **shannon-core 与 shannon-server 共识** | **未统一** | S0 隐含假设"shannon serve = 完整 HTTP" | CLI 启的是 **`shannon-server`(只有 `/v1/sessions`)**,而不是 `api_server.rs`(有 `/api/*` / SSE / WS)。**S0 §3.1 描述的 `/api/query/stream`、`/api/approval/respond` 在 CLI 端不可达**;VSCode 扩展若接 CLI 必须改协议,或改用 desktop 那条老 server。 |

### 修补建议(最小补丁,伪代码)

**目标:**让 VSCode 扩展 0 人工即可拿到 token,S0 不需重做。

```rust
// 新增 crates/shannon-server/src/token_issuance.rs
pub struct IssuedToken {
    pub token: String,        // 32B rand, hex/base64
    pub issued_to: String,    // "vscode@hostname" or peer pid
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,  // 默认 7d
}

// 启动时(auto)若 --auth-token 未提供且 --bind loopback:
//   生成 1 个长期 token,落 ~/.shannon/serve-tokens.jsonl (mode 0600)
//   println! 出来 + 写到 argv-spawned client 的 stdin(可选)

// 端点选择:
//   1. 复用 SHANNON_SERVE_TOKEN 路径(用户显式 set) — 已存在
//   2. 新增 GET /v1/auth/token (loopback-only, 需 evidence of in-process)
//      — 拒绝外部 token 请求,但允许本机 native client 通过 SharedMemory/PID 校验
//   3. 让 VSCode 扩展在 first run 调  `shannon login --client=vscode` 子命令,
//      走同进程 spawn,token 走 stdout pipe 给扩展
//
// 选项 1 + 选项 3(子命令)是最小补丁:0 新增端点,0 新存储,~50 行 CLI 子命令
```

**对 S1 拆任务影响:**
- S1.1(~0.5d → 实际 0.8d)的 token patch 子任务:实现 `shannon login --client=<name>`(输出 token 到 stdout + `~/.shannon/serve-tokens.jsonl`,含 issued_at/expires_at/owner)
- 或退而求其次:扩展要求用户先在 terminal 跑 `shannon serve --auth-token $(openssl rand -hex 32)`,把 token 粘进 VSCode 设置。MVP 能跑,但掉粉。

---

## Q2. repo map 端点 — 详细

### 端点列表(实际存在)

**全部 `crates/shannon-server/src/`(不含 SSE handler 复用,与 query 协议不共用):**

仅 4 个端点,见 Q1 端的表。`/v1/sessions/:id/messages`(`routes/mod.rs:74-114`)走 SSE(`sse.rs`),但**不带 repo_map 参数**。

**`crates/shannon-core/src/api_server.rs` 全部 7 个端点:** 也**没有**任何 `/api/repo-map` / `/api/context/repo-map`。

### 底层能力(已就绪,无 HTTP 暴露)

`crates/shannon-repomap/src/lib.rs`:
- `RepoMap::for_workspace(cwd)`(line 92-94)— 全仓遍历 → `SymbolMap`
- `RepoMap::from_path(path)`(line 132-148)— 单文件
- `RepoMapCache::new(root)` + `RepoMapWatcher::start` —— 增量 watcher(line 25-58 注释)

调用方(`grep -rn "shannon_repomap\|RepoMap::for_workspace" crates/`):
- 仅 `crates/shannon-core/src/` 内部路径,具体定位:
  - `crates/shannon-core/src/repomap*` —— ✅ crate 内有调用方
  - 但这些调用方都是**直接 internal Rust 调用**(query engine 组装 system prompt),不是 HTTP

### 请求体 / 响应体 schema

**全部缺失** —— 必须从零设计。

### 流式 vs 一次性

N/A —— 端点不存在。

### ⚠️ 缺口

| 项 | 现状 | S0 假设 | 真实差距 |
|---|---|---|---|
| **HTTP 端点** | 0 个 | S0 §3.5 "调 `GET /v1/repo-map?workspace=...&focus=paths=...`" | 不存在,任一路径都不存在。 |
| **CLI `shannon` 子命令** | `crates/shannon-commands/src/builtin/repomap.rs` 存在,但需 check 它的输出形态 | S0 没明说 | (本次未读,留待 S1 启动时核查) |
| **`@tree-sitter` 在扩展端** | Rust 端使用;`package.json` 计划走 `shannon serve` | S0 §3.5 vs S0 §5 S1.2 | 决策点:**做 server 路由**(干净、复用 query engine)vs **扩展端独立跑 tree-sitter-wasm**(零扩展后端依赖)。 S0 默认前者。 |

### 修补建议(最小补丁,伪代码)

**最干净的方案:在 `shannon-server` 新增端点,复用 `shannon-repomap` crate。**

```rust
// 新增 crates/shannon-server/src/routes/repo_map.rs

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RepoMapQuery {
    pub root: PathBuf,                       // workspace root (loopback 信任)
    pub focus: Option<Vec<String>>,           // @-mention file/dir 限定
    pub max_tokens: Option<usize>,            // trim budget,默认 4000
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RepoMapResponse {
    pub markdown: String,                    // 系统 prompt 友好的 Render
    pub token_estimate: usize,
    pub file_count: usize,
}

#[utoipa::path(get, path = "/v1/repo-map", params(...))]
pub async fn get_repo_map(
    State(state): State<AppState>,          // 复用现有 auth + sessions
    Query(q): Query<RepoMapQuery>,
) -> Result<Json<RepoMapResponse>, StatusCode> {
    let mut m = shannon_repomap::RepoMap::for_workspace(&q.root)
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    m.trim_to_budget(q.max_tokens.unwrap_or(4000));
    Ok(Json(RepoMapResponse {
        markdown: m.to_system_prompt_markdown(),
        token_estimate: m.token_estimate(),
        file_count: m.map.files.len(),
    }))
}

// crates/shannon-server/src/lib.rs 加 .route("/v1/repo-map", get(routes::repo_map::get_repo_map))
// crates/shannon-server/src/lib.rs OpenApi #[openapi(paths(..., routes::repo_map::get_repo_map))]
```

**关键设计点:**
- **(a) 走 `v1/*` 而非 `/api/*`** —— CLI 当前服务是这个,扩展接它就与其他客户端一致;否则要把扩展接 desktop(`api_server.rs`),分裂两套
- **(b) 单文件 / 范围:** 简单先做 `root + focus(可选)`;增量(cache+watcher)留给 P3
- **(c) 输出 markdown 给 LLM,Json 给 UI:** 同时暴露 `?format=json|markdown`
- **(d) 成本:** 约 80–120 行 Rust + 6 个单测;~0.5d

**对 S1 拆任务影响:**
- **S1.2 估时 1d → 实际 1d**(已合理),但**前提是走 `shannon-server` + `/v1/repo-map`**;若改成 `/api/repo-map` 走 `shannon-core::api_server`,则桌面端需要文档同步,且 CLI 不直接受益。**建议:走 `shannon-server` 路径**。

---

## Q3. 端口冲突 — 详细

### 端口号(双方同)

| 来源 | 默认 | 可配置 | 文件:行 |
|---|---|---|---|
| **CLI `shannon serve`** | `33420` | `--port <u16>` | `crates/shannon-cli/src/main.rs:564` |
| **Desktop loopback API** | `33420`(常量 `LOOPBACK_PORT`) | hard-coded | `desktop/src/loopback_api.rs:33-35` |
| **`shannon doctor` 检查** | `33420` | hard-coded | `crates/shannon-cli/src/main.rs:2896` |
| **Gateway 客户端目标** | `http://127.0.0.1:33420` | env / config | `gateway/src/engine/httpClient.ts:19`;`gateway/src/engine/__tests__/httpClient.test.ts:13` |

### `find_free_port` / 端口冲突处理逻辑

| 来源 | 实现 | 文件:行 |
|---|---|---|
| **Desktop 端 ✅** | 启动 setup 时 `engine_discovery::probe_existing_engine()` 250ms HTTP OPTIONS 探测 `/api/ws`;若已有 server → 切 `EngineMode::External`(客户端连接现有),不重新 bind | `desktop/src/main.rs:291-320` |
| **Desktop 端 测试** | `desktop/src/engine_discovery.rs:75-132`(3 个单测覆盖 free / serving / unresponsive listener) | `engine_discovery.rs` |
| **CLI `shannon serve`** ❌ | **没有**:`shannon_server::run` 直接 `tokio::net::TcpListener::bind((host, port)).await`,失败直接 `Err(_)` 冒到 `run_serve_command`(`main.rs:1975-1977`),无冲突前的探测 | `crates/shannon-server/src/lib.rs:54`;`crates/shannon-cli/src/main.rs:1975-1977` |
| **CLI `shannon doctor`** | 仅 `is_port_free(33420)` 报告 OK/WARN,**不解决** | `crates/shannon-cli/src/main.rs:2862-2899` |

### 同时运行的真实形态

| 场景 | 谁先启 | 谁后启 | 结果 |
|---|---|---|---|
| **A) 单独 `shannon serve`** | CLI | — | 33420 绑定 `/v1/sessions` API ✅ |
| **B) 单独 `shannon-desktop`** | Desktop | — | 33420 绑定 `/api/ws`、`/api/query/stream` 等 ✅(Desktop Q4-A probe 后确认 free 才 bind) |
| **C) Desktop 先启 → 后 `shannon serve`** | Desktop(占 33420) | CLI(试图再绑 33420) | ❌ **CLI 失败**,报 `"address already in use (os error 98)"`,Desktop 不受影响 |
| **D) `shannon serve` 先启 → 后 Desktop** | CLI(占 33420) | Desktop(probe OPTIONS `/api/ws`,250ms 拿到 401 / 200) | ✅ Desktop 切 `EngineMode::External`,不重复 bind,**但**连到 CLI 上的 `/api/ws` —— **`/v1/sessions` 服务没有 `/api/ws`,OPTIONS 一定 404**,probe 会把它判成"unresponsive listener → Hosted",从而**错误地**也试图 bind 33420,**仍然冲突** |
| **E) VSCode 扩展 + Desktop** | Desktop / VSCode 同时启 | — | VSCode 用 SDK 的 SecretStorage + bearer token,Desktop 在 33420 监听;VSCode 直连 Desktop loopback(不需要 spawn `shannon serve`)✅ |
| **F) VSCode 扩展 + CLI(用户偏好 no-desktop)** | 用户手动 `shannon serve` | VSCode | ✅ (但要先解决 Q1 token UX) |

> ⚠️ **场景 D 是隐藏坑**:Desktop 的 Q4-A probe 只验 OPTIONS,有响应就 External,无响应就 Hosted。CLI 的 `/v1/sessions` 没有 `/api/ws`(只有 `/v1/sessions`、`/openapi.json`),OPTIONS `/api/ws` → 404,**没** 200,**没** connection-refused → probe 走 `Ok(Err(_response))` 还是 `Err(_)`?测一下:`engine_discovery::probe_at` 的 match 分支只判 `Ok(Ok(_response))` 为 External,其它都 Hosted —— 所以**遇到 CLI 进程占着 33420 时 Desktop 也会撞 EADDRINUSE**。这是当前 Q4-A 的 corner case,需修复。

### ⚠️ 缺口

| 项 | 现状 | 影响 |
|---|---|---|
| **CLI 端无探测** | `shannon serve --port` bind 失败即崩 | (场景 C)用户已经开 Desktop,再开 CLI 必败;无友好提示 |
| **Desktop 探测忽略 non-`/api/ws` 响应** | Q4-A 只校验 `/api/ws` OPTIONS | (场景 D)CLI 占着 33420 时 Desktop 也撞 EADDRINUSE |
| **无统一健康探针** | `/api/health` 走 `shannon-core::api_server`,`/v1/sessions` 端没有等价物(只有 `/openapi.json`) | 客户端无法同时探测 "是不是 Shannon 在跑" vs "是 Shannon 但接口不对" |
| **`shannon-server` 不支持 `--port 0`(OS 任意分配)** | 当前 `:54` 直接 bind | 写 `:0` 让 OS 选端口,但 CLI 默认是 33420,需 `--bind-once` 行为配套 |
| **gateway 默认端口** | `127.0.0.1:33430`(mobile pairing `/claudedocs/.../mobile-host-implementation-plan.md:115`),与 serve 错开 | ✅ 健康 |

### 修补建议(最小补丁,伪代码)

```rust
// Option A — 把 Q4-A 模式搬到 CLI
// crates/shannon-cli/src/main.rs run_serve_command 开头:
let engine_mode = engine_discovery::probe_at("127.0.0.1", port).await;
match engine_mode {
    EngineMode::External => {
        eprintln!("Shannon engine already listening on 127.0.0.1:{port}; exiting.");
        return Ok(());
    }
    EngineMode::Hosted => { /* fall through to bind */ }
}

// 同时把 desktop/src/engine_discovery.rs 共享到 shannon-core
// (避免 CLI / Desktop 双份实现)

// Option B — 干脆统一成一组端点
//   在 shannon-server lib.rs 加一个 GET /v1/health(同样公开,不需 auth)
//   让 Desktop 的 probe 改打 /v1/health 而不是 /api/ws
//   收益:CLI / Desktop / gateway 三方对"Shannon 在跑"的判据统一
```

**对 S1 拆任务影响:**
- 估时:**+0.3d**(Q4-A 抽到 shannon-core + CLI 调用 + 单测 1 个,合计 30 行)
- **强依赖:** 修这个之前不要"VSCode 扩展同时启 Desktop + CLI",否则首次安装者 50% 概率失败

---

## 决策

**总体建议:需补 ~0.8–1.0 d 修补再进 MVP,不可"直接进 S1.4"**

### 三项决定

| 决定项 | 推荐方向 | 估时 |
|---|---|---|
| **Q1 token UX** | **采纳 Q1 方案 选项 3** —— 新增 `shannon login --client=<name>` 子命令,生成长期 token 落到 `~/.shannon/serve-tokens.jsonl`,stdout 输出给调用方(扩展)。**MVP 不做 TTL / refresh**。 | 0.5d |
| **Q2 repo-map 端点** | 在 `shannon-server` crate 加 `GET /v1/repo-map?root=&focus=&max_tokens=`,输出 markdown + JSON,单文件/全仓二选一支持,trim 预算默认 4000。**不走 `api_server.rs`**(避免再次分裂)。 | 0.8d |
| **Q3 端口** | (1) 把 `engine_discovery` 抽到 `shannon-core`,CLI 与 Desktop 共用;(2) 改 probe 路径到 `GET /v1/health`(同时在 `shannon-server` 加这条免 auth 端点);(3) CLI 在 bind 前先 probe,External 直接提示"已运行,exiting clean"。 | 0.3d |

### 总体估时(refined S1 §5)

| | 旧估 | 新估 | 差 |
|---|---|---|---|
| S1.1 token | 0.5d | **1.0d** | +0.5d(login 子命令 + 多 client 注册) |
| S1.2 repo-map 端点 | 1d | **1.0d**(路径明确)| 0d |
| (新)S1.2b 端口冲突修复 | — | **0.3d** | +0.3d |
| S1.3 NDJSON vs HTTP 差异表 | 0.5d | 0.5d | 0d |
| S1.4–S1.11(原有 MVP) | 8.5d | 8.5d | 0d |
| **合计** | 11d | **11.3d ≈ 2.3w** | +0.8d |

### 决策矩阵(给父代理 handoff)

| 选项 | 利 | 弊 | 推荐? |
|---|---|---|---|
| **A 直接进 S1(原计划)** | 快 0.8d | 首次安装者高概率失败;扩展要求手敲 token;repo-map 必须扩展端自跑 tree-sitter-wasm | ❌ |
| **B 补三处再进 S1(本文推荐)** | 真"用户开箱即用" | 估时 +0.8d,需在 P2-8 内 P0-b 完成前部修补 | ✅ |
| **C token 走 OS keyring 手动拷** | 0 新代码 | UX 不能用,无法 marketplace 推广 | ❌ |

---

## 附录:验证路径(已读文件清单)

- ✅ `crates/shannon-cli/src/main.rs`(全部 5163 行,关键:`560-577` serve 命令,`1931-1978` run_serve_command,`2862-2899` port probe + doctor,`3780-3846` parse tests)
- ✅ `crates/shannon-server/src/{lib.rs,auth.rs,sessions.rs,sse.rs,routes/mod.rs}`(全部,P2-7 新增)
- ✅ `crates/shannon-core/src/api_server.rs`(全部 2462 行,99% 是测试,核心 ~200 行 handlers)
- ✅ `crates/shannon-repomap/src/lib.rs`(120 行公共 API)
- ✅ `desktop/src/loopback_api.rs`(93 行,P0.1)
- ✅ `desktop/src/engine_discovery.rs`(133 行含测试,Q4-A probe)
- ✅ `desktop/src/main.rs` 第 282-321 行 setup hook
- ✅ `crates/shannon-api-protocol/src/lib.rs`(只 wire types,无 routes)
- ✅ `crates/shannon-cli/Cargo.toml` / `crates/shannon-server/Cargo.toml` / `desktop/Cargo.toml`(依赖关系)
- ✅ `legacy-archives/shannon-code/editors/vscode/src/extension.ts` 第 1-80 行(确认 legacy 是 NDJSON 子进程,**非 HTTP**,不在 S0 假设的端口范围)
- ✅ `legacy-archives/shannon-code/editors/vscode/src/shannonClient.ts` 关键行 `196` —— `spawn(cliPath, args, {stdio: ['pipe', 'pipe', 'pipe'], env: ...})` 纯 NDJSON
- ✅ `gateway/src/engine/httpClient.ts:19` / `wsClient.ts:12-27` 确认 gateway 期望 `http://127.0.0.1:33420/api/ws` —— 也就是 **`shannon-core::api_server`** 那套,**不是** `shannon-server`。**再次证明 CLI serve 与 gateway 期望不一致**,VSCode 扩展若想走 `shannon serve`(CLI)需要单独支持 `/v1/sessions` 协议。

---

**S1 验证 spike 收口**:本文件**不含实施代码**;所有 token / repo-map / port 修补留给 P2-8 P0-b(再前面的子阶段),S1 §5 拆任务开工前先完成。
