# D1: Secret-Guard 默认翻转套件(audit → redact)

**Status**: 材料就绪,等待观察期裁决(本文档只备料,**本次不翻转默认**)
**Branch**: `feat/d1-kit-and-followups`(基于最新 dev,2026-10-07)
**Scope**: 唯一翻转 PR 的完整材料 —— 前置判据检查单、精确 diff 说明、Breaking note 草稿、文案改版清单、回归验证清单。
**依据**: ADR-0012(`docs/adr/0012-secret-guard-coverage-and-persistence.md`)、v0.11.0 默认翻转先例(CHANGELOG "Changed — defaults & claims")、`crates/shannon-core/src/secret_guard.rs`(`resolve_mode_with_default` / `init_from_env_or_config`)。

---

## 0. 唯一待用户输入

> **观察期是否满足判据(见 §1 检查单)。** 这是翻转 PR 落地前唯一需要用户裁决的事项;
> 其余全部材料(代码位点、文案、验证命令)已在本套件内备齐,观察期一结束即可小 PR 落地。

- 目标版本:**v0.13.0**(当前 workspace 版本 0.12.0,翻转落在下一个 minor)。
- 影响面:**仅 unset 用户**(env 与 config 均未表态者);显式 `audit` / `off` 行为不变。

---

## 1. 翻转前置判据检查单(逐条可勾选)

前提:用户本机 dogfood 以 `SHANNON_SECRET_GUARD=redact` 连续运行 **N = 14 天**
(推荐值;如用户裁决用别的窗口长度,只改这里,其余材料不受影响)。

### ① 零误报事故

- [ ] 观察期内**零**"误改写导致工具调用失败"的可见 case。
- 误报的定义(全部要算):
  - 工具调用解析失败归因于改写(按 ADR-0012 D1,tool 名与 `input_schema` 永不改写;
    若出现即视为 contract 违约,直接阻断翻转);
  - 执行面 unresolved 警告(D4):工具输出中出现 `[secret-guard] warning: ...`;
  - 显示面 unresolved 警告(D4):输出中出现 `[secret-guard: unresolved placeholder SG1:…]`;
  - 用户主动报告的可归因于 redact 的异常行为(GitHub issue / 会话记录)。
- 取证位置:`shannon::secret_guard` target 的 tracing 日志、L0 会话日志(写时已脱敏,
  可安全翻查)、issue 列表。

### ② 命中样例人工核验(ADR-0012 persistence 记录统计)

- [ ] 按周抽样命中样例并人工核验:每次抽样的命中里,secret 形态判定正确的比例 ≥ 95%
  (与 v0.11.0 audit 观察期基线一致即可);误判样例逐条记录 rule id。
- [ ] 命中/回滚数统计齐全(来自 ADR-0012 的记录面):
  - 命中数:`audit_hits()` 进程内计数(`shannon-core/src/secret_guard.rs`)+ 每次命中的
    `tracing::warn!(target: "shannon::secret_guard", rule = ...)` 日志(rule id only,无 secret 值);
  - 回滚数:restore 成功对数(RestoreStats)与 D4 的 unresolved/不可还原标记数之差;
    删除/损坏 `secret_guard.key` 导致的整批失效事件单列(每次都会使 prompt cache miss 一轮)。

### ③ byte-stable / prompt cache 无明显下降

- [ ] 机制依据(ADR-0012 D1):redact 走确定性 HMAC 代理符(SG1,不变量 I1),稳定前缀
  (system blocks + tool defs)逐字节稳定,prompt cache 理论上不受影响。
- [ ] 实测:`just cache-stats` 按周聚合,redact 观察期的缓存命中率相对 audit 基线
  **持续一周窗口内下降 ≤ 2 个百分点**(无明显下降;一次性 miss 仅允许来自
  `secret_guard.key` 再生成事件,见 ②)。
- [ ] 预算测试保持绿:`full_history_retransform_is_byte_stable_and_within_budget`
  (`secret_guard.rs` 测试模块)。

### ④ 无性能回归

- [ ] 已知开销(ADR-0012 Consequences)不放大:每次 session restore 多一次历史重建扫描、
  每条流式 delta 一个 hold-back 缓冲;两者均在既有软预算内。
- [ ] `just bench` / `just perf` 与观察期起点相比无超阈值回退;turn 延迟无用户可感劣化。

