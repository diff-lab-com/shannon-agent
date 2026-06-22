# Supply Chain Security

Shannon Desktop's release pipeline fetches dependencies from multiple
sources. This doc explains the trust model and hardening measures.

## Dependency sources

| Source | What | Trust basis |
|---|---|---|
| `static.rust-lang.org` | rustup-init binary, Rust toolchains | Official Rust Foundation CDN; SHA256 verification per release |
| `crates.io` (mirrored via `rsproxy.cn`) | Cargo crates | RustSec advisory DB scanned via `cargo audit` |
| `registry.npmjs.org` (mirrored via `npmmirror.com`) | npm packages | Advisory DB scanned via `pnpm audit` |
| `github.com/shannon-agent/shannon-code` | Engine source | Git SHA pin + ssh fetch; commits reviewed before bump |
| `gitea.com/actions/*` | CI actions | Gitea official mirror of github.com/actions/* |

## Hardening measures

### 1. Rustup pinned version + SHA256

`release.yml::RUSTUP_VERSION` pins rustup to a known version. The install
step downloads the platform-specific `rustup-init` binary AND its `.sha256`
file from `static.rust-lang.org/rustup/archive/{VERSION}/{TARGET}/`, then
verifies with `sha256sum -c` before execution. This replaces the
historical `curl ... | sh` pattern.

Bump `RUSTUP_VERSION` explicitly when upstream releases a security fix.

### 2. Cargo audit + pnpm audit gate

The `audit` job runs before all platform builds. It blocks the release
on:
- `cargo audit --deny warnings` — scans `Cargo.lock` against RustSec
- `pnpm audit --audit-level=high` — scans npm lockfile

If audit fails, all platform builds are cancelled (`needs: audit`).

### 3. Third-party mirror trust

`rsproxy.cn` and `npmmirror.com` are third-party mirrors chosen for
build-time velocity in China. The trade-off is:
- **Benefit**: ~50% faster builds (95 min → 48 min for Linux)
- **Risk**: Compromised mirror could inject malicious crate/npm versions

Mitigation: `cargo audit` and `pnpm audit` catch known-malicious versions
that have made it into advisory databases. Zero-day or novel malicious
versions are not detectable — this is the accepted residual risk.

If you don't need China build velocity, remove the mirror configuration
from `release.yml::Configure China mirrors` step to fall back to
official registries.

### 4. Engine SHA pinning

`Cargo.toml` pins `shannon-*` crates to an exact 40-char SHA via git
deps. `[patch]` redirects them to a sibling checkout at the same SHA.
The pinned SHA (`SHANNON_CODE_REV` in `release.yml`) is the single
source of truth — bumping requires verifying the new commit has been
reviewed in the shannon-code repo.

### 5. workflow_dispatch branch filter

Manual workflow triggers (`workflow_dispatch`) are restricted to
`branches: [main, dev]` — protected branches only. Prevents accidental
release from unreviewed feature branches.

## Incident response

If a dependency is found compromised:
1. Identify the source (Rust advisory / npm advisory / direct report)
2. If the affected version is in `Cargo.lock` or `pnpm-lock.yaml`,
   bump to the fixed version immediately
3. Tag a patch release (`v0.3.X+1`) — the updater will roll it out
4. Post-mortem: add an audit exception or hardening measure to prevent
   recurrence
