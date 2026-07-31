# ADR 0007 — `/connect` Auto-Opens the Model Picker

**Status**: Accepted
**Date**: 2026-07-31
**Theme**: Fix the `/connect` flow so users can always choose a model
after connecting a provider, instead of silently landing on
`models_for_provider(p).first()`.
**Supersedes**: —
**Related**: ADR-0005 (Unified Provider/Model/Credential Management —
established the static catalog + models.dev overlay + LiteLLM pricing
stack this ADR builds on); ADR-0006 (Rejected assistant-ui migration —
this ADR is the lighter-touch alternative for the same underlying UX
gap).

---

## TL;DR

After `/connect <provider> <key>` succeeds, **automatically open the
model picker** on the freshly connected provider, pre-selected on the
default model. Concurrently spawn a **5-second background
models.dev refresh** so the picker shows any freshly discovered
models. Enter = use selected; Esc = keep the connect-time default
(already applied to `repl.state.model`).

Both paths are **non-breaking** — the existing default-set behavior at
phase 3 of `apply_connect` stays in place; the picker is an
**additional affordance**, not a replacement.

---

## Context

### The user pain

Today, `/connect minimax sk-…` lands the user on `MiniMax-M2.7` (the
first entry in the static `MODEL_CATALOG` for `LlmProvider::Minimax`)
with no chance to pick. To get `MiniMax-M3` the user has to:

1. Remember the id (or read the catalog source).
2. Run `/model minimax/MiniMax-M3` manually.

If `MiniMax-M3` isn't in the catalog at all, the user has to run
`/model refresh` first (opt-in, network-dependent) and *then* remember
to pick it. Most users don't — they accept `M2.7` and never know `M3`
exists.

### Why this matters

Shannon is competing against Claude Code / Codex CLI / OpenCode on
**agent capabilities**, not chat polish — but model discoverability is
table stakes. A user who finishes `/connect anthropic sk-…` and lands
on Claude Sonnet 4 (May 2025) when Claude Sonnet 4.6 is available
**right next to it** in the catalog will assume Shannon is stale. The
data may be correct; the UX hides it.

### Architectural pre-conditions already in place

ADR-0005 Phase D already shipped:

- `crates/shannon-core/src/model_registry/dynamic.rs` — models.dev
  overlay cached at `~/.shannon/cache/models-dev.json` (24h TTL),
  strict `merge_static_and_dynamic` (dedup by id, static wins).
- `crates/shannon-core/src/query_engine/litellm.rs` — LiteLLM
  community pricing overlay for the same cache directory.
- `ModelPickerWidget::new(Some(model_id))` — auto-positions on the
  provider tab where `model_id` lives, shows merged
  static + dynamic models.
- `handle_model_refresh` — existing `repl.runtime.block_on(dynamic::refresh_overlay_async(timeout))`
  pattern with `DEFAULT_FETCH_TIMEOUT`.

The fix is therefore **purely wiring**: invoke the existing picker from
`apply_connect`, and trigger the existing refresh asynchronously.

---

## Decision

### Behavior

| `/connect` invocation | After this ADR | Before this ADR |
|-----------------------|---------------|-----------------|
| `/connect` (no args) | Dashboard (unchanged) | Dashboard |
| `/connect <provider>` (no key, none stored) | Hint to inline form (unchanged) | Hint to inline form |
| `/connect <provider>` (key stored OR no-auth) | Open picker, background refresh | Silent default switch |
| `/connect <provider> <key>` | Open picker, background refresh | Silent default switch |
| `/connect <provider> <key>` + credential probe FAILS (auth) | Early return, no picker (unchanged) | Early return |
| `/connect <provider> <key>` + credential probe SOFT-WARNS | Open picker (unchanged) | Silent switch |

### Code change

`crates/shannon-ui/src/repl/commands/config.rs::apply_connect`
(line 601-711), after the existing credential-probe block and the
existing `"✓ Switched to {provider} — model: {model_id}"` message:

```rust
// 5. Spawn a non-blocking models.dev refresh so the picker (next step)
//    can show freshly discovered models. 5s timeout — if the user is
//    offline the static catalog remains authoritative and the picker
//    falls back to it transparently. Errors are swallowed by design.
//    Explicit `drop` to satisfy clippy::let_underscore_future.
std::mem::drop(repl.runtime.spawn(async {
    use std::time::Duration;
    let _ = shannon_core::model_registry::dynamic::refresh_overlay_async(
        Duration::from_secs(5),
    ).await;
}));

// 6. Open the model picker on the freshly connected provider.
let mut picker = crate::widgets::select::ModelPickerWidget::new(Some(&cp.model_id));
picker.set_entered_via_connect(true);
repl.state.model_picker = Some(picker);
```

`crates/shannon-ui/src/widgets/select.rs` — `ModelPickerWidget` gets
a new `entered_via_connect: bool` field, a public
`set_entered_via_connect(bool)` setter, and a `was_entered_via_connect()`
getter. The flag is **purely cosmetic** today — it doesn't change
Enter/Esc behavior, because the connect-time default has already been
written to `repl.state.model` by phase 3, so Esc naturally keeps it.
The flag reserves a stable signal for a future "Esc reverts to the
connect-time default" affordance if we want to decouple picker-confirm
from engine-switch.

### i18n

`locales/en.yml` + `locales/zh.yml`:

```yaml
connect:
  success: "✓ Connected to %{provider}"
  choose_model: "Pick a model below (Enter = default, ↑↓ = select, Esc = use default)."
  refresh_started: "Refreshing model catalog in the background…"
  refresh_success: "Catalog refreshed — %{count} models available."
  refresh_failed: "Offline — using cached catalog."
```

The five new keys are currently unused in `apply_connect` itself
(only `success` and `choose_model` are wire-up-ready, since the
background refresh prints no message today). They're staged for a
follow-up commit that surfaces the refresh outcome in chat. Keeping
them localized now avoids a "partially translated" surprise.

### Tests

Three new unit tests in `crates/shannon-ui/src/widgets/select.rs`:

1. `picker_entered_via_connect_defaults_to_false` — fresh picker
   starts with flag false (the `/model` legacy path keeps it that way).
2. `picker_entered_via_connect_setter_roundtrip` —
   `set_entered_via_connect(true/false)` toggles
   `was_entered_via_connect()` symmetrically.
3. `picker_entered_via_connect_survives_provider_navigation` —
   tabbing between providers/tiers does **not** clear the flag. A user
   who arrows across tabs after a connect still gets connect-time
   behavior.

`apply_connect` itself is **not** unit-tested here — it touches
`CredentialManager` (filesystem), `provider_config_store::save`
(filesystem), `query_engine.set_model_for_provider` + `pre_resolve_context`
+ `validate_credential` (network), so a clean unit test would require
significant test-double scaffolding better suited to an integration
test in `crates/shannon-cli/tests/`. The three picker-flag tests lock
in the surface that `apply_connect` consumes.

---

## Consequences

### Positive

- ✅ User is **always** offered a model choice after `/connect`,
  matching OpenCode's `/connect` UX and not lagging Claude Code's
  curated implicit defaults.
- ✅ Background 5-second models.dev refresh means the picker often
  shows the latest model on first display — not just after the user
  explicitly runs `/model refresh`.
- ✅ Zero static-catalog maintenance: we don't need to add `MiniMax-M3`
  or reorder any entries to fix the user pain. The picker will show
  whatever models.dev discovers.
- ✅ Fail-open: network failure during refresh is silent — the
  picker shows the static catalog as before. No user-visible error.
- ✅ Non-breaking: every existing test passes (1416/1416 nextest
  green); the picker is additive on top of the unchanged phase 3
  default-switch.
- ✅ i18n scaffolding in place for future refinement of the
  refresh-status messages.

### Negative

- 🟡 One extra UI step after every `/connect`. Users who genuinely
  want the legacy "do whatever, just connect" behavior now have to
  press Esc once. Acceptable: Esc is one keystroke and the picker is
  modal.
- 🟡 Background refresh is non-deterministic — on a slow network the
  picker may open *before* the fresh data arrives. Mitigated by the
  user being able to dismiss + re-open `/model` (which re-reads the
  overlay), but not a first-class experience.
- 🟡 The `entered_via_connect` flag is dead code today (no behavior
  branches on it). Could be deleted if the follow-up "Esc revert"
  affordance is never added. Worth carrying for now because the
  signal is cheap and the use case is foreseeable.