### ⚠ **已知问题(2026-10-07 更新,X4 实证)**:dev 上 `secret_guard_query_loop_completes_with_redacted_wire` 一度为红——根因**不是产品回归**,而是测试封闭性漏洞:本机 D1 dogfood 的 `~/.shannon/config.toml [secret_guard] mode="redact"` 会经 `init_from_env_or_config` 安装内置 guard(显式 config 优先为设计行为),顶掉测试自装的自定义 transform。已在 `agent_loop_tests.rs` 以文档化的覆盖机制 `SHANNON_SECRET_GUARD=off` save/set/restore 钉住,两测试转绿,产品语义零改动。生产 redact(HMAC 代理符字节稳定)全程完好——本翻转前提成立。

dev HEAD(a0a4bf7b7)上存在一个**预先存在的红测试**,与本次套件无关但直接命中判据①/红act 安装路径,
翻转 PR 前必须修复或归因:

```
query_engine::engine::tests::agent_loop_tests::secret_guard_query_loop_completes_with_redacted_wire
panic: "wire must carry surrogates" —— 自定义 transform 经 set_context_transform 安装后,
query loop 发出的 wire 上仍是原始 secret(代理符未出现)。
隔离运行 / nextest 均复现;env 无 SHANNON_SECRET_GUARD 干扰;clean HEAD 复现。
嫌疑:近期 loop 级 LLM 路径改动(283b63843 "reuse ambient runtime for loop-level LLM compaction")。
```

另:`unified_config::tests::build_client_from_resolved_*` 两条在 dev 上同样红(provider 定价/tier
断言,与 secret-guard 无关)——跑 §5 清单时勿归因于翻转。

---

## 2. 翻转 PR 的精确 diff 说明

翻转 PR 是一个小 diff,只改"unset 时的默认值"这一处语义;**优先级链(D2)与显式取值路径零改动**。

### 2.1 `crates/shannon-core/src/secret_guard.rs`(语义改动,两处)

**(a) 纯函数默认臂** —— `resolve_mode_with_default`(约 L788-804)最后一臂:

```rust
// before
match resolve_mode(env_raw, section_mode) {
    Some(mode) => Some(mode),
    None if explicit_off => None,
    None => Some(SecretGuardMode::Audit),   // ← 改这里
}
// after
    None => Some(SecretGuardMode::Redact),
```

同步改该函数 doc(约 L783-787):"release default: ... `audit` (detect and log ...)" →
`redact`(描述改为:unset 时出站内容被改写为确定性代理符,工具执行/显示时还原)。

**(b) 一次性安装的真空臂** —— `init_from_env_or_config`(约 L856-872)末尾:

```rust
// before
    // Implicit default: only fill the vacuum.
    if context_transform().is_some() {
        return None;
    }
    install_mode(SecretGuardMode::Audit)    // ← 改这里
// after
    install_mode(SecretGuardMode::Redact)
```

同步改 doc(约 L849-855):"Since v0.11.0 the unset default is `audit`" →
"Since v0.13.0 the unset default is `redact`"(保留 vacuum-only 语义描述:已有外部 transform
时不覆盖)。

**(c) 注释面(零行为,随手改)**:
- L73-79 T5 注释块:"The unset default mode is `audit`..." → redact 措辞;
- L1583-1589 测试区 T5 注释:"Audit mode is the unset default..." → "Audit mode is the
  explicit opt-back..."。

### 2.2 `crates/shannon-core/src/unified_config.rs`(doc + 钉住测试,两处)

**(a)** `SecretGuardSection::mode` 字段 doc(约 L92):

```rust
// before
    /// `"audit"` (default since v0.11.0 when unset) | `"redact"` | `"off"`
// after
    /// `"redact"` (default since v0.13.0 when unset) | `"audit"` (explicit
    /// opt-back) | `"off"` to disable entirely.
```

**(b)** 钉住测试 `secret_guard_unset_mode_is_none_and_release_default_stays_audit`
(本套件已加入,见 §2.4):把 `_pinned_release_default` 的类型标注从
`SecretGuardMode::Audit` 改为 `SecretGuardMode::Redact`;`_post_flip_rollback_value`
保持 `Audit`;doc 注释同步。

### 2.3 `crates/shannon-core/src/secret_guard.rs` 测试(随语义自觉翻转)

`resolve_mode_with_default_unset_is_audit_and_off_still_opts_out`(约 L1556):

```rust
// before(首条断言)
    assert_eq!(resolve_mode_with_default(None, None), Some(SecretGuardMode::Audit));
// after
    assert_eq!(resolve_mode_with_default(None, None), Some(SecretGuardMode::Redact));
```

测试名改为 `resolve_mode_with_default_unset_is_redact_and_off_still_opts_out`;
其余断言(显式 `redact` 透传、`off`/垃圾值 opt-out、env > config)全部不动。

### 2.4 钉住机制(为什么翻转 PR"会自觉地改它")

