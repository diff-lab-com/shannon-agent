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
  `#[unstable_api]` or undocumented. New stable APIs may land here too.
- **Major** (0.x → 1.0): reserved for the stability lock-down — no
  breaking changes to `#[stable_api]` APIs without a deprecation cycle.

Until 1.0, treat every minor bump as potentially breaking for any API
not marked `#[stable_api]`.

## Stability tiers

Every `pub` item in a `shannon-*` crate falls into one of three tiers.
Until items are explicitly tagged, assume `unstable`.

| Tier | Marker | Stability promise |
|------|--------|-------------------|
| **Stable** | `#[stable_api(since = "...")]` attribute + doc entry | No breaking changes without deprecation cycle + minor bump. |
| **Unstable** | (default — no marker, or `#[unstable_api]`) | May break in any minor bump. Safe to depend on inside the workspace; external consumers should pin. |
| **Deprecated** | `#[deprecated]` attribute | Scheduled for removal. Migration note required in doc comment. |
| **Internal** | `#[doc(hidden)]` | Not part of the public API. May be removed without bump. |

The attribute macros live in `crates/shannon-stability-attr/`. The names
`stable_api` / `unstable_api` are used because Rust reserves `#[stable]`
and `#[unstable]` for the standard library (E0734).

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

CI runs `cargo-semver-checks` as an **advisory** step. Failures do not
block PRs.

Promotion to blocking requires:
1. Pin baseline to a git tag (e.g. `--baseline-rev v0.5.5`) — the
   default crates.io lookup hits an unrelated `shannon-cli` package
   published under someone else's name.
2. Exclude or publish crates not on crates.io (currently
   `shannon-agents`, `shannon-agent`, etc.).
3. Verify zero drift against the chosen baseline.

Until those land, failures are surfacing-only.

To run locally:

```bash
cargo install cargo-semver-checks --locked
cargo semver-checks --baseline-rev <git-tag>
```

## Deprecation cycle

When a `#[stable_api]` API needs to break:

1. Add `#[deprecated(since = "0.x.y", note = "Migration plan...")]` on
   the old API.
2. Ship the new API alongside it, marked `#[stable_api]`.
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
