#!/usr/bin/env bash
# Local cargo gate — mirrors what CI would run if the Gitea runner could
# reliably reach github.com (it can't, so CI is UI-only).
#
# Run before merging to main/dev. Exits non-zero on any failure.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> cargo fmt --check"
cargo fmt --all -- --check

echo "==> cargo clippy (-D warnings)"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> cargo test"
cargo test --workspace

if command -v cargo-deny >/dev/null 2>&1; then
  echo "==> cargo deny"
  cargo deny check --hide-inclusion-graph
else
  echo "==> cargo-deny not installed; skipping (install with 'cargo install cargo-deny')"
fi

echo "==> events.schema.json sync"
scripts/check-schema-sync.sh || [[ $? -eq 2 ]]

echo "==> UI: pnpm lint + test"
pnpm --dir ui lint
pnpm --dir ui test --run

echo "ALL OK"
