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
#   ./scripts/release.sh --check            # Dry-run: verify toolchain and targets without building
#   ./scripts/release.sh --sign             # GPG-sign the checksums file
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
DRY_RUN=false
SIGN=false

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
        --check)
            DRY_RUN=true
            shift
            ;;
        --sign)
            SIGN=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [--all] [--target <triple>] [--version <ver>] [--check] [--sign]"
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

# ── Check prerequisites ────────────────────────────────────────────────────

check_prerequisites() {
    echo "Checking prerequisites..."

    if ! command -v rustc &>/dev/null; then
        echo "ERROR: rustc not found. Install Rust 1.85+." >&2
        exit 1
    fi

    local rust_version
    rust_version=$(rustc --version | grep -oP '\d+\.\d+' | head -1)
    local major minor
    IFS='.' read -r major minor _ <<< "$rust_version"
    if [[ "$major" -lt 1 ]] || [[ "$major" -eq 1 && "$minor" -lt 85 ]]; then
        echo "ERROR: Rust ${rust_version} is too old. Need 1.85+." >&2
        exit 1
    fi
    echo "  Rust $(rustc --version)"

    if command -v cross &>/dev/null; then
        echo "  cross: $(cross --version 2>&1 | head -1)"
    else
        echo "  cross: not found (native builds only)"
    fi

    echo "  Version: ${VERSION}"
    echo ""
}

check_prerequisites

# ── Helper functions ───────────────────────────────────────────────────────

generate_checksums() {
    echo ""
    echo "Generating SHA256 checksums..."
    local checksum_file="${DIST_DIR}/shannon-code-${VERSION}-checksums.txt"
    : > "$checksum_file"

    for archive in "${DIST_DIR}"/*.tar.gz "${DIST_DIR}"/*.zip; do
        [[ -f "$archive" ]] || continue
        local basename
        basename=$(basename "$archive")
        local hash
        hash=$(sha256sum "$archive" | cut -d' ' -f1)
        echo "${hash}  ${basename}" >> "$checksum_file"
        echo "  ${hash}  ${basename}"
    done

    if [[ "$SIGN" == true ]]; then
        echo "Signing checksums..."
        gpg --detach-sign --armor "$checksum_file"
        echo "  Created ${checksum_file}.asc"
    fi

    echo "  Checksums written to ${checksum_file}"
}

smoke_test_binary() {
    local binary="$1"
    if [[ ! -x "$binary" ]]; then
        echo "  SKIP: ${binary} not executable"
        return 1
    fi

    local output
    output=$("$binary" --version 2>&1 || true)
    if echo "$output" | grep -q "$VERSION"; then
        echo "  OK: $(basename "$binary") reports version ${VERSION}"
    else
        echo "  WARN: $(basename "$binary") version output: ${output}"
    fi
}

build_target() {
    local target="$1"
    echo ""
    echo "═══ Building for ${target} ═══"

    # Check if target needs cross or native cargo
    local runner="cargo"
    if command -v cross &>/dev/null; then
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

    # Dry-run mode: just check, don't build
    if [[ "$DRY_RUN" == true ]]; then
        echo "  [DRY-RUN] Would build with: ${runner} build --release -p shannon-cli -p shannon-agent --target ${target}"
        return 0
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
            # Smoke test native binaries
            if [[ "$runner" == "cargo" ]]; then
                smoke_test_binary "${pkg_dir}/${bin}${ext}" || true
            fi
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

# Generate checksums (skip in dry-run)
if [[ "$DRY_RUN" == false ]]; then
    generate_checksums
fi

echo ""
echo "═══ Release complete ═══"
echo "Artifacts in ${DIST_DIR}/:"
ls -lh "${DIST_DIR}/"*.tar.gz "${DIST_DIR}/"*.zip "${DIST_DIR}/"*checksums* 2>/dev/null || echo "  (no archives found)"

if [[ "$DRY_RUN" == true ]]; then
    echo ""
    echo "[DRY-RUN] No binaries were built. Remove --check to perform the actual build."
fi
