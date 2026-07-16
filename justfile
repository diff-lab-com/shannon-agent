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

ci: fmt lint deny test
    @echo "✅ all gates green"

# ---------- Release helpers (Phase 6) ----------

# Verify a clean-clone desktop build (the Phase 2 KPI), runnable anytime.
kpi-clean-build:
    cargo build -p shannon-desktop
