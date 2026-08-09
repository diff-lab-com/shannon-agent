# W6-3 · ProviderConnection 消费方 Inventory

> **Date**: 2026-08-08
> **Branch**: `feat/w6-3-deprecation-tail`
> **Purpose**: ADR-0009 Phase 2(retire `ProviderConnection` wire type = tech-debt TD-4)的迁移输入。逐个列出消费方,供机械替换时消除。
> **Parent**: [w6-3-p2-2-deprecation-tail.md](./w6-3-p2-2-deprecation-tail.md)

---

## 1. 定义

| 项 | 位置 |
|---|---|
| `struct ProviderConnection` | `desktop/src/config.rs:186` |
| `ProviderProfile → ProviderConnection` 转换 | `desktop/src/config.rs:347` `from_provider_profile`(`pub(crate)`,Phase 1 后的**唯一**转换点) |
| `→ ProvidersFile` wire 投影 | `desktop/src/provider_read_snapshot.rs:95` `to_providers_file`(ADR-0009 Decision 3 单一 chokepoint) |

`ProviderConnection` 是 **desktop crate 内部** wire type——不出现在任何 `shannon-*` library crate 的公开 API,因此对 `cargo-semver-checks` **无影响**(desktop 也被 semver job exclude)。

## 2. 消费方清单(grep,2026-08-08,dev tip `c90adf82`)

### Rust(`desktop/src/`)
| 文件 | 引用数 | 角色 |
|---|---:|---|
| `config.rs` | 26 | 定义 + `from_provider_profile` + `migrate_providers_to_credentials` + serde |
| `commands_config.rs` | 19 | Tauri 命令(`save_provider` / `delete_provider` / `set_active_provider` / `list_providers`)入参/出参 |
| `provider_read_snapshot.rs` | 4 | `to_providers_file` wire 投影(ADR-0009 chokepoint) |

### TypeScript(`desktop/ui/src/`)
| 文件 | 引用数 | 角色 |
|---|---:|---|
| `components/settings/ModelsSettings.tsx` | 6 | provider 列表 + api_key masking + `testProviderConnection`(**最重**,~25 处机械替换) |
| `lib/tauri-api.ts` | 4 | Tauri 命令的 typed wrapper 返回类型 |
| `types/index.ts` | 3 | TS 类型定义(`ProviderConnection` 镜像) |
| `components/settings/AddProviderModal.tsx` | 3 | 新增 provider 表单(~15 处) |
| `__tests__/Welcome.test.tsx` | 1 | 测试 fixture |
| `__tests__/setup.ts` | 1 | mock 默认值 |

**合计**:Rust ~49 处,TS ~18 处。

## 3. 字段映射(Phase 2 替换时要 reconcile 的差异)

| `ProviderConnection` 字段 | `ProviderProfile` 对应 | 迁移动作 |
|---|---|---|
| `label` | `display_name` | rename |
| `provider_kind`(slug 字符串) | `kind`(`ProviderKind` enum) | slug ↔ enum(已有 `kind_engine_to_slug` / `kind_slug_to_engine`) |
| `api_key`(`skip_serializing`,transitional) | —(在 `CredentialRef::Store`) | **删除**;`testProviderConnection` 改从 credential store 取 |
| `model` | —(活动模型在 `active_target`) | **删除** |
| `created_at`(hardcoded epoch) | — | **删除** |
| `id` / `base_url` / `models_url` / `extra_headers` / `default_max_tokens` / `fallback_models` / `quirks` / `tiers` | 同名直映 | 直接替换 |

## 4. 迁移顺序建议(TD-4 执行时)

1. `types/index.ts` TS 类型换 `ProviderProfile` 形状(label→display_name 等)。
2. `tauri-api.ts` wrapper 返回类型跟进。
3. `ModelsSettings.tsx`(~25)+ `AddProviderModal.tsx`(~15)字段级替换;`testProviderConnection` 改从 credential store 取 key(唯一行为风险点)。
4. `Welcome.tsx`(~5,薄消费者)+ 2 个 test fixture。
5. `commands_config.rs` Tauri 命令出参改 `ProviderProfile`;删 `from_provider_profile`。
6. 删 `ProviderConnection` struct + `to_providers_file` 的 wire 投影;`ProviderReadSnapshot` 直接暴露 `ProviderProfile`。

## 5. 为什么 W6-3 不加 `#[deprecated]` attribute

STABILITY.md 的 deprecation cycle 要求 "desktop shell must compile without warnings against the intermediate release"。`ProviderConnection` 仍被 49+18 处使用——现在加 `#[deprecated]` 会立刻产生几十个编译警告,淹没真正有用的警告,并违反上述规则。

`#[deprecated]` 适用于仍在 **stable surface**、有外部消费者的 API。`ProviderConnection` 是 desktop 内部 wire type(无外部消费者),retire 方式是**直接机械替换 + 删除**(TD-4),不经过 deprecation 周期。因此 W6-3 的代码层标注无落脚点——本 inventory + [w6-3-parity-diff.md](./w6-3-parity-diff.md) + STABILITY §Planned deprecations + `ProviderConnection` doc 注释(指向 TD-4)是当前阶段合适的全部信号。

---

*证据:`grep -rc ProviderConnection desktop/`(2026-08-08)。*
