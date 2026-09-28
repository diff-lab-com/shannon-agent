#!/usr/bin/env bash
# Build the full workspace in release mode (product binaries: shannon CLI +
# shannon-desktop). For the per-product shortcuts see the justfile
# (build-code / build-desktop / build-gateway).
set -euo pipefail

cargo build --workspace --release
