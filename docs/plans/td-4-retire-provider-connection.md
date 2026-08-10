# TD-4 · Retire `ProviderConnection` wire divergence

> **Track**: Tech debt([tech-debt.md](../tech-debt.md) TD-4)·ADR-0009 Phase 2
> **Date**: 2026-08-10
> **Status**: 🔄 In progress(branch `feat/td-4-retire-provider-connection`,off `dev` `6704604d`)
> **Estimate**: 1–1.5d · **Priority**: 🟡 中
> **Dependencies**: ADR-0009 Phase 1 ✅(read/write converged to engine `ProviderConfigStore`);W6-3 inventory ✅
> **Parent**: [w6-3-p2-2-deprecation-tail.md](./w6-3-p2-2-deprecation-tail.md) §2.1 · **Input**: [w6-3-provider-connection-inventory.md](./w6-3-provider-connection-inventory.md)

---

## 1. Context

`ProviderConnection`(desktop wire DTO)was created as a transitional type during ADR-0005/0008/0009.
Phase 1 converged all reads/writes to the engine `ProviderConfigStore`(`ProviderProfile`),
leaving `ProviderConnection` as a **pure wire projection** via `from_provider_profile` →
`to_providers_file`. The projection has drifted from `ProviderProfile` and carries dead fields.
TD-4 retires the drift.

### 1.1 Key finding — `api_key` is already dead on the wire

`from_provider_profile` sets `api_key: None`, and the field is `#[serde(skip_serializing)]`.
On the live `list_providers` path `api_key` is **always `None` and never serialized**
(test `list_providers_does_not_serialize_api_key`,commands_config.rs). Consequences:

- Frontend `hasKey = !!conn.api_key`(ModelsSettings.tsx:535)is **always false** — the "key set"
  indicator dot is a pre-existing dead/broken signal since Phase 1.
- `testProviderConnection` flow already prompts for the key every time(`!conn.api_key` always true).

→ Retiring `api_key` is **not** a wire-shape break(consumers never saw it). TD-4 replaces it
with a real presence signal `has_api_key: bool`,derived from the credential store. The
`has_api_key: bool` pattern already exists in the crate(commands_config.rs masking context).

### 1.2 Key finding — full retire(expose `ProviderProfile` directly)is infeasible

The inventory(§4 step 6)recommended deleting `ProviderConnection` and having
`ProviderReadSnapshot`/commands expose `ProviderProfile` directly. Investigation disproves this:

- `ProviderProfile` has `#[serde(deny_unknown_fields)]`(shannon-types/provider_config.rs:134)→
  the wire DTO cannot add derived fields like `has_api_key`.
- `ProviderProfile.credential: CredentialRef` is a tagged enum(`Env{var}`/`Store{service}`/
  `Keyring{..}`/`InlineLegacy{masked}`/`Ephemeral`)→ exposing it leaks backend detail the UI
  never consumes,and still doesn't yield a clean "key present?" boolean without resolution.
- The engine/wire boundary is correct as-is: `ProviderProfile` = engine/protocol type;
  `ProviderConnection` = Tauri wire DTO(thin,frontend-safe,+ derived `has_api_key`).

→ **TD-4 = align the DTO to faithfully mirror `ProviderProfile` + add `has_api_key`,not
delete the struct.** This is the corrected interpretation of "retire the divergence."

## 2. Scope

### 2.1 Field mapping

| `ProviderConnection`(current) | `ProviderProfile` | TD-4 action |
|---|---|---|
| `id` | `id` | keep |
| `label` | `display_name` | **rename** → `display_name` |
| `provider_kind`(slug `String`) | `kind`(`ProviderKind` enum) | **rename** → `kind`;use enum directly(serde kebab-case already matches existing slugs) |
| `api_key`(`Option`,`skip_serializing`,always `None`) | —(`CredentialRef`) | **remove**;add `has_api_key: bool` derived from credential store |
| `base_url` | `base_url` | keep |
| `model`(`Option`) | —(active model in `ActiveTarget`) | **remove**(dead) |
| `created_at`(hardcoded epoch) | — | **remove**(dead) |
| `models_url` / `extra_headers` / `default_max_tokens` / `fallback_models` / `quirks` / `tiers` | same | keep |