### Neutral

- 🟢 Five new locale keys ship unused. Acceptable — they're already
  paired across `en.yml` + `zh.yml` so a follow-up won't have to do
  parity work.
- 🟢 `apply_connect`'s `lines.join("\n")` now precedes a picker
  modal. The "✓ Switched to X — model: Y" line still prints so users
  who close the picker without reading it know what default was
  applied.

---

## Alternatives Considered

### A. Just add `MiniMax-M3` + reorder the static catalog

**Rejected**. Treats the symptom (one model is wrong) not the cause
(users never get a model choice). The next time a flagship ships
(`MiniMax-M4`, `Claude 4.7`, etc.), the same problem reappears.

### B. Make `/model refresh` automatic on every startup

**Rejected** as the primary fix because it (1) adds a hard
network requirement to startup and (2) doesn't address the *flow*
problem — even with fresh data, users still wouldn't know they could
pick. This ADR already opts into a one-time background refresh
*during* `/connect` which is a much narrower scope.

### C. Add a `is_default: bool` field to `ModelInfo`

**Deferred** to ADR-0006's recommended follow-up sprint. Doesn't
solve this ADR's problem either — it changes *which* model is the
default, not whether the user is *asked*.

### D. Switch to assistant-ui's runtime (ADR-0006 Option C)

**Already rejected** at a higher level in ADR-0006. The picker +
background-refresh pattern here is a tiny fraction of the
architectural change ADR-0006 declined to make.

### E. Auto-open picker only for providers with **no** static catalog

**Rejected**. Helps OpenRouter / Bedrock / Custom (where there is
literally no default) but leaves the 12+ static-catalog providers
silently switching. Doesn't fix the user's actual pain.

---

## Implementation References

### Code

- `crates/shannon-ui/src/repl/commands/config.rs` — `apply_connect`
  (line 601-711) — new phase 5 (spawn refresh) + phase 6 (open picker)
- `crates/shannon-ui/src/widgets/select.rs` — `ModelPickerWidget`
  + `entered_via_connect` field + `set_entered_via_connect` +
  `was_entered_via_connect` + 3 new tests
- `crates/shannon-core/src/model_registry/dynamic.rs` — reused
  `refresh_overlay_async(timeout)`; no changes
- `crates/shannon-ui/src/repl/input.rs:1933-2011` —
  `handle_model_picker_input` — unchanged, the existing Enter/Esc
  handling already does the right thing for our flow

### i18n

- `locales/en.yml` — new `connect:` namespace (5 keys)
- `locales/zh.yml` — same

### Tests

- `crates/shannon-ui/src/widgets/select.rs` — 3 new picker-flag tests
  in `mod tests`

### Related docs

- ADR-0005 §410 — models.dev as additive overlay
- ADR-0006 — Option D (incremental improvements, not library swap)
  motivated this ADR's scope

---

## Open Questions / Re-evaluation Triggers

This ADR can be revisited if:

1. **The picker becomes a real barrier.** If users complain that the
   auto-open picker slows down their connect flow, consider a
   `connect.silent_default` config knob (default `false`) to skip it
   for users who opt in.
2. **The `entered_via_connect` flag gains a real consumer.** If we
   implement "Esc reverts to the connect-time default", the flag
   earns its place; otherwise it can be deleted in a future cleanup
   PR.
3. **models.dev becomes authoritative enough that we can retire
   the static catalog.** That's a much larger ADR (the topic of
   ADR-0006's Option D), and orthogonal to this fix.
4. **`/provider <name>` should also auto-open the picker.** That's
   the natural next slice (apply_connect mirrors handle_provider at
   line 215-268); a follow-up PR can do the same wiring without
   needing a new ADR.

---

## Acceptance

- [x] `apply_connect` opens `ModelPickerWidget` after successful
      connect (or successful no-auth path)
- [x] Picker has `entered_via_connect = true`
- [x] Background `dynamic::refresh_overlay_async(5s)` is spawned
- [x] `en.yml` + `zh.yml` carry `connect.*` namespace (5 keys)
- [x] 3 new picker tests in `select.rs`; full shannon-ui nextest
      1416/1416 green
- [x] `cargo fmt --all -- --check` clean
- [x] `cargo clippy --workspace --lib --bins -- -D warnings` clean