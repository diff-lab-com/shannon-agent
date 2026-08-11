# ADR 0008 — Provider/Model Command Architecture Remediation

**Status**: Proposed
**Date**: 2026-08-01
**Theme**: Collapse the duplication and divergent code paths behind
`/connect`, `/model`, and `/provider` into a small set of single-source
contracts, and fix the architectural root causes that produced the
StatusCard tier bug.
**Supersedes**: —
**Related**: ADR-0005 (Unified Provider/Model/Credential Management —
established `providers.toml` + credential store + tier overlay this ADR
consolidates); ADR-0007 (`/connect` auto-opens the picker — this ADR
keeps that behavior and hardens the wiring underneath it).

---

## TL;DR

A senior review of the `/connect`, `/model`, and `/provider` command
surfaces found **three confirmed user-visible bugs** (StatusCard tier
always renders `?`; first-screen card never reflects post-launch
switches; three divergent tier-labeling systems) whose root cause is
**duplicated, drifted code**, plus a longer tail of UX and maintenance
issues recorded in the companion plan
([`docs/plans/provider-model-command-remediation.md`](../plans/provider-model-command-remediation.md)).

This ADR records the four **architectural** decisions that the plan
hangs on — the ones that are cross-crate, have plausible alternatives,
and are expensive to reverse once call sites depend on them. Bug fixes
and routine refactors are **not** ADR'd here; they live in the plan.

The four decisions:

1. **Single source of truth for provider identity and tier labels.**
2. **One mutation path** (`apply_model_selection`) for every
   provider/model/tier switch.
3. **REPL and CLI converge** on one provider-configuration service
   writing `providers.toml`.
4. **`/connect` hot-reloads the credential** into the running engine.

---

## Context

### Symptoms (confirmed bugs — fixed by the plan, caused by decisions 1–2)

1. **StatusCard tier is always `?`.** `ChatWidget::set_active` is called
   with `None` for the tier argument at both call sites
   (`crates/shannon-ui/src/repl/mod.rs:391` and `:1384` — the comment
   reads `// tier label resolved in Task 14`; Task 14 never landed).
   `active_tier` is never written elsewhere, so
   `widgets/status_card.rs:81,146` renders `Tier: [?]` forever.
2. **First-screen StatusCard never updates after launch.** `set_active`
   is only called at REPL init and resume. `handle_model`,
   `handle_provider`, `apply_connect`, and `handle_model_tier` all mutate
   `repl.state.model` / `selected_provider` but never call `set_active`,
   so the welcome card shows the launch-time snapshot until the first
   chat message hides it.
3. **Three divergent tier-label systems.** The StatusBar pill uses a
   substring heuristic (`widgets/status_bar.rs:36-54` — `contains("haiku")`
   etc.); the StatusCard uses the always-`None` `active_tier`; and
   `/model --tier` uses the authoritative
   `model_registry::resolve_tier` (`crates/shannon-core/src/model_registry.rs:1331`)
   backed by `ModelCapabilities`. They can disagree (e.g. an `o3-mini`
   model is labeled `fast` by the substring heuristic).

### Root causes

- **Provider identity is duplicated.** `parse_provider_name`
  (`crates/shannon-ui/src/repl/commands/config.rs:153-186`) is a 25-arm
  hand-maintained `match` that re-encodes knowledge already living in
  `LlmProvider` (`Display`), `provider_resolver::llm_provider_id`, and
  the catalog. Adding a provider currently means editing 5+ sites.
- **The switch dance is duplicated 4×.** `handle_model:60-99`,
  `handle_provider:213-265`, `apply_connect:651-667`, and
  `handle_model_tier:1765-1787` each do the same sequence: set state →
  `set_model_for_provider` → `catch_unwind(pre_resolve_context)` →
  resolve context window → `save_preferences`. Drift is inevitable
  (symptom #2 above is direct evidence — none of them remember to call
  `set_active`).
- **Two parallel provider-configuration implementations.** REPL
  `/connect` goes through `provider_resolver::build_connect_profile`
  (`provider_resolver.rs:390`) → credential + profile write. CLI
  `shannon providers add` (`crates/shannon-cli/src/commands_providers.rs`,
  1181 lines) goes through a `ProviderKind` upsert writing the **same**
  `providers.toml` with a different shape. They can drift on field
  semantics.
- **`/connect` cannot apply a credential to the running session.**
  `apply_connect` ends with `"restart shannon to apply the new credential"`
  (`config.rs:706`); the running client keeps its startup credential
  (`config.rs:651-653` comment). The probe even verifies the new key
  works, then tells the user to restart — a contradictory first-run
  experience.

### Why now

ADR-0007 (2026-07-31) just made `/connect` land the user in the model
picker — increasing the prominence of the first-screen card and the
switching flow. The bugs above are now more visible. Fixing them
piecemeal (patch each of the 4 switch sites, patch each tier surface)
would amplify the existing duplication; this ADR consolidates first.

---

## Decision

### Decision 1 — Single source of truth for provider identity and tier labels