- 行为级钉:`resolve_mode_with_default_unset_is_*`(secret_guard.rs)——不翻转语义它必红;
- 配置面钉:`secret_guard_unset_mode_is_none_and_release_default_stays_audit`
  (unified_config.rs,本套件新增)——类型标注引用 `SecretGuardMode::Audit` 并在 doc 里
  指向翻转套件;不一起改它,`cargo test -p shannon-core unified_config` 必红;
- 两处钉测试的 doc 注释都写了本文件路径,红的第一个动作就是打开本文档。

### 2.5 不变式(评审时逐条对照)

- 显式 `audit` / 显式 `off`(env 或 config)行为**完全不变**(`resolve_mode` D2 链路不动);
- env 优先级不变:env PRESENT 即定案(含"垃圾值 = 显式 off");
- vacuum-only 语义不变:已有宿主/插件安装的 transform 不被内置 guard 覆盖;
- 仅影响 unset 用户;`audit` 成为显式 opt-back 值。

### 2.6 回退方式(合入后应急)

| 手段 | 操作 |
|---|---|
| 环境变量(单机,立竿见影) | `export SHANNON_SECRET_GUARD=audit` |
| 全局配置 | `~/.shannon/config.toml` 写 `[secret_guard]\nmode = "audit"` |
| 项目级覆盖 | 项目根 `.shannon.toml` 同上(覆盖全局) |
| 彻底关闭 | `mode = "off"`(config)或 env 置任意非 `audit`/`redact` 值 |

> ⚠ 回退指引必须写**字面量 `audit`**:按 D2,env 一旦 PRESENT 就定案——
> `SHANNON_SECRET_GUARD=""`(空串)是"显式 off",不是"回落 config"。

---

## 3. Breaking note 草稿(可直接贴 CHANGELOG "Changed — defaults & claims")

```markdown
- **`secret-guard` default flipped to `redact` when unset (since v0.13.0).** Users who set
  neither `$SHANNON_SECRET_GUARD` nor `[secret_guard] mode` now get outbound redaction:
  secret-shaped content in messages, system blocks, and tool descriptions is rewritten to
  deterministic `SG1:` surrogates before it reaches the model, and restored automatically
  for local tool execution and display (previously, since v0.11.0, the unset default was
  `audit` — detect and log only, forward verbatim). Rewrites are byte-stable, so provider
  prompt caches keep hitting; restarts re-derive identical surrogates from history, so
  persisted sessions restore unchanged. Explicit choices are untouched — `audit` and `off`
  keep their exact meaning, and env still beats config. To opt back:
  `[secret_guard] mode = "audit"` in `~/.shannon/config.toml` (or project `.shannon.toml`),
  or `SHANNON_SECRET_GUARD=audit`. Note the env var decides whenever it is present — an
  unparseable value (including `SHANNON_SECRET_GUARD=""`) is an explicit *off*, not a
  fallback to config.
```

---

## 4. T5 文案改版 draft

### 4.1 指定范围的 grep 结果(2026-10-07,本 worktree)

```
$ grep -rn "audit" docs/configuration.md \
    desktop/ui/src/i18n/locales/zh-CN.json \
    desktop/ui/src/i18n/locales/en.json \
    | grep -i "secret\|默认\|default"
(无输出)
```

**结论:三个指定文件均无需翻转表述** ——
- `desktop/ui/src/i18n/locales/{zh-CN,en}.json` 没有任何 secret-guard/audit 相关 key
  (secret-guard 至今无桌面 UI 设置面,全部文案在 Rust 侧);
- `docs/configuration.md` 仅在 L25 提到 `[secret_guard]` 有独立 loader,未陈述默认值。

### 4.2 实际携带"audit 为默认"表述、需要翻转的位置(前后文案)

**A. `README.md` L39(特性列表)**

- 前:`**Outbound secret scanning** — the built-in `secret-guard` (audit mode by default, `redact` opt-in; ...)`
- 后:`**Outbound secret scanning** — the built-in `secret-guard` (`redact` by default since v0.13.0; `audit` to opt back to log-only; ...)`

**B. `README.zh-CN.md` L39**

- 前:`**出站 secret 扫描** —— 内置 `secret-guard`(默认 audit 只记录,`redact` 一键开启;...)`
- 后:`**出站 secret 扫描** —— 内置 `secret-guard`(v0.13.0 起默认 `redact` 改写并自动还原;`audit` 可退回只记录;...)`

**C. `crates/shannon-cli/src/main.rs` `redaction_suggestion_notice()`(约 L1929-1934;单测在 L9007-9019)**