Net:struct becomes a faithful `ProviderProfile` mirror + one derived `has_api_key`,
minus backend-only `credential`.

### 2.2 Legacy read path(separate old-shape structs)

`load_providers`(config.rs) + `migrate_providers_to_toml` read the **historical**
`~/.shannon/desktop/providers.json`(old shape:`label`/`provider_kind`/`api_key`/`model`/
`created_at`). This file is gone after the one-shot migration. To evolve the wire DTO
without breaking historical-file reads,introduce:

- `LegacyProviderConnection` / `LegacyProvidersFile`(old shape,`Serialize`/`Deserialize`)—
  used **only** by `load_providers` + `migrate_providers_to_toml` for deserializing historical
  files. `build_migrated_provider_model` consumes these.

The wire DTO `ProviderConnection`/`ProvidersFile` becomes new-shape;legacy structs are
migration-only.

### 2.3 Out of scope

- Renaming the struct itself(stays `ProviderConnection` — correct engine/wire boundary,§1.2).
- `test_provider_connection` command signature(unchanged;takes `provider`/`api_key`/`base_url`
  directly). Only the **frontend guard** changes(`!conn.api_key` → `!conn.has_api_key`).
- providers.json on-disk format(legacy;untouched beyond the existing one-shot migration).

## 3. Execution order(compile-checked layers,single PR)

1. **Plan doc**(this file).
2. **Rust legacy separation**:introduce `LegacyProviderConnection`/`LegacyProvidersFile`;
   refactor `load_providers`/`migrate_providers_to_toml`/`build_migrated_provider_model` to use
   them. No wire change yet → compiles + tests green.
3. **Rust wire evolution**:`ProviderConnection` → new shape(rename + remove dead + add
   `has_api_key`);update `from_provider_profile`(signature gains `has_api_key`),
   `to_providers_file`(passes credential-store presence through),all command constructors;
   **delete `mask_providers`**(nothing left to mask;`has_api_key` is already safe).
4. **Rust verify**:`cargo clippy -p shannon-desktop -- -D warnings` + `cargo test -p shannon-desktop`.
5. **TS**:`types/index.ts` → `tauri-api.ts` → `ModelsSettings.tsx` + `AddProviderModal.tsx` →
   `Welcome.test.tsx` + `setup.ts` fixtures. `hasKey = conn.has_api_key`;
   `testProviderConnection` guard → `!conn.has_api_key`.
6. **TS verify**:`pnpm lint` + `pnpm test` + `tsc`(desktop/ui).
7. **PR** → `dev`,squash. **No version bump**(desktop is semver-excluded).

## 4. Risks

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| Historical providers.json fails to migrate after old-shape separation | 低 | 中 | `LegacyProviderConnection` preserves old serde shape;covered by existing migration test |
| `test_provider_connection` prompt-for-key flow breaks | 低 | 中 | guard semantics identical(`!has_api_key` ≡ old `!api_key`);manual verify |
| TS field rename miss leaves stale `conn.label`/`conn.provider_kind` | 中 | 低 | `tsc` catches;grep sweep after edit |
| semver break | 无 | — | desktop excluded from cargo-semver-checks;internal wire type |

## 5. 验收

- [ ] `ProviderConnection` mirrors `ProviderProfile`(+ `has_api_key`,− `credential`);
      no `api_key`/`model`/`created_at`/`label`/`provider_kind` fields.
- [ ] `list_providers` wire JSON contains `has_api_key`,no `api_key`/`model`/`created_at`.
- [ ] `mask_providers` deleted;no compile warnings.
- [ ] Historical providers.json still migrates(`LegacyProviderConnection` path).
- [ ] `cargo clippy -p shannon-desktop -- -D warnings` + `cargo test -p shannon-desktop` green.
- [ ] `pnpm lint` + `pnpm test`(desktop/ui)green.
- [ ] Frontend "key set" indicator reflects real credential-store presence.

## 6. 参考

- [ADR-0009](../adr/0009-provider-store-read-facade.md)
- [w6-3-provider-connection-inventory.md](./w6-3-provider-connection-inventory.md)(consumed;§4 step 6 revised per §1.2 above)
- [tech-debt.md](../tech-debt.md) TD-4