**Provider identity.** `LlmProvider` (in `shannon-engine::api`) gains a
canonical `from_slug(&str) -> Option<LlmProvider>` plus an alias table
(`claude → Anthropic`, `gpt → OpenAI`, `glm → Zhipu`, …) that is the
**only** alias map in the codebase. `parse_provider_name`
(`config.rs:153-186`) becomes a thin wrapper over it. The CLI's
`parse_kind` (`commands_providers.rs:60`) and any other provider-string
parsers converge on the same source.

**Tier labels.** A single function lives in `shannon-core::model_registry`:

```rust
/// Authoritative tier label for a model, derived from the catalog's
/// ModelCapabilities / ModelTier — never a substring heuristic.
pub fn tier_label_for(model_id: &str, provider: &LlmProvider) -> Option<TierLabel>;
```

`TierLabel` is `Fast | Standard | Pro` (reuse `TierName` sans `Auto`).
The StatusBar pill (`status_bar.rs:36-54` substring heuristic), the
StatusCard tier (`active_tier`), and any other surface call this one
function. `set_active`'s `tier` argument is populated by it.

### Decision 2 — One mutation path: `apply_model_selection`

A single function in `shannon-ui::repl::commands` becomes the only way
the REPL mutates the active provider/model/tier:

```rust
/// The single mutation path. Called by handle_model, handle_provider,
/// apply_connect, handle_model_tier.
///
/// - Updates repl.state (model, selected_provider, context_window).
/// - Syncs the query engine (set_model_for_provider + pre_resolve_context).
/// - Refreshes the chat widget (set_active) so the first-screen card
///   reflects the switch — closes symptom #2.
/// - Persists preferences (and, when `persist_tier`, providers.toml).
fn apply_model_selection(
    repl: &mut Repl,
    provider: LlmProvider,
    model_id: String,
    tier: Option<TierName>,      // None = infer via tier_label_for
    persist_tier: bool,
) -> Result<()>;
```

The four existing call sites collapse onto it. The `catch_unwind` around
`pre_resolve_context` lives in **one** place and is at minimum logged
(`tracing::error!`) rather than silently swallowed (addresses the
4-site silent-panic issue — `config.rs:85,233,658,1773`).

### Decision 3 — One provider-configuration service

`shannon-core` exposes a `ProviderConfigService` that owns the contract
for writing `~/.shannon/providers.toml` and binding a credential:

```rust
impl ProviderConfigService {
    /// Upsert a provider slot + bind a credential (Store | EnvRef | …).
    /// Single entry for both REPL /connect and CLI `providers add`.
    pub fn connect(&mut self, provider: LlmProvider, kind: ProviderKind,
                   credential: CredentialRef, base_url: Option<String>,
                   tiers: ProviderTiers) -> Result<()>;
    pub fn disconnect(&mut self, provider: &LlmProvider) -> Result<()>;
    pub fn set_tier(&mut self, provider: &LlmProvider, tier: TierName, id: &str) -> Result<()>;
    pub fn connected_slugs(&self) -> HashSet<String>; // replaces the two copies
}
```

`build_connect_profile` (REPL) and the CLI's `run_providers_add`
(`commands_providers.rs:510`) both become thin callers. The duplicated
`connected_provider_slugs()` (`config.rs:478` and inline at
`chat.rs:755`) is replaced by `connected_slugs()`.

### Decision 4 — `/connect` hot-reloads the credential

The query engine gains:

```rust
/// Replace the credential on the live client without a restart.
/// Used by /connect so the just-validated key takes effect immediately.
pub fn reload_credential(&self, service: &str) -> Result<()>;
```

`apply_connect` calls it after the credential probe succeeds, and the
`"restart shannon to apply"` message (`config.rs:706`) is removed.
`ProviderConfigService::connect` (Decision 3) is the single caller.

---

## Consequences

### Positive

- ✅ Symptoms #1–#3 disappear as direct consequences of Decisions 1–2
  (one tier function populates the card; one mutation path calls
  `set_active`).
- ✅ Adding a provider becomes a 1- to 2-site change instead of 5+.
- ✅ The four switch sites can no longer drift; `catch_unwind` panic
  swallowers collapse to one logged site.
- ✅ REPL and CLI write `providers.toml` through the same contract —
  no more "two shapes, one file".
- ✅ `/connect` matches competitor UX (Claude Code / Codex apply
  immediately).

### Negative

- 🟡 Decision 1 changes a public surface (`LlmProvider`). The alias
  table must cover every existing `parse_provider_name` arm or
  callers regress — needs an exhaustive test lift from the current
  arms before deletion.
- 🟡 Decision 3 is the largest blast radius: two mature code paths
  (REPL `apply_connect`, CLI `run_providers_add`) must be reworked to
  call the service. Regression risk on `providers.toml` round-trips;
  mitigated by the existing `ProviderConfigStore` test suite.
- 🟡 Decision 4 adds an engine method that mutates a live client —
  if a query is in flight, the swap needs defined ordering (queue or
  reject). Adds a small concurrency consideration.
- 🟡 Tier inference (`tier_label_for`) for dynamic/custom models
  outside the catalog returns `None`; the StatusBar pill must keep a
  sensible fallback (it does today via substring — keep as last resort).

