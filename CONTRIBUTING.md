# Contributing to Shannon Agent

Thanks for your interest. This monorepo ships three products that share one Rust engine and one wire protocol.

## Development setup

Prerequisites: Rust 1.88+, pnpm 10+, bun latest. On Linux also: `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libgbm-dev libdrm-dev libpipewire-0.3-dev patchelf`.

The three capture-related packages (`libgbm-dev libdrm-dev libpipewire-0.3-dev`) only affect compiling the `shannon-desktop` crate (via xcap → pipewire → libspa) and the `shannon-tools --features computer-use` / libei legs — the rest of the workspace builds and tests without them. Verified matrix:

- Ubuntu 24.04: works (PipeWire 1.0.x headers — same as CI).
- Ubuntu 22.04: does not work. The distro's `libspa-0.2-dev 0.3.48` is too old; the build fails with seven `libspa` errors (E0425 `spa_meta_region_is_valid`/`spa_meta_first` missing, E0560/E0609 on `spa_video_info_raw.flags`, E0308). No version floor is claimed beyond these two data points.

```bash
git clone https://github.com/diff-lab-com/shannon-agent.git
cd shannon-agent
just install
just ci
```

## Branch strategy

- `main` is protected: requires CI pass + 1 approval + linear history. Direct push blocked.
- `dev` is the integration branch. Open PRs against `dev` first.
- After review on `dev`, changes get fast-forwarded to `main` via PR.

## Commit & PR

- One logical change per commit.
- Commit subject ≤ 72 chars, imperative mood ("add X", not "added X" or "adds X").
- PR description must include: what changed, why, how to test, any breaking changes.
- PR title = commit subject.

## Testing

- Run `just ci` before pushing.
- Add `#[serial]` to any new Rust test that mutates shared state (env vars, ~/.shannon, /tmp).
- For TS, tests live next to source as `*.test.ts`. Use `pnpm test` per package.

## Releases

- Maintainer-driven only. Pushing a `vX.Y.Z` tag triggers the single `.github/workflows/release.yml` orchestrator, which produces exactly one GitHub Release containing all three products: the `shannon` CLI (per-target `cargo build` archives + sha256), the desktop app (Tauri, via tauri-action), and `shannon-gateway` (Bun compile). A version-guard job fails fast if the tag doesn't match the manifests' versions.
- Pre-release tags (`vX.Y.Z-rc.N`, `vX.Y.Z-beta.N`) are supported: the guard compares the core version (tag minus the pre-release suffix) against the manifests. There is no dry-run workflow — verify a release build locally before tagging.
- Distribution today: GitHub Releases (CLI archives + Tauri bundles + gateway binaries, all covered by `SHA256SUMS` and build-provenance attestations) plus the `install.sh`/`install.ps1` bootstrap scripts. Third-party channels (Homebrew/Winget/Scoop/AUR) are **not published** — placeholder manifests lived in `packaging/` until they were removed in 2026-09; re-introduce a channel only together with the CI automation that fills in real versions and checksums. cargo-dist is no longer used anywhere in the release path.

## Code of conduct

Be respectful. This project follows the Apache Code of Conduct.
