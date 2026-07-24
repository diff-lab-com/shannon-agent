# ADR 0005 — Unified Provider/Model/Credential Management

**Status**: Proposed
**Date**: 2026-07-24
**Theme**: 统一 Provider/Model/密钥管理 (shannon-code + shannon-desktop)
**Supersedes**: —
**Related**: ADR 0002 (Sprint 5 MCP deepening — `/connect` for MCP servers is separate scope)

## Context

Shannon ships two front-ends over one engine: **shannon-code** (CLI/REPL) and
**shannon-desktop** (Tauri + React). A 2026-07-24 investigation found that
provider / model / credential management is **not unified** — each front-end
keeps its own parallel system, and within shannon-code there are additional
disconnected credential stores. User-visible symptoms:

- `/model` is a placeholder; switching is inconsistent across the two products.
- `/credentials store` writes a key the engine **never reads** at request time.
- A provider configured in the Desktop is invisible to the CLI, and vice-versa.

### Current state — shannon-code (CLI/REPL)

| Aspect | Location | State |
| --- | --- | --- |
| Config files | `~/.shannon/config.toml`, `.shannon.toml` | TOML, merged by `ConfigBuilder` (`shannon-core/src/unified_config.rs`) |
| Model identifier | bare id (e.g. `claude-sonnet-4-20250514`) | **not** `provider/model` |
| Model resolution | `shannon-core/src/provider_resolver.rs:264` | `--model` → `SHANNON_MODEL` → `ANTHROPIC_MODEL` → `OPENAI_MODEL` → default |
| Request-time credential | `provider_resolver.rs:138-140` `resolve_credential()` | **only** `std::env::var()` is consulted |
| `CredentialRef::Keyring` | same | **stub — returns empty string** |
| `CredentialManager` | `shannon-core/src/credential_manager.rs:173` | persists plaintext JSON at `~/.shannon/credentials/<svc>.json` (0600); **disconnected from request path** |
| `secrets.env` | `shannon-core/src/config_migration.rs` | written on migration, **never loaded at runtime** (N2 TODO) — zombie store |
| `/model` command | `shannon-commands/src/builtin/repl.rs:63` | placeholder; no list/switch |
| `/config set` | `shannon-commands/src/builtin/config.rs` | in-memory only, **no persistence** |
| `/credentials` | `shannon-commands/src/builtin/credentials.rs` | works (store/get/delete + mask) but writes to the disconnected store |
| Hidden assets | `provider_config.rs` `ActiveTarget{provider_id,model_id,scope}`; `shannon-engine/src/api/client.rs` `LlmClient::set_model()` / `set_model_for_provider()` | data model + hot-swap primitive **already exist**, just not wired |

### Current state — shannon-desktop (Tauri)

| Aspect | Location | State |
| --- | --- | --- |
| Config file | `~/.shannon/desktop/config.json` (`DesktopConfig`) | JSON, **separate from engine config.toml**; holds plaintext `api_key` field |
| Managed providers | `~/.shannon/desktop/providers.json` (`ProvidersFile`/`ProviderConnection`) | Desktop's own roster; plaintext `api_key`; parallel to engine `ProviderProfile` |
| Provider switching | `desktop/src/commands_config.rs` `switch_provider` / `set_active_provider` | mirrors active provider into `DesktopConfig`, rebuilds `LlmClientConfig` in-memory — **bypasses engine config.toml entirely** |
| Connection test | `test_provider_connection` / `provider_probe_url` | already implemented (Success/InvalidKey/RateLimited/Network…) — the `/connect` validation UX, ready to reuse |
| Env detection | `detect_provider_from_env` | welcome-wizard provider sniff |
| Masking | `mask_providers` / `get_config` | read-back always `"***"` |
| OAuth loopback | `desktop/src/extensions/oauth.rs` + `extensions_commands.rs` | full OAuth 2.1 + PKCE — **but only for MCP servers**, tokens plaintext in `~/.shannon/settings.json` |
| OS keyring | `desktop/src/commands_connections.rs` | **only gateway social creds** (slack/wecom); does **not** cover LLM keys |

### Fragmented stores (the core problem)

