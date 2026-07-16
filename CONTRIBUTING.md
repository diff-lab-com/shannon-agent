# Contributing to Shannon Agent

Thanks for your interest. This monorepo ships three products that share one Rust engine and one wire protocol.

## Development setup

Prerequisites: Rust 1.88+, pnpm 10+, bun latest. On Linux also: `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev patchelf`.

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

- Maintainer-driven only. Tag pattern: `vX.Y.Z` triggers release.yml + release-desktop.yml.
- Pre-release tags `vX.Y.Z-rc.N` are NOT supported by cargo-dist in this monorepo (workspace version must match tag). For dry-runs, manually verify locally first.

## Code of conduct

Be respectful. This project follows the Apache Code of Conduct.
