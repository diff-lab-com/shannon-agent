# SaaS MCP: GitHub Issues/PR Server (P1-3 阶段 1)

> 状态:Planning · 估时:2–3d 单人 · 优先级:P1-3 最高 · 依赖:无
> 关联:[improvement-plan-2026-08.md §P1-3](../improvement-plan-2026-08.md) · [openworker-research §9.2](../openworker-research.md) · 父计划:5 个 SaaS MCP server 阶段 1/5

首个 SaaS MCP server。用户授权顺序:GitHub → Slack → Jira → Notion → Linear。本计划落地阶段 1,同步确立 v2–v5 复用模板(目录结构、auth 抽象、tool 注册、permission 接线、测试矩阵)。

---

## 1. 目标

在 `crates/shannon-mcp-saas/` 新建独立 MCP server,暴露 GitHub Issues/PR 6 个核心工具(`list_issues` / `get_issue` / `create_issue` / `comment` / `list_prs` / `review_pr`),通过 OAuth(交互式)+ PAT(token,headless)两种 auth 路径,密钥走 keyring 不落明文;接通 Shannon 现有 `PermissionRuleChecker` 的工具粒度权限分级(read vs write);让 REPL 一行 `cargo run` 即可发现并调用。阶段 2–5 的 Slack/Jira/Notion/Linear 严格复用 `crates/shannon-mcp-saas/<saas>/` 目录结构,只换 auth client + 工具集。

---

## 2. 架构(高层)

```
┌─────────────────────┐         ┌──────────────────────┐
│  Shannon CLI /      │  stdio  │  shannon-mcp-saas    │
│  Shannon Desktop    │ ──────▶ │  (Rust 二进制)        │
│  (MCP client)       │  JSON-  │                      │
│                     │  RPC    │  ┌────────────────┐  │
│  - 启动时 spawn      │         │  │ github/ 模块    │  │
│  - ~/.shannon/      │         │  │                │  │
│    mcp-servers.json │         │  │ auth.rs    ────│──┼──▶ GitHub OAuth App
│    注册 github      │         │  │ api.rs     ────│──┼──▶ api.github.com
│  - tools/list 列举  │         │  │ tools.rs   ────│──┼──▶ 注册 6 个工具
│  - tools/call 触发  │         │  │ permissions────│──┼──▶ ApprovalMode
│  - 走 PermissionRule│         │  └────────────────┘  │
│    Checker 分级     │         └──────────────────────┘
└─────────────────────┘                  │
                                         ▼
                                ┌──────────────────────┐
                                │  Keyring / OS 凭据    │
                                │  service=shannon     │
                                │  user=github/<acct>  │
                                └──────────────────────┘
```

关键边界:
- **MCP 协议层**:复用 `shannon-mcp/client` 的 transport / schema 序列化,**不**重复造轮子。
- **Auth 抽象**:本 crate 内部定义 `trait SaasAuth { async fn token(&self) -> Result<...>; }`,GitHub/Slack/Jira/Notion/Linear 各自 impl。
- **工具权限**:每个 tool 标注 `permission: Read | Write | Destructive`,在 `tools/list` response 暴露,Shannon 端 `PermissionRuleChecker` 据此走 `ApprovalMode` 自动分流(写工具默认 confirm,删/危险 confirm-or-deny)。

---

## 3. 文件锚点

