# P2-5 Execution Plan — `ProviderConfigService` (ADR-0008 Decision 3)

> Status: **Draft for review — no code changes yet.**
> Parent: [`docs/adr/0008-provider-model-command-architecture-remediation.md`](../adr/0008-provider-model-command-architecture-remediation.md) Decision 3; task [`docs/plans/provider-model-command-remediation.md`](provider-model-command-remediation.md) §P2-5.
> Estimate: ~2 days. Prerequisite P1-4 (`/disconnect`) ✅ done.

## 1. Goal

One service in `shannon-core` owns the write path to `~/.shannon/providers.toml`. Both
the REPL (`/connect`, `/disconnect`) and the CLI (`shannon providers add | remove`,
`shannon list-providers`) call it. Today there are **two** write paths that produce
**different on-disk shapes** for the same file.

## 2. Current state (evidence)

### 2a. The shared store already exists

`shannon-core::provider_config_store::ProviderConfigStore`
([`provider_config_store.rs:227`](../../crates/shannon-core/src/provider_config_store.rs)) already
exposes a rich, **additive** mutator API:

| Method | Effect |
|---|---|
| `load_or_default()` / `load()` | read (parse toml) |
| `connected_slugs()` | slugs present in the `"default"` profile |
| `ensure_provider(p)` | get-or-create a provider slot |
| `upsert_profile(profile, model)` | insert-or-replace a provider + pin `active_target` |
| `set_active(p, model)` | move `active_target` |
| `set_tier(p, tier, model)` | per-tier override |
| `set_default_max_tokens(p, n)` | max-tokens override |
| `remove_profile(id)` | delete a provider slot (clears `active_target` if it pointed there) |
| `save()` / `save_locked()` / `save_at()` | persist (atomic, flock-guarded) |

So the merge/lock/atomic-write machinery is already shared. P2-5 is about routing the
**callers** through one *semantic* entry point, not about rebuilding storage.

### 2b. The divergence (the real bug P2-5 fixes)

| | REPL `/connect` | CLI `providers add` |
|---|---|---|
| Build | `build_connect_profile(p)` → fresh **single-provider** `ProviderModelConfig` (`provider_resolver.rs:411`) | `build_profile(args)` → one `ProviderProfile` (`commands_providers.rs:461`) |
| Apply | `provider_config_store::save(&cp.config, None)` — **overwrite**, no load+merge (`config.rs` `apply_connect` step 2) | `store.upsert_profile(profile, model)` on a `load_or_default()` store — **merge** (`commands_providers.rs:495`) |
| Net effect on `providers.toml` | replaces the file with just this one provider | adds/replaces one provider, keeps the rest |

**Consequence:** `/connect anthropic <k1>` then `/connect openai <k2>` **silently drops
the Anthropic entry.** Yet the rest of the system assumes multiple connected providers
are possible — `connected_slugs()` returns a *set*, the welcome `StatusCard` lists every
connected provider, and `/disconnect` removes *one*. The REPL and CLI disagree on the
contract for the same file. **This is the bug to fix; consolidation is the means.**

(Aside: the API key itself is stored separately in
`~/.shannon/credentials/<service>.json` (0600) via `CredentialManager`, never in
`providers.toml` — decision A1. That part is already shared and stays as-is.)

## 3. Target API — `shannon-core::ProviderConfigService`

A thin semantic layer **over** `ProviderConfigStore` (not a replacement). It captures the
*intent* of each user action and centralizes the load-mutate-persist sequence so the two
front-ends can't diverge again.

```rust
// crates/shannon-core/src/provider_config_service.rs (new, ~150 lines)

pub struct ProviderConfigService {
    store: ProviderConfigStore,
}

/// What `connect`/`providers add` needs back to drive the live session
/// (engine switch + credential reload + user-facing messages).
pub struct ConnectedProvider {
    pub provider: LlmProvider,
    pub model_id: String,
    pub service: String,           // credential-store key (== provider slug)
    pub saved_path: PathBuf,
}

impl ProviderConfigService {
    pub fn load() -> Self { Self { store: ProviderConfigStore::load_or_default() } }

    /// Connect (upsert) a provider. Replaces `build_connect_profile` + overwrite
    /// with an additive upsert — the fix for §2b. `model` defaults to the catalog
    /// first model; `base_url` defaults to the provider's canonical URL.
    /// Does NOT store the API key (that stays with CredentialManager, A1) and
    /// does NOT touch the running engine (callers do `apply_model_selection` +
    /// `reload_credential` — those are session concerns, not config concerns).
    pub fn connect(
        &mut self,
        provider: LlmProvider,
        model: Option<&str>,
        base_url: Option<&str>,
        make_active: bool,
    ) -> ConnectedProvider { /* upsert_profile + optional set_active + save */ }

    /// Disconnect (remove) a provider. Replaces handle_disconnect's inline
    /// load + remove_profile + save. Returns the next remaining connected
    /// slug (so the REPL can switch to it) or None.
    pub fn disconnect(&mut self, provider: &LlmProvider) -> DisconnectOutcome { /* ... */ }

    pub fn set_active(&mut self, provider: &LlmProvider, model: &str) { /* delegates */ }
    pub fn set_tier(&mut self, provider: &LlmProvider, tier: TierName, model: &str) { /* ... */ }
    pub fn set_max_tokens(&mut self, provider: &LlmProvider, max: Option<u32>) { /* ... */ }

    pub fn connected_slugs(&self) -> HashSet<String> { self.store.connected_slugs() }
    pub fn into_inner(self) -> ProviderConfigStore { self.store }
}
```

