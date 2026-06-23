# API Stability Policy

Shannon is pre-1.0 software. This document describes how the team tracks
breaking changes across the `shannon-*` crates and what downstream
consumers (notably `shannon-desktop`) can rely on.

## Versioning

All workspace member crates share a single `version.workspace` setting
in the root `Cargo.toml`. Bumps happen in lockstep until the D1 crate
split completes; after that, each crate versions independently following
the same rules.

We follow Cargo semver:
- **Patch** (0.5.5 → 0.5.6): additive changes, bug fixes, performance.
  Existing public APIs continue to compile and behave identically.
- **Minor** (0.5.5 → 0.6.0): breaking changes to APIs marked
  `#[unstable]` or undocumented. New stable APIs may land here too.
- **Major** (0.x → 1.0): reserved for the stability lock-down — no
  breaking changes to `#[stable]` APIs without a deprecation cycle.

Until 1.0, treat every minor bump as potentially breaking for any API
not marked `#[stable]`.

## Stability tiers

Every `pub` item in a `shannon-*` crate falls into one of three tiers.
Until items are explicitly tagged, assume `unstable`.

| Tier | Marker | Stability promise |
|------|--------|-------------------|
| **Stable** | `#[stable]` attribute + doc entry | No breaking changes without deprecation cycle + minor bump. |
| **Unstable** | (default — no marker) | May break in any minor bump. Safe to depend on inside the workspace; external consumers should pin. |
| **Deprecated** | `#[deprecated]` attribute | Scheduled for removal. Migration note required in doc comment. |
| **Internal** | `#[doc(hidden)]` | Not part of the public API. May be removed without bump. |

`#[stable]` is a custom attribute — see `crates/shannon-codegen/` for
the macro definition (landing in a follow-up PR). Until that lands,
stability is documented per-module in this file.

## Currently stable surface (as of 0.5.5)

These are the APIs the desktop shell depends on without a pin bump:

- `shannon-types::events::*` — event payloads + `event_names` constants.
  Field names are the wire contract with the UI; changes require
  `EVENT_SCHEMA_VERSION` bump.
- `shannon-types::{EntityId, Timestamp, ShannonResult, ShannonError}` —
  foundational aliases, unlikely to shift.
- `shannon_core::query_engine::{QueryEngine, QueryEvent, QueryContext}` —
  the engine entry points. Variants may be added to `QueryEvent` (UI
  must ignore unknown variants), existing variants are stable.
- `shannon_core::api::{LlmClient, Message, MessageContent, ContentBlock}` —
  the LLM client surface.
- `shannon_core::state::StateManager` — session persistence trait.
- `shannon_skills::SkillRegistry` — skill discovery + loading.
- `shannon_mcp::McpProcessPool` — MCP server lifecycle.

Everything else is `unstable` until explicitly promoted.

## cargo-semver-checks

CI runs `cargo-semver-checks` against the latest published tag as an
**advisory** step. Failures do not block PRs; they are surfaced in the
PR comment for review. Promote a failure to a hard block only after the
affected API has been marked `#[stable]`.

To run locally:

```bash
cargo install cargo-semver-checks --locked
cargo semver-checks
```

## Deprecation cycle

When a `#[stable]` API needs to break:

1. Add `#[deprecated(since = "0.x.y", note = "Migration plan...")]` on
   the old API.
2. Ship the new API alongside it, marked `#[stable]`.
3. Wait one minor cycle.
4. Remove the deprecated API in the next minor bump.

The desktop shell must compile without warnings against the
intermediate release.

## Cross-repo coordination

The desktop shell pins a specific `shannon-code` git rev via its
`Cargo.toml` `[patch."ssh://..."]` block. When the engine ships a
breaking change:

1. Engine PR merged first (with the version bump).
2. Engine issue (referenced in the tracking issues) notes the break.
3. Desktop PR follows within 48 hours, bumping the pin + adapting call sites.

In the other direction, if the desktop shell needs an engine API
change, open an engine issue first describing the needed contract.