### 新增
- `crates/shannon-mcp-saas/Cargo.toml` — crate 元数据,依赖 `shannon-mcp`(transport)+ `tokio` + `reqwest` + `keyring` + `serde` + `schemars`
- `crates/shannon-mcp-saas/src/main.rs` — CLI 入口,读 `argv[0]` 决定启动哪个 SaaS 子模块(GitHub 先实,Slack/Jira/Notion/Linear 后续 arg 落地)
- `crates/shannon-mcp-saas/src/lib.rs` — 公共类型:`SaasAuth` trait、`ToolDef`、`Permission` 枚举
- `crates/shannon-mcp-saas/src/github/mod.rs` — 模块入口,聚合 `register_tools(server)`
- `crates/shannon-mcp-saas/src/github/auth.rs` — OAuth(PKCE)+ PAT 双路径,token 存 keyring(`service=shannon-mcp`, `user=github/<account>`)
- `crates/shannon-mcp-saas/src/github/api.rs` — REST 客户端(`octocrab` 或手写 `reqwest`),内置 5000/h rate-limit 处理(`x-ratelimit-remaining` 头 + 退避)
- `crates/shannon-mcp-saas/src/github/tools.rs` — 6 个 tool 定义 + schema + handler
- `crates/shannon-mcp-saas/src/github/types.rs` — `Issue` / `PullRequest` / `Review` / `Comment` 强类型
- `crates/shannon-mcp-saas/src/github/tests.rs` — mockito 回放 + 错误用例
- `docs/integrations/github.md` — 用户文档(配置、权限、示例)

### 复用
- `crates/shannon-mcp/src/` — 现有 MCP server trait 实现模式(`crates/shannon-mcp/src/server.rs` 暴露 `Server::register_tool`);**先 grep 确认公有 API**(`grep -rn "pub fn\|pub trait" crates/shannon-mcp/src/`),如果 API 不直接服务 sub-server 形态,新增 thin wrapper 而**不**改 `shannon-mcp` 本身(避免爆炸半径)。
- `desktop/src/commands_connections.rs` — 平台 credential/keyring 模式参考(读其 `keyring::Entry` 用法、`OAuth flow` 抽象);**只**模仿,不直接依赖 desktop crate(MCP server 必须可独立 CLI 启动)。
- `desktop/src/gateway_supervisor.rs` — connector gateway 监督模式(健康检查 + 自动 restart);GitHub MCP 暂不引入独立 supervisor,记录为 P1-3 v6 通用基础设施 backlog。
- `~/.shannon/mcp-servers.json` — server 注册(新增 `github` 条目,command/path/args/env)
- `crates/shannon-mcp/src/tools/list.rs` — `tools/list` 协议格式参考(grep 现有 `register_tool` + `schema` 序列化),保证 6 工具格式与 Shannon 客户端期望一致。

### 不动
- `shannon-core`、`shannon-ui`、`shannon-commands` — 0 改动
- `shannon-mcp` 主 crate — 仅参考,**不**修改;若发现必须修改的硬阻塞,记入 Open Questions 而非本计划变更

---

## 4. 实施步骤

每步含文件锚点 + 验收 + 估时。完成任一步后必须 `cargo check -p shannon-mcp-saas` 通过 + `just dev` 演草稿不破坏现有 10258 测试。

### Step 1: server skeleton (0.5d)

- **新建 crate**: `crates/shannon-mcp-saas/Cargo.toml`,加 `shannon-mcp-saas` 到 workspace 根 `Cargo.toml` `[workspace.members]` 列表。
- **实现 `main.rs`**: 受 `argv[0]`(实测为 `github`);暂只 stub 一个 `tools/list` 返回空数组,验证客户端能发现。
- **Cargo.toml 依赖**: 
  - `shannon-mcp = { path = "../shannon-mcp" }` — 复用 transport
  - `tokio = { version = "1", features = ["full"] }`
  - `serde` / `serde_json` / `schemars` — schema
  - 暂不引入 `reqwest` / `keyring`(Step 2/3 再加)
- **`~/.shannon/mcp-servers.json` 注册**:
  ```json
  {
    "mcpServers": {
      "github": {
        "command": "cargo",
        "args": ["run", "-p", "shannon-mcp-saas", "--", "github"],
        "env": {}
      }
    }
  }
  ```
- **验收**:
  - [ ] `cargo build -p shannon-mcp-saas` 通过
  - [ ] `cargo run -p shannon-mcp-saas -- github` 启动,stdin JSON-RPC 收到 `initialize` 响应
  - [ ] REPL `/mcp` 或 `tools/list` 能看到 `github` server 注册(即使工具列表空)
- **估时**:0.5d

### Step 2: auth (0.5d)

