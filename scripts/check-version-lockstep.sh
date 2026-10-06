#!/usr/bin/env bash
# Version lockstep guard: the release-version sources must agree with the
# workspace version in the root Cargo.toml (`shannon --version` inherits it
# via `version.workspace = true`; `shannon-plugin-api` re-states its own).
#
# The same six sources are bumped by `just release-prep` and enforced by
# release.yml's prep guard at tag time — but nothing checked them between
# releases, and five of the six silently drifted to 0.11.0 while the
# workspace moved to 0.12.0 (found 2026-09-28), leaving `just release-prep`
# unable to self-heal (it diffs against the OLD workspace version). This
# guard fails fast on drift instead.
#
# Consumers:
#   - `just version-check`, wired into `just dev` (the pre-commit fast path)
#   - ci.yml's `facade-facts` hygiene job (plain shell step)
#
# `::error::` prefixes are GitHub Actions error annotations; they render as
# plain text locally and as inline PR annotations in CI.
set -euo pipefail
cd "$(dirname "$0")/.." # repo root

ws="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
if [ -z "$ws" ]; then
    echo "::error::cannot read the workspace version from Cargo.toml"
    exit 1
fi

fail=0
expect() { # <label> <actual-version>
    if [ "$2" != "$ws" ]; then
        echo "::error::$1 is '${2:-<missing>}', workspace version is '$ws' — run 'just release-prep $ws' or bump the file"
        fail=1
    fi
}

expect "desktop/Cargo.toml [package].version" \
    "$(grep -m1 '^version' desktop/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
expect "desktop/tauri.conf.json .version" \
    "$(grep -m1 '"version"' desktop/tauri.conf.json | sed -E 's/.*"version":[[:space:]]*"([^"]+)".*/\1/')"
expect "gateway/package.json .version" \
    "$(grep -m1 '"version"' gateway/package.json | sed -E 's/.*"version":[[:space:]]*"([^"]+)".*/\1/')"
expect "desktop/ui/package.json .version" \
    "$(grep -m1 '"version"' desktop/ui/package.json | sed -E 's/.*"version":[[:space:]]*"([^"]+)".*/\1/')"
expect "crates/shannon-plugin-api/Cargo.toml version" \
    "$(grep -m1 '^version' crates/shannon-plugin-api/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"

if [ "$fail" -ne 0 ]; then
    echo "version lockstep broken across release-version sources" >&2
    exit 1
fi
echo "version lockstep OK: all six sources at ${ws}"