**Design rules**
- `ProviderConfigStore`'s raw mutators stay `pub` (the desktop and tests use them
  directly). The service is the *recommended* path for the two command front-ends, not a
  hard gate — avoids a big-bang rewrite of desktop callers.
- The service **never** owns the API key or the running engine. Those stay in the REPL/CLI
  call sites (`CredentialManager::store_or_update`, `apply_model_selection`,
  `engine.reload_credential`). Mixing config-persistence with session/runtime concerns is
  what made `apply_connect` a 130-line monolith (and is exactly what P3-4 will split next).
- Every method does `load → mutate → save` internally, so callers can't forget the save
  half. (The store's existing `save()` already flocks + atomic-writes.)

## 4. Migration sequence (low-risk ordering)

Each step is independently mergeable and green before the next.

**Step 1 — Introduce the service (additive, no caller changes).**
New module `provider_config_service.rs`, re-exported from `shannon-core`. Unit tests use
the existing `ProviderConfigStore` round-trip fixtures. *No behavior change anywhere.*

**Step 2 — Route CLI `providers add` through it.**
`apply_provider_add` (`commands_providers.rs:484`) currently does `upsert_profile` on a
store the caller loaded. Swap to `ProviderConfigService::connect` (with `make_active =
args.set_active`). Behavior is identical (it was already an upsert) — this proves the
service on the simpler, already-correct path first. Keep `apply_provider_add`'s signature
so CLI tests don't churn.

**Step 3 — Route REPL `/disconnect` through it.**
`handle_disconnect`'s inline `load_or_default` + `remove_profile` + `save` block becomes
`ProviderConfigService::disconnect`. The "switch to next connected provider, else clear"
logic stays in the REPL (session concern) but reads `DisconnectOutcome::next_active`.

**Step 4 — Route REPL `/connect` through it (the bug fix).**
`apply_connect` step 2 (`provider_config_store::save(&cp.config, ...)`) becomes
`ProviderConfigService::connect(provider, None, None, true)`. **This changes `/connect`
from overwrite to upsert** — the correctness fix from §2b. Everything else in
`apply_connect` (credential store, `apply_model_selection`, validate, `reload_credential`,
spawn refresh) is untouched and stays in the REPL (P3-4 will extract those later).
`build_connect_profile` becomes dead → deleted (its single test migrates to the service).

**Step 5 — Retire `provider_config_store::save` free fn from the connect path.**
After step 4 no caller uses the bare overwrite `save(&cfg, ...)` for connect. Audit
remaining callers (desktop); leave the fn for low-level use but document that
`ProviderConfigService` is the command-layer path.

## 5. Round-trip test matrix

The store already has a round-trip suite; these add the **cross-front-end** scenarios that
catch the §2b class of regression. All hermetic (`tempfile`, no `~/.shannon` touch) —
matches the existing `apply_provider_add` test style.

| # | Scenario | Assert |
|---|---|---|
| T1 | `connect(Anthropic)` then `connect(OpenAI)` via the service | both slugs in `connected_slugs()` (this is the bug fix — fails today via REPL path) |
| T2 | `connect` then `disconnect` the same provider | slug gone; others remain; `next_active` correct |
| T3 | `connect` non-active, then `disconnect` active | active switches to the remaining slug |
| T4 | `connect` with `make_active=false` | provider present, `active_target` unchanged |
| T5 | Service `connect` writes the same toml shape as CLI `apply_provider_add` for identical inputs | byte-shape equality on the `"default"` profile (the "two paths, one shape" guarantee) |
| T6 | Existing CLI `providers add` / `list-providers` tests still pass unchanged | no CLI regression |
| T7 | `/connect A` → `/connect B` → restart → engine loads **both** as connected | end-to-end via `resolve_active_target` + `connected_slugs` |

T1 and T5 are the load-bearing tests; T1 would have failed on today's REPL path.

## 6. Risks & rollback

- **Behavior change in step 4**: `/connect` switches overwrite→upsert. This is the
  *intended* fix (the system already assumes multiple connected providers), but it's the
  one user-visible change. **Mitigation:** call it out in the commit message and the
  CHANGELOG; if it surprises a user who relied on `/connect` as "reset to one provider",
  they can still `/disconnect` others. Rollback = revert the step-4 commit (service stays).
- **CLI `apply_provider_add` signature**: kept stable in step 2 so `commands_providers`
  tests don't churn; only its body changes.
- **Desktop callers**: untouched (they use `ProviderConfigStore` directly and are out of
  scope per ADR-0005 Phase 2 deferral). The service is additive, so no breakage.
- **`build_connect_profile` deletion (step 4)**: one test migrates; grep confirms no other
  caller (`apply_connect` is the only one).

## 7. Out of scope (explicit)

- Splitting `apply_connect` into step functions → **P3-4** (depends on this; do next).
- Splitting `config.rs` / `model_registry.rs` → **P2-8** (independent).
- Desktop re-platforming onto the service → **ADR-0005 Phase 2** (deferred).