### Neutral

- 🟢 The substring `tier_label_for` in `status_bar.rs` is deleted once
  the real one is wired (net deletion).
- 🟢 `parse_provider_name` shrinks to a few lines.

---

## Alternatives Considered

### A. Patch each symptom in place

Fix the three bugs by editing `set_active`'s callers and the substring
heuristic. **Rejected** as the primary approach: it leaves 4 switch
copies and 2 config paths intact, so the same class of bug recurs.
(The plan still ships the symptom fixes — but via Decisions 1–2, not
patches.)

### B. Push `apply_model_selection` into the engine (Decision 2 alt)

Move the mutation path into `QueryEngine` rather than the REPL command
layer. **Rejected**: the function must also update `ChatWidget` and
`save_preferences`, both UI-side concerns. Putting it in the engine
would invert the dependency (`shannon-core → shannon-ui`). Keep it in
the command layer; the engine stays a pure callee.

### C. Keep two config paths, just share a validator (Decision 3 alt)

Leave REPL and CLI implementations separate but extract a shared
`validate_provider_slot`. **Rejected**: the divergence is in the
*write shape*, not just validation. A shared validator would not
prevent the two paths from producing different `providers.toml`
structures. Only a shared write service does.

### D. Reconnect the client instead of hot-reloading (Decision 4 alt)

On `/connect`, tear down and rebuild the `LlmClient` from the new
config. **Rejected**: heavier (reconnect cost, in-flight query
disposition) than swapping the credential handle. Revisit if
`reload_credential` proves insufficient for some provider (e.g. base-URL
change, which may indeed need a rebuild — see Open Questions).

---

## Implementation References

### Code (current state — what the plan changes)

- `crates/shannon-ui/src/repl/commands/config.rs` — the four switch
  sites (60-99, 213-265, 651-667, 1765-1787), `parse_provider_name`
  (153-186), `connected_provider_slugs` (478), `apply_connect`
  (601-733), the `starts_with("--tier")` dispatch (38-49).
- `crates/shannon-ui/src/repl/mod.rs:391,1384` — the two `set_active`
  call sites passing `None` tier.
- `crates/shannon-ui/src/widgets/status_bar.rs:36-54` — substring
  `tier_label_for` (to be replaced by Decision 1's function).
- `crates/shannon-ui/src/widgets/chat.rs:742,755` — raw `MODEL_CATALOG`
  + inline `connected` duplicate.
- `crates/shannon-core/src/model_registry.rs:1331` — authoritative
  `resolve_tier` (the basis for Decision 1's `tier_label_for`).
- `crates/shannon-core/src/provider_resolver.rs:390` —
  `build_connect_profile` (becomes a caller of the new service).
- `crates/shannon-cli/src/commands_providers.rs:510` —
  `run_providers_add` (becomes a caller of the new service).

### Companion plan

- [`docs/plans/provider-model-command-remediation.md`](../plans/provider-model-command-remediation.md)
  — the full P0→P3 task breakdown, evidence anchors, sizing, and
  per-task acceptance criteria. The bug-fix and routine-refactor items
  (status vocabulary, `/disconnect`, i18n, `/model refresh`
  backgrounding, file splits, caching, `/profile` naming) live there,
  not in this ADR.

### Related docs

- ADR-0005 — the unified stack this ADR consolidates.
- ADR-0007 — `/connect` picker behavior (preserved).

---

## Open Questions / Re-evaluation Triggers

1. **Base-URL changes at runtime.** Decision 4's `reload_credential`
   swaps only the key. If `/connect` should also honor a new base URL
   for an existing provider, the engine may need a full client rebuild
   (Alternative D). Decide when the first such case appears.
2. **Alias table exhaustiveness.** Decision 1's `from_slug` must cover
   every arm in today's `parse_provider_name` + `parse_kind`. The
   migration test must be exhaustive before deletion (see plan P2-3).
3. **Desktop parity.** The desktop crate (`desktop/`) also configures
   providers. Decision 3's service should be the desktop's write path
   too; defer to ADR-0005 Phase 2 (desktop re-platforming) if the
   desktop isn't ready.
4. **`/profile` naming collision** (permission profile vs providers.toml
   "default" profile vs preset alias) is a UX/wording fix, handled in
   the plan — but if the rename turns out to need a config-key
   migration, promote it to a follow-up ADR.

---

## Acceptance

This ADR is **Proposed** until the plan's Phase 1 (Decisions 1–2) lands
and the three confirmed bugs are verified fixed. Acceptance checklist
(checked as the plan lands):

- [ ] `LlmProvider::from_slug` + alias table is the sole provider parser.
- [ ] `model_registry::tier_label_for` drives StatusBar, StatusCard, and
      `/model --tier` labeling.
- [ ] `apply_model_selection` is the sole switch path; all four call
      sites use it.
- [ ] StatusCard renders a real tier and refreshes on every switch.
- [ ] `ProviderConfigService` is the sole writer of `providers.toml`
      for both REPL and CLI.
- [ ] `/connect` applies the credential without a restart message.
- [ ] `just dev` (clippy `-D warnings` + fmt check) and `just test`
      green.
