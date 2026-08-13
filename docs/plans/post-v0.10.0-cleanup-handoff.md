# Post-v0.10.0 清理与后续任务 Hand-off

> **日期**: 2026-08-13
> **分支族**: `chore/ci-semver-baseline-and-pnpm-v6` (#65)、`chore/deprecate-secrets-env-store` (#66)
> **背景**: v0.10.0 已发版(tag `v0.10.0` → `0c57ef92`,21 assets)。本 hand-off 记录发版后的清理工作成果、一项被否决的任务及其根因分析,以及交接到下一波(Wave 7)的剩余任务。

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

### 待合并 PR

- **#65** `chore(ci): bump semver baseline + pnpm/action-setup v6`
- **#66** `chore(core): deprecate secrets.env write-path`

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

### 3.1 本轮衍生(小,确定)

- **[#5 跟进] 删除已废弃的 secrets.env 项**:过一个发版宽限期(≥ 一个 minor)后,可整体删除 `persist_secrets` / `default_secrets_path` / `SecretBinding` + `lib.rs` 的 `pub use`。**删 public API = semver-breaking**,0.x 需要一次 minor bump(见 `[[semver-check-baseline-version-bump]]`)。删前确认无外部消费者(当前 grep 仅命中文档注释)。
- **[#2 跟进] 下次发版后再 bump baseline**:每次切新 baseline release 后把 `SEMVER_BASELINE` 往前推到最新 tag(现在 v0.10.0;v0.11.0 发版后推到 v0.11.0)。这是例行维护,不是一次性。

### 3.2 既有未决(中大)

- **[W7-1 / P3-7] 沙盒执行后端**:`docs/spikes/p3-7-sandbox-s0.md` 状态 **Draft**(待 ericdong 评审),4 阶段路线(~7w)。spike 已标注一个具体 bug:`crates/shannon-core/src/sandbox.rs:38` 的 `Bitflags` 应为 `BitFlags`(`use landlock::{…, Bitflags, …}`),在 `--features landlock` 下触发 E0432。**这是低成本可立刻修的**,但建议随沙盒方向定稿一起做(评审可能重构该模块)。
- **[W7-2 / P3-1] per-model 定价 + models.dev 动态刷新**:ADR-0005 的 parity 评估(行 337–339)指出 P0-1 / P0-2 / P1-6 的桌面表面已通过 `list_models` + `ModelInfo` wire type 基本落地;剩余 gap 需在评审 ADR-0005 尾部后明确具体子项再排期。**不要假设这整块没做** —— 先核对 ADR 当前措辞。
- **[OAuth provider 接入]** `crates/shannon-core/src/oauth.rs` 已存在(`OAuthClient` / `OAuthService` / `OAuthToken` / `TokenEncryption`),但 OAuth 流尚未接入 provider 添加 UX(Add Provider modal 目前只支持 API key / base URL)。需要设计:哪些 provider 走 OAuth(Google Gemini / Azure / Bedrock 是典型候选)、desktop 端的回调/深链接如何收口。

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
