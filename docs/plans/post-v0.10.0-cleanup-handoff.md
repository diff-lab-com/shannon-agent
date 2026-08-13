# Post-v0.10.0 清理与后续任务 Hand-off

> **日期**: 2026-08-13
> **分支族**: `chore/ci-semver-baseline-and-pnpm-v6` (#65)、`chore/deprecate-secrets-env-store` (#66)
> **背景**: v0.10.0 已发版(tag `v0.10.0` → `0c57ef92`,21 assets)。本 hand-off 记录发版后的清理工作成果、一项被否决的任务及其根因分析,以及交接到下一波(Wave 7)的剩余任务。
>
> **🔄 二次核查更新(2026-08-13)**:§3 任务规划已据实修订 —— #65/#66 确认已合并;secrets.env 删除项去掉宽限期(项目未上线、无外部消费者);sandbox landlock bug 重新定性为大面积编译失败(非低成本);per-model 定价/models.dev 降级为「待评估」(ADR-0005 已标记落地)。详见各节 `🔄` 标注。

---

## 1. 本轮完成项(6 项任务,5 完成 / 1 否决)

| # | 任务 | 状态 | 产出 |
|---|------|------|------|
| 1 | 删除已合并的陈旧远程分支 | ✅ | 15 条 squash-merge 分支已清(用 `gh pr list --state merged` 验证,`merge-base --is-ancestor` 对 squash 无效) |
| 2 | Semver baseline `v0.8.0 → v0.10.0` | ✅ | PR #65(`ci.yml:326`)。原 baseline 落后两个发版 |
| 3 | `pnpm/action-setup@v4 → @v6` | ✅ | PR #65(`release.yml:254`)。v4 跑在 Node 20(已 deprecated),v6 跑 Node 24,`version` 入参兼容 |
| 4 | Add Provider modal 暴露 allowlist | ❌ 否决 | 见 §2 分析 —— G5 早已完成,且实现存在 kind↔slug 命名空间错配 |
| 5 | 清理 `secrets.env` 僵尸存储 | ✅ | PR #66。`persist_secrets` + `default_secrets_path` 标 `#[deprecated]` |
| 6 | 移除 legacy `providers.json` 迁移码 | ✅ | PR #61 已完成(本轮仅核对,无新增改动) |

另:NSIS 下载 flake 的持久修复已合入(PR #64,`f5a2b329`)—— 在 `tauri-action` 前用 `pwsh` 预缓存 NSIS toolchain(`curl --retry` + SHA1 校验 + 解压到 `%LOCALAPPDATA%\tauri\NSIS\`),忠实复刻 `get_and_extract_nsis`。

### 🔄 PR 合并状态确认(2026-08-13 二次核查)

- **#65** ✅ **已合并**(`ff61072b` 前序 `48e3fd91`,2026-08-13 08:22 UTC)。`SEMVER_BASELINE` 现为 `v0.10.0`(`ci.yml:326` 已生效)。
- **#66** ✅ **已合并**(`ff61072b`,2026-08-13 08:36 UTC)。`persist_secrets` / `default_secrets_path` 已标 `#[deprecated]`。

> 本 hand-off 文档通过 `1342d93a` 直接 commit 到 `dev`(关联 PR #67 已 CLOSED,非 merge —— 文档内容已落地,无需再处理)。

---

## 2. 否决项分析:#4 为何不做(关键学习)

**原任务**:在 Add Provider modal 里,按 `SHANNON_*_PROVIDERS` allowlist 把不允许的 provider kind 灰掉。

**否决理由(双重)**:

### 2a. 功能早已存在(G5 ✅ DONE)

ADR-0005 G5 明确标记完成:`getProviderAllowlist()` TS wrapper + `ModelsSettings.tsx` 里的 `enabled_providers` 编辑器(行 639–729,`ProviderAllowlistSection`)。allowlist 已作为 **桌面设置 UI 中的一个开关** 暴露,而非在 Add Provider modal 里灰掉 kind。原任务出于对 "Desktop tail" 措辞的误读。

### 2b. 实现存在 kind↔slug 命名空间错配(更深层)

即便 G5 未做,原方案也是错的。两个命名空间:

- **allowlist 内容 = provider slug**(`LlmProvider` 的 `Display` impl,`crates/shannon-engine/src/api/types.rs:325`):`anthropic`、`openai`、`deepseek`、`ollama`、`zhipu`、`moonshot`、`minimax`、`custom`、…
- **Add Provider modal 操作 = kind**(`AddProviderModal.tsx` 的 `KIND_INFO` 键 / `QUICK_FILL.kind`):`anthropic`、`openai`、`deepseek`、`ollama`、`openai-compatible`

错配点:
1. **不存在 `openai-compatible` slug**。GLM / Kimi / MiniMax 三个 quick-fill chip 的 `kind` 都是 `openai-compatible`,但它们对应的真实 slug 是 `zhipu` / `moonshot` / `minimax`。按 `kindAllowed(qf.kind)` 判断,只要 allowlist 生效,这三个 chip 就会被错误禁用 —— 哪怕它们被显式允许。
2. `openai-compatible` 是所有 OpenAI 兼容端点(Zhipu / Moonshot / SiliconFlow / Groq / 自托管…)的统一逃生通道,按 allowlist 灰掉它几乎从不是正确行为。

**结论**:modal 工作在 kind(5 种连接形态),allowlist 工作在 slug(20+ provider)。没有干净的 1:1 映射。正确的 "modal 感知 allowlist" 需要一个 kind→slug-set 映射层(`openai-compatible` → `[zhipu, moonshot, minimax, groq, …]`,且仅当该集合与 allowlist 完全不相交才禁用)。这是非平凡工作,而 G5 已经让用户能看见并管理 allowlist —— 边际价值低、破窗风险高,故否决。

> **若未来仍想在 modal 给出 "该 provider 会被 resolver 静默丢弃" 的提示**:正确做法是在 quick-fill chip 上加 `slug` 字段(anthropic→`anthropic`、glm→`zhipu`、kimi→`moonshot`),对有明确 slug 的 chip 按 slug 判断;`custom` chip 与 `openai-compatible` kind dropdown 永远不禁用。不要按 kind 判断。

---

## 3. 剩余任务(交接 Wave 7)

> **🔄 核查后修订(2026-08-13)**:下表已据实调整优先级与定性。判定标准:真实性(代码/编译证据)、必要性(项目未上线,无外部消费者 ⇒ 可省去宽限期)、成本(据 `cargo check` 实测而非推测)。

### 3.1 本轮衍生(小,确定)

- **[#5 跟进] ✅ 已实施 —— 删除废弃的 secrets.env 项(2026-08-13)**:删 `config_migration.rs` 整个模块(274 行;模块仅剩 deprecated primitives)+ `lib.rs` 的 `pub mod` / `pub use` + `main.rs` / `unified_config.rs` 两处文档注释引用。**验证全绿**:`cargo check --workspace` ✅(28s)、CI-exact `cargo clippy --workspace -- -D warnings -A unknown-lints -A clippy::collapsible_if` ✅(exit 0,41s)、`cargo fmt --all -- --check` ✅、`cargo test -p shannon-core --lib` ✅(2601 passed,较 2606 少 5 = 删除的 tests)。
  - **semver 状态**:删 public item = breaking;baseline `v0.10.0` 的 semver-check 会报(预期)。下次发版须为 **0.11.0**(workspace version bump 跨 3 处:workspace.package + 8 内部依赖 + desktop hardcoded,见 `[[semver-check-baseline-version-bump]]`)。**本次未擅自动版本号**(发版决策),改动留 dev 待发版时一并 bump;若现在开 PR,semver-check job 会红(预期)。
- **[#2 跟进] 🔄 baseline bump —— 已部分完成,剩余为例行维护**:`SEMVER_BASELINE` 现已是 `v0.10.0`(#65 已合并生效)。剩余仅为「v0.11.0 发版后推到 v0.11.0」的例行操作,**非一次性任务**,可从 Wave 7 清单移出,纳入发版 SOP。

### 3.2 既有未决(中大)

- **[W7-1 / P3-7] 沙盒执行后端**:`docs/spikes/p3-7-sandbox-s0.md` 状态 **Draft**(待 ericdong 评审),4 阶段路线(~7w)。
  - 🔄 **bug 重新定性(非低成本)**:实测 `cargo check -p shannon-core --features landlock` 暴露**大面积编译失败**,不止 `sandbox.rs:38` 一处。错误清单:`Bitflags`→`BitFlags`(E0432)、`Ruleset` 未声明类型(E0433)、`handle_access` 方法找不到(E0599)、`AccessFs::from_bitflags` 变体不存在(E0599)、`Access` 是 trait 不是类型(E0782,涉 `sandbox.rs:~1600`)。**整块 landlock 后端需按 landlock 0.4.5 API 重写**,原 hand-off「低成本可立刻修的一行拼写」判断**失真**。该 feature 默认关闭、CI 不跑,故长期未暴露。结论:不单独修,随 spike P3-7 方向定稿一起重构(评审很可能重写该模块)。
- **[W7-2 / P3-1] ✅ 评估完成 —— 主体已落地,移出 Wave 7**:核查 ADR-0005(行 337–339)明确标记「Per-model pricing (P0-1/P0-2)、models.dev 动态刷新 + LiteLLM 定价 (P1-6)、tiers + `/model --tier auto` (P2-7)**all land in the desktop now via `list_models` + `ModelInfo`**」。代码侧完整:`model_registry/`(静态 catalog 定价 + `tier.rs` 按成本排序)、`query_engine/litellm.rs`(24h TTL 社区定价 overlay)、`model_registry/dynamic.rs`(models.dev 动态模型层,显式不携定价 → 依赖 litellm overlay 补)、`shannon-api-protocol::ModelInfo`、`shannon-commands/builtin/cost.rs`。**无 P0 gap**;唯一设计取舍:models.dev 动态模型不带定价,无 litellm overlay 时为 `0.0`(已知,非 bug)。**移出 Wave 7 清单**。
- **[OAuth provider 接入] 🔄 评估完成 —— 多阶段工程,本次给出分阶段计划(未写接入代码)**:
  - **关键发现(比原 hand-off 更深)**:`oauth.rs`(1026 行,18 测试)是**模拟骨架**,非可用客户端 —— 模块自承「In a real implementation, the user would visit this URL」:`authorization_url` 自生成 code 存内存 `pending_codes`,`exchange_code` 从内存取(**无真实 HTTP token exchange**)、无持久化、XOR 加密(弱)、无 PKCE、无 state 校验。`CredentialRef`(`shannon-types`)**无 OAuth 变体**(仅 Env/Store/Keyring/InlineLegacy/Ephemeral);provider 认证经 `provider_resolver::resolve_credential` 完全不经 OAuth。codebase 另有两套独立 OAuth:`shannon-tools/mcp_auth.rs`(MCP server)、`shannon-mcp-saas/jira/auth.rs`(Jira)。
  - **分阶段(~2-3 周)**:
    - **S0 — oauth.rs 生产化(不依赖决策)**:真实 code→token exchange(reqwest POST token_url)、PKCE、state 校验、持久化 token store(复用 `~/.shannon/credentials/`)、真实 refresh。~3-5d。
    - **S1 — `CredentialRef::OAuth` + resolver**:新增变体(semver-breaking,随 0.11.0)+ `resolve_credential` 处理(加载 token → 过期则 refresh → 返回 access_token)。~2-3d。
    - **S2 — CLI `/connect` OAuth 流(依赖决策 B)**:loopback callback server + 浏览器开 authorize URL + exchange + 存 token。~2-3d。
    - **S3 — desktop Add Provider modal(依赖决策 A+B)**:OAuth button + `shannon://oauth/callback` 深链接 + Tauri 收口。~3-5d。
    - **S4 — token 生命周期**:后台刷新、过期告警、撤销。~2d。
  - **需决策(写接入代码前必须定,否则返工)**:**A. 首个 OAuth provider**(Google Gemini / Azure OpenAI / 其他?);**B. callback 收口**(CLI loopback HTTP server vs desktop 深链接 `shannon://oauth/callback`,是否统一?)。

---

## 4. 持久教训(本轮新增)

1. **kind ≠ slug**。Provider 的 "连接形态"(kind,5 种)与 "provider 标识"(slug,20+)是两个命名空间,在 UI/allowlist/resolver 之间穿梭时必须显式映射。详见 §2b。
2. **废弃 ≠ 删除(semver)**。`#[deprecated]` 是非破坏性的;真正删除 public item 才触发 semver-breaking。宽限期内用 `#[allow(deprecated)]` 保住 `pub use` 即可,既给编译期提示又不破 gate。对 0.x,breaking 仍需 minor bump。
3. **squash-merge 使 `merge-base --is-ancestor` 失效**。验证分支是否已合并用 `gh pr list --state merged --json headRefName`,不要用祖先检查。
4. **rustfmt 会把尾随 `//` 注释拆到下一行**。`#[attr] // comment` 会被格式化成两行;写 `#[allow(...)]` 带行内注释时直接写成两行省一次 fmt 往返。

---

## 5. 验证记录

- #65:两处均为版本字符串单行替换,无 `run:` 命令/不可信输入(无注入面)。`SEMVER_BASELINE` 改动仅影响 semver job 的 diff tag;`pnpm/action-setup@v6` 由下次 tag 触发的 `release.yml` 实际验证(CI 不在 PR 上跑 release.yml)。
- #66:`cargo clippy --workspace -- -D warnings`(CI-exact,含 workspace allow-list)clean;`cargo fmt --all -- --check` clean;`cargo test -p shannon-core --lib` 2606 passed(含全部 5 个 `config_migration::tests`)。
