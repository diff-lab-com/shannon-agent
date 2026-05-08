#!/usr/bin/env bash
# Cross-platform release script for Shannon Code
#
# Builds release binaries for Linux (x86_64, aarch64), macOS (x86_64, aarch64),
# and Windows (x86_64). Produces tar.gz / zip archives in target/dist/.
#
# Usage:
#   ./scripts/release.sh                    # Build for current platform
#   ./scripts/release.sh --all              # Build for all platforms (needs cross-rs or docker)
#   ./scripts/release.sh --target <triple>  # Build for a specific target
#   ./scripts/release.sh --version 0.2.0    # Override version string
#
# Prerequisites:
#   - Rust toolchain (1.85+)
#   - For cross-compilation: `cargo install cross` or appropriate toolchains
#
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

VERSION=""
TARGETS=()
BINARIES=(shannon shannon-agent)
DIST_DIR="target/dist"

# ── Parse arguments ────────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --all)
            TARGETS=(x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
                     x86_64-apple-darwin aarch64-apple-darwin \
                     x86_64-pc-windows-msvc)
            shift
            ;;
        --target)
            shift
            TARGETS+=("$1")
            shift
            ;;
        --version)
            shift
            VERSION="$1"
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [--all] [--target <triple>] [--version <ver>]"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

# ── Determine version ──────────────────────────────────────────────────────

if [[ -z "$VERSION" ]]; then
    VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
fi
echo "Building Shannon Code v${VERSION}"

# ── Helper functions ───────────────────────────────────────────────────────

build_target() {
    local target="$1"
    echo ""
    echo "═══ Building for ${target} ═══"

    # Check if target needs cross or native cargo
    local runner="cargo"
    if command -v cross &>/dev/null; then
        # Use cross for non-native targets that need a different linker/sysroot
        local host_triple
        host_triple=$(rustc -vV | grep host | cut -d' ' -f2)
        if [[ "$target" != "$host_triple" ]]; then
            runner="cross"
        fi
    fi

    # Install target if not present (for native cross-compilation)
    if [[ "$runner" == "cargo" ]]; then
        rustup target list --installed | grep -q "$target" || {
            echo "Installing target ${target}..."
            rustup target add "$target" 2>/dev/null || true
        }
    fi

    # Build
    echo "Building with ${runner}..."
    "$runner" build --release -p shannon-cli -p shannon-agent --target "$target" 2>&1

    # Package
    local ext=""
    local archive_ext="tar.gz"
    local target_dir="target/${target}/release"
    local pkg_name="shannon-code-${VERSION}-${target}"

    if [[ "$target" == *"-windows-"* ]]; then
        ext=".exe"
        archive_ext="zip"
    fi

    local pkg_dir="${DIST_DIR}/${pkg_name}"
    mkdir -p "$pkg_dir"

    # Copy binaries
    for bin in "${BINARIES[@]}"; do
        local src="${target_dir}/${bin}${ext}"
        if [[ -f "$src" ]]; then
            cp "$src" "$pkg_dir/"
            echo "  Copied ${bin}${ext}"
        else
            echo "  WARNING: ${src} not found"
        fi
    done

    # Copy license and readme
    [[ -f LICENSE ]] && cp LICENSE "$pkg_dir/"
    [[ -f README.md ]] && cp README.md "$pkg_dir/"

    # Create archive
    mkdir -p "$DIST_DIR"
    if [[ "$archive_ext" == "zip" ]]; then
        (cd "$DIST_DIR" && zip -r "${pkg_name}.zip" "${pkg_name}/")
        echo "  Created ${pkg_name}.zip"
    else
        (cd "$DIST_DIR" && tar czf "${pkg_name}.tar.gz" "${pkg_name}/")
        echo "  Created ${pkg_name}.tar.gz"
    fi

    # Cleanup staging dir
    rm -rf "$pkg_dir"
}

# ── Main ────────────────────────────────────────────────────────────────────

mkdir -p "$DIST_DIR"

if [[ ${#TARGETS[@]} -eq 0 ]]; then
    # Build for current host only
    HOST_TARGET=$(rustc -vV | grep host | cut -d' ' -f2)
    echo "No target specified, building for host: ${HOST_TARGET}"
    build_target "$HOST_TARGET"
else
    for t in "${TARGETS[@]}"; do
        build_target "$t"
    done
fi

echo ""
echo "═══ Release complete ═══"
echo "Artifacts in ${DIST_DIR}/:"
ls -lh "${DIST_DIR}/"*.tar.gz "${DIST_DIR}/"*.zip 2>/dev/null || echo "  (no archives found)"
