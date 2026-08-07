# ADR 0009 — Provider Store Read Facade (desktop read-path consolidation)

**Status**: Accepted
**Date**: 2026-08-06
**Theme**: Consolidate the desktop's scattered `provider_store` read path
into a single typed read facade — the read-side complement to ADR-0008
Decision 3 / Wave 6's write-path `ProviderConfigService`.
**Supersedes**: —
**Related**: ADR-0005 (Unified Provider/Model/Credential Management —
established the engine `ProviderConfigStore` + `ProviderProfile` +
credential store this facade reads from); ADR-0008 (Provider/Model
Command Architecture Remediation — Decision 3 consolidated the *write*
path this ADR consolidates on the read side).

---

## TL;DR

ADR-0008 Decision 3 + Wave 6 ([PR #34][pr34]) collapsed every
`providers.toml` **write** across CLI / REPL / Desktop into one
`ProviderConfigService` (RAII `flock` + lock-then-reload read-modify-write).
The **read** path was deliberately left alone, and it now has the same
disease the write path had before #34: **~13 desktop lock sites**
(plus several more bare `.config()` reads) each independently do

```rust
let store = state.provider_store.lock().await;   // 1. acquire the same mutex writers take
let cfg    = store.config();                     // 2. borrow the config
let p      = cfg.profiles.get("default")?;       // 3. hand-navigate the "default" profile
// 4. project to whatever wire view this one command needs
```

with no typed read contract, a hardcoded `"default"` assumption repeated
at every site, the mutex guard held across arbitrary downstream work,
and a legacy `ProviderConnection` wire type that ADR-0005 said to retire.

This ADR proposes a **`ProviderStoreReadFacade`**: a typed, cheaply
cloneable **snapshot** of the UI's read views, produced under one
short-lived lock and released before any further `.await`. It is the
read-path analog of `ProviderConfigService`. Retiring `ProviderConnection`
on the wire is deferred to a later phase (gated by the `Welcome.tsx`
rewrite) so this ADR can land without a frontend overhaul.

[pr34]: https://github.com/diff-lab-com/shannon-agent/pull/34

---

## Context

### What the write path looks like now (the precedent)

`ProviderConfigService` ([`crates/shannon-core/src/provider_config_service.rs`][svc],
landed in #34) gives every write a single shape:

1. acquire an exclusive file `flock` (RAII guard);
2. reload the on-disk `providers.toml` into the in-memory store under that lock;
3. apply the mutation;
4. persist + drop the guard.

One type, one contract, one place that decides lock granularity and
reload semantics. ADR-0008 Decision 3 is the record of that choice.

[svc]: ../../crates/shannon-core/src/provider_config_service.rs

### What the read path looks like now (the problem)

There is no read-side analog. Every desktop command that needs provider
state re-implements the lock-borrow-navigate-project pattern. A census
of `desktop/src/`:

- **13 call sites** in `commands_config.rs` acquire
  `state.provider_store.lock().await` (concentrated in the connect /
  save / delete / set-active / list commands); several more read
  `store.config()` off an already-held guard.
- `commands.rs:406` — `resolve_active_target(provider_store.config())`,
  another hand-navigation of the store for active-target resolution.

Each lock site independently:

- **Re-acquires the same `tokio::Mutex`** the write path takes. A read
  that does async work while holding the guard serializes every
  provider write behind it (and re-entrant `.await` paths are a
  deadlock hazard).
- **Re-hardcodes `profiles.get("default")`**. The "default profile holds
  user-managed desktop connections" invariant lives in comments, not in
  a type. A second profile name (gateway routing, auxiliary) would need
  to be found-and-fixed at every site.
- **Re-projects to a different view**: some want the active provider id,
  some want one `ProviderProfile`, some want the full `Vec`, some want a
  `ProviderConnection` for the wire. There is no shared projection.

### The one good primitive that already exists

PR #37 ([`fix/desktop-drop-providers-json-dualwrite`][pr37]) extracted
**`providers_file_from_store`** (`desktop/src/commands_config.rs:1302`):
a *pure* helper that maps the engine `ProviderConfigStore` → the
`ProvidersFile` wire shape (`active_provider_id` + `Vec<ProviderConnection>`),
and is unit-testable without a Tauri runtime. `list_providers`
(`commands_config.rs:1276`) is the one read command that already goes
through it. This ADR generalizes that pattern to *all* reads and wraps
it in a type.

[pr37]: https://github.com/diff-lab-com/shannon-agent/pull/37

### Why now

#34 (write path) and #37 (`providers_file_from_store`) both just merged
to `dev`. The read path is the remaining half of the same consolidation,
and the cheapest moment to land it is *before* the next desktop feature
adds a 20th read site that copies the pattern. The `Welcome.tsx` rewrite
(ADR-0005 Phase 2 follow-up) will add several *new* read consumers; doing
this first means those consumers are born behind the facade.

---

## Decision

### Decision 1 — A `ProviderStoreReadFacade` typed snapshot

Introduce a small read-only type in `desktop/` (mirroring where
`providers_file_from_store` already lives) that, **in one mutex
acquisition, produces an owned snapshot** of the read views the UI
needs:

```rust
/// Cheaply cloneable, `Send` snapshot of the desktop's provider read views.
/// Built under one short-lived lock; safe to hold across `.await` after release.
pub struct ProviderReadSnapshot {
    pub active_provider_id: Option<String>,
    pub providers: Vec<ProviderProfile>,   // canonical ADR-0005 type
    // …one field per read view the UI actually needs; no MutexGuard inside
}

impl ProviderReadSnapshot {
    /// One lock acquisition, one projection, immediate release.
    pub async fn capture(store: &Mutex<ProviderConfigStore>) -> Self { /* … */ }
    /// Non-async projection over a borrowed store (for already-locked call sites).
    pub fn from_store(store: &ProviderConfigStore) -> Self { /* … */ }
    /// Wire projection, generated in exactly one place.
    pub fn to_providers_file(&self) -> ProvidersFile { /* … */ }
}
```

All desktop read commands go through `ProviderReadSnapshot::capture`
(or `from_store` when already holding the guard for a legitimate
reason). The free function `providers_file_from_store` becomes a
delegation: `snapshot.to_providers_file()`.

The facade lives in **`desktop/`**, not `shannon-core`, because its
projection is a UI read contract (`ProviderProfile` selection + active
id). The core `ProviderConfigStore` remains the source of truth and
gains no desktop-specific surface.

### Decision 2 — Snapshot-then-release: never hold the mutex across `.await`

`capture` clones the projection **under** the lock and drops the guard
before returning. Callers receive an owned, `Send` snapshot they can
hold across async work without blocking writers and without re-entrant
deadlock risk. This is the read-path version of the discipline
`ProviderConfigService` enforces on writes.

Reads do **not** take the file `flock` — they read the in-memory cache,
matching today's behavior and ADR-0005's "engine store is the read
authority" model. Cross-process consistency is the write path's job.

### Decision 3 — Wire type: keep `ProviderConnection` as a DTO, retire it later

`ProviderConnection` is the legacy desktop wire type. ADR-0005's
canonical type is `ProviderProfile`. This ADR deliberately **does not**
change the wire shape: the facade exposes `ProviderProfile` internally
and generates `ProviderConnection` (via the existing
`config::from_provider_profile`) in the single `to_providers_file()`
call. The frontend keeps consuming `ProviderConnection` unchanged.

Retiring `ProviderConnection` on the wire is a **separate phase**,
gated by the `Welcome.tsx` rewrite (~724 lines, per the Wave 6 plan).
Coupling them would force a frontend overhaul into a read-path
consolidation PR. Splitting them lets each land and review independently.

---

## Consequences

### Positive

- **One read contract.** New read consumers call `capture()`; they
  cannot accidentally hold the mutex, hardcode a profile name, or invent
  a new projection.
- **Writers are never blocked by async read work.** The snapshot is
  owned; the guard is gone before any downstream `.await`.
- **`"default"` invariant lives in one type**, not 16 comments.
- **Wire-type retirement becomes mechanical.** When `Welcome.tsx` is
  ready, flip `to_providers_file()` (or expose the `ProviderProfile` vec
  directly) — one call site, not 13.
- `providers_file_from_store` becomes testable as a snapshot projection,
  and `list_providers` shrinks to `Ok(capture().to_providers_file())`.

### Negative

- **One more type** to learn (`ProviderReadSnapshot`). Mitigated by it
  being a thin, documented snapshot with a single constructor.
- **An extra `clone`** per read (the projection is cloned out under the
  lock). Provider lists are small (single-digit `ProviderProfile`s);
  the clone is negligible against the cost it removes (mutex contention
  + duplicated logic).
- **Two-phase wire retirement** means `ProviderConnection` lingers. This
  is intentional (Decision 3) but means a future reader sees both types
  until Phase 2 lands. The ADR link in `to_providers_file()` documents why.

### Neutral

- The facade is desktop-local; `shannon-core` is unchanged. If the CLI
  later wants the same read contract, the snapshot can be promoted to
  core then — but that is speculative (YAGNI) until a second consumer
  appears.

---

## Alternatives Considered

### A. Status quo — keep the helper, don't facade

Leave `providers_file_from_store` as the one good primitive; let the
other ~15 sites keep their inline lock-borrow-navigate.

- **For**: zero new types; the helper already covers the highest-value
  read (`list_providers`).
- **Against**: the disease is the *pattern*, not the one command. The
  next read site copies the inline pattern; the mutex-held-across-await
  and hardcoded-`"default"` risks stay. This is exactly the argument
  ADR-0008 made for consolidating writes — it applies symmetrically here.

### B. Push read-snapshots into the engine (`shannon-core`)

Add `ProviderConfigStore::snapshot() -> ProviderReadSnapshot` in core,
so both CLI and desktop share it.

- **For**: one source of truth for the snapshot shape; reusable.
- **Against**: the snapshot's projection (which fields, `ProviderProfile`
  vs `ProviderConnection`) is a *UI* concern; baking it into core
  couples the engine to a frontend read contract. No second consumer
  exists today. Promote later if one appears (Decision 1 notes this).

### C. Retire `ProviderConnection` now, in the same change

Flip the wire to `ProviderProfile` as part of this ADR.

- **For**: one breaking change instead of two; ADR-0005's canonical type
  reaches the frontend immediately.
- **Against**: forces the `Welcome.tsx` (724-line) rewrite into a
  read-path PR, blowing up its scope and review surface, and couples two
  independently-reasonable changes. Decision 3 splits them deliberately.

---

## Implementation References

### Code (current state)

- Write-path precedent: `crates/shannon-core/src/provider_config_service.rs`
  (RAII `flock` + lock-then-reload R-M-W; landed in #34).
- Read-path primitive to generalize:
  `desktop/src/commands_config.rs:1302` (`providers_file_from_store`).
- Read commands to migrate:
  `desktop/src/commands_config.rs` (16 sites) + `desktop/src/commands.rs:406`.
- Store API read surface:
  `ProviderConfigStore::config() -> &ProviderModelConfig`
  (`crates/shannon-core/src/provider_config_store.rs:334`).

### Companion plan

Wave 6 P2-2 design record: `docs/spikes/p2-2-s1-1-lock-design.md` (§7 —
implementation results + commit map; this ADR is the read-side follow-up
noted there).

### Related docs

- `docs/improvement-plan-2026-08.md` — P4 entry (this ADR).
- ADR-0005 Phase 2 task 4/5 — the one-shot migration that made the engine
  store the read authority this facade wraps.

---

## Open Questions / Re-evaluation Triggers

1. **Watch-based reads vs. snapshot reads.** This ADR proposes pull-based
   snapshots (read commands call `capture()`). If the UI needs live
   updates without a refetch (e.g. status-bar pill reflecting a
   background write), evaluate a `tokio::sync::watch` channel fed by the
   write path. Out of scope here; trigger = a feature that needs push.
2. **Snapshot granularity.** One `ProviderReadSnapshot` with all fields,
   or per-view snapshots (`ActiveProvider`, `ProviderList`, …)? Start
   with one struct (simplest); split if a read consistently pays for
   fields it doesn't use. Trigger = a measurable clone cost or a caller
   that only needs the active id.
3. **Phase 2 (wire retirement) scheduling.** When does `Welcome.tsx` get
   rewritten? That unblocks dropping `ProviderConnection`. Trigger =
   sprint capacity for the frontend work.
4. **Re-evaluate this ADR if** a second non-desktop consumer (CLI) needs
   the same snapshot — promote the type to `shannon-core` (Alternative B).

---

## Acceptance

This ADR is **Accepted** (2026-08-07). The implementation lands
`ProviderReadSnapshot` (`desktop/src/provider_read_snapshot.rs`) and
migrates the read path through it:

- `list_providers` + `providers_file_from_state` → `capture().to_providers_file()`
- `configure('api_key')` / `configure('base_url')` active-provider reads →
  `capture()` + `active_profile()`
- `test_all_providers` → `capture().to_providers_file().providers`
- The free function `providers_file_from_store` is retired (its contract
  lives on in `ProviderReadSnapshot::from_store` + `to_providers_file`,
  and the renamed `snapshot_*` tests).

The remaining read sites (`rebuild_client_config_from_store`,
`configure('provider')`'s `find_provider_by_kind`, the
`AppState::build_client_config` guards) pass the whole store to engine
helpers rather than hand-navigating the `"default"` profile, so they are
out of scope for this ADR and tracked in the companion plan.

`list_providers` returns the identical wire shape — the five
`list_providers_*` tests pass unchanged, proving Decision 3's "no wire
change" claim.
