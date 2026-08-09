# W6-3 · P2-2 STABILITY Deprecation 收尾

> **Track**: Wave 6(功能收尾)
> **Date**: 2026-08-08
> **Status**: ✅ Done(2026-08-09,PR #51)
> **Estimate**: 3–5d · **Priority**: 🟡 中(低风险,推荐 Wave 6 最先做)
> **Dependencies**: 无(读写路径 + C3 parity 已闭环,见 PR #34 / #41 / #42 / #46 / #49)
> **Parent**: [wave-5-followup.md](./wave-5-followup.md) §2 W6-3

---

## 1. Context

P2-2 Wave 6 已完成 provider 读写路径单一化(ADR-0008 D3)+ 读 facade(ADR-0009)+ CLI↔desktop parity 全 6 行。剩余的是"软收尾":给 legacy 路径打 deprecation 标注,补齐 CLI/桌面行为差异表,为 [TD-4](../tech-debt.md)(retire `ProviderConnection` wire type)铺路。

本 task **不改运行时行为**,只补元数据 + 文档,风险最低,推荐 Wave 6 最先做。

## 2. Scope

### 2.1 审计 `ProviderConnection` wire type 残留

- **文件**:`desktop/src/config.rs`(`from_provider_profile` builder)、`desktop/src/provider_read_snapshot.rs`(`to_providers_file()` chokepoint)、`desktop/ui/src/types/index.ts`。
- **动作**:grep 所有 `ProviderConnection` 消费方,产出 inventory 清单(TD-4 机械替换的输入)。

### 2.2 Legacy 路径 deprecation 标注

- **文件**:`crates/shannon-core/src/provider.rs`、`desktop/src/commands_config.rs`。
- **动作**:对 Phase 2 将删除的 API 加 `#[deprecated(note = "ADR-0009 Phase 2: use ProviderProfile directly; see TD-4")]`;验证 `cargo-semver-checks` 接受 minor-bump 路径(`0.8.x → 0.9.0`)。
- **实际执行(2026-08-09,PR #51)**:改为只在 `desktop/src/config.rs` 的 `ProviderConnection` 加 doc-comment 标注退役;**未加 `#[deprecated]` attribute**(会在该类型仍广泛内部使用时 flood ~67 warnings,违反 compile-without-warnings)。真正的退役走 [TD-4](../tech-debt.md) 直接删除,不走 deprecation cycle。

### 2.3 CLI / 桌面 provider·credential 行为差异表

- **动作**:对照 C3 parity matrix 6 行,逐项记录 CLI(`shannon providers add/remove/use`、REPL `/connect`)与桌面(`save_provider` / `delete_provider` / `set_active_provider`)的行为;全对齐则归档矩阵,否则登记差异为后续 task。

### 2.4 STABILITY.md 同步

- **文件**:[STABILITY.md](../STABILITY.md)。
- **动作**:deprecated 清单与代码标注一致;更新 Phase 2 预期 timeline。

## 3. 实施步骤

1. `grep -rn "ProviderConnection" desktop/ crates/` → 产出消费方 inventory。
2. 在 `provider.rs` 给 legacy API 加 `#[deprecated]`(+ `#[doc(hidden)]` 若适用)。
3. `cargo-semver-checks --baseline v0.8.0` 确认 deprecation 不破坏 semver(minor bump 即可)。
4. 写差异表 → `docs/plans/w6-3-parity-diff.md`(或确认无差异后在 wave-5-followup 归档)。
5. 更新 STABILITY.md。

## 4. 验收

- [ ] `ProviderConnection` 消费方 inventory 完整(grep 零遗漏)。
- [ ] legacy API 有 `#[deprecated]` 标注,`cargo-semver-checks` pass。
- [ ] 差异表产出或确认无差异。
- [ ] STABILITY.md deprecated 清单与代码一致。

## 5. 风险

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| deprecation 触发 semver lint 失败 | 低 | 低 | minor bump 即可;pre-1.0 允许 |
| 漏标 legacy 路径 | 中 | 低 | grep 全覆盖 + C3 matrix 交叉验证 |

## 6. 参考

- [tech-debt.md](../tech-debt.md) TD-4(Phase 2 retire `ProviderConnection`)
- [ADR-0009](../adr/0009-provider-store-read-facade.md)
- [spikes/p2-2-c3-cli-desktop-parity-matrix.md](../spikes/p2-2-c3-cli-desktop-parity-matrix.md)