- **OAuth flow**(`auth.rs`):
  - 标准 authorization code + PKCE(`code_verifier` / `code_challenge`)
  - 本地 callback server 端口 8765(可配置),`state` 参数防 CSRF
  - 用户授权后,exchange code → token,存储到 `keyring`(`service=shannon-mcp-saas`, `user=github/<login>`)
  - 文件 `~/.shannon/mcp-servers.json` 的 `env` 可注入 `GITHUB_CLIENT_ID` / `GITHUB_CLIENT_SECRET`
- **PAT fallback**: `GITHUB_TOKEN` env var 优先;无则启动 OAuth
- **Token 读取**: `auth.rs::TokenProvider::get() -> Secret<String>`,封装 keyring + env 两路径
- **`api.rs` 速率限制**: 每次响应检查 `x-ratelimit-remaining`;接近 0 时主动 sleep 到 `x-ratelimit-reset`;遇 429 读 `retry-after` 退避;指数退避上限 60s,最多 3 次
- **验收**:
  - [ ] `cargo test -p shannon-mcp-saas github::auth::tests` 覆盖 OAuth callback 解析 / PKCE 校验 / token 持久化
  - [ ] keyring 失败回退 env var 路径有显式 log(不 panic)
  - [ ] 速率限制 mockito 测试: 模拟 `x-ratelimit-remaining: 0` 触发等待
- **估时**:0.5d

### Step 3: 6 tools (0.5–1d)

每个工具签名(JSON Schema)与 GitHub REST API 对齐,handler 调 `api.rs` 转换 unwrap 到 `github::types::*`:

| 工具 | 权限 | 输入 | 输出 |
|---|---|---|---|
| `list_issues` | Read | `{owner, repo, state?: "open"\|"closed"\|"all", since?: ISO8601, per_page?: u8, page?: u32}` | `Vec<Issue>` |
| `get_issue` | Read | `{owner, repo, number: u32}` | `Issue` |
| `create_issue` | Write | `{owner, repo, title, body?, labels?: Vec<String>, assignees?: Vec<String>}` | `Issue` |
| `comment` | Write | `{owner, repo, number: u32, body}` | `Comment` |
| `list_prs` | Read | `{owner, repo, state?: ..., since?: ..., per_page?, page?}` | `Vec<PullRequest>` |
| `review_pr` | Write | `{owner, repo, number: u32, event: "APPROVE"\|"REQUEST_CHANGES"\|"COMMENT", body?, commit_id?: String}` | `Review` |

- **`tools.rs::register_tools(server)`**: 6 个 tool 用 `shannon-mcp` 的 `register_tool!(server, "<name>", schema, handler)` 宏(或同名 builder)注册;每个 tool 元数据含 `permission: "Read"|"Write"` 字段,Shannon 端 `PermissionRuleChecker` 透传到 `PermissionDecision`。
- **handler 错误模型**: `api.rs` 统一 `ApiError { NotFound, Unauthorized, RateLimited, Forbidden, ServerError }`;handler 转 MCP error code(`-32603` / `-32001..-32005`)。
- **验收**:
  - [ ] `cargo run -p shannon-mcp-saas -- github` 经 stdin JSON-RPC `tools/call` 6 个工具名全部识别
  - [ ] handler 单测覆盖每个工具的 happy path(可用 mockito fixture)
- **估时**:0.5–1d

### Step 4: 测试 (0.5d)

- **mockito fixture**: `crates/shannon-mcp-saas/src/github/tests.rs` 覆盖:
  - `list_issues` happy path(2 条 fixture)
  - `create_issue` 409 conflict(duplicate title)
  - `comment` 201 + 404(repo not found)
  - `review_pr` 422(invalid event)
  - 401 unauthorized(revoked token)
  - 429 rate-limited(断言退避 + 重试)
  - 5xx(断言失败而非 panic)
