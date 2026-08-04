# CI Gates

> Authoritative reference for the GitHub Actions gates that protect `main`
> and `dev`. If a job runs in `.github/workflows/ci.yml`, this document
> explains what it checks, whether it is required for merging, and how
> to reproduce it on a laptop.

## 1. Job catalogue

| Job | Workflow step | Required on `dev`? | Required on `main`? | What it catches |
| --- | --- | --- | --- | --- |
| `test` (Test) | `cargo build --workspace` + nextest | yes | yes | Regressions in the integration suite |
| `vscode` (VS Code Extension) | `npm run compile` + `npm test` | yes | yes | VS Code editor build breakage |
| `clippy` (Clippy) | `cargo clippy --workspace -- -D warnings` | yes | yes | New lint regressions, dead code, performance footguns |
| `fmt` (Format) | `cargo fmt --all -- --check` | yes | yes | Un-formatted code blocks merges |
| `audit` (Dependency Audit) | `cargo deny check --hide-inclusion-graph` | **yes (new)** | **yes (new)** | License violations, banned crates, restricted sources |
| `rustsec-audit` (RustSec Advisory Audit) | `cargo install cargo-audit --locked && cargo audit` | **yes (new)** | **yes (new)** | Public RustSec advisories not covered by cargo-deny's Open Rust feed |
| `doc` (Doc Build) | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | no (advisory) | no (advisory) | Broken intra-doc links, missing examples. Currently `continue-on-error: true` until pre-existing doc fixes land. |
| `metrics` | `scripts/gen-metrics.sh` | yes (downstream) | yes (downstream) | Aggregator: depends on `test`, `clippy`, `audit`, `rustsec-audit` |
| `cross-platform` (Cross-platform Check) | `cargo check --workspace --exclude shannon-desktop` | **yes (new)** | **yes (new)** | Target-conditional code drift on `ubuntu` + `macos` + `windows` |
| `check-linux-musl` (Linux musl Check) | `cargo check --workspace --exclude shannon-desktop --target x86_64-unknown-linux-musl` | no (`continue-on-error`) | no (`continue-on-error`) | Statically-linked Linux regression. Blocked upstream on musl-portable webkit/gtk. |
| `semver-check` (Semver Check) | `cargo semver-checks` vs `v0.5.5` baseline | no (`continue-on-error`) | no (`continue-on-error`) | API-breaking changes vs the last release tag. Pre-1.0 is advisory. |
| `insta` (Insta Snapshots) | nextest + reject `.snap.new` leftovers | **yes (new)** | **yes (new)** | Snapshot drift shipped without `cargo insta review` |

Adjacent workflows (not in `ci.yml`, but referenced by branch protection):

- `release.yml` — runs only on `v*` tags, not a PR gate.
- `vendor.yml` — runs on every push to `main`/`dev` and on manual
  dispatch. Produces the offline build artifact (`cargo-vendor-*.tar.gz`).
  See [§3 Using the vendor artifact](#3-using-the-vendor-artifact).

### How to add a new gate

1. Write the gate locally. Get it green on a clean clone:
   `cargo build --workspace && cargo test --workspace --no-run`.
2. Add the job to `ci.yml`. Match the existing layout (`checkout` →
   `dtolnay/rust-toolchain` pinned to `1.88` → optional `Swatinem/rust-cache` →
   the gate steps). Pin every `uses:` to a commit SHA. Update the table above.
3. If the gate should block merges: add the job name to the `Required
   status checks` list on the branch protection page for both `dev` and
   `main`. Push the protection change AFTER the workflow lands so the
   job exists when the rule is added.
4. If the gate requires a new OS or tool, add a runner matrix entry;
   otherwise do not add `continue-on-error` unless the upstream
   dependency makes the gate impossible (this is documented per-job
   for `check-linux-musl` and `semver-check`).
5. Update `scripts/ci-local.sh` so contributors can run the gate
   without GitHub Actions. See [§2 Local reproduction](#2-local-reproduction).

## 2. Local reproduction

All `ci.yml` jobs (sans OS matrix) are reachable from a laptop with the
toolchain pinned in `rust-toolchain.toml`:

```bash
# Format
cargo fmt --all -- --check

# Lint — matches the `clippy` job exactly
cargo clippy --workspace -- \
  -D warnings \
  -A unknown-lints \
  -A clippy::collapsible_if \
  -A clippy::collapsible_match \
  -A clippy::derivable_impls \
  -A clippy::manual_is_multiple_of \
  -A clippy::manual_checked_div \
  -A clippy::unwrap_used \
  -A clippy::unnecessary_sort_by

# Tests
cargo nextest run --workspace --config-file .config/nextest.toml

# Dependency audit (cargo-deny)
cargo deny check --hide-inclusion-graph

# RustSec audit (optional: `cargo install cargo-audit --locked`)
cargo audit --deny warnings

# Doc (currently advisory)
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

One-shot wrapper:

```bash
./scripts/ci-local.sh          # runs clippy + fmt + nextest + deny
./scripts/ci-local.sh --doc    # also runs the doc gate
```

The shell script is a thin wrapper that echoes a green/red summary
identical to the workflow's `needs: []` graph so a contributor can see
locally which job would block their PR.

`just dev` (`justfile` recipe) is a faster pre-push path that skips
doctests and release-mode linting. Use `just ci` for the full gate
stack including UI + gateway tests.

## 3. Using the vendor artifact

The `vendor` workflow produces a self-contained `.tar.gz` of every
crate the workspace depends on (per `Cargo.lock`). Operators in
air-gapped environments download the artifact, extract it, and point
cargo at it via a local `.cargo/config.toml`:

```bash
# 1. Download `cargo-vendor` artifact from the latest successful
#    `vendor` workflow run on the Actions tab.
gh run download <run-id> --name cargo-vendor --dir vendor-artifact

# 2. Extract into a known location.
mkdir -p ~/.cargo/vendor/shannon
tar -xzf vendor-artifact/shannon-vendor-*.tar.gz -C ~/.cargo/vendor/shannon

# 3. Tell cargo to use the vendored sources.
cat > .cargo/config.toml <<'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "/home/<user>/.cargo/vendor/shannon"
EOF

# 4. Build offline.
cargo build --workspace --offline --locked
```

Why CI itself does NOT vendor: vendoring hides which versions of
crates.io resolve to which advisories. The `audit` and `rustsec-audit`
jobs need direct access to crates.io so they pick up new advisories
the same way the local developer does. Vendor is purely an operator
escape hatch for build reproducibility.

## 4. Action SHA pinning

Every `uses:` reference in `ci.yml` and `vendor.yml` is pinned to a
full 40-char commit SHA. To upgrade:

```bash
# 1. Find the latest commit on the action's default branch.
git ls-remote https://github.com/actions/checkout refs/heads/main

# 2. Replace the SHA in the workflow file (search for the action name).
# 3. Validate.
actionlint .github/workflows/ci.yml
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"
```

Never use a moving tag like `@v4` for any action that can shell out
(see the actionlinter's `expression-injection` warnings).
