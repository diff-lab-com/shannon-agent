# P2-2 C3 — CLI ↔ Desktop Provider Parity Matrix (design spike)

**Track**: Wave 6 P2-2 (read/write path consolidation follow-up)
**Status**: Proposed (design; implementation ~3-5d, separate PRs)
**Date**: 2026-08-07
**Foundation**: [`provider_cross_process_consistency.rs`][harness]
(S1-4, merged in #34); [ADR-0008][adr8] D3 (single write path);
[ADR-0009][adr9] (read facade).

[harness]: ../../crates/shannon-core/tests/provider_cross_process_consistency.rs
[adr8]: ../adr/0008-provider-model-command-architecture-remediation.md
[adr9]: ../adr/0009-provider-store-read-facade.md

---

## 0. Context — what the foundation already proves

S1-4's harness pins the **cross-writer consistency** property: N
`ProviderConfigService` instances over the same `providers.toml`, each
doing `lock → reload → mutate → save`, cannot lose each other's updates.
It models "two processes" as two threads (sound because `flock(2)`
serializes by open-file-description, identical per-call regardless of
whether callers share a process — see the harness header comment).

What it does **not** pin is **surface parity**: that the CLI surface
(`shannon providers add/remove/use`, the REPL `/connect`, the
`ProviderConfigService::connect` path) and the desktop surface
(`save_provider` / `delete_provider` / `set_active_provider` Tauri
commands, the `LockedService` R-M-W arms in `configure()`) produce
**byte-identical on-disk state and identical read views** for the same
logical operation. The foundation's writers are all the same
*kind* — bare `ProviderConfigService` calls — not the two distinct
surfaces a user actually drives.

C3 closes that gap with a **parity matrix**: every provider operation
exercised through both surfaces, asserting they agree on disk and on the
wire.

---

## 1. Why this matters now

- **ADR-0008 D3 collapsed the write path** but the two surfaces still
  compose their R-M-W differently (CLI's `connect` is one bare service
  call; desktop's `configure()` opens a `LockedService`, does
  `reload_locked`, then `upsert`). A regression that makes them disagree
  is invisible to S1-4 (which tests only one composition style).
- **ADR-0009's read facade** claims `list_providers` returns the same
  shape the CLI would compute. That claim is asserted by unit tests on
  one side only; C3 asserts it across surfaces.
- **The `Welcome.tsx` rewrite** (ADR-0005 Phase 2) will add new desktop
  read consumers. Without a parity matrix, a frontend change can drift
  from the CLI's view silently.

---

## 2. The parity matrix

Each row is one logical operation with a CLI driver, a desktop driver,
and a shared assertion over `providers.toml` + the read view.

| # | Operation | CLI driver | Desktop driver | Shared assertion |
|---|-----------|------------|----------------|------------------|
| 1 | add provider | `ProviderConfigService::connect` (the path `shannon providers add` + REPL `/connect` share) | `save_provider` Tauri cmd | `providers.toml` has the profile; `active_target` matches; credential at `~/.shannon/credentials/<id>.json` |
| 2 | remove provider | `ProviderConfigService::remove` (the path `shannon providers remove` shares) | `delete_provider` Tauri cmd | profile absent; active cleared if it was active; credential file removed |
| 3 | set active | `ProviderConfigService::set_active` | `set_active_provider` Tauri cmd (`configure('provider')` arm) | `active_target.{provider_id,model_id}` match exactly |
| 4 | update field | REPL `/connect` re-arm (re-upsert with new base_url/model) | `configure('base_url')` / `configure('api_key')` arms | non-edited fields preserved; `~/.shannon/credentials/<id>.json` updated for key |
| 5 | list / read | `ProviderConfigStore::config()` (the read path `shannon providers list` + REPL `/provider` share) | `ProviderReadSnapshot::capture().to_providers_file()` | same `ProvidersFile` wire shape (active id + provider vec) |
| 6 | concurrent mixed | CLI writer thread + desktop writer thread interleave | (same) | no lost updates; final state independent of interleaving order |

Rows 1-5 are **deterministic single-operation parity**. Row 6 is the
**stress** row: the S1-4 harness generalised to mixed-surface writers.

---

## 3. Design

### 3.1 In-process first, real-binary later

The harness's "threads model processes" argument applies unchanged: the
two surfaces contend on the same `flock` whether they run in one process
or two. So C3's first cut is **in-process**: one test binary drives both
surfaces against a shared `TempDir` `providers.toml`. This is fast,
deterministic, and runs in `just test` without building a release binary.

A **real-binary smoke** layer (spawn `shannon providers add ...` as a
child process, then read via desktop's `ProviderReadSnapshot`) is a
follow-up gated on the CLI's test-fixture story — it needs a built
binary and a hermetic `HOME`, so it belongs in `just ci` (slow lane),
not the per-PR fast lane.

### 3.2 Test module layout

Extend the existing file rather than starting a new one — it already
has the `profile_for` / `count_profiles` helpers and the right header
convention:

```
crates/shannon-core/tests/provider_cross_process_consistency.rs   (S1-4)
  └─ + §C3 parity matrix                                         (this spike)
       ├─ fn cli_add_then_desktop_read_sees_it()
       ├─ fn desktop_add_then_cli_read_sees_it()
       ├─ fn cli_remove_and_desktop_remove_agree_on_disk()
       ├─ fn set_active_cli_vs_desktop_match()
       ├─ fn read_view_cli_vs_desktop_match()      // row 5 — exercises ProviderReadSnapshot
       └─ fn mixed_surface_writers_do_not_lose_updates()  // row 6 — generalises S1-4
```

Row 5 is the ADR-0009 acceptance gate promoted to a cross-surface test:
the desktop's `ProviderReadSnapshot::to_providers_file()` must equal the
file the CLI's `ProviderConfigStore::config()` projection yields, for
the same on-disk state.

### 3.3 The "desktop surface" inside a `shannon-core` test

The desktop Tauri commands live in `shannon-desktop`, not `shannon-core`
— a `shannon-core` integration test cannot call `save_provider` directly.
Two options:

- **(A) Test against the shared primitive, not the Tauri command.** Both
  surfaces ultimately call `ProviderConfigService` (CLI) or
  `LockedService` (desktop). C3 can model "the desktop surface" as the
  `LockedService` R-M-W pattern (`lock → reload_locked → upsert → drop`)
  that `configure()` uses, without needing Tauri. This keeps the test in
  `shannon-core` and is faithful to the actual divergence risk (the two
  R-M-W compositions).
- **(B) Put the parity tests in `shannon-desktop`'s test suite** where
  they can call the real Tauri commands (via the same `AppState` +
  `provider_store` the unit tests already build). Higher fidelity, but
  couples the matrix to the desktop's heavier test harness.

**Recommendation: (A) for rows 1-4,6** (the R-M-W composition is the
risk; `LockedService` is the desktop's actual pattern), **(B) for row 5**
(the read view *is* desktop-local — `ProviderReadSnapshot` lives there —
so the read-parity test belongs in `desktop/tests/` or as a
`shannon-desktop` integration test that also calls the CLI's
`ProviderConfigStore` directly).

### 3.4 Schema-parity shortcut

Rows 1-4 implicitly assert schema parity (both surfaces must read/write
`ProviderModelConfig` v2 identically to agree on disk). An explicit
schema-parity test is redundant once rows 1-4 pass — if the surfaces
disagreed on the schema, every row would fail. Call this out in the test
comments rather than adding a separate test.

---

## 4. Acceptance

C3 is **done** when:

1. All six matrix rows pass in `just test` (nextest, process-isolated).
2. Row 5 (`read_view_cli_vs_desktop_match`) asserts the desktop's
   `ProviderReadSnapshot::to_providers_file()` equals the CLI's
   `ProviderConfigStore::config()` projection for ≥3 fixtures (empty,
   single-profile, multi-profile-with-active).
3. Row 6 (`mixed_surface_writers_do_not_lose_updates`) runs ≥8 writers
   split across the two surface patterns and asserts all land.
4. The real-binary smoke layer (follow-up) is tracked as a separate
   issue, not blocking C3 closure.

---

## 5. Estimate & slicing

| Slice | Rows | Effort | Deliverable |
|-------|------|--------|-------------|
| C3-a  | 1, 2, 3 | ~1.5d | Write-parity (add/remove/set-active) in-process |
| C3-b  | 4, 5    | ~1.5d | Update-field + read-view parity (row 5 in `shannon-desktop`) |
| C3-c  | 6       | ~0.5d | Mixed-surface stress (generalise S1-4) |
| C3-d  | smoke   | ~1d   | Real-binary CLI↔desktop smoke (slow lane) — optional follow-up |

C3-a + C3-b + C3-c = the ~3-5d estimate. C3-d is a stretch.

---

## 6. Risks & open questions

- **Credential store hermeticity.** Rows 1/4 touch
  `~/.shannon/credentials/<id>.json`. The tests must point the
  credential root at the `TempDir`, not the real `~/.shannon/`. Need to
  confirm `CredentialManager` accepts an override path (or inject one);
  if not, that's a small prerequisite task.
- **The desktop surface's `configure()` arms also rebuild the live
  `client_config`** (`rebuild_client_config_from_store`). C3's
  in-process model can skip that (it's not part of the on-disk parity
  contract), but the test should say so explicitly so a reader doesn't
  think the omission is a gap.
- **Real-binary smoke (C3-d)** needs a hermetic `HOME` + a built `shannon`
  binary. If the CI runner can't build the CLI cheaply, C3-d stays local/
  nightly only.

---

## 7. Next steps (post-review)

1. Confirm the credential-root override path (Risk 1).
2. Open C3-a PR (rows 1-3) extending the harness.
3. Open C3-b PR (rows 4-5, the latter in `shannon-desktop`).
4. Open C3-c PR (row 6).
5. File C3-d as a follow-up issue.

Related: [ADR-0009][adr9] (read facade — row 5 promotes its acceptance
to a cross-surface gate); [p2-2-s1-1-lock-design.md](./p2-2-s1-1-lock-design.md)
§7 (S1-1..S1-4 implementation results, the foundation this builds on).