- **集成**: `cargo test -p shannon-mcp-saas` + Shannon 端 `just test`(确认 0 回归)
- **OAuth flow fake**: 不接真实 GitHub,提供 `McpAuth::mock_with_token(...)` 测试替身,`#[cfg(test)]` gated
- **REPL 手动 QA**(Step 5 文档同步引用): 在 test repo(`ericdong/test-mcp`)上跑 `tools/call create_issue` + `comment` 实际 GitHub API
- **验收**:
  - [ ] mockito 7+ case 覆盖
  - [ ] `just test` 全绿
  - [ ] 真实 GitHub test repo 端到端通过(`curl` 验证 issue 已建)
- **估时**:0.5d

### Step 5: 文档与权限分级 (0.5d)

- **`docs/integrations/github.md`** 内容:
  - 概览(为什么 office-track 第一个,GitHub 覆盖率最高)
  - 三步配置:① 创建 GitHub OAuth App + 填 callback `http://localhost:8765/callback`;② `GITHUB_CLIENT_ID` / `GITHUB_CLIENT_SECRET` 注入 `mcp-servers.json` env;③ `~/.shannon/mcp-servers.json` 注册(给完整 JSON);④ 首次启动走 OAuth 授权
  - PAT 简化路径(`GITHUB_TOKEN`,适合 headless / CI)
  - 6 工具一览 + 权限(read vs write)+ 示例调用(`tools/call list_issues` JSON)
  - 速率限制说明(5000/h/route)、密钥存储(keyring)、卸载(`keyring delete`)
  - 故障排查:401(revoke 后重 OAuth)、403(Scope 不足)、429(自动退避)
- **权限分级接通**: 6 工具的 `permission` 字段在 `tools/list` 返回;Shannon 端 `PermissionRuleChecker` 据此对 create/comment/review 默认走 `Confirm`(`ApprovalMode::Default`),list/get/list_prs 走 `Allow`。**这是关键打通项**——若 `PermissionRuleChecker` 不读 tool metadata,需扩展 `shannon-mcp` server trait(影响面 < 50 行,记入 P1-3 完成报告)。**不**改 `shannon-core` 权限 enforcer 本体。
- **验收**:
  - [ ] `docs/integrations/github.md` 完成,内部链接在 `docs/integrations/README.md` 入口登记(新增该 README 列出 5 SaaS 进度)
  - [ ] REPL 实测: 未授权 `create_issue` 弹 confirm;授权后直过
  - [ ] `[MCP policy]` 规则(`.shannon/profiles/*.toml`)能按 tool name 覆写默认
- **估时**:0.5d

---

## 5. 验收

合并门槛(全勾才合):

- [ ] `cargo run -p shannon-mcp-saas -- github` 启动 stdio JSON-RPC server
- [ ] `tools/list` 暴露 6 个工具,metadata 含 `permission` 字段
- [ ] mockito 测试覆盖 6 工具 + 错误(7+ case)
- [ ] `just test` 全绿(零回归)
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] `cargo fmt --all -- --check` clean
- [ ] REPL 实测:`create_issue` + `comment` 在真实 GitHub test repo 通过
- [ ] 密钥走 keyring(`security find-generic-password -s shannon-mcp-saas -l github/<login>` 可见),不落明文配置文件
- [ ] `docs/integrations/github.md` 完成,含 OAuth + PAT 双路径
- [ ] 工具粒度权限接 Shannon `ApprovalMode`(Write 工具默认 confirm,Read 直过)

---

## 6. 风险

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| GitHub 速率限制(5000/h/route,部分端点 30/h)踩坑 | 中 | 中 | client 内置 `x-ratelimit-remaining` 预读取 + `retry-after` 退避;写入 doc 提示高吞吐场景走 GitHub App + separate token |
| OAuth state/csrf 漏洞 | 低 | 高 | 标准 PKCE + `state` 参数(随机 32B)、callback 端口仅绑 `127.0.0.1`、state 一次性消费 |
| `shannon-mcp` crate 不暴露 sub-server friendly API | 中 | 中 | Step 1 先 `grep` 确认;必要时在 `shannon-mcp-saas` 自维护 transport wrapper(MCP 协议 ≤ 200 行,代价可控);**不**改 `shannon-mcp` 本身 |
| 工具粒度权限未接通阻塞 P1-3 整体价值 | 中 | 高 | 先最小方式:在 `tools/list` 返回自定义 metadata;Shannon 端 `PermissionRuleChecker` 扩展读 tool metadata(影响面 < 50 行,放在本计划 Step 5);保留 escape hatch:`.shannon/profiles/*.toml` 显式 allow/deny 工具名 |
| octocrab 引入增加编译时间 | 中 | 低 | 直接 `reqwest` + 手写 REST 6 端点;或 `octocrab` 限定 minimal features;依赖倒置写在 `Cargo.toml` 注释(*KEEP: web 2026-08-02,Aider-style 选型,本计划评审通过则用 octocrab*评审 Open Question 1) |
| OAuth App 创建门槛(新用户首次) | 中 | 低 | PAT 路径文档置顶;OAuth 路径标"多用户/共享场景" |