There are **7** places provider/model/credential config can live. There is no
single source of truth.

| # | Store | Owner | Format | Plaintext? | Read at request time? |
| --- | --- | --- | --- | --- | --- |
| 1 | env vars (`SHANNON_API_KEY`, …) | code | env | yes | ✅ (code's only working source) |
| 2 | `~/.shannon/config.toml` | code | TOML | no (A1: `Env` refs only) | ✅ via `ConfigBuilder` |
| 3 | `~/.shannon/credentials/<svc>.json` | code | JSON | **yes** | ❌ disconnected |
| 4 | `~/.shannon/secrets.env` | code | shell | yes | ❌ zombie (N2 TODO) |
| 5 | `~/.shannon/desktop/config.json` | desktop | JSON | **yes** (`api_key`) | ✅ via in-memory injection |
| 6 | `~/.shannon/desktop/providers.json` | desktop | JSON | **yes** | ✅ mirrored into #5 |
| 7 | OS keyring | desktop | encrypted | no | ✅ gateway social only |

## Competitor benchmark

How the reference CLI coding agents actually store credentials (verified
2026-07-24, not from memory):

| Tool | Config (model/provider) | API key location | Plaintext? | Keyring? |
| --- | --- | --- | --- | --- |
| **Codex CLI** | `~/.codex/config.toml` | `~/.codex/auth.json` (separate) + `cli_auth_credentials_store` directive | yes (JSON) | **optional / configurable** |
| **Hermes Agent** | `config.yaml` | `.env` (separate; `hermes config set` auto-routes keys→`.env`, rest→`config.yaml`) | yes | no |
| **OpenCode** | `opencode.json` | `auth.json` (separate) + `{env:}`/`{file:}` substitution | yes (JSON) | no |
| **Claude Code** | `~/.claude/settings.json` | macOS → Keychain; Linux → `.credentials.json` (separate) | macOS no / Linux yes | **macOS only** |

**Industry consensus**: none of these mix plaintext API keys into the primary
config file. All separate credentials into a dedicated, permission-restricted
file (or macOS Keychain), keep env vars as the headless/CI path, and treat OS
keyring as **optional / macOS-opportunistic**, never a universal requirement.

Why the industry does **not** go keyring-first:

1. **Linux/headless/CI/containers have no secret service.** The `keyring` crate
   depends on D-Bus + a running gnome-keyring/kwallet. SSH sessions, CI runners,
   Docker, and WSL usually have none; keyring silently returns empty. Coding
   agents live in these environments.
2. **Cross-platform consistency cost.** Claude Code's macOS-Keychain-vs-Linux-file
   split directly caused bugs (`.credentials.json` deleted on Mac breaks
   Mac↔Linux shared volumes — anthropics/claude-code#1414).
3. **Portability / debuggability.** A file can be `cat`/edited/backed up/
   migrated; keyring is opaque.
4. **No daemon dependency.** A file needs only a filesystem; keyring needs a
   userspace daemon + D-Bus session bus.
5. **Support burden.** Keyring issues are platform-specific and hard to reproduce.

## Problem / improvement list

Priorities: 🔴 P0 correctness, 🟠 P1 architecture, 🟡 P2 UX, 🟢 P3 security, ⚪ P4 drift.

| ID | Pri | Problem | Evidence |
| --- | --- | --- | --- |
| P0-1 | 🔴 | `CredentialManager` disconnected from request path | `provider_resolver.rs:138-140` only reads `env::var` |
| P0-2 | 🔴 | `secrets.env` zombie store | written by `config_migration`, never loaded (N2 TODO) |
| P0-3 | 🔴 | `/model` is a placeholder | `builtin/repl.rs:63`; `set_model()` exists but unwired |
| P1-1 | 🟠 | No unified config system | code TOML + A1 vs desktop JSON + plaintext |
| P1-2 | 🟠 | No unified provider model | engine `ProviderProfile`+`CredentialRef` vs desktop `ProviderConnection` |
| P1-3 | 🟠 | No unified credential backend | env / plaintext JSON / keyring (4 variants) |
| P1-4 | 🟠 | Desktop bypasses engine config | builds `LlmClientConfig` in `AppState`, never writes config.toml |
| P1-5 | 🟠 | Desktop "mirror" duplication | active provider kept in both `providers.json` + `config.json`, manual sync |
| P2-1 | 🟡 | No `provider/model` identifier | bare ids in both products |
| P2-2 | 🟡 | Code has no `/connect` guided flow | desktop already has `test_provider_connection` + add-provider modal |
| P2-3 | 🟡 | Code `/config set` does not persist | desktop persists, code does not |
| P2-4 | 🟡 | No shared model catalog | independent lists, no discovery |
| P3-1 | 🟢 | A1 policy only enforced on code's config.toml | CredentialManager, secrets.env, desktop config.json all hold plaintext |
| P3-2 | 🟢 | `CredentialManager` has no locking | concurrent-write race risk |
| P3-3 | 🟢 | LLM keys are plaintext on disk everywhere; keyring covers only social | `CredentialRef::Keyring` is a stub |
| P4-1 | ⚪ | Default model id diverges | code `claude-sonnet-4-20250514` vs desktop `claude-sonnet-4-6` |
| P4-2 | ⚪ | Provider kind sets diverge | code 6 (incl Gemini) vs desktop `is_known_kind` 5 (no Gemini) |
| P4-3 | ⚪ | Probe/sniff logic duplicated | desktop `provider_probe_url` has no engine-level reusable copy |

## Decision

Four decisions, calibrated against the competitor benchmark.

### D1 — Adopt `provider/model` as the canonical model identifier

Add a `ModelRef { provider, model }` value type in `shannon-types`. It becomes
the user-facing identifier everywhere (CLI `--model`, `/model`, Desktop picker,
config). Internally it maps onto the existing `ActiveTarget { provider_id,
model_id }`. Bare ids remain accepted via catalog/alias fallback (backward
compat). Closes P2-1; prerequisite for all later phases.

### D2 — Unify provider/credential management at the engine layer

The engine (`shannon-types` / `shannon-core`) becomes the single home for
provider profiles, model refs, and credential resolution. Both front-ends
consume it. Desktop stops bypassing the engine config. Closes P1-1 / P1-2 /
P1-4 / P1-5.

### D3 — Credential backend: plaintext cred file (default) + env (CI) + keyring (opt-in)

**Not keyring-first.** This is the revision from the initial proposal.

- **Default backend**: the existing `CredentialManager` plaintext store at
  `~/.shannon/credentials/<svc>.json` (0600). This already matches what Codex
  (`auth.json`), Hermes (`.env`), and OpenCode (`auth.json`) ship.
- **env vars**: first-class, the CI/headless path. Always works, no daemon.
- **keyring**: **opt-in / opportunistic** (macOS-first, or where a secret
  service is detectable). Modelled on Codex's `cli_auth_credentials_store` —
  configurable, not required. The `CredentialRef::Keyring` variant stays as an
  opportunistic backend that is only selected when the platform reports a usable
  secret service; otherwise it falls through to the file store. **Out of the
  critical path.**

This is the explicit rejection of the earlier "keyring-first → file fallback"
ordering. See *Rejected alternatives*.

### D4 — A1 reframed: separation of concerns, not anti-plaintext

The A1 rule ("config files carry only `CredentialRef::Env` references, never
plaintext") is **retained**, but the justification is corrected:

- ❌ Old framing: "plaintext is evil" → led to over-engineering toward keyring.
- ✅ New framing: **config is shareable, credentials are not.** Separation lets
  config live in dotfiles repos / SCM at 0644 while the credential file stays
  0600 and never leaks. **A plaintext credential file is acceptable** (all four
  benchmarked competitors do this).

Consequence: the cleanup target is **location/hygiene, not encryption**. The
real violations to fix are P0-1 (wiring) and Desktop mixing `api_key` into
`config.json`/`providers.json` (P3-1) — Desktop keys must move **out** of those
files into the shared credential store, matching competitor separation.

## Rejected alternatives

1. **Universal keyring-first.** Rejected: the four reference tools do not do
   this, for the five reasons in *Competitor benchmark*. It would also block
   headless/CI users unless paired with a file fallback — at which point the
   file is the real default and keyring is the opt-in (which is D3).
2. **Adopt OpenCode's flat `auth.json` verbatim.** Rejected: Shannon's typed
   `CredentialRef` (Env/Store/Keyring/Ephemeral) is richer and already
   implemented; flattening loses the per-profile backend selection.
3. **OAuth-first `/connect`.** Rejected for v1: API-key flow covers the
   providers Shannon targets. OAuth (Claude Pro/Max, ChatGPT, Copilot) is
   deferred but can reuse Desktop's existing loopback OAuth infra later.
4. **Copy OpenCode's `npm` provider-package field.** N/A: Shannon is Rust with
   compile-time adapters, not a JS SDK loader.

## Implementation plan

Principle: capability moves **down** into `shannon-types` / `shannon-core`;
both front-ends become thin consumers. Each phase is independently shippable.

### Phase 0 — Shared `ModelRef` (`provider/model`)  *(prerequisite, ~2 days)*

**Why**: P2-1; foundational primitive for every later phase.

**Scope**:
- New `ModelRef { provider, model }` with `parse` / `Display` / `validate` /
  alias resolution in `shannon-types/src/model_ref.rs`.
- Map `ModelRef` ↔ existing `ActiveTarget { provider_id, model_id }`.
- Index `MODEL_CATALOG` (`shannon-core/src/model_registry.rs`) by `provider/model`.
- `provider_resolver` accepts `ModelRef`; CLI `--model anthropic/claude-sonnet-4`
  with bare-id backward-compat (`shannon-cli/src/main.rs`).
- Desktop `configure` / `switch_provider` accept `provider/model`
  (`desktop/src/commands_config.rs`).

**Acceptance**: both products resolve `provider/model` and bare ids identically;
existing env-only configs are unaffected.

### Phase 1 — Unified credential resolution (fix the disconnect)  *(core, ~3 days)*

**Why**: P0-1 / P0-2 / P1-3 — the only actually-"broken" items.

**Scope**:
- Add `CredentialRef::Store { service }` variant; `resolve_credential()`
  (`shannon-core/src/provider_resolver.rs`) consults `CredentialManager` →
  **fixes P0-1**.
- Remove or formally deprecate the `secrets.env` zombie path
  (`shannon-core/src/config_migration.rs`) → **fixes P0-2**.
- `CredentialManager`: advisory file lock + atomic write (`tmp + rename`)
  → **fixes P3-2**.
- `CredentialRef::Keyring` becomes an **opportunistic** backend: selected only
  when a usable secret service is detected, else falls through to the file
  store. **No keyring implementation work blocks delivery.**

**Acceptance**: `/credentials store anthropic sk-…` then sending a request
works **with no environment variable set** (P0-1 fixed). CI/headless (env-only)
paths unchanged.

### Phase 2 — Unified provider model + config convergence  *(architecture, ~4-5 days)*

**Why**: P1-1 / P1-2 / P1-4 / P1-5 / P3-1 / P4-1 / P4-2 / P4-3.

**Scope**:
- Adopt the engine `ProviderProfile` + `CredentialRef` as the **single** provider
  abstraction. Desktop's `ProviderConnection` becomes a thin view over it
  (fields are near-subsets; migration cost is low).
- Desktop stops bypassing engine config: provider write-ops go through the
  engine unified store (schema aligned; JSON format may remain). The active
  selection is read back from one place.
- Move Desktop `api_key` **out** of `config.json` / `providers.json` into the
  shared `credentials/` store (P3-1) — config holds only a `CredentialRef`.
- Promote Desktop's `provider_probe_url` / `test_provider_connection` **down**
  into the engine so both front-ends reuse one implementation (P4-3, P2-2).
- Unify default model id and provider kind set across both products (P4-1, P4-2).

**Acceptance**: after switching provider in Desktop, `shannon` CLI reads the
**same** active provider. Only one connection-test implementation exists.

### Phase 3 — `/connect` + `/model` in code; Desktop re-platformed  *(~2-3 days)*

**Why**: P0-3 / P2-2.

**Scope**:
- Code `/connect` guided wizard, reusing the engine probe + credential store
  promoted in Phase 2 (`shannon-commands/src/builtin/connect.rs`, new).
- Code `/model` real command: list (`provider/model`, grouped by provider) +
  session-scoped hot switch via `LlmClient::set_model()`; alias support;
  optional `--save` to persist (`shannon-commands/src/builtin/repl.rs` + a
  `shannon-ui` ratatui picker).
- Desktop: existing add-provider modal re-points at the unified store (UI
  largely unchanged; backend swaps to engine APIs).
- `/credentials` kept as the power-user escape hatch.
- **OAuth deferred** (Claude Pro/Max, ChatGPT, Copilot) — reuse Desktop's
  existing loopback OAuth infra in a later phase.

**Acceptance**: `code` `/connect anthropic` → paste key → `/model anthropic/…`
works end-to-end without env vars; Desktop flow unchanged to the user but
unified underneath.

### Phase 4 — Config persistence + variable substitution  *(~1-2 days)*

**Why**: P2-3 / P2-4 (partial).

**Scope**:
- Code `/config set` writes through to `.shannon.toml` (A1-respecting: only
  `Env` refs, never plaintext).
- `{env:VAR}` and `{file:path}` substitution in the `unified_config` loader
  (parity with OpenCode).

### Phase 5 — Dynamic catalog (optional, deferrable)

**Scope**: models.dev fetch (cached, static-catalog offline fallback — must not
break headless/CI), `enabled_providers` / `disabled_providers` allowlists,
`small_model` field.

## Open decisions (need sign-off before Phase 1+)

1. **Default credential backend** — confirm D3: plaintext `credentials/` file
   as default, env as CI path, keyring opt-in. (Recommended: yes.)
2. **Desktop key migration** — move Desktop `api_key` out of `config.json` /
   `providers.json` into the shared credential store in Phase 2? (Recommended:
   yes — aligns Desktop with the competitor separation pattern.)
3. **Unified provider abstraction** — engine `ProviderProfile` as canonical,
   Desktop adopts? (Recommended: yes.)
4. **Desktop stops bypassing engine config** — P1-4 fix in Phase 2, or defer?
   (Recommended: Phase 2, but can stage as store-first then client-config-first.)
5. **Model switch default scope** — session-scoped default + `--save` /
   `set_active` to persist (parity with Desktop `set_active_provider` and
   OpenCode session-only switching)? (Recommended: yes.)
6. **OAuth + models.dev** — defer to post-v1? (Recommended: defer.)

## Risks & mitigations

| Risk | Mitigation |
| --- | --- |
| Backward compat break (bare ids, env-only, old `.shannon.toml`, CI/headless) | bare-id fallback in Phase 0; env path never removed; headless never depends on interactive `/connect` |
| A1 regression (plaintext leaks into config) | keep A1 enforcement in `ConfigBuilder`; CI grep for `api_key = "sk-` patterns in committed config samples |
| `CredentialManager` ↔ `CredentialRef` schema migration | add `CredentialRef::Store` as new variant (additive); no breaking change to existing `Env` profiles |
| Desktop behavior change during convergence | stage Phase 2: unify store first, then swap `client_config` construction; keep `CONFIG_UPDATED` event contract stable |
| Dynamic catalog (Phase 5) breaking offline/CI | static catalog always wins when network unavailable; feature-flagged |

## References

- Competitor docs: [Codex auth](https://learn.chatgpt.com/docs/auth) ·
  [Hermes config](https://hermes-agent.nousresearch.com/docs/user-guide/configuration) ·
  [OpenCode providers](https://opencode.ai/docs/providers/) ·
  [Claude Code keychain #1414](https://github.com/anthropics/claude-code/issues/1414)
- Internal: `shannon-types/src/provider_config.rs` ·
  `shannon-core/src/{provider_resolver,credential_manager,config_migration,unified_config,model_registry}.rs` ·
  `shannon-engine/src/api/{types,client}.rs` ·
  `shannon-commands/src/builtin/{repl,config,credentials}.rs` ·
  `desktop/src/{config,commands_config}.rs`
