# Shannon-Agent monorepo task runner.
# ACTIVATED IN PHASE 5 (see ../MIGRATION.md): copy to repo root.
# `just` discovers this at the repo root. Requires: cargo, pnpm, bun.
#
# Note: desktop/ui and gateway are INDEPENDENT TS packages (no pnpm workspace).
# Each is installed/built in its own directory — see `install` below.

default:
    @just --list

# ---------- Install (TS packages are independent — no `pnpm install -r`) ----------

install:
    cd desktop/ui && pnpm install
    cd gateway && pnpm install

# ---------- Products (each independently buildable) ----------

# shannon-code product: the CLI/TUI coding agent.
build-code:
    cargo build --release -p shannon-cli

# shannon-desktop product: Tauri desktop app (member `desktop`).
build-desktop:
    cargo build --release -p shannon-desktop

# shannon-gateway product: TS platform bridge, compiled to a standalone binary.
build-gateway:
    cd gateway && pnpm build:binary

# ---------- Protocol codegen (Phase A) ----------

# Regenerate gateway TS types + OpenAPI from crates/shannon-api-protocol.
gen-protocol:
    cargo run -p shannon-api-protocol --bin gen-ts
    cd gateway && pnpm typecheck

# ---------- Lint / fmt ----------

fmt:
    cargo fmt --all

# Note: clippy runs against the workspace library + bin targets only (matches
# the original shannon-code CI gate). Test targets are intentionally NOT
# linted here -- the upstream test code uses `unwrap()` extensively and was
# never subject to `clippy --all-targets` in the original justfile; re-linting
# it would block CI for pre-existing patterns the migration does not own.
lint:
    cargo clippy --workspace -- -D warnings
    cd desktop/ui && pnpm lint
    cd gateway && pnpm typecheck

# ---------- Test ----------

test-rust:
    cargo nextest run --workspace || cargo test --workspace -- --test-threads=1

test-ui:
    cd desktop/ui && pnpm test:ci

test-gateway:
    cd gateway && pnpm test

test: test-rust test-ui test-gateway

# ---------- Supply chain ----------

deny:
    cargo deny check

# ---------- Full CI gate ----------

ci: fmt lint deny gen-protocol test
    @echo "✅ all gates green"

# ---------- Release helpers (Phase 6) ----------

# Verify a clean-clone desktop build (the Phase 2 KPI), runnable anytime.
kpi-clean-build:
    cargo build -p shannon-desktop

# ---------- Release prep: bump every version source, commit, tag ----------
# Usage: just release-prep 0.7.0
#   then: git push && git push origin v0.7.0   (triggers release.yml)
# Bumps the 4 independent version sources so cargo-dist + tauri + gateway
# + `shannon --version` all agree with the tag:
#   1) Cargo.toml workspace.package.version  (crates with version.workspace=true inherit)
#   2) desktop/tauri.conf.json  "version"  (Tauri does NOT read the cargo workspace)
#   3) gateway/package.json        "version"
#   4) `shannon --version` display value is tied to the workspace version
#      automatically via clap::crate_version!() in shannon-cli (see task C).
release-prep version:
    #!/usr/bin/env bash
    set -euo pipefail
    # Derive the CURRENT workspace version — every release-version source that
    # tracks it is bumped from this value, so they stay in lockstep.
    OLD="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
    echo "release-prep: ${OLD} -> {{version}}"
    # 1) root Cargo.toml — BOTH workspace.package.version AND every
    #    [workspace.dependencies] internal path-dep pin. These pins MUST track
    #    the workspace version: a stale `version = "<OLD>"` makes the requirement
    #    `^<OLD>` fail to resolve against the newly-versioned crate (this broke
    #    v0.7.0-rc1: pins stayed 0.6.0 while the crate bumped to rc1).
    sed -i "s/version = \"${OLD}\"/version = \"{{version}}\"/g" Cargo.toml
    # 2) desktop has its OWN [package] version (NOT workspace-inherited)
    sed -i "s/^version = \"${OLD}\"/version = \"{{version}}\"/" desktop/Cargo.toml
    # 3) Tauri (separate hardcoded version; Tauri does not read the cargo workspace)
    sed -i 's/^    "version": ".*"/    "version": "{{version}}"/' desktop/tauri.conf.json
    # 4) gateway (independent TS package)
    sed -i 's/^  "version": ".*"/  "version": "{{version}}"/' gateway/package.json
    # `shannon --version` is tied to the workspace version automatically via
    # env!("CARGO_PKG_VERSION") in shannon-cli (version.workspace=true) — no sed.
    git add Cargo.toml desktop/Cargo.toml desktop/tauri.conf.json gateway/package.json
    git commit -m "chore(release): v{{version}}"
    git tag v{{version}}
    @echo "✅ tagged v{{version}} — run: git push && git push origin v{{version}}"