---

## 7. 复用到其它 SaaS 的模式

`crates/shannon-mcp-saas/` 设计为**多 SaaS 容器**:每个 SaaS 一个子模块,目录结构 `github/ → slack/ → jira/ → notion/ → linear/`,各自拥有 `auth.rs` + `api.rs` + `tools.rs` + `types.rs` + `tests.rs` + `mod.rs` 6 个文件。`main.rs` 通过 `argv[0]` 子命令 dispatch,`lib.rs` 提供 `SaasAuth` trait + `ToolDef` + `Permission` 公共类型,子模块各自 impl。Tool 注册统一走 `register_tools(server, registry)`,metadata 透传到 `tools/list`,Shannon 端 `PermissionRuleChecker` 据此走 `ApprovalMode`。

**各 SaaS 缺啥(简表)**:

| SaaS | Auth 路径 | API 关键 | 工具候选(≥3) | 复用 GitHub 部分的占比 |
|---|---|---|---|---|
| Slack | OAuth v2 + Bot Token | Web API(分页 cursor)+ Events API | `post_message` / `search` / `read_channel` / `thread_reply` / `list_channels` | ~85%(types + permission yagni) |
| Jira | OAuth 2.0(3LO)+ API Token | REST v3 + JQL | `search_issues`(JQL)/ `get_issue` / `create_issue` / `transition` / `add_comment` | ~80%(JQL vs GitHub since 语义差异) |
| Notion | OAuth + Internal Integration Token | REST API(分页 cursor)| `search_pages` / `get_page` / `append_block` / `create_page` / `update_property` | ~70%(block 模型 vs GitHub flat 字段) |
| Linear | OAuth + Personal API Key | GraphQL | `list_issues` / `get_issue` / `create_issue` / `update_status` / `add_comment` | ~70%(GraphQL 替换 REST,工具更紧凑) |

**复用清单**(跨 5 SaaS 共用):
- `keyring` entry 模式(service = `shannon-mcp-saas`,user = `<saas>/<account>`)
- 速率限制 `RateLimited` 错误模型 + 退避策略
- `tools/list` metadata `permission` 字段协议
- `permissions.rs`(Shannon 端 wire-up)每加新 SaaS 仅需 1–2 profile 规则
- `docs/integrations/<saas>.md` 模板(配置 + 工具表 + 故障排查)

**不复用的特例**:
- Slack 需本地 WebSocket 监听(Events API)—— P1-3 v5 之后单独计
- Notion 的 block-based schema 与"字段"概念差异大,`types.rs` 需独立工具类
- Linear GraphQL 需手写查询文档,不复用 rest types

**阶段节奏(预估)**:
- v1 GitHub: 2–3d(本计划)
- v2 Slack: 3d(websocket 增 1d)
- v3 Jira: 2d
- v4 Notion: 2.5d(block 模型复杂)
- v5 Linear: 2d(GraphQL 替换 REST)
- 合计: ~12–13d 人月单人, P1-3 总估时 2–3w 单人成立

---

## Open Questions / 评审请求