翻转后该提示只会命中**显式 audit** 用户(latch 仅在 Audit 模式武装),文案改为向其说明 audit 已非默认:

- 前:
  ```text
  Notice: potential secrets were detected in outbound LLM requests while secret-guard \
  is in audit-only mode (values were forwarded to the provider and written to the \
  session log). Enable redaction with [secret_guard] mode = "redact" in .shannon.toml \
  (or ~/.shannon/config.toml), or SHANNON_SECRET_GUARD=redact
  ```
- 后:
  ```text
  Notice: potential secrets were detected in outbound LLM requests while secret-guard \
  is in audit-only mode (explicitly opted back; the default since v0.13.0 is redact). \
  Values were forwarded to the provider and written to the session log. Switch to \
  [secret_guard] mode = "redact" in .shannon.toml (or ~/.shannon/config.toml), or \
  SHANNON_SECRET_GUARD=redact
  ```
- 同步改单测 `test_redaction_suggestion_notice_is_single_line_naming_optin`
  (仍断言单行、`Notice:` 前缀、含 `[secret_guard] mode = "redact"` 与
  `SHANNON_SECRET_GUARD=redact`;如断言 `audit-only` 措辞则随新文案调整)。

**D. `crates/shannon-ui/src/repl/query.rs`(约 L1716-1731,REPL 一次性聊天提示)**

- 前:`... (audit-only mode: values were forwarded to the provider and written to the session log). Enable redaction with `[secret_guard] mode = "redact"` ...`
- 后:`... (audit-only mode — explicitly opted back; the default since v0.13.0 is redact: values were forwarded to the provider and written to the session log). Switch to redaction with `[secret_guard] mode = "redact"` ...`
- 上方注释块("The built-in secret-guard runs audit-only by default")同步改写。

**E. `crates/shannon-core/src/secret_guard.rs` `take_redaction_suggestion` 的 `tracing::warn!`(约 L124-131,headless 面)**

- 前:`...while secret-guard is in audit-only mode: values were forwarded...Enable redaction with...`
- 后:`...while secret-guard is in audit-only mode (explicit opt-back; default since v0.13.0 is redact): values were forwarded...Switch to redaction with...`

**F. `docs/configuration.md` L25(可选,建议顺手)**

- 前:`...(`[secret_guard]` has its own loader; full TOML tables are otherwise not read from this file.)`
- 后:追加一句 ``Default `mode` when unset: `redact` (since v0.13.0); `audit` = log-only opt-back, `off` = disabled.``

> C/D/E 是同一条提示的三个宿主面(CLI stderr / REPL 聊天 / tracing),措辞务必同步。

---

## 5. 翻转时的回归验证清单

```bash
# 1) 格式
cargo fmt --all -- --check

# 2) Lint(CI clippy job 同参;或 just lint)
cargo clippy --workspace --all-targets -- -D warnings

# 3) 钉住测试(翻转 PR 的自觉性检查)
cargo test -p shannon-core unified_config        # ⚠ dev 上 build_client_from_resolved_* 两条预存红,与翻转无关(§1⚠)
cargo test -p shannon-core secret_guard          # resolve_mode_with_default_unset_is_redact_* 必须绿
                                                 # ⚠ secret_guard_query_loop_completes_with_redacted_wire 为 dev 预存红,
                                                 #   翻转前必须修复或归因(§1⚠)

# 4) 隔离进程全量(CI 形态,规避共享进程 latch 干扰)
cargo nextest run --workspace --config-file .config/nextest.toml

# 5) e2e 冒烟
just dogfood-selftest                            # dogfood 自检
just replay                                      # 离线结构校验(无需 API key)
just record && just replay-agent                 # 真实链路(需 SHANNON_API_KEY;可选)

# 6) 人工冒烟(翻转 PR 描述里贴结果)
#   - env/config 全 unset 启动 → secret-guard 以 redact 安装;
#   - 提示词里放一个真形态 key → wire 上是 SG1: 代理符,工具执行/显示还原为原值;
#   - SHANNON_SECRET_GUARD=audit → 行为回到只记录;=off → 完全关闭;
#   - just cache-stats → 缓存命中率与翻转前基线相当。
```

---

## 6. 落地顺序(观察期结束后)

1. 用户裁决 §1 检查单(唯一待输入)。
2. 修复/归因 §1⚠ 的预存红测试(redact 路径 e2e 必须先绿)。
3. 开翻转 PR:按 §2 逐位点改(预计 < 60 行 diff,含测试与注释),贴 §3 Breaking note、
   按 §4 改文案,§5 全绿后合入,随 v0.13.0 发布。
