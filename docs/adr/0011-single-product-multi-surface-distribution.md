# ADR-0011: Single Product, Multiple Surfaces — Identity & Distribution Unification

**Date**: 2026-08-19
**Status**: Accepted
**Sprint**: continuous (post-v0.10.0)

## Context

Shannon ships one engine but has been *describing* itself as two products:
"shannon-code (CLI)" and "shannon-desktop". The architecture underneath
disagrees — it is already a single product in every load-bearing dimension:

| Dimension | Evidence |
|---|---|
| One engine, multiple hosts | `desktop/Cargo.toml` depends on engine crates via workspace path deps (in-process embed); `shannon serve` is a second host; `desktop/src/engine_discovery.rs` attaches to an existing `shannon serve` instance on `:33420` instead of spawning a duplicate |
| One state layer | `~/.shannon/` (providers.toml + keyring + sessions) shared by CLI and desktop (ADR-0005); `--resume <uuid>` works across surfaces |
| One version | Lockstep releases; `justfile release-prep` syncs all version sources from one tag |
| One release | `release.yml` builds CLI matrix + desktop (tauri-action) + gateway into a single draft release |

The split-model costs are not hypothetical — they are documented incidents:
the v0.7.0-rc1 version-drift release breakage (recorded in `justfile
release-prep` comments), the pre-July dual-release conflict on the same tag,
and install scripts pointing at stale asset names (see
`docs/RELEASE-INSTALL-PLAN.md` §1).

`docs/RELEASE-INSTALL-PLAN.md` (2026-07-18, user-ratified) already decided
the *mechanics*: one tag → one release, umbrella `shannon` command with
`serve`/`desktop`/`gateway` subcommands, `install.sh` installing everything.
That plan is ~80% implemented. What remains inconsistent is the **product
identity layer** (docs, naming, and the last mile of distribution: desktop
installers do not bundle the `shannon` CLI), which is what this ADR decides.

## Decision

Shannon is **one product: `shannon`**, delivered through four surfaces:

| Surface | Entry | Notes |
|---|---|---|
| Interactive terminal | `shannon` | ratatui TUI/REPL |
| Headless / CI / scripting | `shannon -p … --output-format json-stream` | NDJSON contract, exit codes |
| Engine daemon | `shannon serve` | HTTP/WS on `:33420`; desktop & mobile may attach |
| Desktop GUI | `shannon desktop` | Tauri app; surface name "Shannon Desktop" |

Rules:

1. **"Shannon Desktop" is a surface name, not a product name.** Product
   docs, README, website, and release notes speak of one product with four
   entry points. Historical `shannon-code` references in CHANGELOGs remain
   untouched (existing policy).
2. **Two executables, one distribution.** The desktop installers (NSIS /
   dmg / deb / AppImage) bundle the `shannon` CLI binary alongside the GUI
   app; `shannon` remains the single user-facing entry point. Standalone
   CLI channels (tarballs, `cargo install`, brew) continue to exist for
   headless/server users.
3. **Red lines** (binding for future changes):
   - **Never merge the binaries.** The `shannon` CLI must not link GUI
     libraries (tauri/webview) — enforced by a CI guard
     (`cargo tree -p shannon-cli -i tauri` must fail; on Linux `ldd` on the
     release binary must not resolve webkit2gtk). Rationale: headless
     servers have no display stack; webview event loops and ratatui
     alternate-screen lifecycles do not coexist; GUI crashes must not take
     down terminal sessions.
   - **Bundle, don't restructure.** The CLI is bundled as-is; the two
     processes stay independent (crash isolation, separate lifecycles).
   - **Release-cadence divergence, if it ever emerges, is solved by the D1
     crate split** (desktop pins *published* engine crate versions — the
     path already reserved in `docs/STABILITY.md`), **not** by splitting
     back into two products.

## Consequences

- **Positive**: single install funnel; session portability across surfaces
  becomes a product feature ("start in terminal, continue on desktop");
  version-sync incidents structurally prevented at the identity layer;
  docs stop paying the two-product explanation tax; the mobile/gateway
  story unifies around "attach to a running engine instance".
- **Negative**: desktop installer size grows by the CLI binary (~tens of
  MB); desktop bug fixes ride the same lockstep release as CLI (already
  true today — no regression, but now explicit); support intake needs a
  surface discriminator (`shannon doctor --json` gains a `surface` field).
- **Neutral**: the Tauri updater channel effectively ships CLI updates to
  GUI-first users; standalone CLI users keep using `shannon update` /
  package managers.

## Alternatives Considered

- **Single binary that morphs by argument (TUI ↔ GUI in one process)** —
  rejected: forces webkit2gtk linkage into headless deployments; main-thread
  webview loop conflicts with ratatui; couples crash domains.
- **True two-product split** (desktop as independent release with versioned
  engine deps) — rejected as premature: requires stable public engine API
  discipline and dual release pipelines, and destroys the cross-surface
  session story. Kept open as the D1 escape hatch for cadence, not identity.
- **Status quo (two products in naming, one in mechanics)** — rejected:
  internally inconsistent; every documented incident above came from this
  half-state.

## Implementation References

- `docs/RELEASE-INSTALL-PLAN.md` — prior, ratified decision this ADR completes
- `crates/shannon-cli/src/main.rs` — umbrella subcommands (`serve`,
  `desktop`, `gateway`, `update`, `doctor`), `run_desktop_command` (bootstrap
  rework target)
- `desktop/src/engine_discovery.rs` — attach-to-existing-engine behavior
- `.github/workflows/release.yml` — single-release pipeline (bundle point)
- `desktop/tauri.conf.json` — bundle config (add CLI via
  `bundle.externalBin` / `resources`)
- `docs/plans/single-product-phase-b-distribution.md` — Phase B execution
  checklist (the concrete changes implementing rule 2)

## Open Questions

- Final bundling technique per platform (Tauri `externalBin` vs
  `resources`) — resolved during Phase B implementation.
- In-app "Install `shannon` command to PATH" action (VS Code pattern) vs
  relying on installer scripts on macOS (dmg cannot modify PATH).
- Whether/when to enable `createUpdaterArtifacts` for the self-hosted
  updater now that installers carry the CLI.
- Homebrew cask / Scoop / Winget channel priority (defer until asset
  download counts justify; see Phase B §0).
