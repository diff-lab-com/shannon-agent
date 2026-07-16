# ADR-0001: Product Positioning and CI Migration

**Date:** 2026-06-19
**Status:** Accepted
**Supersedes:** —
**Superseded by:** —

## Context

A dual-role audit (10-year EM + 10-year PM) of `shannon-desktop` and
`shannon-code` surfaced ambiguous product positioning as the root cause
behind several surface-level symptoms: feature sprawl (14 navigation
surfaces), generic onboarding that serves no specific audience, mixed
register in zh-CN copy (`您`/`你` inconsistency), and unclear scope for
CSP hardening, telemetry defaults, and engine packaging.

The engine distribution depends on a sibling-checkout patch
(`Cargo.toml [patch]` block pointing at `../shannon-code/crates/*`). This
makes standalone builds impossible and ties every developer machine and
CI run to a local checkout of `shannon-code` at the pinned rev.

CI runs on Gitea Actions via `.gitea/workflows/ci.yml` with
`runs-on: ubuntu-22.04`, but local `act_runner` simulation has been the
default verification path; remote triggering against the Gitea remote has
not been validated end-to-end.

## Decisions

### 1. Product positioning

- **`shannon-desktop`** is positioned as a general-purpose AI assistant
  for everyday users (knowledge workers, students, casual users) — **not**
  a developer tool. Onboarding, copy, default feature set, and telemetry
  defaults reflect this.
- **`shannon-code`** (sibling engine repo) remains a developer-facing code
  assistant. Its CLI ergonomics and agent model target engineering
  workflows.
- The two products share the engine (`shannon-core` and friends) but ship
  independently with distinct UX contracts.

**Consequences:**
- P2.2 feature reduction: 14 navigation surfaces → 6 primary surfaces;
  `OPC`, `Goals`, `QuickFix`, `Editor`, `Hooks` move behind a `devMode`
  toggle.
- P2.3 user-facing documentation (`docs/user/`) targets non-developers.
- P2.4 CSP hardening removes `'unsafe-inline'` from `script-src`; Markdown
  rendering routes through `DOMPurify`.
- P2.5 telemetry defaults to opt-in; no usage data leaves the device
  without explicit consent.

### 2. Engine distribution

**Status:** Pending — deferred to ADR-0002.

The current subpath patch stays in place for this sprint. Two long-term
paths were identified:

- **Option A:** Publish `shannon-*` crates to `crates.io`. Eliminates the
  sibling checkout. Requires public visibility of engine source.
- **Option B:** Monorepo merge — fold `shannon-code` into `shannon-desktop`
  as a workspace. Keeps everything private, increases repo size.

Decision deferred pending business input on engine source visibility.

### 3. CI triggering

- CI triggers natively on Gitea push/PR events via
  `.gitea/workflows/ci.yml`. Local `act_runner` simulation is no longer
  in the critical path; it remains a convenience for contributors.
- `rust-toolchain.toml` is the single source of truth for the Rust
  toolchain. CI uses `dtolnay/rust-toolchain@stable` without an explicit
  `toolchain:` override; the pinned file wins.
- Repo origin: `ssh://git@gitea.diff-lab.com:2222/bigdong89/shannon-desktop.git`

**Consequences:**
- Builds are reproducible across local, CI, and contributor machines
  without per-environment configuration.
- Cargo `repository` / `homepage` metadata updated to point at Gitea.

## Alternatives Considered

- **Positioning as dual-audience (dev + non-dev):** rejected — feature
  sprawl returns, onboarding dilutes, and the accessibility regressions
  documented in `docs/product-review/01-novice-user-review.md` recur.
- **Keep CI on local `act_runner` only:** rejected — cannot enforce
  pre-merge gating; PRs from external contributors cannot be validated.
- **Pin toolchain only via `Cargo.toml` `rust-version`:** rejected — this
  is a minimum-version floor, not an installation pin. Contributors on
  newer stable can still produce builds that fail on the floor.

## Follow-ups

- ADR-0002: Engine distribution strategy (crates.io vs monorepo) — blocked
  on business input.
- `.omc/plans/s2-roadmap.md`: full P0–P3 backlog derived from the audit.
- `rust-toolchain.toml`: toolchain pin (this sprint).
- `.gitea/workflows/ci.yml` cleanup: drop redundant `toolchain:` overrides
  (this sprint).
