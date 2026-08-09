# W6-3 · CLI ↔ Desktop Provider 行为差异表

> **Date**: 2026-08-08
> **Branch**: `feat/w6-3-deprecation-tail`
> **Purpose**: P2-2 ADR-0005 Phase 2 的"CLI 与桌面 provider/credential 行为逐项对齐"验收。
> **Parent**: [w6-3-p2-2-deprecation-tail.md](./w6-3-p2-2-deprecation-tail.md) · **Matrix**: [spikes/p2-2-c3-cli-desktop-parity-matrix.md](../spikes/p2-2-c3-cli-desktop-parity-matrix.md)

---

## 结论:6 行 parity 全对齐,无悬空差异

C3 parity matrix 的 6 行全部有测试覆盖且 pass。CLI 与桌面 surface 对同一逻辑操作产出**字节一致的磁盘状态 + 一致的读视图**。

## 逐行证据

| # | 操作 | CLI surface | Desktop surface | Parity 测试 | 状态 |
|---|---|---|---|---|---|
| 1 | add provider | `ProviderConfigService::connect`(`shannon providers add` + REPL `/connect` 共用) | `save_provider` Tauri cmd(LockedService R-M-W) | `c3_upsert_cli_surface_matches_desktop_locked_surface` | ✅ |
| 2 | remove provider | `ProviderConfigService::remove`(`shannon providers remove`) | `delete_provider` Tauri cmd | `c3_disconnect_cli_surface_matches_desktop_locked_surface` | ✅ |
| 3 | set active | `ProviderConfigService::set_active` | `set_active_provider`(`configure('provider')` arm) | `c3_set_active_cli_surface_matches_desktop_locked_surface` | ✅ |
| 4 | update field | REPL `/connect` re-arm | `configure('base_url')` / `configure('api_key')` arms | `c3_update_field_cli_surface_matches_desktop_locked_surface` | ✅ |
| 5 | list / read | `ProviderConfigStore::config()`(`shannon providers list` + REPL `/provider`) | `ProviderReadSnapshot::to_providers_file()` | `read_view_cli_vs_desktop_match`(desktop) | ✅ |
| 6 | concurrent mixed | CLI writer thread + desktop writer thread 交错 | (同) | `c3_mixed_surface_writers_do_not_lose_updates` | ✅ |

测试位置:`crates/shannon-core/tests/provider_cross_process_consistency.rs`(rows 1–4、6)+ `desktop/src/provider_read_snapshot.rs`(row 5)。

## 共享契约(两边共同遵守)

- **写入**:都经 `ProviderConfigService` 的 RAII `flock` + lock-then-reload R-M-W(ADR-0008 D3)。
- **读取**:桌面经 `ProviderReadSnapshot` 单一 chokepoint(ADR-0009),投影出的 `ProvidersFile` 与 CLI `ProviderConfigStore::config()` 的 wire shape 一致(row 5 测试断言)。
- **凭证**:都写 `~/.shannon/credentials/<id>.json`(0600);`ProviderConnection.api_key` 是 `skip_serializing` 的 transitional 字段,从不上盘。

## 已知非差异(设计如此,不是行为分歧)

- **桌面独有**的 `ProvidersFile` wire 投影(`to_providers_file`)是 UI 桥接需要,不是"另一条读路径"——读的是同一个引擎 store,只是形状转换。Phase 2(TD-4)retire `ProviderConnection` 后这层消失。
- **CLI 独有**的 `shannon providers` 子命令是 CLI 入口,桌面等价物是 Tauri 命令;两者落到同一 `ProviderConfigService`。

## 下一步

parity 已闭环,本表归档。剩余"软收尾"是 TD-4(retire `ProviderConnection` wire type),见 [w6-3-provider-connection-inventory.md](./w6-3-provider-connection-inventory.md)。

---

*证据:C3 测试函数(2026-08-08,dev tip `c90adf82`);matrix 设计见 [spikes/p2-2-c3-cli-desktop-parity-matrix.md](../spikes/p2-2-c3-cli-desktop-parity-matrix.md)。*