1. **HTTP client 选型**: `octocrab`(生态全,编译 +30s)vs `reqwest` 手写(6 端点可控,编译轻)。倾向 `octocrab` 用于 GitHub/Slack,Jira/Notion/Linear 用 `reqwest`?或统一 `reqwest`?—— 当前计划写 `octocrab` 可选,Step 3 落地前请 ericdong 拍板。
2. **OAuth 端口冲突**: 默认 8765,若与用户本地服务冲突,fallback 启动顺序扫描 8766–8769 哪个空闲?接受即落。
3. **`shannon-mcp` 是否升级为支持 metadata 透传**: 现状 `tools/list` schema 是否包含自定义字段(如 `permission`)?若否,需要小改 `shannon-mcp`(~50 行)。影响 P1-3 全 5 阶段。倾向先改 `shannon-mcp` 一次,后续 4 阶段复用——请确认。
4. **Slack `Events API` websocket 是否纳入 P1-3 v2**: 当前 5 SaaS 工具集是 pull-only(查询/创建/评论),Slack 实时事件是个反向能力(OAuth + 长连接)。建议 v2 加 `post_message` 即可,websocket 后续 P3 单独立项。请确认。
5. **是否同时建 `docs/integrations/README.md` 总目录**: 列 5 SaaS 进度 + 配置入口。倾向建,落地 Step 5。请确认。
6. **GitHub App vs OAuth App**: OAuth App 满足大部分个人/团队场景;GitHub App 适合 ≥5 仓库、高吞吐、有 webhook。如果 P1-3 不需要 GitHub App,留作 backlog。请确认。

---

*评审通过后,本计划进入执行。完成步骤 1–5 全部勾选后,同步出 `cr-2026-08-XX-saas-mcp-github.md` 变更报告 + 父计划 P1-3 进度更新。*

---

## 7a. DRY trigger notes (added after step 3 — Slack)

After landing `crates/shannon-mcp-saas/src/slack/`, the following duplication
exists across `github::tools` and `slack::tools` (each a YAGNI-shaped
copy that would have to be repeated for every new SaaS):

1. **Per-SaaS `McpTool` trait surface (8 methods)** — `name` /
   `description` / `input_schema` / `is_*` / `required_permission` /
   `execute`. ~12 lines of identical signature in each SaaS's
   `tools.rs`.
2. **`XxxServerTool(Box<dyn McpTool>)` adapter struct** —
   `github::tools::GithubServerTool` and `slack::tools::SlackServerTool`
   are mechanical forwarders. ~40 lines each.
3. **`XxxError → McpError` blanket impls** — `From<ApiError>` /
   `From<ToolError>`, plus the `require_string` / `require_write` /
   `optional_*` arg-parsing helpers (`tools.rs`).
4. **JSON-RPC dispatch loop** in `server.rs` is already trait-agnostic
   (operates on `Box<dyn ServerTool>`), so it does **not** count as
   duplication.

### When to revisit

- Trigger A (must): at step 5 (Linear, the 3rd SaaS) — at that point
  the three `McpTool` trait duplicates are a clear pattern, the adapter
  struct is clearly mechanical, and a single `trait McpTool` in
  `shannon-mcp-saas::tool` (or in `shannon-mcp` itself) plus a single
  blanket `impl<T: McpTool> ServerTool for T` would remove ~120 LoC.
- Trigger B (optional): as soon as a non-Slack / non-GitHub API surfaces
  a different error-mapping shape (Notion's per-block validation, Jira's
  JQL-vs-state machine), the current `From<ApiError> for McpError` blanket
  breaks down and we need a `McpTool::map_error` hook. Do not pre-build it.
- Do **not** promote `server.rs` to `shannon-mcp` yet — the local
  copy is 230 lines, and `shannon-mcp` is a client crate that doesn't
  need a server loop; YAGNI holds.

### Why this did not block step 3

- The per-SaaS duplicates are ~50 LoC, comfortably below the DRY
  threshold.
- The adapter struct is the actual price of supporting
  `Box<dyn McpTool>` per SaaS without `dyn-clone`; it would only
  disappear if we adopted a `dyn-clone` dep and trait-object
  `Clone`, which is a bigger refactor than the SaaS work.
- The JSON-RPC loop is **already** shared.
